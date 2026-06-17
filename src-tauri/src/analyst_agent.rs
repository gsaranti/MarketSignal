//! Real OpenAI/Anthropic adapter for the analyst stage — Bull / Bear / Balanced
//! (`docs/agents.md §Analyst Agents`, `docs/weekly-report-workflow.md §§12–15`).
//!
//! Each analyst is one [`Posture`] behind the [`AnalystAgent`] trait: the same
//! condensed research packet in, a structured [`AnalystOutput`] out. The adapter is
//! **dual-provider** like the main agent (the analyst model is user-selectable, so it
//! may be OpenAI or Anthropic), but **non-streaming** like the fixed internal stages
//! (`research_router` / `headline_filter`) — a review is small, so a single response
//! is returned and parsed whole. The blocking HTTP call keeps the trait synchronous;
//! the three analysts run concurrently at the application-layer seam (`pipeline`),
//! offloaded via `spawn_blocking` at the Tauri command.
//!
//! The provider request/transport plumbing mirrors `model_agent`: this stage reuses
//! its public response extractors ([`extract_anthropic_tool_input`] /
//! [`extract_openai_envelope`]) and provider/model resolution ([`Provider`],
//! [`MainAgentConfig`]), supplying its own posture-specific system prompt and review
//! schema. Unlike the gated *data* adapters it carries no `with_base_url` mock seam —
//! it follows the model-adapter house pattern: unit tests for the pure request/parse
//! pieces plus an `#[ignore]`d live smoke.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::{AnalystAgent, AnalystOutput, Confidence, Posture};
use crate::model_agent::{
    extract_anthropic_tool_input, extract_openai_envelope, MainAgentConfig, Provider,
    ANTHROPIC_VERSION,
};
use crate::progress::RunContext;
use crate::research_packet::ResearchPacket;
use crate::skills;

/// Provider endpoints — the analyst stage calls the provider directly, like the
/// other model adapters.
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";

/// A review is a summary plus a few short lists, so a modest ceiling is ample and
/// keeps the response inside the HTTP timeout.
const MAX_TOKENS: u32 = 4096;

/// The single tool the Anthropic arm forces, and the json_schema name on the OpenAI
/// arm. Both feed the same [`ReviewEnvelope`].
const TOOL_NAME: &str = "emit_analyst_review";

/// The tracker `group` for the analyst stage's per-call request rows.
const REQUEST_GROUP: &str = "analyst";

/// The shared instruction, common to all three postures. The posture-specific
/// guidance ([`posture_guidance`]) is appended per analyst.
const BASE_SYSTEM_PROMPT: &str = "You are a market analyst on Market Signal's weekly research \
team, contributing one perspective to a weekly market report. You are given this week's condensed \
research packet — the baseline market data and its change view, the filtered news clusters, the \
deep-research evidence and its sources, any recalled long-term memory, and any user-supplied \
research documents. Produce a structured analytical review of the week from your assigned \
perspective. Ground every point in the provided packet — the baseline numbers, the change view, \
the news, and the research evidence; never invent data or lean on prior knowledge the packet does \
not support. Your review is one of three independent perspectives the Head Market Analyst will \
critique and weigh when synthesizing the final report; argue your perspective in good faith as a \
professional analyst rather than forcing a predetermined conclusion. The prompt also carries \
`analytical skills` — a library of analytical lenses, each with the method it applies and the \
structured verdict it should yield. Not every lens applies every week: work through the ones this \
week's data and research warrant, produce each relevant lens's verdict, and let it inform your \
review — its key points, the risks you see, and the opportunities you see. The skills are \
reasoning tools, not output structure; do not name them or write one up as its own field. Return: \
a short summary of your read of the week, the key points your read rests on, the risks you see, \
the opportunities you see, and your confidence (low, medium, or high) in this read.";

