# Current session handoff

## What happened

**Closed the optional chart-convention doc-mapping item** carried in the prior handoff's open questions — two explicitly user-authorized one-off `.metis/` doc writes, no code touched. (1) Added an INDEX.md line in "Report format & structure" mapping the embedded-chart authoring convention to `report-structure.md §Embedded charts` (the fenced `chart` JSON → inline SVG; line/bar/area; fail-soft). (2) Added a matching mention to BUILD.md's `frontend` module-boundary bullet, appended after the markdown-it sentence: the agent emits the `chart` block as part of its report Markdown (authoring rules in `model_agent.rs`), `src/renderChart.ts` is the authoritative validator, the block is the *only* way a chart enters a report (app layer never injects one) — keeping faith with the agents-emit-Markdown / frontend-renders spine. **Correction to the prior handoff's guess** ("INDEX already maps charts, so likely no change"): on inspection INDEX had only the generic markdown-it line, no charts line, and `report-structure.md` now carries a dedicated `### Embedded charts` subsection — so the line *was* warranted.

## Current state

HEAD is **`7240006`** (prior metis-session-end; last code commit is `68b0765`). **Nothing in flight.** The two `.metis/` doc edits (INDEX.md + BUILD.md chart lines) are **uncommitted in the working tree** — left for the user to commit, since `.metis/` writes are user-run.

## Open questions

- *(carried)* tuning bundle (`MEMORY_TOP_K=5`, `LEARNINGS_PER_REPORT_CAP=5`, `LEARNING_DEDUP_THRESHOLD=0.93`, `MAIN_AGENT_RECENT_REPORTS=3`, `RECENT_REPORT_BODY_CAP=12_000`, inbox caps, `COVERAGE_FLOOR=0.6`) unvalidated vs real `text-embedding-3-large` geometry; needs a separating test embedder (`BasisEmbedder`/`DistinctEmbedder`) since `StubEmbedder` collapses distinct prose to ~1.0 cosine.
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; esbuild/vite advisory parked; wiremock / in-loop offline gap; conditional GPT-5-mini extraction stage.
- *(low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide.

## Where to start

First, decide whether to commit the two uncommitted `.metis/` doc edits (chart-convention lines in INDEX.md + BUILD.md). Otherwise nothing is owed. Heaviest meaningful item: **tuning-bundle validation** — build the separating test embedder (`BasisEmbedder`/`DistinctEmbedder`) first, then validate the constants against it.
