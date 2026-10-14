//! SQLite persistence for report records. The application layer owns the
//! database; agents never touch it. rusqlite with the `bundled` feature keeps
//! SQLite in-process with no system-library dependency — clean for a signed
//! macOS bundle.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::agent::ReportSummary;

pub struct ReportRecord<'a> {
    pub summary: &'a ReportSummary,
    pub markdown_path: &'a str,
    pub summary_json: &'a str,
}

/// Sets `busy_timeout` and WAL on every open: a concurrency-skipped attempt
/// writes its `job_runs` row from a second connection while the winning run is
/// mid-flight, and without a busy timeout that write would return `SQLITE_BUSY`
/// immediately — a generic DB error instead of the clean "Skipped" outcome. WAL
/// lets the live run's reads and the skip-path write coexist.
pub fn open(db_path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    // `PRAGMA journal_mode=WAL` returns a result row, so go through `execute_batch`
    // (which discards rows) rather than `pragma_update` (which rejects them).
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}

/// Idempotent, so any run path can call it. HTML is never persisted (rendered
/// on demand), so it gets no table.
///
/// Constraint coupling with data portability: any UNIQUE / PRIMARY KEY / CHECK
/// constraint on an exported table (`portability::TABLES`) must be mirrored by
/// a pre-destructive validation in `portability::import_archive` — the import's
/// file mutations precede its row transaction, so an unmirrored constraint
/// firing mid-transaction would strand rolled-back rows pointing at
/// already-replaced files.
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
    // The Settings store plus the dismissed-warning markers, as key/value.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS app_settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;
    // `captured_at` is the app-minted scan time (the Δt anchor); `schema_version`
    // records the baseline shape for migration tooling.
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
    // Semantic memory (`docs/storage.md §Vector Memory`; the engine choice is in
    // `vector_memory`'s module header). `kind` ∈ {summary, learning}: a summary row
    // cascades with its report, a learning survives report deletion. `embedding`
    // is little-endian f32 bytes.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS vector_memory (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            kind       TEXT NOT NULL,
            namespace  TEXT NOT NULL DEFAULT 'report',
            report_id  TEXT,
            content    TEXT NOT NULL,
            embedding  BLOB NOT NULL,
            created_at TEXT NOT NULL
        )",
        [],
    )?;
    // `CREATE TABLE IF NOT EXISTS` is a no-op on an existing table, so the column
    // is added here; the `PRAGMA table_info` guard keeps a re-run from erroring on
    // a duplicate `ALTER`.
    if !column_exists(conn, "vector_memory", "namespace")? {
        conn.execute(
            "ALTER TABLE vector_memory ADD COLUMN namespace TEXT NOT NULL DEFAULT 'report'",
            [],
        )?;
    }
    // Encodes "one embedding per report summary" in the schema rather than
    // trusting the flow. Partial: learnings are unconstrained. Mirrored by a
    // data-portability import pre-check.
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ux_vector_memory_summary
         ON vector_memory(report_id) WHERE kind = 'summary'",
        [],
    )?;
    // The most recent pass's parse failures (`docs/research-documents.md §Parse
    // Failures`), identified by name + size + mtime so an edited file stops
    // matching its stale row. Replaced wholesale each run.
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
    // Truncation telemetry (`docs/agents.md §Data Extraction`): one row per
    // head-truncated inbox document, per report. Accumulating — never cleared the
    // way `replace_parse_failures` does — and bounded by the report-retention
    // cascade, not a self-cap.
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
    // The truncation rate's denominator: one row per report whose inbox pass
    // parsed at least one document, accumulating and cascade-bounded like
    // `document_truncations`. `total_original_chars` is nullable because a row
    // recorded before the column existed has no value to backfill; NULL marks that
    // cohort distinctly from a genuine `0`, and the chars ratio is withheld while
    // any NULL row remains (`TruncationStats::parse_runs_missing_original_chars`).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS document_parse_runs (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id           TEXT NOT NULL,
            captured_at         TEXT NOT NULL,
            docs_parsed         INTEGER NOT NULL,
            total_original_chars INTEGER
        )",
        [],
    )?;
    // SQLite has no `ADD COLUMN IF NOT EXISTS`, so the `PRAGMA table_info` guard
    // keeps the additive migration idempotent.
    if !column_exists(conn, "document_parse_runs", "total_original_chars")? {
        conn.execute(
            "ALTER TABLE document_parse_runs ADD COLUMN total_original_chars INTEGER",
            [],
        )?;
    }
    // The local-suite schemas (`docs/storage.md §Local Analysis Suite Storage`)
    // live beside the modules that own them.
    crate::portfolio::store::init_schema(conn)?;
    crate::web_research::store::init_schema(conn)?;
    crate::portfolio::factor_store::init_schema(conn)?;
    Ok(())
}

