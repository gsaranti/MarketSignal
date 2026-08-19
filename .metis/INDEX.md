# Index

*Concept → file:section map for the `docs/` corpus.
Entries are lookup pointers, not summaries: open the cited section rather than
working from the clause here.*

*Three rules keep it that way.
A row names a concept and where to read it, never what the concept says or does
— no threshold, enum, formula, algorithm or behavioral contract belongs in a
row.
This binds the subject as hard as the parenthetical: a subject is a noun phrase,
never a proposition, unless the corpus itself heads the section that way.
A parenthetical is therefore rare, and earns its place only two ways: it
disambiguates this row from one it would be confused with, or it says which
section is canonical.
Naming is not specifying — a row may call a concept what the corpus calls it,
identifier and all, but never uses that identifier to state a rule.
And a row's subject is a concept, never a slice, PR or review round: dates,
commit hashes and branch names live in git, build status in `BUILD.md`.
§Verification records is the one place a date appears, because there it is part
of the path being cited.*

## Product & platform
- Product positioning — overview.md; README.md
- Tech stack — overview.md
- Local-first execution — overview.md
- Docs corpus map — README.md

## Agents & models
- Agent pipeline — agents.md (intro); report-workflow.md §Step 12
- Main Agent (Head Market Analyst) responsibilities — agents.md §Main Agent
- Main Agent synthesis behavior — agents.md §Synthesis Behavior; report-workflow.md §Step 16
- Analyst Agents — agents.md §Analyst Agents; report-workflow.md §Steps 12–15
- Bull / Bear / Balanced postures — agents.md §Bull Analyst, §Bear Analyst, §Balanced Analyst
- Fixed internal models (non-configurable) — agents.md §Fixed Internal Models
  - Headline Filtering — agents.md §Headline Filtering; report-workflow.md §Step 7
  - Data Extraction (no model stage) — agents.md §Data Extraction
  - Research Routing — agents.md §Research Routing; report-workflow.md §Step 8
  - Embeddings — storage.md §Embeddings
- User-configurable agent models — configuration.md §Agent Model Configuration
- Analyst skills — analyst-skills.md

## Configuration & validation
- Settings overview — configuration.md §Settings Overview; interface.md (Settings tree)
- Agent model selection — configuration.md §Agent Model Configuration
- API tokens — configuration.md §API Tokens; data-sources.md §LLM Providers
- External data provider credentials — configuration.md §External Data Provider Credentials; data-sources.md
- Execution gate / pre-run validation — configuration.md; report-workflow.md §Step 1

## Job execution & runtime
- On-demand report generation — scheduling.md §Generating a Report
- The report job — scheduling.md §The Market Signal Report Job
- Job states — scheduling.md §Job States
- Application runtime — scheduling.md §Application Runtime
- Offline behavior — scheduling.md §Offline Behavior
- Concurrent job protection — scheduling.md §Concurrent Job Protection
- Job status visibility — scheduling.md §Job Status Visibility
- Error handling — scheduling.md §Error Handling

## Run tracking & cancellation
- Live run tracker — run-tracking.md §What the Tracker Shows; interface.md
- Per-request pass/fail rows — run-tracking.md §What the Tracker Shows
- Streamed main-agent output — run-tracking.md §What the Tracker Shows
- Step-scoped reasoning stream — run-tracking.md §What the Tracker Shows
- Thought-log capture (diagnostic sink) — run-tracking.md §Thought-log capture
- Job cancellation — run-tracking.md §Cancellation; scheduling.md §Job States
- Run-is-not-a-report invariant — run-tracking.md §A Run Is Not a Report
- Reaching the tracker — run-tracking.md §Reaching the Tracker

