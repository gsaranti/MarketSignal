# Trade Opportunities: logic flow

> This describes the designed job behavior.  
> Trade Opportunities is not yet built — every step below is designed and none is as-built, so the doc carries no built-vs-designed markers.

`Gate → Load context and score due forecasts → Discover names: screens, the watchlist review, routes, the refresh lane → Narrow the slate → Validate each name: archetype → engine and floor → research → analysis → review the prior call → thesis → gate → Order the survivors → Refresh the rest and open episodes → Mark owned → Save → Display`

## How to read this document

- The job is built as a **research instrument**: it tests whether a local reasoner, given primary-source data and the open web, can find a name before the market rewards it, write a defensible thesis for it, and price it.
- Two jobs share one page. **Discover** runs the whole funnel; **Audit** re-evaluates the opportunities the user selects, reusing Discover's stages.
- Every opportunity carries the judgment **twice**, as two arms with different provenance:
  - The **engine arm** is a deterministic Rust calculator over primary-source data — an archetype-weighted quant composite and value-creation read, scenario bands at three horizons, an implied-expectations range, a narrative-versus-reality read, a forensic state, a rule-derived risk tier and horizon, and on a carried name the since-flagged read. It never consults the model.
  - The **model arm** is the reasoner's own prose — a thesis document — and the typed appendix transcribed from it: the risk tier and horizon that place the card, the conviction, the expected price at each horizon, the detection mode, the leading metric with its class, and on a carried name the status. The app checks it on **type alone**, never on content.
- Placement is the model's and the admission yardstick is the engine's: the card sits in the model arm's tier × horizon cell, while the entry gate's required return, haircut, and horizon window read the engine's legs on both arms, so the model never lowers its own bar.
- Four rules keep the experiment honest:
  - Nothing the model writes ever alters or binds an engine value.
  - A name clearing **either** arm's entry gate is admitted; the evidence floor and the hard triggers bind both arms absolutely.
  - Both arms are scored the same way against the same realized prices.
  - The model reads its own track record and the engine's before it writes again.
- One local model does every reasoning job by switching mode: **thinking** for route planning, research, the write-ups, the hypothesis documents, the watchlist review, archetype confirmation, the analysis, the review, and the thesis document; **non-thinking** for distilling write-ups and for transcribing a document's stated values into a typed object or a decision.
- Every model prompt is **one message in two parts**: the data with a one-time gloss of each section, then the task. No prompt names the app, its stages, or its arms.
- The canonical specification is `docs/trade-opportunities.md` and `docs/trade-opportunities-workflow.md`, with `docs/portfolio-analysis.md`, `docs/web-research.md`, `docs/local-models.md`, `docs/storage.md`, `docs/data-sources.md`, and `docs/schwab-integration.md` behind them. This document is the plain-language map, not the specification.

## Important terms

- **DTO — Discover Trade Opportunities**
  - Finds new ideas and maintains existing ones.
  - Runs the full workflow.

- **ATO — Audit Trade Opportunities**
  - Rechecks opportunities you select, in a Quick or a Deep mode.
  - Discovers nothing.
  - Shares one `trade_opportunities` job identity with DTO; each run record carries its mode (`discover` / `audit-quick` / `audit-deep`), so history, retention, and the page footer's last-run stamp read one mode-labeled pool.

- **Candidate**
  - A company being investigated. Not yet an opportunity.

- **Opportunity**
  - A candidate that cleared the floor, the hard triggers, and either arm's entry gate, and sits in the matrix.

- **Debut**
  - A candidate with no live opportunity record.
  - Only a debut can be held out at the evidence floor, excluded by a hard trigger, or turned away at the entry gate.

- **Carried-forward (a carry)**
  - A live opportunity loaded from the prior run.
  - Gets a deep pass (a rotation pick, a budget-winning re-surfacer, or a Deep Audit) or the cheap re-derivation.
  - Leaves the matrix only when a deep pass judges it invalidated.

- **Hypothesis**
  - A testable investment idea: a world change, the mechanism, who captures the margin, the leading metric that would prove it, the public-company expressions, the bear case, the falsifiers.

- **Hypothesis document**
  - The prose a route's research is consolidated into: every hypothesis the route formed, argued in full, with its decision — promote, watchlist, or none.
  - Read as text and validated by nothing. Persisted on the audit and into the opportunity graph.

- **Hypothesis appendix**
  - The hypothesis document's stated values transcribed into a typed object: per hypothesis the title, the decision, the candidate symbols, the leading metric with its re-check class, and for an event-impact hypothesis each name's side and the gate fields.
  - Type-checked only.

- **Route**
  - A research direction the discovery lane spends budget on: policy / regulatory, supply chain, technical bottleneck, procurement / capex, customer capex, industry history, failure analogue, event-impact repricing.
  - Each carries its own source strategy and is worked as a list of topics.

- **Outside-view route**
  - The one mandatory route every run, run graph-blind: it never sees prior hypotheses.
  - Keeps the discovery memory from anchoring the job to its own past.

- **Coverage debt and the coverage ledger**
  - A route class, broad industry, or active theme not successfully researched within the coverage window (~4 weeks, calendar time) is in debt; the app reserves the next route slot after the outside-view route for the oldest.
  - The ledger records, per class and subject, first seen, last attempted, last successfully completed, and the computed debt. A completed route pays debt even when it finds nothing; a failed route does not.

- **Opportunity graph**
  - The job's discovery memory: hypothesis nodes with their document sections, their appendix fields and the sections they superseded as dated history — each live for the carry horizon from its metric-named date and while a linked company node is picked — and company nodes with a status, a leading metric, an admission print once served, refresh timestamps, and a hypothesis link.

- **Watchlist node**
  - A worthy-but-unpicked name remembered in the graph: its hypothesis section, its leading metric with its class, its metric-named date, its status, and its refresh timestamps.
  - Its admission print is the first print the Step 3c refresh serves; a gate-rejected debut's is the series its Step 5c pass computed.
  - Re-read every later run by the watchlist review.

- **Watchlist review**
  - One thinking conversation per DTO run over every live watchlist node, returning a prose review and, by transcription, a typed decision per node: promote, keep, or retire.

- **Refresh write-up**
  - The prose the research-watchlist refresh lane writes for one selected `research`-class node: what current evidence says about its named metric and falsifiers.
  - Persisted onto the node as its latest observation, with its vintage.

- **Archetype**
  - The kind of business or opportunity: secular compounder, AI / secular-cyclical infra, commodity cyclical, category disruptor, quality compounder.
  - Decides which signals matter and which valuation lens applies. Never an input to the risk tier.

- **Leading metric**
  - A countable, dated, third-party-verifiable number expected to move before profits or the stock price.
  - Examples: backlog, bookings, subscriber additions, estimate revisions, segment revenue.
  - A name without one is a story stock.

- **Re-check class**
  - How a leading metric can be refreshed: `structured` (an engine series, every run), `filing` (a standardized statement line, on filing cadence, model-free), `research` (only a web pass can refresh it).
  - A type, not a claim: a `structured` metric is chosen from the engine's series menu, a `filing` metric from the statement-line menu, and anything else is `research` by construction.

- **Story stock (`no-inflecting-metric`)**
  - A debut whose structured metric family is measurable and not inflecting. Held out at the engine, before any research is spent.

- **Insufficient evidence (`insufficient-evidence`)**
  - A debut missing a floor-bearing input, or carrying stale or conflicting data. An abstention, never a low-conviction guess.

- **Inconclusive refresh**
  - A carried name whose deep re-read falls below the floor on missing or stale evidence. It holds its last verdict, stamps nothing, clears nothing, and opens no episode.

- **Engine arm**
  - The deterministic baseline: the composite and sub-scores, the value-creation read, the leading-metric series and its inflecting read, the bear / base / bull bands at three months, twelve months, and three years, the implied-expectations range, the narrative-versus-reality read, the forensic state, the rule-derived risk tier and horizon, and the since-flagged read.
  - App-stamped onto the record directly, never echoed through the model.

- **Model arm**
  - The thesis document plus its typed appendix.
  - Authored with the engine's values and their methodology in the prompt as evidence, never as bounds.
  - Checked on type alone; its tier × horizon place the card.

- **Thesis document**
  - The model's prose record of its view on a candidate: the directional thesis, the detection mode, the leading metric and its trend, why now, the drivers, the scenarios with probabilities, the bear case, the falsifiers and triggers in words, what the price already assumes, the tier and horizon argued from payoff timing and measurable risk, the expected prices and conviction argued in the text, the entry consideration, and on a carried name what changed.
  - Read as text and validated by nothing. Each deep pass's document supersedes the prior one.

- **Typed appendix**
  - The thesis document's stated values transcribed into a typed object: risk tier, horizon, conviction, the expected price at each horizon, the detection mode, the leading metric with its class, and on a carried name the status.
  - Type-checked only.
  - Any field null where the document states no value; a null is acted on by presence, never read as content.

- **Conviction**
  - The model's confidence in its own thesis: high, medium, or low.
  - Its own and bidirectional; never clamped, never derived by the app.

- **Expected price**
  - The model's single-point expected share price at three months, twelve months, and three years.
  - Finite and strictly positive, or the appendix is rejected.

- **Risk tier**
  - High, Medium, or Low, carried in both arms.
  - The model's own tier sets the matrix row; the engine's rule-derived tier scales the entry gate's required return on both arms and renders beside the placement as the baseline.

- **Horizon**
  - Short, Mid, or Long, carried in both arms.
  - The model's own horizon sets the matrix column; the engine's follows a drafted rule over the archetype and dated transaction events, and sets the gate's **H** on both arms.

- **Detection mode**
  - Early: a leading metric inflecting before the income statement and the multiple, while a bear narrative still suppresses the price.
  - Continuation: demand-visibility signals that license buying a move already underway.

- **Evidence floor**
  - The minimum evidence a candidate needs before any judgment is written: a current quote and price history, a measurable and fresh structured metric family where the archetype's tell lives in one, and the statements or their archetype substitute.
  - Engine-only, run before research. Binds both arms absolutely.

- **Entry gate (entry asymmetry)**
  - The required twelve-month forward return a name must clear: `DGS2` plus 8, 16, or 30 points by engine risk tier, with the shape, liquidity, and double-over-horizon legs.
  - Run once per arm; a name clearing either is admitted. Re-run on every cheap pass.

- **Admission provenance (`admitted_by`)**
  - Which arm's gate let a name in: `engine-and-model`, `engine-only`, or `model-only`.

- **Hard triggers**
  - A restatement or auditor change from the item-classified filings.
  - Exclude a debut outright, retiring its watchlist node where one is live; force a carried name to `invalidated`. Bind both arms.

- **Cheap re-derivation**
  - Fast, model-free refresh of the engine fields and both arms' gates.
  - Can raise a warning; cannot re-rate, re-place, or remove an opportunity.

- **Deep re-evaluation**
  - The full per-candidate loop on an existing opportunity.
  - The only process allowed to rewrite the model-authored fields or archive.

- **Attention warning**
  - Amber *Consider Deep Audit* flag the cheap re-derivation raises on a tripwire, exhausted upside, or a re-surfacing.
  - Never changes the verdict; cleared by the next floor-clearing deep pass.

- **Since-flagged read**
  - Running return since the name became an opportunity (absolute, vs sector, vs market), its maximum drawdown, and the continuation state of the engine's metric family.
  - Reconstructed from daily bars each run; cap-only in the thesis document — it can hold or lower conviction, never raise it.

- **Write-up**
  - The prose a research pass produces on one topic. Rewritten whole on a follow-up pass, so a topic has one write-up at a time.

- **Analysis**
  - One document consolidating a candidate's write-ups for this run. The only research artifact the next deep pass reads.

- **Review**
  - The model's assessment of its prior call on a carried name against what actually happened, written before this run's thesis document.
  - Where the accuracy record reaches the model.

- **Distillation**
  - A non-thinking shortening of write-ups, used only when the consuming prompt is over budget.

- **Fetched values**
  - The candidate's provider data as returned, glossed once and never engine-computed.
  - The same block, byte for byte, leads the research, synthesis, and thesis-document prompts.

- **Page roster**
  - Every page shown to the model for a route or a candidate: address, title, publication date, retrieval time, and source tier. Never page text. The durable provenance on the audit record.

- **Episode**
  - A price record opened for a candidate whose appendix carried prices: the symbol and lifecycle, the decision class, the creation date, that day's spot, an anchor close, the model's three expected prices (null where the appendix stated none), and the engine's three base values.
  - Append-only. Never updated or deleted.

- **Decision class**
  - The tag an episode carries: `picked`, `gate-reject`, or `excluded`. A tag, never a verdict on the decision.

- **Check**
  - A comparison written onto an episode when a horizon date passes: the close on that date and both arms' scores.

- **Accuracy score**
  - A lifecycle's mean per-check score at a horizon, 0–100, kept separately for the model and the engine.

- **Lifecycle id**
  - App-assigned identity for one stretch of a ticker under the job's judgment, from its first research as a debut to its departure.
  - A gate-reject or excluded episode carries it, a gate-rejected debut's watchlist node carries it, and an admission through that live node continues it, so the pick's accuracy scores count the turned-away checks; a debut with no live node starts a fresh one.
  - A re-entry from the archive is a new lifecycle; nothing from the old one carries.

- **Divergence tag**
  - The quiet card badge raised when the arms materially disagree: the twelve-month prices' gap, a tier / horizon pair, or the narrative divergence.

- **Status-override divergence**
  - The record kept when a hard trigger forces a carried name to `invalidated` against the model's proposed status: the proposed status, the forced status, the matched trigger, the filing.

- **Narrative divergence**
  - The record kept when the model holds a carried name `still-valid` against the engine's anchorless `hype` read: the engine read, the model status.
  - A dated snapshot of the deep pass that wrote it; the cheap re-derivation neither records nor clears it.

- **Archive**
  - The price-tracked record of departed picks, the most recent 100.
  - A name leaves the matrix for it only on a deep pass that judges it invalidated; re-entry is a fresh start.

- **Rotation slice**
  - The reserved share of the deep-research budget (default ~20%, never below one slot) spent first on live opportunities in maintenance-priority order, backstopped by a max-age service level.

- **Deep-research set**
  - The run-scoped list of tickers deep-researched this run. A ticker in it is never also cheap-swept.

- **Research cache**
  - The cross-run document cache: fetched, readability-extracted pages keyed by normalized URL, under ~4 weeks old, carrying their original retrieval time.
  - Document-level only: a page can be reused, a judgment never is; searches always run live.

- **Split bridge**
  - The conversion that brings every stored price onto today's basis after a stock split, from an anchor bar stamped at authoring against the same bar in a fresh fetch.

- **House view**
  - The current Market Signal thesis and major market themes. Omitted, and recorded as a gap, when older than one week.

- **Reasoning model**
  - The local 122B model: thinking mode for research and every document, non-thinking for distillation and transcription.
  - Fills every model role by switching mode. The job makes no embedding call.

## Main data sources

- **FMP — discovery layer (universe-wide, a bounded number of calls per run)**
  - `company-screener` — universe definition, tradability gate, and market-cap-band / sector stratification (coarse fields only — no valuation or growth filter; `*-bulk` pre-scoring is off-plan).
  - `insider-trading/latest` — market-wide newest Form 4s for insider cluster buys.
  - `biggest-gainers`, `biggest-losers`, `most-actives` — movers.
  - `earnings-calendar` — upcoming reporters, and read backward as the post-earnings surprise screen.
  - `mergers-acquisitions-latest`, `sec-filings-8k`, `ipos-calendar` — fresh corporate events; the M&A feed and the 8-K sweep also supply the engine horizon's transaction-close input.
  - `available-sectors`, `industry-classification-search`, `all-industry-classification`, `stock-peers` — map a hypothesis onto its exposed names; verify each symbol.
  - `news/general-latest`, `news/stock-latest`, `fmp-articles` — ticker-tagged, dated headlines that ride the discovery routes as leads: what to pursue, never evidence a write-up may rest on.

