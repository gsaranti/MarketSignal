# Current session handoff

## What happened

**The research gathering and synthesis prompts were audited, drafted on the v40 frame, ruled, planned, implemented and landed as `portfolio-v43`** (2026-09-17, `cdb49f8`, pushed).
The audit found the last pre-v40 shape on the per-holding loop and four things beyond it: three synthesis fields (`topic_answered`, `material_forward_fact`, the model-attributed `seeded_by`) parsed, persisted and read by nothing; the tier gloss reading the 0–5 scale backwards; a "you are told the budget is exhausted" stop signal the loop never sends; and fix list 4.4 ruled on 2026-09-15 and never landed.
Sixteen prompt rulings and ten plan rulings went through the selector, all the recommended option; the task review returned approve-with-nits (nine taken); one Codex round fixed two P2s (the extraction-quality gloss reworded to the measure; the source-weighing clause added to every synthesis variant).
The shape: both calls one message in two parts, the system prompts the role line and the output names; the shared holding header now closes with "Date: <run date>." on every packet; the seed as STANDING CONDITIONS and PRIOR FINDINGS; NEWS LEADS without ids; the TOOL RESULTS and EVIDENCE glosses once, the tier scale stated; SEARCHING in plain words beside the persisted mechanical gap; the published date on source headers, recency unshown; a per-pass grammar and shape (no follow-up on the disconfirming pass); the no-page pass app-assembled with no synthesis call; the fund topic `fund-exposure-profile`.
Records: `docs/verification/2026-09-17-research-prompt-rewrite.md`; the draft, today's pre-rewrite render and `prompts-v43.md` (section 9 = the research messages) under `~/Downloads/market-signal-fixed-evidence-2026-09-16/`; fix list 4.7.

## Current state

Committed and pushed on `main`, tree clean; Ollama was up and untouched.
Debut stamps: `portfolio-v43` / `checkpoint-v10` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`; portability format 7.
The dev store still holds attempt 6; attempt 7 re-wipes first and must also clear `portfolio_runs` and the `portfolio` namespace of `vector_memory`.
The v40, v41 and v42 fixed-set live read is one read, not run (three repeats, thinking captured, read against the three records' §Verification lists; the v42 half on the synthetic BND).
The v43 research prompts cannot run on the harness; their admission read is the next book-scale attempt's first holdings against the record's §Verification list.
The harness reads its daemon roster from the environment (`MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT=http://localhost:11434 MARKET_SIGNAL_LOCAL_REASONER_MODEL=qwen3.5:122b-a10b MARKET_SIGNAL_LOCAL_EMBEDDER_MODEL=qwen3-embedding:4b` beside the eval variables); launch it detached and log under `~/Downloads/market-signal-fixed-evidence-<date>/`.
Queued, each its own audit → draft → rulings → plan, in this order unless ruled otherwise: the four distillation prompts (`distill.rs`: pass, tier 1, tree reduce, reduce; no harness renders them, so the audit renders them the way the research audit did, through a temporary ignored test removed before commit), the seam-gap slice (1.10–1.11), then 8.6 after 7.2, §4–§7.
Owed, user-run: BUILD §Built bullets and INDEX rows for ten records — 2026-09-15 (two), 2026-09-16 (three), 2026-09-17 (five) — and the §What remains gate sentence.

## Open questions

- Whether the synthesis's empty-string placeholders and its "one JSON object" contract line (fix B once kept "JSON" out of this prompt) hold on the local model as they do on the interpretation call; read on the next attempt's first synthesis calls.
- Whether the model still carries the seed-attribution and topic-answered reasoning once nothing asks for them; the next attempt's synthesis markers per pass are counted beside the attempt-6 table.
- Whether live rationales still borrow "engine" once no prompt uses the word; the v35 fixture prose does, and the harness's banned-word diagnostic reads it.
- Whether the role/risk sections and weighing clause hold on a real fund; the synthetic BND proves the shape only.
- Whether the v40 read's yield on PGNY-type holdings falls without the shown caps; a better example, not the caps, is the answer if it does.
- Whether the distillation prompts count as one more slice under the principle (they are user-message-only on the fast tier).
- Carried: the prose scan's issuer-losses false positive stays a diagnostic; the fixture README's gain / loss parenthetical (F8); concentration trimming is the planner's; the attempt-6 record erratum; ARKF prices; stop rule 1 unread; per-domain denied share and the SearXNG engine set at book scale; the summary text's ".." artifact; the six reconstructed fixtures carry no fund context; fix list 6.2 (remaining turns exposed to gathering) open.

## Where to start

The v40 / v41 / v42 fixed-set live read, only on the user's word: bring Ollama up per the runbook, set the three daemon variables and the eval variables, run `cargo test fixed_evidence_live -- --ignored --nocapture` detached, read it against the three records' §Verification lists, and record the findings in each.
Never start the read unasked, never restart it unasked.
Otherwise the next slice is the four distillation prompts, audited and drafted first as the last four slices were (render, audit, draft, rulings through the selector), then `/metis-plan-task`, on the user's word; do not launch or propose a book-scale attempt.
