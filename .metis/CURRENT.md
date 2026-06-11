# Current session handoff

## What happened

**Live research smoke slice shipped** — squash `d5b42da` (PR #21), on `main`. The prior handoff's "validate the research path live first" item is **done**: new `#[ignore]`d `pipeline::tests::live_research_packet_smoke` runs news → filter → route → execute → packet through the **production wiring** under a recording `RunContext`, asserting anti-vacuously per stage (the whole half is fail-soft, so a bare success check proves nothing) plus tracker request-row group attribution ({news, filter, routing, research} — what `App.vue` buckets under the research step). **Live-verified green**: 6 clusters; 5 topics, 20 findings, 100 sources, 20 requests — exactly the 5×4 router ceiling; rows news 16 / filter 2 / routing 2 / research 40 (one per HTTP call). A first attempt failed red on a transient OpenAI 502 degrading the filter — the anti-vacuity working; retry green. GDELT 429'd (dev IP) and was absorbed fail-soft, as designed.

Reviews: **Metis approve-with-nits** (nit closed — `RecordingReporter` got a `messages()` accessor, now a crate-visible `#[cfg(test)]` helper in `progress.rs`); **Codex 1 Medium + 2 Low, all fixed**: `ResearchStages::live` (pipeline.rs) replaced lib.rs's private `live_research_stages` so production command paths and the smoke **share one construction** (don't reintroduce a mirrored copy); the smoke's spend doc corrected to 20 executor searches; stale "unwired" module docs swept across seven research-half modules (a repo-wide grep caught `model_agent.rs`, which Codex missed).

## Current state

On **`main` @ `d5b42da`**, synced with origin, branch deleted, **nothing in flight**. `cargo test` 236 passed / 11 ignored, clippy `--all-targets --all-features` clean, `npm run build` OK. Smoke rerun (deliberately, ~30 Tavily + 1 OpenAI + 1 Anthropic, no FMP): `source ~/.config/market-signal/keys.env && cargo test --manifest-path src-tauri/Cargo.toml --lib live_research_packet_smoke -- --ignored --nocapture` — capture with `tee`, not `tail`. A red can be an honest provider transient (502) — check the degraded-stage stderr line before suspecting code.

## Open questions

- **LanceDB / vector memory entirely unbuilt** — blocks Step-4/Step-10 memory pulls, embeddings (`text-embedding-3-large`), 30-report/durable-learning retention; its own multi-slice effort.
- **FMP company-news follow-on** — live free-tier probe → `data-sources.md` amendment → new `NewsSource` adapter. Parked by decision.
- **Reduced `RouterInput`** — still 3 of 7 doc inputs (baseline, deltas, clusters).
- **Brancher tuning (deferred)** — thresholds, keyword sets, cadence stance; ships oil+yields only by design.
- **Optional GUI tracker run** — visual corroboration that research rows bucket correctly in the live tracker (~40 FMP calls + one generation); the smoke already asserts group attribution, so corroboration only.
- *(carried)* `fmp_baseline_smoke` still unrun since the quota reset; snapshot retention vs. 30-report cascade; tracker live-SSE (streamed-token) smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Pick the next slice** — the research half is now wired *and* live-validated, so nothing blocks either candidate: **LanceDB / vector memory** is the largest unblock (memory pulls, embeddings, retention; likely multi-slice — plan the first slice only), or **FMP company-news** as the smaller follow-on. `/metis-plan-task` whichever you choose.
