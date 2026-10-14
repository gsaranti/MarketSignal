// Test doubles for the `@tauri-apps/api` modules App.vue imports. Deliberately
// `vi`-free: `vi.mock` factories are hoisted above a spec's imports, so the mock
// functions are declared in the spec via `vi.hoisted` and this module supplies
// only their implementations, keeping it import-order-agnostic.

import type {
  HoldingsPull,
  InvestorProfileDisplay,
  JobStatus,
  PortfolioRun,
  ReportSummary,
  ResearchDocument,
  SchwabStatus,
  SettingsView,
  TruncationStats,
  ValidationReport,
} from "../../src/types";

// Minimal valid shapes for App's `onMounted` reads: a clean, unblocked, idle
// install with nothing yet generated.
export const defaultValidation: ValidationReport = {
  categories: [],
  is_blocked: false,
};

const emptyStamps = {
  last_successful_at: null,
  last_failed_at: null,
  last_failure_detail: null,
  last_skipped_at: null,
  last_cancelled_at: null,
};

export const defaultJobStatus: JobStatus = {
  is_running: false,
  running_kind: null,
  report: { ...emptyStamps },
  portfolio: { ...emptyStamps },
};

export const defaultSettings: SettingsView = {
  models: { main: "", bull: "", bear: "", balanced: "" },
  credentials: {
    openai: false,
    anthropic: false,
    fmp: false,
    fred: false,
    tavily: false,
  },
  local_models: {
    daemon_endpoint: "",
    reasoner_model: "",
    fast_model: "",
  },
  web_research: { searxng_endpoint: "" },
  available_models: [],
};

export const defaultSchwabStatus: SchwabStatus = {
  client_id: "",
  secret_configured: false,
  connection: "not-connected",
  refresh_expires_at: null,
};

export const defaultLocalValidation: ValidationReport = {
  categories: [],
  is_blocked: false,
};

// Kept minimal; PortfolioView's own spec builds the richer fixtures.
export const samplePortfolioRun: PortfolioRun = {
  run_id: "prun-1",
  created_at: "2026-07-01T12:00:00Z",
  holdings: {
    positions: [
      {
        symbol: "AAPL",
        description: "Apple Inc.",
        asset_class: "stock",
        quantity: 100,
        cost_basis: 14000,
        market_value: 19500,
        current_price: 195,
      },
    ],
    cash: 10000,
    account_total: 29500,
  },
  verdicts: [
    {
      symbol: "AAPL",
      asset_class: "stock",
      position_change: "unchanged",
      basis_move: null,
      disposition: {
        status: "priced",
        grade: "B",
        sub_scores: { quality: 70, valuation: 55, momentum: 62, risk: 68 },
        action: "hold",
        thesis_document: "Compounding platform with durable pricing power.",
        appendix: {
          conviction: "medium",
          expected_price_3m: null,
          expected_price_12m: 210,
          expected_price_3y: null,
        },
        price_targets: {
          three_month: null,
          twelve_month: {
            base: 210,
            bear: 180,
            bull: 240,
            methodology: "drift off revenue growth",
          },
          three_year: null,
        },
        options_signal: {
          put_call_volume: null,
          put_call_open_interest: null,
          implied_volatility: null,
          iv_skew: null,
        },
        risk_tier: "medium",
        dead_money: "indeterminate",
        action_rationale: "Hold — the thesis is intact.",
        low_confidence_grade: false,
        fund_class_label: null,
        grade_basis: null,
        engine_rung: "hold",
        authored_band_relation: null,
      },
    },
  ],
  roll_up: {
    graded_count: 1,
    role_risk_only_count: 0,
    not_rated_count: 0,
    insufficient_evidence_count: 0,
    failed_count: 0,
    top_position_weight: 0.66,
    cash_weight: 0.34,
    exited: [],
    data_health: {
      targets_total: 1,
      rate_anchored_count: 1,
      raw_percentile_count: 0,
      current_multiple_carry_count: 0,
      dispersion_floor_count: 0,
      deep_history_failures: 0,
      dgs10_history_gap: false,
      house_view_omitted: false,
      commodity_gaps: 0,
      positioning_gaps: 0,
      cboe_gap: false,
      finra_gap: false,
      benchmark_gaps: 0,
      research_degraded_holdings: 0,
      research_gap_count: 0,
      prompt_usage: [], context_pressure: [],
      peak_prompt: null,
      model_retries: [],
      attention: false,
      summary: "Data health: clean.",
    },
    overview: "One graded holding.",
  },
  audit: [],
  failed_holdings: [],
  accuracy: { scores: {}, opened: [], checks: [] },
};

