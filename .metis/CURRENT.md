# Current session handoff

## What happened

**Codex analyzed the v38 read for double-guessing and confusion, its eight suggestions were verified and ruled, and the residue slice landed as `portfolio-v39`** (2026-09-17).
Six suggestions held, one was re-scoped to the harness (PGNY's balance-sheet split is a fixture artifact, not a live state), one was already ruled.
The rulings widened the residue slice to 3.5–3.14 + 8.5, held the evidence-set additions (3.15–3.16) for after the 3.9 A/B, adopted the strengthened 3.9 read, and added 8.6 (a sampling-profile comparison after 7.2).
Three intent amendments landed before planning: 3.8 states the rule alone, 3.11 defines the grade instead of asserting coexistence, 3.14 states the band's purpose before the cap and watches for anchoring.
The plan's four decisions and four assumption groups were ruled through the selector; only the prompt stamp moved (`checkpoint-v10` unchanged).
Task review: a reject on a README note a script had built but never written, then approve-with-nits with every nit taken (the production trait unwidened, two branches pinned, the permission lexicon tightened).
Codex: one P2 over three rounds — the new margin caps contradicted the validator's zero-threshold exemption in three prompt sentences; all now qualified for a nonzero level, and the harness prints "no ratio: zero level".
Commits: `b23677a` (analysis + rulings), `a0e2055` (slice).
Records: `docs/verification/2026-09-17-residue-slice.md`; the Codex section of `2026-09-16-interpretation-slice.md`; fix list 3.10–3.16, 8.5, 8.6.

## Current state

Committed and pushed on `main`, tree clean; Ollama down.
Debut stamps: `portfolio-v39` / `checkpoint-v10` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`; portability format 7.
The dev store still holds attempt 6; attempt 7 re-wipes first and must also clear `portfolio_runs` and the `portfolio` namespace of `vector_memory`.
The v39 fixed-set live read is NOT run: `MARKET_SIGNAL_LOCAL_EVAL_ENGINE_SET=both` runs the 3.9 A/B in one pass (about 72 calls, roughly 2.5 h); the record's §Verification names the eight things it reads, the strengthened 3.9 read included (permission claims, ownership errors, rationale correctness under both forms).
Queued after that read: 3.15–3.16 (hurdle numbers in the packet; forward `model_target_rationale` plus an engine basis — each needs a §Portfolio action ruling on its form), 8.6 after 7.2 (7.2 still open; 8.6's place relative to 8.3's action leg is a plan flag), then §4–§7 each their own plan.
Owed, user-run: BUILD §Built bullets and INDEX rows for six records (two 2026-09-15, three 2026-09-16, one 2026-09-17) and the §What remains gate sentence.

## Open questions

- Whether the Facts form's "the letter bars the add family in the engine's own rule" is governed content: read as the canonical feasible-set rule for now; the A/B ruling should say so explicitly.
- PGNY's noise bands: five `margin-implausible` on the v38 read, four of them PGNY's; the caps now render in the prompt, so watch the kept cores' margin-to-level ratios for anchoring toward the cap.
- 1.9's yield: PSX and PGNY recovered to two cores each, not four — read again on the v39 read.
- Carried: the prose scan's issuer-losses false positive stays a diagnostic; the fixture README's gain / loss parenthetical (F8); concentration trimming is the planner's; the attempt-6 record erratum; ARKF prices; stop rule 1 unread; per-domain denied share and the SearXNG engine set at book scale; the summary text's ".." artifact.

## Where to start

The v39 fixed-set live read, only on the user's word: bring Ollama up per the runbook, then `cd src-tauri && MARKET_SIGNAL_LOCAL_EVAL_REPEATS=3 MARKET_SIGNAL_LOCAL_EVAL_ENGINE_SET=both MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR=<dir> cargo test fixed_evidence_live -- --ignored --nocapture`; read it against the record's §Verification list, rule the 3.9 form on its evidence, and record the findings in the record.
Never start the read unasked, never restart it unasked.
Do not launch or propose a book-scale attempt.