/// Keeps additive `ALTER TABLE ADD COLUMN` migrations idempotent in the absence
/// of a `user_version` framework.
pub(crate) fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let found = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == column);
    Ok(found)
}

/// The identity triple (`name`, `size_bytes`, `modified`) mirrors
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

/// The table holds the failures of the most recent inbox pass, so a file that
/// parsed (or was deleted) self-heals out of it. One transaction: the panel
/// never reads a half-replaced state.
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

/// One head-truncation event. Append-only telemetry, distinct from the
/// replace-wholesale [`ParseFailureRow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentTruncationRow {
    pub report_id: String,
    pub captured_at: String,
    pub name: String,
    pub format: String,
    pub original_chars: u64,
    pub kept_chars: u64,
}

/// Unlike [`replace_parse_failures`] there is no leading `DELETE`: the table
/// accumulates across runs. One transaction so a reader never sees a
/// half-written run.
pub fn record_document_truncations(
    conn: &Connection,
    rows: &[DocumentTruncationRow],
) -> Result<()> {
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

/// The denominator that turns the truncation numerator into a rate: one row
/// per report whose inbox pass parsed at least one document, appended like
/// [`record_document_truncations`]. `total_original_chars` is the Σ of every
/// parsed doc's pre-truncation char count — the chars-ratio denominator.
pub fn record_document_parse_run(
    conn: &Connection,
    report_id: &str,
    captured_at: &str,
    docs_parsed: u64,
    total_original_chars: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO document_parse_runs (report_id, captured_at, docs_parsed, total_original_chars)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            report_id,
            captured_at,
            docs_parsed as i64,
            total_original_chars as i64
        ],
    )?;
    Ok(())
}

/// The Settings diagnostics' aggregate (`docs/agents.md §Data Extraction`).
/// `total_truncations` is the numerator and `total_docs_parsed` the
/// denominator; both cascade by `report_id`, so they span the same retained
/// window. `unaligned_truncations` flags the cohort gap so the consumer can
/// suppress a mixed-cohort rate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TruncationStats {
    pub total_truncations: u64,
    /// Σ of `docs_parsed` — the rate denominator; `0` when no run with a parsed
    /// document has been recorded.
    pub total_docs_parsed: u64,
    /// Truncation events whose report has no `document_parse_runs` row.
    /// Non-zero means the rate would mix cohorts, and the Settings consumer
    /// withholds it.
    pub unaligned_truncations: u64,
    /// Σ of `total_original_chars` — the chars-ratio denominator; `0` when no
    /// run with a non-NULL char count has been recorded.
    pub total_original_chars: u64,
    /// Parse-run rows with a NULL `total_original_chars`. SQL `SUM` skips NULL,
    /// so a non-zero value means the chars ratio would understate its
    /// denominator, and the Settings consumer withholds it. Distinct from
    /// `unaligned_truncations` (a missing row, not a NULL column).
    pub parse_runs_missing_original_chars: u64,
    /// Distinct reports that recorded at least one truncation.
    pub reports_affected: u64,
    /// Σ of `original − kept`.
    pub total_chars_dropped: u64,
    /// Ordered by descending count then format name.
    pub by_format: Vec<FormatCount>,
    /// `None` when the table is empty.
    pub latest_captured_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FormatCount {
    pub format: String,
    pub count: u64,
}

/// An empty table yields the `Default` — itself the signal that overflow is not
/// common.
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

    let (total_docs_parsed, total_original_chars, parse_runs_missing_original_chars) = conn
        .query_row(
            "SELECT
                COALESCE(SUM(docs_parsed), 0),
                COALESCE(SUM(total_original_chars), 0),
                COALESCE(SUM(CASE WHEN total_original_chars IS NULL THEN 1 ELSE 0 END), 0)
             FROM document_parse_runs",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u64,
                ))
            },
        )?;

    // With an empty `document_parse_runs`, `NOT IN ()` holds for every row, so a
    // legacy-only table reports all its truncations as unaligned and the rate
    // stays suppressed — the intended signal.
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
        total_original_chars,
        parse_runs_missing_original_chars,
        reports_affected,
        total_chars_dropped,
        by_format,
        latest_captured_at,
    })
}

