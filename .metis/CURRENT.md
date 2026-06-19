# Current session handoff

## What happened

Ran a full **audit of the report-generation logic** (auto-run removal, cadence soundness at the data + prompt levels, prompt quality, persistence, docs) — found it **largely sound** (auto-run fully removed; cadence & persistence correct; no blocking issues). Landed the four actionable follow-ups as **PR #32 (squash-merged @ `47fb074`)**:
1. **SQLite hygiene** — `storage::open` now sets `busy_timeout=5s` + `journal_mode=WAL`, closing a confirmed defect where a concurrency-*skipped* run could surface a spurious `SQLITE_BUSY` instead of the clean "Skipped".
2. **Cadence-scaled calendar windows** — threaded `ReportCadence` through `MarketDataSource::baseline_scan(cadence)` (mirroring `gather(cadence)`; `compute_cadence` moved above the scan). FMP earnings + FRED economic-release **back**-windows now scale to the elapsed interval, floored at the old default, capped `EARNINGS_BACK_MAX_DAYS=31` / `CALENDAR_BACK_MAX_DAYS=45`; forward windows + the fixed 7-day index horizon unchanged.
3. **Prompt-rigor upgrades** — main agent: analytical-standards (conviction / anti-hedging / quantitative anchoring), always-on falsifiability, injection guard; analysts: distinct per-posture *methods* + a counter-argument forcing function + schema specificity. Forcing-functions stay **prose-only** (Codex's "schema-back the counter-argument" point consciously declined, per the skills decision).
4. **Doc rename + reconcile** — `weekly-report-workflow.md → report-workflow.md` (incl. 4 refs split across line-wrapped doc comments a contiguous grep + the metis review missed — caught by Codex; memory `rename-grep-split-identifiers`).

metis review **approve-with-nits**; 405 lib tests + clippy clean. BUILD.md amended this session (the four changes + the now-fixed doc-path refs).

## Current state

`origin/main` = local `main` = **`47fb074`**, in sync; tree clean; `audit-followups` deleted (local + remote). **No work in flight, no queued slices.**

## Open questions

All **live-run only**, none owes code:
- **Cadence-const calibration** — the research-threshold clamps (`THRESHOLD_SCALE_MIN/MAX`, `THRESHOLD_ANCHOR_DAYS=7`, `research_executor.rs`) **and now the calendar back-window caps** (`EARNINGS_BACK_MAX_DAYS=31` `fmp.rs`, `CALENDAR_BACK_MAX_DAYS=45` `fred.rs`) await tuning vs real daily/weekly/monthly snapshots. Don't re-implement the curves (memory `manual-pivot-cadence-windows`).
- **Empirical prompt/skills calibration** — which of the 16 lenses + this session's new analytical-standards / posture-methods / counter-argument additions actually improve the report, which get ignored, and prose-repetition across them. No test catches prose dilution (memory `skills-forcing-function-only`).
- **Prompt worked-examples** *(deferred)* — the audit's one un-actioned item (a strong-vs-weak risk/thesis exemplar); validate against a live run alongside the calibration above.

## Where to start

No owed code — the whole audit landed. Every remaining item needs a **live end-to-end run**: tune the cadence consts (research thresholds + the new calendar caps), observe whether the prompt-rigor additions improve reports, and judge the skills + worked-examples questions on the same run. Otherwise open a fresh direction.
