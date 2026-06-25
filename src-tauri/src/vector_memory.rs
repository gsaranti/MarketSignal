//! Long-term semantic memory for the main agent (`docs/storage.md §Vector
//! Memory`), shipped on SQLite rather than LanceDB — a conscious
//! deviation. At this corpus scale (≤30 retained report summaries plus durable
//! learnings) an unindexed LanceDB would run the same exact brute-force scan
//! this module runs; storing the embeddings as BLOBs in the app's existing
//! database buys identical behavior without the arrow/lance/DataFusion
//! dependency tree, the `protoc` build requirement, or an async bridge into the
//! deliberately synchronous pipeline. The seams — these functions plus the
//! `embedding::Embedder` trait — keep a later engine swap contained to this
//! module if the corpus ever outgrows brute force (thousands of vectors).
//!
//! House style: functions over `&rusqlite::Connection`, like `storage`. The
//! table is created by `storage::init_schema`. Two kinds share one table:
//! a report's `summary` row cascades with its report (joined by `report_id` —
//! `delete_report_summary` is the hook the future retention-cascade slice
//! calls), while `learning` rows survive report deletion by design.

use anyhow::Result;
use rusqlite::Connection;

use crate::agent::ReportSummary;
use crate::baseline_delta::{BaselineDeltas, SeriesDelta, SeriesTransition};
use crate::data_sources::{BaselineMarketData, ChangeKind};
use crate::research_executor::ResearchEvidence;

/// What a memory row is (`docs/storage.md §Vector Memory`): a report
/// summary (one per report, cascades with it) or a durable learning (survives
/// report deletion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    Summary,
    Learning,
}

impl MemoryKind {
    /// The canonical label persisted in the `vector_memory.kind` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryKind::Summary => "summary",
            MemoryKind::Learning => "learning",
        }
    }

    /// Parse a stored `kind` label back, or `None` for a label this build does
    /// not know (forward-compat: an unknown kind is skipped, not an error).
    fn parse(label: &str) -> Option<Self> {
        match label {
            "summary" => Some(MemoryKind::Summary),
            "learning" => Some(MemoryKind::Learning),
            _ => None,
        }
    }
}

/// Which job's continuity partition a memory row belongs to (`docs/storage.md
/// §Local Vector Memory`): the Market Signal Report, Portfolio Analysis, or Trade
/// Opportunities. A partition dimension *orthogonal* to [`MemoryKind`] (summary /
/// learning) — every retrieval is scoped to the calling job's namespace, so no job
/// ever reads another's learnings (holding-grading calibration is not
/// opportunity-discovery context). The report keeps the historical `report`
/// namespace, which is what `storage::init_schema` backfills existing rows to when
/// the column is added.
///
/// Isolation here is by partition, not dimensionality: the two local jobs share an
/// embedder (and so a vector space), while the report embeds with a different model
/// — so the namespace is what separates the local pair from each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryNamespace {
    Report,
    Portfolio,
    Opportunities,
}

impl MemoryNamespace {
    /// The canonical label persisted in the `vector_memory.namespace` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryNamespace::Report => "report",
            MemoryNamespace::Portfolio => "portfolio",
            MemoryNamespace::Opportunities => "opportunities",
        }
    }
}

/// One retrieval result: the stored content plus its cosine similarity to the
/// query (higher is closer; 1.0 is identical direction).
#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub kind: MemoryKind,
    pub report_id: Option<String>,
    pub content: String,
    pub created_at: String,
    pub score: f64,
}

impl MemoryHit {
    /// The prompt form shared by the router input and the condensed packet: the
    /// kind and date tag the fragment's provenance so the model can weigh recall
    /// age; the cosine score stays internal (noise to a model). The content keeps
    /// its own newlines — fragments are blocks, not bullets.
    pub fn prompt_fragment(&self) -> String {
        format!(
            "[{} · {}] {}",
            self.kind.as_str(),
            self.created_at,
            self.content
        )
    }
}

