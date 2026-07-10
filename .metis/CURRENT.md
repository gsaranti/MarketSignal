# Current session handoff

## What happened

**A final multi-session docs review began, and its Portfolio pass completed.** The contract: contradictions + dangerous duplication + only job-result-affecting strategy errors — filtered by "does it change code output or job quality" (explicitly not a strategy re-audit). A cold read of the full Portfolio group (~40 cross-checked contracts across portfolio-analysis/-workflow plus the Portfolio sections of storage / configuration / interface / local-models / local-model-operations / schwab-integration / web-research / data-sources / scheduling) found **no contradictions and no strategy errors**. Seven fixes landed (`c177981`, pushed): a **#14645 mode-caveat pointer** in portfolio-workflow §How to read (the doc had stated exactly the bugged non-thinking + `format` combination); **Step 7b pinned to thinking**; **FMP `profile` added to the tier audit** (live-verified 2026-07-10 — on-plan, no US-exchange constraint); **configuration.md's daemon call path corrected** to Ollama's native API; **`historical-sector-pe` memoize-on-first-need** semantics; and two single-homings — the **run-audit-record field set → storage.md** (conviction decomposition + `role_risk_only` branch scoping moved in; workflow Step 8 and §Storage and display reduced to pointers) and the **exposure-composite mechanics → §Asset eligibility**. Anchor sweep over all 24 docs: 0 broken. Three nits deliberately left as immaterial: the "fixed" embedder wording, §Evidence floor's "too few quality sources" fuzz, the feasible-set conviction rationale.

## Current state

On `main` @ `c177981`, tree clean, pushed. Docs review is 1 of 3 passes done: Portfolio ✓ → **Trade Opportunities next, deliberately in a fresh session** (cold-read quality) → market report last. Fund-slice planning stays queued behind the review. **TO-pass carry-forwards:** (1) the **#14645 caveat twin** — trade-opportunities-workflow.md's legend / Step 5e states non-thinking + `format` with no caveat pointer; (2) the **audit-record enumeration twins** — trade-opportunities.md §Storage and display and trade-opportunities-workflow.md Step 8 still enumerate the field set; consolidate onto storage.md §Local Analysis Suite Storage and generalize its now-Portfolio-scoped conviction-decomposition clause ("per-holding", cites Step 6g) to TO's per-pick form; (3) **verify TO §Starting parameters reads rf + 8/16/30** — Portfolio's hurdle bullet cites it; (4) the `profile` audit fix already covers TO's per-candidate call — no action needed.

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

**Trade Opportunities docs-review pass** (fresh session, cold read): trade-opportunities.md + trade-opportunities-workflow.md + the TO-relevant sections of the shared docs, under the same review contract, starting from the four carry-forwards in Current state. The market-report pass follows; when all three passes are done, `/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility` (fund path) — first decision: the fund-form scenario-target methodology.
