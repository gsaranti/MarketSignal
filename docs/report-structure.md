# Report Structure

Reports are written, authored, and stored internally in Markdown by the main agent. An HTML version is generated for application display.

## Canonical Format: Markdown

Markdown is the canonical report format used for:
- agent context and memory workflows
- report continuity
- vector memory ingestion
- report retrieval
- future report synthesis

## Presentation Format: HTML

HTML reports are generated from Markdown and are presentation-only artifacts used for:
- in-app rendering
- styling
- chart display
- PDF generation

The Markdown→HTML conversion uses **markdown-it** as the renderer. Styling, chart rendering, and other presentation-layer choices are MVP-internal implementation details and not specified here.

Agents never ingest or reason over HTML reports.

## Standard Report Structure

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
