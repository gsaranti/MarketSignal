# Current session handoff

## What happened

**Two load-bearing things landed, both docs/state only — no code.** (1) A
**research-loop timeout clarification** was added to `web-research.md §The
research loop and context management`: the per-item budget poll is a
*between-requests gate* that never aborts a healthy in-flight call; a spent
budget still allows the pass's one terminal **findings** turn (matching the
existing *budget-spent → emits findings* contract at
`portfolio-analysis-logic-flow.md:1008` / `trade-opportunities-logic-flow.md:340`);
and a *separate* per-call stuck-daemon timeout (grounded in `local_model.rs:44`,
600 s) guards a hung request. Codex caught that the first draft ("stop
dispatching the next one") contradicted that contract and would have dropped the
findings turn — the fix was verified and **Codex-approved**. (2) A **sequencing
decision**: the pre-run completion bar was **widened (2026-08-20)** to fold the
live research loop inside it, so the *entire* Portfolio Analysis job is built
before the big run. `BUILD.md §What remains` was reordered (user-authorized) to
**completion block → live research loop → big run → TO**.

## Current state

Nothing in flight. Both edits landed and are verified (`web-research.md`
Codex-approved; the `BUILD.md` reorder grepped clean — items numbered 1–4, no
stale phrases). No gates run (docs + `.metis/` only). Reorder rationale, if
revisited: the research loop is the shared, currently-**unbuilt** web substrate
(no SearXNG / `web_search` / `web_fetch` / readability layer yet; the portfolio
pipeline's research stage is a stub) — build it against Portfolio (the simpler
flat-topic consumer) and prove it in the big run before TO's complex consumer
(routes, discovery, card formation) layers on.

## Open questions

- Should placement divergence join the run-level pooled divergence *rates*
  (band + conviction today)? Offered twice, still unruled; Codex's audit classed
  it optional policy, not a conflict.

## Where to start

**Portfolio completion block** (`BUILD.md §What remains` item 1 — run-evidence
slice first) is the queue head. After it the **live research loop** is now
item 2 (the final pre-test Portfolio slice, discharging the pre-profit
activation obligation), then the big run (item 3). One pre-run to-do recorded in
item 3: the big-run watch set still needs research-loop + pre-profit-activation
watches added.
