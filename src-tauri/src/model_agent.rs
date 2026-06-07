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

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::agent::{
    MainAgent, MainAgentInput, MainAgentOutput, MarketCycle, ReportSummary, RiskPosture,
    ThesisStance,
};
use crate::data_sources::BaselineMarketData;

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
fn build_user_prompt(baseline: &BaselineMarketData) -> String {
    if baseline == &BaselineMarketData::default() {
        return USER_PROMPT.to_string();
    }
    match serde_json::to_string_pretty(baseline) {
        Ok(json) => format!("{USER_PROMPT}\n\nBaseline market data gathered for this report:\n{json}"),
        Err(_) => USER_PROMPT.to_string(),
    }
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

/// Pull the envelope value out of an Anthropic response: the first `tool_use`
/// block matching our forced tool, by its `input`.
fn extract_anthropic_envelope(raw: &Value) -> Result<Value> {
    let blocks = raw
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Anthropic response missing a content array"))?;
    blocks
        .iter()
        .find(|b| {
            b.get("type").and_then(Value::as_str) == Some("tool_use")
                && b.get("name").and_then(Value::as_str) == Some(TOOL_NAME)
        })
        .and_then(|b| b.get("input").cloned())
        .ok_or_else(|| anyhow!("Anthropic response contained no {TOOL_NAME} tool_use block"))
}

/// Pull the envelope value out of an OpenAI response: the first choice's message
/// content, which strict json_schema returns as a JSON string.
fn extract_openai_envelope(raw: &Value) -> Result<Value> {
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
        Provider::Anthropic => extract_anthropic_envelope(raw)?,
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
}

impl ModelMainAgent {
    pub fn new(config: MainAgentConfig) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("building the HTTP client")?;
        Ok(Self { config, http })
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
        let text = resp.text().context("reading model response body")?;
        if !status.is_success() {
            bail!("model provider returned {status}: {text}");
        }
        serde_json::from_str(&text).context("parsing model response JSON")
    }
}

impl MainAgent for ModelMainAgent {
    fn generate(&self, input: MainAgentInput) -> Result<MainAgentOutput> {
        let provider = self.config.model.provider();
        let model_id = self.config.model.model_id();
        let schema = response_envelope_schema();
        let user = build_user_prompt(&input.baseline);
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
        use crate::data_sources::{EconomicRelease, Quote};
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
            ..Default::default()
        };
        let prompt = build_user_prompt(&baseline);
        assert!(prompt.starts_with(USER_PROMPT), "{prompt}");
        assert!(prompt.contains("^GSPC"), "{prompt}");
        assert!(prompt.contains("Baseline market data"), "{prompt}");
        // The unit rides into the serialized baseline, so the model sees what `price` is
        // quoted in — the whole point of the field reaching the prompt.
        assert!(prompt.contains("index points"), "{prompt}");
        // The economic-release calendar reaches the model the same way — through the
        // whole-baseline serialization, no formatter change.
        assert!(prompt.contains("Employment Situation"), "{prompt}");
    }

    #[test]
    fn user_prompt_is_bare_when_baseline_empty() {
        assert_eq!(build_user_prompt(&BaselineMarketData::default()), USER_PROMPT);
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
