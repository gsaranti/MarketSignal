//! Job lifecycle around a single report run (`docs/scheduling.md §Job States`):
//! the single-workflow guard (`docs/scheduling.md §Concurrent Job Protection`),
//! the `job_runs` record, and the failed-job warning (`docs/interface.md`).
//! Tauri-free like `pipeline`. Generation is on demand, so there is no scheduler
//! and no `Missed` state.

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

const MARKET_SIGNAL_JOB: &str = "market_signal";

/// `app_settings` key holding the identity (the failed run's `job_runs.id`) of
/// the dismissed warning, so a later, distinct failure re-surfaces
/// (`docs/interface.md §Persistent Warning Area`).
const DISMISSED_FAILED_JOB_RUN_ID_KEY: &str = "dismissed_failed_job_run_id";

const SKIP_REASON: &str = "another report run is already in progress";

const RUN_LABEL: &str = "Market Signal report";

/// How a job run ended (`docs/scheduling.md §Job States`). `Cancelled` is
/// recorded like `Skipped` (no report, no failed-job warning) but kept distinct
/// so history can tell an aborted run from a concurrency skip.
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

/// The success payload is boxed because `GeneratedReport` dwarfs the `String`
/// variants.
#[derive(Debug)]
pub enum JobOutcome {
    Successful(Box<GeneratedReport>),
    Failed(String),
    Skipped(String),
    Cancelled(String),
}

/// What kind of workflow holds the single run slot, so the footer labels the
/// in-flight work honestly. Kebab-case on the wire; mirrored in the frontend
/// `JobStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunKind {
    Report,
    Portfolio,
    /// The engine-only quick check (`docs/portfolio-analysis.md §The quick
    /// check`) holds the slot so it can't race a run's own retrieval or
    /// persistence.
    PortfolioQuickCheck,
    SchwabConnect,
    /// A view-only holdings pull (`docs/portfolio-analysis.md §Triggering`)
    /// holds the slot so it can't race a job's own pull or token refresh.
    HoldingsPull,
    /// An export or import (`docs/data-portability.md`) holds the slot so an
    /// archive never captures or replaces a half-written mid-run store.
    DataPortability,
}

/// The single-workflow-at-a-time guard. The locked check-and-set is what makes
/// two racing runs resolve to exactly one runner and one skip.
#[derive(Clone, Default)]
pub struct RunGuard(Arc<Mutex<Option<RunKind>>>);

impl RunGuard {
    /// `None` when a run is already in flight. Releasing happens on the token's
    /// `Drop`, so success, failure, and panic all free the slot.
    pub fn try_begin(&self, kind: RunKind) -> Option<RunToken> {
        let mut slot = self.lock();
        if slot.is_some() {
            return None;
        }
        *slot = Some(kind);
        Some(RunToken(self.0.clone()))
    }

    pub fn is_running(&self) -> bool {
        self.running_kind().is_some()
    }

    /// A best-effort status read for the UI, not a synchronization point.
    pub fn running_kind(&self) -> Option<RunKind> {
        *self.lock()
    }

    /// Poison-tolerant: a panic elsewhere must never wedge the guard
    /// permanently closed (or open).
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

/// Returns `Err` only on an infrastructure failure (the database itself); a
/// failed report is a normal `Ok(Failed)`. The schema is initialized up front
/// so the Skipped and Failed paths, which may short-circuit before
/// `generate_report` touches the database, still have `job_runs` to write to.
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

    // `_token` is held until the outcome is recorded; its Drop frees the slot.
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

    // After the guard claim, so a concurrency-skipped run never emits
    // `run-started` and never resets the cancellation of the run holding the slot.
    ctx.reset_cancel();
    ctx.run_started(RUN_LABEL);

