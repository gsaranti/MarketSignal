# Current session handoff

## What happened

Shipped four PRs to `main` (now `76e0757`), all verified:
- **Run-tracker progress + liveness** (#38): restored the design kit's determinate
  footer fill + a "Step N of 8" caption + a live elapsed timer; added a
  reduced-motion-gated opacity breathe on the active step marker + in-flight request
  dot ([[design-kit-deviations]] #7).
- **Settings** (#38): fixed the section rule that hugged the Save button; moved the
  Dark-surface toggle into the toolbar so the instant control reads apart from the
  Save-gated form (deviation #8).
- **Demo-run mode** (#39): a `demo-run` Cargo feature that runs the *real* pipeline
  against paced streaming stubs — no keys/network/cost, excluded from `tauri build`.
  GUI-verified end-to-end; documented in `BUILD.md` + the README ([[demo-run-mode]]).
- **Codex review fixes** (#40): the progress fill now credits the in-flight step a
  *half* step (no premature 100% on "Saving the report"); demo mode requires a truthy
  `MARKET_SIGNAL_DEMO`; fixed an elapsed-timer NaN guard + added `JobStatusPanel`
  progress/timer tests.

## Current state

`main` at **`76e0757`**, tree clean, nothing owed. The installed production app stays
in daily use (untouched). **`npm run tauri:demo`** is the cost-free way to verify any
UI/report/tracker change — prefer it over spending FMP/Tavily quota or model tokens.

## Open questions

- **Cadence Run B** — baseline delta-engine + vector-memory recall still need a *real*
  2nd report to exercise live; the demo run uses stubs and doesn't close it
  ([[manual-pivot-cadence-windows]]).
- **opus-main leaning** — accumulating; the worked-examples prompt is an optional carry
  ([[live-config-opus-main-leaning]]).

## Where to start

No code owed — next session likely reacts to live usage; the user's **2nd real report**
closes Cadence Run B (watch the delta + memory-recall paths). Verify any UI/report polish
cost-free with `npm run tauri:demo`. Settled this session (don't re-litigate): design-kit
deviations stay in memory/code/PRs — no repo deviations file — so an external reviewer
(Codex) re-flagging them against the kit is **known-accepted, not a new defect**
([[design-kit-deviations]]). Optional small item: the worked-examples prompt.
