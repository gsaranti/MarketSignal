# Portfolio Analysis Workflow

Portfolio Analysis is one of the two local-suite jobs ([local-models.md](local-models.md)).
This document specifies its end-to-end control flow; the feature's design rationale — the verdict schema, the engine's three layers, the evidence floor, the roll-up — lives in [portfolio-analysis.md](portfolio-analysis.md).

The Portfolio Analysis job:
- pulls the user's Charles Schwab holdings (and live option chains)
- classifies each position by asset type and diffs it against the prior run
- computes a deterministic financial picture for every gradable holding
- researches each holding on the open web with a local reasoner
- grades each gradable holding (A–F) with price targets and a standalone action lean — the intrinsic verdict (an unpriceable fund class takes the typed `role_risk_only` read instead — [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility))
- reconciles those into final per-holding actions and a whole-book roll-up against the Market Signal house view

It runs **on demand only**, from a single **Run analysis** trigger that pulls holdings and runs the analysis in one user action (a separate **Pull holdings** control fetches and displays positions without analyzing; the job never reads it — [portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering)), entirely on local models, with **no cost at the model layer**.
With a card **selection** active, Run analysis becomes a **selective re-analysis** over those holdings **plus Step 6's automatic safety inclusions** ([§Step 6](#step-6-per-holding-analysis-loop)); a third, **engine-only Quick check** control re-evaluates the standing ledgers between full runs without any model call ([§The quick check (engine-only)](#the-quick-check-engine-only)).
A **single global run slot** serializes it against the report and Trade Opportunities (only one runs at a time).
For job states, the global run slot, cancellation, and error handling, see [scheduling.md](scheduling.md) and [run-tracking.md](run-tracking.md); for the failure posture (per-holding checkpoint/resume, fail-soft research), see [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).

## How to read this workflow

Every step below is tagged with a **Type** so it is obvious what the step actually does:

- **Computed (app layer)** — deterministic Rust logic, with no model and no external network: local SQLite and filesystem reads, the holdings diff, and the **financial-analysis engine** (every sub-score, target, and derived read).
- **API retrieval** — fetches from external sources: holdings and option chains from **Charles Schwab** (account-scoped, via OAuth — see [schwab-integration.md](schwab-integration.md)); company data from **FMP / SEC EDGAR / Stooq**; run-level macro and positioning from **FRED / CFTC**; and the **web tool** (SearXNG-primary, Tavily fallback) the orchestrator runs *on a model's behalf*.
  The full per-source endpoint surface, with each call's per-holding / per-fund / run-level cardinality, is in [data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface).
- **Local-model call** — invokes a model on the app-supervised **Ollama** daemon ([local-models.md §Serving runtime](local-models.md#serving-runtime)): the primary reasoner **`Qwen3.5-122B-A10B`** in **thinking** mode (multi-step research and interpretation) or **non-thinking** mode (firm, directed consolidation), or the fixed **`Qwen3-Embedding-4B`** embedder (vectorization only).
  Every generative call is **schema-constrained** via Ollama's native `format` parameter — the model picks values, never structure.
  **Mode caveat:** until Ollama bug #14645 is verified fixed on the pinned version, every `format`-carrying call keeps thinking **enabled** — the non-thinking designations below are the design mode, not the shipped wiring (the rule is canonical in [local-model-operations.md](local-model-operations.md) §Structured output × thinking).

Two load-bearing architectural rules frame the whole table, the same ones the report pipeline holds: **agents are pure stages, and the application layer owns all I/O** — a model stage consumes the structured input handed to it and emits a schema-validated result; when a research stage needs the web it *requests* a tool call and the orchestrator performs the fetch.
And **the engine computes every number** — every *analytical* value: metrics, sub-scores, tiers, scenario targets ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)) — the model interprets computed values and never invents one; the Step-7b target-weight choice is the scoped exception, an **engine-bounded model decision** (not an analytical value) validated by the deterministic joint-feasibility check.
For each model stage, the **Local-model call** block lists what the prompt includes and what the model returns.
Per-step progress, per-request rows, and token/reasoning output stream to the run tracker over the shared `progress` seam ([run-tracking.md](run-tracking.md)), exactly as a report run does.

| Step | Stage | Type | Model |
|---|---|---|---|
| 1 | Job start & gate | Computed | — |
| 2 | Load holdings & fetch option chains | API retrieval (Schwab) + Computed | — |
| 3 | Classify asset eligibility | Computed | — |
| 4 | Holdings change diff | Computed | — |
| 5 | Load shared context (house view, profile, run-level FRED/CFTC/Stooq) | Computed (local read) + API retrieval | — |
| 6 | **Per-holding analysis loop** (per eligible holding, checkpointed) | mixed — see 6a–6g | 122B + embedder |
| 6a | Dossier assembly | API retrieval + Local-model (embedding) + Computed | Qwen3-Embedding-4B · fixed |
| 6b | Deterministic financial analysis | Computed (engine) | — |
| 6c | Bounded web research (+ conditional technology-event topic) | Local-model call (thinking) + API retrieval (web tool), looped | Qwen3.5-122B · thinking |
| 6d | Distillation (single, or hierarchical: tier-1 per topic-tree → reduce) | Local-model call(s) (non-thinking) | Qwen3.5-122B · non-thinking (35B optional) |
| 6e | Deterministic target refinement | Computed (engine) | — |
| 6f | Interpretation & grading — intrinsic verdict + ledger rewrite | Local-model call (thinking) | Qwen3.5-122B · thinking |
| 6g | Continuity check, ledger validation & checkpoint | Computed | — |
| 7a | Whole-book aggregates & sizing-spine inputs | Computed (engine) + API retrieval (label-time Stooq bars + dividends) | — |
| 7b | Portfolio construction — final actions + roll-up | Local-model call (thinking) | Qwen3.5-122B · thinking |
| 8 | Persist run & audit + memory embeddings | Computed (persist) + Local-model (embedding) | Qwen3-Embedding-4B · fixed |
| 9 | Render Portfolio page & update UI | Computed (frontend) | — |

## Step 1: Job Start and Gate

