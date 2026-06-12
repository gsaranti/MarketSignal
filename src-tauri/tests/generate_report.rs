//! End-to-end integration test for the report pipeline: drive it with the
//! deterministic stub agent + stub data source and assert the two side effects —
//! the canonical Markdown file and the SQLite row — both land, and that the
//! Step-6 baseline scan reaches the agent's input.

use std::sync::{Arc, Mutex};

use market_signal_temp_lib::agent::{MainAgent, MainAgentInput, MainAgentOutput, StubMainAgent};
use market_signal_temp_lib::baseline_delta::{BaselineDeltas, Direction};
use market_signal_temp_lib::data_sources::{
    BaselineMarketData, MarketDataSource, Quote, StubMarketDataSource,
};
use market_signal_temp_lib::embedding::{Embedder, StubEmbedder};
use market_signal_temp_lib::headline_filter::StubHeadlineFilter;
use market_signal_temp_lib::news::StubNewsSource;
use market_signal_temp_lib::pipeline::{generate_report, ReportPaths, ResearchStages};
use market_signal_temp_lib::progress::RunContext;
use market_signal_temp_lib::research_executor::StubSearchBackend;
use market_signal_temp_lib::research_packet::ResearchPacket;
use market_signal_temp_lib::research_router::{
    ResearchPlan, ResearchRouter, RouterInput, StubResearchRouter,
};

/// Wraps the stub agent and records the baseline *and* the change view it was handed, so
/// the tests can assert both the Step-6 gather and the prior-snapshot diff reached the
/// agent stage. `seen_deltas` is `Some(None)` once the agent ran with no change view,
/// `Some(Some(_))` once it ran with one.
struct RecordingAgent {
    seen: Mutex<Option<BaselineMarketData>>,
    seen_deltas: Mutex<Option<Option<BaselineDeltas>>>,
    seen_research: Mutex<Option<Option<ResearchPacket>>>,
}

impl RecordingAgent {
    fn new() -> Self {
        Self {
            seen: Mutex::new(None),
            seen_deltas: Mutex::new(None),
            seen_research: Mutex::new(None),
        }
    }
}

impl MainAgent for RecordingAgent {
    fn generate(&self, input: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
        *self.seen.lock().unwrap() = Some(input.baseline.clone());
        *self.seen_deltas.lock().unwrap() = Some(input.deltas.clone());
        *self.seen_research.lock().unwrap() = Some(input.research.clone());
        StubMainAgent.generate(input)
    }
}

/// A data source that returns a fixed baseline, so two successive runs can be given
/// baselines that differ by a known amount.
struct FixedMarketDataSource(BaselineMarketData);

impl MarketDataSource for FixedMarketDataSource {
    fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
        Ok(self.0.clone())
    }
}

/// A minimal coverage-passing baseline (indices + internals clear the floor) with the
/// S&P 500 at `sp_price`.
fn base_with_sp(sp_price: f64) -> BaselineMarketData {
    BaselineMarketData {
        indices: vec![Quote {
            symbol: "^GSPC".into(),
            name: "S&P 500".into(),
            price: sp_price,
            change_pct: 0.0,
            unit: "index points".into(),
        }],
        internals: vec![Quote {
            symbol: "^VIX".into(),
            name: "CBOE Volatility Index".into(),
            price: 14.0,
            change_pct: 0.0,
            unit: "index points".into(),
        }],
        ..Default::default()
    }
}

#[test]
fn generate_report_writes_markdown_file_and_db_row() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    let report = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

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

    let agent = RecordingAgent::new();
    generate_report(
        &agent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    // The pipeline gathered the data source's baseline and handed it to the agent
    // unchanged.
    let seen = agent.seen.lock().unwrap().clone().expect("agent was invoked");
    let expected = StubMarketDataSource.baseline_scan().unwrap();
    assert_eq!(seen, expected);
}

