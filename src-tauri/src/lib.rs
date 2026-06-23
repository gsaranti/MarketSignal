pub mod agent;
pub mod analyst_agent;
pub mod baseline_delta;
pub mod bls;
pub mod cadence;
pub mod config;
pub mod connection_test;
pub mod data_sources;
pub mod document_parser;
pub mod embedding;
pub mod fmp;
pub mod fmp_news;
pub mod fred;
pub mod gdelt;
pub mod headline_filter;
pub mod http_retry;
pub mod jobs;
pub mod model_agent;
pub mod news;
pub mod pipeline;
pub mod progress;
pub mod research;
pub mod research_executor;
pub mod research_packet;
pub mod research_router;
pub mod settings;
pub mod skills;
pub mod storage;
pub mod tavily;
#[cfg(test)]
mod test_http;
pub mod vector_memory;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use bls::BlsDataSource;
use config::{AppConfig, ValidationReport};
use data_sources::CompositeMarketDataSource;
use embedding::OpenAiEmbedder;
use fmp::FmpDataSource;
use fred::FredDataSource;
use jobs::{run_job, JobOutcome, JobStatus, RunGuard};
use model_agent::ModelMainAgent;
use pipeline::{AnalystStages, GeneratedReport, ReportPaths, ResearchStages};
use progress::{ProgressMessage, ProgressReporter, RunContext};

/// Tauri event name carrying every [`ProgressMessage`] for the live job tracker.
/// The frontend listens on this and accumulates the run's trace by `run_id`.
const JOB_PROGRESS_EVENT: &str = "job-progress";

/// Shared cancel flag for the in-flight run. Managed once by the app; the
/// `cancel_run` command flips it, and each run's `RunContext` reads the same bool —
/// the run resets it to `false` as it begins (`live_run_context`), so a stale cancel
/// from a dismissed prior run never carries over. A single flag suffices because the
/// `RunGuard` allows only one run at a time.
#[derive(Clone, Default)]
struct CancelFlag(Arc<AtomicBool>);

/// A [`ProgressReporter`] that forwards each message to the webview as a
/// `job-progress` Tauri event. Defined here, not in `progress`, so that module keeps
/// no `tauri` dependency and stays unit-testable.
struct TauriReporter {
    app: tauri::AppHandle,
}

impl ProgressReporter for TauriReporter {
    fn report(&self, message: &ProgressMessage) {
        // Best-effort: a closed/hidden window just means no one is listening.
        let _ = self.app.emit(JOB_PROGRESS_EVENT, message);
    }
}

/// Build the run context for one live run: a fresh run id, a Tauri-event reporter,
/// and the shared cancel flag. The flag is *not* reset here — `run_job` clears it
/// once it owns the concurrency slot (`RunContext::reset_cancel`), so a competing
/// attempt that is then skipped can't wipe an active run's cancellation.
fn live_run_context(app: &tauri::AppHandle, cancel: Arc<AtomicBool>) -> Arc<RunContext> {
    let reporter: Arc<dyn ProgressReporter> = Arc::new(TauriReporter { app: app.clone() });
    RunContext::new(uuid::Uuid::new_v4().to_string(), reporter, cancel)
}

/// Request cancellation of the in-flight run (the tracker's Cancel button). Sets the
/// shared cancel flag the run polls at its step / request boundaries; an HTTP call
/// already in flight is not interrupted, so the run stops at the next checkpoint. A
/// no-op when no run is active — the next run resets the flag as it begins.
#[tauri::command]
fn cancel_run(cancel: tauri::State<'_, CancelFlag>) {
    cancel.0.store(true, Ordering::Relaxed);
}

