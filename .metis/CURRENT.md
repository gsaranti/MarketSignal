# Current session handoff

## Active task

Trade Opportunities: the job spine, engine arm and gate — next in the backlog and unplanned; the Portfolio prompt review is the work in flight ahead of it.

## What happened

The prompt review's analysis files (15–17) are reviewed and closed: slice 6 landed (`a1ee4c4`, Metis approve-with-nits applied, Codex approved); the stamp is `portfolio-v83`.
The analysis is no longer a distillation task: the role line names its output ("writing the analysis of one holding's research"), the task opens on "the research under WRITE-UPS" (true over a distillate too) and asks what that research establishes "when its topics are read together".
PRIOR ANALYSIS is named as "background and the foundation the new analysis builds on" with no carry rule: "keep what it says where the research does not revisit it" was refused as binding the model to material the write-ups merely omit, and the docs now say what carries forward is the model's judgment — the review's first docs change (Step 6d, stage 4, the research-memory paragraph).
The WRITE-UPS gloss names the contrary-evidence write-up last only when one closes the list, since the disconfirming pass may write none.
The split-context line above a prior document speaks in the model's terms, the unverifiable branch naming the last close before the session (the anchor bar).

## Current state

The prompt review continues at file 18, the first review prompt; files 18–20 are unread end to end; the next stamp is `portfolio-v84`.
The TO spine stays next in the backlog and unplanned; the plan-task decides its task breakdown.
The dev-run batch (every Portfolio item's dev run) and the big run are the user's call; the next big run starts testing afresh, every earlier attempt's findings retired.

Residue, grouped by what triggers it:

**For the Trade Opportunities spine**
1. Wire the store's read (`bucket_position`) into the dossier and audit, and contribute through `contribute`. `CapBand` also serves as discovery's stratum.
2. Draft "runway years" and the illiquid and high-event-exposure predicates.
3. Fetch each candidate's deep history once through `fetch_dated_bars` and derive its closes from the bars.
4. Verify FMP's light-EOD volume and its five annual income rows live at the spine's dev run.
5. `§Step 5h` restates the entry gate's legs beside the entry asymmetry row. It's tagged "later docs pass", but the spine will be editing that section anyway.
6. The TO docs still describe the TO analysis as "consolidating what this run's research established" with the confirm / revise / untouched sort (`trade-opportunities-workflow.md §Step 5e`, `trade-opportunities.md §The pipeline`); Portfolio's Step 6d is the precedent when the TO prompts are reviewed.

**After the dev-run batch (your call)**
7. Shadow-tune the drafted cutoffs, anchors and cap bands through `sweep_grade_bands`.
8. Two task-2 findings: the Real Estate anchor row averages REITs with real-estate operators, and the thin-history flag counts the growth and investment factors.
9. Rebuild the fixed-evidence set so the live harness and the offline pins carry factors, a fund context, statements, a consensus and distributions; the examples no longer wait on it.

**For the prompt review**
10. METRICS still renders P/E, P/S and P/B beside the factor lines that restate them, and the VALUATION ratio line restates P/E and P/B a third time (files 21–22).

**Needs a live run**
11. A denied client-secret read still errors `schwab_status`, so Settings hides its Schwab section; a live denied prompt has only been checked against fixtures.
12. The Step-3 latency of the calendar join, the history reads and the froth group is untimed.
13. The baseline history's prompt size is unmeasured.
14. The report's research-branch thresholds (the 0.5 / 2.5 clamps and the 7-day anchor) await calibration against real snapshots.
15. The reuse block's one-third cap: whether a late topic's root pass loses most of the inventory shows in the persisted "previously retrieved page(s) omitted" gaps; the cap is the lever, not the rule.

**Unruled observations (each may become work; none is decided)**
16. A failed analyst waits out a sibling's retry before the run fails.
17. The `sectors` snapshot carries FMP's default board, unlabelled.
18. FMP's rate-decision estimate and actual go unused while Fed events stay date-only.
19. The deal trend is mostly noise at a weekly cadence.
20. The retry's send timeout is tested at unit level only.
21. A write-up may name FETCHED VALUES as a figure's source while the distillation prompt carries no such block; the task keeps the source as given and the analysis that reads the distillate carries the block.

Standing reminders (not work): every stamp bump touches the pins in `prompt_version_is_stamped_for_the_model_arm_domain_gate`, and the offscreen card-render recipe lives in session memory.

## Open questions

None.

## Where to start

`/metis-session-start`, then the prompt review on file 18: read it against `local-models.md §Prompt posture`, the review call's contract in `portfolio-workflow.md §Step 6e`, the accuracy record in `portfolio-analysis.md §Outcome learning` and the review's length band in `§Starting parameters`, under the lexicon rule; the first known finding is the REALIZED block's "re-based the price series" and "the close its prices were anchored to is missing from the fetched series" in `review.rs`, the split-context line's wording its precedent; check the opening sentence names its input, every noun the system message uses is introduced, and the task's phrases are the analysis's and the thesis document's own; put findings through the selector; land accepted changes, bump the stamp to `portfolio-v84` on the first, regenerate the set and read the diff; `/metis-review-task` then Codex review each slice; commit on the user's word. Then files 19 (the fund review) and 20 (the role/risk review) one at a time.
Never propose the run.
