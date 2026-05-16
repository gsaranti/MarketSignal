# Market Analyzer MVP

## Overview
Market Analyzer is a local-first desktop application built with:
- Tauri
- Vue
- SQLite
- LanceDB

The app runs scheduled market-analysis jobs, produces evolving market reports, stores recent report history, and uses memory retrieval to improve future analysis.

The application is not a trading bot. It acts as a professional market-analysis system focused on:
- market regimes,
- evolving macro theses,
- geopolitical and economic developments,
- sector analysis,
- and investment strategy guidance.

The application runs entirely on the user’s machine except for external API/model requests.

---

## Application Interface

### Main Layout
```text
Market Analyzer
├── Latest Report View
│   ├── Header summary
│   ├── Market regime
│   ├── Index analysis
│   ├── Dynamic market sections
│   ├── Investment strategy section
│   ├── Charts / graphs / links
│   └── Export actions
│
├── Recent Reports Sidebar
│   ├── Ordered descending
│   ├── Premarket reports
│   ├── Postmarket reports
│   └── Weekly review reports
│
└── Settings
```

---

## Scheduled Jobs

### Premarket Report Job
Runs:
```text
Monday–Friday mornings
4:00 AM PT / 7:00 AM ET
```

Focus:
- overnight futures,
- global markets,
- macro calendar,
- geopolitical developments,
- overnight earnings/news,
- expected market drivers.

### Postmarket Report Job
Runs:
```text
Sunday–Friday evenings
4:00 PM PT / 7:00 PM ET
```

Focus:
- what moved markets,
- index performance,
- sector leadership,
- macro reactions,
- yields/oil/dollar/VIX,
- thesis evolution,
- next-day setup.

### Weekly Review Job
Runs:
```text
Saturday
12:00 AM local time
```

Focus:
- analyze the previous week’s reports,
- judge accuracy,
- identify incorrect assumptions,
- identify useful signals,
- extract durable lessons.

The weekly review is stored as a normal readable report inside the application.

---

## Data Sources

### Market and Financial Data
- OpenBB
- Financial Modeling Prep
- FRED
- BLS

### News and Research
- Tavily
- GDELT

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

Before each scheduled job:
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

## Agent Architecture

### Main Agent
The main agent acts as the Head Market Analyst.

Responsibilities:
- gather market data
- gather news and research
- dynamically branch research
- coordinate subagents
- synthesize conclusions
- maintain long-term thesis continuity
- retrieve memory
- publish reports
- write durable learnings

The main agent owns the final report.

### Subagents
Three subagents are used:
- Bull Analyst
- Bear Analyst
- Balanced Analyst

These agents are not forced into biased conclusions.

Their role is to:
- explore different interpretations,
- challenge assumptions,
- and strengthen final analysis quality.

---

## Main Agent Workflow

### Full Flow
1. Scheduled job starts
2. Load settings
3. Load recent Markdown reports
4. Query vector memory
5. Check research inbox
6. Gather baseline market data
7. Gather news and research
8. Perform dynamic research branching
9. Build condensed research packet
10. Send packet to Bull/Bear/Balanced subagents
11. Receive subagent theses
12. Critique subagent responses independently
13. Synthesize final report
14. Save Markdown report to SQLite
15. Save report summary to vector DB
16. Save durable learnings if applicable
17. Generate HTML report from Markdown
18. Update application UI

The main agent does not engage in recursive conversations with subagents.
It critiques responses independently during synthesis.

---

## Cost-Control Architecture
The application is designed with bounded workflows to prevent excessive token usage.

### News Ingestion Flow
The system does not send large raw news volumes into frontier models.

Pipeline:
```text
~500 headlines gathered
→ deduplication
→ relevance scoring
→ clustering
→ ~40 relevant headlines
→ ~10 important stories
→ ~5 deeply analyzed topics
```

### Context Window Control
The application does not repeatedly inject large historical report histories into prompts.

Instead:
- recent reports are loaded separately,
- vector memory retrieval is used selectively,
- only relevant memory fragments are injected into prompts.

### Agent Workflow Limits
The application enforces:
- bounded research depth
- bounded retries
- bounded subagent execution
- no recursive agent loops
- no recursive debate cycles

---

## Fixed Internal Model Usage
Some internal workflows use non-configurable models for cost control and predictable performance.

### Headline Filtering
Uses:
- OpenAI GPT-5 mini

Purpose:
- filtering,
- deduplication,
- relevance scoring,
- clustering headlines into major topics.

Rationale:
- low cost,
- fast latency,
- strong enough for lightweight classification tasks.

### Data Extraction
Uses:
- OpenAI GPT-5 mini

Purpose:
- extracting structured information from:
  - news articles,
  - PDFs,
  - research documents,
  - earnings summaries,
  - macro reports.

Rationale:
- reliable structured output,
- inexpensive,
- good tool/function calling performance.

### Research Routing
Uses:
- Anthropic Claude Sonnet

Purpose:
- determining which topics deserve deeper analysis,
- identifying second-order implications,
- prioritizing research depth,
- deciding which themes/subsectors/geopolitical events matter most.

Rationale:
- stronger reasoning quality,
- better long-context understanding,
- more nuanced prioritization and synthesis.

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
- OpenAI API token,
- Anthropic API token.

If:
- an agent is configured to use a provider,
- but the corresponding API token is missing,

the scheduled job does not run. The application displays a warning message.

---

## Error Handling
If a job fails because of:
- API limits,
- token exhaustion,
- provider failures,
- malformed responses,
- or model execution errors,

the application:
1. cleanly cancels the job,
2. stores the failure state,
3. displays a warning banner inside the application.

