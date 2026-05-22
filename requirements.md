# Market Signal MVP

## Overview
Market Signal is a local-first desktop application built with:
- Tauri
- Vue
- SQLite
- LanceDB

The app runs scheduled market-analysis jobs, produces evolving market reports, stores recent report history, and uses memory retrieval to improve future analysis.

The application is not a trading bot. It acts as a professional market-analysis system focused on:
- market regimes
- evolving macro theses
- geopolitical and economic developments
- sector analysis
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
│   ├── Report type labels
│   ├── Report timestamps
│   ├── Premarket reports
│   ├── Postmarket reports
│   └── Weekly review reports
│
├── Research Documents
│   ├── Research Inbox
│   └── Research Archive
│
├── Warning Banner Area
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
- warning visibility
- and manual job execution controls

---

## Scheduled Jobs

### Premarket Report Job
Runs:
```text
Monday–Friday mornings
4:00 AM PT / 7:00 AM ET
```

Focus:
- overnight futures
- global markets
- macro calendar
- geopolitical developments
- overnight earnings/news
- expected market drivers

### Postmarket Report Job
Runs:
```text
Sunday–Friday evenings
4:00 PM PT / 7:00 PM ET
```

Focus:
- what moved markets
- index performance
- sector leadership
- macro reactions
- yields/oil/dollar/VIX
- thesis evolution
- next-day setup

### Weekly Review Job
Runs:
```text
Saturday
9:00 AM local time
```

Focus:
- analyze the previous week’s reports
- judge accuracy
- identify incorrect assumptions
- identify useful signals
- extract durable lessons

The weekly review is stored as a normal readable report inside the application.

---

## Job Execution and Scheduling
The application uses local scheduled jobs that run directly on the user’s machine.

Jobs are responsible for:
- generating premarket reports
- generating postmarket reports
- generating the weekly review report

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

The application:
- cancels the current job
- stores the failure state
- displays a warning banner

### Concurrent Job Protection
Only one scheduled job may run at a time.

If a report job is currently running and another scheduled job time occurs, the second job is skipped.

The application logs the skipped execution.

### Job Status Visibility
The application displays:
- last successful run time
- currently running job state
- last failure state
- skipped job events

### Job Controls
Users can:
- Enable Premarket Job
- Disable Premarket Job
- Enable Postmarket Job
- Disable Postmarket Job
- Enable Weekly Review Job
- Disable Weekly Review Job

By default, all are enabled.

### Manual Report Generation
The application includes manual execution controls for:
- Premarket Report
- Postmarket Report
- Weekly Review

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
2. displays a warning banner inside the application

If the warning already exists and has not been dismissed/resolved:
- additional failing jobs do not create duplicate warnings.

### Missed Job Detection
The application detects when a scheduled job was missed because:
- the application was not running
- the machine was asleep
- the machine was offline during the scheduled execution window

When the application is next opened or resumed, it displays a notification indicating that the scheduled job was missed.

The user may:
- dismiss the notification
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

The application uses OpenBB to normalize and simplify financial-data retrieval workflows.

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

The application uses Financial Modeling Prep as a direct structured financial-data source.

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

### Main Agent
The main agent acts as the Head Market Analyst.

The main agent is responsible for:
- gathering market data
- gathering news and research
- dynamically branching research
- creating the condensed research packet
- retrieving relevant memory
- maintaining evolving market theses
- reviewing analyst agent outputs
- synthesizing the final report
- publishing reports
- writing durable learnings

The main agent owns the final report.

### Analyst Agents
Three analyst agents run after the main agent creates the condensed research packet:
- Bull Analyst
- Bear Analyst
- Balanced Analyst

These agents are not optional tools. They are fixed review stages in the report-generation pipeline.

Each analyst agent receives the same condensed research packet and produces a structured analysis from its assigned analytical perspective.

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

## Job Logical Flows

Market Signal has two distinct job flows:
- the recurring market report job
- the weekly review job

The recurring market report job is used for both Premarket and Postmarket reports. It gathers current market context, performs dynamic research, runs the analyst agents, and produces a new Market Signal report.

The weekly review job is retrospective. It evaluates the prior week's reports against actual market developments, identifies useful or flawed analysis, and writes durable learnings into vector memory.

---

## Recurring Market Report Job Flow

The recurring market report job runs for:
- Premarket reports
- Postmarket reports
- manual Premarket report generation
- manual Postmarket report generation

Premarket and Postmarket jobs use the same logical workflow, but the research emphasis differs by report type.

