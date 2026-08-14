# Big confirmation run — attempt 2 — dated record

The second attempt at the single big confirmation run, executed 2026-08-13 on the dev store.
The run completed the full 47-holding per-holding pass cleanly and then failed at Step 7b construction on divergence-cause validation, unrepaired by the named-violation re-run.
The Step 7b repair engaged as designed: the pass persisted as a degraded run — `portfolio_runs` row `id = 2`, `run_id` `6a52f1dd`, `constructed = 0`, 47 verdicts, `outcome: null` — where attempt 1 persisted nothing.
This record carries the analysis of that outcome under the plan beside it (`2026-08-13-big-run-attempt-2-analysis-plan.md`): the construction root cause, the prompt-effectiveness read, the accuracy spot-check, and the watch-set reconciliation.
Fix options live in §Disposition; the three rulings were made the same day and the fix slice is built (see §Disposition's closing note).

## Run identity and terminal state

- Progress/execution run id `3f42e8e5` stamps the thought-log dir; the persisted row carries `run_id` `6a52f1dd`.
  The split is by design, not a seam: the thought-log sink keys off the `RunContext` id at attach time (`thought_log.rs`, with the wall-clock folder-name rationale on the `attach` doc comment), while `PortfolioRun.run_id` is freshly minted at the persist seam (`job.rs`).
  Correlating a thought-log dir with a persisted row therefore goes through the timestamp, not the id — acceptable for a human-browsed diagnostic.
- Terminal state: `failed — portfolio construction jointly infeasible after the named-violation re-run`, with the five final violations recorded verbatim in `job_runs` row 3.
- Thought-log capture worked at full scale: `construction.txt` plus 47 per-holding streams (the handoff's "38" undercounted; every holding has one, the shortest being GM at 1.7 KB — a stream that simply ended after the setup).

## What did not recur from attempt 1

- No output-budget exhaustion and no truncation: peak prompt was construction at 21,683 tokens against `num_ctx` 131072, completion 6,532 against `num_predict` 65536, `output_limited: false`, `context_pressure: []`.
- The per-holding pass ran 47/47 with zero 429s, zero retries, zero fetch errors; `deep_history_failures: 0` — every `company-eod-deep` (the Stooq-replacement rung) succeeded on the paid FMP key.
- `dgs10_history_gap: false`, `house_view_omitted: false`.

## Workstream 1 — the construction fail-hard, root-caused

### What actually happened (two rounds, not one)

The first draft carried **eleven** violations, not five.
The model supplied `overlap-emerged` on seven names — AMZN, DIS, GM, NFLX, NIO, NKE, SBUX — and omitted the cause on four — MA, V, RKT, TDOC.
All seven `overlap-emerged` claims failed checkability: the run's only overlap cluster is Technology (combined weight 68.5%), and none of the seven is in it.
The repair re-run fixed six of the eleven (MA and V validated add-side `cash-freed` claims — checkable-true, since the plan raises sell proceeds; NFLX, NIO, NKE, SBUX no longer violated).
The five that failed again are the terminal set: AMZN (`trim` off lean `hold`), DIS (`hold` off lean `add`), GM (`trim` off lean `hold`) with no cause, and RKT, TDOC with sell-side `cash-freed`.

### The root cause, in three layers

1. **The prompt orders a cause "from the vocabulary" but never states the vocabulary or its semantics.**
   The construction system prompt says "say why with a divergence_cause from the vocabulary" and lists nothing; the three causes reach the model only as the response grammar's enum.
   The thought-log shows the model noticing exactly this — "The prompt does not list a specific fixed vocab for `divergence_cause`" — and inventing its own vocabulary (`concentration_cap`, `sector_exposure`, `overlap_cluster`, `dead_money_status`), which the grammar then coerced onto the nearest real enum value.
   Its "sector overweight" intent became `overlap-emerged` (uncheckable off-cluster); its "sell to free cash / tax-harvest" intent became `cash-freed` (add-side-only under the validator, so structurally rejected on a sell).
2. **The vocabulary cannot express the divergences the prompt itself invites.**
   Every checkability condition is closed: `became-oversized` needs current weight ≥ 15% (`OVERSIZED_MIN_WEIGHT`), `overlap-emerged` needs overlap-cluster membership, `cash-freed` needs plan sells > 0 **and** an add-side move (action rung above the lean-else-prior-else-hold baseline).
   For a down-side divergence on a small, un-clustered position — trim a 2.1% AMZN off a hold lean, hold a 0.5% DIS off an add lean, exit dust positions RKT/TDOC — **no cause in the vocabulary can validate, ever**.
   Meanwhile the system prompt says the action choice is "UNRESTRICTED" and that "a dead-money loser is a legitimate source of redeployable cash" — actively inviting exactly the moves the validator then rejects as unattributable.
   GM is the sharpest case: its digest says "DEAD MONEY (hurdle fails)", the prompt blesses raising cash from it, the model trims it, and no cause exists for the trim.
3. **The repair names the violation but offers no legal exit.**
   After the repair round the model had correctly withdrawn the false `overlap-emerged` claims — and then had nothing left to say: null re-fails as `DivergenceMissing`, any other cause re-fails as `ContextCauseUnsupported`.
   The one legal exit — revert the action to the lean — is stated nowhere in the repair prompt.
   The five names sat in a catch-22 the single re-run could not resolve.

### Hypothesis adjudication

- H1 (prompt salience) — **confirmed**, and it is the trigger: the vocabulary and its checkability semantics are absent from the prompt.
- H2 (schema does not force it) — **rejected as a fix**: the model *did* emit causes in round 1; the grammar cannot express an action-dependent requirement (no conditional schema support on the local grammar path), and forcing non-null would convert honest omission into forced fabrication.
- H3 (`cash-freed` unsatisfiable under the fixed preset) — **refuted as stated, confirmed as a trap**: the cause is satisfiable (MA and V validated it add-side against the plan's sells), but its name reads naturally as the sell side, which never validates, and nothing tells the model the difference.
- H4 (vocabulary too narrow) — **confirmed**, the structural core: quality/funding-driven down-side divergences — the most common legitimate whole-book move in this run — are inexpressible.
- H5 (re-run feedback too thin) — **partially confirmed**: the violation text is precise, but the missing escape hatch makes the repair unable to succeed where no truthful cause exists.
- H6 (fail-hard too strict) — **a real ruling**, framed in §Disposition; the evidence here is that the fail-hard destroyed a book over five names whose divergence intents were legible and reasonable in the reasoning stream.

## Workstream 2 — prompt effectiveness (construction + 8 streams read)

Streams read: construction, AMZN (B), DIS (C), RKT (D, longest at 28 KB), TDOC (D, skimmed), SBUX (F), GM (shortest), QQQ (fund), ARKF (role-risk-only).
The contract mostly holds: the two-arm separation is visibly applied (models author their own sub-scores and targets, and temper engine extremes with stated reasons), the ledger series discipline is respected (macro triggers the closed series surface cannot express are correctly dropped to qualitative), and momentum-outside-the-letter is understood.
Weaknesses, each with its evidence:

- **The divergence-cause vocabulary gap** — the workstream-1 trigger; see above.
- **`NEW (not held last run)` conflates run-history debut with a fresh purchase.**
  Every stream read burns substantial reasoning (TDOC re-litigates it five separate times; AMZN roughly a quarter of its stream) on the contradiction between "NEW" and a legacy cost basis, because on a first clean run *every* holding is NEW.
  One prompt sentence — the position may long predate this analysis; NEW means no prior verdict exists — would eliminate the whole class.
- **Unlabeled units and types.**
  `return volatility: 0.0236` (daily vs annualized — AMZN and QQQ both stop to guess), `EXPENSE RATIO: 0.007` (fraction vs percent — ARKF flags it), and `conviction` with no stated type or scale (QQQ guesses a 0–1 float, RKT guesses a percent; the grammar coerces the result silently).
- **Risk sub-score polarity trips the model** — "risk 78 is high, implying low risk? Wait, 'higher better'" (DIS).
  Higher-is-better is stated, but a high score on a field named "risk" still misleads mid-reasoning.
- **Engine targets the model cannot believe consume its budget.**
  RKT's stream spends roughly half its 28 KB wrestling with engine targets 16× spot before sensibly authoring its own; the same class of hesitation appears wherever the engine band is extreme (see workstream 3).
- Cosmetic: models still deliberate JSON fences despite the "format is enforced by the decoder" clause (present and quoted in the streams), and SBUX's stream phrases a −35% base as "modest upside" while the emitted numbers are coherently bearish.

## Workstream 3 — accuracy spot-check

Coverage: all 47 verdicts swept mechanically (band ordering, action/lean consistency, grade re-derivation, outlook-vs-target direction, extreme-move flags); 22 deep-read across every grade bucket, both risk-tier and dead-money extremes, the five construction names, three funds plus the role-risk-only name, and the steepest targets.

### Internal coherence — clean

Every persisted letter re-derives exactly through the real weights (0.40 quality / 0.30 valuation / 0.30 risk, cutoffs 85/70/55/40).
All target bands are ordered; actions equal leans everywhere, as a degraded run must.
One cosmetic flaw: VOO's financial summary mashes the +59% unrealized gain and the 13.85% trailing return into one non-sequitur sentence.

### Model-vs-engine cross-check — clean

Across the 22 deep-read names, every fundamental the model cites matches the engine `metrics` row: no hallucinated P/E, margin, growth, or leverage figure was found (AMZN through TMUS, including TSLA's 351× P/E and SMCI's governance-history discount, all faithful).
The no-truncation context regime appears to be doing its job.

### The engine arm's scenario targets are the accuracy problem

Eight of 46 priced engine 12-month bases are externally implausible, in both directions:
RKT +1503% (base $241.09 on $15.04 spot), LCID +560%, CRM +116%, SMCI +95%, LUV +79%, NFLX +72%, and inverted GM −79% (base $18.10 on $86.39) and LEU −70%.
The mechanism decomposes cleanly from `quick_basis` and `target_meta`, as two trailing-anchored defects that bind at earnings troughs and valuation-regime breaks:

1. **The consensus clamp destroys valid consensus at troughs** (`clamp_flattened`).
   GM's consensus forward EPS mid of $14.35 is clamped to a $2.67 driver (trailing-EPS scale — trailing EPS ≈ $2.12 at the tariff-trough 1.03% net margin); RKT's $0.86 to $0.20.
   The clamp was built to bar absurd consensus; at a trough it bars the *recovery* consensus and manufactures a catastrophic base.
2. **Trailing-multiple percentile anchors explode near zero EPS and fossilize dead regimes.**
   RKT's raw P/E anchors are [420×, 1223×, 2511×] — trailing P/E observed while EPS ≈ 0 — and its clamped driver × P50 = the $241 base; the two trough distortions multiply.
   CRM anchors at P50 43× against a current 20.6× P/E (+116% base is a bet on the 2021-era multiple returning); NFLX at 37–47× against 23.8× today.
3. The propagation matters more than the numbers: `upside_downside` rides into the construction digest (RKT's "+1493%" line), and the dead-money hurdle rides `tr_base` — RKT and LCID read `clears` off fantasy targets, so those two capital-efficiency reads are vacuous.
4. The honest fallbacks behaved: TEAM and NIO carry `current_multiple_carry` and emit base ≈ spot (+0%) — vacuous as targets but correctly flagged; data-health counts 40/46 rate-anchored, 4 raw-percentile (SNDK, MRVL, ALAB, RKT), 2 multiple-carry, dispersion floor on 32.

On every extreme the model arm tempered with stated reasons (RKT +113%, LCID −36%, GM −44%): on this run's evidence the unrestricted model arm is systematically the more credible target source, which is exactly the calibration evidence the drafted-not-calibrated engine stand-in arm was waiting for.

### Distribution reads banked

- Grades B9 / C13 / D16 / F8, no A, plus one role-risk-only.
  The no-A is structural, not sectoral: quality and valuation sub-scores anticorrelate (ALAB Q100/V0, GM Q1.7/V83.5, PSX Q7.7/V98.2), so a ≥ 85 composite under 40/30/30 needs a near-perfect quality score *and* a cheap price at once.
  Sector-aware normalization would not by itself open the A band; that is evidence for the reserved ruling, not a decision.
- Risk tier High 28 / Medium 12 / Low 6; dead-money fails 14 / clears 9 / indeterminate 23.
  The fails cohort reached the model as weighed input, not instruction: 13 of 14 fails names kept hold leans (LEU trimmed) — the weighed-exit-input contract held.
- Engine quality scores are harsh on structurally thin-margin or buyback-shrunk-equity names (LMT Q13.6 at D/E 2.34 and P/B 15.7; PSX Q7.7; SBUX carries negative book equity) — a calibration observation for the same slice, not a defect.
- ARKF classified `ex-US equity fund` (role-risk) on a 67% US-coverage read against the ≥ 70% guard; the model itself flagged the label as odd for a US-listed, US-heavy fund.
  A one-holding near-miss of a drafted guard — evidence for the fund-constants file, not a bug.

## Watch-set reconciliation

Confirmed by this run: the entire per-holding half at 47-position scale — grade/risk/dead-money distributions with ordering intact, target provenance flags live end-to-end, conviction-action pairing and the fails→action read, debut ledger authorship, the fund path and the role-risk-only branch, 128 K runner stability with `num_ctx` honored and zero truncation, FMP quota and 429 behavior under the full dated-EOD load (zero 429s, ladder never needed), `^GSPC` sufficiency, thought-log capture at scale, degraded-run persistence with the `constructed` marker, and SBUX non-degeneracy (coherent steep-bearish, full targets-v3 methodology).
Unexercised because construction failed: the book itself, outcome learning and episode debuts, the two-arm retrospective and scoreboard, the paired-card render, the 7b sizing-movement rate, and every between-runs surface (quick check, selective re-analysis, carries, vintage/stale/demotion renders, debut-gap self-resolution).
Not measured by this analysis (evidence exists but was not tallied): TTM adoption and basis-flip rates, sector-P/E walk-back depth, pre-profit eligibility/financing distributions, overlay classification against real OCC rows, Schwab `averagePrice` multiplier.

## Disposition

The five construction failures were legible, reasonable judgments the contract could not hear.
The fix space, labelled and ranked; F1 is unconditional, R1–R3 are rulings surfaced before any implementation:

- **F1 (code-fix, unconditional).** State the vocabulary in both construction prompts: each cause's meaning *and* its checkability condition (became-oversized ≥ 15% weight; overlap-emerged = membership in a listed overlap cluster; cash-freed = an **add-side** move funded by the plan's own sells), generated from the same constants the validator reads so prompt and enforcement cannot drift — the Finding-2 pattern.
  Add the escape hatch sentence to both prompts: if no cause truthfully applies, return to the lean action.
- **R1 (ruling).** Demote `DivergenceMissing` from violation to annotation: an unattributed divergence persists as authored, stamped `unattributed divergence` on the row (the v7 annotate-don't-bar posture, extended from the engine-set departure to the lean departure).
  The alternative — keep the bar and force-revert unrepaired names to their leans — produces a book deterministically but overwrites the model arm's judgment with the engine arm's at exactly the names where they disagree, contaminating the two-arm record.
- **R2 (ruling).** After the single repair, demote any residual `ContextCauseUnsupported` to stripped-and-annotated (the uncheckable claim is removed, the row stamped `divergence cause rejected — recorded as unattributed`) instead of failing the run.
  Fabricated checkable claims still get caught and named once; what survives repair is recorded honestly; attribution failures then can never fail a book — arithmetic incoherence persisting through the repair remains construction's one failure mode, by design.
- **R3 (ruling, optional).** Extend the vocabulary with the sell-side twin the run actually needed (a `cash-raised` / funding-redeployment cause validating a down-side move when the plan deploys proceeds), and/or a dead-money-exit cause checkable against the row's own `fails` read.
  Cost: each added cause loosens the gate; under R1+R2 the extension is expressiveness, not a gate requirement.
- **Rejected: schema conditional-require (H2)** — see workstream 1; and **fail-hard retention with force-revert** — see R1.
- Expected outcome: F1 alone would likely have produced a book this run (MA/V repaired themselves once the add-side reading was available; the five needed only the escape hatch), but only R1+R2 close the attribution failure mode deterministically (coherence keeps its fail-hard).

**Ruled 2026-08-13, all three on the recommendation: R1 annotate-as-authored, R2 strip-and-annotate, R3 `cash-raised` only.**
The slice is built the same day: `PROMPT_VERSION` bumped to `portfolio-v8`, the vocabulary block and repair-exits sentence generated from `ContextCause::ALL`, the pass-aware validator (`ValidationPass`), the `cash-raised` checkability arm, the `unattributed divergence` / `(cause stripped)` episode stamps, and the annotation strings carrying `divergence_note`.
The contract's canonical home is updated in place (`docs/portfolio-analysis.md` §Portfolio roll-up and construction; the Step-7b pointer sentence follows it).
The coherence rail is untouched: persisting incoherence after the repair still fails the run, and the what-changed audit path keeps its violations — only the divergence-from-lean gate was ruled.

Separately queued by this record — **all resolved 2026-08-13, same session** (the queue below is kept as written; the closing block records each item's resolution):

- **The targets-v3 trough defect** (workstream 3) — a calibration slice against this run's persisted dataset: an EPS-materiality floor on multiple-anchor observations, a sanity bound on anchor multiples, and a consensus-clamp release when multi-row consensus disagrees with a trough-scale trailing EPS; the dead-money hurdle inherits whatever the targets decide.
  This is the first live evidence the drafted engine stand-in parameters were reserved against.
- **Interpretation-prompt tightening** (workstream 2): the NEW-position clarification, unit labels on volatility and expense ratio, the conviction type declaration, and a risk-polarity reminder.
- The ARKF ex-US guard near-miss and the engine quality-score harshness on negative-book issuers ride the existing fund-constants and grade-calibration reservations.

**Resolutions (2026-08-13, second wave):**

- **`targets-v4` built.** One relative mechanism subsumes the queued materiality floor and sanity bound: an anchor observation whose raw multiple exceeds the holding's current trailing multiple × a drafted 3× factor is dropped and counted (`anchor_bounded` on the target meta) — relative rather than absolute, so RKT's 420×–2,511× artifacts and LCID's departed bubble-regime P/S anchors die while TSLA's and ALAB's genuinely extreme-but-current regimes survive.
  The clamp release is signature-gated: corroborated consensus (≥ 2 forward rows) **and** the current trailing multiple above the bounded anchor window's own rich end (P75) — GM releases (the $14.35 consensus prices raw); LUV (whole-window trough) and CRM (downward re-rating) deliberately do not, since releasing there compounds the anchors' own distortion.
  `SCENARIO_TARGET_PARAMETER_VERSION` is `targets-v4`; run `6a52f1dd` is the v3 baseline; three shape-replay tests (the RKT artifact, the GM release with its one-row control, the CRM no-release) pin the behavior; the dead-money hurdle inherits the corrected targets mechanically.
- **Interpretation prompt tightened** (rides the same `portfolio-v8` bump): the NEW line now reads "no prior verdict in this run history — the position itself may long predate this analysis", return volatility is labeled DAILY, both expense-ratio renders carry the decimal-fraction example, conviction declares its exact three-value type, and both sub-score blocks state that a high risk score means resilience.
- **ARKF ruled: relabel, guard pinned.** The class label is now "equity fund below the US-exposure guard" — the measurement, not a nationality claim; the 70% guard held honestly and one near-miss is not calibration evidence.
- **Quality harshness ruled: stays reserved.** The letters' ordering was exact under re-derivation; quality scoring is entangled with the letter-distribution question, so it waits for the grade-calibration shadow-tune rather than a one-run piecemeal fix.
- **No-A ruled: honest — question closed.** An A means quality-at-a-discount, which this book's tape does not offer; the reserved sector-aware normalization slice is retired for the letter distribution (it may return only on realized-outcome evidence).
- **Left as views, not defects:** LEU −70% and TSLA −58% (both arms concur directionally), and the CRM/NFLX/LUV/SMCI-class reversion bets the sanity bound deliberately preserves — the two-arm design's model side tempered every one, which is the mechanism working.

Internal review (Metis task reviewer, same day): **approve-with-nits**, all seven criteria passing with no scope reduction.
Both nits applied: the shared-vocabulary side effect — `cash-raised` also validates the what-changed audit's moved-context claims on a redeployed down-side move, carve-out still excluded — is now stated at the vocabulary's canonical home, and the divergence path's double `ContextCause::parse` collapsed to one.

Codex round 1 (2026-08-14): **changes requested — five findings, all verified real and applied.**
1. The trough release's multiple signature alone could not distinguish an earnings trough from a price rally — the release gained the **direct trough test** (trailing print below a drafted fraction of the anchor window's largest admissible print), with a price-rally control test.
2. `periods_used = 2` counts blended rows, not rows carrying the selected field — `ConsensusEstimate` gained per-field **`eps_mid_rows` / `revenue_mid_rows`** stamped at the blend, the release reads the rung-matching count, and the release test's control now holds `periods_used = 2` throughout so a pass proves the gate reads the right field.
3. An off-band range could feed a funding cause's own aggregate (a "trim" sized above current weight is a dollar buy satisfying its own `cash-raised` check) — both funding causes now require the attributed row's **own dollar delta** to point the claimed direction, with a self-funding-inversion test.
4. The frontend contract lagged v8 — `cash-raised` joined the `ActionWhatChanged` cause union and the render's cause-label map, the annotations field's doc and the roll-up kicker were renamed to cover attribution records ("Construction notes"), and the component spec now renders both annotation kinds under the new label.
5. This record's own determinism wording contradicted the coherence rail — corrected to: attribution failures can never fail a book; persisting arithmetic incoherence remains construction's one failure mode, by design.

Codex round 2 (2026-08-14): four of five closed; **one boundary case, verified real and applied**.
On the fiscal-year boundary (`today == near_date`) the near row's blend weight is exactly 0 and the value is entirely the far row's, yet the presence-based count still read two rows — reopening the single-estimate corroboration hole on a reachable date.
`rows_carrying` now mirrors the blend's own contribution arms — a weightless near row does not count, while the used-alone fallback (far row lacking the field) still counts its one real contributor — with a `today == near_date` regression pinning both halves.

Codex round 3 (2026-08-14): runtime confirmed complete; one P3 wording residue applied — the canonical release sentence and two engine comments still said rows "carrying" the rung's mid (the rejected presence semantics) and now say **contributing to the blended mid**.
