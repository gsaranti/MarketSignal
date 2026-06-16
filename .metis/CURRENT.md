# Current session handoff

## What happened

**Planned, implemented, reviewed (Metis + external Codex), and shipped the chars-dropped *ratio***, resolving the carried truncation-telemetry residual. Shape: a **nullable `total_original_chars`** column on `document_parse_runs` (Σ original chars over **all** parsed docs, truncated or not — the ratio denominator), added by an **idempotent `PRAGMA table_info`-guarded `ALTER`** so pre-existing DBs migrate (no `user_version` framework; NULL marks the pre-migration cohort, distinct from a real `0`). `TruncationStats` gains `total_original_chars` + `parse_runs_missing_original_chars` (the NULL-column cohort guard); the pipeline persist sums `inbox_docs` original chars; Settings renders **"X of Y (Z%)"** with a withhold guard. **Load-bearing learning the next session must keep:** the chars guard needs **both** cohort arms — `unaligned_truncations` (a truncation whose report has *no* parse-run row → numerator without denominator) **and** `parse_runs_missing_original_chars` (a NULL-chars row). The first arm was **missed by both me and the Metis reviewer, caught by Codex** — the original 3-arm guard was wrong; don't reintroduce it. Squash-merged to **`main` @ `ae28115`**, pushed, branch deleted.

## Current state

**Shipped and reconciled.** HEAD = `ae28115`, working tree clean, `main` in sync with `origin/main`. Nothing in flight. Verified green: backend `cargo test` **350 lib** (+2 vs 348 — migration + NULL-cohort tests) + all integration suites, `cargo clippy --all-targets --all-features` warning-free; frontend `npm test` **88 Vitest** (+2) + 38 node, `npm run build` clean. **`BUILD.md` reconciled** — its truncation-rate (adapters) bullet now documents the chars-ratio slice (`ae28115`): the nullable `total_original_chars` column, the guarded-`ALTER` reversal of the predecessor's separate-table call, and the two cohort guards. (`storage.rs` comments were already current — the stale framing only lived in `BUILD.md`.)

## Open questions

- *(new, low)* This slice took an **`ALTER TABLE ADD COLUMN`**, reversing the truncation-rate slice's deliberate "separate table to *avoid* an ALTER" call. A single additive nullable column is the one schema change SQLite makes without a table rebuild — recorded so the precedent is legible.
- *(carried, low)* FRED: the four `max_staleness_days` bounds uncalibrated (`tuning_freshness_headroom_probe` reports live headroom); `latest_numeric_observation_date` is a documented test/prod twin of `observations_to_quote`.
- *(carried, low)* `INDUSTRY_PE_MAX = 100.0` uncalibrated — revisit only if a legit aggregate near the ceiling shows up live.
- *(carried, low)* Truncation **#2** (independent self-cap outliving the 30-report cascade) dropped — both telemetry tables stay cascade-only; revisit only if a longer evidence window is wanted.
- *(carried, low / parked)* calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide; esbuild/vite advisory.

Note: the chars ratio shipping makes the **truncation telemetry feature-complete** — the deferred GPT-5-mini extraction stage now waits purely on accumulated live evidence, no more telemetry plumbing owed.

## Where to start

Nothing owed — `main` is clean and pushed, the chars ratio is **shipped**, and **`BUILD.md` is reconciled**. Pick a next slice from the carried list: **calendar `expected` consensus** or **GDP annualization** are the best-shaped.
