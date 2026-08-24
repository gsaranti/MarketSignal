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

### F2 — major: outcome-label scoring bounds the series tail, not the scored bar

`covers_through` (`outcome.rs:862-867`) checks only that the **series' latest bar** reaches the window end within `COVERAGE_TOLERANCE_DAYS`, but the bar actually scored is `close_at_or_before(closes, w_end)` (`outcome.rs:1103`), which carries no staleness bound.
Over a series with an internal gap, a window's end bar can be arbitrarily stale — in the degenerate case the entry bar itself, recording a fabricated exact-0% return for a window in which no post-entry price was observed (`outcome.rs:1131`).
`bench_return` has the identical shape on the benchmark legs, and the falsifier lead-time stamp is distorted to bar counts over the same sparse series.
These labels enter the cohort means, band calibration, the head-to-head, and the outlook hit-rates — the exact surfaces the calibration tier will tune against.
Reachability is gated: the production cache writer fetches floor→today contiguously, so it needs a source-side range clamp or a stale cache surviving a failed refresh — but when it fires it is silent.
The fix-shaped invariant: bound `end_bar.date` to `w_end − COVERAGE_TOLERANCE_DAYS` and require `end_bar.date > entry.date`.

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

### Priority-1 minor findings

- **Loss-forecast displacement in the shadow fill** — `feed_present` requires `v > 0.0` (`engine.rs:2043`), so a published negative EPS consensus counts as absent and a whitelisted research fact overwrites all three consensus legs (`engine.rs:2082-2089`), against the supplement-never-displaces contract.
  Confined to the shadow-only 6e audit line — but that line is the promotion evidence the 2026-08-24 shadow ruling reads, so pollution there matters.
- **TTM seam gap in the narrative fallback** — `ttm_revenue_window` checks contiguity only inside each 4-quarter window (`engine.rs:3103-3109`), never across the seam between the current and prior-year windows (`engine.rs:3156-3162`), so a feed gap at the seam yields a mislabeled "YoY" over 15 months; the dossier's own `apply_ttm_statement_basis` demands the full 8-row run for exactly this pair (`dossier.rs:587-598`).
- **Historical anchor share-count fallback** — a historical revenue-per-share anchor whose quarter lacks `diluted_shares` falls back to the newest quarter's or today's count (`engine.rs:2210-2213`), skewing the anchor-multiple history in the financially wrong direction under buybacks or dilution.
- **One-month band is unscaled daily volatility** — `(daily σ × 2).clamp(0.02, 0.15)` (`engine.rs:2562-2565`) understates a month's 1σ move by ~√21 against the suite's own √t convention (`dispersion_floor`, `tech_event_pre_flag`), so the printed band covers ~0.44σ of the month.
  The doc comment marks it "v1 mechanics", so this is surfaced as a deliberate-retention judgment call rather than an accident.
- **Tech pre-flag benchmark coverage unchecked** — `latest_on_or_before(benchmark_closes, latest.date)` (`engine.rs:3260`) never verifies the benchmark covers the holding's newest session, so a shorter benchmark series silently mismatches the windows instead of taking a typed gap; rare, since both legs ride one FMP fetch.
- **Pre-profit backfill counts any-role periods** — `backfill_required` counts distinct stored periods of any observation role (`pre_profit.rs:236-251`) where the documented rule counts *comparable* periods (bound + actual pairs), so a metric with four guidance rows and zero actuals suppresses the mandated backfill on later passes — blinding miss-detection exactly where guidance is open.
- **Fund momentum band saturates** — the fund path scores `trailing_return` over the ~1,600-day deep history when present (`fund.rs:1027-1036`) against the stock path's ±30% band tuned to a 180-day window (`fund.rs:823`), so nearly every fund pins at 0 or 100; momentum sits outside the letter, so the damage is context quality in the prompt and the frozen `CalibrationSnapshot`.
- **Expense ratios flattened by `{:.3}` rendering** — the interpretation and action prompts render expense ratio and expense drag through `opt()`'s three-decimal format (`pipeline.rs:2634`, `3236`, via `pipeline.rs:3477-3479`), so a 0.03% fund prints `0.000` — which the prompt's own legend ("0.0075 = 0.75%/yr") teaches the model to read as free — and the legend's own example is unrepresentable.
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

### Named design risk — zero retry on hundreds of hard-path model calls (documented posture, not a defect)

