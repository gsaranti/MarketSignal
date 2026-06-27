# Current session handoff

## What happened

Designed and landed (docs-only, additive) **per-opportunity performance tracking since first-surfaced** for Trade Opportunities. The discussion established that the existing Step-7 outcome labels *already* track from first-surfaced — but only as discrete **matured-window** snapshots (1/3/6/12mo return vs sector/market, drawdown, metric-continuation, failure mode), populated only as each window elapses. Added a **continuous since-flagged read** (running return vs sector/market + max drawdown + metric-continuation from the first-surfaced price) framed as **one engine primitive with two reads** — matured-window labels → calibration; continuous read → display + scoring — and **three readers**: horizons feed calibration; the continuous read renders **inline in the matrix** beside each carried-forward idea; and it feeds the **Step-5g re-score of a carried-forward name as reflexivity-disciplined context** (an unconfirmed gain caps conviction, a drawdown with metric intact = improved asymmetry — **never a momentum boost**). Stateless (reconstructed from Stooq off the carry-forward identity, no stored snapshots); live from the idea's **first *subsequent* run, not debut**. One Codex round (`iris-codex-last.md`): no high-sev; 1 Medium (storage.md contract stale) + 2 Low (first-run wording; INDEX/data-sources behind) — all verified against the text and fixed.

## Current state

6 files modified, **uncommitted** on `main` (HEAD `79cb6fa`): `docs/trade-opportunities.md` (§Outcome learning, schema, §Storage and display), `trade-opportunities-workflow.md` (Steps 5c/5g/7/9 + intro), `storage.md` (matrix + audit contract), `data-sources.md` (Stooq benchmark row), plus `.metis/INDEX.md` (line 158) and `.metis/BUILD.md` (TO bullet — user-authorized in-the-moment). All docs-only, additive, **dormant for new ideas** (the read exists only for carried-forward names). Consistent end-to-end. **Not yet committed; no second Codex round run.**

## Open questions

- **Commit this session's six-file change** + optional final Codex round to confirm the storage-contract Medium is closed.
- **5g bounded-positive option** — the since-flagged read is currently strictly *cap-only*: an unconfirmed gain caps conviction, but a metric-*confirmed* gain gives no positive nudge (confirmation flows via the leading-metric anchor instead). Decide later whether a confirmed gain should give a bounded positive contribution.
- **Implementation-time schemas (build alongside code)** — now also the engine-attached **since-flagged read** field (per carried-forward opportunity); plus prior list: Portfolio thesis ledger / sizing spine / intrinsic-action split / technology-event topic+falsifier; TO watchlist bar / hypothesis score / per-route source strategies / metric re-check classes / `technology_read` / event-impact route+materiality gate / feared-loser pass; hierarchical-distillation knobs + heavy-route classification.
- **BUILD.md compression** — now slightly larger (~5.5k+ tokens); ceiling revisit deferred post-release.
- **Report enrichment** (paid-FMP, four families) — calendar consensus/surprise builder + the prompt landmine (valuation-over-time instruction must be *revised*, not extended).
- **Local suite build order** — live-Schwab OAuth → full Portfolio (funds) → Opportunities; live validation hardware-gated on M5 ([[local-suite-hardware-gated]]).
- **Carried:** INDEX FMP-audit stragglers; register Schwab developer app; `market_clock` holidays; Cadence Run B (yields vs 2s10s + COT); report-side nits (COT extreme-weighting, opus-main leaning, no PDF `@page` margins).

## Where to start

Commit the six-file since-flagged-read change (docs-only). Optionally run one more Codex round to confirm the storage-contract Medium closed. Otherwise the unblocked lead stays design-ahead schema work (now including the since-flagged read field); implementation items wait on their gates (M5, paid-FMP).
