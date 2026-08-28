# Trade Opportunities: logic flow

> This describes the designed job behavior.  
> Trade Opportunities is not yet built — every step below is designed and none is as-built, so the doc carries no built-vs-designed markers.

`Gate → Load context → Discover names → Narrow list → Deep-check each name → Build matrix → Maintain old ideas → Mark owned → Save → Display`

## Important terms

- **DTO — Discover Trade Opportunities**
  - Finds new ideas.
  - Maintains existing ideas.
  - Runs the full workflow.

- **ATO — Audit Trade Opportunities**
  - Rechecks opportunities you select.
  - Has Quick and Deep modes.
  - Does not discover new names.
  - Shares one `trade_opportunities` job identity with DTO — each run record carries its mode (`discover` / `audit-quick` / `audit-deep`), so history, retention, and the page footer's last-run stamp read one mode-labeled pool (ruled 2026-08-19).

- **Candidate**
  - A company being investigated.
  - Not yet approved as an opportunity.

- **Opportunity**
  - A candidate that passed every required check and sits in the matrix.

- **Debut**
  - A candidate with no live opportunity record — new to the matrix this run.
  - Only a debut can be held out at the entry gate or the evidence floor; a carry instead takes a warning or holds its verdict — it leaves only when a deep pass judges it invalidated (model-judged, or app-forced by a validated hard trigger).

- **Carried-forward (a carry)**
  - An existing live opportunity loaded from the prior run.
  - Gets either a deep pass (a rotation pick, a budget-winning re-surfacer, or a Deep Audit) or the cheap re-derivation.

- **Hypothesis**
  - A testable investment idea.
  - Example: “AI data-center growth will benefit cooling suppliers.”

- **Hypothesis card**
  - The structured form a research route produces: world change → mechanism → who captures the margin → leading metric → candidate names → bear case → falsifiers → sources.
  - Scored before any ticker is named.

- **Hypothesis score**
  - Equal-weighted mean of six 0–1 components: magnitude, durability, horizon fit, leading-metric observability, 1 − crowding, margin-capture clarity.
  - Promote at ≥ 0.60; watchlist at ≥ 0.40.

- **Route**
  - A research direction the discovery lane spends budget on.
  - Examples: policy / regulatory, supply chain, technical bottleneck, procurement / capex, customer capex, industry history, failure analogue, event-impact repricing.
  - Each route carries its own source strategy (which source types it targets).

- **Outside-view route**
  - The one mandatory route every run, run graph-blind (it never sees prior hypotheses).
  - Exists so the discovery memory cannot anchor the job to its own past.

- **Coverage debt**
  - A route class, broad industry, or active theme not successfully researched within the coverage window (~4 weeks, calendar time).
  - Causes the app to reserve the next route slot after the outside-view route.

- **Coverage ledger**
  - Per route class and per coverage subject: first seen, last attempted, last successfully completed, computed debt.
  - A completed route pays debt even when it finds nothing; a failed route does not.

- **Seed lineage**
  - Which structured-feed headlines surfaced or oriented a hypothesis.
  - A lead, never evidence: a seed's claim counts only once its source is deep-read.

- **Technology read (`technology_read`)**
  - The sized read an event-impact route attaches to each affected name: substitute / complement / mix-shift, exposed revenue or profit pool, deployment timeline, switching costs, the margin-capturing node.
  - Present only when the name came through that lens.

- **Opportunity graph**
  - The job’s discovery memory.
  - Stores hypotheses with their value-chain traces, watchlist names, and their relationships.

- **Watchlist node**
  - A worthy-but-unpicked name remembered in the graph: hypothesis lineage, a named leading metric (with its re-check class), falsifiers, why deferred, the latest validation gap.
  - Re-checked at its metric’s cadence every later run.

- **Research-watchlist refresh**
  - A small current-search check on one stored `research`-class metric (drafted: one node per DTO run).
  - Does not decide whether the company is investable.

- **Limited-history evidence**
  - Older evidence for a new listing, spin-off, or changed business perimeter.
  - Must map cleanly to the current company; app-declared eligibility, never a looser floor.

- **Archetype**
  - The type of business or opportunity — secular compounder, AI / secular-cyclical infra, commodity cyclical, category disruptor, quality compounder.
  - Determines which financial signals matter most and which valuation lens applies.

- **Leading metric**
  - A countable, dated, third-party-verifiable number expected to move before profits or the stock price.
  - Examples: backlog, bookings, subscriber additions, estimate revisions, segment revenue.
  - Its absence makes a candidate a story stock — it abstains `insufficient-evidence` on the evidence floor, whose validated-leading-metric leg this is.

- **Re-check class**
  - How a leading metric, falsifier, or milestone condition can be refreshed: `structured` (an engine series, every run), `filing` (a statement field, on filing cadence, model-free), `research` (only a web pass can refresh it).
  - An app-validated claim: a class that does not resolve is downgraded to `research`, never dropped.

- **Catalyst**
  - A typed claim `{ description, date (optional), payoff_bearing }` for why the market may notice the opportunity now.
  - Example: earnings, product launch, contract, regulatory decision.

- **Thesis milestone**
  - One step in the expected path from today to the thesis paying off, in a typed ordered plan with one named payoff milestone.
  - May include an evidence-backed expected window; an unsupported date becomes undated, never invented.

- **Falsifier**
  - A measurable condition that would show the thesis is wrong, typed by re-check class.
  - Example: backlog declines for two reporting periods.

- **Condition ID**
  - App-controlled identity for a machine-checkable falsifier or milestone condition.
  - Preserves evaluation history when the machine core is unchanged; a changed core starts fresh with a `supersedes` link.

- **Engine arm (baseline)**
  - The deterministic side of every judgment field: archetype-weighted sub-scores, the v2 scenario targets (structured-only and research-informed), the implied-expectations range, a mechanical conviction stand-in, the rule-derived risk tier, the milestone-derived horizon, and the proxy-mapped `business_runway`.
  - Always obeys its own caps and rules; nothing the model returns alters it.

- **Model arm (model view)**
  - The reasoner’s own read of the same fields — sub-scores, bear / base / bull bands, implied-expectations read, conviction, risk tier, horizon, and business-runway read — authored with the engine’s values in view as evidence (the engine tier and horizon excepted: both are 5h assignments, so placement is authored unanchored).
  - Structurally validated only; never checked against the engine’s numbers; its target bands are scored head-to-head against the engine baseline by the outcome scoreboard, the other authored reads recorded unscored.
  - Its tier × horizon place the card (the placement ruling, 2026-08-19); the engine’s derived pair renders beside them.

- **Conviction**
  - Confidence in the thesis — High, Medium, or Low.
  - Carried in both arms: the model’s own value stands as authored; the engine stand-in carries every cap.
  - Separate from risk.

- **Engine conviction stand-in**
  - The engine arm’s mechanical conviction: a count of disclosed degraded inputs and the entry gate’s distance-to-threshold, the lower rung winning.
  - Exists so the ceilings have a bearer that is not the model’s value.

- **Admission provenance (`admitted_by`)**
  - Which arm’s entry gate let a name in: `engine-and-model`, `engine-only`, or `model-only`.
  - A name clearing either arm’s gate is admitted; both arms’ gate vectors persist either way.

- **Risk tier**
  - How risky the company appears — High, Medium, or Low; carried in both arms.
  - The model’s own tier sets the matrix row (the placement ruling, 2026-08-19); the engine’s rule-derived tier sets the required return at the gate and renders beside the placement as the baseline.

- **Horizon**
  - When the thesis is expected to pay — Short, Mid, or Long; carried in both arms.
  - The model’s own horizon sets the matrix column (the placement ruling, 2026-08-19); the engine derives its own from the validated payoff milestone or catalyst, and that derivation’s basis still sets the gate’s H.

- **Gate**
  - A mandatory rule.
  - Failure prevents a debut from entering the matrix; a carried name failing the entry gate instead takes a warning (only a deep pass's invalidated verdict — model-judged, or app-forced by a hard trigger — removes a carry).

- **Evidence floor**
  - The minimum evidence a candidate needs before any judgment is written: price + history, a validated leading metric, current sources, and statements (or an archetype-defined operating substitute).
  - Below it the candidate abstains as `insufficient-evidence`; binds both arms absolutely.

- **Entry gate (entry asymmetry)**
  - The required forward return a name must clear: `DGS2` + 8 / 16 / 30 points by risk tier, plus the shape, liquidity, and (emerging track) double-over-horizon legs.
  - Run once per arm; re-run on every cheap pass.

- **Cheap re-derivation**
  - Fast, model-free refresh of the engine-computed fields and both arms’ gates.
  - Can raise a warning; cannot re-rate, re-place, or remove an opportunity.

- **Deep re-evaluation**
  - The full per-candidate loop (Steps 5a–5h) on an existing opportunity.
  - The only process allowed to rewrite the model-authored fields or archive.

- **Attention warning**
  - Amber *Consider Deep Audit* flag the cheap re-derivation raises on a tripwire, exhausted upside, or a re-surfacing.
  - Never changes the verdict; cleared by the next floor-clearing deep pass.

- **Since-flagged read**
  - Running return since the name became an opportunity (absolute, vs sector, vs market), its maximum drawdown, and whether the leading metric continued.
  - Reconstructed from daily bars each run; cap-only in scoring — it can hold or lower conviction, never raise it.

- **Episode**
  - A dated record of a decision, used later to measure whether the decision worked.
  - A picked episode records an accepted opportunity; a shadow episode records a name turned away.

- **Lifecycle ID**
  - App-assigned identity for one stretch of a ticker being live — from entry to departure.
  - A re-entry from the archive is a new lifecycle; nothing from the old one carries.

- **Outcome label**
  - Engine-calculated result at 1, 3, 6, and 12 months: return (absolute, vs sector, vs market), maximum drawdown, whether the leading metric continued, and the resolution mode.

- **Resolution mode**
  - A deterministic first-match label for how a matured window resolved: terminal event → forensic materialization → leading-metric rollover → multiple unwind → market beta → thesis played out → no dominant mode (or the typed unscorable states).

- **Shadow ledger**
  - Stores every name the funnel affirmatively turned away, one typed episode per turn-away: gate reject, abstention, deferral, dedup substitute, retired hypothesis.
  - Used to detect missed winners; calibration-only, never a feeder.

- **Archive**
  - The price-tracked record of departed picks (most recent 100).
  - A name leaves the matrix for it only on a failed deep re-evaluation; re-entry is a fresh start.

- **Continuity weight**
  - How hard a deep pass leans on the prior record, banded by the age of its last deep research: ≤ ~1 week continued research, ~1–4 weeks blended, > ~4 weeks fresh look.
  - Frames interpretation only; every engine number is recomputed.

- **Rotation slice**
  - The reserved share of the deep-research budget (default ~20%, never below one slot) spent first on live opportunities in maintenance-priority order.
  - Backstopped by a max-age service level.

- **Deep-research set**
  - The run-scoped list of tickers deep-researched this run.
  - A ticker in it is never also cheap-swept: at most one deep pass per ticker per run.

- **Research cache**
  - The cross-run web-document cache: fetched, readability-extracted pages keyed by normalized URL, under ~4 weeks old, carrying their original retrieval timestamp.
  - Document-level only — a cached page can be reused, a judgment never is; searches always run live.

- **House view**
  - Current Market Signal thesis and major market themes.
  - Omitted (and recorded as a gap) when older than one week.

- **Investor profile**
  - Risk tolerance, horizon, objective, tax posture, cash posture — a fixed default preset for now.
  - Shapes entry framing and conviction emphasis; never which opportunities qualify.

- **Reasoning model**
  - Local 122B model, thinking mode for research and scoring, non-thinking for distillation.
  - Fills every reasoning role by switching mode.

- **Embedding model**
  - Local 4B model.
  - Finds relevant prior analysis; performs no investment reasoning.

## Main data sources

- **FMP — discovery layer (universe-wide, a bounded number of calls per run)**
  - `company-screener` — universe definition, tradability gate, and market-cap-band / sector stratification (coarse fields only — no valuation or growth filter; `*-bulk` pre-scoring is off-plan).
  - `insider-trading/latest` — market-wide newest Form 4s for insider cluster buys.
  - `biggest-gainers`, `biggest-losers`, `most-actives` — movers.
  - `earnings-calendar` — upcoming catalysts, and read backward as the post-earnings surprise screen.
  - `mergers-acquisitions-latest`, `sec-filings-8k`, `ipos-calendar` — fresh catalysts.
  - `available-sectors`, `industry-classification-search`, `all-industry-classification`, `stock-peers` — map a theme onto its exposed names; expand a name to its peers.
  - `news/general-latest`, `news/stock-latest`, `fmp-articles` — ticker-tagged, dated headlines that seed the discovery routes (leads the web tool deep-reads; never evidence).

- **FMP — per-candidate surface (the budget driver; fires only for the narrowed set)**
  - `profile` — sector, industry, beta, description.
  - `income-statement` (+ TTM), `balance-sheet-statement`, `cash-flow-statement` — the core statements.
  - `key-metrics`, `ratios` (+ TTM), `financial-scores` (Altman Z, Piotroski), `owner-earnings`, `enterprise-values`, `discounted-cash-flow`, `financial-growth` (multi-year per-share CAGRs), `dividends` (trailing distributions — the targets' forward-dividend leg).
  - `revenue-product-segmentation`, `revenue-geographic-segmentation` — annual only; the quarterly segment series is research-extracted.
  - `analyst-estimates` (snapshotted run to run for revision velocity), `grades`, `grades-historical`, `grades-consensus`, `price-target-consensus`, `price-target-summary`, `ratings-snapshot`, `ratings-historical`, `earnings` (next date + surprise history).
  - `news/stock` — symbol-scoped headlines seeding the candidate’s narrative read.
  - `insider-trading/search`, `insider-trading/statistics`, `acquisition-of-beneficial-ownership` (13D / 13G), `senate-trades`, `house-trades`, `shares-float`; optionally `historical-employee-count`, `key-executives`.
  - `quote` — the live price the engine prices targets and runs the gate against.
  - `historical-price-eod/light` (dated) — deep daily price history, through the shared price-bar cache.

- **FMP — run-level series**
  - Commodity series `HGUSD` (copper), `GCUSD` (gold), `SIUSD` (silver) — daily price turns for the cyclical sleeve.
  - Benchmark series `^GSPC` and the SPDR sector ETFs — the outcome labels’ and since-flagged read’s market / sector legs.

- **FRED**
  - `DGS2` and `DGS10` Treasury yields; the anchor-window `DGS10` history for the valuation multiples.
  - `DCOILWTICO` (WTI), `DHHNGSP` (Henry Hub) daily; `PCOPPUSDM`, `PALUMUSDM`, `PNICKUSDM`, `PIORECRUSDM`, `PURANUSDM` monthly IMF metals.
  - `/release/dates` — the macro-release calendar (names + dates).

- **SEC EDGAR**
  - Submissions — 10-K / 10-Q / 8-K, item-classified (Item 4.01 auditor change, Item 4.02 restatement); S-1 / Form 10 history for an eligible new listing or separation.
  - XBRL company facts — the authoritative statement cross-check.
  - 13F — run-level, optional, coarse; held out of the grade.

- **FINRA**
  - The consolidated short-interest file, fetched once per run: level, trend, days-to-cover per name.

- **CFTC**
  - `gpe5-46if` (Traders in Financial Futures): E-mini S&P 500, Nasdaq-100, 10Y / 2Y Treasuries, USD index.
  - `72hh-3qpy` (Disaggregated): gold, WTI crude, copper.

- **CBOE**
  - Daily put/call ratios (total, equity, index) — a venue-level sentiment backdrop, never a per-name signal.

- **Charles Schwab**
  - Per-candidate option chains (volume, open interest, implied volatility) → the options-activity signal.
  - Current holdings, pulled fresh at Step 8 for the owned / not-owned label only.

- **SearXNG**
  - Keyless local web search for discovery and per-candidate research.

- **Tavily**
  - Backup search for per-candidate research only when SearXNG cannot serve.
  - Never used for discovery.

- **Local storage**
  - The prior run’s matrix, the opportunity graph, the coverage ledger, the archive, the shadow ledger, and the picked-episode store (the six persisted structures).
  - The shared price-bar cache, the web-document research cache, the factor-distribution store, and the web-research source state.
  - The Market Signal house view and recent report summaries, the investor profile, and the Trade Opportunities vector-memory partition.

---

## The research loop (shared by Steps 3b, 3c, 5d, and Deep Audit)

Four stages reach the open web, and all of them run the **same bounded loop** — the one Portfolio Analysis’s Step 6c runs: Step 3b’s discovery routes, Step 3c’s targeted watchlist refresh, Step 5d’s per-candidate research, and ATO’s Deep Audit (Step 5d on the user’s selection). The mechanics are written once here; each step states only what is its own — its agenda, its budget scope, its seeds, its search backend, and what comes out — and points back.

### What differs per stage

- **Step 3b — discovery** — the unit of work is a **route**, worked as one or more topics (`route ⊃ topic ⊃ pass ⊃ fetch`). One **per-run discovery** fetch + wall-clock ceiling is shared across every route, spent in route-priority order. Search is **keyless SearXNG only** — no Tavily; a down SearXNG means fewer candidates, never a keyed fallback. Seeds are the FMP `news/general-latest`, `news/stock-latest`, and `fmp-articles` feeds plus the macro-release calendar. The route’s findings are consolidated by its **card-formation call** into hypothesis cards (Step 3b), not by Step 5e.
- **Step 3c — the refresh lane** — one selected watchlist node, one isolated bounded conversation, spent from the same discovery ceiling, SearXNG only. It is given only the node’s stored hypothesis, named metric, falsifiers, relevant milestone, latest gap, and this run’s matching structured-event seeds, and returns one typed `watchlist_research_refresh` object.
- **Step 5d — per-candidate (and Deep Audit)** — the unit is the candidate’s agenda (the topic list at Step 5d). A **per-candidate** fetch + wall-clock budget, spent in topic-priority order (leading metric and bear case first). Search is **SearXNG first, Tavily as fallback**. Seeds are the candidate’s `news/stock` headlines. Every worked topic’s full findings flow to Step 5e distillation.
- **No cross-run findings seed.** The cross-run research cache is **document-level only** (below): no step receives a prior run’s distilled object as a seed — Portfolio’s seed-and-merge layer does not exist in this job; every loop starts from its framing inputs and works the open web.

### Build the agenda

The orchestrator assembles the topic list from the stage’s documented list; the reasoner works it one topic at a time. At Step 5d that list is fixed — the candidate’s topics plus its deterministically triggered conditional one (limited-history reconstruction) — and the orchestrator assembles it deterministically. At Step 3b there is no documented topic list, so the route agenda *and* each route’s topic list are the **planning call’s proposal, app-validated** (ruled 2026-08-19) — with the outside-view and coverage-rotation routes app-inserted — the one agenda in the suite the reasoner proposes; inside the loop the research model still never authors a topic.

### Work each topic — the loop

The orchestrator works the agenda **one topic at a time**. Each topic is its own isolated conversation over a clean context, and the orchestrator — never the model — owns every search and fetch, stopping at the stage’s budget.

- **Two nested levels**
  - **Topic** — one isolated conversation per agenda topic; topics never share a context.
  - **Pass** — each topic's conversation is a bounded multi-turn tool loop: one root pass plus up to two follow-up passes, so three passes per topic at most. The cap counts passes (branches), not model calls — a single pass is itself many turns, each turn one model call: the tool-requesting turns ask for a search or fetch the orchestrator runs, and the pass's terminal turn emits its findings.

- **What each topic conversation is given (its inputs)**
  - The stage's framing facts — identical for every topic of the item: at Step 5d the candidate's dossier facts, archetype, and computed leading-metric reads; at Step 3b the house view, the carried-forward opportunity graph (withheld from the outside-view route), and the route's source strategy.
  - That topic's own questions — different per topic.
  - The stage's seeds, as leads — the structured-feed headlines orient the topic (and carry their stable seed IDs), never as evidence: a seed's claim counts only once the model deep-reads its underlying source.
  - No other topic's findings — a later topic gets nothing from an earlier one. The topics meet only downstream, at the consolidating call (Step 5e distillation, or Step 3b's card formation).

- **What is retrieved during a pass (the data)**
  - Live web-page text — the pages the model deep-reads, fetched by a plain HTTP GET with a browser-like header set and readability-extracted in Rust to the article body (navigation, ads, and boilerplate stripped). This is what "current web sources" means.
  - Cached pages — a URL under about four weeks old comes from the **document cache** instead of the network, keyed by normalized URL and carrying its original retrieval timestamp (the vintage is never rewritten on reuse); new URLs are fetched live, and **searches always run live** — the cache satisfies only the re-fetch of an unchanged URL a current search re-surfaces. Each pass records its reused-vs-freshly-fetched split in the run audit.
  - A thin result (a paywall or a JavaScript-rendered page) escalates to a **rendered fetch** in the app's embedded webview — only the pages the extraction telemetry flags, never blanket — and an optional Connected Source (the user's own subscription session, from the Keychain) may carry the fetch past a paywall; both hold the same safety posture as the plain GET.
  - Search is a backend, not a separate data source: SearXNG-first (Tavily fallback at Step 5d only; SearXNG-only at Steps 3b and 3c).

