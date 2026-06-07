# Agent Pipeline Architecture

Market Signal uses a fixed multi-agent pipeline for each report.

The pipeline is not tool-driven by the main agent. The analyst agents are required stages that run for every report after the research packet is created.

The application layer executes external API calls and deterministic data retrieval.

Agents do not directly perform unbounded API access. When deeper research is needed, the application layer executes a bounded research plan against configured data sources. That plan is produced by the fixed research-routing model, not the main agent — the main agent shapes research only indirectly, through the inputs that feed routing (prior-report context, unresolved thesis questions, and retrieved memory). See [weekly-report-workflow.md §Step 8](weekly-report-workflow.md#step-8-perform-research-routing).

The end-to-end ordering of pipeline stages — including when each agent runs relative to research gathering, news filtering, and report saving — is defined in [weekly-report-workflow.md](weekly-report-workflow.md).

## Main Agent

The main agent acts as the Head Market Analyst.

The main agent is responsible for:
- planning market data and research needs
- surfacing unresolved thesis questions and research needs that inform research routing (the executable research plan is produced by the fixed routing model)
- consuming curated data returned by the application layer (including the baseline market scan and the change view of how it moved since the previous report)
- creating the condensed research packet
- using relevant memory retrieved by the application layer
- auditing prior report accuracy using report context and market evidence
- maintaining evolving long-term market theses
- evaluating analyst agent outputs
- synthesizing the final report
- producing the final Markdown report
- identifying durable learnings to write to memory

The main agent owns the final report.

The main agent is responsible for producing a cohesive weekly market publication that:
- evaluates the prior week's market behavior
- reviews evolving market theses
- identifies important structural developments
- prepares for future market-moving conditions
- synthesizes forward-looking market analysis

### Synthesis Behavior

The main agent does not engage in recursive conversations with analyst agents.

It critiques analyst agent outputs independently during synthesis.

The main agent may:
- agree with one or more analyst agents
- reject weak reasoning
- combine arguments
- elevate a minority view
- identify unsupported claims
- update the long-term thesis
- flag uncertainty

The main agent evaluates all analyst agent outputs and determines how much weight to assign each perspective during final report generation.

The final report is written in one unified voice as the Market Signal Thesis.

The report should behave like a professional weekly market publication focused on:
- thesis evolution
- structural market developments
- major macroeconomic and geopolitical forces
- retrospective auditing of prior reports
- forward-looking market preparation
- retrospective evaluation of prior assumptions

For how the unified-voice constraint maps to the report's section layout (including when separate Bull/Bear/Balanced material may surface), see [report-structure.md](report-structure.md).

## Analyst Agents

Three analyst agents run after the main agent creates the condensed research packet:
- Bull Analyst
- Bear Analyst
- Balanced Analyst

These agents are not optional tools. They are fixed review stages in the report-generation pipeline.

Each analyst agent receives the same condensed research packet and produces a structured analysis from its assigned analytical perspective.

The analyst agents evaluate:
- the prior week's market developments
- evolving macroeconomic conditions
- geopolitical developments
- market positioning
- structural risks
- forward-looking opportunities or threats

The analyst agents are not forced into predetermined conclusions or artificial disagreement.

Their purpose is to:
- explore different market interpretations
- challenge assumptions
- stress-test market narratives
- identify overlooked risks or opportunities
- strengthen the quality of the final report

The analyst agents operate as professional analysts with different analytical perspectives rather than ideological positions.

It is completely valid for:
- all three analyst agents to arrive at a similar market conclusion
- two analyst agents to generally agree while one differs
- all three analyst agents to identify different risks and opportunities within the same broader market regime

Examples:
- All three analyst agents may conclude that market conditions remain structurally bullish while identifying different risks beneath the surface.
- The Bull and Balanced analysts may agree that AI infrastructure demand remains strong, while the Bear analyst focuses on valuation and inflation risks.
- The Bear analyst may acknowledge strong market momentum and liquidity conditions while still identifying fragile assumptions underneath the rally.

The goal of the analyst agent system is not conflict for the sake of conflict.

The goal is:
- analytical depth
- thesis stress-testing
- stronger final synthesis by the main agent

### Bull Analyst

The Bull Analyst focuses on constructive interpretations of the market environment.

The Bull Analyst is responsible for:
- identifying upside drivers
- identifying resilience in market structure
- identifying improving conditions
- challenging overly pessimistic assumptions
- explaining constructive market scenarios

The Bull Analyst does not ignore negative data or force bullish conclusions.

It acknowledges risks while focusing on evidence that supports continued market strength or improving conditions.

### Bear Analyst

The Bear Analyst focuses on identifying fragile assumptions and downside risks.

The Bear Analyst is responsible for:
- identifying downside risks
- identifying weakening conditions
- challenging complacency
- inspecting valuation and macroeconomic risks
- inspecting geopolitical, liquidity, and credit-related risks

The Bear Analyst does not deny bullish market conditions when supported by market data.

It acknowledges strength while focusing on hidden vulnerabilities, unsustainable narratives, and structural risks.

### Balanced Analyst

The Balanced Analyst focuses on weighing evidence and identifying the most probable market interpretation.

The Balanced Analyst is responsible for:
- separating signal from noise
- weighing bullish and bearish evidence
- assigning confidence levels
- separating short-term and long-term implications
- identifying conditions that would justify thesis changes

The Balanced Analyst does not attempt to remain artificially neutral.

It may produce bullish or bearish conclusions when evidence strongly supports them.

## Fixed Internal Models

Some internal workflows use non-configurable models for cost control and predictable performance. These are distinct from the user-configurable model selections covered in [configuration.md](configuration.md). The non-configurable model used for vector-memory embeddings is documented in [storage.md §Embeddings](storage.md#embeddings).

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
