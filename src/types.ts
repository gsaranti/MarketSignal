// Shared frontend types mirroring the Rust structs the Tauri commands return.

// The main surface showing — a ref switch in App.vue, no router.
export type AppView = "report" | "portfolio" | "inbox" | "archive" | "settings";

export interface ReportSummary {
  report_id: string;
  report_type: string;
  created_at: string;
  // The agent-written headline; "" falls back to the product name in the UI.
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

// Mirrors the Rust `config::ValidationReport` / `WarningCategory`
// (`check_configuration`).
export interface WarningCategory {
  kind: string;
  title: string;
  items: string[];
  // Echoed back to `dismiss_warning` so the dismissal targets this rendered row,
  // never a re-derived one; null except for the dismissible failed-jobs category.
  dismiss_id: string | null;
}

export interface ValidationReport {
  categories: WarningCategory[];
  is_blocked: boolean;
}

// Mirrors the Rust `research::ResearchDocument` (`list_research_inbox`;
// docs/research-documents.md). `modified` is UTC RFC3339 rendered in local
// time, null when the platform couldn't report one. `parse_error` is set only
// while the file on disk is still the one that failed; always null for the
// archive listing.
export interface ResearchDocument {
  name: string;
  format: string;
  supported: boolean;
  size_bytes: number;
  modified: string | null;
  parse_error: string | null;
}

// Mirrors the Rust `jobs::SectionStamps` (docs/scheduling.md §Job Status
// Visibility). Timestamps are UTC RFC3339, rendered in local time.
export interface SectionStamps {
  last_successful_at: string | null;
  last_failed_at: string | null;
  last_failure_detail: string | null;
  last_skipped_at: string | null;
  last_cancelled_at: string | null;
}

// Mirrors the Rust `jobs::JobStatus` (`job_status`; docs/scheduling.md §Job
// Status Visibility). The run slot is global; the stamps come per section.
export interface JobStatus {
  is_running: boolean;
  // Which workflow holds the run slot, so a Schwab connect never reads as a
  // report run in the footer.
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
  path: string;
  reports: number;
  learnings: number;
  snapshots: number;
  portfolio_runs: number;
  holdings_pulls: number;
  files: number;
  encrypted: boolean;
}

// `skipped_reports` counts report records whose Markdown body was missing from
// the archive, never imported as shells.
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

// `import_data_inspect`'s result, null on a cancelled Open dialog; a non-empty
// store means the replace-all confirmation.
export interface ImportInspection {
  path: string;
  store_empty: boolean;
  info: ArchiveInfo;
}

// Mirrors the Rust `settings::*` structs (docs/configuration.md). A credential
// is reported only as configured or not — the raw key never leaves the backend.

// Sourced from the Rust `AgentModel`, the single home for slugs and labels.
export interface ModelOption {
  slug: string;
  label: string;
  provider: string; // "OpenAI" | "Anthropic" — used to group the dropdown
}

// The four agent slots' model slugs, "" when unset.
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

export type CredentialKey = "openai" | "anthropic" | "fmp" | "fred" | "tavily";

// Mirrors the Rust `connection_test::ConnectionTestResult` (`test_connection`);
// never carries the secret.
export interface ConnectionTestResult {
  ok: boolean;
  detail: string;
}

// docs/configuration.md §Local Models. Not secrets, so they round-trip in full,
// "" when unset.
export interface LocalModelSettings {
  daemon_endpoint: string;
  reasoner_model: string;
  fast_model: string;
}

// docs/configuration.md §Web Research. Not a secret, so it round-trips in full,
// "" when unset.
export interface WebResearchSettings {
  searxng_endpoint: string;
}

// Returned by `test_searxng` (docs/web-research.md §Tavily fallback;
// docs/interface.md §Pre-run web-research notice). Never a gate: `degraded`
// triggers the notice.
export interface WebResearchPreflight {
  status: "ok" | "unreachable" | "not_configured";
  detail: string | null;
  degraded: boolean;
}

// Returned by `get_settings`.
export interface SettingsView {
  models: AgentModels;
  credentials: CredentialStatus;
  local_models: LocalModelSettings;
  web_research: WebResearchSettings;
  available_models: ModelOption[];
}

// Returned by `test_local_daemon` (docs/interface.md §Connection status);
// `missing_models` lists configured roster ids the daemon hasn't pulled.
export interface LocalDaemonStatus {
  reachable: boolean;
  detail: string | null;
  missing_models: string[];
}

// Mirrors the Rust `storage::TruncationStats` (`truncation_stats`;
// docs/agents.md §Data Extraction). Two rates derive: `total_truncations` over
// `total_docs_parsed` and `total_chars_dropped` over `total_original_chars`.
export interface TruncationStats {
  total_truncations: number;
  total_docs_parsed: number;
  // Truncations with no parse-run denominator; > 0 and the readout withholds
  // the doc rate rather than mix cohorts.
  unaligned_truncations: number;
  total_original_chars: number;
  // Parse-run rows with no char count; > 0 and the readout withholds the chars
  // rate rather than mix cohorts.
  parse_runs_missing_original_chars: number;
  reports_affected: number;
  total_chars_dropped: number;
  by_format: FormatCount[];
  latest_captured_at: string | null;
}

export interface FormatCount {
  format: string;
  count: number;
}

// Mirrors the Rust `portfolio::InvestorProfileDisplay` (`get_investor_profile`;
// docs/configuration.md §Investor Profile). Composed backend-side from the same
// label source the action call's prompt renders, so the two cannot drift.
export interface InvestorProfileDisplay {
  objective: string;
  risk_tolerance: string;
  horizon: string;
  tax: string;
  cash: string;
}

// The token-gated half of a `save_settings` submission; null or "" leaves the
// stored secret unchanged (docs/configuration.md §API Tokens).
export interface CredentialUpdate {
  openai: string | null;
  anthropic: string | null;
}

// The `save_provider_credentials` submission, ungated by the cloud tokens; the
// same null/"" = unchanged semantics.
export interface ProviderCredentialUpdate {
  fmp: string | null;
  fred: string | null;
  tavily: string | null;
}

// --- Charles Schwab connection ---------------------------------------------
// Mirrors the Rust `schwab_oauth::{SchwabConnection, SchwabStatus}`
// (`schwab_status`; docs/schwab-integration.md, docs/interface.md §Connection
// status). A parallel shape rather than the credential machinery above: the
// client id is a non-secret that round-trips its value, and connection state is
// a third axis.

// Derived from the stored token set without a network probe; "expired" is a
// lapsed 7-day refresh window.
export type SchwabConnection = "not-connected" | "connected" | "expired";

export interface SchwabStatus {
  client_id: string;
  // Presence only, never the value.
  secret_configured: boolean;
  connection: SchwabConnection;
  // UTC RFC3339, null when not connected; rendered in local time.
  refresh_expires_at: string | null;
}

// The Schwab surface's save payload; a null `client_secret` leaves the stored
// secret unchanged.
export interface SchwabCredentialUpdate {
  client_id: string;
  client_secret: string | null;
}

// --- Portfolio Analysis ------------------------------------------------------
// Mirrors the Rust `portfolio::*` / `schwab::*` structs (docs/portfolio-analysis.md
// §Storage and display, §Triggering). Enum wire shapes are kebab-case.

// The tracker's resume read for a failed or cancelled Portfolio run
// (`portfolio_resume_status`; docs/portfolio-analysis.md §Failure posture).
// camelCase wire shape.
export interface PortfolioResumeStatus {
  available: boolean;
  runId: string | null;
  createdAt: string | null;
  completed: number;
  total: number;
  reason: string | null;
}

export type AssetClass =
  | "stock"
  | "etf"
  | "mutual-fund"
  | "option-contract"
  | "fixed-income"
  | "cash"
  | "other";

// `cost_basis` and `market_value` are Schwab-reported account-currency totals,
// not per-share; the sort keys derive from them, never quote × shares.
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

// The latest standalone pull, distinct from the snapshot persisted inside each
// run (the diff baseline). `pulled_at` is UTC RFC3339, rendered in local time.
export interface HoldingsPull {
  pulled_at: string;
  holdings: Holdings;
}

// The backend's deterministic quantity diff vs the prior run's snapshot, never
// re-derived in the frontend.
export type PositionChange = "new" | "increased" | "decreased" | "unchanged";

export type PortfolioGrade = "A" | "B" | "C" | "D" | "F";
export type PortfolioAction =
  | "sell-all"
  | "trim"
  | "hold"
  | "add"
  | "add-aggressively";
export type PortfolioConviction = "high" | "medium" | "low";

// The four engine-computed sub-scores, 0–100, higher is better (risk inverted at
// source: safer scores higher).
export interface SubScores {
  quality: number;
  valuation: number;
  momentum: number;
  risk: number;
}

// The metric class a stock's factors were chosen by (portfolio::factors).
export type MetricClass = "general" | "bank" | "other-financial" | "equity-reit";

// `anchor_row` is the row the banded factors read (the sector, "market-wide",
// "Banks" or "REITs"); `thin_own_history` means most factors scored on their
// bands alone.
export interface GradeBasis {
  class: MetricClass;
  anchor_row: string;
  thin_own_history: boolean;
}

// The engine computed the figures; the model selected and justified the base
// case.
export interface PriceTarget {
  base: number;
  bear: number;
  bull: number;
  methodology: string;
}

// Rolling windows from the run date (docs/portfolio-analysis.md §Starting
// parameters); each null where underivable.
export interface PriceTargets {
  three_month: PriceTarget | null;
  twelve_month: PriceTarget | null;
  three_year: PriceTarget | null;
}

// Engine-computed on every priced verdict; `HurdleState` is the dead-money read.
export type RiskTier = "low" | "medium" | "high";
export type HurdleState = "clears" | "indeterminate" | "fails" | "unscorable";

// An activity proxy, not positioning truth; any field null when the chain
// lacked the data.
export interface OptionsSignal {
  put_call_volume: number | null;
  put_call_open_interest: number | null;
  implied_volatility: number | null;
  iv_skew: number | null;
}

// The typed appendix of the thesis document (docs/portfolio-analysis.md §The
// holding verdict), persisted exactly as transcribed; null where the document
// states no value.
export interface ThesisAppendix {
  conviction: PortfolioConviction | null;
  expected_price_3m: number | null;
  expected_price_12m: number | null;
  expected_price_3y: number | null;
}

// Spot vs the engine's twelve-month band at authoring, stamped so the quick
// check's band monitor fires on a change in the relationship, never on the
// standing state.
export type BandRelation = "inside" | "below-band" | "above-band";

export interface GradedVerdict {
  grade: PortfolioGrade;
  sub_scores: SubScores;
  // Authored by the dedicated action call from the holding's own evidence plus
  // the investor profile, never the whole book.
  action: PortfolioAction;
  action_rationale: string;
  // The thesis document as the model wrote it, never rewritten.
  thesis_document: string;
  appendix: ThesisAppendix;
  price_targets: PriceTargets;
  options_signal: OptionsSignal;
  risk_tier: RiskTier;
  dead_money: HurdleState;
  // True when the letter rests on an imputed (neutral-50) sub-score.
  low_confidence_grade: boolean;
  // The priced fund's deterministic strategy classification; null for a stock.
  fund_class_label: string | null;
  // What a stock's letter was scored against; null on a fund.
  grade_basis: GradeBasis | null;
  // The engine's own action rung — a computed read beside the model's action,
  // never a recommendation (docs/portfolio-analysis.md §Starting parameters).
  engine_rung: PortfolioAction;
  // The authoring-time band relation; null where no band or spot existed.
  authored_band_relation: BandRelation | null;
}

// A sector or country label and its fraction of the fund.
export interface ExposureWeight {
  label: string;
  weight: number;
}

// The role-risk-only branch: a structurally unpriceable vehicle class — no
// letter, no targets, no conviction (docs/portfolio-analysis.md §Intrinsic
// verdict).
export interface RoleRiskVerdict {
  class_label: string;
  thesis_document: string;
  exposure_tilt: ExposureWeight[];
  expense_drag: number | null;
  observable_risk: number | null;
  structural_flag: boolean;
  is_cef: boolean;
  // Market price ÷ NAV − 1, positive = premium; null is the named gap, carried
  // in evidence_gaps.
  nav_premium: number | null;
  evidence_gaps: string[];
  // Authored by the dedicated action call with the full ladder open; the
  // engine's reduced {sell-all, trim, hold} set rides as evidence only.
  action: PortfolioAction;
  action_rationale: string;
}

// Internally tagged on `status` (serde `tag = "status"`).
export type VerdictDisposition =
  | ({ status: "priced" } & GradedVerdict)
  | ({ status: "role-risk-only" } & RoleRiskVerdict)
  | { status: "not-rated"; reason: string }
  // An abstention carries the prior's thesis document, where one exists, so
  // the holding stays tracked across it; null on a debut abstention.
  | { status: "insufficient-evidence"; reason: string; prior_thesis_document: string | null };

// docs/portfolio-analysis.md §Outcome learning: "rule-demoted" is an over-age
// carried add-family action demoted to hold at the roll-up.
export type ActionSource = "model-chosen" | "rule-demoted";

// Set on an increase: which way it moved the average cost per share, with the
// average on the prior pull and this one (docs/portfolio-analysis.md §Holdings
// change tracking). Null otherwise, or where either average is undefined.
export type BasisDirection = "paid-up" | "averaged-down";
export interface BasisMove {
  direction: BasisDirection;
  prior_average_cost: number;
  average_cost: number;
}

export interface HoldingVerdict {
  symbol: string;
  asset_class: AssetClass;
  position_change: PositionChange;
  basis_move: BasisMove | null;
  disposition: VerdictDisposition;
  // UTC RFC3339 vintage of the full pass that produced this verdict; differs
  // from the run's created_at on a carried verdict. Null on a debut
  // insufficient-evidence exit.
  analyzed_at: string | null;
  action_source: ActionSource;
  // Set on a carried verdict whose position's net side reversed since it was
  // written (docs/portfolio-analysis.md §Triggering); false on fresh passes.
  side_reversed: boolean;
}

// Present last run, absent now; surfaced in the roll-up only, never a card.
export interface ExitedPosition {
  symbol: string;
  description: string;
  prior_quantity: number;
  prior_cost_basis: number;
  prior_market_value: number;
}

// The run-level data-health aggregate, computed from the persisted audits;
// `attention` marks infrastructure degradation.
export interface DataHealth {
  prompt_usage: PromptUsage[];
  targets_total: number;
  rate_anchored_count: number;
  raw_percentile_count: number;
  current_multiple_carry_count: number;
  dispersion_floor_count: number;
  deep_history_failures: number;
  dgs10_history_gap: boolean;
  // The house view was omitted for staleness (docs/portfolio-workflow.md
  // §Step 5); informational only.
  house_view_omitted: boolean;
  // Enriching-feed gaps: named in `summary`, never attention triggers.
  commodity_gaps: number;
  positioning_gaps: number;
  cboe_gap: boolean;
  finra_gap: boolean;
  ma_gap: boolean;
  // The accuracy pass hit an episode-store error; the scores read unavailable.
  accuracy_gap: boolean;
  benchmark_gaps: number;
  // Research coverage gaps: named in `summary`, never an attention trigger.
  research_degraded_holdings: number;
  research_gap_count: number;
  // Local chat calls under context pressure: near-full (≥ 90% of num_ctx) or
  // likely front-truncated (Ollama's reported count is post-truncation, so fill
  // alone can't see it). An attention trigger. `peak_prompt` is the run's
  // fullest prompt regardless.
  context_pressure: PromptUsage[];
  peak_prompt: PromptUsage | null;
  // Retries the bounded retry-once fired (docs/local-models.md §The local-model
  // adapter seam); an attention trigger.
  model_retries: RetryEvent[];
  attention: boolean;
  summary: string;
}

// One fired retry: the stage and the failure class with the failed attempt's
// error chain. Only recovered retries persist (a failed holding's are dropped
// with it), so entries measure the absorbed transient rate.
export interface RetryEvent {
  stage: string;
  cause: string;
}

// One resolved physical chat attempt. API counters are raw, phase-dependent;
// durations are nanoseconds. elapsed_ms is independently measured app wall time.
export interface PromptUsage {
  stage: string;
  model: string;
  think: boolean | null;
  streamed: boolean;
  tools: boolean;
  format: boolean;
  num_ctx: number;
  num_predict: number | null;
  prompt_chars: number;
  elapsed_ms: number;
  thinking: { state: "unavailable" } | { state: "complete" | "partial"; chars: number };
  api: {
    prompt_eval_count: number | null;
    eval_count: number | null;
    total_duration: number | null;
    load_duration: number | null;
    prompt_eval_duration: number | null;
    eval_duration: number | null;
  };
  adapter_ok: boolean;
  done_reason: string | null;
  output_limited: boolean;
}

export interface PortfolioRollUp {
  graded_count: number;
  not_rated_count: number;
  insufficient_evidence_count: number;
  // Counted beside the priced holdings, never pooled with them.
  role_risk_only_count: number;
  // The length of PortfolioRun.failed_holdings, apart from the analyzed counts.
  failed_count: number;
  top_position_weight: number;
  cash_weight: number;
  exited: ExitedPosition[];
  data_health: DataHealth;
  overview: string;
}

export interface PortfolioRun {
  run_id: string;
  created_at: string;
  holdings: Holdings;
  verdicts: HoldingVerdict[];
  roll_up: PortfolioRollUp;
  // Per-holding audit records, persisted for traceability; not rendered.
  audit: unknown[];
  // Holdings whose analysis failed while the run continued
  // (docs/portfolio-analysis.md §Failure posture).
  failed_holdings: HoldingFailure[];
  // The run's accuracy record (docs/portfolio-analysis.md §Outcome learning).
  accuracy: AccuracyRecord;
}

// One arm's accuracy score at one horizon: the mean per-check score, 0–100,
// with the check that last moved it. `null` in its slot is "no score yet".
export interface ArmScore {
  score: number;
  checks: number;
  last_moved_on: string;
  last_moved_check: number;
}

export interface HorizonAccuracy {
  model: ArmScore | null;
  engine: ArmScore | null;
}

export interface AccuracyScores {
  three_month: HorizonAccuracy;
  twelve_month: HorizonAccuracy;
  three_year: HorizonAccuracy;
}

// `scores` is null when the episode store could not be read this run; `opened`
// and `checks` persist for the audit only.
export interface AccuracyRecord {
  scores: Record<string, AccuracyScores> | null;
  opened: unknown[];
  checks: unknown[];
}

// A per-holding analysis failure the run isolated (docs/portfolio-analysis.md
// §Failure posture).
export interface HoldingFailure {
  symbol: string;
  // The failing operation plus its root cause; the full chain rides the tracker
  // and logs.
  cause: string;
  // True when a prior verdict was carried forward and shows vintage-stamped
  // beside the failed badge; false is an empty debut-failure card.
  carried_prior: boolean;
}

// --- Portfolio quick check ---------------------------------------------------
// The engine-only between-run sweep's persisted state (`latest_quick_check`;
// docs/portfolio-analysis.md §The quick check). Cleared by the next successful
// full run, so `null` is the common state.

export type SweepFamily =
  | "market-data"
  | "filing"
  | "revision"
  | "earnings"
  | "fund-info"
  | "rate-anchor";

export type SweepState = "fresh-clear" | "flagged" | "unknown";

export interface FamilySweep {
  family: SweepFamily;
  state: SweepState;
  note?: string | null;
}

// The two engine monitors — the only triggers the quick check raises.
export type FlagTrigger = "hurdle-newly-fails" | "price-outside-band";

export interface AttentionFlag {
  trigger: FlagTrigger;
  detail: string;
  raised_at: string;
}

export type EvidenceEventKind =
  | "earnings-actual"
  | "material-filing"
  | "revision-move"
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
  last_hurdle_state?: string | null;
  notes: string[];
}

