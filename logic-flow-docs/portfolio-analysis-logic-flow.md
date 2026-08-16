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
  - Structurally validated only; never checked against the engine's numbers.
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
  - Full analysis of selected holdings plus required safety inclusions.
  - Unselected safe holdings may keep their earlier intrinsic verdicts.

- **Research reuse**
  - Prior distilled web research, under about four weeks old, used to seed and merge into this run's research.
  - It never skips the research loop for an analyzed holding — the loop runs full each run that holding is analyzed.

- **Held-name research refresh**
  - Tiny current-search check before the main holding loop.
  - Tests one named qualitative thesis driver or falsifier.
  - Can require a normal full pass.
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
  - Option chains for held stocks — fetched per holding at Step 6a; greeks are not parsed.

- **FMP**
  - Company profiles.
  - Financial statements; the ratio endpoints are designed, not yet pulled.
  - Estimates, earnings, dividends, and live quotes; the revision signal is designed.
  - Deep historical stock prices.
  - Sector and market benchmark prices — designed, not yet loaded.
  - Outcome-label price history.
  - Insider and congressional activity — designed, not yet pulled.
  - Peers, segments, and ratings — designed, not yet pulled; company news is pulled by the quick check.
  - Fund information and sector/country weights.
  - Sector valuation data used for supported funds.

- **SEC EDGAR**
  - Official filings and XBRL company facts.
  - Restatements and auditor changes — the designed hard-forensic producer.
  - Optional fund holdings through N-PORT — designed.

- **FRED**
  - Two-year and ten-year Treasury yields.
  - Historical ten-year yields for target calculations.
  - Energy and commodity prices — designed, not yet loaded.

- **FINRA (designed, not yet wired)**
  - Short-interest level, trend, and days-to-cover.

- **CFTC (designed, not yet loaded)**
  - Futures positioning for commodity, index, rate, and currency funds.

- **CBOE (designed, not yet loaded)**
  - Broad put/call market sentiment.

- **SearXNG (designed — research slice)**
  - Primary web search for holding research.

- **Tavily (designed — research slice)**
  - Backup web search when SearXNG fails.

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
  - Preserve original rows for audit and display.

- **Option chains**
  - Fetched per holding at Step 6a, never here — a selective run's carried tail spends no chain call.

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
  - Final strategy routing happens after `etf/info` arrives in Step 6a.
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
  - Compare signed quantities by ticker.
  - Tag each current holding:
    - New.
    - Increased.
    - Decreased.
    - Unchanged.
  - A long-to-short or short-to-long move is a reversal.
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

- **Designed run-level context (not yet loaded; each lands with its consumer)**
  - Energy and other commodity prices from FRED.
  - Gold quote from FMP.
  - Futures positioning from CFTC.
  - Broad put/call statistics from CBOE.
  - Sector and market benchmark histories from FMP.

- **Logic**
  - Omit the house view when older than one week.
  - Normalize rates into decimal form.
  - Share this context across all holdings.

- **What each context input feeds (later steps, not here)**
  - House view and recent report summaries → the Step 6f interpretation call.
  - Investor profile → the Step 6f action decision only — never intrinsic analysis.
  - `DGS10` → valuation multiples at Step 6b.
  - `DGS2` → return hurdles at Step 6b.
  - Commodity prices and the gold quote → commodity-linked holding evidence (designed).
  - CFTC positioning → a commodity / macro fund's underlying-positioning read (designed).
  - CBOE put/call → a venue-level options-sentiment backdrop: broad-market context, never a per-name signal (designed).
  - Sector and market benchmarks → the input delta's technology-event pre-flag at Step 6b (designed).

- **Failure rule**
  - `DGS2` or `DGS10` still unavailable after retries → fail the run.
  - Optional market context may fail softly.

- **Model**
  - None.

- **Output**
  - One shared context packet.

---

# Step 6 — Per-holding analysis loop

The following sequence runs once for every holding in the work list.

Each completed holding is designed to checkpoint separately; as-built only the between-holdings cancellation check exists.

## Work-list logic

- **Full run**
  - No cards selected.
  - Analyze every gradable holding.

- **Selective run — initial list**
  - User-selected holdings.
  - Every new holding.
  - Every holding with no prior verdict to carry.

- **Selective run — automatic safety additions**
  - Holding with an attention flag.
    - The quick check confirmed a problem — a fired falsifier or trigger, a newly-failing dead-money read, or a changed band relation — so the carried verdict is re-analyzed instead of trusted.
  - Holding whose Quick-check family is `unknown`.
    - A required signal family couldn't be checked (retrieval failed and no cache proved it current), so the sweep can't vouch for the carried verdict and refuses to let it stand on silence.
  - Holding whose long/short side reversed.
    - A long-to-short or short-to-long flip inverts the thesis by construction, so no long-side verdict can carry across it and the position re-enters as what it now is.
  - Holding with an unexamined evidence event.
    - New material information the current verdict never saw (earnings, a material filing, or a large estimate revision) landed, and the ledger's anticipated conditions can't catch what nobody anticipated.
  - Stale holding carrying a trim or sell-all action.
    - An over-age exit can't be safely softened (weakening risk-reducing advice on stale evidence raises risk) nor left standing as current advice for an irreversible act, so re-analysis is the only honest resolution.
  - Holding whose carried verdict predates `portfolio-v9` — the one-time migration force-include.
    - Its action was authored under the retired whole-book contract and may still encode portfolio context the tunnel-vision ruling removed, so one forced pass restamps it and the check never fires again.
  - Holding whose held-name refresh finds a material update (designed — research slice).
    - A source-backed material change to a named thesis driver or falsifier pulls the otherwise-carried holding in for a full pass.

