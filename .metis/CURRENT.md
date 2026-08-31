# Current session handoff

## What happened

Two slices landed this session, both off the 2026-08-30 big-run findings.
**Finding 3** (action-call prompt clarity, `portfolio-v32`, `938fa31`): the action
prompt now says the app app-stamps a departure from the ENGINE SET (the model emits
only the rung + one-sentence rationale) and that a `clears`/`indeterminate`
capital-efficiency read is neutral (closing the observed soft sell-lean).
Prompt-prose only, no schema change; two Codex rounds. **Finding 3 was the last
code candidate** in the big-run findings record.
Then, off a design discussion, the **failure-isolation slice** (`ac94f2f`): a hard
per-holding model / grade failure no longer fails the whole Portfolio run — it is
isolated into a run-level `failed_holdings` list rendering a failed card (prior
verdict carried vintage-stamped where one exists, empty debut card where not), the
run continuing; the run fails outright only run-level (persistence, rate anchors) or
when *every* attempted holding fails (no snapshot — prior run stays latest). This
**flipped a documented Portfolio failure-posture contract** — the cloud report's
analyst layer stays fail-hard; only the Portfolio job's per-holding half changed.
Four Codex rounds; BUILD/INDEX updated; no PROMPT_VERSION/stamp moved. Canonical at
`portfolio-analysis.md §Failure posture`, record at
`verification/2026-08-31-portfolio-failure-isolation.md`.

## Current state

Nothing in flight; everything committed and pushed (both slices plus the
`OLLAMA_NUM_PARALLEL=1` daemon-launch note in `local-model-operations.md`).
The big-run re-attempt gate stays **met** and a re-attempt stays the user's call,
from a wiped store per `big-run-watch-set.md`.
**Findings 4–6 remain and are all gated on a full run's telemetry, not code** —
4 (fired bounded-retry rate), 5 (throughput / per-holding research budgets, deferred
to the run), 6 (extraction telemetry for the deferred render tier). With Finding 3
done, **no code candidate remains** in the big-run findings record.
Noted follow-ups: `OLLAMA_NUM_PARALLEL=1` is a verify-on-next-run daemon pin (not
app code); an all-attempted-failed run persists no snapshot (a diagnostic snapshot
is a small follow-up if ever wanted); SKILL/README now record a **fifth** design
accent relaxation (the failed-status tag, `--accent-text`). The prior unrelated
carried follow-ups remain untouched — cloud `run_job` seam, negative composite
yield, `progress.rs` poisonable locks, tracker `ok` dropped count, TO logic-flow
line 397, 600 s `/api/tags` backstop, whole-ledger seed injection, qualitative 6g
un-trip, IPv6-loopback wire test, the audit sources line, the unreconciled-delete
fail-soft sentence's home.

## Open questions

- Whether and when to re-attempt the big run — the user's call; gate met, don't
  propose unprompted.
- Whether a second full pass runs — the user's call after a first full run (none yet).
- Findings 4–6 all want a full run's telemetry first; no code candidate is left to
  take ahead of the run.

## Where to start

The big-run findings' **code queue is clear** (Findings 1–3 done); 4–6 need a full
run's data, not code. So the natural next item is the big confirmation run itself —
but that is the user's decision; **do not propose re-running unprompted**. If
continuing without a run, pick from the unrelated carried follow-ups in *Current
state*. On the next run, verify `OLLAMA_NUM_PARALLEL=1` on the daemon and confirm a
real per-holding failure isolates as designed (failed card + carried prior).