/// Report the current warning state for the Persistent Warning Area. Read-only:
/// it validates the config substrate (`docs/report-workflow.md §Step 1`)
/// and merges in the non-blocking `FailedJob` warning from job history
/// (`docs/scheduling.md §Error Handling`), but runs no job. The frontend calls
/// this on load and after a generate attempt to repopulate the warning area, so
/// a run that just failed surfaces here. The job-history merge is best-effort:
/// if the database can't be read, the authoritative config warnings still show.
#[tauri::command]
fn check_configuration(app: tauri::AppHandle) -> ValidationReport {
    // Open the app DB (best-effort) so config reads from the saved Settings store
    // with an env fallback per field. `open_app_db` creates the data dir and runs
    // the idempotent `init_schema`, tolerating a pre-existing slice-1 DB. If the
    // DB can't be opened, validate against env alone — the authoritative config
    // warnings still show; only the job-history warnings are skipped.
    let conn = open_app_db(&app).ok();
    let cfg = match &conn {
        Some(conn) => AppConfig::load(conn),
        None => AppConfig::from_env(),
    };
    let mut report = config::validate(&cfg);
    if let Some(conn) = &conn {
        if let Ok(Some(warning)) = jobs::failure_warning(conn) {
            report.categories.push(warning);
        }
    }
    report
}

/// Dismiss one Persistent Warning Area warning (`docs/interface.md §Persistent
/// Warning Area` — a dismissed warning stays gone until a fresh event in its
/// category). `id` is the `WarningCategory.dismiss_id` the frontend rendered and
/// echoes back, so the dismissal targets the *shown* warning rather than whatever
/// the backend would re-derive as current at click time — a stale click can then
/// only ever dismiss the row it was on, and a newer failure still surfaces fresh.
/// Only the non-blocking failed-job category is dismissible; a blocking
/// configuration gap is gate state, so a dismiss of one is a no-op (handled in
/// `jobs::dismiss_warning_category`). The frontend re-runs `check_configuration`
/// afterward to repopulate the band.
#[tauri::command]
fn dismiss_warning(
    app: tauri::AppHandle,
    kind: config::WarningKind,
    id: String,
) -> Result<(), String> {
    let conn = open_app_db(&app)?;
    jobs::dismiss_warning_category(&conn, kind, &id).map_err(|e| e.to_string())
}

/// Environment override for the on-disk data directory. A non-empty value wins
/// over the OS app-data location *and* the debug-build split below — the explicit
/// isolation hook for tests, automation, and the live-run harness (otherwise the
/// store is keyed only by the bundle identifier, so every build of the same id
/// shares one directory and isolating it means physically moving the real one).
const DATA_DIR_ENV: &str = "MARKET_SIGNAL_DATA_DIR";

/// Resolve the base data directory from three layered sources, so a `tauri dev`
/// session never shares the production store:
/// 1. an explicit [`DATA_DIR_ENV`] override (non-empty, trimmed) wins outright;
/// 2. otherwise the OS app-data dir, nested under a `dev/` subdirectory for
///    **debug** builds (`tauri dev`) so development data is sandboxed;
/// 3. **release** builds (`tauri build`) use the OS app-data dir as-is — the real
///    production store, untouched.
///
/// Pure (no Tauri `AppHandle`) so the layering is unit-tested directly.
fn resolve_data_dir(app_data_dir: PathBuf, env_override: Option<String>, debug: bool) -> PathBuf {
    if let Some(p) = env_override {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if debug {
        app_data_dir.join("dev")
    } else {
        app_data_dir
    }
}

/// The on-disk layout for a run — the SQLite database, the reports directory,
/// and the research inbox/archive, all under the app data directory
/// (`ReportPaths::under` owns the names). One source for the path layout, shared
/// by the manual command and the research-folder helpers so they can never drift
/// apart. The base dir comes from [`resolve_data_dir`], so a `tauri dev` (debug)
/// session is sandboxed under a `dev/` subdir away from the production store, and
/// `MARKET_SIGNAL_DATA_DIR` can override either.
fn report_paths(app: &tauri::AppHandle) -> Result<ReportPaths, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolving app data directory: {e}"))?;
    let data_dir = resolve_data_dir(
        app_data_dir,
        std::env::var(DATA_DIR_ENV).ok(),
        cfg!(debug_assertions),
    );
    Ok(ReportPaths::under(&data_dir))
}

