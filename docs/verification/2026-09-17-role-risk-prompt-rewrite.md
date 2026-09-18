# The role/risk interpretation-prompt rewrite — `portfolio-v42` (2026-09-17)

## Scope and authority

The `portfolio-v40` interpretation rewrite set the principle: a prompt never explains the app or its concepts to the model (`2026-09-17-interpretation-prompt-rewrite.md`).
The `portfolio-v41` action rewrite carried it to the action call (`2026-09-17-action-prompt-rewrite.md`).
The role/risk interpretation prompt was the last pre-v40 shape on the interpretation side.
Its system prompt stated the app's routing rule and the schema's absences.
Its user prompt carried instructions inside its inputs, and its ledger block was the old rewrite paragraph in full, naming "engine series" five times, the validator's downgrades, a gross-margin example on a vehicle with no gross margin, and an add family the schema does not offer.
Two values rendered twice, one at full float precision.
On a continuity call the prior role read never reached the packet, though a role-read change row must state its old value.
The evidence-gap strings the engine forwarded verbatim were the app explaining its own routing, and they reached the action packet and the card by the same field.
The user asked for the prompt audited and adjusted; the audit and the aligned draft were written beside today's render (`~/Downloads/market-signal-fixed-evidence-2026-09-16/role-risk-prompt-rewrite.md` and `role-risk-prompts-v41-synthetic-bnd.md`), the draft's eleven rulings and the plan's three flags and four assumption groups taken through the selector.
Fix list 3.18 is the entry; this record carries the rulings, the inventory and, once run, the read.

## Rulings — 2026-09-17

The eleven on the prompt text:

- **The shape: placeholder-only.** The role/risk message closes with `role_risk_return_shape` on the one renderer; the v40 A3 scoping and the 2026-09-16 F4 field notes are superseded for this prompt.
- **MARKET ANALYSIS: the shared renderer, stances included.** The audit's house-view predicate is the one predicate both renders read.
- **PRIOR ANALYSIS on continuity: added.** The prior class and the prior role read verbatim, with the prior read's vintage.
- **The evidence-gap strings: reworded at the source as data.** Every classification reason string; the interpretation packet, the action packet and the card change together.
- **The reported asset class: rendered** beside the class label.
- **PRICE VS NAV: trimmed to a unit gloss** on every packet that renders the line.
- **The fund-scoped ledger item: both fund variants.** The threshold example names a fund series and key_drivers carries the fund driver clause on the priced fund message too.
- **The role line: "an investment analyst producing an independent read of one fund holding".**
- **"Read in isolation": dropped.** Input isolation enforces it.
- **The fund's own OPTIONS ACTIVITY: left out**, as today.
- **Live coverage: a synthetic role/risk fixture in the live harness**, labelled synthetic.

The seven from the plan:

- **F1, the two closed-end gap strings: "on the current data surface" dropped** on the same rule.
- **F2, the synthetic fixture's research summary: hand-written.**
- **F3, the reported asset class: the role/risk packet only.**
- **A1, stamps: `portfolio-v42` only.** `checkpoint-v10`, `evidence-floor-v5` and portability format 7 stand, since no persisted shape changes.
- **A2, rendering: as assumed.** The reported asset class renders as served and is omitted when absent; the structure line reads "Structure: leveraged / inverse, resetting daily." or "Structure: option overlay; the options reshape the return path."; EXPOSURE TILT renders the readout's rows as computed; the prior role read renders verbatim.
- **A3, harness route: as assumed.** The continuity render comes from a stub first run and a capturing analyst on the second; `ROLE_RISK_KEYS` in output order; the audit's summary-only house-view claim flips to claimed.
- **A4, records: as assumed.** This record; the v40 and v41 records untouched; one live read covers v40, v41 and v42.

## What landed

