# Portfolio Analysis large-scale review (2026-08-24)

## Verdict

A full review of the built Portfolio Analysis job across three priorities — financial correctness, mid-run abort risk, and alignment with `logic-flow-docs/portfolio-analysis-logic-flow.md` — at `main` `457efe1`, `portfolio-v12`.
The core financial spine is sound: every grading, scenario-target, ledger-evaluation, outcome-scoring, fund-path, TTM-assembly, and netting formula was recomputed by hand and verified correct, and all twelve of the logic-flow doc's "most important safety rules" hold in code.
The review found **1 critical, 7 major, and 26 minor findings**, plus one documented design posture flagged as the dominant real-world abort risk.
The critical finding is a mid-run killer: the local-model HTTP client's 600-second total deadline contradicts the 65,536-token thinking reservation, so a legitimately long reasoning chain fails the whole run.
The three financial majors are a missing split-adjustment guard on every stored-basis price comparison outside outcome learning, an unbounded end-bar staleness hole in outcome-label scoring, and a structurally dead research→ledger citation channel that silently disables research-supported qualitative falsifier trips.
The four alignment majors are a failure-posture sentence that contradicts the checkpoint trail, two "exact inputs" enumerations that omit whole rendered prompt sections, and a misdescribed sub-distillation drop trigger.
The recurring weak class across the financial findings is cross-basis comparison: values stored under one basis (pre-split prices, one share count, a TTM vintage, a benchmark window) compared against values on another without verifying the bases match.

No code or documentation was changed by this review.
This file is the only intended worktree addition; every fix is a separate decision.

## Scope and method

Nine independent parallel reviews covered, in the review's priority order:

- **Financial correctness** — all of `engine.rs`, every model prompt the job sends (6c research, 6d distillation, 6f interpretation both branches, the action call), `fund.rs` / `pre_profit.rs` / `outcome.rs`, and `dossier.rs` / `quick_check.rs` / `diff.rs` plus the Schwab netting path.
- **Mid-run abort risk** — `pipeline.rs` / `job.rs` / `mod.rs` / `store.rs`, the research layer (`research.rs`, `distill.rs`, `web_research/`, `local_model.rs`, `listing.rs`), and a dedicated panic/hang sweep of the six compute modules against hostile real-world data.
- **Logic-flow alignment** — a claim-by-claim walk of all 1,652 lines of `logic-flow-docs/portfolio-analysis-logic-flow.md` against the implementing code, every stated constant checked.

Every finding below was independently re-verified against the cited code in this session before inclusion; none is reported on a reviewer's word alone.
Severity: **critical** = kills or corrupts a run with a realistic trigger; **major** = materially misleading output or a materially wrong doc claim under common conditions; **minor** = edge-case, gated-reachability, or drift.
Deliberate design decisions on record (drafted-uncalibrated constants, data-honesty gaps, the tunnel-vision contract, the 2026-08-24 channel rulings, the documented hard/soft failure split) were treated as intent, not defects.

## Priority 1 — financial correctness

### F1 — major: no split-adjustment guard on any stored-basis price comparison outside outcome learning

FMP dated EOD — the suite's only deep-price rung — is retroactively split-adjusted (pinned by `docs/verification/2026-08-02-fmp-light-eod-adjustment-basis.md`).
After a split, every fresh price is on the new basis while the stored authoring-time values are on the old one, and nothing on the quick-check or ledger path re-bridges them (`quick_check.rs` contains no split handling at all).
The consequences compound:

- A ledger price falsifier authored as an absolute threshold ("below 300") reads a 4:1-split price of ~100 as a breach; two sweep prints then satisfy `LEDGER_CONSECUTIVE_MARKET_DATA = 2`, a **false ConfirmedFalsifierBreach** persists, and `overlay_condition_states` carries it onto the next full run's ledger.
- The stored-multiple rescale `price / b.spot` (`quick_check.rs:1311-1316`) misstates P/E, P/S, and P/B by the split factor.
- `engine::reanchor_scenarios` prices old-basis per-share drivers against the new-basis spot (`quick_check.rs:1447`), producing absurd total returns that flip the hurdle and admission reads.
- `narrative_vs_reality`'s `prior_spot` leg compares spots across runs on mixed bases, corrupting the expansion read.

Outcome learning already solved exactly this hazard class with its `anchor_close / authoring_spot` bridge (`outcome.rs:365-378`; `docs/portfolio-analysis.md` §Outcome learning), so the gap is also an internal inconsistency: the codebase knows the hazard and guards one consumer of three.
It fires only on a real corporate action, but a split inside a dozens-holding book across months is an expected event, and when it fires the output is confidently, silently wrong.

**Resolved 2026-08-27.**
The fix generalizes the bridge with a sharper leg than the reference's quote basis: each full pass stamps an **anchor bar** — the newest settled close strictly before the run's ET session, from the run's own fetched series — onto the audit (`HoldingAudit.authoring_close`, both branches), and a later read re-fetches the same bar date, `fresh ÷ stored` being exactly the cumulative re-basis factor (`engine::split_bridge_factor`, a drafted 10% deadband absorbing data revisions), so the factor is exactly 1.0 in the unchanged common case and carries no intraday quote-vs-close residual.
The sweep converts transiently — ledger `price` thresholds and margins, the stored-multiple rescale's spot, the re-anchor's spot, the frozen band targets, and the revision comparator — while the full pass normalizes the ingested prior ledger once at ingestion (evaluation, 6g validation and carry, and persistence all see one basis, a verbatim re-emission of the converted number keeps its condition id, and the 6c/6d prompts read the same instance while rendering statements only), bridges the prior spot and prior consensus mid together (their ratio is basis-free and must stay so), and writes a detected re-basis as its own input-delta entry.
An anchor whose bar is missing from the fresh window is an unverifiable basis — the comparison is excluded typed, never run cross-basis — and the sweep's per-holding EOD lookback widens past the 180-day floor to keep an old vintage's anchor reachable.
Two Codex rounds hardened the posture where the first cut had it only on the sweep.
Round one: the full pass gates price-denominated conditions out of its ledger evaluation whole on an unresolvable bridge (both branches), and the sweep's revision family and a band-only market family read `unknown` instead of `fresh_clear` through an excluded leg, the band-only note naming the actual cause.
Round two closed the pass-after hole the first anchor policy left open — a dropped anchor read as factor 1.0 one pass later, re-opening the original false-cross: an unresolvable pass now **carries the prior anchor forward unchanged**, so provenance is preserved, later passes stay fail-closed while the bar is missing, and the conversion lands correctly the moment it resolves (a chained three-pass regression pins gated → still-gated → healed under the carried id).
The carry's invariant is validator-enforced: at an unresolvable pass a new or re-anchored price-denominated core downgrades to qualitative (typed — authored against fresh prices, it cannot be tied to the carried anchor), while a carried-verbatim core stays quantitative; the sweep's lookback reaches the anchor's own bar date, which a carried anchor can place older than the last-pass boundary.
Round three closed the mirror hole round two opened: the carried anchor was coherent with the carried cores but not with the same audit's freshly computed comparators (the quick basis, its consensus mid, the fresh-stamped monitor targets), which would double-convert the moment the anchor resolved — fabricated revision events, mis-scaled multiples, hurdle and band distortions.
An unresolvable pass now persists no fresh anchor-dependent comparator at all — the quick basis withheld, the monitor stamped target-less — so the row is single-basis by construction, and the chained regression feeds each pass from the prior audit's actual persisted state, pinning both the withholding and the healed re-persistence.
The same round reconciled the one-basis claim across the code comment, the canonical doc, and this record: the 6c/6d prompts read the normalized instance but render statements only.
Round four traced the withholding to its two downstream consumers.
The verdict's own engine targets rightly persist fresh — the pass's user-facing read, coherent with its own prices — but the next pass's target delta row would have bridged them by the carried-anchor factor, a fabricated target-change entry in the 6g evidence vocabulary; that comparison now also requires the prior pass's certified authoring spot, excluded when it was withheld.
And the sweep after a withholding pass read `fresh_clear` through the very legs the pass withheld — the revision comparator gone, the band target-less — so the sweep now recognizes the withheld signature (anchored, basis-less, priced) and reads the revision, market, and rate-anchor families `unknown` with the cause named, while carried-verbatim price cores stay correctly evaluable through the carried anchor.
Regressions pin the delta-row gate and the full-pass-output → quick-check seam.
The residue that remains is pre-field-shaped: an anchor-less row (pre-field, or a priced verdict whose run had no dated closes) runs as stored until a resolvable full pass stamps an anchor, so a real re-basis inside such a window goes undetected — accepted, and healed in full by the big run's whole-book pass.
Two findings were declined with reasons.
Statement prose quoting an old-basis number is model-authored and is never rewritten by the app: every compared, validated, and persisted machine value converts, a tie to a quantitative condition enables nothing (the research-supported channel honors qualitative trips only), and prose heals at the model's own rewrite, guided by the re-basis delta entry — rendering the machine core beside the statement in the 6c seed and 6d citation list is recorded as a named, undecided candidate and a `PROMPT_VERSION` event, not a rider on this fix.
Ruled 2026-08-27: the core-beside-statement render is declined.
Every compared, validated, and persisted machine value already converts, and prose heals at the model's own rewrite guided by the re-basis delta entry, so the render would add prompt bulk against context pressure for a confusion no run has shown.
It returns only on run evidence of old-basis prose misleading a verdict.
And distinguishing new anchor-less rows from pre-field rows buys nothing pre-release when the big run stamps the whole book.
A pre-field row carries no anchor and runs as stored until its next full pass stamps one; the quote-bridge fallback for such rows was considered and cut by the avoid-premature-backward-compat rule, since the big run — a full pass over the whole book — stamps anchors everywhere.
On re-verification the fourth listed consequence sharpened: `narrative_vs_reality`'s expansion read is a ratio of same-run multiples on both legs and is therefore basis-invariant; the cross-basis leg is the fallback form's **annualized price pace** (`engine.rs`, the `spot / prior_spot` power), which the paired bridging fixes.
The canonical contract is `docs/portfolio-analysis.md §Starting parameters` (Split-adjustment bridge), with pointers from §The quick check and `docs/portfolio-workflow.md §Step 6b`; outcome learning and the retrospective block keep their existing quote-based bridge, whose comparisons target closes.
No `PROMPT_VERSION` bump: the rendered ledger and delta change as data, through the existing entry vocabulary, not as template text.

