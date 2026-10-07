# Portfolio Analysis: logic flow

> This describes the designed job behavior.  
> Some parts are not implemented yet.

`Gate → Pull holdings → Classify positions → Compare with prior run → Load context and score due forecasts → Analyze each holding: engine → research → review the prior call → thesis → action → Roll up and open episodes → Save → Display`

## How to read this document

- The job is built as a **research instrument**: it tests whether a local reasoner, given primary-source data and the open web, can write a defensible thesis for a held position and price it.
- Every holding's result carries the judgment **twice**, as two arms with different provenance:
  - The **engine arm** is a deterministic Rust calculator over primary-source data — a grade, scenario bands, a risk tier, a return hurdle, a forensic read, and its own action rung. It never consults the model.
  - The **model arm** is the reasoner's own prose — a thesis document with its conviction and expected prices — plus the rung it picks at the action call. The app checks it on **type alone** (a price is finite and positive, a rung is on the ladder), never on content.
- Three rules keep the experiment honest:
  - Nothing the model writes ever alters or binds an engine value.
  - Both arms are scored the same way against the same realized prices.
  - The model reads its own track record and the engine's before it writes again.
- One local model does every reasoning job by switching mode: **thinking** for research, the write-ups, the analysis, the review, the thesis document, and the action call; **non-thinking** for distilling write-ups and for transcribing the thesis document's stated values into a typed object.
- Every model prompt is **one message in two parts**: the data with a one-time gloss of each section, then the task. No prompt names the app, its stages, or its arms.
- The canonical specification is `docs/portfolio-analysis.md` and `docs/portfolio-workflow.md`, with `docs/web-research.md`, `docs/local-models.md`, `docs/storage.md`, `docs/data-sources.md`, and `docs/schwab-integration.md` behind them. This document is the plain-language map, not the specification.

## Important terms

- **Holding**
  - One investment currently in the portfolio.

- **Normalized holding**
  - One combined position per ticker.
  - Rows from multiple accounts are added together.
  - Long and short quantities offset each other.

- **Gradable holding**
  - A holding the job can analyze honestly.
  - A US-listed stock or a supported equity fund.

- **Not rated (`not-rated`)**
  - The investment type is outside the grading system.
  - Examples: cash, bonds, standalone options, net-short stocks, non-US listings, ADRs.
  - Its real portfolio exposure may still be counted.

- **Insufficient evidence (`insufficient-evidence`)**
  - The holding is normally gradable.
  - Required data is missing, conflicting, or cannot be computed.
  - The job abstains instead of guessing.

- **Priced verdict (`priced`)**
  - Full analyzed result.
  - The engine's grade, bands, tier, hurdle read, forensic state, and rung beside the model's thesis document, conviction, and three expected prices.

- **Role-risk-only verdict (`role_risk_only`)**
  - Used when a fund can be understood but not honestly priced.
  - Common for bond, commodity, international, leveraged, or option-overlay funds.
  - Carries the engine's class, exposure, expense, and risk reads, and a thesis document describing the vehicle's role.
  - Carries no letter grade, no bands, no tier, no conviction, and no expected price.

- **Intrinsic verdict**
  - Judgment of the holding by itself.
  - Does not consider the investor profile or other holdings.

- **Portfolio action**
  - The holding's final disposition: one ladder rung plus a one-line rationale.
  - The profile-aware counterpart to the intrinsic verdict.
  - Never weighs other holdings; whole-book reconciliation belongs to the future portfolio planner.
  - The ladder: sell all, trim, hold, add, add aggressively.

- **Action call**
  - The model call that selects the portfolio action.
  - The only place the investor profile and the position's economics enter the job.

- **Grade**
  - A–F summary of the business's current quality, valuation, and risk.
  - Engine-computed, backward-looking, and kept apart from the forward read.
  - Momentum is not in the letter.

- **Sub-scores**
  - Quality, valuation, and risk, each 0–100, higher is better.
  - Momentum is computed beside them as market-setup context, outside the letter.

- **Scenario bands**
  - Bear, base, and bull prices at three months, twelve months, and three years.
  - Engine-computed from a per-share driver and a rate-anchored multiple.
  - The three-year band is a declared extrapolation.

- **Risk tier**
  - Deterministic estimate of investment risk: High, Medium, or Low.
  - Scales the return hurdle.

- **Hurdle and dead money**
  - The hurdle is the minimum annual total return needed to justify keeping or adding capital.
  - Three states: clears, indeterminate, fails.
  - Dead money is the `fails` state: even the bull case misses the hurdle.

- **Thesis document**
  - The model's prose record of its standing view on the holding: thesis, drivers, scenarios with probabilities, falsifiers and triggers in words, expected prices and conviction argued in the text, and a summary paragraph.
  - Read as text and validated by nothing.
  - Each run's document supersedes the prior run's.

- **Typed appendix**
  - The thesis document's stated values transcribed into a typed object: conviction and the expected price at each horizon.
  - Type-checked only.
  - Any field null where the document states no value; a null is acted on by presence, never read as content.

- **Conviction**
  - The model's confidence in its own thesis: high, medium, or low.
  - Stated in the thesis document and transcribed into the appendix.

- **Expected price**
  - The model's single-point expected share price at three months, twelve months, and three years.
  - Finite and strictly positive where stated, or the appendix is rejected; null where the document states none, that horizon then scoring the engine leg alone.

- **Write-up**
  - The prose a research pass produces on one topic.
  - Rewritten whole on a follow-up pass, so a topic has one write-up at a time.

- **Analysis**
  - One document consolidating this run's write-ups.
  - The only research artifact the next run reads: the holding's research memory.

- **Review**
  - The model's assessment of its prior position against what actually happened, written before this run's thesis document.
  - Where the holding's accuracy record reaches the model.

- **Distillation**
  - A non-thinking shortening of the write-ups, used only when the analysis prompt is over budget.

- **Fetched values**
  - The holding's provider data as returned, glossed once and never engine-computed.
  - The same block, byte for byte, leads the research, synthesis, and thesis-document prompts.

- **Page roster**
  - The list of every page shown to the model for a holding: address, title, publication date, retrieval time, and source tier.
  - Never page text. The durable provenance on the audit record.

- **Episode**
  - A price record opened for a priced holding: the model's three expected prices and the engine's three base values, with the creation date, that day's spot, and an anchor close.
  - Append-only. Never updated or deleted.

- **Check**
  - A comparison written onto an episode when a horizon date passes: the close on that date and both arms' scores.

- **Accuracy score**
  - A holding's mean per-check score at a horizon, 0–100, kept separately for the model and the engine.

- **Attention flag**
  - Amber warning raised by the quick check on one of its two engine monitors.
  - Suggests a selective re-analysis. Changes no verdict.

- **Evidence event**
  - New information a verdict has not seen: earnings, a material filing, a large estimate revision, or a fund's mandate or exposure change.
  - Rides as a quiet badge, never the amber flag.

- **Position delta**
  - The app-calculated change in a position since the prior run: new, increased, decreased, or unchanged, plus exited.
  - Reaches the model at the action call alone.

- **Vintage**
  - The date of the full pass that produced a holding's current verdict.
  - A carried verdict is vintage-stamped; past ~4 weeks it is over-age.

- **Split bridge**
  - The conversion that brings every stored price onto today's basis after a stock split.
  - Reads an anchor bar stamped at authoring against the same bar in a fresh fetch.

- **Pre-profit overlay**
  - Engine reads on a loss-making stock's cash runway, margin direction, and dilution.
  - Conviction and action context; never a grade input.

- **Hard forensic state**
  - A restatement or auditor change found in the holding's 8-K filings within the last year.
  - Bars the add family from the engine's action set. The model's rung is annotated, never overridden.

- **Engine action set**
  - The rungs the engine's own reads support for a holding, shown to the model as evidence.
  - The model may choose outside it; the departure is recorded on the audit.

## Main data sources

- **Charles Schwab**
  - Current holdings: symbol, description, asset type, signed quantity, average cost, market value, per-account cash and liquidation value.
  - Option chains for held stocks: volume, open interest, implied volatility, and per-contract delta.
  - Read-only by construction: the only calls are the positions read and the chains read. No order, transfer, or write endpoint exists in the adapter.

- **FMP**
  - Company profiles (name, exchange, sector, industry, website).
  - Quarterly income statement, balance sheet, and cash-flow statement.
  - Forward consensus estimates, dividends, live quotes, and the daily price history.
  - Symbol-scoped news headlines, used only as research leads.
  - Sector benchmark price histories (the SPDR sector ETFs) for the technology-event pre-flag.
  - The gold quote for gold-linked holdings.
  - Fund information, sector and country weightings, and the sector P/E snapshot and history.
  - Ratios and key metrics, financial scores, DCF, street targets and ratings, insider and congressional trades, peers, float, revenue segments, and the M&A feed.

