# Current session handoff

## What happened

**Planned, implemented, reviewed, hardened, and shipped the truncation *rate*** — closing last session's most substantive carried item (the diagnostic gave absolute counts only, no derivable overflow rate). New accumulating **`document_parse_runs(report_id, captured_at, docs_parsed)`** denominator table — a **separate table cascade-bound by `report_id`** (not a `reports` column: avoids an ALTER migration, mirrors the `document_truncations` precedent), written once per run with a non-empty inbox **even when nothing truncated**. Aggregated as `TruncationStats.total_docs_parsed`; Settings renders **"X of Y (Z%)"**. A **5th cascade leg** in `delete_report_db_rows` keeps numerator and denominator on the same retained-report window. **#2 (independent self-cap so truncation history outlives the 30-report cascade) was deliberately dropped** during planning as the safer/easier call — retention stays cascade-only. Metis review → **approve-with-nits** (the >100% render edge); folded in (rate withheld when numerator > denominator). An **external Codex pass** then found a real cohort gap both reviews missed (they checked cascade/*deletion* alignment, not *creation*-time): legacy truncation rows recorded before `document_parse_runs` existed have no denominator → a mixed-cohort rate the of-0/>100% guards don't catch. Verified against code **and the real DB** (which predates both tables, so the gap is **dormant** — they co-evolve via `CREATE IF NOT EXISTS`). Hardened anyway: reader exposes **`unaligned_truncations`** (anti-join count), frontend **withholds the rate while > 0** (bare count), self-heals as legacy rows age out. Squash-merged to **`main` @ `a62ffae`**, pushed to `origin/main`, branch deleted.

## Current state

**Clean and shipped.** HEAD = `a62ffae`, working tree clean, `main` in sync with `origin/main`. Nothing in flight. `BUILD.md`'s truncation paragraph was reconciled to the shipped rate + denominator table + cohort guard. Verified green: `cargo test` (341 lib + all integration), `cargo clippy --all-targets --all-features` (warning-free), `npm run build` (vue-tsc + Vite), `npm test` (Node 38/38, Vitest 86/86).

## Open questions

- *(residual, this slice)* A **chars-dropped *ratio*** is still not built — the rate answers "what share of docs truncated", not "how much content lost". Would need a `total_original_chars` column on `document_parse_runs`. Volume/trend still gates the deferred GPT-5-mini extraction stage (first lever stays raising `document_parser` caps 12k/40k).
- *(resolved-but-noted)* The cohort-misalignment rate hazard is **handled** (`unaligned_truncations` suppression), not open — listed so it isn't re-flagged as a bug.
- *(carried, low)* **#2** — independent higher cap so truncation history outlives the 30-report window — *deliberately dropped this session*; revisit only if a longer evidence window is wanted.
- *(carried, low / parked)* FMP free `industries`-P/E clamp (e.g. pe=461); FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide; esbuild/vite advisory.

## Where to start

Nothing owed — `main` is clean and pushed, `BUILD.md` is reconciled, and the truncation-rate thread (last session's top carried item) is **resolved**. Pick a next slice — the **chars-dropped ratio** (add a `total_original_chars` column to `document_parse_runs`), or a carried low item (FMP `industries`-P/E clamp; FRED freshness tuning).
