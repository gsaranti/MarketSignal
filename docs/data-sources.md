# Data Sources

This file lists the external data and model providers the application depends on. Credential configuration for these providers is covered in [configuration.md](configuration.md).

## Market and Financial Data

The application accesses market and financial data by calling provider REST APIs directly from the Rust backend (`reqwest`/`serde`). **Financial Modeling Prep** is the primary financial-data source, supplying equity-market data — indices, volatility, gold, sector performance, multi-horizon index returns, and company financials. **FRED** and **BLS** supply macroeconomic and labor data through their public APIs; FRED additionally provides the US dollar index, commodity prices (oil, natural gas), Treasury yields, credit and yield-curve spreads, and financial-conditions indices.

The gated REST adapters (FMP, FRED, BLS, Tavily) share a bounded retry-with-backoff for transient HTTP-status/transport failures (HTTP 429, 5xx, dropped connections; `Retry-After`-aware). GDELT is deliberately excluded — its escalating per-IP lockout makes a retry harmful, so it keeps its single-shot fail-soft.

Where retries don't recover, the Step-6 baseline scan degrades rather than aborting: an unresolved series or release (a rejected key, a sustained outage, or a malformed / empty response) is recorded as a gap in a missing-data manifest instead of failing the whole scan, and a central coverage floor then decides whether what resolved is sufficient to generate the report (see [weekly-report-workflow.md §Step 6](weekly-report-workflow.md#step-6-gather-baseline-market-data)).

### Financial Modeling Prep
Docs - https://site.financialmodelingprep.com/developer/docs

Financial Modeling Prep is the primary financial-data source for the application.

Responsibilities:
- market prices
- index data (Dow, S&P 500, Nasdaq, Russell 2000)
- market volatility (VIX)
- sector performance
- historical end-of-day prices (free tier)
- multi-horizon index performance (weekly / MTD / YTD / 52-week range)
- company financials
- earnings information
- analyst estimates
- market metrics
- economic calendar (premium tier only — see below)

The application calls Financial Modeling Prep directly for the equity-market portion of the baseline market-data scan ([weekly-report-workflow.md §Step 6](weekly-report-workflow.md#step-6-gather-baseline-market-data)) — indices, volatility, gold, and sector performance, plus each index's multi-horizon performance (weekly, month-to-date, year-to-date, and 52-week-range position) derived from FMP's free historical end-of-day prices (verified live: the indices and the VIX return on the free tier) — and for company-specific financial data surfaced during research. The scan's dollar-index, oil, natural-gas, and Treasury-yield series come from FRED (below). (Gold is on FMP's free tier via `GCUSD`; FRED's former free gold benchmark series were discontinued, so gold stays on FMP.) The **economic-release calendar** is likewise gated behind FMP premium (verified live: the `economic-calendar` endpoint returns HTTP 402 on the free tier), so the Step-6 calendar's release schedule comes from FRED's free release-dates endpoint (below) rather than FMP.

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
- credit spreads (high-yield and investment-grade OAS)
- financial-conditions indices (NFCI, ANFCI, St. Louis stress index)
- unemployment data (incl. weekly initial and continued jobless claims)
- Fed balance sheet and mortgage rates
- consumer data
- broader macroeconomic indicators
- economic-release calendar (release schedule)

The application uses FRED for macroeconomic analysis and long-term market-regime evaluation, and for the market-internal series — the dollar index, oil, and natural gas — that sit outside Financial Modeling Prep's free-tier coverage. It also supplies the risk- and cycle-oriented series that anchor the report's risk-posture and market-cycle reads: credit spreads (high-yield and investment-grade OAS), the 10y–3m and 10y–2y curve spreads, the Chicago Fed financial-conditions indices (NFCI, ANFCI) and the St. Louis stress index (STLFSI4), weekly initial and continued jobless claims, the Fed balance sheet, and the 30-year mortgage rate. (FRED's documented limit is 120 requests/minute with no daily cap; the weekly scan's ~33 requests sit far under it.) It also supplies the Step-6 **economic-release calendar** — the prior-week and upcoming US release schedule (CPI, PCE, jobs, GDP, …) via FRED's free release-dates API — since FMP's economic-calendar endpoint is premium-gated. (FOMC meetings are excluded from the calendar — FRED has no scheduled-date series for them; the Fed's policy stance is carried by the Fed-funds target-range series instead.) FRED provides release dates (and the underlying series values, gathered separately), but no analyst-consensus estimates; no free source supplies US consensus, so the calendar's "expected" value is omitted and reserved for a future paid source.

### BLS (Bureau of Labor Statistics)
Docs - https://www.bls.gov/developers/

BLS provides official United States labor and inflation datasets.

Responsibilities:
- CPI reports
- employment reports
- wage data
- productivity data
- labor-market statistics

The application uses BLS data for inflation and labor-market analysis.

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

## LLM Providers
- [OpenAI](https://platform.openai.com/docs)
- [Anthropic](https://platform.claude.com/docs)

The specific models exposed by these providers, the user-configurable model selections for each agent, and the API-token requirements are covered in [configuration.md](configuration.md). The non-configurable models used by fixed internal pipeline stages are covered in [agents.md](agents.md).
