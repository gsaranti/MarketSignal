pub mod agent;
pub mod analyst_agent;
pub mod baseline_delta;
pub mod baseline_history;
pub mod bls;
pub mod cadence;
pub mod cboe;
pub mod config;
pub mod connection_test;
pub mod cot;
pub mod data_sources;
pub mod document_parser;
pub mod econ_calendar;
pub mod embedding;
pub mod finra;
pub mod fmp;
pub mod fmp_news;
pub mod fred;
pub mod gdelt;
pub mod group_history;
pub mod headline_filter;
pub mod http_retry;
pub mod jobs;
pub mod local_model;
pub mod loopback_https;
pub mod market_clock;
pub mod model_agent;
pub mod news;
pub mod pipeline;
pub mod portability;
pub mod portfolio;
pub mod primary_market;
pub mod progress;
pub mod research;
pub mod research_executor;
pub mod research_packet;
pub mod research_router;
pub mod schwab;
pub mod schwab_live;
pub mod schwab_oauth;
pub mod schwab_secrets;
pub mod sec;
pub mod settings;
pub mod skills;
pub mod storage;
pub mod tavily;
pub mod web_research;
#[cfg(test)]
mod test_http;
pub mod thought_log;
pub mod vector_memory;

// Dev-only demo mode; the `demo-run` feature is not in `default`, so `tauri build`
// never includes it.
#[cfg(feature = "demo-run")]
mod demo;

/// `MARKET_SIGNAL_DEMO` is truthy (`1` / `true`, case-insensitive); a bare, empty
/// or `0` value reads as off, so the var can be exported `=0` without silently
/// enabling stub reports.
#[cfg(feature = "demo-run")]
fn demo_run_enabled() -> bool {
    std::env::var("MARKET_SIGNAL_DEMO")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use bls::BlsDataSource;
use cot::CotDataSource;
use config::{AppConfig, ValidationReport};
use data_sources::CompositeMarketDataSource;
use embedding::OpenAiEmbedder;
use fmp::FmpDataSource;
use fred::FredDataSource;
use jobs::{run_job, JobOutcome, JobStatus, RunGuard, RunKind};
use model_agent::ModelMainAgent;
use pipeline::{AnalystStages, GeneratedReport, ReportPaths, ResearchStages};
use progress::{ProgressMessage, ProgressReporter, RunContext};
use schwab_secrets::TokenStore;

/// Tauri event carrying every [`ProgressMessage`] to the job tracker, which
/// accumulates the run's trace by `run_id`.
const JOB_PROGRESS_EVENT: &str = "job-progress";

/// Shared cancel flag for the in-flight run: `cancel_run` flips it and each run's
/// `RunContext` reads it. A single flag suffices because the `RunGuard` allows one
/// run at a time, and the job resets it as it claims the slot.
#[derive(Clone, Default)]
struct CancelFlag(Arc<AtomicBool>);

/// Forwards each message to the webview as a `job-progress` event. Lives here
/// rather than in `progress` so that module keeps no `tauri` dependency.
struct TauriReporter {
    app: tauri::AppHandle,
}

impl ProgressReporter for TauriReporter {
    fn report(&self, message: &ProgressMessage) {
        // Best-effort: a closed/hidden window just means no one is listening.
        let _ = self.app.emit(JOB_PROGRESS_EVENT, message);
    }
}

/// The cancel flag is *not* reset here — the job clears it once it owns the run
/// slot (`RunContext::reset_cancel`), so a competing attempt that is then skipped
/// can't wipe an active run's cancellation. When the thought-log gate is on the
/// reporter is decorated with the capture sink (`docs/run-tracking.md §Thought-log
/// capture`); a failed app-data lookup skips capture, never the run.
fn live_run_context(app: &tauri::AppHandle, cancel: Arc<AtomicBool>) -> Arc<RunContext> {
    let run_id = uuid::Uuid::new_v4().to_string();
    let mut reporter: Arc<dyn ProgressReporter> = Arc::new(TauriReporter { app: app.clone() });
    let enabled = thought_log_enabled(
        std::env::var(THOUGHT_LOG_ENV).ok().as_deref(),
        cfg!(debug_assertions),
    );
    if enabled {
        match app.path().app_data_dir() {
            Ok(app_data_dir) => {
                let base = resolve_data_dir(
                    app_data_dir,
                    std::env::var(DATA_DIR_ENV).ok(),
                    cfg!(debug_assertions),
                )
                .join("thought-logs");
                reporter =
                    Arc::new(thought_log::ThoughtLogSink::attach(reporter, &base, &run_id));
            }
            Err(e) => eprintln!("[thought-log] capture skipped: app data dir unresolved: {e}"),
        }
    }
    RunContext::new(run_id, reporter, cancel)
}

/// Forces the thought-log capture on (`1` / `true`) or off (`0` / `false`)
/// regardless of build profile; unset falls to the build default.
const THOUGHT_LOG_ENV: &str = "MARKET_SIGNAL_THOUGHT_LOG";

/// An explicit override wins, otherwise debug builds capture and release builds
/// do not; an unrecognized value falls to the build default rather than guessing
/// a direction.
fn thought_log_enabled(env_val: Option<&str>, debug: bool) -> bool {
    match env_val.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("0") | Some("false") => false,
        Some("1") | Some("true") => true,
        _ => debug,
    }
}

/// The run polls the flag at its step / request boundaries, so an HTTP call
/// already in flight is not interrupted. A no-op when no run is active.
#[tauri::command]
fn cancel_run(cancel: tauri::State<'_, CancelFlag>) {
    cancel.0.store(true, Ordering::Relaxed);
}

/// The Persistent Warning Area's state: the config validation
/// (`docs/report-workflow.md §Step 1`) plus the non-blocking failed-job warning
/// (`docs/scheduling.md §Error Handling`). Read-only. The job-history merge is
/// best-effort: an unreadable database still shows the config warnings.
#[tauri::command]
fn check_configuration(app: tauri::AppHandle) -> ValidationReport {
    // Demo mode reports a clean gate so "Generate now" is enabled with no keys.
    #[cfg(feature = "demo-run")]
    if demo_run_enabled() {
        return ValidationReport {
            categories: Vec::new(),
            is_blocked: false,
        };
    }

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

/// Dismisses one Persistent Warning Area warning (`docs/interface.md §Persistent
/// Warning Area`). `id` is the `dismiss_id` the frontend rendered, so a stale
/// click dismisses only the row it was on and a newer failure still surfaces.
/// Only the failed-job category is dismissible; a blocking configuration gap is
/// gate state, so dismissing one is a no-op.
#[tauri::command]
fn dismiss_warning(
    app: tauri::AppHandle,
    kind: config::WarningKind,
    id: String,
) -> Result<(), String> {
    let conn = open_app_db(&app)?;
    jobs::dismiss_warning_category(&conn, kind, &id).map_err(|e| e.to_string())
}

/// Data-directory override: a non-empty value wins over the OS app-data location
/// and the debug-build split — the isolation hook for tests, automation and the
/// live-run harness, since every build of one bundle id otherwise shares a store.
const DATA_DIR_ENV: &str = "MARKET_SIGNAL_DATA_DIR";

/// An explicit [`DATA_DIR_ENV`] override wins; otherwise debug builds nest under
/// `dev/` so a `tauri dev` session never shares the production store. Pure so
/// the layering is unit-tested directly.
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

/// The on-disk layout under the data directory from [`resolve_data_dir`]; the one
/// source for every path, so the commands and the research-folder helpers never
/// drift apart.
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

/// The research-inbox folder (`docs/research-documents.md`).
fn research_inbox_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(report_paths(app)?.inbox_dir)
}

