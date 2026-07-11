# Notes for the next Codex docs review

Written 2026-07-11, after resolving all 21 findings of `codex-review.md` (independent run 2).
Purpose: what changed and what is deliberate, so the next review doesn't re-flag handled or by-design material.
Every fix follows the corpus's single-home discipline — the "canonical home" named per item is where the contract now lives; other mentions are pointers.

## Disposition of the 21 round-2 findings

| # | Finding | Disposition | Canonical home now |
|---|---|---|---|
| P1-1 | Shorts long-only downstream | Fixed (scope-out) | `portfolio-analysis.md` §Asset eligibility — net-short equities are **not rated** (short reason); signed exposure feeds the roll-up; a long↔short reversal force-includes in selective runs; outcome tag vocabulary gains `reversed` |
| P1-2 | Failed sweep = silent clear | Fixed | `portfolio-analysis.md` §The quick check — typed per-family sweep states `fresh_clear` / `flagged` / `unknown`; `unknown` force-includes like a flag; degraded-sweep note in the Research-stale badge family |
| P1-3 | FI/option risk unsourced | Fixed (narrowed) | `portfolio-analysis.md` §Portfolio roll-up and construction — contribution = market value + signed notional (OCC-derivable); duration / credit / **standalone**-option delta are **typed gaps by design** (no on-plan source; held-underlier overlay delta from its own chain is the exception) |
| P1-4 | Vector memory isolation | Fixed | `storage.md` §Local Vector Memory — entry kind = purpose boundary (summary → dossier recall; learning → calibration only); TO rows lifecycle-tagged; **embedder identity, never dimension**, is the compatibility key (re-embed from retained content on change) |
| P1-5 | Step 7b model-authored sizing | **Refuted** | Invariant is scoped to engine-owned *analytical* values (`local-models.md` §Context-memory discipline); bounded sizing is a documented model decision under the joint-feasibility check. Clarifying clause added at `portfolio-workflow.md` §How to read |
| P1-6 | EDGAR CIK resolution | Fixed | `data-sources.md` §SEC EDGAR — the cached `company_tickers.json` resolver is a **named prerequisite of the full Portfolio slice**; unresolved/ambiguous CIK → typed unknown feeding the degraded-sweep rule; SEC endpoint rows tagged "CIK-gated" |
| P1-7 | Fund floor vs bond/commodity | **Refuted** | The carve-out is stated at §Evidence floor (the sentence after the fund analog); routing precedes the floor; the floor input is sector/country weightings (not holdings weights). Scope words added ("exposure-priced (equity-fund) branch") |
| P1-8 | Stooq underspecified | Fixed (narrowed) | `data-sources.md` §Stooq — `^spx`, the SPDR sector-ETF mapping from FMP sector labels, copper `hg.f`, split-adjusted / dividend-unadjusted, all single-homed there and flagged for M5 live verification. Adjustment semantics + fallback were already specified (§Outcome learning / §Stooq) — only symbols and the mapping were missing |
| P1-9 | Durable vectors vs run retention | Fixed | `storage.md` §Local Vector Memory "Own retention" — kind-based carve-out: durable-learning rows tie to their episode stores, not the run window (both local namespaces) |
| P2-1 | Segment-series ingestion | Fixed | `trade-opportunities-workflow.md` §Step 5d/5e/5f — typed `leading_metric_observation` returned at 5e, app-validated + appended, **Step 5f recomputes** the leading-metric read and `business_runway` (Portfolio-6e-style carve-out) before the 5h gate/floor; segment series' recheck class is **`research`** (`filing` = engine-refreshable model-free only); stored series has a schema home in `storage.md` |
| P2-2 | "Computed" mislabeled network-free | Fixed (narrowed) | `trade-opportunities-workflow.md` §How to read — "engine-only" = *no generative model, never no network*; Steps 3c/7 tagged `Computed + API retrieval`; maintenance/outcome cardinalities added to the TO endpoint surface; label-time coverage rule now held by **both** jobs (`storage.md`, TO §Outcome learning). ATO Quick Audit was already honest (names FMP/Stooq/FRED) — that citation was wrong |
| P2-3 | Dedup before union | Fixed | §Step 6 — cheap-swept carries join the dedup input as **collapse targets only**; direction app-enforced (debut may collapse into a carry's lifecycle; a live carry never collapses away); direction recorded in the collapse audit; Step 7 stays computed-only |
| P2-4 | Picked episodes outlive metric-label path | Fixed | `trade-opportunities.md` §Outcome learning — metric label populated per recheck class + lifecycle state; **`leading-metric-unscorable`** (counterpart of `terminal-unscorable`) for windows with no legal refresh path, excluded from calibration denominators; the archive stays price-only |
| P2-5 | Rotation max-age guarantee | Fixed (wording) | §Step 4 — max-age = **best-effort service target under the run's budget**; overflow forms a stalest-first backlog, surfaced (count + oldest age); liveness ("never ages indefinitely") is the only guarantee kept; all hard-bound phrasings removed |
| P2-6 | Picked episodes portability + cap | Fixed (narrowed) | Portability was already covered by the format-extension rule — the stale enumeration at `data-portability.md` §Build-order placement now lists the picked stores; the dangling "its own cap" resolves to the new **picked matured-archive retention (drafted 5,000)** row in TO §Starting parameters + a configuration.md knob |
| P2-7 | Event route selected before evidence | Fixed (narrowed) | Speculative route choice + in-route dormancy is the design; the defective clause (`:164`-area false parallelism) now says the app enforces the materiality gate **at card formation** against typed material-event evidence carried on the card |
| P2-8 | Horizon "model-authored" | Fixed | §Step 7 — `expected_thesis_realization` is rule-derived at the last deep pass's 5h from deep-pass inputs the cheap sweep never refreshes (freeze behavior unchanged; the tier's contrast stated) |
| P2-9 | FMP Articles "keyless" | Fixed | `data-sources.md` §FMP Articles + the TO endpoint row — free-tier but **authenticated on the shared FMP key**, "no new credential", never "keyless" (adapter code confirms: `fmp_news.rs` sends `apikey`) |
| P3-1 | `averageChange` unit bug | Fixed | §Planned report enrichment — `Π(1 + averageChange / 100) − 1`, serialized as a decimal fraction; ±25-percentage-point inadmissibility guard added to the single-homed rule set |
| P3-2 | Consensus double fallback | **Refuted** (wording tightened) | The `§endpoint surface` sentence referred to the narrative layer; it now says so explicitly — structured calendar fields stay names + dates on a missing estimate, never model-filled |
| P3-3 | Step 3 group miscount | Fixed | `report-workflow.md` §Step 3 — three enrichment bundles across **five** groups; the sector trailing-return is re-homed to **sector performance** (`SectorPerformance`), industries carry both reads on one snapshot |

