# Pre-big-run review piece 3 — deterministic value-chain correctness walk (2026-08-05)

The third of the three pre-big-run review pieces
(piece 1 = the first-live-run findings verification, [2026-07-31-first-live-portfolio-run.md](2026-07-31-first-live-portfolio-run.md);
piece 2 = the code-vs-docs conformance walk, [2026-08-04-piece2-conformance-walk.md](2026-08-04-piece2-conformance-walk.md), re-run [2026-08-05-piece2-conformance-rerun.md](2026-08-05-piece2-conformance-rerun.md)):
a correctness walk of the Portfolio Analysis job's deterministic value chain — the math, units, sign conventions, period and date alignment, basis crossings, window bounds, boundary conditions, and stage-to-stage handoffs — judged against the evident finance intent, not against the docs
(doc conformance was the twice-run piece 2; here a doc-code wording mismatch alone was out of scope).

## Method

Eight parallel correctness passes sliced by value-chain segment rather than by doc scope —
(P1) adapters / wire→normalized parse, (P2) dossier assembly + listing guard + fund path + netting/diff, (P3) engine core math, (P4) thesis ledger + 6g + quick check, (P5) pre-profit + construction 7a/7b, (P6) outcome learning + two-arm scoreboard, (P7) orchestration / selective carry / persistence, (P8) frontend deterministic value handling —
with the known designed-not-built and deliberately-dormant items and all prior ruled dispositions excluded up front,
every finding required to carry a concrete inputs→wrong-output failure scenario with reachability traced,
and each pass additionally reporting a conformed-coverage list and unresolved cross-stage handoff leads.
Every surviving finding was re-verified against the code by the orchestrating session before batching (25+ load-bearing site reads; no finding was refuted in verification);
the batch was then user-ruled in-session across three rounds — eleven rulings, each to the recommended disposition.

The bulk of the chain **conformed**:
grade-v2 sub-score formulas and cutoffs, the driver ladder and one-share-basis discipline, the inverse spread mapping with its raw-percentile fallback, the dispersion floor and clamp-collapse recording, the closed-form re-anchor, the three-state hurdle, the Winkler interval score and the anchor-close bridge algebra for vintage-fresh episodes, the 6g identity-carry machinery's order-independence, the feasibility solve's epsilon scoping, statement canonicalization and the pre-profit local-sort agreement, holdings netting and the diff classifier, serde round-trips (`float_roundtrip` pinned) and the portability entry sets, and the two-arm card pairing and divergence tags.

## Dispositions

**33 raw findings → 30 after deduplication** —
three independent cross-pass convergences (the 366-day dividend window P1+P3, the entry-session ex-date dividend P1+P6, the case-sensitive sweep join P4+P7) — plus eight probe/watch leads and four design notes.
Ruled into: a **25-item fix batch** applied this session, a **ruled follow-up slice** (the ET-dating / outcome-hardening slice, its own session), **4 accept-with-note** items, and the recorded leads.

### The fix batch (this session)

The 18 clear repairs, across 17 bullets — the house-view extractor bullet carries two distinct defects (the truncate panic and the level contract):

- **house-view extraction** — `extract_house_view_sections` panicked when byte 6,000 of the cap landed inside a multi-byte character (`String::truncate` after whole-line pushes overshoot); the panic unwound past every `run_finished` emitter, stranding the tracker with no Failed row.
  Fixed to char-boundary truncation, and the section walk now honors its own level contract (a `###` sub-heading inside a kept `##` section no longer ends capture; a `#` heading now does).
- **TTM dividend window** — inclusive on both ends (366 days), so a payment dated exactly `today − 365` rode beside today's: a fifth quarterly (or second annual) payment inflated the TR payout leg with hurdle-flip risk, and the wrong sum persisted on `QuickCheckBasis`.
  Fixed to an exclusive lower bound; found independently by two passes.
- **outcome entry-session dividend** — a dividend ex-dated on the entry session itself was summed into the total-return label though the entry close is already ex; the window is now `(entry, end]`.
  Found independently by two passes.
