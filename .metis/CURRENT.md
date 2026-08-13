# Current session handoff

## What happened

Two slices shipped and pushed this session. **The Stooq-removal slice**
(`cbc6dca`; internal approve + one Codex round): FMP dated-EOD is the only
deep-price rung everywhere, the market benchmark renamed to `^GSPC` (episodes
never persisted the symbol; the dead `^SPX` cache key is cleaned at store
init), data-health's fallback counter retired — **any deep-history failure now
trips attention** — and the identity table re-homed to `docs/data-sources.md
§Financial Modeling Prep` as FMP symbols. **The thought-log capture slice**
(`f2254d7`; internal approve-with-nits applied + three Codex rounds): a
`ThoughtLogSink` decorates the live reporter at `live_run_context` and appends
every thinking stream (per-holding, construction, main-agent, per-analyst) to
per-run text files under `<data-dir>/thought-logs/<UTC stamp>-<id8>/`.
Debug builds capture by default, release silent unless
`MARKET_SIGNAL_THOUGHT_LOG` opts in; keep-10 retention prunes only after a
run's first delta lands (a failed or thought-less capture spends no old log),
exact-shape prune guard, synchronous unbuffered appends by design
(crash-honesty). Docs home: `run-tracking.md §Thought-log capture`; BUILD's
observability section and one INDEX row updated in-session (user-run OK).

## Current state

Nothing in flight. `main` pushed at `f2254d7` plus this session-end metis
commit. Gate at close: **1078** lib + 32 integration cargo tests, clippy clean
at `--all-targets --all-features`, `npm run build` clean, 46 node + 241
vitest.

Behind attempt 2, unchanged: digest compression (candidate 3, doubly
instrumented) and the "declined an engine exit" vocabulary, both waiting on
run evidence. `NUM_PREDICT_*` values remain drafted, uncalibrated.
`SIUSD`/`HGUSD` commodity shapes verify at TO build time.

## Open questions

- **Were attempt 1's engine targets degenerate?** The one sample (SBUX) was
  steeply bearish, not flat; attempt 2 reads whatever persists first.
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant rests
  on the 2026-07-16 verification, not re-probed.

## Where to start

**Big-run attempt 2** — nothing stands in front of it. The setup protocol is
agreed (user-ratified this session), split by who can do each step:

1. **Agent:** verify/start the pinned Ollama daemon with its server log teed
   to a file (the log must exist before the first model call), spin up the
   dev app under `caffeinate`, confirm the app is on the `dev/` store, then
   hand off and wait.
2. **User:** import the latest prod export into the dev app (freshest
   reports → house view quality; the archive never carries settings or
   Keychain, by design).
3. **User:** verify Schwab and clear the Keychain ACL prompts (they stack
   before first paint on a fresh debug binary; the 7-day refresh may want a
   re-login), reach the Portfolio page, then say go.
4. **Agent:** initiate the Portfolio run from the GUI (drive by process name
   `market-signal`, never `tell application`) and monitor tracker + stderr
   tee to a terminal state.
5. **At the end the agent does NOT open or analyze any thought-log file** —
   notify completion (or failure with tracker/tee evidence) and stop;
   output analysis is a separate, user-initiated step.

Run references: checklist `docs/verification/big-run-watch-set.md` (FMP
quota-consumption and 429-ladder watches replaced the retired Stooq lines);
read the SBUX-shape engine targets and `data-health` early. Thought logs
capture automatically in the dev app — the analysis step afterward covers at
least five per-holding files plus `construction.txt` for model-quality and
prompt-structure reads. Expect attempt-1 `ok` suite rows to honestly read
`empty`/`malformed`, and any deep-history failure to trip attention.