## Report workflow (18 steps)
- End-to-end step list — report-workflow.md §Steps 1–18
- News ingestion funnel — report-workflow.md §Step 7
- Research routing / research plan — report-workflow.md §Step 8
- Dynamic research + executor limits — report-workflow.md §Step 9
- Condensed research packet — report-workflow.md §Step 11; agents.md §Main Agent
- Baseline market data scan (series membership lives in data-sources.md) — report-workflow.md §Step 3
- Baseline change view — report-workflow.md §Step 3; storage.md §Baseline Snapshots
- Vector memory retrieval (pre-research §Step 4; post-research §Step 10) — report-workflow.md
- Embedding-response validation (canonical at Step 4) — report-workflow.md §Step 4; local-models.md §The local-model adapter seam

## Data sources
- Financial Modeling Prep — data-sources.md §Financial Modeling Prep
- FRED — data-sources.md §FRED
- BLS — data-sources.md §BLS
- CFTC — data-sources.md §CFTC
- Tavily — data-sources.md §Tavily
- GDELT — data-sources.md §GDELT
- LLM providers — data-sources.md §LLM Providers
- Gated-adapter retry/backoff — data-sources.md (intro retry paragraph); BUILD.md §Module boundaries (adapters)
- Planned report enrichment — data-sources.md §Planned report enrichment; report-workflow.md §Step 3, §Step 16

## Research documents
- /research-inbox and /research-archive — research-documents.md; interface.md (Research Documents)
- Supported formats — research-documents.md §Research Inbox
- Processing at job start + auto-archive — research-documents.md §Processing at Job Start; report-workflow.md §Step 6
- User permissions — research-documents.md §User Permissions

## Thesis & continuity
- Thesis continuity / evolving process — thesis-continuity.md
- Report continuity — thesis-continuity.md §Report Continuity
- Thesis stability — thesis-continuity.md §Thesis Stability
- Thesis pivot conditions — thesis-continuity.md §Thesis Pivot Conditions
- Memory-guided evolution — thesis-continuity.md §Memory-Guided Evolution; report-workflow.md §Steps 4, 10
- Retrospective audit of prior reports — report-workflow.md §Step 5; report-structure.md §Retrospective Audit

## Report format & structure
- Markdown canonical vs HTML presentation rule — report-structure.md; report-workflow.md §Steps 2, 18
- markdown-it renderer — report-structure.md §Presentation Format
- Embedded chart blocks — report-structure.md §Embedded charts
- Standard report sections — report-structure.md §Standard Report Structure
- Market Signal Thesis — report-structure.md §Market Signal Thesis; agents.md §Synthesis Behavior
- Index Picture — report-structure.md §Standard Report Structure
- Investment Strategy — report-structure.md §Investment Strategy

## Storage & retention
- Markdown file storage + naming — storage.md §Markdown File Storage; export.md §Export Naming
- SQLite — storage.md §SQLite
- risk_posture / market_cycle fixed vocabularies — storage.md §SQLite
- Report summary metadata schema — storage.md §Report Summary Metadata Schema
- Report retention — storage.md §SQLite
- Per-report baseline snapshots + change view — storage.md §Baseline Snapshots; report-workflow.md §Step 3
- Baseline-snapshot retention — storage.md §Baseline Snapshots
- Vector memory — storage.md §Vector Memory; report-workflow.md §Steps 4, 10, 17
- Durable-learning retention — storage.md §Vector Memory

## Interface
- Main layout tree — interface.md §Main Layout
- Latest Report View / Recent Reports Sidebar — interface.md; report-workflow.md §Step 18
- Shared-history sidebar / Portfolio-runs history — interface.md §Main Layout; portfolio-analysis.md §Storage and display
- Run Tracker — interface.md; run-tracking.md
- Persistent Warning Area — interface.md §Persistent Warning Area; scheduling.md §Error Handling

## Export
- Export options — export.md §Export Options
- PDF via Tauri webview print-to-PDF — export.md §PDF Export
- Export naming convention — export.md §Export Naming
- Export behavior — export.md §Export Behavior
- Per-report export vs whole-corpus backup (unrelated features) — export.md (intro)

