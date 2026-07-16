// Shared frontend types mirroring the Rust `GeneratedReport` / `ReportSummary`
// structs returned by the `generate_report_manual` command.

// Which main surface is showing. A plain string union driving a ref switch in
// App.vue (no router) — the app has a small, fixed set of destinations.
export type AppView = "report" | "portfolio" | "inbox" | "archive" | "settings";

export interface ReportSummary {
  report_id: string;
  report_type: string;
  created_at: string;
  // The agent-written per-issue headline, shown as the report's label in the
  // sidebar. Empty for reports persisted before this field existed; the UI falls
  // back to the product name "Market Signal Report" in that case.
  title: string;
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
  // The identity of the shown warning, echoed back to `dismiss_warning` so the
  // dismissal targets this row and not a newer one the backend would re-derive.
  // Present only for the non-blocking (dismissible) failed-jobs category; null otherwise.
  dismiss_id: string | null;
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
  is_running: boolean;
  // Which workflow holds the single run slot while is_running — drives the
  // footer's running label (a Schwab connect must not read as a report run).
  running_kind:
    | "report"
    | "portfolio"
    | "schwab-connect"
    | "holdings-pull"
    | "data-portability"
    | null;
  last_successful_at: string | null;
  last_failed_at: string | null;
  last_failure_detail: string | null;
  last_skipped_at: string | null;
  last_cancelled_at: string | null;
}

// --- Data portability (docs/data-portability.md) -----------------------------
// Mirrors the Rust `portability::*` results returned by `export_data`,
// `import_data_inspect`, and `import_data`.

// What an export wrote (`export_data` returns null on a cancelled Save dialog).
export interface ExportSummary {
  // Where the archive landed — surfaced with the counts.
  path: string;
  reports: number;
  learnings: number;
  snapshots: number;
  portfolio_runs: number;
  holdings_pulls: number;
  files: number;
  encrypted: boolean;
}

// What an import loaded. `skipped_reports` counts report records whose Markdown
// body was missing from the archive (skipped, never imported as shells).
export interface ImportSummary {
  reports: number;
  learnings: number;
  snapshots: number;
  portfolio_runs: number;
  holdings_pulls: number;
  files: number;
  skipped_reports: number;
}

// The pre-import peek of a picked archive's manifest.
export interface ArchiveInfo {
  encrypted: boolean;
  format_version: number;
  app_version: string;
  created_at: string;
  reports: number;
  learnings: number;
  snapshots: number;
  portfolio_runs: number;
  holdings_pulls: number;
  files: number;
}

