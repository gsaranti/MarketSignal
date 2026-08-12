# Current session handoff

## What happened

**Both blocking rulings were made, and the unblocked half of the attempt-1 fix queue shipped**
(`main`, five commits, `1e75ec8`..`808b9ae`).

The rulings, written up in full at `docs/verification/2026-08-10-big-run-attempt-1.md` §Disposition:
a construction-failed run **persists its verdicts** while staying terminally `failed` — a
`portfolio_runs` row visible in history, openable read-only, **excluded from `latest_run`** —
with `PORTFOLIO_RUN_RETENTION` 10 → 30 and degraded runs counting against the one cap; and the app
**stamps `moved-intrinsic`** where construction accepted this run's lean, extending the
`reverted_to_lean` precedent. The missing "declined an engine exit" context cause was ruled
**deferred until after the run**, so it is shaped by evidence rather than speculation.

Built: findings 2, 3, 4 and 6 — schema contracts, the version-vocabulary line, the holding header,
and tracker request routing. Three of the record's own claims were wrong and are corrected in place:
finding 2's defect sits at **three** prompts including the whole-book construction call, finding 4's
name fallback needed the profile name threaded rather than a prompt edit, and finding 6's proposed
default is unsafe as literally stated.

**The lesson that should govern the next slice:** every defect this work produced — six across eight
Codex rounds — was a rule enforced in one place and restated in another. The last three commits stop
aligning those pairs and remove them, generating each prompt's response contract from the same
constant its schema is built from.

## Current state

Nothing in flight. Working tree clean, `main` synchronized.

**Slice B is next and fully scoped.** Three code seams: the `latest_run` filter;
`merge_validated_actions` / `carried_action`, where the action field means "7b-blessed final"
downstream but holds the **standalone lean** before the merge — the reason the `latest_run` exclusion
is load-bearing rather than tidy; and the app-stamp beside `reverted_to_lean`. Two spec edits —
`portfolio-workflow.md` §Step 7b's "persisting incoherence fails the run", and `interface.md`'s "a run
row appears only on persisted success" — plus five doc sites citing "the 10-run retention".

Still queued behind it: re-run only the violating names; **digest compression, which waits on a run
persisting** so `record_usage`'s per-stage prompt fill can target it; `num_predict` as a truncation
diagnostic, not headroom; the §Residue adapter-diagnostics gap; and a test pinning the key constants
against their Rust structs (flagged, not built — it fails loudly at parse time).

**Dev DB is still deliberately unchanged** — only the 07-31 run, the sole pre-`grade-v2.1` stamp.
Its 36 sell-alls of 44 remain attempt 2's attribution baseline. Attempt 1's scratch evidence (120
tracker captures, the Ollama server log, `analyze-run.sh`) was **not copied out and is gone**; the
written analysis is all that survives.

## Open questions

- **Finding 5 — should the current engine arm's chosen stand-in show at Step 6f?** Unruled. The prior
  run's pick renders and this run's does not; showing it may anchor the model arm against
  `portfolio-v7`'s intent. Gates nothing, so it can ride with the pre-run slice.
- **Were this run's engine targets degenerate?** The one sample (SBUX) was steeply bearish, not flat —
  a different shape from 07-31. Nothing persisted to settle it; the repeat attempt should read it first.
- **Is the FMP dated-EOD rung de facto primary?** Still unresolved — `data-health` has never persisted.
- **Live-evidence caveat** — the sector-P/E walk-back's "holidays serve carried values" warrant rests
  on the adapter's recorded 2026-07-16 verification, not re-probed.

## Where to start

Plan slice B before writing code, and **write its tests first**. Unlike slice A's defects, these fail
silently — a degraded run leaking into `latest_run` surfaces as a wrong baseline several runs later,
not as a bad line in a diff. Pin the pairs up front: `latest_run` against `list_recent_runs`, and the
action field against both its meanings. Then take the re-run-only-violating-names fix, and only
re-attempt the run once Step 7b can carry a whole-book plan at book scale.
