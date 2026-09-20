# Current session handoff

## Active task

The single big confirmation run — all three attempt-7 fix slices are now landed and the dev store is re-wiped to a clean debut; attempt 8 is the next item and awaits the user naming the launch session.

## What happened

Slice 3 (attempt-7 Finding 5) landed: a bounded EDGAR **8-K Exhibit 99.1** fallback fired on a `403` from an eligible issuer IR host, current-issuer earnings-results only, unresolved on any ambiguity, SSRF-guarded to sec.gov, cited under the SEC URL (the IR URL stays a diagnostic, never a redirect alias), no stamp.
Codex drove the Metis loop and its self-review caught five real matching bugs across two reject rounds (correction wording, fiscal guidance, conflicting periods, announcement notices, year-to-date); all fixed.
Claude reviewed the diff against the code and independently re-ran the full gate green (1,554 Rust library + 32 integration, clippy warning-free, frontend build, clean diff-check).
Committed and pushed as `e385fbd`.
The dev store was re-wiped to a clean debut this session (Claude, on user request) ahead of attempt 8.
The prod hash change flagged last session was explained: the user ran the Market Signal **report** job on prod (a legitimate prod write), not an errant one.

## Current state

No implementation in flight; the confirmation-run BUILD item stays incomplete, but every attempt-7 fix is now committed on `main` (Slice 1 `f43ecde`, Slice 2 `ed13e03`, Slice 3 `e385fbd`).

- **Debut stamp set for attempt 8:** `portfolio-v48` / `checkpoint-v15` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`, portability `11`. The `checkpoint-v15` + portability `11` bump is Finding 3a's persisted `quarter` enum, not Finding 6.
- **Dev store is a clean debut.** Local-suite tables cleared (portfolio_runs / checkpoints / checkpoint_holdings / research_seeds / outcome_episodes / quick_checks / holdings_pulls / price_bars / web_documents / web_source_state → 0 rows, table kept not dropped); `vector_memory` untouched (no portfolio vectors existed); continuity kept — 30 reports / 14 baselines / 70 report vectors / `job_runs` max 7 (attempt 8 = id 8) / app_settings 16; VACUUM 9.85→1.85 MB, integrity ok. Full pre-wipe backup: `~/Downloads/market-signal-dev-store-attempt7-preclear-2026-09-20.db` (sha `c9760e4d`). **Attempt 8 needs no further wipe** — the fresh dev app recreates `web_source_state` via init_schema. Prod untouched (sha verified unchanged after the wipe).
- **Runtime-unverified, for attempt 8:** cache restoration, gathering latency, complete-book coverage, live prompt compliance; Slice 3's live recovery coverage and model citation behavior; distillation latency (attempt-6 tokens-per-topic is no longer an acceptance gate). Slice 3 is deliberately **partial** — peer issuers, production/consensus pages, paywalls and image-only figures stay out — and its SEC recovery spends the holding's 40-fetch budget (bounded +10/resolution, deduped by issuer+release), so watch budget competition on issuers with several failed releases.
- **Owed (session-end cannot edit BUILD):** the BUILD §Standing-constraints gathering-loop line still needs the Slice 1 append-only invariant added.
- Entries 9 and 10 of the 2026-09-17 list still follow a completed run; the conditions-display slice remains separate.

## Open questions

- Whether the §Slices grouping belongs in the findings doc or only in `CURRENT.md` — carried, now low-stakes (all slices landed; the findings doc records them).

## Where to start

The user has signaled attempt 8 for the next session. On their word — never propose it — bring up per [[local-run-bringup-runbook]] (caffeinate → Ollama v0.32.5 one-slot → OrbStack → render Serper + SearXNG up → per-engine probe → fresh `npm run tauri dev` → user clicks Run → monitor to completion). The dev store is already a clean debut, so **skip the re-wipe**; confirm it at bring-up (`web_source_state` absent until the fresh app recreates it, `portfolio_runs` 0). Before or alongside the run, land the owed BUILD gathering-constraint prose update.
