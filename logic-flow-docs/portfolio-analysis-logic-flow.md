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

- **Selective run** (analyzes strictly the selection — ruled 2026-08-16, `docs/verification/2026-08-16-selective-badges-ruling.md`)
  - **Work list**
    - The user-selected holdings, and nothing else.
    - No automatic additions — the former safety additions now surface as card badges (below), never a forced re-analysis.
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
    - Side reversed — a long/short flip since the verdict; the carried thesis is for the opposite position (the verdict is marked `side_reversed`).
    - Stale vintage — the carried verdict is older than the ~4-week window.
    - Each is a non-blocking badge on the card; the user acts on it by selecting the holding or running a full analysis, so an urgent single-holding run is never blocked by the rest of the book.

- **Resume behavior (designed, not built)**
  - Resume uses the interrupted run’s pinned holdings and context.
  - No fresh Schwab pull occurs.
  - Starting resume window: about 48 hours.

---

## Step 6a — Build the holding dossier

For each holding, Step 6a assembles the dossier — the evidence packet the deterministic engine computes over at Step 6b. The gather is guard-ordered: a stock resolves its listing identity first and pulls the rest of its evidence only if that guard clears.

### Resolve stock identity (runs first)

A guard-terminal stock — one the guard finds unsupported, non-US, or identity-conflicting — is routed straight to its verdict here (not-rated or insufficient-evidence) and spends no further pull. A fund carries no stock guard; its priced-vs-role-risk route is decided later, once its data lands (see Fund routing below).

- **Stock identity validation**
  - Company profile and listing identity — one FMP company-profile fetch, pulled before the rest.
  - Cross-check that profile against Schwab’s — exchange first (the symbol is queried as-is; no symbol remap), then issuer name:
    - US primary exchange (NYSE / NASDAQ / AMEX) with a matching name → continue; a US-listed ADR passes on venue, not domicile.
    - FMP definitively resolves no such listing (an honest empty response), or a non-US primary listing → not rated.
    - US exchange but the issuer names share no significant token → insufficient evidence (a possibly-transient identity conflict).
    - A failed or unreadable profile fetch, or identity too sparse to cross-check on either side — a resolved profile missing its exchange or name, or a Schwab description with no issuer name (or only a ticker the FMP name doesn’t contain) → continue with a recorded degraded input.

### Gather the evidence (skipped for a guard-terminal stock)

Once the stock guard clears — and for every fund — the remaining legs are pulled. Stocks and funds pull disjoint financial legs — a fund touches neither the company statements nor SEC — but both share the deep price history, the option chain, and the local prior-run legs.

- **Stock data retrieved from FMP**
  - Income statement, balance sheet, and cash-flow statement (quarterly).
  - Ratios (P/E, P/B, debt/equity), key metrics (return on invested capital, free-cash-flow conversion, gross profitability), owner earnings, and enterprise value (EV, EV/EBITDA) — designed, not yet pulled; P/E, P/S, and P/B are derived from market cap and the statements today instead.
  - Discounted-cash-flow valuation cross-check (intrinsic value vs price) — designed.
  - Financial scores (Piotroski F-score, Altman Z-score) — designed.
  - Forward consensus estimates (revenue, EPS); the revision signal is designed.
  - Street targets and rating history (consensus target price, upgrade/downgrade actions) as opinion evidence — designed.
  - Dividends (the trailing distributions); per-symbol earnings are pulled only by the quick check, not this pass.
  - Insider and congressional activity (Form 4 insider buys/sells, congressional trades) — designed.
  - Peers, float, and revenue segments (comparable tickers, free-float shares, product/geographic revenue mix) — designed.
  - Live quote; company-news seeds are designed (research lane).
  - Deep dated price history (~1,600-day lookback).

- **Stock data retrieved elsewhere**
  - SEC XBRL company facts (revenue, gross profit, net income, total assets, equity) — filings themselves are read only by the quick check and the designed forensic producer, not this pass.
  - FINRA short interest (level, trend, days-to-cover) — designed.

- **Option chains**
  - Fetched per holding from Schwab.
  - Volume, open interest, and implied volatility; greeks are not parsed.
  - Put/call ratios and the IV/skew read are computed at dossier assembly.
  - Linking held options to the same stock and classifying the overlay (covered call, protective put, collar) is designed, not built.
  - Chain fetch failure or malformed body → typed options gap.
  - An empty chain on an un-optioned name is a quiet market fact, not a gap.
  - Option-chain failure does not fail the run.

