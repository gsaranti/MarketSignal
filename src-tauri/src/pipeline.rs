//! Application-layer orchestration for a single manual report run: the app layer
//! drives the agent stage (a pure function) and owns every side effect — the database
//! write and the canonical Markdown file. Written free of any Tauri runtime so an
//! integration test can drive it directly against stubs.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::agent::{
    AnalystAgent, AnalystOutput, MainAgent, MainAgentInput, Posture, RecentReport, ReportSummary,
    StubAnalystAgent,
};
use crate::analyst_agent::ModelAnalystAgent;
use crate::baseline_delta::{self, BaselineDeltas};
use crate::baseline_history::{self, BaselineHistory, PriorBaseline};
use crate::cadence;
use crate::data_sources::{
    BaselineMarketData, GroupKind, MarketDataSource, BASELINE_SCHEMA_VERSION,
};
use crate::document_parser::{self, ParsedResearchDoc};
use crate::embedding::Embedder;
use crate::headline_filter::HeadlineFilter;
use crate::market_clock::MarketClock;
use crate::model_agent::MainAgentConfig;
use crate::news::{self, NewsSource};
use crate::progress::RunContext;
use crate::research_executor::{execute_research, select_branch_policy, SearchBackend, WallClock};
use crate::research_packet::{build_condensed_packet, ResearchPacket};
use crate::research_router::{ResearchPlan, ResearchRouter, RouterInput};
use crate::storage::{self, ReportRecord};
use crate::vector_memory::{self, MemoryKind};

/// Filesystem locations a run reads and writes, injected so tests can point at
/// temporary directories.
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
    /// The canonical app-data layout, shared by the command (`lib.rs::report_paths`)
    /// and every test's temp dir so the two can never drift.
    pub fn under(base: &std::path::Path) -> Self {
        Self {
            db_path: base.join("market_signal.db"),
            reports_dir: base.join("reports"),
            inbox_dir: base.join("research-inbox"),
            archive_dir: base.join("research-archive"),
        }
    }
}

/// The research-half stages (Steps 7–11), injected as trait objects so the spine stays
/// offline-stubbable. The Step-9 clock is deliberately not a member: the research
/// budget anchors at the research phase, so `assemble_research_packet` mints a fresh
/// `WallClock` there rather than letting the baseline scan eat into the budget.
pub struct ResearchStages {
    pub news: Box<dyn NewsSource>,
    pub filter: Box<dyn HeadlineFilter>,
    pub router: Box<dyn ResearchRouter>,
    pub search: Box<dyn SearchBackend>,
}

impl ResearchStages {
    /// Deterministic stand-ins so `generate_report` runs end to end with no live keys.
    pub fn stub() -> Self {
        Self {
            news: Box::new(crate::news::StubNewsSource),
            filter: Box::new(crate::headline_filter::StubHeadlineFilter),
            router: Box::new(crate::research_router::StubResearchRouter),
            search: Box::new(crate::research_executor::StubSearchBackend),
        }
    }