- `role_risk_system_prompt` is the role line, the two-part shape and `role_risk_response_contract`, now "You will return role_summary and ledger, as one JSON object." on a debut and the four names on continuity, through the one `response_contract_line` the priced and action contracts share.
- `role_risk_user_prompt` is `======== PART 1: INPUTS ========` then `======== PART 2: TASK ========`.
  Part 1 opens with the shared holding header and the position sentence; then CLASS (the label, "Reported asset class: …" where the fund metadata carries one, the structure line where one applies), EXPOSURE TILT with its basis glossed once and the readout's rows, the closed-end PRICE VS NAV line where it applies, the shared positioning line, RISK PROFILE (annualized realized volatility with its unit, the shared options backdrop), EVIDENCE GAPS, the shared FINANCIAL METRICS, RESEARCH SUMMARY, the shared MARKET ANALYSIS; on a continuity call PRIOR ANALYSIS (the prior class and role read, or the sentence that the prior was not a role and risk read), PRIOR ANALYSIS NOTES and CHANGES SINCE THE PRIOR ANALYSIS; then the shared prior-ledger data and crossings.
  Part 2 is item 1 (role_summary, naming the sections that rendered), item 2 (the shared ledger item with trim and sell families, the thesis line drawing on MARKET ANALYSIS), on continuity items 3 and 4 (the shared what-changed items with "the role read, the scenario or the condition" and no parameter sentence), and RETURN SHAPE.
