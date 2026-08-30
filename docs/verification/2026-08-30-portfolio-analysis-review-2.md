# Portfolio Analysis review 2 — the blind sweep after the 2026-08-24 review's fixes (2026-08-30)

A second large-scale review of the Portfolio Analysis job, run after the 2026-08-24 review's findings were fixed, to confirm the fixes hold and nothing regressed.
Phase 1 (this record's first half) was a blind pass: it read nothing under `docs/verification/`, nothing under `.metis/`, and no git history, so the 2026-08-24 record could not steer it.
Phase 2 (appended below) cross-checks the non-minor findings against that record.

## Scope and method

- Reviewed: every file under `src-tauri/src/portfolio/`, `local_model.rs`, `market_clock.rs`, the `web_research/` modules, the portfolio-facing adapters (`fmp.rs`, `fred.rs`, `sec.rs`, `finra.rs`, `cboe.rs`, `cot.rs`, `schwab.rs`, `schwab_live.rs`), the Portfolio page's numerics and its `types.ts` contract, and `logic-flow-docs/portfolio-analysis-logic-flow.md` against the code.
- Specs read: `docs/portfolio-analysis.md`, `docs/portfolio-workflow.md`, `docs/local-models.md`, `docs/local-model-operations.md`, `docs/web-research.md`, `docs/storage.md`, `docs/data-sources.md`, `docs/schwab-integration.md`, `docs/configuration.md`, `docs/interface.md`, and the logic-flow doc.
- Method: sixteen scoped, read-only reviewers ran in parallel — the two engine halves, the fund path, the pre-profit overlay, outcome learning, the quick check, the dossier and adapters, three pipeline slices (flow, the 6g validator, the prompts and schemas), the job spine, research and distillation, the store / types / local-model seam, two doc-alignment walks, and the frontend — each under the same independence rule.
  Every reported finding was then re-verified against the code by the coordinating reviewer before entering this record; the reqwest claim (N2) was verified against the vendored crate source.
- Priorities, in the order the user set: financial correctness, run-killing bugs, doc/code alignment against the logic-flow doc.
- Severity: **non-minor** changes a financial output (a metric, grade, target, tier, conviction, action, a number rendered to the model or user, a condition evaluation, an outcome label), can crash, abort, or hang a run, or silently corrupts or loses persisted state.
  **Minor** is cosmetic, wording, a log or audit line, an edge unreachable under the current producer, or harmless drift.
- One read-only copy-out of the dev store (the 2026-08-13 run) settled a wire-shape question (M21); only aggregates were read.

## Summary

- Five non-minor findings and thirty-four minor ones.
- No panic, unwrap, index, or slice on network, model, or store data was found anywhere in scope; the one run-killer is a transport-semantics defect (N2).
- The financial core held: the sub-score and grade math, the scenario-target function and its fallbacks, the hurdle, the tiers, the split-adjustment bridge, the TTM statement basis, the FMP / SEC / FRED / FINRA / CBOE parsing (units, signs, dating), the 6g validator, every prompt's rendered units, every schema against its struct, the checkpoint and resume contract, the persist order, the SSRF guard, the research loop's termination, and the frontend's formatters.
- The logic-flow doc aligns with the code on every constant, gate, and ordering checked; the drift found is wording (M29–M34).

## Non-minor findings

### N1 — A full-year guidance bound pairs against a single-quarter actual (the period-span collision)

- Category 1 (financial correctness).
  Confidence: high on the mechanism, medium on frequency.
- Where: `src-tauri/src/portfolio/pre_profit.rs` `normalize_period` (~929–984) and `execution_read` (~1759–1874); the 6d observation schema and prompt lines in `src-tauri/src/portfolio/distill.rs` (~440–470, ~1766–1790).
- What: `normalize_period` maps `Q4 2026`, `H2 2026`, `FY2026`, and a bare `2026` all onto `2026-12-31` (and `Q2` / `H1` onto `06-30`).
  `execution_read` then pairs a guidance row to an actual on `(metric identity, units, issuer scope, normalized period)` alone.
- Failure scenario: guidance-low `{deliveries, "FY2026", 500,000, published 2026-02-15}` and actual `{deliveries, "Q4 2026", 140,000, published 2027-01-05}` normalize to one key.
  The bound is ex ante (published on or before 2026-12-31 and before the actual), so the period is comparable and the miss ratio reads 0.72.
  That is a recorded miss and, as the newest comparable period, `material_single_miss`; with any second leg (constrained runway is common on an eligible name) it is `severe_deterioration`, so the engine arm's conviction caps at Low and its action set narrows to {trim, sell all}, both rendered to the interpretation and action prompts.
  The same collision reaches the dedup key and `backfill_required`'s comparable-period count.
  If the model also emits the FY actual under the same key, `select_actual` resolves the two actuals by confidence and then date, so the outcome depends on the model's confidence fields.
- Why wrong: a period end is not a period; a guidance-versus-actual attainment test is meaningful only over the same span.
  The spec and the code agree on the one-ISO-period-end convention (`docs/portfolio-workflow.md` §Step 6e; logic-flow ~1172), and nothing on the wire carries span: the schema types `period` as a free string and the prompt states no period convention.
  Annual guidance beside quarterly actuals is the canonical pre-profit shape (an EV maker guiding annual deliveries and reporting them quarterly).
- Reachability: any overlay-eligible stock whose research yields annual guidance and quarterly actuals for one metric; reachable on run 1.

### N2 — The streaming transport deadline is an absolute total cap, not the per-read idle limit the design assumes

- Category 2 (run-killer).
  Confidence: high — verified against the vendored `reqwest-0.12.28` source.
- Where: `src-tauri/src/local_model.rs` `chat_streaming` (~926–935) sends with `.timeout(deadline)` where `deadline = request_deadline(req, true)` is the prefill term alone; the module's premise sits at ~48–79.
- What: the per-request `RequestBuilder::timeout` on the blocking client writes the async request's `TotalTimeout` extension (`blocking/request.rs:360–362`, whose own doc reads "applied from when the request starts connecting until the response body has finished").
  `into_async` carries that extension through; `async_impl/client.rs:2629–2634` arms `tokio::time::sleep(deadline)` at request start and `Response::new` wraps the body in `TotalTimeoutBody`, which "does not reset upon each chunk, but rather requires the whole body be streamed before the deadline is reached".
  The blocking side's header wait and per-read wait are applied in addition; the absolute deadline fires first on an active stream.
  The module comment describes `ClientBuilder::timeout` (which stays on the blocking side), not the per-request timeout the code actually uses.
- Failure scenario: the interpretation, role-risk, and action calls stream at `NUM_CTX_INTERPRET` = 131,072, so the deadline is 1,310,720 ms ≈ 21.85 minutes from request start, regardless of throughput.
  A holding whose prefill plus thinking chain runs past that with tokens flowing normally gets `TimedOut` on a body read; `is_transport_timeout` names it a deadline trip ("the daemon stalled, its throughput fell under the drafted floor…"), `retry_class` returns `None` (deadline trips never re-attempt), and because the call sits on the required 6c–6f path the multi-hour run fails hard.
  A 65,536-token reservation at the 12 tok/s floor admits a ~91-minute chain; the actual streaming cap is ~22 minutes total.
  The non-streaming path (research turns, distillation) sums prefill and decode and generates before the headers, so it is unaffected.
- Why wrong: `docs/local-models.md` §The local-model adapter seam — "transport cannot cut a chain that stays inside its reservation while the daemon holds the drafted floors", and "that same value then bounds each body read as an idle limit".
  The deadline tests (~1682, ~1703) exercise silence only, where idle and total semantics coincide, which is why nothing caught it.
- Reachability: run 1, on any holding whose streamed call exceeds ~22 minutes of wall time — the heavy-prompt tail of a ~47-holding book is where it lands.

### N3 — The quick check evaluates trailing-return and return-volatility conditions over a widened price window

- Category 1.
  Confidence: high on the mechanism; reachability narrow.
- Where: `src-tauri/src/portfolio/quick_check.rs:1393` — `eval_fin.price_history = closes.iter().map(|d| d.value).collect();` with `closes` from `price_and_closes(symbol, eod_lookback_for(boundary, today))` (~593–603, ~660–667).
  The full pass's `price_history` is the fixed 180-day undated window (`fmp.rs:1743`, `COMPANY_EOD_LOOKBACK_DAYS = 180`).
- What: `eod_lookback_for` widens the sweep's fetch to reach the split-bridge anchor bar (`(today − boundary) + 14` days, floored at 180) and the sweep hands the whole widened series to `price_history`, so `compute_metrics` derives `trailing_return` (first-to-last) and `return_volatility` over the wider window.
  Their only sweep consumers are the `trailing-return` and `return-volatility` ledger series; the P/E, P/S, and P/B rescale reads the stored audit instead.
- Failure scenario: a carried holding whose boundary (its last-pass vintage, or a carried-forward anchor bar) is older than ~166 days.
  A condition authored as `trailing-return below −0.20` against the prompt's 180-day figure is evaluated on a ~13-month first-to-last return, so a partly recovered six-month drawdown, or a flat last six months after an older decline, breaches or clears on a window the threshold never described, and market cadence confirms on two prints.
- Why wrong: the series vocabulary defines the read as the engine's short-window momentum figure (`docs/portfolio-analysis.md` §Starting parameters: "the short undated EOD window, never the deep dated history"); the widening exists for the anchor bar only.
- Reachability: not on the debut run or a run-2 sweep; long-carried holdings only.

### N4 — A confirmed falsifier crossing attaches to the episode the same run just opened, not the one that carried the condition

- Category 1 (an outcome-learning read).
  Confidence: medium — the mechanism is certain; whether the design intends the new episode is the ruling to make.
- Where: `src-tauri/src/portfolio/outcome.rs` `plan_episodes` — the open loop (~1900–1935) pushes the new episode before the crossing loop (~1938–2024) selects `rfind(Active, symbol)`.
  `EpisodeState` carries only `Active` and `Matured` (~163–167), so the prior episode stays Active beside the new one.
  `stamp_lead_times` (~1332–1362) positions a `confirmed_at` before the entry bar at the window's first bar.
- Failure scenario: episode E1 (Hold, bear line B1) is active.
  This run's 6b confirms falsifier `c-1`, the crossing renders into the prompts, and the action moves to Trim; `plan_episodes` opens E2 first and then attaches the event to E2.
  On E2 the confirmation predates the anchor, so the lead time is non-negative by construction and measures the new thesis's line; E1 — whose bear line the falsifier guarded — records neither an event nor `no-material-drawdown` and is absent from `falsifier_lead_times`.
  This is the typical path, since a confirmed falsifier is exactly what moves an action in the same run; an earlier sweep's confirmation consumed by a run that changes the action lands the same way.
- Why wrong: `docs/portfolio-analysis.md` §Outcome learning — "A confirmed falsifier event attaches to the episode that carried the condition", and the lead-time read is defined against the episode's own bear line and window.
  The pinned test covers a state change one run before the crossing is consumed, not the coincident case.
- Consumer today: the persisted `falsifier_lead_times` derived read; no rendered surface reads it yet.

### N5 — A one-exchange sector-P/E snapshot is accepted without a gap

- Category 2 (a silent data-quality degradation of a financial output).
  Confidence: high on the mechanism; fault-conditioned.
- Where: `src-tauri/src/portfolio/job.rs` `sector_pe_snapshot` (~497–510) — both `SECTOR_PE_EXCHANGES` are fetched, an `Err` on one is dropped into `last_err`, and a non-empty partial `rows` returns `Ok`; the fund-side gap at ~1577–1587 records only the `Err` arm.
  `sector_pe_history` (~525–553) accepts a partial read the same way.
- Failure scenario: NYSE serves and NASDAQ faults (a 429 past the retry ladder, a 5xx) on the first candidate session.
  Every priced fund in the run blends a NYSE-only snapshot in `blend_sector_pes` while its constant-mix history blended both boards, so today's composite yield sits on a different basis than the history it is ranked against (the valuation percentile and the anchor spreads), with no `sector_pe_gap`, no conviction degradation, and no data-health line.
  Technology is where the two boards' P/Es diverge most.
- Why wrong: `docs/portfolio-analysis.md` §Asset eligibility requires the same exchange-blend convention on the snapshot and its history, and Step 5's enriching loads are "each fail-soft to a typed gap counted on data health".
  The code comment calls the partial read deliberate ("the original single-date behavior, kept"); the missing gap is the defect.
  The failed request is visible only as a run-tracker row.

## Minor findings

### Engine

- **M1 — `normalized_assumption_value` passes "cent(s)" and foreign-currency units unscaled.**
  `src-tauri/src/portfolio/engine.rs` ~2392–2393 and ~2451: `"cent"` / `"cents"` and `eur` / `gbp` / … sit in `CURRENCY_TOKENS`, and the `ForwardEps` branch returns `Ok(value)` with no scale or FX step.
  A forward fact `150, "cents per share"` fills the shadow driver at 150.0; a `5.2, "EUR per share"` fills a USD driver unconverted.
  Shadow-only today (the 2026-08-24 ruling), so only the audit's would-have line is wrong; it would be non-minor on write-back promotion.
- **M2 — Stale doc comment in `normalized_assumption_value`.**
  `engine.rs` ~2443–2446 says an empty unit string passes; the first check (~2404–2406) rejects it.
  The code's behaviour is the safer one.
- **M3 — The logic-flow's "Financial statements" floor bullet overstates `analyze`.**
  Logic-flow ~882–886 lists financial statements as a floor requirement beside the two-real-sub-scores bar; `engine.rs` ~1592–1614 floors on `usable_price` plus the sub-score count, and valuation can be real from P/B alone and risk from volatility plus D/E, so a stock with no income statement can clear the count with quality imputed.
  The canonical doc's own parenthetical names the two-real-sub-scores bar as the mechanism.

### Fund path

- **M4 — The role-risk prompt calls any option-overlay fund "leveraged/inverse".**
  `src-tauri/src/portfolio/pipeline.rs` ~2978–2980 renders `STRUCTURAL FLAG: structurally path-dependent (leveraged/inverse)` whenever `structural_flag` is set; `fund.rs` `classify` sets that flag from `OPTION_OVERLAY_FRAGMENTS` on every non-leveraged role-risk branch (bond, commodity, unknown, weightless equity, below the US guard).
  An international "… Premium Income" fund below the US guard is described to the model as a daily-reset product.
- **M5 — "Ultra-Short" bond funds classify as leveraged/inverse.**
  `src-tauri/src/portfolio/fund.rs` ~64–65 and ~223–231: the bare `short` / `ultra` test is suppressed only by `short-term`, `short term`, `short duration`, `short maturity`, so `JPMorgan Ultra-Short Income ETF`, `Vanguard Ultra-Short Bond ETF`, and `Goldman Sachs Access Ultra Short Bond ETF` take `LeveragedInverse` with `class_label` "leveraged / inverse vehicle", `structural_flag` true, and the `role_reason` "structurally path-dependent (leveraged / inverse daily reset) — a buy-and-hold read is structurally unsound" appended to the evidence gaps and rendered to the role-risk and action prompts.
  Routing is `role_risk_only` either way.
  This extends the cost ruled 2026-08-05 for "iShares Short Treasury Bond" (recorded in the code comment as a wrong class label and a big-run watch), but the hyphenated ultra-short shape is common and the reason text reaches the action call.
- **M6 — A sector-P/E history fetch failure is recorded on the first fund only.**
  `src-tauri/src/portfolio/job.rs` ~1589–1605 memoizes an empty history and pushes the gap on the fund whose turn triggered the fetch; a later fund sharing the sector reads the same empty history with no gap.

### Pre-profit overlay

- **M7 — An integral value ≥ 1e15 trims its search needle to a bare digit.**
  `src-tauri/src/portfolio/pre_profit.rs` ~1027–1032: `Display` for `f64` never uses an exponent, so `2e15` renders `2000000000000000`, takes the non-integer branch, and the trailing-zero trim reduces the needle to `2`.
  Unreachable with plausible operating values.
- **M8 — An en dash used as a minus never reads negative.**
  `pre_profit.rs` ~1169–1182 reads only the ASCII hyphen-minus and U+2212 as a sign, as `docs/portfolio-workflow.md` §Step 6e states.
  A press release printing `–12%` (U+2013) beside a model row of `+12` corroborates at the wrong sign; the range case the carve-out cites is already handled by the digit-before test.
  Documented behaviour, so recorded as a shape rather than a defect.

### Outcome learning

- **M9 — `CalibrationSnapshot.dgs2` is the consuming run's print on a carried open while `hurdle` is the intrinsic run's.**
  `src-tauri/src/portfolio/outcome.rs` ~1843–1844: `hurdle` reads the (prior) audit and `dgs2` reads `input.dgs2`, so a rule-demotion open records a hurdle anchored on one DGS2 beside a different DGS2.
  Rule-demoted episodes are excluded from every grade-linked read.
- **M10 — `stamp_lead_times` clamps a confirmation dated after the window end onto the last bar.**
  `outcome.rs` ~1354–1359: `position(..).unwrap_or(len − 1)`.
  Reachable only when a 12-month label pended past its window inside the grace and a crossing confirmed after `w_end`; the lead time then reads understated, and exactly zero when the breach is the last bar.

### Quick check

- **M11 — The revision-move leg compares two NTM blends, so calendar roll-forward reads as a revision.**
  `src-tauri/src/portfolio/quick_check.rs` ~1020–1023 compares the stored NTM mid to the fresh NTM mid; the blend's near weight (`fmp.rs` ~6274) is `days to the near period end ÷ 365`, so with unchanged consensus FY1 5.00 / FY2 6.00 the mid moves 5.33 → 5.62 over a May-to-August gap and badges a `RevisionMove`.
  Doc and code agree (§Starting parameters names the NTM blend), so this is a drafted-convention question; the badge is quiet and enters no prompt.
  The narrative-vs-reality read shares the comparator, where the roll can mask a genuine hype read below the 5% expansion floor.
- **M12 — The newly-fails hurdle flag prints the base TR while the bull leg decides.**
  `quick_check.rs` ~1691–1700 renders "re-anchored TR base X% vs hurdle Y%"; `fails` is `tr_bull < hurdle`.
  The flag fires on the right condition.
- **M13 — Family notes blame the split-bridge anchor when the quote refresh failed.**
  `quick_check.rs` ~885–889 leaves `bridge` `None` whenever the price is absent and an anchor exists; ~1034–1044 and ~1609–1620 then name "split-bridge anchor unresolvable".
  The `unknown` states are right; only the cause named is off.

### Dossier and adapters

- **M14 — Uranium and coal producers under FMP sector "Energy" get the oil/gas sleeve.**
  `src-tauri/src/portfolio/dossier.rs` ~106–111 maps `Energy` → the Energy group and `Basic Materials` → Metals; `job.rs` ~228–237 files `PURANUSDM` under Metals.
  A uranium miner's commodity context is WTI and Henry Hub, and the uranium print the run fetches attaches to no holding.
  Prompt context only; spec and code agree.

### Pipeline flow

- **M15 — `HoldingAudit.model_ids` is not first-call order on a distinct roster.**
  `src-tauri/src/portfolio/pipeline.rs` `used_model` sites (880/894, 1149/1360): the fast id is recorded after distillation and the reasoner id after interpretation, though 6c research turns ran on the reasoner first; a 6d call routed up to the reasoner still records the fast id.
  Unobservable on the default roster.
- **M16 — `NUM_PREDICT_DISTILL`'s rationale is stale.**
  `pipeline.rs` ~5248–5249 says "Distillation emits 2–3 sentences; generous by two orders of magnitude"; the 6d reduce now emits the combined object, the per-topic layer with claims and URLs, the typed side-channels, and verbatim observation excerpts.
  A length stop on that call is an unclassified hard failure; adequacy of 8,192 is an open question (below).

### The 6g validator and the deltas

- **M17 — Input-delta rows can render a real move as no move.**
  `pipeline.rs` ~2550–2556 tests `old != new` exactly and renders `{old:.0} -> {new:.0}`, so 61.7 → 62.3 renders `62 -> 62`; `delta_val` (~2332–2335) does the same at four places for the metrics.
  The row still resolves as evidence; a what-changed row copied from it is dropped by the no-move check.
  `fmt_crossing_pair` already extends precision until a pair orders; these rows did not get that treatment.
- **M18 — A misattached doc comment.**
  `pipeline.rs` ~2388–2396: `/// Append one input-delta entry…` sits above `grade_branch`.

### Prompts and schemas

- **M19 — A rule-demoted `hold` renders as "the prior run's action" and as "yours".**
  `pipeline.rs` ~4126–4131 and ~3131–3133 read `carried_action` (`mod.rs` ~2008–2014), which ignores `action_source`; `job.rs` ~2097–2112 persists the over-age demotion into the verdict's action.
  The next full pass tells the action call to hold the line on a rung no model chose, and the retrospective labels it the model's own.
  Reachable only through an over-age carry.
- **M20 — The unrealized-P/L tax framing ignores the profile's tax posture.**
  `pipeline.rs` ~4115–4125 asserts a tax benefit or cost unconditionally while the profile row can read "tax-exempt — no tax consideration applied".
  Unreachable under the fixed preset (`tax_sensitive: true`).

### Job spine

- **M21 — `account_total` would double-count a bank-sweep balance reported as both a position and `cashBalance` (latent).**
  `src-tauri/src/schwab_live.rs` ~252–256 and ~259–295 keep a `CASH_EQUIVALENT` row's `marketValue` in the position list, and `schwab.rs` ~124 adds `cashBalance` on top; `job.rs` ~2493–2506 divides every weight by that total.
  The dev store's persisted 2026-08-13 run shows 47 position rows, none cash-class, `cashBalance` 82.79, and `account_total − cash − Σ market_value = 0.00`, so this account's payload carries no such row today.
  Recorded as latent: it fires only if the Trader API ever reports the sweep both ways.
- **M22 — The selective tail sweep dates itself on its own clock.**
  `job.rs` ~1258–1274 stamps `fetched_at: now_rfc3339()` and `quick_check.rs` ~751–752 mints its own `now` / `today`, so a run that crosses the 8 PM ET rollover between its pinned instant and the sweep stamps the carried holdings' eval state on the next session.
  Minutes-wide; no finance changes.
- **M23 — A guard-terminal stock still fetches its sector benchmark and can record a benchmark gap.**
  `job.rs` ~1505 sets `skip_retrieval` for a guard-terminal stock, but the benchmark fetch at ~1689–1697 is gated on `is_stock && prior.is_some()` only.
  Budget and telemetry only.

### Research and distillation

- **M24 — The leading indicator's percent render fails on ordinary fractions (IEEE arithmetic).**
  `src-tauri/src/portfolio/distill.rs` ~1452–1454 tries `value_in_text(l.value * 100.0, page)` for a sub-1 value; `0.29 × 100.0` is `28.999999999999996`, which `value_in_text` renders verbatim and never finds, so the indicator (the narrative-cap suppression anchor) is dropped for roughly half of two-decimal fractions.
  The only test uses 0.25, which is exact.
  A conservative failure: the cap stays.
- **M25 — A one-word issuer string can never identify the holding.**
  `distill.rs` ~1556 applies `text_names_holding` (`mod.rs` ~1534–1568) to the forensic claim's `issuer`; the first word is sentence-initial and needs a following capitalized word, and the bare ticker needs `$` or `NYSE:` context, so `issuer: "Tesla"` and `issuer: "TSLA"` both reject as cross-issuer.
  Advisory channel, fail-soft, gap-logged.
- **M26 — The indicator's `as_of` is hard-rejected unless `YYYY-MM-DD`, but the prompt says only "dated".**
  `distill.rs` ~1389 versus the `leading_indicator` prompt line (~1755–1760); a monthly indicator dated `2026-06` drops silently.
  The pre-profit line in the same prompt does say "ISO date".
- **M27 — `seeded_by` is uncapped although the docs name a seed-lineage cap.**
  `src-tauri/src/portfolio/research.rs` ~1350–1356 accepts every known id; `docs/configuration.md` §Research Context Management describes a small default cap.
  Bounded in practice by the seed count; the logic-flow already says the knobs are not built.

### Frontend

- **M28 — A matured scoreboard line can render `-0.0%`.**
  `src/components/PortfolioView.vue:809` and `:811` use `(x * 100).toFixed(1)` directly, bypassing the signed-zero guard `fmtPct` applies elsewhere; a `total_return` of −0.0004 renders "total return -0.0%".

### Doc/code alignment (the logic-flow doc)

- **M29 — Step 4 names a "reversal" tag the diff never emits and says "signed quantities".**
  Logic-flow ~345–350 versus `src-tauri/src/portfolio/mod.rs` ~280–289 (`New` / `Increased` / `Decreased` / `Unchanged`) and `diff.rs` ~110–133 (absolute size on a same-side move, the signed swing on a flip); a flip is tagged Increased or Decreased, and `side_reversed` is a badge on a carried verdict only.
- **M30 — "Forward dividends" is the trailing-TTM dividends-per-share print.**
  Logic-flow ~712–719 names a forward-dividend input; `engine.rs` `scenario_targets_v2` and `fund.rs` `analyze_fund` read `ttm_dividends_per_share` (zero when absent), which the canonical doc names as the proxy.
- **M31 — The fund dispersion floor binds on every path, not only the carry.**
  Logic-flow ~807 says the floor applies "on the carry path"; `fund.rs` ~926–927 passes `dispersion_floor(vol)` unconditionally and `scenarios_from_surfaces` applies it after the rate-anchored and raw-percentile multiples too.
- **M32 — "action … carried but unscored" reads broader than the engine stand-in it means.**
  Logic-flow ~914; `outcome.rs` `derive_reads` scores the verdict's action as return cohorts; only the engine stand-in's rung has no reader.
- **M33 — The per-topic seed's ledger conditions are neither per-topic nor cache-gated.**
  Logic-flow ~945 and ~1012 say each topic receives "that topic's ledger conditions" and a topic with no cached prior "starts clean"; `research.rs` ~339–393 renders every condition of the whole ledger into every topic's seed, and returns a seed whenever the ledger has conditions, cache or not.
- **M34 — The observation dedup key is understated.**
  Logic-flow ~1170 names "source, publication date, and value"; `pre_profit.rs` ~292–316 keys on metric identity, role, period, source, publication date, and value.

## Checked and held

Recorded so a later reader knows what was examined, not only what was found.

- Engine: `scale` at both ends of every band, negative P/E → 20, negative D/E → 0 in the risk score and High in the tier, population daily σ with √252 applied to daily data only, TTM numerator and denominator on one basis, `quarters_contiguous` across year boundaries, `canonicalize_statements` keeping the latest filing, `resolve_series` identities and comparators, the streak machine and the first-evaluation-adopts gate, the grade weights and cutoffs, `percentile`, the anchor observations pairing each window's TTM with its own close and DGS10, the sanity bound, the inverse spread mapping and the raw fallbacks, the dispersion floor, the driver ladder and the trough release, `hurdle_read`, `feasible_actions`, `engine_outlook`, `engine_conviction`, `engine_action`, `narrative_vs_reality`, `implied_expectations`, `refine_targets_with_assumption` re-running the same `analyze`, `max_drawdown`, `split_bridge_factor`, `tech_event_pre_flag`, `options_signal`.
- Fund: `composite_yield` and its history, `blend_sector_pes`, `nav_premium_read` and its CEF gating, `classify` routing, `assign_fund_tier`, the flat-driver target form, the COT contract map, `resolve_listing`.
- Pre-profit: burn, runway, dilution, and margin arithmetic with their units; eligibility precedence; the financing bands; the excerpt-corroboration chain's byte safety on non-ASCII pages; `select_bound` / `select_actual` under the vintage policy; the miss thresholds; severe deterioration; `clamp_conviction`.
- Outcome: the Winkler interval score, `window_end`, the entry and bridge sessions, price-only and total-return definitions, the benchmark legs, the alignment table, `episode_decision` / `plan_episodes` beyond N4, `derive_reads`, the SPDR map, the store's episode upsert and prune.
- Quick check: every bridge direction, the deadband, the band relation, the D/E withholding, the filing sweep, the fund legs, the hurdle flag's condition, the fail-soft posture (no `portfolio_runs` write), per-holding isolation.
- Dossier and adapters: FRED percent-to-decimal once, the rate wiring, the TTM adoption and the SEC annual fallback, `merge_financials`, SEC concept and period selection, `forensic_events_from_filings`, statement dating, the NTM blend, `ttm_dividends_from_value`, the fund parsers, `usable_price` at every quote and EOD parse, FINRA and CBOE parsing.
- Pipeline: the 6b–6g order, `engine_output` immutable past 6b, the shadow refinement never splicing, the profile reaching only the action call, the feasible-set annotation, the role-risk branch's skips, the error posture per call, exactly one retry layer, `ensure_not_output_limited`, `decode_interpretation`, `distill_route`, the request modes per stage.
- 6g: `parse_quant_core`, `validate_condition`, the bounded supersession search, carry and close semantics, `ContinuityStamps` at authoring, tripped and fired gating on the app's own crossings, `validate_what_changed`.
- Prompts: every fraction ×100 exactly once, IV and skew units, the NAV premium sign, `implied_moves_section`, the schemas against `Interpretation`, `LedgerDraft`, `WhatChangedEntry`, `RoleRiskInterpretation`, and `ActionDecision`, `validate_model_arm` rejecting rather than clamping.
- Job: every unwrap and lock in the run path, panic containment at the job seam, the checkpoint write order and `UNIQUE (run_id, symbol)`, `resume_eligibility`'s stamp set and window, the selective work-list and the over-age demotion, `build_roll_up` given a clean total, the persist transaction and `prune_runs` by id, cancellation.
- Research and distillation: loop termination, the budgets, `parse_tool_calls`, byte slicing on page text, the SSRF guard, the search fallback, the registry, the sub-distillation cap, `validate_combined`, the schemas against the wire structs, the prompts' financial statements.
- Store, types, and the seam: every persisted struct's writer and readers, `init_schema` against every query, id-primary ordering, `diff_holdings`, `text_names_holding`'s byte safety, `DeadlinePolicy` arithmetic, the retry whitelist, `RetryOnce`, `stream_chat_response`, `length_stop_reading`, the gates.
- Frontend: every `types.ts` union against the Rust `rename_all` spellings, every `Option` guarded, every formatter's unit and sign, the sort comparators, `carriedStamp` mirroring `OVER_AGE_DAYS`.

## Open questions (not findings)

Each is a question the reading could not settle; none is asserted as a defect.

- Possessive issuer names: `listing.rs` `significant_tokens` splits on the apostrophe and drops the one-character remainder, so FMP's "McDonald's Corporation" yields {MCDONALD}.
  If Schwab's position description reads "MCDONALDS CORP", the two share no token and the guard returns `Conflict` → `insufficient-evidence` on every run for MCD, KSS, MCO, and WEN.
  A live pull's description settles it.
- Is `NUM_PREDICT_DISTILL` = 8,192 enough for an overflow-eligible stock's 6d reduce (the combined object, the per-topic claims with URLs, the typed channels, and verbatim excerpts)?
  A length stop there fails the run hard.
- Statement freshness is not floored: a delinquent filer's old prints combine with today's market cap under only the audit's basis label.
  The evidence floor's stale arm is documented as price-only and designed-not-built; is the statement side intentionally uncovered?
- An allocation or multi-asset fund whose `assetClass` matches none of the class words but whose `sector-weightings` serve rows routes `Equity` and prices; if FMP normalizes a 60/40 fund's sector weights to the equity sleeve, it prices as pure equity.
- FINRA lookup is by the exact upper-cased Schwab symbol; class-share spellings (`BRK/B`) may miss systematically rather than as a market fact.
- The document cache stores under the normalized final URL and looks up by the normalized requested URL, so a redirecting seed URL never hits and every re-read spends a live fetch.
- M11's NTM-versus-NTM comparison reaches the narrative read too; is the rolling-NTM basis the intended convention for both, or should the revision leg compare fixed-period rows?
- A pre-open run, or a holding analyzed after a resume, widens the authoring-quote-versus-anchor-close residual beyond the "intraday" the scoreboard's bridge describes.

## Disposition

Awaiting the user's rulings.
This review changed no code and no doc other than adding this record.

## Phase 2 — cross-check against the 2026-08-24 record

Method: after Phase 1 was written to disk, `docs/verification/2026-08-24-portfolio-analysis-large-scale-review.md` was read for the first time in this review — §Verdict, §Scope and method, §F1, §F2, §C1, the retry-posture section, §Disposition, §I2, §I3, §I4, §I9, §I14, §I19, §What was verified correct, plus keyword sweeps of the rest — and `git log -S` located the commit that introduced each non-minor finding's code site.
Phase 1's findings were not revised on what the old record says.
For each non-minor finding: (a) does it match a finding from the last review, and if so is it genuinely still broken; (b) did one of the last review's fixes introduce it.

### N1 — the period-span collision

- (a) No match.
  The nearest findings are I3 (source corroboration), I4 (the guidance vintage policy), and I19 (the one-fact ambiguity); none names a span collision.
  I4's ruling records "the FY-normalizes-to-12-31 residual" as the period-end leg being loose for a fiscal year not ending in December — a chronology looseness, not the pairing of a full-year bound against a quarterly actual.
- (b) Not introduced by a fix.
  `normalize_period` landed 2026-08-24 in the research-loop slice (`457efe1`), the commit the last review was conducted at, and the pairing keyed on identity plus period before I4 too.
  The I4 fix (2026-08-28) built the ex-ante chronology on the collapsed key and named the 12-31 residual beside it without seeing the collision, so this is a miss adjacent to a fix rather than a regression.

### N2 — the streaming deadline is a total cap

- (a) Matches C1 in class.
  C1 was "the 600-second transport deadline kills legitimately long thinking chains, and the run with them"; N2 is the same kill on the streaming path at ~22 minutes.
  It is a genuine repeat on that path.
  C1's resolution (2026-08-26) records that "the blocking client never forwards its timeout to the async client … the streaming callers — interpretation, role-risk, and the action call — were therefore already idle-bounded and never at risk mid-chain", and that "the streaming stages' idle bound rises the same way, from ten minutes to the prefill term (~22 min at the interpret context)".
  Both sentences describe `ClientBuilder::timeout`, which the pre-fix code used; the fix replaced it with the per-request `RequestBuilder::timeout`, which reqwest forwards as the async `TotalTimeout` and applies through `TotalTimeoutBody`.
- (b) Introduced by the C1 fix (`64ef432`, 2026-08-26): the diff adds `.timeout(deadline)` on both call sites where only the builder default existed before.
  Before the fix a streaming call's body was idle-bounded at 600 s per read, so a healthy stream ran for as long as the daemon kept producing; after it the same call carries an absolute ~22-minute cap.
  The fix is right for the non-streaming callers (a total budget is what they need) and regressed the streaming callers from idle to total semantics while recording the opposite.
  The slice's three Codex rounds did not catch it, and the deadline tests exercise silence only, where the two semantics coincide.
- Not converging: a fix that created a new instance of the bug it fixed.

### N3 — the widened sweep window

- (a) No direct match.
  The old record's §What was verified correct names "price-window parity across paths" as sound; N3 is that parity broken on the sweep.
- (b) Introduced by the F1 fix (`21e7a19`, 2026-08-27).
  The diff replaces the sweep's fixed `QUICK_EOD_LOOKBACK_DAYS` fetch with `eod_lookback_for(boundary, today)` so "the sweep's per-holding EOD lookback widens past the 180-day floor to keep an old vintage's anchor reachable", and the widened series is handed whole to `price_history`.
  Before F1 the two paths agreed by construction.
  F1's four Codex rounds hardened the anchor's carry and the withholding semantics; none looked at what else the widened fetch fed.
- Not converging on that seam, though the reach is narrow (a boundary older than ~166 days).

### N4 — the crossing attaches to the newly opened episode

- (a) No match.
  The old record verified "lead-time signs" and "the episode lifecycle" as sound and left open only "the falsifier lead-time bar-count distortion over mid-window interior gaps" (§F2); the attachment rule is not mentioned.
- (b) Not introduced by a fix.
  The `rfind` on the latest active episode and its comment date from the outcome-learning slice (`8648164`, 2026-08-04); the `confirmed_at` sourcing from the fresh-start slice (`525a853`, 2026-08-17).
  Pre-existing; the last review's outcome pass did not reach it.

### N5 — the one-exchange snapshot accepted without a gap

- (a) No match.
  I2, I9, and I14 worked the same adapter and sampler (the in-quarter admission, the exchange-identity and plausible-band guards, the canonical dates) without noting the partial accept.
- (b) Not introduced, but a fix widened its reach.
  The partial-accept comment dates from 2026-08-07 (`99b4f61`) and the exchange loop from 2026-07-16 (`ac9434f`).
  I9's fix (2026-08-29) makes `sector_pe_rows_from_value` return `Err` naming the served board when any row's `exchange` is absent or disagrees, so a body one board serves with a missing or mismatched `exchange` field now reads malformed, drops into `last_err`, and the other board's rows return alone as a usable snapshot with no fund-side gap.
  Since I9 the partial-accept path is reachable on a data-shape fault as well as a transport fault.

### Reading

- Two of the five non-minor findings were introduced by the last review's fixes (N2 by C1, N3 by F1), one had its reach widened by a fix (N5 by I9), and two are pre-existing misses in areas the last review recorded as verified (N1 beside I4; N4 under "lead-time signs" and "the episode lifecycle").
- The common thread on N2 and N3: each fix's review rounds hardened the fix's own new machinery and did not re-test the property the surrounding code had relied on — idle semantics on the stream, window parity on the sweep.

## Implementation closure — appended 2026-08-30

The user approved handling the whole record before the confirmation run.
This appendix is additive: the blind review, its Phase-2 reconciliation, and the original awaiting-rulings disposition above remain unchanged as historical evidence.

### Non-minor findings

- **N1** closed in `fdb6273`: observations now carry and validate an explicit reporting span, and pairing, backfill, deduplication, prompts, schemas, persistence, and compatibility all distinguish Q / H / FY / YTD durations that share a period end.
- **N2** closed in `362d35b`: streaming requests retain a true per-read idle timeout without an absolute body deadline, while non-streaming requests keep their total deadline; active-stream and stalled-stream tests pin the distinction.
- **N3** closed in `ff91c0f`: the quick check keeps the widened dated series for split-bridge lookup but slices the engine's trailing-return and return-volatility inputs back to the production short window.
- **N4** closed in `17b4a35`: a crossing is assigned against the episode that carried the condition before a changed-action episode opens, and lead-time attribution is pinned by coincident-crossing tests.
- **N5** closed in `edea770`: a sector-P/E surface is usable only when every required exchange leg succeeds, with the same completeness rule and typed gap propagation on snapshot and history paths.

### Minor findings

- **M1–M2** closed in `0d11faa`: assumption units scale cents, reject named foreign currencies without FX, and carry comments that match the empty-unit rejection.
- **M3** closed in `1535bfa`: the active docs describe the actual stock floor — usable price, two real sub-scores, resolved identity, and an admissible driver — rather than a separate all-statements presence gate.
- **M4–M5** closed in `2284c6d`: option-overlay and leveraged/inverse structure render distinctly, and ultra-short fixed-income names no longer false-match the leveraged classifier.
- **M6** closed in `edea770`: memoized sector-history failures propagate their typed gap to every affected fund, not only the first consumer.
- **M7–M8** closed in `fdb6273`: large integral corroboration needles remain whole, and en-dash negatives are recognized beside ASCII and mathematical minus signs.
- **M9–M10** closed in `17b4a35`: an episode's hurdle and DGS2 share the same intrinsic calibration snapshot, and confirmations after the window do not clamp onto its last bar.
- **M11** closed in `2e1b176`: revision and narrative comparisons hold the prior fiscal-period weights fixed, preventing calendar roll from masquerading as estimate revision.
- **M12–M13 and M22** closed in `ff91c0f`: the hurdle flag names its deciding bull leg, retrieval failures are not mislabeled as split-bridge failures, and selective sweeps use the run's pinned clock.
- **M14 and M23** closed in `646199a`: commodity context routes by industry-aware exposure and guard-terminal holdings skip the irrelevant benchmark retrieval.
- **M15–M16** closed in `cfc130e`: audits preserve issued model order and the actually routed distillation model, while the distillation reservation rationale and overflow behavior match the current wide schema.
- **M17–M20** closed in `7db908b`: delta renders preserve meaningful precision, the comment attaches to its function, carried rule demotions are not presented as model-authored, and tax language respects the profile posture.
- **M21** closed in `859a607`: Schwab cash-equivalent rows reconcile against cash and liquidation totals without double counting; ambiguous or irreconcilable shapes fail instead of guessing, raw source rows remain auditable, and checkpoint compatibility moved to v7.
- **M24–M27** closed in `e644005`: percentage corroboration avoids floating-point text drift, one-word issuers can identify a holding, month-precision indicator dates are accepted, and seed lineage is capped.
- **M28** closed in `5e95f2e`: matured total-return and price-only lines use the shared signed-zero-safe percent formatter, with component tests for both paths.
- **M29–M34** closed in `1535bfa`: the logic flow now matches the diff tags, trailing-TTM payout proxy, all-path fund dispersion floor, model-action scoring, holding-wide condition seed, and full observation dedup key.

### Open-question rulings

- **Q1 — possessive issuer names:** closed in `17568e7`; issuer-token normalization treats possessive and unpunctuated names equivalently without weakening the distinctive-token guard.
- **Q2 — distillation reservation:** closed in `cfc130e`; 8,192 is the normal output reservation, not a prompt reservation or a total-call-token cap.
  An exact reservation-bound length stop receives one final 32,768-token attempt on the 128 K reasoner; using 32,768 on every call was rejected because it removes the normal runaway / latency guard and cannot always coexist with the rendered prompt on the fast tier's context.
- **Q3 — statement freshness:** ruled and documented in `1535bfa` as an accepted boundary for the confirmation run.
  Statement age alone does not abstain until the product persists a typed statement vintage and has a calibrated cadence threshold; the existing concrete sub-score and driver gates remain binding.
- **Q4 — allocation / multi-asset fallback:** closed in `2284c6d`; explicit allocation and multi-asset labels route to `role_risk_only` instead of being priced as pure equity merely because sector weights served.
- **Q5 — FINRA class shares:** closed in `b5732ed`; the FINRA lookup key follows its uppercase separator-free issue-symbol convention while the holding's original identity remains unchanged.
- **Q6 — redirect cache keys:** closed in `4b1f3b2`; requested and final normalized URLs resolve to the same cached document, including migration of the prior final-URL-only rows, without weakening redirect-hop host validation.
- **Q7 — revision convention:** closed in `2e1b176`; fixed fiscal-period rows are the intended revision basis for both the badge and narrative read, while rolling NTM remains the target-driver basis.
- **Q8 — quote-to-close bridge:** ruled and documented in `1535bfa` as a decision-session quote-to-close residual.
  It is intraday on an ordinary after-open pass, can cross the prior close pre-open, and can widen on resume when a fresh quote is compared with the pinned run-session close.

### Superseding disposition

All five non-minor findings, all thirty-four minor findings, and all eight open questions are closed by implementation or an explicit documented ruling.
Every implementation slice was committed and pushed to `main` only after the project-required Rust tests, warning-free clippy, frontend production build, applicable frontend tests, and `git diff --check` passed.
No confirmation run was executed as part of these fixes; this record is now ready for that run.
