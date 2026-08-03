# Current session handoff

## What happened

**The grade-band shadow-tune slice SHIPPED** (`3a1e2df`, pushed; evidence record `docs/verification/2026-08-03-grade-band-shadow-tune.md`), in the locked order: the sub-score formulas were **certified exact** against run `3b21ae85`'s persisted audits *before* any retune (0 derivation / 0 roll-up mismatches; boundary/imputation/negative-P/E contracts newly pinned), then the statement gaps closed, then the bands moved.
Two load-bearing discoveries: **F5 was understated** — `total_debt` and `revenue_prior` had *no source at all* (risk = volatility-alone for the whole book, SEC-annual margins produced artifacts like MA quality 8.3) — and **the compression was genuinely in the bands** (clean refreshed inputs still gave 0 A/B under v1).
Landed: the **TTM statement basis** (one basis per holding, four-quarter sums, SEC annual the wholesale fallback), the **balance-sheet leg** (one light quarterly call per stock), SEC same-concept prior-year revenue (latest-filed row wins), the **fund SEC skip**, and — after a user-approved 93-call probe refreshed the calibration surface — the user-picked **recentered-growth bands as `grade-v2`** with a negative-D/E → 0 guard and a `grade_parameter_version` stamp on every audit (certification harness now version-gated).
Weights and A–F cutoffs deliberately untouched (reserved for the sector-aware normalization slice).
Two review rounds to convergence: internal approve-with-nits (fixed), then **Codex changes-requested — all five findings confirmed and fixed with pins**, including a latest-filed SEC dedup regression the slice itself had introduced.

## Current state

Tree clean, `3a1e2df` pushed.
Block queue advances to **interpretation-prompt adjustments** (F6 / follow-up #6 of the 2026-07-31 record): target provenance in the prompt, dead-money tilt softened to a weighed input, conviction defined, house-view scoped — **plus the new finding from this slice: surface `grade_parameter_version` to the prompt**, so the first post-tune run's what-changed doesn't mislabel engine-driven letter moves as external/self-correction.
Then the depth slices (thesis ledger, quick check, selective re-analysis, pre-profit overlay, outcome learning, 7b construction); the two UI micro-slices slot anywhere as breathers.
`BUILD.md §What remains` still predates the locked block — catch-up pass when convenient (this slice adds to that debt: grade-v2 / TTM basis / version stamp are uncaptured there and in INDEX).

## Open questions

- **Stacked on the single big confirmation run:** grade-v2 letter distribution on the live book (first A/B letters; does ordering hold), TTM-basis adoption + balance-sheet leg live behavior, fund-SEC-skip noise reduction, 128 K runner stability, distill speed, reasoning panes, fails → indeterminate action distribution, fund carry-path floor, data-health render, and whether Stooq's PoW gate is permanent (rung-order slice + FMP re-homing is the contingent follow-up).
- **No A letters under grade-v2** (META 84.0 grazes the ≥ 85 cutoff) — cutoffs stay reserved; revisit with the normalization slice or the big run's evidence.
- **Carried unchanged:** reasoning-pane DOM weight; encrypted portability round-trip; step-17 embedding-failure watch; long/cold-start 600 s stress; local-suite scorecard display; dev-store calibration-run residue (deliberate); Keychain fail-soft candidate; stage-and-swap import hardening; chain both-maps invariant; four-part verdict + bidirectional-conviction bound; §1 open drafts; fraud-producer posture; fund-slice drafted constants.

## Where to start

`/metis-plan-task` for the **interpretation-prompt adjustments slice**: read `docs/verification/2026-07-31-first-live-portfolio-run.md` §F6 and follow-up #6 first — the four settled edits plus this session's `grade_parameter_version`-in-prompt finding.
The exported run JSON and refreshed-metrics JSON were session-scratchpad-only; re-export from the dev store if a harness replay is needed.
