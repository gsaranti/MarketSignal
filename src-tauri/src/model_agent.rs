//! Real OpenAI/Anthropic adapter for the `MainAgent` stage.
//!
//! This is the first real model call behind the `MainAgent` trait — it replaces
//! `StubMainAgent` in the live command while leaving the trait, the pipeline,
//! and the offline tests unchanged. The adapter stays a pure structured-in /
//! structured-out boundary (`agent.rs`): the model returns the analytical fields
//! plus the Markdown body; the application layer mints `report_id`,
//! `report_type`, and `created_at` so a fabricated timestamp can never reach the
//! pipeline's RFC3339 parse.
//!
//! The HTTP call is synchronous (`reqwest::blocking`) to keep the agent trait
//! sync — the broader async machinery lands later with the research executor,
//! which is where it is actually needed. The seed of the future
//! `adapters::models` module lives here.
//!
//! The agent's `MainAgentInput` now carries the Step-6 baseline market-data scan
//! (`data_sources`); this adapter serializes it into the user message so the
//! report is grounded in this run's live data. The rest of the condensed packet
//! (news clusters, deep research, vector memory) joins it as later slices land.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::agent::{
    MainAgent, MainAgentInput, MainAgentOutput, MarketCycle, ReportSummary, RiskPosture,
    ThesisStance,
};
use crate::baseline_delta::BaselineDeltas;
use crate::data_sources::BaselineMarketData;
use crate::progress::RunContext;

/// Which provider an agent model is served by. Selects the request shape, the
/// auth header, and the endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAi,
    Anthropic,
}

impl Provider {
    /// Human-readable provider name, for grouping the Settings model dropdown
    /// (`docs/configuration.md §Agent Model Configuration`).
    pub fn display_name(&self) -> &'static str {
        match self {
            Provider::OpenAi => "OpenAI",
            Provider::Anthropic => "Anthropic",
        }
    }
}

/// The five user-selectable Main Agent models (`docs/configuration.md §Agent
/// Model Configuration`). Each resolves to a provider and the provider's API
/// model-id string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentModel {
    Gpt5,
    Gpt5Mini,
    ClaudeOpus,
    ClaudeSonnet,
    ClaudeHaiku,
}

impl AgentModel {
    /// Every selectable model, in display order (OpenAI first, then Anthropic).
    /// The single source for the Settings dropdown's options so the frontend
    /// never hard-codes the slug/display-name pairing.
    pub const ALL: [AgentModel; 5] = [
        Self::Gpt5,
        Self::Gpt5Mini,
        Self::ClaudeOpus,
        Self::ClaudeSonnet,
        Self::ClaudeHaiku,
    ];

    /// Parse the config-facing slug (the value carried in
    /// `MARKET_SIGNAL_MAIN_AGENT_MODEL`). Distinct from `model_id`, which is the
    /// provider's wire string.
    pub fn from_config_label(label: &str) -> Result<Self> {
        match label {
            "gpt-5" => Ok(Self::Gpt5),
            "gpt-5-mini" => Ok(Self::Gpt5Mini),
            "claude-opus" => Ok(Self::ClaudeOpus),
            "claude-sonnet" => Ok(Self::ClaudeSonnet),
            "claude-haiku" => Ok(Self::ClaudeHaiku),
            other => bail!(
                "unknown main-agent model {other:?}; expected one of gpt-5, gpt-5-mini, \
                 claude-opus, claude-sonnet, claude-haiku"
            ),
        }
    }

    pub fn provider(&self) -> Provider {
        match self {
            Self::Gpt5 | Self::Gpt5Mini => Provider::OpenAi,
            Self::ClaudeOpus | Self::ClaudeSonnet | Self::ClaudeHaiku => Provider::Anthropic,
        }
    }

    /// The provider's API model-id string sent on the wire.
    pub fn model_id(&self) -> &'static str {
        match self {
            Self::Gpt5 => "gpt-5",
            Self::Gpt5Mini => "gpt-5-mini",
            Self::ClaudeOpus => "claude-opus-4-8",
            Self::ClaudeSonnet => "claude-sonnet-4-6",
            Self::ClaudeHaiku => "claude-haiku-4-5",
        }
    }

    /// The config-facing slug — the inverse of `from_config_label`. Persisted in
    /// `app_settings` and offered as the Settings dropdown's option value.
    pub fn config_label(&self) -> &'static str {
        match self {
            Self::Gpt5 => "gpt-5",
            Self::Gpt5Mini => "gpt-5-mini",
            Self::ClaudeOpus => "claude-opus",
            Self::ClaudeSonnet => "claude-sonnet",
            Self::ClaudeHaiku => "claude-haiku",
        }
    }

    /// Human-readable model name for the Settings UI (`docs/configuration.md
    /// §Agent Model Configuration`).
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Gpt5 => "GPT-5",
            Self::Gpt5Mini => "GPT-5 mini",
            Self::ClaudeOpus => "Claude Opus",
            Self::ClaudeSonnet => "Claude Sonnet",
            Self::ClaudeHaiku => "Claude Haiku",
        }
    }
}

/// Resolved configuration for one adapter: which model, and the key for that
/// model's provider.
pub struct MainAgentConfig {
    pub model: AgentModel,
    pub api_key: String,
}

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The single tool the Anthropic arm forces, and the json_schema name on the
/// OpenAI arm. Both feed the same `ResponseEnvelope`.
const TOOL_NAME: &str = "emit_weekly_report";

