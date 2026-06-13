# Configuration

## Settings Overview

The Settings section includes:
- model selection
- API token configuration
- external data provider credentials
- scheduled job enable/disable controls
- manual job execution controls

The scheduled job enable/disable controls and manual job execution controls are described in [scheduling.md](scheduling.md). This file covers model selection, API tokens, and external data provider credentials.

## Agent Model Configuration

The user selects the models used for:
- Main Agent
- Bull Analyst
- Bear Analyst
- Balanced Analyst

OpenAI Models:
- GPT-5
- GPT-5 mini

Anthropic Models:
- Claude Opus
- Claude Sonnet
- Claude Haiku

By default, the application starts with no models selected for:
- Main Agent
- Bull Analyst
- Bear Analyst
- Balanced Analyst

The user must configure a model for all four agents before scheduled jobs can run.

If any agent does not have a configured model:
- the Weekly Market job does not execute
- manual report execution is disabled
- the application displays a warning message on the homepage indicating which agents still require configuration

Scheduled jobs are enabled by default, but jobs cannot execute until all required agent models and provider credentials are configured.

A newly installed application therefore begins in an incomplete configuration state until the user finishes model and API setup.

For the non-configurable models used by fixed internal pipeline stages (headline filtering, research routing), see [agents.md §Fixed Internal Models](agents.md#fixed-internal-models).

## API Tokens

Both API tokens are always required, regardless of which models the user selects for the four agents:
- OpenAI API token
- Anthropic API token

Both are mandatory because the fixed internal pipeline stages always use both providers — OpenAI for headline filtering (GPT-5 mini) and for vector-memory embeddings (`text-embedding-3-large`), and Anthropic for research routing (Claude Sonnet). See [agents.md §Fixed Internal Models](agents.md#fixed-internal-models) and [storage.md §Embeddings](storage.md#embeddings). Because the only model providers are OpenAI and Anthropic, the user's agent model selection adds no token requirement beyond these two.

If either token is missing:
- saving the configuration is disabled
- the application displays a validation warning explaining which token is required

## External Data Provider Credentials

The application also requires configuration for external data providers that use authenticated APIs.

The Settings section includes credential configuration for:
- Financial Modeling Prep
- FRED
- Tavily

The **Financial Modeling Prep**, **FRED**, and **Tavily** credentials are all required to run a job:
- **Tavily** is the primary research and news-ingestion system, and news gathering is a mandatory workflow step (see [weekly-report-workflow.md §Step 7](weekly-report-workflow.md#step-7-gather-and-filter-news)).
- **Financial Modeling Prep** is the primary financial-data source and provides the equity-market portion of the baseline market-data scan — indices, volatility (VIX), gold, and sector performance (see [weekly-report-workflow.md §Step 3](weekly-report-workflow.md#step-3-gather-baseline-market-data)) — which is not optional, so a missing FMP credential blocks execution.
- **FRED** supplies the macro and commodity portion of the same baseline scan — the dollar index, oil, natural gas, and the 2Y/10Y Treasury yields — which is likewise not optional, so a missing FRED credential blocks execution. FRED requires a free API key on every request.

BLS and GDELT are also accessed through public APIs but need no Settings credential: **BLS** works keyless (an optional free key raises its rate limits), and **GDELT** needs no key.

If a required external provider credential (the Financial Modeling Prep, FRED, or Tavily credential) is missing:
- dependent jobs do not execute
- manual report execution is disabled
- the application displays a validation warning explaining which credential is missing

For the data providers themselves and what each is used for, see [data-sources.md](data-sources.md).
