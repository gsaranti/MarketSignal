# Scoped conformance check + off-spine doc pass (2026-08-07)

The follow-up the four prior sweeps left owed
(piece 2 = the code-vs-docs conformance walk, [2026-08-04-piece2-conformance-walk.md](2026-08-04-piece2-conformance-walk.md), re-run [2026-08-05-piece2-conformance-rerun.md](2026-08-05-piece2-conformance-rerun.md);
piece 3 = the deterministic value-chain correctness walk, [2026-08-05-piece3-value-chain-walk.md](2026-08-05-piece3-value-chain-walk.md);
plus the TO docs sweep (PR #65) and the Portfolio docs sweep (PR #66)).
Each of those corrected docs to match code, which **implicitly ruled the code right** — but every finding verified only the *one site it cited*, and nothing confirmed the behavior held anywhere else.
This walk closes both halves of that gap: the **code half** re-checks each changed behavior across *all* paths, and the **doc half** walks the owning-doc sections the spine sweeps consulted but never walked as a spine.

Two gaps motivated it, both concrete rather than theoretical.
`engine::quarters_contiguous` had seven call sites while the piece-3 record claimed three and its commit message five — either stale prose or an unguarded statement window.
And Codex's option/bond display-contract finding on PR #66 came from `schwab-integration.md` / `portfolio-analysis.md §Storage and display` — territory no charter had ever covered.

## Method

**Twelve parallel passes**, one wave, split by charter rather than by file —
seven code passes ((C1) cost-basis / display contract across every consumer, (C2) ET dating across every date comparison, (C3) statement windows + contiguity + basis crossings incl. the `dossier.rs` deep read, (C4) identity / ordering / state-transition invariants, (C5) PR #66's five ruled behaviors across all paths, (C6) `engine.rs` fresh correctness read, (C7) `outcome.rs` fresh correctness read)
and five doc passes ((D1) `portfolio-analysis.md` §Storage and display + its unwalked peers, (D2) `storage.md`, (D3) `schwab-integration.md` + `interface.md`, (D4) `configuration.md` + `local-models.md`, (D5) `data-sources.md` §Portfolio Analysis — endpoint surface).

The two halves carried **different bars**, deliberately.
The doc half was **flag-only-incorrect** — contradictions, unsupported claims, impossible flows; a code/doc divergence *is* the finding and the ruling picks the wrong side; wording, completeness and structure explicitly out of charter.
The code half required a **concrete inputs→wrong-output failure scenario** on every concern, with reachability traced.
Both halves required targeted code reads for every claim about built machinery — no finding from prose alone — and every doc pass carried the designed-not-built exclusion list up front (Trade Opportunities entire, the live research loop, the held-name refresh lane, checkpoint/resume, the deferred investor-profile form), which is the dominant false-positive source in this corpus.

Scope was set by user ruling before the fan-out: the code half runs **consistency-across-paths plus a deep re-review of the hot files**, and the doc half covers the five off-spine sections **plus** the two remaining owning docs (`data-sources.md`, `local-models.md`).

Both of the charter's own leads resolved **negative**, which is itself the result.
The contiguity discrepancy was prose imprecision at three granularities — three *functions*, five *windows* counting pre-profit's three as one, seven *call sites* since the anchor loop calls per `i` — and all eight fixed-width statement windows in production code are guarded.
And the option/bond display contract holds: three passes (C1, D1, D3) independently traced it and found no leak into the 7a spine, either 6f prompt, the 7b prompt, or the outcome features, because `AssetClass::is_gradeable()` routes those classes to `NotRated` before the engine stage and `NotRatedContribution` carries no cost-derived field.

Large parts of both halves **conformed**:
insertion-order run identity across every run-selecting query and the portability export; case-folding on every symbol join in `portfolio/`; the inclusive evidence boundary uniformly across the filings, earnings and news legs; `etDate.ts` behaviorally equivalent to `market_clock` (checked against chrono 0.4.44's strict parser, both divergences unreachable); the canonicalization choke point with no bypassing reader; the SEC prefix + duration guards on every concept read; every number in the retry/backoff paragraph; the anchor-close bridge algebra and the Winkler score; and the full per-symbol call cardinality.

## Dispositions

**26 findings across the twelve passes** — 16 from the seven code passes, 10 from the five doc passes — with **four cross-pass convergences**, the strongest signal available (piece 3 had three in 33).
**3 were ruled and fixed this session; the remaining 23 are enumerated below** for the next ruling round.

### The Tier 1 fix batch (this session)

Three findings, each corrupting exactly what the big confirmation run is meant to measure. All three ruled to the recommended disposition.

- **The sector-P/E snapshot was UTC-dated and single-shot** (C2) — `LiveCompanyData::sector_pe_snapshot` keyed FMP's date-keyed snapshot on `last_weekday(Utc::now())`, so a run after ~8 PM ET on Sun–Thu asked for a session that had not traded.
  The endpoint answers **200 with an empty array**, not an error, so `last_err` stayed `None`, no gap was recorded, and every priced US-equity fund then failed `composite_yield` and abstained — attributed to "no P/E-usable sector overlap" rather than to the missing snapshot.
  Five nights in seven, and from Pacific time the window opens at 5 PM.
  Fixed three ways: the date is now the **ET session date** via `market_clock::et_session_date`; the walk backs over weekday candidates by sharing the report chain's own `sector_candidate_dates` / `SECTOR_LOOKBACK_WEEKDAYS` (now `pub(crate)`); and an **exhausted walk with no transport fault now returns `Err`** — this seam's gap channel — so the abstention names the missing snapshot instead of blaming the fund's sector weights.
  A partial read (one exchange served, the other faulted) is still accepted, as before.
  Two corrections came from external review (round 1) and are folded in.
  The gap is now **memoized alongside the snapshot** and pushed onto *every* fund's `gaps`: the surface is fetched once, so recording its failure only where the fetch happened left funds 2..N abstaining with the original misleading reason — the fix's whole purpose, defeated for every fund but the first.
  And the walk's **holiday** justification was overstated here: `fmp.rs`'s adapter comment carries live-verified evidence (2026-07-03 and Juneteenth, verified 2026-07-16) that a weekday holiday *does* serve carried values, which the old `job.rs` comment's speculative "a market holiday can still gap" contradicted.
  The walk is retained as ruled, but its real warrant is the **empty-is-a-200** problem, not holidays — so the second candidate is a safety net rather than an expected cost, and the practical cardinality stays two calls.
  The stale adapter comment and the two cardinality claims that promised "one call per exchange" ([data-sources.md](../data-sources.md#portfolio-analysis--endpoint-surface), [portfolio-workflow.md](../portfolio-workflow.md) §Step 6) were corrected with it.
- **A negative debt/equity read as *low* leverage in the risk tier** (C6) — `compute_metrics` produces a signed ratio and `risk_score` guards it (the `RISK_DEBT_EQUITY_BAND` comment names this exact inverted-clamp hazard, scoped by name to `risk_score`), but `assign_stock_tier`'s legs compared with naked `>` / `<`.
  Negative equity — levered beyond the equity base, maximal leverage — therefore failed the High leg and *passed* the Low one.
  A buyback-driven negative-book large cap took `RiskTier::Low` with empty `tier_gaps`: hurdle 7% instead of 9%, `HurdleState::Clears` instead of `Indeterminate`, `admits_new_money` true, the add family in the feasible set, and `engine_action` returning **Add** where the correct tier returns Hold — while `risk_score`'s leverage leg scored the same input 0.0 in the same run.
  It also rendered into the 7b digest as literally `tier low` and froze into the outcome calibration snapshot.
  Both legs are now bounded (`!(0.0..=TIER_HIGH_MIN_DEBT_EQUITY).contains(&d)` and `(0.0..TIER_LOW_MAX_DEBT_EQUITY).contains(&d)`) — the Low bound stated even though the High leg now claims the case, so the conjunction reads correctly on its own and survives leg reordering.
- **Priced funds authored ledger conditions on a 1,600-day window and evaluated them on a 180-day one** (C3) — `fund::base_metrics` preferred the dated deep history (`daily_closes`) for `trailing_return` and `return_volatility`, while the quick check evaluates both off `engine::compute_metrics`'s 180-day `price_history`.
  Both are fund-computable ledger series, so a falsifier authored at a ~43% trailing return was swept against a ~3% one: two sweeps on distinct close dates confirm a breach on a fund whose thesis is intact, with the forced selective-run inclusion riding along — and symmetrically, a stop-loss-shaped falsifier becomes permanently un-breachable.
  `base_metrics` now takes both price legs from `engine::compute_metrics`, mirroring the role-risk branch, which `cdb7977`'s own Codex round had already pointed there for precisely this reason ("so the full pass covers the SAME fund-computable surface the quick check evaluates") without bringing the priced branch along.
  The deep history still backs the fund tier's volatility leg and the momentum sub-score — both authored once per run and never re-evaluated, so no second window can disagree with them.
  The existing fixture could not catch this: it gives four closes to both vectors.

### The cross-pass convergences

Four findings were reached by two or three passes independently, and each should be ruled as one item rather than several.

- **The ET conversion is incomplete** (C2 + C3 + C4) — five sites still read the UTC calendar date where the slice's contract is the ET session: the sector-P/E snapshot (fixed above), the house-view freshness gate (`job.rs`, passing `Utc::now().date_naive()` against a UTC date prefix), the quick-check rate-cache age, the per-holding `run_date` re-derived inside the loop rather than read from the run's `created_at`, and the TTM dividend window (`fmp.rs`, a file `512d5ec` never touched).
  The count is the finding: the slice converted the sites its own findings cited and stopped.
- **A signed metric reaches an unguarded ledger comparator** (C3 + C6) — `resolve_series` handles `PeRatio` and `DebtToEquity` with no sign guard, while both sub-score consumers have one.
  A negative P/E breaches "P/E below 15" and fires an *add* trigger on a company that has just gone loss-making; a negative D/E not only cannot breach "debt/equity above 3" but reads as a **clean** observation and resets a standing breach streak — the silent clear the ledger machinery exists to prevent.
  Piece 3's design note dismissing the P/E case rests on a false premise: it argued a wire-served negative P/E always could occur, but `dossier.rs` is the only producer of `pe_ratio` in the codebase.
- **The basis bridge can read a stale pre-split bar** (C2 + C7) — the per-pass price-bar fetch is floored at the earliest **active-episode anchor** while the bridge keys at the **intrinsic vintage**, which on a rule-demotion open is older than any anchor.
  `load_price_bars` has no date bound, `merge_price_bars` rewrites only fetched dates, and `price_bars` is never pruned, so a cached bar outside the refreshed range satisfies the bridge on a stale adjustment basis instead of excluding — producing a fabricated material-drawdown breach.
  The floor's own doc comment states the rationale it fails to carry through.
  The absent-bar sibling case is correctly excluded and tested; this is the present-but-stale-basis hole in the same guard.
- **The falsifier lead-time read is doubly wrong** (C4 + C7) — `confirmed_at` is stamped from the *consuming* run rather than the confirming print, so the lead time is understated by the whole sweep-to-run gap and can sign-flip; and the event dedup keys on `confirmation_observation_id`, which is *designed to change on every re-raise*, so one standing breach accrues a fresh event on every run (~40 over a 12-month episode on a market-cadence falsifier).
  These compound: N events, each mis-stamped.

### Deferred to the next ruling round

**23 findings carry to the next ruling round** — 13 from the code passes, 10 from the doc passes.
All 23 were **ruled 2026-08-07** in the round recorded below (§Rulings); this enumeration stays the evidence home, that section carries the dispositions.
The two convergence pairs above (signed metrics; the stale-basis bridge) each count once and should be ruled once.
Note the split is by *pass*, not by nature: two doc-pass findings (D3-1, D5-1) are code defects, and one (D5-3) may rule either way.

From the code passes:

- **1 (C3+C6, convergence).** `resolve_series` reads `PeRatio` (`engine.rs:805`) and `DebtToEquity` (`engine.rs:787`) with no sign guard, while `valuation_score` (`:1189`) and `risk_score` (`:1219`) both have one.
  A negative D/E additionally reads as a *clean* observation and resets a standing breach streak (`engine.rs:944-951`).
- **2 (C2+C7, convergence).** The price-bar fetch floors at the earliest active-episode anchor (`outcome.rs:981-999`) while the bridge keys at the intrinsic vintage (`outcome.rs:1069`); `load_price_bars` has no date bound (`store.rs:203-208`) and `price_bars` is never pruned, so a cached pre-split bar satisfies the bridge on a stale basis.
  `falsifier_lead_times` (`outcome.rs:2301`) has no `vintage_fresh` gate, so it admits exactly this population.
- **3 (C4).** `FalsifierEvent.confirmed_at` is stamped from the consuming run (`outcome.rs:1882-1890`), not the confirming print; `stamp_lead_times` (`outcome.rs:1319`) positions on it alone, so lead time is understated by the sweep-to-run gap and can sign-flip.
- **4 (C7).** The falsifier-event dedup keys on `confirmation_observation_id` (`outcome.rs:1921`), which is designed to change on every re-raise, so one standing breach accrues ~40 events over a 12-month market-cadence episode.
- **5 (C2).** The house-view freshness gate passes `Utc::now().date_naive()` (`job.rs:538`) against a UTC date prefix (`dossier.rs:389-393`) — an evening-ET run ages a 7-day-old report to 8 and drops the whole house view.
- **6 (C2).** The quick-check rate-cache age takes a UTC prefix (`quick_check.rs:437`, `:704`, compared in `rate_cache_fresh` `:746-757`), reading a 7-ET-day-old print as 8 days old on an evening sweep.
- **7 (C2).** `run_date` is re-derived per holding inside the loop (`job.rs:906-913`) rather than read from the run's `created_at`, and is the UTC prefix — a midnight-crossing run stamps holdings on two different ET days.
  Currently inert (no consumer compares it), so this is a contract divergence, not a wrong output.
- **8 (C3).** The TTM dividend window's `today` is `Utc::now().date_naive()` (`fmp.rs:4088`, a file `512d5ec` never touched), so an evening run can admit a fifth payment (`fmp.rs:4667-4688`) — re-opening the inflation the 366-day fix closed.
- **9 (C7).** The total-return dividend window ends at the calendar `w_end` (`outcome.rs:1177`, `:1185-1193`) while the end price is the last bar at or before it (`:1140`) — a dividend going ex after the last close double-counts, always signed positive.
  The entry side already has the symmetric guard.
- **10 (C7).** The abstained-prior arm of `episode_decision` (`outcome.rs:1527-1558`) compares only the ledger branch and weight range, never the action or lean — so the first fresh pass after an abstention extends the superseded episode instead of opening one.
- **11 (C4).** Episode supersession selects by `max_by(anchor_at)` (`outcome.rs:1726`) with the Open arm only pushing, so a backwards clock step permanently shadows a newly opened episode.
  Same premise as the piece-3 `prune_runs` / `latest_run` fixes.
- **12 (C4).** `baseline_snapshots` retains the pre-fix insert-then-prune shape keyed on `captured_at DESC, id DESC` (`storage.rs:861-883`, `pipeline.rs:464-481`).
  Report chain, not Portfolio — carried because the piece-3 batch admitted one report-chain bug on the same precedent.
- **13 (C3).** A one-quarter feed gap now drops to the SEC annual basis (`dossier.rs:168-174`, `:229-236`), flipping P/S ~8.0 → 10.3, which the sweep then locks in (`quick_check.rs:1268-1274`) and can confirm on a market-data cadence.
  Verified end to end but an amplified consequence of the annual-fallback design rather than a new defect — the accept-with-note candidate.

From the doc passes:

- **14 (D3-1, code defect).** Two producers emit `WarningKind::ProviderCredentials` — `config.rs:446-453` and `local_model.rs:734-741` — and `App.vue:470-482` concatenates them under a comment claiming de-duplication, so an unset FMP key renders two "Provider credentials" rows.
  `PersistentWarningArea.vue:108` keys on `cat.kind`, so it is also a Vue duplicate-key collision, against `interface.md:92`'s one-per-category contract.
- **15 (D5-1, code defect).** The Stooq throttle breaker and politeness pacer are instance state (`stooq.rs:56-66`), and one Portfolio run constructs two adapters (`lib.rs:737`, `lib.rs:790`), so `data-sources.md:325`'s "run-wide breaker" does not hold across the loop→outcome boundary.
- **16 (D5-3, rules either way).** `data-sources.md:664` qualifies `/chains` cardinality as "per-holding (optionable equity)" while `job.rs:850-862` gates only on `guard_terminal` — so options, bonds and cash-equivalents also spend the full per-symbol FMP surface, the SEC facts call and the Stooq deep-history leg before the not-rated gate.
  Either the doc understates the budget or the loop should gate on `is_gradeable()`.
- **17 (D2).** `storage.md:160-161` places the quick check's attention-flag / evidence-event state in the per-run Portfolio record; as-built it is the whole content of the single-row `portfolio_quick_checks` store (`store.rs:46-58`), which the section's store enumeration omits — leaving `:202`'s "each durable store above" false.
- **18 (D4).** `local-models.md:71` asserts a shared embedding-response validator with six named checks; the local path (`embedding.rs:183-204`, `:259-266`) implements none at the call, and only finiteness exists at all, downstream at `vector_memory.rs:130`.
  The same contract is stated canonically at `report-workflow.md:187` and is likewise unimplemented, so a ruling lands on both homes.
  One sub-claim (configured dimensionality) conflicts with the deliberate dimension-agnostic store design.
- **19 (D1).** `portfolio-analysis.md:502` says a `role_risk_only` episode records the parameter version; `RoleRiskEpisode` (`outcome.rs:344`) has no such field and neither does the wrapper, and the field's only consumers exclude role-risk by design.
  This list is the canonical home — `storage.md:164` defers to it.
- **20 (D1).** `portfolio-analysis.md:569` describes the pull table's price as "the per-unit price as the source reported it (absent where it carries none)"; Schwab reports no price field (`schwab-integration.md:34`), the app always derives `market_value / quantity` (`schwab_live.rs:235`), and the real null trigger is zero net quantity.
- **21 (D5).** `data-sources.md:670` names the input delta's technology-event pre-flag as a live consumer of the Stooq benchmark row without the designed tag its neighbours carry (`:642`, `:671`, `:672`); no input delta exists in code, and `portfolio-workflow.md:123` already places that leg in the designed list.
- **22 (D5).** `data-sources.md:606` attributes the pre-profit overlay's operating-income eligibility, burn/runway, capex intensity, margin progression and share change to the quarterly **balance-sheet** call, which returns four lines (`fmp.rs:3934-3953`); only liquid resources comes from it (`pre_profit.rs:527-531`).
- **23 (D3).** `interface.md:52` places a **Report generation** panel in the Settings tree; no such section exists (`Settings.vue` has nine), the trigger lives in the footer (`App.vue:1726`), and `interface.md:79` contradicts it eleven lines later.
  `configuration.md:9` repeats the claim.
  Retired with the move to on-demand generation, not designed-not-built.

One further item was reported below the finding bar and is recorded rather than carried: `schwab-integration.md:3` brands option chains as Trader API while `:45` correctly separates the Market Data product (`schwab_live.rs:137` vs `:92`/`:112`) — a wording ruling at most, and `data-sources.md:273` uses the same loose section brand while being precise in its body.

### Probes and watches this adds for the big run

- **Sector-P/E walk-back depth** — how often the first candidate misses, and whether `SECTOR_LOOKBACK_WEEKDAYS = 5` is enough at 47-position scale.
- **Risk-tier distribution** — negative-book issuers now taking High rather than Low, and the resulting hurdle / action-family shift against the stacked "conviction/action pairing" and "fails → indeterminate action distribution" watches.
- **Fund ledger flag rates** — whether priced-fund `TrailingReturn` / `ReturnVolatility` conditions still flag once authoring and evaluation share the 180-day window.
- **Basis-flip rate** (carried from C3's fourth finding — ruled a defect, not accept-with-note, in §Rulings below) — how often a one-quarter feed gap drops a holding to the SEC annual basis, and whether the resulting multiple move crosses a ledger condition.
  The watch survives the fix: the basis-continuity gate stops the fabricated crossing, so what the run now measures is how often the gate fires and how much of the valuation surface it types unevaluable.

## Rulings (ruling round, 2026-08-07)

**All 23 carried findings are ruled.**
Each was re-verified against current code before ruling — every one holds, and no claim decayed (a few line numbers moved when the Tier-1 batch landed).
Both convergence pairs were ruled once, as the enumeration asked.
The evidence stays single-homed above; this section carries dispositions only.

Three rulings went to the user and all three took the recommended disposition: the basis flip is a **defect, not accept-with-note** (finding 13), the embedding validator is **built, not documented down** (finding 18), and both systemic classes are **swept whole rather than fixed at the enumerated sites** (the scope ruling below).

Three scope corrections came out of the re-verification, and they are the reason for that last ruling.

- **The ET class is wider than the five sites the convergence named.**
  Beyond findings 5–8: `job.rs:877` passes `Utc::now().date_naive()` as `FundContext.as_of`, which drives `fund::quarter_end_before` — so an evening run on a quarter boundary samples the wrong quarter's sector-P/E history; `job.rs:1200` derives a UTC-prefix `run_date` and hands it to `mature_labels` beside an **ET** `today`; and in `fmp.rs` the three *other* date-keyed snapshot walks — `fetch_sectors:1034`, `fetch_sector_pe:1348`, `fetch_industry_snapshot:1427` — are still UTC-dated, which is **the same defect class as the Tier-1 sector-P/E fix in the batch above**.
  `fmp.rs:4005` / `:4040` (the estimates `today`) and `:1271` (the earnings window) carry the same premise at lower stakes; the `to`-bound sites (`:1097`, `:1670`, `:4173`) are benign upper bounds and are deliberately left.
- **The wall-clock class is wider than the one site finding 11 cited.**
  Four production selections use `max_by(anchor_at)` (`outcome.rs:1732`, `:1761`, `:1901`, `:1913`), and **both** store queries are wall-clock-primary: `load_episodes:126` orders `anchor_at ASC, id ASC`, so the loaded vec's own order is wall-clock-keyed and a last-in-vec fix alone would not be insertion-order, while `prune_matured_episodes:192` can preferentially delete the newest matured row.
- **Finding 7 is not inert.**
  It was carried as a contract divergence with no consumer, but `job.rs:950`'s `run_date` is what `engine.rs:938` stamps into a condition's `confirmed_at` — which is exactly what finding 3's fix consumes.
  The two must land together or finding 3 reads a UTC-dated confirmation.

### Batch A — run-gating code (12)

Each writes wrong data into the ledger or the outcome machinery the big run is meant to bank, so all twelve land before it.

- **1 (convergence).** Sign-guard `PeRatio` and `DebtToEquity` in `resolve_series`, matching `valuation_score` / `risk_score`.
  The D/E half is the worse leg: a negative ratio cannot breach "above 3", so it falls to the clean arm at `engine.rs:944-951` and wipes `breach_streak`, `first_breach_at`, `confirmed_at` **and** `acknowledged_observation_id` — the silent clear the ledger machinery exists to prevent.
- **2 (convergence).** Floor the per-pass price-bar fetch at `min(earliest active anchor, intrinsic vintage)` so the bridge's own key is always inside the refreshed range.
  `merge_price_bars` rewrites only fetched dates and nothing prunes `price_bars`, so today an unrefreshed cached bar satisfies the bridge on a stale adjustment basis instead of excluding; the floor's doc comment already states the rationale it fails to carry through.
- **3 + 4 (ruled as one).** Stamp `FalsifierEvent.confirmed_at` from the **confirming pass**, not the consuming run — `ConditionCrossing` carries no confirmation date, so thread the eval state's own `confirmed_at` onto it — and re-key the event dedup on the condition and its standing crossing rather than `confirmation_observation_id`, which is designed to change on every re-raise.
  Ruled together because they compound: without both, the fix produces one correctly-stamped event beside ~40 mis-stamped duplicates.
- **5.** Convert **both** legs of the house-view freshness gate — `et_session_date` for `today` (`job.rs:564`) and `et_date_of` for the report's `created_at` (`dossier.rs:389-393`).
  Both are UTC today, and the error is not that they disagree with each other but that the two events straddle the 8 PM ET rollover: a report written at ET 15:00 reads eight days old to a 21:00 ET run seven ET days later.
  Converting `today` alone would then *under*-age every report written after 8 PM ET.
- **6.** ET-date the quick check's `today` (`quick_check.rs:437`, `:704`), which `rate_cache_fresh` compares against a FRED observation date — a market date, never a UTC calendar day.
- **7.** Read `run_date` from the run's `created_at`, ET-dated, instead of re-deriving it per holding inside the loop.
  Lands with finding 3 per the scope correction above.
- **8.** ET-date the TTM dividend window's `today` (`fmp.rs:4088`).
  Both bounds move on an evening run: it admits a next-day declaration *and* slides the 365-day cutoff off the oldest in-window payment.
- **9.** Bound the total-return dividend window's end at the **end bar's date**, not the calendar `w_end`, symmetric with the entry side's existing `> entry_iso` guard.
  A dividend going ex after the last close is not in `end_bar.value`, so counting it is always-positive inflation.
- **10.** Compare the abstained arm against the **active episode's** recorded action and lean.
  The code comment is right that an abstained verdict carries no action (`rec_state` maps `InsufficientEvidence` → `Abstained`) — but the episode being extended does carry one, and it is the thing calibration scores.
- **11.** Make every episode selection insertion-order: `load_episodes` → `ORDER BY id ASC`, `prune_matured_episodes` → `ORDER BY id DESC`, and all four `max_by(anchor_at)` selections → insertion-order-latest.
  The exact `latest_run` / `prune_runs` precedent from the piece-3 batch.
- **13 (user ruling — fix, not accept-with-note).** Carry a typed statement-basis marker on the audit metrics and `QuickCheckBasis`, and type the valuation series **unevaluable** for a pass whose basis differs from the authoring basis, through the existing `unevaluable_series` channel.
  Ruled a defect on the Tier-1 fund fix's own precedent: that fix corrected a condition authored on one *window* and evaluated on another, and this is the same shape one level down — authored on one *basis*, evaluated on another.
  Observation identity alone does not close it: a distinct observation on the flipped basis still starts a streak, and a market-cadence series confirms on the second sweep.
  The annual fallback itself is retained — the flip is honest, the crossing it manufactures is not.
- **The scope ruling applies to Batch A.** The ET and wall-clock classes are converted **whole**, with the inventory above enumerated in the fix batch so each class is provably closed.
  The walk's own thesis is the warrant: round 2 caught this walk fixing the sites its finding named and stopping, one round after writing that failure mode down as the lesson.

### Batch B — code, not run-gating (5)

- **12.** Re-key `baseline_snapshots` selection and pruning on `id`, matching the same precedent as finding 11.
  Fold in a clamp on `BaselineDeltas::elapsed_days`, which shares the premise and is today an unclamped pass-through that would hand the prompt a negative interval.
  Report chain, not Portfolio — carried on the precedent that the piece-3 batch admitted one report-chain bug.
- **14 (doc pass, code defect).** De-duplicate warning categories by `kind` at the merge in `App.vue` — unioning items — so the comment claiming de-duplication becomes true, and make `PersistentWarningArea.vue`'s `:key` unique independently of it.
  Confirmed both producers (`config.rs:446`, `local_model.rs:734`) and the bare concatenation between them.
- **15 (doc pass, code defect).** Share one `StooqSource` across the per-holding loop and the outcome pass (`lib.rs:737`, `:790`) so the throttle breaker and politeness pacer are genuinely run-wide.
  The struct's own comment — "constructed per run, so the breaker resets naturally" — is false at two instances, and it matters more now that Stooq serves a PoW interstitial.
- **16 (rules one way, not either).** Gate the per-holding retrieval loop on `AssetClass::is_gradeable()`.
  The finding was carried as ruling either way; it does not.
  `pipeline.rs:244` routes every non-gradeable class to `NotRated` with `Default::default()` metrics, reading **none** of the financials, SEC facts, deep history or chain the loop spent on it — so the gate is provably output-neutral and `data-sources.md`'s "per-holding (optionable equity)" qualifier stands as written.
- **18 (user ruling — build it).** Implement the shared embedding validator both homes already specify: exactly one vector per input, every element finite, a nonzero norm, the responding embedder identity, and the input byte cap — at the call, behind the trait, for `OpenAiEmbedder` and `LocalEmbedder` alike, fail-soft at retrieval and drop-and-log at persistence exactly as documented.
  As-built only finiteness exists, downstream at `vector_memory.rs:130`, so it guards persistence and leaves a non-finite or zero-norm vector reaching cosine search unchecked at retrieval.
  One sub-claim is corrected rather than built: **configured dimensionality** conflicts with the deliberate dimension-agnostic store, whose guard is the search-time dimension skip — that clause moves to a pointer at the skip guard in both `local-models.md` and `report-workflow.md`.

### Batch C — doc-only (6)

None gates anything; these ride with the long-doc-line cleanup.

- **17.** Add `portfolio_quick_checks` to `storage.md`'s Local-Analysis-Suite store enumeration — a single latest row, portability format v2 — which makes `:202`'s "each durable store above" true again, and distinguish that live between-run home from the copy overlaid onto the per-run record at ledger carry.
- **19.** Drop "and the parameter version" from `portfolio-analysis.md:502`'s `role_risk_only` sentence.
  `RoleRiskEpisode` carries no `CalibrationSnapshot` and `DecisionEpisode` no such field, and every parameter-version-keyed read excludes the branch by design — so the claim describes a field with no possible consumer.
- **20.** Correct `portfolio-analysis.md:569`: the per-unit price is **derived** as market value ÷ signed net quantity (`schwab_live.rs:235`), not reported by the source, and the absent case is **zero net quantity**, not a source that carries no price.
  The option-contract parenthetical is already consistent with the derivation and stays.
- **21.** Tag the technology-event pre-flag consumer on `data-sources.md`'s Stooq benchmark row *designed*, matching the convention its neighbouring rows carry; the outcome-learning leg stays untagged as built.
- **22.** Re-attribute the pre-profit inputs on `data-sources.md:606` to the statements that actually supply them — the quarterly balance sheet gives total debt, stockholders' equity and liquid resources; operating-income eligibility, gross-margin progression and the diluted-share change come from the income statement, and TTM burn / runway and capex intensity from the cash-flow statement.
- **23.** Remove the retired **Report generation** panel from `interface.md`'s Settings tree and the matching bullet in `configuration.md`.
  No such section exists, `interface.md` contradicts it eleven lines later, and the on-demand footer trigger is already the documented single home.

## The ET-class sweep (built)

The scope ruling's first batch: the session-dating class converted **whole**, with the inventory below so the class is provably closed rather than closed at the sites a finding happened to cite.
Findings **5, 6, 7 and 8** are built here; the class's other members were converted with them.
The cross-cutting rule now has a citable home in [data-sources.md](../data-sources.md)'s intro — one sentence a future conformance pass can check every site against, which the class did not have before.

**Two further sites surfaced during implementation, neither in the ruling's inventory, and one of them is the highest-stakes member of the class:**

- **`fred.rs`'s rate-anchor freshness floor** compared a FRED observation date against `Utc::now().date_naive() − RATE_ANCHOR_MAX_AGE_DAYS`.
  A print sitting exactly on the ten-day bound reads stale on an evening-ET run, and that read does not degrade — `latest_rate_dated` bails, and the Portfolio job's rate-anchor rule **hard-fails the whole run** on it before any per-holding work.
  Every other member of this class produces a wrong number or a silent abstention; this one produces no run at all.
- **The report chain's FRED baseline scan** anchored its single clock sample on the UTC date, and that one `today` feeds both the per-series staleness guards and the release-calendar window — so an evening-ET run aged every FRED series by a day and dropped any print sitting on its own cadence bound to a gap.

The converted sites, by chain:

- **Portfolio suite** — the house-view freshness gate (both legs: the run's `today` in `job.rs` and the report's `created_at` in `dossier.rs`); the quick check's `today` at both entry points, behind one `sweep_session_date` helper; the per-holding `run_date`, now taken from the run's single instant rather than re-derived per holding; `FundContext.as_of`, which drives `fund::quarter_end_before`; and `fmp.rs`'s TTM dividend window and forward-estimates reference date.
  One residual inside the class surfaced on a closing sweep for date-prefix reads and was converted with the rest: `rate_cache_fresh`'s **fallback** arm, used only where FRED served no observation date, aged the cache against the UTC prefix of its own fetch instant — reading an evening fetch a day *younger* than it is, the opposite direction from the rest of the class.
- **Report chain** — `fmp.rs`'s three date-keyed snapshot walks (`fetch_sectors`, `fetch_sector_pe`, `fetch_industry_snapshot`), which were the same defect as the Tier-1 sector-P/E fix left standing one file over; the earnings-calendar window; and the two `fred.rs` sites above.

**Deliberately left, and annotated in place so the next sweep does not re-derive them:** the fetch-range upper bounds (`job.rs`'s rate-history, deep-history and sector-P/E-history ranges; `fmp.rs`'s index, company-EOD and dated-EOD ranges; `outcome.rs`'s FMP-fallback lookback count and price-bar `fetch_to`).
A range ending one untraded day late serves no row for that day, and each range's `from` is a rolling multi-day window rather than a session boundary.

Finding 7 also stopped being the inert contract divergence it was carried as.
The value it stamps reaches `first_breach_at`, `last_evaluated_at` and the `confirmed_at` that finding 3's fix will read, so the run's one ET session date is now the single source for every dated ledger stamp — the full pass and the sweep alike, which previously could disagree by a day.

Three pins, all confirmed failing against the pre-change shape:
`an_evening_et_run_does_not_age_out_a_seven_et_day_old_house_view` (a 7-ET-day-old report survives a 9 PM ET run — the case the prefix gate dropped);
`a_report_written_after_the_et_rollover_ages_from_its_own_session` (the inverse the one-legged fix would have introduced: a report stamped 9 PM ET ages from its own session, not the UTC date it has already rolled into);
and `the_sweep_dates_itself_on_the_et_session_not_the_utc_prefix` (including the degradation arm — a date-only or malformed stamp keeps the old prefix read rather than panicking or emptying).

Two existing house-view fixtures had to be restated: both stamped their reports at `T00:00:00Z`, which is 8 PM ET on the **prior** day, so each test read one more ET day than its comment claimed.
They now use mid-session stamps, which is the point of the conversion — a midnight-UTC stamp is exactly the ambiguity the gate resolves, and a test should not rest on it.
Neither fixture change weakened an assertion; the boundary each test pins is unchanged.

## Batch A — the run-gating batch (built)

All twelve ruled items are built, the ET class among them (its own section above).
Every fix carries a pin **confirmed failing against the exact pre-fix shape** by reverting only that fix and re-running; the boundary pins that guard against over-correction pass either way, deliberately.

- **1 — the off-scale guard.** `resolve_series` resolves a signed P/E or debt/equity **unevaluable** rather than comparing, on the shared `on_scale` predicate.
  Unevaluable, not a sentinel "maximal": a ledger threshold is model-authored with an open comparator, so any sentinel asserts a direction that is wrong for the other one ("debt/equity below 1" must not be satisfied by negative equity either), and `f64` infinities do not survive the `serde_json` round-trip `last_value` takes.
  That is a deliberate divergence from `assign_stock_tier`, whose closed internal predicates *can* express maximal safely and do, and the comment says so at both ends.
  The producers were re-checked: only P/E and debt/equity can go negative — P/S and P/B take the positive-denominator derive, and the signed P/E derive is deliberate upstream (grade-v2.1, so the loss-maker valuation guard stays reachable), which is why the guard belongs at the comparator.
  Zero debt stays on-scale, so the two series carry different admissible ranges rather than one shared floor.
- **2 — the stale-basis bridge.** A holding's own series now floors at `min(anchor, intrinsic vintage)`; the benchmark legs, read from the anchor and never bridged, keep the anchor floor.
  An **existing test had pinned the hole as intended behavior** — it asserted the bridge is *excluded* for a lone rule-demotion episode, which is precisely the guard the ET slice left half-built.
  It is rewritten to pin the fix (the fetch reaches the intrinsic session; the bridge keys there, proven by the bridge close sitting strictly below the anchor session's on a rising series), with a second test keeping the excluded-not-guessed arm for a session the source genuinely cannot serve.
- **3 + 4 — the falsifier event, ruled and built as one.** `ConditionCrossing` now carries the confirming pass's own `confirmed_at`, taken from the condition's evaluation state; the event stamps from it and **dedups on it** instead of `confirmation_observation_id`.
  One field closes both halves, which is why they were ruled together: without both, the fix produces one correctly-stamped event beside ~40 mis-stamped duplicates.
  The pin measures the accrual directly — three passes re-raising one standing breach recorded **3** events before, **1** after.
  A legacy state that reached its count before the field existed falls back to the consuming run's ET date, the pre-fix behavior, so an upgrade loses no event.
  Note this closes a divergence in the *code*, not the docs: `portfolio-analysis.md` already specified the confirming run's ET session date, and the code stamped the consumer.
- **9 — the total-return window.** The dividend window ends at the **end bar's** session, not the calendar window end, mirroring the entry side's existing strict bound.
  A window end on a weekend, a holiday or a stale cache tail leaves days between the last close and the bound, and a dividend going ex inside them is not yet out of the price the label divides by — always-positive inflation, since dividends only add.
  The boundary pin holds an ex-date **on** the end session inside the window, so the fix cannot over-correct into dropping a legitimate payment.
- **10 — the abstention comparison.** The abstained-prior arm now compares against the **standing episode's** own recorded action and lean (`StandingDecision`), selected in insertion order exactly as the extend target is.
  The code comment was right that an abstained verdict carries no action — `InsufficientEvidence` has none — but the episode it extended does, and that is the forecast calibration scores.
  Both directions are pinned: a moved action across an abstention opens, an unchanged one still extends, so the fix mints no episode per abstention.
- **11 — the wall-clock class, swept whole.** Insertion order everywhere: `load_episodes` orders by `id` (its own ordering was wall-clock-primary, so a last-in-vec fix alone would not have been insertion order), `prune_matured_episodes` prunes by `id`, and all four in-memory selections take the last match.
  The pin inverts the clock across two runs and asserts the extension lands on the successor; pre-fix it landed on the wall-clock-newer predecessor, which would have shadowed the new episode for the rest of its twelve months.
  Clippy's `rfind` suggestion was adopted over `filter(..).next_back()` — the build stays warning-free.
- **13 — the basis-continuity gate** (the user ruling, and the largest item).
  A typed `StatementBasis` is stamped on `CompanyFinancials` at the shared `apply_ttm_statement_basis` choke point every statement-consuming path already passes through, so no producer can set the levels without recording their basis, and each condition's evaluation state records the basis its streak was accumulated under.
  On a change the pass types the statement-derived series unevaluable, drops the streak — its observations were taken on the other measurement — and re-stamps, so the gate fires **once per flip**, not permanently.
  Deliberately not the clean arm: a clean read would clear the acknowledgment and report a thesis confirmation the evidence does not support.
  Scope is `statement_derived()`, which is **wider than the filing cadence** on purpose — the three multiples are keyed to the marks' trading day but their denominators are statement lines, and that combination is the dangerous one, since a market-cadence series confirms in two distinct observations and so a basis step can confirm within days.
  Two boundaries came out of implementing it, both closed:
  - **The sweep is not the authority.** Its evaluated values span two bases at once — filing series off its own refresh, the multiples rescaled from the stored full-pass audit by price alone — so one marker cannot describe them. Left set, a refresh that flipped basis would adopt the new stamp while the multiples were still on the old one, and the genuine flip at the next full pass would pass unnoticed. The sweep clears the marker; the full pass, where every evaluated value comes from one basis, owns the gate.
  - **A fund carries no statement basis at all**, which is distinct from a resolved fallback, so the stamp is `None` rather than `Annual` where there are no quarterly rows.
  A pre-stamp state adopts the current basis without a discontinuity, so shipping this does not spend every holding's first pass on a fabricated one — which also means **the first flip the gate can catch is the first one after a pass has stamped**, and the big run does that stamping.

Doc homes updated with the batch, each at its existing single home: the two new unevaluable reads and the gate's ownership (`portfolio-analysis.md` §The position thesis ledger), `confirmed_at`'s provenance and the one-event-per-confirmation rule, the abstention's comparison basis, insertion-order episode selection, the vintage floor, the dividend window's bar-bounded ends, and the evaluation state's new field (`storage.md`).

## External review round 1 (Batch A)

Three findings, all verified against the code and all adopted.
Two of them are this walk's own thesis charged against this batch — a class declared swept whole that was not — which is the third time that lesson has had to be re-learned in this block, and the first two were self-inflicted the same way: by grepping the file a finding named instead of the crate.

- **The basis-continuity gate was bypassed by a zero-row quarterly response.**
  `apply_ttm_statement_basis` stamped `None` when there were no quarterly rows at all, reasoning that no statement basis applies to a fund.
  But a stock whose FMP quarterly set comes back **empty** — the same empty-200 shape the sector-P/E snapshot serves — then has its levels filled from SEC annual facts, and `None` exempted exactly that holding from the gate, because the gate acts only on a `Some` basis.
  The reachable half is the multiples: they key their observation on the marks' trading day, not a statement print, so they resolve normally with zero quarterly rows while the filing-cadence series go unevaluable for want of a period end — so a TTM-authored P/S threshold met an annual-basis ratio and could confirm on a market cadence.
  The stamp now **refines at the merge**, which is the only point that knows what finally supplied the levels: an adopted TTM window stands, any present statement level with no adoption is `Annual`, and `None` is reserved for a holding carrying no statement level at all.
  The original regression test used three quarterly rows and could not have caught it; the new pin uses zero, and a second pin holds `None` to its real meaning.
- **The wall-clock sweep left two production paths.**
  `lost_active_symbols` decided whether a corrupt active episode row was superseded by comparing `anchor_at`, so under a backwards clock step a later-inserted recovery episode looked older, the symbol stayed permanently flagged lost, and a fresh debut episode opened **on every subsequent run** — unbounded one-run episodes polluting every cohort read.
  It now reads insertion order through a new `readable_before` on the skipped row (its position in the `id`-ordered scan), which answers supersession without exposing SQL ids to the policy.
  And `construction.rs` inherited a carried stock's sector with `max_by(anchor_at)` — the same shape as the sector-inheritance site the batch *did* fix in `outcome.rs`, one of two.
  A crate-wide sweep now confirms the only remaining `anchor_at` comparison in production is none; the sole match is a test asserting its own fixture inverts the clock.
- **The ET sweep missed a provider-date staleness bound.**
  `cot.rs` compared the CFTC's own `report_date` against `Utc::now().date_naive()`, so an evening-ET run aged a report by one and dropped a snapshot exactly at the 21-day bound a session early.
  This one is squarely inside the cross-cutting rule the batch wrote into `data-sources.md` — a staleness bound against a provider's observation date — so the doc claimed it and the code did not. Converted.

One finding was **accepted on a different rationale than offered**: the option-chain window built its dates from `Local::now()`.
The review framed it as contradicting the universal doc claim; it does not, because that rule explicitly excepts a fetch range's bounds, and this is one.
It was converted anyway for a reason the rule does not cover: `Local::now()` is **machine-dependent**, so the same code read a different 60-day window on a Pacific-time machine than on an Eastern one, and an expiration at the far edge dropped out of the chain and out of the options signal with it.

**A third site in the wall-clock class surfaced during that crate-wide sweep and is deliberately NOT fixed here.**
`storage::select_reports_beyond_retention` selects the 30-report retention window by `created_at DESC, rowid DESC`, so a backwards clock step at the cap can evict the report the run just wrote — cascading its Markdown, metadata and vector summary row.
It is the same premise as the ruled `baseline_snapshots` finding and worse in consequence, but it is a **new finding, not a ruled one**, and the display orderings on the same table must stay date-ordered (a report is a dated document; only the eviction selection is identity-like).
It is recorded here for the next ruling round rather than self-authorized into this batch.

**Verification after the round:** cargo **1007 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **46 node + 223 vitest** — two further lib tests.

## External review round 2 (Batch A)

One correction, and one finding held back for a ruling.

- **The session-dating rule contradicted the batch's own option-chain change.**
  `data-sources.md` states that a fetch range's bounds stay on the UTC date, and this batch had just moved the Schwab option-chain window to ET — a real contradiction in the doc the batch wrote.
  The rule now records that one range explicitly, and why it is **not** an exception to the session rule: the window read the *machine's* local date, so the same code selected a different sixty-day expiration window on a Pacific-time machine than on an Eastern one.
  The fix there is determinism, not session dating, and the range-bound exception still holds for every UTC-dated range.

- **The report-retention defect — surfaced by this batch, held for a ruling, now ruled in and built.**
  `storage::select_reports_beyond_retention` selected the 30-report window by `created_at DESC`, so a backwards clock step at the cap evicted the report the run had just written — cascading its Markdown file, its row, its vector summary and its baseline snapshots.
  It was found by **this batch's own crate-wide sweep** and recorded rather than absorbed, then held through two review rounds on scope authority rather than doubt: the ruled class was enumerated as *every episode selection*, this table is not in it, and the batch's whole subject is what happens when a fix quietly widens past what was ruled.
  The user ruled it in, so the keep window is now insertion order (`rowid`).
  **Only the eviction decision moved.** `list_recent_reports` stays `created_at`-ordered, because a report is a *dated document* and the sidebar and house view are right to read it by date — so the two windows disagree about **which** 30, never about how many (round 5's correction: a clock-stepped report is preserved by evicting the oldest *insertion* instead, leaving exactly 30 rows; nothing is retained "extra"). "Sweep the table" would have been the wrong instruction, which is why it needed a ruling a review cannot supply.
  The existing tests could not have caught it: one covers strictly ascending timestamps and the other a `created_at` tie, and neither is a **newly inserted older** timestamp. The new pin is that shape, and pre-fix it evicts the just-written report by name.

**Verification after the round:** cargo **1008 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **46 node + 223 vitest** — one further lib test, for the retention fix.

## External review round 3 (Batch A)

One finding adopted as documentation, one pushed back.

- **The retention policy's own doc comment still described the retired rule** — it said the display and retention windows share `created_at DESC, rowid DESC` and can "never disagree about which reports those are", which the fix one function below directly contradicts.
  A contradiction this batch introduced, and the sharpest kind: the constant's comment is where a reader goes to learn the policy.
  Corrected at all three homes — the constant now states that the windows are sized alike but no longer keyed alike (and, after round 5, that they differ on *which* 30 rather than on how many); `storage.md` says "30 most recently **generated**" rather than the newly-ambiguous "most recent"; and `data-portability.md` no longer claims the next run's retention pass is a no-op, which is true only when the archive arrives under the cap.

- **Insertion identity not surviving export/import: pushed back — this is not a defect, and the invariant holds.**
  The mechanics are exactly as described: the export orders reports `created_at, report_id`, carries no insertion sequence, and the import reinserts in that order, so post-import `rowid` follows date order.
  But the invariant the fix protects is *a run cannot evict the report it just wrote*, and that survives an import intact — a report generated after the import takes the highest `rowid` and sits inside the keep window.
  What the review describes as the failure is a clock-stepped report being evicted first, and by every other reading in the system that report **is** the oldest: its own `created_at` says so, the date-ordered sidebar lists it **last** of the 30 (round 5's correction — not *outside* the 30, which was true only of the un-pruned 31-row store the original pin queried), and it falls outside the house view's three-report window (round 4's correction: the *freshness gate* itself only ever examines the date-newest report, so it is the window that excludes a clock-stepped one, not the gate).
  Retention agreeing with the report's own date is the store being self-consistent, not a regression.
  It is also the category portability deliberately excludes: `rowid` is a local database artifact, in the same class as the `markdown_path` the import re-derives rather than carries.
  Serializing an insertion sequence to preserve it would make a machine-local artifact part of the archive contract — a format-version bump — to defend a case the invariant does not need defending in.
  What the finding *did* earn is a doc line, since none of this was written down: `storage.md` and `data-portability.md` now both state that insertion order is not portable and that the two windows coincide again after an import.

**Verification after the round:** cargo **1008 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **46 node + 223 vitest** — doc-only, unchanged from the retention fix.

## External review round 4 (Batch A)

One finding, adopted: the prose written to *document* round 3's pushback over-promised, in three places.
The ruling itself is untouched — the review explicitly grants that leaving insertion order out of the archive can stand as a documented contract choice — but the equivalence claims dressed around it were wrong, and a pushback defended with a false claim is worse than no pushback.

- **"At the cap it evicts exactly as it would have on the source machine" is false.**
  That is the very case the fix distinguishes: a clock-stepped report has the *highest* insertion order on the source and survives, and the *earliest* after an import, where it is evicted first.
  Both docs now say so plainly — a clock-stepped report survives on the source and can be evicted after an import — and name it the accepted cost of leaving a machine-local artifact out of the archive.
- **"A report generated afterwards is newest by both readings" is false whenever the clock is still behind.**
  Highest insertion order guarantees *retention*-newest and nothing about the date-ordered sidebar.
  The guarantee is restated at its real width in both homes: a run can never evict the report **it just wrote**. That is what the fix protects, it is unaffected by an import, and it never needed the wider claim.
- **The round-3 record's own reasoning was imprecise about the house view.**
  It said the freshness gate "already ages that report out"; the gate only ever examines the **date-newest** report, so what excludes a clock-stepped one is falling outside the date-ordered window, not the gate.
  The conclusion is unchanged — every date-based consumer already treats it as old — but the mechanism named was the wrong one, and the record is corrected in place rather than quietly.

Round 4 confirmed the remaining Batch B work resolved: the route-specific predicates match the two actual prompt renderers, and the previously missing route coverage is present.

**Verification after the round:** cargo **1008 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **46 node + 223 vitest** — doc-only, unchanged.

## External review round 5 (Batch A)

One finding, adopted — and it lands on the **test**, which is why the claim survived four rounds of prose correction.

- **The sidebar claim described a state production never renders.**
  The retention pin selected the evictee and then queried the store **without deleting it**, so it asserted against 31 rows and concluded the sidebar places a clock-stepped report outside the newest 30.
  Production prunes inside the same run (`pipeline::prune_old_reports`, after the insert), so once the evictee is gone exactly 30 rows remain and the sidebar lists **all** of them — the stepped report included, last by date.
  The pin now deletes the evictee first and asserts that: 30 rows, the preserved report shown, and shown last.
  This is the round-2 pin that every later round's prose then quoted as evidence, which is how a wrong assertion propagated into three documents.
- **"Keeping the extra report" was wrong in the same way.**
  Retention does not retain an additional row; it preserves the stepped report by evicting the **oldest insertion** instead.
  The two windows therefore disagree about *which* 30, never about how many — corrected in the constant's comment, in `storage.md`, and at both places the record repeated it.

Round 5 confirmed round 4's three portability corrections, and that the non-portability decision and the house-view-window correction remain sound.

**Verification after the round:** cargo **1008 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **46 node + 223 vitest** — the reworked pin replaces an assertion rather than adding one, and still fails against the pre-fix ordering.

## Verification

At Batch A complete: cargo **1005 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **46 node + 223 vitest** — seventeen new lib tests against this session's 988 baseline, plus three fixtures corrected (the two house-view stamps and the bridge-exclusion test that had pinned finding 2's hole as intended behavior).
No frontend change in the whole batch.

At the ET-class sweep: cargo **991 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **46 node + 223 vitest** — three new lib tests against this session's 988 baseline.
No frontend change: `etDate.ts` mirrors `market_clock` and neither moved, so the byte-for-byte parity contract is untouched.

At the converged batch (post external review round 2): cargo **988 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **46 node + 223 vitest** — four new lib tests against the pre-change baseline of 984 lib.
Round 2 was doc + comment only, so the counts are unchanged from round 1.

Round 1 added `a_failed_sector_pe_snapshot_records_its_gap_on_every_fund_not_just_the_first`, an end-to-end two-ETF run over a snapshot source that errors, asserting **both** audits carry the typed gap.
It was confirmed to fail against the exact pre-fix shape — the gap pushed inside the memoization branch — where the first fund keeps it and the second reports `ITOT lost the snapshot gap: []`.

The three original pins, one per fix:
`sector_pe_candidates_start_at_the_et_session_and_walk_back_weekdays` (an evening-ET instant leads with the session that traded, not the rolled-over UTC day; the walk continues over earlier weekdays and skips weekends; a Sunday run starts at the prior Friday);
`negative_equity_reads_as_maximal_leverage_in_the_tier_not_minimal` (a strong large cap with negative equity takes High, the isolated leverage leg scores 0, and the Low conjunction rejects it independently of leg order);
and `base_metrics_price_legs_match_the_window_the_quick_check_evaluates` (a fixture whose deep series has tripled while the 180-day window is flat — the ledger legs must equal `engine::compute_metrics`'s, and the fixture asserts the two windows genuinely differ).

External review round 1 found three issues, all verified against the code and all adopted: the first-fund-only gap propagation, the cardinality / stale-comment divergence the walk-back introduced, and the record's own inconsistent finding counts (it claimed "eight code and eight documentation" remaining where 13 + 10 = 23 carry, and enumerated eleven items under "eight").
The negative-equity tier fix and the fund-metric parity fix were confirmed clean through their production consumers.

Round 2 found **one** — and it is this walk's own thesis, committed by this walk.
Round 1's cardinality fix corrected the two homes its finding cited (the endpoint row, `portfolio-workflow.md` §Step 6a) and missed a third: `data-sources.md`'s **canonical framing sentence** for the very axis in question, which flatly said run-level calls "fire once", plus the `job.rs` cache comment saying the snapshot fires once per exchange.
Fixing the sites a finding names and stopping there is exactly the failure mode these twelve passes were convened to find, and it recurred one round after being written down as the lesson.
Both homes now defer to each row's own count.

One correction back to that finding: "fire once" was **already** inaccurate before this branch, independently of the walk-back — `historical-sector-pe` is run-level at *one call per sector × exchange*, which is not once and scales with the held funds' distinct sectors.
So the sentence was a pre-existing over-statement that the walk-back made visible, not purely its fallout.
`trade-opportunities-workflow.md` carries the same "run-level calls fire once" phrasing for the TO endpoint table; that surface is unbuilt and unaffected, so it was deliberately left alone.

One further defect was introduced and caught inside this session: the engine test's first insertion landed between an existing doc comment's `#[test]` and its function, stacking two attributes on one test so it registered and ran twice.
The lib count (984 → 988 for three new functions) is what surfaced it; corrected to 987.
