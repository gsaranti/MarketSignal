# Current session handoff

## What happened

**Shipped the systemic `unit`-field fix — the small self-contained slice that was a standing open question. Committed `028ddac`, pushed to `origin/main` (`7fc1088..028ddac`).**

- Added `unit: String` to the baseline `Quote` (`data_sources.rs`), populated **per-series from each adapter's own const table** rather than the wire. The five tables (FMP `INDEX_SYMBOLS`/`INTERNAL_SYMBOLS`, FRED `INTERNALS_SERIES`/`MACRO_SERIES`, BLS `LABOR_SERIES`) became `(symbol, name, unit)` 3-tuples; the unit is threaded through the three shaping fns (`quotes_from_value` / `observations_to_quote` / `series_to_quote`) and their callers.
- **Removed the BLS inline-units stopgap:** display names de-unitized (e.g. "Total Nonfarm Payrolls (thousands of persons)" → name "Total Nonfarm Payrolls" + unit "thousands of persons"); the `LABOR_SERIES` "unit rides in the name" doc paragraph dropped.
- **Field annotates `price` only** (not `change_pct`, which stays a percent for every series). It reaches the model via the whole-baseline `serde_json` serialization in `build_user_prompt` — **no formatter change** (the property the BLS slice already leaned on), proved by a new `contains("index points")` assertion on the prompt.
- Review: metis-task-reviewer **approve** (clean — reviewer re-ran all gates, no nits, empty scope report confirmed honest).

Verified: `cargo test` (110) + `cargo clippy` (clean) + `npm run build`.

## Current state

On **`main`** at **`028ddac`**, **pushed**, in sync with `origin/main`, working tree clean. **Nothing in flight.** The unit-field slice is done, reviewed, committed. **One Step-6 sibling remains:** the **economic calendar** — a different event shape (release schedule + prior-week reports with name/date/actual/expected/prior, from FMP's economic-calendar endpoint), the last third of the macro/calendar group.

## Open questions

- **Economic calendar** — the remaining third of the Step-6 macro/calendar group (shape in *Current state*). The natural next slice.
- **`change_pct` reads 0 on level series** — the FRED rate/breakeven series *and* the BLS unemployment rate in the live run, when the two most recent readings were equal. The **level (`price`) is always correct**; the obs-window limit is the knob if a day/month-over-month delta ever matters. Low priority; **candidate to retire** as a recurring question.
- *(parked)* **retention-cascade enforcement** (30-report cap + cascade, durable-learning survival) and **step-5 auto-archive** — self-contained slices.
- *(carried, low priority)* no Vue component-test harness; data-source floors tested via pure helpers, not an HTTP mock (`wiremock` deferred); `cargo fmt` dirty repo-wide (pre-existing; not the gate).

## Where to start

Pick the next slice → `/metis-plan-task`. Natural follow-on: the **economic calendar** (a new event model — completes the Step-6 macro/calendar group). **Retention-cascade enforcement** is the parked self-contained alternative.