- **Holdings outside the final work list**
  - Keep their previous intrinsic verdict, action, and thesis ledger.
  - Display the older analysis vintage.
  - A stale add action is automatically weakened to Hold and marked rule-demoted.
  - Nothing else may move a carried action without fresh analysis.

- **Research reuse (seed and merge — never a skip)**
  - The research loop and distillation run in full every run for every analyzed holding.
  - There is no lighter-vs-heavier case, and Steps 6c and 6d are never skipped.
  - If non-expired (< ~4 weeks) cached research exists for the holding, it is used two ways: to seed this run's loop and to merge into the results.
  - The seed is assembled deterministically — no extra model call — and injected per research topic (only claims within their own ~4-week vintage, bounded by a per-topic budget), so each topic sees its own prior distilled object and ledger conditions and the loop hunts what changed.
  - The merge happens per topic where that topic is first reduced (the tier-1 call, or the single small-run call): fresh findings supersede cached ones on conflict, sources are de-duplicated across topics at the reduce, a cached claim past ~4 weeks by its own vintage expires, and interpretation still reads one compact combined object.
  - If no non-expired cache exists, the loop simply runs cold.

- **Held-name refresh lane**
  - Runs before the per-holding loop.
  - Maximum: two holdings per run.
  - Looks only at holdings that would otherwise stay carried, judged from information available before Step 6b.
  - Requires a named qualitative driver or falsifier in the thesis ledger.
  - Checks one ledger item per selected holding.
  - Priority:
    - Nearest dated catalyst or condition window.
    - Closest prior result to an Add or exit boundary.
    - Oldest supporting research.
    - Highest priced-in expectations with uncertain execution.
    - Ticker as the final tie-break.
  - Retrieves:
    - Current web evidence.
    - Source dates.
    - Evidence tied to the exact ledger item.
  - Model returns:
    - `material_update`.
    - `no_material_change`.
    - `unscorable`.
  - App validates:
    - Correct company.
    - Correct ledger item.
    - Source and publication date.
  - `material_update` result:
    - Force-includes the otherwise-carried holding into the selective run.
    - Sends the evidence into the normal full research pass.
  - Other results:
    - Change nothing.
  - The lane cannot:
    - Confirm a falsifier.
    - Rewrite the thesis ledger.
    - Change conviction.
    - Change a target.
    - Choose an action.
  - Failed search:
    - Record `unscorable`.
    - Keep the prior state.
    - Do not update the full-research date.
  - Later technology pre-flag:
    - Step 6b may still require fresh research.
    - Mark the earlier lane slot `late-invalidated`.
    - Keep its evidence for the full research pass.
    - Do not refill the two-holding cap after the loop starts.

- **Resume behavior (designed, not built)**
  - Resume uses the interrupted run’s pinned holdings and context.
  - No fresh Schwab pull occurs.
  - Starting resume window: about 48 hours.

---

## Step 6a — Build the holding dossier

- **Stock data retrieved from FMP**
  - Company profile and listing identity.
  - Income statement, balance sheet, and cash-flow statement.
  - Ratios, key metrics, owner earnings, and enterprise value — designed, not yet pulled.
  - Discounted cash-flow valuation cross-check — designed.
  - Financial scores — designed.
  - Estimates (the forward consensus); the revision signal is designed.
  - Street targets and rating history as opinion evidence — designed.
  - Earnings and dividends.
  - Insider and congressional activity — designed.
  - Peers, float, and revenue segments — designed.
  - Live quote; company-news seeds are designed (research lane).
  - Deep dated price history.

- **Stock data retrieved elsewhere**
  - SEC filings and XBRL facts.
  - FINRA short interest — designed.

- **Option chains**
  - Fetched per holding from Schwab here, never in the Step-2 pull.
  - Volume, open interest, and implied volatility; greeks are not parsed.
  - Put/call ratios and the IV/skew read are computed at dossier assembly.
  - Linking held options to the same stock and classifying the overlay (covered call, protective put, collar) is designed, not built.
  - Chain fetch failure or malformed body → typed options gap.
  - An empty chain on an un-optioned name is a quiet market fact, not a gap.
  - Option-chain failure does not fail the run.

- **Fund data retrieved from FMP**
  - `etf/info`.
  - Expense ratio, AUM, NAV, asset class, and mandate.
  - Sector and country weights.
  - Sector P/E snapshots.
  - Historical sector P/E data.

- **Optional fund data (designed)**
  - SEC N-PORT fund holdings.
  - Used for concentration and single-name look-through.
  - Never required for the normal fund floor.

- **Local data retrieved**
  - Prior intrinsic verdict.
  - Prior thesis ledger.
  - Position delta.
  - Shared market context.
  - Portfolio Analysis memory for this holding — semantic recall designed, not built.

- **Stock identity validation**
  - Match Schwab identity to an FMP canonical symbol.
  - Matching US listing → continue.
  - US-listed ADR → continue.
  - No FMP resolution or non-US primary listing → not rated.
  - Conflicting issuer identities → insufficient evidence.

- **Fund routing**
  - US equity exposure with usable weights → priced-fund path.
  - Bond or commodity fund → role-risk-only path.
  - International fund below the US-exposure guard → role-risk-only path.
  - Leveraged or inverse fund → role-risk-only path.
  - Option-overlay fund → structural path-dependence flag; other priceability rules decide the route.
  - Mutual fund without usable weights → role-risk-only path.
  - Closed-end fund → the price-versus-NAV leg is designed; as-built it routes as a generic fund.

