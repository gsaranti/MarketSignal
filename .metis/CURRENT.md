# Current session handoff

## What happened

**F2 resolved and shipped** (`587e55c`): outcome-label coverage now binds
the **scored end bar**, not the series tail. One helper
(`outcome::window_end_close`) bounds the last close at or before each
window end to `w_end − COVERAGE_TOLERANCE_DAYS` and requires it strictly
after the entry bar, on the holding and both benchmark legs — retiring
`covers_through` and the silent per-window skip; a failing bound rides the
existing pending → grace → typed-unscorable ladder. A Codex round closed
three follow-ons: the cache-heal judgment weighs **every due window end**
(multi-end `series` seam), the past-grace discriminator reads
disappearance from the series itself (alive past the window end closes
`price-coverage-unscorable`; terminal reserved for a series that actually
stopped — reconciled with `storage.md`'s discriminator sentence and the TO
terminal reservation), and the tolerance constant's comment states the
invariant. Three pre-existing fixtures had silently depended on the bug.
Record: review record §F2 (resolved 2026-08-27); canonical rule at
`portfolio-analysis.md §Outcome learning`. Reviewer verdict
approve-with-nits (nit applied). Deliberately out of scope, recorded open
in §F2: the falsifier lead-time bar-count distortion and `drawdown_over`
under-observation over **mid-window interior** gaps. No `PROMPT_VERSION`
change (`portfolio-v13`).

## Current state

Nothing in flight; `main` at `587e55c`, tree clean, pushed. The pre-run
list stays complete and the watch set current — the **big confirmation
run remains unblocked**. Remaining findings behind the run, severity
order: **A1–A4** (logic-flow doc pass — A1's "no partial work persists"
misstatement still open near line 1056), the priority-1 minors, Codex's
I1–I9 (unverified by a Claude session). Carried untouched: `/api/tags`
probes on the 600 s backstop; seed passes the whole prior ledger per
topic (doc↔code drift vs `portfolio-workflow.md` §Step 6c); 6g
qualitative trips un-trip unless re-researched.

## Open questions

- **When to launch the big run** — nothing blocks it; attempt 3 is the
  queue's next item and is user-launched.
- **Fix grouping** — one-at-a-time vs batching the minors and A1–A4; if
  the accumulator-resume minor is taken, retry events and prompt usage
  should ride `CheckpointAccumulators` together.
- **One-month band** — unscaled daily vol × 2 marked "v1 mechanics":
  deliberate retention or √t scaling?
- **Core-beside-statement render** (6c seed / 6d citation list) — named
  candidate from the F1 rounds; a `PROMPT_VERSION` event needing its own
  decision.
- Carried: runtime auto-start/spin-down; the 6e supersede leg
  structurally dead; channel promotion criteria; research budgets
  calibrate on the run.

## Where to start

`/metis-session-start`, then either launch the **big confirmation run**
(user-run; read `data-health` early per the watch set — fired-retry
events new there, zero the healthy read) or, if not running yet,
`/metis-plan-task A1-A4` (the alignment doc pass; the record's §A
findings name the passages) or pick a priority-1 minor. Keep the loop:
plan → implement → review → Codex → commit, and mark the finding in the
record.
