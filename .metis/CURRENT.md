# Current session handoff

## What happened

**The placement ruling landed (2026-08-19).** The user ruled that Trade
Opportunities' tier, horizon, and `business_runway` become **two-arm judgment
fields**: the model authors its own, its **tier × horizon places the matrix
card**, and the engine's rule-derived values persist beside it as the baseline.
The user explicitly chose **shared engine gate legs** over full symmetry — the
tier-scaled hurdle, haircut, and H read the engine on both arms, so the model
never sets its own admission bar. Mechanical consequences carried through: the
advisory-view mechanism, the cheap-pass re-placement leg, and the
provisional-collapse machinery all retired (collapses final at Step 6;
placement frozen between deep passes); placement is authored unanchored (engine
tier/horizon held out of the 5g prompt, mirroring the stand-in holdout); model
runway shape = positive years or `unknown`, unbounded above; only entry-vintage
bands are outcome-scored — tier/horizon/runway recorded unscored; both arms'
reads freeze into episode snapshots. Swept in one pass:
`logic-flow-docs/trade-opportunities-logic-flow.md` plus five canonical docs
(trade-opportunities, workflow, storage, interface, local-models). Five Codex
rounds (5→3→1→1→1 findings) to approval; every finding verified first; one
pushback sustained (Codex claimed the gate ruling was never made — it was, via
the option dialog). BUILD + INDEX absorbed on user instruction.

## Current state

Nothing in flight. Docs-only session — no code touched, no gates run (none
needed). Committed and pushed at session end. The eight TO design gaps stay
deliberately marked inline in the logic-flow doc as "not yet drafted" — the TO
implementation plan's to sweep, not open work.

## Open questions

- Should placement divergence join the run-level pooled divergence *rates*
  (band + conviction today)? Per-pick tier/horizon/runway divergences are
  recorded; a third pooled rate was offered and not yet ruled on.

## Where to start

**Pick the next initiative — nothing is queued.** The natural next is still the
**Portfolio completion block** (`BUILD.md` §What remains item 1 — run-evidence
slice first), the queue's head since 2026-08-14; both logic-flow docs are
grounded and the TO placement contract is settled, so no doc reason remains to
defer it. Let the user choose.
