# Current session handoff

## What happened

**Step-17 durable-learnings slice shipped** — squash `edf4b32` (PR #26), on `main`. The memory loop is now closed: the main agent emits `durable_learnings` (a **sibling** of `summary` on `MainAgentOutput` — the closed report-summary schema is untouched, and `summary_memory_text` never sees them), grown through the envelope + strict schema on both provider arms (a new test pins schema `properties` ≡ `required`, the strict-mode live-only failure). The persist block writes each as a `learning` row: trimmed, empties dropped, capped at `LEARNINGS_PER_REPORT_CAP = 5` (app-layer bound in `pipeline.rs`, not model-trusted), per-item best-effort, `report_id` as provenance, agent-minted `created_at`. `SYSTEM_PROMPT` teaches the five doc categories, a high bar (most weeks zero or one), and self-containedness (fragments are recalled standalone). **Codex round 1 (P2) fixed**: cancellation is now polled before *every* paid embedding call in persist — a guard on the pre-existing summary embed plus a per-iteration poll in the learning loop — so a cancel during persist spends at most the in-flight call; a cancel that late still records **Successful** (the report exists; `jobs.rs` comment documents this). Reviews: Metis approve (12/12 criteria); Codex found nothing else.

## Current state

On **`main` @ `edf4b32`**, synced with origin, branch deleted, **nothing in flight**. `cargo test` 298 passed / 0 failed / 14 ignored (276 lib + 13 generate_report + 6 + 2 + 1), clippy `--all-targets --all-features` clean, `npm run build` OK (frontend untouched). **No live API spend this session** — stub embedders throughout; live smokes remain deliberately unrun.

## Open questions

- **Learning dedup unbuilt** — a lesson the model re-emits weekly accumulates near-duplicate `learning` rows forever (never deleted). Prompt's high bar mitigates; insert-time similarity check or content hash belongs with the tuning bundle below.
- **Step-4 pull has no audit consumer** — doc says it also steers the Step-5 audit; no audit stage exists, so it lands in routing only (seam ready in `assemble_research_packet`).
- **RouterInput: 6 of 7** — only parsed inbox documents remain (blocked on a Step-6 parsing slice).
- **Retention cascade still unbuilt** — the 30-report cascade owns Markdown + HTML + metadata + the vector summary row; `vector_memory::delete_report_summary` is its ready hook (learning survival by `kind` already tested).
- **Tuning deferred together** — brancher thresholds/keywords; `MEMORY_TOP_K=5`, no similarity floor, query composition; `LEARNINGS_PER_REPORT_CAP=5`.
- **Optional GUI/live run** — would now also show up to 5 learning-embed request rows under the persist step (if the model emits any); ~40 FMP calls + one generation.
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Plan the next slice** (`/metis-plan-task`) — two front-runners now that the memory loop is closed: the **retention cascade** (30-report cascade; `delete_report_summary` hook ready, learning survival tested) or **Step-6 inbox parsing** (unblocks RouterInput 7/7 and the research-documents flow). Either stands alone; pick by priority.
