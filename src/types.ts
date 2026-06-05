// Shared frontend types mirroring the Rust `GeneratedReport` / `ReportSummary`
// structs returned by the `generate_report_manual` command.

// Which main surface is showing. A plain string union driving a ref switch in
// App.vue (no router) — the app has a small, fixed set of destinations.
export type AppView = "report" | "inbox" | "archive" | "settings";

export interface ReportSummary {
  report_id: string;
  report_type: string;
  created_at: string;
  risk_posture: string;
  market_cycle: string;
  thesis_stance: string;
  header_summary_bullets: string[];
  key_risks: string[];
  unresolved_questions: string[];
  forward_outlook_themes: string[];
}

export interface GeneratedReport {
  report_id: string;
  markdown: string;
  markdown_path: string;
  summary: ReportSummary;
}

// Mirrors the Rust `config::ValidationReport` / `WarningCategory` returned by
// the `check_configuration` command and used to gate report generation.
export interface WarningCategory {
  kind: string;
  title: string;
  items: string[];
}

export interface ValidationReport {
  categories: WarningCategory[];
  is_blocked: boolean;
}

// Mirrors the Rust `research::ResearchDocument` returned by the
// `list_research_inbox` command (docs/research-documents.md). The inbox is a
// flat folder of user-supplied files; `supported` flags the formats the
// pipeline can parse. `modified` is a canonical UTC RFC3339 string (or null when
// the platform couldn't report one); the UI renders it in local time.
export interface ResearchDocument {
  name: string;
  format: string;
  supported: boolean;
  size_bytes: number;
  modified: string | null;
}

// Mirrors the Rust `jobs::JobStatus` returned by the `job_status` command
// (docs/scheduling.md §Job Status Visibility). Timestamps are canonical UTC
// RFC3339 strings; the UI renders them in local time.
export interface JobStatus {
  enabled: boolean;
  is_running: boolean;
  last_successful_at: string | null;
  last_failed_at: string | null;
  last_failure_detail: string | null;
  last_skipped_at: string | null;
}

// Mirrors the Rust `settings::*` structs (docs/configuration.md). The Settings
// view shows the four agent model selections and, per credential, only whether
// one is configured — the raw key never leaves the backend (settings.rs).

// One option in the model dropdown, sourced from the Rust `AgentModel` so slugs
// and display names have a single backend home.
export interface ModelOption {
  slug: string;
  label: string;
  provider: string; // "OpenAI" | "Anthropic" — used to group the dropdown
}

// The four agent slots' current model slugs ("" when unset). Round-trips: the
// form pre-selects these and submits them back to `save_settings`.
export interface AgentModels {
  main: string;
  bull: string;
  bear: string;
  balanced: string;
}

// Whether each credential is configured — never the value itself.
export interface CredentialStatus {
  openai: boolean;
  anthropic: boolean;
  fmp: boolean;
  tavily: boolean;
}

// The four testable credentials — the keys shared by CredentialStatus /
// CredentialUpdate and used to drive per-credential "Test connection" state.
export type CredentialKey = "openai" | "anthropic" | "fmp" | "tavily";

// Returned by `test_connection`: whether the saved credential was accepted by a
// single live authenticated request, plus a short message. Mirrors the Rust
// `connection_test::ConnectionTestResult`. Never carries the secret.
export interface ConnectionTestResult {
  ok: boolean;
  detail: string;
}

// Returned by `get_settings`.
export interface SettingsView {
  models: AgentModels;
  credentials: CredentialStatus;
  available_models: ModelOption[];
}

// The credential half of a `save_settings` submission. A field is set only when
// the user entered a new value; null/"" leaves the stored secret unchanged.
export interface CredentialUpdate {
  openai: string | null;
  anthropic: string | null;
  fmp: string | null;
  tavily: string | null;
}
