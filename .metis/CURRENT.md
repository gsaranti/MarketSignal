# Current session handoff

## What happened

Big-run **attempt 2 never actually ran**, and the reason is the load-bearing
takeaway. Setup was clean (pinned Ollama v0.32.5 up with flash-attention and a
teed server log, both models present; dev app launched under `caffeinate` on the
`dev/` store), but the Portfolio run got initiated in the **PROD app by
mistake**: computer-use `open_application` by bundle id launched the installed
prod `.app`, not the bundle-less `cargo run` dev binary (`bundleID=NULL`). The
prod run failed instantly at pull-holdings with Schwab `invalid_client` (prod
has no `schwab_client_id`). **Nothing persisted to dev.** Also surfaced a UI
overflow bug — a long single-line failed-job error breaks the report-view layout
(auto-memory `long-error-breaks-layout`).

## Current state

Everything spun down — Ollama, dev app, `caffeinate` all killed; clean slate.

Reading the DB copies settled the state: the **dev store is already fully
configured** — `local_daemon_endpoint=http://localhost:11434`, reasoner
`qwen3.5:122b-a10b`, embedder `qwen3-embedding:4b`, `schwab_client_id` present.
So the next run needs **no dev reconfiguration** — just a Schwab-freshness check
(weekly re-login, was good through 2026-08-17) and Run analysis.

**Prod residue** to clean later in a separate, prod-only session (to avoid
re-colliding): three local-model settings were written to prod `app_settings`,
and one failed `job_runs` row (id 11) exists — dismiss its warning. No portfolio
or report data was touched (prod `portfolio_runs` = 0). Prod is the older v1.3.0
schema (no `portfolio_quick_checks` / `portfolio_outcome_episodes` /
`schwab_client_id`) — never run Portfolio there.

Behind the run, unchanged: digest compression (candidate 3), the "declined an
engine exit" vocabulary, and drafted-uncalibrated `NUM_PREDICT_*` — all waiting
on run evidence (owned by the attempt-1 record's §Disposition).

## Open questions

- **Were attempt 1's engine targets degenerate?** The one sample (SBUX) was
  steeply bearish, not flat; attempt 2 reads whatever persists first.
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant rests on
  the 2026-07-16 verification, not re-probed.

## Where to start

**Restart big-run attempt 2** in the fresh session — same agreed protocol (agent:
start pinned Ollama with teed log → dev app under `caffeinate`, confirm `dev/`
store → hand off; user: Schwab freshness + reach Portfolio → say go; agent:
initiate + monitor to terminal; do **not** open thought-logs, that's a separate
user step). **Critical fix from this session: drive the dev app by bringing its
OWN window frontmost (Cmd-Tab / relaunch), NEVER `open_application` by bundle id
— that launched prod** (auto-memory `dev-prod-app-identity-collision`). Dev is
already configured, so expect just a Schwab check + Run analysis. Read
`data-health` and the SBUX-shape targets early; checklist
`docs/verification/big-run-watch-set.md`; thought logs auto-capture in the dev app.
