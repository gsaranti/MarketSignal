# Portfolio Analysis Workflow

Portfolio Analysis is one of the two local-suite jobs ([local-models.md](local-models.md)).
This document specifies its end-to-end control flow; the feature's design rationale — the verdict schema, the engine's three layers, the evidence floor, the roll-up — lives in [portfolio-analysis.md](portfolio-analysis.md).

The Portfolio Analysis job:
- pulls the user's Charles Schwab holdings (and live option chains)
- classifies each position by asset type and diffs it against the prior run
- computes a deterministic financial picture for every gradable holding
- researches each holding on the open web with a local reasoner (the research stage is **stubbed as-built** — Step 6c)
- grades each gradable holding (A–F) with price targets — the intrinsic verdict (an unpriceable fund class takes the typed `role_risk_only` read instead — [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility))
- decides each holding's portfolio action from that holding's own verdict plus the investor profile — **tunnel vision by design**: the job never compares holdings, and the whole-book reconciliation belongs to the future portfolio-planner job ([portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action))

It runs **on demand only**, from a single **Run analysis** trigger that pulls holdings and runs the analysis in one user action (a separate **Pull holdings** control fetches and displays positions without analyzing; the job never reads it — [portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering)), entirely on local models, with **no cost at the model layer**.
With a card **selection** active, Run analysis becomes a **selective re-analysis** over **strictly those holdings** — the rest carry forward, badged where the quick check flags a change ([§Step 6](#step-6-per-holding-analysis-loop)); a third, **engine-only Quick check** control re-evaluates the standing ledgers between full runs without any model call ([§The quick check (engine-only)](#the-quick-check-engine-only)).
A **single global run slot** serializes it against the report and Trade Opportunities (only one runs at a time).
For job states, the global run slot, cancellation, and error handling, see [scheduling.md](scheduling.md) and [run-tracking.md](run-tracking.md); for the failure posture (per-holding checkpoint/resume, fail-soft research), see [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).

## How to read this workflow

Every step below is tagged with a **Type** so it is obvious what the step actually does:

- **Computed (app layer)** — deterministic Rust logic, with no model and no external network: local SQLite and filesystem reads, the holdings diff, and the **financial-analysis engine** (every sub-score, target, and derived read).
- **API retrieval** — fetches from external sources: holdings and option chains from **Charles Schwab** (account-scoped, via OAuth — see [schwab-integration.md](schwab-integration.md)); company data from **FMP / SEC EDGAR**; run-level macro and positioning from **FRED / CFTC**; and the **web tool** (SearXNG-primary, Tavily fallback) the orchestrator runs *on a model's behalf*.
  The full per-source endpoint surface, with each call's per-holding / per-fund / run-level cardinality, is in [data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface).
- **Local-model call** — invokes a model on the app-supervised **Ollama** daemon ([local-models.md §Serving runtime](local-models.md#serving-runtime)): the primary reasoner **`Qwen3.5-122B-A10B`** in **thinking** mode (multi-step research and interpretation) or **non-thinking** mode (firm, directed consolidation), or the fixed **`Qwen3-Embedding-4B`** embedder (vectorization only).
  Every generative call is **schema-constrained** via Ollama's native `format` parameter — the model picks values, never structure (one as-built exception: the Step-6d condense runs unconstrained while research is stubbed, noted there).
  **Mode caveat:** Ollama bug #14645 is verified fixed on the pinned version, so a `format`-carrying call runs `think: false` directly where a non-thinking mode is designated below — but an Ollama version bump **re-locks** every `format`-carrying `think: false` call until the schema-integrity check passes on the new version, such calls riding thinking-enabled until it does (the rule is canonical in [local-model-operations.md](local-model-operations.md) §Structured output × thinking).

Two load-bearing architectural rules frame the whole table, the same ones the report pipeline holds: **agents are pure stages, and the application layer owns all I/O** — a model stage consumes the structured input handed to it and emits a schema-validated result; when a research stage needs the web it *requests* a tool call and the orchestrator performs the fetch.
And **the engine computes every deterministic number** — metrics, sub-scores, tiers, scenario targets ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)) — while, since `portfolio-v7`, the model additionally authors its **own arm** of the verdict's judgment fields (its sub-scores, target bands, conviction, outlook, and — since `portfolio-v9` — the action call's rung), unrestricted and clearly typed as model-authored: the engine's values stay the incorruptible baseline — the model arm's judgment values never alter or bind it (the boundary statement at [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)) — the model's are displayed and scored beside them, with engine evidence annotating departures, never enforcing them ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)).
For each model stage, the **Local-model call** block lists what the prompt includes and what the model returns.
Per-step progress, per-request rows, and token/reasoning output stream to the run tracker over the shared `progress` seam ([run-tracking.md](run-tracking.md)), exactly as a report run does.

| Step | Stage | Type | Model |
|---|---|---|---|
| 1 | Job start & gate | Computed | — |
| 2 | Load holdings (option chains fetch per holding at 6a) | API retrieval (Schwab) + Computed | — |
| 3 | Classify asset eligibility | Computed | — |
| 4 | Holdings change diff | Computed | — |
| 5 | Load shared context (house view, profile, run-level FRED rates; commodity/CFTC/benchmark context designed) | Computed (local read) + API retrieval (FRED) | — |
| 6 | **Per-holding analysis loop** (per eligible holding; per-holding checkpoint/resume designed) | mixed — see 6a–6g | 122B |
| 6a | Dossier assembly | API retrieval + Computed (vector-continuity embedding leg designed) | — |
| 6b | Deterministic financial analysis | Computed (engine) | — |
| 6c | Bounded web research (+ conditional technology-event topic) | Local-model call (thinking) + API retrieval (web tool), looped | Qwen3.5-122B · thinking |
| 6d | Distillation (single, or hierarchical: tier-1 per topic-tree → reduce) | Local-model call(s) (non-thinking) | Qwen3.5-122B · non-thinking (35B optional) |
| 6e | Deterministic target refinement | Computed (engine) | — |
| 6f | Interpretation & grading (intrinsic verdict + ledger rewrite), then the per-holding **action decision** (two calls; the investor profile enters at the action call only) | Local-model call (thinking) ×2 | Qwen3.5-122B · thinking |
| 6g | Continuity check, ledger validation & checkpoint | Computed | — |
| — | Outcome-learning pass (after the loop; persists with Step 7) | Computed (engine) + API retrieval (label-time dated-EOD bars + dividends) | — |
| 7 | Persist run & audit + memory embeddings (as-built matured learnings only; per-holding verdict embeddings designed) | Computed (persist) + Local-model (embedding) | Qwen3-Embedding-4B · fixed |
| 8 | Render Portfolio page & update UI | Computed (frontend) | — |

## Step 1: Job Start and Gate

**Type:** Computed (app layer) — the local-suite execution gate.
No model and no external API (credential and daemon *presence/reachability* are checked, not analysis).

The job will not start unless four preconditions hold:
- the **single global run slot** is free (no report or other local job is running — see [scheduling.md §Concurrent Job Protection](scheduling.md#concurrent-job-protection));
- the **local-model daemon is reachable and the configured roster is present** (the 122B reasoner + the embedder) — health-checked at the Ollama endpoint ([local-models.md §Serving runtime](local-models.md#serving-runtime));
- a **connected Schwab account** with a valid (≤7-day) refresh token ([schwab-integration.md §A connected Schwab account is required](schwab-integration.md#a-connected-schwab-account-is-required));
- the **shared FMP and FRED credentials are present** ([configuration.md §External Data Provider Credentials](configuration.md#external-data-provider-credentials)) — the per-holding fundamentals surface (FMP) and the run-level rate anchors (FRED `DGS10` / `DGS2`) are load-bearing engine inputs, so a missing key blocks at the gate rather than failing hours into a run; the check is presence-only (no live probe), surfaced through the **existing missing-provider-credentials warning category** — no new category — while **Tavily deliberately does not gate** the local suite (there it is an optional research fallback — [web-research.md §Tavily fallback](web-research.md#tavily-fallback)).
  (As-built with the fund slice: the shipped gate (`check_local_configuration`) carries the FMP / FRED presence check through the shared missing-provider-credentials category.)

This gate is **independent of the cloud-report gate** — a machine with no OpenAI/Anthropic keys can still run the local suite.
Missing **configuration** (the Ollama endpoint or a roster id unset, Schwab not connected / refresh token lapsed, or the FMP / FRED credential missing) is a presence check that locks the local-suite Run buttons and shows a persistent warning *before* this step is reached — **local models not configured**, **Schwab connection**, and the shared **missing provider credentials**, one per category, no duplicates (see [interface.md §Connection status](interface.md#connection-status-local-suite)).
A live **local-model connectivity** failure caught here at the run-gate (daemon unreachable, a rostered model not pulled) blocks the attempt **inline**, not as a persistent warning; Schwab *API* reachability is **not** tested at this step — there is no external API call here, so a Schwab outage surfaces at the Step-2 holdings fetch, not the run-gate.
Manual-import holdings do **not** satisfy the Schwab gate.

## Step 2: Load Holdings

**Type:** API retrieval (Schwab) + Computed (snapshot assembly — the holdings-normalization step).
No model.

Holdings are **fetched fresh at job start** — the Run-analysis trigger pulls them as its first retrieval, never reusing a standalone **Pull holdings** snapshot (that control is view-only and invisible to the job; the diff baseline below is likewise always the prior *run's* snapshot, [portfolio-analysis.md §Holdings change tracking](portfolio-analysis.md#holdings-change-tracking)); the run's snapshot persists with the run, so the portfolio stays viewable without re-fetching.
A **resumed** run performs no pull at all — it reopens its interrupted run's pinned snapshot (the resume path is designed, not built — [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).
Each position carries instrument identity (symbol, description, asset type — no CUSIP is mapped off the wire), quantity, cost basis (the **signed account-currency total** the app derives from Schwab's per-unit `averagePrice` — [schwab-integration.md §What is pulled](schwab-integration.md#what-is-pulled)), and market value (P/L is derived downstream as market value − cost basis, not a pulled field), from `GET /trader/v1/accounts/{accountHash}?fields=positions` (Schwab identifies accounts by a hashed number; the app resolves plaintext→hash first).
**Manual-import** positions (CSV/paste — designed, not built: [schwab-integration.md §Manual import](schwab-integration.md#manual-import-supplement)) would populate the same holdings model as a supplement.
Snapshot assembly then runs the **holdings-normalization step** — same-symbol rows across granted accounts and manual supplements net into one book-level position per symbol ([schwab-integration.md §What is pulled](schwab-integration.md#what-is-pulled)) — and every later step consumes only the normalized book-level rows.

**Option chains are fetched fresh within the run, per holding at its Step-6 dossier assembly** (so a selective run's carried tail spends no chain call), from `GET /marketdata/v1/chains` — per-contract volume, open interest, and IV (greeks ride the wire unparsed) — bounded by expiration and strike range; the **shared freshness bound** that would reject a stale chain (mirroring the report's COT freshness guard) is **designed, not built** — as-built no as-of timestamp is retained and no staleness rejection runs ([schwab-integration.md §What is pulled](schwab-integration.md#what-is-pulled)).
A per-symbol fetch failure or a malformed response (top-level or per-contract) degrades to the same typed options-signal gap, never a job failure; a genuinely un-optioned name (an empty chain or 404) carries no signal and no gap — a market fact, not a degradation — and a stale chain joins the gap conditions when the designed freshness bound lands ([schwab-integration.md §Failure posture](schwab-integration.md#failure-posture)).
The deterministic put/call + IV/skew signal these chains feed is computed with the dossier at **Step 6a** — by an engine function, but never an input to the Step-6b grade computation — and reaches the model in the **Step-6f** interpretation prompt as an explicit non-grade proxy, persisted on the verdict record.

## Step 3: Classify Asset Eligibility

**Type:** Computed (app layer).
No model.

Each position is classified before analysis (see [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)):
- **Stocks** — the full per-holding pipeline (Step 6, equity path), behind the **loop-time listing-resolution guard** at Step 6a (this step has only Schwab instrument identity — the same reason the fund strategy classification defers): a symbol with no canonical FMP resolution or a non-US primary listing re-classifies to **not-rated (unsupported listing)**; a resolved-but-conflicting identity abstains with the evidence floor's conflicting-identity outcome, routed at the guard itself before the engine stage — a guard-terminal outcome skips the holding's remaining per-symbol retrieval ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
- **ETFs / funds** — the **reduced** pipeline (Step 6, fund path): no single-company financials; graded on strategy / **exposure** (sector / country weightings — constituent look-through is off-plan), valuation, and the house view.
  The further **strategy classification** (asset class from `etf/info`) is a **loop-time routing decision, not made here** — this step is computed-only and `etf/info` is not retrieved until Step 6a — so each fund is classified and routed at 6a/6b once its metadata is in hand: equity funds the exposure-valuation path (US-exposure-guarded: below ~70% US by country weightings the composite is not an honest read, so the fund is unpriceable); bond / commodity funds a further-reduced path with valuation recorded as a gap; leveraged / inverse vehicles carry the deterministic structurally-path-dependent flag; a CEF adds the NAV read (the CEF leg designed, not built — [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
  Every unpriceable class returns the typed **`role_risk_only`** intrinsic verdict — no letter, no targets; the portfolio action machinery still applies ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
- **Options, fixed income, cash, unsupported types — and net-short equities** — marked **not rated**, with a reason, excluded from grading (a short's signed exposure still feeds the roll-up, and a long↔short reversal marks and badges the carried holding in a selective run — [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
  Cash still feeds the roll-up's descriptive cash read ([portfolio-analysis.md §Portfolio roll-up](portfolio-analysis.md#portfolio-roll-up)).

The eligibility decision is explicit and shown in the UI; a not-rated position never receives a fabricated grade.

## Step 4: Holdings Change Diff

**Type:** Computed (app layer) — a deterministic diff before any model stage.
No model.

The current holdings are diffed against the **prior run's persisted snapshot** (see [portfolio-analysis.md §Holdings change tracking](portfolio-analysis.md#holdings-change-tracking)).
Every current position is tagged **by quantity** — by position size (absolute for a same-side move, the signed swing on a sign flip), so a short and a net long↔short reversal read correctly, with cost basis as corroborating context rather than a second axis — as **new / increased / decreased / unchanged**; a symbol present last run but absent now is **exited** (no per-holding verdict — there is nothing left to grade — but surfaced in the Step-7 roll-up as closed-since-last-run).
Each holding's delta rides into its dossier so the verdict reasons over what the user actually did.
The diff is the application's, not the model's.

## Step 5: Load Shared Context

**Type:** Computed (local read — house view, investor profile) + API retrieval (run-level FRED rates; the CFTC / benchmark-series / CBOE / FMP-gold context loads are designed, landing with their consumers).
No model.

Three things are loaded **once per run and shared across every holding**, not re-requested per symbol:
- the **Market Signal house view** — the latest report's Thesis, Investment Strategy, and Forward Outlook sections plus recent report summaries (`thesis_stance`, `forward_outlook_themes`, `key_risks`), loaded **deterministically** from the report store (retrieve-don't-dump — never by vector-searching the report's memory; see [local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)).
  The report's **creation date** rides into the dossier so every downstream stage knows how old the thesis is, and a **freshness window applies**: if the latest report is older than **one week** (a pinned default), the house view is **omitted and recorded as a gap** rather than fed as current — a month-old thesis is not today's, and the data-honesty stance treats a stale input as absent, not current (the same posture the report takes on a stale data series).
  The window counts **whole ET session days on both sides** — the run's own session against the report's, each converted from its stored UTC instant, never a date prefix ([data-sources.md](data-sources.md) — the cross-cutting session-dating rule in its intro).
  Both legs must convert together: the two instants straddle the ~8 PM ET rollover, so a prefix read made an evening run see a 7-ET-day-old report as eight days old and drop the whole view, while converting only the run's side would read a report written after the rollover a day younger than it is.
  The holding is still graded on its fundamentals and research; it simply carries no house-view anchor that run;
- the **investor profile** (risk tolerance, horizon, objective, tax sensitivity — see [configuration.md](configuration.md)) — reaching the model **only at the per-holding action call** (Step 6f's second call), so the intrinsic verdict stays profile-independent by input isolation ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict));
- run-level market context — the **risk-free rates** (FRED `DGS10` / `DGS2`): `DGS10` anchors the engine's scenario-target function, the v2 rate-anchored multiple, and `DGS2` the capital-efficiency hurdle, the suite's short-end anchor mirroring Trade Opportunities' entry-threshold anchor ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
  The full run additionally loads the **anchor-window `DGS10` history** the v2 percentiles join against — one date-ranged request per run, retained as dated observations, the acquisition rule on the DGS10 row ([data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface)).
  A rate retrieval still failing after the shared bounded retries **hard-fails the run here, before any per-holding work** — the canonical rate-anchor rule, [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).
  Designed, landing with their consumers (as-built this step loads the rate anchors alone), the context also carries **cyclical commodity prices** for commodity-linked holdings (FRED daily energy plus the suite-shared monthly IMF metals — [data-sources.md §Trade Opportunities — endpoint surface](data-sources.md#trade-opportunities--endpoint-surface) — and gold via FMP `quote` `GCUSD`), the **CBOE daily put/call statistics** (an optional, fail-soft **venue-level options-sentiment backdrop** — broad-market context, never a per-name signal — [data-sources.md §CBOE](data-sources.md#cboe)), the **sector / market benchmark series** (FMP dated EOD — the identity table in [data-sources.md §Financial Modeling Prep](data-sources.md#financial-modeling-prep)) the input delta's technology-event pre-flag reads (the outcome-learning labels fetch their own benchmark closes at label time), and **CFTC Commitments-of-Traders positioning** on the bellwether contracts, which a commodity / macro **fund** holding maps onto for an underlying-positioning read.

## Step 6: Per-Holding Analysis Loop

Each **gradable** holding (stock or fund, from Step 3) is processed through the chain below.
Holdings are independent, so the loop is designed to **checkpoint per holding** — completed stages persisting so a cancellation or a single model failure resumes the unfinished holdings rather than restarting the (potentially hours-long) run, resume reopening the run's pinned snapshot and versions as its own entry path, never a fresh pull — **as-built only the between-holdings cancellation checkpoint exists** (mid-run persistence and resume are queued in the completion block; [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)), and recent research is cached within a freshness window (nothing is cached while the 6c research stage is stubbed).
The resident **122B reasoner fills every model role in this loop** by switching mode (thinking ↔ non-thinking), so moving a holding across its research passes (thinking), distillation (non-thinking — single or hierarchical, Step 6d), and interpretation (thinking) pays no model-swap cost ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).
A **fund** holding runs the reduced engine path (Step 6b) and a **fund-flavored research agenda** (Step 6c); the loop's structure — research, distillation, interpretation, continuity — is otherwise identical.
Sub-steps 6a–6g are the [portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline) six stages, with the target refinement (6e) surfaced as its own deterministic phase.

In a **selective re-analysis** (Run analysis with a selection), the **work-list is strictly the selected holdings** (ruled 2026-08-16, [verification/2026-08-16-selective-badges-ruling.md](verification/2026-08-16-selective-badges-ruling.md)) — nothing else is pulled in.
Before the loop the quick-check evaluation still runs over the unselected carried tail, but only to **badge** it, not to expand the work-list: a holding it flags, one whose sweep result is **`unknown`** (a required signal family's retrieval failed or a condition it covers couldn't be resolved — the degraded-sweep rule, [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)), one carrying an unexamined evidence event, and one whose **position side reversed** (marked `side_reversed`) each ride the card as a non-blocking badge ([portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering)).
Holdings left outside the selection carry their prior intrinsic verdict, action, and ledger forward, **vintage-stamped**, into the persisted run; a held position with **no prior verdict to carry** (new, or never analyzed) is left **not analyzed** — no verdict this run, rendered as a "run to grade" placeholder card, selectable for the next selective run.
Steps 1–5 run whole-book regardless (the pull, eligibility, diff, and shared context are cheap), so the diff baseline and snapshot semantics are unchanged.
The badges — not force-includes — are what keep mixed vintages safe now, specified once in [portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering); the one deterministic carry rule left is the over-age **add-family** demotion to *hold* (stamped `action_source: rule-demoted`), while an over-age exit or hold stands behind the stale-vintage badge.
Within the loop, research effort is **uniform, not graduated** (designed with the research slice — nothing is cached while 6c is stubbed): every analyzed holding runs 6c–6d in full each run.
A holding's cached distilled findings younger than the ~4-week window **seed** the loop (deterministically, per topic) and **merge** into its results at distillation rather than skipping any step; older or absent, the loop runs cold — the seed-and-merge contract and window live in [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable), and every **per-topic** seeded-vs-cold decision is logged to the audit record.

### Step 6a: Dossier Assembly

**Type:** API retrieval (FMP / SEC EDGAR) + Computed (assemble the packet); the local-model embedding call for semantic continuity retrieval is **designed, not built** (FINRA joins the retrieval sources with its designed short-interest leg below).

The step opens with the **listing-resolution guard's** per-stock profile read (the FMP `profile` identity — issuer name, exchange, sector; one fetch also supplying the outcome episodes' entry-stamped sector identity): a guard-terminal outcome (unsupported listing / conflicting identity) **skips the rest of this step's per-symbol retrieval** and routes the holding out of the loop — not-rated or insufficient-evidence — before the 6b engine stage, while an unverifiable guard proceeds as a recorded degraded input ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
The application builds the holding's evidence packet deterministically, starting from the position + its Step-4 delta.
It adds any **same-underlying option positions** from the Step-2 pull — deterministic symbol link, designed as a **typed overlay** (direction / quantity / strike / expiry / delta / coverage ratio, classified covered-call / protective-put / collar / other); **as-built no overlay leg exists** — the former construction-spine stand-in retired with that stage, so the typed overlay is wholly designed, unbuilt ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline) Step 1).
It is designed to add the symbol-scoped **`news/stock`** headlines as research-loop **seeds** (leads, never evidence — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)); as-built no dossier seed leg exists — the endpoint's wired consumer is the quick check's technology-falsifier news leg, and the seed leg lands with the research loop.
For a stock it adds the **equity** per-symbol surface: as-built the FMP fundamentals — quarterly statements, the forward-estimates consensus, dividends, quote + EOD — joined with SEC EDGAR as a **fill-only merge**; the revenue segments, analyst/revision signals, FINRA short interest, and a conflicting-value SEC cross-check are designed, landing with their data legs ([data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface)); **13F institutional, earnings-call transcripts, and per-symbol M&A are off-plan** → SEC EDGAR / the web-research loop / `mergers-acquisitions-latest`+8-K ([data-sources.md §FMP — current paid-plan tier audit](data-sources.md#fmp--current-paid-plan-tier-audit)).
For a fund it adds the **reduced ETF surface** instead: `etf/info` + sector/country weightings, plus the **sector-P/E surface** the 6b exposure-priced valuation reads — `sector-pe-snapshot` / `historical-sector-pe`, fetched **on first need and memoized across funds**.
The snapshot is fetched once per exchange per candidate session (the run's ET session date, then earlier weekdays until one serves; an exhausted walk records the typed gap on **every** fund rather than abstaining them as a sector-overlap failure), and the historical series per sector × exchange as each fund's retrieved weightings introduce sectors, so a later fund's new sector still gets its trailing history — the sector set can't precede this step ([data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface)); constituent `etf/holdings` and mutual-fund `funds/disclosure*` are off-plan.
It adds deep price history (FMP dated EOD) and a live quote (FMP `quote`); the prior run's verdict **and thesis ledger** for this holding ([portfolio-analysis.md §The position thesis ledger](portfolio-analysis.md#the-position-thesis-ledger)); and the Step-5 shared context.
**Designed, not built**: vector-retrieved continuity from **this job's own prior runs** for this holding — as-built no semantic retrieval runs, and cross-run continuity is exactly the deterministically loaded prior verdict + ledger.
The full input list and every endpoint is in [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) and [data-sources.md](data-sources.md#portfolio-analysis--endpoint-surface).

#### Local-model call — Vector continuity retrieval (Qwen3-Embedding-4B, fixed)

**Designed, not built** — no semantic retrieval runs as-built (the deterministically loaded prior verdict and ledger are the continuity inputs); the contract below binds the lane when it lands, revisited once the job's learning corpus has accumulated.

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

The engine computes the holding's quantitative picture in **three layers**.
**(a)** The grade core → for a stock, the quality / valuation / risk sub-scores the letter rolls up from; for a priced equity fund, real valuation / risk plus the neutral-imputed absent quality axis defined by the fund-grade contract ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)) (momentum computed alongside, outside the letter — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)) — and the scenario price targets (the **v2 rate-anchored scenario-target function** off the run-level `DGS10` — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
**(b)** A conviction layer → the **momentum / market-setup read** and — designed, landing with their data legs ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)) — the narrative-vs-reality ratio (with its thin-coverage fallback), the **forensic flags** (the hard-event kinds reading the shared typed producer — the item-classified EDGAR filing kinds plus the research-fed `forensic_event` claim — [trade-opportunities-workflow.md §Step 5c](trade-opportunities-workflow.md#step-5c-deterministic-analysis-archetype-weighted-engine)), and the **implied-expectations read** (the shared Step-5c primitive), all kept *out* of the letter.
**(c)** Positioning context (as-built the Step-2 **options-activity signal**; designed — insider / congressional / **FINRA short interest**; FMP 13F off-plan → EDGAR/omit), held out of the sub-scores until the outcome-learning scorecard calibrates it ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)).
For a priced stock meeting the deterministic eligibility rule, layer (b) additionally computes the statement-derived legs of the **pre-profit execution / financing overlay** ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
Eligibility is `TTM operating income ≤ 0`, or `no positive forward-EPS consensus AND TTM free cash flow < 0`.
From the comparable quarterly statements the engine computes liquid resources, TTM cash burn and runway months, TTM capex intensity, split-adjusted year-over-year diluted-share change, the latest two-quarter average gross margin, and its change from the preceding two-quarter average.
It derives the financing state and the statement-derived economics / dilution legs, typing every missing input `unscorable`; funds and `role_risk_only` holdings skip the overlay.
The non-standardized operating observations — production, deliveries, bookings / backlog / reservations, guidance, and unit economics — reach the overlay only from validated research, which Step 6e merges before the engine derives the observation-dependent execution read, severe-deterioration state, and rule consequences.
As-built that research producer is dormant, so no new row exists to guess, and the engine computes the complete overlay — those observation-dependent legs included — here in one pass over an empty candidate list, deriving the execution read from the carried prior observation history alone.
This stage also **assigns and persists the holding's risk tier**, per branch — the deterministic assignment rule of [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable) (a `role_risk_only` holding carries none) — before anything downstream consumes it.
The forward targets are a **provisional scenario menu** at this point; from them the engine also derives a **capital-efficiency / dead-money read** (total-return basis, DGS2-anchored, scaled by the tier just assigned, three-state — only *fails* is dead money; [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)), kept out of the sub-scores like layers (b)/(c) and fed to the action call's evidence digest.
The three-layer design is in [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) Step 2.
For a **fund**, this step runs the reduced computation instead — routed by the **strategy classification made at loop time** from the 6a `etf/info` pull (Step 3's eligibility used only Schwab instrument identity) — the reduced fund computation; an **unpriceable class** computes what it honestly can and returns the typed **`role_risk_only`** verdict ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
The **input delta** the continuity audit (6f / 6g) attributes verdict moves to is, as-built, deterministic evidence rather than an engine-computed comparison: the Step-4 position delta, the ledger crossings below, the Step-5 house-view age / change, and the prior run's stored values (from the audit record) rendered beside this run's for the interpretation call to compare.
An engine-computed delta over metrics, sub-scores, and positioning is **designed, not built** — the positioning leg additionally waits on its data legs.
As part of the input delta, the engine also evaluates the prior thesis ledger's **quantitative** falsifiers and triggers — which conditions crossed this run, under their **persistence semantics** ([portfolio-analysis.md §The position thesis ledger](portfolio-analysis.md#the-position-thesis-ledger)) — for interpretation to read; the **technology-event pre-flag is designed, not built** — it lands with its benchmark context loads ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
This stage ends with the **evidence-floor check** — deterministic, over the floor-bearing inputs now all in hand: a below-floor holding **exits the loop here** with the typed **`insufficient-evidence`** disposition and named gap reasons, **checkpointed as completed** (checkpoint/resume is designed, not built — [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)) — Steps 6c–6f never run for it, its standing ledger and any attention flag are retained, and the full exit-state semantics (roll-up contribution, no per-holding action, no new outcome episode) are specified once in [portfolio-analysis.md §Evidence floor](portfolio-analysis.md#evidence-floor).

### Step 6c: Bounded Web Research

**Type:** Local-model call (122B, thinking) + API retrieval (the web tool), **looped**.
This is the only **per-holding** stage that loops.
**As-built this whole stage is a stub**: no agenda, loop, or web tool runs — the orchestrator records a single research-deferred note, so every run to date has graded on the deterministic financials and the house view alone.
Everything below (including the model-call block) is the research-loop slice's binding design.

The orchestrator assembles the holding's **agenda** deterministically — the reasoner works it, never authors it ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).
The agenda is competitive position, recent results/estimate revisions, catalysts/risks, **management quality & capital allocation**, market narrative & sentiment, and forward opportunity & thematic fit.
**Conditionally** it adds a technology-event impact assessment that reads the actual technology and sizes the holding's real exposure into a typed `technology_read`; the conditional topic is triggered by the engine's Step-6b **event pre-flag**, a standing technology-class ledger falsifier, a qualifying `news/stock` seed, or an orchestrator-approved mid-loop follow-up proposal, and stays dormant otherwise.
**For an overlay-eligible stock** it adds a pre-profit execution / financing topic over the actual operating proof the issuer reports (see [portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)).
The orchestrator works the agenda **one topic at a time** — **each topic a separate isolated conversation and research loop**, a bounded multi-turn pass loop ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)) — run over a **clean context**: the dossier facts plus that topic's own questions, with no other topic's findings fed in.
A pre-profit topic asks for comparable, dated issuer observations on production / deliveries where applicable, bookings / backlog / reservations, guidance ranges and matching actuals, unit economics, gross-margin commentary, cash needs, capital spending, and issued or planned financing.
On the holding's first overlay-eligible full pass, or when a previously used guidance metric has fewer than four comparable stored periods, the agenda additionally requires one bounded historical backfill over that metric's latest four reported periods and records which periods / sources were checked plus `complete` / `partial` / `unscorable` coverage.
The model identifies and extracts the facts; it does not calculate runway, attainment, dilution, state, conviction, or action consequences.
A **fund** holding's agenda swaps the company-centric topics for fund-flavored ones matched to its ledger's driver set — mandate / strategy and manager changes, expense and structure vs its category, the exposure's fit against the house view (and whether it is better held directly), and (CEF) the discount and distribution coverage; the technology-event topic is equity-only ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)).
The orchestrator — not the model — owns every request: per-topic depth ≤2 (≤3 passes/topic) and a **per-item fetch + wall-clock budget that binds first**, spent in topic-priority order, fail-soft on exhaustion.
Grounded by the deterministic financials so research fills the gaps the numbers don't. The full loop and its bounds are in [web-research.md](web-research.md).

#### Local-model call — Per-holding research (Qwen3.5-122B, thinking)

**Model.**
The resident 122B reasoner in thinking mode, requesting `web_search` / `web_fetch` tool calls the orchestrator executes (SearXNG-primary, Tavily fallback; SSRF-guarded; untrusted page text inserted as quoted evidence, never as instructions — see [web-research.md §Safety and provenance](web-research.md#safety-and-provenance)).
**One isolated conversation per agenda topic** (a bounded multi-turn pass loop — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)) — topics do not share a context.

**Prompt — input.**
The holding's dossier facts, **that topic's questions**, and — for a holding with non-expired cached research — **that topic's own prior distilled object** (its tier-1 distillation, or its topic-keyed group from a single-pass run) and its ledger conditions as a deterministic orienting **seed** (Portfolio's designed reuse, a bounded cross-run prior — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)); a clean per-topic context otherwise.
Within a pass the model reasons over the fetched, readability-extracted page text and an **append-only evidence ledger** (each extracted claim + its source URL / timestamp); there is **no in-loop re-distillation of *this run's* findings** — the heavy consolidation is deferred to the Step-6d distillation, so research is never planned over its own already-distilled, lossy notes (the cross-run reuse seed above is a distinct prior-run object, not an in-loop summary).

**Returns.**
The topic's **full findings response**, preserved whole (with its evidence-ledger entries), plus any **follow-up proposal** (a structured field the orchestrator decides whether to spend) and any **material forward fact** flagged for the Step-6e refinement.
Every worked topic's full response flows intact to distillation — nothing is summarized away in between — where it is consolidated in a single pass or, when the holding's research is large, **hierarchically** (a tier-1 distillation per topic-tree → a reduce, Step 6d).

### Step 6d: Distillation

**Type:** Local-model call(s) (122B, non-thinking; the optional 35B fast tier if resident) — a single pass, or **hierarchical** (tier-1 per topic-tree → a reduce) when the holding's research is large.
Consolidation, not new reasoning.
**As-built** — with research stubbed (Step 6c) — the call is a single non-thinking condense of the stub note: no evidence-ledger leg, no hierarchical path, and no `format` schema (the doc-wide schema-constrained rule's one standing exception), and the `role_risk_only` branch makes no research or distillation call at all.
The contract below binds when the research loop lands.

The reasoner in non-thinking mode consolidates the topics' **full findings responses** into the compact object the interpretation stage reads — a consolidation over the **complete** per-topic outputs, never a re-distillation of already-distilled notes — so interpretation reasons over a clean synthesis of full-context research ("forward only what's needed").
This is the *only* place research is condensed before interpretation.
It runs as **a single pass by default, or hierarchically** (tier-1 per topic-tree → a reduce) when a holding's research is large — the deterministically orchestrator-chosen primitive shared with Trade Opportunities ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)); there is no cross-lens contradiction check here, so the reduce is purely consolidation.

#### Local-model call(s) — Distillation (Qwen3.5-122B, non-thinking)

**Model.**
The same resident 122B in non-thinking mode by default (no model-swap cost); the fast 35B tier is a benchmark-gated option ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).

**Prompt — input.**
*Single pass:* the **full findings response from every worked topic**, the append-only evidence ledger (claims + sources), and — for Portfolio with a non-expired prior — the **seeded per-topic prior objects**, each merged into its own topic and the call's output **keyed by topic** so every group persists as the next run's seed ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
*Hierarchical:* each **tier-1** call gets one topic-tree's complete findings + that tree's ledger entries **+ that topic's non-expired prior object** — Portfolio's reuse merges **per topic here**, fresh superseding cached (a topic whose *complete* input — its passes' findings, their evidence-ledger entries, and the prior — would itself overflow one call sub-distills along the pass seam, each pass carrying its findings *and* ledger entries together and the bounded prior retained; a fail-softed pass takes its ledger entries with it — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)); the **reduce** gets the merged tier-1 structured outputs with their preserved citations, resolves cross-topic claim/metric conflicts by the same newest-wins rule and dedups sources across topics, and **emits both the combined object and the per-topic seed layer reconciled to those winners** — the persisted seed is that reconciled layer, not the raw tier-1 output (no cross-lens contradiction check here — the reduce is consolidation plus that global reconciliation — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).

**Returns.**
Two schema-validated artifacts, both **emitted by the reduce** (or the single-pass call) from one cross-topic reconciliation, so they are mutually consistent: the **per-topic seed layer** — one distilled object per analyzed topic (the **reduce-reconciled** tier-1 objects under hierarchical 6d, the topic-keyed groups under single-pass 6d; the raw tier-1 output is not itself persisted), emitted with any cross-topic-superseded claim already updated to the global winner and persisted as the next run's per-topic seeds — and the **combined distilled findings object** for interpretation (the cross-topic reduction of that layer, globally reconciled fresh-supersedes-cached). The reduce owns the claim match in both, since Portfolio's claims carry no app-matchable cross-topic identity key.
The combined object surfaces the following typed fields where supported (structured and sourced, with numeric fields never carried as loose prose):
- a **`research_forward_assumption`** — `{ fact type, numeric value, units, period / as-of date, source URL, confidence, the target assumption it affects, conflict_handling }` — the only thing that can reach the engine's target refinement (Step 6e).
  Its **`conflict_handling`** member is a typed two-value declaration, never free text — **`supplement`** (the charter case: the fact fills a forward value the structured feeds don't carry) or **`supersede`** (the fact contradicts a value a structured feed carries) — and it is a **claim the engine validates under the app-owned conflict policy of Step 6e, never a rule the model selects** (the model-proposes / app-validates spine); and
- when research validated a **countable, dated, third-party leading indicator the structured feeds did not carry** (a `research`-class signal), a typed **`validated_leading_indicator`** — `{ metric name, value / level, direction (inflecting up / down), as-of date, source URL, confidence, the thesis-ledger key driver it confirms }`.
  It is **distinct from `research_forward_assumption`** (which moves targets); its conviction-raise citation role is **retired with `portfolio-v7`** — conviction is the model's own ([§Step 6f](#step-6f-interpretation-and-grading)), so the typed signal reaches the model as ledger-driver evidence with no gate riding on it (Trade Opportunities retired the same machinery on the same reasoning, so the retirement is suite-wide — its **cap** contract stands, [trade-opportunities.md §The opportunity](trade-opportunities.md#the-opportunity)); and
- where research surfaced a **hard forensic event**, the shared typed **`forensic_event`** claim — the producer contract's record (event kind, issuer, event / filing date, primary-source lineage, confidence), schema and validation single-homed at [trade-opportunities-workflow.md §Step 5c](trade-opportunities-workflow.md#step-5c-deterministic-analysis-archetype-weighted-engine) — the **fraud kind's sole producer** (it has no structured feed) and the only seam through which a research-surfaced hard event reaches Step 6g's hard rule.

For an overlay-eligible stock, the object may additionally carry **`pre_profit_execution_observations`**, a list of `{metric_kind, observation_role, polarity, numeric_value, units, period, issuer_scope, source_url, published_at, confidence}` rows, plus the backfill attempt's checked periods / sources and `complete` / `partial` / `unscorable` coverage state where the agenda required one.
`observation_role` distinguishes an actual, guidance low, guidance high, point guidance, and contextual level, so the engine can pair comparable facts without asking the model to calculate attainment.
`polarity` is the typed **`higher-is-better` / `lower-is-better` / `target-band`** direction the app validates against the metric kind; only a higher-is-better observation enters the currently defined guidance-miss rule.
The model may extract a row only from source text that states the value; app validation and Step-6e computation own every comparison and state.

These typed fields exist only where their consumers do: a **`role_risk_only`** holding's distillation emits none — no target for an assumption to move, no surviving indicator consumer (the conviction-raise leg is retired with `portfolio-v7`; the indicator's ledger-driver-evidence role rides the priced branch, above), and the hard outcome's legs all live on the priced branch — and is pure consolidation for interpretation.

### Step 6e: Deterministic Target Refinement

**Type:** Computed (the engine).
No model.

This stage also finalizes the pre-profit execution / financing overlay where the priced stock carries one; the heading retains the established target-refinement name because that is the stage's original shared seam.
**Both of this stage's legs are designed, not built** — the forward-assumption target recompute and the observation-driven overlay finalization each land with the research-loop slice.
As-built no research row reaches this seam, so the engine already computed the complete overlay — its states and rule consequences — at the Step-6b engine seam, and this stage recalculates nothing.

If distillation produced a typed **`research_forward_assumption`** (Step 6d — a guidance figure, a signed-contract value, a commodity / ASP turn, each with value, units, as-of date, source, confidence, and its declared `conflict_handling`), the **engine — not the model —** recomputes the affected scenario target with it as an explicit, **logged** assumption.
A malformed, unsourced, or non-numeric claim is **rejected** (it cannot move a target), and a fact that **conflicts** with a structured feed resolves under the **app-owned conflict policy — the model's declaration never selects or bypasses the rule**: a `supplement` may only fill a value the feeds don't carry (it never displaces a present feed value), and a `supersede` is honored **only when the engine verifies all of** — an as-of date strictly newer than the conflicting structured observation's, a fact type on the primary-source whitelist (issued company guidance, a signed contract, a filed figure — drafted), and metric, units, and period matching the feed field it contradicts.
A conflicting assumption failing any check is **rejected and logged with the failed condition — the structured value stands** (structured-wins is the default), and every resolution records the rule the engine matched in the run's audit record ([storage.md §Local Analysis Suite Storage](storage.md#local-analysis-suite-storage)).
So the number stays engine-computed while the forward view reflects what research learned.
Because the **capital-efficiency / dead-money read** derives from the base-case target, the engine **recomputes it here too** when refinement moves that target, so Steps 6f and 7 read a current flag rather than the provisional Step-6b one.
For an overlay-eligible stock, the app separately validates every `pre_profit_execution_observation` structurally — metric kind and polarity, numeric value, units, period, issuer scope, source URL, publication date, and confidence — rejecting a malformed row, or a duplicate (of a stored observation, or of an earlier accepted row in the same batch), with its reason.
A structurally valid but as-yet-unpaired row is kept for a future match rather than rejected; accepted rows merge into the period-keyed observation history, and the pairing of guidance to actuals happens in the execution read below.
The **holding-identity cross-check and source-text corroboration legs are not yet built** — they are the recorded activation obligation of the research-loop slice (with the period-normalization hard rule), which must land before the observation producer activates; while the producer is dormant no row reaches this validator.
It also validates and records any required backfill attempt's searched periods, source coverage, and completion state; a missing period stays a gap and never becomes a model-inferred observation.
The engine then pairs guidance and actuals only where normalized metric identity, issuer scope / perimeter, units, and period are comparable; for a higher-is-better metric with a finite positive lower bound it computes `miss_ratio = (bound − actual) ÷ bound`; treats only `miss_ratio ≥ 0.05` as an execution miss; requires at least two distinct missed periods for that same metric among its latest four comparable periods to produce repeated miss; and treats `miss_ratio ≥ 0.20` on the latest period as material single miss.
Different metrics never combine into repeated miss, and two missed metrics from one period never count as two periods.
The engine joins those execution reads to the Step-6b financing, economics-deterioration, and material-dilution states and derives the **severe-deterioration** state under [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable).
The resulting rule consequences are also deterministic: repeated execution miss → Medium conviction ceiling; constrained runway → add-family bar; severe deterioration → Low conviction ceiling + add-family bar + an exit-family-only (`{trim, sell all}`) action rule — since `portfolio-v7` all three bind the **engine arm** (its stand-in conviction and action observe them) and reach the model as rendered engine rules, the model's own conviction and action unrestricted with departures annotated.
No single observation can create the severe state, and the letter grade and scenario targets remain untouched by the overlay.
The backward-looking sub-scores are untouched; absent a valid assumption, the Step-6b targets stand (see [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) Step 5).
A **`role_risk_only`** holding **skips this step entirely** — its branch carries neither scenario targets nor the priced-stock overlay ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)).

### Step 6f: Interpretation and Grading

**Type:** Local-model call (122B, thinking) ×2.
The verdict-writing call, then the **action decision**.

The reasoner interprets the computed analysis and the distilled research into the holding's **intrinsic verdict** — since `portfolio-v7` a **two-arm** record ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)).
The engine's deterministic values ride as the **baseline arm**, and this call authors the **model arm**: its own four sub-scores (the model letter derives app-side through the shared cutoffs), its own freely-authored one-/twelve-month target bands, conviction, the horizon reads, and a retrospective `self_assessment` — with the engine's numbers in the prompt as evidence, never as bounds.
The same call rewrites the **thesis ledger** (revised thesis, re-weighted monitor, re-set falsifiers/triggers), reading the engine's quantitative crossings from 6b and judging the qualitative conditions from research; the ledger stays fully validated — it feeds deterministic consumers, so it is not part of the unrestricted model arm.
For a **`role_risk_only`** holding the same call authors the union's other branch instead — the role / risk assessment and the rewritten fund ledger, none of the priced fields (see **Returns**).
Since `portfolio-v9` the interpretation authors **no action**: a second, dedicated **action call** follows in the same step — it reads the finished verdict, the holding's own sizing evidence, and the **investor profile** (its only entry point into the job), and returns the rung-only portfolio action with a one-line rationale (see the second call block below).

#### Local-model call — Interpretation & grading (Qwen3.5-122B, thinking)

**Model.**
The resident 122B in thinking mode; schema-constrained output.

**Prompt — input.**
The engine's computed analysis reaches the prompt: sub-scores, the refined scenario targets with exposed methodology **and typed provenance** — the `TargetMeta` derivation flags (rate-anchored vs current-multiple carry, flat / clamp-flattened driver, dispersion floor) rendered into the prompt so a low-signal target surface is weighed, not obeyed — any finalized **pre-profit execution / financing overlay** with its rule-bounded conviction ceiling and engine action rules, and the options-activity signal.
The narrative-vs-reality and forensic reads and the **implied-expectations range** join when their layer-(b) producers land; the **same-underlying option overlay** reaches no prompt as-built — its dossier leg is designed, unbuilt, and its former stand-in retired with the construction spine ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)).
The prompt also carries the distilled research findings and the house view — **scoped to the horizon reads and market-setup context**, never by itself a per-holding exit reason (the investor profile is **deliberately absent**: the intrinsic verdict is profile-independent and the profile enters at the action call only — [portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)).
It carries the prior **thesis ledger** whole, with the engine's quantitative falsifier/trigger crossings from 6b (first-breach vs confirmed per their persistence semantics), plus the band-recalibration NOTE the app derives from the prior letter's stamped **grade-band parameter version**.
Since `portfolio-v7` — a **deliberate reversal of the v4 anchoring guard** — it also carries the **retrospective block**: the prior run's both-arm values (the engine baseline and the model's own), the price move since the prior run, and the deterministic scoreboard's matured window lines for the holding, because self-assessment against the baseline is the model arm's point ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)).
And it carries the position delta.
The **absolute street opinions** (consensus target level, current rating consensus, FMP's ratings snapshot) are designed to be presented as *evidence to weigh against the engine's own read*, not as numbers to adopt — their endpoint rows are not yet pulled, so as-built the only consensus the prompt carries is the target function's forward-estimates read ([data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface)).

**Returns.**
The schema-validated **intrinsic verdict** — a **discriminated union of two branches** ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)).
The default **`priced`** branch is, since `portfolio-v7`, a **two-arm** record.
The engine arm — the composite grade (A–F) over the branch-applicable grade contract (real quality / valuation / risk for a stock; real valuation / risk plus the neutral-imputed absent quality axis for a priced equity fund — [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)), momentum riding as market-setup context outside the letter, the scenario targets, and the mechanical stand-in outlook / conviction / action ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)) — is app-stamped, never echoed through the model.
The **model arm** is authored here, **structurally validated only** (types and enums, never value bounds): the model's own four sub-scores on the shared 0–100 scale (its letter derived app-side through the shared cutoffs), its own one-/twelve-month base/bear/bull target bands (persisted exactly as returned — an inverted pair renders annotated, scoring reads it as `(min, max)`), a single unrestricted **conviction**, and the retrospective **`self_assessment`** prose.
The conviction-raise triple and its 6g re-derivation are **retired unbuilt** with the v7 unrestriction (the anti-reflexivity guard survives where it feeds deterministic consumers — the ledger's tripped/fired validation — not as a clamp on the model's own value).
A matched pre-profit conviction ceiling reaches the model as **prompt evidence and persists as an audit annotation**; it neither narrows the schema nor clamps the returned value, the engine arm's own conviction observes it by construction, and severe deterioration's action rule binds the engine's per-holding action set at the action call.
A below-floor holding never reaches this call — it exited at Step 6b with the outer `insufficient-evidence` disposition ([portfolio-analysis.md §Evidence floor](portfolio-analysis.md#evidence-floor)).
The **forward outlook** is short/mid/long horizon reads with the selected one-month / twelve-month targets ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)), the forward read kept distinct from the backward grade.
A financial-health read and a short **`price_target_rationale`** line join it, plus the **rewritten thesis ledger** — thesis, bear/base/bull monitor, falsifiers, triggers, each **quantitative** falsifier and trigger stated **machine-evaluably**: the engine series, comparator, threshold, and persistence semantics, validated at Step 6g.
Last is the **what-changed audit**: every moved intrinsic value (grade, sub-score, conviction, target, horizon, a re-weighted scenario, a tripped falsifier / fired trigger) shown old → new with its cause **attributed** to an *external* change (market data / company information / research-narrative, each tied to the engine's input delta or a research finding), to a **labeled self-correction** where the inputs did not move, or — when the prior verdict's stamped `grade_parameter_version` differs from the current bands — to the **band recalibration**, never mislabeled as evidence or self-correction.
A **`role_risk_only`** holding returns the union's other branch — the role / exposure / risk / expense-drag read, structural flag, evidence gaps, the rewritten fund ledger, and its what-changed line; none of the priced fields, per the canonical branch schema ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)); its action comes from the same action call as the priced branch, decided from the branch's own attributes.
Full schema in [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict).

#### Local-model call — Action decision (Qwen3.5-122B, thinking)

**Model.**
The resident 122B in thinking mode — the rung is a judgment call weighing the whole verdict against the profile; schema-constrained output (action + rationale, the full ladder structurally open on both branches).

**Prompt — input.**
The finished verdict digest — for a priced holding both arms' grades and scores, conviction, the horizon outlook, the targets' implied upside/downside against spot with their provenance, the capital-efficiency read, and any pre-profit overlay rules; for a `role_risk_only` holding its class label, role read, exposure tilt, expense drag, observable risk, structural flag, and evidence gaps.
Beside it: the position's own economics (unrealized P/L, with the tax framing flagged as a user consideration, never the mover), the prior run's action as the continuity baseline, the **ENGINE SET** — [portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action)'s per-holding feasible set for a priced holding, the reduced {sell all, trim, hold} for the role/risk branch — rendered as the engine arm's own restriction with its pick deliberately withheld (the scoreboard needs the arms independent, the ruled 6f precedent), and the **investor profile**.
**Tunnel vision is stated as the contract**: no whole-book input exists — no cash position, sector weights, concentration, or other holdings — and the prompt says a separate planning stage reconciles actions across the book later.

**Returns.**
The schema-validated **`ActionDecision`**: one rung from the fixed ladder (sell all → trim → hold → add → add aggressively) and a one-line rationale, persisted on the verdict as `action` + `action_rationale`.
**Rung only** — no target weight, share count, or dollar figure; sizing is whole-book work and belongs to the future portfolio planner.
A chosen rung outside the engine set persists exactly as authored, with the departure **app-stamped on the holding's audit** (`action_annotations`) — annotate, never bar, the two-arm contract's posture.

### Step 6g: Continuity Check and Checkpoint

**Type:** Computed (app layer).
No model.

This step is an **app-layer validator**, not just a recorder.
Every move the 6f audit labels **external** must resolve to a concrete entry in the engine's input delta, a source-backed research finding, or the logged `research_forward_assumption`; an attribution that resolves to nothing is **downgraded to self-correction** (or fails schema validation), so the model cannot launder a no-new-facts swing as "the market changed."
This attribution validator is **designed, unbuilt** as-built.
The `research_forward_assumption` leg has no type in code, and with research stubbed no source-backed finding exists, so today the what-changed audit is model-authored prose disciplined only by the interpretation prompt.
The downgrade-to-self-correction and fail-validation machinery lands with the validator, and the self-correction counters stay structurally zero until then ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)).
Since `portfolio-v7` the model's **conviction is its own** — no re-derivation, ceiling, or clamp touches it (the conviction-raise triple and its app recomputation are retired unbuilt with the unrestriction; the anti-reflexivity guard survives in the ledger validation below, where it feeds deterministic consumers, not as a bound on the model's value).
The engine-matched **cap rules persist as annotations that bind the engine arm**: a tripped hard forensic trigger (restatement / auditor change / fraud flag — the trip resolving only from the shared typed producer: an item-classified filing kind, or a validated `forensic_event` research claim from 6d, never a bare model assertion — [trade-opportunities-workflow.md §Step 5c](trade-opportunities-workflow.md#step-5c-deterministic-analysis-archetype-weighted-engine)), a repeated execution miss's Medium ceiling, and severe deterioration's Low ceiling each record on the audit and bound the **engine stand-in arm's** conviction, action rule, and the per-holding engine action set — while the model's conviction and action persist as authored, the departure rendering beside the recorded rule ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable) stays the canonical hard-outcome record).
As-built only the **severe-deterioration** Low ceiling can bind today: its legs are statement-derived, so it trips without research.
The repeated-execution-miss cap waits on the stubbed observation producer, and the hard-forensic trip on its own unbuilt producer (the item-classified filing kind / `forensic_event` claim), so neither fires yet.
The recorded consequences come only from the Step-6e engine state assembled from app-validated observations; model prose cannot create an execution miss, runway state, or severe conjunction.
The same rule validates the **thesis-ledger rewrite**: every falsifier marked tripped and every trigger marked fired must map to a 6b quantitative crossing or a source-backed finding, or it is rejected.
Every **newly written or rewritten quantitative condition is additionally executability-validated**: it must resolve to a series the engine actually computes and refreshes — the suite's shared resolution contract ([trade-opportunities-workflow.md §Step 3c](trade-opportunities-workflow.md#step-3c-carried-forward-watchlist-re-check)), with metric, comparator, threshold, units, and persistence semantics all well-formed; one that doesn't resolve is **downgraded to qualitative with a logged reason, never dropped**, and retains no machine evaluation state — so the quick check's "every quantitative condition" promise is total over conditions it can actually evaluate ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
The **app-owns-the-number rule holds for the engine arm by construction** ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)): the engine's grade, sub-scores, targets, overlay values, and monitor stamps are **app-stamped directly, never echoed through the model**, so no unrestricted-arm output can alter them (the typed, app-validated channels that will be the deliberate exceptions — the Step-6e `research_forward_assumption` and the pre-profit observations — are designed, each validated before it binds when live but both dormant as-built per the caveats above; the boundary statement at [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)) — while the model arm's numbers are its own by design, **never validated against the engine's** (the two-arm contract: the baseline stays incorruptible precisely because the unrestricted arm cannot touch it, and the model stays unrestricted because the baseline's deterministic readings never depend on it).
The rewritten ledger (validated above) and the recorded **what-changed audit** ride against the prior run; as-built the audit is model-authored prose — its attribution validator unbuilt — so firm run-to-run output rests on the ledger validation and the interpretation prompt's discipline until that validator lands ([thesis-continuity.md](thesis-continuity.md)).
On the holding's successful full pass, this step also clears any persisted quick-check attention flag for the holding and stamps the triggering condition's acknowledging observation; the 6b evaluation this same pass consumed is continuity input, never a fresh persisted flag.
The completed holding is designed to be **checkpointed** here so a resumed run skips it (checkpoint/resume is designed, not built — [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).

## Step 7: Outcome Pass, then Persist Run and Audit, with Memory Embeddings

**Type:** Computed (engine + persist) + API retrieval (label-time dated-EOD bars + dividends) + Local-model call (embeddings — as-built the matured outcome learnings alone; the per-holding verdict embeddings are designed).

With the per-holding loop complete, the loop's actions are final — under the tunnel-vision contract no reconciliation stage follows ([portfolio-analysis.md §Portfolio roll-up](portfolio-analysis.md#portfolio-roll-up)).
The app builds the deterministic **roll-up** — verdict counts, the concentration and cash reads, the exited-names acknowledgment from the Step-4 diff (graded nowhere, but acknowledged rather than silently dropped), and the run-level **data-health** aggregate.
Then the deterministic **outcome-learning pass** runs: the engine tags each active decision episode's **`observed_net_alignment`** from the Step-4 diff and computes any newly **matured window labels** after refreshing the episode symbol's dated-EOD bars through the window end — including symbols no longer held — and re-pulling its `dividends` history for the total-return leg (the pass's own label-time API retrievals).
A failed refresh uses cached bars only when they cover the full window; otherwise the label remains pending with a coverage gap, bounded by the shared price-coverage grace — past it the leg closes typed, the grace doubling as the transient-vs-disappearance discriminator: a symbol the source never covered takes **`price-coverage-unscorable`**, while a series that covered the entry and then stopped resolves per the terminal contract ([storage.md §Local Analysis Suite Storage](storage.md#local-analysis-suite-storage)).
It then appends-or-extends this run's own episodes and derives the scorecard reads over the updated set — label mechanics, bases, and cohort layers all per the canonical contract in [portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration); fresh episodes carry no matured windows, so the reads move only on matured history.

The application persists the run: each holding's verdict, the per-holding **thesis ledger** (carried forward to seed the next run's continuity check), each priced stock's **pre-profit overlay record** (the eligibility read for every priced stock — a priced fund carries none; period-keyed execution observations and derived state where entered), the roll-up, and the **holdings snapshot it ran against** (the next run diffs against this).
It persists the **run audit record** that makes the run traceable, in the **full design**: sources and retrieval timestamps, distilled findings, computed metrics and derived reads, the input delta and what-changed attribution, the price-target methodology, model ids and quantizations, prompt/schema version, degraded-input flags, and each holding's research-reuse decision.
The field set **and its as-built wired subset** are specified once in [storage.md §Local Analysis Suite Storage](storage.md#local-analysis-suite-storage).
It also **appends or extends** this run's **decision episodes** in the outcome-episode store and attaches any Step-6g-confirmed falsifier events to the episode that carried their condition — creation, extension, event, and vintage semantics per the canonical contract ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)) — and records the outcome-learning pass's matured labels and derived scorecard reads with the audit record, the matured reads additionally embedding as **durable learnings** in the job's memory partition ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)).
Retention keeps the last N runs; the episode store and its matured archive persist independently of that window ([storage.md](storage.md)).

#### Local-model call — Run-result embeddings (Qwen3-Embedding-4B, fixed)

**The per-holding verdict embeddings below are designed, not built** — as-built the only vectors this job writes are the outcome pass's matured learnings (the durable-learning rows above); the per-holding write joins with the Step-6a retrieval lane, revisited once a learning corpus exists.

**Model.**
The fixed local embedder — vectorization only.

**Prompt (input text).**
Each holding's verdict embedded individually — a text that captures the **standing thesis** (the ledger's thesis, key drivers, and scenario lean), the **intrinsic read** (grade and conviction — or, for a `role_risk_only` holding, its role read and structural flag), and the **portfolio action**, so cross-run semantic recall surfaces the substance of prior analysis rather than a bare grade.

**Returns.**
Vectors stored in the **Portfolio Analysis** memory partition (the job namespace), so a later run of this job can semantically recall the relevant prior analysis for a holding ([local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)).
Best-effort: a failed embedding costs the memory row, never the persisted run — and an **invalid** vector (the shared validator — [local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)) is dropped and logged the same way, so a bad vector never enters durable memory.

## Step 8: Generate Portfolio Page and Update UI

**Type:** Computed (frontend).
No model.

The **Portfolio page** renders each holding's verdict: the **intrinsic** read — the **backward grade** and sub-scores paired *side by side* with the **forward outlook** (horizon reads and scenario targets), so a grade/outlook divergence is legible at a glance, plus conviction and the bear/base/bull monitor — beside the **portfolio action** (the rung with the action call's rationale — no sizing, per the tunnel-vision contract), with financials and the what-changed line.
A **`role_risk_only`** verdict renders its own explicit card branch: role, exposure, observable risk, expense drag, structural flag, evidence gaps — never empty priced placeholders.
Each card also carries its **selection control** (driving selective re-analysis), any amber **attention flag** raised by the quick check and retained only until that holding's next successful full pass, and its **analysis-vintage stamp** after a selective run.
The page renders all of this alongside the portfolio roll-up, and shows not-rated and insufficient-evidence positions with their reason ([interface.md](interface.md), [portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display)).
Above the holding cards a compact **sort bar** reorders the stack in place by overall value, dollar gain, percentage gain, or total cash invested — a display-only control over engine-computed position fields, defaulting to overall-value-descending ([portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display)).
The page also renders the latest standalone **Pull holdings** snapshot — the page body before any run exists; a stamped current-holdings section above the cards when fresher than the last run — with presence-only churn tags (*new · not in last analysis* / *no longer held*), never mutating or hiding the run-anchored cards ([portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display)).
While the job ran, the run tracker replaced the page (latest-run-only); on completion the page shows the persisted results.
A **run is never a report**: a cancel or failure removes nothing that was shown ([run-tracking.md](run-tracking.md)).
A tunnel-vision run persists complete or leaves no row; a corrupt persisted blob still lists as **unreadable** and opens to nothing ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).

## The quick check (engine-only)

**Type:** Computed (engine) + API retrieval (the per-holding price refresh, the run-level `DGS2` and `DGS10` prints, the conditional once-per-run FINRA short-interest file (as-built dormant — see the canonical recipe), and the per-asset-type evidence re-pulls — the full retrieval recipe is canonical in [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
**No model call, no web research, no Schwab call.**

A separate, cheap control that keeps the thesis ledgers live between full runs: it loads the **last run's holdings snapshot and ledgers** (no Schwab pull — it tests theses, not the book), refreshes prices, the `DGS2` and `DGS10` prints, and the per-asset-type evidence legs, evaluates every ledger's machine-checkable conditions under the shared persistence contract, re-derives the **total-return hurdle** (the v2 scenario multiples re-anchored on the fresh `DGS10` against the last full pass's stored percentiles and drivers — the canonical quick-path basis in [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)) and **scenario-band** reads on priced verdicts, and raises **attention flags** and quiet **evidence-event badges** — never rewriting any model-authored content.
The retrieval recipe, the four flag triggers, the evidence-event legs, and the evaluation-state carve-out are all specified once in [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only) (constants in [§Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).

It holds the **single global run slot** and streams per-holding rows to the run tracker like any job.
Because it makes **no model call**, it skips the daemon-connectivity check and runs even with the daemon configured-but-down — the same run-gate relaxation as ATO's Quick Audit ([trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture)); because it does **no web research**, it triggers no pre-run SearXNG notice.
The Schwab connection — and the shared FMP / FRED credential presence — remain presence preconditions, like everywhere in the suite, but no Schwab call is made.
A failed price refresh has no cache to fall to — the sweep never reads the shared price cache — so the holding's market family types `unknown` and the price-dependent reads skip, badging it in a selective run rather than passing silently ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