- **Embedding model (designed — no 6a semantic recall runs as-built)**
  - Converts a holding-specific query into a vector.
  - Searches only Portfolio Analysis memory.
  - Retrieves relevant prior analysis.
  - Performs no investment reasoning.

- **Embedding failure (designed)**
  - Skip semantic recall.
  - Keep the directly loaded prior verdict and ledger.
  - Record a degraded-input flag.

- **Output**
  - Complete stock or fund dossier.
  - Final vehicle route.

---

## Step 6b — Calculate the financial picture

- **Data retrieved**
  - Uses the dossier and shared context.
  - No model or web research.

### Stock grade calculations

- **Quality score**
  - Profitability and cash conversion.
  - Return on invested capital versus capital cost.
  - Gross profitability and free-cash-flow conversion.
  - Compared with sector bands and the company’s history.

- **Valuation score**
  - Uses suitable valuation ratios.
  - Metric choice changes for banks, REITs, cyclicals, and other special cases.
  - Compared with sector bands and the company’s history.

- **Risk score**
  - Realized volatility and leverage (debt/equity).
  - Drawdown enters the risk tier, not this score; no liquidity series is on this job's surface.
  - Higher score means safer.

- **Designed letter weighting**
  - Quality: 40%.
  - Valuation: 30%.
  - Risk: 30%.
  - Momentum stays outside the letter.

- **Letter cutoffs**
  - A: 85 or higher.
  - B: 70–84.
  - C: 55–69.
  - D: 40–54.
  - F: below 40.

- **Missing sub-score handling**
  - Missing score receives neutral 50.
  - At least two real sub-scores are still required.
  - A grade using an imputed score receives a low-confidence marker.

### Supported equity-fund calculations

- **Expense drag**
  - The expense ratio rides as evidence for the interpretation call.
  - No return figure is expense-adjusted deterministically.

- **Exposure tilt**
  - Use sector and country weights.
  - The house-view comparison happens at the interpretation call, not here.

- **Valuation calculation**
  - Read each sector’s earnings yield from its P/E.
  - Weight those yields by the fund’s current sector weights.
  - Ignore sectors without a usable P/E.
  - Renormalize over the covered fund weight.
  - Report the uncovered weight separately.
  - Require at least 70% P/E-usable weight.
  - Compare today’s constant-mix valuation with its historical version.

- **Fund grade**
  - Real valuation score.
  - Real risk score.
  - Structurally absent quality axis receives neutral 50.
  - The neutral value is not presented as fund quality.

- **Open design item**
  - The shipped flat-driver form (spot × composite yield, flat across scenarios) is the settled stopgap.
  - A scenario-differentiated priced-fund target formula is not yet designed.
  - It must be settled before the fund-depth slice is implemented.

### Scenario-target calculation for priced stocks

- **Choose the driver**
  - Positive consensus forward EPS when available.
  - Otherwise consensus forward revenue per share.
  - No usable driver → `no-admissible-driver` evidence gap.

- **Build bear, base, and bull driver cases**
  - Use low, middle, and high consensus values.
  - A missing or half-published spread holds both legs at the mid and records a flat driver.
  - Revision-dispersion widening is designed, waiting on the revision feed.
  - Clamp extreme growth assumptions.

- **Calculate valuation multiples**
  - Driver yield means the per-share driver divided by the stock price.
  - Review about three years of historical driver yields.
  - Compare each yield with the same date’s `DGS10` rate.
  - Form bear, base, and bull spread percentiles.
  - Re-anchor them using today’s `DGS10`.
  - Use recorded raw-multiple fallbacks when history is insufficient.
  - Repair any crossed bear/base/bull prices and log it.

- **Calculate returns**
  - Driver × multiple → scenario price target.
  - Add forward dividends for twelve-month total return.
  - Derive the one-month price target from the twelve-month price-return leg.
  - Keep one-month and twelve-month targets as rolling windows.

### Risk-tier calculation

- **Priced stock — High risk when any major high-risk condition fires**
  - Small company.
  - Unprofitable.
  - High volatility or drawdown.
  - High leverage.

- **Priced stock — Low risk when all low-risk conditions hold**
  - Large company.
  - Profitable.
  - Lower volatility and leverage.

- **Otherwise**
  - Medium risk.
  - Wholly missing tier inputs also produce Medium with a gap flag.
  - The canonical rule's liquidity legs are not on this job's data surface and never fire.

- **Priced equity fund**
  - High for leveraged/inverse structure, high volatility, or deep drawdown.
  - Low for low volatility and no structural flag.
  - Otherwise Medium.

- **Role-risk-only fund**
  - No risk tier.
  - Carries an observable risk description instead.

### Other deterministic reads

- **Conviction context**
  - Estimate and rating changes.
  - Earnings surprises.
  - Price momentum and market setup.
  - Insider, congressional, short-interest, and options activity.
  - These do not change the letter directly.

- **Narrative versus reality**
  - Compare multiple expansion with business or estimate improvement.
  - Thin analyst coverage uses company operating results instead.

- **Implied expectations**
  - Work backward from the current price.
  - Estimate the growth or margin range already priced in.
  - Used as context, not a gate.

