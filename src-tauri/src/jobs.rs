//! Job lifecycle: the application-layer orchestration that wraps a single report
//! run with the four-state contract from `docs/scheduling.md §Job States`.
//!
//! This module owns the *lifecycle*, distinct from `pipeline`, which owns
//! producing one report. `run_job` enforces the single-workflow-at-a-time guard
//! (`docs/scheduling.md §Concurrent Job Protection`), records every outcome to
//! the `job_runs` table, and surfaces a failed run into the Persistent Warning
//! Area (`docs/interface.md`). Like `pipeline`, it is Tauri-free so it can be
//! driven directly by an integration test against a stub agent — the Tauri
//! command is only a thin async wrapper that resolves paths and shares the guard.
//!
//! Scope note (scheduler slice 1): this slice produces Successful / Failed /
//! Skipped on the *manual* run path. The Sunday-9AM timer, the tray runtime,
//! missed-job detection, and `MissedScheduledJob` production are the next slice.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::agent::MainAgent;
use crate::config::{WarningCategory, WarningKind};
use crate::data_sources::MarketDataSource;
use crate::pipeline::{generate_report, GeneratedReport, ReportPaths};
use crate::progress::RunContext;
use crate::storage;

/// The only job type today; the schema carries it so additional recurring jobs
/// can share the table later.
const WEEKLY_MARKET_JOB: &str = "weekly_market";

/// `app_settings` key for the weekly job's enable/disable flag.
const WEEKLY_JOB_ENABLED_KEY: &str = "weekly_job_enabled";

/// Reason recorded and returned when a run is rejected by the concurrency guard.
const SKIP_REASON: &str = "another report run is already in progress";

/// Human title for the run tracker, emitted as the run starts.
const RUN_LABEL: &str = "Weekly market report";

/// Whether the Weekly Market job is enabled. Enabled by default
/// (`docs/scheduling.md §Job Controls`): an absent setting reads as `true`, so a
/// fresh install schedules the job without the user opting in. Only an explicit
/// "false" disables it.
pub fn weekly_job_enabled(conn: &Connection) -> Result<bool> {
    Ok(storage::get_setting(conn, WEEKLY_JOB_ENABLED_KEY)?.as_deref() != Some("false"))
}

/// Persist the Weekly Market job's enable/disable flag.
pub fn set_weekly_job_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    storage::set_setting(conn, WEEKLY_JOB_ENABLED_KEY, if enabled { "true" } else { "false" })
}

/// How a job run ended (`docs/scheduling.md §Job States`). `Missed` is modeled by
/// the scheduler slice that owns scheduled-window detection, not here, so it is
/// intentionally absent from this enum until that slice lands. `Cancelled` is the
/// user-initiated stop from the live run tracker — recorded like `Skipped` (it never
/// produced a report and never raises a failed-job warning), but kept distinct so the
/// status panel and history can tell an aborted run from a concurrency skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Successful,
    Failed,
    Skipped,
    Cancelled,
}

impl JobState {
    /// The canonical lowercase label persisted in the `job_runs.state` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Successful => "successful",
            JobState::Failed => "failed",
            JobState::Skipped => "skipped",
            JobState::Cancelled => "cancelled",
        }
    }
}

/// One job-run row to persist. Timestamps are RFC3339; `report_id` is set only on
/// a successful run, `detail` carries the failure message or skip reason.
pub struct JobRun<'a> {
    pub job_type: &'a str,
    pub state: JobState,
    pub started_at: &'a str,
    pub finished_at: &'a str,
    pub report_id: Option<&'a str>,
    pub detail: Option<&'a str>,
}

/// The result of `run_job`. Carries the report on success and the human-readable
/// message on failure / skip, so the command layer can map it straight to the
/// frontend's `Result<GeneratedReport, String>` contract. The success payload is
/// boxed: `GeneratedReport` dwarfs the two `String` variants, so without
/// indirection every `JobOutcome` would be sized to the largest variant.
#[derive(Debug)]
pub enum JobOutcome {
    Successful(Box<GeneratedReport>),
    Failed(String),
    Skipped(String),
    Cancelled(String),
}