### F2 — major: outcome-label scoring bounds the series tail, not the scored bar

`covers_through` (`outcome.rs:862-867`) checks only that the **series' latest bar** reaches the window end within `COVERAGE_TOLERANCE_DAYS`, but the bar actually scored is `close_at_or_before(closes, w_end)` (`outcome.rs:1103`), which carries no staleness bound.
Over a series with an internal gap, a window's end bar can be arbitrarily stale — in the degenerate case the entry bar itself, recording a fabricated exact-0% return for a window in which no post-entry price was observed (`outcome.rs:1131`).
`bench_return` has the identical shape on the benchmark legs, and the falsifier lead-time stamp is distorted to bar counts over the same sparse series.
These labels enter the cohort means, band calibration, the head-to-head, and the outlook hit-rates — the exact surfaces the calibration tier will tune against.
Reachability is gated: the production cache writer fetches floor→today contiguously, so it needs a source-side range clamp or a stale cache surviving a failed refresh — but when it fires it is silent.
The fix-shaped invariant: bound `end_bar.date` to `w_end − COVERAGE_TOLERANCE_DAYS` and require `end_bar.date > entry.date`.

**Resolved 2026-08-27.**
The named invariant lands at one helper — the scored end bar is the last close at or before the window end, bounded to `w_end − COVERAGE_TOLERANCE_DAYS` and required to sit strictly after the entry bar — applied to the holding leg and both benchmark legs (`window_end_close`, `outcome.rs`).
A window whose end bar fails the bound reads uncovered and rides the existing pending → grace → typed-unscorable ladder, retiring both the tail-only `covers_through` judgment and the silent per-window skip where no end bar resolved at all.
The cache-refresh judgment tightened to the same end-bar rule, so a cached series with an internal gap at the window end earns its one heal attempt instead of being served as covered.
Regressions pin the fabricated flat-return shape held pending then closed terminal, the benchmark analog returning no leg, the end-bar-on-entry-session refusal, and the gapped cache healing through one fetch.
Three pre-existing fixtures that scored their 12-month window off a months-stale bar — the finding's own shape — gained genuine end-window bars.
The falsifier lead-time bar-count distortion over mid-window interior gaps sits outside the named invariant and stays open as recorded above.
A Codex round then closed three follow-ons.
The heal judgment now weighs every due window end rather than only the furthest, so an interior gap at an earlier due end earns the same one fetch (the multi-end `series` seam, regression-pinned).
The past-grace discriminator reads disappearance from the series itself: a series still alive past the window end closes `price-coverage-unscorable`, terminal staying reserved for a series that actually stopped — reconciling the closure typing with the `storage.md` discriminator sentence and the TO contract's terminal reservation.
And the coverage-tolerance constant's comment states the end-bar invariant instead of the retired tail rule.
The canonical coverage rule is `docs/portfolio-analysis.md §Outcome learning`.

### F3 — major: the research-supported qualitative-trip channel is structurally dead, and the prompt asserts it closed

The 6g validator honors a qualitative tripped/fired claim when a fresh research claim ties to the condition via `related_condition_id` (`pipeline.rs:1728-1746`; ids collected at `pipeline.rs:1263-1279` priced, `783-798` role-risk).
That tie can never be produced:

- No prompt anywhere renders the ledger condition ids (app-stamped UUIDs, `pipeline.rs:1686,1704`); the 6c seed renders conditions as bare `FALSIFIER: {statement}` lines (`research.rs:363-368`).
- No distill prompt mentions `related_condition_id`; the field exists only in the schema (`distill.rs:323`), the validation that accepts only known ledger ids (`distill.rs:842-850`), and tests.
- `render_prior` drops a prior claim's `related_condition_id` on re-emission (`distill.rs:1338-1347`), so even a hand-seeded tie would decay.

Consequently every qualitative trip is rejected with "no source-backed research finding supports the claim", and the topic-seed priority tier for claims tied to an open condition (`research.rs:385-398`) never fires.
The parallel `confirms_driver_id` channel proves the intended pattern — render the ids, tell the model to cite them — so the condition channel got the validator but never the prompt.
Compounding it, the ledger-rewrite instruction still carries the pre-v12 sentence "a qualitative claim needs a source-backed research finding, and none are available this run" (`pipeline.rs:4006-4009`) — static text contradicting the same prompt's own rendered DISTILLED RESEARCH block, accurate today only because the channel is broken.
The net effect is misleading output in the design's own terms: legitimate qualitative falsifier trips — the standing-thesis breaks the ledger exists to surface — are systematically suppressed.

