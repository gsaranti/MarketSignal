# Current session handoff

## What happened

**30-report retention cascade shipped** — squash `7529413` (PR #27), on `main`. The persist step now evicts every report beyond the newest 30: Markdown file, vector summary row, baseline-snapshot rows, and report row together; durable learnings survive by `kind`. Selection (`storage::select_reports_beyond_retention`) shares `list_recent_reports`' exact ordering; `REPORT_RETENTION` is a separate constant from the display cap. Failure semantics: file leg first (NotFound = already gone; other fs errors skip the evictee for next-run retry); the three DB legs commit or roll back as **one transaction** (`unchecked_transaction`) — **Codex round 1 (P2) fix**: a partial DB failure must not delete the report row (the retry key), or the summary row would be stranded in retrievable memory forever. Reviews: Metis approve (9/9), Codex otherwise clean. **HTML has no cascade leg because HTML persistence doesn't exist** (`storage.rs` defers it); the obligation is pinned in `prune_old_reports`' doc comment.

**Report-format direction settled in discussion** (not yet in docs): keep Markdown canonical — agent-authored HTML was rejected (webview injection risk, lossy reverse conversion, presentation coupling). Charts/graphs should ride as fenced ```chart blocks (declarative JSON spec in the Markdown, rendered to SVG by a markdown-it fence plugin with design tokens); a future slice.

## Current state

On **`main` @ `7529413`**, synced, branches pruned, **nothing in flight**. `cargo test` 306 passed / 0 failed / 14 ignored, clippy clean, `npm run build` OK. No live API spend this session.

## Open questions

- **Does persisted HTML need to exist at all?** View-time markdown-it rendering + print-to-PDF may make it permanently unnecessary, but `docs/storage.md` still lists "HTML output" in SQLite and names HTML in the deletion list — docs amendment / reconcile item.
- **Chart-block slice unplanned** — needs SYSTEM_PROMPT teaching, the markdown-it fence renderer, and a design-system extension for chart styling (none exists; extend per CLAUDE.md step 5, don't invent).
- **Learning dedup unbuilt** — re-emitted lessons accumulate near-duplicate `learning` rows forever; belongs with the tuning bundle.
- **Step-4 pull has no audit consumer** — no audit stage exists; lands in routing only (seam ready in `assemble_research_packet`).
- **RouterInput: 6 of 7** — only parsed inbox documents remain (blocked on a Step-6 parsing slice).
- **Tuning deferred together** — brancher thresholds/keywords; `MEMORY_TOP_K=5`, no similarity floor, query composition; `LEARNINGS_PER_REPORT_CAP=5`.
- **Optional GUI/live run** — would now also exercise the retention prune after persist; ~40 FMP calls + one generation.
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Plan the next slice** (`/metis-plan-task`). Front-runner: **Step-6 inbox parsing** (unblocks RouterInput 7/7 and the research-documents flow). Alternatives: the **chart-block slice** (direction settled above, unplanned), or the **HTML docs amendment** (a reconcile/docs walk, not code). Pick by priority.
