//! End-to-end integration test for the report pipeline: drive it with the
//! deterministic stub agent + stub data source and assert the two side effects —
//! the canonical Markdown file and the SQLite row — both land, and that the
//! Step-6 baseline scan reaches the agent's input.

use std::sync::{Arc, Mutex};

use market_signal_temp_lib::agent::{
    AnalystAgent, AnalystOutput, MainAgent, MainAgentInput, MainAgentOutput, Posture,
    StubAnalystAgent, StubMainAgent,
};
use market_signal_temp_lib::baseline_delta::{BaselineDeltas, Direction};
use market_signal_temp_lib::data_sources::{
    BaselineMarketData, Change, MarketDataSource, Quote, StubMarketDataSource,
};
use market_signal_temp_lib::embedding::{Embedder, StubEmbedder};
use market_signal_temp_lib::headline_filter::StubHeadlineFilter;
use market_signal_temp_lib::news::StubNewsSource;
use market_signal_temp_lib::pipeline::{
    generate_report, AnalystStages, ReportPaths, ResearchStages,
};
use market_signal_temp_lib::progress::RunContext;
use market_signal_temp_lib::research_executor::StubSearchBackend;
use market_signal_temp_lib::research_packet::ResearchPacket;
use market_signal_temp_lib::research_router::{
    ResearchPlan, ResearchRouter, RouterInput, StubResearchRouter,
};

/// Wraps the stub agent and records the baseline *and* the change view it was handed, so
/// the tests can assert both the Step-6 gather and the prior-snapshot diff reached the
/// agent stage. `seen_deltas` is `Some(None)` once the agent ran with no change view,
/// `Some(Some(_))` once it ran with one. `seen_audit_memory` captures the Step-4
/// pre-research pull that steers the retrospective audit.
struct RecordingAgent {
    seen: Mutex<Option<BaselineMarketData>>,
    seen_deltas: Mutex<Option<Option<BaselineDeltas>>>,
    seen_research: Mutex<Option<Option<ResearchPacket>>>,
    seen_audit_memory: Mutex<Option<Vec<String>>>,
    seen_analyst_reviews: Mutex<Option<Vec<AnalystOutput>>>,
}

impl RecordingAgent {
    fn new() -> Self {
        Self {
            seen: Mutex::new(None),
            seen_deltas: Mutex::new(None),
            seen_research: Mutex::new(None),
            seen_audit_memory: Mutex::new(None),
            seen_analyst_reviews: Mutex::new(None),
        }
    }
}

impl MainAgent for RecordingAgent {
    fn generate(&self, input: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
        *self.seen.lock().unwrap() = Some(input.baseline.clone());
        *self.seen_deltas.lock().unwrap() = Some(input.deltas.clone());
        *self.seen_research.lock().unwrap() = Some(input.research.clone());
        *self.seen_audit_memory.lock().unwrap() = Some(input.audit_memory.clone());
        *self.seen_analyst_reviews.lock().unwrap() = Some(input.analyst_reviews.clone());
        StubMainAgent.generate(input)
    }
}

/// An analyst that always fails, to exercise the not-fail-soft contract: a failing
/// analyst stage is a job failure (`docs/weekly-report-workflow.md §Step 9`), unlike
/// the fail-soft research half.
struct FailingAnalyst;

impl AnalystAgent for FailingAnalyst {
    fn review(
        &self,
        _packet: &ResearchPacket,
        _cadence: market_signal_temp_lib::cadence::ReportCadence,
    ) -> anyhow::Result<AnalystOutput> {
        anyhow::bail!("analyst model unreachable (simulated)")
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
            change: Change::percent(0.0),
            unit: "index points".into(),
        }],
        internals: vec![Quote {
            symbol: "^VIX".into(),
            name: "CBOE Volatility Index".into(),
            price: 14.0,
            change: Change::percent(0.0),
            unit: "index points".into(),
        }],
        ..Default::default()
    }
}

