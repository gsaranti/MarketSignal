# Big confirmation run — watch set

The single big confirmation run banks every runtime confirmation the locked pre-test block stacked up.
This file is the checklist it reads against: what to look for, grouped by the surface that produces it.
It is forward-looking, unlike the dated records beside it — those are written after the fact, this one is written before.
Findings go into a dated record once the run completes; this file is then the index of what that record has to answer.
Revised 2026-08-18 to the `portfolio-v9` shape: the construction-stage, lean / divergence, and sizing watches are removed, since that machinery no longer exists.
Revised again 2026-08-24, the pre-run bar now met: the research-loop, ruling-watch, pre-profit-activation, CBOE-backdrop, narrative-comparator, and Schwab-CEF-typing additions are folded in, with the prompt-stamp and Step-6a notes.
Records this run persists stamp `portfolio-v27`, not the `portfolio-v11` these additions were first queued under — the run-evidence, infrastructure, research-loop, F3 tie-channel, expense-ratio render, ledger-basis vocabulary, IV-skew convention, observation-excerpt, guidance-vintage, action-target, model-arm-domain, finite-record (Codex I7 / I9 / I16), prompt-render / period-word-guard (Codex I8 / I10 / I12 / I19), continuity-attribution (Codex I11 / I13), Review 2 period-span, constant-period revision, assumption-unit, and fund-classification slices all landed since the 2026-08-18 revision with no live run between, and the fresh-start-2 compat cut landed with no stamp.
The observation-admission-stamp slice (Codex I20) moved the checkpoint format stamp to `checkpoint-v3`; the required period span moved it to `checkpoint-v4`; the quick-check carried-tail evaluation stamp moved it to `checkpoint-v5`; the raw fiscal-period consensus rows now move it to `checkpoint-v6`.
The dev store is wiped before this run (ruled 2026-08-29), so every holding is a debut: no prior verdict exists, and nothing that reads against one — the retrospective, the input delta, the ledger evaluation and its crossings, the what-changed audit, the parameter-boundary NOTE, the statement-basis and equity-source gates, episode extension — can fire on run 1.
The quick check is the exception: it runs between runs against run 1's own persisted comparators, so its first sweep is a run-1 watch (the oracle under §Thesis ledger and the quick check).
The items below that read against a prior are therefore run-2 watches, and a second run follows only on the user's decision after run 1's result.
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

The equity-source flip rate when the FMP balance-sheet leg gaps and the equity leg falls to SEC's annual stockholders' equity, or returns from it.
Run 1 stamps every debt/equity and price/book condition authored on a surface with an equity leg with that source at authoring (6g) — one authored with no equity leg stays unstamped until a full pass evaluates it — and can disagree with nothing; on run 2 the first evaluation already has that stamp to disagree with, so the gate types the two instants unevaluable once per flip, naming the source change, and a pass on which the basis and the source flip together is one unevaluable pass, not two.
A between-run sweep withholds every debt/equity condition not stamped with its own FMP-quarterly source — SEC-stamped after a full pass whose FMP leg gapped ("cannot compare across the source"), or unstamped ("carries no equity-source stamp") — the filing family `unknown`, rather than confirming it off the fresh leg; read the sweep notes for both.
Read the basis line of a few prompts for the source it names.
A ledger whose price/book condition confirmed within days of an FMP balance-sheet gap healing is the flip the gate exists to stop.

Target provenance rendering against the sell-all cascade.
Every priced interpretation carries the `TargetMeta` derivation flags — rate-anchored vs current-multiple carry, flat or clamp-flattened driver, dispersion floor — and a floor-widened band inherits its base's signal quality.

The parameter-boundary continuity NOTE and its what-changed attribution.
No prior exists on run 1, so no holding carries a NOTE or a boundary row, and any row citing a recalibration, re-homing, or sector-P/E exchange-basis correction is a defect; on run 2 every prior is stamped `grade-v2.3`, the current stamp, so the same holds there.
The scenario-target boundary reads the same way: every run-2 prior is stamped `targets-v6`, the current stamp, so a row or NOTE citing a scenario-target parameter change is a defect on either run.