## Data portability (whole-corpus export / import)
- Data portability overview — data-portability.md; BUILD.md §Data model & storage
- What moves vs excluded — data-portability.md §What moves, and what deliberately does not
- Structured versioned archive — data-portability.md §The archive, §Why a structured archive, not a DB-file copy, §Import flow, §Build-order placement
- Optional passphrase encryption — data-portability.md §Optional passphrase encryption
- Export flow — data-portability.md §Export flow
- Import flow — data-portability.md §Import flow
- Vector-memory embedder binding on import — data-portability.md §Vector memory is embedder-bound; storage.md §Local Vector Memory
- Build placement — data-portability.md §Build-order placement
- Confirmation dialog — market-signal-design-system (colors_and_type.css, preview/confirmation-dialog.html); data-portability.md §Import flow; BUILD.md §Module boundaries (frontend)

## Local analysis suite

*The docs below describe designed and built features without distinction;
build status lives in `BUILD.md`.*

- Local analysis suite overview (local-only) — overview.md §Local Analysis Suite; local-models.md
- Local model substrate — local-models.md §Serving runtime, §The model roster and per-task routing
- Local model operational reference — local-model-operations.md; local-models.md §The model roster and per-task routing
- Local-model per-stage options wiring — local-model-operations.md §Sampling settings, §The `num_ctx` trap, §M5 pre-flight checklist
- Model residency default — local-models.md §The model roster and per-task routing
- Local-model adapter seam — local-models.md §The local-model adapter seam
- Schema-constrained output — local-models.md §Schema-constrained output
- Context-memory discipline — local-models.md §Context-memory discipline
- Per-job isolated vector memory — local-models.md §Run history and continuity; storage.md §Local Vector Memory
- Web research tool — web-research.md
- Source registry & evidence tiers — data-sources.md §Source registry and evidence tiers; web-research.md §Source quality and evidence weighting; configuration.md §Web Research
- Source quality & evidence weighting — web-research.md §Source quality and evidence weighting
- Connected Sources — web-research.md §Connected sources; configuration.md §Connected Sources (subscriptions)
- Charles Schwab integration — schwab-integration.md
- Schwab connection requirement — schwab-integration.md §A connected Schwab account is required
- Manual holdings import — schwab-integration.md §Manual import (supplement)
- Options-activity signal — schwab-integration.md; data-sources.md §CBOE; portfolio-analysis.md