- **Forensic checks**
  - Altman Z and Piotroski weakness.
  - Profit not supported by operating cash flow.
  - Receivables or inventory outrunning revenue.
  - Restatement or auditor change from SEC filings.
  - Fraud may arrive later from validated primary-source research.

### Pre-profit execution and financing overlay

- **Who enters**
  - Priced stock with non-positive TTM operating income.
  - Or no positive forward-EPS estimate plus negative TTM free cash flow.
  - Funds do not enter.

- **Structured data used**
  - Cash and cash equivalents.
  - Short-term investments.
  - Quarterly free cash flow.
  - Quarterly capital spending.
  - Quarterly revenue and gross profit.
  - Diluted share count.

- **Engine calculations**
  - Liquid resources:
    - Cash plus short-term investments.
  - TTM cash burn:
    - Zero when TTM free cash flow is positive.
    - Otherwise the absolute negative TTM free cash flow.
  - Cash runway:
    - `12 × liquid resources ÷ TTM cash burn`.
  - Capex intensity:
    - TTM capital spending compared with TTM revenue.
  - Dilution:
    - Split-adjusted diluted shares versus one year earlier.
  - Gross-margin direction:
    - Average of the latest two quarters.
    - Compared with the preceding two-quarter average.

- **Financing state**
  - `adequate`: at least 24 months of runway.
  - `watch`: 12 to under 24 months.
  - `constrained`: under 12 months.
  - `not_burning`: no current TTM cash burn.
  - `unscorable`: required data missing.

- **Research data added later**
  - Production and deliveries.
  - Bookings, backlog, or reservations.
  - Guidance ranges and matching actuals.
  - Unit economics.

- **Output at Step 6b**
  - Provisional overlay.
  - Statement-derived values only.
  - Research observations are not guessed.

### Capital-efficiency calculation

- **Return hurdle**
  - Low risk: `DGS2 + 3 percentage points`.
  - Medium risk: `DGS2 + 5 points`.
  - High risk: `DGS2 + 8 points`.

- **Three-state result**
  - Bear return clears hurdle → `clears`.
  - Bull return misses hurdle → `fails`.
  - Otherwise → `indeterminate`.

- **Meaning**
  - Only `fails` means dead money.
  - `indeterminate` does not force an exit.
  - New money uses a stricter point test.
  - Base-case total return must clear before Add is allowed.

### Continuity calculations

- **Input delta**
  - Position change and house-view age.
  - Prior-run values carried for the interpretation call to compare.
  - An engine-computed metric comparison is designed, not built.

- **Ledger checks**
  - Evaluate quantitative falsifiers and action triggers.
  - Advance streaks only on a new observation.
  - Preserve condition state by app-controlled condition ID.

- **Technology-event pre-flag (designed, not built)**
  - Compare the stock’s move with its sector.
  - Adjust the threshold for the stock’s volatility and elapsed time.
  - Large unexplained relative move adds a research topic.
  - It does not claim what caused the move.

### Evidence-floor check

- **Stock requires**
  - Current price.
  - Financial statements.
  - Matching issuer identity.
  - At least two real sub-scores.
  - A usable target driver once the v2 function is active.

- **Exposure-priced fund requires**
  - Current quote or NAV.
  - `etf/info` and expense ratio.
  - Usable sector and country weights.
  - At least 70% valuation coverage.

- **Floor failure**
  - Mark `insufficient-evidence` with named reasons.
  - Skip research, distillation, refinement, and interpretation.
  - Retain the prior thesis ledger and attention flag.
  - Create no new action or decision episode.

- **Non-floor gaps**
  - Missing optional research or positioning lowers confidence.
  - Weak web coverage alone does not force abstention.

- **Output**
  - Deterministic financial analysis.
  - Grade and provisional scenario targets where applicable.
  - Risk tier, hurdle state, and forensic flags.
  - Input delta and evidence-floor result.

---

## Step 6c — Research the holding

- **As-built: stubbed**
  - No web research runs today; a single research-deferred note is recorded.
  - Every run to date has graded on the deterministic financials and the house view.
  - The loop below is the research slice's design.

- **Seeded when (Layer 2 cache)**
  - Non-expired (< ~4 weeks) cached distilled findings exist for this holding — one **per-topic** object apiece, the layer the prior run persisted.
  - The orchestrator injects **each topic's own prior object** — its tier-1 distillation, or its topic-keyed group from a single-pass run — plus that topic's ledger conditions into the topic's opening pass, deterministically and with no extra model call, filtered to claims still within their own ~4-week vintage and bounded by a per-topic seed budget.
  - Seeding from the per-topic distillation, not a slice of the cross-topic combined object, starts each loop with richer, un-re-compressed topic detail; the topic is the storage partition, so the seed is a lookup rather than a per-claim re-assignment.
  - The loop then targets what changed rather than rebuilding the baseline; a cached prior never causes it to be skipped.

- **Data retrieved**
  - Current web sources.
  - SearXNG first.
  - Tavily if SearXNG fails.
  - Dossier facts and company-news seeds.
  - Previously-fetched pages under about four weeks old come from the document cache (Layer 1) instead of the network, carrying their original retrieval timestamp; new URLs are fetched live.

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
  - Or after a qualifying news seed.
  - Or after an approved research follow-up.
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

- **Research loop**
  - One isolated model conversation per topic.
  - One initial pass plus up to two follow-up passes.
  - Maximum three passes per topic.
  - Orchestrator owns every search and fetch.
  - Stop when the holding’s fetch or time budget is reached.
  - Store claims in an append-only evidence ledger.

