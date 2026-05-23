# Market Signal MVP

## Overview
Market Signal is a local-first desktop application built with:
- Tauri
- Vue
- SQLite
- LanceDB

The app runs scheduled weekly market-analysis jobs, produces evolving market reports, stores recent report history, and uses memory retrieval to improve future analysis.

The weekly cadence is intentionally designed to prioritize signal over noise, structural market analysis, thesis continuity, and forward-looking market preparation rather than reactive daily commentary.

The application is not a trading bot. It acts as a professional market-analysis and thesis-generation system focused on:
- market regimes
- evolving macro theses
- geopolitical and economic developments
- sector analysis
- forward-looking market preparation
- investment strategy guidance

The application runs entirely on the user’s machine except for external API/model requests.

---

## Application Interface

### Main Layout
```text
Market Signal
├── Latest Report View
│   ├── Rendered HTML report
│   └── Export actions
│
├── Recent Reports Sidebar
│   ├── Ordered descending
│   ├── Report timestamps
│   └── Weekly Market reports
│
├── Research Documents
│   ├── Research Inbox
│   └── Research Archive
│
├── Persistent Warning Area
│   ├── Missing agent configuration
│   ├── Missing API tokens
│   ├── Failed jobs
│   └── Missed scheduled jobs
│
└── Settings
    ├── Agent model configuration
    ├── API token configuration
    ├── Scheduled job controls
    └── Manual report execution
```

---

## Settings
The Settings section includes:
- model selection
- API token configuration
- scheduled job enable/disable controls
- warning review and dismissal
- and manual job execution controls

---

## Scheduled Jobs

### Weekly Market Report Job
Runs:
```text
Sunday
9:00 AM local time
```

Focus:
- previous week's market behavior
- evolving macro thesis
- geopolitical and economic developments
- sector leadership and weakness
- inflation, rates, and liquidity conditions
- AI infrastructure and technology trends
- market positioning and sentiment
- forward-looking risks and opportunities
- upcoming market-moving events
- retrospective evaluation of prior assumptions and thesis evolution
- retrospective evaluation of prior report accuracy and thesis quality

---

## Job Execution and Scheduling
The application uses local scheduled jobs that run directly on the user’s machine.

Jobs are responsible for:
- generating Weekly Market reports

### Application Runtime Requirements
**Application Must Be Running**

Scheduled jobs only run while the application is running.

If the user fully quits the application:
- scheduled jobs do not run
- report generation stops
- no background processing occurs

Closing the application window does not quit the application if the app remains active in the system tray.

### System Sleep Behavior
Scheduled jobs do not run while the user’s machine is asleep.

Examples:
- laptop sleeping
- laptop lid closed
- suspended desktop state
- operating system sleep mode

If a scheduled execution time occurs while the machine is asleep:
- the job is skipped
- the application does not attempt to retroactively execute the missed job
- and the next scheduled job runs normally

### Offline Behavior
If the machine:
- loses internet connectivity
- cannot reach APIs
- cannot access configured model providers
the scheduled job fails cleanly.

A failed job is different from a missed job.

A failed job occurs when the application successfully starts the job execution process but cannot complete the workflow because required services, APIs, or model providers are unavailable.

The application:
- cancels the current job
- stores the failure state
- displays a warning inside the Persistent Warning Area

### Concurrent Job Protection
Only one report-generation workflow may run at a time.

If a Weekly Market report job is currently running and another scheduled or manual execution is attempted, the second execution is skipped.

The application logs the skipped execution.

### Job Status Visibility
The application displays:
- last successful run time
- currently running job state
- last failure state
- skipped job events

### Job Controls
Users can:
- Enable Weekly Market Job
- Disable Weekly Market Job

By default, all are enabled.

Scheduled jobs are enabled by default, but jobs cannot execute until all required agent models and provider credentials are configured.

A newly installed application therefore begins in an incomplete configuration state until the user finishes model and API setup.