### Portfolio Analysis
- Portfolio Analysis job — portfolio-analysis.md
- Portfolio Analysis workflow — portfolio-workflow.md
- Holding verdict schema — portfolio-analysis.md §The holding verdict
- Two-arm verdict — Portfolio form (the boundary statement is single-homed) — portfolio-analysis.md §The holding verdict, §Intrinsic verdict, §Portfolio action, §Outcome learning, §Storage and display, §Starting parameters; portfolio-workflow.md §Step 6d, §Step 6f, §Step 6g, §Step 7; local-models.md §Context-memory discipline; storage.md §Local Analysis Suite Storage
- Intrinsic-verdict vs portfolio-action separation — portfolio-analysis.md §Intrinsic verdict, §Portfolio action; portfolio-workflow.md §Step 6f
- Intrinsic-verdict discriminated union — portfolio-analysis.md §Intrinsic verdict, §Asset eligibility; portfolio-workflow.md §Step 6f; storage.md §Local Analysis Suite Storage; interface.md §Main Layout
- Portfolio action — the per-holding action call — portfolio-analysis.md §Portfolio action; portfolio-workflow.md §Step 6f
- Action sizing (retired) — portfolio-analysis.md §Starting parameters, §Portfolio roll-up
- Capital-efficiency / dead-money exit + sunk-cost guard — portfolio-analysis.md §The holding verdict, §Portfolio action, §Starting parameters; portfolio-workflow.md §Step 6b, §Step 6e, §Step 6f; configuration.md §Investor Profile
- Portfolio three-layer engine — portfolio-analysis.md §The per-holding pipeline; portfolio-workflow.md
- Grade bands & parameter versioning — portfolio-analysis.md §Starting parameters; data-sources.md §SEC EDGAR, §Portfolio Analysis — endpoint surface
- Interpretation-prompt contract — portfolio-analysis.md §The holding verdict; portfolio-workflow.md §Step 6f
- Position thesis ledger — portfolio-analysis.md §The position thesis ledger, §Storage and display; portfolio-workflow.md §Step 6a, §Step 6f, §Step 6g; storage.md §Local Analysis Suite Storage; interface.md §Main Layout
- Ledger executability validation — portfolio-workflow.md §Step 6f, §Step 6g; portfolio-analysis.md §The position thesis ledger, §The quick check
- Portfolio quick check — portfolio-analysis.md §The quick check; portfolio-workflow.md §The quick check; interface.md §Connection status
- Selective re-analysis + mixed-vintage safety — portfolio-analysis.md §Triggering; portfolio-workflow.md §Step 6, §Step 7
- Evidence events — portfolio-analysis.md §Starting parameters
- Portfolio pre-profit execution / financing overlay — portfolio-analysis.md §The per-holding pipeline, §Starting parameters; portfolio-workflow.md §Step 6b–6g; data-sources.md §Portfolio Analysis — endpoint surface; storage.md §Local Analysis Suite Storage
- Portfolio outcome learning — portfolio-analysis.md §Outcome learning, §Starting parameters; portfolio-workflow.md §Step 7; storage.md §Local Analysis Suite Storage
- Portfolio hard-forensic outcome — portfolio-analysis.md §Portfolio action, §Starting parameters; portfolio-workflow.md §Step 6g; trade-opportunities.md §Starting parameters; storage.md §Local Analysis Suite Storage
- What-changed audit — portfolio-analysis.md §What changed; portfolio-workflow.md §Step 6g
- Run audit record provenance (source labels, model ids) — storage.md §Local Analysis Suite Storage; portfolio-workflow.md §Step 7
- Degraded-run persistence + constructed marker (removed by the fresh-start slice) — verification/2026-08-17-fresh-start-legacy-removal.md; BUILD.md §Runtime, observability & failure posture
- Holdings normalization / book-level netting — schwab-integration.md §What is pulled, §Manual import (supplement); portfolio-workflow.md §Step 2; portfolio-analysis.md §Holdings change tracking
- Holdings change tracking — portfolio-analysis.md §Holdings change tracking
- Net-short equity handling — portfolio-analysis.md §Asset eligibility, §Triggering, §Holdings change tracking, §Outcome learning
- Not-rated positions in roll-up — portfolio-analysis.md §Asset eligibility, §Portfolio roll-up; schwab-integration.md §What is pulled
- Fund path — portfolio-analysis.md §Asset eligibility
- Fund strategy classification & routing — portfolio-analysis.md §Asset eligibility; portfolio-workflow.md §Step 3, §Step 6b
- Listing-resolution guard — portfolio-analysis.md §Asset eligibility, §Starting parameters; portfolio-workflow.md §Step 3, §Step 6a
- House-view freshness gate — portfolio-workflow.md §Step 5
- Post-research target refinement — portfolio-analysis.md §The per-holding pipeline; portfolio-workflow.md §Step 6d, §Step 6e
- Portfolio technology-event impact — portfolio-analysis.md §The position thesis ledger, §The per-holding pipeline; portfolio-workflow.md §Step 6c
- Research reuse (Portfolio) — portfolio-analysis.md §Starting parameters, §The per-holding pipeline; portfolio-workflow.md §Step 6
- New-money admission test — portfolio-analysis.md §Starting parameters
- Portfolio per-holding/per-fund endpoint surface — data-sources.md §Portfolio Analysis — endpoint surface
- Investor profile default preset — configuration.md §Investor Profile; interface.md §Main Layout (Settings tree)

