# Current session handoff

## What happened

**I20 landed** — `0fe8ee0`, pushed: the admission stamp on accepted
pre-profit observation rows, the review record's last open finding. Ruled at
its plan (every recommendation adopted): the axis is the trail's shape —
`store::CHECKPOINT_FORMAT_VERSION` → `checkpoint-v3` — with `portfolio-v23`
and `pre-profit-v3` staying (no schema, prompt or computation change; the
stamp is app-written). Two types: `ObservationCandidate` (the eleven model
fields; the 6d wire row and the pre-admission candidate) and
`PreProfitObservation` (+ `admitted_under`, no serde default), built only by
`ObservationCandidate::admit` at acceptance as `PROMPT_VERSION`;
`RejectedObservation` holds the candidate. The stamp sits outside the dedup
key (a re-offer is a duplicate, the first admission stands), renders
nowhere, and is canonical at `portfolio-workflow.md` §Step 6e. One reviewer
round (approve-with-nits, five folded, the `dedup_key_of` eight-argument
note declined) and three Codex rounds — round 1's low ("rebuilt each run and
never carried" overclaimed: a selective run's unselected holding carries its
prior audit whole, rejected list included) closed as wording, rounds 2–3
approving. **The 2026-08-24 review's finding count is zero.** This
session-end rewrote BUILD §What remains item 1 (nothing ahead of the run),
added the never-re-filter sentence to §Standing constraints' observation
bullet, and gave INDEX the admission-stamp row.

## Current state

Nothing in flight; `main` at `0fe8ee0` plus this handoff, tree otherwise
clean. **The queue ahead of the big confirmation run is empty** — the run
waits only on the user naming its session at its start; never propose it.
It starts from a wiped store: every holding is a debut, every read against
a prior is a run-2 watch, a second run only on the user's decision after
run 1's result. The watch set (`docs/verification/big-run-watch-set.md`)
stamps `portfolio-v23` / `checkpoint-v3`; read `data-health` early. Carried
untouched: the cloud `run_job` seam; negative composite yield; `progress.rs`
poisonable locks; `ok` tracker row's dropped-count; TO logic-flow :397; the
600 s `/api/tags` backstop; seed passes the whole prior ledger; 6g
qualitative trips un-trip; an IPv6-loopback wire test; the audit's sources
line not naming the equity source; the unreconciled-delete fail-soft
sentence homed in §Starting parameters rather than §Failure posture.

## Open questions

- A second run after run 1 — the user decides on run 1's result; the watch
  set's run-2 lines (the admission stamp's first attributable read among
  them) wait on that.

## Where to start

`/metis-session-start`, then take the user's direction: the big run is
launched only in a session the user opens by naming it — do not propose or
prepare it unprompted. If named, the checklist is
`docs/verification/big-run-watch-set.md` and the run's dated record follows
under `docs/verification/`. Otherwise the next build work is Trade
Opportunities (BUILD §What remains item 2), which waits behind the run.
