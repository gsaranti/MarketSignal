# Ledger conditions and the action packet — the first local-model-call slice (2026-09-16)

## Scope and authority

The first slice of the local-model-call fix list (`2026-09-15-local-model-call-fixes.md` §1 and §2, ruled 2026-09-15 as one plan and one `portfolio-v36` stamp).
The plan's fourteen flags and assumptions, and the two decisions the Codex plan review added, were ruled by the user on 2026-09-16 before implementation; the rulings are recorded here and, per entry, on the fix list.
Codex reviewed the plan in three rounds (partial isolation, the missed disagreements, the margin guard, independent semantic judgment, the frontend build; then the level-association gap) and approved proceeding.
The verification substrate is the fixed evidence set of fix list §8.1, reconstructed from the six persisted attempt-6 holdings (`src-tauri/src/portfolio/fixtures/attempt-6/`).

## Rulings — 2026-09-16

- **F1, 1.2 guard form: the narrow prose-mismatch form, basis leg included.**
  No universal decimal-above-1 rule and no spot-proximity rule.
- **F2, 1.3 margin: not adopted — the margin stays model-authored.**
  F2b: 1.2 gains a margin guard — a margin at or beyond the threshold's magnitude downgrades, a zero threshold exempt (Codex: equality already moves a "below" boundary to zero).
- **F3, 1.4 qualifier classes: duration, volume and second-condition clauses.**
  A generic "on <event>" clause is not detected; "without", "unless", "confirmed by", "accompanied by" and their kin are.
- **F4, 2.1 caveat: appended to the rationale by the app, on trim or sell-all only, under a tax-aware profile on a nonzero P/L.**
  No persisted-shape change; the model's sentence is never edited.
- **F5, packet scope: quantity and market value are withheld beside cost basis, P/L and the tax row.**
- **F6, wording ratified.**
  The ENGINE SET line: "the engine arm's rules admit [...]; its own pick is undisclosed. The full ladder is yours; this set is one input."
  The polarity clause on both arms: "0-100, higher is better on every axis: quality, valuation = more attractive, momentum, risk = more resilient".
- **F7, reason typing: a String opening with its class and a colon.**
- **F8, fixtures: reduced, in-repo, synthetic economics (ten units; a gain on four holdings, a loss on ARKF and PGNY).**
- **F9, the live harness: built in this slice at bounded fidelity.**
- **A1, stamps: only `PROMPT_VERSION` moves (`portfolio-v36`); `checkpoint-v9` and `evidence-floor-v5` stay.**
- **A2, tolerances: 5% relative on a percent or multiple, 1% on a price.**
  The implementation review correction preserves explicit signs and interprets narrow implicit-decline wording only on trailing return.
- **A3, the appended caveat rides into the Step-7 memory embedding: accepted.**
- **A4, DIA's persisted financial summary carries invented per-share cost figures: a known carry to fix list 3.2.**
- **A5, current observations render on both interpretation branches.**
- **C1, the option overlay in the action packet: structure and ratios only.**
  Class, coverage ratio, net delta as a fraction of the held shares, each leg's direction, kind, strike, expiry and delta; no contract counts, no share-equivalents.
- **C2, synthetic role/risk and prior-ledger continuity fixtures: offline halves added now, labelled synthetic.**
- **Codex corrections folded in:** a comparator check; a level check reading the comparison clause with ambiguous associations kept qualitative (paired tests prove `above 120` passes and `above 100` fails on "rises from $100 to above $120"); the margin guard at equality; `npm run build` in the verification command.

## What landed

- `engine::LedgerSeries` gains `percent_unit`, `unit_note`, `confirmation_note` and `statement_aliases`; `describe` drops "(decimal)" now that the unit rides beside it.
- `pipeline::validate_quant_core` replaces `parse_quant_core` at the 6g seam with the ordered checks — series, unit, comparator, metric, basis, level, margin, qualifier — and the `downgrade_class` prefixes; the dedup and supersession pre-pass read the same validator.
- `pipeline::LedgerSeriesContract` renders the holding-scoped authoring contract in both interpretation prompts, with the branch's worked examples; the role/risk branch's metric surface is built once by `fund_ledger_metrics` for the evaluation, the audit and the prompt.
- The action packet: `action_holding_header` (identity and spot), no P/L line, no tax row, the set stated once, the polarity clause on both arms, the overlay unsized, the pre-profit departure clauses removed; `tax_caveat` and `with_tax_caveat` append the fixed sentence at both call sites.
- `portfolio::PROMPT_VERSION` = `portfolio-v36`; the action response contract's example drops the tax phrase.
- `portfolio::fixed_evidence` (test-only): the fixture loader, the 6g replay, the packet isolation and invariance checks, the two synthetic cases, and the `#[ignore]`d `fixed_evidence_live` harness.
  With `MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR` set the harness also captures every call's thinking through the dev app's thought-log sink, fenced per call, so the re-think read runs on the fixed set (added after the live read, 2026-09-16).
