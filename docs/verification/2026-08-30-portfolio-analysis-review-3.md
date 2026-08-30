# Portfolio Analysis review 3 — blind sweep after the Review 2 remediation

A third independent correctness pass over the Portfolio Analysis job, run
after the Review 2 findings drove another round of code and doc fixes.
This pass was **blind**: the two prior review records
(`2026-08-30-portfolio-analysis-review-2.md` and
`2026-08-24-portfolio-analysis-large-scale-review.md`) were not read before
or during it, so the read is fresh rather than a re-verification of a known
list.

## Result

**No findings — neither minor nor non-minor.**
Every financial equation and prompt checked is correct, every panic-prone
site in the reviewed code is guarded, and the code matches
`logic-flow-docs/portfolio-analysis-logic-flow.md` everywhere the two were
compared.
Because there are no non-minor findings, the Phase 2 cross-check against the
prior reviews does not apply.

## Scope and method

The review ran in the standing priority order.

- **Priority 1 — financial correctness.**
  Every financial function was read directly against the logic-flow doc's
  stated equations, and every prompt that renders or describes a financial
  quantity was read for mischaracterization.
- **Priority 2 — run-killing bugs.**
  A concrete-crash sweep (reachable `unwrap`/`expect`, out-of-bounds index or
  slice, division or remainder by zero, unsigned underflow, hard-erroring
  model/provider deserialization) covered the plumbing-heavy modules; only
  crashes reachable from real feed, model, parse, or I/O data counted, never
  speculative chains.
- **Priority 3 — doc/code alignment.**
  The logic-flow doc was treated as the human-readable model, not a
  line-by-line mirror; drift was judged against behavior and the constants
  the doc cites by name.

## Priority 1 — financial correctness (clean)

Read and verified against the doc's equations:

- **Grade path** (`engine.rs`) — the `scale` primitive, the quality /
  valuation / risk / momentum sub-scores and their bands, the ex-momentum
  weighted roll-up, the letter cutoffs (A ≥ 85, B ≥ 70, C ≥ 55, D ≥ 40), the
  neutral-50 imputation with the two-real-sub-score floor and the
  low-confidence marker, and the non-positive-P/E and negative-equity
  off-scale guards.
- **P/E, P/S, P/B derivation** (`dossier.rs`) — market cap over TTM net
  income (signed, so a loss-maker keeps the negative read the valuation guard
  needs), over TTM revenue, and over equity, each with the correct
  denominator guard.
- **Scenario targets** (`engine.rs`) — the driver ladder (forward EPS, then
  forward revenue per share) with the growth clamp and half-published-spread
  flat-hold, the dated anchor join on the filing date, the inverse
  spread-percentile multiples (bear = P75 spread = cheapest multiple) with the
  degenerate-denominator raw fallback, the raw-percentile and current-multiple
  fallbacks, the trough-clamp release conjunction, the twelve-month prices and
  dividend-inclusive total returns, the one-month prorated base with the
  √t-scaled 2σ band, and the volatility-scaled dispersion floor.
- **Risk tier and hurdle** — the High / Low / else-Medium rule with the
  missing-input stance, the negative-equity leverage guard mirrored from the
  risk sub-score, the tier-premium hurdle, the three-state dead-money read,
  the base-case new-money point test, and the feasible-action set with the
  overlay and hard-forensic bars.
- **Conviction-layer reads** — implied expectations inverted against the one
  shared multiple derivation, narrative-vs-reality (the fixed-period consensus
  revision pair holding prior weights constant, the thin-coverage annualized
  fallback, the 5% expansion floor and 1.5× hype ratio, the overflow-to-hype
  handling), the technology-event pre-flag's √-sessions threshold, and the
  options put/call and IV-skew signal.
- **Ledger evaluation** — `resolve_series`, the observation-identity keys, and
  the statement-basis and equity-source continuity re-stamping that types a
  measurement-basis flip unevaluable rather than manufacturing a crossing.
