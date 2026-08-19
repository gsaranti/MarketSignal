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

- **Candidate**
  - A company being investigated.
  - Not yet approved as an opportunity.

- **Opportunity**
  - A candidate that passed every required check and sits in the matrix.

- **Debut**
  - A candidate with no live opportunity record — new to the matrix this run.
  - Only a debut can be held out at a gate (a carry takes a warning instead).

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
  - Its absence makes a candidate a story stock — rejected.

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
  - The deterministic side of every judgment field: archetype-weighted sub-scores, the v2 scenario targets (structured-only and research-informed), the implied-expectations range, and a mechanical conviction stand-in.
  - Always obeys its own caps and rules; nothing the model returns alters it.

- **Model arm (model view)**
  - The reasoner’s own read of the same fields — sub-scores, bear / base / bull bands, implied-expectations read, conviction — authored with the engine’s values in view as evidence.
  - Structurally validated only; never checked against the engine’s numbers; scored against the engine baseline by the outcome scoreboard.

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
  - How risky the company appears — High, Medium, or Low.
  - Rule-derived from measurable inputs; sets the matrix row and the required return.

- **Horizon**
  - When the thesis is expected to pay — Short, Mid, or Long.
  - Rule-derived from the validated payoff milestone or catalyst; sets the matrix column.

- **Gate**
  - A mandatory rule.
  - Failure prevents a debut from entering the matrix; a carried name instead takes a warning.

- **Evidence floor**
  - The minimum evidence a candidate needs before any judgment is written: price + history, a validated leading metric, current sources, and statements (or an archetype-defined operating substitute).
  - Below it the candidate abstains as `insufficient-evidence`; binds both arms absolutely.

- **Entry gate (entry asymmetry)**
  - The required forward return a name must clear: `DGS2` + 8 / 16 / 30 points by risk tier, plus the shape, liquidity, and (emerging track) double-over-horizon legs.
  - Run once per arm; re-run on every cheap pass.

- **Cheap re-derivation**
  - Fast, model-free refresh of the engine-computed fields and both arms’ gates.
  - Can raise a warning; cannot re-rate or remove an opportunity.

- **Deep re-evaluation**
  - The full per-candidate loop (Steps 5a–5h) on an existing opportunity.
  - The only process allowed to rewrite the model-authored fields or archive.

- **Attention warning**
  - Amber *Consider Deep Audit* flag the cheap re-derivation raises on a tripwire, exhausted upside, or a re-surfacing.
  - Never changes the verdict; cleared by the next deep pass.

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
  - `key-metrics`, `ratios` (+ TTM), `financial-scores` (Altman Z, Piotroski), `owner-earnings`, `enterprise-values`, `discounted-cash-flow`, `financial-growth` (multi-year per-share CAGRs).
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
  - On further overflow the sub-distillation cap fail-softs the lowest-priority whole passes to a recorded gap.

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

- **Checks**
  - No other Market Signal job is running.
  - Local reasoning model is available.
  - Embedding model is available.
  - Schwab connection is configured.
  - FMP and FRED credentials exist.

- **Model**
  - None.

- **Output**
  - Job starts.
  - Or the app explains what is missing.

---

## Step 2 — Load shared market context

- **Data retrieved**
  - Latest Market Signal house view.
  - Fixed investor-profile preset.
  - Previous opportunity matrix.
  - Previous opportunity graph.
  - Discovery coverage ledger.
  - `DGS2` and `DGS10` Treasury rates from FRED.
  - Historical `DGS10` data for valuation calculations.
  - Commodity prices from FRED and FMP.
  - Commodity positioning from CFTC.
  - Broad put/call data from CBOE.
  - Economic-release dates from FRED.

- **Logic**
  - Ignore the house view if older than one week.
  - Use `DGS10` in price-target calculations.
  - Use `DGS2` in minimum-return requirements.
  - Load previous ideas for continuity.
  - Treat all rates and returns as decimal values internally.

- **Model**
  - None.

- **Output**
  - One shared context packet.
  - Reused for every candidate.