- **Fund data retrieved from FMP**
  - `etf/info`.
    - Expense ratio, AUM, NAV, and asset class (the mandate label — one field, not two).
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
  - Portfolio Analysis memory for this holding — semantic recall designed, not built.

### Fund routing

A fund’s route — read from the `etf/info` and weights gathered above — is applied by the fund engine at Step 6b.

- **Fund routing**
  - US equity exposure with usable weights → priced-fund path.
  - Bond or commodity fund → role-risk-only path.
  - International fund below the US-exposure guard → role-risk-only path.
  - Leveraged or inverse fund → role-risk-only path.
  - Option-overlay fund → structural path-dependence flag; other priceability rules decide the route.
  - Mutual fund without usable weights → role-risk-only path.
  - Closed-end fund → the price-versus-NAV leg is designed; as-built it routes as a generic fund.

### Semantic recall (designed)

The designed embedding-based recall of this holding’s prior analysis; as-built this section does not run.

- **Embedding model (designed — no 6a semantic recall runs as-built)**
  - Converts a holding-specific query into a vector.
  - Searches only Portfolio Analysis memory.
  - Retrieves relevant prior analysis.
  - Performs no investment reasoning.

- **Embedding failure (designed)**
  - Skip semantic recall.
  - Keep the directly loaded prior verdict and ledger.
  - Record a degraded-input flag.

### Output

- **Output**
  - Complete stock or fund dossier.
  - A stock leaves with its resolved listing route — a verdict if the guard was terminal, otherwise clearance into the 6b engine; a fund leaves with its routing inputs, not yet a route.

---

## Step 6b — Calculate the financial picture

- **Data retrieved**
  - Uses the dossier and shared context.
  - No model or web research.

- **How the step runs**
  - The deterministic engine runs the holding down whichever of three routes it resolved to — a priced stock, a priced equity fund, or a role-risk-only fund. The two priced routes are graded (letter + sub-scores); the role-risk-only fund gets a role / risk readout with no grade. Nothing in the step calls a model or the web.
  - The evidence floor is not a closing stage: its gates fire inline as the values below are computed, and the first failure short-circuits the holding to `insufficient-evidence`, skipping 6c–6f; the gates are gathered under Evidence floor at the end of the step.

- **Order of computation (priced stock)** — computed in sequence, several reads feeding the next:
  - **Pre-profit overlay** (pre-profit stocks only) — financing / execution health, whose rule consequences later **cap the engine arm's conviction and narrow its action set**.
  - **Sub-scores** (quality, valuation, risk) → roll up into the letter; **momentum / market setup** is computed alongside them, live, as context outside the letter.
  - **Scenario targets** (bear / base / bull; one-month and twelve-month) → feed the return hurdle.
  - **Letter grade** — the weighted roll-up of the sub-scores.
  - **Risk tier** (High / Medium / Low) → sets the return-hurdle rate.
  - **Return hurdle** (capital efficiency) — risk tier + scenario returns → the dead-money read.
  - **Continuity and ledger checks** — the prior ledger's conditions evaluated against the new engine values.
  - **Designed reads (not computed yet)** — narrative-vs-reality, implied expectations, forensic checks, and the technology-event pre-flag would ride as evidence once built; they are not part of the live order.

- **The through-line**
  - The load-bearing dependencies: **sub-scores → letter**, **risk tier → hurdle**, **targets → hurdle**, **overlay → conviction cap + action-set narrowing**.
  - A **fund** swaps this stock spine for the equity-fund path below; a **role-risk-only** fund computes no grade, target, tier, or hurdle.
  - 6b reaches no verdict of its own: its reads feed each other (the dependencies above) to produce the **deterministic financial analysis** — letter, targets, tier, hurdle, overlay, ledger state — which the interpretation call reasons over at 6f (and whose targets the outcome scoreboard later scores). The evidence floor, gathered at the step's end, is the one gate that acts within the step.

### Engine primitives (used in the formulas below)

Three primitives recur in the value formulas that follow, so they are defined once here.

- **`scale(x, lo → hi)`** maps `x` linearly onto 0–100 and clamps it: `lo` scores 0, `hi` scores 100, and an **inverted** band (`lo > hi`) scores lower inputs higher.
- **`average(…)`** is the unweighted mean of whichever legs are present — a missing leg is dropped *inside* a value; a wholly-absent value is handled per section (usually imputed to a neutral 50 at the roll-up, never dropped there).
- A **ratio** `a ÷ b` is `None` when the denominator is missing or zero; a few ratios below add a stricter `> 0` guard, noted where they do.

