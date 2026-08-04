# Current session handoff

## What happened

**The open-questions sweep SHIPPED — all five questions not blocked by the big run decided (selection UI; every recommended option taken) and landed as one commit `56e5be5`, pushed to `main`.**
1. **`PriceOutsideBand` is transition-only:** 6g stamps spot's authoring-time band relation (`ThesisLedger.authored_band_relation` — inside / below-band / above-band, serde-default; pre-stamp ledgers read authored-inside) and the sweep flags only on a relation *change* (leave / re-enter / side-cross), so an authored-outside band no longer force-includes every selective run.
   Fixture consequence: the selective tests' default sweep price moved 170→195 — the old in-band default now reads as a genuine re-entry transition.
2. **serde_json `float_roundtrip` enabled** — store JSON round-trips bit-exact; the never-compare-carried-numerics-exactly rule is retired (store pin test + exact carried-verdict comparisons).
3. **`job_status` excludes `portfolio_quick_check` rows** from the footer's four last-run stamps; failures still reach the failed-jobs warning.
4. **The two cosmetic nits accepted permanently** (retained `last_checked_at` on run `created_at`; Check→Analyze relabel) — closed, no code.
5. **Docs-capture pass cleared the whole backlog** — monitor-level goalposts, streak observation identities, None-with-no-gap non-payer, no-cash-flow-re-pull truth, FINRA leg recorded dormant, `unevaluable_series` family downgrade, fund-comparison gating + blank-metadata normalization, breadth-flip sub-leg marked inert; the consecutive-count constants were found already pinned (trade-opportunities.md §The opportunity).
One Codex round: **approve, no code findings**; the one Low docs nit (FINRA row present-tense clause) fixed.
Verified: cargo 811/0, clippy 0, npm build clean, 40 node + 188 vitest.

## Current state

`main` at `56e5be5`, pushed, tree clean, nothing in flight.
**Capture debt: BUILD/INDEX are one session behind** — BUILD's quick-check bullet still describes the outside-band trigger as state-based and its big-run checklist still carries `PriceOutsideBand` as an open design question (now answered in code; the leg becomes a runtime watch: the transition flag's live behavior, incl. re-entry flags, at 47-position scale); the footer-stamp exclusion and the float_roundtrip load-bearing feature are also uncaptured. INDEX's quick-check/selective rows need small amends.
Queue head unchanged: the **pre-profit overlay** slice.

## Open questions

- **Debut gaps (self-resolve at the big run's persist):** first sweep reads the rate-anchor family `unknown` (pre-`RatePrints`) and pre-basis funds read FundInfo `unknown`.
- **No A letters under grade-v2** (META 84.0 vs ≥ 85) — reserved for normalization or the big run.
- **Carried unchanged:** big-run checklist in BUILD §What remains (the `PriceOutsideBand` leg now a runtime watch, per capture debt above); reasoning-pane DOM weight; encrypted portability round-trip; step-17 embedding watch; 600 s stress; scorecard display; dev-store residue; Keychain fail-soft; stage-and-swap import; chain both-maps invariant; four-part verdict bound; §1 open drafts; fraud-producer posture; fund-slice drafted constants; checkpoint/resume + the 6g input-delta validator (docs-promised, unbuilt).

## Where to start

Run the small **BUILD/INDEX catch-up** first: fold the transition-only band flag (+ `authored_band_relation`), the footer-stamp exclusion, and the float_roundtrip pin into BUILD's as-built bullets, reword the big-run checklist's `PriceOutsideBand` leg from design question to runtime watch, and amend INDEX's quick-check/selective rows.
Then `/metis-plan-task` for the **pre-profit overlay** slice (docs/portfolio-analysis.md §The per-holding pipeline, §Starting parameters; portfolio-workflow.md §Step 6b–6g, §Step 7a) — the deterministic execution/financing overlay with the ≥5% / ≥2-period / ≥20% miss rules, conjunctive severe state, and Medium/Low caps + add-bar consequences.
The two display-only UI micro-slices remain available as breathers.
