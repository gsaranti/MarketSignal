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

The user must configure a model for all four agents before the application is ready to run scheduled jobs.

"Ready" is distinct from "enabled". The enable/disable toggle in Job Controls (see [../job-execution.md](../job-execution.md)) controls whether a job's schedule is active. "Ready" describes whether execution preconditions — currently, having a configured model for each of the four agents — are satisfied. A job that is enabled but for which the application is not ready will not run.

If any agent does not have a configured model, the application is not ready to run scheduled jobs:
- enabled scheduled jobs do not execute at their scheduled times
- manual report execution is disabled
- the application displays a warning message on the homepage indicating which agents still require configuration
