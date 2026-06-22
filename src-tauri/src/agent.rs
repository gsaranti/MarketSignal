//! The agent stage contract: a pure structured-in / structured-out boundary.
//!
//! The application layer (see `pipeline`) owns all I/O — agents never touch the
//! network, the database, or the filesystem. Each stage is a pure function from
//! a typed request to a typed, validated response. The real OpenAI/Anthropic
//! adapter will later replace `StubMainAgent` behind the same `MainAgent` trait.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::baseline_delta::BaselineDeltas;
use crate::cadence::ReportCadence;
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
    /// A short, specific per-issue headline the main agent writes (e.g. "Rotation,
    /// not rupture") — surfaced as the report's label in the UI in place of the
    /// generic product name. `#[serde(default)]` so summaries persisted before this
    /// field still decode: an older row carries an empty title and the UI falls back
    /// to the product name.
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
/// (`docs/report-workflow.md §Step 2`): the structured summary metadata
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
/// (`docs/report-workflow.md §Step 3`) gathered by the application layer
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
    /// The run's report cadence (`cadence::ReportCadence`): the elapsed interval since
    /// the previous report, classified. Computed by the application layer from the prior
    /// snapshot's timestamp alone — deliberately *not* derived from `deltas`, which is
    /// `None` for both a first report and an undecodable prior, so an existing user with
    /// a corrupt prior snapshot would otherwise be mislabeled a first run. Drives the
    /// main agent's posture steer (a short interval is a tactical update; a long one a
    /// structural refresh), and it is meaningful even when `deltas` is absent. `Default`
    /// is the first-report cadence.
    pub cadence: ReportCadence,
    /// The Step-11 condensed research packet (`research_packet`): the filtered news
    /// clusters and bounded deep-research evidence the application layer assembled from
    /// the research half. `None` on the offline/stub path (no research stages run) — the
    /// research half is fully fail-soft, so a packet always exists on the live path even
    /// when a stage degraded to empty. `baseline` and `deltas` ride at the top level here
    /// rather than being read from the packet's own (inert) copies of them.
    pub research: Option<ResearchPacket>,
    /// The Step-4 pre-research vector-memory pull (`docs/report-workflow.md
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
    /// The Step-2 recent prior-report context (`docs/report-workflow.md §Step 2`):
    /// the bounded set of most-recent reports — structured metadata plus (possibly
    /// truncated) Markdown body — that the main agent reasons over for thesis continuity
    /// and that the **Retrospective Audit** section (`§Step 5`) evaluates. This is the
    /// audit's auditable object and its structural gate: a non-empty list licenses the
    /// section, an empty one (a first run or a DB/file failure) omits it. Best-effort and
    /// additive — never gates the run. Newest first.
    pub recent_reports: Vec<RecentReport>,
    /// The Steps 12–15 analyst reviews (`docs/report-workflow.md §§12–15`):
    /// the Bull, Bear, and Balanced reads of the same condensed research packet,
    /// which the main agent critiques and weighs during synthesis (`§Step 16`,
    /// `docs/agents.md §Synthesis Behavior`). Populated on the live path (the three
    /// analyst stages run before the main agent); empty on the offline/stub path and
    /// the early slices, in which case the synthesis prompt simply omits the block.
    /// Ephemeral — never persisted (`§Step 12`).
    pub analyst_reviews: Vec<AnalystOutput>,
}

/// What the main agent returns: the canonical Markdown body plus the structured
/// summary. No HTML — agents never see or emit HTML.
#[derive(Debug, Clone)]
pub struct MainAgentOutput {
    pub markdown: String,
    pub summary: ReportSummary,
    /// Durable learnings the agent identified this run (`docs/report-workflow.md
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

/// Which analytical perspective an analyst agent argues from (`docs/agents.md
/// §Analyst Agents`). Each of the three fixed analyst stages
/// (`docs/report-workflow.md §§12–15`) is one posture; the posture selects the adapter's system
/// prompt and tags the review it returns so the main agent's synthesis can attribute
/// each read. Serializes to the lowercase label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Posture {
    Bull,
    Bear,
    Balanced,
}