- **SEC EDGAR**
  - XBRL company facts: the annual fallback behind FMP's statements.
  - Filing submissions: the item-classified 8-K sweep (auditor change, restatement) and the quick check's material-filing check.
  - The ticker-to-CIK resolver behind both.
  - Earnings-release recovery: an 8-K exhibit fetched when an issuer's own site blocks the research fetch.
  - Optional fund holdings through N-PORT: designed.

- **FRED**
  - The ten-year and two-year Treasury yields, normalized to decimals.
  - The ten-year's trailing history for the valuation-multiple anchor.
  - Oil, gas, and monthly metals prices for commodity-linked holdings.

- **FINRA**
  - Short-interest level, trend, and days-to-cover from the consolidated file, fetched once per run and looked up per stock.

- **CFTC**
  - Futures positioning for commodity, index, rate, and currency funds.

- **CBOE**
  - The broad-market put/call ratio, a venue-level sentiment backdrop.

- **SearXNG**
  - The only web search for holding research: a self-hosted, keyless metasearch instance.
  - Serper, a paid Google-results API, is wired inside it as a keyed engine that fires on every query, the reliable floor; the keyless engines stay as redundancy, and the key lives outside the repo.
  - Tavily is the report job's and is never called by this job.

- **Local storage**
  - The prior run's holdings snapshot and verdicts.
  - Each holding's prior analysis and prior thesis document.
  - The house view and the investor profile.
  - The page cache, the episode store, and the quick-check state.

---

# Full Portfolio Analysis job

## Step 1 — Start and gate

- **Data retrieved**
  - No investment data yet.

- **Presence checks** (lock the Run button and raise a persistent warning until fixed)
  - The Ollama endpoint and the reasoner model id are configured. Neither local job makes an embedding call; the roster carries no embedder.
  - Schwab is connected and its seven-day refresh token is still valid.
  - FMP and FRED credentials exist.
  - Each failure has its own warning category: local models not configured, Schwab connection, missing provider credentials.

- **Run-time checks**
  - No other job holds the single global run slot.
  - The local daemon answers and the reasoner is pulled. This local-only probe runs before the slot is claimed; every external fetch happens inside the slot. A failure blocks the attempt inline, never as a persistent warning.
  - Schwab API reachability is not tested here; an outage surfaces at the Step 2 pull.

- **Pre-run notice**
  - The app probes the SearXNG instance. If it cannot serve search, a confirm dialog says the run will research blind and is not recommended, with Proceed or Cancel. A consent step, never a block.

- **Model**
  - None.

- **Output**
  - Job starts.
  - Or the app explains what is missing.

---

## Step 2 — Pull and normalize the portfolio

- **Data retrieved from Schwab**
  - Every granted account's positions.
  - Symbol, description, asset type, signed quantity, per-unit average price, market value.
  - Per-account cash balance and liquidation value.
  - Option chains are not fetched here; each holding fetches its own at Step 6a.

- **Normalization logic**
  - Derive each row's cost basis as a signed total: average price × signed quantity. No option-contract or bond-par multiplier is applied, so the card withholds cost-derived figures for those two classes.
  - Reconcile any explicit cash row against the account's cash balance and liquidation value; fold it into cash exactly once. An unreconcilable account fails the pull rather than guessing.
  - Combine the same ticker across accounts: add signed quantities, signed cost totals, and market values.
  - The net quantity decides the position's side.
  - A netted quantity, cost basis, or market value that does not finish finite fails the pull naming the symbol.
  - Preserve original rows for audit and display.

- **Failure logic**
  - A failed or unauthorized pull fails the run. The last good holdings stay intact.
  - A resumed run performs no pull; it reopens its interrupted run's pinned snapshot.

- **Model**
  - None.

- **Output**
  - One normalized portfolio snapshot, pinned for this run and persisted with it.

---

## Step 3 — Classify each position

- **Data retrieved**
  - Uses the normalized snapshot.
  - No new external data.

- **Initial classification** (by Schwab asset type)
  - Stock → possible full analysis.
  - ETF or fund → reduced analysis path.
  - Option, bond, cash, or unsupported type → not rated.
  - Net-short stock → not rated, with a short-position reason.

- **Stock rule**
  - Final US-listing validation happens at Step 6a once the company profile is fetched.

- **Fund rule**
  - Final strategy routing happens at Steps 6a–6b once the fund's own metadata is fetched.
  - Supported US equity fund → possible priced verdict.
  - Structurally unpriceable fund → role-risk-only verdict.

- **Not-rated rules**
  - No fake grade is created; the reason is shown.
  - Cash still feeds the roll-up's cash read.
  - An option on a held stock links into that stock's dossier as an overlay.
  - Weighing a not-rated position's exposure against the book is the portfolio planner's job.

- **Model**
  - None.

- **Output**
  - A preliminary route for every position.
  - Explicit not-rated reasons.

---

## Step 4 — Compare holdings with the prior run

- **Data retrieved**
  - The current normalized snapshot.
  - The prior analysis run's normalized snapshot from local storage.

- **Calculations**
  - Compare position size by ticker: absolute quantity on a same-side move, the signed swing on a long↔short flip.
  - Tag each current holding new, increased, decreased, or unchanged.
  - Cost basis is corroborating context on a quantity move (paid up or averaged down), never a second axis; a cost-only change at unchanged quantity is an accounting event and is not surfaced.
  - A prior ticker now absent is exited.

- **Important rules**
  - Standalone Pull holdings snapshots are ignored here; only the prior run is the baseline.
  - The delta reaches the model at the action call alone. The thesis document never sees it.
  - A long↔short reversal is never read as unchanged.

- **Model**
  - None.

- **Output**
  - A position delta for every current holding.
  - The list of positions closed since the prior run.

---

## Step 5 — Load shared market context and score due forecasts

Loaded once per run and shared across every holding; the accuracy checks run here too, before any per-holding work, so this run's reviews see every check that has come due.

- **Data retrieved from local storage**
  - The house view: the latest Market Signal Report's Thesis, Investment Strategy, and Forward Outlook sections, plus up to three recent reports' date, thesis stance, and risk posture.
  - The fixed investor-profile preset.

- **Data retrieved from FRED**
  - The current `DGS10` and `DGS2` prints.
  - The trailing ~three-year `DGS10` history, one date-ranged request, kept as dated observations.
  - Commodity windows (~400 days each) for commodity-linked holdings: WTI oil, Henry Hub gas, and the monthly IMF copper, aluminum, nickel, iron ore, and uranium series.

- **Enriching run-level context** (each fail-soft to a typed gap counted on data health)
  - The gold quote from FMP, attached on a gold or precious-metals industry label only.
  - CFTC positioning: two weekly datasets normalized into one speculator-net view per contract; a row older than three weeks drops to a gap.
  - The CBOE daily put/call ratio, read by a bounded extraction of the statistics page.
  - Sector benchmark price histories from FMP, fetched per carried holding's sector and memoized.
  - The FINRA consolidated short-interest file, fetched once.

- **Logic**
  - Omit the house view when the latest report is older than one week, counted in whole ET session days on both sides, and record a gap.
  - Normalize rates into decimal form.
  - Industry overrides sector for commodity matching: uranium producers get the uranium print; coal producers get no proxy because no coal series exists.

- **What each input feeds**
  - House view → the thesis-document call at Step 6f, rendered as a market-level analysis.
  - Investor profile → the action call at Step 6f only.
  - `DGS10` → the valuation multiples at Step 6b; `DGS2` → the return hurdle at Step 6b.
  - Commodity prints → evidence for commodity-linked holdings.
  - CFTC → a commodity or macro fund's underlying-positioning read, mapped by fund identity keywords.
  - CBOE → a venue-level backdrop, never a per-name signal.
  - Sector benchmarks → the technology-event pre-flag at Step 6b.
  - FINRA → each stock's short-interest read at Step 6a.

- **Failure rule**
  - `DGS2` or `DGS10` still unavailable after the shared bounded retries → fail the run before any per-holding work.
  - Everything else degrades softly.

### Accuracy checks (engine-only, before the loop)

Engine-computed over the append-only episode store. No model. Opening new episodes happens after the loop (Step 7), since it needs this run's new prices.

- **Find due horizons**
  - An episode's horizon dates are its creation date plus three, twelve, and thirty-six calendar months.
  - A horizon is due when its date is on or before the run's ET session date and nothing has been written for it. One query over the store; nothing is ever re-scored.

- **Refresh prices**
  - The dated daily series is refreshed through the shared price-bar cache for every symbol with a due horizon: held, exited, and unselected carried holdings alike. A user's exit never stops measurement.

- **Write the check**
  - Read the close on the horizon date, or the last session at or before it within 5 sessions.
  - Bridge the episode's prices across any split since creation using its anchor close.
  - Per-check score = 100 × (1 − |expected − actual| ÷ actual), floored at 0, for the model's price and the engine's base value alike.
  - Write onto the episode: horizon, check date, the close, both expected values, both scores.
  - A horizon whose model price is null scores the engine leg alone; the model's accuracy score at that horizon counts no such check.

