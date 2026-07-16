<script setup lang="ts">
import { computed, ref } from "vue";
import { localDateTime } from "../format";
import type {
  HoldingsPull,
  HoldingVerdict,
  PortfolioConviction,
  PortfolioRun,
  Position,
} from "../types";

// The Portfolio page (docs/portfolio-analysis.md §Storage and display,
// docs/interface.md). Analytical register throughout — mono/tabular numerics,
// tracked-caps heads, hairlines, the desaturated directional + grade palette —
// per the design package's Portfolio.jsx fidelity reference. Presentational:
// props in, events out; App.vue owns every invoke.
const props = defineProps<{
  // The latest persisted analysis run (null before the first run).
  run: PortfolioRun | null;
  // The latest standalone Pull-holdings snapshot (null before the first pull).
  // View-only — never merged into the run-anchored verdict cards.
  pull: HoldingsPull | null;
  // Reading the persisted state (initial fetch / on-entry refresh).
  loading: boolean;
  loadError: string | null;
  // An inline run-gate block or run/pull failure — ephemeral, never a
  // persistent warning (docs/interface.md §Connection status).
  runError: string | null;
  // Presence-gate locks (docs/interface.md §Persistent Warning Area): Run
  // needs the whole local gate; the view-only Pull needs only Schwab.
  runBlocked: boolean;
  runBlockedReason: string | null;
  pullBlocked: boolean;
  pullBlockedReason: string | null;
  // A workflow holds the single global run slot (report / portfolio / connect /
  // pull) — both triggers disable while anything runs.
  busy: boolean;
  running: boolean;
  pulling: boolean;
}>();

const emit = defineEmits<{ (e: "run"): void; (e: "pull"): void }>();

// ---- Triggers ---------------------------------------------------------------

const runDisabled = computed(() => props.runBlocked || props.busy);
const pullDisabled = computed(() => props.pullBlocked || props.busy);

// The disabled reason, surfaced as the button title so the lock is explicable
// in place (the warning band carries the full items).
const runTitle = computed(() => {
  if (props.runBlocked)
    return props.runBlockedReason ?? "Local-suite configuration is incomplete";
  if (props.busy) return "Another job is running";
  return "Pull fresh holdings and run the analysis";
});
const pullTitle = computed(() => {
  if (props.pullBlocked)
    return props.pullBlockedReason ?? "Schwab account not connected";
  if (props.busy) return "Another job is running";
  return "Fetch current positions without running the analysis";
});

// ---- Formatting ---------------------------------------------------------------

const money = new Intl.NumberFormat(undefined, {
  style: "currency",
  currency: "USD",
  maximumFractionDigits: 0,
});
const moneyExact = new Intl.NumberFormat(undefined, {
  style: "currency",
  currency: "USD",
  maximumFractionDigits: 2,
});
const qtyFmt = new Intl.NumberFormat(undefined, { maximumFractionDigits: 4 });

function fmtMoney(v: number): string {
  return Math.abs(v) >= 1000 ? money.format(v) : moneyExact.format(v);
}
function fmtPct(fraction: number, digits = 1): string {
  return `${(fraction * 100).toFixed(digits)}%`;
}
function fmtSigned(v: number): string {
  return `${v > 0 ? "+" : ""}${fmtMoney(v)}`;
}
function fmtSignedPct(fraction: number): string {
  return `${fraction > 0 ? "+" : ""}${fmtPct(fraction)}`;
}
function fmtStamp(iso: string): string {
  return localDateTime(iso);
}

// ---- Position lookups ---------------------------------------------------------

const runPositions = computed(() => {
  const map = new Map<string, Position>();
  for (const p of props.run?.holdings.positions ?? []) map.set(p.symbol, p);
  return map;
});

function positionFor(symbol: string): Position | null {
  return runPositions.value.get(symbol) ?? null;
}

function weightOf(pos: Position | null): number | null {
  const total = props.run?.holdings.account_total ?? 0;
  if (!pos || total <= 0) return null;
  return pos.market_value / total;
}

// Unrealized P/L from the two Schwab-reported totals. A position with no
// reported cost basis (cash, typically) has an undefined gain.
function gainOf(pos: Position | null): number | null {
  if (!pos || pos.cost_basis <= 0) return null;
  return pos.market_value - pos.cost_basis;
}
function gainPctOf(pos: Position | null): number | null {
  if (!pos || pos.cost_basis <= 0) return null;
  return (pos.market_value - pos.cost_basis) / pos.cost_basis;
}
function dirOf(v: number | null): "up" | "down" | "flat" {
  if (v === null || v === 0) return "flat";
  return v > 0 ? "up" : "down";
}

// ---- Fresher-pull comparison (presence-only churn tags) -----------------------
// Display-time, symbol-presence only: the quantity-move classification stays the
// run-time engine diff's job (docs/portfolio-analysis.md §Storage and display).

const pullIsFresher = computed(() => {
  if (!props.pull) return false;
  if (!props.run) return true;
  return Date.parse(props.pull.pulled_at) > Date.parse(props.run.created_at);
});

// The current-holdings section shows only when the pull is the fresher vintage
// (before any run it IS the page body).
const showCurrentHoldings = computed(() => props.pull !== null && pullIsFresher.value);

const pullSymbols = computed(() => {
  const s = new Set<string>();
  for (const p of props.pull?.holdings.positions ?? []) s.add(p.symbol);
  return s;
});

function newSinceAnalysis(symbol: string): boolean {
  return props.run !== null && !runPositions.value.has(symbol);
}
function noLongerHeld(symbol: string): boolean {
  return (
    props.run !== null && pullIsFresher.value && !pullSymbols.value.has(symbol)
  );
}

// ---- Holdings sort bar ---------------------------------------------------------
// Four deterministic, position-level keys off the Schwab-reported market value /
// cost basis (docs/portfolio-analysis.md §Storage and display). Display-only:
// reorders the already-computed cards, touches nothing else. The last-used key
// persists in localStorage (pure presentation, like the appearance preference).

type SortKey = "value" | "gain" | "gain-pct" | "cost";
interface SortState {
  key: SortKey;
  dir: "asc" | "desc";
}

const SORT_KEYS: { key: SortKey; label: string }[] = [
  { key: "value", label: "Value" },
  { key: "gain", label: "$ gain" },
  { key: "gain-pct", label: "% gain" },
  { key: "cost", label: "Cash invested" },
];
const SORT_STORAGE_KEY = "market-signal.portfolio-sort";
const DEFAULT_SORT: SortState = { key: "value", dir: "desc" };

