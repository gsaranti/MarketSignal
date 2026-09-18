# Current session handoff

## What happened

**Three rulings, no code change (2026-09-17, late).**
The open local-model-call findings were consolidated into one work list, `docs/verification/2026-09-17-open-findings.md` — eleven flat-numbered entries (1–2 validator bugs, 3 the claim-date persisted shape, 4–6 the fetch layer, 7–8 telemetry, 9 the sampling A/B, 10 the non-thinking experiments, 11 success measures), the checks attempt 7 reads for every rewritten prompt (`portfolio-v40` through `v44`), the watches and the not-adopted list.
The 2026-09-15 fix list and attempt-6 record carry a Superseded line and are history; a new finding appends to the work list as entry 12 on, never elsewhere.
The fixed-evidence harness is deprecated: the v40 / v41 / v42 live read is cancelled, the next test after the remaining fixes is attempt 7 itself, and the harness is rebuilt after it only if the run shows a need (its code stays in the tree; removal is a separate decision).
BUILD never tracks fixes or adjustments and INDEX never indexes verification records (they are temporary and referenced from this file only): INDEX §Verification records removed, BUILD §What remains item 1 trimmed to the decision, the eleven owed BUILD / INDEX rows cancelled.
BUILD was then restructured to the rule: a compressed spine, the suite decisions with §Seams and §Standing constraints kept whole, §Built collapsed to one paragraph, the changelog section dropped, and every verification-record pointer re-homed to an app doc or removed (zero remain).

## Current state

Committed and pushed on `main`, tree clean; Ollama was found up at session start and left as found.
Debut stamps unchanged: `portfolio-v44` / `checkpoint-v11` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`; portability format 7.
The dev store still holds attempt 6; attempt 7 re-wipes first (drop `web_source_state`, clear `portfolio_runs` and the `portfolio` namespace of `vector_memory`).
Queue, from the work list: before attempt 7 — entries 1 and 2 as one validator slice, then 8, then 4 through 7 each its own plan, entry 11's measures set before launch; entry 3 is re-scoped on attempt 7's evidence; entries 9 and 10 follow attempt 7 on a rebuilt harness if needed.
Every entry rules through the selector before implementation; plan flags are ruled with the code in view.

## Open questions

- Whether the harness code (the offline replay in `fixed_evidence.rs`, the attempt-6 fixtures, the ignored live and prompt-dump tests) stays in the tree now the harness is deprecated.
- Validator-slice flags: which downgrade reason wins when one sentence trips both entries 1 and 2; whether entry 4's failed-URL record is run-scoped memory or rides `web_source_state`.
- Entry 3's stamp (evidence-floor, checkpoint or both), decided after attempt 7's dating read.

## Where to start

Read `docs/verification/2026-09-17-open-findings.md`; it is the only work list.
On the user's word, `/metis-plan-task` the validator slice (entries 1 and 2), every flag and assumption through the selector before implementing.
No harness run and no live read: attempt 7 is the next test, launched only when the user names the session and only after the pre-run entries land — do not propose it.
