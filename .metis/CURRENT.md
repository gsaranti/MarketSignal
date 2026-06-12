# Current session handoff

## What happened

**Step-8 RouterInput slice shipped** — squash `2982d18` (PR #23), on `main`. `RouterInput` gained the doc's fourth input, **recent report context**: `pipeline::load_recent_report_context` reads the last **3** `ReportSummary` rows best-effort (degrades to empty, never gates — mirrors `compute_prior_deltas`), threaded through `assemble_research_packet`; the router prompt gains a thesis-continuity block + system-prompt clause, and the empty-input short-circuit covers the new field. Scope calls, kept legible in the doc comments: the doc's "recent **Markdown** report context" ships in **structured summary form** (full Markdown bodies belong to the future Step-2 main-agent context slice — whether they ever feed routing is that slice's call); "upcoming known events" needs no field — it already rides in on `baseline.calendar`. Reviews: **Metis approve** (re-ran verification independently); **Codex 1 Low** — the `RouterInput` doc comment silently absorbed the summaries-vs-Markdown delta — fixed pre-merge.

## Current state

On **`main` @ `2982d18`**, synced with origin, branch deleted, **nothing in flight**. `cargo test` 259 passed / 0 failed / 13 ignored, clippy `--all-targets --all-features` clean, `npm run build` OK. **No live API spend this session.** The full `live_research_packet_smoke` remains unrun since the FMP-news slice; its next deliberate run spends ~9 news calls + up to 20 executor searches and needs `FMP_API_KEY`.

## Open questions

- **LanceDB / vector memory entirely unbuilt** — blocks Step-4/Step-10 memory pulls, embeddings (`text-embedding-3-large`), 30-report/durable-learning retention, and is now also one of RouterInput's two missing inputs; its own multi-slice effort. Clear front-runner.
- **`.metis/BUILD.md` stale in two spots** — the gather passage still reads Tavily+GDELT (needs the FMP-Articles line), and nothing records the RouterInput recent-report input or its summary-form deviation (user-run writes).
- **RouterInput: 5 of 7** — vector memory + parsed inbox documents remain (the latter blocked on a Step-6 parsing slice); report context ships in summary form.
- **Brancher tuning (deferred)** — thresholds, keyword sets, cadence stance; ships oil+yields only by design.
- **Optional GUI tracker run** — visual corroboration that research rows bucket correctly live (~40 FMP calls + one generation); corroboration only.
- *(carried)* `fmp_baseline_smoke` unrun since quota reset; snapshot retention vs. 30-report cascade; tracker live-SSE (streamed-token) smoke; `COVERAGE_FLOOR=0.6` not final; degraded-past-report reader signal; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; filter-prompt snippets; step-6 inbox auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

**Plan the first LanceDB / vector-memory slice** (`/metis-plan-task`) — the RouterInput follow-on is done; vector memory is the largest unblock and now gates two surfaces (the Step-4/10 memory pulls and a missing RouterInput input). First slice: likely the store + embedding seam, offline-stubbable per the spine. Fold the two `BUILD.md` updates in while touching project state.
