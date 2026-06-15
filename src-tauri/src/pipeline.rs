//! Application-layer orchestration for a single manual report run.
//!
//! This is the spine the whole system is built on: the app layer drives the
//! agent stage (a pure function) and owns every side effect — the database
//! write and the canonical Markdown file. It is written free of any Tauri
//! runtime so it can be driven directly by an integration test against stubs.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::agent::{MainAgent, MainAgentInput, RecentReport, ReportSummary};
use crate::baseline_delta::{self, BaselineDeltas};
use crate::data_sources::{
    BaselineMarketData, GroupKind, MarketDataSource, BASELINE_SCHEMA_VERSION,
};
use crate::document_parser::{self, ParsedResearchDoc};
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
    /// The research inbox the Step-6 stage parses (`docs/research-documents.md`).
    pub inbox_dir: PathBuf,
    /// Where successfully processed inbox documents are filed once the run's
    /// report persists.
    pub archive_dir: PathBuf,
}

impl ReportPaths {
    /// The canonical app-data layout under one base directory — the single
    /// source for these names, shared by the production commands and scheduler
    /// (`lib.rs::report_paths`) and every test's temp dir, so the layouts can
    /// never drift apart.
    pub fn under(base: &std::path::Path) -> Self {
        Self {
            db_path: base.join("market_signal.db"),
            reports_dir: base.join("reports"),
            inbox_dir: base.join("research-inbox"),
            archive_dir: base.join("research-archive"),
        }
    }
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

    // Step 6: parse the research inbox (`docs/weekly-report-workflow.md §Step 6`,
    // `docs/research-documents.md`). Fully fail-soft — an unlistable folder or an
    // unparseable file never gates the run; failures are recorded for the panel's
    // error state and the file is left in the inbox for the next run. The parsed
    // documents ride into routing and the condensed packet below; the archive
    // move waits for the persist step, so a failed or cancelled run never
    // consumes the user's documents. Local CPU only — no request rows.
    ctx.step_started("inbox", "Reading research documents");
    let inbox_docs = process_research_inbox(&paths.inbox_dir, &paths.db_path, ctx);
    let inbox_status = if ctx.is_cancelled() { "cancelled" } else { "ok" };
    ctx.step_finished("inbox", inbox_status, None);
    bail_if_cancelled(ctx)?;

    // Steps 7–11: the research half — news → filter → route → execute → condensed packet,
    // fully fail-soft (`assemble_research_packet`), bracketed as one step with the adapters'
    // per-request rows streaming inside it. Computed here, before `baseline` and `deltas`
    // move into the agent input below.
    ctx.step_started("research", "Gathering and condensing research");
    let AssembledResearch { packet: research_packet, audit_memory } = assemble_research_packet(
        research,
        &baseline,
        deltas.as_ref(),
        &recent_reports,
        &inbox_docs,
        embedder,
        &paths.db_path,
        ctx,
    );
    let research_status = if ctx.is_cancelled() { "cancelled" } else { "ok" };
    ctx.step_finished("research", research_status, None);
    bail_if_cancelled(ctx)?;

    // Step 2: the bounded recent prior-report context — structured summaries plus
    // (truncated) Markdown bodies — the main agent reasons over for thesis continuity and
    // that the Retrospective Audit (`§Step 5`) evaluates. Best-effort like the router's
    // recent-report load above; never gates the run. Read here (not at the top with the
    // router's) so the freshest persisted reports are picked up after any upstream work.
    let recent_reports_for_audit = load_recent_reports_for_audit(&paths.db_path);

