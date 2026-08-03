# Grade-band shadow-tune — certification, statement-gap closure, and the grade-v2 bands (2026-08-03)

The calibration-tier slice queued from the first live Portfolio run's F4/F5 findings
  ([2026-07-31-first-live-portfolio-run.md](2026-07-31-first-live-portfolio-run.md)):
  zero A/B letters across 44 priced holdings, with statement-field gaps depressing axes underneath.
Executed in the locked order: certify the sub-score formulas against spec first, close the statement-field gaps second, touch band constants last.

## 1. Formula certification (the unaudited corner of the math) — PASSED, 0 mismatches

Method: run `3b21ae85`'s persisted `run_json` was exported from the dev store and replayed through an `#[ignore]`d harness
  (`certify_run_grade_path`, `src-tauri/src/portfolio/engine.rs`) that recomputes every priced holding's sub-scores, composite, and letter
  from the audit's persisted `ComputedMetrics` and diffs them against the persisted verdict.
Stocks get the full derivation check (metrics → clamped maps → neutral-50 imputation → letter, including the `low_confidence_grade` marker);
  priced funds get the sub-scores → letter roll-up check alone, since their valuation/risk derive from composite history the audit does not carry.

Result: **44 priced (31 stocks): 0 derivation mismatches, 0 roll-up mismatches** —
  the shipped formulas reproduce the persisted audits exactly, so the F4 compression is entirely an input/band problem, not a formula defect.
Three previously unpinned contracts gained offline unit tests with the certification:
  the ≥-inclusive letter cutoffs at exactly 85/70/55/40, the neutral-50 imputation dividing by the full weight sum (with the marker), and the negative-P/E fixed low score.

## 2. The gap census — F5 was understated

The census over the 31 stock audits:
  `debt_to_equity` and `revenue_growth` missing on **all 31** (structural — `total_debt` and `revenue_prior` had **no source at all** in the live path, so the risk sub-score rested on volatility alone for the entire book);
  `gross_margin` missing on ~a third (the SEC `GrossProfit` tag is simply absent for many issuers: V, MA, DIS, SBUX, UBER, LUV, LCID, VRT, TMUS…);
  `pe` missing on the loss-makers (SNDK, TEAM, TDOC, LCID), `pb` on negative-equity SBUX.
Beyond the gaps, the SEC-annual statement lines produced outright artifacts —
  MA's persisted quality of 8.3 on a ~46 %-net-margin business (a stale/mismatched annual pair), AAPL/NVDA valuation floored at 0.0.

## 3. Statement-gap closure (what changed in code)

- **TTM statement basis** (`dossier::apply_ttm_statement_basis`): the four newest quarterly income prints — already fetched for the anchor window; the parser now carries `netIncome` / `grossProfit` / `costOfRevenue` — sum to TTM revenue / net income / gross profit, quarters five-through-eight to the prior-TTM revenue.
  One basis per holding: adoption requires revenue + net income on all four newest quarters; gross profit sums where every quarter carries it (or derives per-quarter from cost of revenue), else stays a gap even when SEC has an annual print; on adoption the SEC statement fills are skipped wholesale so bases never mix inside a ratio.
  Zero new HTTP calls.
- **Balance-sheet leg** (`fmp::fetch_balance_sheet`): one light `/stable/balance-sheet-statement` quarterly call per stock — `totalDebt` + `totalStockholdersEquity` (preferred over `totalEquity`), SEC equity the fallback.
  The risk sub-score's leverage leg exists for the first time on live data.