/// The research-archive folder (`docs/research-documents.md`).
fn research_archive_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(report_paths(app)?.archive_dir)
}

/// Generates a Market Signal Report end to end and returns it. The execution gate
/// runs first: a blocked configuration is refused before any work begins, with
/// `check_configuration` carrying the structured detail.
///
/// The agent uses `reqwest::blocking`, which would panic on the async runtime
/// thread, so the build and run go through `spawn_blocking`. `jobs::run_job`
/// records the outcome and enforces the one-workflow-at-a-time guard
/// (`docs/scheduling.md §Concurrent Job Protection`); the `RunGuard` is cloned
/// out of managed state before the await so it is never held across it. A
/// Skipped or Failed outcome maps to `Err` while still being recorded in job
/// history.
#[tauri::command]
async fn generate_report_manual(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
    cancel: tauri::State<'_, CancelFlag>,
) -> Result<GeneratedReport, String> {
    // Dev-only demo path: the real pipeline against paced stubs — no keys, no
    // network, no gate.
    #[cfg(feature = "demo-run")]
    if demo_run_enabled() {
        return generate_report_demo(app, guard, cancel).await;
    }

    // A `rusqlite::Connection` is not `Send`, so the config read drops it before
    // the await.
    let cfg = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn)
    };
    let report = config::validate(&cfg);
    if report.is_blocked {
        return Err(config::blocked_summary(&report));
    }
    let main_config = cfg.main_agent_config().map_err(|e| e.to_string())?;
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
    let tavily_key = cfg.tavily_key().map_err(|e| e.to_string())?;
    let openai_key = cfg.openai_key().map_err(|e| e.to_string())?;
    let anthropic_key = cfg.anthropic_key().map_err(|e| e.to_string())?;

    let paths = report_paths(&app)?;

    let guard = guard.inner().clone();
    let ctx = live_run_context(&app, cancel.inner().0.clone());

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let agent = ModelMainAgent::new(main_config)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        // BLS and CFTC are keyless and off the execution gate; they nest as outer
        // secondaries so their groups fold into the FMP+FRED baseline
        // (`docs/report-workflow.md §Step 3`).
        let fmp = FmpDataSource::new(fmp_key.clone())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        // FMP's paid economic calendar supplies each release's consensus and
        // surprise; a free key leaves names and dates.
        let consensus = FmpDataSource::new(fmp_key.clone())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let fred = FredDataSource::new(fred_key)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone())
            .with_consensus(consensus);
        let bls = BlsDataSource::new()
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let cot = CotDataSource::new()
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let data = CompositeMarketDataSource::new(
            CompositeMarketDataSource::new(CompositeMarketDataSource::new(fmp, fred), bls),
            cot,
        );
        let research =
            ResearchStages::live(tavily_key, fmp_key, openai_key.clone(), anthropic_key, &ctx)
                .map_err(|e| e.to_string())?;
        let analysts = AnalystStages::live(bull_config, bear_config, balanced_config, &ctx)
            .map_err(|e| e.to_string())?;
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
        // Cancelled still resolves to `Err` so the frontend's generate() settles;
        // its catch suppresses the failure banner when the user asked to cancel.
        JobOutcome::Cancelled(reason) => Err(reason),
    }
}

/// Dev-only demo run: the real run plumbing with the paced demo stages from
/// `demo.rs` in place of the live adapters, and no credential gate.
#[cfg(feature = "demo-run")]
async fn generate_report_demo(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
    cancel: tauri::State<'_, CancelFlag>,
) -> Result<GeneratedReport, String> {
    let paths = report_paths(&app)?;
    let guard = guard.inner().clone();
    let ctx = live_run_context(&app, cancel.inner().0.clone());

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let agent = demo::main_agent(ctx.clone());
        let data = demo::market_data(ctx.clone());
        let research = demo::research_stages(ctx.clone());
        let analysts = demo::analyst_stages(ctx.clone());
        run_job(
            &agent,
            &data,
            &research,
            &analysts,
            &embedding::StubEmbedder,
            &paths,
            &guard,
            &ctx,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("report generation task failed: {e}"))??;

    match outcome {
        JobOutcome::Successful(report) => Ok(*report),
        JobOutcome::Failed(msg) => Err(msg),
        JobOutcome::Skipped(reason) => Err(reason),
        JobOutcome::Cancelled(reason) => Err(reason),
    }
}

/// Which holdings source a local job should use this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldingsChoice {
    /// The offline fixture behind `MARKET_SIGNAL_SCHWAB_FIXTURE`, so the pipeline
    /// runs with no Schwab connection.
    Fixture,
    /// A connected account with an open refresh window.
    Live,
    /// Not connected, lapsed, or unreadable: the job blocks with the gate's prompt
    /// rather than run degraded.
    NotConnected,
}

/// Pure so the gate's decision table is unit-testable without a Keychain or a
/// network.
fn choose_holdings_source(fixture_escape: bool, connected: bool) -> HoldingsChoice {
    if fixture_escape {
        HoldingsChoice::Fixture
    } else if connected {
        HoldingsChoice::Live
    } else {
        HoldingsChoice::NotConnected
    }
}

/// Whether `MARKET_SIGNAL_SCHWAB_FIXTURE` is truthy, so the local jobs run against
/// the offline fixture with no Schwab connection.
fn schwab_fixture_escape() -> bool {
    std::env::var("MARKET_SIGNAL_SCHWAB_FIXTURE")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// The single source-selection seam every holdings fetch goes through. Only the
/// non-fixture path touches the Keychain rail / OAuth. Blocking (Keychain reads);
/// call inside `spawn_blocking`.
fn build_holdings_source(cfg: &AppConfig) -> Result<Box<dyn schwab::HoldingsSource>, String> {
    let store: Arc<dyn schwab_secrets::TokenStore> =
        Arc::new(schwab_secrets::KeyringTokenStore::new());
    let fixture_escape = schwab_fixture_escape();
    let oauth = if fixture_escape {
        None
    } else {
        let client_id = cfg.schwab_client_id.clone().unwrap_or_default();
        Some(Arc::new(
            schwab_oauth::OauthClient::new(client_id, Arc::clone(&store))
                .map_err(|e| e.to_string())?,
        ))
    };
    let presence = if fixture_escape {
        // The fixture path never reads the rail; the escape hatch wins below.
        schwab_oauth::SchwabPresence::NotConnected
    } else {
        schwab_oauth::schwab_presence(store.as_ref(), chrono::Utc::now())
    };
    let connected = presence == schwab_oauth::SchwabPresence::Connected;
    match choose_holdings_source(fixture_escape, connected) {
        HoldingsChoice::Fixture => Ok(Box::new(schwab::FixtureHoldingsSource::new())),
        HoldingsChoice::NotConnected => {
            // The shared gate's item (the reconnect or unreadable-Keychain prompt)
            // is the error, so the block reads the same as the warning band's.
            let report = schwab_oauth::schwab_gate(presence);
            let message = report
                .categories
                .into_iter()
                .flat_map(|c| c.items)
                .collect::<Vec<_>>()
                .join(" ");
            Err(message)
        }
        HoldingsChoice::Live => {
            let oauth = oauth.expect("oauth is built on the non-fixture path");
            let token: schwab_live::TokenProvider =
                Arc::new(move || oauth.valid_access_token(chrono::Utc::now()));
            Ok(Box::new(
                schwab_live::SchwabApiSource::new(token).map_err(|e| e.to_string())?,
            ))
        }
    }
}

/// Begins the interactive Schwab OAuth connection (`docs/schwab-integration.md
/// §Authorization`). The loopback capture is a socket bind plus a browser
/// round-trip, so it runs through `spawn_blocking`. The client secret must
/// already be on the Keychain rail; nothing here logs a token.
#[tauri::command]
async fn schwab_connect(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
) -> Result<(), String> {
    let cfg = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn)
    };
    let client_id = cfg.schwab_client_id.clone().unwrap_or_default();
    if client_id.trim().is_empty() {
        return Err("Schwab client id is not configured".to_string());
    }
    let guard = guard.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        // Hold the run slot for the whole connection: the login can take minutes,
        // and a job starting meanwhile would touch the Keychain under it. The
        // token releases on drop.
        let _token = guard.try_begin(RunKind::SchwabConnect).ok_or_else(|| {
            "Another job is running — connect Schwab once it finishes.".to_string()
        })?;
        let store: Arc<dyn schwab_secrets::TokenStore> =
            Arc::new(schwab_secrets::KeyringTokenStore::new());
        let oauth =
            schwab_oauth::OauthClient::new(client_id.clone(), store).map_err(|e| e.to_string())?;
        let code =
            schwab_oauth::run_loopback_capture(&client_id, true).map_err(|e| e.to_string())?;
        oauth
            .exchange_code(&code, chrono::Utc::now())
            .map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("schwab connect task failed: {e}"))?
}