---

## Step 3 — Discover candidates

Three discovery feeders run.

### Step 3a — Structured market screens

- **Data retrieved**
  - FMP company screener.
  - FMP insider-buy feed.
  - FMP earnings and event calendars.
  - FMP merger, IPO, filing, and market-mover feeds.
  - FINRA short-interest file.
  - Commodity prices already loaded in Step 2.

- **Logic**
  - Keep active US-listed equities.
  - Apply minimum price, volume, and market-cap rules.
  - Tag companies by size, sector, and industry.
  - Find:
    - Insider-buy clusters.
    - Short-interest extremes.
    - Recent positive earnings surprises.
    - New corporate events.
    - Commodity-price turns.
  - Standardize earnings surprises against the company’s history.
  - Do not perform full financial scoring yet.

- **Model**
  - None.

- **Output**
  - Broad candidate list.
  - Each candidate carries its discovery signal.

---

### Step 3b — Model-led hypothesis discovery

- **Data retrieved**
  - House view.
  - Previous opportunity graph.
  - FMP news and articles.
  - Economic-release schedule.
  - Current web sources through SearXNG.

- **Model — planning call**
  - Chooses a limited set of research routes.
  - Examples:
    - Supply-chain changes.
    - Regulation.
    - Technical bottlenecks.
    - Customer spending.
    - Industry history.
    - Major technology events.
  - One mandatory route ignores previous ideas.
  - Purpose: prevent the job from becoming anchored.

- **Coverage rotation**
  - App finds the stalest route type and coverage subject.
  - Coverage subjects include broad industries and active themes.
  - Reserves the next route slot after the outside-view route.
  - Uses calendar age, not number of job runs.
  - A completed search counts even when it finds no opportunity.
  - A failed search does not clear the coverage debt.
  - Cannot force a hypothesis or candidate.

- **Research loop**
  - For each route:
    - Split the route into focused topics.
    - Start a clean model conversation for each topic.
    - Search and fetch sources.
    - Allow up to three passes per topic.
    - Stop when the time or fetch budget is reached.

- **Model — hypothesis work**
  - Determines:
    - What is changing.
    - Why it matters economically.
    - Which part of the value chain benefits.
    - Who has pricing power.
    - Which public companies are exposed.
    - Which leading metric would prove the idea.
    - What could invalidate it.
    - Why the idea may already be priced in.

- **App validation**
  - Verify each ticker exists.
  - Verify it is tradable and US-listed.
  - Check the hypothesis score against fixed thresholds.
  - Drop unsupported technology-event claims.
  - Keep weaker but credible ideas on the watchlist.

- **Large-route handling**
  - Split large research routes into smaller pieces.
  - Distill each piece.
  - Combine them into hypothesis cards.

- **Output**
  - Promoted hypothesis cards with candidate names.
  - Watchlist hypotheses.
  - Sources and discovery lineage.

---

### Step 3c — Recheck the old watchlist

- **Data retrieved**
  - Stored watchlist.
  - FMP metrics.
  - Filing data.
  - FINRA short interest when needed.

- **Logic**
  - Recheck each watchlist metric by class:
    - `structured`: every run.
    - `filing`: when a new filing appears.
    - `research`: when discovery finds it again or the targeted refresh lane selects it.
  - Select a small number of `research` metrics for a targeted current-search refresh.
  - Starting cap:
    - One watchlist name per DTO run.
  - Refresh priority:
    - New filing, contract, or material event.
    - Approaching catalyst or thesis milestone.
    - Near-promotion or near-gate candidate.
    - Higher hypothesis score.
    - Oldest successful research refresh.
  - Search only for the stored metric, falsifier, or milestone.
  - Do not rewrite the thesis, targets, conviction, or opportunity record.
  - If the metric improves:
    - Promote the name into the candidate list.
  - If the thesis fails or expires:
    - Retire it.
  - Otherwise:
    - Keep watching it.

- **Capacity logic**
  - Watchlist has a maximum size.
  - Lowest-scoring names leave first.
  - Evicted names still receive shadow episodes.