impl Posture {
    /// The three postures in the doc's order — the single source for constructing the
    /// analyst trio so callers never hard-code the set.
    pub const ALL: [Posture; 3] = [Self::Bull, Self::Bear, Self::Balanced];

    /// Canonical lowercase label (the serde rename and the per-review tag).
    pub fn as_str(&self) -> &'static str {
        match self {
            Posture::Bull => "bull",
            Posture::Bear => "bear",
            Posture::Balanced => "balanced",
        }
    }

    /// Human-readable analyst name, for prompts and the synthesis block.
    pub fn display_name(&self) -> &'static str {
        match self {
            Posture::Bull => "Bull Analyst",
            Posture::Bear => "Bear Analyst",
            Posture::Balanced => "Balanced Analyst",
        }
    }
}

/// How strongly an analyst holds its read (`docs/agents.md §Balanced Analyst` names
/// assigning confidence levels as part of weighing evidence). Serializes lowercase;
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

/// One analyst agent's structured review of the condensed research packet
/// (`docs/report-workflow.md §§12–15`). This is the contract between the
/// analyst stage and the main agent's synthesis (`§Step 16`): each analyst argues
/// from its `posture` and returns its read as fields the main agent critiques and
/// weighs. Ephemeral — never persisted (`§Step 12`); it rides into the main agent on
/// [`MainAgentInput::analyst_reviews`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalystOutput {
    pub posture: Posture,
    /// A short prose read of the market from this perspective.
    pub summary: String,
    /// The strongest specific points the perspective rests on.
    pub key_points: Vec<String>,
    /// Risks this perspective surfaces (a bull still names what could break its case).
    pub risks: Vec<String>,
    /// Opportunities or constructive developments this perspective surfaces.
    pub opportunities: Vec<String>,
    /// How strongly the analyst holds this read.
    pub confidence: Confidence,
}

/// The analyst stage (`docs/agents.md §Analyst Agents`). One method: read the shared
/// condensed research packet and return a structured review from the adapter's
/// assigned posture. Sync and pure like [`MainAgent`] — the blocking model HTTP call
/// inside the real adapter is offloaded via `spawn_blocking` at the application-layer
/// seam, where the three analysts run concurrently (`docs/report-workflow.md
/// §Step 12`).
pub trait AnalystAgent {
    /// `cadence` is the run's report cadence — how long since the previous report — so
    /// the analyst can weight recent moves versus the structural picture. Passed
    /// explicitly (not read from the packet's change view) so a corrupt prior snapshot
    /// still yields the true interval (see [`MainAgentInput::cadence`]).
    fn review(
        &self,
        packet: &ResearchPacket,
        cadence: ReportCadence,
    ) -> anyhow::Result<AnalystOutput>;
}

/// Deterministic offline stand-in for a real analyst adapter, tagged with the posture
/// it argues from. Returns a fixed structured review so the pipeline and its tests run
/// end to end without live keys.
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
        // The DB columns are written from `as_str()` while JSON uses serde's
        // kebab rename; this pins the two label sources together.
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
        // The posture tag serializes to its lowercase label.
        assert_eq!(serde_json::to_value(&out).unwrap()["posture"], "bear");
    }

    #[test]
    fn stub_analyst_tags_its_posture_and_default_input_has_no_reviews() {
        // Each stub is one posture and returns a review tagged with it.
        for p in Posture::ALL {
            let out = StubAnalystAgent::new(p)
                .review(
                    &crate::research_packet::ResearchPacket::default(),
                    ReportCadence::default(),
                )
                .unwrap();
            assert_eq!(out.posture, p);
            assert!(!out.summary.is_empty());
        }
        // The main agent input defaults to no analyst reviews (the offline/stub path),
        // so the synthesis prompt omits the block until the live stage populates it.
        assert!(MainAgentInput::default().analyst_reviews.is_empty());
    }

    #[test]
    fn confidence_defaults_to_medium_when_absent() {
        // The review schema marks confidence required, but a lenient decode keeps a
        // malformed response from losing the rest of the review.
        #[derive(Deserialize)]
        struct Partial {
            #[serde(default)]
            confidence: Confidence,
        }
        let p: Partial = serde_json::from_str("{}").unwrap();
        assert_eq!(p.confidence, Confidence::Medium);
    }
}