// `import_data_inspect`'s result (null on a cancelled Open dialog): the picked
// path, whether the target store is empty (empty → straight load; non-empty →
// the replace-all confirmation), and the archive's manifest read.
export interface ImportInspection {
  path: string;
  store_empty: boolean;
  info: ArchiveInfo;
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

// The local-analysis-models values (docs/configuration.md §Local Models):
// daemon endpoint + roster ids. Not secrets, so unlike credentials they
// round-trip in full ("" when unset) and the form submits all four verbatim to
// `save_local_model_settings`. Reasoner + embedder are the presence-gated pair;
// the fast tier is optional and never gates.
export interface LocalModelSettings {
  daemon_endpoint: string;
  reasoner_model: string;
  fast_model: string;
  embedder_model: string;
}

// Returned by `get_settings`.
export interface SettingsView {
  models: AgentModels;
  credentials: CredentialStatus;
  local_models: LocalModelSettings;
  available_models: ModelOption[];
}

// Returned by `test_local_daemon` (the local suite's parallel to
// ConnectionTestResult): endpoint reachability plus any configured roster ids
// the daemon doesn't have pulled — "daemon up but the model isn't pulled" is a
// distinct state (docs/interface.md §Connection status).
export interface LocalDaemonStatus {
  reachable: boolean;
  detail: string | null;
  missing_models: string[];
}

// Mirrors the Rust `storage::TruncationStats` returned by the `truncation_stats`
// command — aggregate telemetry for how often the Step-6 inbox parser had to
// head-truncate an oversized document, accumulated across reports
// (docs/agents.md §Data Extraction). Two rates are derivable: `total_truncations`
// over `total_docs_parsed` (share of documents truncated), and `total_chars_dropped`
// over `total_original_chars` (share of ingested text cut). An all-zero aggregate
// (empty table) is the "overflow is rare" signal the Settings diagnostics section
// renders as its empty state.
export interface TruncationStats {
  total_truncations: number;
  // Documents parsed across all recorded runs — the doc-rate denominator. 0 before
  // any run with a parsed document has been recorded.
  total_docs_parsed: number;
  // Truncations whose report has no parse-run denominator (typically recorded
  // before the denominator existed). > 0 means the rate would mix cohorts, so
  // the readout withholds it; 0 once every truncation report has a denominator.
  unaligned_truncations: number;
  // Total original (pre-truncation) chars across all parse runs — the chars-rate
  // denominator. 0 before any run with a char count has been recorded.
  total_original_chars: number;
  // Parse-run rows with no recorded char count (the pre-migration cohort). > 0
  // means the chars denominator omits some rows whose truncations the numerator
  // may still count, so the chars ratio withholds; 0 once every row has a count.
  parse_runs_missing_original_chars: number;
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

// The API-token half of a `save_settings` submission (the token-gated cloud
// save). A field is set only when the user entered a new value; null/"" leaves
// the stored secret unchanged. The FMP/FRED/Tavily provider credentials save
// separately through `save_provider_credentials`, outside the token gate
// (docs/configuration.md §API Tokens).
export interface CredentialUpdate {
  openai: string | null;
  anthropic: string | null;
}

// The provider credentials' own `save_provider_credentials` submission —
// ungated by the cloud tokens, so a cloud-keyless machine persists FMP/FRED for
// the local suite. Same null/"" = leave-unchanged semantics.
export interface ProviderCredentialUpdate {
  fmp: string | null;
  fred: string | null;
  tavily: string | null;
}

// --- Charles Schwab connection ---------------------------------------------
// Mirrors the Rust `schwab_oauth::{SchwabConnection, SchwabStatus}` returned by
// the `schwab_status` command (docs/schwab-integration.md, docs/interface.md
// §Connection status). Kept a parallel shape rather than folded into the closed
// credential machinery above: the client_id is a non-secret identifier that
// round-trips its value, the client secret rides the Keychain, and connection
// state is a third axis the CredentialStatus boolean can't carry.

// The connection state derived from the stored token set without a network probe:
// never linked, a live connection, or a lapsed 7-day refresh window.
export type SchwabConnection = "not-connected" | "connected" | "expired";

export interface SchwabStatus {
  // The developer-app client id — a non-secret identifier, so it round-trips its
  // actual value (unlike the secret-only credentials, shown as a boolean).
  client_id: string;
  // Whether the client secret is present on the Keychain rail — never its value.
  secret_configured: boolean;
  connection: SchwabConnection;
  // Canonical UTC RFC3339 string (or null when not connected); the UI renders it
  // in local time for the weekly-re-login heads-up.
  refresh_expires_at: string | null;
}

// The payload the "Charles Schwab connection" surface emits on save. The client_id
// round-trips in full; client_secret is set only when a new value is entered
// (null leaves the stored secret unchanged, like a CredentialUpdate field).
export interface SchwabCredentialUpdate {
  client_id: string;
  client_secret: string | null;
}

// --- Portfolio Analysis ------------------------------------------------------
// Mirrors the Rust `portfolio::*` / `schwab::*` structs returned by the
// `latest_portfolio_run`, `generate_portfolio_manual`, `pull_holdings`, and
// `latest_holdings_pull` commands (docs/portfolio-analysis.md §Storage and
// display, §Triggering). Enum wire shapes are kebab-case (pinned backend-side).

export type AssetClass =
  | "stock"
  | "etf"
  | "mutual-fund"
  | "option-contract"
  | "fixed-income"
  | "cash"
  | "other";

// One position in the account. Cost basis and market value are account-currency
// totals (not per-share), Schwab-reported — the sort bar's engine-computed keys
// derive from these two, never a naive quote × shares.
export interface Position {
  symbol: string;
  description: string;
  asset_class: AssetClass;
  quantity: number;
  cost_basis: number;
  market_value: number;
  current_price: number | null;
}

export interface Holdings {
  positions: Position[];
  cash: number;
  account_total: number;
}

// The latest standalone Pull-holdings snapshot — view-only page state, distinct
// from the snapshot persisted inside each run (which is the diff baseline).
// `pulled_at` is canonical UTC RFC3339; the UI renders local time.
export interface HoldingsPull {
  pulled_at: string;
  holdings: Holdings;
}

// How a position changed vs the prior run's snapshot — the app's deterministic
// quantity diff, never re-derived in the frontend.
export type PositionChange = "new" | "increased" | "decreased" | "unchanged";

export type PortfolioGrade = "A" | "B" | "C" | "D" | "F";
export type PortfolioAction =
  | "sell-all"
  | "trim"
  | "hold"
  | "add"
  | "add-aggressively";
export type PortfolioConviction = "high" | "medium" | "low";
export type HorizonRead = "bullish" | "neutral" | "bearish";

// The four engine-computed sub-scores, 0–100, higher is better (risk inverted at
// source: safer scores higher).
export interface SubScores {
  quality: number;
  valuation: number;
  momentum: number;
  risk: number;
}

export interface HorizonOutlook {
  short: HorizonRead;
  mid: HorizonRead;
  long: HorizonRead;
}

// One scenario target with its methodology exposed; the engine computed the
// figures, the model selected and justified the base case.
export interface PriceTarget {
  base: number;
  bear: number;
  bull: number;
  methodology: string;
}

// Rolling one-month / twelve-month windows from the run date (the settled rename
// of end-of-month / end-of-year — docs/portfolio-analysis.md §Starting parameters).
// The backend decodes legacy field names through serde aliases and always emits
// these.
export interface PriceTargets {
  one_month: PriceTarget | null;
  twelve_month: PriceTarget | null;
}

// The deterministic per-branch risk tier and the three-state capital-efficiency
// (dead-money) read — engine-computed, absent on runs persisted before the fields.
export type RiskTier = "low" | "medium" | "high";
export type HurdleState = "clears" | "indeterminate" | "fails" | "unscorable";

// The per-stock options-activity signal — an activity proxy, not positioning
// truth; any field null when the chain lacked the data.
export interface OptionsSignal {
  put_call_volume: number | null;
  put_call_open_interest: number | null;
  implied_volatility: number | null;
  iv_skew: number | null;
}

export interface ActionSizing {
  target_weight_low: number;
  target_weight_high: number;
  est_share_delta: number | null;
  est_dollar_delta: number | null;
}

export interface GradedVerdict {
  grade: PortfolioGrade;
  sub_scores: SubScores;
  action: PortfolioAction;
  action_sizing: ActionSizing;
  conviction: PortfolioConviction;
  horizon_outlook: HorizonOutlook;
  price_targets: PriceTargets;
  price_target_rationale: string;
  options_signal: OptionsSignal;
  // Engine reads added by the fund slice — null/false on pre-field runs.
  risk_tier: RiskTier | null;
  dead_money: HurdleState | null;
  // True when the letter rests on an imputed (neutral-50) sub-score — rendered as
  // the visible low-confidence marker beside the letter (every priced fund, per
  // the fund-grade contract).
  low_confidence_grade: boolean;
  // The fund path's deterministic strategy classification, shown on the card
  // (docs/portfolio-analysis.md §Asset eligibility) — null for a stock and on
  // pre-field runs; the structural flag marks an option-overlay fund on the
  // priced branch (leveraged/inverse routes to role_risk_only instead).
  fund_class_label: string | null;
  structural_flag: boolean;
  financial_summary: string;
  what_changed: string;
}

// One exposure weight (a sector or country label and its fraction of the fund).
export interface ExposureWeight {
  label: string;
  weight: number;
}

// The role_risk_only branch of an analyzed verdict: a structurally unpriceable
// vehicle class — no letter, no targets, no conviction; role + risk + the reduced
// action spine (docs/portfolio-analysis.md §Intrinsic verdict).
export interface RoleRiskVerdict {
  class_label: string;
  role_summary: string;
  exposure_tilt: ExposureWeight[];
  expense_drag: number | null;
  observable_risk: number | null;
  structural_flag: boolean;
  evidence_gaps: string[];
  action: PortfolioAction;
  action_sizing: ActionSizing;
  what_changed: string;
}

// Internally tagged on `status` (serde `tag = "status"`): the analyzed verdict is
// a two-branch union — `priced` (the full record; legacy `graded` rows re-serialize
// as this) and `role-risk-only` — beside the two abstention arms.
export type VerdictDisposition =
  | ({ status: "priced" } & GradedVerdict)
  | ({ status: "role-risk-only" } & RoleRiskVerdict)
  | { status: "not-rated"; reason: string }
  | { status: "insufficient-evidence"; reason: string };

export interface HoldingVerdict {
  symbol: string;
  asset_class: AssetClass;
  position_change: PositionChange;
  disposition: VerdictDisposition;
}

// A position present last run but absent now — surfaced in the roll-up only,
// never a card in the sortable stack.
export interface ExitedPosition {
  symbol: string;
  description: string;
  prior_quantity: number;
  prior_cost_basis: number;
  prior_market_value: number;
}

export interface PortfolioRollUp {
  graded_count: number;
  not_rated_count: number;
  insufficient_evidence_count: number;
  // Analyzed holdings on the role_risk_only branch — counted beside the priced
  // holdings, never pooled with them.
  role_risk_only_count: number;
  top_position_weight: number;
  cash_weight: number;
  exited: ExitedPosition[];
  overview: string;
}

export interface PortfolioRun {
  run_id: string;
  created_at: string;
  holdings: Holdings;
  verdicts: HoldingVerdict[];
  roll_up: PortfolioRollUp;
  // The per-holding audit records (sources, metrics, model ids…) — persisted
  // for traceability; not rendered by the Portfolio page in this slice.
  audit: unknown[];
}

// One sidebar row of the Portfolio-runs history, returned by
// `list_portfolio_runs` (docs/interface.md §Main Layout — the shared-history
// sidebar's run list). Light by design: identity, timestamp, and the two counts
// the row renders; opening a run fetches the full record via
// `get_portfolio_run`.
export interface PortfolioRunSummary {
  run_id: string;
  // Canonical UTC RFC3339; rendered in local time.
  created_at: string;
  holdings_count: number;
  graded_count: number;
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
  | "agent-thinking"
  | "analyst-thinking"
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
  // agent-token / agent-thinking / analyst-thinking: a coalesced chunk of the streamed
  // report text, the main agent's reasoning, or one analyst's reasoning, respectively.
  delta?: string;
  // analyst-thinking: which analyst the reasoning chunk belongs to (bull / bear /
  // balanced), so the tracker routes the three concurrent analysts to distinct panes.
  posture?: string;
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
// per-series rows; `agentText` accumulates the main-agent step's streamed report;
// `agentThinking` accumulates its streamed reasoning (extended-thinking summary),
// shown as a quieter stream above the report. Empty for non-thinking models.
// `analystThinking` maps each analyst posture (bull / bear / balanced) to its streamed
// reasoning, accumulated under the "analysts" step — one pane per analyst that surfaces
// thinking; empty for non-thinking analyst models.
export interface TrackerStep {
  key: string;
  label: string;
  status: StepStatus;
  detail: string | null;
  requests: TrackerRequest[];
  agentText: string;
  agentThinking: string;
  analystThinking: Record<string, string>;
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
