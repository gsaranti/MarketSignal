# Current session handoff

## What happened

**One slice landed** — the reduce-prompt size check (`a6dc1fc`). The plan's
shape held: an issue guard at the adapter seam (`distill_route` — within the
fast tier's budget a 6d call issues there, over it on the resident reasoner as
a model choice, over the widest budget refused as an unclassified failure that
fails the run), covering every 6d call. Review moved the boundary twice. The
internal review found the guard's rendered-prompt measure stricter than the
routing's content sum, so on the default roster a single-pass prompt that had
issued and fit was now refused — fixed by falling to the next smaller shape
when the *rendered* prompt outgrows the budget (single pass, then tier-1 off
Codex round 1). Codex round 2 caught that comparing against the *fast* budget
spent the sub-distillation cap on prompts the reasoner could serve — fixed by
`issue_budget_chars`, the widest issuable budget, so a smaller shape is taken
only where the guard would refuse. "Front-truncation unreachable" was ruled an
overclaim (chars-per-token is an estimate) and qualified. Lessons: a fallback
to a smaller shape compares against the budget the guard would refuse at,
never the routing budget; a categorical doc claim needs a provable bound or a
qualifier; check the record's numbering before queueing (I14 is taken — I17 is
the next free Codex item).

## Current state

Nothing in flight; `main` at `a6dc1fc`, tree clean, pushed. Queue ahead of the
run (record §Disposition): **two slices** — the resume prompt usage (its retry
events riding with it, as ruled) and the IPv6 fetch — then Codex I1–I16 and
the §A4 seed edge; a batch never mixes code and doc findings. I15's shape
(wire / retire) is ruled at its own plan; I16 is the required-`f64` audit with
a store round-trip regression. Carried untouched: the cloud report job's
unguarded `run_job` seam; the negative composite yield; `progress.rs`'s
poisonable terminal-leg locks; the `ok` tracker row's dropped-count detail;
`trade-opportunities-logic-flow.md:397` still says the tree-level reduce is
"never sized" (true for the unbuilt TO job — touch when its seam lands);
`/api/tags` probes on the 600 s backstop; seed passes the whole prior ledger
per topic; 6g qualitative trips un-trip unless re-researched.

## Open questions

- None new. `BUILD.md` and `INDEX.md` were bumped to Codex I1–I16 and the
  distillation issue guard indexed at this session-end (user-authorized).

## Where to start

`/metis-session-start`, then `/metis-plan-task` the **resume prompt usage** —
its bullet in the record's Priority-2 minor findings (grep `Resume loses`;
`CheckpointAccumulators`), one slice carrying its retry events as ruled.
Re-read every line anchor and every pointer's owning heading first. Present
the plan's assumptions and flags before implementing — the user rules on them
first. Keep the loop per finding (plan → implement → review → Codex → commit),
mark it resolved in the record with every Codex round named, sweep
`logic-flow-docs/` mirrors, and ask of every fix what stamp it moves: a
prompt-content change bumps `PROMPT_VERSION` with its history paragraph and
the watch-set stamp line; a grade-band change appends a
`GRADE_PARAMETER_HISTORY` row; a stored-target basis change bumps the targets
stamp. Do not launch or propose the big run — the user names that session.
