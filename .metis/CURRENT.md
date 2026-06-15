# Current session handoff

## What happened

**Built, reviewed, and shipped the truncation telemetry table** — the slice the prior session queued as the prerequisite to ever revisiting the deferred GPT-5-mini extraction stage. A new **accumulating** SQLite table `document_truncations` (one row per head-truncated inbox doc: `report_id`/`captured_at`/`name`/`format`/`original_chars`/`kept_chars`) is written best-effort in `generate_report`'s persist step, right after the baseline-snapshot block (`storage.rs`, `pipeline.rs`). It **appends** — no leading `DELETE` — the deliberate divergence from the replace-wholesale `research_parse_failures`, so overflow frequency accumulates across runs. Row derivation is the pure `collect_document_truncations` (filters `ParsedResearchDoc::truncated()`), the unit-test seam. Bounded **only** by the 30-report retention cascade (a new fourth leg in `delete_report_db_rows`'s transaction) — **no independent self-cap** like `baseline_snapshots`' 14, chosen to honor the accumulating intent. Reviewed twice: `metis-task-reviewer` **approve**; external Codex found one **Low** doc-drift (the `prune_old_reports` comment still said "three DB legs") — verified valid, fixed both stale spots. Squash-merged to `main` as **`d7eb644`** and pushed to `origin/main`.

## Current state

**Clean and shipped.** Feature complete, verified green (`cd src-tauri && cargo test && cargo clippy --all-targets --all-features`), merged and pushed. HEAD = `d7eb644`, working tree clean, `main` in sync with `origin/main`. Nothing in flight.

## Open questions

- *(new)* The telemetry table now **ships but has no reader/UI consumer** — the only inspection path is a raw SQL query against `document_truncations`. The extraction-stage revisit now waits on the table **accumulating real evidence** that overflow is common (not on more code). If that history must outlive the 30-report cascade window, an independent higher cap is the one-line follow-up.
- *(carried, low)* FMP free `industries`-P/E wire noise — decide whether to clamp/flag implausible P/E values (e.g. pe=461) or leave them to the agent's judgment.
- *(carried)* **Per-adapter offline round-trips** behind a base-URL injection seam — the deferred half of the http_retry coverage thread (each adapter's `interpret_response` is fixture-covered; the live wires remain the only coverage of adapter→retry→interpret).
- *(carried, low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide; esbuild/vite advisory.

## Where to start

Nothing owed — `main` is clean and pushed. Pick a next slice: **per-adapter offline round-trips** (needs the base-URL injection seam) is the most substantive carried thread; or a carried low-priority item (FMP `industries`-P/E clamp; FRED freshness). One bookkeeping flag worth clearing first: **`BUILD.md`'s adapters line still describes the truncation table as planned/gated** — it now ships, so consider revising it (see pending decisions).
