# Current session handoff

## What happened

A **large-scale review of the Portfolio Analysis job** in the user's priority
order — financial correctness, mid-run abort risk, logic-flow doc alignment —
via nine parallel reviews with every finding re-verified against code before
recording. The record is
`docs/verification/2026-08-24-portfolio-analysis-large-scale-review.md`:
**1 critical, 7 major, 26 minor**, plus the zero-retry posture named as the
dominant real-world abort risk. Headlines: **C1** the Ollama client's 600s
*total* deadline vs the 65,536-token thinking reservation kills long chains
and the run; **F1** no split-adjustment guard on the quick-check / ledger /
narrative price comparisons (outcome learning has the bridge, nothing else
does); **F2** outcome-label end bar unbounded by staleness; **F3** the
research→ledger `related_condition_id` channel is structurally dead (ids never
rendered) and a stale prompt sentence asserts it closed; **A1–A4** the
logic-flow doc's failure paragraph, both "exact inputs" lists, and the
sub-distillation trigger are wrong. The core spine (grading, targets, ledger
evaluation, outcome scoring, fund path, TTM, netting) recomputed sound and all
twelve safety rules hold. **Codex ran the same review independently** and
appended **I1–I9** (six major, three minor: non-positive quote through the
evidence floor, stale fund P/E print fabricating twelve samples, sign-blind
pre-profit corroboration, no guidance-vintage policy, action call never sees
model-arm targets, model-arm numeric domains unenforced, string-NaN weight →
percentile panic, US-share prompt/engine mismatch, fund sector-P/E adapter
bypassing guards). No code was changed; everything is committed and pushed
(`dbb2fb1`).

## Current state

Nothing in flight; `main` at `dbb2fb1`, tree clean, `PROMPT_VERSION` =
`portfolio-v12`. The queue is now **handle the review findings**, then the
**big confirmation run attempt 3**, then Trade Opportunities. The record's
§Disposition names four items to decide before the run: C1, the retry
posture beside it, F1 (a split in the watch window contaminates the ledger
evidence), and F3 (the run's typed-channel yield watch reads zero by
construction). **Codex's I1–I9 are not yet verified by a Claude session** —
the standing discipline is verify-before-agreeing.

## Open questions

- **Does the big run wait** on the four pre-run items, or run as-is with the
  known gaps recorded in its watch set?
- **Retry posture** — a bounded retry-once on local-model calls (the known
  repo-wide deferred item) vs keeping the hard posture; C1 multiplies it.
- **One-month band** — unscaled daily vol × 2 is marked "v1 mechanics";
  deliberate retention or √t scaling?
- **Fix grouping** — severity-ordered slices vs one sweep; the alignment
  findings are a doc pass either way.
- Carried: runtime auto-start/spin-down (undecided, post-run guided-setup
  extension); the 6e supersede leg structurally dead (no dated consensus
  source); channel promotion criteria deliberately open; research budgets
  calibrate on the run.

## Where to start

Read the record's §Disposition and §Codex independent review additions. Verify
I1–I9 against the code before scheduling any of them. Then plan the fix
slices in severity order — C1 first (a transport budget consistent with the
thinking reservation, or an idle-based read timeout), then F1 / F3 and the
verified Codex majors, the alignment findings as one logic-flow doc edit —
and settle whether attempt 3 waits on the pre-run four.