    /// The live bundle, the single construction shared by the command paths (`lib.rs`)
    /// and the live research smoke so the wiring can't drift. Tavily comes first in the
    /// news composite so dedup keeps the primary source's framing; a second Tavily
    /// client is the Step-9 search backend, since the composite owns the gather's by
    /// value. Errors here are HTTP-client build failures only.
    pub fn live(
        tavily_key: String,
        fmp_key: String,
        openai_key: String,
        anthropic_key: String,
        ctx: &std::sync::Arc<RunContext>,
    ) -> Result<Self> {
        Ok(Self {
            news: Box::new(news::CompositeNewsSource::new(
                crate::tavily::TavilyNewsSource::new(tavily_key.clone())?.with_context(ctx.clone()),
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

/// The three analyst stages (Steps 12–15), injected like [`ResearchStages`]. The trait
/// objects are `Send + Sync` so [`run_analysts`]'s scoped threads can borrow them.
pub struct AnalystStages {
    pub bull: Box<dyn AnalystAgent + Send + Sync>,
    pub bear: Box<dyn AnalystAgent + Send + Sync>,
    pub balanced: Box<dyn AnalystAgent + Send + Sync>,
}

impl AnalystStages {
    /// Deterministic stand-ins so `generate_report` runs end to end with no live keys.
    pub fn stub() -> Self {
        Self {
            bull: Box::new(StubAnalystAgent::new(Posture::Bull)),
            bear: Box::new(StubAnalystAgent::new(Posture::Bear)),
            balanced: Box::new(StubAnalystAgent::new(Posture::Balanced)),
        }
    }

    /// The live bundle; each posture's resolved model + key comes from
    /// `AppConfig::analyst_config`. Errors here are HTTP-client build failures only.
    pub fn live(
        bull: MainAgentConfig,
        bear: MainAgentConfig,
        balanced: MainAgentConfig,
        ctx: &std::sync::Arc<RunContext>,
    ) -> Result<Self> {
        Ok(Self {
            bull: Box::new(ModelAnalystAgent::new(Posture::Bull, bull)?.with_context(ctx.clone())),
            bear: Box::new(ModelAnalystAgent::new(Posture::Bear, bear)?.with_context(ctx.clone())),
            balanced: Box::new(
                ModelAnalystAgent::new(Posture::Balanced, balanced)?.with_context(ctx.clone()),
            ),
        })
    }
}

/// Run the three analyst agents concurrently over the shared condensed packet
/// (`docs/report-workflow.md §Step 12`), on scoped OS threads rather than `tokio` to
/// keep the agent half off the async runtime. Reviews come back in Bull/Bear/Balanced
/// order. Not fail-soft: any analyst error that survives its retry, or a panicked
/// thread, propagates as a job failure.
fn run_analysts(
    analysts: &AnalystStages,
    packet: &ResearchPacket,
    cadence: cadence::ReportCadence,
    history: Option<&BaselineHistory>,
) -> Result<Vec<AnalystOutput>> {
    let (bull, bear, balanced) = std::thread::scope(|s| {
        let bull = s.spawn(|| analysts.bull.review(packet, cadence, history));
        let bear = s.spawn(|| analysts.bear.review(packet, cadence, history));
        let balanced = s.spawn(|| analysts.balanced.review(packet, cadence, history));
        (bull.join(), bear.join(), balanced.join())
    });
    let mut reviews = Vec::with_capacity(3);
    for joined in [bull, bear, balanced] {
        match joined {
            Ok(Ok(review)) => reviews.push(review),
            Ok(Err(e)) => return Err(e),
            Err(_) => bail!("an analyst stage thread panicked"),
        }
    }
    Ok(reviews)
}

/// The result of a report run; carries the Markdown so the frontend can render it
/// immediately.
#[derive(Debug, Serialize)]
pub struct GeneratedReport {
    pub report_id: String,
    pub markdown: String,
    pub markdown_path: String,
    pub summary: ReportSummary,
}

/// Run one manual report end to end. The baseline adapters degrade partial failures to
/// a gap manifest, so `enforce_coverage` is the single place a too-thin baseline fails
/// the run (`docs/scheduling.md §Offline Behavior`); the research half is fully
/// fail-soft and never gates a run (`assemble_research_packet`).
///
/// `embedder` is the vector-memory seam (`docs/report-workflow.md §§4, 10, 17`). `ctx`
/// is the run's progress/cancel context: each stage brackets its work in a
/// `step_started` / `step_finished` pair, and a cancel is observed at the boundaries
/// between stages (`bail_if_cancelled`) — a `reqwest::blocking` call already in flight
/// is not interrupted — surfacing as an error `jobs::run_job` maps to a Cancelled
/// outcome. Tests pass the stub bundles and `RunContext::noop()`.
pub fn generate_report(
    agent: &dyn MainAgent,
    data: &dyn MarketDataSource,
    research: &ResearchStages,
    analysts: &AnalystStages,
    embedder: &dyn Embedder,
    paths: &ReportPaths,
    ctx: &RunContext,
) -> Result<GeneratedReport> {
    // The app-minted "as of" time for this run's baseline, the anchor for the snapshot's
    // `captured_at` and the change view's elapsed interval. Deliberately distinct from
    // the agent-minted `created_at`; the snapshot↔report join is by `report_id`, not time.
    let as_of = chrono::Utc::now();

    // Computed independently of `deltas` (from the prior snapshot's timestamp alone) so
    // a prior baseline that won't decode still gets the true interval rather than
    // first-report treatment, and computed up here because the baseline scan sizes its
    // cadence-dependent lookback windows (the earnings and release calendars) from it.
    let cadence = compute_cadence(&paths.db_path, as_of);

    let market_clock = MarketClock::from_utc(as_of);

    // The run's date in market (Eastern) time: the recency anchor for the news-selection
    // stages, never guessed from publish dates.
    let report_date = market_clock.report_date();

    // Each adapter records what it couldn't resolve in `baseline.gaps` instead of
    // failing (`docs/report-workflow.md §Step 3`); the coverage gate then decides
    // whether what landed clears the floor, and a gate failure propagates unwrapped so
    // `jobs::run_job` persists the floor message as the failed-job detail. Gaps that
    // don't trip the floor ride into the model prompt.
    ctx.step_started("baseline", "Gathering baseline market data");
    let baseline = match data.baseline_scan(cadence) {
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

    // Both reads are best-effort (`None` on a first report or any failure) and run before
    // `baseline` moves into the agent input; the serialization is captured for the same
    // reason.
    let deltas = compute_prior_deltas(&paths.db_path, &baseline, as_of);
    // Its own best-effort read, so a corrupt latest snapshot costs the change view
    // without costing the history's other points.
    let history = compute_baseline_history(&paths.db_path, &baseline, as_of);
    let baseline_json = serde_json::to_string(&baseline).ok();

    // Best-effort: a first report or any DB failure degrades to empty and never gates
    // the run.
    let recent_reports = load_recent_report_context(&paths.db_path);

    // Fully fail-soft (`docs/report-workflow.md §Step 6`, `docs/research-documents.md`):
    // failures are recorded for the panel and the file is left in the inbox for the next
    // run. The archive move waits for the persist step, so a failed or cancelled run
    // never consumes the user's documents.
    ctx.step_started("inbox", "Reading research documents");
    let inbox_docs = process_research_inbox(&paths.inbox_dir, &paths.db_path, ctx);
    let inbox_status = if ctx.is_cancelled() {
        "cancelled"
    } else {
        "ok"
    };
    ctx.step_finished("inbox", inbox_status, None);
    bail_if_cancelled(ctx)?;

    // Fully fail-soft: the research half never gates the run (`assemble_research_packet`).
    ctx.step_started("research", "Gathering and condensing research");
    let AssembledResearch {
        packet: research_packet,
        audit_memory,
    } = assemble_research_packet(
        research,
        &baseline,
        deltas.as_ref(),
        cadence,
        report_date.as_deref(),
        &recent_reports,
        &inbox_docs,
        embedder,
        &paths.db_path,
        ctx,
    );
    let research_status = if ctx.is_cancelled() {
        "cancelled"
    } else {
        "ok"
    };
    ctx.step_finished("research", research_status, None);
    bail_if_cancelled(ctx)?;

    // Best-effort; never gates the run. Read here rather than at the top so the freshest
    // persisted reports are picked up after any upstream work.
    let recent_reports_for_audit = load_recent_reports_for_audit(&paths.db_path);

    // Unlike the fail-soft research half, an analyst failure is a job failure
    // (`run_analysts`).
    ctx.step_started("analysts", "Running the analyst agents");
    let analyst_reviews = match run_analysts(
        analysts,
        &research_packet,
        cadence,
        history.as_ref(),
    ) {
        Ok(reviews) => reviews,
        Err(e) => {
            let status = if ctx.is_cancelled() {
                "cancelled"
            } else {
                "failed"
            };
            ctx.step_finished("analysts", status, Some(e.to_string()));
            return Err(e);
        }
    };
    ctx.step_finished("analysts", "ok", None);
    bail_if_cancelled(ctx)?;

    ctx.step_started("agent", "Main agent writing the report");
    let output = match agent.generate(MainAgentInput {
        baseline,
        deltas,
        history,
        cadence,
        market_clock,
        research: Some(research_packet),
        audit_memory,
        recent_reports: recent_reports_for_audit,
        analyst_reviews,
    }) {
        Ok(output) => output,
        Err(e) => {
            // A cancel observed mid-stream surfaces as a parse error here; mark the
            // step cancelled (not failed) so the tracker reads consistently with the
            // run's Cancelled outcome.
            let status = if ctx.is_cancelled() {
                "cancelled"
            } else {
                "failed"
            };
            ctx.step_finished("agent", status, Some(e.to_string()));
            return Err(e);
        }
    };
    ctx.step_finished("agent", "ok", None);
    bail_if_cancelled(ctx)?;

    // The `?` ergonomics stay inside an immediately-invoked closure so any failure still
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

        // Best-effort: a snapshot or prune failure must never lose a report that already
        // persisted, so errors are logged to stderr and swallowed; the log keeps a
        // persistent schema/permission fault from silently disabling baseline history.
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

        // Best-effort truncation telemetry for head-truncated inbox documents, under the
        // same swallow-and-log discipline as the snapshot block; no row when nothing
        // overflowed.
        let truncations =
            collect_document_truncations(&inbox_docs, &summary.report_id, &as_of.to_rfc3339());
        if !truncations.is_empty() {
            if let Err(e) = storage::record_document_truncations(&conn, &truncations) {
                eprintln!(
                    "truncation-telemetry persist failed for report {}: {e:#}",
                    summary.report_id
                );
            }
        }

        // Best-effort: the denominator for the truncation rate, written whenever at
        // least one document parsed so a zero-truncation run still counts; same
        // swallow-and-log posture as above.
        if !inbox_docs.is_empty() {
            let total_original_chars: u64 =
                inbox_docs.iter().map(|d| d.original_chars as u64).sum();
            if let Err(e) = storage::record_document_parse_run(
                &conn,
                &summary.report_id,
                &as_of.to_rfc3339(),
                inbox_docs.len() as u64,
                total_original_chars,
            ) {
                eprintln!(
                    "parse-run telemetry persist failed for report {}: {e:#}",
                    summary.report_id
                );
            }
        }

        // Best-effort, same posture as the snapshot block: an embedding or store failure
        // costs this report's memory row, never the report. Both memory legs poll
        // cancellation before each paid embedding call (`docs/run-tracking.md
        // §Cancellation`); the report persisted above stays, and a cancel this late is
        // honored as a successful run (`jobs::run_job`) with only the memory rows skipped.
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

        // Best-effort (`docs/report-workflow.md §Step 17`).
        persist_durable_learnings(
            &conn,
            embedder,
            &output.durable_learnings,
            &summary.report_id,
            &summary.created_at,
            ctx,
        );

        // Run after this run's insert so the new report counts toward the window; a
        // cascade failure never fails the run.
        prune_old_reports(&conn);

        // Only now, with the report persisted, does the run count as having consumed the
        // documents (`docs/research-documents.md §Processing at Job Start`). A cancel
        // during persist doesn't skip this leg: the persisted report used them.
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

/// The report retention cascade (`docs/storage.md §SQLite`): every report beyond the
/// newest [`storage::REPORT_RETENTION`] is deleted with its artifacts; durable
/// learnings survive by `kind`. Per-evictee best-effort, never failing the run. The
/// file leg goes first: a file already gone counts as removed, but any other removal
/// failure skips that evictee's DB legs (it is re-selected next run) rather than
/// deleting the row and orphaning an untracked file. The DB legs commit or roll back
/// as one transaction: re-selection reads the `reports` table, so deleting the row
/// while another leg failed would strand that leg's rows with no retry path, and
/// neither the vector summary row nor the document-diagnostics rows have another
/// reaper.
fn prune_old_reports(conn: &rusqlite::Connection) {
    let evictees = match storage::select_reports_beyond_retention(conn, storage::REPORT_RETENTION) {
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

/// One evictee's SQLite legs, committed together or not at all (`unchecked_transaction`:
/// the helpers take `&Connection`). The two document-diagnostics legs are load-bearing:
/// neither `document_truncations` (numerator) nor `document_parse_runs` (denominator)
/// has a self-cap, so this report-id join is the only thing that bounds them, and they
/// delete together so the truncation rate's window stays aligned. A separate self-cap
/// for the two tables was deliberately rejected: pruning them on separate schedules
/// would orphan a numerator from its denominator.
fn delete_report_db_rows(conn: &rusqlite::Connection, report_id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    vector_memory::delete_report_summary(&tx, report_id)?;
    storage::delete_report_baseline_snapshots(&tx, report_id)?;
    storage::delete_report_truncations(&tx, report_id)?;
    storage::delete_report_parse_runs(&tx, report_id)?;
    storage::delete_report_row(&tx, report_id)?;
    tx.commit()?;
    Ok(())
}

/// The condensed packet plus the Step-4 pre-research pull on its own channel, distinct
/// from `packet.memory` (the Step-10 pull; `docs/report-workflow.md §Step 10`).
struct AssembledResearch {
    packet: ResearchPacket,
    /// Most relevant first; empty when nothing was recalled or the pull was skipped
    /// (retrieval failure, cancel).
    audit_memory: Vec<String>,
}

/// Run the research half and condense it into the packet the main agent reasons over
/// (`docs/report-workflow.md §§7–11`). Fully fail-soft: a failure in any stage degrades
/// that stage to empty and the phase continues, so this always returns a packet, never
/// an error; only the baseline coverage floor gates a run. Cancellation is polled at
/// each stage boundary here and at each request boundary inside the executor, so a
/// cancel mid-research skips the remaining model calls rather than spending them.
///
/// The Step-4 pre-research pull steers investigation: it feeds the router and is
/// returned as [`AssembledResearch::audit_memory`] on its own channel, kept out of the
/// packet. The Step-10 post-research pull is the one the packet carries.
#[allow(clippy::too_many_arguments)] // one parameter per research-half input
fn assemble_research_packet(
    research: &ResearchStages,
    baseline: &BaselineMarketData,
    deltas: Option<&BaselineDeltas>,
    cadence: cadence::ReportCadence,
    // `YYYY-MM-DD` in market (Eastern) time, the news stages' recency anchor; `None`
    // only off the live path.
    report_date: Option<&str>,
    recent_reports: &[ReportSummary],
    inbox_docs: &[ParsedResearchDoc],
    embedder: &dyn Embedder,
    db_path: &std::path::Path,
    ctx: &RunContext,
) -> AssembledResearch {
    // The news adapters size their recency windows to the cadence, so a daily run isn't
    // fed a fixed week of news and a monthly run isn't starved of it.
    let headlines = match research.news.gather(cadence) {
        Ok(raw) => news::dedupe_headlines(raw),
        Err(e) => {
            eprintln!("research: news gather degraded to empty: {e:#}");
            Vec::new()
        }
    };

    // An empty gather skips the model call entirely, and a cancel during the gather
    // short-circuits here so a stop never still spends the filter call.
    let clusters = if headlines.is_empty() || ctx.is_cancelled() {
        Vec::new()
    } else {
        match research.filter.filter(headlines, report_date) {
            Ok(clusters) => clusters,
            Err(e) => {
                eprintln!("research: headline filter degraded to empty: {e:#}");
                Vec::new()
            }
        }
    };

    // A cancel must not spend the embedding call.
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

    // A cancel between stages must not spend the routing call.
    let plan = if ctx.is_cancelled() {
        ResearchPlan::default()
    } else {
        match research.router.route(RouterInput {
            baseline: baseline.clone(),
            deltas: deltas.cloned(),
            clusters: clusters.clone(),
            recent_reports: recent_reports.to_vec(),
            // Cloned because the pull is also returned below as `audit_memory`.
            memory: pre_memory.clone(),
            // Routing picks topics, so it gets each document's head, not the full
            // condensed text the packet carries below.
            inbox_documents: inbox_docs
                .iter()
                .map(ParsedResearchDoc::router_excerpt)
                .collect(),
            report_date: report_date.map(str::to_string),
        }) {
            Ok(plan) => plan,
            Err(e) => {
                eprintln!("research: router degraded to empty: {e:#}");
                ResearchPlan::default()
            }
        }
    };

    // The clock anchors here so the research budget covers the research phase, not the
    // baseline scan that preceded it.
    let clock = WallClock::new();
    let policy = select_branch_policy(deltas);
    let evidence = execute_research(
        &plan,
        research.search.as_ref(),
        policy.as_ref(),
        &clock,
        ctx,
    );

    // Recalled against what the research found; the only memory the packet carries.
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

    // The inbox blocks are already bounded by `document_parser`'s char budgets.
    let packet = build_condensed_packet(
        baseline.clone(),
        deltas.cloned(),
        clusters,
        evidence,
        memory,
        inbox_docs
            .iter()
            .map(ParsedResearchDoc::prompt_block)
            .collect(),
    );

    AssembledResearch {
        packet,
        audit_memory: pre_memory,
    }
}

/// Parse the inbox (fail-soft) and record this pass's failure set for the Research
/// Documents panel. The failure write is best-effort and skipped on a cancelled pass: a
/// partial pass must not clobber the recorded state of files it never reached.
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

/// The table holds the failures of the most recent inbox pass, so a healed or deleted
/// file self-clears.
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

/// One row per head-truncated inbox document; the pure derivation is split out as the
/// unit-test seam. Whole documents produce no row.
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

/// Cancel checkpoint between pipeline stages: the run stops with an error
/// `jobs::run_job` reclassifies as Cancelled (off the shared flag) rather than Failed.
/// An in-flight HTTP request is never interrupted.
fn bail_if_cancelled(ctx: &RunContext) -> Result<()> {
    if ctx.is_cancelled() {
        bail!("run cancelled before completion");
    }
    Ok(())
}

/// The run's report cadence, computed from the latest snapshot's `captured_at` alone —
/// the baseline JSON is never decoded — so an undecodable prior still yields the true
/// interval. Deliberately not built on [`compute_prior_deltas`], which collapses a
/// first report, a DB error and an undecodable prior into the same `None` and would
/// mislabel an existing user's run as a first report.
fn compute_cadence(
    db_path: &std::path::Path,
    as_of: chrono::DateTime<chrono::Utc>,
) -> cadence::ReportCadence {
    let elapsed_days: Option<f64> = read_db_fail_soft(db_path, "cadence elapsed read", |conn| {
        let Some((captured_at, _baseline_json)) = storage::latest_baseline_snapshot(conn)? else {
            return Ok(None);
        };
        let prior_at = chrono::DateTime::parse_from_rfc3339(&captured_at)
            .context("parsing prior snapshot captured_at")?
            .with_timezone(&chrono::Utc);
        Ok(Some(elapsed_days_since(as_of, prior_at)))
    });
    cadence::ReportCadence::from_elapsed(elapsed_days)
}

/// Fractional days from `prior_at` to `as_of`, floored at zero — see
/// [`compute_prior_deltas`] for why a negative interval must not reach a prompt.
fn elapsed_days_since(
    as_of: chrono::DateTime<chrono::Utc>,
    prior_at: chrono::DateTime<chrono::Utc>,
) -> f64 {
    ((as_of - prior_at).num_seconds() as f64 / 86_400.0).max(0.0)
}

/// Best-effort change view against the previous report's snapshot. `None` — the run
/// proceeds without deltas — for a first report, any DB error, or a prior blob that
/// won't decode; only the absent prior is the silent `None`, the rest leave a labeled
/// stderr trace through [`read_db_fail_soft`]. Cadence is computed separately
/// ([`compute_cadence`]) so an undecodable prior here still yields the true interval.
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
        // Non-negative: the interval is prompt-facing, and a backwards clock step between
        // runs would otherwise hand the agent a negative "days since the last report";
        // the cadence bands read zero as `Intraday`, the honest bucket for two runs at
        // the same instant.
        let elapsed_days = elapsed_days_since(as_of, prior_at);
        Ok(Some(baseline_delta::compute_deltas(
            current,
            &prior,
            elapsed_days,
        )))
    })
}

/// Best-effort: a snapshot whose blob won't decode or whose `captured_at` won't parse
/// loses only its own point, while a DB failure degrades the whole history to `None`.
/// Never a gate (`docs/storage.md §Baseline Snapshots`).
fn compute_baseline_history(
    db_path: &std::path::Path,
    current: &BaselineMarketData,
    as_of: chrono::DateTime<chrono::Utc>,
) -> Option<BaselineHistory> {
    read_db_fail_soft(db_path, "baseline-history read", |conn| {
        let stored =
            storage::recent_baseline_snapshots(conn, storage::BASELINE_SNAPSHOT_RETENTION)?;
        let priors: Vec<PriorBaseline> = stored
            .iter()
            .filter_map(|row| {
                decode_prior_baseline(row)
                    .map_err(|e| {
                        eprintln!(
                            "baseline-history read: skipped the snapshot of report {}: {e:#}",
                            row.report_id
                        )
                    })
                    .ok()
            })
            .collect();
        Ok(baseline_history::build_history(current, as_of, &priors))
    })
}

/// One stored snapshot decoded into a history point's input: its scan and its parsed
/// capture instant.
fn decode_prior_baseline(row: &storage::StoredBaselineSnapshot) -> Result<PriorBaseline> {
    let baseline = serde_json::from_str(&row.baseline_json).context("decoding baseline snapshot")?;
    let captured_at = chrono::DateTime::parse_from_rfc3339(&row.captured_at)
        .context("parsing snapshot captured_at")?
        .with_timezone(&chrono::Utc);
    Ok(PriorBaseline {
        report_id: row.report_id.clone(),
        captured_at,
        baseline,
    })
}

/// Bounded count of recent report summaries handed to research routing
/// (`docs/report-workflow.md §Step 8`); three ≈ three weeks of thesis arc at
/// negligible prompt cost.
const ROUTER_RECENT_REPORTS: u32 = 3;

/// Best-effort, never a gate: a first report or any DB failure degrades to empty.
fn load_recent_report_context(db_path: &std::path::Path) -> Vec<ReportSummary> {
    read_db_fail_soft(db_path, "research recent-report context", |conn| {
        storage::list_recent_reports(conn, ROUTER_RECENT_REPORTS)
    })
}

/// Best-effort `open + init_schema + read` against the report DB, degrading to
/// `T::default()` with a `context`-prefixed stderr log on any failure: the shared shell
/// behind every fail-soft DB read in the pipeline, never a gate.
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

/// Bounded count of recent prior reports handed to the main agent
/// (`docs/report-workflow.md §Step 2`); a separate constant from
/// `ROUTER_RECENT_REPORTS` so the audit window can widen without moving routing's.
const MAIN_AGENT_RECENT_REPORTS: u32 = 3;

/// Per-report ceiling on the Markdown body carried in the Step-2 context, in chars
/// (~5k tokens), sized to clear a typical full report whole; applied per report, so
/// the context can carry up to 3× this.
const RECENT_REPORT_BODY_CAP: usize = 20_000;

/// Best-effort like [`load_recent_report_context`]: a DB failure degrades the whole
/// list to empty, and an unreadable Markdown file drops that one report's body to
/// empty while its summary still carries.
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

/// Head-truncate a report body to `cap` chars with a visible marker when cut; counts
/// and slices by chars, never bytes.
fn truncate_report_body(body: &str, cap: usize) -> String {
    let total = body.chars().count();
    if total <= cap {
        return body.to_string();
    }
    let head: String = body.chars().take(cap).collect();
    format!("{head}\n[truncated — showing the first {cap} of {total} characters]")
}

/// Fragments each vector-memory pull recalls. Selectivity comes from top-k over a small
/// corpus; there is no similarity floor (`docs/report-workflow.md §Step 4`).
const MEMORY_TOP_K: usize = 5;

/// Per-report cap on durable-learning rows the persist write accepts: the store's
/// growth must not depend on the model honoring the prompt's self-bound, and learnings
/// are never deleted, so overflow would be permanent. Overflow is truncated with a
/// stderr note, never a failed report.
const LEARNINGS_PER_REPORT_CAP: usize = 5;

/// Cosine similarity at or above which a fresh durable learning is dropped as a
/// near-restatement of one the store already holds; learnings are never deleted, so
/// unbounded restatement is permanent growth that dilutes retrieval. Calibrated against
/// `text-embedding-3-large` geometry by `tuning_dedup_threshold_calibration`: genuine
/// restatements embed at ~0.72–0.81 cosine while distinct lessons cap at ~0.53, so 0.65
/// sits in that measured gap. A dedup-scan failure fails open (the learning is kept).
const LEARNING_DEDUP_THRESHOLD: f64 = 0.65;

/// One best-effort vector-memory pull, most relevant first; any failure degrades to
/// empty with a stderr note (`label` names which pull). An empty query and an empty
/// store both skip the paid embedding call. An oversized query is truncated in the
/// adapters ([`crate::embedding::EMBEDDING_INPUT_MAX_BYTES`]) rather than rejected by
/// the provider, which would cost the whole pull.
fn retrieve_memory(
    db_path: &std::path::Path,
    embedder: &dyn Embedder,
    query: &str,
    label: &str,
) -> Vec<String> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    read_db_fail_soft(
        db_path,
        &format!("research {label} memory retrieval"),
        |conn| {
            if vector_memory::count_memory(conn)? == 0 {
                return Ok(Vec::new());
            }
            let embedding = embedder.embed(query)?;
            let hits = vector_memory::search_memory(conn, &embedding, None, MEMORY_TOP_K)?;
            Ok(hits
                .iter()
                .map(vector_memory::MemoryHit::prompt_fragment)
                .collect())
        },
    )
}

/// Embed and store a run's durable learnings (`docs/report-workflow.md §Step 17`,
/// `docs/storage.md §Embeddings`). The app-layer bounds live here, not in the model
/// contract. Every write is independently best-effort: one failed embed, dedup scan
/// or insert costs that learning, never its siblings or the report. Dedup reuses the
/// embedding just computed and scans the store including this run's own prior
/// inserts, so two near-identical learnings in one run collapse to one. `report_id` is
/// provenance only: the `kind` column is what makes learnings survive the retention
/// cascade.
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
        // Fails open: a scan error keeps the learning, so dedup can only ever drop a
        // redundant row, never lose a real one.
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

/// The minimum fraction of a group's expected series that must resolve for the coverage
/// floor. "Expected" excludes permanently out-of-scope items, so only this-run failures
/// (`Unavailable` / `Rejected` / `Malformed` gaps) count against it. 0.6 ≈ three of
/// four indices.
const COVERAGE_FLOOR: f64 = 0.6;

/// Resolved series over expected (resolved + this-run gaps). An expected count of zero
/// reads as fully covered, but `clears_floor` additionally requires a non-empty group,
/// so a vacuous 1.0 over zero series can't satisfy the floor.
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
        GroupKind::CotPositioning => data.cot_positioning.len(),
        GroupKind::PrimaryMarket => data.primary_market.as_ref().map_or(0, |p| {
            usize::from(p.ipos.is_some()) + usize::from(p.deals.is_some())
        }),
    }
}

/// The non-empty requirement guards the vacuous "0 of 0 = 100%" case: a floor group
/// always has a fixed expected set, so zero resolved is a failure whatever the ratio.
fn clears_floor(data: &BaselineMarketData, group: GroupKind) -> bool {
    group_present_count(data, group) > 0 && group_coverage(data, group) >= COVERAGE_FLOOR
}

/// The Step-3 coverage gate, the one place a too-thin merged baseline fails the run:
/// the report is structurally impossible without the Index Picture, and its
/// risk-posture / market-cycle reads need at least one grounded macro/internals group,
/// so `indices` must clear and at least one of {`internals`, `macro_levels`} must.
/// Everything else degrades to a manifest gap the agent reasons over.
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

pub fn list_reports(paths: &ReportPaths) -> Result<Vec<ReportSummary>> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;
    storage::list_recent_reports(&conn, storage::RECENT_REPORTS_LIMIT)
}

/// An unknown id, or a Markdown file removed out-of-band, surfaces as a typed error the
/// UI renders.
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

/// Export one report's canonical Markdown from the stored artifacts only
/// (`docs/export.md §Export Behavior`), so an export never re-runs the workflow and
/// never trusts an in-memory copy.
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

/// The canonical Markdown filename, `YYYY-MM-DD-market-signal-report-<id8>.md`. The
/// date is local (`docs/scheduling.md`: reports are local-time artifacts) even though
/// the persisted `created_at` stays UTC; `tz` is injected so the midnight-boundary
/// behavior is testable. The `report_id` prefix keeps a same-date rerun from
/// overwriting an earlier run's file.
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
    Ok(format!("{local_date}-market-signal-report-{id8}.md"))
}

/// The export filename (`docs/export.md §Export Naming`),
/// `YYYY-MM-DD-market-signal-report.<ext>`. Deliberately carries no `-<id8>` suffix:
/// same-name collisions are the user's save-dialog overwrite prompt. `ext` is the bare
/// extension (`"md"`, `"pdf"`).
pub fn export_basename<Tz: chrono::TimeZone>(created_at: &str, ext: &str, tz: &Tz) -> Result<String>
where
    Tz::Offset: std::fmt::Display,
{
    let local_date = local_date_segment(created_at, tz)?;
    Ok(format!("{local_date}-market-signal-report.{ext}"))
}

/// The `YYYY-MM-DD` segment: the UTC `created_at` rendered in `tz`'s calendar. A
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

    use crate::data_sources::{Change, DataGap, GapReason, Quote};

    // ---- Step-3 coverage gate ----

    /// `n` placeholder resolved quotes for a group.
    fn covq(n: usize) -> Vec<Quote> {
        (0..n)
            .map(|i| Quote {
                symbol: format!("S{i}"),
                name: format!("S{i}"),
                price: 1.0,
                change: Change::percent(0.0),
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
        let mut gaps = covgaps(GroupKind::Internals, GapReason::Rejected, 9);
        gaps.extend(covgaps(GroupKind::MacroLevels, GapReason::Rejected, 17));
        let data = BaselineMarketData {
            indices: covq(4),
            gaps,
            ..Default::default()
        };
        let err = enforce_coverage(&data).unwrap_err().to_string();
        assert!(
            err.contains("macro levels") && err.contains("internals"),
            "{err}"
        );
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
        // 3 resolved + 1 OutOfScope gap = 3/3: a permanent absence never fails the floor.
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
        let mut gaps = covgaps(GroupKind::Movers, GapReason::Unavailable, 3);
        gaps.extend(covgaps(GroupKind::Earnings, GapReason::Rejected, 1));
        gaps.extend(covgaps(GroupKind::SectorPe, GapReason::Unavailable, 1));
        gaps.extend(covgaps(GroupKind::Industries, GapReason::Malformed, 2));
        gaps.extend(covgaps(
            GroupKind::MarketRiskPremium,
            GapReason::Rejected,
            1,
        ));
        gaps.extend(covgaps(GroupKind::PrimaryMarket, GapReason::Unavailable, 2));
        let data = BaselineMarketData {
            indices: covq(4),
            internals: covq(9),
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
            name.starts_with("2026-06-02-market-signal-report-"),
            "expected the local (UTC-3) date 2026-06-02, got {name}"
        );
    }

    #[test]
    fn same_date_distinct_report_ids_produce_distinct_filenames() {
        let tz = minus_three();
        let a = canonical_report_filename("2026-06-03T12:00:00Z", "aaaaaaaa-1111", &tz).unwrap();
        let b = canonical_report_filename("2026-06-03T15:00:00Z", "bbbbbbbb-2222", &tz).unwrap();
        assert_ne!(a, b, "same-date reruns must not collide on one filename");
        assert_eq!(a, "2026-06-03-market-signal-report-aaaaaaaa.md");
        assert_eq!(b, "2026-06-03-market-signal-report-bbbbbbbb.md");
    }

    #[test]
    fn short_report_id_does_not_panic_on_the_eight_char_slice() {
        let name =
            canonical_report_filename("2026-06-03T12:00:00Z", "rid", &minus_three()).unwrap();
        assert!(name.ends_with("-rid.md"), "got {name}");
    }

    #[test]
    fn non_rfc3339_created_at_is_a_typed_error() {
        let err = canonical_report_filename("not-a-timestamp", "rid", &minus_three()).unwrap_err();
        assert!(err.to_string().contains("non-RFC3339"), "{err}");
    }

    #[test]
    fn export_basename_has_no_id_suffix_and_carries_the_extension() {
        let md = export_basename("2026-06-03T12:00:00Z", "md", &minus_three()).unwrap();
        assert_eq!(md, "2026-06-03-market-signal-report.md");
        let pdf = export_basename("2026-06-03T12:00:00Z", "pdf", &minus_three()).unwrap();
        assert_eq!(pdf, "2026-06-03-market-signal-report.pdf");
    }

    #[test]
    fn export_basename_uses_local_calendar_date_across_a_midnight_boundary() {
        // 01:30 UTC on the 3rd is 22:30 on the 2nd at UTC-3.
        let name = export_basename("2026-06-03T01:30:00Z", "md", &minus_three()).unwrap();
        assert_eq!(name, "2026-06-02-market-signal-report.md");
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
        fn gather(&self, _cadence: cadence::ReportCadence) -> anyhow::Result<Vec<RawHeadline>> {
            anyhow::bail!("news source down")
        }
    }

    /// A news source that returns no headlines, to drive the empty-gather short-circuit.
    struct EmptyNews;
    impl NewsSource for EmptyNews {
        fn gather(&self, _cadence: cadence::ReportCadence) -> anyhow::Result<Vec<RawHeadline>> {
            Ok(Vec::new())
        }
    }

    /// A filter that panics if called — proves the filter is skipped on an empty gather.
    struct PanicFilter;
    impl HeadlineFilter for PanicFilter {
        fn filter(
            &self,
            _: Vec<RawHeadline>,
            _: Option<&str>,
        ) -> anyhow::Result<Vec<HeadlineCluster>> {
            panic!("filter must not be called when there are no headlines")
        }
    }

    /// A filter that always errors, to drive the filter fail-soft arm.
    struct FailingFilter;
    impl HeadlineFilter for FailingFilter {
        fn filter(
            &self,
            _: Vec<RawHeadline>,
            _: Option<&str>,
        ) -> anyhow::Result<Vec<HeadlineCluster>> {
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
            crate::cadence::ReportCadence::from_elapsed(None),
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
        // A db path whose parent directory doesn't exist can't be opened or created.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-subdir").join("market_signal.db");
        assert!(load_recent_report_context(&path).is_empty());
    }

    #[test]
    fn compute_prior_deltas_degrades_to_none_on_an_unopenable_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-subdir").join("market_signal.db");
        let as_of = chrono::DateTime::parse_from_rfc3339("2026-06-15T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(compute_prior_deltas(&path, &BaselineMarketData::default(), as_of).is_none());
    }

    #[test]
    fn cadence_survives_a_corrupt_prior_snapshot_even_as_the_change_view_degrades() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("market_signal.db");
        let conn = storage::open(&db).unwrap();
        storage::init_schema(&conn).unwrap();
        storage::insert_baseline_snapshot(
            &conn,
            "corrupt-prior",
            "2026-06-08T00:00:00Z",
            BASELINE_SCHEMA_VERSION,
            "this is not valid baseline json {{{",
        )
        .unwrap();
        let as_of = chrono::DateTime::parse_from_rfc3339("2026-06-15T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let cadence = compute_cadence(&db, as_of);
        assert_eq!(
            cadence.bucket(),
            cadence::CadenceBucket::Weekly,
            "a ~7-day interval is recovered despite the corrupt payload"
        );
        assert!(cadence.elapsed_days().is_some(), "the elapsed interval is known");

        assert!(
            compute_prior_deltas(&db, &BaselineMarketData::default(), as_of).is_none(),
            "the corrupt payload drops the diff"
        );
    }

    #[test]
    fn compute_baseline_history_degrades_to_none_on_an_unopenable_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-subdir").join("market_signal.db");
        let as_of = chrono::DateTime::parse_from_rfc3339("2026-06-15T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(compute_baseline_history(&path, &BaselineMarketData::default(), as_of).is_none());
    }

    #[test]
    fn a_corrupt_snapshot_costs_the_history_only_its_own_point() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("market_signal.db");
        let conn = storage::open(&db).unwrap();
        storage::init_schema(&conn).unwrap();
        let with_sp = |price: f64| BaselineMarketData {
            indices: vec![crate::data_sources::Quote {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                price,
                change: crate::data_sources::Change::percent(0.0),
                unit: "index points".into(),
            }],
            ..Default::default()
        };
        let rows = [
            ("rep-a", "2026-06-01T20:00:00Z", serde_json::to_string(&with_sp(5_400.0)).unwrap()),
            ("rep-bad-json", "2026-06-03T20:00:00Z", "not baseline json {{{".to_string()),
            ("rep-b", "2026-06-08T20:00:00Z", serde_json::to_string(&with_sp(5_500.0)).unwrap()),
            ("rep-bad-time", "not a timestamp", serde_json::to_string(&with_sp(1.0)).unwrap()),
        ];
        for (report_id, captured_at, json) in &rows {
            storage::insert_baseline_snapshot(
                &conn,
                report_id,
                captured_at,
                BASELINE_SCHEMA_VERSION,
                json,
            )
            .unwrap();
        }
        let as_of = chrono::DateTime::parse_from_rfc3339("2026-06-15T19:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let history = compute_baseline_history(&db, &with_sp(5_600.0), as_of).unwrap();
        let ids: Vec<Option<&str>> = history
            .points
            .iter()
            .map(|p| p.report_id.as_deref())
            .collect();
        assert_eq!(ids, [Some("rep-a"), Some("rep-b"), None]);
        assert_eq!(
            history.series[0].values,
            [Some(5_400.0), Some(5_500.0), Some(5_600.0)]
        );
    }

    // ---- Step-2 recent prior-report context (`load_recent_reports_for_audit`) ----

    fn audit_summary(id: &str, created_at: &str) -> ReportSummary {
        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        ReportSummary {
            report_id: id.into(),
            report_type: "weekly_market".into(),
            created_at: created_at.into(),
            title: "Audit fixture headline".into(),
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
        assert_eq!(
            truncate_report_body(short, 100),
            short,
            "under the cap rides whole"
        );
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
        assert!(
            recent[1].markdown.contains("[truncated"),
            "an over-cap body is marked"
        );
        assert_eq!(recent[2].summary.report_id, "old");
        assert_eq!(
            recent[2].markdown, "the older report body",
            "an in-cap body rides whole"
        );
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
    fn retrieve_memory_passes_an_oversized_query_through_instead_of_losing_the_pull() {
        let (_dir, path) = seeded_store();
        // The byte cap lives in the adapters (`embedding::bounded_input`), so this seam
        // hands the text through whole.
        let oversized = format!("a{}", "é".repeat(crate::embedding::EMBEDDING_INPUT_MAX_BYTES));
        let recording = RecordingEmbedder(Mutex::new(None));
        let hits = retrieve_memory(&path, &recording, &oversized, "test");
        assert_eq!(hits.len(), 1, "the oversized query still pulls");
        assert_eq!(
            recording.0.lock().unwrap().clone().expect("embedder was called"),
            oversized,
            "the seam no longer caps; the adapter does"
        );
    }

    #[test]
    fn retrieve_memory_skips_the_paid_call_on_an_empty_store_or_empty_query() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("market_signal.db");
        let counting = CountingEmbedder::new();
        assert!(retrieve_memory(&path, &counting, "real query", "test").is_empty());
        assert_eq!(
            counting.calls(),
            0,
            "no embedding call against an empty store"
        );

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

        persist_durable_learnings(
            &conn,
            &BasisEmbedder,
            &[
                "k0 breadth thinned".into(),
                "k0 breadth kept thinning".into(),
            ],
            "rep-1",
            "2026-06-01T00:00:00Z",
            &ctx,
        );
        assert_eq!(learning_count(&conn), 1, "within-run duplicate dropped");

        persist_durable_learnings(
            &conn,
            &BasisEmbedder,
            &["k0 breadth remained narrow".into()],
            "rep-2",
            "2026-06-08T00:00:00Z",
            &ctx,
        );
        assert_eq!(learning_count(&conn), 1, "cross-run duplicate dropped");

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
        assert!(
            !packet.news_clusters.is_empty(),
            "stub chain yields clusters"
        );
        assert!(
            !packet.research.items.is_empty(),
            "stub chain yields evidence"
        );
    }

    /// One recent summary so `pre_research_query` renders a non-blank query; a default
    /// baseline with no recent reports would short-circuit the pull.
    fn one_recent_summary() -> ReportSummary {
        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        ReportSummary {
            report_id: "prior".into(),
            report_type: "weekly_market".into(),
            created_at: "2026-05-20T13:00:00Z".into(),
            title: "Recent fixture headline".into(),
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
        let (_dir, db_path) = seeded_store();
        let recent = [one_recent_summary()];
        let assembled = assemble_research_packet(
            &ResearchStages::stub(),
            &BaselineMarketData::default(),
            None,
            crate::cadence::ReportCadence::from_elapsed(None),
            None,
            &recent,
            &[],
            &StubEmbedder,
            &db_path,
            &RunContext::noop(),
        );
        assert_eq!(
            assembled.audit_memory.len(),
            1,
            "the Step-4 pull reached audit_memory"
        );
        assert!(
            assembled.audit_memory[0].starts_with("[learning · "),
            "audit memory carries the store's prompt fragment, got {:?}",
            assembled.audit_memory[0]
        );
    }

    #[test]
    fn assemble_audit_memory_is_empty_on_an_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("market_signal.db");
        let recent = [one_recent_summary()];
        let assembled = assemble_research_packet(
            &ResearchStages::stub(),
            &BaselineMarketData::default(),
            None,
            crate::cadence::ReportCadence::from_elapsed(None),
            None,
            &recent,
            &[],
            &StubEmbedder,
            &db_path,
            &RunContext::noop(),
        );
        assert!(
            assembled.audit_memory.is_empty(),
            "an empty store recalls no audit memory"
        );
    }

    #[test]
    fn assemble_packet_degrades_to_empty_on_news_failure() {
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
        assert!(
            packet.news_clusters.is_empty(),
            "a failed filter yields no clusters"
        );
        assert!(
            packet.research.items.is_empty(),
            "and so no routed evidence"
        );
    }

    #[test]
    fn assemble_packet_keeps_news_when_only_the_router_fails() {
        let packet = assemble_with(ResearchStages {
            news: Box::new(StubNewsSource),
            filter: Box::new(StubHeadlineFilter),
            router: Box::new(FailingRouter),
            search: Box::new(StubSearchBackend),
        });
        assert!(
            !packet.news_clusters.is_empty(),
            "news survives a router failure"
        );
        assert!(
            packet.research.items.is_empty(),
            "but there is no routed evidence"
        );
    }

    /// Seed one report row whose markdown path never existed (the file leg reads
    /// it as already removed), so the retention tests exercise the DB legs alone.
    fn seed_retention_row(conn: &rusqlite::Connection, id: &str, created_at: &str) {
        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        let summary = ReportSummary {
            report_id: id.to_string(),
            report_type: "weekly_market".to_string(),
            created_at: created_at.to_string(),
            title: "Retention fixture headline".to_string(),
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
        storage::record_document_parse_run(&conn, "old-00", "2026-01-01T00:00:00Z", 4, 80_000)
            .unwrap();

        // Sabotage the last DB leg, so the earlier legs' rollback is observable.
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
        assert_eq!(
            count("SELECT COUNT(*) FROM document_parse_runs WHERE report_id = 'old-00'"),
            1,
            "the parse-run delete rolled back with the failed row delete"
        );

        conn.execute("DROP TRIGGER block_report_delete", [])
            .unwrap();
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
        assert_eq!(
            count("SELECT COUNT(*) FROM document_parse_runs WHERE report_id = 'old-00'"),
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

        // The skipped write is observable as the DB never being touched:
        // `record_parse_failures` is the only DB access in this stage, and opening would
        // create the file.
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("research-inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("broken.json"), "{ not json").unwrap();
        let db_path = dir.path().join("market_signal.db");

        let cancelled =
            RunContext::new("t", Arc::new(NoopReporter), Arc::new(AtomicBool::new(true)));
        let docs = process_research_inbox(&inbox, &db_path, &cancelled);
        assert!(docs.is_empty(), "a cancelled pass parses nothing");
        assert!(
            !db_path.exists(),
            "the failure write was skipped, not emptied"
        );

        // Uncancelled, the same pass records the failure, so the skip came from the cancel.
        let docs = process_research_inbox(&inbox, &db_path, &RunContext::noop());
        assert!(docs.is_empty());
        let conn = storage::open(&db_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM research_parse_failures", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn assemble_packet_skips_model_stages_when_cancelled() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        use crate::progress::{NoopReporter, RunContext};

        // StubNewsSource returns headlines regardless of the flag, so the filter guard is
        // exercised on the cancellation, not on an empty gather; the seeded store plus a
        // prior summary make the pre-research pull's query and store both non-empty, so
        // only the cancel guard stands between the run and the panicking embedder.
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
            title: "Recent context headline".into(),
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
        let AssembledResearch {
            packet,
            audit_memory,
        } = assemble_research_packet(
            &stages,
            &BaselineMarketData::default(),
            None,
            crate::cadence::ReportCadence::from_elapsed(None),
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
        assert!(
            audit_memory.is_empty(),
            "a cancelled run recalls no audit memory"
        );
    }

    // ---- live research-half smoke (news → filter → route → execute → packet) ----

    /// Live end-to-end smoke for the research half through `ResearchStages::live`.
    /// Everything in this path is fail-soft (a dead key degrades to an empty packet), so
    /// every assertion is anti-vacuous: it pins that each stage actually produced
    /// something. Spend per run: ~9 news calls plus up to 20 executor searches on
    /// Tavily, one OpenAI call, one Anthropic call — run it deliberately. A failed GDELT
    /// row is expected from a dev IP (escalating 429 lockout).
    #[test]
    #[ignore = "hits live Tavily + GDELT + FMP + OpenAI + Anthropic; set TAVILY_API_KEY + FMP_API_KEY + OPENAI_API_KEY + ANTHROPIC_API_KEY"]
    fn live_research_packet_smoke() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        use crate::config::AppConfig;
        use crate::progress::{ProgressEvent, RecordingReporter, RunContext};

        // A recording context so the adapters' request rows can be asserted on.
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new(
            "research-smoke",
            rec.clone(),
            Arc::new(AtomicBool::new(false)),
        );

        // The production constructor, so a wiring regression in the bundle fails this
        // smoke too.
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
        // pulls spend no embedding call here; the live embedding wire has its own smoke
        // (`embedding::embedding_live_smoke`).
        let dir = tempfile::tempdir().expect("temp dir");
        let packet = assemble_research_packet(
            &stages,
            &BaselineMarketData::default(),
            None,
            crate::cadence::ReportCadence::from_elapsed(None),
            None,
            &[],
            &[],
            &crate::embedding::StubEmbedder,
            &dir.path().join("market_signal.db"),
            &ctx,
        )
        .packet;

        // Each stage produced real output (fail-soft would have let empty through).
        assert!(
            !packet.news_clusters.is_empty(),
            "gather+filter yielded clusters"
        );
        assert!(
            !packet.research.items.is_empty(),
            "router yielded at least one topic"
        );
        assert!(
            packet.research.requests_made >= 1,
            "executor spent at least one search"
        );
        let total_sources: usize = packet
            .research
            .items
            .iter()
            .flat_map(|i| &i.findings)
            .map(|f| f.sources.len())
            .sum();
        assert!(
            total_sources >= 1,
            "at least one executor search returned sources"
        );

        // "memory" rows follow the currently-running step rather than the research
        // step; none fire here (the temp store is empty), but the set admits them so a
        // populated-store run stays green.
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
            packet
                .research
                .items
                .iter()
                .map(|i| i.findings.len())
                .sum::<usize>(),
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
    // The synthetic embedders only emit cosine 1.0 or 0.0, so they validate the
    // mechanism but not whether `LEARNING_DEDUP_THRESHOLD` and `MEMORY_TOP_K` are
    // well-placed against the intermediate cosines real prose lands at; these
    // `#[ignore]`d probes embed a hand-authored corpus through the live model. Spend is
    // ~27 cheap embedding calls, freely re-runnable.

    /// Restatements of one lesson under two wordings; each pair should embed close.
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

    /// Genuinely distinct lessons, one per topic family; every cross-pair should embed
    /// apart. The first four reuse the `.0` member of each `PARAPHRASE_PAIRS` entry.
    const DISTINCT_LESSONS: [&str; 6] = [
        "Breadth divergences preceded the spring pullback; weight them earlier in the thesis.",
        "Single-event volatility spikes faded within two reports; avoid pivoting the thesis on them.",
        "Credit spreads widened ahead of the equity correction; treat widening as an early risk-off tell.",
        "The yield curve's re-steepening coincided with the cycle turn; track the 10y-2y as a regime marker.",
        "Late-cycle leadership narrowed into mega-cap tech; rotation breadth matters more than the index level.",
        "Inflation surprises moved the front end more than the long end; lean on breakevens for the regime read.",
    ];

    /// Calibrates `LEARNING_DEDUP_THRESHOLD` against real geometry: the threshold must
    /// bracket the measured gap, `distinct_ceiling < LEARNING_DEDUP_THRESHOLD <=
    /// paraphrase_floor`. A red means the threshold drifted out of the gap or the corpus
    /// no longer separates; retuning the constant is a separate decision.
    #[test]
    #[ignore = "hits the live OpenAI embeddings API; set OPENAI_API_KEY (~14 calls)"]
    fn tuning_dedup_threshold_calibration() {
        let embedder = crate::embedding::OpenAiEmbedder::from_env().expect("OPENAI_API_KEY set");
        let cos = |a: &str, b: &str| {
            let va = embedder.embed(a).expect("live embedding call");
            let vb = embedder.embed(b).expect("live embedding call");
            crate::vector_memory::cosine_similarity(&va, &vb)
        };

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
        let paraphrase_ceiling = pair_cosines
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);

        let distinct_vecs: Vec<Vec<f32>> = DISTINCT_LESSONS
            .iter()
            .map(|t| embedder.embed(t).expect("live embedding call"))
            .collect();
        let mut distinct_ceiling = f64::NEG_INFINITY;
        for (i, va) in distinct_vecs.iter().enumerate() {
            for vb in &distinct_vecs[i + 1..] {
                distinct_ceiling =
                    distinct_ceiling.max(crate::vector_memory::cosine_similarity(va, vb));
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

        assert!(
            paraphrase_floor > distinct_ceiling,
            "restatements ({paraphrase_floor:.4}) did not separate from distinct lessons ({distinct_ceiling:.4})"
        );
        assert!(
            distinct_ceiling < LEARNING_DEDUP_THRESHOLD,
            "a distinct-lesson pair ({distinct_ceiling:.4}) reaches the dedup threshold {LEARNING_DEDUP_THRESHOLD} — it would be wrongly merged"
        );
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
    /// observational, so the hard assertion is the robust invariant (every relevant
    /// fragment out-ranks every off-topic one) and the printed ranking with the top-k
    /// cut marked lets a human judge the value.
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
            let cut = if i + 1 == MEMORY_TOP_K {
                "  <-- top-k cut"
            } else {
                ""
            };
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

        assert!(
            worst_relevant > best_irrelevant,
            "relevant fragments ({worst_relevant:.4}) did not cleanly out-rank off-topic ones ({best_irrelevant:.4})"
        );
    }
}
