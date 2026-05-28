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