/// Splits the credentials by sensitivity (`docs/schwab-integration.md §Token
/// lifecycle`): `client_id` into `app_settings`, `client_secret` onto the Keychain
/// rail. The secret is written only when a non-blank value is supplied, so
/// re-saving the id never wipes a secret the form doesn't re-display. Pure over
/// the two stores so the split is unit-testable off the command.
fn persist_schwab_credentials(
    conn: &rusqlite::Connection,
    store: &dyn schwab_secrets::TokenStore,
    client_id: &str,
    client_secret: Option<&str>,
) -> anyhow::Result<()> {
    let client_id = client_id.trim();
    // A changed client id makes the stored token set stale (issued under the old
    // developer app), so the session is cleared to force a reconnect. A secret
    // rotation deliberately is not: a refresh token survives it.
    let previous_client_id = storage::get_setting(conn, config::KEY_SCHWAB_CLIENT_ID)?
        .unwrap_or_default();
    // The id is not a secret, so a blank value clears it rather than being left
    // in place the way the secret below is.
    storage::set_setting(conn, config::KEY_SCHWAB_CLIENT_ID, client_id)?;
    if previous_client_id.trim() != client_id {
        store.delete(schwab_secrets::SECRET_TOKENS)?;
    }
    if let Some(secret) = client_secret {
        // Trimmed before both the supplied check and the store: a pasted trailing
        // newline would otherwise be an unusable secret.
        let secret = secret.trim();
        if !secret.is_empty() {
            store.set(schwab_secrets::SECRET_CLIENT_SECRET, secret)?;
        }
    }
    Ok(())
}

/// Saves the Schwab developer-app credentials from Settings (`docs/interface.md
/// §Settings`). The secret never round-trips back; the frontend re-reads
/// `schwab_status` afterward.
#[tauri::command]
fn save_schwab_credentials(
    app: tauri::AppHandle,
    client_id: String,
    client_secret: Option<String>,
) -> Result<(), String> {
    let conn = open_app_db(&app)?;
    let store = schwab_secrets::KeyringTokenStore::new();
    persist_schwab_credentials(&conn, &store, &client_id, client_secret.as_deref())
        .map_err(|e| e.to_string())
}

/// The Schwab connection state for Settings (`docs/interface.md §Connection
/// status`): local storage only, no network probe; never returns the secret or a
/// token, and an unreadable token set reads as not connected.
#[tauri::command]
fn schwab_status(app: tauri::AppHandle) -> Result<schwab_oauth::SchwabStatus, String> {
    let client_id = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn).schwab_client_id.unwrap_or_default()
    };
    schwab_oauth::SchwabStatus::read(
        client_id,
        &schwab_secrets::KeyringTokenStore::new(),
        chrono::Utc::now(),
    )
    .map_err(|e| e.to_string())
}

/// Clears the stored OAuth token set so the next local run blocks with a re-auth
/// prompt. The developer-app credentials are deliberately kept, so a reconnect
/// needs only the browser round-trip.
#[tauri::command]
fn schwab_disconnect() -> Result<(), String> {
    let store = schwab_secrets::KeyringTokenStore::new();
    store
        .delete(schwab_secrets::SECRET_TOKENS)
        .map_err(|e| e.to_string())
}

