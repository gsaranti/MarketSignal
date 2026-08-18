<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { etDayDiff } from "../etDate";
import { localDate, localDateTime } from "../format";
import type {
  FlagTrigger,
  GradedVerdict,
  HoldingQuickState,
  HoldingsPull,
  HoldingVerdict,
  HorizonOutlook,
  PortfolioConviction,
  PortfolioRun,
  Position,
  QuickCheckState,
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
  // Viewing a persisted past run from the sidebar's runs history — read-only
  // (docs/interface.md §Main Layout): the triggers lock, the current-holdings
  // comparison (keyed to the latest vintage) hides, and a banner names the
  // vintage with a way back. Optional so the latest-run mounting stays the
  // default.
  historical?: boolean;
  // A past-run OPEN failure — its own channel so it never renders under the
  // "Couldn't run" label; App clears it on the next selection / back-to-latest.
  historyError?: string | null;
  // The latest quick-check state (docs/portfolio-analysis.md §The quick check)
  // — the card overlay's source: attention flags, evidence-event badges,
  // degraded-sweep notes. Null when no quick check has run since the last full
  // pass; applied only while it swept the rendered (latest) run.
  quick?: QuickCheckState | null;
  quickChecking?: boolean;
  // The history holds a row that couldn't be decoded (it would otherwise BE the
  // latest view) — the page must say unreadable, never never-ran.
  unreadableHistory?: boolean;
  // The runs-history listing itself failed to load: with `run` null the page
  // cannot tell a never-ran store from one whose listing errored, so the
  // empty state must not claim either.
  historyUnknown?: boolean;
}>();

const emit = defineEmits<{
  // With a payload, the run is a selective re-analysis over those symbols
  // (docs/portfolio-analysis.md §Triggering); without one, the whole book.
  (e: "run", selected?: string[]): void;
  (e: "pull"): void;
  (e: "quick-check"): void;
  (e: "back-to-latest"): void;
}>();

// ---- Triggers ---------------------------------------------------------------

const isHistorical = computed(() => props.historical ?? false);

const runDisabled = computed(
  () => isHistorical.value || props.runBlocked || props.busy
);
const pullDisabled = computed(
  () => isHistorical.value || props.pullBlocked || props.busy
);

// ---- Selective re-analysis selection (docs/portfolio-analysis.md §Triggering)
// Per-card selection driving the selective run — view-local UI state, never
// persisted, reset when the rendered run changes (a new run is a new selection
// context). Design-package note: the analytical register defines no selection
// control, so this extends it minimally — a hairline 2px-radius box beside the
// ticker (checked = accent fill, the actionable state), the register's shared
// focus-visible ring — recorded as an extension, not silently invented.
const selected = ref<Set<string>>(new Set());
const selectionActive = computed(() => selected.value.size > 0);
const selectionDisabled = computed(
  () => isHistorical.value || props.busy || props.run === null
);
// Holdings present in the book but absent from this run's verdicts — new /
// unselected holdings a selective run left not analyzed since the 2026-08-16
// badge ruling (docs/portfolio-analysis.md §Triggering). Derived from
// holdings-minus-verdicts; rendered as placeholder "run to grade" cards,
// selectable so the next selective run can grade them.
const notAnalyzed = computed<Position[]>(() => {
  const graded = new Set(
    (props.run?.verdicts ?? []).map((v) => v.symbol.toUpperCase())
  );
  return (props.run?.holdings.positions ?? []).filter(
    (p) => !graded.has(p.symbol.toUpperCase())
  );
});
// Every current holding a selective run can target — graded/carried verdicts
// plus the not-analyzed holdings above.
const selectableSymbols = computed<string[]>(() => [
  ...(props.run?.verdicts ?? []).map((v) => v.symbol.toUpperCase()),
  ...notAnalyzed.value.map((p) => p.symbol.toUpperCase()),
]);
const allSelected = computed(
  () =>
    selectableSymbols.value.length > 0 &&
    selected.value.size === selectableSymbols.value.length
);
function isSelected(symbol: string): boolean {
  return selected.value.has(symbol.toUpperCase());
}
function toggleSelect(symbol: string) {
  const next = new Set(selected.value);
  const key = symbol.toUpperCase();
  if (next.has(key)) next.delete(key);
  else next.add(key);
  selected.value = next;
}
function selectAll() {
  selected.value = new Set(selectableSymbols.value);
}
function clearSelection() {
  selected.value = new Set();
}
watch(
  () => props.run?.run_id,
  () => clearSelection()
);
function onRun() {
  emit("run", selectionActive.value ? [...selected.value] : undefined);
}

// The disabled reason, surfaced as the button title so the lock is explicable
// in place (the warning band carries the full items).
const runTitle = computed(() => {
  if (isHistorical.value)
    return "Viewing a past analysis — go back to the latest run to run jobs";
  if (props.runBlocked)
    return props.runBlockedReason ?? "Local-suite configuration is incomplete";
  if (props.busy) return "Another job is running";
  if (selectionActive.value)
    return "Re-analyze the selected holdings — the rest carry forward, badged where the safety sweep flags a change";
  return "Pull fresh holdings and run the analysis";
});
const pullTitle = computed(() => {
  if (isHistorical.value)
    return "Viewing a past analysis — go back to the latest run to run jobs";
  if (props.pullBlocked)
    return props.pullBlockedReason ?? "Schwab account not connected";
  if (props.busy) return "Another job is running";
  return "Fetch current positions without running the analysis";
});

// The quick check shares the run trigger's presence gate (it skips only the
// daemon-connectivity probe, which the frontend lock never carried anyway —
// docs/interface.md §Connection status) and needs a run to sweep.
const quickDisabled = computed(
  () => runDisabled.value || props.run === null
);
const quickTitle = computed(() => {
  if (isHistorical.value)
    return "Viewing a past analysis — go back to the latest run to run jobs";
  if (props.runBlocked)
    return props.runBlockedReason ?? "Local-suite configuration is incomplete";
  if (props.busy) return "Another job is running";
  if (props.run === null) return "Run an analysis first — there is no thesis ledger to check yet";
  return "Re-check every standing thesis ledger against fresh data — engine-only, no model call";
});

// ---- Quick-check card overlay -------------------------------------------------
// Applied only on the latest live view: the flags describe the latest vintage,
// and only a state swept against the rendered run is honest to overlay.
const quickOverlayActive = computed(
  () =>
    !isHistorical.value &&
    props.quick != null &&
    props.run !== null &&
    props.quick.swept_run_id === props.run.run_id
);
const quickBySymbol = computed(() => {
  const map = new Map<string, HoldingQuickState>();
  if (!quickOverlayActive.value) return map;
  for (const h of props.quick!.holdings) map.set(h.symbol.toUpperCase(), h);
  return map;
});
function quickFor(symbol: string): HoldingQuickState | null {
  return quickBySymbol.value.get(symbol.toUpperCase()) ?? null;
}
// The side-reversal badge title — the carried thesis is for the opposite
// position (docs/portfolio-analysis.md §Triggering).
const SIDE_REVERSED_TITLE =
  "This position's net side flipped since this verdict was written — the carried thesis is for the opposite position; re-run to refresh";
const FLAG_LABELS: Record<FlagTrigger, string> = {
  "confirmed-falsifier-breach": "falsifier breached",
  "fired-trigger": "trigger fired",
  "hurdle-newly-fails": "hurdle newly fails",
  "price-outside-band": "band relation changed",
};
function flagLabel(trigger: FlagTrigger): string {
  return FLAG_LABELS[trigger] ?? trigger;
}
function flagTitle(h: HoldingQuickState): string {
  const flag = h.flag!;
  return `${flag.detail} — raised ${fmtStamp(flag.raised_at)}; a full or selective analysis over this holding clears it`;
}
// The quiet evidence-event badge: the count carries in the text, the details in
// the title (never the amber action color — docs/interface.md).
function eventBadge(h: HoldingQuickState): { text: string; title: string } | null {
  if (h.evidence_events.length === 0) return null;
  const n = h.evidence_events.length;
  return {
    text: n === 1 ? "Evidence event" : `Evidence events ×${n}`,
    title: h.evidence_events.map((e) => e.detail).join(" · "),
  };
}
// The degraded-sweep note: names the families the sweep couldn't check.
function degradedBadge(h: HoldingQuickState): { text: string; title: string } | null {
  const unknown = h.families.filter((f) => f.state === "unknown");
  if (unknown.length === 0) return null;
  const names = unknown.map((f) => f.family.replace(/-/g, " ")).join(", ");
  return {
    text: "Sweep degraded",
    title: `Couldn't verify: ${names}. ${unknown
      .map((f) => f.note)
      .filter(Boolean)
      .join(" · ")}`,
  };
}

