# Report Structure

Reports are written, authored, and stored internally in Markdown by the main agent. An HTML version is rendered on demand for application display.

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

The Markdown→HTML conversion uses **markdown-it** as the renderer. The *visual* presentation-layer choices — chart styling, colour, geometry, and motion — are MVP-internal details owned by the design system and the renderer and are not specified here. The Markdown **authoring conventions** the report format relies on — notably the embedded-`chart` block below — are part of the format and are recorded here.

HTML is rendered on demand in the application webview and is never persisted — Markdown is the only stored report format (amended 2026-06-12; see [storage.md §SQLite](storage.md#sqlite)).

Agents never ingest or reason over HTML reports.

### Embedded charts

A report's Markdown may embed a small chart — a fenced code block tagged `chart` whose body is a JSON object, which the presentation layer renders to a restrained inline SVG — where the shape of the data reads more clearly than prose (a yield series, an index path, a sector-return comparison). Three forms render:

- **`line`** — a trend or path over time (a yield series, an index path, a spread).
- **`bar`** — a signed quantity tracked across successive periods (a week-by-week return), grown from a zero baseline; or, with an optional `categories` array, a cross-sectional comparison across named groups (returns by sector).
- **`area`** — a single magnitude over time (a credit spread, a volatility level), grown from a zero baseline.

Rendering is **fail-soft**: a malformed or unrenderable block falls back to its raw code block and never breaks the surrounding report. The `chart` block is the only way a chart enters a report — the agent emits it as part of its Markdown; the application layer never injects one.

The agent-facing JSON shape and the authoring rules the agent is given (series count, one `emphasis`, `categories`) live in the main-agent prompt (`src-tauri/src/model_agent.rs`); the renderer `src/renderChart.ts` is the authoritative validator and the source of truth for what renders versus falls back — it enforces some bounds the prompt does not state (e.g. the per-series point-count limits). Chart styling extends the design system's chart register (`market-signal-design-system/project/colors_and_type.css`, `.prose .chart-*`). These remain the sources of truth — this note records the convention, not its schema.

## Standard Report Structure

Within these sections the agent may embed charts via the [`chart` fenced-block convention](#embedded-charts) above.

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
