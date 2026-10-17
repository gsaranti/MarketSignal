# Analysis — fund, first analysis

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v83`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: BND, the synthetic total bond market ETF the fixed evidence set carries for the role/risk branch.
The analysis call is a thinking call with no grammar: Part 1 the holding header and FETCHED VALUES as the brief carries them, on a continuity run PRIOR ANALYSIS, then WRITE-UPS; Part 2 what the analysis states and its length band. The analysis is the only research artifact the next run reads.
A fund takes the same call over its agenda's write-ups — here the exposure-profile topic's — with the fund's FETCHED VALUES.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6d |
| Stage label | `analysis BND` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | free text: no tools, no grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 1661 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst writing the analysis of one holding's research for a portfolio review.
Part 1 of the message gives the inputs. Part 2 says what the analysis covers and how to return it.
~~~~

## User message (1349 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
BND (VANGUARD TOTAL BOND MARKET ETF).
Price: $72.38 per share.
Date: 2026-09-16.

FETCHED VALUES
The holding's data (USD; B is billions, M is millions; a yield or a return as a percentage); a
missing figure reads (gap). The rows follow under their labels.

PROFILE
- name: Vanguard Total Bond Market ETF
- asset class: Fixed Income
- expense ratio: 0.03%
- assets under management: 340.0B
- NAV: 72.41

WEIGHTS
- countries, largest first: United States 94.0%, Supranational 2.0%, Canada 1.0%

DIVIDENDS
- (gap)

QUOTE AND CLOSES
- daily closes: 21 sessions from 2026-08-14 to 2026-09-14

RATES
- Treasury yields (FRED): 10-year 4.50%, 2-year 4.00%

WRITE-UPS
The research on this holding, one write-up per topic.

Exposure profile
[stub: the fund's exposure-profile write-up]

======== PART 2: TASK ========

Write the analysis of the research under WRITE-UPS as plain text — no code fence, no JSON, no
heading before the first line. It states what that research establishes on the holding when its
topics are read together; where its sources disagree; and what stays unanswered. Each figure is
quoted with the date or period its source gives for it, and its source is named: the page address
named under WRITE-UPS, or FETCHED VALUES where the figure comes from there.

The analysis runs 900 to 1,800 words.
~~~~
