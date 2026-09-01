# Portfolio Analysis: logic flow

> This describes the designed job behavior.  
> Some parts are not implemented yet.

`Gate → Pull holdings → Classify positions → Compare with prior run → Load context → Analyze each holding and decide its action → Roll up and score outcomes → Save → Display`

## Important terms

- **Holding**
  - One investment currently in the portfolio.

- **Normalized holding**
  - One combined position per ticker.
  - Rows from multiple accounts are added together.
  - Long and short quantities offset each other.

- **Gradable holding**
  - A holding the job can analyze honestly.
  - Usually a US-listed stock or a supported equity fund.

- **Not rated (`not-rated`)**
  - The investment type is outside the grading system.
  - Examples: cash, bonds, standalone options, and net-short stocks.
  - Its real portfolio exposure may still be counted.

- **Insufficient evidence (`insufficient-evidence`)**
  - The holding is normally gradable.
  - Required data is missing, stale, or conflicting.
  - The job abstains instead of guessing.

- **Priced verdict (`priced`)**
  - Full analyzed result.
  - Includes a letter grade, targets, conviction, and forward outlook.

- **Role-risk-only verdict (`role_risk_only`)**
  - Used when the investment can be understood but not honestly priced.
  - Common for bond, commodity, international, or leveraged funds.
  - Describes portfolio role, exposure, risk, expenses, and data gaps.
  - Does not contain a letter grade or price target.

- **Intrinsic verdict**
  - Judgment of the holding by itself.
  - Does not consider the investor profile or other holdings.

- **Portfolio action**
  - The holding's final disposition — one ladder rung plus a one-line rationale.
  - The profile-aware counterpart to the intrinsic verdict.
  - Selected by the action decision; never weighs other holdings — whole-book reconciliation is the portfolio planner's.
  - Uses the ladder:
    - Sell all.
    - Trim.
    - Hold.
    - Add.
    - Add aggressively.

- **Action decision**
  - The model call that selects the portfolio action.
  - Weighs the holding's own verdict against the investor profile.
  - The only place the investor profile enters the job.
  - Considers no portfolio concentration, overlap, or cash.

- **Grade**
  - A–F summary of the business’s current quality, valuation, and risk.
  - It is mainly backward-looking.
  - This holds in both arms — the model authors its own grade, but its forward view lives in the forward outlook, not the letter.
  - Momentum does not belong in the designed letter grade.

- **Forward outlook**
  - The job’s short-, mid-, and long-term directional view.
  - Kept separate from the backward-looking grade.

- **Conviction**
  - Confidence in the intrinsic verdict.
  - High, Medium, or Low.
  - Separate from the letter grade and risk tier.

- **Engine arm (baseline)**
  - The deterministic side of a priced verdict.
  - Sub-scores, letter, and scenario targets calculated by the engine.
  - Mechanical stand-ins for outlook, conviction, and action.
  - Always obeys its own caps and rules.

- **Model arm (model view)**
  - The model's own read of the same fields, authored freely.
  - Validated structurally and on its declared numeric domain (`docs/portfolio-analysis.md` §The holding verdict); never checked against the engine's numbers.
  - Scored against the engine baseline by the outcome scoreboard.

- **Risk tier**
  - Deterministic estimate of investment risk.
  - High, Medium, or Low.
  - Used in return requirements.

- **Scenario**
  - Bear, base, or bull version of the future.
  - Each priced scenario has an engine-calculated target.

- **Target driver**
  - Per-share financial value used to calculate a target.
  - Forward earnings per share when usable.
  - Otherwise forward revenue per share.

- **Hurdle**
  - Minimum expected return needed to justify keeping or adding capital.
  - Based on the two-year Treasury yield plus a risk premium.

- **Dead money**
  - Even the bull case fails the return hurdle.
  - This tilts the action decision toward an exit.

- **Thesis ledger**
  - Persistent record of why the job holds its view.
  - Stores the thesis, drivers, scenarios, falsifiers, and action triggers.

- **Falsifier**
  - A condition showing the thesis may be wrong.
  - Example: operating margin falls below a stated level.

- **Action trigger**
  - A prewritten condition for adding, trimming, or selling.
  - Sizing is deferred to the portfolio-planner job.

- **Condition ID**
  - App-controlled identity for a machine-checkable condition.
  - Preserves evaluation history when the calculation rule is unchanged.
  - A changed calculation starts fresh.

- **Attention flag**
  - Amber warning raised by the Quick check.
  - Suggests running a full or selective analysis.
  - Does not change the verdict by itself.

- **Evidence event**
  - New information that requires fresh analysis.
  - Examples: earnings, a material filing, or a large estimate revision.

- **Input delta**
  - App-calculated list of what changed since the prior run.
  - Covers prices, financials, estimates, positioning, and the position itself.

- **Analysis vintage**
  - Date of the full analysis that created the current intrinsic verdict.
  - Important when a selective run carries older verdicts forward.

- **Selective re-analysis**
  - Full analysis of strictly the selected holdings (ruled 2026-08-16).
  - Every unselected holding keeps its earlier intrinsic verdict, with the quick check's findings surfaced as non-blocking card badges rather than a forced re-analysis.
  - A held position with no prior verdict is left not analyzed (a selectable "run to grade" placeholder).

- **Research reuse**
  - Prior distilled web research, under about four weeks old, used to seed and merge into this run's research.
  - It never skips the research loop for an analyzed holding — the loop runs full each run that holding is analyzed.
  - Cannot change a verdict itself.

- **Pre-profit overlay**
  - Extra execution and financing check for a stock not yet producing reliable operating profit or cash.
  - Tracks business progress, cash runway, and financing pressure.
  - Does not change the letter grade.

- **Decision episode**
  - Dated record of a changed recommendation state.
  - Used later to measure whether the decision worked.

- **Outcome label**
  - Engine-calculated result after 1, 3, 6, or 12 months.
  - Includes return, benchmark-relative return, and drawdown.

- **House view**
  - Current Market Signal thesis and major market themes.
  - Omitted when older than one week.

- **Investor profile**
  - Risk tolerance, horizon, objective, and tax posture.
  - Used only by the per-holding action decision.
  - Never changes the intrinsic verdict.

- **Option overlay**
  - Options attached to a held stock.
  - Examples: covered call, protective put, or collar.
  - Changes the holding’s effective upside and downside.

- **Reasoning model**
  - Local 122B model.
  - Performs research, interpretation, and the per-holding action decision.

- **Embedding model**
  - Local 4B model.
  - Finds relevant prior analysis.
  - Performs no investment reasoning.

## Main data sources

- **Charles Schwab**
  - Current holdings.
  - Quantities, cost basis, market value, and instrument identity.
  - Option chains for held stocks — fetched per holding at Step 6a; per-contract delta is parsed for the option overlay's targeted per-strike fetch (the activity signal never reads it).

- **FMP**
  - Company profiles.
  - Financial statements; the ratio endpoints are designed, not yet pulled.
  - Estimates, earnings, dividends, and live quotes; the narrative read's revision-pace leg compares the estimates against the prior run's stored comparator.
  - Deep historical stock prices.
  - Sector benchmark prices — loaded run-level per carried holding's sector, memoized.
  - Outcome-label price history.
  - Insider and congressional activity — designed, not yet pulled.
  - Peers, segments, and ratings — designed, not yet pulled; company news is pulled by the full-run pass (the per-stock research-seed leg) and by the quick check (the qualifying-news-seed leg).
  - Fund information and sector/country weights.
  - Sector valuation data used for supported funds.