- **Fund path** (`fund.rs`) — the exchange-blended sector earnings yields, the
  covered-weight-renormalized composite yield over absolute coverage, the
  ≥ 70% coverage guard, the constant-mix history samples admitting only
  in-quarter prints, the vs-own-history valuation percentile, the fund risk
  legs with the neutral-imputed quality axis, the flat-driver fund target, and
  the market-price-only NAV premium.
- **Pre-profit overlay** (`pre_profit.rs`) — liquid resources, TTM cash burn,
  runway, capex intensity over period-aligned windows, split-adjusted YoY
  dilution, the two-quarter gross-margin progression, the eligibility arms, the
  financing bands, economics deterioration, material dilution, the
  guidance-attainment miss ratios and material / repeated states, the
  conjunctive severe-deterioration rule, the derived consequences, and the
  min-only conviction clamp.
- **Outcome learning** (`outcome.rs`) — the Winkler interval score, the
  price-only and dividend-inclusive returns with the ex-date window bound, the
  running-peak drawdown, the price-only benchmark spreads, the split-bridged
  bear-line lead times, the net-alignment table, the target-band calibration
  in return space split by parameter version, and the per-symbol-then-across
  cohort means.
- **Quick check** (`quick_check.rs`) — the stored-basis re-anchor with the
  fresh price and DGS10, the filing-cadence dividend conversion, the
  hurdle-newly-fails change detection, and the frozen bear–bull band-relation
  change on a shared basis.
- **Two-arm assembly** (`pipeline.rs`, `mod.rs`) — the engine stand-in arm's
  outlook / conviction-degradation-count / action rung and its
  strictest-binds ceiling merge, the model-arm domain validation (sub-scores
  0–100, target legs finite and positive, inverted band admitted as a flag),
  and the model-arm letter derived from the model's own sub-scores through the
  shared cutoffs.
- **Prompts** (`pipeline.rs`, `research.rs`) — the interpretation and action
  system prompts (higher-is-better axes, momentum outside the letter,
  two-arm framing, tunnel vision, only `fails` is dead money, tax as a flag
  never the mover, the profile framing the action alone), and every rendered
  section (engine grade / sub-scores / tier / hurdle / metrics / targets,
  implied expectations, implied moves, narrative, options overlay, short
  interest, CFTC positioning, CBOE backdrop, forensic filings with the correct
  8-K item numbers, pre-profit overlay, NAV premium) — all describe the
  underlying computation faithfully, and the research prompt keeps its
  fetched-text-is-data injection guard.

## Priority 2 — run-killing bugs (clean)

A concrete-crash sweep covered `pipeline.rs`, `job.rs`, `store.rs`,
`research.rs`, `distill.rs`, `listing.rs`, `diff.rs`, and `dossier.rs`, with
the math modules read directly during Priority 1.
No reachable panic or hard-erroring deserialization was found.
Every fixed-size-array index, every `unwrap`/`expect`, and every slice range
in the reviewed non-test code is guarded by a preceding check or a fixed
length; model and provider JSON is either fully lenient or schema-enforced to
match its Rust struct; and the whole run is wrapped in a panic-containment
seam that fails the run rather than aborting the process.
The pervasive `finite(...)` filtering means an overflowed intermediate reads
as a gap rather than a persisted non-finite value.

## Priority 3 — doc/code alignment (clean)

The code matches `logic-flow-docs/portfolio-analysis-logic-flow.md` everywhere
compared: the grade weights and cutoffs, the tier bands and premiums, the
hurdle three-state and new-money test, the one-month band and dispersion
floor, the pre-profit thresholds and states, the fund coverage and history
guards, the outcome windows and scoring rules, and the named constants the doc
cites — `OVERFLOW_THRESHOLD` (0.6), `CHARS_PER_TOKEN` (3.0), `NUM_CTX_DISTILL`
(32,768), `NUM_CTX_INTERPRET` (131,072), `SUB_DISTILLATION_CAP` (4), and
`NARRATIVE_MIN_ELAPSED_DAYS` (7) — all read as documented.

## Phase 2

Not applicable: this pass produced no non-minor findings, so there is nothing
to cross-check against the two prior reviews.
