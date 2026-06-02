# Data Sources

This file lists the external data and model providers the application depends on. Credential configuration for these providers is covered in [configuration.md](configuration.md).

## Market and Financial Data

The application accesses market and financial data by calling provider REST APIs directly from the Rust backend (`reqwest`/`serde`). **Financial Modeling Prep** is the primary financial-data source; **FRED** and **BLS** supply macroeconomic and labor data through their public APIs.

### Financial Modeling Prep
Docs - https://site.financialmodelingprep.com/developer/docs

Financial Modeling Prep is the primary financial-data source for the application.

Responsibilities:
- market prices
- index data (Dow, S&P 500, Nasdaq, Russell 2000)
- market internals (VIX, dollar index, oil, natural gas, gold)
- sector performance
- company financials
- earnings information
- analyst estimates
- market metrics
- economic calendar

The application calls Financial Modeling Prep directly for the baseline market-data scan ([weekly-report-workflow.md §Step 6](weekly-report-workflow.md#step-6-gather-baseline-market-data)) and for company-specific financial data surfaced during research.

### FRED (Federal Reserve Economic Data)
Docs - https://fred.stlouisfed.org/docs/api/fred/

FRED provides official macroeconomic and financial data maintained by the Federal Reserve Bank of St. Louis.

Responsibilities:
- Treasury yields
- interest rates
- inflation metrics
- recession indicators
- unemployment data
- consumer data
- broader macroeconomic indicators

The application uses FRED for macroeconomic analysis and long-term market-regime evaluation.

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
