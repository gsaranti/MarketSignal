// Reusable test doubles for the four `@tauri-apps/api` modules `App.vue` imports
// (core / event / window / app). App.vue is the only SFC that touches Tauri, so
// this is the single home for the mock surface.
//
// Deliberately pure — no `vi.*` here. `vi.mock` factories are hoisted above a
// spec's imports, so the mock *functions* must be declared in the spec (via
// `vi.hoisted`); this module only supplies their *implementations* — the invoke
// command-router and the default response shapes — applied in the spec's
// `beforeEach`. Keeping `vi` out means the helper is import-order-agnostic and
// reusable from any spec without fighting hoist ordering.

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

// Minimal valid shapes for the commands App's `onMounted` cascade calls, so a
// full App mount completes without a hand-rolled fixture per spec. A clean,
// unblocked config with one enabled-but-idle job and nothing yet generated.
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
    embedder_model: "",
  },
  web_research: { searxng_endpoint: "" },
  available_models: [],
};

// Read alongside settings (refreshSettings); a clean install has no Schwab
// credentials and no connection.
export const defaultSchwabStatus: SchwabStatus = {
  client_id: "",
  secret_configured: false,
  connection: "not-connected",
  refresh_expires_at: null,
};

// The local-suite presence gate (check_local_configuration). The default keeps
// the fixture "clean and unblocked" like the cloud validation above — specs
// that exercise the local warning band override with categories.
export const defaultLocalValidation: ValidationReport = {
  categories: [],
  is_blocked: false,
};

// A minimal persisted Portfolio run (one graded stock + one not-rated cash-like
// position + one exited name) for specs that render the Portfolio page through
// App. Kept small — PortfolioView's own spec builds richer fixtures.
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
      disposition: {
        status: "priced",
        grade: "B",
        sub_scores: { quality: 70, valuation: 55, momentum: 62, risk: 68 },
        action: "hold",
        conviction: "medium",
        horizon_outlook: { short: "neutral", mid: "bullish", long: "bullish" },
        price_targets: {
          one_month: null,
          twelve_month: {
            base: 210,
            bear: 180,
            bull: 240,
            methodology: "drift off revenue growth",
          },
        },
        price_target_rationale: "base case tracks revenue drift",
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
        structural_flag: false,
        financial_summary: "Solid margins.",
        what_changed: "First analyzed run.",
        model_view: {
          sub_scores: { quality: 70, valuation: 55, momentum: 62, risk: 68 },
          letter: "B",
          price_targets: {
            one_month: { base: 205, bear: 195, bull: 215 },
            twelve_month: { base: 210, bear: 180, bull: 240 },
          },
          self_assessment: "",
        },
        engine_view: {
          outlook: { short: "neutral", mid: "bullish", long: "bullish" },
          conviction: "medium",
          action: "hold",
        },
      },
    },
  ],
  roll_up: {
    graded_count: 1,
    role_risk_only_count: 0,
    not_rated_count: 0,
    insufficient_evidence_count: 0,
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
      context_pressure: [],
      peak_prompt: null,
      model_retries: [],
      attention: false,
      summary: "Data health: clean.",
    },
    overview: "One graded holding.",
  },
  audit: [],
  outcome: {
    matured: [],
    reads: {
      target_calibration: [],
      model_target_calibration: [],
      head_to_head: [],
      outlook_direction: [],
    },
  },
};

// A standalone Pull-holdings snapshot, fresher than samplePortfolioRun.
export const sampleHoldingsPull: HoldingsPull = {
  pulled_at: "2026-07-07T09:00:00Z",
  holdings: samplePortfolioRun.holdings,
};

// Loaded alongside settings when the Settings view opens; a clean install has
// recorded no truncations.
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

// The fixed investor-profile preset the read-only Settings block renders —
// mirrors the Rust `InvestorProfile::default_fixture().display()` strings
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

// The command → response map. Any command absent here throws when invoked, so a
// new `onMounted` call — or a typo — surfaces loudly instead of resolving
// `undefined` and silently passing a half-wired mount.
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
    // Read on Settings-view entry, alongside get_settings.
    truncation_stats: () => defaultTruncationStats,
    // The fixed investor-profile preset for the read-only Settings block —
    // ready-to-render display strings from the backend label source.
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
    // The pre-run web-research probe (also the Settings connection row). The
    // default reads healthy so a spec-driven Run analysis launches without the
    // degraded-mode notice; notice specs override with a degraded read.
    test_searxng: () => ({
      status: "ok",
      detail: null,
      tavily_fallback: false,
      degraded: false,
    }),
    get_portfolio_run: () => null,
    save_schwab_credentials: () => null,
    schwab_connect: () => null,
    schwab_disconnect: () => null,
    generate_portfolio_manual: () => samplePortfolioRun,
    pull_holdings: () => sampleHoldingsPull,
    // Data portability: both dialogs default to "cancelled" (null), the no-op
    // outcome. `import_data` is deliberately unregistered — it must only ever
    // run against an inspected archive a spec explicitly set up, so reaching it
    // without an override is a loud unhandled-invoke failure.
    export_data: () => null,
    import_data_inspect: () => null,
  };
}

// Build an `invoke` implementation. `overrides` replace or add per-command
// handlers — to assert a specific payload, return tailored data, or simulate a
// failure (a handler that throws).
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

// A no-op `UnlistenFn` — the resolved value of both `listen` and `onFocusChanged`.
export const unlisten = (): void => {};

// Capture the callback a spec's mocked `listen` registered for an event, so the
// spec can drive App's run tracker by feeding it `ProgressMessage`s the way the
// backend would over the "job-progress" channel. App registers its listeners in
// `onMounted`, so call this only after the mount's promises have flushed. Stays
// `vi`-free (reads the mock's `.calls` structurally) so the helper keeps its
// import-order-agnostic posture.
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

// The window's `onFocusChanged` sibling of `emitterFor`. App subscribes via
// `getCurrentWindow().onFocusChanged(cb)` in `onMounted`, so the callback is the
// first (only) arg of the single registration; capture it to drive App's
// focus-refresh path by feeding it focus transitions the way wry's window would.
// Same `vi`-free, post-mount-flush contract as `emitterFor`.
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