### Pre-profit overlay (stocks, computed first)

Computed before the grade for every stock and persisting even when the stock does not enter the overlay or later abstains. As-built the overlay — its statement states and the rule consequences that bind the engine arm — is produced here in one pass; the design's later research-observation merge (Step 6e) is dormant while research is stubbed.

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
  - **Execution read** — guidance-vs-actual misses. A comparable period *misses* when `(guidance − actual) ÷ guidance ≥ 5%`; `material_single_miss` when the newest comparable period misses by ≥ 20%; `repeated_miss` when ≥ 2 of one metric's latest four comparable periods miss. [note: the research producer is dormant as-built, so this sees only carried prior observations.]
  - **Economics deterioration** — recent two-quarter average gross margin non-positive **and** down ≥ 5 percentage points.
  - **Material dilution** — YoY diluted-share change ≥ +15%.
  - **Severe deterioration** — at least two of {repeated-or-material execution miss, constrained runway, economics deterioration, material dilution} hold, **and** at least one of those is the execution or the economics leg.

- **Research data added later**
  - Production and deliveries.
  - Bookings, backlog, or reservations.
  - Guidance ranges and matching actuals.
  - Unit economics.

- **What the overlay emits** (the whole record persists — none of the calculations above are scratch)
  - The complete overlay record persists for **every priced stock** — the statement inputs above, the **financing state** (one of the five values), the execution / economics reads, the matched **rule consequences** that bind the engine arm (conviction ceiling and action-set narrowing, detailed at Step 6e), plus its eligibility result, unscorable gaps, and (dormant until research) observation history — carried on the holding's audit row so the period history survives run retention.
  - Only an **eligible** overlay reaches the Step-6f interpretation prompt, and even then the renderer exposes a **selected subset** — the financing / execution states, the matched rules, and the figures behind them (runway, liquid resources, burn) — as engine-arm context, not the entire record.
  - As-built the overlay is **statement-derived only**; the Step-6e research-observation merge that would finalize the execution legs is dormant, and no research observation is guessed.

### Sub-scores (stocks)

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
  - Input: trailing return (first-to-last close over the available history).
  - Equation: `scale(−0.30 → 0.30)` (−30% → 0, 0 → 50, +30% → 100).

### Scenario targets (priced stocks)

Bear / base / bull price targets — one-month and twelve-month — priced from a per-share driver and a rate-anchored multiple; they feed the return hurdle below. Computed before the letter is finalized; a stock with no admissible driver abstains here.

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
  - Inputs: the driver cases, the multiples, spot, forward dividends.
  - Equation:
    - Twelve-month price = `driver × multiple`, per scenario. [note: crossed bear/base/bull prices are repaired to ascending and logged; a dispersion floor = `clamp(daily vol × 15.87 × 0.5, 0.05, 0.20)` widens — never narrows — the bear/bull spread.]
    - Twelve-month total return = `(price + forward dividends) ÷ spot − 1`.
    - One-month price = `spot × (1 + twelve-month base price-return ÷ 12)`; the bear/bull legs take a band = `clamp(daily vol × 2, 0.02, 0.15)` (else 5%), dividends excluded.

- **Where these land**
  - Targets → the output price targets (each with its methodology + provenance flags); total returns → the hurdle read; the drivers, spread / raw percentiles, spot, forward-dividend leg, dispersion floor, and consensus EPS mid persist as the quick-check basis the between-run engine re-anchors against. The multiples themselves are not stored — they are recomputed closed-form from the basis.

### Letter grade (stocks)

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

### Risk tier (priced stock)

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

### Capital efficiency — the return hurdle (stocks)

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

### The equity-fund path

The alternative branch to the stock spine above; the fund engine makes the final priced-fund vs role-risk-only classification here in Step 6b (the routing rules are at Step 6a §Fund routing). A role-risk-only fund takes no grade, target, or risk tier — only the role-risk readout at the end of this section.

