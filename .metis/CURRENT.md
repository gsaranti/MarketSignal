# Current session handoff

## What happened

**Weekly-report workflow reordered 17 → 18 steps** — squash `cb8f61b` (PR #17), on `main`. Two shape changes, both **load-bearing (don't relitigate):**
- **Baseline before audit** — baseline market data moved ahead of the audit (Step 6 → 3); the audit follows (3 → 5) so it grounds prior theses in the measured baseline + change view, not prose. Inbox 5 → 6.
- **Vector memory split into two pulls** — pre-research (Step 4: query from recent context + baseline + change view) *steers* audit + routing; post-research (Step 10: query from research evidence) feeds the packet. The **packet carries only the post-research pull (replace, not merge)**; the pre-research pull is ephemeral. Restores memory as a routing input *in the spec* while adding a research-informed pull.
- Renumber: packet 10 → 11, analysts → 12.., synthesis → 16, save → 17, HTML → 18. **News / Routing / Research kept 7 / 8 / 9.**

**Scope was spec-only.** Confirmed `generate_report` wires only baseline → coverage → agent → persist; every reordered stage is unbuilt, so there was no orchestration code to reorder — the `src/` changes were doc-comments + two `pipeline.rs` coverage-error strings.

**Codex review dispositioned:** finding 1 (audit overclaimed measured deltas across the 2–6 report window — the change view spans only the latest interval) and finding 4 (memory dedup ambiguity) **fixed in spec**; finding 2 (stale CURRENT/SYNTHESIS) **ignored** — point-in-time artifacts, not canonical; finding 3 (src numbering) **done**, and the sweep caught 2 refs Codex missed (Step 17 → 18, Step 5 → 6).

## Current state

On **`main` @ `cb8f61b`**, synced with origin, feature branch deleted (local + remote), **nothing in flight**. Verified before merge: **`cargo test` green, `cargo clippy --all-targets --all-features` clean, `npm run build` pass** (frontend untouched). The 18-step order is now source-of-truth across `docs/` + `.metis/` INDEX & BUILD. **`.metis/SYNTHESIS.md` and `RESOLVED.md` were left** — they still describe the 17-step order; a future `/metis-reconcile` refreshes SYNTHESIS.

## Open questions

- **Research brancher generator (next up)** — the deferred follow-up generator: **deterministic delta-rules** (recommended first cut) vs. **model-backed (Sonnet)** vs. **hybrid**. Wrinkle: `BranchPolicy::follow_up(item, finding)` is *finding-conditioned*, but the doc's "if oil spikes…" triggers are *delta-conditioned* — whichever is chosen needs the change view threaded into the policy at construction (no trait-signature break). `NoBranch` still wired.
- **Step 11 condensed packet** — first real consumer of `ResearchEvidence` (+ the post-research memory pull); until it lands the gather → filter → route → execute chain has nowhere to feed.
- **Pipeline unwired** — nothing beyond baseline → coverage → agent → persist is in `generate_report`; audit / memory / inbox / news / routing / executor all stay standalone modules.
- **Reduced `RouterInput`** — still 3 of 7 doc inputs (baseline, deltas, clusters); the reorder put memory back as a routing input *in the spec*, but the code still lacks it.
- **Live smokes** — research routing/phase smoke unrun (Tavily + OpenAI + Anthropic + a cool GDELT IP); `fmp_baseline_smoke` awaits a quota reset. Offline-covered.
- *(carried)* snapshot retention vs. 30-report cascade; tracker live-SSE smoke unrun; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap; Step-7 funnel never run live.
- *(low / parked)* FRED freshness seasonal tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Settle the research brancher generator** (the thread queued for next session): pick deterministic delta-rules vs. model-backed vs. hybrid, then `/metis-plan-task` it — thread the change view into the chosen `BranchPolicy` at construction. The other major thread is **Step 11: the condensed packet** (first consumer of `ResearchEvidence` + the post-research memory pull), where the research half finally wires onto `MainAgentInput` and into the report.
