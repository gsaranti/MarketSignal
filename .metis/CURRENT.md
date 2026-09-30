# Current session handoff

## Active task

The prompt-by-prompt read-through of `docs/prompts/portfolio/` (26 files in `00-contents.md` order), the user reading each rendered prompt and ruling as they go; the single big confirmation run (attempt 9) waits behind it on the user's word.

## What happened

The session (2026-09-29) landed files 09 to 11, `portfolio-v60` through `portfolio-v63` (`122db00`, `ceda5cb`, `8505555`, `a327f4f`; each commit message carries its rulings).
The calls the next session builds on: a Part 1 heading is pointed at as "the X under HEADING", never as an adjective ("the question under FOLLOW-UP", not "the FOLLOW-UP question"); a topic's last pass under the depth cap asks for no follow-up proposal, as the disconfirming pass does; every object-returning system message reads role line, then "You will return <the object's keys>, as one JSON object.", then the shared frame "Part 1 of the message gives the inputs. Part 2 defines those outputs and gives the shape to return." (`TWO_PART_FRAME`); the distillation task opens in the synthesis's words, its claims items read "one statement per claim", its rules sit under CLAIM RULES and are pointed at by name.
The one structural change: distillation claim lines carry pass-local ids (`C1`, `C2`, … per message) and a returned claim cites `evidence_id`, an enum of the shown ids, in place of copying the 64-hex reference and the URL (attempt 8's W9 drops); `ClaimIndex` resolves the id app-side at every hop.
The checkpoint trail is unchanged throughout (`checkpoint-v15`).

## Current state

Nothing in flight; the working tree is clean at `a327f4f`.

- **Debut stamp set:** `portfolio-v63` / `checkpoint-v15` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`, portability 11.
- **Review method:** the user reads the file and comments; nothing is analysed or proposed unasked until the user asks for thoughts; "go ahead" authorises the builder edit, its pins, the docs mirror, the regenerated examples and the full gate, all left in the working tree; commit and push only after the user has reviewed the diff and says so; one stamp bump per commit, the next being `portfolio-v64`.
- **Carry-overs for later files:** files 18 to 26 still open their task with "in the shape at the end" (the distillation and synthesis say "in the shape under RETURN SHAPE, … the names below are its fields"); files 18 and 19 render `Falsifier:` and `Trigger:` with no gloss of the two words; the no-page app-recorded findings still append the plain-words loss note (offered to drop, not ruled).
- **Deferred by ruling:** page ids for the typed items' `source_url` (forward_assumption, forensic_event, pre_profit_observations, backfill.sources), only if attempt 9 shows drops there.
- **Store:** attempt 8's three checkpoint holding rows persist under `portfolio-v48`, `portfolio_runs` 0, `job_runs` max 8; attempt 9 needs a re-wipe to a clean debut (user's call). Prod untouched.
- **Attempt-9 witnesses:** serve-log `restored context checkpoint` lines on synthesis tasks from a holding's second topic and on root tasks from about its fourth; no expanded distillation on a holding whose first pass stays under 12,288; per-stock prompt-evaluation time below the attempt-8 band; the "unknown evidence_id" distillation drop class at zero.
- **Watches carried (no change):** 40-fetch budget exhausted before follow-ups and the disconfirming pass; funds not faster than stocks; quality sub-score ≤10 on both stocks; empty issuer descriptions.

## Open questions

None.

## Where to start

Give `11-distillation-single-pass-stock-first-analysis.md` one last sweep: load the file and the `distill.rs` builders, then wait for the user's comments; file 12 follows, then 13 to 17 (the same builders, so most rulings already reach them).
Attempt 9 stays the user's call (re-wipe the dev store first, bring the stack up per the runbook, the user clicks Run); never propose the run.
