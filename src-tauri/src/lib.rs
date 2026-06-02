pub mod agent;
pub mod pipeline;
pub mod storage;

use tauri::Manager;

use agent::StubMainAgent;
use pipeline::{generate_report, GeneratedReport, ReportPaths};

/// Manually generate a Weekly Market Report end to end. Resolves the app data
/// directory, runs the (currently stubbed) main agent through the pipeline, and
/// returns the generated report for the frontend to render.
#[tauri::command]
fn generate_report_manual(app: tauri::AppHandle) -> Result<GeneratedReport, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolving app data directory: {e}"))?;

    let paths = ReportPaths {
        db_path: data_dir.join("market_signal.db"),
        reports_dir: data_dir.join("reports"),
    };

    generate_report(&StubMainAgent, &paths).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![generate_report_manual])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