/// Generous ceiling for a full report body; small enough that a single
/// non-streaming response returns well within the client's 120s HTTP timeout.
const MAX_TOKENS: u32 = 8192;

const SYSTEM_PROMPT: &str = "You are the Head Market Analyst for Market Signal, a weekly \
market-research publication. You write a single, cohesive weekly market report in one unified \
voice — the Market Signal Thesis — that reads like a professional market publication: \
thesis-driven, forward-looking, and focused on structural developments rather than reactive \
daily commentary.

Ground your analysis in the baseline market data provided with this prompt. That data \
may carry a `gaps` list — series or releases that could not be gathered this run; treat \
each listed item as unavailable rather than inferring or inventing a value for it, and \
acknowledge any material absence rather than writing around it silently.

The baseline also carries two equity-level breadth signals beyond the index and macro \
series. `movers` lists the run's biggest gainers, losers, and most-active names — each row \
has a ticker, price, percent move, and exchange but no sector or instrument type. Most are \
individual companies, so infer the sector from the ticker; but some may be ETFs or \
leveraged / inverse funds (e.g. a 2x or 3x product), and when a ticker is not a single \
company, read it as a flow / positioning signal rather than attributing the move to one \
company or sector. `earnings` lists large-cap companies reporting in the prior-week and \
upcoming window, with estimate-versus-actual where a date has already reported. Read these \
for sector rotation and single-name drivers rather than leaning on the aggregate indices \
alone — but treat them as breadth color, not a stock-picking mandate.

The baseline also carries valuation and finer-rotation context. These valuation snapshots are \
exchange-specific: every row is tagged with its `exchange`, and the baseline gathers both \
NASDAQ-listed (growth / tech-tilted) and NYSE-listed (broader, more value / financials / \
industrials) reads. A P/E is therefore the aggregate for that one exchange's companies, not a \
whole-market multiple — read sector and industry valuations cross-sectionally (which groups \
are rich versus cheap relative to one another, and how the NASDAQ growth read differs from \
the NYSE value read) rather than as an absolute market level. `sector_pe` gives each sector's \
aggregate P/E per exchange, a valuation read to set against the `sectors` performance group. \
`industries` is a finer cut than the broad sectors: per exchange, the run's strongest and \
weakest industries by average daily move, each joined with that industry's aggregate P/E where \
available (`pe` may be null when earnings are non-positive or the snapshot lacked it), so you \
can read which narrow groups are rotating and whether they are richly or cheaply valued as \
they do. Treat this as a level read — whether a group is expensive or cheap right now — not as \
a claim about multiple expansion or de-rating over time, which a single snapshot cannot \
support. `market_risk_premium` is the US equity-risk-premium (a near-static annual constant, \
so read its level, not week-to-week change) — the excess return demanded over the risk-free \
rate, a valuation anchor for how richly equities are priced. Use these to ground the regime \
and strategy reads in valuation, not momentum alone.

Produce the report body as GitHub-flavored Markdown with these sections, in order:
- # Weekly Market Report (title), followed by a short date / report-type line
- ## Header Summary — the 3 to 6 bullets that also populate header_summary_bullets
- ## Market Regime — the risk-posture and market-cycle read
- ## Index Picture — Dow, S&P 500, Nasdaq
- ## Key Market Drivers
- ## Market Signal Thesis — the unified thesis and the conditions that would change it
- ## Retrospective Audit — how prior reports' assumptions and risks held up against market evidence; this section is dynamic — include it only when there is prior-report context to audit, and keep it brief or omit it otherwise rather than inventing one
- ## Investment Strategy — frame where risk and reward look asymmetric; never give buy/sell instructions
- ## Forward Outlook
- ## Watchlist
- ## Sources

Alongside the Markdown, classify the report on three axes — risk_posture (risk-on, risk-off, or \
mixed), market_cycle (late-cycle, recessionary, or recovery), and thesis_stance (bullish, \
bearish, mixed, or uncertain) — and provide header_summary_bullets (matching the Header Summary), \
key_risks, unresolved_questions, and forward_outlook_themes. Any of the three arrays may be empty.";

const USER_PROMPT: &str =
    "Write this week's Market Signal weekly market report, including its structured summary.";

/// Build the user message: the standing instruction plus, when present, the
/// Step-6 baseline market-data scan serialized as JSON so the model grounds the
/// report in this run's live data rather than its own prior knowledge. An empty
/// baseline (no data gathered — e.g. an offline smoke) falls back to the bare
/// instruction so the prompt never carries an empty data block.
fn build_user_prompt(baseline: &BaselineMarketData, deltas: Option<&BaselineDeltas>) -> String {
    let mut prompt = if baseline == &BaselineMarketData::default() {
        USER_PROMPT.to_string()
    } else {
        match serde_json::to_string_pretty(baseline) {
            Ok(json) => {
                format!("{USER_PROMPT}\n\nBaseline market data gathered for this report:\n{json}")
            }
            Err(_) => USER_PROMPT.to_string(),
        }
    };

    // When a prior report exists, append the deterministic change view computed by the
    // application layer (`baseline_delta`). The framing names the actual elapsed interval
    // rather than assuming a week — the report cadence is not fixed weekly — so the model
    // reads a same-hour regeneration's near-zero moves correctly instead of as a flat market.
    if let Some(d) = deltas {
        if let Ok(json) = serde_json::to_string_pretty(d) {
            prompt.push_str(&format!(
                "\n\nChange since the previous report (its baseline was captured ~{:.1} days ago). \
                 `changed` is level moves keyed by series. `new` / `missing` are series present \
                 in only one run's *gathered* baseline — when an entry carries a `reason` (or the \
                 baseline `gaps` manifest names it), its absence is a data-availability gap \
                 (`unavailable` / `rejected` / `malformed`), not the series itself entering or \
                 leaving the market:\n{json}",
                d.elapsed_days
            ));
        }
    }

    prompt
}

