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
// `parse_error` is the last job pass's parse-failure reason, set only while the
// file on disk is still the one that failed (§Parse Failures — the row renders
// in an error state so the user can fix or delete it); always null for the
// archive listing.
export interface ResearchDocument {
  name: string;
  format: string;
  supported: boolean;
  size_bytes: number;
  modified: string | null;
  parse_error: string | null;
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
  last_cancelled_at: string | null;
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
  fred: boolean;
  tavily: boolean;
}

// The five testable credentials — the keys shared by CredentialStatus /
// CredentialUpdate and used to drive per-credential "Test connection" state.
export type CredentialKey = "openai" | "anthropic" | "fmp" | "fred" | "tavily";

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

// Mirrors the Rust `storage::TruncationStats` returned by the `truncation_stats`
// command — aggregate telemetry for how often the Step-6 inbox parser had to
// head-truncate an oversized document, accumulated across reports
// (docs/agents.md §Data Extraction). Absolute counts only: the underlying table
// records only truncated docs, so a true share-of-all-documents rate is not
// derivable. An all-zero aggregate (empty table) is the "overflow is rare"
// signal the Settings diagnostics section renders as its empty state.
export interface TruncationStats {
  total_truncations: number;
  reports_affected: number;
  total_chars_dropped: number;
  by_format: FormatCount[];
  latest_captured_at: string | null;
}

// One row of the per-format breakdown in TruncationStats.
export interface FormatCount {
  format: string;
  count: number;
}

// The credential half of a `save_settings` submission. A field is set only when
// the user entered a new value; null/"" leaves the stored secret unchanged.
export interface CredentialUpdate {
  openai: string | null;
  anthropic: string | null;
  fmp: string | null;
  fred: string | null;
  tavily: string | null;
}

// --- Live job tracker -------------------------------------------------------
// Mirrors the Rust `progress::ProgressMessage` streamed over the "job-progress"
// Tauri event while a run is in flight. Discriminated by `kind`; every message
// also carries `run_id` (to discard stragglers from a prior run) and a monotonic
// `seq`. Fields beyond those two are present only on the variants that use them.
export type ProgressKind =
  | "run-started"
  | "step-started"
  | "step-finished"
  | "request-started"
  | "request-finished"
  | "agent-token"
  | "run-finished";

export interface ProgressMessage {
  run_id: string;
  seq: number;
  kind: ProgressKind;
  // run-started: a short human title for the run.
  label?: string;
  // step-started / step-finished: the stable step key + its human label.
  step?: string;
  // step-finished ("ok" | "failed" | "cancelled"), request-finished ("ok" or a
  // gap reason), run-finished ("successful" | "failed" | "cancelled").
  status?: string;
  detail?: string | null;
  // request-finished: one baseline series' provider / group / id / name.
  provider?: string;
  group?: string;
  series_id?: string;
  name?: string;
  // agent-token: a coalesced chunk of the streamed report text.
  delta?: string;
  // run-finished: the new report's id, on success only.
  report_id?: string | null;
}

// One baseline data request, as shown in the tracker (one row per actual HTTP
// call). `status` is "running" while in-flight, then "ok", "empty" (a 2xx with no
// usable data), or a gap reason (unavailable / rejected / malformed / out-of-scope).
export interface TrackerRequest {
  provider: string;
  group: string;
  seriesId: string;
  name: string;
  status: string;
  detail: string | null;
}

export type StepStatus = "pending" | "running" | "ok" | "failed" | "cancelled";

// One pipeline step in the tracker. `requests` carries the baseline step's
// per-series rows; `agentText` accumulates the main-agent step's streamed report.
export interface TrackerStep {
  key: string;
  label: string;
  status: StepStatus;
  detail: string | null;
  requests: TrackerRequest[];
  agentText: string;
}

// The assembled trace for one run, built in App.vue from the event stream and
// rendered by JobTrackerView. `terminal` is null until the run finishes; it then
// carries the outcome so the trace can linger (reopenable) after the run ends.
export interface RunTrace {
  runId: string;
  label: string;
  steps: TrackerStep[];
  terminal: { status: string; detail: string | null } | null;
}