#[test]
fn generate_report_writes_markdown_file_and_db_row() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    let report = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
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
    let paths = ReportPaths::under(dir.path());

    let agent = RecordingAgent::new();
    generate_report(
        &agent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    // The pipeline gathered the data source's baseline and handed it to the agent
    // unchanged.
    let seen = agent
        .seen
        .lock()
        .unwrap()
        .clone()
        .expect("agent was invoked");
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
    let paths = ReportPaths::under(dir.path());

    let agent = RecordingAgent::new();
    generate_report(
        &agent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
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
    assert!(
        packet.memory.is_empty(),
        "first run has no memory to recall"
    );
}

#[test]
fn analyst_reviews_reach_the_agent_input() {
    // Steps 12–15 → Step 16: the three stub analysts run over the packet and their
    // reviews ride into the main agent's input, one per posture in Bull/Bear/Balanced
    // order.
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    let agent = RecordingAgent::new();
    generate_report(
        &agent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let reviews = agent
        .seen_analyst_reviews
        .lock()
        .unwrap()
        .clone()
        .expect("agent was invoked");
    assert_eq!(
        reviews.iter().map(|r| r.posture).collect::<Vec<_>>(),
        vec![Posture::Bull, Posture::Bear, Posture::Balanced],
        "all three analyst reviews reached the agent, in posture order"
    );
}

#[test]
fn a_failing_analyst_fails_the_run() {
    // Not fail-soft: a single failing analyst aborts the run rather than degrading to a
    // thinner report (`docs/weekly-report-workflow.md §Step 9`).
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    let analysts = AnalystStages {
        bull: Box::new(FailingAnalyst),
        bear: Box::new(StubAnalystAgent::new(Posture::Bear)),
        balanced: Box::new(StubAnalystAgent::new(Posture::Balanced)),
    };
    let result = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &analysts,
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    );
    assert!(result.is_err(), "a failing analyst fails the run");
    // No report row was persisted — the run aborted before the persist step.
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM reports", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "no report persists when an analyst fails");
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
    let paths = ReportPaths::under(dir.path());

    // Run 1: no prior reports exist, so the router's recent-report context is empty.
    let seen1 = Arc::new(Mutex::new(None));
    let first = generate_report(
        &StubMainAgent,
        &FixedMarketDataSource(base_with_sp(5_500.0)),
        &stages_with_recording_router(seen1.clone()),
        &AnalystStages::stub(),
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
        &AnalystStages::stub(),
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
    // memory (Step 17); run 2's pre-research pull hands it to the router (Step 4) *and*
    // to the main agent's audit channel (Step 5), and its post-research pull carries it
    // into the packet the agent receives (Step 10).
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    // Run 1: the store is empty, so both pulls recall nothing.
    let seen1 = Arc::new(Mutex::new(None));
    let agent1 = RecordingAgent::new();
    generate_report(
        &agent1,
        &FixedMarketDataSource(base_with_sp(5_500.0)),
        &stages_with_recording_router(seen1.clone()),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    let input1 = seen1.lock().unwrap().clone().expect("router ran on run 1");
    assert!(
        input1.memory.is_empty(),
        "an empty store recalls nothing for routing"
    );
    let packet1 = agent1
        .seen_research
        .lock()
        .unwrap()
        .clone()
        .flatten()
        .expect("run 1 carried a packet");
    assert!(
        packet1.memory.is_empty(),
        "an empty store recalls nothing for the packet"
    );
    let audit1 = agent1
        .seen_audit_memory
        .lock()
        .unwrap()
        .clone()
        .expect("run 1 reached the agent");
    assert!(
        audit1.is_empty(),
        "an empty store recalls nothing for the audit"
    );

    // Run 2: run 1's summary is in the store; both pulls surface it.
    let seen2 = Arc::new(Mutex::new(None));
    let agent2 = RecordingAgent::new();
    generate_report(
        &agent2,
        &FixedMarketDataSource(base_with_sp(5_610.0)),
        &stages_with_recording_router(seen2.clone()),
        &AnalystStages::stub(),
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
    assert_eq!(
        packet2.memory.len(),
        1,
        "the Step-10 pull reached the agent's packet"
    );
    assert!(
        packet2.memory[0].contains("Risk posture:"),
        "the packet fragment is run 1's summary text: {}",
        packet2.memory[0]
    );

    // The Step-4 pull also reaches the main agent on its own channel — the audit consumer
    // (distinct from the packet's Step-10 memory above), recalled against run 1's summary.
    let audit2 = agent2
        .seen_audit_memory
        .lock()
        .unwrap()
        .clone()
        .expect("run 2 reached the agent");
    assert_eq!(
        audit2.len(),
        1,
        "the Step-4 pull reached the agent's audit channel"
    );
    assert!(
        audit2[0].starts_with("[summary · ") && audit2[0].contains("Risk posture:"),
        "the audit fragment is run 1's summary text: {}",
        audit2[0]
    );

    // And the store still holds exactly the two runs' summaries — retrieval wrote nothing.
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let memories: i64 = conn
        .query_row("SELECT COUNT(*) FROM vector_memory", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        memories, 2,
        "one summary row per persisted report, none from retrieval"
    );
}

#[test]
fn second_report_diffs_against_the_first_and_snapshots_persist() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    // Run 1: the first report has no prior snapshot, so the agent sees no change view.
    // The run still persists this run's baseline for the next report to diff against.
    let agent1 = RecordingAgent::new();
    generate_report(
        &agent1,
        &FixedMarketDataSource(base_with_sp(5_500.0)),
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    assert!(
        agent1
            .seen_deltas
            .lock()
            .unwrap()
            .clone()
            .flatten()
            .is_none(),
        "first report has no prior snapshot to diff"
    );

    // Run 2: the S&P moved +110. The pipeline reads run 1's snapshot and hands the agent
    // the deterministic change view.
    let agent2 = RecordingAgent::new();
    generate_report(
        &agent2,
        &FixedMarketDataSource(base_with_sp(5_610.0)),
        &ResearchStages::stub(),
        &AnalystStages::stub(),
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
    let paths = ReportPaths::under(dir.path());

    let report = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
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
    assert_eq!(
        hits[0].report_id.as_deref(),
        Some(report.report_id.as_str())
    );
}

#[test]
fn embedding_failure_never_fails_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    // The memory write is best-effort: a dead embedding stage costs the memory row,
    // never the already-persisted report.
    let report = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
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
    assert_eq!(
        memories, 0,
        "no memory row — and no error — on an embedding failure"
    );
}

/// Wraps the stub agent and attaches a fixed set of durable learnings, driving
/// the Step-17 learning-write leg (the stub itself deliberately emits none).
struct LearningAgent(Vec<String>);

impl MainAgent for LearningAgent {
    fn generate(&self, input: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
        let mut out = StubMainAgent.generate(input)?;
        out.durable_learnings = self.0.clone();
        Ok(out)
    }
}

/// A test embedder that places each distinct text on its own basis direction:
/// different texts (almost surely) embed orthogonally (cosine 0) while identical
/// text embeds identically (cosine 1.0). The production `text-embedding-3-large`
/// separates distinct content this way; the crude `StubEmbedder` instead collapses
/// all real prose to ~1.0 cosine (a constant common-mode component plus small
/// positive byte increments), which the Step-17 dedup pass would then over-merge.
/// The learning-write tests below assert that *distinct* lessons all persist, so
/// they need an embedder that models real separation; the identical-text path
/// still exercises dedup (see `durable_learnings_dedup_drops_a_restatement`).
struct DistinctEmbedder;

impl Embedder for DistinctEmbedder {
    fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        // FNV-1a over the trimmed text → one hot dimension in a wide vector:
        // different texts (almost surely) hash to different dimensions → orthogonal;
        // identical text → the same dimension → cosine 1.0.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.trim().bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let mut v = vec![0.0f32; 1024];
        let dim = v.len();
        v[(hash as usize) % dim] = 1.0;
        Ok(v)
    }
}

#[test]
fn durable_learnings_land_in_vector_memory() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    let agent = LearningAgent(vec![
        "Breadth divergences preceded the spring pullback; weight them earlier.".into(),
        "Single-event volatility spikes faded within two reports; avoid thesis pivots on them."
            .into(),
    ]);
    let report = generate_report(
        &agent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        // Distinct learnings must stay distinct under the embedder; StubEmbedder
        // collapses real prose to ~1.0 cosine, which dedup would then over-merge.
        &AnalystStages::stub(),
        &DistinctEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    // Three rows landed: the summary plus one row per learning, each tagged with the
    // report's id (provenance) and the agent-minted created_at, with a decodable blob.
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT report_id, content, embedding, created_at FROM vector_memory
             WHERE kind = 'learning' ORDER BY content",
        )
        .unwrap();
    let learnings: Vec<(String, String, Vec<u8>, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(learnings.len(), 2);
    for (report_id, content, blob, created_at) in &learnings {
        assert_eq!(report_id, &report.report_id);
        assert_eq!(created_at, &report.summary.created_at);
        assert!(!content.is_empty());
        assert!(!blob.is_empty());
        assert_eq!(blob.len() % 4, 0, "embedding blob is whole f32s");
    }
    assert!(
        learnings[0].1.starts_with("Breadth divergences"),
        "{}",
        learnings[0].1
    );

    let summaries: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vector_memory WHERE kind = 'summary'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        summaries, 1,
        "the summary write is unaffected by the learning leg"
    );
}

#[test]
fn durable_learnings_are_trimmed_and_capped() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    // Seven entries: one whitespace-only (dropped before the cap counts it) and six
    // real ones — one past the per-report cap of five.
    let mut entries: Vec<String> = (1..=6)
        .map(|i| format!("Durable lesson number {i}."))
        .collect();
    entries.insert(2, "   \n ".into());
    let report = generate_report(
        &LearningAgent(entries),
        &StubMarketDataSource,
        &ResearchStages::stub(),
        // Six distinct lessons must all survive to the cap; see DistinctEmbedder.
        &AnalystStages::stub(),
        &DistinctEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let learnings: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vector_memory WHERE kind = 'learning'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        learnings, 5,
        "empties are dropped, then the app-layer cap truncates"
    );
    let blanks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vector_memory WHERE TRIM(content) = ''",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        blanks, 0,
        "a whitespace-only learning never reaches the store"
    );
    assert!(std::path::Path::new(&report.markdown_path).exists());
}

