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

## Live admission read — 2026-09-16

Run on commit `f48f582` after the task review and the Codex round, three repeats with thinking captured, Ollama v0.32.5 (one slot, the 128K context override, flash attention), `qwen3.5:122b-a10b`.
Fifty-four calls in 1 h 48 min — eighteen interpretation, thirty action on the fixture verdicts, six on a verdict assembled from each holding's first fresh interpretation — with no transport failure and no retry (interpretation 74–181 s, action 60–166 s).
The harness log and thought logs are `~/Downloads/market-signal-fixed-evidence-2026-09-16/run4-v38-read-3repeats-thinking.log` and `thought-logs-v38/`; the per-condition read below is made from the log.

### Cores beside their sentences

| Holding | Kept cores across the three repeats | Wrong cores caught | Other downgrades |
|---|---|---|---|
| TSLA | 4 (0 / 3 / 1): price above 480, gross-margin below 0.15 (15%), price above 468, price below 300, each matching | — | `qualifier` ×4 (two two-quarter durations, a volume clause, a multiple-level sentence); `margin-implausible` (50 on a 150 multiple) |
| PSX | 2 (1 / 0 / 1): net-margin below 0.045 (4.5%) and 0.035 (3.5%), matching | 1 `price-level-mismatch` (a $192 sentence on a core of 21) | `qualifier` ×2 (volume; "without") |
| SPMO | 5 (0 / 2 / 3): price below 130 and 132 "on two consecutive days", return-volatility above 0.045 (4.5%), price below 130 and 135, each matching | 1 `level-mismatch` (a converted-annualized volatility figure) | `qualifier` (a two-week duration) |
| ARKF | 4 (1 / 1 / 2): price below 32, return-volatility above 0.025 (2.5%), price above 52, price below 38, each matching, three on cadence-exact clauses | — | `qualifier` ("without") |
| DIA | 5 (2 / 1 / 2): price below 478, 475, 460, 480 and 480, each matching | 1 `comparator-mismatch` (a "below $480" sentence on an `above` core) | — |
| PGNY | 2 (2 / 0 / 0): net-margin below 0.04 (4%) and ps-ratio below 1.3, matching | — | `margin-implausible` ×3 (0.015 on 0.02, 10.5 on 24, 1 on 0.05); `qualifier` ×3 (a two-quarter duration, volume, "without … over two consecutive quarters") |

Twenty-two kept cores, twenty-two matching a figure their sentence states in the series' unit; the v37 re-read had twenty-one of twenty-one.
Three wrong cores caught, four implausible margins caught, and no levelless core authored anywhere — the v37 re-read caught six `no-level` downgrades, so the 1.9 sentence removed levelless prose outright.
Yield is twenty-two cores over eighteen calls (v37: twenty-one); PSX and PGNY each rose from one core to two, short of run 1's four, because their remaining conditions now spend themselves on durations and, on PGNY, on noise bands too wide for the relative cap.
The buffered-threshold habit did not appear on any call.
The three fund holdings authored no stock-only series across nine calls, where the v36 read had SPMO author a P/E core the seam downgraded as uncomputable (3.3a).
One kept core rides a cadence gap: TSLA's "$300 for two consecutive weekly closes" kept its core because the duration parser reads "closes" as the unit and tolerates "weekly" as an adjective (finding L10).

### The rationale, the prose and the debut fields

Eighteen of eighteen target rationales explain the model's own bands and their departure from the engine's twelve-month base, each naming one of the model's own figures — TSLA $240 / $240 / $260 against the engine's $163, PSX $265 / $275 / $260 against $319, SPMO $156 / $138 / $135 against $138, ARKF $36.50 / $42 / $36 against $41, DIA $490 / $495 / $505 against $517, PGNY $32 / $32 / $36.50 against $35.51 (3.1).
Of seventy-two prose fields the account-economics scan flagged one, ARKF's first financial summary, on "$13.4B realized/unrealized losses for ARK Management" — the issuer's own losses from the distilled research, the case the fixture lexicon excludes for the same reason; no field carried the position's cost, quantity, value or P/L (3.2).
Every debut call returned the app-written continuity line with no rows, and the thinking across all fifty-four calls mentions `what_changed` zero times against ninety-three on the v37 re-read; `probability_pct` fell from thirty-one mentions to eighteen and the quant `margin` placeholder from ten to two (3.3b, F4).

### The rung under repeats and variants

| Holding | Engine set | Repeats 1–3 | Tax-exempt | Cost basis ×3 | Fresh-interpretation verdict |
|---|---|---|---|---|---|
| TSLA | sell-all, trim, hold | trim ×3 | trim | trim | trim |
| PSX | sell-all, trim, hold, add | trim ×3 | trim | trim | trim |
| SPMO | sell-all, trim, hold | sell-all, trim, sell-all | sell-all | trim | trim |
| ARKF | sell-all, trim, hold | trim, sell-all, trim | sell-all | sell-all | sell-all |
| DIA | sell-all, trim, hold | hold ×3 | hold | hold | hold |
| PGNY | sell-all, trim, hold, add | hold ×3 | hold | hold | hold |

No rung outside its engine set (0 of 36), no rationale carrying tax, cost, quantity or P/L language, and every variant — the tax posture, the cost basis, and the verdict assembled from a fresh interpretation — landed inside its holding's repeat set.
Four holdings held one rung across all six calls; SPMO and ARKF, the two `fails` holdings, moved between sell-all and trim on byte-identical packets as they did on the v36 and v37 reads.
Every `fails` rationale reads the capital-efficiency line as a fact ("capital efficiency fails because expected returns miss hurdles even in bull cases"), the reading 2.5 asked for.