function readStoredSort(): SortState {
  try {
    const raw = localStorage.getItem(SORT_STORAGE_KEY);
    if (!raw) return DEFAULT_SORT;
    const parsed = JSON.parse(raw) as Partial<SortState>;
    if (
      SORT_KEYS.some((k) => k.key === parsed.key) &&
      (parsed.dir === "asc" || parsed.dir === "desc")
    ) {
      return { key: parsed.key as SortKey, dir: parsed.dir };
    }
  } catch {
    // Unreadable storage falls back to the default — never an error surface.
  }
  return DEFAULT_SORT;
}

const sort = ref<SortState>(readStoredSort());

function pickSort(key: SortKey) {
  const next: SortState =
    sort.value.key === key
      ? { key, dir: sort.value.dir === "desc" ? "asc" : "desc" }
      : { key, dir: "desc" };
  sort.value = next;
  try {
    localStorage.setItem(SORT_STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Storage full/unavailable only costs persistence, never the reorder.
  }
}

// The sort metric for one card, or null when undefined for the key (no reported
// cost basis) — nulls sort last under every direction, per the docs.
function sortMetric(symbol: string, key: SortKey): number | null {
  const pos = positionFor(symbol);
  if (!pos) return null;
  switch (key) {
    case "value":
      return pos.market_value;
    case "gain":
      return gainOf(pos);
    case "gain-pct":
      return gainPctOf(pos);
    case "cost":
      return pos.cost_basis > 0 ? pos.cost_basis : null;
  }
}

// The card stack: every verdict (graded, not-rated, insufficient), reordered in
// place; exited positions live only in the roll-up. Stable sort with an
// alphabetical ticker tie-break.
const sortedVerdicts = computed<HoldingVerdict[]>(() => {
  const verdicts = [...(props.run?.verdicts ?? [])];
  const { key, dir } = sort.value;
  const sign = dir === "desc" ? -1 : 1;
  return verdicts.sort((a, b) => {
    const ma = sortMetric(a.symbol, key);
    const mb = sortMetric(b.symbol, key);
    if (ma === null && mb === null) return a.symbol.localeCompare(b.symbol);
    if (ma === null) return 1;
    if (mb === null) return -1;
    if (ma !== mb) return sign * (ma - mb);
    return a.symbol.localeCompare(b.symbol);
  });
});

function sortButtonName(key: SortKey, label: string): string {
  if (sort.value.key !== key) return `Sort by ${label}`;
  return `Sort by ${label}, ${sort.value.dir === "asc" ? "ascending" : "descending"}`;
}

// ---- Current-holdings table sorting ----------------------------------------------
// Column sorting for the pull table, through its grid heads proper (aria-sort —
// the pattern the card sort bar deliberately reserves for tables). Display-only,
// like the card sort: reorders the pulled rows in place, touches nothing else.
// Default is the account's as-pulled order; a position missing a key's value
// (no reported cost basis or price — rendered "—") sorts last under that key
// in either direction.

type PullSortKey = "symbol" | "qty" | "price" | "value" | "cost" | "gain-pct";
interface PullSortState {
  key: PullSortKey;
  dir: "asc" | "desc";
}

// Text opens ascending (alphabetical); the size/money columns open descending.
const PULL_SORT_OPEN_DIR: Record<PullSortKey, "asc" | "desc"> = {
  symbol: "asc",
  qty: "desc",
  price: "desc",
  value: "desc",
  cost: "desc",
  "gain-pct": "desc",
};
const PULL_SORT_STORAGE_KEY = "market-signal.portfolio-pull-sort";

function readStoredPullSort(): PullSortState | null {
  try {
    const raw = localStorage.getItem(PULL_SORT_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<PullSortState>;
    if (
      parsed.key !== undefined &&
      parsed.key in PULL_SORT_OPEN_DIR &&
      (parsed.dir === "asc" || parsed.dir === "desc")
    ) {
      return { key: parsed.key, dir: parsed.dir };
    }
  } catch {
    // Unreadable storage falls back to the as-pulled order — never an error surface.
  }
  return null;
}

const pullSort = ref<PullSortState | null>(readStoredPullSort());

function pickPullSort(key: PullSortKey) {
  const cur = pullSort.value;
  const next: PullSortState =
    cur?.key === key
      ? { key, dir: cur.dir === "desc" ? "asc" : "desc" }
      : { key, dir: PULL_SORT_OPEN_DIR[key] };
  pullSort.value = next;
  try {
    localStorage.setItem(PULL_SORT_STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Storage full/unavailable only costs persistence, never the reorder.
  }
}

function pullSortMetric(
  p: Position,
  key: Exclude<PullSortKey, "symbol">
): number | null {
  switch (key) {
    case "qty":
      return p.quantity;
    case "price":
      return p.current_price;
    case "value":
      return p.market_value;
    case "cost":
      return p.cost_basis > 0 ? p.cost_basis : null;
    case "gain-pct":
      return gainPctOf(p);
  }
}

const sortedPullPositions = computed<Position[]>(() => {
  const positions = props.pull?.holdings.positions ?? [];
  const s = pullSort.value;
  if (!s) return positions;
  const sign = s.dir === "desc" ? -1 : 1;
  return [...positions].sort((a, b) => {
    if (s.key === "symbol") return sign * a.symbol.localeCompare(b.symbol);
    const ma = pullSortMetric(a, s.key);
    const mb = pullSortMetric(b, s.key);
    if (ma === null && mb === null) return a.symbol.localeCompare(b.symbol);
    if (ma === null) return 1;
    if (mb === null) return -1;
    if (ma !== mb) return sign * (ma - mb);
    return a.symbol.localeCompare(b.symbol);
  });
});

function pullSortClasses(key: PullSortKey): Record<string, boolean> {
  const active = pullSort.value?.key === key;
  return {
    sortable: true,
    "sorted-asc": active && pullSort.value?.dir === "asc",
    "sorted-desc": active && pullSort.value?.dir === "desc",
  };
}

function pullAriaSort(key: PullSortKey): "ascending" | "descending" | undefined {
  if (pullSort.value?.key !== key) return undefined;
  return pullSort.value.dir === "asc" ? "ascending" : "descending";
}

function pullSortName(key: PullSortKey, label: string): string {
  if (pullSort.value?.key !== key) return `Sort by ${label}`;
  return `Sort by ${label}, ${pullSort.value.dir === "asc" ? "ascending" : "descending"}`;
}

// ---- Verdict rendering helpers --------------------------------------------------

const CLASS_LABELS: Record<string, string> = {
  stock: "Stock",
  etf: "ETF / fund",
  "mutual-fund": "Mutual fund",
  "option-contract": "Options",
  "fixed-income": "Fixed income",
  cash: "Cash",
  other: "Unsupported",
};
function classLabel(v: HoldingVerdict): string {
  const base = CLASS_LABELS[v.asset_class] ?? v.asset_class;
  if (v.disposition.status === "priced") {
    // A priced fund shows its deterministic strategy classification (e.g. "US
    // equity fund" — docs/portfolio-analysis.md §Asset eligibility); null for a
    // stock and on runs persisted before the field.
    if (v.disposition.fund_class_label)
      return `${v.disposition.fund_class_label} · reduced verdict`;
    return `${base} · ${v.asset_class === "stock" ? "full verdict" : "reduced verdict"}`;
  }
  if (v.disposition.status === "role-risk-only")
    return `${v.disposition.class_label} · role / risk read`;
  if (v.disposition.status === "not-rated") return `${base} · not rated`;
  return `${base} · insufficient evidence`;
}

const ACTION_LABELS: Record<string, string> = {
  "sell-all": "Sell all",
  trim: "Trim",
  hold: "Hold",
  add: "Add",
  "add-aggressively": "Add aggressively",
};

const CONVICTION_LEVEL: Record<PortfolioConviction, number> = {
  low: 1,
  medium: 2,
  high: 3,
};

const HORIZON_DIR: Record<string, "up" | "down" | "flat"> = {
  bullish: "up",
  bearish: "down",
  neutral: "flat",
};

const CHANGE_LABELS: Record<string, string> = {
  new: "New",
  increased: "Increased",
  decreased: "Decreased",
  unchanged: "Unchanged",
};

function gradeClass(grade: string): string {
  return grade.toLowerCase();
}

// The target-weight band as a compact percent range.
function weightBand(low: number, high: number): string {
  return `${(low * 100).toFixed(0)}–${(high * 100).toFixed(0)}%`;
}

// Whether a graded verdict's options signal carries anything to show.
function hasOptionsSignal(v: {
  put_call_volume: number | null;
  put_call_open_interest: number | null;
  implied_volatility: number | null;
  iv_skew: number | null;
}): boolean {
  return (
    v.put_call_volume !== null ||
    v.put_call_open_interest !== null ||
    v.implied_volatility !== null ||
    v.iv_skew !== null
  );
}

// Per-card methodology disclosure (the kit's Reveal, inline rather than a
// popover so it never overlaps neighboring cards). Keyed per symbol.
const openMethodology = ref<Set<string>>(new Set());
function toggleMethodology(symbol: string) {
  const next = new Set(openMethodology.value);
  if (next.has(symbol)) next.delete(symbol);
  else next.add(symbol);
  openMethodology.value = next;
}

// ---- Key-figure strip ------------------------------------------------------------

const keyFigures = computed(() => {
  const run = props.run;
  if (!run) return [];
  const items: { label: string; value: string }[] = [
    { label: "Account value", value: fmtMoney(run.holdings.account_total) },
    { label: "Positions", value: String(run.holdings.positions.length) },
    { label: "Graded", value: String(run.roll_up.graded_count) },
    { label: "Not rated", value: String(run.roll_up.not_rated_count) },
  ];
  if (run.roll_up.role_risk_only_count > 0) {
    items.push({
      label: "Role/risk",
      value: String(run.roll_up.role_risk_only_count),
    });
  }
  if (run.roll_up.insufficient_evidence_count > 0) {
    items.push({
      label: "Insufficient",
      value: String(run.roll_up.insufficient_evidence_count),
    });
  }
  items.push(
    { label: "Cash", value: fmtPct(run.roll_up.cash_weight) },
    { label: "Top position", value: fmtPct(run.roll_up.top_position_weight) }
  );
  return items;
});
</script>

<template>
  <section class="portfolio-pane" aria-label="Portfolio Analysis">
    <!-- Toolbar: the surface eyebrow + the two independent triggers
         (docs/portfolio-analysis.md §Triggering — never sequenced). -->
    <div class="toolbar">
      <span class="toolbar-label">Portfolio</span>
      <div class="toolbar-actions">
        <button
          type="button"
          class="btn btn-secondary"
          :disabled="pullDisabled"
          :title="pullTitle"
          @click="emit('pull')"
        >
          {{ pulling ? "Pulling holdings…" : "Pull holdings" }}
        </button>
        <button
          type="button"
          class="btn btn-primary"
          :disabled="runDisabled"
          :title="runTitle"
          @click="emit('run')"
        >
          {{ running ? "Running analysis…" : "Run analysis" }}
        </button>
      </div>
    </div>

    <!-- Inline run-gate block / failure — ephemeral, never a persistent warning. -->
    <div v-if="runError" class="pane-error" role="alert">
      <span class="pane-error-label">Couldn't run</span>
      <span class="pane-error-detail">{{ runError }}</span>
    </div>

    <div class="pane-scroll">
      <!-- Initial load -->
      <p v-if="loading && !run && !pull" class="pane-quiet" aria-live="polite">
        Loading portfolio…
      </p>

      <!-- Persisted-state read failure (with nothing cached to show) -->
      <div v-else-if="loadError && !run && !pull" class="pane-quiet" role="alert">
        <span class="pane-error-label">Couldn't load the portfolio</span>
        <span class="pane-error-detail">{{ loadError }}</span>
      </div>

      <!-- Empty: no pull, no run -->
      <div v-else-if="!run && !pull" class="empty-state">
        <h2 class="empty-title">No holdings yet.</h2>
        <p class="empty-body">
          Holdings are fetched only on explicit action — never auto-synced.
          <strong>Run analysis</strong> pulls fresh holdings from your connected
          Schwab account and grades them; <strong>Pull holdings</strong> just
          fetches and shows your positions, without running the analysis.
        </p>
      </div>

      <template v-else>
        <!-- Pulled, not yet analyzed: the compact current-holdings view IS the
             page body (docs/portfolio-analysis.md §Storage and display). -->
        <div v-if="!run && pull" class="pulled-only">
          <h2 class="empty-title">
            {{ pull.holdings.positions.length }}
            {{ pull.holdings.positions.length === 1 ? "holding" : "holdings" }}
            pulled. Not yet analyzed.
          </h2>
          <p class="empty-body">
            Pulled {{ fmtStamp(pull.pulled_at) }} from your connected Schwab
            account. Nothing is graded until you run the analysis.
          </p>
        </div>

        <!-- Analyzed: strip → (fresher pull) → sort bar + cards → roll-up. -->
        <div v-if="run" class="keyfig strip" role="list">
          <div v-for="f in keyFigures" :key="f.label" class="kf" role="listitem">
            <div class="kf-label">{{ f.label }}</div>
            <div class="kf-value">{{ f.value }}</div>
          </div>
        </div>

        <!-- Current holdings (the standalone pull), shown when it is the fresher
             vintage — a stamped section ABOVE the run-anchored cards, never
             merged into them. Presence-only churn tags. -->
        <section
          v-if="showCurrentHoldings && pull"
          class="ana-card current-holdings"
          aria-label="Current holdings"
        >
          <header class="ch-head">
            <span class="ana-head">Current holdings</span>
            <span class="ch-stamp"
              >Pulled {{ fmtStamp(pull.pulled_at) }}<template v-if="run">
                · analysis from {{ fmtStamp(run.created_at) }}</template
              ></span
            >
          </header>
          <div class="ch-scroll" tabindex="0">
            <table class="ana-grid">
              <thead>
                <tr>
                  <th
                    scope="col"
                    :class="pullSortClasses('symbol')"
                    :aria-sort="pullAriaSort('symbol')"
                  >
                    <button
                      type="button"
                      :aria-label="pullSortName('symbol', 'Symbol')"
                      @click="pickPullSort('symbol')"
                    >
                      Symbol
                    </button>
                  </th>
                  <th
                    scope="col"
                    class="num"
                    :class="pullSortClasses('qty')"
                    :aria-sort="pullAriaSort('qty')"
                  >
                    <button
                      type="button"
                      :aria-label="pullSortName('qty', 'Quantity')"
                      @click="pickPullSort('qty')"
                    >
                      Qty
                    </button>
                  </th>
                  <th
                    scope="col"
                    class="num"
                    :class="pullSortClasses('price')"
                    :aria-sort="pullAriaSort('price')"
                  >
                    <button
                      type="button"
                      :aria-label="pullSortName('price', 'Price')"
                      @click="pickPullSort('price')"
                    >
                      Price
                    </button>
                  </th>
                  <th
                    scope="col"
                    class="num"
                    :class="pullSortClasses('value')"
                    :aria-sort="pullAriaSort('value')"
                  >
                    <button
                      type="button"
                      :aria-label="pullSortName('value', 'Market value')"
                      @click="pickPullSort('value')"
                    >
                      Market value
                    </button>
                  </th>
                  <th
                    scope="col"
                    class="num"
                    :class="pullSortClasses('cost')"
                    :aria-sort="pullAriaSort('cost')"
                  >
                    <button
                      type="button"
                      :aria-label="pullSortName('cost', 'Cost basis')"
                      @click="pickPullSort('cost')"
                    >
                      Cost basis
                    </button>
                  </th>
                  <th
                    scope="col"
                    class="num"
                    :class="pullSortClasses('gain-pct')"
                    :aria-sort="pullAriaSort('gain-pct')"
                  >
                    <button
                      type="button"
                      :aria-label="pullSortName('gain-pct', '% gain')"
                      @click="pickPullSort('gain-pct')"
                    >
                      % gain
                    </button>
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="p in sortedPullPositions" :key="p.symbol">
                  <td>
                    <span class="ana-ticker">{{ p.symbol }}</span>
                    <span v-if="newSinceAnalysis(p.symbol)" class="ana-tag ch-tag"
                      >New · not in last analysis</span
                    >
                  </td>
                  <td class="num">{{ qtyFmt.format(p.quantity) }}</td>
                  <td class="num">
                    {{ p.current_price !== null ? fmtMoney(p.current_price) : "—" }}
                  </td>
                  <td class="num">{{ fmtMoney(p.market_value) }}</td>
                  <td class="num">
                    {{ p.cost_basis > 0 ? fmtMoney(p.cost_basis) : "—" }}
                  </td>
                  <td class="num">
                    <span
                      v-if="gainPctOf(p) !== null"
                      class="dir"
                      :class="dirOf(gainPctOf(p))"
                      >{{ fmtSignedPct(gainPctOf(p)!) }}</span
                    >
                    <template v-else>—</template>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
          <footer class="ch-foot">
            <span
              >Cash
              <span class="ana-num">{{ fmtMoney(pull.holdings.cash) }}</span></span
            >
            <span
              >Account total
              <span class="ana-num">{{
                fmtMoney(pull.holdings.account_total)
              }}</span></span
            >
          </footer>
        </section>

        <template v-if="run">
          <!-- Sort bar: display-only card-stack reorder; aria-pressed toggles,
               never aria-sort (reserved for the grid heads). -->
          <div
            v-if="run.verdicts.length > 1"
            class="ana-sortbar"
            role="group"
            aria-label="Sort holdings"
          >
            <span class="ana-sortbar-label" aria-hidden="true">Sort</span>
            <button
              v-for="k in SORT_KEYS"
              :key="k.key"
              type="button"
              :aria-pressed="sort.key === k.key"
              :data-dir="sort.key === k.key ? sort.dir : undefined"
              :aria-label="sortButtonName(k.key, k.label)"
              @click="pickSort(k.key)"
            >
              {{ k.label }}
            </button>
          </div>

          <!-- The holding-card stack -->
          <div class="card-stack">
            <article
              v-for="v in sortedVerdicts"
              :key="v.symbol"
              class="ana-card holding-card"
            >
              <!-- Not-rated / insufficient-evidence: a legitimately reduced card. -->
              <div
                v-if="
                  v.disposition.status === 'not-rated' ||
                  v.disposition.status === 'insufficient-evidence'
                "
                class="hc-reduced"
              >
                <div class="hc-reduced-main">
                  <div class="hc-idline">
                    <span class="ana-ticker">{{ v.symbol }}</span>
                    <span class="hc-class">{{ classLabel(v) }}</span>
                    <span v-if="noLongerHeld(v.symbol)" class="ana-tag"
                      >No longer held</span
                    >
                  </div>
                  <p class="hc-reason">{{ v.disposition.reason }}</p>
                </div>
                <div class="hc-reduced-side">
                  <span class="hc-kicker">Weight</span>
                  <span class="ana-num hc-weight">{{
                    weightOf(positionFor(v.symbol)) !== null
                      ? fmtPct(weightOf(positionFor(v.symbol))!)
                      : "—"
                  }}</span>
                </div>
              </div>

              <!-- Role/risk-only verdict: the union's other branch — an explicit
                   designed card (role, exposure, risk, expense, gaps beside the
                   action), never empty priced placeholders
                   (docs/portfolio-analysis.md §Storage and display). -->
              <template v-else-if="v.disposition.status === 'role-risk-only'">
                <header class="hc-head">
                  <div class="hc-id">
                    <div class="hc-id-text">
                      <div class="hc-idline">
                        <span class="ana-ticker">{{ v.symbol }}</span>
                        <span class="hc-class">{{ classLabel(v) }}</span>
                        <span v-if="v.disposition.structural_flag" class="ana-tag"
                          >Structurally path-dependent</span
                        >
                        <span v-if="noLongerHeld(v.symbol)" class="ana-tag"
                          >No longer held</span
                        >
                      </div>
                      <div class="hc-name">
                        {{ positionFor(v.symbol)?.description ?? "" }}
                      </div>
                    </div>
                  </div>
                  <div class="hc-unrealized">
                    <span class="hc-kicker">Unrealized</span>
                    <span
                      v-if="gainOf(positionFor(v.symbol)) !== null"
                      class="dir hc-gain"
                      :class="dirOf(gainOf(positionFor(v.symbol)))"
                    >
                      {{ fmtSigned(gainOf(positionFor(v.symbol))!) }}
                      ({{ fmtPct(gainPctOf(positionFor(v.symbol))!) }})
                    </span>
                    <span v-else class="ana-num hc-gain-none">—</span>
                  </div>
                </header>

                <div class="hc-body">
                  <div class="hc-col hc-col-intrinsic">
                    <span class="hc-kicker">Role &amp; risk</span>
                    <p class="hc-prose">{{ v.disposition.role_summary }}</p>
                    <dl class="hc-kv">
                      <template v-if="v.disposition.exposure_tilt.length > 0">
                        <dt>Exposure</dt>
                        <dd>
                          <span
                            v-for="tilt in v.disposition.exposure_tilt.slice(0, 3)"
                            :key="tilt.label"
                            class="hc-horizon"
                          >
                            <span class="hc-horizon-label">{{ tilt.label }}</span>
                            <span class="ana-num">{{ fmtPct(tilt.weight) }}</span>
                          </span>
                        </dd>
                      </template>
                      <template v-if="v.disposition.expense_drag !== null">
                        <dt>Expense drag</dt>
                        <dd>
                          <span class="ana-num">{{
                            (v.disposition.expense_drag * 100).toFixed(2) + "%"
                          }}</span>
                        </dd>
                      </template>
                      <template v-if="v.disposition.observable_risk !== null">
                        <dt>Realized vol</dt>
                        <dd>
                          <span class="ana-num">{{
                            fmtPct(v.disposition.observable_risk)
                          }}</span>
                        </dd>
                      </template>
                    </dl>
                    <p
                      v-if="v.disposition.evidence_gaps.length > 0"
                      class="hc-reason"
                    >
                      {{ v.disposition.evidence_gaps.join("; ") }}
                    </p>
                  </div>

                  <div class="hc-col">
                    <span class="hc-kicker">Portfolio action</span>
                    <div class="hc-action">
                      <span class="hc-action-word">{{
                        ACTION_LABELS[v.disposition.action]
                      }}</span>
                      <span class="hc-action-band"
                        >to
                        {{
                          weightBand(
                            v.disposition.action_sizing.target_weight_low,
                            v.disposition.action_sizing.target_weight_high
                          )
                        }}</span
                      >
                    </div>
                    <dl class="hc-kv">
                      <dt>Weight</dt>
                      <dd>
                        <span class="ana-num">{{
                          weightOf(positionFor(v.symbol)) !== null
                            ? fmtPct(weightOf(positionFor(v.symbol))!)
                            : "—"
                        }}</span>
                      </dd>
                    </dl>
                  </div>
                </div>

                <footer class="hc-foot">
                  <div class="hc-foot-main">
                    <span class="hc-kicker">What changed · since last run</span>
                    <p class="hc-changed">{{ v.disposition.what_changed }}</p>
                  </div>
                  <span class="ana-tag" :title="'Position vs. prior run'"
                    >Position: {{ CHANGE_LABELS[v.position_change] }}</span
                  >
                </footer>
              </template>

              <!-- Priced verdict -->
              <template v-else>
                <header class="hc-head">
                  <div class="hc-id">
                    <span
                      class="grade hc-grade"
                      :class="gradeClass(v.disposition.grade)"
                      >{{ v.disposition.grade }}</span
                    >
                    <div class="hc-id-text">
                      <div class="hc-idline">
                        <span class="ana-ticker">{{ v.symbol }}</span>
                        <span class="hc-class">{{ classLabel(v) }}</span>
                        <span
                          v-if="v.disposition.low_confidence_grade"
                          class="ana-tag"
                          title="An imputed (neutral) sub-score underlies this letter"
                          >Low confidence</span
                        >
                        <span
                          v-if="v.disposition.structural_flag"
                          class="ana-tag"
                          title="Option-overlay vehicle — structurally path-dependent; the Low risk tier is barred"
                          >Structurally path-dependent</span
                        >
                        <span v-if="noLongerHeld(v.symbol)" class="ana-tag"
                          >No longer held</span
                        >
                      </div>
                      <div class="hc-name">
                        {{ positionFor(v.symbol)?.description ?? "" }}
                      </div>
                    </div>
                  </div>
                  <div class="hc-unrealized">
                    <span class="hc-kicker">Unrealized</span>
                    <span
                      v-if="gainOf(positionFor(v.symbol)) !== null"
                      class="dir hc-gain"
                      :class="dirOf(gainOf(positionFor(v.symbol)))"
                    >
                      {{ fmtSigned(gainOf(positionFor(v.symbol))!) }}
                      ({{ fmtPct(gainPctOf(positionFor(v.symbol))!) }})
                    </span>
                    <span v-else class="ana-num hc-gain-none">—</span>
                  </div>
                </header>

                <!-- Two linked blocks: intrinsic verdict beside portfolio action
                     (distinct but linked — docs/interface.md). -->
                <div class="hc-body">
                  <div class="hc-col hc-col-intrinsic">
                    <span class="hc-kicker">Intrinsic verdict</span>
                    <div class="hc-subscores">
                      <div
                        v-for="(score, name) in v.disposition.sub_scores"
                        :key="name"
                        class="hc-sub"
                      >
                        <span class="hc-sub-label">{{ name }}</span>
                        <span class="ana-num hc-sub-value">{{
                          Math.round(score)
                        }}</span>
                      </div>
                    </div>
                    <dl class="hc-kv">
                      <dt>Conviction</dt>
                      <dd>
                        <span
                          class="conviction"
                          role="img"
                          :aria-label="`Conviction: ${v.disposition.conviction}`"
                        >
                          <i
                            v-for="i in 3"
                            :key="i"
                            :class="{
                              on: i <= CONVICTION_LEVEL[v.disposition.conviction],
                            }"
                          />
                        </span>
                        <span class="hc-conviction-word">{{
                          v.disposition.conviction
                        }}</span>
                      </dd>
                      <template v-if="v.disposition.price_targets.one_month">
                        <dt>1-mo target</dt>
                        <dd>
                          <span class="ana-num"
                            >{{
                              moneyExact.format(
                                v.disposition.price_targets.one_month.base
                              )
                            }}
                            <span class="hc-band"
                              >({{
                                moneyExact.format(
                                  v.disposition.price_targets.one_month.bear
                                )
                              }}–{{
                                moneyExact.format(
                                  v.disposition.price_targets.one_month.bull
                                )
                              }})</span
                            ></span
                          >
                        </dd>
                      </template>
                      <template v-if="v.disposition.price_targets.twelve_month">
                        <dt>12-mo target</dt>
                        <dd>
                          <span class="ana-num"
                            >{{
                              moneyExact.format(
                                v.disposition.price_targets.twelve_month.base
                              )
                            }}
                            <span class="hc-band"
                              >({{
                                moneyExact.format(
                                  v.disposition.price_targets.twelve_month.bear
                                )
                              }}–{{
                                moneyExact.format(
                                  v.disposition.price_targets.twelve_month.bull
                                )
                              }})</span
                            ></span
                          >
                        </dd>
                      </template>
                      <dt>Outlook</dt>
                      <dd class="hc-outlook">
                        <span
                          v-for="(read, horizon) in v.disposition.horizon_outlook"
                          :key="horizon"
                          class="hc-horizon"
                        >
                          <span class="hc-horizon-label">{{ horizon }}</span>
                          <span class="dir" :class="HORIZON_DIR[read]">{{
                            read
                          }}</span>
                        </span>
                      </dd>
                    </dl>
                    <!-- Target methodology: engine-computed figures, exposed
                         (a Reveal-style inline disclosure, not a popover). -->
                    <button
                      type="button"
                      class="hc-reveal"
                      :aria-expanded="openMethodology.has(v.symbol)"
                      @click="toggleMethodology(v.symbol)"
                    >
                      <span aria-hidden="true" class="hc-reveal-glyph">{{
                        openMethodology.has(v.symbol) ? "▾" : "▸"
                      }}</span>
                      Target methodology
                    </button>
                    <div
                      v-if="openMethodology.has(v.symbol)"
                      class="hc-methodology"
                    >
                      <p
                        v-if="v.disposition.price_targets.twelve_month"
                        class="hc-prose"
                      >
                        {{ v.disposition.price_targets.twelve_month.methodology }}
                      </p>
                      <p class="hc-prose">
                        {{ v.disposition.price_target_rationale }}
                      </p>
                    </div>
                  </div>

                  <div class="hc-col">
                    <span class="hc-kicker">Portfolio action</span>
                    <div class="hc-action">
                      <span class="hc-action-word">{{
                        ACTION_LABELS[v.disposition.action]
                      }}</span>
                      <span class="hc-action-band"
                        >to
                        {{
                          weightBand(
                            v.disposition.action_sizing.target_weight_low,
                            v.disposition.action_sizing.target_weight_high
                          )
                        }}</span
                      >
                    </div>
                    <dl class="hc-kv">
                      <dt>Weight</dt>
                      <dd>
                        <span class="ana-num">{{
                          weightOf(positionFor(v.symbol)) !== null
                            ? fmtPct(weightOf(positionFor(v.symbol))!)
                            : "—"
                        }}</span>
                      </dd>
                      <template
                        v-if="v.disposition.action_sizing.est_share_delta !== null"
                      >
                        <dt>Est. shares</dt>
                        <dd>
                          <span class="ana-num">{{
                            (v.disposition.action_sizing.est_share_delta > 0
                              ? "+"
                              : "") +
                            qtyFmt.format(
                              v.disposition.action_sizing.est_share_delta
                            )
                          }}</span>
                        </dd>
                      </template>
                      <template
                        v-if="v.disposition.action_sizing.est_dollar_delta !== null"
                      >
                        <dt>Est. adj.</dt>
                        <dd>
                          <span class="ana-num">{{
                            fmtSigned(v.disposition.action_sizing.est_dollar_delta)
                          }}</span>
                        </dd>
                      </template>
                      <template v-if="hasOptionsSignal(v.disposition.options_signal)">
                        <template
                          v-if="v.disposition.options_signal.put_call_volume !== null"
                        >
                          <dt>Put/call vol</dt>
                          <dd>
                            <span class="ana-num">{{
                              v.disposition.options_signal.put_call_volume.toFixed(2)
                            }}</span>
                          </dd>
                        </template>
                        <template
                          v-if="
                            v.disposition.options_signal.put_call_open_interest !==
                            null
                          "
                        >
                          <dt>Put/call OI</dt>
                          <dd>
                            <span class="ana-num">{{
                              v.disposition.options_signal.put_call_open_interest.toFixed(
                                2
                              )
                            }}</span>
                          </dd>
                        </template>
                        <template
                          v-if="
                            v.disposition.options_signal.implied_volatility !== null
                          "
                        >
                          <dt>ATM IV</dt>
                          <dd>
                            <span class="ana-num">{{
                              fmtPct(v.disposition.options_signal.implied_volatility)
                            }}</span>
                          </dd>
                        </template>
                        <template
                          v-if="v.disposition.options_signal.iv_skew !== null"
                        >
                          <dt>IV skew</dt>
                          <dd>
                            <span class="ana-num">{{
                              (v.disposition.options_signal.iv_skew > 0 ? "+" : "") +
                              fmtPct(v.disposition.options_signal.iv_skew)
                            }}</span>
                          </dd>
                        </template>
                      </template>
                    </dl>
                  </div>
                </div>

                <!-- Financial analysis — model prose over engine numbers. -->
                <div v-if="v.disposition.financial_summary" class="hc-summary">
                  <span class="hc-kicker">Financial analysis</span>
                  <p class="hc-prose">{{ v.disposition.financial_summary }}</p>
                </div>

                <!-- What changed + the app-computed position delta. -->
                <footer class="hc-foot">
                  <div class="hc-foot-main">
                    <span class="hc-kicker">What changed · since last run</span>
                    <p class="hc-changed">{{ v.disposition.what_changed }}</p>
                  </div>
                  <span class="ana-tag" :title="'Position vs. prior run'"
                    >Position: {{ CHANGE_LABELS[v.position_change] }}</span
                  >
                </footer>
              </template>
            </article>
          </div>

          <!-- Whole-book roll-up (+ the exited positions from the holdings diff). -->
          <section class="ana-card rollup" aria-label="Portfolio roll-up">
            <header class="rollup-head">
              <span class="ana-head">Roll-up · whole book</span>
              <span class="ch-stamp">Analyzed {{ fmtStamp(run.created_at) }}</span>
            </header>
            <p class="rollup-overview hc-prose">{{ run.roll_up.overview }}</p>
            <div v-if="run.roll_up.exited.length > 0" class="rollup-exited">
              <span class="hc-kicker">Positions closed since last run</span>
              <ul class="exited-list">
                <li v-for="x in run.roll_up.exited" :key="x.symbol">
                  <span class="ana-ticker">{{ x.symbol }}</span>
                  <span class="exited-desc">{{ x.description }}</span>
                  <span class="ana-num exited-figures"
                    >{{ qtyFmt.format(x.prior_quantity) }} ·
                    {{ fmtMoney(x.prior_market_value) }}</span
                  >
                </li>
              </ul>
            </div>
          </section>
        </template>
      </template>
    </div>
  </section>
</template>

<style scoped>
.portfolio-pane {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--paper);
}

/* Toolbar — same tier as the report/settings toolbars (surface eyebrow left,
   actions right), so the seam lines up with the sidebar header across the gutter. */
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--s-4);
  min-height: 50px;
  padding: 0 var(--s-6);
  border-bottom: var(--border);
  flex-shrink: 0;
}