If the warning already exists and has not been dismissed/resolved:
- additional failing jobs do not create duplicate warnings.

---

## Dynamic Research Behavior
The main agent always begins with a baseline scan.

### Baseline Scan
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

News:
- politics
- geopolitics
- China/trade
- energy
- earnings
- AI/semiconductors
- major economic developments

### Dynamic Branching Examples
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

---

## Analyst Skills
The following reusable skills are included in MVP:
1. Market Regime Analysis
2. Narrative vs Reality
3. Second-Order Effects
4. Inflation Decomposition
5. Historical Analog
6. Positioning & Sentiment
7. Thesis Stress Test
8. Geopolitical Escalation
9. AI Infrastructure Chain
10. Time Horizon Separation
11. Credit Stress Analysis
12. Energy Security Analysis
13. Central Bank Interpretation
14. Valuation Compression
15. Market Breadth Analysis
16. Consensus vs Contrarian Analysis

These skills operate as structured reusable prompts with expected output schemas.

---

## Subagent Responsibilities

### Bull Analyst
Responsibilities:
- identify upside drivers
- identify resilience
- challenge bearish overreaction
- identify improving conditions
- explain constructive scenarios

### Bear Analyst
Responsibilities:
- identify fragile assumptions
- identify downside risks
- challenge complacency
- inspect valuation and macro risks
- inspect geopolitical and credit risks

### Balanced Analyst
Responsibilities:
- separate signal from noise
- weigh evidence
- assign confidence
- separate short-term and long-term implications
- identify thesis change conditions

---

## Report Structure
Reports are written in Markdown by the main agent. An HTML version is generated for application display.

Reports are authored and stored internally as Markdown.

Markdown is the canonical report format used for:
- agent memory,
- report continuity,
- vector memory ingestion,
- report retrieval,
- and future report synthesis.

HTML reports are generated from Markdown and are presentation-only artifacts used for:
- in-app rendering,
- styling,
- chart display,
- and PDF generation.

Agents never ingest or reason over HTML reports.

### Standard Report Structure
```text
# Market Analyzer Report

## Header Summary
3–6 key bullets.

## Market Regime

## Index Picture
- Dow
- S&P 500
- Nasdaq

## What Changed Since Last Report

## Key Market Drivers

Dynamic sections:
- Inflation / Fed
- Energy
- AI / Semiconductors
- China / Geopolitics
- Consumer
- Earnings
- Liquidity / Credit
- Market Breadth

## Bull Case

## Bear Case

## Balanced Case

## Head Analyst View

## Investment Strategy

High-level guidance:
- sectors to watch
- industries benefiting from trends
- industries under pressure
- ETFs/themes of interest
- short/mid/long-term opportunities
- macro-sensitive positioning

No direct trade recommendations.

## Short-Term Outlook

## Long-Term Outlook

## Watchlist

## Sources
```
### Thesis Continuity and Evolution
The system maintains continuity between reports and treats market analysis as an evolving long-term process rather than a collection of disconnected daily summaries.

Each report exists within a broader market narrative that develops over time.

The main agent continuously:
- references recent reports,
- retrieves relevant historical learnings from vector memory,
- follows up on prior market concerns,
- tracks whether previous assumptions are strengthening or weakening,
- and updates long-term theses incrementally as new evidence appears.

The system is designed to behave like a professional analyst team maintaining ongoing market coverage rather than a stateless news summarizer.

#### Report Continuity
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
- continue monitoring unresolved market risks,
- revisit previous conclusions,
- acknowledge when earlier assumptions were incorrect,
- and identify when a thesis is strengthening or weakening.

### Thesis Stability
The system should avoid unnecessary thesis instability.

Long-term market theses should evolve gradually when:
- market conditions remain structurally similar,
- existing narratives continue holding,
- and incoming data reinforces prior conclusions.

The system should not dramatically change positioning or outlook because of isolated single-day market moves or short-lived news cycles.

The main agent should prioritize:
- signal over noise,
- multi-day/multi-week confirmation,
- and structural changes over temporary volatility.

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
- reports may heavily focus on the new event,
- prior assumptions may be explicitly challenged,
- and the long-term thesis may be revised aggressively.

The system should clearly explain:
- why the thesis changed,
- which assumptions failed,
- and what evidence caused the pivot.

### Memory-Guided Evolution
The vector memory system exists to help the main agent maintain analytical continuity over time.

The main agent uses memory retrieval to:
- identify similar historical conditions,
- revisit previous conclusions,
- track recurring market patterns,
- avoid repeating analytical mistakes,
- and maintain coherent long-term reasoning across reports.

The goal is not rigid consistency.

The goal is:
- coherent reasoning,
- gradual evolution when appropriate,
- and decisive adaptation when necessary.

---

## Storage

### SQLite
Stores:
- reports,
- report metadata,
- HTML output,
- job history,
- warning states.

Only the most recent 30 full reports are retained. Older reports are deleted automatically.

### LanceDB Vector Memory
Stores:
- report summaries,
- durable learnings,
- thesis evolution,
- important historical analogs,
- past mistakes,
- useful recurring patterns.

The vector DB acts as long-term semantic memory for the main agent.

---

## Weekly Review Workflow

### Weekly Review Process
1. Load previous week's reports
2. Compare reports against actual market developments
3. Identify correct conclusions
4. Identify incorrect assumptions
5. Identify missed signals
6. Identify useful patterns
7. Generate weekly review report
8. Write durable learnings into vector DB

The weekly review report appears in the normal report history UI.

---
## Export System
Reports are stored internally as:
- Markdown
- HTML

### Export Options
- Export Markdown
- Export PDF

PDF export is generated from the HTML report version.