- **Model determines**
  - Which sources answer the topic.
  - Which findings are supported.
  - Whether another focused follow-up is useful.
  - Which forward facts may affect targets.
  - Whether a research-only leading indicator exists.
  - Whether primary-source evidence shows fraud.

- **Failure logic**
  - Web failure reduces evidence.
  - It may lower conviction.
  - It does not automatically fail the run.

- **Output**
  - Full findings for every topic.
  - Evidence ledger with sources and timestamps.
  - Proposed follow-up and forward facts.

---

## Step 6d — Distill the research

- **As-built**
  - One unconstrained non-thinking condense of the stub note.
  - No evidence-ledger leg, hierarchy, or output schema until research lands.
  - A role-risk-only holding makes no research or distillation call at all.

- **Data retrieved**
  - No new external data.

- **Normal case**
  - One consolidation call, its output **keyed by topic** — globally reconciled, since the one call sees every topic — so each topic's group persists as that topic's next-run seed.

- **Large-input loop**
  - Distill each topic tree separately (**tier-1**) — feeding the reduce, not persisted raw.
  - Run one final combining call (**tier-2**) over the tier-1 objects: it emits the one combined object interpretation reads **and** the per-topic seed layer reconciled to the global winners — that reconciled layer, not the raw tier-1 output, persists as the next-run seeds.
  - Preserve citations through both levels.

