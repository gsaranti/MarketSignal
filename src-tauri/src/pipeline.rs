//! Application-layer orchestration for a single manual report run.
//!
//! This is the spine the whole system is built on: the app layer drives the
//! agent stage (a pure function) and owns every side effect — the database
//! write and the canonical Markdown file. It is written free of any Tauri
//! runtime so it can be driven directly by an integration test against stubs.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::agent::{MainAgent, MainAgentInput, ReportSummary};
use crate::data_sources::MarketDataSource;
use crate::storage::{self, ReportRecord};

/// Filesystem locations a run reads and writes. Injected so tests can point at
/// temporary directories; the Tauri command resolves these from the app data
/// directory.
pub struct ReportPaths {
    pub db_path: PathBuf,
    pub reports_dir: PathBuf,
}

/// The result of a report run, returned to the caller (the Tauri command or a
/// test). Carries the Markdown so the frontend can render it immediately.
/// `Clone` so a scheduled run can hand the report to an open window via a
/// `job-finished` event (Tauri's `emit` requires `Clone`).
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedReport {
    pub report_id: String,
    pub markdown: String,
    pub markdown_path: String,
    pub summary: ReportSummary,
}

/// Run one manual report end to end: gather the baseline market-data scan (Step
/// 6), invoke the agent, write the canonical Markdown file, and persist the
/// record to SQLite. A failed baseline scan (an unreachable / rejecting data
/// provider) propagates here, which `jobs::run_job` records as a failed job
/// (`docs/scheduling.md §Offline Behavior`).
pub fn generate_report(
    agent: &dyn MainAgent,
    data: &dyn MarketDataSource,
    paths: &ReportPaths,
) -> Result<GeneratedReport> {
    // Step 6: baseline market data is gathered before agent reasoning and is not
    // optional (`docs/weekly-report-workflow.md §Step 6`). The data-source error
    // propagates unwrapped — like the agent error below — so `jobs::run_job`
    // persists the provider's own message (e.g. an FMP rejection) as the
    // failed-job detail rather than a vague outer wrapper.
    let baseline = data.baseline_scan()?;
    let output = agent.generate(MainAgentInput { baseline })?;
    let summary = output.summary;

    std::fs::create_dir_all(&paths.reports_dir)
        .with_context(|| format!("creating reports directory {:?}", paths.reports_dir))?;

    let filename =
        canonical_report_filename(&summary.created_at, &summary.report_id, &chrono::Local)?;
    let markdown_path = paths.reports_dir.join(&filename);
    std::fs::write(&markdown_path, &output.markdown)
        .with_context(|| format!("writing report markdown {:?}", markdown_path))?;
    let markdown_path_str = markdown_path.to_string_lossy().into_owned();

    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;
    let summary_json = serde_json::to_string(&summary)?;
    storage::insert_report(
        &conn,
        &ReportRecord {
            summary: &summary,
            markdown_path: &markdown_path_str,
            summary_json: &summary_json,
        },
    )
    .context("inserting report record")?;

    Ok(GeneratedReport {
        report_id: summary.report_id.clone(),
        markdown: output.markdown,
        markdown_path: markdown_path_str,
        summary,
    })
}

/// List the most recent reports (newest first), capped at the retention display
/// limit. The Tauri `list_reports` command is a thin wrapper over this.
pub fn list_reports(paths: &ReportPaths) -> Result<Vec<ReportSummary>> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;
    storage::list_recent_reports(&conn, storage::RECENT_REPORTS_LIMIT)
}

/// Load one persisted report by id for display: its summary from SQLite and its
/// canonical Markdown read back from disk. An unknown id, or a Markdown file
/// removed out-of-band, surfaces as a typed error the UI renders rather than a
/// panic.
pub fn load_report(paths: &ReportPaths, report_id: &str) -> Result<GeneratedReport> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;
    let (markdown_path, summary) = storage::get_report_record(&conn, report_id)?
        .with_context(|| format!("no report with id {report_id}"))?;
    let markdown = std::fs::read_to_string(&markdown_path)
        .with_context(|| format!("reading report markdown {markdown_path:?}"))?;
    Ok(GeneratedReport {
        report_id: summary.report_id.clone(),
        markdown,
        markdown_path,
        summary,
    })
}

