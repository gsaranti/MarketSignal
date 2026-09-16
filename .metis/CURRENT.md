# Current session handoff

## What happened

**Research request rows name their query or URL** (2026-09-15; `45e47b0` on `main`, pushed).
The user asked for the search query or page address on the Portfolio run log's web rows; both were already on the wire in the series id but never rendered.
A typed `RequestTarget { kind, text }` now rides the progress seam's two request events, set only by the research loop's search and fetch emitters; the series id stays the pairing key and is never parsed.
The tracker renders a tracked-caps SEARCH / FETCH token plus the text as the row's third segment, and the ok-row note ("12 hits", "served from document cache") before the status.
A research row is an explicit five-column grid, so the status marker holds the right edge down to the 620px minimum pane and the subject is what clips.
Codex's P2 made the subject a ghost-button disclosure — Enter, Space or click reveals the full text under the row — stripped to the row's metrics after the kit button's 1px border made research rows 2px taller.
Three rulings: a typed field, not a series-id parse; one line as its own column, not a second line; the ok-note shown.
Metis approve-with-nits (applied); the kit README's disclosure note is Codex-authored and kept by ruling.
The watch set gained the research-row subject watch under §How to read the run (`5c5dbc7`).
Verified in an offscreen WKWebView harness at 1180 / 900 / 620px, light and dark — a compiled `swiftc` binary with the sandbox off and read access `/`; the interpreted `swift` script hangs under the sandbox and a page outside the granted directory is refused.
No stamp moved; the dev store was never touched.
**The user named the next session as the attempt-6 session and ruled its stop rules** (see Where to start).

## Current state

Working tree clean, `main` in sync with `origin/main` after this session-end commit.
Nothing in flight.
The dev store is still the clean debut wiped 2026-09-14 (`web_source_state` absent until the dev app's next fresh start recreates it).
Attempt 6 writes `job_runs` id 6; its debut stamp set to confirm is **`portfolio-v35` / `checkpoint-v9` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`**.
**Bring-up caution stands:** build the dev app fresh before the first start — a stale binary would recreate the old `web_source_state` shape and lack the call fences, the bounded panes and the row subjects.
Attempt 6 is the after-measurement of the prompt-clarity bundle, the first run under the unit and overlay guards, the live witness for the fences, the first long-run look at the bounded panes, and the first live look at the subject column at book scale; its holding files split on `^==== call`.

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

**This is the attempt-6 session (user-named 2026-09-15).**
The goal is a run to completion — attempts 3, 4 and 5 were all ended early for findings, and the rates, outcome learning and the run-2 baseline need a completed run 1.
Two early-stop reasons only, both read on the first holdings and both the user's call once surfaced: a wrong-looking unit-exclusion rate, or Qwen reasoning still oscillating after v35 (open the first holding's thought-log early, segment on `^==== call`, count re-think markers per call class against the v34 baseline; the attempt-5 tell was the synthesis call debating the output format before planning content).
Everything else is recorded as a watch and the run continues.
Bring-up in order: keep the Mac awake; Ollama v0.32.5 with one parallel slot; OrbStack; render the Serper key into SearXNG's runtime settings and start the container; per-engine probe (Serper must answer with organic URLs); store pre-check on a copy (zero `portfolio_runs` / `portfolio_checkpoints`, `job_runs` max 5, no `web_source_state` yet); fresh `npm run tauri dev`; `PRAGMA table_info(web_source_state)` must list `failed_count` / `denied_count`; the user re-logs Schwab, tests the connection rows and clicks Run analysis (never drive the dev window by bundle id); confirm the debut stamps off the persisted checkpoint header; monitor from the terminal only — the dev log's `[run …]` rows, the thought-log dir, and copy-out store polls; read `data-health` early; count the unit and overlay exclusions against the watch set.
The ordered commands, gotchas and secret-hygiene rules live in the agent's local-run bring-up runbook memory; the repo homes are `docs/local-model-operations.md`, `docs/web-research.md §Search backend` and `docs/verification/big-run-watch-set.md`.
