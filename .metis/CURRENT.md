# Current session handoff

## What happened

Codex replaced the repeated open-ended Fix-B reviews with the finite C1-C8 closure matrix in `docs/verification/2026-08-31-big-run-attempt-4-findings.md` and completed its static side.
The exercise starts at Attempt 4's production failure rather than the latest diff: the evidence establishes a failure under the joint condition of growing tool history, the terminal protocol switch, `format`, and contemporaneous serving state, but the clean short pre-flight does not isolate one ingredient or prove a universal Ollama incompatibility.
The resulting fixes require the findings wire's three grammar-required keys and nonblank prose/claim fields, make source admission fail closed for zero rendered body, roll persisted research gaps into required typed Data Health counts and the visible Portfolio summary, and share one serialized message/tool projection between gathering bounds and prompt telemetry.
Active documentation and code comments now describe grammar as a generation constraint followed by application parsing and validation.

## Current state

The finite static closure is complete: C1-C7 are closed, and C8's static gates are closed while its live operational confirmation remains explicitly open.
The working tree contains the uncommitted closure exercise on top of `main`; no commit or push was requested.
Independent gates are green: **1,462** Rust tests with 31 live smokes ignored, warning-free clippy, `npm run build`, 46 pure-module tests, 255 component tests, and diff hygiene.
The debut stamp remains **`portfolio-v34`** because these corrections harden validation, telemetry, and app-written Data Health under the wiped-store pre-release posture without changing valid model-facing prompt semantics.
No live run was launched.

## Open questions

- **When to launch attempt 5** — user's call, from a wiped store; don't propose it unprompted.
- **Handle Finding 3 before attempt 5?** — the action-prompt restructure is
  diagnosed (record §Finding 3) but **unbuilt**; the remaining pre-run prompt
  candidate.
- Does Fix B plus the finite closure actually kill the roughly 70% empty-body rate on the live 122B?
  Only Attempt 5 confirms that operational prediction; watch the 6d research-findings retry rate, typed Data Health research counts, and gathering-bound gaps early.
- The **permanent** SearXNG engine set — a post-run call.

## Where to start

On the user's go, either build Finding 3 from the attempt record or launch Attempt 5 from a newly wiped dev store.
For Attempt 5, bring up the required infrastructure, confirm debut stamps `portfolio-v34` / `checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3`, and inspect the 6d findings retry rate plus typed Data Health research counts early.