/// The model's structured return: the Markdown body plus the analytical fields.
/// `report_id` / `report_type` / `created_at` are deliberately absent — the
/// application layer owns those.
#[derive(Debug, Deserialize)]
struct ResponseEnvelope {
    markdown: String,
    risk_posture: RiskPosture,
    market_cycle: MarketCycle,
    thesis_stance: ThesisStance,
    header_summary_bullets: Vec<String>,
    #[serde(default)]
    key_risks: Vec<String>,
    #[serde(default)]
    unresolved_questions: Vec<String>,
    #[serde(default)]
    forward_outlook_themes: Vec<String>,
}

/// JSON Schema for the envelope. Shared by both arms: the Anthropic tool's
/// `input_schema` and the OpenAI `json_schema` format. All fields are required
/// and `additionalProperties` is false so OpenAI strict mode accepts it; the
/// 3–6 bound on bullets is enforced in `envelope_to_output` because strict mode
/// does not honor array-length constraints.
fn response_envelope_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "markdown": {
                "type": "string",
                "description": "The full weekly market report as GitHub-flavored Markdown."
            },
            "risk_posture": { "type": "string", "enum": ["risk-on", "risk-off", "mixed"] },
            "market_cycle": { "type": "string", "enum": ["late-cycle", "recessionary", "recovery"] },
            "thesis_stance": {
                "type": "string",
                "enum": ["bullish", "bearish", "mixed", "uncertain"]
            },
            "header_summary_bullets": {
                "type": "array",
                "items": { "type": "string" },
                "description": "3 to 6 concise summary bullets matching the Header Summary section."
            },
            "key_risks": { "type": "array", "items": { "type": "string" } },
            "unresolved_questions": { "type": "array", "items": { "type": "string" } },
            "forward_outlook_themes": { "type": "array", "items": { "type": "string" } }
        },
        "required": [
            "markdown", "risk_posture", "market_cycle", "thesis_stance",
            "header_summary_bullets", "key_risks", "unresolved_questions", "forward_outlook_themes"
        ]
    })
}

/// Anthropic Messages API request: a single forced tool, with strict schema
/// validation, whose `input_schema` is the envelope (parity with the OpenAI
/// arm's strict json_schema). `cache_control` on the system block is correct
/// placement for when the condensed packet grows the prefix past Opus's
/// ~4096-token cache minimum; below that it is a no-op, not an error.
fn build_anthropic_request(model_id: &str, system: &str, user: &str, schema: &Value) -> Value {
    json!({
        "model": model_id,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": [
            { "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }
        ],
        "tools": [
            {
                "name": TOOL_NAME,
                "description": "Emit the finished weekly market report and its structured summary.",
                "strict": true,
                "input_schema": schema
            }
        ],
        "tool_choice": { "type": "tool", "name": TOOL_NAME },
        "messages": [ { "role": "user", "content": user } ]
    })
}

/// OpenAI Chat Completions request with strict json_schema structured output.
fn build_openai_request(model_id: &str, system: &str, user: &str, schema: &Value) -> Value {
    json!({
        "model": model_id,
        "max_completion_tokens": MAX_TOKENS,
        "stream": true,
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "weekly_market_report", "strict": true, "schema": schema }
        },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    })
}

/// Pull a forced tool's `input` out of an Anthropic response: the first
/// `tool_use` block whose `name` matches `tool_name`. Shared with the fixed
/// internal Anthropic stages (`research_router`), whose forced-tool responses
/// have the identical block shape under a different tool name.
pub(crate) fn extract_anthropic_tool_input(raw: &Value, tool_name: &str) -> Result<Value> {
    let blocks = raw
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Anthropic response missing a content array"))?;
    blocks
        .iter()
        .find(|b| {
            b.get("type").and_then(Value::as_str) == Some("tool_use")
                && b.get("name").and_then(Value::as_str) == Some(tool_name)
        })
        .and_then(|b| b.get("input").cloned())
        .ok_or_else(|| anyhow!("Anthropic response contained no {tool_name} tool_use block"))
}

/// Pull the envelope value out of an OpenAI response: the first choice's message
/// content, which strict json_schema returns as a JSON string. Shared with the
/// fixed-internal OpenAI stages (`headline_filter`), whose strict-json-schema
/// responses have the identical envelope shape.
pub(crate) fn extract_openai_envelope(raw: &Value) -> Result<Value> {
    let content = raw
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("OpenAI response missing choices[0].message.content"))?;
    serde_json::from_str(content).context("OpenAI message content was not valid JSON")
}

