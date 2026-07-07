//! The flexible local-model adapter: the substrate the local analysis suite
//! ([`crate::config`] §Local Analysis Suite, `docs/local-models.md`) builds on.
//!
//! The cloud agents pick a model from a closed enum with hard-coded provider
//! endpoints ([`crate::model_agent::AgentModel`]). The local suite uses *this*
//! flexible adapter instead: a call is parameterized by `{ endpoint, model_id,
//! messages, tools, format_schema, options }`, so a whole roster of models behind
//! one Ollama daemon is addressed by id without enumerating each as a compile-time
//! variant. The `AgentModel` enum stays untouched and the roster changes through
//! configuration.
//!
//! Like the cloud adapters, the HTTP call is synchronous (`reqwest::blocking`) so
//! the per-stage boundary stays sync; the blocking work is offloaded via
//! `spawn_blocking` at the Tauri-command seam (the `test_local_daemon` command in
//! `lib.rs`, mirroring `connection_test`). Schema-constrained output rides Ollama's
//! native `/api/chat` `format` parameter (not the `/v1/` OpenAI-compatible path,
//! which advertises only JSON mode) — the one place this diverges from a plain
//! OpenAI client. Token + reasoning streaming rides the existing `progress` seam,
//! so a local job streams into the run tracker exactly as a report run does; the
//! native `/api/chat` stream is newline-delimited JSON (not SSE), so it carries its
//! own decoder rather than reusing the cloud SSE one.
//!
//! This module is a *primitive* — a provider client plus daemon supervision and the
//! local-suite gate. It deliberately does not implement the report's `MainAgent` /
//! `AnalystAgent` traits (those carry report-specific I/O); the per-feature stages
//! wrap this client and hold the pure-stage boundary themselves.

use std::io::BufRead;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{self, AppConfig, ValidationReport, WarningCategory, WarningKind};
use crate::progress::RunContext;

/// Native Ollama endpoint paths, joined onto the configured daemon base.
const CHAT_PATH: &str = "/api/chat";
const TAGS_PATH: &str = "/api/tags";

/// Backstop per-request timeout. Local reasoning (the 122B in thinking mode) can be
/// slow, so this is generous; it exists only to cap a stuck daemon, not to bound a
/// healthy generation. The supervision probes (`/api/tags`) resolve far inside it.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// Coalesce streamed fragments into a few-hundred progress events rather than one
/// per token (mirrors the cloud streaming path's flush cadence).
const TOKEN_FLUSH_CHARS: usize = 24;

/// Reduce a configured endpoint to the daemon **origin** the `/api/...` paths join
/// onto. The Ollama docs present the API base *with* the `/api` segment
/// (`http://localhost:11434/api`, https://docs.ollama.com/api), while the daemon
/// host (`OLLAMA_HOST`) is `http://localhost:11434` — so a user may reasonably enter
/// either. Trimming a trailing `/api` (and any trailing slashes) makes both resolve
/// to the same origin, so the joined path is never doubled into `/api/api/chat`.
/// Shared by [`LocalModelClient`] and `embedding::LocalEmbedder`, which both append
/// `/api/...`.
pub(crate) fn normalize_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/api")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string()
}

/// One chat message in the Ollama native `/api/chat` shape.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

/// A local chat request — the flexible call shape the suite's stages build. The
/// endpoint is carried by the [`LocalModelClient`]; everything else is per-call.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// The roster model id to address (e.g. `qwen3.5:122b`).
    pub model_id: String,
    pub messages: Vec<ChatMessage>,
    /// Grammar-constraining JSON schema (Ollama native `format`) for schema-valid
    /// structured output. `None` leaves the model unconstrained (free prose).
    pub format_schema: Option<Value>,
    /// Tool definitions (Ollama native `tools`). The orchestrator executes tools and
    /// feeds results back; the model only requests them.
    pub tools: Option<Value>,
    /// Whether to enable the model's thinking mode (Ollama native `think`). Sent only
    /// when `true`, so a non-thinking model is never asked to think.
    pub think: bool,
    /// Generation options (temperature, `num_ctx`, …) passed as Ollama `options`.
    pub options: Option<Value>,
}

impl ChatRequest {
    /// A minimal request: a model id and its messages, everything else unset.
    pub fn new(model_id: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model_id: model_id.into(),
            messages,
            format_schema: None,
            tools: None,
            think: false,
            options: None,
        }
    }
}

/// The result of a chat call (non-streaming, or a reconstructed stream): the
/// assistant content (the structured-output JSON text when a `format_schema` was
/// supplied) and the model's reasoning when thinking mode surfaced it.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub thinking: Option<String>,
}

