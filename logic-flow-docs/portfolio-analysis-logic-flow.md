# Portfolio Analysis: logic flow

> This describes the designed job behavior.  
> Some parts are not implemented yet.

`Gate → Pull holdings → Classify positions → Compare with prior run → Load context → Analyze each holding → Build portfolio actions → Save → Display`

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
  - What to do after considering the whole portfolio.
  - Uses the ladder:
    - Sell all.
    - Trim.
    - Hold.
    - Add.
    - Add aggressively.

- **Standalone lean**
  - The action the holding would deserve by itself.
  - Created before portfolio concentration and overlap are considered.

- **Grade**
  - A–F summary of the business’s current quality, valuation, and risk.
  - It is mainly backward-looking.
  - Momentum does not belong in the designed letter grade.

- **Forward outlook**
  - The job’s short-, mid-, and long-term directional view.
  - Kept separate from the backward-looking grade.

- **Conviction**
  - Confidence in the intrinsic verdict.
  - High, Medium, or Low.
  - Separate from the letter grade and risk tier.

- **Risk tier**
  - Deterministic estimate of investment risk.
  - High, Medium, or Low.
  - Used in return requirements and sizing limits.

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
  - This pushes the standalone lean toward an exit.

- **Thesis ledger**
  - Persistent record of why the job holds its view.
  - Stores the thesis, drivers, scenarios, falsifiers, and action triggers.

- **Falsifier**
  - A condition showing the thesis may be wrong.
  - Example: operating margin falls below a stated level.

- **Action trigger**
  - A prewritten condition for adding, trimming, or selling.
  - The whole-book construction step still decides final sizing.

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
  - Reuse of recent distilled web research.
  - Allowed only when it is under about four weeks old and nothing important changed.

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
  - Risk tolerance, horizon, objective, tax posture, and cash assumption.
  - Used only for final portfolio construction.
  - Never changes the intrinsic verdict.

- **Option overlay**
  - Options attached to a held stock.
  - Examples: covered call, protective put, or collar.
  - Changes the holding’s effective upside and downside.

- **Reasoning model**
  - Local 122B model.
  - Performs research, interpretation, and portfolio construction.

- **Embedding model**
  - Local 4B model.
  - Finds relevant prior analysis.
  - Performs no investment reasoning.

## Main data sources

- **Charles Schwab**
  - Current holdings.
  - Quantities, cost basis, market value, and instrument identity.
  - Option chains and greeks for held stocks.

- **FMP**
  - Company profiles.
  - Financial statements and ratios.
  - Estimates, revisions, earnings, dividends, and live quotes.
  - Insider and congressional activity.
  - Peers, segments, ratings, and company news.
  - Fund information and sector/country weights.
  - Sector valuation data used for supported funds.

- **SEC EDGAR**
  - Official filings and XBRL company facts.
  - Restatements and auditor changes.
  - Optional fund holdings through N-PORT.

- **Stooq**
  - Historical stock prices.
  - Sector and market benchmark prices.
  - Outcome-label price history.

- **FRED**
  - Two-year and ten-year Treasury yields.
  - Historical ten-year yields for target calculations.
  - Energy and commodity prices.

- **FINRA**
  - Short-interest level, trend, and days-to-cover.

- **CFTC**
  - Futures positioning for commodity, index, rate, and currency funds.

- **CBOE**
  - Broad put/call market sentiment.

- **SearXNG**
  - Primary web search for holding research.

- **Tavily**
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
  - Symbol, CUSIP, asset type, and quantity.
  - Average cost and market value.
  - Option chains for held optionable stocks.
  - Volume, open interest, implied volatility, and greeks.

- **Manual data**
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

- **Options calculations**
  - Put/call ratio by volume.
  - Put/call ratio by open interest.
  - Implied-volatility and skew read.
  - Link held options to the same stock.
  - Classify the overlay:
    - Covered call.
    - Protective put.
    - Collar.
    - Other.

- **Failure logic**
  - Failed holdings pull → fail the run.
  - Missing or stale option chain → typed options gap.
  - Option-chain failure does not fail the run.

