# The residue slice — fix list 3.5–3.14 and 8.5 (2026-09-17)

## Scope and authority

The residue slice ruled 2026-09-16 off the v38 read (`2026-09-16-interpretation-slice.md` §Findings from the read: L10–L13 and the L15 A/B, fix list 3.5–3.9) and widened 2026-09-17 off the Codex churn analysis (the same record's §Codex churn analysis: fix list 3.10–3.14 and 8.5), one plan and one prompt stamp (`portfolio-v39`).
The evidence-set additions (3.15–3.16) and the sampling comparison (8.6) are not in it.
The plan's four decisions and four assumption groups were ruled by the user through the selector on 2026-09-17 before implementation.
Contract text belongs in the job docs named per entry; this record carries the rulings, the inventory and, once run, the read.

## Rulings — 2026-09-17

- **F1, 3.10 wording: prefix each capital-efficiency line in place.**
  "CAPITAL EFFICIENCY (the engine's twelve-month total-return test against a 7.7% hurdle): fails — even the bull case misses it, so the position is dead money. …", the ratified remainder of each state's sentence unchanged, the rate rendered from the hurdle read and "no hurdle rate this run" where none.
- **F2, the 3.9 Facts form: one line, the admission fact and the letter's bar.**
  New-money admission passes or fails (the base-case total return clears or misses the hurdle as a point test), and the letter bars the add family (F), supports add-aggressively (A or B), or does neither; the hurdle state, the overlay bars and a forensic trip already render in their own sections.
- **F3, the 3.9 run shape: one harness pass, both forms per holding.**
  `MARKET_SIGNAL_LOCAL_EVAL_ENGINE_SET=both` runs the List repeats then the Facts repeats on each holding; the tax, cost and fresh-interpretation calls stay on List.
- **F4, 3.11 placement: ACTION BASIS plus the two lines.**
  ACTION BASIS defines the grade beside the forward read; the low-confidence parenthetical and the CONVICTION line are reworded.
- **A1, stamps: the prompt stamp only.**
  `portfolio::PROMPT_VERSION` moves to `portfolio-v39`; `checkpoint-v10`, `evidence-floor-v5` and portability format 7 stand, since no persisted shape changes.
- **A2, the 8.5 values: set by inference.**
  `fmp-quarterly` on TSLA, PSX and PGNY, whose computed debt / equity came off an FMP quarterly balance sheet; null on the funds.
- **A3, mechanics: confirmed.**
  The period adjectives are weekly, monthly, quarterly, annual and yearly, in both clause forms; the interpretation line reuses the action packet's `SCORE_POLARITY` clause verbatim; the same fence sentence lands on both response contracts; the permission-phrase scan and the margin-ratio print are printed diagnostics, never gates.
- **A4, record: this file.**

## What landed

- 3.5 — `PERIOD_ADJECTIVES` sits beside `DURATION_UNITS`.
  `duration_clauses` reads a period adjective between the count and the unit as the clause's unit, in both clause forms ("two consecutive weekly closes", "for two weekly closes"), so `is_cadence` refuses it.
  Four qualifier cases and one kept daily-adjective case are pinned in the quantitative-core tests.
- 3.14 and 3.8 — `LedgerSeriesContract::render` states the band's purpose, then the caps, then the key-driver null rule.
  The caps read from `MARGIN_CAP_PRICE_RATIO` and `MARGIN_CAP_FRACTION`, so the prompt cannot drift from the seam, and they are stated against a nonzero level with the seam's zero-level exemption beside them.
  The ledger rewrite instruction says "carrying null where none does".
  The contract test pins all three sentences on the stock and fund contracts, the caps against the constants.
- 3.6 — the interpretation packet's ENGINE SUB-SCORES line takes `SCORE_POLARITY`; pinned.
- 3.7 and 3.13 — `response_shape_contract` and `action_response_contract` close with "with no code fence or surrounding prose"; pinned on every interpretation, role/risk and action contract shape.
- 3.10 — the four capital-efficiency lines open with the owner-horizon-rate prefix and keep their ratified remainders, "the hurdle over the assessed horizon" folded into "it" since the prefix now names both; the per-state test asserts the prefix, the rate and the absence of "over the assessed horizon" on every state.
- 3.11 — ACTION BASIS gains "The grade is the backward composite of the quality, valuation and risk sub-scores, and the targets are the forward scenario read; each is its own read."; the low-confidence parenthetical reads "a low-confidence letter: one sub-score is imputed — a property of the grade, not the conviction"; the line reads "CONVICTION (the verdict's own confidence)"; pinned with a low-confidence verdict on every hurdle state.
- 3.12 — "FINANCIAL SUMMARY (model-authored at interpretation):"; pinned.
- 3.9 — `EngineSetForm { List, Facts }` and `action_user_prompt_with_form`; the plain builder is the List form, and `engine_admission_facts_line` renders the Facts line.
  `action_request` takes the form.
  `LocalAnalyst::decide_action_under` is the one issue path: the trait's `decide_action` calls it under List, and the harness, holding a concrete `LocalAnalyst`, calls it directly for the Facts arm, so the production trait is unchanged (the plan had named a trait default method; the task review's nit was taken).
  The Facts stage reads "action SYM (engine-set facts)", so the thought-log fence and the usage row tell the arms apart.
  The harness reads `MARKET_SIGNAL_LOCAL_EVAL_ENGINE_SET`, labels every action line with its form, prints the permission-phrase scan (`PERMISSION_PHRASES`) under each rationale, and prints the margin-to-level ratio on every KEPT line.
  Offline pins: List is byte-identical to the plain prompt; Facts carries no ENGINE SET line and exactly one admission line, on the priced and the role/risk branch; the packet is identical outside that line; the no-rate capital-efficiency prefix renders; the six-fixture isolation test runs over both forms.