/// The posture-specific half of the system prompt (`docs/agents.md §Bull/Bear/Balanced
/// Analyst`).
fn posture_guidance(posture: Posture) -> &'static str {
    match posture {
        Posture::Bull => "Your perspective is the Bull Analyst. Focus on constructive \
interpretations: upside drivers, resilience in market structure, improving conditions, and \
overly pessimistic assumptions worth challenging. Do not ignore negative data or force a bullish \
conclusion — acknowledge the risks while focusing on the evidence that supports continued strength \
or improving conditions.",
        Posture::Bear => "Your perspective is the Bear Analyst. Focus on fragile assumptions and \
downside risks: weakening conditions, complacency worth challenging, and valuation, \
macroeconomic, geopolitical, liquidity, and credit risks. Do not deny bullish conditions the data \
supports — acknowledge the strength while focusing on hidden vulnerabilities, unsustainable \
narratives, and structural risks.",
        Posture::Balanced => "Your perspective is the Balanced Analyst. Weigh the evidence and \
identify the most probable interpretation: separate signal from noise, weigh bullish against \
bearish evidence, assign confidence, separate short-term from long-term implications, and name the \
conditions that would justify a thesis change. Do not stay artificially neutral — reach a bullish \
or bearish read when the evidence strongly supports one.",
    }
}

/// The full system prompt for one analyst: the shared instruction plus its posture
/// guidance.
fn system_prompt(posture: Posture) -> String {
    format!("{BASE_SYSTEM_PROMPT}\n\n{}", posture_guidance(posture))
}

const USER_INSTRUCTION: &str = "Produce your structured analytical review of this week's market.";

/// The analyst heading for the skills block — review framing (let each verdict inform the
/// review's key points, risks, and opportunities). The per-skill bodies + verdict markers
/// come from the shared [`skills::render_library`]; only this intro is analyst-specific (the
/// main agent supplies its own synthesis-framed heading). The whole library ships to every
/// analyst, which self-selects the lenses its posture and this week's packet warrant — the
/// same all-16-inline call the main agent makes.
const SKILL_LIBRARY_INTRO: &str = "\n\nAnalytical skills — a library of analytical lenses. Not \
every lens applies every week: apply the ones this week's data and research warrant, and for each \
you apply produce its stated verdict and let that conclusion inform your review's key points, \
risks, and opportunities rather than writing it up as its own item:";

/// The model's structured return. Every field is required — the strict schema forces
/// the provider to emit them, so a missing field is a malformed response that fails the
/// parse and the run, honoring the analyst stage's not-fail-soft contract
/// (`docs/weekly-report-workflow.md §Step 9`); the defaults that would have masked it
/// were deliberately removed. `envelope_to_output` further rejects a blank summary. The
/// posture is supplied by the application layer (the adapter knows which analyst it is),
/// not the model.
#[derive(Debug, Deserialize)]
struct ReviewEnvelope {
    summary: String,
    key_points: Vec<String>,
    risks: Vec<String>,
    opportunities: Vec<String>,
    confidence: Confidence,
}

/// JSON Schema for the review envelope. Shared by both arms: the Anthropic tool's
/// `input_schema` and the OpenAI `json_schema` format. All fields required and
/// `additionalProperties` false so OpenAI strict mode accepts it.
fn review_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "summary": {
                "type": "string",
                "description": "A short prose read of the week from this perspective."
            },
            "key_points": { "type": "array", "items": { "type": "string" } },
            "risks": { "type": "array", "items": { "type": "string" } },
            "opportunities": { "type": "array", "items": { "type": "string" } },
            "confidence": { "type": "string", "enum": ["low", "medium", "high"] }
        },
        "required": ["summary", "key_points", "risks", "opportunities", "confidence"]
    })
}

