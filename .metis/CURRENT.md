# Current session handoff

## Active task

The single big confirmation run — attempt 8's findings are all landed; attempt 9 waits on the user's word.

## What happened

Slice B (Findings 3, 4 of `docs/verification/2026-09-20-attempt-8-findings.md`) was planned, ruled, implemented, reviewed and committed as `304b872` on main (pushed); the stamp stays `portfolio-v49`.
Five selector rulings, all on the recommended option: `NUM_PREDICT_DISTILL` 12,288 (fits a 32 K fast tier beside the input budget; 16,384 declined); reused pages render first, in first-retrieval order, in both research messages, with explicit fetches keeping first claim under overflow; the TOOL RESULTS gloss sits in the holding-constant block; the render went to `~/Downloads/market-signal-fixed-evidence-2026-09-16/prompts-v49-slice-b.md`; no same-kind batching.
The attempt-8 serve log plus a source read settled the runtime mechanics: Ollama 0.32.5 runs llama-server b10091 with one slot and an 8 GiB RAM prompt cache; checkpoints sit 1,024 tokens before a prompt's end; a cached conversation is handed back only when the shared prefix is at least a quarter of its length; tools render before the system text, so gathering and synthesis requests share three tokens.
The reorder therefore reaches consecutive same-kind conversations only: syntheses from a holding's second topic, roots from about the fourth; follow-up passes, the closing calls and continuity-run roots with large prior findings stay full-prefill.
Reviews: the Metis reviewer approved with three nits, taken; Codex ran two rounds, four P3 findings taken (both gathering caps reserve their markers and the reuse framing, the docs state the saving as conditional, the debut exemption was narrowed).

## Current state

Nothing in flight; the working tree is clean at `304b872`.

- **Debut stamp set:** `portfolio-v49` / `checkpoint-v15` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`, portability 11.
- **Store:** attempt 8's three checkpoint holding rows persist under `portfolio-v48`, `portfolio_runs` 0, `job_runs` max 8; attempt 9 needs a re-wipe to a clean debut (user's call). Prod untouched.
- **Named follow-up:** same-kind batching (every root gathering before any root synthesis) if attempt 9's serve log shows roots not restoring.
- **Attempt-9 latency witness:** serve-log `restored context checkpoint` lines on synthesis tasks from a holding's second topic and on root tasks from about its fourth; no expanded distillation on a holding whose first pass stays under 12,288; per-stock prompt-evaluation time below the attempt-8 band.
- **Watches carried (no change):** 40-fetch budget exhausted before follow-ups and the disconfirming pass; distillation claim drops; funds not faster than stocks; quality sub-score ≤10 on both stocks (read across a fuller book before any `grade-v2.3` change); empty issuer descriptions.

## Open questions

- Whether the §Slices grouping belongs in the findings doc or only here — carried.
- Whether the runtime cache facts (checkpoint distance, the quarter rule, tools before the system text) need a durable home, likely `docs/local-model-operations.md` beside the one-slot note, or stay in the verification record — unruled.

## Where to start

Attempt 9 is the user's call: re-wipe the dev store to a clean debut first, bring the stack up per the runbook, then the user clicks Run; never propose the run.
No slice is queued; a new task starts with `/metis-plan-task`.
