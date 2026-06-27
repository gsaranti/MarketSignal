# Portfolio Analysis Workflow

Portfolio Analysis is one of the two local-suite jobs ([local-models.md](local-models.md)). This document specifies its end-to-end control flow; the feature's design rationale — the verdict schema, the engine's three layers, the evidence floor, the roll-up — lives in [portfolio-analysis.md](portfolio-analysis.md).

The Portfolio Analysis job:
- pulls the user's Charles Schwab holdings (and live option chains)
- classifies each position by asset type and diffs it against the prior run
- computes a deterministic financial picture for every gradable holding
- researches each holding on the open web with a local reasoner
- grades each holding (A–F) and recommends an action + price targets
- synthesizes a portfolio-level roll-up against the Market Signal house view

It runs **on demand only**, in two user-controlled steps — **pull holdings**, then **run analysis** — entirely on local models, with **no cost at the model layer**. A **single global run slot** serializes it against the report and Trade Opportunities (only one runs at a time). For job states, the global run slot, cancellation, and error handling, see [scheduling.md](scheduling.md) and [run-tracking.md](run-tracking.md); for the failure posture (per-holding checkpoint/resume, fail-soft research), see [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).

## How to read this workflow

Every step below is tagged with a **Type** so it is obvious what the step actually does:

- **Computed (app layer)** — deterministic Rust logic, with no model and no external network: local SQLite and filesystem reads, the holdings diff, and the **financial-analysis engine** (every sub-score, target, and derived read).
- **API retrieval** — fetches from external sources: holdings and option chains from **Charles Schwab** (account-scoped, via OAuth — see [schwab-integration.md](schwab-integration.md)); company data from **FMP / SEC EDGAR / Stooq**; run-level macro and positioning from **FRED / CFTC**; and the **web tool** (SearXNG-primary, Tavily fallback) the orchestrator runs *on a model's behalf*. The full per-source endpoint surface, with each call's per-holding / per-fund / run-level cardinality, is in [data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface).
- **Local-model call** — invokes a model on the app-supervised **Ollama** daemon ([local-models.md §Serving runtime](local-models.md#serving-runtime)): the primary reasoner **`Qwen3.5-122B-A10B`** in **thinking** mode (multi-step research and interpretation) or **non-thinking** mode (firm, directed consolidation), or the fixed **`Qwen3-Embedding-4B`** embedder (vectorization only). Every generative call is **schema-constrained** via Ollama's native `format` parameter — the model picks values, never structure.

Two load-bearing architectural rules frame the whole table, the same ones the report pipeline holds: **agents are pure stages, and the application layer owns all I/O** — a model stage consumes the structured input handed to it and emits a schema-validated result; when a research stage needs the web it *requests* a tool call and the orchestrator performs the fetch. And **the engine computes every number** ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)) — the model interprets computed values and never invents one. For each model stage, the **Local-model call** block lists what the prompt includes and what the model returns. Per-step progress, per-request rows, and token/reasoning output stream to the run tracker over the shared `progress` seam ([run-tracking.md](run-tracking.md)), exactly as a report run does.

| Step | Stage | Type | Model |
|---|---|---|---|
| 1 | Job start & gate | Computed | — |
| 2 | Load holdings & fetch option chains | API retrieval (Schwab) + Computed | — |
| 3 | Classify asset eligibility | Computed | — |
| 4 | Holdings change diff | Computed | — |
| 5 | Load shared context (house view, profile, run-level FRED/CFTC) | Computed (local read) + API retrieval | — |
| 6 | **Per-holding analysis loop** (per eligible holding, checkpointed) | mixed — see 6a–6g | 122B + embedder |
| 6a | Dossier assembly | API retrieval + Local-model (embedding) + Computed | Qwen3-Embedding-4B · fixed |
| 6b | Deterministic financial analysis | Computed (engine) | — |
| 6c | Bounded web research | Local-model call (thinking) + API retrieval (web tool), looped | Qwen3.5-122B · thinking |
| 6d | Distillation | Local-model call (non-thinking) | Qwen3.5-122B · non-thinking (35B optional) |
| 6e | Deterministic target refinement | Computed (engine) | — |
| 6f | Interpretation & grading | Local-model call (thinking) | Qwen3.5-122B · thinking |
| 6g | Continuity check & checkpoint | Computed | — |
| 7 | Portfolio roll-up | Local-model call | Qwen3.5-122B |
| 8 | Persist run & audit + memory embeddings | Computed (persist) + Local-model (embedding) | Qwen3-Embedding-4B · fixed |
| 9 | Render Portfolio page & update UI | Computed (frontend) | — |

