# Research gathering — root pass, first analysis

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v74`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, a stock of the fixed evidence set (attempt 6, reconstructed), on its first analysis.
The first gathering turn of a holding's first research topic on a first analysis.
Part 1 leads with the holding-constant block — the header, FETCHED VALUES as the thesis-document message renders it, the two news leads — then the topic; Part 2 the search task and its stopping rule.
The loop appends the reply countdown before every turn, so the third message is part of the first request.
The model answers with tool calls and writes nothing up; the orchestrator runs each call and feeds the result back as a tool message (the tool-turn file shows the second request).
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.
Under FETCHED VALUES, the issuer line, the 52-week range, the 8-K list, the short-interest print, the street, insider and congressional rows, the surprises, the ratio lines, owner earnings, enterprise value, the float, the M&A match and the segments are synthetic values layered onto the fixture for these examples alone, their names stubbed; the statements, the quote and the closes are the fixed set's.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6c |
| Stage label | `holding-TSLA research competitive-position gathering turn 1` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | the two tools below; no `format` grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 6509 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst researching one holding for a portfolio review. Part 1 of the message
gives the inputs. Part 2 states what to find and when to stop. You search with web_search and fetch
with web_fetch, and write nothing up in this conversation.
~~~~

## User message (5006 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
TSLA (Tesla, Inc.).
Price: $358.97 per share.
Date: 2026-09-16.

FETCHED VALUES
The holding's data as its providers return it, each figure as reported (USD; B is billions, M is
millions; a yield or a return as a percentage); none is computed, and a cell the provider left empty
reads (gap).
Profile: name Tesla, Inc.; exchange NASDAQ; sector Consumer Cyclical; industry Auto - Manufacturers.
Quote: 358.97 per share (the live print, undated), 52-week low 215.38 and high 430.76 as served.
Daily closes: 1 sessions from 2026-09-14 to 2026-09-14.
8-K filings of the trailing twelve months, newest first (filing date: items): 2026-07-23: 2.02,
9.01; 2026-05-15 (8-K/A): 5.02; 2026-04-22: 2.02, 9.01.
Short interest (FINRA, settlement 2026-08-31): 80000000 shares; prior settlement 76000000; average
daily volume 95000000; days to cover 0.84.
Street price targets: consensus 387.69, median 394.87, low 179.49, high 717.94; published last month
4 targets averaging 412.82, last quarter 15 targets averaging 384.10, last year 48 targets averaging
358.97.
Analyst ratings: strong buy 3, buy 14, hold 18, sell 7, strong sell 2; consensus Hold.
Rating actions, newest first (date: firm, previous grade to new grade, action): 2026-09-10: [stub:
grading firm], Hold to Buy, upgrade; 2026-07-24: [stub: grading firm], Buy to Buy, maintain;
2026-04-23: [stub: grading firm], (gap) to Hold, initiate.
FMP rating C (overall 3; discounted cash flow 2, return on equity 3, return on assets 3, debt to
equity 4, price to earnings 1, price to book 1).
Insider trades, newest first (transaction date: name, role, type, shares at price, filing date):
2026-09-05: [stub: insider name], officer, S-Sale, 10000 shares at 351.79, filed 2026-09-08;
2026-08-20: [stub: insider name], director, A-Award, 2500 shares at 0.00, filed 2026-08-21;
2026-07-30: [stub: insider name], 10 percent owner, S-Sale, 50000 shares at 337.43, filed
2026-08-01.
Insider statistics, 2026 Q3: 1 acquiring and 9 disposing transactions; 2500 shares acquired, 310000
disposed.
Congressional trades, newest first (transaction date: chamber, member, owner, type, amount,
disclosure date): 2026-08-14: House, [stub: member], Joint, Purchase, $1,001 - $15,000, disclosed
2026-09-02; 2026-06-03: Senate, [stub: member], Spouse, Sale, $15,001 - $50,000, disclosed
2026-07-01.
Earnings surprises, newest first (announcement date: EPS actual vs estimate, revenue actual):
2026-07-22: EPS 0.40 vs 0.43 estimated, revenue 22.5B; 2026-04-22: EPS 0.27 vs 0.42 estimated,
revenue 19.3B; 2026-01-29: EPS 0.73 vs 0.77 estimated, revenue 25.7B; 2025-10-22: EPS 0.72 vs 0.60
estimated, revenue 25.2B.
Next earnings: 2026-10-21 (EPS estimate 0.55).
Trailing-twelve-month ratios: P/E 95.00; EV/EBITDA 55.00; EV/sales 8.00; P/B 12.50; FCF yield 0.6%;
ROIC 7.0%; ROE 11.0%; net debt/EBITDA -1.20.
Owner earnings (FY2026 Q2, period end 2026-06-30): 2.1B; 0.62 per share.
Enterprise value (2026-06-30): 790.0B; market capitalization 780.0B; total debt 13.0B; cash 33.0B.
Float (2026-09-01): float shares 2.8B; shares outstanding 3.2B; free float 86.9%.
M&A (the market-wide feed, trailing twelve months): acquirer of [stub: counterparty], announced
2026-05-20.
Revenue by product, newest fiscal year first: FY2025 (period end 2025-12-31) Automotive 72.0B,
Services and other 11.0B, Energy generation and storage 10.0B; FY2024 (period end 2024-12-31)
Automotive 77.0B, Services and other 10.5B, Energy generation and storage 10.0B.
Revenue by geography, newest fiscal year first: FY2025 (period end 2025-12-31) United States 44.0B,
Other 28.0B, China 21.0B; FY2024 (period end 2024-12-31) United States 47.0B, Other 29.0B, China
21.5B.
Treasury yields (FRED): 10-year 4.50%, 2-year 4.00%.

NEWS LEADS
Recent headlines about the holding, each with its source and date. A headline is a lead, not
evidence.
- [stub: headline of lead 1] —
https://www.reuters.com/business/autos-transportation/tesla-cybercab-production-2026-09-10/
(reuters.com, 2026-09-10 14:02:00)
- [stub: headline of lead 2] — https://www.nhtsa.gov/press-releases/nhtsa-opens-pe-fsd-v14
(nhtsa.gov, 2026-09-12 09:30:00)

TOPIC
Competitive / business position
- How is the company's competitive position evolving — share, moat, pricing power?
- Which competitors or substitutes are gaining or losing against it?

======== PART 2: TASK ========
Find what the web shows on each question under TOPIC for this holding, as of the date under HOLDING.

1. Search for what the questions ask, then fetch and read the results and the leads under NEWS LEADS
most likely to answer them. Prefer a source tier nearer 0 and an extraction quality nearer 1 where
the questions allow; a weak source lowers confidence in what it says, it does not exclude it, and a
figure that cannot be right is a defect of the source.
2. At most 8 tool calls in one reply.
3. Stop when the questions are answered, or when what remains cannot be found: reply with one
sentence saying which, and no tool call.
~~~~

## Appended user message (the turn countdown)

~~~~text
SEARCHING
Replies remaining, including this one: 8.
Pages fetched on the last reply are kept.
~~~~

## Tools

~~~~json
[
  {
    "function": {
      "description": "Search the web. Returns ranked results: title, url, source tier, published date, snippet. The source tier runs from 0 to 5: 0 is a primary source (a filing, the issuer, a regulator), 5 is sentiment only.",
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
