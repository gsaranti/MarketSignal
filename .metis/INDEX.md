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
- Financial Modeling Prep (primary financial-data source; report endpoint→path table — 12 free wired + 7 planned paid, all on `/stable`) — data-sources.md §Financial Modeling Prep
- FRED (+ 32-series ID table by baseline group; `/series/observations` + `/release/dates`) — data-sources.md §FRED
- BLS (+ 4-series ID table; `/timeseries/data/`) — data-sources.md §BLS
- CFTC (Commitments-of-Traders positioning; keyless Socrata datasets `gpe5-46if` / `72hh-3qpy`) — data-sources.md §CFTC
- Tavily (primary research/news ingestion; `/search` endpoint) — data-sources.md §Tavily
- GDELT (geopolitical/event monitoring; DOC 2.0 endpoint) — data-sources.md §GDELT
- LLM providers (OpenAI, Anthropic) — data-sources.md §LLM Providers
- Planned report enrichment (paid FMP — economic-calendar consensus+surprise layered on FRED schedule; historical sector/industry valuation percentile+band + performance cumulative-return trend; IPO/M&A froth; all engine-derived, persist-derived-not-raw, `#[serde(default)]`, out of the level-delta engine) — data-sources.md §Planned report enrichment; report-workflow.md §Step 3, §Step 16

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
- Portfolio Analysis job (per-holding pipeline → intrinsic verdict; whole-book construction → portfolio action; grades, targets, roll-up) — portfolio-analysis.md
- Holding verdict schema — **intrinsic verdict** (grade + sub-scores, conviction, horizon, targets, **standalone action lean** incl. the engine's **capital-efficiency / dead-money** read) vs **portfolio action** (fixed ladder + target weight/sizing via the action-sizing spine); what-changed split into intrinsic + action halves — portfolio-analysis.md §The holding verdict
- Intrinsic-verdict vs portfolio-action separation + whole-book construction (per-holding loop sets the intrinsic verdict + standalone lean; post-roll-up **Step 7a** deterministic aggregates → **7b** model reconciliation set the final action + sizing — resolves the Step-6→7 feedback path so an A-grade business can be a trim) — portfolio-analysis.md §Portfolio roll-up and construction; portfolio-workflow.md §Step 7
- Action-sizing spine (engine bounds the feasible action set from grade/conviction/upside-downside/**capital-efficiency-dead-money read**/risk-tier/existing-weight/concentration-headroom/overlap/**unrealized-P&L — harvestable-loss-or-taxable-gain by sign**/cash/tax; model proposes within, app constrains) — portfolio-analysis.md §The holding verdict; portfolio-workflow.md §Step 7a
- Capital-efficiency / dead-money exit + sunk-cost guard (engine read = base-case forward return vs a risk-free+premium hurdle, kept out of sub-scores, **provisional — recomputed at Step-6e target refinement** so a research-revived forward case isn't read as stale; flags a holding whose forward prospects don't clear the hurdle → standalone lean leans to **exit, some or all, on its own merits**; firmed at construction by two **generic** counterweights — **possible tax benefit** of realizing a loss + **redeployment optionality** of freed cash — weighed high-level, user acts on specifics, **no harvest/wash-sale/account-type modeling**, replacement-name selection stays Trade Opportunities' isolated job; guardrails — never moves the grade/sub-scores (cost-basis-agnostic), fires only once forward prospects independently judged poor) — portfolio-analysis.md §The holding verdict, §Portfolio roll-up and construction; portfolio-workflow.md §Step 6b, §Step 6e, §Step 7; configuration.md §Investor Profile
- Position thesis ledger (persisted per-holding standing thesis: thesis + key drivers + **bear/base/bull monitor** + typed quantitative/qualitative **falsifiers** + add/trim/sell **triggers** + target-weight range — the Portfolio analog of TO's opportunity graph; engine evaluates quantitative crossings each run, interpretation rewrites, continuity check validates; fund-flavored for funds; **current standing thesis rendered as each holding card's anchor — the single continuity-validated source of truth, not a separate summary**) — portfolio-analysis.md §The position thesis ledger, §Storage and display; portfolio-workflow.md §Step 6a, §Step 6f, §Step 6g; storage.md §Local Analysis Suite Storage; interface.md §Main Layout
- Portfolio technology-event impact (held-name form of TO's event-impact lens — a **conditional per-holding research topic** fired by the same materiality gate when a holding moved on a third-party technology event, reading the actual technology and **sizing real exposure** into a typed `technology_read`; recorded as a first-class **technology-event qualitative falsifier class** on the thesis ledger; separates a panic drop from a genuine impairment, and overstated-benefit euphoria on the upside) — portfolio-analysis.md §The position thesis ledger, §The per-holding pipeline; portfolio-workflow.md §Step 6c
- Holdings change tracking (deterministic prior-run-snapshot diff → per-position new/increased/decreased/unchanged delta into dossier; exited names surfaced in roll-up) — portfolio-analysis.md §Holdings change tracking
- Portfolio Analysis workflow (end-to-end Type-tagged control flow — gate → holdings → classify → diff → shared context → per-holding loop [intrinsic verdict] → roll-up & construction [7a aggregates → 7b final actions] → persist → render; local-model-call contracts) — portfolio-workflow.md
- Portfolio three-layer engine (grade core / conviction layer / positioning context; new signals enrich conviction/risk, never the letter grade) — portfolio-analysis.md §The per-holding pipeline; portfolio-workflow.md
- Portfolio per-holding/per-fund endpoint surface (FMP per-symbol subset — dividends/earnings/float/profile/segments + run-level market-wide M&A; ETF/fund group = sector/country-weighting exposure tilt, constituent look-through off-plan → SEC N-PORT optional/dropped; keyless fallback/cross-check rows surfaced explicitly — SEC EDGAR submissions/XBRL + optional coarse 13F, Stooq, FINRA, CBOE, SearXNG web loop incl. transcript commentary; run-level FRED risk-free + commodity + CFTC) — data-sources.md §Portfolio Analysis — endpoint surface
- Fund path (reduced compute: expense drag, **exposure tilt** from etf sector/country weightings, fund valuation; constituent concentration off-plan with `etf/holdings` → SEC N-PORT optional or omitted; future **issuer-holdings adapter** planned, fresher than N-PORT; fund-flavored thesis ledger) — portfolio-analysis.md §Asset eligibility
- Not-rated positions in roll-up (options/fixed-income excluded from grading, but a **material** position still contributes its risk/exposure to the whole-book aggregates; cash/buying-power feed profile + roll-up) — portfolio-analysis.md §Asset eligibility, §Portfolio roll-up and construction
- House-view freshness gate (latest report carries its date; older than 1 week → omitted as a gap, not fed as current) — portfolio-workflow.md §Step 5
- Post-research target refinement (typed research_forward_assumption: value/units/as-of/source/confidence/conflict; engine recomputes forward targets only, sub-scores fixed) — portfolio-analysis.md §The per-holding pipeline; portfolio-workflow.md §Step 6e
- What-changed audit (split: **intrinsic half** — grade/sub-score/conviction/target/horizon/scenario/falsifier-trigger moves, validated at Step 6g; **action half** — action/weight moves attributed to a moved intrinsic verdict or a moved portfolio context, validated at Step 7b; every external claim maps to a real input-delta/aggregate or downgrades to a flagged self-correction) — portfolio-analysis.md §The holding verdict; portfolio-workflow.md §Step 6g, §Step 7b
- Trade Opportunities job (3×3 risk×horizon matrix) — trade-opportunities.md
- Trade Opportunities — what it hunts (two modes: early detection + continuation; leading-metric anchor) — trade-opportunities.md §What the job hunts
- Opportunity archetype lens (secular-compounder/ai-infra/commodity-cyclical/disruptor/quality-compounder; selects signal weighting + valuation lens) — trade-opportunities.md §Archetype
- Research-driven candidate discovery (three feeders: **model-led hypothesis research** (the edge) + bottom-up structured feeders (screener *stratifies*, no fundamental field; event/positioning) + carried-forward watchlist; model generates candidates, app validates) — trade-opportunities.md §The pipeline; trade-opportunities-workflow.md §Step 3
- Model-led hypothesis research lane (Step 3b — route planner over policy/supply-chain/technical-bottleneck/procurement-capex/customer-capex/industry-history/failure-analogue/event-impact-repricing routes, each with its own source strategy; mandatory **graph-blind outside-view route** (anti-anchoring); **hypothesis cards** (world-change → mechanism → margin-capture value-chain → leading metric → expressions → bear case → falsifiers); **hypothesis score** (magnitude/durability/horizon/metric-observability/crowding/margin-capture-clarity) gating promotion vs watchlist threshold before any ticker; adversarial passes; keyless SearXNG only) — trade-opportunities-workflow.md §Step 3b
- Discovery memory / opportunity graph (persisted `theme→mechanism→node→metric→companies→evidence→falsifiers`; node status picked/watchlist/retired; **carried-forward watchlist** of worthy-but-unpicked names — app-enforced bar (named leading metric + mechanism + falsifier + hypothesis score), leading metric re-checked by **cost class** (structured/filing/research) each run (Step 3c), promoted when confirming, retired on falsifier/stale/carry-horizon; bounded by retention cap; **reverses earlier stateless re-discovery**) — trade-opportunities.md §Discovery memory; trade-opportunities-workflow.md §Step 3c
- FMP paid-plan tier audit (current plan: **all `*-bulk` off-plan**, `company-screener` has no fundamental field, transcripts / 13F-institutional+holder-level / etf-holdings+funds-disclosure / press-releases off-plan → fallbacks SEC EDGAR / `sec-filings-8k` / web loop / N-PORT; three buckets allowed-with-constraint / blocked→fallback / blocked→no-fallback) — data-sources.md §FMP — current paid-plan tier audit
- Narrative-vs-reality ratio (with an operating-reality-vs-price fallback for thinly-covered small/mid caps whose estimates are absent/stale — reported operating momentum (segment revenue / backlog / gross profit / unit economics / retention) vs the price/multiple move in place of revisions) + forensic risk gate + base-rate conjunction discipline — trade-opportunities.md §The pipeline, §The two non-negotiables; trade-opportunities-workflow.md §Step 5c
- Trade Opportunities research method (worldview-first — house-view regime backbone + forward thematic map (value-chain traced **economically** — margin capture / bargaining power / capacity constraint / pricing power, not mere exposure); five lenses — quant composite / value-creation / macro-thematic-fit / investor-judgment / pattern-case-study; two-track proven-vs-emerging reconciliation through one moat/management/price-asymmetry gate; two non-negotiables — inflecting leading-metric hard gate + valuation-vs-forward red-flag; discipline gates — forensic / reflexivity / base-rate conjunction / **cross-lens contradiction** (folded into distillation+scoring, no extra model call); factor-timing avoided; all-cap breadth, size-within-quality) — trade-opportunities.md §The research method
- Event-impact / value-chain repricing lens (TO discovery — a discrete technology/product/standard announcement as a **two-sided** trigger: beneficiaries / panic-vs-real **feared-losers** / **latent** un-moved names; **materiality gate** (announcement + group-repricing OR primary-doc OR adoption-signal OR clear exposure), dormant otherwise; sized typed **`technology_read`** (substitute·complement·mix-shift + exposed revenue/profit pool + timeline + switching costs + margin node) on the hypothesis card; **symmetric feared-loser adversarial pass**; upside euphoria folded in; still one signal among many under leading-metric + validation gates) — trade-opportunities.md §The event-impact / value-chain repricing lens; trade-opportunities-workflow.md §Step 3b; storage.md §Local Analysis Suite Storage
- Trade Opportunities workflow (end-to-end Type-tagged control flow — gate → shared context (incl. opportunity-graph load) → discovery (3a bottom-up structured feeders + 3b model-led hypothesis research + 3c carried-forward watchlist re-check) → consolidation → per-candidate validation loop (archetype → dossier → archetype-weighted engine → bounded research → distill → target refine → score/gate → deterministic risk-tier/horizon) → per-cell selection → continuity carry-forward → holdings cross-ref → persist/audit → render; local-model-call contracts) — trade-opportunities-workflow.md
- Trade Opportunities endpoint surface (three-band cardinality — discovery / per-candidate / run-level; FMP discovery layer = screener (stratify, coarse fields) + peers/taxonomy + news/event feeds (**`*-bulk` off-plan**); per-candidate per-symbol surface incl. financial-growth, SC 13D/13G activist, symbol-scoped `news/stock` (**transcripts, 13F-institutional+holder-level, `news/press-releases`, per-symbol M&A off-plan** → EDGAR/8-K/web); FRED commodity incl. IMF metal series IDs; CFTC; Schwab/SEC/Stooq/FINRA/CBOE/news&web; **keyless SearXNG only for discovery — no Tavily, no GDELT**) — data-sources.md §Trade Opportunities — endpoint surface, §FMP — current paid-plan tier audit
- Trade Opportunities signal inputs (FMP paid-tier working/discovery feed: fundamentals/segments/financial-growth, revision signal, financial-scores forensic gate, symbol-keyed positioning = SC 13D/13G activist + congressional (**13F + holder-level off-plan** → EDGAR/omit), screener-stratify + **per-candidate** composite (`*-bulk` off-plan); FMP structured news — paid-key — market-wide feeds for discovery + symbol-scoped Search Stock News (`news/stock`; **`news/press-releases` off-plan**) per-candidate, a structured surfacing layer the **keyless SearXNG** web loop deep-reads (no Tavily/GDELT for discovery); FINRA short interest; FRED/Stooq commodity prices; engine-computed price-action confirmer (relative strength / base breakout, confirmer not trigger); SEC EDGAR authoritative cross-check; web-research lane for ASPs/supply-discipline + economist leading indicators; ticker→CIK optional; report data-source logic unchanged) — trade-opportunities.md §Signal inputs; data-sources.md §Local analysis suite — shared sourcing
- Opportunity schema (archetype, detection mode, leading metric, thesis, catalyst, conviction, narrative-vs-reality, bear case, **key falsifiers** (monitorable conditions that would invalidate the thesis — from the contradiction check), **hypothesis lineage** (link to its discovery-memory node), carry-forward status) — trade-opportunities.md §The opportunity
- Cross-lens contradiction / falsification check (adjudication folded into the existing distillation + scoring stages, **no extra model call** — distillation emits a typed contradiction + severity + key falsifiers; scoring must resolve/discount; a high-severity contradiction (strong theme over weak business) capped deterministically at the gate) — trade-opportunities.md §Reconciling the lenses; trade-opportunities-workflow.md §Step 5e, §Step 5g, §Step 5h
- Discovery diversity guardrails + research budget (the Step-4 consolidation cap is a **compute budget, not a quality cap** — a generous, **user-configurable** ceiling on how many candidates get expensive validation per run; un-picked names not rejected, only **deferred** — a worthy deferral is **remembered** in the persisted opportunity graph as a **watchlist** node and re-checked each run (reverses earlier stateless re-discovery; bounded by retention cap + carry-horizon); slots filled not by a flat top-N (no universe composite exists pre-validation — **stratification IS the breadth mechanism**, equal-per-bucket budget default) but under floors/ceilings by market-cap band / feeder / archetype / sector-theme so the funnel can't collapse onto mega-cap momentum or one crowded theme. The final matrix has **no output cap** — every gate-clearer is listed, ranked by conviction, with **app-validated completeness** (every gated survivor present or collapsed-into-a-named-peer-with-reason — no silent model drop; Step 6 is a rank+dedup pass, not a model gatekeeper); the gates set the count, not a quota) — trade-opportunities-workflow.md §Step 4, §Step 6; trade-opportunities.md §The pipeline, §The opportunity space; configuration.md §Local Analysis Suite Configuration
- Trade Opportunities outcome learning / calibration (per-run **deterministic** outcome on prior picks — **two reads off one engine primitive**: **matured-window labels** (forward return vs sector/market at 1/3/6/12mo, max drawdown, leading-metric continuation, decision-tree failure mode, templated-not-vibes; tested against stored key falsifiers; populated only as each window elapses) **and a continuous since-flagged read** (running return vs sector/market + max drawdown + metric-continuation from the first-surfaced price, refreshed every run, **live from the idea's first *subsequent* run** — debut pick has no read yet — not month-one; stateless Stooq reconstruction off the carry-forward identity, no stored snapshots; full daily curve regardless of run cadence); **three readers** — horizons→**calibration**, continuous→**inline matrix display** beside each carried-forward idea, continuous→**Step-5g re-score of a carried-forward name as reflexivity-disciplined context** (gain unmatched by metric → multiple-unwind risk → caps conviction; drawdown with metric intact → improved asymmetry; gain matched by metric → neutral, not boosted — metric already scored at 5c, so a positive would double-count; **cap-only by design** — holds or lowers conviction, never raises it, never a momentum boost); embedded as durable learnings; **calibration of archetype weights/gates staged as forward work** — no auto-adjust until a meaningful sample accrues, early runs stay shadow/calibration; sector/market benchmark via Stooq) — trade-opportunities.md §Outcome learning, §Storage and display; trade-opportunities-workflow.md §Step 5c, §Step 5g, §Step 7, §Step 9; storage.md §Local Analysis Suite Storage; data-sources.md §Trade Opportunities — endpoint surface
- Local analysis suite configuration (daemon, roster, SearXNG, Schwab, **Trade Opportunities discovery breadth / candidate research budget + discovery-memory knobs (watchlist retention cap, carry horizon)** + **research context-management knobs (distillation overflow threshold, heavy-route K, per-side substantial threshold, sub-distillation cap)**; gates local jobs only) — configuration.md §Local Analysis Suite Configuration, §Research Context Management
- Investor profile default preset (suite-shared, user config deferred — long-term horizon / profit-max objective / medium-to-high risk / cash treated as always available / **no precise tax modeling — but the generic possible-tax-benefit of realizing a loss is a live qualitative exit factor**; frames the prescription, never which holdings or opportunities qualify) — configuration.md §Investor Profile
- Local analysis suite storage + per-feature retention — storage.md §Local Analysis Suite Storage
- Local suite pages (Portfolio, Trade Opportunities) — interface.md §Main Layout
- Deterministic financial-analysis engine (Rust computes metrics/sub-scores/risk tiers/targets; model interprets) — local-models.md §Context-memory discipline; portfolio-analysis.md
- SEC EDGAR primary source (keyless filings + XBRL company facts) — data-sources.md §SEC EDGAR
- SEC EDGAR role for Trade Opportunities (authoritative cross-check for grade/target numbers + 8-K filings; insider/congressional positioning FMP symbol-keyed, institutional 13F the off-plan exception → coarse/optional EDGAR; ticker→CIK a non-blocking enhancement) — data-sources.md §SEC EDGAR
- FMP paid-tier suite signals (revision flow via estimates/grades-historical/price-targets/upgrades-downgrades + surprises; financial-scores Altman+Piotroski forensic gate; insider/Senate-House/activist-13D-G positioning (**13F off-plan** → EDGAR/omit); screener/peers/taxonomy discovery (**`*-bulk` off-plan** → per-candidate composite); one shared FMP key upgraded to paid, report logic unchanged) — data-sources.md §Local analysis suite — shared sourcing, §FMP — current paid-plan tier audit
- FINRA short interest (keyless biweekly consolidated equity short-interest file; level/trend/days-to-cover; serves Trade Opportunities and Portfolio held-equity risk/squeeze context) — data-sources.md §FINRA
- Evidence floor (insufficient-evidence abstention; floor-bearing vs enriching input tiering; Portfolio equity floor = statements + price, fund analog = quote/NAV/info/disclosure/coverage; Trade Opportunities floor is archetype-aware = price + validated leading metric + source freshness, statements substitutable by hard operating/unit economics for emerging-economics archetypes) — portfolio-analysis.md §Evidence floor; trade-opportunities.md §Evidence floor
- Deterministic risk-tier assignment — trade-opportunities.md §The opportunity space
- Per-holding checkpoint/resume + research caching — portfolio-analysis.md §Failure posture
- Suite data dispersal (SEC EDGAR cross-check + Stooq deep-history offload FMP; quotes on FMP; Finnhub dropped; dispersal is load-relief on the paid key, not free-cap avoidance) — data-sources.md §Local analysis suite — shared sourcing
- Research loop & context management (per-topic agenda — each topic its own model call over a clean context; depth ≤2 / ≤3 passes per topic, branches not LLM turns; per-item fetch+wall-clock budget binds first; no in-loop re-distillation — full per-topic findings → distillation (single, or **hierarchical** tier-1-per-topic-tree → reduce when large); append-only evidence ledger) — web-research.md §The research loop and context management
- Hierarchical distillation (shared reusable primitive — *distill one complete research topic-tree → structured object*; per-item consolidation is a **single pass** by default, **map-reduce** (tier-1 per topic-tree → tier-2 reduce) when it would overflow the consolidation call's input budget; **orchestrator-chosen deterministically** by evidence-ledger size, never the model; invariants — tier-1 sees *complete* findings (never in-loop summaries, so the no-mid-loop-re-distillation rule holds), and cross-topic reasoning (TO's cross-lens contradiction check) rides the **reduce**; Portfolio's reduce is pure consolidation, no contradiction check) — web-research.md §The research loop and context management; trade-opportunities-workflow.md §Step 5e; portfolio-workflow.md §Step 6d; configuration.md §Research Context Management
- Heavy-route sub-distillation (TO discovery — a route classified **heavy deterministically** post-research: card-formation input-budget overflow, >**K** distinct hypotheses, or >1 **substantial sub-agenda** (per-side evidence-ledger threshold; event-impact beneficiary/feared-loser/latent sides auto-substantial when populated); tier-1 sub-distilled along its seam → route-level reduce still emits **many distinct cards**; **cross-route merge stays the deterministic Step 4**, never a model collapse; bounded by sub-distillation cap + wall-clock, logged to audit) — trade-opportunities-workflow.md §Step 3b, §Step 4; configuration.md §Research Context Management
- Research agenda (fundamentals + market narrative/sentiment + forward opportunity/thematic fit) — portfolio-analysis.md; trade-opportunities.md
- Options-activity signal (per-stock put/call vol+OI & IV/skew from Schwab chains, an activity proxy not positioning truth; CBOE venue-level backdrop) — schwab-integration.md; data-sources.md §CBOE; portfolio-analysis.md
- Schwab connection required for both local jobs (hard execution-gate precondition) — schwab-integration.md §A connected Schwab account is required