.toolbar-label {
  font-family: var(--font-sans);
  font-size: 13px;
  font-weight: 600;
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink);
}

.toolbar-actions {
  display: flex;
  align-items: center;
  gap: var(--s-3);
}

/* Inline run-gate block / failure. Chrome register (sans on paper-edge), like
   the warning band — never the serif reading register. */
.pane-error {
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  padding: var(--s-3) var(--s-6);
  background: var(--paper-edge);
  border-bottom: var(--border);
  flex-shrink: 0;
}

.pane-error-label {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  font-weight: 600;
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--accent-text);
  white-space: nowrap;
}

.pane-error-detail {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
  overflow-wrap: anywhere;
}

.pane-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--s-6) var(--s-7) 96px;
}

.pane-quiet {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
  margin: var(--s-6) 0;
}

.pane-quiet .pane-error-detail {
  display: block;
  margin-top: var(--s-2);
}

/* Empty / pulled-not-analyzed states — serif headline + explanation, per the
   kit's EmptyPortfolio (copy updated to the independent-triggers decision). */
.empty-state,
.pulled-only {
  max-width: 60ch;
  margin: var(--s-8) auto 0;
}

.pulled-only {
  margin-bottom: var(--s-6);
}

.empty-title {
  font-family: var(--font-serif);
  font-size: 22px;
  font-weight: 600;
  color: var(--ink);
  margin: 0 0 var(--s-3);
}

