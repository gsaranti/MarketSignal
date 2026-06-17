//! End-to-end integration test for the job-lifecycle core (scheduler slice 1):
//! drive `jobs::run_job` against the deterministic stub and assert each of the
//! three lifecycle states it can produce — Successful, Failed, Skipped — lands a
//! `job_runs` row and the right side effects, all offline.

use market_signal_temp_lib::agent::{MainAgent, MainAgentInput, MainAgentOutput, StubMainAgent};
use market_signal_temp_lib::data_sources::{
    BaselineMarketData, MarketDataSource, StubMarketDataSource,
};
use market_signal_temp_lib::embedding::StubEmbedder;
use market_signal_temp_lib::jobs::{run_job, JobOutcome, RunGuard};
use market_signal_temp_lib::pipeline::{AnalystStages, ReportPaths, ResearchStages};
use market_signal_temp_lib::progress::RunContext;

/// A stub that always fails, standing in for an unreachable provider so the
/// Failed lifecycle path can be exercised without live keys.
struct FailingAgent;

impl MainAgent for FailingAgent {
    fn generate(&self, _input: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
        anyhow::bail!("provider unreachable (simulated)")
    }
}

/// A data source that always fails, standing in for an unreachable / rejecting
/// data provider so the Step-6-failure-is-a-job-failure path can be exercised.
struct FailingDataSource;

impl MarketDataSource for FailingDataSource {
    fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
        anyhow::bail!("data provider unreachable (simulated)")
    }
}

fn paths_in(dir: &std::path::Path) -> ReportPaths {
    ReportPaths::under(dir)
}

fn count(db: &std::path::Path, sql: &str) -> i64 {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn successful_run_records_successful_job_and_writes_report() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(dir.path());

    let outcome =
        run_job(&StubMainAgent, &StubMarketDataSource, &ResearchStages::stub(), &AnalystStages::stub(), &StubEmbedder, &paths, &RunGuard::default(), &RunContext::noop()).unwrap();

    match outcome {
        JobOutcome::Successful(report) => assert!(
            std::path::Path::new(&report.markdown_path).exists(),
            "expected the markdown file at {}",
            report.markdown_path
        ),
        other => panic!("expected Successful, got {other:?}"),
    }

    assert_eq!(
        count(&paths.db_path, "SELECT COUNT(*) FROM job_runs WHERE state = 'successful'"),
        1
    );
    assert_eq!(count(&paths.db_path, "SELECT COUNT(*) FROM reports"), 1);
}

