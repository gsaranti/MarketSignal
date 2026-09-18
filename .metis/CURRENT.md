# Current session handoff

## What happened

**The action prompt was rewritten on the no-app-concepts principle and landed as `portfolio-v41`** (2026-09-17, `117fc45`, pushed).
The user asked for the TSLA action system and user prompt drafted on the v40 decisions; the draft's nine prompt rulings and the plan's eight flags and assumptions went through the selector, the task review returned approve-with-nits (all six taken, the forensic "By rule" drop ratified), and one Codex round fixed three P2s (the total-return gloss lacked the minus one, the set gloss credited two inputs, the harness lacked the fresh-prose print and the banned-word diagnostic).
The shape: one message in two parts, the system prompt the role line and the output names; SCORES, PRICE TARGETS and SUPPORTED ACTIONS as computed / analyst data with one defining sentence; capital efficiency as the three tested returns and the hurdle rate with no state word; the analyst's target rationale, thesis and scenario rows forwarded; method clauses in place of the provenance label; the overlay's and forensic sweep's consequence lines off the action packet; the weighing order, profile tie-break and sunk-cost rule as task clauses; the firmness clause with a chosen prior only; the role/risk branch on the same frame; the Facts form removed; the six fixtures carrying `ledger_prose` from the dev store rows.
Records: `docs/verification/2026-09-17-action-prompt-rewrite.md` (rulings, inventory, Codex corrections); the draft at `~/Downloads/market-signal-fixed-evidence-2026-09-16/action-prompt-rewrite.md`; the dump `prompts-v41.md`; fix list 2.6 (3.9 closed, 3.15 / 3.16 absorbed, 2.2–2.5 and 3.10–3.13 superseded or carried).

## Current state

Committed and pushed on `main`, tree clean; Ollama was up at session start and was not touched.
Debut stamps: `portfolio-v41` / `checkpoint-v10` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`; portability format 7.
The dev store still holds attempt 6; attempt 7 re-wipes first and must also clear `portfolio_runs` and the `portfolio` namespace of `vector_memory`.
Neither the v40 nor the v41 fixed-set live read is run; one read covers both — three repeats, thinking captured, read against the v40 record's §Verification for the interpretation half and the v41 record's for the action half, findings into both records.
The harness reads its daemon roster from the environment (`MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT=http://localhost:11434 MARKET_SIGNAL_LOCAL_REASONER_MODEL=qwen3.5:122b-a10b MARKET_SIGNAL_LOCAL_EMBEDDER_MODEL=qwen3-embedding:4b` beside the eval variables); launch it detached and log under `~/Downloads/market-signal-fixed-evidence-<date>/`; the live harness now prints each interpretation's thesis and scenarios and flags account-economics phrases and banned words in every model-authored field as diagnostics.
Queued, each its own plan, in this order unless ruled otherwise: the role/risk interpretation prompt (still on its old shape and template; shares the rewritten ledger block), the research gathering and synthesis prompts, the distillation prompts; the seam-gap slice (1.10–1.11); then 8.6 after 7.2, §4–§7.
Owed, user-run: BUILD §Built bullets and INDEX rows for eight records — 2026-09-15 (two), 2026-09-16 (three), 2026-09-17 (three) — and the §What remains gate sentence.

## Open questions

- Whether live rationales still borrow "engine" once no prompt uses the word; the v35 fixture prose does, and the harness's banned-word diagnostic reads it.
- Whether the role/risk action packet's sections and weighing clause hold on a live fund; the fixed set has no role/risk holding and no continuity case, so both variants are proven offline only.
- Whether the v40 read's yield on PGNY-type holdings falls without the shown caps (the read watches `margin-implausible`); a better example, not the caps, is the answer if it does.
- Whether the distillation prompts count as one more slice under the principle (they are user-message-only on the fast tier).
- Carried: the prose scan's issuer-losses false positive stays a diagnostic; the fixture README's gain / loss parenthetical (F8); concentration trimming is the planner's; the attempt-6 record erratum; ARKF prices; stop rule 1 unread; per-domain denied share and the SearXNG engine set at book scale; the summary text's ".." artifact; the fixtures carry no fund context, so FUND never renders on the fixed set.

## Where to start

The v40 / v41 fixed-set live read, only on the user's word: bring Ollama up per the runbook, set the three daemon variables and the eval variables, run `cargo test fixed_evidence_live -- --ignored --nocapture` detached, read it against both records' §Verification lists, and record the findings in both.
Never start the read unasked, never restart it unasked.
Otherwise the next slice is the role/risk prompt through `/metis-plan-task`, on the user's word; do not launch or propose a book-scale attempt.