.empty-body {
  font-family: var(--font-serif);
  font-size: 15px;
  line-height: 1.55;
  color: var(--ink-2);
  margin: 0;
}

/* Content column cap: dense analytical surfaces read best bounded (the kit
   caps at 980px); the pane itself keeps scrolling behavior. */
.strip,
.current-holdings,
.ana-sortbar,
.card-stack,
.rollup {
  max-width: 980px;
  margin-left: auto;
  margin-right: auto;
}

.strip {
  margin-bottom: var(--s-6);
}

/* Key-figure strip wraps on narrow windows rather than crushing the figures.
   Every cell draws its own top+left hairline, shifted -1px so row-leading and
   column-leading seams tuck under the container border (overflow clips them) —
   otherwise a wrapped row's first cell would keep a stray inner hairline. */
.strip.keyfig {
  grid-auto-flow: row;
  grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
  overflow: hidden;
}

.strip.keyfig > .kf,
.strip.keyfig > .kf:first-child {
  border-left: 1px solid var(--hairline-soft);
  border-top: 1px solid var(--hairline-soft);
  margin: -1px 0 0 -1px;
}

/* ---- Current holdings (the standalone pull) ---- */
.current-holdings {
  margin-bottom: var(--s-6);
}

.ch-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--s-4);
  flex-wrap: wrap;
  padding: var(--s-4) var(--s-5);
  border-bottom: 1px solid var(--hairline-soft);
}

