# Current session handoff

## What happened

**The Stooq-removal slice shipped and pushed** (`cbc6dca` direct to main;
internal reviewer approve, one Codex round to approval). The stooq module and
wiring are deleted; FMP dated-EOD is the only deep-price rung for the
per-holding history and the outcome pass alike. The `^spx` vs `^GSPC` open
resolved to **rename** — episodes never persist the market-benchmark symbol
(verified), so the only churn was the price-bar cache, cleaned by an
idempotent `DELETE ... WHERE symbol = '^SPX'` at store init. The
`OutcomePriceSource` trait stays as the test seam with an FMP-only live impl.
Data-health dropped `deep_history_fallbacks` (old rows decode; serde ignores
the stray key) — **any deep-history failure now trips attention**, stricter
than attempt 1's recovered-fallback leniency. The Codex round caught what the
decision record's inventory missed: `logic-flow-docs/` (13 live references,
now fixed), the source matrix's FMP cells, and two stale keyless-price claims.
The benchmark / sector / commodity identity table now lives at
`docs/data-sources.md §Financial Modeling Prep` as FMP symbols (`^GSPC`,
plain SPDRs, `GCUSD`/`SIUSD`/`HGUSD`). BUILD/INDEX aligned in-session
(user-run): the slice sits in §Built, the big run is queue item 1.

## Current state

Nothing in flight. `main` pushed at the slice commit plus this session-end
metis commit. Gate at close: **1067** lib + 32 integration cargo tests
(down from 1072 — the stooq module's tests went with it; one store-cleanup
test added), clippy clean at `--all-targets --all-features`, `npm run build`
clean, 46 node + 241 vitest.

Behind attempt 2, unchanged: digest compression (candidate 3, doubly
instrumented) and the "declined an engine exit" vocabulary, both waiting on
run evidence. `NUM_PREDICT_*` values remain drafted, uncalibrated. The
`SIUSD`/`HGUSD` commodity endpoint shapes verify at TO build time (recorded
in data-sources.md; gold live-verified).

## Open questions

- **Were attempt 1's engine targets degenerate?** The one sample (SBUX) was
  steeply bearish, not flat; attempt 2 reads whatever persists first.
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant rests
  on the 2026-07-16 verification, not re-probed.

## Where to start

**Big-run attempt 2** — nothing stands in front of it. Checklist
`docs/verification/big-run-watch-set.md`, whose retired Stooq lines are now
the two FMP-only watches: quota consumption under the full price load, and
429-ladder engage/recover. Read the SBUX-shape engine targets and
`data-health` early, keep the Ollama server log. Expect two shifts from the
slice: suite rows that read `ok` on attempt 1 may now honestly read `empty`
or `malformed`, and any deep-history failure trips the attention flag.