#[test]
fn research_packet_reaches_the_agent_input() {
    // Drive the spine with the stub research stages: the stub news source yields headlines,
    // the stub filter clusters them, the stub router routes them, and the stub search backend
    // returns synthetic evidence — so the assembled packet that reaches the agent carries both
    // news clusters and research evidence (the whole point of the wiring slice).
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    let agent = RecordingAgent::new();
    generate_report(
        &agent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let packet = agent
        .seen_research
        .lock()
        .unwrap()
        .clone()
        .expect("agent was invoked")
        .expect("the wired pipeline always hands the agent a research packet");
    assert!(
        !packet.news_clusters.is_empty(),
        "the stub news → filter chain produced clusters that reached the agent"
    );
    assert!(
        !packet.research.items.is_empty(),
        "the stub route → execute chain produced evidence that reached the agent"
    );
    // On a first run the vector store is empty, so the Step-10 pull recalls nothing
    // and the packet's memory is empty (the two-run test below covers the populated
    // path).
    assert!(packet.memory.is_empty(), "first run has no memory to recall");
}

/// A router that records the full input it was handed and then delegates to the
/// stub, so the tests assert what reached Step 8 (recent-report context, the
/// Step-4 memory pull) without changing the run. The recorded value sits behind
/// an `Arc` because the router itself is boxed into `ResearchStages` and moved
/// into the run.
struct RecordingRouter(Arc<Mutex<Option<RouterInput>>>);

impl ResearchRouter for RecordingRouter {
    fn route(&self, input: RouterInput) -> anyhow::Result<ResearchPlan> {
        *self.0.lock().unwrap() = Some(input.clone());
        StubResearchRouter.route(input)
    }
}

fn stages_with_recording_router(seen: Arc<Mutex<Option<RouterInput>>>) -> ResearchStages {
    ResearchStages {
        news: Box::new(StubNewsSource),
        filter: Box::new(StubHeadlineFilter),
        router: Box::new(RecordingRouter(seen)),
        search: Box::new(StubSearchBackend),
    }
}

#[test]
fn second_report_routes_with_the_first_reports_summary() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    // Run 1: no prior reports exist, so the router's recent-report context is empty.
    let seen1 = Arc::new(Mutex::new(None));
    let first = generate_report(
        &StubMainAgent,
        &FixedMarketDataSource(base_with_sp(5_500.0)),
        &stages_with_recording_router(seen1.clone()),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    let input1 = seen1.lock().unwrap().clone().expect("router ran on run 1");
    assert!(
        input1.recent_reports.is_empty(),
        "first report has no prior reports to route with"
    );

    // Run 2: the router is handed run 1's persisted summary as continuity context.
    let seen2 = Arc::new(Mutex::new(None));
    generate_report(
        &StubMainAgent,
        &FixedMarketDataSource(base_with_sp(5_610.0)),
        &stages_with_recording_router(seen2.clone()),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    let input2 = seen2.lock().unwrap().clone().expect("router ran on run 2");
    assert_eq!(
        input2.recent_reports.len(),
        1,
        "exactly the one prior report rides into routing"
    );
    assert_eq!(input2.recent_reports[0].report_id, first.report_id);
}

#[test]
fn memory_flows_into_routing_and_the_packet_on_the_second_run() {
    // The Step-4/10 retrieval slice end to end: run 1 persists its summary to vector
    // memory (Step 17); run 2's pre-research pull hands it to the router (Step 4) and
    // its post-research pull carries it into the packet the agent receives (Step 10).
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    // Run 1: the store is empty, so both pulls recall nothing.
    let seen1 = Arc::new(Mutex::new(None));
    let agent1 = RecordingAgent::new();
    generate_report(
        &agent1,
        &FixedMarketDataSource(base_with_sp(5_500.0)),
        &stages_with_recording_router(seen1.clone()),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    let input1 = seen1.lock().unwrap().clone().expect("router ran on run 1");
    assert!(input1.memory.is_empty(), "an empty store recalls nothing for routing");
    let packet1 = agent1
        .seen_research
        .lock()
        .unwrap()
        .clone()
        .flatten()
        .expect("run 1 carried a packet");
    assert!(packet1.memory.is_empty(), "an empty store recalls nothing for the packet");

    // Run 2: run 1's summary is in the store; both pulls surface it.
    let seen2 = Arc::new(Mutex::new(None));
    let agent2 = RecordingAgent::new();
    generate_report(
        &agent2,
        &FixedMarketDataSource(base_with_sp(5_610.0)),
        &stages_with_recording_router(seen2.clone()),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let input2 = seen2.lock().unwrap().clone().expect("router ran on run 2");
    assert_eq!(input2.memory.len(), 1, "the Step-4 pull reached the router");
    assert!(
        input2.memory[0].starts_with("[summary · "),
        "fragments carry their provenance tag: {}",
        input2.memory[0]
    );
    assert!(
        input2.memory[0].contains("Risk posture:"),
        "the fragment is run 1's summary text: {}",
        input2.memory[0]
    );

    let packet2 = agent2
        .seen_research
        .lock()
        .unwrap()
        .clone()
        .flatten()
        .expect("run 2 carried a packet");
    assert_eq!(packet2.memory.len(), 1, "the Step-10 pull reached the agent's packet");
    assert!(
        packet2.memory[0].contains("Risk posture:"),
        "the packet fragment is run 1's summary text: {}",
        packet2.memory[0]
    );

    // And the store still holds exactly the two runs' summaries — retrieval wrote nothing.
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let memories: i64 = conn
        .query_row("SELECT COUNT(*) FROM vector_memory", [], |r| r.get(0))
        .unwrap();
    assert_eq!(memories, 2, "one summary row per persisted report, none from retrieval");
}

#[test]
fn second_report_diffs_against_the_first_and_snapshots_persist() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    // Run 1: the first report has no prior snapshot, so the agent sees no change view.
    // The run still persists this run's baseline for the next report to diff against.
    let agent1 = RecordingAgent::new();
    generate_report(
        &agent1,
        &FixedMarketDataSource(base_with_sp(5_500.0)),
        &ResearchStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    assert!(
        agent1.seen_deltas.lock().unwrap().clone().flatten().is_none(),
        "first report has no prior snapshot to diff"
    );

    // Run 2: the S&P moved +110. The pipeline reads run 1's snapshot and hands the agent
    // the deterministic change view.
    let agent2 = RecordingAgent::new();
    generate_report(
        &agent2,
        &FixedMarketDataSource(base_with_sp(5_610.0)),
        &ResearchStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    let deltas = agent2
        .seen_deltas
        .lock()
        .unwrap()
        .clone()
        .flatten()
        .expect("second report carries a change view");
    let sp = deltas
        .changed
        .iter()
        .find(|d| d.id == "^GSPC")
        .expect("S&P delta present");
    assert!((sp.abs_change - 110.0).abs() < 1e-9);
    assert_eq!(sp.direction, Direction::Up);

    // Both runs persisted a baseline snapshot.
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM baseline_snapshots", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

/// An embedder that always errors, to drive the Step-17 memory write's fail-soft arm.
struct FailingEmbedder;

impl Embedder for FailingEmbedder {
    fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
        anyhow::bail!("embeddings down")
    }
}

#[test]
fn report_summary_lands_in_vector_memory() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    let report = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    // Exactly one memory row landed: the report's summary, keyed by its report_id,
    // with a decodable embedding blob.
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let (kind, report_id, content, blob): (String, String, String, Vec<u8>) = conn
        .query_row(
            "SELECT kind, report_id, content, embedding FROM vector_memory",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(kind, "summary");
    assert_eq!(report_id, report.report_id);
    assert!(content.contains("Risk posture:"), "{content}");
    assert!(!blob.is_empty());
    assert_eq!(blob.len() % 4, 0, "embedding blob is whole f32s");

    // And the retrieval path finds it: a same-dimension query (the stub embedder)
    // surfaces the stored summary through the store's own search.
    let query = StubEmbedder.embed("anything").unwrap();
    let hits =
        market_signal_temp_lib::vector_memory::search_memory(&conn, &query, None, 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].report_id.as_deref(), Some(report.report_id.as_str()));
}

#[test]
fn embedding_failure_never_fails_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths {
        db_path: dir.path().join("market_signal.db"),
        reports_dir: dir.path().join("reports"),
    };

    // The memory write is best-effort: a dead embedding stage costs the memory row,
    // never the already-persisted report.
    let report = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &FailingEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    assert!(std::path::Path::new(&report.markdown_path).exists());

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let reports: i64 = conn
        .query_row("SELECT COUNT(*) FROM reports", [], |r| r.get(0))
        .unwrap();
    assert_eq!(reports, 1, "the report row persisted");
    let memories: i64 = conn
        .query_row("SELECT COUNT(*) FROM vector_memory", [], |r| r.get(0))
        .unwrap();
    assert_eq!(memories, 0, "no memory row — and no error — on an embedding failure");
}