**Resolved 2026-08-26.**
The channel's back half was built and its front half never was.
The fix adds the front half on the `confirms_driver_id` pattern.
Every claim-emitting distillation prompt — tier-1, pass, tree-reduce, and reduce, on both branches — now renders the ledger conditions with their app-assigned ids and asks the model to set `related_condition_id` to the condition a claim bears on.
A re-rendered claim carries its tie (the cached prior lines, the dormant-topic lines, and the reduce's tier-1 lines), and the app inherits an omitted tie for a verbatim re-emission — the same URL and claim text — across the pass→tree-reduce and tier-1→reduce hops, and from the prior layer onto a claim that resolves as cached, never onto a different claim from the same page, never at an ambiguity, and never substituting for an unknown cited id.
The known limit of that fallback is that the model cannot clear an inherited tie by omission or null — only rephrasing the claim or citing a different known id moves it — recorded rather than engineered around: a prior tie rides only onto a cached re-emission, which never enters the support set, and a fresh claim's tie is always this run's own assertion validated against the ledger.
The interpretation prompt sees the support without seeing the ids: each fresh tied research finding in the INPUT DELTA names the condition it bears on by statement, the ledger projection marks that condition RESEARCH-SUPPORTED THIS RUN, and the rewrite instruction names that mark as the qualitative leg, retiring the stale "none are available this run" sentence.
The fund branch's fresh research claims now join its input delta with their ties, so the role-risk 6g's already-built research leg is reachable too.
The 6c seed keeps its bare `FALSIFIER:` lines by decision: the tie is made at 6d by the distiller reading the conditions list beside the pass findings, so rendering ids in the seed would spend seed budget for no channel gain.
The contract is recorded at `docs/portfolio-workflow.md §Step 6d` with the 6g pointer, and in the logic-flow doc's 6d output and 6g validation lists.
`PROMPT_VERSION` moves to `portfolio-v13`, so a pre-F3 checkpoint cannot resume into the live channel and every run record stamps the contract it was analyzed under.
Two Codex rounds keyed the inheritance by claim text as well as URL (a URL-only carry let an unrelated fresh claim from the same page acquire a prior tie and count as support), then split the tie pools so a prior tie never rides onto a claim that resolves as fresh (freshness resolves by URL, so a prior claim re-emitted verbatim at a page fetched this run for other evidence would otherwise have carried its old tie into the support set), harvested the pass→tree-reduce hop, and required the version bump.

### Priority-1 minor findings

- **Loss-forecast displacement in the shadow fill** — `feed_present` requires `v > 0.0` (`engine.rs:2043`), so a published negative EPS consensus counts as absent and a whitelisted research fact overwrites all three consensus legs (`engine.rs:2082-2089`), against the supplement-never-displaces contract.
  Confined to the shadow-only 6e audit line — but that line is the promotion evidence the 2026-08-24 shadow ruling reads, so pollution there matters.
  Resolved 2026-08-27: `feed_present` in `refine_targets_with_assumption` now reads feed presence as `is_some()` — a loss forecast is a present value, never an absence.
  The supplement rejects against it and the audit's rejection line names the present leg and its value, so an inspector reads the sign.
  A pinned test holds a `-0.50` and a `0.0` consensus on the revenue rung and proves neither is displaced.
- **Bracket displacement in the shadow fill** — the presence guard read only the mid leg (`eps_mid` / `revenue_mid`) while the accepted supplement overwrites all three low/mid/high legs, so a feed carrying a low/high bracket without a midpoint still admitted a displacing supplement.
  The shape is production-representable: the FMP consensus builder shapes every leg independently, so a row with `epsLow` / `epsHigh` and a null `epsAvg` lands as a bracket with no midpoint.
  Surfaced by the loss-forecast slice's plan and confirmed by its Codex round; folded into that slice under the one-seam exception, since the fix is the same presence predicate.
  Resolved 2026-08-27: presence now reads any of the driver's three legs, and the rejection line names each present leg with its value.
  A pinned test holds an EPS low/high bracket with no midpoint on the revenue rung and proves it is not displaced, and that an EPS bracket never blocks a revenue fill.
- **TTM seam gap in the narrative fallback** — `ttm_revenue_window` checks contiguity only inside each 4-quarter window (`engine.rs:3103-3109`), never across the seam between the current and prior-year windows (`engine.rs:3156-3162`), so a feed gap at the seam yields a mislabeled "YoY" over 15 months; the dossier's own `apply_ttm_statement_basis` demands the full 8-row run for exactly this pair (`dossier.rs:587-598`).
  Resolved 2026-08-27: `ttm_revenue_window` now checks contiguity over the run from the newest quarter through the window, so the prior-year window's check covers the seam.
  A seam gap surfaces as the fallback's typed prior-year-window absence, never a misaligned read.
  A pinned test drops the quarter at the seam and proves the fallback gaps, a gap inside the newest four types the current-window absence, and a gap past the eighth row never trips.
- **Historical anchor share-count fallback** — a historical revenue-per-share anchor whose quarter lacks `diluted_shares` falls back to the newest quarter's or today's count (`engine.rs:2210-2213`), skewing the anchor-multiple history in the financially wrong direction under buybacks or dilution.
  Resolved 2026-08-27: `stock_anchor_observations` denominates each historical print on a diluted count from inside its own TTM window — the anchoring quarter's, else the nearest within the window — with no fallback to the newest filing's or today's count.
  A window with no in-window count is inadmissible, thinning the anchor set like a gapped window.
  The anchor carve-out from the latest-count basis is stated once at `portfolio-analysis.md` §Starting parameters, beside the forward driver's basis.
  The blanket at that home and its two mirrors (`trade-opportunities.md` §Starting parameters, the TO logic-flow) are scoped to forward conversions, off the Codex round.
  A pinned test proves an in-window neighbour denominates a count-less anchoring quarter and that a window with no count drops rather than riding today's shares outstanding.
  The new denomination changes the basis of every stored target the anchor multiples feed, so by the targets stamp's own criterion it should have cut the stamp itself.
  It rides `targets-v5` instead, cut twenty-five minutes later by the one-month band slice, and that stamp's history now names both changes (recorded 2026-08-27).
  No retro-bump: no `targets-v4` record was ever persisted, so there is nothing for a separate stamp to distinguish.
- **One-month band is unscaled daily volatility** — `(daily σ × 2).clamp(0.02, 0.15)` (`engine.rs:2562-2565`) understates a month's 1σ move by ~√21 against the suite's own √t convention (`dispersion_floor`, `tech_event_pre_flag`), so the printed band covers ~0.44σ of the month.
  The doc comment marks it "v1 mechanics", so this is surfaced as a deliberate-retention judgment call rather than an accident.
  Ruled 2026-08-27: scale by √t — the band becomes daily σ × 2 × √21 under the same clamp, matching the suite's `dispersion_floor` / `tech_event_pre_flag` convention.
  It is a units inconsistency rather than a calibration, so it does not wait on outcome evidence.
  It is its own slice and a `SCENARIO_TARGET_PARAMETER_VERSION` bump, since stored one-month targets change basis.
  Resolved 2026-08-27: the one-month half-band is daily σ × 2 × √21 under the unchanged clamp, the methodology line states the basis, and `SCENARIO_TARGET_PARAMETER_VERSION` is `targets-v5`.
  A pinned test proves a 1% daily σ prints a ~9.2% band rather than 2%, the clamp still binds at both ends, and the no-volatility fallback stands.
  The 15% cap now binds from ~1.64% daily σ, so the saturation share is a `big-run-watch-set.md` line (§Grade, valuation and targets).
  The Portfolio logic-flow's band formula is aligned to `targets-v5`.
- **Tech pre-flag benchmark coverage unchecked** — `latest_on_or_before(benchmark_closes, latest.date)` (`engine.rs:3260`) never verifies the benchmark covers the holding's newest session, so a shorter benchmark series silently mismatches the windows instead of taking a typed gap; rare, since both legs ride one FMP fetch.
  Resolved 2026-08-27: `tech_event_pre_flag` reads the benchmark on the holding's own two window sessions — its latest close on or before the prior read and its newest close — through an exact-session lookup, so the two legs are never read over mismatched windows.
  A benchmark lacking a close on either session is a typed gap naming the benchmark and the session, never a flag.
  The exact-session read takes the last row for a date, the policy `latest_on_or_before` already applies, so a duplicated bar never makes the read arbitrary (Codex round).
  The rule is stated once at `portfolio-analysis.md` §Starting parameters and mirrored in the Portfolio logic-flow.
  A pinned test proves a benchmark stopping one session short, or holed on the newest session, gaps where it would have fired over the shorter window, and that a benchmark running past the holding still reads on the holding's newest session.
  The elapsed-session count reads holding rows (`engine.rs:3467-3470`), so a duplicated bar — FMP's parser sorts but never dedupes — would inflate it toward a higher threshold.
  That direction is conservative and no duplicated bar has been observed, so it is recorded 2026-08-27, not actioned.
- **Tech pre-flag prior-endpoint alignment** — `latest_on_or_before(benchmark_closes, prior_session)` (`engine.rs:3360-3361`) resolved the benchmark's prior-end close independently of the holding's, so a benchmark hole at the holding's anchor session shifted one leg's window — the same silent mismatch at the other end.
  Surfaced by the coverage slice's plan and folded into it under the one-seam exception (ruled 2026-08-27), since the fix is the same exact-session read.
  Resolved 2026-08-27: the benchmark's prior-end close is read on the session the holding's anchor resolved to, and a missing one is the same typed gap.
  The pinned test holds a benchmark with bars either side of the holding's anchor session and proves it gaps rather than reading the earlier bar, and that a prior read on a non-session keys off the holding's resolved anchor with the session count untouched.
- **Pre-profit backfill counts any-role periods** — `backfill_required` counts distinct stored periods of any observation role (`pre_profit.rs:236-251`) where the documented rule counts *comparable* periods (bound + actual pairs), so a metric with four guidance rows and zero actuals suppresses the mandated backfill on later passes — blinding miss-detection exactly where guidance is open.
  Resolved 2026-08-27: `backfill_required` counts, per guided identity, the stored periods holding both a guidance bound and an actual — the pairing the execution read attains against — and binds below `BACKFILL_MIN_COMPARABLE_PERIODS`, the miss window's own depth.
  The count sits before the miss rule's polarity and finite-bound guards, which bound which pairs can miss rather than whether history exists.
  The definition is stated once at `portfolio-analysis.md` §Starting parameters; the mirrors already say comparable.
  `PRE_PROFIT_PARAMETER_VERSION` is `pre-profit-v2` (Codex round, two passes): the obligation feeds accepted observations, the execution read, and the verdict, so a pre-change checkpoint trail is refused rather than resumed across the predicate.
  The stamp also keeps a v1 overlay's absent backfill attempt distinguishable from a v2 waiver.
  A pinned test proves four guidance-only periods bind, four bound + actual pairs discharge, unpaired periods never count, point guidance is a bound while a range high alone is not, a never-guided metric carries no obligation, and a covered identity never discharges a thin one.
- **Fund momentum band saturates** — the fund path scores `trailing_return` over the ~1,600-day deep history when present (`fund.rs:1027-1036`) against the stock path's ±30% band tuned to a 180-day window (`fund.rs:823`), so nearly every fund pins at 0 or 100; momentum sits outside the letter, so the damage is context quality in the prompt and the frozen `CalibrationSnapshot`.
  Resolved 2026-08-27: `analyze_fund` scores momentum through `engine::momentum_score` over its `base_metrics` price legs — the stock read on the 180-day `price_history`, one window and one band — and the deep-history `trailing_return` helper is deleted.
  A fund with no short window imputes the neutral 50, the stock path's own posture.
  `GRADE_PARAMETER_VERSION` is `grade-v2.2`: no letter moves, but a persisted fund `sub_scores.momentum` means a different read across the boundary, which the what-changed delta row and the frozen `CalibrationSnapshot` consume.
  The stamp's consumers name what a boundary changed (Codex round 1): `engine::grade_parameter_change` reads the boundary, the delta row and continuity NOTE describe the fund momentum re-homing on a priced fund, and a stock across v2.1 → v2.2 gets neither — the generic "letters can move" row was a citable delta entry the evidence validator accepts, so on an unchanged holding it would have been false evidence for a real move.
  The boundary is read cumulatively from a stamp history per branch (Codex round 2): a `grade-v2` fund crosses only the momentum re-homing since v2.1's signed P/E never touched the fund path, an unrecognized stamp asserts no cause, and both consumers render only over a priced prior — a never-priced record had no letter or sub-score to move.
  The branch is the prior record's own, read off its persisted asset class — the key the job routes the fund path on, so the class is the branch for every record ever written, where the derived `fund_class_label` is post-field (Codex round 4) — so a symbol reclassified between runs reads the boundary its prior actually crossed; and a pre-stamp prior crosses the whole history rather than a blanket recalibration — git shows every fund letter-bearing line unchanged since the fund path landed on 2026-07-16, before the first stamp, so a pre-stamp fund is re-homed, not recalibrated (Codex round 3).
  The rule is stated once at `portfolio-analysis.md` §Starting parameters and mirrored in the Portfolio workflow and logic-flow; the big-run watch set names what each branch should carry.
  A pinned test holds a deep series that tripled over a flat 180-day window and proves the fund's momentum equals the stock read (~52, not 100), and a second proves the neutral impute with no short window.
  A third pins the history per branch, and a fourth proves the consumers over real priced priors: a stock is silent across v2.1 and recalibrated across v2 or pre-stamp, a fund is re-homed across v2.1, v2, and pre-stamp alike, an unrecognized stamp or a never-priced prior is silent on both, a fund prior on a stock dossier is re-homed while a stock prior on a fund dossier is silent, and a fund prior stripped of its derived label still reads the fund branch.
- **Expense ratios flattened by `{:.3}` rendering** — the interpretation and action prompts render expense ratio and expense drag through `opt()`'s three-decimal format (`pipeline.rs:2634`, `3236`, via `pipeline.rs:3477-3479`), so a 0.03% fund prints `0.000` — which the prompt's own legend ("0.0075 = 0.75%/yr") teaches the model to read as free — and the legend's own example is unrepresentable.
  Resolved 2026-08-27: the role-risk, interpretation, and action prompts render the expense ratio through one shared `fmt_expense_ratio`, never `opt()`.
  It states the decimal fraction at four places — the ledger's unit, one basis point — beside its percent reading (`0.0003 (0.03%/yr)`).
  On those three renders a nonzero ratio that would round to zero extends its precision rather than printing as free.
  The ledger crossing lines keep the series' generic four-place render, so a sub-basis-point ratio would print `0.0000` there while the direct render shows it.
  That edge is reachable: the adapter divides any numeric `expenseRatio` by 100 unquantized, and a ledger threshold is any finite value.
  It is deferred, not impossible: recorded off Codex rounds 1–2, not actioned in this slice.
  Ruled 2026-08-27: it is I12 below, its own slice ahead of the run on I10/I11's terms.
  `PROMPT_VERSION` moves to `portfolio-v14`, so a pre-fix checkpoint cannot resume into the corrected render and every record stamps the render it was authored under (Codex round 1).
  A pinned test holds the 0.03% fund, the legend's own 0.0075, the adapter's percent-over-100 arithmetic, a fee-waived zero, and a sub-basis-point ratio; a second proves all three prompts carry both readings.
- **Ledger vocabulary asserts TTM the engine may not deliver** — the series vocabulary hard-codes "TTM net margin" / "TTM gross margin" (`engine.rs:831-832`), but on the annual fallback basis the model's thresholds are evaluated against annual prints, and no prompt discloses which basis the holding is on; the basis-change streak reset bounds the damage to threshold semantics.
- **IV skew rendered without a sign convention** — the options-activity line prints the signed skew bare (`pipeline.rs:3365-3372`); put-minus-call lives only in a Rust doc comment (`mod.rs:607-608`), so a model assuming the opposite convention reads hedging demand as call speculation.
- **FMP statement dates never canonicalized** — quarterly `period_end` / `filing_date` store raw source text (`fmp.rs:5954`, `5989`) while every downstream consumer is lexicographic (`engine.rs:448-461`, `887-893`); a non-padded date misorders the run, failing TTM adoption (a silent basis drop, and a spurious basis-flip gate) rather than producing a wrong sum.

## Priority 2 — mid-run abort risk

### C1 — critical: the 600-second transport deadline kills legitimately long thinking chains, and the run with them

`DEFAULT_TIMEOUT = 600s` is applied as the reqwest blocking client's timeout (`local_model.rs:47`, `516`) — a **total deadline from connect until the body finishes**, for the non-streaming and streaming paths alike.
The comment (`local_model.rs:44-46`) says it "exists only to cap a stuck daemon, not to bound a healthy generation", but the thinking stages reserve `NUM_PREDICT_THINKING = 65_536` tokens (`pipeline.rs:4557`), and the code's own calibration note cites attempt-1 calls generating 12–15K tokens in 7–8 minutes (~30 tok/s).
At that throughput the transport kills any chain past roughly 18–20K generated tokens — under a third of the reservation the design explicitly budgets ("chains run tens of thousands of tokens").
Every 6c research turn, distill call, and interpret/role-risk/action call rides this client; the error propagates through `analyze_holding` under the hard posture and **fails the whole run**, and a holding that reliably provokes long chains can fail again on every resume (sampling variance is the only mercy).
A side effect: the typed `done_reason: "length"` truncation diagnostics (`ensure_not_output_limited`, `pipeline.rs:4486`) are unreachable for any chain that would take over ten minutes — the transport error preempts exactly the attribution the design built.
Observed healthy generations already run at 70–80% of the deadline, so this is the review's most realistic multi-hour-run killer.

**Resolved 2026-08-26.**
Re-verification against the reqwest 0.12.28 source narrowed the mechanism before the fix.
The blocking client never forwards its timeout to the async client; it applies it as the wait for the response headers, then afresh on every body read.
The streaming callers — interpretation, role-risk, and the action call — were therefore already idle-bounded and never at risk mid-chain.
The exposure was the two non-streaming callers, the 6c research turn and distillation, where the daemon answers only once the whole chain has generated.
The header wait's prompt-evaluation span is exposed on both paths; the pre-flight table shows it passing ten minutes above ~100 K prompt tokens on its own.
The fix derives every chat call's deadline from the request's own reservations: `num_ctx` over a prompt-evaluation floor, plus `num_predict` over a decode floor on the non-streaming path, never under the ten-minute backstop (`DeadlinePolicy`, `local_model.rs`).
A trip is named for what it is — a stalled daemon, throughput under the drafted floor, or pre-generation overhead outrunning the reservation's slack — so it is attributable where the length-stop guard cannot reach.
The contract is recorded at `docs/local-models.md §The local-model adapter seam`, and the floors' re-verification obligation at `docs/local-model-operations.md §M5 pre-flight checklist`.
The accepted cost is that a daemon which genuinely hangs mid-research-turn is detected within the derived deadline (~2 h at the thinking reservation) rather than ten minutes, on a path already blind to cancel for the call's duration.
The streaming stages' idle bound rises the same way, from ten minutes to the prefill term (~22 min at the interpret context), so a cancel during a silent daemon waits that long there too.
Three Codex rounds added the deadline attribution on a stalled non-2xx error body, made the guarantee conditional on the daemon holding the floors and on pre-generation overhead fitting the unused reservation's slack, aligned the trip message with that three-condition contract, and pinned the default roster's distillation deadline (~33 min, since the fallen-back fast tier shares the interpret context).

### Named design risk — zero retry on hundreds of hard-path model calls (documented posture, not a defect)

Any single local-model failure inside the required 6c–6f path — a transient daemon error, a connection reset, an empty completion body (`serde_json::from_str("")` at `pipeline.rs:4779/4797/4821`, `research.rs:1120`), a schema-parse failure (`distill.rs:633`, `711`, `719`), a length stop, or a whitespace action rationale (`ensure_action_rationale`, `pipeline.rs:150-160`, ruled fail-hard 2026-08-18) — fails the whole run on first occurrence.
This is the documented hard posture (`docs/portfolio-analysis.md` §Failure posture), typed, checkpointed, and resumable, and both robustness reviews independently confirmed no bounded retry exists anywhere on the path (retry-once is the known repo-wide deferred item).
It is recorded here because at ~4+ reasoner calls per holding across dozens of holdings over hours, it is the dominant real-world abort probability for the big confirmation run, and C1 multiplies it.

**Resolved 2026-08-27.**
The posture is now hard-after-one-bounded-retry, decided against running the confirmation run bare: a transient class re-attempts exactly once — transport-level connection failures, daemon error statuses, empty completion bodies, schema-parse failures of returned content, and broken streams — while deadline trips, length stops, cancellation, and any unclassified failure keep first-occurrence hard failure.
The retry sits at the call/parse seam, not the transport helper, because every content parse happens above the adapter: the three streaming stages wrap call-through-parse whole (`pipeline.rs`), and the research and distillation loops, which parse above their model traits, re-attempt through a default-closed `retry_permitted` gate method on those traits — one shared gate (`local_model::RetryOnce`) doing the whitelist classification (typed `RetryClass` chain markers, never string-matching), the cancellation refusal, the tracker row, the drafted 2 s abortable pause, and the event record.
An empty completion body, previously dying as an opaque serde EOF at the parse sites, now fails typed through its own guard (`ensure_nonempty_completion`), which tolerates a research turn's tool request.
A second failure fails the run annotated with the first attempt's class, and every fired retry lands as a data-health summary line plus structured `model_retries` events (an attention trigger) — the transient-rate measurement the watch set reads after the run.
The blank-rationale guard keeps its 2026-08-18 fail-hard ruling: it sits at the `analyze_holding` level, outside every retry wrap.
The contract is canonical at `docs/local-models.md §The local-model adapter seam`; `docs/portfolio-analysis.md §Failure posture` points to it, and the watch set carries the fired-retry watch.
A Codex round then hardened the classification: a stalled non-2xx error body on the streaming path keeps its live timeout chain rooted, classifying as the deadline trip it is — never retried — while the message still leads with the status and names the deadline.
The round's other findings were absorbed as scoping rather than machinery: a resumed run's retry read is documented as a floor (checkpoint carriage deliberately stays grouped with the recorded prompt-usage resume minor), the compound call+parse worst case got its bounded regression test, and the logic-flow failure bullet gained the retry pointer.
A second round tightened the compound-turn claim to its true shape: the retry bound is once per **issued** call, the legs composing only through the parse leg's re-issue, and both the four-call worst case and its hard ceiling (a fourth-call failure dies annotated, no fifth call) are regression-tested.

### Priority-2 minor findings

- **The hierarchical reduce prompt is never size-checked** — `reduce_prompt` concatenates all tier-1 outputs, dormant priors, and the disconfirming pass with no check against `input_budget_chars` (`distill.rs:715-717`); on a genuinely distinct fast model (`NUM_CTX_DISTILL = 32_768`) an oversized input silently front-truncates (dropped topics, their seed rows deleted as unreconciled) or length-stops into a run failure.
  The default fast-falls-back-to-reasoner roster's 131,072 context is why it has not bitten.
- **Resume loses pre-crash prompt usage** — `CheckpointAccumulators` carries no prompt-usage field (`store.rs:191-199`), so a resumed run's data-health context-pressure / peak-prompt / length-stop lines reflect only post-resume holdings — under-reporting the exact signal the big-run prompt-fit watch reads.
- **No panic containment** — there is no `catch_unwind` around `run_analysis`, so any panic skips both `record_run(Failed)` and the terminal `run_finished` event; the run slot itself frees (poison-tolerant `RunGuard::lock`, `jobs.rs:132-173`), but the tracker never reaches a terminal state and the checkpoint trail is unofferable for that session.
  No realistic panic path was found in the spine files themselves — the exposure is the compute modules below.
- **Three contrived-trigger panic paths, all whole-job kill radius given the missing containment** — the monotonicity-repair sort panics on a NaN scenario price (`engine.rs:1680`), reachable only through the fund flat-driver chain (`fund.rs:838`, `848` — composite yield lacks a finiteness/zero guard; `composite_yield` guards only `covered <= 0.0`); `percentile`'s sort panics on NaN and an inf sample yields a NaN result that feeds the first site (`engine.rs:1578`, `1586`; all live callers guard emptiness); an absurd feed-authored `period_end` near chrono's date ceiling panics on the +45-day grace add (`engine.rs:2227`).
  All require pathological feed values; recorded because containment, not probability, is the gap.
- **IPv6-literal fetch URLs can never fetch** — `resolve_public` passes `host_str()`'s bracketed IPv6 form to `to_socket_addrs()` unparsed (`fetch.rs:153-164`; `check_url_policy` at `:205` knows to trim the brackets), so every IPv6-literal fetch errors and spends a budget unit; fails closed, so the SSRF direction is safe.

## Priority 3 — logic-flow doc alignment

### A1 — major: the Step-6c failure paragraph says no partial work persists

Doc line 1056: a hard model-call failure "fails the run, and as-built no partial work persists".
The code checkpoints every completed holding (`store.rs:237-260`) and the resume path consumes the trail; the canonical `docs/portfolio-analysis.md` §Failure posture says "no partial *run row*" while completed holdings live in the checkpoint trail, and the logic-flow doc's own Step-6 preamble (lines 413–414) states per-holding checkpointing correctly.
As written, line 1056 tells the user a mid-run failure costs hours of work that the design specifically preserves.

**Resolved 2026-08-27.**
Line 1056 now states the canonical posture: the failed run persists no partial run row, while every holding whose checkpoint landed and reads back at resume is restored from the trail.
It points at the Step-6 preamble, the Resume behavior bullet, and `docs/portfolio-analysis.md` §Failure posture.
A Codex round caught the first cut restating the trail as a guarantee: the header and per-holding checkpoint writes are fail-soft (`job.rs`, the run continuing unprotected on a failed write), which no doc had stated, so `docs/portfolio-analysis.md` §Failure posture now carries that posture and both statements read against checkpointed holdings.
A second round added the read-back leg: a row the resume loader cannot parse is skipped and re-analyzed (`store.rs:297`).
A third round aligned the logic-flow line to this formulation.

### A2 — major: the interpretation call's "exact inputs" list omits whole rendered sections

Doc lines 1185–1245 enumerate the interpretation prompt's inputs, but the code renders, into the same call: the forensic filings state with the hard-rule text (`pipeline.rs:3381`, `3153-3190`), the CBOE venue-level put/call backdrop (`3373`, `2915-2930`), the commodity context (`3382`, `3118-3145`), the technology-event pre-flag section (`3417-3430`), the semantic prior-analysis recall block (`3457`, `2135-2147`), and — in the fund context the doc reduces to "expense ratio, US share, and composite P/E coverage" — the CFTC COT positioning block and the closed-end price-vs-NAV line (`3266`, `2885-2909`, `3257-3265`).
Every input the doc does list is present, and both of its claimed absences hold: the investor profile is not rendered (`pipeline.rs:3405-3408`) and no engine stand-in conviction/outlook/action appears.

**Resolved 2026-08-27.**
The interpretation list gained the sections the code renders: the forensic filings state in its three forms with the hard-rule text, the CBOE venue-level backdrop, the commodity context, the technology-event pre-flag, and — under the continuity block — the semantic prior-analysis recall and the rendered input delta with its what-changed-entry rules.
The fund context now names the closed-end price-vs-NAV line (an explicit gap line on the priced-fund branch) and the CFTC COT positioning block, and the prior-ledger bullet names the research-supported mark the F3 channel renders.
A new bullet enumerates the role-risk branch's own render set and what that branch omits, since the list had described only the priced branch.
Re-verification at `ce24895` found the render set had grown past the finding's six omissions (`interpretation_user_prompt`, `pipeline.rs:3429-3713`; `role_risk_user_prompt`, `2850-2930`); everything the doc listed before was present, and both claimed absences still hold.
A Codex round narrowed the twice-rendered note to the latest-report sections, since a house view reduced to the recent-stance list renders without a delta row (`pipeline.rs:2414`).
A second round bounded it to a prior verdict, since a debut renders no delta rows at all (`pipeline.rs:2439`).

### A3 — major: the action call's "exact inputs" list has the same shape

Doc lines 1286–1293 end at the engine action set and the profile, but the action prompt also renders the forensic filings section and the commodity context (`pipeline.rs:3689-3690`), plus the CEF NAV-premium line on the role-risk digest (`3675-3679`).
Everything the doc does enumerate was verified present, including the withheld engine pick and the profile without the cash row.

**Resolved 2026-08-27.**
The action list gained the forensic filings state and the commodity context — rendered for both digests when the dossier carries them, while the role-risk interpretation call renders neither — plus the same-stock option overlay and the closed-end price-vs-NAV line on the role-risk digest (`action_user_prompt`, `pipeline.rs:3785-3954`).

### A4 — major: the sub-distillation drop trigger is misdescribed

Doc line 1092 says passes drop "if even the tree-level reduce would overflow".
No sizing check on the reduce exists; the actual trigger is the per-holding budget `SUB_DISTILLATION_CAP = 4` pass-level sub-distillation calls shared across overflowing topics (`distill.rs:55`, `651-669`), and since `MAX_PASSES_PER_TOPIC = 3` a single overflowing topic can never hit it alone — passes drop only when a second overflowing topic finds the budget partly spent.
The surrounding claims (drops take findings and ledger entries, never the prior) match the code.

**Resolved 2026-08-27.**
The logic-flow bullet now describes the as-built trigger: the tree-level reduce is not sized, the cap is a per-holding budget of four pass-level calls shared across overflowing topics in agenda order, the lowest-priority whole passes drop when a topic's passes exceed what remains, and a lone overflowing topic never drops a pass under the three-pass ceiling.
The canonical `docs/portfolio-analysis.md` §Starting parameters carried the same reduce-overflow wording and was corrected alongside it.
Re-verification found one claim this finding had passed as matching that does not hold in the exhausted-budget edge: a topic whose every pass drops yields no tier-1 object, is named unreconciled at the reduce, and loses its stored seed (`distill.rs:806-846`, `1080-1094`; `job.rs:1889`), so in that edge alone an overflow does cost the topic's seeded status.
Both docs now state that edge as built.
Restoring the invariant — rendering a fully-dropped topic's retained prior into the reduce the way dormant priors ride it — is a named code change; the edge needs three overflowing topics in one holding to reach.
It is queued behind the big confirmation run, whose watch set reads the run's gaps for the cap; a hit promotes it.
Under the 2026-08-27 run ruling (§Disposition) the fix moves ahead of the run with the rest of the record; the watch-set line stays, reading the run for whether the edge is reached.
`logic-flow-docs/trade-opportunities-logic-flow.md` described the shared primitive with the same reduce-overflow wording and was corrected alongside, in the budget terms its own Bounds-and-audit bullet already used; the seed edge is built Portfolio behavior and was not ported to the unbuilt job.
A Codex round then disambiguated what the cap counts in the canonical siblings: `web-research.md` §The research loop and context management and `configuration.md` §Local Analysis Suite Configuration now say only the per-unit map calls count, never the reduce or an ordinary tier-1 call.
A second round noted the unreconciled-row delete is itself fail-soft and logged only (`job.rs:1889`), so the canonical doc now states the cold re-seed as the rule rather than a guarantee.
A third round moved the logic-flow mirror to pointer form — the rule plus a pointer to that posture — rather than restating the mechanics; the watch-set line keeps the rule, since a failed delete is a database anomaly outside what that watch reads.

### Priority-3 minor findings

- **Research-fed fraud listed under "Hard forensic state (live)"** — doc lines 863–865 place "fraud may arrive later from validated primary-source research" inside the hard-state bullet, but the 2026-08-24 ruling made the research-fed claim advisory-only — it never merges into the hard producer state (`pipeline.rs:351-365`), which gates the add family.
- **"A qualifying news seed" reads as an independent tech-topic trigger** — doc line 978; the code requires the conjunction with a standing technology-class ledger falsifier (`pipeline.rs:1002-1003`; same conjunction in the quick check, `quick_check.rs:82-84`), so fresh news alone never fires the deep-dive.
- **The narrative read's 7-day minimum is undocumented** — the doc names only the debut as carrying no read (lines 578, 852–857), but `NARRATIVE_MIN_ELAPSED_DAYS = 7` (`engine.rs:3128`) gaps the read for anyone running more than once a week.
- **"(pre-profit stocks only)" vs computed-for-every-stock** — the Step-6b order list's parenthetical (line 569) contradicts the doc's own lines 596/651 and the code (`pipeline.rs:863-873`): the overlay record is computed and persisted for every priced stock.
- **"One isolated conversation per agenda topic"** — doc lines 1000–1001; as-built each *pass* is its own fresh conversation (`research.rs:1067-1090`), with only the claims ledger and findings carrying across a topic's passes — which the doc's own "who owns the context" bullets state correctly.
- **"The thresholds are config knobs"** — doc line 1076; `OVERFLOW_THRESHOLD`, `CHARS_PER_TOKEN` (`distill.rs:48-51`) and `NUM_CTX_DISTILL` (`pipeline.rs:4535`) are compile-time constants exposed in no settings surface.
- **Supersede validation legs overstated** — doc line 1155 claims metric/units/period match legs and that "a supersede always rejects"; the code has no match legs, and with an absent feed value a supersede-declared claim does not reject — it falls through and fills exactly like a supplement (`engine.rs:2039-2057`; the F-minor loss-displacement finding rides the same guard).
  Ruled 2026-08-27: the supersede leg is dormant by design.
  The doc moves to the as-built rule — a supersede rejects against any present feed value, and a supersede declared against an absent value is downgraded to a supplement fill, which `matched_rule` will name — and the match legs leave the doc.
  The true leg is revivable only if the channel is promoted and the consensus feed gains an as-of date.
  One slice: the doc plus the label.
- **"An accepted forward assumption" as what-changed evidence** — doc line 1327; the delta row is pushed whenever distillation validated an assumption, regardless of its Step-6e shadow resolution — a `rejected:` resolution still anchors an external what-changed row (`pipeline.rs:1222-1230`).

## What was verified correct

Coverage matters as much as findings for a pre-run record; the following were traced and found sound.

- **Engine core** — statement canonicalization (restatement dedup keeps the latest filing), TTM all-or-nothing sums, every sub-score map with both off-scale guards (negative P/E never "cheap", negative D/E never "clean"), grade weights/cutoffs/imputation, the anchor join with filing-date (+45d grace) alignment, the inverse spread→scenario map and its degenerate/monotonicity/dispersion guards, the driver ladder's admissibility and clamps, the v4 trough-release gates, implied expectations round-tripping the shared multiple derivation, re-anchor arithmetic identical to the live pass, risk tiers, the hurdle read's comparison directions, `narrative_vs_reality`'s two forms, the √t-scaled tech pre-flag, options-signal composition, and the full ledger evaluation surface (comparator directions, margins, observation identities, streaks, ack re-raise, basis-flip gate).
- **Fund / pre-profit / outcome** — CEF premium sign and gap-honesty, classification routing, the exchange-blend and constant-mix history arithmetic, the fund-form v2 targets, runway/burn sign conventions, dilution and margin-trajectory reads, `clamp_conviction` as a pure ceiling, observation validation's corroboration rules, the Winkler interval scorer, split-safe return bridging, dividend windowing, lead-time signs, no-lookahead window gating, cohort composition, and the episode lifecycle.
- **Dossier / quick check / diff** — TTM basis adoption and the choke-point's coverage, flow-vs-instant discipline, price-window parity across paths, quick-vs-deep formula parity (no fork), tripwire and band boundary semantics, holdings netting signs (including short semantics and the cash routing), the OCC option-overlay decode, and short-interest passthrough.
- **Prompts** — the two-arm framing, sub-score directions, hurdle/tier renders, target provenance, the retrospective block's split-safe math, the what-changed instruction matching its validator exactly, the pre-profit overlay units, the forensic sections matching the 2026-08-24 rulings, tunnel-vision discipline in the action prompt (no book inputs, engine pick withheld), profile isolation from the intrinsic call, and the 6c prompt suite's seed/citation framing.
- **Robustness** — run-slot release on success/error/panic, the checkpoint/resume cycle (atomic per-holding writes, no skip or double-process, version-stamp refusals, the newer-run eligibility check), cooperative cancellation at every boundary, single-transaction persistence with WAL + busy timeout, fail-soft routing of every enriching feed, web search/fetch failures degrading to tool notes, the SSRF guard (pinned DNS, full special-range table, body caps, re-checked cache reads), guaranteed budget-loop termination against an injectable clock, multi-byte-safe text handling, grammar-constrained local-model decoding with lenient wire structs, and the panic-freedom of the spine files' own unwrap/index/arithmetic sites.
- **Alignment** — every constant the logic-flow doc states matches the code (grade weights and cutoffs, band parameters, tier thresholds, research budgets and freshness windows, retention caps, the resume window, outcome windows and grace), and all twelve "most important safety rules" hold, including the four 2026-08-24-adjacent ones (model arm never binds the baseline, shadow-only forward assumption, advisory fraud, driver-id-gated indicator anchor) and the code-enforced no-order Schwab boundary.
- **Audit provenance** — the distillation shape and tier count reach the persisted audit as `configuration.md` §Local Analysis Suite Configuration states: `DistilledResearch.shape` rides `ResearchAuditRecord.shape` (`pipeline.rs:332`) into `HoldingAudit.research` (`mod.rs:1713`), serde-persisted with the run's audit rows (`store.rs:205`), the `Hierarchical` variant carrying `tier1_calls`, `subdistilled_topics`, and `dropped_passes` (traced 2026-08-27).

## Disposition

Every fix is a separate, undecided piece of work; nothing here was applied.
Four items bear directly on the queued big confirmation run and are worth deciding before it: C1 (a realistic multi-hour-run killer whose fix shape is a transport budget consistent with the thinking reservation, or an idle-based read timeout), the retry-posture weighing beside it, F1 (a split during the watch window would contaminate the run's ledger evidence), and F3 (the run will exercise the research loop whose ledger channel is silently dead, and its watch-set typed-channel yield line will read zero qualitative support by construction).
The alignment findings are doc edits; A1 in particular misinforms the user about the cost of the failure mode C1 makes likelier.
C1 is resolved (2026-08-26; the resolution is recorded under §C1).
F3 is resolved (2026-08-26; the resolution is recorded under §F3).
F1 is resolved (2026-08-27; the resolution is recorded under §F1).
The retry posture is resolved (2026-08-27; the resolution is recorded under §Named design risk) — the pre-run list is complete.
F2 is resolved (2026-08-27; the resolution is recorded under §F2).
A1–A4 are resolved (2026-08-27; each resolution is recorded under its §A heading, the §A4 one naming an exhausted-budget edge left open).
Ruled 2026-08-27: the big confirmation run waits on this whole record — every remaining finding (the Priority-1, -2, and -3 minors, Codex's I1–I9, and the §A4 exhausted-budget seed edge) is handled first, and the user names the session that launches it.
I10 and I11, added 2026-08-27 from the fix-slice Codex rounds, join that queue on the same terms.
I12, added 2026-08-27 off the expense-ratio slice's Codex rounds, joins on the same terms.
Fix grouping ruled 2026-08-27: one finding per slice through the plan → implement → review → Codex → commit loop, each marked here; the resume prompt-usage minor is the one-seam exception, its retry events and prompt usage riding `CheckpointAccumulators` together as a single slice.
Docs register ruled 2026-08-27, off the A1–A4 Codex rounds: a mirror states a store rule as written — "persists", "is deleted" — and the fail-soft posture of each write lives once in the job's canonical §Failure posture, mirrors carrying at most a pointer; the standing rule is `CLAUDE.md` §Docs formatting.

## Codex independent review additions

### Method and de-duplication

This review was completed independently against current `main` `4dc675b` before the Claude Code section above was opened.
The only commits after Claude Code's reviewed `457efe1` are Metis / watch-set documentation commits; the production portfolio code cited below is unchanged between those revisions.
After the independent findings were fixed, they were compared finding-by-finding against Claude Code's report and every duplicate was removed.
The fund-weight NaN finding below is intentionally retained despite touching Claude Code's generic NaN-panic note because it identifies the concrete production adapter input, the false-valid coverage transition, and the exact whole-job panic path that the earlier note did not establish.

The full repository gates were green on `4dc675b`: `cargo test` (1,185 passed, 31 ignored, plus all integration suites), `cargo clippy --all-targets --all-features`, `npm run build`, `npm test` (46 Node tests and 247 Vitest tests), and `git diff --check`.
The green gates do not cover the adversarial boundaries below.
I10 and I11 (2026-08-27) come from Codex's review rounds on the fix slices rather than the independent review, and join the queue ahead of the run on the same terms as I1–I9.
I12 (2026-08-27) is the expense-ratio slice's deferred crossing-render edge, given its own heading by ruling and queued on the same terms.

### I1 — major: a non-positive quote passes the evidence floor and can make a successful run persist as unreadable

The FMP quote adapter accepts any JSON number as `current_price`, including zero or a negative value (`src-tauri/src/fmp.rs:1825-1834`), and the stock evidence floor checks only `Some(price)` (`src-tauri/src/portfolio/engine.rs:1240-1275`).
The fund path has the same hole and makes it worse: `fin.current_price.or(fund.nav)` lets `Some(0.0)` or a negative quote mask a usable positive NAV (`src-tauri/src/portfolio/fund.rs:663-673`).

Downstream target arithmetic divides by spot in the scenario total returns and one-month target builder (`src-tauri/src/portfolio/engine.rs:1702`, `2560-2561`).
A zero quote therefore creates infinities and NaNs; a negative quote creates finite but financially meaningless returns and targets.
The final run serializer writes non-finite `f64` values as JSON `null`, so `insert_run` can report success (`src-tauri/src/portfolio/store.rs:655-666`) while later `PortfolioRun` decoding rejects the blob and the history lists it as unreadable (`src-tauri/src/portfolio/store.rs:763-804`).
The same poison can enter the checkpoint blob before final persistence.

The price / NAV evidence floor must require a finite, strictly positive value before any target, hurdle, checkpoint, or run record is built, and the logic-flow description should call this a usable finite-positive quote rather than mere presence.

### I2 — major: one stale fund P/E print can fabricate all twelve quarterly history samples

`composite_yield_history` samples twelve target quarter ends, but for each target it selects the latest sector print on or before that date with no maximum age and no requirement that the selected print belongs to that quarter (`src-tauri/src/portfolio/fund.rs:456-498`).
The same old print can therefore be reused at every target date and counted as twelve independent samples.
The production fetch intentionally requests about 1,600 days (`src-tauri/src/portfolio/job.rs:525-543`), so a sparse response with one old row is sufficient to reach this path.

The adapter intensifies the issue by converting a historical P/E row with no date to `""`, which compares before every ISO sample date and is consequently eligible for every quarter (`src-tauri/src/fmp.rs:6340-6372`).
The fund evidence floor then checks only `history.len() >= 8` (`src-tauri/src/portfolio/fund.rs:786-803`), and the fabricated series also becomes the target anchor history (`src-tauri/src/portfolio/fund.rs:835-850`).
A single unchanged print can thus produce a 0-or-100 valuation percentile, pass the claimed eight-observation floor, and supply twelve fake target anchors.

This contradicts the logic-flow contract's “~8–12 quarters” and “≥ 8 history samples” language (`logic-flow-docs/portfolio-analysis-logic-flow.md:779-786`).
Each sample needs a bounded-distance observation from its intended quarter, missing dates must be rejected, and the evidence floor must count distinct source observations / quarters rather than generated sample dates.

### I3 — major: pre-profit source corroboration can validate the wrong sign and an unrelated number

`value_in_text` rejects adjacent digits and decimal continuations but does not treat a preceding sign as part of the number (`src-tauri/src/portfolio/pre_profit.rs:816-858`).
As a result, a candidate `+41` corroborates against page text containing `-41`; the test suite also explicitly accepts positive `41` against `(41)` (`src-tauri/src/portfolio/pre_profit.rs:1683-1698`), although parentheses are standard accounting-negative notation.

The broader validator requires only that the page mention the issuer somewhere and contain the numeric string somewhere (`src-tauri/src/portfolio/pre_profit.rs:870-902`).
It never binds that occurrence to the declared metric, units, period, issuer scope, observation role, or nearby text.
A long earnings release can therefore validate a model-authored deliveries row with a revenue number, a current actual with a prior-period number, or a positive unit-economics fact with a negative printed value.

Accepted rows drive the deterministic 5% / 20% guidance-miss rules and can cap engine conviction or restrict its action set, so this is not merely a citation-display weakness.
It contradicts the logic-flow promise to “confirm the source states the number” in the sense required for a typed financial observation (`logic-flow-docs/portfolio-analysis-logic-flow.md:1158-1161`).
Corroboration must be sign-aware and must validate the number in metric / units / period context, or the typed row must carry a source excerpt / locator that the app can verify.

### I4 — major: guidance attainment has no ex-ante chronology or deterministic revision-selection policy

`execution_read` discards `published_at` and confidence from guidance rows, retains only `(value, is_range_low)`, and keeps the first same-role bound encountered except that any range low displaces point guidance (`src-tauri/src/portfolio/pre_profit.rs:1038-1070`).
Actuals, by contrast, are deliberately selected by highest confidence and then latest publication (`src-tauri/src/portfolio/pre_profit.rs:1072-1086`).
Pairing subsequently matches only metric identity and reporting period (`src-tauri/src/portfolio/pre_profit.rs:1092-1105`).

Nothing requires guidance to have been published before the actual, before the period end, or before the actual's publication.
An actual-results release that retrospectively repeats the period's guidance can therefore supply both sides of its own attainment test, and multiple legitimate guidance revisions are selected by model / persistence order rather than a stated financial rule such as original guidance or latest pre-actual guidance.
Either choice can flip the 5% miss, 20% material-miss, repeated-miss, conviction, and action-set results.

The code and logic-flow document need one explicit guidance-vintage policy, and the pairing key must enforce it with the already-persisted publication dates.

### I5 — major: the action decision never receives the model arm's price targets

The action system prompt tells the model to weigh both arms' grades and scores and “the targets' implied upside/downside” (`src-tauri/src/portfolio/pipeline.rs:3508-3522`).
The user prompt renders the model arm's letter and sub-scores only (`src-tauri/src/portfolio/pipeline.rs:3601-3612`), then renders implied moves exclusively from `graded.price_targets`, explicitly labelled as engine targets (`src-tauri/src/portfolio/pipeline.rs:3620-3633`).
It never reads `graded.model_view.price_targets` on the action path.

This means the model can author a materially different forward target band in the intrinsic call, but its own subsequent action call cannot use that numeric forecast; only its derived letter survives into the decision.
The omission conflicts with the canonical statement that price targets exist in both arms and that target-implied upside / downside is in the action evidence set (`docs/portfolio-analysis.md:320-333`; `docs/portfolio-workflow.md:353-355`).
The logic-flow “exact inputs” list mirrors the implementation's ambiguity by naming one implied band after listing both arms (`logic-flow-docs/portfolio-analysis-logic-flow.md:1286-1291`) and should identify the arm or require both.

### I6 — major: the declared model-arm numeric domains are prompt-only, so out-of-domain values derive letters and enter scoring

The interpretation prompt requires model sub-scores on a 0–100 scale and positive ordered target bands (`src-tauri/src/portfolio/pipeline.rs:3352-3362`).
The schema enforces only `number`, with no range keywords, and the code documents that limitation (`src-tauri/src/portfolio/mod.rs:2256-2312`).
There is no app-side post-validation before the values are persisted and the letter is derived (`src-tauri/src/portfolio/pipeline.rs:1305-1312`; `src-tauri/src/portfolio/engine.rs:1324-1341`).

A finite score such as `10000` or `-1000` becomes an ordinary A or F, while zero, negative, or severely crossed targets persist and enter the outcome scorer (`src-tauri/src/portfolio/outcome.rs:2196-2212`).
The engine baseline remains isolated, but the user-visible model arm, action evidence, retrospective, and calibration population are no longer on the declared financial scale.

The documentation calls this “structurally validated only” while also claiming a shared 0–100 scale and “comparability by construction” (`docs/portfolio-analysis.md:286-295`; `docs/portfolio-workflow.md:334-339`).
Unrestricted judgment does not require accepting values outside the declared domain; app-side finite / range / positivity validation can preserve arm independence without clamping the model to engine outputs.

### I7 — minor but whole-job kill radius: the fund-weight adapter admits string NaN into the percentile panic

Claude Code's generic panic note correctly identified that NaN can reach `percentile`, but the production ingress is not merely hypothetical arithmetic.
`weights_from_value` accepts string weights because FMP normally serves strings such as `"97%"`; Rust's `parse::<f64>()` also accepts `"NaN"` / `"NaN%"`, and the adapter performs no finiteness or range check (`src-tauri/src/fmp.rs:6311-6337`).
The shaped fetch treats any non-empty parsed vector as successful (`src-tauri/src/fmp.rs:5644-5675`).

A NaN sector weight makes `covered` and the composite yield NaN, bypasses `covered <= 0.0`, and can surface as apparently full coverage through `covered.min(1.0)` (`src-tauri/src/portfolio/fund.rs:422-447`).
The history then supplies NaN raw multiples to `percentile`, whose `partial_cmp(...).expect("finite percentile inputs")` panics (`src-tauri/src/portfolio/engine.rs:1575-1631`).
Because the job has no panic containment, one drifted string weight can terminate the entire hours-long run without its normal failed terminal state or resumable in-session offer.

Reject every non-finite or out-of-range weight in the adapter and defensively validate the composite / observation vectors before sorting.

### I8 — minor: the priced-fund prompt computes US share differently from the engine guard

The engine sums all recognized aliases — `united states`, `united states of america`, `usa`, `u.s.`, and `us` — and caps the aggregate (`src-tauri/src/portfolio/fund.rs:367-382`).
The interpretation prompt instead takes only the first country label containing `"united states"` (`src-tauri/src/portfolio/pipeline.rs:3230-3244`).
The production adapter's own test demonstrates a valid `"US"` row (`src-tauri/src/fmp.rs:2480-2533`), so a fund can pass the deterministic ≥70% US guard at 97% while the model prompt says `US share: (gap)`.
Multiple US-alias rows are likewise summed by the engine but truncated to the first match by the prompt.

This is a direct mismatch with the logic-flow claim that the fund's US country-weight share is rendered (`logic-flow-docs/portfolio-analysis-logic-flow.md:795-800`).
The prompt should consume the already-computed classified share or a shared helper, not reimplement the label policy.

### I9 — minor: the fund-specific sector-P/E adapter bypasses the suite's established integrity guards

The main sector-P/E adapter rejects an off-board response and drops non-positive or greater-than-100 aggregate P/Es because near-zero earnings denominators have produced live values around 461 (`src-tauri/src/fmp.rs:649-677`, `709-715`).
The fund-specific snapshot / history adapter accepts a supplied exchange that disagrees with the requested exchange, accepts a missing date, and accepts any parseable P/E; it merely echoes the requested exchange when the field is absent (`src-tauri/src/fmp.rs:6340-6374`).
Only non-positive and non-finite P/Es are later excluded by the blend (`src-tauri/src/portfolio/fund.rs:387-407`), so implausibly high values and duplicate off-board rows can influence the fund valuation and target anchors.

The fund path should share the same exchange-identity, date-readability, and plausible-P/E contract as the suite's sibling adapter; otherwise “usable P/E” means materially different things across the same job without the logic-flow document saying so.

### I10 — minor: the one-month target's methodology never reaches the model or the UI

The engine authors a methodology line for both horizons (`engine.rs::build_price_targets`), and `portfolio-analysis.md` §The holding verdict specifies the engine's scenario outputs "with their methodology and assumptions exposed" for both.
The interpretation prompt renders the twelve-month targets with their methodology but the one-month targets as bare numbers (`src-tauri/src/portfolio/pipeline.rs:3522-3533`).
The Portfolio page's "Target methodology" reveal renders only `twelve_month.methodology` (`src/components/PortfolioView.vue:2001-2006`), and the component spec's default fixture carries `one_month: null`, so it cannot catch the omission (`tests/components/PortfolioView.spec.ts:44`).
Surfaced by the Codex round on the `targets-v5` slice, which changed the one-month band's basis — neither the model nor the reader can see which basis a one-month band stands on.
Pre-existing rather than introduced by that slice; it is its own slice (prompt, view, and spec).

### I11 — minor: a scenario-target parameter-version change has no cross-run continuity attribution

The grade-band version has one: the dossier loads the prior audit's `grade_parameter_version` (`src-tauri/src/portfolio/dossier.rs:383`, `:1133`), the input delta carries a "grade bands recalibrated" row (`src-tauri/src/portfolio/pipeline.rs:2547`), and the continuity prompt adds the recalibration NOTE on a mismatch (`pipeline.rs:3679-3688`).
The target version has none: the loader discards `audit.target_meta.parameter_version`, the input delta compares only the twelve-month base target, and no NOTE fires.
A run whose targets moved on a version bump alone — `targets-v4` → `targets-v5` widens every one-month band with no input change — can therefore have that move attributed to company evidence or a self-correction, and a self-correction marks `thesis_changed` and can open a successor outcome episode.
Pre-existing since `targets-v3` → `targets-v4` (2026-08-13); surfaced by the Codex round on the `targets-v5` slice.
The fix mirrors the grade mechanism — a `prior_target_parameter_version` on the dossier, an input-delta row, and the NOTE — as its own slice.

### I12 — minor: the ledger crossing renders flatten a sub-basis-point expense ratio, and the two sites disagree on the threshold's precision

Both crossing renders print the observed value at four places — the input-delta entry (`src-tauri/src/portfolio/pipeline.rs:2424`) and the 6f ENGINE CONDITION CROSSINGS section (`pipeline.rs:4308`) — so an expense ratio below one basis point prints `0.0000` there while the direct render (`fmt_expense_ratio`, `pipeline.rs:3787`) extends its precision.
The edge is reachable: the adapter divides any numeric `expenseRatio` by 100 unquantized (`src-tauri/src/fmp.rs:6297-6300`), and a ledger threshold is any finite value (`pipeline.rs:1548`).
The two sites also disagree on the threshold: the input-delta entry prints it at four places and the 6f section shortest-round-trip (`{}`), so one crossing states its threshold two ways in one prompt.
`ConditionCrossing` carries no series, so the fix is series-agnostic — one shared formatter at both sites, four places extending where a nonzero value would round to zero, the expense-ratio render's own rule — and a `PROMPT_VERSION` event.
Surfaced by Codex rounds 1–2 on the `portfolio-v14` expense-ratio slice and recorded there as deferred; ruled its own slice 2026-08-27.
