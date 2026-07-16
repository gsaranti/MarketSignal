pub mod agent;
pub mod analyst_agent;
pub mod baseline_delta;
pub mod bls;
pub mod cadence;
pub mod config;
pub mod connection_test;
pub mod cot;
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
pub mod local_model;
pub mod loopback_https;
pub mod market_clock;
pub mod model_agent;
pub mod news;
pub mod pipeline;
pub mod portability;
pub mod portfolio;
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
pub mod stooq;
pub mod storage;
pub mod tavily;
#[cfg(test)]
mod test_http;
pub mod vector_memory;

// Dev-only demo mode. Compiled out entirely unless the `demo-run` feature is on
// (it is not in `default`, so `tauri build` never includes it). See `demo.rs`.
#[cfg(feature = "demo-run")]
mod demo;

/// Whether the demo run path is active: the `demo-run` feature is compiled in AND
/// `MARKET_SIGNAL_DEMO` is set to a truthy value (`1` / `true`, case-insensitive). A
/// bare, empty, or `0` value reads as off, so the var can be exported `=0` without
/// silently enabling stub reports.
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
    // Demo mode: report a clean, unblocked gate so the live "Generate now" button is
    // enabled with no keys configured (the demo run path bypasses the gate anyway).
    // Compiled out of any normal/release build with the rest of demo mode.
    #[cfg(feature = "demo-run")]
    if demo_run_enabled() {
        return ValidationReport {
            categories: Vec::new(),
            is_blocked: false,
        };
    }

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
    // Dev-only demo path (feature `demo-run` + MARKET_SIGNAL_DEMO): run the real
    // pipeline against paced, streaming stubs — no keys, no network, no gate. The
    // entire branch is compiled out of a normal/release build, leaving the live
    // body below byte-for-byte unchanged.
    #[cfg(feature = "demo-run")]
    if demo_run_enabled() {
        return generate_report_demo(app, guard, cancel).await;
    }

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
        // dollar index, oil, gas, macro levels) + BLS (labor levels) + CFTC (COT
        // positioning) merged behind one trait (`docs/report-workflow.md §Step 3`). BLS
        // and CFTC are keyless (not in the execution gate); they nest as outer secondaries
        // so their labor_levels / cot_positioning groups fold into the FMP+FRED baseline.
        let fmp = FmpDataSource::new(fmp_key.clone())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let fred = FredDataSource::new(fred_key)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
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

/// Dev-only demo run (feature `demo-run`). Reuses the real run plumbing — the same
/// `report_paths`, `live_run_context` (Tauri-event reporter), `RunGuard`, and
/// `jobs::run_job` — but injects the paced, streaming demo stages from `demo.rs`
/// instead of the live adapters, and skips the key/credential gate. Everything
/// downstream (step events, persistence to the dev data dir, the terminal
/// `run-finished`) is identical to a real run, so the tracker and report rendering
/// are exercised faithfully with no I/O. Mirrors `generate_report_manual`'s
/// spawn-blocking + outcome handling.
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
    /// The offline fixture (the `MARKET_SIGNAL_SCHWAB_FIXTURE` escape hatch), so the
    /// pipeline still runs with no Schwab connection for local validation.
    Fixture,
    /// The live Schwab Trader API source — a connected account with an open refresh
    /// window.
    Live,
    /// No connection: the job is blocked with a re-authentication prompt rather than
    /// run in a degraded mode.
    NotConnected,
}

/// Decide the holdings source from the fixture escape hatch and the live connection
/// state. Pure so the gate's decision table is unit-testable without a Keychain or a
/// network. The escape hatch wins first (offline validation), then a live connection,
/// else the job blocks.
fn choose_holdings_source(fixture_escape: bool, connected: bool) -> HoldingsChoice {
    if fixture_escape {
        HoldingsChoice::Fixture
    } else if connected {
        HoldingsChoice::Live
    } else {
        HoldingsChoice::NotConnected
    }
}

