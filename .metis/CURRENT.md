# Current session handoff

## What happened

**Merged v1.1.1 — a content-safe PDF export side-margin fix** (PR #43, squash
`e1d0406`; version 1.1.0 → 1.1.1 across all 5 anchors). **Load-bearing lesson the next
session must not relitigate:** `@page` margins are *unusable* in this webview's
print-to-PDF path — a non-zero `@page` margin shrinks page capacity, and when content
spills onto another page WebKit **silently drops the overflow** instead of paginating it
(demo-verified: a report's trailing Watchlist table + Sources vanished). So `@page`
stays `0` and margins come from `.report-article` **padding** (`--print-page-margin`,
2cm, now a design-system token). The original "wry doesn't honor @page" code comment was
essentially right. Also this session: reviewed report #1 — validated the #41 goals + the
COT group, flagged a curve-number nit (Open questions); the dropped pagination
"keep-together" rules were *not* the cause (unverifiable in this print path → cut).

## Current state

`main` at **`e1d0406`** (v1.1.1), in sync with origin; tree clean except this handoff
file (the `.metis/CURRENT.md` edit is uncommitted). **v1.1.1 is committed but NOT built
or installed** — the running app is still **v1.1.0** and no `v1.1.1` tag is cut, so the
PDF margin fix is not yet in the daily app. The fix is content-safe but carries a WebKit
ceiling: reliable left/right margins on every page + a first-page top, but **interior
pages run to the top/bottom edge** (no per-page vertical margin is possible without
`@page`). Report **#1** is in the store, so report **#2** is the first non-clean-slate
run.

## Open questions

- **Cadence Run B** — report #2 closes the delta-engine + vector-memory-recall check
  (first run with a prior snapshot + summary embedding) ([[manual-pivot-cadence-windows]]).
- **Curve-number consistency** — report #1's header "2Y/10Y both +5bp to 4.24%/4.51%"
  (27bp, flat) vs thesis "10y-2y +7bp to +0.34" (4.51−4.24=0.27≠0.34). Sanity-check yield
  levels vs the 2s10s claim on future reports; recurs → tighten the main-agent prompt
  ([[report-curve-number-consistency]]).
- **PDF vertical margins** — interior pages touch top/bottom (WebKit `@page` ceiling,
  accepted). Revisit only if a better print mechanism appears; do NOT reintroduce `@page`
  margins (drops content).
- **COT calibration** — weighting of positioning extremes deferred to live runs
  ([[skills-forcing-function-only]]).
- **Market holidays / early closes** — `market_clock` still mislabels them "open until
  4pm" (v1 cut; needs an NYSE calendar).
- **opus-main leaning** — accumulating; optional worked-examples carry
  ([[live-config-opus-main-leaning]]).

## Where to start

**Build + install v1.1.1** (deferred to this session by request): frontend gate →
`npm run tauri build` (stamped 1.1.1) → install over v1.1.0 in `/Applications` (same
bundle id → config/keys/reports/memory preserved), then cut the annotated **`v1.1.1`
tag** on the release tip `e1d0406` per the release-tip convention ([[release-build-install]]).
That delivers the PDF margin fix to the daily app. After that, **Cadence Run B**
(report #2) is the next live validation.
