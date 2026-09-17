# The §3 interpretation slice — fix list 3.1–3.3, 1.9 and 2.5 (2026-09-16)

## Scope and authority

The third local-model-call slice: fix list §3 (`2026-09-15-local-model-call-fixes.md`, the interpretation call), plus entries 1.9 and 2.5 ruled 2026-09-16 to land with it, one plan and one prompt stamp (`portfolio-v38`).
Entry 3.4 stays deferred by construction.
The plan's eight flags and assumptions were ruled by the user on 2026-09-16 before implementation.
A Codex plan review then raised four points, each verified against the code and ruled the same day.
All twelve rulings are recorded here.
The verification substrate is the fixed evidence set of fix list §8.1; the §8.2 live read of this slice follows the task review and Codex, and only when the user says.
Contract text belongs in the job docs named per entry; this record carries the rulings, the inventory and, once run, the read.

## Rulings — 2026-09-16

- **F1, 3.1 form: the model owns its target explanation.**
  `price_target_rationale` becomes `model_target_rationale`, the model explaining its own bands — the assumptions behind its base case and its departure from the engine's twelve-month base target — while the engine's methodology stays app-rendered beside the engine's own targets.
  The card moves the field from the engine column's methodology reveal to the model column.
- **F2, 3.2 reach: one identity-and-spot header serves every model-facing packet.**
  The interpretation, role/risk and research packets stop carrying quantity, cost basis and market value; the action header had held that form since `portfolio-v36`.
- **F3, the position-change line: direction only.**
  New, increased, decreased or unchanged stays, honoring §Holdings change tracking; the quantity and cost-basis figures go.
- **F4, the template's ledger numerics: domain notes adopted.**
  One note each for the three sibling scenario objects and their probability lean, the threshold as exactly the stated level, and the margin as the separate band; the `1` placeholders stay, stated as carrying no magnitude.
- **F5, the `fails` capital-efficiency wording: the draft ratified.**
  "CAPITAL EFFICIENCY: fails — even the bull case misses the hurdle over the assessed horizon, so the position is dead money. This is a weighed exit input beside the forward read, not a rung: it leans toward realizing some or all of the position where the forward prospects are independently poor, and it neither requires nor forbids any rung."
- **F6, the debut `what_changed` sentence: "New holding (no prior verdict)."**
- **A1, stamps: `portfolio-v38`, `checkpoint-v10`, evidence-floor unchanged, portability format 7 with v6 refused.**
  The archived `run_json` changes shape with the rename, following the v6 precedent; attempt 7's wipe also clears `portfolio_runs`.
- **A2, the live check's limit, restated after the Codex review (point 4):** the fixed set is all-debut, so the continuity shape of 3.3 is proven by new offline tests — key selection by debut flag on both branches, decode with the fields present, the persisted line on a continuity run and the app's sentence on a debut — and live continuity stays explicitly unverified.
- **Codex point 1 — 3.3a ruled explicitly: adopt.**
  The ledger schema's series enum is scoped to the vehicle's computable set on both branches; the 6g uncomputable-series downgrade stays as defense in depth.
  Entry 1.9 needed no new ruling (adopted 2026-09-16).
- **Codex point 2 — the interpretation overlay renders unsized: adopt.**
  Contract counts over the coverage ratio had given the held share count away; every packet now renders the C1 form, and the `sized` parameter is gone.
- **Codex point 3 — harness additions: adopt all three.**
  The verdict assembly is a pipeline helper the harness reuses; one action call per holding runs on a verdict built from the fresh interpretation; the prose fields print with a names-own-figure diagnostic that is never a gate.
- **Codex point 4 — A2 restated as above.**

## What landed