- **Model**
  - None.

- **Output**
  - One normalized portfolio snapshot.
  - Option activity and overlay records.
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
  - A material not-rated position still affects whole-book risk.
  - Starting materiality threshold: 5% of the portfolio.
  - No fake grade is created.

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
  - Energy and other commodity prices.

- **Other run-level data**
  - Commodity and market prices from Stooq.
  - Gold quote from FMP.
  - Futures positioning from CFTC.
  - Broad put/call statistics from CBOE.
  - Sector and market benchmark histories from Stooq.

- **Logic**
  - Omit the house view when older than one week.
  - Use `DGS10` to adjust valuation multiples.
  - Use `DGS2` for return hurdles.
  - Normalize rates into decimal form.
  - Share this context across all holdings.
  - Keep the investor profile away from intrinsic analysis.

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

Each completed holding is checkpointed separately.

## Work-list logic

- **Full run**
  - No cards selected.
  - Analyze every gradable holding.

- **Selective run — initial list**
  - User-selected holdings.
  - Every new holding.

- **Selective run — automatic safety additions**
  - Holding with an attention flag.
  - Holding whose Quick-check family is `unknown`.
  - Holding whose long/short side reversed.
  - Holding with an unexamined evidence event.
  - Stale holding carrying a trim or sell-all action.
  - Holding whose held-name refresh finds a material update.

- **Holdings outside the final work list**
  - Keep their previous intrinsic verdict and thesis ledger.
  - Display the older analysis vintage.
  - Final action may move toward Hold.
  - A fresh aggregate may move Hold or an add action to Trim.
  - It may not create a stronger add or escalate Trim to Sell all.
  - A stale add action is automatically weakened to Hold.

- **Research reuse**
  - Reuse research only when under about four weeks old.
  - Position must be unchanged.
  - No attention condition may have fired.
  - No technology-event pre-flag may exist.
  - No new evidence event may exist.
  - No held-name refresh may have found a material update.
  - Steps 6c and 6d are skipped when reuse qualifies.
  - Financial calculations and interpretation still run fresh.

- **Held-name refresh lane**
  - Runs before the per-holding loop.
  - Maximum: two holdings per run.
  - Looks only at holdings that appear reusable from information available before Step 6b or would otherwise stay carried.
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
    - Invalidates research reuse.
    - Adds the holding to a selective run.
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

- **Resume behavior**
  - Resume uses the interrupted run’s pinned holdings and context.
  - No fresh Schwab pull occurs.
  - Starting resume window: about 48 hours.

---

## Step 6a — Build the holding dossier

- **Stock data retrieved from FMP**
  - Company profile and listing identity.
  - Income statement, balance sheet, and cash-flow statement.
  - Ratios, key metrics, owner earnings, and enterprise value.
  - Discounted cash-flow valuation cross-check.
  - Financial scores.
  - Estimates and revisions.
  - Street targets and rating history as opinion evidence.
  - Earnings and dividends.
  - Insider and congressional activity.
  - Peers, float, and revenue segments.
  - Live quote and company-news seeds.

- **Stock data retrieved elsewhere**
  - SEC filings and XBRL facts.
  - Stooq price history.
  - FINRA short interest.
  - Schwab option activity and any same-stock option overlay.

- **Fund data retrieved from FMP**
  - `etf/info`.
  - Expense ratio, AUM, NAV, asset class, and mandate.
  - Sector and country weights.
  - Sector P/E snapshots.
  - Historical sector P/E data.

- **Optional fund data**
  - SEC N-PORT fund holdings.
  - Used for concentration and single-name look-through.
  - Never required for the normal fund floor.

- **Local data retrieved**
  - Prior intrinsic verdict.
  - Prior thesis ledger.
  - Position delta.
  - Shared market context.
  - Portfolio Analysis memory for this holding.

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
  - Closed-end fund → include price-versus-NAV analysis.

- **Embedding model**
  - Converts a holding-specific query into a vector.
  - Searches only Portfolio Analysis memory.
  - Retrieves relevant prior analysis.
  - Performs no investment reasoning.

