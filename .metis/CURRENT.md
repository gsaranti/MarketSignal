# Current session handoff

## What happened

**Step 11 condensed research packet shipped** — squash `8b29a4b` (PR #19), on `main`. The canonical analyst input now exists: `ResearchPacket` + `build_condensed_packet` in `research_packet.rs`.

Three scoping decisions, **locked with the user — don't relitigate:**
- **App-layer deterministic assembler, not a main-agent model stage** — a *conscious deviation from the docs* (`weekly-report-workflow.md §Step 11` and `agents.md §Main Agent` assign packet-building to the main agent). The user chose this over the alternative I recommended (a trait seam on `MainAgent`, deterministic stub now, model later). Rationale: by Step 11 the upstream funnel has already condensed, so assembly is plumbing, not reasoning, and it keeps the pure-stage spine. **Recorded in `BUILD.md:14`** (and `:15` refreshed).
- **Machinery-first, unwired** — like the executor/brancher; nothing in `generate_report` builds the packet yet.
- **Memory deferred** — LanceDB is entirely unbuilt, so the Step-10 pull is out of scope; the packet carries an empty `memory` placeholder.

Also added `select_branch_policy(deltas)` (→ `DeltaBranchPolicy` vs `NoBranch`) — the only form the "select at the call site" intent could take while the research half is unwired. Reviews: **Metis approve; Codex two Low nits fixed** (stale `research_executor.rs` module header; missing baseline/populated-deltas pass-through test).

## Current state

On **`main` @ `8b29a4b`**, synced with origin, branch deleted, **nothing in flight**. **`cargo test` 225 passed / 10 ignored, clippy `--all-targets --all-features` clean** (frontend untouched). `build_condensed_packet` orders clusters by `relevance` / evidence by `priority`, caps (`MAX_PACKET_CLUSTERS=8`, `MAX_SOURCES_PER_FINDING=5`), passes baseline/deltas through, preserves executor accounting, leaves `memory` empty. `ResearchPacket` is `Serialize`-only (`BaselineDeltas` has no `Deserialize`/`Default`). **Built-but-unwired**, like `DeltaBranchPolicy`/`select_branch_policy` (call-site wiring still deferred).

**Uncommitted:** the `BUILD.md` deviation edits (lines 14–15) — should ride in this `metis session end` commit alongside `CURRENT.md`.

## Open questions

- **Research-half wiring slice (next up)** — thread news → filter → route → execute (via `select_branch_policy` from the run's change view) → `build_condensed_packet` → `MainAgentInput` into `generate_report`. Newly pulls in **live-adapter selection** and the **execution gate/credentials**. The Step-10 memory field stays empty until LanceDB.
- **LanceDB / vector memory entirely unbuilt** — no module exists. Blocks the Step-4 pre-research pull, the Step-10 post-research pull, embeddings (`text-embedding-3-large`), and the 30-report/durable-learning retention. Its own multi-slice effort; the packet's `memory` field and `RouterInput`'s memory input both wait on it.
- **Reduced `RouterInput`** — still 3 of 7 doc inputs (baseline, deltas, clusters).
- **Brancher tuning (deferred)** — thresholds (7% oil / 25bp yields), keyword sets, cadence stance (raw-magnitude vs normalize by `elapsed_days`); revisit once wired/observable. Ships oil+yields only by design.
- **Live smokes** — research routing/phase smoke unrun (Tavily + OpenAI + Anthropic + a cool GDELT IP); `fmp_baseline_smoke` awaits a quota reset. Offline-covered.
- *(carried)* snapshot retention vs. 30-report cascade; tracker live-SSE smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap; Step-7 funnel never run live.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**The research-half wiring slice.** `/metis-plan-task` it: wire news → filter → route → execute (constructing the branch policy via `select_branch_policy` from the run's change view) → `build_condensed_packet` into `generate_report` so the packet reaches `MainAgentInput`. Plan around the gate/credential + live-adapter-selection surface this newly touches, and treat the Step-10 memory pull as a no-op until LanceDB lands (likely its own slice first).
