## Stack

- **Shell:** Tauri 2.x (Rust backend, webview frontend)
- **Frontend framework:** Vue 3 with Composition API and `<script setup>` SFCs
- **Styling:** CSS custom properties from `market-signal-design-system/project/colors_and_type.css`
- **Native APIs:** use `@tauri-apps/api/*` packages; never reach for browser File/Notification APIs when a Tauri equivalent exists
- **Backend communication:** Tauri `invoke()`, not `fetch()` to a local server

## Design system

This project has a design package at `./market-signal-design-system/`.
It is the source of truth for all UI decisions — tokens, components, voice, motion, layout posture.

Robustness, accessibility, and cross-component composition are handled by a
separate, brand-neutral skill, `frontend-craft`. Apply it on all UI work in
this project — building, editing, or reviewing. The division is strict: the
design package is the source of truth for **appearance** (what things look
like); `frontend-craft` governs **completeness and robustness** (every
interaction and data state, keyboard and screen-reader access, contrast,
responsive/resize behavior, overflow, and how components align and cohere
when placed together). `frontend-craft` never picks colors, type, spacing, or
motion character — when it calls for a state the design package doesn't define
(a disabled or error treatment, resize behavior, a section seam), don't invent
an off-system look: extend the package per step 5 below.

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

<!-- metis:start -->
## Metis workflow

This project uses Metis — a lightweight toolset for keeping a project's intent, status, and history legible across agent sessions.

**State on disk** lives in `.metis/`:
- `BUILD.md` — what we're building (forward-looking architecture brief).
- `CURRENT.md` — session handoff. Read first on any new session.
- `SYNTHESIS.md`, `INDEX.md`, `CONTRADICTIONS.md`, `QUESTIONS.md`, `RESOLVED.md` — reconciliation artifacts for the project's `docs/` corpus (created when one exists).
- `config.yaml` — project name and Metis version pin.

**Workflow primitives** (type `/metis-` for the full list):
- `/metis-session-start` — load `.metis/CURRENT.md` and orient.
- `/metis-reconcile` — read `docs/`, surface contradictions and open questions.
- `/metis-build-spec` — produce `.metis/BUILD.md`.
- `/metis-plan-task`, `/metis-implement-task`, `/metis-review-task` — the per-task loop.
- `/metis-session-end` — update `.metis/CURRENT.md` for next session.

**Path conventions** in Metis skill instructions — resolve by prefix:
- `.metis/...` and `docs/...` — relative to the project root.
- `${CLAUDE_PLUGIN_ROOT}/...` — relative to the Metis plugin install on disk.
- Anything else (e.g., `references/foo.md`, `references/foo.sh`) — relative to the skill's own folder.

Plans live in chat by default; only `CURRENT.md` persists session-to-session continuity.
<!-- metis:end -->
