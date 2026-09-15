# Current session handoff

## What happened

**Structured per-call thought-log fences landed** (2026-09-15; `c0ca8e6` on `main`, pushed).
The user asked for thinking-mode local-model output written to files for post-run analysis; the debug thought-log sink already captured every such call, so the slice became structure: every local chat call now emits `model-call-started` / `model-call-finished` on the progress seam (call number, stage, model, think flag, transport, tools/format, `num_ctx`/`num_predict`, prompt chars; then outcome, elapsed, token counts, stop reason or a ~200-char failure message), step-stamped at emit, and the sink renders them as `==== call N | …` / `==== end N | …` fences in the holding's file.
The client is the one home for thinking now — `chat_with_role` forwards a non-streamed reply's thinking between the fences, and the research loop's own forwards are gone; the research trait carries a stage label (topic, leg, gathering turn index).
Nine rulings (research turns stay non-streaming, full field set, live witness = attempt 6's first holding, one fenced file per holding, local calls only, capped failure detail, `PromptUsage.stage` unchanged, tracker UI unchanged, every call fenced) are recorded in the commit body; no verification record was written.
Metis reviewer approved with nits (applied); Codex found one doc P3 (applied) and an accepted pre-existing limitation: a stream read error or daemon error chunk skips the pending-thinking flush, so up to 23 trailing chars of a streamed call's thinking can be lost on that path.
No stamp moved; `PROMPT_VERSION` stays `portfolio-v35`.

## Current state

Working tree clean, `main` in sync with `origin/main` after this session-end commit.
Nothing in flight.
The dev store is still the clean debut wiped 2026-09-14 (`web_source_state` absent until the dev app's next fresh start recreates it); untouched this session.
Attempt 6 writes `job_runs` id 6; its debut stamp set to confirm is **`portfolio-v35` / `checkpoint-v9` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`**.
**Bring-up caution stands:** build the dev app fresh before the first start — a stale binary would recreate the old `web_source_state` shape and would lack the fences.
Attempt 6 is the after-measurement of the prompt-clarity bundle, the first run under the unit and overlay guards, and the live witness for the fences; its holding files split on `^==== call` (memory `thought-log-oscillation-measurement` has the fence shape; the attempt-3–5 folders still need the old phrase split).

## Open questions

- **When to launch attempt 6** — the user's call; don't propose it.
- Does the per-call re-think volume drop under v35 — same segmentation as the v34 baseline over four holdings (synthesis 381 markers / action 40 / interpretation ~30 / gathering 0), now per fenced call.
- How many holdings the ADR guard, the null-`reportedCurrency` rule, and the overlay name screen remove at book scale — `reportedCurrency` is untested live.
- Streaming the research gathering/synthesis turns (crash-honest thinking, live tracker view) — deferred follow-up; needs tool-call accumulation in the stream decoder and a live re-verification on the pinned Ollama; take up only after attempt 6.
- Distillation original-source allocation as a throughput/truncation watch; evidence selection is named follow-up work.
- Sampling A/B and non-thinking synthesis — deferred experiments, only if the bundle falls short.
- Carried: the per-domain `denied_count` share at book scale; the permanent SearXNG engine set; action-call oscillation measured on four holdings, not book scale.
- Post-release only: the quick-check state's own parameter stamp has no mismatch consumer — needs a policy before any shipped build.

## Where to start

Nothing is queued ahead of the run.
Wait for the user to name the attempt-6 session; then build the dev app fresh, start it, verify `PRAGMA table_info(web_source_state)` lists `failed_count` / `denied_count`, confirm zero `portfolio_runs` / `portfolio_checkpoints`, bring up infra per the OrbStack bring-up notes, confirm the `portfolio-v35` debut with the v9 / v5 / v4 stamps, read `data-health` early, count the unit and overlay exclusions against the watch set, and open the first holding's thought-log file early to confirm the fences render as documented.
Don't propose the run unprompted.
