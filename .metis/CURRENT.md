# Current session handoff

## What happened

**The post-live-run plan was locked (2026-08-02, four user-confirmed choices):** no further live Portfolio runs until the full pre-test block is built — the calibration tier (grade-band shadow-tune → interpretation-prompt adjustments), the two UI micro-slices, AND every designed depth slice (thesis ledger, quick check, selective re-analysis, pre-profit overlay, outcome learning, 7b construction).
Excluded from the block: the live research loop AND the held-name research refresh lane (it depends on the loop, so it rides with it).
The next live run is the single big confirmation test after the block, banking ALL stacked runtime confirmations; Trade Opportunities moves behind the whole block.
The fund carry-path dispersion floor veto was declined (kept as implemented; a carve-out stays possible after live observation), and the `pace()` mutex nit closed as accepted.
**The FMP light-EOD adjustment-basis desk probe ran and CLOSED its open question** (committed `760d271`): `light` `price` verified **split-adjusted + dividend-unadjusted — Stooq's exact basis** — via FMP's own `non-split-adjusted` / `dividend-adjusted` variants (NVDA 10:1 window; MO cumulative-payout window), so the fallback rung never mixes bases; no code change (evidence: `docs/verification/2026-08-02-fmp-light-eod-adjustment-basis.md`).
Incidental: Stooq now answers all non-JS clients (browser headers included) with a **JS proof-of-work interstitial** — a second 200-HTML body distinct from the daily-hits notice; the parse seam's generic any-HTML classification already trips the breaker → FMP rung, so behavior is unchanged (the daily-hits label may misattribute cause — cosmetic, left unfixed).
**User decision: KEEP Stooq as primary rung** (zero-cost self-healing hedge vs FMP plan/cap risk); revisit only with the big run's data-health evidence.

## Current state

Tree clean; `760d271` committed to main but **not pushed** (user asked commit only).
Block queue, in order: **grade-band shadow-tune** (F4/F5 — opens by certifying the sub-score formulas against spec over run `3b21ae85`'s persisted audits, then closing the FMP statement-field gaps, before touching band constants; that run is a targets-v2-basis dataset, so target comparisons cross the version boundary by design), then **interpretation-prompt adjustments** (target provenance in the prompt, tilt weighting, conviction definition, house-view scoping), then the depth slices; the two UI micro-slices slot anywhere as breathers.
`BUILD.md §What remains` still reads "calibration tier first, then Trade Opportunities" — it predates the locked block and needs a catch-up pass when convenient.

## Open questions

- **Runtime confirmations now all banked on the single big confirmation run at the block's end** (no interim live runs): 128 K runner stability, distill speed, reasoning panes; the fails → indeterminate action distribution at scale; the fund carry-path floor reading sensibly; the data-health line rendering — plus **whether Stooq's PoW gate is permanent** (the data-health deep-history line answers it; if Stooq still serves nothing, a small rung-order slice + re-homing the benchmark/futures identities onto FMP becomes the follow-up).
- **Carried unchanged:** reasoning-pane DOM weight (collapse-on-step-finish is the lever); encrypted portability round-trip; step-17 embedding-failure recurrence watch; long/cold-start 600 s stress softened not closed; local-suite scorecard display; dev-store calibration-run residue (deliberate); Keychain fail-soft candidate; stage-and-swap import hardening; chain both-maps invariant; four-part verdict + bidirectional-conviction bound; §1 open drafts; fraud-producer posture; fund-slice drafted constants.

## Where to start

`/metis-plan-task` for the **grade-band shadow-tune slice**: read `docs/verification/2026-07-31-first-live-portfolio-run.md` §F4/§F5 first, and open with the sub-score-formula certification against `docs/portfolio-analysis.md` spec — not the band constants.
Push `760d271` (or fold it into the next push).