/// Validate the envelope and mint the application-owned identity fields.
fn envelope_to_output(
    env: ResponseEnvelope,
    report_id: String,
    created_at: String,
) -> Result<MainAgentOutput> {
    let n = env.header_summary_bullets.len();
    if !(3..=6).contains(&n) {
        bail!("main agent returned {n} header_summary_bullets; expected 3 to 6");
    }
    if env.markdown.trim().is_empty() {
        bail!("main agent returned an empty markdown body");
    }

    let summary = ReportSummary {
        report_id,
        report_type: "weekly_market".to_string(),
        created_at,
        risk_posture: env.risk_posture,
        market_cycle: env.market_cycle,
        thesis_stance: env.thesis_stance,
        header_summary_bullets: env.header_summary_bullets,
        key_risks: env.key_risks,
        unresolved_questions: env.unresolved_questions,
        forward_outlook_themes: env.forward_outlook_themes,
    };
    Ok(MainAgentOutput {
        markdown: env.markdown,
        summary,
    })
}

/// Turn a raw provider response into a validated `MainAgentOutput`. Pure — the
/// identity fields are injected so tests are deterministic.
fn parse_response(
    provider: Provider,
    raw: &Value,
    report_id: String,
    created_at: String,
) -> Result<MainAgentOutput> {
    let value = match provider {
        Provider::Anthropic => extract_anthropic_tool_input(raw, TOOL_NAME)?,
        Provider::OpenAi => extract_openai_envelope(raw)?,
    };
    let env: ResponseEnvelope =
        serde_json::from_value(value).context("main agent response did not match the schema")?;
    envelope_to_output(env, report_id, created_at)
}

/// Live OpenAI/Anthropic adapter behind the `MainAgent` trait.
pub struct ModelMainAgent {
    config: MainAgentConfig,
    http: reqwest::blocking::Client,
    /// Run context for live token streaming. Defaults to a no-op (tests / offline
    /// smokes); the live command attaches the real one via
    /// [`ModelMainAgent::with_context`], and the streamed report text is emitted to it
    /// as the model writes.
    progress: Arc<RunContext>,
}

impl ModelMainAgent {
    pub fn new(config: MainAgentConfig) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("building the HTTP client")?;
        Ok(Self {
            config,
            http,
            progress: RunContext::noop(),
        })
    }

    /// Attach a live run context so the streamed report text reaches the run tracker.
    /// Without it the adapter keeps its no-op context and streams to nowhere (the HTTP
    /// call still streams; the deltas are simply dropped).
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Resolve the adapter from the environment. Delegates to the single
    /// env-reading path in `config::AppConfig` so the gate and the adapter agree
    /// on variable names and wording; used by the live smoke and any caller that
    /// bypasses the gate. The execution gate itself (`config::validate`) runs
    /// ahead of this in the command and replaces a missing model/key with
    /// structured validation rather than this plain error.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().main_agent_config()?)
    }

    /// Send the (streaming) model request and consume its Server-Sent-Events body,
    /// emitting the report text to the run tracker as the model writes it while
    /// accumulating the structured envelope for the final parse. Returns a `Value`
    /// shaped exactly like the old non-streaming response so `parse_response` —
    /// and all its tests — stay unchanged.
    ///
    /// The envelope accumulation is the source of truth for the report: the live
    /// token extraction is a pure side-channel to the progress reporter, so a bug in
    /// the decoder can only affect what the tracker shows, never the parsed report.
    fn call(&self, provider: Provider, body: &Value) -> Result<Value> {
        let request = match provider {
            Provider::Anthropic => self
                .http
                .post(ANTHROPIC_URL)
                .header("x-api-key", &self.config.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION),
            Provider::OpenAi => self
                .http
                .post(OPENAI_URL)
                .bearer_auth(&self.config.api_key),
        };
        let resp = request.json(body).send().context("sending model request")?;
        let status = resp.status();
        if !status.is_success() {
            // A rejected streaming request answers with a normal (non-SSE) error body.
            let text = resp.text().unwrap_or_default();
            bail!("model provider returned {status}: {text}");
        }

        // Accumulate the structured envelope from the SSE deltas while streaming the
        // decoded `markdown` field to the tracker. Token emits are coalesced to bound
        // the event count over a long report.
        let mut envelope = String::new();
        let mut extractor = MarkdownStreamExtractor::default();
        let mut pending = String::new();
        let reader = BufReader::new(resp);
        for line in reader.lines() {
            // Cancel checkpoint mid-stream: stop reading so a cancel requested during
            // generation lands promptly. The partial envelope then fails to parse,
            // which `run_job` classifies as Cancelled (the shared flag is set).
            if self.progress.is_cancelled() {
                break;
            }
            let line = line.context("reading streamed model response")?;
            // SSE: only `data:` lines carry payload; `event:`/comment/blank lines and
            // the terminal `[DONE]` sentinel are skipped.
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            // Tolerate any non-JSON keep-alive line rather than failing the stream.
            let Ok(event) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if let Some(fragment) = stream_delta(provider, &event) {
                envelope.push_str(fragment);
                pending.push_str(&extractor.update(&envelope));
                if pending.chars().count() >= TOKEN_FLUSH_CHARS {
                    self.progress.agent_token(std::mem::take(&mut pending));
                }
            }
        }
        if !pending.is_empty() {
            self.progress.agent_token(pending);
        }

        reconstruct_response(provider, &envelope)
    }
}

