# Current session handoff

## What happened

Built attempt-4 **Finding 3**: the per-holding action-call prompt was restructured from a dense, unordered paragraph into data framing plus an ordered gate list — the reasoning-guidance bloat cut, the behavioral contracts kept but recast as facts the model infers from (user ruling: recast contracts as facts, not prohibitions).
That extended into a **full-surface sweep** of the Portfolio prompts (interpretation, role/risk, action, research + their evidence sections): app-architecture narration and how-to-weigh nudges cut, load-bearing contracts kept, and the F6 target-provenance weighing trimmed to the provenance facts (user ruling).
Three Codex review rounds then converged — the "ordered gates are a regression" objection was withdrawn — and a **narrow follow-up** removed residual narration the first sweep missed: the scoreboard rationale, the pre-profit cross-stage authorship narration, and the response contracts' decoder/meta-reasoning language.
The **prompt-posture doctrine was refined** from an absolute "never prescribe" to **governed vs. ungoverned**: governed content traces to a canonical contract (field semantics, output requirements, source policy, continuity, ruled action precedence) and stays; ungoverned content (app/downstream architecture, meta-reasoning, author financial preferences, unruled weighting/conclusion nudges) goes. Canonical at `docs/local-models.md §Prompt posture`; BUILD invariant and the attempt-4 record updated.

## Current state

All committed on `main` and pushed — `53f007f` (Finding 3 fix + full-surface sweep) and `75f3016` (posture cleanup follow-up); working tree clean.
No stamp moved: **`portfolio-v34`** remains the never-run debut stamp (both local stores have zero persisted Portfolio runs, the sole dev checkpoint is `portfolio-v32`, no `MARKET_SIGNAL_DATA_DIR` override), so every pre-debut change folds into it.
Gates green: **1,432** library tests (31 live smokes ignored) plus integration suites, warning-free clippy, `npm run build` passed, frontend untouched.
The finite Fix-B closure matrix stands at **C1–C7 closed, C8 static-closed / live-open** (attempt 5 alone closes the live empty-body-rate question).
The Portfolio Analysis job is built in full and the attempt-4 record's pre-run prompt candidates (Findings 3 & 4) are resolved, so the queue's next item is the **single big confirmation run (attempt 5)**.

## Open questions

- **When to launch attempt 5** — user's call, from a wiped store; don't propose it unprompted.
- Does Fix B plus the finite closure kill the ~70% empty-body rate on the live 122B? Only attempt 5 confirms; watch the 6d research-findings retry rate, the typed Data Health research counts (now including fetch-cap truncations), and gathering-bound gaps early.
- **Finding 3 oscillation** — does the leaner action prompt actually stop the action trace oscillating across rungs before settling? A throughput watch, per holding, only attempt 5 measures.
- The **permanent** SearXNG engine set — a post-run config call (finalize `settings.yml` once a full run shows which engines serve reliably under volume).

## Where to start

On the user's go, launch **Attempt 5** from a newly wiped dev store: bring up the infrastructure (Ollama + SearXNG per the OrbStack bring-up notes), confirm debut stamps `portfolio-v34` / `checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3`, and inspect early — the 6d findings retry rate, the typed Data Health research counts, and whether the action trace still oscillates (Finding 3).