#[test]
fn durable_learnings_dedup_drops_a_restatement() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    // The agent emits the same lesson twice (a restatement an LLM might paraphrase).
    // The Step-17 dedup pass embeds each and drops the second as a near-duplicate of
    // the first, so a single learning row lands — proving dedup is wired through
    // generate_report, not just the helper unit-tested in pipeline.rs.
    let learning = "Breadth divergences preceded the pullback; weight them earlier.";
    generate_report(
        &LearningAgent(vec![learning.into(), learning.into()]),
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &DistinctEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let learnings: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vector_memory WHERE kind = 'learning'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        learnings, 1,
        "the restated learning is deduped to a single row"
    );
}

#[test]
fn learning_embedding_failure_never_fails_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    // Same fail-soft posture as the summary write: a dead embedding stage costs the
    // learning rows, never the already-persisted report.
    let report = generate_report(
        &LearningAgent(vec!["A learning that will fail to embed.".into()]),
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &FailingEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    assert!(std::path::Path::new(&report.markdown_path).exists());

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let memories: i64 = conn
        .query_row("SELECT COUNT(*) FROM vector_memory", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        memories, 0,
        "no learning row — and no error — on an embedding failure"
    );
}

#[test]
fn cancel_during_persist_skips_remaining_memory_writes() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use market_signal_temp_lib::progress::NoopReporter;

    // An embedder that requests cancellation during its first call (the persist
    // step's summary embed) and counts every call — modeling a user cancel that
    // lands while that paid request is in flight. The in-flight call completes
    // (cooperative cancellation never interrupts a request), but the learning
    // embeds that would follow must be skipped, not spent.
    struct CancellingEmbedder {
        cancel: Arc<AtomicBool>,
        calls: AtomicUsize,
    }
    impl Embedder for CancellingEmbedder {
        fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.cancel.store(true, Ordering::Relaxed);
            StubEmbedder.embed(text)
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    let cancel = Arc::new(AtomicBool::new(false));
    let ctx = RunContext::new("test-run", Arc::new(NoopReporter), cancel.clone());
    let embedder = CancellingEmbedder {
        cancel,
        calls: AtomicUsize::new(0),
    };

    // A cancel this late is honored as a completed run: the report is already
    // persisted, so generate_report still returns it.
    let report = generate_report(
        &LearningAgent(vec![
            "A learning the cancel must skip.".into(),
            "Another learning the cancel must skip.".into(),
        ]),
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &embedder,
        &paths,
        &ctx,
    )
    .unwrap();
    assert!(std::path::Path::new(&report.markdown_path).exists());

    // Exactly one embedding call was spent — the in-flight summary embed. The two
    // learning embeds were skipped at the loop's cancel checkpoint.
    assert_eq!(
        embedder.calls.load(Ordering::Relaxed),
        1,
        "no paid call after the cancel"
    );
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let (summaries, learnings): (i64, i64) = conn
        .query_row(
            "SELECT
                 COUNT(*) FILTER (WHERE kind = 'summary'),
                 COUNT(*) FILTER (WHERE kind = 'learning')
             FROM vector_memory",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        summaries, 1,
        "the already-in-flight summary write still lands"
    );
    assert_eq!(learnings, 0, "no learning row after the cancel");
}

