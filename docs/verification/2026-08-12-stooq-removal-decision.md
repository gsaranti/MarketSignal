# 2026-08-12 — Stooq removal: decision, evidence, and the removal slice's inventory

A user ruling recorded for the next session, with the probe evidence behind it and the full inventory the removal slice will work from.
This record supersedes the rung-order slice (`BUILD.md §What remains` item 5) and the standing decision it carried ("Stooq stays the primary rung, revisited only on the big run's data-health read"): Stooq is removed everywhere instead, before the big run.

## The decision

Remove Stooq from Market Signal entirely, for now.
The FMP paid plan's dated-EOD rung becomes the only deep-price source, with no Stooq rung ahead of it.

Why, in the user's terms:

- The paid FMP plan allows 200 requests/minute, and the adapter already carries the minute-crossing 429 ladder (63 s cumulative) built for exactly that limit.
  The holding-analysis request pattern comes nowhere near it.
- Stooq is untestable while its JS proof-of-work wall stands: there is no way to verify the adapter against the live service.
- An untestable dependency that may start working again unannounced is a production risk, not an asset — it could resurface someday in a shipped app on a code path no run has exercised since it went dark.

## The evidence

- **The wall is total and current.**
  Probed 2026-08-12: the daily-CSV endpoint (`/q/d/l/?s=aapl.us&i=d`) answers HTTP 200 with an HTML body containing a JavaScript SHA-256 proof-of-work challenge (find a nonce whose hash carries four leading zero hex digits, then POST to `/__verify`), regardless of User-Agent.
  Only a JavaScript-executing browser session can pass it; the deterministic `reqwest` adapter cannot, by design.
  This is the same interstitial first observed 2026-08-02 ([2026-08-02-fmp-light-eod-adjustment-basis.md](2026-08-02-fmp-light-eod-adjustment-basis.md)), preceded by the run-wide daily-hits throttle observed 2026-07-31.
  Solving the challenge natively in Rust is technically trivial but was considered and rejected: it deliberately circumvents an anti-automation measure the provider chose, and a load-bearing rung must not sit on an arms race.
- **FMP covers the reads.**
  Probed 2026-08-12 (two requests against the paid key): `historical-price-eod/light` serves AAPL back to at least **1985** (a 1985–1995 window returned in full, 2,780 rows), with a **5,000-row (~20 trading years) per-request cap** — an open-ended query returned exactly 5,000 rows reaching to 2006, so deeper history needs `from`/`to` windowing.
  The suite's deepest current ask is `DEEP_HISTORY_LOOKBACK_DAYS = 1,600` calendar days (~1,100 trading rows), nowhere near the cap, so every built read is covered in one request per symbol.
- **The basis is already proven interchangeable.**
  FMP light-EOD serves the same split-adjusted, dividend-unadjusted basis as Stooq, desk-verified 2026-08-02, so no computed value changes meaning under the swap.
- **The fallback path already works.**
  The 2026-08-10 tracker (attempt 1) shows the Stooq row FAILED and the FMP deep-price-history substitution succeeding cleanly for every holding.

The accepted cost: the dispersal principle for the bulk price load (keeping the highest-volume per-holding read off the shared FMP key) is consciously retired for this leg.
The quota math above is the ruling's answer to it.

## Inventory — built code

The removal slice's surface, verified by grep on 2026-08-12 at `067de1a`.

| Site | Current role | Replacement |
| --- | --- | --- |
| `src-tauri/src/stooq.rs` | The adapter: CSV endpoint, `.us` symbol map, 1 s politeness throttle, HTML-body typed throttle (`StooqThrottled`), run-wide breaker | Delete the module |
| `src-tauri/src/lib.rs:739-743, 798` | Constructs the shared `StooqSource`; clones it into the outcome pass (`out_stooq`) | Remove the wiring |
| `src-tauri/src/portfolio/job.rs:187-250` | Per-holding deep history: Stooq primary → FMP `fetch_dated_eod` fallback, three-way substitution gap notes | FMP dated-EOD becomes the only source; on its failure the anchor window starves to the existing raw-percentile / carry fallback (today's both-rungs-failed terminal). Substitution gap wording retires |
| `src-tauri/src/portfolio/outcome.rs:723-800` | Outcome pass `daily_closes` seam: Stooq primary with `^spx`→`^GSPC` translation on the FMP rung | FMP-only via `fetch_dated_eod`; SPDR sector ETFs are already plain FMP symbols |
| `src-tauri/src/portfolio/mod.rs:1286` | Data-health: "holdings whose deep-history (Stooq) fetch degraded" | Reword to the FMP leg; degraded still counts, substitution vocabulary retires |
| `src-tauri/src/fmp.rs:5220-5242` | `fetch_dated_eod` doc + tracker row label "Deep price history (Stooq fallback)" | Relabel "Deep price history"; it is the primary |
| `src-tauri/src/portfolio/engine.rs:354, 1035, 4616` | Comments citing the Stooq↔FMP swap (including the out-of-order-older-bars rationale) | Comment updates only; the ordering/contiguity guards are source-agnostic and stay |
| `src-tauri/src/portfolio/quick_check.rs:219` | Already Stooq-free by design ("no Schwab, no Stooq, no model") | No change; the "no Stooq cache exists" reduction note becomes moot |
| `tests/components/App.spec.ts` | "Stooq" as a fixture provider string | Cosmetic swap |

## Inventory — living docs

- `docs/data-sources.md §Stooq` — the section goes, but its **benchmark / futures identity table is single-homed there** and consumed by reference; it must move (the FMP section is the natural home), with `^spx` restated as FMP's `^GSPC` and the SPDRs as FMP symbols.
  The shared-sourcing dispersal paragraph inverts: the bulk price load deliberately rides the paid FMP key (this ruling).
- `docs/portfolio-analysis.md` / `docs/portfolio-workflow.md` — rung references in the failure posture and the per-holding pipeline.
- Trade Opportunities docs + `docs/storage.md` + `docs/configuration.md` + `docs/interface.md` + `docs/README.md` (all design-stage, unbuilt): the gold / silver / copper futures context (`gc.f` / `si.f` / `hg.f`) re-homes to FMP commodities (`GCUSD` / `SIUSD` / `HGUSD` — gold live-verified even on the free tier; verify silver / copper shapes at TO build time), and the "Stooq cache" design re-homes or drops.
- `docs/verification/big-run-watch-set.md:112-113` — the two Stooq watch items retire, replaced by an FMP-only watch: quota consumption and 429-ladder behavior under a full run's price load.
- `BUILD.md` — item 5 superseded by this record; the local-suite invariant naming Stooq in the engine's source list updates.
  (Session-end / user-run writes, per the Metis convention.)
- The five dated verification records naming Stooq are point-in-time evidence and stay untouched.

## Open at plan time

- **The market-benchmark identity.**
  `outcome.rs` `MARKET_BENCHMARK = "^spx"` is Stooq's name; the FMP rung already translates it to `^GSPC` at the seam.
  Either keep `^spx` as the abstract identity and translate at fetch (no churn against persisted episode snapshots), or rename to `^GSPC` and check every persisted reference first.
- **The FMP-only watch items** replacing the retired Stooq lines in the big-run watch set.
- **Whether the outcome `daily_closes` trait seam stays** (it is also the test seam) with an FMP-only live impl, or collapses.

## Disposition

Handled in a new session: plan the removal slice via `/metis-plan-task` against this record, build it, then proceed to big-run attempt 2 with FMP as the only price rung.