- **Embedding failure**
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
  - Volatility, leverage, drawdown, liquidity, and related risks.
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
  - Treat the expense ratio as an annual return cost.

- **Exposure tilt**
  - Use sector and country weights.
  - Compare the exposure with the house view.

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
  - The priced-fund target formula is not yet defined.
  - It must be settled before the full fund slice is implemented.

### Scenario-target calculation for priced stocks

- **Choose the driver**
  - Positive consensus forward EPS when available.
  - Otherwise consensus forward revenue per share.
  - No usable driver → `no-admissible-driver` evidence gap.

- **Build bear, base, and bull driver cases**
  - Use low, middle, and high consensus values.
  - Use revision dispersion when the spread is unavailable.
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
  - Illiquid.

- **Priced stock — Low risk when all low-risk conditions hold**
  - Large company.
  - Profitable.
  - Lower volatility and leverage.
  - Liquid.

- **Otherwise**
  - Medium risk.
  - Wholly missing tier inputs also produce Medium with a gap flag.

- **Priced equity fund**
  - High for leveraged/inverse structure, high volatility, deep drawdown, or thin liquidity.
  - Low for low volatility, normal liquidity, and no structural flag.
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
  - Compare current engine values with the prior run.
  - Include position and house-view changes.

- **Ledger checks**
  - Evaluate quantitative falsifiers and action triggers.
  - Advance streaks only on a new observation.
  - Preserve condition state by app-controlled condition ID.

- **Technology-event pre-flag**
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
  - Keep real exposure in whole-book calculations.
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

- **Skipped when**
  - Research reuse qualified for this holding.

- **Data retrieved**
  - Current web sources.
  - SearXNG first.
  - Tavily if SearXNG fails.
  - Dossier facts and company-news seeds.

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

- **Data retrieved**
  - No new external data.

- **Normal case**
  - One consolidation call.

- **Large-input loop**
  - Distill each topic tree separately.
  - Run one final combining call.
  - Preserve citations through both levels.

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
    - May support a one-level conviction raise.
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
  - No conviction-raise field.
  - Pure research consolidation only.

- **Output**
  - One schema-validated research object.

---

## Step 6e — Recalculate targets using validated research

- **Data retrieved**
  - No new data.

- **Validation**
  - Reject malformed, unsourced, or nonnumeric claims.
  - `supplement` may fill only a missing structured value.
  - `supersede` may replace structured data only when:
    - It is newer.
    - It comes from an approved primary-source fact type.
    - Metric, units, and period match.
  - Otherwise structured data wins.
  - Record every accepted or rejected rule.

- **Calculations**
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

- **Pre-profit rule outputs**
  - Repeated execution misses:
    - Conviction capped at Medium.
  - Constrained runway:
    - Add and Add aggressively removed.
  - Severe deterioration:
    - Conviction capped at Low.
    - Add actions removed.
    - Standalone lean limited to Trim or Sell all.
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

- **Data retrieved**
  - Final engine calculations.
  - Distilled or reused research.
  - House view.
  - Prior verdict and thesis ledger.
  - Position delta and input delta.
  - Option overlay and positioning context.
  - Final pre-profit overlay when applicable.
  - Investor profile is deliberately excluded.

- **Model determines for a priced holding**
  - Interpretation of the engine’s letter grade.
  - Base conviction.
  - Optional one-level conviction raise.
  - Short-, mid-, and long-term outlook.
  - Which engine scenario is the justified base case.
  - Standalone action lean.
  - Financial-health explanation.
  - Updated thesis ledger.
  - Intrinsic what-changed explanation.

- **Model determines for a role-risk-only holding**
  - Portfolio-independent role.
  - Exposure and observable risk.
  - Expense drag and structural concerns.
  - Evidence gaps.
  - Updated reduced fund ledger.
  - No letter, target, conviction triple, or standalone lean.