- **Fund valuation** — what the fund’s sector mix costs now vs its own history; the priced fund’s one real valuation input.
  - Inputs: per-sector P/E snapshots (blended across exchanges), the fund’s current sector weights, ~8–12 quarters of historical sector P/Es.
  - Equation:
    - Per sector: earnings yield = `1 ÷ P/E`.
    - Composite earnings yield = `Σ(weightᵢ ÷ P/Eᵢ) ÷ Σ weightᵢ` over sectors with a usable P/E — renormalized over the **covered** weight; sectors without a usable P/E are skipped.
    - Coverage = the covered weight as an absolute share of the fund; **≥ 70% is required**, else the valuation is a gap (uncovered weight is reported, never averaged in as zero).
    - Valuation sub-score = the percentile rank of today’s composite yield within the same-weights-over-historical-multiples series = `(count of history ≤ today ÷ history length) × 100` (higher yield = cheaper = higher score).
    - [note: needs ≥ 8 history samples, each itself with ≥ 70% coverage.]

- **Fund sub-scores and grade**
  - **Quality** — no fund quality axis exists, so it is a fixed neutral **50** (never presented as fund quality; its presence forces the low-confidence marker).
  - **Valuation** — the coverage-gated percentile above.
  - **Risk** — `average` of two inverted legs: volatility → `scale(0.04 → 0)`, drawdown → `scale(0.6 → 0)`. A **missing leg is imputed to 50** when the other is present (unlike the stock risk score, which drops it); **both legs absent → the fund abstains** (`insufficient-evidence`). [note: these bands differ from the stock risk bands.]
  - **Momentum** — trailing return → `scale(−0.30 → 0.30)`, outside the letter.
  - **Grade** — same weighting and cutoffs as a stock (`quality × 40% + valuation × 30% + risk × 30%`); because fund quality is always 50, this reduces to `20 + valuation × 30% + risk × 30%`, and every priced fund carries the low-confidence marker.

- **Fund metrics**
  - **Expense ratio** — a decimal ratio; rendered to the interpretation prompt (no return figure is expense-adjusted deterministically).
  - **US share** — the fund's US country-weight share; rendered to the prompt.
  - **Composite coverage** — the covered valuation weight from above; rendered to the prompt beside the valuation read.
  - **NAV premium / discount** — `spot ÷ NAV − 1`, only when NAV > 0 (positive is a premium). Computed but currently consumed by no prompt, score, or rule (it lands with the designed CEF price-vs-NAV leg).
  - **Exposure tilt** — the sector and country weights. On the priced path only the US share above reaches the prompt; a top-five tilt (sector weights, else country when sector is absent) is rendered only on the role-risk path. The house-view comparison happens at the interpretation call.

- **Fund scenario target** (flat-driver stopgap)
  - Equation: driver = `spot × composite earnings yield`, held **flat across bear / base / bull**; scenario spread comes only from the multiple axis (and, on the carry path, the volatility-scaled dispersion floor), not the driver.
  - Open design item: a scenario-differentiated priced-fund target formula is not yet designed; it must be settled before the fund-depth slice is implemented.

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

### Continuity and ledger checks

After the branch's engine values are set, the prior ledger's conditions are evaluated against them (the ledger checks). The input delta's pieces — position change, prior values, house-view age — already arrived from Steps 4–5 and the dossier; its engine-computed metric comparison is designed, not built.

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

### Other deterministic reads

Additional stock reads that ride into the interpretation call as evidence; none changes the letter directly, and most are still designed.

- **Conviction context**
  - Estimate revisions and rating changes — analyst estimates are already pulled (the target driver), but the revision signal is designed; rating history is not yet pulled.
  - Earnings surprises — designed.
  - Price momentum and market setup — live.
  - Insider, congressional, and short-interest activity — designed, not yet pulled; options activity is live.
  - These do not change the letter directly.

- **Narrative versus reality (designed, not built)**
  - Compare multiple expansion with business or estimate improvement.
  - Thin analyst coverage uses company operating results instead.

- **Implied expectations (designed, not built)**
  - Work backward from the current price.
  - Estimate the growth or margin range already priced in.
  - Used as context, not a gate.

- **Forensic checks (designed, not built)**
  - Altman Z and Piotroski weakness.
  - Profit not supported by operating cash flow.
  - Receivables or inventory outrunning revenue.
  - Restatement or auditor change from SEC filings — the hard-forensic producer.
  - Fraud may arrive later from validated primary-source research (research lane).

### Evidence floor

