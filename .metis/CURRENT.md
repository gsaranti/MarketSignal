# Current session handoff

## What happened

The attempt-2 analysis plan executed in full, and **every finding was fixed or
ruled the same session**, ending in commit `58ed003` (pushed, direct to main).
Root cause of the 7b fail-hard: the construction prompt never stated the
divergence-cause vocabulary (the model invented causes; the grammar coerced
them), the three-cause vocabulary structurally could not express down-side
divergences on small un-clustered positions, and the repair offered no legal
exit. Fixes, all ruled on recommendation: **portfolio-v8** — vocabulary stated
in-prompt with checkability semantics + null-cause escape hatch, uncaused
divergences annotate as authored, post-repair uncheckable causes strip, new
sell-side `cash-raised` cause; **targets-v4** — anchor-multiple sanity bound
(kills RKT's +1503% / LCID's +560% artifacts) and a triple-gated trough clamp
release (fixes GM's −79%); interpretation-prompt tightenings (NEW disarm, unit
labels, conviction type, risk polarity); ARKF relabeled "equity fund below the
US-exposure guard" (guard pinned); **no-A ruled honest** — the sector
normalization slice retired for the letter distribution; quality-score
harshness stays reserved. Review: internal approve-with-nits + three Codex
rounds to convergence, then Codex approval. The full evidence and rulings live
in `docs/verification/2026-08-13-big-run-attempt-2.md`.

## Current state

Tree clean, everything pushed. Attribution can no longer fail a book —
persisting arithmetic incoherence is construction's one remaining failure mode,
so attempt 3's expected outcome is a constructed book with the new
`anchor_bounded` / `clamp_released` stamps and divergence annotations readable
on the run surfaces. Not yet aligned (user-run edits outside this handoff):
BUILD §Deferred still lists the now-retired sector-normalization slice, and
INDEX lacks the attempt-2 record's row. Still pending elsewhere: prod residue
cleanup (3 local-model settings + failed `job_runs` id 11, prod-only session);
digest compression and `NUM_PREDICT_*` calibration stay behind a produced-book
run.

## Open questions

- **Portfolio-job business logic** — the user flagged possible business-logic
  changes to discuss before anything else runs; scope unknown until that
  conversation.
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant still
  rests on the 2026-07-16 verification, not re-probed.

## Where to start

**Open with the user's business-logic discussion** — they want to talk through
the portfolio job's logic and possible changes; do not schedule big-run
attempt 3 until that lands. Alongside it, the two quick alignment edits:
retire the sector-normalization slice from BUILD §Deferred by decision, and
add the INDEX §Verification records row for
`docs/verification/2026-08-13-big-run-attempt-2.md`. Attempt 3 follows once
the discussion resolves.
