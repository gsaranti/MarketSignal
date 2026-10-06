# Portfolio Analysis Workflow

Portfolio Analysis is one of the two local-suite jobs ([local-models.md](local-models.md)).
This document specifies its end-to-end control flow; the feature's design rationale — the verdict schema, the engine's three layers, the evidence floor, the roll-up — lives in [portfolio-analysis.md](portfolio-analysis.md).

The Portfolio Analysis job:
- pulls the user's Charles Schwab holdings (and live option chains)
- classifies each position by asset type and diffs it against the prior run
- computes a deterministic financial picture for every gradable holding
- researches each holding on the open web with a local reasoner (the bounded per-topic loop — Step 6c) and consolidates the research into one **analysis** (Step 6d)
- writes each gradable holding's **thesis document** and its typed layer — conviction and an expected share price at three months, twelve months and three years — beside the engine's grade and bands: the intrinsic verdict (an unpriceable fund class takes the `role_risk_only` branch, whose thesis document carries the role read and no prices — [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility))
- decides each holding's portfolio action from that holding's own verdict plus the investor profile — **tunnel vision by design**: the job never compares holdings, and the whole-book reconciliation belongs to the future portfolio-planner job ([portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action))

It runs **on demand only**, from a single **Run analysis** trigger that pulls holdings and runs the analysis in one user action (a separate **Pull holdings** control fetches and displays positions without analyzing; the job never reads it — [portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering)), entirely on local models, with **no cost at the model layer**.
With a card **selection** active, Run analysis becomes a **selective re-analysis** over **strictly those holdings** — the rest carry forward, badged where the quick check flags a change ([§Step 6](#step-6-per-holding-analysis-loop)); a third, **engine-only Quick check** control re-reads each priced holding's price against the engine's bands and re-derives the hurdle between full runs without any model call ([§The quick check (engine-only)](#the-quick-check-engine-only)).
A **single global run slot** serializes it against the report and Trade Opportunities (only one runs at a time).
For job states, the global run slot, cancellation, and error handling, see [scheduling.md](scheduling.md) and [run-tracking.md](run-tracking.md); for the failure posture (per-holding checkpoint/resume, fail-soft research), see [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).

## How to read this workflow

Every step below is tagged with a **Type** so it is obvious what the step actually does:

- **Computed (app layer)** — deterministic Rust logic, with no model and no external network: local SQLite and filesystem reads, the holdings diff, and the **financial-analysis engine** (every sub-score, target, and derived read).
- **API retrieval** — fetches from external sources: holdings and option chains from **Charles Schwab** (account-scoped, via OAuth — see [schwab-integration.md](schwab-integration.md)); company data from **FMP / SEC EDGAR**; run-level macro and positioning from **FRED / CFTC**; and the **web tool** (SearXNG-only) the orchestrator runs *on a model's behalf*.
  The full per-source endpoint surface, with each call's per-holding / per-fund / run-level cardinality, is in [data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface).
- **Local-model call** — invokes a model on the app-supervised **Ollama** daemon ([local-models.md §Serving runtime](local-models.md#serving-runtime)): the primary reasoner **`Qwen3.5-122B-A10B`** in **thinking** mode (research, the write-ups, the analysis, the review, the thesis document and the action call) or **non-thinking** mode (distilling write-ups, and transcribing the thesis document's stated values into the typed appendix).
  Two calls return a typed object and are **schema-constrained** via Ollama's native `format` parameter, then app-checked on type: the typed appendix and the action decision; the Step-6c gathering turns carry the tool-call schema and no grammar; every other call — the synthesis write-up, a distillation, the analysis, the review, the thesis document — returns prose with no grammar and no validation.
  **Mode caveat:** Ollama bug #14645 is verified fixed on the pinned version, so the typed appendix — the one `format`-carrying call that runs `think: false` — issues directly; an Ollama version bump **re-locks** it until the schema-integrity check passes on the new version, the appendix riding thinking-enabled until it does (the rule is canonical in [local-model-operations.md](local-model-operations.md) §Structured output × thinking).

Two load-bearing architectural rules frame the whole table, the same ones the report pipeline holds: **agents are pure stages, and the application layer owns all I/O** — a model stage consumes the structured input handed to it and emits its document or typed object; when a research stage needs the web it *requests* a tool call and the orchestrator performs the fetch.
And **the engine computes every deterministic number** — metrics, sub-scores, tiers, scenario bands ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)) — while the model authors its **own arm** of the verdict: the thesis document with its conviction and expected share prices, and the action call's rung, unrestricted and clearly typed as model-authored; the engine's values stay the incorruptible baseline — the model arm never alters or binds it (the boundary statement at [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)) — the model's are displayed and scored beside them, with engine evidence annotating departures, never enforcing them.
For each model stage, the **Local-model call** block lists what the prompt includes and what the model returns.
Per-step progress, per-request rows, and token/reasoning output stream to the run tracker over the shared `progress` seam ([run-tracking.md](run-tracking.md)), exactly as a report run does.

| Step | Stage | Type | Model |
|---|---|---|---|
| 1 | Job start & gate | Computed | — |
| 2 | Load holdings (option chains fetch per holding at 6a) | API retrieval (Schwab) + Computed | — |
| 3 | Classify asset eligibility | Computed | — |
| 4 | Holdings change diff | Computed | — |
| 5 | Load shared context (house view, profile, run-level FRED rates, commodity context, CFTC positioning, CBOE backdrop, FINRA short interest, sector benchmarks), then the accuracy checks over the episode store | Computed (local read + engine) + API retrieval (FRED / FMP / CFTC / CBOE / FINRA; dated-EOD closes on horizon dates) | — |
| 6 | **Per-holding analysis loop** (per eligible holding; each completed holding checkpoints — a resume skips it) | mixed — see 6a–6g | 122B |
| 6a | Dossier assembly | API retrieval + Computed | — |
| 6b | Deterministic financial analysis | Computed (engine) | — |
| 6c | Bounded web research: per-topic gathering, then a synthesis write-up per pass (+ conditional technology-event topic) | Local-model call (thinking) + API retrieval (web tool), looped | Qwen3.5-122B · thinking |
| 6d | Consolidation: budget check, distillation of the write-ups where over budget, then the **analysis** | Local-model call(s): distillation non-thinking, the analysis thinking | Qwen3.5-122B · non-thinking (35B optional) / thinking |
| 6e | Self-review (continuity runs only) | Local-model call (thinking) | Qwen3.5-122B · thinking |
| 6f | Interpretation (the thesis document, then the typed appendix) and the **action decision** (the investor profile enters at the action call only) | Local-model call (thinking) ×2 + (non-thinking) ×1 | Qwen3.5-122B · thinking / non-thinking |
| 6g | Checkpoint | Computed | — |
| — | Open episodes (after the loop; persists with Step 7) | Computed (engine) | — |
| 7 | Persist run & audit | Computed (persist) | — |
| 8 | Render Portfolio page & update UI | Computed (frontend) | — |

## Step 1: Job Start and Gate

**Type:** Computed (app layer) — the local-suite execution gate.
No model and no external API (credential and daemon *presence/reachability* are checked, not analysis).

The job will not start unless four preconditions hold:
- the **single global run slot** is free (no report or other local job is running — see [scheduling.md §Concurrent Job Protection](scheduling.md#concurrent-job-protection));
- the **local-model daemon is reachable and the configured roster is present** (the 122B reasoner — neither local job makes an embedding call) — health-checked at the Ollama endpoint ([local-models.md §Serving runtime](local-models.md#serving-runtime));
- a **connected Schwab account** with a valid (≤7-day) refresh token ([schwab-integration.md §A connected Schwab account is required](schwab-integration.md#a-connected-schwab-account-is-required));
- the **shared FMP and FRED credentials are present** ([configuration.md §External Data Provider Credentials](configuration.md#external-data-provider-credentials)) — the per-holding fundamentals surface (FMP) and the run-level rate anchors (FRED `DGS10` / `DGS2`) are load-bearing engine inputs, so a missing key blocks at the gate rather than failing hours into a run; the check is presence-only (no live probe), surfaced through the **existing missing-provider-credentials warning category** — no new category — while **Tavily deliberately does not gate** the local suite — its web tool is SearXNG-only ([web-research.md §Tavily fallback](web-research.md#tavily-fallback)).
  (As-built with the fund slice: the shipped gate (`check_local_configuration`) carries the FMP / FRED presence check through the shared missing-provider-credentials category.)

This gate is **independent of the cloud-report gate** — a machine with no OpenAI/Anthropic keys can still run the local suite.
Missing **configuration** (the Ollama endpoint or the reasoner id unset, Schwab not connected / refresh token lapsed, or the FMP / FRED credential missing) is a presence check that locks the affected job's Run button and shows a persistent warning *before* this step is reached — **local models not configured**, **Schwab connection**, and the shared **missing provider credentials**, one per category, no duplicates (see [interface.md §Connection status](interface.md#connection-status-local-suite)).
A live **local-model connectivity** failure caught here at the run-gate (daemon unreachable, a rostered model not pulled) blocks the attempt **inline**, not as a persistent warning; Schwab *API* reachability is **not** tested at this step — there is no external API call here, so a Schwab outage surfaces at the Step-2 holdings fetch, not the run-gate.
As-built the daemon health-check runs **before the slot is claimed** (a local-only call), and every external fetch — the SEC ticker map the CIK resolver loads included — happens **inside** the slot (ruled 2026-08-18).
Manual-import holdings do **not** satisfy the Schwab gate.

## Step 2: Load Holdings

**Type:** API retrieval (Schwab) + Computed (snapshot assembly — the holdings-normalization step).
No model.

Holdings are **fetched fresh at job start** — the Run-analysis trigger pulls them as its first retrieval, never reusing a standalone **Pull holdings** snapshot (that control is view-only and invisible to the job; the diff baseline below is likewise always the prior *run's* snapshot, [portfolio-analysis.md §Holdings change tracking](portfolio-analysis.md#holdings-change-tracking)); the run's snapshot persists with the run, so the portfolio stays viewable without re-fetching.
A **resumed** run performs no holdings pull — it reopens its interrupted run's pinned snapshot, while the per-holding chain fetches below still run live at each resumed holding's own Step 6a ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).
Each position carries instrument identity (symbol, description, asset type — no CUSIP is mapped off the wire), quantity, cost basis (the **signed account-currency total** the app derives from Schwab's per-unit `averagePrice` — [schwab-integration.md §What is pulled](schwab-integration.md#what-is-pulled)), and market value (P/L is derived downstream as market value − cost basis, not a pulled field), from `GET /trader/v1/accounts/{accountHash}?fields=positions` (Schwab identifies accounts by a hashed number; the app resolves plaintext→hash first).
The same payload's `currentBalances` supplies per-account cash and liquidation value; snapshot assembly reconciles any explicit cash-equivalent / currency row against those two fields, folds it into cash exactly once, and retains the raw row only on the audit surface.
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
  The further **strategy classification** (asset class from `etf/info`) is a **loop-time routing decision, not made here** — this step is computed-only and `etf/info` is not retrieved until Step 6a — so each fund is classified and routed at 6a/6b once its metadata is in hand: equity funds take the exposure-valuation path (US-exposure-guarded: below ~70% US by country weightings the composite is not an honest read, so the fund is unpriceable); a missing class may fall back to sector-weight evidence, but an explicit unsupported allocation / multi-asset class never does; bond / commodity funds take a further-reduced path with valuation recorded as a gap; leveraged / inverse and option-overlay vehicles route to role/risk only and carry a typed structurally-path-dependent cause; a CEF adds the price-vs-NAV read as a structure marker orthogonal to the class — built; [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility) is canonical for the detection and consumption.
  Every unpriceable class returns the typed **`role_risk_only`** branch — no engine letter, no bands, no expected prices; its thesis document carries the role read, and the portfolio action machinery still applies ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
- **Options, fixed income, cash, unsupported types — and net-short equities** — marked **not rated**, with a reason, excluded from grading (a short's signed exposure still feeds the roll-up, and a carried long-side verdict now sitting net-short is marked `side_reversed` and badged in a selective run — [portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
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

**Type:** Computed (local read — house view, investor profile; the engine's accuracy checks) + API retrieval (run-level FRED rates, the FRED/FMP commodity context, CFTC positioning, the CBOE put/call backdrop, the FINRA short-interest file, the sector-benchmark series, and the dated-EOD closes the accuracy checks read).
No model.

Three things are loaded **once per run and shared across every holding**, not re-requested per symbol:
- the **Market Signal house view** — the latest report's Thesis, Investment Strategy, and Forward Outlook sections plus recent report stances (`created_at`, `thesis_stance`, `risk_posture`), loaded **deterministically** from the report store (retrieve-don't-dump — never by vector-searching the report's memory; see [local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)).
  The report's **creation date** rides into the dossier so every downstream stage knows how old the thesis is, and a **freshness window applies**: if the latest report is older than **one week** (a pinned default), the house view is **omitted and recorded as a gap** rather than fed as current — a month-old thesis is not today's, and the data-honesty stance treats a stale input as absent, not current (the same posture the report takes on a stale data series).
  The window counts **whole ET session days on both sides** — the run's own session against the report's, each converted from its stored UTC instant, never a date prefix ([data-sources.md](data-sources.md) — the cross-cutting session-dating rule in its intro).
  Both legs must convert together: the two instants straddle the ~8 PM ET rollover, so a prefix read made an evening run see a 7-ET-day-old report as eight days old and drop the whole view, while converting only the run's side would read a report written after the rollover a day younger than it is.
  The holding is still graded on its fundamentals and research; it simply carries no house-view anchor that run;
- the **investor profile** (risk tolerance, horizon, objective, tax sensitivity — see [configuration.md](configuration.md)) — reaching the model **only at the per-holding action call** (Step 6f's action call), so the intrinsic verdict stays profile-independent by input isolation ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict));
- run-level market context — the **risk-free rates** (FRED `DGS10` / `DGS2`): `DGS10` anchors the engine's scenario-target function, the v2 rate-anchored multiple, and `DGS2` the capital-efficiency hurdle, the suite's short-end anchor mirroring Trade Opportunities' entry-threshold anchor ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
  The full run additionally loads the **anchor-window `DGS10` history** the v2 percentiles join against — one date-ranged request per run, retained as dated observations, the acquisition rule on the DGS10 row ([data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface)).
  A rate retrieval still failing after the shared bounded retries **hard-fails the run here, before any per-holding work** — the canonical rate-anchor rule, [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).
  The context also carries **cyclical commodity prices** for commodity-linked holdings (FRED daily oil / gas plus the suite-shared monthly IMF metals — [data-sources.md §Trade Opportunities — endpoint surface](data-sources.md#trade-opportunities--endpoint-surface) — and gold via FMP `quote` `GCUSD`), with industry overriding a misleading broad sector: uranium producers receive the uranium print, while coal producers receive no fabricated oil / gas proxy because the catalog carries no coal print; the **CBOE daily put/call statistics** are an optional, fail-soft **venue-level options-sentiment backdrop** — broad-market context, never a per-name signal ([data-sources.md §CBOE](data-sources.md#cboe)); the **sector benchmark series** (FMP dated EOD — the identity table in [data-sources.md §Financial Modeling Prep](data-sources.md#financial-modeling-prep)) feeds the technology-event pre-flag (fetched per carried, non-guard-terminal holding's sector and memoized); and **CFTC Commitments-of-Traders positioning** maps a commodity / macro **fund** holding onto a bellwether contract.
  It also carries the **FINRA consolidated short-interest file** — fetched once per run, each held stock reading it as a local lookup at dossier assembly ([data-sources.md §FINRA](data-sources.md#finra-short-interest)).
  Every one of these enriching loads is fail-soft to a typed gap counted on the run's data-health line, never an attention trigger and never a run failure ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).

Then, before any per-holding work, the deterministic **accuracy checks** run over the **episode store** ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)).
An episode is a price record: a holding's expected share price at three months, twelve months and three years, the engine's base value at the same horizons, the creation date, the spot that day and the anchor close that bridges the record across a later split ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
The checks visit every episode in the store with a horizon that has passed and is not yet checked — held or exited, since the record measures the forecast, not the book.
The dated-EOD series is refreshed through the shared price-bar cache for every symbol with a due horizon, an exited name and an unselected carried holding alike.
Each check reads the close on the horizon date, or the last session at or before it within a drafted few-session proximity ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)), bridges the episode's prices across any split since creation, and writes the check onto the episode — date, horizon, expected, actual and the score, for the model's price and for the engine's — the per-check score one hundred times one minus the absolute error over the actual, floored at zero.
A horizon whose series the run could not refresh stays pending for a later run; one whose refreshed series serves no close inside that proximity is written unscorable at once and leaves the due set, the symbol spending no further pull ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)).
A horizon whose episode carries a null model price scores the engine leg alone, the model's accuracy score at that horizon counting no such check.
From the checks the pass derives each holding's **accuracy scores**: at each horizon, the mean per-check score over the holding's checks, for the model and for the engine separately, 0 to 100, "no score yet" before the first check; the formula and the aggregation are drafted constants ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
The checks run here rather than after the loop so that this run's self-reviews read every check that has come due ([§Step 6e](#step-6e-self-review)); they land in the store as they are written, and the due query excludes a written horizon, so a resumed run never re-scores one.
Opening this run's episodes waits for the loop, since it needs this run's expected prices ([§Step 7](#step-7-open-episodes-then-persist-run-and-audit)).

## Step 6: Per-Holding Analysis Loop

Each **gradable** holding (stock or fund, from Step 3) is processed through the chain below.
Holdings are independent, so the loop **checkpoints per holding** — each completed holding persists so a cancellation or a crash resumes the unfinished holdings rather than restarting the (potentially hours-long) run, resume reopening the run's pinned snapshot and versions as its own entry path, never a fresh holdings pull (the contract and entry path are canonical at [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)) — and fetched pages are cached in the shared document cache ([storage.md §Local Analysis Suite Storage](storage.md#local-analysis-suite-storage)).
A single per-holding model failure does not interrupt the run: it is isolated into a failed card and the run continues, so the resumable trail marks a cancellation, a crash, or a run-level failure rather than one holding's failed analysis ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).
The resident **122B reasoner fills every model role in this loop** by switching mode, so moving a holding across its research and synthesis conversations (thinking), any distillation of its write-ups (non-thinking), its analysis, review and thesis document (thinking), its typed appendix (non-thinking) and its action call (thinking) pays no model-swap cost ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).
A **fund** holding runs the reduced engine path (Step 6b) and a **fund-flavored research agenda** (Step 6c); the loop's structure — research, consolidation, review, interpretation, action — is otherwise identical.
Sub-steps 6a–6g are the [portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline) stages, the self-review (6e) running on a continuity run only.

In a **selective re-analysis** (Run analysis with a selection), the **work-list is strictly the selected holdings** — nothing else is pulled in.
A selective request with no readable prior run to carry from runs the whole book instead — the no-prior-run rule is specified once in [portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering).
Before the loop the quick-check evaluation still runs over the unselected carried tail, but only to **badge** it, not to expand the work-list: a holding it flags, one whose check result is **`unknown`** (a required signal family's retrieval failed — the degraded-check rule, [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)), one carrying an unexamined evidence event, and one whose carried long-side verdict now sits net-short (marked `side_reversed`) each ride the card as a non-blocking badge ([portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering)).
Holdings left outside the selection carry their prior verdict — the thesis document, the typed layer and the action — forward, **vintage-stamped**, into the persisted run; a held position with **no prior verdict to carry** (new, or never analyzed) is left **not analyzed** — no verdict this run, rendered as a "run to grade" placeholder card, selectable for the next selective run.
Steps 1–5 run whole-book regardless (the pull, eligibility, diff, and shared context are cheap), so the diff baseline and snapshot semantics are unchanged.
The badges — not force-includes — are what keep mixed vintages safe, specified once in [portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering); the one deterministic carry rule is the over-age **add-family** demotion to *hold* (stamped `action_source: rule-demoted`), while an over-age exit or hold stands behind the stale-vintage badge.
Within the loop, research effort is **uniform, not graduated**: every analyzed holding runs 6c–6d in full each run.
On a continuity run the holding's **prior analysis** and **prior thesis document** ride every topic's gathering brief as the shared leading block, whatever their age, dated; the analysis is the holding's whole research memory, and no earlier write-up is ever rendered into a later prompt ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).

### Step 6a: Dossier Assembly

**Type:** API retrieval (FMP / SEC EDGAR / Schwab / FINRA lookup) + Computed (assemble the packet).
No model.

The step opens with the **listing-resolution guard's** per-stock profile read (the FMP `profile` identity — issuer name, exchange, sector, and industry; one fetch also supplying the outcome episodes' entry-stamped sector identity): a guard-terminal outcome (unsupported listing / conflicting identity) **skips the rest of this step's per-symbol retrieval**, including its carried-sector benchmark, and routes the holding out of the loop — not-rated or insufficient-evidence — before the 6b engine stage, while an unverifiable guard proceeds as a recorded degraded input ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
The application builds the holding's evidence packet deterministically, starting from the position + its Step-4 delta.
It adds any **same-underlying option positions** from the Step-2 pull — the deterministic OCC symbol decode is the link — as a **typed overlay** (direction / quantity / strike / expiry / delta / coverage ratio, classified covered-call / protective-put / collar / other), **built** ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline) Step 1).
Per-leg delta comes from a **targeted chain fetch per distinct held strike**, scoped to the held contracts' expiry window so the options-activity signal's bounded NTM query is never widened; a failed fetch leaves that delta a typed gap, and the net share-equivalent delta computes whole or not at all.
An option row whose symbol does not decode to the holding's root never links into the overlay at all — fail-safe absence ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline) Step 1).
A **standalone** option (no held underlier) is a not-rated position that joins no overlay and gets no chain fetch — its delta is simply absent, never a recorded gap, since its class never requests one (the canonical statement is the chains row in [data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface)).
It adds the symbol-scoped **`news/stock`** headlines as the holding's **news leads** (leads, never evidence — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).
For a stock it adds the **equity** per-symbol surface: as-built the FMP fundamentals — quarterly statements, the forward-estimates consensus, dividends, quote + EOD — joined with SEC EDGAR as a **fill-only merge**, plus the **item-classified 8-K filings sweep** (the hard-forensic filing kinds' producer — an unresolved CIK or failed fetch types the sweep `unknown`, never a fabricated clear; [data-sources.md §SEC EDGAR](data-sources.md#sec-edgar)) and the **FINRA short-interest lookup** off the once-per-run consolidated file; the revenue segments, the rating/surprise signals, and a conflicting-value SEC cross-check are designed, landing with their data legs ([data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface)); **13F institutional, earnings-call transcripts, and per-symbol M&A are off-plan** → SEC EDGAR / the web-research loop / `mergers-acquisitions-latest`+8-K ([data-sources.md §FMP — current paid-plan tier audit](data-sources.md#fmp--current-paid-plan-tier-audit)).
For a fund it adds the **reduced ETF surface** instead: `etf/info` + sector/country weightings, the one-per-fund `profile` read the closed-end detection consumes, plus the **sector-P/E surface** the 6b exposure-priced valuation reads — `sector-pe-snapshot` / `historical-sector-pe`, fetched **on first need and memoized across funds**.
The snapshot is fetched once per exchange per candidate session (the run's ET session date, then earlier weekdays until a candidate serves **both** NYSE and NASDAQ legs; a partial candidate walks back, and an exhausted walk records the typed gap on **every** fund rather than pricing them on one board or abstaining them as a sector-overlap failure), and the historical series per sector × exchange as each fund's retrieved weightings introduce sectors, so a later fund's new sector still gets its trailing history — the sector set can't precede this step ([data-sources.md §Portfolio Analysis — endpoint surface](data-sources.md#portfolio-analysis--endpoint-surface)); constituent `etf/holdings` and mutual-fund `funds/disclosure*` are off-plan.
Each sector history likewise enters only when both exchange legs served at least one row; its memoized failure reason reaches every fund whose weightings use that sector, including funds after the one whose turn issued the request.
It adds deep price history (FMP dated EOD) and a live quote (FMP `quote`); on a continuity run the prior run's verdict for this holding — its action and rationale, its conviction and its expected prices with their horizon dates, and the prior **thesis document** — together with the prior run's **analysis** and the holding's accuracy scores ([§Step 5](#step-5-load-shared-context)); and the Step-5 shared context.
The full input list and every endpoint is in [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) and [data-sources.md](data-sources.md#portfolio-analysis--endpoint-surface).

### Step 6b: Deterministic Financial Analysis

**Type:** Computed (the financial-analysis engine, shared with Trade Opportunities).
No model.

The engine computes the holding's quantitative picture in **three layers**.
**(a)** The grade core → for a stock, the quality / valuation / risk sub-scores the letter rolls up from; for a priced equity fund, real valuation / risk plus the neutral-imputed absent quality axis defined by the fund-grade contract ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)) (momentum computed alongside, outside the letter — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)) — and the scenario price **bands at three months, twelve months and three years**: the twelve-month band from the **v2 rate-anchored scenario-target function** off the run-level `DGS10`, the three-month band its near-horizon form, and the three-year band a declared extrapolation of consensus earnings at consensus growth to year three at an engine-derived multiple — each a bear / base / bull triple, the functions and their stamp canonical at [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable).
**(b)** A conviction layer → the **momentum / market-setup read**, the **hard forensic state** (the item-classified EDGAR filing kinds alone — [data-sources.md §SEC EDGAR](data-sources.md#sec-edgar)), the **narrative-vs-reality ratio** (with its thin-coverage fallback), and the **implied-expectations read** (the shared Step-5c primitive); the soft **forensic flags** are designed, landing with their data leg ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)) — all kept *out* of the letter and rendered to the model as evidence.
**(c)** Positioning context (as-built the Step-2 **options-activity signal** and **FINRA short interest**; designed — insider / congressional; FMP 13F off-plan → EDGAR/omit), held out of the sub-scores.
For a priced stock meeting the deterministic eligibility rule, layer (b) additionally computes the statement-derived legs of the **pre-profit execution / financing overlay** ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
Eligibility is `TTM operating income ≤ 0`, or `no positive forward-EPS consensus AND TTM free cash flow < 0`.
From the comparable quarterly statements the engine computes liquid resources, TTM cash burn and runway months, TTM capex intensity, split-adjusted year-over-year diluted-share change, the latest two-quarter average gross margin, and its change from the preceding two-quarter average.
It derives the financing state and the statement-derived economics / dilution legs, typing every missing input `unscorable`; funds and `role_risk_only` holdings skip the overlay.
The overlay's execution read — guidance against delivered results — has no deterministic producer: the issuer's operating observations reach the model through the research write-ups alone, so the execution leg types `unscorable` and enters no conjunction, while the financing, economics and dilution legs stand as rendered evidence — severe deterioration reading from them alone — and the constrained-runway **add-family bar** and the severe-deterioration exit restriction bind the engine's action rung.
This stage also **assigns and persists the holding's risk tier**, per branch — the deterministic assignment rule of [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable) (a `role_risk_only` holding carries none) — before anything downstream consumes it.
From the bands the engine also derives a **capital-efficiency / dead-money read** (total-return basis, DGS2-anchored, scaled by the tier just assigned, three-state — only *fails* is dead money; [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)), kept out of the sub-scores like layers (b)/(c) and fed to the action call's evidence.
The engine's own **action rung** follows from its reads by the drafted rung rule ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)): a hard-forensic trip reads the exit family, and it observes the constrained-runway add-family bar and the severe-deterioration exit restriction, and it reaches the action call as the computed read beside the model's verdict.
The three-layer design is in [portfolio-analysis.md](portfolio-analysis.md#the-per-holding-pipeline) Step 2.
For a **fund**, this step runs the reduced computation instead — routed by the **strategy classification made at loop time** from the 6a `etf/info` pull (Step 3's eligibility used only Schwab instrument identity) — the reduced fund computation; an **unpriceable class** computes what it honestly can and returns the typed **`role_risk_only`** branch ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)).
On a continuity run the engine also computes the holding's **realized data** for the self-review (Step 6e): the price now and its move since the prior run; the prior run's every stored metric, sub-score, grade, band, tier and hurdle-state value beside this run's, resolved at exact `old ≠ new` and rendered at enough shared precision that a real move never reads as equality; the holding's accuracy scores, the realized close and score for each of the prior position's own expected prices whose horizon has passed, and the individual checks written since the prior analysis was written — this run's own checks included, since the checks precede the loop — newest by horizon date up to a drafted count ([§Step 5](#step-5-load-shared-context); the count in [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)) — never the whole store; and a grade- or scenario-target parameter boundary named for what it changed, absent where it changed nothing for the holding ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
The prior expected prices, the prior spot, the episodes' recorded prices and every prior raw fiscal-period consensus-EPS row are first converted across the **split-adjustment bridge** where the price series was retroactively re-based since the prior pass, so every comparison and every rendered typed value sits on one price basis, each bridged value labeled as bridged ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable) is canonical).
A document rendered verbatim — the prior analysis and the prior thesis document — is never rewritten: where a split intervened since it was written, a **split-context line** above it states the split's date and ratio and the factor that brings its prices to today's basis, so the model reconciles the prose against the bridged numbers without the app touching the text; every step that renders such a document carries that line.
The **technology-event pre-flag** is evaluated against the Step-5 sector-benchmark series ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)); an unevaluable pre-flag records its typed reason on the audit, never a fired or clear flag.
This stage ends with the **evidence-floor check** — deterministic, over the floor-bearing inputs now all in hand: a below-floor holding **exits the loop here** with the typed **`insufficient-evidence`** disposition and named gap reasons, **checkpointed as completed** so a resumed run does not redo it ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)) — Steps 6c–6f never run for it, its prior documents and any attention flag are retained, and the full exit-state semantics (roll-up contribution, no per-holding action, no new outcome episode) are specified once in [portfolio-analysis.md §Evidence floor](portfolio-analysis.md#evidence-floor).

### Step 6c: Bounded Web Research

**Type:** Local-model call (122B, thinking) + API retrieval (the web tool), **looped**.
This is the only **per-holding** stage that loops.
The orchestrator-assembled agenda, the per-topic pass loop over the SearXNG-only web tool, and the budget machinery below all run live; a construction without a web stack (the demo, offline tests) degrades to the recorded research-unavailable gap, never a failed run.

The orchestrator assembles the holding's **agenda** deterministically — the reasoner works it, never authors it ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).
The agenda is competitive position, recent results/estimate revisions, catalysts/risks, **management quality & capital allocation**, market narrative & sentiment, and forward opportunity & thematic fit.
**Conditionally** it adds a technology-event impact assessment that reads the actual technology and sizes the holding's real exposure; the topic is eligible on the engine's Step-6b **event pre-flag** alone, decided when the agenda is assembled, and stays dormant otherwise — a follow-up question cannot activate it, so an event found only mid-loop may lack a dedicated technology assessment.
The news leads are no trigger of their own; they ride the gathering brief as fetch candidates ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)).
**For an overlay-eligible stock** it adds a pre-profit execution / financing topic over the actual operating proof the issuer reports ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)).
A pre-profit topic asks for comparable, dated issuer observations on production / deliveries where applicable, bookings / backlog / reservations, guidance ranges and matching actuals, unit economics, gross-margin commentary, cash needs, capital spending, and issued or planned financing; the write-up reports what it finds with each figure's date and source, and the engine computes no attainment, runway or dilution from it.
The orchestrator works one pass at a time, with **all eligible roots before pending follow-ups**, then follow-ups in topic-priority order through the depth cap — **each topic a separate isolated conversation and research loop**, a bounded multi-turn pass loop ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)) — run over a **clean context**: the holding-constant block plus that topic's own text, with no other topic's write-up fed in.
A **fund** holding's agenda swaps the company-centric topics for fund-flavored ones — mandate / strategy and manager changes, expense and structure vs its category, the exposure profile it actually supplies and what direct or lower-cost vehicles supply the same exposure, and (CEF) the discount and distribution coverage; the technology-event topic is equity-only ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)).
The fit of that exposure against the house view is the thesis document's judgment, which sees the market analysis this loop never does.
The orchestrator — not the model — owns every request: per-topic depth ≤2 (≤3 passes/topic), at most 8 tool calls from one gathering turn, an aggregate gathering-history input guard checked before every model request and retained tool result, and a **per-item fetch + wall-clock budget that binds first**, spent in root-before-follow-up and then topic-priority order, fail-soft on exhaustion.
A batch over that bound has its head executed and its tail omitted.
An over-large batch or the gathering-history bound ends gathering, records the omitted calls or results as partial coverage, and still takes the synthesis conversation over every page that landed.
Failed-fetch memory spans all holdings of the invocation, and each automatic retry spends a separate attempt under the same holding budget; remembered failures spend none ([web-research.md §Failed fetch memory and bounded retry](web-research.md#failed-fetch-memory-and-bounded-retry)).
The **disconfirming-fetch pass** ([web-research.md §Source quality and evidence weighting](web-research.md#source-quality-and-evidence-weighting)) runs **once per holding, after its topics** — one gathering conversation searching for evidence against the run's write-ups so far, then its own synthesis conversation writing its own write-up and asking no follow-up — spent from the holding's fetch + wall-clock budget, not counted against any topic's ≤3-pass depth, and fail-softing to a recorded gap when the budget is exhausted (this is the canonical placement).
Grounded by the fetched values so research fills the gaps the numbers don't.
The full loop and its bounds are in [web-research.md](web-research.md).

#### Local-model call — Per-holding research (Qwen3.5-122B, thinking)

**Model.**
The resident 122B reasoner in thinking mode, requesting `web_search` / `web_fetch` tool calls the orchestrator executes (SearXNG-only; SSRF-guarded; untrusted page text inserted as quoted evidence, never as instructions — see [web-research.md §Safety and provenance](web-research.md#safety-and-provenance)).
**Topics do not share a conversation or write-ups** — each is worked in isolation as its own bounded multi-turn pass loop, and each pass's write-up is authored by a **separate synthesis conversation** over a fresh conversation carrying the gathered evidence and no tools, so a tool-using gathering turn and the write-up never share one request ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).
The synthesis packet jointly selects headers and bodies under the shared input guard, reclaiming every omitted source's header space before water-filling the selected pages; a selected address is therefore always accompanied by usable body evidence, and every omitted or truncated source is gap-recorded into both the per-holding research audit and the run-level data-health counts.

**Prompt — input (gathering).**
The initial brief is one message in two parts on the frame every Portfolio prompt shares ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).
Part 1 leads with the holding-constant block, so consecutive topic conversations on one holding share it: the holding header with the analysis date; FETCHED VALUES; NEWS LEADS; on a continuity run PRIOR ANALYSIS and PRIOR THESIS, the prior run's analysis and thesis document verbatim, each with its date and, where a split intervened, its split-context line ([§Step 6b](#step-6b-deterministic-financial-analysis)); and PAGES ALREADY RETRIEVED, the bounded holding-scoped reuse block.
The disconfirming pass's brief carries no PAGES ALREADY RETRIEVED block ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).
The topic's own text follows: TOPIC, the topic's questions; on a follow-up pass FOLLOW-UP, the question it pursues, and WRITE-UP SO FAR, the topic's write-up so far; on the disconfirming pass WRITE-UPS SO FAR, the run's write-ups.
**FETCHED VALUES** is the holding's fetched data as the providers return it, glossed once and never engine-computed — the TTM basis and the split bridge are computations and stay out.
For a stock: the profile line (name, exchange, sector, industry); the quarterly statements' headline lines — revenue, operating income, net income, diluted EPS, operating cash flow, capital expenditure, cash, total debt, diluted shares — for the latest eight quarters as reported; the forward consensus for the next two fiscal years; the last four dividends; the quote with its 52-week range, and the closes on the prior run's date and three, twelve and thirty-six months back; the 8-K filings of the trailing twelve months by date and item; and the latest short-interest print.
For a fund: the `etf/info` line (asset class, expense ratio, AUM), the sector and country weightings, the NAV and price, and the profile line.
For both: the `DGS10` and `DGS2` prints.
The same block, byte for byte, leads the synthesis message and the thesis-document message ([§Step 6f](#step-6f-interpretation-and-action)), so the three share one rendering.
Before each gathering request the app appends a short user message stating the replies remaining in this pass; the brief and every previously issued message stay unchanged.
Part 2: what to find, how to weigh a source, the per-reply tool-call bound, and when to stop; where pages are shown it asks the model to read them before searching for the rest.
The order, the reuse block, the countdown, the tool results' register and the fixed failure sentences are canonical at [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management).

**Prompt — input (synthesis).**
A fresh conversation, thinking on, no tools, no grammar.
Its first message leads with the holding header and FETCHED VALUES, then EVIDENCE — every page the pass's gathering conversation carried, the reused pages first, each under the header `web_fetch` gave it — then the topic's own text: TOPIC; on a follow-up pass FOLLOW-UP and WRITE-UP SO FAR; on the disconfirming pass WRITE-UPS SO FAR.
Part 2 asks for the write-up: what the research established on the topic's questions, each figure with the date or period its source gives for it and that source — the page's address, or FETCHED VALUES where the figure comes from there — where pages disagree, and what the evidence leaves unanswered, within the write-up's length band ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
On a follow-up pass it asks for the topic's write-up rewritten whole with the new evidence folded in.
Its second message asks whether the research has a follow-up question, skipped on the topic's last pass under the depth cap and on the disconfirming pass.

**Returns.**
The gathering conversation returns tool calls, executed by the orchestrator, and ends on a reply with no tool call or at a bound.
The synthesis conversation returns the pass's **write-up** as prose — read as text, validated by nothing — and, where asked, the follow-up question verbatim or the one word `none`, the only reply the app interprets; a question becomes the next pass's question under the depth cap and the budget.
A pass that retrieved no page spends no synthesis conversation and leaves the topic's write-up so far standing.
A topic ends with one write-up; the holding ends with one per worked topic plus the disconfirming pass's, and they flow whole to consolidation.
Every page shown to the model enters the holding's **page roster** — address, title, publication date, retrieval time and source tier — persisted on the audit record ([§Step 6g](#step-6g-checkpoint)).

### Step 6d: Consolidation

**Type:** Computed (the budget check) + Local-model call(s): the distillation of the write-ups where the analysis prompt is over budget (122B non-thinking; the optional 35B fast tier if resident), then the **analysis** (122B thinking).

The orchestrator first sizes the analysis prompt — the write-ups, FETCHED VALUES and, on a continuity run, the prior analysis — against the call's input budget ([configuration.md §Research Context Management](configuration.md#research-context-management)).
Within budget, the write-ups go in as written.
Over it, the merged write-ups are distilled into one shorter document; where the merged write-ups themselves exceed one distillation call's budget, each write-up is distilled first and the merge of those outputs is distilled again.
The prior analysis is never distilled.
The shape is chosen deterministically from size, never by the model, logged to the audit record with its call count, and sized once more at issue by the adapter seam ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam); the shapes are canonical at [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).

#### Local-model call(s) — Distillation (Qwen3.5-122B, non-thinking)

**Model.**
The resident 122B in non-thinking mode by default (no model-swap cost); the fast 35B tier is a benchmark-gated option ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).
A distillation whose rendered prompt outgrows the fast tier's budget issues on the resident reasoner instead, and one over the widest budget is refused before issue ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)).

**Prompt — input.**
One message in two parts: the holding header, then the write-ups to distill — the merged write-ups, or one write-up on a per-write-up call — each under its topic's heading; then the task: a shorter document that keeps every dated figure with its source, every disagreement between pages and every open question, within the distillation's length band ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
Consolidation, not new reasoning.

**Returns.**
Prose, read as text and validated by nothing; a length stop at the normal reservation takes one re-attempt at the wider ceiling ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)).

#### Local-model call — The analysis (Qwen3.5-122B, thinking)

**Model.**
The resident 122B in thinking mode; no grammar.

**Prompt — input.**
One message in two parts.
Part 1, in page order: the holding header with the analysis date; FETCHED VALUES; on a continuity run PRIOR ANALYSIS, the prior run's analysis verbatim with its date and any split-context line ([§Step 6b](#step-6b-deterministic-financial-analysis)); then WRITE-UPS, this run's write-ups — or their distillate — each under its topic's heading, the disconfirming pass's last.
Part 2 asks for the **analysis**: one document consolidating what this run's research established on the holding, each dated figure with its source, where sources disagree, what stays unanswered, and on a continuity run what the prior analysis said that this run's research confirms, revises or leaves untouched; a topic with no write-up this run keeps what the prior analysis says about it — the budget exhausted before the topic, the technology topic not triggered, the pre-profit topic lapsed — within the analysis's length band ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).

**Returns.**
The holding's **analysis** as prose, read as text and validated by nothing.
It is the only research artifact the next run reads; the write-ups persist on the audit record as written, never as distilled, and no typed research field exists.

### Step 6e: Self-Review

**Type:** Local-model call (122B, thinking), on a **continuity run only**; a debut holding has no prior position to review and skips the step.

The reasoner reviews the prior run's position against what has happened since, before interpretation writes this run's thesis document.

#### Local-model call — Self-review (Qwen3.5-122B, thinking)

**Model.**
The resident 122B in thinking mode; no grammar.

**Prompt — input.**
One message in two parts.
Part 1, in page order: the holding header with the analysis date and FETCHED VALUES; PRIOR POSITION — the prior run's date and the price then, its action and rationale, its conviction and its three expected prices each with its horizon date; PRIOR THESIS — the prior run's thesis document verbatim, with any split-context line ([§Step 6b](#step-6b-deterministic-financial-analysis)); ANALYSIS — this run's analysis (the prior analysis stays out, already folded into it); and REALIZED — the engine's realized data from Step 6b: the price now and its move since the prior run; for each prior expected price whether its horizon has passed, and if so the close on that date with its accuracy score, else the path so far; the holding's accuracy scores at each horizon for the model and for the engine, each with the date of the last check that moved it and whether the prior analysis read it; the individual checks written since the prior analysis was written — this run's own checks included, since the checks precede the loop ([§Step 5](#step-5-load-shared-context)) — newest by horizon date up to the drafted count, under a heading stating that every line landed after the prior analysis and none was read before, with the total when the cap trimmed some — or, when none landed, one fixed sentence saying so and that the scores shown are the ones the prior analysis read, its thesis document under PRIOR THESIS; the engine's own metric, grade, band, tier and hurdle values then and now; and any parameter-boundary line.
For a `role_risk_only` holding PRIOR POSITION carries the prior action and rationale alone, REALIZED the fund's realized data — price and NAV then and now, the exposure and expense reads then and now — and no accuracy scores.
Part 2 asks for the **review**: each expected price against what happened; each falsifier and trigger the prior thesis document named, tripped or fired or not by the numbers in front of it; whether the thesis survives; where the prior read was right or wrong and why; what to revise; and what should change in how this holding is analyzed — within the review's length band ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).

**Returns.**
The **review** as prose, read as text and validated by nothing, persisted on the audit record.
It feeds the thesis-document message and the thesis document's summary paragraph; nothing app-side reads it.

### Step 6f: Interpretation and Action

**Type:** Local-model call (122B, thinking) for the thesis document, a second message in the same conversation (122B, non-thinking) for the typed appendix, then a separate **action decision** (122B, thinking).

The reasoner interprets the engine's computed analysis and this run's research into the holding's **intrinsic verdict** — a **two-arm** record ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)).
The engine's deterministic values ride as the **baseline arm**; this conversation authors the **model arm**: the **thesis document**, with its conviction and its expected share price at three months, twelve months and three years stated in words as part of the argument, then transcribed into the **typed appendix** — with the engine's numbers in the prompt as evidence, never as bounds.
For a **`role_risk_only`** holding the conversation is the first message alone: its thesis document carries the role read and no prices, and no appendix follows.
The interpretation authors **no action**: a separate **action call** follows — it reads the finished verdict, the position, the engine's evidence and the **investor profile** (its only entry point into the job), and returns the rung with a one-line rationale.

#### Local-model call — The thesis document (Qwen3.5-122B, thinking)

The message is one message in two parts on the frame every Portfolio prompt shares, and the model is told nothing about the app that produced its inputs ([local-models.md §Prompt posture](local-models.md#prompt-posture)).
Part 1 is inputs only: each data section explained once — what it is and each field's unit or polarity — and then its values, with no instruction in it.
Part 2 is the task only: what the document covers, in order, each item naming the Part 1 section it draws on and restating no value or unit.
The system prompt is the role and the two-part shape of the message.

**Model.**
The resident 122B in thinking mode; no grammar.

**Prompt — input.**
Part 1, in page order: the holding header with the analysis date and FETCHED VALUES; COMPUTED — the engine's metrics, its grade and sub-scores (each axis's polarity glossed once, the imputed-score disclosure where it applies), its bear / base / bull bands at three months, twelve months and three years with the twelve-month method and the typed `TargetMeta` provenance flags rendered so a low-signal band is weighed, not obeyed, the risk tier, the capital-efficiency hurdle read, the hard-forensic filings read as typed evidence with the rule it matched stated as a fact, the narrative-vs-reality read, the implied-expectations range beside the bands, the FINRA short-interest read, the options-activity signal with its signed skew and stated convention, the same-underlying option overlay, the overlay's financing / economics / dilution legs where the stock carries the overlay, and the Step-5 context loads where they apply — the sector-matched commodity prints, the CBOE backdrop, a fund's underlying-positioning row, a fired technology-event pre-flag; MARKET ANALYSIS — the house view, rendered as a market-level analysis and never named by product; ANALYSIS — this run's analysis; and on a continuity run REVIEW — this run's review — then PRIOR THESIS verbatim, with any split-context line ([§Step 6b](#step-6b-deterministic-financial-analysis)); the accuracy record rides the review alone ([§Step 6e](#step-6e-self-review)).
The investor profile is **deliberately absent**: the intrinsic verdict is profile-independent and the profile enters at the action call only ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)).
A fund's Part 1 carries its class line (the fund's reported asset class and the structure line where one applies), the exposure tilt with the closed-end price-vs-NAV line and the positioning line, the risk profile with the market-wide options backdrop, and the engine's evidence gaps as data statements, in place of the equity reads.
Part 2 asks for one document covering: the thesis; the key drivers; the bear, base and bull scenarios, each with the conditions that produce it and the model's probability; the falsifiers and triggers in words — each stating a concrete measure, level and period, a trigger stating the direction of the position change; the expected share price at each horizon and the conviction, argued in the text; and the summary paragraph — the financial read, why those prices and that conviction, and on a continuity run what changed since the prior analysis and how the prior read held up, drawing on the review — within the thesis document's length band ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
A `role_risk_only` document covers instead the role — the mandate and the exposure the vehicle supplies — the risks, the triggers for trimming or selling, and the summary paragraph, with no prices and no conviction.

**Returns.**
The **thesis document** as prose, read as text and validated by nothing, persisted on the verdict.

#### Local-model call — The typed appendix (Qwen3.5-122B, non-thinking)

**Model.**
The same conversation's second message, thinking off: a transcription, not a judgment; schema-constrained output.

**Prompt — input.**
The message asks for the conviction and the expected share price at each horizon as the document states them, closing on a placeholder-only return shape.
It states that a field is `null` where the document states no value.

**Returns.**
One JSON object under the grammar: `conviction` (`high` / `medium` / `low`) and `expected_price_3m`, `expected_price_12m`, `expected_price_3y`, each field nullable.
The app keeps only the type check — each present price finite and strictly positive, since the accuracy check divides by it, a present conviction in its enum, `null` accepted on every field — the domain canonical at [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict); an off-domain value re-issues the message once under the adapter's bounded retry ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)).
A null is acted on by presence alone: a null price opens no model leg at that horizon and a null conviction renders as none ([§Step 7](#step-7-open-episodes-then-persist-run-and-audit)).
Not asked for on a `role_risk_only` holding.

#### Local-model call — Action decision (Qwen3.5-122B, thinking)

**Model.**
The resident 122B in thinking mode — the rung is a judgment call weighing the whole verdict against the position and the profile; schema-constrained output (action + rationale, the full ladder structurally open on both branches).

**Prompt — input.**
One message in two parts on the shared frame, the model told nothing about the app that produced its inputs.
Part 1 opens with the holding header and POSITION — shares held, cost basis, market value, unrealized gain or loss, and the position change since the last pull — and carries no FETCHED VALUES block.
For a priced holding it then carries VERDICT — the conviction, the three expected prices and the thesis document verbatim; COMPUTED — the engine's own action rung, its grade, its bands beside the model's prices, the capital-efficiency read as the tested total returns against the hurdle rate, the hard-forensic state from the filings classification, the overlay's financing legs, the option overlay and the commodity prints where they apply; on a continuity run PRIOR ACTION — the prior rung and rationale; SUPPORTED ACTIONS as one data line, the engine's set stated as complete with a rung not listed outside the computed read; and the **investor profile** less its tax row.
For a `role_risk_only` holding it carries the class, the role read, the exposure tilt, the risk profile, the closed-end price-vs-NAV line and the evidence gaps, the thesis document verbatim, the same continuity section, and SUPPORTED ACTIONS from the reduced {sell all, trim, hold}.
The engine's own rung is shown as a computed read, never as a recommendation, and the ladder reaches the model through the return shape's enum rather than a permission sentence ([portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action)).
Part 2 is the two items in output order — the rung with the task's weighing clause, the profile tie-break, on a priced holding the sunk-cost clause and with a prior action the firmness clause, then the one-sentence rationale — and the placeholder-only return shape.
**Tunnel vision is enforced by input isolation**: no whole-book input exists — no cash position, sector weights, concentration, or other holdings — and the prompt scopes the decision to the holding's verdict, position and investor profile alone.

**Returns.**
The schema-validated **`ActionDecision`**: one rung from the fixed ladder (sell all → trim → hold → add → add aggressively) and a one-line rationale, persisted on the verdict as `action` + `action_rationale`; a blank rationale fails the call.
**Rung only** — no target weight, share count, or dollar figure; sizing is whole-book work and belongs to the future portfolio planner.
A chosen rung outside the engine set persists exactly as authored, with the departure **app-stamped on the holding's audit** (`action_annotations`) — annotate, never bar, the two-arm contract's posture; a tripped hard-forensic state is one such annotation, never an override ([portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action)).
The app then appends the fixed tax caveat to the rationale on a trim or sell-all under a tax-aware profile on a position with an unrealized gain or loss ([portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action)).

### Step 6g: Checkpoint

**Type:** Computed (app layer).
No model.

The step persists the holding's completed work and checkpoints it.
The verdict: the engine arm — grade and sub-scores, the three bands, risk tier, hurdle read, hard-forensic state and action rung — app-stamped directly, never echoed through the model, and the model arm — the thesis document, the typed appendix's conviction and prices — persisted exactly as authored, type-checked only, never validated against the engine ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)); and the action with its rationale, `action_source` and annotations.
The audit record: the write-ups as written, the distillation shape and call count, the analysis, the review, the page roster, the research gaps, the engine's computed reads, the model ids and the prompt stamp ([storage.md §Local Analysis Suite Storage](storage.md#local-analysis-suite-storage)).
No channel carries a model value into the engine: the model arm's numbers are its own by design, and the engine's deterministic readings never depend on them — the two-arm contract.
On the holding's successful full pass the step clears any persisted quick-check attention flag for the holding.
The completed holding is **checkpointed** here so a resumed run skips it ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).

## Step 7: Open Episodes, then Persist Run and Audit

**Type:** Computed (engine + persist).
No generative model call.

With the per-holding loop complete, the loop's actions are final — under the tunnel-vision contract no reconciliation stage follows ([portfolio-analysis.md §Portfolio roll-up](portfolio-analysis.md#portfolio-roll-up)).
The app builds the deterministic **roll-up** — verdict counts, the concentration and cash reads, the exited-names acknowledgment from the Step-4 diff (graded nowhere, but acknowledged rather than silently dropped), and the run-level **data-health** aggregate, including completed holdings with research degradation and their total persisted research-gap count.

Then the deterministic pass **opens this run's episodes** in the **episode store** ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)): one for every priced holding analyzed this run whose latest episode is a month or more old, or which has none, recording its expected share price at three months, twelve months and three years — a horizon the appendix left null recorded null — the engine's base value at the same horizons, the creation date, the spot that day and the anchor close that bridges the record across a later split ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)); a `role_risk_only` holding has no prices and no episodes, and a holding whose appendix stated no price at all opens none.
The checks on the episodes already in the store ran at [§Step 5](#step-5-load-shared-context), before the loop, so this run's reviews read them.
Episodes are never updated otherwise and never deleted; the store is the log.
The accuracy scores derived there persist with the holding, render on its card, and ride the self-review alone ([§Step 6e](#step-6e-self-review)).

The application persists the run: each holding's verdict and action, the **holdings snapshot it ran against** (the next run diffs against this), the roll-up, and each holding's audit record — the write-ups, the distillation shape, the analysis, the review, the page roster, the engine's reads, the gaps, the model ids and the prompt stamp — the field set specified once in [storage.md §Local Analysis Suite Storage](storage.md#local-analysis-suite-storage).
Retention keeps the last N runs; the episode store persists independently of that window ([storage.md](storage.md)).
The job writes nothing to vector memory: a holding's continuity is the deterministic load of its own prior documents ([local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)).

## Step 8: Generate Portfolio Page and Update UI

**Type:** Computed (frontend).
No model.

The **Portfolio page** renders each holding's verdict: the **thesis document** as text, the typed strip beside it — the action with its rationale (no sizing, per the tunnel-vision contract), the conviction, the three expected prices and the holding's accuracy scores — and one compact engine line; the card's full content is canonical at [portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display).
A **`role_risk_only`** verdict renders its thesis document with the action alone — no prices, no conviction — never empty priced placeholders.
Each card also carries its **selection control** (driving selective re-analysis), any amber **attention flag** raised by the quick check and retained only until that holding's next successful full pass, and its **analysis-vintage stamp** after a selective run.
The page renders all of this alongside the portfolio roll-up, and shows not-rated and insufficient-evidence positions with their reason ([interface.md](interface.md), [portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display)).
Above the holding cards a compact **sort bar** reorders the stack in place by overall value, dollar gain, percentage gain, or total cash invested — a display-only control over engine-computed position fields, defaulting to overall-value-descending ([portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display)).
The page also renders the latest standalone **Pull holdings** snapshot — the page body before any run exists; a stamped current-holdings section above the cards when fresher than the last run — with presence-only churn tags (*new · not in last analysis* / *no longer held*), never mutating or hiding the run-anchored cards ([portfolio-analysis.md §Storage and display](portfolio-analysis.md#storage-and-display)).
While the job ran, the run tracker replaced the page (latest-run-only); on completion the page shows the persisted results.
A **run is never a report**: a cancel or failure removes nothing that was shown ([run-tracking.md](run-tracking.md)).
A tunnel-vision run persists complete or leaves no row; a corrupt persisted blob still lists as **unreadable** and opens to nothing ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).

## The quick check (engine-only)

**Type:** Computed (engine) + API retrieval (the per-holding price refresh, the run-level `DGS2` and `DGS10` prints, and the per-asset-type evidence re-pulls — the full retrieval recipe is canonical in [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
**No model call, no web research, no Schwab call.**

A separate, cheap control that keeps the engine's reads live between full runs: it loads the **last run's holdings snapshot and verdicts** (no Schwab pull — it tests the computed picture, not the book), refreshes prices, the `DGS2` and `DGS10` prints, and the per-asset-type evidence legs, re-reads each priced holding's price against the engine's **bands**, re-derives the **total-return hurdle** (the v2 scenario multiples re-anchored on the fresh `DGS10` against the last full pass's stored percentiles and drivers — the canonical quick-path basis in [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)) and its cheap re-derivation tripwires, and raises **attention flags** and quiet **evidence-event badges** — never reading or rewriting any model-authored content; the thesis document, conviction and expected prices are not evaluated between runs.
The retrieval recipe, the flag triggers, the evidence-event legs, and the degraded-check rule are all specified once in [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only) (constants in [§Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).

It holds the **single global run slot** and streams per-holding rows to the run tracker like any job.
Because it makes **no model call**, it skips the daemon-connectivity check and runs even with the daemon configured-but-down — the same run-gate relaxation as ATO's Quick Audit ([trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture)); because it does **no web research**, it triggers no pre-run SearXNG notice.
The Schwab connection — and the shared FMP / FRED credential presence — remain presence preconditions, like everywhere in the suite, but no Schwab call is made.
A failed price refresh has no cache to fall to — the sweep never reads the shared price cache — so the holding's market family types `unknown` and the price-dependent reads skip, badging it in a selective run rather than passing silently ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
