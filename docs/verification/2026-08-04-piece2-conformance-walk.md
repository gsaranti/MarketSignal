# Pre-big-run review piece 2 — code-vs-docs conformance walk (2026-08-04)

The second of the three pre-big-run review pieces (piece 1 = the first-live-run findings verification, [2026-07-31-first-live-portfolio-run.md](2026-07-31-first-live-portfolio-run.md); piece 3 = the deterministic value-chain correctness walk, not yet run):
a claim-by-claim walk of the Portfolio Analysis job's documented logic (`portfolio-analysis.md`, `portfolio-workflow.md`, and the storage / interface / data-sources / schwab-integration sections it touches) against the as-built code, flagging every divergence with a proposed verdict.

## Method

Seven parallel conformance passes, each owning a doc scope and its implementing code — (A) gate / holdings / diff / eligibility, (B) dossier / engine / targets / endpoint surface, (C) 6f–6g interpretation + ledger, (D) pre-profit / quick check / selective, (E1) construction 7a/7b, (E2) outcome learning / storage / portability, (F) frontend/interface — with known designed-not-built and deliberately-dormant items excluded up front.
Every surviving finding was re-verified against the code by the orchestrating session before batching (several independently converged across passes); the batch was then user-ruled.
The bulk of the walk **conformed**: the 6g ledger validator property-by-property, the full quick-check contract, the pre-profit machinery's exact constants, the construction solver, the outcome-label machinery, targets-v3, grade-v2, the TTM basis, retention, and portability v1–v3 all matched their docs.

## Dispositions

**39 verified divergences** after deduplication, ruled in three batches:

- **A (5) — code fixes, applied** (`462aaa5`):
  A1 investor profile removed from the 6f intrinsic prompt (input isolation, not instruction — the documented profile-independence invariant now enforced and test-pinned);
  A2 the house-view one-week freshness gate built (`HOUSE_VIEW_MAX_AGE_DAYS`, whole-view drop, typed `DataHealth.house_view_omitted` record);
  A3 a negative netted basis keeps its dollar gain in the sort (exact zero stays undefined — wire-ambiguous with an unreported basis, ruled);
  A4 job-named local gate-block messages with the Settings → Test Connection pointer on the probed path;
  A5 kind-aware tracker labels (back control, scroll-region name, idle headline).
- **C (20) — doc corrections, applied** (`462aaa5` + the Codex rounds `c2d963d`):
  the as-built/designed marker sweep over the unbuilt conviction/positioning layer, Step-5 context loads, street opinions, 6a equity surface, SEC fill-only merge, and the endpoint tables (wired-subset note + per-row markers);
  plus the mechanical staleness fixes — netting/CIK "named prerequisite", quick-check FMP-only pricing, single-payout-leg hurdle, NTM naming, rate-ladder cache rung, flat-driver mechanism, fund-tier liquidity legs, share-basis fallback, chain timing (heading + anchors), pulled-fields (CUSIP / P&L), overlay 7a stand-in narrowing, EoM/EoY, weight-attribution clause, outcome-pass placement (post-7b, own step), episode-snapshot narrowing, learning-row prune truth, pre-profit validation-leg obligation, lean-tag and vintage-stamp qualifiers.
- **B (14) — open design rulings**, batched below; deliberately untouched in code and docs beyond what a marker required, so no ruling is prejudged.

External review: **two Codex rounds to approval** — round 1 confirmed the A/B substance and surfaced five doc residuals (all confirmed and fixed); round 2 caught the trailing-qualifier pattern on the audit-record leads and five stale timing references (fixed); Codex independently endorsed B1–B14 as grounded, valid open decisions.
Verified at the pushed tip: cargo 902 lib + 32 integration / 0 fail, clippy 0, npm build, 40 node + 206 vitest.

## The open B rulings

Each item: the divergence, and the decision it needs.

- **B1 — forward dividends are a raw trailing TTM sum, specials included.**
  `engine.rs` `forward_dividends` + the `dividends` TTM sum take every row in the window; `§Starting parameters` says payout terms are "forward / sustainable, never a raw trailing print".
  A one-off special inside the window inflates `TR` and can flip the hurdle / admission reads.
  *Filter specials (needs a dividend-type flag on the row) vs soften the doc to the as-built TTM basis.*