/// Runs the local Portfolio Analysis job (`docs/portfolio-analysis.md`) and
/// returns the run. A connected Schwab account is a hard precondition (the
/// `MARKET_SIGNAL_SCHWAB_FIXTURE` escape hatch swaps in the offline fixture); the
/// gate is the local-suite gate — daemon reachable + roster + FMP / FRED key
/// presence — probed inside `spawn_blocking`. The run shares the single global
/// `RunGuard`, so the report and both local jobs are mutually exclusive.
///
/// Ordering: the daemon probe is the one check before the run slot, and it is
/// local-only. Every external fetch — FMP, SEC (the ticker → CIK map loaded
/// lazily on first use), FRED, Schwab — happens inside `run_portfolio_job` after
/// it claims the slot and clears the cancel flag, so a competing attempt that
/// records `Skipped` has contacted nothing, and no request row fires before
/// `run_started`.
#[tauri::command]
async fn generate_portfolio_manual(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
    cancel: tauri::State<'_, CancelFlag>,
    selected: Option<Vec<String>>,
    resume: Option<bool>,
) -> Result<portfolio::PortfolioRun, String> {
    // A resume reopens the interrupted run's own checkpoints
    // (`docs/portfolio-analysis.md` §Failure posture); it is validated inside the
    // blocking task, right before the job.
    let resume_requested = resume.unwrap_or(false);
    if resume_requested && selected.as_ref().is_some_and(|s| !s.is_empty()) {
        return Err("a resume reopens the interrupted run's own work-list — \
                    it cannot take a new selection"
            .to_string());
    }
    // A `rusqlite::Connection` is not `Send`, so the config read drops it before
    // the await.
    let cfg = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn)
    };
    let endpoint = local_model::endpoint_from_config(&cfg)
        .ok_or_else(|| "Local model daemon endpoint is not configured".to_string())?;
    let roster = local_model::roster_from_config(&cfg);
    // Key presence is enforced by `local_gate` below; the `unwrap_or_default`
    // only satisfies the constructor's signature.
    let fmp_key = cfg.fmp_api_key.clone().unwrap_or_default();
    let profile = portfolio::InvestorProfile::default_fixture();
    let paths = report_paths(&app)?;
    let guard = guard.inner().clone();
    let ctx = live_run_context(&app, cancel.inner().0.clone());

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let client = local_model::LocalModelClient::new(&endpoint)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());

        let probe = client.probe_daemon(&roster);
        let report = local_model::local_gate(&cfg, &probe);
        if report.is_blocked {
            return Err(config::blocked_summary_local_run(
                "run Portfolio Analysis",
                &report,
            ));
        }

        let analyst = portfolio::pipeline::LocalAnalyst::new(
            client,
            roster.reasoner.clone(),
            roster.fast.clone(),
        );
        // Tavily is reserved for the market-report job, so a local job never
        // spends that quota (`docs/web-research.md §Tavily fallback`). A stack that
        // can't construct runs research offline, never a blocked run — SearXNG sits
        // off the execution gate.
        let analyst = match portfolio::research::LiveResearchWeb::new(
            cfg.searxng_endpoint.as_deref(),
            &paths.db_path,
        ) {
            Ok(web) => analyst.with_research(portfolio::pipeline::LiveResearchCtx {
                web: std::sync::Arc::new(web),
                progress: ctx.clone(),
            }),
            Err(e) => {
                eprintln!("web-research stack unavailable ({e}); research runs offline");
                analyst
            }
        };
        let fmp = FmpDataSource::new(fmp_key.clone())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let sec = sec::SecEdgarSource::new()
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        // The ticker → CIK map loads on first use inside the run slot
        // (`LazyCikResolver`), never eagerly here: a prior cancelled run's
        // still-set cancel flag would make an eager refresh bail, and its request
        // row would fire before the tracker is listening.
        let cik_cache = sec::cik_cache_path_beside(&paths.db_path);
        let cik = sec::LazyCikResolver::new(&cik_cache);
        let company = portfolio::job::LiveCompanyData { fmp, sec, cik };
        let fred = crate::fred::FredDataSource::new(cfg.fred_api_key.clone().unwrap_or_default())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let market_fmp = FmpDataSource::new(fmp_key.clone())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        // The positioning legs are wholly fail-soft: a failed construction records
        // the leg's gap rather than blocking the run.
        let market_cot = crate::cot::CotDataSource::new()
            .ok()
            .map(|c| c.with_context(ctx.clone()));
        let market_cboe = crate::cboe::CboeDataSource::new()
            .ok()
            .map(|c| c.with_context(ctx.clone()));
        let market_finra = crate::finra::FinraDataSource::new()
            .ok()
            .map(|f| f.with_context(ctx.clone()));
        let market = portfolio::job::LiveMarketContext {
            fred,
            fmp: Some(market_fmp),
            cot: market_cot,
            cboe: market_cboe,
            finra: market_finra,
        };

        let holdings: Box<dyn schwab::HoldingsSource> = build_holdings_source(&cfg)?;

        // A per-card selection makes this a selective re-analysis
        // (`docs/portfolio-analysis.md` §Triggering): the safety sweep over the
        // unselected tail needs the quick check's engine-only surface, so it is
        // built only when a selection is active.
        let selected = selected.unwrap_or_default();
        let quick_data = if selected.is_empty() {
            None
        } else {
            let qc_fmp = FmpDataSource::new(fmp_key.clone())
                .map_err(|e| e.to_string())?
                .with_context(ctx.clone());
            let qc_sec = sec::SecEdgarSource::new()
                .map_err(|e| e.to_string())?
                .with_context(ctx.clone());
            let qc_cik = sec::LazyCikResolver::new(&cik_cache);
            let qc_fred =
                crate::fred::FredDataSource::new(cfg.fred_api_key.clone().unwrap_or_default())
                    .map_err(|e| e.to_string())?
                    .with_context(ctx.clone());
            Some(portfolio::quick_check::LiveQuickCheckData {
                fmp: qc_fmp,
                sec: qc_sec,
                cik: qc_cik,
                fred: qc_fred,
            })
        };
        let selective = quick_data.as_ref().map(|qd| portfolio::job::SelectiveRun {
            selected: selected.clone(),
            quick_data: qd,
        });

        // The accuracy checks' refresh surface (`docs/portfolio-analysis.md`
        // §Outcome learning).
        let out_fmp = FmpDataSource::new(fmp_key)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let outcome_prices = portfolio::outcome::LiveOutcomePrices { fmp: out_fmp };

        // Validated here so a refused resume fails fast with the reason, before
        // the slot is claimed. The roster check uses the ids the analyst will
        // actually run under.
        let resume_checkpoint = if resume_requested {
            let conn = storage::open(&paths.db_path).map_err(|e| e.to_string())?;
            let cp = portfolio::store::load_checkpoint(&conn)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "no interrupted run to resume".to_string())?;
            use portfolio::pipeline::HoldingAnalyst as _;
            let ids = vec![analyst.reasoner_id(), analyst.fast_id()];
            portfolio::job::resume_eligibility(&conn, &cp, &ids, chrono::Utc::now())
                .map_err(|reason| format!("cannot resume the interrupted run: {reason}"))?;
            Some(cp)
        } else {
            None
        };

        portfolio::job::run_portfolio_job(
            holdings.as_ref(),
            &company,
            &market,
            &analyst,
            &profile,
            selective,
            Some(&outcome_prices),
            resume_checkpoint,
            &paths,
            &guard,
            &ctx,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("portfolio analysis task failed: {e}"))??;

    match outcome {
        portfolio::job::PortfolioJobOutcome::Successful(run) => Ok(*run),
        portfolio::job::PortfolioJobOutcome::Failed(msg) => Err(msg),
        portfolio::job::PortfolioJobOutcome::Skipped(reason) => Err(reason),
        portfolio::job::PortfolioJobOutcome::Cancelled(reason) => Err(reason),
    }
}

/// The tracker's resume offer for a failed / cancelled run
/// (`docs/portfolio-analysis.md` §Failure posture). The trail is not necessarily
/// the shown run's own — a run failing before it opens its own trail leaves an
/// earlier one standing — which is why `run_id` and `created_at` identify it.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PortfolioResumeStatus {
    available: bool,
    /// `None` when no checkpoint trail exists at all.
    run_id: Option<String>,
    /// The pinned pull's as-of.
    created_at: Option<String>,
    completed: usize,
    /// The interrupted run's work-list size, or the whole book.
    total: usize,
    /// Why resume is unavailable, when a trail exists but cannot resume.
    reason: Option<String>,
}

/// Read-only: never claims the run slot and fetches nothing.
#[tauri::command]
fn portfolio_resume_status(app: tauri::AppHandle) -> Result<PortfolioResumeStatus, String> {
    let conn = open_app_db(&app)?;
    let cfg = AppConfig::load(&conn);
    let none = PortfolioResumeStatus {
        available: false,
        run_id: None,
        created_at: None,
        completed: 0,
        total: 0,
        reason: None,
    };
    let Some(cp) = portfolio::store::load_checkpoint(&conn).map_err(|e| e.to_string())? else {
        return Ok(none);
    };
    // The ids a resume's analyst would actually run under.
    let roster = local_model::roster_from_config(&cfg);
    let fast = portfolio::pipeline::effective_fast_model(&roster.reasoner, &roster.fast);
    let ids = vec![roster.reasoner.clone(), fast];
    let eligibility =
        portfolio::job::resume_eligibility(&conn, &cp, &ids, chrono::Utc::now());
    let total = cp
        .header
        .work_list
        .as_ref()
        .map(|w| w.len())
        .unwrap_or(cp.header.holdings.positions.len());
    Ok(PortfolioResumeStatus {
        available: eligibility.is_ok(),
        run_id: Some(cp.header.run_id.clone()),
        created_at: Some(cp.header.created_at.clone()),
        completed: cp.holdings.len(),
        total,
        reason: eligibility.err(),
    })
}

