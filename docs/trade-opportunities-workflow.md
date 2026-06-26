# Trade Opportunities Workflow

Trade Opportunities is one of the two local-suite jobs ([local-models.md](local-models.md)). This document specifies its end-to-end control flow; the feature's design rationale — the archetype lens, the 3×3 matrix, the two detection modes, the opportunity schema, the continuity vocabulary — lives in [trade-opportunities.md](trade-opportunities.md).

The Trade Opportunities job:
- loads the house view, investor profile, and run-level macro / positioning context
- **discovers** candidates through two converging feeders — bottom-up structured screens and a top-down, research-active theme scan — rather than reading a fixed list
- classifies each candidate by archetype, then computes a deterministic, archetype-weighted financial picture for it
- researches each candidate on the open web — building the leading-metric case **and a mandatory bear case**
- scores, gates, and assigns each survivor a deterministic risk tier and horizon → a 3×3 matrix cell
- carries prior opportunities forward with an updated status, and flags any overlap with current holdings

It runs **on demand only**, entirely on local models, with **no cost at the model layer**. A **single global run slot** serializes it against the report and Portfolio Analysis (only one runs at a time). For job states, the global run slot, cancellation, and error handling, see [scheduling.md](scheduling.md) and [run-tracking.md](run-tracking.md); for the failure posture (per-candidate checkpoint/resume, fail-soft research and discovery), see [trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture).

Unlike Portfolio Analysis, this job is **not tied to current holdings** — it discovers new ideas. It still **requires a connected Schwab account**, whose option chains supply the per-candidate options-activity signal ([schwab-integration.md](schwab-integration.md)); holdings are read only at the end, for a deterministic owned/not-owned cross-reference that never influences what is discovered or chosen.

## How to read this workflow

Every step below is tagged with a **Type** so it is obvious what the step actually does — the same three Types the Portfolio Analysis and report flows use:

