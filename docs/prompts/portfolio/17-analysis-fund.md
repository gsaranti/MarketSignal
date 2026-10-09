# Analysis — fund, first analysis

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v73`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: BND, the synthetic total bond market ETF the fixed evidence set carries for the role/risk branch.
The analysis call is a thinking call with no grammar: Part 1 the holding header and FETCHED VALUES as the brief carries them, on a continuity run PRIOR ANALYSIS, then WRITE-UPS; Part 2 what the analysis consolidates and its length band. The analysis is the only research artifact the next run reads.
A fund takes the same call over its agenda's write-ups — here the exposure-profile topic's — with the fund's FETCHED VALUES.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

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
| Prompt material | 1757 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst consolidating one holding's research for a portfolio review. Part 1 of
the message gives the inputs. Part 2 says what the analysis covers and how to return it.
~~~~

## User message (1467 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
BND (VANGUARD TOTAL BOND MARKET ETF).
Price: $72.38 per share.
Date: 2026-09-16.

FETCHED VALUES
The holding's data as its providers return it, each figure as reported (USD; B is billions, M is
millions); none is computed.
Profile: name Vanguard Total Bond Market ETF.
Fund: asset class Fixed Income; expense ratio 0.0003 (0.03%/yr); assets under management 340.0B; NAV
72.41.
Country weights: United States 94.0%, Supranational 2.0%, Canada 1.0%.
Dividends: 2.61 per share over the trailing twelve months.
Quote: 72.38 per share (the live print, undated).
Daily closes: 21 sessions from 2026-08-14 to 2026-09-14, 52-week low 72.33 on 2026-09-09, high 73.51
on 2026-08-18.
Treasury yields (FRED): 10-year 4.50%, 2-year 4.00%.

WRITE-UPS
This run's research on the holding, one write-up per topic, the contrary-evidence pass last.

Exposure profile
[stub: the fund's exposure-profile write-up]

======== PART 2: TASK ========

Write the holding's analysis as plain text — no code fence, no JSON, no heading before the first
line. It consolidates what this run's research established on the holding: what the write-ups under
WRITE-UPS establish, where their sources disagree, and what stays unanswered. Each figure is quoted
with the date or period its source gives for it, and its source is named: the page address the
write-up names, or FETCHED VALUES where the figure comes from there.

The analysis runs 900 to 1,800 words.
~~~~
