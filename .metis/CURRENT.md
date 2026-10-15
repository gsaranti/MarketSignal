# Current session handoff

## Active task

Trade Opportunities: the job spine, engine arm and gate — next in the backlog and unplanned; the Portfolio prompt review is the work in flight ahead of it.

## What happened

The prompt review restarted by checking the rendered examples under `docs/prompts/portfolio`.
Regenerated at `portfolio-v77` they were byte-identical to the committed set, but three shapes understated a live run: the attempt-6 fixtures predate the factor machinery, so the stock SCORES block carried no factor lines (files 21, 22, 25); the SPMO fixture carries no fund context, so the fund shapes read "(name unavailable)" with no FUND block (19, 23, 24); and the action packet's tax-exempt variant showed a spurious name change (28).
All three were fixed by synthetic layering in the examples writer (`c4b3db8`, pushed; Codex approved, its P3 taken): a factor composite on TSLA scored through the live bands, the four sub-scores its means on the engine output and the verdict alike so the review's then-and-now line reads flat; a fund context on SPMO; the variant built from the debut dossier.
`Factor::band` became crate-visible so the harness reads the live bands rather than copies.
Both calls were put to the user and ruled to stand: the prior's scores stay the synthetic means, and the band lookup stays crate-visible rather than a test-only accessor or copied bands.
The fixture JSON is unchanged, so the live harness and the offline pins still run without factors.

## Current state

The prompt review is in flight and starts at file 01 next session, file by file over many sessions; the stamp is `portfolio-v77`, the next `portfolio-v78`.
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
8. Rebuild the fixed-evidence set so the live harness and the offline pins carry factors and a fund context; the examples no longer wait on it.

**For the prompt review**
9. METRICS still renders P/E, P/S and P/B beside the factor lines that restate them (files 21–22, now visible in the examples).

**Needs a live run**
10. A denied client-secret read still errors `schwab_status`, so Settings hides its Schwab section; a live denied prompt has only been checked against fixtures.
11. The Step-3 latency of the calendar join, the history reads and the froth group is untimed.
12. The baseline history's prompt size is unmeasured.
13. The report's research-branch thresholds (the 0.5 / 2.5 clamps and the 7-day anchor) await calibration against real snapshots.

**Unruled observations (each may become work; none is decided)**
14. A failed analyst waits out a sibling's retry before the run fails.
15. The `sectors` snapshot carries FMP's default board, unlabelled.
16. FMP's rate-decision estimate and actual go unused while Fed events stay date-only.
17. The deal trend is mostly noise at a weekly cadence.
18. The retry's send timeout is tested at unit level only.

Standing reminders (not work): every stamp bump touches the pins in `prompt_version_is_stamped_for_the_model_arm_domain_gate`, and the offscreen card-render recipe lives in session memory.

## Open questions

None.

## Where to start

`/metis-session-start`, then the prompt review from `docs/prompts/portfolio/01-research-gathering-root-pass-first-analysis.md`, one file at a time: read the rendered request against `local-models.md §Prompt posture` and the call's contract in `portfolio-workflow.md`, put each finding through the selector, land accepted changes in the code, bump the stamp to `portfolio-v78` on the first, regenerate the set and read the diff; Codex reviews each slice.
Never propose the run.