## Step 1: Job Start and Gate

**Type:** Computed (app layer) — the local-suite execution gate. No model and no external API (credential and daemon *presence/reachability* are checked, not analysis).

The job will not start unless three preconditions hold:
- the **single global run slot** is free (no report or other local job is running — see [scheduling.md §Concurrent Job Protection](scheduling.md#concurrent-job-protection));
- the **local-model daemon is reachable and the configured roster is present** (the 122B reasoner + the embedder) — health-checked at the Ollama endpoint ([local-models.md §Serving runtime](local-models.md#serving-runtime));
- a **connected Schwab account** with a valid (≤7-day) refresh token ([schwab-integration.md §A connected Schwab account is required](schwab-integration.md#a-connected-schwab-account-is-required)).

This gate is **independent of the cloud-report gate** — a machine with no OpenAI/Anthropic keys can still run the local suite. A failed precondition blocks the job and surfaces in the Persistent Warning Area under its own categories (local models unavailable, Schwab connection), without creating duplicate warnings. Manual-import holdings do **not** satisfy the Schwab gate.

## Step 2: Load Holdings and Fetch Option Chains

**Type:** API retrieval (Schwab) + Computed (load the last persisted pull). No model.

Holdings come from the user's most recent **pull holdings** action (the first of the two trigger steps), persisted so the portfolio is viewable without re-fetching. Each position carries instrument identity (symbol, CUSIP, asset type), quantity, average cost (cost basis), market value, and P/L, from `GET /trader/v1/accounts/{accountHash}?fields=positions` (Schwab identifies accounts by a hashed number; the app resolves plaintext→hash first). **Manual-import** positions (CSV/paste) populate the same holdings model as a supplement.

**Option chains are fetched fresh at job start** (not piggybacked on the holdings pull) from `GET /marketdata/v1/chains` — per-contract volume, open interest, IV, and greeks — bounded by expiration and strike range, carrying an as-of timestamp and **rejected if stale** (mirroring the report's COT freshness guard). A symbol with no listed options degrades to a gap, never a job failure. The deterministic put/call + IV/skew signal these chains feed is computed later, per holding, in Step 6b.

## Step 3: Classify Asset Eligibility

**Type:** Computed (app layer). No model.

Each position is classified before analysis (see [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)):
- **Stocks** — the full per-holding pipeline (Step 6, equity path).
- **ETFs / funds** — the **reduced** pipeline (Step 6, fund path): no single-company financials; graded on strategy / **exposure** (sector / country weightings — constituent look-through is off-plan, see [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)), valuation, and the house view.
- **Options, fixed income, cash, unsupported types** — marked **not rated**, with a reason, excluded from grading. Cash and buying power still feed the investor profile and the roll-up.

The eligibility decision is explicit and shown in the UI; a not-rated position never receives a fabricated grade.

## Step 4: Holdings Change Diff

**Type:** Computed (app layer) — a deterministic diff before any model stage. No model.

The current holdings are diffed against the **prior run's persisted snapshot** (see [portfolio-analysis.md §Holdings change tracking](portfolio-analysis.md#holdings-change-tracking)). Every current position is tagged by quantity and cost basis as **new / increased / decreased / unchanged**; a symbol present last run but absent now is **exited** (no per-holding verdict — there is nothing left to grade — but surfaced in the Step-7 roll-up as closed-since-last-run). Each holding's delta rides into its dossier so the verdict reasons over what the user actually did. The diff is the application's, not the model's.

## Step 5: Load Shared Context

**Type:** Computed (local read — house view, investor profile) + API retrieval (run-level FRED / CFTC). No model.

Three things are loaded **once per run and shared across every holding**, not re-requested per symbol:
- the **Market Signal house view** — the latest report's Thesis, Investment Strategy, and Forward Outlook sections plus recent report summaries (`thesis_stance`, `forward_outlook_themes`, `key_risks`), loaded **deterministically** from the report store (retrieve-don't-dump — never by vector-searching the report's memory; see [local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)). The report's **creation date** rides into the dossier so every downstream stage knows how old the thesis is, and a **freshness window applies**: if the latest report is older than **one week** (a pinned default), the house view is **omitted and recorded as a gap** rather than fed as current — a month-old thesis is not today's, and the data-honesty stance treats a stale input as absent, not current (the same posture the report takes on a stale data series). The holding is still graded on its fundamentals, research, and profile; it simply carries no house-view anchor that run;
- the **investor profile** (risk tolerance, horizon, tax sensitivity, available cash / buying power — see [configuration.md](configuration.md));
- run-level market context — the **risk-free rate** (FRED `DGS10` / `DGS2`) that anchors the engine's discounting, **cyclical commodity prices** (FRED) for commodity-linked holdings, and **CFTC Commitments-of-Traders positioning** on the bellwether contracts, which a commodity / macro **fund** holding maps onto for an underlying-positioning read.

## Step 6: Per-Holding Analysis Loop

Each **gradable** holding (stock or fund, from Step 3) is processed through the chain below. Holdings are independent, so the loop **checkpoints per holding** — each holding's completed stages persist, so a cancellation or a single model failure resumes the unfinished holdings rather than restarting the (potentially hours-long) run, and recent research is cached within a freshness window ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)). The resident **122B reasoner fills every model role in this loop** by switching mode (thinking ↔ non-thinking), so moving a holding across its research passes (thinking), the single distillation (non-thinking), and interpretation (thinking) pays no model-swap cost ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)). A **fund** holding runs the reduced engine path (Step 6b) and skips nothing else structurally. Sub-steps 6a–6g are the [portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline) six stages, with the target refinement (6e) surfaced as its own deterministic phase.

### Step 6a: Dossier Assembly

**Type:** API retrieval (FMP / SEC EDGAR / Stooq / FINRA) + Local-model call (embedding, for continuity retrieval) + Computed (assemble the packet).

The application builds the holding's evidence packet deterministically: the position + its Step-4 delta; the **equity** per-symbol surface (FMP fundamentals + revenue segments + analyst/revision signals + FINRA short interest, joined with SEC EDGAR as the authoritative cross-check; **13F institutional, earnings-call transcripts, and per-symbol M&A are off-plan** → SEC EDGAR / the web-research loop / `mergers-acquisitions-latest`+8-K — [data-sources.md §FMP — current paid-plan tier audit](data-sources.md#fmp--current-paid-plan-tier-audit)) or, for a fund, the **reduced ETF surface** (`etf/info` + sector/country weightings; constituent `etf/holdings` and mutual-fund `funds/disclosure*` off-plan); price history (Stooq) and a live quote (FMP `quote`); the prior run's verdict for this holding; the Step-5 shared context; and vector-retrieved continuity from **this job's own prior runs** for this holding. The full input list and every endpoint is in [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) and [data-sources.md](data-sources.md#portfolio-analysis--endpoint-surface).

#### Local-model call — Vector continuity retrieval (Qwen3-Embedding-4B, fixed)

**Model.** The fixed local embedder — vectorization only, no reasoning. Shares the `Embedder` trait the report pipeline defines; only the vector space differs.

**Prompt (input text).** A query string built deterministically from the holding (symbol, sector/industry, and the prior verdict's themes), byte-capped before the call.

**Returns.** A fixed-dimensionality vector; the application runs a brute-force cosine search scoped to the **Portfolio Analysis** memory partition (the job namespace — never the report's or Trade Opportunities' — see [local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)) and carries the relevant prior analysis into the dossier.

### Step 6b: Deterministic Financial Analysis

**Type:** Computed (the financial-analysis engine, shared with Trade Opportunities). No model.

The engine computes the holding's quantitative picture in three layers — **(a)** the grade core (fundamentals + revenue segments + price + forensic flags → the quality / valuation / momentum / risk sub-scores and the scenario price targets, discounted off the run-level risk-free rate); **(b)** a conviction layer (revision velocity, rating drift, earnings surprise → the narrative-vs-reality ratio, kept *out* of the sub-scores); **(c)** positioning context (insider net, congressional, **FINRA short interest**, and the **options-activity signal** computed from the Step-2 chains — held out of the sub-scores until calibration; FMP 13F is off-plan → EDGAR/omit). The forward targets are a **provisional scenario menu** at this point. The full layer breakdown is in [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) Step 2. For a **fund**, this step runs the reduced computation instead (expense drag, **exposure tilt** from sector/country weightings, fund-level valuation → a reduced sub-score set; constituent concentration only if SEC N-PORT supplies it, else omitted — `etf/holdings` off-plan). The engine also computes a deterministic **input delta** — this run's metrics, sub-scores, positioning, and price against the prior run's stored values (from the audit record), together with the Step-4 position delta and the Step-5 house-view age / change — the evidence the continuity audit (6f / 6g) attributes verdict moves to.

### Step 6c: Bounded Web Research

**Type:** Local-model call (122B, thinking) + API retrieval (the web tool), **looped**. This is the only stage that loops.

The reasoner sets a small **agenda** for the holding (competitive position, recent results/estimate revisions, catalysts/risks, market narrative & sentiment, forward opportunity & thematic fit) and works it **one topic at a time** — **each topic is a separate model call and research loop**, run over a **clean context** (the dossier facts plus that topic's own questions; no other topic's findings are fed in). The orchestrator — not the model — owns every request: per-topic depth ≤2 (≤3 passes/topic) and a **per-item fetch + wall-clock budget that binds first**, spent in topic-priority order, fail-soft on exhaustion. Grounded by the deterministic financials so research fills the gaps the numbers don't. The full loop and its bounds are in [web-research.md](web-research.md).

#### Local-model call — Per-holding research (Qwen3.5-122B, thinking)

**Model.** The resident 122B reasoner in thinking mode, requesting `web_search` / `web_fetch` tool calls the orchestrator executes (SearXNG-primary, Tavily fallback; SSRF-guarded; untrusted page text inserted as quoted evidence, never as instructions — see [web-research.md §Safety and provenance](web-research.md#safety-and-provenance)). **One call per agenda topic** — topics do not share a context.

**Prompt — input.** The holding's dossier facts and **that topic's questions only** — a clean context per topic. Within a pass the model reasons over the fetched, readability-extracted page text and an **append-only evidence ledger** (each extracted claim + its source URL / timestamp); there is **no in-loop re-distillation of findings** — the heavy consolidation is deferred to the single Step-6d distillation, so research is never planned over already-distilled, lossy notes.

**Returns.** The topic's **full findings response**, preserved whole (with its evidence-ledger entries), plus any **follow-up proposal** (a structured field the orchestrator decides whether to spend) and any **material forward fact** flagged for the Step-6e refinement. Every topic's full response flows intact to distillation — nothing is summarized away in between.

### Step 6d: Distillation

**Type:** Local-model call (122B, non-thinking; the optional 35B fast tier if resident). Consolidation, not new reasoning.

The reasoner in non-thinking mode consolidates the topics' **full findings responses** into the compact object the interpretation stage reads — **a single consolidation pass over the complete per-topic outputs**, not a re-distillation of already-distilled notes — so interpretation reasons over a clean synthesis of full-context research ("forward only what's needed"). This is the *only* place research is condensed before interpretation.

#### Local-model call — Distillation (Qwen3.5-122B, non-thinking)

**Model.** The same resident 122B in non-thinking mode by default (no model-swap cost); the fast 35B tier is a benchmark-gated option ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).

**Prompt — input.** The **full findings response from every topic** plus the append-only evidence ledger (claims + sources).

**Returns.** A single schema-validated **distilled findings object** for interpretation, surfacing any **material forward fact** the structured feeds lacked as a typed **`research_forward_assumption`** — `{ fact type, numeric value, units, period / as-of date, source URL, confidence, the target assumption it affects, conflict handling }` — so only a structured, sourced, numeric claim (never loose prose) can reach the engine's target refinement.

### Step 6e: Deterministic Target Refinement

**Type:** Computed (the engine). No model.

If distillation produced a typed **`research_forward_assumption`** (Step 6d — a guidance figure, a signed-contract value, a commodity / ASP turn, each with value, units, as-of date, source, and confidence), the **engine — not the model —** recomputes the affected scenario target with it as an explicit, **logged** assumption. A malformed, unsourced, or non-numeric claim is **rejected** (it cannot move a target), and a fact that **conflicts** with a structured feed is resolved by the assumption's declared conflict-handling rule. So the number stays engine-computed while the forward view reflects what research learned. The backward-looking sub-scores are untouched; absent a valid assumption, the Step-6b targets stand (see [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) Step 5).

### Step 6f: Interpretation and Grading

**Type:** Local-model call (122B, thinking). The verdict-writing call.

The reasoner interprets the computed analysis and the distilled research into the holding verdict: it sets the grade and action, selects and justifies the base-case target, and explains the reasoning — but reads every number from the engine rather than inventing it.

#### Local-model call — Interpretation & grading (Qwen3.5-122B, thinking)

**Model.** The resident 122B in thinking mode; schema-constrained output.

**Prompt — input.** The engine's computed analysis (sub-scores, the refined scenario targets with exposed methodology, the narrative-vs-reality and forensic reads, the positioning/options signal as context); the distilled research findings; the house view and investor profile; the prior run's verdict; and the position delta. The **absolute street opinions** (consensus target level, current rating consensus, FMP's ratings snapshot) are presented as *evidence to weigh against the engine's own read*, not as numbers to adopt.

**Returns.** The schema-validated **holding verdict** — composite grade (A–F) over the four sub-scores; action on the fixed ladder (sell all → trim → hold → add → add aggressively) with a target weight + share/dollar adjustment; conviction (shaped by narrative-vs-reality; abstaining as `insufficient-evidence` below the evidence floor); short/mid/long horizon outlook; the selected EoM/EoY targets; a financial-health read; and the **what-changed audit** — every moved value (grade, sub-score, action, conviction, target, horizon) shown old → new with its cause **attributed** to an *external* change (market data / company information / research-narrative, each tied to the engine's input delta or a research finding) or to a **labeled self-correction** where the inputs did not move. The input delta is the deterministic check that a move called "external" maps to an input that actually moved. Full schema in [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict).

### Step 6g: Continuity Check and Checkpoint

**Type:** Computed (app layer). No model.

This step is an **app-layer validator**, not just a recorder. Every move the 6f audit labels **external** must resolve to a concrete entry in the engine's input delta, a source-backed research finding, or the logged `research_forward_assumption`; an attribution that resolves to nothing is **downgraded to self-correction** (or fails schema validation), so the model cannot launder a no-new-facts swing as "the market changed." The validated **what-changed audit** is recorded against the prior run; output stays firm and does not swing run-to-run absent hard supporting data ([thesis-continuity.md](thesis-continuity.md)). The completed holding is **checkpointed** so the run can resume here.

## Step 7: Portfolio Roll-Up

**Type:** Local-model call (122B) — synthesis over the completed per-holding pass.

#### Local-model call — Portfolio roll-up (Qwen3.5-122B)

**Model.** The resident 122B reasoner; schema-constrained output.

**Prompt — input.** All per-holding verdicts; the engine-computed portfolio aggregates (concentration and sector/factor exposure, **with fund exposure folded in at the sector/country level** — single-name look-through is off-plan with `etf/holdings`, so direct-plus-fund overlap aggregates at the exposure level unless SEC N-PORT supplies constituents); the **exited** names from the Step-4 diff; the house view; and the investor profile.

**Returns.** The schema-validated **portfolio-level view** — concentration and sector/factor exposure, overall risk posture, a cash/deployment stance, and positions closed since the last run — read against both the house view and the profile ([portfolio-analysis.md §Portfolio roll-up](portfolio-analysis.md#portfolio-roll-up)).

## Step 8: Persist Run and Audit, with Memory Embeddings

**Type:** Computed (persist the verdicts, roll-up, holdings snapshot, and audit record) + Local-model call (embeddings for continuity).

The application persists the run: each holding's verdict, the roll-up, the **holdings snapshot it ran against** (the next run diffs against this), and an **audit record** that makes the run traceable — the report(s) and sources used with retrieval timestamps, the distilled findings, the computed metrics and the derived reads, the **input delta and the what-changed attribution**, the price-target methodology including its discount-rate assumption and any research-sourced forward assumption (with source), the model ids and quantizations, the prompt/schema version, and degraded-input flags. Retention keeps the last N runs ([storage.md](storage.md)).

#### Local-model call — Run-result embeddings (Qwen3-Embedding-4B, fixed)

**Model.** The fixed local embedder — vectorization only.

**Prompt (input text).** Each holding's verdict summary, embedded individually.

**Returns.** Vectors stored in the **Portfolio Analysis** memory partition (the job namespace), so a later run of this job can semantically recall the relevant prior analysis for a holding ([local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)). Best-effort: a failed embedding costs the memory row, never the persisted run.

## Step 9: Generate Portfolio Page and Update UI

**Type:** Computed (frontend). No model.

The **Portfolio page** renders each holding's verdict — grade and sub-scores, conviction, action, outlook, targets, financials, and the what-changed line — alongside the portfolio roll-up, and shows not-rated positions with their reason ([interface.md](interface.md)). While the job ran, the run tracker replaced the page (latest-run-only); on completion the page shows the persisted results. A **run is never a report**: a row appears only on persisted success, so a cancel or failure removes nothing ([run-tracking.md](run-tracking.md)).