export interface QuickCheckState {
  parameter_version: string;
  // The full run the sweep ran against — the overlay applies only while this
  // matches the rendered run.
  swept_run_id: string;
  last_checked_at: string;
  holdings: HoldingQuickState[];
}

// One sidebar row of the Portfolio-runs history (`list_portfolio_runs`;
// docs/interface.md §Main Layout). An unreadable row's zero counts are
// placeholders; opening a run fetches the full record via `get_portfolio_run`.
export interface PortfolioRunSummary {
  run_id: string;
  // Canonical UTC RFC3339; rendered in local time.
  created_at: string;
  holdings_count: number;
  graded_count: number;
  // False when the persisted blob no longer decodes: the row lists but cannot
  // open.
  readable: boolean;
}

// --- Live job tracker -------------------------------------------------------
// Mirrors the Rust `progress::ProgressMessage` streamed over the "job-progress"
// event. Discriminated by `kind`; every message carries `run_id` (to discard
// stragglers from a prior run) and a monotonic `seq`.
export type ProgressKind =
  | "run-started"
  | "step-started"
  | "step-finished"
  | "request-started"
  | "request-finished"
  | "agent-token"
  | "agent-thinking"
  | "analyst-thinking"
  // stream-restarted: a streaming call re-attempts under its bounded retry, so
  // its live panes clear — the main agent's when `posture` is absent, otherwise
  // that analyst's.
  | "stream-restarted"
  | "step-thinking"
  // model-call-started / model-call-finished: diagnostic boundaries for the
  // backend's thought-log sink; the tracker deliberately ignores them.
  | "model-call-started"
  | "model-call-finished"
  | "run-finished";