/// Skip a `false` bool when serializing (so `think: false` is omitted entirely).
fn is_false(b: &bool) -> bool {
    !*b
}

/// The `/api/chat` request body, serialized from the typed request. `stream` is set
/// by the caller (`false` for [`LocalModelClient::chat`], `true` for the streaming
/// path). Pure, so the wire contract is unit-testable without a live daemon.
#[derive(Debug, Serialize)]
struct ChatWire<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    #[serde(skip_serializing_if = "is_false")]
    think: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<&'a Value>,
}

/// Build the `/api/chat` request body for the given streaming mode.
fn build_chat_body(req: &ChatRequest, stream: bool) -> Value {
    let wire = ChatWire {
        model: &req.model_id,
        messages: &req.messages,
        stream,
        think: req.think,
        format: req.format_schema.as_ref(),
        tools: req.tools.as_ref(),
        options: req.options.as_ref(),
    };
    // These are plain owned/borrowed values; serialization cannot fail.
    serde_json::to_value(&wire).expect("local chat request is serializable")
}

/// The non-streaming `/api/chat` reply, trimmed to the fields the caller needs.
#[derive(Debug, Deserialize)]
struct ChatReplyWire {
    message: ChatReplyMessage,
}

#[derive(Debug, Deserialize)]
struct ChatReplyMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    thinking: Option<String>,
}

/// Shape a non-streaming `/api/chat` response body into a [`ChatResponse`]. Pure, so
/// the envelope contract is testable without a live call. An empty `thinking` string
/// collapses to `None` so callers don't distinguish "" from absent.
fn parse_chat_reply(body: &str) -> Result<ChatResponse> {
    let wire: ChatReplyWire =
        serde_json::from_str(body).context("parsing local chat response JSON")?;
    Ok(ChatResponse {
        content: wire.message.content,
        thinking: wire.message.thinking.filter(|t| !t.is_empty()),
    })
}

/// Ollama's `/api/tags` model-list reply, trimmed to the name fields.
#[derive(Debug, Deserialize)]
struct TagsWire {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(Debug, Deserialize)]
struct TagModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
}

/// Pull the available model ids out of an `/api/tags` body. Both `name` and `model`
/// are kept (they are usually identical, but either may carry the tagged id), so the
/// tolerant [`model_matches`] check has both forms to compare against.
fn parse_available_models(body: &str) -> Result<Vec<String>> {
    let wire: TagsWire = serde_json::from_str(body).context("parsing local model list JSON")?;
    let mut out = Vec::with_capacity(wire.models.len() * 2);
    for m in wire.models {
        if !m.name.is_empty() {
            out.push(m.name);
        }
        if !m.model.is_empty() {
            out.push(m.model);
        }
    }
    Ok(out)
}

/// Whether a daemon-reported `available` id satisfies a `configured` roster id.
/// Ollama ids carry a `:tag` suffix (`qwen3.5:122b`, `model:latest`); a configured id
/// may name the tag explicitly or omit it. Exact match always wins; a tagless
/// configured id matches any tag of the same base; and a daemon `:latest` matches a
/// tagless configured base.
fn model_matches(available: &str, configured: &str) -> bool {
    if available == configured {
        return true;
    }
    if !configured.contains(':') {
        if let Some((base, _tag)) = available.split_once(':') {
            return base == configured;
        }
    }
    false
}

/// The configured roster's three model ids (reasoner, fast tier, embedder).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roster {
    pub reasoner: String,
    pub fast: String,
    pub embedder: String,
}

impl Roster {
    /// The configured (non-blank) roster ids, deduped, in roster order.
    fn configured_ids(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for id in [
            self.reasoner.trim(),
            self.fast.trim(),
            self.embedder.trim(),
        ] {
            if !id.is_empty() && !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }
}

/// Which configured roster ids the daemon is missing. Pure over the available set.
/// An unconfigured (blank) roster slot is not reported here — that gap is config
/// completeness, surfaced by [`local_gate`].
fn missing_roster_models(roster: &Roster, available: &[String]) -> Vec<String> {
    roster
        .configured_ids()
        .into_iter()
        .filter(|id| !available.iter().any(|a| model_matches(a, id)))
        .map(str::to_string)
        .collect()
}

/// The outcome of probing the local daemon for the gate: unreachable (with a reason),
/// or reachable plus which configured roster ids it was missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonProbe {
    Unreachable(String),
    Reachable { missing: Vec<String> },
}

/// The flexible local-model client: one daemon endpoint, addressed by model id per
/// call. A no-op [`RunContext`] by default (tests / offline); the live command
/// attaches the real one via [`LocalModelClient::with_context`].
pub struct LocalModelClient {
    http: reqwest::blocking::Client,
    base_url: String,
    progress: Arc<RunContext>,
}

