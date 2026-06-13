# Current session handoff

## What happened

**HTML + extraction docs-amendment slice shipped** — squash `242c048` on `main`, pushed to `origin/main`. Aligned the `docs/` corpus, three source-comment files, and `.metis/` state (`BUILD.md`/`INDEX.md`) with two settled decisions: **(1) HTML is never persisted** — it is rendered on demand in the webview from canonical Markdown for display and PDF export, so it gets no SQLite column and no retention-cascade leg. This **resolves the long-standing "does persisted HTML need to exist?" open question** (answer: no; dropped from spec, amended from the original `legacy_docs` requirement, which is left intact as provenance). **(2) Research-inbox extraction is deterministic** — bounded excerpts cut at the nearest paragraph/line seam (falling back to a hard char cut), truncation always disclosed; no GPT-5-mini extraction stage runs, it is **reserved** as the conditional follow-on. Two Codex rounds resolved in-slice: `agents.md` first overstated the seam as "paragraph seams" (corrected to the real fallback ladder); then `pipeline.rs:451` + `storage.rs:26` source comments still framed persisted HTML as a *future* slice (corrected — HTML "for now"/"if a slice lands" framing removed, warning-state's deferred status left untouched). **Verification gap found & closed**: the task's closing grep was `docs/`-only, so it missed the source comments — the sweep is now run **repo-wide**.

## Current state

On **`main` @ `242c048`**, synced and pushed, **nothing in flight**. `cargo test` 333 passed / 0 failed / 14 ignored, clippy clean, `npm run build` OK. No live API spend this session. The slice spanned 8 docs/`.metis` files + 3 comment-only Rust files (`document_parser.rs`, `pipeline.rs`, `storage.rs`).

## Open questions

- **`SYNTHESIS.md` is stale** — reconcile-owned: still says SQLite stores HTML, cascade deletes HTML, a single `market_regime` 6-value vocab, and "17-step pipeline". Clear via `/metis-reconcile` (the main outstanding docs-drift item now that the corpus is amended).
- **GPT-5-mini extraction stage** — conditional follow-on: only if users actually drop docs > ~12k chars; seam ready (replace head-truncation for overflow docs, nothing else changes).
- **GUI visual pass of the inbox error-state row** — deferred (no Vue test harness); folds into the optional GUI/live run, which now also exercises inbox parse → archive → error states live.
- **Chart-block slice unplanned** — fenced ```chart blocks (direction settled): SYSTEM_PROMPT teaching, markdown-it fence renderer, design-system chart styling extension.
- **Learning dedup unbuilt**; **Step-4 pull has no audit consumer** (it feeds routing, not yet the Step-5 audit); **tuning bundle deferred together** (brancher thresholds, `MEMORY_TOP_K=5`, `LEARNINGS_PER_REPORT_CAP=5`, inbox caps 12k/40k/2k chars, 100 CSV rows, 20 MB file guard).
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Plan the next slice** (`/metis-plan-task`). Front-runner is the **chart-block slice** (direction settled, unplanned). Strong alternative now: **`/metis-reconcile`** to clear `SYNTHESIS.md`'s stale HTML/extraction/`market_regime` lines — the corpus and code agree, only the synthesis artifact lags. Or the **optional GUI/live run** (~40 FMP calls + one generation; exercises inbox parsing, archive, error states, and retention live).
