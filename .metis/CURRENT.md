# Current session handoff

## What happened

Resolved the **OpenBB open assumption**: dropped OpenBB from the MVP (Python-only → fragile inside a signed macOS bundle; negligible normalization value across three disjoint providers). The app now calls **FMP/FRED/BLS REST directly from Rust**, with **FMP the primary financial source and a gating credential alongside Tavily** (OA1/OA2 in `RESOLVED.md`; `data-sources.md` + `configuration.md` + `BUILD.md` updated). Per request, OpenBB history was then stripped from `docs/` and `BUILD.md` so they read forward-only — `RESOLVED.md` is now the sole decision trail.

Then planned, implemented, reviewed, and committed the **first vertical slice** (`9c21156`) — the repo is no longer the stock scaffold. Applied review nits (RFC3339-parse the agent `created_at` instead of byte-slicing; label-parity test; added the `## Investment Strategy` stub section) and fixed a local-first violation an external review caught: the design system's `colors_and_type.css` fetched fonts from Google on every launch — replaced with self-hosted `@fontsource` (`1e73ece`).

## Current state

The first vertical slice is **done, reviewed (approve-with-nits → nits resolved), smoke-tested, and committed**. Landed: `MainAgent` trait + `StubMainAgent` (`agent.rs`), rusqlite/bundled storage (`storage.rs`), a Tauri-free `generate_report` orchestrator (`pipeline.rs`), the `generate_report_manual` command (`lib.rs`), a Vue two-pane shell + Latest Report View (markdown-it) + sidebar, plus unit + integration tests. Verification: `cd src-tauri && cargo test` (3 green) + `npm run build`.

Carried forward from the slice's scope report (all → later slices): **HTML generation/persistence + PDF export**; the **execution gate** (config/token/credential validation — the command currently runs unconditionally with the stub); a **`list_reports`** command so the sidebar shows all reports, not just the latest. Stubs in tree: `StubMainAgent` (fixed body + labels), disabled Export button. `SYNTHESIS.md`/`INDEX.md` still carry minor walk-era staleness (a `/metis-reconcile` would refresh).

## Open questions

- **HTML-persistence path (Step 17):** markdown-it renders on the frontend for display now, but how the rendered HTML returns to the backend for SQLite persistence is unresolved — lands with the HTML/PDF slice.
- **Scheduler tech** (not yet built): `tokio`-timer + a tray-resident weekly job.

## Where to start

Run `/metis-plan-task` for the next slice: swap `StubMainAgent` for a real OpenAI/Anthropic `MainAgent` adapter behind the same trait, and build the **execution gate** (all four agent models configured; both OpenAI + Anthropic tokens; Tavily + FMP credentials present; network reachable). Optional first: `npm run tauri dev` to visually re-confirm the self-hosted fonts.