/// Coalesce streamed report text into chunks of at least this many characters before
/// emitting a progress event, so a long report streams as a few hundred events rather
/// than one per model token.
const TOKEN_FLUSH_CHARS: usize = 24;

/// Pull the incremental text fragment out of one SSE event, per provider:
/// - OpenAI Chat Completions stream: `choices[0].delta.content` — fragments of the
///   structured-output JSON string.
/// - Anthropic Messages stream: a `content_block_delta` carrying an `input_json_delta`,
///   whose `partial_json` are fragments of the forced tool's input JSON.
///
/// Every other event type (role deltas, `message_start`/`_stop`, `ping`) carries no
/// envelope text and returns `None`.
fn stream_delta(provider: Provider, event: &Value) -> Option<&str> {
    match provider {
        Provider::OpenAi => event.pointer("/choices/0/delta/content").and_then(Value::as_str),
        Provider::Anthropic => {
            if event.get("type").and_then(Value::as_str) != Some("content_block_delta") {
                return None;
            }
            let delta = event.get("delta")?;
            if delta.get("type").and_then(Value::as_str) != Some("input_json_delta") {
                return None;
            }
            delta.get("partial_json").and_then(Value::as_str)
        }
    }
}

/// Rebuild the `Value` `parse_response` expects from the accumulated streamed
/// envelope, so the streaming and non-streaming paths share one parse/validation path:
/// - OpenAI: the envelope *is* the message content JSON string.
/// - Anthropic: the envelope is the tool input JSON, re-nested as a `tool_use` block.
///
/// A truncated stream (a dropped connection mid-body) surfaces here as a parse error —
/// the same failure shape a truncated non-streaming body would have produced.
fn reconstruct_response(provider: Provider, envelope: &str) -> Result<Value> {
    match provider {
        Provider::OpenAi => Ok(json!({
            "choices": [ { "message": { "role": "assistant", "content": envelope } } ]
        })),
        Provider::Anthropic => {
            let input: Value = serde_json::from_str(envelope)
                .context("parsing streamed Anthropic tool input")?;
            Ok(json!({
                "content": [ { "type": "tool_use", "name": TOOL_NAME, "input": input } ]
            }))
        }
    }
}

/// Streams the decoded `markdown` field out of the growing response envelope so the
/// tracker shows the report as readable prose rather than escaped JSON.
///
/// Resumable by design: it keeps a byte cursor into the envelope and, on each update,
/// decodes only the bytes that have arrived since the last call — O(n) over the whole
/// stream rather than re-decoding the full envelope on every delta. The cursor never
/// advances into an incomplete trailing escape (a lone `\` or a partial `\uXXXX`), so
/// that escape is re-read intact once its remainder streams in; the emitted text only
/// ever grows and is never emitted half-formed. Each call's argument must be an
/// extension of the previous one (the `call` loop only ever appends), which the cursor
/// relies on.
#[derive(Default)]
struct MarkdownStreamExtractor {
    /// Byte offset in the envelope to resume decoding from — `None` until the
    /// `"markdown": "` opener has streamed in, then the first not-yet-decoded byte of
    /// the value (always on a char boundary, never inside an escape).
    cursor: Option<usize>,
    /// The value's closing quote has been seen; nothing further is emitted.
    done: bool,
}

impl MarkdownStreamExtractor {
    /// Decode whatever new markdown-field text has arrived in `envelope` and return
    /// just that suffix — empty until the field opens, and again once it closes.
    fn update(&mut self, envelope: &str) -> String {
        if self.done {
            return String::new();
        }
        let start = match self.cursor {
            Some(start) => start,
            None => match markdown_value_start(envelope) {
                Some(start) => {
                    self.cursor = Some(start);
                    start
                }
                None => return String::new(),
            },
        };
        let (decoded, consumed, closed) = decode_json_string_chunk(&envelope[start..]);
        self.cursor = Some(start + consumed);
        self.done = closed;
        decoded
    }
}

/// Byte offset just after the opening quote of the `"markdown"` string value in a
/// (possibly partial) JSON object, or `None` until `"markdown": "` has streamed in.
fn markdown_value_start(envelope: &str) -> Option<usize> {
    const KEY: &str = "\"markdown\"";
    let after_key = envelope.find(KEY)? + KEY.len();
    // Expect optional whitespace, the ':' separator, more whitespace, then the opening
    // '"' of the value.
    let mut seen_colon = false;
    for (idx, c) in envelope[after_key..].char_indices() {
        if c.is_whitespace() {
            continue;
        }
        if !seen_colon {
            if c == ':' {
                seen_colon = true;
                continue;
            }
            return None;
        }
        if c == '"' {
            return Some(after_key + idx + c.len_utf8());
        }
        return None;
    }
    None
}