- **Merging the seeded prior (Layer 2 cache)**
  - The merge is **per topic**: each topic's prior object joins that topic's fresh findings where the topic is first reduced — the tier-1 call when the run goes hierarchical, or the single consolidation call when it is small — fresh superseding cached within that topic.
  - Cross-topic reconciliation happens at the reduce (the tier-2 reduce, or that same single call): the same newest-wins-by-claim/metric rule is applied **globally** — a metric freshened under one topic supersedes a cached copy another carried forward — on top of source de-duplication, so nothing is double-counted or left conflicting.
  - The reduce **emits the per-topic layer already reconciled to the global winners** — the model that resolved the conflict owns the match, in the same pass, not an app-side re-derivation (Portfolio's claims carry no cross-topic identity key the app could match on) — so no persisted topic object keeps a value another topic superseded, and a later run can't surface it when the fresher topic is dormant.
  - The within-topic fallback triggers on a topic's **complete input** — its passes' findings, their accumulated evidence-ledger entries (claims + sources, which scale with research, distinct from the bounded thesis-ledger conditions that seed at 6c), and its bounded prior — whenever the sum would overflow one call: the research sub-distills along its ≤3-pass seam, each pass carrying its findings *and* ledger entries into a compact per-pass object, then a tree-level reduce over those plus the retained bounded prior; if that still overflows, the cap fail-softs the lowest-priority whole passes to a recorded gap — each taking its findings and ledger entries together, never the prior — so an overflow costs research detail, never seeded status.
  - A cached claim past about four weeks by its own vintage, not re-confirmed this run, expires rather than riding forward; each surviving claim keeps its vintage and whether it is fresh or carried.
  - The run writes a fresh per-topic layer, one object per analyzed topic, as the next run's seed, and the combined object interpretation reads is its cross-topic reduction; the audit records seeded-vs-cold **per topic** with each seeding object's vintage — a standing topic can seed while a newly-activated conditional topic runs cold.

- **Model**
  - Consolidates evidence.
  - Does not perform new searches.
  - Does not calculate financial numbers.

- **Typed outputs when supported**
  - `research_forward_assumption`:
    - Sourced numeric forward fact.
    - May affect an engine target after validation.
  - `validated_leading_indicator`:
    - Countable, dated, third-party indicator.
    - Must be absent from engine scoring.
    - Reaches later passes as ledger-driver evidence.
    - The old one-level conviction raise is retired.
  - `forensic_event`:
    - Primary-source hard forensic event.
    - Fraud can enter only through this validated path.
  - `pre_profit_execution_observations`:
    - Numeric operating facts quoted by a source.
    - Metric name and observation role.
    - Actual, guidance-low, guidance-high, point-guidance, or contextual-level role.
    - Higher-is-better, lower-is-better, or target-band direction.
    - Units and reporting period.
    - Company scope and publication date.
    - Source URL and confidence.
  - Backfill coverage when required:
    - Periods and sources checked.
    - `complete`, `partial`, or `unscorable`.

- **Role-risk-only rule**
  - No target assumption.
  - No leading-indicator field.
  - Pure research consolidation only.

- **Output**
  - One schema-validated research object.

---

## Step 6e — Recalculate targets using validated research

- **As-built**
  - The research-assumption legs below are designed; they land with the research loop.
  - Today the step's work is finalizing the pre-profit overlay at the engine seam.

- **Data retrieved**
  - No new data.

- **Validation (designed — research loop)**
  - Reject malformed, unsourced, or nonnumeric claims.
  - `supplement` may fill only a missing structured value.
  - `supersede` may replace structured data only when:
    - It is newer.
    - It comes from an approved primary-source fact type.
    - Metric, units, and period match.
  - Otherwise structured data wins.
  - Record every accepted or rejected rule.

- **Calculations (designed — research loop)**
  - Recalculate the affected scenario targets.
  - Recalculate the dead-money hurdle result.
  - Leave backward-looking grade sub-scores unchanged.

- **Pre-profit observation validation**
  - Confirm the correct company.
  - Confirm metric, direction, value, units, and period.
  - Confirm actual versus guidance role.
  - Confirm the source states the number.
  - Reject and log unmatched rows.
  - Add accepted rows to the period history.

- **Cold-start and history-gap backfill**
  - Required on the first overlay-eligible full pass.
  - Required again when a previously used guidance metric has fewer than four comparable stored periods.
  - Search the latest four reported periods.
  - Record every period and source checked.
  - Missing history remains a gap.
  - Never infer an observation that was not found.

- **Pre-profit engine calculations**
  - Pair guidance and actuals only when comparable.
  - Match the same metric, company scope, units, and period.
  - Guidance lower bound:
    - Range guidance uses the stated low.
    - Point guidance uses the stated value.
  - Guidance miss:
    - Applies only to a higher-is-better metric.
    - Lower bound must be finite and positive.
    - Actual at least 5% below the lower bound.
    - Smaller shortfall counts as in-line noise.
  - Repeated miss:
    - Same normalized metric only.
    - At least two distinct missed periods.
    - Look at that metric’s latest four comparable periods.
    - Different metrics never combine.
    - Two missed metrics in one period never count twice.
  - Material single miss:
    - Latest actual at least 20% below the lower bound.
  - Economics deterioration:
    - Latest two-quarter gross margin is non-positive.
    - At least 5 points below the preceding two-quarter average.
  - Material dilution:
    - Diluted shares up at least 15% year over year.
  - Severe deterioration:
    - At least two independent warning legs.
    - At least one must be execution or economics.
    - Financing plus dilution alone is not enough.

- **Pre-profit rule outputs (binding the engine arm; annotations beside the model arm)**
  - Repeated execution misses:
    - Engine conviction capped at Medium.
  - Constrained runway:
    - Add and Add aggressively leave the engine action set.
  - Severe deterioration:
    - Engine conviction capped at Low.
    - Engine action set limited to Trim or Sell all.
  - Letter grade remains unchanged.
  - One metric alone cannot force a sale.

- **Role-risk-only rule**
  - Skip this step.
  - No price targets or priced-stock overlay exist.

- **Model**
  - None.

- **Output**
  - Final engine-calculated target set.
  - Logged research assumption and resolution.
  - Final pre-profit overlay when applicable.

---

## Step 6f — Author the intrinsic verdict

Two model calls run in this step.
The interpretation call writes the intrinsic verdict; the action decision then picks the rung.

### Interpretation call — exact inputs

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
- **Fund context (funds only)**
  - Expense ratio, US share, and composite P/E coverage.
- **Computed metrics**
  - Net margin, gross margin, revenue growth, and debt/equity.
  - Daily return volatility and trailing return.
  - P/E, P/S, and P/B.
  - Missing values shown as gaps.
- **Engine scenario targets**
  - One-month and twelve-month bear/base/bull prices.
  - The methodology text.
- **Target provenance**
  - Anchor form: rate-anchored, current-multiple carry, or raw-percentile fallback.
  - Driver rung and consensus-row count.
  - Flat-driver, clamp-flattened, and dispersion-floor flags.
  - Guidance on how much signal each shape carries.
- **Final pre-profit overlay (when applicable)**
  - Engine states, matched rules, and the engine arm's conviction ceiling.
- **Options activity**
  - Put/call by volume and open interest, IV, and IV skew.
  - Labeled proxy-only, never a grade input.
- **Data gaps** from the dossier.
- **Distilled research** — the merged object (this run's fresh findings plus any seeded prior).
- **House view (when under one week old)**
  - The latest report's Thesis, Investment Strategy, and Forward Outlook sections.
  - Recent report stances: date, thesis stance, and risk posture.
  - Scope-limited to horizon reads and market setup.
- **Continuity block**
  - Whether a prior verdict exists.
  - A band-recalibration note when the grade bands changed since the prior letter.
- **Retrospective (when a prior priced verdict exists)**
  - The prior run's engine arm in full: grade, sub-scores, targets, conviction, outlook, action.
  - The prior run's model arm in full, labeled as the model's own.
  - The price move since the prior read.
  - The holding's matured scoreboard lines.
- **Prior thesis ledger**
  - The whole ledger, with this run's engine condition evaluation.
- **Deliberately excluded**
  - The investor profile.
  - The engine's current-run stand-in outlook, conviction, and action picks.
  - Raw statements, filings, and price-bar series — only computed values and distilled research reach the model.
- **Designed additions not yet in the prompt**
  - The implied-expectations range.
  - The narrative-versus-reality read.
  - Absolute street opinions.
  - The same-stock option overlay.

### Interpretation call — what the model authors

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

- **For a role-risk-only holding the same call instead authors**
  - Portfolio-independent role.
  - Exposure and observable risk.
  - Expense drag and structural concerns.
  - Evidence gaps.
  - Updated reduced fund ledger.
  - No letter, target, or conviction.

- **Model boundaries**
  - The engine arm is app-stamped; nothing the model returns can alter an engine value, overlay state, or monitor stamp.
  - The model arm is its own: structurally validated only, never checked against the engine's numbers.
  - Engine caps and ceilings bind the engine arm and annotate the model's departures; they never clamp the model's values.
  - Cannot see the investor profile.
  - Does not choose an action — the dedicated action decision below does.

### Action decision (second model call, same step)

- The profile enters the job here and nowhere else.
- Tunnel vision is stated in the prompt: no whole-book input exists, and a separate planning stage reconciles the book later.

- **Exact inputs**
  - Holding identity: symbol, name, quantity, total cost basis, and total market value.
  - Unrealized P/L, with the tax framing flagged as a user consideration, never the mover.
  - The prior run's action as continuity baseline — labeled as retired-contract history when authored before `portfolio-v9`.
  - Priced digest: engine arm grade, sub-scores, risk tier, and dead-money state; model arm letter and sub-scores; the verdict's conviction and horizon outlook; implied twelve-month bear/base/bull moves as percentages against the current price; a one-line target provenance; the financial summary; the pre-profit overlay when present.
  - Role-risk digest: class label, role, exposure tilt, expense drag, observable risk, structural flag, and evidence gaps.
  - The engine's per-holding action set, shown as evidence with the engine's own pick withheld.
  - The investor profile: objective, risk tolerance, horizon, and tax posture — without the cash row.
- **Deliberately excluded**
  - House view, research, computed metrics, and absolute target prices.
  - Every book-level value: cash, weights, concentration, other holdings.
- **Returns**
  - One rung from the fixed ladder plus a one-sentence rationale.
  - No target weight, share count, or dollar figure — sizing belongs to the portfolio planner.
  - A rung outside the engine set persists as authored, annotated on the audit.

- **Output of Step 6f**
  - Proposed intrinsic verdict with both arms.
  - Rewritten thesis ledger.
  - What-changed audit.
  - The holding's action and rationale.

---

## Step 6g — Validate continuity and checkpoint

- **Data retrieved**
  - No new data.

- **Two-arm stamping rule**
  - Engine values are app-stamped directly, never echoed through the model.
  - Nothing the model returns can alter an engine grade, target, overlay value, or monitor stamp.
  - The model arm's own numbers are structurally validated only, never compared against the engine's.

- **What-changed validation**
  - Every claimed external change must map to:
    - An input-delta entry.
    - A sourced research finding.
    - An accepted forward assumption.
  - Unsupported change becomes a labeled self-correction.
  - Or the response fails validation.

- **Conviction and cap handling**
  - The model's conviction is its own; no app recalculation, ceiling, or clamp touches it.
  - The old one-level conviction raise and its re-derivation are retired.
  - Matched cap rules record as audit annotations that bind the engine arm:
    - Hard forensic trip: engine conviction capped at Low; Add rungs leave the engine set.
    - Repeated execution miss: engine conviction capped at Medium.
    - Severe deterioration: engine conviction capped at Low; engine set limited to Trim or Sell all.
  - The strictest matched ceiling wins on the engine arm.
  - A model value past a ceiling renders beside the recorded rule.
  - Model prose cannot create an overlay warning state.
  - Grade remains unchanged by these caps.

- **Ledger validation**
  - Tripped quantitative condition must map to an engine crossing.
  - Tripped qualitative condition must map to sourced research.
  - New quantitative conditions must resolve to an engine series.
  - Unresolvable condition becomes qualitative.
  - App assigns and preserves condition IDs.
  - Changed machine logic starts a fresh evaluation streak.

- **Attention handling**
  - Successful full pass clears the prior attention flag.
  - Record the observation the pass acknowledged.
  - The same observation cannot immediately raise the flag again.

- **Output**
  - Validated intrinsic verdict and thesis ledger.
  - Completed per-holding checkpoint (designed, not built).

---

# Step 7 — Roll up the run and score past decisions

The construction stage that used to live here — whole-book constraints, a final-action synthesis, and a joint-feasibility check — was removed by the tunnel-vision ruling (2026-08-14).
Each holding's action is now final when its per-holding loop finishes; whole-book reasoning belongs to the portfolio-planner job.

## Roll-up

- **Calculations**
  - Verdict counts by disposition.
  - Largest single-position weight and cash weight (descriptive reads only).
  - Positions closed since the prior run, acknowledged rather than dropped.
  - Run-level data-health read, including context-pressure detection.

## Outcome learning

- **Data retrieved**
  - FMP dated-EOD bars for maturing outcome episodes.
  - FMP dividends for maturing outcome episodes.

- **Logic**
  - Tag each active episode's net alignment from the holdings diff.
  - Mature any window labels whose dates have arrived, including for symbols no longer held.
  - A failed price refresh leaves the label pending inside the coverage grace.
  - Append or extend this run's decision episodes.
  - Derive the scorecard reads over the updated episode set.

---

## Step 8 — Save the run and learning history

- **Data stored**
  - Normalized holdings snapshot used by the run.
  - Every intrinsic verdict.
  - Every portfolio action and its rationale.
  - Thesis ledgers and condition evaluation states.
  - Analysis vintages (attention flags live only in the quick-check store).
  - Portfolio roll-up.
  - Sources and timestamps.
  - Distilled research and per-topic seeded-vs-cold decisions (each with its seeding object's vintage).
  - Held-name refresh eligibility, priority, result, and validation (designed — research slice).
  - Whether the refresh forced a normal full pass (designed — research slice).
  - Engine calculations and input deltas.
  - Accepted and rejected research assumptions (designed — research loop).
  - Accepted and rejected pre-profit operating observations.
  - Period-keyed pre-profit observation history.
  - Required backfill periods, sources, completion state, and gaps.
  - Runway, execution, economics, dilution, and severe-deterioration states.
  - Every matched pre-profit conviction or action rule.
  - Matched cap rules.
  - What-changed audits.
  - Model, prompt, schema, and parameter versions.
  - Degraded-input flags.

- **Decision-episode logic**
  - Open an episode when the recommendation state changes.
  - Change may occur in the verdict branch or the action.
  - A thesis-change leg is designed but dormant until the attribution validator lands.
  - Wording-only thesis edits do not open an episode.
  - A reaffirmation extends an active episode.
  - A matured episode does not remain active forever.
  - The next genuine recommendation change opens a new episode.

- **Episode contents**
  - Anchor date.
  - Intrinsic-analysis vintage.
  - The action.
  - Legacy lean, divergence, and weight fields survive on construction-era episodes.
  - Decision-time grade, conviction, targets, hurdle, and cap inputs — both arms' values — when present.
  - Sector identity for later benchmark comparison.
  - Parameter version.
  - `model-chosen` or `rule-demoted` action source.

- **Retention**
  - Keep newest 30 Portfolio Analysis runs.
  - Keep outcome episodes independently until their labels mature.
  - Freeze matured episodes into their own capped archive.

- **Embedding model**
  - Embed matured calibration lessons.
  - Per-holding thesis, read, and action embeddings are designed, not built.
  - Store vectors only in Portfolio Analysis memory.
  - Failed embedding drops only that memory row.
  - Persisted run still succeeds.

- **Output**
  - Durable run and audit record.
  - Updated decision-episode store.
  - Searchable Portfolio Analysis memory.

---

## Step 9 — Display the result

- **Data retrieved**
  - Persisted run.
  - Latest standalone holdings snapshot when available.

- **Per-holding display**
  - Both arms side by side: the engine baseline and the model view.
  - Backward-looking grade and sub-scores in each arm.
  - Forward outlook, targets, and conviction in each arm, with divergence tags.
  - Standing thesis and scenario monitor.
  - The action and its rationale.
  - Financial summary.
  - What changed.
  - Attention flag.
  - Analysis vintage.

- **Role-risk-only display**
  - Role.
  - Exposure.
  - Observable risk.
  - Expense drag.
  - Structural flag and evidence gaps.
  - The action and its rationale.
  - No empty grade or target fields.

- **Portfolio display**
  - Run-level roll-up counts and data health.
  - Closed positions.
  - Not-rated and insufficient-evidence reasons.

- **Holdings display**
  - Current quantities, prices, values, cost bases, and gains.
  - When newer than the analysis, show both vintages clearly.
  - Do not mutate the older analysis cards.

- **Sorting**
  - Overall value.
  - Dollar gain.
  - Percentage gain.
  - Total cash invested.

- **Model**
  - None.

---

# Quick check

`Load last run → Refresh monitorable data → Evaluate ledgers → Raise warnings → Save state`

- **Purpose**
  - Keep existing thesis ledgers alive between full analyses.
  - Warn without rewriting decisions.

- **Data retrieved from local storage**
  - Last analysis run’s holdings snapshot.
  - Existing thesis ledgers.
  - Stored target inputs and rate anchors.
  - No fresh Schwab holdings pull.

- **Shared data refreshed**
  - Current holding prices from FMP (quote plus dated EOD).
  - `DGS2` and `DGS10` from FRED.
  - Failed rate pull may use a cached print under one week old.
  - No eligible rate cache → rate-dependent families become `unknown`.

- **Stock data refreshed when needed**
  - SEC filing check.
  - Analyst-estimate snapshot.
  - Earnings history.
  - Statements and dividends after a new filing.
  - Company news for technology-falsifier holdings.
  - FINRA file when a short-interest condition exists.
  - Unresolved SEC CIK → filing family becomes `unknown`.

- **Fund data refreshed**
  - `etf/info`.
  - Sector and country weights — always fetched; the gating is evaluation-side.

- **Calculations**
  - Evaluate every machine-checkable falsifier and trigger.
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
  - Newly failing dead-money read → amber attention flag.
  - Band relation changed since authoring (left, re-entered, or crossed the band) → amber attention flag.
  - New earnings, filing, revision, or qualifying news → quiet evidence-event badge.
  - Fund mandate, expense, or major exposure change → quiet evidence-event badge.

- **State updates**
  - Advance condition streak only on a new observation.
  - Persist first breach and confirmation state.
  - Keep model-authored thesis and triggers frozen.

- **Cannot**
  - Rewrite a grade.
  - Rewrite conviction.
  - Rewrite the thesis ledger.
  - Change a portfolio action.
  - Perform web research.

- **Model**
  - None.
  - Can run while the model server is configured but offline.

- **Selective-run effect**
  - `flagged` holding is automatically analyzed.
  - `unknown` holding is also automatically analyzed.
  - A failed check never counts as a clean result.

- **Output**
  - Attention flags.
  - Evidence-event and degraded-sweep badges.
  - Updated machine-condition state.

---

# Pull holdings

`Check Schwab → Fetch positions → Normalize → Save snapshot → Display`

- **Purpose**
  - View current holdings without running analysis.
  - Requires a connected Schwab account.

- **Data retrieved**
  - Current positions from Schwab.

- **Logic**
  - Normalize holdings by ticker.
  - Persist a standalone pulled-at snapshot.
  - Compare symbol presence with the latest analysis for display tags.

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

- The engine calculates every financial number in the baseline arm; the model's own arm is scored against it.
- Engine evidence annotates the model's choices, never bars them.
- Missing floor-bearing data causes abstention, not a guessed grade.
- The investor profile never changes the intrinsic verdict.
- Quick check warns but never rewrites a recommendation.
- A failed Quick-check retrieval becomes `unknown`, never clean.
- Selective runs cannot strengthen stale actions without fresh analysis.
- Actions are rung-only; sizing belongs to the portfolio-planner job.
- Outcome history may propose calibration changes but never applies them automatically.
- The job never places an order.
