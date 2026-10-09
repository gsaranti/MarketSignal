# Analysis — stock, continuity run

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v73`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, on a continuity run over the prior analysis of 2026-09-02.
The analysis call is a thinking call with no grammar: Part 1 the holding header and FETCHED VALUES as the brief carries them, on a continuity run PRIOR ANALYSIS, then WRITE-UPS; Part 2 what the analysis consolidates and its length band. The analysis is the only research artifact the next run reads.
On a continuity run the prior analysis renders verbatim as PRIOR ANALYSIS under its date, never distilled, and the task asks what it said that this run confirms, revises or leaves untouched; a topic with no write-up this run keeps what it says.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6d |
| Stage label | `analysis TSLA` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | free text: no tools, no grammar |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 2118 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst consolidating one holding's research for a portfolio review. Part 1 of
the message gives the inputs. Part 2 says what the analysis covers and how to return it.
~~~~

## User message (1823 chars)

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
Daily closes: 2 sessions from 2026-09-02 to 2026-09-14, 52-week low 348.20 on 2026-09-02, high
358.97 on 2026-09-14; close on the prior analysis date 2026-09-02: 348.20 (2026-09-02).
Treasury yields (FRED): 10-year 4.50%, 2-year 4.00%.

PRIOR ANALYSIS (written 2026-09-02)
[stub: the prior run's analysis — the holding's research memory, as the analysis call returned it on
2026-09-02]

WRITE-UPS
This run's research on the holding, one write-up per topic, the contrary-evidence pass last.

Competitive / business position
[stub: the topic's write-up so far — the root pass's account]

Recent results and estimate revisions
[stub: the results topic's write-up]

Contrary evidence
[stub: the disconfirming pass's write-up — how the contrary evidence bears on the run's write-ups]

======== PART 2: TASK ========

Write the holding's analysis as plain text — no code fence, no JSON, no heading before the first
line. It consolidates what this run's research established on the holding: what the write-ups under
WRITE-UPS establish, where their sources disagree, and what stays unanswered. Each figure is quoted
with the date or period its source gives for it, and its source is named: the page address the
write-up names, or FETCHED VALUES where the figure comes from there. It also states what the
analysis under PRIOR ANALYSIS said that this run's research confirms, revises or leaves untouched; a
topic the write-ups do not cover this run keeps what PRIOR ANALYSIS says about it.

The analysis runs 900 to 1,800 words.
~~~~
