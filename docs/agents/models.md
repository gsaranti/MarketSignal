# Models

## Fixed Internal Model Usage

Some internal workflows use non-configurable models for cost control and predictable performance.

### Headline Filtering

Uses:
- OpenAI GPT-5 mini

Purpose:
- filtering
- deduplication
- relevance scoring
- clustering headlines into major topics

Rationale:
- low cost
- fast latency
- strong enough for lightweight classification tasks

### Data Extraction

Uses:
- OpenAI GPT-5 mini

Purpose:
- extracting structured information from:
  - news articles
  - PDFs
  - research documents
  - earnings summaries
  - macro reports

Rationale:
- reliable structured output
- inexpensive
- good tool/function calling performance

### Research Routing

Uses:
- Anthropic Claude Sonnet

Purpose:
- determining which topics deserve deeper analysis
- identifying second-order implications
- prioritizing research depth
- deciding which themes/subsectors/geopolitical events matter most

Rationale:
- stronger reasoning quality
- better long-context understanding
- more nuanced prioritization and synthesis

## User-Configurable Models

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

The user must provide:
- OpenAI API token
- Anthropic API token

When a user selects a provider for an agent, the corresponding API token is required.

Examples:
- selecting an OpenAI model requires a valid OpenAI API token
- selecting an Anthropic model requires a valid Anthropic API token

If a required token is missing:
- settings saving is disabled
- the application displays a validation warning explaining which token is required

By default, the application starts with no models selected for:
- Main Agent
- Bull Analyst
- Bear Analyst
- Balanced Analyst

The user must configure a model for all four agents before scheduled jobs can run.

If any agent does not have a configured model:
- scheduled jobs do not execute
- manual report execution is disabled
- the application displays a warning message on the homepage indicating which agents still require configuration
