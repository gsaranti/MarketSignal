# Current session handoff

## What happened

**FMP company-news slice shipped** — squash `e239d27` (PR #22), on `main`. The probe gate (`fmp::tests::fmp_news_probe`, run once live) settled the tiers: `fmp-articles` is **free** (200, `page`/`limit` honored); the entire third-party `news/*` family is **premium** (402). Adopted the free feed: new `FmpNewsSource` (`src-tauri/src/fmp_news.rs`) gathers one bounded 20-article page per run — HTML-stripped, ticker-prefixed snippets (400-char cap), tracker rows under the `news` group, **GDELT-style fail-soft** (load-bearing: `CompositeNewsSource` propagates child errors and the pipeline discards the *whole* gather on error, so the hedge must never error). `ResearchStages::live` gained `fmp_key` (reuses the gate-required credential); news is now the nested composite **Tavily → GDELT → FMP last** — dedup keeps first occurrence, so the hedge never displaces primary framing; don't reorder casually. Docs: `data-sources.md` (tiers + FMP Articles entry) and `weekly-report-workflow.md` §Step 7.

Reviews: **Metis approve** (clean, re-ran verification independently); **Codex no blocking defects + 1 Low** (stale Tavily+GDELT-only docs) — fixed, and a repo-wide grep caught three more stale spots Codex missed (`news.rs` RawHeadline doc, `headline_filter.rs`, `tavily.rs`). Second slice running where the repo-wide sweep out-catches Codex on stale docs — keep doing it.

## Current state

On **`main` @ `e239d27`**, synced with origin, branch deleted, **nothing in flight**. `cargo test` 256 passed / 0 failed / 13 ignored, clippy `--all-targets --all-features` clean, `npm run build` OK. Live-verified once each: probe (5 calls) and `fmp_news_smoke` (1 call — full 20-headline page). The full `live_research_packet_smoke` was **not** rerun (plan-optional); its next deliberate run exercises the third source — spend is now ~9 news calls (7 Tavily + 1 GDELT + 1 FMP) and needs `FMP_API_KEY` too.

## Open questions

- **LanceDB / vector memory entirely unbuilt** — blocks Step-4/Step-10 memory pulls, embeddings (`text-embedding-3-large`), 30-report/durable-learning retention; its own multi-slice effort. Now the clear front-runner.
- **`.metis/BUILD.md` gather passage stale** — still describes the news gather as Tavily+GDELT; needs a one-line update to add FMP Articles (user-run write).
- **Reduced `RouterInput`** — still 3 of 7 doc inputs (baseline, deltas, clusters).
- **Brancher tuning (deferred)** — thresholds, keyword sets, cadence stance; ships oil+yields only by design.
- **Optional GUI tracker run** — visual corroboration that research rows bucket correctly live (~40 FMP calls + one generation); corroboration only.
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; snapshot retention vs. 30-report cascade; tracker live-SSE (streamed-token) smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Plan the first LanceDB / vector-memory slice** — the FMP company-news follow-on is done, so the largest unblock is unobstructed. It's a multi-slice effort: `/metis-plan-task` the *first* slice only (likely the store + embedding seam, offline-stubbable per the spine). Fold the one-line `BUILD.md` gather update in while touching project state.