export interface ProgressMessage {
  run_id: string;
  seq: number;
  kind: ProgressKind;
  // run-started: a short human title for the run.
  label?: string;
  // step-started / step-finished: the stable step key. step-thinking: the step
  // the reasoning chunk folds into. request-started / request-finished: the
  // row's owning step, stamped by the backend, never inferred here; absent when
  // the request fired with no step open.
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
  // request-started / request-finished: present on research rows only.
  target?: RequestTarget;
  // agent-token / agent-thinking / analyst-thinking / step-thinking: a coalesced
  // chunk of the streamed text or reasoning.
  delta?: string;
  // analyst-thinking: which analyst (bull / bear / balanced) the chunk belongs
  // to. stream-restarted: the analyst whose pane restarts; absent for the main
  // agent.
  posture?: string;
  // run-finished: the new report's id, on success only.
  report_id?: string | null;
}

// Mirrors the Rust `progress::RequestTarget`; `kind` is "search" or "fetch".
// Only the research loop's rows carry one.
export interface RequestTarget {
  kind: string;
  text: string;
}

// One request row in the tracker. `status` is "running", then "ok", "empty" (a
// 2xx with no usable data) or a gap reason; `target` is null off the research
// rows; `detail` is the failure's cause or a short outcome note.
export interface TrackerRequest {
  provider: string;
  group: string;
  seriesId: string;
  name: string;
  status: string;
  detail: string | null;
  target: RequestTarget | null;
}

// `flagged` / `unknown` are the quick check's sweep outcomes
// (docs/portfolio-analysis.md §The quick check): completed-with-outcome, never
// a failure.
export type StepStatus =
  | "pending"
  | "running"
  | "ok"
  | "failed"
  | "cancelled"
  | "flagged"
  | "unknown";

// `agentThinking` is the step's streamed reasoning (the main agent's, or a
// step-scoped stage's via step-thinking); `analystThinking` maps each analyst
// posture to its own. Both empty for non-thinking models.
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

// Built in App.vue from the event stream. `terminal` is null until the run
// finishes. `unattributed` rows carried no owning-step stamp; they render as
// their own trailing list, never synthesized into a step, so an unowned row can
// never paint a failed stage into a run's record.
export interface RunTrace {
  runId: string;
  label: string;
  steps: TrackerStep[];
  unattributed: TrackerRequest[];
  terminal: { status: string; detail: string | null } | null;
}