/// Insert one memory row. The embedding is stored as little-endian `f32` bytes;
/// the store is dimension-agnostic — `search_memory` skips rows whose dimension
/// does not match the query, so a model change degrades old rows to unmatched
/// rather than corrupting a search. A non-finite component (NaN/±inf) is a typed
/// error: it would poison cosine scores downstream, and the one legitimate
/// writer (a model embedding) never produces one — so it marks a bad response,
/// not data to keep. A `summary` row without a `report_id` is likewise a typed
/// error: a summary is definitionally a report's summary, and both the
/// one-per-report unique index and `delete_report_summary`'s cascade key on the
/// id — an orphan row would be invisible to both.
pub fn insert_memory(
    conn: &Connection,
    kind: MemoryKind,
    namespace: MemoryNamespace,
    report_id: Option<&str>,
    content: &str,
    embedding: &[f32],
    created_at: &str,
) -> Result<()> {
    if embedding.iter().any(|v| !v.is_finite()) {
        anyhow::bail!("refusing to store an embedding with non-finite components");
    }
    if kind == MemoryKind::Summary && report_id.is_none() {
        anyhow::bail!("refusing to store a summary row without a report_id");
    }
    conn.execute(
        "INSERT INTO vector_memory (kind, namespace, report_id, content, embedding, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            kind.as_str(),
            namespace.as_str(),
            report_id,
            content,
            embedding_to_blob(embedding),
            created_at
        ],
    )?;
    Ok(())
}

/// Exact top-k retrieval: brute-force cosine over every stored row in the given
/// `namespace` (optionally one kind), descending by similarity. The namespace
/// scope is what enforces per-job isolation — a Portfolio retrieval never sees the
/// report's or Trade Opportunities' rows (`docs/storage.md §Local Vector Memory`).
/// Brute force is the deliberate choice at this scale — see the module header.
/// Rows that cannot participate — an unknown kind label, an undecodable blob, a
/// dimension mismatch with the query — are skipped with a stderr note rather than
/// failing the search, so one bad row never blanks retrieval.
pub fn search_memory(
    conn: &Connection,
    query: &[f32],
    kind: Option<MemoryKind>,
    namespace: MemoryNamespace,
    top_k: usize,
) -> Result<Vec<MemoryHit>> {
    let mut stmt = conn.prepare(
        "SELECT kind, report_id, content, embedding, created_at FROM vector_memory
         WHERE namespace = ?1 AND (?2 IS NULL OR kind = ?2)",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![namespace.as_str(), kind.map(|k| k.as_str())],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, String>(4)?,
            ))
        },
    )?;

    let mut hits = Vec::new();
    for row in rows {
        let (kind_label, report_id, content, blob, created_at) = row?;
        let Some(kind) = MemoryKind::parse(&kind_label) else {
            eprintln!("vector-memory: skipping row with unknown kind {kind_label:?}");
            continue;
        };
        let Some(embedding) = blob_to_embedding(&blob) else {
            eprintln!("vector-memory: skipping row with an undecodable embedding blob");
            continue;
        };
        if embedding.len() != query.len() {
            eprintln!(
                "vector-memory: skipping row with dimension {} (query has {})",
                embedding.len(),
                query.len()
            );
            continue;
        }
        // Belt-and-braces against blobs that decode to NaN/±inf (insert refuses
        // them, but a corrupted blob still decodes): a non-finite score would
        // sort unpredictably and could displace real hits from the top-k.
        let score = cosine_similarity(query, &embedding);
        if !score.is_finite() {
            eprintln!("vector-memory: skipping row with a non-finite similarity score");
            continue;
        }
        hits.push(MemoryHit {
            kind,
            report_id,
            content,
            created_at,
            score,
        });
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(top_k);
    Ok(hits)
}

/// Delete one report's `summary` row — the vector-memory leg of the 30-report
/// retention cascade (`docs/storage.md`: a deleted report's vector summary
/// reference goes with it, durable learnings do not). The `kind` filter is what
/// guarantees the asymmetry: a `learning` row is never touched, whatever its
/// `report_id`. Returns how many rows were deleted (0 or 1 in practice).
pub fn delete_report_summary(conn: &Connection, report_id: &str) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM vector_memory
         WHERE kind = 'summary' AND namespace = 'report' AND report_id = ?1",
        [report_id],
    )?;
    Ok(deleted)
}

/// Total rows in `namespace` — the cheap guard the retrieval pulls use to skip a
/// paid embedding call when there is nothing to search (an empty partition on
/// early runs). Scoped per namespace so a populated partition (e.g. the report's)
/// never makes another job's empty partition look non-empty.
pub fn count_memory(conn: &Connection, namespace: MemoryNamespace) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM vector_memory WHERE namespace = ?1",
        [namespace.as_str()],
        |r| r.get(0),
    )?)
}

/// The cosine similarity of the closest existing `learning` row to a candidate
/// embedding, or `None` when no learning row can participate — an empty learning
/// corpus, or only rows the search must skip (undecodable blob, dimension
/// mismatch with the candidate). The Step-17 persist write uses this to drop a
/// near-restatement of a learning the store already holds before spending a row
/// on it: learnings are never deleted, so unbounded restatement is permanent
/// growth that dilutes retrieval. Thin by design — it reuses the same
/// brute-force cosine scan as retrieval, restricted to `learning` rows and the
/// single nearest, so the threshold that decides "duplicate" stays an app-layer
/// policy at the call site rather than being baked in here. The `summary` kind
/// is excluded: dedup is within the learning corpus, never against a report's
/// summary.
pub fn nearest_learning_similarity(
    conn: &Connection,
    namespace: MemoryNamespace,
    embedding: &[f32],
) -> Result<Option<f64>> {
    let hits = search_memory(conn, embedding, Some(MemoryKind::Learning), namespace, 1)?;
    Ok(hits.first().map(|h| h.score))
}

