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

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::agent::MainAgent;
use crate::config::{WarningCategory, WarningKind};
use crate::pipeline::{generate_report, GeneratedReport, ReportPaths};
use crate::storage;

/// The only job type today; the schema carries it so additional recurring jobs
/// can share the table later.
const WEEKLY_MARKET_JOB: &str = "weekly_market";

/// Reason recorded and returned when a run is rejected by the concurrency guard.
const SKIP_REASON: &str = "another report run is already in progress";

/// How a job run ended (`docs/scheduling.md §Job States`). `Missed` is modeled by
/// the scheduler slice that owns scheduled-window detection, not here, so it is
/// intentionally absent from this enum until that slice lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Successful,
    Failed,
    Skipped,
}

impl JobState {
    /// The canonical lowercase label persisted in the `job_runs.state` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Successful => "successful",
            JobState::Failed => "failed",
            JobState::Skipped => "skipped",
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
    paths: &ReportPaths,
    guard: &RunGuard,
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

    let started_at = now_rfc3339();
    match generate_report(agent, paths) {
        Ok(report) => {
            let finished_at = now_rfc3339();
            record_run(
                &conn,
                &JobRun {
                    job_type: WEEKLY_MARKET_JOB,
                    state: JobState::Successful,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: Some(&report.report_id),
                    detail: None,
                },
            )?;
            Ok(JobOutcome::Successful(Box::new(report)))
        }
        Err(e) => {
            let finished_at = now_rfc3339();
            let msg = e.to_string();
            record_run(
                &conn,
                &JobRun {
                    job_type: WEEKLY_MARKET_JOB,
                    state: JobState::Failed,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: None,
                    detail: Some(&msg),
                },
            )?;
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

/// Current time as an RFC3339 string. UTC, consistent with the report
/// `created_at` convention; the local-vs-UTC decision is deferred to the
/// scheduler slice that introduces the local-time scheduled window.
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
}