#[test]
fn learnings_written_on_one_run_are_recalled_on_the_next() {
    // The memory loop, closed end to end: run 1 writes a durable learning (Step 17);
    // run 2's pre-research pull hands it to the router (Step 4) and its post-research
    // pull carries it into the packet the agent receives (Step 10).
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());

    generate_report(
        &LearningAgent(vec![
            "Breadth divergences preceded the spring pullback; weight them earlier.".into(),
        ]),
        &FixedMarketDataSource(base_with_sp(5_500.0)),
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let seen2 = Arc::new(Mutex::new(None));
    let agent2 = RecordingAgent::new();
    generate_report(
        &agent2,
        &FixedMarketDataSource(base_with_sp(5_610.0)),
        &stages_with_recording_router(seen2.clone()),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let input2 = seen2.lock().unwrap().clone().expect("router ran on run 2");
    assert!(
        input2.memory.iter().any(|f| f.starts_with("[learning · ")),
        "the Step-4 pull surfaces the learning to the router: {:?}",
        input2.memory
    );
    let packet2 = agent2
        .seen_research
        .lock()
        .unwrap()
        .clone()
        .flatten()
        .expect("run 2 carried a packet");
    assert!(
        packet2
            .memory
            .iter()
            .any(|f| f.starts_with("[learning · ") && f.contains("Breadth divergences")),
        "the Step-10 pull carries the learning into the packet: {:?}",
        packet2.memory
    );
}

// ---------------------------------------------------------------------------
// Step-6 research-inbox parsing (docs/weekly-report-workflow.md §Step 6,
// docs/research-documents.md): parsed documents reach routing and the packet;
// successes archive after the report persists; failures stay in the inbox with
// a recorded reason; a failed run consumes nothing.
// ---------------------------------------------------------------------------

#[test]
fn inbox_documents_flow_to_router_and_packet_and_archive_after_persist() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());
    std::fs::create_dir_all(&paths.inbox_dir).unwrap();
    std::fs::write(
        paths.inbox_dir.join("notes.md"),
        "# Fed outlook\n\nRates likely hold through summer.",
    )
    .unwrap();
    std::fs::write(paths.inbox_dir.join("broken.json"), "{ not json").unwrap();
    std::fs::write(paths.inbox_dir.join("chart.png"), b"\x89PNG").unwrap();

    let seen = Arc::new(Mutex::new(None));
    let agent = RecordingAgent::new();
    generate_report(
        &agent,
        &StubMarketDataSource,
        &stages_with_recording_router(seen.clone()),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    // The parsed document reached routing as a header + excerpt block.
    let input = seen.lock().unwrap().clone().expect("router ran");
    assert_eq!(input.inbox_documents.len(), 1, "one parsed document routed");
    assert!(
        input.inbox_documents[0].contains("notes.md"),
        "{}",
        input.inbox_documents[0]
    );
    assert!(
        input.inbox_documents[0].contains("Rates likely hold through summer."),
        "{}",
        input.inbox_documents[0]
    );

    // And the condensed packet carried its prompt block to the agent.
    let packet = agent
        .seen_research
        .lock()
        .unwrap()
        .clone()
        .flatten()
        .expect("the agent received a packet");
    assert_eq!(packet.inbox_summaries.len(), 1);
    assert!(
        packet.inbox_summaries[0].contains("# Fed outlook"),
        "{}",
        packet.inbox_summaries[0]
    );

    // The report persisted, so the parsed file was archived; the unparseable one
    // stays in the inbox with its reason recorded; the unsupported one is untouched.
    assert!(
        !paths.inbox_dir.join("notes.md").exists(),
        "parsed file left the inbox"
    );
    assert!(
        paths.archive_dir.join("notes.md").exists(),
        "parsed file reached the archive"
    );
    assert!(
        paths.inbox_dir.join("broken.json").exists(),
        "failed file stays in the inbox"
    );
    assert!(!paths.archive_dir.join("broken.json").exists());
    assert!(
        paths.inbox_dir.join("chart.png").exists(),
        "unsupported file is untouched"
    );

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let failures: Vec<(String, String)> = conn
        .prepare("SELECT name, reason FROM research_parse_failures")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        failures.len(),
        1,
        "exactly the broken file is recorded: {failures:?}"
    );
    assert_eq!(failures[0].0, "broken.json");
    assert!(
        failures[0].1.contains("not valid JSON"),
        "{}",
        failures[0].1
    );

    // Nothing truncated (notes.md fits the parser's caps), yet the denominator
    // row was still recorded: one document parsed (broken.json and chart.png
    // never parsed), so the truncation rate is derivable as 0 / 1.
    let (parse_run_count, docs_parsed): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(docs_parsed), 0) FROM document_parse_runs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        parse_run_count, 1,
        "one parse-run row recorded for the report"
    );
    assert_eq!(
        docs_parsed, 1,
        "one parsed document recorded as the rate denominator"
    );
    let truncations: i64 = conn
        .query_row("SELECT COUNT(*) FROM document_truncations", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(truncations, 0, "the small note never truncated");
}

