# Current session handoff

## What happened

**FRED freshness guard shipped to `main`** — squash `fb795d6` (PR #14). The FRED observations fetch is latest-numeric-desc with **no date bound**, so a discontinued/frozen series (the `NASDAQVOLNDX` class) still "resolves" to a stale value and slips past `fred_baseline_smoke`'s `len`-count assert. The slice adds a cadence-keyed freshness guard, **confined to the `fred.rs` test module**: a `Cadence` enum + `max_staleness_days`, a `FRESHNESS` table mapping every `INTERNALS_SERIES`+`MACRO_SERIES` id to a cadence with an **offline parity drift-guard** (a new unguarded series fails CI), a `latest_numeric_observation_date` helper mirroring `observations_to_quote` (skip `"."`, require finite `f64`), and a per-series freshness assert in the live (`#[ignore]`d) smoke.

**Load-bearing decisions (don't relitigate):**
- **Smoke-only scope, not a production gate** — `observations_to_quote` stays freshness-blind (fail-soft never gates a run); the smoke is the roster-rot guard, same posture as the calendar's per-id catalog discipline. A production staleness gate remains a separate, larger slice (would touch the `Unavailable`-gap path + `enforce_coverage`).
- **Bounds keyed to FRED's period-start dating** — obs are dated at period-start, so staleness *peaks just before the next release*; bounds (Daily 16 / Weekly 21 / Monthly 110 / Quarterly 230) were retuned against live data (first live run correctly false-positived on GDP at 157d > original 150d; CCSA at 15d would've tripped the original 14d weekly). Monthly/quarterly are inherently coarse (catch multi-month freezes only); daily/weekly stay tight.

Reviews: metis-task-reviewer **approve**; Codex one **Low** fidelity nit (helper didn't validate numericness) — fixed in-branch with a test.

## Current state

On **`main` @ `fb795d6`**, in sync with origin, **working tree clean**, feature branch deleted (local + remote). Nothing in flight. Verified: **`cargo test` 191 lib + 12 integration, `cargo clippy --all-targets --all-features` clean, `npm run build` clean; live `fred_baseline_smoke` green** (all 30 series fresh as of 2026-06-07). Backend-test-only — **no production code changed**.

Memory updated this session: `live-model-smoke.md` now records `~/.config/market-signal/keys.env` holds **all five** live keys (model + `FRED`/`FMP`/`TAVILY`); FRED has **no daily cap** (freely re-runnable smoke), FMP is 250/day.

## Open questions

- **Snapshot retention vs. report cascade** — `baseline_snapshots` self-prunes at 14; the 30-report report cascade is still unbuilt. `report_id` is stored so snapshots *can* cascade-delete — decide which when the cascade lands. (Folds into the parked retention-cascade item.)
- **Live FMP smoke** still deferred — run `fmp_baseline_smoke` once after the 250/day quota resets (confirmation, not a gap; offline-covered).
- *(carried, untouched)* tracker **live-SSE smoke** unrun; `COVERAGE_FLOOR=0.6` a set must-have not a final constant; slice (B) degraded-past-report reader signal missing; wiremock / in-loop offline gap; **Step-7 news funnel** never run live (Tavily/OpenAI keys + cool GDELT IP).
- *(low / parked)* FRED freshness bounds may need seasonal tuning at period boundaries (`max_staleness_days` is the single tuning point); filter-prompt snippets; step-5 auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Step 8: research routing** via `/metis-plan-task` — the natural forward slice and the deferred consumer of the baseline-history deltas: the `deltas` local in `generate_report` (and `MainAgentInput.deltas`) is ready for the fixed Claude Sonnet router to read alongside the Step-7 clusters → a bounded research plan (`docs/weekly-report-workflow.md §Step 8`). Remaining quick win if preferred: live FMP smoke (once quota resets).
