# Current session handoff

## What happened

**The INDEX.md half of the audit is done, merged and pushed** (`c713e58`). INDEX.md went 314 → 300 lines (~24.8k → ~8.3k tokens) and carries no date outside a cited filename. Both Metis-file audits are now landed; the docs corpus and its map are clean going into the run.

**The prior session's settled ruling was reversed, on evidence.** "Rewrite in place, never a prune" did not survive contact: 23 rows had a *slice, record or review batch* as their subject, and every Portfolio one already had a concept row citing the same doc sections. They were duplicated navigation wrapped around build history, so they were deleted, not shortened. Four concepts reachable *only* through a chronicle row gained rows of their own — Portfolio's two-arm verdict, grade bands + parameter versioning, ET session dating, run data-health.

**Pointers are the payload, so every revision was checked by diffing the full `file.md §section` set against the previous one.** That caught the single regression the audit could have caused — `portfolio-analysis.md §What changed`, cited only by a deleted chronicle row. Worth reusing on any future INDEX edit.

**Three rules now sit in INDEX's header, one per review round**, because each round found the *class* after the previous fix had addressed only the cited instances: a row never states what a concept does (binding the subject as hard as the parenthetical — subjects are noun phrases, not propositions); a parenthetical only disambiguates or names the canonical section (113 glosses failed, 14 survive); and naming is not specifying.

## Current state

Nothing in flight. Tree clean, `main` == `origin/main` at `c713e58`, no other branches.

Queue: **the big confirmation run** — the gate everything else waits behind. The locked pre-test block is fully built; nothing in `BUILD.md §Remaining` sits ahead of it.

**Gates were not run this session and could not be** — the change was `.metis/INDEX.md`-only. The last recorded green set is from `98483fa` (cargo 1050 / 0 failed / 28 ignored, clippy clean, `npm run build` clean, 46 node + 225 vitest). Re-run in full before the next code commit rather than trusting that line.

**Open items live in BUILD.md, not here** — §Remaining in order, §Owned by no slice, §Awaiting a ruling, §Deferred by decision. The run's checklist is `docs/verification/big-run-watch-set.md`. Don't re-list either here.

## Open questions

- **Live-evidence caveat** — the sector-P/E walk-back's "holidays serve carried values" warrant rests on the adapter's recorded 2026-07-16 verification, not re-probed. If invalidated, the cardinality claim moves with it.
- **Is the FMP dated-EOD rung de facto primary?** Stooq now serves a JS-PoW interstitial to non-JS clients. Stooq stays the primary rung by user decision; the run's `data-health` read is what decides the contingent rung-order slice.

## Where to start

Run the **big confirmation run**. Drive the dev app by process name (`market-signal`), never `tell application`. Read `data-health` **early** — several watch-set items resolve off that surface alone, and the run banks every stacked confirmation at once, so a missed read costs a repeat run. Work against `docs/verification/big-run-watch-set.md`, grouped by producing surface. Findings go into a new dated record under `docs/verification/`, which the watch set is the index of.