.ana-head {
  font-family: var(--font-sans);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--ink-3);
}

.ch-stamp {
  font-family: var(--font-mono);
  font-size: 11px;
  font-variant-numeric: tabular-nums lining-nums;
  color: var(--ink-3);
}

/* The table scrolls inside its card on narrow windows; the page never
   h-scrolls. Focusable so keyboard users can scroll it. */
.ch-scroll {
  overflow-x: auto;
}

.ch-scroll:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.ch-tag {
  margin-left: var(--s-2);
}

.ch-foot {
  display: flex;
  justify-content: flex-end;
  gap: var(--s-6);
  padding: var(--s-3) var(--s-5);
  border-top: 1px solid var(--hairline-soft);
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}

.ch-foot .ana-num {
  color: var(--ink);
  margin-left: var(--s-2);
}

/* ---- Sort bar (design package .ana-sortbar; spacing only here) ---- */
.ana-sortbar {
  margin-bottom: var(--s-4);
}

/* ---- Holding cards ---- */
.card-stack {
  display: flex;
  flex-direction: column;
  gap: var(--s-5);
}

.hc-kicker {
  display: block;
  font-family: var(--font-sans);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--ink-3);
}

/* Reduced (not-rated / insufficient) card */
.hc-reduced {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--s-5);
  padding: var(--s-4) var(--s-5);
}