/// `None` when the key has never been set.
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

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Bounds the sidebar's display query (`docs/storage.md`); the on-disk cascade
/// is bounded by [`REPORT_RETENTION`].
pub const RECENT_REPORTS_LIMIT: u32 = 30;

/// A summary-only projection over [`list_recent_reports_with_paths`], which
/// owns the query.
pub fn list_recent_reports(conn: &Connection, limit: u32) -> Result<Vec<ReportSummary>> {
    Ok(list_recent_reports_with_paths(conn, limit)?
        .into_iter()
        .map(|(summary, _path)| summary)
        .collect())
}

/// Newest first, capped at `limit`; the `rowid` tiebreak keeps same-timestamp
/// ordering stable. The Markdown path rides along so
/// `pipeline::load_recent_reports_for_audit` can read each report's body.
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

/// `None` when no such report exists.
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

/// The regime columns store the canonical kebab labels.
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

/// Idempotent launch-time migration (`docs/storage.md §Legacy Naming
/// Migration`). Drives off each row's stored `markdown_path` (a substring
/// replace), so the real `-<id8>` suffix on the canonical filename is
/// preserved. The `report_type` column and the `summary_json` blob are
/// rewritten together, from the deserialized summary, so the two can never
/// disagree. Per-row fail-soft: an undecodable summary is logged and skipped,
/// and a failed rename leaves the stored path at the file's actual location so
/// the next launch retries it.
pub fn migrate_legacy_naming(conn: &Connection) -> Result<()> {
    const LEGACY_SLUG: &str = "weekly_market";
    const NEW_SLUG: &str = "market_signal";
    const LEGACY_NAME: &str = "market-signal-weekly-report";
    const NEW_NAME: &str = "market-signal-report";

    // Snapshot the rows before mutating so the read statement isn't held open
    // across the per-row UPDATE.
    let mut stmt = conn.prepare("SELECT report_id, summary_json, markdown_path FROM reports")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    for (report_id, summary_json, markdown_path) in rows {
        let mut summary: ReportSummary = match serde_json::from_str(&summary_json) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "legacy-naming migration: skipping report {report_id}, undecodable summary ({e})"
                );
                continue;
            }
        };

        let slug_stale = summary.report_type == LEGACY_SLUG;
        let new_path = markdown_path.replace(LEGACY_NAME, NEW_NAME);
        let path_stale = new_path != markdown_path;

        if !slug_stale && !path_stale {
            continue;
        }

        if slug_stale {
            summary.report_type = NEW_SLUG.to_string();
        }

        // Rename before recording the new path, so the persisted path always
        // points at where the file is. Old gone and new present: a prior run
        // renamed it. Neither: an orphan; adopt the new path anyway so the stored
        // convention is consistent (the read path errors on a missing file).
        let stored_path = if path_stale {
            let old = std::path::Path::new(&markdown_path);
            let new = std::path::Path::new(&new_path);
            if old.exists() && !new.exists() {
                match std::fs::rename(old, new) {
                    Ok(()) => new_path.clone(),
                    Err(e) => {
                        eprintln!(
                            "legacy-naming migration: could not rename {markdown_path} -> {new_path} ({e}); leaving stored path"
                        );
                        markdown_path.clone()
                    }
                }
            } else {
                new_path.clone()
            }
        } else {
            markdown_path.clone()
        };

        let summary_json_new = serde_json::to_string(&summary)?;
        conn.execute(
            "UPDATE reports SET report_type = ?1, summary_json = ?2, markdown_path = ?3
             WHERE report_id = ?4",
            rusqlite::params![summary.report_type, summary_json_new, stored_path, report_id],
        )?;
    }
    Ok(())
}

/// The launch-time sibling of [`migrate_legacy_naming`] for `job_runs.job_type`;
/// idempotent because the `WHERE` matches only legacy rows. The new slug is
/// deliberately duplicated here rather than shared with `jobs`: this freezes the
/// historical mapping, independent of the producer's current value.
pub fn migrate_legacy_job_type(conn: &Connection) -> Result<()> {
    const LEGACY_SLUG: &str = "weekly_market";
    const NEW_SLUG: &str = "market_signal";

    conn.execute(
        "UPDATE job_runs SET job_type = ?1 WHERE job_type = ?2",
        rusqlite::params![NEW_SLUG, LEGACY_SLUG],
    )?;
    Ok(())
}

