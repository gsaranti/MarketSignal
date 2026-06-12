# Current session handoff

## What happened

**Step-4/10 vector-memory retrieval shipped** — squash `05934d4` (PR #25), on `main`. Both pulls live inside `pipeline::assemble_research_packet`: the Step-4 pre-research pull (query from recent report context + salient baseline levels + top change-view moves) feeds the new `RouterInput.memory` and is *ephemeral* — the packet carries only the Step-10 post-research pull (query from executor evidence) on `ResearchPacket.memory`, honoring the doc's replace-not-merge rule (compiler-enforced: `pre_memory` is moved into the router input). Shared `retrieve_memory` helper: top-5 across both kinds, no similarity floor, fully fail-soft, cancel-polled, and guarded — an empty query or empty store spends no embedding call. Hits reach both models as `[kind · created_at] content` fragments. **Two deviations worth knowing:** App.vue `"memory"`-group tracker rows follow the *currently-running step* (a dedicated step would strand as perpetually "running"); and after two Codex rounds the retrieval query is capped at **8,000 bytes** before embedding — bytes, not chars, because tokens ≤ bytes for byte-level BPE makes the 8,192-token limit provable without a tokenizer dependency. Reviews: Metis approve (all criteria); Codex rounds 1–2 fixed (size cap, then char→byte hardening).

## Current state

On **`main` @ `05934d4`**, synced with origin, branch deleted, **nothing in flight**. `cargo test` 291 passed / 0 failed / 14 ignored, clippy `--all-targets --all-features` clean, `npm run build` OK. **No live API spend this session.** `live_research_packet_smoke` now admits `"memory"`-group rows and passes a temp DB + stub embedder (no added spend when run); it and `embedding_live_smoke` remain deliberately unrun.

## Open questions

- **Memory loop half-complete: nothing writes learnings** — retrieval spans both kinds but only `summary` rows exist; durable-learning writes need `MainAgentOutput` schema growth (Step 17's second leg). Front-runner next slice.
- **Step-4 pull has no audit consumer** — doc says it also steers the Step-5 audit; no audit stage exists, so it lands in routing only. A future audit slice should consume the same `pre_memory` value (seam ready in `assemble_research_packet`).
- **RouterInput: 6 of 7** — only parsed inbox documents remain (blocked on a Step-6 parsing slice).
- **Retention cascade still unbuilt** — the 30-report cascade owns Markdown + HTML + metadata + the vector summary row; `vector_memory::delete_report_summary` is its ready hook.
- **Tuning deferred together** — brancher thresholds/keywords; memory `MEMORY_TOP_K=5`, no similarity floor, and query composition (two pure functions in `vector_memory.rs`).
- **Optional GUI/live run** — would now show two "Memory embedding" rows under Research (populated store) plus the persist-step row; ~40 FMP calls + one generation.
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Plan the durable-learnings slice** (`/metis-plan-task`) — grow `MainAgentOutput` (and the model envelope + schema in `model_agent.rs`) so the main agent emits durable learnings, persisted best-effort as `learning` rows in the Step-17 persist block; retrieval already surfaces them. Alternatives if priorities shift: the retention cascade (hook ready) or Step-6 inbox parsing (unblocks RouterInput 7/7).