// ---- Analysis-vintage stamp (docs/portfolio-analysis.md §Triggering) ---------
// A verdict whose analyzed_at is older than the run's created_at was not
// re-analyzed by that run — a selective run's carry, or an abstention holding
// its prior vintage — the stamp the card shows. Over-age mirrors the engine's
// 28-day carry boundary exactly: whole ET session days, date-diffed (job.rs
// OVER_AGE_DAYS + market_clock::et_date_of — recalibrate both together; a
// fractional-ms age disagreed with the engine around the boundary); quiet
// informational tags, never the amber action color.
const OVER_AGE_DAYS = 28;
function carriedStamp(
  v: HoldingVerdict
): { text: string; title: string } | null {
  const run = props.run;
  if (!run || !v.analyzed_at || v.analyzed_at === run.created_at) return null;
  const ageDays = etDayDiff(v.analyzed_at, run.created_at);
  if (ageDays === null) return null;
  const when = localDate(v.analyzed_at);
  return ageDays > OVER_AGE_DAYS
    ? {
        text: `Stale · analyzed ${when}`,
        title:
          "Standing on a full pass older than the ~4-week research window — select it to refresh the analysis",
      }
    : {
        text: `Carried · analyzed ${when}`,
        title: "Standing on an earlier full pass — not re-analyzed this run",
      };
}
function demoted(v: HoldingVerdict): boolean {
  return v.action_source === "rule-demoted";
}
const DEMOTED_TITLE =
  "An over-age carried add action was rule-demoted to hold — a labeled, rule-based weaken, not fresh analysis";

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
  const s = (fraction * 100).toFixed(digits);
  // A value that rounds to zero must not keep its sign — toFixed renders
  // −0.0004 as "-0.0", a signed-zero artifact.
  return `${Number(s) === 0 ? (0).toFixed(digits) : s}%`;
}
function fmtSigned(v: number): string {
  // Sign keyed on the rendered (cent-rounded) value, so a sub-cent negative
  // can't render "−$0.00" (and −0 normalizes to 0).
  const cents = Math.round(v * 100) / 100;
  const shown = cents === 0 ? 0 : cents;
  return `${shown > 0 ? "+" : ""}${fmtMoney(shown)}`;
}
function fmtSignedPct(fraction: number): string {
  // "+" keyed on the rendered percent, not the raw fraction — +0.0004 rounds
  // to "0.0%" and must not read "+0.0%".
  const s = fmtPct(fraction);
  return Number.parseFloat(s) > 0 ? `+${s}` : s;
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

// The card header's position facts, each falling to "—" when the pull left
// the input unreported (cash rows carry no cost basis; some vehicles no price).
function priceOf(pos: Position | null): number | null {
  return pos?.current_price ?? null;
}
function costBasisOf(pos: Position | null): number | null {
  if (!pos || pos.cost_basis <= 0 || multiplierUnverified(pos)) return null;
  return pos.cost_basis;
}
// The across-orders average cost per share — the netted book-level cost total
// over the netted quantity, never a share-weighted average of source rows.
function avgCostOf(pos: Position | null): number | null {
  if (!pos || pos.cost_basis <= 0 || pos.quantity <= 0 || multiplierUnverified(pos)) {
    return null;
  }
  return pos.cost_basis / pos.quantity;
}

// Unrealized P/L from the two Schwab-reported totals. A position with no
// reported cost basis (cash, typically — arrives as 0) has an undefined gain;
// a NEGATIVE netted basis (a net-short book, where the short side's proceeds
// offset the basis) keeps its dollar gain — the signed totals hold
// market value − cost basis equal to the book's aggregate unrealized P/L
// (docs/portfolio-analysis.md §Storage and display). Zero stays undefined:
// on the wire it is indistinguishable from "no basis reported".
// Option and fixed-income rows render NO gain: Schwab's `averagePrice` carries
// a contract/par multiplier convention the parse doesn't apply (an option's
// basis would be understated ~100×, a bond's overstated ~1000×), so the
// derived number is withheld rather than fabricated until the wire convention
// is probed (piece-3 walk, 2026-08-05; big-run probe listed in the record).
function multiplierUnverified(pos: Position): boolean {
  return (
    pos.asset_class === "option-contract" || pos.asset_class === "fixed-income"
  );
}
function gainOf(pos: Position | null): number | null {
  if (!pos || pos.cost_basis === 0 || multiplierUnverified(pos)) return null;
  return pos.market_value - pos.cost_basis;
}
function gainPctOf(pos: Position | null): number | null {
  if (!pos || pos.cost_basis <= 0 || multiplierUnverified(pos)) return null;
  return (pos.market_value - pos.cost_basis) / pos.cost_basis;
}
function dirOf(v: number | null): "up" | "down" | "flat" {
  if (v === null || v === 0) return "flat";
  return v > 0 ? "up" : "down";
}
// Direction keyed on the RENDERED value, not the raw one: a fraction whose text
// rounds to "0.0%" (or a dollar gain to "$0.00") must not carry a red/green
// treatment its own number no longer shows.
function pctDir(fraction: number | null): "up" | "down" | "flat" {
  return dirOf(fraction === null ? null : Math.round(fraction * 1000) / 1000);
}
function moneyDir(v: number | null): "up" | "down" | "flat" {
  return dirOf(v === null ? null : Math.round(v * 100) / 100);
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
// (before any run it IS the page body) — and never on a historical view, whose
// run is not the vintage the pull compares against.
const showCurrentHoldings = computed(
  () => !isHistorical.value && props.pull !== null && pullIsFresher.value
);

const pullSymbols = computed(() => {
  const s = new Set<string>();
  for (const p of props.pull?.holdings.positions ?? []) s.add(p.symbol);
  return s;
});

function newSinceAnalysis(symbol: string): boolean {
  return props.run !== null && !runPositions.value.has(symbol);
}
function noLongerHeld(symbol: string): boolean {
  // Suppressed on a historical view: the tag compares the latest pull against
  // the *latest* run's vintage, a pairing a past run doesn't participate in.
  return (
    !isHistorical.value &&
    props.run !== null &&
    pullIsFresher.value &&
    !pullSymbols.value.has(symbol)
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
      // Suppression-aware: an option/bond row's fabricated basis must not rank
      // either (null sorts last, like the hidden render).
      return costBasisOf(pos);
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
      // Same suppression-aware key as the card stack's cost sort.
      return costBasisOf(p);
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

// The three grade inputs, in tile order. Momentum is deliberately not here:
// it is the market-setup read in the conviction context, outside the letter
// (docs/portfolio-analysis.md §Starting parameters), so its tile renders set
// apart behind a divider. The kit's SubScores still shows it undifferentiated —
// a recorded deviation (B10 ruling, 2026-08-05), not drift to revert.
const LETTER_SUBSCORES = ["quality", "valuation", "risk"] as const;

// Always-visible beneath the tile row — the outside-the-letter explanation
// must not be hover-only (Codex P2, ruled 2026-08-05): one copy source serves
// pointer, keyboard, low-vision, and screen-reader users alike.
const SETUP_NOTE = "Setup — market-setup read, outside the letter";

// ---- The two-arm verdict (portfolio-v7) --------------------------------------
// The engine baseline beside the model's own view. Every persisted verdict
// carries both arms.

const MODEL_ARM_NOTE =
  "Model view — the model's own numbers, unrestricted; scored against the baseline";
const MODEL_LETTER_TITLE =
  "The model's letter, derived from its own quality/valuation/risk through the " +
  "shared cutoffs";

// The model outlook's ≠ engine read compares per-horizon against the stand-in;
// any differing window tags the row — one quiet tag, the conviction/lean idiom.
function outlookDiverges(d: GradedVerdict): boolean {
  const ev = d.engine_view;
  return (["short", "mid", "long"] as const).some(
    (h) => d.horizon_outlook[h] !== ev.outlook[h]
  );
}

// Column A renders the engine stand-in's conviction/outlook.
function armAConviction(d: GradedVerdict): PortfolioConviction {
  return d.engine_view.conviction;
}
function armAOutlook(d: GradedVerdict): HorizonOutlook {
  return d.engine_view.outlook;
}

// The matured scoreboard lines for one symbol, from the run's outcome records —
// engine-computed, quiet-note register on the card foot.
function maturedLinesFor(symbol: string): string[] {
  const matured = props.run?.outcome?.matured ?? [];
  return matured
    .filter((m) => m.symbol.toUpperCase() === symbol.toUpperCase())
    .map((m) => {
      const detail =
        m.total_return !== null
          ? `total return ${(m.total_return * 100).toFixed(1)}%`
          : m.price_return !== null
            ? `price-only ${(m.price_return * 100).toFixed(1)}%`
            : m.outcome;
      return `${m.window_months}-mo window ${m.outcome} (${detail})`;
    });
}

// The roll-up's model-vs-engine scoreboard lines: the PAIRED head-to-head per
// window (both arms scored over the identical episode set — the backend's
// same-events contract) plus the outlook direction hit-rates. Empty until v7
// episodes mature.
const scoreboardLines = computed<string[]>(() => {
  const reads = props.run?.outcome?.reads;
  if (!reads) return [];
  const lines: string[] = [];
  for (const h of reads.head_to_head ?? []) {
    if (
      h.scored > 0 &&
      h.model_mean_interval_score !== null &&
      h.engine_mean_interval_score !== null
    ) {
      lines.push(
        `${h.window_months}-mo interval score (paired, ${h.scored}): ` +
          `model ${h.model_mean_interval_score.toFixed(3)} ` +
          `vs engine ${h.engine_mean_interval_score.toFixed(3)} — lower is better`
      );
    }
  }
  for (const window of [1, 6, 12]) {
    const arm = (name: string) =>
      (reads.outlook_direction ?? []).find(
        (r) => r.arm === name && r.window_months === window && r.scored > 0
      );
    const engine = arm("engine");
    const model = arm("model");
    if (engine && model) {
      lines.push(
        `${window}-mo direction: model ${model.hits}/${model.scored} ` +
          `vs engine ${engine.hits}/${engine.scored}`
      );
    }
  }
  return lines;
});

function gradeClass(grade: string): string {
  return grade.toLowerCase();
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

// ---- Standing-thesis anchor (the kit's ThesisAnchor overflow contract) -----------
// A long model-authored thesis clamps to three lines with an accessible reveal,
// shown only when the text actually overflows (market-signal-design-system
// ui_kits Portfolio.jsx). Keyed per symbol, like the methodology disclosure.

const openThesis = ref<Set<string>>(new Set());
const thesisOverflow = ref<Set<string>>(new Set());
const thesisEls = new Map<string, HTMLElement>();
// Re-measure on card-width changes so the toggle tracks real overflow; absent
// ResizeObserver (older webviews), the mount/update measurement stands alone.
const thesisObserver =
  typeof ResizeObserver !== "undefined"
    ? new ResizeObserver(() => {
        for (const [symbol, el] of thesisEls) measureThesis(symbol, el);
      })
    : null;
function measureThesis(symbol: string, el: HTMLElement) {
  // While expanded the paragraph never scrolls; the "Show less" control stays
  // visible via the open set, so skip measuring (it would read no-overflow).
  if (openThesis.value.has(symbol)) return;
  const overflows = el.scrollHeight - el.clientHeight > 2;
  if (overflows !== thesisOverflow.value.has(symbol)) {
    const next = new Set(thesisOverflow.value);
    if (overflows) next.add(symbol);
    else next.delete(symbol);
    thesisOverflow.value = next;
  }
}
// A per-symbol function ref: registers the paragraph for measurement and
// observation on mount/patch, unregisters on unmount.
function thesisRef(symbol: string) {
  return (el: unknown) => {
    const prev = thesisEls.get(symbol);
    if (el instanceof HTMLElement) {
      if (prev !== el) {
        if (prev) thesisObserver?.unobserve(prev);
        thesisEls.set(symbol, el);
        thesisObserver?.observe(el);
      }
      measureThesis(symbol, el);
    } else if (prev) {
      thesisObserver?.unobserve(prev);
      thesisEls.delete(symbol);
    }
  };
}
function toggleThesis(symbol: string) {
  const next = new Set(openThesis.value);
  if (next.has(symbol)) next.delete(symbol);
  else next.add(symbol);
  openThesis.value = next;
}
onBeforeUnmount(() => thesisObserver?.disconnect());

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
          class="btn btn-secondary"
          :disabled="quickDisabled"
          :title="quickTitle"
          @click="emit('quick-check')"
        >
          {{ quickChecking ? "Checking…" : "Quick check" }}
        </button>
        <button
          type="button"
          class="btn btn-primary"
          :disabled="runDisabled"
          :title="runTitle"
          @click="onRun"
        >
          {{
            running
              ? "Running analysis…"
              : selectionActive
                ? `Analyze ${selected.size} selected`
                : "Run analysis"
          }}
        </button>
      </div>
    </div>

    <!-- Historical-view banner (docs/interface.md §Main Layout): a quiet
         informational band, never the amber action treatment — viewing a past
         run is a chosen state, not a problem. -->
    <div v-if="isHistorical && run" class="hist-banner" role="status">
      <span class="hist-banner-label">Past analysis</span>
      <span class="hist-banner-text">
        Viewing the run from {{ fmtStamp(run.created_at) }} — read-only.
      </span>
      <button
        type="button"
        class="btn btn-secondary hist-banner-back"
        @click="emit('back-to-latest')"
      >
        Back to latest
      </button>
    </div>

    <!-- Inline run-gate block / failure — ephemeral, never a persistent warning. -->
    <div v-if="runError" class="pane-error" role="alert">
      <span class="pane-error-label">Couldn't run</span>
      <span class="pane-error-detail">{{ runError }}</span>
    </div>

    <!-- A past-run open failure — same ephemeral posture, its own label so it
         never reads as a job failure. -->
    <div v-if="historyError" class="pane-error" role="alert">
      <span class="pane-error-label">Couldn't open the run</span>
      <span class="pane-error-detail">{{ historyError }}</span>
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

      <!-- The listing failed with nothing else to show: whether a prior
           analysis exists can't be told from here, so claim neither. -->
      <div v-else-if="!run && !pull && historyUnknown" class="empty-state">
        <h2 class="empty-title">Portfolio state unavailable.</h2>
        <p class="empty-body">
          The runs history couldn't be read, so whether a prior analysis exists
          can't be told from here — the sidebar carries the error. Leaving and
          re-opening this page retries the read.
        </p>
      </div>

      <!-- A run exists in the history but couldn't be decoded: unreadable, not
           never-ran. -->
      <div v-else-if="!run && !pull && unreadableHistory" class="empty-state">
        <h2 class="empty-title">A prior run couldn't be read.</h2>
        <p class="empty-body">
          The runs history holds an analysis this build could not decode, so it
          can't be shown here — it lists in the sidebar tagged
          <em>unreadable</em> and will age out of retention.
          <strong>Run analysis</strong> starts a fresh pass.
        </p>
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
        <!-- Pulled with no latest run: the compact current-holdings view IS the
             page body (docs/portfolio-analysis.md §Storage and display). The
             subline is history-aware — a pull over an unreadable history must
             not claim nothing was ever analyzed (Codex round). -->
        <div v-if="!run && pull" class="pulled-only">
          <h2 class="empty-title">
            {{ pull.holdings.positions.length }}
            {{ pull.holdings.positions.length === 1 ? "holding" : "holdings" }}
            pulled.
            <template v-if="unreadableHistory">A prior run couldn't be read.</template>
            <template v-else-if="historyUnknown">Analysis state unknown.</template>
            <template v-else>Not yet analyzed.</template>
          </h2>
          <p class="empty-body">
            Pulled {{ fmtStamp(pull.pulled_at) }} from your connected Schwab
            account.
            <template v-if="unreadableHistory">
              The runs history holds an analysis this build could not decode —
              it lists in the sidebar tagged <em>unreadable</em>.
              <strong>Run analysis</strong> starts a fresh pass.
            </template>
            <template v-else-if="historyUnknown">
              The runs history couldn't be read, so whether a prior analysis
              exists can't be told from here — the sidebar carries the error.
            </template>
            <template v-else>
              Nothing is graded until you run the analysis.
            </template>
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
                    {{ costBasisOf(p) !== null ? fmtMoney(costBasisOf(p)!) : "—" }}
                  </td>
                  <td class="num">
                    <span
                      v-if="gainPctOf(p) !== null"
                      class="dir"
                      :class="pctDir(gainPctOf(p))"
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
          <!-- Stack controls: the sort bar beside the selection controls (the
               row owns the shared spacing so the two don't each guess). -->
          <div class="stack-controls">
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
            <!-- Selection controls (docs/portfolio-analysis.md §Triggering):
                 select-all / clear beside a live count; the per-card boxes
                 drive the same set. Hidden on a read-only past run. -->
            <div
              v-if="!isHistorical"
              class="hc-selectbar"
              role="group"
              aria-label="Selective re-analysis selection"
            >
              <span class="hc-selectbar-count" aria-live="polite">{{
                selectionActive ? `${selected.size} selected` : ""
              }}</span>
              <button
                type="button"
                class="hc-reveal hc-selectbar-btn"
                :disabled="selectionDisabled || allSelected"
                @click="selectAll"
              >
                Select all
              </button>
              <button
                type="button"
                class="hc-reveal hc-selectbar-btn"
                :disabled="selectionDisabled || !selectionActive"
                @click="clearSelection"
              >
                Clear
              </button>
            </div>
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
                    <label
                      v-if="!isHistorical"
                      class="hc-select"
                      :title="`Select ${v.symbol} for selective re-analysis`"
                    >
                      <input
                        type="checkbox"
                        class="hc-select-input"
                        :checked="isSelected(v.symbol)"
                        :disabled="selectionDisabled"
                        :aria-label="`Select ${v.symbol} for re-analysis`"
                        @change="toggleSelect(v.symbol)"
                      />
                      <span class="hc-select-box" aria-hidden="true"></span>
                    </label>
                    <span class="ana-ticker">{{ v.symbol }}</span>
                    <span class="hc-class">{{ classLabel(v) }}</span>
                    <span
                      v-if="carriedStamp(v)"
                      class="ana-tag"
                      :title="carriedStamp(v)!.title"
                      >{{ carriedStamp(v)!.text }}</span
                    >
                    <span
                      v-if="v.side_reversed"
                      class="ana-tag dh-attention-tag"
                      :title="SIDE_REVERSED_TITLE"
                      >Side reversed</span
                    >
                    <span v-if="noLongerHeld(v.symbol)" class="ana-tag"
                      >No longer held</span
                    >
                    <template v-if="quickFor(v.symbol)">
                      <span
                        v-if="quickFor(v.symbol)!.flag"
                        class="ana-tag dh-attention-tag"
                        :title="flagTitle(quickFor(v.symbol)!)"
                        >Attention — {{ flagLabel(quickFor(v.symbol)!.flag!.trigger) }}</span
                      >
                      <span
                        v-if="eventBadge(quickFor(v.symbol)!)"
                        class="ana-tag"
                        :title="eventBadge(quickFor(v.symbol)!)!.title"
                        >{{ eventBadge(quickFor(v.symbol)!)!.text }}</span
                      >
                      <span
                        v-if="degradedBadge(quickFor(v.symbol)!)"
                        class="ana-tag"
                        :title="degradedBadge(quickFor(v.symbol)!)!.title"
                        >{{ degradedBadge(quickFor(v.symbol)!)!.text }}</span
                      >
                    </template>
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
                        <label
                          v-if="!isHistorical"
                          class="hc-select"
                          :title="`Select ${v.symbol} for selective re-analysis`"
                        >
                          <input
                            type="checkbox"
                            class="hc-select-input"
                            :checked="isSelected(v.symbol)"
                            :disabled="selectionDisabled"
                            :aria-label="`Select ${v.symbol} for re-analysis`"
                            @change="toggleSelect(v.symbol)"
                          />
                          <span class="hc-select-box" aria-hidden="true"></span>
                        </label>
                        <span class="ana-ticker">{{ v.symbol }}</span>
                        <span class="hc-class">{{ classLabel(v) }}</span>
                        <span
                          v-if="carriedStamp(v)"
                          class="ana-tag"
                          :title="carriedStamp(v)!.title"
                          >{{ carriedStamp(v)!.text }}</span
                        >
                        <span
                          v-if="v.side_reversed"
                          class="ana-tag dh-attention-tag"
                          :title="SIDE_REVERSED_TITLE"
                          >Side reversed</span
                        >
                        <span v-if="v.disposition.structural_flag" class="ana-tag"
                          >Structurally path-dependent</span
                        >
                        <!-- The over-age rule-demotion is branch-unscoped
                             (job.rs demotes role-risk adds too) — without the
                             tag a demoted hold reads as the model's standing
                             choice. -->
                        <span v-if="demoted(v)" class="ana-tag" :title="DEMOTED_TITLE"
                          >Add demoted to hold</span
                        >
                        <span v-if="noLongerHeld(v.symbol)" class="ana-tag"
                          >No longer held</span
                        >
                        <template v-if="quickFor(v.symbol)">
                          <span
                            v-if="quickFor(v.symbol)!.flag"
                            class="ana-tag dh-attention-tag"
                            :title="flagTitle(quickFor(v.symbol)!)"
                            >Attention — {{ flagLabel(quickFor(v.symbol)!.flag!.trigger) }}</span
                          >
                          <span
                            v-if="eventBadge(quickFor(v.symbol)!)"
                            class="ana-tag"
                            :title="eventBadge(quickFor(v.symbol)!)!.title"
                            >{{ eventBadge(quickFor(v.symbol)!)!.text }}</span
                          >
                          <span
                            v-if="degradedBadge(quickFor(v.symbol)!)"
                            class="ana-tag"
                            :title="degradedBadge(quickFor(v.symbol)!)!.title"
                            >{{ degradedBadge(quickFor(v.symbol)!)!.text }}</span
                          >
                        </template>
                      </div>
                      <div class="hc-name">
                        {{ positionFor(v.symbol)?.description ?? "" }}
                      </div>
                    </div>
                  </div>
                  <!-- The position block: the account facts the verdict is
                       read against — price, the across-orders average cost,
                       the netted cost basis — so the unrealized figure sits
                       on its base (2026-07-31 run review, F9). -->
                  <dl class="hc-kv hc-position">
                    <dt class="hc-kicker">Price</dt>
                    <dd>
                      <span class="ana-num">{{
                        priceOf(positionFor(v.symbol)) !== null
                          ? fmtMoney(priceOf(positionFor(v.symbol))!)
                          : "—"
                      }}</span>
                    </dd>
                    <dt class="hc-kicker">Avg cost</dt>
                    <dd>
                      <span class="ana-num">{{
                        avgCostOf(positionFor(v.symbol)) !== null
                          ? fmtMoney(avgCostOf(positionFor(v.symbol))!)
                          : "—"
                      }}</span>
                    </dd>
                    <dt class="hc-kicker">Cost basis</dt>
                    <dd>
                      <span class="ana-num">{{
                        costBasisOf(positionFor(v.symbol)) !== null
                          ? fmtMoney(costBasisOf(positionFor(v.symbol))!)
                          : "—"
                      }}</span>
                    </dd>
                    <dt class="hc-kicker">Unrealized</dt>
                    <dd>
                      <span
                        v-if="gainOf(positionFor(v.symbol)) !== null"
                        class="dir hc-gain"
                        :class="moneyDir(gainOf(positionFor(v.symbol)))"
                      >
                        {{ fmtSigned(gainOf(positionFor(v.symbol))!) }}
                        <!-- A negative netted basis has a defined dollar gain but
                             no honest percentage — the parenthetical drops. -->
                        <template v-if="gainPctOf(positionFor(v.symbol)) !== null"
                          >({{ fmtPct(gainPctOf(positionFor(v.symbol))!) }})</template
                        >
                      </span>
                      <span v-else class="ana-num hc-gain-none">—</span>
                    </dd>
                  </dl>
                </header>

                <!-- The card's anchor: the ledger's standing thesis, rendered
                     straight from the continuity-validated ledger — never a
                     separately authored summary
                     (docs/portfolio-analysis.md §Storage and display). Long
                     theses follow the kit's ThesisAnchor overflow contract:
                     a three-line clamp with a reveal shown only on overflow. -->
                <div v-if="v.thesis_ledger" class="hc-thesis">
                  <span class="hc-kicker">Standing thesis</span>
                  <p
                    :ref="thesisRef(v.symbol)"
                    class="hc-thesis-text"
                    :class="{ clamped: !openThesis.has(v.symbol) }"
                  >
                    {{ v.thesis_ledger.current_thesis }}
                  </p>
                  <button
                    v-if="thesisOverflow.has(v.symbol) || openThesis.has(v.symbol)"
                    type="button"
                    class="hc-thesis-toggle"
                    :aria-expanded="openThesis.has(v.symbol)"
                    @click="toggleThesis(v.symbol)"
                  >
                    {{ openThesis.has(v.symbol) ? "Show less" : "Read full thesis" }}
                  </button>
                </div>

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
                    </div>
                    <p
                      v-if="v.disposition.action_rationale"
                      class="hc-prose hc-rationale"
                    >
                      {{ v.disposition.action_rationale }}
                    </p>
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

                <!-- Thesis monitor (B13), condition-only on this branch: the
                     role-risk ledger's scenarios carry no engine target
                     (structurally null — docs/portfolio-analysis.md §The
                     position thesis ledger), so the target line drops. -->
                <div
                  v-if="v.thesis_ledger && v.thesis_ledger.monitor.length > 0"
                  class="hc-monitor"
                >
                  <div class="hc-monitor-grid">
                    <div
                      v-for="s in v.thesis_ledger.monitor"
                      :key="s.scenario"
                      class="hc-scenario"
                    >
                      <div class="hc-scenario-head">
                        <span class="hc-kicker">{{ s.scenario }}</span>
                        <span class="ana-num hc-scenario-prob"
                          >{{ Math.round(s.probability_pct) }}%</span
                        >
                      </div>
                      <span
                        v-if="s.engine_target !== null"
                        class="ana-num hc-scenario-target"
                        >{{ moneyExact.format(s.engine_target) }}</span
                      >
                      <p class="hc-scenario-note">{{ s.conditions }}</p>
                    </div>
                  </div>
                  <dl
                    v-if="
                      v.thesis_ledger.what_must_improve ||
                      v.thesis_ledger.what_must_not_break
                    "
                    class="hc-goalposts"
                  >
                    <template v-if="v.thesis_ledger.what_must_improve">
                      <dt class="hc-kicker">Must improve</dt>
                      <dd class="hc-goalpost-text">
                        {{ v.thesis_ledger.what_must_improve }}
                      </dd>
                    </template>
                    <template v-if="v.thesis_ledger.what_must_not_break">
                      <dt class="hc-kicker">Must not break</dt>
                      <dd class="hc-goalpost-text">
                        {{ v.thesis_ledger.what_must_not_break }}
                      </dd>
                    </template>
                  </dl>
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
                        <label
                          v-if="!isHistorical"
                          class="hc-select"
                          :title="`Select ${v.symbol} for selective re-analysis`"
                        >
                          <input
                            type="checkbox"
                            class="hc-select-input"
                            :checked="isSelected(v.symbol)"
                            :disabled="selectionDisabled"
                            :aria-label="`Select ${v.symbol} for re-analysis`"
                            @change="toggleSelect(v.symbol)"
                          />
                          <span class="hc-select-box" aria-hidden="true"></span>
                        </label>
                        <span class="ana-ticker">{{ v.symbol }}</span>
                        <span class="hc-class">{{ classLabel(v) }}</span>
                        <span
                          v-if="carriedStamp(v)"
                          class="ana-tag"
                          :title="carriedStamp(v)!.title"
                          >{{ carriedStamp(v)!.text }}</span
                        >
                        <span
                          v-if="v.side_reversed"
                          class="ana-tag dh-attention-tag"
                          :title="SIDE_REVERSED_TITLE"
                          >Side reversed</span
                        >
                        <span v-if="demoted(v)" class="ana-tag" :title="DEMOTED_TITLE"
                          >Add demoted to hold</span
                        >
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
                        <template v-if="quickFor(v.symbol)">
                          <span
                            v-if="quickFor(v.symbol)!.flag"
                            class="ana-tag dh-attention-tag"
                            :title="flagTitle(quickFor(v.symbol)!)"
                            >Attention — {{ flagLabel(quickFor(v.symbol)!.flag!.trigger) }}</span
                          >
                          <span
                            v-if="eventBadge(quickFor(v.symbol)!)"
                            class="ana-tag"
                            :title="eventBadge(quickFor(v.symbol)!)!.title"
                            >{{ eventBadge(quickFor(v.symbol)!)!.text }}</span
                          >
                          <span
                            v-if="degradedBadge(quickFor(v.symbol)!)"
                            class="ana-tag"
                            :title="degradedBadge(quickFor(v.symbol)!)!.title"
                            >{{ degradedBadge(quickFor(v.symbol)!)!.text }}</span
                          >
                        </template>
                      </div>
                      <div class="hc-name">
                        {{ positionFor(v.symbol)?.description ?? "" }}
                      </div>
                    </div>
                  </div>
                  <!-- The position block: the account facts the verdict is
                       read against — price, the across-orders average cost,
                       the netted cost basis — so the unrealized figure sits
                       on its base (2026-07-31 run review, F9). -->
                  <dl class="hc-kv hc-position">
                    <dt class="hc-kicker">Price</dt>
                    <dd>
                      <span class="ana-num">{{
                        priceOf(positionFor(v.symbol)) !== null
                          ? fmtMoney(priceOf(positionFor(v.symbol))!)
                          : "—"
                      }}</span>
                    </dd>
                    <dt class="hc-kicker">Avg cost</dt>
                    <dd>
                      <span class="ana-num">{{
                        avgCostOf(positionFor(v.symbol)) !== null
                          ? fmtMoney(avgCostOf(positionFor(v.symbol))!)
                          : "—"
                      }}</span>
                    </dd>
                    <dt class="hc-kicker">Cost basis</dt>
                    <dd>
                      <span class="ana-num">{{
                        costBasisOf(positionFor(v.symbol)) !== null
                          ? fmtMoney(costBasisOf(positionFor(v.symbol))!)
                          : "—"
                      }}</span>
                    </dd>
                    <dt class="hc-kicker">Unrealized</dt>
                    <dd>
                      <span
                        v-if="gainOf(positionFor(v.symbol)) !== null"
                        class="dir hc-gain"
                        :class="moneyDir(gainOf(positionFor(v.symbol)))"
                      >
                        {{ fmtSigned(gainOf(positionFor(v.symbol))!) }}
                        <!-- A negative netted basis has a defined dollar gain but
                             no honest percentage — the parenthetical drops. -->
                        <template v-if="gainPctOf(positionFor(v.symbol)) !== null"
                          >({{ fmtPct(gainPctOf(positionFor(v.symbol))!) }})</template
                        >
                      </span>
                      <span v-else class="ana-num hc-gain-none">—</span>
                    </dd>
                  </dl>
                </header>

                <!-- The card's anchor: the ledger's standing thesis, rendered
                     straight from the continuity-validated ledger — never a
                     separately authored summary
                     (docs/portfolio-analysis.md §Storage and display). Long
                     theses follow the kit's ThesisAnchor overflow contract:
                     a three-line clamp with a reveal shown only on overflow. -->
                <div v-if="v.thesis_ledger" class="hc-thesis">
                  <span class="hc-kicker">Standing thesis</span>
                  <p
                    :ref="thesisRef(v.symbol)"
                    class="hc-thesis-text"
                    :class="{ clamped: !openThesis.has(v.symbol) }"
                  >
                    {{ v.thesis_ledger.current_thesis }}
                  </p>
                  <button
                    v-if="thesisOverflow.has(v.symbol) || openThesis.has(v.symbol)"
                    type="button"
                    class="hc-thesis-toggle"
                    :aria-expanded="openThesis.has(v.symbol)"
                    @click="toggleThesis(v.symbol)"
                  >
                    {{ openThesis.has(v.symbol) ? "Show less" : "Read full thesis" }}
                  </button>
                </div>

                <!-- The two-arm body (portfolio-v7): the engine baseline beside
                     the model's own view — the same paired 1fr/1fr hairline grid
                     as the old intrinsic/action split (a recorded design-system
                     extension: no paired-comparison component exists in the kit;
                     comparison here is adjacency + kicker, the system's idiom). -->
                <div class="hc-body">
                  <div class="hc-col hc-col-intrinsic">
                    <span class="hc-kicker">Engine baseline</span>
                    <!-- Letter inputs, then — set apart behind a hairline —
                         the market-setup read (momentum), which is context
                         for conviction and never a grade input (B10). -->
                    <div class="hc-subscores">
                      <div
                        v-for="name in LETTER_SUBSCORES"
                        :key="name"
                        class="hc-sub"
                      >
                        <span class="hc-sub-label">{{ name }}</span>
                        <span class="ana-num hc-sub-value">{{
                          Math.round(v.disposition.sub_scores[name])
                        }}</span>
                      </div>
                      <div class="hc-sub hc-sub-setup">
                        <span class="hc-sub-label">Setup</span>
                        <span class="ana-num hc-sub-value">{{
                          Math.round(v.disposition.sub_scores.momentum)
                        }}</span>
                      </div>
                    </div>
                    <p class="hc-setup-note">{{ SETUP_NOTE }}</p>
                    <dl class="hc-kv">
                      <dt>Conviction</dt>
                      <dd>
                        <span
                          class="conviction"
                          role="img"
                          :aria-label="`Conviction: ${armAConviction(v.disposition)}`"
                        >
                          <i
                            v-for="i in 3"
                            :key="i"
                            :class="{
                              on: i <= CONVICTION_LEVEL[armAConviction(v.disposition)],
                            }"
                          />
                        </span>
                        <span class="hc-conviction-word">{{
                          armAConviction(v.disposition)
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
                          v-for="(read, horizon) in armAOutlook(v.disposition)"
                          :key="horizon"
                          class="hc-horizon"
                        >
                          <span class="hc-horizon-label">{{ horizon }}</span>
                          <span class="dir" :class="HORIZON_DIR[read]">{{
                            read
                          }}</span>
                        </span>
                      </dd>
                      <dt>Action</dt>
                      <dd>
                        {{ ACTION_LABELS[v.disposition.engine_view!.action] }}
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

                  <!-- The model arm: the model's own numbers, authored
                       unrestricted and persisted exactly as returned — scored
                       against the engine baseline by the outcome scoreboard. -->
                  <div class="hc-col">
                    <span class="hc-kicker hc-armhead"
                      >Model view
                      <span
                        class="grade hc-model-letter"
                        :class="gradeClass(v.disposition.model_view!.letter)"
                        :title="MODEL_LETTER_TITLE"
                        >{{ v.disposition.model_view!.letter }}</span
                      ></span
                    >
                    <div class="hc-subscores">
                      <div
                        v-for="name in LETTER_SUBSCORES"
                        :key="name"
                        class="hc-sub"
                      >
                        <span class="hc-sub-label">{{ name }}</span>
                        <span class="ana-num hc-sub-value">{{
                          Math.round(v.disposition.model_view!.sub_scores[name])
                        }}</span>
                      </div>
                      <div class="hc-sub hc-sub-setup">
                        <span class="hc-sub-label">Setup</span>
                        <span class="ana-num hc-sub-value">{{
                          Math.round(v.disposition.model_view!.sub_scores.momentum)
                        }}</span>
                      </div>
                    </div>
                    <p class="hc-setup-note">{{ MODEL_ARM_NOTE }}</p>
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
                        <span
                          v-if="
                            v.disposition.conviction !==
                            v.disposition.engine_view!.conviction
                          "
                          class="ana-tag"
                          >≠ engine</span
                        >
                      </dd>
                      <template
                        v-for="(band, window) in {
                          '1-mo target':
                            v.disposition.model_view!.price_targets.one_month,
                          '12-mo target':
                            v.disposition.model_view!.price_targets.twelve_month,
                        }"
                        :key="window"
                      >
                        <dt>{{ window }}</dt>
                        <dd>
                          <span class="ana-num"
                            >{{ moneyExact.format(band.base) }}
                            <span class="hc-band"
                              >({{ moneyExact.format(band.bear) }}–{{
                                moneyExact.format(band.bull)
                              }})</span
                            ></span
                          >
                          <span
                            v-if="band.bear > band.bull"
                            class="ana-tag"
                            title="The model authored bear above bull; the value renders as returned — scoring reads the band as (min, max)."
                            >band inverted as authored</span
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
                        <span
                          v-if="outlookDiverges(v.disposition)"
                          class="ana-tag"
                          >≠ engine</span
                        >
                      </dd>
                      <dt>Action</dt>
                      <dd>
                        {{ ACTION_LABELS[v.disposition.action] }}
                        <span
                          v-if="
                            v.disposition.action !==
                            v.disposition.engine_view!.action
                          "
                          class="ana-tag"
                          >≠ engine</span
                        >
                      </dd>
                    </dl>
                  </div>
                </div>

                <!-- Portfolio action: the per-holding action call's decision —
                     full-width beneath the arms (the action is the model's
                     since the v7 contract; the engine's own rung reads as the
                     baseline row). -->
                <div class="hc-col hc-actionrow">
                    <span class="hc-kicker">Portfolio action</span>
                    <div class="hc-action">
                      <span class="hc-action-word">{{
                        ACTION_LABELS[v.disposition.action]
                      }}</span>
                    </div>
                    <p
                      v-if="v.disposition.action_rationale"
                      class="hc-prose hc-rationale"
                    >
                      {{ v.disposition.action_rationale }}
                    </p>
                    <dl class="hc-kv">
                      <dt>Weight</dt>
                      <dd>
                        <span class="ana-num">{{
                          weightOf(positionFor(v.symbol)) !== null
                            ? fmtPct(weightOf(positionFor(v.symbol))!)
                            : "—"
                        }}</span>
                      </dd>
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

                <!-- Thesis monitor (B13): the ledger's bear/base/bull scenarios
                     with the app-stamped engine targets, plus the monitor-level
                     goalposts — rendered straight from the continuity-validated
                     ledger (docs/portfolio-analysis.md §The position thesis
                     ledger). Kit fidelity: Portfolio.jsx Scenarios; the goalpost
                     lines extend the kit (recorded deviation, B13 ruling
                     2026-08-05). Renders whatever scenarios exist. -->
                <div
                  v-if="v.thesis_ledger && v.thesis_ledger.monitor.length > 0"
                  class="hc-monitor"
                >
                  <div class="hc-monitor-grid">
                    <div
                      v-for="s in v.thesis_ledger.monitor"
                      :key="s.scenario"
                      class="hc-scenario"
                    >
                      <div class="hc-scenario-head">
                        <span class="hc-kicker">{{ s.scenario }}</span>
                        <span class="ana-num hc-scenario-prob"
                          >{{ Math.round(s.probability_pct) }}%</span
                        >
                      </div>
                      <span
                        v-if="s.engine_target !== null"
                        class="ana-num hc-scenario-target"
                        >{{ moneyExact.format(s.engine_target) }}</span
                      >
                      <p class="hc-scenario-note">{{ s.conditions }}</p>
                    </div>
                  </div>
                  <dl
                    v-if="
                      v.thesis_ledger.what_must_improve ||
                      v.thesis_ledger.what_must_not_break
                    "
                    class="hc-goalposts"
                  >
                    <template v-if="v.thesis_ledger.what_must_improve">
                      <dt class="hc-kicker">Must improve</dt>
                      <dd class="hc-goalpost-text">
                        {{ v.thesis_ledger.what_must_improve }}
                      </dd>
                    </template>
                    <template v-if="v.thesis_ledger.what_must_not_break">
                      <dt class="hc-kicker">Must not break</dt>
                      <dd class="hc-goalpost-text">
                        {{ v.thesis_ledger.what_must_not_break }}
                      </dd>
                    </template>
                  </dl>
                </div>

                <!-- Financial analysis — model prose over engine numbers. -->
                <div v-if="v.disposition.financial_summary" class="hc-summary">
                  <span class="hc-kicker">Financial analysis</span>
                  <p class="hc-prose">{{ v.disposition.financial_summary }}</p>
                </div>

                <!-- The model's retrospective self-assessment (v7): prose input
                     to the learnings; the scored comparison is the deterministic
                     scoreboard's, never this paragraph's. -->
                <div
                  v-if="v.disposition.model_view?.self_assessment"
                  class="hc-summary"
                >
                  <span class="hc-kicker">Model retrospective</span>
                  <p class="hc-prose">
                    {{ v.disposition.model_view!.self_assessment }}
                  </p>
                </div>

                <!-- What changed + the app-computed position delta. -->
                <footer class="hc-foot">
                  <div class="hc-foot-main">
                    <span class="hc-kicker">What changed · since last run</span>
                    <p class="hc-changed">{{ v.disposition.what_changed }}</p>
                    <p
                      v-if="maturedLinesFor(v.symbol).length"
                      class="hc-changed hc-scoreboard-line"
                    >
                      Scored: {{ maturedLinesFor(v.symbol).join("; ") }}
                    </p>
                  </div>
                  <span class="ana-tag" :title="'Position vs. prior run'"
                    >Position: {{ CHANGE_LABELS[v.position_change] }}</span
                  >
                </footer>
              </template>
            </article>
            <!-- Not-analyzed holdings: present in the book but ungraded this run
                 (new / unselected — docs/portfolio-analysis.md §Triggering).
                 Selectable so the next selective run can grade them. -->
            <article
              v-for="p in notAnalyzed"
              :key="`na-${p.symbol}`"
              class="ana-card holding-card"
            >
              <div class="hc-reduced">
                <div class="hc-reduced-main">
                  <div class="hc-idline">
                    <label
                      v-if="!isHistorical"
                      class="hc-select"
                      :title="`Select ${p.symbol} to analyze`"
                    >
                      <input
                        type="checkbox"
                        class="hc-select-input"
                        :checked="isSelected(p.symbol)"
                        :disabled="selectionDisabled"
                        :aria-label="`Select ${p.symbol} to analyze`"
                        @change="toggleSelect(p.symbol)"
                      />
                      <span class="hc-select-box" aria-hidden="true"></span>
                    </label>
                    <span class="ana-ticker">{{ p.symbol }}</span>
                    <span class="hc-class">Not analyzed</span>
                  </div>
                  <p class="hc-reason">
                    Not analyzed in this run — select it and re-run, or run a full
                    analysis, to grade it.
                  </p>
                </div>
                <div class="hc-reduced-side">
                  <span class="hc-kicker">Weight</span>
                  <span class="ana-num hc-weight">{{
                    weightOf(p) !== null ? fmtPct(weightOf(p)!) : "—"
                  }}</span>
                </div>
              </div>
            </article>
          </div>

          <!-- Whole-book roll-up (+ the exited positions from the holdings diff). -->
          <section class="ana-card rollup" aria-label="Portfolio roll-up">
            <header class="rollup-head">
              <span class="ana-head">Roll-up · whole book</span>
              <span class="ch-stamp">Analyzed {{ fmtStamp(run.created_at) }}</span>
            </header>
            <p class="rollup-overview hc-prose">{{ run.roll_up.overview }}</p>
            <!-- Run-level data health: how the target surface was actually sourced.
                 Absent on runs persisted before the field existed. The attention tag
                 reuses the sanctioned grade-D "amber" pair (design system §Grade
                 scale — Portfolio's attention flag). -->
            <div v-if="run.roll_up.data_health" class="rollup-datahealth">
              <span class="hc-kicker">Data health</span>
              <p class="dh-line hc-prose">
                <span
                  v-if="run.roll_up.data_health.attention"
                  class="ana-tag dh-attention-tag"
                  >Attention</span
                >
                {{ run.roll_up.data_health.summary }}
              </p>
            </div>
            <!-- The model-vs-engine scoreboard (v7): deterministic, engine-scored
                 reads over matured windows — empty until v7 episodes mature. -->
            <div v-if="scoreboardLines.length" class="rollup-scoreboard">
              <span class="hc-kicker">Model vs engine · scored</span>
              <ul class="rollup-annotation-list">
                <li v-for="(line, i) in scoreboardLines" :key="i">{{ line }}</li>
              </ul>
            </div>
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

/* Historical-view banner: the pane-error band's geometry with informational ink
   (tracked-caps label in ink-3, body in ink-2 — never the accent/amber action
   treatment; a chosen state, not a problem). The back control rides the row's
   right edge. */
.hist-banner {
  display: flex;
  align-items: center;
  gap: var(--s-3);
  padding: var(--s-3) var(--s-6);
  background: var(--paper-edge);
  border-bottom: var(--border);
  flex-shrink: 0;
}

.hist-banner-label {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink-3);
}

/* The banner's degraded-run tag holds its size beside the flexing text. */
.hist-banner .ana-tag {
  flex-shrink: 0;
}

.hist-banner-text {
  flex: 1;
  min-width: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
  font-variant-numeric: tabular-nums lining-nums;
  overflow-wrap: anywhere;
}

.hist-banner-back {
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
.stack-controls,
.card-stack,
.rollup {
  max-width: 980px;
  margin-left: auto;
  margin-right: auto;
}

.strip {
  margin-bottom: var(--s-6);
}

/* Key-figure strip wraps on narrow windows rather than crushing the figures —
   a wrap-safe extension of the kit's single-row .keyfig (grid-auto-flow:
   column; the package defines no wrapping variant). The hairline lattice is
   the 1px flex gap over the container's hairline background, cells repainting
   paper, so every seam survives wrapping — the per-cell border idiom left
   row-one cells above a partial row with no bottom rule and boxed the
   orphaned cell. Flex, not grid: a partial final row stretches to fill,
   leaving no empty track to expose the lattice background. */
.strip.keyfig {
  display: flex;
  flex-wrap: wrap;
  gap: 1px;
  background: var(--hairline-soft);
  overflow: hidden;
}

.strip.keyfig > .kf {
  flex: 1 1 110px;
  border-left: 0;
  background: var(--paper);
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

/* ---- Stack controls: the sort bar + the selection controls on one row (the
   row owns the inter-control spacing and the stack gap). ---- */
.stack-controls {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--s-3) var(--s-5);
  margin-bottom: var(--s-4);
}

/* Sort bar (design package .ana-sortbar; spacing owned by .stack-controls). */

/* Selection controls (docs/portfolio-analysis.md §Triggering). The count and
   the two link-buttons share the register's tracked-caps voice; the buttons
   reuse the kit Reveal posture. A disabled reveal has no package treatment —
   extended here as the sort bar's dimmed-inactive opacity (noted extension). */
.hc-selectbar {
  display: inline-flex;
  align-items: center;
  gap: var(--s-4);
  margin-left: auto;
}

.hc-selectbar-count {
  font-family: var(--font-sans);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--ink-3);
  font-variant-numeric: tabular-nums;
}

.hc-selectbar-btn {
  margin-top: 0;
}

.hc-selectbar-btn:disabled {
  opacity: 0.45;
  cursor: default;
}

.hc-selectbar-btn:disabled:hover {
  color: var(--ink-3);
}

/* Per-card selection box (docs/portfolio-analysis.md §Triggering). The
   register defines no selection control — extended minimally and noted: a
   hairline 14px box at radius 2px inside a 24px hit target, checked = accent
   fill with a paper check (accent marks the actionable selection), the shared
   accent focus-visible ring. The box border uses --ink-3 (AA-cleared), not the
   sub-3:1 hairline, so the control boundary meets the UI-contrast floor. */
.hc-select {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  margin: -5px 0;
  align-self: center;
  cursor: pointer;
  flex-shrink: 0;
}

.hc-select-input {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  opacity: 0;
  margin: 0;
  cursor: inherit;
}

.hc-select:has(.hc-select-input:disabled) {
  cursor: default;
}

.hc-select-box {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  border: 1px solid var(--ink-3);
  border-radius: 2px;
  background: transparent;
  transition: background 120ms var(--ease), border-color 120ms var(--ease);
}

.hc-select-input:checked + .hc-select-box {
  background: var(--accent);
  border-color: var(--accent);
}

.hc-select-input:checked + .hc-select-box::after {
  content: "";
  width: 8px;
  height: 4px;
  border-left: 2px solid var(--paper);
  border-bottom: 2px solid var(--paper);
  transform: rotate(-45deg) translateY(-1px);
}

.hc-select-input:focus-visible + .hc-select-box {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.hc-select-input:disabled + .hc-select-box {
  opacity: 0.4;
}

@media (prefers-reduced-motion: reduce) {
  .hc-select-box {
    transition: none;
  }
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

/* The header's position block rides the hc-kv primitive, right-aligned:
   both columns hug, labels reuse the kicker treatment via the dt class. */
.hc-position {
  grid-template-columns: max-content max-content;
  justify-content: end;
  align-items: baseline;
  row-gap: var(--s-1);
  text-align: right;
  flex-shrink: 0;
}

.hc-gain {
  font-size: 15px;
}

.hc-gain-none {
  color: var(--ink-3);
}

/* Two linked columns; stack on narrow windows so nothing crushes. Since v7 the
   pair is engine baseline | model view (a recorded extension of the kit's
   two-linked-blocks grid — comparison by adjacency + kicker, the system's
   idiom). */
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

/* The full-width portfolio-action strip beneath the arms (v7) — the same
   self-seaming hairline rhythm as the monitor/summary sections. */
.hc-actionrow {
  border-top: 1px solid var(--hairline-soft);
}

/* The model-view column head: kicker + the model's derived letter, rendered as
   a compact grade chip beside the label (never competing with the header's
   engine chip). */
.hc-armhead {
  display: inline-flex;
  align-items: center;
  gap: var(--s-2);
}

.hc-model-letter {
  font-size: var(--t-caption);
  padding: 0 var(--s-2);
}

.hc-col > .hc-kicker {
  margin-bottom: var(--s-3);
}

.hc-subscores {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0 var(--s-2);
  margin-bottom: var(--s-2);
}

/* The set-apart market-setup tile (B10): a hairline seam divides it from the
   three letter inputs so it never reads as a fourth grade input. The seam is
   supplementary — the always-visible .hc-setup-note caption carries the
   meaning for every modality (hairlines in this system sit far below the 3:1
   boundary floor by design). */
.hc-sub-setup {
  border-left: 1px solid var(--hairline-soft);
  padding-left: var(--s-3);
}

/* The caption rides the card's quiet note register (the kit's health-note
   treatment); the tile row above keeps only a tight gap to it. */
.hc-setup-note {
  font-family: var(--font-serif);
  font-size: 12px;
  line-height: 1.4;
  color: var(--ink-3);
  margin: 0 0 var(--s-4);
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

.hc-summary {
  padding: var(--s-4) var(--s-5);
  border-top: 1px solid var(--hairline-soft);
}

.hc-summary .hc-kicker {
  margin-bottom: var(--s-2);
}

/* Thesis monitor (B13) — the kit's Scenarios strip (Portfolio.jsx): a
   hairline-topped three-cell row, each scenario a kicker + probability over
   the engine target and its defining conditions; the cells keep the kit's
   tighter padding register. The goalpost lines below extend the kit
   (recorded deviation). */
.hc-monitor {
  border-top: 1px solid var(--hairline-soft);
}

.hc-monitor-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.hc-scenario {
  min-width: 0;
  padding: var(--s-3) var(--s-4);
}

.hc-scenario + .hc-scenario {
  border-left: 1px solid var(--hairline-soft);
}

.hc-scenario-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: var(--s-3);
  margin-bottom: var(--s-2);
}

.hc-scenario-prob {
  font-size: 11px;
  color: var(--ink-3);
}

.hc-scenario-target {
  display: block;
  font-size: 14px;
  color: var(--ink);
}

.hc-scenario-note {
  font-family: var(--font-serif);
  font-size: 12px;
  line-height: 1.4;
  color: var(--ink-3);
  margin: var(--s-1) 0 0;
  overflow-wrap: anywhere;
}

.hc-goalposts {
  display: grid;
  grid-template-columns: max-content 1fr;
  column-gap: var(--s-4);
  row-gap: var(--s-2);
  margin: 0;
  padding: var(--s-3) var(--s-4);
  border-top: 1px solid var(--hairline-soft);
}

.hc-goalpost-text {
  font-family: var(--font-serif);
  font-size: 12px;
  line-height: 1.45;
  color: var(--ink-2);
  margin: 0;
  overflow-wrap: anywhere;
}

/* The monitor strip stacks with the body: cell seams rotate from vertical
   hairlines to horizontal ones. Kept AFTER the base rules above — a media
   query adds no cascade priority, so these overrides must win by source
   order (the hazard the .hc-col-intrinsic !important works around). */
@media (max-width: 760px) {
  .hc-monitor-grid {
    grid-template-columns: 1fr;
  }

  .hc-scenario + .hc-scenario {
    border-left: 0;
    border-top: 1px solid var(--hairline-soft);
  }
}

/* The card's standing-thesis anchor (the thesis ledger's current thesis),
   per the kit's ThesisAnchor + card seams (ui_kits Portfolio.jsx): its own
   section between the header and the verdict body, closed by a bottom
   hairline — the header already draws the one above it. */
.hc-thesis {
  padding: var(--s-4) var(--s-5);
  border-bottom: 1px solid var(--hairline-soft);
}

.hc-thesis .hc-kicker {
  margin-bottom: var(--s-2);
}

/* The kit's thesis lead: serif at 15px in full ink (a register up from the
   13px secondary prose elsewhere on the card). */
.hc-thesis-text {
  font-family: var(--font-serif);
  font-size: 15px;
  line-height: 1.5;
  letter-spacing: -0.006em;
  color: var(--ink);
  margin: 0;
  max-width: 78ch;
  overflow-wrap: anywhere;
}

.hc-thesis-text.clamped {
  display: -webkit-box;
  -webkit-line-clamp: 3;
  line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* The reveal, per the kit's text-only toggle — accent TEXT rides the
   AA-safe --accent-text token (--accent is fills/rings only); hover and
   focus-visible follow the .hc-reveal conventions. */
.hc-thesis-toggle {
  appearance: none;
  background: transparent;
  border: 0;
  padding: 2px 0;
  margin-top: var(--s-2);
  cursor: pointer;
  font-family: var(--font-sans);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--accent-text);
}

.hc-thesis-toggle:hover {
  color: var(--ink);
}

.hc-thesis-toggle:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
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

/* The action call's rationale under the action line. */
.hc-rationale {
  margin: var(--s-2) 0 0;
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

.rollup-datahealth {
  padding: var(--s-4) var(--s-5);
  border-top: 1px solid var(--hairline-soft);
}

.rollup-datahealth .hc-kicker {
  margin-bottom: var(--s-2);
}

/* The model-vs-engine scoreboard (v7): a quiet list register in the roll-up's
   section rhythm — recorded findings, never alarm states. */
.rollup-scoreboard {
  padding: var(--s-4) var(--s-5);
  border-top: 1px solid var(--hairline-soft);
}

.rollup-scoreboard .hc-kicker {
  margin-bottom: var(--s-2);
}

.rollup-annotation-list {
  margin: 0;
  padding-left: var(--s-5);
  color: var(--ink-2);
  font-size: var(--t-caption);
}

.rollup-annotation-list li {
  margin: 0 0 var(--s-1);
  overflow-wrap: anywhere;
}

.hc-scoreboard-line {
  color: var(--ink-3);
}

.dh-line {
  margin: 0;
}

/* The sanctioned "amber" attention state — the grade-D pair reused
   (colors_and_type.css §Grade scale), never a literal amber. */
.dh-attention-tag {
  color: var(--grade-d-tx);
  background: var(--grade-d-bg);
  border-color: var(--grade-d-bg);
  margin-right: var(--s-2);
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