The one-month band's cap saturation under current `targets-v6` (the √21 rule introduced at `targets-v5`).
With the band √t-scaled to the month, the 15% cap binds from a daily σ of ~1.64% (~26% annualized), so the run measures the share of priced names pinned at the cap.
The ruling kept the clamp; a wider cap waits on this reading.

Conviction and action pairing, and the *fails* → indeterminate action distribution.
Only a *fails* hurdle read is dead money, and it reaches the model as a weighed exit input rather than an exit instruction.

The sector-P/E walk-back depth — how often the first weekday candidate misses or serves only one exchange leg.
The walk shares the report chain's `sector_candidate_dates`; a candidate enters only with non-empty NYSE and NASDAQ legs, and an exhausted walk returns an error with the gap memoized onto every fund.
For each sector history, verify that a fault or empty response on either board leaves the history absent and repeats the memoized typed gap on every fund whose weights depend on that sector, including later funds.

The corrected fund-classification boundary: any fixed-income `ultra-short` / `ultra short` holding must render as a bond fund without a leveraged / inverse flag, and any explicit allocation / multi-asset class must stay `role_risk_only` even if FMP serves sector rows.
For every role-risk option-overlay fund, inspect the model input and confirm the structural line says option-overlay path dependency rather than leveraged / inverse daily reset.

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
Run 1 persists every quick-check basis and fund comparator fresh (a debut resolves its price bridge at exactly 1.0), so the first sweep after it reads them all; a family reading `unknown` there is a genuinely withheld or missing input — an unresolvable price basis, a failed leg — never an expected state.

Priced-fund ledger flag rates, now that authoring and evaluation share the 180-day window.

Fund-SEC-skip noise reduction, and the fund carry-path floor.

## Selective re-analysis

The first live selective run — the selection UI, the in-run tail sweep, and carried-card vintage, stale and demotion render at scale.

The transition-only `PriceOutsideBand` flag's live behavior: leave, re-enter and side-cross flag rates against the stamped authoring relation.
The authoring-time-outside design question was settled in code — a standing outside state no longer flags.

## The research loop

The first live per-holding research loop at 47-position scale: SearXNG search availability against the Tavily fallback rate, and the per-topic pass loop's depth against real pages.

The per-holding budgets, calibrating on this evidence: the 40-attempt fetch ceiling (`MAX_FETCHES_PER_HOLDING`; failed live attempts spend it), the 30-minute wall clock (`MAX_WALL_PER_HOLDING`), the 4,000-char per-topic seed budget (`SEED_BUDGET_CHARS`), and the pass-shape constants (`MAX_TURNS_PER_PASS`, `MAX_PASSES_PER_TOPIC`, `PAGE_TEXT_CAP_CHARS`, `MAX_CLAIMS_PER_PASS`) — all kept as drafted until this run reads them (ruled 2026-08-27).
What the run measures is how often each binds, and what the seed's fixed drop order actually drops when it does.

Typed-channel validation yield against real fetched pages: how often the page-grounding checks — stated numbers, forward-fact language, the structural identity matcher, monetary unit typing — accept vs reject each of the three channels.

Seed-and-merge cache behavior at scale: hit rates, and the distillation-reconciliation gap recordings where the model fails to re-emit a topic.

