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
  Resolved 2026-08-27: the statement-derived labels name no basis — `net margin (decimal)` / `gross margin (decimal)`, the COMPUTED METRICS block's own words.
  The ledger section of both the interpretation and the role-risk prompt states the holding's statement basis this run — TTM, SEC annual, or none — beside the vocabulary, so a flow-series threshold is authored on the basis it is evaluated against.
  The flow family the line names is read off `LedgerSeries::flow_basis` and the instants off the gate's `statement_derived` less it — never a second list.
  Codex round 1 (2026-08-28): the line had presented the gate's whole family as basis-homogeneous, telling the model to author debt/equity and price/book thresholds on a flow basis those instants do not have; it now names them as instants, the gate's family unchanged.
  Codex round 1 also caught the `Annual` stamp claiming SEC provenance: the merge stamped `Annual` for any level present, so FMP's own balance-sheet instant beside thin quarters and no SEC facts carried a `SEC annual` label for a level SEC never supplied.
  Codex round 2 (2026-08-28) closed the remaining boundary: an equity-only SEC fill had still stamped `Annual`, so the prompt called flows that never reached the engine "SEC annual"; `Annual` is now stamped only where SEC filled a flow line, and an instant standing alone — FMP's, or SEC's equity — carries no flow basis.
  Equity-source continuity — a debt/equity or price/book step when the equity leg moves between FMP's quarterly instant and SEC's annual one — is not the flow-basis stamp's and never fully was, since an FMP balance-sheet gap flips the source under an unchanged TTM basis; recorded off Codex round 2, not actioned, a separate stamp if it is ever wanted.
  Ruled 2026-08-28: it is I13 below, its own slice ahead of the run on I10/I11's terms.
  Codex round 3 (2026-08-28, approve): wording — `None` is no adopted flow basis rather than no statement lines, and only flow-series thresholds are authored on the basis; swept through the sources comment, the canonical bullet and its pointer, the logic-flow, and this record.
  Codex round 4 (2026-08-28, approve): the `portfolio-v15` history paragraph had missed that sweep; aligned.
  The evaluation's basis-change note reads the same labels; its literal had carried thirty-space runs into the prompt (a `\`-continued string collapsed without its continuations), surfaced by this slice's plan and fixed with it.
  Folded under the one-seam exception (ruled 2026-08-27, the same fact — where the holding's basis is disclosed): the audit's sources line now records the annual basis too, `SEC annual statement basis (latest full-year lines)` beside the TTM label.
  It had recorded TTM adoption alone, against `portfolio-analysis.md` §Starting parameters' claim that the adopted basis is recorded.
  `PROMPT_VERSION` moves to `portfolio-v15`; no grade or target parameter moves.
  Pinned: no statement-derived label asserts a basis.
  The section renders each basis and the none case on both branches, the flow family is pinned to its five members and the instants to their two, and the interpretation prompt carries the dossier's stamped basis.
  The sources line names TTM, annual, or nothing; an instant standing alone — FMP's, or an equity-only SEC fill — carries no basis and no label, while a single SEC flow line is annual.
- **IV skew rendered without a sign convention** — the options-activity line prints the signed skew bare (`pipeline.rs:3365-3372`); put-minus-call lives only in a Rust doc comment (`mod.rs:607-608`), so a model assuming the opposite convention reads hedging demand as call speculation.
  Ruled 2026-08-28: the convention text names the as-built method (chain-wide mean put IV minus mean call IV).
  The unit phrase rides with it.
  The text renders on a gap too.
  The Portfolio card's row folds in as the same finding's second surface.
  Resolved 2026-08-28: the interpretation prompt's options-activity line renders the skew signed through `fmt_iv_skew` (`pipeline.rs`, the `OPTIONS ACTIVITY` render) and states its convention on the line — chain-wide mean put IV minus mean call IV, in IV's decimal unit; positive = puts richer (hedging demand), negative = calls richer (call speculation) — beside a `(gap)` too.
  The sign keys on the rendered three-place value, so a skew that rounds away prints `0.000`, never `+0.000`.
  The card's row moved with it: its label reads `Put − call IV skew` and its value routes through `fmtSignedPct`, where a `+` keyed on the raw fraction had rendered a 0.0003 skew as `+0.0%`.
  `PROMPT_VERSION` moves to `portfolio-v16`; no grade or target parameter moves.
  Pinned: the formatter's sign, rounded-zero, and gap cases; the convention text on the value, negative, and gap renders; and the card's label, signed value, unsigned zero, and hidden-on-null row.
  The recorded `pipeline.rs:3365-3372` anchor had drifted to the render's current lines; `mod.rs:607-608` holds.
  Codex round 1 (2026-08-28, approve): no findings.
- **FMP statement dates never canonicalized** — quarterly `period_end` / `filing_date` store raw source text (`fmp.rs:5954`, `5989`) while every downstream consumer is lexicographic (`engine.rs:448-461`, `887-893`); a non-padded date misorders the run, failing TTM adoption (a silent basis drop, and a spurious basis-flip gate) rather than producing a wrong sum.
  Ruled 2026-08-28: an undatable statement date is an unreadable row, dropped, with an all-unreadable body reading malformed through the fetch layer's existing branch.
  A partially unreadable statement response keeps the silent-drop posture of the row-dropping shapers (the EOD closes, the news dates, the per-symbol earnings rows).
  The dividend windower rejects the whole body instead.
  Surfacing the dropped count on the `ok` tracker row is a follow-up candidate, not built.
  The adapter's date contract gains a canonical home at `data-sources.md` §Financial Modeling Prep, the statement-basis bullet carrying a pointer.
  `ConsensusEstimate.period_end` (kept as served since the 2026-08-05 fix, which orders on parsed dates and never compares it) and the report-side `EarningsEvent.date` stay untouched, outside this finding.
  Resolved 2026-08-28: both quarterly shapers store `period_end` and `filing_date` as the canonical fixed-width render through `canonical_date` (`fmp.rs`, `quarterly_income_from_value` / `quarterly_cash_flow_from_value`).
  A `filingDate` that does not parse falls through to the legacy `fillingDate` spelling, and to `None` when neither parses.
  No stamp moves: no prompt content, grade band, or stored-target basis changes.
  Statement-date padding has no live observation recorded either way — the 2026-07-16 live check pinned spellings and row count, and the 2026-08-05 non-padded observation was the estimates and dividend rows — so the fix closes the feed family's documented hazard rather than an observed misorder.
  Pinned: the canonical render on both shapers, the datable-string-first filing-date fallthrough, the unreadable-row drop, the all-unreadable malformed outcome, and four mixed-padding quarters adopting the TTM basis in calendar order end to end.
  The recorded `engine.rs:887-893` anchor has drifted onto the `LedgerSeries` cadence map.
  The lexicographic consumers are the canonicalization sort and tie-break, the filing-observation key, the anchor date against the ISO closes, and the overlay's local sort.
  Codex round 1 (2026-08-28, changes requested): the `data-sources.md` contract had claimed every family the suite orders or compares by date, but the sector-P/E history stores its dates as served (`sector_pe_rows_from_value`, a dateless row kept with an empty date) and `fund::composite_yield_history` compares them lexicographically, so an unpadded print reads as after its own year's sample dates and is excluded there, then misselected as the latest print for later years.
  The contract is narrowed to the five canonical families — the dated EOD closes, the news dates, the per-symbol earnings rows, the dividend history, and the quarterly statement prints — and names the sector-P/E history as the as-built exception.
  Sector-P/E canonicalization is recorded off Codex round 1, not actioned — its own slice if it is wanted, the dateless-row semantics ruled with it.
  Ruled 2026-08-28: it is I14 below, its own slice ahead of the run on I10/I11's terms.
  Codex round 2 (2026-08-28, changes requested): the silent-drop line above had claimed every dated shaper; scoped to the row-dropping shapers, the dividend windower's whole-body rejection named.
  Codex round 3 (2026-08-28, changes requested): I14 had said a dateless print never wins the latest-print pick; it holds its exchange's slot where no dated print qualifies, so a lone dateless row supplies the historical P/E — corrected, the slice's tests to cover dateless-only and mixed inputs.
  Codex round 4 (2026-08-28, approve): no findings.

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
  Ruled 2026-08-28: the check lands at the adapter seam — `ModelAdapter::distill_call` sizes every distillation call's rendered prompt against its model's input budget before any request exists.
  Ruled 2026-08-28: the guard covers every 6d call — the final reduce, the tree-level reduce, the pass calls, and the tier-1 calls.
  Ruled 2026-08-28: a prompt over the fast tier's budget issues on the resident reasoner at its interpretation context — a model choice, never a `num_ctx` change.
  Ruled 2026-08-28: a prompt over the widest budget is refused before issue as an unclassified failure, never retried, and fails the run under §Failure posture's existing hard-path sentence.
  Ruled 2026-08-28: the measure is the whole rendered prompt in chars against `input_budget_chars` — the routing budget's own constants — so the instruction scaffolding counts.
  Ruled 2026-08-28: no new evidence surface — the tracker row's model id and `PromptUsage`'s `num_ctx` already show a routed-up call.
  Ruled 2026-08-28: no watch-set line — the default roster never routes up.
  Ruled 2026-08-28: the fast tier's co-residence whenever configured stands as the roster doc's precondition, so a route-up costs no runner swap.
  Ruled 2026-08-28, off the review round: the rendered single-pass prompt is sized once more against the budget, and one that outgrows it routes hierarchical rather than reaching the guard.
  The content sum the routing measures omits the instruction scaffolding, so on the default roster — where no second rung exists — the refusal would otherwise have bound a single-pass prompt that issued and physically fit before this slice.
  Ruled 2026-08-28, off Codex round 1 (P2): the rendered tier-1 prompt is sized once more the same way, and one that outgrows the budget takes that topic's pass-seam sub-distillation rather than reaching the guard.
  Ruled 2026-08-28, off Codex round 2 (P2): both rendered-size fallbacks compare against the widest budget the adapter can issue — `issue_budget_chars`, the reasoner's on a distinct roster — so a smaller shape is taken only where the guard would refuse.
  A prompt the reasoner can serve therefore routes up rather than spending the sub-distillation cap.
  Resolved 2026-08-28: `distill_route` in `pipeline.rs` sizes every distillation call's rendered prompt at the adapter seam before a request exists.
  Within the fast tier's budget the call issues there.
  Over it but within the reasoner's, it issues on the resident reasoner at the interpretation context.
  Over the widest budget it is refused as an unclassified failure — never retried — and the run fails.
  Two pins: the router's rungs and refusal (the default roster collapsing to one budget, the error carrying no retry class), and an over-budget pass prompt against a daemon stand-in that accepts nothing.
  The guard is canonical at `local-models.md` §The local-model adapter seam; `web-research.md`, `configuration.md`, `portfolio-workflow.md` §Step 6d, `portfolio-analysis.md` §Starting parameters, and the logic-flow's consolidation-call block carry pointers.
  The logic-flow's sub-distillation-cap bullet now reads "not sized as a drop trigger".
  The single-pass fallback is pinned: content within the budget whose rendered prompt outgrows it routes hierarchical with one tier-1 call and no sub-distillation.
  The tier-1 fallback is pinned: a topic within the budget by content whose rendered tier-1 prompt outgrows it sub-distills along its pass seam — a pass call, its tree reduce, then the reduce.
  Codex round 1 (P3): the budget is a chars-per-token estimate and the guard counts characters, so the docs no longer call front-truncation unreachable — closed off as far as the estimate can close it, the data-health likely-front-truncation read the runtime witness for every stage.
  Codex round 2 (P2): the round-1 fold-in had compared against the fast budget, so on a distinct roster near-threshold topics spent the shared cap on prompts the reasoner could serve.
  `issue_budget_chars` closes it, pinned on a two-topic distinct-roster fixture whose tier-1 prompts outgrow the fast budget and distill unsplit, and on its single-pass analog.
  Three Codex rounds — the first two with changes requested, the third approving.
  No stamp moves — the prompt content is unchanged.
- **Resume loses pre-crash prompt usage** — `CheckpointAccumulators` carries no prompt-usage field (`store.rs:191-199`), so a resumed run's data-health context-pressure / peak-prompt / length-stop lines reflect only post-resume holdings — under-reporting the exact signal the big-run prompt-fit watch reads.
  Ruled 2026-08-28: the trail carries completed holdings' calls only.
  The prompt-usage observations and fired-retry events drain into the run-level accumulators at each holding's checkpoint boundary, written beside the holding row in the same transaction.
  The interrupted holding's own abandoned calls are not written, since it re-analyzes whole and re-issues them.
  The `model_retries` invariant therefore holds exactly: in a persisted run every listed re-attempt succeeded.
  Ruled 2026-08-28: the two new accumulator fields carry no `#[serde(default)]`.
  No trail exists pre-slice, and a pre-field trail takes the documented loud-skip — unreadable accumulators, resume unavailable — rather than a compat default for data that never existed.
  Ruled 2026-08-28: the fields are flat vectors like `benchmark_gaps`, chronological order kept for the summary line's first-retry read.
  The residue is the sibling accumulators' own: a holding whose trail row was unreadable at load re-analyzes while its earlier rows stay seeded, so the read double-counts that one holding.
  Ruled 2026-08-28: the scope is the persisted data-health read alone — the pre-crash tracker rows stay out, the tracker being latest-run-only and process-local.
  Ruled 2026-08-28, off Codex round 1 (P2): the telemetry rides the holding's checkpoint row, not the accumulators.
  A row that never landed — the fail-soft write failed — or no longer reads takes its calls out of the trail with it, so the telemetry restored is exactly the rows restored and neither route double-counts a re-analyzed holding; the sibling counters keep their ruled residue.
  Resolved 2026-08-28: `CheckpointHolding` carries the holding's own `prompt_usage` and `model_retries`; the job drains the analyst at each holding's checkpoint boundary into the row and, before the fail-soft write, the run-level vectors, seeds those vectors from the restored rows in completion order on resume, and hands them to the roll-up whole.
  Two pins: a store round-trip of a row's telemetry, and the mid-run-failure resume test under instrumented analysts on both sides — the trail's one row carries the completed holding's calls, the interrupted holding's abandoned distill call reaches no row, and the finished run's retries read in cross-process order with the pre-crash peak and context-pressure row intact over a smaller post-resume fill.
  The mirrors: `portfolio-analysis.md` §Failure posture (canonical), the floor sentences in `local-models.md` §The local-model adapter seam and the watch set's fired-retry watch re-qualified to span the finished run, and a logic-flow §Resume behavior bullet.
  No stamp moves — no prompt content, grade band, or target basis changed, and the trail carries no format stamp.
  Codex round 1 (P2): the accumulator placement let a holding whose fail-soft write failed re-analyze on resume while a later successful write had carried its telemetry — a second route to the ruled residue; resolved by the row placement above.
  Codex round 1 (P3): the resume ran under a stub recording nothing, so the two-process merge and its order were unpinned; the resume now runs under an instrumented analyst.
  Codex round 2 (P3): the mirrors still named the interrupted holding's abandoned calls as the only absence, though a row that never landed or no longer reads drops its original calls too.
  Every mirror — the canonical §Failure posture sentence, `local-models.md`, the watch set, the store doc comment, and the logic-flow — now names the absence as the superseded calls of holdings the resumed process re-analyzed.
  The watch set reads a resumed run's rate over the calls the finished verdicts rest on and its count as a floor on every call the run issued.
  Three Codex rounds — the first with changes requested, the second approving the code and tests with one documentation correction, folded, the third approving with no findings.
  I17 is queued off the first round's P2: the run-level counters keep the double count the row placement closed for the telemetry.
- **No panic containment** — there is no `catch_unwind` around `run_analysis`, so any panic skips both `record_run(Failed)` and the terminal `run_finished` event; the run slot itself frees (poison-tolerant `RunGuard::lock`, `jobs.rs:132-173`), but the tracker never reaches a terminal state and the checkpoint trail is unofferable for that session.
  No realistic panic path was found in the spine files themselves — the exposure is the compute modules below.
  Ruled 2026-08-28: containment lands at the portfolio seam only — `run_analysis` under `catch_unwind` in `run_portfolio_job` — and the cloud report job's identical unguarded `run_job` shape is recorded here as a named candidate, not widened into the slice.
  Ruled 2026-08-28: a panic is never a user stop — with a cancel pending it still records `Failed`, bypassing the cancel arm, so the failed-job warning surfaces the crash.
  Ruled 2026-08-28: the failed detail carries the payload message only (`the analysis panicked: …`); the file:line stays on stderr via the default hook, no process-global hook installed.
  Ruled 2026-08-28: the mirrors reach `portfolio-analysis.md` §Failure posture (canonical) plus one pointer sentence each in `run-tracking.md` and the logic-flow §Failure logic; `scheduling.md` stands, the owning workflow classifying which failures end a job.
  Resolved 2026-08-28: `run_analysis` runs under `catch_unwind` in `run_portfolio_job`.
  A panic records `Failed` with `the analysis panicked: <payload>` as its detail, emits the terminal `run_finished`, and bypasses the cancel arm even with a cancel pending.
  A pinned test panics mid-book with the cancel flag set first and proves the failed row, the terminal event, no partial run, the completed holding's checkpoint intact, and resume eligibility.
  The mirrors landed as ruled — two sentences at `portfolio-analysis.md` §Failure posture, a pointer in `run-tracking.md` §Cancellation, a bullet in the logic-flow §Failure logic.
  Their trail wording reads "any eligible standing checkpoint trail", since a panic before the run opens its own trail leaves none (Codex round 1, P3).
  Observed, not fixed: the terminal legs run on `progress.rs`'s poisonable locks, which the compute modules never hold, and `scheduling.md` §Job States' Failed bullet names no internal error (A6).
  The cloud report job's `run_job` shape stands recorded above as the named containment candidate.
  Three Codex rounds — the second confirming the resume wording resolved, the third approving.
- **Three contrived-trigger panic paths, all whole-job kill radius given the missing containment** — the monotonicity-repair sort panics on a NaN scenario price (`engine.rs:1680`), reachable only through the fund flat-driver chain (`fund.rs:838`, `848` — composite yield lacks a finiteness/zero guard; `composite_yield` guards only `covered <= 0.0`); `percentile`'s sort panics on NaN and an inf sample yields a NaN result that feeds the first site (`engine.rs:1578`, `1586`; all live callers guard emptiness); an absurd feed-authored `period_end` near chrono's date ceiling panics on the +45-day grace add (`engine.rs:2227`).
  All require pathological feed values; recorded because containment, not probability, is the gap.
  Ruled 2026-08-28: the `composite_yield` finiteness/zero guard and the observation-vector filtering land in this slice, discharging I7's engine-side clause — I7 keeps the adapter leg alone (`fmp.rs` weight finiteness/range rejection), noted under I7 at resolution.
  Ruled 2026-08-28: a negative composite yield (short-exposure sector weights) is untouched — only non-finite and zero are guarded — and stands recorded as an observed edge.
  Ruled 2026-08-28: Codex's round-1 P2 — a non-finite scenario or target persisting as a `null` the store cannot read back, the whole run row loud-skipping — folds into this slice as the engine's output gate rather than queuing as I16.
  Its persistence round-trip test is declined, the gate making a non-finite target unemittable.
  Resolved 2026-08-28: `percentile` drops non-finite samples and sorts with `total_cmp`, the three collection sites filter non-finite observations before the floors count them, and the monotonicity repair sorts with `total_cmp`.
  The filing grace add is `checked_add_signed`, an unalignable window skipped like an unparseable one.
  The ceiling is reachable only at exactly `+262142-12-31` — chrono 0.4.44's `NaiveDate::MAX`, a later year failing to parse at the contiguity gate — and is pinned red against the unchecked add and green after.
  `composite_yield` skips a non-finite weight row and reads as absent when the yield is non-finite or zero, so the fund flat-driver chain routes to its existing insufficient-evidence arm.
  That discharges I7's engine-side clause, its adapter leg staying with I7.
  The output gate — `engine::price_targets_finite` — exits a holding as insufficient evidence in `analyze` and `analyze_fund` when any target leg is non-finite.
  It is pinned on Codex's recipe (consensus revenue at `f64::MAX` over a subnormal share count, the carry path's `inf × 0`) and on a fund with a non-finite quote, and `portfolio-analysis.md` §Evidence floor names the exit.
  Observed, not fixed: a negative composite yield still prices a negative flat driver (ruled untouched above).
  Codex round 2 reframed the class — every required persisted float, not the targets alone (the dividend shaper's unchecked sum can store an inf `forward_dividends`) — and it is queued as I16 rather than widened here.
  Its doc-comment placement note (the `analyze` block had been stranded above the new helper) is applied.
  Three Codex rounds, the third approving.
- **IPv6-literal fetch URLs can never fetch** — `resolve_public` passes `host_str()`'s bracketed IPv6 form to `to_socket_addrs()` unparsed (`fetch.rs:153-164`; `check_url_policy` at `:205` knows to trim the brackets), so every IPv6-literal fetch errors and spends a budget unit; fails closed, so the SSRF direction is safe.
  Ruled 2026-08-28: both guards read the typed `Url::host()` — a literal address, IPv4 or bracketed IPv6, resolves as itself with no lookup in `resolve_public`, and `check_url_policy` drops its hand-rolled bracket trim for the same match.
  Ruled 2026-08-28: the SSRF bullet at `web-research.md` §Safety and provenance gains one sentence stating the literal-address rule; no mirror exists to update.
  Resolved 2026-08-28: a shared `literal_host` reads the parser's typed host; `resolve_public` pins a literal to itself with no lookup and runs it through the public-host rules, and `check_url_policy` reads the same helper.
  A public IPv6 literal now fetches, and a non-public literal of either family fails closed with its policy reason instead of a resolver error.
  Two pins: `validate_url` over public v4 / v6 literals (pinned to themselves) and non-public v4 / v6 / mapped-v4 literals (blocked by reason, never "resolving"), and the production loopback guard extended to `[::1]`.
  `url` is named as a direct dependency for `url::Host` — the crate already rode in through reqwest, which re-exports only `Url`.
  No stamp moves.
  One Codex round, approving with no findings.

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
Ruled 2026-08-29 (group 5): the seed-edge fix moves no stamp — the reduce-prompt template is unchanged, a fully-dropped topic's retained prior riding the existing dormant-prior render, and the store is wiped before the run so no trail can straddle the change.
Ruled 2026-08-29: a fully-dropped topic with no prior object in the freshness window is not named unreconciled — nothing of it was in the reduce and there is no object to retain, so its every-pass-dropped gap line stands alone.
A stored row past the window, if one stands, stays inert behind the seed gate, as for any dormant topic whose object expired (the wording narrowed off Codex round 1 below).
Ruled 2026-08-29: provenance stays as built — a retained prior claim whose URL a dropped pass re-fetched resolves fresh at this run's retrieval date (the page was fetched; only its distillation dropped), and an omitted tie decays unless the model re-cites the id the render shows, as for any fresh claim.
Resolved 2026-08-29: "analyzed this run" is defined once in `distill` as a topic whose research reaches the reduce — every topic with a pass, narrowed on the hierarchical path by a topic the cap drops whole — and the routing, the reduce and the reconciliation read that one set (`validate_combined` receives it).
A topic dropped whole leaves the set, so its prior rides the reduce through `dormant_priors_of` on its own vintage, and the drop itself never names it unreconciled — a reduce that fails to re-emit it still does, like any dormant prior.
The gap line keeps the `dropped at the sub-distillation cap` substring the watch set greps, with a tail naming which case applied.
Two tests pin the edge: a retained prior under the dormant render re-emitting on its own vintage with `unreconciled_topics` empty, and the no-prior twin absent from the reduce, the layer and the unreconciled list, an object the model invents for it dropped as an unknown topic.
`portfolio-analysis.md` §Starting parameters, the logic-flow's budget-exhausted bullet and the watch set's cap line now state the edge as closed.
One reviewer round (approve-with-nits) covered the group, its four nits folded in by ruling: the trigger paragraph's semicolon-joined pair split to sentence-per-line, the "never named unreconciled" wording narrowed to the drop itself at every site, the four-pass fixture commented as a cap-spending device beyond the three-pass ceiling, and `subdistilled_topics` no longer counting a topic dropped whole, since it issued no call (pre-existing, folded here); the fail-soft sentence's §Starting parameters home was noted and left.
Codex round 1 (2026-08-29) approved with one low finding: "no prior object" had been written as "no stored row" at the drop-arm comment, the no-prior test's comment and the ruling line above, while the pipeline filters expired objects before distillation (`pipeline.rs`, `topic_object_fresh`) over a store that loads every row, so `prior.is_none()` can coincide with an expired row.
Closed as wording: the row is inert behind the seed gate — the existing posture for a dormant topic whose object expired — and the three sites now say so.
A boundary test is declined, since the row is filtered before `distill` sees it and the dormant-expired path already carries the posture; the gates were re-run clean.
Codex round 2 (2026-08-29) approved the wording fold on static re-review, no findings.

### Priority-3 minor findings

- **Research-fed fraud listed under "Hard forensic state (live)"** — doc lines 863–865 place "fraud may arrive later from validated primary-source research" inside the hard-state bullet, but the 2026-08-24 ruling made the research-fed claim advisory-only — it never merges into the hard producer state (`pipeline.rs:351-365`), which gates the add family.
  Resolved 2026-08-28: the hard-state bullet now states the research-fed `forensic_event` fraud claim is advisory by the 2026-08-24 ruling, pointing at the canonical `trade-opportunities-workflow.md` §Step 5c.
  It never joins the state, the hard rule trips from the item-classified filing kinds alone, and the claim reaches the model only as cited attention evidence.
- **"A qualifying news seed" reads as an independent tech-topic trigger** — doc line 978; the code requires the conjunction with a standing technology-class ledger falsifier (`pipeline.rs:1002-1003`; same conjunction in the quick check, `quick_check.rs:82-84`), so fresh news alone never fires the deep-dive.
  Resolved 2026-08-28: the trigger line now states the conjunction — a fresh symbol-scoped `news/stock` seed while a technology-class falsifier already stands, the quick check's same rule — and that a seed alone never fires the topic.
  It also records what the conjunction implies as-built.
  `build_agenda` resolves the topic's reason in priority order — pre-flag, standing falsifier, news seed — and `tech_news_seed` entails `tech_ledger_falsifier`.
  The seed branch and its `qualifying news-feed seed` reason label are therefore unreachable; the falsifier line fires the topic.
  A Codex round caught the Step-6c inputs bullet still saying the seeds "can trigger" the topic; it now names the conjunction — the seed leg fires only beside a standing falsifier, never alone.
  Ruled 2026-08-28: `portfolio-analysis.md` §The per-holding pipeline's trigger list names the seed by the defined term — a qualifying news-feed seed, the conjunction defined at §Starting parameters — while `portfolio-workflow.md` §Step 6c already uses the term and is untouched.
  The unreachable reason branch — and, off the second Codex round, the reason label's missing consumer — is I15 below, its own slice ahead of the run on I10/I11's terms.
- **The narrative read's 7-day minimum is undocumented** — the doc names only the debut as carrying no read (lines 578, 852–857), but `NARRATIVE_MIN_ELAPSED_DAYS = 7` (`engine.rs:3128`) gaps the read for anyone running more than once a week.
  Resolved 2026-08-28: both logic-flow sites (the Step-6b order entry and the §Other deterministic reads narrative bullet) now state the 7-day minimum beside the debut case, naming `NARRATIVE_MIN_ELAPSED_DAYS` and pointing at `portfolio-analysis.md` §Starting parameters.
  The canonical home already carried the constant (its three-constants sentence landed with the evidence-legs slice, 2026-08-21), so only the mirror moved.
- **"(pre-profit stocks only)" vs computed-for-every-stock** — the Step-6b order list's parenthetical (line 569) contradicts the doc's own lines 596/651 and the code (`pipeline.rs:863-873`): the overlay record is computed and persisted for every priced stock.
  Resolved 2026-08-28: the Step-6b order entry reads "computed and persisted for every priced stock; only an eligible read binds", matching the doc's own §Pre-profit overlay and the code.
- **"One isolated conversation per agenda topic"** — doc lines 1000–1001; as-built each *pass* is its own fresh conversation (`research.rs:1067-1090`), with only the claims ledger and findings carrying across a topic's passes — which the doc's own "who owns the context" bullets state correctly.
  Resolved 2026-08-28: the Topic bullet names the topic as the unit of isolation (the "isolated conversation" as `web-research.md` §Terminology defines it), and the context-ownership bullets are scoped to within a pass.
  The Pass bullet states the as-built mechanics: each pass opens a fresh message history of the system prompt plus a pass brief, so only the evidence ledger and the topic's own accumulated per-pass findings carry across its passes.
  Ruled 2026-08-28: the canonical term stays at `web-research.md` §Terminology and its `portfolio-workflow.md` mirrors — it defines per-topic isolation, not one continuous history — and the Trade Opportunities logic-flow's uses are untouched.
- **"The thresholds are config knobs"** — doc line 1076; `OVERFLOW_THRESHOLD`, `CHARS_PER_TOKEN` (`distill.rs:48-51`) and `NUM_CTX_DISTILL` (`pipeline.rs:4535`) are compile-time constants exposed in no settings surface.
  Resolved 2026-08-28: the consolidation bullet names the compile-time constants — `OVERFLOW_THRESHOLD`, `CHARS_PER_TOKEN`, `NUM_CTX_DISTILL` — as exposed in no settings surface, the knobs `configuration.md` §Local Analysis Suite Configuration designs being unbuilt.
  Ruled 2026-08-28: `configuration.md` keeps describing the designed knobs — the corpus describes designed and built without distinction, and build status is `BUILD.md`'s.
- **Supersede validation legs overstated** — doc line 1155 claims metric/units/period match legs and that "a supersede always rejects"; the code has no match legs, and with an absent feed value a supersede-declared claim does not reject — it falls through and fills exactly like a supplement (`engine.rs:2039-2057`; the F-minor loss-displacement finding rides the same guard).
  Ruled 2026-08-27: the supersede leg is dormant by design.
  The doc moves to the as-built rule — a supersede rejects against any present feed value, and a supersede declared against an absent value is downgraded to a supplement fill, which `matched_rule` will name — and the match legs leave the doc.
  The true leg is revivable only if the channel is promoted and the consensus feed gains an as-of date.
  One slice: the doc plus the label.
  Ruled 2026-08-28: the canonical `portfolio-workflow.md` §Step 6e gains one appended sentence stating the downgrade rule, its designed three-check supersede sentence standing as written — the corpus describes designed and built without distinction, and the true leg stays revivable.
  Ruled 2026-08-28: the rewritten logic-flow line keeps the revival condition as a parenthetical — the leg is dormant, not absent.
  Ruled 2026-08-28: the downgrade label leads with `supplement`, naming the declared supersede and the absent feed value in its parenthetical, so the accepted-rule family stays greppable by prefix.
  Resolved 2026-08-28: when the declaration was `supersede`, the fill's `matched_rule` reads `supplement (downgraded from a declared supersede — the structured feed carries no <driver> value to contradict): filled the absent …`, the plain label unchanged, and a pinned test proves the downgraded fill is the supplement's fill exactly.
  The function's doc comment and the logic-flow's Step-6e validation bullet state the as-built rule — the whitelist binding both declarations, the leg dormant with its revival condition, the present-feed rejection, the downgrade the rule names — and the match legs left the line.
  `portfolio-workflow.md` §Step 6e carries the downgrade sentence beside its designed three-check sentence.
  `storage.md` §Local Analysis Suite Storage's methodology leg now records the resolution for every evaluated assumption rather than only where it conflicted — a sibling imprecision the review surfaced, folded in by ruling.
  One Codex round, no findings.
- **"An accepted forward assumption" as what-changed evidence** — doc line 1327; the delta row is pushed whenever distillation validated an assumption, regardless of its Step-6e shadow resolution — a `rejected:` resolution still anchors an external what-changed row (`pipeline.rs:1222-1230`).
  Ruled 2026-08-28: the row stands as validated-assumption evidence, so the fix is the doc line, joining the Priority-3 batch.
  Nothing is ever accepted under the shadow ruling, an engine rejection grades recomputability rather than the fact's truth, and the canonical homes already say "the logged forward assumption".
  The shadow resolution persists on the audit's research record beside the row; no frontend site renders the assumption or its resolution.
  Resolved 2026-08-28: both Step-6g sites read "the logged forward assumption", stating that its Step-6e shadow resolution records on the audit's research record beside it and never conditions the row.

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
I13, added 2026-08-28 off the ledger-basis slice's Codex rounds, joins on the same terms.
I14, added 2026-08-28 off the statement-date slice's Codex round, joins on the same terms.
I15, added 2026-08-28 off the Priority-3 doc batch — surfaced by its implementation, reframed by its second Codex round — joins on the same terms.
I16, added 2026-08-28 off the panic-posture slice's second Codex round — the unreadable-run class beyond the targets — joins on the same terms.
Fix grouping ruled 2026-08-27: one finding per slice through the plan → implement → review → Codex → commit loop, each marked here; the resume prompt-usage minor is the one-seam exception, its retry events and prompt usage riding `CheckpointAccumulators` together as a single slice.
Fix grouping revised 2026-08-28 for the Priority-2 and -3 minors, the one-finding rule otherwise standing.
The six pure doc-line Priority-3 minors — research-fed fraud under the hard state, the qualifying news seed, the narrative 7-day minimum (its canonical-home sentence in `portfolio-analysis.md` §Starting parameters included), "(pre-profit stocks only)", one conversation per topic, and the config-knob thresholds — are one slice, each still marked resolved on its own line.
The supersede-legs minor stays its own slice as ruled (the doc plus the label).
The forward-assumption what-changed minor is ruled at the doc batch's plan: if the row stands as validated-assumption evidence with its shadow resolution beside it, it joins the batch; if the row should push only on an accepted resolution, it is its own code slice.
The panic-containment minor and the three contrived-trigger panic paths are one slice — the containment and the exposure it contains — with the §Failure posture and run-tracking mirrors.
The reduce-prompt size check, the resume prompt usage (its retry events riding with it, as ruled), and the IPv6-literal fetch each stay their own slice.
A batch never mixes code and doc findings.
The queue is therefore seven slices before Codex I1–I14 and the §A4 seed edge: the Priority-3 doc batch first, then the supersede legs, the forward-assumption minor, panic posture, the reduce-prompt check, the resume prompt usage, and the IPv6 fetch.
The six Priority-3 doc-line minors are resolved (2026-08-28; each resolution is recorded on its own line under §Priority-3 minor findings).
The forward-assumption what-changed minor is ruled and resolved with them (2026-08-28; the ruling and resolution are recorded under its bullet), leaving the supersede legs, panic posture, the reduce-prompt check, the resume prompt usage, and the IPv6 fetch ahead of Codex I1–I15 and the §A4 seed edge.
The grouping line's count above read eight against its enumerated seven; it is corrected off this slice's Codex round.
The supersede-legs minor is resolved (2026-08-28; the resolution is recorded under its bullet), leaving panic posture, the reduce-prompt check, the resume prompt usage, and the IPv6 fetch ahead of Codex I1–I15 and the §A4 seed edge.
The panic-posture slice — the containment minor and the three panic paths, Codex's round-1 P2 folded in by ruling — is resolved (2026-08-28; the resolutions are recorded under both bullets), leaving the reduce-prompt check, the resume prompt usage, and the IPv6 fetch ahead of Codex I1–I15 and the §A4 seed edge.
The reduce-prompt size check is resolved (2026-08-28; the rulings and resolution are recorded under its bullet), leaving the resume prompt usage and the IPv6 fetch ahead of Codex I1–I16 and the §A4 seed edge.
The resume prompt usage is resolved (2026-08-28; the rulings and resolution are recorded under its bullet), and its first Codex round queued I17, leaving the IPv6 fetch ahead of Codex I1–I17 and the §A4 seed edge.
The IPv6-literal fetch is resolved (2026-08-28; the rulings and resolution are recorded under its bullet), leaving Codex I1–I17 and the §A4 seed edge — the named slices before the Codex items are all handled.
I1 is resolved (2026-08-28; the rulings and resolution are recorded under §I1), leaving Codex I2–I17 and the §A4 seed edge.
I18, added 2026-08-28 off I1's Codex round 1 — the resume-across-a-rebuild question — joins the queue as a ruling item on the same terms.
I2 and I14 are resolved together (2026-08-28; the rulings and resolutions are recorded under §I2 and §I14), leaving Codex I3–I13, I15–I18 and the §A4 seed edge.
I3 is resolved (2026-08-28; the rulings and resolution are recorded under §I3), leaving Codex I4–I13, I15–I18 and the §A4 seed edge.
I19, added 2026-08-28 off I3's Codex round 2 and re-cut off its rounds 3 and 4 — the one-fact contract's single-number ambiguity — joins the queue as a ruling item on the same terms.
I4 is resolved (2026-08-28; the rulings and resolution are recorded under §I4), leaving Codex I5–I13, I15–I19 and the §A4 seed edge.
I5 is resolved (2026-08-28; the rulings and resolution are recorded under §I5), leaving Codex I6–I13, I15–I19 and the §A4 seed edge.
I6 is resolved (2026-08-29; the rulings and resolution are recorded under §I6), leaving Codex I7–I13, I15–I19 and the §A4 seed edge.
Fix grouping revised 2026-08-29 for the Codex minors and the §A4 seed edge, the one-finding rule otherwise standing: a group is cut on one code locus and one stamp axis, runs the plan → implement → review → Codex → commit loop once, and each member is still marked resolved on its own line.
Five groups follow, in order.
I7, I9 and I16 are one group — the FMP shapers' integrity guards and the required-float audit, no stamp expected.
I18 ruled, then I17, are one group — I17 changes the trail's row shape, the case I18 asks about, so the ruling governs the implementation.
I8, I10 and I12, with I19 ruled at the top, are one group — the prompt renders under one `PROMPT_VERSION` bump, a guard off I19 riding it since that contract rides the prompt stamp.
I11 and I13 are one group — the continuity-attribution mirrors of the grade-version and flow-basis gates.
I15, ruled at its plan, and the §A4 seed edge are one group — the research loop's residue.
A group never crosses a stamp axis, and a batch still never mixes code and doc findings.
I7, I9 and I16 are resolved (2026-08-29; the rulings and resolutions are recorded under §I7, §I9 and §I16), leaving Codex I8, I10–I13, I15, I17–I19 and the §A4 seed edge.
I18 is ruled and I17 resolved with it (2026-08-29; the rulings and resolutions are recorded under §I17 and §I18, the trail's shape stamped `checkpoint-v2`), leaving Codex I8, I10–I13, I15, I19 and the §A4 seed edge.
I19 is ruled and I8, I10 and I12 resolved with it (2026-08-29; the rulings and resolutions are recorded under §I8, §I10, §I12 and §I19, the prompt stamp moved to `portfolio-v22`), leaving Codex I11, I13, I15 and the §A4 seed edge.
I20, added 2026-08-29 off group 3's Codex round 1 — a carried observation row carries no admission stamp — joins the queue on the same terms, ruled at its addition (attribute, never re-filter; recorded under §I20), leaving Codex I11, I13, I15, I20 and the §A4 seed edge.
Ruled 2026-08-29, off group 4's plan: the dev store is wiped before the run and no local-suite data compat is required pre-release, so the fresh-start-2 compat cut lands ahead of group 4 as its own slice with its own record ([2026-08-29-fresh-start-2-local-suite-compat-removal.md](2026-08-29-fresh-start-2-local-suite-compat-removal.md)); every holding on the run is therefore a debut, and a second run follows only on the user's decision after the first run's result.
I11 and I13 are resolved (2026-08-29, group 4; the rulings and resolutions are recorded under §I11 and §I13, the prompt stamp moved to `portfolio-v23`), leaving Codex I15, I20 and the §A4 seed edge.
I15 is ruled and resolved and the §A4 seed edge closed together (2026-08-29, group 5; the rulings and resolutions are recorded under §I15 and §A4, no stamp moving), leaving Codex I20.
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
I13 (2026-08-28) is the equity-source continuity gap Codex round 2 on the ledger-basis slice surfaced, given its own heading by ruling and queued on the same terms.
I14 (2026-08-28) is the sector-P/E history date canonicalization Codex round 1 on the statement-date slice surfaced, given its own heading by ruling and queued on the same terms.

### I1 — major: a non-positive quote passes the evidence floor and can make a successful run persist as unreadable

The FMP quote adapter accepts any JSON number as `current_price`, including zero or a negative value (`src-tauri/src/fmp.rs:1825-1834`), and the stock evidence floor checks only `Some(price)` (`src-tauri/src/portfolio/engine.rs:1240-1275`).
The fund path has the same hole and makes it worse: `fin.current_price.or(fund.nav)` lets `Some(0.0)` or a negative quote mask a usable positive NAV (`src-tauri/src/portfolio/fund.rs:663-673`).

Downstream target arithmetic divides by spot in the scenario total returns and one-month target builder (`src-tauri/src/portfolio/engine.rs:1702`, `2560-2561`).
A zero quote therefore creates infinities and NaNs; a negative quote creates finite but financially meaningless returns and targets.
The final run serializer writes non-finite `f64` values as JSON `null`, so `insert_run` can report success (`src-tauri/src/portfolio/store.rs:655-666`) while later `PortfolioRun` decoding rejects the blob and the history lists it as unreadable (`src-tauri/src/portfolio/store.rs:763-804`).
The same poison can enter the checkpoint blob before final persistence.

The price / NAV evidence floor must require a finite, strictly positive value before any target, hurdle, checkpoint, or run record is built, and the logic-flow description should call this a usable finite-positive quote rather than mere presence.

Ruled 2026-08-28: the fix seam is the FMP parse plus both engine floors — `company_quote_from_value` shapes a non-positive print to no price, keeping the served value for a named gap, and `analyze` / `analyze_fund` test finite-and-positive as belt-and-braces for the producers that are not FMP.
Ruled 2026-08-28: on the fund analog an unusable market quote beside a usable NAV prices the fund off the NAV — the floor's `or(nav)` design keyed on usability rather than presence.
Ruled 2026-08-28: the quick-check "failed price refresh" sentences and the `data-sources.md` quote row stay unedited; the usability rule is single-homed at `portfolio-analysis.md` §Evidence floor with the logic-flow §Evidence floor bullets as its one mirror.
Ruled 2026-08-28: the watch set gains no line — a usable-quote abstention surfaces under the existing abstention-reasons watch.
Ruled 2026-08-28 (Codex round 2): the round's stamp-persistence and gap-cause findings are fixed in the slice.
Ruled 2026-08-28 (Codex round 2): the general question round 1 raised — a trail resuming across any rebuild that moves no stamp — is queued as I18, a ruling item ahead of the run.
Ruled 2026-08-28 (Codex round 1): the floor rule gets its own version — `engine::EVIDENCE_FLOOR_VERSION`, `evidence-floor-v2` after the `evidence-floor-v1` presence baseline — stamped on the checkpoint header, where `resume_eligibility` refuses a trail under another, and on every holding's audit record.
The standing three stamp axes (prompt content, grade band, stored-target basis) do not cover a floor-rule change, and a pre-fix trail must not resume its completed holdings into the usability floor.
**Resolved 2026-08-28.**
As-built before the fix, the finite-target gate (landed off the panic-posture slice) already exited a zero stock quote — `0 × inf` is NaN on the one-month leg — under that gate's misdescribed reason, so the unreadable-run chain above was closed for a zero print.
A negative quote still graded finite, financially meaningless targets and a hurdle read.
A zero quote masked a usable NAV on the fund path.
A zero live print in the quick check's price refresh read as a bear–bull band crossing.
The fix applies `engine::usable_price` (finite and strictly positive) at the FMP parse, so the per-holding pull records no price with a gap and an `empty` tracker row naming the served print, and the quick check's `fetch_live_price` and the run-level `fetch_commodity_quote` both `Err` naming it.
`analyze` and `analyze_fund` read through the same predicate under their own reasons (the served print named), the fund floor falling to a usable NAV.
The ledger's `Price` series resolves unevaluable on an unusable print.
The gate's reason strings no longer name the quote, which never reaches it.
Codex round 1 added two legs.
`nav_premium_read` reads both legs through the usability test and keeps only a finite quotient, so a non-finite quote from a producer other than the FMP parser never re-enters as a premium the audit would persist as `null`.
The evidence-floor version above is the second leg, with the resume gate's refusal pinned beside the prompt-drift refusal.
Codex round 2 corrected the stamp's persistence.
Both new fields carry a serde default of `evidence-floor-v1`, so a run or trail persisted before the field decodes as the presence floor it was floored under.
The run stays readable — attempt 2 is the store's one persisted run, the big run's diff / carry baseline — and the trail is refused by the gate's own reason instead of loud-skipping as an unreadable header.
A pre-field run and a pre-field header are pinned.
Codex round 3 approved the slice with no findings.
The same round moved the closed-end gap cause onto the usability test on both legs, a usable pair whose quotient is not finite naming the read rather than a missing leg.
Canonical at `portfolio-analysis.md` §Evidence floor (the usability, adapter, fund-analog, and stamp sentences, the gate sentence re-qualified, the fund analog's quote / NAV called usable), mirrored in the logic-flow §Evidence floor bullets and run-record line, the storage inventory naming the audit stamp.
No prompt, grade, or target stamp moves.
The evidence-floor stamp is new with this slice, off its Codex round 1.
Twelve tests pinned (the Codex-round legs included — the NAV-premium read over unusable and overflowing legs, the resume refusal on a floor-version drift, the pre-field run and header decodes, the closed-end gap causes): the parser over zero / negative / usable / absent, the three consumers' gaps, errors, and `empty` rows off one mock server, the stock floor over zero / negative / NaN / inf under the usability reason (never the gate's), the `Price` series unevaluable on a zero print, the fund pricing off the NAV at `spot == nav` under a zero / negative / infinite quote, the fund floor abstaining with no usable leg on either side, and — replacing the retired infinite-quote gate pin, which now lands at the floor — a finite `f64::MAX` quote passing the floor and overflowing the composite scenario into the gate.

### I2 — major: one stale fund P/E print can fabricate all twelve quarterly history samples

`composite_yield_history` samples twelve target quarter ends, but for each target it selects the latest sector print on or before that date with no maximum age and no requirement that the selected print belongs to that quarter (`src-tauri/src/portfolio/fund.rs:456-498`).
The same old print can therefore be reused at every target date and counted as twelve independent samples.
The production fetch intentionally requests about 1,600 days (`src-tauri/src/portfolio/job.rs:525-543`), so a sparse response with one old row is sufficient to reach this path.

The adapter intensifies the issue by converting a historical P/E row with no date to `""`, which compares before every ISO sample date and is consequently eligible for every quarter (`src-tauri/src/fmp.rs:6340-6372`).
The fund evidence floor then checks only `history.len() >= 8` (`src-tauri/src/portfolio/fund.rs:786-803`), and the fabricated series also becomes the target anchor history (`src-tauri/src/portfolio/fund.rs:835-850`).
A single unchanged print can thus produce a 0-or-100 valuation percentile, pass the claimed eight-observation floor, and supply twelve fake target anchors.

This contradicts the logic-flow contract's “~8–12 quarters” and “≥ 8 history samples” language (`logic-flow-docs/portfolio-analysis-logic-flow.md:779-786`).
Each sample needs a bounded-distance observation from its intended quarter, missing dates must be rejected, and the evidence floor must count distinct source observations / quarters rather than generated sample dates.

Ruled 2026-08-28: the bound is the sample's own quarter — after the prior quarter end, on or before its own — with no drafted max-age constant, so one print backs at most one sample and the floor counts distinct observations by construction.
Ruled 2026-08-28: the fix moves `engine::EVIDENCE_FLOOR_VERSION` to `evidence-floor-v3`; the grade, target, and prompt stamps do not move, since a healthy daily feed yields identical samples.
Ruled 2026-08-28: I14 is absorbed into this slice — parsing dates at the sampler closes its sampler half, and its shaper leg lands with it.
Ruled 2026-08-28: no watch-set line and no data-health counter — a thin-history abstention surfaces under the existing floor-reasons watch, and a failed history fetch already lands in the fund's gaps.
**Resolved 2026-08-28.**
`fund::composite_yield_history` parses every print's date once, drops an undatable row, and admits per sector per exchange only the latest print dated within the sample's own quarter, comparing parsed dates rather than strings.
The floor code is unchanged — `history.len()` now counts distinct in-quarter samples by construction — and its reason names them.
Codex round 1 approved the slice — the I14 shaper leg included — with no findings.
Seven tests pinned: the NaN-weight pin re-based onto the quarterly fixture (it had asserted twelve samples off one 2020 print), one stale print backing no sample and the fund abstaining naming `0`, the window boundary (a print on a quarter end backs that quarter alone, the next day's the following quarter alone), a dateless print never qualifying alone or beside dated prints, a non-padded in-quarter print admitted chronologically, seven in-quarter quarters abstaining and eight pricing, and the snapshot blend reading no date.
Canonical at `portfolio-analysis.md` §Asset eligibility (the in-quarter admission and undatable-print sentences) with the §Evidence floor stamp sentence at `evidence-floor-v3`, mirrored in the logic-flow's fund-valuation inputs and note lines and its §Evidence floor fund bullet.

### I3 — major: pre-profit source corroboration can validate the wrong sign and an unrelated number

`value_in_text` rejects adjacent digits and decimal continuations but does not treat a preceding sign as part of the number (`src-tauri/src/portfolio/pre_profit.rs:816-858`).
As a result, a candidate `+41` corroborates against page text containing `-41`; the test suite also explicitly accepts positive `41` against `(41)` (`src-tauri/src/portfolio/pre_profit.rs:1683-1698`), although parentheses are standard accounting-negative notation.

The broader validator requires only that the page mention the issuer somewhere and contain the numeric string somewhere (`src-tauri/src/portfolio/pre_profit.rs:870-902`).
It never binds that occurrence to the declared metric, units, period, issuer scope, observation role, or nearby text.
A long earnings release can therefore validate a model-authored deliveries row with a revenue number, a current actual with a prior-period number, or a positive unit-economics fact with a negative printed value.

Accepted rows drive the deterministic 5% / 20% guidance-miss rules and can cap engine conviction or restrict its action set, so this is not merely a citation-display weakness.
It contradicts the logic-flow promise to “confirm the source states the number” in the sense required for a typed financial observation (`logic-flow-docs/portfolio-analysis-logic-flow.md:1158-1161`).
Corroboration must be sign-aware and must validate the number in metric / units / period context, or the typed row must carry a source excerpt / locator that the app can verify.

Ruled 2026-08-28: the fix shape is the verifiable locator — the typed row carries a verbatim `source_excerpt`, and the app verifies it appears in the fetched page, corroborates the value sign-aware inside it, and requires a metric-family stem for the row's `metric_kind`.
Ruled 2026-08-28: sign-awareness lands in the shared `value_in_text`, so the forward-assumption value / range-endpoint checks and the leading indicator's percent render inherit it — recorded as inherited, not scope widening, with pins on both consumers.
Ruled 2026-08-28: `portfolio::PROMPT_VERSION` moves to `portfolio-v17` (a required schema field plus a prompt line, so a `v16` trail cannot resume); `PRE_PROFIT_PARAMETER_VERSION` stays `pre-profit-v2`, the overlay's computation being unchanged and no store holding a research-produced row; no grade, target, or floor stamp moves.
Ruled 2026-08-28: the excerpt binds the metric-family stem (a drafted `metric_stems` table, calibratable); units and period stay unbound, the residual named below, the excerpt persisting on every accepted and rejected row for audit by eye.
Ruled 2026-08-28: `source_excerpt` carries no serde default — no store holds a research-produced row, and the resume gate refuses the prior trail by its own reason.
Ruled 2026-08-28: the watch set's §Pre-profit overlay rejection-split parenthetical names the new legs as one clause.
Ruled 2026-08-28 (Codex round 1): the round's three findings are fixed in the slice — the metric-family language binds to its nearest number in the quote (period tokens, percentages, and a range partner aside) with the bookings lexicon narrowed to plural and compound forms, the persisted-row round-trip is pinned at the overlay and through the store, and the storage sentence calls the excerpt the row's offered locator.
Ruled 2026-08-28 (Codex round 2): both findings are fixed in the slice — the binding is re-cut from nearest-number to the number a stem states by position, a comma-formatted run never a year and a percentage the stem's own number only where no plain value follows, every sign-correct occurrence of the value a candidate — and the binding's two named residuals are queued as I19, a ruling item ahead of the run.
Ruled 2026-08-28 (Codex round 3): the positional binding is replaced by the narrow one-fact contract — the quote carries the metric stem and exactly one number, the row's value at its sign, every digit run counting, a guidance-low or guidance-high row alone quoting a range's two endpoints — the year and percentage classes deleted with it, and I19 re-cut to the competing-noun residual alone.
Ruled 2026-08-28 (Codex round 4): the contract is recorded as a conservative syntactic admission filter, never semantic proof — a second number beside the value rejects, the meaning of the one admitted number is unverifiable — and I19 broadens from the competing-noun residual to the single-number ambiguity, the candidate-is-the-period shape included.
**Resolved 2026-08-28.**
As-built before the fix, `value_in_text` located the magnitude at number boundaries and read no sign, so `+41` corroborated off `-41` and the boundary test pinned positive `41` against the accounting `(41)` as a pass; `validate_against_source` then asked only that the page mention the holding somewhere and contain the number somewhere, binding the occurrence to nothing.
The fix reads the printed sign beside each boundary-clean occurrence — a minus hugging the digits (ASCII or U+2212, through one currency symbol) whose own left neighbour is not a digit, or a parenthesis pair wrapping exactly the number — and corroborates only at the candidate's sign, zero unsigned, a hyphen between digits a range or date separator.
The row gains a required `source_excerpt` — the fetched page's own sentence stating the value, quoted verbatim — which `validate_observation` bounds structurally (present, at most `SOURCE_EXCERPT_CAP_CHARS`) and `validate_against_source` verifies in the page after whitespace-run normalization, corroborating the value inside the excerpt and requiring a metric-family stem at a word start (`metric_stems`, drafted per kind), each leg under its own reason.
The 6d schema requires the field and the prompt line asks for the verbatim quote, naming the cap.
The reviewer's round caught the excerpt edge — a quote trimmed to the digits shed the page-side sign, parenthesis, or leading digit, and the page-wide check it replaced would have caught the digit.
It is closed by reading the value inside the quoted span with the page's own neighbours around it (`value_stated_in_excerpt`), a number just outside the quote never counting as quoted.
The same round's nit moved a minus after a percent sign or a closing parenthesis to the range-separator reading (`40%-45%`).
Codex round 1 found the metric leg unbound to the number: a compound sentence quoted verbatim ("revenue was 41 million while deliveries reached 141 units") lent its deliveries stem to the revenue number, and a bare `order` stem read "in order to" as bookings language.
The fix binds the stem to its nearest number in the quote — `excerpt_binds_metric` over a number census (`quoted_numbers`) that exempts period tokens and percentages and admits the value's own range partner — under its own reason, and the bookings lexicon keeps plural and compound forms only.
The same round asked for the persisted-row round-trip the plan promised — pinned at the overlay over an accepted and a rejected row, and through the store's run JSON — and corrected the storage sentence, a rejected row's excerpt being the locator it offered rather than one it was corroborated through.
The round's serde-default caveat was settled by reading both app stores: the prod store holds no portfolio run and the dev store's one run (attempt 2) carries no observation or rejected row and no `source_excerpt` key, so no persisted row predates the field.
Codex round 2 found the round-1 exemptions leaking — a comma-stripped `2,025` read as a year and a `25%` as decoration, each clearing the way for the revenue number — and the first-occurrence read rejecting a value the page printed twice.
The binding is re-cut: a stem states its own number by position (`stem_number` — the first plain value after it, else the first percentage after it, else the nearest non-period value before it), the census (`quoted_numbers`) never reads a comma-formatted run as a year and marks the value's own occurrences as values, and `value_stated_in` returns every sign-correct occurrence for the binding to try.
The two residuals the binding cannot close — a competing noun the lexicon does not know, and an unseparated four-digit value in 1900–2099 reading as a year — are named in the canonical and queued as I19.
Codex round 3 found the positional binding crossing a clause boundary the other way — a stem followed by its own percentage and then a later plain value from the next clause bound to that value — and proposed a narrow contract in place of the classifier.
The binding is replaced by ruling: `excerpt_binds_metric` accepts a quote with the stem and exactly one number, the value at its sign, or — for a guidance-low or guidance-high row — two numbers joined as a range whose role-named endpoint is the value; `quoted_numbers` is a bare digit-run census, every run counting, and the year, percentage, and positional machinery is deleted.
A sentence that cannot be trimmed to one metric fact loses its row by design, rejected with its excerpt; the bare-year residual closes with the exemptions, and I19 is re-cut to the competing-noun residual alone.
Codex round 4 approved the contract with one record nit: the canonical read as if any year in the quote rejects, where the filter rejects a year *beside* the value and cannot tell whether the admitted value is itself the period ("delivery guidance for 2025" admits a deliveries row of 2025; "for 2025-2026" fits the range carve-out).
The canonical now names the contract a syntactic admission filter and its residual the single-number ambiguity — a competing noun the lexicon does not know, or a value that is itself the period or a unit — units and period being unbound by ruling; I19 is broadened to match.
Codex round 5 approved the slice with one wording residue — the prompt line still made the absolute claim — aligned to "beside the value" under the still-uncommitted `portfolio-v17`, no further stamp moving.
Canonical at `portfolio-workflow.md` §Step 6e (the excerpt, sign, page-context, metric-language, one-fact, range, loss, residual, unbound-units-and-period, and shared-primitive sentences), the §Step 6d row list and pointer sentence, `portfolio-analysis.md` §The per-holding pipeline and §Starting parameters row lists, `storage.md`'s persisted-row sentence, the logic-flow's 6d row bullet and 6e activation-legs bullet, and the watch set's stamp line and rejection-split parenthetical.
`PROMPT_VERSION` moves to `portfolio-v17`; no grade, target, floor, or pre-profit stamp moves.
Residual, by ruling: units and period are not bound by the excerpt — a current actual quoted from a prior-period sentence of the right metric passes the legs on the model's typed period, visible in the persisted excerpt.
Twelve tests pinned (nine added, three re-based): the sign reads over minus, U+2212, currency, parenthesis, `+`, range and date hyphens (after a digit, a percent sign, or a closing parenthesis), spaced and en dashes, prose parentheses, and zero; the excerpt edge — the sign, the leading digit, and the parenthesis just outside a digit-leading quote each rejecting, a number just outside the quoted span never counting, and a legitimate digit-leading quote passing; the one-fact contract — five compound sentences (the three Codex rounds' cases among them) rejecting in both orderings for every number they print, the trimmed clause passing, a period token or prior print beside the value rejecting until trimmed, an untrimmable `rose 12% to 141 units` losing its row, both ends of a hyphenated or worded guidance range binding to the role that names them with the wrong role or an actual rejecting, and "in order to" reading as no bookings language; the overlay round-trip re-based to carry an accepted and a rejected row's excerpts, and the store round-trip of a run whose audit carries them; the bounded locator's missing and over-cap rejections; the review's own case — a page stating `41` in a revenue sentence and `141` in the deliveries sentence rejecting a deliveries-`41` row on each leg, accepting the honest `141` row across a whitespace run and line break, and a positive margin row rejecting off `(12)%` where the negative row passes; the stem matcher at word starts (`border` never reads `order`); the activation-legs test re-based to per-leg reasons; the schema's required field and the prompt line; the two inheriting consumers — a positive forward fact rejecting off the accounting `(1.2)` and a positive indicator off `-25%` through its percent render, the negative facts passing; and the boundary test's `(41)` pin moved to the sign test.

### I4 — major: guidance attainment has no ex-ante chronology or deterministic revision-selection policy

`execution_read` discards `published_at` and confidence from guidance rows, retains only `(value, is_range_low)`, and keeps the first same-role bound encountered except that any range low displaces point guidance (`src-tauri/src/portfolio/pre_profit.rs:1038-1070`).
Actuals, by contrast, are deliberately selected by highest confidence and then latest publication (`src-tauri/src/portfolio/pre_profit.rs:1072-1086`).
Pairing subsequently matches only metric identity and reporting period (`src-tauri/src/portfolio/pre_profit.rs:1092-1105`).

Nothing requires guidance to have been published before the actual, before the period end, or before the actual's publication.
An actual-results release that retrospectively repeats the period's guidance can therefore supply both sides of its own attainment test, and multiple legitimate guidance revisions are selected by model / persistence order rather than a stated financial rule such as original guidance or latest pre-actual guidance.
Either choice can flip the 5% miss, 20% material-miss, repeated-miss, conviction, and action-set results.

The code and logic-flow document need one explicit guidance-vintage policy, and the pairing key must enforce it with the already-persisted publication dates.

Ruled 2026-08-28: the vintage policy is the latest ex-ante guidance — the standing guidance at results time — original-guidance and walk-down counting declined.
Ruled 2026-08-28: the chronology has two legs, both binding — a guidance row dated on or before its period end and strictly before the period's earliest actual publication; the FY-normalizes-to-12-31 residual is named, the earliest-actual leg carrying a non-December fiscal year.
Ruled 2026-08-28: the 6d prompt names `published_at` — the quoted page's own publication date, a guidance row's issue date, never the fetch date — and `portfolio::PROMPT_VERSION` moves to `portfolio-v18` (no v17 trail exists to lose).
Ruled 2026-08-28: a same-vintage conflict — the same date, role, and confidence with different values — makes the period not comparable on the guidance and the actual side, the actual side's encounter-order tail going with it.
Ruled 2026-08-28: the execution read gains no counter of vintage-excluded rows — the exclusions are re-derivable from the persisted rows and the stamp attributes the rule.
Ruled 2026-08-28: `ExecutionMiss` carries `bound_published_at` and `actual_published_at` with no serde default, both app stores read to confirm no persisted miss predates the fields.
Ruled 2026-08-28 (Codex round 1): the dedup key gains the publication date and the value — a duplicate is the same source stating the same value on the same date — so a same-source revision reaches the vintage read and a same-page conflict reaches the conflict rule, both pinned through the production validator.
**Resolved 2026-08-28.**
As-built before the fix, `execution_read` kept `(value, is_range_low)` per guidance row — `published_at` and confidence discarded — paired on identity and period alone, and let the first same-role bound encountered stand except that a range low displaced point guidance, so a results release restating the period's guidance beside its actual supplied both sides of its own attainment test and a revised guidance was selected by persistence order.
The fix reads every candidate row per identity and period and selects under the guidance vintage policy.
A guidance row is admissible only when its parsed publication date is on or before the period end (the normalized ISO period the row carries) and strictly before the period's earliest actual publication — the first time the actual became public, not the actual selected, so a restatement's later date never readmits a release's restated guidance.
Among the admissible rows the latest binds, a range low over point guidance at the same date, then the higher confidence (`select_bound`).
The actual is the highest confidence, then the latest published (`select_actual`).
A residual tie between different values on either side is a conflict, and the period is not comparable.
Publication dates parse from the ISO prefix (`published_date`) and compare as dates, an undatable row or period failing closed without a panic, and each miss records the bound's and the actual's publication dates.
The 6d prompt line names `published_at` as the quoted page's own publication date, never the fetch date.
`PRE_PROFIT_PARAMETER_VERSION` moves to `pre-profit-v3` (a v2 read does not mean what a v3 read means; the resume gate refuses a v2 trail, now pinned) and `PROMPT_VERSION` to `portfolio-v18`; no grade, target, or floor stamp moves.
Both app stores were read by copy-out: the prod store holds no portfolio run, and the dev store's one run (attempt 2) carries 33 overlays stamped `pre-profit-v1`, every `misses` array empty and no observation row, so no persisted miss predates the new fields.
Canonical at `portfolio-analysis.md` §Starting parameters (the vintage, chronology, selection, conflict, date-comparison, `published at`, miss-dates, and fiscal-year residual sentences, with the stamp sentence at `pre-profit-v3`), `portfolio-workflow.md` §Step 6d's `published_at` sentence and §Step 6e's pointer sentence, the logic-flow's Execution-read bullet and validation-bullet clause, and the watch set's stamp line and vintage-read line.
Residual, by ruling: the period-end leg reads the normalized ISO period end, loose for a fiscal year not ending in December, where the earliest-actual leg carries the rule.
The reviewer's round approved with four nits, each closed in the slice: the period-normalization test pushes the raw spelling through validation again (the pre-normalizing fixture had made its assert tautological), validation's publication-date check shares `published_date` with the pairing, two double-claim sentences are split, and the §Step 6e pointer is trimmed to a pointer.
Codex round 1 found the policy defeated one seam earlier: the dedup key read identity + role + period + source URL, so an issuer page updated with revised guidance re-offered the stored key and the revision was rejected as a duplicate, and one page offering two values on one date collapsed to the first row seen — persistence order selecting again, the slice's order-independence test having called `execution_read` past the validator.
The fix widens the key to the parsed publication date and the value's bit pattern (`dedup_key`), the rejection reason naming both, so a duplicate is the same fact re-offered and every other row reaches the read; `storage.md`'s key sentence, its consequence sentence, the §Step 6e duplicate sentence, the canonical's same-source sentence, and the logic-flow's duplicate clause mirror it.
Codex round 2 approved the slice.
Fourteen tests pinned (twelve added, two extended) and two fixture helpers re-based to ISO periods and role-aware dates: the results-release case pairing nothing, the existing miss-rule pin carrying the fixture's two dates; a same-source revision entering through the production validator and binding in either candidate order while the exact fact re-offered still rejects as a duplicate; a same-page conflict entering and dropping the period; the latest ex-ante revision binding in either order and the original binding without it; a post-period preview never binding while a row on the period end does; guidance dated on the first actual staying retrospective under a selected restatement, the miss carrying both dates; vintage beating role and a range low winning only at the same date; the same-vintage conflict dropping the period on either side, equal values no conflict, confidence breaking the tie first; dates comparing as dates never strings; an undatable row or period never pairing; the `pre-profit-v3` stamp on the constant and the overlay; the resume gate refusing a `pre-profit-v2` header; and the prompt line naming the date.

### I5 — major: the action decision never receives the model arm's price targets

The action system prompt tells the model to weigh both arms' grades and scores and “the targets' implied upside/downside” (`src-tauri/src/portfolio/pipeline.rs:3508-3522`).
The user prompt renders the model arm's letter and sub-scores only (`src-tauri/src/portfolio/pipeline.rs:3601-3612`), then renders implied moves exclusively from `graded.price_targets`, explicitly labelled as engine targets (`src-tauri/src/portfolio/pipeline.rs:3620-3633`).
It never reads `graded.model_view.price_targets` on the action path.

This means the model can author a materially different forward target band in the intrinsic call, but its own subsequent action call cannot use that numeric forecast; only its derived letter survives into the decision.
The omission conflicts with the canonical statement that price targets exist in both arms and that target-implied upside / downside is in the action evidence set (`docs/portfolio-analysis.md:320-333`; `docs/portfolio-workflow.md:353-355`).
The logic-flow “exact inputs” list mirrors the implementation's ambiguity by naming one implied band after listing both arms (`logic-flow-docs/portfolio-analysis-logic-flow.md:1286-1291`) and should identify the arm or require both.

Ruled 2026-08-28: the action prompt renders both horizons for both arms — the engine's and the model's one-month and twelve-month implied bear/base/bull moves against spot; the one-month leg's methodology stays I10's.
Ruled 2026-08-28: an off-domain model leg (non-finite or non-positive) prints as authored with an `(off-scale as authored)` tag in place of a percentage, and a band authored bear above bull carries `(band inverted as authored)` — the frontend's posture, never reordered, never dropped; I6 owns the upstream domain validation.
Ruled 2026-08-28: one line per arm and per horizon — the engine's lines keep their label and the provenance sentence weighs the engine's moves alone, the model's lines are labelled as its own band authored at interpretation, unvalidated, with no provenance to discount, and the system prompt names both arms and how each is weighed.
Ruled 2026-08-28: `portfolio::PROMPT_VERSION` moves to `portfolio-v19` and no other stamp moves, both app stores' checkpoint headers read by copy-out before the bump.
Ruled 2026-08-28: the engine's one-month line renders `(gap)` where the leg is `None`.
Ruled 2026-08-28: INDEX gains no row — the action-call row already points at the canonical sections.
**Resolved 2026-08-28.**
As-built before the fix, `action_user_prompt` rendered the model arm's letter and sub-scores, then one implied-moves line from the engine's twelve-month band alone, labelled engine targets, and never read `model_view.price_targets`, so the model's own authored forecast never reached the rung it then decided.
The fix renders four lines under one usable-spot guard through `implied_moves_section` — the engine's one-month and twelve-month bands, then the model's — each bear / base / bull as a percentage move from spot.
An engine leg the scenario function could not derive prints `(gap)`, the twelve-month line taking the same posture as the ruled one-month line so the two engine horizons read in one register (stated at the plan as an assumption).
A model leg outside the declared domain prints as authored with its tag in place of a percentage, and a band authored bear above bull carries the inverted tag.
The two tags are independent reads: a NaN leg compares false on the inverted predicate, so a NaN bear or bull is never tagged inverted, while an in-domain bear above an off-scale bull carries both.
The off-scale guard reads the derived move as well as the raw value, so a finite authored level whose percentage from spot overflows prints as authored rather than as `inf%` (Codex round 1).
The provenance line is engine-scoped by label and says the model's bands carry none.
The system prompt names the implied upside/downside of both arms' targets, the engine's discounted by their provenance, the model's the verdict's own forward call.
`PROMPT_VERSION` moves to `portfolio-v19`; no grade, target, floor, or pre-profit stamp moves.
Both app stores were read by copy-out: neither holds a `portfolio_checkpoints` table (the prod store has no portfolio run; the dev store's one run predates the checkpoint slice), so no trail of any stamp exists to lose.
Canonical at `portfolio-analysis.md` §Portfolio action (the both-arms, engine-render, and model-render sentences), with `portfolio-workflow.md` §Step 6f's prompt-input sentence, the logic-flow's Priced-digest bullet and its exclusion line's off-scale exception, and the watch set's stamp line.
Three tests pinned (one extended, two added): the digest test reads all four lines, the model's twelve-month base move at the stub's 1.05× differing from the engine's, the engine-scoped provenance line, and the system prompt's both-arms phrase, with neither tag present on a well-formed verdict; a mutated clone pins the engine one-month `(gap)` beside a rendered twelve-month line, the inverted tag on a bear-above-bull band, the off-scale tag on a NaN / negative / zero band with no inverted tag beside it, a NaN bear never tagged inverted while an in-domain bear above an off-scale bull carries both tags, a finite `1e308` base over a $1 spot printing as authored with no `inf` on the page, and no implied line for either arm without a usable spot while the provenance line still renders; and the `portfolio-v19` literal on the constant.
The reviewer's round approved with five notes, four closed in the slice: the NaN-exclusivity claim in the test comment and this record re-cut to the predicate's actual reach with the both-tags case pinned, the spot guard reading finite and positive as ruled, the canonical's model-render sentence split at its two claims, and the logic-flow's Priced-digest bullet trimmed to the inputs with the render rules pointed at the canonical; the fifth, the Display render of an authored off-scale value, needed no change.
Codex round 1 found two items: the off-scale guard read the raw value only, so a finite authored level whose move from spot overflowed the percentage arithmetic rendered `+inf%` — closed by guarding the derived move, the boundary pinned; and the canonical's evidence-set sentence still read as if both arms carried typed provenance while its engine-render sentence said the bands "render discounted" — closed by scoping provenance to the engine in the evidence-set sentence and splitting render from weighting, the render raw and the weighing the model's.
Codex round 2 approved the slice.

### I6 — major: the declared model-arm numeric domains are prompt-only, so out-of-domain values derive letters and enter scoring

The interpretation prompt requires model sub-scores on a 0–100 scale and positive ordered target bands (`src-tauri/src/portfolio/pipeline.rs:3352-3362`).
The schema enforces only `number`, with no range keywords, and the code documents that limitation (`src-tauri/src/portfolio/mod.rs:2256-2312`).
There is no app-side post-validation before the values are persisted and the letter is derived (`src-tauri/src/portfolio/pipeline.rs:1305-1312`; `src-tauri/src/portfolio/engine.rs:1324-1341`).

A finite score such as `10000` or `-1000` becomes an ordinary A or F, while zero, negative, or severely crossed targets persist and enter the outcome scorer (`src-tauri/src/portfolio/outcome.rs:2196-2212`).
The engine baseline remains isolated, but the user-visible model arm, action evidence, retrospective, and calibration population are no longer on the declared financial scale.

The documentation calls this “structurally validated only” while also claiming a shared 0–100 scale and “comparability by construction” (`docs/portfolio-analysis.md:286-295`; `docs/portfolio-workflow.md:334-339`).
Unrestricted judgment does not require accepting values outside the declared domain; app-side finite / range / positivity validation can preserve arm independence without clamping the model to engine outputs.

Ruled 2026-08-29: the model arm's numeric domain is scale and positivity only — each sub-score finite within 0–100 inclusive, each target leg finite and strictly positive; ordering and base-inside-band stay unvalidated (I5's authored-and-annotated posture holds).
Ruled 2026-08-29: an off-domain response is rejected whole under a new `RetryClass::ModelArmDomain` — one bounded re-issue like every content failure, then the hard posture with the class named in the failure detail and a `model_retries` event — never clamped, never annotated-and-persisted, never failed on first sight.
Ruled 2026-08-29: the gate sits inside the live adapter's retry closure alone, through an extracted `decode_interpretation`; no second check at the pipeline call site.
Ruled 2026-08-29: `portfolio::PROMPT_VERSION` moves to `portfolio-v20` with its history paragraph, and the model-arm paragraph gains one enforced-domain clause per field family; no other stamp moves; both app stores read by copy-out before the bump.
Ruled 2026-08-29: the scorer keeps a fail-closed read — a model band any leg of which is non-finite or non-positive reads as no band, excluded from the model read and the paired head-to-head.
Ruled 2026-08-29: no frontend change; BUILD's §Awaiting a ruling item "Unannotated off-scale model-arm renders" closes by this slice, since a `portfolio-v20` record cannot carry a model value outside its declared scale.
Ruled 2026-08-29: Trade Opportunities' two-arm sentences align to the same structural-plus-domain wording in this slice, the enforcement mechanism pointing at the Portfolio canonical.
Ruled 2026-08-29: INDEX gains a model-arm-domain row at session end.
**Resolved 2026-08-29.**
As-built before the fix, the interpretation prompt stated the 0–100 scale and target positivity, the schema enforced `number` alone, and `ModelView` took the decoded values straight to `grade_from_subscores` and the store, so a finite `10000` derived an ordinary A and a zero or negative target persisted into the outcome scorer.
The fix adds `validate_model_arm` beside the schema — the four sub-scores finite within 0–100 inclusive, the six target legs finite and strictly positive, every violation named with its authored value rather than the first alone.
`decode_interpretation` calls it inside the live adapter's retry closure after the parse, contexting an off-domain response with the new `RetryClass::ModelArmDomain`, so the bounded retry-once re-issues it exactly once and a second failure names the class.
Ordering is outside the domain: an inverted band stays authored and annotated.
The I5 render guard stays as the action prompt's fail-closed read, reachable behind the gate only for a finite positive leg whose move from spot overflows the percentage arithmetic (its round-1 case) and never for a non-finite or non-positive leg.
The scorer reads a band as no band when any leg is non-finite or non-positive or when its return-space edge or interval score overflows, excluding it from the arm's read and the paired head-to-head through one shared `band_read`, and every mean the scoreboard persists — the cohort returns included — reads absent rather than infinite when its sum overflows through one shared `finite_mean` (Codex rounds 1 and 2), a per-symbol overflow poisoning the cohort field it feeds rather than shrinking its population under the reported count (Codex round 3).
`PROMPT_VERSION` moves to `portfolio-v20` and the model-arm paragraph names each domain as enforced; no grade, target, floor, or pre-profit stamp moves.
Both app stores were read by copy-out: neither holds a `portfolio_checkpoints` table (prod: no run; dev: one run predating the checkpoint slice), so no trail exists for the stamp to refuse.
Canonical at `portfolio-analysis.md` §The holding verdict (the domain, rejection, and ordering sentences) with §Outcome learning's no-band and finite-mean sentences, §Portfolio action's re-cut render sentences, `portfolio-workflow.md` §Step 6f's pointer sentence, `local-models.md` §The local-model adapter seam's class sentence and its two-arm paragraph, the logic-flow's three model-boundary lines and its exclusion-line exception, the Trade Opportunities doc's three two-arm sentences with its workflow's three and its logic-flow's four, and the watch set's stamp line and fired-retry class sentence.
Eight tests pinned (seven added, one replaced): the validator admits the scale edges, a tiny and a huge finite price, and an inverted band, and rejects nine off-scale values naming each while the in-domain leg beside them stays unnamed; the decode accepts the stub's own arm, rejects a four-violation response under `ModelArmDomain` with every field in the detail and the stage-scoped message, accepts an inverted band, and keeps `SchemaParse` for malformed content; the prompt carries the three enforced-domain clauses; the scorer scores the engine band on three episodes while only the in-domain model band scores and pairs, excludes an in-domain `1e308` band over a $1 spot from the model read and the pairing while the engine band still scores, and reads two near-max finite bands' mean as absent rather than infinite (Codex round 1); two holdings with near-max returns still count as unique while their cohort mean reads as absent (Codex round 2), and a symbol whose own mean overflows leaves the field absent under a count of two while another symbol's present relative-return leg still averages (Codex round 3); and the `portfolio-v20` literal.
The reviewer's first round rejected on one docs claim: two sentences said the I5 render guard was unreachable behind the gate, overstating its reach — closed by re-cutting both, and ruling 6's justification with them, to the residual overflow case.
Its three code-quality notes: the stacked failure-detail phrasing, closed by trimming the decode's outer context to the stage alone; the validator tests placed above the module's glob import, closed by relocating them; the logic-flow's inline restatement of the domain beside its pointer, left as the corpus's summary register.
The reviewer's second round approved the slice with that one note standing.
Codex round 1 found three items.
The scorer's guard read the input alone, so an in-domain band whose return-space edge or Winkler penalty overflows — `1e308` over a $1 spot — reached the means as infinity and would persist as `null`, I5's own lesson restated — closed by reading the derived values through one shared `band_read` that excludes the band and the pair on any non-finite edge or score, by every mean reading absent rather than infinite when its sum overflows, and by a pin over a single overflowing band and two near-max finite bands.
§Portfolio action's I5 sentences still called the model's bands unvalidated and the off-scale tag reachable for any off-domain leg, the logic-flow glossary still read structurally-only, and the action prompt itself told the model its bands were unvalidated — closed by re-cutting each to the declared-domain gate and the residual overflow reach, the prompt change riding the slice's own `portfolio-v20` stamp.
Trade Opportunities' workflow and logic-flow mirrors still read structurally-only with no value bound — closed by aligning their seven lines under ruling 7.
Codex round 2 found one item: the finite-mean claim was broader than the code, the cohort returns still averaging through a raw sum that two near-max finite returns overflow into a persisted `null` — closed by hoisting `finite_mean` to the module and routing the per-symbol and cross-symbol cohort means through it, the canonical sentence naming every mean the scoreboard persists, pinned over two near-max holdings.
Codex round 3 found one item: a per-symbol overflow dropped that holding from the cross-symbol mean while `unique_holdings` still counted it, a mean over fewer holdings than reported — closed by a `FieldMeans` accumulator that poisons the field on any per-symbol non-finite mean over present inputs, a symbol with no inputs for a relative-return field still contributing nothing, the canonical sentence stating the distinction, pinned over two same-symbol near-max episodes beside a third holding.
Codex round 4 approved the slice.

### I7 — minor but whole-job kill radius: the fund-weight adapter admits string NaN into the percentile panic

Claude Code's generic panic note correctly identified that NaN can reach `percentile`, but the production ingress is not merely hypothetical arithmetic.
`weights_from_value` accepts string weights because FMP normally serves strings such as `"97%"`; Rust's `parse::<f64>()` also accepts `"NaN"` / `"NaN%"`, and the adapter performs no finiteness or range check (`src-tauri/src/fmp.rs:6311-6337`).
The shaped fetch treats any non-empty parsed vector as successful (`src-tauri/src/fmp.rs:5644-5675`).

A NaN sector weight makes `covered` and the composite yield NaN, bypasses `covered <= 0.0`, and can surface as apparently full coverage through `covered.min(1.0)` (`src-tauri/src/portfolio/fund.rs:422-447`).
The history then supplies NaN raw multiples to `percentile`, whose `partial_cmp(...).expect("finite percentile inputs")` panics (`src-tauri/src/portfolio/engine.rs:1575-1631`).
Because the job has no panic containment, one drifted string weight can terminate the entire hours-long run without its normal failed terminal state or resumable in-session offer.

Reject every non-finite or out-of-range weight in the adapter and defensively validate the composite / observation vectors before sorting.
The engine-side clause — the composite guard and the observation-vector filtering — landed with the panic-posture slice (2026-08-28; recorded under §Priority-3 minor findings); the adapter leg, `weights_from_value`'s finiteness / range rejection, remains I7's.

Ruled 2026-08-29: a weighting row's served percent is usable only when finite and within 0–100 inclusive — the `FundData` fraction contract as written; a failing row drops like an unreadable one, and an all-dropped non-empty body reads malformed.
Ruled 2026-08-29: a body with some rows dropped keeps its `ok` tracker row with no detail, every existing shaper's posture (the `ok`-row dropped-count detail stays a carried item).
Ruled 2026-08-29: no stamp moves — an adapter integrity rule on I14's precedent.
**Resolved 2026-08-29 (group 1, with I9 and I16).**
`fmp::weights_from_value` drops a row whose served percent is not finite or lies outside 0–100 inclusive, so an all-unreadable body reads malformed at the callers as before and a partially dropped one keeps its `ok` row.
Before the guard a NaN United States row read as a 100% US share — `us_share` sums then takes `min(1.0)`, and Rust's `min` returns the non-NaN operand — so the adapter leg is what closes the ≥ 70% guard, not the engine clause.
The engine holds the line for any other producer: `fund::top_weights` and `fund::exposure_basis` skip a non-finite weight (with I16), beside the composite's existing skip.
Two tests pinned: the shaper over `"NaN%"`, `"inf"`, `"1e999"`, `"-5"`, `250`, and the kept endpoints `0` / `"100%"`, and one mock-server pull where the only United States row is `"NaN%"` — the classification's US share reads 0% where it read 100%, the all-unreadable sector body `malformed` with its gap, the partially readable country body `ok`.
Canonical at `data-sources.md` §Financial Modeling Prep (the weighting-row and partial-drop sentences), `portfolio-analysis.md` §Asset eligibility pointing there, mirrored in the logic-flow's fund-requires bullet.
No stamp moves.

### I8 — minor: the priced-fund prompt computes US share differently from the engine guard

The engine sums all recognized aliases — `united states`, `united states of america`, `usa`, `u.s.`, and `us` — and caps the aggregate (`src-tauri/src/portfolio/fund.rs:367-382`).
The interpretation prompt instead takes only the first country label containing `"united states"` (`src-tauri/src/portfolio/pipeline.rs:3230-3244`).
The production adapter's own test demonstrates a valid `"US"` row (`src-tauri/src/fmp.rs:2480-2533`), so a fund can pass the deterministic ≥70% US guard at 97% while the model prompt says `US share: (gap)`.
Multiple US-alias rows are likewise summed by the engine but truncated to the first match by the prompt.

This is a direct mismatch with the logic-flow claim that the fund's US country-weight share is rendered (`logic-flow-docs/portfolio-analysis-logic-flow.md:795-800`).
The prompt should consume the already-computed classified share or a shared helper, not reimplement the label policy.
Ruled 2026-08-29 (group 3, with I10, I12 and I19 under one `PROMPT_VERSION` bump): the prompt reads the shared helper — `fund::us_share`, the guard's own function, made public — rather than a share plumbed onto the engine output, since a persisted field for a render alone would touch the store and the frontend type.
Resolved 2026-08-29: the FUND CONTEXT line renders `fund::us_share(&f.fund)` — every alias summed, capped at 1, `(gap)` on no weightings — so the model sees the share the ≥ 70% guard read; canonical at `docs/portfolio-analysis.md` §Asset eligibility, mirrored in the logic-flow's fund-metrics US-share bullet.
One test pinned: a `US` row renders 97%, three aliases sum to 80%, an over-served set caps at 100%, and no weightings render `(gap)`; the existing fund-prompt test now asserts the fixture's 99%.
`PROMPT_VERSION` moved to `portfolio-v22` with the group.
Reviewer round 1 (2026-08-29): approve-with-nits, two folded in here — the prompt test renamed `the_priced_fund_prompt_renders_the_guards_us_share_and_both_horizons_methodology` for the I10 assertion it also carries, and `us_share`'s doc naming `US_LABELS` in plain code (a private intra-doc link from a public fn would warn under `cargo doc`); the round is recorded whole under §I19.
Codex round 1 (2026-08-29): no finding here — the shared US-share render read correct under static inspection; the round is recorded whole under §I19.

### I9 — minor: the fund-specific sector-P/E adapter bypasses the suite's established integrity guards

The main sector-P/E adapter rejects an off-board response and drops non-positive or greater-than-100 aggregate P/Es because near-zero earnings denominators have produced live values around 461 (`src-tauri/src/fmp.rs:649-677`, `709-715`).
The fund-specific snapshot / history adapter accepts a supplied exchange that disagrees with the requested exchange, accepts a missing date, and accepts any parseable P/E; it merely echoes the requested exchange when the field is absent (`src-tauri/src/fmp.rs:6340-6374`).
Only non-positive and non-finite P/Es are later excluded by the blend (`src-tauri/src/portfolio/fund.rs:387-407`), so implausibly high values and duplicate off-board rows can influence the fund valuation and target anchors.

The fund path should share the same exchange-identity, date-readability, and plausible-P/E contract as the suite's sibling adapter; otherwise “usable P/E” means materially different things across the same job without the logic-flow document saying so.

The date-readability leg landed with I14 (2026-08-28; the canonical render and the undatable-row drop), leaving the exchange-identity and plausible-P/E legs.
Ruled 2026-08-29: the sibling's rule as written — every row's `exchange` present and equal to the requested board, any failing row reading the whole body malformed with the served board named, the `Ok(vec![])` return keeping the snapshot's bounded date walk-back unchanged.
Ruled 2026-08-29: an out-of-band print — non-finite, non-positive, or above `SECTOR_PE_MAX` — drops at the shaper (the fund `SectorPe.pe` is required, so there is no kept-with-`None` shape), accepting that an earlier in-quarter in-band print backs a sample an artifact print held; `blend_sector_pes`'s own filter stays as belt-and-braces.
Ruled 2026-08-29: no stamp moves.
**Resolved 2026-08-29 (group 1, with I7 and I16).**
`fmp::sector_pe_rows_from_value` returns `Err` naming the served board when any row's `exchange` is absent or disagrees with the requested one, and keeps a print only when finite and inside `(0, SECTOR_PE_MAX]`; both callers route the `Err` to a `malformed` tracker row carrying the reason, the `Ok(vec![])` return contract unchanged.
The former echo-the-request test is replaced by one mock-server pin: the ceiling kept and `461`, `0`, a negative, and `"NaN"` dropped on an `ok` row; an off-board row and an exchange-less row each reading the body malformed with the served board (or `<absent>`) named, on the snapshot and the history; an all-out-of-band body reading malformed.
Canonical at `data-sources.md` §Financial Modeling Prep (the exchange-identity and plausible-band sentences), `portfolio-analysis.md` §Asset eligibility pointing there, mirrored in the logic-flow's fund-valuation equation line.
No stamp moves.

### I10 — minor: the one-month target's methodology never reaches the model or the UI

The engine authors a methodology line for both horizons (`engine.rs::build_price_targets`), and `portfolio-analysis.md` §The holding verdict specifies the engine's scenario outputs "with their methodology and assumptions exposed" for both.
The interpretation prompt renders the twelve-month targets with their methodology but the one-month targets as bare numbers (`src-tauri/src/portfolio/pipeline.rs:3522-3533`).
The Portfolio page's "Target methodology" reveal renders only `twelve_month.methodology` (`src/components/PortfolioView.vue:2001-2006`), and the component spec's default fixture carries `one_month: null`, so it cannot catch the omission (`tests/components/PortfolioView.spec.ts:44`).
Surfaced by the Codex round on the `targets-v5` slice, which changed the one-month band's basis — neither the model nor the reader can see which basis a one-month band stands on.
Pre-existing rather than introduced by that slice; it is its own slice (prompt, view, and spec).
Ruled 2026-08-29 (group 3): the prompt, the card's reveal and the spec move together; the spec's base fixture carries a one-month target with a methodology so the default card exercises both horizons, a null-one-month case beside it; the action call's implied-moves block keeps provenance, not methodology (I5's shape).
Resolved 2026-08-29: the ENGINE ONE-MONTH TARGETS line carries `methodology:` like the twelve-month line; the Portfolio card's Target methodology reveal renders each authored horizon's methodology, one-month first; the logic-flow's prompt-input and card bullets say so.
Pinned: the prompt-level test asserts the one-month methodology line follows its targets; the spec asserts both paragraphs after the click, one-month first, and a null one-month renders only the twelve-month paragraph.
`PROMPT_VERSION` moved to `portfolio-v22` with the group.
Reviewer round 1 (2026-08-29): approve-with-nits, nothing folded in here — the fixture-masking check passed, no spec asserting on the newly rendered 1-mo tile or its numbers; the round is recorded whole under §I19.
Codex round 1 (2026-08-29): no finding here — the prompt and UI methodology disclosure read correct under static inspection; the round is recorded whole under §I19.

### I11 — minor: a scenario-target parameter-version change has no cross-run continuity attribution

The grade-band version has one: the dossier loads the prior audit's `grade_parameter_version` (`src-tauri/src/portfolio/dossier.rs:383`, `:1133`), the input delta carries a "grade bands recalibrated" row (`src-tauri/src/portfolio/pipeline.rs:2547`), and the continuity prompt adds the recalibration NOTE on a mismatch (`pipeline.rs:3679-3688`).
The target version has none: the loader discards `audit.target_meta.parameter_version`, the input delta compares only the twelve-month base target, and no NOTE fires.
A run whose targets moved on a version bump alone — `targets-v4` → `targets-v5` widens every one-month band with no input change — can therefore have that move attributed to company evidence or a self-correction, and a self-correction marks `thesis_changed` and can open a successor outcome episode.
Pre-existing since `targets-v3` → `targets-v4` (2026-08-13); surfaced by the Codex round on the `targets-v5` slice.
The fix mirrors the grade mechanism — a `prior_target_parameter_version` on the dossier, an input-delta row, and the NOTE — as its own slice.
Ruled 2026-08-29 (group 4, with I13 under one `PROMPT_VERSION` bump): the target stamp gains the grade stamp's history — `engine::SCENARIO_TARGET_PARAMETER_HISTORY`, read by `target_parameter_change` per branch as the union of the horizons the rows after the prior's stamp touched — holding a single `targets-v5` anchor row, since the store is wiped before the first run under it and no earlier prior can exist; the current stamp, an unrecognized one (`targets-v4` included) and a prior with no target record (`target_meta: None`) read `None` and stay silent.
Ruled 2026-08-29: `prior_target_parameter_version` rides `PriorHolding` and `HoldingDossier` off the prior audit's `target_meta.parameter_version`, and the delta row and the continuity NOTE render iff the prior is priced with a stamp preceding a history row touching its branch, naming exactly the union of horizons.
Ruled 2026-08-29: `PROMPT_VERSION` moves to `portfolio-v23` with the group, the only axis — the target function itself is unchanged.
Resolved 2026-08-29 (group 4, with I13): `engine::TargetHorizons` and `target_parameter_change` (over an explicit history in `target_parameter_change_in`, so the union rule is tested past the anchor row), the prior stamp carried onto the dossier, the input-delta row ("scenario-target parameters changed (<prior> -> targets-v5) — the <horizons> can move with no input change") and the NOTE rendered after the grade NOTE; canonical at `docs/portfolio-analysis.md` §Starting parameters (the Scenario targets bullet), mirrored at `portfolio-workflow.md` §Step 6b / §Step 6f and the logic-flow's continuity block.
Pinned: the history's last row is the current stamp; every reachable stamp is silent on both branches, and a never-priced prior whatever it carries; the union rule over an explicit three-row history; the row and NOTE text on explicit horizons; and `prior_verdict_for` carrying the stamp off `target_meta`.
`PROMPT_VERSION` moved to `portfolio-v23` with the group.
Reviewer round 1 (2026-08-29): approve-with-nits, every criterion passing on evidence and the scope report judged honest — three of six nits folded in: the `restamp` helper had landed between the gated evaluator's rustdoc and its signature (the doc block now attaches to the evaluator again, the helper above it with its own), the `targets-v6` obligation is named on both history tests (the v5 stamp stops being silent then, and the positive render through the wiring becomes reachable), and the ledger paragraph's stacked parentheticals are reworded; declined for symmetry with the grade mechanism they mirror — the tuple-shaped history row (`GRADE_PARAMETER_HISTORY`'s shape), the delta row's stamp re-read (the grade row's shape), and this record's semicolon-joined ruling lines (the record's existing register); the positive-direction render stays untested until a second history row makes it reachable, as the scope report states.
Codex round 1 (2026-08-29): request-changes on two I13 findings, recorded under §I13; nothing on I11 — the history, the prior-audit carry, the branch-aware union, the prompt / delta wiring and the `portfolio-v23` resume boundary read coherent under static review, the positive production render noted as dormant until a future target-version row exists, as the scope report states.
Codex rounds 2 and 3 (2026-08-29): recorded under §I13 — one further I13 finding fixed, then approved with no remaining findings; nothing on I11.

### I12 — minor: the ledger crossing renders flatten a sub-basis-point expense ratio, and the two sites disagree on the threshold's precision

Both crossing renders print the observed value at four places — the input-delta entry (`src-tauri/src/portfolio/pipeline.rs:2424`) and the 6f ENGINE CONDITION CROSSINGS section (`pipeline.rs:4308`) — so an expense ratio below one basis point prints `0.0000` there while the direct render (`fmt_expense_ratio`, `pipeline.rs:3787`) extends its precision.
The edge is reachable: the adapter divides any numeric `expenseRatio` by 100 unquantized (`src-tauri/src/fmp.rs:6297-6300`), and a ledger threshold is any finite value (`pipeline.rs:1548`).
The two sites also disagree on the threshold: the input-delta entry prints it at four places and the 6f section shortest-round-trip (`{}`), so one crossing states its threshold two ways in one prompt.
`ConditionCrossing` carries no series, so the fix is series-agnostic — one shared formatter at both sites, four places extending where a nonzero value would round to zero, the expense-ratio render's own rule — and a `PROMPT_VERSION` event.
Surfaced by Codex rounds 1–2 on the `portfolio-v14` expense-ratio slice and recorded there as deferred; ruled its own slice 2026-08-27.
Ruled 2026-08-29 (group 3): one shared formatter on the expense-ratio render's places rule, at both sites, for observed and threshold alike; render precision stays out of `docs/` on the `portfolio-v14` precedent — the code doc-comment and this record are its home.
Resolved 2026-08-29: `render_places` is lifted out of `fmt_expense_ratio` and `fmt_crossing_value` prints a crossing value on it — four places extending to ten where a nonzero value would round to zero — at the input-delta entry and the 6f ENGINE CONDITION CROSSINGS section.
Two tests pinned: the formatter over zero, a signed value, a sub-basis-point value, and the half-basis-point boundary (which rounds away from zero at four places, as the expense-ratio rule stands); and both renders stating `observed 0.00004 vs threshold 0.00003` and `observed -0.4500 vs threshold -0.4000` identically.
`PROMPT_VERSION` moved to `portfolio-v22` with the group.
Reviewer round 1 (2026-08-29): approve-with-nits, nothing folded in here — no other `vs threshold` render exists and no test had pinned the old forms; the round is recorded whole under §I19.
Codex round 1 (2026-08-29) found the per-value precision comparison-unsafe: `0.00006` against `0.00005` rendered `0.0001` against `0.0001`, a real crossing shown as equality, and `0.00004` against `0.00005` rendered at two precisions, the gap exaggerated.
Closed: `fmt_crossing_pair` renders observed and threshold together at one precision — the pair's `render_places` floor, extended to ten places until the two strings differ whenever the values do — at both sites, `fmt_crossing_value` retired; rounding is monotone, so differing strings keep the values' order, and values closer than half a unit at ten places render alike, below any margin's meaning.
The pins re-based: the pair test covers an equal pair, signed values, a value beside zero, the `0.00006`-against-`0.00005` case each value alone would print as `0.0001`, the observed floor governing the threshold, a negative zero reading as zero, and the ten-place floor; the prompt-level test renders `observed 0.00006 vs threshold 0.00005` at both sites and refuses `0.0001 vs threshold 0.0001`.
No stamp moves off the round — the render stays under `portfolio-v22`.
Codex round 2 (2026-08-29) found the ten-place floor still comparison-unsafe: the engine's comparison is exact (`value > threshold + margin`, the margin clamped to zero or above, zero valid), so an `above` crossing at `0.1` against `0.1000000000001` emits while the render read `0.1000000000 vs 0.1000000000`; the round-1 "below any margin's meaning" claim overclaimed.
Closed: past ten places the pair falls back to the shortest round-trip render, which differs for any two distinct values and keeps their order; the two floor pins re-based to the fallback, with a one-ulp crossing pinned to render distinct and ordered.
No stamp moves off the round.
Codex round 3 (2026-08-29) found the strings-differ stop test still unsafe on a sign crossing: `-1e-12` against zero renders `-0.0000000000` beside `0.0000000000` at ten places — distinct strings that read as equal — and stopped there, while a zero-margin `below` condition is an exact crossing.
Closed: the stop test reads the rendered pair back as numbers and accepts only a pair that orders as the values do, so that case and any like it fall through to the round-trip render; pinned for the negative-against-zero, zero-against-negative and symmetric sign-crossing pairs, with every pinned pair asserted to order as its values do.
No stamp moves off the round.
Codex round 4 (2026-08-29) found documentation residue: the `portfolio-v22` history paragraph and the formatter's doc comment still stated the round-1 invariant ("until the strings differ"), which round 3 had shown insufficient.
Closed: both, and the pair test's comment, now state the enforced invariant — the rendered pair, read back as numbers, orders as the values do, the shortest round-trip render past ten places; the round-1 "Closed" line above stands as history.
No stamp moves off the round.
Codex round 5 (2026-08-29) found the pair test's comment overstating the guarantee — "a sub-basis-point gap is never exaggerated" — where the formatter guarantees order and one shared precision, never distance: `0.0000451` against `0.0000449` renders `0.00005` against `0.00004`, the gap magnified.
Closed: the comment states the guarantee as order and, on the fixed-decimal branch, one shared precision, distance named as not guaranteed; distance fidelity is not adopted as a requirement — the render is the crossing's comparison aid beside its confirmed / first-breach tag, and the values persist exact on the crossing itself.
No stamp moves off the round.
Codex round 6 (2026-08-29) found the replacement wording absolute — "as any fixed-precision decimal render magnifies it" — where rounding can magnify, preserve, or shrink a gap: `0.0000649` against `0.0000451` renders `0.00006` against `0.00005`, the gap shrunk.
Closed: the comment and the round-5 line above give both examples and say the render promises neither.
No stamp moves off the round.
Codex round 7 (2026-08-29) found the shared-precision claim unqualified — the round-trip fallback past ten places prints each value at its own shortest exact form (`0.000000000001` against `0`), so one shared precision holds on the fixed-decimal branch alone.
Closed: the comment and the round-5 line above scope the shared-precision guarantee to the fixed-decimal branch and name the fallback's form.
No stamp moves off the round.

### I13 — minor: an equity-source flip steps debt/equity and price/book past the continuity gate

Debt/equity and price/book read `total_equity`, which is FMP's latest quarterly balance sheet when that leg returns it and SEC's annual `stockholders_equity` when it does not (`src-tauri/src/portfolio/dossier.rs` `merge_financials`, the equity fill; `docs/portfolio-analysis.md` §Starting parameters, the leverage leg).
The FMP balance-sheet leg is fail-soft, so a gap on one run and a return on the next flips the equity leg between a quarter-end instant and a year-end one, and both series step with nothing having happened — the same size class as the flow-basis step the continuity gate exists for.
The gate does not see it: `ConditionEvalState.authored_statement_basis` tracks the flow basis, which the ledger-basis slice narrowed to SEC flow fills, and the equity source was never stamped — a TTM-to-TTM pair of runs with a balance-sheet gap between them was uncovered under the old stamp too.
Price/book is the reachable half: it keys its observation on the marks' trading day, so a stepped level confirms a breach in two distinct closes; debt/equity is filing-cadence and slower.
No persisted record carries the equity source, so no watch-set read can catch a flip after the fact.
The fix mirrors the basis-flip gate on the two instant series: an `equity_source` stamped on the financials at the merge, an `authored_equity_source` on the evaluation state, and the same one-pass-unevaluable-and-restamp treatment on a change; whether the prompt's basis line names the source (a `PROMPT_VERSION` event) is the slice's call.
Surfaced by Codex round 2 on the ledger-basis slice (`portfolio-v15`) and recorded there as not actioned; ruled its own slice 2026-08-28.
Ruled 2026-08-29 (group 4, with I11): `EquitySource { FmpQuarterly, SecAnnual }` is stamped on the financials at `merge_financials` — the one seam that knows what filled the equity — and `ConditionEvalState.authored_equity_source` carries it on the two instants alone with no serde default; the gate on `statement_derived() && !flow_basis()` mirrors the basis gate (unevaluable, re-stamp, streak reset, once per flip), a pass on which both stamps change is one pass with both adopted, and a flow series never reads the equity stamp.
Ruled 2026-08-29: the sweep clears `equity_source` beside `statement_basis` — it is not the authority on either — and the prompt's basis line names the source, a `PROMPT_VERSION` event (`portfolio-v23`, the group's only axis).
Resolved 2026-08-29 (group 4, with I11): the stamp at the merge, the eval-state field, one `restamp` read shared by both stamps in `evaluate_ledger_conditions_gated`, the sweep's clear, and the basis line's instants sentence ("supplied this run by …", or unevaluable where no equity line reached the engine); canonical at `docs/portfolio-analysis.md` §The position thesis ledger and §Starting parameters (the leverage leg), mirrored at `portfolio-workflow.md` §Step 6f, `storage.md` §Local Analysis Suite Storage and the logic-flow's vocabulary bullet; the watch set gains the equity-source flip watch.
Pinned: a price/book source flip cannot cross and re-evaluates normally once adopted; a debt/equity source flip is unevaluable rather than an immediate filing-cadence confirmation; a flow series adopts no equity stamp; a simultaneous basis-and-source flip is one pass naming both; a first evaluation adopts both stamps and the sweep's cleared surface moves neither; the merge stamps FMP / SEC / none; and the basis line names the source in every shape.
`PROMPT_VERSION` moved to `portfolio-v23` with the group.
Reviewer round 1 (2026-08-29): approve-with-nits, recorded whole under §I11 — the gate's rewrite read as preserving the basis semantics byte-for-byte on the single-flip note, every production financials reaching the evaluator through `merge_financials` (the sweep's cleared surface excepted, by ruling), and no flow series able to adopt or trip on the equity stamp; the `restamp` rustdoc fix is the one fold-in here; two follow-up candidates left standing as the implementer recorded them — the audit's sources line naming the equity source, and the sweep's FMP-only debt/equity refresh evaluating past a healed balance-sheet gap under the full-pass-owns-the-gate posture.
Codex round 1 (2026-08-29): request-changes, both findings verified against the code and fixed under the same stamp.
P1 — new and superseding quantitative conditions persisted `ConditionEvalState::default()` at Step 6g, so a run-1 instant condition carried no stamp until run 2's first evaluation, which adopted the current source silently: a between-run sweep streak accumulated on FMP's equity could then confirm against a full pass whose equity leg had fallen to SEC's — the exact transition the gate exists for — and the watch set's run-2 assurance was false (the same blindness the pre-existing basis stamp carried).
Fixed: `ContinuityStamps` (the authoring surface's basis and equity source, `ContinuityStamps::of(&dossier.financials)` at both 6g call sites) is written onto every new or superseding quantitative condition per series — the basis on every statement-derived series, the equity source on the two instants alone, the same rule the gate reads — so the first evaluation after a debut has a stamp to disagree with; the research-less wrapper stamps none, and a condition authored where the surface carried none still adopts at its first evaluation.
Pinned: the three series' starting states off one surface, the wrapper's none, a superseding core re-stamped from the later surface beside a carried-verbatim core keeping its stamp, and the teeth — the debut's debt/equity condition evaluated for the first time on a SEC-sourced surface typed unevaluable, never adopted and compared across the step.
P2 — the sweep's debt/equity reads its own FMP-only statements refresh with the equity marker cleared, so after a full pass whose FMP leg gapped (equity stamped SEC annual) a healed leg stepped the sweep's debt/equity against the SEC-accumulated streak, and a filing-cadence breach confirms at count one; the implementer had recorded the residual under the full-pass-owns-the-gate posture.
Fixed: the sweep withholds a debt/equity condition whose stamped source is not its own (`EquitySource::FmpQuarterly`) — the condition is excluded from the evaluated ledger whole, typed unevaluable with the sources named, no state movement, the filing family reading `unknown` like any allowed-but-unresolvable series — while price/book, rescaled from the stored audit on the stamp's own source, evaluates; the sweep-clears-the-marker ruling stands, augmented rather than reversed.
Pinned: a SEC-stamped debt/equity condition on a fresh 10-Q with a breaching FMP print raises no flag, moves no state and reads the filing family `unknown` with the withhold note, while the FMP-stamped and the unstamped sibling confirm at count one.
Canonical at `docs/portfolio-analysis.md` §The position thesis ledger (both sentences), mirrored at `portfolio-workflow.md` §Step 6g, the logic-flow's 6g and quick-check bullets, and the watch set's equity-source lines; no stamp moves off the round — `portfolio-v23` already covers every row the new field reaches.
Codex round 2 (2026-08-29): request-changes on one finding, verified and fixed — the round-1 withhold excluded only a debt/equity condition stamped with another source, so an unstamped one (authored on a surface with no equity leg, correctly stamped `None`) still evaluated at the sweep off the FMP refresh with the marker cleared, could confirm at count one, persisted unstamped, and left the next full pass to adopt whichever source it found — SEC included — with nothing to disagree with; price/book is not exposed, since an unstamped one has no stored ratio to rescale until a full pass stamps it.
Fixed on the round's first option, the one consistent with the full pass owning the gate: the sweep evaluates a debt/equity condition only when its streak is stamped with the sweep's own `FmpQuarterly` source and withholds it otherwise — another source, or none — each with its own note, the stamp landing at the next full pass; the alternative (the sweep persisting the FMP stamp on its own debt/equity read) declined as the sweep re-stamping what it is not the authority on.
Pinned: the sweep test's unstamped case flipped from confirming to withheld with the no-stamp note, beside the SEC-stamped case, the FMP-stamped one alone confirming.
The watch set's run-1 line narrowed to conditions authored on a surface with an equity leg, and its sweep line names both withheld shapes; canonical sentence and logic-flow bullet updated; no stamp moves off the round.
Codex round 3 (2026-08-29): approved on static review, no remaining findings — the sweep evaluates debt/equity only when stamped `FmpQuarterly`, withholds the SEC-stamped and the unstamped shapes with distinct typed notes and no flag or state movement, the filing family `unknown`, the full pass the sole source-stamp authority, all three cases pinned through the production quick-check path, docs, logic flow and watch set aligned.

### I14 — minor: sector-P/E history dates are stored as served and compared lexicographically

`sector_pe_rows_from_value` (`src-tauri/src/fmp.rs`) stores each `sector-pe-snapshot` / `historical-sector-pe` row's `date` as served, keeping a row without one under an empty date, while `fund::composite_yield_history` (`src-tauri/src/portfolio/fund.rs`) selects each sector's latest print on or before a quarter-end sample date by comparing those strings lexicographically.
A non-zero-padded print — the feed family's documented wire quirk — reads as after every sample date in its own year and is excluded there, then reads as later than a December print and is misselected as the latest for later years, so the fund valuation history's coverage and its composite yield move with nothing having happened.
An empty date sorts before every sample date, so a dateless row always qualifies; it holds its exchange's slot only where no dated print qualifies, since any dated print replaces it and none is replaced by it.
A lone dateless row therefore supplies the historical P/E for its exchange — a semantics no contract states, so the slice's tests cover dateless-only and mixed dated/dateless inputs.
The snapshot blend ignores dates and the history sampler keys on them, so the two consumers must be pinned separately.
No persisted record carries the prints' source form, so no watch-set read can catch a misselection after the fact; `docs/data-sources.md` §Financial Modeling Prep names the family as the as-built exception until this lands.
The fix is the statement-date slice's: store the canonical fixed-width render at the shaper, rule what a dateless row means, and pin both consumers; no stamp is expected to move.
Surfaced by Codex round 1 on the statement-date slice and recorded there as not actioned; ruled its own slice 2026-08-28.

Ruled 2026-08-28: absorbed into the I2 slice.
Ruled 2026-08-28: a sector-P/E row whose date is missing or does not parse joins the dated-row rule as written — dropped at the shared shaper on both endpoints, the snapshot included, an all-unreadable body reading malformed — and every kept date is stored as the canonical fixed-width render.
**Resolved 2026-08-28 (with I2).**
`fmp::sector_pe_rows_from_value` stores `canonical_date`'s render and drops an undatable row; the `Ok(vec![])` return contract holds, so the snapshot's bounded date walk-back is unchanged.
The sampler consumer compares parsed dates (§I2); the snapshot consumer is pinned to read no date.
One test pinned at the shaper off one mock server — the canonical render kept and the dateless, non-date, and impossible-date rows dropped on the history, the snapshot dropping its dateless row, and the all-undatable body reading `malformed` with its cause on both endpoints — beside the sampler pins under §I2.
`data-sources.md` §Financial Modeling Prep now places the family under the rule in place of the as-built exception.
No stamp moves on this half.

### I15 — minor: the conditional topic's activation reason reaches no consumer, and its news-seed branch is unreachable

`AgendaTopic::conditional_reason` (`src-tauri/src/portfolio/research.rs`) records why a conditional topic activated, its doc comment claiming it is logged to the audit so dormancy stays legible.
As-built it is copied into the transient `TopicResearch` and consumed by nothing: the pass brief renders the topic's title and questions, the distillation prompt (`distill.rs`) renders key and title, and `ResearchAuditRecord` carries no reason field — its only reader is a unit test on the mid-loop escalation label.
The technology topic's reason resolution in `build_agenda` compounds it.
The order is the engine pre-flag, then a standing technology-class ledger falsifier, then the qualifying news-feed seed, and `AgendaTriggers::tech_news_seed` is constructed at its one production site (`pipeline.rs`) as fresh symbol news *and* that same standing falsifier.
The seed branch is therefore entailed by the branch tested before it, and `tech_news_seed` is a trigger field that cannot fire.
Behavior is unchanged: the topic fires from the falsifier line, and the seeds ride the pass brief as structured leads.
The exposure is a doc comment that promises an audit leg the audit does not have, and dead plumbing on the agenda — no model or audit read moves whichever branch wins.
The fix is ruled at its plan, one of two shapes.
Wire: persist the reason on `ResearchAuditRecord` beside its topic (rendering it in the pass brief only if the prompt is meant to carry it — a prompt-content change with its stamp), with the reason order made specific-first so every label is reachable.
The wire shape is not Rust-only: it rewrites the logic-flow's Step-6c trigger line, which now records that the label never appears, and adds the persisted reason to the research-artifact inventory at `storage.md` §Local Analysis Suite Storage once the field is a stored contract.
Retire: drop `conditional_reason` from both structs, drop `tech_news_seed` from `AgendaTriggers`, and correct the doc comment.
The retire shape also sweeps the full-run trigger-leg claims that would then read a removed leg: `portfolio-analysis.md` §The per-holding pipeline's trigger list, `portfolio-workflow.md` §Step 6c's, the `news/stock` row of `data-sources.md` §Portfolio Analysis — endpoint surface, both logic-flow sites (the Step-6c trigger line and the inputs bullet), and the trigger comment in `pipeline.rs`.
The quick check's news leg is distinct and stays as built — the pull under a standing falsifier, its evidence-event badge, and the §Starting parameters definition it reads.
Either shape pins a test holding a fresh seed beside a standing falsifier; no stamp moves on the retire shape.
Surfaced by the Priority-3 doc batch's implementation and confirmed by its review rounds; a second Codex round found the label has no consumer, reframing the finding from a reachability fix to this, a third named the retire shape's contract sweep, and a fourth the wire shape's doc legs; ruled its own slice 2026-08-28.
Ruled 2026-08-29 (group 5): retire.
`conditional_reason` leaves `AgendaTopic` and `TopicResearch`, `tech_news_seed` leaves `AgendaTriggers`, and `technology_topic()` takes no reason.
Every reason stays reconstructible from what the audit already persists — the pre-flag on `tech_event_pre_flag`, the standing falsifier in the ledger, overlay eligibility on the overlay record, and a mid-loop escalation being the topic present with neither.
`TopicResearch` is transient and no doc promised the reason on the audit, so no stamp moves.
The wire shape — every fired trigger persisted beside `seed_decisions`, moving `checkpoint-v3` with a `storage.md` inventory row for a field nothing renders — is declined.
Resolved 2026-08-29: the agenda adds the technology topic on the pre-flag or the standing falsifier alone, and `AgendaTriggers` has exactly one literal site (`pipeline.rs`, its tests riding `Default`).
A pinned `build_agenda` test holds a fresh seed beside a standing falsifier — the topic once, a seed alone nothing, no trigger combination twice — and the mid-loop escalation test re-pins on the topic key.
The full-run trigger claims are swept — `portfolio-analysis.md` §The per-holding pipeline's trigger list, `portfolio-workflow.md` §Step 6c, the `news/stock` row of `data-sources.md` §Portfolio Analysis — endpoint surface (the full-run trigger-surface clause alone; the quick-check leg clause stands), both logic-flow sites, and the trigger comment in `pipeline.rs` — while the quick check's news leg and the §Starting parameters conjunction it reads stay as built.
The group's one reviewer round (approve-with-nits) is recorded under §A4, where its nits fell.

### I16 — minor but whole-run kill radius: required persisted floats are finiteness-gated only at the targets

The panic-posture slice's output gate (`engine::price_targets_finite`) exits a holding whose scenario targets are non-finite, closing the exact path its `total_cmp` sorts had opened.
The unreadable-run class is broader than the targets: the dividend shaper accepts finite amounts and sums them unchecked (`src-tauri/src/fmp.rs`, the in-window `sum += a`), so two in-window `f64::MAX` amounts produce an inf forward-dividend figure.
Forward dividends move total returns, never scenario prices, so all six target legs stay finite and pass the gate while the same inf lands in the required `QuickCheckBasis.forward_dividends: f64`, persisted on the normal path (`pipeline.rs`), serialized as `null`, and fails the whole run row's decode at read (`store.rs` loud-skip).
`ImpliedExpectations` is a sibling required-float surface to include in the same audit.
The fix is an audit of every required `f64` the persisted `PortfolioRun` tree carries — validate each before persist, or reject the overflowing aggregate at its shaper — with a store round-trip regression over finite extreme inputs, the test the panic-posture slice declined for the targets alone.
Surfaced by the panic-posture slice's second Codex round (2026-08-28); queued on I10/I11's terms.

Ruled 2026-08-29: the fix is three layers — the dividend shaper rejects an overflowed in-window sum as a drifted body (the recorded gap and a zero leg), every engine derivation the record carries reads as a gap where its arithmetic does not finish as a finite number, and `store::insert_run` decodes its own record before the write, refusing one that would not read back with the holding named under the hard run-persistence posture; no new dependency, the holding named and not the field.
Ruled 2026-08-29: JSON-number pass-throughs — Schwab quantities, FRED prints, EOD closes, the model arm's numbers — are finite by the parser, pinned rather than guarded.
Ruled 2026-08-29: not an `EVIDENCE_FLOOR_VERSION` event on I1's precedent — the stamp keeps a resumed trail's completed holdings under one admission rule, and no readable pre-fix holding changes (a non-finite one is already an unreadable row, a finite one grades identically); the general resume-across-a-rebuild residual is I18's.
Ruled 2026-08-29: no `GRADE_PARAMETER_VERSION` bump for the one readable pre-fix case that changes — an ±inf metric `scale` clamped to a 0 / 100 sub-score now reads absent — since it is unreachable from live prints and the history row would describe a non-event.
Ruled 2026-08-29: the seam's residual is accepted — a resume restores the same record from the trail and fails the same way, bounded by the resume window and the new run's discard.
**Resolved 2026-08-29 (group 1, with I7 and I9).**
The audit enumerated twenty-seven required float positions reachable from `PortfolioRun`: the targets, the model arm, the ledger thresholds and probabilities, `spot`, and every JSON-number pass-through were guarded or parser-finite already; `drivers` was guarded transitively by the target gate; and eleven derivations persisted beside the gate unguarded — the forward-dividend sum, the engine metrics (`scale` clamps ±inf and passes NaN), a crossing's `observed_value`, the pre-flag's threshold, `ImpliedExpectations`, `NarrativeRead`, an execution miss's ratio, the percentile surfaces' interpolation, the roll-up weights, and the fund tilt / exposure basis over a non-finite weight (I7's ingress).
`fmp::ttm_dividends_from_value` rejects an overflowed in-window sum as a drifted body, and the outcome label's total-return sum takes the labeled price-only fallback with an overflow gap.
`engine::finite` normalizes `compute_metrics`, `return_volatility` (the fund's deep-history caller included), the fund drawdown, and the dispersion floor; `resolve_series` reads a non-finite observation unevaluable; `implied_expectations` reads `None` on a non-finite driver or growth; `narrative_vs_reality` returns its overflow reason; `tech_event_pre_flag` requires a finite positive volatility and a finite move; the percentile triples read no surface on a non-finite edge; an overflowed miss ratio is no miss; the roll-up weights read zero over an unusable total; `top_weights` and `exposure_basis` skip a non-finite weight; both `forward_dividends` sites read through `finite`.
`store::insert_run` decodes its own record before the write and refuses one that would not read back, naming the holding, under the hard run-persistence posture; the checkpoint row stays loud-skip at load.
Twelve tests pinned in the slice (fifteen with the reviewer round below): the shaper overflow (the pure sum and the pull's gap + `malformed` row); the outcome fallback; in the engine the metrics / crossing / floor pin, the percentile surface, the implied read, and the narrative + pre-flag pin; the fund tilt; the miss ratio; the roll-up weights; and in the store the serde premise (`null` out, `null` refused into a bare `f64`, `1e999` rejected at parse), the refused write naming the holding with no row landed, and the feed-extreme run round-trip the panic-posture slice declined.
Canonical at `portfolio-analysis.md` §Evidence floor (the every-other-derivation sentence) and §Failure posture (the validating write and its residual), mirrored at `storage.md` §Local Analysis Suite Storage, `data-sources.md` §Financial Modeling Prep (the dividend windower), `portfolio-analysis.md` §Starting parameters and `portfolio-workflow.md` §Step 6b (the miss ratio), and the logic-flow's targets, execution-read, roll-up, and run-listing lines.
No stamp moved on the slice's original scope; the reviewer round's dated-EOD leg moved the evidence-floor stamp to `evidence-floor-v4` (Codex round 1, below), the prompt, grade, target, and overlay stamps untouched.
Reviewer round 1 (2026-08-29): approve-with-nits, five notes — the episode store's required floats and the unfiltered EOD close parse, the Schwab lot-netting sums, the §Failure posture refusal sentence over-claiming the holding named, three colon- or semicolon-joined doc sentences, and `ScenarioSet.raw_observations` beside a `None` surface.
Ruled 2026-08-29 (reviewer round 1): the episode store's required floats join the slice as an I16 leg — the 180-day and the deep dated EOD parses apply the quote's usability rule, the label pass admits usable closes only at load and at merge, and a window whose price return or drawdown does not finish finite writes no label that pass.
Ruled 2026-08-29 (reviewer round 1): the Schwab lot netting fails the Step-2 pull naming the symbol when a netted quantity, cost basis, or market value is not finite — before any per-holding work, rather than the run refusing at persist.
Ruled 2026-08-29 (reviewer round 1): `ScenarioSet.raw_observations` keeps its admitted-sample count beside a `None` surface — the carry is recorded, and no render reads the count on the carry path — accepted as cosmetic.
The round's two doc notes are applied: the joined sentences split one claim per line, and the §Failure posture refusal sentence re-cut to name the holding only where the value sits in a per-holding record.
Off the round: `fmp::dated_eod_from_value` and `eod_prices_from_value` drop a close that is not finite and positive; `outcome::SeriesCtx` admits usable bars only at load and merge (`usable_bars`), so the price-bar cache never holds one; the label pass computes the drawdown beside the return and writes no label when either is not finite; `schwab::Holdings::normalized` returns `Err` naming the symbol on a non-finite netted sum, propagated by the run's Step-2 pull and the standalone snapshot.
Three more tests pinned: both EOD parses over zero / negative / positive closes with the all-unusable body on the deep pull's `malformed` branch; a label pass over a source serving zero closes mid-window (drawdown 0, never −100%, the cache holding no unusable bar); the netting refusal naming the symbol.
The episode leg is canonical at `portfolio-analysis.md` §Evidence floor (the dated-EOD sentence) and §Outcome learning (the no-label sentence), the netting refusal at `schwab-integration.md` §What is pulled, mirrored in `data-sources.md` §Financial Modeling Prep and the logic-flow's label and normalization lines.
Codex round 1 (2026-08-29) found five items.
Ruled 2026-08-29 (Codex round 1): the dated-EOD usability rule IS a floor-rule change on I1's precedent — a served zero close was admitted under v3 and read as a −100% return into volatility, momentum, and drawdown, so a readable completed holding's verdict can differ — and the stamp moves to `evidence-floor-v4`; the earlier no-stamp ruling stands for the slice's original scope, superseded for this leg alone.
Ruled 2026-08-29 (Codex round 1): a covered window whose price arithmetic does not finish finite takes the existing coverage lifecycle — pending inside the grace, the typed price-coverage closure past it — rather than a new closure variant.
Closed off the round: `engine::EVIDENCE_FLOOR_VERSION` at `evidence-floor-v4` with its history line, a v3-stamped trail refused at the resume gate (pinned beside the v1 refusal), and the §Evidence floor stamp sentence re-cut.
Closed: `Holdings::normalized` also refuses a non-finite summed cash or derived account total — the live adapter's own sums, which reached the standalone snapshot and the checkpoint header unchecked — pinned.
Closed: the narrative classification reads the raw quotient, an infinite ratio being hype (the cap fires) with the persisted ratio absent, where the filtered `None` fell to the justified branch and suppressed the Medium ceiling — pinned over two finite legs whose quotient overflows.
Closed: the label pass's non-finite `continue` — which pended forever, off `pending_coverage` — now takes the coverage lifecycle, and `bench_return` reads a non-finite benchmark return unavailable with its gap naming why; both pinned (the 1-month window closing typed past the grace beside the 12-month window pending inside it; the market leg absent with its gap).
Closed: `storage.md`, the logic-flow run-listing line, and the `insert_run` comment now say the holding is named where the value sits in a per-holding record, matching §Failure posture.
Codex round 2 (2026-08-29) found one item: the interpretation prompt rendered every hype read with no persisted ratio as "reality flat or declining", which since round 1 also covers a positive reality leg the expansion outran beyond any finite multiple, and its percentage render multiplied a finite leg by 100 unguarded.
Ruled 2026-08-29 (Codex round 2): the render change is a `PROMPT_VERSION` event on I12's precedent — edge-only, stamped all the same — moving the stamp to `portfolio-v21`; group 3's prompt renders will bump again.
Closed: `narrative_prompt_section` names the overflowed ratio as its own state, renders a decimal leg whose ×100 overflows as the ratio itself, and is pinned at the prompt level over the overflowed, the flat, the finite, and the extreme-leg reads; `PROMPT_VERSION` at `portfolio-v21` with its history paragraph, the watch set's stamp line moved with it.
Twenty-one tests pinned in all across the slice, the reviewer round, and the two Codex rounds.

### I17 — minor: the run-level checkpoint counters over-count on resume

The resume-prompt-usage slice moved the data-health telemetry onto the holding's checkpoint row, so its trail membership is row membership and a holding whose fail-soft write failed or whose row no longer reads takes its calls with it when it re-analyzes.
The run-level accumulators keep the older cumulative shape (`store.rs`, `CheckpointAccumulators`; `job.rs`, the checkpoint block), re-written whole beside every successful holding write, and two of them over-count on resume.
`deep_history_failures` counts a holding whose own write failed once through the next holding's write and again when the resumed run re-analyzes it; the unreadable-row route reaches the same state.
`benchmark_gaps` is pushed inside the per-process `benchmark_closes` memo, so on any resume a sector benchmark that fails in both processes lands on the seeded list a second time — no write failure needed.
The keyed maps (`sector_by_symbol`, `industry_by_symbol`, `profile_name_by_symbol`) are immune, a re-analysis overwriting its entry.
The fix follows the telemetry's pattern: carry each holding's contribution on its row and rebuild the counts from the restored rows at resume, the gap list deduplicated by benchmark.
Surfaced by the resume-prompt-usage slice's first Codex round (2026-08-28); queued ahead of the run, one finding per slice, like I1–I16.
Ruled 2026-08-29: the fix follows the telemetry's pattern as written — each holding's deep-history flag and the benchmark it read as unavailable ride its checkpoint row (`store::HoldingHealth`), the counts rebuilt from the restored and written rows at the roll-up through one pure `health_counts`, the benchmark list deduplicated by symbol; the accumulators keep only the keyed identities.
Ruled 2026-08-29: a holding's benchmark read lands on its row off a fresh fetch or the per-process memo alike, so the rebuilt list is right whichever rows landed.
Ruled 2026-08-29: carried (unselected) holdings contribute no health row, as they contributed nothing to the retired counters; no compat for the pre-I17 row or accumulator shape, the format stamp of §I18 refusing such a trail; no frontend change.
Ruled 2026-08-29: none of the five version axes moves; the trail's shape moves the checkpoint format stamp to `checkpoint-v2` (§I18).
Resolved 2026-08-29: `CheckpointAccumulators` holds the three keyed maps alone; `CheckpointHolding.health` carries `deep_history_failed` and `benchmark_gap` with no serde default; the loop records the deep-history flag at the fetch and the benchmark read at the memo (which memoizes the degraded flag beside the closes), pushes the row before the fail-soft write, and the roll-up reads `health_counts` over every row; `DataHealth.benchmark_gaps` documents itself as the distinct count.
Four tests pinned: the store round-trip carries the health row and the format stamp; `health_counts` counts two degraded holdings and one benchmark across four rows; a three-stock run whose AAPL row is corrupted after its write resumes to a deep-history count of three, never four; and a benchmark failing in both halves of a resumed run counts once, its row naming the benchmark.
Canonical at `docs/portfolio-analysis.md` §Failure posture, mirrored in the logic-flow's Step 6 preamble, Resume behavior, and Step 6g output.
Reviewer round 1 (2026-08-29) approved with three nits, all folded in: the new `health_counts` test had taken the feed-gaps test's doc comment (moved back), this resolution named `RunDataHealth` for `DataHealth`, and the §Failure posture health-row sentence carried two claims (split); no stamp moves off the round.

### I18 — minor (ruling): the checkpoint trail resumes across any code change that moves no stamp

`resume_eligibility` keys on the persisted version stamps, the model roster, and the prior-run identity (`job.rs`), so a rebuild that changes completed-holding semantics without moving a stamp resumes a pre-change trail into the new binary, mixing verdicts across the change.
I1 stamped its own change (`engine::EVIDENCE_FLOOR_VERSION`).
The general case stands, and every non-stamp slice since the trail landed has shipped under it.
The exposure is bounded: the trail is transient, discarded by any new run, and offerable only inside the 48-hour resume window.
The mix is the pre-change behaviour for the restored holdings, never a corrupt record.
Two answers exist: a build-identity stamp on the trail, refusing a resume across any rebuild (the intent the `resume_eligibility` doc comment already states — the pinned contract cannot be re-created from an updated app), or an explicit ruling that the stamp axes are the contract and a slice that changes completed-holding semantics moves one.
Surfaced by I1's first Codex round (2026-08-28); queued ahead of the run as a ruling item, one finding per slice, like I1–I17.
Ruled 2026-08-29: the stamp axes are the contract — the five version axes stamp what a completed holding's verdict and audit mean, the roster the models, the prior-run identity the baseline — and a slice that changes completed-holding semantics is obliged to move the axis it changed; a rebuild moving none resumes, the restored holdings carrying the pre-change behaviour, the bounded residual as recorded.
Ruled 2026-08-29: a build-identity stamp is declined — `CARGO_PKG_VERSION` is frozen by the no-release rule, a `build.rs` hash or timestamp does not refresh on a source edit under tauri-build's rerun set and a git hash is blind to uncommitted edits, so the only honest identity is an executable mtime probe, and it would refuse the fix-then-resume recovery an hours-long run exists to keep, a CSS-only rebuild included.
Ruled 2026-08-29: the trail's own shape is a sixth stamp — `store::CHECKPOINT_FORMAT_VERSION` on the header, `checkpoint-v2` with I17, a header lacking the field decoding as `checkpoint-v1` on the evidence-floor precedent — so a shape change refuses at the gate with its reason rather than loud-skipping every row and offering a resume that restores nothing against a pinned pull.
Resolved 2026-08-29: `resume_eligibility` checks the format stamp after the floor stamp, and its doc comment states the contract in place of "cannot be re-created from an updated app"; the contract is canonical at `docs/portfolio-analysis.md` §Failure posture and mirrored in the logic-flow's Resume behavior.
Two cases pinned beside the floor refusals: a `checkpoint-v1` header and a header lacking the field are both refused with the format reason.
Codex round 1 (2026-08-29) found two items: the loader still decoded and loud-skipped every row of a trail under another format before the gate ran, so the recorded "refuses rather than loud-skipping every row" overclaimed and the pre-stamp test bypassed the real loader; and BUILD's version-constant list omits `store::CHECKPOINT_FORMAT_VERSION`.
Closed: `load_checkpoint` returns a header under another format alone — its accumulators and rows unread — so the gate's reason is the only surface a stale trail reaches, pinned twice: the store loader leaves a decodable row unrestored under a `checkpoint-v1` header, and the job gate refuses the stripped header written back to the trail through the real loader.
Deferred to session-end: the BUILD §Seams version-constant list gains `store::CHECKPOINT_FORMAT_VERSION` (a `.metis/` write, beside the CURRENT.md stamp list and the INDEX row).
No stamp moves off the round.
Codex round 2 (2026-08-29) approved the correction round with the session-end follow-up standing.

### I19 — minor (ruling): the one-fact contract's single-number ambiguity

Step 6e's metric-context binding reads the row's metric-family language from a drafted stem table and accepts a quote that carries a stem and exactly one number matching the row's value (`pre_profit.rs` — `excerpt_binds_metric`, `metric_stems`).
It is a syntactic admission filter: it rejects a second number beside the value, but cannot tell what the one number it admits means, so two shapes pass that a reader would reject.
A stem with no number of its own beside a competing noun the lexicon does not know — "deliveries and revenue of 41 million" — backs a deliveries row with the revenue number.
A value that is itself the period or a unit — "delivery guidance for 2025" — backs a deliveries row of 2025, and "delivery guidance for 2025-2026" fits the guidance-range carve-out; units and period are unbound by the slice's ruling.
Both shapes are bounded to a quoted, persisted excerpt the audit can read, and the run's rejection split is the evidence for how often either arrives.
Answers exist per shape: a negative lexicon of competing financial nouns (revenue, margin, profit, cash, …) whose presence in the quote rejects it; a period-word guard that rejects a value immediately preceded by "for", "in", "of", "by", "through", or "fiscal" when it reads as a 1900–2099 year; or leaving both to the persisted-excerpt audit and calibrating the stem table off the run.
Surfaced by I3's second Codex round, re-cut by its third (2026-08-28), which closed the bare-year-beside-the-value half by making every digit run count, and broadened by its fourth to the candidate-is-the-period shape; queued ahead of the run as a ruling item, one finding per slice, like I1–I18.
Ruled 2026-08-29: the period-word guard is adopted, its word list `for`, `in`, `of`, `by`, `through`, `fiscal` and `fy` (drafted, calibratable); the range carve-out reads the word before the left endpoint and rejects when both endpoints read as years; a digit run printed with a thousands separator is a count, never a year, and is exempt; a genuine count in the 1900–2099 band after such a word is the accepted loss of an optional row.
Ruled 2026-08-29: the negative lexicon is declined — the semantic-lexicon shape I3's first two rounds leaked on, and one that collides with the UnitEconomics stems (`revenue per`, `margin`) so it would be a second per-kind table drafted blind; the competing-noun shape stays with the persisted-excerpt audit, the run's rejection split calibrating the stem table.
Ruled 2026-08-29: the guard rides `PROMPT_VERSION` on I3's `portfolio-v17` precedent, with the 6d prompt line stating the rule; `PRE_PROFIT_PARAMETER_VERSION` and the other axes do not move.
Resolved 2026-08-29: `excerpt_binds_metric` returns `MetricContext::PeriodValue` — its reason naming the shape — through `reads_as_year` and `period_word_before`, the comma marks carried on `ExcerptRead::Stated` from the stripping; the 6d bullet tells the model a four-digit year after a period word is the period and a row valued at it rejects; canonical at `docs/portfolio-workflow.md` §Step 6e, mirrored in the logic-flow's 6e activation-legs bullet.
Two tests pinned: the guard over the seven words, `FY2025`, the period range in both roles, the accepted loss, and the admitted shapes (another word, a non-year, a decimal, a comma-bearing run, a range whose left endpoint is no year); and the 6d prompt stating the rule on the overlay-eligible branch alone.
`PROMPT_VERSION` moved to `portfolio-v22` with the group.
Reviewer round 1 (2026-08-29) approved the group with five nits, four folded in: the comma hugging the run from outside (`in 2025, deliveries` — the mark sits at the span's end, never inside it) pinned in the reject list; the phrasings the immediate-left word does not reach recorded below as calibration candidates; the fund-prompt test renamed for the I10 assertion it also carries (§I8); `us_share`'s doc link made plain (§I8); the logic-flow 6e mirror's appended clause left standing, the file outside the `docs/` sentence rule; no stamp moves off the round.
Calibration candidates off the round, for the post-run stem-table pass: `guidance for the year 2025`, `in early 2025`, `by year-end 2025` and `for CY2025` still admit a row valued 2025, the immediate-left word being `year`, `early`, `end` or `cy` — the word list is drafted and moves on the run's rejection split, never blind.
Codex round 1 (2026-08-29) returned reject-with-reasons on three items.
P1 — the guard does not re-validate carried observation rows — is pushed back as a group defect: the carry is the documented history contract (`docs/portfolio-workflow.md` §Step 6b and §Step 6e), §I3 ruled 2026-08-28 that no store holds a research-produced row, and nothing has run live since, so no pre-`v22` row exists and the fresh-start rule bars a migration for data that never existed.
The forward question P1 does raise — whether a carried row re-admits through the current excerpt-only legs (metric language, one number, period word) when the admission contract moves, the post-run word-list calibration being the first case — is surfaced for ruling; the page legs stood at admission and cannot re-run.
Ruled 2026-08-29: attribute, never re-filter — each accepted row records the prompt stamp it was admitted under, and the question is queued as I20 on the same terms as I1–I19, its own slice.
P2 — the crossing pair's per-value precision was comparison-unsafe — is closed under §I12.
P3 — BUILD's standing-constraint bullet and §What remains still read this group as queued — is a `.metis/` write, deferred to session-end on §I18's precedent beside CURRENT.md's stamp list.
No stamp moves off the round.
Codex round 2 (2026-08-29) returned one remaining P2 — the ten-place floor of the crossing render — closed under §I12; the P1 push-back was confirmed by Codex's own read-only database check (zero production Portfolio runs, one development run with no `source_excerpt`), I20 accepted as the forward provenance requirement, and P3's session-end deferral accepted; no stamp moves off the round.
Codex round 3 (2026-08-29) returned one remaining P2 — the crossing render's stop test on a sign crossing that rounds to negative zero — closed under §I12; no stamp moves off the round.
Codex round 4 (2026-08-29) returned one remaining P2 — the stale "strings differ" wording in the `portfolio-v22` history paragraph and the formatter's doc comment — closed under §I12, the functional correction judged sound; no stamp moves off the round.
Codex round 5 (2026-08-29) returned one remaining P2 — the pair test's comment claiming distance fidelity the formatter does not guarantee — closed under §I12 as a comment correction, distance fidelity not adopted as a requirement, functional behaviour approved; no stamp moves off the round.
Codex round 6 (2026-08-29) returned one remaining P2 — the replacement comment's absolute "magnifies" where rounding may also preserve or shrink a gap — closed under §I12 as a wording correction, functional behaviour approved; no stamp moves off the round.
Codex round 7 (2026-08-29) returned one remaining P2 — the comment's shared-precision claim unqualified by the round-trip fallback — closed under §I12 as a wording correction, functional behaviour approved; no stamp moves off the round.

### I20 — minor (ruled): a carried observation row carries no admission stamp

An accepted `pre_profit_observation` row persists with its excerpt, source and vintage, and the overlay carries the pre-profit parameter stamp, but nothing on the row says which admission contract admitted it (`pre_profit.rs`, `PreProfitObservation`; the overlay's `parameter_version`).
The history carries whole across runs (`compute_overlay_with_sources`, the prior overlay's rows merged with this run's accepted rows), so every admission-contract move — the excerpt leg (`portfolio-v17`), the publication date (`portfolio-v18`), the period-word guard (`portfolio-v22`), and the post-run calibration of the stem table and word list to come — leaves rows admitted under the looser filter in the history with no way to tell them from rows the current filter would admit.
No store holds such a row today (§I3's ruling stands; nothing has run live since), so this is a forward contract, not a migration.
Surfaced by group 3's Codex round 1 (2026-08-29) as its P1, pushed back there as a group defect and re-cut here as the ruling it is; queued ahead of the run on the same terms as I1–I19, one finding per slice.
Ruled 2026-08-29 (at its addition): attribute, never re-filter — each accepted row records the prompt stamp it was admitted under, the history is never re-admitted through a later filter, and the audit and any later calibration slice read the stamp to tell old rows apart, on the standing stamp doctrine (a persisted record carries the stamp it was written under, so a recalibration stays attributable and old rows never silently re-grade); the re-admit-at-carry alternative is declined, since a later, stricter filter would erase an older row's miss history.
The field takes no serde default (no store holds a row — §I3's precedent), and the slice's plan asks which axis the row's new shape moves, on §I18's contract.