/// Whether the offline Schwab fixture escape hatch is set (`MARKET_SIGNAL_SCHWAB_FIXTURE`
/// truthy). Lets the local jobs run against the fixture with no Schwab connection for
/// pipeline validation, exactly as this slice did before the live source landed.
fn schwab_fixture_escape() -> bool {
    std::env::var("MARKET_SIGNAL_SCHWAB_FIXTURE")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// Build the holdings source — the single source-selection seam every holdings
/// fetch goes through (`generate_portfolio_manual` and `pull_holdings`): the
/// offline fixture behind the escape hatch, else the live Schwab source once
/// connected, else the [`schwab_oauth::schwab_gate`] block flattened to its
/// re-auth prompt. Only the non-fixture path touches the Keychain rail / OAuth.
/// Blocking (Keychain reads); call inside `spawn_blocking`.
fn build_holdings_source(cfg: &AppConfig) -> Result<Box<dyn schwab::HoldingsSource>, String> {
    let store: Arc<dyn schwab_secrets::TokenStore> =
        Arc::new(schwab_secrets::KeyringTokenStore::new());
    let fixture_escape = schwab_fixture_escape();
    let oauth = if fixture_escape {
        None
    } else {
        let client_id = cfg.schwab_client_id.clone().unwrap_or_default();
        Some(Arc::new(
            schwab_oauth::OauthClient::new(client_id, store).map_err(|e| e.to_string())?,
        ))
    };
    let connected = match &oauth {
        Some(o) => o
            .is_connected(chrono::Utc::now())
            .map_err(|e| e.to_string())?,
        None => false,
    };
    match choose_holdings_source(fixture_escape, connected) {
        HoldingsChoice::Fixture => Ok(Box::new(schwab::FixtureHoldingsSource::new())),
        HoldingsChoice::NotConnected => {
            // Produce + consume the WarningKind::Schwab category via the shared
            // gate, mirroring how `local_gate` blocks a run for LocalModels; the
            // same producer feeds the warning band via `check_local_configuration`.
            // Surface its item — the specific reconnect prompt — as the gate error.
            let report = schwab_oauth::schwab_gate(false);
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

/// Begin the interactive Schwab OAuth connection (`docs/schwab-integration.md
/// §Authorization`): stand up the self-signed HTTPS loopback server, open the system
/// browser for the brokerage login, capture the redirect's authorization code, and
/// exchange it for the token set stored on the Keychain rail. Runs the blocking capture
/// (a socket bind + browser round-trip) through `spawn_blocking`. The client *secret*
/// must already be on the Keychain rail (written by the Settings connection surface —
/// deferred); this reads it there and never logs a token.
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
        // Hold the single global run slot for the whole connection: the interactive
        // login can take minutes, and letting a report/portfolio job start meanwhile
        // (both touch the Keychain / shared state) would violate the one-workflow-at-a-
        // time contract. The token releases on drop — success, failure, or panic.
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

/// Persist the Schwab developer-app credentials the OAuth connection needs, split across
/// the two stores by sensitivity (`docs/schwab-integration.md §Token lifecycle`): the
/// non-secret `client_id` into `app_settings` (its read side is `AppConfig::load`) and the
/// bearer `client_secret` onto the Keychain rail. The secret is written only when a fresh,
/// non-blank value is supplied, so re-saving to update the id — or to reconnect — never
/// wipes a stored secret the form doesn't re-display, exactly as `settings::save` treats
/// the API keys. Pure over the two stores so the split is unit-testable off the command.
fn persist_schwab_credentials(
    conn: &rusqlite::Connection,
    store: &dyn schwab_secrets::TokenStore,
    client_id: &str,
    client_secret: Option<&str>,
) -> anyhow::Result<()> {
    let client_id = client_id.trim();
    // A changed client id makes any stored OAuth token set stale — those tokens were
    // issued under the old developer app and can't be refreshed with the new one — so
    // clear the session and force a reconnect, rather than let `schwab_status` report a
    // falsely-connected account (or a later job fail confusingly on a mismatched
    // refresh). A secret *rotation* is deliberately NOT cleared: a refresh token survives
    // a secret change, so correcting or rotating only the secret keeps the connection.
    let previous_client_id = storage::get_setting(conn, config::KEY_SCHWAB_CLIENT_ID)?
        .unwrap_or_default();
    // The id is not a secret, so (like a model slug) it is written in full — a blank value
    // clears it — rather than left-in-place-when-empty the way the secret below is.
    storage::set_setting(conn, config::KEY_SCHWAB_CLIENT_ID, client_id)?;
    if previous_client_id.trim() != client_id {
        store.delete(schwab_secrets::SECRET_TOKENS)?;
    }
    if let Some(secret) = client_secret {
        // Trim both to decide "supplied" and to store: a value pasted with a stray
        // trailing newline/space would otherwise be an unusable secret. This is
        // deliberate paste hygiene, matching how `settings::save` stores the API keys.
        let secret = secret.trim();
        if !secret.is_empty() {
            store.set(schwab_secrets::SECRET_CLIENT_SECRET, secret)?;
        }
    }
    Ok(())
}

/// Save the Schwab developer-app credentials from the Settings "Charles Schwab connection"
/// surface (`docs/interface.md §Settings`) — the write path that lets the loopback connect
/// find its `client_id` (`app_settings`) and `client_secret` (Keychain). Sync: local
/// SQLite + Keychain writes, no network. The secret never round-trips back; the frontend
/// re-reads `schwab_status` afterward for the connection view.
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

/// The current Schwab connection state for the Settings surface (`docs/interface.md
/// §Connection status`): the configured `client_id`, whether the client secret is present,
/// and the derived connection state (never-connected / connected / lapsed) with the
/// refresh-window expiry for the weekly-re-login heads-up. Read from local storage only —
/// no network probe, mirroring the report's presence-not-connectivity posture — and never
/// returns the secret or a token. Sync, like `get_settings`.
#[tauri::command]
fn schwab_status(app: tauri::AppHandle) -> Result<schwab_oauth::SchwabStatus, String> {
    let client_id = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn).schwab_client_id.unwrap_or_default()
    };
    let store = schwab_secrets::KeyringTokenStore::new();
    let secret_configured = store
        .get(schwab_secrets::SECRET_CLIENT_SECRET)
        .map_err(|e| e.to_string())?
        .is_some_and(|s| !s.trim().is_empty());
    let tokens = store.tokens().map_err(|e| e.to_string())?;
    Ok(schwab_oauth::SchwabStatus::build(
        client_id,
        secret_configured,
        tokens,
        chrono::Utc::now(),
    ))
}

/// Disconnect the Schwab account: clear the stored OAuth token set from the Keychain rail
/// so the next local run blocks with a re-auth prompt. The developer-app credentials
/// (`client_id` + `client_secret`) are deliberately kept, so a reconnect — the routine
/// weekly re-login — needs only the browser round-trip, not re-entering the app secret.
/// Sync: a single local Keychain delete.
#[tauri::command]
fn schwab_disconnect() -> Result<(), String> {
    let store = schwab_secrets::KeyringTokenStore::new();
    store
        .delete(schwab_secrets::SECRET_TOKENS)
        .map_err(|e| e.to_string())
}

/// Manually run the local Portfolio Analysis job (`docs/portfolio-analysis.md`). Holdings
/// come from the **live Schwab Trader API** (`schwab_live`) once the account is connected
/// — an OAuth loopback with a 30-min/7-day token lifecycle — plus live FMP + keyless SEC
/// EDGAR and the local models. A connected account is a hard precondition: without one the
/// run is blocked with a re-auth prompt, not degraded. The `MARKET_SIGNAL_SCHWAB_FIXTURE`
/// escape hatch swaps in the offline fixture so the pipeline still validates with no
/// Schwab connection.
///
/// The gate is the **local-suite gate** (daemon reachable + roster present), independent
/// of the cloud-report gate — probed inside `spawn_blocking` since the probe is a
/// blocking call. Like `generate_report_manual`, the blocking run (Schwab + local model
/// HTTP + FMP/SEC) goes through `spawn_blocking` and shares the single global `RunGuard`,
/// so the report and both local jobs are mutually exclusive.
#[tauri::command]
async fn generate_portfolio_manual(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
    cancel: tauri::State<'_, CancelFlag>,
) -> Result<portfolio::PortfolioRun, String> {
    // Read config on a short-lived connection dropped before the await (a
    // `rusqlite::Connection` is not `Send`).
    let cfg = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn)
    };
    let endpoint = local_model::endpoint_from_config(&cfg)
        .ok_or_else(|| "Local model daemon endpoint is not configured".to_string())?;
    let roster = local_model::roster_from_config(&cfg);
    // FMP supplies the per-company price/financials; an absent key degrades to gaps
    // (the holding may then abstain), which is fail-soft for this slice.
    let fmp_key = cfg.fmp_api_key.clone().unwrap_or_default();
    let profile = portfolio::InvestorProfile::default_fixture();
    let paths = report_paths(&app)?;
    let guard = guard.inner().clone();
    let ctx = live_run_context(&app, cancel.inner().0.clone());

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let client = local_model::LocalModelClient::new(&endpoint)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());

        // Local-suite execution gate: probe the daemon, then gate on config + reachability
        // + roster presence. Blocked runs refuse before any analysis work.
        let probe = client.probe_daemon(&roster);
        let report = local_model::local_gate(&cfg, &probe);
        if report.is_blocked {
            return Err(config::blocked_summary(&report));
        }

        let analyst = portfolio::pipeline::LocalAnalyst::new(
            client,
            roster.reasoner.clone(),
            roster.fast.clone(),
        );
        let fmp = FmpDataSource::new(fmp_key)
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let sec = sec::SecEdgarSource::new()
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        // Ticker→CIK resolution over SEC's full company_tickers.json map, cached
        // beside the database with a staleness window; a failed refresh falls back
        // fail-soft (stale cache, else empty → typed gaps per holding).
        let cik_cache = paths
            .db_path
            .parent()
            .map(|d| d.join("sec_company_tickers.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("sec_company_tickers.json"));
        let cik = sec::load_cik_resolver(&cik_cache, &sec);
        let stooq = stooq::StooqSource::new()
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let company = portfolio::job::LiveCompanyData { fmp, sec, cik, stooq };
        // The run-level rate anchors (FRED DGS2/DGS10 + the DGS10 anchor-window
        // history) — hard-fail inside the job, before any per-holding work.
        let fred = crate::fred::FredDataSource::new(cfg.fred_api_key.clone().unwrap_or_default())
            .map_err(|e| e.to_string())?
            .with_context(ctx.clone());
        let market = portfolio::job::LiveMarketContext { fred };

        // Source selection: the shared seam (`build_holdings_source`) — fixture escape
        // hatch, else live Schwab once connected, else a blocked run with a re-auth
        // prompt.
        let holdings: Box<dyn schwab::HoldingsSource> = build_holdings_source(&cfg)?;

        portfolio::job::run_portfolio_job(
            holdings.as_ref(),
            &company,
            &market,
            &analyst,
            &profile,
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

/// The most recent persisted Portfolio Analysis run for the Portfolio page
/// (`docs/portfolio-analysis.md §Storage and display`), or `None` before the first
/// run — the frontend renders the empty state. Sync: one local SQLite read.
#[tauri::command]
fn latest_portfolio_run(
    app: tauri::AppHandle,
) -> Result<Option<portfolio::PortfolioRun>, String> {
    let conn = open_app_db(&app)?;
    portfolio::store::latest_run(&conn).map_err(|e| e.to_string())
}

/// The latest standalone **Pull holdings** snapshot for the Portfolio page
/// (`docs/portfolio-analysis.md §Triggering` — view-only, never read by the job),
/// or `None` before any pull. Sync: one local SQLite read.
#[tauri::command]
fn latest_holdings_pull(
    app: tauri::AppHandle,
) -> Result<Option<portfolio::store::HoldingsPull>, String> {
    let conn = open_app_db(&app)?;
    portfolio::store::latest_pull(&conn).map_err(|e| e.to_string())
}

/// Standalone **Pull holdings** (`docs/portfolio-analysis.md §Triggering`): fetch the
/// current positions from the connected Schwab account (or the fixture behind the
/// escape hatch), persist them as the latest view-only snapshot, and return them. It
/// never triggers analysis and never becomes the holdings-diff baseline — the job
/// always re-pulls and diffs against the prior *run's* snapshot. Gates on the Schwab
/// connection only (no model call → no local-model gate), so it works before local
/// models are configured. Holds the single run slot as `RunKind::HoldingsPull` so it
/// can't race a job's own pull or token refresh; the quick local persist happens
/// after the slot is released.
#[tauri::command]
async fn pull_holdings(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
) -> Result<portfolio::store::HoldingsPull, String> {
    // Read config on a short-lived connection dropped before the await (a
    // `rusqlite::Connection` is not `Send`).
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
        // Snapshot assembly runs the same book-level normalization as a run's pull
        // (`docs/schwab-integration.md` §What is pulled), so the view and the job
        // read one position identity per symbol.
        source
            .holdings()
            .map(|h| h.normalized())
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

/// The local-suite **presence** gate for the Persistent Warning Area
/// (`docs/interface.md §Persistent Warning Area` — both local categories are
/// presence-based, fired on missing *configuration*, never a connectivity probe):
/// the `local-models` category from the config fields alone
/// (`local_model::local_presence_gate`) plus the `schwab` category from the stored
/// token state (`schwab_oauth::schwab_gate` — not-connected or refresh lapsed, no
/// network). Deliberately separate from the cloud `check_configuration`:
/// `is_blocked` here means the **local jobs** are blocked, never the report. Sync —
/// local SQLite + Keychain reads only.
#[tauri::command]
fn check_local_configuration(app: tauri::AppHandle) -> Result<config::ValidationReport, String> {
    let cfg = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn)
    };
    let local = local_model::local_presence_gate(&cfg);
    let store = schwab_secrets::KeyringTokenStore::new();
    let connected = if schwab_fixture_escape() {
        // The offline fixture satisfies the holdings source, so don't warn about a
        // connection the run wouldn't use.
        true
    } else {
        store
            .tokens()
            .map_err(|e| e.to_string())?
            .is_some_and(|t| t.refresh_valid(chrono::Utc::now()))
    };
    let schwab = schwab_oauth::schwab_gate(connected);

    let mut categories = local.categories;
    categories.extend(schwab.categories);
    let is_blocked = categories.iter().any(|c| c.kind.is_blocking());
    Ok(config::ValidationReport {
        categories,
        is_blocked,
    })
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

/// A supplied passphrase, with the "leave blank for plaintext" convention
/// applied: a missing or whitespace-only value means no encryption
/// (`docs/data-portability.md §Optional passphrase encryption`). The original
/// string is otherwise preserved byte-for-byte — a passphrase is never trimmed.
fn normalized_passphrase(passphrase: Option<String>) -> Option<String> {
    passphrase.filter(|p| !p.trim().is_empty())
}

/// Export the whole analytical corpus to a single archive
/// (`docs/data-portability.md §Export flow`): a Save dialog picks the
/// destination, then `portability::export_archive` serializes the included
/// tables and files entirely in Rust. Returns `Ok(None)` on a cancelled dialog.
///
/// The run slot is claimed as `RunKind::DataPortability` for the whole command
/// — including the dialog — so a report or local-suite job can never start
/// against the store mid-archive (and a mid-run export can never capture a
/// half-written state).
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

    // The archive is a corpus file, not a report file: local date, no `-<id8>`
    // suffix (`docs/data-portability.md §The archive`).
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
    // The manifest stamps the local embedder identity for any local-suite
    // vector namespaces (the report namespace is the fixed cloud model).
    let local_embedder = {
        let conn = open_app_db(&app)?;
        AppConfig::load(&conn).local_embedder_model
    };
    let passphrase = normalized_passphrase(passphrase);
    tauri::async_runtime::spawn_blocking(move || {
        portability::export_archive(&paths, &dest, passphrase.as_deref(), local_embedder.as_deref())
            .map(Some)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("export task failed: {e}"))?
}

/// What the Import flow needs to know before committing
/// (`docs/data-portability.md §Import flow`): the picked archive, its manifest
/// read, and whether the target store is empty (empty → straight load;
/// non-empty → the frontend's replace-all confirmation).
#[derive(serde::Serialize)]
struct ImportInspection {
    path: String,
    store_empty: bool,
    info: portability::ArchiveInfo,
}

/// Pick an archive and read its manifest without touching the store. Returns
/// `Ok(None)` on a cancelled dialog. An encrypted archive without a passphrase
/// (or with the wrong one) surfaces as an error telling the user to supply it —
/// read-only, so it deliberately does not claim the run slot.
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

/// Load an inspected archive into the store (`docs/data-portability.md §Import
/// flow`): fresh-load into an empty store, or replace-all when the frontend's
/// confirmation set `replace`. `app_settings` is never read or written. Slot-
/// claimed like `export_data`, so no job can start against the store
/// mid-replacement.
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

/// Probe the local-model daemon for the Settings "Test connection" control (the local
/// analysis suite's parallel to `test_connection`). Reads the *saved* local config
/// (endpoint + roster, env fallback per field); an unconfigured endpoint returns a
/// not-configured result with no network call. The blocking probe (`/api/tags`) goes
/// through `spawn_blocking` — `reqwest::blocking` would panic on the async runtime
/// thread, the same seam `generate_report_manual` uses. Read-only: it never starts a
/// job and does not touch the cloud-report gate.
#[tauri::command]
async fn test_local_daemon(app: tauri::AppHandle) -> Result<local_model::LocalDaemonStatus, String> {
    // Read the saved config on a short-lived connection dropped before the await — a
    // `rusqlite::Connection` is not `Send` and must never cross an await point.
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
            generate_portfolio_manual,
            latest_portfolio_run,
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
            test_connection,
            test_local_daemon,
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
    fn holdings_source_choice_prefers_fixture_then_live_then_blocks() {
        // The fixture escape hatch wins even when a live connection exists...
        assert_eq!(choose_holdings_source(true, true), HoldingsChoice::Fixture);
        assert_eq!(choose_holdings_source(true, false), HoldingsChoice::Fixture);
        // ...otherwise a live connection is used...
        assert_eq!(choose_holdings_source(false, true), HoldingsChoice::Live);
        // ...and with neither, the job blocks for re-auth.
        assert_eq!(
            choose_holdings_source(false, false),
            HoldingsChoice::NotConnected
        );
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

    #[test]
    fn persist_schwab_credentials_splits_the_stores_and_preserves_a_kept_secret() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        let store = schwab_secrets::InMemoryTokenStore::new();

        // First save: id → app_settings, secret → the Keychain rail.
        persist_schwab_credentials(&conn, &store, "  client-abc  ", Some("dev-secret")).unwrap();
        assert_eq!(
            storage::get_setting(&conn, config::KEY_SCHWAB_CLIENT_ID)
                .unwrap()
                .as_deref(),
            Some("client-abc") // trimmed, and it is not a secret
        );
        assert_eq!(
            store.get(schwab_secrets::SECRET_CLIENT_SECRET).unwrap().as_deref(),
            Some("dev-secret")
        );

        // Re-save to change only the id (secret field left empty): the stored secret
        // must survive, exactly as `settings::save` leaves an untouched API key in place.
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

        // Connect under client-abc: a live token set is stored.
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

        // Re-saving the SAME id while rotating the secret keeps the session — a refresh
        // token survives a secret change, so the tokens must NOT be cleared.
        persist_schwab_credentials(&conn, &store, "client-abc", Some("secret-2")).unwrap();
        assert!(
            store.tokens().unwrap().is_some(),
            "a secret rotation must not drop the session"
        );

        // Changing the client id makes the stored tokens stale → cleared, forcing a
        // reconnect. The secret itself is untouched by the id change.
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