- **FMP — per-candidate surface (the budget driver; fires only for the narrowed set)**
  - `profile` — sector, industry, beta, description.
  - `income-statement` (+ TTM), `balance-sheet-statement`, `cash-flow-statement` — the core statements.
  - `key-metrics`, `ratios` (+ TTM), `financial-scores` (Altman Z, Piotroski), `owner-earnings`, `enterprise-values`, `discounted-cash-flow`, `financial-growth` (multi-year per-share CAGRs), `dividends` (trailing distributions — the bands' twelve-month payout proxy, not a forward estimate).
  - `revenue-product-segmentation`, `revenue-geographic-segmentation` — annual only.
  - `analyst-estimates` (snapshotted run to run for revision velocity), `grades`, `grades-historical`, `grades-consensus`, `price-target-consensus`, `price-target-summary`, `ratings-snapshot`, `ratings-historical`, `earnings` (next date + surprise history).
  - `news/stock` — symbol-scoped headlines, the candidate's news leads.
  - `insider-trading/search`, `insider-trading/statistics`, `acquisition-of-beneficial-ownership` (13D / 13G), `senate-trades`, `house-trades`, `shares-float`; optionally `historical-employee-count`, `key-executives`.
  - `quote` — the live price the engine prices targets and runs the gate against.
  - `historical-price-eod/light` (dated) — the deep daily history, through the shared price-bar cache.

- **FMP — run-level series**
  - Commodity series `HGUSD` (copper), `GCUSD` (gold), `SIUSD` (silver) — daily price turns for the cyclical sleeve.
  - Benchmark series `^GSPC` and the SPDR sector ETFs — the since-flagged read's market and sector legs.

- **FRED**
  - `DGS2` and `DGS10` Treasury yields; the anchor-window `DGS10` history for the valuation multiples.
  - `DCOILWTICO` (WTI), `DHHNGSP` (Henry Hub) daily; `PCOPPUSDM`, `PALUMUSDM`, `PNICKUSDM`, `PIORECRUSDM`, `PURANUSDM` monthly IMF metals.
  - `/release/dates` — the macro-release calendar (names + dates).

- **SEC EDGAR**
  - Submissions — 10-K / 10-Q / 8-K, item-classified (Item 4.01 auditor change, Item 4.02 restatement); S-1 / Form 10 history for an eligible new listing or separation.
  - XBRL company facts — the authoritative statement cross-check.
  - 13F — run-level, optional, coarse; held out of the grade.
  - Earnings-release recovery: an 8-K exhibit fetched when an issuer's own site blocks the research fetch.

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
  - Read-only by construction: the only calls are the chains read and the positions read.

- **SearXNG**
  - The only web search for discovery and candidate research: a self-hosted, keyless metasearch instance.
  - Serper, a paid Google-results API, is wired inside it as a keyed engine that fires on every query, the reliable floor; the keyless engines stay as redundancy, and the key lives outside the repo.
  - Tavily is the report job's and is never called by this job. No GDELT.

- **Local storage**
  - The prior run's matrix, the opportunity graph, the coverage ledger, the archive, and the episode store — the five persisted structures.
  - The shared price-bar cache, the document cache, the factor-distribution store, and the web-research source state.
  - The house view and recent report summaries.
  - Each carried name's prior thesis document, analysis, and appendix values, by lifecycle id.

---

## The research loop (shared by Steps 3b, 3c, 5d, and Deep Audit)

Four stages reach the open web, and all of them run the same bounded loop Portfolio Analysis runs at its Step 6c: Step 3b's discovery routes, Step 3c's refresh lane, Step 5d's per-candidate research, and a Deep Audit (Step 5d on the user's selection). The mechanics are written once here; each step states only what is its own — its agenda, its budget, its leads, and what comes out — and points back.

### What differs per stage

- **Step 3b — discovery.** The unit of work is a route, worked as its topic list: `route ⊃ topic ⊃ pass ⊃ fetch`. One per-run discovery fetch and wall-clock ceiling is shared across every route, spent in route-priority order. The leads are the FMP news feeds and the macro-release calendar. After a route's topics, one disconfirming pass runs for the route. The route's write-ups flow to its hypothesis-formation conversation.
- **Step 3c — the refresh lane.** One selected watchlist node, one isolated bounded conversation, spent from the same discovery ceiling after the routes. It is given the node's hypothesis section and its named metric and falsifiers as the questions, and returns a refresh write-up and a typed decision.
- **Step 5d — per-candidate (and Deep Audit).** The unit is the candidate's agenda. A per-candidate fetch and wall-clock budget, spent in topic-priority order, the leading metric and the bear case first. The leads are the candidate's `news/stock` headlines. After the topics, one disconfirming pass runs for the candidate. The write-ups flow to Step 5e.
- **No cross-run findings seed.** The cross-run cache is document-level only. A carried name's prior analysis and prior thesis document ride the prompts verbatim, dated; no distilled object from a prior run seeds a loop.

### Build the agenda

- The orchestrator assembles the topic list from the stage's documented list, and the reasoner works it one topic at a time. At Step 5d the list is fixed: the candidate's topics plus the deterministically triggered reconstruction topic. At Step 3b the route agenda and each route's topic list are the planning call's proposal, app-validated — the one agenda in the suite the reasoner proposes; inside the loop the model never authors a topic.

### Work each topic — the loop

- **Two nested levels**
  - A topic is worked in isolation over a clean context — its gathering loop plus that pass's separate synthesis conversation. Topics never share a context, and no other topic's write-up is fed in; topics meet only downstream, at the consolidating call.
  - A topic's root pass plus at most two follow-ups: three passes per topic. The cap counts passes, not searches.

- **Scheduling**
  - Every eligible root runs before any follow-up; then follow-ups in topic-priority order.
  - A follow-up is the model's own question, spent only if the orchestrator's budget allows; on exhaustion it is simply not spent.

- **Gathering conversation** (thinking, tools, no grammar)
  - The model asks for `web_search` and `web_fetch` calls; the orchestrator executes them and returns the results as data. The model never touches the network.
  - At most 8 replies per pass, counted down by a short app message before each request. Pages fetched on the last reply are kept.
  - At most 8 tool calls per reply; a larger batch has its head executed, its tail recorded as partial coverage, and the pass moves to synthesis.
  - The whole growing conversation plus the tool schema is sized against the shared input guard before every request and before each retained result; an overflow ends gathering as a recorded degradation.
  - Search and page metadata (titles, snippets, dates) are capped so untrusted text cannot consume the packet.
  - Prior `<think>` blocks are stripped from history, never accumulated across turns.
  - A pass ends on a reply with no tool call or at a bound.

- **Synthesis conversation** (thinking, no tools, no grammar)
  - A fresh conversation over the gathered pages writes the pass's write-up. A tool-using turn and the write-up never share a request.
  - On a follow-up pass the topic's write-up is rewritten whole with the new evidence folded in.
  - A second message asks whether the research has a follow-up question: the question verbatim, or the one word `none` — the only reply the app interprets. Not sent on a topic's last pass or on the disconfirming pass.
  - A pass that retrieved no page spends no synthesis; the write-up so far stands.

- **What a conversation is given**
  - The stage's constant block, identical for every topic of the item: at Step 5d the candidate header, FETCHED VALUES, NEWS LEADS, and on a carry the prior analysis and prior thesis document; at Step 3b the route header, MARKET ANALYSIS, PRIOR HYPOTHESES (withheld from the outside-view route), the route's source-strategy rubric, and NEWS LEADS.
  - PAGES ALREADY RETRIEVED — pages fetched for this item earlier in this run, by other topics or earlier passes, in first-retrieval order; held in memory and discarded between items. Supplied to ordinary topic and follow-up gathering only: the disconfirming pass gets no automatic page injection, so its contrary search stays its own.
  - That topic's own questions; on a follow-up pass FOLLOW-UP and WRITE-UP SO FAR; on the disconfirming pass WRITE-UPS SO FAR.
  - Part 2: what to find, how to weigh a source, the per-reply tool-call bound, and when to stop.

- **Leads are leads, not citations**
  - A headline points at what to pursue; a write-up rests on the pages the model read, never on a lead's headline or snippet.

- **Fitting the fixed context window**
  - `num_ctx` is fixed per model and never raised to make room; context pressure is answered by dropping content, not by growing the window.
  - Each page body and each tool-result header is capped at insertion, tool calls per reply and replies per pass are capped, and the gathering packet stops before overflow.
  - The synthesis packet budgets page headers and bodies together: a source survives only with usable body text, omitted headers are reclaimed before the bodies are filled, and every omission or truncation is shown inline and recorded as a gap.
  - The model server's own truncation is never relied on: it silently drops the prompt's head and leaves the model to hallucinate over the gap.

- **Disconfirming pass**
  - Once per route and once per candidate, after the topics: a gathering conversation searching for evidence against the write-ups so far, then its own synthesis conversation writing its own write-up and asking no follow-up.
  - Spent from the stage's budget, outside any topic's depth, fail-soft to a recorded gap when the budget is exhausted.

- **Stops at the budget**
  - The stage's fetch and wall-clock budget binds first, polled between requests, never mid-request, and spent in priority order. When it drains, the lowest-priority remaining topics or routes are skipped as a recorded gap; the current pass still gets its synthesis.
  - A failed live attempt spends like a served one; a cache hit or a remembered failure spends nothing; a retry spends another.

### The fetch layer

- **Search**
  - SearXNG only, over its JSON API on loopback. It fans each query to its keyless engines plus Serper, the keyed Google-results engine that fires on every query as the reliable floor.
  - Queries are paced with jitter against upstream rate limits; a repeated query within a run is served from a per-run cache.
  - A down or misconfigured instance returns a typed search failure — one fixed sentence, never the operator's error text — distinct from an empty search's `No results.`, so unavailable research never reads as absent results; either way the loop proceeds thinner. No Tavily, no GDELT, no keyed fallback.

- **Fetch and extraction**
  - A plain HTTP GET with browser-like headers and a timeout, then a Rust readability extraction to the article body. SEC hosts get the app's declared identity.
  - Paywalled or JavaScript-heavy pages return thin text; a selective render tier escalates pages the extraction telemetry flags as thin, reusing the app's embedded webview.
  - Optional Connected Sources attach a stored login session for a domain; never part of the gate.

- **Safety**
  - Only `http` and `https` to public hosts; private, loopback, and reserved ranges are blocked. Redirects are capped and re-validated. Responses are bounded by size and type.
  - Page text is data, not instructions: inserted as quoted evidence, so an injected page cannot redirect the analysis.

- **Source quality informs, never gates**
  - Every domain carries a tier 0–5 (0 primary filings and regulators; 1 licensed data providers; 2 high-trust reporting; 3 specialist industry sources; 4 opinion; 5 sentiment only). A low tier weights down; it never removes a page or a candidate.
  - The explicit `deny` list (SEO mills, AI-generated quote pages, press-release spam) is the one categorical exclusion.
  - Syndicated reprints of one wire count as one source.
  - This job leans to specialist and value-chain sources: discovery takes a soft preference, candidate research weights stricter.

- **Reuse and memory**
  - A shared cross-run document cache serves repeat fetches inside a ~4-week window, each page carrying its original retrieval time; the vintage is never rewritten on reuse. Searches always run live. Each pass records its reused-versus-fresh split on the audit.
  - An item-scoped in-memory inventory feeds the pages-already-retrieved block and is discarded between items.
  - Failed-fetch memory spans the run: a 401 or 403 puts the host in a five-minute cooldown; a 408, 429, or 5xx, a timeout, or a reset gets one retry after one second; other failures are remembered and not retried.

- **Earnings-release recovery**
  - When an issuer's own site blocks the fetch of an identifiable earnings release, the loop may recover the matching 8-K Item 2.02 exhibit from EDGAR, within ten extra attempts under the item's budget, cited under the SEC address.

### Failure and output

- Research is fail-soft: a failed search, a timed-out fetch, a drained budget, or an unreachable SearXNG thins the evidence and never fails the run.
- A hard model failure inside a required per-candidate path fails the run; the per-candidate checkpoint lets a resume pick up the unfinished candidates.
- Output: one write-up per worked topic plus the disconfirming pass's, flowing whole to the stage's consolidating call.
- Every page shown enters the page roster, persisted on the audit record; every degradation persists as a gap and counts into the run's data health.

## Consolidation (shared by Steps 3b, 5e, and Deep Audit)

Write-ups are consolidated by a thinking call that reads them whole: the hypothesis document at Step 3b, the analysis at Step 5e. Distillation happens only when that call's prompt is over budget, and it condenses write-ups, never already-distilled notes.

- **Budget check**
  - The orchestrator sizes the consuming prompt — the write-ups plus the stage's other inputs — against the call's input budget.
  - Within budget: the write-ups go in as written.
  - Over: the merged write-ups are distilled into one shorter document.
  - Where the merged write-ups exceed one distillation call's budget: each write-up is distilled first and the merge of those outputs is distilled again.
  - A prior analysis is never distilled.
  - The shape is chosen deterministically from size, never by the model, and logged to the audit with its call count.

- **Call: distillation** (non-thinking, no grammar)
  - **Sees** — the item header, then the write-ups to distill under their topic headings: the merged write-ups, or one write-up on a per-write-up call. The task: a shorter document that keeps every dated figure with its source, every disagreement between pages, and every open question.
  - **Returns** — prose at most half its input and at most 1,200 words. Consolidation, not new reasoning. Validated by nothing.
  - **Model** — the resident 122B in non-thinking mode by default; the fast 35B tier is a benchmark-gated option. A rendered prompt over the fast tier's budget issues on the resident reasoner; over the widest budget it takes the per-write-up shape, or is refused as a hard failure where no smaller shape remains. A length stop at the normal 12,288-token output reservation gets one re-attempt at 32,768.

- **The write-ups persist on the audit as written, never as distilled.**

---

# DTO: Discover Trade Opportunities

## Step 1 — Start and gate

- **Data retrieved**
  - No investment data yet.

- **Presence checks** (lock the Discover and Audit buttons and raise a persistent warning until fixed)
  - The Ollama endpoint and the reasoner model id are configured. The job makes no embedding call.
  - Schwab is connected and its seven-day refresh token is still valid — needed only for the per-candidate option chains and the Step 8 holdings label, but a hard precondition all the same.
  - FMP and FRED credentials exist. Tavily deliberately does not gate the local suite.
  - Each failure has its own warning category: local models not configured, Schwab connection, missing provider credentials.

- **Run-time checks**
  - No other job holds the single global run slot.
  - The local daemon answers and the reasoner is pulled. This local-only probe runs before the slot is claimed; every external fetch happens inside the slot. A failure blocks the attempt inline, never as a persistent warning. A Quick Audit, which makes no model call, skips it.
  - Schwab API reachability is not tested here; an outage surfaces when the option-chain or holdings fetch runs.

- **Pre-run notice**
  - The app probes the SearXNG instance. If it cannot serve search, a confirm dialog says the run will discover and research blind and is not recommended, with Proceed or Cancel. A consent step, never a block.
  - The notice also surfaces the rotation backlog's count and oldest research age when a backlog exists (Step 4).

- **Model**
  - None.

- **Output**
  - Job starts.
  - Or the app explains what is missing.

---

## Step 2 — Load shared context and score due forecasts

Loaded once per run and shared across every candidate; nothing here is re-requested per name. The accuracy checks run here too, before any discovery, so this run's reviews see every check that has come due.

- **Data retrieved from local storage**
  - The house view — the latest report's Thesis, Investment Strategy, and Forward Outlook sections plus recent report summaries (`thesis_stance`, `forward_outlook_themes`, `key_risks`), with the report's creation date; loaded deterministically from the report store, never by vector search.
  - The prior run's persisted opportunity matrix (the live carries).
  - The opportunity graph — prior hypothesis documents with their appendix fields, and the watchlist nodes with each one's leading metric, class, status, and refresh timestamps.
  - The discovery-coverage ledger.
  - The investor profile is not loaded: the opportunity record is profile-independent, and no prompt in the job carries it.

- **Data retrieved from FRED**
  - Current `DGS2` (one print) and `DGS10` (one print), plus the anchor-window `DGS10` history as dated observations (one date-ranged request) the spread percentiles join against.
  - The macro-release calendar (`/release/dates` — names + dates).
  - Daily energy prices `DCOILWTICO` (WTI) and `DHHNGSP` (Henry Hub); monthly IMF metals `PCOPPUSDM`, `PALUMUSDM`, `PNICKUSDM`, `PIORECRUSDM`, `PURANUSDM`.

- **Data retrieved from FMP**
  - The daily commodity series `HGUSD` (copper), `GCUSD` (gold), `SIUSD` (silver) — a series, not a point level, because the cyclical sleeve reads a turn.

- **Data retrieved from CFTC and CBOE**
  - CFTC Commitments of Traders — `gpe5-46if` (E-mini S&P 500, Nasdaq-100, 10Y / 2Y Treasuries, USD index) and `72hh-3qpy` (gold, WTI crude, copper).
  - CBOE daily put/call ratios (total, equity, index).

- **Logic**
  - Omit the house view when the report is older than one week, counted in whole ET session days on both sides — recorded as a gap, never fed as current.
  - Normalize rates into decimal form (`N pts = N ⁄ 100`); every later return and threshold reads that representation.
  - The house view's `market_cycle` × `risk_posture` is the job's macro / regime backbone — reused, never recomputed; the forward thematic map that completes the worldview is built at Step 3b.

- **What each context input feeds (later steps, not here)**
  - House view → rendered as a market-level analysis, never named by product, in the Step 3b planning, route, and hypothesis-formation prompts, the Step 3c review and refresh prompts, and the Step 5g thesis-document prompt. It steers where the job hunts; it is never a number the engine consumes.
  - `DGS10` and its history → the scenario multiples at Step 5c and every cheap re-derivation (Step 7, Quick Audit).
  - `DGS2` → the entry gate at Step 5h and every cheap re-derivation.
  - Commodity series (FRED + FMP) → the Step 3a commodity-turn feeder and per-candidate context for a commodity cyclical (Step 5c); Step 2 is the sole commodity owner — nothing re-fetches them.
  - CFTC positioning → the Step 3a washed-out-sentiment read and the commodity-cyclical candidate's underlying-positioning read (Step 5c).
  - CBOE put/call → a venue-level sentiment backdrop, never a per-name signal.
  - Macro-release calendar → leads for the Step 3b routes.
  - Prior matrix → the Step 4 budget split, the Step 5b carried dossier, and the Step 7 carry-forward.
  - Opportunity graph → Step 3c's review, Step 3b's planning and routes (withheld from the outside-view route), and the Step 7 reconcile.
  - Coverage ledger → the Step 3b coverage rotation.

- **Failure rule**
  - `DGS2` or `DGS10` still unavailable after the shared bounded retries → fail the run here, before any per-candidate work (DTO and Deep Audit; the Quick Audit instead fail-softs to a cached print — see ATO).
  - Optional market context (commodities, CFTC, CBOE, the calendar) fails softly to a gap.

### Accuracy checks (engine-only, before discovery, in every mode)

Engine-computed over the append-only episode store. No model. They run in Discover, Deep Audit, and the engine-only Quick Audit alike. Opening new episodes happens after the loop (Step 7), since it needs this run's new prices.

- **Find due horizons**
  - An episode's horizon dates are its creation date plus three, twelve, and thirty-six calendar months.
  - A horizon is due when its date is on or before the run's ET session date and nothing has been written for it. One query over the store; nothing is ever re-scored, and a horizon is scored by the first run on or after its date, however many runs later that is.
  - Every episode in the store is visited — a live pick, a departed one, and a turned-away name alike, since the record measures the forecast.

