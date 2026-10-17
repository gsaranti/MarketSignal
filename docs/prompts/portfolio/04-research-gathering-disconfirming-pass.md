# Research gathering — the disconfirming pass

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v85`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, a stock of the fixed evidence set (attempt 6, reconstructed), on its first analysis.
The disconfirming pass, run once per holding after its topics: the run's write-ups so far are the target and the task is to find what contradicts them.
Its synthesis (file 10) writes its own write-up and asks no follow-up.
The loop appends the reply countdown before every turn, so the third message is part of the first request.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the dates, URLs and headers around it are as rendered.
Under FETCHED VALUES, the issuer line, the quarterly statements and the consensus, the 52-week range, the 8-K list, the short-interest print, the street, insider and congressional rows, the surprises, the ratio lines, owner earnings, enterprise value, the float, the M&A match and the segments are synthetic values layered onto the fixture for these examples alone, their names stubbed; the closes are the fixed set's, as is the price under HOLDING.
The statements are sized to the engine's persisted metrics and the consensus to its band driver, and the surprises' EPS, the ratio lines, the enterprise-value row and the segments agree with the statements.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6c |
| Stage label | `holding-TSLA research disconfirming gathering turn 1` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | the two tools below; no `format` grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 9420 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst researching one holding for a portfolio review. Part 1 of the message
gives the inputs. Part 2 states what to find and when to stop. You search with web_search and fetch
with web_fetch, and write no findings in this conversation.
~~~~

## User message (7698 chars)

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
- daily closes: 1 session from 2026-09-14 to 2026-09-14

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

# NEWS LEADS
Recent headlines about the holding, each with its source and date. A headline is a lead, not
evidence.
- [stub: headline of lead 1] —
https://www.reuters.com/business/autos-transportation/tesla-cybercab-production-2026-09-10/
(reuters.com, 2026-09-10 14:02:00)
- [stub: headline of lead 2] — https://www.nhtsa.gov/press-releases/nhtsa-opens-pe-fsd-v14
(nhtsa.gov, 2026-09-12 09:30:00)

# TOPIC
Contrary evidence
- What contradicts the write-ups under WRITE-UPS SO FAR, or the picture they form together —
contrary data, claims that have failed, credible bear arguments?

# WRITE-UPS SO FAR
What the research on this holding has established so far, each topic under its heading.

## Competitive / business position
[stub: the topic's write-up so far — the root pass's account]

## Recent results and estimate revisions
[stub: the results topic's write-up]

======== PART 2: TASK ========
Find what the web shows on the question under TOPIC for this holding, as of the date under HOLDING.
The write-ups under WRITE-UPS SO FAR are what that question tests: search for evidence against them,
not for more evidence for them.
Use FETCHED VALUES as background to shape how the question is researched.

1. Search for what the question asks, then fetch and read the results and the leads under NEWS LEADS
most likely to answer it. Prefer a source tier nearer 0 and an extraction quality nearer 1 where the
question allows; a weak source lowers confidence in what it says, it does not exclude it, and a
figure that cannot be right is a defect of the source.
2. At most 8 tool calls in one reply.
3. Stop when the question is answered, or when what remains cannot be found: reply with one sentence
saying which, and no tool call.
~~~~

## Appended user message (the turn countdown)

~~~~text
# SEARCHING
Replies remaining, including this one: 8.
Pages fetched on the last reply are kept.
~~~~

## Tools

~~~~json
[
  {
    "function": {
      "description": "Search the web. Returns ranked results: title, url, source tier, published date, snippet. The source tier runs from 0 to 5: 0 a primary source (a filing, the issuer, a regulator); 1 a licensed data provider; 2 high-trust factual reporting; 3 a specialist source, trusted within its vertical; 4 opinion-heavy commentary; 5 sentiment only.",
      "name": "web_search",
      "parameters": {
        "properties": {
          "query": {
            "type": "string"
          }
        },
        "required": [
          "query"
        ],
        "type": "object"
      }
    },
    "type": "function"
  },
  {
    "function": {
      "description": "Fetch a page and return its article text under a header of: the url and title; the published date, where the search reported one; when it was retrieved; its source tier (0 to 5, as on a search result); the subjects its source is trusted on; and its extraction quality (0 to 1: the article text recovered against a full article's worth). A page marked stub did not yield its article (a paywall or script shell, or a fragment), so its text is not the page's content.",
      "name": "web_fetch",
      "parameters": {
        "properties": {
          "url": {
            "type": "string"
          }
        },
        "required": [
          "url"
        ],
        "type": "object"
      }
    },
    "type": "function"
  }
]
~~~~
