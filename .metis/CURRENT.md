# Current session handoff

## What happened

**Research brancher generator settled and shipped** — squash `787b9e2` (PR #18), on `main`. The deferred Step-9 follow-up generator is now `DeltaBranchPolicy`: **deterministic delta-rules** keyed off the baseline change view (chosen over model-backed / hybrid — the unwired stage can't yet measure report-quality gains, and the trait stays the seam for a later model upgrade). Machinery only; `NoBranch` stays the trait default.

Three review rounds (Metis reviewer + Codex ×3) produced **load-bearing refinements — don't relitigate:**
- **Directional gate** — rules fire only on the doc's *direction* ("oil **spikes**" / "yields **rise sharply**" = up-moves). The original plan's `abs()`-magnitude framing was a bug Codex caught: a sharp *decline* is a different thesis and must not emit a rise-flavored follow-up. `TriggerRule` carries a `Direction`; a future crash rule is a one-row `Direction::Down` entry.
- **Emit-once met, not documented around** — each fired rule emits exactly once when a topic matches; rules **colliding on one finding merge into a single combined query** rather than dropping the lower-priority one — preserving the executor's one-follow-up-per-finding invariant (the 50-request budget math depends on the 1:1 shape). An earlier "documented limitation" was a rationalized reduction; the merge is the real fix.
- **Word-boundary keyword match** (tokenized, not substring) so "turmoil" can't trip "oil".

`BUILD.md:15` was hand-updated this session to record the generator shipped.

## Current state

On **`main` @ `787b9e2`**, synced with origin, feature branch deleted (local + remote), **nothing in flight**. Verified before merge: **`cargo test` 217 passed, `cargo clippy --all-targets --all-features` clean** (frontend untouched). `DeltaBranchPolicy` is **built-but-unwired**: nothing selects it over `NoBranch` at the `execute_research` call site yet. Trigger table seeds only **oil** (`DCOILWTICO` / Pct / 7%) and **10y yields** (`DGS10` / Abs / 25bp), both `Direction::Up`. 11 `delta_policy_*` tests cover firing/threshold/direction/word-boundary/single-emit/merge + executor integration.

## Open questions

- **Step 11 condensed packet (next up)** — first real consumer of `ResearchEvidence` (+ the post-research memory pull), **and** the place `DeltaBranchPolicy` is selected over `NoBranch` at the `execute_research` call site (thread the change view into the policy at construction there). Where the research half finally wires onto `MainAgentInput`.
- **Pipeline unwired** — nothing beyond baseline → coverage → agent → persist is in `generate_report`; audit / memory / inbox / news / routing / executor / brancher all stay standalone.
- **Reduced `RouterInput`** — still 3 of 7 doc inputs (baseline, deltas, clusters); memory-as-routing-input is in the spec but not the code.
- **Brancher tuning (deferred)** — thresholds (7% oil / 25bp yields), keyword sets, and the **cadence stance** (raw-magnitude vs normalizing by `elapsed_days`) are all tunable; revisit once the stage is wired and observable. Trigger coverage deliberately ships oil+yields only: geopolitical (news-conditioned), semis-weaken (no price level in `DELTA_GROUPS`), rally-despite-weak-macro (compound) are out of reach for single-series delta rules.
- **Live smokes** — research routing/phase smoke unrun (Tavily + OpenAI + Anthropic + a cool GDELT IP); `fmp_baseline_smoke` awaits a quota reset. Offline-covered.
- *(carried)* snapshot retention vs. 30-report cascade; tracker live-SSE smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap; Step-7 funnel never run live.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Step 11: the condensed packet.** `/metis-plan-task` it — it's the first consumer of `ResearchEvidence` + the post-research memory pull, and it carries the deferred executor wiring: select `DeltaBranchPolicy` over `NoBranch` at the `execute_research` call site, constructing it from the run's change view. This is where the research half (gather → filter → route → execute → branch) finally feeds the report via `MainAgentInput`.