- Shared pieces extracted from the priced message, behaviour unchanged: `financial_metrics_section`, `ledger_task_item` (parameterised on the item number and the message's branch — priced stock, priced fund or role/risk — which fixes the fund form, the trigger families and the market-analysis reference), `what_changed_task_items` (the detail gloss and the parameter sentence), and one `prompt_renders_house_view` in place of the two predicates.
- On a fund the shared ledger item's threshold example reads `("above 0.75%" on expense-ratio is 0.0075)` and its key_drivers line carries "for a fund, the exposure it supplies, its cost and its fidelity to its mandate"; the priced fund message changes with it.
- Removed: `LedgerSeriesContract::render`, `ledger_prompt_section`, `ledger_rewrite_instructions`, `what_changed_instructions`, `role_risk_prompt_renders_house_view`, `priced_prompt_renders_house_view`.
- `mod.rs`: `ROLE_RISK_KEYS` in output order; `role_risk_return_shape` through `placeholder_shape` with `SHAPE_KEY_ORDER` (the priced order with `role_summary` first); `response_shape_contract` stays for distillation only.
- `fund.rs`: the nine classification reason strings and the two closed-end gap strings are data statements.
  A bond fund reads "no duration, credit or yield-curve data for this fund".
  A commodity fund reads "no exposure-priced valuation for a commodity fund".
  A leveraged or inverse vehicle reads "leveraged / inverse exposure with a daily reset".
  An option-overlay fund reads "the option overlay's payoff is not in the exposure data".
  A weightless equity fund reads "no usable sector weightings".
  An equity fund under the guard reads "US exposure 67%, below the 70% floor for a US sector read".
  A closed-end fund with no metadata reads "no fund metadata for the closed-end fund: class and weightings unavailable".
  An unsupported class reads "reported asset class "…" is not an equity, bond or commodity class; any sector weightings are exposure context only".
  An unresolved class reads "no reported asset class and no usable sector weightings".
  The two closed-end gap lines read "fund metadata (etf/info) is empty for the closed-end fund: expense ratio and exposure unavailable" and "price-vs-NAV unavailable: …".
- `nav_premium_line` reads "PRICE VS NAV: -7.2% (discount): the closed-end fund's market price against its net asset value." on every packet.
- `role_risk_verdict_from_interpretation` extracted from `analyze_holding`, beside `graded_verdict_from_interpretation`.
- The offline stub's research text loses its product name.
- The harness gains the synthetic role/risk fixture (a total bond market ETF at $72.38 with twenty-one hand-written closes, a hand-written research summary, a hand-written Treasury positioning line, the fixed set's house view and options backdrop; the readout the fund engine's own on those inputs), a capturing analyst for the continuity render, the pin `synthetic_role_risk_messages_are_two_parts_with_no_app_concept` (debut and continuity: the two parts, the section order, each value once, the banned lexicon over the whole message, the shape's keys, the role line), the pin `synthetic_role_risk_action_message_is_two_parts_with_no_app_concept` (the v41 packet's role/risk branch, closing that record's offline-only limit), the dump's section 8 (both system prompts, the debut message, the continuity message, the action message), and the live harness's synthetic block after the six holdings (each repeat one interpretation validated on the role/risk branch with the per-condition read, then one action call on that repeat's assembled verdict and validated ledger, with the permission scan and the prose diagnostics).
- Stamp: `PROMPT_VERSION` → `portfolio-v42`.
- Docs: `portfolio-workflow.md` §Step 6f (the role/risk call's paragraphs), `portfolio-analysis.md` §The position thesis ledger and §Intrinsic verdict; fix list 3.18; the fixtures README.

## Task-review corrections

The task review returned approve-with-nits; the two coverage gaps and the four nits were taken through the selector (2026-09-17).

1. Plan step 2's pin was missing: the v40 interpretation pin now asserts the fund threshold example and driver clause on the three funds and the stock example, with no driver clause, on the three stocks.
2. Plan step 3's assertion was missing: `GAP_ROUTING_WORDS` (on-plan, honestly, degrade, unsound, this pipeline, current data surface) and `assert_no_routing_words` run over every action packet on the fixed set and over the synthetic role/risk messages and action packet.
3. The live harness's priced loop calls `print_validated_ledger` instead of carrying its own copy of the per-condition read.
4. `ledger_task_item` takes the message's branch (`LedgerItemBranch`: priced stock, priced fund, role/risk) in place of three positional parameters; the trigger-family enum is gone.
5. The PRIOR ANALYSIS fallback line reads "The prior analysis was not a role and risk read." with no reach clause.
6. This record's string inventory is one sentence per string.

## Codex implementation-review corrections

Read-only review after the task review; Codex changed no files and ran no gate, so the full gate ran here after the fix.
One finding, P2, verified against the code and the draft and fixed:

1. The live harness's synthetic block made one action call on the first completed interpretation, while ruling 11 specifies one interpretation and one action call per repeat, so with three repeats the action's variability went unmeasured.
   Fix: each repeat's action call runs on that repeat's assembled verdict and validated ledger, labelled with the repeat number.

## Known limits

- The role/risk branch's live behaviour is read on synthetic evidence only: the fund, its closes, its positioning and its research are hand-written, so the read tells the prompt's shape on a fund of that class, never a run's numbers; a real role/risk holding waits on a run that produces one.
- The continuity render for the pins and the dump comes from the stub's first run, so its research summary is the offline stub's and its prior ledger the stub's debut ledger; the live harness reads the debut only.
- The lexicon pin scans the whole synthetic message because the stub's prose and the hand-written research carry no banned word; on a live call the role read is printed with the diagnostics, never gated.
- The reason strings render lowercase without a closing period, since the gap list joins them with semicolons; the draft's capitalised line was a rendering nit.
- The reworded reason strings change the card's text; no component spec pinned the old wording.

## Verification

Offline, at implement time:

```
cd src-tauri && cargo test
cd src-tauri && cargo clippy --all-targets --all-features
npm run build
npm test
cd src-tauri && MARKET_SIGNAL_LOCAL_EVAL_PROMPT_DUMP=~/Downloads/market-signal-fixed-evidence-2026-09-16/prompts-v42.md cargo test --lib fixed_evidence_prompt_dump -- --ignored
```

The dump's section 8c matches the ruled draft's debut message line for line, the gap line's capitalisation aside.

The live read, after the task review and Codex, only when the user says, with Ollama brought up per the runbook first:

```
cd src-tauri && MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT=http://localhost:11434 MARKET_SIGNAL_LOCAL_REASONER_MODEL=qwen3.5:122b-a10b MARKET_SIGNAL_LOCAL_EMBEDDER_MODEL=qwen3-embedding:4b MARKET_SIGNAL_LOCAL_EVAL_REPEATS=3 MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR=<dir> cargo test fixed_evidence_live -- --ignored --nocapture
```

One read covers v40, v41 and v42; the v40 and v41 records own their own lists.
This record's list, on the synthetic bond fund:
no role read, thesis, scenario row or rationale carries a banned word or an account-economics phrase;
the ledger's kept cores stay on the fund series with trim and sell families and the margins do not drift toward the unshown caps;
the role/risk action rationale reads from the packet's sections, names no hurdle and argues from no permission;
the role read describes the mandate, the exposure, the cost and the risk without a portfolio;
and the markers per call are counted beside the v40 and v41 tables.