/// The research-inbox folder (`docs/research-documents.md`) — the same layout
/// the pipeline's Step-6 stage reads via `ReportPaths`.
fn research_inbox_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(report_paths(app)?.inbox_dir)
}

/// The research-archive folder (`docs/research-documents.md`). Successfully
/// processed inbox documents are moved here by the pipeline's persist step; the
/// user may delete from it but cannot manually archive into it.
fn research_archive_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(report_paths(app)?.archive_dir)
}

/// Manually generate a Market Signal Report end to end. The execution gate runs
/// first: the configuration is validated and a blocked run is refused before any
/// work begins. Once the gate passes, this resolves the app data directory, runs
/// the agent through the pipeline, and returns the generated report for the
/// frontend to render. The structured block detail lives in the
/// `check_configuration` report the warning area already shows; the error here
/// is the concise fallback summary.
///
/// The agent build and pipeline run go through `spawn_blocking`: the agent uses
/// `reqwest::blocking`, which starts its own runtime and would panic if it ran
/// on the async runtime thread this command is dispatched on. The `MainAgent`
/// trait and pipeline stay synchronous; only this seam is async.
///
/// The run is wrapped by `jobs::run_job`, which records the lifecycle outcome and
/// enforces the single-workflow-at-a-time guard (`docs/scheduling.md §Concurrent
/// Job Protection`). The shared `RunGuard` is cloned out of managed state before
/// the await so it is never held across it: a clone shares the same in-flight
/// flag. A Skipped or Failed outcome maps to `Err` here while still being
/// recorded in job history (and a failure surfaces in the warning area on the
/// next `check_configuration`).
#[tauri::command]
async fn generate_report_manual(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
    cancel: tauri::State<'_, CancelFlag>,
) -> Result<GeneratedReport, String> {
    // Execution gate: refuse a blocked run before doing any work. The config is
    // read from the saved Settings store (env fallback) on a connection opened and
    // dropped here, before the await below — a `rusqlite::Connection` is not `Send`
    // and must never cross an await point.
    let cfg = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn)
    };
    let report = config::validate(&cfg);
    if report.is_blocked {
        return Err(config::blocked_summary(&report));
    }
    let main_config = cfg.main_agent_config().map_err(|e| e.to_string())?;
    // The three analyst adapter configs (Steps 12–15): each posture's user-selected
    // model + provider key. The gate above already requires all three.
    let bull_config = cfg
        .analyst_config(agent::Posture::Bull)
        .map_err(|e| e.to_string())?;
    let bear_config = cfg
        .analyst_config(agent::Posture::Bear)
        .map_err(|e| e.to_string())?;
    let balanced_config = cfg
        .analyst_config(agent::Posture::Balanced)
        .map_err(|e| e.to_string())?;
    let fmp_key = cfg.fmp_key().map_err(|e| e.to_string())?;
    let fred_key = cfg.fred_key().map_err(|e| e.to_string())?;
    // Research-half credentials (Steps 7–11): Tavily (news ingestion + the Step-9 search
    // backend), OpenAI (the GPT-5-mini headline filter), Anthropic (the Sonnet research
    // router). The gate above already requires all three; the FMP key above is reused
    // for the supplementary FMP Articles news feed.
    let tavily_key = cfg.tavily_key().map_err(|e| e.to_string())?;
    let openai_key = cfg.openai_key().map_err(|e| e.to_string())?;
    let anthropic_key = cfg.anthropic_key().map_err(|e| e.to_string())?;

    let paths = report_paths(&app)?;

    let guard = guard.inner().clone();
    // One run context for the whole run: a fresh id, the Tauri-event reporter, and the
    // shared cancel flag (reset here for this run). Cloned into each adapter and the
    // agent so the baseline scan streams per-series rows and the agent streams its
    // report text; borrowed by `run_job` for the step events + cancel checkpoints.
    let ctx = live_run_context(&app, cancel.inner().0.clone());

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let agent = ModelMainAgent::new(main_config)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        // The baseline scan is FMP (indices / VIX / gold / sectors) + FRED (yields,
        // dollar index, oil, gas, macro levels) + BLS (labor levels) merged behind one
        // trait (`docs/report-workflow.md §Step 3`). BLS is keyless (not in the
        // execution gate); it nests as the outer secondary so its labor_levels group
        // folds into the FMP+FRED baseline.
        let fmp = FmpDataSource::new(fmp_key.clone())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let fred = FredDataSource::new(fred_key)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let bls = BlsDataSource::new()
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let data = CompositeMarketDataSource::new(CompositeMarketDataSource::new(fmp, fred), bls);
        let research =
            ResearchStages::live(tavily_key, fmp_key, openai_key.clone(), anthropic_key, &ctx)
                .map_err(|e| e.to_string())?;
        // Steps 12–15: the three analyst adapters, one per posture, sharing the run's
        // context like the other live stages so each review streams a request row.
        let analysts = AnalystStages::live(bull_config, bear_config, balanced_config, &ctx)
            .map_err(|e| e.to_string())?;
        // The Step-17 memory write's embedder: the fixed internal OpenAI embedding
        // stage (`text-embedding-3-large`), reusing the same key as the filter.
        let embedder = OpenAiEmbedder::new(openai_key)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        run_job(
            &agent, &data, &research, &analysts, &embedder, &paths, &guard, &ctx,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("report generation task failed: {e}"))??;

    match outcome {
        JobOutcome::Successful(report) => Ok(*report),
        JobOutcome::Failed(msg) => Err(msg),
        JobOutcome::Skipped(reason) => Err(reason),
        // The tracker shows the cancelled terminal state from the `run-finished` event;
        // the command still resolves to `Err` so the frontend's generate() settles
        // (its catch suppresses the failure banner when the user asked to cancel).
        JobOutcome::Cancelled(reason) => Err(reason),
    }
}

