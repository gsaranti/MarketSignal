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
//! `lib.rs`, mirroring `connection_test`). All calls use Ollama's native surface
//! (`/api/chat`, `/api/tags`; embeddings ride `/api/embed` in `embedding.rs`) —
//! the daemon's OpenAI-compatible `/v1/` layer is deliberately unused, since
//! schema-constrained output needs the native `format` parameter (the `/v1/` path
//! advertises only JSON mode). Token + reasoning streaming rides the existing `progress` seam,
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

/// Backstop deadline for a request that declares no generation reservations — the
/// supervision probes (`/api/tags`) and bare test requests — and the floor no
/// derived chat deadline goes under ([`DeadlinePolicy`]).
///
/// How reqwest's blocking client applies a timeout matters here, because the
/// naive reading ("a total deadline for the call") is wrong in a way that hid a
/// run killer. The blocking builder keeps its timeout on the blocking side —
/// it is never forwarded to the async client — and applies it twice: once as
/// the wait for the response *headers*, then afresh on every body `read()`. For
/// a chat call the header wait is where generation happens: with
/// `stream: false` the daemon answers only once the whole chain has generated,
/// and with `stream: true` the first bytes are expected only with the first
/// token (Go's HTTP server sends headers on the first write), so prompt
/// evaluation sits inside the header wait on both paths. A fixed ten minutes
/// there capped a thinking chain at roughly a third of its reservation
/// (`docs/verification/2026-08-24-portfolio-analysis-large-scale-review.md` §C1),
/// which is why a chat request never rides this constant alone.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// The per-call transport bound, derived from the request's own reservations so
/// transport cannot cut a generation that stays inside its reservation while
/// the daemon holds the drafted floors and its pre-generation overhead — runner
/// scheduling, a cold model load — fits in the slack the unused reservation
/// leaves. `num_ctx` over a prompt-evaluation floor covers the prefill that sits
/// inside the header wait, budgeted over the full context rather than the actual
/// prompt; on the non-streaming path, where the whole chain generates before
/// the headers arrive, `num_predict` over a decode floor covers the chain
/// itself. The slack is the unused part of that budget — unfilled context at
/// the prefill floor plus, on the non-streaming path, ungenerated output at the
/// decode floor. Generation shares `num_ctx` with the prompt, so a non-streaming
/// call can never consume both terms: at the thinking reservation (half the
/// context) the combined slack is at least `65_536 / prefill_floor` ≈ 11 min,
/// while a streaming call's slack is only the context its prompt leaves. The
/// streaming path takes the prefill term alone — its tokens arrive as they
/// generate — and the same value then bounds each body read as an idle limit,
/// so a daemon that goes silent mid-stream is still caught. The floors are
/// drafted just under the pinned serving path's worst measured row (160 K prompt
/// tokens: 113 tok/s prefill, 13.3 tok/s decode —
/// `docs/verification/2026-07-28-m5-preflight.md`), calibratable like the
/// engine's other starting parameters, and a re-verification item on any
/// serving-path change (`docs/local-model-operations.md §M5 pre-flight
/// checklist`). The contract is recorded at `docs/local-models.md §The
/// local-model adapter seam`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeadlinePolicy {
    /// Prompt-evaluation throughput floor, tokens per second.
    pub(crate) prefill_floor_tok_s: u32,
    /// Decode throughput floor, tokens per second.
    pub(crate) decode_floor_tok_s: u32,
    /// No derived deadline goes under this; a request declaring no reservation
    /// gets exactly it.
    pub(crate) floor: Duration,
}

impl DeadlinePolicy {
    /// The production policy: the drafted floors over the ten-minute backstop.
    pub(crate) const DEFAULT: Self = Self {
        prefill_floor_tok_s: 100,
        decode_floor_tok_s: 12,
        floor: DEFAULT_TIMEOUT,
    };

    /// The deadline for one request: the prefill term from its `num_ctx`, plus —
    /// non-streaming only — the decode term from its `num_predict`, never under
    /// [`Self::floor`]. A term whose reservation is unset contributes nothing, so a
    /// request with neither rides the floor.
    pub(crate) fn request_deadline(&self, req: &ChatRequest, streaming: bool) -> Duration {
        let prefill = request_num_ctx(req)
            .map(|tokens| Self::span(tokens, self.prefill_floor_tok_s))
            .unwrap_or(Duration::ZERO);
        let decode = if streaming {
            Duration::ZERO
        } else {
            request_num_predict(req)
                .map(|tokens| Self::span(tokens, self.decode_floor_tok_s))
                .unwrap_or(Duration::ZERO)
        };
        (prefill + decode).max(self.floor)
    }

    /// `tokens / floor` as a duration, computed in milliseconds and rounded up so
    /// a tiny test floor still yields a sub-second value.
    fn span(tokens: u32, floor_tok_s: u32) -> Duration {
        let millis = (u64::from(tokens) * 1_000).div_ceil(u64::from(floor_tok_s.max(1)));
        Duration::from_millis(millis)
    }
}

/// Name a transport timeout for what it is: the derived deadline reached. Read as
/// a stalled daemon, throughput under the drafted floor, or pre-generation
/// overhead (runner scheduling, a cold model load) outrunning the reservation's
/// slack — the three things a healthy generation inside its reservation cannot
/// be — rather than reqwest's opaque "operation timed out".
fn deadline_reached(deadline: Duration) -> String {
    format!(
        "transport deadline of {} min reached — the daemon stalled, its throughput fell \
         under the drafted floor, or its pre-generation overhead outran the reservation's \
         slack (`DeadlinePolicy`)",
        deadline.as_millis().div_ceil(60_000)
    )
}

/// Whether an error chain bottoms out in a reqwest timeout — either the header
/// wait's own `reqwest::Error`, or a body read's `io::Error` wrapping one (the
/// streaming path's `BufRead::lines` surfaces the per-read deadline that way).
fn is_transport_timeout(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        if let Some(e) = cause.downcast_ref::<reqwest::Error>() {
            return e.is_timeout();
        }
        cause
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
            .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
            .is_some_and(reqwest::Error::is_timeout)
    })
}

/// Wrap a chat-call error so a deadline trip carries [`deadline_reached`] as its
/// outermost message — the one the tracker row and the failure detail show (the
/// report run's, or an isolated Portfolio holding's failed step).
fn name_deadline_trip(err: anyhow::Error, deadline: Duration) -> anyhow::Error {
    if is_transport_timeout(&err) {
        err.context(deadline_reached(deadline))
    } else {
        err
    }
}

/// A transient failure class the bounded retry-once re-attempts
/// (`docs/local-models.md §The local-model adapter seam`). Doubles as a typed
/// chain marker: the producing site roots or contexts its error with the class,
/// and [`retry_class`] downcasts rather than string-matching. Deadline trips,
/// length stops, cancellation, and any unclassified failure deliberately carry
/// no marker — each is attributable, deterministic, or intentional, so a retry
/// would spend hours reproducing a known outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryClass {
    /// A transport-level connection failure (refused, reset, dropped body) —
    /// never a deadline trip, which [`retry_class`] excludes first.
    Transport,
    /// The daemon answered a non-2xx status.
    DaemonStatus,
    /// HTTP 200 whose completion content is blank with no tool calls.
    EmptyCompletion,
    /// The returned body or content failed its JSON/schema parse.
    SchemaParse,
    /// The stream carried an error chunk or ended before its done chunk.
    Stream,
    /// The interpretation parsed but a model-arm value fell outside its
    /// declared numeric domain (`portfolio::validate_model_arm`) — a sampled
    /// response whose re-issue may well land in-domain, so it classifies
    /// transient, as its own class so the data-health read tells an
    /// off-domain value from malformed content (the 2026-08-24 review's
    /// Codex I6, ruled 2026-08-29).
    ModelArmDomain,
}

