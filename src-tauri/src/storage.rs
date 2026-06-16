//! SQLite persistence for report records. The application layer owns the
//! database; agents never touch it. rusqlite with the `bundled` feature keeps
//! SQLite in-process with no system-library dependency — clean for a signed
//! macOS bundle.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::agent::ReportSummary;

/// One report row to persist: the structured summary, the path to the canonical
/// Markdown file, and the full summary JSON blob.
pub struct ReportRecord<'a> {
    pub summary: &'a ReportSummary,
    pub markdown_path: &'a str,
    pub summary_json: &'a str,
}

/// Open (creating if absent) the SQLite database at `db_path`.
pub fn open(db_path: &std::path::Path) -> Result<Connection> {
    Ok(Connection::open(db_path)?)
}

/// Create the application tables if they do not exist: `reports` (one row per
/// generated report) and `job_runs` (one row per job lifecycle outcome, owned by
/// the scheduler/orchestration layer — see `jobs`). Warning-state tables remain
/// out of scope for now; HTML is never persisted (rendered on demand for
/// display/PDF, settled 2026-06-12), so it gets no table. Idempotent, so any run
/// path can call it.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS reports (
            report_id     TEXT PRIMARY KEY,
            report_type   TEXT NOT NULL,
            created_at    TEXT NOT NULL,
            risk_posture  TEXT NOT NULL,
            market_cycle  TEXT NOT NULL,
            thesis_stance TEXT NOT NULL,
            markdown_path TEXT NOT NULL,
            summary_json  TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS job_runs (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            job_type     TEXT NOT NULL,
            state        TEXT NOT NULL,
            started_at   TEXT NOT NULL,
            finished_at  TEXT NOT NULL,
            report_id    TEXT,
            detail       TEXT
        )",
        [],
    )?;
    // Persistent user/application state: the weekly job's enabled flag and the
    // Settings store — agent models, API tokens, provider credentials, written by
    // `settings::save` and read by `config::AppConfig::load`. A small key/value
    // table rather than typed columns; values that survive restarts but are not
    // part of a report record.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS app_settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;
    // Per-report baseline snapshots: the full Step-3 scan serialized as JSON, one row
    // per generated report, so the next run can diff this run's levels against the prior
    // report's (`baseline_delta`). `captured_at` is the app-minted scan time (the Δt
    // anchor); `schema_version` records the baseline shape for future migration tooling.
    // Capped to the newest `BASELINE_SNAPSHOT_RETENTION` rows by `prune_baseline_snapshots`.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS baseline_snapshots (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id      TEXT NOT NULL,
            captured_at    TEXT NOT NULL,
            schema_version INTEGER NOT NULL,
            baseline_json  TEXT NOT NULL
        )",
        [],
    )?;
    // Long-term semantic memory (`docs/storage.md §Vector Memory`), shipped on
    // SQLite as BLOB-stored embeddings with exact in-Rust cosine search — a conscious
    // deviation from the doc's LanceDB engine; rationale and the engine-swap seam live
    // in `vector_memory`'s module header. `kind` ∈ {summary, learning}: a report's
    // summary row cascades with its report (joined by `report_id`), durable learnings
    // survive report deletion. `embedding` is little-endian f32 bytes.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS vector_memory (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            kind       TEXT NOT NULL,
            report_id  TEXT,
            content    TEXT NOT NULL,
            embedding  BLOB NOT NULL,
            created_at TEXT NOT NULL
        )",
        [],
    )?;
    // Encodes the doc's "one embedding per report summary" in the schema rather than
    // trusting the flow (the persist step runs once per report_id today, but the
    // invariant is what the retrieval slice will lean on). Partial: learnings are
    // unconstrained — many may share a report_id, or carry none.
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ux_vector_memory_summary
         ON vector_memory(report_id) WHERE kind = 'summary'",
        [],
    )?;
    // The research-inbox parse failures from the most recent job pass
    // (`docs/research-documents.md §Parse Failures`): one row per file that could
    // not be parsed, identified by its listing identity (name + size + mtime) so
    // the Research Documents panel can show the error state against the file on
    // disk — and so an edited file stops matching its stale row. Replaced
    // wholesale each run by `replace_parse_failures`.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS research_parse_failures (
            name       TEXT PRIMARY KEY,
            size_bytes INTEGER NOT NULL,
            modified   TEXT,
            reason     TEXT NOT NULL,
            failed_at  TEXT NOT NULL
        )",
        [],
    )?;
    // Truncation telemetry: one row per inbox document the Step-6 parser had to
    // head-truncate, persisted per report so overflow frequency can be judged
    // before ever building the reserved GPT-5-mini extraction stage
    // (`docs/agents.md §Data Extraction`). On the `baseline_snapshots` model
    // (per-report, joined by `report_id`), but deliberately *accumulating* —
    // `record_document_truncations` appends, it never clears the table the way
    // `replace_parse_failures` does, so prior runs' evidence survives. Bounded
    // by the report-retention cascade (`delete_report_truncations`), not a
    // self-cap, so history rides along with the reports it describes.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS document_truncations (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id      TEXT NOT NULL,
            captured_at    TEXT NOT NULL,
            name           TEXT NOT NULL,
            format         TEXT NOT NULL,
            original_chars INTEGER NOT NULL,
            kept_chars     INTEGER NOT NULL
        )",
        [],
    )?;
    // The denominator for the truncation *rate*: one row per report whose Step-6
    // inbox pass parsed at least one document, recording how many docs it parsed
    // (truncated or not). `document_truncations` is the numerator (truncated docs
    // only); on its own it answers "how many truncations" but not "what share of
    // documents truncated". Same per-report, accumulating, cascade-bounded model
    // as `document_truncations` (`delete_report_parse_runs`), so numerator and
    // denominator always span the same retained-report window and the rate stays
    // honest.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS document_parse_runs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id   TEXT NOT NULL,
            captured_at TEXT NOT NULL,
            docs_parsed INTEGER NOT NULL
        )",
        [],
    )?;
    Ok(())
}

