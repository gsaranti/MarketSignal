# Current session handoff

## Active task

The Portfolio essay design is written into `docs/`, and the human-readable logic flow now matches it.
Next is the Metis plugin upgrade and the `.metis/` migration, then `/metis-plan-task` for the implementation slice.

## What happened

`logic-flow-docs/portfolio-analysis-logic-flow.md` was rewritten to the essay design (`8e21acd`, 1,694 → 1,254 lines): one coherent designed flow with no build markers or ruling dates, step numbering mirroring the workflow's Steps 1–8, a table of the eight model calls with a sees/returns block each, every engine rule and threshold kept with the derivations trimmed.
Its review produced one design ruling (`6756744`): the accuracy checks run **before the per-holding loop** (end of Step 5, through the price-bar cache) so this run's self-reviews read every check that has come due, and only episode opening stays after the loop; `docs/portfolio-workflow.md` (Step 5, Step 7 renamed "Open Episodes, then Persist Run and Audit", the 6a/6b/6e glosses), `docs/portfolio-analysis.md` §Outcome learning and §Starting parameters, and `docs/storage.md` carry it.
The read-through surfaced seven `docs/` inconsistencies, still unfixed; auto-memory `logic-flow-rewrite-and-docs-nits.md` lists them.

## Current state

Working tree clean at `6756744`; code unchanged since `a9f5037` (debut stamps `portfolio-v67` / `checkpoint-v16`, portability 12).
Nothing of the redesign is implemented.
`BUILD.md` and `INDEX.md` still predate the design — the ledger, the typed model→engine channels, the scoreboard and calibration path, the vector lane, the stand-in arm, the debut document; INDEX rows for the dropped sections — and do not carry the accuracy-checks ruling.
The user is upgrading the Metis plugin from 0.5.2 between sessions and updating `.metis/config.yaml`, `CLAUDE.md` and `AGENTS.md` by hand; every other `.metis/` file stays in the old shape until migrated.

## Open questions

- Which new-Metis files replace `BUILD.md` and `INDEX.md`, and how the migration carries the design content (the removed machinery out, the essay design and the accuracy-checks ruling in) in the same pass — settled by reading the upgraded skills next session.

## Where to start

Run `/metis-session-start`; the mismatch between the upgraded skills and these old-shape `.metis/` files is expected, not an anomaly — report it in one line and run no `/metis-init` or migration unprompted.
Then read the updated Metis skills the user names and migrate `.metis/` with the design content updated in the same pass (auto-memory `metis-upgrade-transition.md`).
`/metis-plan-task` for the implementation slice follows the migration; attempt 9 stays the user's call after the slice and a store re-wipe; never propose the run.
