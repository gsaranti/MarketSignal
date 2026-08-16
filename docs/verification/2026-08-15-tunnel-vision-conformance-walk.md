# Tunnel-vision doc↔code conformance walk (2026-08-15)

The conformance walk the tunnel-vision slice's disposition queued (user decision 2026-08-14 — [2026-08-14-tunnel-vision-slice.md](2026-08-14-tunnel-vision-slice.md) §Disposition):
a claim-by-claim walk of `portfolio-analysis.md` and `portfolio-workflow.md` (plus `storage.md` §Local Analysis Suite Storage and `interface.md` §Main Layout) and the freshly rewritten `logic-flow-docs/portfolio-analysis-logic-flow.md` against the as-built code, run before the Portfolio completion block's first slice so its plans code against verified contracts.

## Method

Seven parallel conformance passes on the piece-2 pattern ([2026-08-04-piece2-conformance-walk.md](2026-08-04-piece2-conformance-walk.md)), each owning a doc scope and its implementing code — (A) gate / holdings / eligibility / diff, (B1) pipeline orchestration (Steps 5, 6a, 6c–6e), (B2) engine calculations + §Starting parameters, (C) 6f / action call / 6g + ledger + verdict, (D) quick check / work-list / migration gate, (E) roll-up / outcome / persist / storage, (F) frontend / interface — with the designed-not-built, dormant, previously ruled, and awaiting-a-ruling inventories excluded up front.
Every surviving finding was re-verified against the tree by the orchestrating session before batching.

The bulk of the walk **conformed**, most notably the tunnel-vision contract itself, property-by-property:
profile isolation input-enforced by the split call (test-pinned), 6f authoring no action, the action call's schema / thinking / engine-set-as-evidence-with-pick-withheld / cash-row-free profile render / pre-v9 history label, the outside-set annotation on both branches, `feasible_actions` carrying no book term, the `portfolio-weight` retirement's skip-whole semantics, the migration force-include (conservative on missing/unparseable, self-neutralizing), episodes writing `lean = action` / divergence `None`, and no book input reaching the per-holding loop.
The logic-flow's exact per-model-call input lists for the 6f interpretation call and the action call verified **item-by-item** against the prompt builders, save one omitted response field (finding 9).
Also verified clean: the two-arm boundary at every seam, the quick check's full retrieval-and-evaluation contract, the outcome machinery (episode identity, windows, coverage, calibration, falsifier events), storage shapes + portability v3, the roll-up's deterministic reads, and the frontend's v9 rendering (rung + rationale on both branches, no weight/lean/sizing surfaces, `ThesisLedger` mirror weight-free).

Two suspected divergences dissolved on verification and are recorded here as non-findings:
the `data-sources.md` Portfolio endpoint table **does** carry its as-built-vs-designed intro note (the wired subset enumerated, every other row designed), so the unwired endpoint rows are properly marked at their canonical home;
and the Ollama Settings-indicator deviation (manual tests only) **is** recorded — in `BUILD.md`'s guided-setup follow-up bullet — so `interface.md §Connection status` staying spec-shaped is the recorded state, not an omission.

## Dispositions

**31 verified findings** after deduplication (passes B2 and E converged independently on finding 1): **0 code fixes**, **26 doc corrections** (19 plain + 7 as-built marking gaps), **5 open rulings**.
Every doc-correction verdict traces to ruled or canonical authority (a prior ruling, the canonical section, or the deliberate slice that shipped the behavior) — nowhere did the walk find code drifted from a doc that was right.

### C — doc corrections (19)

`portfolio-analysis.md`:

1. **Run retention reads "newest 10"; the ruled and built value is 30** (§Starting parameters :419; also logic-flow :1321).
   `PORTFOLIO_RUN_RETENTION = 30` (`mod.rs`, ruled 2026-08-11), enforced in `store.rs`; `storage.md` :178 already says 30.
2. **The feasible-set bullet still carries the 25% concentration cap and "with headroom"** (§Starting parameters :477–478).
   `engine::feasible_actions` has no weight, cap, or headroom term since `portfolio-v9`; the same bullet's :474 records the removal — :477–478 are tunnel-vision residue inside an already-updated paragraph.
