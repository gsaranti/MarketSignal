# Current session handoff

## What happened

A **documentation/design session (no code)** reviewing the two local-suite jobs and folding decisions into the docs. **Portfolio Analysis:** added deterministic **holdings change-tracking** (prior-run-snapshot diff → per-position new/increased/decreased/unchanged, plus exited names in the roll-up); **per-topic bounded research** (≤3 passes/topic, depth ≤2, a per-holding fetch+wall-clock budget that binds first); and **one-brain model residency** (the 122B fills research/distill/interpret by mode + embedder resident; 35B demoted to a benchmark-gated option). **Trade Opportunities:** full **reframe** — a two-mode hunt (early detection + continuation), **archetype** as a first-class lens (5 archetypes select signal weighting + valuation lens), **research *generates* candidates** (fixes the candidate-before-research gap), the narrative-vs-reality ratio, base-rate conjunction + mandatory bear case, a forensic gate, and an engine-computed price-action confirmer. Grounded in 5-agent historical-winner research + a data-gap probe + a discovery pass over the full FMP catalog (`references/fmp-api.md`, gitignored). **Data plan:** one shared FMP key upgraded to **paid** (fundamentals/estimates/revision-flow/`financial-scores` Altman+Piotroski/symbol-keyed positioning incl. congressional/bulk-screener), FINRA short interest, FRED/Stooq commodities, SEC EDGAR cross-check — FMP **bulk** dissolves the rate cap, symbol-keying dissolves the ticker→CIK prerequisite. Codex-reviewed the Portfolio decisions (4 findings, all fixed).

## Current state

All design-only, on branch **`docs/local-suite-portfolio-design-decisions` = PR #46** (3 commits: `4f6792d` portfolio decisions, `3f13a84` TO reframe + data plan, `accc832` price-action overlay), pushed, **open, NOT merged**; `main` unchanged at `367f09b`. Working tree clean. Touched: `docs/trade-opportunities.md` (rewrite), `portfolio-analysis.md`, `local-models.md`, `web-research.md`, `data-sources.md`, + `.metis/BUILD.md` & `INDEX.md` — all reflect the new design **on the branch, not yet on main**. **Standing direction set** ([[job-doc-deepening-initiative]]): the next few sessions are a deliberate **job-doc deepening sweep** (the jobs are the app's core). The **report data-packet enrichment** (FMP-paid: economic-calendar consensus+surprise, historical sector/industry P/E trend, IPO/M&A froth, true index breadth) is **parked for a sweep session** — non-hardware-gated, runnable now. Report data-source *logic* untouched.

## Open questions

- **PR #46 scope/merge** — title still reads "portfolio design decisions" but it now also carries the TO reframe + data plan; update title/description and merge when ready.
- **Live validation hardware-gated** on the M5 (carried) — verdict quality + runtime; first live run is the acceptance check ([[local-suite-hardware-gated]]).
- **Register the Schwab developer app** (carried) — long external approval; gates live-Schwab real-data runs.
- **Cadence Run B** (carried) — report #2 un-run: delta engine + memory recall; sanity-check yields vs the 2s10s claim + COT ([[manual-pivot-cadence-windows]], [[report-curve-number-consistency]]).
- **Standing report-side nits** (carried) — COT extreme-weighting; `market_clock` holidays/early-closes; opus-main leaning ([[live-config-opus-main-leaning]]); do NOT reintroduce PDF `@page` margins.

## Where to start

Continue the **job-doc deepening sweep** ([[job-doc-deepening-initiative]]). The runnable, non-gated pickup is the **Market Signal Report data-packet enrichment** (parked above — start with Tier-1: economic-calendar consensus+surprise + historical sector/industry P/E), spec'd as its own slice; or keep pressure-testing the Portfolio / Trade Opportunities specs (grade-weights, risk-tier thresholds, archetype rules, funnel budgets — all calibratable-not-pinned). **Merge PR #46** (and fix its title) when ready. Implementation build order unchanged: live-Schwab → full-Portfolio (funds) → Opportunities.
