# Market Signal — Design System

A design system for **Market Signal**, a local‑first desktop application
(Tauri + Vue). Its core is a single long‑form market report, generated on
demand at the user's chosen cadence, written by a multi‑agent pipeline that behaves like a
professional research desk — a Head Market Analyst that synthesizes, three
internal analyst voices (Bull, Bear, Balanced) that stress‑test the thesis,
and a retrospective audit that holds prior reports accountable. Alongside the
report it now ships two **local analysis features** — **Portfolio Analysis**
and **Trade Opportunities** — which are deliberately **prescriptive** and
live in a denser **analytical register** (see
*The analytical palette* and the analytical previews / UI‑kit surfaces).

The audience is the **serious individual investor or independent analyst**
who wants structural, thesis‑driven coverage. The **report** stays
reading‑shaped — structural, not real‑time tickers or daily P&L — and feels
less like a fintech app than a private research bulletin that lives on your
machine; the analytical features apply that same house view to the user's own
positions and to new ideas, in an **instrument‑grade** (never
consumer‑trading) register.

## Product surfaces

Market Signal is narrow on purpose. The report surfaces are reading‑shaped;
the two local‑analysis surfaces (Portfolio, Trade Opportunities) are denser
and live in the analytical register:

- **Latest Report** — the most recent issue, rendered from Markdown into
  HTML. The loosest, most generous surface in the system.
- **Portfolio Analysis** — *(analytical register)* grades the user's
  holdings (A–F + sub-scores, conviction, and price targets where the
  vehicle class is priceable; an unpriceable fund renders a typed
  role/risk card branch instead — no letter, targets, or conviction),
  a standing per-holding thesis, and a whole-book roll-up. Structured
  numeric data, not prose. Holdings shown as cards; a roll-up &
  construction panel below.
- **Trade Opportunities** — *(analytical register)* discovers ideas across
  a 3×3 risk×horizon matrix, with a leading operating metric, a
  since-flagged performance read, a calibration scorecard (its product
  display surface is still a deferred decision), and a watchlist.
- **Recent Reports** — a sidebar listing the last thirty issues; dense,
  hairline‑separated rows. The **same shared-history sidebar** now also
  lists recent Portfolio runs and Trade Opportunities runs (content swaps
  per feature; same density, same oxblood leading-edge accent).
- **Research Inbox / Archive** — user‑supplied PDFs and notes, organized
  for later citation.
- **Persistent Warning Area** — a small, always‑visible row for active
  caveats and revisions.
- **Settings** — model choice and API credentials. A single‑column form;
  the tightest surface in the system.

## What makes it distinctive

- **A deliberate issue cadence as an act of restraint.** Not a feed.
- **Theses that evolve across issues** rather than reset each issue.
- **Explicit retrospective auditing** — last month's call gets graded
  this month.
- **A single unified analyst voice** rather than a feed of widgets.

## Sources

This bundle is **integrated into the Market Signal codebase** (Tauri + Vue) as
the source of truth for UI: `colors_and_type.css` is imported globally by the
app (`src/main.ts`), and the `ui_kits/` JSX is the fidelity reference the Vue
components are built against.

| Source | Status | Notes |
| --- | --- | --- |
| Codebase (Tauri + Vue) | integrated | this repository; `colors_and_type.css` imported in `src/main.ts` |
| Figma | not provided | — |
| Brand assets / logo files | drawn from system | wordmark + ornament in `assets/`, drawn from the type system |

---

## Index

The files at the root of this system:

- `README.md` — this document
- `colors_and_type.css` — design tokens (colors, type, spacing, hairlines, motion) + semantic element styles
- `SKILL.md` — agent‑skill entry point for using this system in Claude Code
- `fonts/` — font‑loading notes (Google Fonts via @import; substitutions flagged below)
- `assets/` — wordmark, ornament, icon notes
- `preview/` — Design System tab cards (colors, type, spacing, components, brand, **analytical**)
- `components/` — **exported components** consuming projects can import from
  `window.MarketSignalDesignSystem_5eede4`: `GradeChip`, `DirectionalValue`,
  `KeyFigureStrip` (each a `.jsx` + `.d.ts` + a live `@dsCard` demo). These
  are the analytical-register primitives promoted to first-class, reusable
  components.
- `ui_kits/market_signal_desktop/` — the desktop application UI kit
  (`index.html` + JSX components, including the **Portfolio**, **Trade
  Opportunities**, shared analytical primitives, and run-tracker surfaces)