#[test]
fn failing_agent_records_failed_job_and_writes_no_report() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(dir.path());

    let outcome =
        run_job(&FailingAgent, &StubMarketDataSource, &ResearchStages::stub(), &AnalystStages::stub(), &StubEmbedder, &paths, &RunGuard::default(), &RunContext::noop()).unwrap();

    match outcome {
        JobOutcome::Failed(msg) => {
            assert!(msg.contains("provider unreachable"), "detail was: {msg}")
        }
        other => panic!("expected Failed, got {other:?}"),
    }

    // The failure detail is persisted, and no report row was created.
    let detail: String = {
        let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
        conn.query_row(
            "SELECT detail FROM job_runs WHERE state = 'failed' ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert!(detail.contains("provider unreachable"), "{detail}");
    assert_eq!(count(&paths.db_path, "SELECT COUNT(*) FROM reports"), 0);
}

#[test]
fn failing_data_source_records_failed_job_and_writes_no_report() {
    // Step 6 runs before the agent, so an unreachable data provider fails the job
    // (`docs/scheduling.md §Offline Behavior`) even with a working agent.
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(dir.path());

    let outcome = run_job(&StubMainAgent, &FailingDataSource, &ResearchStages::stub(), &AnalystStages::stub(), &StubEmbedder, &paths, &RunGuard::default(), &RunContext::noop()).unwrap();

    match outcome {
        JobOutcome::Failed(msg) => {
            assert!(msg.contains("data provider unreachable"), "detail was: {msg}")
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(count(&paths.db_path, "SELECT COUNT(*) FROM reports"), 0);
}

#[test]
fn second_run_while_one_is_in_flight_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(dir.path());
    let guard = RunGuard::default();

    // Simulate an in-flight run by holding the single run slot.
    let token = guard.try_begin().expect("first claim succeeds");

    let outcome = run_job(&StubMainAgent, &StubMarketDataSource, &ResearchStages::stub(), &AnalystStages::stub(), &StubEmbedder, &paths, &guard, &RunContext::noop()).unwrap();
    match outcome {
        JobOutcome::Skipped(_) => {}
        other => panic!("expected Skipped, got {other:?}"),
    }

    // A skipped run is recorded but produces no report.
    assert_eq!(
        count(&paths.db_path, "SELECT COUNT(*) FROM job_runs WHERE state = 'skipped'"),
        1
    );
    assert_eq!(count(&paths.db_path, "SELECT COUNT(*) FROM reports"), 0);

    // Releasing the slot lets the next run proceed to completion.
    drop(token);
    let outcome = run_job(&StubMainAgent, &StubMarketDataSource, &ResearchStages::stub(), &AnalystStages::stub(), &StubEmbedder, &paths, &guard, &RunContext::noop()).unwrap();
    assert!(matches!(outcome, JobOutcome::Successful(_)));
    assert_eq!(
        count(&paths.db_path, "SELECT COUNT(*) FROM job_runs WHERE state = 'successful'"),
        1
    );
}

#[test]
fn cancelled_run_records_cancelled_job_and_writes_no_report() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use market_signal_temp_lib::progress::NoopReporter;

    // A data source that requests cancellation mid-run (as it gathers the baseline),
    // so the pipeline's post-baseline checkpoint trips. This is the realistic cancel
    // path — and it survives run_job's reset of the shared flag at run start (a
    // pre-set flag would be cleared before the run polls it).
    struct CancellingData(Arc<AtomicBool>);
    impl MarketDataSource for CancellingData {
        fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
            self.0.store(true, Ordering::Relaxed);
            StubMarketDataSource.baseline_scan()
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(dir.path());

    let cancel = Arc::new(AtomicBool::new(false));
    let ctx = RunContext::new("t", Arc::new(NoopReporter), cancel.clone());
    let data = CancellingData(cancel);
    let outcome = run_job(&StubMainAgent, &data, &ResearchStages::stub(), &AnalystStages::stub(), &StubEmbedder, &paths, &RunGuard::default(), &ctx).unwrap();

    match outcome {
        JobOutcome::Cancelled(detail) => assert!(detail.contains("cancelled"), "{detail}"),
        other => panic!("expected Cancelled, got {other:?}"),
    }

    assert_eq!(
        count(&paths.db_path, "SELECT COUNT(*) FROM job_runs WHERE state = 'cancelled'"),
        1
    );
    assert_eq!(count(&paths.db_path, "SELECT COUNT(*) FROM reports"), 0);
}

#[test]
fn a_skipped_run_does_not_reset_an_active_runs_cancel_flag() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use market_signal_temp_lib::progress::NoopReporter;

    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(dir.path());
    let guard = RunGuard::default();

    // Simulate an active run holding the slot with a cancel already requested.
    let _token = guard.try_begin().expect("first claim succeeds");
    let cancel = Arc::new(AtomicBool::new(true));
    let ctx = RunContext::new("competing", Arc::new(NoopReporter), cancel.clone());

    // A competing run is skipped (guard busy) before it owns the slot, so it must not
    // reach reset_cancel and wipe the active run's pending cancellation.
    let outcome =
        run_job(&StubMainAgent, &StubMarketDataSource, &ResearchStages::stub(), &AnalystStages::stub(), &StubEmbedder, &paths, &guard, &ctx).unwrap();
    assert!(matches!(outcome, JobOutcome::Skipped(_)));
    assert!(
        cancel.load(Ordering::Relaxed),
        "a skipped run must not reset the active run's cancel flag"
    );
}
