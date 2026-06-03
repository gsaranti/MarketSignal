//! End-to-end integration test for the job-lifecycle core (scheduler slice 1):
//! drive `jobs::run_job` against the deterministic stub and assert each of the
//! three lifecycle states it can produce — Successful, Failed, Skipped — lands a
//! `job_runs` row and the right side effects, all offline.

use market_signal_temp_lib::agent::{MainAgent, MainAgentInput, MainAgentOutput, StubMainAgent};
use market_signal_temp_lib::jobs::{run_job, JobOutcome, RunGuard};
use market_signal_temp_lib::pipeline::ReportPaths;

/// A stub that always fails, standing in for an unreachable provider so the
/// Failed lifecycle path can be exercised without live keys.
struct FailingAgent;

impl MainAgent for FailingAgent {
    fn generate(&self, _input: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
        anyhow::bail!("provider unreachable (simulated)")
    }
}

fn paths_in(dir: &std::path::Path) -> ReportPaths {
    ReportPaths {
        db_path: dir.join("market_signal.db"),
        reports_dir: dir.join("reports"),
    }
}

fn count(db: &std::path::Path, sql: &str) -> i64 {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn successful_run_records_successful_job_and_writes_report() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(dir.path());

    let outcome = run_job(&StubMainAgent, &paths, &RunGuard::default()).unwrap();

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

    let outcome = run_job(&FailingAgent, &paths, &RunGuard::default()).unwrap();

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
fn second_run_while_one_is_in_flight_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(dir.path());
    let guard = RunGuard::default();

    // Simulate an in-flight run by holding the single run slot.
    let token = guard.try_begin().expect("first claim succeeds");

    let outcome = run_job(&StubMainAgent, &paths, &guard).unwrap();
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
    let outcome = run_job(&StubMainAgent, &paths, &guard).unwrap();
    assert!(matches!(outcome, JobOutcome::Successful(_)));
    assert_eq!(
        count(&paths.db_path, "SELECT COUNT(*) FROM job_runs WHERE state = 'successful'"),
        1
    );
}
