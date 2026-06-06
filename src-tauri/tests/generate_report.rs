//! End-to-end integration test for the report pipeline: drive it with the
//! deterministic stub agent + stub data source and assert the two side effects —
//! the canonical Markdown file and the SQLite row — both land, and that the
//! Step-6 baseline scan reaches the agent's input.

use std::sync::Mutex;

use market_signal_temp_lib::agent::{MainAgent, MainAgentInput, MainAgentOutput, StubMainAgent};
use market_signal_temp_lib::data_sources::{
    BaselineMarketData, MarketDataSource, StubMarketDataSource,
};
use market_signal_temp_lib::pipeline::{generate_report, ReportPaths};

/// Wraps the stub agent and records the baseline it was handed, so the test can
/// assert the pipeline's Step-6 gather reached the agent stage.
struct RecordingAgent {
    seen: Mutex<Option<BaselineMarketData>>,
}

impl MainAgent for RecordingAgent {
    fn generate(&self, input: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
        *self.seen.lock().unwrap() = Some(input.baseline.clone());
        StubMainAgent.generate(input)
    }
}

#[test]
fn generate_report_writes_markdown_file_and_db_row() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    let report = generate_report(&StubMainAgent, &StubMarketDataSource, &paths).unwrap();

    // The canonical Markdown file was written to disk.
    assert!(
        std::path::Path::new(&report.markdown_path).exists(),
        "expected markdown file at {}",
        report.markdown_path
    );

    // The SQLite row exists with both regime axes populated.
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let (risk_posture, market_cycle): (String, String) = conn
        .query_row(
            "SELECT risk_posture, market_cycle FROM reports WHERE report_id = ?1",
            [&report.report_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(risk_posture, "mixed");
    assert_eq!(market_cycle, "late-cycle");
}

#[test]
fn step_6_baseline_scan_reaches_the_agent_input() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    let agent = RecordingAgent {
        seen: Mutex::new(None),
    };
    generate_report(&agent, &StubMarketDataSource, &paths).unwrap();

    // The pipeline gathered the data source's baseline and handed it to the agent
    // unchanged.
    let seen = agent.seen.lock().unwrap().clone().expect("agent was invoked");
    let expected = StubMarketDataSource.baseline_scan().unwrap();
    assert_eq!(seen, expected);
}
