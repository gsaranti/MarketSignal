//! The agent stage contract: a pure structured-in / structured-out boundary.
//!
//! The application layer (see `pipeline`) owns all I/O — agents never touch the
//! network, the database, or the filesystem. Each stage is a pure function from
//! a typed request to a typed, validated response. The real OpenAI/Anthropic
//! adapter will later replace `StubMainAgent` behind the same `MainAgent` trait.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::baseline_delta::BaselineDeltas;
use crate::data_sources::BaselineMarketData;
use crate::research_packet::ResearchPacket;

/// The market's risk stance (`docs/storage.md`). Serializes to the canonical
/// kebab labels (`risk-on`, `risk-off`, `mixed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskPosture {
    RiskOn,
    RiskOff,
    Mixed,
}

impl RiskPosture {
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskPosture::RiskOn => "risk-on",
            RiskPosture::RiskOff => "risk-off",
            RiskPosture::Mixed => "mixed",
        }
    }
}

/// The market's cycle stage (`docs/storage.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MarketCycle {
    LateCycle,
    Recessionary,
    Recovery,
}

impl MarketCycle {
    pub fn as_str(&self) -> &'static str {
        match self {
            MarketCycle::LateCycle => "late-cycle",
            MarketCycle::Recessionary => "recessionary",
            MarketCycle::Recovery => "recovery",
        }
    }
}

/// The report's overall thesis stance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThesisStance {
    Bullish,
    Bearish,
    Mixed,
    Uncertain,
}

impl ThesisStance {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThesisStance::Bullish => "bullish",
            ThesisStance::Bearish => "bearish",
            ThesisStance::Mixed => "mixed",
            ThesisStance::Uncertain => "uncertain",
        }
    }
}

/// Structured report-summary metadata the main agent populates when it writes a
/// report (`docs/storage.md §Report Summary Metadata Schema`). The required
/// fields are the queryable keys used for cross-report retrieval; the optional
/// arrays default to empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    pub report_id: String,
    pub report_type: String,
    pub created_at: String,
    pub risk_posture: RiskPosture,
    pub market_cycle: MarketCycle,
    pub thesis_stance: ThesisStance,
    pub header_summary_bullets: Vec<String>,
    #[serde(default)]
    pub key_risks: Vec<String>,
    #[serde(default)]
    pub unresolved_questions: Vec<String>,
    #[serde(default)]
    pub forward_outlook_themes: Vec<String>,
}

/// One recent prior report handed to the main agent as Step-2 context
/// (`docs/weekly-report-workflow.md §Step 2`): the structured summary metadata
/// paired with the report's canonical Markdown body. The body may be head-truncated
/// by the application layer to bound prompt tokens (a truncation marker is appended
/// when it is); it is empty when the Markdown file could not be read, in which case
/// the summary still carries. HTML never appears here — agents reason over Markdown
/// only (`docs/report-structure.md`).
#[derive(Debug, Clone)]
pub struct RecentReport {
    pub summary: ReportSummary,
    pub markdown: String,
}

