# Current session handoff

## What happened

**The market-report docs-review pass (3 of 3) completed — docs review DONE** — and its fixes are committed (`a9121f7`, pushed). Deep read of the planned report enrichment came back structurally sound: all seven planned-paid endpoints verified aligned with the FMP tier audit, the three signals consistent across their homes, the engine-derived / outside-the-delta-engine exclusions holding. Six findings, all approved and applied: **froth trend pairing scoped to the two recent-window counts** (the upcoming-scheduled IPO count is a standalone forward datum — a prior "upcoming" window isn't self-containable); **froth window clamped** (floor + one-month cap, news-window convention) with a **bounded page budget** for the paged `mergers-acquisitions-latest` feed (exhaustion degrades the trend to the recent count alone); the **Step-16 lockstep-prose router bullet now covers the froth extreme** (it contradicted Step 8's contract) and the analyst sentence adds a froth turn; **"every model stage" narrowed** to the baseline-bearing stages (router / analysts / synthesis); a **storage.md §Baseline Snapshots pointer** to the enrichment's snapshot rules; the **breadth ruled-out record added** to §Planned report enrichment (was BUILD-only). Light sweep of the other eight report docs: **clean on both promoted classes**. Anchor sweep: 0 broken. Then wrote **`CODEX-DOCS-REVIEW.md`** at repo root (`e0ce8b2`, pushed) — the user is having Codex independently re-run **all three passes** (scope, weighting, rules, method; deliberately no prior-findings enumeration, for independence; output overwrites `iris-codex-last.md`).

## Current state

On `main` @ `e0ce8b2`, tree clean, pushed. **Docs review 3 of 3 complete** (Portfolio ✓ TO ✓ market report ✓). The **Codex full-corpus round is queued** (user-run, brief in `CODEX-DOCS-REVIEW.md`); the current `iris-codex-last.md` content is stale/superseded — the new round overwrites it. Fund-slice planning stays queued behind the Codex round. BUILD.md needs no revision from this session (its enrichment paragraph is unaffected by the spec-level fixes; the breadth record is now doc-homed too).

## Open questions

- **Fund-form scenario-target methodology (blocking)** — what a *priced* fund's scenario targets derive from; the fund-slice plan's first decision.
- **Local-suite scorecard display (carried, deferred)** — whether the TO shadow scorecard and Portfolio outcome scorecard get UI surfaces.
- **Encrypted-archive live round-trip (carried, optional)** — one passphrase export→import before the M5 move.
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data).
- **Keychain fail-soft candidate (carried)** — a denied Keychain read blanks the local warning categories for the session.
- **Stage-and-swap import hardening (carried)** — mid-import I/O failure can leave partial files; named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage; check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

**Process the Codex round**: when the new `iris-codex-last.md` lands (all three passes, per `CODEX-DOCS-REVIEW.md`), verify every finding against the docs before agreeing — apply what survives, with the user approving fixes. Then **`/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility` (fund path)** — first decision: the fund-form scenario-target methodology.
