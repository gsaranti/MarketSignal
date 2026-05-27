# Market Signal — Design System Seed

## a. Product blurb

Market Signal is a local-first desktop research workstation that publishes one long-form Weekly Market Report, synthesized every Sunday by a fixed multi-agent analyst pipeline (a Head Market Analyst plus Bull, Bear, and Balanced reviewers) running against macro, market, news, and user-supplied research. It is built for individual investors and small-desk operators who want institutional-grade thesis continuity — evolving macro positions, retrospective auditing of prior calls, and forward-looking preparation for known market-moving events — instead of reactive daily commentary or a trading dashboard. The surfaces are a Tauri desktop window (Latest Report view, Recent Reports sidebar, Research Inbox/Archive, a persistent warning strip, Settings) and an exported Markdown or PDF report that is the durable archival artifact. What makes it distinctive: the product is a publication first and an app second — the weekly report is the unit of work, the UI exists only to read it, audit it, and feed it; everything runs on the user's machine; and the system is willing to say "the prior thesis was wrong" rather than perform conviction.

## b. Voice and tone

**Adjectives:** editorial, considered, plainspoken, evidence-anchored, intellectually honest.

The product writes like the head of a small institutional research desk addressing a sophisticated but time-constrained reader. It states things directly, names its uncertainty, distinguishes signal from noise out loud, and is comfortable revising or retracting prior calls. It never performs confidence it doesn't have, never sells, and never addresses the user as a customer being onboarded.

**Phrases the product would use:**
- "The thesis weakened this week."
- "Prior report assumed X; market did Y."
- "Conviction: medium. Pivot triggers below."
- "This is noise, not signal — revisit in two weeks."

**Phrases the product would never use:**
- "Unleash your edge," "Game-changing," "AI-powered insights," "Supercharge your portfolio."
- Exclamation marks. Emoji. Second-person marketing "you." First-person plural "we" used as a brand voice.

## c. Visual direction — positive

### Density and rhythm
Two-tier, not uniform. Long-form report prose is **spacious**: ~65-character measure, generous leading, wide outer margins, no inline chrome competing with the body. Tabular and metadata regions — Recent Reports sidebar, Settings, warning strip, in-report data tables, regime/stance chips — are **information-dense**: tight row height, hairline dividers, no per-row padding inflation. The page reads as a printed sheet with a thin operational margin around it. Never uniformly dense (Bloomberg-terminal mistake), never uniformly airy (consumer-onboarding mistake).

### Type personality
A two-family pairing, no display face.
- **Body / report prose:** a humanist serif with a high x-height designed for sustained on-screen reading — slight warmth, real italics (not slanted), genuine small caps available for the metadata header block at the top of each report.
- **UI chrome, labels, navigation, table headers, chips, settings:** a neutral grotesque sans with subtle geometric construction — restrained, low-contrast strokes, no quirky alternates.
- **All numerics everywhere — prices, yields, percentages, dates, timestamps, file sizes, durations:** tabular figures, always. Numeric columns get a proportional-with-tabular-figures cut from the sans family; code-like artifacts (filenames, JSON metadata previews in Settings, model IDs) get a single mono.
- There is no display moment, no oversized hero typography, no marketing wordmark in the chrome. The Weekly Report's `# Weekly Market Report` header is the largest type in the product, and it is set in the body serif at a restrained size.

### Color logic
Paper-and-ink as the structural base.
- A warm off-white page (think uncoated stock, not screen-white) with near-black ink (not pure black). Light mode is the canonical mode.
- **One** saturated accent — reserved exclusively for the active/interactive state: selected sidebar row, primary button fill, hovered link, focus ring. It appears in chrome and nowhere else.
- **Two** semantic data colors — a desaturated red and a desaturated green — used only inside data (directional change in a yield, an index delta, a sector performance cell). They never appear in chrome, in buttons, in chips, or in copy.
- **One** muted amber, used only in the Persistent Warning Area. Failed jobs, missed jobs, missing tokens, missing model config all share this color; severity is communicated by copy, not by escalating to red.
- **Market regime label chips** (`risk-on`, `risk-off`, `mixed`, `late-cycle`, `recessionary`, `recovery`) and the `thesis_stance` chip (`bullish` / `bearish` / `mixed` / `uncertain`) are tone-mapped: cool desaturated for defensive states, warm desaturated for constructive states, neutral grey for mixed/uncertain. These chips are the only place color enters the chrome other than the single accent. All other chrome — sidebar, settings, tab strip, dividers — is monochrome.
- A dark mode, if shipped, is warm-ink-on-charcoal (the same visual language inverted), not a neon-on-near-black alternative palette.

### Component personality
Mechanical, precise, printed.
- Corners: 0–2px radius across the board. No pill buttons, no rounded cards, no 12px+ radii anywhere.
- Buttons: flat. Primary = accent fill, no shadow, no gradient. Secondary = hairline border, no fill. Tertiary = text-only with a focus underline.
- Inputs: sit on a hairline bottom rule rather than inside a stroked capsule; the label is above and small-cap; the field is full-bleed in its container.
- Cards and surfaces: demarcated by 1px hairline rules or 1px dividers, not by elevation. Drop shadows are absent — there is no z-axis. The page is a sheet; the sidebar is a margin; the warning strip is a stamp.
- Tables: hairline horizontal rules, no zebra striping, no vertical rules, right-aligned numerics, left-aligned labels, header row in the sans family with letter-spacing.
- Chips: rectangular, hairline outline or tone-mapped fill, never pill-shaped, never with an icon inside.

