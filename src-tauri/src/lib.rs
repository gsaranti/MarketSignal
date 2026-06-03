pub mod agent;
pub mod config;
pub mod jobs;
pub mod model_agent;
pub mod pipeline;
pub mod storage;

use tauri::Manager;

use config::{AppConfig, ValidationReport};
use jobs::{run_job, JobOutcome, RunGuard};
use model_agent::ModelMainAgent;
use pipeline::{GeneratedReport, ReportPaths};

/// Report the current warning state for the Persistent Warning Area. Read-only:
/// it validates the config substrate (`docs/weekly-report-workflow.md §Step 1`)
/// and merges in the non-blocking `FailedJob` warning from job history
/// (`docs/scheduling.md §Error Handling`), but runs no job. The frontend calls
/// this on load and after a generate attempt to repopulate the warning area, so
/// a run that just failed surfaces here. The job-history merge is best-effort:
/// if the database can't be read, the authoritative config warnings still show.
#[tauri::command]
fn check_configuration(app: tauri::AppHandle) -> ValidationReport {
    let mut report = config::validate(&AppConfig::from_env());
    if let Ok(data_dir) = app.path().app_data_dir() {
        let db_path = data_dir.join("market_signal.db");
        if db_path.exists() {
            if let Ok(conn) = storage::open(&db_path) {
                if let Ok(Some(warning)) = jobs::failure_warning(&conn) {
                    report.categories.push(warning);
                }
            }
        }
    }
    report
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
    // Execution gate: refuse a blocked run before doing any work.
    let cfg = AppConfig::from_env();
    let report = config::validate(&cfg);
    if report.is_blocked {
        return Err(config::blocked_summary(&report));
    }
    let main_config = cfg.main_agent_config().map_err(|e| e.to_string())?;

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolving app data directory: {e}"))?;

    let paths = ReportPaths {
        db_path: data_dir.join("market_signal.db"),
        reports_dir: data_dir.join("reports"),
    };

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(RunGuard::default())
        .invoke_handler(tauri::generate_handler![
            generate_report_manual,
            check_configuration
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
