# Notes from Restructuring

Observations captured while splitting [requirements.md](../requirements.md) into focused files. Not intended as resolutions or as an exhaustive review — just what surfaced naturally during the pass.

## Potential contradictions or unclear interactions

- **Postmarket schedule includes Sunday.** Postmarket Report Job runs "Sunday–Friday evenings" (requirements.md §Postmarket Report Job). US equity markets are closed on Sunday evenings, which makes the inclusion of Sunday in the postmarket cadence non-obvious.
- **Mixed timezone anchoring across schedules.** Premarket and Postmarket use absolute PT/ET anchors, but Weekly Review uses "9:00 AM local time" (requirements.md §Scheduled Jobs). The mix of fixed market-timezone anchors and machine-local time for a single set of scheduled jobs is worth confirming.
- **"Enabled" used in two senses.** Job Controls says "By default, all are enabled" (requirements.md §Job Controls), while User-Configurable Models says "By default, the application starts with no models selected" and "scheduled jobs do not execute" without configured models (requirements.md §User-Configurable Models). A job can therefore be "enabled" via the toggle but still unable to run.
- **Manual execution + concurrent-job protection.** "Only one scheduled job may run at a time" (requirements.md §Concurrent Job Protection) and "Manual execution follows the same workflow and validation rules as scheduled execution" (requirements.md §Manual Report Generation). It is not stated whether a manual run is treated as a scheduled job for concurrency purposes, nor what happens if the user triggers a manual run while a job is in flight.
- **HTML persistence step not stated in the workflow.** Main Agent Workflow step 14 saves Markdown to SQLite and step 17 generates HTML from Markdown (requirements.md §Full Flow), but no step persists the HTML. SQLite is nonetheless listed as storing "HTML output" (requirements.md §SQLite).

## Terminology used inconsistently or in two senses

- **"Report summary" vs "durable learnings" in vector memory.** Workflow step 15 saves a "report summary to vector DB" and step 16 saves "durable learnings if applicable" (requirements.md §Full Flow). Storage states that deleting a report removes "associated vector-memory summary references" but that durable learnings persist (requirements.md §SQLite, §LanceDB Vector Memory). The boundary between a summary and a durable learning, and how each is keyed back to a report, is not defined.
- **"Warning visibility" in Settings.** Listed as one of the Settings controls (requirements.md §Settings), but the term is not defined elsewhere — it is unclear whether this toggles individual warning categories, suppresses all warnings, or refers to a notification-style setting separate from the warning banner area.
- **"Scheduled execution window" for missed jobs.** Missed Job Detection refers to "the machine was offline during the scheduled execution window" (requirements.md §Missed Job Detection), but a scheduled execution time is described elsewhere as a specific instant (e.g., 4:00 AM PT), not a window. The width of the window — if any — is not specified.

## Areas the doc appears silent on

- **System-tray behavior.** Closing the window does not quit the application "if the app remains active in the system tray" (requirements.md §Application Runtime Requirements), but how tray persistence is configured (default on/off, user toggle, platform differences) is not specified.
- **Report-retention scope.** "Only the most recent 30 full reports are retained" (requirements.md §SQLite). Whether the 30-report cap applies across all report types combined or per type (premarket / postmarket / weekly review) is not stated.
- **Subagent invocation pattern.** Workflow steps 10–11 send a packet to the three subagents and receive theses (requirements.md §Full Flow), but parallel vs sequential execution is not specified, nor is the behavior when a single subagent fails (does the entire job fail per §Error Handling, or proceed with two of three?).
- **Memory retrieval mechanism.** Workflow step 4 says "Query vector memory" (requirements.md §Full Flow) and cost-control rules say only "relevant memory fragments" are injected (requirements.md §Context Window Control), but the query construction, retrieval limits, and ranking strategy are not defined.
- **Research inbox failure modes.** Research Document Workflow lists supported formats and the parse-then-archive flow (requirements.md §Research Inbox), but does not address malformed files, unsupported extensions, parsing failures, or whether one bad file blocks the rest.
- **OpenBB / FMP overlap.** OpenBB is described as "the primary financial-data access layer" while Financial Modeling Prep is "a direct structured financial-data source" (requirements.md §Market and Financial Data). OpenBB itself can wrap FMP, so the division of responsibility between them — when each is used directly — is unclear.
- **Sources section format.** Both report templates end with a `## Sources` section (requirements.md §Standard Report Structure, §Weekly Review Report Structure), but no format, citation style, or content requirement is given.
- **Warning dismissal/resolution semantics.** Error Handling says additional failing jobs do not create duplicate warnings while one is undismissed (requirements.md §Error Handling), but the user-facing dismiss vs resolve flow — and what causes a warning to clear automatically — is not defined.

## Duplications consolidated during the split

- **Settings bullets** appear both in the Main Layout tree and in the standalone Settings section (requirements.md §Main Layout, §Settings). Consolidated into [ui.md](ui.md).
- **Manual report execution** appears in the Main Layout tree, in Settings, and in its own subsection under Job Execution (requirements.md §Main Layout, §Settings, §Manual Report Generation). Primary detail kept in [job-execution.md](job-execution.md); ui.md cross-references it.
- **Scheduled job controls** appear in the Main Layout tree, in Settings, and in Job Controls (requirements.md §Main Layout, §Settings, §Job Controls). Primary detail kept in [job-execution.md](job-execution.md).
- **Research inbox / archive** is mentioned in the Main Layout tree, in the Research Document Workflow section, and in Main Agent Workflow step 5 (requirements.md §Main Layout, §Research Document Workflow, §Full Flow). Primary detail kept in [research-documents.md](research-documents.md).
- **Weekly review** content is split across §Scheduled Jobs (schedule + focus), §Report Structure (template), and §Weekly Review Workflow (process). Schedule remains in [scheduled-jobs.md](scheduled-jobs.md); template and process consolidated into [reports/weekly-review.md](reports/weekly-review.md).
- **Subagent overview** appears in §Agent Architecture (purpose, philosophy, examples) and §Subagent Responsibilities (per-agent detail). Overview-level material kept in [agents/architecture.md](agents/architecture.md); per-agent responsibilities in [agents/subagents.md](agents/subagents.md).
- **Vector memory / durable learnings** are referenced in Overview, §Full Flow (steps 4/15/16), §Context Window Control, §Thesis Continuity (Memory-Guided Evolution), and §LanceDB Vector Memory. The storage definition lives in [storage.md](storage.md); the usage pattern lives in [reports/thesis-continuity.md](reports/thesis-continuity.md) and [agents/main-agent-workflow.md](agents/main-agent-workflow.md).

## External-link gaps a builder would benefit from

- **Stack components.** Tauri, Vue, SQLite, and LanceDB are named in the Overview as the application's stack (requirements.md §Overview), but no canonical documentation links are provided. A pointer to each project's official docs would help.
- **LLM provider docs.** The LLM Providers subsection lists "OpenAI" and "Anthropic" by name (requirements.md §LLM Providers); the data-source links pattern used elsewhere in the section is not applied here. Canonical API documentation links for each provider would be useful.
- **Specific model references.** "OpenAI GPT-5 mini", "Anthropic Claude Sonnet", "GPT-5", "Claude Opus", "Claude Haiku" are named across §Fixed Internal Model Usage and §User-Configurable Models. Provider-side model pages or API model IDs would help a builder confirm availability and pricing.
- **PDF generation.** Export includes "Export PDF" generated from the HTML report (requirements.md §Export System), but no library, renderer, or approach is referenced.
