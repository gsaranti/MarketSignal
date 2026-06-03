pub mod agent;
pub mod config;
pub mod model_agent;
pub mod pipeline;
pub mod storage;

use tauri::Manager;

use config::{AppConfig, ValidationReport};
use model_agent::ModelMainAgent;
use pipeline::{generate_report, GeneratedReport, ReportPaths};

/// Report the current configuration state for the Persistent Warning Area. A
/// read-only pre-run check (`docs/weekly-report-workflow.md §Step 1`): it reads
/// the config substrate and validates it, but runs no job. The frontend calls
/// this on load and after a generate attempt to populate the warning area.
#[tauri::command]
fn check_configuration() -> ValidationReport {
    config::validate(&AppConfig::from_env())
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
#[tauri::command]
async fn generate_report_manual(app: tauri::AppHandle) -> Result<GeneratedReport, String> {
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

    tauri::async_runtime::spawn_blocking(move || {
        let agent = ModelMainAgent::new(main_config).map_err(|e| e.to_string())?;
        generate_report(&agent, &paths).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("report generation task failed: {e}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            generate_report_manual,
            check_configuration
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