/// Export one report's canonical Markdown to a user-chosen destination
/// (`docs/export.md`). Tauri-free so it sits behind the thin `export_report_markdown`
/// command the same way `list_reports`/`load_report` do, and is driveable from a
/// test against temp dirs.
///
/// Reads from the stored artifacts only — the canonical `.md` on disk, located via
/// the SQLite record — so an export never re-runs the workflow and never trusts an
/// in-memory copy (`docs/export.md §Export Behavior`). An unknown id, a Markdown
/// file removed out-of-band, or a write failure surfaces as a typed error rather
/// than a panic, mirroring `load_report`.
pub fn export_markdown_to(
    paths: &ReportPaths,
    report_id: &str,
    dest: &std::path::Path,
) -> Result<()> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;
    let (markdown_path, _summary) = storage::get_report_record(&conn, report_id)?
        .with_context(|| format!("no report with id {report_id}"))?;
    let markdown = std::fs::read_to_string(&markdown_path)
        .with_context(|| format!("reading report markdown {markdown_path:?}"))?;
    std::fs::write(dest, &markdown)
        .with_context(|| format!("writing exported markdown {dest:?}"))?;
    Ok(())
}

/// Build the canonical Markdown filename for a report:
/// `YYYY-MM-DD-market-signal-weekly-report-<id8>.md`.
///
/// Split out as a pure, timezone-injectable function so the two decisions it
/// encodes are unit-testable without the system clock or the filesystem:
///
/// - **Local date segment.** The report is a local-time artifact (the scheduled
///   window is Sunday 9 AM local — see `docs/scheduling.md`), so the filename a
///   user reads matches their wall clock, even though the `created_at` persisted
///   in SQLite stays canonical UTC. `tz` names the zone whose calendar date
///   labels the file; production passes `chrono::Local`, tests pass a fixed
///   offset so the midnight-boundary behavior is deterministic.
/// - **Unique per-run suffix.** The first 8 characters of the `report_id` UUID
///   make every run's file distinct, so a same-date rerun no longer overwrites
///   an earlier run's Markdown (the two-rows-one-file case from slice 1).
///
/// A non-RFC3339 `created_at` surfaces as a typed error here rather than
/// panicking on a byte slice.
fn canonical_report_filename<Tz: chrono::TimeZone>(
    created_at: &str,
    report_id: &str,
    tz: &Tz,
) -> Result<String>
where
    Tz::Offset: std::fmt::Display,
{
    let local_date = local_date_segment(created_at, tz)?;
    let id8 = report_id.get(..8).unwrap_or(report_id);
    Ok(format!("{local_date}-market-signal-weekly-report-{id8}.md"))
}

/// Build the export filename a user sees in the Save dialog
/// (`docs/export.md §Export Naming`): `YYYY-MM-DD-market-signal-weekly-report.<ext>`.
///
/// Deliberately distinct from `canonical_report_filename`: the spec's export name
/// carries **no `-<id8>` suffix** — same-name collisions are the user's own save
/// dialog overwrite prompt, not ours. `ext` is the bare extension (`"md"`, `"pdf"`).
/// Shares the local-date logic so an export's date matches the stored file's date.
pub fn export_basename<Tz: chrono::TimeZone>(
    created_at: &str,
    ext: &str,
    tz: &Tz,
) -> Result<String>
where
    Tz::Offset: std::fmt::Display,
{
    let local_date = local_date_segment(created_at, tz)?;
    Ok(format!("{local_date}-market-signal-weekly-report.{ext}"))
}