Any single local-model failure inside the required 6c–6f path — a transient daemon error, a connection reset, an empty completion body (`serde_json::from_str("")` at `pipeline.rs:4779/4797/4821`, `research.rs:1120`), a schema-parse failure (`distill.rs:633`, `711`, `719`), a length stop, or a whitespace action rationale (`ensure_action_rationale`, `pipeline.rs:150-160`, ruled fail-hard 2026-08-18) — fails the whole run on first occurrence.
This is the documented hard posture (`docs/portfolio-analysis.md` §Failure posture), typed, checkpointed, and resumable, and both robustness reviews independently confirmed no bounded retry exists anywhere on the path (retry-once is the known repo-wide deferred item).
It is recorded here because at ~4+ reasoner calls per holding across dozens of holdings over hours, it is the dominant real-world abort probability for the big confirmation run, and C1 multiplies it.

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

### A2 — major: the interpretation call's "exact inputs" list omits whole rendered sections

Doc lines 1185–1245 enumerate the interpretation prompt's inputs, but the code renders, into the same call: the forensic filings state with the hard-rule text (`pipeline.rs:3381`, `3153-3190`), the CBOE venue-level put/call backdrop (`3373`, `2915-2930`), the commodity context (`3382`, `3118-3145`), the technology-event pre-flag section (`3417-3430`), the semantic prior-analysis recall block (`3457`, `2135-2147`), and — in the fund context the doc reduces to "expense ratio, US share, and composite P/E coverage" — the CFTC COT positioning block and the closed-end price-vs-NAV line (`3266`, `2885-2909`, `3257-3265`).
Every input the doc does list is present, and both of its claimed absences hold: the investor profile is not rendered (`pipeline.rs:3405-3408`) and no engine stand-in conviction/outlook/action appears.

### A3 — major: the action call's "exact inputs" list has the same shape

Doc lines 1286–1293 end at the engine action set and the profile, but the action prompt also renders the forensic filings section and the commodity context (`pipeline.rs:3689-3690`), plus the CEF NAV-premium line on the role-risk digest (`3675-3679`).
Everything the doc does enumerate was verified present, including the withheld engine pick and the profile without the cash row.

### A4 — major: the sub-distillation drop trigger is misdescribed

Doc line 1092 says passes drop "if even the tree-level reduce would overflow".
No sizing check on the reduce exists; the actual trigger is the per-holding budget `SUB_DISTILLATION_CAP = 4` pass-level sub-distillation calls shared across overflowing topics (`distill.rs:55`, `651-669`), and since `MAX_PASSES_PER_TOPIC = 3` a single overflowing topic can never hit it alone — passes drop only when a second overflowing topic finds the budget partly spent.
The surrounding claims (drops take findings and ledger entries, never the prior) match the code.

### Priority-3 minor findings

- **Research-fed fraud listed under "Hard forensic state (live)"** — doc lines 863–865 place "fraud may arrive later from validated primary-source research" inside the hard-state bullet, but the 2026-08-24 ruling made the research-fed claim advisory-only — it never merges into the hard producer state (`pipeline.rs:351-365`), which gates the add family.
- **"A qualifying news seed" reads as an independent tech-topic trigger** — doc line 978; the code requires the conjunction with a standing technology-class ledger falsifier (`pipeline.rs:1002-1003`; same conjunction in the quick check, `quick_check.rs:82-84`), so fresh news alone never fires the deep-dive.
- **The narrative read's 7-day minimum is undocumented** — the doc names only the debut as carrying no read (lines 578, 852–857), but `NARRATIVE_MIN_ELAPSED_DAYS = 7` (`engine.rs:3128`) gaps the read for anyone running more than once a week.
- **"(pre-profit stocks only)" vs computed-for-every-stock** — the Step-6b order list's parenthetical (line 569) contradicts the doc's own lines 596/651 and the code (`pipeline.rs:863-873`): the overlay record is computed and persisted for every priced stock.
- **"One isolated conversation per agenda topic"** — doc lines 1000–1001; as-built each *pass* is its own fresh conversation (`research.rs:1067-1090`), with only the claims ledger and findings carrying across a topic's passes — which the doc's own "who owns the context" bullets state correctly.
- **"The thresholds are config knobs"** — doc line 1076; `OVERFLOW_THRESHOLD`, `CHARS_PER_TOKEN` (`distill.rs:48-51`) and `NUM_CTX_DISTILL` (`pipeline.rs:4535`) are compile-time constants exposed in no settings surface.
- **Supersede validation legs overstated** — doc line 1155 claims metric/units/period match legs and that "a supersede always rejects"; the code has no match legs, and with an absent feed value a supersede-declared claim does not reject — it falls through and fills exactly like a supplement (`engine.rs:2039-2057`; the F-minor loss-displacement finding rides the same guard).
- **"An accepted forward assumption" as what-changed evidence** — doc line 1327; the delta row is pushed whenever distillation validated an assumption, regardless of its Step-6e shadow resolution — a `rejected:` resolution still anchors an external what-changed row (`pipeline.rs:1222-1230`).

