# Current session handoff

## What happened

**The Portfolio prompt-clarity bundle landed** (2026-09-14; PR #72 squash-merged to `main` at `c8f8c08`; `portfolio-v35` unchanged under the pre-debut rule).
The session began as a review of the Portfolio prompts against Qwen's re-thinking traces: the debug thought-logs from attempts 3–5 were segmented per model call, which showed that ~85% of attempt 5's re-thinking sat in the research synthesis call (unseen schema, the "no code fences" prohibition, exact-URL transcription, seed citability), not in the action call the user's screenshots came from; the v34 Finding-3 fix had already cut the action call's churn by roughly a third.
Codex implemented the combined Claude + Codex findings, Claude reviewed the diff and re-ran the gate, Codex fixed all eight review findings, and the gate ran clean again.
Landed: pass-local source-id citations resolved app-side, a dedicated synthesis orientation, schema-derived shape templates on interpretation / role-risk / distillation with neutral samples and named enum placeholders, explicit spot price and daily `return-volatility`, per-holding resolved capital-efficiency facts, validated continuity evidence in the action packet, and bounded original source text for distillation's typed extraction.
User ruling: tax is an optional rationale caveat with no effect on the rung (`docs/verification/2026-09-14-portfolio-prompt-clarity.md`).
BUILD §What remains item 1 + §Standing constraints and INDEX §Verification records were updated at this session-end.

## Current state

Working tree clean, `main` in sync with `origin/main`.
Nothing in flight.
The dev store is still a clean debut (wiped 2026-09-14; `web_source_state` is absent until the dev app's next start recreates it).
Attempt 6 writes `job_runs` id 6; its debut stamp to confirm is still **`portfolio-v35`** (`checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v3` unchanged).
**Bring-up caution stands:** build the dev app fresh before the first start — a stale binary would recreate the old `web_source_state` shape.
Attempt 6 is now the after-measurement of the whole bundle, not of the Finding-5 shape example alone; the per-call segmentation method and the attempt-5 baseline are recorded in memory (`thought-log-oscillation-measurement`).

## Open questions

- **When to launch attempt 6** — the user's call; don't propose it.
- Does the per-call re-think volume drop under v35 — read attempt 6's thought-logs with the same segmentation (v34 baseline over four holdings: synthesis 381 markers / action 40 / interpretation ~30 / gathering 0).
- Distillation original-source allocation (fair-share by URL order, up to ~79k chars of non-thinking prefill per reduce) — a throughput and truncation watch; evidence selection is named follow-up work.
- Sampling A/B (the thinking-precise row for synthesis / action) and non-thinking synthesis — deferred experiments, only if the bundle falls short.
- Carried: the per-domain `denied_count` share at book scale; the permanent SearXNG engine set; Finding 3 oscillation on the action call is now measured on four holdings but not at book scale.

## Where to start

Nothing is queued ahead of the run.
Wait for the user to name the attempt-6 session; then build the dev app fresh, start it, verify `PRAGMA table_info(web_source_state)` lists `failed_count` / `denied_count`, confirm zero `portfolio_runs` / `portfolio_checkpoints`, bring up infra per the OrbStack bring-up notes, confirm the `portfolio-v35` debut, read `data-health` early, and keep the run's thought-log folder for the per-call after-measurement.
Don't propose the run unprompted.