- **B2 — two evidence-floor arms unenforced: stale prices, conflicting identity.**
  The floor is price-present / ≥2 sub-scores / admissible-driver; the FMP quote carries no as-of date, and no identity-conflict producer exists (ties to B3).
  *Wire a dated staleness check vs mark both arms designed.*
- **B3 — the loop-time listing-resolution guard does not exist.**
  No canonical-FMP-resolution or issuer-identity cross-check; every Schwab `EQUITY` symbol goes straight down the pipeline, so a wrong-issuer FMP mapping would grade the wrong company (sparse data often — not deterministically — abstains via the sub-score floor).
  *Build (the strongest pre-big-run candidate — a wrong mapping is invisible to the run's own checks) vs mark designed and let the big run's evidence decide urgency.*
- **B4 — the 6f prompt renders only a prior-verdict existence flag, never its values.**
  The full prior ledger renders; the prior grade / lean / targets do not, so the model authors `what_changed` without seeing what moved.
  *Render prior values (anchoring risk) vs narrow the doc.*
- **B5 — the vector-continuity lane is absent at both ends.**
  No Step-6a semantic retrieval and no Step-8 per-holding summary embeddings (the only portfolio-namespace write is the matured-learning row); continuity is exclusively the deterministic prior verdict + ledger.
  *Build the lane vs mark designed.*
- **B6 — options machinery diverges from its documented canonical method.**
  No greeks parsed; skew = whole-chain mean-put-IV − mean-call-IV, not the documented matched-tenor 25-delta risk reversal; no liquidity floor / zero-bid exclusion; no chain as-of / staleness rejection.
  *Re-scope the doc to the as-built method vs build toward the spec.*
- **B7 — investor-profile preset shape.**
  No `objective` field (the documented "maximize profit" never reaches a prompt); shipped default `Moderate` vs the documented "medium-to-high"; no read-only Settings surface.
  *Add the field + an Aggressive-leaning mapping + the Settings block vs restate the preset as shipped.*
- **B8 — manual CSV/paste import is wholly absent** though documented in present tense with netting and conflict-warning sub-contracts.
  *Mark designed vs schedule the slice.*
- **B9 — no closed-end-fund leg.**
  `FundStrategyClass` has no CEF variant; `nav_premium` computes for any fund with a NAV and is consumed by no score, prompt, or rule.
  *Mark designed vs build detection + consumption.*
- **B10 — momentum renders as a fourth undifferentiated sub-score tile** beside the letter, visually implying a grade input; docs place it as the market-setup read in the conviction context (the engine correctly keeps it out of the letter).
  *Set the tile apart on the card vs restate the doc.*
- **B11 — final-action cohorts are not stratified by the divergence-from-lean rationale**, and no derived read is sliceable by alignment tag (`lean_divergence` and `alignment` are recorded per episode but never read in `derive_reads`; raw facts persist, so strata stay retroactively computable).
  *Add the strata to `derive_reads` vs an as-built narrowing note.*
- **B12 — construction-prompt digest compression is a covenant, not code.**
  No overrun detection exists; the doc-comment names compression as the sanctioned response.
  The big-run watch already tracks construction-prompt fit in the shared 131k `num_ctx`.
  *Reword to covenant vs build an overrun guard.*
- **B13 — the documented "thesis monitor" card element is unrendered.**
  The typed `MonitorScenario` data rides the payload unused; `types.ts` marks the machine detail display-deferred.
  *Narrow the doc vs schedule the display slice.*
- **B14 — fund quick-refresh retrieval is unconditional** (`etf/info` + both weightings for every fund; the documented equity-only / condition-gated retrieval is evaluation-side filtering only) — two extra FMP calls per non-equity fund per sweep.
  *Gate retrieval vs edit the doc.*

One recorded note requiring no ruling: the construction spine's `hard_forensic_bar` field is producer-dormant **and consumer-unread** (no reader in `feasible_actions`, the digest, or validation) — when the forensic producer lands, its consumer seam needs wiring too.