- **SEC annual fallback sharpened** (`sec::latest_two_annual_usd`): the prior-year revenue reads from the **same GAAP concept** as the latest print (second-latest distinct 10-K period end, comparative duplicates collapsed), so the annual-basis growth read can't mix revenue tags.
- **Fund SEC skip** (`job.rs`): a fund holding never fetches company facts — its statement lines feed nothing on the reduced path, and the ETF trust entity routinely 404s the facts API (the run's QQQ gap noise); tripwire-tested.

## 4. Calibration-surface refresh (bounded live probe, user-approved)

`probe_refresh_statement_surface_for_band_tune` (`fmp.rs`, `#[ignore]`d) re-derived the statement surface for the run's 31 stocks
  through the new fetch path — quote + quarterly income + balance sheet, **93 HTTP calls total**, ~250 ms politeness pacing, zero gaps.
The TTM basis adopted on **31 of 31**.
Representative corrections against the persisted surface:
  NFLX net margin 28.2 % / P/E 22.3 (persisted: margins gapped, valuation 27);
  MA net margin 46.3 % (persisted quality 8.3);
  D/E present everywhere — including SBUX at **−2.92** (negative equity), which exposed that the inverted leverage clamp would read negative equity as maximally safe.

## 5. Band sweep and the grade-v2 decision

`sweep_grade_bands` (`engine.rs`, `#[ignore]`d) scored three candidate band sets over both surfaces
  (per-stock letters, distribution, Spearman rank correlation against the shipped bands — F4's "relative ordering carries real signal" as the constraint).
On the **refreshed** surface (31 stocks):

| Band set | A | B | C | D | F | Spearman |
| --- | --- | --- | --- | --- | --- | --- |
| grade-v1 (shipped) | 0 | 0 | 13 | 8 | 10 | 1.000 |
| moderate | 0 | 4 | 13 | 6 | 8 | 0.939 |
| **recentered-growth (chosen)** | 0 | 8 | 10 | 7 | 6 | 0.913 |

Even on clean inputs the v1 bands produced zero A/B — the compression was genuinely in the bands, not only the data.
The chosen set decompresses the top (META 84.0, MSFT 82.4, V 77.1, NFLX 75.6, NVDA 72.5 → B) while the speculative tail stays put
  (LCID 35.7 F, TSLA 34.9 F, TEAM 39.9 F, LEU F) and ordering holds at 0.913.

**Landed as `grade-v2`** (user decision, 2026-08-03), in the engine's calibration surface:
  net margin 0→30 %, gross margin 15→65 %, P/E 70→12 (non-positive P/E keeps the fixed 20), P/S 25→2, P/B 30→2,
  daily volatility 4.5 %→0.5 %, debt/equity 2.5→0 with the new **negative-D/E → 0 guard** (the negative-P/E rule's mirror, unit-pinned).
Weights and the A/B/C/D cutoffs are untouched — their recalibration stays reserved for the full sector-aware normalization slice
  ([portfolio-analysis.md §Starting parameters](../portfolio-analysis.md#starting-parameters-calibratable) — Grade normalization & inputs).

## 6. Versioning

Each audit now stamps `grade_parameter_version` (`GRADE_PARAMETER_VERSION = "grade-v2"`, serde-default so old runs decode as `None` = the pre-stamp v1 bands),
  so a band recalibration — letters moving with no input change — stays recognizable to the what-changed audit and outcome-learning cohorts.
The certification harness is version-aware: the derivation leg only replays a run stamped with the current version
  (run `3b21ae85` was derivation-certified against its own v1 bands **before** the retune — this document is that record);
  the roll-up leg (weights/cutoffs) certifies every vintage.

## 7. Residue

- **No A letters yet**: META grazes the ≥ 85 cutoff at 84.0.
  Deliberate — cutoffs are out of this slice's scope; revisit with the normalization slice (or the big confirmation run's evidence).
- **Buyback-depleted equity still punishes P/B** (AAPL 41.9, MA 89.6 → 0 under any sane band) and negative-equity names lose the P/B leg entirely —
  the structural case for the reserved sector-aware normalization, not for wider bands.
- The sweep's "shipped" baseline reads the **current** consts, so post-tune it no longer reproduces the v1 tables above; this record preserves them.
- Probe hygiene: 93 calls on the paid FMP key, one-shot, logged here; the refreshed-metrics JSON lives in the session scratchpad only (it derives from the user's real book and never enters the repo).

## 8. External review round (Codex, 2026-08-03) — five findings, all confirmed and fixed

- **SEC duplicate-period dedup kept the earliest-filed row** (a regression this slice introduced against the old `max_by` last-wins): a later 10-K restating a comparative year would have served the pre-restatement print, and `latest_annual_usd`'s delegation exposed every SEC fallback field.
  Fixed: per period end the **latest-`filed`** row wins (array position the proxy where `filed` is absent), pinned by a restated-comparative fixture.
- **TTM gross profit was all-or-nothing across derivation routes** where the docs promised per-quarter: fixed — each quarter contributes its reported gross line or derives its own from revenue − cost of revenue, mixing freely; pinned.
- **A JSON-null `totalStockholdersEquity` blocked the `totalEquity` fallback** (key-presence vs numeric-value): fixed — numeric-first per key; pinned.
- **The sweep baseline lacked the negative-D/E guard** production now applies — a mislabeled hybrid after the retune: fixed, baseline mirrors shipped behavior.
- **The Spearman helper assigned arbitrary distinct ranks to ties** under the no-ties shortcut: fixed — average ranks + Pearson-over-ranks, pinned.
  The §5 tables' recorded 0.913 / 0.939 were computed tie-naive; no ties were present in either surface at the recorded precision, so the decision-time numbers stand.
