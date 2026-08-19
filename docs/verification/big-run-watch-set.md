# Big confirmation run — watch set

The single big confirmation run banks every runtime confirmation the locked pre-test block stacked up.
This file is the checklist it reads against: what to look for, grouped by the surface that produces it.
It is forward-looking, unlike the dated records beside it — those are written after the fact, this one is written before.
Findings go into a dated record once the run completes; this file is then the index of what that record has to answer.
Revised 2026-08-18 to the `portfolio-v9` shape: the construction-stage, lean / divergence, and sizing watches are removed, since that machinery no longer exists.

Nothing here is a defect report.
Each item is a behavior that has only ever been exercised against fixtures, a small live run, or a single symbol, and needs a read at real scale (a 47-position book) before it can be called confirmed.

## Grade, valuation and targets

The `grade-v2` letter distribution, and whether the ordering holds.
No A letters have appeared yet — the closest observed was 84.0 against the ≥ 85 cutoff — so the run decides whether the recentered bands need the sector-aware normalization slice or are simply honest.
Weights and A–F cutoffs were deliberately left untouched at the retune, reserving that call for this evidence.

TTM-basis adoption rates, and the quarterly balance-sheet leg live.
The contiguity guard fails a gapped window to the annual fallback rather than admitting a > 12-month "TTM", so the run measures how often adoption actually lands.

The basis-flip rate when a one-quarter feed gap drops a holding to the SEC annual basis.
The gate types a statement-derived series unevaluable once per flip, so what the run measures is how often the gate fires and how much of the valuation surface it types unevaluable.

Target provenance rendering against the sell-all cascade.
Every priced interpretation carries the `TargetMeta` derivation flags — rate-anchored vs current-multiple carry, flat or clamp-flattened driver, dispersion floor — and a floor-widened band inherits its base's signal quality.

The band-recalibration continuity NOTE and its what-changed attribution.

Conviction and action pairing, and the *fails* → indeterminate action distribution.
Only a *fails* hurdle read is dead money, and it reaches the model as a weighed exit input rather than an exit instruction.

The sector-P/E walk-back depth — how often the first weekday candidate misses.
The walk shares the report chain's `sector_candidate_dates`; an exhausted walk returns an error with the gap memoized onto every fund.

The risk-tier distribution now that negative-book issuers take High, read against the stacked conviction and action watches.

## Thesis ledger and the quick check

Debut ledger authorship quality at 47-position scale.

Live condition carry and supersession behavior, and tripped-claim discipline.

The first live quick-check sweep at 47-position scale — flag, badge and degraded-note render, plus the card overlay.

The ThesisAnchor render on the Portfolio card: the three-line clamp and its reveal-on-overflow contract, against real model-authored thesis lengths.
No component spec pins the overflow/reveal behavior, so this is the first read of it at scale — a thesis that never overflows and one that clamps both need to appear.

The debut-gap self-resolution.
The rate-anchor and pre-basis fund families read `unknown` until this run re-persists them.

Priced-fund ledger flag rates, now that authoring and evaluation share the 180-day window.

Fund-SEC-skip noise reduction, and the fund carry-path floor.

## Selective re-analysis

The first live selective run — the selection UI, the in-run tail sweep, and carried-card vintage, stale and demotion render at scale.

The transition-only `PriceOutsideBand` flag's live behavior: leave, re-enter and side-cross flag rates against the stamped authoring relation.
The authoring-time-outside design question was settled in code — a standing outside state no longer flags.
Pre-sweep ledgers carry no stamp and read authored-inside until re-analyzed, so early rates will understate.

## Pre-profit overlay

The first live overlay read at 47-position scale: eligibility rates, financing-state distribution, and unscorable-gap rates.

Two conventions need a live read specifically.
The STI-absent-reads-zero liquid-resources convention, and the YoY share-change quarter-contiguity assumption.

## Outcome learning

The first live pass: episode-debut volume at 47-position scale.

Sector-resolution rates through the fail-soft profile read.

The below-bar eligibility note — proposal statistics stay deferred behind the ≥ 30 unique holdings with matured windows bar (`outcome::PROPOSAL_ELIGIBILITY_BAR`).

## The two-arm verdict

The per-holding interpretation and action-call prompts' fit inside the shared 131 K `num_ctx`.
The settled response to pressure is to compress digests, never to raise `num_ctx`, and the fit is instrumented — per-call prompt counts and sent size, with pressure and truncation flags on the data-health read.

The first two-arm vintage: the retrospective and model-arm brief's prompt fit under the same instrumentation, feasibility-annotation rates, model-vs-engine divergence rates, and the paired two-arm card render at 47-position scale.

## Listing and identity shapes

The listing guard against real Schwab identity shapes.
Slash-notation class-share symbols read unsupported under the verbatim FMP lookup, and ticker-noise descriptions carry a false-conflict risk.

Exchange codes, including B3, and OCC slash notation.

The Schwab `averagePrice` multiplier.
The option and bond cost-derived render suppression stays in force until this settles — the derived basis leaves the contract or par multiplier unapplied, understating an option's and overstating a bond's.

## Data-source probes

`^GSPC` mapping sufficiency.

Analyst-estimates page ordering.

SEC sub-annual durations.

FMP in-progress-bar behavior, and boundary-day rates.

Sector-label taxonomy joins, and SHV-style short-screen labels.

FMP quota consumption under the full run's price load.
Dated-EOD is the only price rung (the 2026-08-12 Stooq removal), so the per-holding bulk load rides the paid key for the first time.
429-ladder behavior under that load: whether the minute-crossing ladder engages at all, and whether it recovers without failing a holding.

## Model serving and runtime

128 K runner stability.

Distill speed.

The data-health render and the reasoning panes.

## How to read the run

Read `data-health` early.
It carries the deep-price fetch health, the context-pressure and truncation flags, and the run-level roll-up with its attention state — several items above resolve off that one surface before the per-holding cards are worth reading.
