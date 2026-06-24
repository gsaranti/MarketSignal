# Current session handoff

## What happened

**Built, installed, and tagged v1.1.1 — closing the deferred release task; the PDF
margin fix is now live in the daily app.** Full verification gate green (cargo test +
clippy `--all-targets --all-features`; `npm run build` + 131 unit tests). `npm run tauri
build` produced the 1.1.1 `.app`/`.dmg` (ad-hoc signed), installed over v1.1.0 in
`/Applications` (same bundle id, quarantine cleared via `xattr -cr`). Cut the **annotated
`v1.1.1` tag on release-tip `e1d0406`** — *not* the trailing `metis:` handoff commit, per
the release-tip convention (v1.1.0 was likewise tagged on its release commit `38d0824`,
not its handoff) — and pushed to origin. **Verified no production data was touched:** the
install only wrote `/Applications`; the root data dir (`com.georgesarantinos.market-signal`)
is unchanged — DB + report `.md` mtimes predate the install, contents intact (1 report,
1 baseline snapshot, 1 summary + 2 durable learnings, 9 `app_settings`), no stray
WAL/journal files ([[release-build-install]]).

## Current state

`main` at **`50d6598`** (the v1.1.1 metis-handoff commit), in sync with origin; tree clean
except this `CURRENT.md` edit. The **daily app is now v1.1.1**; the `v1.1.1` tag sits on
`e1d0406` (one commit behind HEAD, by convention). Data dir preserved at the root path
(config/keys/reports/memory carried over — gate stays green, zero clicks). Report **#1** is
in the store, so report **#2** is the first non-clean-slate run. Nothing in flight.

## Open questions

- **Cadence Run B** — report #2 closes the delta-engine + vector-memory-recall check
  (first run with a prior snapshot + summary embedding) ([[manual-pivot-cadence-windows]]).
- **Curve-number consistency** — report #1's header "2Y/10Y both +5bp to 4.24%/4.51%"
  (27bp, flat) vs thesis "10y-2y +7bp to +0.34". Sanity-check yield levels vs the 2s10s
  claim on future reports; recurs → tighten the main-agent prompt
  ([[report-curve-number-consistency]]).
- **PDF vertical margins** — interior pages touch top/bottom (WebKit `@page` ceiling,
  accepted). Do NOT reintroduce `@page` margins (drops content).
- **COT calibration** — weighting of positioning extremes deferred to live runs
  ([[skills-forcing-function-only]]).
- **Market holidays / early closes** — `market_clock` still mislabels them "open until
  4pm" (v1 cut; needs an NYSE calendar).
- **opus-main leaning** — accumulating; optional worked-examples carry
  ([[live-config-opus-main-leaning]]).

## Where to start

**Cadence Run B** — generate report **#2** on the now-live v1.1.1 build: the first run with
a prior snapshot + summary embedding, validating the delta engine + research-informed
vector-memory recall. On the resulting report, sanity-check the yield levels against the
2s10s claim (the curve-number nit) and eyeball the COT positioning read.
