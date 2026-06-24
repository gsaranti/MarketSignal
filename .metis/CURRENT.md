# Current session handoff

## What happened

Shipped the **CFTC Commitments-of-Traders positioning** feature end-to-end — designed,
implemented, reviewed, and **merged to `main`** (PR **#42**, squash **`7cc788b`**). A new
**keyless** Socrata adapter (`src-tauri/src/cot.rs`) contributes a `cot_positioning` group
to the Step-3 baseline: speculator net (leveraged / managed money) + an asset-manager
"real money" line on **8 bellwether** index / rate / FX / commodity contracts, pinned by
CFTC contract code. **Fail-soft, additive (no coverage floor), delta-exempt**; required
response-identity + 21-day freshness guards; backwards-compatible both directions. Points
the Positioning & Sentiment analyst lens at the new data. Reviewed clean (metis approve +
**3 Codex rounds**, all findings closed); a live `#[ignore]` smoke verified all 8 contracts
against the real API. Full as-built detail in [[cftc-cot-positioning]].

## Current state

`main` at **`7cc788b`**, tree clean, feature branch deleted. The COT feature is **on `main`
but not yet released**: still version **1.0.0**, and the **installed app is the pre-COT
v1.0.0** (built last session). So COT is committed but not running anywhere yet. The #41
prompt changes remain **installed and LIVE-UNVALIDATED** (report history empty, next report
= #1).

## Open questions

- **Cadence Run B** — still owed, but reframed: the first production reports run on the
  **COT build**, so the #41 goals (session-tense + conviction + freshness) and COT both land
  in report #1; the report *after* #1 closes the delta-engine + vector-memory-recall check.
  The earlier "v1.0.0-validation-reports-first" sequencing is **dissolved** (user's call)
  ([[manual-pivot-cadence-windows]]).
- **COT calibration** — how hard to weight positioning extremes is deferred to live runs
  (forcing-function posture); the data plumbing + lens pointer are already in
  ([[skills-forcing-function-only]]).
- **Market holidays / early closes** — `market_clock` still mislabels them "open until 4pm"
  (documented v1 cut; needs an NYSE calendar).
- **opus-main leaning** — accumulating; worked-examples prompt an optional carry
  ([[live-config-opus-main-leaning]]).

## Where to start

**Version-bump 1.0.0 → 1.1.0** (the 5 anchors move together) and **rebuild + install** the
bundle over the installed v1.0.0 (reads the same data dir, keys/models intact) — see
[[release-build-install]]. Then generate the **first report on the COT build**: it validates
the three #41 goals *and* the new positioning group together, and starts the Cadence Run B
chain. A second report after it closes the delta / recall check.
