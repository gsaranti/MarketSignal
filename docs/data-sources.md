# Data Sources

This file lists the external data and model providers the application depends on. Credential configuration for these providers is covered in [configuration.md](configuration.md).

The document is in **two parts**. [The data sources](#the-data-sources) catalogs every provider — what it is, why the app depends on it, and (for FMP) its plan-tier audit. [Endpoints by job](#endpoints-by-job) then lists, per job — **Market Signal Report**, **Portfolio Analysis**, **Trade Opportunities** — exactly which endpoints of each source that job calls, including the report's planned paid-FMP additions. The matrix in [Reading this document](#reading-this-document) is the at-a-glance index across both.

## Reading this document

The app runs three on-demand jobs, each assembling its own evidence from an overlapping set of providers:

- **Market Signal Report** — the macro / market thesis; cloud-model agents reason over a deterministic baseline scan ([report-workflow.md](report-workflow.md)).
- **Portfolio Analysis** — grades the user's Schwab holdings; local models reason over a per-holding evidence packet ([portfolio-analysis.md](portfolio-analysis.md)).
- **Trade Opportunities** — discovers new ideas across a 3×3 risk×horizon matrix; local models reason over a discovery → per-candidate funnel ([trade-opportunities.md](trade-opportunities.md)).

This matrix maps which job draws on which source; [The data sources](#the-data-sources) describes each provider, and [Endpoints by job](#endpoints-by-job) lists each job's exact endpoints.

| Source | Market Signal Report | Portfolio Analysis | Trade Opportunities |
| --- | --- | --- | --- |
| [FMP](#financial-modeling-prep) *(paid key)* | baseline equity-market scan | per-holding fundamentals / revision / positioning / segments / quote | discovery (screener-stratify + event feeds) **+** per-candidate validation |
| [FRED](#fred-federal-reserve-economic-data) | macro, rates, credit, dollar, oil / gas + release calendar | run-level risk-free + commodity context | run-level risk-free + commodity-cyclical sleeve |
| [BLS](#bls-bureau-of-labor-statistics) | CPI / payrolls / wages | — | — |
| [CFTC](#cftc-commitments-of-traders) | bellwether speculator positioning | fund-underlying positioning | commodity-cyclical positioning |
| [Tavily](#tavily) | **primary** news / research sweep | web-loop fallback | per-candidate web-loop fallback only (never discovery) |
| [FMP Articles](#fmp-articles) | Step-7 news funnel (free) | — | discovery theme / event scan |
| [GDELT](#gdelt) | geopolitical sweep | — | — *(dropped from the suite)* |
| [Charles Schwab](#charles-schwab-trader-api) | — | holdings + option chains **(required)** | owned / not-owned cross-ref + option chains |
| [SEC EDGAR](#sec-edgar) | — | authoritative cross-check (10-K/Q/8-K + XBRL) | authoritative cross-check + 8-K |
| [Stooq](#stooq) | — | deep price history + sector / market benchmarks | deep price history + sector / market benchmarks |
| [FINRA](#finra-short-interest) | — | short-interest risk / squeeze context | short-interest extremes screen + per-candidate |
| [CBOE](#cboe) | — | venue-level put/call backdrop | venue-level put/call backdrop |
| [SearXNG](#searxng-local-web-search) | — | **primary** web search (research loop) | **primary** discovery + per-candidate research |
| [OpenAI / Anthropic](#llm-providers) *(cloud)* | the four agent models + fixed internal stages | — | — |
| Local models *(Ollama — [local-models.md](local-models.md))* | — | all reasoning roles | all reasoning roles |

A cell names the job's use; **—** means the job doesn't use that source. "Primary" vs "fallback" is called out where a source plays different roles across jobs (Tavily, SearXNG). The report column spans **two distinct assemblies** — the deterministic **Step-3 baseline scan** and the **Step-7 news funnel** — named per cell, never blended. Rows mirror the catalog sections below, plus two intentional breakouts listed separately because they map to jobs differently — FMP's free **FMP Articles** news sub-feed, and the **model runtimes** (cloud agents for the report vs on-device local models for the suite). Plan-tier constraints on the shared FMP key are audited once for all jobs in [FMP — current paid-plan tier audit](#fmp--current-paid-plan-tier-audit).

## The data sources

This part catalogs every provider — what it is and why the app uses it. **Which** endpoints each job calls is in [Endpoints by job](#endpoints-by-job); plan-tier status lives here, under FMP, since it is a property of the source. FMP, FRED, and CFTC feed **all three jobs**; BLS, GDELT, and the cloud LLMs are report-only; Schwab, SEC EDGAR, Stooq, FINRA, CBOE, and SearXNG are local-suite-only.

The application accesses market and financial data by calling provider REST APIs directly from the Rust backend (`reqwest`/`serde`). **Financial Modeling Prep** is the primary financial-data source, supplying equity-market data — indices, volatility, gold and silver, sector performance, multi-horizon index returns, market movers, the earnings calendar, sector and industry valuation (P/E) plus finer-grained industry rotation, and the equity-risk-premium. **FRED** and **BLS** supply macroeconomic and labor data through their public APIs; FRED additionally provides the US dollar index, commodity prices (oil, natural gas), Treasury yields, credit and yield-curve spreads, and financial-conditions indices.

The gated REST adapters (FMP, FRED, BLS, Tavily) share a bounded retry-with-backoff for transient HTTP-status/transport failures (HTTP 429, 5xx, dropped connections; `Retry-After`-aware). GDELT is deliberately excluded — its escalating per-IP lockout makes a retry harmful, so it keeps its single-shot fail-soft.

Where retries don't recover, the Step-3 baseline scan degrades rather than aborting: an unresolved series or release (a rejected key, a sustained outage, or a malformed / empty response) is recorded as a gap in a missing-data manifest instead of failing the whole scan, and a central coverage floor then decides whether what resolved is sufficient to generate the report (see [report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data)).

### Financial Modeling Prep
Docs - https://site.financialmodelingprep.com/developer/docs

**Used by:** Report · Portfolio Analysis · Trade Opportunities — the data spine of all three. The endpoints each job calls are listed in [Endpoints by job](#endpoints-by-job); plan-tier status for every FMP path any job calls is in the [audit below](#fmp--current-paid-plan-tier-audit).

Financial Modeling Prep is the primary financial-data source for the application. All FMP paths are on the `https://financialmodelingprep.com/stable` base, on **one shared paid-tier key** used by the report and both local jobs (see [Local analysis suite — shared sourcing](#local-analysis-suite--shared-sourcing) for how the suite spreads its load).

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
- historical sector / industry P/E *and* performance (paid tier; trailing-window P/E percentile + cumulative-return trend vs own history — planned report enrichment, see [Endpoints by job](#endpoints-by-job))
- IPO calendar + M&A deal flow (paid tier; issuance / deal-making froth as a risk-appetite read — planned report enrichment, see [Endpoints by job](#endpoints-by-job))
- FMP Articles — in-house, ticker-tagged market commentary (free tier; feeds the Step-7 news funnel — see [FMP Articles](#fmp-articles) below)
- economic calendar — analyst consensus + realized surprise + Fed/FOMC event dates (paid tier; planned report enrichment, see [Endpoints by job](#endpoints-by-job))
- stock / general news feeds (premium tier only; the one free news surface is [FMP Articles](#fmp-articles))

#### FMP — current paid-plan tier audit

*Result of the #8 endpoint tier audit against the suite's actual paid FMP plan (verified 2026-06-26; `profile` added and live-verified 2026-07-10). It covers **every FMP endpoint the report and both local jobs call** — the per-job endpoint surfaces in [Endpoints by job](#endpoints-by-job) below (Market Signal Report, Portfolio Analysis, Trade Opportunities) — sorted into three buckets: **allowed (with constraint)**, **blocked → fallback**, **blocked → no fallback**. Net: the **report is fully covered**; **Portfolio equity grading is clean**; **Portfolio funds degrade** (ETF constituent look-through lost; mutual-fund **constituent holdings** have no on-plan FMP path, while fund information and available allocation data remain usable); **Trade Opportunities needed a discovery-funnel redesign** — every `*-bulk` endpoint is blocked (since done: the discovery layer below is redesigned around `company-screener`-stratified discovery + per-symbol scoring).*

**Allowed (with constraint).** Every US-exchange limit is moot — the suite analyzes US listings only, **enforced rather than assumed**: a Schwab account can hold foreign ordinaries and other non-US listings, and the Portfolio eligibility contract's listing-resolution guard routes those to not-rated before they reach these endpoints ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)); the report side never leaves US aggregates.

| Constraint | Endpoints |
| --- | --- |
| US exchanges only | `quote`, `historical-price-eod/light`, `income-statement`, `balance-sheet-statement`, `cash-flow-statement`, `income-statement-ttm`, `ratios-ttm`, `key-metrics-ttm`, `financial-scores`, `owner-earnings`, `enterprise-values`, `discounted-cash-flow`, `financial-growth`, `price-target-consensus`, `price-target-summary`, `grades`, `grades-historical`, `grades-consensus`, `earnings`, `dividends`, `key-executives` |
| US exchanges + **annual periods only** | `key-metrics`, `ratios`, `analyst-estimates` (also **≤10 responses/call**) |
| Annual periods only | `revenue-product-segmentation`, `revenue-geographic-segmentation` |
| Exchange set — NASDAQ / NYSE / AMEX / CBOE / OTC / PNK / CNQ | `sector-performance-snapshot`, `sector-pe-snapshot`, `industry-performance-snapshot`, `industry-pe-snapshot`, `historical-sector-pe`, `historical-industry-pe`, `historical-sector-performance`, `historical-industry-performance`, `company-screener` |
| History ≤ 1 year | `earnings-calendar`, `economic-calendar`, `news/stock` |
| None (available outright) | `profile`, `biggest-gainers`, `biggest-losers`, `most-actives`, `market-risk-premium`, `fmp-articles`, `ipos-calendar`, `mergers-acquisitions-latest`, `etf/info`, `etf/sector-weightings`, `etf/country-weightings`, `insider-trading/search`, `insider-trading/statistics`, `insider-trading/latest`, `acquisition-of-beneficial-ownership`, `senate-trades`, `house-trades`, `ratings-snapshot`, `ratings-historical`, `stock-peers`, `shares-float`, `shares-float-all`, `historical-employee-count`, `available-sectors`, `industry-classification-search`, `all-industry-classification`, `news/general-latest`, `news/stock-latest`, `sec-filings-8k` |

The annual-only limit on `key-metrics` / `ratios` is absorbed by reading trailing ratios from `ratios-ttm` (allowed) or deriving them in the engine from the quarterly base statements; `analyst-estimates` at annual cadence + ≤10/call covers a single name's forward years.

**Blocked → fallback.**

| Blocked endpoint(s) | Fallback |
| --- | --- |
| **All bulk** — `scores-bulk`, `earnings-surprises-bulk`, `ratios-ttm-bulk`, `key-metrics-ttm-bulk`, `rating-bulk`, `upgrades-downgrades-consensus-bulk`, `price-target-summary-bulk`, `income-statement-growth-bulk`, `cash-flow-statement-growth-bulk`, `dcf-bulk` | `company-screener` does the universe-wide first cut; the multi-factor composite (forensic / surprise / rating-flow / growth / DCF-gap) is computed **per-symbol on the screener-narrowed longlist** (see the Trade Opportunities discovery funnel in [trade-opportunities.md](trade-opportunities.md)). The discovery-breadth budget governs how many names reach per-symbol scoring. |
| `balance-sheet-statement-ttm`, `cash-flow-statement-ttm` | Engine computes TTM from 4 quarters of the (allowed) base statement endpoint, or from SEC EDGAR. |
| `earning-call-transcript`, `earning-call-transcript-dates`, `earnings-transcript-list` | Web-research loop ([web-research.md](web-research.md)) — transcripts are public on IR / aggregator sites. |
| `institutional-ownership/symbol-positions-summary` | SEC EDGAR 13F (coarse) or omit — the institutional-flow leg is held out of the grade until calibrated, so dropping it is low-cost. Insider / activist (13D/G) / congressional positioning survive (allowed above). |
| `news/press-releases` (symbol-scoped) | `news/stock` (allowed, ≤1yr) + `sec-filings-8k` + web-research. |
| `news/press-releases-latest` (market-wide) | `news/general-latest` + `news/stock-latest` + `sec-filings-8k` (all allowed). |
| `mergers-acquisitions-search` (per-symbol) | `mergers-acquisitions-latest` (market-wide, allowed) + `sec-filings-8k`. |
| `etf/holdings` | ETF exposure tilt from `etf/sector-weightings` + `etf/country-weightings` (allowed); single-name constituent concentration is dropped (or sourced from SEC N-PORT, heavy) — see the Portfolio fund-path degrade in [portfolio-analysis.md](portfolio-analysis.md). |
| `funds/disclosure-holders-latest`, `funds/disclosure`, `funds/disclosure-dates` | SEC N-PORT filings (heavy, ~60-day lag) for constituent detail; without it, a mutual fund keeps the on-plan `etf/info`, any returned country / sector allocation, quote / NAV, profile, and Schwab position data, then routes to `role_risk_only` when the usable weighting set is insufficient for exposure pricing. |

**Blocked → no fallback (capability loss).**

| Blocked endpoint | Lost capability |
| --- | --- |
| `institutional-ownership/extract-analytics/holder` | Holder-level 13F deltas (per-institution share / weight Δ, `isNew` / `isSoldOut`, average price paid). EDGAR 13F is filer-keyed, so per-stock holder reconstruction is impractical — the holder-level smart-money read is omitted (the coarse summary's EDGAR fallback above is the most that's recoverable). |
| `etf/asset-exposure` | Reverse "which ETFs hold this name" lookup — no keyless substitute. Was already an optional cross-check (not the look-through source), so it is dropped outright. |

### FRED (Federal Reserve Economic Data)
Docs - https://fred.stlouisfed.org/docs/api/fred/

**Used by:** Report (macro / rates / credit backbone) · Portfolio Analysis (run-level risk-free + commodity context) · Trade Opportunities (run-level risk-free + commodity-cyclical sleeve). Each job's series subset is listed in [Endpoints by job](#endpoints-by-job).

FRED provides official macroeconomic and financial data maintained by the Federal Reserve Bank of St. Louis. Base `https://api.stlouisfed.org/fred`; two endpoints — `/series/observations` (the latest level of each series — the `series_id` doubles as the quote symbol) and `/release/dates` (the economic-release calendar schedule). FRED's documented limit is 120 requests/minute with no daily cap; each report run's ~40-request scan sits far under it.

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

### BLS (Bureau of Labor Statistics)
Docs - https://www.bls.gov/developers/

**Used by:** Market Signal Report only (CPI / payrolls / wages). Neither local job calls BLS.

BLS provides official United States labor and inflation datasets, over a single endpoint `/timeseries/data/` (base `https://api.bls.gov/publicAPI/v2`; series IDs posted in the request body, the `series_id` doubling as the quote symbol).

Responsibilities:
- CPI reports
- employment reports
- wage data
- labor-market statistics

The application uses BLS data for inflation and labor-market analysis.

### CFTC (Commitments of Traders)
Docs - https://publicreporting.cftc.gov/ (Socrata Open Data API)

**Used by:** Report (bellwether positioning) · Portfolio Analysis (fund-underlying positioning) · Trade Opportunities (commodity-cyclical positioning) — the same keyless weekly pull, read per job; each job's contract mapping is in [Endpoints by job](#endpoints-by-job).

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

### Tavily
Docs - https://docs.tavily.com/welcome

**Used by:** Report (**primary** news / research sweep) · local suite (web-loop **fallback** only — never a discovery dependency; [web-research.md](web-research.md)).

Tavily provides AI-oriented web search and research retrieval.

Responsibilities:
- gathering relevant market news
- retrieving research sources
- identifying important developing stories
- supplying contextual research material to the agents

The application uses Tavily as the **Market Signal Report's** primary research and news-ingestion system; for the local suite it is only the keyless [SearXNG](#searxng-local-web-search) web tool's fallback, never the primary.

**Endpoint** — `https://api.tavily.com/search` (the Search API; `/usage` backs the connection test).

Because reports are generated on demand (no fixed cadence — see [scheduling.md §Generating a Report](scheduling.md#generating-a-report)), the Step-7 news sweep sizes Tavily's recency bound to the **elapsed interval since the previous report**: it sends a `start_date` of today minus the elapsed days (clamped to a floor and a one-month cap), so a daily run isn't fed a stale week and a monthly run isn't starved of coverage. (`start_date`/`end_date` are the documented Tavily Search recency parameters; the former `days` field is no longer part of the API.) The first report (no prior interval) omits the bound and takes Tavily's own default. The Step-9 research executor's plan queries carry no recency bound — they target a topic, not a time slice.

### FMP Articles
Docs - https://site.financialmodelingprep.com/developer/docs (Articles)

**Used by:** Report (Step-7 news funnel) · Trade Opportunities (keyless discovery theme / event scan). Not Portfolio Analysis. Reuses the shared FMP key.

FMP Articles is Financial Modeling Prep's in-house editorial feed: short, ticker-tagged market commentary written against FMP's own data (analyst-coverage moves, earnings reactions, notable price action). It is the one FMP news surface on the free tier (verified live; the third-party `news/*` feeds are premium — see [Financial Modeling Prep](#financial-modeling-prep) above), and reuses the FMP API key already required for market data.

Responsibilities:
- supplementary company-level market headlines for the Step-7 news funnel
- ticker-tagged story metadata (each article carries an exchange-prefixed ticker)
- a free resilience hedge alongside Tavily (quota-limited) and GDELT (lockout-prone)

The application gathers a single bounded page of recent articles per run, alongside Tavily's topic sweep and GDELT's geopolitical query. The feed is best-effort: it is house-written and overlaps what Tavily already indexes, so the deterministic dedup pre-pass and the headline-filter model absorb redundancy, and a failing gather degrades to no headlines rather than failing the job — the same fail-soft posture as GDELT.

### GDELT
Docs - https://www.gdeltproject.org/data.html

**Used by:** Market Signal Report only (geopolitical sweep). **Dropped from the local suite** — its escalating IP-lockout makes it unreliable (the same reason it stays single-shot fail-soft here; see the [Trade Opportunities endpoint surface](#trade-opportunities--endpoint-surface)).

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

### SearXNG (local web search)
Docs - https://docs.searxng.org/

**Used by:** Portfolio Analysis · Trade Opportunities (local suite only) — the suite's **primary** web search; [Tavily](#tavily) above is its fallback. Not the report.

SearXNG is the local suite's **web-search backend** — a self-hosted, keyless metasearch instance queried over its JSON API on the loopback interface, fanning queries out to general engines. It is the primary search source for the suite's research loop, with the existing Tavily integration as a fallback. The search / fetch / extract tool built on it is described in [web-research.md](web-research.md).

### Charles Schwab (Trader API)
Docs - https://developer.schwab.com/

**Used by:** Portfolio Analysis · Trade Opportunities (local suite only) — **a connected Schwab account is required to run either local job**. Not the report.

Charles Schwab is the source of the user's **portfolio holdings** *and* **equity option chains** for the local suite — positions (quantity, cost basis, market value, instrument identity) via the accounts endpoint, and live option chains via the same OAuth's market-data endpoint (`/marketdata/v1/chains`, returning per-contract volume, open interest, implied volatility, and greeks). It is not a fundamentals source — Schwab's fundamentals are thin, so company financials come from FMP and SEC EDGAR (below). From the chains the suite computes a deterministic **options-activity signal** — put/call ratio (by volume and open interest) plus an IV/skew read (a rough activity proxy, not positioning truth — see [schwab-integration.md](schwab-integration.md)). A connected Schwab account is **required to run either local job**; authentication, token lifecycle, account hashing, and the manual-import supplement are described in [schwab-integration.md](schwab-integration.md).

### SEC EDGAR
Docs - https://www.sec.gov/edgar/sec-api-documentation

**Used by:** Portfolio Analysis · Trade Opportunities (local suite only) — the authoritative cross-check behind the FMP working feed. Not the report.

SEC EDGAR is the **authoritative source for company filings and fundamentals** behind Portfolio Analysis (and candidate validation in Trade Opportunities): 10-K / 10-Q / 8-K submissions and the XBRL **company-facts** API for normalized financial-statement data. It is **keyless** (like BLS and CFTC), requiring only a declared User-Agent with contact info per the SEC's fair-access policy, and is rate-limited to ~10 requests/second. SEC supplies the raw statement data the deterministic financial-analysis engine ([portfolio-analysis.md](portfolio-analysis.md)) computes over; FMP remains for convenient normalized metrics and market-data signals. Authoritative filings reduce the suite's dependence on web-search summaries for the numbers that drive grades and targets.

For Trade Opportunities, EDGAR is the **authoritative cross-check** behind the FMP working feed — the final grade / target numbers are reconciled against EDGAR filings and XBRL (compute-don't-guess), and 8-K filings remain a primary source. The symbol-keyed **insider and congressional** positioning signals come from FMP's structured endpoints rather than EDGAR's CIK-keyed filings, so **ticker→CIK resolution** (SEC's `company_tickers.json`; today only a hardcoded handful resolve) is a **non-blocking enhancement** — needed only to extend the SEC cross-check to arbitrary names — not a hard prerequisite. **Institutional 13F** is the exception: it is off-plan on the current FMP plan ([§FMP — current paid-plan tier audit](#fmp--current-paid-plan-tier-audit)), and because EDGAR 13F is filer-keyed, a per-name read is only coarse and optional (often omitted) rather than a clean symbol-keyed call.

### FINRA (short interest)
Docs - https://www.finra.org/finra-data/browse-catalog/equity-short-interest

**Used by:** Portfolio Analysis (held-equity risk / squeeze context) · Trade Opportunities (short-interest extremes screen + per-candidate). Local suite only — not the report.

FINRA publishes the **consolidated Equity Short Interest** file — keyless, biweekly (mid- and end-of-month settlement, disseminated ~7–8 business days later), covering exchange-listed and OTC equities since 2021. Each record carries current and prior short interest plus average daily volume, giving short-interest level, trend, and days-to-cover directly — a **bearish-by-default** factor for Trade Opportunities (heavily-shorted names underperform on average), flipping to a *conditional squeeze* read only when paired with an inflecting leading metric + near-term catalyst + a breaking bear thesis ([trade-opportunities.md §The research method](trade-opportunities.md#the-research-method)), **and a risk / squeeze-context input for Portfolio Analysis's held equities** (informing trims and adds; the biweekly file is fetched once per run and looked up per holding). It is best-effort and fail-soft like the suite's other additive feeds.

### Stooq
Docs - https://stooq.com/db/h/

**Used by:** Portfolio Analysis · Trade Opportunities (local suite only) — deep price history the engine's price-action / momentum / volatility reads need (plus sector / market benchmarks for both jobs' outcome learning and Portfolio's technology-event pre-flag). Not the report.

Stooq is the suite's **deep historical price source** — keyless, with no documented rate cap, serving 20–30+ years of daily OHLCV per symbol via a simple CSV endpoint (plus bulk database downloads). Its value holds **independent of FMP's tier**: the multi-decade depth is what the engine's **price-action confirmer** (multi-year base-breakout / long relative-strength) and the momentum / volatility / price-target computations need, and serving that highest-volume per-holding read keylessly keeps the bulk price load off the shared FMP key regardless of cap. Like GDELT it is best-effort and fail-soft: an informal source with no SLA and occasional symbol-mapping gaps (US tickers take a `.us` suffix), so a missing series degrades to a gap rather than failing the run, and the adapter self-throttles to stay polite.

### CBOE
Docs - https://www.cboe.com/us/options/market_statistics/daily/

**Used by:** Portfolio Analysis · Trade Opportunities (local suite only) — a venue-level options-sentiment backdrop. Not the report.

CBOE provides a free, keyless daily **put/call ratio for its own exchange's flow** (total, equity, index) — a Cboe **venue-level** options-sentiment backdrop (not consolidated all-market data, which is a separate paid dataset), distinct from the per-stock put/call the suite computes from Schwab chains. It is an optional broad-market context signal read from Cboe's daily statistics, fail-soft and venue-level only (no per-symbol breakdown), so it informs the macro sentiment read rather than any single verdict.

### LLM Providers
- [OpenAI](https://platform.openai.com/docs)
- [Anthropic](https://platform.claude.com/docs)

**Used by:** Market Signal Report only (the four user-configurable agent models + the fixed internal stages). The **local analysis suite runs entirely on local open-weight models** served on-device ([local-models.md](local-models.md)), so neither local job calls these providers.

The specific models exposed by these providers, the user-configurable model selections for each agent, and the API-token requirements are covered in [configuration.md](configuration.md). The non-configurable models used by fixed internal pipeline stages are covered in [agents.md](agents.md). The local analysis suite uses local open-weight models served on-device instead of these providers — see [local-models.md](local-models.md).

## Source registry and evidence tiers

The web-research loop ([web-research.md](web-research.md)) reaches an open, unbounded set of domains, so the suite governs *which* sources it trusts for *what* through a **source registry** — structured per-domain metadata the orchestrator consults at search-rank and fetch time. The registry is **not a whitelist and not exhaustive**: it is a **thin override over heuristic defaults** — the long tail of the web is never registered and is scored by default rules, while the registry pins the handful of domains whose treatment must be deliberate (primary sources, known specialists, denied junk, connected subscriptions).

**Per-domain metadata.** Each registered domain carries: **`tier`** (its evidence tier, below); **`evidenceKinds`** — *what the source is good for*, not one global score (`["filings","financials","legal-disclosure"]` for SEC, `["event-verification"]` for a wire, `["semis-supply-chain"]` for SemiAnalysis) — so a source can be Tier-0 for one kind and weak for another (SEC is authoritative for reported financials, useless for market narrative), and weighting is **per evidence-kind**, never one reputation number; **`lanePolicy`** — how the source is treated per research lane ([web-research.md §Source quality and evidence weighting](web-research.md#source-quality-and-evidence-weighting)); **`credentialMode`** — `none` (public) or `connected` (an authenticated subscription — [web-research.md §Connected sources](web-research.md#connected-sources-authenticated-fetch)); **`freshnessSlaDays`** — how stale a hit may be before it is down-weighted for this kind (long for a filing, short for an ASP read); **`extractionProfile`** — how the fetch layer should treat the domain (`api_or_html` / `html` / `js_required`), seeded by extraction telemetry; and **`paywall`** — whether full text depends on a connected subscription.

**Evidence tiers.** Every source resolves to a tier — registered explicitly, or by default heuristic:
- **Tier 0 — primary / deterministic:** SEC EDGAR, company IR + earnings releases, 10-K/Q/8-K, the Fed / FRED / BLS / BEA / EIA / FDA / CFTC / FINRA, court & regulator documents. The compute-don't-guess anchor.
- **Tier 1 — licensed structured providers:** FMP, Schwab, and a connected Morningstar where subscribed and extraction works.
- **Tier 2 — high-trust factual reporting:** Reuters, AP, WSJ, FT, Bloomberg, Barron's, The Economist.
- **Tier 3 — specialist industry sources:** SemiAnalysis, TechInsights, TrendForce, DigiTimes, STAT, Endpoints, FierceBiotech, S&P Platts, Argus, Wood Mackenzie, FreightWaves, Benchmark Mineral Intelligence — high trust *within their vertical* (the `evidenceKinds` constraint), noise outside it.
- **Tier 4 — useful but opinion-heavy:** Seeking Alpha, Motley Fool transcripts, Substacks, analyst / trade blogs.
- **Tier 5 — sentiment / noise only:** Reddit, X, StockTwits, YouTube, forums — read as sentiment signal, never as fact.
- **`deny`:** SEO stock mills, AI-generated quote pages, thin affiliate sites, press-release-repost spam — dropped at search-filter and fetch-gate. This is the **one categorical exclusion**, distinct from the tier gradient (0–5), which only *weights* evidence and never drops it — denying junk that isn't evidence at all is not gating on quality ([web-research.md §Source quality and evidence weighting](web-research.md#source-quality-and-evidence-weighting)).

The tiers are seeded with a **representative starter set, not a maintained catalog of the whole web**: an unregistered domain is scored by default heuristic (a recognized `.gov` / primary-filing host defaults high, an unknown blog low), so the system degrades gracefully rather than depending on registry completeness. How tiers and evidence-kinds translate into ranking and conviction — and the load-bearing rule that they **inform, never gate** — is in [web-research.md §Source quality and evidence weighting](web-research.md#source-quality-and-evidence-weighting).

## Endpoints by job

This part lists, per job, which endpoints of each source the job calls — the **Market Signal Report** first (including its **planned** paid-FMP additions, flagged *not yet wired*), then the two local jobs (under a shared sourcing note). Plan-tier status for the FMP paths is in [FMP — current paid-plan tier audit](#fmp--current-paid-plan-tier-audit); **cardinality** (how a call scales per run) is tagged in the local-job tables.

### Market Signal Report — endpoint surface

The report's deterministic Step-3 baseline scan ([report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data)), by source — FMP, FRED, BLS, CFTC, and the news funnel; the paid-tier FMP additions are the **planned report enrichment** at the end of this section.

**FMP — endpoints used by the report** — all on the `https://financialmodelingprep.com/stable` base. The free-tier paths are wired today; the paid-tier paths are the **planned report enrichment** specified below (not yet wired).

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

The application calls Financial Modeling Prep directly for the equity-market portion of the baseline market-data scan ([report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data)) — indices, volatility, gold and silver, and sector performance; each index's multi-horizon performance (weekly, month-to-date, year-to-date, and 52-week-range position) derived from FMP's free historical end-of-day prices (verified live: the indices and the VIX return on the free tier); the **market movers** (biggest gainers / losers / most-active names, filtered to major-exchange names above a price floor, with leveraged / inverse ETFs excluded, capped per list); the free **earnings calendar** (the recently reported and upcoming large-cap reporters, on FMP's ~1-month free window, filtered by revenue estimate — the recently-reported lookback is sized to the report cadence, so a monthly run sees the whole interval's reporters rather than just the last week, while the upcoming window stays fixed); and the free **valuation + finer-rotation** snapshots — per-**sector P/E** (a valuation read alongside sector performance), the strongest and weakest **industries** (FMP reports ~130 per exchange, capped to the extremes; each industry's average move joined with its aggregate P/E where available), and the US **equity-risk-premium** (from FMP's per-country market-risk-premium dataset, a near-static annual constant). The mover lists carry no sector, no instrument type, and no index membership on the free tier, so the agent infers a mover's sector from its ticker (and treats any fund row that slips the name filter as a flow signal, not a company), and the earnings calendar is filtered by revenue magnitude rather than index membership. The sector / industry snapshots and the market-risk-premium are all on FMP's free tier (verified live); the per-sector and per-industry snapshots are date-keyed (the adapter walks back to the most recent trading day with data, like sector performance), and the industry valuation is a join of the industry-performance and industry-P/E snapshots by industry name. **These valuation snapshots are exchange-specific** (verified live: a no-`exchange` call defaults to NASDAQ only; NYSE and AMEX are also free), so the adapter pins and gathers **both NASDAQ (growth / tech-tilted) and NYSE (broader / value)** for each, tags every row with its exchange, applies the industry cap per exchange, and joins performance to P/E within a single exchange — the model reads these cross-sectionally (rich vs cheap, and growth-board vs value-board) rather than as a whole-market multiple. (On the paid tier this point-in-time read gains a trailing-window time dimension — current multiple *and* current return vs each group's own history — see **Planned report enrichment** below.) The scan's dollar-index, oil, natural-gas, and Treasury-yield series come from FRED (below). (Gold is on FMP's free tier via `GCUSD`; FRED's former free gold benchmark series were discontinued, so gold stays on FMP.) The **economic-release calendar**'s release schedule comes from FRED's free release-dates endpoint (below) rather than FMP, whose `economic-calendar` is premium-gated (verified live: HTTP 402 on the free tier); on the paid tier that endpoint becomes available to *layer analyst consensus, realized surprise, and Fed/FOMC dates onto* FRED's schedule (see **Planned report enrichment** below). FMP's third-party **news feeds** (`news/general-latest`, `news/stock-latest`, symbol-scoped `news/stock`, `news/press-releases-latest`) are all premium too (verified live: HTTP 402 on the free tier); the one news surface on the free tier is **FMP Articles** (`fmp-articles`, verified live: HTTP 200 with `page`/`limit` paging honored) — FMP's in-house, ticker-tagged market commentary — which feeds the Step-7 news funnel as a best-effort supplementary source (see [FMP Articles](#fmp-articles)).

**FRED — endpoints + series.** Base `https://api.stlouisfed.org/fred`. Two endpoints: `/series/observations` (latest level of each series below — the `series_id` doubles as the quote symbol) and `/release/dates` (the economic-release calendar schedule). The report's FRED series, by Step-3 baseline group:

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

**BLS — endpoints + series.** Base `https://api.bls.gov/publicAPI/v2`, single endpoint `/timeseries/data/` (series IDs posted in the request body; the `series_id` doubles as the quote symbol):

| Series ID | Series | Unit |
| --- | --- | --- |
| `CUUR0000SA0` | Consumer Price Index (CPI-U, All Items, NSA) | index (1982-84=100) |
| `LNS14000000` | Unemployment Rate (U-3) | percent |
| `CES0000000001` | Total Nonfarm Payrolls | thousands of persons |
| `CES0500000003` | Average Hourly Earnings, Total Private | USD per hour |

**CFTC.** The report reads both CFTC datasets' curated bellwether contracts at run level — the full contract set, the two dataset formats, the weekly cadence, the three-week freshness guard, and the change-view exemption are in the [CFTC catalog entry](#cftc-commitments-of-traders).

**News & research** — the report's Step-7 news funnel (recency / cadence mechanics in each source's catalog entry):

| Source · endpoint | Report use |
| --- | --- |
| [Tavily](#tavily) · `/search` | **primary** news / research sweep (recency-bounded to the elapsed interval since the prior report) |
| [FMP Articles](#fmp-articles) · `/fmp-articles` | free supplementary ticker-tagged commentary, deduped into the funnel |
| [GDELT](#gdelt) · `/doc` | geopolitical / event sweep (single combined query, cadence-sized `timespan`) |

#### Planned report enrichment (paid FMP tier)

**Used by:** Market Signal Report only — planned, paid-tier, not yet wired.

Upgrading the shared FMP credential to the paid tier (the one paid dependency the local analysis suite already requires — see [Local analysis suite — shared sourcing](#local-analysis-suite--shared-sourcing) below) unlocks three report-side baseline enrichments. Each is an **opt-in addition to the existing scan, not a replacement**: the report's current data-source logic is unchanged, each enrichment soft-degrades to today's behavior on any failure, and all are paid-gated, so they are live-verified together with the suite's paid-key checkpoint.

**Economic-calendar consensus + surprise.** FRED stays the release-schedule backbone; FMP's paid `economic-calendar` (`?country=US&from=&to=`, fields `event` / `date` / `impact` / `previous` / `estimate` / `actual`) layers on two things the FRED schedule can't carry. (1) For the report's tracked market-moving releases, the engine joins FMP's `estimate` / `actual` onto the matching FRED release through a **curated release→event map** — FRED release names ("Employment Situation") and FMP event names ("Non Farm Payrolls") don't string-match, and one release fans out to several FMP events. The map is the **versioned canonical table** that makes the classification codeable: each row carries the release identity (the FRED release ↔ its FMP event names), the **unit normalization** between the two feeds, a **date-match tolerance** (same calendar date, ±1 day for cross-midnight publishes — drafted), a **favorable polarity** (`higher-favorable` / `lower-favorable` / `none` — above-consensus payrolls are favorable; above-consensus CPI is not a "beat"), and an **in-line epsilon** in the release's own units. From a matched row the engine computes a deterministic, **neutral relation tag** — `above` / `in-line` / `below` estimate (`in-line` when |actual − estimate| ≤ the row's epsilon) — plus the signed actual-vs-estimate **% gap** (`(actual − estimate) / |estimate|`, **`None` when the estimate is zero** — the absolute gap is always carried), and a **beat / miss** read **only where the row maps a polarity** (beat = the favorable side): the engine derives, the model interprets. (2) FMP-only **Fed / FOMC events** (filtered to Medium/High `impact`), which FRED has no scheduled-date series for, are appended as a distinct event class, closing the documented FOMC-date gap. Fail-soft throughout: an unmapped release, a `null` estimate, or an FMP outage leaves that release at **names + dates only** (today's behavior) — never a fabricated consensus.

**Historical sector / industry valuation + performance.** Today the sector/industry P/E *and* performance are both point-in-time, read only cross-sectionally (which group is rich vs cheap, strong vs weak *right now*). Four paid endpoints — `historical-sector-pe` / `historical-industry-pe` and `historical-sector-performance` / `historical-industry-performance`, each keyed by sector/industry + `exchange`, with `from` / `to` — add a **time dimension** over a fixed trailing ~1 year, fetched for all 11 sectors × both exchanges and for the **extreme industries the snapshot already surfaces** (not all ~130). The two series are shaped differently, so the engine derives a different read from each:
- the P/E endpoints return a `pe` **level**, so the engine takes its **percentile within the trailing window + a min / median / max band** — rich/cheap against its own history;
- the performance endpoints return a daily **`averageChange`** (that date's average constituent move), *not* a price level, so the engine **compounds the daily changes into a trailing cumulative return** (`Π(1 + averageChange) − 1`) — the rising / falling read — rather than percentiling the raw daily moves (a percentile of `averageChange` would say "today's move was unusual," not "the group is up over the window").

Both derivations run under one **single-homed rule set** (the workflow points here rather than restating it): rows are sorted date-ascending and **deduplicated by date** (the last row for a date wins); a null / non-finite value is dropped, and a **P/E ≤ 0 is dropped as inadmissible** against-own-history; the **percentile is the inclusive rank** — the share of the window's valid values ≤ the current value, reported 0–100 (ties count as ≤); and a read requires a **minimum sample of ≥ 60 valid points** with the **newest valid point ≤ 7 days old** (both drafted) — below either, the read is **absent plus a typed partial-data gap**, never a numeric default.

Both are compact derived numbers, not the raw series. Paired, they let the model read a re-rating *with* its price context: a group cheap against its own P/E history *and* up over the trailing window (a re-rating turn) reads differently from one cheap *and still down* (a possible value trap) — the distinction a single snapshot can't support.

**IPO / M&A froth.** The report has no primary-market feed today, so it can't see the issuance / deal-making pace that runs hot late-cycle and freezes under stress. Two paid endpoints add it — `ipos-calendar` (`?from=&to=`, recently priced + upcoming scheduled offerings) and `mergers-acquisitions-latest` (`?page=&limit=`, recently announced deals) — which the engine reduces to a compact **activity read**: the recent-window IPO count and the recent-window M&A deal count — **each recent count paired with its prior equivalent-window count** so the engine carries a native rising / cooling **trend** (the way CFTC positioning carries its own week-over-week change while staying out of the level-delta engine — the model is handed only the current packet plus the computed change view, never prior raw packets, so the trend can't come from the delta engine and must be self-contained) — plus the **upcoming-scheduled IPO count**, a standalone forward-looking datum with **no trend pair** (a prior "upcoming" window isn't reconstructable from the current feed — postponed or since-priced offerings vanish from the schedule, so a pair here couldn't be self-contained) — and a bounded list of the largest / most notable names (and aggregate proceeds / deal value where the feed carries it) for color. Like the earnings and economic-release calendars, the recent window is sized to the report cadence (a monthly run sees the month's froth, not a week's), clamped to **7–31 days** (the earnings calendar's own floor / cap convention; a first run with no cadence anchor defaults to the floor); windows are **half-open** on the feeds' calendar dates in the user's local timezone (the report's date convention) — the recent window ending at the report date (exclusive), the **prior window the contiguous equal-length window immediately before it** — while the upcoming-IPO window is fixed at **14 days** forward (the earnings calendar's forward-window convention). The date-ranged `ipos-calendar` covers the recent and prior windows in one bounded call; `mergers-acquisitions-latest` is paged, not date-ranged, so the adapter walks pages back toward the prior-window boundary under a **drafted page budget of ≤ 10 pages at `limit=100`** (comfortably ≥ two months of deals), the **notable-name list capped at 5 per feed** (drafted). Coverage resolves to one of **three completion states**: pages reached the prior boundary → both counts; pages covered only the recent boundary → the recent count with the prior count **absent** (the trend degrades to the recent count alone — never a fabricated prior); pages failed to cover even the recent window → **no M&A count and a typed partial-data gap**, so a partial count can never serialize as complete. The model reads the pace *and its trend* as a risk-appetite / late-cycle tell — a surge or accelerating pace feeding the risk-on / late-cycle read, a freeze the risk-off / stress read. Fail-soft and non-floor: a failed gather degrades to no froth signal, never a failed run.

A fourth candidate — **true index breadth** (advance/decline, participation) — was considered and **ruled out**: FMP exposes no breadth metric, so the movers group stays the report's breadth proxy.

All three enrichments follow the same three structural rules so they neither bloat storage nor disturb the report-to-report change view:

- **Persist the derived read, not the raw history.** Only the compact derived numbers — the calendar's estimate / actual / relation tag / signed **% gap** (and the polarity-mapped beat / miss read where defined), the P/E percentile + band, the performance trailing cumulative return, and the froth reads (the IPO and M&A **recent + prior-window counts**, the **upcoming-scheduled IPO count**, the bounded notable-name list, and the aggregate proceeds / deal value where the feed carried it) — ride into the packet and the persisted baseline snapshot; the raw ~250-point P/E and performance series and the full IPO / deal lists are transient fetch input, discarded once the derived reads are computed. **Every optional derived value has explicit absent semantics** — absent means *not derivable* (an unmapped release, a `null` or zero estimate, an exhausted page budget), **never zero** — so a prompt or snapshot comparison can always distinguish missing from zero.
- **New fields carry `#[serde(default)]`.** The enrichments add fields to `BaselineMarketData` and its member structs — `EconomicRelease` (calendar consensus/surprise), `SectorPe` and `IndustrySnapshot` (valuation-history context), `SectorPerformance` and `IndustrySnapshot` (performance-history context) — plus a new issuance-activity group; each must default so an *older* snapshot — serialized before the field existed — still deserializes (to empty / `None`), keeping the prior-vs-current comparison backward-compatible. (The prior-snapshot decode is already fail-soft regardless — see [report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data).)
- **All stay out of the level-delta engine.** The surprise is a native actual-vs-consensus value, the P/E percentile + band and the performance trailing cumulative return are trailing-window structural reads, and the IPO / M&A counts are set-valued activity tallies — none is an inter-report level change — so, like CFTC positioning and the existing movers / earnings / calendar groups, none joins the diffed level groups; the existing point-in-time `sector_pe` level-diff is untouched (it reads only the current `pe`), and the `sectors` performance group stays excluded from the diff exactly as it is today.

The prompt-side changes these require are specified in [report-workflow.md §Step 16](report-workflow.md#step-16-main-agent-synthesis): the data rides into every **baseline-carrying generative prompt** — the research router (Step 8), the three analysts (Step 12), and the main synthesis (Step 16) — automatically via JSON serialization (the headline filter and the embedding calls carry no baseline payload), but the interpretive prose must be updated in lockstep — including one existing main-agent instruction that currently tells the model to *ignore* multiple-expansion-over-time, which the P/E history now supports.

### Local analysis suite — shared sourcing

**Used by:** Portfolio Analysis · Trade Opportunities ([local-models.md](local-models.md)), not the Market Signal Report. Both suite jobs share the sourcing strategy below; their per-endpoint tables follow. Beyond the suite-only [Charles Schwab](#charles-schwab-trader-api) / [SEC EDGAR](#sec-edgar) / [FINRA](#finra-short-interest) / [Stooq](#stooq) / [CBOE](#cboe) / [SearXNG](#searxng-local-web-search) sources, the suite also draws on the shared [FMP](#financial-modeling-prep) / [FRED](#fred-federal-reserve-economic-data) / [CFTC](#cftc-commitments-of-traders) catalog entries — the [matrix](#reading-this-document) maps the whole set.

Even on the paid FMP key, the suite **spreads its high-volume per-holding load across keyless providers** rather than routing everything through one: company fundamentals cross-checked against **SEC EDGAR** (keyless) and deep historical prices from **Stooq** (keyless), leaving **FMP** for fundamentals breadth, the niche aggregates (movers, earnings calendar, screener, sector / industry P/E), and live **quotes** (`quote`). All sit behind the same data-source seam and fail-soft posture as the report's adapters.

**Both local jobs run on FMP's paid tier** — Trade Opportunities most heavily (it also screens the universe), Portfolio Analysis across a per-holding subset. The application uses **one shared FMP credential for everything — the report and both local jobs — now upgraded to the paid tier** (the suite's one paid dependency, so the user manages a single key). The report's data-source *logic* is unchanged (its existing calls behave identically on the paid key), and the former free-tier gates simply no longer bind — so the newly-unlocked endpoints are available to enrich the report packet as a separate, opt-in enhancement (see [Planned report enrichment](#planned-report-enrichment-paid-fmp-tier) above). The paid tier is the broad working & discovery feed: financial statements / ratios / revenue **segments** (product + geographic) / owner earnings / DCF (earnings-call **transcripts are off-plan** → web-research loop); the **revision signal** (analyst estimates snapshotted for velocity, the `grades-historical` rating-distribution time series, price-target trend, upgrades / downgrades, earnings surprises); **`financial-scores` — Altman Z + Piotroski** for the forensic gate; **positioning** (insider buys / sells + statistics, **SC 13D/13G activist**, and **Senate / House congressional trading** — all symbol-keyed; **13F institutional is off-plan** → SEC EDGAR or omit); and the **screener / peers / industry-classification** discovery layer (the `*-bulk` universe-scoring endpoints are **off-plan**, so the screener stratifies the universe and the multi-factor scoring moves per-candidate — see [§FMP — current paid-plan tier audit](#fmp--current-paid-plan-tier-audit)). The paid tier additionally unlocks FMP's **structured news** — the market-wide `news/general-latest` / `news/stock-latest` feeds and the symbol-scoped Search Stock News (`news/stock`; the press-release feeds are off-plan) — a ticker-tagged, dated surfacing layer for Trade Opportunities' theme discovery and per-candidate sentiment/catalyst reads that *complements* (never replaces) the keyless web-research loop. The **per-symbol** signals here — fundamentals, the revision signal, `financial-scores`, positioning, and peers — are shared by **Portfolio Analysis** (grading held positions) and **Trade Opportunities** (validating candidates), **with one positioning carve-out: the SC 13D/13G activist filings are Trade-Opportunities-only** (a per-candidate accumulation catalyst — Portfolio's endpoint table and its Step-6b positioning layer deliberately exclude them); the **screener / industry-classification** discovery layer is **Trade Opportunities only**, since Portfolio Analysis grades a known holdings list and never screens the universe. **Short interest** comes keyless from FINRA (FMP has no short-interest endpoint); **cyclical commodity prices** from FRED (daily energy + monthly IMF metals — copper, aluminum, nickel, iron ore, uranium) and Stooq futures. **SEC EDGAR is retained as the authoritative primary-source cross-check** for the numbers that drive grades / targets, which makes **ticker→CIK resolution a non-blocking enhancement** rather than a prerequisite (the FMP feeds are symbol-keyed). DRAM/NAND ASPs and supply-discipline signals have no structured feed and ride the research loop ([web-research.md](web-research.md)).

**Portfolio Analysis** reads a per-holding / per-fund subset of that FMP surface (it grades a known holdings list, so it never touches the discovery layer) plus run-level macro inputs; analyst opinions it pulls — price targets, grade distributions, FMP's ratings snapshot — ride in as *evidence the model weighs, never as inputs to the engine's computed grade*. Its full endpoint surface, by source, is tabulated under [§Portfolio Analysis — endpoint surface](#portfolio-analysis--endpoint-surface) below.

### Portfolio Analysis — endpoint surface

Every endpoint Portfolio Analysis ([portfolio-analysis.md](portfolio-analysis.md)) calls (including the conditional fallback paths, tagged *optional*), by source, paralleling the report's per-source tables above. **Cardinality** is the load-bearing axis — it sets the per-run call budget: **per-holding** and **per-fund** calls scale with portfolio size (the budget driver), while **run-level** calls fire once and are shared across all holdings. All FMP paths are on the `https://financialmodelingprep.com/stable` base and run on the shared paid key.

**FMP** — the per-holding / per-fund endpoints Portfolio Analysis calls on the current plan. *Plan-tier status — and the off-plan endpoints these official paths replaced, with their fallbacks — is recorded once in [§FMP — current paid-plan tier audit](#fmp--current-paid-plan-tier-audit). Equity grading is fully covered; the fund path degrades, since constituent look-through is off-plan (SEC N-PORT optional, else dropped).*

| Endpoint path | Cardinality | Portfolio Analysis use |
| --- | --- | --- |
| `profile` | per-holding | sector / industry / **beta** / description — classification + risk input |
| `income-statement` (+ `-ttm`), `balance-sheet-statement`, `cash-flow-statement` | per-holding | core financial statements — engine fundamentals (balance-sheet / cash-flow TTM derived from the 4 quarterly statements, not a separate call); re-pulled by the quick check on a fresh filing ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)) |
| `key-metrics`, `ratios` (+ `…-ttm`) | per-holding | valuation / quality / leverage / margin ratios |
| `financial-scores` | per-holding | Altman Z + Piotroski → risk / quality forensic input |
| `owner-earnings` | per-holding | owner earnings (cash to shareholders) for valuation |
| `enterprise-values` | per-holding | enterprise value for EV multiples |
| `discounted-cash-flow` | per-holding | DCF valuation cross-check |
| `analyst-estimates` | per-holding | forward revenue / EPS consensus → engine **revision-velocity** read + the quick check's **revision preflight** |
| `price-target-consensus`, `price-target-summary` | per-holding | street price-target level + trend — *evidence, not an engine input* |
| `grades`, `grades-historical`, `grades-consensus` | per-holding | `grades-historical` distribution → engine **rating-drift** read; rating actions + current consensus ride as *evidence* |
| `ratings-snapshot`, `ratings-historical` | per-holding | FMP's own composite rating — opinion cross-check only |
| `dividends` | per-holding + per maturing outcome episode (label time) | yield, frequency, schedule — income / total-return grading; re-pulled at label time per maturing outcome episode (symbol-scoped, so exited names stay computable — [portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)) and by the quick check on a fresh filing |
| `earnings` | per-holding | next earnings date + EPS / revenue estimate (catalyst); actual-vs-estimate surprise history — re-pulled by the quick check (the new-earnings-actual leg) |
| `insider-trading/search`, `insider-trading/statistics` | per-holding | insider buys / sells + aggregate statistics |
| `senate-trades`, `house-trades` | per-holding | congressional trading in the name |
| `stock-peers` | per-holding | peer set for relative valuation |
| `shares-float` | per-holding | free float / liquidity → risk input |
| `mergers-acquisitions-latest` | run-level | market-wide M&A feed + 8-K (SEC EDGAR) → acquirer / target catalyst, matched per holding |
| `revenue-product-segmentation`, `revenue-geographic-segmentation` | per-holding | revenue by product / geography — business mix, thematic exposure, what-changed attribution |
| `quote` | per-holding | live quote (current price); re-pulled by the quick check's per-holding price refresh ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)) |
| `quote` (`GCUSD`) | run-level | gold price context for gold-linked holdings — the one FMP commodity quote (FRED's free gold series were discontinued) |
| `news/stock` | per-holding | symbol-scoped news headlines — research-loop **seed** (a lead, never evidence) + a trigger surface for the conditional technology-event topic ([portfolio-analysis.md §The per-holding pipeline](portfolio-analysis.md#the-per-holding-pipeline)); pulled by the quick check for tech-falsifier holdings (the qualifying-news-seed leg) |
| `etf/info` | per-fund | expense ratio, AUM, NAV, asset class, mandate — refreshed by the quick check (the fund evidence-event leg) |
| `etf/sector-weightings`, `etf/country-weightings` | per-fund | sector / country exposure → fund exposure tilt (the constituent look-through proxy); refreshed by the quick check per the exposure evidence-event legs |
| `sector-pe-snapshot` | run-level (one call per exchange, shared across funds) | per-sector aggregate P/E (exchange-tagged) → the fund path's **exposure-priced valuation** composite ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)) |
| `historical-sector-pe` | run-level (one call per sector × exchange, memoized on first need across funds — the union of the held funds' composite sectors, both exchanges) | the exposure composite's own trailing history → the fund's **constant-current-mix** vs-own-past read ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)) |

**FRED** — base `https://api.stlouisfed.org/fred`, `/series/observations` (the `series_id` doubles as the quote symbol).

| Series ID | Series | Cardinality | Portfolio Analysis use |
| --- | --- | --- | --- |
| `DGS10` | 10-Year Treasury Yield | run-level | risk-free rate → valuation-engine discounting |
| `DGS2` | 2-Year Treasury Yield | run-level | risk-free rate (short end) → the capital-efficiency hurdle anchor; also refreshed by the quick check ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)) |
| `DCOILWTICO` | WTI Crude Oil | run-level | commodity context for energy-linked holdings |
| `DHHNGSP` | Henry Hub Natural Gas | run-level | commodity context for energy-linked holdings |

Materials-linked holdings reuse the suite's broader FRED commodity set (monthly IMF metals incl. copper, aluminum, nickel, iron ore, uranium — series IDs catalogued under [§Trade Opportunities — endpoint surface](#trade-opportunities--endpoint-surface) below, the suite's commodity feed); gold is FMP `GCUSD`.

**CFTC** — Socrata, base `https://publicreporting.cftc.gov/resource/<dataset>.json`. The same keyless pull the report makes; Portfolio Analysis maps a fund holding onto an already-gathered contract.

| Dataset | Contracts | Cardinality | Portfolio Analysis use |
| --- | --- | --- | --- |
| `gpe5-46if` (Traders in Financial Futures) | E-mini S&P 500, Nasdaq-100, 10Y / 2Y Treasuries, USD Index | run-level | underlying positioning for an equity-index / rates / FX **fund** holding |
| `72hh-3qpy` (Disaggregated) | Gold, WTI Crude, Copper | run-level | underlying positioning for a commodity **fund** holding |

A fund whose underlying isn't among these contracts fail-softs to no positioning read.

**Schwab / SEC EDGAR / Stooq / FINRA / CBOE / web** — the account, keyless, sentiment, and research sources (full endpoint detail in their catalog entries under [The data sources](#the-data-sources) and in [web-research.md](web-research.md)).

| Source · endpoint | Cardinality | Portfolio Analysis use |
| --- | --- | --- |
| Schwab · accounts / positions | run-level | holdings — quantity, cost basis, market value, instrument identity |
| Schwab · `/marketdata/v1/chains` | per-holding (optionable equity) | option chains → options-activity signal (put/call, IV/skew) |
| SEC EDGAR · submissions (10-K / 10-Q / 8-K) | per-holding | filings — authoritative cross-check + 8-K events |
| SEC EDGAR · company-facts (XBRL) | per-holding | normalized statement data the engine computes over |
| SEC EDGAR · 13F filings | run-level (optional) | coarse institutional-ownership backdrop — EDGAR 13F is filer-keyed (not symbol-keyed), so a per-name read is approximate and **often omitted**; held out of the grade until calibrated |
| SEC EDGAR · N-PORT filings | per-fund (optional enrichment) | fund constituent holdings for concentration / single-name look-through (heavy, ~60-day lag); without it, ETFs retain the on-plan `etf/info` + sector / country exposure path, while mutual funds retain whatever information / allocation surface resolves and route to `role_risk_only` when the weighting set cannot support exposure pricing ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)) |
| Stooq · daily OHLCV CSV | per-holding + per maturing outcome episode (label time) | deep price history → momentum / volatility / price-target scenarios; read through the shared price-bar cache by the quick check's per-holding price refresh ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)); label-time refresh through the matured window end, including exited symbols (cache may substitute only when it covers the full window — [portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)) |
| Stooq · daily OHLCV CSV (sector / market benchmark indices) | run-level | sector- and market-relative benchmarks → the input delta's technology-event pre-flag and the outcome-learning labels ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)) |
| FINRA · consolidated short-interest file | per-holding lookup (file fetched once / run) | short-interest level / trend / days-to-cover → risk / squeeze context |
| CBOE · daily put/call statistics | run-level | venue-level options-sentiment backdrop (broad-market context, not a per-name signal) |
| Web tool — keyless SearXNG | per-holding (research lane) | management commentary from earnings-call transcripts (IR / aggregator sites) + the per-holding research lane for signals with no structured feed ([web-research.md](web-research.md)) |

### Trade Opportunities — endpoint surface

Every endpoint Trade Opportunities ([trade-opportunities.md](trade-opportunities.md)) calls, by source, paralleling the report's and Portfolio Analysis's per-source tables. Trade Opportunities is a **funnel** (broad discovery → narrowed candidates → expensive per-name validation), so its **cardinality has three bands** rather than Portfolio's two — and the band, not the endpoint, sets the call budget:

- **discovery** — fires a bounded number of times per run and scans the **whole universe** to *generate* candidates (the FMP `company-screener` + event / positioning feeds, the model-led SearXNG research sweep, the short-interest extremes screen — **no `*-bulk` pre-scoring, off-plan**). Broad but cheap at this stage: the rich multi-factor signals are **not** computed here — they're spent **per candidate** after the funnel narrows (Step 5c), which is exactly what the discovery-breadth budget rations.
- **per-candidate** — scales with the **narrowed candidate set** and is the **budget driver**: the same per-symbol FMP / SEC / Stooq surface Portfolio Analysis spends per *holding*, here spent per *candidate*. The funnel exists to keep this set small.
- **run-level** — fires **once** and is shared across all candidates: the macro / positioning / sentiment context, and the holdings list for the Step-8 cross-reference.

All FMP paths are on the `https://financialmodelingprep.com/stable` base and run on the shared paid key. The **per-candidate** per-symbol rows are the same surface tabulated under [§Portfolio Analysis — endpoint surface](#portfolio-analysis--endpoint-surface) (fundamentals, the revision signal, positioning, segments) — repeated here with their Trade-Opportunities use, re-tagged to candidate cardinality, and **minus the ETF / mutual-fund group** (the job hunts operating businesses by archetype, not funds) — **plus the discovery layer Portfolio Analysis never touches** (screener / peers / industry-classification, the structured news / event feeds, and a handful of richer per-candidate signals — growth, activist-stake filings).

*Plan status per endpoint: see [§FMP — current paid-plan tier audit](#fmp--current-paid-plan-tier-audit). On the current plan **every `*-bulk` endpoint is blocked**, so the universe-wide pre-scoring this layer assumed is gone — the discovery funnel is **redesigned** around `company-screener` + per-symbol scoring on a narrowed longlist, plus the model-led hypothesis-research lane (see [trade-opportunities.md](trade-opportunities.md)). The screener / taxonomy / movers / news-event / insider-latest feeders below all survive.*

**FMP — discovery layer (universe-wide; Trade Opportunities only)**

The `*-bulk` universe-scoring endpoints this layer once leaned on are **off-plan** ([§FMP — current paid-plan tier audit](#fmp--current-paid-plan-tier-audit)); the multi-factor composite + forensic gate they fed move **per-candidate** (Step 5c). What survives **generates and stratifies** the longlist rather than pre-scoring it:

| Endpoint path | Cardinality | Trade Opportunities use |
| --- | --- | --- |
| `company-screener` | discovery | universe definition + tradability gate + **market-cap-band / sector stratification** (coarse fields only — no valuation / metric filter) |
| `insider-trading/latest` | discovery | market-wide newest-Form-4 feed → **insider cluster-buy** candidate surfacing (the per-symbol `insider-trading/search` is the per-candidate follow-up) |
| `biggest-gainers`, `biggest-losers`, `most-actives` | discovery | market movers (the report's free movers, reused as a momentum / dislocation feeder) |
| `earnings-calendar` | discovery | upcoming-catalyst calendar (event-scan feeder) — and read **backward** over the trailing window as the **post-earnings surprise screen** (consensus + actuals on the paid key → the continuation-mode feeder; streaks confirmed per-candidate via `earnings`) |
| `available-sectors`, `industry-classification-search`, `all-industry-classification` | discovery | industry taxonomy → map a surfaced theme onto its exposed names |
| `stock-peers` | discovery + per-candidate | expand a screened / surfaced name to its peer cohort (discovery); relative-valuation comps (per-candidate) |

**FMP — news & event feeds (discovery; structured, ticker-tagged, on the shared paid key)**

These were **premium-gated on the free tier** (HTTP 402 — see the report's note that FMP's `news/*` feeds are premium) and become available on the suite's **paid** key. They are a **structured surfacing layer** — ticker-tagged, dated headlines + snippets + source URLs — that *complements*, and does not replace, the keyless SearXNG web-research loop ([web-research.md](web-research.md)): the feed surfaces the headline, the web tool deep-reads the article URL. They reuse the FMP credential, so they add no new dependency. The report's own news funnel (FMP Articles) is unchanged; these are suite-only feeders.

*Plan status: see [§FMP — current paid-plan tier audit](#fmp--current-paid-plan-tier-audit); the feeds below are available on the suite's paid key. Primary-source disclosure catalysts come from `sec-filings-8k` + the web loop.*

| Endpoint path | Cardinality | Trade Opportunities use |
| --- | --- | --- |
| `news/general-latest` | discovery | macro / market news feed → ignition-point input to the top-down theme scan |
| `news/stock-latest` | discovery | ticker-tagged stock-news feed → dislocation / story surfacing for the theme scan |
| `mergers-acquisitions-latest` | discovery + per-candidate | market-wide M&A feed → takeover-target / deal-flow candidates (discovery); also the per-candidate M&A catalyst read — whether a name is acquirer or target, matched per candidate + `sec-filings-8k` |
| `sec-filings-8k` | discovery (optional) | market-wide 8-K material-event feed → fresh-catalyst scan |
| `ipos-calendar` | discovery | recently priced + upcoming scheduled IPOs → the Step-3a IPO-catalyst event feeder |

**FMP — per-candidate validation (the narrowed set; shared per-symbol surface with Portfolio Analysis)**

*These are the available per-symbol paths Trade Opportunities validates a candidate over. Plan-tier status, the off-plan per-symbol signals these rows omit, and the web-research-loop / SEC EDGAR / `sec-filings-8k` fallbacks that cover them are recorded in [§FMP — current paid-plan tier audit](#fmp--current-paid-plan-tier-audit) — as are the US-exchange / annual-period constraints.*

| Endpoint path | Cardinality | Trade Opportunities use |
| --- | --- | --- |
| `profile` | per-candidate | sector / industry / **beta** / description — archetype features + risk input |
| `income-statement` (+ `-ttm`), `balance-sheet-statement`, `cash-flow-statement` | per-candidate | core statements — engine fundamentals + forensic divergences (balance-sheet / cash-flow TTM derived from the 4 quarterly statements, not a separate call) |
| `key-metrics`, `ratios` (+ `…-ttm`) | per-candidate | valuation / quality / leverage / margin ratios (archetype-weighted) |
| `financial-scores` | per-candidate | Altman Z + Piotroski → forensic gate |
| `owner-earnings`, `enterprise-values`, `discounted-cash-flow` | per-candidate | owner-earnings yield, EV multiples, DCF cross-check — archetype valuation lens |
| `financial-growth` | per-candidate | multi-year per-share CAGRs (revenue / EPS / FCF / book value) → growth trajectory + the value-creation reinvestment-runway read |
| `revenue-product-segmentation`, `revenue-geographic-segmentation` | per-candidate | **annual periods only** ([tier audit](#fmp--current-paid-plan-tier-audit)) → segment mix / multi-year trajectory **context** and the own-history normalization basis — **not** the acceleration anchor's cadence source (the quarterly series is filing-derived — see below) |
| `analyst-estimates` | per-candidate | forward consensus, snapshotted run-to-run → engine **revision-velocity** read |
| `grades`, `grades-historical`, `grades-consensus` | per-candidate | `grades-historical` distribution → engine **rating-drift** read; actions + consensus ride as *evidence* |
| `price-target-consensus`, `price-target-summary` | per-candidate | street target level + trend — *evidence, not an engine input* |
| `ratings-snapshot`, `ratings-historical` | per-candidate | FMP composite rating — opinion cross-check only |
| `earnings` | per-candidate | next earnings date (catalyst) + actual-vs-estimate surprise / SUE history |
| `news/stock` (Search Stock News) | per-candidate | symbol-scoped **structured news** → seeds the narrative / sentiment and catalyst reads, then deep-read via the web tool |
| `insider-trading/search`, `insider-trading/statistics` | per-candidate | insider buy clusters + aggregate statistics |
| `acquisition-of-beneficial-ownership` | per-candidate | SC 13D / 13G beneficial-ownership filings → **activist / large-stake accumulation** catalyst |
| `senate-trades`, `house-trades` | per-candidate | congressional buys in the name |
| `shares-float` | per-candidate | free float / liquidity → deterministic risk-tier + squeeze input |
| `historical-employee-count`, `key-executives` | per-candidate (optional) | workforce trend (hiring / revenue-per-employee) + leadership roster → operating-efficiency & management read for the investor-judgment lens |
| `quote` | per-candidate | live quote (current price) |

**The segment-acceleration quarterly series is filing-derived.** FMP's segment endpoints are annual-only (above), which cannot carry the *accelerating*-family gate (seasonally comparable changes, ≥ 5-change minimum history — [trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable)). The quarterly observations come from the name's **own 10-Q / press-release segment disclosures**, deep-read through the bounded research loop as **dated, cited, third-party observations** (the gate's research-supplied path), validated and **appended per filing** to the candidate's stored leading-metric series (a `filing`-cadence series). When neither the annual history nor the filing-derived observations reach the family minimum, the series is **unmeasurable → evidence-floor abstention** ([trade-opportunities.md §Evidence floor](trade-opportunities.md#evidence-floor)) — never an estimated substitute.

**FRED** — base `https://api.stlouisfed.org/fred`, `/series/observations` (the `series_id` doubles as the quote symbol) plus `/release/dates` (run-level — the **macro-release calendar** schedule, names + dates, that seeds the Step-3b discovery routes; the same free endpoint the report's calendar uses). Fired once per run as shared context; the commodity set also seeds the commodity-cyclical discovery sleeve (a price turn surfaces names).

| Series ID | Series | Cardinality | Trade Opportunities use |
| --- | --- | --- | --- |
| `DGS10` | 10-Year Treasury Yield | run-level | risk-free rate → engine discounting / scenario targets |
| `DGS2` | 2-Year Treasury Yield | run-level | risk-free rate (short end) → the entry-asymmetry threshold anchor ([trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable)) |
| `DCOILWTICO` | WTI Crude Oil (daily) | run-level | energy-price level / turn — cyclical sleeve + per-candidate context |
| `DHHNGSP` | Henry Hub Natural Gas (daily) | run-level | energy-price level / turn — cyclical sleeve + per-candidate context |
| `PCOPPUSDM` | Global price of Copper (monthly, IMF) | run-level | metals-price turn — commodity-cyclical archetype |
| `PALUMUSDM` | Global price of Aluminum (monthly, IMF) | run-level | metals-price turn — commodity-cyclical archetype |
| `PNICKUSDM` | Global price of Nickel (monthly, IMF) | run-level | metals-price turn — commodity-cyclical archetype |
| `PIORECRUSDM` | Global price of Iron Ore (monthly, IMF) | run-level | metals-price turn — commodity-cyclical archetype |
| `PURANUSDM` | Global price of Uranium (monthly, IMF) | run-level | uranium-price turn — nuclear / utility cyclical sleeve |

Gold / silver remain FMP `GCUSD` / `SIUSD`; daily-cadence metals (copper futures) come from Stooq (below) as the higher-frequency complement to the monthly IMF series. The five monthly IMF series above are the **suite-shared commodity feed**, not a Trade-Opportunities exclusive: Portfolio Analysis's materials-linked holdings read the same run-level pull ([§Portfolio Analysis — endpoint surface](#portfolio-analysis--endpoint-surface)).

**CFTC** — Socrata, base `https://publicreporting.cftc.gov/resource/<dataset>.json`. The same keyless pull the report and Portfolio Analysis make; Trade Opportunities reads it for the commodity-cyclical sleeve's positioning.

| Dataset | Contracts | Cardinality | Trade Opportunities use |
| --- | --- | --- | --- |
| `gpe5-46if` (Traders in Financial Futures) | E-mini S&P 500, Nasdaq-100, 10Y / 2Y Treasuries, USD Index | run-level | macro / rates / FX positioning backdrop for the theme scan |
| `72hh-3qpy` (Disaggregated) | Gold, WTI Crude, Copper | run-level | speculator crowding for a commodity-cyclical candidate's underlying |

**Schwab / SEC EDGAR / Stooq / FINRA / CBOE / news & web** — the account, keyless, sentiment, and research sources (full endpoint detail in their catalog entries under [The data sources](#the-data-sources) and in [web-research.md](web-research.md)).

| Source · endpoint | Cardinality | Trade Opportunities use |
| --- | --- | --- |
| Schwab · accounts / positions | run-level | holdings list — owned/not-owned cross-reference only (Step 8); never a discovery or scoring input |
| Schwab · `/marketdata/v1/chains` | per-candidate (optionable equity) | option chains → options-activity signal (put/call, IV/skew) |
| SEC EDGAR · submissions (10-K / 10-Q / 8-K) | per-candidate | filings — authoritative cross-check + 8-K events (ticker→CIK a non-blocking enhancement) |
| SEC EDGAR · company-facts (XBRL) | per-candidate | normalized statement data the engine cross-checks against |
| SEC EDGAR · 13F filings | run-level (optional) | coarse institutional-flow backdrop — EDGAR 13F is filer-keyed (not symbol-keyed), so a per-name read is approximate and **often omitted**; held out of the grade until calibrated (holder-level deltas have no source and are dropped) |
| Stooq · daily OHLCV CSV (equities) | per-candidate | deep price history → price-action confirmer (relative strength / base breakout), momentum / volatility, scenario targets |
| Stooq · daily OHLCV CSV (futures, incl. copper) | run-level | daily commodity-price turn for the cyclical sleeve (complements the monthly IMF series) |
| Stooq · daily OHLCV CSV (sector / market benchmark indices) | run-level (outcome learning) | sector- and market-relative forward-return benchmark for the Step-7 deterministic outcome labels and each carried-forward idea's continuous since-flagged read ([trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)) |
| FINRA · consolidated short-interest file | discovery (file fetched once / run) + per-candidate lookup | short-interest extremes screen — a **bearish-by-default** factor, a *conditional* squeeze candidate only with an inflecting metric + catalyst + breaking bear case (discovery); level / trend / days-to-cover per candidate |
| CBOE · daily put/call statistics | run-level | venue-level options-sentiment backdrop (broad-market context, not a per-name signal) |
| FMP · `fmp-articles` | discovery | keyless in-house, ticker-tagged commentary feeding the top-down theme & event scan (ignition points → exposed industries) — reuses the report's keyless adapter; no new credential. **GDELT is dropped** — its escalating rate-limit / IP-lockout makes it unreliable (the same reason the report job doesn't trust it) |
| Web tool — **keyless SearXNG** | discovery (theme→names research loop) + per-candidate (validation loop) | the keyless local search / fetch / extract loop the orchestrator runs on a model's behalf — the theme/news search that lets the model **reason its way to names**, and the per-candidate research lane (signals with no structured feed: DRAM/NAND ASPs, supply discipline, moat/management scuttlebutt, and management commentary from earnings-call transcripts). **Discovery runs on SearXNG only — no Tavily, no GDELT** (a discovery feeder that's down fail-softs to fewer candidates); Tavily remains only the *per-candidate* web loop's degraded fallback when SearXNG is down ([web-research.md](web-research.md)), never a discovery dependency |
