# Current session handoff

## What happened

A documentation session — no code. Designed a new **whole-corpus data export/import** (backup/restore) feature for the Settings page, motivated by the coming M1→M5 migration (the store is stable across app *versions* but has no path across *machines*). Grounded the design in the real storage layer (an Explore pass mapped all 10 SQLite tables + the filesystem stores), then locked three decisions with the user: **(1)** export scope = all durable analytical data (`reports` + on-disk Markdown bodies, `vector_memory` learnings+summaries, `baseline_snapshots`, local-suite `portfolio_runs`/`holdings_pulls`, research folders) — `app_settings` secrets/config, Keychain, `job_runs`, and telemetry excluded, so nothing sensitive ever enters the archive; **(2)** import = fresh-load / replace-all-with-confirmation, merge deferred, `app_settings` never touched; **(3)** optional passphrase encryption. Captured as `docs/data-portability.md` (a structured versioned zip, *not* a raw DB-file copy — WAL sidecars, secret-stripping, no DB schema-version marker), cross-linked from `docs/export.md`. With one-time OK, added a Planned paragraph to `BUILD.md` and a concept section to `INDEX.md`.

## Current state

On `main` @ `bb10761`, but the **working tree is dirty** — four uncommitted files: `docs/data-portability.md` (new), `docs/export.md`, `.metis/BUILD.md`, `.metis/INDEX.md`. Not committed (none was requested). No code, no build — the no-build-until-Portfolio-and-TO rule holds; eight slices still banked, installed app stays v1.2.1. The data-portability feature is **designed, not built** and is now the queued next slice — independent of the local suite (table-driven, covers suite tables as they fill), implementable now against the cloud report corpus. Design detail to carry: vector-memory embedder-binding does **not** affect the M5 migration (report vectors use the fixed cloud `text-embedding-3-large`; local namespaces are empty on hardware-gated machines). (Carried unverified: the table-head glyph hit-target fix is CSS-only — confirm with one click next dev-app open; launch-time local-suite warnings are the proactive render working, not a regression.)

## Open questions

- **First post-v0.31.2 Ollama release (carried)** — does it ship the #14645 fix (PR #15901), and cover the `think:true`+`tools` format-ignored mode? Check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — `/chains` unexercised live; tighten the drift guard to either-absent once a live response confirms both maps present.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s or truncate.
- **local-model M5 pre-flight (carried)** — 122B load/backend, #14645-fix-shipped check, `num_ctx`, throughput, in-house long-context probe.
- **M5-calibration (carried)** — Stooq refresh, `continuity_weight` bands, Research-stale threshold, tripwires, DTO budget, leftover-budget ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

**Next slice = data portability export/import** (user-chosen). First, commit the four staged doc/state files. Then `/metis-plan-task` against `docs/data-portability.md` — a Rust `export_data`/`import_data` command pair + a new Settings **Data** section; verification is a `cargo test` round-trip (export→import parity + secrets-excluded + encrypted round-trip) named alongside clippy, plus a Vue spec. Still no interim build. After this: full Portfolio (funds) → Local-models Settings section / sidebar Portfolio-runs history → Trade Opportunities.
