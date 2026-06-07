# Current session handoff

## What happened

**Baseline-history + per-report deltas shipped to `main`** — squash `fdc65a4` (PR #13). A new slice on top of the twelve-group Step-6 baseline: each run's `BaselineMarketData` is serialized to a new SQLite `baseline_snapshots` table, and on the next run the app computes a deterministic level-by-level change view (`baseline_delta::compute_deltas`) against the prior snapshot and hands it to the main agent — so the thesis grounds change in real deltas, not just the prior report's prose.

**Load-bearing decisions (don't relitigate):**
- **Deltas computed in Rust, not by the model** — honors the spine (app orchestrates, agents pure). Carried on `MainAgentInput.deltas`.
- **Anchor = app-minted `as_of`** (job start) for both the snapshot `captured_at` and the elapsed Δt; deliberately ≠ the agent-minted report `created_at`; snapshot↔report join is by `report_id`.
- **Cadence-agnostic, not "weekly"** — reports aren't weekly (manual / missed runs), so the change view carries the *actual* elapsed interval; a same-hour regen reads as near-zero over a tiny interval, not a flat market.
- **Delta coverage = level-bearing groups only** (indices, internals, macro_levels, labor_levels, sector_pe, market_risk_premium), joined by series id. `sectors` / index_performance / movers / earnings / calendar / industries excluded (returns or set-valued).
- **`new`/`missing` transitions carry the gap reason** (joined to that run's gaps manifest) so a data-feed outage ≠ a series leaving the market (Codex P2 fix).
- **Additive / non-floor / fail-soft** — never gates a run; snapshot write best-effort, failures `eprintln!`'d (Codex P3 fix). Forward-compat: every `BaselineMarketData` field is `#[serde(default)]` + `BASELINE_SCHEMA_VERSION`. `Hash` added to `GroupKind` for the join key.

Reviews: metis-task-reviewer approve; 2 Codex rounds (P2 gap-reason + P3 diagnostic fixed, then good-to-go).

## Current state

On **`main` @ `fdc65a4`**, in sync with origin, **working tree clean**, feature branch deleted (local + remote). Nothing in flight. Verified pre-merge: **`cargo test` 189 lib + 12 integration, `cargo clippy --all-targets --all-features` clean, `npm run build` clean**. Backend-only.

**Docs + BUILD.md updated for this slice** (this session, uncommitted on `main`): `BUILD.md` (SQLite store, the `BaselineMarketData` data-model paragraph — the stale "not a persisted store" claim is gone, `app` module, testing), `docs/storage.md` (new §Baseline Snapshots), `docs/weekly-report-workflow.md` (Step 6 + Step 10 packet list), `docs/agents.md` (Main Agent inputs), and `.metis/INDEX.md`. These still need a commit.

## Open questions

- **Snapshot retention vs. report cascade** — `baseline_snapshots` has its own 14-report cap (self-pruned); the 30-report report cascade is still unbuilt. `report_id` is stored so snapshots *can* cascade-delete with their report — decide which when the cascade lands. (Folds into the parked retention-cascade item.)
- **Live FMP smoke** still deferred — run `fmp_baseline_smoke` once after the 250/day quota resets (confirmation, not a gap; offline-covered).
- **FRED freshness guard** in `fred_baseline_smoke` — `fetch_series` is latest-numeric-desc with no date bound, so a frozen series resolves stale (how `NASDAQVOLNDX` slipped). Quick win.
- *(carried, untouched)* tracker **live-SSE smoke** unrun; `COVERAGE_FLOOR=0.6` a set must-have not a final constant; slice (B) degraded-past-report reader signal missing; wiremock / in-loop offline gap; **Step-7 news funnel** never run live (Tavily/OpenAI keys + cool GDELT IP).
- *(low / parked)* filter-prompt snippets; step-5 auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Step 8: research routing** via `/metis-plan-task` — the natural forward slice, and the deferred consumer of this session's deltas: the `deltas` local in `generate_report` (and `MainAgentInput.deltas`) is ready for the fixed Claude Sonnet router to read alongside the Step-7 clusters → a bounded research plan (`docs/weekly-report-workflow.md §Step 8`). First, **commit the uncommitted docs/BUILD.md/INDEX.md edits** from this session. Quick wins if preferred: live FMP smoke (once quota resets), FRED freshness guard.