impl LocalModelClient {
    /// Build a client for one daemon endpoint (e.g. `http://localhost:11434`). A
    /// trailing slash is trimmed so a joined path's leading slash doesn't double up.
    pub fn new(endpoint: impl Into<String>) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .context("building the local-model HTTP client")?;
        Ok(Self {
            http,
            base_url: normalize_endpoint(&endpoint.into()),
            progress: RunContext::noop(),
        })
    }

    /// Attach a live run context so each call streams a tracker row (and, for the
    /// streaming path, tokens / reasoning). Without it the client stays no-op.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// One non-streaming chat call, returning the (schema-valid, when constrained)
    /// content and any reasoning. Emits one tracker row per call.
    pub fn chat(&self, req: &ChatRequest) -> Result<ChatResponse> {
        self.progress
            .request_started("Local", "local", req.model_id.as_str(), "Local model");
        let result = self.chat_inner(req);
        self.finish_row(&req.model_id, &result);
        result
    }

    fn chat_inner(&self, req: &ChatRequest) -> Result<ChatResponse> {
        let body = build_chat_body(req, false);
        let resp = self
            .http
            .post(self.url(CHAT_PATH))
            .json(&body)
            .send()
            .context("sending local chat request")?;
        let status = resp.status();
        let text = resp.text().context("reading local chat response body")?;
        if !status.is_success() {
            bail!("local model returned {status}: {text}");
        }
        parse_chat_reply(&text)
    }

    /// A streaming chat call: emits tokens / reasoning through the run context as the
    /// model writes (per `role`), reconstructs the full envelope, and returns it. The
    /// reconstructed content is the source of truth for any downstream parse, exactly
    /// like the cloud streaming path — the live emits are a pure side-channel.
    ///
    /// Like the cloud streaming agent (`model_agent::ModelMainAgent::call`), the
    /// streamed channels *are* the tracker view, so no per-request row is emitted here
    /// (a row would also falsely read `ok` on a cancel, which resolves to `Err`). A
    /// cancel or a truncated stream returns `Err` rather than a partial `ChatResponse`,
    /// so a prose stage can't mistake a cut-off stream for a complete answer and
    /// `run_job` classifies a cancelled run off the shared flag (`jobs.rs`).
    pub fn chat_streaming(&self, req: &ChatRequest, role: StreamRole<'_>) -> Result<ChatResponse> {
        let body = build_chat_body(req, true);
        let resp = self
            .http
            .post(self.url(CHAT_PATH))
            .json(&body)
            .send()
            .context("sending local chat request")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().unwrap_or_default();
            bail!("local model returned {status}: {text}");
        }
        stream_chat_response(std::io::BufReader::new(resp), &self.progress, role)
    }

    /// Emit the terminal tracker row for a chat call.
    fn finish_row(&self, model_id: &str, result: &Result<ChatResponse>) {
        match result {
            Ok(_) => self
                .progress
                .request_finished("Local", "local", model_id, "Local model", "ok", None),
            Err(e) => self.progress.request_finished(
                "Local",
                "local",
                model_id,
                "Local model",
                "failed",
                Some(e.to_string()),
            ),
        }
    }

    /// Health-check: the daemon answers `/api/tags`. A transport error or non-2xx is
    /// an unreachable daemon.
    pub fn health_check(&self) -> Result<()> {
        let status = self
            .http
            .get(self.url(TAGS_PATH))
            .send()
            .context("contacting the local model daemon")?
            .status();
        if !status.is_success() {
            bail!("local model daemon returned {status}");
        }
        Ok(())
    }

    /// The model ids the daemon currently has available (`/api/tags`).
    pub fn available_models(&self) -> Result<Vec<String>> {
        let resp = self
            .http
            .get(self.url(TAGS_PATH))
            .send()
            .context("listing local models")?;
        let status = resp.status();
        let text = resp.text().context("reading local model list")?;
        if !status.is_success() {
            bail!("local model daemon returned {status}: {text}");
        }
        parse_available_models(&text)
    }

    /// Probe the daemon for the gate: list models, then check the roster against them.
    /// A failed list (unreachable / non-2xx) is [`DaemonProbe::Unreachable`] — roster
    /// presence can't be judged when the daemon can't be reached.
    pub fn probe_daemon(&self, roster: &Roster) -> DaemonProbe {
        match self.available_models() {
            Err(e) => DaemonProbe::Unreachable(e.to_string()),
            Ok(available) => DaemonProbe::Reachable {
                missing: missing_roster_models(roster, &available),
            },
        }
    }
}