// A standalone Pull-holdings snapshot, fresher than samplePortfolioRun.
export const sampleHoldingsPull: HoldingsPull = {
  pulled_at: "2026-07-07T09:00:00Z",
  holdings: samplePortfolioRun.holdings,
};

export const defaultTruncationStats: TruncationStats = {
  total_truncations: 0,
  total_docs_parsed: 0,
  unaligned_truncations: 0,
  total_original_chars: 0,
  parse_runs_missing_original_chars: 0,
  reports_affected: 0,
  total_chars_dropped: 0,
  by_format: [],
  latest_captured_at: null,
};

// Mirrors the Rust `InvestorProfile::default_fixture().display()` strings
// (pinned backend-side by `investor_profile_display_pins_preset_rows`).
export const defaultInvestorProfile: InvestorProfileDisplay = {
  objective:
    "maximize profit (total return; no income or capital-preservation mandate)",
  risk_tolerance: "aggressive (medium-to-high)",
  horizon: "long-term (durable multi-quarter / multi-year theses)",
  tax: "tax-aware — the possible benefit of realizing a loss is weighed qualitatively; no tax-lot, holding-period, or rate modeling",
  cash: "unconstrained — adds are never gated on observed Schwab cash",
};

export type InvokeHandler = (args?: Record<string, unknown>) => unknown;

// A command absent here throws when invoked, so a new `onMounted` read or a
// typo surfaces loudly instead of resolving `undefined`.
export function defaultInvokeHandlers(): Record<string, InvokeHandler> {
  return {
    // onMounted bootstrap reads.
    check_configuration: () => defaultValidation,
    check_local_configuration: () => defaultLocalValidation,
    latest_portfolio_run: () => null,
    latest_holdings_pull: () => null,
    latest_quick_check: () => null,
    list_portfolio_runs: () => [],
    job_status: () => defaultJobStatus,
    list_reports: () => [] as ReportSummary[],
    list_research_inbox: () => [] as ResearchDocument[],
    list_research_archive: () => [] as ResearchDocument[],
    get_settings: () => defaultSettings,
    truncation_stats: () => defaultTruncationStats,
    get_investor_profile: () => defaultInvestorProfile,
    schwab_status: () => defaultSchwabStatus,
    // Action commands a spec may drive through a user interaction.
    save_settings: () => null,
    save_provider_credentials: () => null,
    save_local_model_settings: () => null,
    test_local_daemon: () => ({
      reachable: false,
      detail: "No local daemon endpoint configured",
      missing_models: [],
    }),
    save_web_research_settings: () => null,
    // Reads healthy so a spec-driven Run analysis launches without the
    // degraded-mode notice.
    test_searxng: () => ({
      status: "ok",
      detail: null,
      degraded: false,
    }),
    get_portfolio_run: () => null,
    save_schwab_credentials: () => null,
    schwab_connect: () => null,
    schwab_disconnect: () => null,
    generate_portfolio_manual: () => samplePortfolioRun,
    pull_holdings: () => sampleHoldingsPull,
    // null is the cancelled-dialog outcome. `import_data` is deliberately
    // unregistered so a spec reaches it only through an archive it set up.
    export_data: () => null,
    import_data_inspect: () => null,
  };
}

export function makeInvokeRouter(
  overrides: Record<string, InvokeHandler> = {}
): (cmd: string, args?: Record<string, unknown>) => Promise<unknown> {
  const handlers = { ...defaultInvokeHandlers(), ...overrides };
  return async (cmd: string, args?: Record<string, unknown>) => {
    const handler = handlers[cmd];
    if (!handler) {
      throw new Error(`tauri test mock: unhandled invoke("${cmd}")`);
    }
    return handler(args);
  };
}

export const unlisten = (): void => {};

// Returns the callback the mocked `listen` registered for `event`. App registers
// its listeners in `onMounted`, so call this only after the mount's promises
// have flushed.
type ListenLike = { mock: { calls: unknown[][] } };
export function emitterFor(
  listenMock: ListenLike,
  event: string
): (payload: unknown) => void {
  const call = listenMock.mock.calls.find((c) => c[0] === event);
  if (!call) {
    throw new Error(`tauri test mock: no listener registered for "${event}"`);
  }
  const cb = call[1] as (e: { payload: unknown }) => void;
  return (payload: unknown) => cb({ payload });
}

// The `onFocusChanged` sibling of `emitterFor`, with the same post-mount-flush
// constraint.
export function focusEmitter(
  onFocusChangedMock: ListenLike
): (focused: boolean) => void {
  const call = onFocusChangedMock.mock.calls[0];
  if (!call) {
    throw new Error("tauri test mock: onFocusChanged was never registered");
  }
  const cb = call[0] as (e: { payload: boolean }) => void;
  return (focused: boolean) => cb({ payload: focused });
}