### Manual Report Generation
The application includes manual execution controls in Settings for:
- Weekly Market Report

Manual execution follows the same workflow and validation rules as scheduled execution.

### Error Handling
If a job fails because of:
- API limits
- token exhaustion
- provider failures
- malformed responses
- model execution errors
the application:
1. cleanly cancels the job
2. stores the failure state
3. displays a warning inside the Persistent Warning Area

If the warning already exists and has not been dismissed/resolved:
- additional failing jobs do not create duplicate warnings.

### Missed Job Detection
The application detects when a scheduled job was missed because:
- the application was not running
- the machine was asleep
- the application could not start the scheduled execution during the scheduled window

Missed jobs are different from failed jobs.

A missed job means the scheduled execution never started.
A failed job means execution started but could not complete successfully.

When the application is next opened or resumed, it displays a warning inside the Persistent Warning Area indicating that the scheduled job was missed.

The user may:
- dismiss the warning
- manually execute the missed job immediately

Missed jobs are not automatically replayed or queued.

---

## Data Sources

### Market and Financial Data
**OpenBB:**
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

**Financial Modeling Prep:**
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

**FRED (Federal Reserve Economic Data):**
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

**BLS (Bureau of Labor Statistics):**
Docs - https://www.bls.gov/developers/

BLS provides official United States labor and inflation datasets.

Responsibilities:
- CPI reports
- employment reports
- wage data
- productivity data
- labor-market statistics

The application uses BLS data for inflation and labor-market analysis.

### News and Research
**Tavily:**
Docs - https://docs.tavily.com/welcome

Tavily provides AI-oriented web search and research retrieval.

Responsibilities:
- gathering relevant market news
- retrieving research sources
- identifying important developing stories
- supplying contextual research material to the agents

The application uses Tavily as the primary research and news-ingestion system.

**GDELT:**
Docs - https://www.gdeltproject.org/data.html

GDELT is a global event and news-monitoring platform that tracks worldwide news coverage and geopolitical developments.

Responsibilities:
- geopolitical monitoring
- conflict tracking
- global event detection
- international news analysis
- large-scale news trend identification

The application uses GDELT to strengthen geopolitical and macro event awareness.

### LLM Providers
- OpenAI
- Anthropic

---

## Research Document Workflow
The application contains two local folders:
```text
/research-inbox
/research-archive
```

### Research Inbox
The user can manually place documents into:
```text
/research-inbox
```

Supported formats:
- PDF
- Markdown
- TXT
- CSV
- JSON
- HTML

At the start of each scheduled job:
1. The main agent checks the inbox folder.
2. If the folder is empty, the job continues normally.
3. If documents exist, they are parsed and treated as professional research sources.
4. The documents are incorporated into the current research process.
5. After successful processing, the documents are automatically moved into:
```text
/research-archive
```

The user may manually delete documents from either folder.
The user cannot manually archive documents.

---

## Agent Pipeline Architecture
Market Signal uses a fixed multi-agent pipeline for each report.

The pipeline is not tool-driven by the main agent. The analyst agents are required stages that run for every report after the research packet is created.

The application layer executes external API calls and deterministic data retrieval.

Agents do not directly perform unbounded API access. When deeper research is needed, the main agent creates structured research requests that the application layer executes against configured data sources.

### Main Agent
The main agent acts as the Head Market Analyst.

The main agent is responsible for:
- planning market data and research needs
- creating structured research requests
- consuming curated data returned by the application layer
- dynamically guiding research priorities
- creating the condensed research packet
- retrieving relevant memory
- auditing prior report accuracy
- maintaining evolving long-term market theses
- evaluating analyst agent outputs
- synthesizing the final report
- publishing reports
- writing durable learnings

The main agent owns the final report.
The main agent is responsible for producing a cohesive weekly market publication that:
- evaluates the prior week's market behavior
- reviews evolving market theses
- identifies important structural developments
- prepares for future market-moving conditions
- synthesizes forward-looking market analysis

### Analyst Agents
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

