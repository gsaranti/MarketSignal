# Weekly Market Report Workflow

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

For when this job runs and how it interacts with sleep, offline, and concurrent-execution conditions, see [scheduling.md](scheduling.md).

## Step 1: Job Start and Validation

The scheduled or manual job starts by loading application configuration and validating that the job is allowed to run.

The application checks:
- whether the Weekly Market job is enabled
- whether another job is already running
- whether the Main Agent and all Analyst Agents are configured
- whether the required OpenAI and Anthropic API tokens exist (both are always required — see [configuration.md §API Tokens](configuration.md#api-tokens))
- whether the required external data provider credentials are configured
- whether the machine has network access to required APIs and model providers

If validation fails, the job does not continue. The application displays the appropriate warning state and avoids creating duplicate unresolved warnings.

The canonical rules for each check live in:
- enable/disable state and concurrent-job protection: [scheduling.md](scheduling.md)
- agent model configuration and API token requirements: [configuration.md](configuration.md)
- external data provider credential requirements: [configuration.md §External Data Provider Credentials](configuration.md#external-data-provider-credentials)
- offline / unreachable-provider behavior: [scheduling.md §Offline Behavior](scheduling.md#offline-behavior)

## Step 2: Load Recent Report Context

The application loads a bounded set of recent Markdown reports and structured metadata before passing relevant context to the main agent.

Only Markdown reports are loaded for agent context. HTML reports are never loaded into agent prompts because HTML is a presentation artifact. See [report-structure.md](report-structure.md) for the canonical Markdown-vs-HTML rule.

Structured metadata may include:
- creation timestamp
- market regime labels (risk posture and market cycle)
- report summary
- prior warnings or job status information

This recent context helps the main agent understand how the broader market thesis has evolved over time, which unresolved risks remain important, whether prior reports were directionally correct, and whether the current report should strengthen, weaken, or revise prior conclusions.

## Step 3: Audit Prior Reports

Before deeper synthesis begins, the application supplies prior report context and actual market developments to the main agent. The main agent then evaluates a bounded set of prior Weekly Market reports against what occurred afterward.

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

## Step 4: Retrieve Relevant Vector Memory

The application queries LanceDB for relevant semantic memory before the main agent begins deeper reasoning, then supplies the retrieved memory fragments to the main agent.

Retrieved memory may include:
- report summaries
- durable learnings
- prior thesis changes
- important historical analogs
- past analytical mistakes
- recurring market patterns

Vector memory is used selectively. The system does not inject the full report history into the prompt.

For what is stored in vector memory and the retention rules around it, see [storage.md §LanceDB Vector Memory](storage.md#lancedb-vector-memory). For how memory shapes the main agent's reasoning across reports, see [thesis-continuity.md §Memory-Guided Evolution](thesis-continuity.md#memory-guided-evolution).

## Step 5: Check Research Inbox

The application checks `/research-inbox` at the start of the report job.

Research document handling follows the workflow defined in [research-documents.md](research-documents.md).

Research documents may influence:
- the research packet
- analyst agent outputs
- the final report

## Step 6: Gather Baseline Market Data

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

## Step 7: Gather and Filter News

The application gathers a broad set of headlines and research candidates from the configured news and research sources — Tavily and GDELT (see [data-sources.md](data-sources.md)). Tavily contributes AI-oriented market and research headlines; GDELT contributes geopolitical and large-scale news trend coverage.

The system does not send large raw news volumes into frontier models.

The news ingestion pipeline follows this bounded flow:

```text
~500 headlines gathered
→ deduplication
→ relevance scoring
→ clustering
→ ~40 relevant headlines
→ ~10 important stories
```

The application uses a fixed low-cost model for headline filtering tasks:
- filtering
- deduplication
- relevance scoring
- clustering headlines into major topics

For the specific model used and its rationale, see [agents.md §Headline Filtering](agents.md#headline-filtering).

This step reduces noise before the main agent performs deeper reasoning. The headline-filtering model's output is this bounded set of clustered important stories; selecting which of them become the ~5 deeply analyzed topics is the job of research routing ([Step 8](#step-8-perform-research-routing)), and the deep analysis itself runs in [Step 9](#step-9-perform-dynamic-and-forward-looking-research).

## Step 8: Perform Research Routing

Research routing determines which topics deserve deeper analysis for the current report. The routing model produces a structured research plan, and the application layer is responsible for executing that plan against configured data sources.

The routing step considers:
- baseline market data
- filtered headline clusters
- recent Markdown report context
- relevant vector memory
- parsed research inbox documents
- upcoming known market-moving events

Research routing uses a fixed mid-tier model to decide which themes, sectors, macro issues, geopolitical events, or company-specific developments deserve deeper investigation. For the specific model used and its rationale, see [agents.md §Research Routing](agents.md#research-routing).

The result is a bounded research plan. The research plan defines what should be investigated further without allowing unbounded agent loops or unlimited tool usage.

## Step 9: Perform Dynamic and Forward-Looking Research

The application executes the bounded research plan produced in Step 8 against configured data sources, applies workflow limits, and returns curated evidence to the main agent.

Workflow limits:
- maximum 50 research requests per job
- maximum duration of 30 minutes for the research phase
- maximum dynamic-branching depth of 2 (a research request may spawn at most one follow-up)

These limits bound the research phase, which is the only stage that can loop or branch. The remaining stages — the analyst reviews and the main agent's synthesis — are fixed single-pass runs and carry no separate overall time budget; stuck or failing model calls in any stage are handled as job failures (see [scheduling.md §Error Handling](scheduling.md#error-handling)).

The research system is designed to analyze both current market conditions and known future developments that may materially impact markets over time.

The system does not operate purely as a reactive news-analysis engine focused only on the current day's headlines.

The main agent uses the curated research evidence to evaluate:
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

## Step 10: Build Condensed Research Packet

The main agent receives curated evidence from the application layer and creates a condensed research packet.

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

## Step 11: Run Analyst Agents

After the research packet is created, the application runs three analyst agents:
- Bull Analyst
- Bear Analyst
- Balanced Analyst

Each analyst agent receives the same condensed research packet and produces structured analysis from its assigned analytical perspective.

The three analyst agents are independent and run concurrently — each works only from the shared research packet, so there is no ordering dependency between them. Steps 12–14 document each analyst's review individually; their numbering is not an execution order.

Analyst agent outputs are ephemeral pipeline artifacts. They are not persisted independently unless specific insights are extracted into the final report or written as durable learnings.

For each analyst agent's responsibilities, posture, and the shared analytical purpose of the analyst stage, see [agents.md §Analyst Agents](agents.md#analyst-agents).

## Step 12: Bull Analyst Review

The Bull Analyst runs its review against the condensed research packet. For the Bull Analyst's responsibilities and posture, see [agents.md §Bull Analyst](agents.md#bull-analyst).

## Step 13: Bear Analyst Review

The Bear Analyst runs its review against the condensed research packet. For the Bear Analyst's responsibilities and posture, see [agents.md §Bear Analyst](agents.md#bear-analyst).

## Step 14: Balanced Analyst Review

The Balanced Analyst runs its review against the condensed research packet. For the Balanced Analyst's responsibilities and posture, see [agents.md §Balanced Analyst](agents.md#balanced-analyst).

## Step 15: Main Agent Synthesis

The main agent receives:
- the original research packet
- Bull Analyst output
- Bear Analyst output
- Balanced Analyst output
- relevant memory
- report structure requirements

For the synthesis behavior the main agent applies — independent critique, allowed actions during synthesis, unified-voice constraint, and editorial focus — see [agents.md §Main Agent](agents.md#main-agent).

## Step 16: Save Report and Memory Outputs

The main agent writes the final report in Markdown.

The application saves:
- the Markdown report to persistent local storage
- report metadata to SQLite
- report summary to LanceDB
- durable learnings identified by the main agent to LanceDB, if applicable

Durable learnings may include:
- mistakes the system should avoid repeating
- analytical strategies that proved useful
- thesis changes
- market patterns worth remembering
- historical analogs that became relevant

For what is stored in each store, retention rules, and deletion behavior, see [storage.md](storage.md).

## Step 17: Generate HTML and Update UI

After the Markdown report is saved, the application generates the HTML version from Markdown.

The HTML version is used for:
- in-app rendering
- styling
- chart display
- PDF generation

Agents never ingest or reason over HTML reports. See [report-structure.md](report-structure.md) for the canonical Markdown-vs-HTML rule.

After HTML generation succeeds, the application updates the Latest Report View and Recent Reports Sidebar.
