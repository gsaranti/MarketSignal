# Current session handoff

## Active task

The single big confirmation run — attempt 8's findings are all landed; attempt 9 waits on the user's word.

## What happened

The session (2026-09-27) built the rendered prompt-examples set: `docs/prompts/portfolio/` holds one Markdown file per Portfolio Analysis local-model call shape (26 files plus a contents table), with `docs/prompts/README.md` as the reader's guide and one corpus-map row in `docs/README.md`.
Eight selector rulings, all on the recommended option: generated from the code rather than hand-written or snapshotted; every material shape (debut and continuity, stock, priced fund and role/risk, each research and distillation pass kind); the fixed-evidence holdings (TSLA, SPMO, the synthetic BND); shape-preserving `[stub: …]` placeholders for every parsed article or research field; the request envelope plus the verbatim messages, tools and schema per file; Markdown; the per-job subdirectory with a README and a corpus-map row; no `.metis/INDEX.md` edit.
Landed as `69fe02b` (the generator `fixed_evidence/prompt_examples.rs`, a child of the harness; a stub mode plus each call's stage label and protocol half on the research and distillation sample modules; the request builders opened `pub(super)`; a render test on every gate and an ignored writer) and `4b4cc1f` (fenced message text wrapped at 100 columns, only line breaks added, pinned by the render test).
Two things the files say and the next session should know: the latest report's sections under MARKET ANALYSIS are stubbed too, an extension beyond the stub ruling, reversible in the generator; and the continuity shapes carry a hand-written prior (attempt 6's persisted verdict re-dated to 2026-09-02 with a prior spot 3% under today's, or a stub first run of 2026-09-03 on the role/risk fund), so their retrospective figures are illustrative.
No prompt changed; the stamp set is unchanged.

## Current state

Nothing in flight; the working tree is clean at `4b4cc1f`.

- **Debut stamp set:** `portfolio-v49` / `checkpoint-v15` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`, portability 11.
- **Store:** attempt 8's three checkpoint holding rows persist under `portfolio-v48`, `portfolio_runs` 0, `job_runs` max 8; attempt 9 needs a re-wipe to a clean debut (user's call). Prod untouched.
- **Prompt examples:** a prompt-stamp bump now also regenerates `docs/prompts/portfolio/` (the command is in `docs/prompts/README.md`), and the regenerated diff is the review surface for the prompt change.
- **Named follow-up:** same-kind batching (every root gathering before any root synthesis) if attempt 9's serve log shows roots not restoring.
- **Attempt-9 latency witness:** serve-log `restored context checkpoint` lines on synthesis tasks from a holding's second topic and on root tasks from about its fourth; no expanded distillation on a holding whose first pass stays under 12,288; per-stock prompt-evaluation time below the attempt-8 band.
- **Watches carried (no change):** 40-fetch budget exhausted before follow-ups and the disconfirming pass; distillation claim drops; funds not faster than stocks; quality sub-score ≤10 on both stocks (read across a fuller book before any `grade-v2.3` change); empty issuer descriptions.

## Open questions

- Whether the §Slices grouping belongs in the findings doc or only here — carried.
- Whether `.metis/INDEX.md` gets a lookup row for the prompt examples (suggested, under Local analysis suite: "Rendered prompt examples — prompts/README.md; prompts/portfolio/") — the user's call, left out this session.
- Whether the stubbed MARKET ANALYSIS sections in the examples should render the report's real text instead — an extension beyond the stub ruling, one line in the generator either way.

## Where to start

Attempt 9 is the user's call: re-wipe the dev store to a clean debut first, bring the stack up per the runbook, then the user clicks Run; never propose the run.
No slice is queued; a new task starts with `/metis-plan-task`.
A slice that moves a prompt stamp regenerates the examples set before its review.
