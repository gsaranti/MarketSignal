# Current session handoff

## Active task

Trade Opportunities: the job spine, engine arm and gate — next in the backlog and unplanned; the Portfolio prompt review is the work in flight ahead of it.

## What happened

The prompt review's first slice landed (`08821a5`, pushed; Codex approved after three rounds).
File 01's FETCHED VALUES block renders under group labels in the docs' order, one figure or record per line, the quarterly statements as one dated series per metric in one unit at the decimals its smallest cell needs; the gloss lost every provenance word; the gathering Part 2 gained "Use FETCHED VALUES as background to shape how the questions are researched", the system message ends "write no findings in this conversation", and the search tool's description names all six tier rungs.
The examples carry synthetic statements, a consensus and SPMO's distributions, reconciled with every other synthetic row.
An outside review of file 01 contributed three of these points and had five declined as ruled design (topic-specific packets, a RULES system message, structured findings, completeness criteria, topic decomposition).
Ruled: a gloss states what the model needs to read the figures, never where they came from or what the app did.

## Current state

The prompt review continues on file 01; the stamp is `portfolio-v78`, the next `portfolio-v79`.
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
10. `§Step 6c` lists no dividends row for a fund, but the fund fetch pulls dividends and the renderer shows them; the docs or the code must move.
11. The role of PRIOR ANALYSIS and PRIOR THESIS in the gathering Part 2 is unstated, and a test keeps Part 2 from naming them; decide at file 03.

**Needs a live run**
12. A denied client-secret read still errors `schwab_status`, so Settings hides its Schwab section; a live denied prompt has only been checked against fixtures.
13. The Step-3 latency of the calendar join, the history reads and the froth group is untimed.
14. The baseline history's prompt size is unmeasured.
15. The report's research-branch thresholds (the 0.5 / 2.5 clamps and the 7-day anchor) await calibration against real snapshots.

**Unruled observations (each may become work; none is decided)**
16. A failed analyst waits out a sibling's retry before the run fails.
17. The `sectors` snapshot carries FMP's default board, unlabelled.
18. FMP's rate-decision estimate and actual go unused while Fed events stay date-only.
19. The deal trend is mostly noise at a weekly cadence.
20. The retry's send timeout is tested at unit level only.

Standing reminders (not work): every stamp bump touches the pins in `prompt_version_is_stamped_for_the_model_arm_domain_gate`, and the offscreen card-render recipe lives in session memory.

## Open questions

None.

## Where to start

`/metis-session-start`, then the prompt review on file 01: put residue 10 through the selector, then the remaining Part 2 items against `local-models.md §Prompt posture` and the call's contract in `portfolio-workflow.md`, then file 02, one file at a time; land accepted changes in the code, bump the stamp to `portfolio-v79` on the first, regenerate the set and read the diff; Codex reviews each slice.
Never propose the run.