/// The deterministic text rendering of a report summary that gets embedded —
/// the atomic unit `docs/storage.md §Embeddings` names ("the report-summary
/// metadata is the unit that enters vector memory", never the report Markdown).
/// Stances first, then each non-empty list section; empty optional sections are
/// omitted rather than rendered as empty headers.
pub fn summary_memory_text(summary: &ReportSummary) -> String {
    let mut out = format!(
        "Risk posture: {}. Market cycle: {}. Thesis stance: {}.\n",
        summary.risk_posture.as_str(),
        summary.market_cycle.as_str(),
        summary.thesis_stance.as_str()
    );
    push_section(&mut out, "Header summary", &summary.header_summary_bullets);
    push_section(&mut out, "Key risks", &summary.key_risks);
    push_section(
        &mut out,
        "Unresolved questions",
        &summary.unresolved_questions,
    );
    push_section(
        &mut out,
        "Forward outlook themes",
        &summary.forward_outlook_themes,
    );
    out
}

fn push_section(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push('\n');
    out.push_str(title);
    out.push_str(":\n");
    for item in items {
        out.push_str("- ");
        out.push_str(item);
        out.push('\n');
    }
}

/// Cap on change-view lines in the pre-research query: the strongest moves carry
/// the recall signal; a full diff would drown them.
const QUERY_MAX_DELTA_LINES: usize = 12;

/// Cap on `new` / `missing` transition names in the pre-research query.
const QUERY_MAX_TRANSITIONS: usize = 6;

/// Cap on source titles rendered per finding in the post-research query.
const QUERY_MAX_SOURCE_TITLES: usize = 3;

/// The deterministic text the Step-4 pre-research pull embeds
/// (`docs/report-workflow.md §Step 4`): memory is recalled against where
/// the market actually is this period — the recent report context, the salient
/// baseline levels (indices + internals, never the full baseline JSON), and the
/// strongest moves in the change view. Pure and bounded: each section is omitted
/// when its input is empty, and all-empty inputs render an empty string, which
/// the pull reads as nothing to recall against.
pub fn pre_research_query(
    recent: &[ReportSummary],
    baseline: &BaselineMarketData,
    deltas: Option<&BaselineDeltas>,
) -> String {
    let mut out = String::new();

    for summary in recent {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&summary_memory_text(summary));
    }

    let levels: Vec<String> = baseline
        .indices
        .iter()
        .chain(&baseline.internals)
        .map(|q| {
            // The change is a percent for most quotes, but a *point delta* for rate-valued
            // series — suffixing a point delta with `%` would contradict its unit. The kind
            // travels on the quote (`change.kind`), so read it there rather than re-deriving
            // it from the series id; a percent / annualized change keeps its `%`.
            match q.change.kind {
                ChangeKind::PointDelta => {
                    format!(
                        "- {}: {} {} ({:+.2})",
                        q.name, q.price, q.unit, q.change.value
                    )
                }
                _ => format!(
                    "- {}: {} {} ({:+.2}%)",
                    q.name, q.price, q.unit, q.change.value
                ),
            }
        })
        .collect();
    if !levels.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("Current market picture:\n");
        out.push_str(&levels.join("\n"));
        out.push('\n');
    }

    if let Some(d) = deltas {
        // Strongest relative moves first; a series without an honest percentage
        // (prior level zero/non-finite) ranks last rather than fabricating one.
        let pct_key = |s: &SeriesDelta| s.pct_change.map_or(0.0, f64::abs);
        let mut moves: Vec<&SeriesDelta> = d.changed.iter().collect();
        moves.sort_by(|a, b| pct_key(b).total_cmp(&pct_key(a)));
        let lines: Vec<String> = moves
            .iter()
            .take(QUERY_MAX_DELTA_LINES)
            .map(|s| match s.pct_change {
                Some(p) => format!("- {}: {} -> {} ({:+.2}%)", s.name, s.prior, s.current, p),
                None => format!("- {}: {} -> {}", s.name, s.prior, s.current),
            })
            .collect();
        if !lines.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!(
                "Change since the previous report (~{:.1} days):\n",
                d.elapsed_days
            ));
            out.push_str(&lines.join("\n"));
            out.push('\n');
        }
        push_transition_line(&mut out, "Newly tracked series", &d.new);
        push_transition_line(&mut out, "Series missing this run", &d.missing);
    }

    out
}

