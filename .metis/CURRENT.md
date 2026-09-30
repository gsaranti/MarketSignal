# Current session handoff

## Active task

The prompt-by-prompt read-through of `docs/prompts/portfolio/` (26 files in `00-contents.md` order), the user reading each rendered prompt and ruling as they go; the single big confirmation run (attempt 9) waits behind it on the user's word.

## What happened

The session (2026-09-30) closed file 11 with its last sweep, `portfolio-v64` (`abd749f`; the commit message carries the rulings).
The calls the next session builds on: the TOPICS gloss names each topic's heading as its key and its title, and item 2 points at it ("topic_key is the topic's key under TOPICS"); a task item carrying several rules is one sentence per rule, each in the "<field> is …" form ("evidence_id is the id of the claim under TOPICS or CONTRARY EVIDENCE the statement rests on."); a gloss that lists a line's fields never hangs them off "each with a, b, and c" after another clause but ends the sentence and opens the list in the synthesis EVIDENCE gloss's shape ("Each claim carries: a; b; and c."), now on TOPICS, CLAIMS SO FAR and PRIOR FINDINGS alike; lists of three or more take the serial comma; and the shared placeholder renderer shows a nullable scalar's both halves ("<0|null>"), which today reaches only stated_low and stated_high.
The "period" gloss kept its words (the file-03 ruling); the example fixture's claims now carry the dates and periods a run renders instead of `unknown` throughout.
The checkpoint trail is unchanged (`checkpoint-v15`).

## Current state

Nothing in flight; the working tree is clean at `abd749f`.

- **Debut stamp set:** `portfolio-v64` / `checkpoint-v15` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`, portability 11.
- **Review method:** the user reads the file and comments; nothing is analysed or proposed unasked until the user asks for thoughts; "go ahead" authorises the builder edit, its pins, the docs mirror, the regenerated examples and the full gate, all left in the working tree; commit and push only after the user has reviewed the diff and says so; one stamp bump per commit, the next being `portfolio-v65`.
- **Carry-overs for later files:** files 18 to 26 still open their task with "in the shape at the end" (the distillation and synthesis say "in the shape under RETURN SHAPE, … the names below are its fields"); files 18 and 19 render `Falsifier:` and `Trigger:` with no gloss of the two words; the no-page app-recorded findings still append the plain-words loss note (offered to drop, not ruled); the two-item NEWS LEADS gloss keeps "each with its source and date" (nothing nests, left as is).
- **Deferred by ruling:** page ids for the typed items' `source_url` (forward_assumption, forensic_event, pre_profit_observations, backfill.sources), only if attempt 9 shows drops there.
- **Store:** attempt 8's three checkpoint holding rows persist under `portfolio-v48`, `portfolio_runs` 0, `job_runs` max 8; attempt 9 needs a re-wipe to a clean debut (user's call). Prod untouched.
- **Attempt-9 witnesses:** serve-log `restored context checkpoint` lines on synthesis tasks from a holding's second topic and on root tasks from about its fourth; no expanded distillation on a holding whose first pass stays under 12,288; per-stock prompt-evaluation time below the attempt-8 band; the "unknown evidence_id" distillation drop class at zero.
- **Watches carried (no change):** 40-fetch budget exhausted before follow-ups and the disconfirming pass; funds not faster than stocks; quality sub-score ≤10 on both stocks; empty issuer descriptions.

## Open questions

None.

## Where to start

Load `12-distillation-single-pass-stock-continuity.md` and the `distill.rs` builders (`reduce_task`, `topics_gloss`, the samples module), then wait for the user's comments; files 13 to 17 share the builders, so most rulings already reach them.
Attempt 9 stays the user's call (re-wipe the dev store first, bring the stack up per the runbook, the user clicks Run); never propose the run.
