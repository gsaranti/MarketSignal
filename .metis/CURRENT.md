# Current session handoff

## What happened

**Open-questions sweep — five resolved, three commits pushed to `origin/main`** (`1289d5f..7947f31`).

- **Appearance persistence**: kept in **localStorage** (not `app_settings`) by decision — synchronous pre-mount read gives a zero-flash launch, and it has no backend consumer. The deliberate exception to "config lives in SQLite" is now documented in `BUILD.md` (`7947f31`).
- **`--accent-text` (dark)** dialed `#D28A99 → #CE8090` (`aa47bf9`) — deeper oxblood, closer to the light token; still clears AA on every dark surface (4.95–5.99).
- **FMP bad-key path live-verified** (`cdda6a0`): invalid/empty/missing keys return **HTTP 401 + `Error Message` body** on both `/stable/` and `/api/v3/` — *not* the 200-with-error-body the comments assumed — and are handled correctly by the status branch. Comments corrected; finding locked in as `fmp_401_with_error_body_is_a_failure`. The 200-body branch stays as defensive cover for FMP's plan/rate-limit conditions.
- **Jun-3 failed job**: benign — a `sk-bogus…test` placeholder key → 401, then a real-key success 4 min later. It *validates* the failed-job path (401 body in `job_runs.detail`, state `failed`, surfaced in warnings). No fix.
- **Settings test-saved vs test-before-save**: settled as **test-saved** — the backend reads the saved key (the typed secret never crosses the invoke boundary) and the UI gates Test off while the field is dirty. Coherent; no change.

Verified throughout: `cargo test`, `cargo clippy --all-targets --all-features` (warning-free), `npm run build`.

## Current state

On **`main`** at **`7947f31`**, **pushed** (up to date with `origin/main`), working tree clean. **Nothing in flight.**

## Open questions

- **No Vue component-test harness** *(deferred)* — stand up Vitest + Vue Test Utils when wanted; nothing is blocked on it.
- *(parked for plan-task)* **retention-cascade enforcement** (30-report cap + cascade delete, durable-learnings survival) and **step-5 auto-archive** — real build slices, not quick checks.
- *(carried, low priority)* report-body rendering fidelity ceiling; PDF `@page` margin fidelity.

## Where to start

Pick the next build target → `/metis-plan-task`. Per `BUILD.md`'s "immediate next slices": the **FMP/FRED/BLS data-source adapters** are the next major build (FMP groundwork already exists in `connection_test.rs`); **retention-cascade enforcement** is the smaller, self-contained option.