/// How many reports the retention cascade keeps (`docs/storage.md`). A separate
/// constant from [`RECENT_REPORTS_LIMIT`], which bounds the sidebar's display
/// query: the two windows are sized alike but keyed differently — the sidebar by
/// `created_at`, the eviction by insertion order — so they can disagree about
/// which 30, never about how many (see [`select_reports_beyond_retention`]).
pub const REPORT_RETENTION: u32 = 30;

/// The id joins the SQLite legs of the cascade; the stored markdown path
/// locates the file leg.
pub struct ReportEvictee {
    pub report_id: String,
    pub markdown_path: String,
}

/// Selection only; the caller owns the per-evictee cascade. Empty at or under
/// the cap. The keep window is insertion order (`rowid`), not `created_at`: the
/// pipeline inserts and then prunes, so keyed on the wall clock a backwards step
/// would evict the report the run had only just produced.
/// [`list_recent_reports`] deliberately stays `created_at`-ordered — a report is
/// a dated document — so only the eviction decision, which is identity-like,
/// moves.
pub fn select_reports_beyond_retention(conn: &Connection, keep: u32) -> Result<Vec<ReportEvictee>> {
    let mut stmt = conn.prepare(
        "SELECT report_id, markdown_path FROM reports
         WHERE report_id NOT IN (
             SELECT report_id FROM reports
             ORDER BY rowid DESC
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

/// The final leg of the retention cascade, called after the file and vector
/// legs so a partially-failed cascade leaves the row behind rather than an
/// untracked orphan file.
pub fn delete_report_row(conn: &Connection, report_id: &str) -> Result<()> {
    conn.execute("DELETE FROM reports WHERE report_id = ?1", [report_id])?;
    Ok(())
}

/// The 14-snapshot cap prunes these long before the 30-report window reaches
/// them, so this leg is belt-and-braces against orphans, not the primary bound.
pub fn delete_report_baseline_snapshots(conn: &Connection, report_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM baseline_snapshots WHERE report_id = ?1",
        [report_id],
    )?;
    Ok(())
}

/// `document_truncations` has no self-cap, so this report-id join is the only
/// thing that reaps its rows.
pub fn delete_report_truncations(conn: &Connection, report_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM document_truncations WHERE report_id = ?1",
        [report_id],
    )?;
    Ok(())
}

/// Mirrors [`delete_report_truncations`]: no self-cap, and the two delete
/// together so the rate's window stays aligned.
pub fn delete_report_parse_runs(conn: &Connection, report_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM document_parse_runs WHERE report_id = ?1",
        [report_id],
    )?;
    Ok(())
}

/// Report-indexed, not calendar-indexed: 14 reports, whatever their cadence.
/// The change view reads only the immediately-prior snapshot; the baseline
/// history reads every retained one, so this cap is also the history's depth.
/// Decoupled from [`REPORT_RETENTION`].
pub const BASELINE_SNAPSHOT_RETENTION: u32 = 14;

#[derive(Debug, Clone, PartialEq)]
pub struct StoredBaselineSnapshot {
    pub report_id: String,
    pub captured_at: String,
    pub baseline_json: String,
}

/// Serialization is the caller's concern — storage stays agnostic of the
/// baseline shape.
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

