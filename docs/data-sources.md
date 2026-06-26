# Data Sources

This file lists the external data and model providers the application depends on. Credential configuration for these providers is covered in [configuration.md](configuration.md).

## Market and Financial Data

The application accesses market and financial data by calling provider REST APIs directly from the Rust backend (`reqwest`/`serde`). **Financial Modeling Prep** is the primary financial-data source, supplying equity-market data — indices, volatility, gold and silver, sector performance, multi-horizon index returns, market movers, the earnings calendar, sector and industry valuation (P/E) plus finer-grained industry rotation, and the equity-risk-premium. **FRED** and **BLS** supply macroeconomic and labor data through their public APIs; FRED additionally provides the US dollar index, commodity prices (oil, natural gas), Treasury yields, credit and yield-curve spreads, and financial-conditions indices.

The gated REST adapters (FMP, FRED, BLS, Tavily) share a bounded retry-with-backoff for transient HTTP-status/transport failures (HTTP 429, 5xx, dropped connections; `Retry-After`-aware). GDELT is deliberately excluded — its escalating per-IP lockout makes a retry harmful, so it keeps its single-shot fail-soft.

Where retries don't recover, the Step-3 baseline scan degrades rather than aborting: an unresolved series or release (a rejected key, a sustained outage, or a malformed / empty response) is recorded as a gap in a missing-data manifest instead of failing the whole scan, and a central coverage floor then decides whether what resolved is sufficient to generate the report (see [report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data)).

### Financial Modeling Prep
Docs - https://site.financialmodelingprep.com/developer/docs

Financial Modeling Prep is the primary financial-data source for the application.

Responsibilities:
- market prices
- index data (Dow, S&P 500, Nasdaq, Russell 2000)
- market volatility (VIX)
- precious metals (gold, silver)
- sector performance
- historical end-of-day prices (free tier)
- multi-horizon index performance (weekly / MTD / YTD / 52-week range)
- market movers (biggest gainers / losers / most-active)
- earnings calendar (free tier, ~1-month window)
- sector valuation (per-sector aggregate P/E, by exchange — NASDAQ + NYSE)
- finer industry rotation (per-industry average move + aggregate P/E, by exchange)
- market risk premium (US equity-risk-premium)
- historical sector / industry P/E *and* performance (paid tier; trailing-window P/E percentile + cumulative-return trend vs own history — planned report enrichment, see below)
- IPO calendar + M&A deal flow (paid tier; issuance / deal-making froth as a risk-appetite read — planned report enrichment, see below)
- FMP Articles — in-house, ticker-tagged market commentary (free tier; feeds the Step-7 news funnel — see News and Research below)
- economic calendar — analyst consensus + realized surprise + Fed/FOMC event dates (paid tier; planned report enrichment, see below)
- stock / general news feeds (premium tier only — see below)

**Endpoints used by the report** — all on the `https://financialmodelingprep.com/stable` base. The free-tier paths are wired today; the paid-tier paths are the **planned report enrichment** specified below (not yet wired).

| Endpoint path | Tier | Report use |
| --- | --- | --- |
| `/quote` | free | per-symbol quotes — indices, VIX, gold (`GCUSD`), silver (`SIUSD`); also the connection test |
| `/historical-price-eod/light` | free | end-of-day price history → multi-horizon index performance (weekly / MTD / YTD / 52-week range) |
| `/sector-performance-snapshot` | free | per-sector performance (point-in-time, date-keyed) |
| `/sector-pe-snapshot` | free | per-sector aggregate P/E (point-in-time, by exchange) |
| `/industry-performance-snapshot` | free | per-industry performance (point-in-time, by exchange) |
| `/industry-pe-snapshot` | free | per-industry aggregate P/E (point-in-time, by exchange) |
| `/biggest-gainers` | free | market movers — biggest gainers |
| `/biggest-losers` | free | market movers — biggest losers |
| `/most-actives` | free | market movers — most active (NB plural; singular `most-active` 404s) |
| `/earnings-calendar` | free | large-cap earnings (recent + upcoming window) |
| `/market-risk-premium` | free | US equity-risk-premium (Damodaran per-country dataset, US row) |
| `/fmp-articles` | free | FMP in-house, ticker-tagged commentary → Step-7 news funnel |
| `/economic-calendar` | paid | release consensus + realized surprise + Fed/FOMC dates, layered onto FRED's schedule |
| `/historical-sector-pe` | paid | trailing-window sector P/E → percentile + band |
| `/historical-industry-pe` | paid | trailing-window industry P/E → percentile + band |
| `/historical-sector-performance` | paid | sector daily `averageChange` → trailing cumulative return |
| `/historical-industry-performance` | paid | industry daily `averageChange` → trailing cumulative return |
| `/ipos-calendar` | paid | recently priced + upcoming scheduled IPOs → issuance-froth count |
| `/mergers-acquisitions-latest` | paid | recently announced M&A deals → deal-froth count |

