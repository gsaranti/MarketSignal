pub mod agent;
pub mod model_agent;
pub mod pipeline;
pub mod storage;

use tauri::Manager;

use model_agent::ModelMainAgent;
use pipeline::{generate_report, GeneratedReport, ReportPaths};

/// Manually generate a Weekly Market Report end to end. Builds the real main
/// agent from the environment, resolves the app data directory, runs the agent
/// through the pipeline, and returns the generated report for the frontend to
/// render. This runs unconditionally — the five-category execution gate is a
/// separate slice; a missing model or provider key surfaces as a plain error.
///
/// The agent build and pipeline run go through `spawn_blocking`: the agent uses
/// `reqwest::blocking`, which starts its own runtime and would panic if it ran
/// on the async runtime thread this command is dispatched on. The `MainAgent`
/// trait and pipeline stay synchronous; only this seam is async.
#[tauri::command]
async fn generate_report_manual(app: tauri::AppHandle) -> Result<GeneratedReport, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolving app data directory: {e}"))?;

    let paths = ReportPaths {
        db_path: data_dir.join("market_signal.db"),
        reports_dir: data_dir.join("reports"),
    };

    tauri::async_runtime::spawn_blocking(move || {
        let agent = ModelMainAgent::from_env().map_err(|e| e.to_string())?;
        generate_report(&agent, &paths).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("report generation task failed: {e}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![generate_report_manual])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