/// Anthropic Messages request: a non-streaming forced-tool call whose `input_schema`
/// is the review envelope (mirrors `research_router::build_request`).
fn build_anthropic_request(model_id: &str, system: &str, user: &str) -> Value {
    json!({
        "model": model_id,
        "max_tokens": MAX_TOKENS,
        "stream": false,
        "system": [
            { "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }
        ],
        "tools": [
            {
                "name": TOOL_NAME,
                "description": "Emit this analyst's structured review of the week.",
                "strict": true,
                "input_schema": review_schema()
            }
        ],
        "tool_choice": { "type": "tool", "name": TOOL_NAME },
        "messages": [ { "role": "user", "content": user } ]
    })
}

/// OpenAI Chat Completions request: non-streaming strict json_schema (mirrors
/// `headline_filter::build_request`).
fn build_openai_request(model_id: &str, system: &str, user: &str) -> Value {
    json!({
        "model": model_id,
        "max_completion_tokens": MAX_TOKENS,
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "analyst_review", "strict": true, "schema": review_schema() }
        },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    })
}

/// Build the user message: the standing instruction, the condensed research packet
/// serialized as JSON — the canonical analyst input (`docs/weekly-report-workflow.md
/// §Step 11`) — and the full analytical-skills library (`docs/analyst-skills.md`). A
/// default (empty) packet falls back to the bare instruction so the prompt never carries an
/// empty data block, but the skills library is appended in every case: the lenses are
/// packet-independent, mirroring the main agent, which appends the whole library
/// unconditionally.
fn build_user_prompt(packet: &ResearchPacket) -> String {
    let mut prompt = USER_INSTRUCTION.to_string();
    if packet != &ResearchPacket::default() {
        if let Ok(json) = serde_json::to_string_pretty(packet) {
            prompt.push_str(&format!("\n\nCondensed research packet for this week:\n{json}"));
        }
    }
    prompt.push_str(&skills::render_library(SKILL_LIBRARY_INTRO));
    prompt
}

/// Validate the model's envelope and tag it with the adapter's posture. The summary is
/// the analyst's core output, so a blank one fails the run rather than feeding an empty
/// section into synthesis — the analyst stage is not fail-soft
/// (`docs/weekly-report-workflow.md §Step 9`), and this mirrors the main agent's
/// non-empty-body check (`model_agent::envelope_to_output`). The lists stay lenient: a
/// terse review (e.g. a bear naming no opportunities) is legitimate, so only the summary
/// is required — the main agent likewise does not require its optional arrays to be
/// non-empty.
fn envelope_to_output(posture: Posture, env: ReviewEnvelope) -> Result<AnalystOutput> {
    if env.summary.trim().is_empty() {
        bail!("{} returned an empty review summary", posture.display_name());
    }
    Ok(AnalystOutput {
        posture,
        summary: env.summary,
        key_points: env.key_points,
        risks: env.risks,
        opportunities: env.opportunities,
        confidence: env.confidence,
    })
}

/// Live OpenAI/Anthropic adapter behind the [`AnalystAgent`] trait, for one posture.
pub struct ModelAnalystAgent {
    posture: Posture,
    config: MainAgentConfig,
    http: reqwest::blocking::Client,
    /// Run context for the single tracker row the review call emits. Defaults to a
    /// no-op (tests / offline smokes); the live command attaches the real one via
    /// [`ModelAnalystAgent::with_context`].
    progress: Arc<RunContext>,
}

impl ModelAnalystAgent {
    pub fn new(posture: Posture, config: MainAgentConfig) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("building the analyst HTTP client")?;
        Ok(Self {
            posture,
            config,
            http,
            progress: RunContext::noop(),
        })
    }

    /// Attach a live run context so the review call streams a request row to the
    /// tracker. Without it the adapter keeps its no-op context.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Resolve the adapter from the environment for the given posture, for the live
    /// smoke and any caller that bypasses the gate. Reads the posture's user-selected
    /// model + its provider key (`config::AppConfig::analyst_config`).
    pub fn from_env(posture: Posture) -> Result<Self> {
        Self::new(posture, crate::config::AppConfig::from_env().analyst_config(posture)?)
    }

    fn call(&self, provider: Provider, body: &Value) -> Result<Value> {
        let request = match provider {
            Provider::Anthropic => self
                .http
                .post(ANTHROPIC_URL)
                .header("x-api-key", &self.config.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION),
            Provider::OpenAi => self.http.post(OPENAI_URL).bearer_auth(&self.config.api_key),
        };
        let resp = request.json(body).send().context("sending analyst request")?;
        let status = resp.status();
        let text = resp.text().context("reading analyst response body")?;
        if !status.is_success() {
            bail!("analyst model returned {status}: {text}");
        }
        serde_json::from_str(&text).context("parsing analyst response JSON")
    }
}

