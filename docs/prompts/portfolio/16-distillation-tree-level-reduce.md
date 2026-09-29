# Distillation — tree-level reduce over the pass outputs

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v51`; regenerate rather than edit (`docs/prompts/README.md`).*

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
| Prompt material | 4559 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst consolidating one topic of research on one holding for a portfolio
review. Part 1 of the message gives the inputs. Part 2 states what to determine from them and the
shape to return. You will return a summary and claims, as one JSON object.
~~~~

## User message (4152 chars)

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
each with the address of the page that states it. A claim marked "bears on" names the condition
under STANDING CONDITIONS it is evidence on. Prior findings are from an earlier analysis of the
topic, dated.

TOPIC competitive-position — Competitive / business position
Search 1 (summary):
[stub: pass 1's summary, as the pass-level call returned it]
Claims:
- [stub: claim 1 — one dated fact from its source]
[https://ir.tesla.com/press-release/tesla-second-quarter-2026-results] — evidence_ref:
E1e6ce8151b7a246c52dbf433fd04714be016be6e9ce474e498888431ff70e841 — published: unknown; fact period:
unknown — bears on c-margin
Search 2 (summary):
[stub: pass 2's summary, as the pass-level call returned it]
Claims:
- [stub: claim 3 — one dated fact from its source]
[https://www.nhtsa.gov/press-releases/nhtsa-opens-pe-fsd-v14] — evidence_ref:
Ea19800ab2b9e3b5267210688a3e7afaaa8ddbecc1a153964a24bd817c185c902 — published: unknown; fact period:
unknown
Prior findings (analysis of 2026-09-01):
[stub: the prior run's summary of this topic]
- [stub: prior claim 1, tied to a standing condition]
[https://ir.tesla.com/press-release/tesla-second-quarter-2026-results] — evidence_ref:
Ee6e3fba76d4e450c58965b9370cb4c76a84f83f8c38f91687372f8dccf413575 — published: unknown; fact period:
unknown — bears on c-margin
- [stub: prior claim 2] [https://www.acea.auto/pc-registrations/new-car-registrations-july-2026/] —
evidence_ref: Ea5c488fde44478b33183a91e48849ae568b1a35f3496d0b205329462ff7cacde — published:
unknown; fact period: unknown

======== PART 2: TASK ========
Determine the following from the inputs and return them as one JSON object in the shape at the end,
with no code fence and no surrounding text.

1. summary — what the topic's searches and prior findings establish, as of the date under HOLDING:
the figures with their dates and periods as the claims state them; where two claims cover the same
fact, reconcile by fact period and publication as described below, with prior findings assessed by
the same date and conflict rules below; and what the searches left unanswered.

2. claims — every distinct statement the topic rests on, one per item, with source_url the address
shown beside it under TOPICS: the claims from this time's searches and prior findings, reconciled by
fact period and publication as described below. related_condition_id is the id of the condition
under STANDING CONDITIONS the claim is evidence on — that it has tripped, is holding, or is at risk
— else null.


For each claim, evidence_ref copies the reference of the supporting claim shown under TOPICS or
CONTRARY EVIDENCE; source_url copies its address. Keep each claim to one fact and period; separate
facts with different periods. Publication describes the source; fact period describes when the fact
applies. An unknown date stays unknown. Compare periods only for the same measure and basis;
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
