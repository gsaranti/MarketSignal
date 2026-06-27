# Current session handoff

## What happened

Cleaned `docs/data-sources.md` so the **per-job endpoint tables** (Portfolio Analysis, Trade Opportunities) list **only FMP paths actually called** — every blocked/off-plan endpoint pruned, with the **FMP paid-plan audit** kept as the single home for the blocked→fallback mapping. Each real fallback now appears as its own **official source row**: `mergers-acquisitions-latest` (+ 8-K), ETF sector/country weightings as the look-through proxy, SEC EDGAR **optional coarse 13F**, SEC **N-PORT** (optional enrichment), the **SearXNG web loop** (incl. transcript commentary), and **CBOE** added to Portfolio's surface. The two genuine capability losses (`etf/asset-exposure`, holder-level 13F) stay only in the audit. **Three Codex rounds addressed:** restored the M&A `+ 8-K` clause; fixed a **TTM shorthand leak** (`balance-sheet`/`cash-flow` TTM are engine-derived, not blocked-endpoint calls — only the allowed `income-statement-ttm` is listed); tightened the SEC 13F rows to **run-level (optional), filer-keyed**; and fixed the stale SEC-catalog claim (~line 245) to mark **institutional 13F the off-plan exception**, resolving a contradiction. Final Codex round = clean pass.

## Current state

Committed + pushed to **`origin/main` at `3c50bfe`** ("Docs: Prune blocked FMP endpoints from per-job tables; surface fallbacks as official source rows" — 2 files: `docs/data-sources.md` + `.metis/INDEX.md`). **INDEX.md** got two precision fixes — line 137 (Portfolio endpoint-surface: transcripts off-plan→web loop, look-through via weightings, the new keyless fallback rows named) and line 166 (SEC-EDGAR-role: institutional 13F off-plan exception). **BUILD.md reviewed and left unchanged** — it already states the off-plan/fallback architecture at the right altitude; this was data-sources-level precision, not a load-bearing shift. Working tree clean; nothing in flight.

## Open questions

- **TO 5g bounded-positive** — a metric-*confirmed* since-flagged gain still gives no positive nudge (docs are cap-only and internally consistent); decide later whether it earns a bounded positive.
- **Implementation-time schemas (build alongside code)** — backlog: capital-efficiency/dead-money field + sign-aware unrealized-P/L; since-flagged read; Portfolio thesis ledger / sizing spine / intrinsic-action split / `technology_read`; TO watchlist bar / hypothesis score / event-impact route; hierarchical-distillation knobs.
- **Report enrichment (paid-FMP, three families)** — calendar consensus/surprise builder + the valuation-over-time prompt landmine (must be *revised*, not extended); spec'd under the report job's *Planned report enrichment*, not built.
- **Cross-job isolation (parked)** — Portfolio naming a *specific* TO redeploy target needs the cross-job-read decision; deliberately not built.
- **BUILD.md compression** — ~5.6k+ tokens; ceiling revisit deferred post-release.
- **Carried:** local-suite build order (live-Schwab OAuth → full Portfolio → Opportunities, M5-gated); register Schwab dev app; `market_clock` holidays; Cadence Run B; report-side nits (COT extreme-weighting, opus-main leaning, no PDF `@page` margins).

## Where to start

The data-sources.md endpoint-table cleanup is landed, pushed, and Codex-clean — nothing to follow up there. Unblocked leads unchanged: the **job-doc deepening initiative** (now on a cleaner, internally consistent data-sources.md) or the parked **TO 5g bounded-positive** decision. Implementation items still wait on their gates (M5, paid-FMP).
