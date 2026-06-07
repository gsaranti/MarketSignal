//! Application-layer orchestration for a single manual report run.
//!
//! This is the spine the whole system is built on: the app layer drives the
//! agent stage (a pure function) and owns every side effect — the database
//! write and the canonical Markdown file. It is written free of any Tauri
//! runtime so it can be driven directly by an integration test against stubs.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::agent::{MainAgent, MainAgentInput, ReportSummary};
use crate::data_sources::{BaselineMarketData, GroupKind, MarketDataSource};
use crate::progress::RunContext;
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
/// 6), enforce its coverage floor, invoke the agent, write the canonical Markdown
/// file, and persist the record to SQLite. The adapters degrade partial failures to
/// a recorded gap manifest rather than aborting, so the scan itself rarely errs; the
/// `enforce_coverage` gate below is the single place a too-thin baseline fails the
/// run, which `jobs::run_job` records as a failed job (`docs/scheduling.md §Offline
/// Behavior`).
///
/// `ctx` is the run's progress/cancel context (`progress::RunContext`). Each of the
/// four stages below brackets its work in a `step_started` / `step_finished` pair so
/// an open window can render the run live; the real data adapters and the streaming
/// model adapter emit their own finer-grained events *inside* the baseline and agent
/// steps. A cancel requested mid-run is observed at the boundaries between stages
/// (`bail_if_cancelled`) — a `reqwest::blocking` call already in flight is not
/// interrupted — and surfaces as a recognizable error that `jobs::run_job` maps to a
/// Cancelled outcome. Tests and offline smokes pass `RunContext::noop()`, which
/// reports nowhere and is never cancelled, so this stays the same pure spine.
pub fn generate_report(
    agent: &dyn MainAgent,
    data: &dyn MarketDataSource,
    paths: &ReportPaths,
    ctx: &RunContext,
) -> Result<GeneratedReport> {
    // Step 6: baseline market data is gathered before agent reasoning and is not
    // optional (`docs/weekly-report-workflow.md §Step 6`). Each adapter records what it
    // couldn't resolve in `baseline.gaps` instead of failing; the coverage gate then
    // decides whether what landed clears the run's mandatory floor. A gate failure
    // propagates unwrapped — like the agent error below — so `jobs::run_job` persists
    // the floor message as the failed-job detail. The gaps that *don't* trip the floor
    // ride into `MainAgentInput` and on into the model prompt, so the agent reasons over
    // what's known-absent rather than inferring it.
    ctx.step_started("baseline", "Gathering baseline market data");
    let baseline = match data.baseline_scan() {
        Ok(baseline) => baseline,
        Err(e) => {
            ctx.step_finished("baseline", "failed", Some(e.to_string()));
            return Err(e);
        }
    };
    ctx.step_finished("baseline", "ok", None);
    bail_if_cancelled(ctx)?;

    ctx.step_started("coverage", "Checking baseline coverage");
    if let Err(e) = enforce_coverage(&baseline) {
        ctx.step_finished("coverage", "failed", Some(e.to_string()));
        return Err(e);
    }
    ctx.step_finished("coverage", "ok", None);
    bail_if_cancelled(ctx)?;

    ctx.step_started("agent", "Main agent writing the report");
    let output = match agent.generate(MainAgentInput { baseline }) {
        Ok(output) => output,
        Err(e) => {
            // A cancel observed mid-stream surfaces as a parse error here; mark the
            // step cancelled (not failed) so the tracker reads consistently with the
            // run's Cancelled outcome.
            let status = if ctx.is_cancelled() { "cancelled" } else { "failed" };
            ctx.step_finished("agent", status, Some(e.to_string()));
            return Err(e);
        }
    };
    ctx.step_finished("agent", "ok", None);
    bail_if_cancelled(ctx)?;

    // Persist: write the canonical Markdown file and the SQLite record. The `?`
    // ergonomics stay inside an immediately-invoked closure so any failure still
    // flips the persist step to `failed` with its reason.
    ctx.step_started("persist", "Saving the report");
    let report = (|| -> Result<GeneratedReport> {
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
    })();
    match &report {
        Ok(_) => ctx.step_finished("persist", "ok", None),
        Err(e) => ctx.step_finished("persist", "failed", Some(e.to_string())),
    }
    report
}