### Trade Opportunities
- Trade Opportunities job — trade-opportunities.md
- Trade Opportunities workflow — trade-opportunities-workflow.md
- Trade Opportunities — what it hunts — trade-opportunities.md §What the job hunts
- Trade Opportunities research method — trade-opportunities.md §The research method
- Opportunity schema — trade-opportunities.md §The opportunity; trade-opportunities-workflow.md §Step 5g, §Step 5h; storage.md §Local Analysis Suite Storage
- Two-arm contract — TO form (the boundary statement is single-homed) — trade-opportunities.md §The opportunity, §Starting parameters; local-models.md §Context-memory discipline; trade-opportunities-workflow.md §Step 5g, §Step 5h; storage.md §Local Analysis Suite Storage
- Either-arm admission — trade-opportunities.md §The opportunity, §Evidence floor, §Outcome learning, §Starting parameters; trade-opportunities-workflow.md §Step 5h, §Step 7
- Blind-first diagnostic reservation — trade-opportunities.md §The opportunity, §Failure posture; trade-opportunities-workflow.md §Step 5g
- Opportunity archetype lens — trade-opportunities.md §Archetype
- Archetype stickiness on carried-forward names — trade-opportunities.md §Archetype; trade-opportunities-workflow.md §Step 5a
- Archetype classification prefetch + low-confidence branch — trade-opportunities-workflow.md §Step 5a, §Step 5b; data-sources.md §Trade Opportunities — endpoint surface
- Research-driven candidate discovery — trade-opportunities.md §The pipeline; trade-opportunities-workflow.md §Step 3
- Post-earnings surprise screen — trade-opportunities.md §The pipeline; trade-opportunities-workflow.md §Step 3a; data-sources.md §Trade Opportunities — endpoint surface
- Model-led hypothesis research lane — trade-opportunities-workflow.md §Step 3b
- Discovery route-topic proposal (the one model-proposed agenda) — trade-opportunities-workflow.md §Step 3b; web-research.md §The research loop and context management
- Discovery coverage rotation / ledger — trade-opportunities-workflow.md §Step 3b; trade-opportunities.md §Starting parameters; configuration.md §Local Analysis Suite Configuration; storage.md §Local Analysis Suite Storage
- Discovery memory / opportunity graph — trade-opportunities.md §Discovery memory; trade-opportunities-workflow.md §Step 3c, §Step 7, §ATO: the audit flow
- Research-watchlist refresh lane — trade-opportunities-workflow.md §Step 3c; trade-opportunities.md §Starting parameters; configuration.md §Local Analysis Suite Configuration; storage.md §Local Analysis Suite Storage
- Watchlist cap eviction — trade-opportunities-workflow.md §Step 3c; trade-opportunities.md §Starting parameters, §Discovery memory, §Outcome learning; configuration.md §Local Analysis Suite Configuration
- Discovery diversity guardrails + research budget — trade-opportunities-workflow.md §Step 4, §Step 6; trade-opportunities.md §The pipeline, §The opportunity space; configuration.md §Local Analysis Suite Configuration
- Narrative-vs-reality ratio + forensic risk gate + base-rate conjunction discipline — trade-opportunities.md §The pipeline, §The two non-negotiables; trade-opportunities-workflow.md §Step 5c
- Leading-metric inflection gate — trade-opportunities.md §The two non-negotiables, §Starting parameters
- Limited-history support — trade-opportunities.md §Evidence floor, §Starting parameters; trade-opportunities-workflow.md §Step 5b, §Step 5e, §Step 5f, §Step 5h; data-sources.md §SEC EDGAR, §Trade Opportunities — endpoint surface; configuration.md §Local Analysis Suite Configuration; storage.md §Local Analysis Suite Storage
- Historical episode library — trade-opportunities.md §The lenses, §Starting parameters; trade-opportunities-workflow.md §Step 5d
- Event-impact / value-chain repricing lens — trade-opportunities.md §The event-impact / value-chain repricing lens; trade-opportunities-workflow.md §Step 3b; storage.md §Local Analysis Suite Storage
- Implied-expectations read — trade-opportunities.md §The pipeline; trade-opportunities-workflow.md §Step 5c, §Step 5g
- Cross-lens contradiction / falsification check — trade-opportunities.md §Reconciling the lenses; trade-opportunities-workflow.md §Step 5e, §Step 5g, §Step 5h
- Conviction-cap ceiling & precedence — trade-opportunities.md §Starting parameters, §Reconciling the lenses; trade-opportunities-workflow.md §Step 5g, §Step 5h; portfolio-analysis.md §Starting parameters; portfolio-workflow.md §Step 6g; storage.md §Local Analysis Suite Storage
- Engine-arm conviction stand-in (the Step-5h computation is canonical; never a Step-5g input) — trade-opportunities.md §Starting parameters, §The opportunity; trade-opportunities-workflow.md §Step 5g, §Step 5h; storage.md §Local Analysis Suite Storage
- Key-falsifier / milestone re-check classes (the canonical vocabulary, owned by §Step 3c) — trade-opportunities.md §The opportunity, §Reconciling the lenses; trade-opportunities-workflow.md §Step 3c, §Step 5e, §Step 5h, §Step 7, §ATO: the audit flow; storage.md §Local Analysis Suite Storage
- Thesis milestone plan + horizon assignment — trade-opportunities.md §The opportunity, §Starting parameters; trade-opportunities-workflow.md §Step 5e, §Step 5g, §Step 5h, §Step 7; storage.md §Local Analysis Suite Storage
- Matrix final assembly over the union — trade-opportunities-workflow.md §Step 6, §Step 7; trade-opportunities.md §The opportunity; storage.md §Local Analysis Suite Storage
- Opportunity re-evaluation lifecycle — trade-opportunities-workflow.md §Step 7, §ATO: the audit flow; trade-opportunities.md §The two jobs, §The opportunity, §Archived opportunities, §Starting parameters; storage.md §Local Analysis Suite Storage; interface.md; local-models.md §Serving runtime
- DTO deep-budget rotation — trade-opportunities.md §The two jobs, §Archived opportunities; trade-opportunities-workflow.md §Step 4, §Step 7; configuration.md §Local Analysis Suite Configuration
- Carried-name hard-trigger forced archival — trade-opportunities.md §Starting parameters, §Archived opportunities; trade-opportunities-workflow.md §Step 5h; storage.md §Local Analysis Suite Storage
- Archived opportunities — trade-opportunities.md §Archived opportunities; trade-opportunities-workflow.md §Step 7, §Step 9, §Step 10; storage.md §Local Analysis Suite Storage; interface.md §Main Layout
- Trade Opportunities outcome learning / calibration — trade-opportunities.md §Outcome learning, §Storage and display; trade-opportunities-workflow.md §Step 5c, §Step 5g, §Step 7, §Step 9; storage.md §Local Analysis Suite Storage; data-sources.md §Trade Opportunities — endpoint surface
- Shadow outcome ledger / picked-vs-rejected calibration — trade-opportunities.md §Outcome learning, §Starting parameters; trade-opportunities-workflow.md §Step 3c, §Step 5h, §Step 6, §Step 7, §Step 9; storage.md §Local Analysis Suite Storage; configuration.md §Local Analysis Suite Configuration
- Outcome measurement contract — trade-opportunities.md §Outcome learning; storage.md §Local Analysis Suite Storage
- Picked decision episodes / lifecycle id — trade-opportunities.md §Outcome learning, §Starting parameters; storage.md §Local Analysis Suite Storage; trade-opportunities-workflow.md §Step 5b, §Step 7; configuration.md §Local Analysis Suite Configuration; data-portability.md §Build-order placement
- Trade Opportunities endpoint surface — data-sources.md §Trade Opportunities — endpoint surface, §FMP — current paid-plan tier audit; storage.md §Local Analysis Suite Storage (price-bar cache)
- Trade Opportunities signal inputs — trade-opportunities.md §Signal inputs; data-sources.md §Local analysis suite — shared sourcing, §Trade Opportunities — endpoint surface
- TO research-target scenario bridge — trade-opportunities.md §The opportunity, §Starting parameters; trade-opportunities-workflow.md §Step 5e, §Step 5f, §Step 5g, §Step 5h, §Step 7, §ATO: the audit flow; storage.md §Local Analysis Suite Storage
- Entry asymmetry threshold — trade-opportunities.md §Starting parameters, §The opportunity; trade-opportunities-workflow.md §Step 5h, §Step 2
- TO research cache — trade-opportunities.md §Failure posture, §Starting parameters; trade-opportunities-workflow.md §Step 3c, §Step 5, §ATO; storage.md §Local Analysis Suite Storage (web-research document cache)
- Trade Opportunities persisted structures — storage.md §Local Analysis Suite Storage; trade-opportunities-workflow.md §Step 9

