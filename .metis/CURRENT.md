# Current session handoff

## What happened

Ran the **Step 6a** and **Step 6b** clarity batches on the
`logic-flow-docs/portfolio-analysis-logic-flow.md` walk.
**6a** (`d93600c`): de-duped the intro/identity paragraphs, grounded the listing
guard (FMP company-profile fetch queried by the Schwab symbol as-is, US-exchange +
name cross-check, all four `ListingResolution` outcomes incl. degraded-continue),
named the gather fields, nested elaborating bullets, rewrote the Output as
what-leaves. Three Codex rounds.
**6b** (`a084816`): added an engine-primitives preamble (`scale` / `average` /
`ratio`) and gave **every value the four-aspect format** — what it is / inputs +
source / equation / where it lands — grounded against `engine.rs`, `pre_profit.rs`,
`fund.rs` by four parallel explorer agents (the doc's behavioral claims only the
Rust pins). Expanded the risk-tier thresholds and fixed nesting. Four Codex rounds.
Format calls settled this session: **example-fields-in-parens** for gather-leg lists
(6a); **preamble + four aspects** and **clean equations with footnoted guards** (6b)
— carry these forward for consistency.

## Current state

Clean tree, all pushed. The clarity walk now has **6a + 6b done** and is **paused
before Step 6e**. Docs-only on the one logic-flow doc; no `BUILD.md` / `INDEX.md`
change was needed.

## Open questions

- **Scenario-differentiated priced-fund target formula** — undesigned; the shipped
  flat-driver form is the settled stopgap. (carried)
- **Share-based action sizing** — ruled the only legal action numeric, unbuilt;
  nothing blocks on it. (carried)
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant still rests
  on the 2026-07-16 verification, not re-probed. (carried)
- **Line-513 "applied" vs "decided"** — the Fund-routing paragraph says the route is
  "applied" at 6b, but the code decides it there; Codex judged it a non-blocker. A
  one-word fix if touched. (parked from the 6a batch)

## Where to start

Resume the clarity walk at **Step 6e — Recalculate targets using validated
research** (as-built: the pre-profit overlay finalization is the whole built work;
the forward-assumption / observation legs are designed, landing with the research
loop). **Ground new behavioral claims against the Rust** (`pre_profit.rs`,
`engine.rs`, `pipeline.rs`, `outcome.rs`, `fund.rs`), not just `docs/` — every 6a/6b
Codex round came from doc↔as-built divergence; for equation-level sections dispatch
parallel grounding explorers as 6b did. Reuse the 6b value format + the primitives
preamble. Codex per batch, commit per batch to `main`; then 6f / 6g, 7–9, and the
Quick check / Pull holdings sections.
