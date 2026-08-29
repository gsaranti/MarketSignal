# Big confirmation run — watch set

The single big confirmation run banks every runtime confirmation the locked pre-test block stacked up.
This file is the checklist it reads against: what to look for, grouped by the surface that produces it.
It is forward-looking, unlike the dated records beside it — those are written after the fact, this one is written before.
Findings go into a dated record once the run completes; this file is then the index of what that record has to answer.
Revised 2026-08-18 to the `portfolio-v9` shape: the construction-stage, lean / divergence, and sizing watches are removed, since that machinery no longer exists.
Revised again 2026-08-24, the pre-run bar now met: the research-loop, ruling-watch, pre-profit-activation, CBOE-backdrop, narrative-comparator, and Schwab-CEF-typing additions are folded in, with the prompt-stamp and Step-6a notes.
Records this run persists stamp `portfolio-v22`, not the `portfolio-v11` these additions were first queued under — the run-evidence, infrastructure, research-loop, F3 tie-channel, expense-ratio render, ledger-basis vocabulary, IV-skew convention, observation-excerpt, guidance-vintage, action-target, model-arm-domain, finite-record (Codex I7 / I9 / I16), and prompt-render / period-word-guard (Codex I8 / I10 / I12 / I19) slices all landed since the 2026-08-18 revision with no live run between.
Revised again 2026-08-27 with the fired-retry watch (§Model serving and runtime), the sub-distillation-cap watch (§The research loop), and the one-month cap-saturation watch (§Grade, valuation and targets).
Revised again 2026-08-27 with the technology-event pre-flag watch (§Thesis ledger and the quick check).

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
An annual-basis holding whose rewritten ledger still says "TTM" in a statement condition is the retired vocabulary leaking back — the prompt now states the basis beside the series list, so read a few annual-basis ledgers for it.

Target provenance rendering against the sell-all cascade.
Every priced interpretation carries the `TargetMeta` derivation flags — rate-anchored vs current-multiple carry, flat or clamp-flattened driver, dispersion floor — and a floor-widened band inherits its base's signal quality.

The parameter-boundary continuity NOTE and its what-changed attribution.
Attempt 2's priors are stamped `grade-v2.1`, so a stock carries no NOTE and no boundary row, and a priced fund carries the momentum re-homing row — read each fund's momentum delta against it, and any stock row citing a recalibration is a defect.

The one-month band's cap saturation under `targets-v5`.
With the band √t-scaled to the month, the 15% cap binds from a daily σ of ~1.64% (~26% annualized), so the run measures the share of priced names pinned at the cap.
The ruling kept the clamp; a wider cap waits on this reading.

Conviction and action pairing, and the *fails* → indeterminate action distribution.
Only a *fails* hurdle read is dead money, and it reaches the model as a weighed exit input rather than an exit instruction.

The sector-P/E walk-back depth — how often the first weekday candidate misses.
The walk shares the report chain's `sector_candidate_dates`; an exhausted walk returns an error with the gap memoized onto every fund.

The risk-tier distribution now that negative-book issuers take High, read against the stacked conviction and action watches.

## Thesis ledger and the quick check

Debut ledger authorship quality at 47-position scale.

Live condition carry and supersession behavior, and tripped-claim discipline.

The technology-event pre-flag's fire rate at book scale, and the memoized-benchmark race.
The sector benchmark is fetched run-level and memoized per symbol, so a holding whose dated EOD lands after that fetch across the EOD-posting boundary reads a benchmark one session short.
That reads as the typed gap `no <benchmark> close on the holding's newest session …` on `degraded_inputs`, never a flag; count those gaps, since each is a holding whose input delta lost its pre-flag to timing rather than to data.

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

## The research loop

The first live per-holding research loop at 47-position scale: SearXNG search availability against the Tavily fallback rate, and the per-topic pass loop's depth against real pages.

The per-holding budgets, calibrating on this evidence: the 40-attempt fetch ceiling (`MAX_FETCHES_PER_HOLDING`; failed live attempts spend it), the 30-minute wall clock (`MAX_WALL_PER_HOLDING`), the 4,000-char per-topic seed budget (`SEED_BUDGET_CHARS`), and the pass-shape constants (`MAX_TURNS_PER_PASS`, `MAX_PASSES_PER_TOPIC`, `PAGE_TEXT_CAP_CHARS`, `MAX_CLAIMS_PER_PASS`) — all kept as drafted until this run reads them (ruled 2026-08-27).
What the run measures is how often each binds, and what the seed's fixed drop order actually drops when it does.

Typed-channel validation yield against real fetched pages: how often the page-grounding checks — stated numbers, forward-fact language, the structural identity matcher, monetary unit typing — accept vs reject each of the three channels.

