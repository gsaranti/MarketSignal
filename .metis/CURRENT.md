# Current session handoff

## What happened

**Released v1.1.0 — the COT build is now the installed daily-use app.** Bumped the
version 1.0.0 → 1.1.0 across all **5 anchors** (commit `010ce36`); reconciled the
**CFTC Commitments-of-Traders** source into the two docs that enumerate data sources
but had missed it — `configuration.md`'s keyless-sources sentence (now BLS/GDELT/**CFTC**)
and `docs/README.md`'s corpus-map line (commit `38d0824`). Both pushed to `main`.
Cut an **annotated `v1.1.0` tag** on HEAD (`38d0824`) and pushed it to origin
(matches the v1.0.0 annotated-tag-on-release-tip convention; chose HEAD over the bump
commit so the tag includes the doc fixes — binaries identical either way). **Built**
the release bundle (frontend gate + `--release` compile, stamped 1.1.0) and
**installed** it over the v1.0.0 in `/Applications` — same bundle id → config/keys/
reports/memory preserved, no re-seed ([[release-build-install]]).

## Current state

`main` at **`38d0824`**, tree clean, in sync with origin; tag `v1.1.0` pushed.
Installed app is **v1.1.0** (running, PID was 18481), execution gate green with zero
clicks (app_settings intact: 9 rows = 5 keys + 4 model picks). **Verified the
production store is a clean slate** — `reports`, `vector_memory`, `baseline_snapshots`,
`job_runs` all **0**, `reports/*.md` empty — so the next report is genuinely **#1**
with no prior continuity (user never ran a job on v1.0.0; the `dev/` sandbox is
separate and untouched). The COT feature **and** the #41 prompt changes (session-tense
+ conviction + freshness) are installed but **LIVE-UNVALIDATED** — no report has run
on this build yet.

## Open questions

- **Cadence Run B** — report #1 on the COT build lands the #41 goals *and* the new
  positioning group together; the report *after* #1 closes the delta-engine +
  vector-memory-recall check ([[manual-pivot-cadence-windows]]).
- **COT calibration** — how hard to weight positioning extremes is deferred to live
  runs (forcing-function posture); plumbing + lens pointer already in
  ([[skills-forcing-function-only]]).
- **Market holidays / early closes** — `market_clock` still mislabels them "open until
  4pm" (documented v1 cut; needs an NYSE calendar).
- **opus-main leaning** — accumulating; worked-examples prompt an optional carry
  ([[live-config-opus-main-leaning]]).

## Where to start

**Generate report #1** on the v1.1.0 COT build (a real, billable live run) — it
validates the three #41 goals *and* the CFTC positioning group together and starts the
Cadence Run B chain. A **second report after #1** closes the delta / memory-recall
check. The release plumbing (bump → docs → tag → build → install → clean-slate verify)
is fully done; the first live run is the only remaining step.
