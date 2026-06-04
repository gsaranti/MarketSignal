# Current session handoff

## What happened

**Planned and shipped the Settings surface slice — squash-merged to `main` (#6, `c8784e6`), branch deleted (local + remote).** Four load-bearing calls: (1) **config now persists in SQLite** — new `settings.rs` + `AppConfig::load(conn)` reads `app_settings` SQLite-first with per-field **env fallback**, redirecting `check_configuration` / `generate_report_manual` / the scheduler off the env-only path; `ModelMainAgent::from_env` stays env-only so the live smoke survives until a value is saved. (2) **Secrets never round-trip to the webview** — `get_settings` returns a `configured` bool per credential (never the raw key); model-dropdown options are sourced from `AgentModel` (closes the env-slug/display-name drift). (3) **Token save-gate** — a save is refused unless both API tokens are present (`configuration.md §API Tokens`), enforced in **both** `settings::save` and the Save button (this was Codex's P2 fix). (4) **Schedule toggle + manual Generate mirrored into Settings**, bound to the same `App.vue` state as the footer so they can't drift.

Reviews: metis (approve) + two Codex passes. Verified `cargo test` (62+4), `cargo clippy`, `npm run build`, **plus a live Tauri-dev GUI smoke** — nav opens Settings, the token gate disables Save, saving clears warnings + persists to SQLite, Generate enables when config completes, and the schedule toggle syncs with the footer.

## Current state

Slice merged, branches cleaned; `main` at `c8784e6`, working tree clean (this `CURRENT.md` rewrite aside). **No implementation in flight.** The last remaining net-new surface is the **Archive view** — small, reuses `research.rs` listing; needs an archive-dir resolver in `lib.rs` + an Archive nav item + the Vue view.

The user's real DB still has **`weekly_job_enabled=false`** (re-enable in-app or the Sunday-9AM job stays paused; the GUI smoke used a disposable DB, since restored). See [[live-model-smoke]].

## Open questions

- **Archive view (next slice)** — reuses `research.rs` listing; resolve the archive dir + add an Archive nav item/view. The kit's Archive screen is the visual reference.
- **Backend token-gate not exercised live** — `settings::save`'s rejection is unit-tested; the frontend gate prevents reaching it via the UI. *(awareness)*
- **No "clear a saved credential" affordance** — absent=unchanged means no UI path to remove a stored key. *(deferred, flagged in plan)*
- **Switch CSS duplicated** between `JobStatusPanel.vue` and `Settings.vue` — extract a shared `Toggle.vue` if they need to stay locked. *(minor)*
- **Plaintext secrets** in `app_settings` — macOS Keychain hardening deferred. *(plan assumption)*
- *(carried, untouched by this slice)* Inbox parse-failure error state (→ Step-5); inbox folder only materializes on "Add files…" (Step-5 needs it to pre-exist); inbox badge freshness (minor); sidebar single hardcoded report row → needs a recent-reports backend query; warning-area dismissal; `is_running` event-driven only; unused `.report-title`/`.report-dateline`; agent-construction failure isn't a recorded Failed job; Step-1 network pre-check; HTML-persistence path (Step 17).

## Where to start

**Plan the Archive view with `/metis-plan-task`** — the last net-new surface. Reuse `research.rs`'s listing pattern: an archive-dir resolver in `lib.rs`, an Archive nav item, and the Vue view (kit Archive screen for fidelity). If smoke-testing the live schedule first, re-enable `weekly_job_enabled` in-app.
