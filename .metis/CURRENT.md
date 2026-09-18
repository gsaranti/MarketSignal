# Current session handoff

## What happened

**The four distillation prompts were audited, drafted on the v40 frame, ruled, planned, implemented and landed as `portfolio-v44` / `checkpoint-v11`** (2026-09-17, `81870b1`, pushed) — the last pre-v40 shape on the Portfolio local-model surface, so the prompt-rewrite series that began with the interpretation call is complete.
The audit read the rendered prompts beside attempt 6's persisted distillation output: claims rendered with retrieval timestamps only, so the model dated facts by retrieval (ARKF's "announced 2026-09-16, trading 2025-03-31"); every typed field was asked on every holding and filled regardless of support (invented driver ids on first analyses and funds, a capex fill the engine cannot recompute, a news-site fraud claim, confidence 1.0 throughout); `conflict_handling` changed only the rejection wording; backfill was asked without the obligation; one object per topic was never stated though an omission deletes the seed row.
Eighteen prompt rulings and ten plan rulings through the selector, all the recommended option; task review approve-with-nits (nine taken); one Codex round (two fixed: the source-text framing must fit before anything renders, three stale doc tuples reconciled).
Records: `docs/verification/2026-09-17-distillation-prompt-rewrite.md`; the draft, the pre-rewrite render and `prompts-v44.md` (section 10 = the seven distillation messages) under `~/Downloads/market-signal-fixed-evidence-2026-09-16/`; fix list 4.8.

## Current state

Committed and pushed on `main`, tree clean; Ollama untouched this session.
Debut stamps: `portfolio-v44` / `checkpoint-v11` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`; portability format 7.
The dev store still holds attempt 6; attempt 7 re-wipes first and must also clear `portfolio_runs` and the `portfolio` namespace of `vector_memory`.
The v40, v41 and v42 fixed-set live read is one read, not run (three repeats, thinking captured, read against the three records' §Verification lists; the v42 half on the synthetic BND); the v43 research and v44 distillation prompts cannot run on the harness, and their admission is the next book-scale attempt's first holdings against each record's §Verification list.
The harness reads its daemon roster from the environment (`MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT=http://localhost:11434 MARKET_SIGNAL_LOCAL_REASONER_MODEL=qwen3.5:122b-a10b MARKET_SIGNAL_LOCAL_EMBEDDER_MODEL=qwen3-embedding:4b` beside the eval variables); launch it detached and log under `~/Downloads/market-signal-fixed-evidence-<date>/`.
Queued, each its own plan, in this order unless ruled otherwise: the seam-gap slice (1.10–1.11), then 8.6 after 7.2, then §4–§7.
Owed, user-run: BUILD §Built bullets and INDEX rows for eleven records — 2026-09-15 (two), 2026-09-16 (three), 2026-09-17 (six) — and the §What remains gate sentence.

## Open questions

- Whether the daemon's grammar honours the per-call enums at book scale — a topic-key enum of up to eight keys and the nullable condition-id enum; the ledger's nullable series enum is the precedent and the v40–v42 reads showed no grammar failure.
- Whether the no-declaration path changes the Step-6e shadow record: every forward fact now enters as a supplement, so a present feed value rejects the fill and the supersede leg stays dormant.
- Whether interpretation misses anything on a priced fund now that its distillation carries no typed field (none had a consumer on a fund).
- Whether the synthesis's and distillation's empty-string placeholders and "one JSON object" line hold on the local model as they do on the interpretation call; read on the next attempt's first calls.
- Whether live rationales still borrow "engine" once no prompt uses the word; the v35 fixture prose does, and the harness's banned-word diagnostic reads it.
- Whether the v40 read's yield on PGNY-type holdings falls without the shown caps; a better example, not the caps, is the answer if it does.
- Carried: the prose scan's issuer-losses false positive stays a diagnostic; the fixture README's gain / loss parenthetical (F8); concentration trimming is the planner's; the attempt-6 record erratum; ARKF prices; stop rule 1 unread; per-domain denied share and the SearXNG engine set at book scale; the summary text's ".." artifact; the six reconstructed fixtures carry no fund context; fix list 5.2 and 6.2 open.

## Where to start

The v40 / v41 / v42 fixed-set live read, only on the user's word: bring Ollama up per the runbook, set the three daemon variables and the eval variables, run `cargo test fixed_evidence_live -- --ignored --nocapture` detached, read it against the three records' §Verification lists, and record the findings in each.
Never start the read unasked, never restart it unasked.
Otherwise the next slice is the seam gaps (fix list 1.10–1.11), through `/metis-plan-task` on the user's word; do not launch or propose a book-scale attempt.
