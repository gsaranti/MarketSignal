# Current session handoff

## What happened

**One P1 minor from the 2026-08-24 large-scale review resolved** — fund
momentum band saturation — as `e530bd9` on `main`, stamped **`grade-v2.2`**.
The fix itself was small (`analyze_fund` scores momentum through
`engine::momentum_score` over its 180-day `price_history` leg; the deep-history
helper is gone; no short window imputes 50). Four Codex rounds turned the
**stamp's consumers** into the real work, and the lesson carries: the generic
"grade bands recalibrated — letters can move" delta row is a *citable* entry the
what-changed validator accepts, so a bump that doesn't describe itself hands the
model false evidence for a real move. Now `engine::GRADE_PARAMETER_HISTORY`
types each bump per branch, `grade_parameter_change(prior, branch)` folds the
rows after the prior's stamp on the **prior record's own branch** (its persisted
asset class — the fund-path routing key), both consumers render only over a
**priced prior**, and stay silent where the boundary changed nothing on that
branch or the stamp is unrecognized. A pre-stamp fund reads as re-homed, not
recalibrated — git shows every fund letter-bearing line untouched since the
fund path landed (2026-07-16, before the first stamp). The Codex rounds were
folded in under the one-seam exception; the user committed. Carried nits hold:
rustfmt-shape only edited hunks, sweep `logic-flow-docs/` mirrors, read gate
output in full, verify Codex findings against code *and git* before agreeing.

## Current state

Nothing in flight; `main` at `e530bd9`, tree clean, pushed. The fix queue still
sits ahead of the run, one finding per slice, severity order: **4 P1 minors**
remain (next: **expense-ratio `{:.3}` rendering** — the role/risk,
interpretation, and action prompts render expense ratio and drag through
`opt()`'s three-decimal format (`pipeline.rs:2911`, `3513`, `3964`, via
`opt()` at `3772`; the record's older anchors have drifted), so a 0.03% fund
prints `0.000` and the legend's own example `0.0075` is unrepresentable; then
ledger TTM vocabulary, IV skew sign convention, FMP statement dates), then the
5 P2, 8 P3, Codex I1–I11, and the §A4 seed fix. Carried untouched outside the
record: `/api/tags` probes on the 600 s backstop; seed passes the whole prior
ledger per topic; 6g qualitative trips un-trip unless re-researched.

## Open questions

- **I12?** The pre-flag's `sessions` count counts holding rows, not distinct
  dates — pre-existing and conservative; recorded, not actioned.
- **Stamp criterion, recorded once?** The anchor-share fix (`28332e1`) should
  also have bumped `targets`; `35bf8af`'s v4→v5 boundary conflates two changes.
  No retro-bump proposed.
- **Watch-set line for the pre-flag typed gap?** `no XLK close on the holding's
  newest session …` on `degraded_inputs` if the memoized-benchmark race fires;
  no watch line added (the grade-boundary watch line *was* added this session).
- **`Letters` NOTE wording.** `pipeline.rs`'s recalibration NOTE speaks only of
  "the letter"; a future stock-branch sub-score-only bump would need its own
  text. No such bump exists; recorded, not actioned.

## Where to start

`/metis-session-start`, then `/metis-plan-task` the next P1 minor — expense-ratio
`{:.3}` rendering. Keep the loop per finding (plan → implement → review → Codex →
commit), mark it resolved in the record, sweep `logic-flow-docs/` mirrors, and
ask of every fix whether it changes what a stamped record means — if it bumps a
grade stamp, append the `GRADE_PARAMETER_HISTORY` row (a test pins the last row
to the current stamp) and check both consumers' wording. Do not launch or
propose the big run — the user names that session.