fn push_transition_line(out: &mut String, label: &str, transitions: &[SeriesTransition]) {
    if transitions.is_empty() {
        return;
    }
    let names: Vec<&str> = transitions
        .iter()
        .take(QUERY_MAX_TRANSITIONS)
        .map(|t| t.name.as_str())
        .collect();
    out.push_str(&format!("{label}: {}\n", names.join(", ")));
}

/// The deterministic text the Step-10 post-research pull embeds
/// (`docs/report-workflow.md §Step 10`): memory is recalled against what
/// the research actually found — each routed topic with its rationale, its
/// concrete queries, and a bounded slice of the source titles the executor
/// surfaced. Pure; empty evidence renders an empty string.
pub fn post_research_query(evidence: &ResearchEvidence) -> String {
    let mut out = String::new();
    for item in &evidence.items {
        out.push_str(&format!(
            "Research topic: {} — {}\n",
            item.topic, item.rationale
        ));
        for finding in &item.findings {
            out.push_str(&format!("- {}", finding.query));
            let titles: Vec<&str> = finding
                .sources
                .iter()
                .take(QUERY_MAX_SOURCE_TITLES)
                .map(|s| s.title.as_str())
                .collect();
            if !titles.is_empty() {
                out.push_str(&format!(": {}", titles.join("; ")));
            }
            out.push('\n');
        }
    }
    out
}

/// Cosine similarity over equal-length vectors, computed in `f64` for stable
/// accumulation. A zero-magnitude vector has no direction, so its similarity is
/// defined as 0.0 rather than NaN — keeping the sort in `search_memory` total.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    debug_assert_eq!(a.len(), b.len(), "cosine over mismatched dimensions");
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (f64::from(*x), f64::from(*y));
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Encode an embedding as the stored little-endian `f32` byte blob.
pub fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(embedding.len() * 4);
    for v in embedding {
        blob.extend_from_slice(&v.to_le_bytes());
    }
    blob
}

