//! Application-layer orchestration for a single manual report run.
//!
//! This is the spine the whole system is built on: the app layer drives the
//! agent stage (a pure function) and owns every side effect — the database
//! write and the canonical Markdown file. It is written free of any Tauri
//! runtime so it can be driven directly by an integration test against stubs.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::agent::{MainAgent, MainAgentInput, ReportSummary};
use crate::baseline_delta::{self, BaselineDeltas};
use crate::data_sources::{
    BaselineMarketData, GroupKind, MarketDataSource, BASELINE_SCHEMA_VERSION,
};
use crate::embedding::Embedder;
use crate::headline_filter::HeadlineFilter;
use crate::news::{self, NewsSource};
use crate::progress::RunContext;
use crate::research_executor::{execute_research, select_branch_policy, SearchBackend, WallClock};
use crate::research_packet::{build_condensed_packet, ResearchPacket};
use crate::research_router::{ResearchPlan, ResearchRouter, RouterInput};
use crate::storage::{self, ReportRecord};
use crate::vector_memory::{self, MemoryKind};

/// Filesystem locations a run reads and writes. Injected so tests can point at
/// temporary directories; the Tauri command resolves these from the app data
/// directory.
pub struct ReportPaths {
    pub db_path: PathBuf,
    pub reports_dir: PathBuf,
}

/// The research-half stages (Steps 7–11), injected into the pipeline as trait objects
/// so the spine stays offline-stubbable (`ResearchStages::stub`) while the live command
/// constructs the real adapters (`lib.rs`). Owned `Box`es rather than borrows so a caller
/// hands over one value instead of threading several lifetimes; built once per run and
/// dropped with it. The Step-9 `Clock` is deliberately *not* a member — the 30-minute
/// research budget anchors at the research phase, so `assemble_research_packet` mints a
/// fresh `WallClock` there rather than at bundle-construction time (which would let the
/// baseline scan eat into the budget).
pub struct ResearchStages {
    pub news: Box<dyn NewsSource>,
    pub filter: Box<dyn HeadlineFilter>,
    pub router: Box<dyn ResearchRouter>,
    pub search: Box<dyn SearchBackend>,
}

impl ResearchStages {
    /// The offline stub bundle: deterministic stand-ins for every research stage, so
    /// `generate_report` runs end to end against fixtures with no live keys. Used by the
    /// integration tests and any offline smoke.
    pub fn stub() -> Self {
        Self {
            news: Box::new(crate::news::StubNewsSource),
            filter: Box::new(crate::headline_filter::StubHeadlineFilter),
            router: Box::new(crate::research_router::StubResearchRouter),
            search: Box::new(crate::research_executor::StubSearchBackend),
        }
    }

    /// The live stage bundle (Steps 7–11) for one run — the single construction shared
    /// by the production command paths (`lib.rs`) and the live research smoke below, so
    /// the wiring can't drift between what ships and what the smoke validates. Tavily +
    /// GDELT + FMP Articles feed the news gather (one nested composite — Tavily first so
    /// dedup keeps the primary source's framing, the best-effort supplements after); the
    /// GPT-5-mini headline filter and the Sonnet research router are the fixed internal
    /// stages; a *second* Tavily client is the Step-9 search backend, since the
    /// composite owns the gather's Tavily by value. Every adapter carries `ctx` so each
    /// call streams a per-request tracker row. Errors here are HTTP-client build
    /// failures only.
    pub fn live(
        tavily_key: String,
        fmp_key: String,
        openai_key: String,
        anthropic_key: String,
        ctx: &std::sync::Arc<RunContext>,
    ) -> Result<Self> {
        Ok(Self {
            news: Box::new(news::CompositeNewsSource::new(
                crate::tavily::TavilyNewsSource::new(tavily_key.clone())?
                    .with_context(ctx.clone()),
                news::CompositeNewsSource::new(
                    crate::gdelt::GdeltNewsSource::new()?.with_context(ctx.clone()),
                    crate::fmp_news::FmpNewsSource::new(fmp_key)?.with_context(ctx.clone()),
                ),
            )),
            filter: Box::new(
                crate::headline_filter::ModelHeadlineFilter::new(openai_key)?
                    .with_context(ctx.clone()),
            ),
            router: Box::new(
                crate::research_router::ModelResearchRouter::new(anthropic_key)?
                    .with_context(ctx.clone()),
            ),
            search: Box::new(
                crate::tavily::TavilyNewsSource::new(tavily_key)?.with_context(ctx.clone()),
            ),
        })
    }
}

/// The result of a report run, returned to the caller (the Tauri command or a
/// test). Carries the Markdown so the frontend can render it immediately.
/// `Clone` so a scheduled run can hand the report to an open window via a
/// `job-finished` event (Tauri's `emit` requires `Clone`).
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedReport {
    pub report_id: String,
    pub markdown: String,
    pub markdown_path: String,
    pub summary: ReportSummary,
}

