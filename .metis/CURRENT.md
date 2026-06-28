# Current session handoff

## What happened

Two docs-only commits to `main`, both Codex-clean — the local-suite **web-research layer**.

**1. Seed-lineage provenance** (`f080b3c`). Structured-feed seeds (FMP news / articles, macro calendar) that orient a research loop are recorded as **leads, never evidence-ledger claims**; two captures — a deterministic `surfaced_by` back-pointer on a deep-read claim, and a bounded model-attributed `seeded_by` **validated against the route's fed seed-ID set** (unknown refs dropped + logged, so lineage can't be fabricated).

**2. Source-quality research layer** (`1b31bfa`). The Source Registry (per-domain `tier` 0–5/`deny` + `evidenceKinds` + lanePolicy + freshnessSla + extractionProfile + paywall, a *thin override over heuristic defaults*, not a whitelist); the deterministic evidence-annotation seam split **app-computed** (sourceTier/extractionQuality/recency/primarySourceBonus/paywall-stub) vs **model-derived** (claimSpecificity/contradiction/supportsClaimIds); the load-bearing rule **quality informs conviction, never gates discovery** (tiers grade; only the categorical `deny` excludes); cheap fast-follows (extraction telemetry, source-diversity/wire-syndication caps, deny list, a budget-bound + fail-soft disconfirming-fetch pass); and **Connected Sources** (optional subscription enrichment — webview login → Keychain session → authenticated fetch → health-test states, yield-gated, **never on the execution gate**, on the Schwab credential rails). One BUILD.md clause added for Connected Sources; INDEX.md kept current.

## Current state

Nothing in flight; working tree clean, both commits pushed. The research layer is at a coherent resting point. The **"defer-to-M5-calibration" tier was deliberately NOT documented** (user will re-raise): the evidence-quality scoring *combining formula* (the dimensions are defined — tier/recency/extractionQuality/primarySourceBonus — but how they fold into one conviction adjustment is intentionally open), claim-quorum *thresholds*, and **Tavily-as-calibrator**. The whole local suite stays **M5-hardware-gated** for live validation (extraction yield, SearXNG recall, the Connected-Sources fetch path are exactly what the M5 must exercise first).

## Open questions

- **Research-layer M5-calibration tier — parked by intent, not forgotten:** the evidence-quality *combining formula*, claim-quorum *thresholds*, Tavily-as-calibrator. Re-raise when wanted. **User preference: no new Tavily** in this feature — the existing per-candidate-validation Tavily fallback stays untouched; don't re-propose a Tavily calibrator.
- **Portfolio holding-card overflow** (implementation-time UI, carried forward): the full / non-concise standing thesis is the card anchor, so the card must handle a long thesis with graceful overflow when the Portfolio UI is built.
- The **standing design/implementation backlog carries forward unchanged and intentionally not re-enumerated** (per prior handoffs): implementation-time schemas, paid-FMP report enrichment, cross-job isolation, the 35B second-model residency benchmark, BUILD.md compression, and the carried local-suite/build + report-side items — all gated on M5 / paid-FMP.

## Where to start

Research-layer docs are landed (`f080b3c`, `1b31bfa`), Codex-clean, and pushed — nothing to follow up there. Open to pick up: continue the job-doc deepening initiative, or re-raise the parked M5-calibration tier if you want the evidence-quality formula / quorum thresholds specced. All implementation work remains gated on M5 hardware / paid-FMP.
