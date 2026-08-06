# Current session handoff

## What happened

**Queue item 1 — the TO docs model-decision-power audit — done, ruled, and applied; PR #65 is OPEN and unmerged** (branch `to-docs-two-arm`, commit `9268100`, 6 files +198/−81, docs only). The audit found TO holding the exact conviction machinery Portfolio retired at `portfolio-v7` — the raise triple, the app-derived `final_conviction`, cap-only levers clamping the model — and explicitly claiming parity with it. The carve-out at `local-models.md` was real but scoped to **numbers only**, so conviction, archetype, tier/horizon views and the retrospective sat outside the ruling while being treated as inside it.

**Five rulings, all to the recommended disposition**, and the fifth went further than proposed: TO gets a **real second arm on the numbers** (own sub-scores, bands, implied-expectations read), and **admission became either-arm** — a name clearing either gate enters, stamped `admitted_by` with both gate vectors, making "should the model's judgment bind admission?" a measured question. Contract single-homed at `trade-opportunities.md` §The opportunity. Three classes stay single-valued by design: facts and their arithmetic, matrix placement, and the outcome machinery (whoever keeps score can't be a player). Grant scoped to the entry-asymmetry gate alone — floor, forensic hard triggers and anchorless `hype` still bind absolutely on both arms.

**The load-bearing process call:** fourteen Codex rounds produced 27 findings, all real — but rounds 3–14 were all the *reserved blind-first counterfactual* accreting built-feature contract surface, with the last five findings each created by the prior round's fix. We cut that back: the reservation, its two interventions, diagnostic-only authority and a reconstructability requirement stay; call placement, timeouts, cardinality, state vocabulary, eligibility timing and arbitration are **deliberately unspecified until TO is built**. Codex concurred and contributed the reconstructability refinement.

## Current state

Nothing mid-build. **PR #65 is open and not merged** — next session starts there.

The queue, in order:

1. **The final TO logic sweep** (the agreed next task) — walk `trade-opportunities-workflow.md` Steps 1→10 plus the ATO flow as the spine, checking each step's claims against the docs that own them: `data-sources.md` (what endpoints exist / what's off-plan), `trade-opportunities.md` (contract), `storage.md` (what's persisted), `local-models.md` / `configuration.md` (substrate). **Flag only incorrect information** — contradictions between docs, claims a source or engine read doesn't support, flows that can't happen in the stated order. Anything shaped "this should also specify X" is *missing* information and goes to task planning untouched. Read-and-report; bring findings back for rulings before editing.
2. **Cleanup: the extremely long doc lines** — content-preserving sentence surgery (format-only commits go in `.git-blame-ignore-revs`). Two known items: this slice added prose to already-long lines, and `storage.md:187` carries a pre-existing unindented continuation.

Then the **big confirmation run** (dev app, process name `market-signal`).

## Open questions

- **BUILD/INDEX alignment for this slice is not done** — deliberately deferred while PR #65 is unmerged. When it lands: the TO as-built/designed entry needs the two-arm contract, either-arm admission, and the deferred-execution reservation.
- **Big-run watches** — the carried set (Schwab `averagePrice`, `^GSPC` mapping, estimates ordering, SEC sub-annual durations, FMP in-progress-bar, sector-taxonomy joins, SHV-style labels, exchange codes/B3, OCC slash notation, boundary-day re-raise/force-include rates) **plus new from this slice: the band and conviction divergence rates**, now a standing recorded read rather than an experiment input.
- **Design notes awaiting rulings** (piece-3 record) — carried-audit data-health mixing on selective runs; unbounded model-arm renders; "rated N" sidebar wording; `rate_prints.fetched_at` stamp; `ScoredLabel.labeled_at` / `run_date` staying UTC display stamps.
- **Two-arm follow-ups / research-loop activation obligation / standing list** — unchanged.

## Where to start

`/metis-session-start`, then **check PR #65's state** (merged? review comments?). If merged, do the BUILD/INDEX alignment for it first — that's the one piece of this slice still outstanding. Then queue item 1: the final TO logic sweep, scoped to *incorrect* information only, per the framing in Current state. Item 2 follows; the big run closes the block.
