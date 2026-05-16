# Agent Architecture

## Main Agent

The main agent acts as the Head Market Analyst.

Responsibilities:
- gather market data
- gather news and research
- dynamically branch research
- coordinate subagents
- synthesize conclusions
- maintain evolving market theses
- retrieve memory
- publish reports
- write durable learnings

The main agent owns the final report.

The step-by-step procedure the main agent follows during a report run is described in [main-agent-workflow.md](main-agent-workflow.md).

## Subagents

Three subagents are used:
- Bull Analyst
- Bear Analyst
- Balanced Analyst

These agents are not forced into predetermined conclusions or artificial disagreement.

Their purpose is to:
- explore different market interpretations
- challenge assumptions
- stress-test market narratives
- identify overlooked risks or opportunities
- strengthen the quality of the final report

The subagents operate as professional analysts with different analytical perspectives rather than ideological positions.

It is completely valid for:
- all three agents to arrive at a similar market conclusion
- two agents to generally agree while one differs
- all three agents to identify different risks and opportunities within the same broader market regime

Examples:
- All three agents may conclude that market conditions remain structurally bullish while identifying different risks beneath the surface.
- The Bull and Balanced agents may agree that AI infrastructure demand remains strong, while the Bear agent focuses on valuation and inflation risks.
- The Bear agent may acknowledge strong market momentum and liquidity conditions while still identifying fragile assumptions underneath the rally.

The goal of the subagent system is not conflict for the sake of conflict.

The goal is:
- analytical depth
- thesis stress-testing
- stronger final synthesis by the main agent

The main agent evaluates all subagent responses and determines how much weight to assign each perspective during final report generation.

Per-agent responsibilities for the Bull, Bear, and Balanced analysts are described in [subagents.md](subagents.md).