/// Run one manual report end to end: gather the baseline market-data scan (Step
/// 6), enforce its coverage floor, run the research half (Steps 7–11) and condense
/// it into the analyst packet, invoke the agent, write the canonical Markdown file,
/// and persist the record to SQLite. The adapters degrade partial failures to a
/// recorded gap manifest rather than aborting, so the scan itself rarely errs; the
/// `enforce_coverage` gate below is the single place a too-thin baseline fails the
/// run, which `jobs::run_job` records as a failed job (`docs/scheduling.md §Offline
/// Behavior`). The research half is fully fail-soft and never gates a run (see
/// `assemble_research_packet`).
///
/// `research` is the bundle of research-half stages (`ResearchStages`) — the live
/// command supplies the real news / filter / router / search adapters; tests and
/// offline smokes pass `ResearchStages::stub()`, keeping this the same offline-driven
/// spine the agent and data halves already are.
///
/// `embedder` is the vector-memory seam (`docs/weekly-report-workflow.md §§4, 10, 17`;
/// see `vector_memory`): the research half runs the two fail-soft retrieval pulls
/// against it (`assemble_research_packet`), and after the report record persists the
/// summary is embedded and stored best-effort. The live command supplies
/// `OpenAiEmbedder` (the fixed internal `text-embedding-3-large` stage); tests pass
/// `embedding::StubEmbedder`.
///
/// `ctx` is the run's progress/cancel context (`progress::RunContext`). Each stage
/// below brackets its work in a `step_started` / `step_finished` pair so an open
/// window can render the run live; the real data adapters, the research adapters, and
/// the streaming model adapter emit their own finer-grained per-request / token events
/// *inside* the baseline, research, and agent steps. A cancel requested mid-run is
/// observed at the boundaries between stages (`bail_if_cancelled`) — a
/// `reqwest::blocking` call already in flight is not interrupted — and surfaces as a
/// recognizable error that `jobs::run_job` maps to a Cancelled outcome. Tests and
/// offline smokes pass `RunContext::noop()`, which reports nowhere and is never
/// cancelled, so this stays the same pure spine.
pub fn generate_report(
    agent: &dyn MainAgent,
    data: &dyn MarketDataSource,
    research: &ResearchStages,
    embedder: &dyn Embedder,
    paths: &ReportPaths,
    ctx: &RunContext,
) -> Result<GeneratedReport> {
    // The app-minted "as of" time for this run's baseline: the anchor both for the
    // persisted snapshot's `captured_at` and for the elapsed interval in the change view
    // below. Deliberately distinct from the report's agent-minted `created_at` (they
    // differ by the agent's runtime); the snapshot↔report join is by `report_id`, not time.
    let as_of = chrono::Utc::now();

    // Step 3: baseline market data is gathered before agent reasoning and is not
    // optional (`docs/weekly-report-workflow.md §Step 3`). Each adapter records what it
    // couldn't resolve in `baseline.gaps` instead of failing; the coverage gate then
    // decides whether what landed clears the run's mandatory floor. A gate failure
    // propagates unwrapped — like the agent error below — so `jobs::run_job` persists
    // the floor message as the failed-job detail. The gaps that *don't* trip the floor
    // ride into `MainAgentInput` and on into the model prompt, so the agent reasons over
    // what's known-absent rather than inferring it.
    ctx.step_started("baseline", "Gathering baseline market data");
    let baseline = match data.baseline_scan() {
        Ok(baseline) => baseline,
        Err(e) => {
            ctx.step_finished("baseline", "failed", Some(e.to_string()));
            return Err(e);
        }
    };
    ctx.step_finished("baseline", "ok", None);
    bail_if_cancelled(ctx)?;

    ctx.step_started("coverage", "Checking baseline coverage");
    if let Err(e) = enforce_coverage(&baseline) {
        ctx.step_finished("coverage", "failed", Some(e.to_string()));
        return Err(e);
    }
    ctx.step_finished("coverage", "ok", None);
    bail_if_cancelled(ctx)?;

    // Compute the change view against the previous report's snapshot, and serialize this
    // run's baseline for persistence — both before `baseline` is moved into the agent
    // input. Reads/compute are best-effort (`None` on a first report or any failure); the
    // serialization is captured here because `baseline` is consumed by the agent below.
    let deltas = compute_prior_deltas(&paths.db_path, &baseline, as_of);
    let baseline_json = serde_json::to_string(&baseline).ok();

    // The bounded recent-report context for research routing — Step 8's
    // thesis-continuity input. Best-effort like the deltas read above: a first
    // report or any DB failure degrades to empty and never gates the run.
    let recent_reports = load_recent_report_context(&paths.db_path);

    // Steps 7–11: the research half — news → filter → route → execute → condensed packet,
    // fully fail-soft (`assemble_research_packet`), bracketed as one step with the adapters'
    // per-request rows streaming inside it. Computed here, before `baseline` and `deltas`
    // move into the agent input below.
    ctx.step_started("research", "Gathering and condensing research");
    let research_packet = assemble_research_packet(
        research,
        &baseline,
        deltas.as_ref(),
        &recent_reports,
        embedder,
        &paths.db_path,
        ctx,
    );
    let research_status = if ctx.is_cancelled() { "cancelled" } else { "ok" };
    ctx.step_finished("research", research_status, None);
    bail_if_cancelled(ctx)?;

    ctx.step_started("agent", "Main agent writing the report");
    let output = match agent.generate(MainAgentInput {
        baseline,
        deltas,
        research: Some(research_packet),
    }) {
        Ok(output) => output,
        Err(e) => {
            // A cancel observed mid-stream surfaces as a parse error here; mark the
            // step cancelled (not failed) so the tracker reads consistently with the
            // run's Cancelled outcome.
            let status = if ctx.is_cancelled() { "cancelled" } else { "failed" };
            ctx.step_finished("agent", status, Some(e.to_string()));
            return Err(e);
        }
    };
    ctx.step_finished("agent", "ok", None);
    bail_if_cancelled(ctx)?;

    // Persist: write the canonical Markdown file and the SQLite record. The `?`
    // ergonomics stay inside an immediately-invoked closure so any failure still
    // flips the persist step to `failed` with its reason.
    ctx.step_started("persist", "Saving the report");
    let report = (|| -> Result<GeneratedReport> {
        let summary = output.summary;
        std::fs::create_dir_all(&paths.reports_dir)
            .with_context(|| format!("creating reports directory {:?}", paths.reports_dir))?;

        let filename =
            canonical_report_filename(&summary.created_at, &summary.report_id, &chrono::Local)?;
        let markdown_path = paths.reports_dir.join(&filename);
        std::fs::write(&markdown_path, &output.markdown)
            .with_context(|| format!("writing report markdown {:?}", markdown_path))?;
        let markdown_path_str = markdown_path.to_string_lossy().into_owned();

        let conn = storage::open(&paths.db_path)?;
        storage::init_schema(&conn)?;
        let summary_json = serde_json::to_string(&summary)?;
        storage::insert_report(
            &conn,
            &ReportRecord {
                summary: &summary,
                markdown_path: &markdown_path_str,
                summary_json: &summary_json,
            },
        )
        .context("inserting report record")?;

        // Best-effort: persist this run's baseline snapshot for the next report's change
        // view, then prune to the retention cap. A snapshot or prune failure must never
        // lose a report that already generated and persisted, so errors are logged to
        // stderr and swallowed rather than propagated — a persistent schema/permission
        // fault would otherwise silently disable baseline history with no diagnostic trail.
        if let Some(json) = &baseline_json {
            if let Err(e) = storage::insert_baseline_snapshot(
                &conn,
                &summary.report_id,
                &as_of.to_rfc3339(),
                BASELINE_SCHEMA_VERSION,
                json,
            ) {
                eprintln!(
                    "baseline-snapshot persist failed for report {}: {e:#}",
                    summary.report_id
                );
            }
            if let Err(e) =
                storage::prune_baseline_snapshots(&conn, storage::BASELINE_SNAPSHOT_RETENTION)
            {
                eprintln!("baseline-snapshot prune failed: {e:#}");
            }
        }

        // Best-effort: embed the structured summary and store it in vector memory —
        // Step 17's "report summary to vector memory" leg (SQLite-backed `vector_memory`).
        // Same posture as the snapshot block above: a flaky embedding call or a store
        // failure costs this report's memory row, never the report itself, and leaves a
        // diagnostic trail on stderr (plus a `failed` tracker row from the embedder).
        //
        // Both memory legs (this one and the learnings below) poll cancellation
        // before each paid embedding call — the contract is that a run stops within
        // a request or two of a cancel (`docs/run-tracking.md §Cancellation`), so a
        // cancel landing during persist cuts the memory spend short rather than
        // riding out up to six more calls. The report persisted above and stays: a
        // cancel this late is honored as a successful run (`jobs::run_job`); only
        // the best-effort memory rows are skipped, an already-accepted state.
        if !ctx.is_cancelled() {
            let memory_text = vector_memory::summary_memory_text(&summary);
            match embedder.embed(&memory_text) {
                Ok(embedding) => {
                    if let Err(e) = vector_memory::insert_memory(
                        &conn,
                        MemoryKind::Summary,
                        Some(&summary.report_id),
                        &memory_text,
                        &embedding,
                        &summary.created_at,
                    ) {
                        eprintln!(
                            "vector-memory persist failed for report {}: {e:#}",
                            summary.report_id
                        );
                    }
                }
                Err(e) => eprintln!(
                    "vector-memory embedding failed for report {}: {e:#}",
                    summary.report_id
                ),
            }
        }

        // Best-effort: embed and store the run's durable learnings — Step 17's
        // second memory leg ("durable learnings identified by the main agent",
        // `docs/weekly-report-workflow.md §Step 17`). The bound lives here in the
        // app layer, not the model contract: entries are trimmed, empties dropped,
        // and the rest capped before any embedding call is spent. Each learning is
        // its own atomic unit (one embedding per learning, `docs/storage.md
        // §Embeddings`) and each write is independently best-effort — one failed
        // embed or insert costs that learning, never its siblings or the report.
        // `report_id` is provenance only: the `kind` column, not the id, is what
        // makes learnings survive the future retention cascade.
        let learnings: Vec<&str> = output
            .durable_learnings
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        if learnings.len() > LEARNINGS_PER_REPORT_CAP {
            eprintln!(
                "vector-memory: report {} emitted {} durable learnings; keeping the first {}",
                summary.report_id,
                learnings.len(),
                LEARNINGS_PER_REPORT_CAP
            );
        }
        for learning in learnings.into_iter().take(LEARNINGS_PER_REPORT_CAP) {
            // Polled at each request boundary, like the executor: a cancel that
            // lands during one embedding call must not spend the next.
            if ctx.is_cancelled() {
                break;
            }
            match embedder.embed(learning) {
                Ok(embedding) => {
                    if let Err(e) = vector_memory::insert_memory(
                        &conn,
                        MemoryKind::Learning,
                        Some(&summary.report_id),
                        learning,
                        &embedding,
                        &summary.created_at,
                    ) {
                        eprintln!(
                            "vector-memory learning persist failed for report {}: {e:#}",
                            summary.report_id
                        );
                    }
                }
                Err(e) => eprintln!(
                    "vector-memory learning embedding failed for report {}: {e:#}",
                    summary.report_id
                ),
            }
        }

        Ok(GeneratedReport {
            report_id: summary.report_id.clone(),
            markdown: output.markdown,
            markdown_path: markdown_path_str,
            summary,
        })
    })();
    match &report {
        Ok(_) => ctx.step_finished("persist", "ok", None),
        Err(e) => ctx.step_finished("persist", "failed", Some(e.to_string())),
    }
    report
}

