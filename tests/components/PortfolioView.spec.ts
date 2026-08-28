// PortfolioView is presentational (props in, events out; App owns every
// invoke), so this spec needs no Tauri mocks — the JobStatusPanel pattern.
// Covers the page's data states (empty / pulled-not-analyzed / analyzed /
// analyzed + fresher pull), the three verdict-card variants, the presence-only
// churn tags, the sort bar (ordering, nulls-last, direction flip, persistence,
// direction-bearing accessible names), the current-holdings table (price column,
// head sorting via aria-sort, nulls-last, persistence), and the trigger gating.

import { describe, test, expect, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import PortfolioView from "../../src/components/PortfolioView.vue";
import type {
  GradedVerdict,
  HoldingQuickState,
  HoldingsPull,
  HoldingVerdict,
  PortfolioRun,
  Position,
  QuickCheckState,
  ThesisLedger,
} from "../../src/types";

function position(symbol: string, over: Partial<Position> = {}): Position {
  return {
    symbol,
    description: `${symbol} Inc.`,
    asset_class: "stock",
    quantity: 100,
    cost_basis: 10_000,
    market_value: 12_000,
    current_price: 120,
    ...over,
  };
}

function graded(over: Partial<GradedVerdict> = {}): GradedVerdict {
  return {
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
        methodology: "v2 spread-anchored multiples",
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
    low_confidence_grade: false,
    fund_class_label: null,
    structural_flag: false,
    financial_summary: "Solid margins.",
    what_changed: "First analyzed run.",
    // Every persisted verdict carries both arms (portfolio-v7). The base
    // fixture's engine arm mirrors its top-level model reads, so no "≠ engine"
    // divergence tag or retrospective shows unless a test overrides the arms
    // (e.g. twoArmGraded).
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
    ...over,
  };
}

function verdict(
  symbol: string,
  disposition: HoldingVerdict["disposition"],
  over: Partial<HoldingVerdict> = {}
): HoldingVerdict {
  return {
    symbol,
    asset_class: "stock",
    position_change: "unchanged",
    disposition,
    ...over,
  };
}

// A full priced-branch thesis ledger; override for the role-risk condition-only
// shape (branch + null engine targets).
function ledger(over: Partial<ThesisLedger> = {}): ThesisLedger {
  return {
    branch: "priced",
    original_thesis: "Original thesis.",
    current_thesis: "Compounding platform with durable pricing power.",
    key_drivers: [{ driver_id: "kd-services", name: "services growth", series: null }],
    monitor: [
      {
        scenario: "bear",
        conditions: "Services growth stalls below 5%",
        probability_pct: 20,
        engine_target: 150,
      },
      {
        scenario: "base",
        conditions: "Services compounding holds",
        probability_pct: 55,
        engine_target: 210,
      },
      {
        scenario: "bull",
        conditions: "Margin expansion resumes",
        probability_pct: 25,
        engine_target: 280,
      },
    ],
    what_must_improve: "Hardware upgrade cycle re-accelerates",
    what_must_not_break: "Services attach rate holds above 30%",
    conditions: [],
    ...over,
  };
}

// Four cards spanning the variants and the sort matrix:
//   MSFT — graded, biggest value, negative gain (30k on 32k cost).
//   AAPL — graded, +39.3% gain.
//   XYZ  — insufficient-evidence, +150% gain (best % gain).
//   OPT  — not-rated option with no reported cost basis (null on gain keys).
const positions: Position[] = [
  position("MSFT", { cost_basis: 32_000, market_value: 30_000 }),
  position("AAPL", { cost_basis: 14_000, market_value: 19_500 }),
  position("XYZ", { cost_basis: 2_000, market_value: 5_000 }),
  position("OPT", {
    asset_class: "option-contract",
    cost_basis: 0,
    market_value: 800,
    current_price: null,
  }),
];

const run: PortfolioRun = {
  run_id: "prun-1",
  created_at: "2026-07-01T12:00:00Z",
  holdings: { positions, cash: 4_700, account_total: 60_000 },
  verdicts: [
    verdict("MSFT", { status: "priced", ...graded({ grade: "A", action: "trim" }) }),
    verdict("AAPL", { status: "priced", ...graded() }, { position_change: "increased" }),
    verdict("XYZ", {
      status: "insufficient-evidence",
      reason: "Too few sources to grade.",
    }),
    verdict("OPT", { status: "not-rated", reason: "Options are not rated." }),
  ],
  roll_up: {
    graded_count: 2,
    not_rated_count: 1,
    insufficient_evidence_count: 1,
    role_risk_only_count: 0,
    top_position_weight: 0.5,
    cash_weight: 0.078,
    exited: [
      {
        symbol: "TSLA",
        description: "Tesla, Inc.",
        prior_quantity: 20,
        prior_cost_basis: 4_000,
        prior_market_value: 5_200,
      },
    ],
    overview: "Two graded holdings; one exit acknowledged.",
  },
  audit: [],
  // The persist-seam marker the backend ships (always concrete on the wire).
};

// A pull FRESHER than the run: NVDA appears (new), MSFT is gone (no longer held).
const fresherPull: HoldingsPull = {
  pulled_at: "2026-07-07T09:00:00Z",
  holdings: {
    positions: [
      position("AAPL", { cost_basis: 14_000, market_value: 20_000 }),
      position("NVDA", { cost_basis: 6_000, market_value: 9_000 }),
      position("XYZ"),
      position("OPT", {
        asset_class: "option-contract",
        cost_basis: 0,
        // 800 keeps the fixture's account_total arithmetic honest (44,800 =
        // 20,000 + 9,000 + 12,000 + 800 + 3,000 cash).
        market_value: 800,
        current_price: null,
      }),
    ],
    cash: 3_000,
    account_total: 44_800,
  },
};

// A pull OLDER than the run — must not render a current-holdings section.
const stalePull: HoldingsPull = {
  ...fresherPull,
  pulled_at: "2026-06-20T09:00:00Z",
};

const baseProps = {
  run: null as PortfolioRun | null,
  pull: null as HoldingsPull | null,
  loading: false,
  loadError: null as string | null,
  runError: null as string | null,
  runBlocked: false,
  runBlockedReason: null as string | null,
  pullBlocked: false,
  pullBlockedReason: null as string | null,
  busy: false,
  running: false,
  pulling: false,
  quick: null as QuickCheckState | null,
  quickChecking: false,
  historical: false,
};

function mountView(over: Partial<typeof baseProps> = {}) {
  return mount(PortfolioView, { props: { ...baseProps, ...over } });
}

// The card stack's tickers, in rendered order (scoped so table/roll-up tickers
// don't leak in).
function stackTickers(wrapper: ReturnType<typeof mountView>): string[] {
  return wrapper
    .findAll(".card-stack .holding-card")
    .map((c) => c.find(".ana-ticker").text());
}

beforeEach(() => {
  localStorage.clear();
});

describe("PortfolioView states", () => {
  test("empty state names both triggers and explains the split", () => {
    const wrapper = mountView();
    expect(wrapper.text()).toContain("No holdings yet.");
    expect(wrapper.text()).toContain("Run analysis");
    expect(wrapper.text()).toContain("Pull holdings");
  });

  test("an errored runs listing renders the unknown state, claiming neither", () => {
    // With the listing failed and nothing else loaded, whether a prior
    // analysis exists can't be told — the empty state must not assert either.
    const wrapper = mountView({ historyUnknown: true });
    expect(wrapper.text()).toContain("Portfolio state unavailable.");
    expect(wrapper.text()).not.toContain("No holdings yet.");
  });

  test("an unreadable history reads as unreadable, claiming neither", () => {
    // A row that couldn't be decoded would otherwise BE the latest view — the
    // empty state must say unreadable, not never-ran (Codex round).
    const wrapper = mountView({ unreadableHistory: true });
    expect(wrapper.text()).toContain("A prior run couldn't be read.");
    expect(wrapper.text()).not.toContain("No holdings yet.");
  });

  test("a pull over an unreadable history never claims never-analyzed", () => {
    // Pull holdings while the only prior run is unreadable: the pull and the
    // persisted (if corrupt) work coexist, so "Not yet analyzed." would be
    // false (Codex round).
    const unreadable = mountView({ pull: fresherPull, unreadableHistory: true });
    expect(unreadable.text()).toContain("A prior run couldn't be read.");
    expect(unreadable.text()).not.toContain("Not yet analyzed.");
    // The default pull-only copy is unchanged when the history is empty.
    const plain = mountView({ pull: fresherPull });
    expect(plain.text()).toContain("Not yet analyzed.");
    expect(plain.text()).toContain("Nothing is graded");
  });

  test("loading state shows while nothing is cached", () => {
    const wrapper = mountView({ loading: true });
    expect(wrapper.text()).toContain("Loading portfolio…");
  });

  test("a load error with nothing cached surfaces as an alert", () => {
    const wrapper = mountView({ loadError: "db locked" });
    expect(wrapper.find('[role="alert"]').text()).toContain("db locked");
  });

  test("pulled-not-analyzed: the compact holdings view is the page body", () => {
    const wrapper = mountView({ pull: fresherPull });
    expect(wrapper.text()).toContain("4 holdings pulled. Not yet analyzed.");
    const rows = wrapper.findAll(".ana-grid tbody tr");
    expect(rows).toHaveLength(4);
    // No churn tags without a run to compare against.
    expect(wrapper.findAll(".ana-tag")).toHaveLength(0);
  });

  test("an inline run error renders as an alert, never a card", () => {
    const wrapper = mountView({ run, runError: "Schwab account not connected" });
    expect(wrapper.find('[role="alert"]').text()).toContain(
      "Schwab account not connected"
    );
  });
});

describe("PortfolioView historical mode", () => {
  // Viewing a past run from the sidebar's runs history (docs/interface.md
  // §Main Layout): read-only — banner + back control, triggers locked with the
  // reason as title, and the latest-vintage comparison section suppressed.
  test("a historical view banners the vintage, locks the triggers, and emits back-to-latest", async () => {
    const wrapper = mountView({ run, pull: fresherPull, historical: true });
    const banner = wrapper.find(".hist-banner");
    expect(banner.exists()).toBe(true);
    expect(banner.text()).toContain("read-only");

    const buttons = wrapper.findAll(".toolbar-actions button");
    for (const b of buttons) {
      expect(b.attributes("disabled")).toBeDefined();
      expect(b.attributes("title")).toContain("past analysis");
    }

    // The fresher pull's current-holdings section is keyed to the LATEST
    // vintage, so it never renders over a historical run — and no churn tags.
    expect(wrapper.find(".current-holdings").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("no longer held");

    await banner.find(".hist-banner-back").trigger("click");
    expect(wrapper.emitted("back-to-latest")).toHaveLength(1);
  });

  test("the latest view renders no banner and keeps the triggers live", () => {
    const wrapper = mountView({ run, pull: fresherPull });
    expect(wrapper.find(".hist-banner").exists()).toBe(false);
    const buttons = wrapper.findAll(".toolbar-actions button");
    for (const b of buttons) expect(b.attributes("disabled")).toBeUndefined();
    expect(wrapper.find(".current-holdings").exists()).toBe(true);
  });

  test("a past-run open failure renders under its own label, not 'Couldn't run'", () => {
    const wrapper = mountView({ run, historyError: "run row unreadable" });
    const alert = wrapper.find('[role="alert"]');
    expect(alert.text()).toContain("Couldn't open the run");
    expect(alert.text()).toContain("run row unreadable");
    expect(alert.text()).not.toContain("Couldn't run");
  });
});

describe("PortfolioView setup tile and thesis monitor", () => {
  // B10: momentum is the market-setup read, outside the letter — its tile sits
  // set apart behind the divider, never among the three letter inputs.
  test("momentum renders as the set-apart Setup tile, not a letter input", () => {
    const wrapper = mountView({ run });
    // Both priced fixtures share sub_scores; the first block stands for all.
    const scores = wrapper.find(".hc-subscores");
    const labels = scores.findAll(".hc-sub-label").map((l) => l.text());
    expect(labels).toEqual(["quality", "valuation", "risk", "Setup"]);
    const setup = scores.find(".hc-sub-setup");
    expect(setup.find(".hc-sub-value").text()).toBe("62");
    // The outside-the-letter explanation is always visible — never hover-only
    // (Codex P2): the caption line serves every modality with one copy source.
    expect(wrapper.find(".hc-setup-note").text()).toBe(
      "Setup — market-setup read, outside the letter"
    );
    expect(setup.attributes("title")).toBeUndefined();
  });

  // B13: the ledger's bear/base/bull monitor renders as the card's scenario
  // strip with the app-stamped engine targets and the monitor-level goalposts.
  test("the thesis monitor renders scenarios, targets, and goalposts on a priced card", () => {
    const monRun: PortfolioRun = {
      ...run,
      verdicts: [
        verdict(
          "AAPL",
          { status: "priced", ...graded() },
          { thesis_ledger: ledger() }
        ),
      ],
    };
    const wrapper = mountView({ run: monRun });
    const cells = wrapper.findAll(".hc-scenario");
    expect(cells).toHaveLength(3);
    expect(cells.map((c) => c.find(".hc-kicker").text())).toEqual([
      "bear",
      "base",
      "bull",
    ]);
    expect(cells[1].find(".hc-scenario-prob").text()).toBe("55%");
    expect(cells[1].find(".hc-scenario-target").text()).toContain("210");
    expect(cells[2].text()).toContain("Margin expansion resumes");
    const goals = wrapper.find(".hc-goalposts");
    expect(goals.text()).toContain("Must improve");
    expect(goals.text()).toContain("Hardware upgrade cycle re-accelerates");
    expect(goals.text()).toContain("Must not break");
    expect(goals.text()).toContain("Services attach rate holds above 30%");
  });

  test("a role-risk card renders the condition-only monitor without target cells", () => {
    const roleRun: PortfolioRun = {
      ...run,
      holdings: {
        positions: [
          position("BND", {
            asset_class: "etf",
            cost_basis: 9_000,
            market_value: 10_000,
          }),
        ],
        cash: 0,
        account_total: 10_000,
      },
      verdicts: [
        verdict(
          "BND",
          {
            status: "role-risk-only",
            class_label: "bond fund",
            role_summary: "Core fixed-income sleeve.",
            exposure_tilt: [],
            expense_drag: null,
            observable_risk: null,
            structural_flag: false,
            is_cef: false,
            nav_premium: null,
            evidence_gaps: [],
            action: "hold",
            what_changed: "new holding",
          },
          {
            asset_class: "etf",
            thesis_ledger: ledger({
              branch: "role-risk-only",
              monitor: [
                {
                  scenario: "bear",
                  conditions: "Duration losses exceed the income cushion",
                  probability_pct: 30,
                  engine_target: null,
                },
                {
                  scenario: "base",
                  conditions: "Carry accrues; rates range-bound",
                  probability_pct: 70,
                  engine_target: null,
                },
              ],
            }),
          }
        ),
      ],
      roll_up: { ...run.roll_up, graded_count: 0, role_risk_only_count: 1 },
    };
    const wrapper = mountView({ run: roleRun });
    const cells = wrapper.findAll(".hc-scenario");
    expect(cells).toHaveLength(2);
    expect(cells[0].text()).toContain("Duration losses exceed the income cushion");
    // Structurally null targets: no target element renders on this branch.
    expect(wrapper.find(".hc-scenario-target").exists()).toBe(false);
  });

  test("no ledger means no monitor block; a partial monitor renders what exists", () => {
    // The base fixtures carry no thesis_ledger (pre-ledger/debut shape).
    const bare = mountView({ run });
    expect(bare.find(".hc-monitor").exists()).toBe(false);
    // One authored scenario and empty goalposts: the cell renders, the
    // goalpost list drops.
    const partialRun: PortfolioRun = {
      ...run,
      verdicts: [
        verdict(
          "AAPL",
          { status: "priced", ...graded() },
          {
            thesis_ledger: ledger({
              monitor: [
                {
                  scenario: "base",
                  conditions: "Services compounding holds",
                  probability_pct: 60,
                  engine_target: 200,
                },
              ],
              what_must_improve: "",
              what_must_not_break: "",
            }),
          }
        ),
      ],
    };
    const wrapper = mountView({ run: partialRun });
    expect(wrapper.findAll(".hc-scenario")).toHaveLength(1);
    expect(wrapper.find(".hc-goalposts").exists()).toBe(false);
  });

  test("the monitor renders on a historical view — it is run content, not live state", () => {
    const monRun: PortfolioRun = {
      ...run,
      verdicts: [
        verdict(
          "AAPL",
          { status: "priced", ...graded() },
          { thesis_ledger: ledger() }
        ),
      ],
    };
    const wrapper = mountView({ run: monRun, historical: true });
    expect(wrapper.find(".hc-monitor").exists()).toBe(true);
    expect(wrapper.findAll(".hc-scenario")).toHaveLength(3);
  });
});

describe("PortfolioView verdict cards", () => {
  test("renders all four cards: grades, abstention reasons, and the roll-up", () => {
    const wrapper = mountView({ run });
    expect(stackTickers(wrapper)).toHaveLength(4);
    // Graded card content.
    expect(wrapper.find(".grade.a").text()).toBe("A");
    expect(wrapper.text()).toContain("Trim");
    expect(wrapper.text()).toContain("Solid margins.");
    expect(wrapper.text()).toContain("What changed · since last run");
    // Abstentions carry their reasons, no fabricated grade.
    expect(wrapper.text()).toContain("Too few sources to grade.");
    expect(wrapper.text()).toContain("Options are not rated.");
    // Roll-up: overview + the exited position (never in the sortable stack).
    expect(wrapper.text()).toContain("Two graded holdings; one exit acknowledged.");
    expect(wrapper.text()).toContain("TSLA");
    expect(stackTickers(wrapper)).not.toContain("TSLA");
  });

  test("the position delta tag is the app's, rendered per card", () => {
    const wrapper = mountView({ run });
    const tags = wrapper.findAll(".ana-tag").map((t) => t.text());
    expect(tags).toContain("Position: Increased");
    expect(tags).toContain("Position: Unchanged");
  });

  test("target methodology is a keyboard-operable disclosure", async () => {
    const wrapper = mountView({ run });
    // Scoped to the card: the selection bar reuses the reveal primitive above the stack.
    const reveal = wrapper.findAll(".holding-card .hc-reveal")[0];
    expect(reveal.attributes("aria-expanded")).toBe("false");
    expect(wrapper.text()).not.toContain("v2 spread-anchored multiples");
    await reveal.trigger("click");
    expect(reveal.attributes("aria-expanded")).toBe("true");
    expect(wrapper.text()).toContain("v2 spread-anchored multiples");
  });

  test("a low-confidence letter carries its visible marker", () => {
    const lowConf: PortfolioRun = {
      ...run,
      verdicts: [
        verdict("AAPL", {
          status: "priced",
          ...graded({ low_confidence_grade: true }),
        }),
      ],
    };
    const wrapper = mountView({ run: lowConf });
    const tags = wrapper.findAll(".ana-tag").map((t) => t.text());
    expect(tags).toContain("Low confidence");
    // The unmarked fixture renders no marker.
    const clean = mountView({ run });
    expect(clean.findAll(".ana-tag").map((t) => t.text())).not.toContain(
      "Low confidence"
    );
  });

  test("a priced fund shows its classification and the option-overlay flag", () => {
    // The deterministic classification is shown on the card — the priced branch
    // included — and an option-overlay fund carries the structural flag beside it
    // (docs/portfolio-analysis.md §Asset eligibility).
    const overlayRun: PortfolioRun = {
      ...run,
      holdings: {
        positions: [
          position("QYLD", {
            asset_class: "etf",
            cost_basis: 9_000,
            market_value: 10_000,
          }),
        ],
        cash: 0,
        account_total: 10_000,
      },
      verdicts: [
        verdict(
          "QYLD",
          {
            status: "priced",
            ...graded({
              low_confidence_grade: true,
              fund_class_label: "US equity fund",
              structural_flag: true,
            }),
          },
          { asset_class: "etf" }
        ),
      ],
    };
    const wrapper = mountView({ run: overlayRun });
    expect(wrapper.text()).toContain("US equity fund · reduced verdict");
    const tags = wrapper.findAll(".ana-tag").map((t) => t.text());
    expect(tags).toContain("Structurally path-dependent");
    // A stock (null classification) renders neither.
    const clean = mountView({ run });
    expect(clean.text()).toContain("Stock · full verdict");
    expect(clean.findAll(".ana-tag").map((t) => t.text())).not.toContain(
      "Structurally path-dependent"
    );
  });

  test("a role-risk-only verdict renders its own card branch, never priced placeholders", () => {
    const roleRun: PortfolioRun = {
      ...run,
      holdings: {
        positions: [
          position("BND", {
            asset_class: "etf",
            cost_basis: 9_000,
            market_value: 10_000,
          }),
        ],
        cash: 0,
        account_total: 10_000,
      },
      verdicts: [
        verdict(
          "BND",
          {
            status: "role-risk-only",
            class_label: "bond fund",
            role_summary: "Core fixed-income sleeve supplying duration exposure.",
            exposure_tilt: [{ label: "United States", weight: 0.97 }],
            expense_drag: 0.0003,
            observable_risk: 0.06,
            structural_flag: false,
            is_cef: false,
            nav_premium: null,
            evidence_gaps: ["no on-plan duration/credit surface"],
            action: "hold",
            what_changed: "new holding",
          },
          { asset_class: "etf" }
        ),
      ],
      roll_up: { ...run.roll_up, graded_count: 0, role_risk_only_count: 1 },
    };
    const wrapper = mountView({ run: roleRun });
    // The typed branch: role read, classification, exposure, gaps, reduced action.
    expect(wrapper.text()).toContain("Role & risk");
    expect(wrapper.text()).toContain("bond fund · role / risk read");
    expect(wrapper.text()).toContain("Core fixed-income sleeve");
    expect(wrapper.text()).toContain("no on-plan duration/credit surface");
    expect(wrapper.text()).toContain("Hold");
    // No letter, no targets — the branch never renders priced placeholders.
    expect(wrapper.find(".hc-grade").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("12-mo target");
    // The key-figure strip counts the branch in its own tile.
    expect(wrapper.text()).toContain("Role/risk");
  });

  test("the closed-end price-vs-NAV row renders only when the read exists", () => {
    // The CEF card line (ruled 2026-08-21): a present read renders signed with
    // its premium/discount word, a rendered zero reads "at par" (rounding is
    // half-away-from-zero to match the backend prompt line), and the row is
    // absent on the gap case and on any non-CEF fund.
    const cefRun = (nav_premium: number | null, is_cef = true): PortfolioRun => ({
      ...run,
      holdings: {
        positions: [
          position("PDI", {
            asset_class: "etf",
            cost_basis: 9_000,
            market_value: 10_000,
          }),
        ],
        cash: 0,
        account_total: 10_000,
      },
      verdicts: [
        verdict(
          "PDI",
          {
            status: "role-risk-only",
            class_label: "closed-end fund",
            role_summary: "Income sleeve.",
            exposure_tilt: [],
            expense_drag: null,
            observable_risk: null,
            structural_flag: false,
            is_cef,
            nav_premium,
            evidence_gaps: [],
            action: "hold",
            what_changed: "new holding",
          },
          { asset_class: "etf" }
        ),
      ],
      roll_up: { ...run.roll_up, graded_count: 0, role_risk_only_count: 1 },
    });

    let wrapper = mountView({ run: cefRun(-0.072) });
    expect(wrapper.text()).toContain("Price vs NAV");
    expect(wrapper.text()).toContain("-7.2%");
    expect(wrapper.text()).toContain("discount");
    wrapper = mountView({ run: cefRun(0.031) });
    expect(wrapper.text()).toContain("+3.1%");
    expect(wrapper.text()).toContain("premium");
    // The exact negative half rounds away from zero — bare Math.round would
    // read "at par" here and split from the backend's label (Codex round 5).
    wrapper = mountView({ run: cefRun(-0.0005) });
    expect(wrapper.text()).toContain("-0.1%");
    expect(wrapper.text()).toContain("discount");
    // A value rendering as 0.0% reads at par, never a signed zero.
    wrapper = mountView({ run: cefRun(-0.0004) });
    expect(wrapper.text()).toContain("0.0%");
    expect(wrapper.text()).toContain("at par");
    // The gap case renders no row at all, and a non-CEF never renders one.
    wrapper = mountView({ run: cefRun(null) });
    expect(wrapper.text()).not.toContain("Price vs NAV");
    wrapper = mountView({ run: cefRun(0.002, false) });
    expect(wrapper.text()).not.toContain("Price vs NAV");
  });

  test("the header position block carries price, avg cost, and cost basis beside the gain", () => {
    // AAPL: price 120, avg cost 14,000 / 100 = 140, cost basis 14,000.
    const moneyExact = new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: "USD",
      maximumFractionDigits: 2,
    });
    const money = new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: "USD",
      maximumFractionDigits: 0,
    });
    const wrapper = mountView({ run });
    const aapl = wrapper
      .findAll(".card-stack .holding-card")
      .find((c) => c.find(".ana-ticker").text() === "AAPL")!;
    const block = aapl.find(".hc-position");
    expect(block.exists()).toBe(true);
    const text = block.text();
    expect(text).toContain("Price");
    expect(text).toContain("Avg cost");
    expect(text).toContain("Cost basis");
    expect(text).toContain("Unrealized");
    expect(text).toContain(moneyExact.format(120));
    expect(text).toContain(moneyExact.format(140));
    expect(text).toContain(money.format(14_000));
  });

  test("option rows withhold cost basis and gain until the multiplier is probed", () => {
    // Schwab's averagePrice carries a contract/par multiplier the parse doesn't
    // apply — averagePrice 3.5 × qty 2 wires cost_basis 7 for a $700 position,
    // so the derived $/% gain would be wildly wrong. Withheld, not fabricated.
    const pull: HoldingsPull = {
      ...fresherPull,
      holdings: {
        positions: [
          position("AAPL", { cost_basis: 14_000, market_value: 20_000 }),
          position("OPT2", {
            asset_class: "option-contract",
            cost_basis: 7,
            market_value: 800,
            current_price: null,
          }),
        ],
        cash: 3_000,
        account_total: 23_800,
      },
    };
    const wrapper = mountView({ run, pull });
    const optRow = wrapper
      .findAll(".ana-grid tbody tr")
      .find((r) => r.text().includes("OPT2"))!;
    const cells = optRow.findAll("td").map((c) => c.text().trim());
    expect(cells).not.toContain("$7.00");
    const gainCell = cells[cells.length - 1];
    expect(gainCell).toBe("—");
  });

  test("sub-rounding gains render an unsigned zero, never a signed-zero artifact", () => {
    // −0.0004 rounds to 0.0 at one decimal — "-0.0%" (red) is a signed-zero
    // artifact; the rendered value keys the sign.
    const pull: HoldingsPull = {
      ...fresherPull,
      holdings: {
        positions: [position("AAPL", { cost_basis: 10_000, market_value: 9_996 })],
        cash: 3_000,
        account_total: 12_996,
      },
    };
    const wrapper = mountView({ run, pull });
    const row = wrapper
      .findAll(".ana-grid tbody tr")
      .find((r) => r.text().includes("AAPL"))!;
    const cells = row.findAll("td");
    const gainCell = cells[cells.length - 1];
    expect(gainCell.text().trim()).toBe("0.0%");
    // Direction keys on the rendered value: a cell reading "0.0%" must not
    // wear the red/down treatment its own number no longer shows.
    expect(gainCell.find(".down").exists()).toBe(false);
  });

  test("unreported position inputs render as dashes, never fabricated numbers", () => {
    const bare: PortfolioRun = {
      ...run,
      holdings: {
        positions: [position("AAPL", { cost_basis: 0, current_price: null })],
        cash: 0,
        account_total: 12_000,
      },
      verdicts: [verdict("AAPL", { status: "priced", ...graded() })],
    };
    const wrapper = mountView({ run: bare });
    const cells = wrapper.findAll(".hc-position dd").map((d) => d.text());
    expect(cells).toEqual(["—", "—", "—", "—"]);
  });

  test("a net-short position renders the reduced card — never a position block", () => {
    // The position block's cost guards assume long rows: the engine short-circuits
    // any net-short position to not-rated before class routing (pipeline.rs
    // eligibility), so no full-card branch ever pairs with a signed-negative
    // position row. Pin that seam: if short routing ever changes, this fails
    // before the block silently dashes a legitimate short's cost figures.
    const shortRun: PortfolioRun = {
      ...run,
      holdings: {
        positions: [
          position("XYZ", {
            quantity: -40,
            cost_basis: -4_000,
            market_value: -4_800,
            current_price: 120,
          }),
        ],
        cash: 0,
        account_total: 10_000,
      },
      verdicts: [
        verdict("XYZ", {
          status: "not-rated",
          reason: "held net short — the ladder's long-side semantics don't apply",
        }),
      ],
    };
    const wrapper = mountView({ run: shortRun });
    expect(wrapper.text()).toContain("held net short");
    expect(wrapper.find(".hc-position").exists()).toBe(false);
    expect(wrapper.find(".hc-grade").exists()).toBe(false);
  });

  test("the action renders rung-only — no weight band or share/dollar figures", () => {
    // Tunnel vision (portfolio-v9): sizing is the future planner's job, so the
    // card carries the rung and its rationale alone.
    const wrapper = mountView({ run });
    expect(wrapper.find(".hc-action-word").exists()).toBe(true);
    expect(wrapper.find(".hc-action-band").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("Est. shares");
    expect(wrapper.text()).not.toContain("Est. adj.");
  });

  test("the IV skew row names its put − call convention and keys its sign on the rendered value", () => {
    // The row printed the signed skew under a bare "IV skew" label — the same
    // ambiguity the prompt carried (large-scale review 2026-08-24, P1 minor) —
    // and keyed its "+" on the raw fraction, so +0.0003 read "+0.0%".
    const withSkew = (iv_skew: number | null): PortfolioRun => ({
      ...run,
      verdicts: [
        verdict("AAPL", {
          status: "priced",
          ...graded({
            options_signal: {
              put_call_volume: 1.2,
              put_call_open_interest: 1.1,
              implied_volatility: 0.3,
              iv_skew,
            },
          }),
        }),
      ],
    });
    const skewValue = (iv_skew: number | null): string | undefined => {
      const dt = mountView({ run: withSkew(iv_skew) })
        .findAll(".hc-actionrow .hc-kv dt")
        .find((d) => d.text().includes("IV skew"));
      if (!dt) return undefined;
      expect(dt.text()).toBe("Put − call IV skew");
      return dt.element.nextElementSibling?.textContent?.trim();
    };
    expect(skewValue(0.03)).toBe("+3.0%");
    expect(skewValue(-0.02)).toBe("-2.0%");
    // A skew that rounds away carries no sign.
    expect(skewValue(0.0003)).toBe("0.0%");
    // No skew, no row — the other three rows still render.
    expect(skewValue(null)).toBeUndefined();
    const siblings = mountView({ run: withSkew(null) }).text();
    for (const label of ["Put/call vol", "Put/call OI", "ATM IV"]) {
      expect(siblings).toContain(label);
    }
  });
});

