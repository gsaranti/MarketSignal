# Distillation — single pass, fund

*Generated from the code at `portfolio-v50` by `fixed_evidence::prompt_examples`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: BND, the synthetic total bond market ETF the fixed evidence set carries for the role/risk branch.
A fund's reduce is consolidation only: the combined findings and the topic layer, with the standing condition rendered for citation, and no source text or typed field.
Distillation is explicitly non-thinking, grammar-constrained, and issued on the fast tier where the roster has one.
On the default roster, where the fast tier is the reasoner, the call issues on the reasoner at `num_ctx` 131072 and the rendered prompt is measured against that budget instead.
A reply that stops exactly at the 12288-token reservation is re-issued once on the reasoner with `num_predict` 32768, its stage label suffixed `(expanded)`.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6d |
| Stage label | `distill BND` |
| Model | the roster's fast tier where one is configured, else the reasoner (the default roster) |
| Thinking | off (`think: false`) |
| Options | `{"min_p":0.0,"num_ctx":32768,"num_predict":12288,"presence_penalty":1.5,"temperature":0.7,"top_k":20,"top_p":0.8}` |
| Output protocol | the JSON schema below as the `format` grammar; no tools |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 3651 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst consolidating the research on one holding for a portfolio review. Part
1 of the message gives the inputs. Part 2 states what to determine from them and the shape to
return. You will return combined findings and findings per topic, as one JSON object.
~~~~

## User message (3232 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
BND (VANGUARD TOTAL BOND MARKET ETF).
Price: $72.38 per share.
Date: 2026-09-16.

STANDING CONDITIONS
Conditions the thesis on this holding is being watched against, each with its id.
- c-dur — Trigger: Effective duration rises above 7 years.

TOPICS
The research on this holding, one topic at a time: what its searches established, then its claims,
each with the address of the page that states it. A claim marked "bears on" names the condition
under STANDING CONDITIONS it is evidence on.

TOPIC fund-exposure-profile — Exposure profile
Search 1:
[stub: the root pass's findings on the fund's exposure profile]
Claims:
- [stub: claim 1 — one dated fact from its source]
[https://investor.vanguard.com/investment-products/etfs/profile/bnd] — evidence_ref:
E9dcc02ac89ec51165bc5c21754dcfb72484725409345ee85a1f938dfa9e15dc2 — publication (search/seed
report): unknown; fact period: unknown

======== PART 2: TASK ========
Determine the following from the inputs and return them as one JSON object in the shape at the end,
with no code fence and no surrounding text.

1. combined_findings — what the research established on this holding, across every topic under
TOPICS, as of the date under HOLDING: the figures with their dates and periods as the claims state
them; where two claims cover the same fact, reconcile by fact period and publication as described
below; and what the searches left unanswered.

2. topics — exactly one object per topic under TOPICS, in that order. topic_key is the key as shown.
summary is what the topic's searches establish, as of the date under HOLDING. claims is every
distinct statement the topic rests on, one per item, with source_url the address shown beside it
under TOPICS: a fact two topics state is one claim, under the topic it belongs to.
related_condition_id is the id of the condition under STANDING CONDITIONS the claim is evidence on —
that it has tripped, is holding, or is at risk — else null.


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
{"combined_findings":"","topics":[{"topic_key":"<fund-exposure-profile>","summary":"","claims":[{"claim":"",
"evidence_ref":"","source_url":"","related_condition_id":"<c-dur|null>"}]}]}
~~~~

## Response schema (`format`)

~~~~json
{
  "properties": {
    "combined_findings": {
      "type": "string"
    },
    "topics": {
      "items": {
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
                    "c-dur",
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
          },
          "topic_key": {
            "enum": [
              "fund-exposure-profile"
            ],
            "type": "string"
          }
        },
        "required": [
          "topic_key",
          "summary",
          "claims"
        ],
        "type": "object"
      },
      "type": "array"
    }
  },
  "required": [
    "combined_findings",
    "topics"
  ],
  "type": "object"
}
~~~~
