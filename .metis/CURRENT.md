# Current session handoff

## What happened

**Research-half wiring slice shipped** — squash `1af9f21` (PR #20), on `main`. `generate_report` now runs the research half: `pipeline::assemble_research_packet` threads news → dedupe → filter → route → execute (branch policy via `select_branch_policy` from the change view) → `build_condensed_packet`, and the packet reaches the main agent on `MainAgentInput.research` (the real `ModelMainAgent` prompt serializes its news clusters + evidence).

Four scoping decisions, **locked with the user — don't relitigate:**
- **Additive input shape** — `research: Option<ResearchPacket>`; `baseline`/`deltas` stay top-level, so the packet's own copies of them are inert. (Not the packet-as-sole-input refactor BUILD.md's "canonical input" framing could imply.)
- **Fully fail-soft** — every research stage degrades to empty on failure; the run always reaches the agent; only the Step-3 coverage floor gates a run. **Conscious deviation from `weekly-report-workflow.md §Step 9`** (failing model call → job failure), **recorded in `BUILD.md:15`**.
- **Full tracker fidelity** — `with_context` + one request row per real API call on the news/filter/router adapters; `App.vue` routes request rows to their owning step by `group`.
- **News = Tavily + GDELT**; Tavily's gather is now **per-topic fail-soft** (one bad topic no longer discards the rest or GDELT). FMP company-news parked as a follow-on.

Reviews: **Metis approve-with-nits (nits closed); Codex two High findings fixed** — frontend request rows were mis-bucketed under "baseline"; cancellation could still spend the filter + router model calls (now a cancel checkpoint before each).

## Current state

On **`main` @ `1af9f21`**, synced with origin, branch deleted, **nothing in flight**. **`cargo test` 236 passed / 10 ignored, clippy `--all-targets --all-features` clean, `npm run build` OK.** `RunConfig` + `decide_scheduled_run` now carry the Tavily/OpenAI/Anthropic keys; both command seams build the real stages via `live_research_stages`. Two Tavily clients per run (deliberate — the composite owns the gather one by value, the executor gets its own; documented). `iris-codex-last.md` in the repo root is gitignored (the Codex review artifact; not committed).

## Open questions

- **LanceDB / vector memory entirely unbuilt** — no module. Blocks the Step-4 pre-research pull, Step-10 post-research pull, embeddings (`text-embedding-3-large`), and 30-report/durable-learning retention. Its own multi-slice effort; the packet's `memory` field (ships empty) and `RouterInput`'s memory input both wait on it.
- **FMP company-news follow-on** — ticker-specific press tied to movers/earnings. Needs a live free-tier probe → `data-sources.md` amendment → a new `NewsSource` adapter. Parked by decision.
- **Live research smoke unrun** — the just-wired news→filter→route→execute path has never run live (Tavily + OpenAI + Anthropic + a cool GDELT IP); `fmp_baseline_smoke` awaits a quota reset. Offline-covered only.
- **Reduced `RouterInput`** — still 3 of 7 doc inputs (baseline, deltas, clusters).
- **Brancher tuning (deferred)** — thresholds (7% oil / 25bp yields), keyword sets, cadence stance; revisit now that it's wired/observable. Ships oil+yields only by design.
- *(carried)* snapshot retention vs. 30-report cascade; tracker live-SSE smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap; Step-7 funnel never run live.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Validate the just-wired research path live first** — run the research smoke (Tavily + OpenAI + Anthropic + a cool GDELT IP) to confirm news→filter→route→execute→packet works end-to-end and the tracker rows land under the research step; it's offline-only so far. Then pick the next slice: **LanceDB / vector memory** is the largest unblock (memory pulls, embeddings, retention) and likely its own multi-slice effort; **FMP company-news** is the smaller follow-on. `/metis-plan-task` whichever you choose.