The sub-distillation cap's first live reading: how often a holding's distillation goes hierarchical, and whether any topic drops passes at the cap (`dropped at the sub-distillation cap` in the run's gaps).
Zero drops is the healthy read.
Any drop is the first evidence the shared per-holding budget binds at real sizes.
A topic whose every pass dropped keeps its seed: its prior rides the reduce retained on its own vintage (the 2026-08-24 review's §A4 edge, closed 2026-08-29).
Read that gap line's tail — `its prior object rides the reduce retained on its own vintage` or `no prior to retain` — and confirm the topic is absent from `unreconciled_topics`.

Extraction telemetry — the deferred rendered-retrieval tier's scheduling evidence.
Per-domain thin-stub and `extraction_quality` rates decide whether and where a render tier earns its slice, so the run record's disposition reads them deliberately, not incidentally.

The three 2026-08-24 ruling watches — the shadow evidence the promotion decisions read.
Shadow-assumption resolutions: inspect each would-have audit line against its cited pages.
Verify any cents-denominated fact records the dollar-scaled value and every named foreign-currency fact records a no-FX rejection rather than a would-have USD target.
Unverified-driver indicator gaps: how often an indicator arrives without a resolvable `confirms_driver_id` and stays gap-noted evidence rather than suppressing the hype ceiling.
Advisory fraud claims: any validated claim's render as labeled attention evidence, never a hard-forensic trip.

## Pre-profit overlay

The first live overlay read at 47-position scale: eligibility rates, financing-state distribution, and unscorable-gap rates.

The producer's first live activation: observation rows entering through the research loop's fetched-page lineage — validated-row volume, the rejection split across the activation legs (the quoted excerpt's presence in the page, the value at its sign inside it, its metric-family language, its one-number shape, ISO period normalization, period-span presence and label consistency, holding identity), and the Step-6e recompute of the observation-dependent legs.
Every accepted row persists `admitted_under` = `portfolio-v27` (Codex I20 plus Review 2 N1; later revision, assumption-unit, and fund-classification slices moved the shared prompt stamp).
The stamp's first attributable read — a carried row telling itself apart under a later contract — is a run-2 watch.

The span-aware guidance-vintage policy's first live read (stamped `pre-profit-v4`): the distribution of quarter / half-year / full-year / year-to-date / point-in-time / unknown rows; how often unknown stays unpaired or an explicit label conflicts; and how often an accepted guidance row is retrospective — dated on or after its same-span period's earliest actual, or after the period end — or a same-vintage conflict drops a period, read off the persisted rows against the execution read's comparable count and each miss's recorded span, bound date, and actual date.

Two conventions need a live read specifically.
The STI-absent-reads-zero liquid-resources convention, and the YoY share-change quarter-contiguity assumption.

## Outcome learning

The first live pass: episode-debut volume at 47-position scale.

On any later run where a confirmed falsifier and recommendation change coincide, the event must remain on the episode active at run start; the successor's event list stays empty for that crossing.
A carried rule-demotion episode's calibration snapshot must show one intrinsic hurdle / DGS2 pair, never the consuming run's rate beside the carried hurdle.
A lead-time event dated after its episode's twelve-month window must persist as post-maturity context and stay out of the derived lead-time read, never clamp to the final bar.

Sector-resolution rates through the fail-soft profile read.

The below-bar eligibility note — proposal statistics stay deferred behind the ≥ 30 unique holdings with matured windows bar (`outcome::PROPOSAL_ELIGIBILITY_BAR`).

## The two-arm verdict

The per-holding interpretation and action-call prompts' fit inside the shared 131 K `num_ctx`.
The settled response to pressure is to compress digests, never to raise `num_ctx`, and the fit is instrumented — per-call prompt counts and sent size, with pressure and truncation flags on the data-health read.

The first two-arm vintage: the retrospective and model-arm brief's prompt fit under the same instrumentation, feasibility-annotation rates, model-vs-engine divergence rates, and the paired two-arm card render at 47-position scale.

The narrative-vs-reality comparator debut.
No prior run exists, so every holding carries no pace read on run 1 — a missing comparator must record as a debut reason, never a fabricated neutral — and run 1 must persist the comparator run 2's read needs.
On run 2, inspect the persisted fiscal-period matches and confirm that the reality leg uses the prior weights across those matches; a rolled NTM blend alone must neither create a quick-check revision event nor mask the narrative read, and no common period must use the operating fallback.

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
The per-holding summary partition holds no rows until this run persists them, retrieval going live only on a later run — an empty recall is design, not a defect.