/// Run the research half (Steps 7–11) and condense it into the packet the main agent
/// reasons over (`docs/weekly-report-workflow.md §§7–11`). **Fully fail-soft** — the
/// locked posture for this slice: a failure in any stage degrades that stage to empty and
/// the phase continues, so a flaky news, headline-filter, or routing call yields a thinner
/// report rather than failing the run. Only the baseline coverage floor gates a run; the
/// research half never does. (This is a conscious deviation from
/// `docs/weekly-report-workflow.md §250`, which treats a failing model call in any stage as
/// a job failure — recorded as a flag for the session-end build-spec note.)
///
/// The bounded executor is already fail-soft per query (`research_executor`) and
/// `build_condensed_packet` is pure, so this always returns a packet — never an error.
/// Per-call progress rows live inside the real adapters; cancellation is polled at each
/// stage boundary here (before the filter call, the Step-4 pull, the router call, and
/// the Step-10 pull) and at each request boundary inside the executor, so a cancel
/// requested mid-research skips the remaining model calls rather than spending them
/// before the run stops. The caller brackets this under one `research` step.
/// `baseline`, `deltas`, and `recent_reports` are borrowed (the caller still owns
/// them); the router input keeps its own clones.
///
/// Both vector-memory retrievals live here (`retrieve_memory`, fail-soft): the Step-4
/// pre-research pull — queried from the recent report context, baseline, and change
/// view — feeds only the router (ephemeral, per the doc's replace-not-merge rule);
/// the Step-10 post-research pull — queried from the executor's evidence — is the one
/// the condensed packet carries to the main agent. `embedder` is the same fixed
/// embedding stage the Step-17 persist write uses; `db_path` locates the store.
fn assemble_research_packet(
    research: &ResearchStages,
    baseline: &BaselineMarketData,
    deltas: Option<&BaselineDeltas>,
    recent_reports: &[ReportSummary],
    embedder: &dyn Embedder,
    db_path: &std::path::Path,
    ctx: &RunContext,
) -> ResearchPacket {
    // Step 7: gather raw headlines (Tavily + GDELT + FMP Articles) and run the
    // deterministic dedup pre-pass.
    let headlines = match research.news.gather() {
        Ok(raw) => news::dedupe_headlines(raw),
        Err(e) => {
            eprintln!("research: news gather degraded to empty: {e:#}");
            Vec::new()
        }
    };

    // Step 7 (filter): cluster the headlines into the ~10 important stories. An empty gather
    // has nothing to cluster, so skip the model call entirely — and a cancel requested during
    // the gather sweep short-circuits here too, so a stop never still spends the (up to ~120s)
    // GPT-5-mini call. Cooperative cancellation is polled at this stage boundary, mirroring the
    // pipeline's step boundaries; the executor below polls it at each request boundary.
    let clusters = if headlines.is_empty() || ctx.is_cancelled() {
        Vec::new()
    } else {
        match research.filter.filter(headlines) {
            Ok(clusters) => clusters,
            Err(e) => {
                eprintln!("research: headline filter degraded to empty: {e:#}");
                Vec::new()
            }
        }
    };

    // Step 4: the pre-research memory pull — recalled against where the market
    // actually is this period (recent context + baseline + change view) to steer
    // routing. Ephemeral routing input only: the packet below carries the Step-10
    // pull instead (`docs/weekly-report-workflow.md §Step 10`, replace-not-merge).
    // The cancel guard mirrors the filter's: a cancel must not spend the embedding
    // call.
    let pre_memory = if ctx.is_cancelled() {
        Vec::new()
    } else {
        retrieve_memory(
            db_path,
            embedder,
            &vector_memory::pre_research_query(recent_reports, baseline, deltas),
            "pre-research",
        )
    };

    // Step 8: route the baseline, change view, clusters, recent-report summaries, and
    // recalled memory into a bounded research plan. Cancel checkpoint before the Sonnet
    // call, mirroring the filter guard: a cancel that lands between stages must not
    // still spend the routing call.
    let plan = if ctx.is_cancelled() {
        ResearchPlan::default()
    } else {
        match research.router.route(RouterInput {
            baseline: baseline.clone(),
            deltas: deltas.cloned(),
            clusters: clusters.clone(),
            recent_reports: recent_reports.to_vec(),
            memory: pre_memory,
        }) {
            Ok(plan) => plan,
            Err(e) => {
                eprintln!("research: router degraded to empty: {e:#}");
                ResearchPlan::default()
            }
        }
    };

    // Step 9: execute the plan under the three hard bounds (`research_executor`), with the
    // change view's delta-rules driving any depth-2 follow-ups (`select_branch_policy`). The
    // clock anchors here so the 30-minute budget covers the research phase, not the baseline
    // scan that preceded it.
    let clock = WallClock::new();
    let policy = select_branch_policy(deltas);
    let evidence = execute_research(&plan, research.search.as_ref(), policy.as_ref(), &clock, ctx);

    // Step 10: the post-research memory pull — recalled against what the research
    // actually found, and the only memory the packet carries forward. Same cancel
    // guard as the stages above.
    let memory = if ctx.is_cancelled() {
        Vec::new()
    } else {
        retrieve_memory(
            db_path,
            embedder,
            &vector_memory::post_research_query(&evidence),
            "post-research",
        )
    };

    // Step 11: condense everything into the token-bounded packet.
    build_condensed_packet(baseline.clone(), deltas.cloned(), clusters, evidence, memory)
}

/// Cancel checkpoint between pipeline stages. A cancel requested mid-run lands
/// here: the run stops with a recognizable error that `jobs::run_job` reclassifies
/// as a Cancelled outcome (keyed off the same shared cancel flag) rather than a
/// Failed one. An in-flight HTTP request is never interrupted — the cancel takes
/// effect at the next boundary.
fn bail_if_cancelled(ctx: &RunContext) -> Result<()> {
    if ctx.is_cancelled() {
        bail!("run cancelled before completion");
    }
    Ok(())
}

/// Best-effort change view for this run: read the previous report's baseline snapshot and
/// diff this run's scan against it (`baseline_delta::compute_deltas`), anchored on the
/// elapsed interval `as_of − captured_at`. Returns `None` — and the run proceeds without
/// deltas — for the first report, any DB error, or a prior blob that won't decode. The
/// deltas are additive context, never a gate, so every failure degrades to `None` rather
/// than propagating.
fn compute_prior_deltas(
    db_path: &std::path::Path,
    current: &BaselineMarketData,
    as_of: chrono::DateTime<chrono::Utc>,
) -> Option<BaselineDeltas> {
    let conn = storage::open(db_path).ok()?;
    storage::init_schema(&conn).ok()?;
    let (captured_at, baseline_json) = storage::latest_baseline_snapshot(&conn).ok()??;
    let prior: BaselineMarketData = serde_json::from_str(&baseline_json).ok()?;
    let prior_at = chrono::DateTime::parse_from_rfc3339(&captured_at)
        .ok()?
        .with_timezone(&chrono::Utc);
    let elapsed_days = (as_of - prior_at).num_seconds() as f64 / 86_400.0;
    Some(baseline_delta::compute_deltas(current, &prior, elapsed_days))
}