/// `(captured_at, baseline_json)`, or `None` before any snapshot exists. Newest
/// by insertion order (`id`), not `captured_at`: under a backwards clock step an
/// older snapshot would answer as "latest", and this snapshot is the next
/// report's delta baseline. `captured_at` is the capture instant the interval
/// reads, not the identity.
pub fn latest_baseline_snapshot(conn: &Connection) -> Result<Option<(String, String)>> {
    let row = conn
        .query_row(
            "SELECT captured_at, baseline_json FROM baseline_snapshots
             ORDER BY id DESC
             LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    Ok(row)
}

/// Newest first by insertion order, the [`latest_baseline_snapshot`] rule, so a
/// backwards clock step can't reorder the history's points.
pub fn recent_baseline_snapshots(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<StoredBaselineSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT report_id, captured_at, baseline_json FROM baseline_snapshots
         ORDER BY id DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |row| {
            Ok(StoredBaselineSnapshot {
                report_id: row.get(0)?,
                captured_at: row.get(1)?,
                baseline_json: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The same insertion-order rule as [`latest_baseline_snapshot`], and it matters
/// more here: the pipeline inserts then prunes, so under `captured_at` a
/// backwards clock step could delete the row it had only just written.
/// Idempotent.
pub fn prune_baseline_snapshots(conn: &Connection, keep: u32) -> Result<()> {
    conn.execute(
        "DELETE FROM baseline_snapshots
         WHERE id NOT IN (
             SELECT id FROM baseline_snapshots
             ORDER BY id DESC
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

    #[test]
    fn open_sets_wal_and_busy_timeout() {
        // WAL needs a file-backed DB (in-memory reports "memory"), so go through a
        // tempdir rather than `mem()`.
        let tmp = tempfile::tempdir().unwrap();
        let conn = open(&tmp.path().join("hygiene.db")).unwrap();

        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal", "open() should enable WAL");

        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, 5000, "open() should set a 5s busy timeout");
    }

    fn sample_summary(id: &str, created_at: &str) -> ReportSummary {
        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        ReportSummary {
            report_id: id.to_string(),
            report_type: "weekly_market".to_string(),
            created_at: created_at.to_string(),
            title: "Sample headline".to_string(),
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
    fn migrate_legacy_naming_rewrites_slug_and_renames_file_idempotently() {
        let conn = mem();
        let tmp = tempfile::tempdir().unwrap();

        // The legacy name carries the real `-<id8>` suffix the doc's simplified
        // example omits.
        let legacy_path = tmp
            .path()
            .join("2026-06-03-market-signal-weekly-report-aaaaaaaa.md");
        std::fs::write(&legacy_path, "# Report\n").unwrap();
        let legacy_path_str = legacy_path.to_string_lossy().into_owned();
        let summary = sample_summary("rid-legacy", "2026-06-03T12:00:00Z");
        assert_eq!(summary.report_type, "weekly_market", "fixture starts legacy");
        let summary_json = serde_json::to_string(&summary).unwrap();
        insert_report(
            &conn,
            &ReportRecord {
                summary: &summary,
                markdown_path: &legacy_path_str,
                summary_json: &summary_json,
            },
        )
        .unwrap();

        migrate_legacy_naming(&conn).unwrap();

        let new_path = tmp
            .path()
            .join("2026-06-03-market-signal-report-aaaaaaaa.md");
        assert!(new_path.exists(), "renamed to the new convention");
        assert!(!legacy_path.exists(), "legacy file removed");

        let (stored_path, migrated) = get_report_record(&conn, "rid-legacy").unwrap().unwrap();
        assert_eq!(stored_path, new_path.to_string_lossy());
        assert_eq!(
            migrated.report_type, "market_signal",
            "summary_json slug rewritten"
        );
        let col: String = conn
            .query_row(
                "SELECT report_type FROM reports WHERE report_id = ?1",
                ["rid-legacy"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(col, "market_signal", "report_type column rewritten");

        migrate_legacy_naming(&conn).unwrap();
        let (stored_path_again, _) = get_report_record(&conn, "rid-legacy").unwrap().unwrap();
        assert_eq!(stored_path_again, stored_path, "second pass is a no-op");
        assert!(new_path.exists());
    }

    #[test]
    fn migrate_legacy_naming_leaves_already_migrated_rows_untouched() {
        let conn = mem();
        let mut summary = sample_summary("rid-new", "2026-06-03T12:00:00Z");
        summary.report_type = "market_signal".to_string();
        // Already on the new path, and deliberately not on disk: a row needing no
        // change is skipped before any filesystem touch.
        let new_path = "/tmp/2026-06-03-market-signal-report-bbbbbbbb.md";
        let summary_json = serde_json::to_string(&summary).unwrap();
        insert_report(
            &conn,
            &ReportRecord {
                summary: &summary,
                markdown_path: new_path,
                summary_json: &summary_json,
            },
        )
        .unwrap();

        migrate_legacy_naming(&conn).unwrap();

        let (stored_path, migrated) = get_report_record(&conn, "rid-new").unwrap().unwrap();
        assert_eq!(stored_path, new_path, "path left untouched");
        assert_eq!(migrated.report_type, "market_signal");
    }

    #[test]
    fn migrate_legacy_job_type_rewrites_slug_idempotently() {
        let conn = mem();
        conn.execute(
            "INSERT INTO job_runs (job_type, state, started_at, finished_at)
             VALUES ('weekly_market', 'successful', '2026-06-03T12:00:00Z', '2026-06-03T12:05:00Z')",
            [],
        )
        .unwrap();

        migrate_legacy_job_type(&conn).unwrap();

        let slug: String = conn
            .query_row("SELECT job_type FROM job_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(slug, "market_signal", "legacy job slug rewritten");

        migrate_legacy_job_type(&conn).unwrap();
        let slug_again: String = conn
            .query_row("SELECT job_type FROM job_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(slug_again, "market_signal", "second pass is a no-op");
    }

    #[test]
    fn migrate_legacy_job_type_leaves_non_legacy_rows_untouched() {
        let conn = mem();
        // An already-current row plus a hypothetical future job type.
        conn.execute(
            "INSERT INTO job_runs (job_type, state, started_at, finished_at)
             VALUES ('market_signal', 'successful', '2026-06-03T12:00:00Z', '2026-06-03T12:05:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO job_runs (job_type, state, started_at, finished_at)
             VALUES ('some_other_job', 'failed', '2026-06-04T12:00:00Z', '2026-06-04T12:05:00Z')",
            [],
        )
        .unwrap();

        migrate_legacy_job_type(&conn).unwrap();

        let other: String = conn
            .query_row(
                "SELECT job_type FROM job_runs WHERE state = 'failed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(other, "some_other_job", "non-legacy slug left untouched");
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
        assert!(select_reports_beyond_retention(&conn, 3)
            .unwrap()
            .is_empty());
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
    fn a_backwards_clock_step_does_not_evict_the_report_the_run_just_wrote() {
        let conn = mem();
        // A full store, then one more report dated earlier than every row in it —
        // an NTP correction or a manual clock change between runs.
        for i in 0..REPORT_RETENTION {
            insert_sample(
                &conn,
                &format!("id-{i:02}"),
                &format!("2026-05-{:02}T00:00:00Z", i + 1),
            );
        }
        insert_sample(&conn, "id-stepped", "2026-01-01T00:00:00Z");

        let evictees = select_reports_beyond_retention(&conn, REPORT_RETENTION).unwrap();
        assert_eq!(evictees.len(), 1, "one over the cap");
        assert_eq!(
            evictees[0].report_id, "id-00",
            "the genuinely-oldest INSERTION is evicted, not the newest write"
        );
        assert!(
            !evictees.iter().any(|e| e.report_id == "id-stepped"),
            "the report this run just wrote must survive its own prune"
        );

        // Production prunes in the same run, so the sidebar is asserted against
        // the 30 rows that remain, not the un-pruned 31 it never renders.
        delete_report_row(&conn, &evictees[0].report_id).unwrap();
        let listed = list_recent_reports(&conn, REPORT_RETENTION).unwrap();
        assert_eq!(listed.len(), REPORT_RETENTION as usize, "still exactly 30 rows");
        assert!(
            listed.iter().any(|r| r.report_id == "id-stepped"),
            "the preserved report is shown — the windows differ on WHICH 30, not how many"
        );
        assert_eq!(
            listed.last().unwrap().report_id,
            "id-stepped",
            "and the date-ordered view places it last, because its own date is the oldest"
        );
    }

    #[test]
    fn select_reports_beyond_retention_breaks_created_at_ties_by_insertion_order() {
        let conn = mem();
        // Three reports sharing one timestamp: rowid decides.
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
        assert_eq!(get_setting(&mem(), "demo_key").unwrap(), None);
    }

    #[test]
    fn set_then_get_round_trips_and_upserts() {
        let conn = mem();
        set_setting(&conn, "demo_key", "first").unwrap();
        assert_eq!(
            get_setting(&conn, "demo_key").unwrap().as_deref(),
            Some("first")
        );
        set_setting(&conn, "demo_key", "second").unwrap();
        assert_eq!(
            get_setting(&conn, "demo_key").unwrap().as_deref(),
            Some("second")
        );
    }

    #[test]
    fn latest_baseline_snapshot_is_none_before_any_insert() {
        assert!(latest_baseline_snapshot(&mem()).unwrap().is_none());
    }

    #[test]
    fn a_backwards_clock_step_neither_hides_nor_deletes_the_newest_snapshot() {
        // The snapshot the pipeline just wrote is the next report's delta
        // baseline, so "latest" and "keep" must both be insertion order.
        let conn = mem();
        for i in 0..BASELINE_SNAPSHOT_RETENTION {
            insert_baseline_snapshot(
                &conn,
                &format!("rep-{i:02}"),
                &format!("2026-03-{:02}T00:00:00Z", i + 1),
                crate::data_sources::BASELINE_SCHEMA_VERSION,
                &format!("{{\"marker\":{i}}}"),
            )
            .unwrap();
        }
        // The newest insert, dated BEFORE every row already stored.
        insert_baseline_snapshot(
            &conn,
            "rep-stepped",
            "2026-01-01T00:00:00Z",
            crate::data_sources::BASELINE_SCHEMA_VERSION,
            "{\"marker\":\"stepped\"}",
        )
        .unwrap();

        assert_eq!(
            latest_baseline_snapshot(&conn).unwrap().unwrap().1,
            "{\"marker\":\"stepped\"}",
            "the just-inserted snapshot is the latest, whatever its timestamp says"
        );
        prune_baseline_snapshots(&conn, BASELINE_SNAPSHOT_RETENTION).unwrap();
        assert_eq!(
            latest_baseline_snapshot(&conn).unwrap().unwrap().1,
            "{\"marker\":\"stepped\"}",
            "the prune must not delete the row the same run just wrote"
        );
        let survived: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM baseline_snapshots WHERE report_id = 'rep-stepped'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(survived, 1);
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

        let (captured_at, json) = latest_baseline_snapshot(&conn).unwrap().unwrap();
        assert_eq!(captured_at, "2026-01-16T00:00:00Z");
        assert_eq!(json, "{\"marker\":15}");

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
    fn recent_baseline_snapshots_read_newest_first_by_insertion_up_to_the_limit() {
        let conn = mem();
        assert!(
            recent_baseline_snapshots(&conn, BASELINE_SNAPSHOT_RETENTION)
                .unwrap()
                .is_empty(),
            "no snapshot before any insert"
        );
        for (i, captured_at) in ["2026-03-01T00:00:00Z", "2026-03-02T00:00:00Z"]
            .iter()
            .enumerate()
        {
            insert_baseline_snapshot(
                &conn,
                &format!("rep-{i}"),
                captured_at,
                crate::data_sources::BASELINE_SCHEMA_VERSION,
                &format!("{{\"marker\":{i}}}"),
            )
            .unwrap();
        }
        // The newest insert, dated BEFORE both rows already stored.
        insert_baseline_snapshot(
            &conn,
            "rep-stepped",
            "2026-01-01T00:00:00Z",
            crate::data_sources::BASELINE_SCHEMA_VERSION,
            "{\"marker\":\"stepped\"}",
        )
        .unwrap();

        let all = recent_baseline_snapshots(&conn, BASELINE_SNAPSHOT_RETENTION).unwrap();
        let ids: Vec<&str> = all.iter().map(|s| s.report_id.as_str()).collect();
        assert_eq!(ids, ["rep-stepped", "rep-1", "rep-0"]);
        assert_eq!(
            all[0],
            StoredBaselineSnapshot {
                report_id: "rep-stepped".into(),
                captured_at: "2026-01-01T00:00:00Z".into(),
                baseline_json: "{\"marker\":\"stepped\"}".into(),
            }
        );

        let two = recent_baseline_snapshots(&conn, 2).unwrap();
        let ids: Vec<&str> = two.iter().map(|s| s.report_id.as_str()).collect();
        assert_eq!(ids, ["rep-stepped", "rep-1"], "the limit keeps the newest");
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

        replace_parse_failures(&conn, &[row("b.json", "still not json")]).unwrap();
        let listed = list_parse_failures(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "b.json");
        assert_eq!(listed[0].reason, "still not json");

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

        record_document_truncations(&conn, &[row("rep-1", "big.pdf")]).unwrap();
        record_document_truncations(&conn, &[row("rep-2", "huge.pdf")]).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM document_truncations"), 2);

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

        let empty = truncation_stats(&conn).unwrap();
        assert_eq!(empty, TruncationStats::default());
        assert_eq!(empty.latest_captured_at, None);

        let row =
            |report_id: &str, name: &str, format: &str, at: &str, original: u64, kept: u64| {
                DocumentTruncationRow {
                    report_id: report_id.into(),
                    captured_at: at.into(),
                    name: name.into(),
                    format: format.into(),
                    original_chars: original,
                    kept_chars: kept,
                }
            };

        record_document_truncations(
            &conn,
            &[
                row(
                    "rep-1",
                    "a.pdf",
                    "pdf",
                    "2026-06-01T09:00:00+00:00",
                    30_000,
                    12_000,
                ),
                row(
                    "rep-1",
                    "b.pdf",
                    "pdf",
                    "2026-06-01T09:00:00+00:00",
                    20_000,
                    12_000,
                ),
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

        // rep-1 parsed 7 docs totalling 100k original chars, rep-2 parsed 4
        // totalling 50k: 11 docs / 150k chars against 3 truncations / 29k dropped.
        record_document_parse_run(&conn, "rep-1", "2026-06-01T09:00:00+00:00", 7, 100_000).unwrap();
        record_document_parse_run(&conn, "rep-2", "2026-06-08T09:00:00+00:00", 4, 50_000).unwrap();

        let stats = truncation_stats(&conn).unwrap();
        assert_eq!(stats.total_truncations, 3);
        assert_eq!(stats.total_docs_parsed, 11);
        assert_eq!(stats.unaligned_truncations, 0);
        assert_eq!(stats.total_original_chars, 150_000);
        assert_eq!(stats.parse_runs_missing_original_chars, 0);
        assert_eq!(stats.reports_affected, 2);
        // (30k−12k) + (20k−12k) + (15k−12k) = 18k + 8k + 3k.
        assert_eq!(stats.total_chars_dropped, 29_000);
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
        assert_eq!(
            stats.latest_captured_at.as_deref(),
            Some("2026-06-08T09:00:00+00:00")
        );
    }

    #[test]
    fn document_parse_runs_accumulate_and_cascade_by_report() {
        let conn = mem();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };

        record_document_parse_run(&conn, "rep-1", "2026-06-01T09:00:00+00:00", 5, 40_000).unwrap();
        record_document_parse_run(&conn, "rep-2", "2026-06-08T09:00:00+00:00", 3, 20_000).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM document_parse_runs"), 2);
        let stats = truncation_stats(&conn).unwrap();
        assert_eq!(stats.total_docs_parsed, 8);
        assert_eq!(stats.total_original_chars, 60_000);

        delete_report_parse_runs(&conn, "rep-1").unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM document_parse_runs"), 1);
        let stats = truncation_stats(&conn).unwrap();
        assert_eq!(stats.total_docs_parsed, 3);
        assert_eq!(stats.total_original_chars, 20_000);
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

        // rep-2 has no parse-run row: the cohort gap.
        record_document_truncations(&conn, &[trow("rep-1", "a.pdf"), trow("rep-2", "b.pdf")])
            .unwrap();
        record_document_parse_run(&conn, "rep-1", "2026-06-15T09:00:00+00:00", 5, 40_000).unwrap();

        let stats = truncation_stats(&conn).unwrap();
        assert_eq!(stats.total_truncations, 2);
        assert_eq!(stats.total_docs_parsed, 5);
        assert_eq!(stats.unaligned_truncations, 1);

        record_document_parse_run(&conn, "rep-2", "2026-06-15T09:00:00+00:00", 4, 30_000).unwrap();
        assert_eq!(truncation_stats(&conn).unwrap().unaligned_truncations, 0);
    }

    #[test]
    fn truncation_stats_flags_a_parse_run_with_no_original_char_count() {
        // A raw insert omitting the column (the production writer always supplies
        // it) simulates the NULL cohort.
        let conn = mem();
        conn.execute(
            "INSERT INTO document_parse_runs (report_id, captured_at, docs_parsed)
             VALUES ('legacy', '2026-06-01T09:00:00+00:00', 6)",
            [],
        )
        .unwrap();
        record_document_parse_run(&conn, "rep-2", "2026-06-08T09:00:00+00:00", 4, 50_000).unwrap();

        let stats = truncation_stats(&conn).unwrap();
        assert_eq!(stats.total_docs_parsed, 10);
        assert_eq!(stats.total_original_chars, 50_000);
        assert_eq!(stats.parse_runs_missing_original_chars, 1);

        conn.execute(
            "UPDATE document_parse_runs SET total_original_chars = 30000 WHERE report_id = 'legacy'",
            [],
        )
        .unwrap();
        let stats = truncation_stats(&conn).unwrap();
        assert_eq!(stats.total_original_chars, 80_000);
        assert_eq!(stats.parse_runs_missing_original_chars, 0);
    }

    #[test]
    fn init_schema_adds_total_original_chars_to_a_preexisting_parse_runs_table() {
        // The old table shape, with a row.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE document_parse_runs (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                report_id   TEXT NOT NULL,
                captured_at TEXT NOT NULL,
                docs_parsed INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO document_parse_runs (report_id, captured_at, docs_parsed)
             VALUES ('old', '2026-06-01T09:00:00+00:00', 9)",
            [],
        )
        .unwrap();

        assert!(!column_exists(&conn, "document_parse_runs", "total_original_chars").unwrap());
        init_schema(&conn).unwrap();
        assert!(column_exists(&conn, "document_parse_runs", "total_original_chars").unwrap());
        let legacy: Option<i64> = conn
            .query_row(
                "SELECT total_original_chars FROM document_parse_runs WHERE report_id = 'old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(legacy, None);

        // A second init_schema must not error on the duplicate column.
        init_schema(&conn).unwrap();
    }
}
