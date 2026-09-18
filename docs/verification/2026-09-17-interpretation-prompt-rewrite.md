# The interpretation-prompt rewrite — `portfolio-v40` (2026-09-17)

## Scope and authority

The v39 fixed-set read (`2026-09-17-residue-slice.md` §Live read) found that every explanatory sentence the v35–v39 clarity slices had added became a quantity the model computed with: the margin's purpose and caps were sized toward, the hurdle prefix's rate and test were re-run by hand, and the admission-facts line was read as a rule.
The user then read the rendered prompts (`~/Downloads/market-signal-fixed-evidence-2026-09-16/prompts-v39.md`) and ruled the principle: a prompt never explains the app or its concepts to the model — it gives the data, explains the data where a value needs it, says what to compute, and gives the return structure.
The stock first-analysis prompt was drafted to that principle and aligned in three rounds (`~/Downloads/market-signal-fixed-evidence-2026-09-16/interpretation-prompt-rewrite.md`), and the plan's four decisions and four assumption groups were ruled through the selector before implementation.
Fix list 3.17 is the entry; this record carries the rulings, the inventory and, once run, the read.

## Rulings — 2026-09-17

- **D1, variant scope: all four now.**
  Stock and fund, first analysis and continuity, one builder and one frame; the aligned stock first-analysis text is the reference and the other three follow it.
- **D2, optional sub-sections: strip narration, keep data.**
  Forensic, commodity, positioning, short interest, the put/call backdrop, the option overlay, the pre-profit overlay, the narrative read and the technology pre-flag lose "deterministic", "NOT a grade input", "the engine's", "never a gate" and their kin; every value and unit gloss stays; no restructure.
  The four shared with the action packet change there too.
- **D3, self_assessment on a first analysis: the prompt says what to put.**
  One sentence noting no prior read exists; no schema change.
- **D4, the v39 residue: record yes, seam fixes separate.**
  The read section lands in `2026-09-17-residue-slice.md` with this slice; the two validator gaps are fix list 1.10 and 1.11, their own slice.
- **A1, stamps: `portfolio-v40` only.**
  `checkpoint-v10`, `evidence-floor-v5` and portability format 7 stand, since no persisted shape changes.