/// Bounded count of recent report summaries handed to research routing — the
/// "recent Markdown report context" input of `docs/weekly-report-workflow.md
/// §Step 8`; §Step 2 requires only "a bounded set". Three reports ≈ three weeks
/// of thesis arc at negligible prompt cost.
const ROUTER_RECENT_REPORTS: u32 = 3;

/// Best-effort recent-report context for this run: the most recent prior report
/// summaries, newest first (`storage::list_recent_reports`). The structured
/// summary carries the continuity signal routing needs — stance, key risks,
/// unresolved questions, forward themes — without the full Markdown bodies (those
/// belong to the Step-2 main-agent context, a later slice). Additive context like
/// the change view above, never a gate: a first report or any DB failure degrades
/// to empty rather than propagating.
fn load_recent_report_context(db_path: &std::path::Path) -> Vec<ReportSummary> {
    let read = || -> Result<Vec<ReportSummary>> {
        let conn = storage::open(db_path)?;
        storage::init_schema(&conn)?;
        storage::list_recent_reports(&conn, ROUTER_RECENT_REPORTS)
    };
    read().unwrap_or_else(|e| {
        eprintln!("research: recent-report context degraded to empty: {e:#}");
        Vec::new()
    })
}

/// Bounded count of fragments each vector-memory pull recalls (Steps 4 and 10).
/// Selectivity comes from top-k over a small corpus (≤30 summaries plus
/// learnings) — no similarity floor; threshold tuning is deferred alongside the
/// brancher's ("Vector memory is used selectively",
/// `docs/weekly-report-workflow.md §Step 4`).
const MEMORY_TOP_K: usize = 5;

/// Per-report cap on durable-learning rows the Step-17 persist write accepts.
/// An app-layer bound, like the executor's limits — the prompt asks the model to
/// self-bound ("never more than five"), but the store's growth must not depend
/// on the model honoring prose; learnings are never deleted, so overflow here is
/// permanent. Not doc-pinned: tunable alongside `MEMORY_TOP_K` and the brancher
/// thresholds. Overflow is truncated with a stderr note, never a failed report.
const LEARNINGS_PER_REPORT_CAP: usize = 5;

/// Hard byte cap on a retrieval query before the paid embedding call. The query
/// builders bound line *counts*, not sizes — the stored summaries' optional
/// arrays, topic rationales, and web source titles are all length-unbounded — so
/// a pathological query would draw a 400 from the embedding API and cost the
/// whole pull. Truncating costs only the recall tail instead. The cap is in
/// *bytes* because that is what makes the guarantee provable without a tokenizer
/// dependency: the embedding model's tokenizer is a byte-level BPE, where every
/// token consumes at least one input byte, so `tokens ≤ bytes` always — 8,000
/// bytes can never exceed the 8,192-token input limit. (A char cap cannot
/// promise this: a multi-byte char can fall back to several byte tokens.)
const MEMORY_QUERY_MAX_BYTES: usize = 8_000;

/// `query` cut to at most `max_bytes` bytes, backed off to a char boundary so
/// the slice can never split a multi-byte character.
fn bounded_query(query: &str, max_bytes: usize) -> &str {
    if query.len() <= max_bytes {
        return query;
    }
    let mut end = max_bytes;
    while !query.is_char_boundary(end) {
        end -= 1;
    }
    &query[..end]
}

/// One best-effort vector-memory pull: embed the query text, search the store
/// across both kinds, and return the hits in their shared prompt form, most
/// relevant first. Additive context like the reads above, never a gate — any
/// failure (an unopenable store, a flaky embedding call) degrades to empty with
/// a stderr note (`label` names which pull). Two cheap guards keep early runs
/// free: an empty query has nothing to recall against, and an empty store can't
/// return hits — both skip the paid embedding call entirely. The query is capped
/// at [`MEMORY_QUERY_MAX_BYTES`] before embedding, so an oversized one is
/// truncated rather than rejected by the provider and lost.
fn retrieve_memory(
    db_path: &std::path::Path,
    embedder: &dyn Embedder,
    query: &str,
    label: &str,
) -> Vec<String> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let pull = || -> Result<Vec<String>> {
        let conn = storage::open(db_path)?;
        storage::init_schema(&conn)?;
        if vector_memory::count_memory(&conn)? == 0 {
            return Ok(Vec::new());
        }
        let embedding = embedder.embed(bounded_query(query, MEMORY_QUERY_MAX_BYTES))?;
        let hits = vector_memory::search_memory(&conn, &embedding, None, MEMORY_TOP_K)?;
        Ok(hits.iter().map(vector_memory::MemoryHit::prompt_fragment).collect())
    };
    pull().unwrap_or_else(|e| {
        eprintln!("research: {label} memory retrieval degraded to empty: {e:#}");
        Vec::new()
    })
}

/// The minimum fraction of a Step-3 group's *expected* series that must resolve for the
/// group to count as present for the coverage floor. "Expected" excludes permanently
/// out-of-scope items (premium / discontinued series), so a series a deployment never had
/// doesn't drag the ratio down — only this-run failures (`Unavailable` / `Rejected` /
/// `Malformed` gaps) count against it. 0.6 ≈ three of four indices, or ~60% of a posture
/// group.
const COVERAGE_FLOOR: f64 = 0.6;

/// One group's coverage: resolved series over expected (resolved + this-run gaps).
/// Permanent (`OutOfScope`) gaps are excluded from the denominator. An expected count of
/// zero reads as fully covered, but `clears_floor` additionally requires a non-empty
/// group, so a vacuous 1.0 over zero series can't satisfy the floor.
fn group_coverage(data: &BaselineMarketData, group: GroupKind) -> f64 {
    let present = group_present_count(data, group);
    let missing_this_run = data
        .gaps
        .iter()
        .filter(|g| g.group == group && g.reason.counts_against_coverage())
        .count();
    let expected = present + missing_this_run;
    if expected == 0 {
        1.0
    } else {
        present as f64 / expected as f64
    }
}

/// How many series resolved in `group`.
fn group_present_count(data: &BaselineMarketData, group: GroupKind) -> usize {
    match group {
        GroupKind::Indices => data.indices.len(),
        GroupKind::Internals => data.internals.len(),
        GroupKind::Sectors => data.sectors.len(),
        GroupKind::MacroLevels => data.macro_levels.len(),
        GroupKind::LaborLevels => data.labor_levels.len(),
        GroupKind::Calendar => data.calendar.len(),
        GroupKind::IndexPerformance => data.index_performance.len(),
        GroupKind::Movers => data.movers.len(),
        GroupKind::Earnings => data.earnings.len(),
        GroupKind::SectorPe => data.sector_pe.len(),
        GroupKind::Industries => data.industries.len(),
        GroupKind::MarketRiskPremium => data.market_risk_premium.len(),
    }
}

/// Whether `group` clears the coverage floor: at least one resolved series *and* a
/// coverage ratio of at least `COVERAGE_FLOOR`. The non-empty requirement guards the
/// vacuous "0 of 0 = 100%" case — a floor group always has a fixed expected set, so zero
/// resolved is a failure regardless of the ratio.
fn clears_floor(data: &BaselineMarketData, group: GroupKind) -> bool {
    group_present_count(data, group) > 0 && group_coverage(data, group) >= COVERAGE_FLOOR
}

