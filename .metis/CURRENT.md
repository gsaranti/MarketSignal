# Current session handoff

## What happened

Big-run **attempt 2 ran on the dev app and failed — productively.** Driven
entirely from the terminal (no computer-use: `request_access` resolves "Market
Signal" to the prod bundle and the bundle-less dev window is filtered from
screenshots, so the user clicked "Run analysis"). The full **47-holding pass ran
clean** — zero 429s/retries/fetch errors, every deep-EOD ok, `num_ctx` 131072
honored, construction prompts ~9.5–12K/131K `truncated=0`: attempt-1's
output-budget exhaustion did **not** recur. It then **fail-harded at Step 7b
construction on divergence-cause validation** (AMZN/DIS/GM changed action off the
standalone lean with no `divergence_cause`; RKT/TDOC gave `cash-freed` that maps
to no whole-book aggregate), unrepaired by the named-violation re-run. **The 7b
repair worked**: persisted as a degraded run (`portfolio_runs` id 2, `run_id`
6a52f1dd, `constructed=0`, 47 verdicts) — attempt 1 lost everything here.

## Current state

Everything spun down (Ollama, dev app, `caffeinate` killed; port 11434 free).
The degraded run and the thought-logs
(`dev/thought-logs/20260813-191600-3f42e8e5/` — `construction.txt` + 38 holding
streams) are durable on disk.

Queued work is the **analysis plan** at
`docs/verification/2026-08-13-big-run-attempt-2-analysis-plan.md` — three
workstreams: (1, primary) root-cause the divergence-cause fail-hard and produce a
fix that yields a book; (2) prompt-effectiveness from 5–10 thought-logs; (3)
accuracy spot-check over 30–50% of holdings (model-view vs engine-`metrics`
cross-check). Read-only, dev store; no app run needed.

Banked distributions: grades B9/C13/D16/F8 (**no A**), risk-tier High 28/Med
12/Low 6, dead-money fails 14/clears 9/indeterminate 23. Book NOT produced →
outcome learning / the two-arm retrospective / the paired-card render stay
unexercised (watch-set items deferred to a future produced-book run).

Still pending: **prod residue** from the prior mis-run (3 local-model settings +
failed `job_runs` id 11) — separate prod-only cleanup session. Behind
everything: digest compression (candidate 3), `NUM_PREDICT_*` calibration — still
awaiting a produced-book run.

## Open questions

- **Divergence-cause root cause** — is `cash-freed` structurally unsatisfiable
  under the fixed preset (cash unconstrained), and is the three-cause vocabulary
  too narrow? (owned by the plan, WS1)
- **Grade distribution has no A** (B9/C13/D16/F8) — needs the reserved
  sector-aware normalization slice, or honest? Decide off this run's letters.
- **Two-id split** — thought-log dir keyed by progress id `3f42e8e5`, persisted
  row by `run_id` 6a52f1dd; confirm it is by design.
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant still
  rests on the 2026-07-16 verification, not re-probed.

## Where to start

**Execute the analysis plan**
(`docs/verification/2026-08-13-big-run-attempt-2-analysis-plan.md`). Primary
goal: diagnose the 7b divergence-cause fail-hard and propose the fix that
produces a book — read `construction.txt`, `roll_up.aggregates`,
`construction.rs`, the output schema, and the named-violation re-run path. All
read-only on the dev store (copy-out the DB) — no app run, no computer-use, no
prod. Surface fix rulings to the user before implementing.
