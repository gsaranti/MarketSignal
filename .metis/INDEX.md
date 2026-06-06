# Index

*Concept → file:section map. Written by /metis-reconcile.*

## Product & platform
- Product positioning (what it is / isn't) — overview.md; README.md
- Tech stack (Tauri, Vue, SQLite, LanceDB) — overview.md
- Local-first / runs on user machine except external API calls — overview.md
- Docs corpus map — README.md

## Agents & models
- Agent pipeline (fixed multi-agent, not tool-driven) — agents.md (intro); weekly-report-workflow.md §Step 11
- Main Agent (Head Market Analyst) responsibilities — agents.md §Main Agent
- Main Agent synthesis behavior (independent critique, unified voice) — agents.md §Synthesis Behavior; weekly-report-workflow.md §Step 15
- Analyst Agents (Bull / Bear / Balanced) — agents.md §Analyst Agents; weekly-report-workflow.md §Steps 11–14
- Bull / Bear / Balanced postures — agents.md §Bull Analyst, §Bear Analyst, §Balanced Analyst
- Fixed internal models (non-configurable) — agents.md §Fixed Internal Models
  - Headline Filtering = OpenAI GPT-5 mini — agents.md §Headline Filtering; weekly-report-workflow.md §Step 7
  - Data Extraction = OpenAI GPT-5 mini — agents.md §Data Extraction
  - Research Routing = Anthropic Claude Sonnet — agents.md §Research Routing; weekly-report-workflow.md §Step 8
  - Embeddings = OpenAI text-embedding-3-large — storage.md §Embeddings
- User-configurable agent models — configuration.md §Agent Model Configuration
- Analyst skills (16 reusable prompts + output schemas) — analyst-skills.md

## Configuration & validation
- Settings overview — configuration.md §Settings Overview; interface.md (Settings tree)
- Agent model selection (default = none selected) — configuration.md §Agent Model Configuration
- API tokens (OpenAI, Anthropic) — configuration.md §API Tokens; data-sources.md §LLM Providers
- External data provider credentials (FMP + Tavily required; FRED needs a free API key; BLS/GDELT keyless) — configuration.md §External Data Provider Credentials; data-sources.md
- Execution gate / pre-run validation — configuration.md; scheduling.md §Job Controls; weekly-report-workflow.md §Step 1

## Scheduling & runtime
- Weekly job schedule (Sunday 9:00 AM local) — scheduling.md §Weekly Market Report Job
- Job states (Successful / Failed / Missed / Skipped) — scheduling.md §Job States
- App-must-be-running / system tray — scheduling.md §Application Runtime Requirements
- System sleep behavior — scheduling.md §System Sleep Behavior
- Offline behavior — scheduling.md §Offline Behavior
- Concurrent job protection (single workflow) — scheduling.md §Concurrent Job Protection
- Job status visibility — scheduling.md §Job Status Visibility
- Enable/disable controls (enabled by default) — scheduling.md §Job Controls
- Manual report generation — scheduling.md §Manual Report Generation
- Error handling — scheduling.md §Error Handling
- Missed job detection (no replay/queue) — scheduling.md §Missed Job Detection

## Weekly report workflow (17 steps)
- End-to-end step list — weekly-report-workflow.md §Steps 1–17
- News ingestion funnel (~500 → ~5 topics) — weekly-report-workflow.md §Step 7
- Research routing / research plan — weekly-report-workflow.md §Step 8
- Dynamic research + limits (50 requests / 30 min / depth 2) — weekly-report-workflow.md §Step 9
- Condensed research packet — weekly-report-workflow.md §Step 10; agents.md §Main Agent
- Baseline market data scan — weekly-report-workflow.md §Step 6

## Data sources
- Financial Modeling Prep (primary financial-data source) — data-sources.md §Financial Modeling Prep
- FRED — data-sources.md §FRED
- BLS — data-sources.md §BLS
- Tavily (primary research/news ingestion) — data-sources.md §Tavily
- GDELT (geopolitical/event monitoring) — data-sources.md §GDELT
- LLM providers (OpenAI, Anthropic) — data-sources.md §LLM Providers

## Research documents
- /research-inbox and /research-archive — research-documents.md; interface.md (Research Documents)
- Supported formats (PDF/MD/TXT/CSV/JSON/HTML) — research-documents.md §Research Inbox
- Processing at job start + auto-archive — research-documents.md §Processing at Job Start; weekly-report-workflow.md §Step 5
- User permissions (delete yes / archive no) — research-documents.md §User Permissions

## Thesis & continuity
- Thesis continuity / evolving process — thesis-continuity.md
- Report continuity (flow between reports) — thesis-continuity.md §Report Continuity
- Thesis stability (signal over noise) — thesis-continuity.md §Thesis Stability
- Thesis pivot conditions — thesis-continuity.md §Thesis Pivot Conditions
- Memory-guided evolution — thesis-continuity.md §Memory-Guided Evolution; weekly-report-workflow.md §Step 4
- Retrospective audit of prior reports — weekly-report-workflow.md §Step 3; report-structure.md §Retrospective Audit

## Report format & structure
- Markdown canonical vs HTML presentation rule — report-structure.md; weekly-report-workflow.md §Steps 2, 17
- markdown-it renderer — report-structure.md §Presentation Format
- Standard report sections — report-structure.md §Standard Report Structure
- Market Signal Thesis (unified voice) — report-structure.md §Market Signal Thesis; agents.md §Synthesis Behavior
- Index Picture (Dow/S&P/Nasdaq) — report-structure.md §Standard Report Structure
- Investment Strategy (no buy/sell) — report-structure.md §Investment Strategy

## Storage & retention
- Markdown file storage + naming — storage.md §Markdown File Storage; export.md §Export Naming
- SQLite (records, metadata, HTML, job history, warnings) — storage.md §SQLite
- market_regime fixed vocabulary (6 labels) — storage.md §SQLite
- Report summary metadata schema (JSON, required/optional fields) — storage.md §Report Summary Metadata Schema
- Retention (30 reports, cascade delete) — storage.md §SQLite
- LanceDB vector memory (summaries, durable learnings) — storage.md §LanceDB Vector Memory; weekly-report-workflow.md §Steps 4, 16
- Durable learnings survive report deletion — storage.md §LanceDB Vector Memory

## Interface
- Main layout tree — interface.md §Main Layout
- Latest Report View / Recent Reports Sidebar — interface.md; weekly-report-workflow.md §Step 17
- Persistent Warning Area (5 categories, de-dup, dismiss) — interface.md §Persistent Warning Area; scheduling.md §Error Handling

## Export
- Export options (Markdown, PDF) — export.md §Export Options
- PDF via Tauri webview print-to-PDF — export.md §PDF Export
- Export naming convention — export.md §Export Naming
- Export does not re-run workflow — export.md §Export Behavior
