# Rendered prompt examples

One file per local-model call shape, rendered from the code, so each example is exactly what the running app sends at the current prompt stamp.
Each file's header names the stamp at which that file last changed, not the current stamp: a file whose prompt did not move is never rewritten.
The set is generated, never hand-edited: a prompt change lands in the code, and the files are regenerated from it.
Today the set covers the Portfolio Analysis job, under `portfolio/`; the Trade Opportunities job's set lands beside it when that job is built.

## What each file carries

A request table: the workflow step the call belongs to (`portfolio-workflow.md`), the stage label the run tracker shows, the model tier, the thinking flag, the generation options, the output protocol (a `format` grammar or the two research tools), the residency setting, and the size of the prompt material as the input guard measures it.
Then every message of the request as sent, verbatim, each in its own fenced block: the system message, the user message, and on a gathering turn what the loop appends (the reply countdown, the model's echoed tool calls, the tool results).
Then the tools JSON or the response schema the grammar enforces, and any variant whose lines differ from the base message.
`portfolio/00-contents.md` lists the files in pipeline order.

## The data behind the examples

The holdings are the fixed evidence set of big-run attempt 6 (`src-tauri/src/portfolio/fixtures/attempt-6/README.md`): TSLA for the stock shapes, SPMO for the priced-fund shapes, and the synthetic BND bond fund for the role/risk shapes.
Their financials, engine output, position and ledger are the reconstructed persisted values, with synthetic position economics.
Every parsed article or research field is a stub: a bracketed `[stub: …]` label that keeps the field's place and names what stood there.
Ids, dates, URLs, hosts, condition ids and section headers are never stubbed, so the tiers, the ties and the glosses that read them render as on a run.
The latest report's sections under MARKET ANALYSIS are stubbed the same way; the report's dated stance lines stay.
The continuity shapes carry a hand-written prior: attempt 6's persisted verdict re-dated to 2026-09-02 with a prior spot 3% under today's on the priced holdings, and a stub first run of 2026-09-03 on the role/risk fund.
Attempt 6 wrote no second run, so the retrospective figures in those files are illustrative, not observed.

## Regenerating

```
cd src-tauri
MARKET_SIGNAL_PROMPT_EXAMPLES_DIR=../docs/prompts/portfolio cargo test portfolio_prompt_examples_write -- --ignored
```

The writer removes Markdown files no longer in the set and writes each file in the set, `00-contents.md` included, only where its text changed with the header's stamp set aside.
An untouched prompt's file is therefore never written, and a regeneration's diff shows only the prompts that moved (user rule, 2026-09-28).
The non-ignored `portfolio_prompt_examples_render` test builds the same set on every `cargo test`, so the generator cannot rot beside the prompts.
Regenerate whenever a prompt stamp moves; the diff is the review surface for the prompt change.

## Where the contracts live

The examples show the prompts; they specify nothing.
Each call's contract is canonical in `portfolio-workflow.md` at its step, the prompt posture in `local-models.md §Prompt posture`, and the research loop in `web-research.md §The research loop and context management`.
