# Current session handoff

## What happened

Finished the Step 6d clarity cleanup and ran a Step 6b clarity batch on the
`logic-flow-docs/portfolio-analysis-logic-flow.md` walk.
**6d** (`c5b9c30`, `c1e74d1`, `d833253`): folded the two routing branches into one
consolidation-call block **and added the missing deterministic single-vs-hierarchical
routing fact** (the orchestrator sizes the aggregate; only single-pass sees it whole),
labeled the merge block, reverted a wrong "stale"→"cached", and rewrote the overflow
block as **map/reduce distillation calls** (one map per pass + one reduce, 2–4).
**6b** (`72fbe00`): an order-of-computation + through-line intro, the overlay calcs
marked **persisted, not scratch**, and the terminal Output rewritten into "what leaves
the step" — the deterministic analysis vs the true two-arm **engine-arm** subset (only
sub-scores/letter + targets + later stand-ins; only targets/outlook actually scored),
the persisted working reads, and the post-interpretation stand-ins. The 6b batch took
~6 Codex rounds: its behavioral claims diverge from the canonical docs and only the
Rust pins them.

## Current state

Clean tree, all pushed. The clarity walk is **paused at Step 6e**. This session was
docs-only on the one logic-flow doc; no `BUILD.md` / `INDEX.md` change was needed. This
`CURRENT.md` rewrite is the session-end handoff (it retires the long-stale pre-session
diff that still said "resume at 6d, clean tree").

## Open questions

- **Scenario-differentiated priced-fund target formula** — undesigned; the shipped
  flat-driver form is the settled stopgap. (carried)
- **Share-based action sizing** — ruled the only legal action numeric, unbuilt;
  nothing blocks on it. (carried)
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant still rests on
  the 2026-07-16 verification, not re-probed. (carried)

## Where to start

Resume the clarity walk at **Step 6e — Recalculate targets using validated research**
(as-built: the pre-profit overlay finalization is the whole built work; the
forward-assumption / observation legs are designed, landing with the research loop).
**Ground new behavioral claims against the Rust** (`pre_profit.rs`, `engine.rs`,
`pipeline.rs`, `outcome.rs`, `fund.rs`), not just `docs/` — the 6b rounds all came from
doc↔as-built divergence. Codex per batch, commit per batch; then 6f / 6g, 7–9, and the
Quick check / Pull holdings sections.