- **market-benchmark fallback identity** — `"^spx"` (Stooq identity) was passed verbatim to the FMP dated-EOD fallback rung, whose S&P identity is `^GSPC` — with Stooq throttled the vs-market leg had no working fallback and died "never covered" past grace.
  The FMP rung now maps the benchmark to its FMP identity.
- **post-commit retention failure** — the quick-check retention step ran after the run transaction committed with `?` propagation, so a failure there recorded the run Failed after it durably persisted (a "failed" run becoming the next diff baseline).
  The step is now fail-soft, matching the run-start read's posture.
- **carried-audit crossing re-attach** — a carried audit's prior-run confirmed falsifier crossings re-entered the attach loop and could land on a newly opened episode (empty event list defeating the per-episode dedup) as a fresh confirmation dated this run.
  The attach loop now skips carried audits — their crossings attached in their own run.
- **`cash-freed` on lean-less rows** — structurally unsupportable (`unwrap_or(false)`) on role-risk and carried rows, so a truthful attribution on a role-risk add (reachable since v7) was rejected as a violation with run-failure risk.
  The check now falls back to the prior-action baseline when no lean exists.
- **role-risk demotion tag** — the "Add demoted to hold" tag rendered only on the priced branch; a rule-demoted role-risk card showed a bare Hold (the frontend never followed the piece-2 A2 branch-unscoping).
  The tag now renders on both branches.
- **`weightBand` collapse** — precision keyed on the high endpoint alone rendered a real 1.8–2.2% hold band as "2–2%" and a 0.4–3% range as "0–3%"; the precision now guards against degenerate collapse and false-zero low endpoints.
- **signed-zero renders** — "−0.0%" / "+0.0%" / "−$0.00" artifacts on sub-0.05% fractions; rounded-to-zero values now render unsigned and direction-neutral.
- **FRED rate-anchor finiteness** — `latest_rate_dated` / `rate_history_decimal` accepted `"NaN"`/`"inf"` parses the baseline path explicitly rejects (a NaN anchor would panic the percentile sort); both now filter to finite values.
- **case-sensitive sweep join** — the sweep's prior-state lookup was the one case-sensitive symbol join on the seam; now `eq_ignore_ascii_case` like every neighbor.
  Found independently by two passes.
- **debut-after-abstention open reason** — a priced verdict following a ledger-less debut abstention opened `WeightRangeChange` instead of `Debut`; a ledger-less abstained prior now opens as a debut.
- **construction draft keys** — case-variant duplicate proposal keys could double-count the implied book and external funding; draft keys are now case-folded with a typed violation on a resulting duplicate.
- **`comparable_periods` count** — was truncated to the 4-period miss window despite the field's across-identities contract; now counted pre-truncation (misses stay window-scoped).
- **FRED calendar back-window** — `releases_to_calendar` re-filtered with the fixed 10-day floor, nullifying the cadence-scaled lookback the query requested (released entries 10–45 days back silently dropped).
  The scaled window is now threaded through.
  (Report-chain, not Portfolio — kept in the batch as a real bug.)
- **prune-vs-inserted-run guard** — a backwards wall-clock step could make `prune_runs` evict the run being inserted inside its own transaction; the just-inserted run is now always retained.

The 7 ruled fixes:

- **monotonic observation identity** (ruling 3) — ledger observation identity was `!=`, not monotonic, and the sweep (FMP) and full run (Stooq) key different EOD feeds: a one-day Stooq lag made the full pass acknowledge an older print and the next sweep re-raise the just-consumed breach, and an out-of-order clean print could reset a legitimate streak.
  Date-keyed ids now advance streak and acknowledgment state only on a strictly newer date (a stale print is a full non-event — no advance, no reset, no state regression); the value-keyed expense-ratio id keeps the distinct test.
  One as-built refinement past the ruling: a still-unconsumed confirmed breach re-raising on a stale-print pass keys its crossing to the **recorded** (newer) observation, never the stale id — 6g acks whatever the crossing names, and acking the stale id would recreate the consumed-then-re-raised corruption.