3. **The data-health sentence still names "FMP-fallback recoveries"** (§Portfolio roll-up :361).
   No recovery concept survives the Stooq removal; `DataHealth` carries `deep_history_failures` only, and its doc comment names the retired key as decode-ignored legacy.
4. **The attention-trigger list omits the length-stop leg** (§Portfolio roll-up :364).
   The code fires attention on five triggers — the documented four plus `output_limited` (generation length-stopped), added deliberately by the pre-run slice (`job.rs` :1551–1555; 2026-08-10 record §Fix candidates 4).
5. **The cash-bound parenthetical implies a live profile-gated mechanism** (§Starting parameters :483).
   `available_cash` is consumer-less since v9 (`mod.rs` :97–103) — even a set value could gate nothing; the field survives for the planner and Settings display only.

`portfolio-workflow.md`:

6. **"Cash and buying power still feed the investor profile and the roll-up"** (§Step 3 :97).
   The roll-up leg is real; nothing feeds pulled cash into the profile (construction-era residue), and buying power is never pulled at all (`Holdings` carries `cash` alone).
7. **"A failed price refresh fail-softs to the last cached series"** (§The quick check :436).
   No such path exists — the market family types `unknown` and the band / hurdle reads skip (`quick_check.rs` :831–838); the canonical recipe itself says the sweep never reads the shared price cache (`portfolio-analysis.md` :185).
   The typed-`unknown`-and-force-include half of the sentence is built and stays.
8. **The 6a Type line names FINRA as a current retrieval source** (:172), while :178 of the same section marks FINRA short interest designed.
9. **`price_target_rationale` is missing from both authored-fields enumerations** (§6f Returns; logic-flow :1137–1153).
   The interpretation contract requires nine keys (`INTERPRETATION_KEYS`), the field is schema-required and persisted; both lists that purport to enumerate what the model authors omit it.
10. **The input-delta sentence overstates the engine's role** (§Step 6b :218; logic-flow :800–802).
    No typed input-delta artifact exists; as-built the deterministic legs are the position delta, the ledger-condition crossings, and the prior-run values rendered into the retrospective / continuity prompt for the **model** to compare — no engine metric-vs-prior or sub-score-vs-prior comparison runs, and positioning has no code.
    Soften to the as-built legs and mark the fuller delta designed.

`logic-flow-docs/portfolio-analysis-logic-flow.md`:

11. **§Work-list logic omits two force-include legs** (:428–438): the no-prior-verdict leg and the one-time pre-v9 migration gate — both built (`job.rs` :640–649, `whole_book_era_version`) and both stated in the sibling docs.
12. **Fund quick-refresh weights "when exposure is relevant"** (:1417) — retrieval is unconditional (the prior-ruled contract; gating is evaluation-side).
13. **Band semantics read "moved outside the band"** (:1425, :1441) — the flag fires on a **change in spot's relation to the authored stamp**, including re-entry, and not on a standing authored-outside state (`quick_check.rs` :1525–1553; canonical at `portfolio-analysis.md` :203).
14. **The `fresh_clear` / `unknown` definitions miss the unevaluable downgrade** (:1429–1436) — a fully successful retrieval still types `unknown` when a condition is `unevaluable_series` (`quick_check.rs` :1393–1425; canonical at `portfolio-analysis.md` :216).
15. **The Step-2 options block is wrong in four ways** (:268–297): chains are fetched per holding at the Step-6a gather, not at Step 2 (`job.rs` :903–915; `portfolio-workflow.md` :82 states it); greeks are not parsed (ruled as-built 2026-08-04); the put/call + IV/skew signal is computed at dossier assembly; and the overlay linking / classification sub-list (covered call / protective put / collar) is the designed, unbuilt overlay leg listed as built work.
16. **The risk-tier lists carry liquidity legs** (:685, :691, :698–699) — neither tier function takes a liquidity input, and `portfolio-analysis.md` :462 records the drop; the fund "thin / normal liquidity" legs are a flat contradiction.
17. **"Use revision dispersion when the spread is unavailable"** (:659) — a missing or half-published spread holds **both** driver legs at mid and records `flat_driver` (ruled 2026-08-05); the revision-dispersion widening waits on the revision feed.
18. **The risk-score input list includes drawdown and liquidity** (:601–602) — `risk_score` is realized volatility + debt/equity only; drawdown enters the tier, liquidity nothing.
19. **Expense-drag arithmetic and the house-view comparison sit under the no-model Step 6b** (:623–630) — no deterministic expense-adjusted return or exposure-vs-house-view comparison exists; the expense ratio and house view ride into the interpretation prompts as evidence.

