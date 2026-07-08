# Current session handoff

## What happened

**Data portability was verified at the data level against real production data — the last open verification on the feature.** The user exported from the prod app (v1.3.0) and imported into the dev app; a strictly read-only comparison (scratchpad DB copies, live stores never opened for write) confirmed a perfect copy: all five exported tables row-identical (12 reports, 12 baseline snapshots, 27 vector rows with byte-exact embeddings by SHA-256, 0 portfolio_runs / holdings_pulls), all 12 report Markdown files hash-identical, `markdown_path` correctly re-derived to the dev reports dir with 1:1 DB↔disk agreement on both sides, referential integrity clean, and the stay-behind tables (`app_settings`, `job_runs`, telemetry) confirmed left behind. The one anomaly was run to ground: prod's one-line `app_settings` DDL in `sqlite_master` is a fossil of the 2026-06-23 v1.0.0 pre-seed `sqlite3` command (the one-line form never existed in repo history) — pre-existing, unrelated to the import. Coverage note: this exercised the **unencrypted** path; the passphrase (AES-256-GCM/Argon2id) leg remains offline-tested only.

## Current state

On `main` @ `687f4bb`, tree clean; nothing mid-flow — a verification-only session (no code, docs, or `.metis/` project-state changes; BUILD.md needs no update since no slice landed). Installed app = **v1.3.0**; the no-build rule remains until full Portfolio + Trade Opportunities land. The dev store now holds an imported copy of prod's corpus (expected — it was the import target). The M5 migration path is validated end-to-end: export (use a passphrase for the transit copy) → import on the M5 → re-enter keys/config there, since `app_settings` deliberately doesn't travel.

## Open questions

- **Encrypted-archive live round-trip (new, optional)** — today's check covered the unencrypted path only; one passphrase export→import before the M5 move would close it (comparison method reusable: scratchpad DB copies + normalized NDJSON diff).
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data to render the holdings table; the dev store now has prod's corpus but no portfolio runs).
- **Keychain fail-soft candidate (carried)** — a denied Keychain read errors the whole `check_local_configuration` report, blanking the local warning categories for the session; fail-soft with the v2 wiring.
- **Stage-and-swap import hardening (carried)** — mid-import I/O failure can leave partial files; named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage; check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Back to the standing order: **full Portfolio (funds)** — `/metis-plan-task` against `docs/portfolio-analysis.md` §Asset eligibility (fund path); BUILD.md's §What remains carries the queue. Then Local-models Settings section (the clear path for the shipped warning band) / sidebar Portfolio-runs history → Trade Opportunities.
