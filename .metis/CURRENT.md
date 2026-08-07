# Current session handoff

## What happened

**The scoped conformance check + off-spine doc pass completed and merged** (PR #67, squash `99b4f61`, branch deleted) — twelve parallel passes in one wave: seven re-checking every changed behavior of the two unwalked commits (`cdb7977`, `512d5ec`) across **all** paths, five walking the seven owning-doc sections no spine ever covered. **26 findings, four cross-pass convergences.**

**Both of the charter's own leads resolved negative — do not retry them.** The `quarters_contiguous` call-site discrepancy (7 sites vs "three"/"five") was prose imprecision at three granularities; all **eight** fixed-width statement windows are guarded. And the option/bond cost-basis scenario is **unreachable**: `AssetClass::is_gradeable()` routes those classes to `NotRated` before the engine stage and `NotRatedContribution` carries no cost-derived field — confirmed independently by three passes (C1, D1, D3), with `outcome.rs` carrying no `cost_basis` reference at all.

**Three Tier-1 defects built** (all ruled to the recommended disposition; mechanisms in BUILD.md §Local analysis suite): the UTC-dated sector-P/E snapshot whose **empty 200** silently abstained every priced US-equity fund on evening runs, the tier's unsigned debt/equity legs reading negative equity as *low* leverage, and priced funds authoring ledger conditions on a different window than the sweep evaluates.

**The systemic result: the ET slice's conversion is incomplete**, found by three passes independently at five sites — it converted the sites its own findings cited and stopped. Two Codex rounds; round 2 caught this walk committing that same thesis (round 1 fixed the two cardinality homes its finding named and missed `data-sources.md`'s canonical framing sentence). BUILD/INDEX aligned in-session.

## Current state

Nothing in flight; working tree clean on `main`. **23 findings carry to their own ruling round**, each enumerated with `file:line` evidence in `docs/verification/2026-08-07-scoped-conformance-check.md` — the citable home, so nothing needs re-deriving. They are not uniform:

- **~11 gate the big run** — the ET conversion's four remaining sites (house-view freshness gate, quick-check rate-cache age, per-holding `run_date`, `fmp.rs` TTM dividend window), the unguarded signed metrics in `resolve_series` (P/E + debt/equity — same root cause as the tier fix that just landed), the stale-basis bridge, the falsifier pair (`confirmed_at` from the consuming run; dedup keyed on a changing id), the TR dividend window's end bound, the abstention arm ignoring an action/lean change, and wall-clock-keyed episode supersession. All write wrong data into the ledger or outcome machinery the run is meant to bank.
- **2 code defects found by doc passes** — duplicate `provider-credentials` warning rows; the per-instance Stooq breaker.
- **1 accept-with-note candidate** (contiguity→annual basis flip) and **1 that rules either way** (`/chains` cardinality vs gating the fetch on `is_gradeable()`).
- **7 doc-only**, gating nothing.

Recommended shape: rule all 23 in one pass, then **split the fix batch** — run-gating code first, doc corrections alongside the long-doc-line cleanup or after the run. The two batches are independent.

Then the **long-doc-line cleanup** (now also covering lines this session lengthened in `data-sources.md`; `storage.md:187` is the pre-existing item), then the **big confirmation run** (dev app, process name `market-signal`).

## Open questions

- **TO hard-trigger acceptance cases** — parked for the TO implementation slice (no other home): (1) carried + deep hard trigger + model `still-valid` → archived, no shadow entry; (2) identical through all three deep-pass routes; (3) cheap-pass hard signal → warning only; (4) debut hard trigger → shadow rejection; (5) soft trigger → stand-in capped, conviction preserved, no forced archival.
- **Big-run watches** — the carried set (Schwab `averagePrice` multiplier, `^GSPC` mapping, estimates ordering, SEC sub-annual durations, FMP in-progress-bar, sector-taxonomy joins, SHV-style labels, exchange codes/B3, OCC slash notation, boundary-day rates, two-arm divergence rates) **plus this session's four**: sector-P/E walk-back depth (how often the first candidate misses), the risk-tier distribution now negative-book issuers take High, priced-fund ledger flag rates on the shared window, and the basis-flip rate on a one-quarter feed gap.
- **Design notes awaiting rulings** (piece-3 record) — carried-audit data-health mixing on selective runs; unbounded model-arm renders; "rated N" sidebar wording; `rate_prints.fetched_at` stamp; `ScoredLabel.labeled_at` / `run_date` staying UTC display stamps.
- **Live-evidence caveat** — the walk-back's "holidays serve carried values" warrant rests on the adapter's recorded 2026-07-16 verification, not re-probed; if that is ever invalidated, the cardinality claim moves with it.
- **Two-arm follow-ups / research-loop activation obligation / standing list** — unchanged.

## Where to start

`/metis-session-start`, then open `docs/verification/2026-08-07-scoped-conformance-check.md` §Dispositions and **rule the 23 carried findings** — they are already evidenced, so this is a ruling pass, not a re-investigation. Rule the two convergence pairs once each. Then implement the run-gating subset first: the **four ET sites plus the signed-metric guard in `resolve_series`** is the natural opening batch — shared root cause with the tier fix, and the test shape already exists. Doc corrections can ride with the long-doc-line cleanup. The big run closes the block.
