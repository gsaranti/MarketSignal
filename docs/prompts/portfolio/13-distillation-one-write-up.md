# Distillation — one write-up of the per-write-up shape

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v82`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, a stock of the fixed evidence set (attempt 6, reconstructed), on its first analysis.
A distillation is a non-thinking call with no grammar on the resident reasoner (the fast tier where the roster has one), issued only where the analysis prompt is over budget; the shape is the orchestrator's choice from size, never the model's, and the write-ups persist on the audit as written.
The per-write-up shape's first calls, one per write-up, taken where the merged prompt outgrows the widest issuable budget; the stage label names the topic.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6d |
| Stage label | `distill TSLA competitive-position` |
| Model | the roster's resident reasoner |
| Thinking | off (`think: false`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":12288,"presence_penalty":1.5,"temperature":0.7,"top_k":20,"top_p":0.8}` |
| Output protocol | free text: no tools, no grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 1122 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst shortening research write-ups on one holding into one shorter document
for a portfolio review. Part 1 of the message gives the inputs. Part 2 says what the document keeps
and how to return it.
~~~~

## User message (809 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
TSLA (Tesla, Inc.).
Price: $358.97 per share.
Date: 2026-09-16.

WRITE-UP
One write-up on this holding, under its topic.

Competitive / business position
[stub: the topic's write-up so far — the root pass's account]

======== PART 2: TASK ========

Condense the content under WRITE-UP into a shorter document as plain text — no code fence, no JSON,
no heading before the first line. It states what the write-up establishes in fewer words: every
dated figure it quotes, with the date or period and the source each is given, every disagreement
between pages it notes, and what it leaves unanswered. The document adds nothing the write-up does
not say: consolidation, not new reasoning.

The document runs at most half the length of the write-up and at most 1,200 words.
~~~~