- **Pending versus unscorable**
  - A failed refresh leaves the horizon pending; it is due again next run. Never a run failure.
  - A served series with no close inside the window, or whose anchor bar it no longer carries, writes the horizon unscorable at once with its cause. It leaves the due set, counts in no score, and the symbol spends no further pull.

- **Derive the scores**
  - Per holding, per horizon, per arm: the mean of its per-check scores, 0–100. "No score yet" before the first check.
  - Each score carries the date of the last check that moved it and whether the prior analysis read it. Because the checks precede the loop, every check this run writes is new to this run's review.
  - The scores persist with the holding, render on its card, and reach the model in the self-review alone (Step 6e).

- **Model**
  - None.

- **Output**
  - One shared context packet.
  - The episode store updated with this run's checks, and each holding's current accuracy scores.

---

## Step 6 — Per-holding analysis loop

The following sequence runs once for every holding in the work list.
Each completed holding checkpoints as it lands, so a cancellation or crash resumes the unfinished holdings instead of restarting the run.
A single holding's model failure isolates into a failed card; the run continues.

### Work-list logic

- **Full run**
  - No cards selected.
  - Analyze every gradable holding.

- **Selective run** (analyzes strictly the selection)
  - **Work list**
    - The user-selected holdings and nothing else.
    - A selective request with no readable prior run runs the whole book.
  - **Carried holdings** (unselected, with a prior verdict)
    - Keep their prior thesis document, typed layer, engine arm, and action, vintage-stamped.
    - An over-age add-family action (past ~4 weeks, counted in whole ET session days) is demoted to hold and marked `rule-demoted`.
    - An over-age exit or hold stands as-is behind the stale-vintage badge.
    - Nothing else moves a carried action without fresh analysis.
  - **Not-analyzed holdings** (unselected, no prior verdict)
    - Left not analyzed; rendered as a selectable "run to grade" placeholder.
  - **Badges** (the quick check sweeps the carried tail to inform, never to re-analyze)
    - Attention flag — the hurdle newly reads fails, or spot's relation to the frozen twelve-month band changed.
    - Unknown family — a signal family could not be checked, so the sweep cannot vouch for the carried verdict.
    - Unexamined evidence event — new information since the holding's last full pass, detected by the quick check's own re-pulls (earnings, estimates, filings; a fund's info and weightings) against the holding's vintage date.
    - Side reversed — the carried long-side verdict now sits on a net-short position.
    - Stale vintage — the carried verdict is over-age.
    - Each is non-blocking; the user acts by selecting the holding or running a full analysis.

- **Uniform effort**
  - Every analyzed holding runs the full research loop every run. There is no lighter path.
  - On a continuity run the holding's prior analysis and prior thesis document ride every research prompt, dated, whatever their age.

- **Resume behavior**
  - Offered from a failed or cancelled run's tracker state while a checkpoint trail exists and its pinned pull is younger than 48 hours.
  - Reopens the interrupted run's id and pins its holdings pull, shared context, and model, prompt, and schema versions. No new pull.
  - Per-holding retrieval (option chains, FMP, EDGAR) still runs live for each resumed holding.
  - Refused with its reason when a version or roster changed, a newer run persisted in between, or the trail's evidence-floor or format stamp differs.
  - Each restored holding row carries its own telemetry, so the finished run's data health spans both processes and counts no call twice.
  - A new run always discards any standing trail once its own pull and context loads succeed.

### The model calls at a glance

| Call | Step | Mode | Returns | App check |
|---|---|---|---|---|
| Research gathering | 6c | thinking, tools, no grammar | tool calls, then a reply with none | none |
| Synthesis (write-up) | 6c | thinking, no tools, no grammar | prose write-up; a follow-up question or `none` | none |
| Distillation | 6d | non-thinking, no grammar | shorter prose | none |
| The analysis | 6d | thinking, no grammar | prose | none |
| Self-review | 6e | thinking, no grammar | prose | none |
| Thesis document | 6f | thinking, no grammar | prose | none |
| Typed appendix | 6f | non-thinking, grammar | conviction + three prices | each price finite and positive; reissue once |
| Action decision | 6f | thinking, grammar | rung + rationale | rung on the ladder; rationale non-blank |

- **Rules every call shares**
  - One message in two parts: data with glosses, then the task. A typed call closes on a placeholder-only return shape.
  - The deadline derives from the call's own reservations (context over a prefill floor, plus output over a decode floor on the non-streaming path), never a fixed backstop.
  - A transient failure (connection, daemon error, empty body, broken stream, schema parse, an off-domain appendix) re-attempts exactly once. A deadline trip, a length stop, a cancel, or anything unclassified fails on first occurrence. A second failure isolates the holding.
  - Every fired retry is evidence: a tracker row plus a data-health event.
  - One context size per model; context pressure is answered by compressing documents or picking a smaller shape, never by raising it.
  - Prompts put the holding-constant text first and keep the per-conversation text short (under ~1,024 tokens), so consecutive conversations on one holding reuse the server's prompt cache.
  - Sampling: thinking calls at temperature 1.0, top-p 0.95, top-k 20; non-thinking calls at 0.7, 0.8, 20. Greedy decoding is forbidden.

---

### Step 6a — Build the holding dossier

The app assembles the evidence packet the engine computes over. For a stock the listing guard runs first; the rest is pulled only if it clears.

#### Resolve stock identity (runs first)

- **Stock identity validation**
  - One FMP company-profile fetch, queried by the Schwab symbol as-is.
  - Cross-check against Schwab's identity: exchange first, then issuer name.
    - US primary exchange (NYSE, NASDAQ, AMEX) with a matching name → continue, subject to the unit guards.
    - An ADR, or a profile whose quote currency is not USD → not rated, unsupported units.
    - Any quarterly statement row not reported in USD → insufficient evidence before grading.
    - FMP definitively resolves no listing, or a non-US or OTC primary listing → not rated, unsupported listing.
    - US exchange but the issuer names share no significant token → insufficient evidence (a possible identity collision).
    - A failed profile fetch, or identity too sparse to compare → continue with a recorded degraded input. An outage can never mass-not-rate a book.
  - A guard-terminal stock routes straight to its verdict and spends no further pull.

#### Gather the evidence (skipped for a guard-terminal stock)

- **Stock data from FMP**
  - Quarterly income statement, balance sheet, and cash-flow statement.
  - Forward consensus estimates (revenue, EPS) for the coming fiscal years.
  - Dividends.
  - Live quote; the short undated price window; the deep dated price history (~1,600 days).
  - Symbol-scoped news headlines as research leads (history ≤ 1 year).

- **Stock data from elsewhere**
  - SEC XBRL company facts, merged fill-only behind FMP's statements (SEC fills absent lines).
  - The item-classified 8-K sweep: Item 4.01 auditor change and Item 4.02 restatement. An unresolved CIK or failed fetch types it `unknown`, never clear.
  - The FINRA short-interest lookup off the run-level file. A symbol absent from the file carries no read, a market fact.

- **Option chains from Schwab** (per optionable equity)
  - Volume, open interest, and implied volatility over the whole chain → put/call by volume and by open interest, plus skew (mean put IV − mean call IV). A rough activity proxy, never a grade input.
  - Held options on the same stock link by the OCC symbol decode into a typed **overlay**: direction, quantity, strike, expiry, delta, coverage ratio, classified covered call, protective put, collar, or other. Delta comes from a targeted per-strike fetch.
  - A row that does not decode to the holding's root never links. A fetch failure or malformed body → a typed options gap. An empty chain on an un-optioned name → no gap.
  - A standalone option with no held underlier gets no chain fetch.

- **Fund data from FMP**
  - The live quote, the short price window, and the deep price history, shared with the stock surface: they feed the fund's volatility, drawdown, momentum, and bands.
  - Dividends, for the fund's trailing-twelve-month distributions.
  - `etf/info`: expense ratio, AUM, NAV, asset class. Serves closed-end funds an empty body.
  - `profile`: the `isFund` flag and the description, the two legs of closed-end detection.
  - Sector and country weightings; a row is usable only when its percent is finite and within 0–100.
  - The sector P/E snapshot (one call per exchange per candidate session, walking back up to five weekdays until both NYSE and NASDAQ serve) and each sector's P/E history (per sector × exchange, memoized across funds).

- **Local data**
  - The position and its Step 4 delta.
  - On a continuity run: the prior position (action and rationale, conviction, the three expected prices with their horizon dates), the prior analysis, the prior thesis document, and the holding's accuracy scores, all loaded by identity.
  - The shared Step 5 context.

#### Fund routing

Decided at Step 6b from the metadata gathered above.

- US equity fund with usable weightings and ≥ 70% US exposure → priced-fund path.
- Bond or commodity fund → role-risk-only.
- Equity fund below the US-exposure guard → role-risk-only.
- Leveraged, inverse, or option-overlay vehicle → role-risk-only with a structural flag.
- Explicit allocation or multi-asset class → role-risk-only.
- Mutual fund without usable weightings → role-risk-only.
- Closed-end fund → a structure marker orthogonal to the class, requiring both the `isFund` flag and a closed-end description fragment. On the empty `etf/info` surface it routes role-risk-only labeled closed-end, never insufficient-evidence.

#### Output

- A complete stock or fund dossier.
- A stock leaves with its route; a fund leaves with its routing inputs.

---

### Step 6b — Calculate the financial picture

The deterministic engine runs the holding down one of three routes: priced stock, priced equity fund, or role-risk-only fund. Nothing here calls a model or the web.

- **Order of computation (priced stock)**
  - Statement basis → pre-profit overlay → sub-scores → scenario bands → letter grade → risk tier → return hurdle → the engine's action set and rung → the other reads → realized data for the review (continuity runs) → the evidence floor.
  - Through-line: sub-scores → letter; targets and tier → hurdle; hurdle, grade, forensic state, and overlay → the engine's action set.

#### Statement basis

- One basis per holding for every margin, growth, and multiple read.
- Preferred: FMP trailing twelve months, summing the four newest quarters (revenue and net income required on all four; gross profit all-or-gap). Quarters five through eight supply the prior-year revenue for growth.
- Fallback: the SEC annual basis, the latest 10-K full-year lines.
- Leverage reads the latest quarterly balance sheet, SEC equity as fallback.
- The basis in use is recorded, and the thesis-document prompt states it.

#### Pre-profit overlay (stocks)

Computed and persisted for every priced stock; only an eligible read binds.

- **Who enters** (either condition admits)
  - TTM operating income ≤ 0.
  - Or no positive forward-EPS consensus and TTM free cash flow < 0.
  - A satisfied condition admits even while the other condition's inputs are missing; a missing input never infers entry and is recorded as a gap.
  - Funds and role-risk-only holdings never enter.

- **Engine reads**
  - Liquid resources = cash + short-term investments.
  - TTM cash burn = max(0, −TTM free cash flow).
  - Runway months = 12 × liquid resources ÷ burn.
  - Capex intensity, year-over-year diluted-share change (split-adjusted), the latest two-quarter average gross margin and its change from the preceding two quarters.

- **Financing state**
  - `not_burning` at zero burn; `adequate` ≥ 24 months; `watch` 12–24; `constrained` < 12; `unscorable` when runway cannot be computed.

- **Derived states**
  - Economics deterioration: recent two-quarter gross margin non-positive and at least 5 points below the preceding average.
  - Material dilution: diluted shares up at least 15% year over year.
  - Severe deterioration: economics deterioration plus constrained runway or material dilution.
  - The execution read (guidance versus delivered results) has no deterministic producer; it types `unscorable` and enters no conjunction. The issuer's operating proof reaches the model through the research alone.

- **Consequences** (bind the engine's action set; annotate the model's)
  - Constrained runway removes the add family from the engine set.
  - Severe deterioration restricts the engine set to trim and sell all.
  - Never changes the letter. No single burn, margin, or dilution figure can force a sale.

#### Sub-scores and letter grade (stocks)

- **Sub-scores** (each a clamped linear map to 0–100, higher is better)
  - Quality: net margin and gross margin.
  - Valuation: inverted P/E, P/S, and P/B. A negative P/E scores low, never cheap.
  - Risk (higher = safer): realized volatility and debt/equity. Negative equity takes the floor.
  - Momentum: trailing return over the short price window. Context outside the letter.
  - A missing sub-score is imputed to 50; a letter resting on one carries a visible low-confidence marker.
  - The grade's fuller form lands with Trade Opportunities' shared grade slice: quality lifts gross profitability, the return-on-invested-capital spread over the cost of capital, and free-cash-flow conversion; every factor normalizes against sector-adjusted bands and the name's own history; the metric follows the sector (P/B-family valuation for financials, FFO economics for REITs, debt/equity dropped for banks). The weights and cutoffs recalibrate then.

- **Letter**
  - Composite = quality × 0.40 + valuation × 0.30 + risk × 0.30.
  - A ≥ 85, B ≥ 70, C ≥ 55, D ≥ 40, else F.
  - At least two of the three must be real, else the holding abstains.
  - The grade parameter version is stamped on every record; a bump names what it changed.

#### Scenario bands (priced stocks)

Bear, base, and bull prices at three months, twelve months, and three years. A target the arithmetic cannot finish as a finite number exits the holding as insufficient evidence.

- **Choose the driver**
  - Positive consensus forward EPS; else consensus forward revenue per share on the latest reported diluted count; else `no-admissible-driver` → the evidence floor.
  - The consensus is the next-twelve-months read: the two nearest coming fiscal-year rows blended by their overlap with the forward year.

- **Build the three driver cases**
  - Base from the consensus mid, bear and bull from the low and high.
  - A missing spread holds the driver flat and records it.
  - Each case is clamped to −25% / +35% implied growth; a clamp that collapses a published spread is recorded. A corroborated trough (two or more forward rows, trailing multiple above its window's rich end, trailing print depressed) releases the clamp, recorded.

- **Calculate the valuation multiple**
  - Over a ~three-year window of quarterly history, the spread between the driver yield and the contemporaneous `DGS10` is taken at the 75th, 50th, and 25th percentiles for bear, base, and bull.
  - Each re-anchors on today's rate: multiple = 1 ÷ (spread + `DGS10` today).
  - Needs at least 8 admissible quarters; fewer falls back to raw-multiple percentiles. An observation whose multiple exceeds today's by more than 3× is excluded. A near-zero denominator falls back to the raw percentile for that scenario.
  - A crossed band is sorted as a repair and recorded; scenario identity never comes from sorting.

- **Three-month band**
  - Base = spot × (1 + the twelve-month base price return ÷ 4), so the twelve-month band below is computed first.
  - Half-band = daily volatility × 2 × √63, clamped [3.5%, 26%], 8.7% when volatility cannot be computed.

- **Twelve-month band**
  - Price = driver × multiple per scenario.
  - A dispersion floor widens bear and bull to at least a half-spread of annualized volatility × 0.5, clamped 5–20%, never narrows.
  - Total return = (price + trailing-twelve-month dividends per share) ÷ spot − 1.

- **Three-year band** (declared extrapolation)
  - Each scenario's twelve-month driver compounded two further years at the growth the two coming fiscal-year rows imply, under the same clamp, times the same scenario multiple.
  - A single forward row holds growth flat, recorded. The dispersion floor scales by √3.

- **Where these land**
  - The bands with each horizon's method clause and provenance flags → the thesis-document and action prompts, the card, and the quick check's frozen twelve-month band.
  - The base value at each horizon → the episode store, as the engine's forecast.
  - The drivers, percentiles, spot, and dividend proxy persist as the quick-check basis.
  - The scenario-target parameter version is stamped; a bump names the horizons it moved.

#### Risk tier

- **Priced stock**
  - High if any fires: market cap < $2B, annualized volatility > 40%, max drawdown > 50%, debt/equity > 2.0 or negative equity, unprofitable.
  - Low if all hold: market cap > $10B, profitable, debt/equity in [0, 1.0), annualized volatility < 25%.
  - Else Medium. A missing input cannot fire a leg; wholesale missing inputs read Medium with a logged gap.

- **Priced equity fund**
  - High on volatility > 40% or drawdown > 50%; Low on volatility < 25%; else Medium.

- **Role-risk-only**
  - No tier.

#### Capital efficiency — the return hurdle

- **Hurdle rate** = `DGS2` + 3 points (Low), 5 points (Medium), 8 points (High).
- **Three-state result** on the twelve-month total returns
  - Bear ≥ hurdle → `clears`.
  - Else bull < hurdle → `fails`.
  - Else `indeterminate`.
- **Meaning**
  - Only `fails` is dead money, a weighed exit input. `indeterminate` tilts nothing.
  - New money uses a stricter point test: the base-case total return must clear before add is admitted.

#### The engine's action set and rung

- **Feasible set** (from per-holding engine inputs only)
  - Add and add aggressively enter only when the new-money test passes, the hurdle is not `fails`, the grade is not F, the hard forensic state is clear, and no overlay rule bars them.
  - Add aggressively additionally needs an A or B.
  - Severe deterioration restricts the set to trim and sell all.
  - A role-risk-only holding's set is sell all, trim, hold.
  - The set renders to the action call as one data line and is never a bar on the model.

- **The engine's own rung** (read in order)
  - Hard forensic state tripped → trim, or sell all with dead money or an F.
  - Else F and dead money → sell all; either alone → trim.
  - Else A or B, hurdle clears, and the new-money test passes → add. Add aggressively is never the rule's pick.
  - Else hold.
  - The pick is then walked toward hold, and past it toward trim where hold is barred, until it sits inside the feasible set.
  - Reaches the action call as a computed read, never a recommendation.

#### Other reads (evidence for the model; none changes the letter, none caps conviction)

- **Hard forensic state**
  - A restatement or auditor change in the item-classified 8-K sweep within a 365-day lookback.
  - An unresolved CIK or failed sweep reads `unknown`.
  - A fraud allegation found by research is prose the model weighs; it never trips the state.
  - Consequence: the add family leaves the engine set and the engine's rung reads the exit family; the model's rung is annotated, never overridden.

- **Narrative versus reality**
  - Multiple expansion paced against estimate revision since the prior run's stored comparator.
  - Hype when expansion outruns revisions by more than ~1.5×, above a 5% expansion floor, over at least 7 days.
  - Thin coverage falls back to operating results against the annualized price move.
  - A debut carries no read.

- **Implied expectations**
  - The growth or margin trajectory the current price already assumes, found by inverting the scenario math at spot per scenario multiple. A range, never one number.
  - Absent on a current-multiple carry and on the fund form.

- **Technology-event pre-flag**
  - Fires when the holding's sector-relative move since the prior run exceeds 2 × its interval-scaled realized volatility.
  - Adds the conditional research topic at Step 6c. Asserts nothing about the cause.
  - The benchmark must carry a close on both window sessions, else unevaluable.

- **Positioning context**
  - The Schwab options signal, the FINRA short-interest read, and for a commodity or macro fund the CFTC underlying positioning.
  - Insider and congressional activity, rating drift, and earnings surprises ride the dossier as evidence; the soft forensic flags (Altman Z < 1.8, Piotroski ≤ 3, net income > 1.3× operating cash flow, receivables or inventory growth > 1.5× revenue growth) are computed from financial scores and the statements.

#### The equity-fund path

- **Fund valuation** — what the fund's sector mix costs now versus its own history.
  - Composite earnings yield = Σ(weight ÷ sector P/E) ÷ Σ weight over sectors with a usable P/E, each sector P/E a blend of the NYSE and NASDAQ prints.
  - Covered weight must be ≥ 70% of the fund, else valuation is a gap. Uncovered weight is reported, never averaged in.
  - Valuation sub-score = the percentile of today's yield within the constant-current-mix history; at least 8 in-quarter samples, each with ≥ 70% coverage.

- **Fund sub-scores and grade**
  - Quality is a fixed neutral 50, never presented as fund quality; every priced fund carries the low-confidence marker.
  - Risk: volatility and drawdown; one missing leg imputes to 50; both missing abstains.
  - Momentum as for a stock.
  - Same weights and cutoffs as a stock.

- **Fund scenario bands** (the settled flat-driver form)
  - Driver = spot × composite earnings yield, held flat across scenarios; the spread comes from the multiple axis and the dispersion floor.
  - Multiples anchor on the composite yield's own history under the stock rules.
  - Twelve-month total return adds trailing-twelve-month distributions. Three-month as for a stock. Three-year carries the twelve-month prices flat.
  - Known conservative bias: no growth leg.

- **Fund metrics for the prompt**
  - Expense ratio, US share, composite coverage, exposure tilt.
  - NAV premium or discount = price ÷ NAV − 1 on the closed-end form only, a prompt line and a card line, never a score.

- **Fund hurdle and tier** — the stock hurdle and the fund tier rule above.

- **Role-risk-only readout** (no grade, bands, tier, or sub-scores)
  - Class label; the top five sector or country weights; expense ratio; observable risk as annualized volatility; the structural flag; the evidence-gap list; the closed-end price-vs-NAV read or its named gap.

#### Realized data for the self-review (continuity runs)

- The price now and its move since the prior run.
- Every stored engine value then and now: metrics, sub-scores, grade, bands, tier, hurdle state. A real move never reads as equality.
- For each of the prior position's expected prices: whether its horizon has passed, and if so the close on that date and its score, else the path so far.
- The holding's six accuracy scores, each with the date of the last check that moved it and whether the prior analysis read it.
- The checks written since the prior analysis was written, this run's own Step 5 checks included, newest by horizon date, up to 12.
- A parameter-boundary line naming what a grade or target stamp change moved for this holding, absent where it moved nothing.

- **Split bridge**
  - Each full pass stamps an anchor bar: the newest settled close strictly before the run's session. A later read divides the fresh close at that date by the stored one; the factor snaps to 1.0 inside a 10% deadband.
  - The prior expected prices, prior spot, episode prices, and prior consensus rows convert through the factor, each labeled bridged.
  - A verbatim document (the prior analysis, the prior thesis document) is never rewritten; a split-context line above it states the split's date, ratio, and factor.
  - An anchor missing from the fresh window excludes the comparison, typed; nothing runs cross-basis.
  - Such a pass carries the prior anchor forward unchanged and withholds its own fresh quick-check basis and band-relation stamp, so every value a later bridge reads shares the carried anchor's basis and nothing converts twice when the anchor resolves. The verdict's displayed bands stay fresh.

#### Evidence floor

Gates fire inline as the values above are computed; the first failure short-circuits the holding.

- **Stock requires**
  - A usable current price: finite and strictly positive.
  - No resolved identity conflict.
  - At least two real sub-scores.
  - An admissible target driver.
  - USD-denominated statements where statements are served.
  - Statement age alone does not abstain.

- **Priced fund requires**
  - A usable quote or NAV (an unusable quote falls to a usable NAV).
  - `etf/info` and an expense ratio.
  - Usable sector and country weightings with ≥ 70% valuation coverage.
  - At least one risk leg.
  - At least eight constant-mix history samples.

- **Floor failure**
  - Mark `insufficient-evidence` with named reasons.
  - Checkpoint the holding as completed; skip research, consolidation, review, interpretation, and the action call.
  - Retain the prior analysis, the prior thesis document, and any attention flag.
  - Persist the overlay record.
  - No action this run, no new episode. Pending checks still run.

- **Non-floor gaps**
  - Missing enriching data lowers confidence and records a degraded-input flag.
  - Web coverage never enters the floor.

- **Output**
  - The engine arm: sub-scores and letter, the three bands with method and provenance, tier, hurdle read, hard forensic state with the soft forensic flags, the feasible set and the engine's rung.
  - The evidence the prompts render: computed metrics, momentum, the overlay, the narrative read, implied expectations, positioning, the pre-flag, the fund readout.
  - The realized data for the review, the band-relation stamp and anchor bar for later passes, and the quick-check basis.

---

### Step 6c — Research the holding

The reasoner works a deterministic agenda over the web tool, one topic at a time, each in its own conversation. The orchestrator owns every request.

#### The agenda

- **Stock topics**
  - Competitive and business position.
  - Recent results and estimate revisions.
  - Catalysts and risks.
  - Management quality and capital allocation.
  - Market narrative and sentiment, as a sourced qualitative read.
  - Forward opportunity and thematic fit.
  - Conditional: a technology-event impact assessment, only when the Step 6b pre-flag fired; decided when the agenda is assembled, never mid-loop.
  - Conditional: a pre-profit execution and financing topic for an overlay-eligible stock, over the operating proof the issuer reports (production, deliveries, bookings, guidance versus actuals, unit economics, margins, cash needs, financing). The write-up reports what it finds with dates and sources; the engine computes nothing from it.

- **Fund topics**
  - Mandate, strategy, and manager changes.
  - Expense and structure versus its category.
  - The exposure it actually supplies, and what direct or cheaper vehicles supply the same.
  - For a closed-end fund: the discount and distribution coverage.

- **After the topics**
  - A disconfirming pass searches for evidence against the run's write-ups and writes its own write-up, once per holding, asking no follow-up.

#### How a pass runs

- **Scheduling**
  - A topic's root pass plus at most two follow-ups: three passes per topic.
  - Every eligible root runs before any follow-up; then follow-ups in topic-priority order.
  - A follow-up is the model's own question, spent only if the orchestrator's budget allows.

- **Gathering conversation** (thinking, tools, no grammar)
  - The model asks for `web_search` and `web_fetch` calls; the orchestrator executes them and returns the results as data.
  - At most 8 replies per pass, counted down by a short app message before each request. Pages fetched on the last reply are kept.
  - At most 8 tool calls per reply; a larger batch has its head executed, its tail recorded as partial coverage, and the pass moves to synthesis.
  - The whole growing conversation plus the tool schema is sized against the shared input guard before every request and before each retained result; an overflow ends gathering as a recorded degradation.
  - Search and page metadata (titles, snippets, dates) are capped so untrusted text cannot consume the packet.
  - A pass ends on a reply with no tool call or at a bound.

- **Synthesis conversation** (thinking, no tools, no grammar)
  - A fresh conversation over the gathered pages writes the write-up. A tool-using turn and the write-up never share a request.
  - A pass that retrieved no page spends no synthesis; the write-up so far stands.

- **Per-holding budget** (binds first)
  - 40 fetch attempts and a wall-clock cap per holding, spent in root-then-follow-up and topic-priority order, polled between requests, never mid-request.
  - A failed live attempt spends like a served one; a cache hit or remembered failure spends nothing; a retry spends another.
  - When the budget drains, the lowest-priority remaining topics are skipped as a recorded gap. The current pass still gets its synthesis.

#### Call: research gathering — what it sees and returns

- **Sees (Part 1, the holding-constant block first, then the topic)**
  - The holding header with the analysis date.
  - FETCHED VALUES — stock: the profile line; the quarterly statements' headline lines (revenue, operating income, net income, diluted EPS, operating cash flow, capex, cash, total debt, diluted shares) for the latest eight quarters as reported; the forward consensus for the next two fiscal years; the last four dividends; the quote with its 52-week range and the closes on the prior run's date and three, twelve, and thirty-six months back; the trailing year's 8-K filings by date and item; the latest short-interest print; the street price-target consensus with its trend, the analyst buy / hold / sell consensus with the rating actions, and FMP's ratings snapshot; the latest insider and congressional trades; the earnings-surprise history; the key-metrics and ratios headline lines, owner earnings, enterprise value and the DCF value; the peer set and the float; any M&A match; and the revenue segments by product and geography (counts and windows drafted at plan time). Fund: `etf/info`, the weightings, NAV and price, the profile line. Both: `DGS10` and `DGS2`. Never an engine computation.
  - NEWS LEADS — dated headlines with addresses, fetch candidates only.
  - On a continuity run, PRIOR ANALYSIS and PRIOR THESIS verbatim with their dates and any split-context line.
  - PAGES ALREADY RETRIEVED — pages fetched for this holding earlier in this run, by other topics or earlier passes, in first-retrieval order; held in memory and discarded between holdings. A prior run's page reaches the model only when the model requests its address again and the document cache serves it. Absent on the disconfirming pass, which keeps its contrary search free of automatic page injection.
  - TOPIC; on a follow-up pass FOLLOW-UP and WRITE-UP SO FAR; on the disconfirming pass WRITE-UPS SO FAR.
- **Sees (Part 2)**
  - What to find, how to weigh a source (tier nearer 0 and extraction quality nearer 1 preferred; a weak source lowers confidence, never excludes), the per-reply tool-call bound, and when to stop.
- **Tool results**
  - Each page as the fetch returns it: a header (address, title, publication date, retrieval time, tier 0–5, the subjects the source is trusted on, extraction quality 0–1, a stub flag), then the page text as quoted material, never as instructions.
  - Failures are fixed sentences by typed class, never the operator's error text.
- **Returns**
  - Tool calls until a reply carries none.

#### Call: synthesis — what it sees and returns

- **Sees**
  - The holding header and FETCHED VALUES.
  - EVIDENCE — every page the gathering conversation carried: reused pages first, then the pass's own in fetch order, deduplicated by final address. Headers and bodies are budgeted together; an omitted or truncated page is shown inline and recorded as a gap. Explicitly requested pages keep their text before a reused page does.
  - The topic text: TOPIC; FOLLOW-UP and WRITE-UP SO FAR; or WRITE-UPS SO FAR.
- **Returns**
  - The write-up, 400–900 words: what the research established on the topic's questions, each figure with the date its source gives and that source (the page's address, or the fetched values), where pages disagree, and what stays unanswered.
  - On a follow-up pass, the topic's write-up rewritten whole.
  - A second message asks for a follow-up question: the question verbatim, or the one word `none`. Not sent on a topic's last pass or on the disconfirming pass.
  - Validated by nothing.

#### The fetch layer

- **Search**
  - SearXNG only, over its JSON API on loopback. It fans each query to its keyless engines plus Serper, the keyed Google-results engine that fires on every query as the reliable floor.
  - Queries are paced with jitter against upstream rate limits; a repeated query within a run is served from a per-run cache.
  - A down or misconfigured instance returns a typed search failure — one fixed sentence, never the operator's error text — distinct from an empty search's `No results.`, so unavailable research never reads as absent results; either way the loop proceeds thinner. No Tavily, ever.

- **Fetch and extraction**
  - A plain HTTP GET with browser-like headers and a timeout, then a Rust readability extraction to the article body. SEC hosts get the app's declared identity.
  - Paywalled or JavaScript-heavy pages return thin text; a selective render tier escalates pages the extraction telemetry flags as thin, reusing the app's embedded webview.
  - Optional Connected Sources attach a stored login session for a domain; never part of the gate.

- **Safety**
  - Only `http` and `https` to public hosts; private, loopback, and reserved ranges are blocked. Redirects are capped and re-validated. Responses are bounded by size and type.
  - Page text is data, not instructions.

- **Source quality informs, never gates**
  - Every domain carries a tier 0–5 (0 primary filings and regulators; 1 licensed data providers; 2 high-trust reporting; 3 specialist industry sources; 4 opinion; 5 sentiment only). A low tier weights down.
  - The explicit `deny` list (SEO mills, AI-generated quote pages, press-release spam) is the one categorical exclusion.
  - Syndicated reprints of one wire count as one source.

- **Reuse and memory**
  - A shared cross-run document cache serves repeat fetches inside a ~4-week window, each page carrying its original retrieval time.
  - A holding-scoped in-memory inventory feeds the pages-already-retrieved block and is discarded between holdings.
  - Failed-fetch memory spans the run: a 401 or 403 puts the host in a five-minute cooldown; a 408, 429, or 5xx, a timeout, or a reset gets one retry after one second; other failures are remembered and not retried.

- **Earnings-release recovery**
  - When an issuer's own site blocks the fetch of an identifiable earnings release, the loop may recover the matching 8-K Item 2.02 exhibit from EDGAR, within ten extra attempts under the holding's budget, cited under the SEC address.

#### Failure and output

- Research is fail-soft: a failed search, a timed-out fetch, a drained budget, or an unreachable SearXNG thins the evidence and never fails the run.
- Output: one write-up per worked topic plus the disconfirming pass's, flowing whole to consolidation.
- Every page shown enters the page roster, persisted on the audit record.
- Every degradation persists as a gap on the holding's audit and is counted into the run's data health.

---

### Step 6d — Consolidate the research

The write-ups become one analysis, distilled first only when they will not fit.

- **Budget check**
  - The orchestrator sizes the analysis prompt (the write-ups, the fetched values, and on a continuity run the prior analysis) against the call's input budget.
  - Within budget: the write-ups go in as written.
  - Over: the merged write-ups are distilled into one shorter document.
  - Where the merged write-ups exceed one distillation call's budget: each write-up is distilled first and the merge of those outputs is distilled again.
  - The prior analysis is never distilled.
  - The shape is chosen deterministically from size and logged to the audit with its call count.

#### Call: distillation — what it sees and returns

- **Sees**
  - The holding header, then the write-ups to distill under their topic headings.
  - The task: a shorter document that keeps every dated figure with its source, every disagreement, and every open question.
- **Returns**
  - Prose at most half its input and at most 1,200 words. Consolidation, not new reasoning.
- **Sizing at issue**
  - The rendered prompt is measured before any request: within the fast tier's budget it issues there; over it, on the resident reasoner; over the widest budget it takes the per-write-up shape, or is refused as a hard failure where no smaller shape remains.
  - A length stop at the normal 12,288-token output reservation gets one re-attempt at 32,768.

#### Call: the analysis — what it sees and returns

- **Sees (in page order)**
  - The holding header with the analysis date.
  - FETCHED VALUES.
  - On a continuity run, PRIOR ANALYSIS verbatim with its date and any split-context line.
  - WRITE-UPS — this run's write-ups or their distillate, each under its topic heading, the disconfirming pass's last.
- **Returns**
  - The analysis, 900–1,800 words: what this run's research established, each dated figure with its source, where sources disagree, what stays unanswered, and on a continuity run what the prior analysis said that this run confirms, revises, or leaves untouched.
  - A topic with no write-up this run keeps what the prior analysis says about it.
  - Validated by nothing. The only research artifact the next run reads; the write-ups persist on the audit as written.

---

### Step 6e — Self-review (continuity runs only)

Before this run's thesis document is written, the reasoner reviews its prior position against what has happened. A debut holding skips the step.

#### Call: the review — what it sees and returns

- **Sees (in page order)**
  - The holding header with the analysis date and FETCHED VALUES.
  - PRIOR POSITION — the prior run's date and the price then, its action and rationale, its conviction, and its three expected prices each with its horizon date.
  - PRIOR THESIS — the prior thesis document verbatim, with any split-context line.
  - ANALYSIS — this run's analysis.
  - REALIZED — the engine's realized data: the price now and its move; for each prior expected price whether its horizon has passed and, if so, the close and its score, else the path so far; the six accuracy scores each with its last-moved date and whether the prior analysis read it; the checks written since the prior analysis was written, this run's Step 5 checks included, newest by horizon date up to 12, under a heading stating every line is new, or one fixed sentence when none landed naming what the prior analysis read; the engine's own values then and now; any parameter-boundary line.
  - A role-risk-only holding sees the prior action and rationale, the fund's realized price, NAV, exposure, and expense reads then and now, and no accuracy scores.
- **Returns**
  - The review, 400–900 words: each expected price against what happened; each falsifier and trigger the prior document named, tripped or not by the numbers; whether the thesis survives; where the prior read was right or wrong and why; what to revise; and what should change in how this holding is analyzed.
  - Persisted on the audit record. Feeds the thesis-document call. Nothing app-side reads it.

---

### Step 6f — Thesis document, typed appendix, action

Three model calls. The first two share one conversation and author the intrinsic verdict's model arm; the third decides the action.

#### Call: the thesis document — what it sees and returns

- **Sees (Part 1, in page order)**
  - The holding header with the analysis date and FETCHED VALUES.
  - COMPUTED — the engine's metrics and the statement basis in use; the grade and sub-scores with each axis's polarity and any imputed-score disclosure; the bear, base, and bull bands at the three horizons with the twelve-month method and the provenance flags, so a low-signal band is weighed, not obeyed; the risk tier; the hurdle read; the hard forensic read with the rule it matched stated as fact; the soft forensic flags as typed evidence; the narrative-versus-reality read; the implied-expectations range beside the bands; the short-interest read; the options signal with its signed skew; the option overlay; the overlay's financing, economics, and dilution legs where the stock carries it; and the Step 5 context where it applies (commodity prints, the CBOE backdrop, a fund's positioning row, a fired pre-flag).
  - MARKET ANALYSIS — the house view, never named by product.
  - ANALYSIS — this run's analysis.
  - On a continuity run, REVIEW then PRIOR THESIS verbatim with any split-context line.
  - A fund sees its class and structure lines, exposure tilt, the price-vs-NAV line, positioning, its risk profile, and its evidence gaps in place of the equity reads.