### Motion stance
Restrained — motion has to earn its place.
- Default: ~120ms opacity transition on report load and on warning appearance. No transform-based entrances.
- Sidebar selection, tab switching, settings toggles: instant. No transition.
- The one deliberate motion is a slightly slower (~240ms) crossfade when navigating between historical reports in the Recent Reports sidebar — to reinforce the sense of paging through an archive rather than swapping screens.
- Running-job state uses a single quiet 1px progress line at the top of the report pane. No spinners, no pulsing skeletons, no shimmer.
- No hover micro-interactions on data. No parallax. No scroll-linked animation. No entrance animations on list items.

### Imagery and iconography
- **No photography. No illustration. No empty-state illustrations. No marketing imagery anywhere in the product.**
- Charts and tables are the only visual content inside reports, generated from real data, styled with the same restraint as the chrome: hairline axes, single accent per series, no gradient fills, no drop shadows, no 3D, no chart legends positioned over data.
- Icons: a single thin-stroke (1.25–1.5px) outlined set at 16/20px, used only in chrome — sidebar nav, warning strip, settings rows, export menu. Never decorative, never inside table cells, never inside chips, never inside body copy. No filled or two-tone icons. No flag-of-country glyphs next to geopolitical sections.

## d. Visual direction — negative (what to avoid)

- **No** purple→blue, blue→teal, or any multi-stop brand gradients. Anywhere. Including buttons, logos, chart fills, and backgrounds.
- **No** soft drop shadows on cards, panels, or inputs. No glassmorphism, no frosted blurs, no layered "lifted" surfaces.
- **No** rounded-everything: no pill buttons, no 12px+ corner radii on cards, no rounded input capsules, no rounded chips.
- **No** "AI product" visual signifiers: no sparkle/magic-wand icons, no animated gradient borders on the report pane, no chat-bubble assistant overlay, no shimmer on generating states.
- **No** dashboard-style KPI hero tiles with giant numerals and tiny labels. The report is the artifact; do not summarize it into a tile grid above itself.
- **No** stock photography, abstract 3D renders, isometric illustrations, or character mascots — including in onboarding, empty states, or error screens.
- **No** dark-mode-as-default neon palettes (cyan/magenta on near-black). If dark mode ships, it is the warm-ink-on-charcoal twin of light mode, not a separate visual language.
- **No** emoji in product copy, in section headings, in warnings, in chips, or in agent output.
- **No** Bootstrap-style alert blocks with a thick left color bar and an icon-circle. Warnings are a one-line stamp in the Persistent Warning Area, set in chrome type, with a text action.
- **No** animated loading skeletons that pulse with gradients. A single hairline progress line is the entire loading vocabulary.
- **No** chart styling tropes: no area-fill gradients under line charts, no glow/shadow under series, no donut charts, no 3D, no legend overlays floating on top of data.

## e. Reference screenshots

Each reference is for the single narrow axis named. Extract that axis only.

1. **Stripe Dashboard — Payments list table.** Axis: tabular-figures alignment in a dense ledger, right-aligned numeric columns, and how row selection is communicated with a 1px state change rather than a fill or shadow. Ignore Stripe's color palette and side navigation entirely.

2. **Linear — Issue detail page, right-hand metadata column.** Axis: the chip-and-label metadata block — small monochrome chips, low color, dense vertical stack, sans-serif labels with adequate letter-spacing. Take only the metadata-chip treatment; discard Linear's overall app shell, color system, and command palette.

3. **The Browser Company "Arc" release notes page (or any well-set long-form release-notes page).** Axis: the body-copy rhythm — measure, leading, the spacing between an `H2` and the paragraph beneath it, and how inline code/metadata sits inside running prose. Discard the brand chrome.

4. **NYT digital long-form article (e.g., a daily feature page).** Axis: the printed-publication feel of the headline-deck-body block at the top of a piece, including how a publish date and byline-equivalent metadata are typeset under the headline. The Weekly Report's header (`# Weekly Market Report` + Date + Report Type) should borrow this restraint. Discard everything else about nytimes.com.

5. **Notion — empty state for a new database.** Axis: the restraint of the empty state — one line of plain copy, one secondary action, no illustration, no gradient, no animated tutorial. Use this exact restraint for the Research Inbox empty state and the "no reports yet" state on first run.

6. **Are.na — a single channel page.** Axis: the editorial whitespace ratio — content is the foreground, chrome is a thin frame, the page does not compete with what's on it. Apply this only to the Latest Report view.

7. **Anti-reference — Robinhood mobile portfolio screen.** Explicitly do *not* resemble. Avoid its oversized green/red P&L numerals, the celebratory motion on data changes, the consumer-app gestalt, and the implicit message that up-is-good / down-is-bad on a daily cadence. Market Signal must feel like its opposite: quiet, weekly, comfortable with a losing week, and structurally uninterested in dopamine.

## f. Originality constraints

> This system must be original to this product. Do not adopt brand colors, proprietary typefaces, distinctive layout signatures, or recognizable visual treatments from any existing company or product. Reference screenshots are for the specific narrow axis noted next to each — extract that axis only and discard the rest. The goal is a system that feels intentional and specific to this product, not a system that resembles any other product.
