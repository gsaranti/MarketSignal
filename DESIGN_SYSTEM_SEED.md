# Market Signal — Design System Seed

## a. Product blurb

Market Signal is a local-first desktop application (Tauri + Vue) that publishes a single long-form weekly market report on Sunday mornings, written by a multi-agent pipeline that behaves like a professional research desk: a Head Market Analyst that synthesizes, three internal analyst voices (Bull, Bear, Balanced) that stress-test the thesis, and a retrospective audit that holds prior reports accountable. The audience is the serious individual investor or independent analyst who wants structural, thesis-driven coverage — not real-time tickers, not daily P&L, not trade signals. The product surfaces are narrow and reading-shaped: a Latest Report view rendered from Markdown into HTML, a Recent Reports sidebar (the last thirty issues), a Research Inbox/Archive for user-supplied PDFs and notes, a Persistent Warning Area, and a Settings panel for model choice and API credentials. What makes it distinctive is the editorial posture — weekly cadence as an act of restraint, theses that evolve across issues rather than reset, explicit retrospective auditing, and a single unified analyst voice rather than a feed of widgets. The product should feel less like a fintech app and more like a private research bulletin that happens to live on your machine.

## b. Voice and tone

Five adjectives: **measured, plain-spoken, accountable, structural, unhurried.**

The product writes the way a senior analyst writes the Sunday note to their own desk: declarative, specific, willing to say "we don't know," willing to say "we were wrong last month." It treats the reader as a peer, not a customer being onboarded. It does not perform expertise through jargon, and it does not soften analysis through hedging. When it is confident it commits; when it is uncertain it names the conditions under which it would revise.

Phrases the product would use:
- "The thesis is unchanged this week."
- "Last month's call on energy looks early; the underlying logic still holds, but the timing was wrong."
- "Two things would force a revision: a sustained breach of [X], or a clear inflection in [Y]."

Phrases the product would never use:
- "Crushing it 🚀"
- "Smart money is positioning for…"
- "Don't miss this week's must-read insights"
- "Powered by AI"
- "Buy / Sell / Hold" (the product explicitly does not give trade calls)

## c. Visual direction — positive

### Density and rhythm

The rhythm is that of a well-set print broadsheet that has been quietly translated to a screen — generous breathing room around the report body, with a single readable column of prose roughly 64–72 characters wide, paragraph spacing that follows a vertical baseline grid rather than arbitrary gaps, and clear typographic hierarchy doing the work that boxes and cards do elsewhere. The Latest Report view is reading-heavy and therefore the loosest surface in the system: comfortable margins, no sidebar chrome competing with prose, charts inset with the same restraint as figures in a journal article. The Recent Reports sidebar and the Watchlist within reports are denser by intent — tight row rhythm, hairline rules between rows rather than padding, monospaced numerals so columns of figures align without effort. Settings is the tightest surface: a single-column form with label-above-field and no decorative grouping cards. Whitespace is never decorative; it earns its place by serving the eye's transition between sections or the reader's pause between ideas. Nothing on the screen has the airy "look how minimal we are" emptiness of a marketing page.

### Type personality

