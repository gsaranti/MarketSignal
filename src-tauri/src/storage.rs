//! SQLite persistence for report records. The application layer owns the
//! database; agents never touch it. rusqlite with the `bundled` feature keeps
//! SQLite in-process with no system-library dependency — clean for a signed
//! macOS bundle.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

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
/// (insertion order) rather than arbitrary.
pub fn list_recent_reports(conn: &Connection, limit: u32) -> Result<Vec<ReportSummary>> {
    let mut stmt = conn.prepare(
        "SELECT summary_json FROM reports
         ORDER BY created_at DESC, rowid DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| row.get::<_, String>(0))?;
    let mut summaries = Vec::new();
    for json in rows {
        summaries.push(serde_json::from_str(&json?)?);
    }
    Ok(summaries)
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
}
