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
//! Report generation is on demand only — there is no scheduler, so this module
//! produces just the four manual-run states (Successful / Failed / Skipped /
//! Cancelled); there is no missed-job detection or `MissedScheduledJob` warning.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::agent::MainAgent;
use crate::config::{WarningCategory, WarningKind};
use crate::data_sources::MarketDataSource;
use crate::embedding::Embedder;
use crate::pipeline::{
    generate_report, AnalystStages, GeneratedReport, ReportPaths, ResearchStages,
};
use crate::progress::RunContext;
use crate::storage;

/// The only job type today; the schema carries it so additional recurring jobs
/// can share the table later.
const MARKET_SIGNAL_JOB: &str = "market_signal";

/// `app_settings` key recording a dismissed Persistent Warning Area warning
/// (`docs/interface.md §Persistent Warning Area` — "Dismissing a warning
/// permanently removes it. A subsequent event in the same category produces a fresh
/// warning."). Stores the *identity* of the dismissed warning — the failed run's
/// `job_runs.id` — so a later, distinct failure re-surfaces. Only the non-blocking
/// failed-job category is dismissible; the blocking configuration gaps are gate
/// state, not notices.
const DISMISSED_FAILED_JOB_RUN_ID_KEY: &str = "dismissed_failed_job_run_id";

/// Reason recorded and returned when a run is rejected by the concurrency guard.
const SKIP_REASON: &str = "another report run is already in progress";

/// Human title for the run tracker, emitted as the run starts.
const RUN_LABEL: &str = "Market Signal report";

/// How a job run ended (`docs/scheduling.md §Job States`). There is no `Missed`
/// state — report generation is on demand, so a report is never "due" while
/// unattended. `Cancelled` is the user-initiated stop from the live run tracker —
/// recorded like `Skipped` (it never produced a report and never raises a
/// failed-job warning), but kept distinct so the status panel and history can tell
/// an aborted run from a concurrency skip.
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

/// What kind of workflow holds the single run slot. Carried by the guard and
/// surfaced through `JobStatus`, so the footer can label the in-flight work
/// honestly — a Schwab connect or a Portfolio run must never read as a report
/// generation. Kebab-case on the wire; mirrored in the frontend `JobStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunKind {
    Report,
    Portfolio,
    SchwabConnect,
}

/// The single-workflow-at-a-time guard. A shared slot (cloneable via the inner
/// `Arc`) so the Tauri command can `manage` one instance and hand a clone to
/// each blocking run. The locked check-and-set is what makes two racing runs
/// resolve to exactly one runner and one skip; the slot remembers *which* kind
/// of workflow claimed it, for the status read.
#[derive(Clone, Default)]
pub struct RunGuard(Arc<Mutex<Option<RunKind>>>);

impl RunGuard {
    /// Try to claim the single run slot for a `kind` of workflow. Returns a
    /// `RunToken` held for the run's duration when the slot was free, or `None`
    /// when a run is already in flight. Releasing happens on the token's `Drop`,
    /// so success, failure, and panic all free the slot.
    pub fn try_begin(&self, kind: RunKind) -> Option<RunToken> {
        let mut slot = self.lock();
        if slot.is_some() {
            return None;
        }
        *slot = Some(kind);
        Some(RunToken(self.0.clone()))
    }

    /// Whether a run currently holds the slot.
    pub fn is_running(&self) -> bool {
        self.running_kind().is_some()
    }

    /// The kind of workflow holding the slot right now, if any. A best-effort
    /// status read for the UI, not a synchronization point.
    pub fn running_kind(&self) -> Option<RunKind> {
        *self.lock()
    }

    /// The slot, poison-tolerant: the critical sections are plain assignments, but
    /// a panic elsewhere must never wedge the guard permanently closed (or open).
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<RunKind>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Held for the duration of a run; clears the guard slot on drop.
pub struct RunToken(Arc<Mutex<Option<RunKind>>>);

impl Drop for RunToken {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
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
#[allow(clippy::too_many_arguments)] // each is a distinct injected stage/dependency or path, documented at the call sites
pub fn run_job(
    agent: &dyn MainAgent,
    data: &dyn MarketDataSource,
    research: &ResearchStages,
    analysts: &AnalystStages,
    embedder: &dyn Embedder,
    paths: &ReportPaths,
    guard: &RunGuard,
    ctx: &RunContext,
) -> Result<JobOutcome> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;

