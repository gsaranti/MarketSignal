# Current session handoff

## What happened

A post-closure review of the finite Fix-B matrix found C5 was incomplete: the per-page 12,000-character fetch-cap truncation persisted no research gap — only budget-forced truncations did — so a holding truncated solely by the fetch cap read clean in Data Health.
The fix records the truncation once at the fetch site (from the original extracted length, cache hits included) through the existing `PassDegradation` → gap → `build_data_health` path into the typed research counts, completing C5 for every evidence-truncation event.
A user ruling then made the synthesis author's gathering-degradation note purely factual for every degradation type — "GATHERING WAS PARTIAL: <losses> — treat coverage as incomplete" — dropping "temper conviction" and "do not mark the topic fully answered" from both the model note and the persisted gap.
That ruling was generalized into a recorded design principle — **model prompts inform, never prescribe the conclusion** — canonical at `docs/local-models.md §Prompt posture`, an invariant in `BUILD.md`, with the overloaded per-holding action prompt (Finding 3) named as the known unbuilt violation.

## Current state

The C5 fix and de-prescribe are committed on `main` (`eea56b2`); the attempt-4 record's §Post-closure C5 completion subsection, this handoff, and the prompt-posture principle (BUILD §invariants, canonical at `docs/local-models.md §Prompt posture`) are the documentation-only follow-up committed with it.
The finite matrix now stands at C1-C7 closed (C5 complete), C8 static-closed / live-open.
The debut stamp stays **`portfolio-v34`** — v34 has never run, so every pre-debut change folds into the debut stamp.
Gates green: **1,463** Rust tests with 31 live smokes ignored, warning-free clippy, frontend untouched.
No live run was launched.

## Open questions

- **When to launch attempt 5** — user's call, from a wiped store; don't propose it unprompted.
- **Handle Finding 3 before attempt 5?** — the action-prompt restructure is diagnosed (record §Finding 3) but **unbuilt**; the remaining pre-run prompt candidate, and the known violation of the new prompt-posture invariant (it still prescribes how to weigh).
- Does Fix B plus the finite closure actually kill the roughly 70% empty-body rate on the live 122B?
  Only Attempt 5 confirms that; watch the 6d research-findings retry rate, the typed Data Health research counts (now including fetch-cap truncations), and gathering-bound gaps early.
- The **permanent** SearXNG engine set — a post-run call.

## Where to start

On the user's go, either build Finding 3 from the attempt record (which also discharges the prompt-posture violation) or launch Attempt 5 from a newly wiped dev store.
For Attempt 5, bring up the required infrastructure, confirm debut stamps `portfolio-v34` / `checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3`, and inspect the 6d findings retry rate plus typed Data Health research counts early.
