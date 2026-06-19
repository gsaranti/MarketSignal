//! The research-routing stage: a pure structured-in / structured-out boundary
//! that turns this run's baseline, its change view, the Step-7 news clusters, and
//! the recent prior-report summaries into a bounded research plan
//! (`docs/report-workflow.md §Step 8`).
//!
//! This is Step 8. Step 7 narrows ~500 headlines to ~10 clusters
//! (`headline_filter`); routing decides which at most ~5 of those topics — plus
//! whatever the baseline moves and the change view surface — deserve the deep
//! investigation that Step 9's bounded executor will carry out. The plan defines
//! *what* to investigate; it does not loop or fetch (`docs/agents.md §Research
//! Routing`).
//!
//! Mirrors the `headline_filter` spine: the trait method is synchronous and pure,
//! a deterministic `StubResearchRouter` stands in offline, and the real
//! `ModelResearchRouter` (its blocking Claude Sonnet HTTP call) replaces the stub
//! behind the same trait. Routing uses the fixed mid-tier model (Claude Sonnet,
//! `docs/agents.md §Research Routing`) — non-configurable, distinct from the
//! user-selectable agent models. Like the Step-7 filter, this stage runs inside
//! `pipeline::assemble_research_packet`; the plan's consumer is the Step-9
//! executor.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::ReportSummary;
use crate::baseline_delta::BaselineDeltas;
use crate::data_sources::BaselineMarketData;
use crate::headline_filter::HeadlineCluster;
use crate::model_agent::{extract_anthropic_tool_input, ANTHROPIC_VERSION};
use crate::news::RawHeadline;
use crate::progress::RunContext;

/// The bounded ceiling on topics the plan carries — the "~5 deeply analyzed
/// topics" Step 7's funnel hands to routing (`docs/report-workflow.md
/// §Step 7`). Enforced in `plan_from_envelope` (the model is asked for at most
/// this many, but the cap is applied deterministically rather than trusted).
pub const MAX_RESEARCH_ITEMS: usize = 5;

/// The per-topic ceiling on concrete research questions. A shape bound, not the
/// hard request budget — the 50-request / depth-2 / 30-minute limits live in the
/// Step-9 executor (`docs/report-workflow.md §Step 9`), not the plan — but
/// keeping each topic to a handful of directions stops a lax model response from
/// handing the executor an unbounded fan-out (5 topics × 4 × depth-2 stays inside
/// the 50-request budget with headroom).
const MAX_QUERIES_PER_ITEM: usize = 4;

/// One topic the router flagged for deeper investigation: a label, a rationale
/// tying it to the evidence, the model's 0.0–1.0 priority (used to rank and cap
/// the set), and the concrete questions Step 9 should pursue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchItem {
    pub topic: String,
    pub rationale: String,
    pub priority: f64,
    pub queries: Vec<String>,
}

/// The bounded research plan (`docs/report-workflow.md §Step 8`): the
/// topics that deserve deeper investigation this report, ranked by priority and
/// capped at `MAX_RESEARCH_ITEMS`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ResearchPlan {
    pub items: Vec<ResearchItem>,
}