/// Cancel checkpoint between pipeline stages. A cancel requested mid-run lands
/// here: the run stops with a recognizable error that `jobs::run_job` reclassifies
/// as a Cancelled outcome (keyed off the same shared cancel flag) rather than a
/// Failed one. An in-flight HTTP request is never interrupted — the cancel takes
/// effect at the next boundary.
fn bail_if_cancelled(ctx: &RunContext) -> Result<()> {
    if ctx.is_cancelled() {
        bail!("run cancelled before completion");
    }
    Ok(())
}

/// The minimum fraction of a Step-6 group's *expected* series that must resolve for the
/// group to count as present for the coverage floor. "Expected" excludes permanently
/// out-of-scope items (premium / discontinued series), so a series a deployment never had
/// doesn't drag the ratio down — only this-run failures (`Unavailable` / `Rejected` /
/// `Malformed` gaps) count against it. 0.6 ≈ three of four indices, or ~60% of a posture
/// group.
const COVERAGE_FLOOR: f64 = 0.6;

/// One group's coverage: resolved series over expected (resolved + this-run gaps).
/// Permanent (`OutOfScope`) gaps are excluded from the denominator. An expected count of
/// zero reads as fully covered, but `clears_floor` additionally requires a non-empty
/// group, so a vacuous 1.0 over zero series can't satisfy the floor.
fn group_coverage(data: &BaselineMarketData, group: GroupKind) -> f64 {
    let present = group_present_count(data, group);
    let missing_this_run = data
        .gaps
        .iter()
        .filter(|g| g.group == group && g.reason.counts_against_coverage())
        .count();
    let expected = present + missing_this_run;
    if expected == 0 {
        1.0
    } else {
        present as f64 / expected as f64
    }
}

/// How many series resolved in `group`.
fn group_present_count(data: &BaselineMarketData, group: GroupKind) -> usize {
    match group {
        GroupKind::Indices => data.indices.len(),
        GroupKind::Internals => data.internals.len(),
        GroupKind::Sectors => data.sectors.len(),
        GroupKind::MacroLevels => data.macro_levels.len(),
        GroupKind::LaborLevels => data.labor_levels.len(),
        GroupKind::Calendar => data.calendar.len(),
        GroupKind::IndexPerformance => data.index_performance.len(),
        GroupKind::Movers => data.movers.len(),
        GroupKind::Earnings => data.earnings.len(),
    }
}

/// Whether `group` clears the coverage floor: at least one resolved series *and* a
/// coverage ratio of at least `COVERAGE_FLOOR`. The non-empty requirement guards the
/// vacuous "0 of 0 = 100%" case — a floor group always has a fixed expected set, so zero
/// resolved is a failure regardless of the ratio.
fn clears_floor(data: &BaselineMarketData, group: GroupKind) -> bool {
    group_present_count(data, group) > 0 && group_coverage(data, group) >= COVERAGE_FLOOR
}

