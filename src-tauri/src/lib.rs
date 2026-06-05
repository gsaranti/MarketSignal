pub mod agent;
pub mod config;
pub mod jobs;
pub mod model_agent;
pub mod pipeline;
pub mod research;
pub mod schedule;
pub mod settings;
pub mod storage;

use std::path::PathBuf;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_opener::OpenerExt;

use config::{AppConfig, ValidationReport};
use jobs::{run_job, JobOutcome, JobStatus, RunGuard};
use model_agent::ModelMainAgent;
use pipeline::{GeneratedReport, ReportPaths};

/// How long the scheduler sleeps between wake-ups while waiting for the next
/// window. Bounded (rather than one long sleep to the window) so a clock change
/// or a suspend/resume is re-evaluated within the hour instead of overshooting
/// silently.
const SCHEDULER_POLL_CHUNK: Duration = Duration::from_secs(60 * 60);

/// Report the current warning state for the Persistent Warning Area. Read-only:
/// it validates the config substrate (`docs/weekly-report-workflow.md §Step 1`)
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
        let enabled = jobs::weekly_job_enabled(conn).unwrap_or(true);
        if let Ok(Some(warning)) = jobs::missed_warning(conn, chrono::Local::now(), enabled) {
            report.categories.push(warning);
        }
    }
    report
}

/// The on-disk layout for a run — the SQLite database and the reports directory,
/// both under the app data directory. One source for the path layout, shared by
/// the manual command and the scheduler so they can never drift apart.
fn report_paths(app: &tauri::AppHandle) -> Result<ReportPaths, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolving app data directory: {e}"))?;
    Ok(ReportPaths {
        db_path: data_dir.join("market_signal.db"),
        reports_dir: data_dir.join("reports"),
    })
}

/// The research-inbox folder under the app data directory
/// (`docs/research-documents.md`). Resolved here alongside `report_paths` so the
/// whole app-data layout stays defined in one place.
fn research_inbox_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolving app data directory: {e}"))?;
    Ok(data_dir.join("research-inbox"))
}

/// The research-archive folder under the app data directory
/// (`docs/research-documents.md`). Successfully-processed inbox documents are
/// moved here; the user may delete from it but cannot manually archive into it.
fn research_archive_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolving app data directory: {e}"))?;
    Ok(data_dir.join("research-archive"))
}

/// Manually generate a Weekly Market Report end to end. The execution gate runs
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

    let paths = report_paths(&app)?;

    let guard = guard.inner().clone();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let agent = ModelMainAgent::new(main_config).map_err(|e| e.to_string())?;
        run_job(&agent, &paths, &guard).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("report generation task failed: {e}"))??;

    match outcome {
        JobOutcome::Successful(report) => Ok(*report),
        JobOutcome::Failed(msg) => Err(msg),
        JobOutcome::Skipped(reason) => Err(reason),
    }
}

/// Resolve the SQLite path and ensure the app data directory exists, so a
/// command that touches the database works even before the first report has been
/// generated (the pipeline creates the directory as a side effect, but the
/// status/settings commands can run first).
fn open_app_db(app: &tauri::AppHandle) -> Result<rusqlite::Connection, String> {
    let paths = report_paths(app)?;
    if let Some(parent) = paths.db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating app data directory: {e}"))?;
    }
    let conn = storage::open(&paths.db_path).map_err(|e| e.to_string())?;
    storage::init_schema(&conn).map_err(|e| e.to_string())?;
    Ok(conn)
}

/// Current job status for the UI's status panel (`docs/scheduling.md §Job Status
/// Visibility`): last successful run, last failure, last skipped event, whether
/// a run is in flight, and the enable flag.
#[tauri::command]
fn job_status(
    app: tauri::AppHandle,
    guard: tauri::State<'_, RunGuard>,
) -> Result<JobStatus, String> {
    let conn = open_app_db(&app)?;
    jobs::job_status(&conn, &guard).map_err(|e| e.to_string())
}

