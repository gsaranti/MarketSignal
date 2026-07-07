# Current session handoff

## What happened

A user-driven Portfolio-page polish session, two commits straight to `main`. (1) **`d488416`** — the two toolbar triggers were missing the `.btn` base class (`class="btn-primary"` alone), which let WebKit's native outset border show as a gold bevel on Run analysis and silently disabled the package's `:disabled`/`:focus-visible` treatments; both now compose `btn btn-<variant>` like every other surface. (2) **`1e04cb8`** — the current-holdings (pull) table grew **sortable column heads** (all six; symbol opens ascending, numerics descending, re-click flips, "—" values sort last both ways, default = as-pulled order, last-used column in `localStorage` under its own key) plus **Price** and **% gain** columns (the `.dir` directional token) and dropped Description. Three design-package changes ride in `colors_and_type.css`, each noted in-file: a keyboard-operable **inner `<button>` that owns the whole head cell** (th padding moves onto it; the ▾/▴ glyph rides the button's `::after` — fixing a glyph click dead-zone the user caught), the **active head carries the sort bar's active-key treatment** (full ink + paper-soft; glyph-only opacity was illegibly faint), and `aria-sort` on the th per the docs' reservation. `docs/portfolio-analysis.md` §current-holdings amended (incl. the flag that an option's stored price is per-contract, `market_value/quantity`). Spec fixture fix: the pull-side OPT row now sets `market_value: 800`, the value its `account_total` arithmetic always assumed. Full gate green throughout (cargo 584+ / clippy / npm build / npm test 40+136 — five-plus new table specs).

## Current state

On `main` @ `1e04cb8`, pushed, clean tree, nothing mid-implementation. **Eight slices now accumulated on `main` awaiting a build** — #48/#49/#50/#51, `0645351` (footer/RunKind), #52 (Portfolio page), `d488416` (button base class), `1e04cb8` (holdings table) — installed app still v1.2.1. One unverified detail: the glyph hit-target fix is CSS-only (`:has()`-switched, invisible to happy-dom specs) — confirm with one click on a head glyph next time the dev app is open. Carried #52 deferrals unchanged: sidebar Portfolio-runs history, portfolio-specific tracker layout, unpersisted card fields, and the local-models warning still lacking its in-app clear path (Local-analysis-models Settings section unbuilt).

## Open questions

- **Chain both-maps invariant unconfirmed (carried)** — `/chains` still unexercised live; tighten the drift guard to either-absent once a live response confirms both maps present.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s or truncate.
- **local-model M5 pre-flight (carried)** — 122B load/backend, Ollama #14645, `format` constraint, `num_ctx`, throughput.
- **M5-calibration (carried)** — Stooq refresh, `continuity_weight` bands, Research-stale threshold, tripwires, DTO budget, leftover-budget ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Same two clean options, now with eight slices banked: (1) **ship the accumulated `main` slices** — 5-anchor version bump + `npm run tauri build` (launch-time local-suite warnings are the proactive render working, not a regression; click a table-head glyph to confirm the CSS hit-target fix live); or (2) next slice via `/metis-plan-task` — the **Local analysis models Settings section** (still the most user-visible gap), the **sidebar Portfolio-runs history**, or **full Portfolio (funds)**.