- **Deliberately absent**
  - The investor profile. The position's economics. Any accuracy section. Raw statements, filings, or price series.
- **Returns**
  - The thesis document, 900–1,800 words: the thesis; the key drivers; the bear, base, and bull scenarios each with the conditions that produce it and the model's probability; the falsifiers and triggers in words, each with a concrete measure, level, and period, a trigger stating the direction of the position change; the expected share price at each horizon and the conviction, argued in the text; and a summary paragraph covering the financial read, why those prices and that conviction, and on a continuity run what changed and how the prior read held up.
  - A role-risk-only document covers the role, the risks, the triggers for trimming or selling, and the summary, with no prices and no conviction.
  - Validated by nothing. Persisted on the verdict.

#### Call: the typed appendix — what it sees and returns

- The same conversation's second message, thinking off, under a grammar: a transcription, not a judgment.
- **Sees**
  - A request for the conviction and the expected price at each horizon as the document states them — null where it states none — closing on a placeholder-only return shape.
- **Returns**
  - `conviction` (high, medium, low), `expected_price_3m`, `expected_price_12m`, `expected_price_3y`, each nullable.
  - The app checks type only: each present price finite and strictly positive, a present conviction in its enum, null accepted everywhere. An off-domain object is rejected whole and the message reissued once; a second failure isolates the holding.
  - Nothing else about the model arm is checked: not the document against the appendix, not the prices against the bands.
  - A null is acted on by presence: a null price opens no model leg at that horizon (Step 7), a null conviction renders as none, and an appendix with every field null opens no episode.
  - Not asked on a role-risk-only holding.