- **Refresh prices**
  - The dated daily series is refreshed through the shared price-bar cache for every symbol with a due horizon.

- **Write the check**
  - Read the close on the horizon date, or the last session at or before it within 5 sessions.
  - Bridge the episode's prices across any split since creation using its anchor close.
  - Per-check score = 100 × (1 − |expected − actual| ÷ actual), floored at 0, for the model's price and the engine's base value alike.
  - Write onto the episode: horizon, check date, the close, both expected values, both scores.
  - A horizon whose model price is null scores the engine leg alone; the model's accuracy score at that horizon counts no such check.

- **Pending versus unscorable**
  - A failed refresh leaves the horizon pending; it is due again next run. Never a run failure.
  - A served series with no close inside the window — a delisting, an acquisition, a ticker change, a symbol the source never covered — or whose anchor bar it no longer carries, writes the horizon unscorable at once with its cause. It leaves the due set, counts in no score, and the symbol spends no further pull.

- **Derive the scores**
  - Per lifecycle, per horizon, per arm: the mean of its per-check scores, 0–100. "No score yet" before the first check.
  - Each score carries the date of the last check that moved it and whether the prior analysis read it. Because the checks precede the loop, every check this run writes is new to this run's reviews.
  - A pick's scores persist with the opportunity, render on its card, and reach the model in the self-review alone (Step 5f). A turned-away name's checks and scores sit in the store with their decision class and reach no prompt and no card.

- **Model**
  - None.

- **Output**
  - One shared context packet, reused for every candidate.
  - The episode store updated with this run's checks, and each lifecycle's current accuracy scores.

---

## Step 3 — Discover candidates

