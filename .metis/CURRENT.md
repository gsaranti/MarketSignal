# Current session handoff

## What happened

**The step-ownership slice shipped and pushed** (`067de1a` direct to main;
internal reviewer approve-with-nits, then five Codex rounds to approval).
Request-row ownership is now stated, not inferred: `RunContext::emit` stamps
every request event with the run's active step (a single cell — set by
`step_started`, cleared only by its matching `step_finished`; the analyst
trio's concurrency lives inside one step, so the cell is exact, and emitter
call sites are untouched). The tracker attaches rows by the stamp — group
routing, follow-the-running-step, and phantom-step synthesis are retired;
unowned rows render in a neutral "requests outside any step" list; the
run-finished reconcile mirrors the terminal status, so a successful run closes
a still-running step `ok`, never FAILED. All 19 FMP suite rows ride
`suite_get_shaped`: status decided after the caller's parse (ok only when
usable data landed / empty / malformed with cause-on-row). The Codex rounds
hardened the boundary: non-array bodies, wholly-unreadable arrays, and
undatable date strings read malformed; served-empty `[]` reads empty
(quote, etf/info); drifted earnings/news/deep-EOD bodies return `Err` so the
quick check types the family unknown; accepted non-zero-padded dates
normalize to canonical ISO before any lexicographic consumer. The news guard
splits readability from its domain date filter. `run-tracking.md` carries the
contract.

**Stooq removal ruled** (user decision 2026-08-12, superseding the rung-order
slice — BUILD §What remains item 5): Stooq is untestable behind its JS-PoW
wall and must not resurrect untested in production; FMP dated-EOD becomes the
only price rung (200/min + the 63 s ladder cover the load; light-EOD probed to
≥1985 depth with a 5,000-row/request cap against a ~1,100-row ask). Evidence,
full code/docs inventory, and plan-time opens:
`docs/verification/2026-08-12-stooq-removal-decision.md`.

## Current state

Nothing in flight. `main` pushed at the session-end commit (the slice at
`067de1a`, then the removal record + this handoff). Gate at close: 1072 lib +
32 integration cargo tests, clippy clean at `--all-targets --all-features`,
`npm run build` clean, 46 node + 241 vitest.

Behind attempt 2, unchanged: digest compression (candidate 3, doubly
instrumented) and the "declined an engine exit" vocabulary, both waiting on
run evidence. `NUM_PREDICT_*` values remain drafted, uncalibrated.

## Open questions

- **Stooq-removal plan-time opens** (owned by the record's §Open at plan
  time): keep `^spx` as the abstract market-benchmark identity vs rename to
  `^GSPC` (persisted-episode references decide); the FMP-only watch items
  replacing the watch set's two retired Stooq lines; whether the outcome
  `daily_closes` trait seam stays as the test seam.
- **Were attempt 1's engine targets degenerate?** The one sample (SBUX) was
  steeply bearish, not flat; attempt 2 reads whatever persists first.
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant rests
  on the 2026-07-16 verification, not re-probed.
- **INDEX.md rows** offered, not ruled: the degraded-run/constructed-marker
  concept, and a §Verification records row for the new Stooq-removal record.
- **BUILD.md** still lists the superseded item 5 and names Stooq in the
  suite's engine source list — update wants a user-run edit.

## Where to start

Plan the **Stooq-removal slice** via `/metis-plan-task` against
`docs/verification/2026-08-12-stooq-removal-decision.md` — the record carries
the decision, the nine built-code sites with replacements, the docs surface
(including the single-homed benchmark/futures identity table that must move
out of `data-sources.md §Stooq`), and the plan-time opens. Build it, then
**big-run attempt 2**: checklist `docs/verification/big-run-watch-set.md`
(its two Stooq lines retire with the slice), read the SBUX-shape engine
targets and `data-health` first, keep the Ollama server log — and expect
suite rows that read `ok` on attempt 1 to now honestly read `empty` or
`malformed`.
