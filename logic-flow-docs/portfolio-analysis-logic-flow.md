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

## Step 6 — Per-holding analysis loop

The following sequence runs once for every holding in the work list.

Each completed holding is designed to checkpoint separately; as-built only the between-holdings cancellation check exists.

### Work-list logic

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
    - Side reversed — the carried verdict now sits on a net-short position, so its thesis is for the opposite side (a directional verdict is only ever authored for a long; the verdict is marked `side_reversed`).
    - Stale vintage — the carried verdict is older than the ~4-week window.
    - Each is a non-blocking badge on the card; the user acts on it by selecting the holding or running a full analysis, so an urgent single-holding run is never blocked by the rest of the book.

- **Resume behavior (designed, not built)**
  - Resume uses the interrupted run’s pinned holdings and context.
  - No fresh Schwab pull occurs.
  - Starting resume window: about 48 hours.

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

#### Fund routing

A fund’s route — read from the `etf/info` and weights gathered above — is applied by the fund engine at Step 6b.

- **Fund routing**
  - US equity exposure with usable weights → priced-fund path.
  - Bond or commodity fund → role-risk-only path.
  - International fund below the US-exposure guard → role-risk-only path.
  - Leveraged or inverse fund → role-risk-only path.
  - Option-overlay fund → structural path-dependence flag; other priceability rules decide the route.
  - Mutual fund without usable weights → role-risk-only path.
  - Closed-end fund → the price-versus-NAV leg is designed; as-built it routes as a generic fund.

#### Semantic recall (designed)

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

#### Engine primitives (used in the formulas below)

Three primitives recur in the value formulas that follow, so they are defined once here.

- **`scale(x, lo → hi)`** maps `x` linearly onto 0–100 and clamps it: `lo` scores 0, `hi` scores 100, and an **inverted** band (`lo > hi`) scores lower inputs higher.
- **`average(…)`** is the unweighted mean of whichever legs are present — a missing leg is dropped *inside* a value; a wholly-absent value is handled per section (usually imputed to a neutral 50 at the roll-up, never dropped there).
- A **ratio** `a ÷ b` is `None` when the denominator is missing or zero; a few ratios below add a stricter `> 0` guard, noted where they do.

#### Pre-profit overlay (stocks, computed first)

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
  - **Execution read** — guidance-vs-actual misses, over higher-is-better operating metrics only. A guidance row and an actual pair only when their metric identity, units, issuer scope, and reporting period all match; the *bound* is the range's stated low (a range low winning over point guidance when both are present) and must be finite and positive. A paired period *misses* when `(bound − actual) ÷ bound ≥ 5%`; `material_single_miss` when the newest comparable period misses by ≥ 20%; `repeated_miss` when ≥ 2 of one metric's latest four comparable periods miss (different metrics never combine, and two missed metrics in one period never count twice). [note: the research producer is dormant as-built, so this sees only carried prior observations.]
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
  - Input: trailing return (first-to-last close over the available history).
  - Equation: `scale(−0.30 → 0.30)` (−30% → 0, 0 → 50, +30% → 100).

#### Scenario targets (priced stocks)

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

#### Continuity and ledger checks

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

#### Other deterministic reads

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

#### Evidence floor

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

### Step 6c — Research the holding

- **As-built: stubbed**
  - No web research runs today; a single research-deferred note is recorded.
  - Every run to date has graded on the deterministic financials and the house view.
  - The loop below is the research slice's design.

#### How the stage runs

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

#### Work each topic — the research loop

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

#### Failure and output

- **Failure logic**
  - Web failure reduces evidence.
  - It may lower conviction.
  - It does not automatically fail the run.

- **Output**
  - Full findings for every worked topic; any lower-priority topic the budget couldn't reach is a recorded degraded-input gap (lower conviction).
  - Evidence ledger with sources and timestamps.
  - Proposed follow-up and forward facts.

---

### Step 6d — Distill the research

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

### Step 6e — Recalculate targets using validated research

