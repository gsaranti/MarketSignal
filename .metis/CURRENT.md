# Current session handoff

## What happened

**The full Portfolio (funds) slice was triaged through two Codex review rounds, fixed, and merged to `main` (fast-forward `a91e53b → bda9a06`); the feature branch is deleted local + remote.**
Round 1 (`108c628`): of 8 findings, 6 confirmed and fixed — net-short positions → not-rated with a short reason; the fund evidence floor now enforces the expense-ratio and country-weighting legs on the exposure-priced branch (**behavior change**: a fund without country weightings now abstains where it previously priced on an "assumed US" note); a failed DGS10 anchor-window history is fail-soft (`RateAnchors.history_gap` → degraded inputs) — only the two prints hard-fail; analyst-estimates consensus returns `None` with no forward-dated row (no stale-row masquerade past `no-admissible-driver`); TTM dividends bounded both sides; option-overlay funds get a deterministic name-screen flag — priced (not `role_risk_only`), barred from the Low tier without forcing High.
One finding was a deliberate deferral (the `role_risk_only` action authored in-loop — the 7b construction slice owns the move, both branches), one was doc catch-up (pending→as-built flips across portfolio-analysis/workflow/TO).
Round 2 (`6789501`): `AnchorObservation.spread` became `Option` so raw multiples survive a failed DGS10 join (raw-percentile fallback, never the current-multiple carry); the fund classification + structural flag now ride `GradedVerdict` (`fund_class_label`/`structural_flag`, serde defaults) and render on the priced card.
BUILD/INDEX catch-up ran (user-directed, `bda9a06`).
Verification green throughout: cargo test 690/0, clippy clean, `npm run build`, npm test 40 + 155.

## Current state

Nothing in flight — the fund slice is fully landed on `main`, no open branch, working tree clean.
Honest residue (unchanged, now recorded in BUILD §What remains): the research loop stays the stub; thesis ledger, quick check, selective re-analysis, held-name refresh lane, pre-profit overlay, outcome learning, and the 7b construction stage are designed depth slices, unsequenced; grade normalization rides TO's engine work; persisted price-bar cache later; CFTC fund mapping + N-PORT skipped.

## Open questions

- **Hurdle × rate-anchored-multiple tightness (M5-calibration)** — the strong test fixture lands dead-money; the bars may bind harder than intended on real names.
- **FMP shape assumptions (paid-key checkpoint)** — new-endpoint field spellings and the expense-ratio `/100` normalization are fixture-pinned; `sector-pe-snapshot` last-weekday keying gaps on market holidays.
- **Fraud-producer posture (carried, review-optional)** — research-fed `forensic_event`, tier-0 lineage.
- **Local-suite scorecard display (carried, deferred)**; **encrypted-archive live round-trip (carried, optional)**; **dev-app sanity residue (carried)**; **Keychain fail-soft candidate (carried)**; **stage-and-swap import hardening (carried)**; **first post-v0.31.2 Ollama release (carried)**; **chain both-maps invariant (carried)**; **long/cold-start 600s stress (carried)** — all unchanged.
- **Local-model M5 pre-flight + M5-calibration (carried)** — prior list plus the fund slice's drafted constants (CIK-cache staleness, coverage/US guards, tier premiums, add floors) and the two items above.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with the remaining Portfolio depth slices + TO.
- **Legacy-docs broken anchor (carried, trivial)** — `legacy_docs/NOTES-FROM-RESTRUCTURING.md:15`.

## Where to start

**Run `/metis-plan-task` for the next queue item (BUILD §What remains): the Local-analysis-models Settings section + sidebar Portfolio-runs history.**
The Settings section's named code prerequisite is the provider-credential save split out of the token-gated cloud save (`configuration.md §API Tokens`), and it is the in-app clear path for the shipped presence warning.
After that: Trade Opportunities (design settled 2026-07-09, ready for implementation planning).