/// Persist the Weekly Market job's enable/disable flag (`docs/scheduling.md
/// §Job Controls`). The scheduler reads this each time a window fires, so a
/// disabled job no-ops rather than running.
#[tauri::command]
fn set_job_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let conn = open_app_db(&app)?;
    jobs::set_weekly_job_enabled(&conn, enabled).map_err(|e| e.to_string())
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

/// List the user-supplied documents currently in the research inbox
/// (`docs/research-documents.md`). A fresh install with no inbox folder yet lists
/// as empty rather than erroring; the frontend renders the empty state.
#[tauri::command]
fn list_research_inbox(app: tauri::AppHandle) -> Result<Vec<research::ResearchDocument>, String> {
    let inbox = research_inbox_dir(&app)?;
    research::list_folder(&inbox).map_err(|e| e.to_string())
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

/// Debug-only schedule override: when `MARKET_SIGNAL_SCHEDULE_OVERRIDE` is set to
/// a number of seconds, the scheduler fires on that fixed interval instead of the
/// weekly window, so a `tauri dev` smoke can exercise a scheduled run in seconds.
/// Compiled out of release builds entirely.
#[cfg(debug_assertions)]
fn schedule_override_secs() -> Option<u64> {
    std::env::var("MARKET_SIGNAL_SCHEDULE_OVERRIDE")
        .ok()
        .and_then(|v| v.parse().ok())
}

#[cfg(not(debug_assertions))]
fn schedule_override_secs() -> Option<u64> {
    None
}

/// The tokio timer that drives scheduled runs. Computes the next Sunday-9AM
/// local window, sleeps until it in bounded chunks, fires, and repeats. A window
/// the machine slept through is not replayed (`docs/scheduling.md §System Sleep
/// Behavior`): if wake-up overshoots the window by more than a short grace, the
/// run is skipped and surfaces as a missed-job warning on next check instead.
fn spawn_scheduler(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Debug fast-path: a fixed interval for smoke testing the fire path.
        if let Some(secs) = schedule_override_secs() {
            loop {
                tokio::time::sleep(Duration::from_secs(secs)).await;
                run_scheduled_once(&app).await;
            }
        }

        let grace = chrono::Duration::minutes(15);
        loop {
            let next = schedule::next_run_after(chrono::Local::now());
            // Sleep toward the window in bounded chunks.
            loop {
                let now = chrono::Local::now();
                if now >= next {
                    break;
                }
                let remaining = (next - now).to_std().unwrap_or(Duration::ZERO);
                tokio::time::sleep(remaining.min(SCHEDULER_POLL_CHUNK)).await;
            }
            // Fire only if we reached the window roughly on time. A large
            // overshoot means the machine slept past it — that window is missed,
            // not replayed; the loop advances to the next future window.
            if chrono::Local::now() - next <= grace {
                run_scheduled_once(&app).await;
            } else {
                // Overshot: the window is missed, not replayed — but nudge an
                // open window to refresh so the missed-job warning surfaces on
                // resume (`docs/scheduling.md §Missed Job Detection`), reusing the
                // same channel a finished run uses (no report to carry).
                let _ = app.emit("job-finished", Option::<GeneratedReport>::None);
            }
        }
    });
}