/// Decode a JSON string body (the bytes *after* the opening quote) from the start of
/// `s`, up to the first unescaped closing quote or the last fully-formed character.
/// Returns `(decoded_text, bytes_consumed, closed)`. `bytes_consumed` stops *before*
/// any incomplete trailing escape (a lone `\` or a partial `\uXXXX`), so a resumed call
/// re-reads that escape once its remainder arrives. `closed` is true once the value's
/// closing quote was reached.
fn decode_json_string_chunk(s: &str) -> (String, usize, bool) {
    let mut out = String::new();
    let mut consumed = 0;
    let mut chars = s.char_indices();
    while let Some((idx, c)) = chars.next() {
        match c {
            '"' => return (out, idx + 1, true), // closing quote of the value
            '\\' => {
                let Some((esc_idx, esc)) = chars.next() else {
                    return (out, idx, false); // lone trailing '\' — resume here
                };
                let mut next_consumed = esc_idx + esc.len_utf8();
                match esc {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{0008}'),
                    'f' => out.push('\u{000C}'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'u' => {
                        let mut hex = String::with_capacity(4);
                        for _ in 0..4 {
                            match chars.next() {
                                Some((j, h)) => {
                                    hex.push(h);
                                    next_consumed = j + h.len_utf8();
                                }
                                None => return (out, idx, false), // partial \uXXXX
                            }
                        }
                        if let Some(ch) =
                            u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            out.push(ch);
                        }
                    }
                    other => out.push(other),
                }
                consumed = next_consumed;
            }
            c => {
                out.push(c);
                consumed = idx + c.len_utf8();
            }
        }
    }
    (out, consumed, false)
}