/// What the router reasons over. Carries the Step-8 inputs that exist today: the
/// Step-3 baseline scan, its deterministic change view (`baseline_delta`), the
/// Step-7 clusters, and the recent prior-report summaries — the thesis-continuity
/// context through which the main agent shapes research indirectly
/// (`docs/agents.md` names prior-report context and unresolved thesis questions as
/// routing's continuity inputs). Upcoming known events already ride in on
/// `baseline.calendar` (the economic-release schedule), so they carry no separate
/// field. The doc's "recent Markdown report context" ships here in bounded
/// summary form — whether the full Markdown bodies ever feed routing (rather than
/// only the Step-2 main-agent context, a later slice) is that slice's call. With
/// the inbox-parsing slice's `inbox_documents`, all seven doc-listed Step-8
/// inputs now ride in.
#[derive(Debug, Clone, Default)]
pub struct RouterInput {
    pub baseline: BaselineMarketData,
    pub deltas: Option<BaselineDeltas>,
    pub clusters: Vec<HeadlineCluster>,
    /// Summaries of the most recent prior reports, newest first, bounded at the
    /// read site (`pipeline::load_recent_report_context`). Empty on a first
    /// report or when the best-effort read degraded.
    pub recent_reports: Vec<ReportSummary>,
    /// The Step-4 pre-research vector-memory pull (`docs/report-workflow.md
    /// §Step 4`): recalled fragments, most relevant first, each in the
    /// `MemoryHit::prompt_fragment` form. Ephemeral routing context only — the
    /// packet carries the separate Step-10 pull, never this one. Empty on an
    /// early run's bare store or when the fail-soft pull degraded.
    pub memory: Vec<String>,
    /// The Step-6 parsed research-inbox documents (`docs/report-workflow.md
    /// §Step 8`'s "parsed research inbox documents"): one short excerpt block per
    /// successfully parsed user-supplied document
    /// (`document_parser::ParsedResearchDoc::router_excerpt`) — routing picks
    /// topics, so it gets the head, not the full condensed text the packet
    /// carries. Empty when the inbox was empty or every file failed.
    pub inbox_documents: Vec<String>,
}

/// The research-routing stage. One method: turn the inputs into a bounded plan.
/// Sync and pure, like the `HeadlineFilter` trait — the blocking model HTTP call
/// inside the real adapter is offloaded via `spawn_blocking` at the Tauri command
/// seam.
pub trait ResearchRouter {
    fn route(&self, input: RouterInput) -> anyhow::Result<ResearchPlan>;
}

/// Deterministic offline stand-in for the real model router. Derives a bounded
/// plan straight from the clusters: the top `MAX_RESEARCH_ITEMS` by relevance
/// each become one topic with a single templated query, so the pipeline and its
/// tests run without live keys. No clusters yields an empty plan.
#[derive(Debug, Default)]
pub struct StubResearchRouter;

impl ResearchRouter for StubResearchRouter {
    fn route(&self, input: RouterInput) -> anyhow::Result<ResearchPlan> {
        let mut clusters = input.clusters;
        clusters.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let items = clusters
            .into_iter()
            .take(MAX_RESEARCH_ITEMS)
            .map(|c| ResearchItem {
                queries: vec![format!("Investigate {}", c.topic)],
                topic: c.topic,
                rationale: c.summary,
                priority: c.relevance.clamp(0.0, 1.0),
            })
            .collect();
        Ok(ResearchPlan { items })
    }
}

/// Anthropic Messages endpoint — the fixed internal stages call the provider
/// directly (the user-selectable agent models live behind `model_agent`).
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";

/// The fixed internal model for research routing (`docs/agents.md §Research
/// Routing`) — non-configurable, distinct from the user-selectable agent models.
const RESEARCH_ROUTER_MODEL: &str = "claude-sonnet-4-6";

/// The single tool the router forces; its `input` is the plan envelope.
const TOOL_NAME: &str = "emit_research_plan";

/// The plan is small (≤5 topics, each a label + short rationale + a few
/// questions), so a modest ceiling is ample and keeps the response inside the
/// HTTP timeout.
const MAX_TOKENS: u32 = 4096;

const SYSTEM_PROMPT: &str =
    "You are the research router for Market Signal's market report. \
Given the current baseline market data, the change since the previous report, the filtered \
news clusters, summaries of recent prior reports, and any recalled long-term memory, decide \
which topics deserve deeper investigation for this report. Favor topics where the data moved \
materially, where second-order implications matter, or where a known upcoming event could move \
markets. When prior-report summaries are provided, weigh thesis continuity: favor topics that \
answer a prior report's unresolved questions or test its key risks and forward outlook themes \
against the current data. When long-term memory fragments are provided — prior report summaries \
and durable learnings recalled against the current market picture — use them to steer \
investigation: favor topics that test a recurring pattern, revisit a past analytical mistake, or \
probe a historical analog the memory surfaces. When user-supplied research documents are \
provided — files the user placed in the research inbox, parsed and excerpted by the application \
layer — treat them as deliberately curated, high-signal input: favor topics they raise or \
substantiate when the market evidence supports them. \
Return at most 5 topics — fewer if fewer matter. For each topic give: a \
short topic label, a one-to-two-sentence rationale tying it to the provided evidence, a priority \
from 0.0 to 1.0 for how much it matters now, and at most 4 concrete research questions to \
investigate. Ground every topic in the provided baseline, change view, clusters, prior-report \
summaries, recalled memory, or user-supplied documents; never invent data.";