/// One scheduled fire: re-check the enable flag and the execution gate, then run
/// the identical workflow as the manual command (`docs/weekly-report-workflow.md
/// §Step 1`). A disabled job or a blocked configuration no-ops — a blocked run
/// records nothing, since its warnings already surface via `check_configuration`.
/// Concurrency with a manual run is handled inside `run_job` (→ Skipped). On any
/// terminal outcome a `job-finished` event lets an open window refresh.
async fn run_scheduled_once(app: &tauri::AppHandle) {
    let paths = match report_paths(app) {
        Ok(p) => p,
        Err(e) => return log_scheduler(e),
    };

    // Read the enable flag and load the config from one short-lived connection,
    // opened and dropped here before any await; `run_job` opens its own on the
    // blocking thread. Deliberately scoped this way: no `rusqlite::Connection`
    // (which is not `Send`) ever crosses an await point.
    let (enabled, cfg) = match open_app_db(app) {
        Ok(conn) => match jobs::weekly_job_enabled(&conn) {
            Ok(enabled) => (enabled, AppConfig::load(&conn)),
            Err(e) => return log_scheduler(format!("reading enable flag: {e}")),
        },
        Err(e) => return log_scheduler(format!("opening database: {e}")),
    };

    // The same pre-run decision the manual command makes, plus the enable flag,
    // as one pure step (`config::decide_scheduled_run`).
    let main_config = match config::decide_scheduled_run(&cfg, enabled) {
        config::ScheduledRun::Proceed(main_config) => main_config,
        config::ScheduledRun::Disabled => return, // expected, quiet no-op
        config::ScheduledRun::Blocked(reason) => return log_scheduler(reason),
    };

    let guard = app.state::<RunGuard>().inner().clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let agent = ModelMainAgent::new(main_config).map_err(|e| e.to_string())?;
        run_job(&agent, &paths, &guard).map_err(|e| e.to_string())
    })
    .await;

    // Carry the freshly generated report to an open window so its Latest Report
    // View updates without a manual refresh (`docs/weekly-report-workflow.md
    // §Step 17`); on failure/skip the payload is None and only the warning area
    // and status panel refresh. (The Recent Reports sidebar's historical list
    // still awaits the `list_reports` slice.)
    let report: Option<GeneratedReport> = match outcome {
        Ok(Ok(JobOutcome::Successful(report))) => Some(*report),
        Ok(Ok(JobOutcome::Failed(msg))) => {
            log_scheduler(format!("job failed: {msg}"));
            None
        }
        Ok(Ok(JobOutcome::Skipped(reason))) => {
            log_scheduler(format!("skipped: {reason}"));
            None
        }
        Ok(Err(e)) => {
            log_scheduler(e);
            None
        }
        Err(e) => return log_scheduler(format!("run task failed: {e}")),
    };
    let _ = app.emit("job-finished", report);
}

/// Scheduler diagnostics go to stderr — the scheduler runs headless, so there is
/// no UI surface to route them to beyond the warning area the next check rebuilds.
fn log_scheduler(message: String) {
    eprintln!("scheduler: {message}");
}

/// Reveal and focus every window. Shared by the tray "Show" menu item and the
/// macOS Dock-icon reopen handler — both undo the hide-to-tray performed on
/// window close. `set_focus` no-ops on a hidden or minimized window (and it
/// activates the app itself), so unminimize and show first, then focus.
fn restore_windows(app: &tauri::AppHandle) {
    for window in app.webview_windows().values() {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(RunGuard::default())
        .invoke_handler(tauri::generate_handler![
            generate_report_manual,
            check_configuration,
            job_status,
            set_job_enabled,
            get_settings,
            save_settings,
            list_research_inbox,
            delete_research_document,
            reveal_research_inbox,
            list_research_archive,
            delete_research_archive_document,
            reveal_research_archive
        ])
        .setup(|app| {
            // Tray runtime: the app stays resident so scheduled jobs keep running
            // when the window is closed (`docs/scheduling.md §Application Runtime
            // Requirements`).
            let show = MenuItem::with_id(app, "show", "Show Market Signal", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Market Signal", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().ok_or("missing default window icon")?)
                .tooltip("Market Signal")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => restore_windows(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            // Keep the TrayIcon alive for the app's lifetime: a dropped handle
            // severs menu-event delivery — the icon still draws, but clicks reach
            // no handler (tauri-apps/tauri#11462).
            app.manage(tray);

            // Start the Sunday-9AM-local timer.
            spawn_scheduler(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window hides it to the tray rather than quitting, so the
            // scheduler keeps running. Quitting is explicit (tray "Quit").
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS: clicking the Dock icon asks the app to reopen. When the
            // window has been hidden to the tray on close there are no visible
            // windows, so restore them — matching the tray "Show" menu item.
            // (Reopen is macOS-only; other platforms never emit it.)
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = event
            {
                restore_windows(app);
            }
        });
}
