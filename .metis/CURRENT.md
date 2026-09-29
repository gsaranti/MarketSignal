# Current session handoff

## Active task

The prompt-by-prompt read-through of `docs/prompts/portfolio/` (26 files in `00-contents.md` order), the user reading each rendered prompt and ruling as they go; the single big confirmation run (attempt 9) waits behind it on the user's word.

## What happened

The session (2026-09-28) opened the read-through at file 01, the gathering brief's first-root shape, and landed its eight rulings as one stamp bump, `portfolio-v50` (`69d6055`): the tier scale states its range; the TOOL RESULTS legend left Part 1 and the two tool descriptions now state what a search result and a fetched page carry, the page header's subject field renamed `trusted on`; item 1 reads the pages under PAGES ALREADY RETRIEVED only where one is shown and names the news leads as fetch candidates beside the search results under one relevance test; the fallible-source clause moved from the fetch description onto Part 2's weighing sentence; the gathering system message names `web_search` and `web_fetch`.
The principles behind them, to apply to every later file: state what the model would otherwise infer; Part 1 holds inputs only and every heading in it is named by Part 2; a legend about a tool's result lives on the tool description; Part 2 never points at absent content; one instruction, one test; safety text sits by the data and weighing text in the task, neither in a tool description; the same words across description, header and gloss; semicolon lists with bracketed explanations.
A second commit (`1e40fc8`) changed the examples writer: it rewrites only the files whose text changed with the header's stamp set aside, so an untouched prompt's file is never written and its header reads "last changed at `portfolio-vNN`" (user rule).
The checkpoint trail is unchanged.

## Current state

Nothing in flight; the working tree is clean at `1e40fc8`.

- **Debut stamp set:** `portfolio-v50` / `checkpoint-v15` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`, portability 11.
- **Review method:** the user reads the file and comments; nothing is analysed or proposed unasked; a ruling that changes prompt text lands as the builder edit, its pins (the research tests and the fixed-evidence harness), the docs mirror (`web-research.md`, `portfolio-workflow.md`, sentence-per-line with a `Since` record), a history line on `portfolio::PROMPT_VERSION` and its `pipeline.rs` pin, the regenerated examples and the full gate; one stamp bump per commit, the next being `portfolio-v51`.
- **Carry-overs for later files (raise when the file comes up):** file 02 — item 1's "the questions" is ambiguous between the FOLLOW-UP question and TOPIC; file 08 — the synthesis EVIDENCE gloss still has the unstated extraction-quality range, the opaque stub sentence, the frame sentence inside a Part 1 gloss and the comma-list shape (only its `trusted on` phrase was aligned, for the shared header renderer).
- **Store:** attempt 8's three checkpoint holding rows persist under `portfolio-v48`, `portfolio_runs` 0, `job_runs` max 8; attempt 9 needs a re-wipe to a clean debut (user's call). Prod untouched.
- **Named follow-up:** same-kind batching if attempt 9's serve log shows roots not restoring.
- **Attempt-9 latency witness:** serve-log `restored context checkpoint` lines on synthesis tasks from a holding's second topic and on root tasks from about its fourth; no expanded distillation on a holding whose first pass stays under 12,288; per-stock prompt-evaluation time below the attempt-8 band.
- **Watches carried (no change):** 40-fetch budget exhausted before follow-ups and the disconfirming pass; distillation claim drops; funds not faster than stocks; quality sub-score ≤10 on both stocks; empty issuer descriptions.

## Open questions

- Whether the §Slices grouping belongs in the findings doc or only here — carried.
- Whether `.metis/INDEX.md` gets a lookup row for the prompt examples (suggested under Local analysis suite: "Rendered prompt examples — prompts/README.md; prompts/portfolio/") — the user's call, carried.
- Whether the stubbed MARKET ANALYSIS sections in the examples should render the report's real text instead — carried.

## Where to start

Continue the read-through at `02-research-gathering-follow-up-pass.md`: load the file and the builders that render it, then wait for the user's comments; raise the file-02 carry-over only when they reach item 1.
Attempt 9 stays the user's call (re-wipe the dev store first, bring the stack up per the runbook, the user clicks Run); never propose the run.
