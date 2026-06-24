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
- FMP Articles — in-house, ticker-tagged market commentary (free tier; feeds the Step-7 news funnel — see News and Research below)
- economic calendar (premium tier only — see below)
- stock / general news feeds (premium tier only — see below)

The application calls Financial Modeling Prep directly for the equity-market portion of the baseline market-data scan ([report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data)) — indices, volatility, gold and silver, and sector performance; each index's multi-horizon performance (weekly, month-to-date, year-to-date, and 52-week-range position) derived from FMP's free historical end-of-day prices (verified live: the indices and the VIX return on the free tier); the **market movers** (biggest gainers / losers / most-active names, filtered to major-exchange names above a price floor, with leveraged / inverse ETFs excluded, capped per list); the free **earnings calendar** (the recently reported and upcoming large-cap reporters, on FMP's ~1-month free window, filtered by revenue estimate — the recently-reported lookback is sized to the report cadence, so a monthly run sees the whole interval's reporters rather than just the last week, while the upcoming window stays fixed); and the free **valuation + finer-rotation** snapshots — per-**sector P/E** (a valuation read alongside sector performance), the strongest and weakest **industries** (FMP reports ~130 per exchange, capped to the extremes; each industry's average move joined with its aggregate P/E where available), and the US **equity-risk-premium** (from FMP's per-country market-risk-premium dataset, a near-static annual constant). The mover lists carry no sector, no instrument type, and no index membership on the free tier, so the agent infers a mover's sector from its ticker (and treats any fund row that slips the name filter as a flow signal, not a company), and the earnings calendar is filtered by revenue magnitude rather than index membership. The sector / industry snapshots and the market-risk-premium are all on FMP's free tier (verified live); the per-sector and per-industry snapshots are date-keyed (the adapter walks back to the most recent trading day with data, like sector performance), and the industry valuation is a join of the industry-performance and industry-P/E snapshots by industry name. **These valuation snapshots are exchange-specific** (verified live: a no-`exchange` call defaults to NASDAQ only; NYSE and AMEX are also free), so the adapter pins and gathers **both NASDAQ (growth / tech-tilted) and NYSE (broader / value)** for each, tags every row with its exchange, applies the industry cap per exchange, and joins performance to P/E within a single exchange — the model reads these cross-sectionally (rich vs cheap, and growth-board vs value-board) rather than as a whole-market multiple. The scan's dollar-index, oil, natural-gas, and Treasury-yield series come from FRED (below). (Gold is on FMP's free tier via `GCUSD`; FRED's former free gold benchmark series were discontinued, so gold stays on FMP.) The **economic-release calendar** is likewise gated behind FMP premium (verified live: the `economic-calendar` endpoint returns HTTP 402 on the free tier), so the Step-3 calendar's release schedule comes from FRED's free release-dates endpoint (below) rather than FMP. FMP's third-party **news feeds** (`news/general-latest`, `news/stock-latest`, symbol-scoped `news/stock`, `news/press-releases-latest`) are all premium too (verified live: HTTP 402 on the free tier); the one news surface on the free tier is **FMP Articles** (`fmp-articles`, verified live: HTTP 200 with `page`/`limit` paging honored) — FMP's in-house, ticker-tagged market commentary — which feeds the Step-7 news funnel as a best-effort supplementary source (see News and Research below).

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

The application uses FRED for macroeconomic analysis and long-term market-regime evaluation, and for the market-internal series — the dollar index, oil, and natural gas — that sit outside Financial Modeling Prep's free-tier coverage. It also supplies the risk- and cycle-oriented series that anchor the report's risk-posture and market-cycle reads: credit spreads (the aggregate high-yield and investment-grade OAS plus the BBB and single-B buckets for credit-quality dispersion), the equity volatility term structure (the S&P 3-month VIX, paired with the FMP VIX for a backwardation read, and Nasdaq-100 volatility), the 10y–3m and 10y–2y curve spreads, the Chicago Fed financial-conditions indices (NFCI, ANFCI) and the St. Louis stress index (STLFSI4), weekly initial and continued jobless claims, the Fed balance sheet, and the 30-year mortgage rate. (FRED's documented limit is 120 requests/minute with no daily cap; each run's ~40-request scan sits far under it.) It additionally supplies two **forward-looking expectation gauges** in the macro-levels group — the Atlanta Fed **GDPNow** current-quarter real-GDP nowcast (an annualized growth rate, a forward complement to the actual GDP print) and the Cleveland Fed **1-year expected-inflation** series (a model-based read alongside the market-implied breakevens). It also supplies the Step-3 **economic-release calendar** — the recent and upcoming US release schedule (CPI, PCE, jobs, GDP, …) via FRED's free release-dates API — since FMP's economic-calendar endpoint is premium-gated. (Like the earnings calendar, the recent-releases lookback is sized to the report cadence — a monthly run keeps the whole interval's releases — while the upcoming-schedule window stays fixed.) (FOMC meetings are excluded from the calendar — FRED has no scheduled-date series for them; the Fed's policy stance is carried by the Fed-funds target-range series instead.) FRED provides release dates (and the underlying series values, gathered separately), but not analyst-consensus estimates — the calendar carries release names and dates only. The "expected" consensus value is left to the agents' research-phase synthesis, where it bears on the thesis, rather than a market-data feed.