#### Call: the action decision — what it sees and returns

- **Sees (Part 1)**
  - The holding header and POSITION — shares held, cost basis, market value, unrealized gain or loss, and the position change since the last pull. The one model-facing packet that sees economics. No FETCHED VALUES block.
  - Priced: VERDICT — the conviction, the three expected prices, and the thesis document verbatim; COMPUTED — the engine's own rung as a computed read, its grade, its bands beside the model's prices, the three tested twelve-month total returns and the hurdle rate as numbers with no state word, the hard forensic state, the overlay's financing legs, the option overlay, and the commodity prints where they apply.
  - Role-risk-only: the class, role read, exposure tilt, risk profile, price-vs-NAV line, evidence gaps, and the thesis document verbatim.
  - On a continuity run, PRIOR ACTION — the prior rung and rationale; a model-chosen prior anchors the firmness clause, a rule-demoted one anchors nothing.
  - SUPPORTED ACTIONS — the engine's feasible set as one data line, stated as complete, the engine's pick withheld.
  - The investor profile without its tax row.
- **Deliberately absent**
  - The house view, the research, the computed metrics, the accuracy record.
  - Every book-level value: cash, weights, concentration, other holdings. Tunnel vision is enforced by what is not in the packet.
- **Sees (Part 2)**
  - The rung with its weighing clause; the profile tie-break; on a priced holding the sunk-cost clause (the returns weighed by value, a position the forward read has written off leaned toward realizing); with a prior action the firmness clause; then the one-sentence rationale; and the placeholder return shape.
