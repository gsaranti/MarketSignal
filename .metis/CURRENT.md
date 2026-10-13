# Current session handoff

## Active task

Bounded baseline-history render — next up; nothing started.

## What happened

Retry-once for the streaming agents landed in one task (`7083282`, pushed), and the item is complete.
The main agent's and the three analysts' streaming calls re-attempt exactly once on a transient failure through one shared gate (`model_agent::retry_stream_once`), the class a typed marker beneath each unchanged error message.
The whitelist took the parse failure, the 429 / 5xx, the failed send and, against the recommendation, timeouts; a refusal from either provider, a cut-off, any other 4xx and a cancelled run never re-attempt.
A fired retry is its own tracker row, a new `stream-restarted` event clears the pane, and the thought log writes a restart line.
The contract lives in `agents.md §Bounded Retry on the Streaming Calls`, with one-line pointers elsewhere and an INDEX row.
The Metis reviewer rejected on one docs criterion (`run-tracking.md §Thought-log capture`), which was fixed, then approved with nits, all ruled.
Codex's P2 — an OpenAI structured-output refusal reached the parse marker and retried — was fixed, the refusal now caught on `response.refusal.done`.
Every ruling is in the commit message.

## Current state

Nothing in flight.
Retry-once is under Built; the bounded baseline-history render is next and unplanned, its series set, depth and render shape settled at plan time.
The dev-run batch (every Portfolio item's dev run) and the big run are the user's call.
The prompt review can resume at file 18 (the review examples are 18–20, the thesis, appendix, role/risk and action files 21–30); the stamp is `portfolio-v76`, the next `portfolio-v77`.
Residue carried (do not re-raise): the harness fixtures carry no quarterly, balance, score or target-stamp rows, and the live harness's TSLA header still reads "(name unavailable)" until the fixed-evidence set is rebuilt from a post-redesign run, after the dev-run batch; the fixed-evidence lexicon scan reads provider-served words as app prose; a role/risk prior that abstained reads its NAV, expense and exposure "then" from the abstaining run's basis; the examples' continuity fixture anchors on the prior session itself, so the review examples layer a 2026-09-01 close.
A denied client-secret read still errors `schwab_status`, so Settings hides its Schwab section for that session; a live denied prompt is fixture-verified only.
The calendar join, the history reads and the froth group are fixture- and probe-verified only, their Step-3 latency untimed; the deal counts are small, so the deal trend is mostly noise at a weekly cadence; the IPO count rests on FMP leaving `shares` off non-offering rows.
FMP's rate-decision row carries an estimate and an actual, unused while the Fed events stay date-only; the `sectors` snapshot carries FMP's default board unlabeled.
The retry is wire-tested over a localhost mock only, a send timeout at unit level only (the 600 s deadlines are consts); a failed analyst now waits out a sibling's retry before the run fails, since the pipeline joins all three threads; a cancel during the pause keeps its "retrying once" row.
For the prompt review: the review's wording ("computed base none stated", no date line on the role/risk PRIOR POSITION, the bridge gloss doubled on a role/risk abstention across a split), and the statement-basis line's ledger-era "not evaluable here".
Every stamp bump touches the pins in `prompt_version_is_stamped_for_the_model_arm_domain_gate` (`portfolio/pipeline.rs`), which include the archive's `FORMAT_VERSION`.
The offscreen card-render recipe lives in session memory, not the repo.

## Open questions

None.

## Where to start

`/metis-session-start`, then `/metis-plan-task` for the bounded baseline-history render: read its BUILD entry and the docs it names first (`storage.md §Baseline Snapshots`; `report-workflow.md §Step 3`, `§Step 5`, `§Step 16`; `data-sources.md §Financial Modeling Prep`, `§FRED`, `§Report enrichment`) and its candidate note in session memory (`baseline-history-render-candidate`); every flag and assumption through the selector.
Never propose the run.