### M — as-built marking gaps (7)

One convention question underlies all seven: the workflow doc marks as-built gaps inline everywhere else, and the logic-flow doc splits built vs designed at Step 5 — these places don't.

20. **Workflow §6c carries no stub marker** — the whole section (and its model-call block) is present-tense working behavior while the research stage is a deterministic stub (`pipeline.rs` :52–64); the stub status is nowhere plainly recorded in the docs corpus.
    The logic-flow §6c section (:848–931) rides the same gap, covered only by the file-level header.
21. **Workflow §6d documents the research-lane contract as current** — full per-topic findings + evidence ledger in, hierarchical variant, typed schema-validated output — while the as-built call is an unconstrained, non-thinking 2–3-sentence condense over the stub note (`distill_request`, no `format` schema, free string), and the `role_risk_only` branch makes no research or distill call at all (vs :284).
    Workflow :27's doc-wide "every generative call is schema-constrained" carries this standing exception unmarked.
22. **Workflow §6e's forward-assumption leg is present-tense** (:293–297) with only the two validator sub-legs marked unbuilt (:299) — `research_forward_assumption` has no code anywhere, no 6e stage exists in the loop (the overlay finalization is computed at the 6b seam, as the code comments say), and BUILD already names the forward assumption as joining with the research loop.
23. **Workflow :177 adds `news/stock` headlines "as research-loop seeds"** — no dossier seed leg exists (`HoldingDossier` has no news field); the endpoint's only wired consumer is the quick check's technology-falsifier leg.
24. **The technology-event pre-flag is present-tense in both step docs** (workflow :219; logic-flow :806–813) — no implementing code; it is queued in the completion block's first slice, so the marker's lifetime is short but the docs should be honest until it lands.
25. **The logic-flow's §Main data sources and Step-6a retrieval lists present the designed FMP surface as retrieved** (:205–206, :511–547): insider / congressional activity, peers / segments / ratings, key-metrics / scores / DCF, street targets — none fetched (`fmp.rs` :1783–1798 is the whole as-built stock pull); `data-sources.md`'s table marks the same rows designed, and the logic-flow's own Step 5 shows the built-vs-designed split convention.
26. **The logic-flow Step-8 embedding list includes the designed per-holding embeds unmarked** (:1326) — the only portfolio-namespace write is the matured-learning row; the workflow doc marks the per-holding verdict embeddings designed (:398).

### R — open rulings (5)

27. **The priced-fund scenario-target formula's status needs one consistent statement.**
    Logic-flow :647–649 says the formula "is not yet defined"; `portfolio-analysis.md` :87 says the fund-form methodology is **settled** (2026-07-16) and the v2-over-composite function is built and shipping (`fund.rs` :643–736, stamped with the shared target parameter version) — while BUILD / CURRENT carry "priced-fund scenario-target formula (undesigned)" as the fund-depth group's open item.
    The readings reconcile only if the shipped flat-driver v2-over-composite form is the stopgap and the undesigned item is a scenario-differentiated fund target formula — but no doc says that; the ruling should state precisely what is settled and what the fund-depth slice still owes, in one home.
    *Rule the wording (and where it lives) vs treat logic-flow :647 as simply stale.*
28. **`storage.md` :168 claims each holding on the run record carries a copy of the attention-flag state** (flag + trigger + evidence-event note, overlaid at ledger carry).
    As-built the overlay carries **condition evaluation states only** (`overlay_condition_states`); `HoldingVerdict` has no flag field, and the flag's only home is the single-row quick-check store.
    :169's separate eval-state claim is implemented.
    *Narrow the doc to the eval-state overlay vs build the per-run flag copy.*