    ctx.step_started("agent", "Main agent writing the report");
    let output = match agent.generate(MainAgentInput {
        baseline,
        deltas,
        research: Some(research_packet),
        // Step 4 → Step 5: the pre-research pull steers the main agent's audit.
        audit_memory,
        // Step 2 → Step 5: the recent reports are the audit's auditable object and its gate.
        recent_reports: recent_reports_for_audit,
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

        // Best-effort: append truncation telemetry for any inbox document the
        // Step-6 parser had to head-truncate — the accumulating evidence base
        // that gates the reserved GPT-5-mini extraction stage. Same pure-local,
        // no-paid-call posture as the snapshot block above (so no cancellation
        // poll), and the same swallow-and-log discipline: a write failure must
        // never lose a report that already persisted. No row when nothing
        // overflowed, so the common case touches no DB.
        let truncations = collect_document_truncations(
            &inbox_docs,
            &summary.report_id,
            &as_of.to_rfc3339(),
        );
        if !truncations.is_empty() {
            if let Err(e) = storage::record_document_truncations(&conn, &truncations) {
                eprintln!(
                    "truncation-telemetry persist failed for report {}: {e:#}",
                    summary.report_id
                );
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
        // `docs/weekly-report-workflow.md §Step 17`). Trim / cap / near-duplicate
        // drop and the per-learning best-effort writes all live in
        // `persist_durable_learnings`.
        persist_durable_learnings(
            &conn,
            embedder,
            &output.durable_learnings,
            &summary.report_id,
            &summary.created_at,
            ctx,
        );

        // Best-effort: the 30-report retention cascade, run after this run's
        // insert so the new report counts toward the window. No paid calls, so
        // no cancellation poll; a cascade failure never fails the run.
        prune_old_reports(&conn);

        // Best-effort: file the successfully parsed inbox documents into the
        // archive (`docs/research-documents.md §Processing at Job Start`) — only
        // now, with the report persisted, does the run count as having consumed
        // them. A failed move logs and leaves the file; the next run's re-parse
        // is idempotent. A cancel landing during persist doesn't skip this leg:
        // the documents were used by the report that just persisted.
        document_parser::archive_processed(&paths.inbox_dir, &paths.archive_dir, &inbox_docs);

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

/// The 30-report retention cascade (`docs/storage.md §SQLite`): every report
/// beyond the newest [`storage::REPORT_RETENTION`] is deleted together with its
/// artifacts — the canonical Markdown file, its vector-memory summary row, its
/// baseline-snapshot rows, its truncation-telemetry rows, and finally the report
/// row itself. Durable learnings survive by `kind`, never touched whatever their
/// `report_id`
/// (`vector_memory::delete_report_summary`). There is no HTML leg — HTML is
/// rendered on demand for display/PDF and never persisted (settled 2026-06-12),
/// so the cascade has nothing to remove.
///
/// Per-evictee best-effort, never failing the run. The file leg goes first: a
/// file that is already gone counts as removed, but any other removal failure
/// skips that evictee's DB legs — the row keeps pointing at the still-existing
/// file and the evictee is simply re-selected on the next run's cascade —
/// rather than deleting the row and orphaning an untracked file. The four DB
/// legs then commit or roll back as one transaction: re-selection reads the
/// `reports` table, so deleting the row while an earlier leg failed would
/// strand that leg's rows with no retry path — and neither the vector summary
/// row nor the truncation-telemetry rows have any other reaper, leaving stale
/// memory retrievable forever and truncation rows unbounded. A rolled-back
/// evictee is re-selected next run, where its already-removed file reads as
/// NotFound and the DB legs run again.
fn prune_old_reports(conn: &rusqlite::Connection) {
    let evictees = match storage::select_reports_beyond_retention(conn, storage::REPORT_RETENTION)
    {
        Ok(evictees) => evictees,
        Err(e) => {
            eprintln!("report-retention: selecting reports beyond the cap failed: {e:#}");
            return;
        }
    };
    for evictee in evictees {
        match std::fs::remove_file(&evictee.markdown_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                eprintln!(
                    "report-retention: removing markdown {:?} for report {} failed \
                     (will retry next run): {e}",
                    evictee.markdown_path, evictee.report_id
                );
                continue;
            }
        }
        if let Err(e) = delete_report_db_rows(conn, &evictee.report_id) {
            eprintln!(
                "report-retention: deleting rows for report {} failed \
                 (rolled back, will retry next run): {e:#}",
                evictee.report_id
            );
        }
    }
}

/// One evictee's four SQLite legs — vector summary, baseline snapshots,
/// truncation telemetry, report row — committed together or not at all
/// (`unchecked_transaction`: the helpers take `&Connection`, and `Transaction`
/// derefs to it). The truncation leg is load-bearing, not belt-and-braces like
/// the snapshot leg: `document_truncations` has no self-cap, so this report-id
/// join is the only thing that bounds it.
fn delete_report_db_rows(conn: &rusqlite::Connection, report_id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    vector_memory::delete_report_summary(&tx, report_id)?;
    storage::delete_report_baseline_snapshots(&tx, report_id)?;
    storage::delete_report_truncations(&tx, report_id)?;
    storage::delete_report_row(&tx, report_id)?;
    tx.commit()?;
    Ok(())
}

/// What [`assemble_research_packet`] hands back: the Step-11 condensed packet plus the
/// Step-4 pre-research memory pull, surfaced separately so the main agent's retrospective
/// audit can consume it. The two memory pulls stay on distinct channels — `packet.memory`
/// is the Step-10 research-informed pull, `audit_memory` is the Step-4 pull — per the doc's
/// replace-not-merge rule (`docs/weekly-report-workflow.md §Step 10`).
struct AssembledResearch {
    packet: ResearchPacket,
    /// The Step-4 pre-research pull's prompt fragments (most relevant first); empty when
    /// nothing was recalled or the pull was skipped (early run, retrieval failure, cancel).
    audit_memory: Vec<String>,
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
/// view — is the ephemeral pull that steers investigation; it feeds the router here and
/// is also returned as [`AssembledResearch::audit_memory`] so the main agent's
/// retrospective audit (`§Step 5`) consumes the same pull on its own channel (the doc's
/// replace-not-merge rule keeps it *out* of the packet). The Step-10 post-research pull —
/// queried from the executor's evidence — is the one the condensed packet carries to the
/// main agent. `embedder` is the same fixed embedding stage the Step-17 persist write
/// uses; `db_path` locates the store.
#[allow(clippy::too_many_arguments)] // one parameter per Step-6/7/8/10 input, each documented above
fn assemble_research_packet(
    research: &ResearchStages,
    baseline: &BaselineMarketData,
    deltas: Option<&BaselineDeltas>,
    recent_reports: &[ReportSummary],
    inbox_docs: &[ParsedResearchDoc],
    embedder: &dyn Embedder,
    db_path: &std::path::Path,
    ctx: &RunContext,
) -> AssembledResearch {
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
    // investigation. Ephemeral: it feeds routing here and is returned as
    // `audit_memory` for the main agent's retrospective audit (`§Step 5`), but the
    // packet below carries the Step-10 pull instead (`§Step 10`, replace-not-merge).
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
            // Clone: routing consumes the Step-4 pull here, and it is also returned
            // below as `audit_memory` for the main agent's audit. A ~5-fragment Vec.
            memory: pre_memory.clone(),
            // Routing picks topics, so it gets each document's head, not the
            // full condensed text the packet carries below.
            inbox_documents: inbox_docs.iter().map(ParsedResearchDoc::router_excerpt).collect(),
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

    // Step 11: condense everything into the token-bounded packet. The inbox
    // blocks are already bounded by `document_parser`'s char budgets.
    let packet = build_condensed_packet(
        baseline.clone(),
        deltas.cloned(),
        clusters,
        evidence,
        memory,
        inbox_docs.iter().map(ParsedResearchDoc::prompt_block).collect(),
    );

    // The Step-4 pull rides back out alongside the packet so the main agent's audit
    // gets it on its own channel, distinct from `packet.memory` (the Step-10 pull).
    AssembledResearch { packet, audit_memory: pre_memory }
}

/// The Step-6 inbox stage: parse every supported document in the inbox
/// (`document_parser::process_inbox`, fail-soft with per-file cancellation
/// polling), log each failure, and record this pass's failure set for the
/// Research Documents panel's error states. The failure write is best-effort
/// (stderr on error, never a gate) and is skipped entirely on a cancelled pass —
/// a partial pass must not clobber the recorded state of files it never reached.
fn process_research_inbox(
    inbox_dir: &std::path::Path,
    db_path: &std::path::Path,
    ctx: &RunContext,
) -> Vec<ParsedResearchDoc> {
    let outcome = document_parser::process_inbox(inbox_dir, ctx);
    for failure in &outcome.failures {
        eprintln!(
            "research-inbox: {} could not be parsed (left in the inbox): {}",
            failure.name, failure.reason
        );
    }
    if !ctx.is_cancelled() {
        if let Err(e) = record_parse_failures(db_path, &outcome.failures) {
            eprintln!("research-inbox: recording parse failures failed: {e:#}");
        }
    }
    outcome.docs
}

/// Replace the recorded parse-failure set with this pass's
/// (`storage::replace_parse_failures` — the table holds "the failures of the
/// most recent inbox pass", so a healed or deleted file self-clears).
fn record_parse_failures(
    db_path: &std::path::Path,
    failures: &[crate::document_parser::ParseFailure],
) -> Result<()> {
    let conn = storage::open(db_path)?;
    storage::init_schema(&conn)?;
    let failed_at = chrono::Utc::now().to_rfc3339();
    let rows: Vec<storage::ParseFailureRow> = failures
        .iter()
        .map(|f| storage::ParseFailureRow {
            name: f.name.clone(),
            size_bytes: f.size_bytes,
            modified: f.modified.clone(),
            reason: f.reason.clone(),
            failed_at: failed_at.clone(),
        })
        .collect();
    storage::replace_parse_failures(&conn, &rows)
}

/// Build the truncation-telemetry rows for this run: one per inbox document the
/// parser had to head-truncate (`ParsedResearchDoc::truncated`), stamped with
/// the report it persisted under and the run's `captured_at` scan time. The pure
/// derivation is split out as the unit-test seam (the persist-step call that
/// writes these is best-effort and untestable in isolation). Whole documents
/// produce no row, so an inbox that never overflows leaves the table untouched.
fn collect_document_truncations(
    inbox_docs: &[ParsedResearchDoc],
    report_id: &str,
    captured_at: &str,
) -> Vec<storage::DocumentTruncationRow> {
    inbox_docs
        .iter()
        .filter(|doc| doc.truncated())
        .map(|doc| storage::DocumentTruncationRow {
            report_id: report_id.to_string(),
            captured_at: captured_at.to_string(),
            name: doc.name.clone(),
            format: doc.format.clone(),
            original_chars: doc.original_chars as u64,
            kept_chars: doc.text.chars().count() as u64,
        })
        .collect()
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
/// deltas — for the first report (no prior snapshot), any DB error, or a prior blob that
/// won't decode. The deltas are additive context, never a gate, so every failure degrades
/// to `None`. Routed through the shared [`read_db_fail_soft`] shell (`Option::default()`
/// is `None`), so a DB-level failure now leaves the same labeled stderr trace as the
/// other fail-soft reads — only an absent prior snapshot is the silent, expected `None`.
fn compute_prior_deltas(
    db_path: &std::path::Path,
    current: &BaselineMarketData,
    as_of: chrono::DateTime<chrono::Utc>,
) -> Option<BaselineDeltas> {
    read_db_fail_soft(db_path, "prior-delta read", |conn| {
        let Some((captured_at, baseline_json)) = storage::latest_baseline_snapshot(conn)? else {
            return Ok(None);
        };
        let prior: BaselineMarketData =
            serde_json::from_str(&baseline_json).context("decoding prior baseline snapshot")?;
        let prior_at = chrono::DateTime::parse_from_rfc3339(&captured_at)
            .context("parsing prior snapshot captured_at")?
            .with_timezone(&chrono::Utc);
        let elapsed_days = (as_of - prior_at).num_seconds() as f64 / 86_400.0;
        Ok(Some(baseline_delta::compute_deltas(current, &prior, elapsed_days)))
    })
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
    read_db_fail_soft(db_path, "research recent-report context", |conn| {
        storage::list_recent_reports(conn, ROUTER_RECENT_REPORTS)
    })
}

/// Run a best-effort `open + init_schema + read` against the report DB, degrading to
/// `T::default()` with a `context`-prefixed stderr log on any failure. Owns the shared
/// shape behind every fail-soft DB read in the pipeline — the recent-report context
/// loaders, the vector-memory pulls, and the prior-delta change view — so each caller
/// carries only its own query and post-read work, never the open/init/degrade shell.
/// Never gates the run: `T::default()` is the empty `Vec`/`None`/zero the caller falls
/// back to, and the labeled stderr line is the only trace a degraded read leaves.
fn read_db_fail_soft<T: Default>(
    db_path: &std::path::Path,
    context: &str,
    read: impl FnOnce(&rusqlite::Connection) -> Result<T>,
) -> T {
    let run = || -> Result<T> {
        let conn = storage::open(db_path)?;
        storage::init_schema(&conn)?;
        read(&conn)
    };
    run().unwrap_or_else(|e| {
        eprintln!("{context}: degraded to empty: {e:#}");
        T::default()
    })
}

/// Bounded count of recent prior reports handed to the main agent as Step-2
/// context (`docs/weekly-report-workflow.md §Step 2`). Matches `ROUTER_RECENT_REPORTS`
/// today and sits inside the audit's "usually 2–6 reports" window (`§Step 5`); a
/// separate constant so the audit window can widen without moving routing's. Tunable
/// alongside the other prompt-budget caps.
const MAIN_AGENT_RECENT_REPORTS: u32 = 3;

/// Per-report ceiling on the Markdown body carried in the Step-2 context, in chars
/// (~3k tokens — the inbox `PER_DOC_CHAR_CAP` magnitude). A typical weekly report rides
/// whole; only a pathological one is head-truncated, visibly (a marker matching the
/// inbox-doc convention the system prompt already explains). Tunable alongside the
/// packet caps.
const RECENT_REPORT_BODY_CAP: usize = 12_000;

/// Best-effort Step-2 recent prior-report context for the main agent: the most recent
/// reports' structured summaries paired with their (head-truncated) Markdown bodies,
/// newest first (`storage::list_recent_reports_with_paths` plus one file read per
/// report). The body is the auditable object the Retrospective Audit reasons over
/// (`§Step 5`); the summary carries the thesis-continuity metadata. Additive and
/// fail-soft like [`load_recent_report_context`]: a DB failure degrades the whole list
/// to empty, an unreadable Markdown file drops that one report's body to empty (the
/// summary still carries) — never gates the run.
fn load_recent_reports_for_audit(db_path: &std::path::Path) -> Vec<RecentReport> {
    let rows = read_db_fail_soft(db_path, "main-agent recent-report context", |conn| {
        storage::list_recent_reports_with_paths(conn, MAIN_AGENT_RECENT_REPORTS)
    });
    rows.into_iter()
        .map(|(summary, path)| {
            let markdown = std::fs::read_to_string(&path)
                .map(|body| truncate_report_body(&body, RECENT_REPORT_BODY_CAP))
                .unwrap_or_else(|e| {
                    eprintln!(
                        "main-agent: recent-report body unreadable ({path}), summary only: {e:#}"
                    );
                    String::new()
                });
            RecentReport { summary, markdown }
        })
        .collect()
}

/// Head-truncate a report body to `cap` chars, appending a visible marker (matching the
/// inbox-doc convention) when it is cut. Char-boundary safe — counts and slices by chars,
/// never bytes.
fn truncate_report_body(body: &str, cap: usize) -> String {
    let total = body.chars().count();
    if total <= cap {
        return body.to_string();
    }
    let head: String = body.chars().take(cap).collect();
    format!("{head}\n[truncated — showing the first {cap} of {total} characters]")
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

/// Cosine-similarity threshold at or above which a freshly embedded durable
/// learning is treated as a near-restatement of one the store already holds and
/// dropped before it spends a row. An app-layer policy like the cap above, not a
/// model contract: the prompt can't be trusted to avoid paraphrasing a prior
/// run's lesson, and learnings are never deleted, so unbounded restatement is
/// permanent growth that dilutes retrieval. Calibrated to real
/// `text-embedding-3-large` geometry by `tuning_dedup_threshold_calibration`:
/// genuine restatements embed at ~0.72–0.81 cosine while distinct lessons cap at
/// ~0.53, so 0.65 sits in that measured gap — catching real paraphrases without
/// ever merging two distinct lessons. (The original 0.93 was a conservative guess
/// that proved vacuously high: nothing real reached it, so dedup never fired.) Not
/// doc-pinned, tunable alongside `MEMORY_TOP_K` and `LEARNINGS_PER_REPORT_CAP`. A
/// dedup-scan failure fails *open* (the learning is kept), so the check can only
/// ever drop a redundant row, never lose a real one.
const LEARNING_DEDUP_THRESHOLD: f64 = 0.65;

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
/// a stderr note (`label` names which pull), via the shared
/// [`read_db_fail_soft`] shell. Two cheap guards keep early runs free: an empty
/// query has nothing to recall against (checked here, before the DB is even
/// opened), and an empty store can't return hits — both skip the paid embedding
/// call entirely. The query is capped at [`MEMORY_QUERY_MAX_BYTES`] before
/// embedding, so an oversized one is truncated rather than rejected by the
/// provider and lost.
fn retrieve_memory(
    db_path: &std::path::Path,
    embedder: &dyn Embedder,
    query: &str,
    label: &str,
) -> Vec<String> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    read_db_fail_soft(db_path, &format!("research {label} memory retrieval"), |conn| {
        if vector_memory::count_memory(conn)? == 0 {
            return Ok(Vec::new());
        }
        let embedding = embedder.embed(bounded_query(query, MEMORY_QUERY_MAX_BYTES))?;
        let hits = vector_memory::search_memory(conn, &embedding, None, MEMORY_TOP_K)?;
        Ok(hits.iter().map(vector_memory::MemoryHit::prompt_fragment).collect())
    })
}

/// Embed and store a run's durable learnings — Step 17's second memory leg
/// (`docs/weekly-report-workflow.md §Step 17`). The app-layer bounds live here,
/// not the model contract: entries are trimmed, empties dropped, the rest capped
/// at [`LEARNINGS_PER_REPORT_CAP`], and each survivor checked against the store
/// for a near-restatement before a row is spent. Each learning is its own atomic
/// unit (one embedding per learning, `docs/storage.md §Embeddings`) and every
/// write is independently best-effort — one failed embed, dedup scan, or insert
/// costs that learning, never its siblings or the report. Dedup reuses the
/// embedding just computed (no extra paid call) and scans the store *including*
/// this run's own prior inserts, so two near-identical learnings in one run
/// collapse to one; a scan error fails open and keeps the learning. Cancellation
/// is polled before each embedding call, like the executor, so a cancel landing
/// mid-persist stops spending rather than riding out the remaining learnings.
/// `report_id` is provenance only: the `kind` column, not the id, is what makes
/// learnings survive the retention cascade.
fn persist_durable_learnings(
    conn: &rusqlite::Connection,
    embedder: &dyn Embedder,
    learnings: &[String],
    report_id: &str,
    created_at: &str,
    ctx: &RunContext,
) {
    let trimmed: Vec<&str> = learnings
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if trimmed.len() > LEARNINGS_PER_REPORT_CAP {
        eprintln!(
            "vector-memory: report {report_id} emitted {} durable learnings; keeping the first {}",
            trimmed.len(),
            LEARNINGS_PER_REPORT_CAP
        );
    }
    for learning in trimmed.into_iter().take(LEARNINGS_PER_REPORT_CAP) {
        // Polled at each request boundary, like the executor: a cancel that lands
        // during one embedding call must not spend the next.
        if ctx.is_cancelled() {
            break;
        }
        let embedding = match embedder.embed(learning) {
            Ok(embedding) => embedding,
            Err(e) => {
                eprintln!("vector-memory learning embedding failed for report {report_id}: {e:#}");
                continue;
            }
        };
        // Drop a near-restatement of a learning the store already holds. Fails
        // open: a scan error treats the learning as novel and keeps it, so dedup
        // can only ever drop a redundant row, never lose a real one.
        match vector_memory::nearest_learning_similarity(conn, &embedding) {
            Ok(Some(sim)) if sim >= LEARNING_DEDUP_THRESHOLD => {
                eprintln!(
                    "vector-memory: report {report_id} dropping near-duplicate learning (sim {sim:.3})"
                );
                continue;
            }
            Ok(_) => {}
            Err(e) => eprintln!(
                "vector-memory learning dedup scan failed for report {report_id} (keeping the learning): {e:#}"
            ),
        }
        if let Err(e) = vector_memory::insert_memory(
            conn,
            MemoryKind::Learning,
            Some(report_id),
            learning,
            &embedding,
            created_at,
        ) {
            eprintln!("vector-memory learning persist failed for report {report_id}: {e:#}");
        }
    }
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
            &[],
            &StubEmbedder,
            &dir.path().join("market_signal.db"),
            &RunContext::noop(),
        )
        .packet
    }

    #[test]
    fn recent_report_context_degrades_to_empty_on_an_unopenable_db() {
        // A db path whose parent directory doesn't exist can't be opened or created;
        // the best-effort read must degrade to empty rather than erroring.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-subdir").join("market_signal.db");
        assert!(load_recent_report_context(&path).is_empty());
    }

    #[test]
    fn compute_prior_deltas_degrades_to_none_on_an_unopenable_db() {
        // Same fail-soft arm as the recent-report read, now that the change view shares
        // the `read_db_fail_soft` shell: an unopenable db degrades to `None` (no deltas)
        // rather than erroring — the deltas are additive context, never a gate.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-subdir").join("market_signal.db");
        let as_of = chrono::DateTime::parse_from_rfc3339("2026-06-15T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(compute_prior_deltas(&path, &BaselineMarketData::default(), as_of).is_none());
    }

    // ---- Step-2 recent prior-report context (`load_recent_reports_for_audit`) ----

    fn audit_summary(id: &str, created_at: &str) -> ReportSummary {
        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        ReportSummary {
            report_id: id.into(),
            report_type: "weekly_market".into(),
            created_at: created_at.into(),
            risk_posture: RiskPosture::Mixed,
            market_cycle: MarketCycle::LateCycle,
            thesis_stance: ThesisStance::Uncertain,
            header_summary_bullets: vec!["a".into(), "b".into(), "c".into()],
            key_risks: vec![],
            unresolved_questions: vec![],
            forward_outlook_themes: vec![],
        }
    }

    #[test]
    fn truncate_report_body_marks_only_when_cut() {
        let short = "a short report body";
        assert_eq!(truncate_report_body(short, 100), short, "under the cap rides whole");
        let long = "x".repeat(50);
        let cut = truncate_report_body(&long, 10);
        assert!(cut.starts_with(&"x".repeat(10)), "head kept: {cut}");
        assert!(
            cut.contains("[truncated — showing the first 10 of 50 characters]"),
            "marker carries both counts: {cut}"
        );
    }

    #[test]
    fn load_recent_reports_for_audit_carries_bodies_truncates_and_is_fail_soft() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("market_signal.db");
        let conn = storage::open(&db).unwrap();
        storage::init_schema(&conn).unwrap();
        let reports_dir = dir.path().join("reports");
        std::fs::create_dir_all(&reports_dir).unwrap();

        let insert = |id: &str, created_at: &str, body: Option<&str>| {
            let summary = audit_summary(id, created_at);
            let path = reports_dir.join(format!("{id}.md"));
            // `body == None` models a report whose Markdown file is missing on disk.
            if let Some(b) = body {
                std::fs::write(&path, b).unwrap();
            }
            let summary_json = serde_json::to_string(&summary).unwrap();
            storage::insert_report(
                &conn,
                &ReportRecord {
                    summary: &summary,
                    markdown_path: &path.to_string_lossy(),
                    summary_json: &summary_json,
                },
            )
            .unwrap();
        };
        insert("old", "2026-01-01T00:00:00Z", Some("the older report body"));
        let big = "y".repeat(RECENT_REPORT_BODY_CAP + 500);
        insert("new", "2026-02-01T00:00:00Z", Some(&big));
        insert("ghost", "2026-03-01T00:00:00Z", None);

        let recent = load_recent_reports_for_audit(&db);
        assert_eq!(recent.len(), 3, "all three within the cap, newest first");
        assert_eq!(recent[0].summary.report_id, "ghost");
        assert!(
            recent[0].markdown.is_empty(),
            "a missing Markdown file drops the body, summary still carries"
        );
        assert_eq!(recent[1].summary.report_id, "new");
        assert!(recent[1].markdown.contains("[truncated"), "an over-cap body is marked");
        assert_eq!(recent[2].summary.report_id, "old");
        assert_eq!(recent[2].markdown, "the older report body", "an in-cap body rides whole");
    }

    #[test]
    fn recent_reports_for_audit_degrades_to_empty_on_an_unopenable_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-subdir").join("market_signal.db");
        assert!(load_recent_reports_for_audit(&path).is_empty());
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

    /// Embeds text to a basis vector chosen by a leading `k<N>` tag, so a test can
    /// set exact cosine relationships without depending on the stub's hash
    /// geometry: same tag → identical embedding (cosine 1.0, a duplicate);
    /// different tags → orthogonal (cosine 0, genuinely distinct).
    struct BasisEmbedder;

    impl Embedder for BasisEmbedder {
        fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
            let idx: usize = text
                .trim()
                .strip_prefix('k')
                .and_then(|r| r.split_whitespace().next())
                .and_then(|n| n.parse().ok())
                .expect("BasisEmbedder text must start with `k<N> `");
            let mut v = vec![0.0f32; 8];
            v[idx % 8] = 1.0;
            Ok(v)
        }
    }

    fn learning_count(conn: &rusqlite::Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM vector_memory WHERE kind = 'learning'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn persist_durable_learnings_drops_near_duplicates_within_and_across_runs() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        let ctx = RunContext::noop();

        // One run restates the same lesson twice under different wording (same tag →
        // same embedding). The second collapses into the first: one row, not two —
        // dedup scans this run's own prior insert.
        persist_durable_learnings(
            &conn,
            &BasisEmbedder,
            &["k0 breadth thinned".into(), "k0 breadth kept thinning".into()],
            "rep-1",
            "2026-06-01T00:00:00Z",
            &ctx,
        );
        assert_eq!(learning_count(&conn), 1, "within-run duplicate dropped");

        // A later run restates the same lesson again: still deduped against the
        // stored row, not just same-run siblings.
        persist_durable_learnings(
            &conn,
            &BasisEmbedder,
            &["k0 breadth remained narrow".into()],
            "rep-2",
            "2026-06-08T00:00:00Z",
            &ctx,
        );
        assert_eq!(learning_count(&conn), 1, "cross-run duplicate dropped");

        // A genuinely different lesson (orthogonal embedding) is kept — dedup must
        // not collapse distinct learnings.
        persist_durable_learnings(
            &conn,
            &BasisEmbedder,
            &["k1 credit spreads widened".into()],
            "rep-2",
            "2026-06-08T00:00:00Z",
            &ctx,
        );
        assert_eq!(learning_count(&conn), 2, "a distinct learning persists");
    }

    #[test]
    fn persist_durable_learnings_trims_blanks_and_caps_the_rest() {
        // Regression guard for the trim + cap the extraction moved: blank entries are
        // dropped before counting, and at most LEARNINGS_PER_REPORT_CAP rows land.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        let ctx = RunContext::noop();

        let mut learnings: Vec<String> = vec!["   ".into(), "".into()];
        // Distinct tags (idx < 8) so none dedup against each other; more than the cap.
        for i in 0..(LEARNINGS_PER_REPORT_CAP + 3) {
            learnings.push(format!("k{i} distinct lesson {i}"));
        }
        persist_durable_learnings(&conn, &BasisEmbedder, &learnings, "rep-1", "t", &ctx);
        assert_eq!(
            learning_count(&conn),
            LEARNINGS_PER_REPORT_CAP as i64,
            "blanks dropped, the rest capped"
        );
    }

    #[test]
    fn assemble_packet_happy_path_with_stubs_carries_news_and_evidence() {
        let packet = assemble_with(ResearchStages::stub());
        assert!(!packet.news_clusters.is_empty(), "stub chain yields clusters");
        assert!(!packet.research.items.is_empty(), "stub chain yields evidence");
    }

    /// One recent summary so `pre_research_query` renders a non-blank query (a default
    /// baseline + no recent reports would short-circuit the pull). The text is irrelevant
    /// to the assertion — only that the Step-4 query is non-empty so the pull can fire.
    fn one_recent_summary() -> ReportSummary {
        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        ReportSummary {
            report_id: "prior".into(),
            report_type: "weekly_market".into(),
            created_at: "2026-05-20T13:00:00Z".into(),
            risk_posture: RiskPosture::Mixed,
            market_cycle: MarketCycle::LateCycle,
            thesis_stance: ThesisStance::Uncertain,
            header_summary_bullets: vec!["Breadth stayed thin.".into()],
            key_risks: vec![],
            unresolved_questions: vec![],
            forward_outlook_themes: vec![],
        }
    }

    #[test]
    fn assemble_surfaces_the_step4_pull_as_audit_memory() {
        // A seeded store plus a real (non-blank) Step-4 query: the pre-research pull
        // fires and rides back out as `audit_memory` for the main agent's audit —
        // separate from `packet.memory` (the Step-10 pull). This is the audit consumer
        // the routing-only wiring previously dropped.
        let (_dir, db_path) = seeded_store();
        let recent = [one_recent_summary()];
        let assembled = assemble_research_packet(
            &ResearchStages::stub(),
            &BaselineMarketData::default(),
            None,
            &recent,
            &[],
            &StubEmbedder,
            &db_path,
            &RunContext::noop(),
        );
        assert_eq!(assembled.audit_memory.len(), 1, "the Step-4 pull reached audit_memory");
        assert!(
            assembled.audit_memory[0].starts_with("[learning · "),
            "audit memory carries the store's prompt fragment, got {:?}",
            assembled.audit_memory[0]
        );
    }

    #[test]
    fn assemble_audit_memory_is_empty_on_an_empty_store() {
        // No seeded rows: the empty-store guard skips the pull, so the audit gets nothing
        // even with a non-blank query. (The cancel arm is covered by the cancellation test.)
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("market_signal.db");
        let recent = [one_recent_summary()];
        let assembled = assemble_research_packet(
            &ResearchStages::stub(),
            &BaselineMarketData::default(),
            None,
            &recent,
            &[],
            &StubEmbedder,
            &db_path,
            &RunContext::noop(),
        );
        assert!(assembled.audit_memory.is_empty(), "an empty store recalls no audit memory");
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

    /// Seed one report row whose markdown path never existed (the file leg reads
    /// it as already removed), so the retention tests exercise the DB legs alone.
    fn seed_retention_row(conn: &rusqlite::Connection, id: &str, created_at: &str) {
        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        let summary = ReportSummary {
            report_id: id.to_string(),
            report_type: "weekly_market".to_string(),
            created_at: created_at.to_string(),
            risk_posture: RiskPosture::Mixed,
            market_cycle: MarketCycle::LateCycle,
            thesis_stance: ThesisStance::Uncertain,
            header_summary_bullets: vec!["a".into(), "b".into(), "c".into()],
            key_risks: vec![],
            unresolved_questions: vec![],
            forward_outlook_themes: vec![],
        };
        let summary_json = serde_json::to_string(&summary).unwrap();
        storage::insert_report(
            conn,
            &ReportRecord {
                summary: &summary,
                markdown_path: &format!("/nonexistent/{id}.md"),
                summary_json: &summary_json,
            },
        )
        .unwrap();
    }

    #[test]
    fn prune_old_reports_rolls_back_all_db_legs_when_one_fails() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();

        // One report over the cap; the evictee owns a vector summary row and a
        // baseline-snapshot row.
        for i in 0..=storage::REPORT_RETENTION {
            let created_at = format!("2026-01-{:02}T00:00:00Z", i + 1);
            seed_retention_row(&conn, &format!("old-{i:02}"), &created_at);
        }
        vector_memory::insert_memory(
            &conn,
            MemoryKind::Summary,
            Some("old-00"),
            "summary for old-00",
            &[0.1, 0.2],
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        storage::insert_baseline_snapshot(&conn, "old-00", "2026-01-01T00:00:00Z", 1, "{}")
            .unwrap();
        storage::record_document_truncations(
            &conn,
            &[storage::DocumentTruncationRow {
                report_id: "old-00".into(),
                captured_at: "2026-01-01T00:00:00Z".into(),
                name: "big.pdf".into(),
                format: "pdf".into(),
                original_chars: 30_000,
                kept_chars: 12_000,
            }],
        )
        .unwrap();

        // Sabotage the *last* DB leg: the summary and snapshot deletes succeed
        // inside the transaction, then the row delete aborts — proving the
        // earlier legs roll back with it rather than committing piecemeal.
        conn.execute_batch(
            "CREATE TRIGGER block_report_delete BEFORE DELETE ON reports
             BEGIN SELECT RAISE(ABORT, 'sabotaged'); END;",
        )
        .unwrap();

        prune_old_reports(&conn);

        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(
            count("SELECT COUNT(*) FROM reports"),
            storage::REPORT_RETENTION as i64 + 1,
            "the failed row delete keeps the evictee selectable"
        );
        assert_eq!(
            count(
                "SELECT COUNT(*) FROM vector_memory
                 WHERE kind = 'summary' AND report_id = 'old-00'"
            ),
            1,
            "the summary delete rolled back with the failed row delete"
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM baseline_snapshots WHERE report_id = 'old-00'"),
            1,
            "the snapshot delete rolled back with the failed row delete"
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM document_truncations WHERE report_id = 'old-00'"),
            1,
            "the truncation delete rolled back with the failed row delete"
        );

        // Next run, sabotage gone: the same evictee is re-selected and fully
        // evicted — the retry path the rollback preserves.
        conn.execute("DROP TRIGGER block_report_delete", []).unwrap();
        prune_old_reports(&conn);
        assert_eq!(
            count("SELECT COUNT(*) FROM reports"),
            storage::REPORT_RETENTION as i64
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM vector_memory WHERE report_id = 'old-00'"),
            0
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM baseline_snapshots WHERE report_id = 'old-00'"),
            0
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM document_truncations WHERE report_id = 'old-00'"),
            0
        );
    }

    #[test]
    fn collect_document_truncations_keeps_only_head_cut_docs() {
        let doc = |name: &str, text: &str, original: usize| ParsedResearchDoc {
            name: name.into(),
            format: "pdf".into(),
            size_bytes: 0,
            modified: None,
            text: text.into(),
            original_chars: original,
        };
        // One head-cut doc (text shorter than the original) and one whole doc
        // (text length == original) — only the first is telemetry.
        let docs = vec![
            doc("big.pdf", "kept head", 12_000),
            doc("note.md", "all of it", "all of it".chars().count()),
        ];

        let rows = collect_document_truncations(&docs, "rep-42", "2026-06-15T09:00:00+00:00");

        assert_eq!(rows.len(), 1, "the whole doc produces no row");
        let row = &rows[0];
        assert_eq!(row.report_id, "rep-42");
        assert_eq!(row.captured_at, "2026-06-15T09:00:00+00:00");
        assert_eq!(row.name, "big.pdf");
        assert_eq!(row.format, "pdf");
        assert_eq!(row.original_chars, 12_000);
        assert_eq!(row.kept_chars, "kept head".chars().count() as u64);
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
    fn cancelled_inbox_pass_skips_the_failure_write() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        use crate::progress::{NoopReporter, RunContext};

        // A cancelled pass parses nothing, and — critically — must not replace
        // the recorded failure set with its partial (empty) view. The skipped
        // write is observable as the DB never being touched: `record_parse_failures`
        // is the only DB access in this stage, and opening would create the file.
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("research-inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("broken.json"), "{ not json").unwrap();
        let db_path = dir.path().join("market_signal.db");

        let cancelled =
            RunContext::new("t", Arc::new(NoopReporter), Arc::new(AtomicBool::new(true)));
        let docs = process_research_inbox(&inbox, &db_path, &cancelled);
        assert!(docs.is_empty(), "a cancelled pass parses nothing");
        assert!(!db_path.exists(), "the failure write was skipped, not emptied");

        // The same pass uncancelled records the failure — pinning that the
        // skip above came from the cancel, not from a quiet no-op.
        let docs = process_research_inbox(&inbox, &db_path, &RunContext::noop());
        assert!(docs.is_empty());
        let conn = storage::open(&db_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM research_parse_failures", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
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
        let AssembledResearch { packet, audit_memory } = assemble_research_packet(
            &stages,
            &BaselineMarketData::default(),
            None,
            &recent,
            &[],
            &PanicEmbedder,
            &db_path,
            &ctx,
        );
        assert!(packet.news_clusters.is_empty());
        assert!(packet.research.items.is_empty());
        assert!(packet.memory.is_empty());
        // The Step-4 audit pull obeys the same cancel guard: a pre-cancelled run skips
        // it (the `PanicEmbedder` proves the embedding call never fires) and the audit
        // gets nothing.
        assert!(audit_memory.is_empty(), "a cancelled run recalls no audit memory");
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
            &[],
            &crate::embedding::StubEmbedder,
            &dir.path().join("market_signal.db"),
            &ctx,
        )
        .packet;

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

    // ---- live tuning-constant calibration against real text-embedding-3-large ----
    //
    // The synthetic separating embedders (`BasisEmbedder` here, `DistinctEmbedder` in
    // tests/generate_report.rs) only ever emit cosine 1.0 or 0.0, so they validate the
    // dedup/retrieval *mechanism* but say nothing about whether the *values*
    // `LEARNING_DEDUP_THRESHOLD` (0.65) and `MEMORY_TOP_K` (5) are well-placed against
    // the intermediate cosines real prose lands at. These two `#[ignore]`d probes
    // close that gap by embedding a hand-authored corpus through the live model and
    // checking where the constants fall. Spend is small (~27 cheap embedding calls,
    // no daily cap), so unlike the FMP smokes they are freely re-runnable.

    /// Restatements of one lesson under two wordings — each pair should embed *close*
    /// (a near-duplicate the Step-17 dedup pass is meant to catch).
    const PARAPHRASE_PAIRS: [(&str, &str); 4] = [
        (
            "Breadth divergences preceded the spring pullback; weight them earlier in the thesis.",
            "Deteriorating market breadth led the spring drawdown — give breadth signals more weight, sooner.",
        ),
        (
            "Single-event volatility spikes faded within two reports; avoid pivoting the thesis on them.",
            "One-off volatility shocks reverted within a couple of weeks — don't swing the thesis on a single spike.",
        ),
        (
            "Credit spreads widened ahead of the equity correction; treat widening as an early risk-off tell.",
            "High-yield spread widening front-ran the stock-market selloff — read spread widening as an early risk-off signal.",
        ),
        (
            "The yield curve's re-steepening coincided with the cycle turn; track the 10y-2y as a regime marker.",
            "Curve re-steepening lined up with the turn in the cycle — watch the 10s-2s spread as a regime indicator.",
        ),
    ];

    /// Genuinely distinct lessons, one per topic family — every cross-pair should embed
    /// *apart* (the dedup pass must never merge two of these). The first four reuse the
    /// canonical (`.0`) member of each `PARAPHRASE_PAIRS` entry; the last two add topics.
    const DISTINCT_LESSONS: [&str; 6] = [
        "Breadth divergences preceded the spring pullback; weight them earlier in the thesis.",
        "Single-event volatility spikes faded within two reports; avoid pivoting the thesis on them.",
        "Credit spreads widened ahead of the equity correction; treat widening as an early risk-off tell.",
        "The yield curve's re-steepening coincided with the cycle turn; track the 10y-2y as a regime marker.",
        "Late-cycle leadership narrowed into mega-cap tech; rotation breadth matters more than the index level.",
        "Inflation surprises moved the front end more than the long end; lean on breakevens for the regime read.",
    ];

    /// Calibrates `LEARNING_DEDUP_THRESHOLD` against real geometry — the empirical basis
    /// for the retune from a vacuously-high 0.93 (which no real restatement reached, so
    /// dedup never fired) to 0.65. Asserts the threshold brackets the measured gap:
    /// `distinct_ceiling < LEARNING_DEDUP_THRESHOLD <= paraphrase_floor` — it catches
    /// *every* authored restatement (full recall over this corpus) while never reaching
    /// a distinct-lesson pair (no false merge). The ordering invariant (restatements
    /// out-cosine distinct lessons) guards the embedder/corpus premise independent of the
    /// threshold value. A red means the threshold drifted out of the gap, or the corpus
    /// no longer separates — retuning the constant is a separate decision, not this
    /// probe's job.
    #[test]
    #[ignore = "hits the live OpenAI embeddings API; set OPENAI_API_KEY (~14 calls)"]
    fn tuning_dedup_threshold_calibration() {
        let embedder = crate::embedding::OpenAiEmbedder::from_env().expect("OPENAI_API_KEY set");
        let cos = |a: &str, b: &str| {
            let va = embedder.embed(a).expect("live embedding call");
            let vb = embedder.embed(b).expect("live embedding call");
            crate::vector_memory::cosine_similarity(&va, &vb)
        };

        // Within-pair cosine for each restatement.
        let pair_cosines: Vec<f64> = PARAPHRASE_PAIRS
            .iter()
            .enumerate()
            .map(|(i, (a, b))| {
                let c = cos(a, b);
                eprintln!("paraphrase pair {i}: cos {c:.4}");
                c
            })
            .collect();
        let paraphrase_floor = pair_cosines.iter().copied().fold(f64::INFINITY, f64::min);
        let paraphrase_ceiling = pair_cosines.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        // Max cosine across every pair of genuinely distinct lessons.
        let distinct_vecs: Vec<Vec<f32>> = DISTINCT_LESSONS
            .iter()
            .map(|t| embedder.embed(t).expect("live embedding call"))
            .collect();
        let mut distinct_ceiling = f64::NEG_INFINITY;
        for (i, va) in distinct_vecs.iter().enumerate() {
            for vb in &distinct_vecs[i + 1..] {
                distinct_ceiling = distinct_ceiling.max(crate::vector_memory::cosine_similarity(va, vb));
            }
        }

        let cleared = pair_cosines
            .iter()
            .filter(|c| **c >= LEARNING_DEDUP_THRESHOLD)
            .count();
        eprintln!(
            "dedup calibration: threshold={LEARNING_DEDUP_THRESHOLD} | distinct_ceiling={distinct_ceiling:.4} \
             paraphrase_floor={paraphrase_floor:.4} paraphrase_ceiling={paraphrase_ceiling:.4} | \
             {cleared}/{} restatements clear the threshold (recall)",
            PARAPHRASE_PAIRS.len()
        );

        // Ordering invariant (embedder/corpus premise, threshold-independent): a
        // restatement out-cosines any pair of distinct lessons.
        assert!(
            paraphrase_floor > distinct_ceiling,
            "restatements ({paraphrase_floor:.4}) did not separate from distinct lessons ({distinct_ceiling:.4})"
        );
        // Safety: the threshold must sit above every distinct-lesson pair, or dedup would wrongly merge two real lessons.
        assert!(
            distinct_ceiling < LEARNING_DEDUP_THRESHOLD,
            "a distinct-lesson pair ({distinct_ceiling:.4}) reaches the dedup threshold {LEARNING_DEDUP_THRESHOLD} — it would be wrongly merged"
        );
        // Recall: the threshold must sit at or below *every* restatement (full recall over
        // this corpus), or a real paraphrase would wrongly survive.
        assert!(
            paraphrase_floor >= LEARNING_DEDUP_THRESHOLD,
            "a restatement ({paraphrase_floor:.4}) sits below the dedup threshold {LEARNING_DEDUP_THRESHOLD} — it would wrongly survive"
        );
    }

    /// A recall query plus a mixed corpus: four fragments genuinely on-topic, eight
    /// off-topic.
    const TOPK_QUERY: &str =
        "How have market breadth and credit risk evolved heading into a possible cycle turn?";
    const TOPK_RELEVANT: [&str; 4] = [
        "Market breadth narrowed sharply as fewer stocks carried the index higher.",
        "High-yield credit spreads widened, signaling rising risk aversion.",
        "The yield curve re-steepened from deep inversion, a classic late-cycle signal.",
        "Defensive sectors began outperforming cyclicals as the cycle matured.",
    ];
    const TOPK_IRRELEVANT: [&str; 8] = [
        "Quarterly earnings season featured several large-cap technology beats.",
        "Mortgage rates drifted lower, lifting refinancing activity.",
        "Consumer sentiment ticked up on falling gasoline prices.",
        "Retail sales rose modestly, led by online spending.",
        "A major chipmaker announced a new data-center accelerator.",
        "Crude inventories built more than expected on weak refinery demand.",
        "The trade deficit narrowed as exports of capital goods rose.",
        "Home construction starts edged higher in the Sun Belt.",
    ];

    /// Calibrates `MEMORY_TOP_K` against real geometry. Top-k's value judgment is
    /// inherently observational — "is 5 right?" depends on the real corpus — so the hard
    /// assertion is the robust invariant (every relevant fragment out-ranks every
    /// off-topic one), and the printed ranking with the rank-5 cut marked lets a human
    /// see whether 5 captures the relevant set without dragging in noise.
    #[test]
    #[ignore = "hits the live OpenAI embeddings API; set OPENAI_API_KEY (~13 calls)"]
    fn tuning_topk_selectivity_probe() {
        let embedder = crate::embedding::OpenAiEmbedder::from_env().expect("OPENAI_API_KEY set");
        let qv = embedder.embed(TOPK_QUERY).expect("live embedding call");

        let mut ranked: Vec<(bool, f64, &str)> = TOPK_RELEVANT
            .iter()
            .map(|t| (true, *t))
            .chain(TOPK_IRRELEVANT.iter().map(|t| (false, *t)))
            .map(|(rel, t)| {
                let v = embedder.embed(t).expect("live embedding call");
                (rel, crate::vector_memory::cosine_similarity(&qv, &v), t)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        for (i, (rel, c, t)) in ranked.iter().enumerate() {
            let tag = if *rel { "REL" } else { "   " };
            let cut = if i + 1 == MEMORY_TOP_K { "  <-- top-k cut" } else { "" };
            eprintln!("{:>2}. {c:.4} [{tag}] {t}{cut}", i + 1);
        }

        let worst_relevant = ranked
            .iter()
            .filter(|r| r.0)
            .map(|r| r.1)
            .fold(f64::INFINITY, f64::min);
        let best_irrelevant = ranked
            .iter()
            .filter(|r| !r.0)
            .map(|r| r.1)
            .fold(f64::NEG_INFINITY, f64::max);
        let rel_in_topk = ranked.iter().take(MEMORY_TOP_K).filter(|r| r.0).count();
        eprintln!(
            "topk probe: {rel_in_topk}/{} relevant within top-{MEMORY_TOP_K} | \
             worst_relevant={worst_relevant:.4} best_irrelevant={best_irrelevant:.4}",
            TOPK_RELEVANT.len()
        );

        // Clean separation: real geometry ranks every on-topic fragment above every off-topic one.
        assert!(
            worst_relevant > best_irrelevant,
            "relevant fragments ({worst_relevant:.4}) did not cleanly out-rank off-topic ones ({best_irrelevant:.4})"
        );
    }
}
