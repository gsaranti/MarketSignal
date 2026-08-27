# Current session handoff

## What happened

**F1 resolved** (`21e7a19`): the split-adjustment bridge now covers every
stored-basis price comparison. Each full pass stamps an anchor bar
(`HoldingAudit.authoring_close`, both branches); re-fetching the same bar
date gives the exact re-basis factor (`engine::split_bridge_factor`, drafted
10% deadband, exactly 1.0 in the unchanged case). The sweep converts
transiently (ledger `price` thresholds+margins, multiple rescale, hurdle
re-anchor + same-sweep filing dividend, frozen band, revision comparator);
the full pass normalizes the ingested prior ledger once at ingestion and
bridges prior spot + consensus mid together. Four Codex rounds hardened the
unresolvable-bridge posture into one invariant — *a bridge factor applies
only to values whose basis the same anchor certifies*: price conditions gate
out whole, the prior anchor carries forward, a re-anchored price core
downgrades (validator-enforced), fresh comparators are withheld (quick
basis, monitor target stamps), the cross-pass target delta row requires the
prior pass's certified authoring spot, and the next sweep reads the withheld
row's families `unknown`. Declined with reasons recorded in §F1: app never
rewrites statement prose (core-beside-statement render = named candidate, a
`PROMPT_VERSION` event); no pre-field-vs-couldn't-stamp distinction. No
version bumps. Canonical contract: `portfolio-analysis.md` §Starting
parameters.

## Current state

Nothing in flight; `main` at `21e7a19`, tree clean, pushed;
`PROMPT_VERSION` = `portfolio-v13`. Of the record's four pre-run items C1,
F3, and F1 are done; only the **retry posture** remains. Remaining findings
in severity order: **F2** (outcome-label end bar unbounded by staleness —
fix-shaped invariant already in the record: bound `end_bar.date` to
`w_end − COVERAGE_TOLERANCE_DAYS` and require `end_bar.date > entry.date`;
`bench_return` has the same shape), A1–A4 (logic-flow doc pass), the minors,
and Codex's I1–I9 (unverified by a Claude session). Carried untouched:
`/api/tags` probes on the 600 s backstop; seed passes the whole prior ledger
per topic (doc↔code drift vs `portfolio-workflow.md` §Step 6c); 6g
qualitative trips un-trip unless re-researched.

## Open questions

- **Does the big run wait** on the retry posture alone now, or run with it
  recorded in the watch set?
- **Retry posture** — bounded retry-once on local-model calls vs the hard
  posture (C1 no longer multiplies it).
- **One-month band** — unscaled daily vol × 2 marked "v1 mechanics":
  deliberate retention or √t scaling?
- **Fix grouping** — keep one-at-a-time, or batch the minors and A1–A4.
- **Core-beside-statement render** (6c seed / 6d citation list) — named
  candidate from the F1 rounds; a `PROMPT_VERSION` event needing its own
  decision.
- Carried: runtime auto-start/spin-down; the 6e supersede leg structurally
  dead; channel promotion criteria; research budgets calibrate on the run.

## Where to start

`/metis-session-start`, then either settle the **retry posture** (the last
pre-run item — a decision, then at most a small slice) or `/metis-plan-task
F2` (the record's §F2 names the invariant; `outcome.rs` `covers_through` /
`close_at_or_before` / `bench_return` are the surfaces, reachability gated on
a source-side range clamp or stale cache). Keep the loop: plan → implement →
review → Codex → commit, and mark the finding in the record. The big-run
watch set still needs its 2026-08-20/24 additions before the run.