.hc-reduced-main {
  min-width: 0;
}

.hc-idline {
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  flex-wrap: wrap;
}

.hc-idline .ana-ticker {
  font-size: 15px;
}

.hc-class {
  font-family: var(--font-sans);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--ink-3);
  white-space: nowrap;
}

.hc-reason {
  font-family: var(--font-serif);
  font-size: 13px;
  line-height: 1.45;
  color: var(--ink-2);
  margin: var(--s-2) 0 0;
  max-width: 70ch;
}

.hc-reduced-side {
  text-align: right;
  flex-shrink: 0;
}

.hc-weight {
  display: block;
  font-size: 14px;
  color: var(--ink-2);
  margin-top: var(--s-1);
}

/* Graded card */
.hc-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--s-5);
  padding: var(--s-5) var(--s-5) var(--s-4);
  border-bottom: 1px solid var(--hairline-soft);
}

.hc-id {
  display: flex;
  align-items: center;
  gap: var(--s-4);
  min-width: 0;
}

.hc-grade {
  min-width: 34px;
  height: 30px;
  font-size: 18px;
  flex-shrink: 0;
}

.hc-id-text {
  min-width: 0;
}

.hc-name {
  font-family: var(--font-sans);
  font-size: 12px;
  color: var(--ink-3);
  margin-top: 1px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.hc-unrealized {
  text-align: right;
  flex-shrink: 0;
}

.hc-gain {
  font-size: 15px;
}

.hc-gain-none {
  color: var(--ink-3);
}

/* Two linked columns; stack on narrow windows so nothing crushes. */
.hc-body {
  display: grid;
  grid-template-columns: 1fr 1fr;
}

@media (max-width: 760px) {
  .hc-body {
    grid-template-columns: 1fr;
  }

  .hc-col-intrinsic {
    border-right: 0 !important;
    border-bottom: 1px solid var(--hairline-soft);
  }
}

.hc-col {
  padding: var(--s-4) var(--s-5);
  min-width: 0;
}

.hc-col-intrinsic {
  border-right: 1px solid var(--hairline-soft);
}

.hc-col > .hc-kicker {
  margin-bottom: var(--s-3);
}

.hc-subscores {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0 var(--s-2);
  margin-bottom: var(--s-4);
}

.hc-sub-label {
  display: block;
  font-family: var(--font-sans);
  font-size: 9px;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--ink-3);
  margin-bottom: 2px;
}

