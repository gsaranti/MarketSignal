# Current session handoff

## What happened

**Planned, implemented, reviewed, and shipped the FRED freshness-guard promotion** — resolving the carried "FRED freshness tuning" item. The guard (`Cadence` / `FRESHNESS` / `cadence_for` / `max_staleness_days`, Daily 16 / Weekly 21 / Monthly 110 / Quarterly 230) had lived **only inside the `#[ignore]`d `fred_baseline_smoke`**; production `observations_to_quote` took the latest numeric observation with **no date check**, so a frozen/discontinued series (the `NASDAQVOLNDX` class — still resolves, returns a months-old value) **silently fed a stale level into the Step-3 baseline**. Promoted the machinery into production: `observations_to_quote` now dates the latest numeric observation against its cadence (`today` **injected**, mirroring `releases_to_calendar`) and **drops a too-stale series to `Ok(None)` → `GapReason::Unavailable`** — reusing the existing all-gaps path, **no new variant** (the user's call), so it counts against the coverage floor. **Fail-closed** on an unparseable latest-observation date; `cadence_for` falls back to the tightest (`Daily`) bound so the fail-soft scan never panics (parity test backs the fallback). Bounds **unchanged** (still uncalibrated); added the `#[ignore]`d **`tuning_freshness_headroom_probe`** (report-only) to re-tune from live lag. Metis review → **approve** (all 8 criteria, both gates re-run). Merged to **`main` @ `c4c778b`**, pushed to `origin/main`, branch deleted.

## Current state

**Clean and shipped.** HEAD = `c4c778b`, working tree clean, `main` in sync with `origin/main`. Nothing in flight. Verified green: `cargo test` (**348 lib**, +5 vs prior 343 — 4 freshness unit tests + 1 `fetch_series` stale round-trip; probe `#[ignore]`d), `cargo clippy --all-targets --all-features` (warning-free). Rust-only change, so `npm` gates weren't in scope. **`BUILD.md` reconciled** — its data-sources adapters bullet (which had been *silent* on FRED freshness, not stale) now documents the production guard: the cadence bounds (Daily 16 / Weekly 21 / Monthly 110 / Quarterly 230), the drop-to-`None` → `Unavailable` posture, the fail-closed/fail-tight calls, and the `tuning_freshness_headroom_probe`.

## Open questions

- *(new, low)* The four `max_staleness_days` bounds are **uncalibrated judgment values, now in production**. `tuning_freshness_headroom_probe` reports live headroom; re-tune only if it shows thin/negative headroom on a *legitimate* (non-discontinued) series.
- *(new, low)* `latest_numeric_observation_date` (test helper) is a documented **test/prod twin** of `observations_to_quote`'s selection logic; a future refactor could have the smoke exercise production directly.
- *(carried, residual)* A **chars-dropped *ratio*** (Σ dropped / Σ original) is still not built — needs a `total_original_chars` column on `document_parse_runs`. Still gates the deferred GPT-5-mini extraction stage.
- *(carried, low)* `INDUSTRY_PE_MAX = 100.0` uncalibrated — revisit only if a legit aggregate near the ceiling shows up live.
- *(carried, low)* **#2** — independent self-cap so truncation history outlives the 30-report window — dropped; revisit only if a longer evidence window is wanted.
- *(carried, low / parked)* calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide; esbuild/vite advisory.

## Where to start

Nothing owed — `main` is clean and pushed, **FRED freshness** is resolved (guard enforced in production), and **`BUILD.md` is reconciled**. Pick a next slice: the **chars-dropped ratio** (add `total_original_chars` to `document_parse_runs`), or the carried **calendar `expected` consensus**.
