# Current session handoff

## What happened

Shipped **v1.0.0** — the first stable release. Bumped the version 0.1.0→1.0.0 across
all five anchors (`tauri.conf.json` runtime source-of-truth, `package.json` + `Cargo.toml`
+ both lockfiles; plus the App.vue masthead comment) → commit **`f336bcd`** on `main`,
annotated tag **`v1.0.0`**, both pushed. Full verify green (cargo test + clippy, npm build
+ 40 pure / 91 Vitest). Built the release bundle and **installed it to `/Applications`**
(replaced the 0.1.0, quarantine cleared) — so the daily driver is now v1.0.0 and **no
longer predates #41**.

**Clean-slated production data** before the build (user's call): emptied the report-derived
tables (`reports`, `baseline_snapshots`, `vector_memory` incl. learnings, `job_runs`) and
deleted the one morning report `.md`, but **kept `app_settings`** (5 keys + 4 model picks)
so no re-seed. Deleted the old `…BACKUP-2026-06-23` (22 pre-launch reports —
**unrecoverable**); left `dev/` alone. Method + nuances in [[release-build-install]]. This
**reset the continuity chain to zero**.

## Current state

`main` at **`f336bcd`** / tag `v1.0.0`, tree clean, nothing owed. v1.0.0 is installed and
reads the cleaned production DB with keys/models intact; **report history is empty** (next
report = #1). The #41 prompt changes (session tense, conviction, news freshness) are now
**installed and ready but still LIVE-UNVALIDATED** — the prior "rebuild off `c3ca28d` first"
blocker is cleared; only an actual report exercises them.

## Open questions

- **Cadence Run B** — clearing the DB reset continuity, so this now needs **two** fresh
  reports: #1 validates session-tense + conviction + freshness (the #41 goals); the report
  *after* #1 validates the delta-engine + vector-memory recall (delta/recall is now #2-vs-#1,
  no longer against the old report) ([[manual-pivot-cadence-windows]]).
- **Market holidays / early closes** — `market_clock` still mislabels them "open until 4pm"
  (documented v1 cut; needs an NYSE calendar).
- **opus-main leaning** — accumulating; the worked-examples prompt is an optional carry
  ([[live-config-opus-main-leaning]]).

## Where to start

Generate the **first v1.0.0 report** from the installed app — no rebuild needed (it reads
keys from `app_settings`). Read it against the three #41 goals: correct session tense, a
*firm base-case* thesis (not hedged), and a fresh-vs-important news balance. A second report
after it closes Cadence Run B (watch the delta + memory-recall paths now rebuilding from a
clean chain).