- **Thesis-ledger rewrite**
  - Standing thesis.
  - Key drivers.
  - Bear, base, and bull monitor.
  - Quantitative and qualitative falsifiers.
  - Add, trim, and sell triggers.
  - Target-weight range.
  - Role-risk-only ledger uses condition-only scenarios.
  - Role-risk-only triggers are Trim or Sell only.

- **Conviction raise restriction**
  - Must cite the typed research-only leading indicator.
  - Indicator must confirm a named ledger driver.
  - Maximum increase: one level.
  - Price action cannot raise conviction.
  - Narrative cannot raise conviction.
  - A metric already scored by the engine cannot raise conviction again.

- **Model restrictions**
  - Cannot invent the grade.
  - Cannot invent a target.
  - Cannot alter engine calculations.
  - Cannot alter an overlay value or state.
  - Must obey an overlay conviction ceiling.
  - Must choose Trim or Sell all when severe deterioration restricts the standalone lean.
  - Cannot see the investor profile.
  - Cannot set the final portfolio action or target weight.

- **Output**
  - Proposed intrinsic verdict.
  - Rewritten thesis ledger.
  - Proposed conviction decomposition.
  - Intrinsic what-changed audit.

---

## Step 6g — Validate continuity and checkpoint

- **Data retrieved**
  - No new data.

- **Number validation**
  - Returned letter must equal the engine letter.
  - Returned targets must come from the engine scenario set.
  - Returned overlay values and states must equal the engine result.
  - Mismatch rejects the model response.

- **What-changed validation**
  - Every claimed external change must map to:
    - An input-delta entry.
    - A sourced research finding.
    - An accepted forward assumption.
  - Unsupported change becomes a labeled self-correction.
  - Or the response fails validation.

- **Conviction validation**
  - Recalculate final conviction in the app.
  - Honor only a valid one-level leading-indicator raise.
  - Drop and record unsupported raises.
  - Apply soft cap after any raise:
    - Maximum Medium.
  - Apply hard forensic cap after any raise:
    - Maximum Low.
    - Add actions barred later.
    - Standalone lean must tilt toward exit.
  - Apply pre-profit rules after any raise:
    - Repeated miss: maximum Medium.
    - Severe deterioration: maximum Low.
    - Constrained runway or severe state: Add actions barred later.
    - Severe state: standalone lean must be Trim or Sell all.
  - Strictest matched conviction ceiling wins.
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
  - Completed per-holding checkpoint.

---

# Step 7 — Build the whole-portfolio recommendation

## Step 7a — Calculate whole-book constraints

- **Data retrieved**
  - Completed and carried-forward intrinsic verdicts.
  - Current normalized portfolio.
  - Stooq bars for maturing outcome episodes.
  - FMP dividends for maturing outcome episodes.

- **Whole-book calculations**
  - Current weight of every position.
  - Single-position concentration.
  - Sector and country exposure.
  - Fund exposure added at sector/country level.
  - Ninety-day return correlations.
  - Overlap clusters when absolute correlation exceeds about 0.7.
  - Cash and buying-power position.
  - Market value and signed notional of material not-rated positions.
  - Fixed-income duration, credit risk, or standalone-option delta remain typed gaps when unavailable.

- **Per-holding sizing inputs**
  - Intrinsic grade and conviction when present.
  - Standalone lean when present.
  - Upside and downside from targets.
  - Dead-money result.
  - Existing weight and concentration headroom.
  - Correlation and exposure overlap.
  - Option overlay.
  - Unrealized gain or loss.
  - Risk tier.
  - Hard forensic state.
  - Pre-profit runway and severe-deterioration action rules.
  - Tax as a high-level user consideration.

### Feasible-action calculation

- **Add family requires**
  - Base-case total return clears the tier hurdle.
  - Grade is not F.
  - Position is below the 25% concentration cap.
  - No hard forensic trigger.
  - No constrained-runway state.
  - No severe-deterioration state.

- **Add aggressively additionally requires**
  - A or B grade.
  - Enough concentration headroom.

- **Role-risk-only set**
  - Sell all.
  - Trim.
  - Hold.
  - Add actions are unavailable without return evidence.

