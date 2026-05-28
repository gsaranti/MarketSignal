## Stack

- **Shell:** Tauri 2.x (Rust backend, webview frontend)
- **Frontend framework:** Vue 3 with Composition API and `<script setup>` SFCs
- **Styling:** CSS custom properties from `market-signal-design-system/project/colors_and_type.css`
- **Native APIs:** use `@tauri-apps/api/*` packages; never reach for browser File/Notification APIs when a Tauri equivalent exists
- **Backend communication:** Tauri `invoke()`, not `fetch()` to a local server

## Design system

This project has a design package at `./market-signal-design-system/`.
It is the source of truth for all UI decisions — tokens, components, voice, motion, layout posture.

When working on any UI task:

1. Start by reading `market-signal-design-system/README.md` (written
   for coding agents) and `market-signal-design-system/project/SKILL.md`.
2. Use the tokens in `colors_and_type.css` directly — they're CSS
   custom properties. Reference them in component <style> blocks
   via var(--token-name). Never invent off-system colors, radii,
   or spacing values.
3. Use the components in `ui_kits/market_signal_desktop/` as the
   fidelity reference for surfaces and component composition.
   Translate to Vue 3 single-file components faithfully; match
   visual output, not internal structure.
4. The "What this system rejects on sight" section of `SKILL.md`
   is binding. If a design decision drifts toward any of those
   patterns, revert.
5. If the design package doesn't cover a case, extend it
   consistent with the system's rationale and note what you
   extended. Don't silently resolve.

Before implementing any non-trivial UI work, share a brief plan:
which tokens and components you'll use, any new patterns you're
introducing, and any gaps or conflicts you noticed.