/// List the most recent persisted reports for the Recent Reports sidebar
/// (`docs/interface.md`, `docs/storage.md` — newest first, capped at the
/// 30-report retention window). A fresh install with no reports yet lists as
/// empty rather than erroring; the frontend renders the empty state.
#[tauri::command]
fn list_reports(app: tauri::AppHandle) -> Result<Vec<agent::ReportSummary>, String> {
    let paths = report_paths(&app)?;
    pipeline::list_reports(&paths).map_err(|e| e.to_string())
}

/// Load one persisted report by id for the Latest Report View: its summary plus
/// its canonical Markdown read back from disk (`docs/report-workflow.md
/// §Step 18`). An unknown id, or a Markdown file removed out-of-band, surfaces as
/// an error the view renders.
#[tauri::command]
fn load_report(app: tauri::AppHandle, report_id: String) -> Result<GeneratedReport, String> {
    let paths = report_paths(&app)?;
    pipeline::load_report(&paths, &report_id).map_err(|e| e.to_string())
}

/// Export one report's canonical Markdown to a user-chosen location
/// (`docs/export.md`). The report is resolved first — a bad id or a Markdown file
/// removed out-of-band fails here, before any dialog pops — which also yields the
/// `created_at` used to suggest the spec's export filename
/// (`YYYY-MM-DD-market-signal-report.md`, no internal id suffix). The
/// native Save dialog runs on a blocking thread: `blocking_save_file` parks the
/// calling thread until the user responds and must not run on the async runtime
/// thread, so it goes through `spawn_blocking` (the same seam
/// `generate_report_manual` uses). A cancelled dialog returns `Ok(false)`; a saved
/// file returns `Ok(true)` after the stored Markdown is written to the chosen path.
/// Exporting reads stored artifacts only and never re-runs the workflow
/// (`docs/export.md §Export Behavior`).
#[tauri::command]
async fn export_report_markdown(app: tauri::AppHandle, report_id: String) -> Result<bool, String> {
    let paths = report_paths(&app)?;

    // Resolve the report before showing a dialog: validates the id and that the
    // Markdown is readable, and supplies created_at for the suggested name.
    let report = pipeline::load_report(&paths, &report_id).map_err(|e| e.to_string())?;
    let suggested = pipeline::export_basename(&report.summary.created_at, "md", &chrono::Local)
        .map_err(|e| e.to_string())?;

    let chosen = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app.dialog()
                .file()
                .set_file_name(&suggested)
                .add_filter("Markdown", &["md"])
                .blocking_save_file()
        })
        .await
        .map_err(|e| format!("save dialog task failed: {e}"))?
    };

    // User dismissed the dialog without choosing a path.
    let Some(chosen) = chosen else {
        return Ok(false);
    };

    let dest = chosen.into_path().map_err(|e| e.to_string())?;
    pipeline::export_markdown_to(&paths, &report_id, &dest).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Resolve the SQLite path and ensure the app data directory exists, so a
