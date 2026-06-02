# Market Signal Documentation Index

## Files

- [overview.md](overview.md) — Product positioning, technology stack, and what the application is and isn't.
- [interface.md](interface.md) — The main UI layout and pointers to the operational behavior of each panel.
- [configuration.md](configuration.md) — Settings the user must complete: agent model selection, API tokens, external data provider credentials, and the validation rules that gate job execution.
- [scheduling.md](scheduling.md) — When the weekly job runs and the runtime semantics around it: sleep, offline, concurrency, status visibility, manual execution, error handling, missed-vs-failed jobs.
- [data-sources.md](data-sources.md) — External data and model providers: Financial Modeling Prep, FRED, BLS, Tavily, GDELT, and LLM providers.
- [research-documents.md](research-documents.md) — The `/research-inbox` and `/research-archive` workflow for user-supplied documents.
- [agents.md](agents.md) — Agent pipeline architecture: the main agent, the three analyst agents, and the non-configurable fixed internal models.
- [weekly-report-workflow.md](weekly-report-workflow.md) — The 17-step Weekly Market Report job flow from validation through HTML generation.
- [analyst-skills.md](analyst-skills.md) — The 16 reusable analytical skills (Market Regime Analysis, Narrative vs Reality, etc.).
- [thesis-continuity.md](thesis-continuity.md) — How market theses evolve across reports: continuity, stability, pivot conditions, and memory-guided evolution.
- [report-structure.md](report-structure.md) — Canonical Markdown authoring format, the HTML-for-presentation rule, and the standard report sections.
- [storage.md](storage.md) — SQLite and LanceDB responsibilities, retention rules, and deletion behavior.
- [export.md](export.md) — Markdown and PDF export options, naming conventions, and behavior.
