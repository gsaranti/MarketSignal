# Current session handoff

## What happened

**Vector-memory slice 1 shipped** — squash `bceb095` (PR #24), on `main`. The load-bearing call, resolved with the user: the store ships on **SQLite + exact in-Rust cosine** (BLOB f32-LE embeddings in a `vector_memory` table) instead of the docs' LanceDB — at this corpus scale an unindexed LanceDB runs the same exhaustive scan, and the crate's costs (arrow/lance/DataFusion tree, protoc, async-only API vs. the sync spine) bought nothing; swap stays contained behind the `vector_memory` module + `embedding::Embedder` trait seams. Landed: the store (insert / kind-filtered top-k search / `delete_report_summary` cascade hook), the embedder seam (stub + live `text-embedding-3-large`), the **Step-17 best-effort summary write** in the persist step (mirrors the snapshot block — embedding failure costs the row, never the report), and App.vue routing for the new `"memory"` tracker group. Reviews: **Metis approve**; **two Codex rounds** produced four findings, all fixed (non-finite-embedding guards at insert+search; one-summary-per-report partial unique index **plus** a NULL-`report_id` summary guard; stale-comment sweep). Project records updated with explicit user authorization: `BUILD.md` (engine deviation, RouterInput recent-report record, FMP-Articles gather line), `docs/` (storage.md §Vector Memory rename + amendment note, four other mentions), `INDEX.md` pointers.

## Current state

On **`main` @ `bceb095`**, synced with origin, branch deleted, **nothing in flight**. `cargo test` 277 passed / 0 failed / 14 ignored, clippy `--all-targets --all-features` clean, `npm run build` OK. **No live API spend this session.** Two deliberate smokes remain unrun: `embedding_live_smoke` (one OpenAI call; pins the 3072-dim contract) and the full `live_research_packet_smoke` (unrun since the FMP-news slice; ~9 news calls + up to 20 executor searches).

## Open questions

- **Vector memory: store landed, consumers not wired** — Step-4/10 retrieval (query construction + fail-soft pulls), `ResearchPacket.memory` / `RouterInput` memory input, and durable-learning writes (needs `MainAgentOutput` schema growth) are the follow-on slices. Retrieval is the clear front-runner.
- **RouterInput: 5 of 7** — vector memory (now unblocked) + parsed inbox documents (blocked on a Step-6 parsing slice).
- **Retention cascade still unbuilt** — the 30-report cascade now also owns the vector summary row; `vector_memory::delete_report_summary` is its ready hook. Snapshot-retention interplay carried.
- **Brancher tuning (deferred)** — thresholds, keyword sets, cadence stance; oil+yields only by design.
- **Optional GUI tracker run** — visual corroboration (~40 FMP calls + one generation); would now also show the persist-step memory row.
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Plan the Step-4/10 retrieval slice** (`/metis-plan-task`) — the store and search APIs are ready; the work is query construction (Step 4 from recent context + baseline/change view; Step 10 from research evidence), two fail-soft pulls, and threading hits into `RouterInput`, `ResearchPacket.memory`, and `MainAgentInput`. Implementation note that will bite otherwise: any new request group the retrieval emits needs an `App.vue` `requestStep` routing entry, or its rows misfile under baseline.