The inline gates referenced above, gathered — with each branch's requirements and the short-circuit behavior.

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
  - At least one risk leg — volatility or drawdown (both absent → abstain).
  - At least eight constant-mix history samples (fewer → abstain).

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
    - For a fund, the **fund grade** and **fund risk tier** (priced); or, for a role-risk-only fund, the engine-computed **role-risk readout** — class label, exposure tilt, expense ratio, numeric observable risk (annualized volatility), structural flag, and evidence gaps (the model authors the role prose on top).
    - Of these, the two-arm **engine arm** — the fields **carried in both arms** (authored deterministically here and by the model at 6f) — is just the **sub-scores / letter** and the **scenario targets** (plus the outlook / conviction / action stand-ins assembled later); of those, only the **targets and outlook** are scored against realized outcomes today — and only the target bands additionally get an engine-vs-model head-to-head — while sub-scores, conviction, and action are carried but unscored. The risk tier, hurdle read, and overlay are shared deterministic evidence, and a **role-risk-only** fund is a separate, non-two-arm branch.
  - **Supporting reads emitted as 6f evidence** (none changes the letter):
    - The **computed metrics** — net / gross margin, revenue growth, debt-to-equity, volatility, trailing return, P/E, P/S, P/B.
    - **Momentum / market setup**; for a priced fund, the **expense ratio**, **US share**, and **composite coverage** reach the prompt (the computed NAV premium and the full exposure tilt do not).
    - The **ledger evaluation** — the prior ledger's conditions' tripped / fired state.
    - The **input delta** — position change, house-view age, and prior-run values carried for comparison.
    - Designed, not yet emitted: forensic flags, narrative-vs-reality, implied expectations, and the designed conviction-context signals.
  - **Control result**: the **data-gap manifest** and the **evidence-floor outcome** — pass, or `insufficient-evidence` with named reasons (which short-circuits 6c–6f).
  - **Persisted working reads (not scratch)**: the engine keeps its intermediates too — the overlay's statement inputs (liquid resources, burn, runway, capex intensity, dilution, margin direction) ride the audit row, and the settled per-share drivers, spread / raw-multiple percentiles, spot, and forward-dividend leg persist as the **quick-check basis** the between-run engine paths re-anchor against.
  - **Assembled later, not here**: the engine arm's mechanical **stand-in outlook / conviction / action** are built at verdict time (after interpretation) from these 6b values — they are not part of the Step-6b output.

---

## Step 6c — Research the holding

- **As-built: stubbed**
  - No web research runs today; a single research-deferred note is recorded.
  - Every run to date has graded on the deterministic financials and the house view.
  - The loop below is the research slice's design.

### How the stage runs

For an analyzed holding the research loop and distillation always run and are never skipped — a recent cache only seeds them, it never replaces them.

- **Always runs (seed and merge, never a skip)**
  - The research loop and distillation run in full every run for every analyzed holding.
  - There is no lighter-vs-heavier case, and Steps 6c and 6d are never skipped.

- **Seeded when (Layer 2 cache)**
  - Non-expired (< ~4 weeks) cached distilled findings exist for this holding — one **per-topic** object apiece, the layer the prior run persisted.
  - The orchestrator injects **each topic's own prior object** — its tier-1 distillation, or its topic-keyed group from a single-pass run — plus that topic's ledger conditions into the topic's opening pass, deterministically and with no extra model call, filtered to claims still within their own ~4-week vintage and bounded by a per-topic seed budget.
  - Seeding from the per-topic distillation, not a slice of the cross-topic combined object, starts each loop with richer, un-re-compressed topic detail; the topic is the storage partition, so the seed is a lookup rather than a per-claim re-assignment.
  - The loop then targets what changed rather than rebuilding the baseline; a cached prior never causes it to be skipped.

- **Cold when (no Layer 2 cache)**
  - If no non-expired cache exists, the loop simply runs cold.

### Build the agenda (the topics)

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

### Work each topic — the research loop

The orchestrator works the agenda **one topic at a time**. Each topic is its own isolated conversation over a clean context, and the orchestrator — never the model — owns every search and fetch, stopping at the holding's budget.

- **Two nested levels**
  - **Topic** — one isolated conversation per agenda topic; topics never share a context.
  - **Pass** — each topic's conversation is a bounded multi-turn tool loop: one root pass plus up to two follow-up passes, so three passes per topic at most. The cap counts passes (branches), not model calls — a single pass is itself many turns, each turn one model call: the tool-requesting turns ask for a search or fetch the orchestrator runs, and the pass's terminal turn emits its findings.