---

## c. Content fundamentals

The product writes the way a senior analyst writes a note to their
own desk: **measured, plain‑spoken, accountable, structural, unhurried.**

### Voice

- **Declarative.** Say the thing. Subject, verb, claim.
  *"The thesis is unchanged since the last issue."* — not *"We're continuing to monitor…"*
- **Specific.** Numbers belong to the prose, not to widgets.
  *"Energy is up 3.2% on the week; the underlying logic still holds, but
  the timing was wrong."*
- **Willing to say what isn't known.** Hedging through omission is worse
  than naming uncertainty.
  *"We don't know whether the move in rates is regime change or noise."*
- **Accountable.** Prior calls are quoted and graded.
  *"Last month's call on energy looks early."*
- **Conditional commitments.** When confident, commit; when uncertain,
  name the revision conditions.
  *"Two things would force a revision: a sustained breach of 4,400, or a
  clear inflection in core services inflation."*

### Casing & person

- **Sentence case** for headings, labels, buttons, and menu items. Title
  Case only on the report's own title line. No ALL CAPS except in 11px
  caption labels with restrained tracking.
- **First‑person plural ("we")** in the report prose — the analyst desk
  voice. **Second‑person ("you")** for product chrome that addresses the
  reader directly ("You haven't opened the latest issue yet.").
- **Never first‑person singular** in the reading surface. The product is
  a desk, not a person.

### Tone — phrases the product would use

> "The thesis is unchanged since the last issue."
> "Last month's call on energy looks early; the underlying logic still holds, but the timing was wrong."
> "Two things would force a revision: a sustained breach of [X], or a clear inflection in [Y]."
> "We are not confident in this read. The conditions for revision are below."

### Tone — phrases the product would **never** use

> ~~"Crushing it 🚀"~~
> ~~"Smart money is positioning for…"~~
> ~~"Don't miss this week's must‑read insights"~~
> ~~"Powered by AI"~~
> ~~"Buy / Sell / Hold"~~ — the *report* gives no trade calls; the local suite's prescriptive actions use the analytical register's structured verdict vocabulary, never promotional phrasing like this.

### Empty‑state and status copy

Plain prose, no decoration. No emoji. No exclamation marks.

- Empty inbox → *"No research has been filed yet. Drag a PDF here to add it to the archive."*
- Generating → *"Generating the new issue. Started 06:12 ET. Estimated 24 minutes remaining."*
- Generation complete → *"The new issue is ready."* (no toast theatrics, no celebration)
- Warning → *"Last month's energy call was early. See the retrospective in §4."*

### Emoji

**None, anywhere in the product.** Not in sidebar labels, not in empty
states, not in warning messages. The product can spell.

---

## d. Visual foundations

### Density and rhythm

The rhythm is that of a **well‑set print broadsheet quietly translated to
a screen.** Whitespace earns its place by serving the eye's transition
between sections or the reader's pause between ideas — it is never
decorative.

- **Latest Report** is the loosest surface. Single readable column,
  **64–72 characters wide**, generous margins, paragraph spacing on a
  vertical baseline grid (8px). Charts inset with the same restraint as
  figures in a journal article.
- **Recent Reports sidebar** and the **Watchlist** within reports are
  denser: tight row rhythm, **hairline rules between rows** (not padding),
  monospaced/tabular numerals so columns of figures align without effort.
- **Settings** is the tightest surface: single‑column form, label above
  field, no decorative grouping cards.

Nothing on the screen has the airy *"look how minimal we are"* emptiness
of a marketing page.

### Type

A deliberate pairing of two families, used with discipline.

- **Serif (body & report prose, plus the rare display moment) — `Source Serif 4`.**
  A contemporary humanist book face — open apertures, modest contrast,
  designed for long‑form reading. Set at 17px / 1.55 in the report body
  with letter‑spacing tightened to **−0.006em**. Display moments (report
  title, date line) set in the same family at **28–32px**, never larger.
- **Sans (UI chrome, labels, captions, numerals, tabular data) — `Public Sans`.**
  A neutral humanist sans with modest x‑height, true tabular figures, and
  weights 400/500/600. UI chrome runs at 13–14px at default tracking;
  11px caption/label sizes open to **+0.04em–+0.06em**.
- **Mono (optional, for dense numeric tables only) — `IBM Plex Mono`.**