- 8.5 — `Fixture.equity_source` (required, no default): `fmp-quarterly` on the stocks and null on the funds, read into the continuity stamps and the financials; the interpretation-packet test pins the balance-sheet line against each fixture's computed debt / equity; the fixture README notes the field.
- Stamp: `portfolio::PROMPT_VERSION` → `portfolio-v39`; the constant's doc names the slice.
- Docs: `portfolio-analysis.md` §The position thesis ledger (3.5, 3.8, 3.14) and §Portfolio action (3.9, 3.10, 3.11, 3.12, 3.13); `portfolio-workflow.md` §Step 6f (3.6, 3.7, the contract pointer) and §Step 6g (3.5); the fix list's landed lines.

## Known limits

- The Facts form names the letter's bar as the engine's own rule, a rule term the packet had not carried; neither form is a contract change until the A/B is read and ruled (3.9).
- The fixed set is all-debut, so the 3.11 wording is read on debut calls only; a continuity packet renders the same lines.
- The 8.5 equity source is inferred, not read from the attempt-6 store (A2); the live dossier stamps the source wherever an equity line reached the engine (the SEC merge in `dossier.rs`) and derives debt / equity from that equity, so the harness-only split cannot arise live.
- "annually" rides with "annual" among the period adjectives; A3 named the five words, and the adverb names the same period.
- The permission-phrase scan is a lexicon: a rationale saying the profile "admits" the aggressive rung does not match it, and a permission claim in other words does not either.
  The strengthened 3.9 read of the thinking stays human.

## Codex implementation-review corrections

Read-only review after the task review; Codex changed no files and ran no gate, so the full gate ran here after the fix.
One finding, P2, verified against the code and fixed:

1. The new margin sentence stated the caps without the seam's zero-threshold exemption: both margin checks run only on a nonzero threshold, and the regression case "Net margin falls below 0%" with a 0.02 margin keeps its core, so the prompt had made a valid condition impossible to author consistently.
   Fix: the contract states the caps against a nonzero level and adds that a zero level has no cap, since the margin is then the condition's only scale; the older rewrite instruction says "a nonzero level's magnitude"; the harness prints "no ratio: zero level" instead of 0% on a zero threshold, so the anchoring read is not distorted; the contract pin covers the sentence and the exemption; the ledger doc names the exception.
   Codex's second round found the same sentence's earlier sizing clause ("well inside the level's magnitude") still unconditional; it now asks for a positive noise band well inside a nonzero level's magnitude, with the zero-level exemption stated in the same parenthesis.
   Codex's third round found the response template's field note ("a fraction of the level") still unqualified, which the second-round claim that the three margin sentences agreed had missed; the note now reads "a fraction of a nonzero level (a zero level has no cap)", pinned on every contract shape.
   Codex otherwise confirmed the scope: production keeps the List form, the A/B shares the action-call path, period adjectives defeat the cadence exemption, fixture equity sources reach the prompts, and the stamp bars a v38 resume.

## Verification

Offline, at implement time: `cargo test` 1483 passed, 0 failed, 32 ignored (the other targets green); `cargo clippy --all-targets --all-features` 0 warnings, 0 errors (full output read); `npm run build` green; `npm test` 46 and 266 passed.

```
cd src-tauri && cargo test
cd src-tauri && cargo clippy --all-targets --all-features
npm run build
npm test
```

The live read, after the task review and Codex, only when the user says, with Ollama brought up per the runbook first:

```
cd src-tauri && MARKET_SIGNAL_LOCAL_EVAL_REPEATS=3 MARKET_SIGNAL_LOCAL_EVAL_ENGINE_SET=both MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR=<dir> cargo test fixed_evidence_live -- --ignored --nocapture
```

