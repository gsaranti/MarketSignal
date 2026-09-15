# Current session handoff

## What happened

**Bounded, self-scrolling reasoning panes landed** (2026-09-15; `80fb228` on `main`, pushed).
The user asked that the run tracker's streamed thoughts stop dominating the Portfolio run log, so the reasoning pane became its own primitive, `ReasoningPane.vue`, shared by the main agent, each analyst and each holding step: the serif-italic well is capped at fourteen ui-sm lines (a token-derived `calc`, ≈281px) with its own scroller, per-pane auto-follow that yields once the reader scrolls up, a fresh mount that starts at the tail, and a named `role="group"` with `tabindex="0"` (prop `accessibleLabel`, "<step label> reasoning") so a 47-holding run adds no landmarks.
Four rulings: all reasoning panes; the report-text console stays unbounded; no collapse of finished holdings; the clipped last line inside the hairline well is the only scroll cue.
Metis reviewer approve-with-nits (mount-at-tail and doc placement applied); Codex P3 region→group applied by Codex, then the prop rename at the user's word.
Verified by the full gate plus an offscreen WKWebView harness (light / dark / 640px), since deleted; the dev store was never touched.
No stamp moved; `PROMPT_VERSION` stays `portfolio-v35`.

## Current state

Working tree clean, `main` in sync with `origin/main` after this session-end commit.
Nothing in flight.
The dev store is still the clean debut wiped 2026-09-14 (`web_source_state` absent until the dev app's next fresh start recreates it).
Attempt 6 writes `job_runs` id 6; its debut stamp set to confirm is **`portfolio-v35` / `checkpoint-v9` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`**.
**Bring-up caution stands:** build the dev app fresh before the first start — a stale binary would recreate the old `web_source_state` shape and lack the call fences and the bounded panes.
Attempt 6 is the after-measurement of the prompt-clarity bundle, the first run under the unit and overlay guards, the live witness for the fences, and the first long-run look at the bounded panes; its holding files split on `^==== call`.

## Open questions

- **When to launch attempt 6** — the user's call; don't propose it.
- Does the per-call re-think volume drop under v35 — same segmentation as the v34 baseline over four holdings (synthesis 381 markers / action 40 / interpretation ~30 / gathering 0), now per fenced call.
- How many holdings the ADR guard, the null-`reportedCurrency` rule, and the overlay name screen remove at book scale — `reportedCurrency` is untested live.
- Whether 47 capped panes still read long on a full run — collapsing finished holdings was declined this session; revisit only on the user's word.
- Streaming the research gathering/synthesis turns — deferred; needs tool-call accumulation in the stream decoder and a live re-verification on the pinned Ollama; only after attempt 6.
- Distillation original-source allocation as a throughput/truncation watch; evidence selection is named follow-up work.
- Sampling A/B and non-thinking synthesis — deferred experiments, only if the bundle falls short.
- Carried: the per-domain `denied_count` share at book scale; the permanent SearXNG engine set; action-call oscillation measured on four holdings, not book scale.
- `BUILD.md` §Built has no bullet for the fences slice (`c0ca8e6`) or the pane slice (`80fb228`); whether either rises to BUILD's altitude is the user's call.
- Post-release only: the quick-check state's own parameter stamp has no mismatch consumer — needs a policy before any shipped build.

## Where to start

Nothing is queued ahead of the run.
Wait for the user to name the attempt-6 session; then build the dev app fresh, start it, verify `PRAGMA table_info(web_source_state)` lists `failed_count` / `denied_count`, confirm zero `portfolio_runs` / `portfolio_checkpoints`, bring up infra per the OrbStack bring-up notes, confirm the `portfolio-v35` debut with the v9 / v5 / v4 stamps, read `data-health` early, count the unit and overlay exclusions against the watch set, open the first holding's thought-log file early to confirm the fences, and watch the first holding's reasoning well cap and follow in the tracker.
Don't propose the run unprompted.
