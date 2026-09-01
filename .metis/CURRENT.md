# Current session handoff

## What happened

A post-closure review of the finite Fix-B matrix found C5 was incomplete: the per-page 12,000-character fetch-cap truncation persisted no research gap — only budget-forced truncations did — so a holding truncated solely by the fetch cap read clean in Data Health.
The fix records the truncation once at the fetch site (from the original extracted length, cache hits included) through the existing `PassDegradation` → gap → `build_data_health` path into the typed research counts, completing C5 for every evidence-truncation event.
A user ruling then made the synthesis author's gathering-degradation note purely factual for every degradation type — "GATHERING WAS PARTIAL: <losses> — treat coverage as incomplete" — dropping "temper conviction" and "do not mark the topic fully answered" from both the model note and the persisted gap.
That ruling was generalized into a recorded design principle — **model prompts inform, never prescribe the conclusion** — canonical at `docs/local-models.md §Prompt posture`, an invariant in `BUILD.md`.
This session then built Finding 3: the per-holding action-call prompt was restructured to data framing plus ordered factual gates, and the full Portfolio prompt surface (interpretation, role/risk, action, research + their evidence sections) was swept to the same bar — architecture narration and how-to-weigh nudges cut, load-bearing contracts kept, the F6 target-provenance weighing trimmed to facts by user ruling (attempt-4 record §Finding 3 fix and §Full portfolio-prompt posture sweep).
A later Codex and Claude Code reconciliation identified a small residual slice in that sweep: scoreboard rationale in the action prompt, cross-stage authorship narration in the pre-profit helper, and decoder plus meta-reasoning narration in the three structured response contracts.
The follow-up removes those residues while retaining the engine-rule facts, two-arm freedom, ordered action gates, exact output keys, and priced nested shapes; it also clarifies the canonical governed-versus-ungoverned prompt-posture boundary without claiming a new repository-wide traceability audit.

## Current state

The C5 fix and de-prescribe are committed on `main` (`eea56b2`); the attempt-4 record's §Post-closure C5 completion subsection, this handoff, and the prompt-posture principle (BUILD §invariants, canonical at `docs/local-models.md §Prompt posture`) are the documentation-only follow-up committed with it.
The finite matrix now stands at C1-C7 closed (C5 complete), C8 static-closed / live-open.
The debut stamp stays **`portfolio-v34`** — v34 has never run, so every pre-debut change folds into the debut stamp.
Gates green: **1,463** Rust tests with 31 live smokes ignored, warning-free clippy, frontend untouched.
No live run was launched.

The Finding 3 fix and full-surface prompt sweep are committed on `main` at `53f007f`.
The narrow prompt-posture follow-up is in the working tree, **uncommitted**; no stamp moved because both local stores have zero persisted Portfolio runs, the sole development checkpoint is `portfolio-v32`, and no data-directory override is configured.
Its gates are green: **1,432** library tests passed with 31 live smokes ignored plus all integration suites, clippy was warning-free, `npm run build` passed, and `git diff --check` passed.

## Open questions

- **When to launch attempt 5** — user's call, from a wiped store; don't propose it unprompted.
- Does Fix B plus the finite closure actually kill the roughly 70% empty-body rate on the live 122B?
  Only Attempt 5 confirms that; watch the 6d research-findings retry rate, the typed Data Health research counts (now including fetch-cap truncations), and gathering-bound gaps early.
- The **permanent** SearXNG engine set — a post-run call.

## Where to start

First review and commit the uncommitted narrow prompt-posture follow-up.
Then, on the user's go, launch Attempt 5 from a newly wiped dev store: bring up the required infrastructure, confirm debut stamps `portfolio-v34` / `checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3`, and inspect the 6d findings retry rate plus typed Data Health research counts early — plus, per Finding 3, whether the action trace still oscillates across rungs before settling.