/// Runs the engine-only Portfolio quick check (`docs/portfolio-analysis.md §The
/// quick check`) and returns its state. The gate is presence-only — local-suite
/// config presence plus the Schwab connection, no daemon probe — so the check
/// runs even configured-but-down. Holds the global run slot like any job; no
/// external fetch precedes the slot (the ticker → CIK map loads lazily on first
/// use).
#[tauri::command]
async fn run_portfolio_quick_check(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
    cancel: tauri::State<'_, CancelFlag>,
) -> Result<portfolio::quick_check::QuickCheckState, String> {
    // A `rusqlite::Connection` is not `Send`, so the config read drops it before
    // the await.
    let cfg = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn)
    };
    let report = local_model::local_presence_gate(&cfg);
    if report.is_blocked {
        return Err(config::blocked_summary_named("run the quick check", &report));
    }
    let paths = report_paths(&app)?;
    let guard = guard.inner().clone();
    let ctx = live_run_context(&app, cancel.inner().0.clone());

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        // Building the source verifies the Schwab precondition though no Schwab
        // call is made; inside the blocking task because it reads the Keychain.
        build_holdings_source(&cfg)?;
        let fmp = FmpDataSource::new(cfg.fmp_api_key.clone().unwrap_or_default())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let sec = sec::SecEdgarSource::new()
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        // Loaded on first use inside the run slot, never eagerly here: no external
        // fetch before the slot is claimed and the cancel flag cleared.
        let cik = sec::LazyCikResolver::new(sec::cik_cache_path_beside(&paths.db_path));
        let fred = crate::fred::FredDataSource::new(cfg.fred_api_key.clone().unwrap_or_default())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let data = portfolio::quick_check::LiveQuickCheckData { fmp, sec, cik, fred };
        portfolio::quick_check::run_quick_check_job(&data, &paths, &guard, &ctx)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("quick check task failed: {e}"))??;

    match outcome {
        portfolio::quick_check::QuickCheckJobOutcome::Successful(state) => Ok(*state),
        portfolio::quick_check::QuickCheckJobOutcome::Failed(msg) => Err(msg),
        portfolio::quick_check::QuickCheckJobOutcome::Skipped(reason) => Err(reason),
        portfolio::quick_check::QuickCheckJobOutcome::Cancelled(reason) => Err(reason),
    }
}

/// `None` when no quick check has run since the last full pass.
#[tauri::command]
fn latest_quick_check(
    app: tauri::AppHandle,
) -> Result<Option<portfolio::quick_check::QuickCheckState>, String> {
    let conn = open_app_db(&app)?;
    portfolio::store::latest_quick_check(&conn).map_err(|e| e.to_string())
}

/// The most recent run (`docs/portfolio-analysis.md §Storage and display`), or
/// `None` before the first.
#[tauri::command]
fn latest_portfolio_run(
    app: tauri::AppHandle,
) -> Result<Option<portfolio::PortfolioRun>, String> {
    let conn = open_app_db(&app)?;
    portfolio::store::latest_run(&conn).map_err(|e| e.to_string())
}

/// The sidebar's run history (`docs/interface.md §Main Layout`): newest first,
/// capped at the retention window, columns only — never the full run blobs.
#[tauri::command]
fn list_portfolio_runs(
    app: tauri::AppHandle,
) -> Result<Vec<portfolio::store::PortfolioRunSummary>, String> {
    let conn = open_app_db(&app)?;
    portfolio::store::list_run_summaries(&conn, portfolio::PORTFOLIO_RUN_RETENTION)
        .map_err(|e| e.to_string())
}

/// One run for the historical view (`docs/interface.md §Main Layout`). `None`
/// when the id is unknown — e.g. pruned between listing and click; the frontend
/// re-lists.
#[tauri::command]
fn get_portfolio_run(
    app: tauri::AppHandle,
    run_id: String,
) -> Result<Option<portfolio::PortfolioRun>, String> {
    let conn = open_app_db(&app)?;
    portfolio::store::run_by_id(&conn, &run_id).map_err(|e| e.to_string())
}

/// The latest Pull holdings snapshot (`docs/portfolio-analysis.md §Triggering`):
/// view-only, never read by the job; `None` before any pull.
#[tauri::command]
fn latest_holdings_pull(
    app: tauri::AppHandle,
) -> Result<Option<portfolio::store::HoldingsPull>, String> {
    let conn = open_app_db(&app)?;
    portfolio::store::latest_pull(&conn).map_err(|e| e.to_string())
}

/// Standalone Pull holdings (`docs/portfolio-analysis.md §Triggering`): persists
/// the view-only snapshot and returns it; never a holdings-diff baseline. Gates on
/// the Schwab connection only, so it works before local models are configured.
/// Holds the run slot as `RunKind::HoldingsPull` so it can't race a job's own pull
/// or token refresh; the persist happens after the slot is released.
#[tauri::command]
async fn pull_holdings(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
) -> Result<portfolio::store::HoldingsPull, String> {
    // A `rusqlite::Connection` is not `Send`, so the config read drops it before
    // the await.
    let cfg = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn)
    };
    let guard = guard.inner().clone();

    let holdings = tauri::async_runtime::spawn_blocking(move || {
        let _token = guard
            .try_begin(RunKind::HoldingsPull)
            .ok_or_else(|| "Another job is running — pull holdings once it finishes.".to_string())?;
        let source = build_holdings_source(&cfg)?;
        // The same book-level normalization as a run's pull
        // (`docs/schwab-integration.md` §What is pulled), so the view and the job
        // read one position identity per symbol.
        source
            .holdings()
            .and_then(|h| h.normalized())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("holdings pull task failed: {e}"))??;

    let pull = portfolio::store::HoldingsPull {
        pulled_at: chrono::Utc::now().to_rfc3339(),
        holdings,
    };
    let conn = open_app_db(&app)?;
    portfolio::store::save_pull(&conn, &pull).map_err(|e| e.to_string())?;
    Ok(pull)
}

/// The local-suite presence gate for the Persistent Warning Area
/// (`docs/interface.md §Persistent Warning Area`): `is_blocked` means the local
/// jobs are blocked, never the report. No connectivity probe.
#[tauri::command]
fn check_local_configuration(app: tauri::AppHandle) -> Result<config::ValidationReport, String> {
    let cfg = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn)
    };
    Ok(local_configuration_report(
        &cfg,
        &schwab_secrets::KeyringTokenStore::new(),
        schwab_fixture_escape(),
        chrono::Utc::now(),
    ))
}

