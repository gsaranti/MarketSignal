//! The agent stage contracts: pure structured-in / structured-out boundaries. The
//! application layer (`pipeline`) owns all I/O; agents never touch the network, the
//! database or the filesystem.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::baseline_delta::BaselineDeltas;
use crate::baseline_history::BaselineHistory;
use crate::cadence::ReportCadence;
use crate::data_sources::BaselineMarketData;
use crate::market_clock::MarketClock;
use crate::research_packet::ResearchPacket;

/// The market's risk stance (`docs/storage.md`).
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

/// Structured report-summary metadata (`docs/storage.md §Report Summary Metadata
/// Schema`); the required fields are the queryable keys for cross-report retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    pub report_id: String,
    pub report_type: String,
    pub created_at: String,
    /// A short per-issue headline (e.g. "Rotation, not rupture"), the report's label in
    /// the UI; an empty title falls back to the product name.
    #[serde(default)]
    pub title: String,
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
/// (`docs/report-workflow.md §Step 2`). The body may be head-truncated by the
/// application layer (a marker is appended when it is) and is empty when the Markdown
/// file could not be read, the summary still carrying. Never HTML: agents reason over
/// Markdown only (`docs/report-structure.md`).
#[derive(Debug, Clone)]
pub struct RecentReport {
    pub summary: ReportSummary,
    pub markdown: String,
}

/// Input handed to the main agent.
#[derive(Debug, Clone, Default)]
pub struct MainAgentInput {
    pub baseline: BaselineMarketData,
    /// `None` on the first report, or when no prior snapshot could be read or decoded;
    /// additive, never a gate.
    pub deltas: Option<BaselineDeltas>,
    /// `None` below three points (a first or second report) or when the snapshots could
    /// not be read; additive, never a gate.
    pub history: Option<BaselineHistory>,
    /// Computed from the prior snapshot's timestamp alone, deliberately not derived from
    /// `deltas`, which is `None` for both a first report and an undecodable prior: a
    /// corrupt prior snapshot would otherwise mislabel a run as a first one. `Default`
    /// is the first-report cadence.
    pub cadence: ReportCadence,
    /// The session state when the baseline was gathered, with the US/Eastern wall-clock
    /// time: tells the main agent whether to narrate the day's moves as live and
    /// provisional or as a completed session. `Default` carries no session context, and
    /// the prompt omits the block.
    pub market_clock: MarketClock,
    /// `None` on the offline/stub path; the research half is fail-soft, so a packet
    /// always exists on the live path even when a stage degraded to empty. `baseline`
    /// and `deltas` ride at the top level, not from the packet's own copies.
    pub research: Option<ResearchPacket>,
    /// The Step-4 pre-research pull (`docs/report-workflow.md §Step 4`), steering the
    /// retrospective audit (`§Step 5`). A sibling of `research`, not inside it: the
    /// replace-not-merge rule keeps the packet carrying only the Step-10 pull. It steers
    /// the audit but never gates it; `recent_reports` is the auditable object.
    pub audit_memory: Vec<String>,
    /// The Step-2 context (`docs/report-workflow.md §Step 2`), newest first: the
    /// auditable object and structural gate of the Retrospective Audit (`§Step 5`) — a
    /// non-empty list licenses the section, an empty one omits it. Best-effort; never
    /// gates the run.
    pub recent_reports: Vec<RecentReport>,
    /// The Steps 12–15 reviews the main agent weighs at synthesis (`§Step 16`); empty on
    /// the offline/stub path, where the synthesis prompt omits the block. Ephemeral,
    /// never persisted (`§Step 12`).
    pub analyst_reviews: Vec<AnalystOutput>,
}

/// No HTML: agents never see or emit HTML.
#[derive(Debug, Clone)]
pub struct MainAgentOutput {
    pub markdown: String,
    pub summary: ReportSummary,
    /// Durable learnings (`docs/report-workflow.md §Step 17`): a sibling of `summary`,
    /// never inside it, because the summary schema (`docs/storage.md`) is closed and
    /// each learning is its own atomic vector-memory unit. Usually empty; the app layer
    /// owns the per-report cap.
    pub durable_learnings: Vec<String>,
}

