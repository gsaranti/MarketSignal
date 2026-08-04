# Current session handoff

## What happened

**Open-questions sweep #2 SHIPPED and pushed (`eb9295c`)** — the three questions not blocked by the big run, decided via the selection UI (every recommended option taken), landed as one commit:

- **driver_ladder wire-order parity fixed**: `engine::canonicalize_statements` is now the shared statement policy's named home — both quarterly statement vecs sorted (period_end, filing_date)-desc + period-end-deduped **in place** at the `apply_ttm_statement_basis` choke point (every statement-consuming path passes it before any engine read, TTM adoption or not), so the driver ladder's growth-clamp trailing prints / share basis and the anchor windows inherit canonical order. `pre_profit::statement_inputs` keeps its local sort deliberately — order-independence is a test-pinned standalone contract. +4 tests.
- **Research-loop activation obligation** confirmed as-recorded (riding the research-loop slice) — no change.
- **Docs capture, both candidates**: the every-priced-stock three-state eligibility record (stock surface only — a priced fund carries none; derived states ride every record, meaningful only on an eligible read) → portfolio-analysis §Starting parameters + storage.md; the abstention-survival mechanism (**fresh engine recompute + carried observation history, never frozen carry** — engine-only, computed before the floor check) → §Evidence floor; Step-8 pointer in portfolio-workflow.md.

One Codex round: approve w/ one Low docs finding (the workflow pointer's priced-holding scope; serialized-neutral-defaults precision) — both legs verified against code, then fixed.
Verified: cargo 851/0 (819 lib + integration), clippy 0, npm build clean, 40 node + 188 vitest.

## Current state

`main` at `eb9295c` (+ this session-end), pushed, tree clean, nothing in flight.
**Capture debt: BUILD one paragraph behind** — §Watches still names the driver-ladder raw-order read as an unscheduled parity cleanup (done at `eb9295c`; the canonicalization's home is `engine::canonicalize_statements` at the TTM-basis choke point). INDEX pointers still resolve.
Queue head: the **outcome learning** slice.

## Open questions

- **Research-loop activation obligation** (recorded in `pre_profit.rs`'s validator doc comment + BUILD): holding-identity + source-text observation validation and a period-normalization hard rule are mandatory before the pre-profit producer goes live.
- **Live-run calibration watches**: STI-absent-reads-zero liquid-resources convention; YoY diluted-share quarter-contiguity — both ride the big-run checklist's overlay leg.
- **Debut gaps (self-resolve at the big run's persist):** first sweep reads the rate-anchor family `unknown` and pre-basis funds read FundInfo `unknown`.
- **No A letters under grade-v2** (META 84.0 vs ≥ 85) — reserved for normalization or the big run.
- **Carried unchanged:** big-run checklist in BUILD §What remains; reasoning-pane DOM weight; encrypted portability round-trip; step-17 embedding watch; 600 s stress; scorecard display; dev-store residue; Keychain fail-soft; stage-and-swap import; chain both-maps invariant; four-part verdict bound; §1 open drafts; fraud-producer posture; fund-slice drafted constants; checkpoint/resume + the 6g input-delta validator (docs-promised, unbuilt).

## Where to start

Amend BUILD's §Watches paragraph first (drop the driver-ladder raw-order watch — done at `eb9295c` — naming `engine::canonicalize_statements` as the shared home), then run `/metis-plan-task` for the **outcome learning** slice (docs/portfolio-analysis.md §Outcome learning + the outcome-learning constants in §Starting parameters; portfolio-workflow.md §Step 7a, §Step 8; storage.md §Local Analysis Suite Storage): recommendation-state-keyed decision episodes with the calibration-feature snapshot, engine-computed labels (total-return primary / price-only common basis, each read on a declared basis and cohort layer), intrinsic calibration keyed on the standalone lean — never the construction-shaped final action — feeding a propose-only calibration.
The two display-only UI micro-slices (Portfolio-page polish; section-scoped footer + report-nav) remain available as breathers.