Premarket reports focus on:
- overnight futures
- global market activity
- upcoming macro events
- geopolitical developments
- overnight earnings/news
- likely market drivers for the coming session

Postmarket reports focus on:
- what moved markets during the day
- index and sector performance
- macro reactions
- yields/oil/dollar/VIX behavior
- thesis evolution
- the next market setup

### Step 1: Job Start and Validation

The scheduled or manual job starts by loading application settings and validating that the job is allowed to run.

The application checks:
- whether the relevant job type is enabled
- whether another job is already running
- whether all required agent models are configured
- whether required API tokens exist for selected model providers
- whether the machine has network access to required APIs and model providers

If validation fails, the job does not continue. The application displays the appropriate warning state and avoids creating duplicate unresolved warnings.

### Step 2: Load Recent Report Context

The application loads a bounded set of recent Markdown reports and structured metadata.

Only Markdown reports are loaded for agent context. HTML reports are never loaded into agent prompts because HTML is a presentation artifact.

Structured metadata may include:
- report type
- creation timestamp
- market session metadata
- market regime label
- report summary
- prior warnings or job status information

This recent context helps the main agent understand what the system previously believed, what risks were being monitored, and whether the current report should follow up on unresolved themes.

### Step 3: Retrieve Relevant Vector Memory

The application queries LanceDB for relevant semantic memory before the main agent begins deeper reasoning.

Retrieved memory may include:
- report summaries
- durable learnings
- prior thesis changes
- important historical analogs
- past analytical mistakes
- recurring market patterns

Vector memory is used selectively. The system does not inject the full report history into the prompt.

### Step 4: Check Research Inbox

The application checks `/research-inbox` at the start of the report job.

If the folder is empty, the job continues normally.

If documents exist, they are parsed and incorporated as professional research sources for the current report. These documents may influence the research packet, analyst agent outputs, and final report.

After successful processing, the documents are moved automatically into `/research-archive`.

The user may manually delete documents from either folder. The user cannot manually archive documents.

### Step 5: Gather Baseline Market Data

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

News categories:
- politics
- geopolitics
- China/trade
- energy
- earnings
- AI/semiconductors
- major economic developments

### Step 6: Gather and Filter News

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

### Step 7: Perform Research Routing

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

### Step 8: Perform Dynamic and Forward-Looking Research

The application executes the approved research plan against configured data sources and returns curated evidence to the main agent.

The research system is designed to analyze both current market conditions and known future developments that may materially impact markets over time.

The system does not operate purely as a reactive news-analysis engine focused only on the current day’s headlines.

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
- what is happening now
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

### Step 9: Build Condensed Research Packet

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

### Step 10: Run Analyst Agents

After the research packet is created, the application runs three analyst agents:
- Bull Analyst
- Bear Analyst
- Balanced Analyst

These agents are not optional tools. They are fixed review stages in the report-generation pipeline.

Each analyst agent receives the same condensed research packet and produces structured analysis from its assigned analytical perspective.

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

### Step 11: Bull Analyst Review

The Bull Analyst focuses on constructive interpretations of the market environment.

The Bull Analyst is responsible for:
- identifying upside drivers
- identifying resilience in market structure
- identifying improving conditions
- challenging overly pessimistic assumptions
- explaining constructive market scenarios

The Bull Analyst does not ignore negative data or force bullish conclusions.

It acknowledges risks while focusing on evidence that supports continued market strength or improving conditions.

### Step 12: Bear Analyst Review

The Bear Analyst focuses on identifying fragile assumptions and downside risks.

The Bear Analyst is responsible for:
- identifying downside risks
- identifying weakening conditions
- challenging complacency
- inspecting valuation and macroeconomic risks
- inspecting geopolitical, liquidity, and credit-related risks

The Bear Analyst does not deny bullish market conditions when supported by market data.

It acknowledges strength while focusing on hidden vulnerabilities, unsustainable narratives, and structural risks.

### Step 13: Balanced Analyst Review

The Balanced Analyst focuses on weighing evidence and identifying the most probable market interpretation.

The Balanced Analyst is responsible for:
- separating signal from noise
- weighing bullish and bearish evidence
- assigning confidence levels
- separating short-term and long-term implications
- identifying conditions that would justify thesis changes

The Balanced Analyst does not attempt to remain artificially neutral.

It may produce bullish or bearish conclusions when evidence strongly supports them.

### Step 14: Main Agent Synthesis

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

