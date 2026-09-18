# Local model calls — fix list from big-run attempt 6 (2026-09-15)

The single-homed work list distilled from the attempt-6 findings record
(`2026-09-15-big-run-attempt-6-findings.md`: the Claude read in its Findings 1–9 and
the Codex analysis in its §1–§10).
Each entry is a decision, the finding it rests on, the acceptance check, and the
stamp it moves; the evidence stays in the record and is not repeated here.
Every entry needs a ruling before implementation starts (`Ruled <date>:` lines);
"open" marks the ones still waiting.
Ruled 2026-09-15: scope and direction are ruled here; every other open entry is ruled
as a flag of the slice's `/metis-plan-task` plan, with the code in view, not before.
Ruled 2026-09-15: the first slice is §1 (ledger conditions) plus §2 (action packet),
one plan and one `portfolio-v36` stamp; §3–§7 follow as their own slices.
Ruled 2026-09-16: the live read's follow-up (1.5–1.8, 2.4) is its own small slice ahead
of §3, under `portfolio-v37`.
Contract text belongs in the job docs named per entry, never here.

Standing constraints that apply to every entry:

- Pre-release, no data compatibility work: a persisted-shape change moves its stamp and
  the next attempt re-wipes; where the Codex analysis asks for a "compatibility
  review", read that as "move the stamp" (user principle, 2026-08-17 / 2026-08-29).
- `portfolio-v35` has now run (six persisted holdings), so any prompt or schema change
  here lands as `portfolio-v36`; the checkpoint and evidence-floor stamps move only where
  an entry says its persisted shape changes — confirm each at plan time.
- Nothing is adopted on one holding's impression; each entry names the check that
  admits it, run on the fixed evidence set (entry 8.1) before any book-scale attempt.
- Checks are behavioral where the finding was: a source-evidence input and an expected
  persisted outcome; re-think marker counts stay diagnostic (8.4).
- Every slice keeps the existing guards under test: the gathering-tools / synthesis-
  grammar separation, blank-findings rejection, and citation admission.
- A field whose meaning changes still updates its producer, its consumers and its
  single-home doc section in the same slice; that is doc discipline, not compat.

## 1. Ledger conditions — the executable core must mean what the sentence says

Rests on: record Finding 2; Codex §2.
Docs that change: `portfolio-analysis.md §The position thesis ledger`,
`portfolio-workflow.md §Step 6g`.

- 1.1 Show the model a ledger-authoring contract scoped to the holding: only the series
  the engine computes for this vehicle kind, each with its unit, statement basis,
  current observation and supported crossing cadence, plus one worked quantitative
  example and one genuinely qualitative example.
  Check: a fund never sees a stock-only series; a stock's current observation appears
  beside each series it may threshold.
  Ruled 2026-09-16: adopt, on both branches — the contract renders the computable series
  with unit, basis, current observation and confirmation cadence plus two worked
  examples; landed as `portfolio-v36`.
  Live 2026-09-16: the model mirrors the contract's cadence language and units on every
  kept core; it still buffers thresholds away from the stated level (finding L4 in
  `2026-09-16-ledger-conditions-and-action-packet.md` §Live admission read).
- 1.2 Validate prose-versus-core agreement at 6g and downgrade on disagreement with a
  typed reason (downgrade-not-drop, the existing contract): a statement naming N% on a
  decimal series against a core of N; a statement naming one metric or basis against a
  core on another (TSLA "operating margin" on `net-margin`, PSX "quarterly" on the TTM
  basis); a qualitative statement bound to `price`; a margin that moves the effective
  boundary outside the series' plausible range (zero and negative thresholds are valid
  on their own).
  Never silently repair or clamp a threshold toward the engine's view.
  Check: the seven attempt-6 defects (TSLA ×3, PGNY ×2, SPMO ×2) each downgrade with the
  reason named; DIA's cores pass 1.2 unchanged; ARKF's pass 1.2 and are then handled by
  1.4.
  Ruled 2026-09-16: adopt the narrow prose-mismatch form, basis leg included, plus a
  comparator check and a level check that reads the comparison clause (ambiguous
  associations stay qualitative) and the margin guard at or beyond the threshold's
  magnitude (a zero threshold exempt); landed as `portfolio-v36`.
  Live 2026-09-16: sixteen of nineteen kept cores match a stated figure; the level and
  margin classes caught eight wrong cores; one wrong core (a 300% growth floor) passed
  on a sentence naming no figure, and a margin of 489.5 on a $490 level passed the
  magnitude bound — findings L1 and L5 in `2026-09-16-ledger-conditions-and-action-packet.md` §Live admission read.
- 1.3 Assign the noise margin from a per-series app policy instead of asking the model
  to author it; the model authors series, comparator and threshold only.
  This changes the persisted `QuantCore` authoring path but not its shape.
  Check: no persisted margin exceeds its series' policy value; the prompt no longer
  shows a `margin` placeholder.
  Ruled 2026-09-16: not adopted — the margin stays model-authored; 1.2's margin guard
  covers the implausible case.