- **Returns**
  - One rung from the ladder and a one-line rationale, persisted as `action` and `action_rationale`.
  - A blank rationale fails the holding, isolated like any other failure.
  - No weight, share count, or dollar figure. Sizing is the planner's.
  - A rung outside the engine set persists exactly as authored, with the departure stamped on the audit. A tripped hard forensic state is one such annotation, never an override.
  - Under a tax-aware profile the app appends a fixed tax-caveat sentence on a trim or sell-all with an unrealized gain or loss. The model's sentence is never edited.

- **Output of Step 6f**
  - The intrinsic verdict with both arms.
  - The holding's action, rationale, source, and annotations.

---

### Step 6g — Checkpoint

- **Data retrieved**
  - No new data.

- **What is persisted**
  - The verdict: the engine arm app-stamped directly, never echoed through the model; the model arm exactly as authored, type-checked only.
  - The action with its rationale, `action_source` (model-chosen or rule-demoted), and annotations.
  - The audit record: the write-ups as written, the distillation shape and call count, the analysis, the review, the thesis document, the page roster, the research gaps, the engine's computed reads and band methodology, the statement basis, the model ids, the prompt stamp, and the holding's call telemetry.
  - Two stamps for later passes: spot's relation to the twelve-month band at authoring (inside, below, above), and the anchor bar for the split bridge. A pass that could not verify its anchor withholds the band-relation stamp and the quick-check basis and carries the prior anchor (Step 6b, split bridge).

