# Index

*Concept → file:section map. Written by /metis-reconcile.*

## Product & platform
- Product positioning (what it is / isn't) — overview.md; README.md
- Tech stack (Tauri, Vue, SQLite — incl. vector memory) — overview.md
- Local-first / runs on user machine except external API calls — overview.md
- Docs corpus map — README.md

## Agents & models
- Agent pipeline (fixed multi-agent, not tool-driven) — agents.md (intro); report-workflow.md §Step 12
- Main Agent (Head Market Analyst) responsibilities — agents.md §Main Agent
- Main Agent synthesis behavior (independent critique, unified voice) — agents.md §Synthesis Behavior; report-workflow.md §Step 16
- Analyst Agents (Bull / Bear / Balanced) — agents.md §Analyst Agents; report-workflow.md §Steps 12–15
- Bull / Bear / Balanced postures — agents.md §Bull Analyst, §Bear Analyst, §Balanced Analyst
- Fixed internal models (non-configurable) — agents.md §Fixed Internal Models
  - Headline Filtering = OpenAI GPT-5 mini — agents.md §Headline Filtering; report-workflow.md §Step 7
  - Data Extraction — no model stage runs (inbox parsing is deterministic) — agents.md §Data Extraction
  - Research Routing = Anthropic Claude Sonnet — agents.md §Research Routing; report-workflow.md §Step 8
  - Embeddings = OpenAI text-embedding-3-large — storage.md §Embeddings
- User-configurable agent models — configuration.md §Agent Model Configuration
- Analyst skills (16 reusable prompts + output schemas) — analyst-skills.md

## Configuration & validation
- Settings overview — configuration.md §Settings Overview; interface.md (Settings tree)
- Agent model selection (default = none selected) — configuration.md §Agent Model Configuration
- API tokens (OpenAI, Anthropic) — configuration.md §API Tokens; data-sources.md §LLM Providers
- External data provider credentials (FMP + Tavily required; FRED needs a free API key; BLS/GDELT keyless) — configuration.md §External Data Provider Credentials; data-sources.md
- Execution gate / pre-run validation — configuration.md; report-workflow.md §Step 1

## Job execution & runtime
- On-demand report generation (no scheduler) — scheduling.md §Generating a Report
- The report job (analytical focus) — scheduling.md §The Market Signal Report Job
- Job states (Successful / Failed / Skipped / Cancelled) — scheduling.md §Job States
- Application runtime (windowed app, no background jobs) — scheduling.md §Application Runtime
- Offline behavior (unreachable provider → Failed run; no pre-run reachability gate) — scheduling.md §Offline Behavior
- Concurrent job protection (single workflow) — scheduling.md §Concurrent Job Protection
- Job status visibility — scheduling.md §Job Status Visibility
- Error handling — scheduling.md §Error Handling

## Run tracking & cancellation
- Live run tracker (replaces the report pane while a job runs; latest-run-only) — run-tracking.md §What the Tracker Shows; interface.md
- Per-request pass/fail rows (one row per actual API call) — run-tracking.md §What the Tracker Shows
- Streamed main-agent output (report text token-by-token) — run-tracking.md §What the Tracker Shows
- Job cancellation (cooperative; Cancelled state, raises no warning) — run-tracking.md §Cancellation; scheduling.md §Job States
- Run-is-not-a-report invariant — run-tracking.md §A Run Is Not a Report
- Reaching the tracker (footer: View progress / View run log) — run-tracking.md §Reaching the Tracker

## Report workflow (18 steps)
- End-to-end step list — report-workflow.md §Steps 1–18
- News ingestion funnel (~500 → ~5 topics) — report-workflow.md §Step 7
- Research routing / research plan — report-workflow.md §Step 8
- Dynamic research + limits (50 requests / 30 min / depth 2) — report-workflow.md §Step 9
- Condensed research packet — report-workflow.md §Step 11; agents.md §Main Agent
- Baseline market data scan — report-workflow.md §Step 3
- Baseline change view (deltas since previous report) — report-workflow.md §Step 3; storage.md §Baseline Snapshots
- Vector memory retrieval — pre-research (steers audit + routing) §Step 4; post-research (research-informed) §Step 10 — report-workflow.md

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
- Processing at job start + auto-archive — research-documents.md §Processing at Job Start; report-workflow.md §Step 6
- User permissions (delete yes / archive no) — research-documents.md §User Permissions

## Thesis & continuity
- Thesis continuity / evolving process — thesis-continuity.md
- Report continuity (flow between reports) — thesis-continuity.md §Report Continuity
- Thesis stability (signal over noise) — thesis-continuity.md §Thesis Stability
- Thesis pivot conditions — thesis-continuity.md §Thesis Pivot Conditions
- Memory-guided evolution — thesis-continuity.md §Memory-Guided Evolution; report-workflow.md §Steps 4, 10
- Retrospective audit of prior reports — report-workflow.md §Step 5; report-structure.md §Retrospective Audit

## Report format & structure
- Markdown canonical vs HTML presentation rule — report-structure.md; report-workflow.md §Steps 2, 18
- markdown-it renderer — report-structure.md §Presentation Format
- Embedded chart blocks (fenced `chart` JSON → inline SVG; line/bar/area; fail-soft authoring convention) — report-structure.md §Embedded charts
- Standard report sections — report-structure.md §Standard Report Structure
- Market Signal Thesis (unified voice) — report-structure.md §Market Signal Thesis; agents.md §Synthesis Behavior
- Index Picture (Dow/S&P/Nasdaq) — report-structure.md §Standard Report Structure
- Investment Strategy (no buy/sell) — report-structure.md §Investment Strategy

## Storage & retention
- Markdown file storage + naming — storage.md §Markdown File Storage; export.md §Export Naming
- SQLite (records, metadata, job history, warnings, baseline snapshots; HTML deliberately not stored) — storage.md §SQLite
- risk_posture / market_cycle fixed vocabularies (two orthogonal axes, 3 labels each) — storage.md §SQLite
- Report summary metadata schema (JSON, required/optional fields) — storage.md §Report Summary Metadata Schema
- Retention (30 reports, cascade delete) — storage.md §SQLite
- Per-report baseline snapshots + change view (deltas vs previous report) — storage.md §Baseline Snapshots; report-workflow.md §Step 3
- Baseline-snapshot retention (14, independent of report retention) — storage.md §Baseline Snapshots
- Vector memory (summaries, durable learnings; SQLite-backed, amended from LanceDB) — storage.md §Vector Memory; report-workflow.md §Steps 4, 10, 17
- Durable learnings survive report deletion — storage.md §Vector Memory

## Interface
- Main layout tree — interface.md §Main Layout
- Latest Report View / Recent Reports Sidebar — interface.md; report-workflow.md §Step 18
- Run Tracker (live job progress; replaces report pane) — interface.md; run-tracking.md
- Persistent Warning Area (4 categories, de-dup, dismiss) — interface.md §Persistent Warning Area; scheduling.md §Error Handling

## Export
- Export options (Markdown, PDF) — export.md §Export Options
- PDF via Tauri webview print-to-PDF — export.md §PDF Export
- Export naming convention — export.md §Export Naming
- Export does not re-run workflow — export.md §Export Behavior

## Local analysis suite
- Local analysis suite overview (local-only, two prescriptive features) — overview.md §Local Analysis Suite; local-models.md
- Local model substrate (Ollama-on-MLX serving, roster, per-task routing) — local-models.md §Serving runtime, §The model roster and per-task routing
- Model residency default (one 122B fills research/distill/interpret by mode + embedder resident; 35B fast tier a benchmark-gated option) — local-models.md §The model roster and per-task routing
- Local-model adapter seam (flexible endpoint/model_id client, distinct from the cloud AgentModel enum) — local-models.md §The local-model adapter seam
- Schema-constrained output (grammar-constrained JSON) — local-models.md §Schema-constrained output
- Context-memory discipline (distilled hand-offs, retrieve-don't-dump) — local-models.md §Context-memory discipline
- Per-job isolated vector memory (three partitions) — local-models.md §Run history and continuity; storage.md §Local Vector Memory
- Web research tool (SearXNG-primary, Tavily fallback, fetch/extract) — web-research.md
- Charles Schwab integration (OAuth loopback, 30-min/7-day token lifecycle, positions, account hashing) — schwab-integration.md
- Manual holdings import (CSV/paste fallback) — schwab-integration.md §Manual import fallback
- Portfolio Analysis job (per-holding pipeline, grade, action, targets, roll-up) — portfolio-analysis.md
- Holding verdict schema (grade + sub-scores, action ladder, horizon, targets, what-changed) — portfolio-analysis.md §The holding verdict
- Holdings change tracking (deterministic prior-run-snapshot diff → per-position new/increased/decreased/unchanged delta into dossier; exited names surfaced in roll-up) — portfolio-analysis.md §Holdings change tracking
- Trade Opportunities job (3×3 risk×horizon matrix) — trade-opportunities.md
- Trade Opportunities — what it hunts (two modes: early detection + continuation; leading-metric anchor) — trade-opportunities.md §What the job hunts
- Opportunity archetype lens (secular-compounder/ai-infra/commodity-cyclical/disruptor/quality-compounder; selects signal weighting + valuation lens) — trade-opportunities.md §Archetype
- Research-driven candidate discovery (top-down theme/event scan + bottom-up screens + keyless positioning scans; research generates candidates) — trade-opportunities.md §The pipeline
- Narrative-vs-reality ratio + forensic risk gate + base-rate conjunction discipline — trade-opportunities.md §The pipeline
- Trade Opportunities signal inputs (FMP paid-tier working/discovery feed: fundamentals/segments, revision signal, financial-scores forensic gate, symbol-keyed positioning incl. congressional, bulk-screener funnel; FINRA short interest; FRED/Stooq commodity prices; engine-computed price-action confirmer (relative strength / base breakout, confirmer not trigger); SEC EDGAR authoritative cross-check; web-research lane for ASPs/supply-discipline; ticker→CIK optional; report data-source logic unchanged) — trade-opportunities.md §Signal inputs; data-sources.md §Local Analysis Suite Sources
- Opportunity schema (archetype, detection mode, leading metric, thesis, catalyst, conviction, narrative-vs-reality, bear case, carry-forward status) — trade-opportunities.md §The opportunity
- Local analysis suite configuration (daemon, roster, SearXNG, Schwab; gates local jobs only) — configuration.md §Local Analysis Suite Configuration
- Local analysis suite storage + per-feature retention — storage.md §Local Analysis Suite Storage
- Local suite pages (Portfolio, Trade Opportunities) — interface.md §Main Layout
- Deterministic financial-analysis engine (Rust computes metrics/sub-scores/risk tiers/targets; model interprets) — local-models.md §Context-memory discipline; portfolio-analysis.md
- SEC EDGAR primary source (keyless filings + XBRL company facts) — data-sources.md §SEC EDGAR
- SEC EDGAR role for Trade Opportunities (authoritative cross-check for grade/target numbers + 8-K filings; positioning moved to FMP symbol-keyed; ticker→CIK a non-blocking enhancement) — data-sources.md §SEC EDGAR
- FMP paid-tier suite signals (revision flow via estimates/grades-historical/price-targets/upgrades-downgrades + surprises; financial-scores Altman+Piotroski forensic gate; insider/13F/Senate-House positioning; screener/peers/bulk discovery; one shared FMP key upgraded to paid, report logic unchanged) — data-sources.md §Local Analysis Suite Sources
- FINRA short interest (keyless biweekly consolidated equity short-interest file; level/trend/days-to-cover) — data-sources.md §FINRA
- Evidence floor (insufficient-evidence abstention) — portfolio-analysis.md §Evidence floor
- Deterministic risk-tier assignment — trade-opportunities.md §The opportunity space
- Per-holding checkpoint/resume + research caching — portfolio-analysis.md §Failure posture
- Free-tier data dispersal (SEC/Stooq/Finnhub offload FMP; FMP keeps niche aggregates) — data-sources.md §Local Analysis Suite Sources
- Research loop & intra-loop context management (per-topic agenda, depth ≤2 / ≤3 passes per topic — branches not LLM turns, per-item fetch+wall-clock budget binds first, condense-as-you-go) — web-research.md §The research loop and context management
- Research agenda (fundamentals + market narrative/sentiment + forward opportunity/thematic fit) — portfolio-analysis.md; trade-opportunities.md
- Options-activity signal (per-stock put/call vol+OI & IV/skew from Schwab chains, an activity proxy not positioning truth; CBOE venue-level backdrop) — schwab-integration.md; data-sources.md §CBOE; portfolio-analysis.md
- Schwab connection required for both local jobs (hard execution-gate precondition) — schwab-integration.md §A connected Schwab account is required