- Docs: `portfolio-analysis.md` §The position thesis ledger and §Portfolio action, `portfolio-workflow.md` §Step 6f and §Step 6g, `configuration.md` §Investor Profile, and the fix list's per-entry rulings.

## The fixed evidence set — expected 6g outcomes

Every attempt-6 condition replayed through the new seam lands as below (`attempt_6_conditions_resolve_to_their_expected_6g_outcome`).

| Holding | Kept quantitative | Downgraded, by class | Authored qualitative |
|---|---|---|---|
| TSLA | gross-margin falsifier | NHTSA `no-price-level`; ≤$145 add `price-level-mismatch`; operating margin `metric-mismatch`; $485 trim `margin-implausible`; two-quarter sell `qualifier` | — |
| PSX | price below $210 | quarterly net margin `basis-mismatch` | — |
| SPMO | — | P/E `series-uncomputable` (as on the run); both volatility cores `margin-implausible` | two concentration conditions |
| ARKF | — | $55 for two weeks `qualifier`; $38 on volume `qualifier` | thematic rotation |
| DIA | both price cores | — | three |
| PGNY | price below $22 | both growth cores `unit-mismatch`; P/E >40x without acceleration `qualifier` | — |

The plan's table had PGNY's P/E condition kept; under F3 its "without corresponding margin/growth acceleration" is a second condition, so it downgrades as `qualifier`, and the fixture encodes that.
Fix list 1.2's seven named defects each downgrade with their class; DIA's cores pass unchanged; ARKF's two are handled by 1.4.

## Known carries and limits

- The model-authored prose the action packet embeds (the current and prior financial summary, the continuity summary, the what-changed rows) is the remaining route account economics can reach the rung by; in the live store DIA's summary carries invented per-share cost figures and PGNY's self-assessment an implied entry price until fix list 3.2 removes them at the source.
  This slice is partial isolation of the packet, not the complete 2.1 acceptance check.
- The fixtures are scrubbed of those phrases (review R1): every rationale and self-assessment is replaced by a labelled placeholder and DIA's position sentence is removed, and a test greps every fixture string for them.
- The appended tax caveat rides into the holding's Step-7 summary embedding (A3).
- The concentration question the user raised — trimming a holding because it dominates the account — is the planner's by the 2026-08-14 tunnel-vision ruling and is not decided by today's app; recorded as an open planner item, not a change here.
- The live harness reconstructs at bounded fidelity (review R2 corrected the label): the engine numbers, persisted options readings, distilled research and house view are exact; the ledger contract's market-data observations resolve from the authoring close seeded as the one dated print, while its filing-series observations render unavailable because the statement rows are not persisted; the fund context, the option overlay and the pre-profit overlay stay absent.
  Its output is labelled reconstructed and names those limits per holding.
- The §8.2 admission read was run 2026-09-16 after implement time; §Live admission read below carries the table, the findings and the verdict.

## Verification

Offline, at implement time:

```
cd src-tauri && cargo test          # 1478 passed, 0 failed, 32 ignored (the workspace's other targets green)
cd src-tauri && cargo clippy --all-targets --all-features   # 0 warnings, 0 errors (full output read)
npm run build                       # vue-tsc + vite green
```

The live gate, run 2026-09-16 (§Live admission read):

```
cd src-tauri && MARKET_SIGNAL_LOCAL_EVAL_REPEATS=3 cargo test fixed_evidence_live -- --ignored --nocapture
```

## Codex implementation-review corrections