/// Which channels a streamed local chat surfaces to the tracker — mirrors the cloud
/// `model_agent::StreamRole`. All roles accumulate the full envelope (the parse
/// source of truth); they differ in what they *stream*.
#[derive(Debug, Clone, Copy)]
pub enum StreamRole<'a> {
    /// Stream the decoded content (`agent_token`) and reasoning (`agent_thinking`).
    Main,
    /// Stream reasoning only, posture-tagged (`analyst_thinking`).
    Analyst(&'a str),
    /// Stream nothing — accumulate silently (structured stages with no console value).
    Silent,
}

/// Route a coalesced reasoning chunk to the channel the role selects.
fn emit_thinking(progress: &RunContext, role: StreamRole<'_>, delta: String) {
    match role {
        StreamRole::Main => progress.agent_thinking(delta),
        StreamRole::Analyst(posture) => progress.analyst_thinking(posture, delta),
        StreamRole::Silent => {}
    }
}

/// Decode an Ollama native `/api/chat` newline-delimited JSON stream to completion,
/// accumulating the content + reasoning while streaming the live channels `role`
/// selects. Each line is one JSON chunk (`{ "message": { "content", "thinking" },
/// "done" }`); the terminal chunk carries `done: true`.
///
/// Takes `impl BufRead` (not the `reqwest::Response` directly) so the loop is
/// unit-testable offline against a synthetic byte stream. A cancel observed mid-stream
/// stops reading promptly; a stream that ends without a `done` chunk and was not
/// cancelled is a truncation and fails the call (rather than returning a silently
/// short envelope that would surface only as an opaque downstream parse error).
fn stream_chat_response(
    reader: impl BufRead,
    progress: &RunContext,
    role: StreamRole<'_>,
) -> Result<ChatResponse> {
    let mut content = String::new();
    let mut thinking = String::new();
    let mut token_pending = String::new();
    let mut thinking_pending = String::new();
    let mut saw_done = false;

    for line in reader.lines() {
        if progress.is_cancelled() {
            break;
        }
        let line = line.context("reading streamed local model response")?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Tolerate any non-JSON keep-alive line rather than failing the stream.
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // An explicit error chunk fails the stream with its reason.
        if let Some(err) = event.get("error").and_then(Value::as_str) {
            bail!("local model stream error: {err}");
        }
        if let Some(c) = event.pointer("/message/content").and_then(Value::as_str) {
            if !c.is_empty() {
                content.push_str(c);
                if matches!(role, StreamRole::Main) {
                    token_pending.push_str(c);
                    if token_pending.chars().count() >= TOKEN_FLUSH_CHARS {
                        progress.agent_token(std::mem::take(&mut token_pending));
                    }
                }
            }
        }
        if let Some(t) = event.pointer("/message/thinking").and_then(Value::as_str) {
            if !t.is_empty() {
                thinking.push_str(t);
                thinking_pending.push_str(t);
                if thinking_pending.chars().count() >= TOKEN_FLUSH_CHARS {
                    emit_thinking(progress, role, std::mem::take(&mut thinking_pending));
                }
            }
        }
        if event.get("done").and_then(Value::as_bool) == Some(true) {
            saw_done = true;
            break;
        }
    }
    // Flush whatever streamed so the tracker shows the partial output even when the
    // stream then resolves to an error below.
    if !token_pending.is_empty() {
        progress.agent_token(token_pending);
    }
    if !thinking_pending.is_empty() {
        emit_thinking(progress, role, thinking_pending);
    }
    // Cancellation and truncation both resolve to `Err`, never a partial `Ok`: a
    // schema stage's truncated JSON would fail to parse downstream anyway, but a prose
    // stage's would not — so the cut-off must be signalled here, not left to the
    // caller. `run_job` maps the cancel error to a Cancelled run via the shared flag.
    if progress.is_cancelled() {
        bail!("local model stream cancelled");
    }
    if !saw_done {
        bail!("local model stream ended before completion");
    }
    Ok(ChatResponse {
        content,
        thinking: (!thinking.is_empty()).then_some(thinking),
    })
}

/// The local daemon endpoint from validated configuration: the set, non-blank value
/// or `None`. Blank reads as unset, like the cloud credential resolvers.
pub fn endpoint_from_config(cfg: &AppConfig) -> Option<String> {
    config::present(&cfg.local_daemon_endpoint).map(str::to_string)
}

/// The configured roster from configuration (each slot trimmed, blank slots left "").
pub fn roster_from_config(cfg: &AppConfig) -> Roster {
    let val = |opt: &Option<String>| opt.as_deref().unwrap_or("").trim().to_string();
    Roster {
        reasoner: val(&cfg.local_reasoner_model),
        fast: val(&cfg.local_fast_model),
        embedder: val(&cfg.local_embedder_model),
    }
}