- **Model**
  - None for structured and filing checks.
  - Targeted reasoning and web research for selected `research` metrics only.

- **Output**
  - Promoted watchlist candidates.
  - Updated watchlist and retired nodes.
  - Targeted-refresh audit record.

---

## Step 4 — Consolidate and allocate research slots

- **Data retrieved**
  - No major new data.

- **Logic**
  - Combine all three discovery feeders.
  - Remove duplicate tickers.
  - Remove funds and non-equities.
  - Recheck basic tradability.
  - Preserve every discovery reason.

- **Research-budget allocation**
  - First: existing opportunities needing maintenance.
  - Second: new candidates.
  - Third: existing opportunities that resurfaced.

- **Maintenance priority**
  - Warning-bearing opportunities.
  - Near-term catalysts.
  - Names close to failing the return gate.
  - Oldest deep research.

- **New-name diversity rules**
  - Protect mid- and small-cap representation.
  - Limit mega-cap concentration.
  - Limit one feeder, archetype, sector, or theme from dominating.

- **Deferred names**
  - Not treated as rejected.
  - Worthy names go to the watchlist.

- **Model**
  - None.

- **Output**
  - Final list receiving expensive Step-5 validation.
  - Record of which existing names receive a deep pass.

---

## Step 5 — Deep validation loop

The following sequence runs once for every selected candidate.

Each candidate is checkpointed separately.

### Step 5a — Classify the archetype

- **Data retrieved**
  - FMP company profile.
  - Income statements.
  - Ratios and key metrics.
  - Segment information.
  - Historical financial patterns.

- **Logic**
  - Calculate classification features:
    - Sector and industry.
    - Margin structure.
    - Recurring revenue.
    - Cyclicality.
    - Discovery signals.

- **Model**
  - Confirms one archetype:
    - Secular compounder.
    - AI infrastructure.
    - Commodity cyclical.
    - Category disruptor.
    - Quality compounder.

- **Validation**
  - Exactly one archetype must result.
  - Failed calls use a deterministic fallback.
  - Existing names cannot change archetype without changed evidence.

- **Output**
  - Authoritative archetype.
  - Confidence and rationale.

---

### Step 5b — Build the candidate dossier

- **Data retrieved**
  - FMP:
    - Statements and ratios.
    - Estimates and revisions.
    - Earnings surprises.
    - Insider and congressional activity.
    - Activist filings.
    - Peers, float, news, and corporate events.
  - SEC EDGAR filings.
  - FINRA short interest.
  - Deep FMP price history.
  - Live FMP quote.
  - Schwab option chain.
  - Relevant prior analysis from local memory.
  - For an eligible recent listing or separation:
    - S-1 or Form 10 history.
    - Predecessor or carved-out business disclosures.
    - Contracts and dated operating milestones.
    - Customer and supplier evidence.

- **Logic**
  - Cross-check FMP data against SEC filings.
  - Assemble one evidence packet.
  - Keep prior analysis within the same opportunity lifecycle.
  - Give older prior research less influence.
  - App determines limited-history eligibility from listing and corporate-identity facts.
  - Missing provider data cannot create limited-history eligibility.

- **Embedding model**
  - Converts the candidate query into a vector.
  - Retrieves similar prior analysis.
  - Performs no reasoning.

- **Output**
  - Complete candidate dossier.

---

### Step 5c — Calculate the financial picture

- **Data retrieved**
  - Uses the dossier.
  - No model or web research.

- **Calculations**
  - Value, quality, momentum, volatility, and revision composite.
  - Return on invested capital versus capital cost.
  - Owner earnings and reinvestment runway.
  - Leading-metric trend.
  - Standardized earnings surprises.
  - Insider, short-interest, congressional, and options signals.
  - Relative price strength.
  - Liquidity and days-to-cover.
  - Bear, base, and bull price targets.
  - Growth already implied by the current price.
  - Price movement versus actual business improvement.
  - Accounting and governance warnings.
  - Performance since first becoming an opportunity.