#[test]
fn a_failed_run_never_consumes_inbox_documents() {
    /// An agent that always errors, failing the run after the inbox stage parsed.
    struct FailingMainAgent;
    impl MainAgent for FailingMainAgent {
        fn generate(&self, _: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
            anyhow::bail!("agent down")
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());
    std::fs::create_dir_all(&paths.inbox_dir).unwrap();
    std::fs::write(paths.inbox_dir.join("notes.md"), "# kept\n\nStill here.").unwrap();

    let result = generate_report(
        &FailingMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    );
    assert!(result.is_err(), "the agent failure fails the run");

    // No report persisted, so the document was not consumed: it stays in the
    // inbox and nothing reached the archive.
    assert!(paths.inbox_dir.join("notes.md").exists());
    assert!(!paths.archive_dir.join("notes.md").exists());
}

#[test]
fn a_healed_inbox_clears_previously_recorded_failures() {
    // Run 1 records the broken file; the user fixes it; run 2's pass replaces
    // the failure set, so the panel's error state self-heals.
    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());
    std::fs::create_dir_all(&paths.inbox_dir).unwrap();
    std::fs::write(paths.inbox_dir.join("data.json"), "{ not json").unwrap();

    generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM research_parse_failures", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1, "run 1 recorded the failure");
    drop(conn);

    std::fs::write(paths.inbox_dir.join("data.json"), r#"{"fixed": true}"#).unwrap();
    generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM research_parse_failures", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0, "the healed file's row is gone");
    assert!(
        paths.archive_dir.join("data.json").exists(),
        "and the fixed file archived"
    );
}

// ---------------------------------------------------------------------------
// Retention cascade (docs/storage.md §SQLite): only the newest 30 reports are
// kept; an evicted report loses its Markdown file, report row, vector summary
// row, and baseline-snapshot rows together, while durable learnings survive.
// ---------------------------------------------------------------------------

/// Seed one pre-existing report directly: a real Markdown file in `file_dir`, a
/// `reports` row pointing at it, and a vector-memory summary row. Returns the
/// absolute markdown path. The 3-dim embedding never matches the stub
/// embedder's query dimension; retrieval skips such rows, the cascade's SQL
/// deletes don't care.
fn seed_report(
    conn: &rusqlite::Connection,
    file_dir: &std::path::Path,
    id: &str,
    created_at: &str,
) -> String {
    use market_signal_temp_lib::agent::{MarketCycle, ReportSummary, RiskPosture, ThesisStance};

    let path = file_dir.join(format!("{id}.md"));
    std::fs::write(&path, format!("# seeded report {id}\n")).unwrap();
    let path_str = path.to_string_lossy().into_owned();

    let summary = ReportSummary {
        report_id: id.to_string(),
        report_type: "weekly_market".to_string(),
        created_at: created_at.to_string(),
        risk_posture: RiskPosture::Mixed,
        market_cycle: MarketCycle::LateCycle,
        thesis_stance: ThesisStance::Uncertain,
        header_summary_bullets: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        key_risks: vec![],
        unresolved_questions: vec![],
        forward_outlook_themes: vec![],
    };
    let summary_json = serde_json::to_string(&summary).unwrap();
    market_signal_temp_lib::storage::insert_report(
        conn,
        &market_signal_temp_lib::storage::ReportRecord {
            summary: &summary,
            markdown_path: &path_str,
            summary_json: &summary_json,
        },
    )
    .unwrap();
    market_signal_temp_lib::vector_memory::insert_memory(
        conn,
        market_signal_temp_lib::vector_memory::MemoryKind::Summary,
        Some(id),
        &format!("summary for {id}"),
        &[0.1, 0.2, 0.3],
        created_at,
    )
    .unwrap();
    path_str
}

#[test]
fn retention_cascade_evicts_the_oldest_report_beyond_the_cap() {
    use market_signal_temp_lib::storage;
    use market_signal_temp_lib::vector_memory::MemoryKind;

    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());
    std::fs::create_dir_all(&paths.reports_dir).unwrap();

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    storage::init_schema(&conn).unwrap();

    // Exactly the cap's worth of pre-existing reports, past-dated so the stub
    // agent's freshly-minted created_at sorts newest. The oldest also owns a
    // durable learning and a baseline-snapshot row.
    let mut seeded_paths = Vec::new();
    for i in 0..30 {
        let created_at = format!("2026-01-{:02}T00:00:00Z", i + 1);
        seeded_paths.push(seed_report(
            &conn,
            &paths.reports_dir,
            &format!("old-{i:02}"),
            &created_at,
        ));
    }
    market_signal_temp_lib::vector_memory::insert_memory(
        &conn,
        MemoryKind::Learning,
        Some("old-00"),
        "a durable lesson",
        &[0.4, 0.5, 0.6],
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    storage::insert_baseline_snapshot(&conn, "old-00", "2026-01-01T00:00:00Z", 1, "{}").unwrap();
    drop(conn);

    // The 31st report pushes old-00 past the window.
    let report = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM reports", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 30, "exactly the retention cap remains");
    let oldest: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reports WHERE report_id = 'old-00'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(oldest, 0, "the oldest report row was evicted");
    let newest: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reports WHERE report_id = ?1",
            [&report.report_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(newest, 1, "the new report is in the window");

    // The evictee's file is gone; the next-oldest survivor's file is intact.
    assert!(!std::path::Path::new(&seeded_paths[0]).exists());
    assert!(std::path::Path::new(&seeded_paths[1]).exists());

    // Its vector summary row went with it; the durable learning survives.
    let summary_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vector_memory WHERE kind = 'summary' AND report_id = 'old-00'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(summary_rows, 0, "the evictee's summary row was deleted");
    let learning_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vector_memory WHERE kind = 'learning' AND report_id = 'old-00'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(learning_rows, 1, "the durable learning survives eviction");

    // Its baseline-snapshot row is gone too.
    let snapshot_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM baseline_snapshots WHERE report_id = 'old-00'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(snapshot_rows, 0, "the evictee's snapshot rows were deleted");
}

#[test]
fn retention_cascade_tolerates_an_already_missing_markdown_file() {
    use market_signal_temp_lib::storage;

    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());
    std::fs::create_dir_all(&paths.reports_dir).unwrap();

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    storage::init_schema(&conn).unwrap();
    let mut seeded_paths = Vec::new();
    for i in 0..30 {
        let created_at = format!("2026-01-{:02}T00:00:00Z", i + 1);
        seeded_paths.push(seed_report(
            &conn,
            &paths.reports_dir,
            &format!("old-{i:02}"),
            &created_at,
        ));
    }
    drop(conn);

    // The evictee's file disappeared out from under the app (user deleted it
    // by hand): NotFound counts as removed, the DB legs still run.
    std::fs::remove_file(&seeded_paths[0]).unwrap();

    generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    )
    .unwrap();

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let oldest: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reports WHERE report_id = 'old-00'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(oldest, 0, "a missing file does not block eviction");
    let summary_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vector_memory WHERE kind = 'summary' AND report_id = 'old-00'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(summary_rows, 0);
}

