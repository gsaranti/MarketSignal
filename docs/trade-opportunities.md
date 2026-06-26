# Trade Opportunities

Trade Opportunities is a **local, on-demand job** that surfaces investment ideas through deep web research and hard data analysis, organized across a fixed risk-by-horizon matrix and grounded in the current Market Signal house view. Like Portfolio Analysis it runs entirely on local models (see [local-models.md](local-models.md)) and is **prescriptive and firm**; unlike it, it is **not tied to current holdings** — its purpose is to discover new opportunities. It still **requires a connected Schwab account**, whose option chains supply the per-candidate options-activity signal (see [schwab-integration.md](schwab-integration.md)).

## What the job hunts

The goal is profit: ideally **finding a name before it moves**, but also **catching a move early enough that even a "late" entry compounds** (a memory-cycle or AI-infrastructure name six months into its run still returned multiples). These are two distinct detection problems, and the job runs **both modes**:

- **Early detection** — a *leading operating metric inflecting before the income statement and before the multiple re-rates*, while a live bear narrative still suppresses the price (e.g. a high-margin segment re-accelerating at a depressed multiple; a free-cash-flow turn while the market prices bankruptcy; a first surprise profit into a crowded short).
- **Continuation** — *demand-visibility signals* that license buying a move already underway: an estimate-revision and beat-and-raise streak, a guidance step-change far above consensus, rising backlog / book-to-bill, corroborating customer (hyperscaler) capex, or a commodity contract-price turn with supply still constrained.

The discipline that separates a real opportunity from a chased story is **the leading-metric anchor**: a candidate's narrative must be tied to a *countable, dated, third-party-verifiable* number that is moving — not the story alone. A narrative with no inflecting operating metric behind it is a story stock, and is rejected. This is the same data-honesty stance as the report's evidence floor and the suite's compute-don't-guess rule ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)), applied to discovery.

## Triggering

