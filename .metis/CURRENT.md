# Current session handoff

## Active task

Trade Opportunities: the job spine, engine arm and gate — next in the backlog and unplanned; the Portfolio prompt review is the work in flight ahead of it.

## What happened

The prompt review's distillation files (12–14) are reviewed and closed: slice 5 landed (`d4f1f8d`, Metis approve-with-nits applied, Codex P3 taken, Codex approved); the stamp is `portfolio-v82`.
The distillation's role line names its output ("into one shorter document"), and the task opens with the purpose as the verb and the input named: "Consolidate the write-ups under WRITE-UPS into one shorter document", the keep list following "It states what they establish in fewer words".
The single shape opens "Condense the content under WRITE-UP": a sentence never stacks one root word ("Rewrite the write-up under WRITE-UP" was refused for three "write"s), and a sentence with two pronouns for two nouns names the nouns instead.
The lexicon of a downstream task follows the upstream prompt's phrases ("what they leave unanswered", "every dated figure they quote" — the synthesis task's own words).
The WRITE-UPS block's gloss is one constant shared by the distillation's merged shape and the analysis, so files 15–17 did not move.

## Current state

The prompt review continues at file 15, the first analysis prompt; files 15–17 are unread end to end; the next stamp is `portfolio-v83`.
The TO spine stays next in the backlog and unplanned; the plan-task decides its task breakdown.
The dev-run batch (every Portfolio item's dev run) and the big run are the user's call; the next big run starts testing afresh, every earlier attempt's findings retired.

Residue, grouped by what triggers it:

**For the Trade Opportunities spine**
1. Wire the store's read (`bucket_position`) into the dossier and audit, and contribute through `contribute`. `CapBand` also serves as discovery's stratum.
2. Draft "runway years" and the illiquid and high-event-exposure predicates.
3. Fetch each candidate's deep history once through `fetch_dated_bars` and derive its closes from the bars.
4. Verify FMP's light-EOD volume and its five annual income rows live at the spine's dev run.
5. `§Step 5h` restates the entry gate's legs beside the entry asymmetry row. It's tagged "later docs pass", but the spine will be editing that section anyway.

**After the dev-run batch (your call)**
6. Shadow-tune the drafted cutoffs, anchors and cap bands through `sweep_grade_bands`.
7. Two task-2 findings: the Real Estate anchor row averages REITs with real-estate operators, and the thin-history flag counts the growth and investment factors.
8. Rebuild the fixed-evidence set so the live harness and the offline pins carry factors, a fund context, statements, a consensus and distributions; the examples no longer wait on it.

**For the prompt review**
9. METRICS still renders P/E, P/S and P/B beside the factor lines that restate them, and the VALUATION ratio line restates P/E and P/B a third time (files 21–22).

**Needs a live run**
10. A denied client-secret read still errors `schwab_status`, so Settings hides its Schwab section; a live denied prompt has only been checked against fixtures.
11. The Step-3 latency of the calendar join, the history reads and the froth group is untimed.
12. The baseline history's prompt size is unmeasured.
13. The report's research-branch thresholds (the 0.5 / 2.5 clamps and the 7-day anchor) await calibration against real snapshots.
14. The reuse block's one-third cap: whether a late topic's root pass loses most of the inventory shows in the persisted "previously retrieved page(s) omitted" gaps; the cap is the lever, not the rule.

**Unruled observations (each may become work; none is decided)**
15. A failed analyst waits out a sibling's retry before the run fails.
16. The `sectors` snapshot carries FMP's default board, unlabelled.
17. FMP's rate-decision estimate and actual go unused while Fed events stay date-only.
18. The deal trend is mostly noise at a weekly cadence.
19. The retry's send timeout is tested at unit level only.
20. A write-up may name FETCHED VALUES as a figure's source while the distillation prompt carries no such block; the task keeps the source as given and the analysis that reads the distillate carries the block.

Standing reminders (not work): every stamp bump touches the pins in `prompt_version_is_stamped_for_the_model_arm_domain_gate`, and the offscreen card-render recipe lives in session memory.

## Open questions

None.

## Where to start

`/metis-session-start`, then the prompt review on file 15: read it against `local-models.md §Prompt posture`, the analysis call's contract in `portfolio-workflow.md §Step 6d`, the consolidation paragraph of `web-research.md §The research loop` and the analysis's length band in `portfolio-analysis.md §Starting parameters`, under the lexicon rule; check the opening sentence names its input, every noun the system message uses is introduced, and the task's phrases are the write-ups' own; put findings through the selector; land accepted changes, bump the stamp to `portfolio-v83` on the first, regenerate the set and read the diff; `/metis-review-task` then Codex review each slice; commit on the user's word. Then files 16 (the continuity run: `§Step 6b`'s split-context line and the PRIOR ANALYSIS clause) and 17 one at a time.
Never propose the run.
