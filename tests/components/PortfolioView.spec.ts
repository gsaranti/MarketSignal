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
  HoldingsPull,
  HoldingVerdict,
  PortfolioRun,
  Position,
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
    action_sizing: {
      target_weight_low: 0.1,
      target_weight_high: 0.2,
      est_share_delta: null,
      est_dollar_delta: null,
    },
    conviction: "medium",
    horizon_outlook: { short: "neutral", mid: "bullish", long: "bullish" },
    price_targets: {
      end_of_month: null,
      end_of_year: {
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
    financial_summary: "Solid margins.",
    what_changed: "First analyzed run.",
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
    verdict("MSFT", { status: "graded", ...graded({ grade: "A", action: "trim" }) }),
    verdict("AAPL", { status: "graded", ...graded() }, { position_change: "increased" }),
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
    const reveal = wrapper.findAll(".hc-reveal")[0];
    expect(reveal.attributes("aria-expanded")).toBe("false");
    expect(wrapper.text()).not.toContain("drift off revenue growth");
    await reveal.trigger("click");
    expect(reveal.attributes("aria-expanded")).toBe("true");
    expect(wrapper.text()).toContain("drift off revenue growth");
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
    const [pull, runBtn] = wrapper.findAll(".toolbar-actions button");
    expect(pull.attributes("disabled")).toBeDefined();
    expect(pull.attributes("title")).toContain("Schwab account not connected");
    expect(runBtn.attributes("disabled")).toBeDefined();
    expect(runBtn.attributes("title")).toContain("daemon endpoint");
  });

  test("the pull trigger works while only the run is model-blocked", async () => {
    const wrapper = mountView({
      runBlocked: true,
      runBlockedReason: "Not configured: reasoner model.",
    });
    const [pull, runBtn] = wrapper.findAll(".toolbar-actions button");
    expect(runBtn.attributes("disabled")).toBeDefined();
    expect(pull.attributes("disabled")).toBeUndefined();
    await pull.trigger("click");
    expect(wrapper.emitted("pull")).toHaveLength(1);
    expect(wrapper.emitted("run")).toBeUndefined();
  });

  test("a busy run slot disables both triggers", () => {
    const wrapper = mountView({ busy: true });
    for (const b of wrapper.findAll(".toolbar-actions button")) {
      expect(b.attributes("disabled")).toBeDefined();
    }
  });
});
