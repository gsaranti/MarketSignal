# Current session handoff

## What happened

Executed the **FMP endpoint tier audit (#8)** against the actual paid plan and handled all its fallout, then merged ([[job-doc-deepening-initiative]]). The audit (now in `data-sources.md` as three buckets) found **all `*-bulk` endpoints off-plan** and **`company-screener` has no fundamental fields**, plus transcripts / 13F-institutional / `etf-holdings`+`funds-disclosure` / press-releases off-plan; the **report is fully on-plan**, **Portfolio equity clean**, **Portfolio funds degrade**. **Trade Opportunities discovery was redesigned**: the screener *stratifies* and the multi-factor composite moves **per-candidate**; a new **model-led hypothesis-research lane** (route planner w/ per-route sources + a mandatory graph-blind **outside-view** route, hypothesis cards, adversarial passes, **hypothesis-score-before-tickers**); a **persisted opportunity graph + carried-forward watchlist** (leading-metric re-check by cost class) that **reverses the earlier stateless re-discovery** — resolving the deferred candidate-backlog choice; **GDELT dropped, SearXNG-only discovery**. **Portfolio fund path degraded** (exposure tilt from ETF weightings; constituent concentration via optional SEC N-PORT; mutual funds hardest) and held-equity off-plan endpoints rerouted (transcripts→web, 13F→EDGAR/omit, per-symbol M&A→latest+8-K). Synced `.metis/BUILD.md` + `INDEX.md`. Reviewed across ~3 Codex rounds per area (all findings verified + applied).

## Current state

**PR #46 squash-merged to `main`** (`3e221b4`); local `main` in sync, working tree clean, feature branch deleted (+ stale remote refs pruned). All docs-only — TO and full Portfolio pipelines remain **planned, no code**. The FMP-audit arc is fully landed and Codex-clean.

## Open questions

- **Decision-discipline initiative (next-session lead)** — Codex's "next big Portfolio upgrade," not more data-sourcing: a per-holding **thesis ledger + bear/base/bull monitor** (falsifiers / triggers / target-weight — the Portfolio analog of TO's key falsifiers), an explicit **action-sizing spine** (grade + conviction + up/downside + risk + weight + concentration + cash/tax), and **intrinsic-verdict-vs-portfolio-action separation** — the last needs a **Step-6→7 feedback** path (per-holding action is currently decided before the roll-up sees the whole book). Plus a small **fund issuer-holdings adapter** note (fresher ETF look-through than N-PORT).
- **BUILD.md trim** — still over the ~4.5k-token compact-brief ceiling (this session added net redesign content).
- **TO implementation-time schemas** — the watchlist bar, hypothesis score, per-route source strategies, and metric re-check classes are spec'd as schemas/rubrics to build alongside the code.
- **Implement report enrichment** (paid-FMP, all four families on-plan) — calendar consensus/surprise builder + the **prompt landmine** (the valuation-over-time instruction must be *revised*, not just extended).
- **Implement local suite** — build order: live-Schwab OAuth → full Portfolio (funds) → Opportunities.
- **Live validation hardware-gated** on M5 ([[local-suite-hardware-gated]]) — verdict quality + runtime + FMP-tier + local-model.
- **Carried:** register Schwab developer app; `market_clock` holidays (FMP `holidays-by-exchange`); Cadence Run B (yields vs 2s10s + COT); report-side nits (COT extreme-weighting, opus-main leaning, no PDF `@page` margins).

## Where to start

**Decision-discipline initiative for Portfolio** — the design pass Codex flagged. Spec the per-holding **thesis ledger + bear/base/bull monitor**, the **action-sizing spine**, and **intrinsic-vs-action separation** (resolve the Step-6→7 sequencing first), plus the fund issuer-holdings note — in `portfolio-analysis.md` / `portfolio-workflow.md`, via the same docs-design → Codex-review loop used this session. Fold the **BUILD.md trim** in opportunistically.