- 1.4 Conditions that need a qualifier the machine predicate cannot carry (duration,
  volume, catalyst, a different accounting basis) stay qualitative; a later slice may
  add an extended representation.
  Check: ARKF's "above $55 for two weeks" and "below $38 on elevated volume" persist
  qualitative with the reason.
  Ruled 2026-09-16: adopt for duration, volume and second-condition clauses ("without",
  "unless", "confirmed by"); a generic "on <event>" clause is not detected; landed as
  `portfolio-v36`.
  Live 2026-09-16: every duration, volume and "without" clause downgraded as ruled; five
  sound cores were lost to the lexicon's reach ("confirming …" ×3, a cadence-exact
  duration, a unit-restating parenthetical) — findings L2, L3 and L6 in `2026-09-16-ledger-conditions-and-action-packet.md` §Live admission read.
- 1.5 A sentence naming no figure downgrades on every series (`no-level`), extending
  1.2's level check from the price alias to a figure; the model can always restate the
  number.
  Check: PGNY's "revenue growth slows materially below sustainable organic run rate"
  with a core of 3 downgrades; a sentence stating "3%" with a core of 0.03 keeps.
  Raised by the live read (L1).
  Ruled 2026-09-16: adopt, on every series; landed as `portfolio-v37`.
- 1.6 The duration detector exempts a duration equal to the series' confirmation count
  in its cadence unit and tolerates adjectives between count, "consecutive" and unit;
  "confirming …" leaves the conjunction list or narrows to a named series or level;
  a parenthetical restating the level in another unit is not a second level.
  Check: ARKF's "for two consecutive sessions" on price keeps; "three consecutive
  distinct daily sessions" downgrades; TSLA's "$320 confirming broader multiple
  compression" keeps; DIA's "1.2% (approx annualized >19%)" keeps.
  Raised by the live read (L2, L3, L6).
  Ruled 2026-09-16: adopt all three; landed as `portfolio-v37`.
- 1.7 The ledger contract states that the threshold is exactly the level the sentence
  names and the margin is the separate band.
  Check: on the fixed set the level-mismatch rate on repeats falls from SPMO's three
  of three.
  Raised by the live read (L4).
  Ruled 2026-09-16: adopt; landed as `portfolio-v37`.
- 1.9 A quantitative condition's statement names its level in the series' unit, stated
  in the authoring contract, so `no-level` stops costing executable cores where the
  model writes levelless prose.
  Check: PSX and PGNY recover from one executable core over three repeats toward run 1's
  four on the fixed set.
  Raised by the v37 re-read (L9).
  Ruled 2026-09-16: adopt; lands with the §3 slice, not its own.
  Landed 2026-09-16 as `portfolio-v38`: the contract's sentence names the unit per series kind (`2026-09-16-interpretation-slice.md`).
  Live 2026-09-16: no levelless core authored on eighteen calls (the v37 re-read caught six); PSX and PGNY each recovered from one core to two, not four — the remaining conditions spend themselves on durations and, on PGNY, on over-wide margins (§Live admission read in `2026-09-16-interpretation-slice.md`).
  Live 2026-09-17: PSX recovered to six cores over three calls (v38 two) and PGNY to three (v38 two), with three `no-level` downgrades returning on PSX and PGNY (v38 none).
- 1.8 A relative margin cap per series kind on top of the magnitude bound.
  Check: DIA's margin of 489.5 on a $490 level downgrades; a $15 margin on $485 keeps.
  Raised by the live read (L5).
  Ruled 2026-09-16: adopt, the fraction per series kind drafted at plan time (25% on the price and ratios, 50% on fractions); landed as `portfolio-v37`.

- 1.10 A window preposition masks a statement's direction: "over" in "over a rolling
  6-month window" or "over two quarters" counts as an above-word, the direction reads as
  mixed, and the comparator check is skipped.
  Check: SPMO's "Trailing price return inflects below -10% over rolling 6-month window"
  on a core of `above -0.1` downgrades as `comparator-mismatch` instead of keeping — the
  one wrong core admitted on the v39 read, the first since the v37 re-read.
  Raised by the v39 read (§Live read in `2026-09-17-residue-slice.md`).
  Ruled 2026-09-17: a separate small validator slice, not the prompt rewrite.
- 1.11 "over N units" is not read as a duration: the parser accepts "for N units" and "N
  consecutive units" only, so "Net margin sustains below 4% over two quarters" keeps a
  core that confirms on the first breaching print.
  Check: the PSX sentence downgrades as `qualifier`; "across two quarters" and "through
  two quarters" likewise.
  Raised by the v39 read.
  Ruled 2026-09-17: the same validator slice as 1.10.
## 2. Action call — an investment-only packet

Rests on: record Finding 3; Codex §4.
Docs that change: `portfolio-analysis.md §Portfolio action`.

- 2.1 Withhold the tax-sensitivity flag, unrealized P/L and purchase economics from the
  action packet; the app renders the optional tax caveat onto the rationale after the
  rung is fixed, per the standing caveat-only tax ruling.
  Check: the complete serialized action packet, embedded summaries included, carries no
  tax flag, P/L or cost figure; varying tax status and entry cost with the verdict
  held fixed cannot change the rung over repeated runs.
  Ruled 2026-09-15: adopt — the tax caveat is app-rendered after the rung; the
  caveat-only tax ruling is enforced by input separation, not by sentence.
  Live 2026-09-16: no rationale in thirty carried tax, P/L, cost or quantity language;
  the variants never left their repeat set except one DIA call on a byte-identical
  prompt — the check holds by construction (§The rung under repeats and variants in
  `2026-09-16-ledger-conditions-and-action-packet.md` §Live admission read).
