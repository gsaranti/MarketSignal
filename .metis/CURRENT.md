# Current session handoff

## What happened

Specced **and shipped** (commits `4bca81a` docs+metis, `815ed70` design-system;
pushed to `main`) **sorting + views** for the two local-suite surfaces —
**display-only**, reordering already-computed/persisted fields with **no
engine/schema/workflow change**: a **Portfolio holdings sort bar** (in-place card
reorder by value / $ gain / % gain / cash invested, off **Schwab-reported
market-value/cost-basis** — handles option multiplier/bonds/cash, not
`quote×shares`; null/zero cost basis sorts last) and a **TO Matrix/List view
toggle** (the 3×3 matrix stays **canonical**; List flattens all nine cells into
one sortable `.ana-grid` ranking, default **forward-%-upside-desc** — anti-momentum;
since-flagged return the alternate — preserving the load-bearing matrix). Extended
the design-system analytical register with two additive controls
(`.ana-sortbar`/`.ana-viewtoggle` + `SortBar`/`ViewToggle` kit), authored in
Claude Design.

**Load-bearing lesson (don't retry):** the Claude Design export **regenerated the
whole bundle and silently dropped** the report reading-register CSS (every
`.chart-*`, most `.prose`), two tokens (`--accent-text`, `--print-page-margin`),
and **de-guarded** button `:hover` states. **Never swap a Claude Design DS export
wholesale — graft only the validated additions onto the current package** (did
exactly that; purely additive, verified nothing lost). **3 Codex rounds** resolved
(aria-sort→aria-pressed + direction carried in the accessible name; Schwab metric
edge-cases; localStorage persistence precedent; preview keys + static aria-labels).

## Current state

On `main` @ `815ed70`, pushed, in sync with origin. Two commits: `4bca81a` (docs
spec across interface / portfolio / TO + both workflow render steps + `.metis/`
BUILD/INDEX) and `815ed70` (design-system controls + new
`preview/analytical-controls.html`). **Spec'd-not-built** — these are display
affordances on the **not-yet-built** full Portfolio/TO UIs; they implement when
those surfaces are built. Build order unchanged.

## Open questions

- **M5-calibration (carried):** Stooq 8 PM-ET / 24h refresh, ~4wk
  `continuity_weight` bands + Research-stale threshold, tripwire thresholds, DTO
  deep-research budget default, leftover-budget oldest-N ordering, archive
  retention 100 + upside-exhausted threshold.
- **Four-part verdict model + bidirectional-conviction bound** (carried): lands
  when full Portfolio + TO are built.
- §1 **genuinely-open drafts** (carried): dead-money hurdle, feasible-set bounding;
  TO risk-tier / horizon / hypothesis-score / quota / gate tables.
- Standing **M5-gated backlog** (carried): web-research provisioning / gating / UI
  + rendered-retrieval, analytical-register live-check, no new Tavily, local-model
  live quality / FMP-tier.

## Where to start

Begin the **live Schwab OAuth slice** (next in build order — unaffected by this
session's docs/DS work; `schwab-integration.md` audited clean: OAuth loopback,
30-min/7-day tokens, Keychain, positions + option chains). The sorting/views
controls (like the DTO/ATO redesign + four-part verdict) implement **later**, when
the full Portfolio/TO UIs are built. **Check code-vs-doc first** on any
Portfolio/TO formula — PR #45 already implements the MVP engine math.