Authorized after the independent read-only review of this slice.
Explicit numeric signs survive the level check; opposite signed thresholds no longer agree, while narrow implicit-decline wording on trailing return remains supported.
Multiple comparison levels downgrade to qualitative, and percentage-unit validation uses the associated trigger level instead of any starting value in the sentence.
The reconstructed dossier restores each fixture's persisted options readings rather than inheriting the generic test helper's sample values.
Regression coverage was added for signed levels, multiple comparisons, starting-versus-trigger percentage units, and options evidence in the rendered interpretation packet.
Codex ran no build or tests; the gate run afterwards found one regression — its implicit-decline lexicon covered "falls more than" but not "collapses more than", so a reworded carried condition read as +40% against a −0.40 core and downgraded — closed by extending the lexicon (`IMPLICIT_DECLINE_PHRASES`) to the collapse, plunge, slide, lose and drawdown forms.
The signed-level rule refines ruling A2 (levels no longer compare on magnitude alone) and is recorded here as the standing form.
The action documentation now distinguishes the model's investment sentence from the app's appended caveat and removes stale claims that tax posture and unrealized P/L enter the action digest.
Builds and unit tests were not rerun for these corrections under the user's existing waiver; the earlier implementer-reported gate results above predate these corrections.

## Live admission read — 2026-09-16

Run on the fixed set against Ollama v0.32.5 (one slot, the 128K context override, flash attention) with `qwen3.5:122b-a10b`, three repeats per call, thinking on, sampling as the pipeline sets it.
Eighteen interpretation and thirty action calls completed in 1 h 49 min with no transport failure and no retry (interpretation 68–303 s, action 71–211 s).
The read is made from the harness table; the per-condition notes behind it stayed in the session.

### Cores beside their sentences (§1)

| Holding | Kept cores across the three repeats | Downgrades, by class | Qualitative |
|---|---|---|---|
| TSLA | gross-margin below 0.17, below 0.15 (repeats 2 and 3), each matching its sentence's 17% / 15% | `qualifier` ×4: a two-quarter duration, "without margin expansion", "confirming broader multiple compression", a volume clause | 5 |
| PSX | net-margin below 0.03 and 0.04 and price above $320 (a trim trigger) matching; net-margin below 0.035 on a sentence naming no level | `ambiguous-level` (a $190–$210 zone); `qualifier` ("without commensurate earnings expansion") | 3 |
| SPMO | price below 135, return-volatility above 0.024, price below 130 (repeat 2), each matching and each phrased "for two consecutive distinct daily sessions" | `qualifier` (two levels plus a duration); `level-mismatch` ×4 (0.12 vs 0.0238; 125 vs 128; 133 vs 135; 0.03 vs 0.028) | 2 |
| ARKF | return-volatility above 0.025 and 0.028 (repeat 2), price above $52 (repeat 3), matching | `price-level-mismatch` (a $62 sentence on a core at the spot, 46.43); `margin-implausible` (0.25 on 0.03); `qualifier` ×2 ("for two consecutive sessions" on price; "confirming bear case extension") | 5 |
| DIA | return-volatility above 0.025, price below $490 (a trim trigger, margin 489.5), price below $485, levels matching | `margin-implausible` (490 on 481); `qualifier` ×3 (a unit-restating parenthetical; "confirming short-term breakdown"; "without commensurate value addition") | 4 |
| PGNY | net-margin below 0.04 twice, matching; revenue-growth below 3 and price below $24 (repeat 3), both on sentences naming no figure | `no-price-level`; `qualifier` ×2 (volume; "without earnings acceleration"); `margin-implausible` (50 on 23) | 2 |

