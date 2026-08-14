# Current session handoff

## What happened

The business-logic discussion landed the **tunnel-vision ruling** (2026-08-14):
Portfolio Analysis stops comparing holdings — the whole Step-7 construction
stage (7a aggregates + 7b synthesis, ~6,400 net lines) was removed, every
action is now authored by a new **per-holding action call** (finished verdict +
holding evidence + investor profile → rung + one-line rationale; the profile's
only entry point, 6f stays profile-blind), and whole-book reasoning is deferred
to a future fourth job, the **portfolio planner**. `PROMPT_VERSION` is
`portfolio-v9`. Follow-on rulings: the ledger's target-weight range and the
`portfolio-weight` series are retired (persisted conditions decode but are
skipped whole — never unevaluable), a one-time pre-v9 migration force-include
is enforced in code, and any future action numeric must be holding-based (share
counts — recorded, unbuilt). Review: internal approve-with-nits (fixed) + three
Codex rounds to no-High-remaining; all gates green (1,034 backend tests, clippy
clean, 46+238 frontend). The full record — nine rulings, build inventory,
per-finding dispositions, standing pushbacks — is
`docs/verification/2026-08-14-tunnel-vision-slice.md`.

## Current state

The slice was committed and pushed at session close (this handoff rides with
it). Legacy construction-era rows keep their `constructed`-marker exclusion
machinery; old runs' retired panels are display-only losses. Three pushbacks
stand on recorded grounds (rationale enforcement prompt-only; `Portfolio.jsx`
not hand-edited — deviation noted in both design READMEs; `.metis` alignment
user-run). Not yet aligned: BUILD/INDEX are still construction-era — the
concrete edit list is in the record's §Disposition. Still pending elsewhere:
prod residue cleanup; digest compression and `NUM_PREDICT_*` calibration behind
a produced-book run.

## Open questions

- **Share-based action sizing** ("trim from X to Y shares") — ruled as the only
  legal action numeric, unbuilt; build when wanted, nothing blocks on it.
- **`logic-flow-docs/portfolio-analysis-logic-flow.md`** carries pre-existing
  v7-era drift (retired conviction-raise machinery still described) — needs its
  own reconciliation pass someday.
- **Live-evidence caveat** (carried) — the sector-P/E walk-back's holiday
  warrant still rests on the 2026-07-16 verification, not re-probed.

## Where to start

Work the tunnel-vision record's queue —
`docs/verification/2026-08-14-tunnel-vision-slice.md` §Disposition. First the
two user-run alignment edits: BUILD (retire construction bullets, record the
tunnel-vision contract + planner, reword the `hard_forensic_bar` seam, retire
the cash-residual item) and INDEX (re-point the §Step-7b / roll-up-and-
construction rows, add the record's row). Then revise
`docs/verification/big-run-watch-set.md` for the v9 shape. **Big-run attempt 3
follows** — its first run is structurally full (the migration gate enforces
it); keep the thought logs and the standing FMP quota / 429-ladder watches.