pub trait MainAgent {
    fn generate(&self, input: MainAgentInput) -> anyhow::Result<MainAgentOutput>;
}

/// Deterministic offline stand-in; `report_id` and `created_at` are freshly minted per
/// call so repeated runs produce distinct records.
#[derive(Debug, Default)]
pub struct StubMainAgent;

impl MainAgent for StubMainAgent {
    fn generate(&self, _input: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
        let summary = ReportSummary {
            report_id: Uuid::new_v4().to_string(),
            report_type: "market_signal".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            title: "Thin breadth, softening cut odds".to_string(),
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
            // No learnings, the normal case; exact-row-count tests over the memory store
            // rely on it.
            durable_learnings: Vec::new(),
        })
    }
}

/// A small but structurally valid body in the `docs/report-structure.md` section order,
/// with real headings, a list, a blockquote and a table for the frontend `.prose` styling.
const STUB_REPORT_MARKDOWN: &str = r#"# Market Signal Report

Date: current
Report Type: Market Signal Report

## Header Summary

- Equities held recent gains on thin breadth while rate-cut expectations softened.
- Long-end yields drifted higher; the curve's bear-steepening bears watching.
- AI capex remains the dominant earnings driver and the dominant concentration risk.

## Market Regime

Risk posture reads mixed and the cycle reads late. Leadership is narrow and
liquidity is adequate but no longer expanding, a combination that rewards
patience over conviction in either direction.

## Index Picture

- Dow: roughly flat.
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

With no prior Market Signal reports on record yet, there is nothing to audit;
subsequent reports will revisit whether these assumptions and risks
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

/// Which perspective an analyst argues from (`docs/agents.md §Analyst Agents`);
/// selects the adapter's system prompt and tags the review it returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Posture {
    Bull,
    Bear,
    Balanced,
}

impl Posture {
    /// The doc's order; the single source for constructing the analyst trio.
    pub const ALL: [Posture; 3] = [Self::Bull, Self::Bear, Self::Balanced];

    /// The serde rename and the per-review tag.
    pub fn as_str(&self) -> &'static str {
        match self {
            Posture::Bull => "bull",
            Posture::Bear => "bear",
            Posture::Balanced => "balanced",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Posture::Bull => "Bull Analyst",
            Posture::Bear => "Bear Analyst",
            Posture::Balanced => "Balanced Analyst",
        }
    }
}

/// How strongly an analyst holds its read (`docs/agents.md §Balanced Analyst`);
/// defaults to `Medium` so a model response that omits it still decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    #[default]
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }
}

/// One analyst's structured review (`docs/report-workflow.md §§12–15`), the contract
/// with the main agent's synthesis (`§Step 16`). Ephemeral, never persisted (`§Step 12`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalystOutput {
    pub posture: Posture,
    pub summary: String,
    pub key_points: Vec<String>,
    /// A bull still names what could break its case.
    pub risks: Vec<String>,
    pub opportunities: Vec<String>,
    pub confidence: Confidence,
}

/// The analyst stage (`docs/agents.md §Analyst Agents`). Sync and pure like
/// [`MainAgent`]: the real adapter's blocking HTTP call is offloaded via
/// `spawn_blocking` at the application-layer seam, where the three analysts run
/// concurrently (`docs/report-workflow.md §Step 12`).
pub trait AnalystAgent {
    /// `cadence` is passed explicitly, not read from the packet's change view, so a
    /// corrupt prior snapshot still yields the true interval (see
    /// [`MainAgentInput::cadence`]); `history` renders beside the packet rather than
    /// inside it.
    fn review(
        &self,
        packet: &ResearchPacket,
        cadence: ReportCadence,
        history: Option<&BaselineHistory>,
    ) -> anyhow::Result<AnalystOutput>;
}