The main agent evaluates all analyst agent outputs and determines how much weight to assign each perspective during final report generation.

---

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

---

## Model and API Configuration
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
- the Weekly Market job does not execute
- manual report execution is disabled
- the application displays a warning message on the homepage indicating which agents still require configuration

### External Data Provider Credentials
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

---

## Job Logical Flows

Market Signal has a single recurring job flow:
- the Weekly Market Report job

The Weekly Market Report workflow:
- analyzes the prior week's market behavior
- evaluates prior thesis accuracy
- performs dynamic and forward-looking research
- runs the analyst agents
- updates the long-term market thesis
- produces a new Market Signal report

The report combines:
- current market analysis
- retrospective thesis evaluation
- prior report accuracy auditing
- forward-looking market preparation
- long-term market-thesis evolution

---
## Weekly Market Report Job Flow

The Weekly Market Report workflow runs for:
- scheduled Weekly Market reports
- manual Weekly Market report generation

The Weekly Market Report focuses on synthesizing the previous week's market behavior, evaluating evolving macro and geopolitical conditions, updating the long-term market thesis, and identifying forward-looking risks and opportunities.

The report emphasizes:
- structural market developments
- major macroeconomic trends
- liquidity and valuation conditions
- sector leadership and weakness
- AI infrastructure and technology trends
- geopolitical developments
- market positioning and sentiment
- upcoming market-moving events

The report also performs retrospective auditing of prior Weekly Market reports to evaluate:
- thesis accuracy
- incorrect assumptions
- overlooked risks
- useful signals
- and whether prior market concerns evolved as expected

### Step 1: Job Start and Validation

The scheduled or manual job starts by loading application settings and validating that the job is allowed to run.

The application checks:
- whether the Weekly Market job is enabled
- whether another job is already running
- whether the Main Agent and all Analyst Agents are configured
- whether required API tokens exist for selected model providers
- whether the machine has network access to required APIs and model providers

If validation fails, the job does not continue. The application displays the appropriate warning state and avoids creating duplicate unresolved warnings.

### Step 2: Load Recent Report Context

The application loads a bounded set of recent Markdown reports and structured metadata.

Only Markdown reports are loaded for agent context. HTML reports are never loaded into agent prompts because HTML is a presentation artifact.

Structured metadata may include:
- creation timestamp
- market regime label
- report summary
- prior warnings or job status information

This recent context helps the main agent understand how the broader market thesis has evolved over time, which unresolved risks remain important, whether prior reports were directionally correct, and whether the current report should strengthen, weaken, or revise prior conclusions.

### Step 3: Audit Prior Reports

Before deeper synthesis begins, the main agent evaluates a bounded set of prior Weekly Market reports against actual market developments that occurred afterward.

The audit window should usually include the previous 2–6 Weekly Market reports, depending on relevance and context limits.

The retrospective audit process may evaluate:
- whether major market concerns materialized
- whether bullish or bearish expectations proved directionally correct
- whether risks were underestimated or overestimated
- whether market-moving events evolved differently than expected
- which analytical signals proved most useful
- whether the broader thesis strengthened or weakened over time

The goal of the audit system is not prediction scoring or numerical accuracy tracking.

The goal is:
- improving long-term analytical quality
- identifying weak assumptions
- reinforcing useful analytical patterns
- maintaining intellectual honesty
- and improving future market-thesis generation

The retrospective audit system behaves similarly to how professional research firms review prior theses and market calls over time.

### Step 4: Retrieve Relevant Vector Memory

The application queries LanceDB for relevant semantic memory before the main agent begins deeper reasoning.

Retrieved memory may include:
- report summaries
- durable learnings
- prior thesis changes
- important historical analogs
- past analytical mistakes
- recurring market patterns

Vector memory is used selectively. The system does not inject the full report history into the prompt.

### Step 5: Check Research Inbox

The application checks `/research-inbox` at the start of the report job.

Research document handling follows the `## Research Document Workflow` section.

