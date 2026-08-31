# Current session handoff

## Active task

Handling the findings from the 2026-08-30 big-run attempt
(`docs/verification/2026-08-30-big-run-findings.md`). **Finding 1 is done**;
Findings 2–6 remain.

## What happened

This session handled **Finding 1** (SearXNG engine blocking under the research
loop's volume → the Tavily spillover that stopped attempt 3) end to end.
Landed the burst-rate mitigations: client-side query **pacing** + a run-scoped
**query-dedup cache** on the search tool, and the `searxng/settings.yml`
**engine re-tune** (keep the engines that served, disable the CAPTCHA/429-always
ones).
Then, by user decision, made the local suite **SearXNG-only** — Tavily is
reserved for the report job, so the whole Tavily-fallback machinery was removed
from the local web tool (`FallbackSearch`→`SearchTool`, `SearchRoute`,
`tavily_fallback_used`, the fallback branch, `LiveResearchWeb`'s `tavily_key`
param); a blocked/empty SearXNG now degrades a holding's research to a thinner
packet, never a spillover.
Dropping `tavily_fallback_used` from the persisted research audit changed the
checkpoint trail shape, so `CHECKPOINT_FORMAT_VERSION` bumped to
**`checkpoint-v8`** (the attempt-3 v7 trail is now refused at the resume gate).
Five Codex review rounds were run and remediated — including the `checkpoint-v8`
miss and a query-normalization revert to case+whitespace-only (so SearXNG
operators like `!bang`/`:lang` stay distinct keys).
Committed `a3b86d3` and pushed; `BUILD.md` updated to the SearXNG-only contract.

## Current state

Nothing in flight.
The Tavily-spillover failure mode that stopped attempt 3 is **closed by
construction**, so the SearXNG-mitigation re-attempt gate is **met** — a
re-attempt is now just the user's call, still from a **wiped store** per
`docs/verification/big-run-watch-set.md` (`checkpoint-v8` refuses the old v7
trail regardless).
**Findings 2–6 are not yet handled**: Finding 2 (ledger `quant` under-population
on numeric falsifiers — prose-only thresholds don't machine-evaluate) is the
flagged next candidate, then action-call prompt friction (3), the bounded-retry
rate (4), throughput (5), and extraction telemetry (6).
Per-holding research budgets stay **deferred** to the full run for calibration.
The `settings.yml` engine re-tune is **verify-on-bring-up** — the health-check
probe confirms the engine names the next time SearXNG comes up (the stack is
torn down).
The prior unrelated carried follow-ups remain untouched — the cloud `run_job`
seam, negative composite yield, `progress.rs` poisonable locks, the tracker `ok`
row's dropped count, TO logic-flow line 397, the 600 s `/api/tags` backstop,
whole-ledger seed injection, qualitative 6g un-trip semantics, an IPv6-loopback
wire test, the audit sources line, and the unreconciled-delete fail-soft
sentence's home.

## Open questions

- Whether and when to re-attempt the big run — the user's call; the
  SearXNG-mitigation gate is now met, but whether/when stays the user's decision
  (don't propose it unprompted).
- Whether a second full pass runs — the user's call after a *first full* run's
  result (none exists yet).
- Which of Findings 2–6 to take next — Finding 2 is the flagged candidate.

## Where to start

Do not propose re-running unprompted. To continue the findings work, take
**Finding 2** (ledger `quant` under-population): a prompt-side fix — describe the
`quant` sub-schema and the engine-series units/scale in the ledger-authoring
prose so the model authors structured falsifiers rather than prose thresholds.
Read `docs/verification/2026-08-30-big-run-findings.md` §Finding 2 first.