/// A failed Keychain read raises the Schwab category rather than failing the
/// report, so every other category still stands (`docs/schwab-integration.md
/// §Failure posture`).
fn local_configuration_report(
    cfg: &AppConfig,
    store: &dyn TokenStore,
    fixture_escape: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> config::ValidationReport {
    let local = local_model::local_presence_gate(cfg);
    let presence = if fixture_escape {
        // The offline fixture satisfies the holdings source, so don't warn about a
        // connection the run wouldn't use.
        schwab_oauth::SchwabPresence::Connected
    } else {
        schwab_oauth::schwab_presence(store, now)
    };
    let schwab = schwab_oauth::schwab_gate(presence);

    let mut categories = local.categories;
    categories.extend(schwab.categories);
    let is_blocked = categories.iter().any(|c| c.kind.is_blocking());
    config::ValidationReport {
        categories,
        is_blocked,
    }
}

/// The Recent Reports sidebar's list (`docs/interface.md`, `docs/storage.md`):
/// newest first, capped at the retention window. A fresh install lists as empty
/// rather than erroring.
#[tauri::command]
fn list_reports(app: tauri::AppHandle) -> Result<Vec<agent::ReportSummary>, String> {
    let paths = report_paths(&app)?;
    pipeline::list_reports(&paths).map_err(|e| e.to_string())
}

/// A report's summary plus its Markdown read back from disk
/// (`docs/report-workflow.md §Step 18`). An unknown id, or a Markdown file removed
/// out-of-band, is an error the view renders.
#[tauri::command]
fn load_report(app: tauri::AppHandle, report_id: String) -> Result<GeneratedReport, String> {
    let paths = report_paths(&app)?;
    pipeline::load_report(&paths, &report_id).map_err(|e| e.to_string())
}

/// Exports one report's Markdown to a user-chosen location (`docs/export.md`).
/// The report is resolved before any dialog pops, which also yields the
/// `created_at` for the suggested filename. `blocking_save_file` parks the calling
/// thread, so the dialog runs through `spawn_blocking`. Returns `Ok(false)` on a
/// cancelled dialog, `Ok(true)` once the file is written.
#[tauri::command]
async fn export_report_markdown(app: tauri::AppHandle, report_id: String) -> Result<bool, String> {
    let paths = report_paths(&app)?;

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

    let Some(chosen) = chosen else {
        return Ok(false);
    };

    let dest = chosen.into_path().map_err(|e| e.to_string())?;
    pipeline::export_markdown_to(&paths, &report_id, &dest).map_err(|e| e.to_string())?;
    Ok(true)
}

/// A missing or whitespace-only passphrase means plaintext
/// (`docs/data-portability.md §Optional passphrase encryption`); the string is
/// otherwise never trimmed.
fn normalized_passphrase(passphrase: Option<String>) -> Option<String> {
    passphrase.filter(|p| !p.trim().is_empty())
}

/// Exports the corpus to one archive (`docs/data-portability.md §Export flow`);
/// `Ok(None)` on a cancelled dialog. The run slot is held as
/// `RunKind::DataPortability` for the whole command, dialog included, so no job
/// can start against the store mid-archive.
#[tauri::command]
async fn export_data(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
    passphrase: Option<String>,
) -> Result<Option<portability::ExportSummary>, String> {
    let guard = guard.inner().clone();
    let Some(_token) = guard.try_begin(RunKind::DataPortability) else {
        return Err("A job is currently running — export can start once it finishes.".into());
    };

    // A corpus file, not a report file: local date, no `-<id8>` suffix
    // (`docs/data-portability.md §The archive`).
    let suggested = format!(
        "market-signal-export-{}.zip",
        chrono::Local::now().format("%Y-%m-%d")
    );
    let chosen = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app.dialog()
                .file()
                .set_file_name(&suggested)
                .add_filter("Zip archive", &["zip"])
                .blocking_save_file()
        })
        .await
        .map_err(|e| format!("save dialog task failed: {e}"))?
    };
    let Some(chosen) = chosen else {
        return Ok(None);
    };
    let dest = chosen.into_path().map_err(|e| e.to_string())?;

    let paths = report_paths(&app)?;
    let passphrase = normalized_passphrase(passphrase);
    tauri::async_runtime::spawn_blocking(move || {
        portability::export_archive(&paths, &dest, passphrase.as_deref())
            .map(Some)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("export task failed: {e}"))?
}

/// What the Import flow needs before committing (`docs/data-portability.md
/// §Import flow`): an empty store loads straight, a non-empty one goes through
/// the frontend's replace-all confirmation.
#[derive(serde::Serialize)]
struct ImportInspection {
    path: String,
    store_empty: bool,
    info: portability::ArchiveInfo,
}

/// Picks an archive and reads its manifest without touching the store;
/// `Ok(None)` on a cancelled dialog. Read-only, so it deliberately does not
/// claim the run slot.
#[tauri::command]
async fn import_data_inspect(
    app: tauri::AppHandle,
    passphrase: Option<String>,
) -> Result<Option<ImportInspection>, String> {
    let chosen = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app.dialog()
                .file()
                .add_filter("Zip archive", &["zip"])
                .blocking_pick_file()
        })
        .await
        .map_err(|e| format!("open dialog task failed: {e}"))?
    };
    let Some(chosen) = chosen else {
        return Ok(None);
    };
    let src = chosen.into_path().map_err(|e| e.to_string())?;

    let store_empty = {
        let conn = open_app_db(&app)?;
        portability::store_is_empty(&conn).map_err(|e| e.to_string())?
    };
    let path = src.to_string_lossy().into_owned();
    let passphrase = normalized_passphrase(passphrase);
    let info = tauri::async_runtime::spawn_blocking(move || {
        portability::inspect_archive(&src, passphrase.as_deref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("inspect task failed: {e}"))??;
    Ok(Some(ImportInspection {
        path,
        store_empty,
        info,
    }))
}

/// Loads an inspected archive (`docs/data-portability.md §Import flow`):
/// fresh-load into an empty store, or replace-all when `replace` is set.
/// `app_settings` is never touched. Slot-claimed like `export_data`.
#[tauri::command]
async fn import_data(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
    path: String,
    passphrase: Option<String>,
    replace: bool,
) -> Result<portability::ImportSummary, String> {
    let guard = guard.inner().clone();
    let Some(_token) = guard.try_begin(RunKind::DataPortability) else {
        return Err("A job is currently running — import can start once it finishes.".into());
    };
    let paths = report_paths(&app)?;
    let src = PathBuf::from(path);
    let passphrase = normalized_passphrase(passphrase);
    tauri::async_runtime::spawn_blocking(move || {
        portability::import_archive(&paths, &src, passphrase.as_deref(), replace)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("import task failed: {e}"))?
}

/// Ensures the data directory exists, so a command that touches the database
/// works before the first report has been generated.
fn open_app_db(app: &tauri::AppHandle) -> Result<rusqlite::Connection, String> {
    let paths = report_paths(app)?;
    if let Some(parent) = paths.db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating app data directory: {e}"))?;
    }
    let conn = storage::open(&paths.db_path).map_err(|e| e.to_string())?;
    storage::init_schema(&conn).map_err(|e| e.to_string())?;
    Ok(conn)
}

/// The status panel's job state (`docs/scheduling.md §Job Status Visibility`).
#[tauri::command]
fn job_status(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
) -> Result<JobStatus, String> {
    let conn = open_app_db(&app)?;
    jobs::job_status(&conn, &guard).map_err(|e| e.to_string())
}

/// The Settings state (`docs/configuration.md`, `docs/interface.md §Settings`):
/// a configured flag per credential, never the secret. Reads the saved store with
/// an env fallback per field.
#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<settings::SettingsView, String> {
    let conn = open_app_db(&app)?;
    Ok(settings::load_view(&conn))
}

/// Persists the cloud Settings submission (`docs/configuration.md §API Tokens`).
/// Each token is written only when a new value is supplied, so an untouched
/// field keeps its stored secret.
#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    models: settings::AgentModels,
    credentials: settings::CredentialUpdate,
) -> Result<(), String> {
    let conn = open_app_db(&app)?;
    settings::save(&conn, &models, &credentials).map_err(|e| e.to_string())
}

/// Persists the FMP / FRED / Tavily credentials, ungated by the cloud tokens so
/// a cloud-keyless machine completes local-suite setup (`docs/configuration.md
/// §External Data Provider Credentials`).
#[tauri::command]
fn save_provider_credentials(
    app: tauri::AppHandle,
    credentials: settings::ProviderCredentialUpdate,
) -> Result<(), String> {
    let conn = open_app_db(&app)?;
    settings::save_provider_credentials(&conn, &credentials).map_err(|e| e.to_string())
}