Research documents may influence:
- the research packet
- analyst agent outputs
- the final report

### Step 6: Gather Baseline Market Data

The application gathers required baseline market data before agent reasoning begins.

Baseline market data is not optional and does not depend on the main agent deciding to request it.

The baseline scan includes:

Indices:
- Dow
- S&P 500
- Nasdaq
- Russell 2000

Market internals:
- VIX
- 2Y yield
- 10Y yield
- dollar index
- oil
- natural gas
- gold
- sector performance

Macro:
- Fed expectations
- CPI/PCE/jobs calendar
- inflation expectations
- consumer confidence
- major economic reports from the prior week

News categories:
- politics
- geopolitics
- China/trade
- energy
- earnings
- AI/semiconductors
- major economic developments

### Step 7: Gather and Filter News

The application gathers a broad set of headlines and research candidates from configured news and research sources.

The system does not send large raw news volumes into frontier models.

The news ingestion pipeline follows this bounded flow:

```text
~500 headlines gathered
→ deduplication
→ relevance scoring
→ clustering
→ ~40 relevant headlines
→ ~10 important stories
→ ~5 deeply analyzed topics
```

Headline filtering uses a fixed low-cost model for:
- filtering
- deduplication
- relevance scoring
- clustering headlines into major topics

This step reduces noise before the main agent performs deeper reasoning.

### Step 8: Perform Research Routing

Research routing determines which topics deserve deeper analysis for the current report.

The routing step considers:
- baseline market data
- filtered headline clusters
- recent Markdown report context
- relevant vector memory
- parsed research inbox documents
- upcoming known market-moving events

Research routing uses a fixed mid-tier model to decide which themes, sectors, macro issues, geopolitical events, or company-specific developments deserve deeper investigation.

The result is a bounded research plan. The research plan defines what should be investigated further without allowing unbounded agent loops or unlimited tool usage.

### Step 9: Perform Dynamic and Forward-Looking Research

The application executes the approved research plan against configured data sources and returns curated evidence to the main agent.

The research system is designed to analyze both current market conditions and known future developments that may materially impact markets over time.

The system does not operate purely as a reactive news-analysis engine focused only on the current day's headlines.

The main agent continuously evaluates:
- short-term developments
- medium-term macroeconomic and political events
- long-term structural trends

The research process should remain aware of known future events and begin incorporating their potential market impact before those events occur.

Examples include:
- presidential elections
- midterm elections
- central-bank policy cycles
- major economic reports
- debt ceiling events
- trade negotiations
- geopolitical escalation risks
- regulatory changes
- energy supply transitions
- long-term AI infrastructure buildouts

The system is expected to think similarly to a professional analyst team that prepares for future market-moving conditions well before they fully materialize.

The market thesis should therefore reflect:
- what shaped markets during the previous week
- what is likely developing next
- what longer-term structural forces may shape future market behavior

Dynamic branching examples:

```text
If oil spikes:
  Research inflation, shipping, supply disruptions, geopolitical escalation.

If yields rise sharply:
  Research Fed repricing, inflation expectations, bond market stress.

If semiconductors weaken:
  Research AI capex, export controls, datacenter demand, supply-chain risks.

If markets rally despite weak macro:
  Research positioning, liquidity, breadth, sentiment, FOMO dynamics.

If geopolitical tensions escalate:
  Research affected sectors, commodities, supply chains, inflation impact.
```

### Step 10: Build Condensed Research Packet

The main agent receives curated evidence and creates a condensed research packet.

The research packet is the canonical input for the analyst agents.

It may include:
- baseline market data
- filtered news clusters
- deep research findings
- source links
- recent Markdown report context
- relevant vector memory
- research inbox summaries
- unresolved thesis questions
- upcoming events that may affect the market thesis

The research packet must be concise enough to control token usage while still preserving the evidence needed for high-quality analysis.

### Step 11: Run Analyst Agents

After the research packet is created, the application runs three analyst agents:
- Bull Analyst
- Bear Analyst
- Balanced Analyst

