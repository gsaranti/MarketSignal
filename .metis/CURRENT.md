# Current session handoff

## What happened

**The Portfolio financial-correctness sweep landed** (2026-09-15; `7987972` on `main`, pushed).
Codex audited the Portfolio job and found four financial-correctness defects: non-USD and ADR statement units priced against a USD market cap, covered-call funds priced on an uncapped equity payoff plus distributions, a quick-check sweep able to confirm a falsifier off an annual-to-TTM basis flip, and model outlooks scored at the engine's 1/6/12-month windows instead of their authored 1-month / 1-year / 3–5-year horizons.
Codex implemented; Claude reviewed over three rounds.
Round one cut roughly 250 lines of pre-release data-compat Codex had added (the 2026-08-29 no-compat rule) and the now-dead priced-branch structural flag; the pre-existing v1/v2 quick-state migrations went with them.
User ratified all four rulings: option-overlay funds route to `role_risk_only`; every depositary receipt and any non-USD quote or statement currency is excluded as unsupported units (reversing the 2026-08-05 US-listed-ADR admission — the way back is the deferred FX + ADS-ratio slice in BUILD §Owned by no slice); the scoreboard scores the model's mid read at 12 months and leaves its long read unscored while the engine keeps 1/6/12; stamps moved to `evidence-floor-v5` / `quick-check-v4` / `checkpoint-v9` with `PROMPT_VERSION` staying `portfolio-v35` (unrun).
Record: `docs/verification/2026-09-15-portfolio-financial-correctness.md`; the unit rule is single-homed at `portfolio-analysis.md §Asset eligibility`; the watch set gained the null-`reportedCurrency`, ADR/overlay-exclusion, and first-sweep flow-withhold watches; BUILD and INDEX were updated in the same commit.
The prompt-clarity bundle (PR #72, 2026-09-14) is still the unrun v35 content attempt 6 measures.

## Current state

Working tree clean, `main` in sync with `origin/main` after this session-end commit.
Nothing in flight.
The dev store is still the clean debut wiped 2026-09-14 (`web_source_state` absent until the dev app's next fresh start recreates it); this session did not touch it.
Attempt 6 writes `job_runs` id 6; its debut stamp set to confirm is **`portfolio-v35` / `checkpoint-v9` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`**.
**Bring-up caution stands:** build the dev app fresh before the first start — a stale binary would recreate the old `web_source_state` shape.
Attempt 6 is both the after-measurement of the prompt-clarity bundle (segmentation method and attempt-5 baseline in memory `thought-log-oscillation-measurement`) and the first run under the unit and overlay guards, so expect some holdings to land not-rated or role-risk that priced in attempts 3–5.

## Open questions

- **When to launch attempt 6** — the user's call; don't propose it.
- Does the per-call re-think volume drop under v35 — same segmentation as the v34 baseline over four holdings (synthesis 381 markers / action 40 / interpretation ~30 / gathering 0).
- How many holdings the ADR guard, the null-`reportedCurrency` rule, and the overlay name screen remove at book scale — `reportedCurrency` is a provider field the app never read before, untested live.
- Distillation original-source allocation (fair-share by URL order, up to ~79k chars of prefill per reduce) — a throughput and truncation watch; evidence selection is named follow-up work.
- Sampling A/B and non-thinking synthesis — deferred experiments, only if the bundle falls short.
- Carried: the per-domain `denied_count` share at book scale; the permanent SearXNG engine set; action-call oscillation measured on four holdings, not book scale.
- Post-release only: the quick-check state's own parameter stamp now has no mismatch consumer (the checkpoint header's copy still gates resume) — needs a policy before any shipped build, outside the pre-release wipe rule.

## Where to start

Nothing is queued ahead of the run.
Wait for the user to name the attempt-6 session; then build the dev app fresh, start it, verify `PRAGMA table_info(web_source_state)` lists `failed_count` / `denied_count`, confirm zero `portfolio_runs` / `portfolio_checkpoints`, bring up infra per the OrbStack bring-up notes, confirm the `portfolio-v35` debut with the v9 / v5 / v4 stamps, read `data-health` early, count the unit and overlay exclusions against the watch set, and keep the run's thought-log folder for the per-call after-measurement.
Don't propose the run unprompted.
