# Current session handoff

## What happened

**Two more P1 minors from the 2026-08-24 large-scale review resolved**, one
commit each on `main` through the plan → implement → review → Codex → commit
loop. Tech pre-flag benchmark coverage (`bb16861`): both window endpoints are
now the holding's own sessions, read on the benchmark through an exact-session
`close_on` (a duplicated date takes its last row — the policy
`latest_on_or_before` already applies, off the Codex round); the prior-endpoint
alignment was folded in under the one-seam exception (user-ruled), recorded as
its own sibling bullet. Pre-profit backfill (`e84c1aa`): the obligation counts
per guided identity the periods holding both a guidance bound and an actual —
role-based pairing, deliberately *before* the miss rule's polarity /
finite-bound guards (those bound which pairs can miss, not whether history
exists) — under `BACKFILL_MIN_COMPARABLE_PERIODS = MISS_WINDOW_PERIODS`.
Codex pressed twice for a stamp bump and won on the merits:
`PRE_PROFIT_PARAMETER_VERSION` is **`pre-profit-v2`**. The criterion applied,
worth carrying: **bump when a fix changes what a stamped record means**, since
the stamp is what the resume gate refuses on and what attribution reads — not
only when a stored numeric changes basis. Carried nits still hold: rustfmt-shape
only the edited hunks (the crate isn't rustfmt-clean), sweep `logic-flow-docs/`
mirrors on numeric changes, read gate output in full.

## Current state

Nothing in flight; `main` at `e84c1aa`, tree clean, pushed. The fix queue
still sits ahead of the run, one finding per slice, severity order:
**5 P1 minors** remain (next: fund momentum band saturation — `fund.rs:1027-1036`
scores `trailing_return` over the ~1,600-day deep history against the stock
path's ±30% band tuned to 180 days at `fund.rs:823`, so funds pin at 0/100;
then expense-ratio `{:.3}` rendering, ledger TTM vocabulary, IV skew sign
convention, FMP statement dates), then the 5 P2, 8 P3, Codex I1–I11, and the
§A4 seed fix. Carried untouched outside the record: `/api/tags` probes on the
600 s backstop; seed passes the whole prior ledger per topic; 6g qualitative
trips un-trip unless re-researched.

## Open questions

- **I12?** The pre-flag's `sessions` count (`engine.rs:3371`) counts holding
  rows, not distinct dates, so a duplicated bar inflates √sessions — pre-existing
  and conservative (can only raise the threshold). Codex seconded recording it;
  not actioned.
- **Stamp criterion, recorded once?** Under the bump-when-meaning-changes
  criterion, the anchor-share fix (`28332e1`) should also have bumped `targets`;
  `35bf8af`'s v4→v5 boundary now conflates two changes. No retro-bump proposed.
- **Watch-set line?** The new pre-flag typed gap lands on `degraded_inputs` as
  `no XLK close on the holding's newest session … (series ends …)` if the
  memoized-benchmark fetch race fires in the big run; no
  `big-run-watch-set.md` line was added.

## Where to start

`/metis-session-start`, then `/metis-plan-task` the next P1 minor — fund
momentum band saturation. Keep the loop per finding (plan → implement → review
→ Codex → commit), mark it resolved in the record, sweep `logic-flow-docs/`
mirrors on numeric changes, and ask of every fix whether it changes what a
stamped record means — bump the stamp if so. Do not launch or propose the big
run — the user names that session.