These agents are not optional tools. They are fixed review stages in the report-generation pipeline.

Each analyst agent receives the same condensed research packet and produces structured analysis from its assigned analytical perspective.

Analyst agent outputs are ephemeral pipeline artifacts. They are not persisted independently unless specific insights are extracted into the final report or written as durable learnings.

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

### Step 12: Bull Analyst Review

The Bull Analyst focuses on constructive interpretations of the market environment.

The Bull Analyst is responsible for:
- identifying upside drivers
- identifying resilience in market structure
- identifying improving conditions
- challenging overly pessimistic assumptions
- explaining constructive market scenarios

The Bull Analyst does not ignore negative data or force bullish conclusions.

It acknowledges risks while focusing on evidence that supports continued market strength or improving conditions.

### Step 13: Bear Analyst Review

The Bear Analyst focuses on identifying fragile assumptions and downside risks.

The Bear Analyst is responsible for:
- identifying downside risks
- identifying weakening conditions
- challenging complacency
- inspecting valuation and macroeconomic risks
- inspecting geopolitical, liquidity, and credit-related risks

The Bear Analyst does not deny bullish market conditions when supported by market data.

It acknowledges strength while focusing on hidden vulnerabilities, unsustainable narratives, and structural risks.

### Step 14: Balanced Analyst Review

The Balanced Analyst focuses on weighing evidence and identifying the most probable market interpretation.

The Balanced Analyst is responsible for:
- separating signal from noise
- weighing bullish and bearish evidence
- assigning confidence levels
- separating short-term and long-term implications
- identifying conditions that would justify thesis changes

The Balanced Analyst does not attempt to remain artificially neutral.

It may produce bullish or bearish conclusions when evidence strongly supports them.

### Step 15: Main Agent Synthesis

The main agent receives:
- the original research packet
- Bull Analyst output
- Bear Analyst output
- Balanced Analyst output
- relevant memory
- report structure requirements

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

The final report is written in one unified voice as the Market Signal Thesis.

The report should behave like a professional weekly market publication focused on:
- thesis evolution
- structural market developments
- major macroeconomic and geopolitical forces
- retrospective auditing of prior reports
- forward-looking market preparation
- retrospective evaluation of prior assumptions

The report should not expose separate Bull/Bear/Balanced sections unless current market conditions specifically require multiple plausible market paths to be explained.

### Step 16: Save Report and Memory Outputs

The main agent writes the final report in Markdown.

The application saves:
- the Markdown report to persistent local storage
- report metadata to SQLite
- report summary to LanceDB
- durable learnings to LanceDB, if applicable

Durable learnings may include:
- mistakes the system should avoid repeating
- analytical strategies that proved useful
- thesis changes
- market patterns worth remembering
- historical analogs that became relevant

### Step 17: Generate HTML and Update UI

After the Markdown report is saved, the application generates the HTML version from Markdown.

The HTML version is used for:
- in-app rendering
- styling
- chart display
- PDF generation

Agents never ingest or reason over HTML reports.

After HTML generation succeeds, the application updates the Latest Report View and Recent Reports Sidebar.

---

---

## Analyst Skills
The following reusable skills are included in MVP.
These skills operate as structured reusable prompts with expected output schemas.

### Market Regime Analysis
Determines the current market regime and the dominant forces driving market behavior.

The skill evaluates whether the market is primarily:
- risk-on or risk-off
- liquidity-driven or earnings-driven
- inflation-sensitive or growth-sensitive
- whether market leadership is broadening or narrowing over time

### Narrative vs Reality
Separates genuine market or economic changes from exaggerated media narratives and short-term emotional reactions.
The skill evaluates whether market behavior is supported by underlying data, positioning, earnings, macro trends, and structural conditions rather than headlines alone.

### Second-Order Effects
Analyzes downstream consequences of major market, economic, geopolitical, or policy developments.

