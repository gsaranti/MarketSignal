# Market Signal Documentation Index

## Files

- [overview.md](overview.md) — Product positioning, technology stack, and what the application is and isn't.
- [interface.md](interface.md) — The main UI layout and pointers to the operational behavior of each panel.
- [configuration.md](configuration.md) — Settings the user must complete: agent model selection, API tokens, external data provider credentials, and the validation rules that gate job execution.
- [scheduling.md](scheduling.md) — Job execution semantics for on-demand report generation: job states, offline behavior, concurrency protection, status visibility, and error handling.
- [run-tracking.md](run-tracking.md) — The live run tracker shown while a job runs: per-step and per-request progress, streamed agent output, the run-not-a-report rule, and job cancellation.
- [data-sources.md](data-sources.md) — External data and model providers: Financial Modeling Prep, FRED, BLS, CFTC, Tavily, GDELT, and LLM providers.
- [research-documents.md](research-documents.md) — The `/research-inbox` and `/research-archive` workflow for user-supplied documents.
- [agents.md](agents.md) — Agent pipeline architecture: the main agent, the three analyst agents, and the non-configurable fixed internal models.
- [report-workflow.md](report-workflow.md) — The 18-step Market Signal Report job flow from validation through HTML generation.
- [analyst-skills.md](analyst-skills.md) — The 16 reusable analytical skills (Market Regime Analysis, Narrative vs Reality, etc.).
- [thesis-continuity.md](thesis-continuity.md) — How market theses evolve across reports: continuity, stability, pivot conditions, and memory-guided evolution.
- [report-structure.md](report-structure.md) — Canonical Markdown authoring format, the HTML-for-presentation rule, and the standard report sections.
- [storage.md](storage.md) — SQLite and vector-memory responsibilities, retention rules, and deletion behavior.
- [export.md](export.md) — Markdown and PDF export options, naming conventions, and behavior.
- [local-models.md](local-models.md) — The local analysis suite's model substrate: local serving, the model roster and per-task routing, schema-constrained output, the context-memory discipline, and isolated per-job run memory.
- [web-research.md](web-research.md) — The local suite's web tool: the SearXNG-backed search / fetch / extract loop with a Tavily fallback.
- [schwab-integration.md](schwab-integration.md) — Portfolio holdings ingestion from Charles Schwab (OAuth, token lifecycle, positions) plus the manual-import fallback.
- [portfolio-analysis.md](portfolio-analysis.md) — The local Portfolio Analysis job: the per-holding pipeline, grading, price targets, and portfolio roll-up.
- [portfolio-workflow.md](portfolio-workflow.md) — The Portfolio Analysis job's end-to-end control flow: Type-tagged steps from the gate through the per-holding loop to the roll-up, with each local-model-call contract.
- [trade-opportunities.md](trade-opportunities.md) — The local Trade Opportunities job: the risk × horizon opportunity matrix and its continuity.
- [trade-opportunities-workflow.md](trade-opportunities-workflow.md) — The Trade Opportunities job's end-to-end control flow: Type-tagged steps from the gate through the discovery funnel and per-candidate validation loop to per-cell selection, with each local-model-call contract.
