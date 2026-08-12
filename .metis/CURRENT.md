# Current session handoff

## What happened

**Slice B shipped — the Step 7b repair is fully built** (`e650274`, direct to `main`,
pushed). A construction-failed run now persists its verdicts, audits, and 7a aggregates
as a **degraded row** — `aggregates: Some` + `construction: None`, the one predicate
`PortfolioRun::has_constructed_book` — terminally failed, listed with a quiet "no book"
tag, openable read-only, **excluded from `latest_run`** (the store fn all four consumers
route through). `PORTFOLIO_RUN_RETENTION` is 30, one cap. The **accepted-lean
`moved-intrinsic` app-stamp** sits beside `reverted_to_lean`: Step 7b owes a model
attribution only where it overruled the lean. `carried_action`'s two copies are one
`pub(crate)` helper carrying the dual-meaning rule.

Four review rounds hardened it: the internal reviewer caught an incomplete retention
sweep; Codex rounds forced the transactional degraded persist and, twice, the
presentation truth — a degraded run's actions are **pre-construction values** (fresh
lean / carried action, possibly rule-demoted / role-risk placeholder never authored),
so the priced kicker reads "Per-holding read", the role-risk action is suppressed
("construction failed to validate a plan"), and every "standalone leans" enumeration
was corrected. Degraded rows carry `record_usage`, so digest compression is now
targetable.

## Current state

Nothing in flight. Working tree clean, `main` pushed at `e650274`.

**BUILD.md is stale in two known spots** (user-run edit pending): §Runtime's run-tracker
invariant exception is marked *Planned* but is now built, and §What remains item 1 still
says the persistence half "stands between here and a second attempt".

**Next queue item: re-run only the violating names** (disposition candidate 2) — ask the
construction re-run for corrected objects only for the violating symbols, shrinking
required output exactly when the violation list is longest. Then attempt 2. Behind it:
digest compression (targetable once a run persists — degraded rows now carry the
construction stage's prompt fill), `num_predict` as a truncation diagnostic, the
§Residue adapter-diagnostics gap, and the key-constants-vs-Rust-structs pin test.

Verification gate at ship: 1062 cargo tests, clippy clean at
`--all-targets --all-features`, `npm run build` clean, 46 node + 231 vitest.

## Open questions

- **Finding 5 — show the engine arm's chosen stand-in at Step 6f?** Still unruled; gates
  nothing; rides with the pre-run slice.
- **Were attempt 1's engine targets degenerate?** The one sample (SBUX) was steeply
  bearish, not flat. Attempt 2 should read its persisted targets first — and if it fails
  at 7b again, the degraded row now preserves the evidence.
- **Is the FMP dated-EOD rung de facto primary?** Unresolved — `data-health` has never
  persisted; a degraded row would now carry it even on a 7b failure.
- **Live-evidence caveat** — the sector-P/E walk-back's "holidays serve carried values"
  warrant rests on the adapter's 2026-07-16 verification, not re-probed.

## Where to start

Update BUILD.md's two stale spots (user-run), then plan the **re-run-only-violating-names
slice**: the seam is `portfolio/job.rs`'s named-violation re-run and the construction
prompt/schema — the re-run currently resends the full book and demands the full plan
back. Only after that lands, re-attempt the big run; read the SBUX-shape engine-target
question from whatever it persists before anything else.