    let started_at = now_rfc3339();
    match generate_report(agent, data, research, analysts, embedder, paths, ctx) {
        Ok(report) => {
            let finished_at = now_rfc3339();
            // `run-finished` is emitted before any job-history write error can propagate.
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
        // A mid-run cancel surfaces as an error from `generate_report`; the cancel
        // flag tells it from a genuine failure. A cancel landing after the report
        // persisted is honored as the Ok(report) above.
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

/// The `FailedJob` warning, or `None`. Warns only when the most recent run that
/// actually executed (Successful or Failed; a Skipped run neither raises nor
/// clears it) Failed. Non-blocking.
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
            // A later failure carries a higher `id`, so the dismiss marker no
            // longer matches and a fresh warning surfaces.
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

/// Dismiss by the identity the frontend rendered (`docs/interface.md
/// §Persistent Warning Area`), never one re-derived at click time, so a stale
/// click records a stale identity and a later, distinct failure still surfaces.
/// The blocking configuration categories are gate state, not dismissible
/// notices, so a dismiss of one is a no-op.
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

/// One section's "last X" stamps (`docs/scheduling.md §Job Status
/// Visibility`), UTC RFC3339; the frontend renders them in local time.
#[derive(Debug, Clone, Serialize)]
pub struct SectionStamps {
    pub last_successful_at: Option<String>,
    pub last_failed_at: Option<String>,
    pub last_failure_detail: Option<String>,
    pub last_skipped_at: Option<String>,
    pub last_cancelled_at: Option<String>,
}

/// Job status for the UI (`docs/scheduling.md §Job Status Visibility`). The
/// stamps are per section, keyed by `job_type`, so a portfolio run never stamps
/// LAST RUN under report chrome; the run-slot fields stay global. The quick
/// check's rows reach neither section (a between-run sweep is not analysis
/// freshness); its failures still surface through `failure_warning`, which
/// deliberately keeps no `job_type` filter.
#[derive(Debug, Clone, Serialize)]
pub struct JobStatus {
    pub is_running: bool,
    pub running_kind: Option<RunKind>,
    pub report: SectionStamps,
    pub portfolio: SectionStamps,
}

/// Each "last X" is independent of the others: a later failure does not erase
/// the last successful stamp. Both sections ride one payload so a view switch
/// never waits on, or briefly mislabels across, a re-fetch.
pub fn job_status(conn: &Connection, guard: &RunGuard) -> Result<JobStatus> {
    let stamps_for = |job_type: &str| -> Result<SectionStamps> {
        let last_of = |state: &str| -> Result<Option<(String, Option<String>)>> {
            Ok(conn
                .query_row(
                    "SELECT finished_at, detail FROM job_runs
                     WHERE state = ?1 AND job_type = ?2 ORDER BY id DESC LIMIT 1",
                    params![state, job_type],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?)
        };
        let failed = last_of(JobState::Failed.as_str())?;
        Ok(SectionStamps {
            last_successful_at: last_of(JobState::Successful.as_str())?.map(|(at, _)| at),
            last_failed_at: failed.as_ref().map(|(at, _)| at.clone()),
            last_failure_detail: failed.and_then(|(_, detail)| detail),
            last_skipped_at: last_of(JobState::Skipped.as_str())?.map(|(at, _)| at),
            last_cancelled_at: last_of(JobState::Cancelled.as_str())?.map(|(at, _)| at),
        })
    };
    Ok(JobStatus {
        is_running: guard.is_running(),
        running_kind: guard.running_kind(),
        report: stamps_for(MARKET_SIGNAL_JOB)?,
        portfolio: stamps_for(crate::portfolio::job::PORTFOLIO_JOB)?,
    })
}

/// UTC is the canonical persisted form for `job_runs` timestamps; local time is
/// a display concern at the seams that show time to the user.
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
        assert!(st.report.last_successful_at.is_some());
        assert!(st.report.last_failed_at.is_some());
        assert_eq!(st.report.last_failure_detail.as_deref(), Some("provider 500"));
        assert!(st.report.last_skipped_at.is_some());
    }

    #[test]
    fn job_status_scopes_stamps_to_each_sections_job_type() {
        let conn = mem();
        insert(&conn, JobState::Successful, None);
        record_run(
            &conn,
            &JobRun {
                job_type: crate::portfolio::job::PORTFOLIO_JOB,
                state: JobState::Failed,
                started_at: "2026-06-02T09:00:00Z",
                finished_at: "2026-06-02T09:00:05Z",
                report_id: None,
                detail: Some("daemon unreachable"),
            },
        )
        .unwrap();
        let st = job_status(&conn, &RunGuard::default()).unwrap();
        assert_eq!(st.report.last_successful_at.as_deref(), Some("2026-06-01T09:05:00Z"));
        assert!(st.report.last_failed_at.is_none(), "a portfolio failure is not the report's");
        assert!(st.portfolio.last_successful_at.is_none(), "a report run is not the portfolio's");
        assert_eq!(st.portfolio.last_failed_at.as_deref(), Some("2026-06-02T09:00:05Z"));
        assert_eq!(st.portfolio.last_failure_detail.as_deref(), Some("daemon unreachable"));
    }

    #[test]
    fn job_status_ignores_quick_check_rows() {
        let conn = mem();
        record_run(
            &conn,
            &JobRun {
                job_type: crate::portfolio::quick_check::QUICK_CHECK_JOB,
                state: JobState::Successful,
                started_at: "2026-06-02T09:00:00Z",
                finished_at: "2026-06-02T09:00:05Z",
                report_id: None,
                detail: None,
            },
        )
        .unwrap();
        let st = job_status(&conn, &RunGuard::default()).unwrap();
        assert!(st.report.last_successful_at.is_none(), "a sweep is not a run the footer reports");
        assert!(st.portfolio.last_successful_at.is_none(), "a sweep is not portfolio analysis freshness");
        // A real workflow's stamp still surfaces, unshadowed by a later sweep.
        insert(&conn, JobState::Successful, None);
        record_run(
            &conn,
            &JobRun {
                job_type: crate::portfolio::quick_check::QUICK_CHECK_JOB,
                state: JobState::Successful,
                started_at: "2026-06-03T09:00:00Z",
                finished_at: "2026-06-03T09:00:05Z",
                report_id: None,
                detail: None,
            },
        )
        .unwrap();
        let st = job_status(&conn, &RunGuard::default()).unwrap();
        assert_eq!(st.report.last_successful_at.as_deref(), Some("2026-06-01T09:05:00Z"));
        assert!(st.portfolio.last_successful_at.is_none());
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
        assert!(guard.try_begin(RunKind::Report).is_none());
        assert_eq!(guard.running_kind(), Some(RunKind::SchwabConnect));
        drop(token);
        assert_eq!(guard.running_kind(), None);
        assert!(guard.try_begin(RunKind::Report).is_some());
    }

    #[test]
    fn running_kind_serializes_kebab_case_for_the_frontend() {
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
        assert_eq!(
            serde_json::to_value(RunKind::HoldingsPull).unwrap(),
            "holdings-pull"
        );
    }

    /// The `dismiss_id` the frontend would echo back for the shown warning.
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
        dismiss_warning_category(&conn, WarningKind::ApiTokens, "1").unwrap();
        assert!(
            failure_warning(&conn).unwrap().is_some(),
            "a blocking-category dismiss must not suppress the failed-job warning"
        );
    }

}