Seed-and-merge cache behavior at scale: hit rates, and the distillation-reconciliation gap recordings where the model fails to re-emit a topic.

The sub-distillation cap's first live reading: how often a holding's distillation goes hierarchical, and whether any topic drops passes at the cap (`dropped at the sub-distillation cap` in the run's gaps).
Zero drops is the healthy read.
Any drop is the first evidence the shared per-holding budget binds at real sizes.
A topic whose every pass dropped also loses its seed — the exhausted-budget edge recorded under the 2026-08-24 review's §A4 — and one such hit promotes the queued fix that routes that topic's prior through the reduce.

Extraction telemetry — the deferred rendered-retrieval tier's scheduling evidence.
Per-domain thin-stub and `extraction_quality` rates decide whether and where a render tier earns its slice, so the run record's disposition reads them deliberately, not incidentally.

The three 2026-08-24 ruling watches — the shadow evidence the promotion decisions read.
Shadow-assumption resolutions: inspect each would-have audit line against its cited pages.
Unverified-driver indicator gaps: how often an indicator arrives without a resolvable `confirms_driver_id` and stays gap-noted evidence rather than suppressing the hype ceiling.
Advisory fraud claims: any validated claim's render as labeled attention evidence, never a hard-forensic trip.

## Pre-profit overlay

The first live overlay read at 47-position scale: eligibility rates, financing-state distribution, and unscorable-gap rates.

The producer's first live activation: observation rows entering through the research loop's fetched-page lineage — validated-row volume, the rejection split across the activation legs (the quoted excerpt's presence in the page, the value at its sign inside it, its metric-family language, its one-number shape, ISO period normalization, holding identity), and the Step-6e recompute of the observation-dependent legs.

The guidance vintage policy's first live read (stamped `pre-profit-v3`): how often an accepted guidance row is retrospective — dated on or after its period's earliest actual, or after the period end — or a same-vintage conflict drops a period, read off the persisted rows against the execution read's comparable count and each miss's recorded bound and actual dates.

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

The narrative-vs-reality comparator debut.
No prior run has persisted the audit-basis comparator, so every holding should carry no pace read this run — a missing comparator must record as a debut or typed unreadable-pace reason, never a fabricated neutral — and the run must persist the comparator the next run's read needs.

## Listing and identity shapes

The listing guard against real Schwab identity shapes.
Slash-notation class-share symbols read unsupported under the verbatim FMP lookup, and ticker-noise descriptions carry a false-conflict risk.

Exchange codes, including B3, and OCC slash notation.

The Schwab CEF typing: whether a held CEF arrives `COLLECTIVE_INVESTMENT` (routing to the fund path, where the CEF leg lives) or `EQUITY` (the stock path, which floor-abstains it before detection ever runs).
Unverifiable without holding one, so this reads only if the book holds a CEF.

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

The CBOE venue-level put/call backdrop's first live read: the extraction against the live payload shape, the `selectedDate` as-of, and the fail-soft render where the backdrop applies.

## Model serving and runtime

128 K runner stability.

Distill speed.

The data-health render and the reasoning panes.

Fired bounded retries — the transient-failure rate over hundreds of hard-path calls (settled 2026-08-27; the contract is [local-models.md §The local-model adapter seam](../local-models.md#the-local-model-adapter-seam)).
Each fired retry lands as a data-health summary line plus structured `model_retries` events with stage and class; zero fired retries is the healthy read, and any nonzero count is the first live measurement of the rate the 2026-08-24 review could only bound by construction.
A resumed run's count spans both processes — every restored row's calls and the resumed process's own — and omits only the superseded calls of holdings the resumed process re-analyzed ([portfolio-analysis.md §Failure posture](../portfolio-analysis.md#failure-posture)).
Read its rate as the rate over the calls the finished verdicts rest on, and its count as a floor on every call the run ever issued.
A run that still fails hard *after* a retry names the first attempt's class in its failure detail — read that class before treating the failure as novel.
A `model arm value off its declared domain` class is the first live measurement of the off-domain rate the decode gate bounds by construction (Codex I6): a zero count says the prompt's stated scale held, and any nonzero count is a prompt-fit signal to read before it is a model fault.

## How to read the run

Read `data-health` early.
It carries the deep-price fetch health, the context-pressure and truncation flags, the fired-retry events, and the run-level roll-up with its attention state — several items above resolve off that one surface before the per-holding cards are worth reading.

One expected absence: Step-6a semantic recall retrieves nothing this run.
The per-holding summary partition holds no rows until this run persists them, retrieval going live from the second run — an empty recall is design, not a defect.