A deliberate pairing of two families, used with discipline:
- **Body and report prose:** a humanist serif with a clear text-face design — open apertures, modest contrast between strokes, designed for long-form reading at 16–18px. Think the temperament of a contemporary book face: warm but not bookish, authoritative but not severe. Not a transitional like Times, not a slab.
- **UI chrome, labels, captions, numerals, tabular data:** a neutral humanist sans with a single weight family used at 400/500/600. Modest x-height, restrained ink-traps, true tabular figures available for column alignment. Not a geometric sans (no Futura-ish circular o's), not a Helvetica-style grotesque, not anything ultra-rounded.

Letter-spacing posture: **tight at body sizes** (the serif sets at -0.005em to -0.01em for paragraph reading), **neutral at UI sizes** (the sans tracks at its default metrics for 13–14px chrome), and **modestly opened** (+0.04em to +0.06em) only at the smallest 11px caption/label sizes — never at any sort of large or display size, where opened tracking reads as "1990s magazine deck." Display moments are rare: only the report title at the top of each issue and the date line beneath it. Both set in the serif, not the sans, and at restrained sizes (28–32px for the title), never the oversized "hero" treatment.

### Color logic

The system is essentially monochrome — warm ink on warm paper — with a single accent used sparingly and structurally.

- **Surface:** a paper-toned off-white, slightly warm (a hint of cream, not blue-white), the color of premium uncoated stock. Dark mode is an inverted version using a soft graphite rather than pure black, with the same off-white inverted to a warm near-white text color.
- **Text:** a near-black with a touch of brown in it — never pure #000 — so the body color reads as ink rather than pixels.
- **Accent:** a single deep, desaturated hue. **Primary recommendation: oxblood / aged burgundy** — editorial, gravitas-bearing, distinctly non-fintech, doesn't compete with red/green chart conventions. *(If oxblood feels wrong on review, two equally viable alternatives that fit the same temperament: a deep india-ink blue with a touch of slate, or an oxidized-brass ochre. The brief asks me to flag color choices, and the hue is the one place where the docs give no signal — so pick one of the three, but do not retreat to a generic SaaS blue or any green/red financial palette.)*

Where color appears:
- Interactive states (primary button fill, link underline-on-hover, focused input border)
- The current/selected item in the sidebar (a thin accent rule on the leading edge, not a fill)
- One narrow band of chart ink when a series needs emphasis against neutral series

Where color is deliberately absent:
- Body text and report prose (always ink)
- Iconography (icons are stroked in the text color, not the accent)
- Section dividers and table rules (hairlines in a low-contrast neutral)
- Up/down signal for market data — direction is communicated through sign, position, weight, and a small neutral chevron, **not** through saturated red and green fills (the product is explicitly not a trading app; daily up/down is not the point)
- Backgrounds of any card, panel, or widget — surfaces stay paper-toned; differentiation is by hairline border, not fill

### Component personality

Buttons are rectangular with a **1px–2px corner radius** — barely rounded, just enough to avoid hard pixel corners on screen. No pill shapes. Primary buttons are filled in the ink color with paper-toned text; secondary buttons are ghosted with a hairline border in the text color. Inputs are open: a single 1px bottom border in the text color, no fill, no top/side border, label sitting above the field rather than floating inside it. Cards, where they exist at all, are defined by a single hairline border in a desaturated neutral — never by a fill change, never by a shadow. The system is **flat with hairlines**; there are no shadows, no elevation layers, no z-axis. Section breaks within a report are a thin horizontal rule with a small dotted center ornament — a print convention, not a div.

Hover-state philosophy: **hover shifts state through a faint paper-tinted background fill or a hairline color change, never through scale, never through shadow, never through saturated color flares.** A hovered row in the sidebar acquires a 2% darker paper tone; a hovered link gains an underline rather than changing color; a hovered primary button darkens by one tonal step rather than glowing.

### Motion stance

**Restrained throughout.** Motion exists to confirm a state change happened, not to entertain. Durations are quietly **perceptible** (~120ms) for state changes — input focus, button press, sidebar selection — and **instant** (no animation) for navigation between views. The longest acceptable transition is the appearance of a newly generated report after a Sunday job completes: the new entry slides into the sidebar with a single ~200ms fade-and-settle and stops there. There is no celebration. There are no number spinners counting up to final values, no progress bars dressed as art, no shimmer placeholders during loading. A long-running job (the report can take ~30 minutes of background processing) shows a steady, undecorated status row — text and a single hairline progress indicator. Page transitions are avoided entirely; switching views is a hard cut. Scroll-triggered reveals do not exist.

### Imagery and iconography

**No photography. No illustration. No mascots.** The only graphical content in the product is chart imagery generated from real market data, and even that is rendered with restraint: a single ink color, optional accent for emphasis, hairline gridlines, tabular figure labels. Charts read as figures in a journal, not as dashboard widgets.

Icons are **outlined, single-weight, hairline (1.25px stroke at 16px size)**, sharing the typographic line weight of the body sans — when placed next to a 14px label, the icon's stroke and the text's stem should appear to be drawn with the same pen. No two-tone icons, no filled icons, no brand-colored fills, no rounded corner caps inside icons, no decorative gradients. The icon set is small by design: the product needs perhaps a dozen icons (settings, archive, inbox, export, warning, success, etc.), not a hundred.

## d. Visual direction — negative (what to avoid)

Each of these should be recognizable on sight as a violation:

1. **No gradients of any kind on brand surfaces or buttons** — no purple-to-blue, no warm-sunset, no subtle "premium" gradient washes on cards or headers. Surfaces are flat tones.
2. **No drop shadows for elevation.** No "card lifted off the surface" treatment. Differentiation is hairlines and tone, not z-axis.
3. **No glassmorphism, no frosted blur, no translucent overlays** stacked on background imagery. The product has no background imagery to overlay in the first place.
4. **No rounded-everything.** Corner radii above 4px read as consumer-app on sight. Pill-shaped buttons (full-radius capsules) are prohibited.
5. **No neon or saturated accents on dark surfaces** — no cyan, lime, hot pink, electric purple, "AI gradient" accent stripes. The dark mode is graphite-on-warm-near-white text and the same restrained accent, nothing brighter.
6. **No oversized hero text.** The Latest Report does not get a 64px headline. Report titles set at ~28–32px in the serif and stop there. Marketing-page typography does not appear inside the product.
7. **No all-uppercase navigation, no character-spaced uppercase labels at body sizes.** Uppercase appears only at 11px caption sizes and only with restrained tracking.
8. **No centered marketing compositions inside the product.** The home view is not a landing page. Left-aligned hierarchy throughout; no centered hero blocks with a CTA pair underneath.
9. **No "dashboard widget" cards** — the genre of a card containing one large number, a sparkline, and a percent-change pill. The product does not have a KPI dashboard. Numerical context lives inside the report prose and its embedded figures.
10. **No red/green saturation as the primary up/down signal.** Direction is communicated by sign, position, weight, and a small neutral chevron. Chart series are not colored by sentiment.
11. **No celebratory motion on data changes** — no confetti, no checkmark flourishes, no number-counter spin-ups, no "your report is ready!" toast animation. New artifacts appear quietly.
12. **No page transitions** — no slide-in between views, no crossfade between routes, no shared-element morphing. View changes are hard cuts.
13. **No scroll-triggered reveals or parallax** anywhere — not in report bodies, not in settings, not on first launch.
14. **No skeleton shimmer placeholders.** Loading shows text status; empty states show plain prose.
15. **No emoji as functional UI** — no "📊 Reports" sidebar labels, no emoji in empty states, no emoji in warning messages. The product can spell.
16. **No stock photography, no human portrait imagery, no illustrated mascots, no abstract geometric brand shapes** layered onto surfaces.
17. **No two-tone icons, no brand-colored fills inside icons, no rounded-cap strokes** — icons share the type's line; that is the whole rule.
18. **No "Powered by AI" badges, no spark-icon flourishes, no model-name surfacing in chrome.** The user picks the model in Settings and otherwise never sees provider branding in the reading surface.

## e. Anti-references

**Robinhood (and the broader consumer-trading-app gestalt).** Avoid the consumer-app posture that treats financial data as entertainment — oversized celebratory P&L numerals, motion-as-reward, the implicit message that up-is-good on a daily cadence, the gamified onboarding flows, the bold saturated greens against pitch-black. Market Signal is the philosophical opposite: weekly cadence, no positions, no P&L, no daily score. Nothing in this product should ever feel like it is congratulating the user. The reader is here for analysis, not dopamine.

**The generic modern-SaaS look exemplified by Linear-and-its-imitators.** Avoid the "elevated startup" gestalt that has become the default for venture-backed productivity tools: a purple-to-blue brand gradient, near-black surfaces with cool accent glows, geometric sans-serif everywhere, sub-pixel-perfect rounded cards with soft shadows, command-palette-as-personality, an aesthetic that signals "modern" through uniformity with every other tool in the category. The problem isn't the craft — that gestalt is well-executed — it's that it has no editorial spine. Market Signal is a publication with a viewpoint; it should look like one, not like another well-built workspace tool.

**Notion / Substack / the "soft startup" reading aesthetic.** Avoid the friendly-rounded-decorative posture: emoji garnish in headers, hand-drawn-feeling illustration spots, oversized cover images on every document, light pastel accent surfaces, the conversational "👋 Welcome back" empty states. That gestalt reads as casual and personable, which is the wrong register entirely. Market Signal is a private research desk, not a friendly newsletter. The reading surface should feel composed and edited, not warm and chatty.

## f. Originality constraints

> This system must be original to this product. Do not adopt brand colors, proprietary typefaces, distinctive layout signatures, or recognizable visual treatments from any existing company or product. No reference screenshots are being provided — the system's identity should come from the directional language in this document, not from emulating any external source. The goal is a system that feels intentional and specific to this product, anchored only by the product's own UX goals and the directional language written here.