- **As-built**
  - This step changes nothing today. Everything 6b produced — scenario targets, dead-money hurdle, letter, and pre-profit overlay — passes through unchanged into the verdict, because the two legs that would refine any of it are dormant while research is stubbed: no validated forward fact and no sourced observation reaches this seam.
  - By design the step *is* a refinement, and it would rewrite a **bounded subset** of 6b's outputs: the **affected scenario targets** and the **dead-money hurdle**, and on the overlay side the **observation history** (with its rejected-row and backfill-attempt audit), the **execution read** derived from it, the **severe-deterioration** state, and the **rule consequences**. A single validated observation can move the history alone without changing the execution read or consequences. It never touches the **letter or grade sub-scores**, nor the overlay's **statement-derived legs** — the statement inputs (runway, burn, margins, share count) and the financing / economics / dilution states built from them — which no research observation moves.
  - Because both refinement legs are dormant, the 6b overlay pass already ran the observation-merge machinery once, over an empty candidate list, so the overlay is complete *before* this point rather than finished here.
  - The one piece with genuine as-built content here is the overlay's matched **rule consequences** — computed at 6b, detailed below because 6b defers the detail here — which bind the engine arm at verdict assembly.
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

- **Target and hurdle refinement (designed — research loop)**
  - Validate each research claim before it can move a number: reject malformed, unsourced, or nonnumeric claims; a `supplement` may fill only a missing structured value; a `supersede` may replace structured data only when it is newer, comes from an approved primary-source fact type, and matches on metric, units, and period — otherwise the structured value wins. Record every accepted or rejected rule.
  - Then recalculate only what a validated forward fact touches: the affected scenario targets and the dead-money hurdle result, leaving the backward-looking grade sub-scores unchanged.
  - [note: no wire for this exists as-built — there is no `research_forward_assumption` type, and the closed-form re-anchor a recompute would call (`reanchor_scenarios`) is today reached only by the between-run quick check, never by this stage.]