/// The Step-3 coverage gate — the single, centralized replacement for the per-adapter
/// completeness floors. The adapters now degrade every failure to a recorded gap; this
/// is the one place a too-thin merged baseline fails the run. The floor (resolved in
/// planning): the report is structurally impossible without the Index Picture, and its
/// risk-posture / market-cycle reads need at least one grounded macro/internals group —
/// so `indices` must clear, AND at least one of {`internals`, `macro_levels`} must clear.
/// Everything else (sectors, labor, calendar, index performance, and whichever of
/// internals/macro didn't clear) degrades to a manifest gap the agent reasons over. A
/// failure here propagates like any Step-3 error, which `jobs::run_job` records as a
/// failed job.
pub fn enforce_coverage(data: &BaselineMarketData) -> Result<()> {
    if !clears_floor(data, GroupKind::Indices) {
        bail!(
            "Step-3 baseline below floor: the index picture is missing — indices coverage \
             {:.0}% (need {:.0}%). The report can't be written without the Dow / S&P / Nasdaq \
             reads; the data provider is unreachable, rejecting the key, or rate-limited.",
            group_coverage(data, GroupKind::Indices) * 100.0,
            COVERAGE_FLOOR * 100.0
        );
    }
    if !clears_floor(data, GroupKind::Internals) && !clears_floor(data, GroupKind::MacroLevels) {
        bail!(
            "Step-3 baseline below floor: neither market internals ({:.0}%) nor macro levels \
             ({:.0}%) reached {:.0}% — the risk-posture / market-cycle reads would be \
             ungrounded. FRED is unreachable, rejecting the key, or rate-limited.",
            group_coverage(data, GroupKind::Internals) * 100.0,
            group_coverage(data, GroupKind::MacroLevels) * 100.0,
            COVERAGE_FLOOR * 100.0
        );
    }
    Ok(())
}

/// List the most recent reports (newest first), capped at the retention display
/// limit. The Tauri `list_reports` command is a thin wrapper over this.
pub fn list_reports(paths: &ReportPaths) -> Result<Vec<ReportSummary>> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;
    storage::list_recent_reports(&conn, storage::RECENT_REPORTS_LIMIT)
}

/// Load one persisted report by id for display: its summary from SQLite and its
/// canonical Markdown read back from disk. An unknown id, or a Markdown file
/// removed out-of-band, surfaces as a typed error the UI renders rather than a
/// panic.
pub fn load_report(paths: &ReportPaths, report_id: &str) -> Result<GeneratedReport> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;
    let (markdown_path, summary) = storage::get_report_record(&conn, report_id)?
        .with_context(|| format!("no report with id {report_id}"))?;
    let markdown = std::fs::read_to_string(&markdown_path)
        .with_context(|| format!("reading report markdown {markdown_path:?}"))?;
    Ok(GeneratedReport {
        report_id: summary.report_id.clone(),
        markdown,
        markdown_path,
        summary,
    })
}

/// Export one report's canonical Markdown to a user-chosen destination
/// (`docs/export.md`). Tauri-free so it sits behind the thin `export_report_markdown`
/// command the same way `list_reports`/`load_report` do, and is driveable from a
/// test against temp dirs.
///
/// Reads from the stored artifacts only — the canonical `.md` on disk, located via
/// the SQLite record — so an export never re-runs the workflow and never trusts an
/// in-memory copy (`docs/export.md §Export Behavior`). An unknown id, a Markdown
/// file removed out-of-band, or a write failure surfaces as a typed error rather
/// than a panic, mirroring `load_report`.
pub fn export_markdown_to(
    paths: &ReportPaths,
    report_id: &str,
    dest: &std::path::Path,
) -> Result<()> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;
    let (markdown_path, _summary) = storage::get_report_record(&conn, report_id)?
        .with_context(|| format!("no report with id {report_id}"))?;
    let markdown = std::fs::read_to_string(&markdown_path)
        .with_context(|| format!("reading report markdown {markdown_path:?}"))?;
    std::fs::write(dest, &markdown)
        .with_context(|| format!("writing exported markdown {dest:?}"))?;
    Ok(())
}

/// Build the canonical Markdown filename for a report:
/// `YYYY-MM-DD-market-signal-weekly-report-<id8>.md`.
///
/// Split out as a pure, timezone-injectable function so the two decisions it
/// encodes are unit-testable without the system clock or the filesystem:
///
/// - **Local date segment.** The report is a local-time artifact (the scheduled
///   window is Sunday 9 AM local — see `docs/scheduling.md`), so the filename a
///   user reads matches their wall clock, even though the `created_at` persisted
///   in SQLite stays canonical UTC. `tz` names the zone whose calendar date
///   labels the file; production passes `chrono::Local`, tests pass a fixed
///   offset so the midnight-boundary behavior is deterministic.
/// - **Unique per-run suffix.** The first 8 characters of the `report_id` UUID
///   make every run's file distinct, so a same-date rerun no longer overwrites
///   an earlier run's Markdown (the two-rows-one-file case from slice 1).
///
/// A non-RFC3339 `created_at` surfaces as a typed error here rather than
/// panicking on a byte slice.
fn canonical_report_filename<Tz: chrono::TimeZone>(
    created_at: &str,
    report_id: &str,
    tz: &Tz,
) -> Result<String>
where
    Tz::Offset: std::fmt::Display,
{
    let local_date = local_date_segment(created_at, tz)?;
    let id8 = report_id.get(..8).unwrap_or(report_id);
    Ok(format!("{local_date}-market-signal-weekly-report-{id8}.md"))
}

/// Build the export filename a user sees in the Save dialog
/// (`docs/export.md §Export Naming`): `YYYY-MM-DD-market-signal-weekly-report.<ext>`.
///
/// Deliberately distinct from `canonical_report_filename`: the spec's export name
/// carries **no `-<id8>` suffix** — same-name collisions are the user's own save
/// dialog overwrite prompt, not ours. `ext` is the bare extension (`"md"`, `"pdf"`).
/// Shares the local-date logic so an export's date matches the stored file's date.
pub fn export_basename<Tz: chrono::TimeZone>(
    created_at: &str,
    ext: &str,
    tz: &Tz,
) -> Result<String>
where
    Tz::Offset: std::fmt::Display,
{
    let local_date = local_date_segment(created_at, tz)?;
    Ok(format!("{local_date}-market-signal-weekly-report.{ext}"))
}

