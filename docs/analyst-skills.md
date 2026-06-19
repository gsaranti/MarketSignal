# Analyst Skills

The following reusable skills are included in MVP.
Each skill is a reusable analytical **lens**: a method the agent applies to the current report's data and research, plus a structured **verdict** it must land. They are not pipeline stages, and not every lens applies to every report.

## How Skills Are Used

The skills form a shared analytical library. **As built, both the main agent and the three Bull / Bear / Balanced analysts consume them**: the analysts apply the relevant lenses when forming their independent reviews (Steps 12–15), and the main agent applies them again during synthesis (Step 16; see [agents.md](agents.md)), folding the relevant lenses' conclusions into the Market Signal Thesis.

The **whole library is supplied in full every report**:
- Each consumer — the main agent and each analyst — receives every skill, each skill's method body and the structured verdict it should yield, in one pass with no separate selection step.
- Not every lens applies to every report. Each consumer applies the ones the current report's data and research actually warrant and leaves the rest aside.
- Each applied lens's verdict is **folded into the consumer's own output** — the main agent's thesis prose, an analyst's review — never written up as its own report section or review field, and the skills are not named. They are reasoning tools, not output structure.

A skill's verdict shape is a **forcing function on the prose**: it disciplines the model to land a specific conclusion rather than vague "consider X" guidance. It is **not** a machine-readable channel — nothing is parsed back from the report or persisted.

Each skill below is listed by name with its description and what it evaluates. The authoritative catalog — every skill's exact method body and verdict shape — lives in `src-tauri/src/skills.rs`.

### Deviations from the original design

Recorded for continuity; the catalog of skills below is unchanged.

- **Consumers** — the original design (the three analyst agents — Bull / Bear / Balanced — *and* the main agent) is now **fully as-built**: the main-agent consumer shipped first, and the analyst follow-on has since landed. Each analyst receives the full library inline and self-selects the lenses its posture and the report warrant, forcing-function-only like the main agent (no parsed or persisted channel). The analyst verdict is folded into its review's key points, risks, and opportunities rather than thesis prose.
- **Delivery** — originally **progressive disclosure** (each agent sees only frontmatter, then requests the relevant subset via a selection call). Removed: the bodies are small (~150 tokens each, ~2.4k for all 16), and the phase-1 selection call re-sent the entire packet to the model just to save the frontmatter catalog — a round-trip and a fail-soft code path for negative net benefit. All 16 now ship inline; the model self-selects which lenses to apply.
- **Output** — originally a per-skill **output schema** the application layer supplied and consumed; as built, the verdict is **prose-level only** (a forcing function folded into the thesis), with no parsed schema and no persistence. The richer structured-output channel remains a deferred option.

## Market Regime Analysis
Determines the current market regime and the dominant forces driving market behavior.

The skill evaluates whether the market is primarily:
- risk-on or risk-off
- liquidity-driven or earnings-driven
- inflation-sensitive or growth-sensitive
- whether market leadership is broadening or narrowing over time

## Narrative vs Reality
Separates genuine market or economic changes from exaggerated media narratives and short-term emotional reactions.
The skill evaluates whether market behavior is supported by underlying data, positioning, earnings, macro trends, and structural conditions rather than headlines alone.

## Second-Order Effects
Analyzes downstream consequences of major market, economic, geopolitical, or policy developments.

The skill maps how first-order events can propagate into:
- inflation
- yields
- liquidity
- sector performance
- consumer behavior
- long-term market conditions

## Inflation Decomposition
Breaks inflation into its underlying components and evaluates whether inflation pressure is temporary, structural, broadening, or narrowing.

The skill analyzes:
- energy
- shelter
- services
- wages
- transportation
- goods inflation separately rather than treating CPI as a single signal

## Historical Analog
Compares current market conditions to historical market environments and macroeconomic periods.

The skill identifies similarities and differences between current conditions and events such as:
- the dot-com bubble
- inflationary periods
- tightening cycles
- liquidity crises
- prior geopolitical or commodity shocks

## Positioning & Sentiment
Analyzes investor psychology, market positioning, and sentiment conditions.

The skill evaluates:
- fear and greed dynamics
- FOMO behavior
- crowded trades
- defensive positioning
- whether market behavior is becoming euphoric, complacent, or overly pessimistic

## Thesis Stress Test
Challenges the current market thesis and searches for weak assumptions or contradictory evidence.

The skill evaluates:
- what could invalidate the thesis
- which assumptions are fragile
- which signals are being ignored
- and what conditions would force a reassessment

## Geopolitical Escalation
Evaluates geopolitical developments and their potential market implications.

The skill analyzes:
- military conflicts
- trade tensions
- sanctions
- shipping disruptions
- commodity risks
- global supply-chain exposure

## AI Infrastructure Chain
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

## Time Horizon Separation
Separates short-term market reactions from medium-term and long-term structural market trends.
The skill helps prevent the system from confusing temporary volatility with meaningful changes to the broader market thesis.
The skill also helps the system distinguish between:
- short-term market noise
- cyclical developments
- structural long-term market shifts

## Credit Stress Analysis
Evaluates financial stress inside credit markets and identifies signs of tightening financial conditions.

The skill analyzes:
- credit spreads
- refinancing risk
- default pressure
- liquidity conditions
- commercial real estate stress
- broader systemic financial risk

## Energy Security Analysis
Analyzes energy-market stability and the macroeconomic implications of energy disruptions.

The skill evaluates:
- oil and natural gas supply
- OPEC activity
- shipping chokepoints
- grid stress
- energy-driven inflation risk
- the relationship between AI infrastructure growth and power demand

## Central Bank Interpretation
Interprets central-bank communication, policy decisions, and market expectations.

The skill evaluates:
- rate expectations
- liquidity conditions
- inflation priorities
- policy tone
- how central-bank positioning may affect equities, bonds, and broader market behavior

## Valuation Compression
Analyzes how interest rates, yields, and macroeconomic conditions may affect valuation multiples.

The skill focuses particularly on:
- long-duration growth assets
- high-multiple sectors
- whether earnings growth is sufficient to justify current valuations

## Market Breadth Analysis
Evaluates the health and participation level of the broader market beyond headline index performance.

The skill analyzes:
- advance/decline trends
- equal-weight vs cap-weight performance
- sector participation
- leadership concentration
- whether rallies or selloffs are broad-based or narrow

## Consensus vs Contrarian Analysis
Evaluates what the market currently expects versus what outcomes would genuinely surprise participants.

The skill helps identify:
- overconsensus narratives
- underappreciated risks
- asymmetric opportunities
- situations where market positioning may be vulnerable to unexpected developments
- areas where long-term market expectations may be mispriced
