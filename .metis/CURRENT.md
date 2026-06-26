# Current session handoff

## What happened

The **Trade Opportunities job sweep** (docs-only, [[job-doc-deepening-initiative]]), mirroring the Portfolio sweep. Created **`trade-opportunities-workflow.md`** (Type-tagged control flow: gate → shared context → discovery funnel → per-candidate validation loop → per-cell selection → continuity → holdings cross-ref → persist → render) and the **`### Trade Opportunities — endpoint surface`** table in `data-sources.md` (three-band cardinality: discovery / per-candidate / run-level). Two larger calls landed: (1) the **investor profile is now a fixed default preset** — long-term / profit-max / medium-high risk / **cash-unconstrained** / no-tax; user config deferred. (2) A research-grounded **`trade-opportunities.md §The research method`**, built from 4 parallel web-research threads (quant / economist / famous-investor / winner-case-studies): **worldview-first** (house-view regime backbone + forward thematic map), **five lenses** (quant composite / value-creation / macro-thematic / investor-judgment / case-study pattern), **two-track** proven-vs-emerging reconciliation through one moat/management/price gate, a **leading-metric hard gate** + valuation-vs-forward red-flag. Discovery Step 3b restructured (SearXNG-primary defined-topic bounded loops; **Tavily demoted to fallback**). Combed the full FMP reference and **added structured news** (Stock News + Search Stock News + general/press-releases + M&A/8-K event feeds) plus growth-bulk/dcf-bulk, holder-level 13F, financial-growth, SC 13D/13G activist, insider-latest (paths verified). **Codex #1+#2 applied**: archetype-aware TO evidence floor; short-interest reconciled to bearish-default / conditional-squeeze. `BUILD.md` + `INDEX.md` kept current (authorized).

## Current state

All committed + pushed on `docs/local-suite-portfolio-design-decisions` (**PR #46**) — commits `87643c5` (sweep) + `528a7a1` (Codex #1/#2). Working tree clean; no code changed (design docs for the still-planned TO pipeline). **Next session is a cleanup pass**, led by the **FMP endpoint tier audit** — the user manually verifies each endpoint against the actual paid plan (bulk / holder-13F / congressional / transcripts / structured-news / segmentation are the likely higher-tier families); mark each `required`/`useful`/`optional` + fallback in `data-sources.md`.

## Open questions

- **Merge PR #46** — now carries report enrichment + Portfolio sweep + TO sweep + Codex fixes; docs-only, clean.
- **Next-session cleanup (deferred Codex + tier audit):** #8 FMP tier audit (lead); #4 discovery diversity caps (anti-mega-cap quotas by feeder/archetype/cap/theme); #5 undercovered-names operating-reality-vs-price path (estimates thin for small caps); #3 cross-lens contradiction/falsification folded into distillation+scoring (no new model call); #6 more economic value-chain (margin capture / pricing power / capacity); #7 outcome-learning labels (planned calibration).
- **BUILD.md trim** — now ~5.5k tokens, over its ~4.5k ceiling; compress the local-suite section once features lock.
- **Implement report enrichment** (paid-FMP-gated) — calendar consensus/surprise builder + prompt-landmine fix.
- **Implement local suite** — build order: live-Schwab OAuth → full Portfolio (funds) → Opportunities.
- **Live validation hardware-gated** on M5 ([[local-suite-hardware-gated]]) — verdict quality + runtime + FMP-tier + local-model.
- **Carried:** register Schwab developer app; `market_clock` holidays (FMP `holidays-by-exchange`); Cadence Run B (yields vs 2s10s + COT); report-side nits (COT extreme-weighting, opus-main leaning, no PDF `@page` margins).

## Where to start

**Next session: the cleanup pass.** Start with the **FMP endpoint tier audit** (user-driven plan check) — annotate each endpoint in `data-sources.md` `required`/`useful`/`optional` + fallback. Then work the deferred Codex items (#4 diversity caps, #5 undercovered-names path, #3 contradiction check, #6 value-chain, #7 outcome labels). **BUILD.md trim** when convenient. PR #46 is ready to merge whenever.
