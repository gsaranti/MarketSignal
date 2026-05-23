# Data Sources

This file lists the external data and model providers the application depends on. Credential configuration for these providers is covered in [configuration.md](configuration.md).

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

The application uses OpenBB as the primary financial-data access layer where practical.

OpenBB is used to normalize and simplify financial-data retrieval workflows across supported providers and datasets.

OpenBB uses configured provider credentials where required by the selected data source.

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

The application uses Financial Modeling Prep directly for structured financial datasets when direct access is simpler, more complete, or required by the workflow.

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
- OpenAI
- Anthropic

The specific models exposed by these providers, the user-configurable model selections for each agent, and the API-token requirements are covered in [configuration.md](configuration.md). The non-configurable models used by fixed internal pipeline stages are covered in [agents.md](agents.md).
