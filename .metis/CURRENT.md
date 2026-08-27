# Current session handoff

## What happened

**Retry posture settled and shipped** (`3728372`): the user chose building
bounded retry-once over running bare. A transient failure on the required
6c–6f path re-attempts exactly once before the hard posture applies. The
whitelist rides typed `RetryClass` chain markers (never string-matching):
transport connection failures, daemon error statuses, empty completion
bodies (new typed guard, tool-call turns tolerated), schema-parse failures,
broken streams. Never retried: deadline trips, length stops, cancellation,
the blank-rationale guard (2026-08-18 ruling holds), anything unclassified.
One shared gate (`local_model::RetryOnce`): the three streaming stages wrap
call-through-parse whole; research/distill re-attempt via default-closed
`retry_permitted` trait gates. Once per **issued** call; legs compose only
through the parse leg's re-issue — a logical terminal turn is hard-bounded
at four calls (worst case + ceiling regression-tested). Fired retries land
as tracker rows plus data-health `model_retries` events (attention trigger);
a resumed run's read is a floor (shares the prompt-usage accumulator resume
gap — deliberately left grouped with that recorded minor). Two Codex rounds:
a stalled non-2xx streaming error body now keeps its live timeout chain, so
it classifies as the deadline trip it is; compound-turn claim tightened to
its tested shape. Canonical contract: `local-models.md` §The local-model
adapter seam. Watch set gained the fired-retry watch.

## Current state

Nothing in flight; `main` at `3728372`, tree clean, pushed;
`PROMPT_VERSION` still `portfolio-v13` (no prompt change). **The record's
pre-run list is complete** (C1, F3, F1, retry posture — §Disposition says
so) and the watch set already carries its 2026-08-20/24 additions, so the
**big confirmation run is unblocked**. Remaining findings behind the run,
severity order: **F2** (invariant named in the record: bound `end_bar.date`
to `w_end − COVERAGE_TOLERANCE_DAYS`, require `end_bar.date > entry.date`;
`bench_return` same shape), A1–A4 (logic-flow doc pass — the retry pointer
landed at line 1056 but A1's "no partial work persists" misstatement is
still open there), the minors, Codex's I1–I9 (unverified by a Claude
session). Carried untouched: `/api/tags` probes on the 600 s backstop; seed
passes the whole prior ledger per topic (doc↔code drift vs
`portfolio-workflow.md` §Step 6c); 6g qualitative trips un-trip unless
re-researched.

## Open questions

- **When to launch the big run** — nothing blocks it now; attempt 3 is the
  queue's next item and is user-launched.
- **Fix grouping** — one-at-a-time vs batching the minors and A1–A4; if the
  accumulator-resume minor is taken, retry events and prompt usage should
  ride `CheckpointAccumulators` together.
- **One-month band** — unscaled daily vol × 2 marked "v1 mechanics":
  deliberate retention or √t scaling?
- **Core-beside-statement render** (6c seed / 6d citation list) — named
  candidate from the F1 rounds; a `PROMPT_VERSION` event needing its own
  decision.
- Carried: runtime auto-start/spin-down; the 6e supersede leg structurally
  dead; channel promotion criteria; research budgets calibrate on the run.

## Where to start

`/metis-session-start`, then either launch the **big confirmation run**
(user-run; read `data-health` early per the watch set — the fired-retry
events are new on that surface, zero is the healthy read) or, if not
running yet, `/metis-plan-task F2` (the record's §F2 names the invariant;
`outcome.rs` `covers_through` / `close_at_or_before` / `bench_return` are
the surfaces). Keep the loop: plan → implement → review → Codex → commit,
and mark the finding in the record.