- **What each topic conversation is given (its inputs)**
  - The shared dossier facts — identical for every topic.
  - That topic's own questions — different per topic.
  - That topic's own seed, only when a non-expired (< ~4 weeks) cache exists: its own prior distilled object (its tier-1 distillation, or its topic-keyed group from a single-pass run) plus its ledger conditions. The seed is per topic — there is no single shared seed — and a topic with no cached prior starts clean.
  - The company-news seeds — the symbol-scoped FMP `news/stock` headlines pulled into the dossier at Step 6a — orient the topics as leads (and can trigger the technology-event topic), never as evidence: a seed's claim counts only once the model deep-reads its underlying source.
  - No other topic's findings — a later topic gets nothing from an earlier one. The topics meet only downstream, at the Step-6d distillation (a single consolidation call, or a reduce when the research is large).

- **What is retrieved during a pass (the data)**
  - Live web-page text — the pages the model deep-reads, fetched and readability-extracted from the open web. This is what "current web sources" means.
  - Cached pages — previously-fetched pages under about four weeks old come from the document cache (Layer 1) instead of the network, carrying their original retrieval timestamp; new URLs are fetched live.
  - Search backend, not a separate data source: the orchestrator runs search SearXNG-first, falling back to Tavily only when SearXNG can't serve.

- **Who owns the context, and what persists**
  - The orchestrator owns the prompt: on every turn it appends the tool results and the model's non-thinking output (a tool request, or the pass's findings), threading the growing context forward — the model only requests tools, it never touches the network. Prior `<think>` blocks are stripped from history, never accumulated across turns (`docs/local-model-operations.md` §Strip thinking from history).
  - Carried across the topic's passes (canonical): the append-only evidence ledger and the accumulated per-pass findings, which the orchestrator assembles for the Step-6d distillation. The framing inputs (dossier facts, questions, seed) anchor the conversation from its start.
  - Raw fetched page text is the bulky working material: it may roll off the context as a pass proceeds, and the durable record of what a page yielded is its claims in the ledger, not the page text itself. How much raw page text survives across a pass boundary is not pinned by the contract.

- **Fitting the fixed context window**
  - `num_ctx` is fixed per model and never raised to make room — raising it reloads the runner and starves memory (`docs/local-model-operations.md` §The num_ctx trap); context pressure is answered by dropping content, not by growing the window.
  - When the prompt approaches the ceiling, older raw page text rolls off the working context — but only after its claims are banked in the ledger, so nothing is silently dropped. The roll-off is pressure-driven within a pass; the contract fixes only what is eligible to roll off (raw page text, never the ledger), not an eviction order or a trigger threshold — this stage is still a stub.
  - It never relies on the model server's own truncation, which silently front-drops the prompt's head and leaves the model to hallucinate over the gap.

- **What each model call returns**
  - Inside a pass, a turn either requests `web_search` / `web_fetch` calls — the orchestrator executes them and returns the results for the next turn — or, once the topic is answered or the budget is spent, emits that pass's findings.
  - The model authors each pass's findings write-up; the orchestrator only accumulates them — there is **no topic-close model synthesis**, so the first model consolidation of the findings is the Step-6d distillation.
  - Each ledger entry is a hybrid: the model supplies the claim, the orchestrator stamps its provenance (the source URL / timestamp).

- **Follow-up passes**
  - A follow-up is the model's **proposal** — a structured field the orchestrator reads and decides whether to spend; the model never recurses on its own.
  - It is granted only while depth remains (≤2 follow-ups) **and** the per-item budget has room; on exhaustion it is simply not spent (fail-soft, no follow-up).

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

### Failure and output

- **Failure logic**
  - Web failure reduces evidence.
  - It may lower conviction.
  - It does not automatically fail the run.

- **Output**
  - Full findings for every worked topic; any lower-priority topic the budget couldn't reach is a recorded degraded-input gap (lower conviction).
  - Evidence ledger with sources and timestamps.
  - Proposed follow-up and forward facts.

---

## Step 6d — Distill the research

- **As-built**
  - One unconstrained non-thinking condense of the stub note.
  - No evidence-ledger leg, hierarchy, or output schema until research lands.
  - The contract below is the designed research-loop shape; it binds when research lands.
  - A role-risk-only holding makes no research or distillation call at all.

- **Data retrieved**
  - No new external data.

