# Current session handoff

## Active task

Retry-once for the streaming agents — next up; nothing started.

## What happened

Primary-market froth landed in one task (`e2701a4`, pushed), and the item is complete.
The pure `primary_market.rs` reduces FMP's paid `ipos-calendar` and `mergers-acquisitions-latest` to the `primary_market` group: the IPO and registered-deal counts in the recent window against the equal-length window before, the upcoming-IPO count, and the notable names.
The 2026-10-12 probe reshaped the design: the IPO feed is a new-listings calendar, so an IPO counts only on a share count; the M&A feed is S-4 / F-4 registrations fanned out per security, so one deal is one filing link, worded as registrations, with no deal value.
Portfolio's M&A page walk is now shared (`walk_mergers` → `MaWalk`), Portfolio's output and gap sentences unchanged.
The Metis reviewer approved with nits, all ruled; Codex flagged three P2s, fixed two (withhold a side when a countable row is unreadable; dedup before windowing), answered one as the ruled completion state, then approved.
Every ruling is in the commit message.

## Current state

Nothing in flight.
Primary-market froth is under Built; retry-once for the streaming agents is next and unplanned.
The dev-run batch (every Portfolio item's dev run) and the big run are the user's call.
The prompt review can resume at file 18 (the review examples are 18–20, the thesis, appendix, role/risk and action files 21–30).
Residue carried (do not re-raise): the harness fixtures carry no quarterly, balance, score or target-stamp rows, and the live harness's TSLA header still reads "(name unavailable)" until the fixed-evidence set is rebuilt from a post-redesign run, after the dev-run batch; the fixed-evidence lexicon scan reads provider-served words as app prose; a role/risk prior that abstained reads its NAV, expense and exposure "then" from the abstaining run's basis (carrying the prior basis would change the quick check's comparator); the examples' continuity fixture anchors on the prior session itself, so the review examples layer a 2026-09-01 close.
A denied client-secret read still errors `schwab_status`, so Settings hides its Schwab section for that session (the report never reads the secret); a live denied prompt is fixture-verified only.
The calendar join, the history reads and the froth group are fixture- and probe-verified only; no live report run has carried them, and their Step-3 latency (the history's ~124 requests, the froth's IPO request and up to 10 M&A pages) is untimed.
The deal counts are small (the probe: 7 vs 15 monthly, 1 vs 2 weekly), so the deal trend is mostly noise at a weekly cadence; the IPO count rests on FMP leaving `shares` off non-offering rows.
FMP's rate-decision row carries an estimate and an actual, unused while the Fed events stay date-only.
The `sectors` performance snapshot is fetched without an exchange, so it carries FMP's default board unlabeled.
For the prompt review: the review's wording ("computed base none stated", no date line on the role/risk PRIOR POSITION, the bridge gloss doubled on a role/risk abstention across a split), and the statement-basis line's ledger-era "not evaluable here".
Every stamp bump touches the pins in `prompt_version_is_stamped_for_the_model_arm_domain_gate` (`portfolio/pipeline.rs`), which include the archive's `FORMAT_VERSION`.
The offscreen card-render recipe lives in session memory, not the repo.

## Open questions

- Portfolio's run-level M&A feed is the same S-4 / F-4 registration feed (dated at filing, weeks after announcement, stock deals only), yet `data-sources.md`'s Portfolio row and `portfolio-workflow.md §Step 5` call it a market-wide M&A feed — reword them, or leave?

## Where to start

`/metis-session-start`, then `/metis-plan-task` for retry-once for the streaming agents: read its BUILD entry and the docs it names first (`agents.md §Main Agent`, `§Analyst Agents`; `report-workflow.md §Step 12`, `§Step 16`; `scheduling.md §Error Handling`; `data-sources.md §LLM Providers`), and the trigger-set spec its assumption names (session memory, `model-call-empty-markdown-retry`); every flag and assumption through the selector.
Never propose the run.