/// The `YYYY-MM-DD` local-date segment shared by the canonical filename and the
/// export basename: parse the canonical-UTC `created_at` and render it in `tz`'s
/// calendar (`docs/scheduling.md` — reports are local-time artifacts). A
/// non-RFC3339 stamp is a typed error, not a panic on a byte slice.
fn local_date_segment<Tz: chrono::TimeZone>(created_at: &str, tz: &Tz) -> Result<String>
where
    Tz::Offset: std::fmt::Display,
{
    Ok(chrono::DateTime::parse_from_rfc3339(created_at)
        .with_context(|| format!("agent supplied a non-RFC3339 created_at: {created_at:?}"))?
        .with_timezone(tz)
        .format("%Y-%m-%d")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    /// UTC-3 so a late-evening UTC stamp lands on the *previous* local day —
    /// pins that the filename date is the local calendar date, not the UTC one.
    fn minus_three() -> FixedOffset {
        FixedOffset::west_opt(3 * 3600).unwrap()
    }

    #[test]
    fn filename_uses_local_calendar_date_across_a_midnight_boundary() {
        // 01:30 UTC on the 3rd is 22:30 on the 2nd at UTC-3.
        let name =
            canonical_report_filename("2026-06-03T01:30:00Z", "1ca71d1f-aaaa", &minus_three())
                .unwrap();
        assert!(
            name.starts_with("2026-06-02-market-signal-weekly-report-"),
            "expected the local (UTC-3) date 2026-06-02, got {name}"
        );
    }

    #[test]
    fn same_date_distinct_report_ids_produce_distinct_filenames() {
        let tz = minus_three();
        let a = canonical_report_filename("2026-06-03T12:00:00Z", "aaaaaaaa-1111", &tz).unwrap();
        let b = canonical_report_filename("2026-06-03T15:00:00Z", "bbbbbbbb-2222", &tz).unwrap();
        assert_ne!(a, b, "same-date reruns must not collide on one filename");
        assert_eq!(a, "2026-06-03-market-signal-weekly-report-aaaaaaaa.md");
        assert_eq!(b, "2026-06-03-market-signal-weekly-report-bbbbbbbb.md");
    }

    #[test]
    fn short_report_id_does_not_panic_on_the_eight_char_slice() {
        // A test-style id shorter than 8 chars must not panic on `get(..8)`.
        let name = canonical_report_filename("2026-06-03T12:00:00Z", "rid", &minus_three()).unwrap();
        assert!(name.ends_with("-rid.md"), "got {name}");
    }

    #[test]
    fn non_rfc3339_created_at_is_a_typed_error() {
        let err = canonical_report_filename("not-a-timestamp", "rid", &minus_three()).unwrap_err();
        assert!(err.to_string().contains("non-RFC3339"), "{err}");
    }

    #[test]
    fn export_basename_has_no_id_suffix_and_carries_the_extension() {
        // The spec's export name (docs/export.md §Export Naming) is suffix-free,
        // distinct from the internal canonical filename's `-<id8>` segment.
        let md = export_basename("2026-06-03T12:00:00Z", "md", &minus_three()).unwrap();
        assert_eq!(md, "2026-06-03-market-signal-weekly-report.md");
        let pdf = export_basename("2026-06-03T12:00:00Z", "pdf", &minus_three()).unwrap();
        assert_eq!(pdf, "2026-06-03-market-signal-weekly-report.pdf");
    }

    #[test]
    fn export_basename_uses_local_calendar_date_across_a_midnight_boundary() {
        // Shares local_date_segment with the canonical filename: 01:30 UTC on the
        // 3rd is 22:30 on the 2nd at UTC-3, so the export date is the local one.
        let name = export_basename("2026-06-03T01:30:00Z", "md", &minus_three()).unwrap();
        assert_eq!(name, "2026-06-02-market-signal-weekly-report.md");
    }

    #[test]
    fn export_basename_non_rfc3339_created_at_is_a_typed_error() {
        let err = export_basename("not-a-timestamp", "md", &minus_three()).unwrap_err();
        assert!(err.to_string().contains("non-RFC3339"), "{err}");
    }
}
