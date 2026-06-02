//! End-to-end integration test for the first vertical slice: drive the pipeline
//! with the deterministic stub agent and assert the two side effects — the
//! canonical Markdown file and the SQLite row — both land.

use market_signal_temp_lib::agent::StubMainAgent;
use market_signal_temp_lib::pipeline::{generate_report, ReportPaths};

#[test]
fn generate_report_writes_markdown_file_and_db_row() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    let report = generate_report(&StubMainAgent, &paths).unwrap();

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
