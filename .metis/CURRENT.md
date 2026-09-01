# Current session handoff

## What happened

Launched **attempt 4** of the big confirmation run (`job_runs` id 5,
`portfolio-v32`, 47-holding debut): infra up (OrbStack + SearXNG with the
**Serper paid floor** + Ollama `NUM_PARALLEL=1`), engines re-probed (Serper +
google/bing/qwant/reuters served; google cse / duckduckgo / mojeek blocked but
fail-soft), clean debut confirmed, dev app run, user triggered. The search
backend served cleanly in-run (Finding 1's gate met, spillover closed) and the
ledger `quant` under-population did **not** reproduce (Finding 2) — but the **6d
research-findings terminal turn returned empty/fenced bodies at ~70%**, failing a
holding (PGNY): **Finding 4**. The action-call prompt friction was diagnosed to
an overloaded, unordered prompt: **Finding 3**. User cancelled at 7/47 once the
findings were in.

Then implemented **Fix B** for Finding 4 — the Step-6c research turn split so a
tools-only gathering loop and a separate grammar-only **synthesis call** never
share a request (the interleaving caused the empty bodies; mirrors the clean
interpretation call). User ruled the five build decisions via selector;
`portfolio::PROMPT_VERSION` → **`portfolio-v33`**. Eight Codex review rounds
hardened the evidence-sizing (source-quality fidelity, water-fill allocator,
drop-and-summarize, claim-validator provenance), all fixed.

## Current state

**Fix B landed — Codex-clean, user-approved** — 4 commits pushed (`6c27a22` core,
`c46d331` round-7, `3fd45f2` round-8, `1113317` BUILD.md); gate green (1413 tests,
clippy 0). Findings record: `docs/verification/2026-08-31-big-run-attempt-4-findings.md`
(+ INDEX pointer, `b238532`).

Attempt 4 is **cancelled** (`job_runs` id 5, no `portfolio_runs`, 5 holdings
checkpointed). **Infra torn down** (Ollama, SearXNG, OrbStack all stopped). The
dev store still carries the 5 cancelled-run checkpoints, so **attempt 5 must
re-wipe to a clean debut** per the watch set. The **debut stamp is now
`portfolio-v33`** (was v32).

Backlog: **Finding 3** — the action-prompt restructure (cut the reasoning-guidance
bloat to data+values+schema, keep the ~4–5 behavioral contracts as an ordered gate
list stated once) — diagnosed in the record's §Finding 3, **unbuilt**; the
remaining pre-run prompt candidate. Findings 5–6 (throughput, extraction) + the
attempt-3 open items want the run's telemetry first.

## Open questions

- **When to launch attempt 5** — user's call, from a wiped store; don't propose
  it unprompted.
- **Handle Finding 3 before attempt 5?** — the restructure is diagnosed but
  unbuilt; a pre-run prompt candidate.
- Does Fix B actually kill the ~70% empty-body rate on the live 122B — only
  attempt 5 confirms (watch the 6d research-findings retry rate early).
- The **permanent** SearXNG engine set — a post-run call; `settings.yml` set is
  provisional.
- **Memory drift:** `searxng-orbstack-bringup` + `big-run-waits-on-review-record`
  still say the debut stamp is `portfolio-v32` → now `portfolio-v33`.

## Where to start

Fix B is done. On the user's go, either **build Finding 3** (action-prompt
restructure — diagnosis in the attempt-4 record §Finding 3) or **launch attempt
5**: re-wipe the dev store to a clean debut → bring up infra
(`searxng-orbstack-bringup` memory) → confirm debut stamps **`portfolio-v33`** /
`checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3` → trigger → read
`data-health` early and watch the 6d research-findings retry rate to confirm Fix B.