- **acknowledgment cleared on clean reset** (ruling 10) — the value-keyed expense-ratio ack was never cleared when a clean observation reset the streak, so a genuine re-breach at the previously acknowledged value was suppressed until a third value printed; a clean reset now clears the ack.
- **negative-P/E guard made reachable** (ruling 4) — the engine's fixed-20 "a loss-maker is never cheap" score was unreachable live: the dossier derive required a positive denominator, so a loss-maker's P/E was `None`, never negative — a ~25-point valuation-axis escape that could cross a letter cutoff.
  The P/E derive is now signed (net income ≠ 0), and `grade_parameter_version` is bumped so the band-recalibration continuity NOTE attributes the letter shifts.
- **quarter-contiguity check** (ruling 5) — the four-newest-quarters assumption ran unchecked at three sites (dossier TTM basis, engine anchor windows, pre-profit YoY / TTM / margin windows); a one-quarter feed gap silently produced a >12-month "TTM."
  A contiguity check now guards the windows — consecutive period-ends must sit approximately one quarter apart — degrading to the existing typed-gap / annual-fallback paths instead of a wrong multiple.
- **fund-weighting normalization** (ruling 7) — the percent-vs-fraction heuristic (`any element > 1.5`) misread an all-small percent list as fractions, inflating the 7a sector fold ~100× (fabricated overlap clusters) and exposing the ≥70%-US guard input.
  The read is now sum-based, and the fold clamps a per-sector contribution to the fund's own weight.
- **SEC annual-fallback filters** (ruling 8) — the `form == "10-K"` exact match never saw `10-K/A` restatements (defeating the latest-filed-supersedes dedup), and rows were duration-blind (a Q4-duration fact on a 10-K row could win as "annual").
  The form filter is now prefix-matched and duration concepts require an approximately annual span.
- **class-aware 6g executability** (ruling 10) — validation was asset-class-blind, so a series the branch never computes (a statement series on a fund) validated as quantitative and then degraded its sweep family every sweep, force-including the holding on every selective run forever.
  6g now downgrades a series the asset class cannot compute to qualitative at validation.

Plus the small ruled fixes (ruling 10): bare `"short"` added to the inverse-vehicle screen — as-built it is gated on a maturity vocabulary (`term` / `duration` / `bond` / `treasury` / `municipal` / `maturity`) because the fragment screen runs on every fund's name+class blob *before* class routing, so an ungated fragment would have misclassified "Short-Term Bond" duration funds as leveraged/inverse ("ProShares Short S&P500" flags; "Short Duration Municipal" does not);
an exactly-zero netted position routes not-rated (zero net exposure) instead of taking the long-semantics path;
a guard-terminal not-rated stock leaves the 7a sector fold (its weight already rides the not-rated surface — no double presentation);
the analyst-estimates page limit raised 6→10 (the nearest forward year could page out; ordering stays a probe).
And the ruled render suppression (ruling 6): option and fixed-income rows render no cost basis, average cost, or $ / % gain — on the card position block and the pull table alike — Schwab's `averagePrice` multiplier convention is unverified (probe below), and `averagePrice × quantity` is a total only for multiplier-1 instruments, so the fabricated numbers are withheld rather than guessed.

### The ruled follow-up slice — ET dating / outcome hardening (own session, before the big run)

Rulings 1 and 9 settle the design; the build is deliberately its own slice (one code region, one coherent review):

- **ET session dating** (ruling 1) — the per-holding evidence boundary truncates the UTC vintage to a calendar date and compares ET-dated filings with strict `>`: a filing on the pass's own ET day — and the entire next ET day after an evening run — is permanently invisible to sweeps (no badge, no statement re-pull, filing falsifiers starve, a wrong `FreshClear`), and on selective runs the holding is never force-included.
  The outcome entry anchor and basis bridge share the root: an evening-ET run anchors a day late — entry one session late, `anchor_close` from a session traded entirely after the decision, folding that day's move into every interval score and the bear line.
  Ruled: adopt ET session dating via `market_clock` for the vintage boundary, the entry anchor, and the bridge; rule the news-leg's inclusive boundary together with the filing/earnings legs; the frontend stale-tag boundary (fractional days vs date-diff) aligns in the same slice.
  No interim patch — the slice lands next session, before the big run.
