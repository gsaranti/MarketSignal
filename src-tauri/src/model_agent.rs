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
//! sync — the research executor stayed synchronous too (`research_executor`), so
//! `tokio` lives only at the app-layer seams. The seed of the future
//! `adapters::models` module lives here.
//!
//! The agent's `MainAgentInput` carries the Step-3 baseline market-data scan
//! (`data_sources`), its change view, and the Step-11 condensed research packet —
//! including the packet's Step-10 vector-memory pull; this adapter serializes
//! them into the user message so the report is grounded in this run's live data,
//! research, and recalled memory.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::agent::{
    AnalystOutput, MainAgent, MainAgentInput, MainAgentOutput, MarketCycle, RecentReport,
    ReportSummary, RiskPosture, ThesisStance,
};
use crate::baseline_delta::BaselineDeltas;
use crate::data_sources::BaselineMarketData;
use crate::progress::RunContext;
use crate::research_packet::ResearchPacket;
use crate::skills;

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

/// The forced tool (Anthropic) / json_schema name (OpenAI) for the phase-1 skill
/// selection call. Both feed the same [`SkillSelection`].
const SKILL_SELECTION_TOOL_NAME: &str = "request_analyst_skills";

/// The selection response is just a short list of skill names, so a small ceiling is
/// ample and keeps the extra call fast.
const SKILL_SELECTION_MAX_TOKENS: u32 = 512;