The skill maps how first-order events can propagate into:
- inflation
- yields
- liquidity
- sector performance
- consumer behavior
- long-term market conditions

### Inflation Decomposition
Breaks inflation into its underlying components and evaluates whether inflation pressure is temporary, structural, broadening, or narrowing.

The skill analyzes:
- energy
- shelter
- services
- wages
- transportation
- goods inflation separately rather than treating CPI as a single signal

### Historical Analog
Compares current market conditions to historical market environments and macroeconomic periods.

The skill identifies similarities and differences between current conditions and events such as:
- the dot-com bubble
- inflationary periods
- tightening cycles
- liquidity crises
- prior geopolitical or commodity shocks

### Positioning & Sentiment
Analyzes investor psychology, market positioning, and sentiment conditions.

The skill evaluates:
- fear and greed dynamics
- FOMO behavior
- crowded trades
- defensive positioning
- whether market behavior is becoming euphoric, complacent, or overly pessimistic

### Thesis Stress Test
Challenges the current market thesis and searches for weak assumptions or contradictory evidence.

The skill evaluates:
- what could invalidate the thesis
- which assumptions are fragile
- which signals are being ignored
- and what conditions would force a reassessment

### Geopolitical Escalation
Evaluates geopolitical developments and their potential market implications.

The skill analyzes:
- military conflicts
- trade tensions
- sanctions
- shipping disruptions
- commodity risks
- global supply-chain exposure

### AI Infrastructure Chain
Analyzes the AI infrastructure ecosystem and its broader market implications.

The skill evaluates:
- semiconductors
- datacenter buildouts
- HBM memory
- networking
- optics
- cooling
- power demand
- AI-related capital expenditure trends

### Time Horizon Separation
Separates short-term market reactions from medium-term and long-term structural market trends.
The skill helps prevent the system from confusing temporary volatility with meaningful changes to the broader market thesis.
The skill also helps the system distinguish between:
- weekly market noise
- cyclical developments
- structural long-term market shifts

### Credit Stress Analysis
Evaluates financial stress inside credit markets and identifies signs of tightening financial conditions.

The skill analyzes:
- credit spreads
- refinancing risk
- default pressure
- liquidity conditions
- commercial real estate stress
- broader systemic financial risk

### Energy Security Analysis
Analyzes energy-market stability and the macroeconomic implications of energy disruptions.

The skill evaluates:
- oil and natural gas supply
- OPEC activity
- shipping chokepoints
- grid stress
- energy-driven inflation risk
- the relationship between AI infrastructure growth and power demand

### Central Bank Interpretation
Interprets central-bank communication, policy decisions, and market expectations.

The skill evaluates:
- rate expectations
- liquidity conditions
- inflation priorities
- policy tone
- how central-bank positioning may affect equities, bonds, and broader market behavior

### Valuation Compression
Analyzes how interest rates, yields, and macroeconomic conditions may affect valuation multiples.

The skill focuses particularly on:
- long-duration growth assets
- high-multiple sectors
- whether earnings growth is sufficient to justify current valuations

### Market Breadth Analysis
Evaluates the health and participation level of the broader market beyond headline index performance.

The skill analyzes:
- advance/decline trends
- equal-weight vs cap-weight performance
- sector participation
- leadership concentration
- whether rallies or selloffs are broad-based or narrow

### Consensus vs Contrarian Analysis
Evaluates what the market currently expects versus what outcomes would genuinely surprise participants.

The skill helps identify:
- overconsensus narratives
- underappreciated risks
- asymmetric opportunities
- situations where market positioning may be vulnerable to unexpected developments
- areas where long-term market expectations may be mispriced

---

## Thesis Continuity and Evolution
The system maintains continuity between reports and treats market analysis as an evolving long-term process rather than a collection of disconnected market snapshots.

Each report exists within a broader market narrative that develops over time.

The main agent continuously:
- references recent reports
- retrieves relevant historical learnings from vector memory
- audits prior thesis accuracy
- follows up on prior market concerns
- tracks whether previous assumptions are strengthening or weakening
- updates long-term theses incrementally as new evidence appears

