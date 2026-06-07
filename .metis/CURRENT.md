# Current session handoff

## What happened

**Shipped Step-6 partial-failure tolerance — squash-merged to `main` as `4c73537` (PR #10).** The baseline scan no longer aborts the weekly report when a single series/provider fails:
- **Adapters degrade to a recorded gap, not a `bail!`.** Each unresolved series/release → a `DataGap{group, series_id, series_name, reason}` on `BaselineMarketData.gaps`, merged across providers by the composite, serialized into the main agent's prompt so it reasons over what's absent. The per-adapter completeness floors (`check_completeness`, the FMP `indices.is_empty()` bail) are deleted.
- **One central gate replaces them:** `pipeline::enforce_coverage` (runs in `generate_report` right after `baseline_scan()?`) fails the run only if `indices` **and** ≥1 of {`internals`, `macro_levels`} fall below `COVERAGE_FLOOR` (0.6 of a group's expected, OutOfScope excluded).

**Load-bearing decisions (don't relitigate):**
- **`OutOfScope` = explicit provider permanent-absence signals ONLY** (FMP 402/404, FRED "does not exist"). A 2xx that merely carried *no value* — empty array, all-`.` FRED window, BLS empty-`data` — is **`Unavailable`** (counts against coverage). This split was the Codex fix; folding no-value cases back into OutOfScope re-opens a hole that bypasses the floor.
- **The floor's shape is what makes "degrade everything" safe:** a rejected FMP key (empties `indices`) or FRED key (empties `macro`; `internals` can't clear on FMP's VIX+gold alone) still **fails the run loudly**; only BLS/labor + the additive `calendar`/`index_performance` degrade silently.
- Gaps ride as a field on `BaselineMarketData` (no `MarketDataSource` trait-signature change); the composite now propagates only a *catastrophic* (non-data) child `Err`. `COVERAGE_FLOOR = 0.6` kept as a tunable in code + BUILD.md, not the docs.

Reviews: metis-task-reviewer (approve) + Codex ×3 rounds (two functional findings — empty FMP arrays + FRED all-gap windows bypassing the floor; then a stale-comment pass — all fixed). `docs/` (§Step 6, data-sources.md) + BUILD.md amended.

## Current state

On **`main`** at **`4c73537`**, merged + pulled, **working tree clean, nothing in flight**, branch deleted. Verified **offline**: `cargo test` (150 lib + 9 integration) + `cargo clippy` clean + `npm run build`. **Live smokes were NOT re-run this session** — the happy path (all series resolve → no gaps) is unchanged from the prior live-verify, but the new failure-degradation paths have no live coverage.

## Open questions

- **`COVERAGE_FLOOR = 0.6` is the live knob** — revisit if the Russell-gated "2-of-3 majors" case bites: because OutOfScope is excluded from the denominator, a permanently-premium Russell makes `indices` effectively "2 of 3 majors" at 0.6. The fix, if needed, is a named must-have set (S&P/Nasdaq), not a higher constant (which over-tightens elsewhere).
- **Slice (B) — frontend "degraded-but-successful job" UI signal** (deferred this session): a degraded run now *succeeds*, so a silently-dropped BLS isn't surfaced to the reader. Needs a per-report "degraded data" badge / warning-area entry; likely a design-system extension.
- **`wiremock` still deferred** → the partial-tolerance **in-loop gap wiring** (FMP/FRED empty→Unavailable, all-gap→Unavailable, `Rejected` short-circuit, transport→Unavailable) has **no offline coverage**; only the pure `interpret_response` classifiers, `enforce_coverage`, and BLS `assemble_labor_levels` are offline-tested. Loop paths ride on live smokes only — unrun.
- **Step-7 news funnel never run live** — `news_ingestion_smoke` + `headline_filter_funnel_smoke` need `TAVILY_API_KEY`/`OPENAI_API_KEY` + a cool GDELT egress IP.
- *(carried)* snippets omitted from the filter prompt (`format_headlines` sends title+source only); spread absolute-bps field if percent proves insufficient; *(parked)* retention-cascade + step-5 auto-archive; *(deferred/paid)* calendar `expected` consensus + FOMC; GDP `change_pct` not annualized; *(low)* no Vue component-test harness; `cargo fmt` dirty repo-wide (pre-existing).

## Where to start

Forward slice is still **Step 8: research routing** (`/metis-plan-task`) — the fixed Claude Sonnet router turns the 7b clusters + the now-richer baseline into a bounded research plan, retiring the "unwired" clusters flag. Alternatives surfaced this session: **slice (B)** the degraded-job UI signal (self-contained frontend), the **Step-7 news-funnel live smokes** (needs a cool GDELT IP), or the parked retention-cascade / step-5 auto-archive slice.
