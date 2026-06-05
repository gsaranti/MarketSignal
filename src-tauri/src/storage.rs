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
/// the scheduler/orchestration layer — see `jobs`). HTML and warning-state
/// tables remain out of scope for now. Idempotent, so any run path can call it.
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
    Ok(())
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
/// *display* query; the retention-cascade deletion that enforces the same number
/// on disk is a separate concern.
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
}
