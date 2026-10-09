# Distillation — the merged write-ups

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v74`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, a stock of the fixed evidence set (attempt 6, reconstructed), on its first analysis.
A distillation is a non-thinking call with no grammar on the resident reasoner (the fast tier where the roster has one), issued only where the analysis prompt is over budget; the shape is the orchestrator's choice from size, never the model's, and the write-ups persist on the audit as written.
The merged shape: every write-up of the run under its topic, the contrary-evidence pass last, shortened in one call; taken where the analysis prompt is over budget and this prompt fits the widest issuable budget.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6d |
| Stage label | `distill TSLA` |
| Model | the roster's resident reasoner |
| Thinking | off (`think: false`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":12288,"presence_penalty":1.5,"temperature":0.7,"top_k":20,"top_p":0.8}` |
| Output protocol | free text: no tools, no grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 1306 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst shortening research write-ups on one holding for a portfolio review.
Part 1 of the message gives the inputs. Part 2 says what the document keeps and how to return it.
~~~~

## User message (1013 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
TSLA (Tesla, Inc.).
Price: $358.97 per share.
Date: 2026-09-16.

WRITE-UPS
This run's write-ups on the holding, each under its topic, the contrary-evidence pass last.

Competitive / business position
[stub: the topic's write-up so far — the root pass's account]

Recent results and estimate revisions
[stub: the results topic's write-up]

Contrary evidence
[stub: the disconfirming pass's write-up — how the contrary evidence bears on the run's write-ups]

======== PART 2: TASK ========

Write a shorter document as plain text — no code fence, no JSON, no heading before the first line.
It keeps every dated figure the write-ups under WRITE-UPS state, with the date or period and the
source each is given, every disagreement between pages they note, and every question they leave
open. It adds nothing the write-ups under WRITE-UPS do not say: this is consolidation, not new
reasoning.

The document runs at most half the length of the write-ups and at most 1,200 words.
~~~~
