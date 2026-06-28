# Market Signal — Desktop UI kit

A high-fidelity recreation of the desktop application's surfaces, built
from the directional language in the brief (no codebase was provided).

## Surfaces

| View | File | Notes |
| --- | --- | --- |
| Latest Report | `LatestReport.jsx` | The loosest surface. One readable column, 8px baseline, serif body, hairline-ruled watchlist, retrospective callout, three-voice stress test, restrained yield chart. |
| Portfolio Analysis | `Portfolio.jsx` | *Analytical register.* Two-step trigger (Pull holdings → Run analysis), key-figure strip, controlled-rich holding cards (full / reduced ETF / not-rated / insufficient-evidence variants, thesis-anchored with graceful overflow), and a whole-book roll-up & construction panel. |
| Trade Opportunities | `TradeOpportunities.jsx` | *Analytical register.* The 3×3 risk×horizon matrix, opportunity cards (directional thesis, prominent leading metric, since-flagged sparkline, honest empty cells), shadow banner, calibration scorecard. |
| Analytical primitives | `Analytical.jsx` | Shared: directional value token, grade chip/scale, conviction meter, key-figure strip, restrained sparkline, methodology + reveal disclosures, card shell. |
| Run tracker | `RunTracker.jsx` | The one leaveable run tracker (not a modal). Per-step / per-holding / per-cell progress, streamed output, cancel; the job lives in the footer and keeps going when you leave. |
| Recent Reports sidebar | `Sidebar.jsx` | The **shared-history** sidebar. Dense rows, hairline separators, 2px accent leading edge on the current item. Content swaps per feature: report issues / Portfolio runs / TO runs. |
| Research Inbox | `ResearchInbox.jsx` | User-supplied PDFs and notes. Dense single-column list. |
| Settings | `Settings.jsx` | The tightest surface. Single-column form, label above field, no decorative grouping cards. |
| Persistent warning area | `WarningBar.jsx` | Always-visible row. No icon, no color flag — the words are the alert. |
| Status row | inside `app.jsx` | Long-running job indicator. Text + 1px bar. No spinner, no celebration. |
| Window chrome | `Window.jsx` | Traffic-light dots, a hairline-bordered titlebar, wordmark centered. No glass, no large radius. |
| Icons | `Icon.jsx` | Outlined, single-weight (1.25px at 20px), squared caps. Twelve icons total — the brief calls for "perhaps a dozen." |

## Anchored to the brief

- **Type pairing** — Source Serif 4 (body + display) + Public Sans (UI).
  Substitution flagged in the root README.
- **Color** — monochrome warm ink on warm paper with a single oxblood
  accent. Used on the focused input, the current sidebar item, and one
  emphasized chart series. Body prose is always ink.
- **Elevation** — flat with hairlines. The only shadow in the kit sits
  on the window itself, used to lift the frame off the page in screenshots;
  no app surface uses elevation.
- **Direction signal** — neutral `▴ / ▾ / ·` chevrons. No red/green
  saturation anywhere in the watchlist.
- **Motion** — state changes 120ms ease-out. View switches are hard cuts.
  No page transitions, no celebrations, no skeleton shimmer.

## To run

Open `index.html` in any browser. React + Babel are loaded from CDN. No
build step.

## What this kit does **not** do (and why)

- **No real LLM calls.** The Settings panel is cosmetic.
- **No actual report generation.** The "Generate now" button toggles the
  status row into its progress state.
- **No persistent storage.** Switching reports updates the title but the
  body is reused; the focus of the kit is visual fidelity, not data.
- **No keyboard handlers.** Production should wire `↑ / ↓` through the
  sidebar and `⌘ ,` to Settings.