Sixteen of the nineteen kept cores match a figure their sentence states, in the series' unit.
The other three ride sentences that name no figure, and one of the three is wrong: PGNY's revenue-growth floor of 3 means 300% on a fraction series, the attempt-6 unit defect in a shape the check cannot see.
Of the twenty-three downgrades, eighteen are the rule doing its work, four of them on cores that did not mean what their sentence said (SPMO's repeat 3 buffered every threshold away from its stated level; ARKF put the spot where the sentence said $62).
Five downgrades lost a sound core to the lexicon's reach: three on an interpretive "confirming …" (TSLA $320, ARKF $32, DIA $490), one on "for two consecutive sessions" on price (ARKF $52, exactly the market-data cadence), one on a parenthetical restating a level in another unit (DIA 1.2% daily / 19% annualized).
None of attempt 6's metric, basis or uncomputable-series defects recurred; its implausible-margin shape recurred three times and was caught each time, and once more at a margin of 489.5 on a $490 threshold, which the ruled magnitude bound admits.
Executable yield is low on every holding — nineteen kept cores over eighteen calls, at most three on one call and none on five calls — because the model keeps authoring durations, second clauses and buffered thresholds that 1.2 and 1.4 send qualitative, and on DIA's second repeat authored a computable $580 price condition with no core at all.

### The rung under repeats and variants (§2)

| Holding | Engine set | Attempt 6 | Repeats 1–3 | Tax-exempt | Cost basis ×3 |
|---|---|---|---|---|---|
| TSLA | sell-all, trim, hold | trim | sell-all, trim, trim | trim | trim |
| PSX | sell-all, trim, hold, add | trim | hold, trim, trim | trim | trim |
| SPMO | sell-all, trim, hold | trim | sell-all, sell-all, trim | sell-all | sell-all |
| ARKF | sell-all, trim, hold | sell-all | sell-all ×3 | sell-all | sell-all |
| DIA | sell-all, trim, hold | hold | hold ×3 | trim | hold |
| PGNY | sell-all, trim, hold, add | sell-all | hold ×3 | hold | hold |

No rung landed outside its engine set (0 of 30), and no rationale carried tax, P/L, cost or quantity language.
Nine of the thirty rationales lead on the resolved capital-efficiency facts (ARKF's and SPMO's exit input), and the two sub-score readings that attempt 6 got both ways (PSX momentum, DIA valuation) read with the stated polarity.
Three holdings held one rung across all five calls; the other three moved between adjacent rungs on a byte-identical packet.
The one variant outside its repeat set, DIA tax-exempt on trim, is a fourth sample of the same prompt, since `attempt_6_action_packets_carry_no_account_economics_and_are_tax_invariant` pins the rendered packet identical across the tax posture and across cost, quantity and market value.
Tax and P/L variation therefore cannot move the rung by construction, and what the live wobble measures is the model's own sampling variance between adjacent rungs, which bounds how sharp any rung claim from three repeats can be.

### Findings from the read

Each is a candidate for the ruling round, not a change made here; the code locus is `pipeline::validate_quant_core` and its lexicons.

- L1 — A sentence naming no figure passes with any core.
  The unit and level checks compare against a figure read from the sentence, and `no-price-level` tests for the price series' alias rather than a number, so "Price retests strong support zone" passes it.
  Observed: PGNY revenue-growth below 3 (300%), PGNY price below $24, PSX net-margin below 0.035.
  Proposed: a `no-level` downgrade on every series when the sentence names no figure; the model can always restate the number.
  Ruled 2026-09-16: adopt, on every series (fix list 1.5).
- L2 — The duration detector is phrase-shaped and has no cadence exemption.
  `names_duration` matches "<count> consecutive <unit>" only when adjacent and "for <count> <unit>", so "for two consecutive distinct daily sessions" and "on 2 consecutive closes" pass while "for two consecutive sessions" downgrades, though all three state the market-data cadence exactly.
  Observed: SPMO repeat 2 and ARKF repeat 2 kept by phrasing; ARKF's $52 lost.
  Proposed: a duration equal to the series' `required_consecutive` in its cadence unit is representable and keeps; adjectives between count, "consecutive" and unit do not defeat the match.
  Ruled 2026-09-16: adopt, with L3 and L6, as fix list 1.6.
- L3 — "confirming …" reads an interpretive clause as a second condition.
  Ruling 1.4 named "confirmed by"; the landed lexicon also carries "confirming", which catches "break below $320 confirming broader multiple compression", where the clause states what the break would mean rather than a second test.
  Observed three times (TSLA, ARKF, DIA), each a sound price core.
  Proposed: drop "confirming" from the conjunction list, or restrict it to "confirming <a series or level>".
  Ruled 2026-09-16: adopt (fix list 1.6).
- L4 — The model bakes its buffer into the threshold.
  SPMO's third repeat stated $125, $133 and 3% and cored 128, 135 and 0.028; all three downgraded correctly and all three were sound falsifiers.
  Proposed: one contract sentence in `LedgerSeriesContract` — the threshold is exactly the level the sentence names, the margin is the separate band.
  Ruled 2026-09-16: adopt (fix list 1.7).
- L5 — The ruled margin bound admits a margin at 99.9% of the level.
  DIA's price below $490 kept a margin of 489.5, an effective band of 0.5 to 979.5, because the guard is "at or beyond the threshold's magnitude".
  Proposed: a relative cap per series kind on top of the magnitude bound (a cap, not the not-adopted 1.3 assignment).
  Ruled 2026-09-16: adopt, the fraction per series kind drafted at plan time (fix list 1.8).
- L6 — A parenthetical restating the level in another unit reads as a second required level.
  DIA's "1.2% (approx annualized >19%)" downgraded as multiple levels.
  Proposed: fold into L3's lexicon pass; a low-yield shape on its own.
  Ruled 2026-09-16: adopt (fix list 1.6).

### Re-think markers on the fixed set — run 2, thinking captured

A second pass the same day, one repeat, with the harness's new thought-log capture on, segmented by the attempt-6 method (`==== call` fences, `wait` / `actually` / `re-read` per call, each marker's ±150-character context bucketed by cause).
Twenty-four calls in 47 min.
The numbers are one interpretation call per holding and three action calls (the repeat plus the two variants, which send a byte-identical prompt), so the interpretation column is single-sample; per 8.4 the counts are a diagnostic beside the table, never the gate.

