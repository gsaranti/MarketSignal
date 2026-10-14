//! Step 11: the condensed research packet (`docs/report-workflow.md §Step 11`), the
//! canonical input the analyst agents and the main agent reason over. Built as a
//! deterministic app-layer assembler rather than by the main agent as the doc assigns:
//! the upstream funnel has already done the condensation, so assembly is plumbing.

use serde::Serialize;

use crate::baseline_delta::BaselineDeltas;
use crate::data_sources::BaselineMarketData;
use crate::headline_filter::HeadlineCluster;
use crate::research_executor::ResearchEvidence;

/// Defensive ceiling on news clusters, ranked by `relevance`. The Step-7 filter already
/// caps its output, so this bounds the packet's token footprint, not the funnel.
pub const MAX_PACKET_CLUSTERS: usize = 8;

/// Defensive ceiling on sources per finding: the executor's request budget bounds the
/// number of findings, this bounds how wide any one grows.
pub const MAX_SOURCES_PER_FINDING: usize = 5;

/// The condensed research packet (`docs/report-workflow.md §Step 11`). `Serialize` but
/// not `Deserialize`: [`BaselineDeltas`] is `Serialize`-only, and the packet is an
/// in-memory artifact, never persisted.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ResearchPacket {
    /// Passed through unchanged (already bounded).
    pub baseline: BaselineMarketData,
    /// `None` on a first report or an unreadable prior snapshot.
    pub deltas: Option<BaselineDeltas>,
    /// Most market-significant first, capped at [`MAX_PACKET_CLUSTERS`].
    pub news_clusters: Vec<HeadlineCluster>,
    /// Topics highest-priority first, each finding's sources capped at
    /// [`MAX_SOURCES_PER_FINDING`]; the request/stop accounting rides through untouched
    /// so a truncated phase stays visible.
    pub research: ResearchEvidence,
    /// The Step-10 research-informed pull (`docs/report-workflow.md §Step 10`), most
    /// relevant first, in `MemoryHit::prompt_fragment` form. Deliberately not the Step-4
    /// pre-research pull: replace, not merge.
    pub memory: Vec<String>,
    /// One prompt block per successfully parsed inbox document, newest first, bounded
    /// upstream by `document_parser`'s char budgets, so this section carries no cap of
    /// its own.
    pub inbox_summaries: Vec<String>,
}

/// Deterministic condensation: ordered selection (clusters by `relevance`, topics by
/// `priority`) plus the per-section caps above.
pub fn build_condensed_packet(
    baseline: BaselineMarketData,
    deltas: Option<BaselineDeltas>,
    clusters: Vec<HeadlineCluster>,
    evidence: ResearchEvidence,
    memory: Vec<String>,
    inbox_summaries: Vec<String>,
) -> ResearchPacket {
    let mut news_clusters = clusters;
    news_clusters.sort_by(|a, b| b.relevance.total_cmp(&a.relevance));
    news_clusters.truncate(MAX_PACKET_CLUSTERS);

    let mut research = evidence;
    research
        .items
        .sort_by(|a, b| b.priority.total_cmp(&a.priority));
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

    use crate::data_sources::{Change, Quote};
    use crate::news::RawHeadline;
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
        assert!(packet
            .news_clusters
            .windows(2)
            .all(|w| w[0].relevance >= w[1].relevance));
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

        let topics: Vec<&str> = packet
            .research
            .items
            .iter()
            .map(|i| i.topic.as_str())
            .collect();
        assert_eq!(topics, vec!["high", "mid", "low"], "highest priority first");
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
        assert_eq!(
            packet.research.stopped_reason,
            Some(StopReason::RequestBudget)
        );
    }

    #[test]
    fn baseline_and_present_deltas_pass_through_untouched() {
        // Non-default values, so "untouched" can't be satisfied by a fabricated default.
        let baseline = BaselineMarketData {
            indices: vec![Quote {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                price: 5500.0,
                change: Change::percent(0.8),
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

        assert_eq!(
            packet.baseline, baseline,
            "baseline carried through unchanged"
        );
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
        // Already ranked by the store's search, so the assembler neither reorders nor trims.
        let memory = vec![
            "[summary · 2026-06-04T13:00:00Z] Risk posture: risk-off.".to_string(),
            "[learning · 2026-05-21T13:00:00Z] Breadth divergences preceded the pullback."
                .to_string(),
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
        // Already ordered and bounded by `document_parser`, so the assembler neither
        // reorders nor trims.
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
