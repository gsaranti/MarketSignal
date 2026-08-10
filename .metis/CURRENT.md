# Current session handoff

## What happened

**The BUILD.md half of the audit is done, merged and pushed** (`98483fa`). BUILD.md went 1,218 → 667 lines (~21.6k → ~11k tokens), and now carries **zero** dates, PR references, commit hashes, branch names or review-round counts (was 76 / 23 / 20 / 15).

**Audit ruling 1 resolved by evidence, not preference** — compress in place, because nothing needed relocating: of 142 bolded concepts in the chronicle, 85 appear verbatim in `docs/`, and 15 of 16 sampled non-matches have a `docs/` home under BUILD's own paraphrase. **The user's framing is the durable part:** BUILD tracks what is done and what is left, never how something was implemented, and never a date or hash.

Compression removed things a planner uses, so **§Seams a plan builds on**, **§Standing constraints** and **§What each built slice left for the next** were added — module homes, shared entry points, and version *constants* rather than their current values. §What remains is now a complete ledger, collecting nine unbuilt items that had been scattered through the body.

**The lesson from eight Codex rounds, all approved by the end:** every single finding was a scope error inside a *restatement* of a contract `docs/` owns — never in a pointer, seam, or constraint. Compressing two per-job statements into one sentence is exactly what drops the qualifier doing the scoping work. The fix was to stop restating and start pointing. The same rule now binds inside the file: one obligation had been stated three times and drifted between copies.

## Current state

Nothing in flight. Tree clean, `main` == `origin/main` at `98483fa`, no other branches. Full gate set re-run before commit: cargo **1050 / 0 failed / 28 ignored**, clippy clean, `npm run build` clean, **46 node + 225 vitest**.

Queue: **INDEX.md audit → the big confirmation run.**

**INDEX.md audit — the ruling is already settled: rewrite in place, never a prune.** Rows get shorter, none disappear. Scope: **15 rows over 1,000 chars, longest 3,209**, against a header stating rows are "lookup pointers, not summaries: open the cited doc section rather than working from the clause here." Like BUILD, it audits against a charter it declares itself — a conformance check, not a taste argument.

**Method that transferred from the BUILD half, and should again:** verify every claim against code and `docs/` *before* rewriting, and sweep the class rather than the cited instance. That found two stale claims this session — `research_forward_assumption` is docs-only, never built; and the `run_date` / `ScoredLabel.labeled_at` "stays UTC" note was **disproven** (`job.rs` derives one ET session date and `outcome.rs` copies it into `labeled_at`). INDEX's rows are older accretions than BUILD's, so expect the audit to be as much a correctness pass as a compression one.

**Open items now live in BUILD.md, not here** — §Remaining in order, §Owned by no slice, §Awaiting a ruling, §Deferred by decision. The big-run watch set moved to `docs/verification/big-run-watch-set.md`. Don't re-list either here.

## Open questions

- **Do INDEX's long rows compress the way BUILD's chronicle did?** The hypothesis is yes — they summarize `docs/verification/` records, so the content already has a home. Test it before assuming it; that is what made the BUILD half safe.
- **Live-evidence caveat** — the sector-P/E walk-back's "holidays serve carried values" warrant rests on the adapter's recorded 2026-07-16 verification, not re-probed. If invalidated, the cardinality claim moves with it.

## Where to start

Run the **INDEX.md audit** against the charter its own header states, under the settled rewrite-in-place ruling. Verify each row's claims before shortening it — several are old enough to have drifted.

Then the **big confirmation run** (dev app, process name `market-signal`), reading `data-health` early against `docs/verification/big-run-watch-set.md`. Stooq's PoW interstitial may have made the FMP dated-EOD rung de facto primary, and the run's evidence is what decides the rung-order slice.