> ⚠️ **Substitution flag.** No font files were provided. `Source Serif 4`
> and `Public Sans` are Google Fonts choices that match the brief's
> description point‑for‑point (humanist serif text face for long reading;
> neutral humanist sans with modest x‑height, restrained ink‑traps, true
> tabular figures, weights 400/500/600). If Market Signal has a licensed
> pairing in mind, please drop `.woff2` / `.ttf` files into `fonts/` and
> we'll swap the `@import` for a local `@font-face` block.

Letter‑spacing posture, summarized:

| Size band | Family | Tracking |
| --- | --- | --- |
| Report body (16–18px) | serif | **−0.006em** (tight) |
| Display (28–32px) | serif | 0 (neutral) |
| UI chrome (13–14px) | sans | 0 (neutral) |
| Caption / label (11px) | sans | **+0.05em** (opened) |
| Anything else large | either | **0**, never opened |

### Color

The system is **monochrome — warm ink on warm paper — with a single accent
used sparingly and structurally.**

Light surfaces:

| Token | Hex | Role |
| --- | --- | --- |
| `--paper` | `#F4EFE4` | primary surface (warm off‑white, premium uncoated stock) |
| `--paper-soft` | `#ECE6D5` | hovered row / selected sidebar item background |
| `--paper-edge` | `#E6DFCC` | the next tonal step down, for inset wells |
| `--ink` | `#1F1A14` | body text — a near‑black with a touch of brown |
| `--ink-2` | `#4A4238` | secondary text |
| `--ink-3` | `#7A6F5F` | tertiary text, captions |
| `--hairline` | `#C8BFAE` | section dividers, table rules, input borders |
| `--accent` | `#6E2230` | oxblood — used **only** for interactive states, current sidebar item, and one emphasized chart series |
| `--accent-press` | `#581923` | accent darkened by one tonal step |

Dark surfaces (graphite, not pure black):

| Token | Hex | Role |
| --- | --- | --- |
| `--paper-dk` | `#1B1814` | primary surface (warm graphite) |
| `--paper-soft-dk` | `#252119` | hovered row in dark mode |
| `--ink-dk` | `#ECE5D2` | body text in dark mode (warm near‑white) |
| `--ink-2-dk` | `#B5AC97` | secondary text |
| `--ink-3-dk` | `#7E7560` | tertiary text |
| `--hairline-dk` | `#3A342A` | dividers and rules |
| `--accent-dk` | `#B0596A` | the same oxblood, lifted for legibility on graphite — **still desaturated**, never neon |

**Where the accent appears:** primary button fill, link‑on‑hover
underline, focused input border, the **leading‑edge rule on the current
sidebar item** (not a fill), and one narrow band of chart ink when a
series needs emphasis.

**Where the accent is deliberately absent:** body prose (always ink),
icons (stroked in ink), section dividers (hairlines), card backgrounds
(surfaces stay paper‑toned; differentiation is by hairline, not fill).
In the **report and generic chrome**, up/down market signal stays
**monochrome** (sign + position + weight + a neutral chevron). Direction
gains hue **only in the analytical register** (see below).

### The analytical palette (Portfolio & Trade Opportunities only)

A small, **desaturated, unified** palette confined to the analytical
register. The *same* tokens serve all three uses — direction, the grade
scale, and conviction — so the register reads as one system. It is
desaturated to match oxblood's restraint and **must never read as a
trading-app green.** Never used in the report or generic chrome.

Directional pair + neutral mid:

| Token | Light | Dark | AA (light · dark) |
| --- | --- | --- | --- |
| `--ana-up` (gain / above / top grades) | `#2E6049` muted pine | `#6FA98A` sage | 6.35 · 6.50 |
| `--ana-down` (loss / below / low grades) | `#6E2230` oxblood | `#B0596A` oxblood-dk | 9.47 · 3.78\* |
| `--ana-flat` / `--ana-mid` (unchanged) | `#6F6657` | `#9C9078` | 4.93 · 5.62 |

Grade scale — five discrete tonal steps (green → neutral → oxblood), each
an AA-validated `--grade-{a..f}-tx` text on a faint `--grade-{a..f}-bg`
tint. A grade is a **hairline/flat chip** (2px radius, mono letter),
never a glossy badge.

> All pairings are WCAG AA as text on their surface (5.3–9.5 light). The
> one exception, dark-mode oxblood at 3.78\:1, **matches the system's
> existing `--accent-dk`** and meets AA at the bold/large sizes the
> register uses for grades and deltas.

