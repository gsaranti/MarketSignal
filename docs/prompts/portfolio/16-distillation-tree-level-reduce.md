# Distillation — tree-level reduce over the pass outputs

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v70`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, on a continuity run, the hierarchical route.
The tree-level reduce that follows the pass-level calls: each pass output renders as its summary and claims, the prior merges here, and the call returns the topic's summary and claims.
Distillation is explicitly non-thinking, grammar-constrained, and issued on the fast tier where the roster has one.
On the default roster, where the fast tier is the reasoner, the call issues on the reasoner at `num_ctx` 131072 and the rendered prompt is measured against that budget instead.
A reply that stops exactly at the 12288-token reservation is re-issued once on the reasoner with `num_predict` 32768, its stage label suffixed `(expanded)`.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6d |
| Stage label | `distill TSLA competitive-position reduce` |
| Model | the roster's fast tier where one is configured, else the reasoner (the default roster) |
| Thinking | off (`think: false`) |
| Options | `{"min_p":0.0,"num_ctx":32768,"num_predict":12288,"presence_penalty":1.5,"temperature":0.7,"top_k":20,"top_p":0.8}` |
| Output protocol | the JSON schema below as the `format` grammar; no tools |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 3560 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst consolidating one topic of research on one holding for a portfolio
review. You will return summary and claims, as one JSON object. Part 1 of the message gives the
inputs. Part 2 defines those outputs and gives the shape to return.
~~~~

## User message (3176 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
TSLA (name unavailable).
Price: $358.97 per share.
Date: 2026-09-16.

TOPICS
The research on one topic of this holding, headed by its key and its title: what its searches
established, then its claims. Each claim carries: its id; the address of the page that states it;
the publication date the search or lead reported; and the period the fact applies to. Prior findings
are from an earlier analysis of the topic, dated.

TOPIC competitive-position — Competitive / business position
Search 1 (summary):
[stub: pass 1's summary, as the pass-level call returned it]
Claims:
- C1: [stub: claim 1 — one dated fact from its source]
[https://ir.tesla.com/press-release/tesla-second-quarter-2026-results] — published: 2026-07-22; fact
period: 2026-Q2
Search 2 (summary):
[stub: pass 2's summary, as the pass-level call returned it]
Claims:
- C2: [stub: claim 3 — one dated fact from its source]
[https://www.nhtsa.gov/press-releases/nhtsa-opens-pe-fsd-v14] — published: unknown; fact period:
unknown
Prior findings (analysis of 2026-09-01):
[stub: the prior run's summary of this topic]
- C3: [stub: prior claim 1, tied to a standing condition]
[https://ir.tesla.com/press-release/tesla-second-quarter-2026-results] — published: 2026-07-22; fact
period: 2026-Q2
- C4: [stub: prior claim 2]
[https://www.acea.auto/pc-registrations/new-car-registrations-july-2026/] — published: 2026-08-26;
fact period: 2026-07

======== PART 2: TASK ========
Determine the following from the inputs and return them as one JSON object in the shape under RETURN
SHAPE, with no code fence and no surrounding text; the names below are its fields.

1. summary — what the topic's searches and prior findings establish, as of the date under HOLDING:
the figures with their dates and periods as the claims state them; where two claims cover the same
fact, reconcile them by the rules under CLAIM RULES; and what the searches left unanswered.

2. claims — every distinct statement the topic rests on, one statement per claim. The statements
come from the searches and the prior findings, reconciled by the rules under CLAIM RULES.
evidence_id is the id of the claim under TOPICS the statement rests on.


CLAIM RULES
Keep each claim to one fact and period; separate facts with different periods. Publication describes
the source; fact period names the period the fact applies to. An unknown date stays unknown. Compare
periods only for the same measure and basis; different periods remain distinct observations, with
the latest applicable period informing a current-state conclusion. For the same period, an explicit
correction or revision supersedes its predecessor; a later publication alone does not establish a
revision. Where sources still conflict or periods are incomparable, report the uncertainty and
retain the conflicting claims with their own ids. Retrieval order and the analysis date never select
a factual winner or supply a missing fact date. Apply the same resolution in the summary and the
claims.

RETURN SHAPE (every value is a placeholder; an array holds as many items as apply)
{"summary":"","claims":[{"claim":"","evidence_id":"<C1|C2|C3|C4>"}]}
~~~~

## Response schema (`format`)

~~~~json
{
  "properties": {
    "claims": {
      "items": {
        "properties": {
          "claim": {
            "type": "string"
          },
          "evidence_id": {
            "enum": [
              "C1",
              "C2",
              "C3",
              "C4"
            ],
            "type": "string"
          }
        },
        "required": [
          "claim",
          "evidence_id"
        ],
        "type": "object"
      },
      "type": "array"
    },
    "summary": {
      "type": "string"
    }
  },
  "required": [
    "summary",
    "claims"
  ],
  "type": "object"
}
~~~~
