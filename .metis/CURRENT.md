# Current session handoff

## What happened

**The Portfolio Analysis strategy-convergence docs (authored last session) were independently re-reviewed and committed** (`92a6798`, pushed to main — 18 files: 12 docs, 4 design-system, BUILD + INDEX). The fresh-session, docs-only review — contradictions + single-home duplication — confirmed the diff clean: every load-bearing change (the `priced`/`role_risk_only` union, loop-time fund routing + the exposure-priced composite, the DGS2-anchored three-state total-return hurdle, momentum-out-of-the-letter, the rolling one-/twelve-month rename, the engine-only quick check, selective re-analysis, outcome learning, the shared FMP/FRED gate) is consistent across the corpus, and **all cross-doc anchor links were mechanically validated — 0 broken**. Four minor nits were found and fixed pre-commit: a run-slot citation redirected to `scheduling.md §Concurrent Job Protection` (the canonical home naming the quick check + pull); the duplicated quick-check trigger list in §Starting parameters reduced to a pointer (its one unique clause folded into the canonical §The quick check); the §Outcome learning intro's window/basis qualifiers scoped to the return-scoring reads; and the design README's leftover cadence exemplars ("Sunday note", "this week" ×2) neutralized.

## Current state

On `main` @ `92a6798`, tree clean, pushed. Portfolio Analysis is now **design-settled and committed** — converged with exactly one named open input: the **fund-form scenario-target methodology**, the first decision of the fund slice's plan (the letter and valuation read don't depend on it; the priced branch's targets, dead-money read, and action sizing do). Trade Opportunities waits design-settled behind Portfolio. Installed app = v1.3.0; the no-build rule holds until Portfolio + TO land.

## Open questions

- **Fund-form scenario-target methodology (new, blocking)** — what a *priced* fund's scenario targets derive from (the equity drift runs on revenue growth, which a fund lacks); the fund-slice plan's first decision.
- **Local-suite scorecard display (carried, deferred)** — whether the TO shadow scorecard and Portfolio outcome scorecard get UI surfaces; both docs now carry the mutual deferral.
- **Encrypted-archive live round-trip (carried, optional)** — one passphrase export→import before the M5 move; comparison method reusable.
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data to render the holdings table).
- **Keychain fail-soft candidate (carried)** — a denied Keychain read blanks the local warning categories for the session; fail-soft with the v2 wiring.
- **Stage-and-swap import hardening (carried)** — mid-import I/O failure can leave partial files; named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage; check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

**Full Portfolio (funds)** — `/metis-plan-task` against `docs/portfolio-analysis.md` §Asset eligibility (fund path). The docs are converged and committed, so planning can start immediately; the plan's first decision is the **fund-form scenario-target methodology** (the named blocking input — §Asset eligibility / BUILD §What remains). Then the Local-models Settings section / sidebar Portfolio-runs history → TO implementation planning.
