# Current session handoff

## What happened

**Shipped the FRED-sourced economic-release calendar — completes the Step-6 macro/calendar group. Commit `f46ebe7`, pushed (`f48bb61..f46ebe7`).**

- **Load-bearing pivot (FMP→FRED):** FMP's `economic-calendar` endpoint is premium-gated (live-verified HTTP 402 stable / 403 legacy), so the calendar is built on **FRED's free `release/dates` schedule**, not FMP — the same free-tier split as dollar/oil/gas. No free source carries analyst **consensus**, so an optional `expected: Option<f64>` is reserved for a future paid tier.
- **Shape:** new `EconomicRelease {release, date, status, expected}` + `calendar` field on `BaselineMarketData`; curated `RELEASES` table (CPI, PCE, jobs, PPI, retail, JOLTS, GDP), windowed `[today-10d,+21d]`, classified released/upcoming. Reaches the model via the existing whole-baseline serialization (no formatter change).
- **FOMC dropped** — FRED has no scheduled-date series for "FOMC Press Release"; with the no-data flag it **fabricates one row per day** (live-discovered). Don't retry it via FRED release-dates; Fed stance stays in the target-range series.
- **Backing actuals:** added 4 FRED series to `MACRO_SERIES` (`PPIFIS`, `RSAFS`, `JTSJOL`, `GDPC1`) so every curated release's figure reaches the model via `macro_levels`/`labor_levels`.
- **Fail-soft:** a failed calendar fetch degrades to empty + `eprintln!` (research-inbox policy); off the completeness floor. Required series groups still fail loud.
- **Two Codex rounds folded in:** P1 (abort→fail-soft), P2 (4 backing series), P3 (smoke now validates each id by **name** against FRED's `releases` catalog — catches a wrong-but-valid id, proven to bite — plus a wide-window dates probe). Codex's `realtime_*` finding withdrawn on live evidence. **Both metis-task-reviewer and Codex approve.**

Verified: `cargo test` (113) + `cargo clippy` (clean) + live `fred_baseline_smoke`.

## Current state

On **`main`** at **`f46ebe7`**, **pushed**, in sync with `origin/main`, **working tree clean. Nothing in flight.** The **Step-6 baseline scan is now complete** — indices, internals, sectors, macro_levels, labor_levels, and the calendar all ship. `docs/data-sources.md` amended (calendar FMP→FRED); `.metis/BUILD.md` adapters line updated this session.

## Open questions

- **GDP `change_pct` is raw quarter-over-quarter, not annualized** (markets quote annualized). Same family as the level-series change_pct question below; level is correct. Low priority.
- **`change_pct` reads 0 on level series** when the two most recent readings are equal — level (`price`) always correct. Low priority; **candidate to retire**.
- **Calendar `expected` consensus + a FOMC meeting schedule** — both need a future **paid** source (no free US consensus; FRED has no FOMC schedule series). Deferred enhancement.
- A **`/metis-reconcile`** could fold in the `data-sources.md` calendar amendment.
- *(parked)* **retention-cascade enforcement** (30-report cap + cascade, durable-learning survival) and **step-5 auto-archive** — self-contained slices.
- *(carried, low)* no Vue component-test harness; data-source floors via pure helpers not an HTTP mock (`wiremock` deferred); `cargo fmt` dirty repo-wide (pre-existing; not the gate).

## Where to start

Step-6 is done — pick the next slice → `/metis-plan-task`. The forward direction is the **news/research pipeline (Steps 7–10)**; the parked self-contained alternative is **retention-cascade enforcement**.
