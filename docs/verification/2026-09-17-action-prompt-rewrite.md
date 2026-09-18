# The action-prompt rewrite — `portfolio-v41` (2026-09-17)

## Scope and authority

The `portfolio-v40` interpretation rewrite set the principle: a prompt never explains the app or its concepts to the model (`2026-09-17-interpretation-prompt-rewrite.md`).
The v39 read had also found the action call computing with its own explanatory sentences — the hurdle prefix's rate and test re-run on the indeterminate holdings and "indeterminate" cited as a reason (L19), and the engine set's Facts form steering harder than the list on both admission outcomes (L20).
The user then asked for the action prompt drafted on the same decisions, rendered for TSLA.
The draft was aligned in conversation and its nine rulings taken through the selector (`~/Downloads/market-signal-fixed-evidence-2026-09-16/action-prompt-rewrite.md`).
The plan's four flags and four assumption groups were ruled the same way before implementation.
Fix list 2.6 is the entry; this record carries the rulings, the inventory and, once run, the read.

## Rulings — 2026-09-17

The nine on the prompt text:

- **Capital efficiency: numbers only.** The three tested twelve-month total returns and the hurdle rate with a one-line gloss; no state word, no reach sentence.
- **The sunk-cost clause: every priced packet.** One clause of the task; the numbers it reads are present on every packet.
- **SUPPORTED ACTIONS: one data line, no permission sentence.** This closes fix list 3.9's ruling on the line; the return shape's enum shows the ladder.
- **The target rationale: forwarded (3.16).**
- **Provenance: method clauses.** Each computed band carries the method clause the interpretation packet renders; the provenance label is gone.
- **The firmness sentence: dropped on a debut.** It returns on a continuity call as a PRIOR ACTION section and one task clause; supersedes L14 (2026-09-16).
- **Labels: computed and analyst.** One sentence at the top of Part 1 says what the labels mean; the packet does not say the analyst's read came from the same model.
- **Weighing order: scores and targets first, the rest refining.** The former ACTION BASIS hierarchy as a task clause.
- **Thesis and scenarios: added.** The ledger's current thesis and the three scenario rows with their probabilities; the must-improve and must-not-break lines stay out.

The eight from the plan:

- **F1, the pre-profit consequence lines: dropped from the action packet.** SUPPORTED ACTIONS carries the narrowed set; the overlay's facts stay.
  The forensic sweep's rule sentence is off the action packet on the same reasoning, ratified 2026-09-17 at task review.
- **F2, the role/risk branch: rewritten in this slice.**
- **F3, the lexicon pin on the action packet: scans a render with the analyst prose blanked.** The persisted v35 rationale legitimately says "the engine's base case".
- **F4, a rule-demoted prior action: rung plus a gloss**, anchoring no firmness clause.
- **A1, stamps: `portfolio-v41` only.** `checkpoint-v10`, `evidence-floor-v5` and portability format 7 stand, since no persisted shape changes.
- **A2, rendering: as assumed.** An unscorable hurdle renders "No assessment this run."; the gap / off-scale / inverted tags stay as tags; the computed grade reuses the interpretation's low-confidence gloss.
- **A3, what_must_improve / what_must_not_break: out.**
- **A4, fixtures: `ledger_prose` on all six**, from the dev store's checkpoint rows; the README updated; the dump regenerated as `prompts-v41.md`.

## What landed

