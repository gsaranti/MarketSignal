# Current session handoff

## What happened

**The pre-run review batch shipped and pushed** (`c8d5308..2790338`, 12 commits,
Codex approved after four rounds). A pre-attempt-2 review sweep (15 confirmed
findings over the repair + pre-run slices) became four fix slices: **A** —
failure evidence is true, complete, and bounded (`job_runs.detail` carries the
error chain, `record_usage` never drops a length-stop row, the length-stop
classification is single-homed with an honest unattributed reading, parse
contexts embed capped snippets, suite rows carry the HTTP-level cause, the
connection-test key leak closed); **B** — the repair pass can't burn itself
(shared `overlay_keeps` kept-set predicate, scoped violations, case-variant
collapse); **C** — the **persisted `constructed` marker** (authored at the
persist seam, SQL-filterable column with a transactional migration, loud-skip
`decode_run` at every read seam, three-way never-ran/degraded-only/unreadable
refusals, marker on the wire); **D** — identity + tracker (three-way
`description_identity` with the TickerOnly-vs-profile cross-check, canonical-
source name standard for header fallbacks, the sweep price pass bracketed,
failed tracker rows render their cause). A combined-range review fixed 14 more,
including **unreadable rows listing column-backed** (disabled, count-less,
tagged) with history-aware empty states and pull copy. BUILD.md was updated:
marker as-built; **the step-ownership slice queued as item 1 before attempt 2
(user decision)**.

## Current state

Nothing in flight. `main` pushed at `2790338`. Gate at close: 1062 lib + 32
integration cargo tests, clippy clean at `--all-targets --all-features`,
`npm run build` clean, 46 node + 239 vitest.

Behind attempt 2, unchanged: digest compression (candidate 3, doubly
instrumented) and the "declined an engine exit" vocabulary, both waiting on run
evidence. `NUM_PREDICT_*` values remain drafted, uncalibrated.

## Open questions

- **Step-ownership concurrency semantics** — what "the owning step" means when
  stages run concurrently (the report's analyst trio, the research executor);
  to settle at plan time for the slice below.
- **Were attempt 1's engine targets degenerate?** The one sample (SBUX) was
  steeply bearish, not flat; attempt 2 reads whatever persists first.
- **Is the FMP dated-EOD rung de facto primary?** Read `data-health` early.
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant rests
  on the 2026-07-16 verification, not re-probed.
- **INDEX.md row** for the degraded-run / constructed-marker concept (docs span
  portfolio-workflow §7b, storage, interface) — offered, not yet ruled.

## Where to start

Plan and build **the progress step-ownership slice** (BUILD §What remains
item 1): stamp request events with their owning step at `RunContext::emit`,
retire the 17-site bracket convention and the tracker's phantom-step synthesis
(which paints FAILED on successful runs when tripped), and **fold in shaped
row statuses for the remaining ~17 `suite_get` sites** (quote/EOD already
carry them). Settle the concurrency semantics at plan time; update
`run-tracking.md`. Then **big-run attempt 2**: checklist
`docs/verification/big-run-watch-set.md`, read the SBUX-shape engine targets
and `data-health` first, keep the Ollama server log deliberately.
