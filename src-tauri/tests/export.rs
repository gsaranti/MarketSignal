//! Integration coverage for the Markdown export path: drive the pipeline with the
//! stub agent, then export the persisted report's canonical Markdown to a chosen
//! destination and assert the written bytes match the stored artifact
//! (`docs/export.md §Export Behavior` — exports read stored artifacts, no re-run).

use market_signal_temp_lib::agent::StubMainAgent;
use market_signal_temp_lib::data_sources::StubMarketDataSource;
use market_signal_temp_lib::pipeline::{export_markdown_to, generate_report, ReportPaths};

fn paths(dir: &std::path::Path) -> ReportPaths {
    ReportPaths {
        db_path: dir.join("market_signal.db"),
        reports_dir: dir.join("reports"),
    }
}

#[test]
fn exports_stored_markdown_to_a_chosen_destination() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths(dir.path());

    let report = generate_report(&StubMainAgent, &StubMarketDataSource, &paths).unwrap();

    let dest = dir.path().join("exported.md");
    export_markdown_to(&paths, &report.report_id, &dest).unwrap();

    // The exported bytes are exactly the stored canonical Markdown — not a
    // regenerated body.
    let exported = std::fs::read_to_string(&dest).unwrap();
    let stored = std::fs::read_to_string(&report.markdown_path).unwrap();
    assert_eq!(exported, stored);
    assert_eq!(exported, report.markdown);
}

#[test]
fn exporting_an_unknown_id_is_a_typed_error() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths(dir.path());
    // Persist one report so the schema exists; then export a different id.
    generate_report(&StubMainAgent, &StubMarketDataSource, &paths).unwrap();

    let dest = dir.path().join("exported.md");
    assert!(export_markdown_to(&paths, "does-not-exist", &dest).is_err());
    // A failed export leaves no partial file at the destination.
    assert!(!dest.exists());
}
