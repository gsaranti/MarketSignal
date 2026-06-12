//! Step 11: the condensed research packet (`docs/weekly-report-workflow.md §Step 11`).
//!
//! The canonical input the analyst agents (and, at synthesis, the main agent) reason
//! over: the application layer gathers the curated evidence from the research half and
//! condenses it into one bounded artifact. The doc assigns packet-building to the main
//! agent; this slice builds it as a **deterministic app-layer assembler** instead — by
//! Step 11 the upstream funnel (~500 headlines → ~10 stories → ~5 routed topics →
//! bounded executor evidence) has already done the heavy condensation, so assembling
//! the already-pruned parts is plumbing rather than reasoning. A conscious extension of
//! the doc, noted here so the deviation stays legible.
//!
//! Wired into `generate_report` since the research-half wiring slice:
//! `pipeline::assemble_research_packet` builds this packet every run and hands it to
//! the main agent on `MainAgentInput.research`. The not-yet-built analyst agents join
//! as its other consumers when their slice lands.
//!
//! Scoped, like [`RouterInput`](crate::research_router::RouterInput), to the input types
//! that exist today: the Step-3 baseline scan and its change view, the Step-7 clusters,
//! the Step-9 evidence, the Step-10 research-informed vector-memory pull (`memory`,
//! populated by the retrieval slice), and the Step-6 research-inbox summaries
//! (`inbox_summaries`, the inbox-parsing slice). The remaining doc-listed contents —
//! recent report context (a bounded form of which now feeds routing via
//! `RouterInput.recent_reports`, but is not yet carried in this packet), unresolved
//! thesis questions, and upcoming events — join this struct as their slices land.

use serde::Serialize;

use crate::baseline_delta::BaselineDeltas;
use crate::data_sources::BaselineMarketData;
use crate::headline_filter::HeadlineCluster;
use crate::research_executor::ResearchEvidence;

/// Defensive ceiling on news clusters carried into the packet, ranked by `relevance`.
/// The Step-7 filter already caps its output (~10 important stories), so this is a
/// belt-and-braces bound on the packet's token footprint, not the primary funnel.
pub const MAX_PACKET_CLUSTERS: usize = 8;

/// Defensive ceiling on sources kept per research finding. The executor's 50-request
/// budget bounds the *number* of findings; this bounds how wide any one finding's
/// source list can grow before it crowds the packet.
pub const MAX_SOURCES_PER_FINDING: usize = 5;

/// The condensed research packet (`docs/weekly-report-workflow.md §Step 11`): the
/// canonical, token-bounded input for the analyst agents.
///
/// `Serialize` (but not `Deserialize`) for parity with [`ResearchEvidence`] and for
/// debug/progress surfacing — [`BaselineDeltas`] is `Serialize`-only, so the packet
/// inherits that asymmetry. It is an in-memory pipeline artifact, never persisted, so
/// the one-way derive is sufficient.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ResearchPacket {
    /// The Step-3 baseline market-data scan, passed through unchanged (already bounded).
    pub baseline: BaselineMarketData,
    /// The deterministic change view against the previous report's snapshot, when one
    /// existed. `None` on a first report or an unreadable prior snapshot.
    pub deltas: Option<BaselineDeltas>,
    /// Filtered news clusters, most market-significant first, capped at
    /// [`MAX_PACKET_CLUSTERS`].
    pub news_clusters: Vec<HeadlineCluster>,
    /// The Step-9 research evidence, topics highest-priority first, each finding's
    /// source list capped at [`MAX_SOURCES_PER_FINDING`]. The request/stop accounting
    /// is carried through untouched so a truncated phase stays visible.
    pub research: ResearchEvidence,
    /// The Step-10 research-informed vector-memory pull (`docs/weekly-report-workflow.md
    /// §Step 10`): recalled fragments, most relevant first, each in the
    /// `MemoryHit::prompt_fragment` form. Deliberately *not* the Step-4 pre-research
    /// pull — the packet carries only the research-informed result set (replace, not
    /// merge). Empty on an early run's bare store or when the fail-soft pull degraded.
    pub memory: Vec<String>,
    /// The Step-6 research-inbox documents (`docs/weekly-report-workflow.md §Step 6`,
    /// "research inbox summaries" in §Step 11): one prompt block per successfully
    /// parsed user-supplied document, in the inbox's newest-first order. The doc's
    /// "summaries" ship as deterministic condensed excerpts — bounded upstream by
    /// `document_parser`'s per-doc and total char budgets, so this section carries
    /// no cap of its own. Empty when the inbox was empty or every file failed.
    pub inbox_summaries: Vec<String>,
}

