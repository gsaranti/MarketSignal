//! Integration coverage for the recent-reports read path: drive the pipeline
//! with the stub agent, then list the persisted reports and load one back from
//! disk by id.

use market_signal_temp_lib::agent::StubMainAgent;
use market_signal_temp_lib::data_sources::StubMarketDataSource;
use market_signal_temp_lib::embedding::StubEmbedder;
use market_signal_temp_lib::pipeline::{
    generate_report, list_reports, load_report, ReportPaths, ResearchStages,
};
use market_signal_temp_lib::progress::RunContext;

fn paths(dir: &std::path::Path) -> ReportPaths {
    ReportPaths::under(dir)
}

#[test]
fn lists_reports_newest_first_and_loads_one_back_by_id() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths(dir.path());

    let first = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    let second = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    // Both reports list, newest (second) first. The rowid tiebreak in the query
    // keeps this stable even if the two stub timestamps collide.
    let recent = list_reports(&paths).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].report_id, second.report_id);
    assert_eq!(recent[1].report_id, first.report_id);

    // Loading the older report by id reconstructs its Markdown from disk.
    let loaded = load_report(&paths, &first.report_id).unwrap();
    assert_eq!(loaded.report_id, first.report_id);
    assert_eq!(loaded.markdown, first.markdown);
    let on_disk = std::fs::read_to_string(&first.markdown_path).unwrap();
    assert_eq!(loaded.markdown, on_disk);

    // An unknown id is a typed error, not a panic.
    assert!(load_report(&paths, "does-not-exist").is_err());
}