impl std::fmt::Display for RetryClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Transport => "transport error",
            Self::DaemonStatus => "daemon error status",
            Self::EmptyCompletion => "empty completion body",
            Self::SchemaParse => "content failed its parse",
            Self::Stream => "stream broke before completion",
            Self::ModelArmDomain => "model arm value off its declared domain",
        })
    }
}

impl std::error::Error for RetryClass {}

/// One fired retry, recorded for the run's data-health read
/// (`build_data_health`): which stage re-attempted and for which
/// [`RetryClass`]. In a persisted run every recorded event's re-attempt
/// succeeded — a second failure is not recorded here: the report run fails
/// before any row persists, and the Portfolio job drops a failed holding's
/// retry events as it isolates the holding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetryEvent {
    pub stage: String,
    /// The [`RetryClass`] display form — a string so persisted rows outlive
    /// enum changes.
    pub cause: String,
}

/// Classify whether one failed chat call may re-attempt: `None` for a deadline
/// trip (attributable — a retry doubles a multi-hour wait), then the typed
/// marker anywhere in the chain, then bare transport errors (a reqwest or IO
/// failure that is not a timeout). Everything else — length stops,
/// cancellation, unclassified failures — stays `None`: the whitelist is the
/// contract.
pub(crate) fn retry_class(err: &anyhow::Error) -> Option<RetryClass> {
    if is_transport_timeout(err) {
        return None;
    }
    if let Some(class) = err.downcast_ref::<RetryClass>() {
        return Some(*class);
    }
    err.chain()
        .any(|cause| {
            cause.downcast_ref::<reqwest::Error>().is_some()
                || cause.downcast_ref::<std::io::Error>().is_some()
        })
        .then_some(RetryClass::Transport)
}

/// The pause before the single re-attempt — long enough to ride out a daemon
/// hiccup (a dropped socket, a restarting runner), negligible beside any real
/// generation. Drafted, calibratable like the deadline floors.
const RETRY_ONCE_DELAY: Duration = Duration::from_secs(2);

/// The annotation a second failure carries after one fired retry — shared by
/// [`RetryOnce::run`] and the trait-gate legs (research / distill), so every
/// path's hard failure names the first attempt's class.
pub(crate) fn retried_once_annotation(first: &anyhow::Error) -> String {
    match retry_class(first) {
        Some(class) => format!("failed again after one retry ({class} on the first attempt)"),
        None => "failed again after one retry".to_string(),
    }
}

/// The bounded retry-once gate (`docs/local-models.md §The local-model adapter
/// seam`): at most one re-attempt per **issued** call, only for a
/// [`RetryClass`] failure, never into a cancelled run. One instance rides the
/// per-run analyst; the streaming stages wrap through [`RetryOnce::run`],
/// while the research / distill loops — which own their parse above their
/// model traits — ask [`RetryOnce::permit`] through those traits' gate
/// methods. The layers must not nest: each call path wraps exactly once. They
/// compose only through re-issue — the research loop's findings-parse leg
/// issues a fresh call that carries its own single re-attempt, so one logical
/// terminal turn is hard-bounded at four calls. Every fired retry emits a
/// tracker row and records a [`RetryEvent`] for the run's data-health read.
pub(crate) struct RetryOnce {
    events: std::sync::Mutex<Vec<RetryEvent>>,
    delay: Duration,
}

impl RetryOnce {
    pub(crate) fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
            delay: RETRY_ONCE_DELAY,
        }
    }

    /// Test seam: no pause, so retry tests don't sleep.
    #[cfg(test)]
    pub(crate) fn without_delay() -> Self {
        Self {
            delay: Duration::ZERO,
            ..Self::new()
        }
    }

    /// Drain the fired-retry records (the data-health read).
    pub(crate) fn take_events(&self) -> Vec<RetryEvent> {
        std::mem::take(
            &mut *self
                .events
                .lock()
                .expect("retry-event lock is never poisoned"),
        )
    }

    /// Whether one re-attempt may fire for this failure: classify, refuse when
    /// cancelled, then note it on the tracker, record the event, and pause
    /// (abortably). `true` means the caller runs the attempt exactly once more.
    pub(crate) fn permit(&self, progress: &RunContext, stage: &str, err: &anyhow::Error) -> bool {
        let Some(class) = retry_class(err) else {
            return false;
        };
        if progress.is_cancelled() {
            return false;
        }
        // The retry's own row pair: the streaming stages emit no per-request
        // rows, so this is a fired retry's only tracker surface — status
        // "failed" (attempt one did fail), the detail naming the class and the
        // single re-attempt.
        progress.request_started("Local", "local", stage, "Local model retry");
        progress.request_finished(
            "Local",
            "local",
            stage,
            "Local model retry",
            "failed",
            Some(format!("{class}; retrying once: {err}")),
        );
        self.events
            .lock()
            .expect("retry-event lock is never poisoned")
            .push(RetryEvent {
                stage: stage.to_string(),
                cause: class.to_string(),
            });
        // Abortable pause: poll the cancel flag rather than sleeping blind.
        const POLL: Duration = Duration::from_millis(100);
        let mut waited = Duration::ZERO;
        while waited < self.delay {
            if progress.is_cancelled() {
                return false;
            }
            std::thread::sleep(POLL.min(self.delay - waited));
            waited += POLL;
        }
        !progress.is_cancelled()
    }

    /// Run `attempt`, re-running it once when [`Self::permit`] allows. The
    /// second failure is the hard posture, annotated with the first attempt's
    /// class so the failure detail stays attributable (the seam is job-agnostic —
    /// what a hard failure fails is each job's posture).
    pub(crate) fn run<T>(
        &self,
        progress: &RunContext,
        stage: &str,
        mut attempt: impl FnMut() -> Result<T>,
    ) -> Result<T> {
        match attempt() {
            Ok(v) => Ok(v),
            Err(first) => {
                if !self.permit(progress, stage, &first) {
                    return Err(first);
                }
                attempt().map_err(|second| second.context(retried_once_annotation(&first)))
            }
        }
    }
}

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

/// One chat message in the Ollama native `/api/chat` shape. `tool_calls`
/// rides only on an assistant turn echoed back into the history (the research
/// loop's tool protocol — the daemon needs the call it made beside the tool
/// result that answered it); `None` everywhere else and omitted on the wire.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            tool_calls: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: None,
        }
    }
    /// An assistant turn carrying the tool calls the daemon returned — echoed
    /// into the history verbatim so the next turn sees what it asked for.
    pub fn assistant_with_tool_calls(content: impl Into<String>, tool_calls: Value) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: Some(tool_calls),
        }
    }
    /// A tool-result turn (Ollama role `tool`): the orchestrator's answer to
    /// one requested call.
    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
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
    /// The model's thinking mode (Ollama native `think`). `None` omits the field —
    /// the model's own default, and the safe shape for a model that can't take the
    /// parameter at all. `Some(b)` is **always serialized**, so a non-thinking stage
    /// explicitly says `think: false` rather than silently riding a thinking-on
    /// default (the first live run's F3: the omitted flag cost ~45 minutes,
    /// `docs/verification/2026-07-31-first-live-portfolio-run.md`).
    pub think: Option<bool>,
    /// Generation options (temperature, `num_ctx`, …) passed as Ollama `options`.
    pub options: Option<Value>,
    /// Residency (Ollama native `keep_alive`), in seconds; `-1` keeps the model
    /// loaded indefinitely — the roster's stay-resident default
    /// (`docs/local-models.md §The model roster and per-task routing`). `None`
    /// omits the field (the daemon's own idle-unload default).
    pub keep_alive: Option<i64>,
}