/// System prompt for the phase-1 skill-selection call (`docs/analyst-skills.md`,
/// progressive disclosure). Focused on the one decision — which lenses this week's packet
/// warrants — separate from the full report [`SYSTEM_PROMPT`].
const SKILL_SELECTION_SYSTEM_PROMPT: &str = "You are the Head Market Analyst for Market Signal, \
selecting which analytical skills to apply to this week's report. You are shown this week's market \
data and research and a catalog of available analytical skills, each with a short description. \
Choose the skills whose lens is most relevant to what this week's data and research actually \
warrant — not every skill applies every week. Request only the relevant subset; requesting none \
is acceptable when none clearly apply.";

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
aggregate P/E per exchange, a valuation read to set against the `sectors` performance group \
(`pe` may be null when a sector's aggregate earnings are non-positive or its multiple is \
implausibly inflated by a near-zero earnings base — read a null as no usable valuation, not as zero). \
`industries` is a finer cut than the broad sectors: per exchange, the run's strongest and \
weakest industries by average daily move, each joined with that industry's aggregate P/E where \
available (`pe` may be null when earnings are non-positive, implausibly inflated by a \
near-zero earnings base, or the snapshot lacked it), so you \
can read which narrow groups are rotating and whether they are richly or cheaply valued as \
they do. Treat this as a level read — whether a group is expensive or cheap right now — not as \
a claim about multiple expansion or de-rating over time, which a single snapshot cannot \
support. `market_risk_premium` is the US equity-risk-premium (a near-static annual constant, \
so read its level, not week-to-week change) — the excess return demanded over the risk-free \
rate, a valuation anchor for how richly equities are priced. Use these to ground the regime \
and strategy reads in valuation, not momentum alone.

When present, the prompt also carries this week's news and deep research, condensed by the \
application layer. `news clusters` are the week's most market-significant stories, each a topic \
with a relevance score and its member headlines. `deep-research evidence` is the bounded \
follow-up investigation into the topics that mattered most — each item a topic with its \
findings and their sources, plus the request/stop accounting for the research phase. Use these \
to explain *why* the data moved and to source the Key Market Drivers, the thesis, and the \
Sources section; ground every claim in the provided headlines and evidence rather than your own \
prior knowledge, and treat an absent or empty research block as no qualifying news this run, not \
a quiet market. The prompt may also carry `recalled long-term memory` — prior report summaries \
and durable learnings retrieved from the system's vector memory against this week's research. \
Use it for continuity: to strengthen, weaken, or revise the standing thesis, surface historical \
analogs, and avoid repeating past analytical mistakes. Weigh it as recall, not fresh data — this \
week's baseline and research evidence take precedence where they conflict, and an absent memory \
block simply means nothing relevant was recalled. The prompt may also carry `recent prior reports` — the most recent weekly reports this one \
continues, each with its structured summary metadata and its Markdown body (a body may be \
head-truncated, marked inline). These are the prior reports the Retrospective Audit evaluates: \
read their theses, stances, and flagged risks, and judge directionally how they held up against \
what the market actually did this period. A second memory block may also appear — `recalled \
memory for the retrospective audit` — semantic recall against the recent reports and current \
market state rather than this week's research. Let it *steer* the audit: its `[learning · …]` \
fragments are standing lessons that point at what to scrutinise and that you weigh in the thesis \
and strategy, and its `[summary · …]` fragments are supplementary recall (often older reports \
beyond the recent window). It does not by itself license the section — a learning, or a recalled \
summary, is not a prior report to audit. Write the Retrospective Audit section only when the \
`recent prior reports` block is present; when it is absent (a first report), omit the section \
rather than inventing one, and apply any recalled learnings in the thesis and strategy instead. \
These blocks may overlap; weigh the memory as recall and the recent reports as the reports themselves. The prompt may also \
carry `user-supplied \
research documents` — files the user placed in the research inbox, parsed and condensed by the \
application layer. Treat them as deliberately curated, high-signal sources the user wants \
weighed; cite them like any other source where they inform the analysis. A truncation marker on \
a document means only the head of a longer document is shown — weigh it accordingly rather than \
assuming it is complete.

The prompt also carries `analyst reviews` — three independent reads of this week's research \
packet from a Bull, a Bear, and a Balanced analyst, each with its summary, key points, risks, \
opportunities, and stated confidence. Evaluate them independently and critically rather than \
averaging them: agree with one or more, reject weak reasoning, combine arguments, elevate a \
minority view, or flag unsupported claims, and decide how much weight each perspective earns. \
Do not stage a debate or quote the analysts as characters — the final report is your own \
synthesis in one unified voice, the Market Signal Thesis. When the analyst-reviews block is \
absent, synthesize from the data and research directly.

The prompt may also carry `analytical skills you requested` — structured analytical lenses you \
selected from Market Signal's skill library for this week's packet. Apply each as a lens while you \
reason: let it sharpen the relevant part of your analysis and fold its conclusion into the unified \
thesis and the report's existing sections. Do not write a skill up as its own section or name the \
skills in the report — they are reasoning tools, not report structure. When no skills block is \
present, reason from the data, research, and analyst reviews directly.

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

Within the report body you may embed a small chart where the shape of the data reads more \
clearly shown than told — a yield series, an index path, a spread. Emit it as a \
fenced code block tagged `chart` whose body is a JSON object of exactly this \
shape: {\"type\": \"line\", \"title\": \"10Y vs 2Y Treasury yield, recent weeks\", \
\"series\": [{\"label\": \"10Y\", \"points\": [4.10, 4.21, 4.33], \"emphasis\": \
true}, {\"label\": \"2Y\", \"points\": [4.41, 4.52, 4.60]}]}. The chart has no \
labeled time axis, so its `title` is the only place a time span can appear — \
strongly prefer to name the span the points cover there. Points are plotted \
evenly left-to-right as oldest-to-newest, so use regularly-spaced \
observations and give every series the same number of points (they share one \
x-axis — a chart whose series differ in length is dropped). Every `points` value \
must be a real number taken from the baseline or research data you were given — \
never invent or estimate a series to fill a chart. Use at most three series with \
at most one marked `emphasis` (the single highlighted series). By default each \
point is a time step (oldest-to-newest): \"line\" for a trend or path (a yield \
series, an index path, a spread); \"bar\" for a single signed quantity tracked \
across successive periods, shown as bars above / below zero (an index's week-by- \
week return, the weekly change in jobless claims); and \"area\" for a single \
magnitude over time (a credit spread, a volatility level). Bar and area are drawn \
from a zero baseline, so reach for them when the data is signed or sits near zero, \
and use a line for levels far from zero. \
A \"bar\" chart may instead carry an optional `categories` array — one label per \
point — to compare a quantity across named groups rather than over time (returns \
by sector, the week's biggest movers): {\"type\": \"bar\", \"title\": \"Sector \
returns, week to date (%)\", \"categories\": [\"Tech\", \"Energy\", \
\"Financials\", \"Utilities\"], \"series\": [{\"points\": [2.1, -1.4, 0.6, \
-0.3]}]}. A categorical bar shows at most two series — for a two-series comparison \
(e.g. this week vs. last) give each series a short, distinct `label` and mark one \
`emphasis`, so the legend names which colour is which; for three or more series, \
use a table. Give exactly one category per point (at most 16). Prefer short, distinct category \
labels — a long name is shortened on the axis (its full text stays available on \
hover and to assistive tech), and short labels read most cleanly, so \"Cons. \
Disc.\" / \"Staples\" is better than \"Consumer Discretionary\" / \"Consumer \
Staples\" where you can. `categories` applies only to \"bar\" — a line or area connecting unrelated \
groups would imply a trend that isn't there. Without `categories` there is no \
category axis: a cross-sectional comparison is a categorical bar or a table, never \
time-step points labeled as if they were categories. \
Reach for a chart sparingly and only where it earns its place — most \
reports need none, and prose and tables remain the default.

Alongside the Markdown, classify the report on three axes — risk_posture (risk-on, risk-off, or \
mixed), market_cycle (late-cycle, recessionary, or recovery), and thesis_stance (bullish, \
bearish, mixed, or uncertain) — and provide header_summary_bullets (matching the Header Summary), \
key_risks, unresolved_questions, and forward_outlook_themes. Any of the three arrays may be empty.

Also provide durable_learnings: long-lived analytical lessons from this run worth carrying into \
future reports' reasoning — a mistake the system should avoid repeating, an analytical strategy \
that proved useful, an explicit thesis change, a market pattern worth remembering, or a \
historical analog that became relevant. Hold a high bar: a durable learning is signal that will \
still matter months from now, not a restatement of this week's news or data moves — most weeks \
have none or one, never more than five, and an empty array is the normal case. Write each as a \
single self-contained statement that stands alone without this report's context, because it is \
recalled in isolation, possibly years later.";

const USER_PROMPT: &str =
    "Write this week's Market Signal weekly market report, including its structured summary.";

/// Build the user message: the standing instruction plus, when present, the
/// Step-3 baseline market-data scan serialized as JSON so the model grounds the
/// report in this run's live data rather than its own prior knowledge. An empty
/// baseline (no data gathered — e.g. an offline smoke) falls back to the bare
/// instruction so the prompt never carries an empty data block.
///
/// `audit_memory` is the Step-4 pre-research vector pull, appended as its own block
/// to steer the Retrospective Audit — deliberately distinct from the packet's Step-10
/// research-informed memory block (`docs/weekly-report-workflow.md §Step 10`,
/// replace-not-merge); the two reach the model on separate channels.
///
/// `recent_reports` is the Step-2 recent prior-report context — the bounded set of
/// most-recent reports (structured metadata + Markdown body) rendered as its own block.
/// It is the Retrospective Audit's *auditable object* and its gate: a non-empty block
/// licenses the section, an empty one (a first run) omits it (see [`SYSTEM_PROMPT`]).
fn build_user_prompt(
    baseline: &BaselineMarketData,
    deltas: Option<&BaselineDeltas>,
    research: Option<&ResearchPacket>,
    audit_memory: &[String],
    recent_reports: &[RecentReport],
) -> String {
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

    // The Step-11 condensed research packet, when the research half ran. `baseline` and
    // `deltas` already rode in above from the input's top-level fields, so only the news
    // and research sections are appended here (the packet's own baseline/deltas copies are
    // inert). Each block is omitted when empty so a fail-soft-degraded stage never leaves a
    // blank section in the prompt.
    if let Some(packet) = research {
        if !packet.news_clusters.is_empty() {
            if let Ok(json) = serde_json::to_string_pretty(&packet.news_clusters) {
                prompt.push_str(&format!(
                    "\n\nFiltered news clusters for this week (most market-significant first):\n{json}"
                ));
            }
        }
        if !packet.research.items.is_empty() {
            if let Ok(json) = serde_json::to_string_pretty(&packet.research) {
                prompt.push_str(&format!(
                    "\n\nDeep-research evidence gathered for this week (topics highest-priority first):\n{json}"
                ));
            }
        }
        // The Step-10 research-informed memory pull: fragments are blocks (each
        // carries its own newlines), so they join on blank lines rather than posing
        // as bullets.
        if !packet.memory.is_empty() {
            prompt.push_str(&format!(
                "\n\nRecalled long-term memory, most relevant first (prior report summaries and durable learnings retrieved against this week's research):\n{}",
                packet.memory.join("\n\n")
            ));
        }
        // The Step-6 research-inbox documents: each entry is its own block (a
        // provenance header, an optional truncation marker, and the condensed
        // text), so they join on blank lines like the memory fragments.
        if !packet.inbox_summaries.is_empty() {
            prompt.push_str(&format!(
                "\n\nUser-supplied research documents (from the research inbox, parsed and condensed by the application layer):\n{}",
                packet.inbox_summaries.join("\n\n")
            ));
        }
    }

    // The Step-2 recent prior-report context: the bounded set of most-recent reports —
    // structured summary metadata plus the (possibly truncated) Markdown body — that the
    // main agent reasons over for continuity and that the Retrospective Audit evaluates.
    // This block is the audit's auditable object and its structural gate (its presence
    // licenses the section; see the system prompt). Each report is its own sub-block;
    // omitted entirely on a first run or a degraded read.
    if !recent_reports.is_empty() {
        prompt.push_str(
            "\n\nRecent prior reports, most recent first — the weekly reports this one continues, \
             and the prior reports the Retrospective Audit evaluates. Each carries its structured \
             summary metadata and its Markdown body (a body may be head-truncated, marked inline):",
        );
        for report in recent_reports {
            let meta =
                serde_json::to_string_pretty(&report.summary).unwrap_or_else(|_| "{}".to_string());
            prompt.push_str(&format!("\n\n--- Prior report ---\nSummary metadata:\n{meta}"));
            if !report.markdown.is_empty() {
                prompt.push_str(&format!("\n\nReport body:\n{}", report.markdown));
            }
        }
    }

    // The Step-4 pre-research memory pull, on its own channel (not the packet): the
    // recall the retrospective audit reasons over. Heading is deliberately distinct
    // from the Step-10 "retrieved against this week's research" block above so the two
    // don't read as one. Fragments are blocks (own newlines), so they join on blank
    // lines. It now *steers* the audit (what to scrutinise); the recent-reports block
    // above is what gates and grounds it.
    if !audit_memory.is_empty() {
        prompt.push_str(&format!(
            "\n\nRecalled memory for the retrospective audit, most relevant first (prior report summaries and durable learnings recalled against the recent reports and current market state):\n{}",
            audit_memory.join("\n\n")
        ));
    }

    prompt
}

/// Render the Steps 12–15 analyst reviews as the synthesis block the main agent reasons
/// over (`docs/weekly-report-workflow.md §Step 16`, `docs/agents.md §Synthesis
/// Behavior`). Each review is a labeled sub-block (Bull / Bear / Balanced) carrying that
/// analyst's summary, key points, risks, opportunities, and confidence. Returns an empty
/// string when no reviews ran (the offline/stub path), so the prompt omits the section.
/// Appended after [`build_user_prompt`]'s blocks rather than threaded through its
/// signature — a focused, separately-tested seam — with a heading distinct from the
/// memory and research blocks so they never read as one.
fn format_analyst_reviews(reviews: &[AnalystOutput]) -> String {
    if reviews.is_empty() {
        return String::new();
    }
    let mut block = String::from(
        "\n\nAnalyst reviews of this week's research packet — three independent perspectives to \
         critique and weigh in your synthesis (agree, reject weak reasoning, combine arguments, or \
         elevate a minority view), then write the final report in one unified voice:",
    );
    for r in reviews {
        block.push_str(&format!(
            "\n\n--- {} (confidence: {}) ---\n{}",
            r.posture.display_name(),
            r.confidence.as_str(),
            r.summary
        ));
        if !r.key_points.is_empty() {
            block.push_str(&format!("\nKey points:\n- {}", r.key_points.join("\n- ")));
        }
        if !r.risks.is_empty() {
            block.push_str(&format!("\nRisks:\n- {}", r.risks.join("\n- ")));
        }
        if !r.opportunities.is_empty() {
            block.push_str(&format!("\nOpportunities:\n- {}", r.opportunities.join("\n- ")));
        }
    }
    block
}

/// The model's phase-1 skill request: the names it selected from the catalog. The schema
/// `enum` constrains these to authored catalog names, so an unknown name can't appear (and
/// `skills::select_bodies` drops one defensively regardless).
#[derive(Debug, Deserialize)]
struct SkillSelection {
    skills: Vec<String>,
}

/// JSON Schema for the skill-selection response: a `skills` array whose items are an
/// `enum` of the catalog's names (`docs/analyst-skills.md` — the agent requests from the
/// frontmatter it was shown). Shared by both arms; `additionalProperties` false so OpenAI
/// strict mode accepts it.
fn skill_selection_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "skills": {
                "type": "array",
                "items": { "type": "string", "enum": skills::catalog_names() },
                "description": "Names of the analytical skills relevant to this week's packet (may be empty)."
            }
        },
        "required": ["skills"]
    })
}