It reads the following.
A weekly-close condition downgrades as `qualifier` while a daily one keeps (3.5).
The interpretation re-think's polarity, fence and driver-mapping causes fall (3.6–3.8).
The rung table and the marker share are read under both set forms, and every action call is read for invented permission claims, prose-ownership errors and rationale correctness (3.9, as strengthened 2026-09-17).
No action call guesses the assessed horizon, no rationale calls a low-confidence grade "low conviction", and no `indeterminate` rationale cites the line as its reason (3.10–3.11).
The action fence deliberation falls (3.13).
`margin-implausible` downgrades fall, and the kept cores' margin-to-level ratios do not drift toward the cap (3.14).
PGNY's packet names its equity source (8.5).

## Live read — 2026-09-17

Run on commit `a0e2055` after the task review and the Codex rounds, three repeats with thinking captured, both engine-set forms, Ollama v0.32.5 (one slot, the 128K context override, flash attention), `qwen3.5:122b-a10b`.
The user ended the run at sixty-four of seventy-two calls — eighteen interpretation, thirty-two List-form action, fifteen Facts-form action, no transport failure and no retry (interpretation 137–245 s, action 73–199 s) — with PGNY's interpretation complete and four of its nine action calls in.
The harness log, thought logs and rendered prompts are `~/Downloads/market-signal-fixed-evidence-2026-09-16/run5-v39-read-3repeats-both-thinking.log`, `thought-logs-v39/` and `prompts-v39.md`; the per-condition read below is made from the log.
The harness reads its daemon roster from the environment, not the store: `MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT`, `MARKET_SIGNAL_LOCAL_REASONER_MODEL` and `MARKET_SIGNAL_LOCAL_EMBEDDER_MODEL` must be set beside the eval variables (the runbook's line omitted them).

### Cores beside their sentences

| Holding | Kept cores across the three repeats (v38) | Wrong cores | Other downgrades |
|---|---|---|---|
| TSLA | 5 (2 / 1 / 2) (v38 4) | — | `qualifier` ×4; `margin-implausible` ×2 (480 on 620, 0.186 on 0.29); `comparator-mismatch` |
| PSX | 6 (3 / 0 / 3) (v38 2) | — | `no-level` ×2 ("deep intrinsic value estimate", "mean reversion support zone"); `unit-mismatch` (2.5 on a fraction series) |
| SPMO | 5 (2 / 1 / 2) (v38 5) | 1 kept: `trailing-return above -0.1` on "inflects below -10% over rolling 6-month window" (L16) | `comparator-mismatch` ×2; `qualifier` ×2 |
| ARKF | 4 (0 / 0 / 4) (v38 4) | — | `price-level-mismatch` ×4 (the statement and the core one margin apart: "$40 ($38 + margin buffer)" on a core of 39) |
| DIA | 4 (1 / 1 / 2) (v38 5) | — | `price-level-mismatch` ×4 (490 on 480 and 495; 570 on 560; 600 on 38); `level-mismatch`; `qualifier` |
| PGNY | 3 (1 / 0 / 2) (v38 2) | — | `margin-implausible` ×3; `level-mismatch` ×2; `qualifier` ×3; `no-level`; `unit-mismatch` |

Twenty-seven kept cores over eighteen calls against twenty-two on the v38 read, twenty-six matching a figure their sentence states and one wrong core admitted — the first since the v37 re-read.
It came through a pre-existing seam gap, not a v39 change: "over" in a window phrase counts as an above-word, the direction reads as mixed, and the comparator check is skipped (fix list 1.10); PSX's "below 4% over two quarters" rode the same skip and is also a duration the parser does not read (fix list 1.11).
The buffered-threshold habit the v38 record had declared absent is back: eight `price-level-mismatch` downgrades against one, the seam reasons showing the statement carrying the confirmation price and the core the level, or the reverse, and ARKF's thinking sizing a margin to the shown cap ("margin of 0.005 is ~23%, acceptable per text 'at most 25%'", the price cap misread as the fraction cap).
The kept cores' margin-to-level ratios drifted up on the non-price series — mean 9.1% against 6.1%, median 5% against 4%, max 40% against 29%, three non-price cores at 20% or more against two — with the price series unchanged.
Entry 1.9's yield rose (PSX six, PGNY three) with three `no-level` downgrades returning; no weekly-close core was kept (3.5).
Every target rationale named one of the model's own figures (eighteen of eighteen), no prose field carried account economics, every debut carried the app-written continuity line, and the funds authored no stock-only series.

### The rung under repeats and variants

| Holding | Engine set | List repeats 1–3 | Facts repeats 1–3 | Tax-exempt | Cost basis ×3 | Fresh-interpretation verdict |
|---|---|---|---|---|---|---|
| TSLA | sell-all, trim, hold | trim, sell-all, sell-all | trim, sell-all, trim | sell-all | trim | sell-all |
| PSX | sell-all, trim, hold, add | trim ×3 | add, add, hold | trim | trim | hold |
| SPMO | sell-all, trim, hold | sell-all ×3 | trim, trim, sell-all | sell-all | trim | sell-all |
| ARKF | sell-all, trim, hold | trim, sell-all, sell-all | trim, trim, sell-all | trim | sell-all | sell-all |
| DIA | sell-all, trim, hold | hold ×3 | hold ×3 | trim | hold | hold |
| PGNY | sell-all, trim, hold, add | hold (one of three) | not run | not run | not run | not run |

No rung outside its engine set (0 of 47).
TSLA, which held trim on all six v38 calls, moved between trim and sell-all on eight of nine; the v38 record's knife-edge holdings (SPMO, ARKF) moved as before.
DIA's tax-exempt variant went trim against hold on its other five calls: the rendered dump shows the tax-exempt and cost-basis packets differ from the List packet by no line, so the move is sampling, not tax.
The Facts form (3.9) steered harder than the list on both admission outcomes: PSX's passing admission became a buy signal ("Engine says Buy/Add", "a crucial override mechanism"; engine-set markers 1.0 per List call, 7.7 per Facts call) and DIA's failing admission a prohibition ("This kills add", "explicitly prohibits increasing exposure"; the permission scan hit "prohibit"); on the two failing holdings it leaned trim where the list chose sell-all.
Rationale correctness under the 3.10 prefix: the four failing-holding rationales that cite the hurdle quote the rate and the state correctly; on the indeterminate holdings one TSLA rationale asserts the position "fails capital efficiency tests" from a point test the model ran itself on the target lines, and four rationales (TSLA's tax-exempt, three of DIA's) cite "indeterminate capital efficiency" as their reason, which 3.11 asked to be absent.
No rationale called a low-confidence grade "low conviction".

### Re-think markers

| Call class | v38 read | v39 read |
|---|---|---|
| Interpretation, words / markers per call | 1,996 / 7.1 | 2,210 / 10.2 |
| Interpretation, fence markers (all calls) | 12 | 3 |
| Interpretation, polarity markers (all calls) | 11 | 8 |
| Action, List form, words / markers per call | 2,413 / 13.0 | 2,568 / 13.9 |
| Action, Facts form, words / markers per call | — | 2,759 / 13.7 |
| Action, ENGINE SET share of markers (record basis) | 30% | 32% |
| Action, fence markers (all calls) | 17 | 42 |

Interpretation churn rose on every holding (TSLA 9.0 to 14.0, PSX 8.3 to 11.0, SPMO 5.3 to 5.7, ARKF 5.3 to 10.0, DIA 8.7 to 11.0, PGNY 5.7 to 11.0).
The fence deliberation 3.7 targeted fell; what replaced it is deliberation about targets, prices and margin sizing, the last invited by the 3.14 sentence.
Per fix list 8.4 the counts are a diagnostic beside the tables, never the gate.

### Findings from the read

- L16 — A window "over" masks the statement's direction (fix list 1.10, ruled 2026-09-17: a separate validator slice).
- L17 — "over N units" is not read as a duration (fix list 1.11, the same slice).
- L18 — The shown margin caps and the band's purpose sentence brought back the buffered-threshold habit and cap-anchoring (3.14 not met; superseded by `portfolio-v40`, which shows no caps and sizes the margin by example).
- L19 — The hurdle prefix's rate and test were re-run by the model on the indeterminate holdings, and "indeterminate" was cited as a reason (3.10 read as a regression there, 3.11's second check not met); the action-prompt rewrite carries it.
- L20 — The Facts form steers harder than the list on both admission outcomes; not adopted, the List form stays production, and the 3.9 ruling on the line itself is open.
- L21 — Every explanatory clause the v35–v39 slices added became an input the model computed with; interpretation churn rose on every holding.
  Ruled 2026-09-17: the prompts stop explaining the app — data, data glosses where a value needs one, the task, the return structure — beginning with the interpretation prompt (`2026-09-17-interpretation-prompt-rewrite.md`, fix list 3.17).
- Watches, no entry: PGNY's noise bands (three `margin-implausible` on three calls); the ARKF qualitative condition naming the issuer's own losses is a condition, not a prose field, and the economics scan did not flag it.

### Verdict

Against the record's §Verification list: 3.5 met; 3.6, 3.8 no change read; 3.7 met on the interpretation call; 3.9 read, the Facts form not adopted; 3.10 and 3.11 not met on the indeterminate holdings; 3.13 not met; 3.14 not met and superseded; 8.5 met.
One wrong core was admitted through a seam gap older than the slice.
The residue slice's wording entries are superseded by the `portfolio-v40` rewrite rather than re-tuned, and the v40 read is the next gate.