/// The local-suite execution gate, as a [`ValidationReport`] reusing the report's
/// warning model (one [`WarningKind::LocalModels`] category). Pure over the probe
/// outcome so the matrix is unit-testable; the live probe is
/// [`LocalModelClient::probe_daemon`]. Independent of the cloud-report gate
/// ([`config::validate`]) — a machine set up for the report need not be set up for
/// the local suite, and vice versa.
///
/// Three gaps fold into the one category, in order: configuration not yet complete
/// (endpoint / a **required** roster slot — reasoner or embedder — blank; the
/// optional fast tier never gates, `docs/configuration.md §Local Analysis Suite
/// Configuration` — a blank fast falls back to the reasoner in the pipeline), the
/// daemon unreachable, and a configured roster id the daemon doesn't have.
pub fn local_gate(cfg: &AppConfig, probe: &DaemonProbe) -> ValidationReport {
    let mut items: Vec<String> = Vec::new();

    let mut unconfigured: Vec<&str> = Vec::new();
    if config::present(&cfg.local_daemon_endpoint).is_none() {
        unconfigured.push("daemon endpoint");
    }
    if config::present(&cfg.local_reasoner_model).is_none() {
        unconfigured.push("reasoner model");
    }
    if config::present(&cfg.local_embedder_model).is_none() {
        unconfigured.push("embedder model");
    }
    if !unconfigured.is_empty() {
        items.push(format!("Not configured: {}.", config::join_list(&unconfigured)));
    }

    match probe {
        DaemonProbe::Unreachable(detail) => {
            items.push(format!("Daemon unreachable: {detail}."));
        }
        DaemonProbe::Reachable { missing } if !missing.is_empty() => {
            let refs: Vec<&str> = missing.iter().map(String::as_str).collect();
            items.push(format!("Models not available: {}.", config::join_list(&refs)));
        }
        DaemonProbe::Reachable { .. } => {}
    }

    let mut categories = Vec::new();
    if !items.is_empty() {
        categories.push(WarningCategory {
            kind: WarningKind::LocalModels,
            title: "Local models".to_string(),
            items,
            dismiss_id: None,
        });
    }
    let is_blocked = categories.iter().any(|c| c.kind.is_blocking());
    ValidationReport {
        categories,
        is_blocked,
    }
}

/// The **presence-only** half of [`local_gate`], for the proactive Persistent
/// Warning Area render (`docs/interface.md §Connection status`): the persistent
/// warning fires on missing *configuration* only, never on a live connectivity
/// probe — so this gates on the config fields alone by treating the daemon as
/// reachable with a full roster. Connectivity stays a run-gate / Test-Connection
/// concern, discovered at run time. Sync-safe: no network.
pub fn local_presence_gate(cfg: &AppConfig) -> ValidationReport {
    local_gate(cfg, &DaemonProbe::Reachable { missing: Vec::new() })
}

/// The Settings "Test connection" result for the local daemon: reachable?, a reason
/// when not, and any configured roster ids the daemon is missing.
#[derive(Debug, Clone, Serialize)]
pub struct LocalDaemonStatus {
    pub reachable: bool,
    pub detail: Option<String>,
    pub missing_models: Vec<String>,
}

impl LocalDaemonStatus {
    /// The result when no daemon endpoint is configured — no network call is made.
    pub fn not_configured() -> Self {
        Self {
            reachable: false,
            detail: Some("No local daemon endpoint configured".to_string()),
            missing_models: Vec::new(),
        }
    }
}