The job is manual and runs in the run tracker with per-cell progress. It runs under the same single global run slot as the report and Portfolio Analysis — only one run at a time across the app (see [local-models.md §Failure posture](local-models.md#failure-posture), [run-tracking.md](run-tracking.md)).

## The opportunity space

Output is organized as a **3×3 matrix**: three **risk tiers** (high / medium / low) × three **horizons** (short / mid / long term) = nine cells, each holding a small set of opportunities (or none, when nothing qualifies). The user sees high-, medium-, and low-risk sections, each containing short-, mid-, and long-term ideas. **Risk-tier assignment is deterministic** — derived by rule from measurable inputs (profitability, market cap, liquidity, volatility, leverage, drawdown, and event exposure), not a label the model picks — so the same asset lands in the same tier run to run. The fixed matrix is what makes the output comparable across runs and forces breadth — the job must consider every risk/horizon combination rather than clustering on whatever is topical.

## Archetype — the lens that decides which signals matter

Risk×horizon *places* an opportunity; **archetype decides how to find and judge it.** The same signal means opposite things in different kinds of business — a low trailing P/E is a *buy* tell for a steady compounder and a *top-of-cycle sell* tell for a commodity cyclical — so the job classifies each candidate into one of five archetypes, and that classification selects the **signal weighting and the valuation lens** for the rest of the pipeline. Archetype is a first-class dimension (deterministic features, model-confirmed), feeding the risk-tier rule rather than replacing it:

| Archetype | The leading tell to detect | Valuation lens | "Late but still works" continuation tell |
|---|---|---|---|
| **Secular compounder** | a high-margin recurring/platform engine accelerating *inside* a mispriced business | PEG; estimate-revisions-vs-multiple | margin/segment reveal + base breakout |
| **AI / secular-cyclical infra** | segment-revenue YoY acceleration; estimate-revision velocity | forward P/E *against* the revision rate | beat-and-raise streak, guidance step-change, customer-capex corroboration, backlog/book-to-bill |
| **Commodity cyclical** | supply discipline + spot/contract-price turn at washed-out sentiment, *while still loss-making* | **P/B, P/NAV, mid-cycle EPS — trailing P/E suppressed** | contract-price up-quarter with supply still cut; a new demand kicker |
| **Category disruptor** | a leading operating metric (reservations, subscriber net-adds, trial efficacy, scanner share) + a hard demand>supply proxy + a strategic-incumbent stake | track the leading metric, not EPS | net-add / guidance beats — *and the inverse as the exit* |
| **Quality compounder** | operating income decoupling upward from revenue; durability-under-stress (e.g. renewal rate holding through price hikes) | valuation is a **risk gate, not an entry** | margin stair-steps; recurring profit pool growing toward a third of profit |

Archetype also informs the deterministic risk-tier (quality compounders skew lower-risk; commodity cyclicals and disruptors skew higher) and the horizon read.

## The pipeline

The job runs as a **funnel**: broad, cheap discovery narrows to a small candidate set, then each candidate gets expensive per-name validation — the condense-as-you-go discipline the rest of the suite uses, and the shape the data budget requires (the richest signals are rate-limited, so they are spent only on the narrowed set).

1. **Theme & candidate discovery (research generates candidates).** Two converging feeders, both research-active — this is where ideas are *found*, not merely vetted:
   - **Top-down** — a secular-theme / event scan over the news stack (Tavily / GDELT / FMP Articles) and the macro-release calendar identifies the *dated public ignition points* (a technology shift, a policy / geopolitical event, a regulatory approval) and the industries they lift; the web-research loop then surfaces the names exposed to each theme.
   - **Bottom-up screens** — cheap, broad, structured: FMP movers / valuation extremes / earnings; **estimate-revision and earnings-surprise screens**; **commodity-price turns** (FRED / Stooq) for the cyclical sleeve; and **positioning scans** — insider-buy clusters and new institutional / activist stakes (FMP), short-interest extremes (FINRA), and notable congressional buys (FMP).

   The combined set is deduped, sanity-filtered for tradability, and tagged with the signals that surfaced it. (This is the load-bearing fix to the old ordering: "names surfaced by research" is a *discovery* feeder that runs **here**, before candidate selection — research is no longer only a downstream validation step.)
2. **Archetype classification.** Each candidate is tagged with its archetype (deterministic features + model confirmation), selecting the signal weights and valuation lens for the steps below.
3. **Deterministic analysis (the engine computes every number).** Per candidate, the Rust financial-analysis engine (shared with Portfolio Analysis — see [portfolio-analysis.md](portfolio-analysis.md)) computes the archetype-appropriate quantitative picture: the leading-metric series (revision velocity, segment acceleration, commodity-price turn, margin decoupling), the earnings surprise / SUE, positioning (insider net, short interest, COT for cyclicals, the Schwab options signal), scenario price targets, a **price-action confirmation read** (relative strength vs the market / sector and proximity to a multi-year base breakout, from equity price history), **and two derived reads the selection stage leans on** — the **narrative-vs-reality ratio** (estimate-revision pace vs multiple change: *justified-expensive* when estimates outrun the multiple, *hype* when the multiple outruns flat/declining estimates) and the **forensic flags** (margin compression while revenue accelerates; net-income-vs-operating-cash-flow divergence; receivable/inventory build outpacing sales; restatement / auditor-change history). The model interprets these; it never invents them.
4. **Deep web research** — the 122B reasoner (thinking mode) plus the web tool, running the bounded per-topic research loop per candidate ([web-research.md](web-research.md)), fail-soft. It validates and builds the case around the leading metric: competitive / business position; the **driving narrative and market sentiment** — how much of the price reflects emotion about *what might come* versus present fundamentals, weighed against the numbers; **forward opportunity and thematic tailwinds**; the external corroboration the structured feeds can't give (customer / hyperscaler capex commentary, supply discipline, transcript backlog / TAM targets / inflection language, and **DRAM/NAND ASP direction** — the one cyclical price with no free structured feed); and, **mandatory, the contemporaneous bear case** — the job must state why a name might fail, because the winning traits also appeared in the famous failures (Cisco, Intel, GE rode the same surface signals down).
5. **Distillation** — the reasoner in non-thinking mode consolidates the loop's findings into candidate summaries (the same resident 122B by default; the fast 35B tier is a benchmark-gated option — see [local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).
6. **Selection, scoring & gating.** The 122B reasoner interprets the computed analysis and distilled research and selects per cell, under explicit discipline: **score the conjunction, never a single signal** (base rates are brutal — most stocks underperform Treasuries over their life, and the winner traits recur in losers); **require the leading-metric anchor plus external validation**; **apply the narrative-vs-reality ratio**; treat **price action (relative strength / base breakout) as a confirmation overlay** that adjusts conviction but never substitutes for the leading-metric anchor; and **run the risk / forensic gate** — a candidate tripping the forensic flags, or whose move is mostly multiple-expansion, is capped or excluded rather than promoted. Each survivor is assigned its deterministic risk tier (archetype-informed) and horizon → matrix cell. **A cell may return no opportunity** when nothing qualifies — empty cells are honest, not failures, and the matrix never pads itself to fill them.
7. **Continuity check.** Prior opportunities are carried forward with an updated status; additions and removals must be justified by what changed (see [thesis-continuity.md](thesis-continuity.md)). The **continuation-failure alarms are first-class exit signals** — estimate revisions rolling over, a beat-streak breaking, shipments diverging below sell-through — and flip a still-valid idea to `played-out` or `invalidated`.

## The opportunity

Each opportunity is a structured, schema-validated record:

- **asset / ticker**
- **archetype** — secular-compounder / ai-infra / commodity-cyclical / disruptor / quality-compounder (the lens that judged it)
- **detection mode** — early or continuation
- **directional thesis** — firm and specific
- **leading operating metric** — the countable, dated anchor and its trend (the thing that must be moving)
- **catalyst** — why now
- **horizon** — short / mid / long (matches its cell)
- **risk tier** — high / medium / low, assigned deterministically by rule (matches its cell)
- **conviction** level
- **narrative-vs-reality read** — whether the move is fundamentally underwritten or multiple-expansion, from the estimate-revisions-vs-multiple ratio
- **bear case** — the contemporaneous reason it might fail (mandatory)
- **entry consideration**
- **risk / forensic flags** — any tripped quality gate
- **status** — `new`, `still-valid`, `played-out`, or `invalidated`, for carry-forward across runs

The fixed archetype / risk / horizon / status vocabularies keep the matrix stable and the list evolving rather than churning — an idea persists with an updated status instead of silently reappearing or vanishing.

## Signal inputs

The job's signals are tiered by how they're sourced, mapping onto the engine / research split (full catalog in [data-sources.md](data-sources.md)). **The Market Signal Report's data-source logic is unaffected** — these are additive, suite-only feeds. **All FMP requests (the report and both local jobs) use one shared credential, now upgraded to the paid tier**; the report's existing calls behave identically on it (see [data-sources.md §Local Analysis Suite Sources](data-sources.md#local-analysis-suite-sources)).

- **Already built** — commodity positioning (CFTC COT) and per-stock options activity (Schwab chains).
- **FMP paid tier — the broad working & discovery feed (deterministic engine):**
  - *Fundamentals & valuation:* financial statements, ratios, revenue **segments**, owner earnings, DCF.
  - *The revision signal:* analyst estimate consensus (snapshotted run-to-run for velocity) plus the analyst-action flow — the rating-distribution time series (`grades-historical`), price-target trend (`price-target-summary` / `-consensus`), and upgrades / downgrades; and earnings surprises.
  - *Forensic gate:* `financial-scores` — **Altman Z-Score + Piotroski** — a drop-in for the quality / forensic gate, alongside the margin-vs-revenue and cash-flow-vs-income divergences computed from the statements.
  - *Positioning (all symbol-keyed):* insider buys / sells + per-symbol statistics, 13F institutional ownership, and **Senate / House congressional trading**.
  - *Discovery:* the company screener, stock peers, and industry classification; **bulk** endpoints (scores / surprises / ratings / ratios across the whole universe) drive the discovery funnel in a few calls — which is why the per-symbol rate cap is a non-issue and no second estimates provider is needed.
- **Keyless feeds:** **short interest** from FINRA (consolidated biweekly — FMP has no short-interest endpoint); **commodity spot / contract prices** from FRED (daily energy + monthly IMF metals incl. uranium) and Stooq (daily futures incl. copper); **CFTC COT** (already built).
- **Engine-computed price-action confirmation:** relative strength (vs the market / sector) and multi-year base-breakout proximity, derived from equity price history (Stooq deep history, reusing the portfolio engine's momentum / volatility computations). A cross-archetype **confirmer, not a trigger** — it adjusts conviction, never substitutes for the leading-metric anchor (the overlay the historical winners consistently showed: a multi-year base → high-relative-strength breakout).
- **SEC EDGAR — authoritative primary-source cross-check:** the numbers that drive final grades / targets are reconciled against EDGAR filings + XBRL (the compute-don't-guess, primary-source stance), with FMP as the broad working feed. Because the FMP fundamental and positioning feeds are symbol-keyed, **ticker→CIK resolution is a non-blocking enhancement** (needed only to extend the SEC cross-check to an arbitrary name), not a hard prerequisite.
- **Web-research lane (the bounded loop):** the signals with no structured feed — **DRAM/NAND ASP direction** (TrendForce is paid), **supply discipline** (capex cuts / curtailments, which live in transcripts and news), customer-capex corroboration, and the qualitative narrative read.

## Holdings cross-reference

After selection, a **deterministic post-step** flags any opportunity that overlaps the user's current holdings (owned / not-owned), so the two features cohere — an idea you already hold is surfaced as such. Crucially this runs *after* candidate discovery and selection, so holdings never influence which opportunities are found or chosen; it reads only the holdings list, never the Portfolio Analysis memory partition. Trade Opportunities therefore stays genuinely independent of the account.

## Continuity and isolation

The job retains its most recent N runs, feeds the prior run into the next, and embeds results into the **Trade Opportunities memory partition only** — isolated from the report's and Portfolio Analysis's memory (see [local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)). Output is firm and does not churn between runs absent hard data.

## Storage and display

Each run persists its matrix of opportunities together with an **audit record** (the report(s) and sources used with retrieval timestamps, the screening and discovery inputs, the computed signals and the bear case behind each pick, the model ids and quantizations, and the prompt/schema version); retention keeps the last N runs ([storage.md](storage.md)). The **Trade Opportunities page** renders the 3×3 matrix, each cell listing its opportunities with archetype, thesis, leading metric, catalyst, conviction, entry consideration, bear case, and status (see [interface.md](interface.md)).

## Failure posture

The execution gate requires the local model daemon and roster ([local-models.md](local-models.md)) **and a connected Schwab account** (for the options-activity signal — see [schwab-integration.md](schwab-integration.md)); a missing or lapsed connection blocks the job. Web research within a run is fail-soft (thinner evidence, lower conviction), while a hard model or persistence failure fails the run ([scheduling.md](scheduling.md)). A discovery feeder that fails (a screen, a positioning scan, a theme sweep) degrades to fewer candidates rather than failing the run.