The system is designed to behave like a professional analyst team maintaining ongoing market coverage rather than a stateless news summarizer.

### Report Continuity
Reports should naturally flow from previous reports.

Examples:
```text
Previous report:
"The primary market risk remains whether elevated oil prices begin bleeding into core inflation."

Next report:
"That concern increased after core CPI accelerated while oil remained elevated."

Later report:
"Inflation pressure has not yet materially damaged AI infrastructure spending, but rising yields are becoming a larger risk to valuation multiples."
```

The system should:
- continue monitoring unresolved market risks
- revisit previous conclusions
- acknowledge when earlier assumptions were incorrect
- identify when a thesis is strengthening or weakening

The system should also periodically evaluate:
- whether prior concerns evolved as expected
- whether the system overemphasized unimportant narratives
- whether important signals were missed
- and whether the broader market thesis remained directionally correct

### Thesis Stability
The system should avoid unnecessary thesis instability.

Long-term market theses should evolve gradually when:
- market conditions remain structurally similar
- existing narratives continue holding
- incoming data reinforces prior conclusions

The system should not dramatically change positioning or outlook because of isolated short-term volatility, temporary news cycles, or single-event reactions.

The main agent should prioritize:
- signal over noise
- multi-week confirmation when appropriate
- and structural changes over temporary volatility

### Thesis Pivot Conditions
The system may rapidly pivot its outlook when major evidence materially changes the market environment.

Major thesis pivots should remain relatively rare and should only occur when evidence strongly suggests that structural market conditions have materially changed.

Examples include:
- major geopolitical escalation
- financial system stress
- persistent inflation regime shifts
- abrupt central bank policy changes
- major recession indicators
- significant AI infrastructure slowdown
- supply-chain disruptions
- systemic credit events
- major energy disruptions

In these situations:
- reports may heavily focus on the new event
- prior assumptions may be explicitly challenged
- the long-term thesis may be revised aggressively

The system should clearly explain:
- why the thesis changed
- which assumptions failed
- what evidence caused the pivot

### Memory-Guided Evolution
The vector memory system exists to help the main agent maintain analytical continuity over time.

The main agent uses memory retrieval to:
- identify similar historical conditions
- revisit previous conclusions
- track recurring market patterns
- avoid repeating analytical mistakes
- maintain coherent long-term reasoning across reports

The goal is not rigid consistency.

The goal is:
- coherent reasoning
- gradual evolution when appropriate
- decisive adaptation when necessary

---

## Report Structure
Reports are written, authored, and stored internally in Markdown by the main agent. An HTML version is generated for application display.

Markdown is the canonical report format used for:
- agent memory
- report continuity
- vector memory ingestion
- report retrieval
- future report synthesis

HTML reports are generated from Markdown and are presentation-only artifacts used for:
- in-app rendering
- styling
- chart display
- PDF generation

Agents never ingest or reason over HTML reports.

