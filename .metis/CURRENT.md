# Current session handoff

## Active task

Primary-market froth — next up; nothing started.

## What happened

The sector and industry valuation and performance history landed in one task (`811c2db`, pushed), and the item is complete.
The pure `group_history.rs` derives two trailing-year reads per (group, exchange) from FMP's four paid history endpoints: `pe_history` (percentile, min / median / max, and the `start` / `latest` prints) and `trailing_return` (`return_pct`).
They ride the `SectorPe` and `IndustrySnapshot` rows; `SectorPerformance` carries neither, its snapshot row naming no exchange.
The fetch is about 124 sequential requests per run, fail-soft per endpoint and exchange; one four-GET probe (2026-10-12) verified the wire.
The main-agent prose reads the percentile as no direction: expansion or de-rating is claimed only from `start` against `latest`, or from the change view's sector `pe` move, and the three readings are level + price (Codex's P2, fixed in two rounds).
The Metis reviewer approved with nits, all ruled; Codex approved after the second round.
Every ruling is in the commit message.

## Current state

Nothing in flight.
The sector and industry history is under Built; primary-market froth is next and unplanned.
The dev-run batch (every Portfolio item's dev run) and the big run are the user's call.
The prompt review can resume at file 18 (the review examples are 18–20, the thesis, appendix, role/risk and action files 21–30).
Residue carried (do not re-raise): the harness fixtures carry no quarterly, balance, score or target-stamp rows, and the live harness's TSLA header still reads "(name unavailable)" until the fixed-evidence set is rebuilt from a post-redesign run, after the dev-run batch; the fixed-evidence lexicon scan reads provider-served words as app prose; a role/risk prior that abstained reads its NAV, expense and exposure "then" from the abstaining run's basis (carrying the prior basis would change the quick check's comparator); the examples' continuity fixture anchors on the prior session itself, so the review examples layer a 2026-09-01 close.
A denied client-secret read still errors `schwab_status`, so Settings hides its Schwab section for that session (the report never reads the secret); a live denied prompt is fixture-verified only.
The calendar join and the history reads are fixture- and probe-verified only; no live report run has carried them, and the history's Step-3 latency against FMP's per-minute limit is untimed.
FMP's rate-decision row carries an estimate and an actual, unused while the Fed events stay date-only.
The `sectors` performance snapshot is fetched without an exchange, so it carries FMP's default board unlabeled.
The froth enrichment extends the router's enrichment sentences and `report-workflow.md §Step 16`'s enrichment-prose block, whose main-agent bullet still reads the built calendar prose as "Add prose for…" and whose analyst bullet still offers the optional sentence.
For the prompt review: the review's wording ("computed base none stated", no date line on the role/risk PRIOR POSITION, the bridge gloss doubled on a role/risk abstention across a split), and the statement-basis line's ledger-era "not evaluable here".
Every stamp bump touches the pins in `prompt_version_is_stamped_for_the_model_arm_domain_gate` (`portfolio/pipeline.rs`), which include the archive's `FORMAT_VERSION`.
The offscreen card-render recipe lives in session memory, not the repo.

## Open questions

None.

## Where to start

`/metis-session-start`, then `/metis-plan-task` for primary-market froth: read its BUILD entry and the docs it names first (`data-sources.md §Report enrichment`, `report-workflow.md §Step 3`, `§Step 8`, `§Step 12`, `§Step 16`, `storage.md §Baseline Snapshots`); every flag and assumption through the selector.
Never propose the run.