### Shared across both jobs
- Deterministic financial-analysis engine — local-models.md §Context-memory discipline; portfolio-analysis.md
- Evidence floor (each job defines its own) — portfolio-analysis.md §Evidence floor; trade-opportunities.md §Evidence floor; trade-opportunities-workflow.md §Step 5h, §Step 7
- Evidence-floor freshness basis — trade-opportunities.md §Starting parameters, §Evidence floor; trade-opportunities-workflow.md §Step 5h; web-research.md §Source quality and evidence weighting; portfolio-analysis.md §Evidence floor
- Deterministic risk-tier assignment (the TO rule is canonical) — trade-opportunities.md §The opportunity space, §Starting parameters; portfolio-analysis.md §Starting parameters; portfolio-workflow.md §Step 6b
- Scenario-target function — portfolio-analysis.md §Starting parameters, §Evidence floor; trade-opportunities.md §Starting parameters, §Evidence floor; portfolio-workflow.md §Step 6b; trade-opportunities-workflow.md §Step 5c; data-sources.md (both `analyst-estimates` rows, both `dividends` rows)
- Rate-anchor failure rule — portfolio-analysis.md §Failure posture, §Starting parameters, §The quick check; trade-opportunities.md §Failure posture; trade-opportunities-workflow.md §ATO: the audit flow; data-sources.md §Portfolio Analysis — endpoint surface (FRED), §Trade Opportunities — endpoint surface (FRED)
- Factor normalization basis — trade-opportunities-workflow.md §Step 5c; trade-opportunities.md §Starting parameters, §The lenses; storage.md §Local Analysis Suite Storage
- ET session dating — data-sources.md (intro session-dating rule); portfolio-analysis.md §The quick check, §Triggering, §Outcome learning; portfolio-workflow.md §The quick check
- Run data-health roll-up — portfolio-analysis.md §Portfolio roll-up, §Starting parameters; interface.md §Main Layout
- Re-check class resolution contract — trade-opportunities-workflow.md §Step 3c, §Step 5h; portfolio-workflow.md §Step 6g
- Per-item checkpoint/resume + research caching, both jobs — portfolio-analysis.md §Failure posture; portfolio-workflow.md §Step 6; trade-opportunities.md §Failure posture; trade-opportunities-workflow.md §Step 5
- Research loop & context management — web-research.md §The research loop and context management
- Research agenda — portfolio-analysis.md; trade-opportunities.md
- Seed lineage — web-research.md §The research loop and context management; trade-opportunities-workflow.md §Step 3b; configuration.md §Research Context Management; storage.md §Local Analysis Suite Storage; trade-opportunities.md §The opportunity, §Signal inputs, §Discovery memory
- Hierarchical distillation — web-research.md §The research loop and context management; trade-opportunities-workflow.md §Step 5e; portfolio-workflow.md §Step 6d; configuration.md §Research Context Management
- Disconfirming-fetch pass (each job's placement is canonical in its own workflow) — portfolio-workflow.md §Step 6c; trade-opportunities-workflow.md §Step 5d; web-research.md §Source quality and evidence weighting
- Heavy-route sub-distillation — trade-opportunities-workflow.md §Step 3b, §Step 4; configuration.md §Research Context Management
- SEC EDGAR primary source — data-sources.md §SEC EDGAR
- SEC EDGAR role for Trade Opportunities — data-sources.md §SEC EDGAR
- FMP paid-tier suite signals — data-sources.md §Local analysis suite — shared sourcing, §FMP — current paid-plan tier audit
- FMP paid-plan tier audit — data-sources.md §FMP — current paid-plan tier audit
- FINRA short interest — data-sources.md §FINRA
- Benchmark / sector / commodity identities + adjustment convention — data-sources.md §Financial Modeling Prep
- Suite data dispersal — data-sources.md §Local analysis suite — shared sourcing
- Local analysis suite configuration — configuration.md §Local Analysis Suite Configuration, §Research Context Management
- Local analysis suite storage + per-feature retention — storage.md §Local Analysis Suite Storage
- Local suite pages — interface.md §Main Layout
- Suite sorting & views — interface.md §Main Layout, §Persistent Warning Area; portfolio-analysis.md §Storage and display; portfolio-workflow.md §Step 8; trade-opportunities.md §Storage and display; trade-opportunities-workflow.md §Step 10; market-signal-design-system (SKILL.md, README.md §Analytical-register controls, colors_and_type.css, ui_kits Analytical.jsx, preview/analytical-controls.html)

## Verification records

*Evidence files under `docs/verification/`, named for what each covers.
Most are dated, point-in-time records; the contracts they tested live in the
docs cited beside them.
The watch set is the exception — it is written before the run it checks, and
the run's own dated record follows it.*

- Local-model serving pre-flight — verification/2026-07-28-m5-preflight.md; local-model-operations.md §M5 pre-flight checklist
- First live Portfolio run — verification/2026-07-31-first-live-portfolio-run.md
- FMP light-EOD adjustment-basis probe — verification/2026-08-02-fmp-light-eod-adjustment-basis.md; data-sources.md §Financial Modeling Prep
- Grade-band calibration — verification/2026-08-03-grade-band-shadow-tune.md; portfolio-analysis.md §Starting parameters
- Portfolio code-vs-docs conformance — verification/2026-08-04-piece2-conformance-walk.md; verification/2026-08-05-piece2-conformance-rerun.md
- Deterministic value-chain correctness — verification/2026-08-05-piece3-value-chain-walk.md
- Residual conformance + off-spine doc coverage — verification/2026-08-07-scoped-conformance-check.md
- Big confirmation run — watch set (forward-looking; the run's dated record follows) — verification/big-run-watch-set.md; BUILD.md §What remains
- Big confirmation run — attempt 1 — verification/2026-08-10-big-run-attempt-1.md; portfolio-analysis.md §Failure posture
- Big confirmation run — attempt 2 (analysis, rulings, and the fix slices) — verification/2026-08-13-big-run-attempt-2.md; portfolio-analysis.md §Portfolio roll-up, §Starting parameters
- Stooq removal — decision, evidence, and removal-slice inventory — verification/2026-08-12-stooq-removal-decision.md; BUILD.md §What remains (Built)
- Tunnel-vision slice — ruling, build inventory, and per-finding dispositions — verification/2026-08-14-tunnel-vision-slice.md; portfolio-analysis.md §Portfolio action, §Portfolio roll-up
- Tunnel-vision doc↔code conformance walk — findings, rulings, and applied corrections — verification/2026-08-15-tunnel-vision-conformance-walk.md
- Selective-run safety additions → card badges — ruling, build inventory, and held-name-lane / pre-v9-gate retirement — verification/2026-08-16-selective-badges-ruling.md; portfolio-analysis.md §Triggering
- Fresh-start legacy removal — the ruling, the full pre-`v9` removal inventory, and the kept-vs-removed boundary — verification/2026-08-17-fresh-start-legacy-removal.md; BUILD.md §Runtime, observability & failure posture
- Portfolio Analysis doc/code audit — the 21 findings, their re-verification and dispositions, four rulings, and two Codex rounds — verification/2026-08-18-portfolio-analysis-doc-code-audit.md; BUILD.md §What remains (Built)
