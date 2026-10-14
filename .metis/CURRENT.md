# Current session handoff

## Active task

Trade Opportunities: the job spine, engine arm and gate — next and unplanned.

## What happened

The factor machinery's task 4, the factor-distribution store, landed (`180a411`, pushed), so the item is complete and moved to Built.
`portfolio/factor_store.rs` keeps one current observation per issuer per factor (the raw value, or floored for debt over a non-positive equity), bucketed by profile sector × market-cap band.
Observations age out after 24 calendar months, and the read waits for 20 other issuers holding the factor.
Portfolio's fresh passes contribute fail-soft; the read has no caller yet.
The store joins the archive at format v24, and import now refuses any table entry present but unlisted in the manifest whatever the archive's version (Codex's P2).
No prompt or checkpoint stamp moved.
The Metis reviewer approved with nits: N1–N4 applied, N5 carried, N6 left; every ruling is in the commit message.

## Current state

The TO spine is next and unplanned; the plan-task decides its task breakdown.
The dev-run batch (every Portfolio item's dev run) and the big run are the user's call; the next big run starts testing afresh, every earlier attempt's findings retired.
The prompt review restarts from scratch in an upcoming session; the stamp is `portfolio-v77`, the next `portfolio-v78`.

Residue, grouped by what triggers it:

**For the Trade Opportunities spine (the next item)**
1. Wire the store's read (`bucket_position`) into the dossier and audit, and contribute through `contribute`. `CapBand` also serves as discovery's stratum.
2. Draft "runway years" and the illiquid and high-event-exposure predicates.
3. Fetch each candidate's deep history once through `fetch_dated_bars` and derive its closes from the bars.
4. Verify FMP's light-EOD volume and its five annual income rows live at the spine's dev run.
5. `§Step 5h` restates the entry gate's legs beside the entry asymmetry row. It's tagged "later docs pass", but the spine will be editing that section anyway.

**After the dev-run batch (your call)**
6. Shadow-tune the drafted cutoffs, anchors and cap bands through `sweep_grade_bands`.
7. Two task-2 findings: the Real Estate anchor row averages REITs with real-estate operators, and the thin-history flag counts the growth and investment factors.
8. Rebuild the fixed-evidence set, which fixes examples 21–22's missing factor lines and the TSLA "(name unavailable)" header.

**For the restarted prompt review**
9. METRICS still renders P/E, P/S and P/B beside the factor lines.

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

`/metis-session-start`, then `/metis-plan-task` for the Trade Opportunities job spine: read its BUILD entry's docs (TO-workflow §Steps 1, 2, 5a–5c, 5h, 6, 9; trade-opportunities.md §Evidence floor, §The opportunity space, §Archetype, §Starting parameters, §Failure posture), decide its task breakdown, then put every flag and assumption through the selector.
Never propose the run.