- `portfolio::GradedVerdict` and the `Interpretation` wire struct: `price_target_rationale` → `model_target_rationale`; the contract sentence asks for the model's own assumptions and the departure from the engine's base; the six attempt-6 fixtures rename the key (their content already explained the model's targets); the card renders it under the model column's kv list as "Target rationale" (F1).
- `pipeline::holding_header` is identity and spot on every packet (the former `action_holding_header` folded in); `describe_position_change` renders the direction only; `option_overlay_prompt_section` renders structure and ratios everywhere (F2, F3, Codex point 2).
- `ledger_schema(role_risk, is_fund)` filters the series enum by `LedgerSeries::computable_for`; `interpretation_schema(is_fund, debut)`, `role_risk_interpretation_schema(debut)`, the response contracts and both system prompts take the call's shape; `interpret_request` and `role_risk_request` pass it from the dossier (3.3a).
- `interpretation_keys(debut)` / `role_risk_keys(debut)` drop `what_changed` and `what_changed_entries` on a debut; `strip_debut_continuity_fields` removes them from the schema; `decode_response_body` completes a debut body through `complete_debut_response` before typing; `own_debut_continuity` writes `DEBUT_WHAT_CHANGED` and an empty row set on every analyst path; the `#[serde(default)]` on both wire structs is gone; the continuity contract states the two fields' relation once; the debut CONTINUITY line no longer instructs on `what_changed_entries` (3.3b, F6).
- `response_shape_contract` appends the ledger's field notes whenever the schema carries a ledger (F4).
- `LedgerSeriesContract::render` states that a quantitative condition's statement names its level in the series' unit (1.9); the `fails` line takes the ratified wording (2.5, F5).
- `graded_verdict_from_interpretation` is the one verdict assembly; the live harness prints each interpretation's prose with the names-own-figure and account-economics-phrase diagnostics and runs a "fresh-interpretation verdict" action call per holding (Codex point 3).
- Stamps: `portfolio::PROMPT_VERSION` → `portfolio-v38`; `store::CHECKPOINT_FORMAT_VERSION` → `checkpoint-v10`; `portability::FORMAT_VERSION` → 7, v2–v6 refused (A1).
- Tests: `attempt_6_interpretation_packets_carry_no_account_economics` (both branches, byte-identical under tax posture, cost, quantity and market value, the shared phrase lexicon); the header, position-line and overlay tests rewritten; the schema, template and contract-versus-schema tests run over stock and fund, debut and continuity; `the_interpretation_request_is_scoped_to_the_vehicle_and_the_debut_shape`; `a_debut_persists_the_app_written_continuity_fields_on_both_branches`; the contract-sentence and `fails`-line pins; the v6 refusal in portability; the card spec's target-rationale test.
- Docs: `portfolio-analysis.md` §The holding verdict, §The position thesis ledger, §Portfolio action, §What changed, §Holdings change tracking; `portfolio-workflow.md` §Step 6f; `data-portability.md` §Import flow and the table; the fix list's per-entry rulings.

## Known limits

- The fixed set is all-debut, so the live read exercises the debut path only; the continuity shape is proven offline (A2).
- The names-own-figure diagnostic matches a rounded figure from the model's six legs against the rationale text; a rationale that explains its bands without restating a number reads "NO" and is a human read, never a gate.
- The research brief lost the position header as a consequence of the shared header (F2); no research prompt test asserted the economics, so the header test is the pin.
- Attempt 7's wipe must also clear `portfolio_runs`: a persisted v37 verdict does not decode under the renamed field.
- Attempt 7's wipe must clear the `portfolio` namespace of `vector_memory` as well: attempt 6's six summary rows were embedded under `portfolio-v35`, when the model's own rationale could carry tax and cost language, and a fresh run's recall would render them (pre-release, no migration — the reset policy).

## Verification

Offline, at implement time: `cargo test` 1482 passed, 0 failed, 32 ignored (the other targets green); `cargo clippy --all-targets --all-features` 0 warnings, 0 errors (full output read); `npm run build` green; `npm test` 46 and 265 passed.

Task review (2026-09-16): approve-with-nits, every criterion passing on the first round.
Four nits taken: the role/risk continuity line is now pinned beside the debut sentence, the card spec covers the empty-rationale state, the semicolon-joined doc sentences are split, and the Step-6f restatements of the what-changed relation and the header rule are one-line pointers to their canonical sections.
The gate re-ran green after them (`npm test` 46 and 266).
The reviewer's chronology note — 1.9, 2.5, the v37 re-read and the §1 admission had carried a 2026-09-17 date in the prior commit — was ruled by the user: every one of those events happened on 2026-09-16, and the dates were corrected across the fix list, the follow-up record, the job docs and the code comments in this slice.

## Codex implementation-review corrections

Read-only review after the task review; Codex changed no files and ran no gate, so the full gate ran here after the fix.
One finding, P2, verified against the code and fixed:

1. The 3.2 isolation was not closed through memory: `holding_summary_text` embedded the persisted `action_rationale` with the app's tax caveat on both branches, and `semantic_recall_prompt_section` renders recalled summaries verbatim, so a prior taxable trim or sell could re-supply the position's unrealized gain or loss and the tax posture to the next intrinsic interpretation.
   The §1+§2 slice had accepted the caveat riding the embedding (its A3); that acceptance is superseded.
   Fix: `pipeline::investment_sentence` strips exactly the appended caveat, and the summary text embeds the model's sentence alone on both branches; the unit test pins the strip, the summary-text test covers both branches, and the recall test writes a taxed exit on each branch and reads it back at the next run's recall without the caveat.
   Static inspection proved the input route; whether it ever moved a rung was not established and is not claimed.
   The reset policy carries the stored rows: attempt 7's wipe clears the `portfolio` vector namespace (§Known limits).

```
cd src-tauri && cargo test
cd src-tauri && cargo clippy --all-targets --all-features
npm run build
npm test
```

The live read, after the task review and Codex, only when the user says:

```
cd src-tauri && MARKET_SIGNAL_LOCAL_EVAL_REPEATS=3 MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR=<dir> cargo test fixed_evidence_live -- --ignored --nocapture
```

It reads five things: every rationale explains the model's own targets (3.1); no prose field carries a cost or entry figure (3.2); SPMO, DIA and ARKF author no stock-only series and the debut calls spend no markers on `what_changed` (3.3); PSX and PGNY recover executable cores (1.9); SPMO's and ARKF's capital-efficiency markers fall toward the reworded holdings' share (2.5).