/// The `YYYY-MM-DD` local-date segment shared by the canonical filename and the
/// export basename: parse the canonical-UTC `created_at` and render it in `tz`'s
/// calendar (`docs/scheduling.md` — reports are local-time artifacts). A
/// non-RFC3339 stamp is a typed error, not a panic on a byte slice.
fn local_date_segment<Tz: chrono::TimeZone>(created_at: &str, tz: &Tz) -> Result<String>
where
    Tz::Offset: std::fmt::Display,
{
    Ok(chrono::DateTime::parse_from_rfc3339(created_at)
        .with_context(|| format!("agent supplied a non-RFC3339 created_at: {created_at:?}"))?
        .with_timezone(tz)
        .format("%Y-%m-%d")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    use crate::data_sources::{DataGap, GapReason, Quote};

    // ---- Step-3 coverage gate ----

    /// `n` placeholder resolved quotes for a group.
    fn covq(n: usize) -> Vec<Quote> {
        (0..n)
            .map(|i| Quote {
                symbol: format!("S{i}"),
                name: format!("S{i}"),
                price: 1.0,
                change_pct: 0.0,
                unit: "x".into(),
            })
            .collect()
    }

    /// `n` gaps of one reason for `group`.
    fn covgaps(group: GroupKind, reason: GapReason, n: usize) -> Vec<DataGap> {
        (0..n)
            .map(|_| DataGap::new(group, "id", "name", reason))
            .collect()
    }

    #[test]
    fn enforce_coverage_passes_a_full_baseline() {
        let data = BaselineMarketData {
            indices: covq(4),
            internals: covq(9),
            macro_levels: covq(17),
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_fails_without_the_index_picture() {
        // Every index gapped (a rejected FMP key) -> the report can't be written.
        let data = BaselineMarketData {
            internals: covq(9),
            macro_levels: covq(17),
            gaps: covgaps(GroupKind::Indices, GapReason::Unavailable, 4),
            ..Default::default()
        };
        let err = enforce_coverage(&data).unwrap_err().to_string();
        assert!(err.contains("index picture"), "{err}");
    }

    #[test]
    fn enforce_coverage_fails_without_posture_grounding() {
        // Indices present, but both internals and macro fully gone (a rejected FRED key)
        // -> risk-posture / market-cycle would be ungrounded.
        let mut gaps = covgaps(GroupKind::Internals, GapReason::Rejected, 9);
        gaps.extend(covgaps(GroupKind::MacroLevels, GapReason::Rejected, 17));
        let data = BaselineMarketData {
            indices: covq(4),
            gaps,
            ..Default::default()
        };
        let err = enforce_coverage(&data).unwrap_err().to_string();
        assert!(err.contains("macro levels") && err.contains("internals"), "{err}");
    }

    #[test]
    fn enforce_coverage_passes_when_one_posture_group_degrades_above_floor() {
        // Internals 6 of 9 (0.67 >= floor) clears even though macro is gone.
        let mut gaps = covgaps(GroupKind::Internals, GapReason::Unavailable, 3);
        gaps.extend(covgaps(GroupKind::MacroLevels, GapReason::Unavailable, 17));
        let data = BaselineMarketData {
            indices: covq(4),
            internals: covq(6),
            gaps,
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_passes_on_macro_when_internals_is_gone() {
        // Internals fully gapped but macro intact -> the posture-grounding clause is met
        // by macro alone.
        let data = BaselineMarketData {
            indices: covq(4),
            macro_levels: covq(17),
            gaps: covgaps(GroupKind::Internals, GapReason::Unavailable, 9),
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_excludes_out_of_scope_from_the_floor() {
        // Russell permanently premium: 3 resolved + 1 OutOfScope gap -> 3/3 = 100%, so a
        // permanent absence never fails the floor (the whole reason OutOfScope is split
        // from the this-run reasons).
        let data = BaselineMarketData {
            indices: covq(3),
            internals: covq(9),
            macro_levels: covq(17),
            gaps: covgaps(GroupKind::Indices, GapReason::OutOfScope, 1),
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_ignores_additive_groups() {
        // movers / earnings and the valuation + finer-rotation groups (sector-PE,
        // industries, market-risk-premium) are all additive, non-floor: empty groups and
        // even this-run gaps in them never gate a run whose floor groups (indices + a
        // posture group) are covered.
        let mut gaps = covgaps(GroupKind::Movers, GapReason::Unavailable, 3);
        gaps.extend(covgaps(GroupKind::Earnings, GapReason::Rejected, 1));
        gaps.extend(covgaps(GroupKind::SectorPe, GapReason::Unavailable, 1));
        gaps.extend(covgaps(GroupKind::Industries, GapReason::Malformed, 2));
        gaps.extend(covgaps(GroupKind::MarketRiskPremium, GapReason::Rejected, 1));
        let data = BaselineMarketData {
            indices: covq(4),
            internals: covq(9),
            // movers / earnings / sector_pe / industries / market_risk_premium left empty
            gaps,
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_ok());
    }

    #[test]
    fn enforce_coverage_fails_when_indices_below_floor() {
        // 2 resolved + 2 this-run gaps = 50% < 60%.
        let data = BaselineMarketData {
            indices: covq(2),
            internals: covq(9),
            macro_levels: covq(17),
            gaps: covgaps(GroupKind::Indices, GapReason::Unavailable, 2),
            ..Default::default()
        };
        assert!(enforce_coverage(&data).is_err());
    }

    /// UTC-3 so a late-evening UTC stamp lands on the *previous* local day —
    /// pins that the filename date is the local calendar date, not the UTC one.
    fn minus_three() -> FixedOffset {
        FixedOffset::west_opt(3 * 3600).unwrap()
    }

    #[test]
    fn filename_uses_local_calendar_date_across_a_midnight_boundary() {
        // 01:30 UTC on the 3rd is 22:30 on the 2nd at UTC-3.
        let name =
            canonical_report_filename("2026-06-03T01:30:00Z", "1ca71d1f-aaaa", &minus_three())
                .unwrap();
        assert!(
            name.starts_with("2026-06-02-market-signal-weekly-report-"),
            "expected the local (UTC-3) date 2026-06-02, got {name}"
        );
    }

    #[test]
    fn same_date_distinct_report_ids_produce_distinct_filenames() {
        let tz = minus_three();
        let a = canonical_report_filename("2026-06-03T12:00:00Z", "aaaaaaaa-1111", &tz).unwrap();
        let b = canonical_report_filename("2026-06-03T15:00:00Z", "bbbbbbbb-2222", &tz).unwrap();
        assert_ne!(a, b, "same-date reruns must not collide on one filename");
        assert_eq!(a, "2026-06-03-market-signal-weekly-report-aaaaaaaa.md");
        assert_eq!(b, "2026-06-03-market-signal-weekly-report-bbbbbbbb.md");
    }

    #[test]
    fn short_report_id_does_not_panic_on_the_eight_char_slice() {
        // A test-style id shorter than 8 chars must not panic on `get(..8)`.
        let name = canonical_report_filename("2026-06-03T12:00:00Z", "rid", &minus_three()).unwrap();
        assert!(name.ends_with("-rid.md"), "got {name}");
    }

    #[test]
    fn non_rfc3339_created_at_is_a_typed_error() {
        let err = canonical_report_filename("not-a-timestamp", "rid", &minus_three()).unwrap_err();
        assert!(err.to_string().contains("non-RFC3339"), "{err}");
    }

    #[test]
    fn export_basename_has_no_id_suffix_and_carries_the_extension() {
        // The spec's export name (docs/export.md §Export Naming) is suffix-free,
        // distinct from the internal canonical filename's `-<id8>` segment.
        let md = export_basename("2026-06-03T12:00:00Z", "md", &minus_three()).unwrap();
        assert_eq!(md, "2026-06-03-market-signal-weekly-report.md");
        let pdf = export_basename("2026-06-03T12:00:00Z", "pdf", &minus_three()).unwrap();
        assert_eq!(pdf, "2026-06-03-market-signal-weekly-report.pdf");
    }

    #[test]
    fn export_basename_uses_local_calendar_date_across_a_midnight_boundary() {
        // Shares local_date_segment with the canonical filename: 01:30 UTC on the
        // 3rd is 22:30 on the 2nd at UTC-3, so the export date is the local one.
        let name = export_basename("2026-06-03T01:30:00Z", "md", &minus_three()).unwrap();
        assert_eq!(name, "2026-06-02-market-signal-weekly-report.md");
    }

    #[test]
    fn export_basename_non_rfc3339_created_at_is_a_typed_error() {
        let err = export_basename("not-a-timestamp", "md", &minus_three()).unwrap_err();
        assert!(err.to_string().contains("non-RFC3339"), "{err}");
    }

    // ---- research half: fully fail-soft assembly (`assemble_research_packet`) ----

    use crate::headline_filter::{HeadlineCluster, StubHeadlineFilter};
    use crate::news::{RawHeadline, StubNewsSource};
    use crate::research_executor::StubSearchBackend;
    use crate::research_router::StubResearchRouter;

    /// A news source that always errors, to drive the news-gather fail-soft arm.
    struct FailingNews;
    impl NewsSource for FailingNews {
        fn gather(&self) -> anyhow::Result<Vec<RawHeadline>> {
            anyhow::bail!("news source down")
        }
    }

    /// A news source that returns no headlines, to drive the empty-gather short-circuit.
    struct EmptyNews;
    impl NewsSource for EmptyNews {
        fn gather(&self) -> anyhow::Result<Vec<RawHeadline>> {
            Ok(Vec::new())
        }
    }

    /// A filter that panics if called — proves the filter is skipped on an empty gather.
    struct PanicFilter;
    impl HeadlineFilter for PanicFilter {
        fn filter(&self, _: Vec<RawHeadline>) -> anyhow::Result<Vec<HeadlineCluster>> {
            panic!("filter must not be called when there are no headlines")
        }
    }

    /// A filter that always errors, to drive the filter fail-soft arm.
    struct FailingFilter;
    impl HeadlineFilter for FailingFilter {
        fn filter(&self, _: Vec<RawHeadline>) -> anyhow::Result<Vec<HeadlineCluster>> {
            anyhow::bail!("filter down")
        }
    }

    /// A router that always errors, to drive the router fail-soft arm.
    struct FailingRouter;
    impl ResearchRouter for FailingRouter {
        fn route(&self, _: RouterInput) -> anyhow::Result<ResearchPlan> {
            anyhow::bail!("router down")
        }
    }

    /// A router that panics if called — proves routing is skipped under cancellation.
    struct PanicRouter;
    impl ResearchRouter for PanicRouter {
        fn route(&self, _: RouterInput) -> anyhow::Result<ResearchPlan> {
            panic!("router must not be called after a cancel")
        }
    }

    use crate::embedding::StubEmbedder;

    fn assemble_with(stages: ResearchStages) -> ResearchPacket {
        let dir = tempfile::tempdir().unwrap();
        assemble_research_packet(
            &stages,
            &BaselineMarketData::default(),
            None,
            &[],
            &StubEmbedder,
            &dir.path().join("market_signal.db"),
            &RunContext::noop(),
        )
    }

    #[test]
    fn recent_report_context_degrades_to_empty_on_an_unopenable_db() {
        // A db path whose parent directory doesn't exist can't be opened or created;
        // the best-effort read must degrade to empty rather than erroring.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-subdir").join("market_signal.db");
        assert!(load_recent_report_context(&path).is_empty());
    }

    // ---- vector-memory retrieval pulls (`retrieve_memory`, Steps 4 / 10) ----

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// An embedder that counts its calls before delegating to the stub, so the
    /// tests can assert the paid call was (not) spent.
    struct CountingEmbedder(AtomicUsize);

    impl CountingEmbedder {
        fn new() -> Self {
            Self(AtomicUsize::new(0))
        }
        fn calls(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    impl Embedder for CountingEmbedder {
        fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            StubEmbedder.embed(text)
        }
    }

    /// An embedder that always errors, to drive the pull's fail-soft arm.
    struct FailingEmbedder;
    impl Embedder for FailingEmbedder {
        fn embed(&self, _: &str) -> anyhow::Result<Vec<f32>> {
            anyhow::bail!("embeddings down")
        }
    }

    /// A temp store seeded with one learning row (stub-embedded), plus the dir
    /// guard keeping it alive.
    fn seeded_store() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("market_signal.db");
        let conn = storage::open(&path).unwrap();
        storage::init_schema(&conn).unwrap();
        let embedding = StubEmbedder.embed("breadth divergence learning").unwrap();
        vector_memory::insert_memory(
            &conn,
            vector_memory::MemoryKind::Learning,
            None,
            "Breadth divergences preceded the pullback.",
            &embedding,
            "2026-05-21T13:00:00Z",
        )
        .unwrap();
        (dir, path)
    }

    #[test]
    fn retrieve_memory_returns_prompt_fragments_from_a_seeded_store() {
        let (_dir, path) = seeded_store();
        let hits = retrieve_memory(&path, &StubEmbedder, "breadth and positioning", "test");
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0],
            "[learning · 2026-05-21T13:00:00Z] Breadth divergences preceded the pullback."
        );
    }

    #[test]
    fn retrieve_memory_degrades_to_empty_on_an_unopenable_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-subdir").join("market_signal.db");
        assert!(retrieve_memory(&path, &StubEmbedder, "real query", "test").is_empty());
    }

    #[test]
    fn retrieve_memory_degrades_to_empty_on_an_embedding_failure() {
        let (_dir, path) = seeded_store();
        assert!(retrieve_memory(&path, &FailingEmbedder, "real query", "test").is_empty());
    }

    /// An embedder that records the text it was handed before delegating to the
    /// stub, so the cap test can assert what actually went over the wire.
    struct RecordingEmbedder(Mutex<Option<String>>);

    impl Embedder for RecordingEmbedder {
        fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
            *self.0.lock().unwrap() = Some(text.to_string());
            StubEmbedder.embed(text)
        }
    }

    #[test]
    fn retrieve_memory_caps_an_oversized_query_instead_of_losing_the_pull() {
        let (_dir, path) = seeded_store();
        // The cap is in bytes (tokens ≤ bytes for a byte-level BPE, so the byte cap
        // is what guarantees the provider's token limit). The leading ASCII char
        // shifts every 2-byte `é` onto an odd offset, so the cap lands mid-char and
        // the cut must back off to the previous boundary rather than split it.
        let oversized = format!("a{}", "é".repeat(MEMORY_QUERY_MAX_BYTES));
        let recording = RecordingEmbedder(Mutex::new(None));
        let hits = retrieve_memory(&path, &recording, &oversized, "test");
        assert_eq!(hits.len(), 1, "the capped query still pulls");
        let seen = recording.0.lock().unwrap().clone().expect("embedder was called");
        assert!(seen.len() <= MEMORY_QUERY_MAX_BYTES, "byte cap respected");
        assert_eq!(
            seen.len(),
            MEMORY_QUERY_MAX_BYTES - 1,
            "the mid-char cut backed off to the previous char boundary"
        );

        // A query inside the cap passes through untouched.
        assert_eq!(bounded_query("short", MEMORY_QUERY_MAX_BYTES), "short");
    }

    #[test]
    fn retrieve_memory_skips_the_paid_call_on_an_empty_store_or_empty_query() {
        // Empty store: the count guard short-circuits before embedding.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("market_signal.db");
        let counting = CountingEmbedder::new();
        assert!(retrieve_memory(&path, &counting, "real query", "test").is_empty());
        assert_eq!(counting.calls(), 0, "no embedding call against an empty store");

        // Empty query: nothing to recall against, even with a populated store.
        let (_dir2, seeded) = seeded_store();
        let counting = CountingEmbedder::new();
        assert!(retrieve_memory(&seeded, &counting, "  \n", "test").is_empty());
        assert_eq!(counting.calls(), 0, "no embedding call for a blank query");
    }

    #[test]
    fn assemble_packet_happy_path_with_stubs_carries_news_and_evidence() {
        let packet = assemble_with(ResearchStages::stub());
        assert!(!packet.news_clusters.is_empty(), "stub chain yields clusters");
        assert!(!packet.research.items.is_empty(), "stub chain yields evidence");
    }

    #[test]
    fn assemble_packet_degrades_to_empty_on_news_failure() {
        // A failed gather degrades the whole news half to empty without erroring; the run
        // still gets a (bare) packet rather than failing.
        let packet = assemble_with(ResearchStages {
            news: Box::new(FailingNews),
            filter: Box::new(StubHeadlineFilter),
            router: Box::new(StubResearchRouter),
            search: Box::new(StubSearchBackend),
        });
        assert!(packet.news_clusters.is_empty());
        assert!(packet.research.items.is_empty());
    }

    #[test]
    fn assemble_packet_skips_the_filter_when_no_headlines() {
        // EmptyNews → no headlines → the filter must not be called (PanicFilter would blow).
        let packet = assemble_with(ResearchStages {
            news: Box::new(EmptyNews),
            filter: Box::new(PanicFilter),
            router: Box::new(StubResearchRouter),
            search: Box::new(StubSearchBackend),
        });
        assert!(packet.news_clusters.is_empty());
    }

    #[test]
    fn assemble_packet_degrades_to_empty_on_filter_failure() {
        let packet = assemble_with(ResearchStages {
            news: Box::new(StubNewsSource),
            filter: Box::new(FailingFilter),
            router: Box::new(StubResearchRouter),
            search: Box::new(StubSearchBackend),
        });
        assert!(packet.news_clusters.is_empty(), "a failed filter yields no clusters");
        assert!(packet.research.items.is_empty(), "and so no routed evidence");
    }

    #[test]
    fn assemble_packet_keeps_news_when_only_the_router_fails() {
        // The router failing must not discard the clusters already gathered — the packet
        // still carries news for the agent even when deep research can't be planned. This is
        // the partial-degradation guarantee the fully-fail-soft posture buys.
        let packet = assemble_with(ResearchStages {
            news: Box::new(StubNewsSource),
            filter: Box::new(StubHeadlineFilter),
            router: Box::new(FailingRouter),
            search: Box::new(StubSearchBackend),
        });
        assert!(!packet.news_clusters.is_empty(), "news survives a router failure");
        assert!(packet.research.items.is_empty(), "but there is no routed evidence");
    }

    /// An embedder that panics if called — proves the memory pulls are skipped
    /// under cancellation.
    struct PanicEmbedder;
    impl Embedder for PanicEmbedder {
        fn embed(&self, _: &str) -> anyhow::Result<Vec<f32>> {
            panic!("embedder must not be called after a cancel")
        }
    }

    #[test]
    fn assemble_packet_skips_model_stages_when_cancelled() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        use crate::progress::{NoopReporter, RunContext};

        // A cancel requested during the gather must skip the filter, the memory pulls,
        // and the router model calls — PanicFilter / PanicEmbedder / PanicRouter would
        // blow if reached — so a stop never still spends the model calls. StubNewsSource
        // returns headlines regardless of the flag, so the filter guard is exercised on
        // the cancellation, not on an empty gather; the seeded store plus a prior
        // summary make the pre-research pull's query and store both non-empty, so only
        // the cancel guard stands between the run and the panicking embedder.
        let stages = ResearchStages {
            news: Box::new(StubNewsSource),
            filter: Box::new(PanicFilter),
            router: Box::new(PanicRouter),
            search: Box::new(StubSearchBackend),
        };
        let recent = vec![ReportSummary {
            report_id: "rep-1".into(),
            report_type: "weekly_market".into(),
            created_at: "2026-06-04T13:00:00Z".into(),
            risk_posture: RiskPosture::Mixed,
            market_cycle: MarketCycle::LateCycle,
            thesis_stance: ThesisStance::Uncertain,
            header_summary_bullets: vec!["Breadth stayed thin.".into()],
            key_risks: vec![],
            unresolved_questions: vec![],
            forward_outlook_themes: vec![],
        }];
        let (_dir, db_path) = seeded_store();
        let ctx = RunContext::new("t", Arc::new(NoopReporter), Arc::new(AtomicBool::new(true)));
        let packet = assemble_research_packet(
            &stages,
            &BaselineMarketData::default(),
            None,
            &recent,
            &PanicEmbedder,
            &db_path,
            &ctx,
        );
        assert!(packet.news_clusters.is_empty());
        assert!(packet.research.items.is_empty());
        assert!(packet.memory.is_empty());
    }

    // ---- live research-half smoke (news → filter → route → execute → packet) ----

    /// Live end-to-end smoke for the research half exactly as `generate_report` runs it:
    /// the production stage bundle (`ResearchStages::live` — the same constructor both
    /// command paths call), the real Tavily+GDELT+FMP-Articles gather, the GPT-5-mini
    /// filter, the Sonnet router, and the bounded executor against live Tavily,
    /// condensed into the packet. Everything in this path is fail-soft — a dead key or a
    /// down provider degrades to an *empty* packet rather than an error — so every
    /// assertion below is anti-vacuous: it pins that each stage actually ran and
    /// produced something, which a bare "returns a packet" check could never show.
    ///
    /// Spend per run: ~9 news calls (7 Tavily topics + 1 GDELT + 1 FMP articles page)
    /// plus up to 20 executor searches on Tavily (5 topics × 4 queries; `deltas: None`
    /// selects `NoBranch`, so no follow-ups), one OpenAI call, one Anthropic call — run
    /// it deliberately, not repeatedly. A failed GDELT row is expected from a dev IP
    /// (escalating 429 lockout) and is absorbed by the news group's other rows.
    #[test]
    #[ignore = "hits live Tavily + GDELT + FMP + OpenAI + Anthropic; set TAVILY_API_KEY + FMP_API_KEY + OPENAI_API_KEY + ANTHROPIC_API_KEY"]
    fn live_research_packet_smoke() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        use crate::config::AppConfig;
        use crate::progress::{ProgressEvent, RecordingReporter, RunContext};

        // A recording context instead of `noop`, so the request rows the adapters emit
        // can be asserted on — the tracker-attribution half of what this smoke proves.
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new(
            "research-smoke",
            rec.clone(),
            Arc::new(AtomicBool::new(false)),
        );

        // The production constructor, with the keys resolved from the same env vars the
        // adapters' own from_env seams read — so a wiring regression in the bundle (a
        // dropped with_context, a swapped backend) fails this smoke too.
        let cfg = AppConfig::from_env();
        let stages = ResearchStages::live(
            cfg.tavily_key().expect("TAVILY_API_KEY set"),
            cfg.fmp_key().expect("FMP_API_KEY set"),
            cfg.openai_key().expect("OPENAI_API_KEY set"),
            cfg.anthropic_key().expect("ANTHROPIC_API_KEY set"),
            &ctx,
        )
        .expect("building the live research stages");

        // A stub embedder against a fresh temp store: the empty-store guard means the
        // retrieval pulls spend no embedding call here, keeping the smoke's live spend
        // unchanged — the retrieval path itself is covered offline. The live embedding
        // wire contract has its own smoke (`embedding::embedding_live_smoke`).
        let dir = tempfile::tempdir().expect("temp dir");
        let packet = assemble_research_packet(
            &stages,
            &BaselineMarketData::default(),
            None,
            &[],
            &crate::embedding::StubEmbedder,
            &dir.path().join("market_signal.db"),
            &ctx,
        );

        // Each stage produced real output (fail-soft would have let empty through).
        assert!(!packet.news_clusters.is_empty(), "gather+filter yielded clusters");
        assert!(!packet.research.items.is_empty(), "router yielded at least one topic");
        assert!(packet.research.requests_made >= 1, "executor spent at least one search");
        let total_sources: usize = packet
            .research
            .items
            .iter()
            .flat_map(|i| &i.findings)
            .map(|f| f.sources.len())
            .sum();
        assert!(total_sources >= 1, "at least one executor search returned sources");

        // Tracker attribution: every request row carries a research-half group, and each
        // live stage emitted at least one row. The frontend buckets the four research
        // groups under the research step (`App.vue`'s RESEARCH_REQUEST_GROUPS); "memory"
        // rows (the retrieval pulls, when a populated store makes them fire) follow the
        // currently-running step instead. None fire here — the temp store is empty —
        // but the set admits them so a populated-store run stays green.
        let msgs = rec.messages();
        let groups: Vec<&str> = msgs
            .iter()
            .filter_map(|m| match &m.event {
                ProgressEvent::RequestStarted { group, .. }
                | ProgressEvent::RequestFinished { group, .. } => Some(group.as_str()),
                _ => None,
            })
            .collect();
        assert!(!groups.is_empty(), "the adapters emitted request rows");
        for g in &groups {
            assert!(
                matches!(*g, "news" | "filter" | "routing" | "research" | "memory"),
                "request row carries a non-research group {g:?}"
            );
        }
        for expected in ["news", "filter", "routing", "research"] {
            assert!(
                groups.contains(&expected),
                "no request row for the {expected:?} stage"
            );
        }

        eprintln!(
            "research smoke: {} clusters; {} topics, {} findings, {} sources, \
             {} requests (stopped: {:?}); rows by group: news {}, filter {}, \
             routing {}, research {}",
            packet.news_clusters.len(),
            packet.research.items.len(),
            packet.research.items.iter().map(|i| i.findings.len()).sum::<usize>(),
            total_sources,
            packet.research.requests_made,
            packet.research.stopped_reason,
            groups.iter().filter(|g| **g == "news").count(),
            groups.iter().filter(|g| **g == "filter").count(),
            groups.iter().filter(|g| **g == "routing").count(),
            groups.iter().filter(|g| **g == "research").count(),
        );
    }
}
