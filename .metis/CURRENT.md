# Current session handoff

## What happened

**The selective re-analysis slice SHIPPED — merged to `main` as PR #57 (squash `3603b01`), feature branch deleted.**
Planned via `/metis-plan-task` (the code exploration surfaced that **no 7b construction stage exists in code**, so the carried-action transition rule's model-facing validation was scoped to the 7b slice by plan), implemented, then converged: internal review approve-with-nits (all fixed — carried-stamp title copy, panic-safe `over_age` slicing, an abstention-vintage test, reciprocal `OVER_AGE_DAYS` comments), **Codex round 1 changes-requested with both findings verified real and fixed** — carried verdicts now **recompute action sizing at current weights** on both branches (sizing is engine context, never carried stale), and `action_source` renamed to the **canonical `model-chosen` / `rule-demoted`** vocabulary (docs §Outcome learning; I had invented `analytical` without checking) — round 2 approved.
Load-bearing shapes: per-holding vintages (`analyzed_at` + `effective_vintage`; abstentions preserve their prior vintage so the evidence-event boundary never advances past unexamined events), the boundary itself now per-holding in both sweep paths, the subset-capable sweep (`quick_check::sweep_tail`, reusing the run's rate prints), the carried-audit-row carry (`quick_basis`/`fund_exposure` survive), and the persist seam retaining carried sweep state re-stamped.
BUILD/INDEX caught up this session — **no capture debt**.

## Current state

`main` at `3603b01` (+ this session-end commit), tree clean, nothing in flight. Verified at merge: cargo 807 / clippy 0 / npm build clean / 40 node + 181→188 vitest. Queue head: the **pre-profit overlay** slice.

## Open questions

- **`PriceOutsideBand` on an authoring-time-outside band:** a holding whose engine bear–bull targets don't straddle spot flags immediately and will **force-include on every selective run** until re-analyzed — design question (signal or noise?) before the big run; rides the BUILD big-run checklist.
- **Accepted cosmetic nits:** retained quick-state `last_checked_at` uses the run's `created_at` (skew vs the sweep's actual time); tracker step relabels "Check X"→"Analyze X" on force-includes.
- **Carried-verdict float noise:** serde_json's default float parsing drifts 1 ulp on the store round-trip — never compare carried numerics exactly (bit us twice in tests; `float_roundtrip` feature would close it).
- **Docs-capture candidates (carried):** the earlier rounds' code-enforced-but-docs-unpinned contracts (None-with-no-gap non-payer; mandate-vs-label comparability + blank-metadata normalization; unevaluable→family downgrade) + the standing set (FINRA-leg vacuity; cash-flow re-pull unconsumed; breadth-flip unbuildable; the ledger slice's three). Canonical docs homes still unwritten.
- **Debut gaps (self-resolve at the big run's persist):** first sweep reads the rate-anchor family `unknown` (pre-`RatePrints`) and pre-basis funds read FundInfo `unknown`.
- `job_status` has no `job_type` filter — quick-check runs move the footer's last-run timestamps (accepted).
- **No A letters under grade-v2** (META 84.0 vs ≥ 85) — reserved for normalization or the big run.
- **Carried unchanged:** big-run checklist in BUILD §What remains (now incl. the selective legs); reasoning-pane DOM weight; encrypted portability round-trip; step-17 embedding watch; 600 s stress; scorecard display; dev-store residue; Keychain fail-soft; stage-and-swap import; chain both-maps invariant; four-part verdict bound; §1 open drafts; fraud-producer posture; fund-slice drafted constants; checkpoint/resume + the 6g input-delta validator (docs-promised, unbuilt — the 7b slice now has `action_source` + vintages persisted to build the transition-rule validation on).

## Where to start

Run `/metis-plan-task` for the **pre-profit overlay** slice (docs/portfolio-analysis.md §The per-holding pipeline, §Starting parameters; portfolio-workflow.md §Step 6b–6g, §Step 7a): the deterministic execution/financing overlay — statement-derived runway / margin / capex / dilution plus app-validated period-keyed operating observations, the bounded latest-four-period backfill on a first/history-thin pass, the ≥5% / ≥2-period / ≥20% miss rules, the conjunctive severe state, and the Medium/Low caps + add-bar consequences validated at 6g/7a. The two display-only UI micro-slices (Portfolio-page polish; section-scoped footer + report-nav) remain available as breathers.