.hc-sub-value {
  font-size: 14px;
  color: var(--ink);
}

.hc-kv {
  display: grid;
  grid-template-columns: max-content 1fr;
  column-gap: var(--s-4);
  row-gap: var(--s-2);
  margin: 0;
  font-family: var(--font-sans);
  font-size: 12px;
}

.hc-kv dt {
  color: var(--ink-3);
  white-space: nowrap;
}

.hc-kv dd {
  margin: 0;
  color: var(--ink);
  min-width: 0;
}

.hc-band {
  color: var(--ink-3);
}

.hc-conviction-word {
  margin-left: var(--s-2);
  text-transform: capitalize;
}

.hc-outlook {
  display: flex;
  gap: var(--s-4);
  flex-wrap: wrap;
}

.hc-horizon {
  display: inline-flex;
  align-items: baseline;
  gap: var(--s-2);
}

.hc-horizon-label {
  font-family: var(--font-sans);
  font-size: 10px;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--ink-3);
}

.hc-horizon .dir {
  font-size: 12px;
  text-transform: capitalize;
}

/* Reveal disclosure (kit Reveal): tracked-caps trigger + inline body. */
.hc-reveal {
  display: inline-flex;
  align-items: center;
  gap: var(--s-2);
  appearance: none;
  background: transparent;
  border: 0;
  padding: 2px 0;
  margin-top: var(--s-4);
  cursor: pointer;
  font-family: var(--font-sans);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--ink-3);
}

