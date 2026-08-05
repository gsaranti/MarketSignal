# Current session handoff

## What happened

**B7, the investor-profile alignment, is COMPLETE** — planned, three plan-time user rulings, built, reviewed **approve (no nits)**, pushed (`79e781c`; BUILD/INDEX aligned in-session, `58cd04a`).
The rulings: the read-only Settings block **in scope**; risk stays the existing **`Aggressive` variant** (no new enum rung) rendered **"aggressive (medium-to-high)"**; docs match surfaces (configuration.md's risk bullet gained the aggressive-rung representation note — zero drift across docs / prompt / Settings); the Settings payload is **backend-composed display strings** from the same Rust `label()` helpers the prompt uses — one label source, no TS mirror.
As-built: typed `ProfileObjective::MaximizeProfit` + fixture default `Moderate → Aggressive` on the never-persisted `InvestorProfile` (verified: no serde default needed), the 7b construction prompt renders the objective clause + exact framing (prompt-pinned — the framing the big run banks), `get_investor_profile` returns `default_fixture().display()` rows (shape-pinned), and the read-only Settings section rides the diagnostics idiom (own fail-soft startup channel, omitted-on-null, no form controls, spec-pinned; placed above Schwab per the interface.md tree).
One reported widening: App.spec's startup exact-set pin + the shared tauri.ts mock gained the command.
Verified: cargo 922 lib + 32 integration / 0 fail, clippy 0, npm build, 40 node + 208 vitest.

## Current state

Queue after B7: **B10+B13 card-display pair → review piece 3 (own session) → the big confirmation run.**

- **B10 + B13** — the momentum tile set apart as a market-setup read (display-only) + the MonitorScenario card render (user-elevated pre-run); plausibly one UI slice under the design package + frontend-craft.

## Open questions

- **B10+B13 packaging** — one combined UI slice or two; decide at plan time.
- **Big-run watches from B3** — (1) slash-notation class shares (`BRK/B`) read Unresolved → not-rated under the verbatim FMP lookup; (2) ticker-noise descriptions ("NTDOF COM") risk a false Conflict. Real Schwab shapes on the big run settle whether either needs a rider.
- **Big-run watches (construction leg)** — unchanged: lean-divergence / engine-bar / carried-stale-lean rates, construction-prompt fit (instrumented), overlay classification vs real OCC rows, 7b decided-range movement rate — and the run now also banks **B7's changed profile framing** in the 7b prompt.
- **Research-loop activation obligation** — identity + source-text validation + period normalization before the pre-profit producer activates.
- **Standing** — unchanged carried list: live-run calibration watches (STI-reads-zero, YoY contiguity, outcome-leg watches), no A letters under grade-v2, big-run checklist, reasoning-pane DOM weight, encrypted portability round-trip, step-17 embedding, 600 s stress, scorecard display, dev-store residue, Keychain fail-soft, stage-and-swap import, chain both-maps invariant, four-part verdict bound, §1 open drafts, fraud-producer posture + the `hard_forensic_bar` consumer seam, fund-slice drafted constants, checkpoint/resume + the 6g input-delta validator.

## Where to start

**`/metis-plan-task` on the B10+B13 pair**: decide the packaging first (one combined UI slice or two), then plan against the ruling record (piece-2 §Rulings) — B10 is display-only differentiation of the momentum tile on the holding card, B13 renders the typed `MonitorScenario` payload (`types.ts` marks it display-deferred). Design package + frontend-craft bind. Then piece 3 in its own session, then the big run per the locked plan.
