# Current session handoff

## What happened

The findings program on
`docs/verification/2026-08-24-portfolio-analysis-large-scale-review.md` began,
one finding at a time through plan → implement → review → Codex rounds →
commit, every finding verified against code before being taken. **C1
resolved** (`64ef432`): the local-model transport deadline is derived per
request from `num_ctx` / `num_predict` against drafted throughput floors
(`DeadlinePolicy`, `local_model.rs`); re-verification narrowed the mechanism —
reqwest's blocking timeout is a header wait plus a per-read bound, so only the
non-streaming 6c research turns and distill faced a true 600 s total. **F3
resolved** (`1a9fd9c`, `portfolio-v13`): the research→ledger
`related_condition_id` channel is live — every claim-emitting 6d prompt renders
the ledger conditions with ids, a tie carries across verbatim re-emission only
(a prior tie never becomes fresh support), the interpretation prompt marks
research-supported conditions off tied input-delta entries, and the stale
"none are available this run" sentence is gone. Both resolutions sit under
their sections in the record with §Disposition status lines. Five Codex
rounds in total. `BUILD.md` was updated with the user's OK: §Standing
constraints gained the transport-deadline rule and names the research tie as
the third live typed channel; §What remains item 1 places the review program
ahead of the big run.

## Current state

Nothing in flight; `main` at `1a9fd9c`, tree clean, `PROMPT_VERSION` =
`portfolio-v13`. Remaining findings in severity order: **F1** (split-adjustment
guard on the quick-check / ledger / narrative price comparisons — outcome
learning's `anchor_close / authoring_spot` bridge is the reference, nothing
else has one), **F2** (outcome-label end bar unbounded by staleness), the
alignment findings A1–A4 (one logic-flow doc pass), the minors, and Codex's
I1–I9 (still unverified by a Claude session). Of the record's four pre-run
items C1 and F3 are done; F1 and the retry posture remain. Carried untouched
by decision: `/api/tags` probes ride the 600 s backstop; the seed passes the
whole prior ledger to every topic (doc↔code drift vs `portfolio-workflow.md`
§Step 6c); 6g honors qualitative trips on fresh claims only, so a trip
un-trips next run unless re-researched.

## Open questions

- **Does the big run wait** on F1 and the retry posture, or run with them
  recorded in the watch set?
- **Retry posture** — bounded retry-once on local-model calls vs the hard
  posture (C1 no longer multiplies it).
- **One-month band** — unscaled daily vol × 2 marked "v1 mechanics":
  deliberate retention or √t scaling?
- **Fix grouping** for the rest — keep one-at-a-time, or batch the minors and
  A1–A4.
- Carried: runtime auto-start/spin-down; the 6e supersede leg structurally
  dead; channel promotion criteria open; research budgets calibrate on the run.

## Where to start

`/metis-session-start`, then `/metis-plan-task F1`. Outcome learning's bridge
is the reference implementation; the plan must cover the quick check
(`quick_check.rs` has no split handling at all), ledger evaluation,
`engine::reanchor_scenarios`, and `narrative_vs_reality`'s `prior_spot` leg,
and settle where the split factor comes from (FMP dated-EOD is retroactively
adjusted, so stored authoring-time values need the bridge). Keep the loop:
plan → implement → review → Codex → commit, and mark the finding in the record.
