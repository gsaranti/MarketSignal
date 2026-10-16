# Current session handoff

## Active task

Trade Opportunities: the job spine, engine arm and gate — next in the backlog and unplanned; the Portfolio prompt review is the work in flight ahead of it.

## What happened

The prompt review's gathering files (01–07) are reviewed and closed: slices 2 and 3 landed (`30128a8`, `895e51e`, both Metis- and Codex-approved) with two docs commits beside them (`e89fe7d`, `e2df51e`); the stamp is `portfolio-v80`.
File 02 renders the follow-up pass as a run issues it; FETCHED VALUES lost the FMP rating row (and its fetch leg — ruled: a vendor composite of ratios already shown is a restatement, never re-propose it for Portfolio, the report or TO) and the quote row, and renders every group of the holding's branch with a `(gap)` row where nothing was served; share counts print in B/M and the expense ratio as its published percent everywhere.
A follow-up pass reuses its own topic's pages alone (a page belongs to every topic whose pass fetched or re-read it; root passes stay holding-wide).
On a continuity run Part 2 states the prior documents' role with the falsifier clause; the follow-up opening names WRITE-UP SO FAR; the holding brief carries the prior documents as two fields; the synthesis conversation carries none by design.
Ruled as a sweep: no "run" or "pass" as an app term in any model-facing text ("this conversation", "the research on this holding", "the contrary-evidence write-up", "beside today's"); scanning the regenerated examples alone misses fail-soft strings — grep the prompt builders' string literals.
Step 6c's stock and fund FETCHED VALUES sentences now list the rendered groups in order (a fund has no profile lookup; the quote's market cap and shares outstanding are both branches').

## Current state

The prompt review continues at file 08, the first synthesis prompt; files 08–11 are unread end to end; the next stamp is `portfolio-v81`.
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

Standing reminders (not work): every stamp bump touches the pins in `prompt_version_is_stamped_for_the_model_arm_domain_gate`, and the offscreen card-render recipe lives in session memory.

## Open questions

None.

## Where to start

`/metis-session-start`, then the prompt review on file 08: read it against `local-models.md §Prompt posture`, the synthesis contract in `portfolio-workflow.md §Step 6c` and `web-research.md §The research loop`, under the lexicon rule; put findings through the selector; land accepted changes, bump the stamp to `portfolio-v81` on the first, regenerate the set and read the diff; `/metis-review-task` then Codex review each slice; commit on the user's word. Then files 09–11 one at a time.
Never propose the run.
