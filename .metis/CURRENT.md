# Current session handoff

## What happened

Designed and shipped (docs-only) the **Archived Opportunities** feature for the
not-yet-built **Trade Opportunities** job. A departed opportunity moves to a
**price-tracked tombstone archive** (last 100): frozen retrospective record + a
live **since-flagged return** (price-only — return vs sector/market + drawdown,
*not* metric-continuation), **no forward prediction shown** (it would read as a
live call). The exit model collapsed to **one trigger, `failed-reevaluation`** —
fired by either the cheap every-run engine-only **upside re-derivation** (Step 7,
off the discovery budget) or a deep Step-5 `invalidated` on re-surfacing; **no
"target met" exit** (`played-out` status **retired**); the upside-exhausted gate
is the **sole archival decision** (judges the post-decay target); fail-soft on
missing data. **Re-evaluation splits by cost**: every live pick gets the cheap
re-derivation each run (freshens quant, can fail), while **deep research re-runs
only on re-surfacing** through discovery — so the model-authored fields (thesis,
conviction, bear case) + the price prediction's **research layer** freeze between
deep passes (the prediction is a hybrid — quant skeleton fresh every run).
`research_forward_assumption` **freshness-decays** (~4wk → structured-only, reset
by a passing deep re-eval). Re-entry is a **fresh** opportunity, **matched by
ticker** (one-state-per-ticker), anti-reflexive, archive never self-promotes. New
fields `became_opportunity_at` / `last_deep_researched_at`. Reviewed clean — metis
task-reviewer (approve) + **two Codex rounds** (decay/archive rule, date anchor,
price-only, Step 10 — all fixed).

## Current state

On `main` @ `88df3e0`, in sync with origin — two commits: `439694d` (archive spec
delta) + `88df3e0` (re-entry match-key). Docs-only across 4 corpus docs
(`trade-opportunities.md`, `trade-opportunities-workflow.md`, `storage.md`,
`interface.md`) + **`BUILD.md`** (the lifecycle decisions folded into the
TO-designed bullet) + **`INDEX.md`** (two concept entries). The archive is
**specified, not implemented** — design for the not-yet-built full Trade
Opportunities, which is *later* in build order. Corpus **stays dev-ready**; build
order unchanged.

## Open questions

- **M5-calibration** (carried): the archive's new constants — **~4-week** freshness
  window, **100**-retention, upside-exhausted threshold — are MVP starting values,
  live-tune on the M5 with every other engine constant.
- **Four-part verdict model + bidirectional-conviction bound** (carried): lands when
  full Portfolio + TO are built — `validated_leading_indicator`, base/raise/final
  decomposition, per-candidate raise validation; ≤ one-band lift is calibratable.
- §1 **genuinely-open drafts** (carried): dead-money hurdle, feasible-set bounding,
  thresholds; TO risk-tier / horizon / hypothesis-score / quota / gate table —
  per-slice starting values.
- Standing **M5-gated backlog** (carried, unchanged): web-research provisioning /
  gating / UI + rendered-retrieval, analytical-register restyle live-check, no new
  Tavily, local-model live quality / FMP-tier.

## Where to start

Begin the **live Schwab OAuth slice** (next in build order; unblocked and unaffected
by the archive work — `schwab-integration.md` audited clean: OAuth loopback,
30-min/7-day tokens, Keychain, positions + option chains). The archive (like the
four-part verdict model) lands later, when full Portfolio + TO are built. **Check
code-vs-doc first** on any Portfolio/TO formula — PR #45 already implements the MVP
engine math.