### Re-think markers

| Call class | v37 re-read (48 calls) | v38 read (54 calls) |
|---|---|---|
| Interpretation, words / markers per call | 2,474 / 11.9 | 1,996 / 7.1 |
| Interpretation, worst call | 28 (ARKF), 27 (TSLA), 20 (PGNY) | 10 |
| Action, words / markers per call | 2,404 / 12.9 | 2,413 / 13.0 |
| Action, capital-efficiency markers per call on SPMO / ARKF | 7.0 / 6.8 | 4.5 / 3.2 |
| Action, capital-efficiency markers per call on the four reworded holdings | 3.4 / 0.6 / 1.6 / 2.4 | 3.3 / 1.5 / 2.5 / 3.7 |
| Action, ENGINE SET share of markers | 28% | 30% |

Interpretation churn fell forty percent and the bimodality is gone: fifteen of eighteen calls sit between six and ten markers, the other three are short calls at zero, and no call resembles the v37 outliers.
The residual interpretation churn is polarity re-checks on the engine's sub-scores ("valuation 97 — meaning cheap?"), the fence question ("raw JSON or a code block?"), and whether a macro key driver must map to an engine series before it may carry null (findings L11–L13).
Action churn is flat in aggregate.
The `fails` line's rewording cut the capital-efficiency churn on SPMO and ARKF into the reworded holdings' band (2.5's check), and what remains there is genuine deliberation between trim and sell-all under an aggressive profile.
The ENGINE SET line is unchanged at thirty percent of the action markers: the model still reads the admitted list as a hidden recommendation or exclusion ("does this imply the engine doesn't recommend sell-all?") despite 2.2's declarative form (finding L15), and "keep the action firm run to run" costs a check on every debut call (finding L14).
Per fix list 8.4 the counts are a diagnostic beside the tables, never the gate.

### Findings from the read

Each is a candidate for the ruling round, not a change made here.

- L10 — The cadence exemption admits a weekly close.
  `duration_clauses` reads "closes" as the unit and tolerates "weekly" as one of the two adjectives it allows between the count and the unit, so "two consecutive weekly closes" passes as the market-data cadence.
  Proposed: a weekly, monthly or quarterly adjective on the unit defeats the exemption; the core is otherwise sound.
  Ruled 2026-09-16: adopt (fix list 3.5).
- L11 — Interpretation re-checks the engine's sub-score polarity.
  The interpretation prompt's sub-score line glosses only the risk axis; the action packet's polarity clause glosses every axis and its polarity bucket is small.
  Proposed: gloss every axis on the interpretation line, mirroring the action packet's clause under the 2.3 contract.
  Ruled 2026-09-16: adopt (fix list 3.6).
- L12 — The fence question persists on the interpretation call.
  "The entire response is one JSON object beginning with {" still leaves the model deciding whether a code block is expected; the grammar makes a fence impossible, so the deliberation is pure cost.
  Proposed: close the shape contract's sentence with "with no code fence or surrounding prose".
  Ruled 2026-09-16: adopt (fix list 3.7).
- L13 — The key-driver series rule invites a search for a mapping.
  The model deliberates whether a macro driver such as Fed policy must map to an engine series before settling on null.
  Proposed: state the ledger doc's own rule in the prompt — a driver with no series in this holding's list carries null, and most drivers will.
  Ruled 2026-09-16: adopt (fix list 3.8).
- L14 — "Keep the action firm run to run" fires a check on a debut.
  Proposed: render the sentence on a continuity call only.
  Ruled 2026-09-16: not adopted — the check is accepted cost.
- L15 — The ENGINE SET line is the action call's remaining hotspot.
  Thirty percent of the action markers, unchanged since 2.2, on the model inferring a recommendation or an exclusion from the admitted list.
  Two forms, either a contract change to §Portfolio action: render the facts the set derives from (the new-money admission result, the hurdle state, the grade bar) in place of the derived list, so nothing is left to reverse-engineer; or drop the list from the packet and keep it audit-side.
  The 8.3 non-thinking action experiment, already ruled for after §1–§6, removes the churn by construction if judgment quality holds and now has a clean thinking-on baseline.
  Ruled 2026-09-16: A/B on the fixed set first — the contract stands; the harness gains a variant that renders the set's underlying facts in place of the list, eighteen action calls each way, and the form is ruled on that evidence (fix list 3.9).
- Watches, no entry: PGNY's noise bands are too wide on three of three calls, each caught by the relative cap at a yield cost; the prose scan's issuer-losses false positive stays a diagnostic.

### Verdict

Against fix list 8.2 as ruled: every executable core admitted agrees with its sentence, no levelless core was authored, tax and cost variation cannot move the rung by construction or on the live table, every rationale explains the model's own targets, no account economics reached any prose field, the fund calls authored no stock-only series, and every debut carried the app's continuity fields.
**§3 is admitted** (2026-09-16).
Entry 1.9 is admitted with its yield read honestly: levelless prose is gone, and PSX and PGNY recovered to two cores each, not four.
Entry 2.5's check is met.
L10 through L15 are yield and clarity, flags for the ruling round, not conditions on the admission.
Ruled 2026-09-16: L10–L13 and the L15 A/B land as one small residue slice ahead of §4, its own plan and stamp (`portfolio-v39`), mirroring the v37 follow-up; L14 stays as it is.