/// The single-workflow-at-a-time guard. A shared atomic flag (cloneable via the
/// inner `Arc`) so the Tauri command can `manage` one instance and hand a clone
/// to each blocking run. The atomic check-and-set is what makes two racing runs
/// resolve to exactly one runner and one skip.
#[derive(Clone, Default)]
pub struct RunGuard(Arc<AtomicBool>);

impl RunGuard {
    /// Try to claim the single run slot. Returns a `RunToken` held for the run's
    /// duration when the slot was free, or `None` when a run is already in
    /// flight. Releasing happens on the token's `Drop`, so success, failure, and
    /// panic all free the slot.
    pub fn try_begin(&self) -> Option<RunToken> {
        match self
            .0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Some(RunToken(self.0.clone())),
            Err(_) => None,
        }
    }

    /// Whether a run currently holds the slot. A relaxed load is enough: this is
    /// a best-effort status read for the UI, not a synchronization point.
    pub fn is_running(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Held for the duration of a run; clears the guard flag on drop.
pub struct RunToken(Arc<AtomicBool>);

impl Drop for RunToken {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Run one job end to end with the lifecycle contract: claim the concurrency
/// slot (or record a Skipped run and return), produce the report, then record
/// the Successful or Failed outcome. Returns `Err` only on an infrastructure
/// failure (the database itself) — a failed *report* is a normal `Ok(Failed)`
/// outcome, not an error.
///
/// The schema is initialized up front so the Skipped and Failed paths — which
/// may short-circuit before `generate_report` touches the database — still have
/// `job_runs` (and `reports`) to write to.
pub fn run_job(
    agent: &dyn MainAgent,
    data: &dyn MarketDataSource,
    paths: &ReportPaths,
    guard: &RunGuard,
    ctx: &RunContext,
) -> Result<JobOutcome> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;

    // Claim the single run slot. `_token` is held until this function returns;
    // its Drop frees the slot after the outcome is recorded.
    let _token = match guard.try_begin() {
        Some(t) => t,
        None => {
            let now = now_rfc3339();
            record_run(
                &conn,
                &JobRun {
                    job_type: WEEKLY_MARKET_JOB,
                    state: JobState::Skipped,
                    started_at: &now,
                    finished_at: &now,
                    report_id: None,
                    detail: Some(SKIP_REASON),
                },
            )?;
            return Ok(JobOutcome::Skipped(SKIP_REASON.to_string()));
        }
    };

    // The run owns the slot now. Reset the cancel flag and open the tracker's lifecycle
    // here — *after* the guard claim — so a concurrency-skipped run (which returned
    // above) never emits a `run-started` and never resets the cancellation of the run
    // that actually holds the slot.
    ctx.reset_cancel();
    ctx.run_started(RUN_LABEL);

    let started_at = now_rfc3339();
    match generate_report(agent, data, paths, ctx) {
        Ok(report) => {
            let finished_at = now_rfc3339();
            // Emit the terminal tracker event even if the job-history write fails, so a
            // (rare) database error can't leave the UI stuck showing a run in progress.
            // The DB error still propagates after, as the infrastructure failure it is.
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: WEEKLY_MARKET_JOB,
                    state: JobState::Successful,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: Some(&report.report_id),
                    detail: None,
                },
            );
            ctx.run_finished("successful", None, Some(report.report_id.clone()));
            recorded?;
            Ok(JobOutcome::Successful(Box::new(report)))
        }
        // A cancel requested mid-run surfaces as an error from `generate_report`; the
        // shared cancel flag tells a user-initiated stop apart from a genuine failure.
        // A cancel that lands after the report was already persisted is honored as the
        // Ok(report) above — the work is done — so this branch only fires on a true
        // mid-run stop.
        Err(_) if ctx.is_cancelled() => {
            let finished_at = now_rfc3339();
            let detail = "run cancelled by user".to_string();
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: WEEKLY_MARKET_JOB,
                    state: JobState::Cancelled,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: None,
                    detail: Some(&detail),
                },
            );
            ctx.run_finished("cancelled", Some(detail.clone()), None);
            recorded?;
            Ok(JobOutcome::Cancelled(detail))
        }
        Err(e) => {
            let finished_at = now_rfc3339();
            let msg = e.to_string();
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: WEEKLY_MARKET_JOB,
                    state: JobState::Failed,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: None,
                    detail: Some(&msg),
                },
            );
            ctx.run_finished("failed", Some(msg.clone()), None);
            recorded?;
            Ok(JobOutcome::Failed(msg))
        }
    }
}