- **A2, the draft's remaining questions: confirmed as assumed.**
  The capital-efficiency line leaves the interpretation call (nothing downstream reads it from the response — the verdict's dead-money state is app-stamped and the action packet re-renders the hurdle); the target method clauses stay, since `model_target_rationale` asks for the departure; momentum stays in `model_sub_scores`.
- **A3, the shape renderer: split.**
  Interpretation gets a placeholder-only shape with the enums inline; the distillation and role/risk prompts keep `response_shape_contract`.
- **A4, the live read: three repeats, List form, thinking captured.**
  The same shape as the v38 read, so it compares directly against the v38 and v39 tables; the Facts A/B stays a separate 3.9 ruling.
- **User additions to the draft (2026-09-17):** the one-sentence role line is the system prompt with the output names appended; a third margin example covers a multiple.

## What landed

- The system prompt (`interpretation_system_prompt`) is the role, the two-part shape and the output-name line `interpretation_response_contract`, built from the same key list as the schema's required set.
- The user prompt (`interpretation_user_prompt`) is `======== PART 1: INPUTS ========` then `======== PART 2: TASK ========`.
  Part 1 opens with HOLDING (identity, the first-analysis or position-change sentence, the price) and, on a fund with fund context, FUND (expense ratio, US share, composite coverage, price versus NAV, positioning).
  FINANCIAL METRICS carries the basis sentence, then one line per computable series — value, ledger label in brackets, unit gloss, confirmation rule — and the data gaps.
  COMPUTED SCORES states the scale and polarity once, the four scores, the grade with its low-confidence gloss, and the risk tier.
  COMPUTED PRICE TARGETS carries both bands, each with a method clause rendered from the typed target inputs, a notes line where a derivation flag applies, what the price implies, the narrative read and the pre-profit overlay.
  OPTIONS ACTIVITY is followed by the market-wide backdrop, short interest, the option overlay, forensic filings and commodity prices; then RESEARCH SUMMARY and MARKET ANALYSIS (the latest report's sections as a market-level analysis dated, with the recent stances folded under it).
  On a continuity call Part 1 adds SECTOR-RELATIVE MOVE, PRIOR ANALYSIS (the retrospective, with the parameter-boundary lines as data), PRIOR ANALYSIS NOTES, CHANGES SINCE THE PRIOR ANALYSIS (the id rows only), PRIOR THESIS LEDGER and CONDITION CROSSINGS THIS RUN; on a first analysis PRIOR THESIS LEDGER says none.
  Part 2 numbers financial_summary, model_sub_scores, model_price_targets with model_target_rationale, horizon_outlook, the ledger item, then on continuity what_changed_entries and what_changed, then conviction and self_assessment; RETURN SHAPE closes with `interpretation_return_shape`, its top-level keys in that order.
  The ledger item states its parts in output order, the quant contract as requirements on the output, threshold and margin in one clause each, the margin sized by three examples, the two worked examples, and on continuity the carry rule and the tripped / fired rule.
- `LedgerSeriesContract::metric_lines` renders the series lines for FINANCIAL METRICS and for the role/risk prompt's block; `render` (role/risk only) carries no caps; `confirmation_note` reads "confirmed by one filing" / "confirmed by two consecutive daily closes"; `statement_basis_line` takes the vehicle kind and names the metrics in words.
- `ledger_prompt_section` (role/risk) is split into `prior_ledger_data_section` (shared with the priced Part 1) and `ledger_rewrite_instructions`; `input_delta_prompt_section` is the id rows only and `what_changed_instructions` carries the role/risk prompt's authoring text.
- The holding header reads `HOLDING / SYM (name). / Price: $358.97 per share.` on every packet.
  The position-change line is a sentence.
- The optional sub-sections keep their values and lose their narration (D2); the pre-profit consequence lines read the same on both stages.
- `interpretation_return_shape` in `mod.rs`: strings "", numbers 0, booleans false, enums "<a|b|c>", a nullable enum "<a|b|null>", arrays one item, every object's keys in the order the task items state them, and the heading saying an array holds as many items as apply (the last three ruled 2026-09-17 after the commit, off the user's read of the rendered shape).
- Stamp: `PROMPT_VERSION` → `portfolio-v40`.
- Docs: `portfolio-workflow.md` §Step 6f (the interpretation-call paragraphs and the prompt-input paragraph), `portfolio-analysis.md` §The position thesis ledger (the authoring-contract sentences) and §Intrinsic verdict; fix list 3.17, 1.10, 1.11 and the live lines on 1.9, 3.5–3.14 and 8.5.
- The fixed-evidence harness gains `fixed_evidence_prompt_dump` (ignored; `MARKET_SIGNAL_LOCAL_EVAL_PROMPT_DUMP=<file>`), which writes every rendered fixed-set prompt to one Markdown file for a human read.

## Known limits

- The target method clauses are rendered from the typed target inputs (`twelve_month_method`, `one_month_method`), so the engine's methodology strings and their stamp stay audit-only; the one-month band is read back from the target itself.
- The retrospective's app bookkeeping is reduced to its facts ("split-adjusted", "demoted by rule after authoring"); the banned lexicon is the ruled list plus the target tokens, and a continuity message is scanned by the pipeline tests.
- The role/risk prompt keeps its structure and its schema-derived template; only the sub-sections it shares with the priced message changed.
- The continuity and fund variants follow the frame without a separate aligned draft; they are read from the regenerated dump before the live read.
- The banned-lexicon test runs over the fixed set's rendered prompts, so a live research summary or house-view text that happens to carry a banned word is not in its reach; the ban is on the app's own sentences.
- The margin caps stay enforced at the seam; a holding whose noise bands need the cap to be seen (PGNY on v38) may lose cores to `margin-implausible` again, and the read watches for it.

## Task-review corrections

The task review returned reject-with-reasons on two items and nits; all taken, the two rulings by the user through the selector.

1. The target method lines had shipped the engine's methodology strings less the stamp, still naming "v1 mechanics", "growth clamp released", "degenerate-denominator raw fallback" and the clamp bounds; the reviewer held that the basis line already proved the typed inputs suffice.
   Fix: both method clauses render from `TargetMeta` and the target band, and the internal tokens joined the lexicon and the structural pins.
2. The stamp's doc comment (the v2–v39 history, about four hundred lines) had been replaced by the v40 paragraph without being surfaced.
   Ruled 2026-09-17: restored, the v40 paragraph appended.
3. The ledger item's "which most drivers will be" clause was in the aligned draft but fix list 3.8 had been amended to drop it.
   Ruled 2026-09-17: dropped; the 3.8 live line records it.
4. Nits taken: the return shape's top-level keys in output order; the expense ratio on the metric line through the shared formatter; item 1 cites FUND only where the section rendered; item 5 names CONDITION CROSSINGS THIS RUN exactly; the contract function's unused parameter removed; the continuity message scanned for the lexicon; the retrospective's bookkeeping phrases reduced to their facts; the docs' multi-sentence lines split.

## Codex implementation-review corrections

Read-only review after the task review; Codex changed no files and ran no gate, so the full gate ran here after the fixes.
Three findings, P2, each verified against the code and fixed:

1. The continuity message still received the production delta labels verbatim — "engine sub-score", "engine grade", "engine twelve-month base target", "house view: the latest Market Signal report", the stamp pairs on the parameter-boundary rows — and the capital-efficiency change row, which the rewrite had removed from the interpretation call; the continuity lexicon pin passed an empty delta and missed it.
   Fix: the labels name the change without the app's words or its stamps ("computed grade: B -> A", "grade bands changed since the prior analysis — …", "market analysis: a market-level analysis supplied this run"), the capital-efficiency row stays in the audit and the action packet's evidence list and never reaches the interpretation projection (one label prefix, `CAPITAL_EFFICIENCY_DELTA_PREFIX`, homes the push and the filter), and the pipeline test builds a production delta and scans every label.
2. The raw-percentile fallback's method clause said 75th / 50th / 25th while the engine takes the 25th, 50th and 75th percentile of the raw multiples for bear, base and bull.
   Fix: the clause reads 25th / 50th / 75th; the fallback branch is pinned.
3. The holding header dated the quote with the last daily close's date, though the quote and the dated closes are separate requests and a Monday intraday quote would read as Friday's.
   Fix: the header carries the price alone ("Price: $358.97 per share."); every packet that shares the header changes with it.

Codex's second round found the corrected market-analysis label claiming a newer report on presence alone, since the row is pushed whenever report text exists; it now reads "a market-level analysis supplied this run".
The fallback test now asserts the 25th / 50th / 75th order, and this record's dated-quote lines were brought into line with the correction.

## Verification

Offline, at implement time:

```
cd src-tauri && cargo test
cd src-tauri && cargo clippy --all-targets --all-features
npm run build
npm test
```

The live read, after the task review and Codex, only when the user says, with Ollama brought up per the runbook first:

```
cd src-tauri && MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT=http://localhost:11434 MARKET_SIGNAL_LOCAL_REASONER_MODEL=qwen3.5:122b-a10b MARKET_SIGNAL_LOCAL_EMBEDDER_MODEL=qwen3-embedding:4b MARKET_SIGNAL_LOCAL_EVAL_REPEATS=3 MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR=<dir> cargo test fixed_evidence_live -- --ignored --nocapture
```

It reads the following against the v38 and v39 tables.
Interpretation markers per call fall below the v39 read's 10.2 (v38 7.1), and the causes that remain are read.
The level-class downgrades fall — `price-level-mismatch` (v39 eight), `margin-implausible` (five), `no-level` (three) — and every kept core matches its sentence.
The kept cores' margin-to-level ratios do not drift toward the unshown caps (v39 non-price max 40%).
Every target rationale names one of the model's own figures and no prose field carries account economics.
The rung table under the List form stays inside its engine set, and the action rationales are read for claims that compute with sentences the interpretation packet no longer carries.