- **Rules**
  - No channel carries a model value into the engine, and no validator reads the documents.
  - A successful full pass clears any persisted quick-check attention flag for the holding.

- **Output**
  - A completed per-holding checkpoint row, so a resumed run skips the holding.

---

## Step 7 — Roll up the run, open episodes, save

Every action is final by now; this step decides none. It builds the book-level summary, opens this run's episodes, and persists the run.

### Roll-up

- **Calculations**
  - Verdict counts by disposition (priced, role-risk-only, not-rated, insufficient-evidence, failed).
  - The largest single-position weight and the cash weight, descriptive only.
  - Positions closed since the prior run, acknowledged rather than dropped.
  - The run-level data-health read: how targets were sourced (rate-anchored, raw-percentile, current-multiple carry), dispersion-floor engagements, deep-history failures, a run-wide rate-history gap, research-degraded holdings and total research gaps, every physical model-call attempt with its counters, fired retries, the peak prompt fill, and context pressure (a call at or past 90% of its context, or a prompt count too small for the characters sent).
  - An attention state on infrastructure degradation: a deep-history failure, a multiple-carry target, a rate-history gap, a context-pressured call, a length-stopped generation. Research gaps are counted, never flagged.

### Open episodes

Engine-computed over the append-only episode store. No model. The checks on existing episodes ran at Step 5.

- **Open episodes**
  - For every priced holding analyzed this run whose appendix stated at least one price, with no episode or whose latest episode is a month or more old.
  - An episode records: symbol, creation date, that day's spot, the anchor close with its bar date, the model's expected price at three, twelve, and thirty-six months — a horizon the appendix left null recorded null — and the engine's base value at the same horizons.
  - Role-risk-only holdings and abstentions open none. A holding re-priced between the monthly points is not recorded, so the scored forecast is the monthly snapshot.
  - An episode is never updated or deleted; the checks are written onto it once each. The store is the log.

- **Three disciplines**
  - The engine keeps score and never plays; nothing the model writes alters a check.
  - The record scores the forecast, not the book; held and exited names score alike.
  - Nothing is derived beyond the scores: no cohorts, no calibration proposals, no per-decision verdict. The learning rides the review's own answer to what should change.

### Save

- **Data stored** (one row per successful run)
  - The normalized holdings snapshot the run ran against.
  - Every verdict with both arms, every action with its rationale, source, and annotations, the vintage of each carried holding.
  - Every priced stock's overlay record.
  - The roll-up and the data-health aggregate.
  - Every holding's audit record, and the run's failed-holdings list with causes.
  - Model, prompt, grade, target, overlay, and evidence-floor versions.
  - The episodes opened, the checks and unscorable states written at Step 5, and each holding's accuracy scores.

- **Rules**
  - The write validates before it lands: a record that would not read back is refused, naming the holding.
  - A run with every attempted holding failed persists no row; the prior run stays latest.
  - Keep the newest 30 runs. The episode store persists outside that window. The checkpoint trail is cleared by the successful persist.
  - Nothing is written to vector memory. A holding's continuity is the deterministic load of its own documents.

