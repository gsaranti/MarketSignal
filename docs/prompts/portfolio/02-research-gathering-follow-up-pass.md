# Research gathering — follow-up pass

*Generated from the code at `portfolio-v50` by `fixed_evidence::prompt_examples`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, a stock of the fixed evidence set (attempt 6, reconstructed), on its first analysis.
The follow-up pass on the same topic, taken when the root pass's synthesis proposed a question worth one more pass.
The approved question and the topic's claims so far ride in Part 1, so the search starts from what the root pass established.
The loop appends the reply countdown before every turn, so the third message is part of the first request.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

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
| Prompt material | 3546 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst researching one holding for a portfolio review. Part 1 of the message
gives the inputs. Part 2 states what to find and when to stop. You search with web_search and fetch
with web_fetch, and write nothing up in this conversation.
~~~~

## User message (2091 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
TSLA (name unavailable).
Price: $358.97 per share.
Date: 2026-09-16.

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

FOLLOW-UP
The question this pass pursues, and why it was proposed.
[stub: the approved follow-up question from the root pass's synthesis]
Because: [stub: why the question matters to the thesis]

CLAIMS SO FAR
What this topic's earlier searching established, each with its source.
- [stub: claim 1 — one dated fact from its source]
[https://ir.tesla.com/press-release/tesla-second-quarter-2026-results]
  publication (search/seed report): unknown; fact period: unknown
- [stub: claim 2 — one dated fact from its source]
[https://www.acea.auto/pc-registrations/new-car-registrations-august-2026/]
  publication (search/seed report): unknown; fact period: unknown

======== PART 2: TASK ========
Find what the web shows on the FOLLOW-UP question for this holding, as of the date under HOLDING;
the TOPIC questions are its context, and CLAIMS SO FAR need no second search.

1. Search for what the questions ask, then fetch and read the results and the leads under NEWS LEADS
most likely to answer them. Prefer a lower tier number and a higher extraction quality where the
questions allow; a weak source lowers confidence in what it says, it does not exclude it, and a
figure that cannot be right is a defect of the source.
2. At most 8 tool calls in one reply.
3. Stop when the questions are answered, or when what remains cannot be found: reply with one
sentence saying which, and no tool call.
~~~~

## Appended user message (the turn countdown)

~~~~text
SEARCHING
Replies remaining, including this one: 8.
~~~~

## Tools

~~~~json
[
  {
    "function": {
      "description": "Search the web. Returns ranked results: title, url, tier, published date, snippet. The tier runs from 0 to 5: 0 is a primary source (a filing, the issuer, a regulator), 5 is sentiment only.",
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
      "description": "Fetch a page and return its article text under a header of: the url and title; the published date, where the search reported one; when it was retrieved; its tier (0 to 5, as on a search result); the subjects its source is trusted on (its tier holds within them); and its extraction quality (0 to 1: the article text recovered against a full article's worth). A page marked stub did not yield its article (a paywall or script shell, or a fragment), so its text is not the page's content.",
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