- **Starting target-size rules**
  - Trim: about 40–70% of current weight.
  - Hold: about 90–110% of current weight.
  - Add: about 120–160% of current weight, with roughly a 1.5% portfolio floor.
  - Add aggressively: about 160–220% of current weight, with roughly a 3% floor.
  - Absolute concentration cap: 25%.

- **Cash rule**
  - Current default profile treats outside cash as available.
  - Observed Schwab cash does not block an add.
  - External funding required is shown later.

### Outcome-learning calculations

- **For active decision episodes**
  - Compare the user’s net quantity move with the recommendation.
  - Tag alignment:
    - Aligned.
    - Contrary.
    - Partial.
    - Unknown.
    - Reversed.

- **Matured windows**
  - 1 month.
  - 3 months.
  - 6 months.
  - 12 months.

- **Price calculations**
  - Refresh Stooq through the window end.
  - Add cash dividends without reinvestment for total return.
  - Total return is the main absolute result.
  - Sector and market comparisons use price-only returns.
  - Calculate maximum drawdown.

- **Missing coverage**
  - Keep the label pending when bars do not reach the window end.
  - Starting grace period: about three months.
  - Then close it as `price-coverage-unscorable`.

- **Measurement rules**
  - Start from the next trading session’s close after the decision.
  - Continue measuring a stock after the user exits it.

- **Derived scorecard reads**
  - Did Add outperform Hold?
  - Did Hold outperform Trim and Sell?
  - Did target bands contain the later price?
  - Did falsifiers warn before a material decline?
  - How often did the model self-correct?
  - Fewer than 30 unique holdings with matured windows → no calibration proposal.
  - Results may propose calibration changes.
  - They never change rules automatically.

- **Output**
  - Whole-book aggregate packet.
  - Allowed actions and sizing bounds for every holding.
  - Newly matured outcomes and scorecard updates.

---

## Step 7b — Choose final actions and portfolio shape

- **Data retrieved**
  - No new external data.
  - All intrinsic verdicts.
  - Whole-book aggregates.
  - Allowed action sets and sizing bounds.
  - House view.
  - Investor profile.
  - Exited positions from Step 4.

- **Model determines**
  - Final action for every analyzed holding.
  - Target-weight range.
  - Estimated share and dollar adjustment.
  - Why final action differs from standalone lean.
  - Overall portfolio risk posture.
  - Concentration and exposure assessment.
  - What trims may fund which adds.
  - Positions closed since the prior run.

- **Important reasoning split**
  - Strong company may still be Trim because it is oversized.
  - Weaker company may be Add because it diversifies the portfolio.
  - Tax benefit of realizing a loss is a user consideration.
  - Tax alone cannot choose an action.

- **Model restrictions**
  - Must choose inside the engine-provided action set.
  - Must respect target-weight bounds.
  - Cannot place trades.
  - Cannot rewrite intrinsic grades or targets.

- **App validation**
  - Validate each action-change explanation against real aggregates.
  - Apply every proposed adjustment simultaneously.
  - Calculate external funding as buys minus trim/sell proceeds.
  - Negative external funding means the plan raises cash.
  - Final weights plus cash must account for the whole implied book.
  - Every weight must land inside its proposed range.
  - No position may exceed the concentration cap.
  - Constrained-cash profiles must fund buys from cash and sales.
  - Selective-run carried actions must obey transition rules.
  - Stale add actions must have been demoted to Hold.
  - Stale exit actions must have received fresh analysis.

- **Validation retry**
  - If infeasible, return the named violation to the model once.
  - Re-run portfolio construction.
  - A second infeasible result fails the run.

- **Output**
  - Final per-holding actions.
  - Target weights and adjustments.
  - Portfolio-level recommendation.
  - Action half of the what-changed audit.

---

## Step 8 — Save the run and learning history

