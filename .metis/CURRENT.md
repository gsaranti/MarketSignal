# Current session handoff

## What happened

**The quick check slice SHIPPED — merged to `main` as PR #56 (squash `4db77a0`), feature branch deleted.**
The held diff was committed to `portfolio-quick-check`, then driven through **eight external review rounds to convergence** (18 findings over rounds 1–7, each verified against code/docs before fixing; round 8 approved).
Load-bearing shapes the rounds added beyond the original slice: per-holding clear on persist (an abstention retains its carried sweep state re-stamped to the new run, its rate cache following the run's prints); the filing re-pull's dividend leg reaching the hurdle under the adapter's **None-with-no-gap = confirmed-non-payer** contract (shared gap-prefix consts, Result-shaped parser, parsed-date windowing); fund-refresh honesty — mandate / label / overlay-flag legs independently gated on what each derives from, `FundExposureBasis.structural_flag` as `Option<bool>` (legacy `None` degrades, never fabricates), blank `etf/info` strings normalized to `None` at the adapter; the typed `unevaluable_series` channel downgrading a claimed clear per family; malformed-200 strictness across the SEC/FMP shapers; portability **format v2** with the versioned closed entry set; `StepStatus` gaining `flagged`/`unknown` (sanctioned amber pair — a noted design-package extension); the report pane excluding both portfolio trace kinds.
BUILD/INDEX caught up this session — **no capture debt**.

## Current state

`main` at `4db77a0`, tree clean, nothing in flight. Queue head: the **selective re-analysis** slice — the quick check supplies its full force-include surface (per-holding flags, `unknown` degraded families, accumulated unexamined evidence events).

## Open questions

- **Docs-capture candidates:** the review rounds' code-enforced-but-docs-unpinned contracts (None-with-no-gap non-payer; mandate-vs-label comparability + blank-metadata normalization; the unevaluable→family downgrade) — plus the carried set: FINRA-leg vacuity vs the 12-series surface; cash-flow re-pull named but unconsumed; breadth-flip sub-leg unbuildable; the ledger slice's three (cadence-derived counts; monitor-level goalposts; value-keyed + marks-day identities). Canonical docs homes still unwritten.
- **Debut gaps (self-resolve at the big run's persist):** the first sweep reads the rate-anchor family `unknown` (pre-`RatePrints` run) and pre-basis funds read FundInfo `unknown`.
- **Evidence-event boundary** uses the run's `created_at` — must move to per-holding vintages in the selective slice.
- `job_status` has no `job_type` filter — quick-check runs move the footer's last-run timestamps (accepted).
- **No A letters under grade-v2** (META 84.0 vs ≥ 85) — reserved for normalization or the big run.
- **Carried unchanged:** big-run checklist in BUILD §What remains (now incl. the quick-check legs); reasoning-pane DOM weight; encrypted portability round-trip; step-17 embedding watch; 600 s stress; scorecard display; dev-store residue; Keychain fail-soft; stage-and-swap import; chain both-maps invariant; four-part verdict bound; §1 open drafts; fraud-producer posture; fund-slice drafted constants; checkpoint/resume + the 6g input-delta validator (docs-promised, unbuilt).

## Where to start

Run `/metis-plan-task` for the **selective re-analysis** slice (docs/portfolio-analysis.md §Triggering; portfolio-workflow.md §Step 6, §Step 7b): the chosen-subset re-run under the three mixed-vintage safety rules — force-include off the sweep's flags / `unknown` families / side reversals / evidence events, the carried-action transition rule, over-age add-demotion with `action_source` — plus the per-card selection UI and analysis-vintage stamps. Move the evidence-event boundary to per-holding vintages in the same slice.