- **Computed (app layer)** — deterministic Rust logic, with no model and no external network: local SQLite and filesystem reads, the candidate dedup / tradability filter, the deterministic **risk-tier and horizon assignment**, and the **financial-analysis engine** (every sub-score, target, and derived read).
- **API retrieval** — fetches from external sources: the **FMP discovery layer** (screener / bulk / peers / industry classification — the universe scan) and the **FMP per-symbol surface** (fundamentals, the revision signal, positioning, segments, transcripts); company cross-checks from **SEC EDGAR / Stooq / FINRA**; run-level macro and positioning from **FRED / CFTC / CBOE**; the keyless news feeds (**GDELT / FMP Articles**) the discovery scan reads; option chains from **Charles Schwab**; and the **web tool** (SearXNG-primary, Tavily as the degraded fallback) the orchestrator runs *on a model's behalf*. The full per-source endpoint surface, with each call's **discovery / per-candidate / run-level** cardinality, is in [data-sources.md §Trade Opportunities — endpoint surface](data-sources.md#trade-opportunities--endpoint-surface).
- **Local-model call** — invokes a model on the app-supervised **Ollama** daemon ([local-models.md §Serving runtime](local-models.md#serving-runtime)): the primary reasoner **`Qwen3.5-122B-A10B`** in **thinking** mode (theme research, candidate research, archetype confirmation, scoring) or **non-thinking** mode (firm, directed consolidation), or the fixed **`Qwen3-Embedding-4B`** embedder (vectorization only). Every generative call is **schema-constrained** via Ollama's native `format` parameter — the model picks values, never structure.

Two load-bearing architectural rules frame the whole table, the same ones the report and Portfolio pipelines hold: **agents are pure stages, and the application layer owns all I/O** — a model stage consumes the structured input handed to it and emits a schema-validated result; when a research stage needs the web it *requests* a tool call and the orchestrator performs the fetch. And **the engine computes every number** ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)) — the model interprets computed values and never invents one; the **risk tier and horizon are likewise rule-derived, not model-picked**, so the same asset lands in the same cell run to run ([trade-opportunities.md §The opportunity space](trade-opportunities.md#the-opportunity-space)). For each model stage, the **Local-model call** block lists what the prompt includes and what the model returns. Per-step progress, per-request rows, and token/reasoning output stream to the run tracker over the shared `progress` seam ([run-tracking.md](run-tracking.md)), with **per-cell progress** as the matrix fills.

The job is a **funnel**: broad, cheap discovery narrows to a small candidate set, then each survivor gets expensive per-name validation — the condense-as-you-go discipline the rest of the suite uses, and the shape the data budget requires (the richest per-symbol signals are rate-relevant, so they are spent only on the narrowed set). The cardinality bands in the endpoint table track this directly: **discovery** calls scan the universe a handful of times, **per-candidate** calls are the budget driver, and **run-level** calls fire once.

| Step | Stage | Type | Model |
|---|---|---|---|
| 1 | Job start & gate | Computed | — |
| 2 | Load shared context (house view, profile, run-level FRED/CFTC/CBOE, prior run) | Computed (local read) + API retrieval | — |
| 3 | **Candidate discovery** (two feeders) | mixed — see 3a–3b | 122B · thinking |
| 3a | Bottom-up structured screens | API retrieval (FMP bulk/screener, FRED/Stooq, FINRA) + Computed (engine) | — |
| 3b | Top-down theme & event scan | Local-model call (thinking) + API retrieval (news + web tool), looped | 122B · thinking |
| 4 | Candidate consolidation (dedup, tradability filter, signal tagging) | Computed | — |
| 5 | **Per-candidate validation loop** (per candidate, checkpointed) | mixed — see 5a–5h | 122B + embedder |
| 5a | Archetype classification | Computed (features) + Local-model call (confirm) | Qwen3.5-122B · thinking |
| 5b | Dossier assembly | API retrieval + Local-model (embedding) + Computed | Qwen3-Embedding-4B · fixed |
| 5c | Deterministic analysis (archetype-weighted engine) | Computed (engine) | — |
| 5d | Bounded web research (+ mandatory bear case) | Local-model call (thinking) + API retrieval (web tool), looped | Qwen3.5-122B · thinking |
| 5e | Distillation | Local-model call (non-thinking) | Qwen3.5-122B · non-thinking (35B optional) |
| 5f | Deterministic target refinement | Computed (engine) | — |
| 5g | Scoring & gating (the opportunity record) | Local-model call (thinking) | Qwen3.5-122B · thinking |
| 5h | Deterministic risk-tier, gate validation & checkpoint | Computed | — |
| 6 | Selection & matrix assembly (per cell) | Local-model call | Qwen3.5-122B |
| 7 | Continuity check & carry-forward | Computed | — |
| 8 | Holdings cross-reference | Computed | — |
| 9 | Persist run & audit + memory embeddings | Computed (persist) + Local-model (embedding) | Qwen3-Embedding-4B · fixed |
| 10 | Render Trade Opportunities page & update UI | Computed (frontend) | — |

## Step 1: Job Start and Gate

**Type:** Computed (app layer) — the local-suite execution gate. No model and no external API (credential and daemon *presence/reachability* are checked, not analysis).

The job will not start unless three preconditions hold, the same gate Portfolio Analysis clears ([portfolio-workflow.md §Step 1](portfolio-workflow.md#step-1-job-start-and-gate)):
- the **single global run slot** is free (no report or other local job is running — see [scheduling.md §Concurrent Job Protection](scheduling.md#concurrent-job-protection));
- the **local-model daemon is reachable and the configured roster is present** (the 122B reasoner + the embedder) — health-checked at the Ollama endpoint ([local-models.md §Serving runtime](local-models.md#serving-runtime));
- a **connected Schwab account** with a valid (≤7-day) refresh token ([schwab-integration.md §A connected Schwab account is required](schwab-integration.md#a-connected-schwab-account-is-required)) — Trade Opportunities needs it only for the per-candidate options-activity signal and the closing holdings cross-reference, but the connection is a hard precondition all the same.

This gate is **independent of the cloud-report gate** — a machine with no OpenAI/Anthropic keys can still run the local suite. A failed precondition blocks the job and surfaces in the Persistent Warning Area under its own categories (local models unavailable, Schwab connection), without creating duplicate warnings.

## Step 2: Load Shared Context

**Type:** Computed (local read — house view, investor profile, prior run) + API retrieval (run-level FRED / CFTC / CBOE). No model.

Loaded **once per run and shared across every candidate**, not re-requested per name:
- the **Market Signal house view** — the latest report's Thesis, Investment Strategy, and Forward Outlook sections plus recent report summaries (`thesis_stance`, `forward_outlook_themes`, `key_risks`), loaded **deterministically** from the report store (retrieve-don't-dump — never by vector-searching the report's memory; see [local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)). The report's **creation date** rides into the context, and the same **one-week freshness window** Portfolio applies holds here: a house view older than one week is **omitted and recorded as a gap** rather than fed as current ([portfolio-workflow.md §Step 5](portfolio-workflow.md#step-5-load-shared-context)). The house view supplies the **macro / regime backbone of the job's worldview** — its `market_cycle` and `risk_posture` are a compact growth × inflation regime read ([trade-opportunities.md §The research method](trade-opportunities.md#the-research-method)) — and steers *where the job hunts* (it biases the top-down theme scan and the archetype-appropriate read) without being a number the engine consumes; the **forward thematic map** that completes the worldview is the job's own, built in the Step-3b theme scan;
- the **investor profile** — for now a **fixed default preset** (long-term horizon, profit-maximization objective, medium-to-high risk tolerance, **cash treated as always available**, no tax adjustment; user configuration is deferred — see [configuration.md §Investor Profile](configuration.md#investor-profile)) — which shapes the **entry framing and conviction emphasis**, never which opportunities qualify; because cash is unconstrained, full-size and *add-style* entries are never gated on observed Schwab cash;
- run-level market context — the **risk-free rate** (FRED `DGS10` / `DGS2`) that anchors the engine's discounting and scenario targets; **cyclical commodity prices** (FRED daily energy + monthly IMF metals, Stooq futures) read both as the **commodity-cyclical discovery feeder** (a price turn surfaces the sleeve) and as per-candidate context; **CFTC Commitments-of-Traders positioning** on the bellwether contracts (the commodity sleeve's positioning read); and the **CBOE venue-level put/call backdrop** (a broad-market sentiment context, not a per-name signal);
- the **prior run's persisted opportunity matrix**, loaded for the Step-7 carry-forward continuity check.

## Step 3: Candidate Discovery

**Type:** mixed — two converging feeders (see 3a–3b). This is where ideas are **found**, not merely vetted: research is a first-class *discovery* feeder that runs here, before candidate selection, not only as a downstream validation step ([trade-opportunities.md §The pipeline](trade-opportunities.md#the-pipeline)). Both feeders are **fail-soft** — a failed screen or theme sweep degrades to fewer candidates, never a failed run.

### Step 3a: Bottom-Up Structured Screens

**Type:** API retrieval (FMP discovery layer + FRED / Stooq + FINRA) + Computed (engine screens). No model.

Cheap, broad, structured screens over the whole universe, run deterministically and tagged with the signal that surfaced each name:
- **FMP discovery layer → the quant composite** — the company **screener** (market cap / sector / liquidity / valuation extremes) and the **bulk** endpoints (`scores-bulk`, `earnings-surprises-bulk`, `rating-bulk`, `ratios-ttm-bulk`, `key-metrics-ttm-bulk`, `price-target-summary-bulk`, `upgrades-downgrades-consensus-bulk`, plus the **growth-bulk** family and `dcf-bulk`), plus the report's free movers / earnings-calendar feeders. A handful of calls return hundreds of names, which the engine ranks by an **integrated, cross-sectionally rank-normalized multi-factor composite** ([trade-opportunities.md §The research method](trade-opportunities.md#the-research-method)) — value (incl. the `dcf-bulk` fair-value gap), quality (gross-profitability-anchored), price + **fundamental momentum** (the growth-bulk acceleration), low-volatility, and the **estimate-revision / earnings-surprise** flow — scored as one composite over the **whole cross-section and all market caps** (breadth is the edge), with **size read only within quality** and a **tradability flag** (illiquidity / days-to-cover) attached rather than excluding small names. The **forensic pre-screen** (`scores-bulk` Altman Z + Piotroski F) drops the universe's distress / earnings-management tail before the expensive stage;
- **commodity-price turns** (FRED / Stooq) for the **cyclical sleeve** — a spot/contract-price turn at washed-out sentiment surfaces commodity-cyclical candidates;
- **positioning & event scans** — the **market-wide insider-buy feed** (`insider-trading/latest`) for cluster buys, new institutional / activist stakes (FMP), **short-interest extremes** (FINRA's consolidated biweekly file, fetched once per run), notable **congressional** buys (FMP), and the **event feeds** (`mergers-acquisitions-latest`, `sec-filings-8k`, and the `news/*-latest` headlines) for fresh catalysts.

The engine reduces each screen to a ranked candidate set with its surfacing signal attached. Because the bulk endpoints cover the universe in a few calls, the per-symbol rate cap never binds at this stage ([data-sources.md §Trade Opportunities — endpoint surface](data-sources.md#trade-opportunities--endpoint-surface)).

### Step 3b: Top-Down Theme & Event Scan

**Type:** Local-model call (122B, thinking) + API retrieval (news stack + web tool), **looped**. A research-active feeder — it *generates* candidate names, not just context.

The reasoner builds the **forward thematic map** — the job's worldview layer ([trade-opportunities.md §The research method](trade-opportunities.md#the-research-method)) — through the **same bounded per-topic research loop the per-candidate stage uses** ([web-research.md](web-research.md)), here aimed at *discovery*. It works a **small fixed discovery agenda** — a handful of defined topics, **each its own model call over a clean context** so the local model is never handed a sprawling prompt:
- **active secular themes & their S-curve position** — which themes are inflecting, and where each sits on its adoption / cost curve (low penetration + falling unit cost = durable; story-only with no anchor = parked);
- **policy / regulatory / geopolitical ignition points** — dated public catalysts and the industries they lift;
- **cyclical & commodity turns** — supply / demand inflections for the cyclical sleeve;
- **theme → value chain → exposed names** — tracing each live theme down to its beneficiaries, deliberately past the crowded pure-plays to the **picks-and-shovels enablers** (often mid / small cap), resolved against FMP's industry classification.

The loop is bounded exactly as the per-candidate one: **per-topic depth ≤2 (≤3 passes/topic)** and a **per-run discovery fetch + wall-clock budget that binds first**, spent in topic-priority order, fail-soft on exhaustion — so the discovery research has a hard ceiling and can never run away. The Step-2 house view steers the hunt (aligning it with the current regime / thesis) without confining it. Search is **SearXNG-primary** (keyless, cost-free); the **FMP structured news feeds** (`news/general-latest`, `news/stock-latest`, `news/press-releases-latest`, on the paid key) plus the keyless GDELT and FMP Articles feeds and the macro-release calendar **seed** the news topics with ticker-tagged, dated headlines, which the web tool then **deep-reads**; **Tavily is only the fallback when SearXNG is down**.

#### Local-model call — Theme & exposed-name discovery (Qwen3.5-122B, thinking)

**Model.** The resident 122B reasoner in thinking mode, requesting `web_search` / `web_fetch` tool calls the orchestrator executes (SearXNG-primary, Tavily fallback; SSRF-guarded; untrusted page text inserted as quoted evidence, never as instructions — see [web-research.md §Safety and provenance](web-research.md#safety-and-provenance)). The orchestrator owns every request and the per-topic depth / fetch / wall-clock budget, exactly as in the per-candidate loop.

**Prompt — input.** **One discovery topic's questions at a time** over a clean context — the Step-2 house view (regime, themes, forward outlook) plus the keyless news / macro-release inputs relevant to that topic; topics never share a context.

**Returns.** A schema-validated **forward thematic map** (each live theme with its S-curve position and value-chain mapping) and the **(theme → exposed candidate names)** set — each name with its surfacing rationale and source URLs — flowed into Step 4 alongside the bottom-up screen output. The thematic map is retained as run-level worldview context for the per-candidate thematic-fit read (Step 5d). The model proposes names; it neither fetches per-symbol data nor scores them here.

## Step 4: Candidate Consolidation

**Type:** Computed (app layer) — a deterministic merge before any per-candidate stage. No model.

The two feeders' outputs are **deduped** into one candidate list, **sanity-filtered for tradability** (exchange listing, a liquidity / price floor, an instrument-type filter — funds and non-equities drop out, since the job hunts operating businesses by archetype), and each surviving name is **tagged with every signal that surfaced it** (which screen, which theme, the positioning flag). A bounded cap keeps the per-candidate budget finite, ranked by signal strength and house-view fit. The result is the narrowed candidate set the expensive per-name loop runs over — the funnel's waist.

## Step 5: Per-Candidate Validation Loop

Each candidate from Step 4 is processed through the chain below. Candidates are independent, so the loop **checkpoints per candidate** — each candidate's completed stages persist, so a cancellation or a single model failure resumes the unfinished candidates rather than restarting the (potentially long) run, and recent research is cached within a freshness window ([trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture)). The resident **122B reasoner fills every model role in this loop** by switching mode (thinking ↔ non-thinking), so moving a candidate across archetype confirmation (thinking), its research passes (thinking), the single distillation (non-thinking), and scoring (thinking) pays no model-swap cost ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)). The shared **financial-analysis engine** is the same one Portfolio Analysis uses ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)); the difference is the **archetype** selects which signals it weights and which valuation lens it applies.

### Step 5a: Archetype Classification

**Type:** Computed (deterministic features) + Local-model call (confirmation). The lens that decides which signals matter for this candidate.

The application computes the candidate's **classification features** — sector / industry (FMP `profile`), margin and recurring-revenue structure, cyclicality, and the signals that surfaced it (Step 4 tags) — and the reasoner **confirms** one of the five archetypes (secular-compounder / ai-infra / commodity-cyclical / disruptor / quality-compounder). The confirmed archetype selects the **signal weighting and valuation lens** for 5c–5g ([trade-opportunities.md §Archetype](trade-opportunities.md#archetype--the-lens-that-decides-which-signals-matter)) — e.g. a commodity cyclical is judged on P/B, P/NAV, and mid-cycle EPS with trailing P/E suppressed, while an ai-infra name is judged on segment-revenue acceleration and forward P/E against its revision rate — and informs the deterministic risk-tier and horizon reads. Archetype is a first-class dimension (deterministic features, model-confirmed); it feeds the risk-tier rule rather than replacing it.

#### Local-model call — Archetype confirmation (Qwen3.5-122B, thinking)

**Model.** The resident 122B in thinking mode; schema-constrained output.

**Prompt — input.** The candidate's classification features and surfacing signals only — a compact, clean context. No web access (this is a classification, not research).

**Returns.** The schema-validated **archetype label** plus a short rationale and a confidence; a low-confidence or contradictory classification flags the candidate for a wider valuation read rather than committing to one lens.

### Step 5b: Dossier Assembly

**Type:** API retrieval (FMP per-symbol / SEC EDGAR / Stooq / FINRA / Schwab chains) + Local-model call (embedding, for continuity retrieval) + Computed (assemble the packet).

The application builds the candidate's evidence packet deterministically: the **FMP per-symbol surface** on the paid key — fundamentals (statements / ratios / key metrics, `financial-scores` for Altman Z + Piotroski, owner earnings, enterprise values, DCF, **multi-year `financial-growth`** per-share CAGRs), **product / geographic revenue segments**, the **analyst / revision signal** (forward estimates snapshotted for velocity, price-target consensus + summary trend, the `grades-historical` rating distribution + consensus, upgrades / downgrades, earnings surprises), **symbol-keyed positioning** (insider, 13F institutional **with holder-level analytics** — who's adding / new / sold-out and at what cost — **SC 13D / 13G activist filings**, Senate / House congressional), **stock peers**, **share float / liquidity**, the **next earnings date** (catalyst), **earnings-call transcripts** (backlog / book-to-bill / guidance / supply-discipline language), the **symbol-scoped structured news** (`news/stock`, `news/press-releases` — the narrative / sentiment / catalyst feed the research lane deep-reads), and **M&A involvement** (acquirer or target) — joined with **SEC EDGAR** (10-K/Q/8-K + XBRL company facts) as the authoritative cross-check; **FINRA** short interest (looked up in the once-per-run file); **Stooq** deep price history and a live **FMP `quote`**; the **Schwab option chain** for the candidate if it is optionable (→ the options-activity signal, computed in 5c); the Step-2 shared context; and vector-retrieved continuity from **this job's own prior runs** for this candidate. The full input list and every endpoint, with cardinality, is in [trade-opportunities.md §Signal inputs](trade-opportunities.md#signal-inputs) and [data-sources.md §Trade Opportunities — endpoint surface](data-sources.md#trade-opportunities--endpoint-surface). This is the **per-candidate** surface — the budget driver — so it runs only for the narrowed set, never the discovery longlist.

#### Local-model call — Vector continuity retrieval (Qwen3-Embedding-4B, fixed)

**Model.** The fixed local embedder — vectorization only, no reasoning. Shares the `Embedder` trait the report pipeline defines; only the vector space differs.

**Prompt (input text).** A query string built deterministically from the candidate (symbol, archetype, sector/industry, and the prior opportunity's thesis themes if carried), byte-capped before the call.

**Returns.** A fixed-dimensionality vector; the application runs a brute-force cosine search scoped to the **Trade Opportunities** memory partition (the job namespace — never the report's or Portfolio Analysis's — see [local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)) and carries the relevant prior analysis into the dossier.

### Step 5c: Deterministic Analysis (Archetype-Weighted Engine)

**Type:** Computed (the financial-analysis engine, shared with Portfolio Analysis). No model.

The engine computes the candidate's quantitative picture, **weighted by the Step-5a archetype** — the archetype selects which sub-scores dominate and which valuation lens applies. It produces: the **multi-factor quant composite** (value / quality anchored on gross-profitability / price + fundamental momentum / low-volatility / revision flow — the same factors the discovery screen ranks on, recomputed precisely for this name) and the **value-creation read** (**ROIC vs cost of capital**, the reinvestment runway *g* ≈ ROIC × reinvestment, owner earnings with **R&D capitalized** so research-heavy names aren't mis-scored, and the moat-source features); the **leading-metric series** (revision velocity, segment-revenue acceleration, commodity-price turn, or margin decoupling, per archetype); the **earnings surprise / SUE**; **positioning** (insider net, 13F, congressional, short interest, COT for the cyclical sleeve, and the **options-activity signal** from the Step-5b chain — put/call by volume and open interest, IV/skew); scenario **price targets** discounted off the run-level risk-free rate (methodology recorded); a **price-action confirmation read** (relative strength vs the market / sector and proximity to a multi-year base breakout, from Stooq deep history — a confirmer, not a trigger); and the **two derived reads selection leans on** — the **narrative-vs-reality ratio** (estimate-revision pace vs multiple change: *justified-expensive* when estimates outrun the multiple, *hype* when the multiple outruns flat / declining estimates) and the **forensic flags** (margin compression while revenue accelerates, net-income-vs-operating-cash-flow divergence, receivable / inventory build outpacing sales, restatement / auditor-change history, plus `financial-scores` Altman Z + Piotroski). A **tradability flag** (Amihud-style illiquidity + days-to-cover) rides alongside, so a small / illiquid name is flagged rather than silently excluded. The forward targets are a **provisional scenario menu** at this point. The model interprets these reads; it never invents one ([trade-opportunities.md §The pipeline](trade-opportunities.md#the-pipeline)).

### Step 5d: Bounded Web Research

**Type:** Local-model call (122B, thinking) + API retrieval (the web tool), **looped**. This is the only stage in the loop that itself loops.

The reasoner sets a small **agenda** that works the three research lenses of [trade-opportunities.md §The research method](trade-opportunities.md#the-research-method), **one topic at a time** — **each topic a separate model call and research loop over a clean context** (the dossier facts plus that topic's own questions; no other topic's findings are fed in), so the local model reasons over a tight prompt, never a sprawling one:
- **leading-metric validation** — confirm the engine's leading metric is real, countable, dated, and inflecting from a third-party source (the mandatory anchor);
- **macro / thematic-fit** — which theme the name rides and where it sits on the S-curve (against the Step-3b thematic map), pure-play vs enabler, bottom-up TAM, and the economist's front-running indicators (capex commentary, book-to-bill, freight, the cycle) the structured feeds don't carry;
- **investor-judgment** — the driving **narrative and market sentiment** (how much of the price is emotion about *what might come* vs present fundamentals), management / capital-allocation quality, and the pre-consensus tells — seeded by the candidate's symbol-scoped FMP news + press releases (Step 5b) and deep-read on the open web;
- **external corroboration** the feeds can't give — customer / hyperscaler capex, supply discipline, transcript backlog / TAM / inflection language, and **DRAM/NAND ASP direction**;
- **mandatory — the contemporaneous bear case** — why the name might fail (the winning traits also rode the famous failures down).

The orchestrator — not the model — owns every request, so the loop has a hard ceiling: **per-topic depth ≤2 (≤3 passes/topic)** and a **per-candidate fetch + wall-clock budget that binds first**, spent in topic-priority order (leading-metric and bear case first), fail-soft on exhaustion — the lowest-priority topics drop to a recorded gap rather than overrunning the run. Grounded throughout by the engine's quant and value-creation numbers so research builds the case around the leading metric rather than substituting a story for it. The full loop and its bounds are in [web-research.md](web-research.md).

#### Local-model call — Per-candidate research (Qwen3.5-122B, thinking)

**Model.** The resident 122B reasoner in thinking mode, requesting `web_search` / `web_fetch` tool calls the orchestrator executes (SearXNG-primary, Tavily fallback; SSRF-guarded; untrusted page text inserted as quoted evidence, never as instructions). **One call per agenda topic** — topics do not share a context.

**Prompt — input.** The candidate's dossier facts, its archetype and computed leading-metric reads, and **that topic's questions only** — a clean context per topic. Within a pass the model reasons over the fetched, readability-extracted page text and an **append-only evidence ledger** (each extracted claim + its source URL / timestamp); there is **no in-loop re-distillation of findings** — the heavy consolidation is deferred to the single Step-5e distillation, so research is never planned over already-distilled, lossy notes.

**Returns.** The topic's **full findings response**, preserved whole (with its evidence-ledger entries), plus any **follow-up proposal** (a structured field the orchestrator decides whether to spend) and any **material forward fact** flagged for the Step-5f refinement. The **bear-case topic is non-optional** — the candidate cannot reach scoring without a stated, sourced bear case (the winning traits also appeared in the famous failures). Every topic's full response flows intact to distillation — nothing is summarized away in between.

### Step 5e: Distillation

**Type:** Local-model call (122B, non-thinking; the optional 35B fast tier if resident). Consolidation, not new reasoning.

The reasoner in non-thinking mode consolidates the topics' **full findings responses** into the compact object the scoring stage reads — **a single consolidation pass over the complete per-topic outputs**, not a re-distillation of already-distilled notes — so scoring reasons over a clean synthesis of full-context research. This is the *only* place research is condensed before scoring.

#### Local-model call — Distillation (Qwen3.5-122B, non-thinking)

**Model.** The same resident 122B in non-thinking mode by default (no model-swap cost); the fast 35B tier is a benchmark-gated option ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).

**Prompt — input.** The **full findings response from every topic** (including the bear-case topic) plus the append-only evidence ledger (claims + sources).

**Returns.** A single schema-validated **distilled findings object** for scoring — the leading-metric validation, the narrative/sentiment read, the forward-opportunity read, and the bear case, each cited — surfacing any **material forward fact** the structured feeds lacked as a typed **`research_forward_assumption`** — `{ fact type, numeric value, units, period / as-of date, source URL, confidence, the target assumption it affects, conflict handling }` — so only a structured, sourced, numeric claim (never loose prose) can reach the engine's target refinement.

### Step 5f: Deterministic Target Refinement

**Type:** Computed (the engine). No model.

If distillation produced a typed **`research_forward_assumption`** (Step 5e — a guidance figure, a signed-contract value, a commodity / ASP turn, each with value, units, as-of date, source, and confidence), the **engine — not the model —** recomputes the affected scenario target with it as an explicit, **logged** assumption. A malformed, unsourced, or non-numeric claim is **rejected** (it cannot move a target), and a fact that **conflicts** with a structured feed is resolved by the assumption's declared conflict-handling rule. So the number stays engine-computed while the forward view reflects what research learned. The backward-looking sub-scores and derived reads are untouched; absent a valid assumption, the Step-5c targets stand. This is the same refinement contract Portfolio Analysis uses ([portfolio-workflow.md §Step 6e](portfolio-workflow.md#step-6e-deterministic-target-refinement)).

### Step 5g: Scoring & Gating

**Type:** Local-model call (122B, thinking). The opportunity-authoring call — but it does **not** pick the risk tier or the horizon (those are deterministic, Step 5h).

The reasoner interprets the computed analysis and the distilled research into the candidate's **opportunity record**, under explicit discipline: **score the conjunction, never a single signal** (base rates are brutal — most stocks underperform Treasuries over their life, and the winner traits recur in losers); **require the leading-metric anchor plus external validation**; **apply the narrative-vs-reality ratio**; treat **price action (relative strength / base breakout) as a confirmation overlay** that adjusts conviction but never substitutes for the leading-metric anchor. It reads every number from the engine rather than inventing it. The scoring runs the candidate down the archetype's **track** — *proven-economics* (trailing returns on capital + a margin of safety) or *emerging-economics* (a forward TAM × penetration × margin model clearing a return hurdle) — but **both tracks pass through the same moat / management / price-asymmetry gate** ([trade-opportunities.md §The research method](trade-opportunities.md#the-research-method)), so a strong-numbers name and a revolutionary one are judged on one spine without collapsing into pure value or pure momentum.

#### Local-model call — Scoring & gating (Qwen3.5-122B, thinking)

**Model.** The resident 122B in thinking mode; schema-constrained output.

**Prompt — input.** The engine's computed analysis (archetype-weighted sub-scores, the refined scenario targets with exposed methodology, the narrative-vs-reality and forensic reads, the price-action confirmer, the positioning / options signal as context); the distilled research findings including the mandatory bear case; the candidate's archetype and surfacing signals; the house view and investor profile; and any prior opportunity record for this name. The **absolute street opinions** (consensus target level, current rating consensus, FMP's ratings snapshot) are presented as *evidence to weigh against the engine's own read*, not as numbers to adopt.

**Returns.** The schema-validated **opportunity record** ([trade-opportunities.md §The opportunity](trade-opportunities.md#the-opportunity)) — directional thesis, **detection mode** (early / continuation), the **leading operating metric** and its trend, the catalyst, **conviction** (shaped by narrative-vs-reality; abstaining as `insufficient-evidence` below the evidence floor), the **narrative-vs-reality read**, the **mandatory bear case**, the entry consideration, and any tripped **risk / forensic flags** — plus a proposed **carry-forward status** (`new` / `still-valid` / `played-out` / `invalidated`) with each status / conviction move attributed to an input that changed (Step 7 validates the attribution). The model proposes the record; it does **not** assign the risk tier or horizon.

### Step 5h: Deterministic Risk-Tier, Gate Validation & Checkpoint

**Type:** Computed (app layer). No model.

This step is an **app-layer validator and tier-assigner**, not just a recorder:
- the **risk tier** (high / medium / low) and the **horizon** (short / mid / long) are assigned **deterministically by rule** from the engine's measurable inputs (profitability, market cap, liquidity, volatility, leverage, drawdown, event exposure — archetype-informed), so the model's conviction never sets the cell ([trade-opportunities.md §The opportunity space](trade-opportunities.md#the-opportunity-space)). Tier + horizon → the matrix cell;
- the **forensic / risk gate** is enforced in the app, not left to the model: a candidate tripping the forensic flags (Step 5c), or whose move is mostly multiple-expansion (a `hype` narrative-vs-reality read with no leading-metric anchor), is **capped or excluded** rather than promoted — a hard rule the model's enthusiasm can't override;
- a candidate that abstained as `insufficient-evidence` (below the **archetype-aware evidence floor** — missing the floor-bearing price + validated leading metric, or statements-or-substitute per archetype, or carrying stale / conflicting data — see [trade-opportunities.md §Evidence floor](trade-opportunities.md#evidence-floor)) is held out of the matrix rather than promoted as a low-conviction guess;
- the surviving candidate is **checkpointed** so the run can resume here.

## Step 6: Selection & Matrix Assembly

**Type:** Local-model call (122B) — synthesis across the gated candidate set, per cell.

#### Local-model call — Per-cell selection (Qwen3.5-122B)

**Model.** The resident 122B reasoner; schema-constrained output.

**Prompt — input.** All gated opportunity records from Step 5 with their assigned cells; the house view and investor profile; and, for context, the count of candidates competing for each cell.

**Returns.** The schema-validated **3×3 matrix** — for each of the nine risk × horizon cells, the small set of opportunities that qualify, deduped against competing names and ranked, applying the cross-candidate base-rate discipline (a cell holds only genuinely distinct, anchored ideas). **A cell may return no opportunity** when nothing qualifies — empty cells are honest, not failures, and the matrix never pads itself to fill them ([trade-opportunities.md §The pipeline](trade-opportunities.md#the-pipeline)). The model selects and ranks within the deterministically-assigned cells; it cannot move a candidate to a different tier or horizon.

## Step 7: Continuity Check & Carry-Forward

**Type:** Computed (app layer) — an **app-layer validator**, not just a recorder. No model.

Prior opportunities (from the Step-2 prior run) are reconciled with this run's matrix, mirroring the discipline of Portfolio Analysis's what-changed audit ([portfolio-workflow.md §Step 6g](portfolio-workflow.md#step-6g-continuity-check-and-checkpoint)) at the opportunity-list level:
- every opportunity carried forward keeps its identity and gets an **updated status** (`new` / `still-valid` / `played-out` / `invalidated`); an **addition or removal must be justified by what changed**;
- each status / conviction move the Step-5g record claims as **external** must resolve to a concrete entry in the engine's deterministic input delta (this run's metrics / positioning / price vs the prior run's stored values), a source-backed research finding, or the logged `research_forward_assumption`; an attribution that resolves to nothing is **downgraded to a self-correction** (or fails schema validation), so the model cannot launder a no-new-facts swing as "the thesis changed";
- the **continuation-failure alarms are first-class deterministic exit signals** — estimate revisions rolling over, a beat-and-raise streak breaking, shipments diverging below sell-through — and flip a still-valid idea to `played-out` or `invalidated` from the engine's input delta, not the model's mood.

Output stays **firm and does not churn** between runs absent hard data ([thesis-continuity.md](thesis-continuity.md)) — an idea persists with an updated status rather than silently reappearing or vanishing.

## Step 8: Holdings Cross-Reference

**Type:** Computed (app layer) — a deterministic post-step. No model.

After selection and continuity, the application flags any opportunity that **overlaps the user's current holdings** (owned / not-owned), reading only the holdings list from the most recent Schwab pull — **never** the Portfolio Analysis memory partition. Crucially this runs *after* discovery and selection, so holdings **never influence which opportunities are found or chosen** ([trade-opportunities.md §Holdings cross-reference](trade-opportunities.md#holdings-cross-reference)); the job stays genuinely independent of the account, surfacing an already-held idea as such only for the user's awareness.

## Step 9: Persist Run and Audit, with Memory Embeddings

**Type:** Computed (persist the matrix and audit record) + Local-model call (embeddings for continuity).

The application persists the run: the **3×3 matrix** of opportunities, and an **audit record** that makes the run traceable — the report(s) and sources used with retrieval timestamps, the **discovery and screening inputs** (which screens / themes surfaced each candidate), the distilled findings, the computed signals and the **bear case** behind each pick, the **input delta and the carry-forward attribution**, the price-target methodology including its discount-rate assumption and any research-sourced forward assumption (with source), the model ids and quantizations, the prompt/schema version, and degraded-input flags ([trade-opportunities.md §Storage and display](trade-opportunities.md#storage-and-display)). Retention keeps the last N runs ([storage.md](storage.md)).

#### Local-model call — Run-result embeddings (Qwen3-Embedding-4B, fixed)

**Model.** The fixed local embedder — vectorization only.

**Prompt (input text).** Each opportunity's record summary, embedded individually.

**Returns.** Vectors stored in the **Trade Opportunities** memory partition (the job namespace), so a later run of this job can semantically recall the relevant prior analysis for a name ([local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)). Best-effort: a failed embedding costs the memory row, never the persisted run.

## Step 10: Generate Trade Opportunities Page and Update UI

**Type:** Computed (frontend). No model.

The **Trade Opportunities page** renders the 3×3 matrix — high-, medium-, and low-risk sections, each containing short-, mid-, and long-term ideas — each cell listing its opportunities with archetype, directional thesis, leading metric, catalyst, conviction, narrative-vs-reality read, entry consideration, bear case, status, and the owned/not-owned flag, with empty cells shown as empty ([interface.md](interface.md)). While the job ran, the run tracker replaced the page (latest-run-only); on completion the page shows the persisted matrix. A **run is never a report**: the matrix appears only on persisted success, so a cancel or failure removes nothing ([run-tracking.md](run-tracking.md)).