### Four deliberate relaxations (decisions, not drift)

The analytical register required four scoped extensions to the original
"strictly monochrome + one oxblood accent" rules. Recorded here so they
read as decisions. All four are **analytical-register only**; the report
and generic chrome are unchanged, and the anti-references below remain
fully binding.

1. **Desaturated analytical palette** (above) — replaces "monochrome + one
   accent" inside the analytical register only.
2. **Directional hue (up/down/flat)** — adds the desaturated pair to the
   existing sign + position + weight + chevron treatment. Still no
   saturated red/green.
3. **Controlled-rich data cards** — the "big-number + sparkline +
   percent-pill" dashboard widget, previously banned, is narrowly
   reopened: a flat hairline card may carry a restrained sparkline
   (single ink weight, one accent series) + a delta + metrics. No shadow,
   no pill, no radius > 2px.
4. **Grade scale as a desaturated tonal scale** — A–F and conviction map
   across the unified palette as discrete, AA-validated hairline chips.

### Analytical-register controls (Portfolio & Trade Opportunities only)

Two controls live in the register alongside the data primitives, confined
to the Portfolio and Trade Opportunities surfaces plus shared chrome —
never the report's reading register or generic chrome. Both are additive,
reuse existing tokens and glyphs, and restyle in place (no new nav model).

- **Sort bar** (`.ana-sortbar` / `SortBar`) — a horizontal toolbar of
  tracked-caps toggle triggers that reorders the Portfolio holdings
  **card stack**. It is not a table: each trigger is a `button` with
  `aria-pressed` for the active key, and it never carries `aria-sort`
  (reserved for `.ana-grid` heads). The active key shows the exact ▾
  (descending) / ▴ (ascending) glyph the grid heads and `.dir` already
  use; inactive keys a dimmed ▾ at `th.sortable`'s reduced opacity.
  Clicking the active key flips direction.
- **Matrix / list view toggle** (`.ana-viewtoggle` / `ViewToggle`) — a
  minimal two-option switch (Matrix · List) flipping the Trade
  Opportunities surface between a card matrix and a flat list. Ghost-text
  built on the `.btn-ghost` posture; segmented by hairlines **without a
  capsule** (radius ≤ 2px, no pill); the active view marked via
  `aria-pressed`. Restrained — not a tab bar.
- **Status tag** (`.ana-tag`) — a quiet, tracked-caps informational marker
  riding a card or grid row (the Portfolio holdings-churn tags *new · not
  in last analysis* / *no longer held*; the neutral base the TO lifecycle
  badges will extend). The words are the alert: ink tokens only, hairline
  border, radius ≤ 2px, never a pill, and **never the accent** (reserved
  for actionable states).

### Spacing & baseline

An **8px vertical baseline grid** governs all paragraph rhythm in the
reading surface. UI chrome uses a smaller 4px step for hit‑target spacing.

Scale: `2 · 4 · 8 · 12 · 16 · 20 · 24 · 32 · 40 · 56 · 72 · 96`. The big
numbers are reserved for **inter‑section** breathing in the Latest Report;
chrome stays in the 4–24 range.

### Backgrounds

Flat tones. **No** background imagery, photography, illustration spots,
patterns, textures, mascots, geometric brand shapes, gradients, or
glass/blur layers. The only graphical content in the product is chart
imagery generated from real data, rendered with the same restraint as a
journal figure.

### Borders, corners, elevation

- **Corner radii.** `1px–2px` everywhere — **just enough to avoid hard
  pixel corners.** Buttons, inputs, cards, focus rings: all 2px max.
  Pill / capsule shapes are **prohibited**. Any radius ≥ 4px is a
  violation.
- **Borders.** A single 1px hairline in `--hairline`. No double rules, no
  inset/outset borders, no colored borders except the **2px accent
  leading‑edge** on the current sidebar item.
- **Elevation.** The system is **flat with hairlines.** There are no drop
  shadows, no inner shadows, no z‑axis layers, no glass, no blur. Cards
  exist only as a hairline rectangle — never a fill change, never a shadow.

### Hover & press states

- **Hover** shifts state through a faint **paper‑tone darkening** (~2%
  toward `--paper-soft`) or a hairline color change. A hovered link gains
  an **underline**, never a color change. A hovered primary button
  darkens by one tonal step. **Never scale, never shadow, never
  saturation flares.**
- **Press** darkens one further tonal step. Buttons do not shrink, do not
  emboss, do not glow.
