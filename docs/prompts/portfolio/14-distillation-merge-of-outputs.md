# Distillation — the merge of the per-write-up outputs

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v82`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, a stock of the fixed evidence set (attempt 6, reconstructed), on its first analysis.
A distillation is a non-thinking call with no grammar on the resident reasoner (the fast tier where the roster has one), issued only where the analysis prompt is over budget; the shape is the orchestrator's choice from size, never the model's, and the write-ups persist on the audit as written.
The per-write-up shape's last call, over the shortened write-ups under their topics; it runs whenever that shape is taken, so the shape spends the write-ups plus one call.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6d |
| Stage label | `distill TSLA merge` |
| Model | the roster's resident reasoner |
| Thinking | off (`think: false`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":12288,"presence_penalty":1.5,"temperature":0.7,"top_k":20,"top_p":0.8}` |
| Output protocol | free text: no tools, no grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 1355 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst shortening research write-ups on one holding into one shorter document
for a portfolio review. Part 1 of the message gives the inputs. Part 2 says what the document keeps
and how to return it.
~~~~

## User message (1036 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
TSLA (Tesla, Inc.).
Price: $358.97 per share.
Date: 2026-09-16.

WRITE-UPS
The research on this holding, one write-up per topic, each already shortened, the contrary-evidence
write-up last.

Competitive / business position
[stub: the write-up shortened by its own distillation call]

Recent results and estimate revisions
[stub: the write-up shortened by its own distillation call]

Contrary evidence
[stub: the write-up shortened by its own distillation call]

======== PART 2: TASK ========

Consolidate the write-ups under WRITE-UPS into one shorter document as plain text — no code fence,
no JSON, no heading before the first line. It states what they establish in fewer words: every dated
figure they quote, with the date or period and the source each is given, every disagreement between
pages they note, and what they leave unanswered. It adds nothing they do not say: consolidation, not
new reasoning.

The document runs at most half the length of the write-ups and at most 1,200 words.
~~~~