/// Probe one daemon endpoint for the Settings test command (runs inside
/// `spawn_blocking`). Builds a client and resolves a [`DaemonProbe`] into the
/// view-facing [`LocalDaemonStatus`].
pub fn daemon_status(endpoint: &str, roster: &Roster) -> LocalDaemonStatus {
    let client = match LocalModelClient::new(endpoint) {
        Ok(c) => c,
        Err(e) => {
            return LocalDaemonStatus {
                reachable: false,
                detail: Some(e.to_string()),
                missing_models: Vec::new(),
            }
        }
    };
    match client.probe_daemon(roster) {
        DaemonProbe::Unreachable(detail) => LocalDaemonStatus {
            reachable: false,
            detail: Some(detail),
            missing_models: Vec::new(),
        },
        DaemonProbe::Reachable { missing } => LocalDaemonStatus {
            reachable: true,
            detail: None,
            missing_models: missing,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::{ProgressEvent, RecordingReporter};
    use crate::test_http::{Canned, MockHttp};
    use std::sync::atomic::AtomicBool;

    fn roster(reasoner: &str, fast: &str, embedder: &str) -> Roster {
        Roster {
            reasoner: reasoner.to_string(),
            fast: fast.to_string(),
            embedder: embedder.to_string(),
        }
    }

    fn local_cfg() -> AppConfig {
        AppConfig {
            local_daemon_endpoint: Some("http://localhost:11434".into()),
            local_reasoner_model: Some("qwen3.5:122b".into()),
            local_fast_model: Some("qwen3.5:35b".into()),
            local_embedder_model: Some("qwen3-embedding:4b".into()),
            ..AppConfig::default()
        }
    }

    // ---- pure request/response shaping ----

    #[test]
    fn build_chat_body_carries_model_messages_format_and_stream() {
        let mut req = ChatRequest::new("qwen3.5:122b", vec![ChatMessage::user("hi")]);
        req.format_schema = Some(serde_json::json!({ "type": "object" }));
        req.think = true;
        let body = build_chat_body(&req, true);
        assert_eq!(body["model"], "qwen3.5:122b");
        assert_eq!(body["stream"], true);
        assert_eq!(body["think"], true);
        assert_eq!(body["format"]["type"], "object");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hi");
    }

    #[test]
    fn build_chat_body_omits_optional_and_false_fields() {
        let req = ChatRequest::new("m", vec![ChatMessage::user("x")]);
        let body = build_chat_body(&req, false);
        assert_eq!(body["stream"], false);
        // think:false, format, tools, options are all omitted entirely.
        let obj = body.as_object().unwrap();
        assert!(!obj.contains_key("think"), "{obj:?}");
        assert!(!obj.contains_key("format"), "{obj:?}");
        assert!(!obj.contains_key("tools"), "{obj:?}");
        assert!(!obj.contains_key("options"), "{obj:?}");
    }

    #[test]
    fn parse_chat_reply_extracts_content_and_thinking() {
        let r = parse_chat_reply(
            r#"{"message":{"role":"assistant","content":"hello","thinking":"reasoning"}}"#,
        )
        .unwrap();
        assert_eq!(r.content, "hello");
        assert_eq!(r.thinking.as_deref(), Some("reasoning"));
    }

    #[test]
    fn parse_chat_reply_collapses_empty_thinking_to_none() {
        let r = parse_chat_reply(r#"{"message":{"content":"x"}}"#).unwrap();
        assert_eq!(r.content, "x");
        assert!(r.thinking.is_none());
    }

    #[test]
    fn parse_chat_reply_errors_on_malformed_body() {
        let err = parse_chat_reply("not json").unwrap_err();
        assert!(err.to_string().contains("parsing local chat response"), "{err}");
    }

    #[test]
    fn parse_available_models_collects_name_and_model_ids() {
        let ids = parse_available_models(
            r#"{"models":[{"name":"qwen3.5:122b","model":"qwen3.5:122b"},{"name":"x:latest","model":"x:latest"}]}"#,
        )
        .unwrap();
        assert!(ids.contains(&"qwen3.5:122b".to_string()));
        assert!(ids.contains(&"x:latest".to_string()));
    }

    #[test]
    fn model_matches_handles_exact_and_tagless() {
        assert!(model_matches("qwen3.5:122b", "qwen3.5:122b")); // exact
        assert!(model_matches("qwen3.5:122b", "qwen3.5")); // tagless configured matches a tag
        assert!(!model_matches("qwen3.5:35b", "qwen3.5:122b")); // different tag
        assert!(!model_matches("other:122b", "qwen3.5")); // different base
    }

    #[test]
    fn missing_roster_models_flags_only_absent_configured_ids() {
        let available = vec!["qwen3.5:122b".to_string(), "qwen3.5:35b".to_string()];
        // embedder absent; a blank slot is not reported here (config completeness is
        // the gate's job).
        let missing = missing_roster_models(&roster("qwen3.5:122b", "qwen3.5:35b", "absent:4b"), &available);
        assert_eq!(missing, vec!["absent:4b".to_string()]);
        let none = missing_roster_models(&roster("qwen3.5:122b", "qwen3.5:35b", ""), &available);
        assert!(none.is_empty());
    }

    // ---- the gate matrix ----

    #[test]
    fn local_gate_passes_when_configured_and_reachable_and_complete() {
        let report = local_gate(&local_cfg(), &DaemonProbe::Reachable { missing: vec![] });
        assert!(!report.is_blocked);
        assert!(report.categories.is_empty());
    }

    #[test]
    fn local_gate_blocks_when_unconfigured() {
        let report = local_gate(&AppConfig::default(), &DaemonProbe::Reachable { missing: vec![] });
        assert!(report.is_blocked);
        let cat = &report.categories[0];
        assert_eq!(cat.kind, WarningKind::LocalModels);
        assert!(cat.items[0].contains("daemon endpoint"), "{:?}", cat.items);
        assert!(cat.items[0].contains("reasoner model"), "{:?}", cat.items);
    }

    #[test]
    fn gate_never_blocks_on_the_optional_fast_tier() {
        // Endpoint + reasoner + embedder with NO fast model is a valid documented
        // setup (`docs/configuration.md` — the fast tier never gates); the
        // pipeline falls back to the reasoner for distillation.
        let cfg = AppConfig {
            local_fast_model: None,
            ..local_cfg()
        };
        let report = local_gate(&cfg, &DaemonProbe::Reachable { missing: vec![] });
        assert!(!report.is_blocked, "{:?}", report.categories);
        assert!(!local_presence_gate(&cfg).is_blocked);
    }

    #[test]
    fn presence_gate_reads_config_only_never_a_probe() {
        // Unset config blocks; full config passes with no daemon anywhere — the
        // presence-only contract the proactive warning band relies on
        // (`docs/interface.md §Connection status`).
        let blocked = local_presence_gate(&AppConfig::default());
        assert!(blocked.is_blocked);
        assert_eq!(blocked.categories[0].kind, WarningKind::LocalModels);
        assert!(!local_presence_gate(&local_cfg()).is_blocked);
    }

    #[test]
    fn local_gate_blocks_when_daemon_unreachable() {
        let report = local_gate(
            &local_cfg(),
            &DaemonProbe::Unreachable("connection refused".to_string()),
        );
        assert!(report.is_blocked);
        assert!(report.categories[0].items[0].contains("unreachable"), "{:?}", report.categories);
    }

    #[test]
    fn local_gate_blocks_when_a_model_is_missing() {
        let report = local_gate(
            &local_cfg(),
            &DaemonProbe::Reachable {
                missing: vec!["qwen3.5:122b".to_string()],
            },
        );
        assert!(report.is_blocked);
        assert!(
            report.categories[0].items[0].contains("not available"),
            "{:?}",
            report.categories
        );
    }

    // ---- offline round trips over the wire ----

    #[test]
    fn chat_round_trips_a_200_into_a_response() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"message":{"role":"assistant","content":"graded"}}"#,
        }]);
        let client = LocalModelClient::new(&server.base_url).unwrap();
        let resp = client
            .chat(&ChatRequest::new("m", vec![ChatMessage::user("grade AAPL")]))
            .unwrap();
        assert_eq!(resp.content, "graded");
        assert_eq!(server.attempts(), 1);
        assert_eq!(server.request_paths(), vec!["/api/chat".to_string()]);
    }

    #[test]
    fn normalize_endpoint_accepts_host_and_documented_api_base() {
        // The daemon host and the documented `…/api` base both resolve to one origin,
        // so the joined `/api/...` path never doubles into `/api/api/...`.
        for input in [
            "http://localhost:11434",
            "http://localhost:11434/",
            "http://localhost:11434/api",
            "http://localhost:11434/api/",
            "  http://localhost:11434/api  ",
        ] {
            assert_eq!(
                normalize_endpoint(input),
                "http://localhost:11434",
                "{input:?}"
            );
        }
    }

    #[test]
    fn chat_does_not_double_api_when_endpoint_includes_it() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"message":{"content":"ok"}}"#,
        }]);
        // The user entered the documented `…/api` base (server.base_url ends in '/').
        let endpoint = format!("{}api", server.base_url);
        let client = LocalModelClient::new(&endpoint).unwrap();
        client
            .chat(&ChatRequest::new("m", vec![ChatMessage::user("x")]))
            .unwrap();
        assert_eq!(server.request_paths(), vec!["/api/chat".to_string()]);
    }

    #[test]
    fn chat_surfaces_a_non_2xx_as_an_error() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 500,
            headers: vec![],
            body: "boom",
        }]);
        let client = LocalModelClient::new(&server.base_url).unwrap();
        let err = client
            .chat(&ChatRequest::new("m", vec![ChatMessage::user("x")]))
            .unwrap_err();
        assert!(err.to_string().contains("500"), "{err}");
    }

    #[test]
    fn available_models_round_trips_the_tags_endpoint() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"models":[{"name":"qwen3.5:122b","model":"qwen3.5:122b"}]}"#,
        }]);
        let client = LocalModelClient::new(&server.base_url).unwrap();
        let ids = client.available_models().unwrap();
        assert!(ids.contains(&"qwen3.5:122b".to_string()));
        assert_eq!(server.request_paths(), vec!["/api/tags".to_string()]);
    }

    // ---- the NDJSON stream decoder ----

    fn recording_ctx() -> (Arc<RecordingReporter>, Arc<RunContext>) {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run", rec.clone(), Arc::new(AtomicBool::new(false)));
        (rec, ctx)
    }

    #[test]
    fn stream_decoder_accumulates_and_emits_tokens_and_thinking() {
        let (rec, ctx) = recording_ctx();
        // Two content chunks + one thinking chunk, then the terminal done chunk.
        let ndjson = concat!(
            r#"{"message":{"content":"Hel"}}"#,
            "\n",
            r#"{"message":{"thinking":"weighing the evidence carefully here"}}"#,
            "\n",
            r#"{"message":{"content":"lo, world from the local model"}}"#,
            "\n",
            r#"{"message":{"content":""},"done":true}"#,
            "\n",
        );
        let resp = stream_chat_response(ndjson.as_bytes(), &ctx, StreamRole::Main).unwrap();
        assert_eq!(resp.content, "Hello, world from the local model");
        assert_eq!(
            resp.thinking.as_deref(),
            Some("weighing the evidence carefully here")
        );
        let msgs = rec.messages();
        let tokens: String = msgs
            .iter()
            .filter_map(|m| match &m.event {
                ProgressEvent::AgentToken { delta } => Some(delta.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tokens, "Hello, world from the local model");
        assert!(msgs
            .iter()
            .any(|m| matches!(&m.event, ProgressEvent::AgentThinking { .. })));
    }

    #[test]
    fn stream_decoder_analyst_role_streams_thinking_not_content() {
        let (rec, ctx) = recording_ctx();
        let ndjson = concat!(
            r#"{"message":{"content":"the structured review body"}}"#,
            "\n",
            r#"{"message":{"thinking":"the bear case rests on the curve here"}}"#,
            "\n",
            r#"{"done":true}"#,
            "\n",
        );
        let resp = stream_chat_response(ndjson.as_bytes(), &ctx, StreamRole::Analyst("bear")).unwrap();
        assert_eq!(resp.content, "the structured review body"); // still accumulated
        let msgs = rec.messages();
        // No content tokens stream for an analyst...
        assert!(!msgs
            .iter()
            .any(|m| matches!(&m.event, ProgressEvent::AgentToken { .. })));
        // ...but its thinking does, posture-tagged.
        assert!(msgs.iter().any(|m| matches!(
            &m.event,
            ProgressEvent::AnalystThinking { posture, .. } if posture == "bear"
        )));
    }

    #[test]
    fn stream_decoder_bails_on_truncation() {
        let (_rec, ctx) = recording_ctx();
        // No done chunk and not cancelled => truncated.
        let ndjson = "{\"message\":{\"content\":\"partial\"}}\n";
        let err = stream_chat_response(ndjson.as_bytes(), &ctx, StreamRole::Main).unwrap_err();
        assert!(err.to_string().contains("before completion"), "{err}");
    }

    #[test]
    fn stream_decoder_errors_on_cancel_rather_than_returning_partial() {
        let rec = Arc::new(RecordingReporter::default());
        let cancel = Arc::new(AtomicBool::new(true)); // already cancelled
        let ctx = RunContext::new("run", rec, cancel);
        let ndjson = "{\"message\":{\"content\":\"x\"}}\n";
        // A cancelled stream resolves to Err (not a partial Ok), so a prose stage can't
        // mistake a cut-off stream for a complete answer; run_job maps it to Cancelled
        // via the shared flag.
        let err = stream_chat_response(ndjson.as_bytes(), &ctx, StreamRole::Main).unwrap_err();
        assert!(err.to_string().contains("cancelled"), "{err}");
    }

    #[test]
    fn stream_decoder_bails_on_an_error_chunk() {
        let (_rec, ctx) = recording_ctx();
        let ndjson = "{\"error\":\"model not found\"}\n";
        let err = stream_chat_response(ndjson.as_bytes(), &ctx, StreamRole::Main).unwrap_err();
        assert!(err.to_string().contains("model not found"), "{err}");
    }

    // ---- config helpers ----

    #[test]
    fn endpoint_and_roster_read_from_config() {
        let cfg = local_cfg();
        assert_eq!(
            endpoint_from_config(&cfg).as_deref(),
            Some("http://localhost:11434")
        );
        let r = roster_from_config(&cfg);
        assert_eq!(r.reasoner, "qwen3.5:122b");
        assert_eq!(r.fast, "qwen3.5:35b");
        assert_eq!(r.embedder, "qwen3-embedding:4b");
        // A blank endpoint reads as unset.
        let blank = AppConfig {
            local_daemon_endpoint: Some("  ".into()),
            ..AppConfig::default()
        };
        assert!(endpoint_from_config(&blank).is_none());
    }
}