#[cfg(unix)]
#[test]
fn retention_cascade_skips_db_deletes_when_the_file_cannot_be_removed() {
    use std::os::unix::fs::PermissionsExt;

    use market_signal_temp_lib::storage;

    let dir = tempfile::tempdir().unwrap();
    let paths = ReportPaths::under(dir.path());
    std::fs::create_dir_all(&paths.reports_dir).unwrap();

    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    storage::init_schema(&conn).unwrap();

    // The oldest report's file lives in a directory the cascade cannot write
    // to, so its removal fails with a real error (not NotFound). The other 29
    // live in the normal reports dir.
    let locked_dir = dir.path().join("locked");
    std::fs::create_dir_all(&locked_dir).unwrap();
    let locked_path = seed_report(&conn, &locked_dir, "old-00", "2026-01-01T00:00:00Z");
    for i in 1..30 {
        let created_at = format!("2026-01-{:02}T00:00:00Z", i + 1);
        seed_report(
            &conn,
            &paths.reports_dir,
            &format!("old-{i:02}"),
            &created_at,
        );
    }
    drop(conn);
    std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = generate_report(
        &StubMainAgent,
        &StubMarketDataSource,
        &ResearchStages::stub(),
        &AnalystStages::stub(),
        &StubEmbedder,
        &paths,
        &RunContext::noop(),
    );

    // Restore before asserting so the tempdir can clean up even on failure.
    std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    result.unwrap();

    // The evictee is left fully intact — file, row, and vector summary — so the
    // next run's cascade re-selects it rather than orphaning the file.
    assert!(std::path::Path::new(&locked_path).exists());
    let conn = rusqlite::Connection::open(&paths.db_path).unwrap();
    let oldest: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reports WHERE report_id = 'old-00'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(oldest, 1, "a failed file removal skips the DB legs");
    let summary_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vector_memory WHERE kind = 'summary' AND report_id = 'old-00'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(summary_rows, 1);
}
