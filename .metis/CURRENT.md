# Current session handoff

## Active task

The big confirmation run — debut **attempted 2026-08-30 and cancelled early at 2/47**; a re-attempt is gated on the SearXNG mitigations below.

## What happened

The debut big confirmation run (attempt 3, the `portfolio-v30` v9-shape full run) was launched from a wiped store and **deliberately cancelled by the user at 2 of 47 holdings** (TSLA, PSX; `job_runs` id 4 = `cancelled`, no `portfolio_runs` row).
Setup: dev app + production-export import (report continuity — 30 reports / 67 vectors; portfolio store wiped to a clean debut, `prior_run_id=None`, 47 positions), Ollama `qwen3.5:122b-a10b` on the M5 (100% GPU, `num_ctx=131072`), SearXNG via a **freshly installed OrbStack** (it was not installed — `brew install --cask orbstack`, engine started headless).
It was stopped because the keyless `google cse` engine began rate-limiting under the research loop's volume → SearXNG returned empty → **Tavily spillover** (7 requests logged), and Tavily is reserved for the market-report job.
Six findings were recorded in `docs/verification/2026-08-30-big-run-findings.md`: SearXNG engine blocking + apply-later mitigations, ledger `quant` under-population, action-call prompt friction, bounded-retry rate (2/2 holdings), throughput (~25 min/holding → ~20 h full run), and extraction telemetry.
The run environment was fully torn down afterward.

## Current state

Nothing in flight; Ollama, the SearXNG container, the OrbStack engine, and the tauri-dev app are all down.
The debut run is banked only as a 2-holding partial, so every finding is candidate-not-rate — a full run recomputes them.
A re-attempt is **gated on applying Finding 1's SearXNG mitigations first** — client-side pacing + pruning the dead engines from `searxng/settings.yml`, or a paid SERP overflow — or it burns the report job's Tavily quota again.
Uncommitted working-tree changes on top of `e9d5cd8`: the new findings doc, its `.metis/INDEX.md` row, and this handoff — not yet committed.
`BUILD.md §What remains` item 1 still describes the run as pending/unattempted; it wants a line noting the 2026-08-30 partial attempt and the mitigation gate (see Pending decisions).
The prior unrelated carried follow-ups remain untouched — the cloud `run_job` seam, negative composite yield, `progress.rs` poisonable locks, the tracker `ok` row's dropped count, TO logic-flow line 397, the 600 s `/api/tags` backstop, whole-ledger seed injection, qualitative 6g un-trip semantics, an IPv6-loopback wire test, the audit sources line, and the unreconciled-delete fail-soft sentence's home.

## Open questions

- Whether and when to re-attempt the run — the user's call; the SearXNG mitigations must land first.
- Which mitigation to adopt — client pacing + engine pruning (keyless) vs a paid SERP overflow (Serper.dev) — decided against this run's Tavily-cost read.
- Whether a second full pass runs — the user's call after a *first full* run's result (no full-run result exists yet).

## Where to start

Do not propose re-running unprompted. If the user wants to re-attempt: first land Finding 1's SearXNG mitigations (prune dead engines in `searxng/settings.yml` + client pacing, or wire a paid SERP fallback), then re-run from a wiped store per `docs/verification/big-run-watch-set.md`, reading `docs/verification/2026-08-30-big-run-findings.md` first. The operational stack bring-up is the `searxng-orbstack-bringup` memory.
