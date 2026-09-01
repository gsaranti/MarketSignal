# Current session handoff

## What happened

**Attempt 5 — the single big confirmation run — was launched and user-ended at 4 of 47.**
From a re-wiped clean debut store the `portfolio-v34` debut was confirmed (stamps `checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v3`); infra was brought up (Ollama + OrbStack + SearXNG with the Serper floor serving) and torn down after.
TSLA, PSX, SPMO, ARKF completed; the user stopped for a trip.
Record committed + pushed `42e47db` (`docs/verification/2026-09-01-big-run-attempt-5-findings.md`).

Reads on the 4-holding sample (not rates): **Fix B / empty-body — the C8 live question — does NOT reproduce**, with 2 retries over 4 holdings from two causes (parse, daemon-error), both recovered, 0 hard failures.
**Finding 5 (new):** the 6d synthesis prompt shows the model no schema, so it hand-builds a Markdown serialization; on PSX the confused narrative-sentiment topic dropped whole as the one unreconciled topic (its content absent from the synthesis, the diverse citation set hiding the loss).
**Finding 6 (new):** fund path clean — SPMO `priced`, ARKF `role_risk_only`.
Findings 1–3 interim clean (search serving with paywall fetch-failure friction, ledger `quant` populated, action output clean on all four).

## Current state

Working tree clean; the findings doc and the big-run tracking memory are committed/pushed.
**The dev store was NOT re-wiped** — it holds the 4 completed holdings + checkpoint header for post-trip inspection; **attempt 6 must re-wipe** per the standing ruling.
No terminal `job_runs` row was written (the run was ended by killing the dev process, not a cooperative cancel; the table stays at id 5).
C8's live side is trending toward closed but not closed — the full-book empty-body rate, the unreconciled-topic rate, and the Finding-3 oscillation all still need a completed run.
**Finding 5 has a fix candidate — show the model the schema shape (field names, types, a terse example object)** — a post-run prompt-clarity slice, same class as attempt-4 Findings 2/3; not built.

## Open questions

- **Build the Finding-5 synthesis-prompt fix before attempt 6, or re-launch first?** The fix would improve attempt 6's research quality; re-launching as-is banks the full-book rates on the current build. User's call.
- **When to launch attempt 6** — user's call, from a re-wiped store; don't propose it unprompted.
- Does the empty-body rate stay low at book scale, and does topic-drop cluster on the confusion-prone topics (narrative-sentiment, disconfirming) and correlate with the parse retries? Finding 5's across-book reads — only a completed run answers.
- **Finding 3 oscillation** — still unmeasured; needs the reasoning stream across the book.
- The **permanent** SearXNG engine set — a post-run config call once a full run shows which engines serve under volume.

## Where to start

The next move is the user's call: either build the **Finding-5 fix** (show the synthesis schema — a post-run prompt-clarity slice) or **re-launch attempt 6** from a freshly re-wiped store (bring up infra per the OrbStack bring-up notes, confirm the `portfolio-v34` debut, read `data-health` early).
Don't propose the run unprompted.
