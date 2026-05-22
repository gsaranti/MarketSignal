# Notes from Restructuring

Observations captured while splitting [requirements.md](../requirements.md) into focused files. Not intended as resolutions or as an exhaustive review — just what surfaced naturally during the pass.

## Potential contradictions or unclear interactions

## Terminology used inconsistently or in two senses

## Areas the doc appears silent on

## Duplications consolidated during the split

- **Settings bullets** appear both in the Main Layout tree and in the standalone Settings section (requirements.md §Main Layout, §Settings). Consolidated into [ui.md](../docs/ui.md).
- **Manual report execution** appears in the Main Layout tree, in Settings, and in its own subsection under Job Execution (requirements.md §Main Layout, §Settings, §Manual Report Generation). Primary detail kept in [job-execution.md](../docs/job-execution.md); ui.md cross-references it.
- **Scheduled job controls** appear in the Main Layout tree, in Settings, and in Job Controls (requirements.md §Main Layout, §Settings, §Job Controls). Primary detail kept in [job-execution.md](../docs/job-execution.md).
- **Research inbox / archive** is mentioned in the Main Layout tree, in the Research Document Workflow section, and in Main Agent Workflow step 5 (requirements.md §Main Layout, §Research Document Workflow, §Full Flow). Primary detail kept in [research-documents.md](../docs/research-documents.md).
- **Weekly review** content is split across §Scheduled Jobs (schedule + focus), §Report Structure (template), and §Weekly Review Workflow (process). Schedule remains in [scheduled-jobs.md](../docs/scheduled-jobs.md); template and process consolidated into [reports/weekly-review.md](../docs/reports/weekly-review.md).
- **Subagent overview** appears in §Agent Architecture (purpose, philosophy, examples) and §Subagent Responsibilities (per-agent detail). Overview-level material kept in [agents/architecture.md](../docs/agents/architecture.md); per-agent responsibilities in [agents/subagents.md](../docs/agents/subagents.md).
- **Vector memory / durable learnings** are referenced in Overview, §Full Flow (steps 4/15/16), §Context Window Control, §Thesis Continuity (Memory-Guided Evolution), and §LanceDB Vector Memory. The storage definition lives in [storage.md](../docs/storage.md); the usage pattern lives in [reports/thesis-continuity.md](../docs/reports/thesis-continuity.md) and [agents/main-agent-workflow.md](../docs/agents/main-agent-workflow.md).

## External-link gaps a builder would benefit from