## What was verified correct

Coverage matters as much as findings for a pre-run record; the following were traced and found sound.

- **Engine core** — statement canonicalization (restatement dedup keeps the latest filing), TTM all-or-nothing sums, every sub-score map with both off-scale guards (negative P/E never "cheap", negative D/E never "clean"), grade weights/cutoffs/imputation, the anchor join with filing-date (+45d grace) alignment, the inverse spread→scenario map and its degenerate/monotonicity/dispersion guards, the driver ladder's admissibility and clamps, the v4 trough-release gates, implied expectations round-tripping the shared multiple derivation, re-anchor arithmetic identical to the live pass, risk tiers, the hurdle read's comparison directions, `narrative_vs_reality`'s two forms, the √t-scaled tech pre-flag, options-signal composition, and the full ledger evaluation surface (comparator directions, margins, observation identities, streaks, ack re-raise, basis-flip gate).
- **Fund / pre-profit / outcome** — CEF premium sign and gap-honesty, classification routing, the exchange-blend and constant-mix history arithmetic, the fund-form v2 targets, runway/burn sign conventions, dilution and margin-trajectory reads, `clamp_conviction` as a pure ceiling, observation validation's corroboration rules, the Winkler interval scorer, split-safe return bridging, dividend windowing, lead-time signs, no-lookahead window gating, cohort composition, and the episode lifecycle.
- **Dossier / quick check / diff** — TTM basis adoption and the choke-point's coverage, flow-vs-instant discipline, price-window parity across paths, quick-vs-deep formula parity (no fork), tripwire and band boundary semantics, holdings netting signs (including short semantics and the cash routing), the OCC option-overlay decode, and short-interest passthrough.
- **Prompts** — the two-arm framing, sub-score directions, hurdle/tier renders, target provenance, the retrospective block's split-safe math, the what-changed instruction matching its validator exactly, the pre-profit overlay units, the forensic sections matching the 2026-08-24 rulings, tunnel-vision discipline in the action prompt (no book inputs, engine pick withheld), profile isolation from the intrinsic call, and the 6c prompt suite's seed/citation framing.
- **Robustness** — run-slot release on success/error/panic, the checkpoint/resume cycle (atomic per-holding writes, no skip or double-process, version-stamp refusals, the newer-run eligibility check), cooperative cancellation at every boundary, single-transaction persistence with WAL + busy timeout, fail-soft routing of every enriching feed, web search/fetch failures degrading to tool notes, the SSRF guard (pinned DNS, full special-range table, body caps, re-checked cache reads), guaranteed budget-loop termination against an injectable clock, multi-byte-safe text handling, grammar-constrained local-model decoding with lenient wire structs, and the panic-freedom of the spine files' own unwrap/index/arithmetic sites.
- **Alignment** — every constant the logic-flow doc states matches the code (grade weights and cutoffs, band parameters, tier thresholds, research budgets and freshness windows, retention caps, the resume window, outcome windows and grace), and all twelve "most important safety rules" hold, including the four 2026-08-24-adjacent ones (model arm never binds the baseline, shadow-only forward assumption, advisory fraud, driver-id-gated indicator anchor) and the code-enforced no-order Schwab boundary.

## Disposition

Every fix is a separate, undecided piece of work; nothing here was applied.
Four items bear directly on the queued big confirmation run and are worth deciding before it: C1 (a realistic multi-hour-run killer whose fix shape is a transport budget consistent with the thinking reservation, or an idle-based read timeout), the retry-posture weighing beside it, F1 (a split during the watch window would contaminate the run's ledger evidence), and F3 (the run will exercise the research loop whose ledger channel is silently dead, and its watch-set typed-channel yield line will read zero qualitative support by construction).
The alignment findings are doc edits; A1 in particular misinforms the user about the cost of the failure mode C1 makes likelier.

## Codex independent review additions

### Method and de-duplication

This review was completed independently against current `main` `4dc675b` before the Claude Code section above was opened.
The only commits after Claude Code's reviewed `457efe1` are Metis / watch-set documentation commits; the production portfolio code cited below is unchanged between those revisions.
After the independent findings were fixed, they were compared finding-by-finding against Claude Code's report and every duplicate was removed.
The fund-weight NaN finding below is intentionally retained despite touching Claude Code's generic NaN-panic note because it identifies the concrete production adapter input, the false-valid coverage transition, and the exact whole-job panic path that the earlier note did not establish.

The full repository gates were green on `4dc675b`: `cargo test` (1,185 passed, 31 ignored, plus all integration suites), `cargo clippy --all-targets --all-features`, `npm run build`, `npm test` (46 Node tests and 247 Vitest tests), and `git diff --check`.
The green gates do not cover the adversarial boundaries below.

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
