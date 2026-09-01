# Current session handoff

## What happened

Hardened **Fix B** (attempt-4 Finding 4) through **four Codex review rounds** —
each finding verified against code, some pushed back on, the rest fixed. Landed
in `portfolio/research.rs`: **body-required citability** (a fetch with no body is
dropped, gap-recorded, out of the claim-validator allow-set — title-only pages
included), the **extracted title threaded** into each kept source's synthesis
header (it was lost when the gathering transcript was discarded), a **title cap +
header-fit trim + `pass_brief` prefix bound** (per-claim / ledger-block /
follow-up caps + head-cap) so neither the gathering request nor the synthesis
prefix can exceed the input guard whatever the page or ledger size (cache hits
spend no fetch budget, so per-pass counts aren't bounded by the fetch ceiling),
and **degradation propagation** (failed/empty searches, failed fetches,
budget-skips, the turn cap, an exact-ceiling budget exhaustion, and a malformed
non-array `tool_calls`) into a synthesis note + data-health gap. `topic_answered`
was deliberately **not** hard-overridden (no consumer). Because the synthesis
input changed, **`portfolio::PROMPT_VERSION` moved v33 → v34**
(`job::resume_eligibility` refuses a cross-semantics resume). Codex approved.

## Current state

**Merged to `main`** — PR #71 squash-merged (`dccf293`), feature branch deleted.
Gate was green (1453 backend tests, clippy 0, `npm run build`, 46 + 254 frontend).
Record kept current at
`docs/verification/2026-08-31-big-run-attempt-4-findings.md` §Post-landing review.
BUILD.md unchanged (it cites the `PROMPT_VERSION` constant, not the value, and
delegates fix-B detail to the attempt record). The **debut stamp is now
`portfolio-v34`** (was v33). Nothing in flight. Attempt 5 remains cancelled-store
state; **attempt 5 must re-wipe to a clean debut** and confirm **v34**.

## Open questions

- **When to launch attempt 5** — user's call, from a wiped store; don't propose
  it unprompted.
- **Handle Finding 3 before attempt 5?** — the action-prompt restructure is
  diagnosed (record §Finding 3) but **unbuilt**; the remaining pre-run prompt
  candidate.
- Does Fix B + this hardening actually kill the ~70% empty-body rate on the live
  122B — only attempt 5 confirms (watch the 6d research-findings retry rate early).
- The **permanent** SearXNG engine set — a post-run call.
- **Memory drift:** `searxng-orbstack-bringup` updated to v34, but the
  `big-run-waits-on-review-record` memory + MEMORY.md index still say the debut
  stamp is `portfolio-v32` → now **v34**.

## Where to start

On the user's go, either **build Finding 3** (action-prompt restructure —
diagnosis in the attempt-4 record §Finding 3) or **launch attempt 5**: re-wipe the
dev store to a clean debut → bring up infra (`searxng-orbstack-bringup` memory) →
confirm debut stamps **`portfolio-v34`** / `checkpoint-v8` / `evidence-floor-v4` /
`grade-v2.3` → trigger → read `data-health` early and watch the 6d
research-findings retry rate to confirm Fix B.