impl ChatRequest {
    /// A minimal request: a model id and its messages, everything else unset.
    pub fn new(model_id: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model_id: model_id.into(),
            messages,
            format_schema: None,
            tools: None,
            think: None,
            options: None,
            keep_alive: None,
        }
    }
}

/// Generation-option profiles for the roster reasoner, straight from the vendor
/// sampling table (`docs/local-model-operations.md §Sampling settings`). Greedy
/// decoding (temperature 0) is explicitly warned against for this model family, so
/// stages build `options` through these rather than ad-hoc literals. `num_ctx` is
/// always explicit — never the daemon's memory-dependent auto-size — per the
/// ops doc's `num_ctx` trap: too small silently front-truncates the deterministic
/// packet, too large starves memory.
pub mod options {
    use serde_json::{json, Value};

    /// The "Thinking — general" row: research / interpretation stages.
    /// `num_predict` is the stage's explicit output reservation — a generous
    /// diagnostic ceiling, not a constraint: a stop at the limit surfaces as
    /// `done_reason: "length"` and becomes a legible truncation error instead of
    /// an opaque downstream parse failure
    /// (`docs/verification/2026-08-10-big-run-attempt-1.md` §Fix candidates 4).
    /// A load-time option it is not — unlike `num_ctx`, changing it never
    /// reloads the resident runner.
    pub fn thinking_general(num_ctx: u32, num_predict: u32) -> Value {
        json!({
            "temperature": 1.0,
            "top_p": 0.95,
            "top_k": 20,
            "min_p": 0.0,
            "presence_penalty": 1.5,
            "num_ctx": num_ctx,
            "num_predict": num_predict,
        })
    }

    /// The "Non-thinking — general" row: consolidation / distillation stages.
    pub fn non_thinking_general(num_ctx: u32, num_predict: u32) -> Value {
        json!({
            "temperature": 0.7,
            "top_p": 0.8,
            "top_k": 20,
            "min_p": 0.0,
            "presence_penalty": 1.5,
            "num_ctx": num_ctx,
            "num_predict": num_predict,
        })
    }
}

/// The result of a chat call (non-streaming, or a reconstructed stream): the
/// assistant content (the structured-output JSON text when a `format_schema` was
/// supplied) and the model's reasoning when thinking mode surfaced it.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub thinking: Option<String>,
    /// Ollama's reported prompt token count (`prompt_eval_count` — on the
    /// non-streaming reply body, or the stream's `done` chunk). The context-fit
    /// instrumentation: `num_ctx` overflow **silently front-truncates** the prompt
    /// (`docs/local-model-operations.md §The num_ctx trap`), and this count against
    /// the request's `num_ctx` is the only in-app way to see it. `None` when the
    /// daemon omits the field.
    pub prompt_eval_count: Option<u64>,
    /// Ollama's generated-token count (`eval_count`, same sources) — thinking and
    /// content together. Against the request's `num_predict` it is the
    /// output-budget read. `None` when the daemon omits the field.
    pub eval_count: Option<u64>,
    /// Why generation stopped (`done_reason`, same sources) — `"stop"` for a
    /// natural end, `"length"` for a `num_predict`/context stop. A length stop
    /// still arrives with `done: true` and HTTP 200, so this field is the only
    /// way a truncated body is told apart from a complete one before it fails a
    /// downstream parse. `None` when the daemon omits the field.
    pub done_reason: Option<String>,
    /// Tool calls the model requested (`message.tool_calls`, raw) — the
    /// research loop's turn protocol: the orchestrator executes them and feeds
    /// the results back as `tool` messages. `None` on a turn that requested
    /// none (the terminal-findings shape). Carried raw; the loop owns the
    /// typed parse so an unexpected shape degrades that turn, not the adapter.
    pub tool_calls: Option<Value>,
}

/// One chat call's prompt-size observation — the stage label, Ollama's reported
/// prompt token count, and the `num_ctx` the request declared. Collected per run
/// and folded into the run's data-health read (`docs/portfolio-analysis.md`
/// §Portfolio roll-up: the digest-compression covenant's detection leg).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptUsage {
    /// Which call this measures (e.g. `interpret AAPL`, `construction`).
    pub stage: String,
    /// Ollama's `prompt_eval_count` for the call — **post-truncation**: on
    /// `num_ctx` overflow the daemon front-truncates and reports only the kept
    /// tokens (live-verified far *below* `num_ctx`, not near it —
    /// `docs/verification/2026-07-28-m5-preflight.md` §Truncation behavior), so
    /// this count alone cannot witness a truncation. `None` when the daemon
    /// omits the count — the row still records the output-side observation.
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    /// The `num_ctx` the request was sent with; `0` when the request declared
    /// none. Every context-fit consumer gates on `num_ctx > 0`, so a
    /// count-less row can never fake a fill or truncation read.
    pub num_ctx: u32,
    /// The size of the prompt the app actually sent (chars across all message
    /// contents) — the app-side ground truth a post-truncation `prompt_tokens`
    /// is checked against.
    #[serde(default)]
    pub prompt_chars: u64,
    /// Ollama's generated-token count for the call (`eval_count` — thinking and
    /// content together), when reported. The output-side half of the read.
    #[serde(default)]
    pub completion_tokens: Option<u64>,
    /// The `num_predict` the request declared, when set.
    #[serde(default)]
    pub num_predict: Option<u32>,
    /// The call stopped at a length limit (`done_reason: "length"`) — its own
    /// output reservation, or the shared context filling first. The consumers
    /// disambiguate through [`length_stop_reading`].
    #[serde(default)]
    pub output_limited: bool,
}

/// How a `done_reason: "length"` stop reads against its counts. The one
/// predicate the per-call typed failure and the run-level data-health line
/// both consume, so the two renderings can never drift apart on the same stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthStopReading {
    /// Generated at least the reservation: the call's own `num_predict` bound —
    /// a runaway chain or a genuinely undersized reservation.
    AtReservation,
    /// Generated well under a declared reservation: the shared context filled
    /// first — the digest-compression lever, never a bigger `num_ctx`.
    UnderReservation,
    /// A daemon-omitted count or an unset reservation: the stop is real but
    /// unattributable, and no lever should be named on a guess.
    Unattributed,
}

/// Classify a length stop by its counts (`eval_count` vs the declared
/// `num_predict`).
pub fn length_stop_reading(generated: Option<u64>, reservation: Option<u32>) -> LengthStopReading {
    match (generated, reservation) {
        (Some(g), Some(r)) if g >= u64::from(r) => LengthStopReading::AtReservation,
        (Some(_), Some(_)) => LengthStopReading::UnderReservation,
        _ => LengthStopReading::Unattributed,
    }
}

/// The `num_ctx` a request declares in its generation options, when one is set.
/// Requests built through [`options`] always set it explicitly.
pub fn request_num_ctx(req: &ChatRequest) -> Option<u32> {
    req.options
        .as_ref()?
        .get("num_ctx")?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
}

/// The `num_predict` a request declares in its generation options, when one is
/// set — the [`request_num_ctx`] mirror for the output side.
pub fn request_num_predict(req: &ChatRequest) -> Option<u32> {
    req.options
        .as_ref()?
        .get("num_predict")?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
}

