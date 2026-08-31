# Portfolio per-holding failure isolation

A robustness slice for the big run: a hard per-holding model / grade failure no longer fails the whole Portfolio run.
It is isolated — the holding is recorded failed, its prior verdict carried where one exists, and the run continues — so a single flaky call on a ~20-hour, 47-holding run leaves 46 good cards and one failed card rather than halting the run and requiring a manual resume.
The contract is canonical at [portfolio-analysis.md §Failure posture](../portfolio-analysis.md#failure-posture); this record is the slice's dated evidence.

## Motivation

Attempt 3's finding work surfaced that the Portfolio job was **fail-hard per run**: `analyze_holding(...)?` propagated any per-holding model failure out of the loop, so one holding's double-failure on a hard-path call (interpretation / action) would fail the entire run.
On the big run — one holding every ~25 minutes — that turns a single transient into a halted overnight run needing a babysit-and-resume.
The user proposed isolating the failure into a failed card the user can re-run.
The fail-hard posture's original justification was *visibility* (a holding silently dropped from the book); an explicit failed badge satisfies that justification a different way, so isolation does not fight the rationale.

## The ruling (2026-08-31)

A hard **per-holding** model / grade / persistence failure on the required 6c–6f path is **isolated, not run-fatal**.
The run fails outright only on a **run-level** failure (the Schwab pull, the rate anchors, the run-persistence write) or when **every attempted holding fails**.
This is a deliberate change to the documented failure posture — recorded here and in the canonical §Failure posture, not silently applied.

## User decisions (ruled before implement)

- **Representation** — a failure is a run-level `failed_holdings: Vec<HoldingFailure>` on `PortfolioRun`, **not** a new `VerdictDisposition` variant.
  Carried data comes from the existing carry-forward path when a prior verdict exists, so the roll-up and two-arm scoreboard are nearly untouched (a debut failure is simply absent from the analyzed counts, like a not-analyzed holding).
- **All-attempted-failed → the run records `Failed`** and persists **no snapshot**, so the prior good run stays the latest view rather than being demoted by an all-stale / all-failed one.
- **The failed card surfaces a concise stage + cause** (the failing operation plus its root cause), while the full error chain rides the run tracker's failed step and stderr, not a bare "analysis failed".
- **A holding with a prior successful verdict shows that data carried and vintage-stamped beside the failed badge**, not emptied; a debut failure (no prior) renders an empty failed card naming the cause.

## Build inventory

Backend (`src-tauri/src/portfolio/`):

- `HoldingFailure { symbol, cause, carried_prior }` and `PortfolioRun.failed_holdings` (`mod.rs`); `PortfolioRollUp.failed_count`.
- The loop `Err` arm (`job.rs`): re-checks `is_cancelled()` first so a mid-holding cancel stays a `Cancelled` run (never a failed card), records the concise cause on the card while the full chain goes to the tracker's failed step and stderr, keeps the drained prompt-fit usage but **drops the failed holding's retry events** (the `model_retries` aggregate carries the "every listed retry succeeded" contract), marks the step failed, and continues.
- A shared `carry_prior_verdict` helper (`job.rs`) extracted from the selective carried-tail path and reused by a post-loop failed-carried pass, so a failed holding's prior verdict gets the identical carry treatment (vintage stamp, position delta, side-reversal badge, over-age add-family demotion) — the pass runs **after** the fresh-pass vintage stamp so the carried vintage is preserved.
- The all-attempted-failed guard: an empty `verdicts` with recorded failures fails the run (returns `Err`), so it records `Failed` and persists no snapshot.

Frontend (`src/`):

- `HoldingFailure` + `failed_holdings` / `failed_count` types (`types.ts`).
- `failedBySymbol` / `failureFor` / `failedBadgeTitle` and the `Failed` key figure (`PortfolioView.vue`); an **analysis-failed badge** on carried cards, a **visible, accessible failure note** on carried cards (the cause + re-run guidance, so keyboard / touch / screen-reader users are not left with a hover-only tooltip), and a **failed placeholder** (badge + visible cause) for a debut failure.
- `.ana-tag-failed` — the failure badge — lives in the **design package** (`market-signal-design-system/colors_and_type.css`, beside the status-tag contract), recorded there as a deliberate fifth relaxation of "tags never use the accent": the failure is the one status that is both the worst outcome (grade-F oxblood, as the grade chip already uses) and an actionable one (re-run), and the visible words carry the meaning so the color is reinforcement, not the sole signal.

Tests:

- The four checkpoint/resume tests that had used a model failure to interrupt a run now interrupt via **cancellation** (a `CancelOnStep` reporter that flips the shared cancel flag at a named holding's step) — the realistic interrupt now that a model failure isolates; their end-state assertions are unchanged (the interrupt mechanism differs, the partial trail and data-health aggregates do not).
- New backend: `a_per_holding_failure_isolates_and_carries_the_prior_verdict`, `a_debut_holding_failure_isolates_with_no_carry`, `every_attempted_holding_failing_fails_the_run`.
- New `PortfolioView` "isolated failures" specs: carried badge + Failed figure, debut placeholder + visible cause, clean run shows neither; the base fixture now carries `failed_holdings` / `failed_count` so it models the real wire shape.

## Verification

- `cargo test` — full suite green (lib + integration binaries); `cargo clippy --all-targets --all-features` — clean.
- `npm run build` (vue-tsc + Vite) — clean; `npm test` — Node + Vitest suites all pass.

## Residue and watch

- No `PROMPT_VERSION` / stamp move — no prompt, schema, engine, or checkpoint-shape change; `failed_holdings` rides the existing `run_json` blob (pre-release, no serde default per the fresh-start-2 no-compat ruling — the dev store is wiped before a run, so every persisted run carries the field and reads it back).
- **All-attempted-failed persistence** is deliberately *no snapshot* (the prior run stays latest). If a diagnostic all-failed snapshot is later wanted, it is a small follow-up.
- A failed holding's prompt-fit usage is kept in data-health but its retry events are dropped (the succeeded-only contract); a retry that succeeded earlier in a since-failed holding is therefore under-counted, the safe side of that contract.
- The big-run watch set should confirm on the next run that a real per-holding failure isolates as designed (a failed card + carried prior), and that the run completes rather than halting.