**Type:** Computed (app layer) — the local-suite execution gate.
No model and no external API (credential and daemon *presence/reachability* are checked, not analysis).

The job will not start unless four preconditions hold:
- the **single global run slot** is free (no report or other local job is running — see [scheduling.md §Concurrent Job Protection](scheduling.md#concurrent-job-protection));
- the **local-model daemon is reachable and the configured roster is present** (the 122B reasoner + the embedder) — health-checked at the Ollama endpoint ([local-models.md §Serving runtime](local-models.md#serving-runtime));
- a **connected Schwab account** with a valid (≤7-day) refresh token ([schwab-integration.md §A connected Schwab account is required](schwab-integration.md#a-connected-schwab-account-is-required));
- the **shared FMP and FRED credentials are present** ([configuration.md §External Data Provider Credentials](configuration.md#external-data-provider-credentials)) — the per-holding fundamentals surface (FMP) and the run-level rate anchors (FRED `DGS10` / `DGS2`) are load-bearing engine inputs, so a missing key blocks at the gate rather than failing hours into a run; the check is presence-only (no live probe), surfaced through the **existing missing-provider-credentials warning category** — no new category — while **Tavily deliberately does not gate** the local suite (there it is an optional research fallback — [web-research.md §Tavily fallback](web-research.md#tavily-fallback)).
  **Settled (engine update pending):** the shipped gate (`check_local_configuration`) merges only the local-model and Schwab presence checks, and the as-built single-equity slice treats a missing FMP key as fail-soft — this credential precondition lands with the full Portfolio slice.

This gate is **independent of the cloud-report gate** — a machine with no OpenAI/Anthropic keys can still run the local suite.
Missing **configuration** (the Ollama endpoint or a roster id unset, Schwab not connected / refresh token lapsed, or the FMP / FRED credential missing) is a presence check that locks the local-suite Run buttons and shows a persistent warning *before* this step is reached — **local models not configured**, **Schwab connection**, and the shared **missing provider credentials**, one per category, no duplicates (see [interface.md §Connection status](interface.md#connection-status-local-suite)).
A live **local-model connectivity** failure caught here at the run-gate (daemon unreachable, a rostered model not pulled) blocks the attempt **inline**, not as a persistent warning; Schwab *API* reachability is **not** tested at this step — there is no external API call here, so a Schwab outage surfaces at the Step-2 holdings fetch, not the run-gate.
Manual-import holdings do **not** satisfy the Schwab gate.

## Step 2: Load Holdings and Fetch Option Chains

**Type:** API retrieval (Schwab).
No model.

Holdings are **fetched fresh at job start** — the Run-analysis trigger pulls them as its first retrieval, never reusing a standalone **Pull holdings** snapshot (that control is view-only and invisible to the job; the diff baseline below is likewise always the prior *run's* snapshot, [portfolio-analysis.md §Holdings change tracking](portfolio-analysis.md#holdings-change-tracking)); the run's snapshot persists with the run, so the portfolio stays viewable without re-fetching.
A **resumed** run performs no pull at all — it reopens its interrupted run's pinned snapshot ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).
Each position carries instrument identity (symbol, CUSIP, asset type), quantity, cost basis (the **signed account-currency total** the app derives from Schwab's per-unit `averagePrice` — [schwab-integration.md §What is pulled](schwab-integration.md#what-is-pulled)), market value, and P/L, from `GET /trader/v1/accounts/{accountHash}?fields=positions` (Schwab identifies accounts by a hashed number; the app resolves plaintext→hash first).
**Manual-import** positions (CSV/paste) populate the same holdings model as a supplement.
Snapshot assembly then runs the **holdings-normalization step** — same-symbol rows across granted accounts and manual supplements net into one book-level position per symbol ([schwab-integration.md §What is pulled](schwab-integration.md#what-is-pulled)) — and every later step consumes only the normalized book-level rows.

**Option chains are fetched fresh at job start** alongside the holdings, from `GET /marketdata/v1/chains` — per-contract volume, open interest, IV, and greeks — bounded by expiration and strike range, carrying an as-of timestamp and **rejected if stale** (mirroring the report's COT freshness guard).
Any no-value chain condition — no listed options, stale, malformed, or a per-symbol fetch failure — degrades to the same typed options-signal gap, never a job failure ([schwab-integration.md §Failure posture](schwab-integration.md#failure-posture)).
The deterministic put/call + IV/skew signal these chains feed is computed later, per holding, in Step 6b.

## Step 3: Classify Asset Eligibility

**Type:** Computed (app layer).
No model.

Each position is classified before analysis (see [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)):
- **Stocks** — the full per-holding pipeline (Step 6, equity path), behind the **loop-time listing-resolution guard** at Step 6a (this step has only Schwab instrument identity — the same reason the fund strategy classification defers): a symbol with no canonical FMP resolution or a non-US primary listing re-classifies to **not-rated (unsupported listing)**; a resolved-but-conflicting identity abstains at the 6b floor check ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
- **ETFs / funds** — the **reduced** pipeline (Step 6, fund path): no single-company financials; graded on strategy / **exposure** (sector / country weightings — constituent look-through is off-plan), valuation, and the house view.
  The further **strategy classification** (asset class from `etf/info`) is a **loop-time routing decision, not made here** — this step is computed-only and `etf/info` is not retrieved until Step 6a — so each fund is classified and routed at 6a/6b once its metadata is in hand: equity funds the exposure-valuation path (US-exposure-guarded: below ~70% US by country weightings the composite is not an honest read, so the fund is unpriceable); bond / commodity funds a further-reduced path with valuation recorded as a gap; leveraged / inverse vehicles carry the deterministic structurally-path-dependent flag; a CEF adds the NAV read.
  Every unpriceable class returns the typed **`role_risk_only`** intrinsic verdict — no letter, no targets; the portfolio action machinery still applies ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
- **Options, fixed income, cash, unsupported types — and net-short equities** — marked **not rated**, with a reason, excluded from grading (a short's signed exposure still feeds the roll-up, and a long↔short reversal force-includes in a selective run — [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
  Cash and buying power still feed the investor profile and the roll-up.

The eligibility decision is explicit and shown in the UI; a not-rated position never receives a fabricated grade.

## Step 4: Holdings Change Diff

**Type:** Computed (app layer) — a deterministic diff before any model stage.
No model.

The current holdings are diffed against the **prior run's persisted snapshot** (see [portfolio-analysis.md §Holdings change tracking](portfolio-analysis.md#holdings-change-tracking)).
Every current position is tagged **by quantity** — by position size (absolute for a same-side move, the signed swing on a sign flip), so a short and a net long↔short reversal read correctly, with cost basis as corroborating context rather than a second axis — as **new / increased / decreased / unchanged**; a symbol present last run but absent now is **exited** (no per-holding verdict — there is nothing left to grade — but surfaced in the Step-7 roll-up as closed-since-last-run).
Each holding's delta rides into its dossier so the verdict reasons over what the user actually did.
The diff is the application's, not the model's.

## Step 5: Load Shared Context

**Type:** Computed (local read — house view, investor profile) + API retrieval (run-level FRED / CFTC / Stooq / CBOE / FMP gold quote).
No model.

Three things are loaded **once per run and shared across every holding**, not re-requested per symbol:
- the **Market Signal house view** — the latest report's Thesis, Investment Strategy, and Forward Outlook sections plus recent report summaries (`thesis_stance`, `forward_outlook_themes`, `key_risks`), loaded **deterministically** from the report store (retrieve-don't-dump — never by vector-searching the report's memory; see [local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)).
  The report's **creation date** rides into the dossier so every downstream stage knows how old the thesis is, and a **freshness window applies**: if the latest report is older than **one week** (a pinned default), the house view is **omitted and recorded as a gap** rather than fed as current — a month-old thesis is not today's, and the data-honesty stance treats a stale input as absent, not current (the same posture the report takes on a stale data series).
  The holding is still graded on its fundamentals, research, and profile; it simply carries no house-view anchor that run;
- the **investor profile** (risk tolerance, horizon, objective, tax sensitivity, available cash / buying power — see [configuration.md](configuration.md)) — supplied to **Step 7b construction only**; the intrinsic loop never sees it ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict));
- run-level market context — the **risk-free rates** (FRED `DGS10` / `DGS2`: `DGS10` anchors the engine's scenario-target function — the v2 rate-anchored multiple — and `DGS2` the capital-efficiency hurdle, the suite's short-end anchor mirroring Trade Opportunities' entry-threshold anchor — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable); a rate retrieval still failing after the shared bounded retries **hard-fails the run here, before any per-holding work** — the canonical rate-anchor rule, [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)), **cyclical commodity prices** for commodity-linked holdings (FRED daily energy plus the suite-shared monthly IMF metals — [data-sources.md §Trade Opportunities — endpoint surface](data-sources.md#trade-opportunities--endpoint-surface) — and gold via FMP `quote` `GCUSD`), the **CBOE daily put/call statistics** (an optional, fail-soft **venue-level options-sentiment backdrop** — broad-market context, never a per-name signal — [data-sources.md §CBOE](data-sources.md#cboe)), the **Stooq sector / market benchmark series** the input delta's technology-event pre-flag and the outcome-learning labels read, and **CFTC Commitments-of-Traders positioning** on the bellwether contracts, which a commodity / macro **fund** holding maps onto for an underlying-positioning read.

## Step 6: Per-Holding Analysis Loop

Each **gradable** holding (stock or fund, from Step 3) is processed through the chain below.
Holdings are independent, so the loop **checkpoints per holding** — each holding's completed stages persist, so a cancellation or a single model failure resumes the unfinished holdings rather than restarting the (potentially hours-long) run, and recent research is cached within a freshness window — resume is its own entry path, reopening the run's pinned snapshot and versions, never a fresh pull ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).
The resident **122B reasoner fills every model role in this loop** by switching mode (thinking ↔ non-thinking), so moving a holding across its research passes (thinking), distillation (non-thinking — single or hierarchical, Step 6d), and interpretation (thinking) pays no model-swap cost ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).
A **fund** holding runs the reduced engine path (Step 6b) and a **fund-flavored research agenda** (Step 6c); the loop's structure — research, distillation, interpretation, continuity — is otherwise identical.
Sub-steps 6a–6g are the [portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline) six stages, with the target refinement (6e) surfaced as its own deterministic phase.

In a **selective re-analysis** (Run analysis with a selection), the **initial work-list** is the selected holdings plus any holding **new since the last run** (nothing to carry).
Before the loop, the quick-check evaluation runs over the remaining tail and **expands that work-list** with every holding it flags, every holding whose sweep result is **`unknown`** (a required signal family's retrieval failed — the degraded-sweep rule, [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)), every holding whose **position side reversed**, every holding carrying an unexamined evidence event, and every over-age exit-family carry.
Holdings left outside the resulting work-list carry their prior intrinsic verdict and ledger forward, **vintage-stamped**, into Step 7.
Steps 1–5 run whole-book regardless (the pull, eligibility, diff, and shared context are cheap), so the diff baseline and snapshot semantics are unchanged.
These force-includes are the first of the three safety rules that make mixed vintages analytically safe, specified once in [portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering); **at Step 7b**, validation enforces the carried-action transition rule (toward *hold* only, save the aggregate-validated context trim) and the per-family over-age resolution (add-family rule-demoted to *hold*, stamped `action_source: rule-demoted`; exit-family force-included above).
Within the loop, research effort is **graduated**: a holding whose research is younger than the ~4-week freshness window with nothing moved — no position change, attention condition, event pre-flag, or cache-invalidating **new evidence event** — **reuses its cached distilled findings** — 6c–6d skip, 6b and 6e–6g still run fresh (6e where the verdict branch carries targets) — with the reuse conditions and window in [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable) and every reuse decision logged to the audit record.

### Step 6a: Dossier Assembly

**Type:** API retrieval (FMP / SEC EDGAR / Stooq / FINRA) + Local-model call (embedding, for continuity retrieval) + Computed (assemble the packet).

The application builds the holding's evidence packet deterministically: the position + its Step-4 delta; any **same-underlying option positions** from the Step-2 pull (deterministic symbol link, carried as a **typed overlay** — direction / quantity / strike / expiry / delta / coverage ratio, classified covered-call / protective-put / collar / other); the symbol-scoped **`news/stock`** headlines as research-loop **seeds** (leads, never evidence — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)); the **equity** per-symbol surface (FMP fundamentals + revenue segments + analyst/revision signals + FINRA short interest, joined with SEC EDGAR as the authoritative cross-check; **13F institutional, earnings-call transcripts, and per-symbol M&A are off-plan** → SEC EDGAR / the web-research loop / `mergers-acquisitions-latest`+8-K — [data-sources.md §FMP — current paid-plan tier audit](data-sources.md#fmp--current-paid-plan-tier-audit)) or, for a fund, the **reduced ETF surface** (`etf/info` + sector/country weightings, plus the **sector-P/E surface** the 6b exposure-priced valuation reads — `sector-pe-snapshot` / `historical-sector-pe`, fetched **on first need and memoized across funds** — the snapshot once per exchange, the historical series per sector × exchange as each fund's retrieved weightings introduce sectors (so a later fund's new sector still gets its trailing history; the sector set can't precede this step — [data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface)); constituent `etf/holdings` and mutual-fund `funds/disclosure*` off-plan); price history (Stooq) and a live quote (FMP `quote`); the prior run's verdict **and thesis ledger** for this holding ([portfolio-analysis.md §The position thesis ledger](portfolio-analysis.md#the-position-thesis-ledger)); the Step-5 shared context; and vector-retrieved continuity from **this job's own prior runs** for this holding.
The full input list and every endpoint is in [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) and [data-sources.md](data-sources.md#portfolio-analysis--endpoint-surface).

#### Local-model call — Vector continuity retrieval (Qwen3-Embedding-4B, fixed)

**Model.**
The fixed local embedder — vectorization only, no reasoning.
Shares the `Embedder` trait the report pipeline defines; only the vector space differs.

**Prompt (input text).**
A query string built deterministically from the holding (symbol, sector/industry, and the prior verdict's themes), byte-capped before the call.

**Returns.**
A vector validated against the shared embedding-response contract ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)); the application runs a brute-force cosine search scoped to the **Portfolio Analysis** memory partition (the job namespace — never the report's or Trade Opportunities' — see [local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)) and carries the relevant prior analysis into the dossier.
An invalid or failed response **skips semantic recall for this query fail-soft** (a degraded-input flag; the deterministically loaded prior verdict and ledger are unaffected).

### Step 6b: Deterministic Financial Analysis

**Type:** Computed (the financial-analysis engine, shared with Trade Opportunities).
No model.

The engine computes the holding's quantitative picture in **three layers** — **(a)** the grade core → for a stock, the quality / valuation / risk sub-scores the letter rolls up from; for a priced equity fund, real valuation / risk plus the neutral-imputed absent quality axis defined by the fund-grade contract ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)) (momentum computed alongside, outside the letter — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)) — and the scenario price targets (the **v2 rate-anchored scenario-target function** off the run-level `DGS10` — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable); as-built the v1 drift, pending that engine update); **(b)** a conviction layer → the narrative-vs-reality ratio (with its thin-coverage fallback), the **forensic flags**, the **momentum / market-setup read**, and the **implied-expectations read** (the shared Step-5c primitive), all kept *out* of the letter; and **(c)** positioning context (insider / congressional / **FINRA short interest** / the Step-2 **options-activity signal**; FMP 13F off-plan → EDGAR/omit), held out of the sub-scores until the outcome-learning scorecard calibrates it ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)).
This stage also **assigns and persists the holding's risk tier**, per branch — the deterministic assignment rule of [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable) (a `role_risk_only` holding carries none) — before anything downstream consumes it.
The forward targets are a **provisional scenario menu** at this point; from them the engine also derives a **capital-efficiency / dead-money read** (total-return basis, DGS2-anchored, scaled by the tier just assigned, three-state — only *fails* is dead money; [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)), kept out of the sub-scores like layers (b)/(c) and fed to the Step-7 action-sizing spine.
The three-layer design is in [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) Step 2.
For a **fund**, this step runs the reduced computation instead — routed by the **strategy classification made at loop time** from the 6a `etf/info` pull (Step 3's eligibility used only Schwab instrument identity) — the reduced fund computation; an **unpriceable class** computes what it honestly can and returns the typed **`role_risk_only`** verdict ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
The engine also computes a deterministic **input delta** — this run's metrics, sub-scores, positioning, and price against the prior run's stored values (from the audit record), together with the Step-4 position delta and the Step-5 house-view age / change — the evidence the continuity audit (6f / 6g) attributes verdict moves to.
As part of the input delta, the engine also evaluates the prior thesis ledger's **quantitative** falsifiers and triggers — which conditions crossed this run, under their **persistence semantics** ([portfolio-analysis.md §The position thesis ledger](portfolio-analysis.md#the-position-thesis-ledger)) — for interpretation to read, and sets the **technology-event pre-flag** ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
This stage ends with the **evidence-floor check** — deterministic, over the floor-bearing inputs now all in hand: a below-floor holding **exits the loop here** with the typed **`insufficient-evidence`** disposition and named gap reasons, **checkpointed as completed** — Steps 6c–6f never run for it, its standing ledger and any attention flag are retained, and the full exit-state semantics (roll-up contribution, no per-holding action, no new outcome episode) are specified once in [portfolio-analysis.md §Evidence floor](portfolio-analysis.md#evidence-floor).

### Step 6c: Bounded Web Research

**Type:** Local-model call (122B, thinking) + API retrieval (the web tool), **looped**.
This is the only stage that loops.

The orchestrator assembles the holding's **agenda** deterministically — the reasoner works it, never authors it ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)) — (competitive position, recent results/estimate revisions, catalysts/risks, **management quality & capital allocation**, market narrative & sentiment, forward opportunity & thematic fit, **plus — conditionally — a technology-event impact assessment** that reads the actual technology and sizes the holding's real exposure into a typed `technology_read`; the conditional topic is triggered by the engine's Step-6b **event pre-flag**, a standing technology-class ledger falsifier, a qualifying `news/stock` seed, or an orchestrator-approved mid-loop follow-up proposal, and stays dormant otherwise — see [portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)) and works it **one topic at a time** — **each topic a separate isolated conversation and research loop** (a bounded multi-turn pass loop — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)), run over a **clean context** (the dossier facts plus that topic's own questions; no other topic's findings are fed in).
A **fund** holding's agenda swaps the company-centric topics for fund-flavored ones matched to its ledger's driver set — mandate / strategy and manager changes, expense and structure vs its category, the exposure's fit against the house view (and whether it is better held directly), and (CEF) the discount and distribution coverage; the technology-event topic is equity-only ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)).
The orchestrator — not the model — owns every request: per-topic depth ≤2 (≤3 passes/topic) and a **per-item fetch + wall-clock budget that binds first**, spent in topic-priority order, fail-soft on exhaustion.
Grounded by the deterministic financials so research fills the gaps the numbers don't. The full loop and its bounds are in [web-research.md](web-research.md).

#### Local-model call — Per-holding research (Qwen3.5-122B, thinking)

**Model.**
The resident 122B reasoner in thinking mode, requesting `web_search` / `web_fetch` tool calls the orchestrator executes (SearXNG-primary, Tavily fallback; SSRF-guarded; untrusted page text inserted as quoted evidence, never as instructions — see [web-research.md §Safety and provenance](web-research.md#safety-and-provenance)).
**One isolated conversation per agenda topic** (a bounded multi-turn pass loop — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)) — topics do not share a context.

**Prompt — input.**
The holding's dossier facts and **that topic's questions only** — a clean context per topic.
Within a pass the model reasons over the fetched, readability-extracted page text and an **append-only evidence ledger** (each extracted claim + its source URL / timestamp); there is **no in-loop re-distillation of findings** — the heavy consolidation is deferred to the Step-6d distillation, so research is never planned over already-distilled, lossy notes.

**Returns.**
The topic's **full findings response**, preserved whole (with its evidence-ledger entries), plus any **follow-up proposal** (a structured field the orchestrator decides whether to spend) and any **material forward fact** flagged for the Step-6e refinement.
Every topic's full response flows intact to distillation — nothing is summarized away in between — where it is consolidated in a single pass or, when the holding's research is large, **hierarchically** (a tier-1 distillation per topic-tree → a reduce, Step 6d).

### Step 6d: Distillation

**Type:** Local-model call(s) (122B, non-thinking; the optional 35B fast tier if resident) — a single pass, or **hierarchical** (tier-1 per topic-tree → a reduce) when the holding's research is large.
Consolidation, not new reasoning.

The reasoner in non-thinking mode consolidates the topics' **full findings responses** into the compact object the interpretation stage reads — a consolidation over the **complete** per-topic outputs, never a re-distillation of already-distilled notes — so interpretation reasons over a clean synthesis of full-context research ("forward only what's needed").
This is the *only* place research is condensed before interpretation.
It runs as **a single pass by default, or hierarchically** (tier-1 per topic-tree → a reduce) when a holding's research is large — the deterministically orchestrator-chosen primitive shared with Trade Opportunities ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)); there is no cross-lens contradiction check here, so the reduce is purely consolidation.

#### Local-model call(s) — Distillation (Qwen3.5-122B, non-thinking)

**Model.**
The same resident 122B in non-thinking mode by default (no model-swap cost); the fast 35B tier is a benchmark-gated option ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).

**Prompt — input.**
*Single pass:* the **full findings response from every topic** plus the append-only evidence ledger (claims + sources).
*Hierarchical:* each **tier-1** call gets one topic-tree's complete findings + that tree's ledger entries; the **reduce** gets the tier-1 structured outputs with their preserved citations (no cross-lens contradiction check here — the reduce is purely consolidation — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).

**Returns.**
A single schema-validated **distilled findings object** for interpretation, surfacing two typed forward fields (each structured, sourced, numeric — never loose prose):
- a **`research_forward_assumption`** — `{ fact type, numeric value, units, period / as-of date, source URL, confidence, the target assumption it affects, conflict_handling }` — the only thing that can reach the engine's target refinement (Step 6e).
  Its **`conflict_handling`** member is a typed two-value declaration, never free text — **`supplement`** (the charter case: the fact fills a forward value the structured feeds don't carry) or **`supersede`** (the fact contradicts a value a structured feed carries) — and it is a **claim the engine validates under the app-owned conflict policy of Step 6e, never a rule the model selects** (the conviction raise's model-proposes / app-validates spine); and
- when research validated a **countable, dated, third-party leading indicator the structured feeds did not carry** (a `research`-class signal — the **engine-unscored** signal a conviction *raise* must cite), a typed **`validated_leading_indicator`** — `{ metric name, value / level, direction (inflecting up / down), as-of date, source URL, confidence, the thesis-ledger key driver it confirms }`.
  It is **distinct from `research_forward_assumption`** (which moves targets, not conviction) and is the **only** field a Step-6f conviction raise may cite, so Step 6g can validate the raise deterministically rather than trusting prose.

Both typed fields exist only where their consumers do: a **`role_risk_only`** holding's distillation emits neither — no target for an assumption to move, no conviction for an indicator to raise — and is pure consolidation for interpretation.

### Step 6e: Deterministic Target Refinement

**Type:** Computed (the engine).
No model.

If distillation produced a typed **`research_forward_assumption`** (Step 6d — a guidance figure, a signed-contract value, a commodity / ASP turn, each with value, units, as-of date, source, confidence, and its declared `conflict_handling`), the **engine — not the model —** recomputes the affected scenario target with it as an explicit, **logged** assumption.
A malformed, unsourced, or non-numeric claim is **rejected** (it cannot move a target), and a fact that **conflicts** with a structured feed resolves under the **app-owned conflict policy — the model's declaration never selects or bypasses the rule**: a `supplement` may only fill a value the feeds don't carry (it never displaces a present feed value), and a `supersede` is honored **only when the engine verifies all of** — an as-of date strictly newer than the conflicting structured observation's, a fact type on the primary-source whitelist (issued company guidance, a signed contract, a filed figure — drafted), and metric, units, and period matching the feed field it contradicts.
A conflicting assumption failing any check is **rejected and logged with the failed condition — the structured value stands** (structured-wins is the default), and every resolution records the rule the engine matched in the run's audit record ([storage.md §Local Analysis Suite Storage](storage.md#local-analysis-suite-storage)).
So the number stays engine-computed while the forward view reflects what research learned.
Because the **capital-efficiency / dead-money read** derives from the base-case target, the engine **recomputes it here too** when refinement moves that target, so Steps 6f and 7 read a current flag rather than the provisional Step-6b one.
The backward-looking sub-scores are untouched; absent a valid assumption, the Step-6b targets stand (see [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) Step 5).
A **`role_risk_only`** holding **skips this step entirely** — its branch carries no scenario targets for an assumption to refine ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)).

### Step 6f: Interpretation and Grading

**Type:** Local-model call (122B, thinking).
The verdict-writing call.

The reasoner interprets the computed analysis and the distilled research into the holding's **intrinsic verdict**: it sets the grade, conviction, and horizon, selects and justifies the base-case target, and commits to a **standalone action lean** — but reads every number from the engine rather than inventing it, and rewrites the **thesis ledger** (revised thesis, re-weighted monitor, re-set falsifiers/triggers — reading the engine's quantitative crossings from 6b and judging the qualitative conditions from research).
For a **`role_risk_only`** holding the same call authors the union's other branch instead — the role / risk assessment and the rewritten fund ledger, none of the priced fields (see **Returns**).
The *final* portfolio action and target weight are set in Step 7b with the whole book in view; this stage produces the intrinsic read the construction stage reconciles.

#### Local-model call — Interpretation & grading (Qwen3.5-122B, thinking)

**Model.**
The resident 122B in thinking mode; schema-constrained output.

**Prompt — input.**
The engine's computed analysis (sub-scores, the refined scenario targets with exposed methodology, the narrative-vs-reality and forensic reads, the **implied-expectations range**, the positioning/options signal and any **same-underlying option overlay** as context); the distilled research findings; the house view (the investor profile is **deliberately absent** — the intrinsic verdict is profile-independent and the profile enters at Step 7b only — [portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)); the prior run's verdict **and thesis ledger** (with the engine's quantitative falsifier/trigger crossings from 6b, first-breach vs confirmed per their persistence semantics); and the position delta.
The **absolute street opinions** (consensus target level, current rating consensus, FMP's ratings snapshot) are presented as *evidence to weigh against the engine's own read*, not as numbers to adopt.

**Returns.**
The schema-validated **intrinsic verdict** — a **discriminated union of two branches** ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)).
The default **`priced`** branch: composite grade (A–F) over the branch-applicable grade contract — real quality / valuation / risk for a stock; real valuation / risk plus the neutral-imputed absent quality axis for a priced equity fund ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)) — with momentum riding as market-setup context outside the letter; conviction as a **decomposed triple** so the raise is app-checkable rather than a single opaque value: **`base_conviction`** (the read *before* any forward-leading-indicator raise — capped by narrative-vs-reality, lowered on thin evidence; a below-floor holding never reaches this call — it exited at Step 6b with the outer `insufficient-evidence` disposition — [portfolio-analysis.md §Evidence floor](portfolio-analysis.md#evidence-floor)), an **optional `conviction_raise`** (present only when the model asserts a raise — a `+1` band, the cited **`validated_leading_indicator`**, and a rationale), and **`final_conviction`** (the model's proposed end value = `base_conviction`, or one band higher on a valid raise).
The app — not the model — owns the final value: Step 6g re-derives `final_conviction` from `base_conviction` ± the validated raise, so a single returned number can never smuggle an unaudited lift (§Step 6g).
The **forward outlook** — short/mid/long horizon reads with the selected one-month / twelve-month targets (as-built named EoM/EoY — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)), the forward read kept distinct from the backward grade; a **standalone action lean** on the fixed ladder (sell all → trim → hold → add → add aggressively) *before* portfolio context (a **fails** hurdle read leans it toward exit on its own merits — [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)); and a financial-health read — plus the **rewritten thesis ledger** (thesis, bear/base/bull monitor, falsifiers, triggers — each **quantitative** falsifier and trigger stated **machine-evaluably**: the engine series, comparator, threshold, and persistence semantics, validated at Step 6g) and the **intrinsic half of the what-changed audit**: every moved intrinsic value (grade, sub-score, conviction, target, horizon, a re-weighted scenario, a tripped falsifier / fired trigger) shown old → new with its cause **attributed** to an *external* change (market data / company information / research-narrative, each tied to the engine's input delta or a research finding) or to a **labeled self-correction** where the inputs did not move.
The final action, target weight, and the **action half** of the what-changed audit are produced in Step 7b, where the whole-book context exists.
A **`role_risk_only`** holding returns the union's other branch — the role / exposure / risk / expense-drag read, structural flag, evidence gaps, the rewritten fund ledger, and its intrinsic what-changed half; none of the priced fields, per the canonical branch schema ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)); its action is set wholly at Step 7b from the reduced spine.
Full schema in [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict).

### Step 6g: Continuity Check and Checkpoint

**Type:** Computed (app layer).
No model.

This step is an **app-layer validator**, not just a recorder.
Every move the 6f audit labels **external** must resolve to a concrete entry in the engine's input delta, a source-backed research finding, or the logged `research_forward_assumption`; an attribution that resolves to nothing is **downgraded to self-correction** (or fails schema validation), so the model cannot launder a no-new-facts swing as "the market changed."
A conviction **raise** is held to a *stricter* form of this rule, and the app **computes `final_conviction` itself** rather than trusting the model's: a `conviction_raise` is honored only when it cites the typed **`validated_leading_indicator`** Step 6d emitted (a research-sourced countable / dated metric the structured feeds did not carry, confirming a ledger key driver ahead of the financials) and is **≤ one band** → `final_conviction = base_conviction + 1` (capped at the High ceiling — and, where a soft forensic or narrative cap has tripped, at the suite's shared **categorical Medium ceiling**, which binds *after* the raise: `final = min(base + validated raise, ceiling)`, the matched cap rule recorded in the audit — [trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable)); a raise that cites no valid such field — resolving only to price action, narrative, or a metric the engine already scored — is **dropped, so `final_conviction = base_conviction`** (the now-well-defined "un-raised level"), the dropped raise recorded in the audit (the anti-reflexivity / no-double-count guard — [portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)).
The same rule validates the **thesis-ledger rewrite**: every falsifier marked tripped and every trigger marked fired must map to a 6b quantitative crossing or a source-backed finding, or it is rejected.
Every **newly written or rewritten quantitative condition is additionally executability-validated**: it must resolve to a series the engine actually computes and refreshes — the suite's shared resolution contract ([trade-opportunities-workflow.md §Step 3c](trade-opportunities-workflow.md#step-3c-carried-forward-watchlist-re-check)), with metric, comparator, threshold, units, and persistence semantics all well-formed; one that doesn't resolve is **downgraded to qualitative with a logged reason, never dropped**, and retains no machine evaluation state — so the quick check's "every quantitative condition" promise is total over conditions it can actually evaluate ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
The **app-owns-the-number rule covers the verdict's deterministic fields too** ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)): the returned **letter grade must equal the engine's own composite letter**, and the returned one- / twelve-month **targets must be members of the engine's 6b / 6e scenario set** — a mismatched echo **rejects the response** rather than persisting a model-written number.
The validated **intrinsic what-changed audit** and the rewritten ledger are recorded against the prior run — the **action half** of the audit is validated later, in Step 7b, where the portfolio context it cites exists; output stays firm and does not swing run-to-run absent hard supporting data ([thesis-continuity.md](thesis-continuity.md)).
On a successful checkpoint, this step also clears any persisted quick-check attention flag for the holding and stamps the triggering condition's acknowledging observation; the 6b evaluation this same pass consumed is continuity input, never a fresh persisted flag.
The completed holding is **checkpointed** so the run can resume here.

## Step 7: Portfolio Roll-Up and Construction

The per-holding loop produced each holding's **intrinsic** verdict and standalone action lean (none for a `role_risk_only` verdict — its action arises wholly here); this step takes the whole book in view — the two things the loop structurally cannot do, because it decides each holding before any other's verdict exists ([portfolio-analysis.md §Portfolio roll-up and construction](portfolio-analysis.md#portfolio-roll-up-and-construction)).
It runs in two parts.

### Step 7a: Whole-Book Aggregates and Sizing-Spine Inputs

**Type:** Computed (the engine) + API retrieval (a label-time **Stooq daily-bar refresh** and **`dividends`** re-pull per maturing outcome episode).
No model.

The engine computes the deterministic whole-book picture: concentration and sector / factor exposure (**fund exposure folded in at the sector / country level** — single-name look-through is off-plan with `etf/holdings`, so direct-plus-fund overlap aggregates at the exposure level unless SEC N-PORT supplies constituents), correlation / overlap clusters, the cash / buying-power position, and **the risk / exposure contribution of any material not-rated positions** (market value + signed notional — the payload-derivable inputs; fixed-income duration / credit and a standalone option's delta ride as **typed gaps**, no on-plan source carrying them, the held-underlier overlay delta from its own chain the exception — [portfolio-analysis.md §Portfolio roll-up and construction](portfolio-analysis.md#portfolio-roll-up-and-construction) — graded nowhere but real exposure); and, per holding, the **action-sizing spine inputs** — existing weight, concentration headroom against the profile's limits, overlap contribution, the upside/downside the targets imply against price, the **capital-efficiency / dead-money** read (total-return basis), any **same-underlying option overlay**, the position's **unrealized P/L** (a harvestable loss or a taxable gain, by sign), the risk tier (assigned and persisted at Step 6b — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)), and tax.
These **bound the feasible action set** the next part chooses within — the grade tests in those bounding rules reading the **ex-momentum composite** ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict), [§Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
A **`role_risk_only`** holding supplies the **reduced spine**, its feasible set **{sell all, trim, hold}** ([portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action)).
This step also runs the deterministic half of the **outcome-learning pass**: the engine tags each active decision episode's **`observed_net_alignment`** from the Step-4 diff and computes any newly **matured window labels** after refreshing the episode symbol's Stooq bars through the window end — including symbols no longer held — and re-pulling its `dividends` history for the total-return leg.
A failed Stooq refresh uses cached bars only when they cover the full window; otherwise the label remains pending with a coverage gap, bounded by the shared price-coverage grace — past it the leg closes as the typed `price-coverage-unscorable` label ([storage.md §Local Analysis Suite Storage](storage.md#local-analysis-suite-storage)).
It then derives the scorecard reads — label mechanics, bases, and cohort layers all per the canonical contract in [portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration); this run's own episodes are appended-or-extended at Step 8, once 7b has set the final actions.

### Step 7b: Portfolio Construction

**Type:** Local-model call (122B, thinking) — synthesis over the completed per-holding pass and the Step-7a aggregates.

#### Local-model call — Portfolio construction (Qwen3.5-122B, thinking)

**Model.**
The resident 122B reasoner in thinking mode — the whole-book reconciliation is multi-step reasoning, like interpretation (6f); schema-constrained output.

**Prompt — input.**
All per-holding **intrinsic** verdicts and standalone action leans (a `role_risk_only` verdict carries no lean — its action arises wholly from the reduced spine); the Step-7a whole-book aggregates and per-holding sizing-spine inputs (with the feasible action set the engine bounded); the **exited** names from the Step-4 diff; the house view; and the investor profile.

**Returns.**
Each holding's **final action** (fixed ladder) and **target-weight range / share-dollar adjustment**, reconciled from its intrinsic lean against the aggregates and bounded by the sizing spine — plus the **action half of the what-changed audit** (a changed action / weight attributed to a moved intrinsic verdict *or* a moved portfolio context) and the schema-validated **portfolio-level view**: concentration and sector/factor exposure, overall risk posture, a cash / deployment stance (what to trim to fund which adds — including **raising cash from a dead-money loser**, where the *possible* tax benefit of realizing the loss and the redeployment optionality of the proceeds are valid supporting rationale, framed high-level with the user acting on the specifics — and, under the unconstrained-cash profile, the **external funding the plan implies** ([portfolio-analysis.md §Portfolio roll-up and construction](portfolio-analysis.md#portfolio-roll-up-and-construction))), and positions closed since the last run — read against both the house view and the profile.
The action-half attributions are **app-validated** the same way 6g validates the intrinsic half: a "became oversized" or "freed cash" claim must map to a real Step-7a aggregate, not a model assertion — and the proposed sizing passes the deterministic **joint-feasibility check**, whose solve, validations (including the selective-run carried-action and over-age rules), and single named-violation re-run are canonical in [portfolio-analysis.md §Portfolio roll-up and construction](portfolio-analysis.md#portfolio-roll-up-and-construction); a persisting infeasibility fails the run like any hard model failure.

## Step 8: Persist Run and Audit, with Memory Embeddings

**Type:** Computed (persist the verdicts, roll-up, holdings snapshot, and audit record) + Local-model call (embeddings for continuity).

The application persists the run: each holding's verdict, the per-holding **thesis ledger** (carried forward to seed the next run's continuity check), the roll-up, the **holdings snapshot it ran against** (the next run diffs against this), and the **run audit record** that makes the run traceable — sources and retrieval timestamps, distilled findings, computed metrics and derived reads, the conviction decomposition, the input delta and what-changed attribution, the price-target methodology, model ids and quantizations, prompt/schema version, degraded-input flags, and each holding's research-reuse decision — with the field set specified once in [storage.md §Local Analysis Suite Storage](storage.md#local-analysis-suite-storage).
It also **appends or extends** this run's **decision episodes** in the outcome-episode store and attaches any Step-6g-confirmed falsifier events to the episode that carried their condition — creation, extension, event, and vintage semantics per the canonical contract ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)) — and records the Step-7a matured labels and derived scorecard reads with the audit record, the matured reads additionally embedding as **durable learnings** in the job's memory partition ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)).
Retention keeps the last N runs; the episode store and its matured archive persist independently of that window ([storage.md](storage.md)).

#### Local-model call — Run-result embeddings (Qwen3-Embedding-4B, fixed)

**Model.**
The fixed local embedder — vectorization only.

**Prompt (input text).**
Each holding's verdict embedded individually — a text that captures the **standing thesis** (the ledger's thesis, key drivers, and scenario lean), the **intrinsic read** (grade and conviction — or, for a `role_risk_only` holding, its role read and structural flag), and the **final portfolio action**, so cross-run semantic recall surfaces the substance of prior analysis rather than a bare grade.

**Returns.**
Vectors stored in the **Portfolio Analysis** memory partition (the job namespace), so a later run of this job can semantically recall the relevant prior analysis for a holding ([local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)).
Best-effort: a failed embedding costs the memory row, never the persisted run — and an **invalid** vector (the shared validator — [local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)) is dropped and logged the same way, so a bad vector never enters durable memory.

## Step 9: Generate Portfolio Page and Update UI

**Type:** Computed (frontend).
No model.

The **Portfolio page** renders each holding's verdict — the **intrinsic** read (the **backward grade** and sub-scores paired *side by side* with the **forward outlook** — horizon reads and scenario targets — so a grade/outlook divergence is legible at a glance, plus conviction and the bear/base/bull monitor) beside the **portfolio action** (action, target weight, sizing rationale), with financials and the what-changed line — a **`role_risk_only`** verdict rendering its own explicit card branch (role, exposure, observable risk, expense drag, structural flag, evidence gaps — never empty priced placeholders) — plus each card's **selection control** (driving selective re-analysis), any amber **attention flag** raised by the quick check and retained only until that holding's next successful full pass, and its **analysis-vintage stamp** after a selective run — alongside the portfolio roll-up, and shows not-rated and insufficient-evidence positions with their reason ([interface.md](interface.md), [portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display)).
Above the holding cards a compact **sort bar** reorders the stack in place by overall value, dollar gain, percentage gain, or total cash invested — a display-only control over engine-computed position fields, defaulting to overall-value-descending ([portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display)).
The page also renders the latest standalone **Pull holdings** snapshot — the page body before any run exists; a stamped current-holdings section above the cards when fresher than the last run — with presence-only churn tags (*new · not in last analysis* / *no longer held*), never mutating or hiding the run-anchored cards ([portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display)).
While the job ran, the run tracker replaced the page (latest-run-only); on completion the page shows the persisted results.
A **run is never a report**: a row appears only on persisted success, so a cancel or failure removes nothing ([run-tracking.md](run-tracking.md)).

## The quick check (engine-only)

**Type:** Computed (engine) + API retrieval (the per-holding price refresh, the run-level `DGS2` and `DGS10` prints, and the per-asset-type evidence re-pulls — the full retrieval recipe is canonical in [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
**No model call, no web research, no Schwab call.**

A separate, cheap control that keeps the thesis ledgers live between full runs: it loads the **last run's holdings snapshot and ledgers** (no Schwab pull — it tests theses, not the book), refreshes prices, the `DGS2` and `DGS10` prints, and the per-asset-type evidence legs, evaluates every ledger's machine-checkable conditions under the shared persistence contract, re-derives the **total-return hurdle** (the v2 scenario multiples re-anchored on the fresh `DGS10` against the last full pass's stored percentiles and drivers — the canonical quick-path basis in [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)) and **scenario-band** reads on priced verdicts, and raises **attention flags** and quiet **evidence-event badges** — never rewriting any model-authored content.
The retrieval recipe, the four flag triggers, the evidence-event legs, and the evaluation-state carve-out are all specified once in [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only) (constants in [§Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).

It holds the **single global run slot** and streams per-holding rows to the run tracker like any job.
Because it makes **no model call**, it skips the daemon-connectivity check and runs even with the daemon configured-but-down — the same run-gate relaxation as ATO's Quick Audit ([trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture)); because it does **no web research**, it triggers no pre-run SearXNG notice.
The Schwab connection — and the shared FMP / FRED credential presence — remain presence preconditions, like everywhere in the suite, but no Schwab call is made.
A failed price refresh fail-softs to the last cached series with its as-of date — typed into the holding's per-family sweep state (`fresh_clear` / `flagged` / `unknown`), so a family the sweep couldn't check reads `unknown` and force-includes in a selective run rather than passing silently ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
