# Fonts

This system is loaded from **Google Fonts** via `@import` at the top of
`colors_and_type.css`. There are no `.woff2` / `.ttf` files in this folder
because none were provided in the brief.

## Current substitutions

| Role | Family | Status |
| --- | --- | --- |
| Serif (body + display) | **Source Serif 4** | Google Fonts substitute |
| Sans (UI chrome) | **Public Sans** | Google Fonts substitute |
| Mono (optional, dense tables only) | **IBM Plex Mono** | Google Fonts substitute |

Both substitutes match the brief's description point-for-point:

- **Source Serif 4** — humanist book face with open apertures, modest
  contrast between strokes, designed for long-form reading at 16–18 px.
  Not a transitional like Times, not a slab.
- **Public Sans** — neutral humanist sans with a modest x-height,
  restrained ink-traps, true tabular figures (via `tnum`), three weights
  available (400 / 500 / 600). Not geometric, not a grotesque, not
  ultra-rounded.

## To replace with licensed files

1. Drop `.woff2` files into this folder (e.g. `MarketSignal-Serif.woff2`,
   `MarketSignal-Sans.woff2`).
2. Open `../colors_and_type.css`, delete the Google Fonts `@import` at the
   top, and replace with a local `@font-face` block per family.
3. Update the `--font-serif` / `--font-sans` variables to point at the new
   family names.