/// Deterministic offline stand-in for one posture.
#[derive(Debug)]
pub struct StubAnalystAgent {
    posture: Posture,
}

impl StubAnalystAgent {
    pub fn new(posture: Posture) -> Self {
        Self { posture }
    }
}

impl AnalystAgent for StubAnalystAgent {
    fn review(
        &self,
        _packet: &ResearchPacket,
        _cadence: ReportCadence,
        _history: Option<&BaselineHistory>,
    ) -> anyhow::Result<AnalystOutput> {
        Ok(AnalystOutput {
            posture: self.posture,
            summary: format!(
                "{} read (offline stub): a fixed perspective for deterministic runs.",
                self.posture.display_name()
            ),
            key_points: vec![format!("{} key point", self.posture.as_str())],
            risks: vec![format!("{} risk", self.posture.as_str())],
            opportunities: vec![format!("{} opportunity", self.posture.as_str())],
            confidence: Confidence::Medium,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_populates_required_fields_and_serializes_kebab_labels() {
        let out = StubMainAgent.generate(MainAgentInput::default()).unwrap();
        let s = &out.summary;

        assert!(!s.report_id.is_empty());
        assert_eq!(s.report_type, "market_signal");
        assert!(!s.created_at.is_empty());
        assert!((3..=6).contains(&s.header_summary_bullets.len()));
        assert!(!out.markdown.is_empty());
        assert!(
            out.durable_learnings.is_empty(),
            "the stub emits no learnings"
        );

        let json = serde_json::to_value(s).unwrap();
        assert_eq!(json["risk_posture"], "mixed");
        assert_eq!(json["market_cycle"], "late-cycle");
        assert_eq!(json["thesis_stance"], "uncertain");
    }

    #[test]
    fn as_str_matches_serde_labels() {
        // The DB columns are written from `as_str()` while JSON uses the serde rename.
        for v in [
            RiskPosture::RiskOn,
            RiskPosture::RiskOff,
            RiskPosture::Mixed,
        ] {
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

    #[test]
    fn posture_and_confidence_serialize_lowercase() {
        for p in Posture::ALL {
            assert_eq!(serde_json::to_value(p).unwrap(), p.as_str());
        }
        for c in [Confidence::Low, Confidence::Medium, Confidence::High] {
            assert_eq!(serde_json::to_value(c).unwrap(), c.as_str());
        }
    }

    #[test]
    fn analyst_output_round_trips_through_serde() {
        let out = AnalystOutput {
            posture: Posture::Bear,
            summary: "Fragile breadth under a narrow rally.".into(),
            key_points: vec!["Leadership is concentrated".into()],
            risks: vec!["A reacceleration in core inflation".into()],
            opportunities: vec!["Quality at a discount if the tape breaks".into()],
            confidence: Confidence::High,
        };
        let json = serde_json::to_string(&out).unwrap();
        let back: AnalystOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(out, back);
        assert_eq!(serde_json::to_value(&out).unwrap()["posture"], "bear");
    }

    #[test]
    fn stub_analyst_tags_its_posture_and_default_input_has_no_reviews() {
        for p in Posture::ALL {
            let out = StubAnalystAgent::new(p)
                .review(
                    &crate::research_packet::ResearchPacket::default(),
                    ReportCadence::default(),
                    None,
                )
                .unwrap();
            assert_eq!(out.posture, p);
            assert!(!out.summary.is_empty());
        }
        assert!(MainAgentInput::default().analyst_reviews.is_empty());
    }

    #[test]
    fn confidence_defaults_to_medium_when_absent() {
        // The schema marks it required; the lenient decode keeps a malformed response
        // from losing the rest of the review.
        #[derive(Deserialize)]
        struct Partial {
            #[serde(default)]
            confidence: Confidence,
        }
        let p: Partial = serde_json::from_str("{}").unwrap();
        assert_eq!(p.confidence, Confidence::Medium);
    }
}
