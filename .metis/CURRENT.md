# Current session handoff

## What happened

**The fail-soft DB-read fold shipped and merged** — the adjacent follow-up the last handoff flagged. The pipeline's two remaining hand-rolled fail-soft reads now share the shell that already backed the recent-report loaders: `read_recent_fail_soft` was **renamed `read_db_fail_soft`** (no longer recent-report-specific) with a neutral `"{context}: degraded to empty"` message, and the two siblings delegate to it. `retrieve_memory` keeps its pre-open empty-query guard and `bounded_query` cap **outside** the shell, so the guards and the paid-call skip are unchanged (its five tests pass as-is). `compute_prior_deltas` returns `Option<BaselineDeltas>` via the `<T: Default>` bound (`None` is its default) — **one behavioral delta:** a DB/decode failure now degrades to a *logged* `None` where it was previously silent (an absent prior snapshot stays the silent, expected `None`); added one degrade-arm test. Full loop ran — planned → implemented → reviewed (`metis-task-reviewer` **approve**, no nits) → **also Codex-reviewed**: two P3s, the memory stderr colon-shift (inherent to the shared template, operator-only — no code change) and a stale BUILD.md symbol name (fixed by the doc edit). Squash-merged to **main @ `9378624`**, pushed to `origin/main`. `cargo test` 348 pass / 14 ignored + `cargo clippy --all-targets --all-features` clean. BUILD.md updated to record the rename + broadened scope (explicit one-time OK).

## Current state

On **main @ `9378624`**, synced with `origin/main`, **nothing in flight**. The feature (`pipeline.rs` + `BUILD.md`) is committed and pushed; working tree is otherwise clean. The only uncommitted edit is this `CURRENT.md` rewrite — the usual "metis session end" commit is yours to make. The fail-soft-read consolidation idiom now backs **all three** pipeline reads, so no fold candidates remain.

## Open questions

- *(carried)* tuning bundle (`MEMORY_TOP_K=5`, `LEARNINGS_PER_REPORT_CAP=5`, `LEARNING_DEDUP_THRESHOLD=0.93`, `MAIN_AGENT_RECENT_REPORTS=3`, `RECENT_REPORT_BODY_CAP=12_000`, inbox caps, `COVERAGE_FLOOR=0.6`) unvalidated vs real `text-embedding-3-large` geometry; needs a separating test embedder (`BasisEmbedder`/`DistinctEmbedder`) since `StubEmbedder` collapses distinct prose to ~1.0 cosine.
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; esbuild/vite advisory parked; wiremock / in-loop offline gap; conditional GPT-5-mini extraction stage; optional ` ```chart ` doc note.
- *(carried, non-blocking)* two SFC sub-branches left unasserted: `JobTrackerView`'s `"Generating report"` headline fallback and `PersistentWarningArea`'s empty-state `is-current` marking.
- *(low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide.

## Where to start

Nothing owed — the fold arc is complete, merged, and BUILD.md is current. First, commit this `CURRENT.md` as the session-end commit. Then best-fit low-effort items: the ` ```chart ` **doc note** or the two **unasserted SFC sub-branches** (test-only). Heavier: **tuning-bundle validation** (build the separating test embedder first).