/// The Step-6 coverage gate — the single, centralized replacement for the per-adapter
/// completeness floors. The adapters now degrade every failure to a recorded gap; this
/// is the one place a too-thin merged baseline fails the run. The floor (resolved in
/// planning): the report is structurally impossible without the Index Picture, and its
/// risk-posture / market-cycle reads need at least one grounded macro/internals group —
/// so `indices` must clear, AND at least one of {`internals`, `macro_levels`} must clear.
/// Everything else (sectors, labor, calendar, index performance, and whichever of
/// internals/macro didn't clear) degrades to a manifest gap the agent reasons over. A
/// failure here propagates like any Step-6 error, which `jobs::run_job` records as a
/// failed job.
pub fn enforce_coverage(data: &BaselineMarketData) -> Result<()> {
    if !clears_floor(data, GroupKind::Indices) {
        bail!(
            "Step-6 baseline below floor: the index picture is missing — indices coverage \
             {:.0}% (need {:.0}%). The report can't be written without the Dow / S&P / Nasdaq \
             reads; the data provider is unreachable, rejecting the key, or rate-limited.",
            group_coverage(data, GroupKind::Indices) * 100.0,
            COVERAGE_FLOOR * 100.0
        );
    }
    if !clears_floor(data, GroupKind::Internals) && !clears_floor(data, GroupKind::MacroLevels) {
        bail!(
            "Step-6 baseline below floor: neither market internals ({:.0}%) nor macro levels \
             ({:.0}%) reached {:.0}% — the risk-posture / market-cycle reads would be \
             ungrounded. FRED is unreachable, rejecting the key, or rate-limited.",
            group_coverage(data, GroupKind::Internals) * 100.0,
            group_coverage(data, GroupKind::MacroLevels) * 100.0,
            COVERAGE_FLOOR * 100.0
        );
    }
    Ok(())
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

    use crate::data_sources::{DataGap, GapReason, Quote};

    // ---- Step-6 coverage gate ----

    /// `n` placeholder resolved quotes for a group.
    fn covq(n: usize) -> Vec<Quote> {
        (0..n)
            .map(|i| Quote {
                symbol: format!("S{i}"),
                name: format!("S{i}"),
                price: 1.0,
                change_pct: 0.0,
                unit: "x".into(),
            })
            .collect()
    }

    /// `n` gaps of one reason for `group`.
    fn covgaps(group: GroupKind, reason: GapReason, n: usize) -> Vec<DataGap> {
        (0..n)
            .map(|_| DataGap::new(group, "id", "name", reason))
            .collect()
    }

    #[test]
    fn enforce_coverage_passes_a_full_baseline() {
        let data = BaselineMarketData {
            indices: covq(4),
            internals: covq(9),
            macro_levels: covq(17),
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_fails_without_the_index_picture() {
        // Every index gapped (a rejected FMP key) -> the report can't be written.
        let data = BaselineMarketData {
            internals: covq(9),
            macro_levels: covq(17),
            gaps: covgaps(GroupKind::Indices, GapReason::Unavailable, 4),
            ..Default::default()
        };
        let err = enforce_coverage(&data).unwrap_err().to_string();
        assert!(err.contains("index picture"), "{err}");
    }

    #[test]
    fn enforce_coverage_fails_without_posture_grounding() {
        // Indices present, but both internals and macro fully gone (a rejected FRED key)
        // -> risk-posture / market-cycle would be ungrounded.
        let mut gaps = covgaps(GroupKind::Internals, GapReason::Rejected, 9);
        gaps.extend(covgaps(GroupKind::MacroLevels, GapReason::Rejected, 17));
        let data = BaselineMarketData {
            indices: covq(4),
            gaps,
            ..Default::default()
        };
        let err = enforce_coverage(&data).unwrap_err().to_string();
        assert!(err.contains("macro levels") && err.contains("internals"), "{err}");
    }

    #[test]
    fn enforce_coverage_passes_when_one_posture_group_degrades_above_floor() {
        // Internals 6 of 9 (0.67 >= floor) clears even though macro is gone.
        let mut gaps = covgaps(GroupKind::Internals, GapReason::Unavailable, 3);
        gaps.extend(covgaps(GroupKind::MacroLevels, GapReason::Unavailable, 17));
        let data = BaselineMarketData {
            indices: covq(4),
            internals: covq(6),
            gaps,
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_passes_on_macro_when_internals_is_gone() {
        // Internals fully gapped but macro intact -> the posture-grounding clause is met
        // by macro alone.
        let data = BaselineMarketData {
            indices: covq(4),
            macro_levels: covq(17),
            gaps: covgaps(GroupKind::Internals, GapReason::Unavailable, 9),
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_excludes_out_of_scope_from_the_floor() {
        // Russell permanently premium: 3 resolved + 1 OutOfScope gap -> 3/3 = 100%, so a
        // permanent absence never fails the floor (the whole reason OutOfScope is split
        // from the this-run reasons).
        let data = BaselineMarketData {
            indices: covq(3),
            internals: covq(9),
            macro_levels: covq(17),
            gaps: covgaps(GroupKind::Indices, GapReason::OutOfScope, 1),
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_ignores_additive_movers_and_earnings_groups() {
        // movers / earnings are additive, non-floor groups: empty groups and even a
        // this-run gap in them never gate a run whose floor groups (indices + a posture
        // group) are covered.
        let mut gaps = covgaps(GroupKind::Movers, GapReason::Unavailable, 3);
        gaps.extend(covgaps(GroupKind::Earnings, GapReason::Rejected, 1));
        let data = BaselineMarketData {
            indices: covq(4),
            internals: covq(9),
            // movers / earnings deliberately left empty
            gaps,
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_fails_when_indices_below_floor() {
        // 2 resolved + 2 this-run gaps = 50% < 60%.
        let data = BaselineMarketData {
            indices: covq(2),
            internals: covq(9),
            macro_levels: covq(17),
            gaps: covgaps(GroupKind::Indices, GapReason::Unavailable, 2),
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_err());
    }

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
