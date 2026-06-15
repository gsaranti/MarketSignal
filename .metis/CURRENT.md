# Current session handoff

## What happened

**Shipped tuning-constant calibration to `main` (`e3dc771`, squash-merged).** Added two `#[ignore]`d live probes to `pipeline.rs` (`tuning_dedup_threshold_calibration`, `tuning_topk_selectivity_probe`) that validate the memory tuning constants against **real `text-embedding-3-large` geometry** — the gap the synthetic separating embedders can't cover. **Key correction to the prior handoff's framing:** the separating embedders it called for (`BasisEmbedder`, `DistinctEmbedder`) *already existed* and already covered the dedup *mechanism*; since those embedders only emit cosine 1.0/0.0, they say nothing about the *values* — so the real residual was empirical value-calibration via live embeddings, not building an embedder. The live run found **0.93 vacuously high** (genuine restatements embed at ~0.72–0.81 cosine, distinct lessons cap at ~0.53), so **`LEARNING_DEDUP_THRESHOLD` was retuned 0.93 → 0.65** (mid-gap: 4/4 restatements dedup, no distinct pair merges). **`MEMORY_TOP_K=5` validated unchanged** (clean cliff after rank 4). A Codex review round was folded in: the dedup probe's reachability guard was strengthened from `paraphrase_ceiling` (one pair) to `paraphrase_floor` (full recall over the corpus), realigning it with the original plan.

## Current state

HEAD is **`e3dc771`**, working tree clean, in sync with `origin/main`. **Nothing in flight — the feature is complete.** PR #29 auto-closed (its content landed via the local squash). (Aside, resolved: a mid-session `gh` API `401` was a user-side token issue, now re-authed and verified working.) The prior handoff's "uncommitted chart-convention edits" are moot — they were already committed (`215f3b4`) before this session.

## Open questions

- *(resolved this session)* Tuning bundle — the **geometry-sensitive** constants are now validated live: `LEARNING_DEDUP_THRESHOLD` (now **0.65**) and `MEMORY_TOP_K` (5), via the two `#[ignore]`d probes. The rest of the bundle (`LEARNINGS_PER_REPORT_CAP=5`, `MAIN_AGENT_RECENT_REPORTS=3`, `RECENT_REPORT_BODY_CAP=12_000`, inbox caps, `COVERAGE_FLOOR=0.6`) are **not** embedding-geometry-sensitive, so the "vs real geometry" framing no longer applies — they stay on reasoned engineering defaults.
- *(new / small)* `BUILD.md` §Vector-memory still cites "`LEARNING_DEDUP_THRESHOLD`, 0.93" — **stale, update to 0.65** (a user-run `.metis/` edit).
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; esbuild/vite advisory parked; wiremock / in-loop offline gap; conditional GPT-5-mini extraction stage.
- *(low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide.

## Where to start

Nothing is owed — the feature is shipped and on `main`. One small cleanup if you want it: update `BUILD.md`'s stale `0.93` → `0.65`. Otherwise pick the next meaningful carried item — likely the **tracker live-SSE smoke** (the streamed-token decoder is still unexercised against a real wire) or running `fmp_baseline_smoke` now that quota has reset.
