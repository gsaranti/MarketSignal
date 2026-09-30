# Distillation — single pass, fund

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v64`; regenerate rather than edit (`docs/prompts/README.md`).*

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
| Prompt material | 3587 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst consolidating the research on one holding for a portfolio review. You
will return combined_findings and topics, as one JSON object. Part 1 of the message gives the
inputs. Part 2 defines those outputs and gives the shape to return.
~~~~

## User message (3190 chars)

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
The research on this holding, one topic at a time, each headed by its key and its title: what its
searches established, then its claims. Each claim carries: its id; the address of the page that
states it; the publication date the search or lead reported; and the period the fact applies to. A
claim marked "bears on" names the condition under STANDING CONDITIONS it is evidence on.

TOPIC fund-exposure-profile — Exposure profile
Search 1:
[stub: the root pass's findings on the fund's exposure profile]
Claims:
- C1: [stub: claim 1 — one dated fact from its source]
[https://investor.vanguard.com/investment-products/etfs/profile/bnd] — published: unknown; fact
period: 2026-08-31

======== PART 2: TASK ========
Determine the following from the inputs and return them as one JSON object in the shape under RETURN
SHAPE, with no code fence and no surrounding text; the names below are its fields.

1. combined_findings — what the research established on this holding, across every topic under
TOPICS, as of the date under HOLDING: the figures with their dates and periods as the claims state
them; where two claims cover the same fact, reconcile them by the rules under CLAIM RULES; and what
the searches left unanswered.

2. topics — exactly one object per topic under TOPICS, in that order. topic_key is the topic's key
under TOPICS. summary is what the topic's searches establish, as of the date under HOLDING. claims
is every distinct statement the topic rests on, one statement per claim. evidence_id is the id of
the claim under TOPICS the statement rests on. A fact two topics state is one claim, under the topic
it belongs to. related_condition_id is the id of the condition under STANDING CONDITIONS the claim
is evidence on — that it has tripped, is holding, or is at risk — else null.


CLAIM RULES
Keep each claim to one fact and period; separate facts with different periods. Publication describes
the source; fact period names the period the fact applies to. An unknown date stays unknown. Compare
periods only for the same measure and basis; different periods remain distinct observations, with
the latest applicable period informing a current-state conclusion. For the same period, an explicit
correction or revision supersedes its predecessor; a later publication alone does not establish a
revision. Where sources still conflict or periods are incomparable, report the uncertainty and
retain the conflicting claims with their own ids. Retrieval order and the analysis date never select
a factual winner or supply a missing fact date. Apply the same resolution in the combined findings,
summaries, and every topic's claims.

RETURN SHAPE (every value is a placeholder; an array holds as many items as apply)
{"combined_findings":"","topics":[{"topic_key":"<fund-exposure-profile>","summary":"","claims":[{"claim":"",
"evidence_id":"<C1>","related_condition_id":"<c-dur|null>"}]}]}
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
                "evidence_id": {
                  "enum": [
                    "C1"
                  ],
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
