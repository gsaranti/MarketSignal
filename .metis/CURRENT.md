# Current session handoff

## What happened

Three changes shipped + squash-merged to `main` this session, all live-verified:
- **Run-tracker footer** (PR #38, `a899efc`): restored the design kit's *determinate*
  progress fill (it had regressed to a flat, never-moving hairline) + a "Step N of 8"
  caption + a live mono elapsed timer; added a reduced-motion-gated opacity **breathe**
  on the active step marker + in-flight request dot — a user-approved override of the
  system's "no idle motion" rule ([[design-kit-deviations]] #7).
- **Settings** (same PR): fixed the section rule that hugged the Save button (gave
  `.settings-actions` the section margin rhythm); **moved the Dark-surface toggle into
  the toolbar** so the instant-applying control reads apart from the Save-gated form
  (deviation #8); promoted Agent models to the lead section.
- **Demo-run mode** (PR #39, `ac81c50`): a `demo-run` Cargo feature that runs the *real*
  pipeline against paced streaming stubs with **no keys/network/cost** (excluded from
  `tauri build`). Driven a GUI run end-to-end to verify ([[demo-run-mode]]). This also
  confirmed the per-posture Bull/Bear/Balanced reasoning panes render correctly.

## Current state

Both PRs merged; `main` at **`ac81c50`**; tree clean, nothing owed. The installed
production app stays in daily use (untouched). **`npm run tauri:demo`** is now the
cost-free way to verify any UI / report / tracker change — prefer it over spending
FMP/Tavily quota or model tokens.

## Open questions

- **Cadence Run B** stays open — the baseline delta-engine + vector-memory recall still
  need a *real* 2nd report to exercise live; the demo run uses stubs and does not close
  this ([[manual-pivot-cadence-windows]]).
- **opus-main leaning** — two strong runs, still accumulating; optional carry is the
  worked-examples prompt enhancement ([[live-config-opus-main-leaning]]).
- Minor doc nit: `CLAUDE.md`'s verification section names only `ResearchDocuments.spec.ts`,
  but `Settings.spec.ts` exists too — flagged, not yet fixed.

## Where to start

No code owed — next session likely reacts to live usage. When the user fires their **2nd
real report**, the cadence delta + memory-recall paths finally exercise live (Run B closes
itself) — watch those. For any UI/report/tracker polish, drive it cost-free with
`npm run tauri:demo` ([[demo-run-mode]]) rather than a real run. Optional small items: the
worked-examples prompt, and the `CLAUDE.md` spec-list doc nit.
