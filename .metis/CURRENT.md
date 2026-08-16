# Current session handoff

## What happened

Refined the always-run seed-and-merge reuse (shipped `ab82347`) into a
**per-topic research-reuse layer**. Each 6c topic now seeds from **its own
depth-1 distillation** (the tier-1 object under hierarchical 6d, the topic-keyed
group under single-pass) rather than a slice of the cross-topic combined
object — richer per-topic seeds for the same budget, and the topic becomes the
storage partition (seed is a lookup, not a per-claim re-assignment). The rolling
state is a **per-topic layer**, not one flat object. The reduce resolves
cross-topic claim/metric conflicts **globally** (newest-wins) and **emits the
per-topic layer already reconciled** — the model owns the semantic match, since
Portfolio's claims carry no app-matchable cross-topic identity key (so there is
no deterministic app-side write-back; raw tier-1 is not persisted).
**Seeded-vs-cold and the ~4-week gate are per topic**; a dormant conditional
topic keeps its object aging by its own vintage. Within-topic overflow
sub-distills along the **pass seam**, the **pass** (findings + its evidence-ledger
entries) the atomic unit — a dropped pass takes its ledger entries and records a
gap; the bounded prior rides through. Single-homed at `portfolio-analysis.md
§Starting parameters`; propagated to portfolio-workflow, storage, web-research
(the shared distillation primitive), and the logic-flow doc. Eight Codex rounds
to approval. Shipped `fb0b403` + `c76b08f` (two consistency follow-ups).

## Current state

Clean tree on `main` at `c76b08f` (this handoff aside). No work in flight. The
refinement is **doc-only** — the 6c research stage is still stubbed, so it lands
when the research-loop slice is built. BUILD/INDEX confirmed **not** to need
updating (the ruling deepened *within* the contract they already point to). The
build queue is unchanged: completion block (Step-5 context loads + pre-flag +
forensic producer; evidence legs incl. FINRA/CBOE; 6a recall + checkpoint/resume
+ 6g validator; fund depth) → big run (watch-set v9 revision first) → Trade
Opportunities → research loop + refresh lane.

## Open questions

- **Scenario-differentiated priced-fund target formula** — undesigned; the
  shipped flat-driver form is the settled stopgap. Needs its ruling before the
  fund-depth group is planned. (carried)
- **Share-based action sizing** — ruled the only legal action numeric, unbuilt;
  nothing blocks on it. (carried)
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant still
  rests on the 2026-07-16 verification, not re-probed. (carried)

## Where to start

Resume the `logic-flow-docs/portfolio-analysis-logic-flow.md` clarity walk from
**Step 6e onward** — 6c/6d's research-loop and reuse mechanics are now settled
this session. Same posture: read each section, surface confusions, apply clarity
edits with the user, and ground any doubtful claim against the canonical `docs/`.
