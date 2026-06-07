# Current session handoff

## What happened

**Both Step-6 baseline enrichment slices are merged to `main`** — squash `873cae3` (PR #12). The micro-enrichment slice (movers / earnings / VXV·VXN / BBB·single-B OAS / silver, from last session) was opened and merged, and last session's deferred **"valuation & finer-rotation"** slice (a) was planned, implemented, reviewed, and merged in the same PR. The baseline now has **twelve groups**; the new `sector_pe`, `industries`, `market_risk_premium` are additive & non-floor like movers/earnings (degrade to `DataGap`s, never gate `enforce_coverage`).

**Load-bearing decisions (don't relitigate):**
- **Exchange-aware "Both" (user-chosen):** the FMP sector / industry P/E snapshots are exchange-specific (no-`exchange` default = NASDAQ; NYSE & AMEX also free), so the adapter gathers **both NASDAQ (growth-tilted) and NYSE (value-tilted)** to surface the growth-vs-value valuation spread rather than privileging one board.
- **Exchange integrity (4 Codex rounds):** every row is labelled by its **wire** exchange (not the request); the perf↔P/E join is keyed by `(industry, exchange)` so it's same-board; and each leg **validates** returned-board == requested, failing an off-board response as a `Malformed` gap (no silent mislabel/duplication).
- Non-positive industry P/E (FMP's `0.0` sentinel for loss-making) → `None`, not a fake "cheap" multiple. Prompt reads valuation **cross-sectionally and as a level** (not multiple-expansion, which one snapshot can't support).
- `.metis/BUILD.md` updated (data-model = twelve groups; adapters = FMP breadth/valuation + silver, FRED BBB/single-B + VXV/VXN, 250/day cap). Reviews: metis-task-reviewer (approve) + Codex ×4, all fixed.

## Current state

On **`main` @ `873cae3`**, in sync with origin, **working tree clean**, feature branch deleted (local + remote). Nothing in flight. Verified pre-merge: **`cargo test` 178 lib + 11 integration, `cargo clippy --all-targets --all-features` clean, `npm run build` clean**. Backend-only.

## Open questions

- **Live FMP smoke deferred** — run `fmp_baseline_smoke` once next session **after the 250/day FMP quota resets** to exercise the round-4 exchange-validation in the live path. Offline-covered by the reject tests, so this is confirmation, not a gap. (Discipline now in memory `fmp-smoke-rate-limit-discipline`: live smokes are scarce — at most once per change.)
- **FRED freshness guard** still recommended in `fred_baseline_smoke`: `fetch_series` takes latest-numeric-desc with no date bound, so a frozen series resolves stale (how `NASDAQVOLNDX` slipped). Quick win.
- *(carried, untouched)* tracker **live-SSE smoke** unrun (real OpenAI/Anthropic `stream_delta` shapes); **`COVERAGE_FLOOR=0.6`** is a named must-have set, not a higher constant; **slice (B)** degraded-past-report *reader* signal still missing; **wiremock / in-loop** offline gap; **Step-7 news funnel** never run live (Tavily/OpenAI keys + cool GDELT IP).
- *(low / parked)* filter-prompt snippets; retention-cascade + step-5 auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

Pick the next forward slice via `/metis-plan-task`. The natural one now that the baseline is fully enriched: **Step 8: research routing** — the fixed Claude Sonnet router over the richer baseline + Step-7b clusters → a bounded research plan (`docs/weekly-report-workflow.md §Step 8`). Quick wins first if preferred: the **live FMP smoke** (once quota resets) and the **FRED freshness guard** in `fred_baseline_smoke`.
