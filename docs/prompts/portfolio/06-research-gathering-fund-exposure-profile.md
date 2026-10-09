# Research gathering — a fund's exposure-profile topic

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v74`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: BND, the synthetic total bond market ETF the fixed evidence set carries for the role/risk branch.
The root-pass shape on a fund: the agenda's fund topics replace the stock topics, and the header and FETCHED VALUES name the fund's reported lines.
Everything else — the leads, the countdown, the tools, the stopping rule — is the stock shape.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6c |
| Stage label | `holding-BND research fund-exposure-profile gathering turn 1` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | the two tools below; no `format` grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 3478 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst researching one holding for a portfolio review. Part 1 of the message
gives the inputs. Part 2 states what to find and when to stop. You search with web_search and fetch
with web_fetch, and write nothing up in this conversation.
~~~~

## User message (1991 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
BND (VANGUARD TOTAL BOND MARKET ETF).
Price: $72.38 per share.
Date: 2026-09-16.

FETCHED VALUES
The holding's data as its providers return it, each figure as reported (USD; B is billions, M is
millions; a yield or a return as a percentage); none is computed, and a cell the provider left empty
reads (gap).
Profile: name Vanguard Total Bond Market ETF.
Fund: asset class Fixed Income; expense ratio 0.0003 (0.03%/yr); assets under management 340.0B; NAV
72.41.
Country weights: United States 94.0%, Supranational 2.0%, Canada 1.0%.
Quote: 72.38 per share (the live print, undated).
Daily closes: 21 sessions from 2026-08-14 to 2026-09-14.
Treasury yields (FRED): 10-year 4.50%, 2-year 4.00%.

NEWS LEADS
Recent headlines about the holding, each with its source and date. A headline is a lead, not
evidence.
- [stub: headline of lead 1] —
https://www.reuters.com/markets/funds/vanguard-bond-index-fee-cut-2026-09-08/ (reuters.com,
2026-09-08 13:10:00)
- [stub: headline of lead 2] — https://www.ft.com/content/treasury-curve-steepens-2026-09-11
(ft.com, 2026-09-11 16:45:00)

TOPIC
Exposure profile
- What exposure does the fund supply — its largest holdings, its sector, country and factor tilts,
and how they have shifted?
- What direct or lower-cost vehicles supply the same exposure?

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
