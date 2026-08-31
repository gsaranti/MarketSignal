# Current session handoff

## Active task

Handling the findings from the 2026-08-30 big-run attempt
(`docs/verification/2026-08-30-big-run-findings.md`). **Findings 1 and 2 are
done**; Findings 3–6 remain.

## What happened

This session handled **Finding 2** (ledger `quant` under-population) end to end.
The root cause was prompt-side: the ledger-authoring prompt named the
machine-evaluable falsifier only as a concept, never the grammar-constrained
`quant` object, so a debut holding wrote a numeric threshold into the statement
prose and left `quant` null — a mechanically-evaluable falsifier silently
degraded to prose. Rewrote the ledger prose (both interpretation branches) to
name the `quant` object + its four fields, state the prose-only anti-pattern,
give the decimal-scale example (gross margin below 16% is 0.16, not 16),
describe the falsifier-only `technology_class`, and name the falsifier/trigger
field split so the prose mirrors the schema (`quant` on both, `technology_class`
on falsifiers, `family` on triggers).
A cross-prompt audit (the user's question) of the job's other schema-carrying
calls found the **distillation** call is the structural twin (nullable typed
numeric side-channels beside a free-text sink) but already well-specified from
prior review rounds; added two hardening touches anyway — the distillation
typed-field anti-pattern line and `what_changed_entries` field naming.
`PROMPT_VERSION` bumped to **`portfolio-v31`**. Two Codex rounds were remediated
(`technology_class` scoped falsifier-only; `family` described as the add/trim/sell
family, not an action rung the trigger pre-commits), and the watch set refreshed
(`portfolio-v31`; checkpoint history extended to `checkpoint-v8`, last session's
SearXNG-only bump that had gone unrecorded there).
Committed `17245ef` and pushed. `BUILD.md` was assessed and left unchanged — a
prompt-prose calibration, no contract/schema/module change, and it cites
`PROMPT_VERSION` by constant.

## Current state

Nothing in flight. Finding 2 committed and pushed.
The big-run re-attempt gate is still **met** and unchanged — a re-attempt stays
the user's call, from a wiped store per `docs/verification/big-run-watch-set.md`
(`checkpoint-v8` refuses the old v7 trail).
**Findings 3–6 remain**: Finding 3 (action-call prompt friction) is the natural
next *code* slice; Findings 4 (fired bounded-retry rate), 5 (throughput /
per-holding research budgets), and 6 (extraction telemetry for the deferred
render tier) are largely **gated on a full run's telemetry**, not code.
Per-holding research budgets stay **deferred** to the full run for calibration.
The prior unrelated carried follow-ups remain untouched — the cloud `run_job`
seam, negative composite yield, `progress.rs` poisonable locks, the tracker `ok`
row's dropped count, TO logic-flow line 397, the 600 s `/api/tags` backstop,
whole-ledger seed injection, qualitative 6g un-trip semantics, an IPv6-loopback
wire test, the audit sources line, and the unreconciled-delete fail-soft
sentence's home.

## Open questions

- Whether and when to re-attempt the big run — the user's call; the gate is met,
  but whether/when stays the user's decision (don't propose it unprompted).
- Whether a second full pass runs — the user's call after a *first full* run's
  result (none exists yet).
- Which of Findings 3–6 to take next — Finding 3 is the next code candidate; 4–6
  want a full run's data first.

## Where to start

Do not propose re-running unprompted. To continue the findings work, take
**Finding 3** (action-call prompt friction): tell the action prompt that the app
app-stamps a departure from the ENGINE SET (so the model stops re-deriving the
annotation and just picks its best rung), and watch the `indeterminate`
capital-efficiency read leaking in as a soft sell-lean. Read
`docs/verification/2026-08-30-big-run-findings.md` §Finding 3 first. Findings 4–6
need a full run's telemetry, not a code change.
