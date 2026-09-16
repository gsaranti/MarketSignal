# Current session handoff

## What happened

**No launch; last night's dev-app failure diagnosed** (2026-09-15, late session).
This was the user-named attempt-6 session, but the user ended it early to start the bring-up fresh; nothing launched, no code or doc changed, no stamp moved, the dev store was never touched.
The 2026-09-15 evening bring-up had run legs 1–7 clean and then died at dev-app launch with Vite serving HTTP 500 — `EPERM` opening the repo's `index.html`.
Cause: a macOS Files and Folders denial for the Desktop folder, attributed to Claude.app because that session had been started from the Claude desktop app by accident (its bg-pty-host sat in the process chain; the user-level TCC database changed at 18:38, inside that bring-up window).
The deny hit that whole app tree at once — Vite, the dev app and the session's own file reads — and never touched Terminal's grant.
This session ran directly under Terminal.app (ancestry `Terminal → login → zsh → claude → zsh`) and read the repo fine.
The attribution finding and a pre-launch ancestry check went into the agent's bring-up runbook memory (an agent memory, not a repo file).

## Current state

Working tree clean, `main` in sync with `origin/main` after this session-end commit.
Nothing in flight; nothing running (no Ollama, dev app or caffeinate; OrbStack stopped).
The dev store is the clean debut: expect 0 `portfolio_runs` / 0 `portfolio_checkpoints` / `job_runs` max 5, and `web_source_state` PRESENT in the v6 shape with 0 rows (the evening's one fresh app start recreated it) — no re-wipe needed.
Attempt 6 is unlaunched and writes `job_runs` id 6; its debut stamp set is **`portfolio-v35` / `checkpoint-v9` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`** (re-verified against the source constants this session).
Stop rules stand as ruled 2026-09-15: two early-stop reasons only, both read on the first holdings and both the user's call once surfaced — a wrong-looking unit-exclusion rate, or Qwen reasoning still oscillating after v35 (segment the first holding's thought-log on `^==== call`, count re-think markers per call class against the v34 baseline); everything else is a watch and the run continues to completion.
The Desktop-folder deny is still on record for Claude.app: a session started from the desktop app repeats the failure until Desktop Folder is turned on for Claude under System Settings → Privacy & Security → Files and Folders.

## Open questions

- Does the per-call re-think volume drop under v35 — same segmentation as the v34 baseline over four holdings (synthesis 381 markers / action 40 / interpretation ~30 / gathering 0), now per fenced call; this is also attempt 6's second stop rule.
- How many holdings the ADR guard, the null-`reportedCurrency` rule, and the overlay name screen remove at book scale — `reportedCurrency` is untested live; this is attempt 6's first stop rule.
- Whether 47 capped panes, and dozens of subject rows per holding, still read long on a full run — collapsing finished holdings was declined; revisit only on the user's word.
- Streaming the research gathering/synthesis turns — deferred; needs tool-call accumulation in the stream decoder and a live re-verification on the pinned Ollama; only after attempt 6.
- Distillation original-source allocation as a throughput/truncation watch; evidence selection is named follow-up work.
- Sampling A/B and non-thinking synthesis — deferred experiments, only if the bundle falls short.
- Carried: the per-domain `denied_count` share at book scale; the permanent SearXNG engine set; action-call oscillation measured on four holdings, not book scale.
- Post-release only: the quick-check state's own parameter stamp has no mismatch consumer — needs a policy before any shipped build.

## Where to start

The user ended this session to run the attempt-6 bring-up fresh in a new one; confirm that naming at its start rather than assuming it.
Start it from a plain Terminal window, never the Claude desktop app, and print the shell's process ancestry before the dev-app launch.
Then the bring-up in order: keep the Mac awake; Ollama v0.32.5 with one parallel slot; OrbStack; render the Serper key into SearXNG's runtime settings and start the container; per-engine probe (Serper must answer with organic URLs); store pre-check on a copy (expectations in Current state); FRESH `npm run tauri dev` (a stale binary would recreate the old `web_source_state` shape and lack the call fences, bounded panes and row subjects); `PRAGMA table_info(web_source_state)` must list `failed_count` / `denied_count`; the user re-logs Schwab, tests the connection rows and clicks Run analysis (never drive the dev window by bundle id); confirm the debut stamps off the persisted checkpoint header; monitor from the terminal only; read `data-health` early; count the unit and overlay exclusions against the watch set.
The ordered commands, gotchas and secret-hygiene rules live in the agent's local-run bring-up runbook memory; the repo homes are `docs/local-model-operations.md`, `docs/web-research.md §Search backend` and `docs/verification/big-run-watch-set.md`.