/// command that touches the database works even before the first report has been
/// generated (the pipeline creates the directory as a side effect, but the
/// status/settings commands can run first).
fn open_app_db(app: &tauri::AppHandle) -> Result<rusqlite::Connection, String> {
    let paths = report_paths(app)?;
    if let Some(parent) = paths.db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating app data directory: {e}"))?;
    }
    let conn = storage::open(&paths.db_path).map_err(|e| e.to_string())?;
    storage::init_schema(&conn).map_err(|e| e.to_string())?;
    Ok(conn)
}

/// Current job status for the UI's status panel (`docs/scheduling.md §Job Status
/// Visibility`): last successful run, last failure, last skipped event, and
/// whether a run is in flight.
#[tauri::command]
fn job_status(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
) -> Result<JobStatus, String> {
    let conn = open_app_db(&app)?;
    jobs::job_status(&conn, &guard).map_err(|e| e.to_string())
}

/// The current Settings state (`docs/configuration.md`, `docs/interface.md
/// §Settings`): the four agent model selections, a configured flag per credential
/// (never the secret itself), and the model dropdown's options. Reads from the
/// saved store with an env fallback per field.
#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<settings::SettingsView, String> {
    let conn = open_app_db(&app)?;
    Ok(settings::load_view(&conn))
}

/// Persist a Settings submission (`docs/configuration.md`). Model slugs are
/// validated; each credential is written only when a new value is supplied, so an
/// untouched field keeps its stored secret. The frontend re-runs
/// `check_configuration` afterward, so completing the config clears the
/// Persistent Warning Area's blocking categories.
#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    models: settings::AgentModels,
    credentials: settings::CredentialUpdate,
) -> Result<(), String> {
    let conn = open_app_db(&app)?;
    settings::save(&conn, &models, &credentials).map_err(|e| e.to_string())
}