The final report is written in one unified voice as the Market Signal Thesis. The report should not expose separate Bull/Bear/Balanced sections unless current market conditions specifically require multiple plausible paths to be explained.

### Step 15: Save Report and Memory Outputs

The main agent writes the final report in Markdown.

The application saves:
- the Markdown report to SQLite
- report metadata to SQLite
- report summary to LanceDB
- durable learnings to LanceDB, if applicable

Durable learnings may include:
- mistakes the system should avoid repeating
- analytical strategies that proved useful
- thesis changes
- market patterns worth remembering
- historical analogs that became relevant

### Step 16: Generate HTML and Update UI

After the Markdown report is saved, the application generates the HTML version from Markdown.

The HTML version is used for:
- in-app rendering
- styling
- chart display
- PDF generation

Agents never ingest or reason over HTML reports.

After HTML generation succeeds, the application updates the Latest Report View and Recent Reports Sidebar.

---

## Weekly Review Job Flow

The weekly review job runs once per week and produces a Weekly Review report.

The weekly review job is different from the recurring market report job. It is not primarily focused on producing a new market outlook. Its main purpose is retrospective evaluation, thesis review, and memory improvement.

### Step 1: Weekly Review Job Start and Validation

The weekly review job starts by loading application settings and validating that the job is allowed to run.

The application checks:
- whether the Weekly Review job is enabled
- whether another job is already running
- whether all required agent models are configured
- whether required API tokens exist for selected model providers
- whether the machine has network access to required APIs and model providers

If validation fails, the job does not continue. The application displays the appropriate warning state and avoids creating duplicate unresolved warnings.

### Step 2: Load Previous Week's Reports

The application loads the previous week's Markdown reports and structured metadata.

Only Markdown reports are loaded for review. HTML reports are never loaded into agent prompts.

The weekly review may include:
- Premarket reports
- Postmarket reports
- prior weekly review context when relevant
- report metadata
- market session metadata
- previous market regime labels
- previous report summaries

### Step 3: Gather Actual Market Developments

The application gathers market data and relevant news needed to evaluate what actually happened after the reports were written.

This may include:
- index performance
- sector performance
- yield movement
- oil and commodity movement
- major macro data releases
- major earnings reactions
- geopolitical developments
- liquidity and credit signals

This step gives the weekly review enough context to judge prior analysis against market outcomes and new evidence.

### Step 4: Review Thesis Evolution

The main agent evaluates how the system's market thesis changed throughout the week.

The review considers:
- whether prior assumptions strengthened or weakened
- whether reports followed up on unresolved risks
- whether the system adapted appropriately to new evidence
- whether major thesis pivots were justified
- whether the system overreacted to noise or underreacted to meaningful signals

### Step 5: Identify Correct Calls and Incorrect Assumptions

The weekly review identifies where prior reports were analytically useful and where they were wrong or incomplete.

Correct calls may include:
- risks that were identified before they mattered
- market shifts that were anticipated correctly
- structural developments that were interpreted well
- signals that proved useful

Incorrect assumptions may include:
- conclusions that were wrong
- weak assumptions
- missed risks
- overemphasized narratives
- underweighted signals
- situations where the system misread market conditions

The goal is honest analytical review rather than defending prior conclusions.

### Step 6: Evaluate Signals

The weekly review evaluates which market signals mattered most during the week.

Signals may include:
- yields
- breadth
- energy prices
- inflation data
- liquidity
- earnings strength
- geopolitical developments
- positioning/sentiment behavior

The review should also identify signals that were expected to matter but were ultimately less important than anticipated.

### Step 7: Generate Weekly Review Report

The main agent generates a Weekly Review report in Markdown.

The Weekly Review report includes:
- Weekly Summary
- Major Market Drivers
- Thesis Review
- Correct Calls
- Incorrect Assumptions
- Signal Evaluation
- Thesis Changes
- Durable Learnings
- Forward Watchlist
- Sources

The weekly review report appears in the normal report history UI.

### Step 8: Write Durable Learnings to Vector Memory

The main agent writes durable learnings from the weekly review into LanceDB when the learnings are useful for future analysis.

Durable learnings may include:
- recurring market patterns
- signals that proved more important than expected
- signals that were overemphasized
- thesis-management mistakes
- improved research strategies
- useful historical analogs

These learnings help the system improve future reports without requiring full historical report context to be injected into every prompt.

### Step 9: Save Weekly Review and Update UI

The application saves:
- the Weekly Review Markdown report to SQLite
- report metadata to SQLite
- durable learnings to LanceDB, if applicable

