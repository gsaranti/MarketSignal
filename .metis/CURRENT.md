# Current session handoff

## What happened

**Shipped slice 1 of the Step-6 macro/calendar group — the FRED `macro_levels` group. Committed `a068aec`, pushed to `origin/main` (`77e7baf..a068aec`).**

- **Closed last session's live-verification TODO first:** obtained a real FRED key (saved at `~/.config/market-signal/keys.env`), ran both baseline smokes with real keys. All FMP series resolve including **`GCUSD` gold on the free tier** (the regression-prone symbol) and all FRED series — the FRED/gold mappings are now live-confirmed.
- **Decomposed the macro/calendar group into 3 sibling slices** (FRED macro levels · BLS adapter · economic calendar) and built the first. The macro "Macro" bullet spans three data shapes/sources, so it is not one task.
- **`macro_levels` group** added to `BaselineMarketData` (reuses `Quote`), populated from six FRED series via the unchanged `interpret_response`/`observations_to_quote` machinery; composite merges it primary-first. Series: `DFEDTARU`/`DFEDTARL` (Fed-funds target range), `T5YIE`/`T10YIE` (inflation breakevens), `UMCSENT` (sentiment), `PCEPI` (PCE index) — all live-verified.
- **Decision: "Fed expectations" → Fed-funds target range** (`DFEDTARU`/`DFEDTARL`), because no configured source has free futures-implied expectations (FMP-free is equities-only, FRED has no futures, BLS is labor-only).
- **Codex P2 fixed:** the first cut used a *both-empty* floor (`&&`), which silently weakened the pre-slice internals guarantee. Replaced with a **pure per-group floor** (`check_completeness` — each non-optional group fails independently), aligning FRED with FMP's required-group floor, plus offline tests for the four cases.
- Reviews: metis-task-reviewer **approve**, then Codex **approve** after the floor fix.

Verified: `cargo test` (101) + `cargo clippy` (clean) + live `fred_baseline_smoke` (5 internals + 6 macro) + `npm run build`.

## Current state

On **`main`** at **`a068aec`**, **pushed**, in sync with `origin/main`, working tree clean. **Nothing in flight.** The FRED macro-levels slice is done, reviewed, committed. The two **sibling slices remain queued**: the **BLS adapter** (keyless REST, CPI/jobs labor actuals — point-in-time levels) and the **economic calendar** (a different event shape — release schedule + prior-week reports with name/date/actual/expected/prior, from FMP's economic-calendar endpoint).

## Open questions

- **`change_pct` reads 0 on the rate/breakeven series in the live run.** Correct for the target range (constant between FOMC moves). For the breakevens it means the two most recent *numeric* observations in the 10-obs window were equal (or only one was numeric, given publication lag). The **level** (`price`) is always correct; `OBSERVATION_LIMIT` (10) is the knob if day-over-day change on breakevens ever matters. Low priority, pre-existing machinery.
- **BLS adapter + economic calendar** — the two remaining thirds of the Step-6 macro/calendar group (see *Current state* for their shapes).
- *(parked)* **retention-cascade enforcement** (30-report cap + cascade, durable-learning survival) and **step-5 auto-archive** — self-contained slices.
- *(carried, low priority)* no Vue component-test harness; data-source floors are tested via pure helpers, not an HTTP mock (`wiremock` deferred); `cargo fmt` dirty repo-wide (pre-existing; not the gate).

## Where to start

Pick the next slice → `/metis-plan-task`. Natural follow-on within the macro group: the **BLS adapter** (a new keyless source — CPI/jobs actuals as a labor-levels group) or the **economic calendar** (a new event model). **Retention-cascade enforcement** is the smaller self-contained alternative.
