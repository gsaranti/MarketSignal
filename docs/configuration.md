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

For the non-configurable models used by fixed internal pipeline stages (headline filtering, data extraction, research routing), see [agents.md §Fixed Internal Models](agents.md#fixed-internal-models).

## API Tokens

The user must provide:
- OpenAI API token
- Anthropic API token

When a user selects a provider for an agent, the corresponding API token is required.

Examples:
- selecting an OpenAI model requires a valid OpenAI API token
- selecting an Anthropic model requires a valid Anthropic API token

If a required token is missing:
- saving the configuration is disabled
- the application displays a validation warning explaining which token is required

## External Data Provider Credentials

The application also requires configuration for external data providers that use authenticated APIs.

The Settings section includes credential configuration for:
- Financial Modeling Prep
- Tavily

OpenBB uses configured provider credentials where required by the selected data source.

FRED, BLS, and GDELT may be accessed through their publicly available APIs when supported.

If a required external provider credential is missing:
- dependent jobs do not execute
- manual report execution is disabled
- the application displays a validation warning explaining which credential is missing

For the data providers themselves and what each is used for, see [data-sources.md](data-sources.md).
