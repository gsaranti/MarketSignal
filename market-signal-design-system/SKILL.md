---
name: market-signal-design
description: Use this skill to generate well-branded interfaces and assets for Market Signal — a local-first weekly market-report desktop app — either for production or throwaway prototypes/mocks. Contains essential design guidelines, colors, type, fonts, assets, and UI kit components for prototyping in the brand's voice.
user-invocable: true
---

Read the `README.md` file within this skill, and explore the other
available files (`colors_and_type.css`, the `preview/` cards, the
`ui_kits/market_signal_desktop/` kit, and the `assets/` folder).

## When generating visual artifacts (slides, mocks, throwaway prototypes)

1. Copy assets out and create static HTML files for the user to view.
2. Always link `colors_and_type.css` (or inline its tokens) so the
   surface starts on the right foundation. Then design with the tokens —
   never invent off-system colors or radii.
3. The non-negotiables, on every artifact:
   - **Monochrome** warm ink on warm paper. The accent is **oxblood**
     and appears only on interactive states, the current sidebar item,
     focused inputs, and at most one emphasized chart series.
   - **Flat with hairlines.** No shadows, no glass/blur, no gradients,
     no rounded-everything (1–2px radii only, pill capsules prohibited).
   - **Serif body, sans UI.** Source Serif 4 (body + display) and Public
     Sans (chrome). Display moments stay at 28–32px — never a 64px hero.
   - **Direction is sign + position + weight + neutral chevron** — never
     red/green saturation.
   - **No emoji. No drop shadows. No skeleton shimmer. No celebratory
     motion. No "Powered by AI" badges.**

## The analytical register (Portfolio Analysis & Trade Opportunities)

Market Signal now ships two **local analysis features** alongside the
weekly report. They are **structured numeric data**, not prose, and they
live in a second, denser **analytical register** — instrument-grade
(Bloomberg terminal, FT market pages, a research desk's working tools),
never consumer-trading (Robinhood). The register split is the whole idea:
**the report is unchanged** (serif, monochrome, 8px rhythm, 64–72ch
column); the analytical surfaces are technical; shared chrome (sidebar,
run tracker, warning band) ties them together with mono tabular figures
and tracked-caps labels.

When building a Portfolio or Trade Opportunities surface:

- **Use the analytical palette, analytical-register only.** A small
  desaturated, *unified* set serves direction, grades, and conviction:
  muted-green up (`--ana-up`), oxblood down (`--ana-down` = the accent),
  neutral-mid flat (`--ana-flat`); grade scale `--grade-a..f`. All AA in
  both themes. It **never** touches the report or generic chrome, and it
  must **never** read as a trading-app green.
- **IBM Plex Mono is first-class here** — all metrics, grades, deltas,
  targets, and **tickers**. Public Sans carries labels and the
  tracked-caps (11px, +0.05em) column heads. Source Serif 4 stays the
  report's, and the analytical thesis/anchor prose.
- **Density is tighter than the report** — 4px chrome step, hairline rules
  between rows, not padding.
- **Components** (in `colors_and_type.css` + the UI kit): key-figure
  strip, controlled-rich holding card (thesis-anchored), dense data grid
  (the 3×3 matrix + tabular roll-ups), grade chip/scale, directional
  value token, conviction meter, methodology reveal.
- **Two register-only controls** (`.ana-sortbar`, `.ana-viewtoggle`;
  `SortBar`, `ViewToggle` in the kit): a **sort bar** of tracked-caps
  toggle triggers that reorders the Portfolio holdings card stack (each a
  `button` with `aria-pressed` — never `aria-sort`, which stays reserved
  for `.ana-grid` heads; the active key carries the grid's ▾/▴ glyph and
  flips on re-click), and a **matrix/list view toggle** for Trade
  Opportunities (ghost-text on the `.btn-ghost` posture, segmented by
  hairlines without a capsule, 2px radius). Both are confined to the
  Portfolio and Trade Opportunities surfaces plus shared chrome, reuse
  existing tokens and glyphs only, and restyle in place — no new nav.
- **Still flat with hairlines.** No shadow, no pill, no radius > 2px, no
  celebratory framing. A "controlled-rich" data card is the *narrow*
  reopening of the dashboard-widget ban: a hairline card may now carry a
  restrained sparkline (single ink weight, one accent series) + a delta
  + key metrics. Nothing more.
- **Layout & IA do not change** — this was a look-and-feel iteration.
  Restyle in place; don't move panels or invent a new nav model. The
  sidebar is one shared-history component (report issues / Portfolio runs
  / TO runs); the run tracker is one leaveable view, never a modal.

## When working on production code

Treat this folder as the source of truth for the system. Import
`colors_and_type.css`, use the JSX components in `ui_kits/` as a fidelity
reference, and lift exact hex values and spacing tokens from the CSS file
rather than redescribing them.

## When the user invokes this skill without other guidance

Ask them what they want to build or design. Then act as an expert
designer who outputs HTML artifacts or production code, depending on the
need. Suggest concrete next steps (a slide explaining last week's thesis;
a Settings-panel refresh; a new chart figure that lives inside a report)
that exercise the system in the right register.

## What this system rejects on sight

- The consumer-trading-app gestalt (Robinhood-style celebratory P&L,
  saturated greens on pitch black).
- The elevated-SaaS gestalt (purple-to-blue gradient brand, soft-shadow
  rounded cards, geometric sans, command-palette-as-personality).
- The soft-startup reading aesthetic (emoji garnish, hand-drawn
  illustration spots, oversized cover images, "👋 Welcome back").

If a design starts drifting toward any of these, revert.

### Four deliberate relaxations (decisions, not drift)

The analytical register required four scoped extensions to the original
"strictly monochrome + one oxblood accent" rules. Each was a deliberate
decision, recorded here so it never reads as drift. **All four are
confined to the analytical register; the report and generic chrome are
unchanged. The anti-references above remain fully binding** — the green
is desaturated precisely so it never crosses into the Robinhood gestalt.

1. **A desaturated analytical palette.** Was: monochrome + one oxblood.
   Now: a small unified set (muted-green / neutral-mid / oxblood) in the
   analytical register only, AA-validated light + dark.
2. **Directional hue (up/down/flat).** Was: sign + position + weight +
   neutral chevron, no color. Now: the same, plus the desaturated
   directional pair. Still no saturated red/green.
3. **Controlled-rich data cards.** Was: the big-number + sparkline +
   percent-pill "dashboard widget" was banned. Now: narrowly reopened — a
   flat hairline card may carry a restrained sparkline + delta + metrics.
   No shadow, no pill, no radius > 2px.
4. **Grade scale as a desaturated tonal scale.** A–F and conviction map
   across the unified palette as discrete, AA-validated hairline chips —
   never glossy badges.