    // Claim the single run slot. `_token` is held until this function returns;
    // its Drop frees the slot after the outcome is recorded.
    let _token = match guard.try_begin(RunKind::Report) {
        Some(t) => t,
        None => {
            let now = now_rfc3339();
            record_run(
                &conn,
                &JobRun {
                    job_type: MARKET_SIGNAL_JOB,
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
    match generate_report(agent, data, research, analysts, embedder, paths, ctx) {
        Ok(report) => {
            let finished_at = now_rfc3339();
            // Emit the terminal tracker event even if the job-history write fails, so a
            // (rare) database error can't leave the UI stuck showing a run in progress.
            // The DB error still propagates after, as the infrastructure failure it is.
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: MARKET_SIGNAL_JOB,
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
                    job_type: MARKET_SIGNAL_JOB,
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
                    job_type: MARKET_SIGNAL_JOB,
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
            "SELECT id, state, finished_at, detail FROM job_runs
             WHERE state IN ('successful', 'failed')
             ORDER BY id DESC LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;

    match latest {
        Some((id, state, finished_at, detail)) if state == JobState::Failed.as_str() => {
            // Suppressed if the user dismissed this exact failed run; a later
            // failure carries a higher `id`, so the marker no longer matches and a
            // fresh warning surfaces.
            if storage::get_setting(conn, DISMISSED_FAILED_JOB_RUN_ID_KEY)?.as_deref()
                == Some(id.to_string().as_str())
            {
                return Ok(None);
            }
            let item = match detail {
                Some(d) => format!("{finished_at} — {d}"),
                None => format!("{finished_at} — job execution failed"),
            };
            Ok(Some(WarningCategory {
                kind: WarningKind::FailedJob,
                title: "Last job failed".to_string(),
                items: vec![item],
                dismiss_id: Some(id.to_string()),
            }))
        }
        _ => Ok(None),
    }
}

/// Dismiss one Persistent Warning Area warning by the *identity the frontend
/// rendered* (`docs/interface.md §Persistent Warning Area`). `dismiss_id` is the
/// `WarningCategory.dismiss_id` echoed back from the shown row — the failed run's id
/// for `FailedJob` — written verbatim into the `app_settings` marker.
/// `failure_warning` then suppresses only the warning whose *current* identity
/// equals the marker, so dismissing a stale row records that stale identity
/// (already gone) and a later, distinct failure still surfaces fresh. Keying off
/// the rendered id — rather than re-deriving "current" at click time — is what
/// makes a stale click safe. The three blocking configuration categories are gate
/// state rather than dismissible notices, so a dismiss of one is a no-op (the UI
/// also offers no control for them).
pub fn dismiss_warning_category(conn: &Connection, kind: WarningKind, dismiss_id: &str) -> Result<()> {
    match kind {
        WarningKind::FailedJob => {
            storage::set_setting(conn, DISMISSED_FAILED_JOB_RUN_ID_KEY, dismiss_id)?;
        }
        WarningKind::AgentConfiguration
        | WarningKind::ApiTokens
        | WarningKind::ProviderCredentials
        | WarningKind::LocalModels
        | WarningKind::Schwab => {}
    }
    Ok(())
}

/// A snapshot of job status for the UI (`docs/scheduling.md §Job Status
/// Visibility`): last successful run, last failure, last skipped event, and
/// whether a run is in flight now — and, when one is, which kind of workflow
/// holds the slot, so the footer's running label matches the actual work.
/// Timestamps are the canonical UTC RFC3339 strings; the frontend renders them
/// in local time.
#[derive(Debug, Clone, Serialize)]
pub struct JobStatus {
    pub is_running: bool,
    pub running_kind: Option<RunKind>,
    pub last_successful_at: Option<String>,
    pub last_failed_at: Option<String>,
    pub last_failure_detail: Option<String>,
    pub last_skipped_at: Option<String>,
    pub last_cancelled_at: Option<String>,
}

/// Assemble the current `JobStatus` from job history and the live run guard. Each
/// "last X" is the most recent run of that state by insertion order (`id`),
/// independent of the others — a later failure does not erase the last successful
/// run's timestamp, and vice versa.
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
        is_running: guard.is_running(),
        running_kind: guard.running_kind(),
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
/// report filename (`pipeline`) and the frontend.
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
                job_type: MARKET_SIGNAL_JOB,
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
        let w = failure_warning(&conn)
            .unwrap()
            .expect("a failed-job warning");
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

    #[test]
    fn job_status_reports_last_of_each_state_independently() {
        let conn = mem();
        insert(&conn, JobState::Successful, None);
        insert(&conn, JobState::Failed, Some("provider 500"));
        insert(&conn, JobState::Skipped, Some(SKIP_REASON));
        let st = job_status(&conn, &RunGuard::default()).unwrap();
        assert!(!st.is_running);
        assert!(st.last_successful_at.is_some());
        assert!(st.last_failed_at.is_some());
        assert_eq!(st.last_failure_detail.as_deref(), Some("provider 500"));
        assert!(st.last_skipped_at.is_some());
    }

    #[test]
    fn job_status_reflects_a_held_run_guard_as_running() {
        let guard = RunGuard::default();
        let _token = guard.try_begin(RunKind::Report).unwrap();
        let st = job_status(&mem(), &guard).unwrap();
        assert!(st.is_running);
        assert_eq!(st.running_kind, Some(RunKind::Report));
    }

    #[test]
    fn run_guard_slot_is_exclusive_and_freed_on_drop() {
        let guard = RunGuard::default();
        let token = guard.try_begin(RunKind::SchwabConnect).unwrap();
        // Any second claim loses while the slot is held, regardless of kind.
        assert!(guard.try_begin(RunKind::Report).is_none());
        assert_eq!(guard.running_kind(), Some(RunKind::SchwabConnect));
        drop(token);
        assert_eq!(guard.running_kind(), None);
        assert!(guard.try_begin(RunKind::Report).is_some());
    }

    #[test]
    fn running_kind_serializes_kebab_case_for_the_frontend() {
        // Pins the wire contract the frontend `JobStatus.running_kind` mirrors.
        let guard = RunGuard::default();
        let _token = guard.try_begin(RunKind::SchwabConnect).unwrap();
        let st = job_status(&mem(), &guard).unwrap();
        let json = serde_json::to_value(&st).unwrap();
        assert_eq!(json["running_kind"], "schwab-connect");
        assert_eq!(
            serde_json::to_value(RunKind::Portfolio).unwrap(),
            "portfolio"
        );
        assert_eq!(serde_json::to_value(RunKind::Report).unwrap(), "report");
    }

    /// The rendered identity of the current warning of `kind` — the `dismiss_id` the
    /// frontend would echo back. Panics if no such warning is shown, since these
    /// tests dismiss a warning they just asserted is present.
    fn shown_dismiss_id(category: Option<WarningCategory>) -> String {
        category
            .expect("a warning is shown")
            .dismiss_id
            .expect("a dismissible warning carries an identity")
    }

    #[test]
    fn dismissing_the_failed_job_warning_suppresses_it() {
        let conn = mem();
        insert(&conn, JobState::Failed, Some("boom"));
        let id = shown_dismiss_id(failure_warning(&conn).unwrap());
        dismiss_warning_category(&conn, WarningKind::FailedJob, &id).unwrap();
        assert!(
            failure_warning(&conn).unwrap().is_none(),
            "a dismissed failure must not re-surface"
        );
    }

    #[test]
    fn a_newer_failure_resurfaces_after_dismiss() {
        let conn = mem();
        insert(&conn, JobState::Failed, Some("first"));
        let id = shown_dismiss_id(failure_warning(&conn).unwrap());
        dismiss_warning_category(&conn, WarningKind::FailedJob, &id).unwrap();
        assert!(failure_warning(&conn).unwrap().is_none());
        // A second, distinct failure (a new id) is a fresh event the marker can't match.
        insert(&conn, JobState::Failed, Some("second"));
        let w = failure_warning(&conn)
            .unwrap()
            .expect("a fresh failed-job warning");
        assert!(w.items[0].contains("second"), "{:?}", w.items);
    }

    #[test]
    fn dismissing_a_blocking_category_is_a_noop() {
        let conn = mem();
        insert(&conn, JobState::Failed, Some("boom"));
        // A blocking config category is gate state, not a dismissible notice: its
        // dismiss writes no marker, whatever id is passed, so the failed-job warning
        // is untouched.
        dismiss_warning_category(&conn, WarningKind::ApiTokens, "1").unwrap();
        assert!(
            failure_warning(&conn).unwrap().is_some(),
            "a blocking-category dismiss must not suppress the failed-job warning"
        );
    }

}
