# Current session handoff

## What happened

Designed and landed (docs-only, additive) a **capital-efficiency / dead-money exit factor** for Portfolio Analysis. From the user's question about selling a loser at a loss for tax benefit + redeployment, the design was deliberately **reframed away from a tax-loss-harvesting engine** (no account-taxability detection, wash-sale rules, lot-level basis, or proxy-swaps — all considered and rejected as too heavy) to a **generic, high-level factor**: the engine derives a **dead-money read** (base-case forward return vs a risk-free+premium hurdle, kept out of the sub-scores); a flagged holding's **standalone lean leans to exit (some or all) on its own merits**; and construction firms that exit with two **generic** counterweights — the **possible tax benefit** of realizing a loss + the **redeployment optionality** of freed cash — weighed high-level, *the model assumes the user acts on the specifics*, and **naming a replacement stock stays Trade Opportunities' isolated job**. Two guardrails keep it from being a license to dump every red position: the loss **never moves the grade/sub-scores** (cost-basis-agnostic), and the lean fires **only once forward prospects are independently judged poor**. Codex round 1: 1 Medium (dead-money read could go **stale after Step-6e target refinement** — real behavioral gap) + 2 Low (roll-up summary drift; sign-blind "unrealized P/L") — all three verified valid and fixed (the read is now declared provisional and **recomputed at 6e** in three spots).

## Current state

**5 files modified, uncommitted** on `main` (HEAD `5650b2b`): `docs/portfolio-analysis.md` (engine layer-a read, standalone lean, new "Capital efficiency and the sunk-cost guard" sub-para, 6e recompute, roll-up summary, audit record), `docs/portfolio-workflow.md` (Steps 6b/6e/7a/7b), `docs/configuration.md` (investor-profile `tax sensitivity` → "no precise modeling" + generic loss-realization factor), plus `.metis/BUILD.md` (action-sizing-spine pillar) and `.metis/INDEX.md` (lines 128/130 + new concept line + line 160 — user-authorized). Docs-only, additive, no code. **Codex round-1 clean** (Medium + 2 Lows fixed and re-verified consistent end-to-end). **Not yet committed; no second Codex round run.**

## Open questions

- **Commit the 5-file change** (docs + `.metis/`) — suggested msg "Docs: Portfolio capital-efficiency / dead-money exit + generic tax-benefit & redeployment factor (Codex round-1 clean)"; optional final Codex round to confirm the 6e-recompute wording reads clean.
- **Cross-job isolation (parked)** — if the job should ever name a *specific* redeploy target (a TO idea) instead of "raise cash + exposure-level rationale," that needs the cross-job-read decision; deliberately not built this session.
- **Implementation-time schemas (build alongside code)** — now also the engine-attached **capital-efficiency / dead-money read** field + **sign-aware unrealized-P/L** handling; joins the prior backlog (since-flagged read; Portfolio thesis ledger / sizing spine / intrinsic-action split / `technology_read`; TO watchlist bar / hypothesis score / event-impact route; hierarchical-distillation knobs).
- **TO 5g bounded-positive option** — still open: a metric-*confirmed* since-flagged gain gives no positive nudge (cap-only); decide later if it should give a bounded positive.
- **BUILD.md compression** — grew again (~5.6k+, ~80 words added this session); ceiling revisit still deferred post-release.
- **Report enrichment** (paid-FMP, four families) — calendar consensus/surprise builder + the valuation-over-time prompt landmine (must be *revised*, not extended).
- **Carried:** local-suite build order (live-Schwab OAuth → full Portfolio → Opportunities, M5-gated); INDEX FMP-audit stragglers; register Schwab dev app; `market_clock` holidays; Cadence Run B; report-side nits (COT extreme-weighting, opus-main leaning, no PDF `@page` margins).

## Where to start

Commit the 5-file capital-efficiency / dead-money change (docs + `.metis/`). Optionally run one more Codex round first. Otherwise the unblocked lead stays the **job-doc deepening initiative** (Report / Portfolio / TO docs) or the parked **TO 5g bounded-positive** decision; implementation items wait on their gates (M5, paid-FMP).