/// Input handed to the main agent. Carries the Step-3 baseline market-data scan
/// (`docs/weekly-report-workflow.md §Step 3`) gathered by the application layer
/// before agent reasoning, its change view, and the Step-11 condensed research
/// packet; vector memory joins it when the Step-10 retrieval slice lands.
#[derive(Debug, Clone, Default)]
pub struct MainAgentInput {
    pub baseline: BaselineMarketData,
    /// Deterministic change view of `baseline` against the previous report's persisted
    /// snapshot (`baseline_delta`), computed by the application layer. `None` on the
    /// first report, or when no prior snapshot could be read or decoded — the deltas are
    /// additive and never gate a run.
    pub deltas: Option<BaselineDeltas>,
    /// The Step-11 condensed research packet (`research_packet`): the filtered news
    /// clusters and bounded deep-research evidence the application layer assembled from
    /// the research half. `None` on the offline/stub path (no research stages run) — the
    /// research half is fully fail-soft, so a packet always exists on the live path even
    /// when a stage degraded to empty. `baseline` and `deltas` ride at the top level here
    /// rather than being read from the packet's own (inert) copies of them.
    pub research: Option<ResearchPacket>,
    /// The Step-4 pre-research vector-memory pull (`docs/weekly-report-workflow.md
    /// §Step 4`): memory recalled against the recent report context and current
    /// measured market state, to steer the **retrospective audit** (`§Step 5`). It is
    /// the audit's consumer of the same ephemeral pull that also feeds research
    /// routing — a top-level sibling of `baseline`/`deltas`, deliberately *not* inside
    /// `research`: the doc's replace-not-merge rule keeps the packet carrying only the
    /// Step-10 research-informed pull (`§Step 10`), so the two memory pulls reach the
    /// main agent on separate channels for separate purposes. Empty when nothing was
    /// recalled (an early run or a retrieval failure). It *steers* the audit — what to
    /// scrutinise — but no longer gates it; `recent_reports` is the auditable object.
    pub audit_memory: Vec<String>,
    /// The Step-2 recent prior-report context (`docs/weekly-report-workflow.md §Step 2`):
    /// the bounded set of most-recent reports — structured metadata plus (possibly
    /// truncated) Markdown body — that the main agent reasons over for thesis continuity
    /// and that the **Retrospective Audit** section (`§Step 5`) evaluates. This is the
    /// audit's auditable object and its structural gate: a non-empty list licenses the
    /// section, an empty one (a first run or a DB/file failure) omits it. Best-effort and
    /// additive — never gates the run. Newest first.
    pub recent_reports: Vec<RecentReport>,
}

/// What the main agent returns: the canonical Markdown body plus the structured
/// summary. No HTML — agents never see or emit HTML.
#[derive(Debug, Clone)]
pub struct MainAgentOutput {
    pub markdown: String,
    pub summary: ReportSummary,
    /// Durable learnings the agent identified this run (`docs/weekly-report-workflow.md
    /// §Step 17`): rare, self-contained analytical lessons worth carrying across future
    /// reports. Deliberately a sibling of `summary`, never inside it — the summary
    /// metadata schema (`docs/storage.md`) is closed, and each learning is its own
    /// atomic vector-memory unit, embedded and persisted separately from the summary.
    /// Usually empty; the app layer owns the per-report cap.
    pub durable_learnings: Vec<String>,
}

/// The agent stage. One method: structured input -> structured output.
pub trait MainAgent {
    fn generate(&self, input: MainAgentInput) -> anyhow::Result<MainAgentOutput>;
}

/// Deterministic offline stand-in for the real model adapter. Body and labels
/// are fixed; `report_id` and `created_at` are freshly minted per call so
/// repeated runs produce distinct records.
#[derive(Debug, Default)]
pub struct StubMainAgent;

impl MainAgent for StubMainAgent {
    fn generate(&self, _input: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
        let summary = ReportSummary {
            report_id: Uuid::new_v4().to_string(),
            report_type: "weekly_market".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            risk_posture: RiskPosture::Mixed,
            market_cycle: MarketCycle::LateCycle,
            thesis_stance: ThesisStance::Uncertain,
            header_summary_bullets: vec![
                "Equities held recent gains on thin breadth while rate-cut expectations softened."
                    .to_string(),
                "Long-end yields drifted higher; the curve's bear-steepening bears watching."
                    .to_string(),
                "AI capex remains the dominant earnings driver and the dominant concentration risk."
                    .to_string(),
            ],
            key_risks: vec![
                "A reacceleration in core inflation that forces the Fed to hold longer than priced."
                    .to_string(),
            ],
            unresolved_questions: vec![
                "Is narrow leadership a late-cycle warning or a durable productivity regime?"
                    .to_string(),
            ],
            forward_outlook_themes: vec![
                "Liquidity and breadth".to_string(),
                "Rate-path repricing".to_string(),
            ],
        };

        Ok(MainAgentOutput {
            markdown: STUB_REPORT_MARKDOWN.to_string(),
            summary,
            // The stub models a run where nothing clears the durable-learning bar —
            // the normal case, and what keeps exact-row-count tests over the memory
            // store meaningful.
            durable_learnings: Vec::new(),
        })
    }
}

