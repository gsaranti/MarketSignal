# Current session handoff

## What happened

**The role/risk interpretation prompt was audited, drafted on the v40 frame, ruled, planned, implemented and landed as `portfolio-v42`** (2026-09-17, `db042a8`, pushed).
The audit found the last pre-v40 shape (routing rule in the system prompt, instructions inside the inputs, the old ledger paragraph, two values rendered twice, no prior role read on continuity, the engine's gap strings explaining the app's routing); the draft's eleven rulings and the plan's three flags and four assumption groups went through the selector, the task review returned approve-with-nits (six taken), and one Codex round fixed one P2 (an action call on every repeat, not the first).
The shape: the two-part frame with the role line "an investment analyst … one fund holding"; CLASS with the reported asset class, EXPOSURE TILT, RISK PROFILE, EVIDENCE GAPS, the shared FINANCIAL METRICS and MARKET ANALYSIS with stances, on continuity PRIOR ANALYSIS with the prior role read; the shared ledger item on a `LedgerItemBranch` with the fund threshold example and driver clause on both fund variants (the priced fund message changed by exactly those two lines); a placeholder-only shape; the engine's eleven gap strings and the PRICE VS NAV line reworded as data on every surface, the card included; a synthetic BND role/risk fixture in the harness (pins, dump §8, the live block).
Records: `docs/verification/2026-09-17-role-risk-prompt-rewrite.md`; the draft, today's pre-rewrite render and `prompts-v42.md` under `~/Downloads/market-signal-fixed-evidence-2026-09-16/`; fix list 3.18.

## Current state

Committed and pushed on `main`, tree clean; Ollama was up all session and was not touched.
Debut stamps: `portfolio-v42` / `checkpoint-v10` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`; portability format 7.
The dev store still holds attempt 6; attempt 7 re-wipes first and must also clear `portfolio_runs` and the `portfolio` namespace of `vector_memory`.
The v40, v41 and v42 fixed-set live read is one read, not run: three repeats, thinking captured, read against the three records' §Verification lists, findings into each; the v42 half runs on the synthetic BND after the six holdings and tells the prompt's shape on a bond fund, never a run's numbers.
The harness reads its daemon roster from the environment (`MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT=http://localhost:11434 MARKET_SIGNAL_LOCAL_REASONER_MODEL=qwen3.5:122b-a10b MARKET_SIGNAL_LOCAL_EMBEDDER_MODEL=qwen3-embedding:4b` beside the eval variables); launch it detached and log under `~/Downloads/market-signal-fixed-evidence-<date>/`.
Queued, each its own audit → draft → rulings → plan, in this order unless ruled otherwise: the research gathering and synthesis prompts (`research.rs`), the four distillation prompts (`distill.rs`: pass, tier 1, tree reduce, reduce), the seam-gap slice (1.10–1.11), then 8.6 after 7.2, §4–§7.
Owed, user-run: BUILD §Built bullets and INDEX rows for nine records — 2026-09-15 (two), 2026-09-16 (three), 2026-09-17 (four) — and the §What remains gate sentence.

## Open questions

- Whether live rationales still borrow "engine" once no prompt uses the word; the v35 fixture prose does, and the harness's banned-word diagnostic reads it.
- Whether the role/risk sections and weighing clause hold on a real fund; the synthetic BND proves the shape only, and a real role/risk holding waits on a run.
- Whether the v40 read's yield on PGNY-type holdings falls without the shown caps (the read watches `margin-implausible`); a better example, not the caps, is the answer if it does.
- Whether the distillation prompts count as one more slice under the principle (they are user-message-only on the fast tier).
- Carried: the prose scan's issuer-losses false positive stays a diagnostic; the fixture README's gain / loss parenthetical (F8); concentration trimming is the planner's; the attempt-6 record erratum; ARKF prices; stop rule 1 unread; per-domain denied share and the SearXNG engine set at book scale; the summary text's ".." artifact; the six reconstructed fixtures carry no fund context, so FUND never renders on them (the synthetic BND carries one).

## Where to start

The v40 / v41 / v42 fixed-set live read, only on the user's word: bring Ollama up per the runbook, set the three daemon variables and the eval variables, run `cargo test fixed_evidence_live -- --ignored --nocapture` detached, read it against the three records' §Verification lists, and record the findings in each.
Never start the read unasked, never restart it unasked.
Otherwise the next slice is the research gathering and synthesis prompts, audited and drafted first as the last three slices were, then `/metis-plan-task`, on the user's word; do not launch or propose a book-scale attempt.
