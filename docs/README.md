# Market Signal Documentation

This folder contains the Market Signal MVP requirements, restructured into focused files. The original [requirements.md](../requirements.md) at the project root is preserved unchanged.

## Files

- [overview.md](overview.md) — what the application is, its stack, and what it is not
- [ui.md](ui.md) — main layout, settings section, warning banner area
- [scheduled-jobs.md](scheduled-jobs.md) — the three job types: schedules and focus areas
- [job-execution.md](job-execution.md) — runtime requirements, sleep/offline behavior, concurrency, controls, manual runs, errors, status visibility, missed-job detection
- [data-sources.md](data-sources.md) — OpenBB, Financial Modeling Prep, FRED, BLS, Tavily, GDELT, LLM providers
- [research-documents.md](research-documents.md) — research inbox / research archive workflow
- [agents/architecture.md](agents/architecture.md) — main agent role and subagent system overview
- [agents/main-agent-workflow.md](agents/main-agent-workflow.md) — the 18-step main agent flow
- [agents/subagents.md](agents/subagents.md) — Bull, Bear, and Balanced analyst responsibilities
- [agents/models.md](agents/models.md) — fixed internal models and user-configurable model selection
- [agents/analyst-skills.md](agents/analyst-skills.md) — the reusable analyst skills included in MVP
- [research-behavior.md](research-behavior.md) — baseline scan, forward-looking research, dynamic branching
- [cost-control.md](cost-control.md) — news ingestion pipeline, context-window control, agent workflow limits
- [reports/format.md](reports/format.md) — Markdown as canonical format, HTML as presentation artifact
- [reports/standard-structure.md](reports/standard-structure.md) — premarket and postmarket report template
- [reports/weekly-review.md](reports/weekly-review.md) — weekly review process and report template
- [reports/thesis-continuity.md](reports/thesis-continuity.md) — continuity, stability, pivot conditions, memory-guided evolution
- [reports/export.md](reports/export.md) — Markdown and PDF export
- [storage.md](storage.md) — SQLite and LanceDB vector memory
- [NOTES-FROM-RESTRUCTURING.md](NOTES-FROM-RESTRUCTURING.md) — observations flagged during the restructuring pass (contradictions, gaps, terminology, consolidated duplicates, external-link gaps)