- `action_system_prompt` is the role line, the two-part shape and `action_response_contract`, now "You will return action and rationale, as one JSON object."; `action_return_shape` renders the placeholder-only shape from the decision schema through `placeholder_shape`, the one renderer behind both shapes.
- `action_user_prompt` is `======== PART 1: INPUTS ========` then `======== PART 2: TASK ========`.
  Part 1 opens with the shared holding header and the two-reads sentence.
  On a priced holding: SCORES (the polarity and the grade's derivation glossed once, the computed and analyst rows, the low-confidence gloss shared with the interpretation packet), PRICE TARGETS (both reads' bands with the move each implies; the computed bands' method clauses and notes; the gap, off-scale and inverted tags as before), TARGET RATIONALE, CAPITAL EFFICIENCY (the three tested returns and the hurdle rate, or "No assessment this run."), CONVICTION AND OUTLOOK, FINANCIAL SUMMARY, THESIS, SCENARIOS, then with a prior verdict PRIOR ACTION, PRIOR ANALYSIS and CHANGES SINCE THE PRIOR ANALYSIS, then the overlay's facts.
  On a role/risk holding: CLASS, ROLE, EXPOSURE TILT, RISK PROFILE, the PRICE VS NAV line, EVIDENCE GAPS, THESIS, SCENARIOS and the continuity sections, the task's weighing clause naming each section that rendered.
  Both branches close Part 1 with the forensic sweep, commodity prices and the option overlay where they apply, SUPPORTED ACTIONS as one data line, and INVESTOR PROFILE.
  Part 2 is item 1 (the rung, the weighing clause naming the sections present, the profile tie-break, on a priced holding the sunk-cost clause, with a chosen prior the firmness clause), item 2 (the one-sentence rationale) and RETURN SHAPE.
- `ActionSubject` carries the holding's validated ledger on both branches; both call sites in `analyze_holding` pass it.
- The overlay's consequence lines and the forensic sweep's rule sentence render on the interpretation packet only (`PromptStage`).
- `EngineSetForm`, `action_user_prompt_with_form`, `engine_admission_facts_line`, `decide_action_under`, `implied_moves_section` and `SCORE_POLARITY` are removed; the harness's `MARKET_SIGNAL_LOCAL_EVAL_ENGINE_SET` variable is gone.
- The six attempt-6 fixtures carry `ledger_prose` (the current thesis, the three monitor rows and the two must-lines), extracted from the dev store's `portfolio_checkpoint_holdings` rows of run 8d6b0b88 on 2026-09-17; the harness builds the packet's ledger from it, and the fixtures README says so.
- The harness gains `attempt_6_action_messages_are_two_parts_with_no_app_concept` — the two parts, the sections, the hurdle numbers, the persisted thesis and rows, the shape's keys, and the banned lexicon over a render with the analyst prose blanked — and the dump drops its Facts section.
  The live harness prints each interpretation's validated thesis and scenario rows, the prose the fresh-interpretation action call receives, and reports an account-economics phrase or a banned word in any model-authored field or rationale as a diagnostic.
- Stamp: `PROMPT_VERSION` → `portfolio-v41`.
- Docs: `portfolio-analysis.md` §Portfolio action (the packet's sentences), `portfolio-workflow.md` §Step 6f (the action call's input paragraph); fix list 2.6 with the supersession, carry and absorption lines on 2.2–2.5, 3.9–3.13, 3.15 and 3.16.

## Task-review corrections

The task review returned approve-with-nits; the forensic drop was ratified and all six nits taken through the selector (2026-09-17).

1. The forensic sweep's rule sentence off the action packet went beyond F1's wording; ratified as part of F1.
2. `Conviction`, `HorizonRead` and `ScenarioKind` gained `as_str`, `ChangeAttribution` gained `as_words`, and the serde round-trip helper and the hand-written attribution match are gone; one mechanism prints every enum.
3. The task section's list closure takes its names by value.
4. One `horizon_words` helper serves the CONVICTION AND OUTLOOK and PRIOR ANALYSIS lines.
5. The role/risk section EXPENSE AND RISK is RISK PROFILE, so the weighing clause's and-joined list reads cleanly, and PRICE VS NAV is named in the clause where the line renders.
6. Two kept §Portfolio action sentences that described the old packet (typed provenance, raw implied moves) now describe the new one; the capital-efficiency sentence and this record's authority paragraph are split per sentence.

## Codex implementation-review corrections

Read-only review after the task review; Codex changed no files and ran no gate, so the full gate ran here after the fixes.
Three findings, P2, each verified against the code and fixed:

1. The CAPITAL EFFICIENCY gloss read "the scenario price plus forward income per share, over the current price", a ratio without the minus one the engine applies, so $100 to $110 would read as 110%.
   Fix: the gloss reads "the move from the current price to the scenario price, plus forward income per share, as a fraction of the current price"; the draft's line is corrected with it.
2. The SUPPORTED ACTIONS gloss credited "the computed scores and capital-efficiency read" with the set, while the financing bar, severe deterioration and a hard forensic trip narrow it too.
   Fix: "The rungs the computed read supports on its own", on both branches; the draft's line is corrected with it.
3. The live harness printed four interpretation prose fields but not the thesis and scenario rows the action packet now forwards, and the record claimed a banned-word diagnostic the harness did not run.
   Fix: the harness prints the validated thesis and scenario rows after each interpretation and runs the account-economics and banned-word scans over every model-authored field and every action rationale, as diagnostics.

## Known limits

- The lexicon pin on the action packet scans the app's own sentences; a live rationale, summary, thesis or scenario row that carries a banned word is printed by the harness as a diagnostic, never a gate.
- The continuity and role/risk variants are proven by offline tests; the fixed set has no continuity case and no role/risk holding, so their live behaviour stays unread until a run supplies one.
- The fixtures' ledger prose is the v35 run's; the harness's fresh-interpretation action call uses the ledger the fresh interpretation authored, validated through 6g.
- The method clauses render with a closing period on the action packet and without one on the interpretation packet, since the shared renderers omit it and the ruled draft carried it.

## Verification

Offline, at implement time:

```
cd src-tauri && cargo test
cd src-tauri && cargo clippy --all-targets --all-features
npm run build
npm test
cd src-tauri && MARKET_SIGNAL_LOCAL_EVAL_PROMPT_DUMP=~/Downloads/market-signal-fixed-evidence-2026-09-16/prompts-v41.md cargo test --lib fixed_evidence_prompt_dump -- --ignored
```

The dump's TSLA action message matches the ruled draft line for line, the draft's two gloss lines corrected with the Codex round.

The live read, after the task review and Codex, only when the user says, with Ollama brought up per the runbook first:

```
cd src-tauri && MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT=http://localhost:11434 MARKET_SIGNAL_LOCAL_REASONER_MODEL=qwen3.5:122b-a10b MARKET_SIGNAL_LOCAL_EMBEDDER_MODEL=qwen3-embedding:4b MARKET_SIGNAL_LOCAL_EVAL_REPEATS=3 MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR=<dir> cargo test fixed_evidence_live -- --ignored --nocapture
```

It reads the following against the v39 tables, beside the v40 interpretation read's own list.
No rationale cites a hurdle state word or computes the hurdle test from a prefix; the rationales that name the hurdle quote the tested returns.
No rationale argues from what the computed read permits (the permission scan), and the rung table stays inside each holding's repeat behaviour (TSLA trim / sell-all, PSX trim, SPMO sell-all, ARKF trim / sell-all, DIA hold, PGNY hold).
Action fence markers fall from forty-two on forty-seven calls, and the ENGINE SET and capital-efficiency shares of the re-think fall.
The rationales are read for consistency with the thesis and scenarios now in the packet, and for any claim that computes with a sentence the packet no longer carries.