### Standard Report Structure
```text
# Weekly Market Report

Date
Report Type:
- Weekly Market Report

## Header Summary
3–6 key bullets summarizing the most important conclusions, risks, developments, and thesis changes.

## Market Regime
Current market regime assessment and the dominant forces driving market behavior.

## Index Picture
Brief high-level overview of:
- Dow
- S&P 500
- Nasdaq

This section is intentionally concise and serves as a quick market snapshot rather than a detailed breakdown.

## Key Market Drivers

Primary developments currently influencing markets.

This section is dynamic and may include topics such as:
- Inflation / Federal Reserve
- Energy
- AI / Semiconductors
- China / Geopolitics
- Consumer Strength or Weakness
- Earnings
- Liquidity / Credit
- Market Breadth
- Major Economic Reports
- Elections / Political Developments
- Global Conflicts
- Sector Rotation
- Currency Markets

The importance, ordering, size, and presentation of topics may vary significantly between reports depending on current market conditions.

Sections may include:
- charts,
- graphs,
- tables,
- earnings analysis,
- macroeconomic breakdowns,
- geopolitical analysis,
- or deeper long-form commentary when appropriate.

The report should emphasize the topics most materially affecting the market at that time rather than forcing equal coverage across all categories.

## Market Signal Thesis

This section may also include retrospective evaluation of prior reports when meaningful thesis confirmations, failures, or analytical mistakes occurred.

The primary market thesis synthesized by the Head Market Analyst after evaluating:
- market data,
- research,
- analyst agent outputs,
- historical context,
- and memory retrieval.

This section represents the unified voice of the system rather than separate Bull/Bear/Balanced outputs.

The thesis may:
- lean bullish,
- lean bearish,
- remain mixed,
- or heavily emphasize uncertainty depending on current market conditions.

If conditions are unusually uncertain or bifurcated, the thesis may explicitly discuss multiple plausible market paths and the signals that would support each outcome.

## Retrospective Audit

Evaluation of prior Weekly Market reports and whether previous assumptions, risks, and market expectations evolved as anticipated.

This section may discuss:
- thesis confirmations
- incorrect assumptions
- missed risks
- signal quality
- overemphasized narratives
- useful analytical patterns
- and meaningful thesis changes

This section is dynamic and only expands when meaningful retrospective analysis is warranted.

## Investment Strategy

High-level investment guidance based on current market conditions and evolving market theses.

This section may include:
- sectors to monitor,
- industries benefiting from current trends,
- industries under pressure,
- ETFs/themes of interest,
- short/mid/long-term opportunities,
- defensive positioning,
- macro-sensitive positioning,
- or areas where risk/reward appears asymmetric.

The application does not provide direct buy/sell instructions or trade execution guidance.

## Forward Outlook

Key themes, risks, opportunities, and developments likely to influence markets over the coming weeks and months.

This section may discuss:
- evolving macroeconomic conditions
- upcoming market-moving events
- structural market trends
- geopolitical risks
- sector leadership changes
- liquidity and valuation conditions
- long-term opportunities or threats

## Watchlist

Key:
- events,
- economic reports,
- earnings releases,
- geopolitical developments,
- and market signals

that should be monitored in upcoming weeks and report cycles.

## Sources
```

---

## Storage

### SQLite
Stores:
- report records
- report metadata
- HTML output
- job history
- warning states

Each report stores:
- creation timestamp
- structured report summary metadata
- market regime metadata

Only the most recent 30 Weekly Market reports are retained.

Older reports are deleted automatically.

When a report is removed:
- its Markdown
- generated HTML
- metadata
- associated vector-memory summary references
are deleted together.

### LanceDB Vector Memory
Stores:
- report summaries
- durable learnings
- thesis evolution
- important historical analogs
- past mistakes
- retrospective audit learnings
- useful recurring patterns

The vector DB acts as long-term semantic memory for the main agent.

Deleting older reports does not remove durable learnings already stored in vector memory.

This allows the system to preserve long-term analytical continuity even while older report files are removed from local storage.

---

## Export System
Reports are authored and stored internally as Markdown. The application also generates an HTML version for in-app display and PDF generation.

### Export Options
Users can export a Weekly Market report as:
- Markdown
- PDF

### Markdown Export
Markdown export uses the canonical Markdown report.

Markdown exports preserve:
- report structure
- headings
- source links
- written analysis
- and any Markdown-compatible tables or lists

### PDF Export
PDF export is generated from the HTML report version.

PDF exports preserve:
- rendered report styling
- charts, graphs, and tables included in the HTML report
- source links when supported by the PDF renderer
- and the full written report content

### Export Naming
Exported files use the report date and report title in the filename.

Example:
```text
2026-05-24-market-signal-weekly-report.md
2026-05-24-market-signal-weekly-report.pdf
```

### Export Behavior
Exports are generated from the stored report artifacts.

Exporting a report does not re-run the agent workflow, regenerate analysis, or modify the stored report.