## Deliberate designs — do not re-flag

- **Net-short equities are not rated** — a scope decision, not an omission. Signed exposure still reaches the roll-up; reversals force-include; the `reversed` alignment tag is excluded from aligned/contrary cohorts.
- **Duration / credit / standalone-option delta are typed gaps** — mirrors the bond-fund valuation gap; adding a bond-analytics source was considered and rejected (no verified on-plan source).
- **The event-impact route is chosen speculatively** — planning is deliberately seed-free and spends no fetch budget; the materiality gate is enforceable only at card formation, and a gate-less route staying dormant is by design.
- **Step 7 / ATO Quick Audit make network calls while "engine-only"** — "engine-only" is now defined as *no generative model*; the `+ API retrieval` type tag is the network marker.
- **The rotation max-age is best-effort, not a wall-clock bound** — DTO is user-run and the budget fixed; liveness + surfaced backlog is the whole promise.
- **The archive is price-only after departure** — a still-maturing pick's metric label deliberately freezes and records `leading-metric-unscorable`; post-departure metric re-pulls were considered and rejected to preserve the archive invariant.
- **Segment-revenue quarterly observations are research-extracted** — a deterministic 10-Q segment extractor was considered and rejected (non-uniform disclosures); `filing`-class membership requires model-free refreshability, so segment anchors are `research`-class.
- **A live carry never collapses away in dedup** — the direction rule is an invariant (matrix exit = deep invalidation only), not an oversight.
- **Bounded sizing is a model decision** — the compute-every-number invariant covers engine-owned analytical values only; Step 7b's target weight is engine-bounded, model-chosen, joint-feasibility-validated.
- **The SEC cross-check for Trade Opportunities remains non-blocking** — only Portfolio promotes the CIK resolver to a prerequisite.

## New drafted constants / typed states (deliberate drafts, shadow-tunable)

- Quick-check sweep states `fresh_clear` / `flagged` / `unknown` (per required signal family).
- `observed_net_alignment` value `reversed`.
- `leading-metric-unscorable` (metric-label availability), plus the TO label-time price-coverage **pending** rule.
- `leading_metric_observation` typed object `{ metric, period, value, units, filing / as-of date, source URL, confidence }`.
- Picked matured-archive retention: **5,000** rows (mirrors the shadow matured archive).
- `averageChange` sanity guard: **±25 percentage points**.
- Stooq identities: `^spx`, SPDR sector ETFs (`xlk.us` … `xlc.us`), copper `hg.f` — **pending M5 live verification**, stated in-doc; flagging them as *unverified* is redundant, flagging a factual error in the mapping is welcome.

## Known open items (named in-doc; re-flagging adds nothing)

- **Fund-form scenario-target methodology** — the fund slice's named blocking input (§Asset eligibility), deliberately open.
- **Scorecard display surfaces** (Portfolio + TO) — deferred, stated in both §Outcome learning sections.
- **Stage-and-swap import hardening** — named, unscheduled (data-portability).
- **Factor-exposure model + portfolio stress reads** — deferred together (§Starting parameters).
- **M5-gated live verifications** — local-suite smoke, Stooq symbol map, serving pre-flight.

## Notes on review mechanics

- Anchor sweep after the fixes: 1,119 relative links, 0 broken (GitHub slugging, duplicate-heading suffixes included).
- Two of round 2's citations were mis-located and worth avoiding as anchors next time: portfolio-workflow §Step 2 contains no EDGAR reference (the quick check's EDGAR sweep lives in `portfolio-analysis.md` §The quick check), and trade-opportunities §Continuity and isolation contains neither the uncapped-matrix nor the user-run fact (those live in §The opportunity space / §The two jobs).