impl MainAgent for ModelMainAgent {
    fn generate(&self, input: MainAgentInput) -> Result<MainAgentOutput> {
        let provider = self.config.model.provider();
        let model_id = self.config.model.model_id();
        let schema = response_envelope_schema();
        let user = build_user_prompt(&input.baseline, input.deltas.as_ref());
        let body = match provider {
            Provider::Anthropic => build_anthropic_request(model_id, SYSTEM_PROMPT, &user, &schema),
            Provider::OpenAi => build_openai_request(model_id, SYSTEM_PROMPT, &user, &schema),
        };
        let raw = self.call(provider, &body)?;

        // The application layer owns identity, not the model.
        let report_id = Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();
        parse_response(provider, &raw, report_id, created_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_envelope() -> Value {
        json!({
            "markdown": "# Weekly Market Report\n\n## Header Summary\n- a\n- b\n- c\n",
            "risk_posture": "mixed",
            "market_cycle": "late-cycle",
            "thesis_stance": "uncertain",
            "header_summary_bullets": ["a", "b", "c"],
            "key_risks": ["a reacceleration in core inflation"],
            "unresolved_questions": [],
            "forward_outlook_themes": ["liquidity and breadth"]
        })
    }

    #[test]
    fn resolves_each_label_to_provider_and_model_id() {
        let cases = [
            ("gpt-5", Provider::OpenAi, "gpt-5"),
            ("gpt-5-mini", Provider::OpenAi, "gpt-5-mini"),
            ("claude-opus", Provider::Anthropic, "claude-opus-4-8"),
            ("claude-sonnet", Provider::Anthropic, "claude-sonnet-4-6"),
            ("claude-haiku", Provider::Anthropic, "claude-haiku-4-5"),
        ];
        for (label, provider, model_id) in cases {
            let m = AgentModel::from_config_label(label).unwrap();
            assert_eq!(m.provider(), provider, "{label}");
            assert_eq!(m.model_id(), model_id, "{label}");
        }
        assert!(AgentModel::from_config_label("bogus").is_err());
    }

    #[test]
    fn all_models_round_trip_label_and_carry_a_display_name() {
        // ALL is the Settings option source: every entry's slug must parse back
        // to itself, and provider display names cover both providers.
        for m in AgentModel::ALL {
            assert_eq!(
                AgentModel::from_config_label(m.config_label()).unwrap(),
                m,
                "{}",
                m.config_label()
            );
            assert!(!m.display_name().is_empty());
        }
        assert_eq!(AgentModel::Gpt5.provider().display_name(), "OpenAI");
        assert_eq!(AgentModel::ClaudeOpus.provider().display_name(), "Anthropic");
    }

    #[test]
    fn anthropic_request_forces_the_tool_and_caches_system() {
        let body = build_anthropic_request(
            "claude-opus-4-8",
            SYSTEM_PROMPT,
            USER_PROMPT,
            &response_envelope_schema(),
        );
        assert_eq!(body["model"], "claude-opus-4-8");
        assert_eq!(body["tool_choice"]["type"], "tool");
        assert_eq!(body["tool_choice"]["name"], TOOL_NAME);
        assert_eq!(body["tools"][0]["name"], TOOL_NAME);
        assert_eq!(body["tools"][0]["strict"], true);
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["messages"][0]["content"], USER_PROMPT);
    }

    #[test]
    fn openai_request_uses_strict_json_schema() {
        let body =
            build_openai_request("gpt-5", SYSTEM_PROMPT, USER_PROMPT, &response_envelope_schema());
        assert_eq!(body["model"], "gpt-5");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(body["messages"][1]["content"], USER_PROMPT);
    }

    #[test]
    fn user_prompt_embeds_baseline_when_present() {
        use crate::data_sources::{DataGap, EconomicRelease, GapReason, GroupKind, Quote};
        let baseline = BaselineMarketData {
            indices: vec![Quote {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                price: 5500.0,
                change_pct: 0.4,
                unit: "index points".into(),
            }],
            calendar: vec![EconomicRelease {
                release: "Employment Situation".into(),
                date: "2026-06-05".into(),
                status: "released".into(),
                expected: None,
            }],
            gaps: vec![DataGap::new(
                GroupKind::LaborLevels,
                "CES0500000003",
                "Average Hourly Earnings, Total Private",
                GapReason::Rejected,
            )],
            ..Default::default()
        };
        let prompt = build_user_prompt(&baseline, None);
        assert!(prompt.starts_with(USER_PROMPT), "{prompt}");
        assert!(prompt.contains("^GSPC"), "{prompt}");
        assert!(prompt.contains("Baseline market data"), "{prompt}");
        // The unit rides into the serialized baseline, so the model sees what `price` is
        // quoted in — the whole point of the field reaching the prompt.
        assert!(prompt.contains("index points"), "{prompt}");
        // The economic-release calendar reaches the model the same way — through the
        // whole-baseline serialization, no formatter change.
        assert!(prompt.contains("Employment Situation"), "{prompt}");
        // The missing-data manifest rides in the same way: the agent sees which series
        // were absent this run, and why, rather than inferring values for them.
        assert!(prompt.contains("CES0500000003"), "{prompt}");
        assert!(prompt.contains("rejected"), "{prompt}");
    }

    #[test]
    fn user_prompt_is_bare_when_baseline_empty() {
        assert_eq!(
            build_user_prompt(&BaselineMarketData::default(), None),
            USER_PROMPT
        );
    }

    fn one_index_baseline() -> BaselineMarketData {
        use crate::data_sources::Quote;
        BaselineMarketData {
            indices: vec![Quote {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                price: 5_610.0,
                change_pct: 0.4,
                unit: "index points".into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn user_prompt_appends_change_block_when_deltas_present() {
        use crate::baseline_delta::{BaselineDeltas, Direction, SeriesDelta};
        use crate::data_sources::GroupKind;
        let deltas = BaselineDeltas {
            elapsed_days: 6.0,
            changed: vec![SeriesDelta {
                group: GroupKind::Indices,
                id: "^GSPC".into(),
                name: "S&P 500".into(),
                current: 5_610.0,
                prior: 5_500.0,
                abs_change: 110.0,
                pct_change: Some(2.0),
                direction: Direction::Up,
            }],
            new: vec![],
            missing: vec![],
        };
        let prompt = build_user_prompt(&one_index_baseline(), Some(&deltas));
        assert!(
            prompt.contains("Change since the previous report"),
            "{prompt}"
        );
        // Cadence-honest framing names the actual elapsed interval, not a week.
        assert!(prompt.contains("6.0 days"), "{prompt}");
        // The serialized change view rides in, so the model reads the move, not just the level.
        assert!(prompt.contains("abs_change"), "{prompt}");
    }

    #[test]
    fn user_prompt_omits_change_block_when_no_deltas() {
        let prompt = build_user_prompt(&one_index_baseline(), None);
        assert!(
            !prompt.contains("Change since the previous report"),
            "{prompt}"
        );
    }

    #[test]
    fn parses_anthropic_tool_use_into_output() {
        let raw = json!({
            "content": [
                { "type": "text", "text": "preamble that should be ignored" },
                { "type": "tool_use", "id": "toolu_1", "name": TOOL_NAME, "input": valid_envelope() }
            ],
            "stop_reason": "tool_use"
        });
        let out = parse_response(
            Provider::Anthropic,
            &raw,
            "rid-123".to_string(),
            "2026-06-02T00:00:00Z".to_string(),
        )
        .unwrap();

        assert_eq!(out.summary.report_id, "rid-123");
        assert_eq!(out.summary.report_type, "weekly_market");
        assert_eq!(out.summary.created_at, "2026-06-02T00:00:00Z");
        assert_eq!(out.summary.header_summary_bullets.len(), 3);
        assert!(!out.markdown.is_empty());

        let json = serde_json::to_value(&out.summary).unwrap();
        assert_eq!(json["risk_posture"], "mixed");
        assert_eq!(json["market_cycle"], "late-cycle");
        assert_eq!(json["thesis_stance"], "uncertain");
    }

    #[test]
    fn parses_openai_json_content_into_output() {
        let content = serde_json::to_string(&valid_envelope()).unwrap();
        let raw = json!({ "choices": [ { "message": { "role": "assistant", "content": content } } ] });
        let out = parse_response(
            Provider::OpenAi,
            &raw,
            "rid-456".to_string(),
            "2026-06-02T00:00:00Z".to_string(),
        )
        .unwrap();

        assert_eq!(out.summary.report_id, "rid-456");
        assert_eq!(out.summary.thesis_stance, ThesisStance::Uncertain);
        assert_eq!(out.summary.forward_outlook_themes, vec!["liquidity and breadth"]);
    }

    #[test]
    fn rejects_bullet_count_out_of_range() {
        let mut env = valid_envelope();
        env["header_summary_bullets"] = json!(["only", "two"]);
        let raw = json!({ "choices": [ { "message": { "content": env.to_string() } } ] });
        let err = parse_response(Provider::OpenAi, &raw, "r".into(), "t".into()).unwrap_err();
        assert!(
            err.to_string().contains("header_summary_bullets"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_anthropic_response_without_tool_call() {
        let raw = json!({ "content": [ { "type": "text", "text": "no tool call here" } ] });
        let err = parse_response(Provider::Anthropic, &raw, "r".into(), "t".into()).unwrap_err();
        assert!(err.to_string().contains(TOOL_NAME), "unexpected error: {err}");
    }

    #[test]
    fn rejects_openai_non_json_content() {
        let raw = json!({ "choices": [ { "message": { "content": "not json at all" } } ] });
        assert!(parse_response(Provider::OpenAi, &raw, "r".into(), "t".into()).is_err());
    }

    #[test]
    fn both_request_arms_enable_streaming() {
        let a = build_anthropic_request(
            "claude-opus-4-8",
            SYSTEM_PROMPT,
            USER_PROMPT,
            &response_envelope_schema(),
        );
        assert_eq!(a["stream"], true);
        let o = build_openai_request("gpt-5", SYSTEM_PROMPT, USER_PROMPT, &response_envelope_schema());
        assert_eq!(o["stream"], true);
    }

    #[test]
    fn stream_delta_reads_each_provider_fragment_and_ignores_the_rest() {
        // OpenAI: the content delta is the fragment; a role-only delta is not.
        let oai = json!({ "choices": [ { "delta": { "content": "# He" } } ] });
        assert_eq!(stream_delta(Provider::OpenAi, &oai), Some("# He"));
        let oai_role = json!({ "choices": [ { "delta": { "role": "assistant" } } ] });
        assert_eq!(stream_delta(Provider::OpenAi, &oai_role), None);

        // Anthropic: only an input_json_delta carries envelope text.
        let ant = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "input_json_delta", "partial_json": "{\"mark" }
        });
        assert_eq!(stream_delta(Provider::Anthropic, &ant), Some("{\"mark"));
        assert_eq!(stream_delta(Provider::Anthropic, &json!({ "type": "ping" })), None);
        let text_delta = json!({
            "type": "content_block_delta",
            "delta": { "type": "text_delta", "text": "ignored preamble" }
        });
        assert_eq!(stream_delta(Provider::Anthropic, &text_delta), None);
    }

    #[test]
    fn markdown_extractor_streams_decoded_prose_one_char_at_a_time() {
        // A realistic envelope: markdown first, with newline / quote / backslash escapes,
        // then the structured fields. Fed one character at a time — the worst case for a
        // partial escape landing on a chunk boundary.
        let envelope =
            r##"{"markdown":"# Title\n\nA \"quoted\" word and a slash \\ end.","risk_posture":"mixed"}"##;
        let expected = "# Title\n\nA \"quoted\" word and a slash \\ end.";

        let mut extractor = MarkdownStreamExtractor::default();
        let mut grown = String::new();
        let mut streamed = String::new();
        for ch in envelope.chars() {
            grown.push(ch);
            streamed.push_str(&extractor.update(&grown));
        }
        assert_eq!(streamed, expected);
        // Once the value's closing quote is in, no further suffix is emitted.
        assert_eq!(extractor.update(envelope), "");
    }

    #[test]
    fn markdown_extractor_resumes_a_unicode_escape_split_across_chunks() {
        // U+2014 is an em dash, written here as the JSON escape —. Fed one char
        // at a time, the partial "\u20..." must be held back (the cursor parks before
        // the backslash) and completed on a later call, never emitted half-formed.
        let envelope = "{\"markdown\":\"a\\u2014b\"}";
        let mut extractor = MarkdownStreamExtractor::default();
        let mut grown = String::new();
        let mut streamed = String::new();
        for ch in envelope.chars() {
            grown.push(ch);
            streamed.push_str(&extractor.update(&grown));
        }
        assert_eq!(streamed, "a\u{2014}b");
    }

    #[test]
    fn markdown_extractor_is_empty_until_the_field_opens() {
        let mut extractor = MarkdownStreamExtractor::default();
        assert_eq!(extractor.update("{\"risk_posture\":\"mixed\","), "");
        assert_eq!(extractor.update("{\"risk_posture\":\"mixed\",\"markdown\":\"Hi"), "Hi");
    }

    #[test]
    fn reconstruct_response_feeds_the_unchanged_parse_path() {
        // The streamed envelope text, reconstructed, must parse through `parse_response`
        // exactly as a non-streaming body would — both provider arms.
        let env = serde_json::to_string(&valid_envelope()).unwrap();

        let raw = reconstruct_response(Provider::OpenAi, &env).unwrap();
        let out =
            parse_response(Provider::OpenAi, &raw, "rid".into(), "2026-06-02T00:00:00Z".into())
                .unwrap();
        assert_eq!(out.summary.header_summary_bullets.len(), 3);

        let raw = reconstruct_response(Provider::Anthropic, &env).unwrap();
        let out =
            parse_response(Provider::Anthropic, &raw, "rid".into(), "2026-06-02T00:00:00Z".into())
                .unwrap();
        assert!(!out.markdown.is_empty());

        // A truncated stream is a typed parse error, not a panic (Anthropic arm parses
        // the tool input eagerly).
        assert!(reconstruct_response(Provider::Anthropic, "{\"markdown\":\"unterminated").is_err());
    }

    #[test]
    #[ignore = "hits the live provider API; set MARKET_SIGNAL_MAIN_AGENT_MODEL + the provider key"]
    fn live_generate_smoke() {
        let agent = ModelMainAgent::from_env().expect("env configured for a live run");
        let out = agent
            .generate(MainAgentInput::default())
            .expect("live generate");
        assert!(!out.markdown.is_empty());
        assert!((3..=6).contains(&out.summary.header_summary_bullets.len()));
    }
}