- **Who owns the context, and what persists**
  - The orchestrator owns the prompt: on every turn it appends the tool results and the model's non-thinking output (a tool request, or the pass's findings), threading the growing context forward — the model only requests tools, it never touches the network. Prior `<think>` blocks are stripped from history, never accumulated across turns (`docs/local-model-operations.md` §Strip thinking from history).
  - Carried across the topic's passes: the append-only **evidence ledger** (each extracted claim + its source URL + retrieval timestamp; a claim deep-read from a seed's URL additionally carries a `surfaced_by` back-pointer to that seed) and the accumulated per-pass findings, which the orchestrator assembles for the consolidating call. The framing inputs anchor the conversation from its start.
  - Raw fetched page text is the bulky working material: it may roll off the context as a pass proceeds, and the durable record of what a page yielded is its claims in the ledger, not the page text itself.

- **Fitting the fixed context window**
  - `num_ctx` is fixed per model and never raised to make room — raising it reloads the runner and starves memory (`docs/local-model-operations.md` §The num_ctx trap); context pressure is answered by dropping content, not by growing the window.
  - When the prompt approaches the ceiling, older raw page text rolls off the working context — but only after its claims are banked in the ledger, so nothing is silently dropped. The contract fixes only what is eligible to roll off (raw page text, never the ledger), not an eviction order or a trigger threshold.
  - It never relies on the model server's own truncation, which silently front-drops the prompt's head and leaves the model to hallucinate over the gap.

