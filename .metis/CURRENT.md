# Current session handoff

## Active task

Trade Opportunities: the job spine, engine arm and gate — next in the backlog and unplanned; the Portfolio prompt review is the work in flight ahead of it.

## What happened

The prompt review's review files (18–20) are closed and a whole-set convention landed: slice 7 (`c713860`, review waived by the user) and slice 8 (`6592ee0`, Metis approve-with-nits taken, Codex approved over three rounds); the stamp is `portfolio-v85`.
Every Portfolio prompt now heads its blocks `# LABEL` and the groups inside a block `## LABEL`, the Part banners unchanged, stated once in `local-models.md §Prompt posture`; the five sections that sit under COMPUTED on the priced messages and at top level on the role/risk ones take a `Tier` from their caller; COMPUTED's gloss no longer names the label it runs up to.
The review's Part 1 reads today's material, then the prior's, then REALIZED.
REALIZED lost its per-position outcome group — on a weekly cadence a prior position's horizons never pass before the next run, and resolutions reach the model through CHECKS SINCE THE PRIOR ANALYSIS — relabelled ACCURACY SCORES as ACCURACY RECORD, describes each group once at the group in short sentences (the close rule stated once, "check" and "computed" defined at first use), leaves a no-price check out of the list, and its item 2 allows "cannot be told, from what is under FETCHED VALUES, ANALYSIS and REALIZED".
Rulings that bind the remaining files: glosses are short sentences or bullets, never a semicolon chain; a block's gloss never restates its groups' glosses; "the prior analysis" stays the time marker; a shared heading change re-stamps the whole set.
The docs moved with each change (Steps 6b and 6e, stage 5, §Outcome learning, TO Step 5e's mirrored clause).

## Current state

The prompt review continues at file 21, the first thesis-document prompt; files 21–31 are unread in the new format; the next stamp is `portfolio-v86`.
The TO spine stays next in the backlog and unplanned; the plan-task decides its task breakdown.
The dev-run batch (every Portfolio item's dev run) and the big run are the user's call; the next big run starts testing afresh, every earlier attempt's findings retired.

Residue, grouped by what triggers it:

**For the Trade Opportunities spine**
1. Wire the store's read (`bucket_position`) into the dossier and audit, and contribute through `contribute`. `CapBand` also serves as discovery's stratum.
2. Draft "runway years" and the illiquid and high-event-exposure predicates.
3. Fetch each candidate's deep history once through `fetch_dated_bars` and derive its closes from the bars.
4. Verify FMP's light-EOD volume and its five annual income rows live at the spine's dev run.
5. `§Step 5h` restates the entry gate's legs beside the entry asymmetry row. It's tagged "later docs pass", but the spine will be editing that section anyway.
6. The TO docs still describe the TO analysis as "consolidating what this run's research established" with the confirm / revise / untouched sort (`trade-opportunities-workflow.md §Step 5e`, `trade-opportunities.md §The pipeline`); Portfolio's Step 6d is the precedent when the TO prompts are reviewed, and TO Step 5e's REALIZED list should be read against Portfolio's slice-8 shape then.

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

`/metis-session-start`, then the prompt review on file 21: read it against the thesis-document contract in `portfolio-workflow.md §Step 6f`, `portfolio-analysis.md §The holding verdict` and `§Intrinsic verdict`, and the document's length band in `§Starting parameters`, under the lexicon rule and the slice-8 conventions (every block `#`, every group `##`, glosses in short sentences, one description per group, no app concept); the first known finding is residue 10; check the role line names its output, the opening names its input, every noun the system message uses is introduced, and the task's phrases are the analysis's and the review's own; put findings through the selector; land accepted changes, bump the stamp to `portfolio-v86` on the first, regenerate the set and read the diff; `/metis-review-task` then Codex review; commit on the user's word. Then files 22–27 one at a time.
Never propose the run.