describe("PortfolioView fresher pull (presence-only churn)", () => {
  test("a fresher pull renders the stamped current-holdings section with tags", () => {
    const wrapper = mountView({ run, pull: fresherPull });
    const section = wrapper.find(".current-holdings");
    expect(section.exists()).toBe(true);
    expect(section.text()).toContain("Pulled");
    expect(section.text()).toContain("analysis from");
    // NVDA is in the pull but not the analyzed snapshot.
    expect(section.text()).toContain("New · not in last analysis");
    // MSFT's run-anchored card stays, tagged — never removed.
    const msft = wrapper
      .findAll(".card-stack .holding-card")
      .find((c) => c.text().includes("MSFT"));
    expect(msft?.text()).toContain("No longer held");
    expect(stackTickers(wrapper)).toContain("MSFT");
  });

  test("an older pull renders no current-holdings section and no tags", () => {
    const wrapper = mountView({ run, pull: stalePull });
    expect(wrapper.find(".current-holdings").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("No longer held");
  });
});

describe("PortfolioView sort bar", () => {
  test("defaults to overall value, descending", () => {
    const wrapper = mountView({ run });
    expect(stackTickers(wrapper)).toEqual(["MSFT", "AAPL", "XYZ", "OPT"]);
    const active = wrapper.find('.ana-sortbar button[aria-pressed="true"]');
    expect(active.attributes("aria-label")).toBe("Sort by Value, descending");
  });

  test("a no-cost-basis position sorts last on the gain keys, any direction", async () => {
    const wrapper = mountView({ run });
    const pctButton = wrapper
      .findAll(".ana-sortbar button")
      .find((b) => b.text().includes("% gain"))!;
    await pctButton.trigger("click");
    // Desc: XYZ +150% > AAPL +39.3% > MSFT −6.25% > OPT (undefined) last.
    expect(stackTickers(wrapper)).toEqual(["XYZ", "AAPL", "MSFT", "OPT"]);
    expect(pctButton.attributes("aria-label")).toBe("Sort by % gain, descending");
    // Re-click flips to ascending; the undefined key still sorts last.
    await pctButton.trigger("click");
    expect(stackTickers(wrapper)).toEqual(["MSFT", "AAPL", "XYZ", "OPT"]);
    expect(pctButton.attributes("aria-label")).toBe("Sort by % gain, ascending");
  });

  test("a negative netted basis keeps its dollar gain — sortable and rendered without a percentage", async () => {
    // A net-short book: the short side's proceeds push the netted basis below
    // zero (docs/portfolio-analysis.md §Storage and display). The dollar gain
    // stays defined — market value − cost basis is the book's aggregate
    // unrealized P/L — while %gain and cash-invested stay undefined and sort
    // last. Pins the fix for the `<= 0` guard that conflated a negative basis
    // with "no basis reported".
    const shortRun: PortfolioRun = {
      ...run,
      holdings: {
        ...run.holdings,
        positions: [
          ...positions,
          // market_value −1_000 (short 100 shares), basis −1_500 (proceeds):
          // dollar gain = −1_000 − (−1_500) = +500 — third under $ gain desc
          // (AAPL +5_500 > XYZ +3_000 > SHRT +500 > MSFT −2_000 > OPT null).
          position("SHRT", {
            quantity: -100,
            cost_basis: -1_500,
            market_value: -1_000,
          }),
        ],
      },
      verdicts: [
        ...run.verdicts,
        verdict("SHRT", { status: "not-rated", reason: "Net short position." }),
      ],
    };
    const wrapper = mountView({ run: shortRun });
    const gainButton = wrapper
      .findAll(".ana-sortbar button")
      .find((b) => b.text().includes("$ gain"))!;
    await gainButton.trigger("click");
    // SHRT ranks by its defined +500 gain — never lumped with the undefined
    // no-basis case. (Its card is the reduced not-rated branch, which carries
    // no Unrealized figure — the gain's render home is the sort rank here.)
    expect(stackTickers(wrapper)).toEqual(["AAPL", "XYZ", "SHRT", "MSFT", "OPT"]);
    // Undefined keys still sort it last alongside the no-basis option.
    const pctButton = wrapper
      .findAll(".ana-sortbar button")
      .find((b) => b.text().includes("% gain"))!;
    await pctButton.trigger("click");
    expect(stackTickers(wrapper).slice(-2).sort()).toEqual(["OPT", "SHRT"]);
  });

  test("the last-used key persists in localStorage and seeds the next mount", async () => {
    const first = mountView({ run });
    const costButton = first
      .findAll(".ana-sortbar button")
      .find((b) => b.text().includes("Cash invested"))!;
    await costButton.trigger("click");
    first.unmount();

    const second = mountView({ run });
    const active = second.find('.ana-sortbar button[aria-pressed="true"]');
    expect(active.attributes("aria-label")).toBe(
      "Sort by Cash invested, descending"
    );
    // Desc by cost basis: MSFT 32k > AAPL 14k > XYZ 2k > OPT (none) last.
    expect(stackTickers(second)).toEqual(["MSFT", "AAPL", "XYZ", "OPT"]);
  });
});

// The pull table's tickers, in rendered order (scoped to the current-holdings
// section so card/roll-up tickers don't leak in).
function tableTickers(wrapper: ReturnType<typeof mountView>): string[] {
  return wrapper
    .findAll(".current-holdings tbody tr")
    .map((r) => r.find(".ana-ticker").text());
}

function headButton(wrapper: ReturnType<typeof mountView>, label: string) {
  return wrapper
    .findAll(".current-holdings thead button")
    .find((b) => b.text() === label)!;
}

describe("PortfolioView current-holdings table", () => {
  test("renders price and % gain columns, no description; missing values show an em dash", () => {
    const wrapper = mountView({ pull: fresherPull });
    const heads = wrapper
      .findAll(".current-holdings thead th")
      .map((h) => h.text());
    expect(heads.join(" ")).toContain("Price");
    expect(heads.join(" ")).toContain("% gain");
    expect(heads.join(" ")).not.toContain("Description");
    // Column order: Symbol · Qty · Price · Market value · Cost basis · % gain.
    const rows = wrapper.findAll(".current-holdings tbody tr");
    expect(rows[0].findAll("td")[2].text()).toContain("120");
    expect(rows[3].findAll("td")[2].text()).toBe("—");
    // % gain rides the directional token: signed value + a non-color glyph.
    const aaplGain = rows[0].findAll("td")[5].find(".dir");
    expect(aaplGain.text()).toBe("+42.9%");
    expect(aaplGain.classes()).toContain("up");
    expect(rows[3].findAll("td")[5].text()).toBe("—");
  });

  test("price and % gain sort with their missing values last", async () => {
    const wrapper = mountView({ pull: fresherPull });
    await headButton(wrapper, "% gain").trigger("click");
    // Desc: NVDA +50% > AAPL +42.9% > XYZ +20% > OPT (no cost basis) last.
    expect(tableTickers(wrapper)).toEqual(["NVDA", "AAPL", "XYZ", "OPT"]);
    await headButton(wrapper, "% gain").trigger("click");
    expect(tableTickers(wrapper)).toEqual(["XYZ", "AAPL", "NVDA", "OPT"]);
    // Price: the three priced names tie at 120 (ticker tie-break); OPT last.
    await headButton(wrapper, "Price").trigger("click");
    expect(tableTickers(wrapper)).toEqual(["AAPL", "NVDA", "XYZ", "OPT"]);
  });

  test("defaults to the as-pulled order with no aria-sort anywhere", () => {
    const wrapper = mountView({ pull: fresherPull });
    expect(tableTickers(wrapper)).toEqual(["AAPL", "NVDA", "XYZ", "OPT"]);
    expect(wrapper.findAll(".current-holdings th[aria-sort]")).toHaveLength(0);
  });

  test("symbol opens ascending, carries aria-sort, and flips on re-click", async () => {
    const wrapper = mountView({ pull: fresherPull });
    const symbol = headButton(wrapper, "Symbol");
    await symbol.trigger("click");
    expect(tableTickers(wrapper)).toEqual(["AAPL", "NVDA", "OPT", "XYZ"]);
    const ascHead = wrapper.find('.current-holdings th[aria-sort="ascending"]');
    expect(ascHead.text()).toContain("Symbol");
    // The active head carries the package's visible active-sort treatment.
    expect(ascHead.classes()).toContain("sorted-asc");
    expect(symbol.attributes("aria-label")).toBe("Sort by Symbol, ascending");
    await symbol.trigger("click");
    expect(tableTickers(wrapper)).toEqual(["XYZ", "OPT", "NVDA", "AAPL"]);
    expect(
      wrapper.find('.current-holdings th[aria-sort="descending"]').text()
    ).toContain("Symbol");
  });

  test("a money column opens descending; a missing cost basis sorts last, any direction", async () => {
    const wrapper = mountView({ pull: fresherPull });
    const cost = headButton(wrapper, "Cost basis");
    await cost.trigger("click");
    // Desc: AAPL 14k > XYZ 10k > NVDA 6k > OPT (none) last.
    expect(tableTickers(wrapper)).toEqual(["AAPL", "XYZ", "NVDA", "OPT"]);
    await cost.trigger("click");
    expect(tableTickers(wrapper)).toEqual(["NVDA", "XYZ", "AAPL", "OPT"]);
  });

  test("the table sort persists independently of the card sort", async () => {
    const first = mountView({ run, pull: fresherPull });
    await headButton(first, "Market value").trigger("click");
    // Desc: AAPL 20k > XYZ 12k > NVDA 9k > OPT 800.
    expect(tableTickers(first)).toEqual(["AAPL", "XYZ", "NVDA", "OPT"]);
    // The card stack keeps its own default (value, descending) — untouched.
    expect(stackTickers(first)).toEqual(["MSFT", "AAPL", "XYZ", "OPT"]);
    first.unmount();

    const second = mountView({ run, pull: fresherPull });
    expect(tableTickers(second)).toEqual(["AAPL", "XYZ", "NVDA", "OPT"]);
    expect(
      second.find('.current-holdings th[aria-sort="descending"]').text()
    ).toContain("Market value");
  });
});

describe("PortfolioView trigger gating", () => {
  test("presence locks disable each trigger with its reason", () => {
    const wrapper = mountView({
      runBlocked: true,
      runBlockedReason: "Not configured: daemon endpoint.",
      pullBlocked: true,
      pullBlockedReason: "Schwab account not connected.",
    });
    const [pull, quick, runBtn] = wrapper.findAll(".toolbar-actions button");
    expect(pull.attributes("disabled")).toBeDefined();
    expect(pull.attributes("title")).toContain("Schwab account not connected");
    expect(quick.attributes("disabled")).toBeDefined();
    expect(quick.attributes("title")).toContain("daemon endpoint");
    expect(runBtn.attributes("disabled")).toBeDefined();
    expect(runBtn.attributes("title")).toContain("daemon endpoint");
  });

  test("the pull trigger works while only the run is model-blocked", async () => {
    const wrapper = mountView({
      runBlocked: true,
      runBlockedReason: "Not configured: reasoner model.",
    });
    const [pull, , runBtn] = wrapper.findAll(".toolbar-actions button");
    expect(runBtn.attributes("disabled")).toBeDefined();
    expect(pull.attributes("disabled")).toBeUndefined();
    await pull.trigger("click");
    expect(wrapper.emitted("pull")).toHaveLength(1);
    expect(wrapper.emitted("run")).toBeUndefined();
  });

  test("a busy run slot disables every trigger", () => {
    const wrapper = mountView({ busy: true });
    for (const b of wrapper.findAll(".toolbar-actions button")) {
      expect(b.attributes("disabled")).toBeDefined();
    }
  });
});

describe("PortfolioView quick check", () => {
  function quickState(
    holdings: Partial<HoldingQuickState>[],
    sweptRunId = "prun-1"
  ): QuickCheckState {
    return {
      swept_run_id: sweptRunId,
      last_checked_at: "2026-08-03T10:00:00Z",
      holdings: holdings.map((h) => ({
        symbol: "AAPL",
        families: [],
        flag: null,
        evidence_events: [],
        condition_states: [],
        last_hurdle_state: null,
        notes: [],
        ...h,
      })),
    };
  }
  const flagged = quickState([
    {
      symbol: "AAPL",
      flag: {
        trigger: "confirmed-falsifier-breach",
        detail: "confirmed falsifier breach: operating margin below 30%",
        raised_at: "2026-08-03T10:00:00Z",
      },
      evidence_events: [
        {
          kind: "earnings-actual",
          detail: "earnings actual reported 2026-07-30",
          observed_at: "2026-08-03T10:00:00Z",
        },
      ],
      families: [
        { family: "market-data", state: "flagged", note: null },
        { family: "filing", state: "unknown", note: "EDGAR sweep failed" },
      ],
    },
  ]);

  test("the quick-check trigger needs a run to sweep, then emits", async () => {
    const empty = mountView();
    const quickBtn = () =>
      empty.findAll(".toolbar-actions button").at(1)!;
    expect(quickBtn().attributes("disabled")).toBeDefined();
    expect(quickBtn().attributes("title")).toContain("Run an analysis first");

    // A corrupt-only history (audit L3): a run exists but no readable ledger
    // does — the lock explains unreadable, never "run first".
    const unreadable = mountView({ unreadableHistory: true });
    const unreadableBtn = unreadable.findAll(".toolbar-actions button").at(1)!;
    expect(unreadableBtn.attributes("disabled")).toBeDefined();
    expect(unreadableBtn.attributes("title")).toContain("couldn't be read");
    expect(unreadableBtn.attributes("title")).not.toContain("Run an analysis first");

    const wrapper = mountView({ run });
    const btn = wrapper.findAll(".toolbar-actions button").at(1)!;
    expect(btn.text()).toBe("Quick check");
    expect(btn.attributes("disabled")).toBeUndefined();
    await btn.trigger("click");
    expect(wrapper.emitted("quick-check")).toHaveLength(1);
  });

  test("a flagged holding carries the amber attention tag plus the quiet badges", () => {
    const wrapper = mountView({ run, quick: flagged });
    const card = wrapper
      .findAll(".card-stack .holding-card")
      .find((c) => c.find(".ana-ticker").text() === "AAPL")!;
    const attention = card.find(".dh-attention-tag");
    expect(attention.exists()).toBe(true);
    expect(attention.text()).toContain("falsifier breached");
    expect(attention.attributes("title")).toContain("operating margin below 30%");
    // The quiet informational badges — never the amber treatment.
    const quiet = card
      .findAll(".ana-tag")
      .filter((t) => !t.classes().includes("dh-attention-tag"));
    const texts = quiet.map((t) => t.text());
    expect(texts).toContain("Evidence event");
    expect(texts).toContain("Sweep degraded");
    const degraded = quiet.find((t) => t.text() === "Sweep degraded")!;
    expect(degraded.attributes("title")).toContain("filing");
    // The unflagged sibling cards carry no overlay.
    const msft = wrapper
      .findAll(".card-stack .holding-card")
      .find((c) => c.find(".ana-ticker").text() === "MSFT")!;
    expect(msft.find(".dh-attention-tag").exists()).toBe(false);
  });

  test("the overlay applies only to the run it swept and never to a past view", () => {
    // A state swept against a superseded run renders nothing.
    const stale = mountView({
      run,
      quick: quickState(flagged.holdings, "prun-0"),
    });
    expect(stale.find(".dh-attention-tag").exists()).toBe(false);
    // A historical view renders nothing even when the ids match.
    const historical = mountView({ run, quick: flagged, historical: true });
    expect(historical.find(".dh-attention-tag").exists()).toBe(false);
  });
});

describe("PortfolioView selective re-analysis", () => {
  const boxFor = (wrapper: ReturnType<typeof mountView>, sym: string) =>
    wrapper
      .findAll(".hc-select-input")
      .find(
        (b) => b.attributes("aria-label") === `Select ${sym} for re-analysis`
      )!;

  test("per-card selection retitles the Run trigger and rides the run emit", async () => {
    const wrapper = mountView({ run });
    // Every card variant is selectable — graded, abstained, and not-rated alike.
    expect(wrapper.findAll(".hc-select-input")).toHaveLength(4);
    await boxFor(wrapper, "AAPL").setValue(true);
    await boxFor(wrapper, "XYZ").setValue(true);
    const runBtn = wrapper.find(".toolbar-actions .btn-primary");
    expect(runBtn.text()).toBe("Analyze 2 selected");
    await runBtn.trigger("click");
    const emitted = wrapper.emitted("run");
    expect(emitted).toHaveLength(1);
    expect([...(emitted![0][0] as string[])].sort()).toEqual(["AAPL", "XYZ"]);
  });

  test("without a selection the run emit carries no payload", async () => {
    const wrapper = mountView({ run });
    await wrapper.find(".toolbar-actions .btn-primary").trigger("click");
    expect(wrapper.emitted("run")![0][0]).toBeUndefined();
  });

  test("select all and clear drive the whole stack", async () => {
    const wrapper = mountView({ run });
    const [selectAllBtn, clearBtn] = wrapper.findAll(".hc-selectbar-btn");
    await selectAllBtn.trigger("click");
    expect(wrapper.find(".toolbar-actions .btn-primary").text()).toBe(
      "Analyze 4 selected"
    );
    expect(wrapper.find(".hc-selectbar-count").text()).toBe("4 selected");
    await clearBtn.trigger("click");
    expect(wrapper.find(".toolbar-actions .btn-primary").text()).toBe(
      "Run analysis"
    );
    expect(wrapper.find(".hc-selectbar-count").text()).toBe("");
  });

  test("the selection resets when the rendered run changes", async () => {
    const wrapper = mountView({ run });
    await boxFor(wrapper, "AAPL").setValue(true);
    expect(wrapper.find(".toolbar-actions .btn-primary").text()).toContain(
      "selected"
    );
    await wrapper.setProps({ run: { ...run, run_id: "prun-2" } });
    expect(wrapper.find(".toolbar-actions .btn-primary").text()).toBe(
      "Run analysis"
    );
  });

  test("selection is absent on a historical view and disabled while busy", () => {
    const hist = mountView({ run, historical: true });
    expect(hist.findAll(".hc-select-input")).toHaveLength(0);
    expect(hist.find(".hc-selectbar").exists()).toBe(false);
    const busy = mountView({ run, busy: true });
    for (const b of busy.findAll(".hc-select-input")) {
      expect(b.attributes("disabled")).toBeDefined();
    }
  });

  test("a carried verdict shows its vintage stamp — stale past the carry window — and the demotion tag", () => {
    const carriedRun: PortfolioRun = {
      ...run,
      verdicts: [
        // Carried 6 days back: the quiet vintage stamp.
        verdict(
          "MSFT",
          { status: "priced", ...graded() },
          { analyzed_at: "2026-06-25T12:00:00Z" }
        ),
        // Carried 61 days back with a demoted add: stale + the demotion tag.
        verdict(
          "AAPL",
          { status: "priced", ...graded({ action: "hold" }) },
          { analyzed_at: "2026-05-01T12:00:00Z", action_source: "rule-demoted" }
        ),
        // Stamped with the run's own created_at: no stamp at all.
        verdict(
          "XYZ",
          { status: "insufficient-evidence", reason: "thin" },
          { analyzed_at: run.created_at }
        ),
      ],
    };
    const wrapper = mountView({ run: carriedRun });
    const tags = wrapper.findAll(".ana-tag").map((t) => t.text());
    expect(tags.some((t) => t.startsWith("Carried · analyzed"))).toBe(true);
    expect(tags.some((t) => t.startsWith("Stale · analyzed"))).toBe(true);
    expect(tags).toContain("Add demoted to hold");
    const xyzCard = wrapper
      .findAll(".holding-card")
      .find((c) => c.text().includes("XYZ"))!;
    expect(xyzCard.text()).not.toContain("analyzed");
  });

  test("an unreadable vintage tags stale — vintage unknown — matching the engine, never a NaN date", () => {
    // Audit L1: the engine reads an unparseable `analyzed_at` as over-age
    // (`job.rs over_age`); the card must not lose the stale warning, and the
    // garbage stamp must never render (localDate would print NaN-NaN-NaN).
    const malformedRun: PortfolioRun = {
      ...run,
      verdicts: [
        verdict(
          "MSFT",
          { status: "priced", ...graded() },
          { analyzed_at: "soon" }
        ),
      ],
    };
    const wrapper = mountView({ run: malformedRun });
    const tag = wrapper
      .findAll(".ana-tag")
      .find((t) => t.text() === "Stale · vintage unknown")!;
    expect(tag.exists()).toBe(true);
    expect(tag.attributes("title")).toContain("could not be read");
    expect(tag.classes()).not.toContain("dh-attention-tag");
    expect(wrapper.text()).not.toContain("NaN");
    expect(wrapper.text()).not.toContain("soon");
  });

  test("the stale boundary counts whole ET session days, matching the engine", () => {
    // Analyzed 2026-06-03 00:30 EDT (04:30Z); run 2026-07-01 23:00 EDT
    // (07-02 03:00Z): exactly 28 whole ET days — inside the carry window, so
    // the quiet Carried tag, never Stale. The old fractional-ms read (~28.9
    // days) called this stale a day early, disagreeing with the engine's
    // ET date-diff (`job.rs over_age`).
    const boundaryRun: PortfolioRun = {
      ...run,
      created_at: "2026-07-02T03:00:00Z",
      verdicts: [
        verdict(
          "MSFT",
          { status: "priced", ...graded() },
          { analyzed_at: "2026-06-03T04:30:00Z" }
        ),
      ],
    };
    const wrapper = mountView({ run: boundaryRun });
    const tags = wrapper.findAll(".ana-tag").map((t) => t.text());
    expect(tags.some((t) => t.startsWith("Carried · analyzed"))).toBe(true);
    expect(tags.some((t) => t.startsWith("Stale"))).toBe(false);
  });

  test("a rule-demoted role-risk verdict shows the demotion tag on its own branch", () => {
    // The backend demotion is branch-unscoped (piece-2 A2); a demoted role-risk
    // hold with no tag would read as the model's standing choice.
    const roleRun: PortfolioRun = {
      ...run,
      holdings: {
        positions: [position("BND", { asset_class: "etf", market_value: 10_000 })],
        cash: 0,
        account_total: 10_000,
      },
      verdicts: [
        verdict(
          "BND",
          {
            status: "role-risk-only",
            class_label: "bond fund",
            role_summary: "Core fixed-income sleeve.",
            exposure_tilt: [],
            expense_drag: null,
            observable_risk: null,
            structural_flag: false,
            is_cef: false,
            nav_premium: null,
            evidence_gaps: [],
            action: "hold",
            what_changed: "carried",
          },
          {
            asset_class: "etf",
            analyzed_at: "2026-05-01T12:00:00Z",
            action_source: "rule-demoted",
          }
        ),
      ],
    };
    const wrapper = mountView({ run: roleRun });
    const tags = wrapper.findAll(".ana-tag").map((t) => t.text());
    expect(tags).toContain("Add demoted to hold");
  });
});

describe("PortfolioView per-holding action", () => {
  // Tunnel-vision runs (portfolio-v9): the action call's rationale renders
  // under the rung.
  const actionRun: PortfolioRun = {
    ...run,
    verdicts: [
      verdict("MSFT", {
        status: "priced",
        ...graded({
          action: "trim",
          action_rationale: "Dead-money read plus a weak forward outlook.",
        }),
      }),
      verdict("AAPL", {
        status: "priced",
        ...graded({ action: "hold" }),
      }),
    ],
  };

  test("the action call's rationale renders under the rung", () => {
    const wrapper = mountView({ run: actionRun });
    const msft = wrapper
      .findAll(".holding-card")
      .find((c) => c.text().includes("MSFT"))!;
    expect(msft.find(".hc-rationale").text()).toContain("weak forward outlook");
    const aapl = wrapper
      .findAll(".holding-card")
      .find((c) => c.find(".ana-ticker").text() === "AAPL")!;
    expect(aapl.find(".hc-rationale").exists()).toBe(false);
  });
});

// ---- The two-arm verdict (portfolio-v7) --------------------------------------
// Engine baseline | model view paired columns, the full-width action strip, the
// model retrospective, and the roll-up's scoreboard + engine-bound annotations.

function twoArmGraded(over: Partial<GradedVerdict> = {}): GradedVerdict {
  return graded({
    action: "add",
    model_view: {
      sub_scores: { quality: 88, valuation: 35, momentum: 70, risk: 60 },
      letter: "C",
      price_targets: {
        one_month: { base: 215, bear: 200, bull: 230 },
        twelve_month: { base: 280, bear: 190, bull: 340 },
      },
      self_assessment: "First read for this holding — no prior call to assess.",
    },
    engine_view: {
      outlook: { short: "bearish", mid: "neutral", long: "bearish" },
      conviction: "low",
      action: "hold",
    },
    ...over,
  });
}

describe("PortfolioView two-arm verdict", () => {
  test("a v7 card renders the paired arms, the action strip, and the retrospective", () => {
    const wrapper = mountView({
      run: {
        ...run,
        verdicts: [verdict("AAPL", { status: "priced", ...twoArmGraded() })],
      },
    });
    // Located by ticker: the book's other holdings (MSFT, XYZ, OPT) have no
    // verdict here, so their not-analyzed placeholders sort into the same
    // stack by value and MSFT's stands first.
    const card = wrapper
      .findAll(".card-stack .holding-card")
      .find((c) => c.find(".ana-ticker").text() === "AAPL")!;
    const kickers = card.findAll(".hc-kicker").map((k) => k.text());
    expect(kickers.some((k) => k.startsWith("Engine baseline"))).toBe(true);
    expect(kickers.some((k) => k.startsWith("Model view"))).toBe(true);
    expect(kickers).toContain("Portfolio action");
    expect(kickers).toContain("Model retrospective");
    // The model letter chip is derived from the model's own scores and carries
    // the grade-scale tint class the design system's `.grade.c` compound reads
    // (the same `gradeClass()` binding as the engine chip).
    expect(card.find(".hc-model-letter").text()).toBe("C");
    expect(card.find(".hc-model-letter").classes()).toContain("c");
    // The engine column carries the stand-in action (rung-only).
    const engineCol = card.find(".hc-col-intrinsic");
    expect(engineCol.text()).toContain("Hold");
    // Model values render as authored beside the engine's.
    expect(card.text()).toContain("$280.00");
    // Divergent conviction, outlook, and action each carry the quiet ≠ engine
    // tag (the fixture's model outlook differs from the engine stand-in's).
    const tags = card.findAll(".ana-tag").map((t) => t.text());
    expect(tags.filter((t) => t === "≠ engine").length).toBe(3);
    // The action strip is full-width beneath the arms.
    expect(card.find(".hc-actionrow .hc-action-word").exists()).toBe(true);
    expect(
      card.find(".hc-summary + .hc-summary .hc-prose").text()
    ).toContain("First read for this holding");
  });

  test("a model outlook matching the stand-in on every horizon drops its tag", () => {
    const aligned = twoArmGraded();
    aligned.horizon_outlook = { ...aligned.engine_view!.outlook };
    const wrapper = mountView({
      run: {
        ...run,
        verdicts: [verdict("AAPL", { status: "priced", ...aligned })],
      },
    });
    // Conviction and action still diverge in the fixture — the outlook row's
    // tag is the one that disappears when every horizon matches.
    const tags = wrapper.findAll(".ana-tag").map((t) => t.text());
    expect(tags.filter((t) => t === "≠ engine").length).toBe(2);
  });


  test("an inverted model band renders as authored with the annotation tag", () => {
    const inverted = twoArmGraded();
    inverted.model_view!.price_targets.twelve_month = {
      base: 250,
      bear: 300,
      bull: 200,
    };
    const wrapper = mountView({
      run: {
        ...run,
        verdicts: [verdict("AAPL", { status: "priced", ...inverted })],
      },
    });
    const tags = wrapper.findAll(".ana-tag").map((t) => t.text());
    expect(tags).toContain("band inverted as authored");
    // The authored numbers are not reordered.
    expect(wrapper.text()).toContain("($300.00–$200.00)");
  });

  test("the roll-up renders the model-vs-engine scoreboard", () => {
    const wrapper = mountView({
      run: {
        ...run,
        roll_up: {
          ...run.roll_up,
        },
        outcome: {
          matured: [
            {
              symbol: "AAPL",
              episode_id: "ep-1",
              window_months: 1,
              outcome: "scored",
              total_return: 0.042,
              price_return: 0.04,
            },
          ],
          reads: {
            target_calibration: [
              {
                window_months: 12,
                parameter_version: "targets-v3",
                scored: 2,
                coverage_rate: 1,
                nominal_coverage: 0.8,
                mean_interval_score: 0.41,
                mean_base_signed_error: 0.02,
              },
            ],
            model_target_calibration: [
              {
                window_months: 12,
                parameter_version: "targets-v3",
                scored: 2,
                coverage_rate: 1,
                nominal_coverage: 0.8,
                mean_interval_score: 0.35,
                mean_base_signed_error: -0.01,
              },
            ],
            head_to_head: [
              {
                window_months: 12,
                scored: 2,
                engine_mean_interval_score: 0.41,
                model_mean_interval_score: 0.35,
                engine_coverage_rate: 1,
                model_coverage_rate: 1,
              },
            ],
            outlook_direction: [
              { arm: "engine", window_months: 12, scored: 2, hits: 0, neutral: 0 },
              { arm: "model", window_months: 12, scored: 2, hits: 2, neutral: 0 },
            ],
          },
        },
      },
    });
    const scoreboard = wrapper.find(".rollup-scoreboard");
    expect(scoreboard.exists()).toBe(true);
    expect(scoreboard.text()).toContain(
      "12-mo interval score (paired, 2): model 0.350 vs engine 0.410"
    );
    expect(scoreboard.text()).toContain("12-mo direction: model 2/2 vs engine 0/2");
    // The card foot carries the symbol's matured scored line.
    expect(wrapper.find(".hc-scoreboard-line").text()).toContain(
      "1-mo window scored (total return 4.2%)"
    );
  });
});

// The 2026-08-16 badge ruling: a selective run analyzes strictly the selection.
// A held position with no verdict renders a not-analyzed placeholder; a carried
// verdict whose side reversed carries a "Side reversed" badge.
describe("PortfolioView badge ruling", () => {
  test("a held position with no verdict renders a not-analyzed placeholder card", () => {
    const runWithNew: PortfolioRun = {
      ...run,
      holdings: {
        ...run.holdings,
        positions: [
          ...positions,
          position("NVDA", { cost_basis: 6_000, market_value: 9_000 }),
        ],
      },
    };
    const wrapper = mountView({ run: runWithNew });
    expect(stackTickers(wrapper)).toContain("NVDA");
    expect(wrapper.text()).toContain("Not analyzed in this run");
  });

  test("the not-analyzed placeholder is selectable for a selective re-run", () => {
    const runWithNew: PortfolioRun = {
      ...run,
      holdings: {
        ...run.holdings,
        positions: [
          ...positions,
          position("NVDA", { cost_basis: 6_000, market_value: 9_000 }),
        ],
      },
    };
    const wrapper = mountView({ run: runWithNew });
    const nvdaCard = wrapper
      .findAll(".card-stack .holding-card")
      .find((c) => c.find(".ana-ticker").text() === "NVDA")!;
    expect(nvdaCard.find(".hc-select-input").exists()).toBe(true);
  });

  test("the sort bar orders placeholders into the stack on the same position keys", async () => {
    // Audit L2 (2026-08-18 ruling): the placeholder is a holding card too —
    // it sorts on its own position, never appended after the verdicts.
    const runWithNew: PortfolioRun = {
      ...run,
      holdings: {
        ...run.holdings,
        positions: [
          ...positions,
          // 9k value sits between AAPL (19.5k) and XYZ (5k); +50% gain sits
          // between XYZ (+150%) and AAPL (+39.3%).
          position("NVDA", { cost_basis: 6_000, market_value: 9_000 }),
        ],
      },
    };
    const wrapper = mountView({ run: runWithNew });
    // Default value-desc interleaves the placeholder by market value.
    expect(stackTickers(wrapper)).toEqual(["MSFT", "AAPL", "NVDA", "XYZ", "OPT"]);
    const nvdaCard = wrapper
      .findAll(".card-stack .holding-card")
      .find((c) => c.find(".ana-ticker").text() === "NVDA")!;
    expect(nvdaCard.text()).toContain("Not analyzed in this run");
    // A different key reorders it with the rest.
    const pctButton = wrapper
      .findAll(".ana-sortbar button")
      .find((b) => b.text().includes("% gain"))!;
    await pctButton.trigger("click");
    expect(stackTickers(wrapper)).toEqual(["XYZ", "NVDA", "AAPL", "MSFT", "OPT"]);
    // The placeholder keeps its selection box after the reorder.
    const moved = wrapper
      .findAll(".card-stack .holding-card")
      .find((c) => c.find(".ana-ticker").text() === "NVDA")!;
    expect(moved.find(".hc-select-input").exists()).toBe(true);
  });

  test("the sort bar shows once the stack has two cards, placeholders counted", () => {
    // One verdict alone: nothing to reorder, no bar.
    const single: PortfolioRun = {
      ...run,
      holdings: { ...run.holdings, positions: [positions[0]] },
      verdicts: [run.verdicts[0]],
    };
    expect(mountView({ run: single }).find(".ana-sortbar").exists()).toBe(false);
    // One verdict plus one not-analyzed placeholder: two cards, the bar shows.
    const withPlaceholder: PortfolioRun = {
      ...single,
      holdings: {
        ...single.holdings,
        positions: [
          positions[0],
          position("NVDA", { cost_basis: 6_000, market_value: 9_000 }),
        ],
      },
    };
    const wrapper = mountView({ run: withPlaceholder });
    expect(wrapper.find(".ana-sortbar").exists()).toBe(true);
    expect(stackTickers(wrapper)).toEqual(["MSFT", "NVDA"]);
  });

  test("a side-reversed carried verdict renders a Side reversed badge", () => {
    const runReversed: PortfolioRun = {
      ...run,
      verdicts: run.verdicts.map((v) =>
        v.symbol === "MSFT" ? { ...v, side_reversed: true } : v
      ),
    };
    const wrapper = mountView({ run: runReversed });
    expect(wrapper.text()).toContain("Side reversed");
  });
});
