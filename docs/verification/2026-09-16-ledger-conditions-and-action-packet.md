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
- The §8.2 admission read — the live calls on the fixed set, the accepted cores read beside their sentences, the rung across repeats and the tax and cost variants — is user-run with Ollama up and has not been run at implement time.

## Verification

Offline, at implement time:

```
cd src-tauri && cargo test          # 1478 passed, 0 failed, 32 ignored (the workspace's other targets green)
cd src-tauri && cargo clippy --all-targets --all-features   # 0 warnings, 0 errors (full output read)
npm run build                       # vue-tsc + vite green
```

The live gate, user-run:

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