- **Focus** is the only place where the accent appears as an outline: a
  **2px `--accent` outline at 1px offset.**

### Motion

**Restrained throughout. Motion exists to confirm a state change happened,
not to entertain.**

- State changes (focus, button press, sidebar selection): **120ms,
  ease‑out.** Quietly perceptible.
- Navigation between views: **0ms.** A hard cut.
- The longest acceptable transition is the appearance of a newly
  generated report after a report job completes: a single **~200ms
  fade‑and‑settle** as the new entry slides into the sidebar. Then it
  stops.
- **No** number‑counter spin‑ups, **no** shimmer placeholders during
  loading, **no** confetti, **no** toast theatrics, **no**
  scroll‑triggered reveals, **no** parallax. Long‑running jobs (the
  report can take ~30 minutes) show a steady, undecorated status row —
  text and a single 1px progress indicator.

### Layout rules

- **Left‑aligned hierarchy throughout.** No centered hero blocks. The
  home view is not a landing page.
- **No oversized headlines.** Report titles set at 28–32px in the serif;
  marketing‑page typography does not appear inside the product.
- **No "dashboard widget" cards** — the genre of a card containing one
  large number, a sparkline, and a percent‑change pill. In the **report and
  generic chrome**, numerical context lives inside the prose and its embedded
  figures. *(Narrowly reopened for the **analytical register only** — see the
  controlled‑rich data card, relaxation #3 under "Four deliberate
  relaxations.")*

### Transparency & blur

None. The product has no background imagery to overlay in the first
place, and the system rejects glass / frosted / translucent treatments
outright.

### Cards, summarized

A card is **a hairline rectangle on the paper surface.** That is the
whole spec. No fill change, no shadow, no rounded corners above 2px, no
gradient. A "card" in this system is closer to a journal figure box than
to a Material card.

---

## e. Iconography

Icons are **outlined, single‑weight, hairline (1.25px stroke at 16px
size), sharing the typographic line weight of the body sans.** When
placed next to a 14px label, the icon's stroke and the text's stem should
appear to be drawn with the same pen.

**Rules — what icons are:**

- One stroke weight, one color (the text color, never the accent).
- Square or near‑square caps; **no rounded line‑caps inside icons.**
- No fills, no two‑tone treatments, no brand‑colored fills, no gradients.
- A **small set by design** — perhaps a dozen total: settings, archive,
  inbox, export, warning, success/check, chevron, search, plus a few for
  table/chart affordances.

**Source.** No icon set was provided in the brief. We've adopted
**[Lucide](https://lucide.dev)** as the working substitute — it ships
outlined single‑weight icons with squared caps and a 2px native stroke
that downscales cleanly to the 1.25px target at 16px. Lucide is loaded
from CDN in UI‑kit screens.

> ⚠️ **Substitution flag.** Lucide is the closest CDN match to the brief's
> rules. If the Market Signal codebase has its own icon font / SVG sprite,
> drop it into `assets/icons/` and the UI kit will be re‑pointed.

**Emoji.** Never used. Not in labels, empty states, or warnings.

**Unicode glyphs.** Used sparingly and only where the typographic system
already accepts them — section ornament (`✻`), bullet marker for the
masthead date line (`·`), and the small neutral chevron (`▸`, `▴`, `▾`)
for up/down direction in tables.

**Logos / wordmark.** A serif wordmark drawn from the type system itself
(see `assets/wordmark.svg`) and a small typographic ornament for section
breaks within reports (`assets/ornament.svg`). No symbol mark, no
geometric monogram, no abstract brand shape.

---

## Anti‑references (binding)

The system explicitly rejects the gestalt of these three categories. If a
design decision starts to drift toward any of them, revert.

1. **Robinhood and the consumer‑trading gestalt.** No celebratory P&L,
   no gamification, no saturated greens on pitch black, no
   up‑is‑good‑on‑a‑daily‑cadence framing. Market Signal is the
   philosophical opposite.
2. **Linear / the elevated‑SaaS gestalt.** No purple‑to‑blue brand
   gradient, no near‑black surfaces with cool accent glows, no geometric
   sans everywhere, no soft‑shadow rounded cards,
   no command‑palette‑as‑personality.
3. **Notion / Substack / the soft‑startup reading aesthetic.** No
   friendly emoji garnish, no hand‑drawn illustration spots, no oversized
   cover images, no pastel accent surfaces, no "👋 Welcome back" empty
   states.