- **Forensic events**
  - Restatement: SEC Item 4.02.
  - Auditor change: SEC Item 4.01.
  - Fraud: primary-source research only.
  - Missing evidence becomes `unknown`, not “clear.”

- **Model**
  - None.

- **Output**
  - Deterministic financial analysis.
  - Provisional price targets.
  - Risk and forensic flags.

---

### Step 5d — Research the company

- **Data retrieved**
  - Current web sources.
  - SearXNG first.
  - Tavily if SearXNG fails.
  - Company filings and disclosures where relevant.

- **Research topics**
  - Validate the leading metric.
  - Test theme and economic fit.
  - Assess management and market narrative.
  - Compare with past winners and failures.
  - Seek outside corroboration.
  - Build the mandatory bear case.
  - When limited-history eligible:
    - Confirm source and target company identities.
    - Confirm periods and units.
    - Classify observations as direct, recast, or proxy.

- **Loop**
  - One isolated conversation per topic.
  - Up to three passes per topic.
  - Leading metric and bear case receive priority.
  - Stop at the fetch or time limit.

- **Model**
  - Requests searches and page fetches.
  - Extracts sourced findings.
  - Proposes follow-up questions.

- **Output**
  - Full findings for every worked topic; lower-priority topics the budget couldn't reach drop to a recorded gap.
  - Evidence ledger.
  - Mandatory sourced bear case.

---

### Step 5e — Distill the research

- **Data retrieved**
  - No new external data.

- **Model**
  - Condenses the full research.
  - Does not perform new searches.

- **Large-input loop**
  - Normal case:
    - One consolidation call.
  - Large case:
    - Distill each topic separately.
    - Run one final combining call.

- **Model determines**
  - Which findings matter.
  - Whether the research lenses disagree.
  - Severity of contradictions.
  - Key falsifiers.
  - Material forward facts.
  - Possible research-only leading indicators.

- **Typed outputs**
  - Leading-metric observations.
  - Direct forward assumptions.
  - Research target scenarios.
  - Runway evidence.
  - Milestone evidence.
  - Research-only leading indicators.
  - Primary-source forensic events.
  - Sourced bear case.
  - Limited-history evidence when eligible.

- **Output**
  - One structured research object.

---

### Step 5f — Recalculate using validated research

- **Data retrieved**
  - No new data.

- **Logic**
  - Validate all numerical research claims.
  - Reject malformed or unsourced claims.
  - Prefer structured data during unresolved conflicts.
  - Validate any proposed calculation bridge.
  - Recalculate each bridge from sourced facts.
  - Retain the structured-only target for comparison.
  - Add valid new leading-metric observations.
  - Add only direct or explicitly reconciled recast observations to company history.
  - Keep proxy evidence separate from company financial results.
  - Recalculate:
    - Price targets.
    - Leading-metric trend.
    - Business runway.

- **Model**
  - None.

- **Output**
  - Structured-only targets.
  - Research-informed targets.
  - Exact explanation of the difference.
  - Final engine-calculated metrics.

---

### Step 5g — Author the opportunity record

- **Data retrieved**
  - Final calculations.
  - Distilled research.
  - House view.
  - Previous opportunity record, when applicable.

- **Model determines**
  - The investment thesis.
  - Early-detection or continuation mode.
  - Base conviction.
  - Which validated research assumptions support the forward case.
  - Catalyst description.
  - The expected thesis milestones.
  - Evidence-backed milestone date ranges.
  - Which milestone represents the thesis paying off.
  - Bear case.
  - Key falsifiers.
  - Entry consideration.
  - Proposed status:
    - New.
    - Still valid.
    - Invalidated.

- **Model restrictions**
  - Cannot invent financial numbers.
  - Cannot directly choose a price target.
  - Cannot choose a valuation multiple or discount rate.
  - Cannot assign risk tier.
  - Cannot assign horizon.
  - Cannot enforce admission gates.
  - Price action alone cannot raise conviction.
  - A research-only leading indicator can raise conviction by at most one level for any archetype.

