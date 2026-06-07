# Current session handoff

## What happened

**Shipped the Step-6 baseline micro-enrichment slice — pushed as branch `baseline-micro-enrichment` (`d8e22a9`), not yet merged (user opening the PR).** Added an equity-level breadth layer so the research router gets single-name/breadth context, not just index/macro aggregates: **FMP movers** (biggest gainers / losers / most-actives, filtered to major-exchange companies above a price floor, leveraged/inverse ETFs excluded, capped per list), the **FMP earnings calendar** (prior-week + upcoming large-cap reporters, filtered by revenue estimate, free ~1-month window), **FRED** volatility term structure (VXV `VXVCLS`, VXN `VXNCLS`) + credit-quality dispersion (BBB `BAMLC0A4CBBB`, single-B `BAMLH0A2HYB` OAS), and **FMP silver** (`SIUSD`).

**Load-bearing decisions (don't relitigate):**
- New `movers` / `earnings` baseline groups are **additive & non-floor** (like `calendar` / `index_performance`): degrade to recorded `DataGap`s, never gate `enforce_coverage`.
- **Breadth in the baseline; per-name fundamentals stay in Step-9 router-directed research.** Constituent lists / batch-quote / copper are FMP-premium (live-probed), so movers carry **no sector** (LLM infers from ticker; prompt caveat: a mover may be a fund → read as flow) and earnings are filtered by **revenue magnitude**, not index membership.
- Free-vs-premium tiers were settled by a live probe harness (`fmp_freetier_probe`, kept `#[ignore]`d). Full tier table in memory `fmp-freetier-baseline-probe`.

Reviews: metis-task-reviewer (approve) + **Codex ×3, all fixed** — P1 `NASDAQVOLNDX` was discontinued/frozen (~11,882, an index level) → swapped to `VXNCLS` (~23 live); P2 leveraged ETFs leaked into movers → name-heuristic exclusion + prompt softening; bull/bear substring false-positive (would drop "Build-A-Bear") → dropped bare markers, added regression test.

## Current state

On **`baseline-micro-enrichment` @ `d8e22a9`**, pushed to origin, **working tree clean**; `main` unchanged. Verified: **`cargo test` 168 lib + 11 integration, `cargo clippy --all-targets --all-features` clean, `npm run build`**, plus live FMP/FRED smokes (movers ETF-free, VXN 23.22, earnings resolve). Backend-only — frontend run tracker renders the new group labels generically (no change). Two items left to the user: open/merge the PR, and update `.metis/BUILD.md` (below).

## Open questions

- **`.metis/BUILD.md` not yet updated** for this slice — its data-model + adapters lines need the `movers` / `earnings` groups, the +4 FRED series (VXV/VXN/BBB/single-B), and silver. *(Pending decision — user to apply.)*
- **FRED freshness guard recommended** in `fred_baseline_smoke`: `fetch_series` takes latest-numeric-desc with no date bound, so a discontinued series feeds a stale value silently and still "resolves" (how `NASDAQVOLNDX` slipped both reviews). Lesson now in memory: live-probe new FRED series for recency + sane magnitude, not just existence.
- **Movers heuristic is imperfect by design** — plain ETFs (e.g. QQQ) still pass; the prompt "treat funds as flow signals" caveat is the intentional backstop (over-tuning the name list risks dropping real companies).
- *(carried, untouched)* tracker **live-SSE smoke** unrun (real OpenAI/Anthropic `stream_delta` shapes); **`COVERAGE_FLOOR=0.6`** is a named must-have set, not a higher constant; **slice (B)** degraded-past-report *reader* signal still missing; **wiremock / in-loop** offline gap; **Step-7 news funnel** never run live (Tavily/OpenAI keys + cool GDELT IP).
- *(low / parked)* filter-prompt snippets; retention-cascade + step-5 auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

User is opening the `baseline-micro-enrichment` PR; once merged, **update `.metis/BUILD.md`** for the new groups/series. Then pick the forward slice via `/metis-plan-task`: **(a)** the deferred **"valuation & finer-rotation"** slice — sector-PE, industry-performance + industry-PE, market-risk-premium (all already free-probed); or **(b)** the original forward slice **Step 8: research routing** (fixed Claude Sonnet router over the now-richer baseline + Step-7b clusters → bounded research plan). Quick win regardless: the **FRED freshness guard** in `fred_baseline_smoke`.
