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
- **Basis-flip rate** (carried from C3's fourth finding, accept-with-note candidate) — how often a one-quarter feed gap drops a holding to the SEC annual basis, and whether the resulting multiple move crosses a ledger condition.

## Verification

At the converged batch (post external review round 1): cargo **988 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **46 node + 223 vitest** — four new lib tests against the pre-change baseline of 984 lib.

Round 1 added `a_failed_sector_pe_snapshot_records_its_gap_on_every_fund_not_just_the_first`, an end-to-end two-ETF run over a snapshot source that errors, asserting **both** audits carry the typed gap.
It was confirmed to fail against the exact pre-fix shape — the gap pushed inside the memoization branch — where the first fund keeps it and the second reports `ITOT lost the snapshot gap: []`.

The three original pins, one per fix:
`sector_pe_candidates_start_at_the_et_session_and_walk_back_weekdays` (an evening-ET instant leads with the session that traded, not the rolled-over UTC day; the walk continues over earlier weekdays and skips weekends; a Sunday run starts at the prior Friday);
`negative_equity_reads_as_maximal_leverage_in_the_tier_not_minimal` (a strong large cap with negative equity takes High, the isolated leverage leg scores 0, and the Low conjunction rejects it independently of leg order);
and `base_metrics_price_legs_match_the_window_the_quick_check_evaluates` (a fixture whose deep series has tripled while the 180-day window is flat — the ledger legs must equal `engine::compute_metrics`'s, and the fixture asserts the two windows genuinely differ).

External review round 1 found three issues, all verified against the code and all adopted: the first-fund-only gap propagation, the cardinality / stale-comment divergence the walk-back introduced, and the record's own inconsistent finding counts (it claimed "eight code and eight documentation" remaining where 13 + 10 = 23 carry, and enumerated eleven items under "eight").
The negative-equity tier fix and the fund-metric parity fix were confirmed clean through their production consumers.

One further defect was introduced and caught inside this session: the engine test's first insertion landed between an existing doc comment's `#[test]` and its function, stacking two attributes on one test so it registered and ran twice.
The lib count (984 → 988 for three new functions) is what surfaced it; corrected to 987.