const USER_INSTRUCTION: &str = "Produce the bounded research plan for this report.";

/// The model's structured return: a list of plan items. Mirrors the
/// `headline_filter` envelope — the deterministic caps live in
/// `plan_from_envelope`, not the schema, since strict mode does not honor
/// array-length constraints.
#[derive(Debug, Deserialize)]
struct PlanEnvelope {
    #[serde(default)]
    items: Vec<ItemRaw>,
}

#[derive(Debug, Deserialize)]
struct ItemRaw {
    #[serde(default)]
    topic: String,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    priority: f64,
    #[serde(default)]
    queries: Vec<String>,
}

/// JSON Schema for the plan envelope, used as the Anthropic tool's strict
/// `input_schema`. All fields required and `additionalProperties` false for strict
/// mode; the ≤5-item / ≤4-query caps are enforced in `plan_from_envelope`.
fn plan_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "topic": { "type": "string" },
                        "rationale": { "type": "string" },
                        "priority": { "type": "number" },
                        "queries": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["topic", "rationale", "priority", "queries"]
                }
            }
        },
        "required": ["items"]
    })
}

/// Resolve the model's envelope into a plan, enforcing the routing invariants
/// deterministically rather than trusting the model:
///
/// - **Rank by priority first**, so the strongest topics win the item cap.
/// - **Drop malformed topics** — a blank label or rationale is useless to Step 9.
/// - **Clean each topic's queries** — drop blank ones and cap at
///   `MAX_QUERIES_PER_ITEM`; a topic left with no research direction is dropped.
/// - **Clamp priority** to 0.0–1.0 and **cap topics** at `MAX_RESEARCH_ITEMS`.
///
/// Pure, so these invariants are unit-testable without a live call.
fn plan_from_envelope(env: PlanEnvelope) -> ResearchPlan {
    let mut ranked = env.items;
    ranked.sort_by(|a, b| {
        b.priority
            .partial_cmp(&a.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut items: Vec<ResearchItem> = Vec::new();
    for raw in ranked {
        if items.len() >= MAX_RESEARCH_ITEMS {
            break;
        }
        if raw.topic.trim().is_empty() || raw.rationale.trim().is_empty() {
            continue;
        }
        let queries: Vec<String> = raw
            .queries
            .into_iter()
            .filter(|q| !q.trim().is_empty())
            .take(MAX_QUERIES_PER_ITEM)
            .collect();
        // A topic with no concrete question gives the executor nothing to run.
        if queries.is_empty() {
            continue;
        }
        items.push(ResearchItem {
            topic: raw.topic,
            rationale: raw.rationale,
            priority: raw.priority.clamp(0.0, 1.0),
            queries,
        });
    }
    ResearchPlan { items }
}

/// Render the clusters as a compact numbered list for the prompt: each topic with
/// its relevance and summary, followed by an indented list of its member
/// headlines. The headlines are the primary-source signal — Step 7 keeps a bounded
/// set per cluster (`headline_filter::MAX_RETAINED_HEADLINES`) precisely so they
/// ride into this packet — and routing needs them: the cluster `summary` is the
/// weaker headline-filter model's compression, so Sonnet validates it and grounds
/// its queries against the actual titles, sources, and snippets rather than the
/// derived gloss alone. The count is already bounded by the funnel, so no extra cap
/// is applied here.
fn format_clusters(clusters: &[HeadlineCluster]) -> String {
    clusters
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut block = format!("[{i}] ({:.2}) {} — {}", c.relevance, c.topic, c.summary);
            for h in &c.headlines {
                block.push_str(&format!("\n    - {}", format_headline(h)));
            }
            block
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One member headline as a compact prompt line: the title and source, with the
/// publish date and snippet appended when present. The `url` is dropped — it adds
/// length without helping the router reason — while the title, source, date, and
/// snippet are the grounding Sonnet checks the cluster summary against.
fn format_headline(h: &RawHeadline) -> String {
    let mut line = match &h.published {
        Some(date) => format!("{} ({}, {})", h.title, h.source, date),
        None => format!("{} ({})", h.title, h.source),
    };
    if let Some(snippet) = &h.snippet {
        line.push_str(&format!(": {snippet}"));
    }
    line
}

/// Build the user message: the standing instruction plus this run's evidence —
/// the baseline scan and its change view serialized as JSON (so the router reasons
/// over the live numbers, mirroring the main agent's prompt), and the news
/// clusters. Each block is omitted when empty so the prompt never carries a blank
/// section.
fn build_user_prompt(input: &RouterInput) -> String {
    let mut prompt = USER_INSTRUCTION.to_string();

    if input.baseline != BaselineMarketData::default() {
        if let Ok(json) = serde_json::to_string_pretty(&input.baseline) {
            prompt.push_str(&format!(
                "\n\nBaseline market data for this report:\n{json}"
            ));
        }
    }

    if let Some(d) = &input.deltas {
        if let Ok(json) = serde_json::to_string_pretty(d) {
            prompt.push_str(&format!(
                "\n\nChange since the previous report (its baseline was captured ~{:.1} days ago):\n{json}",
                d.elapsed_days
            ));
        }
    }

    if !input.recent_reports.is_empty() {
        if let Ok(json) = serde_json::to_string_pretty(&input.recent_reports) {
            prompt.push_str(&format!(
                "\n\nSummaries of recent prior reports, most recent first (the evolving thesis):\n{json}"
            ));
        }
    }

    // The Step-4 pre-research memory pull: fragments are blocks (each carries its
    // own newlines), so they join on blank lines rather than posing as bullets.
    if !input.memory.is_empty() {
        prompt.push_str(&format!(
            "\n\nRecalled long-term memory, most relevant first (prior report summaries and durable learnings):\n{}",
            input.memory.join("\n\n")
        ));
    }

    if !input.clusters.is_empty() {
        prompt.push_str(&format!(
            "\n\nFiltered news clusters:\n{}",
            format_clusters(&input.clusters)
        ));
    }

    // The Step-6 parsed inbox documents: like the memory fragments, each entry is
    // its own block (header + excerpt), so they join on blank lines.
    if !input.inbox_documents.is_empty() {
        prompt.push_str(&format!(
            "\n\nUser-supplied research documents (parsed excerpts from the research inbox):\n{}",
            input.inbox_documents.join("\n\n")
        ));
    }

    prompt
}

/// Build the Sonnet request: a non-streaming forced-tool call whose `input_schema`
/// is the plan envelope. Unlike the main agent's streaming call, the plan is small
/// and internal, so a single non-streaming response is returned and parsed whole.
fn build_request(input: &RouterInput) -> Value {
    json!({
        "model": RESEARCH_ROUTER_MODEL,
        "max_tokens": MAX_TOKENS,
        "stream": false,
        "system": [
            { "type": "text", "text": SYSTEM_PROMPT, "cache_control": { "type": "ephemeral" } }
        ],
        "tools": [
            {
                "name": TOOL_NAME,
                "description": "Emit the bounded research plan for this report.",
                "strict": true,
                "input_schema": plan_schema()
            }
        ],
        "tool_choice": { "type": "tool", "name": TOOL_NAME },
        "messages": [ { "role": "user", "content": build_user_prompt(input) } ]
    })
}

/// Live Claude Sonnet adapter behind the `ResearchRouter` trait.
pub struct ModelResearchRouter {
    api_key: String,
    http: reqwest::blocking::Client,
    /// Run context for the single tracker row the routing call emits. Defaults to a
    /// no-op (tests / offline smokes); the live command attaches the real one via
    /// [`ModelResearchRouter::with_context`].
    progress: Arc<RunContext>,
}

impl ModelResearchRouter {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("building the research-router HTTP client")?;
        Ok(Self {
            api_key,
            http,
            progress: RunContext::noop(),
        })
    }

    /// Attach a live run context so the routing call streams a request row to the tracker.
    /// Without it the adapter keeps its no-op context.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Resolve the adapter from the environment, for the live smoke and any caller
    /// that bypasses the gate. Uses the Anthropic key — research routing is a fixed
    /// internal Anthropic stage (`config::anthropic_key`).
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().anthropic_key()?)
    }

    fn call(&self, body: &Value) -> Result<Value> {
        let resp = self
            .http
            .post(ANTHROPIC_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(body)
            .send()
            .context("sending research-router request")?;
        let status = resp.status();
        let text = resp
            .text()
            .context("reading research-router response body")?;
        if !status.is_success() {
            bail!("research-router model returned {status}: {text}");
        }
        serde_json::from_str(&text).context("parsing research-router response JSON")
    }
}

/// Nothing to route: no baseline data, no change view, no clusters, no
/// prior-report context, no recalled memory, and no inbox documents means the
/// prompt would carry only the bare instruction. Pure so the conjunction stays
/// unit-testable as inputs join it slice by slice.
fn input_is_empty(input: &RouterInput) -> bool {
    input.baseline == BaselineMarketData::default()
        && input.deltas.is_none()
        && input.clusters.is_empty()
        && input.recent_reports.is_empty()
        && input.memory.is_empty()
        && input.inbox_documents.is_empty()
}

impl ResearchRouter for ModelResearchRouter {
    fn route(&self, input: RouterInput) -> Result<ResearchPlan> {
        // Short-circuit rather than spend a call on an empty packet.
        if input_is_empty(&input) {
            return Ok(ResearchPlan::default());
        }
        // One tracker row for the Sonnet routing call.
        self.progress.request_started(
            "Anthropic",
            "routing",
            "research-router",
            "Research routing",
        );
        let result = (|| -> Result<ResearchPlan> {
            let raw = self.call(&build_request(&input))?;
            let value = extract_anthropic_tool_input(&raw, TOOL_NAME)?;
            let env: PlanEnvelope = serde_json::from_value(value)
                .context("research-router response did not match the schema")?;
            Ok(plan_from_envelope(env))
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "Anthropic",
                "routing",
                "research-router",
                "Research routing",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "Anthropic",
                "routing",
                "research-router",
                "Research routing",
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

    use crate::agent::{MarketCycle, RiskPosture, ThesisStance};

    fn cluster(topic: &str, relevance: f64) -> HeadlineCluster {
        HeadlineCluster {
            topic: topic.into(),
            summary: format!("{topic} summary"),
            relevance,
            headlines: Vec::new(),
        }
    }

    fn prior_summary(report_id: &str) -> ReportSummary {
        ReportSummary {
            report_id: report_id.into(),
            report_type: "weekly_market".into(),
            created_at: "2026-06-04T13:00:00Z".into(),
            risk_posture: RiskPosture::RiskOff,
            market_cycle: MarketCycle::LateCycle,
            thesis_stance: ThesisStance::Bearish,
            header_summary_bullets: vec!["Credit spreads widened".into()],
            key_risks: vec!["A refinancing wall into Q3".into()],
            unresolved_questions: vec!["Will the labor market crack?".into()],
            forward_outlook_themes: vec!["Defensive rotation".into()],
        }
    }

    #[test]
    fn stub_derives_a_bounded_plan_from_clusters_by_relevance() {
        // 7 clusters of ascending relevance -> capped at the top 5.
        let clusters: Vec<HeadlineCluster> = (0..7)
            .map(|i| cluster(&format!("t{i}"), f64::from(i) / 7.0))
            .collect();
        let plan = StubResearchRouter
            .route(RouterInput {
                clusters,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(plan.items.len(), MAX_RESEARCH_ITEMS);
        // Sorted by relevance desc, so the strongest survives and the weakest is cut.
        assert!(plan.items[0].priority > plan.items[MAX_RESEARCH_ITEMS - 1].priority);
        assert!(plan
            .items
            .iter()
            .all(|it| it.queries.len() == 1 && !it.queries[0].trim().is_empty()));
    }

    #[test]
    fn stub_on_empty_input_yields_an_empty_plan() {
        let plan = StubResearchRouter.route(RouterInput::default()).unwrap();
        assert!(plan.items.is_empty());
    }

    #[test]
    fn plan_from_envelope_ranks_caps_and_clamps() {
        // 7 items, ascending priority, one with an out-of-range priority.
        let items: Vec<ItemRaw> = (0..7)
            .map(|i| ItemRaw {
                topic: format!("t{i}"),
                rationale: "why".into(),
                priority: if i == 6 { 1.5 } else { f64::from(i) / 7.0 },
                queries: vec![format!("q{i}")],
            })
            .collect();
        let plan = plan_from_envelope(PlanEnvelope { items });
        assert_eq!(
            plan.items.len(),
            MAX_RESEARCH_ITEMS,
            "capped at the item ceiling"
        );
        // The 1.5 priority sorts first and is clamped to 1.0.
        assert!(
            (plan.items[0].priority - 1.0).abs() < 1e-9,
            "priority clamped to 1.0"
        );
        assert!(plan.items[0].priority >= plan.items[MAX_RESEARCH_ITEMS - 1].priority);
    }

    #[test]
    fn plan_from_envelope_drops_blank_topic_or_rationale() {
        let items = vec![
            // Highest priority but a blank topic -> dropped.
            ItemRaw {
                topic: "   ".into(),
                rationale: "r".into(),
                priority: 0.99,
                queries: vec!["q".into()],
            },
            // Blank rationale -> dropped too.
            ItemRaw {
                topic: "t".into(),
                rationale: String::new(),
                priority: 0.9,
                queries: vec!["q".into()],
            },
            // Well-formed -> survives.
            ItemRaw {
                topic: "good".into(),
                rationale: "real".into(),
                priority: 0.5,
                queries: vec!["q".into()],
            },
        ];
        let plan = plan_from_envelope(PlanEnvelope { items });
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].topic, "good");
    }

    #[test]
    fn plan_from_envelope_cleans_queries_and_drops_queryless_items() {
        let items = vec![
            // Five queries, one blank -> blank dropped, rest capped at the per-item ceiling.
            ItemRaw {
                topic: "t".into(),
                rationale: "r".into(),
                priority: 0.9,
                queries: vec![
                    "q1".into(),
                    "  ".into(),
                    "q2".into(),
                    "q3".into(),
                    "q4".into(),
                    "q5".into(),
                ],
            },
            // No usable query -> the whole item is dropped.
            ItemRaw {
                topic: "empty".into(),
                rationale: "r".into(),
                priority: 0.8,
                queries: vec!["   ".into()],
            },
        ];
        let plan = plan_from_envelope(PlanEnvelope { items });
        assert_eq!(plan.items.len(), 1, "the queryless item is dropped");
        assert_eq!(
            plan.items[0].queries.len(),
            MAX_QUERIES_PER_ITEM,
            "capped per item"
        );
        assert!(plan.items[0].queries.iter().all(|q| !q.trim().is_empty()));
    }

    #[test]
    fn research_plan_round_trips_through_serde() {
        let plan = ResearchPlan {
            items: vec![ResearchItem {
                topic: "AI capex".into(),
                rationale: "Semis led the move; capex intentions are the swing factor.".into(),
                priority: 0.88,
                queries: vec!["What did hyperscaler capex guidance say?".into()],
            }],
        };
        let json = serde_json::to_string(&plan).unwrap();
        let back: ResearchPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(plan, back);
    }

    #[test]
    fn build_request_targets_sonnet_with_a_forced_tool_and_the_evidence() {
        let mut c = cluster("AI / semiconductors", 0.92);
        c.headlines = vec![RawHeadline {
            title: "Nvidia raises capex outlook".into(),
            url: "https://example.com/nvda".into(),
            source: "reuters.com".into(),
            published: Some("2026-06-05".into()),
            snippet: Some("Hyperscaler demand stays strong.".into()),
        }];
        let input = RouterInput {
            clusters: vec![c],
            ..Default::default()
        };
        let body = build_request(&input);
        assert_eq!(body["model"], RESEARCH_ROUTER_MODEL);
        assert_eq!(body["stream"], false);
        assert_eq!(body["tool_choice"]["name"], TOOL_NAME);
        assert_eq!(body["tools"][0]["name"], TOOL_NAME);
        assert_eq!(body["tools"][0]["strict"], true);
        // The user message carries the cluster line and, critically, its member
        // headline's primary-source detail (title + source) — not just the
        // model-derived summary.
        let user = body["messages"][0]["content"].as_str().unwrap();
        assert!(user.contains("[0] (0.92) AI / semiconductors"), "{user}");
        assert!(
            user.contains("Nvidia raises capex outlook (reuters.com, 2026-06-05)"),
            "{user}"
        );
    }

    #[test]
    fn build_request_carries_recent_report_context_and_omits_it_when_absent() {
        // With prior summaries: the prompt carries the block, the continuity signal
        // (unresolved questions / key risks), and the canonical kebab regime labels —
        // the same serialized form the rest of the system speaks.
        let input = RouterInput {
            recent_reports: vec![prior_summary("prior-1")],
            ..Default::default()
        };
        let body = build_request(&input);
        let user = body["messages"][0]["content"].as_str().unwrap();
        assert!(user.contains("Summaries of recent prior reports"), "{user}");
        assert!(user.contains("Will the labor market crack?"), "{user}");
        assert!(user.contains("A refinancing wall into Q3"), "{user}");
        assert!(user.contains("risk-off"), "{user}");

        // Without them: the block is omitted entirely, never rendered blank.
        let bare = build_user_prompt(&RouterInput {
            clusters: vec![cluster("t", 0.5)],
            ..Default::default()
        });
        assert!(
            !bare.contains("Summaries of recent prior reports"),
            "{bare}"
        );
    }

    #[test]
    fn route_on_empty_input_returns_an_empty_plan_without_a_call() {
        // A dummy key is fine: an empty packet short-circuits before any network call.
        let router = ModelResearchRouter::new("sk-test".into()).unwrap();
        assert!(router
            .route(RouterInput::default())
            .unwrap()
            .items
            .is_empty());
    }

    #[test]
    fn build_request_carries_recalled_memory_and_omits_it_when_absent() {
        // With recalled fragments: the prompt carries the block, with the fragments
        // joined as blocks (blank-line separated), most relevant first.
        let input = RouterInput {
            memory: vec![
                "[summary · 2026-05-28T13:00:00Z] Risk posture: risk-off.".into(),
                "[learning · 2026-05-21T13:00:00Z] Breadth divergences preceded the pullback."
                    .into(),
            ],
            ..Default::default()
        };
        let body = build_request(&input);
        let user = body["messages"][0]["content"].as_str().unwrap();
        assert!(user.contains("Recalled long-term memory"), "{user}");
        assert!(
            user.contains("Risk posture: risk-off.\n\n[learning"),
            "fragments join on blank lines: {user}"
        );

        // Without them: the block is omitted entirely, never rendered blank.
        let bare = build_user_prompt(&RouterInput {
            clusters: vec![cluster("t", 0.5)],
            ..Default::default()
        });
        assert!(!bare.contains("Recalled long-term memory"), "{bare}");
    }

    #[test]
    fn memory_alone_clears_the_empty_input_short_circuit() {
        // A memory-only input is a real routing input (it can steer investigation on
        // its own), so it must not read as empty and short-circuit to an empty plan.
        assert!(input_is_empty(&RouterInput::default()));
        assert!(!input_is_empty(&RouterInput {
            memory: vec!["[summary · t] Risk posture: mixed.".into()],
            ..Default::default()
        }));
    }

    #[test]
    fn build_request_carries_inbox_documents_and_omits_them_when_absent() {
        // With parsed inbox documents: the prompt carries the block, blocks joined on
        // blank lines like the memory fragments.
        let input = RouterInput {
            inbox_documents: vec![
                "### Research document: notes.md (MD)\n\nRates likely hold.".into(),
                "### Research document: deck.pdf (PDF)\n\nCapex steady.".into(),
            ],
            ..Default::default()
        };
        let body = build_request(&input);
        let user = body["messages"][0]["content"].as_str().unwrap();
        assert!(user.contains("User-supplied research documents"), "{user}");
        assert!(
            user.contains("Rates likely hold.\n\n### Research document: deck.pdf"),
            "blocks join on blank lines: {user}"
        );

        // Without them: the block is omitted entirely, never rendered blank.
        let bare = build_user_prompt(&RouterInput {
            clusters: vec![cluster("t", 0.5)],
            ..Default::default()
        });
        assert!(!bare.contains("User-supplied research documents"), "{bare}");

        // And inbox documents alone are a real routing input — no empty short-circuit.
        assert!(!input_is_empty(&input));
    }

    #[test]
    #[ignore = "hits live Tavily + GDELT + OpenAI + Anthropic; set TAVILY_API_KEY + OPENAI_API_KEY + ANTHROPIC_API_KEY"]
    fn research_routing_smoke() {
        use crate::headline_filter::{HeadlineFilter, ModelHeadlineFilter};
        use crate::news::{dedupe_headlines, CompositeNewsSource, NewsSource};

        let tavily = crate::tavily::TavilyNewsSource::from_env().expect("TAVILY_API_KEY set");
        let gdelt = crate::gdelt::GdeltNewsSource::new().expect("gdelt client");
        let raw = CompositeNewsSource::new(tavily, gdelt)
            .gather(crate::cadence::ReportCadence::from_elapsed(None))
            .expect("gather headlines");
        let deduped = dedupe_headlines(raw);
        assert!(!deduped.is_empty(), "expected headlines to filter");

        let clusters = ModelHeadlineFilter::from_env()
            .expect("OPENAI_API_KEY set")
            .filter(deduped)
            .expect("filter headlines");
        assert!(
            !clusters.is_empty(),
            "the filter produced at least one cluster"
        );
        let cluster_count = clusters.len();

        let plan = ModelResearchRouter::from_env()
            .expect("ANTHROPIC_API_KEY set")
            .route(RouterInput {
                clusters,
                ..Default::default()
            })
            .expect("route research");

        assert!(
            !plan.items.is_empty(),
            "routing produced at least one topic"
        );
        assert!(
            plan.items.len() <= MAX_RESEARCH_ITEMS,
            "respects the topic ceiling"
        );
        for it in &plan.items {
            assert!(!it.topic.trim().is_empty(), "every topic has a label");
            assert!(
                !it.rationale.trim().is_empty(),
                "every topic has a rationale"
            );
            assert!(!it.queries.is_empty(), "every topic has at least one query");
            assert!(
                it.queries.len() <= MAX_QUERIES_PER_ITEM,
                "queries within the per-item ceiling"
            );
            assert!((0.0..=1.0).contains(&it.priority), "priority in range");
        }
        eprintln!(
            "research routing: {} clusters -> {} topics ({} queries total)",
            cluster_count,
            plan.items.len(),
            plan.items.iter().map(|i| i.queries.len()).sum::<usize>()
        );
    }
}