| Call class | Attempt 6 (v35), words / markers per holding | Run 2 (v36), words / markers per call |
|---|---|---|
| Interpretation | 2,057 / 12 — bimodal: TSLA 692 / 0, PSX 3,513 / 20, SPMO 844 / 0, ARKF 5,496 / 46, DIA 748 / 1, PGNY 1,048 / 4 | 1,811 / 7.5 — TSLA 2,414 / 9, PSX 1,973 / 7, SPMO 614 / 0, ARKF 1,091 / 2, DIA 2,350 / 12, PGNY 2,425 / 15 |
| Action | 2,465 / 15.3 — TSLA 2,172 / 17, PSX 2,600 / 17, SPMO 4,002 / 21, ARKF 2,287 / 17, DIA 1,477 / 8, PGNY 2,249 / 12 | 2,433 / 13.1 — TSLA 2,336 / 9.0, PSX 3,168 / 22.0, SPMO 2,350 / 14.7, ARKF 2,022 / 8.7, DIA 2,188 / 9.7, PGNY 2,531 / 14.3 |

Action, 235 markers over 18 calls: the ENGINE SET clause fell from 40 of attempt 6's 92 (43%) to 16 (7%), so 2.2 removed the churn it targeted.
What now leads is the packet's capital-efficiency line, 83 markers (35%): "indeterminate — this assessment carries no exit signal" (or "clears — …") reads to the model as a directive not to sell rather than as the fact that the hurdle test did not fire, and TSLA's first call re-reads it five times deciding between trim and hold.
The line predates this slice (it landed with the Finding-3 restructure) and attempt 6 counted 19 capital-efficiency markers across every class, so it did not change; it became the most ambiguous sentence left once the ENGINE SET churn and the tax block were gone.
Genuine rung deliberation is 61 (26%), the reasoning the call exists for; the rest is the sub-score polarity line (17), whether to fence the JSON (14), the one-sentence rule (14), target arithmetic (7) and unbucketed (22).
Interpretation, 45 markers over 6 calls: attempt 6's hotspot, the shape template with the `what_changed*` and `price_target_rationale` sentences (PSX 20, ARKF 46), is gone (PSX 7, ARKF 2); the residue is arithmetic against the engine targets and the spot (18), whether to fence the JSON (7) and sub-score polarity (5).
One trace matters for §3 rather than here: TSLA's interpretation derives the share count from the packet's market value over the spot, so the interpretation packet's account economics reach the model arm's reasoning (fix list 3.2, now observed rather than inferred).
Across all 24 calls no thinking carries a tax posture, a cost figure or a P/L; "if cost basis allows" appears once as the model's own phrase with no number behind it.

- L7 — The action packet's capital-efficiency line reads as a directive.
  "this assessment carries no exit signal" is read as "do not exit"; 83 of 235 action markers sit on it, the most of any cause after 2.2 removed the ENGINE SET churn.
  Proposed: state the fact and its reach — the hurdle test cleared or could not be evaluated, and the line neither requires nor forbids any rung.
  Ruled 2026-09-16: adopt (fix list 2.4).

### Verdict

Against fix list 8.2 as ruled: tax or P/L variation cannot move the rung, by input identity and on the live table, so **§2 is admitted**.
Executable cores agree with their sentences on every core the check could see, none of the seven attempt-6 defects recurred in a shape the check sees, and the level, margin and qualifier classes caught eight wrong or uncarriable cores live; but one wrong core was admitted through L1, so **§1 is admitted conditionally on L1 landing**, with a re-read of the fixed set after it.
Source dates and claim provenance are §4 and §5 work and were not exercised here.
L2 through L7 are yield and clarity, not correctness.
Ruled 2026-09-16: the verdict stands as written; L1–L7 land as one small follow-up slice ahead of §3 (fix list 1.5–1.8 and 2.4, `portfolio-v37`), after which the fixed-set re-read admits §1.
