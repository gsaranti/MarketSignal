# Thesis Continuity and Evolution

The system maintains continuity between reports and treats market analysis as an evolving long-term process rather than a collection of disconnected market snapshots.

Each report exists within a broader market narrative that develops over time.

The main agent continuously:
- reasons over recent report context supplied by the application layer
- uses relevant historical learnings retrieved from vector memory
- audits prior thesis accuracy
- follows up on prior market concerns
- tracks whether previous assumptions are strengthening or weakening
- updates long-term theses incrementally as new evidence appears

The system is designed to behave like a professional analyst team maintaining ongoing market coverage rather than a stateless news summarizer.

For the main agent's broader responsibilities, see [agents.md](agents.md). For how memory is retrieved and stored, see [storage.md](storage.md).

## Report Continuity

Reports should naturally flow from previous reports.

Examples:
```text
Previous report:
"The primary market risk remains whether elevated oil prices begin bleeding into core inflation."

Next report:
"That concern increased after core CPI accelerated while oil remained elevated."

Later report:
"Inflation pressure has not yet materially damaged AI infrastructure spending, but rising yields are becoming a larger risk to valuation multiples."
```

The system should:
- continue monitoring unresolved market risks
- revisit previous conclusions
- acknowledge when earlier assumptions were incorrect
- identify when a thesis is strengthening or weakening

The system should also periodically evaluate:
- whether prior concerns evolved as expected
- whether the system overemphasized unimportant narratives
- whether important signals were missed
- and whether the broader market thesis remained directionally correct

## Thesis Stability

The system should avoid unnecessary thesis instability.

Long-term market theses should evolve gradually when:
- market conditions remain structurally similar
- existing narratives continue holding
- incoming data reinforces prior conclusions

The system should not dramatically change positioning or outlook because of isolated short-term volatility, temporary news cycles, or single-event reactions.

The main agent should prioritize:
- signal over noise
- multi-week confirmation when appropriate
- and structural changes over temporary volatility

## Thesis Pivot Conditions

The system may rapidly pivot its outlook when major evidence materially changes the market environment.

Major thesis pivots should remain relatively rare and should only occur when evidence strongly suggests that structural market conditions have materially changed.

Examples include:
- major geopolitical escalation
- financial system stress
- persistent inflation regime shifts
- abrupt central bank policy changes
- major recession indicators
- significant AI infrastructure slowdown
- supply-chain disruptions
- systemic credit events
- major energy disruptions

In these situations:
- reports may heavily focus on the new event
- prior assumptions may be explicitly challenged
- the long-term thesis may be revised aggressively

The system should clearly explain:
- why the thesis changed
- which assumptions failed
- what evidence caused the pivot

## Memory-Guided Evolution

The vector memory system exists to help the main agent maintain analytical continuity over time.

The application retrieves relevant memory from the vector store, and the main agent uses those retrieved memory fragments to:
- identify similar historical conditions
- revisit previous conclusions
- track recurring market patterns
- avoid repeating analytical mistakes
- maintain coherent long-term reasoning across reports

The goal is not rigid consistency.

The goal is:
- coherent reasoning
- gradual evolution when appropriate
- decisive adaptation when necessary
