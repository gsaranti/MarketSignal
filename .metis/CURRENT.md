# Current session handoff

## What happened

Shipped the **selective-run badges slice**. A selective Portfolio run now
analyzes **strictly the user's selection** (ruled 2026-08-16,
`docs/verification/2026-08-16-selective-badges-ruling.md`): the former automatic
safety additions no longer force-include the flagged tail — each surfaces as a
**non-blocking card badge** (attention flag, `unknown` degraded-sweep, evidence
event, side reversal, stale vintage). A held position with no prior verdict is
left **not analyzed** (a selectable "run to grade" placeholder card). The
**held-name research refresh lane** was retired (its only purpose was the
material-update force-include) and the **pre-`v9` migration gate** removed
(`whole_book_era_version` kept for its `pipeline.rs` history-label consumer).
The side-reversal badge is computed from the **invariant long authoring side vs
the current side** — directional verdicts are only ever authored long (net-short
/ net-zero are not-rated), so it is robust across a flip through an exactly-zero
net (Codex flagged an earlier cumulative approach). Full doc sweep +
`.metis/BUILD.md` / `INDEX.md`.

## Current state

Slice complete, reviewed, and landed on `main`. Clean tree. Reviewed by the
Metis task-reviewer (approve) and Codex (three rounds → approve). Verified:
`cargo test` 1037 + clippy clean; `npm run build` + `npm test` 241 component + 46
pure. The `logic-flow-docs/portfolio-analysis-logic-flow.md` **clarity walk was
not advanced this session** — it detoured into this ruling, which began from a
question about the Work-list section's held-name lane.

## Open questions

- **Scenario-differentiated priced-fund target formula** — undesigned; the
  shipped flat-driver form is the settled stopgap. (carried)
- **Share-based action sizing** — ruled the only legal action numeric, unbuilt;
  nothing blocks on it. (carried)
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant still
  rests on the 2026-07-16 verification, not re-probed. (carried)

## Where to start

Resume the `logic-flow-docs/portfolio-analysis-logic-flow.md` clarity walk from
**Step 6e onward** (the earlier sections, including the restructured §Work-list,
were touched this session and are current). Same posture: read each section,
surface confusions, apply clarity edits with the user, and ground any doubtful
claim against the canonical `docs/`.