The application generates the HTML version from Markdown and updates the Latest Report View and Recent Reports Sidebar.

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
- whether market leadership is broadening or narrowing

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
Separates short-term market reactions from medium-term and long-term structural trends.
The skill helps prevent the system from confusing temporary volatility with meaningful changes to the broader market thesis.

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

---

## Thesis Continuity and Evolution
The system maintains continuity between reports and treats market analysis as an evolving long-term process rather than a collection of disconnected daily summaries.

Each report exists within a broader market narrative that develops over time.

The main agent continuously:
- references recent reports
- retrieves relevant historical learnings from vector memory
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

### Thesis Stability
The system should avoid unnecessary thesis instability.

Long-term market theses should evolve gradually when:
- market conditions remain structurally similar
- existing narratives continue holding
- incoming data reinforces prior conclusions

The system should not dramatically change positioning or outlook because of isolated single-day market moves or short-lived news cycles.

The main agent should prioritize:
- signal over noise
- multi-day/multi-week confirmation
- and structural changes over temporary volatility

### Thesis Pivot Conditions
The system may rapidly pivot its outlook when major evidence materially changes the market environment.

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
Reports are written in Markdown by the main agent. An HTML version is generated for application display.

Reports are authored and stored internally as Markdown.

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
# Market Signal Report

Date
Report Type:
- Premarket
- Postmarket

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

## Short-Term Outlook

Key themes, risks, and developments likely to influence markets over the near term.

## Long-Term Outlook

Longer-term structural market themes, risks, and opportunities.

## Watchlist

Key:
- events,
- economic reports,
- earnings releases,
- geopolitical developments,
- and market signals

that should be monitored going forward.

## Sources
```

### Weekly Review Report Structure
```text
# Market Signal Weekly Review Report

Date
Review Period

## Weekly Summary

High-level summary of:
- the week's market behavior,
- major developments,
- thesis evolution,
- and overall market conditions.

## Major Market Drivers

The most important events, themes, and developments that influenced markets during the review period.

This section may include:
- inflation developments,
- Federal Reserve expectations,
- geopolitical escalation,
- earnings reactions,
- liquidity changes,
- energy market shifts,
- AI infrastructure developments,
- elections or political developments,
- and major macroeconomic signals.

## Thesis Review

Evaluation of how the system's market thesis evolved throughout the week.

This section reviews:
- whether previous assumptions strengthened or weakened,
- whether the system adapted appropriately to new evidence,
- and whether major thesis pivots were justified.

## Correct Calls

Analysis areas where the system's prior reports correctly identified:
- important risks,
- opportunities,
- market shifts,
- or structural developments.

This section focuses on meaningful analytical accuracy rather than isolated lucky predictions.

## Incorrect Assumptions

Analysis of:
- incorrect conclusions,
- weak assumptions,
- missed risks,
- overemphasized narratives,
- or situations where the system misread market conditions.

The goal is honest analytical review rather than defending prior conclusions.

## Signal Evaluation

Evaluation of which signals proved most useful during the week.

Examples:
- yields,
- breadth,
- energy prices,
- inflation data,
- liquidity,
- earnings strength,
- geopolitical developments,
- or positioning/sentiment behavior.

This section also identifies signals that were ultimately less important than expected.

## Thesis Changes

Summary of:
- meaningful changes to the long-term market thesis,
- evolving macroeconomic expectations,
- structural risks,
- and major market narratives that materially shifted during the week.

## Durable Learnings

Longer-term analytical lessons extracted from the week's reports and market behavior.

These learnings may be written into vector memory for future report generation and thesis continuity.

## Forward Watchlist

Key upcoming developments the system believes are likely to matter in the coming weeks.

Examples:
- economic reports,
- elections,
- Federal Reserve meetings,
- geopolitical developments,
- earnings cycles,
- energy market risks,
- or structural market signals.

## Sources
```

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

---

## Storage

### SQLite
Stores:
- reports
- report metadata
- HTML output
- job history
- warning states

Each report stores:
- report type
- creation timestamp
- associated market session metadata

Only the most recent 30 full reports are retained. Older reports are deleted automatically.

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
- useful recurring patterns

The vector DB acts as long-term semantic memory for the main agent.

Deleting older reports does not remove durable learnings already stored in vector memory.

---

## Export System
Reports are stored internally as:
- Markdown
- HTML

### Export Options
- Export Markdown
- Export PDF

PDF export is generated from the HTML report version.