- **What each model call returns**
  - Inside a pass, a turn either requests `web_search` / `web_fetch` calls — the orchestrator executes them and returns the results for the next turn — or, once the topic is answered or the budget is spent, emits that pass's findings.
  - The model authors each pass's findings write-up; the orchestrator only accumulates them — there is **no topic-close model synthesis**, so the first model consolidation of the findings is the stage's downstream call (Step 5e distillation, or Step 3b's card formation).
  - Each ledger entry is a hybrid: the model supplies the claim, the orchestrator stamps its provenance (the source URL / timestamp, and any seed back-pointer).

- **Follow-up passes**
  - A follow-up is the model's **proposal** — a structured field the orchestrator reads and decides whether to spend; the model never recurses on its own.
  - It is granted only while depth remains (≤2 follow-ups) **and** the stage's budget has room; on exhaustion it is simply not spent (fail-soft, no follow-up).

- **Seeds are leads, and their lineage is validated**
  - A seed is never written into the evidence ledger as a claim — a second-hand snippet is not verified evidence.
  - Lineage is kept in a small typed lane beside the ledger, two ways: deterministically, via the `surfaced_by` stamp whenever a seed's URL is deep-read; and model-attributed, via a bounded `seeded_by` list (config-capped) naming the seeds the reasoner judges oriented it even if it never fetched them.
  - `seeded_by` is validated, not trusted: each entry must reference one of the stable seed IDs the orchestrator fed this loop; an unknown reference is dropped and logged, so lineage can't be fabricated.
  - Distillation reads the lineage as provenance, never as scored evidence; the gap between a seed's headline and what its deep-read found is itself a narrative-vs-reality tell.

- **Source quality informs, it never gates**
  - Every fetched document carries app-computed annotations — `sourceTier` (0–5, from the source registry), `extractionQuality` (0–1, how much article body was recovered), `recencyScore` (against the source's freshness SLA), `primarySourceBonus`, a paywall / JS-stub flag — beside the model-derived ones (`claimSpecificity`, `contradictionFlag`, which claim IDs a document supports).
  - A low tier lowers conviction; it never removes a claim or candidate. The one exclusion is the explicit `deny` list (SEO mills, AI quote pages, PR spam) — keeping non-sources out, not gating on quality.
  - Lane policy: **discovery** takes a soft preference (a low-tier lead is still pursued, weighted down); **per-candidate validation** weights stricter (a claim resting only on tier-4/5 sources is flagged low-confidence, still surfaced). Trade Opportunities leans to specialist and value-chain sources.
  - Syndication is collapsed: five outlets reprinting one wire are one independent source — independence is counted by origin, not URL count.
  - Claim **freshness** is a different question: whether a floor-bearing input is current enough is decided by the evidence floor's typed freshness basis (Step 5h), never by the tier weights.

- **Safety**
  - Fetches are SSRF-guarded — `http`/`https` only, public hosts only (the app's own Ollama and SearXNG run on loopback), redirects capped and re-validated, responses bounded by size and content type.
  - Fetched text is data, never instructions — inserted as quoted evidence, so an injected page can't redirect the analysis.

- **Stops at the budget**
  - The stage's fetch + wall-clock budget binds first, polled at each request boundary and spent in priority order — topic priority at Step 5d (leading metric and bear case first), route priority at Step 3b. When it drains, the lowest-priority remaining topics or routes are skipped fail-soft, each recorded as a degraded-input gap (lower conviction), never failing the run.
  - The per-topic depth cap (≤2 follow-ups, ≤3 passes) works alongside it, guarding against rabbit-holing one topic.
  - The fetch-count, topic, and depth caps are pinned defaults; the wall-clock cap is calibrated against measured local throughput on first runs.

- **Disconfirming-fetch pass (placed at Step 5d)**
  - Once per candidate, after its topics, the loop spends one bounded pass searching specifically for what would disprove the thesis — a disconfirming *fetch*, not just a disconfirming prompt.
  - It is spent from the candidate's fetch / wall-clock budget, is not counted against any topic's three-pass depth, and fail-softs to a recorded gap when the budget is already exhausted. Step 3b's adversarial passes (*why already priced? · why is the obvious beneficiary wrong? · who captures the margin instead? · is the impairment real or panic?*) are prompt-side discipline inside card formation, not fetch passes.

- **Failure**
  - Web failure reduces evidence; it may lower conviction; it does not fail the run.
  - A hard model failure inside a required per-candidate path fails the run; the per-candidate checkpoint (Step 5h) lets a resume pick up the unfinished candidates.

## Distillation (shared by Steps 3b, 5e, and Deep Audit)

Consolidation is one reusable primitive — *distill one complete research topic-tree (a topic plus its ≤3 passes) into a compact, structured object* — applied wherever research must be condensed before a reasoning call reads it: Step 5e (the per-candidate distilled findings object), Step 3b's heavy routes (tier-1 sub-distillation before card formation), and a Deep Audit (Step 5e again).

- **Model**
  - The 122B reasoner in non-thinking mode (the optional 35B fast tier if resident).
  - Consolidates evidence; performs no new searches; calculates no financial numbers.

- **Single or hierarchical — chosen deterministically**
  - The orchestrator, never the model, sizes the stage's **full input** — every worked topic's findings, the accumulated evidence ledger, and the job-specific inputs that join them (Step 5e: the engine's computed reads) — against the call's input budget; above the configured overflow threshold it routes hierarchical. Growth *across* topics trips the hierarchical path rather than overflowing one call.
  - **Single pass:** one call over every topic's findings — the only call that sees the whole input.
  - **Hierarchical (large input):** a **tier-1** distillation per topic-tree (each call seeing one tree's complete findings + ledger entries, no cross-tree context), then one **tier-2 reduce** over the tier-1 objects into the one object the next stage reads. Tier-1 outputs are structured and field-preserving — per-lens claims with sources and confidence plus any internal-tension flag — so nothing the reduce depends on is lost.
  - Any reasoning that must span topics lives at the consolidating pass that first sees them all — the single call, or the tier-2 reduce (never tier-1): for this job, the cross-lens contradiction check (Step 5e).
  - Either path preserves each claim's citations end to end.

- **If one topic's own input overflows a call**
  - Trigger — the topic's complete input *summed* (all its passes' findings plus their ledger entries) would exceed one call.
  - Map — one distillation call per pass, condensing that pass's findings and ledger entries into a compact per-pass object.
  - Reduce — one more call combines the per-pass objects into the topic's tier-1 object, which joins the outer reduce like any other. So an overflowing topic costs one map call per pass plus one reduce — two to four calls — against the single call a normal topic uses.
  - The cap, not a further overflow check, is what drops passes — the per-item sub-distillation cap (Bounds and audit, below) is a budget of pass-level map calls shared across the item's overflowing topics; a topic whose passes exceed what remains fail-softs its lowest-priority whole passes to a recorded gap, each taking its findings and ledger entries with it, and the tree-level reduce is never sized.

- **Bounds and audit**
  - A per-item sub-distillation cap (config), spent from the stage's existing budget (wall-clock binds first); the chosen shape and tier count are logged to the run audit, so the fan-out is never silent.

- **Where it is used, and what comes out**
  - Step 5e — the candidate's single schema-validated distilled findings object plus the typed research claims (listed there).
  - Step 3b — a heavy route sub-distills along its natural seam (per side or sub-agenda — tier-1), then the route-level reduce is the card-formation call in its reduce form (listed there).

---

# DTO: Discover Trade Opportunities

## Step 1 — Start and safety checks

- **Data retrieved**
  - No investment data yet.

- **Checks** (the same gate Portfolio Analysis clears)
  - The single global run slot is free — no report, Portfolio Analysis, or other Trade Opportunities job is running.
  - Local reasoning and embedding models are configured (presence) and the daemon is reachable with the roster pulled (connectivity, checked here at the run-gate — never at startup).
    - That availability probe — a local-only daemon call, no investment-data API — is the suite's one check before the run slot is claimed; every external fetch happens inside the slot.
  - Schwab is connected and its seven-day refresh token is still valid — needed only for the per-candidate option chains and the Step-8 holdings label, but a hard precondition all the same.
  - FMP and FRED credentials exist (presence only; Tavily deliberately does not gate the local suite).
  - SearXNG is **not** on the gate: an unreachable instance raises a pre-run notice (model-led discovery can't run; per-candidate validation falls back to metered Tavily; flagged *not recommended* when Tavily is absent too) and the run proceeds degraded, never blocked.

- **Failure logic**
  - Missing configuration (daemon endpoint or roster id unset, Schwab not connected or token lapsed, FMP / FRED credential missing) locks the Run buttons and shows a persistent warning *before* this step — one per category.
  - A live connectivity failure caught here (daemon unreachable, a rostered model not pulled) blocks the attempt inline, not as a persistent warning.
  - Schwab API reachability is not tested here — it surfaces when the option-chain or holdings fetch runs.

- **Model**
  - None.

- **Output**
  - Job starts.
  - Or the app explains what is missing.

---

## Step 2 — Load shared market context

Loaded once per run and shared across every candidate; nothing here is re-requested per name.

- **Data retrieved from local storage**
  - The Market Signal house view — the latest report's Thesis, Investment Strategy, and Forward Outlook sections plus recent report summaries (`thesis_stance`, `forward_outlook_themes`, `key_risks`), with the report's creation date; loaded deterministically from the report store, never by vector search.
  - The fixed investor-profile preset — long-term horizon, profit-maximization objective, medium-to-high ("aggressive") risk tolerance, cash treated as always available, no tax modeling.
  - The prior run's persisted opportunity matrix (the live carries).
  - The opportunity graph — prior hypotheses with their value-chain traces, and the watchlist nodes with each one's leading metric, re-check class, falsifiers, latest gap, and refresh timestamps.
  - The discovery-coverage ledger — last-attempted / last-successfully-completed per route class and per coverage subject.

- **Data retrieved from FRED**
  - Current `DGS2` (one print) and `DGS10` (one print), plus the anchor-window `DGS10` history as dated observations (one date-ranged request) the v2 spread percentiles join against.
  - The macro-release calendar (`/release/dates` — names + dates).
  - Daily energy prices `DCOILWTICO` (WTI) and `DHHNGSP` (Henry Hub); monthly IMF metals `PCOPPUSDM`, `PALUMUSDM`, `PNICKUSDM`, `PIORECRUSDM`, `PURANUSDM`.

- **Data retrieved from FMP**
  - The daily commodity series `HGUSD` (copper), `GCUSD` (gold), `SIUSD` (silver) — a *series*, not a point level, because the cyclical sleeve reads a turn.

- **Data retrieved from CFTC and CBOE**
  - CFTC Commitments of Traders — `gpe5-46if` (E-mini S&P 500, Nasdaq-100, 10Y / 2Y Treasuries, USD index) and `72hh-3qpy` (gold, WTI crude, copper).
  - CBOE daily put/call ratios (total, equity, index).

- **Logic**
  - Omit the house view when the report is older than one week — recorded as a gap, never fed as current.
  - Normalize rates into decimal form (`N pts = N ⁄ 100`); every later return and threshold reads that representation.
  - The house view's `market_cycle` × `risk_posture` is the job's macro / regime backbone — reused, never recomputed; the forward thematic map that completes the worldview is built at Step 3b.

- **What each context input feeds (later steps, not here)**
  - House view → the Step 3b planning and route prompts (steers *where* the job hunts, never a number the engine consumes), the Step 5g scoring prompt, and the Step 6 ranking prompt.
  - Investor profile → Step 5g scoring and Step 6 ranking — entry framing and conviction emphasis only, never which candidates qualify; because cash is unconstrained, full-size entries are never gated on observed Schwab cash.
  - `DGS10` and its history → the v2 scenario multiples at Steps 5c / 5f and every cheap re-derivation (Step 7, Quick Audit).
  - `DGS2` → the entry-asymmetry gate at Step 5h and every cheap re-derivation.
  - Commodity series (FRED + FMP) → the Step 3a commodity-turn feeder and per-candidate context for a commodity cyclical (Step 5c); Step 2 is the sole commodity owner — nothing re-fetches them.
  - CFTC positioning → the commodity-cyclical candidate's underlying-positioning read (Step 5c) and the macro / rates / FX backdrop for the Step 3b theme scan.
  - CBOE put/call → a venue-level sentiment backdrop for the worldview — broad-market context, never a per-name signal.
  - Macro-release calendar → seed input to the Step 3b routes.
  - Prior matrix → the Step 4 budget split (the rotation slice and re-surfacer reconciliation), the Step 5b carried dossier, and the Step 7 carry-forward.
  - Opportunity graph → Step 3b (extend or retire prior theses; withheld from the outside-view route), Step 3c (the watchlist re-check), and the Step 7 reconcile.
  - Coverage ledger → the Step 3b coverage rotation.

- **Failure rule**
  - `DGS2` or `DGS10` still unavailable after the shared bounded retries → fail the run here, before any per-candidate work (DTO and Deep Audit; the Quick Audit instead fail-softs to a cached print — see ATO).
  - Optional market context (commodities, CFTC, CBOE, the calendar) fails softly to a gap.

- **Model**
  - None.

- **Output**
  - One shared context packet.
  - Reused for every candidate.

---

## Step 3 — Discover candidates

Three feeders run and converge: the bottom-up structured screens (3a), the model-led hypothesis-research lane (3b — the job's edge, where names are *found* by reasoning rather than looked up), and the carried-forward watchlist (3c — the discovery memory). All three are fail-soft — a failed screen, route, or re-check means fewer candidates, never a failed run — and each reduces to a candidate set with the signal that surfaced each name attached.

### Step 3a — Structured market screens

- **Data retrieved (FMP discovery layer; each a bounded number of calls per run)**
  - `company-screener` — every eligible name with market cap, sector / industry, beta, price, dividend, volume, exchange, `isActivelyTrading`.
  - `insider-trading/latest` — the market-wide newest Form-4 feed.
  - `biggest-gainers`, `biggest-losers`, `most-actives` — movers.
  - `earnings-calendar` — read forward for upcoming reporters, and **backward over the trailing window** (the paid calendar carries consensus + actuals) as the post-earnings surprise screen.
  - `mergers-acquisitions-latest`, `sec-filings-8k`, `ipos-calendar` — fresh corporate events.
  - The FINRA consolidated short-interest file — **fetched once per run** and reused as a local lookup by Steps 3c, 5b, and 7.
  - The commodity series already loaded at Step 2 — read, not re-fetched.

- **Logic**
  - **Universe and stratification** — the screener applies the hard tradability gates (price / volume / market-cap floors, `isActivelyTrading`, the allowed exchange set, equities only — the floor values are not yet drafted) and tags every eligible name with its **market-cap band and sector / industry**. This is the breadth backbone — it defines the strata Step 4 fills, not a ranked shortlist: the screener carries no valuation, profitability, or growth field, and the `*-bulk` universe-scoring endpoints are off-plan, so the universe **cannot be pre-scored**.
  - **Insider-buy clusters** — open-market buys by multiple insiders from the Form-4 feed.
  - **Short-interest extremes** — from the FINRA file: level, trend (current vs prior settlement), and days-to-cover (short interest ÷ average daily volume). Short interest is a **bearish-by-default** factor; the squeeze reading is a narrow conditional setup (an inflecting leading metric + a near-term catalyst + evidence the bear case is breaking) decided per candidate at Step 5, never here.
  - **Post-earnings surprise screen** — for each recent reporter, the surprise is **standardized** against the name's own surprise history (SUE-style — the surprise scaled by the dispersion of its past surprises, never a raw percentage; the scaling window is not yet drafted); large positive surprises surface as **continuation-mode** candidates, prioritized where the revenue surprise agrees with the EPS surprise (post-earnings drift is markedly stronger when both point the same way). The beat-and-raise streak itself is confirmed per candidate at Step 5c from `earnings`. This feeder skews coincident / lagging by construction — a defect for early detection, exactly right for continuation.
  - **Corporate events** — M&A deal flow, 8-K material events, movers, recently priced and upcoming IPOs → fresh-catalyst candidates.
  - **Commodity-price turns** — over the Step-2 FRED / FMP series, a spot / contract-price turn at washed-out sentiment (CFTC positioning) surfaces commodity-cyclical candidates. [note: the turn read's exact rule is not yet drafted; the docs name the read, not its threshold.]
  - **No fundamental scoring here** — the multi-factor composite and the forensic gate are computed per candidate at Step 5c, on the narrowed set only; the activist (13D / 13G) and congressional feeds are symbol-keyed on the current plan, so they enter per candidate at Step 5b, never as discovery feeders.

- **Model**
  - None.

- **Output**
  - A broad candidate longlist.
  - Each name carries its feeder, its surfacing signal, its cap band, and its sector / industry.

---

### Step 3b — Model-led hypothesis discovery

The job's edge: a research-active feeder that forms investable **hypotheses** and reasons its way to names — *worldview → hypothesis → mechanism → value-chain node → leading metric → candidate* — so the model commits to a hypothesis before it commits to a ticker. It runs in three movements: a planning call chooses the routes, each route is researched in the shared loop and consolidated into hypothesis cards, and the app validates and promotes.

- **Data retrieved**
  - Seeds — ticker-tagged, dated headline / snippet / URL rows from `news/general-latest`, `news/stock-latest`, and `fmp-articles`, plus the macro-release calendar from Step 2; each seed the orchestrator feeds a route gets a stable seed ID.
  - The house view, the opportunity graph, and the coverage ledger from Step 2.
  - Live web pages through the shared loop — **keyless SearXNG only** (no Tavily, no GDELT); a down SearXNG means this lane yields fewer candidates.
  - FMP industry classification (`industry-classification-search`, `all-industry-classification`, `available-sectors`) and `stock-peers` to resolve a hypothesis to its exposed public names; the screener fields to verify each name (exists, US-listed, clears the tradability gate).

#### Movement 1 — Research-strategy planning (one model call, thinking, no web tool)

- **Exact inputs**
  - The house view (regime, themes, forward outlook).
  - The carried-forward opportunity graph (prior hypotheses + watchlist status).
  - The route menu with each route's source-strategy rubric — *policy / regulatory* → legislation, agency notices, procurement databases; *supply chain* → trade journals, filings, customer / supplier commentary; *technical bottleneck* → standards bodies, engineering blogs, patent / product docs; *procurement / grant / capex*; *customer capex*; *industry history*; *failure analogue*; *event-impact / value-chain repricing* → the announcing company's primary materials, standards bodies, teardowns, the affected names' segment disclosures.
  - The app-computed coverage ages per route class and per coverage subject, and any app-inserted overdue route.
  - The run's route cap and discovery-budget posture.

- **Returns**
  - A priority-ordered route list under the route cap — each route with its source strategy, selection rationale, `selection_origin` (`outside-view` / `coverage-rotation` / `model`), any coverage-unit ids it is expected to work, and its **topic list** (the focused questions the route is worked as, each topic one isolated conversation).
  - App-validated, like the route list: the planning call proposes the topics, the app validates them (ruled 2026-08-19) — the one agenda in the suite the reasoner proposes.

- **App-enforced clauses (never model discretion)**
  - The **outside-view route** is always present and marked graph-blind — inserted if the model omitted it — so the discovery memory can't anchor the job to its own prior theses.
  - The **coverage-rotation route** is app-owned: the model may refine its questions and source plan but cannot remove it, substitute a less-overdue unit, or claim its debt cleared.
  - The **event-impact route** may be chosen speculatively — its materiality gate is research-derived and unknowable at planning time, so it is enforced at card formation (below).

- **Coverage rotation — how the inserted route is chosen**
  - Age is tracked separately for each canonical route class and each coverage subject (the stable broad-industry taxonomy plus currently active house-view themes), in calendar time against the ~4-week window — never run count.
  - When debt exists the app pairs the oldest compatible overdue class + subject and inserts that route in the first slot after the outside-view route; ties break by canonical route order, then stable subject id; a model-proposed duplicate merges into the inserted route rather than taking a second slot.
  - A newly active theme starts due; a successfully completed route advances every unit it actually researched even when it correctly emits no hypothesis; a failed or budget-exhausted route records an attempt and does not clear its debt.
  - If the route cap leaves no slot beyond the outside-view route, the debt stays overdue and the run audit surfaces it — liveness is best-effort under the cap, never bought by dropping the outside-view guard.
  - Coverage can force research, never a hypothesis, promotion, or opportunity.

#### Movement 2 — Route research and card formation (the shared loop, aimed at discovery)

- **The loop, as it runs here** (mechanics in *The research loop*, above)
  - Nesting is `route ⊃ topic ⊃ pass ⊃ fetch`: each route is worked as its topic list, each topic its own isolated conversation of a root pass plus ≤2 follow-ups, each pass a bounded tool loop of `web_search` / `web_fetch` requests the orchestrator executes.
  - Each topic conversation is given the house view, the opportunity graph (withheld from the outside-view route), the route's source strategy, that topic's questions, and the seeds relevant to the route — and nothing from any other topic or route.
  - Two ceilings bind: per-topic depth ≤2 (≤3 passes per topic, counting passes, not searches), and the **per-run discovery fetch + wall-clock budget** across every route, spent in route-priority order, fail-soft on exhaustion.
  - The Step-2 house view steers the hunt without confining it; a seed whose URL is deep-read stamps the resulting claim `surfaced_by`.

- **Card formation (one consolidating call per route)**
  - Inputs: the route's accumulated topic findings and their evidence-ledger entries, whole — or, for a heavy route, the tier-1 sub-distillates in its reduce form (no further web-tool turns) — plus the route's fed seed records with their stable seed ids (the `seeded_by` validation set, riding the call directly in either form so an oriented-but-never-fetched seed stays attributable).
  - Returns: a schema-validated set of **hypothesis cards**, each — *what is changing in the world* → *the affected system / mechanism* → *the economic value-chain trace* (margin capture, bargaining power, capacity constraint / bottleneck, pricing power **versus mere exposure** — past the crowded pure-plays to the picks-and-shovels enablers, often mid / small cap) → *the leading metric that would prove it* (tagged with its re-check class) → *the likely public-company expressions* → *bear case* → *key falsifiers + sources*.
  - Each card also carries its **hypothesis score** — the equal-weighted mean of magnitude, durability, horizon fit, leading-metric observability, 1 − crowding, and margin-capture clarity, each 0–1 — scored on the hypothesis's own merits *before any ticker*; its **adversarial-pass verdicts** — *why is this already priced?*, *why might the obvious beneficiary be the wrong expression?*, *who actually captures the margin instead?* — and a bounded, config-capped **`seeded_by`** list (the seeds the reasoner judges oriented it, each validated against the route's fed seed IDs; unknown references dropped and logged).
  - An **event-impact route's cards are two-sided**: they trace the chain into **beneficiaries**, **feared losers** (the names that sold off, with the *actually-exposed* revenue / profit pool sized), and **latent names** (chain nodes that did not move but should be affected); each affected name carries its side and a typed **`technology_read`** — `{ technical claim, deployment timeline, substitute / complement / mix-shift, affected workload or use case, exposed revenue / profit estimate, adoption constraints, switching costs, margin-capturing node, source confidence, leading metric to monitor }` — and a feared-loser name gets the symmetric pass: *is the impairment real or panic, and what is the actually-exposed pool?*
  - The model proposes hypotheses and names; it fetches no per-symbol data and scores no name here.

- **Heavy routes sub-distill first** (the shared distillation primitive)
  - Classified heavy **deterministically**, after the route's loop completes and before card formation: when its accumulated findings + ledger would overflow a single card-formation call's input budget (a byte / token measure against a configured fraction), **or** it spans more than one substantial sub-agenda (a side whose ledger size crosses the per-side threshold; the event-impact route's beneficiary / feared-loser / latent sides count substantial whenever populated), **or** it resolves to more than *K* distinct hypotheses.
  - Tier-1 sub-distillation per side / sub-agenda, then the route-level reduce **is** the card-formation call in its reduce form — it still emits many distinct cards (structure within the route, never a cross-route merge); bounded by the per-route sub-distillation cap from the discovery budget, the classification and sub-unit count logged.

#### Movement 3 — App validation and promotion

- **Per card**
  - Score ≥ 0.60 **and** the adversarial passes survived → **promoted**: its names are resolved against FMP's industry classification / peers and each is verified — exists, US-listed, clears the tradability gate — before it can earn enrichment budget.
  - 0.40 ≤ score < 0.60, or a failing pass → recorded in the opportunity graph with its score, falsifiers, and the failing pass; it may seed a **watchlist node** if it meets the discovery-worthiness bar (Step 3c).
  - Score < 0.40 → recorded in the graph only.
  - An **event-impact card** must carry the typed material-event evidence — the announcement plus at least one corroborating condition (meaningful group repricing, credible primary-source documentation, a customer-adoption signal, clear value-chain exposure), with source lineage — or it is **dropped and logged**; a route whose research surfaces no qualifying event emits nothing and stays dormant.

- **Model**
  - The 122B reasoner in thinking mode — once for planning, per topic conversation in the loop, once per route for card formation (plus tier-1 calls for a heavy route).

- **Output**
  - Promoted candidate names, each with hypothesis lineage, surfacing rationale, and source URLs → Step 4.
  - Every card written to the opportunity graph (the persisted discovery memory), with its seed lineage and, for an event-impact card, its `technology_read` and side.
  - Watchlist-bar hypotheses → Step 3c admission.
  - The hypothesis set retained as run-level worldview context for Step 5d's thematic-fit topic.
  - A completed-route record per route → the coverage ledger.

---

### Step 3c — Recheck the old watchlist

Discovery is stateful: every worthy-but-unpicked name from prior runs is a watchlist node, re-checked here at its metric's cadence — so a deferred name that quietly starts compounding is caught rather than left to chance.

- **Data retrieved**
  - The watchlist nodes from the Step-2 graph.
  - For a **`structured`-class** node — the engine's structured feeds: `analyst-estimates` (revision velocity), `earnings` (surprise), dated-EOD bars through the shared price-bar cache (relative strength), and the once-per-run FINRA file (short interest).
  - For a **`filing`-class** node — on the filing-cadence rider: when the swept `earnings` row shows a new reported period, the statement-derived rows re-pull (`income-statement` / `balance-sheet-statement` / `cash-flow-statement`, `key-metrics` / `ratios`, `financial-scores`, `financial-growth` — the rider is FMP-only; SEC submissions / company-facts stay per-candidate).
  - For a **`research`-class** node — nothing (no engine feed), unless the refresh lane selects it.
  - The swept population is **one union** — every live matrix carry (Step 7's cheap re-derivation) plus every recheckable watchlist node — with cache / dedup applied per distinct symbol after the union; a `research`-class node never enters the per-symbol sweep.

- **Watchlist admission (this step, over the Step-3b returns)**
  - The discovery-worthiness bar is app-enforced: a node qualifies only with a named, countable, dated leading metric (tagged by re-check class), a stated economic mechanism, at least one falsifier, and a hypothesis score ≥ 0.40.
  - Class resolution: a `structured` claim must resolve to an engine series the sweep re-pulls; a `filing` claim to a standardized field the filing-cadence feeds carry model-free; a claim that doesn't resolve exactly is **re-classed `research` and logged, never dropped**. The filing-derived quarterly segment series rides the `research` class (its observations exist only where a deep pass extracted them).

- **Per `structured` / `filing` node — re-pull at class cadence, then one of three outcomes**
  - Metric **inflected or continued** (the hypothesis is confirming) → promoted as a **priority feeder** into Step 4.
  - **Falsifiers tripped, metric dead or stale, or the carry horizon elapsed** (drafted ~4 of the metric's own reporting periods — never runs; a `research`-class metric counts calendar time) → **retired**: removed from active monitoring, kept in history, one retirement-class shadow episode opened so the forward path still teaches the gates.
  - Otherwise → stays on the watchlist with a refreshed timestamp.

- **The research-watchlist refresh lane (`research`-class nodes only)**
  - Selects at most the configured number of nodes (drafted **one per DTO run**), deterministically and in priority order: a newly detected filing / contract / material event tied to the node → a catalyst or validated milestone entering its expected window → a prior result close to promotion or an entry gate → highest hypothesis score; ties by oldest successful research refresh, then ticker.
  - Runs the shared loop per node (SearXNG only; spent from the discovery ceiling) — inputs: the node's stored hypothesis, named leading metric, key falsifiers, relevant milestone / catalyst, latest validation gap, prior observation vintage, and this run's matching structured-event seeds; no dossier, target methodology, conviction, or opportunity record.
  - Returns one typed **`watchlist_research_refresh`** `{ node_id, dated metric observations, falsifier facts, milestone facts, source lineage, result — confirming / falsified / unchanged / insufficient }`; the app revalidates identity, dates, units, sources, and linkage to the named claim, drops and logs out-of-scope fields, and **never** moves a node on the bare result: a confirmed metric promotes into Step 4, a validated falsifier may retire, and no result / a failed call / ambiguous evidence leaves the node unchanged without advancing `last_successful_research_refresh_at`.
  - It is a discovery refresh, not a deep re-evaluation: it never stamps `last_deep_researched_at`, rewrites an opportunity record, clears a warning, or archives — and promotion buys only normal Step-4 candidacy.

- **Capacity logic**
  - The watchlist retention cap is enforced deterministically at add time, after the pruning above: the **lowest hypothesis score** retires first, ties by oldest successful metric refresh then ticker, persisted reason `capacity-evicted`, graph history retained, exactly one retirement-class shadow episode.

- **Model**
  - None for `structured` and `filing` checks.
  - The reasoner (thinking) plus the web tool for the selected `research` nodes only.

- **Output**
  - Promoted watchlist candidates (flagged *maturing watchlist*) → Step 4.
  - The updated watchlist, retired nodes, and their shadow episodes.
  - The refresh-lane audit — every node considered / selected / skipped, its priority inputs, evidence result, and timestamp decision.

---

## Step 4 — Consolidate and allocate research slots

- **Data retrieved**
  - No new external data — the screener fields and surfacing tags are already in hand.

- **Consolidation logic (deterministic, no model)**
  - Union the three feeders and **dedup by ticker**.
  - Tradability sanity filter — exchange listing, a liquidity / price floor, and an instrument-type filter: funds and non-equities drop out, since the job hunts operating businesses by archetype.
  - Tag each surviving name with **every** signal that surfaced it — which screen, which hypothesis, the positioning flag, whether it is a maturing watchlist name — plus its cap band and sector / industry.
  - Reconcile against the live matrix: a name **already live** is a **re-surfacer** (reconciled against its record, never re-discovered blind); a name matching a **departed (archived)** ticker is a fresh **debut** — nothing from the archive carries.
  - Assign a **provisional archetype** deterministically from sector / industry + the surfacing tags — used only for the archetype quota below; Step 5a's confirmed archetype is authoritative from 5c on.
  - This is the cross-feeder reduce and it is deliberately computed: distinct hypotheses are never collapsed by a model, which would destroy auditable breadth and could silently drop a name.

- **The deep-research budget (a Settings knob — how many names get the expensive Step-5 loop this run)**
  - Spent in three slices, in this order:
    1. **The rotation slice** — a configured share (default ~20%, never rounding below one slot) on **live opportunities** in maintenance-priority order: warning-bearing names first (a tripped falsifier or continuation break never queues behind an uneventful stale name), then catalyst proximity, then names near the entry threshold, then stalest by `last_deep_researched_at`. A **max-age service level** force-promotes any live name whose research age exceeds the configured bound whether or not it re-surfaced (ties by `became_opportunity_at`, then ticker). When more names are overdue than the slice can carry, the slice does **not** expand — the overflow forms an **overdue backlog drained stalest-first through a reserved overdue sub-slot**: each run's slice holds at least one slot for the backlog's stalest name (ruled 2026-08-19), so a fresh warning outranks everything except that reservation, liveness is structural rather than best-effort, and the backlog's count and oldest research age surface in the run audit and the pre-run notice.
    2. **New names** — the remainder, filled under the diversity guardrails below.
    3. **Leftover** → re-surfaced existing opportunities, oldest `last_deep_researched_at` first.
  - **Diversity guardrails (new-name slice only)** — each a floor or ceiling, never a single ranking: mid / small-cap **floor ≥ 40%** of the new-name slots and a mega-cap **ceiling ≤ 30%**; **per feeder ≤ 50%** (no one screen, the hypothesis lane, the watchlist, or a positioning scan may supply more); **per (provisional) archetype ≤ 40%**; **per sector / theme ≤ 35%**. Within each floor and ceiling names rank by **signal strength × house-view fit**, ties resolving by ticker. Default allocation is **equal per cap-band × sector bucket** (proportional-to-population and signal-adaptive variants are calibration knobs). The rounding rule, the relaxation order when a small slate makes the ceilings jointly infeasible, the multi-tag counting rule, and the ranking score's exact formula are **not yet drafted**.
  - Why stratify rather than rank: no universe-wide composite exists at this point (the fundamental score and forensic gate are computed per candidate at 5c), so a flat top-N on the cheap surfacing signals would collapse the funnel onto whatever is loudest — mega-cap momentum, the most-covered AI names, one crowded theme — and throw away the breadth that is the job's edge.
  - The **maintenance spend** (the rotation slice and the re-surfacer leftover) is **exempt from diversity** — a quota never blocks a warning-bearing or max-age-promoted name.
  - All three classes enter the **same Step-5 loop** as candidates; a rotation pick or re-surfacer is flagged carried-forward so Step 5b loads its prior record and `continuity_weight`. A carried name's deep pass therefore runs in Step 5, before matrix assembly, not in Step 7.
  - Every ticker deep-researched — rotation, new, or re-surfaced — is recorded in the **run-scoped deep-research set**, so Step 7 never also cheap-sweeps it: at most one deep pass per ticker per run.

- **Deferred names**
  - Not rejected — nothing is validated yet. A genuinely worthy deferral (a real hypothesis + an identified leading metric, meeting the watchlist bar) is written to the opportunity graph as a **watchlist node** and re-checked every later run; a name not worth watchlisting carries no state and is simply re-derivable. A re-surfacer that wins no leftover slot falls to the Step-7 cheap re-derivation, its re-surfacing raising the attention warning so the user can choose a Deep Audit.

- **Model**
  - None.

- **Output**
  - The narrowed candidate slate — debuts plus carried names — each with its tags and provisional archetype, receiving Step-5 validation.
  - The run-scoped deep-research set, and the rotation backlog record.
  - Watchlist writes for worthy deferrals.
  - The budget bounds how many names get **researched**, never how many validated opportunities reach the matrix — the gates alone set that (Step 6).

---

## Step 5 — Deep validation loop

The following sequence runs once for every candidate on the Step-4 slate — debuts and carried names alike.

- **Checkpoint and resume**
  - Each candidate's completed stages persist (the checkpoint is written at Step 5h), so a cancellation or a single model failure resumes the unfinished candidates rather than restarting the run. Resume reopens the interrupted run's ID and pins everything upstream of the loop — the Step-2 context, the Step-3 feeder outputs and route plan, the Step-4 slate with its budget allocation — for a drafted ~48-hour window; a new Discover run discards the old checkpoints. The document-level research cache survives independently.

- **One model, switched by mode**
  - The resident 122B reasoner fills every model role in the loop — archetype confirmation (thinking), the research passes (thinking), distillation (non-thinking), scoring (thinking) — so moving a candidate through them pays no model-swap cost. The fixed 4B embedder handles continuity retrieval.

- **The engine is shared with Portfolio Analysis**
  - The same Rust financial-analysis engine computes every number; the difference is the **archetype**, which selects which signals it weights and which valuation lens it applies. The engine's values are the **engine arm** — a disclosed baseline the model reads as evidence at Step 5g and against which it authors its own arm. Nothing the model returns alters an engine value.

---

### Step 5a — Classify the archetype

The lens that decides which signals matter for this candidate.

- **Data retrieved (the classification prefetch — fired once per candidate, cached for the run and reused by Step 5b)**
  - FMP `profile` — sector, industry, beta, description.
  - The statement-derived rows the feature extractor reads: `income-statement` plus `key-metrics` / `ratios` for margin structure; the annual `revenue-product-segmentation` / `revenue-geographic-segmentation` rows for recurring-revenue structure; the statement history for cyclicality.

- **Calculations — the classification features (deterministic)**
  - Sector and industry (from `profile`).
  - Margin structure — gross and operating margin levels and their trend from the statements.
  - Recurring-revenue structure — the share of revenue in recurring / platform segments, from the annual segment rows.
  - Cyclicality — the variability of revenue and margin across the statement history.
  - The signals that surfaced the name (the Step-4 tags).
  - [note: the docs name these feature families, not their exact formulas — the cut-points are implementation-time.]

- **Model — archetype confirmation (one call, thinking, no web access)**
  - Exact inputs: the classification features and surfacing signals only — a compact, clean context; for a carried name, also the prior archetype.
  - Returns: one archetype label — secular compounder / AI-infra / commodity cyclical / disruptor / quality compounder — plus a short rationale and a confidence; the rationale may name a runner-up lens as diagnostics riding the audit only, never a second lens in targets, weights, or gates.

- **Validation — the branch is total: one authoritative archetype always emerges**
  - A valid but **low-confidence** label stands as authoritative, the record carrying an `archetype_low_confidence` degraded-input flag that Step 5g weighs interpretively — lowering conviction weight, never gating, never touching an engine number.
  - A **contradictory or schema-invalid** confirmation, or a **failed call**, adopts the Step-4 deterministic provisional archetype, flagged and logged — a deterministic fallback, never a model retry.
  - A **carried name's archetype is sticky**: the call is an affirm-or-overturn; an overturn must cite the classification feature that changed, and the app validates that the cited feature actually moved in the input delta. An unvalidated overturn does **not** take effect — the prior label holds — and is recorded as a **divergence** (the proposed label, its cited feature, the validator's reason) beside the label that held, so a lens the model repeatedly disputes is a visible, scoreable pattern.

- **What the archetype decides downstream**
  - The composite's **signal weighting** and the **valuation lens** for 5c–5g: a commodity cyclical is judged on P/B, P/NAV, and mid-cycle EPS with trailing P/E suppressed; an AI-infra name on segment-revenue acceleration and forward P/E against its revision rate; a secular compounder on PEG and revisions-vs-multiple; a disruptor on its leading operating metric rather than EPS; a quality compounder on operating-income-decoupling with valuation as a risk gate, not an entry.
  - The **target driver override** on the shared v2 scenario-target function (Step 5c).
  - The **engine horizon rule's Long branch** (a compounder archetype can earn Long on multi-year compounding — Step 5h).
  - The **engine risk tier takes no archetype input** — any archetype–tier correlation is emergent through the rule's measurable legs.

- **Output**
  - The authoritative archetype, its confidence and rationale, and any recorded overturn divergence.
  - The cached classification responses, reused by Step 5b.

---

### Step 5b — Build the candidate dossier

The application assembles the candidate's evidence packet deterministically; the Step-5a responses are reused from the run cache, and every per-candidate row fires once. This is the per-candidate surface — the budget driver — so it runs only for the narrowed slate, never the discovery longlist.

- **Data retrieved from FMP**
  - Fundamentals — `income-statement` (+ TTM), `balance-sheet-statement`, `cash-flow-statement`; `key-metrics`, `ratios` (+ TTM); `financial-scores` (Altman Z, Piotroski); `owner-earnings`, `enterprise-values`, `discounted-cash-flow`; `financial-growth` (multi-year per-share revenue / EPS / FCF / book-value CAGRs); `dividends` — the trailing distributions, the scenario function's forward-dividend leg (a nonpayer contributes a clean zero leg; a failed pull reads zero too, but with a recorded degraded-input gap — conservative at the entry gate, and the gap feeds the stand-in's flag leg).
  - Segments — `revenue-product-segmentation`, `revenue-geographic-segmentation` (annual only — trajectory context and the own-history basis; the quarterly acceleration series is research-extracted at Step 5d).
  - The revision signal — `analyst-estimates` (forward consensus, snapshotted run to run for velocity), `grades` / `grades-historical` / `grades-consensus` (the rating distribution and actions), `price-target-consensus` / `price-target-summary` (street target level and trend), `ratings-snapshot` / `ratings-historical`, `earnings` (next earnings date + actual-vs-estimate history).
  - Positioning (all symbol-keyed) — `insider-trading/search` + `insider-trading/statistics`, `acquisition-of-beneficial-ownership` (SC 13D / 13G activist stakes), `senate-trades` + `house-trades`.
  - `stock-peers`, `shares-float` (free float / liquidity), optionally `historical-employee-count` + `key-executives`.
  - `news/stock` — the symbol-scoped headline feed that seeds the Step-5d narrative / sentiment and catalyst topics.
  - M&A involvement — acquirer or target, matched from the market-wide `mergers-acquisitions-latest` + `sec-filings-8k` (the per-symbol M&A search is off-plan).
  - `quote` — the live price the engine prices targets and runs the gate against (a job-time input, logged in the audit; never a persisted current-price field).
  - `historical-price-eod/light` (dated) — the deep daily history, through the shared price-bar cache.
  - Off-plan, and where they go instead: earnings-call transcript language (backlog, book-to-bill, guidance, supply discipline) → the Step-5d research lane; press releases → `sec-filings-8k` + the web; 13F institutional flow → SEC EDGAR 13F (coarse, often omitted) and held out of the grade regardless.

- **Data retrieved elsewhere**
  - SEC EDGAR — the submissions feed (10-K / 10-Q / 8-K, item-classified) and XBRL company facts as the authoritative cross-check; for an eligible new listing or separation, the S-1 / Form 10 history. Ticker → CIK resolution is non-blocking here: an unresolved CIK degrades the EDGAR legs to the FMP working feed and reads the filing-kind forensic legs `unknown`.
  - FINRA short interest — looked up in the once-per-run file (level, trend, days-to-cover).
  - The Schwab option chain, if the name is optionable — per-contract volume, open interest, implied volatility (greeks not parsed) → the options-activity signal computed at 5c; a chain failure is a typed gap, never a failed run.
  - The Step-2 shared context.

- **Logic**
  - Cross-check the FMP working feed against SEC filings and assemble one evidence packet.
  - **Limited-history eligibility** — before research, derive a mode from objective identity facts only: `new-listing`, `spin-off-carve-out`, or `new-economic-perimeter`, and only when that event is why comparable public history is short; a missing provider response never creates eligibility, and the model never chooses it. For an eligible candidate the dossier carries the filing identifiers and source plan to recover pre-listing / predecessor evidence (S-1 or Form 10 historicals, carved-out or predecessor segment disclosures, contracts, customer / supplier observations, dated operational / technical milestones) for the Step-5d topic.
  - **For a carried-forward candidate** (a rotation pick, a budget-winning re-surfacer, or a Deep Audit selection):
    - Load its prior opportunity record and derive the **`continuity_weight`** from the age of `last_deep_researched_at` — ≤ ~1 week *continued research* (anchor on it), ~1–4 weeks *blended*, > ~4 weeks *fresh look* (test it skeptically). It frames how hard Step 5g leans on the prior thesis / conviction / bear case / milestone plan; it weights interpretation only, never an engine number.
    - Carry its **own-lifecycle retrospective** — the prior deep pass's both-arm values, the realized move since that pass, this name's matured outcome labels with their `resolution_mode`s, and its leading-metric-continuation state. The price leg is the since-flagged primitive read over a second window (since the prior deep pass rather than since entry) — a segment of the same reconstructed curve, no extra fetch, and no independent price signal; it is cap-only wherever it appears.
    - A name re-entering **from the archive** is a new lifecycle: it carries no continuity weight and no retrospective.
  - The aggregate calibration reads (picked-vs-rejected spreads, false-negative flags, archetype resolution-mode distributions) are never a dossier input — they feed the calibration pass only, behind the ≥ 30-unique-issuer bar.

- **Embedding model — vector continuity retrieval (one call, no reasoning)**
  - Input text: a query string built deterministically from the candidate — symbol, archetype, sector / industry, and the prior opportunity's thesis themes if carried — byte-capped before the call.
  - Returns: a vector validated against the shared embedding-response contract; the app runs a brute-force cosine search scoped to the **Trade Opportunities** memory partition, restricted to **record-summary rows** (the `summary` kind — a calibration learning can never match) and **scoped by lifecycle id**, so a fresh re-entry retrieves nothing from a prior lifecycle.
  - An invalid or failed response skips semantic recall fail-soft (a degraded-input flag); the deterministically loaded prior record is unaffected.

- **Output**
  - The complete candidate dossier — statements, ratios, scores, growth, segments, the revision signal, positioning, float, news seeds, price history, the live quote, the option chain, the SEC cross-check, FINRA short interest, the shared context, any limited-history eligibility + source plan, any carried record + `continuity_weight` + retrospective, and the retrieved prior analysis.

---

### Step 5c — Calculate the financial picture

- **Data retrieved**
  - Uses the dossier and the shared context.
  - No model or web research.

- **How the step runs**
  - The deterministic engine computes the candidate's quantitative picture **weighted by the Step-5a archetype** — the archetype selects which sub-scores dominate and which valuation lens applies. Everything it produces is the **engine arm**, carried into Step 5g with its methodology exposed (the `TargetMeta` derivation flags, the driver rung taken, the normalization basis, any neutral-midpoint imputation) so the model can dispute a *derivation*, not merely a number.
  - The risk-tier **inputs** (market cap, volatility, leverage, profitability, drawdown, liquidity, event exposure) are computed here; the engine tier itself is assigned by rule at Step 5h, and the inputs ride into the Step-5g prompt as evidence for the model's own tier.
  - Limited-history eligibility changes nothing here: the engine uses current structured rows plus any already-persisted app-validated comparable history, flags thin own-history as degraded, and leaves a too-short leading-metric family unmeasurable until Step 5f validates this pass's recovered observations.

- **Order of computation** — each read feeding the next where noted:
  - **Quant composite** (the multi-factor backward anchor) → **value-creation read** → **leading-metric series and its inflection read** → **earnings surprise / SUE** and **positioning** → **price-action confirmer** → **scenario targets** (structured-only, provisional) → the **three derived reads** (narrative-vs-reality, forensic flags + `forensic_event`, implied expectations) → **tradability flag** → for a carried name, the **since-flagged read**.

#### Engine primitives (used below)

- **`scale(x, lo → hi)`** maps `x` linearly onto 0–100 and clamps it; an inverted band (`lo > hi`) scores lower inputs higher. In this job the bands are **sector-adjusted** — a per-sector `lo → hi` per factor (the band values are not yet drafted).
- A **ratio** `a ÷ b` is `None` when the denominator is missing or zero.
- **Winsorizing** clips a factor value to a bounded percentile range of its reference distribution before scoring, so one outlier can't dominate a composite.

#### Quant composite (the backward anchor, computed here for the first time)

Discovery only stratified by coarse fields, so the multi-factor picture is computed here, on the narrowed slate.

- **Factors** (each a 0–100 factor score)
  - **Value** — earnings yield, FCF yield, and EBIT-to-EV.
  - **Momentum** — 12-month-minus-1-month price return *and* the name's own time-series trend.
  - **Quality** — anchored on gross profitability `(revenue − COGS) ÷ total assets` (the least-polluted quality metric), plus profitability, growth, and safety legs.
  - **Low beta** (from `profile`).
  - **Conservative investment** — low asset growth.
  - Size enters **only within quality** (small-*and*-high-quality, never raw small-cap).

- **Event / flow signals** (ride beside the composite)
  - **Estimate-revision breadth** `(up − down) ÷ total` and revision velocity from the run-to-run `analyst-estimates` snapshots; **rating drift** from `grades-historical`.
  - **Earnings surprise (SUE)** and its post-announcement drift (below).
  - **Insider cluster buys** (open-market, multiple insiders — strongest in small caps); **institutional accumulation** (13F off-plan → omitted or coarse EDGAR); **short interest** — bearish by default, a squeeze read only as the conditional setup (inflecting metric + catalyst + breaking bear case).

- **Normalization and roll-up**
  - Each factor is scored against its **sector-adjusted absolute band** plus the company's **own-history distribution**, winsorized — honest without any accumulated sample, and the sector adjustment keeps the composite from being a disguised sector bet.
  - It is deliberately **not** a within-cohort rank: a within-run cohort has no statistical mass (the quotas spread the slate to one-to-three names per bucket), ranking against live peers would multiply the per-symbol budget, and the persisted **factor-distribution store** is a selected convenience sample — so that store is **diagnostic-only, never a score input** (shown in the dossier / audit as context once a bucket holds ≥ 20 unique issuers, drafted).
  - A factor that failed to resolve is **imputed to its band's neutral midpoint** (disclosed), so a missing call can't silently sink a name; the factors integrate at **one final composite score** rather than chained hard cutoffs — value and momentum are negatively correlated, so sequential cutoffs would collapse breadth.
  - **Archetype weights** — the archetype's dominant axes tilted ~2× over a neutral baseline: AI-infra → momentum / revision; commodity cyclical → valuation (P/B, P/NAV); quality- and secular-compounder → quality / ROIC; disruptor → growth / leading metric; the remaining axes balanced. [note: the exact weight vectors are not yet drafted.]
  - A composite resting on thin own-history carries a **low-confidence degraded-input flag** — lowers conviction weight, never gates.
  - Where it lands: the engine arm's **sub-scores** (on the shared 0–100 scale, so the model's own sub-scores at 5g are comparable by construction) and the value lens's read for scoring.

#### Value-creation read

Whether the business *creates* value rather than just growing.

- **ROIC vs cost of capital** — the spread; growth funded below the cost of capital destroys value however fast revenue grows.
- **Owner earnings with R&D capitalized** — so research-heavy leaders aren't mis-scored as unprofitable.
- **Reinvestment runway** `g ≈ ROIC × reinvestment rate`.
- **Moat-source features** — intangibles, switching costs, network effects, cost advantage, efficient scale — weighted by how often each actually sustains a durable advantage.
- Inputs: the statements, `key-metrics` / `ratios`, `owner-earnings`, `financial-growth` (the multi-year per-share CAGRs → growth trajectory and the runway read).
- [note: the cost-of-capital and R&D-capitalization conventions are not yet drafted.]

#### Leading-metric series and the inflection read

The anchor the whole thesis hangs on — per archetype: revision velocity (AI-infra), segment-revenue acceleration (AI-infra / disruptor / secular compounder), a commodity-price turn (commodity cyclical), margin decoupling — operating income growing faster than revenue (quality compounder), or a disruptor's named operating metric.

- **The series** — built from the structured feeds where the class is `structured` / `filing`, or from the **stored** research-extracted series where it is `research`: the quarterly segment observations are app-appended per filing from Step 5e's typed returns (FMP's segment endpoints are annual-only), so a **debut's** segment series can read unmeasurable here and become measurable only through the Step-5f recompute once this run's research lands.
- **"Inflecting" is metric-family-shaped, archetype-mapped, two-phase, noise-floored** — the shape test belongs to the family declared on the metric:
  - *Accelerating* (segment revenue, revision velocity, net-adds) — the rate of change rising.
  - *Turned-and-holding* (commodity prices, ASPs) — a positive move after a declining stretch, or holding above a turn ≤ 4 reporting periods back; never re-demanding re-acceleration.
  - *Stability-under-stress / decoupling* (a renewal rate through a price hike; operating income outgrowing revenue with the gap widening).
  - *Threshold-crossing* (a first profit, an FCF turn).
  - *Deterioration* — the exit shape the continuation state watches for.
- **Comparability** — changes are seasonally comparable (YoY for quarterly reported series, never QoQ on seasonal data); trend is a robust slope over the available points, never the latest two deltas alone.
- **Continuation phase** (a continuation-mode candidate, or any re-check of an already-inflected anchor) — inflected within the trailing ~4 reporting periods and not rolled over; steady post-inflection strength passes.
- **Noise floor and minimum history** — a qualifying move must exceed `0.5 × σ` of the series' trailing comparable changes (up to 8); *accelerating* / *turned-and-holding* need ≥ 5 comparable changes, *stability-under-stress* the full stressor window, *threshold-crossing* the crossing plus one confirming observation; below its family minimum the series is **unmeasurable** → an evidence-floor abstention at 5h unless research supplies further dated third-party observations — never a story-stock rejection.
- **Direction** is by declared metric polarity — a narrowing loss counts as improvement.
- Always dated and third-party-sourced, else the candidate is a story stock.
- Where it lands: the leading-metric trend and continuation state on the engine arm; the evidence floor (5h); the `business_runway` durability read, which the engine derives from validated runway proxies — penetration + falling unit cost, contracted multi-year backlog coverage, a multi-year forward assumption, a validated multi-year milestone chain — mapped to years at Step 5f (penetration ≲ 20% with unit cost still falling → ≥ ~5 years; contracted backlog coverage → the covered years, ≥ ~3 clears; an assumption or milestone chain → its validated span; none cleared → `unknown`, a degraded input, never a gate). On a debut with no validated research yet, runway reads `unknown` here.

#### Earnings surprise, positioning, and the price-action confirmer

- **SUE** — each reported surprise standardized against the name's own surprise history (from `earnings`); the beat-and-raise streak confirmed here; post-announcement drift as a continuation tell.
- **Positioning** — insider net buying (`insider-trading/*`), congressional buys, activist 13D / 13G stakes, short-interest level / trend / days-to-cover (FINRA), CFTC positioning for a commodity cyclical's underlying (Step 2), and the **options-activity signal** from the Step-5b chain — put/call by volume and by open interest, and the IV/skew read (the whole-chain form: mean put IV minus mean call IV, no moneyness banding; the banded form is the calibration slice's) — an activity proxy, **held out of the grade** until calibrated.
- **Price-action confirmer** — relative strength vs the market (`^GSPC`) and the sector benchmark, and proximity to a multi-year base breakout, from the dated-EOD deep history (reusing the shared engine's momentum / volatility computations). A cross-archetype **confirmer, not a trigger** — it adjusts conviction at 5g, never substitutes for the leading-metric anchor.

#### Scenario targets — the v2 rate-anchored function (structured-only set)

Bear / base / bull price targets over the fixed **twelve-month** window, priced from a per-share driver and a rate-anchored multiple. The function is the same one Portfolio runs — restated compactly here; the clamp, repair, and fallback detail is at `portfolio-analysis-logic-flow.md` §Scenario targets — with one Trade Opportunities leg: the **archetype driver override**.

- **Choose the driver** — the archetype names the preferred driver / multiple form, the shared ladder's fallback discipline applying when it isn't computable:
  - Quality- and secular-compounder → consensus forward EPS / P-E.
  - AI-infra and disruptor → forward revenue per share / P-S while pre-profit — the ladder climbs back to EPS once a positive EPS consensus exists.
  - Commodity cyclical → **mid-cycle EPS** = median margin over the trailing cycle window (drafted ~5 years) × forward revenue per share, / P-E — spot earnings mislead at cycle extremes (the archetype's P/B / P/NAV read stays a scoring lens, never a target path).
  - Every forward per-share conversion reads the ladder's one share basis — the latest reported diluted count; the anchor window's historical revenue-per-share prints take a diluted count from inside their own TTM window, never the latest filing's or today's (`portfolio-analysis.md` §Starting parameters).
  - No positive forward-EPS consensus and no computable forward revenue per share → `no-admissible-driver`, an evidence-floor abstention (the gate cannot price a name with no computable target).
- **Build the three driver cases** — base = the consensus mid, bear / bull = the low / high (a missing spread holds both at the mid, flagged flat), each clamped to `[trailing × 0.75, trailing × 1.35]`.
- **Calculate the multiple** — per historical quarter (~12): driver yield = `driver ÷ price`, spread = `yield − that quarter's DGS10` (latest on or before); the bear / base / bull spread percentiles (75th / 50th / 25th — a wider spread is a cheaper multiple); re-anchored with today's `DGS10`: `multiple = 1 ÷ (spread percentile + today's DGS10)` (needs ≥ 8 observations; else raw-multiple percentiles; with no history, the current `spot ÷ base driver` multiple carried).
- **Price and return** — `twelve-month price = driver × multiple` per scenario (crossed scenarios repaired to ascending; a volatility-scaled dispersion floor widens, never narrows, the bear / bull spread); `total return = (price + forward dividends) ÷ spot − 1` (the dividend leg from the 5b-pulled trailing distributions; a nonpayer contributes zero).
- **Where these land** — the structured-only target set with its `TargetMeta` (anchor form: rate-anchored / current-multiple carry / raw-percentile fallback; driver rung; flat / clamp / dispersion flags) on the engine arm; a **provisional scenario menu** until Step 5f; the entry gate (5h) and the implied-expectations inversion below read it; the anchor-window percentiles and drivers persist as the basis every cheap re-derivation re-anchors against.

#### The three derived reads selection leans on

- **Narrative-vs-reality ratio** — estimate-revision pace vs multiple change over a trailing 12-month window (or since `became_opportunity_at` when the idea is younger): *justified-expensive* when estimates outrun the multiple; **`hype`** when multiple expansion exceeds **70%** of the price move. For a thinly-covered name whose estimates are absent or stale, the numerator falls back to **operating reality** — the hard operating momentum the company itself reports (segment revenue, backlog / bookings, gross profit, unit economics, cohort retention — the archetype's leading metric) against the move in price and multiple. Where it lands: the engine-arm read the model weighs at 5g; the 5h forensic / risk gate (`hype` caps; anchorless `hype` excludes a debut).
- **Forensic flags** — computed from the statements and `financial-scores`:
  - Soft flags (each a 5h **cap** trigger): Altman Z < 1.8; Piotroski ≤ 3; net income > 1.3× operating cash flow; receivables / inventory growth > 1.5× revenue growth.
  - Also computed as evidence: margin compression while revenue accelerates; the restatement / auditor-change history.
  - **Forensic event kinds are typed events with named producers, never bare model assertions** (the hard consequences ride the filing kinds alone) — the shared `forensic_event` record `{ event kind — restatement / auditor-change / fraud; issuer; event / filing date; source lineage; confidence }`. The two **filing kinds are engine-detected, model-free**, from the item-classified SEC 8-K submissions already on the surface — a restatement from an Item 4.02 non-reliance filing, an auditor change from an Item 4.01 filing; an unresolved CIK reads these legs `unknown` (a logged degraded input, never a fabricated clear). The **fraud kind is research-fed only** — it enters as a validated Step-5e `forensic_event` claim cited to a primary-source document (regulator / court / the issuer's own filing) — and is **advisory by the 2026-08-24 ruling**: cited attention evidence in scoring, never part of 5h's automatic hard set.
- **Implied-expectations read** — the v2 scenario math inverted at the live quote under the archetype's driver override: the driver growth the spot implies across the scenario multiples, plus the margin dimension where the driver is revenue-based → a **range** of growth / margin trajectories the price already assumes under stated assumptions, never one solved number (many combinations justify a price). Where it lands: the engine arm's implied-expectations read — the anchor for 5g's priced-in / crowding judgment and the *why is this already priced?* discipline.

#### Tradability flag and since-flagged read

- **Tradability flag** — Amihud-style illiquidity plus days-to-cover, resolved into the entry gate's **banded liquidity haircut**: unflagged 0 / flagged −3 pts / severely flagged −6 pts (the band boundaries from the flag's own inputs — not yet drafted) — so a small illiquid name is discounted, not silently excluded.
- **Since-flagged read (carried names only)** — the price legs — running return since `became_opportunity_at` (absolute, vs sector, vs market) and maximum drawdown over that window — from the dated-EOD history, the identical primitive Step 7's scorecard uses, so 5g can weigh how the idea has actually done, cap-only. The leading-metric-continuation state is the read's other part and is **not** price-derived — it comes from the metric's own re-check-class path (a `research`-class anchor holds its last read between deep passes).

- **Model**
  - None.

- **Output — the engine arm, provisional**
  - The archetype-weighted **sub-scores** and quant composite (with the normalization basis and any neutral-midpoint imputation disclosed), the value-creation read, the leading-metric series / trend / continuation state, SUE, positioning and the options signal, the price-action confirmer.
  - The **structured-only scenario target set** with its `TargetMeta`, the narrative-vs-reality read, the forensic flags and any filing-kind `forensic_event`, the implied-expectations range, the tradability flag and its haircut band, `business_runway` (or `unknown`), the risk-tier inputs.
  - For a carried name, the since-flagged read.
  - The degraded-input flags disclosed so far (thin own-history, `archetype_low_confidence`, imputed factors, vector-recall miss, a failed dividend pull's zero-leg gap) — the engine stand-in's flag leg at 5h.
  - Nothing the model returns downstream alters any of these values.

---

### Step 5d — Research the company

The shared research loop (*The research loop*, above), aimed at one candidate. This is the only stage in the per-candidate loop that itself loops; it builds the three research lenses — and the mandatory bear case — around the engine's numbers, so research fills the gaps the numbers can't rather than substituting a story for them.

- **What differs here**
  - Unit: the candidate's agenda below; one isolated conversation per topic.
  - Budget: a **per-candidate** fetch + wall-clock budget, spent in topic-priority order — **leading metric and bear case first** — fail-soft on exhaustion (the lowest-priority topics drop to a recorded gap).
  - Search: **SearXNG first, Tavily fallback** (a name's research should complete).
  - Seeds: the candidate's `news/stock` headlines from the dossier, as leads; the Step-3b hypothesis set as run-level worldview context.
  - Terminal consolidation: Step 5e.

- **The agenda (assembled deterministically by the orchestrator)**
  - **Leading-metric validation** (mandatory anchor) — confirm the engine's leading metric is real, countable, dated, and inflecting from a third-party source; for a research-extracted series (the quarterly segment observations) this topic also deep-reads the name's own 10-Q / press-release segment disclosures and captures each period's dated, cited observation for the typed Step-5e return.
  - **Limited-history reconstruction** (conditional — only when Step 5b marked the candidate eligible) — read the identified S-1 / Form 10 / carve-out / predecessor disclosures and third-party operating evidence, recover dated observations, and state for each whether it is directly comparable, needs a disclosed recast, or is only a proxy; never treat a customer / supplier proxy as the issuer's revenue or merge unlike economic perimeters.
  - **Macro / thematic fit** — which theme the name rides and where it sits on the S-curve (against the Step-3b thematic map), pure-play vs enabler at a margin-capturing, capacity-constrained node, bottom-up TAM (units × price) vs top-down, and the economist's front-running indicators the feeds don't carry (capex commentary, book-to-bill, freight, the cycle, PMIs).
  - **Investor judgment** — the driving narrative and market sentiment (how much of the price is emotion about what might come vs present fundamentals), management quality and capital-allocation behavior (insider buying, buybacks, guidance delivered vs promised, candor in bad quarters), durability of growth, and the pre-consensus tells (thin coverage, low institutional ownership, a variant perception) — seeded by `news/stock`, deep-read on the open web.
  - **Pattern / case study** — the candidate against the **shipped episode library** (name, period, archetype, the tell's dated metric series, how it resolved), its matching episodes supplied to this topic as **structured retrieval, never model recall** — the recurring early tells (a high-margin segment compounding inside a lower-margin whole, an oligopoly's supply discipline turning, a usage metric inflecting ahead of revenue, new management + a credible guidance step-change, forward customer commitments, replicable unit economics with whitespace) against the red-flag set (a multiple outrunning estimates, peak-cycle margins extrapolated, pull-forward mistaken for trend, a narrative with no metric, a deteriorating metric behind strong share, earnings-quality games). The library ships in three partitions — grounding (what this lens retrieves), development (the only set gate constants are shaped on), and a locked holdout never tuned against.
  - **External corroboration** the feeds can't give — customer / hyperscaler capex, supply discipline (capex cuts, curtailments), transcript backlog / TAM / inflection language, and DRAM / NAND ASP direction.
  - **The contemporaneous bear case — mandatory** — why the name might fail; the candidate cannot reach scoring without a stated, sourced bear case (the winning traits also rode the famous failures down).
  - Then the **disconfirming-fetch pass** — one bounded pass after the topics, searching for what would disprove the thesis, from the same budget, outside the depth cap, fail-soft to a gap.

- **What each topic conversation is given**
  - The candidate's dossier facts, its archetype, and its computed leading-metric reads (the engine's quant and value-creation numbers ground the research).
  - That topic's questions only; the relevant seeds; the pattern topic's retrieved episodes.
  - No other topic's findings.

- **What each model call returns**
  - Per turn: `web_search` / `web_fetch` requests, or the pass's findings. Per topic: its **full findings response**, preserved whole with its evidence-ledger entries (each claim + source URL + retrieval timestamp + any `surfaced_by`), plus any **follow-up proposal** (the orchestrator decides) and any **material forward fact** flagged for the Step-5f refinement. There is no in-loop re-distillation — every worked topic's full response flows intact to Step 5e.

- **Model**
  - The 122B reasoner in thinking mode, requesting tools the orchestrator executes (SSRF-guarded; page text inserted as quoted evidence, never as instructions).

- **Failure logic**
  - Web failure reduces evidence and may lower conviction; it never fails the run. A hard model failure fails the run; the candidate resumes from its last checkpoint.

- **Output**
  - Full findings for every worked topic; any lower-priority topic the budget couldn't reach is a recorded degraded-input gap.
  - The evidence ledger with sources and timestamps; the seed-lineage lane.
  - The mandatory sourced bear case; the disconfirming pass's findings (or its gap).
  - The reused-vs-freshly-fetched document split for the audit.

---

### Step 5e — Distill the research

The shared distillation primitive (*Distillation*, above) over this candidate's research — the only place research is condensed before scoring, and it always consolidates full-context research, never already-distilled notes.

- **Data retrieved**
  - No new external data.

- **The consolidation call(s) — exact inputs**
  - *Single pass* (the orchestrator sized the full input under the overflow threshold): every worked topic's full findings response (the bear-case topic included) plus the append-only evidence ledger, **and the engine's Step-5c reads — the quant composite, the value-creation read, the narrative-vs-reality ratio, and the forensic flags — as the two engine lenses the research findings are reconciled against**, so the contradiction check spans all five lenses.
  - *Hierarchical*: each tier-1 call gets one topic-tree's complete findings + that tree's ledger entries (no engine reads — it sees one tree); the tier-2 reduce gets the tier-1 structured outputs plus those same engine reads, every claim carrying its citations.
  - The shape and tier count are logged; hierarchical distillation is bounded by the per-candidate sub-distillation cap and the run's wall-clock.

- **Model determines** (non-thinking — consolidation, not new reasoning; no searches; no financial numbers)
  - Which findings matter per lens — the leading-metric validation, the narrative / sentiment read, the forward-opportunity read, the bear case — each cited.
  - The **cross-lens contradiction read**: which lenses disagree (a strong thematic story over a failing value-creation, management, or unit-economics read; a composite that likes a name its unit economics don't support), with a **severity** — produced at the consolidating pass that first sees all five lenses (the single pass, or the tier-2 reduce; never tier-1), so it costs no dedicated call.
  - The thesis's **key falsifiers** — specific monitorable conditions that would break it, each typed by re-check class (`structured` / `filing` / `research`).
  - Any stale or conflicted source.

- **Claim identity**
  - Every accepted evidence-ledger claim receives a stable deep-pass **`claim_id`**, persisted with any target-scenario or milestone consumer, which those outputs must reference instead of repeating or silently altering the fact.

- **Typed channels into the engine** (each app-validated at Step 5f before it binds — which is exactly why none is part of the model arm)
  - **`research_forward_assumption`** — a direct sourced forward fact the feeds lacked: `{ fact type, numeric value, units, period / as-of date, source URL, confidence, target assumption affected, conflict_handling — supplement | supersede }` (`conflict_handling` is a typed declaration the engine validates, never a rule the model selects).
  - **`research_target_scenario`** — a composite bridge when no single fact captures the inference: `{ target driver, fixed target period, bear / base / bull scenario nodes, dependencies, confidence }`, each node `{ evidence claim ids, typed expression }`, each dependency `{ predicate over a validated claim or named engine field, affected scenario ids, on_failure: fallback-to-structured }`. The expression is a closed, non-executable tree whose leaves are validated `claim_id`s or named current engine fields and whose only operations are `add`, `subtract`, `multiply`, `divide`, `min`, `max` plus app-owned unit conversions — units × ASP → revenue, backlog × conversion → revenue, subscribers × ARPU → revenue, revenue × margin ÷ diluted shares → EPS. Arbitrary code, free numeric literals, a model-authored multiple / discount rate / price, or a bridge that does not resolve dimensionally to the archetype's admissible driver are invalid.
  - **`leading_metric_observation`** — a dated backward observation for a research-extracted series: `{ metric, period, value, units, filing / as-of date, source URL, confidence }`, appended for the Step-5f recompute.
  - **`limited_history_evidence`** (eligible candidates only) — `{ eligibility reason, metric or milestone, value / state, units, period or as-of date, source URL, source entity, target entity, economic-perimeter mapping, comparability — direct / recast / proxy, mapping rationale, confidence }`. Only `direct`, and `recast` with an explicit reconciliation, may extend the issuer's series or substitute statement history; a `proxy` stays corroboration or milestone evidence and can never be inserted as the issuer's financial print, satisfy the statement floor, or become a target-driver value.
  - **`runway_evidence`** — `{ proxy kind — penetration-cost-curve / backlog-coverage / forward-assumption / milestone-chain, numeric value(s), units, as-of / period, source URL, confidence }`, mapped to years at Step 5f.
  - **`milestone_evidence`** — `{ claim_id, kind — operational / financial / catalyst / market-recognition, observed or expected event, explicit date or bounded interval where sourced, measurable completion fact (optional), source URL, confidence }`; Step 5g may combine several into an inferred interval, but the original claim ids and explicit dates stay immutable inputs to 5h.
  - **`validated_leading_indicator`** — an engine-unscored `research`-class signal: `{ metric, value / level, direction, as-of date, source URL, confidence, confirmed key-driver or milestone reference }`. With the conviction-raise machinery retired this is **evidence, not a permission slip**: it carries the signal into scoring, the thesis drivers, the key falsifiers, and the milestone plan; its source, dating, and third-party independence are still validated (malformed → dropped and logged), but no conviction arithmetic hangs on it.
  - **`forensic_event`** (the fraud kind) — the Step-5c producer record cited to a primary-source document; a malformed or non-primary-sourced claim is ignored and logged.

- **Output**
  - One schema-validated **distilled findings object** — the per-lens findings, the typed contradiction read, the key falsifiers, the stale / conflicted sources — plus the typed claims above, each claim carrying its `claim_id` and citations, for Steps 5f and 5g.

---

### Step 5f — Recalculate using validated research

The engine applies what research validated, and nothing else. The same refinement contract as Portfolio's Step 6e, extended by the observation-append, the research-target-scenario bridge, and the limited-history mapping legs.

- **Data retrieved**
  - No new data.

- **Retain the counterfactual**
  - The structured-only bear / base / bull target set from Step 5c is kept immutable as this pass's counterfactual.

- **Validate the direct forward assumptions** (each `research_forward_assumption`) — **shadow-only, ruled 2026-08-24 suite-wide**
  - Reject a malformed, unsourced, non-numeric, stale, or dimensionally incompatible claim.
  - A claim that conflicts with a structured feed resolves under the **app-owned conflict policy** — the model's declaration never selects the rule: `supplement` may only fill a value the feeds don't carry (never displaces a present value); `supersede` is honored only when the engine verifies an as-of date strictly newer than the conflicting observation, a fact type on the primary-source whitelist (issued guidance, a signed contract, a filed figure), and metric, units, and period matching the feed field; otherwise structured wins. Every accepted or rejected rule is recorded.
  - The accepted claim's fill records as a **shadow would-have outcome on the audit and never enters the applied driver set** — research-informed drivers reach the applied set only through the claim-by-claim-validated target bridge below.

- **Validate and evaluate the target bridge** (a `research_target_scenario`)
  - Validate every referenced claim id, engine-field leaf, dependency, operation, unit, period, and the target-driver output's dimension; then evaluate each bear / base / bull expression itself.
  - An invalid scenario leg falls back **independently** to that leg's structured-only driver with the failed condition recorded; an invalid or absent base bridge never floors the candidate by itself, because the structured baseline remains.

- **Recompute**
  - Apply the unchanged v2 rate-anchored multiple function to the resulting admissible drivers, run the positivity / growth-clamp / monotonicity guards, and record **both target sets** plus the exact evidence-to-driver bridge and delta.
  - **Which set is authoritative**: the validated research-informed set while its bridge's evidence is inside the ~4-week freshness window — the forward outlook and the Step-5h gate input; without a valid current bridge, the structured-only set.
  - Validate each `limited_history_evidence` observation — the Step-5b eligibility reason, source and target identities, economic perimeter, period, units, comparability, any recast reconciliation — before it reaches a series; a rejected observation stays in the audit and never enters a series or statement substitute.
  - Where validated `leading_metric_observation`s or `direct` / reconciled-`recast` limited-history observations were appended, **recompute the leading-metric read over the extended comparable series** — and the reads derived from it, `business_runway` included (each `runway_evidence` proxy mapped to years by the Step-5c duration rules, a validated multi-year milestone span joining them; the strongest cleared proxy sets the runway) — so the 5h gate and floor evaluate the **post-research** series, and a debut whose series was unmeasurable at 5c is admitted or abstained on what research actually supplied.
  - Operational / technical milestones and customer / supplier proxies stay separately cited corroboration under their own rules — they can validate a threshold crossing or milestone chain, never manufacture a missing comparable financial period.
  - The backward-looking sub-scores and derived reads are untouched; absent a valid assumption or appended observation, the Step-5c reads and targets stand.

- **Model**
  - None.

- **Output**
  - The structured-only target set (counterfactual) and the research-informed target set, with the exact bridge and delta and every accepted / rejected rule or leg.
  - The leading-metric read, continuation state, and `business_runway` re-derived over the extended series.
  - The accepted / rejected limited-history mapping record (revalidated exact-equal at 5h).
  - The final engine-arm reads for Step 5g.

---

### Step 5g — Author the opportunity record

The opportunity-authoring call, and the stage where the **model arm is written**. One model call (thinking, schema-constrained).

#### Exact inputs

- **The engine arm, in full — as evidence, never as values to reproduce**
  - The archetype-weighted sub-scores and quant composite (with normalization basis and imputations disclosed); the value-creation read.
  - The structured-only and research-informed scenario targets with their exposed methodology (`TargetMeta`) and the bridge delta.
  - The narrative-vs-reality read, the forensic flags, the implied-expectations range.
  - The price-action confirmer; the positioning reads and the options signal.
  - The risk-tier inputs (market cap, volatility, leverage, profitability, drawdown, liquidity, event exposure) and the engine's `business_runway` read — the measurables behind the engine's placement legs, as evidence for the model's own tier and runway.
  - Exposing the methodology is deliberate — the model is asked to dispute a *derivation* where it disagrees, not merely to name a different number.
- **The distilled research** — the per-lens findings including the mandatory bear case, the target-scenario and milestone evidence, the cross-lens contradiction read and key falsifiers, the validated leading indicator.
- **The candidate's archetype and surfacing signals** (its hypothesis lineage and any `technology_read`).
- **The house view and the investor profile** (entry framing and conviction emphasis only).
- **Any prior opportunity record for this name**, framed by `continuity_weight` — continued research to anchor on, blended, or a prior view to test skeptically.
- **For a carried-forward name**: the own-lifecycle retrospective (the prior pass's both-arm values, this name's matured labels and their `resolution_mode`s, its leading-metric-continuation state) plus the since-flagged read — one price primitive behind both, the retrospective quoting its segment since the prior deep pass, the card since first entry.
- **The absolute street opinions** (consensus target level, current rating consensus, FMP's ratings snapshot) — evidence to weigh against both arms' reads, not numbers to adopt.
- **Deliberately excluded**: raw statements, filings, and page text — only computed values and distilled research reach the model; the engine's **mechanical conviction stand-in**, computed only at Step 5h (its gate-distance leg needs the engine horizon derived there from this call's own milestone plan — feeding it back would be a causal loop; ruled 2026-08-19, mirroring Portfolio's 6f holdout of the engine's stand-in picks); and the engine's **rule-derived tier and derived horizon** — both 5h assignments (the horizon cannot exist yet, deriving from this call's own milestone plan; the tier is held out deliberately so the model's placement is authored unanchored from the raw tier inputs above, mirroring the stand-in holdout — the placement ruling, 2026-08-19).

#### Discipline the prompt states (prompt-side, human-auditable — not an app clamp)

- Score the **conjunction** of the lenses, never a single signal — base rates are brutal, and the winner traits recur in losers.
- Require the **leading-metric anchor plus external validation**; apply the **narrative-vs-reality ratio**.
- Treat **price action** as a confirmation overlay that adjusts conviction, never a substitute for the anchor.
- For a carried name, read the **since-flagged performance** cap-only: a gain unmatched by leading-metric progress caps conviction (the asymmetry narrowed); a gain *matched* by it is neutral — never boosted, since the metric is already scored at 5c and crediting the gain too would double-count; a drawdown with the metric intact reads as improved asymmetry, not a reason to abandon.
- **Resolve the cross-lens contradiction** — a loud lens contradicted by a weak value-creation, management, or unit-economics read is capped, not promoted, never averaged away.
- Run the archetype's **track** — proven-economics (trailing returns on capital + a margin of safety) or emerging-economics (a forward TAM × penetration × margin model clearing a return hurdle) — both through the same moat / management / price-asymmetry gate, so a strong-numbers name and a revolutionary one are judged on one spine.
- Place the **horizon** consistently with the milestone plan this same call authors, and the **tier** from the disclosed measurables — placement is authored judgment under the same audit discipline, not a schema the app checks for consistency.
- Since the two-arm contract the app derives no conviction for this arm, so these levers are **instructed and auditable rather than enforced**; the scoreboard measures a band's accuracy, never why the model chose it.

#### What the model returns — the opportunity record

- **Shared fields it proposes**
  - The directional thesis; the **detection mode** (early / continuation); the **leading operating metric** and its trend; the typed **catalyst** `{ description, date (optional), payoff_bearing }`.
  - A proposed **`thesis_milestone_plan`** — an ordered DAG of milestones, each `{ temporary label, kind — operational / financial / catalyst / market-recognition, description, expected window { earliest, latest }, timing basis — explicit-date / inferred-interval, timing derivation, evidence claim ids, prerequisite labels, measurable completion condition (optional), proposed re-check class, payoff_bearing, confidence }`, with one named **payoff milestone**. An inferred interval must cite its claim ids and state the derivation; it cannot return a bare horizon label, or use a bare earnings date as a payoff milestone.
  - The mandatory bear case, the key falsifiers, the entry consideration, any tripped risk / forensic flags it sees.
  - For a **carried name only**, a proposed carry-forward **status** (`still-valid` / `invalidated`), every status move attributed to an input that changed; a **debut's record carries no status choice** — the app stamps `new` — the enum origin-constrained by the schema so an incompatible value is structurally impossible (ruled 2026-08-19).
- **The model arm — structurally validated only; no bound, band, ceiling, or clamp within each field's typed shape**
  - A single **conviction** value with its rationale (no triple, no raise field, no app re-derivation).
  - Its **own sub-scores** on the shared 0–100 scale.
  - Its **own bear / base / bull bands** over the fixed twelve-month window.
  - Its **own implied-expectations read**.
  - Its **own risk tier, horizon, and business-runway read** (the placement ruling, 2026-08-19; the runway a positive year count — fractional allowed — or `unknown`, unbounded above) — the tier × horizon are the card's matrix cell; the engine's derived values persist beside them as the baseline, a mismatch recorded as a divergence; the gate's legs never read them (the required-return scale, haircut, and H stay engine-derived).
- **For a carried name, a `self_assessment`** — how its prior call resolved against the retrospective it was shown; prose input to the learnings, never the scorekeeper.

#### What the model does not do

- Echo any engine value — the engine's target sets, sub-scores, narrative-vs-reality read, and stand-in conviction are app-stamped onto the record directly.
- Assign `expected_thesis_realization` or rewrite any engine-arm value — Step 5h still derives the engine's tier, horizon, realization basis, and runway from validated inputs, never thesis prose; the model's own tier / horizon / runway are its arm's authored fields, a second reading beside the engine's, never a replacement of it.
- Alter the engine arm's multiples, prices, or returns; enforce a gate (5h runs the gate on both arms itself).
- Raise conviction through a permission slip — the ≤ one-level raise, its `validated_leading_indicator` citation, and the app's re-derived final conviction are **retired**; conviction is the model's own and bidirectional.
- A **blind-first diagnostic** (engine-blind, or realized-move-blind for carried names) is reserved as a second call that would re-issue this one with an input withheld — diagnostic-only, never admitting, displaying, or scored as a third arm, its execution deliberately unspecified until the job is implemented.

- **Output**
  - The proposed opportunity record — shared fields, the model arm (placement included), any `self_assessment` — for Step 5h.

---

### Step 5h — Deterministic final validation

An app-layer validator and tier-assigner, not a recorder. No model. Every rule below reads engine values and validated research; the model arm is validated structurally and otherwise left exactly as authored.

- **Data retrieved**
  - No new data.

- **Engine risk tier (rule-derived; no archetype term — the gate's scale and the baseline beside the placement)**
  - **High** if any: market cap < $2B · realized volatility > 40% · debt/equity > 2 · unprofitable · drawdown > 50% · illiquid (thin ADV / high Amihud) · high event exposure.
  - **Low** if all: market cap > $10B · profitable · debt/equity < 1 · volatility < 25% · liquid.
  - Otherwise **Medium**.
  - **Missing-input rule** (ruled 2026-08-19): a leg whose input is missing simply cannot trigger, and a candidate whose tier inputs are wholesale missing reads **Medium with a logged tier-input gap** — never a fabricated High or Low (Portfolio's stated stance, adopted here).
  - The exact predicates for `illiquid` and `high event exposure` are **not yet drafted** — the implementation plan's to sweep, alongside the liquidity flag's band constants.

- **Milestone plan validation → horizon**
  - Assign stable milestone ids; resolve prerequisite references; reject cycles and backward ordering; verify evidence claim ids and measurable conditions; accept an inferred expected window only when its cited evidence and derivation support both bounds. A failed timing check makes that milestone **undated** — never fabricated, never a reason to reject a floor-clearing candidate.
  - Every resolution-backed completion condition gets a stable app-assigned **`condition_id`** under the structural-identity rule: across a deep-plan replacement an unchanged machine-evaluable core carries its id and evaluation state through wording / timing / evidence / label revisions; a changed core supersedes the old condition into the audit and starts fresh with a `supersedes` link; a removed condition closes into that record; neither a milestone id nor a model assertion transfers state.
  - **`expected_thesis_realization` → the engine horizon**, in order: **Short** only when a validated payoff-bearing catalyst date, or the payoff milestone's entire expected window, ends < 3 months out; **Long** when the payoff milestone's entire window begins > 12 months out, or the payoff mechanism is multi-year compounding / runway itself (a compounder archetype, or a cleared runway proxy — any archetype can earn it); else **Mid** — a boundary-straddling interval, an undated recognition thesis, a re-rate expected over the next 1–2 reporting periods. The matched branch persists as the **derived basis** (`dated-catalyst` / `milestone-chain` / `recognition` / `multi-year-compounding`).
  - The engine's `business_runway` (Steps 5c / 5f) rides the record as a durability read beside the model's own; neither sets the cell.
  - The model's authored tier / horizon / runway are validated structurally (tier and horizon as enums; runway as a positive year count — fractional allowed — or `unknown`), never against these derivations; where a pair disagrees the divergence is recorded — never a failure, never a re-run. A tier or horizon divergence rides the card's tag; a runway-only divergence stays a recorded read on the expand view and audit, never the tag.
  - **The model arm's tier × horizon → the matrix cell** (the placement ruling, 2026-08-19); the engine's derived pair persists beside it as the baseline.

- **Engine conviction stand-in (computed here, the bearer of every ceiling)**
  - Flag leg — the count of this candidate's disclosed degraded inputs (thin-own-history composite, `archetype_low_confidence`, neutral-midpoint-imputed factors, `freshness-unscorable` floor inputs, a degraded limited-history recast, a fail-soft vector-recall miss, a failed dividend retrieval's zero-leg gap): **0 → High, 1–2 → Medium, ≥ 3 → Low**.
  - Distance leg — the entry gate's signed distance-to-threshold on the binding leg, in return points (the base and double-over-horizon legs are returns; the shape leg reads as base upside − bear downside; the liquidity haircut is already inside the distance): **≥ 5 pts → High, 0 to < 5 → Medium, negative → Low**.
  - Rung = the **min** over the two legs; every matched soft ceiling is then applied to it and persisted as an annotation. It is never shown as the job's conviction.

- **The entry-asymmetry gate — recomputed and enforced here, once per arm**
  - For the **engine arm** over its authoritative target set (research-informed while current, else structured-only); for the **model arm** over its own authored bands; every other leg engine-derived and shared, so the arms differ only in the targets each brings.
  - The shared legs deliberately survived the placement ruling: the required-return scale reads the **engine** tier and H the engine realization basis / runway on both arms — placement is the model's, the admission yardstick is not, so the model can never lower its own bar (ruled 2026-08-19).
  - **Base leg** — the post-haircut twelve-month base-case forward return must clear `DGS2 + 8 pts` (Low) / `+ 16 pts` (Medium) / `+ 30 pts` (High) (decimal ratios; `DGS2` the run-level print).
  - **Shape leg** — bear-case downside may not exceed base-case upside.
  - **Liquidity leg** — the Step-5c banded haircut (0 / −3 / −6 pts), the pre-haircut return recorded beside it.
  - **Double-over-horizon leg** (emerging-economics track only) — required base-case return ≥ `2^(12 ⁄ H) − 1`, **H** in months from the realization's derived basis: `dated-catalyst` → months to the date (floor 3); `milestone-chain` → months to the payoff milestone's **earliest** expected date (floor 3, so a wide interval can't lower the hurdle); `recognition` → ~6; `multi-year-compounding` → `min(business_runway years, 5) × 12`; unknown runway → 36. The strictest leg binds.
  - **Admission is either-arm**: a candidate clearing either arm's gate is admitted, stamped `admitted_by` (`engine-and-model` / `engine-only` / `model-only`), and **both arms' full gate vectors** (per leg: required value, actual value, signed distance; the binding gate id) persist whatever the outcome.
  - A **debut no arm clears** → held out as a `gate-reject` shadow episode. A **carried name no arm clears** → the upside-exhaustion attention warning, never a 5h archive.

- **The forensic / risk gate (app-enforced; the grant above never reaches it)**
  - **Soft triggers** — the four soft forensic flags, a `hype` read *with* a leading-metric anchor, a high-severity unresolved cross-lens contradiction → the engine stand-in's conviction capped at the shared **Medium** ceiling (min over matched rules, order-independent), the rule annotated beside both arms' values; the model's value is never clamped — an exceedance renders beside the annotation.
  - **Hard triggers** — a restatement or auditor change (the Step-5c filing kinds) or **anchorless `hype`** → a **debut is excluded on both arms**; a **carried name is app-forced to `invalidated`** (the archival path), the model's conflicting status persisting as a typed **status-override divergence** `{ model-proposed status, app-forced status, matched hard trigger, trigger source lineage }`. A validated fraud `forensic_event` is **advisory by the 2026-08-24 ruling** — cited attention evidence in scoring, never the automatic hard set. A high-severity contradiction always caps, never excludes.
  - The either-arm grant is scoped to the entry gate alone — a `model-only` admission can never carry a name past a hard trigger or the evidence floor.

- **Carried name outcomes out of this step**
  - Effective status `invalidated` (the 5g proposal, or the app-forced override) → held out of the matrix and flagged for archival; Step 7 moves it (the deep pass is the only archival path).
  - `still-valid` → reconciles into the matrix at Step 7.
  - An abstaining re-read → the inconclusive-refresh rule under the evidence floor below.

- **Limited-history revalidation**
  - Any limited-history evidence is revalidated here exact-equal against Step 5f's accepted / rejected mapping record before it can support the floor or the leading-metric shape; an unmapped predecessor, undisclosed recast, proxy-as-print, or boundary-crossing observation is rejected and logged, and the candidate stands or abstains under the unchanged floor. A valid path carries a degraded-confidence flag where recast observations materially support it; it never changes archetype, tier, required return, ceiling, or gate math.

- **The evidence floor (archetype-aware; binds both arms absolutely)**
  - Floor-bearing for every candidate: a current quote and price history; a **validated, inflecting leading metric** (its absence = story stock, however complete the rest); **source freshness** — the leading metric and the bear-case evidence current per the freshness basis below.
  - The statement floor, archetype-substituted: statements (FMP / SEC) are floor-bearing for a proven-economics archetype (quality / secular compounder, commodity cyclical) — absent, stale, or identity-conflicting ⇒ abstain; for an emerging-economics archetype (early disruptor, pre-profit AI-infra) a defined substitute stands in — hard operating / unit economics (segment revenue, backlog / bookings, net-adds, cohort retention, gross profit) sufficient to underwrite the metric, plus the bear case — explicit and logged, relaxing the *form* of the financials, never the metric.
  - Enriching inputs (estimates / revisions, positioning, peers, the options signal, analyst opinion, the narrative read) lower conviction when absent, recorded as degraded inputs, never floor the candidate — except the `no-admissible-driver` carve-out, which abstains because the gate cannot price a name with no computable target.
  - **Freshness states**, per floor-bearing input: `fresh` / `stale` / `freshness-unscorable` (the source carries no as-of), with named gap reasons — quote / bars current through the latest completed session; structured / filing metrics and statements an observation for the latest *expected* period with a drafted ~45-day filing grace; research-derived metric observations and bear-case evidence within the shared ~4-week window. `freshness-unscorable` is a degraded input, not an abstention, unless the input is itself floor-bearing.
  - A **debut** below the floor abstains `insufficient-evidence` — held out of the matrix, never a low-conviction guess.
  - A **carried live opportunity** whose deep re-read falls below the floor is an **inconclusive refresh, never a turn-away**: it holds its last verdict, conviction, and matrix identity; the engine-only fields the pass could compute refresh under the cheap-sweep rules; a typed refresh gap is recorded; **no** `last_deep_researched_at` stamp, decay restart, milestone-plan replacement, warning clear, or shadow episode — it stays exactly as stale and as flagged as it was, so the rotation slice keeps prioritizing it.

- **Structural validation of the model arm and the typed fields**
  - The model arm is validated for types, enums, and well-formedness only — no value bound applies to its sub-scores, bands, implied-expectations read, or conviction; the authored tier and horizon are enum-validated, and the runway read must be a positive year count (fractional allowed) or `unknown` — a shape constraint with no upper bound, enforced by the schema; an inverted band pair persists annotated (the gate and scoreboard read it as `(min, max)`).
  - There are **no numeric echoes to validate** — engine-owned values are app-stamped onto the record directly.
  - The key falsifiers, the leading metric, and each machine-checkable milestone condition are **class-validated under the Step-3c resolution contract**: a `structured` condition must resolve to an engine-evaluable condition — a series the engine computes, a comparator, a threshold, and persistence semantics (a materiality margin + a consecutive-observation count: drafted 1 for filing-cadence series, 2 for high-frequency ones); a `filing` condition to a standardized field the filing-cadence feeds carry model-free; anything else is downgraded to `research` and logged, never dropped. Only resolution-backed conditions carry machine evaluation state into Step 7.

- **Held-out candidates → the shadow ledger** (typed decision episodes)
  - Every turn-away carries the identity fields — ticker, run date, decision class, surfacing tags, feeder / route lineage, archetype, the entry-stamped sector identity (or `sector-unscorable`) — plus the **model's Step-5g record digest** (its conviction, thesis line, and bear case at refusal), so what the model wanted is preserved and the spread stays sliceable by model conviction.
  - A **`gate-reject`** (no arm cleared the entry gate) additionally carries **both arms' full gate vectors** with per-gate distance-to-threshold — not just the first failing gate, so miss attribution is order-independent.
  - An **`insufficient-evidence` abstention** carries its named floor-gap reasons and freshness states; any independently computable gate leg is optional, never fabricated.
  - A **debut's forensic / `hype` exclusion** is a gate-reject-class episode whose recorded failing gate is its tripped trigger.
  - A debut qualifying for more than one class in the same pass — anchorless `hype` by construction also fails the floor — takes exactly one, by fixed precedence, first match wins: **hard trigger, then evidence floor, then ordinary `gate-reject`** (ruled 2026-08-19); the losing conditions ride the episode's recorded content, never a second episode.
  - **Not** shadow entries: an `invalidated` carry (it goes to the archive, which already tracks its price — a pick is never a turn-away); a carried name's inconclusive re-read.

- **Checkpoint**
  - The surviving candidate is checkpointed here; the run can resume from this point.

- **Output**
  - A survivor with its model-placed cell, both arms' tier / horizon / runway with any recorded divergence, the derived realization basis, the engine stand-in with annotations, the admission provenance with both gate vectors, the validated milestone plan and condition ids, the class-validated falsifiers, the freshness states — or a typed held-out record (shadow episode), or a carried name flagged for archival / holding its prior verdict.
  - The candidate checkpoint.

---

## Step 6 — Rank and assemble new survivors

Every survivor's **cell is already fixed** — the model arm's authored tier × horizon, enum-validated at Step 5h (the placement ruling, 2026-08-19). This step never chooses *which* opportunities appear or where — only their **order within a cell** and which near-duplicates **collapse** — and completeness is enforced by the app, not the model's good behavior.

- **Data retrieved**
  - No external data.

- **Model — per-cell ranking and dedup proposal (one call, thinking)**
  - Exact inputs: every gated opportunity record from Step 5 with its assigned cell; a compact card for every **still-valid carried opportunity without a fresh Step-5h verdict this run** — the cheap-swept names Step 7 will insert, and the inconclusive deep-read carries holding their prior verdict — each with ticker, thesis summary, leading metric, catalyst, and its standing cell (placement is a frozen model-authored field, so every carried card's cell is current), supplied as **collapse targets only**; the house view and investor profile; the count of candidates competing per cell.
  - Returns: per cell, a **conviction ranking** of that cell's gated survivors — ordered on the **model arm's** conviction, the value the card headlines — plus any **dedup-collapse proposals**, each naming the merged-away candidate, the peer it collapses into, and a reason (near-identical thesis / shared leading metric / shared catalyst).
  - The model cannot drop or hide a survivor, cannot move one to another tier or horizon, and can neither rank nor merge the carried cards.

- **App validation — collapse eligibility is typed, never judged**
  - A proposal is accepted only when the pair is equivalent on **typed identity**: both records in the **same assigned cell** *and* sharing at least one of — a hypothesis-lineage node in the opportunity graph, an identical leading-metric series identity, or the same typed catalyst. The free-text reason is recorded color, never the acceptance basis.
  - A proposal failing the predicate **defaults to list-both** — fail-open to redundancy, never omission — rejected and logged, every proposal's predicate inputs and validator result persisting with the collapse audit.
  - **Direction is enforced**: a debut may collapse into a live carry's lifecycle (the carry's record absorbs it), but a **live carry can never be collapsed away** — carries are targets only, since a live pick leaves the matrix solely through deep invalidation; a proposal to collapse a carry is a validation error.
  - Every predicate-validated acceptance is **final here**: the placement ruling froze placement between deep passes, so a collapse target's cell cannot move before Step 7 — the provisional debut-into-cheap-carry acceptance and its Step-7 final-cell re-check are retired with the re-placement leg (2026-08-19).

- **Completeness validation and assembly (app)**
  - Assemble the 3×3 survivor matrix from the model-placed cells and the model's ranking, applying each accepted collapse (recorded with its reason, predicate inputs, validator result, and direction, and written to the **shadow ledger** as a `dedup-substitute` episode so the merged-away peer's forward path is still scored).
  - Every Step-5h survivor must be listed in its cell or recorded as a predicate-validated collapsed peer; a survivor absent from both **fails the run's matrix validation** rather than vanishing.
  - This covers **this run's survivor set only** — the matrix is final after Step 7 inserts the cheap-swept carries and re-validates over the union.
  - No per-cell cap: a cell holds as many or as few ideas as cleared the gates; an empty cell is honest, never padded, and a rich cell is never trimmed.

- **Output**
  - The ranked survivor matrix (this run's set), the validated collapse records, and the completeness check.

---

## Step 7 — Refresh existing ideas and finalize the matrix

The continuity step — an app-layer validator with no model: the rotation picks' and re-surfacers' deep passes already ran in Step 5, so this step reconciles their verdicts, **cheap-sweeps every other live opportunity**, finalizes the matrix over the union, scores past decisions, and reconciles the graph and archive. The dividing rule throughout: **only a deep pass can archive**.

- **Data retrieved**
  - The prior matrix (Step 2) and the run-scoped deep-research set (Step 4).
  - For each cheap-swept name: FMP `quote` and `analyst-estimates` (the swept-union population with Step 3c; one sweep per distinct symbol); dated-EOD bars through the shared price-bar cache; on the **filing-cadence rider** (a new reported period on the swept `earnings` row) the statement-derived rows — statements, `key-metrics` / `ratios`, `financial-scores`, `financial-growth`; the once-per-run FINRA file where a stored condition is short-interest-fed; current `DGS2` / `DGS10` (Step 2).
  - For outcome labels: FMP dated-EOD bars through the window end per maturing picked **or shadow** episode (symbol-deduped; requested until the series covers the window end), and the run-level benchmark bars — `^GSPC` plus the SPDR sector ETFs — the market and sector legs.
  - For the archive: dated-EOD bars per distinct archived symbol (deduped against the swept and label-time populations).

### Reconcile the deep-researched carries

- A carried name that won a deep pass this run (rotation pick or re-surfacer) has its fresh Step-5h verdict: **still-valid** → reconciles into the matrix; **invalidated** (model-judged, or app-forced on a hard trigger — the only path that archives in DTO: a qualitative erosion the cheap read can't see, or an exhausted-upside / continuation-failure finding *confirmed* under fresh research) → moved to the **archive** here; **inconclusive** (the re-read fell below the floor) → reconciles on its prior verdict with its typed refresh gap, treated exactly like a cheap-swept carry, its freshness never advanced so it stays rotation-eligible.
- Every such ticker is in the deep-research set, so the cheap sweep skips it.

### The cheap re-derivation (every other live opportunity; engine-only, never archives)

- **Re-derive the engine arm's targets** — the v2 multiples re-anchored closed-form on the fresh `DGS10` against the stored anchor-window percentiles and drivers; the last deep pass's persisted `research_target_scenario` (direct assumptions are shadow-only, never applied props) re-evaluated over current engine fields **while fresh** (inside the ~4-week window, the external evidence leaves frozen at their cited vintage) and **decayed to the retained structured-only baseline past it**, so a stale prop cannot keep a target inflated. Decay alone raises no warning — it surfaces only as the quiet *Research stale* badge.
- **Re-run the full entry-asymmetry gate on both arms** against the live quote — every recomputable leg live: the refreshed engine tier, fresh `DGS2`, the banded liquidity haircut from live tradability inputs, the emerging leg's **H** re-read from the persisted realization basis (a milestone-chain basis remeasuring months to the validated payoff window's earliest date). The model arm needs **no model call**: its bands are frozen numbers from the last deep pass, re-measured against the live price exactly as the engine's — and unlike the research-informed set they **do not decay** (a dated judgment is not a stale prop; its age rides the *Research stale* badge).
- **Re-derive the engine risk tier** from the refreshed inputs — a gate leg and the card's baseline read (a newly disagreeing pair surfaces through the divergence tag), never the card's cell, which is the model's frozen placement.
- **Evaluate the stored key falsifiers and machine-checkable milestone conditions by class** — `structured` every run, `filing` when a fresh filing landed, `research` held for a deep pass — under the persistence semantics: a streak advances only on a distinct new observation (a new print or filing), keyed by `condition_id`; a filing-cadence condition confirms on its first qualifying breach (count 1), a high-frequency one logs a quiet first-breach note and confirms on the second (count 2); once a deep pass clears a warning, the acknowledging observation id is stored so the warning re-raises only on a breach confirmed against a *later* observation. Milestones: the plan, its timing, and the horizon stay the last deep pass's; the cheap path updates only resolution-backed evaluation state and months-to-date for the gate.
- **Refresh the engine-computed fields** — the since-flagged read (below), the leading-metric-continuation state (a `research`-class anchor holds its last read), the narrative-vs-reality ratio, the forensic computations on the rider.
- **Raise the attention warning — *Consider Deep Audit* — on any of three high-bar triggers**, and otherwise act on nothing:
  - **Upside exhausted** — **neither arm's** re-derived base case still clears the full entry gate (the engine's, structured-only once any prop decayed; the model's frozen bands re-measured); while either arm clears, the divergence is recorded and no warning fires — mirroring either-arm admission.
  - **A tripwire** — a stored `structured` / `filing` falsifier confirming a breach, a machine-checkable milestone missing its validated window, a forensic flag newly tripping, a **continuation-failure signal** (estimate revisions rolling over, a beat-and-raise streak breaking, shipments diverging below sell-through), a since-flagged gain diverging hard from leading-metric continuation, the narrative-vs-reality read crossing into `hype`, the margin of safety compressing near the exhaustion line, or a drawdown breach.
  - **A re-surfacing** — the name re-appeared in discovery without winning a leftover-budget deep pass.
  - Both readings are fail-soft: a *missing* input holds the opportunity on its last verdict, never an escalation; only an affirmative signal **confirmed under a deep pass** ends an opportunity. The model-authored fields — thesis, conviction, the model arm's sub-scores / bands / implied-expectations read, its tier / horizon / runway (the card's placement), bear case, falsifiers, catalyst, milestone plan, archetype, `technology_read`, entry consideration — stay **frozen** between deep passes.

### Final matrix assembly over the union

- Each still-valid carried opportunity **holds its model-placed cell** — placement is a frozen model-authored field, and only a deep pass re-places (the placement ruling, 2026-08-19; the former engine re-tier re-placement is retired). The refreshed engine tier persists beside the placement as the baseline; a newly disagreeing pair surfaces through the divergence tag, never a moved card.
- Insert each carry deterministically into the cell's Step-6 ranking by its **frozen model-arm conviction**, ties by ticker — computed-only, no model re-orders anything.
- **Re-validate completeness over the union**: every Step-5h survivor present in its cell or recorded as a predicate-validated collapsed peer, and every still-valid carry present in its held cell — anything absent fails the run's matrix validation.
- **What-changed attribution**: each status, conviction, or placement (tier / horizon) move a Step-5g record claims as *external* must resolve to a concrete input-delta entry (this run's metrics / positioning / price vs the prior run's stored values), a source-backed research finding, a logged direct / composite target assumption, or a validated milestone claim; an attribution that resolves to nothing is downgraded to a self-correction (or fails schema), so a no-new-facts swing can't be laundered as "the thesis changed".

### Outcome learning

Each DTO run turns the job's own track record into structured feedback. The unit is the **decision episode** — a picked episode per pick (opened at its `became_opportunity_at` run with its entry calibration snapshot, keyed by lifecycle id), and a shadow episode per turn-away — every label engine-computed, never model-judged: whoever keeps score cannot also be a player.

- **Matured-window labels (1 / 3 / 6 / 12 months, each window evaluated independently)** for every prior pick old enough:
  - Forward return, absolute and **vs sector and vs market** — split-adjusted, **price-only as the common basis** (total return carried supplementary where dividends exist); entry reference = the **next session's daily close** after the decision (a consistent, conservative anchor — never the same-day bar the decision couldn't have traded); the sector leg from the episode's **entry-stamped sector identity** (sector label + resolved SPDR benchmark symbol, frozen at entry — never a label-time re-classification; no mapping → `sector-unscorable`, the sector legs excluded, counted and logged).
  - **Maximum drawdown** over the window — the path, not only the endpoint.
  - **Did the leading metric continue?** — per the anchor's re-check class and lifecycle state: a `structured` anchor re-pulls freely, a `filing` anchor reads its filing feed, a `research` anchor advances only where a deep pass produced a dated observation; persisted / stalled / reversed where a path exists; a window with no legal refresh path (a research anchor with no fresh observation, a pick that departed before the window matured) records `leading-metric-unscorable` — excluded from denominators and rollover attribution, counted and logged.
  - **`resolution_mode`** — assigned per matured window by an ordered, first-match-wins tree over the episode's own inputs (the entry snapshot: both arms' entry-vintage targets + methodology + parameter version, the admission provenance and gate vectors, the valuation / revision baseline, the falsifier `condition_id`s and entry states, the initial forensic state, the sector stamp — plus the dated events recorded while live; never a pruned audit, never a later run's values):
    1. **Terminal typing** — an acquisition realizes the label at the final trading price, a bankruptcy scores to zero, an ambiguous delisting is `terminal-unscorable`; a terminal type is assigned **only from an already-recorded corporate-action fact** (the M&A-involvement read, the archive's failing signal, a filing event already in a run record) — a disappearance with no recorded fact resolves conservatively to `terminal-unscorable`, never a guessed class.
    2. **`forensic-materialization`** — a forensic hard flag newly tripped, or a forensic-class falsifier confirmed, within the window; price direction irrelevant.
    3. **`leading-metric-rollover`** — the window's continuation state is reversed (a price winner with a reversed anchor lands here deliberately: a lucky win is a gate miss).
    4. **`multiple-unwind`** — market-relative return ≤ **−10%** (the loser bar) *and* multiple contraction ≥ **60%** of the market-relative down-move with estimate revisions within a **±5%** noise band (the downside mirror of the 70% hype decomposition).
    5. **`market-beta`** — absolute return ≤ −10% while within **±5 pts** of the sector benchmark's return, with no stored falsifier confirmed in the window (the name fell with its group, thesis intact); `sector-unscorable` leaves this branch unevaluable.
    6. **`thesis-played-out`** — a dated-EOD daily close reached the **engine arm's entry-vintage base-case target** during the window with the metric persisted — an internal-calibration label, not a matrix status (hitting a target is not an exit).
    7. **`no-dominant-mode`** — the explicit residual, **guarded on input completeness**: it claims only a window whose still-relevant branches were all evaluable; a window with any still-relevant branch blocked by an unavailable input (a live `leading-metric-unscorable` window blocks 3 and 6; `sector-unscorable` blocks 5; the post-departure window the limiting case) records **`resolution-unscorable`** instead — excluded like its terminal / metric counterparts, counted and logged. The explanation is templated from the matched branch.
  - Price labels populate only when the refreshed bars **cover the window end**; a failed refresh leaves the label **pending with a price-coverage gap**, bounded by the shared price-coverage grace (drafted ~3 months past the window end), past which it closes as the typed `price-coverage-unscorable` label — the same bound turning a transiently stale series into a genuine disappearance for the terminal contract.
  - The labels record onto the pick's durable episode (independent of matrix presence, archive retention, and run retention) and **score both arms identically** off the stored entry-vintage bands — the shared interval scorer, the fixed twelve-month bands scoring at the matured **12-month window only**, the 1 / 3 / 6-month labels serving the cohort and resolution reads (ruled 2026-08-19): per-arm target calibration, a head-to-head over the paired population only, and the slice by **admission provenance** — `model-only` admissions measured against the `gate-reject` shadow population they were drawn from and against their own admitting hurdle, `engine-only` as the symmetric read on the model's refusals; each arm's **conviction is recorded unscored** behind the ≥ 30-unique-issuer bar. The single-valued `resolution_mode` keys on the engine arm's entry-vintage target. A debut pick's episode **opens in this pass** with its entry snapshot; each later live pass appends its dated events.
- **The shadow scorecard** — the same price-derived labels (return vs sector / market, drawdown — never a leading-metric re-pull, so no research or model spend) over the persisted shadow ledger: Step-5h held-outs, Step-6 dedup-collapsed peers, and the graph's retired / still-unpromoted watchlist nodes — never `departed` pick tombstones (their outcomes live on the picked episodes). Each entry anchors on the run that turned it away (a deferral on its first-surfaced date; a retirement on both that date and its retirement date). The labels reduce **per decision class, never pooled** — picks vs **gate rejects** is the headline **picked-vs-rejected spread** (unique-issuer counted; sliceable by feeder, route, archetype, and gate); deferrals, abstentions, dedup substitutes, and retirements read separately — and a **false-negative flag** on any turned-away name whose market-relative return exceeds **+15% at 6 months** or **+25% at 12 months**, tradability-discounted through the haircut band (the severe band exempt entirely). Calibration-only: a flagged false negative tunes the gates, never re-promotes or re-surfaces the name.
- **The continuous since-flagged read** — refreshed for every carried-forward opportunity (live from its first subsequent run; a debut carries none yet): its price-derived parts — running return since `became_opportunity_at` (absolute, vs sector, vs market) and maximum drawdown — reconstructed from the daily-bar cache from the first close after `became_opportunity_at` to the latest cached close, and its leading-metric-continuation state read from the metric's own re-check-class path (a structured re-pull, never price bars) — one engine primitive with three readers: the discrete horizons feed calibration, the continuous read feeds the matrix card, and (for a carried name re-validated this run) it fed Step 5g as cap-only context. The matured labels attach to it as they elapse.
- Calibration **proposes, never applies**: no proposal until ≥ 30 unique issuers with matured windows, and then with effect size and an issuer-clustered interval for the user's review; the labels are recorded and audited as the job's honest scorecard meanwhile.

### Graph and archive reconciliation (same pass)

- **Opportunity graph** — this run's picks link to their matrix entry (`picked`); worthy-but-unpicked names are added or refreshed as `watchlist` nodes; nodes whose falsifiers tripped or whose carry horizon elapsed are `retired` (Step 3c); a deeply invalidated pick's node moves to **`departed`** in the same pass as its archival — a terminal tombstone visible in route context as a dead thesis, never a feeder, never re-promotable in place, excluded from shadow scoring; a genuine re-entry opens a new node under a new lifecycle. Departed tombstones prune on the archive's retention.
- **Archive** — an `invalidated` opportunity is moved to the archive (the most recent **100**, oldest evicted first) as a **frozen verdict snapshot** — thesis, archetype, leading metric, catalyst, final milestone plan, bear case, `became_opportunity_at`, the departure date, the archive trigger (`failed-reevaluation` — the single trigger, always a deep pass) with the specific failing signal that retired it, admission provenance, conviction at exit (the model arm's value with the engine stand-in beside it), the stamped sector identity, and any status-override divergence. Afterward **only the price is tracked** — each run refreshes its since-flagged return (absolute, vs sector / market) and drawdown from the bar cache; no leading-metric continuation, no research, no model call; a still-maturing episode freezes its metric state at the last live refresh. There is no "target met" exit; staleness alone never archives.
- **Re-entry is a fresh start**: a later run that independently re-discovers an archived ticker removes it from the archive and it enters as a new opportunity with a new `became_opportunity_at`; none of the archived record influences the new one (the old episode keeps maturing under its own lifecycle). In the matrix and the archive a ticker is in exactly one state — live, departed, or neither (a re-entry vacates its archived slot); the graph is lifecycle-scoped, so the old node's `departed` tombstone remains beside the re-entry's new node. The archive never promotes itself.

- **Model**
  - None.

- **Output**
  - The final matrix over the union (held-cell carries, this run's survivors, the validated collapses), validated complete.
  - Attention warnings raised, archive moves, the updated opportunity graph and coverage state.
  - This run's opened picked episodes, newly matured labels (picked and shadow), the since-flagged reads, the scorecard reads, and the what-changed attribution.

---

## Step 8 — Mark opportunities you already own

- **Data retrieved**
  - The holdings list, **pulled fresh from Schwab at this step** — the run's sole holdings consumer, so the tags are current rather than hours stale after a long run.
  - On a failed pull: the most recent persisted holdings snapshot, the tags labeled with its captured-at date; with no snapshot, the owned tags are omitted as a typed gap. Never a failed run.

- **Logic**
  - Flag each matrix opportunity owned / not-owned.
  - Runs *after* discovery, selection, and continuity, and reads only the holdings list — never the Portfolio Analysis memory partition — so holdings never influence what is found or chosen; the job stays independent of the account.

- **Model**
  - None.

- **Output**
  - Display-only ownership tags.

---

## Step 9 — Save everything

- **Data stored — the six persisted structures**
  - **The run record** — the 3×3 matrix (every opportunity's record: thesis, detection mode, archetype, leading metric + its stored series, typed catalyst, validated milestone plan with condition ids and evaluation states, both arms' risk tier / horizon / runway (the model's placing the card; the engine's derived pair + realization basis and runway inputs beside it), both target sets + the bridge tree and delta, both arms' conviction / sub-scores / bands / implied-expectations reads, narrative-vs-reality read, bear case, class-typed falsifiers with evaluation state, hypothesis + seed lineage, any `technology_read`, entry consideration, risk / forensic flags + any `forensic_event`, `admitted_by` + both gate vectors, status, attention-warning state + trigger, `became_opportunity_at`, `last_deep_researched_at`, the stamped sector identity (the live record's copy, refreshed at each deep pass — distinct from the episode's frozen entry stamp), since-flagged read) plus the **run audit record** — sources and retrieval timestamps with their source-quality annotations, the discovery and screening inputs (which screens / routes / themes surfaced each candidate, the coverage-debt snapshot and inserted route, attempted / completed units, every refresh-lane node considered / selected / skipped with its result), the distilled findings, the typed claims and accepted / rejected rules and bridge legs, limited-history mapping decisions, the engine calculations with the target methodology (the job-time `quote`, the anchor-window percentiles and drivers the cheap paths re-anchor against), the engine stand-in's matched ceiling annotations and any hard-trigger record, the recorded divergences (archetype overturn, the tier / horizon / runway divergence, status override), the run-level **band and conviction divergence rates** (band: the model's base differs from the engine's authoritative base by > 10% or the base bands don't overlap; conviction: different rungs; pooled over the last 5 pick-producing DTO runs), any `self_assessment`, the input delta and what-changed attribution, the dedup-collapse decisions (predicate inputs, result, direction), the outcome labels and since-flagged reads, the shadow scorecard, each pass's reused-vs-fresh document split, model ids and quantizations, prompt / schema / parameter versions, and degraded-input flags.
  - **The opportunity graph** — hypotheses with value-chain traces; watchlist nodes with lineage, seed lineage, score, metric + class, falsifiers, latest gap, `last_successful_research_refresh_at`, refresh-attempt state, status, timestamps; event-impact nodes with `technology_read` + side.
  - **The discovery-coverage ledger** — per route class and coverage subject: first seen, last attempted, last successfully completed, last route id, completion / gap state, computed debt.
  - **The archive** — the most recent 100 departed picks as frozen snapshots (Step 7); since-flagged numbers recomputed, never stored.
  - **The shadow ledger** — typed turn-away episodes (gate-reject / abstention / deferral / dedup-substitute / retired-hypothesis, a capacity eviction a retirement carrying `capacity-evicted`), each with its identity fields, the Step-5g digest on the post-5g classes (a pre-5g deferral or retirement carries none — its model-side context is the watchlist node's persisted score and lineage; ruled 2026-08-19), per-class content, anchor date(s), sector stamp; bounded by its retention cap, matured entries frozen into a compact archive (drafted 5,000 rows).
  - **The picked-episode store** — one immutable episode per lifecycle: the entry calibration snapshot, the dated live events, the matured labels; independent of matrix presence, archive retention, and run retention; matured episodes frozen under their own cap (drafted 5,000).
  - Shared stores touched: the price-bar cache, the document research cache, the factor-distribution store (one current observation per issuer per factor), the web-research source state.

- **Retention**
  - The last N Trade Opportunities runs; the archive at 100; `departed` tombstones on the archive's retention; the watchlist under its cap; the shadow ledger and picked matured archives under theirs.

- **Embedding model (DTO and Deep Audit only; a Quick Audit never invokes it)**
  - Each opportunity's record summary embedded individually — a `summary`-kind row stamped with the pick's lifecycle id (the rows Step 5b's recall reads); a Deep Audit embeds the touched opportunities' summaries only.
  - **On DTO only**: each outcome label newly matured since the prior DTO pass (an ATO-refreshed label included, so each is written once) and each new shadow false-negative flag — durable `learning`-kind rows (for a false negative: the name, the failing gate, the model's conviction at refusal, the return it posted), consumed only by the calibration pass, never a dossier input.
  - Vectors land in the Trade Opportunities partition only; a failed or invalid vector costs the memory row, never the persisted run.

- **Output**
  - A durable run and audit record, the carried stores for the next run's Step 2, and the searchable Trade Opportunities memory.

---

## Step 10 — Display the result

Display is a pure read of the persisted matrix; no model runs.

- **Data retrieved**
  - The persisted run (matrix, archive, badges' inputs).
  - The per-ticker daily-bar cache — the since-flagged read's **price-derived parts** (return vs sector / market, drawdown) and the % upside to target are **re-derived at render** from the latest cached close (the cache refreshes a symbol lazily, after 8 PM ET and at most once per 24 hours, fail-soft), so the card is current between runs and opening the page costs no fetch **once the day's bar is cached**; the leading-metric-continuation state needs a structured re-pull, so it refreshes only when a job runs; the live `quote` is a job-time input only, never a render dependency.

- **The matrix (default, canonical view)**
  - Three risk sections × three horizons — every card placed by the **model arm's** authored tier × horizon (the placement ruling, 2026-08-19); each card: archetype, directional thesis, leading metric, catalyst, **the model arm's conviction and forward outlook headlining** (base-case target and bear / bull range over the twelve-month window), narrative-vs-reality read, entry consideration, bear case, status, `became_opportunity_at`, `last_deep_researched_at`, owned / not-owned, and — for a carried idea — the since-flagged performance (return since it became an opportunity, vs sector / market, a compact running curve, maximum drawdown).
  - **Two arms by progressive disclosure**: a quiet **divergence tag** where the arms materially disagree (conviction rung, band overlap, or a tier / horizon divergence — the engine's derived pair against the model's placement); the paired engine / model view — tier, horizon, and runway included — on card expand; the `admitted_by` tag on both single-arm states — `engine-only` (the headline model target did *not* itself clear the gate; the engine admitted it) and `model-only` (the headline admitted it; the baseline dissented); consensus cards untagged.
  - Lifecycle affordances per card: the selection control (plus select-all / deselect-all), an amber actionable **Consider Deep Audit** badge when the attention warning is set, a green **Deep-researched today** badge when `last_deep_researched_at` is the current local-timezone day, a quiet **Research stale** badge when the last deep pass is older than ~4 weeks (computed at render; never amber).
  - Empty cells shown as empty.

- **List view (toggle)**
  - All nine cells flattened into one sortable grid, each row keeping its placed (model-arm) risk tier and horizon, selection control, badges, and engine-target / model-target / divergence columns; sort keys: **forward % upside to target** (default, descending — the model arm's target against the cached close; the engine's sortable in its own column) or **realized since-flagged return** (a debut sorts last). Display-only reordering.

- **Archived opportunities (separate view)**
  - Each departed pick's frozen record, departure date, and live since-flagged return — **no forward prediction**; sortable by since-flagged return or drawdown, default departure date descending.

- **Controls**
  - **Discover** (DTO) and a selection-gated **Audit** button forking to Quick Audit / Deep Audit (a large Deep-Audit selection confirms first). While a job runs the run tracker replaces the page; a run is never a report — a cancel or failure removes nothing.

- **Model**
  - None.

---

# ATO: Audit selected opportunities

The user-directed maintenance job. No discovery: the user selects one or more **existing** matrix opportunities (per-card selection, select-all / deselect-all — from the matrix or the List view) and chooses **Quick Audit** or **Deep Audit**; the job re-evaluates exactly that selection by reusing DTO's stages, and re-renders. It holds the same single global run slot and clears the same **presence** gate; the fork differs only at run-gate **connectivity**. ATO's depth is bounded by the selection, never the DTO deep-research budget.

## Gate and load (Steps 1–2, reused)

- **Gate**
  - Presence is uniform — local models configured, Schwab connected (a presence precondition even though Quick Audit's analytical pass reads no Schwab data — its one Schwab touch is the fail-soft, display-only Step-8 cross-reference), FMP / FRED present.
  - **Deep Audit** clears the full Step-1 gate (daemon reachable + roster pulled — it makes model calls) and triggers the SearXNG pre-run notice when the instance is down (its selected names get thinner evidence; *not recommended* without Tavily).
  - **Quick Audit** is engine-only, so it **skips the daemon-connectivity check** and runs with the daemon configured-but-down; no web research, so no pre-run notice.

- **Load**
  - Deep Audit: the Step-2 load as in DTO (house view, profile, run-level FRED / FMP-commodity / CFTC / CBOE, the prior matrix and opportunity graph).
  - Quick Audit: only the subset its engine pass needs — the FRED rate anchors (`DGS2` for the entry threshold, `DGS10` for the v2 re-anchor) under the **quick-path cached-print rule** (a failed FRED retrieval fail-softs to the last cached print with its as-of date, eligible only within the shared rate-cache max age; older, or no cache, types the rate-dependent reads `unknown` rather than computing off a stale anchor), and **conditionally** the once-per-run FINRA consolidated file when a selected name carries a short-interest-fed `structured` condition (a failed file fetch types those conditions `unknown`).
  - The selection is the work list; no discovery feeders run.

## Quick Audit

`Selected names → Refresh numbers → Re-run both arms' gates → Check warnings → Save`

- **Data retrieved (per selected name)**
  - FMP `quote` and `analyst-estimates`; dated-EOD bars through the shared price-bar cache; on the filing-cadence rider, the statement-derived rows; the conditional FINRA lookup. No Schwab data in the analytical pass (the options signal is held out of the grade and is not an input) — the run's one Schwab touch is the closing Step-8 holdings cross-reference, fail-soft and display-only.

- **Logic — the same cheap re-derivation Step 7 applies to the DTO matrix tail**
  - Re-derive the engine arm's scenario targets — the v2 multiples re-anchored closed-form on the fresh `DGS10` against the stored anchor-window percentiles and drivers; a still-fresh `research_target_scenario` re-evaluated over current engine fields, a stale one decayed to the structured-only baseline (direct assumptions are shadow-only and never applied).
  - Re-run the **full entry-asymmetry gate on both arms** — the engine's live targets and the model's frozen, non-decaying bands against the current price; re-derive the engine risk tier (a gate leg and the card's baseline read — never the card's cell, which is the model's frozen placement).
  - Evaluate the stored `structured` / `filing` key falsifiers and milestone completion conditions under the persistence semantics (a short-interest condition reading the conditional file); refresh the since-flagged read and the leading-metric-continuation state where a structured path exists.
  - Raise or retain the **attention warning** on an upside-exhaustion (neither arm clears) or tripwire reading — never an archive.

- **Model**
  - None — it cannot fail on research, and it runs while the model server is offline.

- **Cannot**
  - Rewrite the thesis, conviction, or any model-authored field (they stay frozen).
  - Move a card between cells — placement is a model-authored field.
  - Perform new research; stamp `last_deep_researched_at`; clear a warning.
  - Archive an opportunity.
  - It never checkpoints — engine-only and fast, it simply re-runs.

- **Persist and render**
  - Step 9's deterministic persistence leg only — the embedder is never invoked; the Step-8 holdings cross-reference re-runs over the touched names; the page re-renders.
  - The engine-computable outcome labels of the touched names refresh and record onto their picked episodes in the same leg — labels are engine-computed, so both audit modes refresh them for the names they touch (`research`-class metric state alone stays frozen until a deep pass); a label matured here embeds durably only at the next DTO pass (Step 9).

## Deep Audit

`Selected names → Full Step-5 loop → Reconcile → Save → Display`

- **Data retrieved**
  - Everything a Step-5 candidate gets — the full per-symbol surface and fresh web research (the shared loop, SearXNG first, Tavily fallback).

- **Logic**
  - Each selected name runs the Step-5 per-candidate loop as a **carried-forward candidate**: 5a affirm-or-overturn on the prior archetype → 5b dossier with the prior record, its `continuity_weight` (from the age of `last_deep_researched_at`), and the own-lifecycle retrospective → 5c engine → 5d research → 5e distillation → 5f refinement → 5g scoring (with the since-flagged read, cap-only) → 5h validation.
  - A **large selection prompts a confirmation first** — the loop runs per name and can be long.
  - Resume holds under the same per-candidate checkpoint contract over its smaller pinned set (the selected names and the run's shared context).

- **Model**
  - The full archetype, research, distillation, and scoring calls, per selected name; the embedder for the touched summaries at persist.

- **Can — contingent on a floor-clearing verdict whose floor-bearing freshness reads were met from currently searched results (the document cache never substitutes for a live search)**
  - Rewrite the model-authored fields — thesis, the model arm's reads, its tier / horizon / runway (re-placing the card), bear case, falsifiers, catalyst, entry consideration.
  - Write fresh direct assumptions / `research_target_scenario` and a validated `thesis_milestone_plan` (restarting their freshness window).
  - Stamp `last_deep_researched_at` (the green *Deep-researched today* badge) and **clear the attention warning**.
  - Judge the name `invalidated` → the **archive** — model-judged, or app-forced on a validated hard trigger with the status-override divergence; the archival write atomically takes the touched picked node to `departed` under the same lifecycle id. It is the **only** ATO path that can archive.
  - A selected name whose re-read abstains `insufficient-evidence` holds its last verdict under the carried-name rule — no stamp, no decay restart, no warning clear.

- **Does not**
  - Run discovery, add watchlist nodes, or run the re-check retirements — the touched picked nodes' lifecycle transition is the graph's only mutation.
  - Modify unrelated opportunities.

- **Continuity, persist, and render (Steps 7–10, reduced)**
  - The audited records reconcile into the matrix under the same what-changed attribution discipline; an `invalidated` result moves to the archive; the labels the pass refreshes record onto the touched names' episodes (the durable `learning`-kind embed of a matured label stays the next DTO pass's side-effect, written once).
  - The Step-8 holdings cross-reference re-runs over the touched names; the run + audit record persist with the touched summaries embedded; the page re-renders.

---

# The most important safety rules

- The engine calculates every fact and its arithmetic — prices, statements, positioning, short interest, the options signal, Altman Z, Piotroski, the composite's normalization — once; a second version of a fact is fabrication, not judgment.
- The model arm never binds or alters an engine value: engine-owned values are app-stamped directly and never echoed through the model; the model's own sub-scores, bands, implied-expectations read, conviction, and tier / horizon / runway are structurally validated only and persisted exactly as authored. Outcome scoring is narrower than persistence: the entry-vintage target bands are the one read graded head-to-head against the engine; conviction, sub-scores, and the other authored reads are recorded unscored until the calibration tier settles a rule.
- Every ceiling binds the engine arm's conviction stand-in and annotates the model's exceedance; nothing clamps the model's value.
- Admission is either-arm — scoped to the entry-asymmetry gate alone, stamped `admitted_by`, both gate vectors persisted; the evidence floor, the hard forensic triggers, and anchorless `hype` bind both arms absolutely.
- Placement is the model's: the card sits at the model arm's authored tier × horizon, frozen between deep passes, with the engine's rule-derived tier and milestone-derived horizon beside it as the disclosed baseline — shown, never the placement (the placement ruling, 2026-08-19). The admission yardstick is not: the gate's required-return scale, haircut, and H read the engine legs on both arms, so the model never sets its own bar.
- The scoreboard is single-valued: outcome labels, `resolution_mode`, realized return, and drawdown stay engine-computed — whoever keeps score cannot also be a player.
- A candidate with no inflecting, dated, third-party leading metric is a story stock and never enters the matrix; missing floor-bearing evidence causes abstention, not a guessed verdict.
- Fast checks may warn; only a deep re-evaluation — confirmed under fresh, currently searched research — may rewrite a model-authored field or remove an opportunity. Missing data never causes removal; staleness alone never archives; there is no "target met" exit.
- Only a debut can be excluded at the entry gate or the evidence floor; a carried name failing the entry gate takes a warning, an inconclusive re-read holds its last verdict, and it leaves only when a deep pass judges it invalidated — model-judged, or app-forced by a validated hard trigger (the one app-forced removal).
- Price never raises conviction: the since-flagged read is cap-only, the price-action confirmer adjusts but never substitutes for the anchor, and the archive never promotes itself — re-entry is a fresh start.
- Every name the funnel affirmatively judges and turns away is still tracked (the shadow ledger; an unworthy deferral carries no state), and what it teaches only ever proposes a calibration change — never applies one.
- Holdings never influence what is found or chosen; the owned tag is display-only, and the job never places an order.