.hc-reveal:hover {
  color: var(--ink-2);
}

.hc-reveal:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.hc-reveal-glyph {
  font-family: var(--font-mono);
  font-size: 11px;
}

.hc-methodology {
  margin-top: var(--s-3);
}

.hc-prose {
  font-family: var(--font-serif);
  font-size: 13px;
  line-height: 1.5;
  letter-spacing: -0.006em;
  color: var(--ink-2);
  margin: 0;
  max-width: 78ch;
  overflow-wrap: anywhere;
}

.hc-prose + .hc-prose {
  margin-top: var(--s-2);
}

.hc-action {
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  margin-bottom: var(--s-3);
  flex-wrap: wrap;
}

.hc-action-word {
  font-family: var(--font-sans);
  font-size: 15px;
  font-weight: 600;
  color: var(--ink);
}

.hc-action-band {
  font-family: var(--font-sans);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--ink-3);
  white-space: nowrap;
}

.hc-summary {
  padding: var(--s-4) var(--s-5);
  border-top: 1px solid var(--hairline-soft);
}

.hc-summary .hc-kicker {
  margin-bottom: var(--s-2);
}

.hc-foot {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--s-5);
  padding: var(--s-3) var(--s-5);
  border-top: 1px solid var(--hairline-soft);
  background: var(--paper-edge);
}

.hc-foot-main {
  min-width: 0;
}

.hc-foot .hc-kicker {
  margin-bottom: 2px;
}

.hc-changed {
  font-family: var(--font-sans);
  font-size: 12px;
  color: var(--ink-2);
  margin: 0;
  overflow-wrap: anywhere;
}

.hc-foot .ana-tag {
  flex-shrink: 0;
  margin-top: 2px;
}

/* ---- Roll-up ---- */
.rollup {
  margin-top: var(--s-6);
}

.rollup-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--s-4);
  flex-wrap: wrap;
  padding: var(--s-4) var(--s-5);
  border-bottom: 1px solid var(--hairline-soft);
}

.rollup-overview {
  padding: var(--s-4) var(--s-5);
}

.rollup-exited {
  padding: var(--s-4) var(--s-5);
  border-top: 1px solid var(--hairline-soft);
}

.rollup-exited .hc-kicker {
  margin-bottom: var(--s-3);
}

.exited-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--s-2);
}

.exited-list li {
  display: flex;
  align-items: baseline;
  gap: var(--s-4);
  font-size: 12px;
}

.exited-desc {
  font-family: var(--font-sans);
  color: var(--ink-2);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.exited-figures {
  margin-left: auto;
  color: var(--ink-3);
  white-space: nowrap;
}

/* Reduced motion: the buttons/toggles inherit the package's transitions; no
   motion originates here beyond those, so nothing extra to suppress. */
</style>
