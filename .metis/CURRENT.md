# Current session handoff

## What happened

**The Local-analysis-models Settings section + sidebar Portfolio-runs history slice shipped** — planned, implemented, reviewed, and merged (PR #55, squash `cb9d0ff` on `main`; feature branch deleted).
What landed: the provider-credential save split (two-token gate scoped to the cloud submission; FMP/FRED/Tavily + local-model config save ungated — the cloud-keyless path), the Local analysis models Settings section (presence-warning clear path; manual daemon Test Connection with untested / unreachable / model-missing / connected states), the **atomic embedder-identity guard** (identity change clears both local vector namespaces in one transaction with the write; rollback regression-tested), and the shared-history sidebar's Portfolio-runs list (last 10) with a **read-only past-run view** (banner + Back to latest; triggers locked; current-holdings latest-only).
Two decisions shaped it: the design package's **swap-per-feature** sidebar (kit `Sidebar.jsx`) superseded a stacked second group, and guided setup shipped as **test + status + textual install guidance only**.
Review: metis reviewer approve-with-nits (race nit fixed with a selection-seq guard) plus **two Codex rounds to convergence** — five findings verified, four fixed (cross-section edit preservation, transaction atomicity, empty-roster status copy, dedicated history-error channel) + two round-2 nits (mid-save input locks, rollback test).
BUILD/INDEX caught up in-session (user-directed), including closing the stale FMP paid-key-checkpoint references.
Verification green: cargo test 695/0, clippy clean, npm test 40+175, `npm run build`.

## Current state

Nothing in flight — clean tree on `main` at `cb9d0ff`.
Queue head is now **Trade Opportunities** (BUILD §What remains; design settled 2026-07-09, paid-key shapes live-verified, so planning codes against verified shapes).
The **guided-setup follow-up** is recorded in BUILD §What remains as a named, unscheduled slice: in-app `ollama pull` + `ollama serve` start, Install-Ollama deep-link (opener capability), the Settings daemon indicator reflecting run-gate connectivity checks (today manual tests only — the accepted, recorded deviation from `interface.md §Connection status`), and embedder re-embed-from-content (M5-gated).

## Open questions

- **Hurdle × rate-anchored-multiple tightness (M5-calibration)** — the strong test fixture lands dead-money; the bars may bind harder than intended on real names.
- **Fraud-producer posture (carried, review-optional)** — research-fed `forensic_event`, tier-0 lineage.
- **Ollama pin + #14645 behavioral verify (folds into the M5 pre-flight)** — pin ≥ v0.32.0; the schema-integrity check on the pinned version decides when non-thinking distillation unlocks.
- **Local-suite scorecard display (carried, deferred)**; **encrypted-archive live round-trip (carried, optional)**; **dev-app sanity residue (carried)**; **Keychain fail-soft candidate (carried)**; **stage-and-swap import hardening (carried)**; **chain both-maps invariant (carried)**; **long/cold-start 600s stress (carried)** — all unchanged.
- **Local-model M5 pre-flight + M5-calibration (carried)** — the prior list plus the fund slice's drafted constants (CIK-cache staleness, coverage/US guards, tier premiums, add floors).
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with the remaining Portfolio depth slices + TO.

## Where to start

**Run `/metis-plan-task` for Trade Opportunities implementation planning** — the queue head.
TO is large (two jobs, six persisted stores, three discovery feeders); expect planning to carve a first vertical slice rather than one plan for the whole feature — the slice choice is itself the first planning decision.
The docs are implementation-ready: `trade-opportunities.md`, `trade-opportunities-workflow.md`, and the endpoint surface in `data-sources.md`.
