//! Application-layer orchestration for a single manual report run.
//!
//! This is the spine the whole system is built on: the app layer drives the
//! agent stage (a pure function) and owns every side effect — the database
//! write and the canonical Markdown file. It is written free of any Tauri
//! runtime so it can be driven directly by an integration test against stubs.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::agent::{MainAgent, MainAgentInput, ReportSummary};
use crate::storage::{self, ReportRecord};

/// Filesystem locations a run reads and writes. Injected so tests can point at
/// temporary directories; the Tauri command resolves these from the app data
/// directory.
pub struct ReportPaths {
    pub db_path: PathBuf,
    pub reports_dir: PathBuf,
}

/// The result of a report run, returned to the caller (the Tauri command or a
/// test). Carries the Markdown so the frontend can render it immediately.
#[derive(Debug, Serialize)]
pub struct GeneratedReport {
    pub report_id: String,
    pub markdown: String,
    pub markdown_path: String,
    pub summary: ReportSummary,
}

/// Run one manual report end to end: invoke the agent, write the canonical
/// Markdown file, and persist the record to SQLite.
pub fn generate_report(agent: &dyn MainAgent, paths: &ReportPaths) -> Result<GeneratedReport> {
    let output = agent.generate(MainAgentInput)?;
    let summary = output.summary;

    std::fs::create_dir_all(&paths.reports_dir)
        .with_context(|| format!("creating reports directory {:?}", paths.reports_dir))?;

    // Canonical filename: YYYY-MM-DD-market-signal-weekly-report.md. Parse the
    // agent-supplied `created_at` as RFC3339 so a malformed timestamp surfaces
    // as a typed error here rather than panicking on a byte slice.
    let date = chrono::DateTime::parse_from_rfc3339(&summary.created_at)
        .with_context(|| {
            format!(
                "agent supplied a non-RFC3339 created_at: {:?}",
                summary.created_at
            )
        })?
        .format("%Y-%m-%d");
    let filename = format!("{date}-market-signal-weekly-report.md");
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

    Ok(GeneratedReport {
        report_id: summary.report_id.clone(),
        markdown: output.markdown,
        markdown_path: markdown_path_str,
        summary,
    })
}
