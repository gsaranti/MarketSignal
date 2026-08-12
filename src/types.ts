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

// One section's "last X" stamps — mirrors the Rust `jobs::SectionStamps`
// (docs/scheduling.md §Job Status Visibility). Timestamps are canonical UTC
// RFC3339 strings; the UI renders them in local time.
export interface SectionStamps {
  last_successful_at: string | null;
  last_failed_at: string | null;
  last_failure_detail: string | null;
  last_skipped_at: string | null;
  last_cancelled_at: string | null;
}

// Mirrors the Rust `jobs::JobStatus` returned by the `job_status` command
// (docs/scheduling.md §Job Status Visibility). The run-slot fields are global
// (one shared slot); the stamps come per section, keyed by job_type, and the
// footer renders the active section's group.
export interface JobStatus {
  is_running: boolean;
  // Which workflow holds the single run slot while is_running — drives the
  // footer's running label (a Schwab connect must not read as a report run).
  running_kind:
    | "report"
    | "portfolio"
    | "portfolio-quick-check"
    | "schwab-connect"
    | "holdings-pull"
    | "data-portability"
    | null;
  report: SectionStamps;
  portfolio: SectionStamps;
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

// Mirrors the Rust `portfolio::InvestorProfileDisplay` returned by the
// `get_investor_profile` command — the fixed investor-profile preset as
// ready-to-render read-only Settings rows (docs/configuration.md §Investor
// Profile). The strings are composed backend-side from the same label source
// the Step-7b construction prompt renders, so the two surfaces cannot drift.
export interface InvestorProfileDisplay {
  objective: string;
  risk_tolerance: string;
  horizon: string;
  tax: string;
  cash: string;
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
  // The construction call's validated sizing rationale (the card's action
  // rationale line) — absent on pre-construction runs and engine-band sizing.
  sizing_rationale?: string | null;
}

// The action half of the what-changed audit, authored and app-validated at the
// 7b construction stage (docs/portfolio-analysis.md §What changed): a changed
// action traces to a moved intrinsic verdict or a moved portfolio context —
// null when the action did not change, and on pre-construction runs.
export interface ActionWhatChanged {
  attribution: "moved-intrinsic" | "moved-context";
  cause?: "became-oversized" | "overlap-emerged" | "cash-freed" | null;
  note: string;
}

// One model-authored target band (the model arm's counterpart of PriceTarget) —
// authored freely at interpretation, persisted exactly as returned; the render
// annotates an inverted bear/bull pair rather than reordering it.
export interface ModelPriceTarget {
  base: number;
  bear: number;
  bull: number;
}

export interface ModelPriceTargets {
  one_month: ModelPriceTarget;
  twelve_month: ModelPriceTarget;
}

// The model arm of the two-arm verdict (docs/portfolio-analysis.md §The holding
// verdict): the model's own sub-scores, its letter derived through the shared
// cutoffs, freely-authored targets, and the retrospective self-assessment.
// Beside it, the verdict's conviction / horizon_outlook / lean complete the arm.
// Absent on runs persisted before portfolio-v7.
export interface ModelView {
  sub_scores: SubScores;
  letter: PortfolioGrade;
  price_targets: ModelPriceTargets;
  self_assessment: string;
}

// The engine's mechanical stand-in arm — deterministic outlook / conviction /
// action baselines so every model-authored field has a scored engine counterpart.
// Absent on pre-v7 runs.
export interface EngineView {
  outlook: HorizonOutlook;
  conviction: PortfolioConviction;
  action: PortfolioAction;
  action_sizing: ActionSizing;
}

export interface GradedVerdict {
  grade: PortfolioGrade;
  sub_scores: SubScores;
  // The final portfolio action, set at construction with the whole book in view.
  action: PortfolioAction;
  // The standalone action lean — what the action would be if the holding stood
  // alone, authored at interpretation before portfolio constraints. Absent on
  // pre-construction runs (reads as equal to the action). A lean differing from
  // the action is the visible intrinsic-vs-portfolio split.
  lean?: PortfolioAction | null;
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
  // The intrinsic half of the what-changed audit (authored at interpretation).
  what_changed: string;
  // The action half, authored at construction — absent when unchanged and on
  // pre-construction runs.
  action_what_changed?: ActionWhatChanged | null;
  // The two arms (portfolio-v7) — both absent on earlier runs, which render the
  // legacy single-arm card.
  model_view?: ModelView | null;
  engine_view?: EngineView | null;
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
  // Set wholly at construction — the reduced {sell-all, trim, hold} set is the
  // engine arm's there, the model's choice structurally open (departures
  // annotated); this branch carries no standalone lean.
  action: PortfolioAction;
  action_sizing: ActionSizing;
  what_changed: string;
  // The action half of the what-changed audit — same contract as the priced branch.
  action_what_changed?: ActionWhatChanged | null;
}

// Internally tagged on `status` (serde `tag = "status"`): the analyzed verdict is
// a two-branch union — `priced` (the full record; legacy `graded` rows re-serialize
// as this) and `role-risk-only` — beside the two abstention arms.
export type VerdictDisposition =
  | ({ status: "priced" } & GradedVerdict)
  | ({ status: "role-risk-only" } & RoleRiskVerdict)
  | { status: "not-rated"; reason: string }
  | { status: "insufficient-evidence"; reason: string };

// One bear/base/bull monitor scenario of the thesis ledger: the model's defining
// conditions and probability lean; the engine's own scenario price target is
// app-stamped (null on the condition-only role-risk-only branch).
export interface MonitorScenario {
  scenario: "bear" | "base" | "bull";
  conditions: string;
  probability_pct: number;
  engine_target: number | null;
}

// The persisted per-holding thesis ledger (docs/portfolio-analysis.md §The
// position thesis ledger). This slice renders the standing thesis as the card's
// anchor; the conditions' machine detail stays untyped until a display slice
// needs it.
export interface ThesisLedger {
  branch: "priced" | "role-risk-only";
  original_thesis: string;
  current_thesis: string;
  key_drivers: { name: string; series: string | null }[];
  monitor: MonitorScenario[];
  what_must_improve: string;
  what_must_not_break: string;
  conditions: unknown[];
  target_weight_low: number;
  target_weight_high: number;
  // Spot's relationship to the monitor band at authoring — app-stamped; absent
  // on pre-stamp ledgers and wherever no band exists (role-risk-only, no spot).
  authored_band_relation?: "inside" | "below-band" | "above-band" | null;
}

// How a verdict's action came to be — the canonical vocabulary from
// docs/portfolio-analysis.md §Outcome learning: model-chosen (a model pass
// actually chose it — the default) or rule-demoted (an over-age carried
// add-family action demoted to hold at the roll-up — §Triggering).
export type ActionSource = "model-chosen" | "rule-demoted";

export interface HoldingVerdict {
  symbol: string;
  asset_class: AssetClass;
  position_change: PositionChange;
  disposition: VerdictDisposition;
  // The holding's thesis ledger — the card's "why we hold this view" anchor.
  // Absent on not-rated positions and on runs persisted before the ledger.
  thesis_ledger?: ThesisLedger | null;
  // The analysis vintage (UTC RFC3339) of the full pass that produced this
  // verdict — differs from the run's created_at on a verdict a selective run
  // carried forward; absent on runs persisted before the field (their vintage
  // is the run's created_at).
  analyzed_at?: string | null;
  // Absent on pre-field runs (reads as model-chosen).
  action_source?: ActionSource | null;
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

// The run-level data-health aggregate: how the target surface was actually
// sourced (rate-anchored vs the fallbacks) plus the deep-history and DGS10
// degradations — computed deterministically from the persisted audits, so a
// degraded-but-successful run is visible at a glance. `attention` marks
// infrastructure degradation (unrecovered deep-history failures, any
// current-multiple carry, a run-wide DGS10 history gap).
export interface DataHealth {
  targets_total: number;
  rate_anchored_count: number;
  raw_percentile_count: number;
  current_multiple_carry_count: number;
  dispersion_floor_count: number;
  deep_history_failures: number;
  deep_history_fallbacks: number;
  dgs10_history_gap: boolean;
  // The house view was omitted for staleness (latest report older than the pinned
  // window — docs/portfolio-workflow.md §Step 5); informational, rides `summary`.
  house_view_omitted: boolean;
  // Local chat calls under context pressure (the digest-compression covenant's
  // detection leg) — near-full (≥ 90% of num_ctx) or likely front-truncated (a
  // reported count too small to cover the chars actually sent; Ollama's count
  // is post-truncation and lands far below num_ctx, so fill alone can't see
  // it). Named in `summary` and an attention trigger; `peak_prompt` is the
  // run's fullest prompt regardless, the big-run prompt-fit watch's measurement.
  context_pressure: PromptUsage[];
  peak_prompt: PromptUsage | null;
  attention: boolean;
  summary: string;
}

// One local chat call's prompt-size observation: Ollama's reported prompt token
// count (post-truncation) against the num_ctx the request declared and the
// prompt size the app actually sent.
export interface PromptUsage {
  stage: string;
  prompt_tokens: number;
  num_ctx: number;
  prompt_chars: number;
}

// One holding's action-sizing spine row (docs/portfolio-workflow.md §Step 7a) —
// the engine-known decision surface the construction call read (engine sets +
// annotation bounds since portfolio-v7), persisted with the roll-up for
// auditability. Not rendered by the Portfolio page in this slice.
export interface SizingSpineRow {
  symbol: string;
  asset_class: AssetClass;
  branch: "priced" | "role-risk";
  current_weight: number;
  market_value: number;
  current_price: number | null;
  concentration_headroom: number;
  upside_downside: number | null;
  dead_money: HurdleState | null;
  unrealized_pl: number | null;
  risk_tier: RiskTier | null;
  grade: PortfolioGrade | null;
  conviction: PortfolioConviction | null;
  lean: PortfolioAction | null;
  prior_lean: PortfolioAction | null;
  prior_action: PortfolioAction | null;
  position_change: PositionChange;
  carried: boolean;
  over_age: boolean;
  rule_demoted: boolean;
  pre_profit_rule: string | null;
  // Dormant wiring — structurally false until a forensic event producer lands.
  hard_forensic_bar: boolean;
  sector: string | null;
  // The engine action set: feasible (fresh) / transition (carried) / the reduced
  // set (fresh role-risk) — annotation-bounding only since portfolio-v7.
  offered: PortfolioAction[];
  context_trim_carveout: boolean;
  tax_note: string | null;
  // role_risk_only decision inputs (null/empty on priced): 7b is that branch's
  // sole action author, so the verdict's reads ride the spine.
  class_label: string | null;
  role_summary: string | null;
  expense_drag: number | null;
  observable_risk: number | null;
  structural_flag: boolean;
  exposure_tilt: ExposureWeight[];
  evidence_gaps: string[];
  // Same-underlying option-overlay read (covered call / protective put +
  // coverage), classified from the snapshot's OCC option rows; both branches.
  option_overlay: string | null;
}

// One sector row of the whole-book exposure table (direct + fund-folded).
export interface SectorExposureRow {
  sector: string;
  direct_weight: number;
  fund_weight: number;
  holdings: string[];
}

// A sector-level overlap cluster — holdings sharing one exposure above the
// threshold, so they size down together.
export interface OverlapCluster {
  sector: string;
  combined_weight: number;
  symbols: string[];
}

// A not-rated position's risk/exposure contribution — market value + signed
// notional where derivable; the unsourceable analytics ride as typed gaps.
export interface NotRatedContribution {
  symbol: string;
  asset_class: AssetClass;
  weight: number;
  market_value: number;
  signed_notional: number | null;
  material: boolean;
  gaps: string[];
}

// The Step-7a whole-book aggregates + per-holding spine, persisted with the
// roll-up (docs/portfolio-analysis.md §Portfolio roll-up and construction).
export interface BookAggregates {
  spine: SizingSpineRow[];
  sector_exposure: SectorExposureRow[];
  unknown_sector_weight: number;
  overlap_clusters: OverlapCluster[];
  not_rated: NotRatedContribution[];
  cash_weight: number;
  top_position_weight: number;
  correlation_note: string;
}

// The validated portfolio-level view the 7b construction call produced — the
// roll-up card's synthesis half (docs/portfolio-workflow.md §Step 7b). The
// external-funding line is app-computed by the joint-feasibility solve: net new
// dollars = buys − disposition proceeds; a negative value is net cash raised.
export interface ConstructionView {
  risk_posture: string;
  deployment_stance: string;
  concentration_read: string;
  closed_positions_note?: string | null;
  external_funding: number | null;
  implied_total: number | null;
  retried: boolean;
  // Engine-bound findings against the model's plan (a rung outside the engine
  // set, a range outside its band, a cap breach, unfunded buys) — annotations
  // since portfolio-v7, never enforcement. Absent on pre-v7 runs.
  engine_bound_annotations?: string[];
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
  // Absent on runs persisted before the field existed.
  data_health?: DataHealth | null;
  // The Step-7a aggregates + per-holding spine the construction call read
  // (engine sets + annotation bounds) — persisted for auditability; the page
  // renders only the view today.
  aggregates?: BookAggregates | null;
  // The construction call's portfolio-level view — absent on pre-construction
  // runs, and null WITH aggregates present on a degraded run (construction
  // failed; the persisted actions are pre-construction reads — leans, carried
  // actions, role/risk placeholders — not a plan).
  construction?: ConstructionView | null;
  overview: string;
}

// One arm's band-calibration read (window × target-parameter version) — the
// deterministic scoreboard's unit (docs/portfolio-analysis.md §Outcome learning).
export interface TargetCalibrationRead {
  window_months: number;
  parameter_version?: string | null;
  scored: number;
  coverage_rate: number | null;
  nominal_coverage: number;
  mean_interval_score: number | null;
  mean_base_signed_error: number | null;
}

// One arm's outlook direction hit-rate at its mapped window (short→1, mid→6,
// long→12 months); neutral reads count beside the hit-rate, never inside it.
export interface OutlookDirectionRead {
  arm: "engine" | "model";
  window_months: number;
  scored: number;
  hits: number;
  neutral: number;
}

// The paired model-vs-engine head-to-head: both arms scored over the identical
// episode set (episodes where both bands exist), so the comparison is
// same-events by construction — the only read the arms are compared on.
export interface HeadToHeadRead {
  window_months: number;
  scored: number;
  engine_mean_interval_score: number | null;
  model_mean_interval_score: number | null;
  engine_coverage_rate: number | null;
  model_coverage_rate: number | null;
}

// One matured outcome-window line (per symbol) from the run's label pass.
export interface MaturedNote {
  symbol: string;
  episode_id: string;
  window_months: number;
  outcome: string;
  total_return: number | null;
  price_return: number | null;
}

// The subset of the run's outcome-learning records the page renders: the
// model-vs-engine scoreboard reads and the matured lines. Absent on pre-outcome
// runs; the fuller record (cohorts, lead times, eligibility) stays backend-only.
export interface OutcomeRecordsView {
  matured: MaturedNote[];
  reads: {
    target_calibration: TargetCalibrationRead[];
    model_target_calibration?: TargetCalibrationRead[];
    head_to_head?: HeadToHeadRead[];
    outlook_direction?: OutlookDirectionRead[];
  };
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
  // This run's outcome-learning records (scoreboard subset) — absent on runs
  // persisted before the outcome slice.
  outcome?: OutcomeRecordsView | null;
}

// --- Portfolio quick check ---------------------------------------------------
// The engine-only between-run sweep's persisted state, returned by
// `latest_quick_check` (docs/portfolio-analysis.md §The quick check). Rendered
// as the Portfolio page's card overlay: the amber attention flag, the quiet
// evidence-event badge, and the degraded-sweep note. Cleared by the next
// successful full run, so `null` is the common state.

export type SweepFamily =
  | "market-data"
  | "filing"
  | "revision"
  | "earnings"
  | "news-seed"
  | "fund-info"
  | "rate-anchor";

export type SweepState = "fresh-clear" | "flagged" | "unknown";

export interface FamilySweep {
  family: SweepFamily;
  state: SweepState;
  note?: string | null;
}

export type FlagTrigger =
  | "confirmed-falsifier-breach"
  | "fired-trigger"
  | "hurdle-newly-fails"
  | "price-outside-band";

export interface AttentionFlag {
  trigger: FlagTrigger;
  detail: string;
  raised_at: string;
}

export type EvidenceEventKind =
  | "earnings-actual"
  | "material-filing"
  | "revision-move"
  | "news-seed"
  | "fund-info-change"
  | "exposure-shift";

export interface EvidenceEvent {
  kind: EvidenceEventKind;
  detail: string;
  observed_at: string;
}

export interface HoldingQuickState {
  symbol: string;
  families: FamilySweep[];
  flag?: AttentionFlag | null;
  evidence_events: EvidenceEvent[];
  // Engine evaluation state per condition id — persisted for the next run,
  // never rendered.
  condition_states: unknown[];
  last_hurdle_state?: string | null;
  notes: string[];
}

export interface QuickCheckState {
  // The full run the sweep ran against — the overlay applies only while this
  // matches the rendered run.
  swept_run_id: string;
  last_checked_at: string;
  holdings: HoldingQuickState[];
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
  // False marks a degraded run — Step 7b's construction failed after the
  // per-holding pass, so the row carries verdicts but no constructed book and
  // is excluded from the latest view (it opens read-only from the history).
  constructed: boolean;
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
  | "step-thinking"
  | "run-finished";

export interface ProgressMessage {
  run_id: string;
  seq: number;
  kind: ProgressKind;
  // run-started: a short human title for the run.
  label?: string;
  // step-started / step-finished: the stable step key + its human label.
  // step-thinking: the key of the step the reasoning chunk belongs to (the
  // portfolio per-holding steps), folded into that step's reasoning pane.
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
  // agent-token / agent-thinking / analyst-thinking / step-thinking: a coalesced
  // chunk of the streamed report text, the main agent's reasoning, one analyst's
  // reasoning, or a step-scoped stage's reasoning, respectively.
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

// `flagged` / `unknown` are the quick check's per-holding sweep outcomes
// (docs/portfolio-analysis.md §The quick check): the step completed, but the
// sweep raised an attention flag / could not vouch for a signal family — the
// tracker renders them as completed-with-outcome, never as a failure.
export type StepStatus =
  | "pending"
  | "running"
  | "ok"
  | "failed"
  | "cancelled"
  | "flagged"
  | "unknown";

// One pipeline step in the tracker. `requests` carries the baseline step's
// per-series rows; `agentText` accumulates the main-agent step's streamed report;
// `agentThinking` accumulates its streamed reasoning (extended-thinking summary),
// shown as a quieter stream above the report — and, via step-thinking events, a
// step-scoped stage's reasoning on its own step (the portfolio per-holding
// interpretation). Empty for non-thinking models.
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
  /// True once the backend emitted `step-started` for this key. A step synthesized
  /// by `ensureStep` from a request row is "running" too, so request routing needs
  /// to tell the two apart before adopting rows into the step that is running.
  backendStarted: boolean;
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