/// One recorded research-inbox parse failure, as the panel join reads it. The
/// identity triple (`name`, `size_bytes`, `modified`) mirrors
/// `research::ResearchDocument`, so a listing row matches its failure only while
/// the file on disk is byte-for-byte the one that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseFailureRow {
    pub name: String,
    pub size_bytes: u64,
    pub modified: Option<String>,
    pub reason: String,
    /// App-minted UTC RFC3339 stamp of the job pass that recorded the failure.
    pub failed_at: String,
}

/// Replace the recorded parse failures with this run's set — the table holds
/// "the failures of the most recent inbox pass", so a file that parsed (or was
/// deleted) self-heals out of it. One transaction: the panel never reads a
/// half-replaced state.
pub fn replace_parse_failures(conn: &Connection, rows: &[ParseFailureRow]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM research_parse_failures", [])?;
    for row in rows {
        tx.execute(
            "INSERT INTO research_parse_failures (name, size_bytes, modified, reason, failed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                row.name,
                row.size_bytes as i64,
                row.modified,
                row.reason,
                row.failed_at
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// All recorded parse failures, for the inbox listing's error-state join.
pub fn list_parse_failures(conn: &Connection) -> Result<Vec<ParseFailureRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, size_bytes, modified, reason, failed_at FROM research_parse_failures",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ParseFailureRow {
                name: row.get(0)?,
                size_bytes: row.get::<_, i64>(1)? as u64,
                modified: row.get(2)?,
                reason: row.get(3)?,
                failed_at: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// One head-truncation event: the report it was captured under, the app-minted
/// scan time, and the document's identity (`name`/`format`) with its full vs.
/// kept char counts. Append-only telemetry (`document_truncations`) — distinct
/// from the replace-wholesale [`ParseFailureRow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentTruncationRow {
    pub report_id: String,
    pub captured_at: String,
    pub name: String,
    pub format: String,
    pub original_chars: u64,
    pub kept_chars: u64,
}

/// Append this run's truncation rows to `document_truncations`. Unlike
/// [`replace_parse_failures`], there is no leading `DELETE` — the table
/// accumulates across runs so overflow frequency can be judged over time. One
/// transaction so a reader never sees a half-written run.
pub fn record_document_truncations(conn: &Connection, rows: &[DocumentTruncationRow]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for row in rows {
        tx.execute(
            "INSERT INTO document_truncations
                (report_id, captured_at, name, format, original_chars, kept_chars)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                row.report_id,
                row.captured_at,
                row.name,
                row.format,
                row.original_chars as i64,
                row.kept_chars as i64
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Append this run's parsed-document count to `document_parse_runs` — the
/// denominator that turns the truncation numerator into a rate. One row per
/// report whose inbox pass parsed at least one document; like
/// [`record_document_truncations`] it appends rather than replaces, so the
/// per-report history accumulates.
pub fn record_document_parse_run(
    conn: &Connection,
    report_id: &str,
    captured_at: &str,
    docs_parsed: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO document_parse_runs (report_id, captured_at, docs_parsed)
         VALUES (?1, ?2, ?3)",
        rusqlite::params![report_id, captured_at, docs_parsed as i64],
    )?;
    Ok(())
}

/// Aggregate view over `document_truncations` for the Settings diagnostics
/// section (`docs/agents.md §Data Extraction` — the accumulating evidence that
/// gates the reserved GPT-5-mini extraction stage). `total_truncations` is the
/// numerator and `total_docs_parsed` (from the companion `document_parse_runs`
/// table) the denominator, so a true share-of-documents truncation rate is
/// derivable. Both span the same retained-report window (both cascade by
/// `report_id`), so for any report whose run recorded both, the rate stays
/// honest. The one cohort gap is historical: truncation rows recorded before
/// `document_parse_runs` existed have no denominator counterpart, which
/// `unaligned_truncations` flags so the consumer can suppress a mixed-cohort
/// rate until those rows age out of the retention window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TruncationStats {
    /// Total truncation events recorded (rows in the table).
    pub total_truncations: u64,
    /// Total documents parsed across all recorded runs (Σ of `docs_parsed` in
    /// `document_parse_runs`) — the rate denominator. `0` when no run with a
    /// parsed document has been recorded yet.
    pub total_docs_parsed: u64,
    /// Truncation events whose report has no `document_parse_runs` row — i.e. a
    /// numerator cohort the denominator does not cover (typically rows recorded
    /// before the denominator table existed). Non-zero means the rate would mix
    /// cohorts; the Settings consumer withholds the rate while it is. `0` once
    /// every truncation report also has a parse-run row.
    pub unaligned_truncations: u64,
    /// Distinct reports that recorded at least one truncation.
    pub reports_affected: u64,
    /// Total characters cut across all events (Σ of `original − kept`).
    pub total_chars_dropped: u64,
    /// Per-format event counts, ordered by descending count then format name.
    pub by_format: Vec<FormatCount>,
    /// Most recent capture timestamp, or `None` when the table is empty.
    pub latest_captured_at: Option<String>,
}

/// One row of the per-format truncation breakdown in [`TruncationStats`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FormatCount {
    pub format: String,
    pub count: u64,
}

/// Aggregate `document_truncations` for the Settings diagnostics read: the
/// scalar headline numbers in one pass plus the per-format breakdown in a
/// grouped pass. An empty table yields the `Default` (all-zero counts, empty
/// breakdown, `None` timestamp) — itself the signal that overflow is not common.
pub fn truncation_stats(conn: &Connection) -> Result<TruncationStats> {
    let (total_truncations, reports_affected, total_chars_dropped, latest_captured_at) = conn
        .query_row(
            "SELECT
                COUNT(*),
                COUNT(DISTINCT report_id),
                COALESCE(SUM(original_chars - kept_chars), 0),
                MAX(captured_at)
             FROM document_truncations",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u64,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )?;

    let mut stmt = conn.prepare(
        "SELECT format, COUNT(*) FROM document_truncations
         GROUP BY format
         ORDER BY COUNT(*) DESC, format ASC",
    )?;
    let by_format = stmt
        .query_map([], |row| {
            Ok(FormatCount {
                format: row.get(0)?,
                count: row.get::<_, i64>(1)? as u64,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let total_docs_parsed = conn.query_row(
        "SELECT COALESCE(SUM(docs_parsed), 0) FROM document_parse_runs",
        [],
        |row| Ok(row.get::<_, i64>(0)? as u64),
    )?;

    // Truncations whose report never recorded a parse-run denominator — the
    // historical cohort gap. With an empty `document_parse_runs`, `NOT IN ()`
    // holds for every row, so a legacy-only table reports all its truncations as
    // unaligned (and the rate stays suppressed), which is the intended signal.
    let unaligned_truncations = conn.query_row(
        "SELECT COUNT(*) FROM document_truncations
         WHERE report_id NOT IN (SELECT report_id FROM document_parse_runs)",
        [],
        |row| Ok(row.get::<_, i64>(0)? as u64),
    )?;

    Ok(TruncationStats {
        total_truncations,
        total_docs_parsed,
        unaligned_truncations,
        reports_affected,
        total_chars_dropped,
        by_format,
        latest_captured_at,
    })
}

/// Read a value from `app_settings`, or `None` when the key has never been set.
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(value)
}

/// Insert or update a single `app_settings` row.
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// How many of the most recent reports the sidebar lists (`docs/storage.md` —
/// only the most recent 30 Weekly Market reports are retained). This bounds the
/// *display* query; the retention cascade that enforces the same number on disk
/// is bounded by [`REPORT_RETENTION`] below.
pub const RECENT_REPORTS_LIMIT: u32 = 30;

/// List the most recent reports, newest first, capped at `limit`. The stored
/// `summary_json` blob is the whole `ReportSummary`, so it round-trips back into
/// the struct; the `rowid` tiebreak keeps same-timestamp ordering stable
/// (insertion order) rather than arbitrary. A thin projection over
/// [`list_recent_reports_with_paths`] (it owns the query) — the Markdown path is
/// dropped for callers that only need the summary.
pub fn list_recent_reports(conn: &Connection, limit: u32) -> Result<Vec<ReportSummary>> {
    Ok(list_recent_reports_with_paths(conn, limit)?
        .into_iter()
        .map(|(summary, _path)| summary)
        .collect())
}

/// List the most recent reports as `(summary, markdown_path)`, newest first,
/// capped at `limit`. The stored `summary_json` blob is the whole `ReportSummary`,
/// so it round-trips back into the struct; the `rowid` tiebreak keeps
/// same-timestamp ordering stable (insertion order) rather than arbitrary. Returns
/// the canonical Markdown path alongside so the application layer
/// (`pipeline::load_recent_reports_for_audit`) can read each report's body for the
/// main agent's Step-2 prior-report context; [`list_recent_reports`] is the
/// summary-only projection over this query.
pub fn list_recent_reports_with_paths(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<(ReportSummary, String)>> {
    let mut stmt = conn.prepare(
        "SELECT summary_json, markdown_path FROM reports
         ORDER BY created_at DESC, rowid DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (json, path) = row?;
        out.push((serde_json::from_str(&json)?, path));
    }
    Ok(out)
}

/// Look up one report's canonical Markdown path and summary by id, or `None`
/// when no such report exists. The application layer (`pipeline::load_report`)
/// reads the Markdown file the path points at.
pub fn get_report_record(
    conn: &Connection,
    report_id: &str,
) -> Result<Option<(String, ReportSummary)>> {
    let row = conn
        .query_row(
            "SELECT markdown_path, summary_json FROM reports WHERE report_id = ?1",
            [report_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    match row {
        Some((path, json)) => Ok(Some((path, serde_json::from_str(&json)?))),
        None => Ok(None),
    }
}

/// Insert a report record. The regime columns store the canonical kebab labels;
/// the full summary lives in `summary_json` for retrieval.
pub fn insert_report(conn: &Connection, record: &ReportRecord) -> Result<()> {
    let s = record.summary;
    conn.execute(
        "INSERT INTO reports
            (report_id, report_type, created_at, risk_posture, market_cycle,
             thesis_stance, markdown_path, summary_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            s.report_id,
            s.report_type,
            s.created_at,
            s.risk_posture.as_str(),
            s.market_cycle.as_str(),
            s.thesis_stance.as_str(),
            record.markdown_path,
            record.summary_json,
        ],
    )?;
    Ok(())
}

/// How many reports the retention cascade keeps (`docs/storage.md` — only the
/// most recent 30 Weekly Market reports are retained; older reports are deleted
/// automatically). Deliberately a separate constant from [`RECENT_REPORTS_LIMIT`],
/// which bounds the sidebar's display query; the two agree today, and the shared
/// `created_at DESC, rowid DESC` ordering keeps the display window and the
/// retention window from ever disagreeing about which reports those are.
pub const REPORT_RETENTION: u32 = 30;

/// One report selected for eviction by the retention cascade: the id joins the
/// SQLite legs (report row, vector summary, baseline snapshots); the stored
/// markdown path locates the file leg.
pub struct ReportEvictee {
    pub report_id: String,
    pub markdown_path: String,
}

/// The reports outside the newest-`keep` window, oldest first — selection only;
/// the caller owns the per-evictee cascade. Uses the same `created_at DESC,
/// rowid DESC` ordering as [`list_recent_reports`], so retention evicts exactly
/// the reports the sidebar no longer shows. Empty at or under the cap.
pub fn select_reports_beyond_retention(
    conn: &Connection,
    keep: u32,
) -> Result<Vec<ReportEvictee>> {
    let mut stmt = conn.prepare(
        "SELECT report_id, markdown_path FROM reports
         WHERE report_id NOT IN (
             SELECT report_id FROM reports
             ORDER BY created_at DESC, rowid DESC
             LIMIT ?1
         )
         ORDER BY created_at ASC, rowid ASC",
    )?;
    let rows = stmt.query_map([keep], |row| {
        Ok(ReportEvictee {
            report_id: row.get(0)?,
            markdown_path: row.get(1)?,
        })
    })?;
    let mut evictees = Vec::new();
    for evictee in rows {
        evictees.push(evictee?);
    }
    Ok(evictees)
}

/// Delete one report's row — the final SQLite leg of the retention cascade,
/// called after the file and vector legs so a partially-failed cascade leaves
/// the row (and the next run's selection) behind rather than an untracked
/// orphan file.
pub fn delete_report_row(conn: &Connection, report_id: &str) -> Result<()> {
    conn.execute("DELETE FROM reports WHERE report_id = ?1", [report_id])?;
    Ok(())
}

/// Delete one report's baseline-snapshot rows (the join `report_id` was stored
/// for). The 14-snapshot cap prunes these long before the 30-report window
/// reaches them, so this leg is belt-and-braces against orphans, not the
/// primary bound.
pub fn delete_report_baseline_snapshots(conn: &Connection, report_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM baseline_snapshots WHERE report_id = ?1",
        [report_id],
    )?;
    Ok(())
}

/// Delete one report's truncation-telemetry rows — the cascade leg that bounds
/// the accumulating `document_truncations` table. Unlike the baseline-snapshot
/// table there is no independent self-cap, so this report-id join is the *only*
/// thing that reaps these rows: a row lives exactly as long as the report it
/// describes.
pub fn delete_report_truncations(conn: &Connection, report_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM document_truncations WHERE report_id = ?1",
        [report_id],
    )?;
    Ok(())
}

/// Delete one report's parsed-document-count row — the cascade leg that bounds
/// the accumulating `document_parse_runs` denominator table. Mirrors
/// [`delete_report_truncations`]: like the truncation numerator it has no
/// independent self-cap, so this report-id join is the only thing that reaps
/// these rows, and the two delete together so the rate's window stays aligned.
pub fn delete_report_parse_runs(conn: &Connection, report_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM document_parse_runs WHERE report_id = ?1",
        [report_id],
    )?;
    Ok(())
}

/// How many of the most recent baseline snapshots to retain (`baseline_snapshots`).
/// Report-indexed, not calendar-indexed: 14 *reports*, whatever their cadence. Only the
/// immediately-prior snapshot is needed for the change view today; the headroom leaves
/// room for a future trajectory-over-N read. Decoupled from the report retention window
/// ([`REPORT_RETENTION`]); `report_id` is stored so the report cascade can join
/// ([`delete_report_baseline_snapshots`]).
pub const BASELINE_SNAPSHOT_RETENTION: u32 = 14;

/// Persist one run's baseline snapshot: the serialized `BaselineMarketData`, the report
/// it backs, the app-minted `captured_at` scan time, and the baseline `schema_version`.
/// Serialization is the caller's concern — storage stays agnostic of the baseline shape.
pub fn insert_baseline_snapshot(
    conn: &Connection,
    report_id: &str,
    captured_at: &str,
    schema_version: u32,
    baseline_json: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO baseline_snapshots
            (report_id, captured_at, schema_version, baseline_json)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![report_id, captured_at, schema_version as i64, baseline_json],
    )?;
    Ok(())
}

/// The most recent baseline snapshot's `(captured_at, baseline_json)`, or `None` before
/// any snapshot exists (the first report). Ordered newest-first by `captured_at` with a
/// `rowid` tiebreak so same-timestamp inserts resolve to the latest insertion.
pub fn latest_baseline_snapshot(conn: &Connection) -> Result<Option<(String, String)>> {
    let row = conn
        .query_row(
            "SELECT captured_at, baseline_json FROM baseline_snapshots
             ORDER BY captured_at DESC, id DESC
             LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    Ok(row)
}

/// Delete all but the newest `keep` baseline snapshots (same newest-first ordering as
/// [`latest_baseline_snapshot`]). Idempotent; a no-op when at or under the cap.
pub fn prune_baseline_snapshots(conn: &Connection, keep: u32) -> Result<()> {
    conn.execute(
        "DELETE FROM baseline_snapshots
         WHERE id NOT IN (
             SELECT id FROM baseline_snapshots
             ORDER BY captured_at DESC, id DESC
             LIMIT ?1
         )",
        [keep],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn sample_summary(id: &str, created_at: &str) -> ReportSummary {
        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        ReportSummary {
            report_id: id.to_string(),
            report_type: "weekly_market".to_string(),
            created_at: created_at.to_string(),
            risk_posture: RiskPosture::Mixed,
            market_cycle: MarketCycle::LateCycle,
            thesis_stance: ThesisStance::Uncertain,
            header_summary_bullets: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            key_risks: vec![],
            unresolved_questions: vec![],
            forward_outlook_themes: vec![],
        }
    }

    fn insert_sample(conn: &Connection, id: &str, created_at: &str) {
        let summary = sample_summary(id, created_at);
        let summary_json = serde_json::to_string(&summary).unwrap();
        insert_report(
            conn,
            &ReportRecord {
                summary: &summary,
                markdown_path: &format!("/tmp/{id}.md"),
                summary_json: &summary_json,
            },
        )
        .unwrap();
    }

    #[test]
    fn list_recent_reports_caps_at_limit_and_orders_newest_first() {
        let conn = mem();
        // 32 reports with strictly ascending timestamps; ids encode insertion order.
        for i in 0..32 {
            let created_at = format!("2026-01-{:02}T00:00:00Z", i + 1);
            insert_sample(&conn, &format!("id-{i:02}"), &created_at);
        }
        let recent = list_recent_reports(&conn, 30).unwrap();
        assert_eq!(recent.len(), 30, "capped at the limit");
        // Newest (id-31) first; the two oldest (id-00, id-01) fall off the window.
        assert_eq!(recent[0].report_id, "id-31");
        assert_eq!(recent[29].report_id, "id-02");
    }

    #[test]
    fn list_recent_reports_with_paths_caps_orders_and_carries_the_path() {
        let conn = mem();
        for i in 0..5 {
            let created_at = format!("2026-02-{:02}T00:00:00Z", i + 1);
            insert_sample(&conn, &format!("id-{i:02}"), &created_at);
        }
        let recent = list_recent_reports_with_paths(&conn, 3).unwrap();
        assert_eq!(recent.len(), 3, "capped at the limit");
        // Newest first, each paired with its canonical Markdown path.
        assert_eq!(recent[0].0.report_id, "id-04");
        assert_eq!(recent[0].1, "/tmp/id-04.md");
        assert_eq!(recent[2].0.report_id, "id-02");
        assert_eq!(recent[2].1, "/tmp/id-02.md");
    }

    #[test]
    fn get_report_record_round_trips_and_misses_cleanly() {
        let conn = mem();
        insert_sample(&conn, "abc", "2026-02-01T00:00:00Z");
        let (path, summary) = get_report_record(&conn, "abc").unwrap().unwrap();
        assert_eq!(path, "/tmp/abc.md");
        assert_eq!(summary.report_id, "abc");
        assert_eq!(summary.created_at, "2026-02-01T00:00:00Z");
        assert!(get_report_record(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn select_reports_beyond_retention_is_empty_at_or_under_the_cap() {
        let conn = mem();
        for i in 0..3 {
            let created_at = format!("2026-01-0{}T00:00:00Z", i + 1);
            insert_sample(&conn, &format!("id-{i}"), &created_at);
        }
        assert!(select_reports_beyond_retention(&conn, 3).unwrap().is_empty());
        assert!(select_reports_beyond_retention(&conn, REPORT_RETENTION)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn select_reports_beyond_retention_returns_the_oldest_first() {
        let conn = mem();
        // 32 reports with strictly ascending timestamps; ids encode insertion order.
        for i in 0..32 {
            let created_at = format!("2026-01-{:02}T00:00:00Z", i + 1);
            insert_sample(&conn, &format!("id-{i:02}"), &created_at);
        }
        let evictees = select_reports_beyond_retention(&conn, REPORT_RETENTION).unwrap();
        assert_eq!(evictees.len(), 2, "two over the cap of 30");
        assert_eq!(evictees[0].report_id, "id-00");
        assert_eq!(evictees[1].report_id, "id-01");
        assert_eq!(evictees[0].markdown_path, "/tmp/id-00.md");
    }

    #[test]
    fn select_reports_beyond_retention_breaks_created_at_ties_by_insertion_order() {
        let conn = mem();
        // Three reports sharing one timestamp: rowid decides, exactly as it does
        // in list_recent_reports, so the earliest insertion is the one evicted.
        for id in ["first", "second", "third"] {
            insert_sample(&conn, id, "2026-03-01T00:00:00Z");
        }
        let evictees = select_reports_beyond_retention(&conn, 2).unwrap();
        assert_eq!(evictees.len(), 1);
        assert_eq!(evictees[0].report_id, "first");
    }

    #[test]
    fn delete_report_row_and_snapshots_remove_only_the_target() {
        let conn = mem();
        insert_sample(&conn, "keep", "2026-01-01T00:00:00Z");
        insert_sample(&conn, "evict", "2026-01-02T00:00:00Z");
        insert_baseline_snapshot(&conn, "keep", "2026-01-01T00:00:00Z", 1, "{}").unwrap();
        insert_baseline_snapshot(&conn, "evict", "2026-01-02T00:00:00Z", 1, "{}").unwrap();

        delete_report_row(&conn, "evict").unwrap();
        delete_report_baseline_snapshots(&conn, "evict").unwrap();

        assert!(get_report_record(&conn, "evict").unwrap().is_none());
        assert!(get_report_record(&conn, "keep").unwrap().is_some());
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM baseline_snapshots WHERE report_id = 'keep'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 1);
        let evicted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM baseline_snapshots WHERE report_id = 'evict'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(evicted, 0);
    }

    #[test]
    fn unset_setting_reads_as_none() {
        assert_eq!(get_setting(&mem(), "weekly_job_enabled").unwrap(), None);
    }

    #[test]
    fn set_then_get_round_trips_and_upserts() {
        let conn = mem();
        set_setting(&conn, "weekly_job_enabled", "false").unwrap();
        assert_eq!(
            get_setting(&conn, "weekly_job_enabled").unwrap().as_deref(),
            Some("false")
        );
        // Second write to the same key updates rather than erroring on the PK.
        set_setting(&conn, "weekly_job_enabled", "true").unwrap();
        assert_eq!(
            get_setting(&conn, "weekly_job_enabled").unwrap().as_deref(),
            Some("true")
        );
    }

    #[test]
    fn latest_baseline_snapshot_is_none_before_any_insert() {
        assert!(latest_baseline_snapshot(&mem()).unwrap().is_none());
    }

    #[test]
    fn baseline_snapshots_report_latest_and_prune_to_the_cap() {
        let conn = mem();
        // 16 snapshots, strictly ascending captured_at; the body marks insertion order.
        for i in 0..16 {
            let captured_at = format!("2026-01-{:02}T00:00:00Z", i + 1);
            insert_baseline_snapshot(
                &conn,
                &format!("rep-{i:02}"),
                &captured_at,
                crate::data_sources::BASELINE_SCHEMA_VERSION,
                &format!("{{\"marker\":{i}}}"),
            )
            .unwrap();
        }

        // Latest is the newest captured_at (the 16th insert).
        let (captured_at, json) = latest_baseline_snapshot(&conn).unwrap().unwrap();
        assert_eq!(captured_at, "2026-01-16T00:00:00Z");
        assert_eq!(json, "{\"marker\":15}");

        // Prune keeps only the newest 14; the two oldest fall off, the latest survives.
        prune_baseline_snapshots(&conn, BASELINE_SNAPSHOT_RETENTION).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM baseline_snapshots", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, BASELINE_SNAPSHOT_RETENTION as i64);
        let oldest_remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM baseline_snapshots
                 WHERE captured_at IN ('2026-01-01T00:00:00Z', '2026-01-02T00:00:00Z')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(oldest_remaining, 0);
        assert_eq!(
            latest_baseline_snapshot(&conn).unwrap().unwrap().0,
            "2026-01-16T00:00:00Z"
        );
    }

    #[test]
    fn parse_failures_are_replaced_wholesale_and_read_back() {
        let conn = mem();
        let row = |name: &str, reason: &str| ParseFailureRow {
            name: name.into(),
            size_bytes: 42,
            modified: Some("2026-06-09T12:00:00+00:00".into()),
            reason: reason.into(),
            failed_at: "2026-06-11T09:00:00+00:00".into(),
        };

        replace_parse_failures(&conn, &[row("a.pdf", "broken"), row("b.json", "not json")])
            .unwrap();
        let mut listed = list_parse_failures(&conn).unwrap();
        listed.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "a.pdf");
        assert_eq!(listed[0].size_bytes, 42);
        assert_eq!(listed[0].reason, "broken");

        // The next pass's set replaces the previous one — a healed file's row is gone.
        replace_parse_failures(&conn, &[row("b.json", "still not json")]).unwrap();
        let listed = list_parse_failures(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "b.json");
        assert_eq!(listed[0].reason, "still not json");

        // An empty pass clears the table.
        replace_parse_failures(&conn, &[]).unwrap();
        assert!(list_parse_failures(&conn).unwrap().is_empty());
    }

    #[test]
    fn document_truncations_accumulate_across_runs_and_cascade_by_report() {
        let conn = mem();
        let row = |report_id: &str, name: &str| DocumentTruncationRow {
            report_id: report_id.into(),
            captured_at: "2026-06-15T09:00:00+00:00".into(),
            name: name.into(),
            format: "pdf".into(),
            original_chars: 30_000,
            kept_chars: 12_000,
        };
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };

        // Two runs, each appending one row — the second must NOT clear the first
        // (the divergence from the replace-wholesale parse-failures table).
        record_document_truncations(&conn, &[row("rep-1", "big.pdf")]).unwrap();
        record_document_truncations(&conn, &[row("rep-2", "huge.pdf")]).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM document_truncations"), 2);

        // Field round-trip on the first row.
        let (name, original, kept): (String, i64, i64) = conn
            .query_row(
                "SELECT name, original_chars, kept_chars FROM document_truncations
                 WHERE report_id = 'rep-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(name, "big.pdf");
        assert_eq!(original, 30_000);
        assert_eq!(kept, 12_000);

        // The cascade leg reaps only the named report's rows.
        delete_report_truncations(&conn, "rep-1").unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM document_truncations"), 1);
        assert_eq!(
            count("SELECT COUNT(*) FROM document_truncations WHERE report_id = 'rep-2'"),
            1
        );
    }

    #[test]
    fn truncation_stats_aggregates_counts_formats_and_chars() {
        let conn = mem();

        // Empty table → all-zero default, no timestamp (the "overflow is rare"
        // signal the Settings section renders as its empty state).
        let empty = truncation_stats(&conn).unwrap();
        assert_eq!(empty, TruncationStats::default());
        assert_eq!(empty.latest_captured_at, None);

        let row = |report_id: &str, name: &str, format: &str, at: &str, original: u64, kept: u64| {
            DocumentTruncationRow {
                report_id: report_id.into(),
                captured_at: at.into(),
                name: name.into(),
                format: format.into(),
                original_chars: original,
                kept_chars: kept,
            }
        };

        // Two reports, three events across two formats.
        record_document_truncations(
            &conn,
            &[
                row("rep-1", "a.pdf", "pdf", "2026-06-01T09:00:00+00:00", 30_000, 12_000),
                row("rep-1", "b.pdf", "pdf", "2026-06-01T09:00:00+00:00", 20_000, 12_000),
            ],
        )
        .unwrap();
        record_document_truncations(
            &conn,
            &[row(
                "rep-2",
                "c.html",
                "html",
                "2026-06-08T09:00:00+00:00",
                15_000,
                12_000,
            )],
        )
        .unwrap();

        // Denominator: rep-1 parsed 7 docs (3 truncated), rep-2 parsed 4 (1
        // truncated) → 11 docs parsed against 3 truncations, the rate's two halves.
        record_document_parse_run(&conn, "rep-1", "2026-06-01T09:00:00+00:00", 7).unwrap();
        record_document_parse_run(&conn, "rep-2", "2026-06-08T09:00:00+00:00", 4).unwrap();

        let stats = truncation_stats(&conn).unwrap();
        assert_eq!(stats.total_truncations, 3);
        assert_eq!(stats.total_docs_parsed, 11);
        // Both truncation reports recorded a parse-run, so the cohorts align.
        assert_eq!(stats.unaligned_truncations, 0);
        assert_eq!(stats.reports_affected, 2);
        // (30k−12k) + (20k−12k) + (15k−12k) = 18k + 8k + 3k.
        assert_eq!(stats.total_chars_dropped, 29_000);
        // pdf (2 events) ordered before html (1) by descending count.
        assert_eq!(
            stats.by_format,
            vec![
                FormatCount {
                    format: "pdf".into(),
                    count: 2,
                },
                FormatCount {
                    format: "html".into(),
                    count: 1,
                },
            ]
        );
        // Newest capture across both runs.
        assert_eq!(
            stats.latest_captured_at.as_deref(),
            Some("2026-06-08T09:00:00+00:00")
        );
    }

    #[test]
    fn document_parse_runs_accumulate_and_cascade_by_report() {
        let conn = mem();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };

        // Two runs append rather than replace (the denominator accumulates the
        // way the numerator does), including a zero-truncation run that still
        // contributes its parsed-doc count.
        record_document_parse_run(&conn, "rep-1", "2026-06-01T09:00:00+00:00", 5).unwrap();
        record_document_parse_run(&conn, "rep-2", "2026-06-08T09:00:00+00:00", 3).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM document_parse_runs"), 2);
        assert_eq!(truncation_stats(&conn).unwrap().total_docs_parsed, 8);

        // The cascade leg reaps only the named report's row, keeping the
        // denominator aligned with the truncation numerator's window.
        delete_report_parse_runs(&conn, "rep-1").unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM document_parse_runs"), 1);
        assert_eq!(truncation_stats(&conn).unwrap().total_docs_parsed, 3);
    }

    #[test]
    fn truncation_stats_flags_truncations_without_a_parse_run_denominator() {
        let conn = mem();
        let trow = |report_id: &str, name: &str| DocumentTruncationRow {
            report_id: report_id.into(),
            captured_at: "2026-06-15T09:00:00+00:00".into(),
            name: name.into(),
            format: "pdf".into(),
            original_chars: 30_000,
            kept_chars: 12_000,
        };

        // rep-1's run recorded both legs (aligned); rep-2 is a legacy truncation
        // with no parse-run row — the cohort gap a pre-`document_parse_runs` build
        // would leave behind.
        record_document_truncations(&conn, &[trow("rep-1", "a.pdf"), trow("rep-2", "b.pdf")])
            .unwrap();
        record_document_parse_run(&conn, "rep-1", "2026-06-15T09:00:00+00:00", 5).unwrap();

        let stats = truncation_stats(&conn).unwrap();
        assert_eq!(stats.total_truncations, 2);
        assert_eq!(stats.total_docs_parsed, 5);
        // Exactly rep-2's truncation lacks a denominator, so the rate is unsafe.
        assert_eq!(stats.unaligned_truncations, 1);

        // Once rep-2 records its own parse-run, the cohorts realign.
        record_document_parse_run(&conn, "rep-2", "2026-06-15T09:00:00+00:00", 4).unwrap();
        assert_eq!(truncation_stats(&conn).unwrap().unaligned_truncations, 0);
    }
}