- **The consolidation call — single or hierarchical (designed — research loop)**
  - The stage's full input is the findings from every worked topic, this run's evidence ledger (claims + sources), and — for a holding with a non-expired cache — each topic's seeded prior object merged into its own topic.
  - The orchestrator, never the model, sizes that full input to choose single-pass vs hierarchical **deterministically** — so growth *across* topics trips the hierarchical path rather than overflowing one call; the thresholds are config knobs.
  - That aggregate is what the orchestrator sizes to route, **not what any one call receives**: only the single-pass call sees every topic's findings whole, while hierarchical routing partitions them across the tier-1 → reduce shape below.
  - **Single-pass:** one call over every topic's findings, its output **keyed by topic** — globally reconciled, since the one call sees every topic — so each topic's group persists as that topic's next-run seed.
  - **Hierarchical (large input):** distill each topic tree separately (**tier-1**, feeding the reduce, not persisted raw), then one final combining call (**tier-2 reduce**) over the tier-1 objects — it emits the one combined object interpretation reads **and** the per-topic seed layer reconciled to the global winners, that reconciled layer (not the raw tier-1 output) persisting as the next-run seeds.
  - Either path preserves each claim's citations end to end.

- **Merging the seeded prior (Layer 2 cache) (designed — research loop)**
  - **Within a topic** — each topic's prior object merges into that topic's fresh findings where the topic is first reduced (the tier-1 call when the run goes hierarchical, the single call when it is small), fresh superseding cached on conflict.
  - **Across topics** — the reduce (the tier-2 reduce, or that same single call) applies the same *newest-wins-by-claim/metric* rule **globally** and dedups sources, so a metric freshened under one topic supersedes a cached copy another topic carried forward, and nothing is double-counted or left conflicting.
  - **The persisted seeds inherit that reconciliation** — in the same reduce pass, each topic's next-run seed object is written already updated to those global winners, so no seed keeps a value another topic superseded and no later run can resurface it once the fresher topic goes dormant (why the reduce owns this match rather than a later app step: `docs/portfolio-analysis.md` §Starting parameters).
  - **Claim expiry is by claim, not object** — a cached claim past ~4 weeks by its own vintage that this run didn't re-confirm expires rather than riding forward; each surviving claim keeps its vintage and its fresh-vs-carried mark.

- **If a topic's own input overflows one call (designed — research loop)**
  - **Trigger** — the fallback fires when the topic's **complete input** *summed* would exceed one distillation call: all its passes' findings, their evidence-ledger entries (claims + sources, which grow with the research, unlike the bounded thesis-ledger conditions seeded at 6c), and its retained prior. The sizing is measured on that whole aggregate, not pass by pass.
  - **Map (one distillation call per pass)** — the topic sub-distills along its ≤3-pass seam: the model condenses each pass on its own into a compact per-pass object (that pass's findings *and* ledger entries together).
  - **Reduce (one more distillation call)** — a tree-level reduce then combines those per-pass objects **with the retained prior** into the topic's single tier-1 object, which joins the outer **tier-2 reduce** across topics like any other (the sub-distillation is invisible above this point). So building that topic's tier-1 object takes one map call per pass plus one reduce — two to four distillation calls (four for a full three-pass topic) — versus the single distillation call a non-overflowing topic uses, separate from the multi-turn calls the topic's research passes already spent at 6c.
  - **On further overflow** — if even the tree-level reduce would overflow, the sub-distillation cap fail-softs the lowest-priority whole passes to a recorded gap — each dropped pass taking its findings and ledger entries with it, never the prior — so an overflow costs research detail, never the topic's seeded status.

- **Model**
  - Consolidates evidence.
  - Does not perform new searches.
  - Does not calculate financial numbers.

- **Typed outputs when supported (designed — research loop)**
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
  - Two schema-validated artifacts, both emitted by the reduce (or the single-pass call) from one reconciliation, so they stay mutually consistent.
  - The combined distilled findings object interpretation reads — the cross-topic reduction of the per-topic layer.
  - The fresh per-topic seed layer, one object per analyzed topic, persisted as the next run's seed.
  - The audit records seeded-vs-cold **per topic** with each seeding object's vintage — a standing topic can seed while a newly-activated conditional topic runs cold.

---

## Step 6e — Recalculate targets using validated research

- **As-built**
  - The research-assumption legs below are designed; they land with the research loop.
  - The pre-profit overlay is already complete from Step 6b (its statement-driven rule consequences included); the observation-driven refinement here waits on the research loop.

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