/// Persists the local-models config (`docs/configuration.md §Local Models`),
/// ungated. Presence, not connectivity: the save never probes the daemon.
#[tauri::command]
fn save_local_model_settings(
    app: tauri::AppHandle,
    values: settings::LocalModelSettings,
) -> Result<(), String> {
    let conn = open_app_db(&app)?;
    settings::save_local_models(&conn, &values).map_err(|e| e.to_string())
}

/// Validates one saved credential with a single live request (Settings "Test
/// connection"); an unset credential returns not-configured with no network
/// call. The blocking request goes through `spawn_blocking`. Validates the key
/// only: it never spends model tokens, and the execution gate checks presence,
/// not validity.
#[tauri::command]
async fn test_connection(
    app: tauri::AppHandle,
    provider: String,
) -> Result<connection_test::ConnectionTestResult, String> {
    use connection_test::CredentialProvider;
    let target = CredentialProvider::from_label(&provider).map_err(|e| e.to_string())?;

    // A `rusqlite::Connection` is not `Send`, so the credential read drops it
    // before the await.
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

/// Persists the SearXNG endpoint (`docs/configuration.md §Web Research`),
/// ungated: SearXNG is off every gate. No probe.
#[tauri::command]
fn save_web_research_settings(
    app: tauri::AppHandle,
    values: settings::WebResearchSettings,
) -> Result<(), String> {
    let conn = open_app_db(&app)?;
    settings::save_web_research(&conn, &values).map_err(|e| e.to_string())
}

/// Probes the configured SearXNG instance for the Settings row's test and the
/// pre-run notice (`docs/web-research.md §Tavily fallback`; `docs/interface.md
/// §Pre-run web-research notice`). Never a gate. The local suite is SearXNG-only,
/// so the preflight reports no fallback and a degraded run is always flagged not
/// recommended. The blocking probe goes through `spawn_blocking`.
#[tauri::command]
async fn test_searxng(
    app: tauri::AppHandle,
) -> Result<web_research::search::WebResearchPreflight, String> {
    // A `rusqlite::Connection` is not `Send`, so the config read drops it before
    // the await.
    let endpoint = {
        let conn = open_app_db(&app)?;
        let cfg = AppConfig::load(&conn);
        cfg.searxng_endpoint.clone()
    };
    tauri::async_runtime::spawn_blocking(move || {
        web_research::search::preflight(endpoint.as_deref())
    })
    .await
    .map_err(|e| format!("SearXNG probe task failed: {e}"))
}

/// Probes the local-model daemon for Settings "Test connection"; an unconfigured
/// endpoint returns not-configured with no network call. The blocking probe goes
/// through `spawn_blocking`. Read-only.
#[tauri::command]
async fn test_local_daemon(app: tauri::AppHandle) -> Result<local_model::LocalDaemonStatus, String> {
    // A `rusqlite::Connection` is not `Send`, so the config read drops it before
    // the await.
    let (endpoint, roster) = {
        let conn = open_app_db(&app)?;
        let cfg = AppConfig::load(&conn);
        (
            local_model::endpoint_from_config(&cfg),
            local_model::roster_from_config(&cfg),
        )
    };

    let Some(endpoint) = endpoint else {
        return Ok(local_model::LocalDaemonStatus::not_configured());
    };

    tauri::async_runtime::spawn_blocking(move || local_model::daemon_status(&endpoint, &roster))
        .await
        .map_err(|e| format!("local daemon test task failed: {e}"))
}

/// The research inbox's documents (`docs/research-documents.md`); a fresh
/// install with no inbox folder lists as empty. The last pass's parse failures
/// are joined best-effort (`§Parse Failures`): an unreadable DB costs the error
/// states, never the listing.
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

/// (`docs/research-documents.md` §User Permissions.) The name is validated as a
/// bare file name in `research::` so it cannot escape the inbox directory.
#[tauri::command]
fn delete_research_document(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let inbox = research_inbox_dir(&app)?;
    research::delete_folder_document(&inbox, &name).map_err(|e| e.to_string())
}

/// Opens the inbox folder in the OS file manager (`docs/research-documents.md`
/// §Research Inbox); created on demand so a first-time reveal lands somewhere
/// real.
#[tauri::command]
fn reveal_research_inbox(app: tauri::AppHandle) -> Result<(), String> {
    let inbox = research_inbox_dir(&app)?;
    std::fs::create_dir_all(&inbox)
        .map_err(|e| format!("creating research inbox directory: {e}"))?;
    app.opener()
        .open_path(inbox.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| format!("opening research inbox: {e}"))
}

/// The research archive's documents (`docs/research-documents.md`); a fresh
/// install with no archive folder lists as empty.
#[tauri::command]
fn list_research_archive(app: tauri::AppHandle) -> Result<Vec<research::ResearchDocument>, String> {
    let archive = research_archive_dir(&app)?;
    research::list_folder(&archive).map_err(|e| e.to_string())
}

/// (`docs/research-documents.md` §User Permissions.) The name is validated as a
/// bare file name in `research::` so it cannot escape the archive directory.
#[tauri::command]
fn delete_research_archive_document(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let archive = research_archive_dir(&app)?;
    research::delete_folder_document(&archive, &name).map_err(|e| e.to_string())
}

/// Opens the archive folder in the OS file manager (`docs/research-documents.md`
/// §User Permissions); created on demand so a first-time reveal lands somewhere
/// real.
#[tauri::command]
fn reveal_research_archive(app: tauri::AppHandle) -> Result<(), String> {
    let archive = research_archive_dir(&app)?;
    std::fs::create_dir_all(&archive)
        .map_err(|e| format!("creating research archive directory: {e}"))?;
    app.opener()
        .open_path(archive.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| format!("opening research archive: {e}"))
}

/// The Settings diagnostics' truncation telemetry (`docs/agents.md §Data
/// Extraction`). Fail-soft: an unopenable DB degrades to an empty
/// `TruncationStats` rather than failing the Settings load, since the empty
/// aggregate is itself a signal.
#[tauri::command]
fn truncation_stats(app: tauri::AppHandle) -> storage::TruncationStats {
    let Ok(conn) = open_app_db(&app) else {
        return storage::TruncationStats::default();
    };
    storage::truncation_stats(&conn).unwrap_or_default()
}

/// The investor-profile preset as read-only Settings rows (`docs/configuration.md`
/// §Investor Profile). Pure: the preset is not user-configured, so the display
/// derives from the fixture alone, through the same label source the action
/// prompt renders.
#[tauri::command]
fn get_investor_profile() -> portfolio::InvestorProfileDisplay {
    portfolio::InvestorProfile::default_fixture().display()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(RunGuard::default())
        .manage(CancelFlag::default())
        .setup(|app| {
            // Legacy-naming migrations (docs/storage.md §Legacy Naming Migration):
            // best-effort and idempotent — a failure logs and launch proceeds.
            match open_app_db(app.handle()) {
                Ok(conn) => {
                    if let Err(e) = storage::migrate_legacy_naming(&conn) {
                        eprintln!("legacy-naming migration: degraded ({e})");
                    }
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
            generate_portfolio_manual,
            portfolio_resume_status,
            run_portfolio_quick_check,
            latest_quick_check,
            latest_portfolio_run,
            list_portfolio_runs,
            get_portfolio_run,
            latest_holdings_pull,
            pull_holdings,
            check_local_configuration,
            schwab_connect,
            save_schwab_credentials,
            schwab_status,
            schwab_disconnect,
            cancel_run,
            list_reports,
            load_report,
            export_report_markdown,
            export_data,
            import_data_inspect,
            import_data,
            check_configuration,
            dismiss_warning,
            job_status,
            get_settings,
            save_settings,
            save_provider_credentials,
            save_local_model_settings,
            save_web_research_settings,
            test_searxng,
            test_connection,
            test_local_daemon,
            list_research_inbox,
            delete_research_document,
            reveal_research_inbox,
            list_research_archive,
            delete_research_archive_document,
            reveal_research_archive,
            truncation_stats,
            get_investor_profile
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
    fn thought_log_gate_is_debug_default_with_an_explicit_override() {
        assert!(thought_log_enabled(None, true));
        assert!(!thought_log_enabled(None, false));
        assert!(!thought_log_enabled(Some("0"), true));
        assert!(!thought_log_enabled(Some("false"), true));
        assert!(thought_log_enabled(Some("1"), false));
        assert!(thought_log_enabled(Some(" TRUE "), false));
        // An unrecognized value falls to the build default.
        assert!(thought_log_enabled(Some("yes please"), true));
        assert!(!thought_log_enabled(Some("yes please"), false));
    }

    #[test]
    fn holdings_source_choice_prefers_fixture_then_live_then_blocks() {
        assert_eq!(choose_holdings_source(true, true), HoldingsChoice::Fixture);
        assert_eq!(choose_holdings_source(true, false), HoldingsChoice::Fixture);
        assert_eq!(choose_holdings_source(false, true), HoldingsChoice::Live);
        assert_eq!(
            choose_holdings_source(false, false),
            HoldingsChoice::NotConnected
        );
    }

    /// Fully configured, so the only category the local report can raise is
    /// Schwab's.
    fn configured_local_suite() -> AppConfig {
        AppConfig {
            local_daemon_endpoint: Some("http://localhost:11434".into()),
            local_reasoner_model: Some("qwen3.5:122b".into()),
            fmp_api_key: Some("fmp-key".into()),
            fred_api_key: Some("fred-key".into()),
            ..AppConfig::default()
        }
    }

    #[test]
    fn local_configuration_report_raises_the_schwab_warning_on_a_denied_keychain_read() {
        let report = local_configuration_report(
            &configured_local_suite(),
            &schwab_secrets::DeniedTokenStore,
            false,
            chrono::Utc::now(),
        );
        assert!(report.is_blocked);
        assert_eq!(report.categories.len(), 1, "{:?}", report.categories);
        let cat = &report.categories[0];
        assert_eq!(cat.kind, config::WarningKind::Schwab);
        assert_eq!(
            cat.items,
            vec![schwab_oauth::SCHWAB_UNREADABLE_MSG.to_string()]
        );
    }

    #[test]
    fn local_configuration_report_keeps_the_other_categories_beside_a_denied_read() {
        let report = local_configuration_report(
            &AppConfig::default(),
            &schwab_secrets::DeniedTokenStore,
            false,
            chrono::Utc::now(),
        );
        let kinds: Vec<_> = report.categories.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&config::WarningKind::LocalModels), "{kinds:?}");
        assert!(kinds.contains(&config::WarningKind::ProviderCredentials), "{kinds:?}");
        assert!(kinds.contains(&config::WarningKind::Schwab), "{kinds:?}");
        assert!(report.is_blocked);
    }

    #[test]
    fn local_configuration_report_never_reads_the_store_behind_the_fixture_escape() {
        let report = local_configuration_report(
            &configured_local_suite(),
            &schwab_secrets::DeniedTokenStore,
            true,
            chrono::Utc::now(),
        );
        assert!(report.categories.is_empty(), "{:?}", report.categories);
        assert!(!report.is_blocked);
    }

    #[test]
    fn local_configuration_report_reads_the_stored_connection() {
        let now = chrono::Utc::now();
        let store = schwab_secrets::InMemoryTokenStore::new();
        let report = local_configuration_report(&configured_local_suite(), &store, false, now);
        assert!(report.is_blocked);
        assert_eq!(
            report.categories[0].items,
            vec![schwab_oauth::SCHWAB_NOT_CONNECTED_MSG.to_string()]
        );
        store
            .set_tokens(&schwab_secrets::SchwabTokens {
                access_token: "a".into(),
                refresh_token: "r".into(),
                access_expires_at: now + chrono::Duration::minutes(30),
                refresh_expires_at: now + chrono::Duration::days(7),
            })
            .unwrap();
        let report = local_configuration_report(&configured_local_suite(), &store, false, now);
        assert!(report.categories.is_empty(), "{:?}", report.categories);
        assert!(!report.is_blocked);
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
        assert_eq!(
            resolve_data_dir(base.clone(), Some(String::new()), true),
            PathBuf::from("/data/app/dev"),
        );
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

    #[test]
    fn persist_schwab_credentials_splits_the_stores_and_preserves_a_kept_secret() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        let store = schwab_secrets::InMemoryTokenStore::new();

        persist_schwab_credentials(&conn, &store, "  client-abc  ", Some("dev-secret")).unwrap();
        assert_eq!(
            storage::get_setting(&conn, config::KEY_SCHWAB_CLIENT_ID)
                .unwrap()
                .as_deref(),
            Some("client-abc") // trimmed
        );
        assert_eq!(
            store.get(schwab_secrets::SECRET_CLIENT_SECRET).unwrap().as_deref(),
            Some("dev-secret")
        );

        persist_schwab_credentials(&conn, &store, "client-xyz", None).unwrap();
        assert_eq!(
            storage::get_setting(&conn, config::KEY_SCHWAB_CLIENT_ID)
                .unwrap()
                .as_deref(),
            Some("client-xyz")
        );
        assert_eq!(
            store.get(schwab_secrets::SECRET_CLIENT_SECRET).unwrap().as_deref(),
            Some("dev-secret")
        );
        // A whitespace-only secret is likewise a no-op, not a wipe.
        persist_schwab_credentials(&conn, &store, "client-xyz", Some("   ")).unwrap();
        assert_eq!(
            store.get(schwab_secrets::SECRET_CLIENT_SECRET).unwrap().as_deref(),
            Some("dev-secret")
        );
    }

    #[test]
    fn persist_schwab_credentials_clears_stale_tokens_only_when_the_client_id_changes() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        let store = schwab_secrets::InMemoryTokenStore::new();

        persist_schwab_credentials(&conn, &store, "client-abc", Some("secret-1")).unwrap();
        let now = chrono::Utc::now();
        store
            .set_tokens(&schwab_secrets::SchwabTokens {
                access_token: "a".into(),
                refresh_token: "r".into(),
                access_expires_at: now + chrono::Duration::minutes(30),
                refresh_expires_at: now + chrono::Duration::days(7),
            })
            .unwrap();

        persist_schwab_credentials(&conn, &store, "client-abc", Some("secret-2")).unwrap();
        assert!(
            store.tokens().unwrap().is_some(),
            "a secret rotation must not drop the session"
        );

        persist_schwab_credentials(&conn, &store, "client-xyz", None).unwrap();
        assert!(
            store.tokens().unwrap().is_none(),
            "a client-id change must clear the stale token set"
        );
        assert_eq!(
            store.get(schwab_secrets::SECRET_CLIENT_SECRET).unwrap().as_deref(),
            Some("secret-2")
        );
    }
}