/// Insert one job-run row.
pub fn record_run(conn: &Connection, run: &JobRun) -> Result<()> {
    conn.execute(
        "INSERT INTO job_runs
            (job_type, state, started_at, finished_at, report_id, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            run.job_type,
            run.state.as_str(),
            run.started_at,
            run.finished_at,
            run.report_id,
            run.detail,
        ],
    )?;
    Ok(())
}

/// Build the Persistent Warning Area's `FailedJob` category from job history, or
/// `None` when there is nothing to warn about. The rule: look at the most recent
/// run that actually *executed* (Successful or Failed — a Skipped run never ran,
/// so it neither raises nor clears the indicator); warn only when that run
/// Failed. A later successful run therefore clears the warning. The category is
/// non-blocking (`WarningKind::FailedJob.is_blocking()` is false), so the
/// caller's `is_blocked` is unaffected.
pub fn failure_warning(conn: &Connection) -> Result<Option<WarningCategory>> {
    let latest = conn
        .query_row(
            "SELECT state, finished_at, detail FROM job_runs
             WHERE state IN ('successful', 'failed')
             ORDER BY id DESC LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;

    match latest {
        Some((state, finished_at, detail)) if state == JobState::Failed.as_str() => {
            let item = match detail {
                Some(d) => format!("{finished_at} — {d}"),
                None => format!("{finished_at} — job execution failed"),
            };
            Ok(Some(WarningCategory {
                kind: WarningKind::FailedJob,
                title: "Last job failed".to_string(),
                items: vec![item],
            }))
        }
        _ => Ok(None),
    }
}

/// Build the Persistent Warning Area's `MissedScheduledJob` category, or `None`
/// when no scheduled window was missed (`docs/scheduling.md §Missed Job
/// Detection`). A missed job is a *derived* warning, not a `job_runs` row — a
/// missed window never started, so there is nothing to record; it is computed on
/// next open by comparing job history against the most recent scheduled window.
///
/// The rule: with the job enabled, look at the most recent run that exists (any
/// state) by insertion order. If that latest run started *before* the most
/// recent Sunday-9AM window, then the window came and went with no run — missed.
/// A run at or after the window means it either fired or a later manual run has
/// since caught up, so the warning clears. No runs at all means the app was not
/// around to miss anything (a fresh install), so no warning — this deliberately
/// under-reports the install-then-immediately-miss case; a persisted last-launch
/// heartbeat would be needed to catch it. `MissedScheduledJob` is non-blocking,
/// so it never gates a run.
pub fn missed_warning<Tz>(
    conn: &Connection,
    now: DateTime<Tz>,
    enabled: bool,
) -> Result<Option<WarningCategory>>
where
    Tz: TimeZone,
    Tz::Offset: std::fmt::Display,
{
    if !enabled {
        return Ok(None);
    }
    let window = crate::schedule::previous_window_at_or_before(now);
    let window_utc = window.with_timezone(&Utc);

    let latest = conn
        .query_row(
            "SELECT started_at FROM job_runs ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    let latest_started = match latest {
        // No history: the app was never running to miss a window.
        None => return Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(&s)
            .map_err(|e| anyhow!("job_runs.started_at not RFC3339 ({s:?}): {e}"))?
            .with_timezone(&Utc),
    };

    if latest_started >= window_utc {
        // A run started at or after the most recent window — it fired, or a
        // later manual run has caught up. Nothing missed.
        return Ok(None);
    }

    let item = format!(
        "The scheduled run for {} did not start.",
        window.format("%Y-%m-%d %H:%M")
    );
    Ok(Some(WarningCategory {
        kind: WarningKind::MissedScheduledJob,
        title: "Scheduled job missed".to_string(),
        items: vec![item],
    }))
}

/// A snapshot of job status for the UI (`docs/scheduling.md §Job Status
/// Visibility`): last successful run, last failure, last skipped event, whether
/// a run is in flight now, and whether the job is enabled. Timestamps are the
/// canonical UTC RFC3339 strings; the frontend renders them in local time.
#[derive(Debug, Clone, Serialize)]
pub struct JobStatus {
    pub enabled: bool,
    pub is_running: bool,
    pub last_successful_at: Option<String>,
    pub last_failed_at: Option<String>,
    pub last_failure_detail: Option<String>,
    pub last_skipped_at: Option<String>,
    pub last_cancelled_at: Option<String>,
}

/// Assemble the current `JobStatus` from job history, the enable flag, and the
/// live run guard. Each "last X" is the most recent run of that state by
/// insertion order (`id`), independent of the others — a later failure does not
/// erase the last successful run's timestamp, and vice versa.
pub fn job_status(conn: &Connection, guard: &RunGuard) -> Result<JobStatus> {
    let last_of = |state: &str| -> Result<Option<(String, Option<String>)>> {
        Ok(conn
            .query_row(
                "SELECT finished_at, detail FROM job_runs
                 WHERE state = ?1 ORDER BY id DESC LIMIT 1",
                params![state],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()?)
    };
    let failed = last_of(JobState::Failed.as_str())?;
    Ok(JobStatus {
        enabled: weekly_job_enabled(conn)?,
        is_running: guard.is_running(),
        last_successful_at: last_of(JobState::Successful.as_str())?.map(|(at, _)| at),
        last_failed_at: failed.as_ref().map(|(at, _)| at.clone()),
        last_failure_detail: failed.and_then(|(_, detail)| detail),
        last_skipped_at: last_of(JobState::Skipped.as_str())?.map(|(at, _)| at),
        last_cancelled_at: last_of(JobState::Cancelled.as_str())?.map(|(at, _)| at),
    })
}

/// Current time as an RFC3339 string. UTC remains the canonical persisted form
/// for `job_runs` timestamps (sortable and unambiguous); local-time conversion
/// is a display concern handled at the seams that show time to the user — the
/// report filename (`pipeline`) and the frontend. The scheduled *window* is
/// computed in local time by `schedule`.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        conn
    }

    fn insert(conn: &Connection, state: JobState, detail: Option<&str>) {
        record_run(
            conn,
            &JobRun {
                job_type: WEEKLY_MARKET_JOB,
                state,
                started_at: "2026-06-01T09:00:00Z",
                finished_at: "2026-06-01T09:05:00Z",
                report_id: None,
                detail,
            },
        )
        .unwrap();
    }

    #[test]
    fn no_runs_produce_no_warning() {
        assert!(failure_warning(&mem()).unwrap().is_none());
    }

    #[test]
    fn latest_failure_surfaces_failed_job_category() {
        let conn = mem();
        insert(&conn, JobState::Failed, Some("provider unreachable"));
        let w = failure_warning(&conn).unwrap().expect("a failed-job warning");
        assert_eq!(w.kind, WarningKind::FailedJob);
        assert!(w.items[0].contains("provider unreachable"), "{:?}", w.items);
    }

    #[test]
    fn success_after_failure_clears_warning() {
        let conn = mem();
        insert(&conn, JobState::Failed, Some("boom"));
        insert(&conn, JobState::Successful, None);
        assert!(failure_warning(&conn).unwrap().is_none());
    }

    #[test]
    fn skipped_run_does_not_mask_prior_failure() {
        let conn = mem();
        insert(&conn, JobState::Failed, Some("boom"));
        insert(&conn, JobState::Skipped, Some(SKIP_REASON));
        assert!(
            failure_warning(&conn).unwrap().is_some(),
            "a skipped run never executed, so it must not clear the failure"
        );
    }

    #[test]
    fn failed_job_category_is_non_blocking() {
        // The warning must never gate a run; that is a config-gate concern.
        assert!(!WarningKind::FailedJob.is_blocking());
    }

    fn insert_at(conn: &Connection, started_at: &str) {
        record_run(
            conn,
            &JobRun {
                job_type: WEEKLY_MARKET_JOB,
                state: JobState::Successful,
                started_at,
                finished_at: started_at,
                report_id: None,
                detail: None,
            },
        )
        .unwrap();
    }

    /// A Wednesday-noon `now` whose most recent window is Sunday 2026-06-14 09:00.
    fn now_wed_after_a_window() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 17, 12, 0, 0).single().unwrap()
    }

    #[test]
    fn default_enabled_is_true_until_explicitly_disabled() {
        let conn = mem();
        assert!(weekly_job_enabled(&conn).unwrap());
        set_weekly_job_enabled(&conn, false).unwrap();
        assert!(!weekly_job_enabled(&conn).unwrap());
        set_weekly_job_enabled(&conn, true).unwrap();
        assert!(weekly_job_enabled(&conn).unwrap());
    }

    #[test]
    fn missed_when_latest_run_predates_the_window() {
        let conn = mem();
        // Last activity was the prior Sunday; the 06-14 window then had no run.
        insert_at(&conn, "2026-06-07T09:00:05+00:00");
        let w = missed_warning(&conn, now_wed_after_a_window(), true)
            .unwrap()
            .expect("a missed-window warning");
        assert_eq!(w.kind, WarningKind::MissedScheduledJob);
        assert!(w.items[0].contains("2026-06-14"), "{:?}", w.items);
    }

    #[test]
    fn not_missed_when_a_run_started_at_or_after_the_window() {
        let conn = mem();
        insert_at(&conn, "2026-06-14T09:00:05+00:00"); // the window fired
        assert!(missed_warning(&conn, now_wed_after_a_window(), true)
            .unwrap()
            .is_none());
    }

    #[test]
    fn no_history_is_not_a_missed_window() {
        // A fresh install was not around to miss anything.
        assert!(missed_warning(&mem(), now_wed_after_a_window(), true)
            .unwrap()
            .is_none());
    }

    #[test]
    fn disabled_job_never_reports_a_missed_window() {
        let conn = mem();
        insert_at(&conn, "2026-06-07T09:00:05+00:00");
        assert!(missed_warning(&conn, now_wed_after_a_window(), false)
            .unwrap()
            .is_none());
    }

    #[test]
    fn job_status_reports_last_of_each_state_independently() {
        let conn = mem();
        insert(&conn, JobState::Successful, None);
        insert(&conn, JobState::Failed, Some("provider 500"));
        insert(&conn, JobState::Skipped, Some(SKIP_REASON));
        let st = job_status(&conn, &RunGuard::default()).unwrap();
        assert!(st.enabled);
        assert!(!st.is_running);
        assert!(st.last_successful_at.is_some());
        assert!(st.last_failed_at.is_some());
        assert_eq!(st.last_failure_detail.as_deref(), Some("provider 500"));
        assert!(st.last_skipped_at.is_some());
    }

    #[test]
    fn job_status_reflects_a_held_run_guard_as_running() {
        let guard = RunGuard::default();
        let _token = guard.try_begin().unwrap();
        assert!(job_status(&mem(), &guard).unwrap().is_running);
    }
}