impl AnalystAgent for ModelAnalystAgent {
    fn review(&self, packet: &ResearchPacket) -> Result<AnalystOutput> {
        let provider = self.config.model.provider();
        let model_id = self.config.model.model_id();
        let system = system_prompt(self.posture);
        let user = build_user_prompt(packet);
        let body = match provider {
            Provider::Anthropic => build_anthropic_request(model_id, &system, &user),
            Provider::OpenAi => build_openai_request(model_id, &system, &user),
        };

        // One tracker row for this analyst's review call.
        let name = self.posture.display_name();
        self.progress
            .request_started(provider.display_name(), REQUEST_GROUP, self.posture.as_str(), name);
        let result = (|| -> Result<AnalystOutput> {
            let raw = self.call(provider, &body)?;
            let value = match provider {
                Provider::Anthropic => extract_anthropic_tool_input(&raw, TOOL_NAME)?,
                Provider::OpenAi => extract_openai_envelope(&raw)?,
            };
            let env: ReviewEnvelope = serde_json::from_value(value)
                .context("analyst review did not match the schema")?;
            envelope_to_output(self.posture, env)
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                provider.display_name(),
                REQUEST_GROUP,
                self.posture.as_str(),
                name,
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                provider.display_name(),
                REQUEST_GROUP,
                self.posture.as_str(),
                name,
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::data_sources::{BaselineMarketData, Change, Quote};

    fn one_index_packet() -> ResearchPacket {
        ResearchPacket {
            baseline: BaselineMarketData {
                indices: vec![Quote {
                    symbol: "^GSPC".into(),
                    name: "S&P 500".into(),
                    price: 5_600.0,
                    change: Change::percent(0.5),
                    unit: "index points".into(),
                }],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn system_prompt_carries_base_plus_posture_guidance() {
        for p in Posture::ALL {
            let prompt = system_prompt(p);
            assert!(prompt.contains("market analyst on Market Signal"), "{p:?} base missing");
            assert!(prompt.contains(p.display_name()), "{p:?} posture label missing");
        }
        // The three guidances are distinct.
        assert_ne!(system_prompt(Posture::Bull), system_prompt(Posture::Bear));
        assert_ne!(system_prompt(Posture::Bear), system_prompt(Posture::Balanced));
    }

    #[test]
    fn system_prompt_directs_skill_application_for_every_posture() {
        // The skills directive lives in the shared base, so every posture carries it.
        for p in Posture::ALL {
            let prompt = system_prompt(p);
            assert!(prompt.contains("`analytical skills`"), "{p:?} skills directive missing");
            assert!(
                prompt.contains("reasoning tools, not output structure"),
                "{p:?} skills framing missing"
            );
        }
    }

    #[test]
    fn anthropic_request_forces_the_tool_and_is_not_streamed() {
        let body = build_anthropic_request("claude-opus-4-8", &system_prompt(Posture::Bull), "u");
        assert_eq!(body["model"], "claude-opus-4-8");
        assert_eq!(body["stream"], false);
        assert_eq!(body["tool_choice"]["name"], TOOL_NAME);
        assert_eq!(body["tools"][0]["name"], TOOL_NAME);
        assert_eq!(body["tools"][0]["strict"], true);
    }

    #[test]
    fn openai_request_uses_strict_json_schema() {
        let body = build_openai_request("gpt-5", &system_prompt(Posture::Bear), "u");
        assert_eq!(body["model"], "gpt-5");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["response_format"]["json_schema"]["name"], "analyst_review");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    }

    #[test]
    fn user_prompt_embeds_packet_when_present_and_omits_the_data_block_when_empty() {
        let with = build_user_prompt(&one_index_packet());
        assert!(with.starts_with(USER_INSTRUCTION), "{with}");
        assert!(with.contains("Condensed research packet"), "{with}");
        assert!(with.contains("S&P 500"), "{with}");

        // A default packet still leads with the bare instruction and carries no data block,
        // but is no longer exactly USER_INSTRUCTION — the skills library is appended (below).
        let bare = build_user_prompt(&ResearchPacket::default());
        assert!(bare.starts_with(USER_INSTRUCTION), "{bare}");
        assert!(!bare.contains("Condensed research packet"), "{bare}");
    }

    #[test]
    fn user_prompt_carries_the_skill_library_with_and_without_a_packet() {
        // The lenses are packet-independent, so the full library + its verdict forcing
        // function ride into the prompt in both the populated and default-packet paths.
        for packet in [one_index_packet(), ResearchPacket::default()] {
            let prompt = build_user_prompt(&packet);
            assert!(prompt.contains("Analytical skills"), "intro missing: {prompt}");
            assert!(prompt.contains("Market Regime Analysis"), "a skill name missing: {prompt}");
            assert!(prompt.contains("Verdict to produce —"), "verdict marker missing: {prompt}");
        }
    }

    #[test]
    fn envelope_to_output_tags_posture_and_passes_fields_through() {
        let env = ReviewEnvelope {
            summary: "constructive".into(),
            key_points: vec!["breadth improving".into()],
            risks: vec!["valuation".into()],
            opportunities: vec!["AI capex".into()],
            confidence: Confidence::High,
        };
        let out = envelope_to_output(Posture::Bull, env).unwrap();
        assert_eq!(out.posture, Posture::Bull);
        assert_eq!(out.summary, "constructive");
        assert_eq!(out.confidence, Confidence::High);
        assert_eq!(out.key_points, vec!["breadth improving".to_string()]);
    }

    #[test]
    fn envelope_to_output_rejects_a_blank_summary() {
        // Not fail-soft: an empty-summary review fails the run rather than feeding a
        // blank analyst section into synthesis (`docs/weekly-report-workflow.md §Step 9`).
        let env = ReviewEnvelope {
            summary: "   ".into(),
            key_points: vec!["something".into()],
            risks: vec![],
            opportunities: vec![],
            confidence: Confidence::Medium,
        };
        assert!(envelope_to_output(Posture::Bear, env).is_err());
    }

    #[test]
    fn envelope_to_output_accepts_a_terse_review_with_empty_lists() {
        // Only the summary is required; a spare but real review (empty lists) is
        // legitimate and passes — the lists stay lenient.
        let env = ReviewEnvelope {
            summary: "A spare but real read.".into(),
            key_points: vec![],
            risks: vec![],
            opportunities: vec![],
            confidence: Confidence::Low,
        };
        let out = envelope_to_output(Posture::Balanced, env).unwrap();
        assert!(out.key_points.is_empty());
    }

    #[test]
    #[ignore = "hits a live OpenAI/Anthropic agent model; set the analyst model + provider key"]
    fn analyst_review_smoke() {
        let agent = ModelAnalystAgent::from_env(Posture::Balanced).expect("analyst configured");
        let review = agent.review(&one_index_packet()).expect("review");
        assert_eq!(review.posture, Posture::Balanced);
        assert!(!review.summary.trim().is_empty(), "the review carries a summary");
        eprintln!(
            "analyst review: {} key points, {} risks, {} opportunities, confidence {}",
            review.key_points.len(),
            review.risks.len(),
            review.opportunities.len(),
            review.confidence.as_str()
        );
    }
}
