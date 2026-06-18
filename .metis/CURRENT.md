# Current session handoff

## What happened

Planned, implemented, reviewed, and **merged the cadence-awareness feature** (`/metis-plan-task` → `/metis-implement-task` → `/metis-review-task` **approve**, then PR #30 squash-merged → `origin/main` = local `main` @ **`4879831`**). On-demand runs now treat the elapsed interval since the previous report as a first-class input: a new `cadence` module (`ReportCadence`/`CadenceBucket`) computed **once** in `pipeline::compute_cadence` from the prior snapshot's `captured_at` **alone** (independent of the change view), threaded to the GDELT/Tavily news windows (`NewsSource::gather(cadence)`) and to the agents (main-agent `format_cadence` posture block + analyst `review(packet, cadence)` cue), so daily/weekly/monthly runs are written differently. **Two external Codex rounds each caught a real bug, both fixed + pinned by tests:** (1) Tavily's `days` field is no longer in the Search API → switched to the documented **`start_date`** bound; (2) an initial "derive cadence from `deltas`" simplification conflated first-report with a corrupt prior → **reverted to the independent `compute_cadence`** (regression test `cadence_survives_a_corrupt_prior_snapshot…`). A third Codex pass was doc/test hygiene (orphaned doc comment + the regression test), also addressed. Verified green: lib tests **392**, integration suites, clippy `--all-targets --all-features` clean. `docs/data-sources.md §Tavily/§GDELT`, `.metis/BUILD.md`, and memory `manual-pivot-cadence-windows` all amended.

## Current state

`origin/main` = local `main` = **`4879831`**, in sync; working tree clean; feature branch deleted. **No work in flight, no queued code slices.** BUILD.md's "Pending code slices" paragraph now reads cadence-awareness as LANDED with the threshold-scaling item narrowed to deferred.

## Open questions

Both deferred, neither owed code:
- *(deferred, needs a live run)* **Research-branch threshold scaling** — the *one* remaining cadence gap: `research_executor`'s `DeltaBranchPolicy` thresholds (oil +7% / 10y +25bp) stay weekly-calibrated absolutes, so they over-fire on long gaps / rarely fire on daily ones. **Agreed fix (with the user): time-normalize — sqrt-of-time, anchored to the current weekly numbers.** Needs a live run to calibrate the curve (memory `manual-pivot-cadence-windows`).
- *(live, needs a run)* **Empirical skills calibration** — which of the 16 lenses improve the thesis/analyst reviews, which get ignored, and whether prose-only delivery repeats language across the 16. No test catches prose dilution (memory `skills-forcing-function-only`).

## Where to start

No owed code. The most concrete next slice is **research-branch threshold scaling** — the design call is settled (sqrt-of-time, weekly-anchored); the work is in `research_executor.rs` `TRIGGER_RULES`/`rule_fired`, with the `ReportCadence` value already available to thread in — but it **needs a live run to calibrate**, so pair it with a smoke. Otherwise pick up skills calibration (also needs a live run) or open a fresh direction.