/// A small but structurally valid report body following the section order in
/// `docs/report-structure.md`, written so the frontend `.prose` styling has
/// real headings, a list, a blockquote, and a table to render.
const STUB_REPORT_MARKDOWN: &str = r#"# Weekly Market Report

Date: this week
Report Type: Weekly Market Report

## Header Summary

- Equities held recent gains on thin breadth while rate-cut expectations softened.
- Long-end yields drifted higher; the curve's bear-steepening bears watching.
- AI capex remains the dominant earnings driver and the dominant concentration risk.

## Market Regime

Risk posture reads mixed and the cycle reads late. Leadership is narrow and
liquidity is adequate but no longer expanding, a combination that rewards
patience over conviction in either direction.

## Index Picture

- Dow: roughly flat on the week.
- S&P 500: modest gains, leadership concentrated.
- Nasdaq: outperformed on a handful of megacaps.

## Key Market Drivers

- Inflation / Federal Reserve: the market is repricing the path, not the destination.
- AI / Semiconductors: capex intentions remain the swing factor for forward earnings.
- Liquidity / Credit: spreads are calm; watch for any change in tone.

> We hold the mixed read until breadth either confirms or breaks the rally; a
> sustained deterioration in market breadth would revise us toward risk-off.

## Market Signal Thesis

The weight of evidence supports neither a clean bullish nor a clean bearish
stance. We favor balance, with explicit triggers that would move the thesis.

## Retrospective Audit

With no prior Market Signal reports on record yet, there is nothing to audit
this week; subsequent reports will revisit whether these assumptions and risks
played out as anticipated.

## Investment Strategy

Positioning favors balance over conviction: quality, cash-generative leaders
warrant attention while crowding in the narrow leadership warrants caution.
This frames where risk and reward look asymmetric — it is not buy/sell guidance.

## Forward Outlook

- Liquidity and breadth as the tell for whether the rally broadens or narrows.
- Rate-path repricing around the next inflation and jobs prints.

## Watchlist

| Signal | What we are watching |
| --- | --- |
| Market breadth | Confirmation or divergence versus the index |
| Long-end yields | A sustained move that reprices the Fed path |

## Sources

- Stubbed report — no external sources in this slice.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_populates_required_fields_and_serializes_kebab_labels() {
        let out = StubMainAgent.generate(MainAgentInput::default()).unwrap();
        let s = &out.summary;

        assert!(!s.report_id.is_empty());
        assert_eq!(s.report_type, "weekly_market");
        assert!(!s.created_at.is_empty());
        assert!((3..=6).contains(&s.header_summary_bullets.len()));
        assert!(!out.markdown.is_empty());
        assert!(out.durable_learnings.is_empty(), "the stub emits no learnings");

        let json = serde_json::to_value(s).unwrap();
        assert_eq!(json["risk_posture"], "mixed");
        assert_eq!(json["market_cycle"], "late-cycle");
        assert_eq!(json["thesis_stance"], "uncertain");
    }

    #[test]
    fn as_str_matches_serde_labels() {
        // The DB columns are written from `as_str()` while JSON uses serde's
        // kebab rename; this pins the two label sources together.
        for v in [RiskPosture::RiskOn, RiskPosture::RiskOff, RiskPosture::Mixed] {
            assert_eq!(serde_json::to_value(v).unwrap(), v.as_str());
        }
        for v in [
            MarketCycle::LateCycle,
            MarketCycle::Recessionary,
            MarketCycle::Recovery,
        ] {
            assert_eq!(serde_json::to_value(v).unwrap(), v.as_str());
        }
        for v in [
            ThesisStance::Bullish,
            ThesisStance::Bearish,
            ThesisStance::Mixed,
            ThesisStance::Uncertain,
        ] {
            assert_eq!(serde_json::to_value(v).unwrap(), v.as_str());
        }
    }
}