- **Pre-profit observation validation (designed — research loop; validator built, producer dormant)**
  - Each sourced observation a research row supplies is checked **structurally** before it enters the period history — a finite numeric value, units, a reporting period, an issuer scope, a source URL, a publication date, a confidence in [0, 1], and a direction (polarity) consistent with the metric kind (the row also carries its actual-versus-guidance role, read later when pairing). A **malformed** row (any of those legs bad) or a **duplicate** — of a stored observation, or of an earlier accepted row in the same batch — is rejected and logged with its reason; every other row is accepted and merged into the period history (append, sort, dedup). A structurally valid but as-yet-**unpaired** row — an actual with no matching guidance, or the reverse — is kept, not rejected: pairing into misses happens later in the execution read, and an unpaired row simply waits for a future match.
  - Two legs cannot be built until the research producer exists, and the research-loop slice owes both: **confirm the correct company** (the holding-identity cross-check — the typed row carries no issuer symbol yet) and **confirm the source states the number** (source-text corroboration — no source text exists while the producer is dormant). Until then the structural validator alone is not the whole contract.
  - As-built the validator runs on an empty candidate list, so the overlay sees only carried prior observations plus the statement-derived legs, and no observation is ever guessed. [note: periods compare exactly after trimming and order lexicographically, so the live producer must normalize each issuer's periods to one convention — ISO period end preferred.]

- **Cold-start and history-gap backfill (designed — research loop; dormant)**
  - When live, a backfill search is required on the first overlay-eligible full pass, and again whenever a previously used guidance metric has fewer than four comparable stored periods; it searches the latest four reported periods, records every period and source checked, and marks its coverage complete, partial, or unscorable.
  - Missing history stays a gap — an observation that was not found is never inferred.
  - As-built no backfill is required (the producer is dormant); the persisted attempt record exists only to pin the shape, and only prior attempts are carried forward.

- **Role-risk-only rule**
  - Skip the step: no price targets or priced-stock overlay exist to refine.

- **Model**
  - None.

- **Output — what leaves the step**
  - **As-built:**
    - The **final target set** and **hurdle read** — Step 6b's values, unchanged.
    - The **final pre-profit overlay** — Step 6b's, whose matched rule consequences are now ready to bind the engine stand-in arm at verdict assembly.
    - No research assumption is logged — the producer is dormant, so there is nothing to resolve.
  - **Designed (research loop):**
    - The refined target set and hurdle after validated forward facts are applied.
    - The logged research assumption and its resolution — accepted or rejected, with the deciding rule.
    - The overlay's merged observation history (and its rejected-row / backfill audit) and the execution read, severe-deterioration state, and rule consequences re-derived over it.

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
- **Data gaps** — the dossier's degraded-input notes naming financial legs the gather could not resolve (e.g. an SEC CIK-mapping miss, an SEC company-facts fetch failure, an unwired fund-metadata source); distinct from the per-metric `(gap)` markers in the computed-metrics block above, which flag one missing computed value.
- **Distilled research** — as-built the condensed **research-deferred stub note** (research is stubbed at Step 6c, so no sourced findings reach the model); the merged object of this run's fresh findings plus any seeded prior is the *designed* shape.
- **House view (loaded only when the latest report is ≤ 7 ET days old; older drops the whole view)**
  - The latest report's Thesis, Investment Strategy, and Forward Outlook sections.
  - Recent report stances — up to three, the most recent by date: date, thesis stance, and risk posture.
  - Both are scope-limited to horizon reads and market setup — context for the outlook, never by itself a reason to exit the holding. (In the prompt the explicit caveat sits on the latest sections; the stances ride as a bare list.)
- **Continuity block**
  - Whether a prior verdict exists.
  - A band-recalibration note when the grade bands changed since the prior letter.
- **Retrospective (when a prior priced verdict exists)**
  - The prior run's engine arm in full: grade, sub-scores, targets, conviction, outlook, action.
  - The prior run's model arm in full, labeled as the model's own.
  - The price move since the prior read.
  - The holding's matured scoreboard lines.
- **Prior thesis ledger** — one ledger per holding, model-authored on the prior run and shared by both arms (not an engine-arm or model-arm ledger of its own).
  - The prior ledger as a **model-facing projection** (not the complete persisted record) — thesis (original + current), key drivers, the whole bear/base/bull monitor, and every falsifier and trigger (both roles), each with its statement plus, for quantitative ones, the machine core and current breach streak. Unscoped, unlike the house view and retrospective above. Held out of the prompt: the app-owned bookkeeping (condition ids, supersession lineage, downgrade/trip flags, the rest of the evaluation state, the authored band relation) and the model-authored falsifier `technology_class` tag.
  - Beside it, **this run's engine condition evaluation**: the engine's deterministic re-evaluation of that ledger's *quantitative* conditions against this run's computed surface — each crossing tagged confirmed or first-breach, plus the typed unevaluable notes. The engine evaluates the conditions; it does not author the ledger.
- **Deliberately excluded**
  - The investor profile.
  - The engine's current-run stand-in outlook, conviction, and action picks.
  - Raw statements, filings, and price-bar series — only computed values and distilled research reach the model.
- **Designed additions not yet in the prompt**
  - The implied-expectations range.
  - The narrative-versus-reality read.
  - Absolute street opinions.
  - The same-stock option overlay.
  - Positioning-context feeds — insider and congressional activity, and FINRA short interest — gathered at Step 6a once their data legs land; on the live options signal's precedent they reach the model as positioning evidence, held out of the grade.

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
  - The model arm is its own: structurally validated only, never checked against the engine's numbers.
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

### Step 6g — Validate continuity and checkpoint

- **As-built**
  - The validators here run every run, but the legs that depend on the stubbed research producer or its unbuilt downstream validator are dormant: the what-changed **attribution** check, the repeated-execution-miss cap's trigger, and the ledger's qualitative-trip → sourced-research leg. What runs today: the two-arm stamping, the engine-series ledger validation, the live severe-deterioration cap, and the attention clear-and-acknowledge; the per-holding checkpoint is designed, not built.

- **Data retrieved**
  - No new data.

- **Two-arm stamping rule**
  - Engine values are app-stamped directly, never echoed through the model.
  - Nothing the model returns can alter an engine grade, target, overlay value, or monitor stamp.
  - The model arm's own numbers are structurally validated only, never compared against the engine's.

- **What-changed validation (designed — the attribution validator is unbuilt)**
  - As-built the what-changed audit is model-authored prose, persisted as returned; only the interpretation prompt disciplines it, and the self-correction counters stay structurally zero. No app code checks the attribution today.
  - Designed, every claimed external change must map to one of:
    - An input-delta entry.
    - A sourced research finding.
    - An accepted forward assumption.
  - Then an unsupported change becomes a labeled self-correction, or the response fails validation.
  - [note: the forward-assumption leg has no wire at all — there is no `research_forward_assumption` type in the code (the same gap Step 6e records); the input-delta and research legs await the validator, and the research leg additionally awaits the stubbed research producer.]

- **Conviction and cap handling**
  - The model's conviction is its own; no app recalculation, ceiling, or clamp touches it.
  - The old one-level conviction raise and its re-derivation are retired.
  - Matched cap rules record as audit annotations that bind the **engine stand-in arm only** (the overlay-derived caps are computed at Step 6b, their mechanics detailed at Step 6e §Pre-profit rule consequences). Their as-built status:
    - **Severe deterioration** (→ Low ceiling, engine set limited to Trim or Sell all) is **live** — its legs are statement-derived (economics, financing/runway, dilution), so it can trip without research.
    - **Repeated execution miss** (→ Medium ceiling) is built but **dormant** — its execution read needs the stubbed research observations.
    - **Hard forensic trip** (→ Low ceiling, Add rungs leave the set) is **designed, unbuilt** — it has a separate producer (a filing-classified restatement/auditor change, or a validated `forensic_event` research claim) that does not exist yet, so the overlay carries no such rule today (its ceiling is Medium or Low, both from the deterioration legs above).
  - The strictest matched ceiling wins on the engine arm.
  - A model value past a ceiling renders beside the recorded rule.
  - Model prose cannot create an overlay warning state — the overlay is computed deterministically from statements and (dormant) observations.
  - Grade remains unchanged by these caps.

- **Ledger validation** (built — the seam runs every run)
  - Tripped quantitative condition must map to a confirmed engine crossing.
  - Tripped qualitative condition must map to sourced research — as-built this leg always clears the trip and logs it, because research is stubbed and no source-backed finding exists to support one.
  - New quantitative conditions must resolve to an engine series.
  - Unresolvable condition becomes qualitative (downgraded and logged, never dropped).
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

## Step 7 — Roll up the run and score past decisions

The post-loop stage — by now every action that exists is final, whether set in this run's per-holding loop, carried from a prior run, or rule-demoted by the app, so this step makes none. It does two independent things: **roll up** the finished run into a book-level summary, and run **outcome learning**, which grades how *earlier* decisions have actually turned out.

### Roll-up

A descriptive, book-level summary of the finished run — for the results display and the stored run record. It decides and drives nothing.

- **Calculations**
  - Verdict counts by disposition (graded, role-risk-only, not-rated, insufficient-evidence — role-risk-only kept separate from graded).
  - Largest single-position weight and cash weight (descriptive reads only).
  - Positions closed since the prior run, acknowledged rather than dropped.
  - Run-level data-health read — target-provenance and degraded-input aggregates, the generation-health signals (context-pressure and output-length-stop), and the run's attention flag.
  - A deterministic one-line run overview string.

### Outcome learning

Outcome learning has two halves that share one unit, the **decision episode** — a bounded twelve-month instrument measuring how a single recommendation actually turned out. One half **scores** *earlier* episodes as their outcome windows come due; the other is **where this run's decision becomes an episode**, the raw material later calibration reads. For each holding the run compares its recommendation state against the prior run's and **opens a new episode when that state has changed** — a verdict-**branch flip** (priced ↔ role-risk) or a change in the **action** — otherwise extending the holding's still-active episode. Each episode carries **outcome labels** due at 1 / 3 / 6 / 12 months (both terms are defined in Important terms): when a window arrives its label is scored, or — if price coverage is missing — held pending inside a grace and closed unscorable past it.

- **Data retrieved**
  - FMP dated-EOD bars for maturing outcome episodes.
  - FMP dividends for maturing outcome episodes.

- **Logic**
  - Tag net alignment from the holdings diff — only for still-untagged episodes anchored to the immediately-prior run (that diff observed only that move); nothing is tagged on a first run.
  - Mature any window labels whose dates have arrived, including for symbols no longer held.
  - Each matured (scored) label measures, from split-adjusted daily closes: the price-only return (the always-present cross-entry common basis) and the maximum drawdown, plus — each recorded with a typed gap when its source is missing — the dividend-inclusive total return (the primary basis) and the price-only spreads vs the market (`^GSPC`) and the entry-stamped sector; on the 12-month window a confirmed ledger falsifier attached to its still-active episode (not one that landed post-maturity), whose bear-line basis resolves, additionally carries its signed trading-day lead time to the first close below that line, or `no-material-drawdown`.
  - A failed price refresh leaves the label pending while inside the coverage grace; past the grace it closes as a typed unscorable label rather than staying pending. A failed dividend pull instead degrades to a price-only label, never blocking maturation.
  - Append or extend this run's decision episodes — the run's episode-creation step: open a new episode when a holding's recommendation state changed since the prior run (a verdict-branch flip or an action change), otherwise extend the still-active episode.
  - A holding's first analysis opens a debut episode; an abstention extends the standing episode without opening one; a reaffirmation after the episode has matured records nothing.
  - A thesis-change trigger is designed but dormant until the attribution validator lands; wording-only thesis edits never open an episode.
  - Derive the scorecard reads over the updated episode set — the reads below.

- **Scorecard reads** (derived deterministically over the updated episode set — engine-computed, never model-judged; of them the roll-up surfaces only the head-to-head and outlook-direction reads, as the model-vs-engine scoreboard, and each holding's own matured window lines ride back into its next interpretation — they decide nothing on their own, the calibration loop they feed only ever proposes)
  - Both arms are scored, separately: the engine baseline and the unrestricted model arm each froze their own targets and outlook on the episode at open, and the reads below score each arm on its own — the target-band read is the one place they meet directly head-to-head — because grading the model against the baseline is the whole point of the two-arm design.
  - **Target-band calibration** — the bear–bull band's coverage of the realized price against its declared nominal 80%, an interval score rewarding calibration and sharpness together, and the base case's mean signed error; scored on the price-only label at the **1- and 12-month windows only** (each band against its matching window — the 3- and 6-month labels are never band-scored), over vintage-fresh priced episodes, split by target-parameter version so a recalibration never mixes bases. The same scorer runs unchanged for the engine bands and for the model's frozen bands.
  - **Engine-vs-model head-to-head** — that same interval score and coverage for both arms over the paired population alone (the 1- and 12-month episodes where both arms carried the band and the window scored), so neither arm is graded on an easier sample; this is the only read the two arms are directly compared on.
  - **Outlook direction hit-rate** — each arm's short / mid / long read scored against the realized price sign at its mapped window (short → 1-month, mid → 6-month, long → 12-month); a flat outcome scores a directional call as a miss, and a neutral read is counted beside the hit-rate, never inside it.
  - **Action cohorts** — mean total and price return plus vs-market / vs-sector spreads, grouped by the action rung recorded at episode creation, across all four windows: the cohort spreads the action ranking is read from (do the add cohorts out-return the hold cohort, and hold the trim / sell cohorts). Computed over model-chosen priced episodes — a vintage-fresh intrinsic-layer set reported beside the all-model-chosen final-action set; role-risk-only and rule-demoted episodes are counted in their own classes, out of the pooled read.
  - **Falsifier lead times** — the 12-month bear-line crossings above, surfaced per episode.
  - **Proposal eligibility** — a gate counting the unique holdings with a scored matured window against a bar (drafted 30). **As-built the gate is built but the proposals are not**: below the bar the pass records the typed below-bar note and proposes nothing, and above it the proposal statistics still land with a later slice once enough matured data exists — and even then the loop only proposes, never auto-applies.
  - [note: the self-correction accumulation is a scorecard field but reads structurally zero — its producer is the dormant 6g what-changed attribution validator, the same one gating the standing-thesis episode-open leg above.]

---

## Step 8 — Save the run and learning history

- **Data stored** (the whole run persists as one serialized blob; the *Dormant* and *Designed* groups at the end are not populated on a run today — each states why)
  - Normalized holdings snapshot used by the run.
  - Every intrinsic verdict.
  - Every portfolio action and its rationale.
  - Thesis ledgers and condition evaluation states.
  - Analysis vintages (attention flags live only in the quick-check store).
  - Portfolio roll-up.
  - Sources and timestamps.
  - Engine calculations, each holding's categorical position-change tag, and the roll-up's exited positions (the full position delta — prior quantity and cost basis — is runtime-only; per-value input-delta attribution is the designed input-delta validator's).
  - Every priced stock's pre-profit overlay record — the runway, economics, dilution, and severe-deterioration states computed live from statements, with the conviction, action, and cap rules they fire.
  - What-changed audits.
  - The outcome-learning records for this run — the opened-episode notes, the symbols whose episode this run extended, the net-alignment tags, the matured window labels, the symbols with a window still pending on a price-coverage gap, and the derived scorecard reads (detailed in Step 7's outcome learning).
  - Model, prompt, schema, and parameter versions.
  - Degraded-input flags.
  - *Dormant producer — no pre-profit research loop feeds these yet:* the accepted pre-profit observation history (period-keyed) and the backfill legs carry forward from the prior run, and the execution-miss state and its rule recompute from that carried history each run; the rejected-observation list, by contrast, is rebuilt from the current candidate batch, not carried. All are empty on a fresh v9 store today — by carry / recompute over an empty producer, not a forced-empty field.
  - *Designed — lands with the research loop:* per-topic research-reuse decisions (seeded-from-cache vs cold, each with its seeding vintage) and accepted / rejected research assumptions; distilled research itself is a transient prompt input, not persisted.

- **Decision-episode logic**
  - Decided in Step 7's outcome learning (the open / extend rule lives there); this step only persists the resulting episodes.

- **Episode contents**
  - Anchor date.
  - Intrinsic-analysis vintage.
  - The action.
  - Decision-time calibration snapshot (priced branch only): both arms' targets, sub-scores, outlook, and conviction, plus the engine arm's grade, hurdle, dead-money, and cap signals.
  - Sector identity for later benchmark comparison.
  - Grade and target parameter versions.
  - `model-chosen` or `rule-demoted` action source.

- **Retention**
  - Keep newest 30 Portfolio Analysis runs.
  - Keep outcome episodes independently until their labels mature.
  - Freeze matured episodes into their own capped archive.

- **Embedding model**
  - Embed a calibration learning only when this run records newly matured outcome-window labels — keyed to window-label maturation, not to an episode freezing into the archive, and not fired every run.
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

Display is a **pure read**: the frontend invokes read-only commands that return the persisted run blob verbatim and renders it — no model runs, and the backend shapes nothing (one presentational Vue page computes every derived read from the returned struct; the app layer owns the invokes).

- **Data retrieved** (read-only commands — no model, no view-model shaping)
  - The **latest run** — the whole persisted run blob, selected **id-primary (insertion order), never `created_at`**, so a backward clock step can never hand the page to a prior run while the just-saved one sits invisible.
  - An **older run by id** for the read-only history view — a pure read, with no re-run entry point.
  - The **run-summary listing** for the sidebar (columns only, never blobs).
  - The **latest standalone holdings snapshot**, when present — a separate single-row store distinct from the snapshot inside each run and never read by the job; when a run exists it feeds the dual-vintage comparison, and with no run yet it is the page body on its own.
  - The **latest quick-check state**, loaded alongside — the overlay that badges the latest live view (below), applied only when its swept run matches the rendered run.
  - **As-built** an unparseable stored run costs only its own surface: the latest read skips it to the next-newest, an id fetch reads it as not-found, and the listing still emits its row marked **unreadable** with zeroed counts — never dropping the history listing or the next run's baseline.

- **Per-holding display — priced branch** (the two arms in a paired side-by-side grid)
  - **Engine-baseline arm** — the letter sub-scores (quality / valuation / risk) plus a divider-separated **Setup** tile (the market-setup read, deliberately outside the letter), the engine conviction meter, the 1- and 12-month targets (base plus bear–bull band, each shown only when the engine authored it), the engine outlook (short / mid / long), the engine's own action, and a target-methodology reveal; the card-head letter is the engine (canonical) grade.
  - **Model-view arm** — the model's own letter, sub-scores, conviction, 1- and 12-month target bands (always present on this arm), outlook, and action.
  - **Divergence tags** — a quiet **≠ engine** tag rides the model arm wherever it departs from the baseline on **conviction, outlook, or action**: a display cue for where the arms differ, distinct from the scoreboard's scoring (which grades the target bands head-to-head and the outlooks per arm, and leaves conviction and action unscored). The letter, sub-scores, and target bands carry no tag, and an authored **inverted-band** note is a data-integrity flag rather than a divergence.
  - **Standing thesis** (clamped, with a reveal) and the **thesis monitor** — each scenario's probability, engine target (when non-null), and conditions, plus the improve / must-not-break goalposts.
  - **Action + rationale** as a full-width row beneath the arms, with the position weight and — when present — the same-stock **options-activity** signal (put/call volume and open interest, ATM IV, IV skew).
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
  - [note: the sidebar's per-run **"rated N"** binds to the graded (priced-only) count while the word reads broader than the number it shows — a recorded open-ruling item, unsettled here.]

- **Holdings display — dual vintage**
  - The standalone holdings pull is **view-only, never merged into the run-anchored cards**, and the **frontend** decides freshness (the backend hands over both timestamped payloads and compares nothing): when the pull is newer than the run, a separate **Current holdings** section renders **above** the verdict cards, stamped with both vintages and carrying presence-only churn tags (*new · not in last analysis* / *no longer held*).
  - The older analysis cards are never mutated, and the whole comparison is suppressed on a historical view.

- **Sorting** (display-only — reorders already-computed cards, computes nothing)
  - Four keys — **Value**, **$ gain**, **% gain**, **Cash invested** — a stable in-place sort with an alphabetical ticker tie-break, nulls last, and the last-used key persisted; shown only with more than one verdict. The current-holdings table carries its own independent column sort.

- **Read-only past-run view** (any sidebar row but the newest readable one)
  - The older run renders on the same page with every trigger locked — run analysis, pull holdings, and quick check all disabled with the reason stated — no selection controls, and the current-holdings comparison suppressed.
  - A quiet informational **vintage banner** names the run's date and carries **Back to latest**.

- **Model**
  - None — no display command invokes a model; the page is a deterministic render of the persisted run.

---

# Quick check

`Load last run → Refresh monitorable data → Evaluate ledgers → Raise warnings → Save state`

- **As-built**
  - The quick check **runs today, engine-only** — no model call, no web research, no Schwab pull. Every leg below is live except the FINRA short-interest refresh, which is unwired (its own subsection below).
  - It is the between-run freshness safeguard the 2026-08-16 badge ruling leans on: it **warns without deciding**, and its warnings ride as non-blocking card badges, never a forced re-analysis.

- **Purpose**
  - Keep existing thesis ledgers alive between full analyses.
  - Warn without rewriting decisions.

- **Data retrieved from local storage**
  - Last analysis run’s holdings snapshot.
  - Existing thesis ledgers.
  - Stored target inputs and rate anchors — the last full pass's drivers, spread / raw-multiple percentiles, spot, and forward-dividend leg (the quick-check basis the between-run engine re-anchors against).
  - No fresh Schwab holdings pull.

- **Shared data refreshed**
  - Current holding prices from FMP — the live `quote` plus dated-EOD closes (two FMP calls per holding; the sweep never reads the shared price cache).
  - `DGS2` and `DGS10` from FRED (one print each).
  - A failed rate pull fails soft to the freshest cached print — a prior quick check's own print first, else the last run's — eligible only within a drafted ~1-week bound (`RATE_CACHE_MAX_AGE_DAYS = 7`).
  - No eligible rate cache → the rate-dependent families read `unknown`.
  - A failed price refresh has no cache to fall to → that holding's market family reads `unknown` and its price-dependent reads skip.

- **Per-stock data refreshed**
  - Always, per stock: the SEC EDGAR filing check (CIK-gated), an analyst-estimate snapshot (the revision preflight), and an earnings-history re-pull.
  - Only after the EDGAR check surfaces a **new filing**: an income-statement, balance-sheet, and dividends re-pull, so a filing-cadence condition's fresh observation arrives with the value it reads. [note: no cash-flow re-pull exists; the hurdle's payout term is the dividend leg alone.]
  - Only for a holding carrying a **standing technology-class falsifier**: a `news/stock` pull (the qualifying-news-seed leg).
  - Unresolved SEC CIK → filing family becomes `unknown`.

- **FINRA short-interest refresh (designed, not wired)**
  - Designed as a conditional once-per-run consolidated-file pull, read only when some holding carries a validated short-interest-fed condition.
  - As-built the leg is **absent**: the closed engine series surface has no short-interest series, so no condition can validate as short-interest-fed and the trigger never arms. It activates only when a short-interest series joins the surface.

- **Fund data refreshed**
  - `etf/info` plus **both** the sector and country weight sets — fetched unconditionally for every fund (bond and commodity funds included).
  - The equity / condition gating is evaluation-side, not fetch-side.

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
  - Newly failing dead-money read → amber attention flag.
  - Band relation changed since authoring (left, re-entered, or crossed the band) → amber attention flag.
  - New earnings, filing, revision, or qualifying news → quiet evidence-event badge.
  - Fund mandate, expense, or major exposure change → quiet evidence-event badge.

- **State updates**
  - Advance a condition streak only on a distinct new observation (a new trading day's print, a new filing).
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
  - Normalize holdings by ticker — the same signed-quantity / cost-basis netting as the analysis run.
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