- **Data stored**
  - Normalized holdings snapshot used by the run.
  - Every intrinsic verdict.
  - Every final portfolio action.
  - Thesis ledgers and condition evaluation states.
  - Attention flags and analysis vintages.
  - Portfolio roll-up.
  - Sources and timestamps.
  - Distilled research and reuse decisions.
  - Held-name refresh eligibility, priority, result, and validation.
  - Whether the refresh forced a normal full pass.
  - Engine calculations and input deltas.
  - Accepted and rejected research assumptions.
  - Accepted and rejected pre-profit operating observations.
  - Period-keyed pre-profit observation history.
  - Required backfill periods, sources, completion state, and gaps.
  - Runway, execution, economics, dilution, and severe-deterioration states.
  - Every matched pre-profit conviction or action rule.
  - Conviction decomposition and cap rules.
  - Intrinsic and action what-changed audits.
  - Model, prompt, schema, and parameter versions.
  - Degraded-input flags.

- **Decision-episode logic**
  - Open an episode when the recommendation state changes.
  - Change may occur in:
    - Intrinsic branch, lean, or thesis.
    - Final action or target-weight range.
  - Wording-only thesis edits do not open an episode.
  - A reaffirmation extends an active episode.
  - A matured episode does not remain active forever.
  - The next genuine recommendation change opens a new episode.

- **Episode contents**
  - Anchor date.
  - Intrinsic-analysis vintage.
  - Final action and target weight.
  - Standalone lean and divergence reason when present.
  - Decision-time grade, conviction, targets, hurdle, and cap inputs when present.
  - Sector identity for later benchmark comparison.
  - Parameter version.
  - `model-chosen` or `rule-demoted` action source.

- **Retention**
  - Keep newest 10 Portfolio Analysis runs.
  - Keep outcome episodes independently until their labels mature.
  - Freeze matured episodes into their own capped archive.

- **Embedding model**
  - Embed each holding’s standing thesis, intrinsic read, and final action.
  - Store vectors only in Portfolio Analysis memory.
  - Embed matured calibration lessons.
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
  - Backward-looking grade and sub-scores.
  - Forward outlook and scenario targets.
  - Conviction.
  - Standing thesis and scenario monitor.
  - Standalone lean.
  - Final action and target weight.
  - Financial and sizing rationale.
  - What changed.
  - Attention flag.
  - Analysis vintage.

- **Role-risk-only display**
  - Role.
  - Exposure.
  - Observable risk.
  - Expense drag.
  - Structural flag and evidence gaps.
  - Final action.
  - No empty grade or target fields.

- **Portfolio display**
  - Overall risk and concentration.
  - Sector and country exposure.
  - Cash and deployment stance.
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
  - Current holding prices from FMP and Stooq.
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
  - Sector and country weights when exposure is relevant.

- **Calculations**
  - Evaluate every machine-checkable falsifier and trigger.
  - Market-data condition → every pass.
  - Filing condition → only after a new filing-style observation.
  - Re-anchor stored v2 multiples using current `DGS10`.
  - Recalculate the dead-money hurdle using current price and `DGS2`.
  - Check whether price moved outside the stored bear–bull band.
  - Detect new evidence events.

- **Per-family result**
  - `fresh_clear`:
    - Retrieval succeeded.
    - No condition fired.
  - `flagged`:
    - Confirmed condition, trigger, hurdle change, or band break fired.
  - `unknown`:
    - Retrieval failed.
    - Available cache could not prove the family current.

- **Warning logic**
  - Confirmed falsifier or trigger → amber attention flag.
  - Newly failing dead-money read → amber attention flag.
  - Price outside the stored scenario band → amber attention flag.
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

- The engine calculates every financial number.
- The model interprets numbers and chooses only inside app-defined bounds.
- Missing floor-bearing data causes abstention, not a guessed grade.
- The investor profile never changes the intrinsic verdict.
- Quick check warns but never rewrites a recommendation.
- A failed Quick-check retrieval becomes `unknown`, never clean.
- Selective runs cannot strengthen stale actions without fresh analysis.
- Whole-book target weights must work simultaneously.
- Outcome history may propose calibration changes but never applies them automatically.
- The job never places an order.