29. **The SearXNG surfaces in `interface.md` are wholly unbuilt and unmarked**: §Pre-run web-research notice (probe + consent modal, wording table, suppress toggle) and the SearXNG halves of §Connection status have zero implementing code — they ride the deferred web-research slice.
    *Mark both designed (they are research-slice-gated, outside the completion block's bar) vs leave under the corpus's build-status-lives-in-BUILD convention.*
30. **`interface.md` :28's tree entry names "CSV import supplement" beside built siblings** with no designed marker — manual import was marked designed by the 2026-08-04 B8 ruling in the other docs; this mention appears to have missed the sweep.
    *Add the marker vs no action (tree reads as design).*
31. **The holdings sort bar renders only with more than one verdict card** (`PortfolioView.vue` :1252) — defensible (one card cannot reorder) but undocumented against two "carries a sort bar" doc claims.
    *No action vs a one-line qualifier in §Storage and display.*

## Notes (no ruling required)

- A **fully-offset zero-quantity netted position** is deliberately not-rated with its own reason (`pipeline.rs` :317–337); the docs describe only the net-short case.
  Recorded as an observed-stronger-than-documented behavior, not a divergence.
- The trough-release's "largest admissible print" (`portfolio-analysis.md` :433) is ambiguous between finite-positive admission and post-sanity-bound admission; code takes the print **before** the close join and bound (deliberate, per its comment).
  A wording tighten is available if wanted; not counted.
- `portfolio-analysis.md` :74's "expense drag (the expense ratio as an annual return headwind)" frames a raw evidence value as a computation; it rides finding 19's wording fix if the ruling extends there.
- The not-rated materiality threshold (:484–485) is doc-drafted with no code constant; the text already says "no as-built consumer", so it conforms as drafting.

## Rulings

- **C (19) + M (7) — ruled doc-fix wholesale and applied same-day (2026-08-15).**
  All 26 corrections landed across `portfolio-analysis.md`, `portfolio-workflow.md`, and the logic-flow doc; no code changed.
  Same-convention riders applied with the M batch — each a designed item the walk verified unbuilt, sitting present-tense in the logic-flow doc beside the flagged ones: the Manual-data block header, the N-PORT rows, the FINRA / CFTC / CBOE / SearXNG / Tavily source headers, the held-name-refresh bullets in §Work-list and the Step-8 stored list, and the CEF routing line.
- **R27–R31 — all five ruled by the user in a four-question walkthrough (R30/R31 combined into one question) and applied same-day (2026-08-15):**
  R27 the stopgap-plus-open-depth-item reading — the shipped flat-driver v2-over-composite form is the settled stopgap and the fund-depth group's undesigned item is a **scenario-differentiated** priced-fund formula; the logic-flow open-item block now says exactly that, and BUILD / CURRENT's "undesigned" naming rides the next user-run alignment.
  R28 the doc narrowed — `storage.md` now states the flag and evidence-event note live only in the quick-check store, the run record carrying the condition evaluation states alone.
  R29 both `interface.md` SearXNG surfaces marked designed-not-built (they land with the web-research slice).
  R30 the CSV-import tree entry gained its designed marker.
  R31 the sort bar's more-than-one-card condition documented in §Storage and display.

## Review

External review — **Codex rounds over the applied batch**, every finding verified against the tree before adoption.

**Round 1** — six findings adopted (two of them execution misses in the batch itself), one refuted.

Adopted:

- **The phantom quick-check price fail-soft survived in three spots.**
  The walk's own finding 7 edit was recorded as applied but never executed — the workflow §The quick check sentence stood unchanged — and two spots the walk had not caught carried the same claim: `portfolio-analysis.md` §Failure posture's cached-data generalization (now scoped: rate legs fail-soft to a cached print under the drafted max age; a failed price refresh has no cache to fall to), and the sweep-prices tracker message ("sweep from last values" → "market family reads unknown"; `quick_check.rs`).
- **The `unknown` definition stayed too narrow in the sibling statements** — the finding-14 fix landed only in the logic-flow: `portfolio-analysis.md` §Triggering, workflow §Step 6 pre-loop's force-include line, and the `quick_check.rs` sweep-state rustdoc all restricted `unknown` to retrieval failure; each now names the unresolvable-condition leg, which controls selective-run force inclusion.
- **The band-flag UI label contradicted the corrected semantics** — `PortfolioView.vue` rendered "price outside band" for every band trigger, false for a re-entry flag; the label is now "band relation changed" (the wire value `price-outside-band` is unchanged — persisted state identity).
- **Five propagation misses outside or beside the walked scope**: `data-portability.md`'s 10-run retention echo (→ 30), the logic-flow's Step-8 "attention flags stored with each run" line (→ quick-check store only, per R28) and Step-2 output's "overlay records" residue, `data-sources.md`'s Step-7a stand-in residue on the chains row (the stand-in retired with the construction stage), and `storage.md`'s present-tense web-research document cache (no cache table exists; marked designed).
- **Research presented as operational outside the new stage markers** — adopted at its four named spots: the workflow intro's research line (stub clause added), the Step-6 research-cache clause, the held-name refresh lane's missing designed marker, and the storage cache above; the 6c/6d/6e stage markers stay the single home for the stub's mechanics.
- **Two stale source comments** — `Holdings`' "cash / buying power" (buying power is never pulled) and the data-health attention rustdoc (Stooq-era "unrecovered", and the length-stop trigger missing).

Refuted:

- **"R27–R31 were applied off one blanket approval."**
  The five rulings were user-selected per question in the in-session walkthrough; the transcript the reviewer read does not capture that exchange.
  The rulings line above was tightened to name the four-question shape — a precision edit, not a concession.

**Codex round 2** — four findings, all adopted, none refuted (the reviewer withdrew the provenance claim on the round-1 evidence):

- **Checkpoint/resume documented as built.**
  The walk's own exclusion list carried "mid-run checkpoint/resume — only a cancel checkpoint exists (known)", which kept the marking gap invisible to every pass: as-built the only checkpoint is the between-holdings cancellation poll (`job.rs` :507, :957) — no per-holding persistence (a failed run leaves no row) and no resume entry path exists.
  Marked designed at every present-tense spot: §Failure posture's checkpoint/resume block (with the as-built statement), the hard-fail line's resume parenthetical, §Triggering's pinned-pull exception, §Evidence floor's resumed-run clause, and the workflow's step-table row and Step-6 intro.
  A residue sweep then caught seven sibling spots beyond the round's cited lines: the workflow's Step-2 resumed-run line, the 6b floor-exit checkpoint clause, and the 6g checkpoint pair; `portfolio-analysis.md`'s resume-window bullet; and the logic-flow's Step-6 checkpoint line, resume-behavior block, and 6g output row.
- **Research build status in the canonical doc** — the round-1 fixes reached only the workflow; `portfolio-analysis.md`'s intro, pipeline Step 3, §Triggering reuse clause, and the refresh-lane §Starting parameters bullet now carry the stub / designed markers, and the workflow's graduated-reuse sentence gained its clause.
- **The input-delta / pre-flag correction missed the canonical document** — the pipeline section's Step-2 sentence (`portfolio-analysis.md`) now matches the corrected workflow: the as-built delta evidence named, the engine-computed metric diff and the technology-event pre-flag marked designed.
- **`CURRENT.md` no longer described the working tree** — refreshed (file inventory, gate state, and both review rounds).

Round 2's edits are docs + metis only, so round 1's gate results stand: cargo 1,066 / 0 (28 ignored), clippy 0, `npm run build` clean, 46 + 238 frontend tests.

**Codex round 3** — three findings, all adopted:

- **The refresh lane and research reuse still read operational at the remaining sibling spots** — §Triggering's refresh force-include, the workflow's force-include line, the §Starting parameters reuse-window bullet, §Failure posture's fail-soft line, `configuration.md`'s lane paragraph, and `data-sources.md`'s web-tool row (whose own table marks its FINRA / CBOE siblings designed but had left this row bare) — each now marked designed.
- **The technology-event pre-flag's §Starting parameters bullet** marked designed like its pipeline sentence.
- **This section's stale "one Codex round" intro** reworded to per-round labels.

Round 3's edits are docs-only again; the gate results above stand.

**Codex round 4** — two findings: one adopted, one deferred by user decision.

- **Adopted:** §Failure posture's two standalone research claims ahead of the scoped fail-soft line — the "research cache survives" clause in the new-run sentence and the in-run "web research is fail-soft" sentence — now carry the stub / designed scoping.
- **Deferred (user decision):** the `CURRENT.md` round-count and file-inventory staleness rides the metis session-end refresh rather than a mid-cycle edit.

Round 4's edit is docs-only; the gate results above stand.
