# Data Sources

## Market and Financial Data

### OpenBB

Docs - https://docs.openbb.co

OpenBB acts as the primary financial-data access layer for the application.

Responsibilities:
- market prices
- index data
- sector data
- company fundamentals
- earnings data
- financial metrics
- standardized access to financial datasets

The application uses OpenBB to normalize and simplify financial-data retrieval workflows.

### Financial Modeling Prep

Docs - https://site.financialmodelingprep.com/developer/docs

Financial Modeling Prep provides structured financial and market datasets.

Responsibilities:
- company financials
- earnings information
- analyst estimates
- market metrics
- sector performance
- supplemental financial data used by the agents

The application uses Financial Modeling Prep as a direct structured financial-data source.

OpenBB is the default access path for financial data. Financial Modeling Prep is called directly only for data that OpenBB does not expose, or for cases where FMP's data shape is materially better suited to the task (e.g., specific analyst-estimate endpoints). This keeps OpenBB as the consistent abstraction layer and avoids duplicate calls for the same data.

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

- OpenAI — [API docs](https://platform.openai.com/docs)
- Anthropic — [API docs](https://platform.claude.com/docs)

Specific models and their roles (fixed-internal vs user-configurable) are described in [agents/models.md](agents/models.md).
