# Current session handoff

## What happened

Built and squash-merged **Phase 2 of the local analysis suite — the narrow
single-equity Portfolio slice** (PR #45, `367f09b`) through the full Metis loop
(plan → implement → metis-review → Codex → merge). New `src-tauri/src/portfolio/`
(`mod`/`engine`/`dossier`/`pipeline`/`store`/`job`), `schwab.rs` (fixture
`HoldingsSource` + stub chain), `sec.rs` (keyless EDGAR company-facts adapter), a
per-company FMP pull reusing the `with_base_url` wire seam, and a per-job
**`namespace` partition** threaded through `vector_memory` (additive column,
backfill to `report`; cloud report pipeline behaviourally unchanged). The
load-bearing split held: the Rust **engine computes every number** (4 sub-scores →
A–F grade that rolls up from them → scenario EOM/EOY targets w/ methodology →
options-activity signal), the local model **only interprets** (action/conviction/
horizon/prose); missing inputs become tagged gaps; the options signal is kept **out
of the grade**. metis review = approve-with-nits; Codex caught three, all fixed: SEC
fetch failures were silently erased → now a tagged degraded-input gap; the required
`price_target_rationale` was dropped → now persisted; coarse cancellation →
checkpointed in the job and the per-company FMP/SEC calls.

## Current state

`main` at **`367f09b`**, clean, synced with origin; feature branch deleted.
`cargo test` 517 lib + integration green / clippy clean. **No work in flight.**
Pinned this slice: **N=10** run retention, **X=3** house-view reports, ~1mo/1yr/3–5yr
horizons, moderate/long/taxable default investor profile. *Calibratable (NOT pinned):*
grade-weight formula, risk-tier thresholds, options-signal params. **Deferred to later
slices** (all surfaced in the scope report): web research (SearXNG — stubbed behind a
function), live Schwab OAuth (fixture this slice), the Portfolio UI page
(backend+tests only), local-embedder vector recall (namespace column added now;
prior-run-verdict continuity *is* wired), the 122B roll-up synthesis (deterministic
roll-up shipped), and per-holding checkpoint/resume.

## Open questions

- **Live validation is hardware-gated** — the live `portfolio_live_smoke` (verdict
  quality, runtime, and whether FMP per-company endpoints are premium-gated on the
  tier) cannot run until the **M5 (128 GB)** arrives; user is on an M1. The first live
  Portfolio run on the M5 is the retroactive acceptance check
  ([[local-suite-hardware-gated]]).
- **Register the Schwab developer app** (carried) — multi-day approval is the external
  long pole; gates the live-Schwab slice's real-data runs.
- **Cadence Run B** (carried) — report #2 still un-run: validates the delta engine +
  memory recall; sanity-check yield levels vs the 2s10s claim and the COT read
  ([[manual-pivot-cadence-windows]], [[report-curve-number-consistency]]).
- **Standing report-side nits** (carried) — COT extreme-weighting calibration;
  `market_clock` mislabels holidays / early closes (needs an NYSE calendar); opus-main
  leaning accumulating ([[live-config-opus-main-leaning]]); do NOT reintroduce PDF
  `@page` margins.

## Where to start

Build order's next step is **`/metis-plan-task` for the live Schwab OAuth slice** —
swap the fixture `HoldingsSource` for the real Trader API loopback (30-min/7-day
tokens, Keychain) behind the same trait. **Kick off Schwab developer-app registration
first** (long approval lead; it gates real-data runs). Full Portfolio (funds + 122B
roll-up) and Trade Opportunities follow. **Before planning, advance BUILD.md's
local-suite build-order line** (see the pending-decision flag): substrate ✓ → narrow
Portfolio slice ✓ → next is live Schwab.
