# Current session handoff

## What happened

Codex ran a **fifth full sweep of Fix B** and found + fixed a P1 and two P2s,
all folding into the **never-run `portfolio-v34` debut (no new stamp)**: the
**gathering conversation is now aggregate-bounded** (≤8 tool calls accepted per
turn — `MAX_TOOL_CALLS_PER_TURN`; the whole serialized message history + tool
schema checked against the shared input guard before every model call and every
retained result, a bound ending gathering as recorded degradation then still
synthesizing; untrusted search/fetch metadata capped) — closing the last
"relies on daemon-side truncation" hole Fix B's synthesis-only bounding had
left; the **synthesis allocator now jointly selects headers + bodies**,
reclaiming omitted-header space so a cache-hit burst can't collapse to
all-header/no-evidence (the no-drop invariant proven). Reviewed it all, verified
the gates independently, and flagged one **defense-in-depth gap** — the render
loop trusted a debug-only assert, so a future allocator regression could admit a
body-less URL in release. Codex fixed that too: `admit_planned_source` fails
closed in release + a persisted data-health gap + a direct regression test.

## Current state

All committed + pushed to `main` (**`42a974a`** "Harden portfolio research Fix
B"), working tree clean, HEAD == origin/main. Gates green: **1458** backend
tests, clippy 0, `npm run build`, 46 + 254 frontend. Record current at
`docs/verification/2026-08-31-big-run-attempt-4-findings.md` §Final full-sweep
corrections. BUILD.md unchanged (it cites the `PROMPT_VERSION` constant, not the
value). **Debut stamp is `portfolio-v34`.** Nothing in flight. Attempt 5 remains
cancelled-store state; **attempt 5 must re-wipe to a clean debut** and confirm
v34.

## Open questions

- **When to launch attempt 5** — user's call, from a wiped store; don't propose
  it unprompted.
- **Handle Finding 3 before attempt 5?** — the action-prompt restructure is
  diagnosed (record §Finding 3) but **unbuilt**; the remaining pre-run prompt
  candidate.
- Does Fix B + this hardening actually kill the ~70% empty-body rate on the live
  122B — only attempt 5 confirms (watch the 6d research-findings retry rate and
  the new gathering-input-bound / per-turn-cap partial-coverage gaps early).
- The **permanent** SearXNG engine set — a post-run call.
- **Memory drift (still unaddressed):** `big-run-waits-on-review-record` +
  MEMORY.md index still say the debut stamp is `portfolio-v32` → now **v34**
  (`searxng-orbstack-bringup` already updated).

## Where to start

On the user's go, either **build Finding 3** (diagnosis in the attempt-4 record
§Finding 3) or **launch attempt 5**: re-wipe the dev store to a clean debut →
bring up infra (`searxng-orbstack-bringup` memory) → confirm debut stamps
**`portfolio-v34`** / `checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3` →
trigger → read `data-health` early, watching the 6d research-findings retry rate
and the new gathering-bound gaps to confirm Fix B.