- **SEC EDGAR**
  - Official filings and XBRL company facts.
  - Restatements and auditor changes — the item-classified 8-K sweep (the hard-forensic filing kinds' producer).
  - Optional fund holdings through N-PORT — designed.

- **FRED**
  - Two-year and ten-year Treasury yields.
  - Historical ten-year yields for target calculations.
  - Energy and metals commodity prices — level-basis windows, loaded run-level.

- **FINRA**
  - Short-interest level, trend, and days-to-cover — the consolidated file, fetched once per run and looked up per stock.
  - The quick check's conditional refresh stays dormant (no short-interest series exists in the closed engine surface).

- **CFTC**
  - Futures positioning for commodity, index, rate, and currency funds.

- **CBOE**
  - Broad put/call market sentiment.
  - A bounded HTML extraction of the daily statistics page — no machine-readable current-day endpoint exists; locally detectable structure drift degrades to a typed gap (the guarantee's scope lives at data-sources.md §CBOE).

- **SearXNG (built)**
  - Primary web search for holding research.

- **Tavily (built)**
  - Backup web search when SearXNG can't serve usable results.

- **Local storage**
  - Prior holdings snapshot.
  - Prior verdicts and thesis ledgers.
  - House view and investor profile.
  - Research cache, decision episodes, and outcome history.

---

# Full Portfolio Analysis job

## Step 1 — Start and safety checks

- **Data retrieved**
  - No investment data yet.

- **Checks**
  - No other Market Signal job is running.
  - Local reasoning and embedding models are configured and available.
    - That availability probe — a local-only daemon call, no investment-data API — is the one check that runs before the run slot is claimed; every external fetch happens inside the slot.
  - Schwab is connected.
  - Schwab’s seven-day refresh token is still valid.
  - FMP and FRED credentials exist.

- **Model**
  - None.

- **Output**
  - Job starts.
  - Or the app explains what is missing.

---

## Step 2 — Pull and normalize the portfolio

- **Data retrieved from Schwab**
  - Every granted account’s positions.
  - Symbol, description, asset type, and quantity.
  - Average cost and market value.

- **Manual data (designed, not built)**
  - Optional imported holdings.
  - Supplements Schwab holdings.
  - Never replaces the Schwab connection requirement.

- **Normalization logic**
  - Combine the same ticker across accounts.
  - Add signed quantities.
  - Add signed cost-basis totals.
  - Add market values.
  - Determine the final net long or short side.
  - A netted quantity, cost basis, or market value that does not finish finite fails the pull naming the symbol (`docs/schwab-integration.md` §What is pulled).
  - Preserve original rows for audit and display.

- **Failure logic**
  - Failed holdings pull → fail the run.

- **Model**
  - None.

- **Output**
  - One normalized portfolio snapshot.
  - Snapshot pinned for this run.

---

## Step 3 — Classify each position

- **Data retrieved**
  - Uses the normalized Schwab snapshot.
  - No new external data yet.

- **Initial classification**
  - Stock → possible full analysis.
  - ETF or fund → reduced analysis path.
  - Option, bond, cash, or unsupported type → not rated.
  - Net-short stock → not rated.

- **Stock rule**
  - Final US-listing validation happens in Step 6a.
  - A long US-listed stock can use the full pipeline.

- **Fund rule**
  - Final strategy routing runs in the fund engine at Step 6b, once `etf/info` has arrived at Step 6a.
  - Supported US equity fund → possible priced verdict.
  - Structurally unpriceable fund → role-risk-only verdict.

- **Not-rated exposure rule**
  - No fake grade is created.
  - Weighing a not-rated position's risk against the book is the portfolio planner's job.

- **Model**
  - None.

- **Output**
  - Preliminary analysis route for every position.
  - Explicit not-rated reasons.

---

## Step 4 — Compare holdings with the prior run

- **Data retrieved**
  - Current normalized snapshot.
  - Prior analysis run’s normalized snapshot from local storage.

- **Calculations**
  - Compare position size by ticker: absolute quantity on a same-side move, and the signed swing on a long↔short flip.
  - Tag each current holding:
    - New.
    - Increased.
    - Decreased.
    - Unchanged.
  - A long↔short flip still receives Increased or Decreased from that size rule; `side_reversed` is a separate badge on a carried verdict, not a fifth diff tag.
  - A prior ticker now absent is exited.

- **Important rule**
  - Standalone Pull holdings snapshots are ignored here.
  - Only the prior analysis run is the comparison baseline.

- **Model**
  - None.

- **Output**
  - Position delta for every current holding.
  - List of positions closed since the prior run.

---

## Step 5 — Load shared market context

- **Data retrieved from local storage**
  - Latest Market Signal house view.
  - Recent report summaries.
  - Fixed investor-profile preset.

- **Data retrieved from FRED**
  - Current `DGS10` ten-year Treasury yield.
  - Current `DGS2` two-year Treasury yield.
  - Historical `DGS10` observations for valuation anchors.

- **Enriching run-level context (each fail-soft to a typed gap counted on data health)**
  - Energy and metals commodity prices from FRED (level basis, never the rate normalization).
  - Gold quote from FMP.
  - Futures positioning from CFTC.
  - Broad put/call statistics from CBOE.
  - Sector benchmark histories from FMP (fetched per carried holding's sector, memoized).

- **Logic**
  - Omit the house view when older than one week.
  - Normalize rates into decimal form.
  - Share this context across all holdings.

- **What each context input feeds (later steps, not here)**
  - House view and recent report summaries → the Step 6f interpretation call.
  - Investor profile → the Step 6f action decision only — never intrinsic analysis.
  - `DGS10` → valuation multiples at Step 6b.
  - `DGS2` → return hurdles at Step 6b.
  - Commodity prices → commodity-linked holding evidence, matched by profile sector; the gold quote attaches on a gold / precious-metals industry label only.
  - CFTC positioning → a commodity / macro fund's underlying-positioning read, mapped by fund identity keywords.
  - CBOE put/call → a venue-level options-sentiment backdrop: broad-market context, never a per-name signal.
  - Sector benchmarks → the input delta's technology-event pre-flag at Step 6b.

- **Failure rule**
  - `DGS2` or `DGS10` still unavailable after retries → fail the run.
  - Optional market context may fail softly.

- **Model**
  - None.

- **Output**
  - One shared context packet.

---

## Step 6 — Per-holding analysis loop

The following sequence runs once for every holding in the work list.

Each completed holding checkpoints separately as it lands — verdict, audit, its own telemetry and data-health contribution, and the refreshed run-level keyed identities — so a cancellation or a crash resumes the unfinished holdings (Resume behavior, below; a per-holding model failure isolates rather than interrupting); the between-holdings cancellation check runs beside it.

### Work-list logic

- **Full run**
  - No cards selected.
  - Analyze every gradable holding.

- **Selective run** (analyzes strictly the selection — ruled 2026-08-16, `docs/verification/2026-08-16-selective-badges-ruling.md`)
  - **Work list**
    - The user-selected holdings, and nothing else.
    - No automatic additions — the former safety additions now surface as card badges (below), never a forced re-analysis.
    - A selective request that finds no readable prior run runs the whole book — there is nothing to carry, and the page cannot offer a selection without a rendered run (as-built, ruled 2026-08-18).
  - **Carried holdings** (unselected, with a prior verdict)
    - Keep their previous intrinsic verdict, action, and thesis ledger, vintage-stamped.
    - Display the older analysis vintage.
    - A stale (over-age) add action is automatically weakened to Hold and marked rule-demoted.
    - A stale exit action and a stale hold both stand as-is, behind the stale-vintage badge.
    - Nothing else may move a carried action without fresh analysis.
  - **Not-analyzed holdings** (unselected, no prior verdict)
    - A new or never-analyzed holding is left not analyzed — no verdict is written this run.
    - It renders as a "run to grade" placeholder card, selectable so the next selective run can grade it (a full run grades the whole book).
  - **Badges** (the quick check still sweeps the carried tail, but only to inform — it never re-analyzes)
    - Attention flag — the quick check confirmed a problem (a fired falsifier or trigger, a newly-failing dead-money read, or a changed band relation).
    - Unknown family — a required signal family couldn't be checked (a degraded-sweep note), so the sweep can't vouch for the carried verdict.
    - Unexamined evidence event — new material information the verdict never saw (earnings, a material filing, or a large estimate revision).
    - Side reversed — the carried verdict now sits on a net-short position, so its thesis is for the opposite side (a directional verdict is only ever authored for a long; the verdict is marked `side_reversed`).
    - Stale vintage — the carried verdict is older than the ~4-week window.
    - Each is a non-blocking badge on the card; the user acts on it by selecting the holding or running a full analysis, so an urgent single-holding run is never blocked by the rest of the book.

- **Resume behavior (built)**
  - Resume uses the interrupted run’s pinned holdings and context.
  - No fresh holdings pull occurs; per-holding retrieval (option chains included) still runs live at each resumed holding’s own Step 6a.
  - Starting resume window: 48 hours.
  - A version, roster, or baseline change since the interrupted run refuses the resume with its reason.
  - The stamps are the contract: the six version axes stamp completed-holding and selective carried-tail semantics and the checkpoint format stamp the trail's shape, so a rebuild moving none resumes with the restored holdings' pre-change behaviour, and no build identity is checked (`docs/portfolio-analysis.md` §Failure posture, ruled 2026-08-29).
  - Each restored holding row carries its own prompt-usage observations and fired-retry events, so the finished run's data-health read spans both processes and counts no call twice, omitting only the superseded calls of holdings the resumed process re-analyzed — the interrupted holding's abandoned calls reach no row, and a row that never landed or no longer reads takes its calls with it (`docs/portfolio-analysis.md` §Failure posture).
  - Each restored row likewise carries its holding's deep-history and benchmark health, and the finished run rebuilds the data-health counts from the rows — a re-analyzed holding counts once and a benchmark unavailable in both processes once (`docs/portfolio-analysis.md` §Failure posture).

---

### Step 6a — Build the holding dossier

For each holding, Step 6a assembles the dossier — the evidence packet the deterministic engine computes over at Step 6b. The gather is guard-ordered: a stock resolves its listing identity first and pulls the rest of its evidence only if that guard clears.

#### Resolve stock identity (runs first)

A guard-terminal stock — one the guard finds unsupported, non-US, or identity-conflicting — is routed straight to its verdict here (not-rated or insufficient-evidence) and spends no further pull. A fund carries no stock guard; its priced-vs-role-risk route is decided later, once its data lands (see Fund routing below).

- **Stock identity validation**
  - Company profile and listing identity — one FMP company-profile fetch, pulled before the rest.
  - Cross-check that profile against Schwab’s — exchange first (the symbol is queried as-is; no symbol remap), then issuer name:
    - US primary exchange (NYSE / NASDAQ / AMEX) with a matching name → continue; a US-listed ADR passes on venue, not domicile.
    - FMP definitively resolves no such listing (an honest empty response), or a non-US primary listing → not rated.
    - US exchange but the issuer names share no significant token → insufficient evidence (a possibly-transient identity conflict).
    - A failed or unreadable profile fetch, or identity too sparse to cross-check on either side — a resolved profile missing its exchange or name, or a Schwab description with no issuer name (or only a ticker the FMP name doesn’t contain) → continue with a recorded degraded input.

#### Gather the evidence (skipped for a guard-terminal stock)

Once the stock guard clears — and for every fund — the remaining legs are pulled. Stocks and funds pull disjoint financial legs — a fund touches neither the company statements nor SEC — but both share the deep price history, the option chain, and the local prior-run legs.

- **Stock data retrieved from FMP**
  - Income statement, balance sheet, and cash-flow statement (quarterly); the income and cash-flow rows' period and filing dates are stored as the canonical fixed-width ISO render, and a row whose date does not parse is dropped as unreadable.
  - Ratios (P/E, P/B, debt/equity), key metrics (return on invested capital, free-cash-flow conversion, gross profitability), owner earnings, and enterprise value (EV, EV/EBITDA) — designed, not yet pulled; P/E, P/S, and P/B are derived from market cap and the statements today instead.
  - Discounted-cash-flow valuation cross-check (intrinsic value vs price) — designed.
  - Financial scores (Piotroski F-score, Altman Z-score) — designed.
  - Forward consensus estimates (revenue, EPS); the narrative read's revision-pace leg diffs the NTM mid against the prior run's stored comparator.
  - Street targets and rating history (consensus target price, upgrade/downgrade actions) as opinion evidence — designed.
  - Dividends (the trailing distributions); per-symbol earnings are pulled only by the quick check, not this pass.
  - Insider and congressional activity (Form 4 insider buys/sells, congressional trades) — designed.
  - Peers, float, and revenue segments (comparable tickers, free-float shares, product/geographic revenue mix) — designed.
  - Live quote; company-news seeds for the research lane (symbol-scoped news inside the research-freshness window, typed as leads with stable app-assigned IDs — stocks only).
  - Deep dated price history (~1,600-day lookback).

- **Stock data retrieved elsewhere**
  - SEC XBRL company facts (revenue, gross profit, net income, total assets, equity) — the filings index is read by the quick check's material sweep and this pass's item-classified forensic sweep.
  - FINRA short interest (level, trend, days-to-cover) — the once-per-run consolidated file, looked up per stock; a symbol absent from the file carries no read (a market fact, not a gap).

- **Option chains**
  - Fetched per holding from Schwab.
  - Volume, open interest, implied volatility, and per-contract delta (delta feeds only the option overlay, never the activity signal).
  - Put/call ratios and the IV/skew read are computed at dossier assembly.
  - Held options link to the same stock by the OCC symbol decode and classify into the typed overlay (covered call, protective put, collar, other); delta comes from a targeted per-strike fetch scoped to the held contracts, so the activity signal's bounded NTM query never widens.
  - A row whose symbol does not decode to the holding's root never links — fail-safe absence.
  - Chain fetch failure or malformed body → typed options gap.
  - An empty chain on an un-optioned name is a quiet market fact, not a gap.
  - Option-chain failure does not fail the run.

- **Fund data retrieved from FMP**
  - `etf/info`.
    - Expense ratio, AUM, NAV, and asset class (the mandate label — one field, not two).
    - Serves closed-end funds an empty body (probe 2026-08-21) — a CEF gets none of these fields.
  - `profile` (one per fund).
    - The `isFund` flag and the description text — the closed-end detection's two conjunctive legs.
    - A failed read records the "closed-end detection cannot run" gap; detection then honestly reads not-a-CEF.
  - Sector and country weights.
  - Sector P/E snapshots, current and historical.

- **Optional fund data (designed)**
  - SEC N-PORT fund holdings.
    - Used for concentration and single-name look-through.
    - Never required for the normal fund floor.

- **Local data retrieved**
  - Prior intrinsic verdict.
  - Prior thesis ledger.
  - Position delta.
  - Shared market context.
  - Portfolio Analysis memory for this holding — semantic recall over the job's own summary rows (top-k, fail-soft; empty on the first run after the lane landed).

#### Fund routing

A fund’s route — read from the `etf/info` and weights gathered above — is applied by the fund engine at Step 6b.

- **Fund routing**
  - US equity exposure with usable weights → priced-fund path.
  - Bond or commodity fund → role-risk-only path.
  - International fund below the US-exposure guard → role-risk-only path.
  - Leveraged or inverse fund → role-risk-only path.
  - Option-overlay fund → structural path-dependence flag; other priceability rules decide the route.
  - Mutual fund without usable weights → role-risk-only path.
  - Closed-end fund (built) → a structure marker orthogonal to the class, detected from the profile's `isFund` flag plus a closed-end description fragment (both required — never guessed). A bond CEF still routes bond; on today's empty `etf/info` surface a CEF resolves no class and takes the role-risk path labeled "closed-end fund", never `insufficient-evidence`.

#### Semantic recall (built)

The embedding-based recall of this holding’s prior analysis, rendered into the dossier beside the deterministically loaded prior verdict and ledger.

- **Embedding model**
  - Converts a holding-specific query (symbol, sector/industry, standing thesis, key drivers) into a vector via the fixed local embedder.
  - Searches only Portfolio Analysis memory, and only its per-holding summary rows (durable learnings never participate).
  - Retrieves the top-3 prior per-holding summaries.
  - Performs no investment reasoning.
  - An unconfigured embedder or an empty summary shelf (the first post-slice run, by design) is silent absence — no query embedding is spent and no gap is recorded.

- **Embedding failure**
  - Skip semantic recall for this holding only.
  - Keep the directly loaded prior verdict and ledger.
  - Record a typed degraded-input gap.

#### Output

- **Output**
  - Complete stock or fund dossier.
  - A stock leaves with its resolved listing route — a verdict if the guard was terminal, otherwise clearance into the 6b engine; a fund leaves with its routing inputs, not yet a route.

---

### Step 6b — Calculate the financial picture

- **Data retrieved**
  - Uses the dossier and shared context.
  - No model or web research.

- **How the step runs**
  - The deterministic engine runs the holding down whichever of three routes it resolved to — a priced stock, a priced equity fund, or a role-risk-only fund. The two priced routes are graded (letter + sub-scores); the role-risk-only fund gets a role / risk readout with no grade. Nothing in the step calls a model or the web.
  - The evidence floor is not a closing stage: its gates fire inline as the values below are computed, and the first failure short-circuits the holding to `insufficient-evidence`, skipping 6c–6f; the gates are gathered under Evidence floor at the end of the step.

- **Order of computation (priced stock)** — computed in sequence, several reads feeding the next:
  - **Pre-profit overlay** (computed and persisted for every priced stock; only an eligible read binds — §Pre-profit overlay) — financing / execution health, whose rule consequences later **cap the engine arm's conviction and narrow its action set**.
  - **Sub-scores** (quality, valuation, risk) → roll up into the letter; **momentum / market setup** is computed alongside them, live, as context outside the letter.
  - **Scenario targets** (bear / base / bull; one-month and twelve-month) → feed the return hurdle.
  - **Letter grade** — the weighted roll-up of the sub-scores.
  - **Risk tier** (High / Medium / Low) → sets the return-hurdle rate.
  - **Return hurdle** (capital efficiency) — risk tier + scenario returns → the dead-money read.
  - **Continuity and ledger checks** — the prior ledger's conditions evaluated against the new engine values.
  - **The hard forensic state and the technology-event pre-flag** — computed here as input-delta / conviction-layer evidence.
  - **Implied expectations** — the scenario multiples inverted at the live price (computed with the targets, sharing their one multiple derivation); absent on the current-multiple carry.
  - **Narrative versus reality** — the pace pair against the prior run's stored comparator (or the operating-reality fallback), computed before interpretation; a hype read soft-caps the engine arm's conviction at Medium. A debut carries no read, and neither does a carried holding read fewer than 7 days after the prior read (`NARRATIVE_MIN_ELAPSED_DAYS`, drafted — `docs/portfolio-analysis.md` §Starting parameters).
  - **Designed reads (not computed yet)** — the soft forensic checks would ride as evidence once built; they are not part of the live order.

- **The through-line**
  - The load-bearing dependencies: **sub-scores → letter**, **risk tier → hurdle**, **targets → hurdle**, **overlay → conviction cap + action-set narrowing**.
  - A **fund** swaps this stock spine for the equity-fund path below; a **role-risk-only** fund computes no grade, target, tier, or hurdle.
  - 6b reaches no verdict of its own: its reads feed each other (the dependencies above) to produce the **deterministic financial analysis** — letter, targets, tier, hurdle, overlay, ledger state — which the interpretation call reasons over at 6f (and whose targets the outcome scoreboard later scores). The evidence floor, gathered at the step's end, is the one gate that acts within the step.

#### Engine primitives (used in the formulas below)

Three primitives recur in the value formulas that follow, so they are defined once here.

- **`scale(x, lo → hi)`** maps `x` linearly onto 0–100 and clamps it: `lo` scores 0, `hi` scores 100, and an **inverted** band (`lo > hi`) scores lower inputs higher.
- **`average(…)`** is the unweighted mean of whichever legs are present — a missing leg is dropped *inside* a value; a wholly-absent value is handled per section (usually imputed to a neutral 50 at the roll-up, never dropped there).
- A **ratio** `a ÷ b` is `None` when the denominator is missing or zero; a few ratios below add a stricter `> 0` guard, noted where they do.

#### Pre-profit overlay (stocks, computed first)

Computed before the grade for every stock and persisting even when the stock does not enter the overlay or later abstains. The overlay's statement states are produced here in one pass over the carried observation history (research has not yet run at this stage); the research-observation merge at Step 6e then recomputes the observation-dependent legs and rule consequences over the loop's app-validated rows.

- **Who enters** (either arm admits)
  - Priced stock with non-positive TTM operating income.
  - Or no positive forward-EPS consensus plus negative TTM free cash flow.
  - Funds never enter (the overlay is called only on the stock branch). [note: an arm that can't be resolved from the data yields `unscorable`, not entry.]

- **Structured data used**
  - Cash and cash equivalents.
  - Short-term investments.
  - Quarterly free cash flow.
  - Quarterly capital spending.
  - Quarterly revenue and gross profit.
  - Diluted share count.

- **Engine calculations** (each is its own value; all persist on the overlay record)
  - **Liquid resources** — cash on hand to fund the burn.
    - Inputs: cash and equivalents, short-term investments (balance sheet).
    - Equation: `cash + short-term investments`. Absent short-term investments read as 0; absent cash → `None`.
  - **TTM cash burn** — annual cash consumption; zero when not burning.
    - Input: TTM free cash flow (the reported FCF line, else `operating cash flow − |capex|` — sign-tolerant, since FMP reports capex as a negative outflow).
    - Equation: `max(0, −TTM free cash flow)` — positive FCF gives 0.
  - **Cash runway** — months of cash left at the current annual burn.
    - Inputs: liquid resources, TTM cash burn (both above).
    - Equation: `12 × liquid resources ÷ TTM cash burn`, only when burn > 0 (else `None`). (liquid ÷ annual burn = years; ×12 → months.)
  - **Capex intensity** — capital intensity; **context only, no rule consumes it**.
    - Inputs: TTM capex magnitude, TTM revenue.
    - Equation: `Σ|quarterly capex| ÷ Σ quarterly revenue` over the trailing four quarters — a decimal ratio (the prompt renders it ×100). [note: requires revenue > 0 and that both four-quarter windows share the same newest quarter-end.]
  - **Dilution** — split-adjusted year-over-year change in the diluted share count.
    - Inputs: the newest diluted-share count and the same quarter one year back.
    - Equation: `now ÷ prior − 1` (signed; +0.30 = 30% more shares). [note: needs five contiguous quarters and prior > 0.]
  - **Gross-margin direction** — where the recent gross margin sits and which way it moved.
    - Inputs: per-quarter gross margin (`gross profit ÷ revenue`, gross profit derived as `revenue − cost of revenue` when the line is absent).
    - Equation: recent level = average of the latest two quarters; direction = `recent − preceding two-quarter average` (signed margin points). The preceding average is used but not persisted; the recent level and the change are.

- **Financing state** (evaluated in this precedence — first match wins)
  - `unscorable`: TTM cash burn missing (or, while burning, runway uncomputable).
  - `not_burning`: TTM cash burn is 0 (checked before the runway bands).
  - `adequate`: runway ≥ 24 months.
  - `watch`: 12 ≤ runway < 24 months.
  - `constrained`: runway < 12 months.

- **Derived states** (computed only for an eligible overlay; each persists)
  - **Execution read** — guidance-vs-actual misses, over higher-is-better operating metrics only. A guidance row and an actual pair only when their metric identity, units, issuer scope, reporting period end, and exact non-unknown reporting span all match and the guidance is ex ante — published on or before the period end and strictly before the period's earliest actual, so a results release never supplies its own bound and a post-period preview never binds; an annual, half-year, and quarter row can share one end date without ever pairing. The *bound* is the latest admissible revision's stated low (a range low winning over point guidance at the same date, then confidence), must be finite and positive, and a same-vintage conflict on either side drops the period (canonical: `docs/portfolio-analysis.md` §Starting parameters, the guidance vintage policy; each miss records its reporting span and the bound's and actual's publication dates). A paired period *misses* when `(bound − actual) ÷ bound ≥ 5%`; `material_single_miss` when the newest comparable period misses by ≥ 20%; `repeated_miss` when ≥ 2 of one metric-and-span identity's latest four comparable periods miss (different metrics or spans never combine, and two missed metrics in one period never count twice). An overflowed ratio is no miss, the period staying comparable. [note: at this stage the read sees only carried prior observations; this run's research rows join at the Step-6e recompute.]
  - **Economics deterioration** — recent two-quarter average gross margin non-positive **and** down ≥ 5 percentage points.
  - **Material dilution** — YoY diluted-share change ≥ +15%.
  - **Severe deterioration** — at least two of {repeated-or-material execution miss, constrained runway, economics deterioration, material dilution} hold, **and** at least one of those is the execution or the economics leg.

- **Research data added later**
  - Production and deliveries.
  - Bookings, backlog, or reservations.
  - Guidance ranges and matching actuals.
  - Unit economics.

- **What the overlay emits** (the whole record persists — none of the calculations above are scratch)
  - The complete overlay record persists for **every priced stock** — the statement inputs above, the **financing state** (one of the five values), the execution / economics reads, the matched **rule consequences** that bind the engine arm (conviction ceiling and action-set narrowing, detailed at Step 6e), plus its eligibility result, unscorable gaps, and the period-end-and-span-keyed observation history (accumulating validated research rows across runs) — carried on the holding's audit row so the period history survives run retention.
  - Only an **eligible** overlay reaches the Step-6f interpretation prompt, and even then the renderer exposes a **selected subset** — the financing / execution states, the matched rules, and the figures behind them (runway, liquid resources, burn) — as engine-arm context, not the entire record.
  - Here the overlay is **statement-derived**; the Step-6e research-observation merge finalizes the execution legs over app-validated rows, and no research observation is ever guessed.

#### Sub-scores (stocks)

The letter’s three inputs (quality, valuation, risk), each on a 0–100 scale where higher is better, plus a momentum read carried outside the letter. As-built each is a fixed-band score with no sector-relative or own-history normalization; the richer form — sector-adjusted bands, the name’s own history, and the value-creation reads below — lands with the full grade slice (designed, `docs/portfolio-analysis.md` §Starting parameters).

- **Quality score** — profitability on one statement basis. In the letter (40%); stored in the output sub-scores.
  - Inputs (6a dossier statements, TTM or annual basis): net margin, gross margin.
  - Equation: `average` of two `scale`d legs —
    - net margin → `scale(0 → 0.30)` (0% → 0, 15% → 50, ≥30% → 100).
    - gross margin → `scale(0.15 → 0.65)` (15% → 0, 40% → 50, ≥65% → 100).
    - a loss-maker scores 0 on the net-margin leg (on-scale, not off-scale); both legs missing → `None`, imputed to 50 at the letter.
  - Designed, not computed: return-on-invested-capital vs capital cost, gross profitability, free-cash-flow conversion, sector-adjusted bands, own history.

- **Valuation score** — cheapness via inverted multiples (cheaper → higher). In the letter (30%); stored in the output sub-scores.
  - Inputs: P/E, P/S, P/B (6a dossier; derived from market cap and the statements when FMP does not supply them).
  - Equation: `average` of three `scale`d legs —
    - P/E → `scale(70 → 12)`, inverted (P/E 12 → 100, 41 → 50, ≥70 → 0); a non-positive P/E scores a fixed **20** (low, not off-scale, not `None`).
    - P/S → `scale(25 → 2)`, inverted (2 → 100, ≥25 → 0).
    - P/B → `scale(30 → 2)`, inverted (2 → 100, ≥30 → 0).
    - all three absent → `None`, imputed to 50 at the letter.
  - Designed, not computed: sector-appropriate metric selection (banks, REITs, cyclicals), sector-adjusted bands, own history.

- **Risk score** — safety (higher = safer): low realized volatility and low leverage. In the letter (30%); stored in the output sub-scores.
  - Inputs: realized volatility, debt-to-equity.
  - Equation: `average` of two inverted `scale`d legs —
    - volatility → `scale(0.045 → 0.005)` on the raw daily figure (0.005 → 100, ≥0.045 → 0).
    - debt/equity → `scale(2.5 → 0)` (0 → 100, ≥2.5 → 0); negative book equity scores a fixed **0** (floor).
  - Drawdown enters the risk tier, not this score; no liquidity series is on this job’s surface.

- **Momentum score** — trailing price return; context, **outside the letter**. Stored in the output sub-scores.
  - Input: trailing return (first-to-last close over the short undated EOD window, never the deep dated closes).
  - Equation: `scale(−0.30 → 0.30)` (−30% → 0, 0 → 50, +30% → 100).

#### Scenario targets (priced stocks)

Bear / base / bull price targets — one-month and twelve-month — priced from a per-share driver and a rate-anchored multiple; they feed the return hurdle below. Computed before the letter is finalized; a stock with no admissible driver abstains here. A target the arithmetic cannot finish as a finite number exits the holding as insufficient evidence, and every other derivation reads as a gap where its arithmetic does not finish finite (`docs/portfolio-analysis.md` §Evidence floor).

- **Choose the driver** — the per-share fundamental the scenarios are priced from.
  - Inputs: consensus forward EPS, else consensus forward revenue per share (÷ diluted shares).
  - Rule (first admissible wins): positive consensus forward EPS; else consensus forward revenue per share; else → `no-admissible-driver` evidence gap.

- **Build bear, base, and bull driver cases** — the three per-share driver values.
  - Inputs: the consensus low / mid / high, and the trailing TTM print (for the clamp).
  - Equation: base = the mid; bear / bull = the low / high. [note: a missing or half-published spread holds both legs at the mid and records a flat driver; each leg is then clamped to `[trailing × 0.75, trailing × 1.35]` to bound extreme growth (released only when a corroborated trough signature is detected). Revision-dispersion widening is designed, waiting on the revision feed.]

- **Calculate the valuation multiple** — the price-per-unit-of-driver applied to each case, rate-anchored so the target tracks rates.
  - Inputs: ~3 years (12 quarters) of historical driver yields, each paired with the `DGS10` as of that quarter (latest published on or before the anchor date), plus today's `DGS10`.
  - Equation:
    - Per historical quarter: driver yield = `driver ÷ price`; spread = `driver yield − that quarter's DGS10`.
    - Form bear / base / bull spread percentiles (bear = 75th, base = 50th, bull = 25th — a wider spread is a cheaper multiple).
    - Re-anchor each with today's rate: multiple = `1 ÷ (spread percentile + today's DGS10)` (if that denominator < 0.01, fall back to the raw-multiple percentile).
    - [note: needs ≥ 8 usable observations; below that, fall back to recorded raw-multiple percentiles; with no history, carry the current `spot ÷ base-driver` multiple.]

- **Calculate the scenario prices and returns**
  - Inputs: the driver cases, the multiples, spot, and trailing-TTM dividends per share as the twelve-month payout proxy.
  - Equation:
    - Twelve-month price = `driver × multiple`, per scenario. [note: crossed bear/base/bull prices are repaired to ascending and logged; a dispersion floor = `clamp(daily vol × 15.87 × 0.5, 0.05, 0.20)` widens — never narrows — the bear/bull spread.]
    - Twelve-month total return = `(price + trailing-TTM dividends per share) ÷ spot − 1`; the payout leg is a proxy, not a forward estimate.
    - One-month price = `spot × (1 + twelve-month base price-return ÷ 12)`; the bear/bull legs take a band = `clamp(daily vol × 2 × √21, 0.02, 0.15)` (else 5%) — 2σ √t-scaled to the 21-session month, introduced at `targets-v5` and carried by current `targets-v6` — dividends excluded.

- **Where these land**
  - Targets → the output price targets (each with its methodology + provenance flags); total returns → the hurdle read; the drivers, spread / raw percentiles, spot, trailing-TTM dividend proxy, dispersion floor, and consensus EPS mid persist as the quick-check basis the between-run engine re-anchors against. The multiples themselves are not stored — they are recomputed closed-form from the basis.

#### Letter grade (stocks)

The A–F letter — the weighted roll-up of the three letter sub-scores. Computed once the scenario-target stage above has cleared its floor gate; stored in the output as the grade plus a low-confidence marker.

- **Equation**
  - `composite = quality × 40% + valuation × 30% + risk × 30%` — a weighted mean on 0–100. Momentum is not an input.

- **Letter cutoffs** (on the composite)
  - A: 85 or higher.
  - B: 70–84.
  - C: 55–69.
  - D: 40–54.
  - F: below 40.

- **Missing sub-score handling**
  - A missing sub-score is imputed to a neutral 50 before the roll-up.
  - At least two of the three letter sub-scores must be real, else the holding abstains (`insufficient-evidence`).
  - A letter resting on any imputed sub-score carries the low-confidence marker.

#### Risk tier (priced stock)

High / Medium / Low, set after the grade and feeding the return hurdle below. Inputs: market cap, annualized volatility (raw daily × 15.87 — unlike the risk sub-score, which scores the raw daily figure), debt-to-equity, profitability (net or operating income), and max drawdown.

- **High** — any one condition fires (a missing input cannot fire it)
  - Small company — market cap < $2B.
  - High volatility — annualized volatility > 40%.
  - Deep drawdown — max drawdown > 50%.
  - High leverage — debt/equity > 2.0, or negative book equity.
  - Unprofitable — net (or operating) income ≤ 0.

- **Low** — all four of these hold (drawdown is not a Low condition)
  - Large company — market cap > $10B.
  - Profitable — net (or operating) income > 0.
  - Low leverage — debt/equity in [0, 1.0).
  - Low volatility — annualized volatility < 25%.

- **Otherwise — Medium**
  - Wholly missing tier inputs also produce Medium, with a gap flag.
  - The canonical rule’s liquidity legs are not on this job’s data surface and never fire.

#### Capital efficiency — the return hurdle (stocks)

The dead-money read — whether the scenario returns clear the required return for the risk taken. Inputs: the risk tier, `DGS2`, and the twelve-month scenario total returns. Stored in the output hurdle read (state, rate, the total returns tested, and the new-money flag).

- **Hurdle rate** — `DGS2 + tier premium`
  - Low risk: `DGS2 + 3 percentage points`.
  - Medium risk: `DGS2 + 5 points`.
  - High risk: `DGS2 + 8 points`.

- **Three-state result** (against the hurdle rate)
  - Bear total return ≥ hurdle → `clears`.
  - Else bull total return < hurdle → `fails`.
  - Else → `indeterminate`.

- **Meaning**
  - Only `fails` means dead money; `indeterminate` does not force an exit.
  - New money uses a stricter point test — the base-case total return itself must clear (`base ≥ hurdle`) before Add is allowed.

#### The equity-fund path

The alternative branch to the stock spine above; the fund engine makes the final priced-fund vs role-risk-only classification here in Step 6b (the routing rules are at Step 6a §Fund routing). A role-risk-only fund takes no grade, target, or risk tier — only the role-risk readout at the end of this section.

- **Fund valuation** — what the fund’s sector mix costs now vs its own history; the priced fund’s one real valuation input.
  - Inputs: per-sector P/E snapshots (blended across exchanges), the fund’s current sector weights, ~8–12 quarters of historical sector P/Es — each snapshot candidate and each sector history entering only when both NYSE and NASDAQ legs served at least one row; each quarterly sample admitting only prints dated within its own quarter (after the prior quarter end, on or before its own), so one print backs at most one sample; a print whose date does not parse is inadmissible to every sample.
  - An incomplete snapshot candidate walks to the prior weekday; an exhausted walk records one memoized gap on every fund.
    A failed sector history's memoized gap likewise reaches every later fund whose weightings depend on that sector, never only the fund whose turn issued the request.
  - Equation:
    - Per sector: earnings yield = `1 ÷ P/E`.
    - Composite earnings yield = `Σ(weightᵢ ÷ P/Eᵢ) ÷ Σ weightᵢ` over sectors with a usable P/E — renormalized over the **covered** weight; sectors without a usable P/E are skipped. A usable print is finite, positive, within the plausible-aggregate ceiling, and served under the requested board — the shaper's rule (`docs/data-sources.md` §Financial Modeling Prep).
    - Coverage = the covered weight as an absolute share of the fund; **≥ 70% is required**, else the valuation is a gap (uncovered weight is reported, never averaged in as zero).
    - Valuation sub-score = the percentile rank of today’s composite yield within the same-weights-over-historical-multiples series = `(count of history ≤ today ÷ history length) × 100` (higher yield = cheaper = higher score).
    - [note: needs ≥ 8 history samples — distinct in-quarter observations by construction, since a sample admits only its own quarter's prints — each itself with ≥ 70% coverage.]

- **Fund sub-scores and grade**
  - **Quality** — no fund quality axis exists, so it is a fixed neutral **50** (never presented as fund quality; its presence forces the low-confidence marker).
  - **Valuation** — the coverage-gated percentile above.
  - **Risk** — `average` of two inverted legs: volatility → `scale(0.04 → 0)`, drawdown → `scale(0.6 → 0)`. A **missing leg is imputed to 50** when the other is present (unlike the stock risk score, which drops it); **both legs absent → the fund abstains** (`insufficient-evidence`). [note: these bands differ from the stock risk bands.]
  - **Momentum** — the stock momentum score above, unchanged: trailing return over the short undated window → `scale(−0.30 → 0.30)`, outside the letter; the deep dated closes back the volatility and drawdown reads (risk legs, dispersion floor, tier), never momentum.
  - **Grade** — same weighting and cutoffs as a stock (`quality × 40% + valuation × 30% + risk × 30%`); because fund quality is always 50, this reduces to `20 + valuation × 30% + risk × 30%`, and every priced fund carries the low-confidence marker.

- **Fund metrics**
  - **Expense ratio** — a decimal ratio; rendered to the interpretation prompt (no return figure is expense-adjusted deterministically).
  - **US share** — the fund's US country-weight share; rendered to the prompt as the ≥ 70% guard's own read (every recognized US label summed and capped — `docs/portfolio-analysis.md` §Asset eligibility), never a first-label read.
  - **Composite coverage** — the covered valuation weight from above; rendered to the prompt beside the valuation read.
  - **NAV premium / discount** — `market price ÷ NAV − 1`, only when both are positive (positive result is a premium; the NAV-fallback spot never substitutes for the price — that would fabricate an exact 0%). Consumed on the closed-end form only: a prompt line and a card line, never a score or rule; a CEF missing either usable leg — a positive market quote or a positive NAV — records the named price-vs-NAV gap, its text naming which leg is missing. Its metric-delta row is likewise CEF-gated, so an open-end ETF's transient premium flicker never seeds an input-delta row.
  - **Exposure tilt** — the sector and country weights. On the priced path only the US share above reaches the prompt; a top-five tilt (sector weights, else country when sector is absent) is rendered only on the role-risk path. The house-view comparison happens at the interpretation call.

- **Fund scenario target** (settled flat-driver form)
  - Equation: driver = `spot × composite earnings yield`, held **flat across bear / base / bull**; scenario spread comes from the multiple axis and the volatility-scaled dispersion floor, which applies after every target path, not only the current-multiple carry.
  - The flat-driver form is the settled design, not a stopgap (ruled 2026-08-21, closing the former open item): a scenario-differentiated priced-fund formula returns only on realized-outcome evidence ([portfolio-analysis.md §Starting parameters](../docs/portfolio-analysis.md) is canonical).

- **Fund risk tier** (priced fund)
  - High — annualized volatility > 40%, or max drawdown > 50%. (The function also flags leveraged/inverse structure, but those funds route to role-risk and never reach the priced tier.)
  - Low — no option-overlay flag and annualized volatility < 25%.
  - Otherwise Medium (an option-overlay flag bars Low without forcing High).

- **Fund hurdle** — identical to the stock hurdle (`DGS2 + tier premium`; same three-state and new-money test).

- **Role-risk-only readout** (no grade, target, tier, or sub-scores)
  - Class label — bond fund, commodity fund, leveraged / inverse vehicle, equity fund below the US-exposure guard, equity fund without usable weightings, or fund with unresolved strategy class.
  - Exposure tilt — the top five sector weights, else the top five country weights.
  - Expense ratio.
  - Observable risk — annualized realized volatility (the only numeric risk; no tier).
  - Structural flag, and the evidence-gap manifest (the classification’s own reason appended).
  - On a closed-end fund: the price-vs-NAV read where both a usable market quote and NAV exist, else its named gap in the manifest (the text names the missing leg); the closed-end marker rides the class label.

#### Continuity and ledger checks

After the branch's engine values are set, the prior ledger's conditions are evaluated against them (the ledger checks). The input delta's pieces — position change, prior values, house-view age — already arrived from Steps 4–5 and the dossier; the engine-computed metric comparison renders them as bracketed-id entries the what-changed rows cite.

- **Input delta**
  - Position change and house-view age.
  - Prior-run values carried for the interpretation call to compare.
  - The engine-computed metric comparison resolves at exact old ≠ new; its positioning leg still waits on its data legs.

- **Ledger checks**
  - Evaluate quantitative falsifiers and action triggers.
  - Advance streaks only on a new observation.
  - Preserve condition state by app-controlled condition ID.
  - Withhold a debt/equity condition not stamped with the sweep's own FMP-quarterly equity source — stamped with another, or with none — unevaluable, no state movement, the filing family `unknown`; the full pass stamps and compares it.

- **Technology-event pre-flag**
  - Compare the stock’s move with its sector.
  - Adjust the threshold for the stock’s volatility and elapsed time.
  - Large unexplained relative move adds a research topic.
  - It does not claim what caused the move.
  - A benchmark without a close on the stock’s own two window sessions is a typed gap, not a flag.

#### Other deterministic reads

Additional stock reads that ride into the interpretation call as evidence; none changes the letter directly, and most are still designed.

- **Conviction context**
  - Estimate revisions — the narrative read's revision-pace leg (live); rating history is not yet pulled.
  - Earnings surprises — designed.
  - Price momentum and market setup — live.
  - Short-interest activity — live (the FINRA lookup); insider and congressional activity — designed, not yet pulled; options activity is live.
  - These do not change the letter directly.

- **Narrative versus reality (live)**
  - Compare multiple expansion with estimate improvement — both legs paced over the interval since the prior run's stored comparator.
  - Thin analyst coverage falls back to company operating results against the annualized price move.
  - A hype read (expansion outrunning reality >1.5×, above a 5% expansion floor) soft-caps the engine arm's conviction at Medium — annotation-recorded, the model's own value untouched.
  - A debut has no comparator and carries no read; a carried holding read fewer than **7 days** after the prior read carries none either (`NARRATIVE_MIN_ELAPSED_DAYS`, drafted and calibratable — under it the two legs are same-week noise and the fallback's annualization explodes), the unreadable pace recording its typed reason on the audit (`docs/portfolio-analysis.md` §Starting parameters).

- **Implied expectations (live)**
  - Work backward from the current price.
  - Estimate the growth or margin range already priced in — per scenario multiple, sharing pricing's one multiple derivation.
  - Used as context, not a gate; absent on the current-multiple carry (nothing independent to invert).

- **Hard forensic state (live)**
  - Restatement or auditor change from the item-classified SEC filings sweep — the hard-forensic filing kinds' producer.
  - The research-fed `forensic_event` fraud claim is **advisory** by the 2026-08-24 ruling: it never joins this state — the hard rule trips from the item-classified filing kinds alone — and reaches the model only as cited attention evidence (the producer contract is canonical at `docs/trade-opportunities-workflow.md` §Step 5c).

- **Soft forensic checks (designed, not built)**
  - Altman Z and Piotroski weakness.
  - Profit not supported by operating cash flow.
  - Receivables or inventory outrunning revenue.

#### Evidence floor

The inline gates referenced above, gathered — with each branch's requirements and the short-circuit behavior.

- **Stock requires**
  - A usable current price — finite and strictly positive; a served zero or negative print is a named gap at the FMP parse, never a price (`docs/portfolio-analysis.md` §Evidence floor).
  - No resolved identity conflict (an unverified cross-check proceeds with a degraded-input flag).
  - At least two real sub-scores.
  - An admissible target driver on the v2 ladder (`no-admissible-driver` is a live floor exit).
  - Financial statements are not a separate presence or age gate: missing legs can fail the two-real-sub-score or driver requirements above, but a latest-available statement's age alone does not abstain as-built.

- **Exposure-priced fund requires**
  - A usable current quote or NAV — an unusable market quote falls to a usable NAV rather than masking it.
  - `etf/info` and expense ratio.
  - Usable sector and country weights — a row finite and within 0–100% as served, the adapter dropping any other (`docs/data-sources.md` §Financial Modeling Prep).
  - At least 70% valuation coverage.
  - At least one risk leg — volatility or drawdown (both absent → abstain).
  - At least eight constant-mix history samples, each on prints dated within its own quarter so one print backs at most one sample (fewer → abstain).

- **Floor failure**
  - Mark `insufficient-evidence` with named reasons.
  - Skip research, distillation, refinement, and interpretation.
  - Retain the prior thesis ledger and attention flag.
  - Create no new action or decision episode.

- **Non-floor gaps**
  - Missing optional research or positioning lowers confidence.
  - Weak web coverage alone does not force abstention.

- **Output — the values that leave the step**
  - **The deterministic financial analysis** (the engine's computed reads the 6f verdict is built on):
    - The three **sub-scores** and the **letter grade** (a low-confidence marker when a sub-score was imputed).
    - The **scenario price targets** — bear / base / bull, one-month and twelve-month, each with its exposed **methodology** — plus their **provenance** (anchor form, driver rung, flat / clamp / dispersion flags), or a `no-admissible-driver` abstention.
    - The **risk tier**, and the **hurdle read**: state (clears / fails / indeterminate), rate, the twelve-month scenario **total returns** it tested, and the new-money admission flag.
    - For a pre-profit stock, the complete **pre-profit overlay** (statement inputs, financing / execution states, and the rule consequences binding the arm).
    - For a fund, the **fund grade** and **fund risk tier** (priced); or, for a role-risk-only fund, the engine-computed **role-risk readout** — class label, exposure tilt, expense ratio, numeric observable risk (annualized volatility), structural flag, the closed-end marker with its price-vs-NAV read (or that read's named gap), and evidence gaps (the model authors the role prose on top).
    - Of these, the two-arm **engine arm** — the fields **carried in both arms** (authored deterministically here and by the model at 6f) — is just the **sub-scores / letter** and the **scenario targets** (plus the outlook / conviction / action stand-ins assembled later); the **targets and outlook** are scored against realized outcomes today, and only the target bands additionally get an engine-vs-model head-to-head, while sub-scores and conviction remain unscored.
      The persisted model action is scored through the action-cohort reads; only the engine stand-in's action rung has no reader.
      The risk tier, hurdle read, and overlay are shared deterministic evidence, and a **role-risk-only** fund is a separate, non-two-arm branch.
  - **Supporting reads emitted as 6f evidence** (none changes the letter):
    - The **computed metrics** — net / gross margin, revenue growth, debt-to-equity, volatility, trailing return, P/E, P/S, P/B.
    - **Momentum / market setup**; for a priced fund, the **expense ratio**, **US share**, and **composite coverage** reach the prompt (the full exposure tilt does not; the computed NAV premium reaches it only on the closed-end form — the premium line, or its explicit gap line).
    - The **ledger evaluation** — the prior ledger's conditions' tripped / fired state.
    - The **input delta** — position change, house-view age, and prior-run values carried for comparison.
    - The **hard forensic state** (the filings sweep, with its engine-matched rule when tripped) and a **fired technology-event pre-flag**.
    - The **implied-expectations range**, the **narrative-vs-reality read** (with its matched soft rule when tripped), the **FINRA short-interest read**, and the **option overlay** (both 6f prompts).
    - Designed, not yet emitted: the soft forensic flags and the remaining conviction-context signals (rating history, surprises, insider / congressional).
  - **Control result**: the **data-gap manifest** and the **evidence-floor outcome** — pass, or `insufficient-evidence` with named reasons (which short-circuits 6c–6f).
  - **Persisted working reads (not scratch)**: the engine keeps its intermediates too — the overlay's statement inputs (liquid resources, burn, runway, capex intensity, dilution, margin direction) ride the audit row, and the settled per-share drivers, spread / raw-multiple percentiles, spot, and trailing-TTM dividend proxy persist as the **quick-check basis** the between-run engine paths re-anchor against.
  - **Assembled later, not here**: the engine arm's mechanical **stand-in outlook / conviction / action** are built at verdict time (after interpretation) from these 6b values — they are not part of the Step-6b output.

---

### Step 6c — Research the holding

- **As-built: live** (the research-loop slice)
  - The loop below runs live over the SearXNG-only web tool; a construction without a web stack (the demo, offline tests) degrades to a recorded research-unavailable gap, never a failed run.
  - Runs before the slice graded on the deterministic financials and the house view alone.

#### How the stage runs

For an analyzed holding the research loop and distillation always run and are never skipped — a recent cache only seeds them, it never replaces them.

- **Always runs (seed and merge, never a skip)**
  - The research loop and distillation run in full every run for every analyzed holding.
  - There is no lighter-vs-heavier case, and Steps 6c and 6d are never skipped.

- **Seeded when (Layer 2 cache)**
  - Non-expired (< ~4 weeks) cached distilled findings exist for this holding — one **per-topic** object apiece, the layer the prior run persisted.
  - The orchestrator injects **each topic's own prior object** — its tier-1 distillation, or its topic-keyed group from a single-pass run — plus the holding ledger's **entire standing condition list** into every topic's opening pass, deterministically and with no extra model call, filtered to claims still within their own ~4-week vintage and bounded by a per-topic seed budget.
  - Seeding from the per-topic distillation, not a slice of the cross-topic combined object, starts each loop with richer, un-re-compressed topic detail; the topic is the storage partition, so the seed is a lookup rather than a per-claim re-assignment.
  - The loop then targets what changed rather than rebuilding the baseline; a cached prior never causes it to be skipped.

- **Cold when (no Layer 2 cache)**
  - If no non-expired cache exists, the cache decision reads cold; the holding's standing ledger conditions still orient every topic and can produce seed text on their own.

#### Build the agenda (the topics)

The orchestrator assembles the topic list deterministically; the reasoner works it, never authors it. A stock gets the company topics (plus the conditional topics when their trigger fires); a fund swaps in the fund-flavored topics instead.

- **Stock research topics**
  - Competitive and business position.
  - Recent results and estimate revisions.
  - Catalysts and risks.
  - Management quality and capital allocation.
  - Market narrative and sentiment.
  - Forward opportunity and thematic fit.

- **Pre-profit topic**
  - Runs only for an overlay-eligible stock.
  - Retrieves issuer-reported:
    - Production and deliveries where relevant.
    - Bookings, backlog, or reservations.
    - Guidance ranges and matching actuals.
    - Unit economics.
    - Gross-margin explanation.
    - Cash needs and capital spending.
    - Completed or planned financing.
  - Model extracts the facts.
  - Model does not calculate runway or guidance attainment.
  - First or history-thin pass:
    - Search the latest four reported periods.
    - Record checked periods and sources.
    - Missing history stays `partial` or `unscorable`.

- **Conditional technology-event topic**
  - Runs after a technology pre-flag.
  - Or after a standing technology falsifier.
  - Or after an approved research follow-up.
  - Never from a news seed — a qualifying news seed is a fresh symbol-scoped `news/stock` seed arriving **while a technology-class falsifier already stands** (the deterministic conjunction, `docs/portfolio-analysis.md` §Starting parameters — the rule the quick check's news-seed leg reads for its badge), and the standing-falsifier line above fires the topic by itself, so the seeds ride the pass brief as leads and the agenda carries no seed trigger (retired 2026-08-29, Codex I15).
  - Determines:
    - Substitute, complement, or mix shift.
    - Revenue or profit truly exposed.
    - Deployment timeline.
    - Switching costs.
    - Whether the move looks like panic, real impairment, or overstated benefit.

- **Fund research topics**
  - Mandate and strategy changes.
  - Manager changes.
  - Expense and structure versus peers.
  - Exposure fit with the house view.
  - Whether the exposure is better held directly.
  - Closed-end-fund discount and distribution coverage.

#### Work each topic — the research loop

The orchestrator works the agenda **one topic at a time**. Each topic is worked in isolation over a clean context — a gathering loop plus that pass's separate synthesis call, sharing no context with other topics — and the orchestrator — never the model — owns every search and fetch, stopping at the holding's budget.

- **Two nested levels**
  - **Topic** — the unit of isolation: its own per-topic research loop and synthesis call (the topic isolation `docs/web-research.md` §Terminology defines), and topics never share a context.
  - **Pass** — each topic's loop is a bounded multi-turn tool loop: one root pass plus up to two follow-up passes, so three passes per topic at most. The cap counts passes (branches), not model calls — a single pass runs a bounded gathering loop of tool turns (each one model call requesting a search or fetch the orchestrator runs), then a **separate synthesis call** authors the pass's findings from the gathered evidence over a fresh, tool-history-free conversation, so the gathering turns and the findings grammar never share a request (attempt-4 Finding 4, fix B). As-built the gathering loop opens a **fresh conversation** per pass — a new message history of the system prompt plus a pass brief (the dossier facts, the topic's questions, its seed, the structured seeds, the approved follow-up question on a follow-up pass, and the topic's own claims gathered so far, capped at 40; only the holding's closing disconfirming-fetch pass reads claims drawn from every topic, under the same 40 cap) — and the synthesis call opens its own fresh conversation from the evidence, so only the evidence ledger and the accumulated per-pass findings carry across a topic's passes (`research.rs`, `run_pass` / `synthesize_findings` / `pass_brief`); no message history does.

- **What each topic conversation is given (its inputs)**
  - The shared dossier facts — identical for every topic.
  - That topic's own questions — different per topic.
  - A per-topic seed assembled from that topic's non-expired (< ~4 weeks) prior distilled object, where one exists, plus the holding ledger's entire standing condition list, which is rendered into every topic. The cached object is topic-owned, but the condition component is holding-wide; a topic with no cached prior is logged cold even though standing conditions can still orient it.
  - The company-news seeds — the symbol-scoped FMP `news/stock` headlines pulled into the dossier at Step 6a — orient the topics as leads, never as evidence and never as a trigger (the technology-event bullet above): a seed's claim counts only once the model deep-reads its underlying source.
  - No other topic's findings — a later topic gets nothing from an earlier one. The topics meet only downstream, at the Step-6d distillation (a single consolidation call, or a reduce when the research is large).

- **What is retrieved during a pass (the data)**
  - Live web-page text — the pages the model deep-reads, fetched and readability-extracted from the open web. This is what "current web sources" means.
  - Cached pages — previously-fetched pages under about four weeks old come from the document cache (Layer 1) instead of the network, carrying their original retrieval timestamp; new URLs are fetched live.
  - Search backend, not a separate data source: the orchestrator runs search on SearXNG only (the local suite wires no Tavily fallback).

- **Who owns the context, and what persists**
  - Within a pass the orchestrator owns the prompt: on every gathering turn it appends the tool results and the model's tool request, threading the growing context forward — the model only requests tools, it never touches the network; at most 8 calls from one response are accepted, and the complete serialized message history plus tool schema must remain under the shared input-budget guard before every issued request and retained result. Crossing either bound ends gathering with the omitted tail/results recorded as partial coverage; the pass's findings are not part of this growing context — they are authored by a separate synthesis call over a fresh conversation (attempt-4 Finding 4, fix B), with the grammar-required keys and nonblank findings/claim fields validated again by the app before the pass can complete. Prior `<think>` blocks are stripped from history, never accumulated across turns (`docs/local-model-operations.md` §Strip thinking from history).
  - Carried across the topic's passes (canonical): the append-only evidence ledger and the accumulated per-pass findings, which the orchestrator assembles for the Step-6d distillation. The framing inputs (dossier facts, questions, seed) anchor each pass's conversation from its start.
  - Raw fetched page text is the bulky working material, and the durable record of what a page yielded is its claims in the ledger, not the page text itself. As-built the capped tool results stay in the pass's message history (see the context-fitting note below); how much raw page text survives across a pass boundary is not pinned by the contract.

- **Fitting the fixed context window**
  - `num_ctx` is fixed per model and never raised to make room — raising it reloads the runner and starves memory (`docs/local-model-operations.md` §The num_ctx trap); context pressure is answered by dropping content, not by growing the window.
  - As-built the pressure is bounded at insertion rather than evicted later: untrusted tool-result metadata and each fetch's page text are capped before they enter the context, tool calls per turn and turns per pass are capped, and the aggregate gathering packet is checked before every model request and retained result. A pressure-driven mid-pass roll-off remains unbuilt because the guard stops gathering and moves to synthesis before overflow.
  - The fresh synthesis packet jointly selects source headers and body allocations: a source survives only with usable body text, omitted headers are reclaimed before the selected bodies are water-filled, and every omission or truncation is carried inline, persisted on the research audit, and counted on the run-level data-health read.
  - It never relies on the model server's own truncation, which silently front-drops the prompt's head and leaves the model to hallucinate over the gap.

- **What each model call returns**
  - Inside a pass, each gathering turn requests `web_search` / `web_fetch` calls — the orchestrator executes them and returns the results for the next turn — until the topic is answered or the budget is spent; then a separate synthesis call authors that pass's findings from the gathered evidence (attempt-4 Finding 4, fix B).
  - The model authors each pass's findings write-up; the orchestrator only accumulates them — there is **no topic-close model synthesis**, so the first model consolidation of the findings is the Step-6d distillation.
  - Each ledger entry is a hybrid: the model supplies the claim, the orchestrator stamps its provenance (the source URL / timestamp).

- **Follow-up passes**
  - A follow-up is the model's **proposal** — a structured field the orchestrator reads and decides whether to spend; the model never recurses on its own.
  - It is granted only while depth remains (≤2 follow-ups) **and** the per-item budget has room; on exhaustion it is simply not spent (fail-soft, no follow-up).

- **Disconfirming-fetch pass (built; canonical placement `docs/portfolio-workflow.md` §Step 6c)**
  - The disconfirming-fetch pass (`docs/web-research.md` §Source quality and evidence weighting) runs once per holding after its topics, once the thesis has formed.
  - It is spent from the holding's fetch / wall-clock budget, is not counted against any topic's three-pass depth, and fail-softs to a recorded gap when the budget is already exhausted.

- **Model determines**
  - Which sources answer the topic.
  - Which findings are supported.
  - Whether another focused follow-up is useful.
  - Which forward facts may affect targets.
  - Whether a research-only leading indicator exists.
  - Whether primary-source evidence shows fraud.

- **Stops at the budget**
  - A per-item fetch + wall-clock budget that binds first — a per-holding cap on web-fetches and wall-clock, not a per-pass timer — spent in topic-priority order. When it drains, the lowest-priority remaining topics are skipped fail-soft, each recorded as a degraded-input gap (lower conviction), never failing the run.
  - The per-topic depth cap (≤2 follow-ups, ≤3 passes) works alongside it, guarding against rabbit-holing one topic.

#### Failure and output

- **Failure logic**
  - Web failure reduces evidence.
  - It may lower conviction.
  - It does not automatically fail the run.
  - A model-call failure inside the required 6c–6f path (the action call included) is **isolated per holding** (ruled 2026-08-31): the holding is recorded in `failed_holdings`, its prior verdict carried forward vintage-stamped where one exists, and the run **continues** to the next holding rather than failing. The run fails outright only when **every attempted holding fails** — a systemic cause under which it persists no snapshot and the prior good run stays the latest view (`docs/portfolio-analysis.md` §Failure posture).
    A transient failure on those calls first re-attempts once under the bounded retry-once before the failed holding is isolated (`docs/local-models.md` §The local-model adapter seam).
  - An internal error — a panic anywhere below the job spine — is contained at the job seam and **fails the run** (a panic is a bug, contained but not isolated the way a per-holding model failure is): recorded `Failed` with the panic message, the tracker reaching its failed terminal state, any eligible standing checkpoint trail offerable for resume (`docs/portfolio-analysis.md` §Failure posture).

- **Output**
  - Full findings for every worked topic; any lower-priority topic the budget couldn't reach is a recorded degraded-input gap (lower conviction).
  - Evidence ledger with sources and timestamps.
  - Proposed follow-up and forward facts.

---

### Step 6d — Distill the research

- **As-built** (the research-loop slice)
  - The call is schema-constrained (the stub-era free-prose condense is retired), consolidates the full findings with the evidence ledger, and routes single-pass vs hierarchical deterministically.
  - A role-risk-only holding runs the fund agenda and a pure-consolidation distillation like any fund (the stub-time bypass is retired); its distillation emits no typed side-channel fields.

- **Data retrieved**
  - No new external data.

- **The consolidation call — single or hierarchical (built)**
  - The stage's full input is the findings from every worked topic, this run's evidence ledger (claims + sources), and — for a holding with a non-expired cache — each topic's seeded prior object merged into its own topic.
  - The orchestrator, never the model, sizes that full input to choose single-pass vs hierarchical **deterministically** — so growth *across* topics trips the hierarchical path rather than overflowing one call; the thresholds are compile-time constants — `OVERFLOW_THRESHOLD` (0.6) and `CHARS_PER_TOKEN` (3.0) in `distill.rs`, `NUM_CTX_DISTILL` (32,768) in `pipeline.rs` — exposed in no settings surface, so the knobs `docs/configuration.md` §Local Analysis Suite Configuration designs are not built. `NUM_CTX_DISTILL` is the budget's `num_ctx` only under a genuinely distinct fast model; with the default fast-falls-back-to-reasoner roster `distill_num_ctx` resolves to `NUM_CTX_INTERPRET` (131,072). The rendered single-pass and tier-1 prompts are then sized once more against the widest budget the adapter can issue (`issue_budget_chars` — the reasoner's on a distinct roster, the same budget on the default one) — the content sum omits the instruction scaffolding, the ledger conditions, and the per-claim render — and one that outgrows it takes the next smaller shape rather than issuing: hierarchical for the single pass, the pass-seam sub-distillation for a tier-1 call.
  - Every 6d call's rendered prompt is then sized **at issue** — the instruction scaffolding, ledger conditions, and distillates together, in chars, against the model's input budget — before any request exists: within the fast tier's budget it issues there, over it on the resident reasoner at the interpretation context (a model choice, never a `num_ctx` change), over the widest budget it is refused as an unclassified failure — a hard 6d failure that **isolates the holding** (the run continues, per §Failure posture), not the run; `distill_route` in `pipeline.rs`, canonical at `docs/local-models.md` §The local-model adapter seam. On the default roster the two rungs are one budget, so only the refusal is live.
  - That aggregate is what the orchestrator sizes to route, **not what any one call receives**: only the single-pass call sees every topic's findings whole, while hierarchical routing partitions them across the tier-1 → reduce shape below.
  - **Single-pass:** one call over every topic's findings, its output **keyed by topic** — globally reconciled, since the one call sees every topic — so each topic's group persists as that topic's next-run seed.
  - **Hierarchical (large input):** distill each topic tree separately (**tier-1**, feeding the reduce, not persisted raw), then one final combining call (**tier-2 reduce**) over the tier-1 objects — it emits the one combined object interpretation reads **and** the per-topic seed layer reconciled to the global winners, that reconciled layer (not the raw tier-1 output) persisting as the next-run seeds.
  - Either path preserves each claim's citations end to end.

- **Merging the seeded prior (Layer 2 cache) (built)**
  - **Within a topic** — each topic's prior object merges into that topic's fresh findings where the topic is first reduced (the tier-1 call when the run goes hierarchical, the single call when it is small), fresh superseding cached on conflict.
  - **Across topics** — the reduce (the tier-2 reduce, or that same single call) applies the same *newest-wins-by-claim/metric* rule **globally** and dedups sources, so a metric freshened under one topic supersedes a cached copy another topic carried forward, and nothing is double-counted or left conflicting.
  - **The persisted seeds inherit that reconciliation** — in the same reduce pass, each topic's next-run seed object is written already updated to those global winners, so no seed keeps a value another topic superseded and no later run can resurface it once the fresher topic goes dormant (why the reduce owns this match rather than a later app step: `docs/portfolio-analysis.md` §Starting parameters).
  - **Claim expiry is by claim, not object** — a cached claim past ~4 weeks by its own vintage that this run didn't re-confirm expires rather than riding forward; each surviving claim keeps its vintage and its fresh-vs-carried mark.

- **If a topic's own input overflows one call (built)**
  - **Trigger** — the fallback fires when the topic's **complete input** *summed* would exceed one distillation call: all its passes' findings, their evidence-ledger entries (claims + sources, which grow with the research, unlike the bounded thesis-ledger conditions seeded at 6c), and its retained prior. The sizing is measured on that whole aggregate, not pass by pass. It fires too when the topic's rendered tier-1 prompt itself outgrows the widest issuable budget, the content sum omitting the scaffolding (the consolidation-call bullet above).
  - **Map (one distillation call per pass)** — the topic sub-distills along its ≤3-pass seam: the model condenses each pass on its own into a compact per-pass object (that pass's findings *and* ledger entries together).
  - **Reduce (one more distillation call)** — a tree-level reduce then combines those per-pass objects **with the retained prior** into the topic's single tier-1 object, which joins the outer **tier-2 reduce** across topics like any other (the sub-distillation is invisible above this point). So building that topic's tier-1 object takes one map call per pass plus one reduce — two to four distillation calls (four for a full three-pass topic) — versus the single distillation call a non-overflowing topic uses, separate from the multi-turn calls the topic's research passes already spent at 6c.
  - **The sub-distillation cap (the only drop trigger)** — the tree-level reduce is not sized as a drop trigger (every 6d call is sized at issue — the consolidation-call bullet above); what bounds the fallback is a per-holding budget of 4 pass-level map calls (`SUB_DISTILLATION_CAP`) shared across every overflowing topic in the holding's distillation, spent in the agenda's topic order. When a topic's passes exceed what remains, its lowest-priority whole passes (the latest — the root pass ranks highest) fail-soft to a recorded gap, each dropped pass taking its findings and ledger entries with it while the retained prior still rides the tree reduce. Because a topic runs at most 3 passes, a lone overflowing topic never drops a pass; drops happen only when a second overflowing topic finds the budget partly spent.
  - **Budget exhausted** — a further overflowing topic drops every pass and yields no tier-1 object, so it counts as not analyzed this run: its prior object rides the tier-2 reduce as a dormant one — the same render, re-emitted on its own vintage — and the drop itself never names it unreconciled (a reduce that fails to re-emit it still does, like any dormant prior), so the seed survives; the topic issued no call, so it is not counted sub-distilled; with no prior the topic yields no object this run and its gap line stands alone. An overflow never costs a topic's seeded status (`docs/portfolio-analysis.md` §Starting parameters; ruled 2026-08-29, the 2026-08-24 review's §A4 edge).

- **Model**
  - Consolidates evidence.
  - Does not perform new searches.
  - Does not calculate financial numbers.

- **Typed outputs when supported (built)**
  - `research_forward_assumption`:
    - Sourced numeric forward fact.
    - Feeds only the shadow-mode target recompute (ruled 2026-08-24) — no engine target moves.
  - `validated_leading_indicator`:
    - Countable, dated, third-party indicator.
    - Must be absent from engine scoring.
    - Reaches later passes as ledger-driver evidence.
    - Carries `confirms_driver_id`; only an app-verified reference to a current ledger driver grants the cap-suppression anchor.
    - The old one-level conviction raise is retired.
  - `forensic_event`:
    - Primary-source fraud claim, advisory by ruling (2026-08-24) — cited attention evidence, never a hard trigger.
  - `pre_profit_execution_observations`:
    - Numeric operating facts quoted by a source.
    - Metric name and observation role.
    - Actual, guidance-low, guidance-high, point-guidance, or contextual-level role.
    - Higher-is-better, lower-is-better, or target-band direction.
    - Units, ISO reporting-period end, and reporting span (`quarter`, `half-year`, `full-year`, `year-to-date`, `point-in-time`, or `unknown`).
    - Company scope and publication date.
    - Source URL, the page's own sentence stating the value quoted verbatim, and confidence.
  - Backfill coverage when required:
    - Metric identity, exact reporting span, periods, and sources checked.
    - `complete`, `partial`, or `unscorable`.

- **Role-risk-only rule**
  - No target assumption.
  - No leading-indicator field.
  - Pure research consolidation only.

- **Output**
  - Two schema-validated artifacts, both emitted by the reduce (or the single-pass call) from one reconciliation, so they stay mutually consistent.
  - The combined distilled findings object interpretation reads — the cross-topic reduction of the per-topic layer.
  - The fresh per-topic seed layer, one object per analyzed topic, persisted as the next run's seed.
  - Each claim's optional `related_condition_id` — cited from the prompt's rendered ledger conditions, app-validated, and carried across a verbatim re-emission (same URL and claim text) from this run's earlier hops, or from the prior layer onto a cached claim only — the source-backed leg Step 6g honors for a qualitative trip.
  - The audit records seeded-vs-cold **per topic** with each seeding object's vintage — a standing topic can seed while a newly-activated conditional topic runs cold.

---

### Step 6e — Recalculate targets using validated research

- **As-built** (both legs live with the research-loop slice)
  - On the overlay side the step rewrites the **observation history** (with its rejected-row and backfill-attempt audit), the **execution read** derived from it, the **severe-deterioration** state, and the **rule consequences**; a single validated observation can move the history alone without changing the execution read or consequences. The **target / hurdle leg runs in shadow** (ruled 2026-08-24): the hypothetical recompute records on the audit and nothing splices into the baseline. The step never touches the **letter or grade sub-scores**, nor the overlay's **statement-derived legs** — the statement inputs (runway, burn, margins, share count) and the financing / economics / dilution states built from them — which no research observation moves.
  - Absent a validated forward fact and any accepted observation, everything 6b produced passes through unchanged into the verdict.
  - A role-risk-only holding skips the step entirely: it has no priced overlay, targets, or hurdle to refine.

- **Data retrieved**
  - No new data.

- **Pre-profit rule consequences (as-built — bind the engine stand-in arm)**
  - The overlay's deterioration states (computed at Step 6b) match a fixed set of consequences that act on the **engine stand-in arm only** — never on the model, whose own conviction and action persist as authored with any departure annotated. The letter grade is never among them: the overlay is conviction / risk / action context, not a grade component.
  - **Repeated execution miss** → engine conviction capped at Medium.
  - **Constrained runway** → Add and Add aggressively leave the engine action set.
  - **Severe deterioration** → engine conviction capped at Low, and the engine action set is limited to Trim or Sell all.
  - **One metric alone cannot force a sale** — the exit-only narrowing rides severe deterioration, which needs at least two warning legs with at least one from execution or economics; a lone miss or lone dilution is a single leg and never trips it.
  - Where these bind: the conviction ceiling is applied when the engine stand-in arm's conviction is assembled, and the action-set narrowing when the engine's per-holding action set is built for the action call — both at verdict assembly (Step 6f), in deterministic app code, not a model call. [note: the calculations that produce the states — the 5% / 20% miss thresholds, the repeated-miss window, and the economics / dilution / severe rules — are Step 6b's; see §Pre-profit overlay. This step re-feeds them only once research supplies new observations (below).]

- **Target and hurdle refinement (built — shadow mode, ruled 2026-08-24)**
  - Validate each research claim before it can move a number: reject malformed, unsourced, or nonnumeric claims, and hold both declarations to the primary-source fact-type whitelist; a `supplement` may fill only a missing structured value, its stated units magnitude-normalized deterministically first (a per-share fact rejects any magnitude token, an unscalable ambiguous unit rejects); the `supersede` leg is dormant by design (ruled 2026-08-27; revivable only if the channel is promoted and the consensus feed gains an as-of date): a `supersede` rejects against any present feed value — the consensus feed carries no as-of date to verify it as newer, so the structured value wins — and a `supersede` declared against an absent value is downgraded to a supplement fill, which the recorded rule names (canonical: `docs/portfolio-workflow.md` §Step 6e). Record every accepted or rejected rule.
  - The recompute is **shadow-only**: the engine computes the hypothetical refined targets and records the would-have outcome on the audit, but nothing splices into the baseline — the Step-6b targets and dead-money read always stand, and write-back promotion waits on manually inspected shadow cases.

- **Pre-profit observation validation (built — producer active)**
  - Each sourced observation a research row supplies is checked **structurally** before it enters the period history — a finite numeric value, units, an ISO reporting-period end, an explicit reporting span, an issuer scope, a source URL, an ISO publication date, a confidence in [0, 1], and a direction (polarity) consistent with the metric kind (the row also carries its actual-versus-guidance role and its publication date — the quoted page's own, never the fetch date — both read later when pairing, the date under the guidance vintage policy). An explicit Q / H / FY / YTD label must agree with the typed span; an ISO-only label carries no inferred duration, and `unknown` remains audit context without pairing. A **malformed** row (any of those legs bad) or a **duplicate** — the same metric identity, actual-versus-guidance role, reporting-period end, reporting span, source, publication date, and value as a stored observation or an earlier accepted row in the same batch, so a same-source revision, same-page conflict, or same-end fact over another span is never one — is rejected and logged with its reason; every other row is accepted and merged into the period history (append, sort, dedup). A structurally valid but as-yet-**unpaired** row — an actual with no matching guidance, the reverse, or either side with unknown span — is kept, not rejected: pairing into misses happens later in the execution read, and an unpaired row simply waits for a future match.
  - The two activation legs the slice owed are **built and binding**: **confirm the correct company** (the holding-identity cross-check — the fetched page must name the holding by symbol or a distinctive issuer-name token, generic corporate suffixes never qualifying) and **confirm the source states the number** (source-text corroboration inside the row's quoted source excerpt, which must itself appear verbatim in the page and is read with the page's own neighbours around it — at number boundaries and at the printed sign, so a value never corroborates off a longer number, a decimal it merely prefixes, or a print of the opposite sign such as `-41` or the accounting `(41)`, even when the quote is trimmed to the digits — and the excerpt must carry the declared metric's own language and state exactly one number, the value, a guidance-low / guidance-high row alone quoting a range's two endpoints, and that number must not read as the period the sentence names — a 1900–2099 year right after a period word such as `for` or `fiscal`, the range form when both endpoints read so; canonical: `docs/portfolio-workflow.md` §Step 6e). Every row's source page must have been fetched by this holding's own loop; an unevidenced call rejects every candidate.
  - Periods normalize to an ISO end plus the explicit span before the dedup key is taken, so spellings compare exactly without collapsing unlike durations that end on the same day.
  - An accepted row persists the prompt stamp it was admitted under (`admitted_under`, app-written at acceptance, outside the dedup key); the history is never re-admitted through a later filter, so a later, stricter contract leaves an older row telling itself apart by its stamp (canonical: `docs/portfolio-workflow.md` §Step 6e).

- **Cold-start and history-gap backfill (built)**
  - A backfill search is required on the first overlay-eligible full pass, and again whenever a previously used guidance metric-and-span identity has fewer than four comparable stored periods; unlike spans cannot discharge its depth. It searches the latest four reported periods, records the metric, exact span, every period and source checked, and marks its coverage complete, partial, or unscorable.
  - Missing history stays a gap — an observation that was not found is never inferred, and a required attempt that never reported is recorded as a gap.

- **Role-risk-only rule**
  - Skip the step: no price targets or priced-stock overlay exist to refine.

- **Model**
  - None.

- **Output — what leaves the step**
  - The **final target set** and **hurdle read** — always Step 6b's values (the assumption recompute is shadow-only).
  - The logged research assumption and its resolution — accepted or rejected, with the deciding rule — whenever distillation produced one.
  - The **final pre-profit overlay** — the merged observation history (and its rejected-row / backfill audit) and the execution read, severe-deterioration state, and rule consequences re-derived over it, ready to bind the engine stand-in arm at verdict assembly.

---

### Step 6f — Author the intrinsic verdict

Two model calls run in this step.
The interpretation call writes the intrinsic verdict; the action decision then picks the rung.

#### Interpretation call — exact inputs

- **Holding identity**
  - Symbol, issuer name, quantity, total cost basis, and total market value.
  - Position change since the prior run.
- **Engine grade and sub-scores**
  - The baseline letter, with a low-confidence marker when a sub-score was imputed.
  - Quality, valuation, and risk scores.
  - Momentum, labeled as market-setup context outside the letter.
- **Risk tier and the capital-efficiency read**
  - Hurdle state and hurdle rate.
  - Framed as evidence to weigh, not an instruction.
- **Fund context (priced funds only)**
  - Expense ratio, US share, and composite P/E coverage.
  - The closed-end price-vs-NAV line — rendered as an explicit `(gap)` line on this branch when the NAV read is missing.
  - The underlying CFTC COT positioning block, when the fund's underlying carries one.
- **Computed metrics**
  - Net margin, gross margin, revenue growth, and debt/equity.
  - Daily return volatility and trailing return.
  - P/E, P/S, and P/B.
  - Missing values shown as gaps.
- **Engine scenario targets**
  - One-month and twelve-month bear/base/bull prices.
  - Each horizon's methodology text.
- **Target provenance**
  - Anchor form: rate-anchored, current-multiple carry, or raw-percentile fallback.
  - Driver rung and consensus-row count.
  - Flat-driver, clamp-flattened, and dispersion-floor flags.
  - Guidance on how much signal each shape carries.
- **Final pre-profit overlay (when applicable)**
  - Engine states, matched rules, and the engine arm's conviction ceiling.
- **Options activity**
  - Put/call by volume and open interest, IV, and the IV skew — signed, its put-minus-call convention and unit stated on the line.
  - Labeled proxy-only, never a grade input.
- **Data gaps** — the dossier's degraded-input notes naming financial legs the gather could not resolve (e.g. an SEC CIK-mapping miss, an SEC company-facts fetch failure, an unwired fund-metadata source); distinct from the per-metric `(gap)` markers in the computed-metrics block above, which flag one missing computed value.
- **Market options sentiment** — the CBOE venue-level daily put/call backdrop, when Step 5 loaded one.
- **Forensic filings state** — only when the forensic sweep produced a state (an unrun sweep renders nothing; its reason rides the data gaps); then one of three forms: clean, unknown with its reason, or hard trigger tripped with the event rows and the hard-rule text (engine conviction capped Low; the add family barred from the engine action set).
- **Commodity context** — the run-level dated levels with their trailing delta, when the holding's dossier carries any.
- **Technology-event pre-flag** — the input-delta pre-flag section, only when it fired at Step 6b.
- **Distilled research** — the combined distilled findings object: this run's fresh findings merged with any seeded prior (fresh superseding cached), plus the typed leading indicator rendered as ledger-driver evidence where one validated; a run without a web stack carries a recorded research-unavailable gap instead.
- **House view (loaded only when the latest report is ≤ 7 ET days old; older drops the whole view)**
  - The latest report's Thesis, Investment Strategy, and Forward Outlook sections.
  - Recent report stances — up to three, the most recent by date: date, thesis stance, and risk posture.
  - Both are scope-limited to horizon reads and market setup — context for the outlook, never by itself a reason to exit the holding. (In the prompt the explicit caveat sits on the latest sections; the stances ride as a bare list.)
- **Continuity block**
  - Whether a prior verdict exists.
  - A parameter-boundary note when a priced prior verdict's grade-parameter stamp sits across a boundary that changed this holding's record on the prior's own branch — a band recalibration, or (fund only) the momentum re-homing; a stock across the fund-momentum boundary, a never-priced prior, and an unrecognized stamp get none, and neither does the input delta.
  - A second parameter-boundary note when a priced prior verdict's scenario-target stamp sits across a target-history row touching its branch, naming the union of horizons those rows can have moved; the current stamp, an unrecognized one (`targets-v4` included — the history starts at the pre-run `targets-v5` anchor), and a prior with no target record get none.
    Current `targets-v6` appends the complete-exchange fund-input boundary: a v5 fund prior names both horizons while a v5 stock prior stays silent, and the same distinction reaches the input delta.
  - The semantic prior-analysis recall — the Step-6a hits, when any.
  - The rendered input delta with its what-changed-entry rules (the vocabulary is Step 6b §Input delta; the attribution check is Step 6g). With a prior verdict, a firing technology-event pre-flag, narrative read, hard forensic state, or rendered latest-report sections reaches the model twice — as its own section and as a delta row; a debut carries no delta rows, and a house view reduced to the recent-stance list earns none.
- **Retrospective (when a prior priced verdict exists)**
  - The prior run's engine arm in full: grade, sub-scores, targets, conviction, outlook, action.
  - The prior run's model arm in full, labeled as the model's own.
  - The price move since the prior read.
  - The holding's matured scoreboard lines.
- **Prior thesis ledger** — one ledger per holding, model-authored on the prior run and shared by both arms (not an engine-arm or model-arm ledger of its own).
  - The prior ledger as a **model-facing projection** (not the complete persisted record) — thesis (original + current), key drivers, the whole bear/base/bull monitor, and every falsifier and trigger (both roles), each with its statement plus, for quantitative ones, the machine core and current breach streak, plus the research-supported mark where a fresh distilled claim cites the condition this run (Step 6g §Ledger validation). Unscoped, unlike the house view and retrospective above. Held out of the prompt: the app-owned bookkeeping (condition ids, supersession lineage, downgrade/trip flags, the rest of the evaluation state, the authored band relation) and the model-authored falsifier `technology_class` tag.
  - Beside it, **this run's engine condition evaluation**: the engine's deterministic re-evaluation of that ledger's *quantitative* conditions against this run's computed surface — each crossing tagged confirmed or first-breach, plus the typed unevaluable notes. The engine evaluates the conditions; it does not author the ledger.
  - The **engine-series vocabulary** the rewritten ledger's quantitative conditions must use — the closed labels, none naming a basis — with **this run's statement basis** for the flow family (TTM, SEC annual, or none), the two balance-sheet instants named as such, so flow-series thresholds are authored on the basis Step 6g evaluates them against — and which balance sheet supplied the two instants' equity this run (FMP's quarterly, SEC's annual, or none), the instants' own continuity stamp, so their thresholds are authored on the source Step 6g gates on.
- **Deliberately excluded**
  - The investor profile.
  - The engine's current-run stand-in outlook, conviction, and action picks.
  - Raw statements, filings, and price-bar series — only computed values and distilled research reach the model.
- **Evidence sections in the prompt since the evidence-legs slice**
  - The implied-expectations range (beside the forward outlook, never a gate).
  - The narrative-versus-reality read, with its engine-matched soft rule when the hype cap fired.
  - The FINRA short-interest read — positioning evidence on the options signal's precedent, held out of the grade.
  - The same-stock option overlay (this prompt and the action prompt).
- **Designed additions not yet in the prompt**
  - Absolute street opinions.
  - Insider and congressional activity — gathered at Step 6a once their data legs land; positioning evidence on the same precedent.
- **Role-risk branch — what differs (role-risk-only holdings)**
  - Renders: holding identity and position change; the classification, structural flag, exposure tilt, expense ratio, and observable risk from the fund readout; the closed-end price-vs-NAV line (silent on a gap — the gap rides the evidence gaps instead); the evidence gaps; the distilled research; the CFTC COT positioning block; the CBOE backdrop; the house-view sections without the recent-stance list; a continuity block limited to the semantic recall and the input delta (no retrospective, no band note); and the prior ledger with the fund-flavored rewrite addendum.
  - Not rendered on this branch: engine grade, sub-scores, computed metrics, targets, provenance, options activity, forensic filings, commodity context, short interest, the option overlay, the narrative and implied-expectations reads, and the technology-event pre-flag.

#### Interpretation call — what the model authors

- **The model arm (unrestricted; scored later against the engine baseline)**
  - Its own four sub-scores; the app derives its letter from them through the shared cutoffs.
  - Its own one-month and twelve-month target bands.
  - Its own conviction.
  - A retrospective self-assessment.
- **Shared verdict fields**
  - Short-, mid-, and long-term outlook.
  - Financial-health explanation.
  - Price-target rationale.
- **The rewritten thesis ledger**
  - Standing thesis and key drivers.
  - Bear, base, and bull monitor with probability weights.
  - Quantitative and qualitative falsifiers.
  - Add, trim, and sell triggers.
  - Role-risk-only ledger uses condition-only scenarios and Trim/Sell triggers.
- **The intrinsic what-changed explanation.**

- **For a role-risk-only holding the same call authors instead**
  - The portfolio-independent **role** read.
  - The **updated reduced fund ledger** — condition-only scenarios and Trim/Sell triggers only.
  - The intrinsic **what-changed** line.
  - No letter, target, or conviction.
  - [note: exposure tilt, observable risk, expense drag, the structural flag, and the evidence gaps are **engine-supplied** from the fund readout, not authored by this call — they ride onto the branch verdict beside the model's three fields.]

- **Model boundaries**
  - The engine arm is app-stamped; nothing the model returns can alter an engine value, overlay state, or monitor stamp.
  - The model arm is its own: validated structurally and on its declared numeric domain — sub-scores 0–100, target legs finite and positive; an off-domain response is rejected and re-issued once (`docs/portfolio-analysis.md` §The holding verdict) — never checked against the engine's numbers.
  - Engine caps and ceilings bind the engine arm and annotate the model's departures; they never clamp the model's values.
  - The rewritten **ledger is the exception to "the model's output stands"**: model-authored here but app-validated at Step 6g, not preserved like the model arm. There the app clears any tripped/fired claim no confirmed engine crossing (or, for a qualitative condition, no source-backed finding) supports, downgrades a non-executable quantitative core to qualitative, and owns every condition id and its lineage across the rewrite (Step 6g §Ledger validation). What is preserved exactly is the model *arm* — its sub-scores, targets, and conviction; what the app corrects is unsupported ledger claims and structure.
  - Cannot see the investor profile.
  - Does not choose an action — the dedicated action decision below does.

#### Action decision (second model call, same step)

- The profile enters the job here and nowhere else.
- Tunnel vision is stated in the prompt: no whole-book input exists, and a separate planning stage reconciles the book later.

- **Exact inputs**
  - Holding identity: symbol, name, quantity, total cost basis, and total market value.
  - Unrealized P/L, with the tax framing flagged as a user consideration, never the mover.
  - The prior run's action, as a continuity baseline (move only on materially moved evidence).
  - Priced digest: engine arm grade, sub-scores, risk tier, and dead-money state; model arm letter and sub-scores; the verdict's conviction and horizon outlook; implied one-month and twelve-month bear/base/bull moves as percentages against the current price for both arms — the engine's under a one-line target provenance, the model's as its own authored band, the gap and as-authored tag rules canonical at `portfolio-analysis.md` §Portfolio action; the financial summary; the pre-profit overlay when present.
  - Role-risk digest: class label, role, exposure tilt, expense drag, observable risk, structural flag, the closed-end price-vs-NAV line when one exists, and evidence gaps.
  - The forensic filings state and the commodity context, rendered for both digests when the dossier carries them — the role-risk interpretation call renders neither, so for a role-risk holding they reach the model only here.
  - The same-stock option overlay (shared with the interpretation call).
  - The engine's per-holding action set, shown as evidence with the engine's own pick withheld.
  - The investor profile: objective, risk tolerance, horizon, and tax posture — without the cash row.
- **Deliberately excluded**
  - House view, research, computed metrics, and absolute target prices (an off-scale model leg printing its authored value beside its tag is the one exception — behind the decode gate reachable only for a finite positive leg whose move from spot overflows the percentage arithmetic, never for a non-finite or non-positive leg).
  - Every book-level value: cash, weights, concentration, other holdings.
- **Returns**
  - One rung from the fixed ladder plus a short rationale (prompted as one sentence; an empty rationale fails the holding, isolated like any 6c–6f failure — the run continues).
  - No target weight, share count, or dollar figure — sizing belongs to the portfolio planner.
  - A rung outside the engine set persists as authored, annotated on the audit.

- **Output of Step 6f**
  - Proposed intrinsic verdict with both arms.
  - Rewritten thesis ledger.
  - What-changed audit.
  - The holding's action and rationale.

---

### Step 6g — Validate continuity and checkpoint

- **As-built**
  - The validators here run every run, all legs live with the research loop: the what-changed **attribution** check (external rows resolve against the rendered input delta, a sourced research finding, or the logged forward assumption — distillation-validated and sourced, its Step-6e shadow resolution recorded on the audit's research record beside it and never a condition of the row — or downgrade to self-correction with a logged reason), the two-arm stamping, the engine-series ledger validation, the qualitative-trip → sourced-research leg, the overlay caps, the attention clear-and-acknowledge, and the per-holding checkpoint write.

- **Data retrieved**
  - No new data.

- **Two-arm stamping rule**
  - Engine values are app-stamped directly, never echoed through the model.
  - Nothing the model returns can alter an engine grade, target, overlay value, or monitor stamp.
  - The model arm's own numbers are validated structurally and on their declared domain, never compared against the engine's.

- **What-changed validation (built)**
  - The what-changed audit is typed rows beside the prose (kind, old → new, attribution, evidence), and every claimed external change must map to one of:
    - An input-delta entry (by bracketed id or label verbatim).
    - A sourced research finding.
    - The logged forward assumption — distillation-validated and sourced; its Step-6e shadow resolution (the would-have line or the failed condition) records on the audit's research record beside it and never conditions the row.
  - An unsupported external change is downgraded to a labeled self-correction with a logged reason; hard schema failure is reserved for structurally malformed rows.

- **Conviction and cap handling**
  - The model's conviction is its own; no app recalculation, ceiling, or clamp touches it.
  - The old one-level conviction raise and its re-derivation are retired.
  - Matched cap rules record as audit annotations that bind the **engine stand-in arm only** (the overlay-derived caps are computed at Step 6b, their mechanics detailed at Step 6e §Pre-profit rule consequences). Their as-built status:
    - **Severe deterioration** (→ Low ceiling, engine set limited to Trim or Sell all) is **live** — its legs are statement-derived (economics, financing/runway, dilution), so it can trip without research.
    - **Repeated execution miss** (→ Medium ceiling) is **live** — its execution read pairs the app-validated research observations at Step 6e.
    - **Hard forensic trip** (→ Low ceiling, Add rungs leave the set) is **live for the filing kinds** — the item-classified restatement / auditor-change sweep runs at dossier assembly; the validated `forensic_event` research claim (the fraud kind's sole producer) is **advisory by ruling (2026-08-24)** — cited attention evidence and a model-arm input that never trips the hard rule.
  - The strictest matched ceiling wins on the engine arm.
  - A model value past a ceiling renders beside the recorded rule.
  - Model prose cannot create an overlay warning state — the overlay is computed deterministically from statements and app-validated observations.
  - Grade remains unchanged by these caps.

- **Ledger validation** (built — the seam runs every run)
  - Tripped quantitative condition must map to a confirmed engine crossing.
  - Tripped qualitative condition must map to sourced research — a fresh distilled claim citing the condition (`related_condition_id`), surfaced to the interpretation prompt as a research-supported mark — and a trip with no such finding behind it clears with a logged reason.
  - New quantitative conditions must resolve to an engine series.
  - Unresolvable condition becomes qualitative (downgraded and logged, never dropped).
  - App assigns and preserves condition IDs.
  - Changed machine logic starts a fresh evaluation streak.
  - A fresh streak starts stamped with the statement basis and, on a balance-sheet instant, the equity source the prompt stated, so its first evaluation can already disagree with a flip.

- **Attention handling**
  - Successful full pass clears the prior attention flag.
  - Record the observation the pass acknowledged.
  - The same observation cannot immediately raise the flag again.

- **Output**
  - Validated intrinsic verdict and thesis ledger.
  - Completed per-holding checkpoint (verdict + audit row with the holding's telemetry and data-health contribution, plus the refreshed run-level keyed identities).

---

## Step 7 — Roll up the run and score past decisions

The post-loop stage — by now every action that exists is final, whether set in this run's per-holding loop, carried from a prior run, or rule-demoted by the app, so this step makes none. It does two independent things: **roll up** the finished run into a book-level summary, and run **outcome learning**, which grades how *earlier* decisions have actually turned out.

### Roll-up

A descriptive, book-level summary of the finished run — for the results display and the stored run record. It decides and drives nothing.

- **Calculations**
  - Verdict counts by disposition (graded, role-risk-only, not-rated, insufficient-evidence — role-risk-only kept separate from graded).
  - Largest single-position weight and cash weight (descriptive reads only; both read zero over an unusable total or an overflowed quotient).
  - Positions closed since the prior run, acknowledged rather than dropped.
  - Run-level data-health read — target-provenance and degraded-input aggregates, the persisted research-degraded holding and gap counts, the generation-health signals (context-pressure and output-length-stop), and the run's attention flag.
  - A deterministic one-line run overview string.

### Outcome learning

Outcome learning has two halves that share one unit, the **decision episode** — a bounded twelve-month instrument measuring how a single recommendation actually turned out. One half **scores** *earlier* episodes as their outcome windows come due; the other is **where this run's decision becomes an episode**, the raw material later calibration reads. For each holding the run compares its recommendation state against the prior run's and **opens a new episode when that state has changed** — a verdict-**branch flip** (priced ↔ role-risk) or a change in the **action** — otherwise extending the holding's still-active episode. Each episode carries **outcome labels** due at 1 / 3 / 6 / 12 months (both terms are defined in Important terms): when a window arrives its label is scored, or — if price coverage is missing — held pending inside a grace and closed unscorable past it.

- **Data retrieved**
  - FMP dated-EOD bars for maturing outcome episodes.
  - FMP dividends for maturing outcome episodes.

- **Logic**
  - Tag net alignment from the holdings diff — only for still-untagged episodes anchored to the immediately-prior run (that diff observed only that move); nothing is tagged on a first run.
  - Mature any window labels whose dates have arrived, including for symbols no longer held.
  - Each matured (scored) label measures, from split-adjusted daily closes: the price-only return (the always-present cross-entry common basis) and the maximum drawdown, plus — each recorded with a typed gap when its source is missing — the dividend-inclusive total return (the primary basis) and the price-only spreads vs the market (`^GSPC`) and the entry-stamped sector; on the 12-month window a confirmed ledger falsifier attached to the episode that carried its condition at run start (not one that landed post-maturity), whose bear-line basis resolves, additionally carries its signed trading-day lead time to the first close below that line, or `no-material-drawdown`.
    A same-run successor never steals its predecessor's crossing, and a confirmation dated after the episode window is typed post-maturity and excluded rather than clamped onto the final bar.
  - A failed price refresh leaves the label pending while inside the coverage grace; past the grace it closes as a typed unscorable label rather than staying pending. A failed dividend pull instead degrades to a price-only label, never blocking maturation. The series admits usable closes only; a covered window whose price arithmetic does not finish finite takes the same pending-then-typed-close path, and a benchmark leg that does not finish finite reads unavailable with its gap (`docs/portfolio-analysis.md` §Outcome learning).
  - Append or extend this run's decision episodes — the run's episode-creation step: open a new episode when a holding's recommendation state changed since the prior run (a verdict-branch flip or an action change), otherwise extend the still-active episode.
  - A holding's first analysis opens a debut episode; an abstention extends the standing episode without opening one; a reaffirmation after the episode has matured records nothing.
  - A thesis-change trigger is live off the attribution validator's audit — a resolved thesis-level external row or any labeled self-correction on a fresh pass; wording-only thesis edits never open an episode.
  - Derive the scorecard reads over the updated episode set — the reads below.

- **Scorecard reads** (derived deterministically over the updated episode set — engine-computed, never model-judged; of them the roll-up surfaces only the head-to-head and outlook-direction reads, as the model-vs-engine scoreboard, and each holding's own matured window lines ride back into its next interpretation — they decide nothing on their own, the calibration loop they feed only ever proposes)
  - Both arms are scored, separately: the engine baseline and the unrestricted model arm each froze their own targets and outlook on the episode at open, and the reads below score each arm on its own — the target-band read is the one place they meet directly head-to-head — because grading the model against the baseline is the whole point of the two-arm design.
  - **Target-band calibration** — the bear–bull band's coverage of the realized price against its declared nominal 80%, an interval score rewarding calibration and sharpness together, and the base case's mean signed error; scored on the price-only label at the **1- and 12-month windows only** (each band against its matching window — the 3- and 6-month labels are never band-scored), over vintage-fresh priced episodes, split by target-parameter version so a recalibration never mixes bases. The same scorer runs unchanged for the engine bands and for the model's frozen bands.
  - **Engine-vs-model head-to-head** — that same interval score and coverage for both arms over the paired population alone (the 1- and 12-month episodes where both arms carried the band and the window scored), so neither arm is graded on an easier sample; this is the only read the two arms are directly compared on.
  - **Outlook direction hit-rate** — each arm's short / mid / long read scored against the realized price sign at its mapped window (short → 1-month, mid → 6-month, long → 12-month); a flat outcome scores a directional call as a miss, and a neutral read is counted beside the hit-rate, never inside it.
  - **Action cohorts** — mean total and price return plus vs-market / vs-sector spreads, grouped by the action rung recorded at episode creation, across all four windows: the cohort spreads the action ranking is read from (do the add cohorts out-return the hold cohort, and hold the trim / sell cohorts). Computed over model-chosen priced episodes — a vintage-fresh intrinsic-layer set reported beside the all-model-chosen final-action set; role-risk-only and rule-demoted episodes are counted in their own classes, out of the pooled read.
  - **Falsifier lead times** — the 12-month bear-line crossings above, surfaced per episode.
  - **Proposal eligibility** — a gate counting the unique holdings with a scored matured window against a bar (drafted 30). **As-built the gate is built but the proposals are not**: below the bar the pass records the typed below-bar note and proposes nothing, and above it the proposal statistics still land with a later slice once enough matured data exists — and even then the loop only proposes, never auto-applies.
  - **Self-correction accumulation** — the 6g validator's post-validation labels (authored plus downgraded), seeded onto the episode a fresh pass opens and accumulated across extensions; a thesis-level external row or any labeled self-correction also opens an episode with the action unchanged (the standing-thesis leg, live).

---

## Step 8 — Save the run and learning history

- **Data stored** (the whole run persists as one serialized blob)
  - Normalized holdings snapshot used by the run.
  - Every intrinsic verdict.
  - Every portfolio action and its rationale.
  - Thesis ledgers and condition evaluation states.
  - Analysis vintages (attention flags live only in the quick-check store).
  - Portfolio roll-up.
  - Source labels, plus the research audit's source URLs with their retrieval timestamps.
  - Engine calculations, each holding's categorical position-change tag, and the roll-up's exited positions (the full position delta — prior quantity and cost basis — is runtime-only; per-value input-delta attribution is the designed input-delta validator's).
  - Every priced stock's pre-profit overlay record — the runway, economics, dilution, and severe-deterioration states computed live from statements, with the conviction, action, and cap rules they fire.
  - What-changed audits.
  - The outcome-learning records for this run — the opened-episode notes, the symbols whose episode this run extended, the net-alignment tags, the matured window labels, the symbols with a window still pending on a price-coverage gap, and the derived scorecard reads (detailed in Step 7's outcome learning).
  - Model, prompt, schema, parameter, and evidence-floor versions.
  - Degraded-input flags.
  - The accepted pre-profit observation history (period-end-and-span keyed, now research-fed, each row stamped with the prompt version it was admitted under) and the backfill legs, carried and extended run to run; the rejected-observation list, by contrast, is rebuilt from the current candidate batch on each re-analysis and never read from the prior overlay (a carried verdict carries its prior audit whole, rejected rows included).
  - Per-topic research-reuse decisions (seeded-from-cache vs cold, each with its seeding vintage) and accepted / rejected research assumptions with their resolutions; the distilled findings themselves persist — the combined cross-topic object on the run audit record, and the reconciled per-topic seed layer as the next run's seeds (`docs/storage.md` §Local Analysis Suite Storage).

- **Decision-episode logic**
  - Decided in Step 7's outcome learning (the open / extend rule lives there); this step only persists the resulting episodes.

- **Episode contents**
  - Anchor date.
  - Intrinsic-analysis vintage.
  - The action.
  - Decision-time calibration snapshot (priced branch only): both arms' targets, sub-scores, outlook, and conviction, plus the engine arm's grade, hurdle, dead-money, and cap signals; its DGS2 print is recovered from that same intrinsic hurdle and risk-tier premium on a carried rule-demotion open, never borrowed from the consuming run.
  - Sector identity for later benchmark comparison.
  - Grade and target parameter versions.
  - `model-chosen` or `rule-demoted` action source.

- **Retention**
  - Keep newest 30 Portfolio Analysis runs.
  - Keep outcome episodes independently until their labels mature.
  - Freeze matured episodes into their own capped archive.

- **Embedding model**
  - Embed a calibration learning only when this run records newly matured outcome-window labels — keyed to window-label maturation, not to an episode freezing into the archive, and not fired every run.
  - Per-holding thesis, read, and action summaries embed as run-pruned summary rows — fresh-vintage analyzed verdicts only.
  - Store vectors only in Portfolio Analysis memory.
  - Failed embedding drops only that memory row.
  - Persisted run still succeeds.

- **Output**
  - Durable run and audit record.
  - Updated decision-episode store.
  - Searchable Portfolio Analysis memory.

---

## Step 9 — Display the result

Display is a **pure read**: the frontend invokes read-only commands that return the persisted run blob verbatim and renders it — no model runs, and the backend shapes nothing (one presentational Vue page computes every derived read from the returned struct; the app layer owns the invokes).

- **Data retrieved** (read-only commands — no model, no view-model shaping)
  - The **latest run** — the whole persisted run blob, selected **id-primary (insertion order), never `created_at`**, so a backward clock step can never hand the page to a prior run while the just-saved one sits invisible.
  - An **older run by id** for the read-only history view — a pure read, with no re-run entry point.
  - The **run-summary listing** for the sidebar (columns only, never blobs).
  - The **latest standalone holdings snapshot**, when present — a separate single-row store distinct from the snapshot inside each run and never read by the job; when a run exists it feeds the dual-vintage comparison, and with no run yet it is the page body on its own.
  - The **latest quick-check state**, loaded alongside — the overlay that badges the latest live view (below), applied only when its swept run matches the rendered run.
  - **As-built** an unparseable stored run costs only its own surface: the latest read skips it to the next-newest, an id fetch reads it as not-found, and the listing still emits its row marked **unreadable** with zeroed counts — never dropping the history listing or the next run's baseline. The write refuses a record that would not read back — naming the holding where the value sits in a per-holding record — so a non-finite required value fails the run rather than landing such a row (`docs/portfolio-analysis.md` §Failure posture).

- **Per-holding display — priced branch** (the two arms in a paired side-by-side grid)
  - **Engine-baseline arm** — the letter sub-scores (quality / valuation / risk) plus a divider-separated **Setup** tile (the market-setup read, deliberately outside the letter), the engine conviction meter, the 1- and 12-month targets (base plus bear–bull band, each shown only when the engine authored it), the engine outlook (short / mid / long), the engine's own action, and a target-methodology reveal showing each authored horizon's methodology; the card-head letter is the engine (canonical) grade.
  - **Model-view arm** — the model's own letter, sub-scores, conviction, 1- and 12-month target bands (always present on this arm), outlook, and action.
  - **Divergence tags** — a quiet **≠ engine** tag rides the model arm wherever it departs from the baseline on **conviction, outlook, or action**: a display cue for where the arms differ, distinct from the scoreboard's scoring (which grades the target bands head-to-head and the outlooks per arm, and leaves conviction and action unscored). The letter, sub-scores, and target bands carry no tag, and an authored **inverted-band** note is a data-integrity flag rather than a divergence.
  - **Standing thesis** (clamped, with a reveal) and the **thesis monitor** — each scenario's probability, engine target (when non-null), and conditions, plus the improve / must-not-break goalposts.
  - **Action + rationale** as a full-width row beneath the arms, with the position weight and — when present — the same-stock **options-activity** signal (put/call volume and open interest, ATM IV, and the put − call IV skew, signed).
  - **Financial summary** and the **model retrospective** (the model arm's self-assessment), the **what-changed** footer, the holding's **matured scoreboard lines**, and its categorical position-change tag.
  - Plus the per-card **selection control**, **attention flag**, and **analysis-vintage stamp** — the badge surface below.
  - [note: the engine's persisted `risk_tier` and dead-money reads, the ledger's key drivers, and the authored band relation are carried on the verdict but rendered nowhere today.]

- **Card badges** (non-blocking, all quiet except the two amber ones noted), split by what drives them
  - **Quick-check overlay** — **attention** (the one amber, actionable badge; the sweep's four triggers: falsifier breached, trigger fired, hurdle newly fails, band relation changed), **evidence event(s)**, and **sweep degraded** (naming the `unknown` families it couldn't vouch for). These render **only on the latest live view whose swept run matches the rendered run** — never on a historical past-run view.
  - **Persisted verdict state** — the **carried / stale-vintage** stamp (stale past the over-age boundary), **side reversed** (amber; the carried long-authored verdict now sits on a net-short position — a directional verdict is only ever authored for a long, so the compromised carry stays visible), and **add-demoted-to-hold** (the over-age rule demotion). These read off the stored verdict, so they render outside the overlay guard — **including on a historical view**.
  - A held position with **no prior verdict to carry** renders as a distinct **not-analyzed placeholder** card, selectable to grade on the next run — never a fabricated verdict.

- **Role-risk-only display** (the discriminated union's unpriceable branch)
  - Role summary, top exposure tilts, expense drag, realized-vol observable risk, evidence gaps, the structural-flag badge, and the action + rationale; standing thesis and a condition-only monitor (no engine target).
  - **No empty priced fields** — the branch's type carries no grade, targets, conviction, sub-scores, or arm views at all, so the priced fields are absent by construction, never blanked.

- **Portfolio roll-up display**
  - A **key-figures strip** — account value, positions, the disposition counts (graded and not-rated always; role/risk and insufficient only when non-zero), cash weight, and top-position weight.
  - The **roll-up card** — the overview line, the **data-health** read (an amber attention tag plus its summary), the **model-vs-engine scoreboard** (paired head-to-head interval scores and per-arm outlook-direction hit-rates), and the positions **closed since last run**.
  - **As-built** the scoreboard renders **only once episodes have matured** — a run with nothing scored omits the block rather than showing a labeled *pending* state, so absence reads the same as no outcome records; the target-band calibration reads are carried but not rendered, and not-rated / insufficient reasons render on their own cards, not here.
  - [note: the sidebar's per-run label is **`graded N`** — it names the priced (graded) count it renders; role-risk-only holdings are excluded from it and appear only in the open run's key-figures strip, never pooled into the sidebar number (ruled 2026-08-18; the former `rated N` wording read broader than its number).]

- **Holdings display — dual vintage**
  - The standalone holdings pull is **view-only, never merged into the run-anchored cards**, and the **frontend** decides freshness (the backend hands over both timestamped payloads and compares nothing): when the pull is newer than the run, a separate **Current holdings** section renders **above** the verdict cards, stamped with both vintages and carrying presence-only churn tags (*new · not in last analysis* / *no longer held*).
  - The older analysis cards are never mutated, and the whole comparison is suppressed on a historical view.

- **Sorting** (display-only — reorders already-computed cards, computes nothing)
  - Four keys — **Value**, **$ gain**, **% gain**, **Cash invested** — a stable in-place sort with an alphabetical ticker tie-break, nulls last, and the last-used key persisted; shown only with more than one card. It sorts every card — verdict cards and not-analyzed placeholders together, on the same position-level keys (ruled 2026-08-18). The current-holdings table carries its own independent column sort.

- **Read-only past-run view** (any sidebar row but the newest readable one)
  - The older run renders on the same page with every trigger locked — run analysis, pull holdings, and quick check all disabled with the reason stated — no selection controls, and the current-holdings comparison suppressed.
  - A quiet informational **vintage banner** names the run's date and carries **Back to latest**.

- **Model**
  - None — no display command invokes a model; the page is a deterministic render of the persisted run.

---

# Quick check

`Load last run → Refresh monitorable data → Evaluate ledgers → Raise warnings → Save state`

- **As-built**
  - The quick check **runs today, engine-only** — no model call, no web research, no Schwab pull. Every leg below is live except the FINRA short-interest refresh, which is dormant (its own subsection below).
  - It is the between-run freshness safeguard the 2026-08-16 badge ruling leans on: it **warns without deciding**, and its warnings ride as non-blocking card badges, never a forced re-analysis.
  - Its gate is presence-only — the local-model configuration plus FMP / FRED credential presence, with no daemon probe — and the Schwab connection is a precondition even though no Schwab call is made; it holds the single global run slot like any job.

- **Purpose**
  - Keep existing thesis ledgers alive between full analyses.
  - Warn without rewriting decisions.

- **Data retrieved from local storage**
  - Last analysis run’s holdings snapshot.
  - Existing thesis ledgers.
  - Stored target inputs and rate anchors — the last full pass's drivers, spread / raw-multiple percentiles, spot, and trailing-TTM dividend proxy (the quick-check basis the between-run engine re-anchors against).
  - No fresh Schwab holdings pull.

- **Shared data refreshed**
  - Current holding prices from FMP — the live `quote` plus dated-EOD closes (two FMP calls per holding; the sweep never reads the shared price cache).
  - The dated-EOD fetch widens beyond 180 days only to recover an older carried split anchor; trailing-return and return-volatility conditions are carved back to the full pass's inclusive 180-day UTC range.
  - `DGS2` and `DGS10` from FRED (one print each).
  - A failed rate pull fails soft to the freshest cached print — a prior quick check's own print first, else the last run's — eligible only within a drafted ~1-week bound (`RATE_CACHE_MAX_AGE_DAYS = 7`).
  - No eligible rate cache → the rate-dependent families read `unknown`.
  - A failed price refresh has no cache to fall to → that holding's market family reads `unknown` and its price-dependent reads skip.

- **Per-stock data refreshed**
  - Always, per stock: the SEC EDGAR filing check (CIK-gated), an analyst-estimate snapshot (the revision preflight), and an earnings-history re-pull.
  - Only after the EDGAR check surfaces a **new filing**: an income-statement, balance-sheet, and dividends re-pull, so a filing-cadence condition's fresh observation arrives with the value it reads. [note: no cash-flow re-pull exists; the hurdle's payout term is the dividend leg alone.]
  - Only for a holding carrying a **standing technology-class falsifier**: a `news/stock` pull (the qualifying-news-seed leg).
  - Unresolved SEC CIK → filing family becomes `unknown`.

- **FINRA short-interest refresh (dormant)**
  - Designed as a conditional once-per-run consolidated-file pull, read only when some holding carries a validated short-interest-fed condition.
  - The FINRA adapter itself is built (the full run's dossier leg uses it), but this sweep leg stays **dormant**: the closed engine series surface has no short-interest series, so no condition can validate as short-interest-fed and the trigger never arms. It activates only when a short-interest series joins the surface.

- **Fund data refreshed**
  - `etf/info` plus **both** the sector and country weight sets — fetched unconditionally for every fund (bond and commodity funds included).
  - The equity / condition gating is evaluation-side, not fetch-side.
  - The `profile` read is full-pass only — the sweep never re-runs closed-end detection.

- **Calculations**
  - Evaluate every machine-checkable falsifier and trigger — **quantitative conditions only** (qualitative falsifiers are research-checkable, so they stay a full pass's job).
  - Market-data condition → every pass.
  - Filing condition → only after a new filing-style observation.
  - Re-anchor stored v2 multiples using current `DGS10`.
  - Recalculate the dead-money hurdle using current price and `DGS2`.
  - Check whether price's relation to the stored bear–bull band changed from its authored stamp.
  - Detect new evidence events.

- **Per-family result**
  - `fresh_clear`:
    - Retrieval succeeded.
    - Every condition the family covers actually evaluated.
    - No condition fired.
  - `flagged`:
    - Confirmed condition, trigger, hurdle change, or band-relation change fired.
  - `unknown`:
    - Retrieval failed and the cache could not prove the family current.
    - Or a covered condition could not be resolved this sweep.

- **Warning logic**
  - Confirmed falsifier or trigger → amber attention flag.
  - Newly failing dead-money read → amber attention flag that reports the bull-case total return against the hurdle, the same leg that decides `fails`.
  - Band relation changed since authoring (left, re-entered, or crossed the band) → amber attention flag.
  - New earnings, filing, revision, or qualifying news → quiet evidence-event badge.
  - Fund mandate, expense, or major exposure change → quiet evidence-event badge.

- **State updates**
  - Advance a condition streak only on a distinct new observation (a new trading day's print, a new filing).
  - A selective run's carried-tail sweep reuses the parent run's pinned instant and ET session for every badge and evaluation-state date.
  - Persist first-breach and confirmation state.
  - Keep model-authored thesis and triggers frozen — the sweep evaluates against a clone and writes back only each condition's evaluation state.
  - The **standalone** sweep persists its whole result only to the quick check's **own single-row store — it never creates or updates a run**, so it can't contaminate run history, `latest_run`, or the diff baseline. [note: quick-check eval-state does reach `portfolio_runs`, but only downstream and by design — a later **selective** analysis overlays its in-run carried-tail sweep's eval-state onto the carried verdicts it saves in the new run's blob (the cross-run condition-state chaining); the quick check never writes to `portfolio_runs` itself.]

- **Cannot**
  - Rewrite a grade.
  - Rewrite conviction.
  - Rewrite the thesis ledger.
  - Change a portfolio action.
  - Perform web research.

- **Model**
  - None.
  - Can run while the model server is configured but offline.

- **Selective-run effect** (badges, never forced re-analysis — ruled 2026-08-16)
  - A selective run analyzes **strictly the user's selection**; the sweep's findings never expand the work list.
  - A `flagged` holding rides its card as a non-blocking amber attention badge.
  - An `unknown` family rides its card as a quiet degraded-sweep badge — a verdict the sweep couldn't check is badged, never silently trusted on the sweep's silence.
  - The user acts on a badge by selecting the holding (or running a full analysis); an urgent single-holding run is never blocked by the rest of the book.

- **Output**
  - Attention flags.
  - Evidence-event and degraded-sweep badges.
  - Updated machine-condition state.

---

# Pull holdings

`Check Schwab → Fetch positions → Normalize → Save snapshot → Display`

- **Purpose**
  - View current holdings without running analysis.
  - Requires a connected Schwab account — only the Schwab connection, no local-model configuration.
  - Holds the single global run slot like any job, then releases it before the quick local persist.

- **Data retrieved**
  - Current positions from Schwab — the same holdings fetch the full run uses at Step 2.

- **Logic**
  - Normalize holdings by ticker — the same signed-quantity / cost-basis netting as the analysis run. A netted sum that does not finish finite fails the pull naming the symbol, as in the run.
  - Persist a **standalone** pulled-at snapshot to its own single-row store — never read by the analysis job.
  - Compare symbol presence with the latest analysis for display tags — a **frontend, presence-only** comparison (new / no-longer-held); the backend compares nothing.

- **Model**
  - None.
  - Does not require local-model configuration.

- **Does not**
  - Analyze holdings.
  - Change a verdict.
  - Trigger the Quick check.
  - Replace the next analysis run’s diff baseline.

- **Output**
  - Current-holdings view.

---

# The most important safety rules

- The engine calculates every financial number in the baseline arm; the model's own arm never binds or alters an engine value — it is validated structurally and on its declared domain, never against the engine, then scored on realized outcomes (its target bands the one read graded head-to-head against the engine; a band with a non-finite or non-positive leg reads as no band).
- Engine evidence annotates the model's choices, never bars them.
- Missing floor-bearing data causes abstention, not a guessed grade.
- A role-risk-only verdict carries no fabricated priced number — no letter grade, price target, or conviction on a structurally unpriceable vehicle.
- A directional verdict is only ever **authored** for a long position — a fresh pass returns not-rated for a net-short or net-zero holding; a selective run's unselected carry instead keeps its prior directional verdict (marked `side_reversed` when now net-short), rather than re-rating it without a fresh pass.
- The investor profile never changes the intrinsic verdict.
- Quick check warns but never rewrites a recommendation.
- A failed Quick-check retrieval becomes `unknown`, never clean.
- Selective runs cannot strengthen stale actions without fresh analysis.
- Actions are rung-only; sizing belongs to the portfolio-planner job.
- Outcome history may propose calibration changes but never applies them automatically.
- The job never places an order.