/// Validate one configured provider credential with a single live authenticated
/// request (Settings "Test connection"). Reads the *saved* credential (env
/// fallback per field, like the gate); an unset credential returns a
/// not-configured result without any network call. The blocking HTTP request
/// goes through `spawn_blocking` — `reqwest::blocking` would panic on the async
/// runtime thread, the same seam `generate_report_manual` uses. The request
/// validates the key only: it never spends model tokens, and it does not change
/// the execution gate, which checks credential *presence*, not validity.
#[tauri::command]
async fn test_connection(
    app: tauri::AppHandle,
    provider: String,
) -> Result<connection_test::ConnectionTestResult, String> {
    use connection_test::CredentialProvider;
    let target = CredentialProvider::from_label(&provider).map_err(|e| e.to_string())?;

    // Read the saved credential on a short-lived connection dropped before the
    // await — a `rusqlite::Connection` is not `Send` and must never cross an
    // await point.
    let key = {
        let conn = open_app_db(&app)?;
        let cfg = AppConfig::load(&conn);
        let stored = match target {
            CredentialProvider::OpenAi => &cfg.openai_api_key,
            CredentialProvider::Anthropic => &cfg.anthropic_api_key,
            CredentialProvider::Fmp => &cfg.fmp_api_key,
            CredentialProvider::Fred => &cfg.fred_api_key,
            CredentialProvider::Tavily => &cfg.tavily_api_key,
        };
        stored
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    let Some(key) = key else {
        return Ok(connection_test::ConnectionTestResult::not_configured());
    };

    tauri::async_runtime::spawn_blocking(move || connection_test::run_test(target, &key))
        .await
        .map_err(|e| format!("connection test task failed: {e}"))
}

/// List the user-supplied documents currently in the research inbox
/// (`docs/research-documents.md`). A fresh install with no inbox folder yet lists
/// as empty rather than erroring; the frontend renders the empty state. The last
/// job pass's parse failures are joined on best-effort (`§Parse Failures` — the
/// file shows in an error state so the user can fix or delete it); an unreadable
/// DB costs the error states, never the listing.
#[tauri::command]
fn list_research_inbox(app: tauri::AppHandle) -> Result<Vec<research::ResearchDocument>, String> {
    let inbox = research_inbox_dir(&app)?;
    let mut docs = research::list_folder(&inbox).map_err(|e| e.to_string())?;
    if let Ok(conn) = open_app_db(&app) {
        if let Ok(failures) = storage::list_parse_failures(&conn) {
            research::annotate_parse_failures(&mut docs, &failures);
        }
    }
    Ok(docs)
}

/// Delete one document from the research inbox by file name
/// (`docs/research-documents.md` §User Permissions — the user may delete from the
/// inbox). The name is validated as a bare file name in `research::` so it cannot
/// escape the inbox directory.
#[tauri::command]
fn delete_research_document(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let inbox = research_inbox_dir(&app)?;
    research::delete_folder_document(&inbox, &name).map_err(|e| e.to_string())
}

/// Open the research-inbox folder in the OS file manager so the user can drop
/// documents into it (the spec's canonical interaction — the user manually places
/// files; `docs/research-documents.md` §Research Inbox). The folder is created on
/// demand so a first-time reveal lands somewhere real.
#[tauri::command]
fn reveal_research_inbox(app: tauri::AppHandle) -> Result<(), String> {
    let inbox = research_inbox_dir(&app)?;
    std::fs::create_dir_all(&inbox)
        .map_err(|e| format!("creating research inbox directory: {e}"))?;
    app.opener()
        .open_path(inbox.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| format!("opening research inbox: {e}"))
}

/// List the documents currently in the research archive
/// (`docs/research-documents.md`). Successfully-processed inbox documents are
/// moved here; a fresh install with no archive folder yet lists as empty rather
/// than erroring, so the frontend renders the empty state.
#[tauri::command]
fn list_research_archive(app: tauri::AppHandle) -> Result<Vec<research::ResearchDocument>, String> {
    let archive = research_archive_dir(&app)?;
    research::list_folder(&archive).map_err(|e| e.to_string())
}

/// Delete one document from the research archive by file name
/// (`docs/research-documents.md` §User Permissions — the user may delete from
/// either folder). The name is validated as a bare file name in `research::` so it
/// cannot escape the archive directory.
#[tauri::command]
fn delete_research_archive_document(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let archive = research_archive_dir(&app)?;
    research::delete_folder_document(&archive, &name).map_err(|e| e.to_string())
}

/// Open the research-archive folder in the OS file manager so the user can inspect
/// what the pipeline has filed. The archive is read-only by spec — the user may
/// view or delete here but not add (archiving is automatic;
/// `docs/research-documents.md` §User Permissions). The folder is created on demand
/// so a first-time reveal lands somewhere real.
#[tauri::command]
fn reveal_research_archive(app: tauri::AppHandle) -> Result<(), String> {
    let archive = research_archive_dir(&app)?;
    std::fs::create_dir_all(&archive)
        .map_err(|e| format!("creating research archive directory: {e}"))?;
    app.opener()
        .open_path(archive.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| format!("opening research archive: {e}"))
}

/// Aggregate truncation telemetry for the Settings diagnostics section
/// (`docs/agents.md §Data Extraction`): how often the deterministic Step-6 parser
/// had to head-truncate an oversized inbox document, accumulated across reports.
/// Fail-soft like the rest of the diagnostics surface — an unopenable DB degrades
/// to an empty `TruncationStats` (which reads as "no truncations recorded") rather
/// than failing the Settings load. The empty aggregate is itself the signal that
/// overflow is not yet common, so it must never be a hard error here.
#[tauri::command]
fn truncation_stats(app: tauri::AppHandle) -> storage::TruncationStats {
    let Ok(conn) = open_app_db(&app) else {
        return storage::TruncationStats::default();
    };
    storage::truncation_stats(&conn).unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(RunGuard::default())
        .manage(CancelFlag::default())
        .setup(|app| {
            // One-time legacy-naming migration (docs/storage.md §Legacy Naming
            // Migration): on first launch after the manual-only pivot, rewrite
            // pre-pivot `report_type` slugs and `…-weekly-report` filenames in
            // place. Best-effort — a failure logs and launch proceeds (the app
            // still works against the old names), matching the codebase's
            // fail-soft persistence posture. Idempotent, so a later launch with
            // nothing to migrate is a cheap no-op.
            match open_app_db(app.handle()) {
                Ok(conn) => {
                    if let Err(e) = storage::migrate_legacy_naming(&conn) {
                        eprintln!("legacy-naming migration: degraded ({e})");
                    }
                    // Sibling slug migration: rewrite the pre-pivot
                    // `job_runs.job_type` value `weekly_market` → `market_signal`.
                    // Independent of the naming migration above (different column),
                    // same best-effort posture — a failure logs and launch proceeds.
                    if let Err(e) = storage::migrate_legacy_job_type(&conn) {
                        eprintln!("legacy-job-type migration: degraded ({e})");
                    }
                }
                Err(e) => eprintln!("legacy-naming migration: could not open database ({e})"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            generate_report_manual,
            cancel_run,
            list_reports,
            load_report,
            export_report_markdown,
            check_configuration,
            dismiss_warning,
            job_status,
            get_settings,
            save_settings,
            test_connection,
            list_research_inbox,
            delete_research_document,
            reveal_research_inbox,
            list_research_archive,
            delete_research_archive_document,
            reveal_research_archive,
            truncation_stats
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_build_uses_app_data_dir_as_is() {
        let base = PathBuf::from("/data/app");
        assert_eq!(resolve_data_dir(base.clone(), None, false), base);
    }

    #[test]
    fn debug_build_nests_under_dev_subdir() {
        let base = PathBuf::from("/data/app");
        assert_eq!(
            resolve_data_dir(base, None, true),
            PathBuf::from("/data/app/dev"),
        );
    }

    #[test]
    fn env_override_wins_over_both_debug_and_release() {
        let base = PathBuf::from("/data/app");
        let over = Some("/tmp/scratch".to_string());
        assert_eq!(
            resolve_data_dir(base.clone(), over.clone(), true),
            PathBuf::from("/tmp/scratch"),
        );
        assert_eq!(
            resolve_data_dir(base, over, false),
            PathBuf::from("/tmp/scratch"),
        );
    }

    #[test]
    fn blank_env_override_falls_through_to_build_split() {
        let base = PathBuf::from("/data/app");
        // empty string -> debug split applies
        assert_eq!(
            resolve_data_dir(base.clone(), Some(String::new()), true),
            PathBuf::from("/data/app/dev"),
        );
        // whitespace-only -> release passes through unchanged
        assert_eq!(
            resolve_data_dir(base.clone(), Some("   ".to_string()), false),
            base,
        );
    }

    #[test]
    fn env_override_is_trimmed() {
        let base = PathBuf::from("/data/app");
        assert_eq!(
            resolve_data_dir(base, Some("  /tmp/scratch \n".to_string()), false),
            PathBuf::from("/tmp/scratch"),
        );
    }
}