Three feeders run and converge: the bottom-up structured screens (3a), the model-led hypothesis-research lane (3b — the job's edge, where names are *found* by reasoning rather than looked up), and the carried-forward watchlist (3c — the discovery memory). In execution order the 3a screens and 3c's engine refresh and watchlist review run first, so 3b's planner reads the review's decisions; the 3b routes follow; and 3c's research refresh lane runs last, from whatever discovery budget the routes leave. All three are fail-soft — a failed screen, route, or review means fewer candidates, never a failed run — and each reduces to a candidate set with the signal that surfaced each name attached.

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
  - **Universe and stratification** — the screener applies the hard tradability gates (price / volume / market-cap floors, `isActivelyTrading`, the allowed exchange set, equities only) and tags every eligible name with its **market-cap band and sector / industry**. This is the breadth backbone — it defines the strata Step 4 fills, not a ranked shortlist: the screener carries no valuation, profitability, or growth field, and the `*-bulk` universe-scoring endpoints are off-plan, so the universe **cannot be pre-scored**.
  - **Insider-buy clusters** — open-market buys by multiple insiders from the Form-4 feed.
  - **Short-interest extremes** — from the FINRA file: level, trend (current vs prior settlement), and days-to-cover (short interest ÷ average daily volume). Short interest is a **bearish-by-default** factor; the squeeze reading is a narrow conditional setup (an inflecting leading metric + a near-term catalyst + evidence the bear case is breaking) decided per candidate at Step 5, never here.
  - **Post-earnings surprise screen** — for each recent reporter, the surprise is **standardized** against the name's own surprise history (SUE-style — the surprise scaled by the dispersion of its past surprises, never a raw percentage); large positive surprises surface as **continuation-mode** candidates, prioritized where the revenue surprise agrees with the EPS surprise (post-earnings drift is markedly stronger when both point the same way). The beat-and-raise streak itself is confirmed per candidate at Step 5c from `earnings`. This feeder skews coincident / lagging by construction — a defect for early detection, exactly right for continuation.
  - **Corporate events** — M&A deal flow, 8-K material events, movers, recently priced and upcoming IPOs → fresh-catalyst candidates.
  - **Commodity-price turns** — over the Step-2 FRED / FMP series, a spot / contract-price turn at washed-out sentiment (CFTC positioning) surfaces commodity-cyclical candidates.
  - **No fundamental scoring here** — the multi-factor composite and the forensic reads are computed per candidate at Step 5c, on the narrowed set only; the activist (13D / 13G) and congressional feeds are symbol-keyed on the current plan, so they enter per candidate at Step 5b, never as discovery feeders.

- **Model**
  - None.

- **Output**
  - A broad candidate longlist.
  - Each name carries its feeder, its surfacing signal, its cap band, and its sector / industry.

---

### Step 3b — Model-led hypothesis discovery

The job's edge: a research-active feeder that forms investable **hypotheses** and reasons its way to names — *worldview → hypothesis → mechanism → value-chain node → leading metric → candidate* — so the model commits to a hypothesis before it commits to a ticker. It runs in three movements: a planning call chooses the routes, each route is researched in the shared loop, and each route's research is consolidated into a hypothesis document whose appendix the app checks on type and promotes from.

- **Data retrieved**
  - Leads — ticker-tagged, dated headline / snippet / URL rows from `news/general-latest`, `news/stock-latest`, and `fmp-articles`, plus the macro-release calendar from Step 2.
  - The house view, the opportunity graph with the watchlist review's decisions from this run, and the coverage ledger from Step 2.
  - Live web pages through the shared loop — **keyless SearXNG only**; a down SearXNG means this lane yields fewer candidates, never a keyed fallback.
  - FMP industry classification (`industry-classification-search`, `all-industry-classification`, `available-sectors`) and `stock-peers` to resolve a hypothesis to its exposed public names; the screener fields to verify each name (exists, US-listed, clears the tradability gate).

#### Movement 1 — Research-strategy planning

- **Call: route planning** (thinking, grammar, no web tool — once per run, before any route executes; planning spends none of the discovery budget)
  - **Sees (Part 1)**
    - MARKET ANALYSIS — the house view.
    - The carried-forward opportunity graph — each live hypothesis's document section with its decision, and the watchlist review's decisions from this run (promote / keep / retire); the departed tombstones included as dead theses. Rendered only when the graph holds a node; a first run renders no graph block, and no heading stands over absent content.
    - The route menu with each route's source-strategy rubric — *policy / regulatory* → legislation, agency notices, procurement databases; *supply chain* → trade journals, filings, customer / supplier commentary; *technical bottleneck* → standards bodies, engineering blogs, patent / product docs, trade publications; *procurement / grant / capex*; *customer capex*; *industry history*; *failure analogue*; *event-impact / value-chain repricing* → the announcing company's primary materials (spec sheets, reference designs, keynote / launch docs), standards bodies, teardown / engineering analysis, the affected names' segment disclosures.
    - The app-computed coverage ages per route class and per coverage subject, and any app-inserted overdue route. On a first run the empty ledger reads every unit due and the tie-break alone chooses the inserted route.
    - The run's route cap and discovery-budget posture.
  - **Sees (Part 2)**
    - The route list to return and its shape, and the hunt's end the routes serve — topics that carry the chain from the world-change to the operators that capture the margin and the leading metrics that would prove it — closing on a placeholder-only return shape.
  - **Returns**
    - A priority-ordered route list under the route cap — each route with its source strategy, selection rationale, `selection_origin` (`outside-view` / `coverage-rotation` / `model`), any coverage-unit ids it is expected to work, and its **topic list** — the focused questions the route is worked as, each topic one isolated conversation. The one place in the suite where the reasoner proposes an agenda's topics.
    - The app checks the list on type — route ids from the menu, the cap, the origin enum, non-empty topic strings — and nothing else.

- **App-enforced clauses (never model discretion)**
  - The **outside-view route** is always present and marked graph-blind — inserted if the model omitted it: "assume the carried-forward graph is stale: what world-change are we missing?" An inserted route carries that question as its single topic, the menu's outside-view rubric, an app-fixed rationale, and the `outside-view` origin.
  - The **coverage-rotation route** is app-owned: the model may refine its questions and source plan but cannot remove it, substitute a less-overdue unit, or claim its debt cleared; only the orchestrator's completed-route record updates the ledger.
  - The **event-impact route** may be chosen speculatively — its materiality gate is research-derived and unknowable at planning time, so it is checked at hypothesis formation (below). A scheduled route whose research surfaces no qualifying event emits nothing and stays dormant.

- **Coverage rotation — how the inserted route is chosen**
  - Age is tracked separately for each canonical route class and each coverage subject (the stable broad-industry taxonomy plus currently active house-view themes), in calendar time against the ~4-week window — never run count.
  - When debt exists the app pairs the oldest compatible overdue class + subject and inserts that route in the first slot after the outside-view route; ties break by canonical route order, then stable subject id; a model-proposed duplicate merges into the inserted route rather than taking a second slot.
  - A newly active theme starts due; a successfully completed route advances every unit it actually researched even when it correctly emits no hypothesis; a failed or budget-exhausted route records an attempt and does not clear its debt.
  - If the route cap leaves no slot beyond the outside-view route, the debt stays overdue and the run audit surfaces it — liveness is best-effort under the cap, never bought by dropping the outside-view guard.
  - The inserted route consumes the normal cap and budget. Coverage can force research, never a hypothesis, promotion, or opportunity.

#### Movement 2 — Route research (the shared loop, aimed at discovery)

- **The loop, as it runs here** (mechanics in *The research loop*, above)
  - Nesting is `route ⊃ topic ⊃ pass ⊃ fetch`: each route is worked as its validated topic list, each topic its own isolated conversation of a root pass plus at most two follow-ups, each pass a bounded tool loop of `web_search` / `web_fetch` requests the orchestrator executes, then a separate synthesis conversation writing the pass's write-up.
  - Two ceilings bind: per-topic depth (three passes per topic, counting passes, not searches, and not per route), and the **per-run discovery fetch + wall-clock budget** across every route, spent in route-priority order, fail-soft on exhaustion.
  - After a route's topics, one disconfirming pass runs for the route, from the same budget and outside any topic's depth.
  - The Step-2 house view steers the hunt without confining it. Routes never share a context.

- **Call: route research gathering** (thinking, tools, no grammar — once per topic on the route's topic list)
  - **Sees (Part 1, the route-constant block first, then the topic)**
    - The route header with the run date.
    - MARKET ANALYSIS — the house view, rendered as a market-level analysis.
    - PRIOR HYPOTHESES — the carried-forward graph's live hypothesis sections, each under a header carrying its id and date, and watchlist names relevant to the route, so the model extends or retires existing theses rather than re-deriving blind. Withheld on the outside-view route. Absent on a first run, when the graph holds no section.
    - The route's source-strategy rubric.
    - NEWS LEADS — the structured news and macro-release headlines relevant to the route, with their addresses. Fetch candidates only.
    - PAGES ALREADY RETRIEVED — the bounded route-scoped reuse block; absent on the disconfirming pass.
    - TOPIC, the topic's questions; on a follow-up pass FOLLOW-UP and WRITE-UP SO FAR; on the disconfirming pass WRITE-UPS SO FAR, the route's write-ups.
    - Before each gathering request the app appends a short user message stating the replies remaining in this pass; the brief and every previously issued message stay unchanged.
  - **Sees (Part 2)**
    - What to find — on a root pass the operators that capture the margin and the leading metrics that would prove it, beside the world-change the topic asks about; a follow-up pass its question alone and the disconfirming pass evidence against the write-ups, each naming that end only as what its question serves — how to weigh a source, the per-reply tool-call bound, and when to stop.
  - **Returns**
    - Tool calls until a reply carries none, or a bound.

- **Call: route synthesis** (thinking, no tools, no grammar — a fresh conversation per pass)
  - **Sees**
    - The route header and MARKET ANALYSIS.
    - EVIDENCE — every page the pass's gathering conversation carried, the reused pages first, each under the header `web_fetch` gave it.
    - The topic's own text.
  - **Returns**
    - The pass's write-up, 400–900 words: what the research established on the topic's questions, each figure with the date or period its source gives for it and that source, where pages disagree, and what the evidence leaves unanswered. On a follow-up pass the write-up rewritten whole.
    - A second message asks for a follow-up question: the question verbatim, or the one word `none`. Not sent on the topic's last pass or on the disconfirming pass.
    - Validated by nothing. A topic ends with one write-up; the route ends with one per worked topic plus the disconfirming pass's, flowing whole to hypothesis formation.
    - Every page shown enters the route's page roster, persisted on the audit record.

#### Movement 3 — Hypothesis formation: the hypothesis document and its appendix

One conversation per route, after the route's topic conversations and its disconfirming pass complete: the first model consolidation of the route's research, and the call that authors the route's hypotheses. The adversarial passes are the document's own discipline, not fetch passes.

- **Budget check**
  - The orchestrator sizes the hypothesis-formation prompt — the route's write-ups, the house view, the graph context — against the call's input budget. Over it, the write-ups are distilled under the shared shapes (*Consolidation*, above); the write-ups persist on the audit as written.

- **Call: the hypothesis document** (thinking, no grammar)
  - **Sees (Part 1, in page order)**
    - The route header; MARKET ANALYSIS; PRIOR HYPOTHESES (withheld from the outside-view route, absent on a first run); the route's source-strategy rubric.
    - WRITE-UPS — the route's write-ups, or their distillate, each under its topic's heading, the disconfirming pass's last.
  - **Sees (Part 2)**
    - The document to write: for each hypothesis the world-change, the mechanism, the economic value-chain trace (margin capture, bargaining power, capacity constraint, pricing power versus mere exposure — past the crowded pure-plays to the picks-and-shovels enablers at the constrained, margin-capturing nodes, often mid or small cap), the leading metric that would prove it with the kind of series it is, the likely public-company expressions, the bear case, the key falsifiers, the adversarial passes' answers — *why is this already priced? · why might the obvious beneficiary be the wrong expression? · who actually captures the margin instead?* — and the decision with its reasons — promote, watchlist, or none — in the model's priority order. The dimensions the task names are magnitude, durability, time horizon, leading-metric observability, crowding, and margin-capture clarity. For each hypothesis, whether it is a prior section shown updated, naming that section, or a new one.
    - For an event-impact route: the document is two-sided — beneficiaries, feared losers (the names that sold off, with the actually-exposed revenue / profit pool sized), and latent names (chain nodes that did not move but should be affected) — with each affected name's technology read sized in prose: the technical claim, the deployment timeline, substitute / complement / mix-shift, the affected workload, the exposed pool, the adoption constraints, the switching costs, the margin-capturing node, the source confidence, and the leading metric to monitor; and for a feared-loser name the symmetric pass — *is the impairment real or panic, and what is the actually-exposed pool?*
    - Within the hypothesis document's length band, drafted at plan time.
  - **Returns**
    - The hypothesis document as prose, read as text and validated by nothing, persisted on the audit and into the opportunity graph.

- **Call: the hypothesis appendix** (the same conversation's second message, thinking off, under a grammar — a transcription, not a judgment)
  - **Sees**
    - A request for the appendix as the document states it, closing on a placeholder-only return shape.
  - **Returns**
    - Per hypothesis: the title, the decision (`promote` / `watchlist` / `none`) in document order, the candidate symbols, `supersedes` — the id of the prior section the hypothesis updates, or null — the leading metric by name and its re-check class — chosen from the engine's series menu (`structured`), the standardized statement-line menu (`filing`), or free text (`research`) — and, for an event-impact hypothesis, each name's side (`beneficiary` / `feared-loser` / `latent`) and the announcement and corroborating condition.
    - The app keeps only the type check: the enums; a `supersedes` id among the ids the route's PRIOR HYPOTHESES block showed, so the outside-view route can return only null; each symbol resolving against FMP's industry classification (exists, US-listed, clears the tradability gate) before it can earn enrichment budget; and the presence of the gate fields on an event-impact entry — an entry that leaves them blank is dropped and logged. The materiality gate is checked on presence, never read. An off-domain value re-issues the message once.

- **Promotion (app)**
  - A promoted hypothesis's candidate names — each with its hypothesis link and surfacing rationale — flow into Step 4 alongside the screens and the watchlist, in the model's priority order.
  - Every hypothesis is written to the opportunity graph with its document section and appendix fields. A hypothesis decided `watchlist`, or `promote` without a validation slot, becomes a watchlist node (Step 3c).
  - A hypothesis that supersedes a prior section replaces that node's section and typed fields, the old section kept as dated history and the node's first-surfaced date standing; its watchlist companies re-link and take the new metric and class, the supersession advancing their refresh timestamps; a metric changed by name sends the old metric's print and write-up to history with the superseded section, the new metric taking its admission print as a new node's does and resetting the metric-named date the carry horizon counts from. Two in one run superseding the same node apply in route-priority order, so the node ends on the last.
  - The hypothesis documents are retained as run-level worldview context for the per-candidate thesis document (Step 5g), which carries the section a name expresses.
  - The model proposes hypotheses and names; it neither fetches per-symbol data nor scores them here.

- **Model**
  - The 122B reasoner — once for planning (thinking, grammar), per topic conversation in the loop (thinking), and once per route for the hypothesis document (thinking) and its appendix (non-thinking, grammar).

- **Output**
  - Promoted candidate names with hypothesis lineage → Step 4.
  - Every hypothesis in the opportunity graph; watchlist-decided hypotheses → Step 3c admission.
  - A completed-route record per route → the coverage ledger; the route's write-ups and page roster → the audit.

---

### Step 3c — Recheck the old watchlist

Discovery is stateful: every worthy-but-unpicked name from prior runs is a watchlist node, re-read here every run by the watchlist review — so a deferred name that quietly starts compounding is caught rather than left to chance. The step runs in three movements: the engine refreshes the prints, the review judges every node, and the refresh lane researches one.

- **Who is on the watchlist (typed facts alone)**
  - A node qualifies with a named leading metric with its class, and one of: a hypothesis decision of `watchlist`; `promote` without a validation slot; or an ordinary `gate-reject` at Step 5h — a debut no arm's gate admitted, never a hard-trigger exclusion and never a floor hold-out. The mechanism and falsifiers live in the document the review reads: the hypothesis section, or the thesis document for a gate-rejected debut.
  - Whether a gate-rejected debut's thesis is still worth watching is not a question the app can read off its prose; the next run's review decides it, typed, like every other node's fate.

- **Data retrieved**
  - The watchlist nodes from the Step-2 graph.
  - For a **`structured`-class** node — the engine's structured feeds: `analyst-estimates` (revision velocity), `earnings` (surprise), dated-EOD bars through the shared price-bar cache (relative strength), and the once-per-run FINRA file (short interest).
  - For a **`filing`-class** node — on the filing-cadence rider: when the swept `earnings` row shows a new reported period, the statement-derived rows re-pull (`income-statement` / `balance-sheet-statement` / `cash-flow-statement`, `key-metrics` / `ratios`, `financial-scores`, `financial-growth` — the rider is FMP-only; SEC submissions / company-facts stay per-candidate).
  - For a **`research`-class** node — nothing, unless the refresh lane selects it.
  - Evidence events tied to a watchlist name — a new item-classified 8-K or a material filing from the shared sweep.
  - The swept population is **one union** — every live matrix carry (Step 7's cheap re-derivation) plus every recheckable watchlist node — with cache / dedup applied per distinct symbol after the union; a `research`-class node never enters the per-symbol sweep.

- **Movement 1 — the engine refresh (no model)**
  - Every `structured`- and `filing`-class node's print is refreshed at its class's cadence; a failed refresh types that print `unknown`, never a fabricated value.
  - A node enters with no print of its own; the first print the refresh serves is its admission print, the then leg of the review's THEN AND NOW. A gate-rejected debut's admission print is the series its Step 5c pass computed.
  - Evidence events are collected per name.
  - A node — a company node, or a hypothesis node no `picked` company node holds live — whose **carry horizon** has elapsed — a configurable cap counted in the leading metric's own reporting periods (drafted ~4) from the node's metric-named date, never runs — retires regardless of the review.

- **Movement 2 — Call: the watchlist review** (thinking, no tools, no grammar; then a non-thinking transcription — once per DTO run, before route planning, over every live node)
  - Skipped when no node is live: no call over nothing, no review on the audit, and the refresh lane has nothing to select.
  - **Sees (Part 1, in page order)**
    - The run date; MARKET ANALYSIS — the house view.
    - Per node: its header (symbol, first-surfaced date, last refresh); HYPOTHESIS — the document section the node expresses, verbatim with its date; THEN AND NOW — the node's `structured` / `filing` prints at admission and as refreshed this run, glossed once, with any print the refresh could not serve stated as unknown and a node whose admission print has not yet been served stated as such; EVENTS — the evidence events tied to the name since its last refresh, if any; OBSERVATION — the node's latest refresh write-up where the lane has produced one, verbatim with its vintage.
  - **Sees (Part 2)**
    - For each node: whether the leading metric is confirming, whether a falsifier the hypothesis named has tripped by the numbers in front of it, whether the hypothesis survives, and the decision — promote, keep, or retire — with its reason. Within the review's length band, drafted at plan time.
  - **Returns**
    - The review as prose, read as text and validated by nothing, persisted on the audit record.
    - A second message, thinking off, under a grammar, asks for the decisions as the review states them, closing on a placeholder-only return shape: per node id one of `promote` / `keep` / `retire`. The app checks the ids and the enum and nothing else.
  - **What the decisions do**
    - **Promote** — the node enters Step 4 as a priority feeder (flagged *maturing watchlist*), so a maturing thesis is never missed for want of a fresh trigger. Promotion buys only normal Step-4 candidacy; every Step-5 floor, gate, and trigger still binds.
    - **Keep** — carried forward. Its refresh timestamp advances only on a successful evidence refresh — a served `structured` / `filing` print this run, or a refresh write-up from the lane, or a supersession of its hypothesis at Step 3b — never on the review alone; the review's own date is recorded separately.
    - **Retire** — removed from active monitoring, kept in history, with the review as its reason.

- **Movement 3 — the research-watchlist refresh lane (`research`-class nodes only; after the Step-3b routes, from the discovery budget they leave)**
  - Selects at most the configured number of nodes (drafted **one per DTO run**), deterministically and in priority order: a newly detected filing or material event tied to the node first, then the oldest successful research refresh, then ticker.
  - It is a discovery refresh, not a deep re-evaluation: it never stamps `last_deep_researched_at`, rewrites an opportunity record, clears an attention warning, or archives anything.

- **Call: the targeted refresh** (thinking, tools, no grammar; then synthesis; then a non-thinking transcription — one isolated bounded conversation per selected node on the shared loop, SearXNG only)
  - **Sees**
    - The node's header, MARKET ANALYSIS, HYPOTHESIS — its document section verbatim with its date — any evidence event from this run, and the topic's text: the node's named `research`-class leading metric and the falsifiers the hypothesis states, as the questions to pursue. No per-candidate dossier, no engine reads, no opportunity record.
    - The synthesis message follows the shared loop's form and asks for the refresh write-up — what current evidence says about the named metric and each falsifier, dated and sourced — and the decision with its reason.
  - **Returns**
    - The refresh write-up as prose, persisted on the audit and onto the node as its latest observation with its vintage.
    - The decision under the grammar — `promote` / `keep` / `retire` — applied as the review's are: a confirmed metric promotes the node into Step 4, a tripped falsifier may retire it, and no result or a failed call leaves it unchanged without advancing its refresh timestamp.
    - Searches are current; a cached document may help extraction but never substitutes for the live search that establishes the refresh.

- **Capacity logic**
  - The watchlist retention cap is enforced deterministically at add time, after the pruning above: when it binds, the node with the **oldest successful refresh** retires first, tie-broken by ticker, persisted reason `capacity-evicted`, graph history retained. No episode opens, since a watchlist node carries no prices.

- **Model**
  - None for the engine refresh.
  - The reasoner for the review (thinking, then non-thinking) and for the selected `research` node's refresh (thinking with the web tool, then non-thinking).

- **Output**
  - Promoted watchlist candidates → Step 4.
  - The review and its decisions, the refresh write-up where the lane ran, the retired nodes, and the lane's audit — every node considered, selected, or skipped, with its result and timestamp decision.

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
  - This is the cross-feeder reduce and it is deliberately computed: distinct hypotheses are never collapsed by a model, which would destroy auditable breadth and could silently drop a name. The only model-side consolidation in discovery is within a route's own hypothesis formation.

- **The deep-research budget (a Settings knob — how many names get the expensive Step-5 loop this run)**
  - Spent in three slices, in this order:
    1. **The rotation slice** — a configured share (default ~20%, never rounding below one slot) on **live opportunities** in maintenance-priority order: warning-bearing names first (a tripwire or continuation break never queues behind an uneventful stale name), then proximity to the next earnings date, then names near the entry threshold, then stalest by `last_deep_researched_at`. A **max-age service level** force-promotes any live name whose research age exceeds the configured bound whether or not it re-surfaced (ties by `became_opportunity_at`, then ticker). When more names are overdue than the slice can carry, the slice does **not** expand — the overflow forms an **overdue backlog drained stalest-first through a reserved overdue sub-slot**: each run's slice holds at least one slot for the backlog's stalest name, so a fresh warning outranks everything except that reservation, liveness is structural rather than best-effort, and the backlog's count and oldest research age surface in the run audit and the pre-run notice.
    2. **New names** — the remainder, filled under the diversity guardrails below.
    3. **Leftover** → re-surfaced existing opportunities, oldest `last_deep_researched_at` first.
  - **Diversity guardrails (new-name slice only)** — each a floor or ceiling, never a single ranking: mid / small-cap **floor ≥ 40%** of the new-name slots and a mega-cap **ceiling ≤ 30%**; **per feeder ≤ 50%** (no one screen, the hypothesis lane, the watchlist, or a positioning scan may supply more); **per (provisional) archetype ≤ 40%**; **per sector / theme ≤ 35%**. Within each floor and ceiling names rank by **signal strength × house-view fit**, ties resolving by ticker; hypothesis-lane names rank within their feeder by the model's priority order. Default allocation is **equal per cap-band × sector bucket**. The rounding rule, the relaxation order when a small slate makes the ceilings jointly infeasible, the multi-tag counting rule, and the ranking score's exact formula are **not yet drafted**.
  - Why stratify rather than rank: no universe-wide composite exists at this point (the fundamental score and forensic reads are computed per candidate at 5c), so a flat top-N on the cheap surfacing signals would collapse the funnel onto whatever is loudest — mega-cap momentum, the most-covered AI names, one crowded theme — and throw away the breadth that is the job's edge.
  - The **maintenance spend** (the rotation slice and the re-surfacer leftover) is **exempt from diversity** — a quota never blocks a warning-bearing or max-age-promoted name.
  - All three classes enter the **same Step-5 loop** as candidates; a rotation pick or re-surfacer is flagged carried-forward so Step 5b loads its prior record. A carried name's deep pass therefore runs in Step 5, before matrix assembly, not in Step 7.
  - Every ticker deep-researched — rotation, new, or re-surfaced — is recorded in the **run-scoped deep-research set**, so Step 7 never also cheap-sweeps it: at most one deep pass per ticker per run.

- **Deferred names**
  - Not rejected — nothing is validated yet. A genuinely worthy deferral (a hypothesis the document decided `watchlist` or `promote`, with a named leading metric) is already a watchlist node and is re-read every later run; a name not worth watchlisting carries no state and is simply re-derivable. A re-surfacer that wins no leftover slot falls to the Step-7 cheap re-derivation, its re-surfacing raising the attention warning so the user can choose a Deep Audit.

- **Model**
  - None.

- **Output**
  - The narrowed candidate slate — debuts plus carried names — each with its tags and provisional archetype, receiving Step-5 validation.
  - The run-scoped deep-research set, and the rotation backlog record.
  - The budget bounds how many names get **researched**, never how many validated opportunities reach the matrix — the gates alone set that (Step 6).

---

## Step 5 — Deep validation loop

The following sequence runs once for every candidate on the Step-4 slate — debuts and carried names alike. The loop's order is deliberate: the engine and its floor run before any research is spent, the research meets the engine's reads only at the thesis document, and the entry gate runs on both arms after the model has written.

- **Checkpoint and resume**
  - Each candidate's completed stages persist (the checkpoint is written at Step 5h), so a cancellation or a single model failure resumes the unfinished candidates rather than restarting the run.
  - Resume reopens the interrupted run's id and pins everything upstream of the loop — the Step-2 context, the Step-3 feeder outputs, the watchlist review's decisions and the route plan, the Step-4 slate with its budget allocation and rotation backlog, every source's as-of timestamps, and the model and prompt versions — for a drafted ~48-hour window; past it, Discover starts a new run. A new Discover run discards any interrupted run's checkpoints. The document cache survives independently.

- **One model, switched by mode**
  - The resident 122B reasoner fills every model role in the loop — archetype confirmation (thinking, grammar), the research passes and write-ups (thinking), distillation (non-thinking, only where over budget), the analysis (thinking), the review (thinking), the thesis document (thinking) and its appendix (non-thinking, grammar) — so moving a candidate through them pays no model-swap cost.

- **The engine is shared with Portfolio Analysis**
  - The same Rust financial-analysis engine computes every number; the difference is the **archetype**, which selects which signals it weights and which valuation lens it applies. Everything it produces is the **engine arm** — a disclosed baseline the model reads as evidence at Step 5g and against which it authors its own arm. Nothing the model returns alters an engine value.

- **Lifecycle id**
  - A debut entering the loop takes its lifecycle id here — the one its ticker's live watchlist node carries, else a fresh one, never one read from an archive row, a tombstone or an earlier episode; a carried name keeps its own.

### The model calls at a glance

| Call | Step | Mode | Returns | App check |
|---|---|---|---|---|
| Archetype confirmation | 5a | thinking, grammar | label + rationale | label on the enum; an invalid or failed call adopts the provisional archetype |
| Research gathering | 5d | thinking, tools, no grammar | tool calls, then a reply with none | none |
| Synthesis (write-up) | 5d | thinking, no tools, no grammar | prose write-up; a follow-up question or `none` | none |
| Distillation | 5e | non-thinking, no grammar | shorter prose | none |
| The analysis | 5e | thinking, no grammar | prose | none |
| Self-review | 5f | thinking, no grammar | prose | none |
| Thesis document | 5g | thinking, no grammar | prose | none |
| Typed appendix | 5g | non-thinking, grammar | tier, horizon, conviction, three prices, detection mode, leading metric + class, status on a carry | the enums; each price finite and positive; reissue once |

- **Rules every call shares**
  - One message in two parts: data with glosses, then the task. A typed call closes on a placeholder-only return shape.
  - The deadline derives from the call's own reservations (context over a prefill floor, plus output over a decode floor on the non-streaming path), never a fixed backstop.
  - A transient failure (connection, daemon error, empty body, broken stream, schema parse, an off-domain appendix) re-attempts exactly once. A deadline trip, a length stop, a cancel, or anything unclassified fails on first occurrence. A second failure fails the run; the per-candidate checkpoint lets a resume pick up the unfinished candidates.
  - Every fired retry is evidence: a tracker row plus a data-health event.
  - One context size per model; context pressure is answered by compressing documents or picking a smaller shape, never by raising it.
  - Prompts put the candidate-constant text first and keep the per-conversation text short, so consecutive conversations on one candidate reuse the server's prompt cache.
  - Sampling: thinking calls at temperature 1.0, top-p 0.95, top-k 20; non-thinking calls at 0.7, 0.8, 20. Greedy decoding is forbidden.
  - A grammar-carrying transcription runs thinking off on the pinned Ollama version; an Ollama version bump re-locks such calls to thinking-enabled until the schema-integrity check passes on the new version.

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

- **Call: archetype confirmation** (thinking, grammar, no web access — a classification, not research)
  - **Sees**
    - The classification features and surfacing signals only — a compact, clean context; for a carried name, the prior label beside them.
  - **Returns**
    - One archetype label under the grammar — secular compounder / AI-infra / commodity cyclical / disruptor / quality compounder — plus a short rationale as prose.
    - The branch is total: one authoritative archetype always emerges. The label stands for 5c–5h; its doubt and any runner-up lens live in the rationale, which rides the audit only, never a second lens in targets, weights, or gates.
    - A schema-invalid confirmation or a failed call adopts the Step-4 provisional archetype as authoritative, flagged and logged — a deterministic fallback, never a model retry.
    - For a carried name the rationale affirms or overturns the prior label, and the prior and current labels both persist on the record, so a lens that churns between passes on a borderline name is visible on the card and the audit rather than policed.

- **What the archetype decides downstream**
  - The composite's **signal weighting** and the **valuation lens** for 5c–5g: a commodity cyclical is judged on P/B, P/NAV, and mid-cycle EPS with trailing P/E suppressed; an AI-infra name on segment-revenue acceleration and forward P/E against its revision rate; a secular compounder on PEG and revisions-vs-multiple; a disruptor on its leading operating metric rather than EPS; a quality compounder on operating-income decoupling with valuation as a risk gate, not an entry.
  - The **target driver override** on the shared scenario-target function (Step 5c).
  - The **engine horizon rule's Long branch** — a compounder archetype earns Long (Step 5c).
  - The archetype's **track** at the thesis document — proven economics or emerging economics (Step 5g).
  - The **engine risk tier takes no archetype input** — any archetype–tier correlation is emergent through the rule's measurable legs.

- **Output**
  - The authoritative archetype and its rationale; on a carry, the prior label beside it.
  - The cached classification responses, reused by Step 5b.

---

### Step 5b — Build the candidate dossier

The application assembles the candidate's evidence packet deterministically; the Step-5a responses are reused from the run cache, and every per-candidate row fires once. This is the per-candidate surface — the budget driver — so it runs only for the narrowed slate, never the discovery longlist.

- **Data retrieved from FMP**
  - Fundamentals — `income-statement` (+ TTM), `balance-sheet-statement`, `cash-flow-statement`; `key-metrics`, `ratios` (+ TTM); `financial-scores` (Altman Z, Piotroski); `owner-earnings`, `enterprise-values`, `discounted-cash-flow`; `financial-growth` (multi-year per-share revenue / EPS / FCF / book-value CAGRs); `dividends` — the trailing distributions used as the bands' twelve-month payout proxy, not a forward estimate (a nonpayer contributes a clean zero; a failed pull reads zero with a recorded degraded-input gap).
  - Segments — `revenue-product-segmentation`, `revenue-geographic-segmentation` (annual only — trajectory context and the engine's segment-acceleration series).
  - The revision signal — `analyst-estimates` (forward consensus, snapshotted run to run for velocity), `grades` / `grades-historical` / `grades-consensus` (the rating distribution and actions), `price-target-consensus` / `price-target-summary` (street target level and trend), `ratings-snapshot` / `ratings-historical`, `earnings` (the next earnings date — the rotation slice's proximity key, never an engine-horizon input — and the actual-vs-estimate history).
  - Positioning (all symbol-keyed) — `insider-trading/search` + `insider-trading/statistics`, `acquisition-of-beneficial-ownership` (SC 13D / 13G activist stakes), `senate-trades` + `house-trades`.
  - `stock-peers`, `shares-float` (free float / liquidity), optionally `historical-employee-count` + `key-executives`.
  - `news/stock` — the symbol-scoped headline feed, the candidate's news leads for Step 5d.
  - M&A involvement — acquirer or target, matched from the market-wide `mergers-acquisitions-latest` + `sec-filings-8k` (the per-symbol M&A search is off-plan); an announced transaction with a dated close is the engine horizon's Short input.
  - `quote` — the live price the engine prices targets and runs the gate against (a job-time input, logged in the audit; never a persisted current-price field).
  - `historical-price-eod/light` (dated) — the deep daily history, through the shared price-bar cache.
  - Off-plan, and where they go instead: earnings-call transcript language (backlog, book-to-bill, guidance, supply discipline) → the Step-5d research lane; press releases → `sec-filings-8k` + the web; 13F institutional flow → SEC EDGAR 13F (coarse, often omitted) and held out of the grade regardless.

- **Data retrieved elsewhere**
  - SEC EDGAR — the submissions feed (10-K / 10-Q / 8-K, item-classified) and XBRL company facts as the authoritative cross-check; for an eligible new listing or separation, the S-1 / Form 10 history. Ticker → CIK resolution is non-blocking here: an unresolved CIK degrades the EDGAR legs to the FMP working feed and reads the filing-kind forensic legs `unknown`.
  - FINRA short interest — looked up in the once-per-run file (level, trend, days-to-cover).
  - The Schwab option chain, if the name is optionable — per-contract volume, open interest, implied volatility → the options-activity signal computed at 5c; a chain failure is a typed gap, never a failed run.
  - The Step-2 shared context.

- **Logic**
  - Cross-check the FMP working feed against SEC filings and assemble one evidence packet.
  - **Limited-history eligibility** — before research, derive a mode from objective identity facts only: `new-listing`, `spin-off-carve-out`, or `new-economic-perimeter`, and only when that event is why comparable public history is short; a missing provider response never creates eligibility, and the model never chooses it. For an eligible candidate the dossier carries the filing identifiers and source plan the reconstruction topic deep-reads (S-1 or Form 10 historicals, carved-out or predecessor segment disclosures, contracts, customer / supplier observations, dated operational / technical milestones).
  - **For a carried-forward candidate** (a rotation pick, a budget-winning re-surfacer, or a Deep Audit selection): load the prior record deterministically by lifecycle id — the prior pass's date and the price then, its thesis document verbatim, its analysis verbatim, its appendix values (tier, horizon, conviction, the three expected prices with their horizon dates, status, detection mode), its prior archetype, and the lifecycle's accuracy scores from Step 2. A name re-entering from the archive is a new lifecycle and carries none of it.

- **Model**
  - None. The job makes no embedding call; continuity is the deterministic load above.

- **Output**
  - The complete candidate dossier — statements, ratios, scores, growth, segments, the revision signal, positioning, float, news leads, price history, the live quote, the option chain, the SEC cross-check, FINRA short interest, the shared context, any limited-history eligibility + source plan, and on a carry the prior record.

---

### Step 5c — Calculate the financial picture, and the evidence floor

- **Data retrieved**
  - Uses the dossier and the shared context. No model or web research.

- **How the step runs**
  - The deterministic engine computes the candidate's quantitative picture **weighted by the Step-5a archetype** — the archetype selects which sub-scores dominate and which valuation lens applies. Everything it produces is the **engine arm**, carried into Step 5g with its methodology exposed (the `TargetMeta` derivation flags, the driver rung taken, the normalization basis, any neutral-midpoint imputation) so the model can dispute a *derivation*, not merely a number.
  - The evidence floor runs here too, over the same structured data, before any research is spent.

- **Order of computation**
  - Quant composite → value-creation read → leading-metric series and its inflecting read → earnings surprise and positioning → price-action confirmer → scenario bands at three horizons → the three derived reads (narrative-vs-reality, forensic, implied expectations) → risk tier and horizon → tradability flag → for a carried name, the since-flagged read and the realized block → the evidence floor.

#### Engine primitives (used below)

- **`scale(x, lo → hi)`** maps `x` linearly onto 0–100 and clamps it; an inverted band (`lo > hi`) scores lower inputs higher. In this job the bands are **sector-adjusted** — a per-sector `lo → hi` per factor.
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
  - It is deliberately **not** a within-cohort rank: a within-run cohort has no statistical mass (the quotas spread the slate to one-to-three names per bucket), ranking against live peers would multiply the per-symbol budget, and the persisted **factor-distribution store** is a selected convenience sample — so that store is **diagnostic-only, never a score input** (shown in the dossier / audit as context once a bucket holds ≥ 20 unique issuers, drafted; it graduates into the score only when fed by a representative-universe snapshot).
  - A factor that failed to resolve is **imputed to its band's neutral midpoint** (disclosed), so a missing call can't silently sink a name; the factors integrate at **one final composite score** rather than chained hard cutoffs — value and momentum are negatively correlated, so sequential cutoffs would collapse breadth.
  - **Archetype weights** — the archetype's dominant axes tilted ~2× over a neutral baseline: AI-infra → momentum / revision; commodity cyclical → valuation (P/B, P/NAV); quality- and secular-compounder → quality / ROIC; disruptor → growth / leading metric; the remaining axes balanced.
  - A composite resting on thin own-history carries a **low-confidence degraded-input flag** — a data-health note the model reads, never a gate.
  - Where it lands: the engine arm's **sub-scores** and composite, on the shared 0–100 scale, and the value lens's read for the thesis document.

#### Value-creation read

Whether the business *creates* value rather than just growing.

- **ROIC vs cost of capital** — the spread; growth funded below the cost of capital destroys value however fast revenue grows.
- **Owner earnings with R&D capitalized** — so research-heavy leaders aren't mis-scored as unprofitable.
- **Reinvestment runway** `g ≈ ROIC × reinvestment rate`.
- **Moat-source features** — intangibles, switching costs, network effects, cost advantage, efficient scale — weighted by how often each actually sustains a durable advantage.
- Inputs: the statements, `key-metrics` / `ratios`, `owner-earnings`, `financial-growth` (the multi-year per-share CAGRs → growth trajectory and the runway read).
- Where it lands: the engine arm; the engine horizon rule's Long branch and the gate's **H** (a multi-year reinvestment runway).

#### Leading-metric series and the inflecting read

The anchor the whole thesis hangs on — per archetype: revision velocity (AI-infra), segment-revenue acceleration from the annual segment rows (AI-infra / disruptor / secular compounder), a commodity-price turn (commodity cyclical), margin decoupling — operating income growing faster than revenue (quality compounder).

- **The series** — built from the structured and filing-cadence feeds, dated provider data by construction. A family that lives only in research — product ASPs, channel checks, net-adds — has no engine series: the leading-metric topic's write-up states the dated observations with their sources, and the thesis document judges the inflection in prose.
- **"Inflecting" is metric-family-shaped, archetype-mapped, two-phase, noise-floored** — the shape test belongs to the family declared on the metric (a renewal rate is a stability-family metric whoever holds it); the archetype maps to the family its tell is expected to live in:
  - *Accelerating* (segment revenue, revision velocity, net-adds) — the rate of change rising.
  - *Turned-and-holding* (commodity prices, ASPs) — a positive move after a declining stretch, or holding above a turn ≤ 4 reporting periods back; never re-demanding re-acceleration.
  - *Stability-under-stress / decoupling* (a renewal rate through a price hike; operating income outgrowing revenue with the gap widening).
  - *Threshold-crossing* (a first profit, an FCF turn).
  - *Deterioration* — the exit shape the continuation state watches for.
- **Comparability** — changes are seasonally comparable (YoY for quarterly reported series, never QoQ on seasonal data); trend is a robust slope over the available points, never the latest two deltas alone.
- **Continuation phase** (a continuation-mode candidate, or any re-check of an already-inflected anchor) — inflected within the trailing ~4 reporting periods and not rolled over; steady post-inflection strength passes.
- **Noise floor and minimum history** — a qualifying move must exceed `0.5 × σ` of the series' trailing comparable changes (up to 8); *accelerating* / *turned-and-holding* need ≥ 5 comparable changes, *stability-under-stress* the full stressor window, *threshold-crossing* the crossing plus one confirming observation. Below its family minimum the series is **unmeasurable**: where the family is the floor-bearing one the candidate abstains on the evidence floor as missing evidence (the limited-history branch waives the leg).
- **The not-inflecting read** — a measurable family that fails its shape test. A debut is held out at the floor below as a story stock; a carried name proceeds with the read as an engine annotation the review and the thesis document must confront, with or without `hype`.
- **Direction** is by declared metric polarity — a narrowing loss counts as improvement.
- Where it lands: the leading-metric series, trend, and continuation state on the engine arm; the evidence floor; the thesis-document prompt.

#### Earnings surprise, positioning, and the price-action confirmer

- **SUE** — each reported surprise standardized against the name's own surprise history (from `earnings`); the beat-and-raise streak confirmed here; post-announcement drift as a continuation tell.
- **Positioning** — insider net buying (`insider-trading/*`), congressional buys, activist 13D / 13G stakes, short-interest level / trend / days-to-cover (FINRA), CFTC positioning for a commodity cyclical's underlying (Step 2), and the **options-activity signal** from the Step-5b chain — put/call by volume and by open interest, and the IV/skew read (mean put IV minus mean call IV over the whole chain) — an activity proxy, **held out of the grade** until calibrated.
- **Price-action confirmer** — relative strength vs the market (`^GSPC`) and the sector benchmark, and proximity to a multi-year base breakout, from the dated-EOD deep history (reusing the shared engine's momentum / volatility computations). A cross-archetype **confirmer, not a trigger** — it adjusts conviction in the thesis document, never substitutes for the leading-metric anchor.

#### Scenario bands — the rate-anchored function at three horizons

Bear, base, and bull prices at three months, twelve months, and three years, priced from a per-share driver and a rate-anchored multiple. The function is the one Portfolio runs — restated compactly here; the clamp, repair, and fallback detail is at `portfolio-analysis-logic-flow.md` §Scenario bands — with one Trade Opportunities leg: the **archetype driver override**. Computed from structured data alone; nothing the model returns alters them.

- **Choose the driver** — the archetype names the preferred driver / multiple form, the shared ladder's fallback discipline applying when it isn't computable:
  - Quality- and secular-compounder → consensus forward EPS / P-E.
  - AI-infra and disruptor → forward revenue per share / P-S while pre-profit — the ladder climbs back to EPS once a positive EPS consensus exists.
  - Commodity cyclical → **mid-cycle EPS** = median margin over the trailing cycle window (drafted ~5 years) × forward revenue per share, / P-E — spot earnings mislead at cycle extremes (the archetype's P/B / P/NAV read stays a scoring lens, never a target path).
  - Every forward per-share conversion reads the ladder's one share basis — the latest reported diluted count; the anchor window's historical prints take a diluted count from inside their own TTM window.
  - No positive forward-EPS consensus and no computable forward revenue per share → `no-admissible-driver`, an evidence-floor abstention (the gate cannot price a name with no computable target).
- **Build the three driver cases** — base = the consensus mid, bear / bull = the low / high (a missing spread holds both at the mid, flagged flat), each clamped to `[trailing × 0.75, trailing × 1.35]`.
- **Calculate the multiple** — per historical quarter (~12): driver yield = `driver ÷ price`, spread = `yield − that quarter's DGS10` (latest on or before); the bear / base / bull spread percentiles (75th / 50th / 25th — a wider spread is a cheaper multiple); re-anchored with today's `DGS10`: `multiple = 1 ÷ (spread percentile + today's DGS10)` (needs ≥ 8 observations; else raw-multiple percentiles; with no history, the current `spot ÷ base driver` multiple carried).
- **Twelve-month band** — price = driver × multiple per scenario (crossed scenarios repaired to ascending; a volatility-scaled dispersion floor widens, never narrows, the bear / bull spread); total return = `(price + trailing-TTM dividends per share) ÷ spot − 1` (the payout proxy comes from the 5b-pulled trailing distributions; a nonpayer contributes zero).
- **Three-month band** — base = spot × (1 + the twelve-month base price return ÷ 4), so the twelve-month band is computed first; half-band = daily volatility × 2 × √63, clamped [3.5%, 26%], 8.7% when volatility cannot be computed.
- **Three-year band** (declared extrapolation) — each scenario's twelve-month driver compounded two further years at the growth the two coming fiscal-year rows imply, under the same clamp, times the same scenario multiple; a single forward row holds growth flat, recorded; the dispersion floor scales by √3.
- **Where these land** — the bands with each horizon's method clause and the `TargetMeta` provenance flags (anchor form: rate-anchored / current-multiple carry / raw-percentile fallback; driver rung; flat / clamp / dispersion flags) on the engine arm → the thesis-document prompt, the card's expand view, and the entry gate (5h); the base value at each horizon → the episode store, as the engine's forecast; the anchor-window percentiles and drivers persist as the basis every cheap re-derivation re-anchors against.

#### The three derived reads the interpretation leans on

- **Narrative-vs-reality ratio** — estimate-revision pace vs multiple change over a trailing 12-month window, or since `became_opportunity_at` when the idea is younger: *justified-expensive* when estimates outrun the multiple; **`hype`** when multiple expansion exceeds **70%** of the price move. For a thinly-covered name whose estimates are absent or stale, the numerator falls back to **operating reality** — the hard operating momentum the company itself reports (segment revenue, gross profit, the archetype's structured leading metric) against the move in price and multiple. Where it lands: an engine-arm read the thesis document weighs. With a measurable, inflecting family behind it, `hype` is an annotation; **anchorless `hype`** — the read with the archetype's structured family measurable and not inflecting — is an annotation too on a carried name, the model's status standing and a `still-valid` status against it recorded as the narrative divergence at Step 5h (a debut with that family read is held out below, before research). Where the archetype's tell lives only in research the engine cannot read anchorlessness, and `hype` is an annotation alone.
- **Forensic reads** — computed from the statements, `financial-scores`, and the filings:
  - **Soft flags** — Altman Z < 1.8; Piotroski ≤ 3; net income > 1.3× operating cash flow; receivables / inventory growth > 1.5× revenue growth; margin compression while revenue accelerates. Persisted as engine annotations the thesis document weighs; they clamp nothing.
  - **Hard forensic state** — a restatement (an Item 4.02 non-reliance filing) or an auditor change (an Item 4.01 filing) in the item-classified SEC 8-K submissions within the drafted 365-day lookback. Engine-detected, model-free. An unresolved CIK reads these legs `unknown` — a logged degraded input, never a fabricated clear and never a silent no-event. A fraud allegation has no filing leg: it reaches the model only through the write-ups and the analysis, as prose it weighs, and never trips the state. Consequence: a hard trigger at Step 5h.
- **Implied-expectations read** — the scenario math inverted at the live quote under the archetype's driver override: the driver growth the spot implies across the scenario multiples, plus the margin dimension where the driver is revenue-based → a **range** of growth / margin trajectories the price already assumes under stated assumptions, never one solved number (many combinations justify a price). Where it lands: the engine arm — the computed anchor for the thesis document's priced-in / crowding judgment.

#### Risk tier and horizon (the engine arm's placement legs, assigned here)

- **Risk tier** (rule-derived; no archetype term — the gate's scale and the baseline beside the placement)
  - **High** if any: market cap < $2B · realized volatility > 40% · debt/equity > 2 · unprofitable · drawdown > 50% · illiquid (thin ADV / high Amihud) · high event exposure.
  - **Low** if all: market cap > $10B · profitable · debt/equity < 1 · volatility < 25% · liquid.
  - Otherwise **Medium**.
  - A leg whose input is missing cannot trigger; a candidate whose tier inputs are wholesale missing reads **Medium with a logged tier-input gap** — never a fabricated High or Low.
  - The exact predicates for `illiquid` (the band boundaries from the tradability flag's own inputs) and `high event exposure` are **not yet drafted**.
- **Horizon** (a drafted rule over the archetype and the dated events the feeds carry, no model input, evaluated in order)
  - **Short** when the name is party to an announced transaction with a dated close inside 3 months — read from the M&A feed and the item-classified 8-K sweep, never from an earnings date, since every name has one.
  - Else **Long** when the archetype is a compounder (quality or secular) or the value-creation read's reinvestment runway is multi-year (ROIC above its cost of capital with a reinvestment rate that compounds).
  - Else **Mid**.
  - The matched branch persists as the horizon's **derived basis** (`transaction-close` / `multi-year-compounding` / `default`) and sets the gate's **H** (Step 5h).
- Both ride the self-review and thesis-document prompts as computed reads, feed the Step-5h gate's shared legs, and persist beside the model's placement as the baseline. The model's own tier and horizon are separate fields it argues in the thesis document; they place the card and never enter the gate.

#### Tradability flag, and the carried name's reads

- **Tradability flag** — Amihud-style illiquidity plus days-to-cover, resolved into the entry gate's **banded liquidity haircut**: unflagged 0 / flagged −3 pts / severely flagged −6 pts, the band and its points computed from the flag's own inputs — so a small illiquid name is discounted, not silently excluded.
- **Since-flagged read (carried names only)** — the price legs — running return since `became_opportunity_at` (absolute, vs sector, vs market) and maximum drawdown over that window — reconstructed from the dated-EOD history, from the **first daily close after `became_opportunity_at`** (a consistent evaluation anchor, never the same-day bar the decision could not have traded) to the latest cached close, split-adjusted and price-only (total return supplementary where dividend data exists). The sector leg reads the record's entry-stamped sector identity and its resolved SPDR benchmark, refreshed at each deep pass; a record with no valid mapping carries `sector-unscorable` for that leg while the absolute and market legs compute. The read's other part is the continuation state of the engine's metric family where it is structured or filing class. A brand-new pick carries none on its debut run.
- **Realized block for the self-review (carried names only)**
  - The price now and its move since the prior pass, absolute and vs sector / market, with the maximum drawdown since the name became an opportunity.
  - For each prior expected price: whether its horizon has passed, and if so the close on that date with its accuracy score, else the path so far.
  - The lifecycle's six accuracy scores, each with the date of the last check that moved it and whether the prior analysis read it.
  - The checks written since the prior analysis was written, this run's Step-2 checks included, newest by horizon date, up to 12.
  - The engine's own composite, targets, tier, horizon, narrative-vs-reality and forensic reads then and now, with any hard-forensic or anchorless-`hype` read stated as a fact; the metric family's continuation state where it has one.
- **Split bridge**
  - Each deep pass stamps an anchor bar: the newest settled close strictly before the run's session. A later read divides the fresh close at that date by the stored one; the factor snaps to 1.0 inside a 10% deadband.
  - The prior expected prices, the prior spot, and episode prices convert through the factor, each labeled bridged. A verbatim document (the prior analysis, the prior thesis document) is never rewritten; a split-context line above it states the split's date, ratio, and factor.
  - An anchor missing from the fresh window excludes the comparison, typed; nothing runs cross-basis.

#### Evidence floor

Runs here, over the same structured data, before any research is spent: every leg is a read over structured data and its absence, so a debut that abstains has cost no model call. Archetype-aware, because an early-stage disruptor legitimately carries thinner financials than a quality compounder.

- **Freshness states** (typed per floor-bearing input: `fresh` / `stale` / `freshness-unscorable`, with named gap reasons)
  - Quote / price bars — current through the latest completed session (the job-time fetch self-satisfies).
  - Structured / filing metrics and financial statements — an observation for the latest *expected* reporting period, with a drafted ~45-day filing grace past period end; older than one full period + grace is stale.
  - `freshness-unscorable` (the source carries no as-of) reads as a degraded input, not an abstention, unless the input is itself floor-bearing. Distinct from source-quality recency, which only weights.

- **Floor-bearing for every candidate**
  - A current quote and price history.
  - The engine's leading-metric family measurable and fresh, where the archetype's tell lives in a structured or filing-cadence series; an archetype whose tell lives only in research has no floor leg for it.
  - Source freshness on the floor-bearing inputs, per the basis above.

- **Statement floor, archetype-substituted**
  - A proven-economics archetype (quality / secular compounder, commodity cyclical): the financial statements (FMP / SEC) — absent, stale, or carrying conflicting identity ⇒ abstain.
  - An emerging-economics archetype (early disruptor, pre-profit AI-infra): the structured operating rows the feeds carry — the revenue segments, gross profit, and the revision signal — stand in, sufficient to anchor the engine's targets. The substitution is explicit and logged; it relaxes the *form* of the supporting financials, never the leading-metric requirement. The operating metrics only research carries (backlog / bookings, net-adds, cohort retention) reach the model through the write-ups.

- **Limited-history branch** (an app-eligible `new-listing`, `spin-off-carve-out`, or `new-economic-perimeter` only)
  - The family test is unmeasurable by construction on a post-event series, so the leading-metric leg is **waived**: the metric's inflection is the thesis document's judgment from the reconstruction write-up, never a story-stock hold-out, and the not-inflecting read does not arise.
  - The statement leg requires ≥ 1 full reported period since the event (drafted) — the proven-economics statements, or the emerging-economics substitute rows — else the candidate abstains as missing evidence.
  - What research recovers reaches the model in prose only and never enters the engine's series or substitutes for a statement; limited history never changes archetype, risk tier, required return, or gate math. The thin-own-history degraded flag rides the composite.

- **Enriching inputs (never force abstention)**
  - Estimates / revisions, positioning, peers, the options signal, analyst opinion, the dividend distributions (a failed retrieval; a confirmed nonpayer's clean zero is no gap). Their absence is recorded as a degraded-input flag the thesis document reads.
  - One named carve-out: `no-admissible-driver` — no positive forward-EPS consensus and no computable forward revenue per share — abstains, because the entry gate cannot price a name with no computable target.

- **Three outcomes for a debut**
  - A name missing a floor-bearing input or carrying stale or conflicting data → **`insufficient-evidence`**: an abstention held out of the matrix, with named reasons.
  - A name whose structured metric family is measurable and **not inflecting** → a **story stock** (`no-inflecting-metric`): the engine's affirmative read, the leading-metric non-negotiable holding before the model is asked.
  - A name clearing both → research.
  - Neither hold-out spends a model call, and neither opens an episode. The hold-out is checkpointed as completed; its reasons and freshness states persist on the audit.

- **A carried live opportunity**
  - Missing or stale evidence → an **inconclusive refresh, never a turn-away**: the name holds its last verdict, conviction, and matrix identity; a typed refresh gap is recorded; no research is spent; the pass stamps no `last_deep_researched_at`, clears no attention warning, and opens no episode — it stays exactly as stale and as flagged as it was, so the rotation slice keeps prioritizing it. Whatever engine-only fields this step could compute refresh under the cheap re-derivation's rules.
  - A metric family reading not inflecting → not inconclusive: evidence is present, so the deep pass proceeds with the read as an engine annotation — anchorless `hype` and a flat metric without `hype` alike reads the review and the thesis document must confront, a `still-valid` status against anchorless `hype` recorded as the narrative divergence at Step 5h.

- **The floor is arm-independent and binds absolutely.** Either-arm admission is scoped to the entry gate alone; a candidate below the floor abstains however conviction-bearing the model's arm would have been. The floor and the hard triggers are reads over *facts and their absence*, not judgments about the future, so there is no second arm for them to have.

- **Model**
  - None.

- **Output — the engine arm**
  - The archetype-weighted sub-scores and composite (with the normalization basis and any neutral-midpoint imputation disclosed), the value-creation read, the leading-metric series / trend / continuation state and the family's inflecting read, SUE, positioning and the options signal, the price-action confirmer.
  - The bands at three horizons with their `TargetMeta`, the narrative-vs-reality read, the forensic annotations and the hard forensic state, the implied-expectations range, the risk tier and the horizon with its derived basis, the tradability flag and its haircut band.
  - For a carried name, the since-flagged read, the realized block, and the anchor bar.
  - The floor's freshness states and degraded-input flags; or a typed hold-out record.
  - Nothing the model returns downstream alters any of these values.

---

### Step 5d — Research the company

The shared research loop (*The research loop*, above), aimed at one candidate. This is the only stage in the per-candidate loop that itself loops; it builds the three research lenses — and the mandatory bear case — around the fetched values, so research fills the gaps the numbers leave rather than substituting a story for them. The engine's reads are not in the brief; the research meets them at the thesis document.

- **What differs here**
  - Unit: the candidate's agenda below; one isolated conversation per topic.
  - Budget: a **per-candidate** fetch + wall-clock budget, spent in topic-priority order — **leading metric and bear case first**, so the bear case is never the topic the budget drops — fail-soft on exhaustion. Failed-fetch memory spans all candidates of the invocation; each automatic retry spends a separate attempt.
  - Search: **SearXNG only**.
  - Leads: the candidate's `news/stock` headlines from the dossier.
  - Terminal consolidation: Step 5e.

- **The agenda (assembled deterministically by the orchestrator)**
  - **Leading-metric validation** (mandatory anchor) — confirm the leading metric is real, countable, dated, and inflecting from a third-party source — the metric the name arrived with: the hypothesis's named metric for a hypothesis-sourced name, the watchlist node's for a name promoted from the watchlist, the prior thesis document's for a carried name, the archetype's metric family for a screen-surfaced debut; for a metric the feeds never carry, this topic deep-reads the name's own 10-Q / press-release disclosures and the write-up states each period's dated, sourced observation, which the thesis document judges.
  - **Limited-history reconstruction** (conditional — only when Step 5b marked the candidate eligible) — read the identified S-1 / Form 10 / carve-out / predecessor disclosures and third-party operating evidence, recover dated observations, and state for each whether it is directly comparable, requires a disclosed recast, or is only a proxy; never treat a customer / supplier proxy as the issuer's revenue or merge unlike economic perimeters. Nothing it recovers enters the engine's series.
  - **Macro / thematic fit** — which theme the name rides and where it sits on the S-curve (against the Step-3b hypothesis documents), pure-play vs enabler at a margin-capturing, capacity-constrained node, bottom-up TAM (units × price) vs top-down, and the economist's front-running indicators the feeds don't carry (capex commentary, book-to-bill, freight, the cycle, PMIs).
  - **Investor judgment** — the driving narrative and market sentiment (how much of the price is emotion about what might come vs present fundamentals), management quality and capital-allocation behavior (insider buying, buybacks, guidance delivered vs promised, candor in bad quarters), durability of growth, and the pre-consensus tells (thin coverage, low institutional ownership, a variant perception).
  - **Pattern / case study** — the candidate against the shipped **case library** (name, period, archetype, the tell's dated metric series, how it resolved), its matching cases supplied to this topic as **structured retrieval, never model recall** — the recurring early tells (a high-margin segment compounding inside a lower-margin whole, an oligopoly's supply discipline turning, a usage metric inflecting ahead of revenue, new management + a credible guidance step-change, forward customer commitments, replicable unit economics with whitespace) against the red-flag set (a multiple outrunning estimates, peak-cycle margins extrapolated, pull-forward mistaken for trend, a narrative with no metric, a deteriorating metric behind strong share, earnings-quality games). The library ships in three partitions — grounding (what this lens retrieves), development (the only set gate constants are shaped on), and a locked holdout never tuned against.
  - **External corroboration** the feeds can't give — customer / hyperscaler capex, supply discipline (capex cuts, curtailments), transcript backlog / TAM / inflection language, and DRAM / NAND ASP direction.
  - **The contemporaneous bear case — mandatory** — why the name might fail; the winning traits also rode the famous failures down.
  - Then the **disconfirming pass** — one bounded pass after the topics, searching for what would disprove the write-ups so far, from the same budget, outside the depth cap, fail-soft to a gap.

#### Call: research gathering — what it sees and returns

- **Sees (Part 1, the candidate-constant block first, then the topic)**
  - The candidate header with the archetype and the analysis date.
  - FETCHED VALUES — the candidate's fetched data as the providers return it, glossed once and never engine-computed: the profile line (name, exchange, sector, industry); the quarterly statements' headline lines for the latest eight quarters as reported; the annual revenue segments; the forward consensus for the next two fiscal years and the revision snapshot; the last four dividends; the quote with its 52-week range, and the closes on the prior pass's date and three, twelve, and thirty-six months back; the 8-K filings of the trailing twelve months by date and item; the latest short-interest print; the positioning rows; the next earnings date; and the `DGS10` and `DGS2` prints.
  - HYPOTHESIS — the discovery hypothesis document's section this name expresses, verbatim with its date; for a gate-rejected debut promoted from the watchlist, the node's own text, its prior thesis document; or the surfacing signal for a screen-surfaced name.
  - NEWS LEADS — dated headlines with addresses, fetch candidates only.
  - On a carried name, PRIOR ANALYSIS and PRIOR THESIS — the prior pass's analysis and thesis document verbatim, each with its date and, where a split intervened, its split-context line.
  - PAGES ALREADY RETRIEVED — pages fetched for this candidate earlier in this run, by other topics or earlier passes, in first-retrieval order; held in memory and discarded between candidates. A prior run's page reaches the model only when the model requests its address again and the document cache serves it. Absent on the disconfirming pass, which keeps its contrary search free of automatic page injection.
  - TOPIC; on a follow-up pass FOLLOW-UP and WRITE-UP SO FAR; on the disconfirming pass WRITE-UPS SO FAR.
  - Before each gathering request the app appends a short user message stating the replies remaining in this pass; the brief and every previously issued message stay unchanged.
- **Sees (Part 2)**
  - What to find, how to weigh a source (tier nearer 0 and extraction quality nearer 1 preferred; a weak source lowers confidence, never excludes), the per-reply tool-call bound, and when to stop; where pages are shown, to read them before searching for the rest.
- **Tool results**
  - Each page as the fetch returns it: a header (address, title, publication date, retrieval time, tier 0–5, the subjects the source is trusted on, extraction quality 0–1, a stub flag), then the page text as quoted material, never as instructions.
  - Failures are fixed sentences by typed class, never the operator's error text.
- **Returns**
  - Tool calls until a reply carries none.

#### Call: synthesis — what it sees and returns

- **Sees**
  - The candidate header and FETCHED VALUES.
  - EVIDENCE — every page the gathering conversation carried: reused pages first, then the pass's own in fetch order, deduplicated by final address. Headers and bodies are budgeted together; an omitted or truncated page is shown inline and recorded as a gap. Explicitly requested pages keep their text before a reused page does.
  - The topic text: TOPIC; FOLLOW-UP and WRITE-UP SO FAR; or WRITE-UPS SO FAR.
- **Returns**
  - The write-up, 400–900 words: what the research established on the topic's questions, each figure with the date or period its source gives for it and that source (the page's address, or FETCHED VALUES where the figure comes from there), where pages disagree, and what the evidence leaves unanswered.
  - On a follow-up pass, the topic's write-up rewritten whole.
  - A second message asks for a follow-up question: the question verbatim, or the one word `none`. Not sent on a topic's last pass or on the disconfirming pass.
  - Validated by nothing. A pass that retrieved no page spends no synthesis conversation.

- **Failure and output**
  - Research is fail-soft; a hard model failure fails the run, and the candidate resumes from its last checkpoint.
  - Output: one write-up per worked topic plus the disconfirming pass's, flowing whole to consolidation; any lower-priority topic the budget couldn't reach is a recorded gap.
  - Every page shown enters the candidate's page roster, persisted on the audit record.

---

### Step 5e — Consolidate the research

The write-ups become one analysis, distilled first only when they will not fit (*Consolidation*, above).

- **Budget check**
  - The orchestrator sizes the analysis prompt — the write-ups, FETCHED VALUES, and on a carried name the prior analysis — against the call's input budget. Within budget the write-ups go in as written; over it, the shared shapes apply. The prior analysis is never distilled.
  - This is the only place research is condensed for the candidate, and it condenses write-ups, never already-distilled notes.

#### Call: the analysis — what it sees and returns

- **Sees (in page order)**
  - The candidate header with the archetype and the analysis date.
  - FETCHED VALUES.
  - On a carried name, PRIOR ANALYSIS verbatim with its date and any split-context line.
  - WRITE-UPS — this run's write-ups or their distillate, each under its topic heading, the disconfirming pass's last.
- **Deliberately absent**
  - The engine's reads. The analysis adjudicates the research lenses among themselves; the research meets the engine at the thesis document.
- **Returns**
  - The analysis, 900–1,800 words: what this run's research established on the candidate — the leading metric's observed path, the thematic fit, the narrative and sentiment, management, the case-library parallels, the corroboration, and the bear case — each dated figure with its source, where the research lenses disagree with each other and where sources disagree, what stays unanswered, and on a carried name what the prior analysis said that this run confirms, revises, or leaves untouched.
  - A topic with no write-up this run keeps what the prior analysis says about it.
  - Validated by nothing. The only research artifact the next deep pass reads; the write-ups persist on the audit as written, and no typed research field exists.

---

### Step 5f — Self-review (carried names only)

Before this run's thesis document is written, the reasoner reviews its prior call against what has happened: the prior placement and conviction, the prior expected prices against the realized path, the falsifiers and triggers the prior document named, and the accuracy record. A debut has no prior record and skips the step.

#### Call: the review — what it sees and returns

- **Sees (in page order)**
  - The candidate header with the archetype and the analysis date, and FETCHED VALUES.
  - PRIOR CALL — the prior pass's date and the price then, its risk tier and horizon, its conviction, its three expected prices each with its horizon date, its status, and its detection mode.
  - PRIOR THESIS — the prior thesis document verbatim, with any split-context line.
  - ANALYSIS — this run's analysis (the prior analysis stays out, already folded into it).
  - REALIZED — the engine's realized block from Step 5c: the price now and its move since the prior pass, absolute and vs sector / market, with the maximum drawdown since the name became an opportunity; for each prior expected price whether its horizon has passed and, if so, the close on that date with its accuracy score, else the path so far; the lifecycle's accuracy scores at each horizon for the model and for the engine, each with the date of the last check that moved it and whether the prior analysis read it; the individual checks written since the prior analysis was written — this run's own checks included — newest by horizon date up to 12, under a heading stating that every line landed after the prior analysis and none was read before, with the total when the cap trimmed some, or one fixed sentence when none landed; the engine metric family's continuation state where it has one; and the engine's own composite, targets, tier, horizon, narrative-vs-reality and forensic reads then and now, with any hard-forensic or anchorless-`hype` read stated as a fact.
- **Returns**
  - The review, 400–900 words: each expected price against what happened; each falsifier and trigger the prior thesis document named, tripped or not by the numbers in front of it; whether the thesis survives and whether the placement still fits the payoff timing; where the prior read was right or wrong and why; what to revise; and what should change in how this name is analyzed.
  - Persisted on the audit record. Feeds the thesis-document call and the thesis document's summary paragraph. Nothing app-side reads it.

---

### Step 5g — Thesis document and typed appendix

Two messages in one conversation author the model arm. The reasoner interprets the engine's computed picture and this run's research into the candidate's two-arm opportunity record: the **thesis document**, with its risk tier and horizon, its conviction, and its expected price at three months, twelve months, and three years stated in words as part of the argument, then transcribed into the **typed appendix** — with the engine's numbers in the prompt as evidence, never as bounds. There is **no action call**: the job has no position to act on, the investor profile is absent from every prompt, and the archive decision on a carried name is the status the appendix carries.

- **Discipline the task states** (prompt-side and human-auditable, not an app clamp — the app derives no conviction and reads none of the document)
  - Score the **conjunction** of the lenses, never a single signal — base rates are brutal: most stocks underperform Treasuries over their life, and the winner traits recur in losers.
  - Require the **leading-metric anchor plus external validation**; apply the **narrative-vs-reality ratio**.
  - Treat **price action** (relative strength / base breakout) as a confirmation overlay that adjusts conviction, never a substitute for the anchor.
  - **Resolve or discount a research story the engine's lenses don't support** — a composite that likes a name whose unit economics don't, a thematic narrative over a failing value-creation read — rather than averaging it away.
  - Run the candidate down the archetype's **track** — proven economics (trailing returns on capital + a margin of safety) or emerging economics (a forward TAM × penetration × margin model clearing a return hurdle) — both through the same moat / management / price-asymmetry judgment.
  - On a carried name, weigh the **since-flagged performance** cap-only: a gain unmatched by leading-metric progress reads as building multiple-unwind risk and caps conviction; a drawdown with the metric intact reads as improved asymmetry rather than a reason to abandon; a gain matched by metric progress is held neutral, never boosted, since the confirming metric is already in the engine's composite and crediting the gain on top would double-count it.
  - Read the engine's numbers as evidence with their methodology exposed, author its own arm beside them rather than echoing them back, and say so in the document where it disputes a derivation.

#### Call: the thesis document — what it sees and returns

- **Sees (Part 1, in page order)**
  - The candidate header with the archetype and the analysis date, and FETCHED VALUES.
  - COMPUTED — the engine arm in full: its composite and sub-scores (each axis's polarity glossed once, the imputed-score disclosure where it applies, the thin-own-history flag as a data-health note); the value-creation read; its leading-metric series and the family's inflecting read where the family is structured or filing class; its bear / base / bull bands at three months, twelve months, and three years with the `TargetMeta` provenance flags rendered so a low-signal band is weighed, not obeyed; its risk tier and its horizon with the horizon's derived basis; the narrative-vs-reality read; the implied-expectations range beside the bands; the hard-forensic filings read as typed evidence with the rule it matched stated as a fact, and the soft forensic flags as annotations; the price-action confirmer; the positioning and options-activity reads with their stated conventions; the tradability flag and its haircut band; the floor's degraded-input flags as data statements; the Step-2 context loads where they apply — the sector-matched commodity prints, the CBOE backdrop, the COT row for the cyclical sleeve; and on a carried name the since-flagged read.
  - MARKET ANALYSIS — the house view, rendered as a market-level analysis and never named by product.
  - HYPOTHESIS — the discovery hypothesis document's section this name expresses, verbatim with its date; for a gate-rejected debut promoted from the watchlist, the node's own text, its prior thesis document; or the surfacing signal for a screen-surfaced name.
  - ANALYSIS — this run's analysis.
  - On a carried name, REVIEW — this run's review — then PRIOR THESIS verbatim, with any split-context line.
- **Deliberately absent**
  - The investor profile. Any accuracy section (the accuracy record rides the review alone). Raw statements, filings, or page text.
  - The engine's tier and horizon are shown with the rest of its reads: anchoring is the accepted cost, and the divergence is recorded on every pick.
- **Returns**
  - The thesis document, 900–1,800 words: the directional thesis, firm and specific, and whether this is an early-detection or a continuation idea; the leading operating metric and its trend; why now; the key drivers; the bear, base, and bull scenarios, each with the conditions that produce it and the model's probability; the bear case; the falsifiers and triggers in words, each stating a concrete measure, level, and period; what the current price already assumes; the risk tier and the horizon, argued from the payoff timing and the measurable risk; the expected share price at each horizon and the conviction, argued in the text; the entry consideration; on a carried name the status — still valid or invalidated — with what changed since the prior document and how the prior read held up, drawing on the review; and the summary paragraph.
  - Validated by nothing. Persisted on the opportunity record.

#### Call: the typed appendix — what it sees and returns

- The same conversation's second message, thinking off, under a grammar: a transcription, not a judgment.
- **Sees**
  - A request for the values as the document states them — null where it states none — closing on a placeholder-only return shape.
- **Returns**
  - `risk_tier` (`high` / `medium` / `low`), `horizon` (`short` / `mid` / `long`), `conviction` (`high` / `medium` / `low`), `expected_price_3m`, `expected_price_12m`, `expected_price_3y`, `detection_mode` (`early` / `continuation`), the leading metric's name and `metric_class` (`structured` from the engine's series menu, `filing` from the standardized statement-line menu, else `research`), and on a carried name `status` (`still-valid` / `invalidated`), every field nullable. A debut's grammar carries no status; the app stamps `new`, so an origin-incompatible value is structurally impossible.
  - The app keeps only the type check: a present enum in its domain, a present price finite and strictly positive, since the accuracy check divides by it, null accepted on every field. An off-domain object is rejected whole and the message reissued once; a second failure fails the run, the candidate resuming from its last checkpoint.
  - Nothing else about the model arm is checked: not the document against the appendix, not the prices against the bands, not the tier or horizon against the engine's.
  - It does not echo any engine value: the engine's targets, sub-scores, tier, horizon, and reads are app-stamped onto the record directly.
  - A null is acted on by presence alone: a null tier, horizon, or carried-name status reissues the thesis-document message once as a failed document (Step 5h); a null twelve-month price leaves the model arm no gate leg (Step 5h); a null conviction orders after low (Step 6); a null price opens no model leg at that horizon (Step 7); a null detection mode or metric shows as none.

- **Output of Step 5g**
  - The two-arm opportunity record, before the gate: the engine arm app-stamped, the model arm as written and transcribed.

---

### Step 5h — Gate, validate, checkpoint

An app-layer validator, not just a recorder. No model. Every rule below reads engine values; the model arm is checked on type and otherwise left exactly as authored.

- **Data retrieved**
  - No new data.

- **Placement and the baseline pair**
  - The **cell reads the model arm's tier × horizon** from the appendix, enum-checked at the transcription, never derived and never bounded.
  - The engine arm's risk tier and horizon, assigned at Step 5c, enter here as the gate's shared legs and persist beside the placement as the baseline. Where the pairs disagree the divergence is recorded — never a validation failure, never a reason to re-run the call — and rides the card's tag. The prior and current archetype labels are recorded the same way.

- **The entry-asymmetry gate — recomputed and enforced here, deterministically, once per arm**
  - For the **engine arm** over its structured twelve-month base target; for the **model arm** over its appendix's twelve-month expected price — a null leaving it no leg: it cannot admit, `admitted_by` can read only `engine-only`, and the audit records the model leg absent. Every other leg — the tier, the haircut band, **H** — is engine-derived and shared, so the arms differ only in the price each brings. The model's authored tier and horizon place the card, never the hurdle, so the model cannot lower its own admission bar.
  - **Base leg** — the post-haircut twelve-month base-case forward return must clear `DGS2 + 8 pts` (Low) / `+ 16 pts` (Medium) / `+ 30 pts` (High), decimal ratios, `DGS2` the run-level print.
  - **Shape leg** — bear-case downside may not exceed base-case upside; tests the engine arm's bands alone, the model arm stating no bear price.
  - **Liquidity leg** — the Step-5c banded haircut (0 / −3 / −6 pts) on both arms, the pre-haircut return recorded beside it so the haircut's own counterfactual stays answerable.
  - **Double-over-horizon leg** (emerging-economics track only) — required twelve-month base-case return ≥ `2^(12 ⁄ max(H, 12)) − 1`, a plain double where **H** is under twelve months and the on-pace annualized rate above it, **H** in months from the engine horizon's derived basis: `transaction-close` → months to the close (floor 3); `multi-year-compounding` → min(the value-creation read's runway years, 5) × 12, or 36 where the runway is not computable; `default` → ~6 (drafted). The strictest leg binds.
  - **Admission is either-arm**: a candidate clearing either arm's gate is admitted, stamped `admitted_by` (`engine-and-model` / `engine-only` / `model-only`), and **both arms' gate legs** — each leg's required value, actual value, and signed distance — persist on the run audit whatever the outcome.
  - A **debut no arm clears** is held out as a **`gate-reject`**: its thesis document and appendix persist on the audit, Step 7 opens its episode under that class and the debut's lifecycle id with both arms' prices and, where its appendix named a leading metric with its class, adds the name to the watchlist carrying that lifecycle id, where the next run's review decides its fate.
  - A **carried name no arm clears** takes the **upside-exhaustion attention warning** instead — never a 5h archive — so Step 6's every-gate-clearer-appears premise holds for debuts and carries alike.

- **The hard triggers** (app-enforced, binding absolutely on both arms; the either-arm grant never reaches them)
  - A **restatement or auditor change** — the item-classified filing kinds from the Step-5c sweep, never a bare model assertion — **excludes a debut outright on both arms** (held out as `excluded`, its episode opened under that class at Step 7, where any live watchlist node for the name retires) and, on a carried name, **app-forces the status to `invalidated`** — the archival path below — the model's conflicting status persisting as the typed **status-override divergence** `{ model-proposed status, app-forced status, matched hard trigger, filing }`, never the transition's control.
  - A model-only admission cannot carry a name past a hard trigger or the floor.

- **Annotations that clamp nothing here**
  - **Anchorless `hype`** — the narrative-vs-reality read at `hype` with the archetype's structured metric family measurable and not inflecting — is an annotation on a **carried** name, never a trigger: the review and the thesis document confront it and the model's status stands. A `still-valid` status held against it persists as the typed **narrative divergence** `{ engine read, model status }` on the record and the audit and raises the card's divergence tag (Step 10); a debut with that family read never reaches this step, held out at Step 5c as a story stock before research.
  - The soft forensic flags and a `hype` read, anchored or anchorless, are annotations the thesis document already weighed; they clamp nothing here.

- **Carried name outcomes out of this step**
  - Effective status **`invalidated`** — the appendix's status, or the app-forced override — → held out of the matrix and flagged for archival; Step 7 moves it. This deep pass is the only archival path.
  - **`still-valid`** → reconciles into the matrix at Step 7.
  - An inconclusive re-read never reached this step (Step 5c).

- **The model arm is checked on type alone**
  - The appendix's present enums, its present finite positive prices, its origin-constrained status, null accepted on every field. No engine-relative bound, band, ceiling, or clamp applies to its tier, horizon, conviction, or prices, and the thesis document is persisted exactly as written and validated by nothing.
  - A null tier, horizon, or — on a carried name — status is a failed document, never a turn-away: the thesis-document message reissues once, the task naming the missing item in its own words; a second null fails the candidate under the shared second-failure rule, the checkpoint carrying a resume.
  - The reissue is the orchestrator's — a new thesis-document message followed by its own appendix message — never a retry of either call: each issued call keeps the adapter's own single re-attempt for its transient classes and nests nothing, so a deep pass issues at most two thesis-document messages and two appendix messages before the terminal failure.
  - There are **no numeric echoes to validate**: engine-owned values are app-stamped directly onto the record, and the model returns only its own arm.

- **Persist and checkpoint**
  - The surviving candidate's record is persisted — the engine arm app-stamped, the model arm as written and transcribed, the admission provenance, the divergences — with its audit record: the write-ups as written, the distillation shape and call count, the analysis, the review, the page roster, the research gaps, the engine's computed reads, the prior and current archetype, the model ids, and the prompt stamp.
  - The candidate is **checkpointed** so the run can resume here. A gate-reject or an excluded debut checkpoints with its typed hold-out record.

- **Output**
  - A survivor with its model-placed cell, both arms' tier and horizon with any recorded divergence, and the admission provenance with both gate legs; or a typed held-out record (`gate-reject` / `excluded`); or a carried name flagged for archival.
  - The candidate checkpoint.

---

## Step 6 — Order the survivors

Every survivor's **cell is already fixed** — the model arm's tier × horizon from its appendix, enum-checked at Step 5g — so this step chooses neither *which* opportunities appear nor where; it orders them. Computed-only; no model call.

- **Data retrieved**
  - No external data.

- **Logic**
  - Within a cell the order is deterministic: by **conviction level** (high before medium before low, a null after low), then by the **model arm's twelve-month upside** — its appendix's twelve-month expected price against the live quote — descending, a null price after every present one, then by ticker for a stable order; a carried name inserted at Step 7 takes the same order.
  - **Every Step-5h survivor appears in its cell** — a name either arm admitted is listed, and nothing collapses two names into one: names that express the same discovery hypothesis are **linked** through their hypothesis node and shown as such on their cards, never merged, so breadth and auditability survive and completeness holds by construction rather than by validation.
  - This assembly covers this run's survivor set only; the matrix is final only after Step 7 inserts the still-valid carried opportunities into the same order.
  - No per-cell cap: the gates set a cell's count, not a quota. A cell may be empty when nothing qualified — honest, not a failure — and the matrix never pads itself; a rich cell is never trimmed.

- **Model**
  - None.

- **Output**
  - The ordered survivor matrix (this run's set).

---

## Step 7 — Refresh existing ideas, open episodes, finalize the matrix

The continuity step — an app-layer validator with no model: the rotation picks' and re-surfacers' deep passes already ran in Step 5, so this step reconciles their verdicts, **cheap-sweeps every other live opportunity**, finalizes the matrix over the union, opens this run's episodes, and reconciles the graph and archive. The dividing rule throughout: **only a deep pass can archive**.

- **Data retrieved**
  - The prior matrix (Step 2) and the run-scoped deep-research set (Step 4).
  - For each cheap-swept name: FMP `quote` and `analyst-estimates` (the swept-union population with Step 3c; one sweep per distinct symbol); dated-EOD bars through the shared price-bar cache; on the **filing-cadence rider** (a new reported period on the swept `earnings` row) the statement-derived rows — statements, `key-metrics` / `ratios`, `financial-scores`, `financial-growth`; the once-per-run FINRA file where the metric family is short-interest-fed; current `DGS2` / `DGS10` (Step 2).
  - For the archive: dated-EOD bars per distinct archived symbol (deduped against the swept population).

### Reconcile the deep-researched carries

- A carried name that won a deep pass this run (rotation pick or re-surfacer) has its fresh Step-5h verdict: **still-valid** → reconciles into the matrix; **invalidated** (model-judged, or app-forced on a hard trigger — the only path that archives in DTO: a qualitative erosion the cheap read can't see, or an exhausted-upside / continuation-failure finding *confirmed* under fresh research) → moved to the **archive** here; **inconclusive** (the re-read fell below the floor at Step 5c) → reconciles on its prior verdict with its typed refresh gap, treated exactly like a cheap-swept carry, its freshness never advanced so it stays rotation-eligible.
- Every such ticker is in the deep-research set, so the cheap sweep skips it.

### The cheap re-derivation (every other live opportunity; engine-only, never archives)

- **Re-derive the engine arm's bands** — the multiples re-anchored closed-form on the fresh `DGS10` against the stored anchor-window percentiles and drivers (Step 5c), from structured data alone.
- **Re-run the full entry gate on both arms** against the live quote — every recomputable leg live: the refreshed engine tier, fresh `DGS2`, the banded liquidity haircut from live tradability inputs, the emerging leg's **H** re-read from the persisted horizon basis (a transaction close's months-to-date remeasured). The model arm needs **no model call**: its twelve-month expected price is a frozen number from the last deep pass, so the engine re-measures the live price against it exactly as it does its own, a null frozen price reading upside exhaustion on the engine arm alone — and it **does not decay**, since a dated judgment is not a stale prop (its age rides the *Research stale* badge).
- **Re-derive the engine risk tier** from the refreshed inputs — a gate leg and the card's baseline read, never the card's cell, which is the model's frozen placement; a newly disagreeing pair surfaces through the divergence tag.
- **Refresh the engine-computed fields** — the metric family's continuation state where the family is structured or filing class (a `research`-class anchor has no engine feed and holds its last read), the narrative-vs-reality ratio, the forensic computations on the rider, and the since-flagged read (below).
- **Raise the attention warning — *Consider Deep Audit* — on any of three high-bar triggers**, and otherwise act on nothing:
  - **Upside exhausted** — **neither arm's** re-derived base case still clears the full entry gate (the engine's live targets; the model's frozen twelve-month expected price re-measured); while either arm clears, the divergence is recorded and no warning fires — mirroring either-arm admission.
  - **A tripwire** — a forensic flag newly tripping, a **continuation-failure signal** (estimate revisions rolling over, a beat-and-raise streak breaking, shipments diverging below sell-through), a since-flagged gain diverging hard from the engine family's continuation, the narrative-vs-reality read crossing into `hype`, the margin of safety compressing near the exhaustion line, or a drawdown breach.
  - **A re-surfacing** — the name re-appeared in discovery without winning a leftover-budget deep pass.
  - Both readings are fail-soft: a *missing* input holds the opportunity on its last verdict, never an escalation; only an affirmative signal **confirmed under a deep pass** ends an opportunity. Decay alone raises nothing — research staleness is the quiet *Research stale* badge, so the amber warning stays reserved for an actual problem.
  - The next **floor-clearing** deep pass (a rotation-slice pick, a later re-surface, or an ATO Deep Audit) clears it; an inconclusive below-floor re-read leaves it standing.
- **The model-authored fields freeze between deep passes** — the thesis document, the placement, the conviction, the expected prices, the detection mode, the leading metric, and the status hold their last deep-pass values. The narrative divergence freezes with them — a dated snapshot of the deep pass that wrote it; the refreshed narrative read neither records nor clears it and raises only the tripwire, and the tag carries it until the next deep pass re-decides.

### Final matrix assembly over the union

- Each still-valid carried opportunity **holds its model-placed cell** — placement is a frozen model-authored field, and only a deep pass re-places. The refreshed engine tier persists beside the placement as the baseline; a newly disagreeing pair surfaces through the divergence tag, never a moved card.
- Insert each carry deterministically into the cell's Step-6 order — conviction level, then the model's twelve-month upside against the live quote, then ticker — computed-only; no model re-orders anything.
- Every Step-5h survivor is in its cell and every still-valid carry is in its held cell by construction — there is no model step that could have dropped one.

### Open episodes

Engine-computed over the append-only episode store. No model. The checks on existing episodes ran at Step 2, before discovery, so this run's reviews read them.

- **Open episodes**
  - For every candidate whose appendix carried prices this run and that has none, or whose latest episode is a month or more old — the cadence counted within one decision class, so an admission opens its `picked` episode whatever the age of a turned-away episode on the lifecycle: a pick under `picked` (on its debut and on each later deep pass past that cadence); a debut no arm admitted under `gate-reject`; a debut a hard trigger excluded under `excluded`.
  - An episode records: the symbol and lifecycle, the decision class, the creation date, that day's spot, the anchor close with its bar date, the model's expected price at three, twelve, and thirty-six months — a horizon the appendix left null recorded null — and the engine's base value at the same horizons — nothing else; the engine's bear and bull edges stay on the run record.
  - An `insufficient-evidence` or story-stock hold-out has no prices and opens none, and so does an appendix with every price null; a carried name's inconclusive re-read opens none; a watchlist node opens none.
  - Which episode is a lifecycle's latest is insertion order; the creation date is data, never identity.
  - An episode is never updated or deleted; the checks are written onto it once each. The store persists independently of run retention, the archive, and matrix presence — a departed lifecycle's episodes keep maturing on the same clock, and a re-entry opens episodes under its new lifecycle while the old ones run to their horizons.

- **Three disciplines**
  - The engine keeps score and never plays; nothing the model writes alters a check or a score.
  - The record scores the forecast, not the decision: a pick, a gate reject, and an exclusion score alike, and the decision class is a tag.
  - Nothing is derived beyond the scores: no cohorts, no spreads, no calibration proposals, no per-decision verdict. The learning rides the review's own answer to what should change, and the engine's drafted constants move only on the user's reading of the record.

### Since-flagged read, graph, and archive (same pass)

- **Since-flagged read** — refreshed for every carried-forward opportunity: its price-derived parts (running return since `became_opportunity_at`, absolute and vs sector / market, and maximum drawdown) from the same daily-bar reconstruction, the engine metric family's continuation state from its structured or filing path — **live from the idea's first subsequent run** (a debut pick has no elapsed window yet), so the matrix display and the self-review of a carried name read the same numbers.
- **Opportunity graph** — this run's picks link to their matrix entry (`picked`); this run's hypotheses decided `watchlist` (and `promote` without a slot) are added or refreshed as **watchlist** nodes with their document sections and appendix fields; every ordinary `gate-reject` debut — researched, no arm's gate admitting it, no hard trigger and no floor hold-out, its appendix naming a leading metric with its class — is added or refreshed as a watchlist node too, carrying its thesis document as its text and its appendix's leading metric and class, the next run's review deciding whether it stays (its forecast scored on its `gate-reject` episode under the lifecycle its admission would continue); a watchlist node whose name a hard trigger excluded this run is retired in this pass with that reason; company nodes the watchlist review retired and company or hypothesis nodes whose carry horizon elapsed are **retired** (Step 3c); a deeply invalidated pick's node moves to **`departed`** in the same pass as its archival — a terminal tombstone, visible in route context as a dead thesis, never a discovery feeder and never re-promotable in place; a genuine re-entry opens a new node under a new lifecycle. Departed tombstones prune on the archive's retention.
- **Archive** — an `invalidated` opportunity is moved to the archive (the most recent **100**, oldest evicted first) as a **frozen verdict snapshot**: the thesis document with its bear case, the archetype, the leading metric, `became_opportunity_at`, the departure date, the archive trigger (`failed-reevaluation` — the single trigger, always a deep pass) with the specific failing signal, `admitted_by` alone (both arms' gate legs live on the run audit), conviction at exit (the model arm's value — a record of where it ended, not a live call), the stamped sector identity, any status-override divergence, and any standing narrative divergence. Afterward **only the price is tracked** — each run refreshes its since-flagged return (absolute, vs sector / market) and drawdown from the bar cache; no metric continuation, no research, no model call. Its episodes keep maturing in the episode store. There is no "target met" exit; staleness alone never archives.
- **Re-entry is a fresh start**: a later run that independently re-discovers an archived ticker removes it from the archive and it enters as a new opportunity with a new `became_opportunity_at` and a new lifecycle; none of the archived record influences the new one, and the re-entry carries no since-flagged read at its first interpretation. Re-entry is matched by ticker, so a ticker is in exactly one state — live, departed, or neither; a same-ticker-new-thesis re-entry simply retires the prior tombstone early (the archive holds at most one slot per ticker). The archive is passive: a large archived gain never pulls a name back into analysis.

- **Model**
  - None.

- **Output**
  - The final matrix over the union, held-cell carries and this run's survivors.
  - Attention warnings raised, archive moves, the updated opportunity graph and coverage state.
  - This run's opened episodes and the refreshed since-flagged reads.

---

## Step 8 — Mark opportunities you already own

- **Data retrieved**
  - The holdings list, **pulled fresh from Schwab at this step** — the run's sole holdings consumer, so the tags are current rather than hours stale after a long run.
  - On a failed pull: the most recent persisted holdings snapshot, the tags labeled with its captured-at date; with no snapshot, the owned tags are omitted as a typed gap. Never a failed run.

- **Logic**
  - Flag each matrix opportunity owned / not-owned.
  - Runs *after* discovery, selection, and continuity, and reads only the holdings list — never Portfolio Analysis's stores — so holdings never influence what is found or chosen; the job stays independent of the account.

- **Model**
  - None.

- **Output**
  - Display-only ownership tags.

---

## Step 9 — Save everything

- **Data stored — the five persisted structures**
  - **The run record** — the 3×3 matrix (every opportunity's record: the lifecycle id, the archetype with its prior label on a carry, the detection mode, the thesis document, the leading metric with its class, both arms' risk tier and horizon — the model's placing the card, the engine's pair with its derived basis beside it — the conviction, the model's three expected prices and the engine's three bands with their `TargetMeta`, the narrative-vs-reality and implied-expectations reads, the composite and sub-scores with their provenance flags, the hypothesis lineage, `admitted_by`, the status, the attention-warning state and trigger, `became_opportunity_at`, `last_deep_researched_at`, the stamped sector identity, the since-flagged read, and the accuracy scores for both arms) plus the **run audit record** — sources and retrieval timestamps; the discovery and screening inputs — the route plan with its coverage-rotation selections and completions, each route's hypothesis document and appendix with the route's write-ups and page roster, the watchlist review and its decisions, and the refresh write-up where the lane ran; and, per candidate, the write-ups as written, the distillation shape and call count, the analysis, the review, the page roster, the research gaps, the engine's computed reads with their provenance flags (the job-time `quote`, the anchor-window percentiles and drivers the cheap paths re-anchor against), the thesis document and the appendix values, the admission provenance with both arms' gate legs, the recorded divergences (the prior and current archetype, the tier / horizon pair, the narrative divergence) and any status-override divergence, the floor's hold-out reasons and freshness states, the cheap sweep's warnings, each pass's reused-vs-fresh document split, model ids and quantizations, and the prompt stamp.
  - **The opportunity graph** — hypothesis nodes with their document sections and appendix fields; company nodes with their status, leading metric and class, refresh timestamps, and hypothesis link.
  - **The discovery-coverage ledger** — per route class and coverage subject: first seen, last attempted, last successfully completed, computed debt.
  - **The archive** — the most recent 100 departed picks as frozen snapshots (Step 7); since-flagged numbers recomputed at render, never stored.
  - **The episode store** — append-only, outside run retention: this run's opened episodes, and the checks and unscorable states written at Step 2.
  - Shared stores touched: the price-bar cache, the document cache, the factor-distribution store (one current observation per issuer per factor, diagnostic-only), the web-research source state.

- **Rules**
  - A hard persistence failure fails the run.
  - Retention keeps the last N Trade Opportunities runs; the archive at 100; `departed` tombstones on the archive's retention; the watchlist under its cap; the episode store outside every window. The checkpoint trail is cleared by the successful persist.
  - Nothing is written to vector memory. A carried name's continuity is the deterministic load of its own documents.

- **Output**
  - A durable run and audit record, the carried stores for the next run's Step 2, and the updated episode store.

---

## Step 10 — Display the result

Display is a pure read of the persisted matrix; no model runs.

- **Data retrieved**
  - The persisted run (matrix, archive, badges' inputs).
  - The per-ticker daily-bar cache — the since-flagged read's **price-derived parts** and the % upside to target are **re-derived at render** from the latest cached close (the cache refreshes a symbol lazily, after 8 PM ET and not within the prior 24 hours, fail-soft), so the card is current between runs and opening the page costs no fetch once the day's bar is cached; the metric family's continuation state and the live-quote gate read refresh only when a job runs; the live `quote` is a job-time input only, never a render dependency.

- **The matrix (default, canonical view)**
  - Three risk sections × three horizons — every card placed by the **model arm's** tier × horizon; each card: archetype, the thesis document's summary, leading metric, detection mode, conviction, the expected price at twelve months beside the engine's base target, the narrative-vs-reality read, status, `became_opportunity_at`, `last_deep_researched_at`, the hypothesis it expresses (linking the names that share one), owned / not-owned, and — for a carried idea — the since-flagged performance (return since it became an opportunity, vs sector / market, a compact running curve, maximum drawdown).
  - **Two arms by progressive disclosure**: the model arm headlines — its conviction and its twelve-month expected price are the numbers shown — with a quiet **divergence tag** where the arms materially disagree (the twelve-month prices' gap, a tier / horizon pair, or the narrative divergence); on card expand the paired engine / model view — the three price pairs, the engine's bands and provenance flags, the engine's tier and horizon, and the accuracy scores for both arms; the `admitted_by` tag on both single-arm states — `engine-only` (the headline model price did *not* itself clear the gate; the engine admitted it) and `model-only` (the headline admitted it; the baseline dissented); consensus cards untagged.
  - Lifecycle affordances per card: the selection control (plus select-all / deselect-all), an amber actionable **Consider Deep Audit** badge when the attention warning is set, a green **Deep-researched today** badge when `last_deep_researched_at` is the current local-timezone day, a quiet **Research stale** badge when the last deep pass is older than ~4 weeks (computed at render; never amber).
  - Empty cells shown as empty.

- **List view (toggle)**
  - All nine cells flattened into one sortable grid, each row keeping its placed (model-arm) risk tier and horizon, selection control, badges, and engine-target / model-target / divergence columns; sort keys: **forward % upside to target** (default, descending — the model arm's twelve-month expected price against the cached close, a null price sorting last; the engine's sortable in its own column) or **realized since-flagged return** (a debut sorts last). Display-only reordering; keyboard-operable sortable headers and a stable sort.

- **Archived opportunities (separate view)**
  - Each departed pick's frozen record, departure date, and live since-flagged return — **no forward prediction**, no expected prices, no accuracy scores; sortable by since-flagged return or drawdown, default departure date descending.

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
  - **Deep Audit** clears the full Step-1 gate (daemon reachable + the reasoner pulled — it makes model calls) and triggers the SearXNG pre-run notice when the instance is down (its selected names get thinner evidence; always *not recommended*, since the local suite is SearXNG-only).
  - **Quick Audit** is engine-only, so it **skips the daemon-connectivity check** and runs with the daemon configured-but-down; no web research, so no pre-run notice.

- **Load**
  - Deep Audit: the Step-2 load as in DTO (house view, run-level FRED / FMP-commodity / CFTC / CBOE, the prior matrix and opportunity graph).
  - Quick Audit: only the subset its engine pass needs — the FRED rate anchors (`DGS2` for the entry threshold, `DGS10` for the re-anchor) under the **quick-path cached-print rule** (a failed FRED retrieval fail-softs to the last cached print with its as-of date, eligible only within the shared rate-cache max age; older, or no cache, types the rate-dependent reads `unknown` rather than computing off a stale anchor), and **conditionally** the once-per-run FINRA consolidated file when a selected name's engine metric family reads short interest (a failed file fetch types that read `unknown`).
  - **The accuracy checks run in both modes** over every due episode in the store — not only the selected names — since they are engine-only (Step 2).
  - The selection is the work list; no discovery feeders run.

## Quick Audit

`Selected names → Refresh numbers → Re-run both arms' gates → Check warnings → Save`

- **Data retrieved (per selected name)**
  - FMP `quote` and `analyst-estimates`; dated-EOD bars through the shared price-bar cache; on the filing-cadence rider, the statement-derived rows; the conditional FINRA lookup. No Schwab data in the analytical pass (the options signal is held out of the grade and is not an input) — the run's one Schwab touch is the closing Step-8 holdings cross-reference, fail-soft and display-only.

- **Logic — the same cheap re-derivation Step 7 applies to the DTO matrix tail**
  - Re-derive the engine arm's bands — the multiples re-anchored closed-form on the fresh `DGS10` against the stored anchor-window percentiles and drivers, from structured data alone.
  - Re-run the **full entry gate on both arms** — the engine's live targets and the model's frozen twelve-month expected price against the current price; re-derive the engine risk tier (a gate leg and the card's baseline read — never the card's cell, which is the model's frozen placement).
  - Refresh the metric family's continuation state where a structured or filing path exists, and the since-flagged read.
  - Raise or retain the **attention warning** on an upside-exhaustion (neither arm clears) or tripwire reading — never an archive.

- **Model**
  - None — it cannot fail on research, and it runs while the model server is offline.

- **Cannot**
  - Rewrite the thesis document, the conviction, the expected prices, or any model-authored field (they stay frozen).
  - Move a card between cells — placement is a model-authored field.
  - Perform new research; stamp `last_deep_researched_at`; clear a warning.
  - Archive an opportunity. Open an episode — its model fields are unchanged.
  - It never checkpoints — engine-only and fast, it simply re-runs.

- **Persist and render**
  - Step 9's persistence over the touched records; the Step-8 holdings cross-reference re-runs over the touched names; the page re-renders. The opportunity graph is untouched.

## Deep Audit

`Selected names → Full Step-5 loop → Reconcile → Save → Display`

- **Data retrieved**
  - Everything a Step-5 candidate gets — the full per-symbol surface and fresh web research (the shared loop, SearXNG only).

- **Logic**
  - Each selected name runs the Step-5 per-candidate loop as a **carried-forward candidate**: 5a affirm-or-overturn on the prior archetype → 5b dossier with the prior record loaded by lifecycle id → 5c engine and floor → 5d research → 5e consolidation → 5f self-review → 5g interpretation → 5h gate and validate.
  - A **large selection prompts a confirmation first** — the loop runs per name and can be long.
  - Resume holds under the same per-candidate checkpoint contract over its smaller pinned set (the selected names and the run's shared context).

- **Model**
  - The full archetype, research, consolidation, review, and interpretation calls, per selected name.

- **Can — contingent on the pass completing past the engine floor**
  - Rewrite the model-authored fields — the thesis document and appendix, placement included: a freshly written thesis document and appendix re-place the card.
  - Stamp `last_deep_researched_at` (the green *Deep-researched today* badge) and **clear the attention warning**.
  - Judge the name `invalidated` → the **archive** — model-judged, or app-forced on a validated hard trigger with the status-override divergence; the archival write atomically takes the touched picked node to `departed` under the same lifecycle id. It is the **only** ATO path that can archive.
  - Open an episode for a touched name under Step 7's cadence — a pick whose latest episode is a month or more old, or which has none — with the prices its new appendix carries.
  - A selected name whose re-read falls below the floor at Step 5c holds its last verdict under the carried-name rule, before any research is spent — no stamp, no warning clear, no episode.

- **Does not**
  - Run discovery, add watchlist nodes, or run the review's retirements — the touched picked nodes' lifecycle transition is the graph's only mutation.
  - Modify unrelated opportunities.

- **Continuity, persist, and render (Steps 7–10, reduced)**
  - The audited records reconcile into the matrix in the deterministic order; an `invalidated` result moves to the archive; the Step-8 holdings cross-reference re-runs over the touched names; the run + audit record persist (Step 9); the page re-renders (Step 10).

---

# The most important safety rules

- The engine computes every fact and its arithmetic — prices, statements, positioning, short interest, the options signal, Altman Z, Piotroski, the composite's normalization — once; a second version of a fact is fabrication, not judgment.
- Nothing the model writes alters or binds an engine value: engine-owned values are app-stamped directly and never echoed through the model.
- The model's arm is checked on type alone — the enums, a price finite and positive — never against the engine, never on content; the thesis document is persisted exactly as written and validated by nothing.
- Admission is either-arm, scoped to the entry gate alone and stamped `admitted_by`; the evidence floor and the hard triggers bind both arms absolutely.
- Placement is the model's: the card sits at the model arm's tier × horizon, frozen between deep passes, with the engine's rule-derived pair beside it as the disclosed baseline. The admission yardstick is not: the gate's required return, haircut, and H read the engine's legs on both arms, so the model never sets its own bar.
- Both arms are scored identically by the accuracy checks, and the engine keeps score without playing; the record scores the forecast, never the decision, and nothing is derived beyond the scores.
- A candidate with no inflecting, dated, third-party leading metric is a story stock and never enters the matrix; missing floor-bearing evidence causes abstention, not a guessed verdict. Both hold-outs happen before any research is spent.
- Fast checks may warn; only a deep re-evaluation may rewrite a model-authored field or remove an opportunity. Missing data never causes removal; staleness alone never archives; there is no "target met" exit.
- Only a debut can be held out at the floor, excluded by a hard trigger, or turned away at the entry gate; a carried name failing the gate takes a warning, an inconclusive re-read holds its last verdict, and it leaves only when a deep pass judges it invalidated — model-judged, or app-forced by a hard trigger, the one app-forced removal.
- Price never raises conviction: the since-flagged read is cap-only, the price-action confirmer adjusts but never substitutes for the anchor, and the archive never promotes itself — re-entry is a fresh start.
- The investor profile reaches no prompt in the job.
- Holdings never influence what is found or chosen; the owned tag is display-only, and the job never places an order.
- The job writes nothing to vector memory; a name's memory is its own documents.
