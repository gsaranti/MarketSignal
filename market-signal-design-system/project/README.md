# Market Signal — Design System

A design system for **Market Signal**, a local‑first desktop application
(Tauri + Vue) that publishes a single long‑form market report every Sunday
morning. The report is written by a multi‑agent pipeline that behaves like a
professional research desk — a Head Market Analyst that synthesizes, three
internal analyst voices (Bull, Bear, Balanced) that stress‑test the thesis,
and a retrospective audit that holds prior reports accountable.

The audience is the **serious individual investor or independent analyst**
who wants structural, thesis‑driven coverage — not real‑time tickers, not
daily P&L, not trade signals. The product should feel less like a fintech
app and more like a private research bulletin that happens to live on your
machine.

## Product surfaces

Market Signal is narrow on purpose. The surfaces are reading‑shaped:

- **Latest Report** — the current Sunday issue, rendered from Markdown into
  HTML. The loosest, most generous surface in the system.
- **Recent Reports** — a sidebar listing the last thirty issues; dense,
  hairline‑separated rows.
- **Research Inbox / Archive** — user‑supplied PDFs and notes, organized
  for later citation.
- **Persistent Warning Area** — an always‑visible **status band** for active
  caveats and config gaps. Rendered in the chrome register — **sans** body on a
  `--paper-edge` inset‑well, an oxblood **"Needs attention"** header, one grouped
  block with no inter‑row hairlines — so it can never be mistaken for report
  prose. (The earlier serif‑italic "words are the alert" treatment was: it read
  as report content on the same serif reading surface.)
- **Settings** — model choice and API credentials. A single‑column form;
  the tightest surface in the system.

## What makes it distinctive

- **Weekly cadence as an act of restraint.** Not a feed.
- **Theses that evolve across issues** rather than reset each week.
- **Explicit retrospective auditing** — last month's call gets graded
  this month.
- **A single unified analyst voice** rather than a feed of widgets.

## Sources

No external assets, codebase, or Figma file were attached to this brief.
The system is built from the directional language in the brief itself, as
the brief explicitly instructs. Should a codebase or Figma later become
available, this README is the place to document the link.

| Source | Status | Notes |
| --- | --- | --- |
| Codebase (Tauri + Vue) | not provided | request from user when available |
| Figma | not provided | request from user when available |
| Brand assets / logo files | not provided | wordmark and ornament drawn from directional language |

---

## Index

The files at the root of this system:

- `README.md` — this document
- `colors_and_type.css` — design tokens (colors, type, spacing, hairlines, motion) + semantic element styles
- `SKILL.md` — agent‑skill entry point for using this system in Claude Code
- `fonts/` — font‑loading notes (Google Fonts via @import; substitutions flagged below)
- `assets/` — wordmark, ornament, icon notes
- `preview/` — Design System tab cards (colors, type, spacing, components, brand)
- `ui_kits/market_signal_desktop/` — the desktop application UI kit
  (`index.html` + JSX components)

---

## c. Content fundamentals

The product writes the way a senior analyst writes the Sunday note to their
own desk: **measured, plain‑spoken, accountable, structural, unhurried.**

### Voice

- **Declarative.** Say the thing. Subject, verb, claim.
  *"The thesis is unchanged this week."* — not *"We're continuing to monitor…"*
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
  reader directly ("You haven't opened this week's issue yet.").
- **Never first‑person singular** in the reading surface. The product is
  a desk, not a person.

### Tone — phrases the product would use

> "The thesis is unchanged this week."
> "Last month's call on energy looks early; the underlying logic still holds, but the timing was wrong."
> "Two things would force a revision: a sustained breach of [X], or a clear inflection in [Y]."
> "We are not confident in this read. The conditions for revision are below."

### Tone — phrases the product would **never** use

> ~~"Crushing it 🚀"~~
> ~~"Smart money is positioning for…"~~
> ~~"Don't miss this week's must‑read insights"~~
> ~~"Powered by AI"~~
> ~~"Buy / Sell / Hold"~~ — the product explicitly does not give trade calls.

### Empty‑state and status copy

Plain prose, no decoration. No emoji. No exclamation marks.

- Empty inbox → *"No documents yet. Use 'Add files…' to open the inbox folder and add some."* (there is no in‑window drag‑drop; the button reveals the folder the user drops files into)
- Generating → *"Generating this week's issue. Started 06:12 ET. Estimated 24 minutes remaining."*
- Generation complete → *"This week's issue is ready."* (no toast theatrics, no celebration)
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
| `--paper-soft` | `#ECE6D5` | hovered row / selected sidebar item; **recessed chrome regions** (sidebar, footer) |
| `--paper-edge` | `#E6DFCC` | next tonal step down — inset wells, the **warning band**, and hover/selected rows on a tinted region |
| `--ink` | `#1F1A14` | body text — a near‑black with a touch of brown |
| `--ink-2` | `#4A4238` | secondary text (and **body‑size secondary prose** — see Contrast) |
| `--ink-3` | `#7A6F5F` | tertiary text, captions — **large/caption sizes only** (see Contrast) |
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
icons (stroked in ink), section dividers (hairlines), **up/down market
signal** (direction is sign + position + weight + a neutral chevron, never
saturated red/green), card backgrounds and the reading column (these stay
paper‑toned; differentiation is by hairline, not fill — but see **Region
grounding**, which does tint the structural chrome *zones*).

**Region grounding.** The structural chrome *regions* carry one flat tonal
step so the eye can find their edges without leaning on the hairline alone: the
**report/reading surface stays `--paper`** (the lightest — the hero), the
**sidebar and footer sit on `--paper-soft`**, and the **warning band sits on
`--paper-edge`**. Within a tinted region, hover/selected rows step one further
to `--paper-edge`. This is still "flat with hairlines" — a tonal fill, never a
shadow, gradient, or glass — and it stops at regions: individual **cards and the
reading column never get a fill.**

**Contrast (WCAG AA).** `--ink-3` (#7A6F5F) on `--paper` is ≈ **4.3:1** — below
the 4.5:1 floor for normal text. Use `--ink-3` only for **large or caption**
text (eyebrows, metadata, dates); for **body‑size secondary prose** (empty‑state
bodies, form hints, section notes) use **`--ink-2`** (#4A4238, ≈ 8:1). The same
asymmetry holds in dark mode.

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
  generated report after a Sunday job completes: a single **~200ms
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
- **Per‑surface titles.** Serif display is reserved for the report (its own
  title line). Config and list surfaces — **Settings, Research Inbox** — are
  named once by their toolbar eyebrow and carry **no separate serif H2**. That
  surface‑title eyebrow is set one step **above** the section sub‑heading eyebrows
  it governs — **13px, ink, semibold** vs. the 11px ink caption used for
  in‑surface section headings (e.g. "Scheduled job") — so the surface name reads
  as the dominant label, not weaker than its own sections. The same 13px
  semibold treatment carries the persistent warning band's oxblood "Needs
  attention" header. (Extension noted: a 13px tracked‑uppercase label tier on
  top of the 11px caption.)
- **Window minimum.** The layout holds from a **~720×480** floor upward (the
  app window's `minWidth`/`minHeight`); below that the fixed‑width sidebar would
  crush the reading column, so the window is not allowed to shrink past it. The
  reading measure still caps on the wide end (64–72ch).
- **No "dashboard widget" cards** — the genre of a card containing one
  large number, a sparkline, and a percent‑change pill. Numerical context
  lives inside the report prose and its embedded figures.

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