- **bear-line bridge keying** (ruling 9) — the bridge was keyed at the episode anchor while `authoring_spot` belongs to the older intrinsic vintage on rule-demotion opens, shearing the material-drawdown line for vintage-stale episodes (band calibration and head-to-head guard `vintage_fresh`; this path didn't — and the retrospective's caller of the same "one home, one bound" contract keys correctly at the prior vintage).
  Ruled: key the bridge at the episode's intrinsic vintage, excluded-not-guessed when that session isn't covered.
- **price-bar fetch range** (ruling 9) — the per-symbol fetch starts at the fetching episode's anchor, so a partial-range merge after a split can leave one series in two adjustment bases and a second episode with an older anchor can score entry pre-split vs end post-split with no fetch to heal it.
  Ruled: fetch from the symbol's earliest active-episode anchor.

### Accepted with a note (ruling 10)

- A same-period restatement cannot advance a filing-cadence streak (the settled period-end identity's consequence; the badge and statement re-pull still fire) — noted in code, waits for a real occurrence.
- The fund valuation percentile counts exact ties as cheap (a degenerate flat history scores 100) — cosmetic.
- The carried-stale-lean stamp misses pre-lean-era carried verdicts — ages out naturally; noted.
- The per-row cash cap in `sizing_from_range` is not joint across adds — inert under the fixed preset (`available_cash: None`); noted beside the documented draw-down deferral.

### Probes and watches for the big run (ruling 11)

- **Schwab `averagePrice` wire convention** for OPTION / FIXED_INCOME rows (sizes the real cost-basis fix behind the render suppression).
- **`^spx` on FMP** (the fallback now maps to `^GSPC`; one probe confirms the mapping's necessity/sufficiency).
- **SEC company-facts sub-annual durations under 10-K rows** (how often the new duration filter bites).
- **Analyst-estimates page ordering under `limit`** (farthest-first assumed; oldest-first would starve the consensus read).
- **FMP dated-EOD in-progress-bar behavior** (an intraday sweep could key a provisional close as a distinct observation).
- **Sector-label taxonomy** across `etf/sector-weightings`, `sector-pe-snapshot`, and stock profiles (an exact-string join; one vocabulary variant splits a sector row or drops composite coverage).
- **Exchange-code strings** on the profile read (folds into the existing B3 listing watch).
- **OCC-root ↔ slash-notation** overlay matching (folds into the existing BRK/B watch).
- **"Short Treasury Bond"-style duration names** now mislabel as leveraged/inverse under the phrase-narrowed screen (the ruled cost — both routes end `role_risk_only`, so the exposure is a wrong class label/reason on the card).

### Design notes (no defect claimed; later rulings)

- Carried audits mix prior-run retrieval outcomes into the run-level data-health counts on selective runs (one stale multiple-carry audit re-trips attention each run).
- Model-arm sub-scores / targets are grammar-unbounded and render unannotated when off-scale (the inverted-band case got a tag; off-scale values did not).
- The sidebar's "rated N" wording reads broader than the priced-only `graded_count` it renders.
- `rate_prints.fetched_at` is stamped with run `created_at` though the FRED fetch precedes the per-holding loop (consumed only by a last-resort fallback).
- The signed P/E derive means a derived negative P/E can now feed a `pe-ratio` ledger condition — not a new behavior class (a wire-served negative P/E always could), but adjacent territory the big run's ledger reads can keep an eye on (internal-review observation).

## Review

Internal review (the Metis task reviewer over the applied working-tree batch): **approve-with-nits** —
per-ruling fidelity checked hunk-by-hunk in both directions (nothing unlisted changed, no follow-up-slice leakage), the named edge-case probes walked, old-code-failing status spot-verified, gates re-run independently;
its two record-count nits were fixed and its one observation (a derived negative P/E reaching `pe-ratio` ledger conditions — a pre-existing behavior class) is recorded under the design notes.

External review (Codex round 1, every finding verified against code before adoption): **eight findings, all adopted** — two through explicit rulings:

- **(High) the weights `%`-suffix was stripped before the unit decision**, so a sparse explicit `"1.4%"` row stayed 1.4 and the new `us_share` cap turned it into full-US, waving the fund past the ≥70% guard.
  An explicit suffix now declares the whole set percent regardless of sum; only suffix-less numerics fall to the sum heuristic (the ambiguous suffix-less sparse residue stays clamp-bounded), and the test pins the suffixed form.
- **(Medium) a same-identity corrected-clean value left stale breach state** — and a carried confirmed state could emit a Confirmed crossing carrying the clean value.
  The same-observation arm now recomputes: a corrected-clean read resets the streak, so no crossing can stand on or emit a value that no longer breaches.
  One pre-existing overlay-chain test was found to fabricate exactly this inconsistent state (a confirmed streak on a value that never breached the served data — passing only because nothing re-checked); it was rewritten to the honest cross-feed scenario, which now also pins the stale-arm recorded-id crossing keying end to end.
- **(Medium, ruled) the FRED anchor's malformed-newest fallback was unbounded** — skipping `"."` markers is correct FRED semantics, but a long malformed run would serve an arbitrarily stale print as current.
  Ruled to a **10-day staleness bound** (`RATE_ANCHOR_MAX_AGE_DAYS`): an older latest-print errors onto the callers' existing hard-fail / unknown postures.
- **(Medium) pre-profit income and cash-flow windows were internally contiguous but never cross-aligned** — capex intensity could divide mismatched trailing years.
  The one cross-statement ratio now requires matching newest period-ends; single-source reads keep their own windows.
- **(Medium) the SEC duration guard failed open on an absent or unparseable `start`** — readmitting exactly the stub rows it exists to stop.
  Now concept-aware and fail-closed: duration concepts require a parseable ~annual span; the explicit instant list (`Assets`, `StockholdersEquity`) skips the check, and unknown future concepts default to the fail-closed side.
- **(Low, ruled) the bare-`short` maturity veto was over-broad** — "ProShares Short 20+ Year Treasury" (a genuine inverse fund) contains "treasury" and was suppressed.
  Ruled to **duration phrases** ("short-term" / "short term" / "short duration" / "short maturity"): TBF-style inverse bond funds flag correctly; the known cost — "iShares Short Treasury Bond"-style duration names now mislabel as leveraged/inverse (still `role_risk_only` either way) — is a recorded big-run watch.
- **(Low) three frontend residuals**: both `cost` sort keys still ranked the fabricated raw basis (now the suppression-aware `costBasisOf`, nulls-last); direction classes keyed on unrounded values, so a "0.0%" cell could render red (now `pctDir` / `moneyDir` key on the rendered value); `weightBand`'s final fallback could still collapse (escalation extended to three decimals — the honest floor for dust positions).
- **(Low) `docs/portfolio-analysis.md` still stamped `grade-v2`** — the canonical line now reads `grade-v2.1` with the input-semantics cause and the no-band-move clarification.

External review (Codex round 2 — a ground-up independent pass over all eight segments, not a validation of round 1; every finding verified against code before adoption): **six findings, all adopted**, two of them completing round-1 partials:

- **(High) the fund-weight unit question is settled by the wire contract, not a heuristic** — FMP's own reference serves the field in percent in both forms (numeric `1.8` = SPY's ~2% Basic Materials sleeve; `"97.82%"` strings), so `weights_from_value` now **always divides by 100** and the round-1 sum heuristic is deleted (it deliberately kept a sparse suffix-less `1.4` as a "fraction" — which the `us_share` cap then read as full-US).
  The deeper half: `composite_yield`'s coverage renormalized over the **served rows' sum**, so a sparse response reported 100% coverage and priced the whole fund off a sliver past the ≥70% guard — `covered_share` is now the **absolute** priced share of the fund (the code now matches its own doc comment), while the yield stays renormalized over covered weight.
- **(Medium) the full role-risk pass now covers the same fund-computable surface the quick check evaluates** — its reduced metrics carried only the expense ratio, so a sweep-confirmed price-leg crossing (trailing return / volatility, which 6g's class-aware check rightly admits for funds) read unevaluable at the full pass, was never acknowledged, and re-raised on every later sweep after the successful pass cleared the store.
  The price-derived legs now compute from the closes the dossier already carries; pinned end to end through `analyze_holding`.
- **(Medium) run identity moved to insertion order** — the round-1 max-id prune guard kept the backdated row but `latest_run` / the capped history still ordered by `created_at`, so under a stepped clock the just-persisted run survived as an invisible eleventh row while the page refresh and the next diff baseline used the prior run.
  All three queries now order by `id` (insertion order — preserved across machines by the portability export's id-order), `created_at` stays display data, the prune guard became inherent and was removed, and the regression test asserts the **capped** production queries.
- **(Medium) consensus fiscal dates are parsed before they order** — the raw-string sort inverted the NTM blend on FMP's non-zero-padded dates (`"2026-9-30"` sorts after `"2026-12-31"` — the dividend windower's documented wire quirk, same feed family); filter / sort / dedup / weight now all run on parsed dates, and an undatable row never enters the forward set.
- **(Low) `weightBand`'s last fallback floors at `<0.001`** — a positive dust endpoint that still rounds to zero at three decimals can no longer wear the sell-all "0" read (completing the round-1 partial).
- **(Low) `"ultra"` joined `"short"` under the duration-phrase guard** — the unconditional fragment misread "Ultra Short-Term Bond" duration funds as daily-reset vehicles; "UltraShort 20+ Year Treasury" and "Ultra S&P500" still flag.

External review (Codex round 3): **two findings, both adopted** — round 2's other five implementations confirmed complete:

- **(Medium) the strict estimates read now splits undatable rows from an honest past-only set** — the strict adapter validated only the array shape, and the shared shaper silently drops undatable rows, so a drifted body (`[{"date":"soon",…}]`) read `Ok(None)` — exactly what the Revision family clears on — letting a stale verdict carry uninspected behind a fabricated `fresh_clear` (selective analysis force-includes only `unknown` families).
  Every row must now carry a datable `date` on the strict path (non-zero-padded dates stay valid); undatable is `Err` → family `unknown`, past-only stays the honest `Ok(None)`.
- **(Low) the `<0.001` dust-band fallback is now spec-pinned** (a band below three-decimal precision renders "maintain <0.001–0.002%", never a literal zero).

## Verification

At the converged batch (post Codex round 3): cargo **973 lib + 32 integration / 0 fail**, **clippy 0 warnings**, `npm run build` clean, **40 node + 222 vitest** —
round 3 added the strict-estimates undatable-vs-past-only pin and the dust-band spec case.

After round 2: cargo **972 lib + 32 integration / 0 fail** —
that round added the absolute-coverage pin, the parsed-date consensus ordering pin, and the role-risk price-leg end-to-end pin, rewrote the weights test to the always-percent contract, extended the short-screen test with the three "ultra" cases, and rewrote the stepped-clock test to assert the capped production queries (`latest_run` + the retention-capped history).

After round 1: cargo **969 lib + 32 integration / 0 fail** —
that round added three lib tests (the FRED anchor bound, the same-observation corrected-clean reset, the cross-window capex alignment), extended the weights / short-screen / SEC pins to the adopted shapes, rewrote the overlay-chain fixture to the honest cross-feed scenario, and extended the signed-zero spec with the direction-class assertion.

At the originally applied batch (pre external review): cargo **966 lib + 32 integration / 0 fail** — 22 new lib test functions (the dividend-window boundary pin, the sum-based weights read, the short-screen vocabulary, the us_share cap, the SEC amendment+duration pin, the two house-view extractor pins (levels; multi-byte cap by construction), the signed-P/E seam, the contiguity helper + the two dossier window pins, the three ledger-identity pins (stale non-event; stale re-raise keying; reset-clears-ack), the calendar back-window pin, the zero-net arm, the class-aware 6g downgrade pair, the debut-after-abstention open, the carried-audit attach skip, the lean-less cash-freed validation, the duplicate-proposal violation, the stepped-clock prune guard) plus two contracts pinned as new assertions inside existing tests (the not-rated fold gate, the pre-truncation comparable count) — **clippy 0 warnings**, `npm run build` clean, **40 node + 222 vitest** (4 new specs — the band collapse pair, the role-risk demotion tag, the option-row withholding, the signed-zero render).
One pre-existing quick-check fixture was corrected rather than pinned around: its synthetic *monthly* statement period-ends read as four consecutive quarters only because nothing checked — the contiguity guard now (correctly) rejects them, and the fixture uses real quarterly ends.