The application calls Financial Modeling Prep directly for the equity-market portion of the baseline market-data scan ([report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data)) — indices, volatility, gold and silver, and sector performance; each index's multi-horizon performance (weekly, month-to-date, year-to-date, and 52-week-range position) derived from FMP's free historical end-of-day prices (verified live: the indices and the VIX return on the free tier); the **market movers** (biggest gainers / losers / most-active names, filtered to major-exchange names above a price floor, with leveraged / inverse ETFs excluded, capped per list); the free **earnings calendar** (the recently reported and upcoming large-cap reporters, on FMP's ~1-month free window, filtered by revenue estimate — the recently-reported lookback is sized to the report cadence, so a monthly run sees the whole interval's reporters rather than just the last week, while the upcoming window stays fixed); and the free **valuation + finer-rotation** snapshots — per-**sector P/E** (a valuation read alongside sector performance), the strongest and weakest **industries** (FMP reports ~130 per exchange, capped to the extremes; each industry's average move joined with its aggregate P/E where available), and the US **equity-risk-premium** (from FMP's per-country market-risk-premium dataset, a near-static annual constant). The mover lists carry no sector, no instrument type, and no index membership on the free tier, so the agent infers a mover's sector from its ticker (and treats any fund row that slips the name filter as a flow signal, not a company), and the earnings calendar is filtered by revenue magnitude rather than index membership. The sector / industry snapshots and the market-risk-premium are all on FMP's free tier (verified live); the per-sector and per-industry snapshots are date-keyed (the adapter walks back to the most recent trading day with data, like sector performance), and the industry valuation is a join of the industry-performance and industry-P/E snapshots by industry name. **These valuation snapshots are exchange-specific** (verified live: a no-`exchange` call defaults to NASDAQ only; NYSE and AMEX are also free), so the adapter pins and gathers **both NASDAQ (growth / tech-tilted) and NYSE (broader / value)** for each, tags every row with its exchange, applies the industry cap per exchange, and joins performance to P/E within a single exchange — the model reads these cross-sectionally (rich vs cheap, and growth-board vs value-board) rather than as a whole-market multiple. (On the paid tier this point-in-time read gains a trailing-window time dimension — current multiple *and* current return vs each group's own history — see **Planned report enrichment** below.) The scan's dollar-index, oil, natural-gas, and Treasury-yield series come from FRED (below). (Gold is on FMP's free tier via `GCUSD`; FRED's former free gold benchmark series were discontinued, so gold stays on FMP.) The **economic-release calendar**'s release schedule comes from FRED's free release-dates endpoint (below) rather than FMP, whose `economic-calendar` is premium-gated (verified live: HTTP 402 on the free tier); on the paid tier that endpoint becomes available to *layer analyst consensus, realized surprise, and Fed/FOMC dates onto* FRED's schedule (see **Planned report enrichment** below). FMP's third-party **news feeds** (`news/general-latest`, `news/stock-latest`, symbol-scoped `news/stock`, `news/press-releases-latest`) are all premium too (verified live: HTTP 402 on the free tier); the one news surface on the free tier is **FMP Articles** (`fmp-articles`, verified live: HTTP 200 with `page`/`limit` paging honored) — FMP's in-house, ticker-tagged market commentary — which feeds the Step-7 news funnel as a best-effort supplementary source (see News and Research below).

### FRED (Federal Reserve Economic Data)
Docs - https://fred.stlouisfed.org/docs/api/fred/

FRED provides official macroeconomic and financial data maintained by the Federal Reserve Bank of St. Louis.

Responsibilities:
- Treasury yields
- interest rates
- the US dollar index
- commodity prices (oil, natural gas)
- inflation metrics
- recession indicators (yield-curve spreads: 10y–3m, 10y–2y)
- credit spreads (high-yield, investment-grade, BBB, and single-B OAS)
- equity volatility term structure (S&P 3-month VIX, Nasdaq-100 volatility)
- financial-conditions indices (NFCI, ANFCI, St. Louis stress index)
- unemployment data (incl. weekly initial and continued jobless claims)
- Fed balance sheet and mortgage rates
- consumer data
- broader macroeconomic indicators
- forward-looking expectations (Atlanta Fed GDPNow real-GDP nowcast; Cleveland Fed expected inflation)
- economic-release calendar (release schedule)

**Endpoints + series** — base `https://api.stlouisfed.org/fred`. Two endpoints: `/series/observations` (latest level of each series below — the `series_id` doubles as the quote symbol) and `/release/dates` (the economic-release calendar schedule). The report's FRED series, by Step-3 baseline group:

*Market internals — daily, market-priced (the level is the signal):*

| Series ID | Series | Unit |
| --- | --- | --- |
| `DGS2` | 2-Year Treasury Yield | percent |
| `DGS10` | 10-Year Treasury Yield | percent |
| `DTWEXBGS` | US Dollar Index (Broad) | index (Jan 2006=100) |
| `DCOILWTICO` | WTI Crude Oil | USD / barrel |
| `DHHNGSP` | Henry Hub Natural Gas | USD / MMBtu |
| `BAMLH0A0HYM2` | US High-Yield Corporate OAS | percent |
| `BAMLC0A0CM` | US Investment-Grade Corporate OAS | percent |
| `BAMLH0A2HYB` | US High-Yield B OAS | percent |
| `BAMLC0A4CBBB` | US Corporate BBB OAS | percent |
| `T10Y3M` | 10Y − 3M Treasury spread | percent |
| `T10Y2Y` | 10Y − 2Y Treasury spread | percent |
| `VXVCLS` | CBOE S&P 500 3-Month Volatility (VXV) | index points |
| `VXNCLS` | CBOE Nasdaq-100 Volatility (VXN) | index points |

*Macro levels — mixed daily / monthly / quarterly:*

| Series ID | Series | Unit |
| --- | --- | --- |
| `DFEDTARU` / `DFEDTARL` | Fed Funds Target Range — upper / lower | percent |
| `T5YIE` / `T10YIE` | 5- / 10-Year Breakeven Inflation | percent |
| `EXPINF1YR` | Cleveland Fed 1-Year Expected Inflation | percent |
| `UMCSENT` | U. Michigan Consumer Sentiment | index (1966Q1=100) |
| `PCEPI` | PCE Price Index | index (2017=100) |
| `PPIFIS` | Producer Price Index (Final Demand) | index (Nov 2009=100) |
| `RSAFS` | Advance Retail Sales (Retail & Food Services) | millions USD |
| `JTSJOL` | Job Openings — Total Nonfarm (JOLTS) | thousands |
| `GDPC1` | Real GDP (growth annualized) | billions chained 2017 USD |
| `GDPNOW` | Atlanta Fed GDPNow nowcast (annualized) | percent |
| `NFCI` / `ANFCI` | Chicago Fed (Adjusted) National Financial Conditions | index (0 = average) |
| `STLFSI4` | St. Louis Fed Financial Stress Index | index (0 = normal) |
| `ICSA` / `CCSA` | Initial / Continued Jobless Claims | persons |
| `WALCL` | Fed Total Assets (balance sheet) | millions USD |
| `MORTGAGE30US` | 30-Year Fixed Mortgage Rate | percent |

The application uses FRED for macroeconomic analysis and long-term market-regime evaluation, and for the market-internal series — the dollar index, oil, and natural gas — that sit outside Financial Modeling Prep's free-tier coverage. It also supplies the risk- and cycle-oriented series that anchor the report's risk-posture and market-cycle reads: credit spreads (the aggregate high-yield and investment-grade OAS plus the BBB and single-B buckets for credit-quality dispersion), the equity volatility term structure (the S&P 3-month VIX, paired with the FMP VIX for a backwardation read, and Nasdaq-100 volatility), the 10y–3m and 10y–2y curve spreads, the Chicago Fed financial-conditions indices (NFCI, ANFCI) and the St. Louis stress index (STLFSI4), weekly initial and continued jobless claims, the Fed balance sheet, and the 30-year mortgage rate. (FRED's documented limit is 120 requests/minute with no daily cap; each run's ~40-request scan sits far under it.) It additionally supplies two **forward-looking expectation gauges** in the macro-levels group — the Atlanta Fed **GDPNow** current-quarter real-GDP nowcast (an annualized growth rate, a forward complement to the actual GDP print) and the Cleveland Fed **1-year expected-inflation** series (a model-based read alongside the market-implied breakevens). It also supplies the Step-3 **economic-release calendar** — the recent and upcoming US release schedule (CPI, PCE, jobs, GDP, …) via FRED's free release-dates API — since FMP's economic-calendar endpoint is premium-gated. (Like the earnings calendar, the recent-releases lookback is sized to the report cadence — a monthly run keeps the whole interval's releases — while the upcoming-schedule window stays fixed.) (FRED has no scheduled-date series for FOMC meetings, so the FRED calendar excludes them — the planned FMP enrichment supplies Fed / FOMC event dates; the Fed-funds target-range series continues to carry the policy *stance*.) FRED provides release dates (and the underlying series values, gathered separately) but not analyst-consensus estimates, so the FRED-sourced calendar carries release names and dates only. Consensus and realized surprise are a **planned paid-tier enrichment** layered on from FMP (see **Planned report enrichment** below); where FMP carries no estimate for a release, the "expected" value falls back to the agents' research-phase synthesis as it does today.

### Planned report enrichment (paid FMP tier)

Upgrading the shared FMP credential to the paid tier (the one paid dependency the local analysis suite already requires — see *Local Analysis Suite Sources* below) unlocks three report-side baseline enrichments. Each is an **opt-in addition to the existing scan, not a replacement**: the report's current data-source logic is unchanged, each enrichment soft-degrades to today's behavior on any failure, and all are paid-gated, so they are live-verified together with the suite's paid-key checkpoint.

**Economic-calendar consensus + surprise.** FRED stays the release-schedule backbone; FMP's paid `economic-calendar` (`?country=US&from=&to=`, fields `event` / `date` / `impact` / `previous` / `estimate` / `actual`) layers on two things the FRED schedule can't carry. (1) For the report's tracked market-moving releases, the engine joins FMP's `estimate` / `actual` onto the matching FRED release through a **curated release→event map** — FRED release names ("Employment Situation") and FMP event names ("Non Farm Payrolls") don't string-match, and one release fans out to several FMP events — then computes a deterministic **beat / miss / in-line** tag and an actual-vs-estimate **% gap**: the engine derives, the model interprets. (2) FMP-only **Fed / FOMC events** (filtered to Medium/High `impact`), which FRED has no scheduled-date series for, are appended as a distinct event class, closing the documented FOMC-date gap. Fail-soft throughout: an unmapped release, a `null` estimate, or an FMP outage leaves that release at **names + dates only** (today's behavior) — never a fabricated consensus.

**Historical sector / industry valuation + performance.** Today the sector/industry P/E *and* performance are both point-in-time, read only cross-sectionally (which group is rich vs cheap, strong vs weak *right now*). Four paid endpoints — `historical-sector-pe` / `historical-industry-pe` and `historical-sector-performance` / `historical-industry-performance`, each keyed by sector/industry + `exchange`, with `from` / `to` — add a **time dimension** over a fixed trailing ~1 year, fetched for all 11 sectors × both exchanges and for the **extreme industries the snapshot already surfaces** (not all ~130). The two series are shaped differently, so the engine derives a different read from each:
- the P/E endpoints return a `pe` **level**, so the engine takes its **percentile within the trailing window + a min / median / max band** — rich/cheap against its own history;
- the performance endpoints return a daily **`averageChange`** (that date's average constituent move), *not* a price level, so the engine **accumulates the daily changes into a trailing cumulative return** — the rising / falling read — rather than percentiling the raw daily moves (a percentile of `averageChange` would say "today's move was unusual," not "the group is up over the window").

Both are compact derived numbers, not the raw series. Paired, they let the model read a re-rating *with* its price context: a group cheap against its own P/E history *and* up over the trailing window (a re-rating turn) reads differently from one cheap *and still down* (a possible value trap) — the distinction a single snapshot can't support.

**IPO / M&A froth.** The report has no primary-market feed today, so it can't see the issuance / deal-making pace that runs hot late-cycle and freezes under stress. Two paid endpoints add it — `ipos-calendar` (`?from=&to=`, recently priced + upcoming scheduled offerings) and `mergers-acquisitions-latest` (`?page=&limit=`, recently announced deals) — which the engine reduces to a compact **activity read**: the recent-window IPO count plus the upcoming-scheduled count, and the recent-window M&A deal count — each paired with the **prior equivalent-window count** so the engine carries a native rising / cooling **trend** (the way CFTC positioning carries its own week-over-week change while staying out of the level-delta engine — the model is handed only the current packet plus the computed change view, never prior raw packets, so the trend can't come from the delta engine and must be self-contained) — plus a bounded list of the largest / most notable names (and aggregate proceeds / deal value where the feed carries it) for color. Like the earnings and economic-release calendars, the recent window is sized to the report cadence (a monthly run sees the month's froth, not a week's) while the upcoming-IPO window stays fixed. The model reads the pace *and its trend* as a risk-appetite / late-cycle tell — a surge or accelerating pace feeding the risk-on / late-cycle read, a freeze the risk-off / stress read. Fail-soft and non-floor: a failed gather degrades to no froth signal, never a failed run.

All three enrichments follow the same three structural rules so they neither bloat storage nor disturb the report-to-report change view:

- **Persist the derived read, not the raw history.** Only the compact derived numbers — the calendar's estimate / actual / surprise tag, the P/E percentile + band, the performance trailing cumulative return, and the froth recent + prior-window counts with the bounded notable-name list — ride into the packet and the persisted baseline snapshot; the raw ~250-point P/E and performance series and the full IPO / deal lists are transient fetch input, discarded once the derived reads are computed.
- **New fields carry `#[serde(default)]`.** The enrichments add fields to `BaselineMarketData` and its member structs — `EconomicRelease` (calendar consensus/surprise), `SectorPe` and `IndustrySnapshot` (valuation-history context), `SectorPerformance` and `IndustrySnapshot` (performance-history context) — plus a new issuance-activity group; each must default so an *older* snapshot — serialized before the field existed — still deserializes (to empty / `None`), keeping the prior-vs-current comparison backward-compatible. (The prior-snapshot decode is already fail-soft regardless — see [report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data).)
- **All stay out of the level-delta engine.** The surprise is a native actual-vs-consensus value, the P/E percentile + band and the performance trailing cumulative return are trailing-window structural reads, and the IPO / M&A counts are set-valued activity tallies — none is an inter-report level change — so, like CFTC positioning and the existing movers / earnings / calendar groups, none joins the diffed level groups; the existing point-in-time `sector_pe` level-diff is untouched (it reads only the current `pe`), and the `sectors` performance group stays excluded from the diff exactly as it is today.

The prompt-side changes these require are specified in [report-workflow.md §Step 16](report-workflow.md#step-16-main-agent-synthesis): the data appears in every model prompt automatically via JSON serialization, but the interpretive prose must be updated in lockstep — including one existing main-agent instruction that currently tells the model to *ignore* multiple-expansion-over-time, which the P/E history now supports.

### BLS (Bureau of Labor Statistics)
Docs - https://www.bls.gov/developers/

BLS provides official United States labor and inflation datasets.

Responsibilities:
- CPI reports
- employment reports
- wage data
- labor-market statistics

**Endpoint + series** — base `https://api.bls.gov/publicAPI/v2`, single endpoint `/timeseries/data/` (series IDs posted in the request body; the `series_id` doubles as the quote symbol):

| Series ID | Series | Unit |
| --- | --- | --- |
| `CUUR0000SA0` | Consumer Price Index (CPI-U, All Items, NSA) | index (1982-84=100) |
| `LNS14000000` | Unemployment Rate (U-3) | percent |
| `CES0000000001` | Total Nonfarm Payrolls | thousands of persons |
| `CES0500000003` | Average Hourly Earnings, Total Private | USD per hour |

The application uses BLS data for inflation and labor-market analysis.

### CFTC (Commitments of Traders)
Docs - https://publicreporting.cftc.gov/ (Socrata Open Data API)

The CFTC's weekly Commitments of Traders report supplies the one signal the price, valuation, macro, and credit groups can't: how crowded or extended the *speculative* cohort is in the market's bellwether futures. It is the application's positioning input, accessed through the CFTC public-reporting Socrata API, which is **keyless** — like BLS, it needs no credential and sits outside the execution gate.

**Endpoint** — each report is a Socrata resource at `https://publicreporting.cftc.gov/resource/<dataset-id>.json`, one call per dataset (the two dataset IDs below).

Two report formats are read and normalized into a single speculator-net view per contract:
- **Traders in Financial Futures** (dataset `gpe5-46if`) — for the equity-index, rates, and FX contracts. Its leveraged-money ("fast money") and asset-manager ("real money") split is the signal: the two cohorts often diverge (real money net long while leveraged money presses shorts).
- **Disaggregated** (dataset `72hh-3qpy`) — for the commodity contracts. Managed money is the speculator proxy; there is no asset-manager cohort, so the real-money line is omitted.

Curated bellwether contracts, pinned by CFTC contract code (never free-text — names collide across micro / consolidated variants):
- equity index: E-Mini S&P 500, Nasdaq-100
- rates: 10-Year and 2-Year U.S. Treasury Notes
- FX: U.S. Dollar Index
- commodities: Gold, WTI Crude Oil, Copper

Each row carries the speculator net (long − short), its week-over-week change, the speculator long as a percent of open interest, and — for financial futures — the asset-manager net and its change. The data is weekly: a Tuesday snapshot released the following Friday, so a report always reads the prior week's positioning, and each row carries its snapshot date so the model reads it as-of. A bounded freshness guard drops a row older than three weeks (a stalled feed) to a gap rather than presenting it as current. The group is fail-soft and additive — a flaky contract or a whole-API outage degrades to a recorded gap rather than failing the run, and it carries no coverage floor. Because COT already carries its own native week-over-week change and follows a fixed weekly cadence, the positioning group is exempt from the report-over-report baseline change view.

## News and Research

### Tavily
Docs - https://docs.tavily.com/welcome

Tavily provides AI-oriented web search and research retrieval.

Responsibilities:
- gathering relevant market news
- retrieving research sources
- identifying important developing stories
- supplying contextual research material to the agents

The application uses Tavily as the primary research and news-ingestion system.

**Endpoint** — `https://api.tavily.com/search` (the Search API; `/usage` backs the connection test).

Because reports are generated on demand (no fixed cadence — see [scheduling.md §Generating a Report](scheduling.md#generating-a-report)), the Step-7 news sweep sizes Tavily's recency bound to the **elapsed interval since the previous report**: it sends a `start_date` of today minus the elapsed days (clamped to a floor and a one-month cap), so a daily run isn't fed a stale week and a monthly run isn't starved of coverage. (`start_date`/`end_date` are the documented Tavily Search recency parameters; the former `days` field is no longer part of the API.) The first report (no prior interval) omits the bound and takes Tavily's own default. The Step-9 research executor's plan queries carry no recency bound — they target a topic, not a time slice.

### FMP Articles
Docs - https://site.financialmodelingprep.com/developer/docs (Articles)

FMP Articles is Financial Modeling Prep's in-house editorial feed: short, ticker-tagged market commentary written against FMP's own data (analyst-coverage moves, earnings reactions, notable price action). It is the one FMP news surface on the free tier (verified live; the third-party `news/*` feeds are premium — see Financial Modeling Prep above), and reuses the FMP API key already required for market data.

Responsibilities:
- supplementary company-level market headlines for the Step-7 news funnel
- ticker-tagged story metadata (each article carries an exchange-prefixed ticker)
- a free resilience hedge alongside Tavily (quota-limited) and GDELT (lockout-prone)

The application gathers a single bounded page of recent articles per run, alongside Tavily's topic sweep and GDELT's geopolitical query. The feed is best-effort: it is house-written and overlaps what Tavily already indexes, so the deterministic dedup pre-pass and the headline-filter model absorb redundancy, and a failing gather degrades to no headlines rather than failing the job — the same fail-soft posture as GDELT.

### GDELT
Docs - https://www.gdeltproject.org/data.html

GDELT is a global event and news-monitoring platform that tracks worldwide news coverage and geopolitical developments.

Responsibilities:
- geopolitical monitoring
- conflict tracking
- global event detection
- international news analysis
- large-scale news trend identification

The application uses GDELT to strengthen geopolitical and macro event awareness.

**Endpoint** — `https://api.gdeltproject.org/api/v2/doc/doc` (the DOC 2.0 API; a single combined query per run).

GDELT's single combined query sizes its `timespan` lookback to the **elapsed interval since the previous report** (rounded up to whole days, clamped to a floor and a one-month cap), rather than a fixed week — keeping the geopolitical feed matched to the on-demand cadence. The first report (no prior interval) uses a one-week default. This changes only the window width, not the request count: it stays a single bounded query, so GDELT's burst rate limit is unaffected.

## Local Analysis Suite Sources

These sources serve the local analysis suite ([local-models.md](local-models.md)), not the Market Signal Report. Even on the paid FMP key, the suite **spreads its high-volume per-holding load across keyless providers** rather than routing everything through one: company fundamentals cross-checked against **SEC EDGAR** (keyless) and deep historical prices from **Stooq** (keyless), leaving **FMP** for fundamentals breadth, the niche aggregates (movers, earnings calendar, screener, sector / industry P/E), and live **quotes** (`quote`). All sit behind the same data-source seam and fail-soft posture as the report's adapters.

**Both local jobs run on FMP's paid tier** — Trade Opportunities most heavily (it also screens the universe), Portfolio Analysis across a per-holding subset. The application uses **one shared FMP credential for everything — the report and both local jobs — now upgraded to the paid tier** (the suite's one paid dependency, so the user manages a single key). The report's data-source *logic* is unchanged (its existing calls behave identically on the paid key), and the former free-tier gates simply no longer bind — so the newly-unlocked endpoints are available to enrich the report packet as a separate, opt-in enhancement (see [trade-opportunities.md §Signal inputs](trade-opportunities.md#signal-inputs)). The paid tier is the broad working & discovery feed: financial statements / ratios / revenue **segments** (product + geographic) / **earnings-call transcripts** / owner earnings / DCF; the **revision signal** (analyst estimates snapshotted for velocity, the `grades-historical` rating-distribution time series, price-target trend, upgrades / downgrades, earnings surprises); **`financial-scores` — Altman Z + Piotroski** for the forensic gate; **positioning** (insider buys / sells + statistics, 13F institutional, and **Senate / House congressional trading** — all symbol-keyed); and the **screener / peers / industry-classification** plus **bulk** endpoints (scores / surprises / ratings / ratios across the whole universe) that power the discovery funnel without a per-symbol rate cap. The **per-symbol** signals here — fundamentals, the revision signal, `financial-scores`, positioning, and peers — are shared by **Portfolio Analysis** (grading held positions) and **Trade Opportunities** (validating candidates); the **screener / bulk / industry-classification** discovery layer is **Trade Opportunities only**, since Portfolio Analysis grades a known holdings list and never screens the universe. **Short interest** comes keyless from FINRA (FMP has no short-interest endpoint); **cyclical commodity prices** from FRED (daily energy + monthly IMF metals — copper, aluminum, nickel, iron ore, uranium) and Stooq futures. **SEC EDGAR is retained as the authoritative primary-source cross-check** for the numbers that drive grades / targets, which makes **ticker→CIK resolution a non-blocking enhancement** rather than a prerequisite (the FMP feeds are symbol-keyed). DRAM/NAND ASPs and supply-discipline signals have no structured feed and ride the research loop ([web-research.md](web-research.md)).

**Portfolio Analysis** reads a per-holding / per-fund subset of that FMP surface (it grades a known holdings list, so it never touches the discovery layer) plus run-level macro inputs; analyst opinions it pulls — price targets, grade distributions, FMP's ratings snapshot — ride in as *evidence the model weighs, never as inputs to the engine's computed grade*. Its full endpoint surface, by source, is tabulated under [§Portfolio Analysis — endpoint surface](#portfolio-analysis--endpoint-surface) below.

### Charles Schwab (Trader API)
Docs - https://developer.schwab.com/

Charles Schwab is the source of the user's **portfolio holdings** *and* **equity option chains** for the local suite — positions (quantity, cost basis, market value, instrument identity) via the accounts endpoint, and live option chains via the same OAuth's market-data endpoint (`/marketdata/v1/chains`, returning per-contract volume, open interest, implied volatility, and greeks). It is not a fundamentals source — Schwab's fundamentals are thin, so company financials come from FMP and SEC EDGAR (below). From the chains the suite computes a deterministic **options-activity signal** — put/call ratio (by volume and open interest) plus an IV/skew read (a rough activity proxy, not positioning truth — see [schwab-integration.md](schwab-integration.md)). A connected Schwab account is **required to run either local job**; authentication, token lifecycle, account hashing, and the manual-import supplement are described in [schwab-integration.md](schwab-integration.md).

### SEC EDGAR
Docs - https://www.sec.gov/edgar/sec-api-documentation

SEC EDGAR is the **authoritative source for company filings and fundamentals** behind Portfolio Analysis (and candidate validation in Trade Opportunities): 10-K / 10-Q / 8-K submissions and the XBRL **company-facts** API for normalized financial-statement data. It is **keyless** (like BLS and CFTC), requiring only a declared User-Agent with contact info per the SEC's fair-access policy, and is rate-limited to ~10 requests/second. SEC supplies the raw statement data the deterministic financial-analysis engine ([portfolio-analysis.md](portfolio-analysis.md)) computes over; FMP remains for convenient normalized metrics and market-data signals. Authoritative filings reduce the suite's dependence on web-search summaries for the numbers that drive grades and targets.

For Trade Opportunities, EDGAR is the **authoritative cross-check** behind the FMP working feed — the final grade / target numbers are reconciled against EDGAR filings and XBRL (compute-don't-guess), and 8-K filings remain a primary source. Positioning signals (insider, 13F, congressional) are sourced from FMP's structured, **symbol-keyed** endpoints rather than parsed from EDGAR's CIK-keyed filings, so **ticker→CIK resolution** (SEC's `company_tickers.json`; today only a hardcoded handful resolve) is a **non-blocking enhancement** — needed only to extend the SEC cross-check to arbitrary names — not a hard prerequisite.

### FINRA (short interest)
Docs - https://www.finra.org/finra-data/browse-catalog/equity-short-interest

FINRA publishes the **consolidated Equity Short Interest** file — keyless, biweekly (mid- and end-of-month settlement, disseminated ~7–8 business days later), covering exchange-listed and OTC equities since 2021. Each record carries current and prior short interest plus average daily volume, giving short-interest level, trend, and days-to-cover directly — the squeeze-fuel / crowded-bear signal for Trade Opportunities **and a risk / squeeze-context input for Portfolio Analysis's held equities** (informing trims and adds; the biweekly file is fetched once per run and looked up per holding). It is best-effort and fail-soft like the suite's other additive feeds.

### Stooq
Docs - https://stooq.com/db/h/

Stooq is the suite's **deep historical price source** — keyless, with no documented rate cap, serving 20–30+ years of daily OHLCV per symbol via a simple CSV endpoint (plus bulk database downloads). Its value holds **independent of FMP's tier**: the multi-decade depth is what the engine's **price-action confirmer** (multi-year base-breakout / long relative-strength) and the momentum / volatility / price-target computations need, and serving that highest-volume per-holding read keylessly keeps the bulk price load off the shared FMP key regardless of cap. Like GDELT it is best-effort and fail-soft: an informal source with no SLA and occasional symbol-mapping gaps (US tickers take a `.us` suffix), so a missing series degrades to a gap rather than failing the run, and the adapter self-throttles to stay polite.

### CBOE
Docs - https://www.cboe.com/us/options/market_statistics/daily/

CBOE provides a free, keyless daily **put/call ratio for its own exchange's flow** (total, equity, index) — a Cboe **venue-level** options-sentiment backdrop (not consolidated all-market data, which is a separate paid dataset), distinct from the per-stock put/call the suite computes from Schwab chains. It is an optional broad-market context signal read from Cboe's daily statistics, fail-soft and venue-level only (no per-symbol breakdown), so it informs the macro sentiment read rather than any single verdict.

### SearXNG (local web search)
Docs - https://docs.searxng.org/

SearXNG is the local suite's **web-search backend** — a self-hosted, keyless metasearch instance queried over its JSON API on the loopback interface, fanning queries out to general engines. It is the primary search source for the suite's research loop, with the existing Tavily integration as a fallback. The search / fetch / extract tool built on it is described in [web-research.md](web-research.md).

### Portfolio Analysis — endpoint surface

Every endpoint Portfolio Analysis ([portfolio-analysis.md](portfolio-analysis.md)) calls, by source, paralleling the report's per-source tables above. **Cardinality** is the load-bearing axis — it sets the per-run call budget: **per-holding** and **per-fund** calls scale with portfolio size (the budget driver), while **run-level** calls fire once and are shared across all holdings. All FMP paths are on the `https://financialmodelingprep.com/stable` base and run on the shared paid key.

**FMP**

| Endpoint path | Cardinality | Portfolio Analysis use |
| --- | --- | --- |
| `profile` | per-holding | sector / industry / **beta** / description — classification + risk input |
| `income-statement`, `balance-sheet-statement`, `cash-flow-statement` (+ `…-ttm`) | per-holding | core financial statements — engine fundamentals |
| `key-metrics`, `ratios` (+ `…-ttm`) | per-holding | valuation / quality / leverage / margin ratios |
| `financial-scores` | per-holding | Altman Z + Piotroski → risk / quality forensic input |
| `owner-earnings` | per-holding | owner earnings (cash to shareholders) for valuation |
| `enterprise-values` | per-holding | enterprise value for EV multiples |
| `discounted-cash-flow` | per-holding | DCF valuation cross-check |
| `analyst-estimates` | per-holding | forward revenue / EPS consensus → engine **revision-velocity** read |
| `price-target-consensus`, `price-target-summary` | per-holding | street price-target level + trend — *evidence, not an engine input* |
| `grades`, `grades-historical`, `grades-consensus` | per-holding | `grades-historical` distribution → engine **rating-drift** read; rating actions + current consensus ride as *evidence* |
| `ratings-snapshot`, `ratings-historical` | per-holding | FMP's own composite rating — opinion cross-check only |
| `dividends` | per-holding | yield, frequency, schedule — income / total-return grading |
| `earnings` | per-holding | next earnings date + EPS / revenue estimate (catalyst); actual-vs-estimate surprise history |
| `insider-trading/search`, `insider-trading/statistics` | per-holding | insider buys / sells + aggregate statistics |
| `institutional-ownership/symbol-positions-summary` | per-holding | 13F institutional ownership |
| `senate-trades`, `house-trades` | per-holding | congressional trading in the name |
| `stock-peers` | per-holding | peer set for relative valuation |
| `shares-float` | per-holding | free float / liquidity → risk input |
| `mergers-acquisitions-search` | per-holding | whether the held name is acquirer or target (catalyst) |
| `revenue-product-segmentation`, `revenue-geographic-segmentation` | per-holding | revenue by product / geography — business mix, thematic exposure, what-changed attribution |
| `earning-call-transcript`, `earning-call-transcript-dates` | per-holding | management commentary — guidance / margins / demand / capex (grounds the research lane) |
| `quote` | per-holding | live quote (current price) |
| `etf/info` | per-fund | expense ratio, AUM, NAV, asset class, mandate |
| `etf/holdings` | per-fund | constituents + weights — look-through, concentration |
| `etf/sector-weightings`, `etf/country-weightings` | per-fund | sector / country exposure |
| `etf/asset-exposure` | per-equity (optional) | reverse lookup — which ETFs hold a directly-held name; a cross-check only, *not* the look-through source (look-through concentration is computed locally from `etf/holdings`) |
| `funds/disclosure-holders-latest`, `funds/disclosure`, `funds/disclosure-dates` | per-fund | mutual-fund holdings disclosures |

**FRED** — base `https://api.stlouisfed.org/fred`, `/series/observations` (the `series_id` doubles as the quote symbol).

| Series ID | Series | Cardinality | Portfolio Analysis use |
| --- | --- | --- | --- |
| `DGS10` | 10-Year Treasury Yield | run-level | risk-free rate → valuation-engine discounting |
| `DGS2` | 2-Year Treasury Yield | run-level | risk-free rate (short end) |
| `DCOILWTICO` | WTI Crude Oil | run-level | commodity context for energy-linked holdings |
| `DHHNGSP` | Henry Hub Natural Gas | run-level | commodity context for energy-linked holdings |

Materials-linked holdings reuse the suite's broader FRED commodity set (monthly IMF metals incl. copper, aluminum, nickel, iron ore, uranium — IDs catalogued with the suite commodity feed, see [trade-opportunities.md §Signal inputs](trade-opportunities.md#signal-inputs)); gold is FMP `GCUSD`.

**CFTC** — Socrata, base `https://publicreporting.cftc.gov/resource/<dataset>.json`. The same keyless pull the report makes; Portfolio Analysis maps a fund holding onto an already-gathered contract.

| Dataset | Contracts | Cardinality | Portfolio Analysis use |
| --- | --- | --- | --- |
| `gpe5-46if` (Traders in Financial Futures) | E-mini S&P 500, Nasdaq-100, 10Y / 2Y Treasuries, USD Index | run-level | underlying positioning for an equity-index / rates / FX **fund** holding |
| `72hh-3qpy` (Disaggregated) | Gold, WTI Crude, Copper | run-level | underlying positioning for a commodity **fund** holding |

A fund whose underlying isn't among these contracts fail-softs to no positioning read.

**Schwab / SEC EDGAR / Stooq / FINRA** — the account and keyless sources (full endpoint detail in their sections above).

| Source · endpoint | Cardinality | Portfolio Analysis use |
| --- | --- | --- |
| Schwab · accounts / positions | run-level | holdings — quantity, cost basis, market value, instrument identity |
| Schwab · `/marketdata/v1/chains` | per-holding (optionable equity) | option chains → options-activity signal (put/call, IV/skew) |
| SEC EDGAR · submissions (10-K / 10-Q / 8-K) | per-holding | filings — authoritative cross-check + 8-K events |
| SEC EDGAR · company-facts (XBRL) | per-holding | normalized statement data the engine computes over |
| Stooq · daily OHLCV CSV | per-holding | deep price history → momentum / volatility / price-target scenarios |
| FINRA · consolidated short-interest file | per-holding lookup (file fetched once / run) | short-interest level / trend / days-to-cover → risk / squeeze context |

## LLM Providers
- [OpenAI](https://platform.openai.com/docs)
- [Anthropic](https://platform.claude.com/docs)

The specific models exposed by these providers, the user-configurable model selections for each agent, and the API-token requirements are covered in [configuration.md](configuration.md). The non-configurable models used by fixed internal pipeline stages are covered in [agents.md](agents.md). The local analysis suite uses local open-weight models served on-device instead of these providers — see [local-models.md](local-models.md).