### BLS (Bureau of Labor Statistics)
Docs - https://www.bls.gov/developers/

BLS provides official United States labor and inflation datasets.

Responsibilities:
- CPI reports
- employment reports
- wage data
- labor-market statistics

The application uses BLS data for inflation and labor-market analysis.

### CFTC (Commitments of Traders)
Docs - https://publicreporting.cftc.gov/ (Socrata Open Data API)

The CFTC's weekly Commitments of Traders report supplies the one signal the price, valuation, macro, and credit groups can't: how crowded or extended the *speculative* cohort is in the market's bellwether futures. It is the application's positioning input, accessed through the CFTC public-reporting Socrata API, which is **keyless** — like BLS, it needs no credential and sits outside the execution gate.

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

GDELT's single combined query sizes its `timespan` lookback to the **elapsed interval since the previous report** (rounded up to whole days, clamped to a floor and a one-month cap), rather than a fixed week — keeping the geopolitical feed matched to the on-demand cadence. The first report (no prior interval) uses a one-week default. This changes only the window width, not the request count: it stays a single bounded query, so GDELT's burst rate limit is unaffected.

## Local Analysis Suite Sources

These sources serve the local analysis suite ([local-models.md](local-models.md)), not the Market Signal Report. The suite also reuses **FMP** (documented above) — as the candidate-screening source for Trade Opportunities (movers, valuation extremes, earnings signals) and for the company financials behind Portfolio Analysis.

### Charles Schwab (Trader API)
Docs - https://developer.schwab.com/

Charles Schwab is the source of the user's **portfolio holdings** for Portfolio Analysis — positions with quantity, cost basis, market value, and instrument identity, read through the Schwab Trader API over OAuth. It is holdings-only: Schwab's fundamentals are thin, so company financials come from FMP and SEC EDGAR (below). Authentication, token lifecycle, account hashing, and the manual-import fallback are described in [schwab-integration.md](schwab-integration.md).

### SEC EDGAR
Docs - https://www.sec.gov/edgar/sec-api-documentation

SEC EDGAR is the **authoritative source for company filings and fundamentals** behind Portfolio Analysis (and candidate validation in Trade Opportunities): 10-K / 10-Q / 8-K submissions and the XBRL **company-facts** API for normalized financial-statement data. It is **keyless** (like BLS and CFTC), requiring only a declared User-Agent with contact info per the SEC's fair-access policy, and is rate-limited to ~10 requests/second. SEC supplies the raw statement data the deterministic financial-analysis engine ([portfolio-analysis.md](portfolio-analysis.md)) computes over; FMP remains for convenient normalized metrics and market-data signals. Authoritative filings reduce the suite's dependence on web-search summaries for the numbers that drive grades and targets.

### SearXNG (local web search)
Docs - https://docs.searxng.org/

SearXNG is the local suite's **web-search backend** — a self-hosted, keyless metasearch instance queried over its JSON API on the loopback interface, fanning queries out to general engines. It is the primary search source for the suite's research loop, with the existing Tavily integration as a fallback. The search / fetch / extract tool built on it is described in [web-research.md](web-research.md).

## LLM Providers
- [OpenAI](https://platform.openai.com/docs)
- [Anthropic](https://platform.claude.com/docs)

The specific models exposed by these providers, the user-configurable model selections for each agent, and the API-token requirements are covered in [configuration.md](configuration.md). The non-configurable models used by fixed internal pipeline stages are covered in [agents.md](agents.md). The local analysis suite uses local open-weight models served on-device instead of these providers — see [local-models.md](local-models.md).
