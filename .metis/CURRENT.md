# Current session handoff

## What happened

**Step 9 bounded research executor shipped to `main`** — squash `bd3ffb8` (PR #16). New `research_executor.rs`: `execute_research` walks a Step-8 `ResearchPlan`'s topics by priority, issues each query through a `SearchBackend` (`StubSearchBackend` offline + a live `impl` on `TavilyNewsSource` reusing its `/search`), and returns `ResearchEvidence { items, requests_made, stopped_reason }`. The three Step-9 bounds live in the executor, not any model: **≤50 requests**, **≤30 min**, **depth ≤2** — all polled at request boundaries exactly like cancellation, with one progress row per request and fail-soft on a failed search. Tavily's inherent helper was renamed `search` → `run_search` to free the trait name.

**Load-bearing decisions (don't relitigate):**
- **Synchronous executor, no `tokio`** — the 30-min budget is an injectable `Clock` elapsed-check at each boundary (≤50 sequential blocking searches × one backend timeout stays well inside 30 min), consistent with every adapter and the cancellation model. `BUILD.md` was amended to match (it had framed Step 9 as "where the tokio async seam lands").
- **Branching ships as machinery only** — depth-2 *and* the one-follow-up-per-request shape are both executor-enforced: `BranchPolicy::follow_up -> Option<String>` type-enforces "at most one follow-up" (the shape the router's `5×4×depth-2` budget math depends on). `NoBranch` is the wired default; the real follow-up *generator* is deferred.
- **Unwired by design** (Step-7/8 posture) — evidence's consumer is the not-yet-built Step-10 packet.

**Reviews:** Metis task-reviewer **approve**. Codex two **Medium**s: **finding 2** (a `Vec` follow-up let a policy fan out at depth 2) fixed in-branch via `Option<String>`; **finding 1** (the 50-cap / one-row invariant count *logical* requests, not HTTP attempts incl. retries) is the established project-wide convention (baseline adapters identical) — addressed by a `docs/run-tracking.md` clarification, **not** a code divergence.

## Current state

On **`main` @ `bd3ffb8`**, in sync with origin, feature branch deleted (local + remote), **nothing in flight**. Working tree carries only this `CURRENT.md` rewrite. Verified: **`cargo test` 206 lib + integration green, 10 ignored; `cargo clippy --all-targets --all-features` clean; `npm run build` unaffected** (no frontend change). The executor is built and tested but unwired from `generate_report`.

## Open questions

- **Deferred research brancher** — the follow-up generator (a model call vs. deterministic rules keyed off the baseline change view) is unbuilt; `NoBranch` is wired, so the executor does no dynamic branching yet. *New this session.*
- **Step 10 condensed packet** — now the first real consumer of `ResearchEvidence`; until it lands the gather→filter→route→execute chain has nowhere to feed and the whole research half stays unwired.
- **Reduced `RouterInput`** — still 3 of Step-8's 7 doc inputs (baseline, deltas, clusters); recent-report context / vector memory / parsed inbox / upcoming events join later. The executor inherits the same gap.
- **Live smokes** — research routing/phase smoke unrun (needs Tavily + OpenAI + Anthropic + a cool GDELT IP); `fmp_baseline_smoke` deferred to a quota reset. Offline-covered, so confirmation not a gap.
- *(carried)* snapshot retention vs. 30-report cascade; tracker live-SSE smoke unrun; `COVERAGE_FLOOR=0.6` not final; slice (B) degraded-past-report reader signal; wiremock / in-loop offline gap; Step-7 funnel never run live.
- *(low / parked)* FRED freshness seasonal tuning; filter-prompt snippets; step-5 auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Step 10: the condensed research packet** via `/metis-plan-task` — the first consumer of `ResearchEvidence`, and where the gather → filter → route → execute chain finally threads onto `MainAgentInput` and into the report. Alternative: settle the **deferred brancher** decision (model vs. deterministic rules) before Step 10, or run a quick-win live smoke once keys/quota allow.
