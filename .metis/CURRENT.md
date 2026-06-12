# Current session handoff

## What happened

**Step-6 research-inbox parsing shipped** — squash `ba1d1b2` (PR #28), on `main`. The pipeline now parses inbox documents (PDF/MD/TXT/CSV/JSON/HTML) **deterministically** — a settled user decision: no GPT-5-mini extraction stage; "summaries" ship as condensed excerpts (12k-char per-doc head cap at a paragraph seam, 40k-char water-filled total budget, truncation always disclosed by a marker; pinned in `document_parser.rs`'s header as a conscious deviation from `agents.md §Data Extraction`, with the extraction stage the named follow-on *if oversized docs prove common*). **RouterInput is 7/7** (router gets 2k-char excerpts; the packet's `inbox_summaries` carry the full blocks). Parse failures never gate: recorded in `research_parse_failures` (replaced wholesale per pass, identity-matched on name+size+mtime at read), surfaced as panel error states (`.docs-tag--error` — a noted design-system extension). **Archive happens only after the report persists** (planner's recommendation, adopted as a named assumption — user never explicitly confirmed but merged it); a failed/cancelled run never consumes documents. `ReportPaths::under` is now the single app-data layout source. New deps: `pdf-extract 0.10` (under `catch_unwind` — it panics on malformed input; residual stack-overflow abort risk accepted) and `html2text 0.17`. Reviews: Metis approve-with-nits (both actionable nits fixed in-slice), Codex round both findings fixed (post-run inbox/archive UI refresh at both completion sites; record-aware CSV splitting) with one half **declined**: strict CSV validation rejected as worse UX — leniency pinned in code.

## Current state

On **`main` @ `ba1d1b2`**, synced, branches pruned, **nothing in flight**. `cargo test` 333 passed / 0 failed / 14 ignored, clippy clean, `npm run build` OK. No live API spend this session.

## Open questions

- **GPT-5-mini extraction stage** — conditional follow-on: only if users actually drop docs > ~12k chars; seam ready (replace head-truncation for overflow docs, nothing else changes).
- **GUI visual pass of the inbox error-state row** — deferred (no Vue test harness); folds into the optional GUI/live run, which would now also exercise inbox parse → archive → error states live.
- **Does persisted HTML need to exist at all?** `docs/storage.md` still lists it — docs amendment / reconcile item. Related: docs also assign extraction to GPT-5-mini and "summaries" to the packet — the deterministic-excerpts deviation is pinned in code but not in docs.
- **Chart-block slice unplanned** — fenced ```chart blocks (direction settled earlier): SYSTEM_PROMPT teaching, markdown-it fence renderer, design-system chart styling extension.
- **Learning dedup unbuilt**; **Step-4 pull has no audit consumer**; **tuning bundle deferred together** (brancher thresholds, `MEMORY_TOP_K=5`, `LEARNINGS_PER_REPORT_CAP=5`, now also the inbox caps: 12k/40k/2k chars, 100 CSV rows, 20 MB file guard).
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Plan the next slice** (`/metis-plan-task`). With RouterInput closed, the front-runner is the **chart-block slice** (direction settled, unplanned); alternatives: the **HTML + extraction docs amendment** (reconcile/docs walk — two pinned code deviations now await docs), or the **optional GUI/live run** (~40 FMP calls + one generation; now exercises inbox parsing and retention live).
