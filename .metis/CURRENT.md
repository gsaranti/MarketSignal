# Current session handoff

## What happened

**The pre-run slice shipped** (`b634c23`, direct to `main`, pushed) — everything queued
between the Step 7b repair and attempt 2. The **named-violation re-run is repair-scoped**:
corrected objects only for the violating names (narrowed holdings-only schema, the
first draft's envelope reused), overlaid onto the first draft and re-validated
**whole** (implied weights are book-coupled); non-spine keys drop deterministically,
scope is enforced both ways, and an unknown-key-only failure repairs with **no model
call**. **Degraded persistence now covers any 7b construction-call failure** (ruled
2026-08-11) — a parse failure is exactly what a truncation becomes — with a cancel
still leaving no row. Every portfolio stage sets **`num_predict`**; a
`done_reason: "length"` stop fails typed, the counts disambiguating
reservation-hit from context exhaustion, and the observation rides data-health.
**Adapter diagnostics** landed at the seams that already know: a stderr tee at
`RunContext::emit`, http_retry backoff lines with `without_url()` stripping (a
query-string API key can no longer reach a log, tracker row, or persisted job
detail — this also closed a pre-existing leak via returned error chains), suite
rows carrying gap reason + detail, a Stooq breaker-trip line. **Finding 5 ruled**:
the engine pick stays withheld at 6f and the prompt says so.

Review: internal approve-with-nits (all closed, incl. strict scope enforcement on
the overlay) plus one Codex round (key redaction, length-stop disambiguation,
strict full-call envelope decode via a dedicated `RepairResponse`) to approval.

## Current state

Nothing in flight. Working tree clean, `main` pushed at `b634c23`.

Two docs are now stale on this slice (user-run edits pending):
`docs/verification/2026-08-10-big-run-attempt-1.md` §Disposition still lists
candidates 2 and 4 and the §Residue diagnostics gap as open, and Finding 5 as
unruled; BUILD.md §What remains item 1 still says the
re-run-only-violating-names fix stands between here and a second attempt.

Behind attempt 2, unchanged: digest compression (candidate 3 — now doubly
instrumented: `record_usage` completion fields persist even on a 7b failure) and
the "declined an engine exit" vocabulary, both deliberately waiting on run
evidence. `NUM_PREDICT_*` values are drafted-calibratable, uncalibrated.

Verification gate at ship: 1074 cargo tests, clippy clean at
`--all-targets --all-features`, `npm run build` clean, 46 node + 231 vitest.

## Open questions

- **Were attempt 1's engine targets degenerate?** The one sample (SBUX) was steeply
  bearish, not flat. Attempt 2 should read its persisted targets first — any 7b-stage
  failure now preserves the evidence as a degraded run.
- **Is the FMP dated-EOD rung de facto primary?** Unresolved — read `data-health`
  early on the run; the Stooq breaker trip now also leaves a stderr line.
- **Live-evidence caveat** — the sector-P/E walk-back's "holidays serve carried
  values" warrant rests on the adapter's 2026-07-16 verification, not re-probed.

## Where to start

Update the two stale docs spots (user-run): the attempt-1 record's §Disposition
(candidates 2 + 4 built, diagnostics gap closed, Finding 5 ruled — all
2026-08-11, `b634c23`) and BUILD.md §What remains item 1. Then **run big-run
attempt 2**: read the SBUX-shape engine-target question from whatever persists
before anything else, read `data-health` early, and keep the Ollama server log
deliberately. Watch checklist: `docs/verification/big-run-watch-set.md`.
