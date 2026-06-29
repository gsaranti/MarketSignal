# Current session handoff

## What happened

Ran a **v2 docs open-questions audit** (4 parallel readers across the local-suite
corpus) to confirm the requirement docs are dev-ready. Resolved all **§2 contradictions**
— four product calls (**Connected Sources into v2**, source-registry override UI
**deferred**, investor profile **read-only panel**, **Tavily fallback kept**), plus
fast-tier **not gate-bearing**, buying-power / dossier-tag / warning-tree clarifications,
TO Step-4 **provisional archetype** for the quota, and the **pattern/case-study lens
restored** to TO Step 5d. Checked **PR #45 code**: most Portfolio engine math is already
implemented as **calibratable placeholders**, so added **"Starting parameters
(calibratable)"** sections to `portfolio-analysis.md` + `trade-opportunities.md` (doc-sync
the as-built constants + draft the genuinely-open pieces). **Two Codex rounds** caught real
code-vs-doc accuracy bugs (grade is **impute-to-50, not renormalize**; stale comments; cash
gating) — all fixed. Resolved the cash **code-vs-preset gap** properly:
`InvestorProfile.available_cash` → **`Option<f64>`** (`None` = unconstrained = the preset's
stance), aligning code to BUILD.md's "cash always available." Committed as two (docs
`13570ff`, code `3cf53f2`), pushed to `main`. `cargo test` 523 ✓ + clippy clean.

## Current state

On `main` @ `3cf53f2`, in sync with origin. **v2 docs are dev-ready.** §2 resolved; §1
engine math reframed (Portfolio mostly already coded + doc-synced; open pieces drafted as
calibration surfaces; TO starting table drafted). **BUILD.md confirmed NOT needing an
update** — the changes sit below its as-built altitude, and the cash fix aligned code to
the existing "cash always available" stance. Remaining open buckets are all **plan-time /
per-slice**: §3 SQLite DDL for the not-yet-built TO tables (opportunity graph, thesis
ledger), §4 config-knob default values, §5 keyless endpoint URLs (SEC EDGAR / Stooq /
FINRA / CBOE / SearXNG). Build order unchanged.

## Open questions

- The **calibration-surface pattern**: engine constants are MVP starting values flagged
  *shadow-tune-not-pin* — live-calibrate on the M5 (carried).
- The §1 **genuinely-open drafts** (dead-money hurdle = risk-free + 5%, action
  feasible-set bounding, material 5% threshold, conviction-layer trips; the TO
  risk-tier / horizon / hypothesis-score / quota / gate table) are starting values to
  validate when each slice is built.
- Optional **BUILD.md micro-tweak**: tighten the gate wording (`:307`) to "(endpoint +
  *required* roster ids — reasoner + embedder)" for fast-tier-optional consistency.
  Skippable.
- Standing **M5-gated backlog** unchanged (web-research provisioning/gating/UI +
  rendered-retrieval live validation, analytical-register restyle live-check, no new
  Tavily, local-model live quality/FMP-tier checks).

## Where to start

Begin the **live Schwab OAuth slice** (next in build order; `schwab-integration.md`
audited clean — OAuth loopback, 30-min / 7-day tokens, Keychain, positions + option chains
all concretely specified). **When touching any Portfolio formula, check code-vs-doc
first** — PR #45 already implements the MVP engine math, so a doc "gap" may just be the doc
lagging the code.