/// The `/api/chat` request body, serialized from the typed request. `stream` is set
/// by the caller (`false` for [`LocalModelClient::chat`], `true` for the streaming
/// path). Pure, so the wire contract is unit-testable without a live daemon.
/// `think: Some(false)` serializes as an explicit `"think": false` — never skipped —
/// so a non-thinking stage actually reaches the wire as one.
#[derive(Debug, Serialize)]
struct ChatWire<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_alive: Option<i64>,
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
        keep_alive: req.keep_alive,
    };
    // These are plain owned/borrowed values; serialization cannot fail.
    serde_json::to_value(&wire).expect("local chat request is serializable")
}

/// The non-streaming `/api/chat` reply, trimmed to the fields the caller needs.
#[derive(Debug, Deserialize)]
struct ChatReplyWire {
    message: ChatReplyMessage,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
    #[serde(default)]
    done_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatReplyMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    tool_calls: Option<Value>,
}

/// Shape a non-streaming `/api/chat` response body into a [`ChatResponse`]. Pure, so
/// the envelope contract is testable without a live call. An empty `thinking` string
/// collapses to `None` so callers don't distinguish "" from absent.
fn parse_chat_reply(body: &str) -> Result<ChatResponse> {
    let wire: ChatReplyWire = serde_json::from_str(body)
        .map_err(|e| anyhow::Error::new(e).context(RetryClass::SchemaParse))
        .context("parsing local chat response JSON")?;
    Ok(ChatResponse {
        content: wire.message.content,
        thinking: wire.message.thinking.filter(|t| !t.is_empty()),
        prompt_eval_count: wire.prompt_eval_count,
        eval_count: wire.eval_count,
        done_reason: wire.done_reason,
        tool_calls: wire
            .message
            .tool_calls
            .filter(|t| !matches!(t, Value::Array(a) if a.is_empty())),
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
    deadline: DeadlinePolicy,
}

impl LocalModelClient {
    /// Build a client for one daemon endpoint (e.g. `http://localhost:11434`). A
    /// trailing slash is trimmed so a joined path's leading slash doesn't double up.
    /// The builder-level timeout is the backstop the probes ride. Non-streaming
    /// chat calls set their derived total deadline per request; streaming calls
    /// build a client whose blocking-side timeout is their derived idle bound
    /// ([`DeadlinePolicy`]).
    pub fn new(endpoint: impl Into<String>) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .context("building the local-model HTTP client")?;
        Ok(Self {
            http,
            base_url: normalize_endpoint(&endpoint.into()),
            progress: RunContext::noop(),
            deadline: DeadlinePolicy::DEFAULT,
        })
    }

    /// Attach a live run context so each call streams a tracker row (and, for the
    /// streaming path, tokens / reasoning). Without it the client stays no-op.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// The attached run context (no-op by default) — the bounded retry-once
    /// gate's cancellation checks and tracker rows ride the same seam as the
    /// calls themselves.
    pub(crate) fn progress(&self) -> &RunContext {
        &self.progress
    }

    /// Test seam: swap the deadline policy so a wire test can trip a deadline in
    /// milliseconds rather than the production policy's hours.
    #[cfg(test)]
    fn with_deadline_policy(mut self, policy: DeadlinePolicy) -> Self {
        self.deadline = policy;
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Build the streaming transport with its derived bound on the blocking side.
    ///
    /// `RequestBuilder::timeout` is an async total deadline in reqwest 0.12: it
    /// remains armed while the whole response body streams. The blocking builder's
    /// timeout is intentionally different — reqwest waits up to this duration for
    /// the headers and then afresh for every body read. A request-specific client
    /// therefore preserves the variable reservation-derived bound without cutting
    /// off a healthy stream whose cumulative generation time exceeds it.
    fn streaming_http(deadline: Duration) -> Result<reqwest::blocking::Client> {
        reqwest::blocking::Client::builder()
            .timeout(deadline)
            .build()
            .context("building the streaming local-model HTTP client")
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
        let deadline = self.deadline.request_deadline(req, false);
        let body = build_chat_body(req, false);
        let resp = self
            .http
            .post(self.url(CHAT_PATH))
            .timeout(deadline)
            .json(&body)
            .send()
            .context("sending local chat request")
            .map_err(|e| name_deadline_trip(e, deadline))?;
        let status = resp.status();
        let text = resp
            .text()
            .context("reading local chat response body")
            .map_err(|e| name_deadline_trip(e, deadline))?;
        if !status.is_success() {
            // Rooted in the class marker so the bounded retry-once classifies
            // without string-matching; the message stays the display.
            return Err(anyhow::Error::new(RetryClass::DaemonStatus)
                .context(format!("local model returned {status}: {text}")));
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
        let deadline = self.deadline.request_deadline(req, true);
        let body = build_chat_body(req, true);
        let http = Self::streaming_http(deadline)?;
        let resp = http
            .post(self.url(CHAT_PATH))
            .json(&body)
            .send()
            .context("sending local chat request")
            .map_err(|e| name_deadline_trip(e, deadline))?;
        let status = resp.status();
        if !status.is_success() {
            // The status is the diagnosis; the body is detail. A body read that
            // itself fails keeps its live error chain rooted — a stalled error
            // body is a deadline trip and must classify as one (never retried),
            // not as a retryable daemon status — while the message still leads
            // with the status and names the deadline.
            let text = match resp.text() {
                Ok(text) => text,
                Err(e) => {
                    let e = anyhow::Error::from(e);
                    let named = if is_transport_timeout(&e) {
                        deadline_reached(deadline)
                    } else {
                        e.to_string()
                    };
                    return Err(e.context(format!(
                        "local model returned {status}: (error body unreadable: {named})"
                    )));
                }
            };
            return Err(anyhow::Error::new(RetryClass::DaemonStatus)
                .context(format!("local model returned {status}: {text}")));
        }
        stream_chat_response(std::io::BufReader::new(resp), &self.progress, role)
            .map_err(|e| name_deadline_trip(e, deadline))
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

/// Which channels a streamed local chat surfaces to the tracker — a superset of the
/// cloud `model_agent::StreamRole` (`Main` / `Analyst` shared; `Step` and `Silent`
/// are local-only). All roles accumulate the full envelope (the parse source of
/// truth); they differ in what they *stream*.
#[derive(Debug, Clone, Copy)]
pub enum StreamRole<'a> {
    /// Stream the decoded content (`agent_token`) and reasoning (`agent_thinking`).
    Main,
    /// Stream reasoning only, posture-tagged (`analyst_thinking`).
    Analyst(&'a str),
    /// Stream reasoning only, scoped to one tracker step by its key
    /// (`step_thinking`) — the portfolio per-holding interpretation stages, whose
    /// reasoning renders on the holding's own "Analyze {SYM}" step.
    Step(&'a str),
    /// Stream nothing — accumulate silently (structured stages with no console value).
    Silent,
}

/// Route a coalesced reasoning chunk to the channel the role selects.
fn emit_thinking(progress: &RunContext, role: StreamRole<'_>, delta: String) {
    match role {
        StreamRole::Main => progress.agent_thinking(delta),
        StreamRole::Analyst(posture) => progress.analyst_thinking(posture, delta),
        StreamRole::Step(step) => progress.step_thinking(step, delta),
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
    let mut prompt_eval_count = None;
    let mut eval_count = None;
    let mut done_reason = None;

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
            return Err(anyhow::Error::new(RetryClass::Stream)
                .context(format!("local model stream error: {err}")));
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
            // The terminal chunk carries the run counters (`prompt_eval_count` …)
            // and the stop reason — a `num_predict`/length stop still ends with
            // `done: true`, so this is where a truncation becomes visible.
            prompt_eval_count = event.get("prompt_eval_count").and_then(Value::as_u64);
            eval_count = event.get("eval_count").and_then(Value::as_u64);
            done_reason = event
                .get("done_reason")
                .and_then(Value::as_str)
                .map(str::to_string);
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
        // Marked for the bounded retry-once; the cancel bail above deliberately
        // is not — a cancelled stream must never re-attempt.
        return Err(anyhow::Error::new(RetryClass::Stream)
            .context("local model stream ended before completion"));
    }
    Ok(ChatResponse {
        content,
        thinking: (!thinking.is_empty()).then_some(thinking),
        prompt_eval_count,
        eval_count,
        done_reason,
        // The streaming path serves the prose/schema stages, which pass no
        // tools; the research loop's tool turns ride the non-streaming call.
        tool_calls: None,
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

    // The shared FMP / FRED credential presence joins the local gate
    // (`docs/portfolio-workflow.md` §Step 1): the per-holding fundamentals surface
    // (FMP) and the run-level rate anchors (FRED) are load-bearing engine inputs, so
    // a missing key blocks at the gate rather than failing hours into a run.
    // Presence-only (no live probe), surfaced through the **existing**
    // missing-provider-credentials category — no new category — while Tavily
    // deliberately does not gate the local suite (which does not use it — SearXNG-only).
    let mut missing_creds: Vec<&str> = Vec::new();
    if config::present(&cfg.fmp_api_key).is_none() {
        missing_creds.push("Financial Modeling Prep");
    }
    if config::present(&cfg.fred_api_key).is_none() {
        missing_creds.push("FRED");
    }
    if !missing_creds.is_empty() {
        categories.push(WarningCategory {
            kind: WarningKind::ProviderCredentials,
            title: "Provider credentials".to_string(),
            items: vec![format!("Missing for {}.", config::join_list(&missing_creds))],
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
            // The shared FMP / FRED credentials joined the local gate with the full
            // Portfolio slice (`docs/portfolio-workflow.md` §Step 1).
            fmp_api_key: Some("fmp-key".into()),
            fred_api_key: Some("fred-key".into()),
            ..AppConfig::default()
        }
    }

    // ---- pure request/response shaping ----

    #[test]
    fn build_chat_body_carries_model_messages_format_and_stream() {
        let mut req = ChatRequest::new("qwen3.5:122b", vec![ChatMessage::user("hi")]);
        req.format_schema = Some(serde_json::json!({ "type": "object" }));
        req.think = Some(true);
        let body = build_chat_body(&req, true);
        assert_eq!(body["model"], "qwen3.5:122b");
        assert_eq!(body["stream"], true);
        assert_eq!(body["think"], true);
        assert_eq!(body["format"]["type"], "object");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hi");
    }

    #[test]
    fn build_chat_body_omits_unset_optional_fields() {
        let req = ChatRequest::new("m", vec![ChatMessage::user("x")]);
        let body = build_chat_body(&req, false);
        assert_eq!(body["stream"], false);
        // Unset think, format, tools, options, keep_alive are all omitted entirely.
        let obj = body.as_object().unwrap();
        assert!(!obj.contains_key("think"), "{obj:?}");
        assert!(!obj.contains_key("format"), "{obj:?}");
        assert!(!obj.contains_key("tools"), "{obj:?}");
        assert!(!obj.contains_key("options"), "{obj:?}");
        assert!(!obj.contains_key("keep_alive"), "{obj:?}");
    }

    #[test]
    fn build_chat_body_serializes_an_explicit_think_false_and_keep_alive() {
        // The F3 regression guard: `Some(false)` must reach the wire as a literal
        // `"think": false` (never skipped), or a non-thinking stage silently rides
        // the model's thinking-on default. `keep_alive: -1` rides beside it as the
        // stay-resident posture.
        let mut req = ChatRequest::new("m", vec![ChatMessage::user("x")]);
        req.think = Some(false);
        req.keep_alive = Some(-1);
        let body = build_chat_body(&req, false);
        assert_eq!(body["think"], false);
        assert_eq!(body["keep_alive"], -1);
    }

    #[test]
    fn option_profiles_match_the_vendor_sampling_rows() {
        // The two rows from `docs/local-model-operations.md §Sampling settings`,
        // exact — and never greedy (temperature 0 is explicitly warned against).
        let think = options::thinking_general(131_072, 65_536);
        assert_eq!(think["temperature"], 1.0);
        assert_eq!(think["top_p"], 0.95);
        assert_eq!(think["top_k"], 20);
        assert_eq!(think["min_p"], 0.0);
        assert_eq!(think["presence_penalty"], 1.5);
        assert_eq!(think["num_ctx"], 131_072);
        assert_eq!(think["num_predict"], 65_536);

        let fast = options::non_thinking_general(32_768, 8_192);
        assert_eq!(fast["temperature"], 0.7);
        assert_eq!(fast["top_p"], 0.8);
        assert_eq!(fast["top_k"], 20);
        assert_eq!(fast["min_p"], 0.0);
        assert_eq!(fast["presence_penalty"], 1.5);
        assert_eq!(fast["num_ctx"], 32_768);
        assert_eq!(fast["num_predict"], 8_192);
        for profile in [&think, &fast] {
            assert_ne!(profile["temperature"], 0.0, "greedy decoding is forbidden");
        }
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
    fn local_gate_blocks_on_missing_fmp_or_fred_via_the_shared_category() {
        // The credential precondition of `docs/portfolio-workflow.md` §Step 1: FMP +
        // FRED presence joins the local gate through the existing
        // missing-provider-credentials category (no new category); Tavily
        // deliberately does not gate the local suite.
        let mut cfg = local_cfg();
        cfg.fred_api_key = None;
        cfg.tavily_api_key = None;
        let report = local_gate(&cfg, &DaemonProbe::Reachable { missing: vec![] });
        assert!(report.is_blocked);
        let cat = report
            .categories
            .iter()
            .find(|c| c.kind == WarningKind::ProviderCredentials)
            .expect("the shared provider-credentials category");
        assert!(cat.items[0].contains("FRED"), "{:?}", cat.items);
        assert!(!cat.items[0].contains("Tavily"), "Tavily never gates: {:?}", cat.items);
        // With both present, no credential category fires.
        let clean = local_gate(&local_cfg(), &DaemonProbe::Reachable { missing: vec![] });
        assert!(clean.categories.iter().all(|c| c.kind != WarningKind::ProviderCredentials));
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

    // ---- transport deadline (2026-08-24 review §C1) ----

    /// A thinking-stage request as the pipeline builds it: the interpret context
    /// and the thinking reservation.
    fn thinking_request() -> ChatRequest {
        let mut req = ChatRequest::new("m", vec![ChatMessage::user("think hard")]);
        req.options = Some(options::thinking_general(131_072, 65_536));
        req
    }

    /// A policy whose floors are so high every derived term rounds to a millisecond,
    /// leaving `floor` as the deadline — milliseconds, so a wire test trips it fast.
    fn instant_policy(floor_ms: u64) -> DeadlinePolicy {
        DeadlinePolicy {
            prefill_floor_tok_s: u32::MAX,
            decode_floor_tok_s: u32::MAX,
            floor: Duration::from_millis(floor_ms),
        }
    }

    #[test]
    fn deadline_policy_derives_from_the_request_reservations() {
        let policy = DeadlinePolicy::DEFAULT;
        // Non-streaming thinking (the 6c research turn): 131,072 / 100 tok/s of
        // prefill plus 65,536 / 12 tok/s of decode — ~113 minutes, an order past the
        // old fixed ten, and past the reservation at the drafted floor by design.
        let thinking = policy.request_deadline(&thinking_request(), false);
        assert_eq!(thinking, Duration::from_millis(1_310_720 + 5_461_334));
        assert!(
            thinking > Duration::from_secs(110 * 60) && thinking < Duration::from_secs(115 * 60)
        );
        // Streaming thinking (interpret / role-risk / action): the prefill term alone —
        // tokens arrive as they generate — which then also bounds each body read.
        let streamed = policy.request_deadline(&thinking_request(), true);
        assert_eq!(streamed, Duration::from_millis(1_310_720));
        // Distillation on a genuinely distinct fast model, at the distill context:
        // ~17 minutes.
        let mut distill = ChatRequest::new("m", vec![ChatMessage::user("condense")]);
        distill.options = Some(options::non_thinking_general(32_768, 8_192));
        assert_eq!(
            policy.request_deadline(&distill, false),
            Duration::from_millis(327_680 + 682_667)
        );
        // Distillation on the default roster — the fast tier fallen back to the
        // reasoner — shares the interpret context (one `num_ctx` per model), so its
        // deadline is ~33 minutes, not 17.
        let mut distill_default = ChatRequest::new("m", vec![ChatMessage::user("condense")]);
        distill_default.options = Some(options::non_thinking_general(131_072, 8_192));
        assert_eq!(
            policy.request_deadline(&distill_default, false),
            Duration::from_millis(1_310_720 + 682_667)
        );
        // No reservations (a probe-shaped request) rides the backstop exactly.
        let bare = ChatRequest::new("m", vec![ChatMessage::user("x")]);
        assert_eq!(policy.request_deadline(&bare, false), DEFAULT_TIMEOUT);
        assert_eq!(policy.request_deadline(&bare, true), DEFAULT_TIMEOUT);
        // A small reservation never goes under the backstop.
        let mut small = ChatRequest::new("m", vec![ChatMessage::user("x")]);
        small.options = Some(options::non_thinking_general(2_048, 256));
        assert_eq!(policy.request_deadline(&small, false), DEFAULT_TIMEOUT);
    }

    #[test]
    fn deadline_policy_floors_never_divide_by_zero() {
        let zero = DeadlinePolicy {
            prefill_floor_tok_s: 0,
            decode_floor_tok_s: 0,
            floor: Duration::from_secs(1),
        };
        // A zero floor reads as one token per second rather than panicking.
        let d = zero.request_deadline(&thinking_request(), false);
        assert_eq!(d, Duration::from_secs(131_072 + 65_536));
    }

    #[test]
    fn chat_trips_the_derived_deadline_when_the_daemon_never_answers() {
        // The daemon reads the request and goes quiet for longer than the deadline
        // — the header wait is where a non-streaming generation lives, so this is
        // the exact shape a chain past the old fixed ten minutes produced.
        let server = MockHttp::serve(vec![Canned::Delay {
            for_ms: 1_500,
            then: Box::new(Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"message":{"role":"assistant","content":"late"}}"#,
            }),
        }]);
        let client = LocalModelClient::new(&server.base_url)
            .unwrap()
            .with_deadline_policy(instant_policy(200));
        let err = client.chat(&thinking_request()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("transport deadline of 1 min reached"),
            "deadline trip must be named, got: {msg}"
        );
        assert!(msg.contains("daemon stalled"), "{msg}");
        assert_eq!(
            server.attempts(),
            1,
            "a deadline trip is never retried here"
        );
    }

    #[test]
    fn chat_non_streaming_keeps_a_total_deadline_across_an_active_body() {
        // Non-streaming generation normally precedes the headers, but the wire
        // contract is still total: unlike the streaming path, active body chunks
        // must not reset this request-wide deadline.
        let server = MockHttp::serve(vec![Canned::DripBody {
            status: 200,
            chunks: vec![
                ("{\"message\":{\"role\":\"assistant\",", 125),
                ("\"content\":\"late\"", 125),
                ("}}", 0),
            ],
        }]);
        let client = LocalModelClient::new(&server.base_url)
            .unwrap()
            .with_deadline_policy(instant_policy(200));
        let err = client.chat(&thinking_request()).unwrap_err();
        assert!(
            err.to_string().contains("transport deadline"),
            "total trip must be named, got: {err:#}"
        );
        assert_eq!(server.attempts(), 1);
    }

    #[test]
    fn chat_streaming_trips_the_derived_deadline_before_the_first_chunk() {
        // With `stream: true` nothing arrives before the first token, so prefill
        // sits inside the header wait; a daemon silent past the deadline trips it.
        let server = MockHttp::serve(vec![Canned::Delay {
            for_ms: 1_500,
            then: Box::new(Canned::Reply {
                status: 200,
                headers: vec![],
                body: "{\"message\":{\"role\":\"assistant\",\"content\":\"late\"},\"done\":true}\n",
            }),
        }]);
        let client = LocalModelClient::new(&server.base_url)
            .unwrap()
            .with_deadline_policy(instant_policy(200));
        let err = client
            .chat_streaming(&thinking_request(), StreamRole::Main)
            .unwrap_err();
        assert!(err.to_string().contains("transport deadline"), "{err:#}");
    }

    #[test]
    fn chat_streaming_active_chunks_can_outlive_the_idle_deadline() {
        // Each 125 ms gap stays under the 200 ms idle bound, while the complete
        // response takes more than 200 ms. A request-level total timeout rejects
        // this healthy stream; the blocking-side per-read bound must accept it.
        let server = MockHttp::serve(vec![Canned::DripBody {
            status: 200,
            chunks: vec![
                (
                    "{\"message\":{\"role\":\"assistant\",\"content\":\"still \"}}\n",
                    125,
                ),
                (
                    "{\"message\":{\"role\":\"assistant\",\"content\":\"working\"}}\n",
                    125,
                ),
                ("{\"message\":{\"role\":\"assistant\"},\"done\":true}\n", 0),
            ],
        }]);
        let client = LocalModelClient::new(&server.base_url)
            .unwrap()
            .with_deadline_policy(instant_policy(200));
        let response = client
            .chat_streaming(&thinking_request(), StreamRole::Main)
            .unwrap();
        assert_eq!(response.content, "still working");
        assert_eq!(server.attempts(), 1);
    }

    #[test]
    fn chat_streaming_trips_the_idle_deadline_when_the_stream_goes_silent() {
        // Headers and a first chunk arrive, then nothing: the per-read bound catches
        // the stall and the trip is named through the io-error chain, not left as an
        // opaque read failure. The partial content must not leak out as `Ok`.
        let server = MockHttp::serve(vec![Canned::StallMidBody {
            status: 200,
            partial: "{\"message\":{\"role\":\"assistant\",\"content\":\"par\"}}\n",
            for_ms: 1_500,
        }]);
        let client = LocalModelClient::new(&server.base_url)
            .unwrap()
            .with_deadline_policy(instant_policy(200));
        let err = client
            .chat_streaming(&thinking_request(), StreamRole::Main)
            .unwrap_err();
        assert!(
            err.to_string().contains("transport deadline"),
            "idle trip must be named, got: {err:#}"
        );
    }

    #[test]
    fn chat_streaming_names_the_deadline_when_an_error_body_stalls() {
        // A non-2xx whose body then goes silent: the status stays the headline and
        // the stalled body read names the deadline instead of collapsing to blank.
        let server = MockHttp::serve(vec![Canned::StallMidBody {
            status: 500,
            partial: "{\"error\":\"runner ",
            for_ms: 1_500,
        }]);
        let client = LocalModelClient::new(&server.base_url)
            .unwrap()
            .with_deadline_policy(instant_policy(200));
        let err = client
            .chat_streaming(&thinking_request(), StreamRole::Main)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("local model returned 500"), "{msg}");
        assert!(msg.contains("transport deadline"), "{msg}");
        // The stalled error body is a deadline trip: it must never classify for
        // the bounded retry-once, whatever status headline it carries (a
        // DaemonStatus marker here would spend another idle span on a stalled
        // daemon — external review finding, 2026-08-27).
        assert!(is_transport_timeout(&err));
        assert_eq!(retry_class(&err), None);
    }

    #[test]
    fn chat_paths_succeed_under_the_production_policy_against_a_prompt_daemon() {
        // The control: the same requests under `DeadlinePolicy::DEFAULT` complete
        // when the daemon answers — the derived deadline is a ceiling, never a gate.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"message":{"role":"assistant","content":"now"}}"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "{\"message\":{\"role\":\"assistant\",\"content\":\"now\"},\"done\":true}\n",
            },
        ]);
        let client = LocalModelClient::new(&server.base_url).unwrap();
        assert_eq!(client.chat(&thinking_request()).unwrap().content, "now");
        assert_eq!(
            client
                .chat_streaming(&thinking_request(), StreamRole::Main)
                .unwrap()
                .content,
            "now"
        );
        assert_eq!(server.attempts(), 2);
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
        // The non-2xx carries the retry class for the bounded retry-once.
        assert_eq!(retry_class(&err), Some(RetryClass::DaemonStatus));
    }

    // ---- the bounded retry-once (classification + gate) ----

    #[test]
    fn retry_class_finds_the_marker_through_context_layers() {
        // Rooted marker (the daemon-status / stream sites), re-wrapped the way
        // call sites wrap.
        let rooted = anyhow::Error::new(RetryClass::DaemonStatus)
            .context("local model returned 500")
            .context("research turn failed");
        assert_eq!(retry_class(&rooted), Some(RetryClass::DaemonStatus));
        // Context marker over a source error (the schema-parse sites).
        let serde_err = serde_json::from_str::<ChatReplyWire>("not json").unwrap_err();
        let marked = anyhow::Error::new(serde_err)
            .context(RetryClass::SchemaParse)
            .context("parsing interpretation JSON: not json");
        assert_eq!(retry_class(&marked), Some(RetryClass::SchemaParse));
    }

    #[test]
    fn retry_class_refuses_unmarked_failures() {
        // The whitelist is the contract: a refusal-shaped or unknown failure and
        // a length-stop bail (both unmarked) never classify.
        assert_eq!(retry_class(&anyhow::anyhow!("model declined the task")), None);
        assert_eq!(
            retry_class(&anyhow::anyhow!(
                "stage: response truncated at the output reservation"
            )),
            None
        );
    }

    #[test]
    fn retry_class_names_a_connect_failure_transport_but_a_deadline_trip_none() {
        // A freshly freed port: the connect is refused — a reqwest error that is
        // not a timeout, so it classifies as transport.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let client = LocalModelClient::new(format!("http://127.0.0.1:{port}")).unwrap();
        let err = client
            .chat(&ChatRequest::new("m", vec![ChatMessage::user("x")]))
            .unwrap_err();
        assert_eq!(retry_class(&err), Some(RetryClass::Transport));

        // A deadline trip also bottoms out in reqwest, but must classify None —
        // it is attributable, and a retry would double a multi-hour wait.
        let server = MockHttp::serve(vec![Canned::Delay {
            for_ms: 1_500,
            then: Box::new(Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"message":{"role":"assistant","content":"late"}}"#,
            }),
        }]);
        let client = LocalModelClient::new(&server.base_url)
            .unwrap()
            .with_deadline_policy(instant_policy(200));
        let err = client.chat(&thinking_request()).unwrap_err();
        assert!(is_transport_timeout(&err));
        assert_eq!(retry_class(&err), None);
    }

    #[test]
    fn stream_failures_classify_but_a_cancelled_stream_does_not() {
        let (_rec, ctx) = recording_ctx();
        // Ends without a done chunk: truncation.
        let err = stream_chat_response(
            std::io::Cursor::new(b"{\"message\":{\"content\":\"par\"}}\n".to_vec()),
            &ctx,
            StreamRole::Silent,
        )
        .unwrap_err();
        assert_eq!(retry_class(&err), Some(RetryClass::Stream));
        // An explicit error chunk.
        let err = stream_chat_response(
            std::io::Cursor::new(b"{\"error\":\"runner crashed\"}\n".to_vec()),
            &ctx,
            StreamRole::Silent,
        )
        .unwrap_err();
        assert_eq!(retry_class(&err), Some(RetryClass::Stream));
        // A cancelled stream must never classify: cancellation is intentional.
        let cancel = Arc::new(AtomicBool::new(true));
        let rec = Arc::new(RecordingReporter::default());
        let cancelled = RunContext::new("run", rec, cancel);
        let err = stream_chat_response(
            std::io::Cursor::new(b"{\"message\":{\"content\":\"x\"}}\n".to_vec()),
            &cancelled,
            StreamRole::Silent,
        )
        .unwrap_err();
        assert_eq!(retry_class(&err), None);
    }

    #[test]
    fn retry_once_reruns_a_marked_failure_and_records_the_event() {
        let (rec, ctx) = recording_ctx();
        let retry = RetryOnce::without_delay();
        let calls = std::cell::Cell::new(0u32);
        let out = retry.run(&ctx, "interpret TEST", || {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Err(anyhow::Error::new(RetryClass::DaemonStatus)
                    .context("local model returned 500: boom"))
            } else {
                Ok("ok")
            }
        });
        assert_eq!(out.unwrap(), "ok");
        assert_eq!(calls.get(), 2);
        let events = retry.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].stage, "interpret TEST");
        assert_eq!(events[0].cause, RetryClass::DaemonStatus.to_string());
        assert!(retry.take_events().is_empty(), "the drain empties the record");
        // The fired retry left its own tracker row naming the re-attempt.
        let noted = rec.messages().iter().any(|m| {
            matches!(
                &m.event,
                ProgressEvent::RequestFinished { name, detail, .. }
                    if name == "Local model retry"
                        && detail.as_deref().is_some_and(|d| d.contains("retrying once"))
            )
        });
        assert!(noted, "a fired retry must be visible on the tracker");
    }

    #[test]
    fn retry_once_passes_an_unmarked_failure_straight_through() {
        let (_rec, ctx) = recording_ctx();
        let retry = RetryOnce::without_delay();
        let calls = std::cell::Cell::new(0u32);
        let out: Result<()> = retry.run(&ctx, "interpret TEST", || {
            calls.set(calls.get() + 1);
            Err(anyhow::anyhow!("model declined the task"))
        });
        assert!(out.is_err());
        assert_eq!(calls.get(), 1, "an unclassified failure never re-attempts");
        assert!(retry.take_events().is_empty());
    }

    #[test]
    fn retry_once_annotates_the_second_failure_with_the_first_class() {
        let (_rec, ctx) = recording_ctx();
        let retry = RetryOnce::without_delay();
        let calls = std::cell::Cell::new(0u32);
        let out: Result<()> = retry.run(&ctx, "action TEST", || {
            calls.set(calls.get() + 1);
            Err(anyhow::Error::new(RetryClass::EmptyCompletion)
                .context("action TEST: the model returned an empty completion body"))
        });
        let err = out.unwrap_err();
        assert_eq!(calls.get(), 2, "exactly one re-attempt, never more");
        assert!(
            err.to_string()
                .contains("failed again after one retry (empty completion body on the first attempt)"),
            "{err}"
        );
        assert_eq!(retry.take_events().len(), 1);
    }

    #[test]
    fn retry_once_refuses_into_a_cancelled_run() {
        let rec = Arc::new(RecordingReporter::default());
        let cancel = Arc::new(AtomicBool::new(true));
        let ctx = RunContext::new("run", rec, cancel);
        let retry = RetryOnce::without_delay();
        let calls = std::cell::Cell::new(0u32);
        let out: Result<()> = retry.run(&ctx, "interpret TEST", || {
            calls.set(calls.get() + 1);
            Err(anyhow::Error::new(RetryClass::DaemonStatus).context("local model returned 502"))
        });
        assert!(out.is_err());
        assert_eq!(calls.get(), 1, "a cancelled run never re-attempts");
        assert!(retry.take_events().is_empty());
    }

    #[test]
    fn a_daemon_hiccup_is_absorbed_by_one_retry_at_the_wire() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 500,
                headers: vec![],
                body: "hiccup",
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"message":{"role":"assistant","content":"ok"}}"#,
            },
        ]);
        let client = LocalModelClient::new(&server.base_url).unwrap();
        let retry = RetryOnce::without_delay();
        let req = ChatRequest::new("m", vec![ChatMessage::user("x")]);
        let resp = retry
            .run(client.progress(), "interpret TEST", || client.chat(&req))
            .unwrap();
        assert_eq!(resp.content, "ok");
        assert_eq!(server.attempts(), 2);
        assert_eq!(retry.take_events().len(), 1);
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
    fn stream_decoder_step_role_streams_step_scoped_thinking() {
        let (rec, ctx) = recording_ctx();
        let ndjson = concat!(
            r#"{"message":{"content":"{\"grade\":\"B\"}"}}"#,
            "\n",
            r#"{"message":{"thinking":"the trim balances concentration risk"}}"#,
            "\n",
            r#"{"done":true}"#,
            "\n",
        );
        let resp =
            stream_chat_response(ndjson.as_bytes(), &ctx, StreamRole::Step("holding-AAPL")).unwrap();
        assert_eq!(resp.content, r#"{"grade":"B"}"#); // still accumulated
        let msgs = rec.messages();
        // No content tokens stream for a structured stage...
        assert!(!msgs
            .iter()
            .any(|m| matches!(&m.event, ProgressEvent::AgentToken { .. })));
        // ...but its reasoning streams onto the owning step's channel.
        assert!(msgs.iter().any(|m| matches!(
            &m.event,
            ProgressEvent::StepThinking { step, .. } if step == "holding-AAPL"
        )));
    }

    #[test]
    fn chat_reply_captures_prompt_eval_count_and_tolerates_absence() {
        // The context-fit instrumentation: the count rides the reply when the daemon
        // reports it, and its absence is `None`, never a parse failure.
        let with = parse_chat_reply(
            r#"{"message":{"content":"ok"},"prompt_eval_count":117964,"eval_count":42}"#,
        )
        .unwrap();
        assert_eq!(with.prompt_eval_count, Some(117_964));
        let without = parse_chat_reply(r#"{"message":{"content":"ok"}}"#).unwrap();
        assert_eq!(without.prompt_eval_count, None);
    }

    #[test]
    fn stream_decoder_captures_prompt_eval_count_from_the_done_chunk() {
        let (_rec, ctx) = recording_ctx();
        let ndjson = concat!(
            r#"{"message":{"content":"body"}}"#,
            "\n",
            r#"{"message":{"content":""},"done":true,"prompt_eval_count":131000,"eval_count":9,"done_reason":"stop"}"#,
            "\n",
        );
        let resp = stream_chat_response(ndjson.as_bytes(), &ctx, StreamRole::Silent).unwrap();
        assert_eq!(resp.content, "body");
        assert_eq!(resp.prompt_eval_count, Some(131_000));
        assert_eq!(resp.eval_count, Some(9));
        assert_eq!(resp.done_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn tool_calls_parse_from_a_reply_and_empty_arrays_collapse_to_none() {
        // The research loop's turn protocol: a reply carrying tool_calls
        // surfaces them raw; an empty array reads as a no-tools turn.
        let with = parse_chat_reply(
            r#"{"message":{"content":"","tool_calls":[{"function":{"name":"web_search","arguments":{"q":"x"}}}]}}"#,
        )
        .unwrap();
        let calls = with.tool_calls.expect("tool calls surface");
        assert_eq!(calls[0]["function"]["name"], "web_search");
        let empty = parse_chat_reply(r#"{"message":{"content":"done","tool_calls":[]}}"#).unwrap();
        assert_eq!(empty.tool_calls, None);
        let none = parse_chat_reply(r#"{"message":{"content":"done"}}"#).unwrap();
        assert_eq!(none.tool_calls, None);

        // The wire side: a tool-result message serializes with role "tool",
        // and an echoed assistant turn carries its tool_calls; plain messages
        // omit the field entirely.
        let req = ChatRequest::new(
            "m",
            vec![
                ChatMessage::assistant_with_tool_calls("", serde_json::json!([{"f": 1}])),
                ChatMessage::tool("result body"),
                ChatMessage::user("next"),
            ],
        );
        let body = build_chat_body(&req, false);
        assert_eq!(body["messages"][0]["tool_calls"][0]["f"], 1);
        assert_eq!(body["messages"][1]["role"], "tool");
        assert!(body["messages"][1].get("tool_calls").is_none());
        assert!(body["messages"][2].get("tool_calls").is_none());
    }

    #[test]
    fn a_length_stop_is_a_done_stream_with_the_reason_captured() {
        // Ollama ends a `num_predict` stop with `done: true` and HTTP 200 — the
        // stream is NOT the "ended before completion" truncation, so the decoder
        // returns Ok and `done_reason` is the only truncation witness.
        let (_rec, ctx) = recording_ctx();
        let ndjson = concat!(
            r#"{"message":{"content":"partial"}}"#,
            "\n",
            r#"{"message":{"content":""},"done":true,"eval_count":65536,"done_reason":"length"}"#,
            "\n",
        );
        let resp = stream_chat_response(ndjson.as_bytes(), &ctx, StreamRole::Silent).unwrap();
        assert_eq!(resp.content, "partial");
        assert_eq!(resp.done_reason.as_deref(), Some("length"));
        assert_eq!(resp.eval_count, Some(65_536));
    }

    #[test]
    fn request_num_ctx_reads_the_options_field() {
        let mut req = ChatRequest::new("m", vec![ChatMessage::user("x")]);
        assert_eq!(request_num_ctx(&req), None); // no options at all
        assert_eq!(request_num_predict(&req), None);
        req.options = Some(options::thinking_general(131_072, 65_536));
        assert_eq!(request_num_ctx(&req), Some(131_072));
        assert_eq!(request_num_predict(&req), Some(65_536));
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

    // ---- length-stop classification ----

    #[test]
    fn length_stop_reading_attributes_only_on_full_counts() {
        use LengthStopReading::*;
        // At or past the reservation: the reservation bound.
        assert_eq!(length_stop_reading(Some(65_536), Some(65_536)), AtReservation);
        assert_eq!(length_stop_reading(Some(70_000), Some(65_536)), AtReservation);
        // Well under a declared reservation: context filled first.
        assert_eq!(length_stop_reading(Some(1_200), Some(65_536)), UnderReservation);
        // A missing count or reservation never earns a confident lever.
        assert_eq!(length_stop_reading(None, Some(65_536)), Unattributed);
        assert_eq!(length_stop_reading(Some(1_200), None), Unattributed);
        assert_eq!(length_stop_reading(None, None), Unattributed);
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
