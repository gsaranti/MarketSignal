# Current session handoff

## What happened

**The big confirmation run was attempted and failed at Step 7b**, after 2 h 46 m and a completed
per-holding pass, with 44 self-coherence violations across 38 symbols. **Nothing persisted** —
checkpoint/resume is unbuilt, so every verdict, ledger, overlay and episode was discarded. Full
analysis: `docs/verification/2026-08-10-big-run-attempt-1.md`.

**Root cause is output-budget exhaustion, not a validator defect.** `num_ctx` 131,072 covers input
*and* output, and `num_predict` is set nowhere in the crate. The model said so in its own reasoning
("Token limit is tight if I output 60+ objects with full detail") and emitted a "representative
corrected set", dropping the attribution fields the validator enforces. **The named-violation re-run
makes this worse, not better** — it resends the full plan plus the violation list, so the recovery
path has a bigger prompt and less output room than the attempt it rescues.

**The attribution baseline is the *prior run's* action** (`construction.rs:1345`), not this run's
engine read. 21 of the 26 unattributed moves are off `sell-all`, and 2026-07-31 recorded 36 sell-alls
of 44 priced. A degenerate run therefore taxes the next run that disagrees with it — a coupling worth
keeping in view when scoping the fix.

**Five further findings** came from the reasoning panes, each verified against code: the response
schema is enforced but never declared (the model re-derives keys and fence-or-not every call, on the
same shared budget); `"pre-v7 run"` leaks build vocabulary into the prompt; the holding header omits
units and often the company name (`HOLDING: PSX ()`); the engine arm shows a lean *set* but no pick;
and Portfolio request rows fall through `App.vue`'s report-pipeline routing into a frontend-synthesized
"Baseline market data" step that no backend ever finishes.

**User ruling: `BUILD.md` stays forward-looking** — what we are building toward, not what needs fixing.
Fix queues live in the verification record. BUILD gained only the gating fact on §Remaining item 1.

## Current state

Nothing in flight. **The run is blocked on the 7b repair** — a repeat attempt would hit the same wall
on a more constrained path.

Fix candidates are scoped with code references in the record, in recovery-value order: persist a run
whose construction fails (`roll_up.construction`/`.aggregates` are already `Option`, so the record
shape already admits it — but this contradicts §Step 7b and needs a ruling first); send the re-run only
the violating names; compress the construction digest; reserve output explicitly. The prompt fixes are
all in `pipeline.rs` and cheap; the routing fix is one default in `App.vue:196`.

**Confirmed despite the failure:** 128 K runner stability across 2 h 46 m — one runner, 131072, 100 %
GPU, `Forever`, no reload or spill. 98 % of wall-clock was model time (95 chat calls).

**Dev DB is deliberately unchanged** — still only the 07-31 run, whose pre-`grade-v2.1` stamp is the
only input exercising the band-recalibration continuity path.

**Session-scoped evidence will be lost:** 120 tracker captures, the Ollama server log (the most durable
artifact a failed run leaves), and `analyze-run.sh` — the 13-section watch-set analyzer, already
smoke-proven against the v2 run. Copy out of the scratchpad if any should survive.

## Open questions

- **Should persisting 7b incoherence stop being run-terminal?** §Step 7b currently ties it to a hard
  model failure, so persisting a degraded run is a spec change, not just a code change.
- **Should the current engine arm's chosen stand-in be shown at Step 6f?** The prior run's pick is
  rendered but this run's is not; showing it may anchor the model arm against `portfolio-v7`'s intent.
- **Were this run's engine targets degenerate?** The one sample (SBUX) was steeply *bearish*, not flat
  — 12m base 34 % below spot — which is a different shape from the 07-31 flat-target syndrome. Nothing
  persisted to settle it.
- **Is the FMP dated-EOD rung de facto primary?** Still unresolved: `data-health` never persisted.
- **Live-evidence caveat** — the sector-P/E walk-back's "holidays serve carried values" warrant rests
  on the adapter's recorded 2026-07-16 verification, not re-probed.

## Where to start

Rule the **persist-on-failure question** first — it is the one change that converts a failed attempt
from a total loss, and everything else is cheaper than re-running blind. Then take the output-budget
fix (re-run only violating names, compress the digest), the `pipeline.rs` prompt fixes, and the
`App.vue` routing fix. Only re-attempt the run once 7b can carry a whole-book plan at book scale; read
`data-health` early when it happens.