/// Decode a stored blob back into an embedding, or `None` when the byte length
/// is not a whole number of `f32`s (a truncated or foreign blob).
pub fn blob_to_embedding(blob: &[u8]) -> Option<Vec<f32>> {
    if !blob.len().is_multiple_of(4) {
        return None;
    }
    Some(
        blob.chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{MarketCycle, RiskPosture, ThesisStance};

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::storage::init_schema(&conn).unwrap();
        conn
    }

    fn sample_summary() -> ReportSummary {
        ReportSummary {
            report_id: "rep-1".into(),
            report_type: "weekly_market".into(),
            created_at: "2026-06-11T00:00:00Z".into(),
            title: "Test thesis headline".into(),
            risk_posture: RiskPosture::Mixed,
            market_cycle: MarketCycle::LateCycle,
            thesis_stance: ThesisStance::Uncertain,
            header_summary_bullets: vec![
                "Breadth stayed thin.".into(),
                "Yields drifted up.".into(),
            ],
            key_risks: vec!["Sticky core inflation.".into()],
            unresolved_questions: vec![],
            forward_outlook_themes: vec![],
        }
    }

    #[test]
    fn blob_round_trips_and_rejects_truncation() {
        let v = vec![0.5f32, -1.25, 3.0, f32::MIN_POSITIVE];
        assert_eq!(blob_to_embedding(&embedding_to_blob(&v)).unwrap(), v);
        assert!(
            blob_to_embedding(&[1, 2, 3]).is_none(),
            "13 % 4 != 0 must not decode"
        );
    }

    #[test]
    fn cosine_handles_identity_orthogonality_and_zero() {
        assert!((cosine_similarity(&[1.0, 2.0], &[1.0, 2.0]) - 1.0).abs() < 1e-12);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-12);
        // A zero vector has no direction: similarity is defined 0, never NaN.
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn search_orders_by_similarity_and_caps_at_top_k() {
        let conn = mem();
        // Against query [1,0]: a≈1.0, b≈0.6, c≈0.0.
        insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("a"), "a", &[1.0, 0.0], "t").unwrap();
        insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("b"), "b", &[0.6, 0.8], "t").unwrap();
        insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("c"), "c", &[0.0, 1.0], "t").unwrap();

        let hits = search_memory(&conn, &[1.0, 0.0], None, MemoryNamespace::Report, 2).unwrap();
        assert_eq!(hits.len(), 2, "capped at top_k");
        assert_eq!(hits[0].content, "a");
        assert_eq!(hits[1].content, "b");
        assert!(hits[0].score > hits[1].score);
        assert!((hits[0].score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn search_filters_by_kind_or_spans_both() {
        let conn = mem();
        insert_memory(
            &conn,
            MemoryKind::Summary,
            MemoryNamespace::Report,
            Some("r"),
            "the summary",
            &[1.0, 0.0],
            "t",
        )
        .unwrap();
        insert_memory(
            &conn,
            MemoryKind::Learning,
            MemoryNamespace::Report,
            None,
            "the learning",
            &[1.0, 0.0],
            "t",
        )
        .unwrap();

        let learnings = search_memory(&conn, &[1.0, 0.0], Some(MemoryKind::Learning), MemoryNamespace::Report, 10).unwrap();
        assert_eq!(learnings.len(), 1);
        assert_eq!(learnings[0].content, "the learning");
        assert_eq!(learnings[0].kind, MemoryKind::Learning);
        assert!(learnings[0].report_id.is_none());

        let both = search_memory(&conn, &[1.0, 0.0], None, MemoryNamespace::Report, 10).unwrap();
        assert_eq!(both.len(), 2, "no kind filter spans both kinds");
    }

    #[test]
    fn insert_rejects_non_finite_embeddings() {
        let conn = mem();
        for bad in [
            vec![1.0, f32::NAN],
            vec![f32::INFINITY],
            vec![f32::NEG_INFINITY, 0.5],
        ] {
            let err =
                insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("r"), "c", &bad, "t").unwrap_err();
            assert!(err.to_string().contains("non-finite"), "{err}");
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vector_memory", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "no rejected embedding landed");
    }

    #[test]
    fn search_skips_rows_with_non_finite_scores() {
        let conn = mem();
        insert_memory(
            &conn,
            MemoryKind::Summary,
            MemoryNamespace::Report,
            Some("ok"),
            "finite",
            &[1.0, 0.0],
            "t",
        )
        .unwrap();
        // Insert refuses non-finite components, so plant the corrupt row the way it
        // would really arrive: bytes in the blob that decode to NaN.
        conn.execute(
            "INSERT INTO vector_memory (kind, report_id, content, embedding, created_at)
             VALUES ('summary', 'bad', 'corrupt', ?1, 't')",
            rusqlite::params![embedding_to_blob(&[f32::NAN, 1.0])],
        )
        .unwrap();

        let hits = search_memory(&conn, &[1.0, 0.0], None, MemoryNamespace::Report, 10).unwrap();
        assert_eq!(hits.len(), 1, "the NaN-scoring row is skipped, not ranked");
        assert_eq!(hits[0].content, "finite");
    }

    #[test]
    fn duplicate_summary_for_a_report_is_rejected_but_learnings_are_not() {
        let conn = mem();
        insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("rep-1"), "s", &[1.0], "t").unwrap();
        // Second summary for the same report violates the partial unique index.
        assert!(
            insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("rep-1"), "s2", &[1.0], "t").is_err(),
            "one embedding per report summary is schema-enforced"
        );
        // Learnings are outside the partial index: same report_id, and several of them.
        insert_memory(
            &conn,
            MemoryKind::Learning,
            MemoryNamespace::Report,
            Some("rep-1"),
            "l1",
            &[1.0],
            "t",
        )
        .unwrap();
        insert_memory(
            &conn,
            MemoryKind::Learning,
            MemoryNamespace::Report,
            Some("rep-1"),
            "l2",
            &[1.0],
            "t",
        )
        .unwrap();
        // A different report's summary is fine.
        insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("rep-2"), "s", &[1.0], "t").unwrap();
    }

    #[test]
    fn summary_without_a_report_id_is_rejected() {
        let conn = mem();
        // SQLite unique indexes treat NULLs as distinct, so the one-per-report index
        // can't catch NULL-id summaries — the API guard is what closes that hole.
        let err =
            insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, None, "orphan", &[1.0], "t").unwrap_err();
        assert!(err.to_string().contains("without a report_id"), "{err}");
        // Learnings legitimately carry no report_id.
        insert_memory(&conn, MemoryKind::Learning, MemoryNamespace::Report, None, "learning", &[1.0], "t").unwrap();
    }

    #[test]
    fn search_skips_dimension_mismatched_rows() {
        let conn = mem();
        insert_memory(
            &conn,
            MemoryKind::Summary,
            MemoryNamespace::Report,
            Some("ok"),
            "fits",
            &[1.0, 0.0],
            "t",
        )
        .unwrap();
        insert_memory(
            &conn,
            MemoryKind::Summary,
            MemoryNamespace::Report,
            Some("old"),
            "stale dims",
            &[1.0, 0.0, 0.0],
            "t",
        )
        .unwrap();
        let hits = search_memory(&conn, &[1.0, 0.0], None, MemoryNamespace::Report, 10).unwrap();
        assert_eq!(hits.len(), 1, "the 3-dim row is skipped, not an error");
        assert_eq!(hits[0].content, "fits");
    }

    #[test]
    fn delete_report_summary_preserves_learnings_and_other_reports() {
        let conn = mem();
        insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("rep-1"), "s1", &[1.0], "t").unwrap();
        insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("rep-2"), "s2", &[1.0], "t").unwrap();
        // A learning tagged with the same report id must still survive the cascade —
        // the kind filter, not the report_id, is what protects durable learnings.
        insert_memory(
            &conn,
            MemoryKind::Learning,
            MemoryNamespace::Report,
            Some("rep-1"),
            "l1",
            &[1.0],
            "t",
        )
        .unwrap();

        assert_eq!(delete_report_summary(&conn, "rep-1").unwrap(), 1);

        let remaining = search_memory(&conn, &[1.0], None, MemoryNamespace::Report, 10).unwrap();
        let contents: Vec<&str> = remaining.iter().map(|h| h.content.as_str()).collect();
        assert_eq!(remaining.len(), 2);
        assert!(contents.contains(&"s2"), "other reports' summaries survive");
        assert!(
            contents.contains(&"l1"),
            "durable learnings survive report deletion"
        );
    }

    #[test]
    fn summary_memory_text_renders_stances_and_omits_empty_sections() {
        let text = summary_memory_text(&sample_summary());
        assert!(text.contains("Risk posture: mixed."), "{text}");
        assert!(text.contains("Market cycle: late-cycle."), "{text}");
        assert!(text.contains("Thesis stance: uncertain."), "{text}");
        assert!(
            text.contains("Header summary:\n- Breadth stayed thin."),
            "{text}"
        );
        assert!(
            text.contains("Key risks:\n- Sticky core inflation."),
            "{text}"
        );
        // Empty optional sections are omitted entirely, not rendered as bare headers.
        assert!(!text.contains("Unresolved questions"), "{text}");
        assert!(!text.contains("Forward outlook themes"), "{text}");
        // Deterministic: the same summary always embeds the same text.
        assert_eq!(text, summary_memory_text(&sample_summary()));
    }

    #[test]
    fn count_memory_reflects_inserts() {
        let conn = mem();
        assert_eq!(count_memory(&conn, MemoryNamespace::Report).unwrap(), 0);
        insert_memory(&conn, MemoryKind::Learning, MemoryNamespace::Report, None, "l", &[1.0], "t").unwrap();
        assert_eq!(count_memory(&conn, MemoryNamespace::Report).unwrap(), 1);
    }

    #[test]
    fn namespace_scopes_search_count_and_dedup_so_jobs_stay_isolated() {
        let conn = mem();
        // The same vector lands in two partitions; neither job may see the other's row
        // (`docs/storage.md §Local Vector Memory` — isolation is by partition, since the
        // two local jobs share an embedder and so a vector space).
        insert_memory(
            &conn,
            MemoryKind::Learning,
            MemoryNamespace::Report,
            None,
            "report learning",
            &[1.0, 0.0],
            "t",
        )
        .unwrap();
        insert_memory(
            &conn,
            MemoryKind::Learning,
            MemoryNamespace::Portfolio,
            None,
            "portfolio learning",
            &[1.0, 0.0],
            "t",
        )
        .unwrap();

        // count is per-namespace: a populated report partition never makes the
        // Opportunities partition look non-empty.
        assert_eq!(count_memory(&conn, MemoryNamespace::Report).unwrap(), 1);
        assert_eq!(count_memory(&conn, MemoryNamespace::Portfolio).unwrap(), 1);
        assert_eq!(count_memory(&conn, MemoryNamespace::Opportunities).unwrap(), 0);

        // search is scoped: a Portfolio query sees only the Portfolio row, even though
        // the report row is an identical-direction match.
        let hits = search_memory(&conn, &[1.0, 0.0], None, MemoryNamespace::Portfolio, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].content, "portfolio learning");

        // dedup is scoped too: the report row is not a near-duplicate for the
        // Opportunities corpus (which is empty), so it returns None.
        assert!(nearest_learning_similarity(&conn, MemoryNamespace::Opportunities, &[1.0, 0.0])
            .unwrap()
            .is_none());
    }

    #[test]
    fn nearest_learning_similarity_scores_the_closest_learning() {
        let conn = mem();
        // Nothing to be near yet: an empty learning corpus returns None, never 0.0
        // (the call site must be able to tell "no neighbor" from "an orthogonal one").
        assert!(nearest_learning_similarity(&conn, MemoryNamespace::Report, &[1.0, 0.0])
            .unwrap()
            .is_none());

        insert_memory(&conn, MemoryKind::Learning, MemoryNamespace::Report, None, "l", &[1.0, 0.0], "t").unwrap();
        // An identical direction scores ~1.0 (a duplicate); an orthogonal one ~0.0.
        let same = nearest_learning_similarity(&conn, MemoryNamespace::Report, &[1.0, 0.0])
            .unwrap()
            .unwrap();
        assert!(
            (same - 1.0).abs() < 1e-9,
            "identical embedding scores ~1.0: {same}"
        );
        let orthogonal = nearest_learning_similarity(&conn, MemoryNamespace::Report, &[0.0, 1.0])
            .unwrap()
            .unwrap();
        assert!(
            orthogonal.abs() < 1e-9,
            "orthogonal embedding scores ~0.0: {orthogonal}"
        );

        // The *closest* learning wins: a second row aligned with a new query
        // direction is returned over the now-orthogonal first row (top-1, not first).
        insert_memory(&conn, MemoryKind::Learning, MemoryNamespace::Report, None, "l2", &[0.0, 1.0], "t").unwrap();
        let best = nearest_learning_similarity(&conn, MemoryNamespace::Report, &[0.0, 1.0])
            .unwrap()
            .unwrap();
        assert!(
            (best - 1.0).abs() < 1e-9,
            "the closer of the two learnings wins: {best}"
        );
    }

    #[test]
    fn nearest_learning_similarity_ignores_summary_rows() {
        let conn = mem();
        // A summary identical to the query must not register as a near-duplicate
        // learning — dedup is within the learning corpus only.
        insert_memory(&conn, MemoryKind::Summary, MemoryNamespace::Report, Some("r"), "s", &[1.0, 0.0], "t").unwrap();
        assert!(
            nearest_learning_similarity(&conn, MemoryNamespace::Report, &[1.0, 0.0])
                .unwrap()
                .is_none(),
            "a summary is not a learning"
        );
    }

    #[test]
    fn prompt_fragment_tags_kind_and_date_and_drops_the_score() {
        let hit = MemoryHit {
            kind: MemoryKind::Summary,
            report_id: Some("rep-1".into()),
            content: "Risk posture: mixed.".into(),
            created_at: "2026-06-04T13:00:00Z".into(),
            score: 0.87,
        };
        let frag = hit.prompt_fragment();
        assert_eq!(
            frag,
            "[summary · 2026-06-04T13:00:00Z] Risk posture: mixed."
        );
        assert!(!frag.contains("0.87"), "the cosine score stays internal");
    }

    // ---- retrieval query builders (Steps 4 / 10) ----

    use crate::baseline_delta::Direction;
    use crate::data_sources::{Change, Quote};
    use crate::news::RawHeadline;
    use crate::research_executor::{EvidenceItem, Finding};

    fn quote(name: &str, price: f64, change_pct: f64) -> Quote {
        Quote {
            symbol: name.into(),
            name: name.into(),
            price,
            change: Change::percent(change_pct),
            unit: "index points".into(),
        }
    }

    fn delta(name: &str, prior: f64, current: f64, pct: Option<f64>) -> SeriesDelta {
        SeriesDelta {
            group: crate::data_sources::GroupKind::Indices,
            id: name.into(),
            name: name.into(),
            current,
            prior,
            abs_change: current - prior,
            pct_change: pct,
            direction: if current >= prior {
                Direction::Up
            } else {
                Direction::Down
            },
        }
    }

    #[test]
    fn pre_research_query_renders_each_section_and_is_deterministic() {
        let baseline = BaselineMarketData {
            indices: vec![quote("S&P 500", 5610.0, 0.4)],
            internals: vec![quote("CBOE Volatility Index", 14.0, -2.1)],
            ..Default::default()
        };
        let deltas = BaselineDeltas {
            elapsed_days: 7.0,
            changed: vec![delta("S&P 500", 5500.0, 5610.0, Some(2.0))],
            new: vec![SeriesTransition {
                group: crate::data_sources::GroupKind::Internals,
                id: "DGS2".into(),
                name: "2-Year Treasury Yield".into(),
                reason: None,
            }],
            missing: Vec::new(),
        };
        let q = pre_research_query(&[sample_summary()], &baseline, Some(&deltas));

        // Recent context, levels, change view, and transitions all render.
        assert!(q.contains("Thesis stance: uncertain."), "{q}");
        assert!(
            q.contains("Current market picture:\n- S&P 500: 5610 index points (+0.40%)"),
            "{q}"
        );
        assert!(
            q.contains("Change since the previous report (~7.0 days):"),
            "{q}"
        );
        assert!(q.contains("- S&P 500: 5500 -> 5610 (+2.00%)"), "{q}");
        assert!(
            q.contains("Newly tracked series: 2-Year Treasury Yield"),
            "{q}"
        );
        assert!(
            !q.contains("Series missing this run"),
            "no missing series, no line"
        );
        // Deterministic: same inputs, same query text.
        assert_eq!(
            q,
            pre_research_query(&[sample_summary()], &baseline, Some(&deltas))
        );
    }

    #[test]
    fn pre_research_query_omits_percent_suffix_for_a_point_delta_quote() {
        // A rate-valued FRED internal (DGS10) carries a point delta in `change` (kind
        // PointDelta), not a percent. The current-market-picture line must NOT suffix that
        // figure with `%` — a `(+0.10%)` would contradict the change's unit. A genuine-percent
        // quote still keeps its `%`. The name is untagged now; the kind carries the unit.
        let baseline = BaselineMarketData {
            indices: vec![Quote {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                price: 5610.0,
                change: Change::percent(0.4),
                unit: "index points".into(),
            }],
            internals: vec![Quote {
                symbol: "DGS10".into(),
                name: "10-Year Treasury Yield".into(),
                price: 4.3,
                change: Change::point_delta(0.10),
                unit: "percent".into(),
            }],
            ..Default::default()
        };
        let q = pre_research_query(&[], &baseline, None);

        // The point-delta rate renders its move with no `%`.
        assert!(
            q.contains("- 10-Year Treasury Yield: 4.3 percent (+0.10)"),
            "point-delta quote should render the move without a unit suffix: {q}"
        );
        assert!(
            !q.contains("(+0.10%)"),
            "a point delta must not be suffixed with a misleading %: {q}"
        );
        // A genuine-percent quote keeps its `%`.
        assert!(
            q.contains("- S&P 500: 5610 index points (+0.40%)"),
            "a percent quote should keep its %: {q}"
        );
    }

    #[test]
    fn pre_research_query_on_empty_inputs_is_empty() {
        assert_eq!(
            pre_research_query(&[], &BaselineMarketData::default(), None),
            ""
        );
    }

    #[test]
    fn pre_research_query_keeps_the_strongest_moves_within_the_cap() {
        // More changed series than the cap; the weakest relative moves drop out and
        // a no-percentage series ranks last rather than fabricating a percentage.
        let changed: Vec<SeriesDelta> = (0..QUERY_MAX_DELTA_LINES + 3)
            .map(|i| {
                delta(
                    &format!("series-{i}"),
                    100.0,
                    100.0 + i as f64,
                    Some(i as f64),
                )
            })
            .collect();
        let deltas = BaselineDeltas {
            elapsed_days: 7.0,
            changed,
            new: Vec::new(),
            missing: Vec::new(),
        };
        let q = pre_research_query(&[], &BaselineMarketData::default(), Some(&deltas));
        let lines = q.lines().filter(|l| l.starts_with("- series-")).count();
        assert_eq!(
            lines, QUERY_MAX_DELTA_LINES,
            "capped at the strongest moves"
        );
        // The strongest move survived; the weakest did not.
        assert!(
            q.contains(&format!("series-{}", QUERY_MAX_DELTA_LINES + 2)),
            "{q}"
        );
        assert!(!q.contains("- series-0:"), "{q}");
    }

    #[test]
    fn post_research_query_renders_topics_queries_and_bounded_titles() {
        let headline = |t: &str| RawHeadline {
            title: t.into(),
            url: "https://example.com".into(),
            source: "example.com".into(),
            published: None,
            snippet: None,
        };
        let evidence = ResearchEvidence {
            items: vec![EvidenceItem {
                topic: "AI capex".into(),
                rationale: "Semis led the move.".into(),
                priority: 0.9,
                findings: vec![Finding {
                    query: "hyperscaler capex guidance".into(),
                    depth: 1,
                    sources: vec![
                        headline("t1"),
                        headline("t2"),
                        headline("t3"),
                        headline("t4"),
                    ],
                }],
            }],
            requests_made: 1,
            stopped_reason: None,
        };
        let q = post_research_query(&evidence);
        assert!(
            q.contains("Research topic: AI capex — Semis led the move."),
            "{q}"
        );
        assert!(
            q.contains("- hyperscaler capex guidance: t1; t2; t3"),
            "{q}"
        );
        assert!(!q.contains("t4"), "source titles capped per finding: {q}");
        assert_eq!(post_research_query(&ResearchEvidence::default()), "");
    }
}
