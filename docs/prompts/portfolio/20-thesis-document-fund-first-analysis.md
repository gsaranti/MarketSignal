# Thesis document — priced fund, first analysis

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v70`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: SPMO, an ETF of the fixed evidence set (attempt 6, reconstructed), on its first analysis; the fixture carries no fund context, so the FUND block does not render.
The thesis document is a thinking call with no grammar: Part 1 the fetched values, the computed reads under one heading, the market analysis and this run's analysis; Part 2 what the document covers, in order, and its length band.
A priced fund takes the same call with the fund's metric labels and an investment analyst's role line.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6f |
| Stage label | `thesis SPMO` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | free text: no tools, no grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 4745 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst writing the thesis document for one holding in a portfolio review.
Part 1 of the message gives the inputs. Part 2 says what the document covers, in order, and how to
return it.
~~~~

## User message (4394 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
SPMO (name unavailable).
Price: $144.19 per share.
Date: 2026-09-16.

FETCHED VALUES
The holding's data as its providers return it, each figure as reported (USD; B is billions, M is
millions); none is computed.
Quote: 144.19 per share (the live print, undated).
Daily closes: 1 sessions from 2026-09-14 to 2026-09-14, 52-week low 144.19 on 2026-09-14, high
144.19 on 2026-09-14.
Treasury yields (FRED): 10-year 4.50%, 2-year 4.00%.

COMPUTED
The computed reads follow under their labels, through to MARKET ANALYSIS; each is derived from the
fetched data by fixed formulas.

METRICS
The market metrics are daily; the expense ratio is the fund's published figure.
- daily realized return volatility: 0.0197 — a daily fraction, never a percent (0.02 means 2% per
day)
- trailing price return: 0.2629 — a fraction, never a percent (0.16 means 16%)

SCORES
Four scores from 0 to 100, higher is better on every axis: quality; valuation, where higher means
more attractive; momentum; risk, where higher means more resilient. The grade is a letter derived
from the quality, valuation and risk scores.
quality 50, valuation 83, momentum 94, risk 67. Grade C. One score is imputed. Risk tier: low.

PRICE BANDS (USD)
- three-month: bear 105.63 / base 142.74 / bull 179.85. Method: base = the twelve-month base price
return prorated to three months; bear and bull = ±26.0% (two standard deviations of daily volatility
over 63 sessions, capped at 26%)
- twelve-month: bear 124.03 / base 138.38 / bull 152.74. Method: fund exposure composite × composite
P/E multiples at the 75th / 50th / 25th percentile of their spread to the 10-year Treasury over the
last 12 quarterly observations
- three-year: bear 124.03 / base 138.38 / bull 152.74. Method: the twelve-month mix re-rating held
unchanged for three years (the fund's flat driver carries no growth to compound) — an extrapolation
that assumes today's rate and spread regime holds
- Notes: the driver is held flat across scenarios; the band was widened to the volatility dispersion
floor.

CAPITAL EFFICIENCY
The computed twelve-month total return in each scenario (the move from the current price to the
scenario price, plus forward income per share, as a fraction of the current price) and the hurdle
rate it is measured against, with the read: clears when even the bear case clears the hurdle, fails
when even the bull case misses it, indeterminate otherwise.
bear -13.0% / base -3.0% / bull +7.0%; hurdle 7.7%; read: fails.

OPTIONS ACTIVITY
put/call volume 0.734, put/call open interest 0.699, implied volatility 0.278, IV skew 0.000 (mean
put IV minus mean call IV, in IV's decimal unit; positive means puts are richer).

MARKET ANALYSIS
A market-level analysis dated 2026-09-16, followed by the stance of the 3 most recent analyses.
## Market Signal Thesis

[stub: the latest report's Market Signal Thesis section]

## Investment Strategy

[stub: the latest report's Investment Strategy section]

- 2026-09-16: thesis bearish, risk posture risk-off
- 2026-08-30: thesis mixed, risk posture mixed
- 2026-08-25: thesis mixed, risk posture mixed

ANALYSIS
[stub: the distilled research — the combined findings across the topics, as the reduce call returned
them]

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

6. A summary paragraph — the financial read, why those prices and that conviction.

The document runs 900 to 1,800 words.
~~~~