/// Anthropic skill-selection request: a non-streaming forced-tool call whose
/// `input_schema` is the selection schema (mirrors the analyst adapter's request shape).
fn build_skill_selection_anthropic_request(model_id: &str, system: &str, user: &str) -> Value {
    json!({
        "model": model_id,
        "max_tokens": SKILL_SELECTION_MAX_TOKENS,
        "stream": false,
        "system": system,
        "tools": [
            {
                "name": SKILL_SELECTION_TOOL_NAME,
                "description": "Request the analytical skills relevant to this week's packet.",
                "strict": true,
                "input_schema": skill_selection_schema()
            }
        ],
        "tool_choice": { "type": "tool", "name": SKILL_SELECTION_TOOL_NAME },
        "messages": [ { "role": "user", "content": user } ]
    })
}

/// OpenAI skill-selection request: non-streaming strict json_schema.
fn build_skill_selection_openai_request(model_id: &str, system: &str, user: &str) -> Value {
    json!({
        "model": model_id,
        "max_completion_tokens": SKILL_SELECTION_MAX_TOKENS,
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "analyst_skill_selection", "strict": true, "schema": skill_selection_schema() }
        },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    })
}

/// Render the phase-2 block: the full bodies of the skills the agent requested, supplied
/// into the generation prompt (`docs/analyst-skills.md`). Each is a labeled sub-block;
/// returns an empty string when no skills were selected (or selection degraded), so the
/// prompt omits the section. Heading is distinct from the analyst-reviews, memory, and
/// research blocks so they never read as one.
fn format_selected_skills(selected: &[&skills::Skill]) -> String {
    if selected.is_empty() {
        return String::new();
    }
    let mut block = String::from(
        "\n\nAnalytical skills you requested for this report — apply each as an analytical lens \
         in your reasoning, folding its conclusion into the unified thesis and the report's \
         existing sections rather than writing it up as a separate section:",
    );
    for s in selected {
        block.push_str(&format!("\n\n--- {} ---\n{}", s.name, s.body));
    }
    block
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
    #[serde(default)]
    durable_learnings: Vec<String>,
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
            "forward_outlook_themes": { "type": "array", "items": { "type": "string" } },
            "durable_learnings": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Rare, self-contained analytical lessons worth carrying into future reports (mistakes to avoid, strategies that proved useful, thesis changes, market patterns, historical analogs). Usually empty; at most five."
            }
        },
        "required": [
            "markdown", "risk_posture", "market_cycle", "thesis_stance",
            "header_summary_bullets", "key_risks", "unresolved_questions",
            "forward_outlook_themes", "durable_learnings"
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
        // Passed through unvalidated: the per-report cap is an application-layer
        // bound (the pipeline's persist step), not a model-contract failure.
        durable_learnings: env.durable_learnings,
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

    /// Phase 1 of progressive disclosure (`docs/analyst-skills.md`): ask the model which
    /// analytical skills this week's packet warrants, given the catalog's frontmatter, and
    /// return the chosen skills' bodies for phase-2 supply.
    ///
    /// **Fail-soft** — any failure (an already-cancelled run, an HTTP error, a malformed
    /// response) degrades to no skills so a flaky selection call never costs the report.
    /// Skills are enrichment, *unlike* the not-fail-soft analyst stage; the report
    /// generation call below keeps its own (unchanged) failure semantics.
    fn select_skills(&self, grounding: &str) -> Vec<&'static skills::Skill> {
        if self.progress.is_cancelled() {
            return Vec::new();
        }
        match self.run_skill_selection(grounding) {
            Ok(selected) => selected,
            Err(e) => {
                eprintln!("skill selection degraded to no skills: {e:#}");
                Vec::new()
            }
        }
    }

    /// The fallible body of [`select_skills`]: build the selection request, make the
    /// non-streaming call, and resolve the requested names to skill bodies.
    fn run_skill_selection(&self, grounding: &str) -> Result<Vec<&'static skills::Skill>> {
        let provider = self.config.model.provider();
        let model_id = self.config.model.model_id();
        let user = format!(
            "{grounding}\n\nAvailable analytical skills (name: description):\n{}\n\nRequest the \
             skills whose lens is most relevant to this week's packet. Request only those that add \
             analytical value; requesting none is fine.",
            skills::frontmatter_catalog()
        );
        let body = match provider {
            Provider::Anthropic => {
                build_skill_selection_anthropic_request(model_id, SKILL_SELECTION_SYSTEM_PROMPT, &user)
            }
            Provider::OpenAi => {
                build_skill_selection_openai_request(model_id, SKILL_SELECTION_SYSTEM_PROMPT, &user)
            }
        };
        let raw = self.call_nonstreaming(provider, &body)?;
        let value = match provider {
            Provider::Anthropic => extract_anthropic_tool_input(&raw, SKILL_SELECTION_TOOL_NAME)?,
            Provider::OpenAi => extract_openai_envelope(&raw)?,
        };
        let selection: SkillSelection =
            serde_json::from_value(value).context("skill selection did not match the schema")?;
        Ok(skills::select_bodies(&selection.skills))
    }

    /// Non-streaming POST + whole-body JSON parse, for the phase-1 selection call — the
    /// report [`ModelMainAgent::call`] consumes an SSE stream, so the small selection
    /// response needs its own plain transport (mirrors `analyst_agent`'s `call`).
    fn call_nonstreaming(&self, provider: Provider, body: &Value) -> Result<Value> {
        let request = match provider {
            Provider::Anthropic => self
                .http
                .post(ANTHROPIC_URL)
                .header("x-api-key", &self.config.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION),
            Provider::OpenAi => self.http.post(OPENAI_URL).bearer_auth(&self.config.api_key),
        };
        let resp = request
            .json(body)
            .send()
            .context("sending skill-selection request")?;
        let status = resp.status();
        let text = resp.text().context("reading skill-selection response body")?;
        if !status.is_success() {
            bail!("skill-selection model returned {status}: {text}");
        }
        serde_json::from_str(&text).context("parsing skill-selection response JSON")
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
                        match u32::from_str_radix(&hex, 16) {
                            // A high surrogate is only half a scalar: JSON encodes a
                            // non-BMP char (e.g. an emoji) as a `😀` pair, and
                            // serde_json — the source of `out.markdown` — recombines it, so
                            // this side channel must too or the two would diverge.
                            Ok(high) if (0xD800..=0xDBFF).contains(&high) => {
                                match peek_unicode_escape(chars.clone()) {
                                    // Low half not fully streamed in yet: park at this
                                    // escape's `\` so the pair re-reads whole next call.
                                    LowHalf::Incomplete => return (out, idx, false),
                                    LowHalf::Escape(low, end)
                                        if (0xDC00..=0xDFFF).contains(&low) =>
                                    {
                                        let scalar =
                                            0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                                        if let Some(ch) = char::from_u32(scalar) {
                                            out.push(ch);
                                        }
                                        for _ in 0..6 {
                                            chars.next(); // consume the peeked `\uXXXX` low half
                                        }
                                        next_consumed = end;
                                    }
                                    // Lone/invalid high surrogate — malformed JSON a real
                                    // provider won't emit; drop it (the report is unaffected).
                                    _ => {}
                                }
                            }
                            Ok(code) => {
                                if let Some(ch) = char::from_u32(code) {
                                    out.push(ch);
                                }
                            }
                            Err(_) => {} // non-hex \u escape — drop, as before
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

/// Outcome of peeking the `\uXXXX` escape that should be a UTF-16 low surrogate, used to
/// resolve a surrogate pair whose halves may be split across stream chunks. The caller
/// passes a *clone* of its iterator, so nothing is consumed unless it acts on the result.
enum LowHalf {
    /// A prefix of `\uXXXX` that has not fully arrived — the caller parks and resumes.
    Incomplete,
    /// A complete `\uXXXX` escape: its code unit and the byte index just past it.
    Escape(u32, usize),
    /// The next bytes are not a `\uXXXX` escape at all — there is no low half here.
    NotEscape,
}

/// Peek a `\uXXXX` escape from `it` (moved in by value — pass a clone so the caller's
/// iterator is untouched). Separates a not-yet-complete escape (resume once more streams
/// in) from a definitely-absent one (a malformed high-surrogate pairing).
fn peek_unicode_escape(mut it: std::str::CharIndices<'_>) -> LowHalf {
    match it.next() {
        None => return LowHalf::Incomplete,
        Some((_, '\\')) => {}
        Some(_) => return LowHalf::NotEscape,
    }
    match it.next() {
        None => return LowHalf::Incomplete,
        Some((_, 'u')) => {}
        Some(_) => return LowHalf::NotEscape,
    }
    let mut hex = String::with_capacity(4);
    let mut end = 0;
    for _ in 0..4 {
        match it.next() {
            Some((j, h)) => {
                hex.push(h);
                end = j + h.len_utf8();
            }
            None => return LowHalf::Incomplete,
        }
    }
    match u32::from_str_radix(&hex, 16) {
        Ok(code) => LowHalf::Escape(code, end),
        Err(_) => LowHalf::NotEscape,
    }
}

impl MainAgent for ModelMainAgent {
    fn generate(&self, input: MainAgentInput) -> Result<MainAgentOutput> {
        let provider = self.config.model.provider();
        let model_id = self.config.model.model_id();
        let schema = response_envelope_schema();
        let mut user = build_user_prompt(
            &input.baseline,
            input.deltas.as_ref(),
            input.research.as_ref(),
            &input.audit_memory,
            &input.recent_reports,
        );
        // Progressive disclosure (`docs/analyst-skills.md`): phase 1 selects the analytical
        // skills this packet warrants (grounded on the data/research assembled above), then
        // phase 2 supplies the chosen bodies into the prompt. Fail-soft — a degraded
        // selection appends nothing and the report still generates.
        let selected_skills = self.select_skills(&user);
        user.push_str(&format_selected_skills(&selected_skills));
        // Steps 12–15 → Step 16: append the analyst reviews the synthesis reasons over.
        // Empty on the offline/stub path, so the block is simply omitted.
        user.push_str(&format_analyst_reviews(&input.analyst_reviews));
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
            "forward_outlook_themes": ["liquidity and breadth"],
            "durable_learnings": ["Breadth divergences preceded the spring pullback; weight them earlier."]
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

    fn sample_review(posture: crate::agent::Posture) -> AnalystOutput {
        AnalystOutput {
            posture,
            summary: format!("{} summary line", posture.as_str()),
            key_points: vec!["kp".into()],
            risks: vec!["rk".into()],
            opportunities: vec!["op".into()],
            confidence: crate::agent::Confidence::High,
        }
    }

    #[test]
    fn analyst_block_renders_each_posture_when_present_and_is_empty_otherwise() {
        use crate::agent::Posture;
        let reviews = [
            sample_review(Posture::Bull),
            sample_review(Posture::Bear),
            sample_review(Posture::Balanced),
        ];
        let block = format_analyst_reviews(&reviews);
        assert!(block.contains("Analyst reviews of this week's research packet"), "{block}");
        assert!(block.contains("Bull Analyst (confidence: high)"), "{block}");
        assert!(block.contains("Bear Analyst"), "{block}");
        assert!(block.contains("Balanced Analyst"), "{block}");
        assert!(block.contains("Key points:"), "{block}");
        // Empty input -> the block is omitted entirely, never rendered blank.
        assert!(format_analyst_reviews(&[]).is_empty());
    }

    #[test]
    fn analyst_block_heading_is_distinct_from_memory_and_research_blocks() {
        let block = format_analyst_reviews(&[sample_review(crate::agent::Posture::Bull)]);
        assert!(!block.contains("Recalled long-term memory"), "{block}");
        assert!(!block.contains("Recalled memory for the retrospective audit"), "{block}");
        assert!(!block.contains("Filtered news clusters"), "{block}");
    }

    #[test]
    fn system_prompt_directs_independent_analyst_synthesis() {
        assert!(SYSTEM_PROMPT.contains("analyst reviews"));
        assert!(SYSTEM_PROMPT.contains("one unified voice"));
    }

    #[test]
    fn user_prompt_embeds_baseline_when_present() {
        use crate::data_sources::{
            Change, DataGap, EconomicRelease, GapReason, GroupKind, Quote,
        };
        let baseline = BaselineMarketData {
            indices: vec![Quote {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                price: 5500.0,
                change: Change::percent(0.4),
                unit: "index points".into(),
            }],
            calendar: vec![EconomicRelease {
                release: "Employment Situation".into(),
                date: "2026-06-05".into(),
                status: "released".into(),
            }],
            gaps: vec![DataGap::new(
                GroupKind::LaborLevels,
                "CES0500000003",
                "Average Hourly Earnings, Total Private",
                GapReason::Rejected,
            )],
            ..Default::default()
        };
        let prompt = build_user_prompt(&baseline, None, None, &[], &[]);
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
            build_user_prompt(&BaselineMarketData::default(), None, None, &[], &[]),
            USER_PROMPT
        );
    }

    fn one_index_baseline() -> BaselineMarketData {
        use crate::data_sources::{Change, Quote};
        BaselineMarketData {
            indices: vec![Quote {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                price: 5_610.0,
                change: Change::percent(0.4),
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
        let prompt = build_user_prompt(&one_index_baseline(), Some(&deltas), None, &[], &[]);
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
        let prompt = build_user_prompt(&one_index_baseline(), None, None, &[], &[]);
        assert!(
            !prompt.contains("Change since the previous report"),
            "{prompt}"
        );
    }

    #[test]
    fn user_prompt_appends_research_packet_when_present() {
        use crate::headline_filter::HeadlineCluster;
        use crate::news::RawHeadline;
        use crate::research_executor::{EvidenceItem, Finding, ResearchEvidence};
        use crate::research_packet::ResearchPacket;

        let packet = ResearchPacket {
            news_clusters: vec![HeadlineCluster {
                topic: "AI / semiconductors".into(),
                summary: "Capex intentions stayed the swing factor.".into(),
                relevance: 0.93,
                headlines: vec![RawHeadline {
                    title: "Nvidia raises capex outlook".into(),
                    url: "https://example.com/nvda".into(),
                    source: "reuters.com".into(),
                    published: Some("2026-06-05".into()),
                    snippet: None,
                }],
            }],
            research: ResearchEvidence {
                items: vec![EvidenceItem {
                    topic: "AI capex".into(),
                    rationale: "Semis led the move.".into(),
                    priority: 0.9,
                    findings: vec![Finding {
                        query: "hyperscaler capex guidance".into(),
                        depth: 1,
                        sources: Vec::new(),
                    }],
                }],
                requests_made: 1,
                stopped_reason: None,
            },
            memory: vec![
                "[summary · 2026-05-28T13:00:00Z] Risk posture: risk-off.".into(),
                "[learning · 2026-05-21T13:00:00Z] Breadth divergences preceded the pullback.".into(),
            ],
            inbox_summaries: vec![
                "### Research document: notes.md (MD)\n\nRates likely hold through summer.".into(),
            ],
            ..Default::default()
        };
        let prompt = build_user_prompt(&one_index_baseline(), None, Some(&packet), &[], &[]);
        // All four packet sections ride into the prompt, grounding the report in the
        // week's news, research, recalled memory, and the user's own documents.
        assert!(prompt.contains("Filtered news clusters"), "{prompt}");
        assert!(prompt.contains("AI / semiconductors"), "{prompt}");
        assert!(prompt.contains("Deep-research evidence"), "{prompt}");
        assert!(prompt.contains("hyperscaler capex guidance"), "{prompt}");
        assert!(prompt.contains("Recalled long-term memory"), "{prompt}");
        assert!(
            prompt.contains("Risk posture: risk-off.\n\n[learning"),
            "memory fragments join on blank lines: {prompt}"
        );
        assert!(prompt.contains("User-supplied research documents"), "{prompt}");
        assert!(prompt.contains("Rates likely hold through summer."), "{prompt}");
    }

    #[test]
    fn user_prompt_omits_research_sections_for_an_empty_packet() {
        use crate::research_packet::ResearchPacket;
        // A fail-soft-degraded run still carries a packet, but with no news, evidence,
        // or recalled memory — no section should appear, leaving the prompt as the
        // baseline-only form.
        let empty = ResearchPacket::default();
        let with_packet = build_user_prompt(&one_index_baseline(), None, Some(&empty), &[], &[]);
        let without = build_user_prompt(&one_index_baseline(), None, None, &[], &[]);
        assert_eq!(with_packet, without, "an empty packet adds nothing to the prompt");
        assert!(!with_packet.contains("Filtered news clusters"), "{with_packet}");
        assert!(!with_packet.contains("Deep-research evidence"), "{with_packet}");
        assert!(!with_packet.contains("Recalled long-term memory"), "{with_packet}");
        assert!(!with_packet.contains("User-supplied research documents"), "{with_packet}");
    }

    #[test]
    fn user_prompt_appends_audit_memory_block_when_present() {
        // The Step-4 pull rides in on its own channel (no packet needed) under a heading
        // that names the retrospective audit; an empty pull adds nothing.
        let audit =
            ["[learning · 2026-05-21T13:00:00Z] Breadth divergences preceded the pullback.".to_string()];
        let with = build_user_prompt(&one_index_baseline(), None, None, &audit, &[]);
        assert!(
            with.contains("Recalled memory for the retrospective audit"),
            "{with}"
        );
        assert!(with.contains("Breadth divergences preceded the pullback."), "{with}");

        let without = build_user_prompt(&one_index_baseline(), None, None, &[], &[]);
        assert!(
            !without.contains("Recalled memory for the retrospective audit"),
            "{without}"
        );
    }

    #[test]
    fn user_prompt_keeps_audit_and_research_memory_blocks_distinct() {
        use crate::research_packet::ResearchPacket;
        // The Step-10 research-informed memory (in the packet) and the Step-4 audit memory
        // (its own channel) must read as two separate blocks, not one merged recall —
        // the doc's replace-not-merge rule made visible in the prompt.
        let packet = ResearchPacket {
            memory: vec!["[summary · 2026-05-28T13:00:00Z] Risk posture: risk-off.".into()],
            ..Default::default()
        };
        let audit =
            ["[learning · 2026-05-21T13:00:00Z] Breadth divergences preceded the pullback.".to_string()];
        let prompt = build_user_prompt(&one_index_baseline(), None, Some(&packet), &audit, &[]);
        assert!(
            prompt.contains("Recalled long-term memory"),
            "Step-10 block present: {prompt}"
        );
        assert!(
            prompt.contains("Recalled memory for the retrospective audit"),
            "Step-4 block present: {prompt}"
        );
        // Distinct headings, so neither is a substring of the other's framing.
        assert_ne!(
            prompt.find("Recalled long-term memory"),
            prompt.find("Recalled memory for the retrospective audit"),
            "the two memory blocks occupy different positions"
        );
    }

    #[test]
    fn user_prompt_appends_recent_reports_block_when_present() {
        // The Step-2 recent prior-report context rides in on its own channel: both the
        // structured summary metadata and the Markdown body reach the model, under a
        // heading distinct from the two memory blocks. An empty list adds nothing.
        let recent = [RecentReport {
            summary: ReportSummary {
                report_id: "prior-1".into(),
                report_type: "weekly_market".into(),
                created_at: "2026-06-07T13:00:00Z".into(),
                risk_posture: RiskPosture::RiskOff,
                market_cycle: MarketCycle::LateCycle,
                thesis_stance: ThesisStance::Bearish,
                header_summary_bullets: vec!["Breadth stayed thin.".into()],
                key_risks: vec![],
                unresolved_questions: vec![],
                forward_outlook_themes: vec![],
            },
            markdown: "## Market Signal Thesis\nDefensive into the print.".into(),
        }];
        let with = build_user_prompt(&one_index_baseline(), None, None, &[], &recent);
        assert!(with.contains("Recent prior reports"), "heading present: {with}");
        // Both the structured metadata and the Markdown body reach the model.
        assert!(with.contains("thesis_stance"), "metadata present: {with}");
        assert!(with.contains("bearish"), "metadata value present: {with}");
        assert!(with.contains("Defensive into the print."), "body present: {with}");

        let without = build_user_prompt(&one_index_baseline(), None, None, &[], &[]);
        assert!(!without.contains("Recent prior reports"), "absent on an empty list: {without}");
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
        // Durable learnings ride on the output as a sibling of the summary —
        // never inside it (the summary metadata schema is closed).
        assert_eq!(out.durable_learnings.len(), 1);
        assert!(out.durable_learnings[0].starts_with("Breadth divergences"));
        let summary_json = serde_json::to_value(&out.summary).unwrap();
        assert!(summary_json.get("durable_learnings").is_none());

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
    fn envelope_without_durable_learnings_still_parses() {
        // Forward/backward-compat: the strict arms always emit the field, but an
        // older fixture (or a provider quirk) without it must read as no learnings,
        // not a parse failure.
        let mut env = valid_envelope();
        env.as_object_mut().unwrap().remove("durable_learnings");
        let raw = json!({ "choices": [ { "message": { "content": env.to_string() } } ] });
        let out = parse_response(Provider::OpenAi, &raw, "r".into(), "t".into()).unwrap();
        assert!(out.durable_learnings.is_empty());
    }

    #[test]
    fn envelope_schema_lists_every_property_as_required() {
        // Both strict arms (the Anthropic forced tool and OpenAI strict json_schema)
        // reject a schema whose `required` omits a declared property — a new envelope
        // field that misses the list fails live, not in offline tests.
        let schema = response_envelope_schema();
        let props = schema["properties"].as_object().unwrap();
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(props.len(), required.len());
        for key in props.keys() {
            assert!(required.contains(&key.as_str()), "{key} missing from required");
        }
        assert!(required.contains(&"durable_learnings"));
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
    fn markdown_extractor_recombines_a_surrogate_pair_split_across_chunks() {
        // U+1F600 (the 😀 emoji) is a non-BMP scalar that JSON encodes as a UTF-16
        // surrogate pair of two `\uXXXX` escapes. The escape *form* is built from raw
        // bytes (0x5C is the backslash, 0x22 the quote) so the source carries the escapes
        // literally and drives the decoder's `\u` path — a pasted emoji would instead hit
        // the plain-char path and prove nothing. Fed one char at a time (the high half
        // lands a full call before the low half), the decoder must hold the high half back
        // and recombine the two into one scalar, never emit a lone surrogate. The
        // reference is serde_json's own decode of the same envelope — that equality is the
        // invariant the live smoke's `streamed == out.markdown` leans on for non-BMP text.
        let bs = char::from(0x5C_u8); // backslash
        let q = char::from(0x22_u8); // double quote
        let body = format!("a{bs}uD83D{bs}uDE00b"); // a😀b
        let envelope = format!("{{{q}markdown{q}:{q}{body}{q}}}");
        let reference = serde_json::from_str::<Value>(&envelope).unwrap()["markdown"]
            .as_str()
            .unwrap()
            .to_string();

        let mut extractor = MarkdownStreamExtractor::default();
        let mut grown = String::new();
        let mut streamed = String::new();
        for ch in envelope.chars() {
            grown.push(ch);
            streamed.push_str(&extractor.update(&grown));
        }
        let expected = format!("a{}b", char::from_u32(0x1_F600).unwrap());
        assert_eq!(streamed, expected);
        assert_eq!(streamed, reference);
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

    /// A non-empty input so a live model has real material to summarise. The empty
    /// `MainAgentInput::default()` led weaker models (notably the Anthropic arm) to emit a
    /// stub with too few header bullets to clear `generate`'s 3–6 validation, which errored
    /// before the streaming assertions below could run. A handful of index/internals/macro
    /// levels plus a change view gives the model enough to write a conforming header.
    fn populated_input() -> MainAgentInput {
        use crate::baseline_delta::{BaselineDeltas, Direction, SeriesDelta};
        use crate::data_sources::{Change, GroupKind, Quote};

        let q = |symbol: &str, name: &str, price: f64, change_pct: f64| Quote {
            symbol: symbol.into(),
            name: name.into(),
            price,
            change: Change::percent(change_pct),
            unit: "index points".into(),
        };
        let baseline = BaselineMarketData {
            indices: vec![
                q("^DJI", "Dow Jones Industrial Average", 41_200.0, 0.8),
                q("^GSPC", "S&P 500", 5_610.0, 1.2),
                q("^IXIC", "Nasdaq Composite", 18_400.0, 1.9),
            ],
            internals: vec![q("^VIX", "CBOE Volatility Index", 14.2, -6.5)],
            macro_levels: vec![
                Quote {
                    symbol: "DGS10".into(),
                    name: "10-Year Treasury Yield".into(),
                    price: 4.18,
                    change: Change::percent(-3.0),
                    unit: "percent".into(),
                },
                Quote {
                    symbol: "FEDFUNDS".into(),
                    name: "Federal Funds Target (upper)".into(),
                    price: 4.50,
                    change: Change::percent(0.0),
                    unit: "percent".into(),
                },
            ],
            ..Default::default()
        };
        let deltas = BaselineDeltas {
            elapsed_days: 7.0,
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
        MainAgentInput {
            baseline,
            deltas: Some(deltas),
            ..Default::default()
        }
    }

    #[test]
    #[ignore = "hits the live provider API; set MARKET_SIGNAL_MAIN_AGENT_MODEL + the provider key"]
    fn live_generate_and_stream_smoke() {
        use crate::progress::{ProgressEvent, RecordingReporter};
        use std::sync::atomic::AtomicBool;

        // A recording context instead of `noop`, so the streamed-token side-channel is
        // captured against the real SSE wire (real chunk boundaries, keep-alives, escape
        // splits) — the bare `generate`'s no-op context drops every `agent_token`, which
        // is why the decoder was previously only fixture-tested.
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("stream-smoke", rec.clone(), Arc::new(AtomicBool::new(false)));
        let agent = ModelMainAgent::from_env()
            .expect("env configured for a live run")
            .with_context(ctx);

        let out = agent
            .generate(populated_input())
            .expect("live generate");

        // Envelope accumulation + `reconstruct_response` are proven by a clean parse.
        assert!(!out.markdown.is_empty());
        assert!((3..=6).contains(&out.summary.header_summary_bullets.len()));

        // The streamed-token side-channel: concatenating the coalesced AgentToken deltas
        // must rebuild the report markdown exactly — proving the resumable decoder handled
        // the live wire. A clean parse with zero tokens would mean the decoder silently
        // emitted nothing; the non-empty assert localizes that regression.
        let streamed: String = rec
            .messages()
            .iter()
            .filter_map(|m| match &m.event {
                ProgressEvent::AgentToken { delta } => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            !streamed.is_empty(),
            "no AgentToken events were emitted by the live stream"
        );
        assert_eq!(
            streamed, out.markdown,
            "streamed tokens did not reconstruct the report markdown"
        );
    }

    // --- Progressive-disclosure skill selection (docs/analyst-skills.md) ---

    #[test]
    fn skill_selection_schema_enumerates_the_catalog_names() {
        let schema = skill_selection_schema();
        let enumed = schema["properties"]["skills"]["items"]["enum"]
            .as_array()
            .expect("the skills items carry an enum");
        for name in skills::catalog_names() {
            assert!(
                enumed.iter().any(|v| v == name),
                "selection enum missing catalog skill {name}"
            );
        }
        assert_eq!(schema["required"][0], "skills");
    }

    #[test]
    fn skill_selection_anthropic_request_forces_the_tool_and_is_not_streamed() {
        let body = build_skill_selection_anthropic_request(
            "claude-opus-4-8",
            SKILL_SELECTION_SYSTEM_PROMPT,
            "u",
        );
        assert_eq!(body["model"], "claude-opus-4-8");
        assert_eq!(body["stream"], false);
        assert_eq!(body["tool_choice"]["name"], SKILL_SELECTION_TOOL_NAME);
        assert_eq!(body["tools"][0]["name"], SKILL_SELECTION_TOOL_NAME);
        assert_eq!(body["tools"][0]["strict"], true);
    }

    #[test]
    fn skill_selection_openai_request_uses_strict_json_schema() {
        let body =
            build_skill_selection_openai_request("gpt-5", SKILL_SELECTION_SYSTEM_PROMPT, "u");
        assert_eq!(body["model"], "gpt-5");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            "analyst_skill_selection"
        );
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    }

    #[test]
    fn skill_selection_envelope_parses_and_resolves_to_bodies() {
        let value = json!({ "skills": [skills::CATALOG[0].name, "No Such Skill"] });
        let sel: SkillSelection = serde_json::from_value(value).unwrap();
        let bodies = skills::select_bodies(&sel.skills);
        assert_eq!(bodies.len(), 1, "unknown name dropped");
        assert_eq!(bodies[0].name, skills::CATALOG[0].name);
    }

    #[test]
    fn selected_skills_block_renders_when_present_and_is_empty_otherwise() {
        let chosen = skills::select_bodies(&[skills::CATALOG[0].name.to_string()]);
        let block = format_selected_skills(&chosen);
        assert!(block.contains(skills::CATALOG[0].name), "{block}");
        assert!(block.contains(skills::CATALOG[0].body), "the full body is supplied");
        assert!(block.contains("apply each as an analytical lens"), "{block}");

        assert_eq!(format_selected_skills(&[]), "");
    }

    #[test]
    fn selected_skills_block_heading_is_distinct_from_other_blocks() {
        let chosen = skills::select_bodies(&[skills::CATALOG[0].name.to_string()]);
        let block = format_selected_skills(&chosen);
        assert!(block.contains("Analytical skills you requested for this report"));
        // Must not collide with the analyst-reviews, memory, or research headings.
        assert!(!block.contains("Analyst reviews of this week's research packet"));
        assert!(!block.contains("Recalled long-term memory"));
        assert!(!block.contains("Deep-research evidence"));
    }

    #[test]
    fn system_prompt_directs_skill_application() {
        assert!(SYSTEM_PROMPT.contains("analytical skills you requested"));
        assert!(SYSTEM_PROMPT.contains("reasoning tools, not report structure"));
    }

    #[test]
    #[ignore = "hits the live provider API; set MARKET_SIGNAL_MAIN_AGENT_MODEL + the provider key"]
    fn live_skill_selection_smoke() {
        let agent = ModelMainAgent::from_env().expect("env configured for a live run");
        let input = populated_input();
        let grounding = build_user_prompt(
            &input.baseline,
            input.deltas.as_ref(),
            input.research.as_ref(),
            &input.audit_memory,
            &input.recent_reports,
        );
        let chosen = agent.run_skill_selection(&grounding).expect("skill selection");
        eprintln!(
            "skill selection chose {} skill(s): {:?}",
            chosen.len(),
            chosen.iter().map(|s| s.name).collect::<Vec<_>>()
        );
        // Whatever it picked, every result is an authored catalog skill.
        for s in &chosen {
            assert!(skills::catalog_names().contains(&s.name));
        }
    }
}
