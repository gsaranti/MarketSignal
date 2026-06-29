# Current session handoff

## What happened

A design conversation that became a docs change. Question: how much do research vs hard
data move grades/verdicts in Portfolio + TO (user's observation: C/D/F-graded stocks often
compound for years). Settled — with Codex concurrence — on a **four-part verdict model**: a
deterministic **grade** (backward/current anchor, hard-data only), a **first-class forward
outlook** surfaced *beside* the grade (so a weak-grade / strong-forward name reads as such,
not buried), **bidirectional conviction**, and the portfolio **action**. The load-bearing
change: research may now **raise** conviction (not only cap it), but **only** via a typed
**`validated_leading_indicator`** — an *engine-unscored* (not in the deterministic
composite) countable/dated leading metric, returned as a `base`/`raise`/`final`
decomposition the **app recomputes**, ≤ one band, app-validated per-candidate (incl.
debuts) — **never** via price/narrative (the **anti-reflexivity / no-double-count
invariant**). **TO price predictions are now user-facing** on the matrix card (engine EOY
~12-mo scenario target + bear/bull range). Iterated across **7 Codex rounds** (typed-field
producer, double-count definition, debut-validation gap, target time-basis, conviction
decomposition, product↔workflow consistency, a nested-bold bug, grammar) — all resolved.

## Current state

On `main` @ `4cc2284` (5 docs, +27/−17), in sync with origin. **Docs-only — no code
touched.** This is *design* for the **not-yet-built** full Portfolio + TO: the four-part
model, the `validated_leading_indicator` field, the base/raise/final decomposition, the
per-candidate raise validation (Step 6g / 5h), and the user-facing TO price prediction are
**specified, not implemented** (the narrow Portfolio slice / shared engine are unchanged).
**`BUILD.md` updated this session** (Portfolio decision-discipline bullet) to record the
four-part model + the bidirectional-conviction invariant. Build order unchanged.

## Open questions

- New **conviction-raise bound** is calibratable — ≤ **one band** lift + the
  engine-unscored leading-indicator gate; validate on the M5 with the other surfaces.
- **Calibration-surface pattern** (carried): engine constants are MVP shadow-tune-not-pin
  values — live-calibrate on the M5.
- §1 **genuinely-open drafts** (dead-money hurdle, feasible-set bounding, thresholds; TO
  risk-tier / horizon / hypothesis-score / quota / gate table) — per-slice starting values.
- Optional **BUILD.md gate-wording micro-tweak** (`:307` → "endpoint + *required* roster
  ids — reasoner + embedder"). Skippable.
- Standing **M5-gated backlog** (web-research provisioning/gating/UI + rendered-retrieval,
  analytical-register restyle live-check, no new Tavily, local-model live quality/FMP-tier).

## Where to start

Begin the **live Schwab OAuth slice** (next in build order; `schwab-integration.md`
audited clean — OAuth loopback, 30-min/7-day tokens, Keychain, positions + option chains).
The four-part verdict model + bidirectional conviction land **later**, when full Portfolio
+ TO are built — at which point the new `validated_leading_indicator` field, the
base/raise/final decomposition, and the per-candidate raise validation must be implemented.
**Check code-vs-doc first** on any Portfolio formula — PR #45 already implements the MVP
engine math.
