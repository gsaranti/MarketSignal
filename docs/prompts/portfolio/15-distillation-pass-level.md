# Distillation — pass-level sub-distillation

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v62`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, on a continuity run, the hierarchical route.
The pass-level sub-distillation, taken when a topic tree itself outgrows the tier-1 budget: one search of one topic, returning its summary and claims.
Distillation is explicitly non-thinking, grammar-constrained, and issued on the fast tier where the roster has one.
On the default roster, where the fast tier is the reasoner, the call issues on the reasoner at `num_ctx` 131072 and the rendered prompt is measured against that budget instead.
A reply that stops exactly at the 12288-token reservation is re-issued once on the reasoner with `num_predict` 32768, its stage label suffixed `(expanded)`.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6d |
| Stage label | `distill TSLA competitive-position pass 0` |
| Model | the roster's fast tier where one is configured, else the reasoner (the default roster) |
| Thinking | off (`think: false`) |
| Options | `{"min_p":0.0,"num_ctx":32768,"num_predict":12288,"presence_penalty":1.5,"temperature":0.7,"top_k":20,"top_p":0.8}` |
| Output protocol | the JSON schema below as the `format` grammar; no tools |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 3649 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst consolidating one topic of research on one holding for a portfolio
review. You will return summary and claims, as one JSON object. Part 1 of the message gives the
inputs. Part 2 defines those outputs and gives the shape to return.
~~~~

## User message (3257 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
TSLA (name unavailable).
Price: $358.97 per share.
Date: 2026-09-16.

STANDING CONDITIONS
Conditions the thesis on this holding is being watched against, each with its id.
- c-margin — Falsifier: Automotive gross margin ex-credits falls below 14% for two consecutive
quarters.
- c-price — Trigger: Price closes below $250.

TOPICS
The research on this holding, one topic at a time: what its searches established, then its claims,
each with the address of the page that states it, its reference, the publication date the search or
lead reported and the period the fact applies to. A claim marked "bears on" names the condition
under STANDING CONDITIONS it is evidence on.

TOPIC competitive-position — Competitive / business position
Search 1:
[stub: the root pass's findings — what the search established, in prose]
Claims:
- [stub: claim 1 — one dated fact from its source]
[https://ir.tesla.com/press-release/tesla-second-quarter-2026-results] — evidence_ref:
E1e6ce8151b7a246c52dbf433fd04714be016be6e9ce474e498888431ff70e841 — published: unknown; fact period:
unknown
- [stub: claim 2 — one dated fact from its source]
[https://www.acea.auto/pc-registrations/new-car-registrations-august-2026/] — evidence_ref:
Ee7813b90e372fb8834d605e28d329f92e9739329009aefc4d3ba84ac1fe3d60b — published: unknown; fact period:
unknown

======== PART 2: TASK ========
Determine the following from the inputs and return them as one JSON object in the shape under RETURN
SHAPE, with no code fence and no surrounding text; the names below are its fields.

1. summary — what this search established, as of the date under HOLDING: the figures with their
dates and periods as the claims state them, and what it left unanswered.

2. claims — every distinct statement the search rests on, one statement per claim, with source_url
the address shown beside it under TOPICS. related_condition_id is the id of the condition under
STANDING CONDITIONS the claim is evidence on — that it has tripped, is holding, or is at risk — else
null.


CLAIM RULES
For each claim, evidence_ref copies the reference of the supporting claim shown under TOPICS or
CONTRARY EVIDENCE; source_url copies its address. Keep each claim to one fact and period; separate
facts with different periods. Publication describes the source; fact period names the period the
fact applies to. An unknown date stays unknown. Compare periods only for the same measure and basis;
different periods remain distinct observations, with the latest applicable period informing a
current-state conclusion. For the same period, an explicit correction or revision supersedes its
predecessor; a later publication alone does not establish a revision. Where sources still conflict
or periods are incomparable, report the uncertainty and retain the conflicting claims with their own
references. Retrieval order and the analysis date never select a factual winner or supply a missing
fact date. Apply the same resolution in the combined findings, summaries, and every topic's claims.

RETURN SHAPE (every value is a placeholder; an array holds as many items as apply)
{"summary":"","claims":[{"claim":"","evidence_ref":"","source_url":"","related_condition_id":"<c-margin|c-price|null>"}]}
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
          "evidence_ref": {
            "type": "string"
          },
          "related_condition_id": {
            "enum": [
              "c-margin",
              "c-price",
              null
            ],
            "type": [
              "string",
              "null"
            ]
          },
          "source_url": {
            "type": "string"
          }
        },
        "required": [
          "claim",
          "source_url",
          "evidence_ref"
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
