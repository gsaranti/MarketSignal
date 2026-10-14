# Current session handoff

## Active task

The shared engine's factor machinery — tasks 1 and 2 of 4 landed; task 3 next and unplanned.

## What happened

Task 2, Portfolio's letter inheriting the factor scores, landed (`bd1e6c5`, pushed).
A stock's quality, valuation, risk and momentum sub-scores are now the factor composite's axes under the profile's sector and a metric class (general, bank, other financial, equity REIT), banks and REITs reading their own anchor rows.
The cutoffs moved to 70 / 60 / 45 / 35 on both branches; the stamps are `grade-v3`, `evidence-floor-v6`, `portfolio-v77`, `checkpoint-v26` and archive 23.
The audit persists the whole composite and the verdict a grade basis, both required keys through the new `required_option` deserializer (the user's ruling: an absent key fails the decode, `null` reads none).
SCORES lists each score's factors, the card's engine detail shows a basis line, and the ignored harness replays persisted composites (the sweep shifting anchors as well as widths).
The Metis reviewer rejected once (the sweep had no anchor knob), then approved with nits; Codex's overflow P2 was fixed and its required-key P2 ruled.
Every ruling is in the commit message.

## Current state

The factor-machinery item stays under To build; tasks 1 and 2 of 4 are done.
Task 3: the Trade Opportunities–side reads — the price-action confirmer, the tradability flag with its banded haircut (EOD volume kept, the Amihud and days-to-cover bands drafted), the 70% narrative trip, and the archetype driver overrides (a driver-ladder hook, mid-cycle EPS).
Task 4: the factor-distribution store — one current observation per issuer per factor, the 24-month age-out and 20-issuer floor, Portfolio's engine pass contributing fail-soft from `job::run_analysis`, the portability archive at the next format; its INDEX row lands with it.
"Runway years" (the TO horizon rule's input) stays undefined; the TO spine item drafts it.
The dev-run batch (every Portfolio item's dev run) and the big run are the user's call; the drafted cutoffs and the drafted anchors (the market-wide bands, GP/A, D/E, volatility, FFO over assets) are shadow-tuned after it through `sweep_grade_bands`.
The prompt review can resume at file 18 (the review examples are 18–20, the thesis, appendix, role/risk and action files 21–30); the stamp is `portfolio-v77`, the next `portfolio-v78`.
Task 2's findings for later: the Real Estate row still averages REITs with real-estate operators though REITs read their own row; METRICS still renders P/E, P/S and P/B beside the factor lines (prompt-review material); the thin-history flag counts the growth and investment factors outside the letter.
Residue carried (do not re-raise): the harness fixtures carry no quarterly, balance, score, target-stamp or factor-composite rows, so examples 21–22 show no factor lines and the live harness's TSLA header still reads "(name unavailable)" until the fixed-evidence set is rebuilt after the dev-run batch; the fixed-evidence lexicon scan reads provider-served words as app prose; a role/risk prior that abstained reads its NAV, expense and exposure "then" from the abstaining run's basis; the examples' continuity fixture anchors on the prior session itself, so the review examples layer a 2026-09-01 close.
A denied client-secret read still errors `schwab_status`, so Settings hides its Schwab section for that session; a live denied prompt is fixture-verified only.
The calendar join, the history reads and the froth group are fixture- and probe-verified only, their Step-3 latency untimed; the deal counts are small, so the deal trend is mostly noise at a weekly cadence; the IPO count rests on FMP leaving `shares` off non-offering rows.
FMP's rate-decision row carries an estimate and an actual, unused while the Fed events stay date-only; the `sectors` snapshot carries FMP's default board unlabeled.
The retry is wire-tested over a localhost mock only, a send timeout at unit level only (the 600 s deadlines are consts); a failed analyst now waits out a sibling's retry before the run fails; a cancel during the pause keeps its "retrying once" row.
The baseline history's prompt size is unmeasured until a live run; no wire test drives the real main agent over a history; the valuation rule's "only" does not admit the history's sector P/E path (ruled left).
The statement currency check spans sixteen balance-sheet and cash-flow rows (accepted).
For the prompt review: the review's wording ("computed base none stated", no date line on the role/risk PRIOR POSITION, the bridge gloss doubled on a role/risk abstention across a split), and the statement-basis line's ledger-era "not evaluable here".
Every stamp bump touches the pins in `prompt_version_is_stamped_for_the_model_arm_domain_gate` (`portfolio/pipeline.rs`), which include the archive's `FORMAT_VERSION`.
The offscreen card-render recipe lives in session memory, not the repo.

## Open questions

- Should CLAUDE.md and AGENTS.md §Pre-release data posture gain one sentence naming the `required_option` convention for new persisted `Option` fields (offered 2026-10-13, unanswered)?

## Where to start

`/metis-session-start`, then `/metis-plan-task` for the factor machinery's task 3, the Trade Opportunities–side reads: read the matching rows of `trade-opportunities.md §Starting parameters`, `trade-opportunities-workflow.md §Step 5c` and `portfolio/factors.rs`, then put every flag and assumption through the selector.
Never propose the run.
