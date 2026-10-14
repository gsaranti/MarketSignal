# Current session handoff

## Active task

Trade Opportunities: the job spine, engine arm and gate — next and unplanned.

## What happened

The factor machinery's task 4, the factor-distribution store, landed (`180a411`, pushed), so the item is complete and moved to Built.
`portfolio/factor_store.rs` keeps one current observation per issuer per factor (the raw value, or floored for debt over a non-positive equity), bucketed by profile sector × market-cap band.
Observations age out after 24 calendar months, and the read waits for 20 other issuers holding the factor.
Portfolio's fresh passes contribute fail-soft; the read has no caller yet.
The store joins the archive at format v24, and import now refuses any table entry present but unlisted in the manifest whatever the archive's version (Codex's P2).
No prompt or checkpoint stamp moved.
The Metis reviewer approved with nits: N1–N4 applied, N5 carried, N6 left; every ruling is in the commit message.

## Current state

The TO spine is next and unplanned; the plan-task decides its task breakdown.
For the spine: it renders `factor_store::bucket_position` into the dossier and audit and contributes through `factor_store::contribute`; `CapBand` is also discovery's stratum (the Market-cap bands row).
"Runway years" (the TO horizon rule's input) stays undefined for the spine to draft, as do the illiquid and high-event-exposure predicates.
Fetch a candidate's deep history once through `fetch_dated_bars` and derive its closes from the bars; FMP's light-EOD volume and its five annual income rows stay live-unverified until the spine's dev run.
For a later docs pass: `trade-opportunities-workflow.md §Step 5h` restates the entry gate's legs beside the entry asymmetry row, and `§Step 5c`'s "unique-issuer floor" reads looser than the store row's per-factor floor.
Calibration residue: the cap bands are drafted; two share classes count as two issuers.
The dev-run batch (every Portfolio item's dev run) and the big run are the user's call; the drafted cutoffs and anchors (the market-wide bands, GP/A, D/E, volatility, FFO over assets) are shadow-tuned after it through `sweep_grade_bands`.
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

The factor-distribution store's INDEX row (and a Market-cap bands row) under "Shared across both jobs" — proposed at this session's end, unwritten until the user says go.

## Where to start

`/metis-session-start`; if the INDEX rows are still unruled, settle them first.
Then `/metis-plan-task` for the Trade Opportunities job spine: read its BUILD entry's docs (TO-workflow §Steps 1, 2, 5a–5c, 5h, 6, 9; trade-opportunities.md §Evidence floor, §The opportunity space, §Archetype, §Starting parameters, §Failure posture), decide its task breakdown, then put every flag and assumption through the selector.
Never propose the run.
