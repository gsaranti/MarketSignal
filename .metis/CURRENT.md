# Current session handoff

## What happened

**Confirmed the app is feature-complete** against the as-built spec via a full four-layer audit (pipeline/agents, frontend/UX, runtime/continuity, data/storage/config) — every one of the 18 workflow steps has a verified code path; no `todo!()`/`unimplemented!()` in production; the only findings were two `data-sources.md` over-promises. Landed **two docs-only commits to `main`**:
- **`05ba726`** — trimmed `data-sources.md`'s aspirational FMP/BLS bullets (company financials / analyst estimates / market metrics / productivity data) to as-built reality; research is Tavily-only, those feeds were never wired.
- **`2b04c6c`** — annotated `report-workflow.md` with per-step **Type tags** (model call / computed / API) + an at-a-glance table + full **prompt-and-return contracts** for all 7 model-calling stages; fixed Step 5 (audit reasoning runs in the Step-16 call), Step 11 (packet is app-assembled, not model-built), Step 9 (branching is deterministic delta-rules) model-vs-computed attributions.

Also **settled a file-structure question** (no refactor needed — Codex corrected my size analysis: `fmp.rs` is genuinely ~1580 logic lines, not test-inflated; defer the `fmp.rs` submodule split + SSE-decoder extraction to next-touch, no churn). **Explored local-model feasibility** (discussion only, parked → memory `local-model-research-loop-idea`).

## Current state

`origin/main` = local `main` = **`2b04c6c`**, in sync; tree clean. **No work in flight, no queued slices, no owed code** — still feature-complete; this session was audit + docs reconciliation only.

## Open questions

All **live-run only**, none owes code (carried forward):
- **Cadence-const calibration** — research-threshold clamps (`THRESHOLD_SCALE_MIN/MAX`, `THRESHOLD_ANCHOR_DAYS=7`) and calendar back-window caps (`EARNINGS_BACK_MAX_DAYS=31`, `CALENDAR_BACK_MAX_DAYS=45`) await tuning vs real daily/weekly/monthly snapshots. Don't re-implement the curves (memory `manual-pivot-cadence-windows`).
- **Empirical prompt/skills calibration** — which of the 16 lenses + analytical-standards / posture-methods / counter-argument additions improve the report, which get ignored, and prose-repetition across them. No test catches prose dilution (memory `skills-forcing-function-only`).
- **Prompt worked-examples** *(deferred)* — a strong-vs-weak risk/thesis exemplar; validate against the same live run.
- **Future direction (parked, not owed):** local-model pipeline — hybrid (local for cheap stages, cloud for Step-16 synthesis) + a mediated model-driven research loop in Step 9. Full design conclusions in memory `local-model-research-loop-idea`.

## Where to start

**Live end-to-end run tests** — the only remaining work, all of it calibration that can't be resolved by reading code (tune the cadence consts, judge whether the prompt-rigor/skills additions improve real reports, validate worked-examples). Keys are in `keys.env` (memory `live-model-smoke`). Run discipline matters: FMP free tier is 250 req/day and GDELT is lockout-prone — a deliberate once-per-change activity, not a loop.
