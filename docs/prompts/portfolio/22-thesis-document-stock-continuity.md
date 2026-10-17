# Thesis document — stock, continuity run

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v85`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, on a continuity run over the prior document of 2026-09-02.
The thesis document is a thinking call with no grammar: Part 1 the fetched values, the computed reads under one heading, the market analysis and this run's analysis; Part 2 what the document covers, in order, and its length band.
On a continuity run this run's review renders under REVIEW, then the prior document verbatim as PRIOR THESIS under its date, and the summary item asks what changed since it and how the prior read held up; the document is never rewritten.
The prior here is attempt 6's persisted verdict, its model arm re-shaped by hand into a thesis document and an appendix, re-dated to 2026-09-02, with a hand-written prior spot 3% under today's and its anchor bar; attempt 6 wrote no second run.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the dates, URLs and headers around it are as rendered.
Under FETCHED VALUES, the issuer line, the quarterly statements and the consensus, the 52-week range, the 8-K list, the short-interest print, the street, insider and congressional rows, the surprises, the ratio lines, owner earnings, enterprise value, the float, the M&A match and the segments are synthetic values layered onto the fixture for these examples alone, their names stubbed; the closes are the fixed set's, as is the price under HOLDING.
The statements are sized to the engine's persisted metrics and the consensus to its band driver, and the surprises' EPS, the ratio lines, the enterprise-value row and the segments agree with the statements.
Under SCORES, the factor lines and the return and cost of capital they name are synthetic values layered onto the fixture for these examples alone, scored through the live bands; the four scores are their means.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6f |
| Stage label | `thesis TSLA` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | free text: no tools, no grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 16814 chars — the messages and tools as serialized |

## System message

~~~~text
You are an equity analyst writing the thesis document for one holding in a portfolio review. Part 1
of the message gives the inputs. Part 2 says what the document covers, in order, and how to return
it.
~~~~

## User message (16332 chars)

~~~~text
======== PART 1: INPUTS ========

# HOLDING
TSLA (Tesla, Inc.).
Price: $358.97 per share.
Date: 2026-09-16.

# FETCHED VALUES
The holding's data (USD; B is billions, M is millions; a yield or a return as a percentage); a
missing figure reads (gap). The rows follow under their labels.

## PROFILE
- name: Tesla, Inc.
- exchange: NASDAQ
- sector: Consumer Cyclical
- industry: Auto - Manufacturers
- market capitalization: 1260.0B
- shares outstanding: 3.5B

## STATEMENTS
Quarterly statements as reported, newest first (period end, then the figure)
- revenue: 2026-06-30 22.5B; 2026-03-31 19.3B; 2025-12-31 25.7B; 2025-09-30 25.2B; 2025-06-30 20.1B;
2025-03-31 17.3B; 2024-12-31 23.0B; 2024-09-30 22.5B
- gross profit: 2026-06-30 4.3B; 2026-03-31 3.5B; 2025-12-31 4.9B; 2025-09-30 4.8B; 2025-06-30 3.6B;
2025-03-31 2.9B; 2024-12-31 3.7B; 2024-09-30 4.0B
- operating income: 2026-06-30 1.50B; 2026-03-31 0.80B; 2025-12-31 1.30B; 2025-09-30 1.10B;
2025-06-30 1.30B; 2025-03-31 0.40B; 2024-12-31 1.60B; 2024-09-30 2.70B
- net income: 2026-06-30 1.20B; 2026-03-31 0.60B; 2025-12-31 0.90B; 2025-09-30 0.70B; 2025-06-30
1.40B; 2025-03-31 0.40B; 2024-12-31 2.30B; 2024-09-30 2.20B
- diluted EPS: 2026-06-30 0.34; 2026-03-31 0.17; 2025-12-31 0.26; 2025-09-30 0.20; 2025-06-30 0.40;
2025-03-31 0.11; 2024-12-31 0.66; 2024-09-30 0.63
- diluted shares: 2026-06-30 3.5B; 2026-03-31 3.5B; 2025-12-31 3.5B; 2025-09-30 3.5B; 2025-06-30
3.5B; 2025-03-31 3.5B; 2024-12-31 3.5B; 2024-09-30 3.5B
- operating cash flow: 2026-06-30 3.5B; 2026-03-31 2.6B; 2025-12-31 3.6B; 2025-09-30 3.3B;
2025-06-30 3.0B; 2025-03-31 2.2B; 2024-12-31 4.0B; 2024-09-30 4.5B
- free cash flow: 2026-06-30 1.00B; 2026-03-31 0.30B; 2025-12-31 1.00B; 2025-09-30 0.90B; 2025-06-30
0.90B; 2025-03-31 0.10B; 2024-12-31 2.00B; 2024-09-30 2.70B
- capital expenditure: 2026-06-30 -2.5B; 2026-03-31 -2.3B; 2025-12-31 -2.6B; 2025-09-30 -2.4B;
2025-06-30 -2.1B; 2025-03-31 -2.1B; 2024-12-31 -2.0B; 2024-09-30 -1.8B
- total debt: 2026-06-30 8.4B; 2026-03-31 8.3B; 2025-12-31 8.1B; 2025-09-30 7.9B; 2025-06-30 7.8B;
2025-03-31 7.6B; 2024-12-31 7.5B; 2024-09-30 7.3B
- total equity: 2026-06-30 77.7B; 2026-03-31 76.5B; 2025-12-31 75.9B; 2025-09-30 75.0B; 2025-06-30
74.3B; 2025-03-31 72.9B; 2024-12-31 72.5B; 2024-09-30 70.3B
- cash and equivalents: 2026-06-30 33.0B; 2026-03-31 31.5B; 2025-12-31 30.8B; 2025-09-30 29.9B;
2025-06-30 28.7B; 2025-03-31 27.4B; 2024-12-31 26.1B; 2024-09-30 25.0B

## CONSENSUS
- consensus EPS by fiscal period end, as published: 2026-12-31 1.71; 2027-12-31 2.27

## DIVIDENDS
- (gap)

## QUOTE AND CLOSES
- 52-week range as served: low 215.38, high 430.76
- daily closes: 2 sessions from 2026-09-02 to 2026-09-14
- close on the prior analysis date 2026-09-02: 348.20

## FILINGS
8-K filings of the trailing twelve months, newest first (filing date: items)
- 2026-07-23: 2.02, 9.01
- 2026-05-15 (8-K/A): 5.02
- 2026-04-22: 2.02, 9.01

## SHORT INTEREST PRINT
- settlement: 2026-08-31 (FINRA)
- shares short: 80.0M
- shares short at the prior settlement: 76.0M
- average daily volume: 95.0M
- days to cover: 0.84

## STREET
- price targets: consensus 387.69, median 394.87, low 179.49, high 717.94
- targets published: last month 4 averaging 412.82; last quarter 15 averaging 384.10; last year 48
averaging 358.97
- analyst ratings: strong buy 3, buy 14, hold 18, sell 7, strong sell 2; consensus Hold
Rating actions, newest first (date: firm, previous grade to new grade, action)
- 2026-09-10: [stub: grading firm], Hold to Buy, upgrade
- 2026-07-24: [stub: grading firm], Buy to Buy, maintain
- 2026-04-23: [stub: grading firm], (gap) to Hold, initiate

## INSIDERS AND CONGRESS
Insider trades, newest first (transaction date: name, role, type, shares at price, filing date)
- 2026-09-05: [stub: insider name], officer, S-Sale, 10000 shares at 351.79, filed 2026-09-08
- 2026-08-20: [stub: insider name], director, A-Award, 2500 shares at 0.00, filed 2026-08-21
- 2026-07-30: [stub: insider name], 10 percent owner, S-Sale, 50000 shares at 337.43, filed
2026-08-01
- insider statistics, 2026 Q3: 1 acquiring and 9 disposing transactions; 2500 shares acquired,
310000 disposed
Congressional trades, newest first (transaction date: chamber, member, owner, type, amount,
disclosure date)
- 2026-08-14: House, [stub: member], Joint, Purchase, $1,001 - $15,000, disclosed 2026-09-02
- 2026-06-03: Senate, [stub: member], Spouse, Sale, $15,001 - $50,000, disclosed 2026-07-01

## EARNINGS
Surprises, newest first (announcement date: EPS actual vs estimate, revenue actual)
- 2026-07-22: EPS 0.34 vs 0.37 estimated, revenue 22.5B
- 2026-04-22: EPS 0.17 vs 0.25 estimated, revenue 19.3B
- 2026-01-29: EPS 0.26 vs 0.29 estimated, revenue 25.7B
- 2025-10-22: EPS 0.20 vs 0.16 estimated, revenue 25.2B
- next earnings: 2026-10-21 (EPS estimate 0.55)

## VALUATION
- trailing-twelve-month ratios: P/E 370.07; EV/EBITDA 124.79; EV/sales 13.33; P/B 16.22; FCF yield
0.2%; ROIC 8.3%; ROE 4.4%; net debt/EBITDA -2.48
- owner earnings (FY2026 Q2, period end 2026-06-30): 2.1B; 0.60 per share
- enterprise value (2026-06-30): 1235.4B; market capitalization 1260.0B; total debt 8.4B; cash 33.0B
- float (2026-09-01): float shares 3.0B; shares outstanding 3.5B; free float 86.9%

## M&A
SEC merger registration filings, trailing twelve months; a filing follows a deal's announcement,
typically by several weeks, and covers deals paid at least partly in stock
- acquirer of [stub: counterparty], filed 2026-05-20

## SEGMENTS
Revenue by product, newest fiscal year first
- FY2025 (period end 2025-12-31): Automotive 68.5B, Services and other 10.3B, Energy generation and
storage 9.5B
- FY2024 (period end 2024-12-31): Automotive 68.0B, Services and other 9.5B, Energy generation and
storage 8.5B
Revenue by geography, newest fiscal year first
- FY2025 (period end 2025-12-31): United States 41.5B, Other 27.0B, China 19.8B
- FY2024 (period end 2024-12-31): United States 42.0B, Other 25.5B, China 18.5B

## RATES
- Treasury yields (FRED): 10-year 4.50%, 2-year 4.00%

# COMPUTED
The computed reads follow under their labels; each is derived from the fetched data by fixed
formulas.

## METRICS
Flow metrics (net margin, gross margin, revenue growth, P/E, P/S) are on a TTM (four trailing
quarters) basis. Balance-sheet metrics (debt / equity, P/B) are from FMP's latest quarterly balance
sheet.
- net margin: 0.0368 — a fraction, never a percent (0.16 means 16%)
- gross margin: 0.1885 — a fraction, never a percent (0.16 means 16%)
- year-over-year revenue growth: 0.1175 — a fraction, never a percent (0.16 means 16%)
- debt / equity ratio: 0.1076 — a ratio (1.5 means debt is 1.5 times equity)
- daily realized return volatility: 0.0320 — a daily fraction, never a percent (0.02 means 2% per
day)
- trailing price return: -0.0309 — a fraction, never a percent (0.16 means 16%)
- price / earnings multiple: 368.8658 — a multiple (25 means 25x)
- price / sales multiple: 13.5914 — a multiple (25 means 25x)
- price / book multiple: 16.2142 — a multiple (25 means 25x)
- forward consensus, next twelve months blended over 2 fiscal periods: EPS low 1.38 / mid 2.10 /
high 3.07; revenue low 98.0B / mid 104.0B / high 110.0B — the price bands' driver

## SCORES
Four scores from 0 to 100, higher is better on every axis: quality; valuation, where higher means
more attractive; momentum; risk, where higher means more resilient. The grade is a letter derived
from the quality, valuation and risk scores. Each score is the mean of the factors listed under it,
each read over the trailing twelve months where it reads the statements. A factor scores 50 at its
band's anchor and 0 or 100 at the band's edges, a value past an edge held at it. Where the company
has 8 or more earlier trailing-year readings of a factor, that score is averaged with the factor's
rank among them, on the same scale. A factor that could not be computed scores 50.
quality 27, valuation 12, momentum 58, risk 39. Grade F. Risk tier: high.
quality
- gross profit over total assets: 0.1263; band 0.3500 ± 0.3500 (Consumer Cyclical): 18; rank among
12 earlier readings: 8; score 13
- operating income over revenue: 0.0505; band 0.1010 ± 0.1010 (Consumer Cyclical): 25; rank among 12
earlier readings: 8; score 17
- return on invested capital less its cost of capital: -0.0460 (return 0.0827 less cost of capital
0.1287); band 0.0000 ± 0.1500 (market-wide): 35; rank among 12 earlier readings: 8; score 22
- free cash flow over net income: 0.9253; band 0.9000 ± 0.6000 (market-wide): 52; rank among 12
earlier readings: 58; score 55
valuation
- net income over market value: 0.0027; band 0.0335 ± 0.0335 (Consumer Cyclical): 4; rank among 12
earlier readings: 25; score 15
- free cash flow over market value: 0.0025; band 0.0302 ± 0.0302 (Consumer Cyclical): 4; rank among
12 earlier readings: 17; score 10
- operating income over enterprise value: 0.0038; band 0.0380 ± 0.0380 (Consumer Cyclical): 5; rank
among 12 earlier readings: 17; score 11
momentum
- price return from a year ago to a month ago: 0.4200; band 0.0800 ± 0.4000 (market-wide): 92; rank
among 8 earlier readings: 62; score 78
- last close over its 200-session average, less one: -0.0450; band 0.0000 ± 0.2000 (market-wide):
39; score 39
risk
- beta (lower is better): 2.3000; band 1.0000 ± 0.6000 (market-wide): 0, past its edge; score 0
- annualized volatility of daily returns over the trailing year (lower is better): 0.5080; band
0.3500 ± 0.1750 (Consumer Cyclical): 5; rank among 8 earlier readings: 50; score 27
- debt over equity (lower is better): 0.1076; band 0.8000 ± 0.8000 (Consumer Cyclical): 93; rank
among 12 earlier readings: 83; score 88

## PRICE BANDS (USD)
- three-month: bear 229.41 / base 310.01 / bull 390.61. Method: base = the twelve-month base price
return prorated to three months; bear and bull = ±26.0% (two standard deviations of daily volatility
over 63 sessions, capped at 26%)
- twelve-month: bear 62.04 / base 163.12 / bull 911.74. Method: consensus forward EPS (low / mid /
high) × P/E multiples at the 75th / 50th / 25th percentile of their spread to the 10-year Treasury
over the last 12 quarterly observations
- three-year: bear 62.04 / base 163.12 / bull 911.74. Method: the twelve-month drivers held at flat
growth for two further years (a single forward consensus row, or no definable growth) at the same
multiples — an extrapolation that assumes today's rate and spread regime holds
- Notes: the driver blends two consensus rows.
- What the current price implies, at each scenario's multiple: EPS growth versus the trailing print
of +11.2% at the bull multiple, +325.8% at the base multiple, +634.8% at the bear multiple.
Assumptions: rate-anchored (spread-percentile) multiples, 10-year Treasury 4.97%.

## CAPITAL EFFICIENCY
The computed twelve-month total return in each scenario (the move from the current price to the
scenario price, plus forward income per share, as a fraction of the current price) and the hurdle
rate it is measured against, with the read: clears when even the bear case clears the hurdle, fails
when even the bull case misses it, indeterminate otherwise.
bear -82.6% / base -54.3% / bull +155.7%; hurdle 12.7%; read: indeterminate.

## OPTIONS ACTIVITY
put/call volume 0.961, put/call open interest 1.090, implied volatility 0.435, IV skew 0.000 (mean
put IV minus mean call IV, in IV's decimal unit; positive means puts are richer).

## SHORT INTEREST (FINRA biweekly file, settlement 2026-08-31; the file lags its settlement by about
7 business days)
80.0M shares short (+5.3% vs the prior settlement's 76.0M), average daily volume 95.0M, days to
cover 0.84

# MARKET ANALYSIS
A market-level analysis dated 2026-09-16, followed by the stance of the 3 most recent analyses.
## Market Signal Thesis

[stub: the latest report's Market Signal Thesis section]

## Investment Strategy

[stub: the latest report's Investment Strategy section]

- 2026-09-16: thesis bearish, risk posture risk-off
- 2026-08-30: thesis mixed, risk posture mixed
- 2026-08-25: thesis mixed, risk posture mixed

# ANALYSIS
[stub: this run's analysis — the research consolidated across the topics, as the analysis call
returned it]

# REVIEW
[stub: this run's review — the prior position against what happened since, as the review call
returned it]

# PRIOR THESIS (written 2026-09-02)
Thesis: Investment thesis rests on Robotaxi monetization within 12-24 months justifying premium
multiple, with safety scandal resolution and margin stabilization as near-term catalysts; core EV
business provides downside floor at ~$180/share (base case earnings power) but offers limited upside
without autonomous revenue validation.

Key drivers: what must improve — Robotaxi deployment with regulatory approval by Q1-CY26; gross
margins must re-expand toward mid-20s through cost reductions and volume leverage; FSD safety
metrics need to demonstrably improve vs human baseline. What must not break — Gross margin floor at
17%+ over next four quarters; NHTSA investigation remains non-actionable without vehicle
restrictions or recall; regulatory credits remain ≥30% of operating income until Robotaxi revenue
materializes.

Scenarios: bear (35%): Gross margins breach floor at ≤16%, NHTSA mandates vehicle
restrictions/recall, FSD crashes increase >25%, or Robotaxi commercialization delayed beyond CY27.;
base (40%): Robotaxi commercialization begins Q4-Q1 CY26 without NHTSA enforcement action; gross
margin stabilizes above 17%; regulatory credits decline to <35% of operating income over 8
quarters.; bull (25%): Robotaxi fleet achieves profitable unit economics by Q2-CY26 with regulatory
approval; gross margins re-expand to >24%; Energy/storage revenue doubles within 18 months.

Expected price: twelve months $145.00; no stated price at three months or three years. Conviction:
low.

Summary: Tesla exhibits structurally thin operating margins (3.68% net) heavily reliant on
regulatory credits, with gross margin contraction to 18.85%. Revenue growth at 11.75% YoY reflects
deceleration from historical hyper-growth rates. Balance sheet remains fortress-quality with minimal
leverage (D/E: 0.11x), but the valuation is exceptionally extended across all multiples—P/E of 369x,
P/S of 14x, and P/B over 16x assume material Robotaxi monetization within 12-24 months without
commercialized revenue to validate these expectations. My base twelve-month target of $145 derives
from a conservative auto peer multiple (P/S: ~6x on normalized revenue) plus Energy/storage
optionality at ~30% discount to standalone value. I reject the engine's base case ($163) because it
assumes Robotaxi commercialization without material regulatory headwinds—a scenario unsupported by
current NHTSA investigation status and FSD safety record deterioration (236 crashes in July CY25,
four fatalities). My bear target ($92) applies a distress multiple to core auto earnings excluding
credits. Bull case ($398) requires successful Robotaxi launch with approved commercial operations by
Q1-CY27; this is possible but not probable given regulatory and technical execution risks.

======== PART 2: TASK ========

Write the thesis document for this holding as plain text — no code fence, no JSON, no heading before
the first line. It covers, in this order:

1. The thesis — the investment case, from FETCHED VALUES, COMPUTED, ANALYSIS and MARKET ANALYSIS.

2. The key drivers — what the thesis depends on.

3. The bear, base and bull scenarios — the conditions that produce each and your probability for it;
the three sum to about 100 percent.

4. The falsifiers — the observations that would show the thesis wrong — and the triggers — the
conditions on which the position would be added to, trimmed or sold — each a concrete measure, a
level and a period, a trigger stating the direction of the position change.

5. The expected share price at three months, twelve months and three years, and your conviction in
the read as a whole as high, medium or low, each argued in the text; the price bands under COMPUTED
are evidence, not bounds. Where you state no price at a horizon, or no conviction, say so.

6. A summary paragraph — the financial read, why those prices and that conviction, and what changed
since the prior analysis and how the prior read held up, drawing on REVIEW and PRIOR THESIS.

The document runs 900 to 1,800 words.
~~~~