- **Output**
  - A durable run and audit record.
  - An updated episode store.

---

## Step 8 — Display the result

Display is a pure read of the persisted run. No model runs; the backend shapes nothing.

- **Data retrieved**
  - The latest run, selected by insertion order, never by timestamp.
  - An older run by id for the read-only history view.
  - The run listing for the sidebar.
  - The latest standalone holdings snapshot and the latest quick-check state.
  - An unreadable stored run costs only its own surface: it lists as unreadable, cannot open, and never becomes a baseline.

- **Per-holding card — priced**
  - The thesis document as the body. No separately authored summary exists.
  - The typed strip beside it: the action with its rationale, the conviction, the three expected prices with their horizon dates, and the six accuracy scores ("no score yet" where none).
  - One compact engine line beneath: the grade with its low-confidence marker, the risk tier, the three bands, the hurdle state, each labeled a computed read.
  - The grade and the thesis document's forward read are presented as a pairing, so a C-grade with a constructive thesis reads as intentional.

- **Per-holding card — role-risk-only**
  - The thesis document with the role read, the class label, exposure tilt, observable risk, expense drag, structural flag, and evidence gaps beside the action.
  - No prices, no conviction, no empty priced placeholders.

- **Card badges** (non-blocking)
  - Amber attention flag, from the quick check's two monitors; cleared by the holding's next successful full pass.
  - Quiet badges: evidence event, degraded sweep (naming the families that could not be checked), stale vintage, add-demoted-to-hold.
  - The quick-check badges overlay only the latest live view whose swept run matches the rendered run.
  - Side reversed, amber, read from the stored verdict.
  - Analysis failed: the carried prior verdict vintage-stamped beside the badge, or an empty failed card naming the cause.
  - A held position with no verdict renders as a "run to grade" placeholder.
  - Every card carries a selection control; after a selective run each carried card carries its vintage stamp.

- **Roll-up display**
  - A key-figures strip: account value, positions, disposition counts, cash weight, top-position weight, failed count.
  - The roll-up card: the overview line, the data-health readout with its attention tag, and the positions closed since the last run.
  - Not-rated and insufficient-evidence positions show their reason on their own cards.

- **Current-holdings view**
  - The latest Pull holdings snapshot: symbol, quantity, derived per-unit price, market value, cost basis, percentage gain, pulled-at stamp; column heads sort in place.
  - Before any run it is the page body. Once runs exist it appears only when fresher than the last run, above the cards, never replacing them.
  - Churn is tagged by symbol presence only: new, not in last analysis; no longer held.

- **Sorting**
  - Four position-level keys over engine-computed totals: overall value, dollar gain, percentage gain, total cash invested. Never the grade or conviction.
  - Default overall value descending, ticker tie-break, undefined values last. Option and fixed-income rows withhold cost-derived figures.
  - Remembered in webview local storage. Shown only with more than one card.

- **Read-only past-run view**
  - Any older sidebar row renders the same page with every trigger locked, no selection controls, and a vintage banner with Back to latest.
  - The current-holdings comparison never renders over a past run.
  - Quick-check badges (attention, evidence event, degraded sweep) render only on the latest live view whose swept run matches the rendered run; a past run shows none. Badges read from the stored verdict (side reversed, stale vintage, add-demoted) still render, so today's state is never presented as part of an old result.
  - The sidebar lists the last 30 runs with holdings count and `graded N`; an unreadable row shows no counts and cannot open.

- **Model**
  - None.

---

# Quick check

`Load last run → Refresh prices and rates → Re-derive the hurdle → Compare spot with the frozen band → Detect evidence events → Save state`

- **Purpose**
  - Keep the engine's reads live between full runs, which can be hours apart.
  - Warn without deciding. It reads the engine arm alone.
  - The thesis document, conviction, and expected prices are never evaluated between runs; the falsifiers and triggers they name are tested by the self-review at the next full pass. The accepted cost: a thesis broken on a measure only its document names waits for the next pass.

- **Gate**
  - Presence of the local-model configuration, the Schwab connection, and the FMP and FRED credentials. No daemon probe: it makes no model call and runs even when the daemon is down.
  - No web research, so no pre-run notice. No Schwab call.
  - Holds the single global run slot and streams to the run tracker like any job.

- **Data retrieved from local storage**
  - The last run's holdings snapshot and verdicts.
  - Each priced holding's quick-check basis: the stored drivers, spread percentiles, spot, and dividend proxy.
  - The frozen twelve-month band and the authoring-time band-relation stamp.

- **Data refreshed**
  - Prices for every holding: the live quote plus the dated daily closes (two FMP calls; the sweep never reads the shared price-bar cache).
  - `DGS2` and `DGS10` from FRED. A failed pull fails soft to the freshest cached print, a prior quick check's first, else the last run's, eligible only within ~1 week; none eligible reads the rate-dependent families `unknown`.
  - Per stock: the EDGAR filing check (CIK-gated), an analyst-estimates snapshot, and an earnings re-pull; after a new filing, the income statement, balance sheet, and dividends again. No cash-flow re-pull, no news call, no FINRA call.
  - Per fund, unconditionally: `etf/info` and both weighting sets.

- **Calculations**
  - The two engine monitors, priced verdicts only, every stored price first crossing the split bridge:
    - Re-anchor the stored multiples closed-form on the fresh `DGS10` against the stored percentiles and drivers, and re-derive the hurdle with the fresh price and `DGS2`. The dividend leg refreshes only on a new filing; a confirmed non-payer re-reads zero, a failed pull keeps the stored leg.
    - Compare spot's relation to the frozen twelve-month band (inside, below, above) against the relation stamped at authoring. The three-month band is not monitored; the three-year band is an extrapolation.
  - Evidence events, for every holding on either verdict branch: a role-risk-only fund still gets its mandate and expense checks, and an equity fund is checked for the US-exposure guard crossing in both directions.

- **Attention flag** (amber, actionable)
  - The hurdle newly reads `fails`, named against the bull-case total return.
  - Spot's relation to the band changed since authoring: left it, re-entered it, or crossed to the other side. A band authored with spot already outside it does not re-flag while that stands.

- **Evidence events** (quiet badge, never amber)
  - A new earnings actual.
  - A new 8-K, 10-Q, or 10-K.
  - A large revision: same-period consensus EPS moving more than ~5%, or ~$0.10 per share where the prior consensus is negative or below $0.10.
  - A fund's expense ratio or mandate fields changing, the US-exposure guard crossing in either direction, or a top sector weight moving ~10 points or more.
  - No news leg: a headline's materiality cannot be classified without a model.
  - The "since the last full pass" boundary is the holding's vintage in ET session days, the boundary day inclusive.

- **Per-family result**
  - `fresh_clear` — successfully checked, nothing fired.
  - `flagged` — a monitor fired.
  - `unknown` — the retrieval failed and nothing could vouch; an unresolved CIK reads the filing family `unknown`. Never a silent clear.

- **State updates**
  - Persists its whole result only to its own single-row store, never a run row, so it cannot become the next run's baseline.
  - The flag and which monitor raised it persist until the holding's next successful full pass.
  - A selective run's carried-tail sweep reuses the parent run's pinned instant for every date.

- **Cannot**
  - Rewrite a grade, a band, a thesis document, a conviction, or an action.
  - Perform web research or call a model.

- **Output**
  - Attention flags, evidence-event and degraded-sweep badges, refreshed prints.

---

# Pull holdings

`Check Schwab → Fetch positions → Normalize → Save snapshot → Display`

- **Purpose**
  - View current holdings without running analysis.
  - Requires only the Schwab connection, not the local-model configuration.
  - Holds the run slot like any job.

- **Logic**
  - The same fetch and normalization as Step 2, including the non-finite guard.
  - Persists a standalone snapshot to its own single-row store, never read by the analysis job.
  - The frontend compares symbol presence with the latest run for display tags; the backend compares nothing.

- **Does not**
  - Analyze holdings, change a verdict, trigger the quick check, or replace the next run's diff baseline.

- **Output**
  - The current-holdings view.

---

# The most important safety rules

- The engine computes every financial number in the baseline arm; nothing the model writes alters or binds an engine value.
- The model's arm is checked on type alone: a price finite and positive, a rung on the ladder. Never against the engine, never on content.
- Both arms are scored identically by the accuracy pass, and the engine keeps score without playing.
- Engine evidence annotates the model's choices, never bars them.
- Missing floor-bearing data causes abstention, not a guessed grade.
- A role-risk-only verdict carries no fabricated priced number.
- A directional verdict is only ever authored for a long position.
- The investor profile and the position's economics reach the action call alone; the thesis document never sees them.
- The action call sees one holding at a time; no book-level value exists in its packet.
- The quick check warns but never rewrites; a failed retrieval becomes `unknown`, never clean.
- Selective runs cannot strengthen stale actions without fresh analysis.
- Actions are rung-only; sizing belongs to the portfolio planner.
- The job writes nothing to vector memory; a holding's memory is its own documents.
- The job never places an order.
