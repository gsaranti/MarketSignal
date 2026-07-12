# Index

*Concept → file:section map. Originally written by /metis-reconcile; now
hand-maintained — entries are lookup pointers, not summaries: open the cited
doc section rather than working from the clause here.*

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
- API tokens (OpenAI, Anthropic; the save-disable rule scoped to the cloud agent/token submission — provider credentials + local-suite fields independently savable, the save split a named code prerequisite of the local-suite Settings slice) — configuration.md §API Tokens; data-sources.md §LLM Providers
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
- Error handling (fail-hard vs fail-soft classification = the owning workflow's contract) — scheduling.md §Error Handling

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
- Baseline market data scan (13 canonical groups by reference — series membership single-homed in data-sources.md; news owned by Step 7 alone) — report-workflow.md §Step 3
- Baseline change view (deltas since previous report) — report-workflow.md §Step 3; storage.md §Baseline Snapshots
- Vector memory retrieval — pre-research (steers audit + routing) §Step 4; post-research (research-informed) §Step 10 — report-workflow.md
- Embedding-response validation (report: canonical at Step 4, Steps 10/17 point to it; local suite: the shared validator) — report-workflow.md §Step 4; local-models.md §The local-model adapter seam

## Data sources
- Financial Modeling Prep (primary financial-data source; report endpoint→path table — 12 free wired + 7 planned paid, all on `/stable`) — data-sources.md §Financial Modeling Prep
- FRED (+ 32-series ID table by baseline group; `/series/observations` + `/release/dates`) — data-sources.md §FRED
- BLS (+ 4-series ID table; `/timeseries/data/`) — data-sources.md §BLS
- CFTC (Commitments-of-Traders positioning; keyless Socrata datasets `gpe5-46if` / `72hh-3qpy`) — data-sources.md §CFTC
- Tavily (primary research/news ingestion; `/search` endpoint) — data-sources.md §Tavily
- GDELT (geopolitical/event monitoring; DOC 2.0 endpoint) — data-sources.md §GDELT
- LLM providers (OpenAI, Anthropic) — data-sources.md §LLM Providers
- Planned report enrichment (paid FMP — calendar consensus+surprise as per-event `surprises[]` via a versioned per-event polarity-mapped release↔event map (complete 17-row drafted table over the 7 tracked releases, keyed on the numeric FRED `release_id` w/ the release name display-only; FMP event strings pending paid-key verification — a named blocking prerequisite), historical valuation/performance w/ single-homed derivation rules, IPO/M&A froth w/ deterministic windows + completion states; complete persisted payload w/ absent-never-zero semantics; engine-derived, out of the level-delta engine) — data-sources.md §Planned report enrichment; report-workflow.md §Step 3, §Step 16

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
- Persistent Warning Area (cloud + local-suite categories, de-dup; condition-owned blocking warnings = non-dismissible gate state, failed-jobs the one dismissible category) — interface.md §Persistent Warning Area; scheduling.md §Error Handling

## Export
- Export options (Markdown, PDF) — export.md §Export Options
- PDF via Tauri webview print-to-PDF — export.md §PDF Export
- Export naming convention — export.md §Export Naming
- Export does not re-run workflow — export.md §Export Behavior
- Per-report export vs whole-corpus backup (unrelated features — single report → export.md; entire store → data-portability.md) — export.md (intro)

## Data portability (whole-corpus export / import — built, PRs #53/#54)
- Data portability overview (backup/restore of the entire store; `portability.rs` + `export_data`/`import_data_inspect`/`import_data`) — data-portability.md; BUILD.md §Data model & storage
- What moves vs excluded (durable analytical data moves; `app_settings`/Keychain/`job_runs`/telemetry stay behind — secrets never serialized) — data-portability.md §What moves, and what deliberately does not
- Structured versioned archive (manifest + per-table NDJSON + files; deliberately not a raw DB-file copy) — data-portability.md §The archive, §Why a structured archive, not a DB-file copy
- Optional passphrase encryption (AES-256-GCM / Argon2id, frozen KDF parameters; lost passphrase unrecoverable) — data-portability.md §Optional passphrase encryption
- Export flow (Settings Data section → save dialog → `export_data`; holds the single run slot) — data-portability.md §Export flow
- Import flow (fresh-load or replace-all-with-confirmation, merge deferred; everything validated pre-destructively; `markdown_path` re-derived; `app_settings` untouched) — data-portability.md §Import flow
- Vector-memory embedder binding on import (report namespace portable; local namespaces re-embed on identity mismatch — identity compared, never dimension — content retained) — data-portability.md §Vector memory is embedder-bound
- Build placement (independent of the local suite — suite coverage = a format-extension rule + versioned import entry-set, never automatic) — data-portability.md §Build-order placement
- Confirmation dialog (design-package generic-chrome extension: `.dialog-*` + `--scrim`; first use = import replace-all) — market-signal-design-system (colors_and_type.css, preview/confirmation-dialog.html); data-portability.md §Import flow; BUILD.md §Module boundaries (frontend)

## Local analysis suite

*Status: substrate, narrow single-equity Portfolio slice, Schwab OAuth +
Connect surface, holdings diff, and the Portfolio page are built; full
Portfolio (funds) and Trade Opportunities are designed, not built — the docs
below describe both without distinction; build status lives in BUILD.md.*

- Local analysis suite overview (local-only, two prescriptive features) — overview.md §Local Analysis Suite; local-models.md
- Local model substrate (Ollama serving, roster, per-task routing) — local-models.md §Serving runtime, §The model roster and per-task routing
- Local model operational reference (Qwen3.5-122B context / thinking / sampling / `num_ctx` / `format` gotchas; serving pre-flight; M5-gated) — local-model-operations.md; local-models.md §The model roster and per-task routing
- Model residency default (one 122B fills all reasoning roles + embedder resident; 35B fast tier benchmark-gated) — local-models.md §The model roster and per-task routing
- Local-model adapter seam (flexible endpoint/model_id client, distinct from the cloud AgentModel enum) — local-models.md §The local-model adapter seam
- Schema-constrained output (grammar-constrained JSON) — local-models.md §Schema-constrained output
- Context-memory discipline (distilled hand-offs, retrieve-don't-dump) — local-models.md §Context-memory discipline
- Per-job isolated vector memory (three partitions; entry kind = purpose boundary, TO rows lifecycle-tagged; embedder-identity re-embed on change; durable-learning retention carve-out) — local-models.md §Run history and continuity; storage.md §Local Vector Memory
- Web research tool (SearXNG-primary, Tavily fallback, fetch/extract) — web-research.md
- Source registry & evidence tiers (per-domain tier 0–5 / deny + evidenceKinds; a thin override over heuristic defaults) — data-sources.md §Source registry and evidence tiers; web-research.md §Source quality and evidence weighting; configuration.md §Web Research
- Source quality & evidence weighting (quality informs conviction, never gates discovery; app-computed vs model-derived annotations; lane policy; diversity caps) — web-research.md §Source quality and evidence weighting
- Connected Sources (optional subscription enrichment; webview login → Keychain session; health-tested; never on the execution gate) — web-research.md §Connected sources; configuration.md §Connected Sources (subscriptions)
- Charles Schwab integration (OAuth loopback, 30-min/7-day token lifecycle, positions, account hashing) — schwab-integration.md
- Manual holdings import (CSV/paste fallback; never clears the gate) — schwab-integration.md §Manual import fallback
- Portfolio Analysis job (per-holding pipeline → intrinsic verdict; whole-book construction → portfolio action) — portfolio-analysis.md
- Holding verdict schema (intrinsic verdict vs portfolio action; what-changed split) — portfolio-analysis.md §The holding verdict
- Intrinsic-verdict vs portfolio-action separation (per-holding loop → Step 7a deterministic aggregates → 7b model reconciliation) — portfolio-analysis.md §Portfolio roll-up and construction; portfolio-workflow.md §Step 7
- Action-sizing spine (engine bounds the feasible action set; model proposes within, app constrains) — portfolio-analysis.md §The holding verdict; portfolio-workflow.md §Step 7a
- Capital-efficiency / dead-money exit + sunk-cost guard (forward total return vs three-state hurdle — only *fails* is dead money; never moves the grade) — portfolio-analysis.md §The holding verdict, §Portfolio roll-up and construction, §Starting parameters; portfolio-workflow.md §Step 6b, §Step 6e, §Step 7; configuration.md §Investor Profile
- Position thesis ledger (persisted per-holding standing thesis, monitors, typed falsifiers, triggers; each holding card's anchor) — portfolio-analysis.md §The position thesis ledger, §Storage and display; portfolio-workflow.md §Step 6a, §Step 6f, §Step 6g; storage.md §Local Analysis Suite Storage; interface.md §Main Layout
- Portfolio technology-event impact (held-name form of TO's event-impact lens; typed `technology_read`; a first-class qualitative falsifier class) — portfolio-analysis.md §The position thesis ledger, §The per-holding pipeline; portfolio-workflow.md §Step 6c
- Holdings normalization / book-level netting (same-symbol rows across granted accounts + manual supplements net at snapshot assembly — signed quantities, market values, and app-derived signed cost-basis totals each sum, never a share-weighted average; per-source rows display/audit-only; the netting step = a named code prerequisite of the full Portfolio slice) — schwab-integration.md §What is pulled, §Manual import (supplement); portfolio-workflow.md §Step 2; portfolio-analysis.md §Holdings change tracking
- Holdings change tracking (deterministic prior-run-snapshot diff over the normalized book-level rows — absolute quantity same-side, signed swing on a sign flip; exited names in the roll-up) — portfolio-analysis.md §Holdings change tracking
- Net-short equity handling (not-rated w/ short reason; signed exposure in the roll-up; long↔short reversal force-include; `reversed` alignment tag) — portfolio-analysis.md §Asset eligibility, §Triggering, §Holdings change tracking, §Outcome learning
- Portfolio Analysis workflow (end-to-end Type-tagged control flow; local-model-call contracts) — portfolio-workflow.md
- Portfolio three-layer engine (grade core / conviction layer / positioning context) — portfolio-analysis.md §The per-holding pipeline; portfolio-workflow.md
- Portfolio per-holding/per-fund endpoint surface (FMP per-symbol subset + keyless fallback/cross-check rows) — data-sources.md §Portfolio Analysis — endpoint surface
- Fund path (reduced compute: expense drag, exposure tilt, fund valuation; priced-fund grade = real valuation/risk + neutral-imputed absent quality; constituent look-through off-plan → N-PORT/dropped; future issuer-holdings adapter) — portfolio-analysis.md §Asset eligibility
- Not-rated positions in roll-up (excluded from grading; a material position feeds the whole-book aggregates via market value + signed notional — duration / credit / standalone-option delta = typed gaps) — portfolio-analysis.md §Asset eligibility, §Portfolio roll-up and construction; schwab-integration.md §What is pulled
- Listing-resolution guard (stocks; no-resolution / non-US listing → not-rated unsupported-listing; resolved-but-conflicting identity → insufficient-evidence at the floor) — portfolio-analysis.md §Asset eligibility; portfolio-workflow.md §Step 3, §Step 6a
- House-view freshness gate (older than 1 week → dropped as a gap, not fed stale) — portfolio-workflow.md §Step 5
- Post-research target refinement (typed research_forward_assumption w/ the `supplement`/`supersede` app-owned conflict policy — structured-wins default, the model never selects the rule; engine recomputes targets only, sub-scores fixed) — portfolio-analysis.md §The per-holding pipeline; portfolio-workflow.md §Step 6d, §Step 6e
- What-changed audit (intrinsic half validated at Step 6g; action half at Step 7b) — portfolio-analysis.md §The holding verdict; portfolio-workflow.md §Step 6g, §Step 7b
- Intrinsic-verdict discriminated union (`priced` / `role_risk_only`; reduced {sell all, trim, hold} spine) — portfolio-analysis.md §Intrinsic verdict, §Asset eligibility; portfolio-workflow.md §Step 6f; storage.md §Local Analysis Suite Storage; interface.md §Main Layout
- Fund strategy classification & routing (loop-time from `etf/info`; exposure-priced composite w/ ≥70%-US guard; fund-form target methodology = the open decision) — portfolio-analysis.md §Asset eligibility; portfolio-workflow.md §Step 3, §Step 6b
- Portfolio quick check (engine-only between-run ledger liveness; warn-don't-decide attention flags; typed per-family sweep states — `fresh_clear` / `flagged` / `unknown`, unknown force-includes) — portfolio-analysis.md §The quick check; portfolio-workflow.md §The quick check; interface.md §Connection status
- Selective re-analysis + mixed-vintage safety (force-include on flags / `unknown` sweeps / side reversals / evidence events, carried-action transition rule, over-age add-demotion w/ `action_source`) — portfolio-analysis.md §Triggering; portfolio-workflow.md §Step 6, §Step 7b
- Evidence events (the canonical deterministic list; drives cache invalidation, force-include, and the quiet badge) — portfolio-analysis.md §Starting parameters
- New-money admission test (add family needs base-case hurdle clearance; three-state tolerance is exit-side only) — portfolio-analysis.md §Starting parameters
- Portfolio outcome learning (recommendation-state decision episodes — no active episode post-maturity, post-maturity falsifier confirmations → the thesis ledger; total-return-primary labels; entry-stamped sector identity w/ `sector-unscorable`; pending price legs close `price-coverage-unscorable` past the shared grace; propose-only) — portfolio-analysis.md §Outcome learning, §Starting parameters; portfolio-workflow.md §Step 7a, §Step 8; storage.md §Local Analysis Suite Storage
- Graduated research depth / research reuse (~4-week window; evidence events invalidate; reuse decisions logged) — portfolio-analysis.md §The per-holding pipeline, §Starting parameters; portfolio-workflow.md §Step 6
- Trade Opportunities job (3×3 risk×horizon matrix) — trade-opportunities.md
- Trade Opportunities — what it hunts (early detection + continuation; leading-metric anchor) — trade-opportunities.md §What the job hunts
- Opportunity archetype lens (5 archetypes set signal weighting + valuation lens) — trade-opportunities.md §Archetype
- Archetype stickiness on carried-forward names (affirm-or-overturn; an overturn must cite a delta-validated changed feature) — trade-opportunities.md §Archetype; trade-opportunities-workflow.md §Step 5a
- Archetype classification prefetch + low-confidence total branch (Step 5a fetches the classification subset — `profile` + statement-derived rows — once per candidate, 5b reuses from cache, cardinality unchanged; low-confidence label stands w/ `archetype_low_confidence` flag, contradictory/invalid/failed → Step-4 provisional label adopted logged, runner-up = audit diagnostics only) — trade-opportunities-workflow.md §Step 5a, §Step 5b; data-sources.md §Trade Opportunities — endpoint surface
- Research-driven candidate discovery (three feeders; model generates candidates, app validates) — trade-opportunities.md §The pipeline; trade-opportunities-workflow.md §Step 3
- Post-earnings surprise screen (paid earnings calendar read backward → the continuation-mode structured feeder; SUE-standardized, revenue-agreement-prioritized) — trade-opportunities.md §The pipeline; trade-opportunities-workflow.md §Step 3a; data-sources.md §Trade Opportunities — endpoint surface
- Model-led hypothesis research lane (Step 3b routes incl. mandatory graph-blind outside view; the research-strategy-planning call + per-route research/card-formation call contracts; hypothesis cards + hypothesis score gating promotion before any ticker) — trade-opportunities-workflow.md §Step 3b
- Discovery memory / opportunity graph (persisted theme→mechanism→…→falsifiers graph; node statuses picked / watchlist / retired / departed — departed = an archived pick's terminal tombstone taken on any deep-invalidation archival (DTO Step 7 or ATO Deep Audit), visible-in-context but never a feeder, excluded from shadow scoring; carried-forward watchlist re-checked each run) — trade-opportunities.md §Discovery memory; trade-opportunities-workflow.md §Step 3c, §Step 7, §ATO: the audit flow
- FMP paid-plan tier audit (`*-bulk` / transcripts / 13F / fund-holdings / press-releases off-plan → fallbacks) — data-sources.md §FMP — current paid-plan tier audit
- Narrative-vs-reality ratio (+ operating-reality-vs-price fallback for thin coverage) + forensic risk gate + base-rate conjunction discipline — trade-opportunities.md §The pipeline, §The two non-negotiables; trade-opportunities-workflow.md §Step 5c
- Conviction-cap ceiling & precedence (soft triggers — forensic soft flags / anchored `hype` / high-severity contradiction — share the categorical **Medium** ceiling, min over matched rules; hard triggers exclude, taking precedence; ceiling binds **after** the validated raise — `final = min(base + raise, ceiling)`; matched cap rule persisted; contradiction caps-never-excludes; since-flagged stays directionally cap-only; Portfolio shares by pointer) — trade-opportunities.md §Starting parameters, §Reconciling the lenses; trade-opportunities-workflow.md §Step 5g, §Step 5h; portfolio-analysis.md §Starting parameters; portfolio-workflow.md §Step 6g; storage.md §Local Analysis Suite Storage
- Leading-metric inflection gate (metric-family-shaped, archetype-mapped; seasonally comparable robust-slope changes; per-family minimum history + noise floor; declared metric polarity) — trade-opportunities.md §The two non-negotiables, §Starting parameters
- Trade Opportunities research method (worldview-first thematic map traced economically; five lenses; two non-negotiables; discipline gates; all-cap breadth) — trade-opportunities.md §The research method
- Historical episode library (shipped, versioned winners / failures with dated metric series; pattern-lens retrieval grounding + gate shape-regression harness; grounding / development / locked-holdout partitions, never self-validating) — trade-opportunities.md §The lenses, §Starting parameters; trade-opportunities-workflow.md §Step 5d
- Event-impact / value-chain repricing lens (two-sided trigger — beneficiaries / feared-losers / latent; materiality gate app-enforced at card formation, route chosen speculatively; typed `technology_read`) — trade-opportunities.md §The event-impact / value-chain repricing lens; trade-opportunities-workflow.md §Step 3b; storage.md §Local Analysis Suite Storage
- Trade Opportunities workflow (end-to-end Type-tagged control flow; local-model-call contracts) — trade-opportunities-workflow.md
- Trade Opportunities endpoint surface (three-band cardinality — discovery / per-candidate / run-level — plus the three maintenance populations: the carried-matrix / watchlist sweep over one "swept names" union (live carries ∪ recheckable watchlist nodes, symbol-deduped after the union) w/ the conditional filing-cadence rider + the label-time outcome refresh covering picked and shadow episodes, symbol-deduped, benchmarks via the entry-stamped sector identity — no label-time classification call + the archive-price refresh (per distinct archived symbol after dedup, shared cache — the job-time owner of the archive's each-run promise); SearXNG-only discovery, no Tavily/GDELT) — data-sources.md §Trade Opportunities — endpoint surface, §FMP — current paid-plan tier audit; storage.md §Local Analysis Suite Storage (Stooq cache)
- Trade Opportunities signal inputs (FMP paid-tier feeds + FINRA / FRED / Stooq / SEC / web; engine-computed price-action confirmer; segment-acceleration quarterly series research-extracted via the typed `leading_metric_observation` (Step-5e append → Step-5f recompute) — FMP segments annual-only = context; macro-release calendar seed = FRED /release/dates, Step-2-owned) — trade-opportunities.md §Signal inputs; data-sources.md §Local analysis suite — shared sourcing, §Trade Opportunities — endpoint surface
- Opportunity schema (archetype, detection mode, leading metric, thesis, bear case, key falsifiers, hypothesis lineage, carry-forward status) — trade-opportunities.md §The opportunity
- Cross-lens contradiction / falsification check (folded into distillation + scoring, no extra model call; high-severity capped at the gate) — trade-opportunities.md §Reconciling the lenses; trade-opportunities-workflow.md §Step 5e, §Step 5g, §Step 5h
- Key-falsifier re-check classes (structured / filing / research — the canonical 3-value vocabulary owned by §Step 3c; filing = engine-refreshable model-free only, research-class = non-recheckable context (the segment series rides it); cheap-sweep evaluation at class cadence; persistence semantics — materiality margin + consecutive observations, first-breach note vs confirmed-breach warning; app-validated conditions) — trade-opportunities.md §The opportunity, §Reconciling the lenses; trade-opportunities-workflow.md §Step 5e, §Step 5h, §Step 7, §ATO: the audit flow; storage.md §Local Analysis Suite Storage
- Discovery diversity guardrails + research budget (stratified floors/ceilings; consolidation cap = compute budget not quality cap; no output cap on the matrix — app-validated completeness) — trade-opportunities-workflow.md §Step 4, §Step 6; trade-opportunities.md §The pipeline, §The opportunity space; configuration.md §Local Analysis Suite Configuration
- Trade Opportunities outcome learning / calibration (one engine primitive, two reads — matured-window labels incl. the all-outcome `resolution_mode` ordered first-match-wins tree w/ the input-completeness-guarded `no-dominant-mode` residual + the typed `resolution-unscorable` mode (any unevaluable still-relevant branch — live metric-unscorable included, post-departure the limiting case), inputs read from the picked episode's entry calibration snapshot (incl. the entry-stamped sector identity) + accumulated live events (entry-vintage target), constants in §Starting parameters + continuous since-flagged for display and the cap-only Step-5g re-score) — trade-opportunities.md §Outcome learning, §Storage and display; trade-opportunities-workflow.md §Step 5c, §Step 5g, §Step 7, §Step 9; storage.md §Local Analysis Suite Storage; data-sources.md §Trade Opportunities — endpoint surface
- Shadow outcome ledger / picked-vs-rejected calibration (typed decision episodes — gate-reject / abstention / deferral / dedup-substitute / retired — each w/ its entry-stamped sector identity, read per class on unique-issuer counts; full gate vector w/ distance-to-threshold; tradability-discounted false-negative flags; calibration-only, never re-promotes) — trade-opportunities.md §Outcome learning, §Starting parameters; trade-opportunities-workflow.md §Step 3c, §Step 5h, §Step 6, §Step 7, §Step 9; storage.md §Local Analysis Suite Storage; configuration.md §Local Analysis Suite Configuration
- Outcome measurement contract (next-close evaluation anchor, one common basis per comparison, typed terminal outcomes — acquisition at final price, bankruptcy to zero, ambiguous = unscorable-but-counted; terminal class only from recorded corporate-action facts, unresolved disappearance → terminal-unscorable; `leading-metric-unscorable` metric-label availability by re-check class + lifecycle; label-time price-coverage pending rule bounded by the shared grace → `price-coverage-unscorable`, both jobs; benchmarks from the entry-stamped sector identity w/ the typed `sector-unscorable` state) — trade-opportunities.md §Outcome learning; storage.md §Local Analysis Suite Storage
- Picked decision episodes / lifecycle id (retention-independent pick outcomes — outlive matrix presence, the archive's 100-cap, and run retention; opened w/ the entry calibration snapshot (incl. the entry-stamped sector identity) + accumulating dated live events, the resolution tree's input record; re-entry = new lifecycle while the old episode matures; lifecycle-scoped Step-5b recall; picked matured-archive cap; in the portability enumeration) — trade-opportunities.md §Outcome learning, §Starting parameters; storage.md §Local Analysis Suite Storage; trade-opportunities-workflow.md §Step 5b, §Step 7; configuration.md §Local Analysis Suite Configuration; data-portability.md §Build-order placement
- Matrix final assembly over the union (Step 6 = survivor-set assembly, w/ still-valid carries — cheap-swept + inconclusive deep-read — as dedup collapse-targets, cards carrying their cells — collapse eligibility app-validated on the typed equivalence predicate, invalid → list both, a debut-into-cheap-carry acceptance provisional until Step 7's final-cell re-check (mismatch reinstates the debut); direction app-enforced, a live carry never collapses away; Step 7 re-places carried names by refreshed tier, frozen-conviction insertion, completeness re-validated over the union) — trade-opportunities-workflow.md §Step 6, §Step 7; trade-opportunities.md §The opportunity; storage.md §Local Analysis Suite Storage
- Opportunity re-evaluation lifecycle (two jobs DTO/ATO on one page; only a deep re-evaluation archives — the cheap re-derivation warns, an ATO Deep-Audit archival taking the touched picked node to `departed` in the same pass; Quick Audit persists without embedding — Step 9's legs split; `continuity_weight`; card badges; Stooq price-only render floor) — trade-opportunities-workflow.md §Step 7, §ATO: the audit flow; trade-opportunities.md §The two jobs, §The opportunity, §Archived opportunities, §Starting parameters; storage.md §Local Analysis Suite Storage; interface.md; local-models.md §Serving runtime
- DTO deep-budget rotation slice (reserved maintenance-priority self-refresh — warning-bearing → catalyst-near → threshold-near → stalest, one-slot floor + max-age service level (best-effort under budget, surfaced stalest-first backlog), non-disableable; maintenance precedence over discovery diversity, quotas scope to the new-name remainder; the third deep-pass path) — trade-opportunities.md §The two jobs, §Archived opportunities; trade-opportunities-workflow.md §Step 4, §Step 7; configuration.md §Local Analysis Suite Configuration
- Archived opportunities (price-tracked tombstones, last 100; single exit `failed-reevaluation`; passive, anti-reflexive re-entry by ticker) — trade-opportunities.md §Archived opportunities; trade-opportunities-workflow.md §Step 7, §Step 9, §Step 10; storage.md §Local Analysis Suite Storage; interface.md §Main Layout
- Local analysis suite configuration (daemon, roster, SearXNG, Schwab; TO breadth/budget + discovery-memory + research context-management knobs) — configuration.md §Local Analysis Suite Configuration, §Research Context Management
- Investor profile default preset (frames the prescription, never which holdings or ideas qualify — nor the intrinsic verdict, profile-independence declared; user config deferred) — configuration.md §Investor Profile
- Local analysis suite storage + per-feature retention — storage.md §Local Analysis Suite Storage
- Local suite pages (Portfolio, Trade Opportunities) — interface.md §Main Layout
- Suite sorting & views (Portfolio holdings sort bar + sortable table heads, TO Matrix/List toggle; all display-only; design-system `.ana-sortbar` / `.ana-viewtoggle` extensions) — interface.md §Main Layout, §Persistent Warning Area; portfolio-analysis.md §Storage and display; portfolio-workflow.md §Step 9; trade-opportunities.md §Storage and display; trade-opportunities-workflow.md §Step 10; market-signal-design-system (SKILL.md, README.md §Analytical-register controls, colors_and_type.css, ui_kits Analytical.jsx, preview/analytical-controls.html)
- Deterministic financial-analysis engine (Rust computes metrics/sub-scores/tiers/targets; model interprets; app-enforced exact-equality on engine-owned values echoed in model schemas) — local-models.md §Context-memory discipline; portfolio-analysis.md
- Factor normalization basis (score = sector-adjusted bands + own-history; the factor-distribution store — one obs per issuer — is diagnostic-only, never a score input, graduating only via the representative-universe snapshot) — trade-opportunities-workflow.md §Step 5c; trade-opportunities.md §Starting parameters, §The lenses; storage.md §Local Analysis Suite Storage
- Implied-expectations read (scenario math inverted at the live price → the range of trajectories the price already assumes, never one solved number; anchors the priced-in / crowding judgment) — trade-opportunities.md §The pipeline; trade-opportunities-workflow.md §Step 5c, §Step 5g
- SEC EDGAR primary source (keyless filings + XBRL company facts; ticker→CIK resolver = a named Portfolio-slice prerequisite, unresolved → typed unknown) — data-sources.md §SEC EDGAR
- SEC EDGAR role for Trade Opportunities (authoritative cross-check; 13F the off-plan exception; ticker→CIK non-blocking) — data-sources.md §SEC EDGAR
- FMP paid-tier suite signals (revision flow, forensic scores, positioning, screener/peers discovery; one shared paid key, report logic unchanged) — data-sources.md §Local analysis suite — shared sourcing, §FMP — current paid-plan tier audit
- FINRA short interest (keyless biweekly file; level/trend/days-to-cover) — data-sources.md §FINRA
- Evidence floor (insufficient-evidence abstention; per-feature floor definitions, archetype-aware for TO; debut semantics — a carried live name's inconclusive re-read holds its last verdict) — portfolio-analysis.md §Evidence floor; trade-opportunities.md §Evidence floor; trade-opportunities-workflow.md §Step 5h, §Step 7
- Deterministic risk-tier assignment (TO rule canonical; Portfolio adopts for priced stocks w/ the missing-input rule + its own priced-equity-fund mapping, assigned/persisted at Step 6b, none on `role_risk_only`) — trade-opportunities.md §The opportunity space, §Starting parameters; portfolio-analysis.md §Starting parameters; portfolio-workflow.md §Step 6b
- Scenario-target function (v2 rate-anchored forward multiple — driver ladder fwd EPS → rev/sh w/ finite-positive rung eligibility, one diluted share basis, and the `no-admissible-driver` floor reason; × DGS10 spread-anchored P25/50/75 multiples under the explicit **inverse spread mapping** (bear = P75; the raw-percentile fallbacks map direct), monotonicity sort defensive-only; TR adds fwd dividends, one-month leg v1 mechanics; versioned via the calibration snapshot; as-built v1 drift pending engine update; TO archetype driver overrides; fund-form = the named open item) — portfolio-analysis.md §Starting parameters, §Evidence floor; trade-opportunities.md §Starting parameters, §Evidence floor; portfolio-workflow.md §Step 6b; trade-opportunities-workflow.md §Step 5c; data-sources.md (both `analyst-estimates` rows)
- Rate-anchor failure rule (full/deep runs hard-fail pre-per-item after shared retries; engine-only quick paths = cached print ≤ ~1-week max age, `unknown` beyond; both quick paths refresh DGS2 **and DGS10**, re-anchoring the stored v2 multiples closed-form — the ledger band stays frozen) — portfolio-analysis.md §Failure posture, §Starting parameters, §The quick check; trade-opportunities.md §Failure posture; trade-opportunities-workflow.md §ATO: the audit flow; data-sources.md §Portfolio Analysis — endpoint surface (FRED), §Trade Opportunities — endpoint surface (FRED)
- Ledger executability validation (rewritten quantitative conditions resolve-or-downgrade at 6g; machine-evaluable statement required at 6f Returns; quick-check promise anchored on validated conditions) — portfolio-workflow.md §Step 6f, §Step 6g; portfolio-analysis.md §The position thesis ledger, §The quick check
- Re-check class resolution contract (class tags = app-validated claims vs the endpoint surface; hypothesis-card metrics at 3c admission, opportunity metric + structured AND filing falsifiers at 5h; unresolvable → research + logged; Portfolio 6g rides it) — trade-opportunities-workflow.md §Step 3c, §Step 5h; portfolio-workflow.md §Step 6g
- Evidence-floor freshness basis (typed per-input fresh/stale/freshness-unscorable — session-current quotes, reporting period + ~45d grace, ~4-week research window; informs-never-gates scoped to the tier gradient) — trade-opportunities.md §Starting parameters, §Evidence floor; trade-opportunities-workflow.md §Step 5h; web-research.md §Source quality and evidence weighting; portfolio-analysis.md §Evidence floor
- TO research cache (cross-run, document-level — normalized-URL key, immutable vintage; searches always live; stamps/warning-clears/archival need current results; reuse split logged) — trade-opportunities.md §Failure posture, §Starting parameters; trade-opportunities-workflow.md §Step 3c, §Step 5, §ATO; storage.md §Local Analysis Suite Storage (web-research document cache)
- Watchlist cap eviction (lowest hypothesis score first, tie oldest refresh then ticker; `capacity-evicted` reason; one retirement-class shadow episode) — trade-opportunities-workflow.md §Step 3c; trade-opportunities.md §Starting parameters, §Discovery memory, §Outcome learning; configuration.md §Local Analysis Suite Configuration
- Horizon assignment (`expected_thesis_realization` sets the cell, `business_runway` feeds durability / conviction — two typed fields; inputs typed, never thesis prose — the catalyst claim `{description, date?, payoff_bearing}` + the validated `runway_evidence` w/ drafted duration mappings — the derived basis (`dated-catalyst` / `recognition` / `multi-year-compounding`) persisted beside them; payoff-bearing catalyst → Short, compounding-as-the-mechanism → Long open to any archetype, early-detection re-rate → Mid) — trade-opportunities.md §Starting parameters, §The opportunity; trade-opportunities-workflow.md §Step 5e, §Step 5g, §Step 5h; storage.md §Local Analysis Suite Storage
- Entry asymmetry threshold (DGS2-anchored + risk-tier-scaled required return — the short-end anchor for the ~12-month window while discounting keeps DGS10 — liquidity-discounted upside, no-inverted-shape test; re-run by every cheap re-derivation) — trade-opportunities.md §Starting parameters, §The opportunity; trade-opportunities-workflow.md §Step 2
- Per-item checkpoint/resume + research caching, both jobs (resume = its own entry path reopening the run's pinned snapshot/context/versions; DTO additionally pins the discovery outputs + candidate slate; shared ~48h window) — portfolio-analysis.md §Failure posture; portfolio-workflow.md §Step 6; trade-opportunities.md §Failure posture; trade-opportunities-workflow.md §Step 5
- Suite data dispersal (SEC + Stooq offload FMP; quotes on FMP; load-relief on the paid key) — data-sources.md §Local analysis suite — shared sourcing
- Stooq benchmark / futures identities + adjustment convention (`^spx`; SPDR sector-ETF mapping from FMP sector labels; futures copper `hg.f`, gold `gc.f`, silver `si.f` — TO's canonical gold/silver price context; split-adjusted dividend-unadjusted; M5 live-verify) — data-sources.md §Stooq
- Research loop & context management (per-topic agenda; depth ≤2 / ≤3 passes; budget binds first; no in-loop re-distillation; append-only evidence ledger) — web-research.md §The research loop and context management
- Seed lineage (structured-feed seeds recorded as leads, never evidence-ledger claims; `surfaced_by` + validated `seeded_by`) — web-research.md §The research loop and context management; trade-opportunities-workflow.md §Step 3b; configuration.md §Research Context Management; storage.md §Local Analysis Suite Storage; trade-opportunities.md §The opportunity, §Signal inputs, §Discovery memory
- Hierarchical distillation (shared primitive; single pass default, map-reduce on overflow, orchestrator-chosen deterministically) — web-research.md §The research loop and context management; trade-opportunities-workflow.md §Step 5e; portfolio-workflow.md §Step 6d; configuration.md §Research Context Management
- Heavy-route sub-distillation (deterministic heavy classification post-research; cross-route merge stays the deterministic Step 4) — trade-opportunities-workflow.md §Step 3b, §Step 4; configuration.md §Research Context Management
- Research agenda (fundamentals + market narrative/sentiment + forward opportunity/thematic fit) — portfolio-analysis.md; trade-opportunities.md
- Options-activity signal (put/call + IV/skew from Schwab chains; activity proxy, not positioning truth; CBOE backdrop) — schwab-integration.md; data-sources.md §CBOE; portfolio-analysis.md
- Schwab connection required for both local jobs (hard execution-gate precondition) — schwab-integration.md §A connected Schwab account is required
