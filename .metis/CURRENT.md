# Current session handoff

## What happened

**Shipped the Step-6 baseline enrichment — squash-merged to `main` as `320c097` (PR #9).** One slice, four bundled changes:
- **FRED +11 series** (no schema change; folded into the two existing groups): credit spreads (HY/IG OAS) + 10y–3m / 10y–2y curve spreads in `internals`; NFCI/ANFCI/STLFSI4, initial+continued jobless claims (ICSA/CCSA), Fed balance sheet (WALCL), 30y mortgage (MORTGAGE30US) in `macro_levels`. These feed `risk_posture` / `market_cycle`.
- **FMP index historical EOD** → new additive `index_performance` group (`IndexPerformance`: weekly/MTD/YTD/52-week-range per index), **fail-soft per-index**.
- **Sector lookback** now skips weekends (`sector_candidate_dates`) — the Sunday job stops burning 2 empty weekend calls/run.
- **Shared `http_retry`** (bounded exponential backoff, 429/5xx/transport incl. mid-body, `Retry-After`-aware) over FMP/FRED/BLS/Tavily; **GDELT excluded** (lockout).

**Load-bearing facts/decisions (don't relitigate):**
- **Retry is HTTP-status/transport only.** Provider rate/plan limits arriving as **HTTP 200** bodies (FMP `Error Message`, BLS `REQUEST_NOT_PROCESSED`) are *deliberately fatal* — quota/plan/malformed, not transient; body semantics stay in the adapters, not the retry layer. Don't push them into `http_retry`.
- **FMP free tier** = US EOD-equities sandbox: `historical-price-eod/light` is free for the 4 indices + VIX (probed live); commodities/FX/crypto beyond gold are premium.
- **FRED limit** = 120 req/min, no daily cap (~33-req scan is well under).
- **Spread `change_pct` is low-signal** (percent of a near-zero/inverted spread); the **level** is the documented signal. Use **`STLFSI4`** — `STLFSI`/`2`/`3` are discontinued.

Reviews: metis-task-reviewer (approve-with-nits; per-index EOD nit fixed) + Codex (3 findings: mid-body-retry fixed, HTTP-200-limits resolved via doc, `Retry-After`-HTTP-date deferred-low). Docs amended (`data-sources.md`, `BUILD.md`).

## Current state

On **`main`** at **`320c097`**, merged + pulled, **working tree clean, nothing in flight**; feature branch deleted. Verified: `cargo test` (145 lib + integration) + `cargo clippy` clean + `npm run build`, **and the live `fred_baseline_smoke` + `fmp_baseline_smoke` ran green** — all 11 new FRED ids resolve and the EOD path resolves per index. The **baseline data path (FMP/FRED/BLS) is now live-verified**; the **Step-7 news funnel was NOT exercised this session** (still unrun).

## Open questions

- **Step-7 news funnel never run live** — `news_ingestion_smoke` + `headline_filter_funnel_smoke` still need `TAVILY_API_KEY`/`OPENAI_API_KEY` and a **cool GDELT egress IP**. (Distinct from the baseline smokes, now green.)
- **Snippets omitted from the filter prompt** — `format_headlines` sends title+source only; revisit against live output.
- *(follow-up)* spread **absolute-bps change field** if percent-of-spread proves insufficient (level is the signal today).
- *(parked)* retention-cascade enforcement (30-report cap + cascade, durable-learning survival) and step-5 auto-archive — self-contained slices.
- *(deferred, paid)* calendar `expected` consensus + FOMC schedule; *(low)* GDP `change_pct` not annualized / reads 0 when two latest readings are equal.
- *(carried, low)* no Vue component-test harness; **`wiremock` still deferred** — so the `http_retry` loop and data-source floors have no HTTP-level unit coverage (pure predicate/backoff + live smokes only); `cargo fmt` dirty repo-wide (pre-existing, not the gate).

## Where to start

Forward slice is still **Step 8: research routing** (`/metis-plan-task`). The fixed **Claude Sonnet** router turns the 7b clusters (+ the now-richer baseline — credit/curve spreads, financial conditions, index performance — recent reports, vector memory, inbox, upcoming events) into a bounded research plan; the clusters are its first consumer, retiring the "unwired" flag. Alternatives: run the Step-7 news-funnel live smokes (needs a cool GDELT IP), or take the parked retention-cascade / step-5 auto-archive slice.