/// Assemble the condensed packet from the gathered evidence. Deterministic condensation:
/// ordered selection (clusters by `relevance`, evidence topics by `priority`) plus the
/// defensive per-section caps above. Consumes its inputs — the caller hands over the
/// gathered artifacts and gets the bounded packet back.
pub fn build_condensed_packet(
    baseline: BaselineMarketData,
    deltas: Option<BaselineDeltas>,
    clusters: Vec<HeadlineCluster>,
    evidence: ResearchEvidence,
    memory: Vec<String>,
    inbox_summaries: Vec<String>,
) -> ResearchPacket {
    // News clusters: most market-significant first, then capped.
    let mut news_clusters = clusters;
    news_clusters.sort_by(|a, b| b.relevance.total_cmp(&a.relevance));
    news_clusters.truncate(MAX_PACKET_CLUSTERS);

    // Research evidence: highest-priority topics first, each finding's source list capped
    // so a noisy topic can't blow the packet's token budget. `requests_made` and
    // `stopped_reason` ride through unchanged.
    let mut research = evidence;
    research.items.sort_by(|a, b| b.priority.total_cmp(&a.priority));
    for item in &mut research.items {
        for finding in &mut item.findings {
            finding.sources.truncate(MAX_SOURCES_PER_FINDING);
        }
    }

    ResearchPacket {
        baseline,
        deltas,
        news_clusters,
        research,
        memory,
        inbox_summaries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::data_sources::Quote;
    use crate::news::RawHeadline;
    // `BaselineDeltas` and `ResearchEvidence` are already in scope via `super::*`.
    use crate::research_executor::{EvidenceItem, Finding, StopReason};

    fn headline(i: usize) -> RawHeadline {
        RawHeadline {
            title: format!("headline {i}"),
            url: format!("https://example.com/{i}"),
            source: "example.com".into(),
            published: None,
            snippet: None,
        }
    }

    fn cluster(topic: &str, relevance: f64) -> HeadlineCluster {
        HeadlineCluster {
            topic: topic.into(),
            summary: format!("summary of {topic}"),
            relevance,
            headlines: vec![headline(0)],
        }
    }

    /// One evidence item with a single depth-1 finding carrying `n_sources` sources.
    fn evidence_item(topic: &str, priority: f64, n_sources: usize) -> EvidenceItem {
        EvidenceItem {
            topic: topic.into(),
            rationale: format!("why {topic}"),
            priority,
            findings: vec![Finding {
                query: format!("{topic} q"),
                depth: 1,
                sources: (0..n_sources).map(headline).collect(),
            }],
        }
    }

    #[test]
    fn clusters_are_ordered_by_relevance_and_capped() {
        // More clusters than the cap, deliberately out of relevance order.
        let clusters = (0..MAX_PACKET_CLUSTERS + 3)
            .map(|i| cluster(&format!("topic{i}"), i as f64 / 100.0))
            .collect();
        let packet = build_condensed_packet(
            BaselineMarketData::default(),
            None,
            clusters,
            ResearchEvidence::default(),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(packet.news_clusters.len(), MAX_PACKET_CLUSTERS, "capped");
        // Descending relevance: each cluster is at least as relevant as the next.
        assert!(packet
            .news_clusters
            .windows(2)
            .all(|w| w[0].relevance >= w[1].relevance));
        // The single most relevant cluster survived the cap and leads.
        assert_eq!(
            packet.news_clusters[0].topic,
            format!("topic{}", MAX_PACKET_CLUSTERS + 2)
        );
    }

    #[test]
    fn evidence_items_ordered_by_priority_and_sources_capped() {
        let evidence = ResearchEvidence {
            items: vec![
                evidence_item("low", 0.2, MAX_SOURCES_PER_FINDING + 4),
                evidence_item("high", 0.9, MAX_SOURCES_PER_FINDING + 4),
                evidence_item("mid", 0.5, 1),
            ],
            requests_made: 3,
            stopped_reason: None,
        };
        let packet = build_condensed_packet(
            BaselineMarketData::default(),
            None,
            Vec::new(),
            evidence,
            Vec::new(),
            Vec::new(),
        );

        let topics: Vec<&str> = packet.research.items.iter().map(|i| i.topic.as_str()).collect();
        assert_eq!(topics, vec!["high", "mid", "low"], "highest priority first");
        // Every finding's source list is capped; the short one is left untouched.
        assert!(packet
            .research
            .items
            .iter()
            .flat_map(|i| &i.findings)
            .all(|f| f.sources.len() <= MAX_SOURCES_PER_FINDING));
    }

    #[test]
    fn request_accounting_and_stop_reason_pass_through() {
        let evidence = ResearchEvidence {
            items: vec![evidence_item("oil", 0.9, 1)],
            requests_made: 42,
            stopped_reason: Some(StopReason::RequestBudget),
        };
        let packet = build_condensed_packet(
            BaselineMarketData::default(),
            None,
            Vec::new(),
            evidence,
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(packet.research.requests_made, 42);
        assert_eq!(packet.research.stopped_reason, Some(StopReason::RequestBudget));
    }

    #[test]
    fn baseline_and_present_deltas_pass_through_untouched() {
        // The assembler reorders/caps only the news and evidence sections; baseline and a
        // populated change view must arrive on the packet exactly as handed in. A distinct
        // `elapsed_days` and a non-default index quote make "untouched" meaningful — neither
        // could match a fabricated default.
        let baseline = BaselineMarketData {
            indices: vec![Quote {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                price: 5500.0,
                change_pct: 0.8,
                unit: "index".into(),
            }],
            ..Default::default()
        };
        let deltas = BaselineDeltas {
            elapsed_days: 5.5,
            changed: Vec::new(),
            new: Vec::new(),
            missing: Vec::new(),
        };
        let packet = build_condensed_packet(
            baseline.clone(),
            Some(deltas.clone()),
            Vec::new(),
            ResearchEvidence::default(),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(packet.baseline, baseline, "baseline carried through unchanged");
        assert_eq!(
            packet.deltas,
            Some(deltas),
            "populated change view carried through unchanged"
        );
    }

    #[test]
    fn empty_inputs_yield_an_empty_but_valid_packet() {
        let packet = build_condensed_packet(
            BaselineMarketData::default(),
            None,
            Vec::new(),
            ResearchEvidence::default(),
            Vec::new(),
            Vec::new(),
        );

        assert!(packet.news_clusters.is_empty());
        assert!(packet.research.items.is_empty());
        assert!(packet.deltas.is_none());
        assert!(packet.memory.is_empty());
    }

    #[test]
    fn memory_pull_is_carried_through_untouched() {
        // The Step-10 research-informed pull rides the packet exactly as handed in —
        // already ranked most-relevant-first by the store's search, so the assembler
        // neither reorders nor trims it.
        let memory = vec![
            "[summary · 2026-06-04T13:00:00Z] Risk posture: risk-off.".to_string(),
            "[learning · 2026-05-21T13:00:00Z] Breadth divergences preceded the pullback.".to_string(),
        ];
        let packet = build_condensed_packet(
            BaselineMarketData::default(),
            None,
            vec![cluster("AI / semiconductors", 0.95)],
            ResearchEvidence {
                items: vec![evidence_item("AI capex", 0.9, 2)],
                requests_made: 1,
                stopped_reason: None,
            },
            memory.clone(),
            Vec::new(),
        );
        assert_eq!(packet.memory, memory);
    }

    #[test]
    fn inbox_summaries_are_carried_through_untouched() {
        // The Step-6 inbox blocks ride the packet exactly as handed in — already
        // ordered (newest first) and bounded by `document_parser`'s char budgets,
        // so the assembler neither reorders nor trims them.
        let inbox = vec![
            "### Research document: notes.md (MD)\n\nRates likely hold.".to_string(),
            "### Research document: deck.pdf (PDF)\n\nCapex steady.".to_string(),
        ];
        let packet = build_condensed_packet(
            BaselineMarketData::default(),
            None,
            Vec::new(),
            ResearchEvidence::default(),
            Vec::new(),
            inbox.clone(),
        );
        assert_eq!(packet.inbox_summaries, inbox);
    }
}
