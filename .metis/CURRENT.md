# Current session handoff

## Active task

Engine arm at three horizons — the first *To build* item, not started.
A docs-only Trade Opportunities refactor precedes its planning (see *Where to start*).

## What happened

The Metis plugin moved to 0.6.0 and `.metis/` was migrated by hand, no reconcile or build-spec run.
`SYNTHESIS.md` was rewritten to its final form: the target system only, nothing that goes stale as items complete.
`INDEX.md` was walked against the docs' headings: ten rows for concepts the essay design removed were deleted, nine re-pointed, four Trade-Opportunities-only rows moved to that section, fourteen design rows and twenty rows for never-indexed headings added; every citation resolves.
`BUILD.md` is now the two-list backlog: eleven items under *To build*, *Built* empty by ruling (built history is not carried; the old brief stays readable at `6d3b26e`).
The six Portfolio refactor items lead (engine arm at three horizons → the holding verdict → the research chain → outcome learning → the self-review → the removal sweep), then the three paid-tier report enrichments, the streaming-agent retry-once and the bounded baseline-history render.
The brief's two rules with no docs home moved to `CLAUDE.md` / `AGENTS.md`: the pre-release no-compat posture and three code disciplines.
Auto-memory was cleaned to match.

## Current state

Code unchanged since `a9f5037` (debut stamps `portfolio-v67` / `checkpoint-v16`, portability 12); nothing of the design is implemented and no item is in progress.
Trade Opportunities has no backlog items yet by decision: its docs still describe the machinery the Portfolio design retired, so the TO design is refactored first, docs only, and its items are derived afterwards.
Owed after that refactor: the INDEX rows in the TO section re-walked; the seven `docs/` inconsistencies in auto-memory `logic-flow-rewrite-and-docs-nits.md` proposed as one batch.
The big confirmation run (attempt 9) waits behind the whole Portfolio backlog and a dev-store re-wipe; the user names its session.

## Open questions

- What stays typed in Trade Opportunities: the matrix cell, the archive decision and the floor admission are structural — which stay an appendix, which become prose?
- What a TO episode scores: TO has no book, so picked and turned-away names can both be scored — does Portfolio's price-record episode cover it, or does a reduced shadow record survive?
- Does TO still need the embedder, or does its discovery memory go prose too, taking the embedder out of the roster?

## Where to start

Run `/metis-session-start`.
Then the Trade Opportunities docs-only refactor to the Portfolio strategies (prose model outputs, episodes, the shared research primitives): `trade-opportunities.md`, `trade-opportunities-workflow.md`, the TO logic-flow doc and the TO legs of the shared docs, under the docs-first cadence — propose per doc, go ahead, selector rulings, edit, diff review, Codex, commit on the user's word — with no Metis skills and no redesign language.
After it: INDEX's TO rows, TO items into `BUILD.md` behind the report items, then `/metis-plan-task "Engine arm at three horizons"`.
Never propose the run.