- 2.2 Restate the ENGINE SET once, declaratively, as engine evidence: the full ladder is
  the model's, the set is one input, choose and stop; drop the departure-mechanics
  sentence (stamping is app behavior).
  Check: on the fixed evidence set the rung is stable across repeats and never lands
  outside the ladder; the trace's reading of the set is a diagnostic only.
  Ruled 2026-09-16: adopt with the ratified wording; landed as `portfolio-v36`.
  Live 2026-09-16: no rung outside the set in thirty calls; three holdings held one rung
  across five calls and three moved between adjacent rungs on identical packets, the
  model's sampling variance.
  Thinking 2026-09-16: the ENGINE SET clause fell from 43% of the action call's re-think
  markers to 7% on the fixed set; the capital-efficiency line replaced it at 35% (2.4).
  Superseded 2026-09-17 by 2.6: the set is one data line with no permission sentence under `portfolio-v41`.
- 2.3 State every sub-score's polarity in the action prompt's verdict block ("0–100,
  higher better on every axis"), as the interpretation prompt already does.
  Check: on PSX and SPMO the rationale's reading of the valuation score matches the
  score's meaning (high = attractive), where attempt 6's traces read it both ways.
  Ruled 2026-09-16: adopt with the ratified wording on both arms; landed as
  `portfolio-v36`.
  Live 2026-09-16: PSX's momentum and DIA's valuation readings carry the stated
  polarity in every rationale that names them.
  Carried into `portfolio-v41` as the SCORES section's one gloss (2.6).
- 2.4 The packet's capital-efficiency line states the fact and its reach — what the
  three-state read found (even the bear case clears; the bear case misses and the bull
  case clears; no assessment available), and that the line neither requires nor forbids
  any rung — instead of "carries no exit signal", which the model reads as a directive.
  Check: on the fixed set with thinking captured, the capital-efficiency cause falls
  from 83 of 235 action markers toward the attempt-6 share (19 across every class).
  Raised by the live thinking read (L7 in `2026-09-16-ledger-conditions-and-action-packet.md`).
  Ruled 2026-09-16: adopt; landed as `portfolio-v37`.
  Re-read 2026-09-16: the per-call capital-efficiency markers fell on every holding whose
  line changed (TSLA 4.3 → 3.4, PSX 1.3 → 0.6, DIA 5.3 → 1.6, PGNY 6.7 → 2.4).
  Superseded 2026-09-17 by 2.6: the capital-efficiency section is numbers alone under `portfolio-v41`.
- 2.5 The `fails` capital-efficiency line states the fact and its reach as the other
  three states do — the bull case misses the hurdle; a weighed exit input, not a rung —
  instead of "an exit input when forward prospects are independently poor", which the
  model reads as a mandate to sell everything.
  Check: on the fixed set with thinking captured, SPMO's and ARKF's capital-efficiency
  markers (38 and 48 over five action calls) fall toward the reworded holdings' share.
  Raised by the v37 re-read (L8).
  Ruled 2026-09-16: adopt; lands with the §3 slice, not its own.
  Ruled 2026-09-16 (F5): the ratified wording states the fact, the sunk-cost lean it feeds beside the forward read, and the same closing reach as the other three states; landed as `portfolio-v38`.
  Live 2026-09-16: SPMO's and ARKF's capital-efficiency markers fell to 4.5 and 3.2 per action call (from 7.0 and 6.8), inside the reworded holdings' 1.5–3.7 band; every `fails` rationale reads the line as a fact — check met.
  Superseded 2026-09-17 by 2.6: the capital-efficiency section is numbers alone under `portfolio-v41`.
- 2.6 The action prompt names no app concept, on the 3.17 principle: one message in two parts
  — inputs, each section explained once and then its values, labelled computed or analyst;
  then the task in output order — the system prompt the role line and the output names, the
  capital-efficiency read as the three tested returns and the hurdle rate with no state word,
  the set as one data line with no permission sentence, the analyst's target rationale, thesis
  and scenario rows added, the computed bands' method clauses in place of the provenance label,
  the overlay's and the forensic sweep's consequence lines off the packet, the weighing order,
  the profile tie-break and the sunk-cost rule as task clauses, the firmness clause with a
  chosen prior only, and a placeholder-only return shape.
  Check: on the fixed set with thinking captured, no rationale cites a hurdle state word or
  computes the hurdle test from a prefix, no rationale argues from what the computed read
  permits, the rung table stays inside the fixed set's repeat behaviour, and the action fence
  markers fall from forty-two on forty-seven calls.
  Raised by the user's reading of the rendered v39 and v40 prompts and the v39 read's L19 and
  L20 (2026-09-17).
  Ruled 2026-09-17: adopt; the nine prompt rulings and the eight plan rulings are recorded in
  `2026-09-17-action-prompt-rewrite.md` §Rulings.
  Landed 2026-09-17 as `portfolio-v41` (`2026-09-17-action-prompt-rewrite.md`).

## 3. Interpretation call — one responsibility per field, no account economics

Rests on: record Finding 8; Codex §5.
Docs that change: `portfolio-workflow.md §Step 6f`, `portfolio-analysis.md §The holding
verdict` (the model-arm field list), `§Portfolio action`, `§What changed`, `§Holdings
change tracking` and `§The position thesis ledger`.
Ruled 2026-09-16: the slice's twelve plan flags and Codex-review points are recorded in
`2026-09-16-interpretation-slice.md` §Rulings; 3.1–3.3 landed as `portfolio-v38`, and 3.5–3.14 as `portfolio-v39` (`2026-09-17-residue-slice.md`).

- 3.1 Resolve `price_target_rationale` ownership: either the app renders the engine's
  methodology and the model owns a clearly named explanation of its own targets, or the
  field keeps its engine meaning and the prompt enforces it.
  A rename or meaning change moves `checkpoint-v9`.
  Check: TSLA, PSX and PGNY's persisted rationales explain the field's stated subject.
  Ruled 2026-09-16 (F1): the first form — the field is `model_target_rationale`, the
  model explaining its own bands against the engine's; the card moves it to the model
  column; `checkpoint-v9` → `checkpoint-v10` and portability format 6 → 7 (A1).
  Live 2026-09-16: eighteen of eighteen rationales explain the model's own bands and their departure from the engine's base, each naming an own figure — check met.
- 3.2 Remove purchase cost and position P/L from the intrinsic interpretation packet;
  spot stays.
  Check: no persisted financial summary carries a per-share cost figure.
  Ruled 2026-09-16 (F2, F3, the Codex review's point 2): one identity-and-spot header
  serves every model-facing packet, the research brief included; the position line
  states its direction only; the option overlay renders unsized on every packet.
  Live 2026-09-16: no account-economics phrase in seventy-two prose fields (one issuer-losses false positive on ARKF); every tax, cost and fresh-interpretation variant landed inside its holding's repeat set — check met.
- 3.3 Give funds a fund-specific ledger schema whose series enum is the fund-computable
  set, and let the app insert the debut continuity fields (`what_changed_entries = []`)
  deterministically instead of asking the model.
  Check: a fund interpretation never authors a `pe-ratio` core; a debut never spends
  markers on `what_changed` versus `what_changed_entries`.
  Ruled 2026-09-16 (3.3a, F4, F6, A2): the series enum is scoped to the vehicle kind on
  both branches; a debut requests neither continuity field and the app writes "New
  holding (no prior verdict)." with an empty row set; the continuity contract states the
  two fields' relation once; the template gains notes for the ledger's numeric fields;
  the continuity shape is proven by offline tests and live continuity stays unverified.
  Live 2026-09-16: the three fund holdings authored no stock-only series over nine calls; `what_changed` is mentioned zero times in fifty-four calls' thinking (ninety-three on the v37 re-read) — check met.
- 3.4 Candidate, not adopted: a separate compact ledger-authoring call on the same
  evidence packet and accepted read.
  Test only if 3.1–3.3 leave interpretation slow or unreliable (Codex §9 step 6).
  Ruling: deferred by construction; re-tested only if the v38 fixed-set read leaves
  interpretation slow or unreliable.
  Live 2026-09-16: interpretation ran 74–181 s per call at 7.1 markers, no failures — not triggered.
- 3.5 A weekly, monthly or quarterly adjective on the duration unit defeats the cadence
  exemption ("two consecutive weekly closes" is not the daily cadence).
  Check: TSLA's "$300 for two consecutive weekly closes" downgrades as `qualifier`;
  "two consecutive daily closes" keeps.
  Raised by the v38 read (L10).
  Ruled 2026-09-16: adopt; lands in the residue slice (`portfolio-v39`).
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17: no weekly-close core kept on eighteen v39 calls (§Live read in `2026-09-17-residue-slice.md`).
- 3.6 The interpretation prompt's sub-score line glosses every axis, as the action
  packet's polarity clause does (valuation = more attractive, risk = more resilient).
  Check: the interpretation re-think's polarity re-checks fall on the fixed set.
  Raised by the v38 read (L11).
  Ruled 2026-09-16: adopt; residue slice.
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17: polarity re-checks did not fall (v38 eleven markers, v39 eight on eighteen calls) and interpretation churn rose on every holding, 7.1 to 10.2 markers per call.
  Superseded by the `portfolio-v40` rewrite (§Live read in `2026-09-17-residue-slice.md`).
- 3.7 The shape contract's closing sentence states "with no code fence or surrounding
  prose".
  Check: fence deliberation in the interpretation thinking falls toward zero on the fixed set.
  Raised by the v38 read (L12).
  Ruled 2026-09-16: adopt; residue slice.
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17: interpretation fence markers fell from twelve to three; the sentence rides into `portfolio-v40`.
- 3.8 The ledger prompt states that a key driver with no series in this holding's list
  carries null.
  Check: the driver-mapping deliberation falls on the fixed set; the persisted driver
  series are unchanged in kind, and a driver with a plainly matching series still maps.
  Raised by the v38 read (L13).
  Ruled 2026-09-16: adopt; residue slice.
  Amended 2026-09-17: the prompt states the rule alone; the expectation clause ("most
  drivers will") is dropped, since it invites skipping a mapping that plainly exists.
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17: driver-null markers four on eighteen calls (v38 two); no change read.
  The rule rides into `portfolio-v40` as an output requirement, without the "which most drivers will be" clause the aligned draft had carried (ruled 2026-09-17 at task review: the amendment above stands).
- 3.9 The ENGINE SET line, at thirty percent of the action call's re-think since 2.2:
  an A/B on the fixed set — the live harness gains a variant that renders the set's
  underlying facts (the new-money admission result, the hurdle state, the grade bar)
  in place of the derived list, eighteen action calls each way, the contract unchanged
  until the evidence is read.
  Check: the ENGINE SET marker share and the rung table under both forms.
  Raised by the v38 read (L15).
  Ruled 2026-09-16: adopt the A/B; the form is ruled on its evidence.
  Ruled 2026-09-17: the A/B's read also scans every action call under both forms for invented permission claims, prose-ownership errors and rationale correctness.
  The marker share and the rung table alone do not decide the form (Codex churn analysis S3; DIA call 44 concluded three times that the set excludes add while quoting "the full ladder is yours").
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17 (fifteen Facts calls against thirty-two List): the Facts form steers harder on both admission outcomes — PSX's passing admission read as a buy signal (List trim ×3, Facts add, add, hold; engine-set markers 1.0 to 7.7 per call) and DIA's failing admission as a prohibition ("explicitly prohibits increasing exposure"; the permission scan hit "prohibit"); the Facts form is not adopted, the List form stays production, and the entry's ruling on the line itself is open (§Live read in `2026-09-17-residue-slice.md`).
  Ruled 2026-09-17 (the line): one data line naming the rungs the computed read supports, no permission sentence; the Facts form and its harness variant removed; landed as `portfolio-v41` (2.6).
- 3.10 The action packet's capital-efficiency line names its owner, horizon and rate: the
  engine's twelve-month total-return test against the stated hurdle rate, each of the four
  states saying what it establishes and no more.
  Check: no action call guesses the assessed horizon, and no `indeterminate` rationale cites
  the line as its reason for a rung (SPMO call 27 and DIA call 41 on the v38 read).
  Raised by the 2026-09-17 Codex churn analysis of the v38 read (S1), verified against the
  engine and the packet.
  Ruled 2026-09-17: adopt the wording; lands in the residue slice (`portfolio-v39`).
  The numbers themselves are 3.15.
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17: the prefix's rate and test gave the model a computation to run itself — on TSLA (indeterminate) one rationale asserts the position "fails capital efficiency tests" from a self-run point test; the failing holdings' rationales quote the rate correctly.
  Read as a regression on the indeterminate holdings (§Live read in `2026-09-17-residue-slice.md`).
  Superseded 2026-09-17 by 2.6: the section is the three tested returns and the hurdle rate as numbers under `portfolio-v41`.
- 3.11 The action packet distinguishes the grade's low-confidence marker (an imputed
  sub-score) from conviction (the degradation count), and states what the grade is: the
  backward composite of the quality, valuation and risk sub-scores, distinct from the
  forward scenario targets.
  Check: no rationale calls a low-confidence grade "low conviction" or a "confidence
  rating" on a Medium-conviction packet (DIA calls 40, 41 and 43 on the v38 read).
  Raised by the Codex churn analysis (S2).
  Ruled 2026-09-17: adopt; residue slice.
  Amended 2026-09-17: the coexistence clause became the grade's definition, so the sentence
  reads as a meaning, never a permission, and answers the imported grade-to-action
  convention directly (PGNY calls 51 and 53).
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17: no rationale called a low-confidence grade "low conviction"; four rationales across TSLA and DIA cite "indeterminate capital efficiency" as their reason, so the entry's second check is not met (§Live read in `2026-09-17-residue-slice.md`).
  Carried into `portfolio-v41` as the SCORES gloss and the grade line's imputed-score gloss; the state word the second check watched no longer renders (2.6).
- 3.12 The action packet labels the financial summary as model-authored, so its provenance
  reads beside the engine's.
  Check: no action thinking attributes the financial summary to the engine.
  Raised by the Codex churn analysis (S4, the labeling half).
  Ruled 2026-09-17: adopt; residue slice.
  Forwarding the model's target rationale is 3.16.
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Carried into `portfolio-v41` as the analyst / computed labels with one defining sentence (2.6).
- 3.13 The action response contract states "with no code fence or surrounding prose",
  the 3.7 sentence, since the two contracts are separate strings.
  Check: fence deliberation in the action thinking falls toward zero on the fixed set (SPMO
  call 22 spent seven markers on it).
  Raised by the Codex churn analysis (S5).
  Ruled 2026-09-17: adopt; residue slice.
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17: action fence markers forty-two on forty-seven calls (v38 seventeen on thirty-six); not met; rides into `portfolio-v40` unchanged pending the action-prompt rewrite.
  Carried into `portfolio-v41` as the task's opening sentence; the check rides 2.6's read.
- 3.14 The ledger prompt states what the margin is for — the noise band that keeps a print
  inside the series' ordinary variation from confirming a breach — and then the relative
  and magnitude caps the 6g seam applies (the ruled 1.8 rule), where the margin is authored,
  in the series' own unit; the margin stays model-authored.
  Check: `margin-implausible` downgrades fall on the fixed set (five on the v38 read, four
  of them PGNY's), and the margin-to-level ratios of the kept cores do not drift toward the
  cap across the read.
  Raised by the Codex churn analysis (S6).
  Ruled 2026-09-17: adopt; residue slice.
  Amended 2026-09-17: the purpose precedes the cap and the check watches for anchoring, so a
  model that learns the ceiling as the norm shows in the read.
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17: not met — `margin-implausible` five on eighteen calls (v38 five), eight `price-level-mismatch` against one (the statement and the core one margin apart, "$40 ($38 + margin buffer)"), and the thinking sized to the cap ("~23%, acceptable per text 'at most 25%'"); non-price kept margins drifted up (max 29% to 40%).
  Superseded: `portfolio-v40` shows no caps and sizes by example (§Live read in `2026-09-17-residue-slice.md`).
- 3.15 The action packet carries the hurdle read's numbers — the hurdle rate and the bear /
  base / bull twelve-month total returns it tested — beside the state.
  This adds to the decision's evidence set under `portfolio-analysis.md §Portfolio action`
  and needs that ruling on its form.
  Check: SPMO's `fails` calls reason from the tested returns rather than a guessed horizon.
  Raised by the Codex churn analysis (S1, the numeric half).
  Ruled 2026-09-17: a follow-on after the 3.9 A/B is read, its own plan; not in the residue
  slice.
  Absorbed 2026-09-17 by 2.6: the three tested returns render beside the hurdle rate under `portfolio-v41`.
- 3.16 The action packet forwards `model_target_rationale` and a compact statement of the
  engine's valuation and target basis, so a disagreement between the arms reaches the rung
  with both explanations and no governing arm.
  Same evidence-set ruling as 3.15.
  Check: PSX's action calls reconcile the engine's valuation sub-score against the model's
  fair-value read from the forwarded rationale rather than by inference (call 18 spent 3,447
  words on it).
  Raised by the Codex churn analysis (S4, the forwarding half).
  Ruled 2026-09-17: follow-on after the 3.9 A/B; not in the residue slice.
  Absorbed 2026-09-17 by 2.6: the analyst's target rationale and the computed bands' method clauses render in the packet under `portfolio-v41`.
- 3.17 The interpretation prompt names no app concept: one message in two parts — inputs,
  each section explained once and then its values; then the task in output order — with no
  arm, baseline, stage, seam, validator behaviour, stamp or product name, the caps unshown
  and the margin sized by example, and a placeholder-only return shape.
  Check: on the fixed set, interpretation markers per call and the level-class downgrades
  fall below the v39 read's, the kept cores match their sentences, and no rationale
  computes with a sentence the packet no longer carries.
  Raised by the user's reading of the rendered v39 prompts and the v39 read's finding that
  every explanatory clause became an input (2026-09-17).
  Ruled 2026-09-17: adopt; the action, role/risk and distillation prompts follow the same
  principle in their own slices.
  Landed 2026-09-17 as `portfolio-v40` (`2026-09-17-interpretation-prompt-rewrite.md`).
- Not adopted (L14, ruled 2026-09-16): rendering "keep the action firm run to run" on
  continuity calls only — the debut check is accepted cost.

## 4. Synthesis fields — define what the model is asked to attribute

Rests on: record stop-rule-2 residuals, Finding 9; Codex §6.
Docs that change: `web-research.md §The research loop and context management`,
`portfolio-workflow.md §Step 6d`.

- 4.1 `seeded_by`: derive seed lineage deterministically from the URLs the pass actually
  fetched (plus any attribution captured at gathering time), or redefine the field as
  thematic relevance; the fresh synthesis conversation cannot reconstruct which seeds
  guided a gathering history it never saw.
  When the pass has no seeds, show an explicit empty allowed set and require `[]`.
  Check: zero `unknown seeded_by reference(s) dropped` gaps on the fixed evidence set.
  Ruling: open.
- 4.2 `topic_answered`: define the coverage it asserts (evidence obtained versus every
  question answered); consider a compact per-question coverage state if one boolean
  stays ambiguous.
  Check: the definition appears in the prompt and the persisted value matches it on
  the six holdings' passes.
  Ruling: open.
- 4.3 Zero-page passes: the app assembles the no-evidence findings object itself,
  preserving gaps and orientation, instead of asking the model to write it up.
  This is a contract change to the always-synthesize path.
  Check: TSLA's forward-thematic pass costs no model call.
  Ruling: open.
- 4.4 Fund exposure fit: research gathers exposure facts only; the fit judgment moves
  whole to interpretation, which already sees the house view; retitle the topic.
  Check: no synthesis trace reasons about an unseen house view.
  Ruled 2026-09-15: adopt — research gathers exposure facts only, interpretation owns
  the fit judgment, the topic is retitled; rendering the house view into research is
  not adopted.
- 4.5 Untrusted text is data, not instruction, and also fallible evidence: an
  internally impossible number may be excluded or reported as a source defect.
  Check: PSX's impossible insider-sale amount is excluded or persisted as a source
  defect, not as a claim.
  Ruling: open.
- 4.6 `material_forward_fact`: supply a compact list of the fields the structured feeds
  cover, or change the model's task to naming a sourced forward fact and let the app
  decide whether the feeds lack it; the prompt currently asks the model to infer feed
  absence without showing the feeds (Codex §3).
  Check: no persisted forward fact duplicates a feed-covered field; the CapEx guidance
  case on TSLA resolves by the app's rule, not the model's guess.
  Ruling: open — Claude recommends the second form (app decides).

## 5. Dates — an authoritative anchor on every stage

Rests on: record Finding 7; Codex §3.
Docs that change: `web-research.md §The research loop and context management`,
`portfolio-workflow.md §Step 6d`, `§Step 6f`.

- 5.1 Put the run timestamp and market-session date at the top of every gathering,
  synthesis, distillation, interpretation and action prompt, with the newest filed
  quarter in the packet.
  Check: no trace calls a 2026 source "future-dated" or "simulated".
  Ruling: open.
- 5.2 Carry publication date, fact period and retrieval time as distinct fields through
  claims and distillation; reconciliation orders on fact period and publication, not
  retrieval time, and the rule must say what wins when two sources cover the same
  period, when a revision supersedes, and when a period or date is unknown or
  incomparable — the rule itself is the plan's to write.
  This changes the persisted claim shape (evidence-floor stamp).
  Check: ARKF's "announced September 16, 2026, trading March 31, 2025" contradiction
  and TSLA's "July CY25" cannot persist.
  Ruling: open.
- 5.3 Search date anchoring: queries for prior-year material are intentional and
  labelled, never the default.
  Check: no query asks for "2024/2025" as the latest period on a 2026 run.
  Ruling: open.

## 6. Gathering — failure memory and a visible stopping contract

Rests on: record Finding 1; Codex §7.
Docs that change: `web-research.md §The research loop and context management`,
`§Search backend` (fetcher notes).

- 6.1 A per-holding record of failed URLs with failure classes, consulted by the fetch
  tool: an exact-URL repeat returns the earlier failure without spending an attempt;
  a host that denies (401/403) gets a short-lived backoff; a transient failure never
  becomes a permanent ban.
  Check, as three separate expectations on the attempt-6 log: exact-URL reuse removes
  the repeats (nhtsa.gov 3 attempts on 1 URL, wsj 4 on 2, reuters 6 on 4); host
  backoff bounds a denying host to a set number of live attempts per window
  (phillips66 spent 11 on 9 distinct URLs, progyny 15 on 13, so URL reuse alone barely
  helps there); a transient failure is retried once and never banned.
  Ruling: open.
- 6.2 Reuse successfully fetched evidence across a holding's topics before searching
  again; expose remaining turns and the topic's unanswered questions to gathering.
  Check: cap-hit share falls on the fixed evidence set without raising the cap.
  Ruling: open.
- 6.3 Fetcher: declare the SEC fair-access User-Agent on web fetches (EDGAR API legs
  already do); render the error source chain into the progress row's detail, not only
  the outer "fetching <url>" context.
  Check: the web fetcher's request to sec.gov carries the declared User-Agent and a 403
  still classifies as denied (live availability is not the test); a progyny-class
  failure names its cause in the tracker row.
  Ruling: open.

## 7. Telemetry — measure what the runtime actually does

Rests on: Codex §8; record Finding 4 (retry causes).
Docs that change: `local-model-operations.md`.

- 7.1 Record per call, with explicit semantics: original packet size, whole-call
  elapsed, thinking characters, and the returned API counters; stop describing the
  fence's `generated` count as thinking-plus-content (it is the second task's count on
  this runtime's two-phase path).
  Check: a persisted prompt-usage row carries the four measures with their names, and
  the fence trailer's count is labelled as the runtime returns it.
  Ruling: open.
- 7.2 Log effective sampling parameters including inherited defaults (the observed
  `repeat_penalty 1.1` beside the app's own) at model load.
  Check: the serve-log line is quoted in the run record's configuration section.
  Ruling: open.

## 8. Process — a fixed-evidence gate before another book-scale attempt

Rests on: Codex §9; user priority stated 2026-09-15 (local model calls first).

- 8.1 Preserve a fixed evaluation set from attempt 6: TSLA (short-thinking corrupt
  ledger), PGNY (percent units, stale facts), ARKF (interpretation outlier, missing
  house view), DIA (tax leakage, numeric narrative drift), PSX (thin-source synthesis,
  the recovered blank findings), plus an empty-evidence topic and a fund with no seeds;
  add a role/risk-only fixture and a prior-ledger continuity fixture before any
  release claim.
  Packets rebuilt from the persisted rows and thought-log headers are labelled
  reconstructed, not replayed.
  Ruled 2026-09-15: adopt — the fixed-evidence gate precedes any further book-scale
  attempt; every slice from §1 on verifies against it, and the single big confirmation
  run comes after the calls pass it (BUILD §What remains' pre-run bar gains that
  sentence).
- 8.2 Order: sections 1–6 first, model, runtime and sampling held fixed; admit a
  candidate only when executable cores agree with their sentences, source dates stay
  intact, claims keep valid provenance, and tax or P/L variation cannot move the rung.
  Ruled 2026-09-15: adopt, as the gate's admission rule.
  Read 2026-09-16, §1+§2 slice: §2 admitted; §1 admitted conditionally on 1.5 landing and
  a re-read of the fixed set; dates and provenance not exercised (§4, §5) — the table and
  verdict are in `2026-09-16-ledger-conditions-and-action-packet.md` §Live admission read.
  Ruled 2026-09-16: that verdict stands; 1.5–1.8 and 2.4 land as one small follow-up
  slice before §3 (one plan, `portfolio-v37` since 1.7 and 2.4 touch prompts), then the
  fixed-set re-read admits §1 and §3 planning starts from it.
  Re-read 2026-09-16 on `portfolio-v37`: twenty-one kept cores, twenty-one matching; no
  wrong or levelless core admitted; the rung never moved with tax or cost — §1 ADMITTED
  (`2026-09-16-ledger-validator-follow-up.md` §The fixed-set re-read). §1 and §2 are
  both admitted; §3 planning starts from the admitted set.
  Read 2026-09-16 on `portfolio-v38`: twenty-two kept cores, twenty-two matching; no
  levelless core; the rung invariant under every variant; rationale ownership,
  account isolation, the fund enum and the debut fields all clean — §3 ADMITTED
  (`2026-09-16-interpretation-slice.md` §Live admission read); its six findings
  L10–L15 ruled the same day: 3.5–3.9 land as one residue slice ahead of §4
  (`portfolio-v39`); §4 planning follows it.
- 8.3 Experiments, after 8.2: non-thinking synthesis on the frozen evidence, comparing
  claim retention, dates, citation resolution and partial-coverage accuracy as well as
  time, with repeats for variability; then non-thinking action separately on the
  investment-only packet; interpretation stays thinking-on throughout.
  Ruled 2026-09-15: run them in that order after the packet fixes; synthesis is the
  expected win, action may be dropped if judgment quality suffers.
- 8.4 Success measures: end-to-end acceptance and useful evidence retained per unit of
  elapsed time, per-stage median and worst-case latency, retries, source drops,
  thought length; re-think marker counts stay a diagnostic, never the gate; set the
  user's time budget explicitly.
  Ruling: open.

- 8.5 The fixed evidence set's equity source is consistent with each fixture's computed
  metrics: a fixture carrying a debt/equity or price/book value stamps the equity source its
  live dossier would have, so the ledger prompt's statement-basis line and the computed
  metrics never disagree on the harness alone.
  Check: PGNY's interpretation prompt on the fixed set no longer says no equity line reached
  the engine beside a computed debt/equity (call 47 on the v38 read read that line as a
  template error and used the metrics).
  Raised by the Codex churn analysis (S7), read as a harness artifact: the live pipeline
  derives both from the same statement surface.
  Ruled 2026-09-17: a fixture fix only, no prompt change and no stamp, landing with the
  residue slice; the plan verifies the live dossier cannot produce the split before treating
  it as harness-only.
  Landed 2026-09-17 as `portfolio-v39` (`2026-09-17-residue-slice.md`).
  Live 2026-09-17: PGNY's prompt carried the balance-sheet basis with its computed debt / equity; no template-error reading in its thinking.
- 8.6 A sampling-profile comparison on the action call, on the fixed set: the current
  "Thinking — general" row against the vendor "Thinking — precise" row
  (`local-model-operations.md §Sampling settings`), the packet byte-identical, thinking on,
  the same repeats per holding each way with thinking captured; the model, runtime and
  `num_ctx` held fixed; greedy decoding excluded (vendor-warned).
  Rests on: the v38 read's rung flips on byte-identical packets (SPMO and ARKF between
  sell-all and trim across repeats, as on v36 and v37) — a debut-only exposure, since a
  continuity run renders the prior model-chosen action as its baseline; Codex churn analysis
  S8 / P9 and the Claude brief's H3, which both name sampling as a plausible cause no
  observation yet isolates.
  Requires 7.2 first, so each run's effective options (the inherited defaults included) are
  on the record and the comparison is attributable.
  Check: judged in this order — no wrong core admitted and every rung inside its holding's
  repeat set under both profiles; then the between-repeat rung agreement per holding; then
  elapsed time; re-think markers diagnostic only (8.4).
  The precise row is adopted for the action call only if correctness holds and rung
  agreement rises; an adoption updates the ops doc's stage mapping and is a per-call option,
  never a load-time change.
  The interpretation call is a second leg only if the first shows an effect.
  Ruled 2026-09-17: adopt as an entry; runs after the residue slice's read and after 7.2
  lands; its place relative to 8.3's action leg is a plan flag — Claude recommends before
  it, so 8.3 compares against the better thinking-on profile.

## Not adopted

Ruled 2026-09-15: all six ratified; none is re-raised in a plan.

- A universal "decimal threshold above 1 is invalid" guard (Claude, record Finding 2):
  legitimate growth exceeds 100%; superseded by 1.2's prose-mismatch form.
- Rendering the house view into the fund exposure-fit research topic (Claude, record
  Finding 9): superseded by 4.4.
- Compatibility reviews for persisted changes (Codex §2, §5, §9): pre-release, the stamp
  moves and the store re-wipes.
- Raising `num_ctx`, the output ceiling or the retry counts (Codex §9): no context
  exhaustion observed; the bounded retries recovered every transport or content failure
  while the semantic errors passed the gates.
- Sampling changes as a remedy (Codex §8): not identified as a cause; one variable at a
  time, only after sections 1–6.
- Streaming the research turns (record open question): visibility only, no effect on
  correctness or deliberation; stays deferred.

## Deferred watches (kept, not scheduled)

Recorded in the findings record's §Smaller watches and carried to the next attempt's
watch set rather than fixed here:

- The research leading indicator's direction field and its invented driver ids (SPMO,
  DIA) — the unverified-driver gap fires as ruled; read the direction accuracy at scale.
- `audit.narrative = null` on every holding that ran the narrative-sentiment topic —
  establish whether null is the no-hype resting state or a missing input.
- Source-registry coverage (CNBC landing as tier 4 unregistered).
- Distillation original-source allocation and the 12,000-character page cap binding on
  every fund — the evidence-selection follow-up already named in the record.
- Schwab stock rows arriving without an issuer description, leaving the
  listing-resolution guard unverifiable — an account-data property to raise with the
  ingestion leg, not a model-call fix.

## Open questions for the ruling round

- ~~Whether 1.3's app-assigned margin is per series only or per series and vehicle kind.~~
  Ruled 2026-09-16: moot — 1.3 not adopted.
- ~~Whether 3.1 renames the field (moves `checkpoint-v9`) or keeps its engine meaning.~~
  Ruled 2026-09-16: renamed — `model_target_rationale`, `checkpoint-v10`.
- Whether 4.3's app-built no-evidence object counts the pass as answered or as a gap.
- Which stamp 5.2's claim-shape change moves (evidence-floor, checkpoint, or both).