- **Output**
  - Proposed opportunity record.
  - Proposed thesis milestone plan.

---

### Step 5h — Deterministic final validation

- **Data retrieved**
  - No new data.

- **Risk-tier calculation**
  - High risk:
    - Small company, unprofitable, highly volatile, highly leveraged, illiquid, or event-exposed.
  - Low risk:
    - Large, profitable, liquid, lower-volatility, lower-debt company.
  - Otherwise Medium.

- **Horizon calculation**
  - Short:
    - The full payoff window ends within three months.
  - Long:
    - The payoff window begins after twelve months.
    - Or multi-year compounding is the payoff.
  - Otherwise Mid.
  - The app derives the category.
  - The model supplies the evidence and timing range.

- **Entry gate**
  - Uses the research-informed target while its evidence is valid and current.
  - Otherwise uses the structured-only target.
  - Expected return must beat:
    - Low risk: `DGS2 + 8 percentage points`.
    - Medium risk: `DGS2 + 16 points`.
    - High risk: `DGS2 + 30 points`.
  - Bear downside cannot exceed base-case upside.
  - Illiquid names receive a return haircut.
  - Emerging businesses must also satisfy their double-over-horizon requirement.

- **Evidence gate**
  - Requires:
    - Current price and price history.
    - Valid leading metric.
    - Current financial or operating evidence.
    - Current bear-case evidence.
    - Computable price target.
  - Limited history does not lower these requirements.
  - Unmapped predecessor or proxy financial data is rejected.

- **Forensic logic**
  - Soft accounting warnings cap conviction at Medium.
  - Restatement, auditor change, fraud, or unsupported hype excludes a debut.

- **Validation**
  - Recalculate final conviction.
  - Confirm any conviction raise uses an independent, unscored, sourced indicator.
  - Validate milestone evidence, dates, dependencies, and completion conditions.
  - Give every machine-checkable milestone condition its own app-controlled ID.
  - An unchanged condition keeps its evaluation history when a deep pass replaces the plan.
  - A changed condition starts fresh; a milestone name alone cannot transfer history.
  - Verify model numbers match engine numbers.
  - Verify falsifiers are actually monitorable.
  - Verify source freshness.

- **Held-out candidates**
  - Gate failure → shadow gate-reject episode.
  - Missing evidence → shadow abstention episode.
  - Hard exclusion → shadow exclusion episode.

- **Existing opportunity exception**
  - Missing evidence does not remove it.
  - Its previous verdict stays.
  - A refresh gap is recorded.

- **Output**
  - Survivor with assigned matrix cell.
  - Or a typed rejection/abstention record.
  - Candidate checkpoint.

---

## Step 6 — Rank and assemble new survivors

- **Data retrieved**
  - No external data.

- **Model**
  - Ranks survivors within each predetermined cell.
  - Suggests merges for near-identical opportunities.

- **App validation**
  - A merge requires:
    - Same matrix cell.
    - Shared hypothesis, leading metric, or catalyst.
  - Invalid merge:
    - List both opportunities.
  - Existing live opportunity:
    - Cannot be merged away.
  - Every survivor must appear or have a validated merge record.

- **Output**
  - Ranked survivor matrix.
  - Validated duplicate records.
  - No fixed number of opportunities per cell.

---

## Step 7 — Refresh existing ideas and finalize the matrix

- **Data retrieved**
  - FMP prices and estimates.
  - New filing-derived metrics when available.
  - FINRA short interest when needed.
  - Deep FMP price history.
  - Stored opportunity and shadow episodes.

- **Deep-researched existing names**
  - Use their Step-5 result.
  - Still valid → remain in matrix.
  - Invalidated → move to archive.
  - Inconclusive → keep previous verdict.

- **All other live names**
  - Run cheap re-derivation.
  - Recalculate targets and risk tier.
  - Recheck entry gate.
  - Recheck structured falsifiers.
  - Recheck structured and filing-based milestones.
  - Refresh since-flagged performance.
  - Raise “Consider Deep Audit” when needed.
  - Never archive.

