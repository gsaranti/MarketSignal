# Research gathering — the disconfirming pass

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v72`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, a stock of the fixed evidence set (attempt 6, reconstructed), on its first analysis.
The disconfirming pass, run once per holding after its topics: the run's write-ups so far are the target and the task is to find what contradicts them.
Its synthesis (file 10) writes its own write-up and asks no follow-up.
The loop appends the reply countdown before every turn, so the third message is part of the first request.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

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
| Prompt material | 3583 chars — the messages and tools as serialized |

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

FETCHED VALUES
The holding's data as its providers return it, each figure as reported (USD; B is billions, M is
millions); none is computed.
Quote: 358.97 per share (the live print, undated).
Daily closes: 1 sessions from 2026-09-14 to 2026-09-14, 52-week low 358.97 on 2026-09-14, high
358.97 on 2026-09-14.
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
Contrary evidence
- What contradicts the write-ups under WRITE-UPS SO FAR, or the picture they form together —
contrary data, claims that have failed, credible bear arguments?

WRITE-UPS SO FAR
This run's write-ups on the holding, each under its topic.

Competitive / business position
[stub: the topic's write-up so far — the root pass's account]

Recent results and estimate revisions
[stub: the results topic's write-up]

======== PART 2: TASK ========
Find what the web shows on the question under TOPIC for this holding, as of the date under HOLDING.
The write-ups under WRITE-UPS SO FAR are what that question tests: search for evidence against them,
not for more evidence for them.

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
