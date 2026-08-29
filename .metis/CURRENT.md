# Current session handoff

## What happened

**Codex I5 landed** (`e89b166`). The per-holding action call now receives
**both arms' price targets, both horizons**: `implied_moves_section` renders
the engine's and the model's one-month and twelve-month implied bear/base/bull
moves against spot under one usable-spot guard (finite, positive). An engine
leg the scenario function could not derive prints `(gap)`; a model leg outside
the domain — non-finite, non-positive, or a finite level whose move from spot
overflows (Codex round 1) — prints as authored with an off-scale tag, an
inverted band carries its own tag, and the two tags are independent reads
(annotate, never reorder, never drop — the frontend's posture; I6 owns the
upstream domain validation). Provenance is engine-scoped by label; the system
prompt names BOTH arms and how each is weighed. `PROMPT_VERSION` →
**`portfolio-v19`**; no other stamp. Copy-out of both stores: neither has a
`portfolio_checkpoints` table, so no trail existed. Six rulings, one reviewer
round (five notes, four closed), two Codex rounds. Lessons: a render's
fail-closed guard must read the *derived* value, not only the input (a finite
`1e308` over a $1 spot printed `+inf%`); a doc sentence must separate what the
render shows from what the prompt tells the model to weigh. `BUILD.md`'s count
line bumped to I6–I13, I15–I19 and `INDEX.md` corrected and extended
(user-authorized at session end).

## Current state

Nothing in flight; `main` at the session-end commit, tree clean, pushed. Queue
ahead of the run (record §Disposition): **Codex I6–I13, I15–I19** and the
**§A4 seed edge**, one finding per slice, a batch never mixing code and doc
findings. I18 and I19 are ruling items; I15's shape is ruled at its plan; I16
the required-`f64` audit; I17 the telemetry row pattern. I6 (model-arm numeric
domains are prompt-only) will make I5's off-scale render branch unreachable
upstream — the render guard stays as the fail-closed read. Carried untouched
(unchanged): the cloud report job's unguarded `run_job` seam; the negative
composite yield; `progress.rs`'s poisonable terminal-leg locks; the `ok`
tracker row's dropped-count detail; `trade-opportunities-logic-flow.md:397`
"never sized"; `/api/tags` probes on the 600 s backstop; seed passes the whole
prior ledger per topic; 6g qualitative trips un-trip unless re-researched; an
IPv6-loopback wire test. Watch set: the attempt-2-prior line stays true;
records now stamp `portfolio-v19`. Named residuals: FY periods normalize to
12-31 (I4); the one-month leg's methodology still reaches neither model nor UI
(I10).

## Open questions

- None new. The INDEX row candidates closed this session: the §Verification
  row moved to I1–I19, and rows were added for the guidance vintage policy,
  the pre-profit observation source excerpt, and the fund history in-quarter
  sample admission — trim any that reads as more than a pointer.

## Where to start

`/metis-session-start`, then `/metis-plan-task` **Codex I6** (the declared
model-arm numeric domains are prompt-only, so out-of-domain values derive
letters and enter scoring — confirm it is the first `### I` section with no
`Resolved` line) and re-read every line anchor and every pointer's owning
heading first, checking whether a later slice already closed it. Present
assumptions and flags before implementing — the user rules first. Keep the
loop per finding (plan → implement → review → Codex → commit), record every
Codex round, sweep `logic-flow-docs/` mirrors, and ask of every fix what stamp
it moves across the four axes (prompt content now `portfolio-v19`, grade band,
stored-target basis, floor rule `evidence-floor-v3`; the overlay stamp
`pre-profit-v3`). Do not launch or propose the big run — the user names that
session.