- **Final matrix logic**
  - Reinsert every surviving existing opportunity.
  - Update its risk row if risk changed.
  - Keep its last deep-pass horizon.
  - Revalidate completeness.

- **Outcome calculations**
  - Measure picked opportunities after:
    - 1 month.
    - 3 months.
    - 6 months.
    - 12 months.
  - Calculate:
    - Return.
    - Return versus sector and market.
    - Maximum drawdown.
    - Whether the leading metric continued.
    - Why the result likely occurred.

- **Shadow scorecard**
  - Measure rejected, deferred, and merged-away names.
  - Identify false negatives.
  - Never automatically promote them.

- **Graph updates**
  - Add or refresh watchlist names.
  - Retire failed hypotheses.
  - Mark archived picks as departed.

- **Model**
  - None.

- **Output**
  - Final matrix.
  - Attention warnings.
  - Archive changes.
  - Picked and shadow outcome episodes.
  - Updated opportunity graph.

---

## Step 8 — Mark opportunities you already own

- **Data retrieved**
  - Fresh holdings from Schwab.
  - Cached holdings if the pull fails.

- **Logic**
  - Add owned/not-owned labels.
  - Holdings do not affect discovery or selection.

- **Model**
  - None.

- **Output**
  - Display-only ownership tags.

---

## Step 9 — Save everything

- **Data stored**
  - Final matrix.
  - Opportunity graph.
  - Discovery coverage ledger.
  - Watchlist research-refresh state.
  - Archived opportunities.
  - Structured-only and research-informed target calculations.
  - Target assumption bridges.
  - Thesis milestone plans and their evaluation states.
  - Limited-history evidence and mapping decisions.
  - Picked and shadow episodes.
  - Outcome labels.
  - Sources and timestamps.
  - Calculations and model versions.
  - Rejection and dedup reasons.

- **Embedding model**
  - DTO:
    - Embeds opportunity summaries.
    - Embeds newly matured lessons and false negatives.
  - ATO Deep:
    - Embeds touched opportunity summaries only.
  - ATO Quick:
    - No embedding call.

- **Output**
  - Durable run record.
  - Searchable continuity memory.

---

## Step 10 — Display the result

- **Data retrieved**
  - Persisted results.
  - Cached daily bars for current display values.

- **UI output**
  - 3×3 risk-by-horizon matrix.
  - Optional flat sortable list.
  - Thesis, target, bear case, catalyst, and conviction.
  - Performance since first selection.
  - Owned/not-owned status.
  - Research-stale badge.
  - Consider-Deep-Audit warning.
  - Separate archive.

- **Model**
  - None.

---

# ATO: Audit selected opportunities

## Quick Audit

`Selected names → Refresh numbers → Check warnings → Save`

- **Data retrieved**
  - FMP price and estimates.
  - Deep FMP price history.
  - FRED rates.
  - FINRA data when required.

- **Logic**
  - Recalculate targets.
  - Recheck the entry gate.
  - Recheck structured falsifiers.
  - Recheck structured and filing-based milestones.
  - Refresh performance.
  - Raise or retain warnings.

- **Model**
  - None.
  - Can run while the model server is offline.

- **Cannot**
  - Rewrite the thesis.
  - Change conviction.
  - Perform new research.
  - Archive an opportunity.

## Deep Audit

`Selected names → Full Step-5 loop → Reconcile → Save`

- **Data retrieved**
  - Full per-company data and fresh web research.

- **Model**
  - Full archetype, research, distillation, and scoring calls.

- **Can**
  - Rewrite thesis and conviction.
  - Refresh research assumptions.
  - Replace the thesis milestone plan.
  - Clear warnings after a successful pass.
  - Mark an opportunity invalidated.
  - Move an invalidated opportunity to the archive.

- **Does not**
  - Run discovery.
  - Rebuild the full watchlist.
  - Modify unrelated opportunities.

## The most important safety rule

- Fast checks may warn.
- Only fresh deep research may remove an opportunity.
- Missing data never causes automatic removal.
- A rejected candidate is still tracked for later evaluation.
