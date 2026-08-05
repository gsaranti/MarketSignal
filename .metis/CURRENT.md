# Current session handoff

## What happened

**B3, the listing-resolution guard, is COMPLETE** — planned, five parameters user-ruled, built, internally reviewed (approve-with-nits), one Codex round, both commits pushed (`22534fd`, `4ee464c`).
The rulings: US listing = **exchange allowlist NYSE/NASDAQ/AMEX** (all OTC/PNK → not-rated `unsupported listing`, the exchange test — never HQ country — is what passes US-listed ADRs); conflict = **zero-shared-significant-token** (conservative, stopword-normalized; nothing-to-compare → unverified, never conflict); **unverified → proceed + degraded input**; **no persistence** (recomputed per fresh pass); scope = exactly the guard.
As-built: new `portfolio/listing.rs` (pure rules), `fetch_profile_identity` tri-state on the one per-stock `/profile` call (`fetch_profile_sector` retired; sector identity rides the same lookup), `ListingResolution` on the dossier, routing beside the eligibility gates (no-resolution/non-US → not-rated; conflict → insufficient-evidence w/ ledger + prior-vintage retention), and the **guard-terminal fetch skip** (the loop's first short-circuit — all four per-symbol pulls tripwire-pinned).
Codex round fixed the one real boundary defect: **only the definitive empty-array body reads Unresolved** — drifted/malformed-but-valid-JSON shapes are Unverified, never terminal; plus honest guard-terminal audit sources and the "never re-spent" doc reword.
Docs flipped as-built (portfolio-analysis §Asset eligibility + §Evidence floor identity arm + §Starting parameters constants; workflow Step 3/6a; data-sources profile row) — the B2 "in build" line is no longer stale.
**BUILD + INDEX aligned in-session at user request** (same commit as this handoff): BUILD's as-built list + Portfolio Analysis bullet gain the guard, §What remains now queues three B slices and carries the two new listing-guard big-run watches; INDEX gains the slice row, the status paragraph and concept row updated.
Verified: cargo 921 lib + 32 integration / 0 fail, clippy 0, npm build, 40 node + 206 vitest.

## Current state

Queue unchanged after B3: **B7 → B10+B13, then review piece 3, then the big confirmation run.**

- **B7 profile alignment** — `objective` field + medium-to-high default + 7b render + read-only Settings block; a deliberate change to the framing the big run banks.
- **B10 + B13** — card-display pair (momentum tile set apart as market-setup; MonitorScenario render), plausibly one UI slice under the design package + frontend-craft.

## Open questions

- **B7 scope confirm** — the ruled option covered field + mapping + 7b render; the read-only Settings block was assumed in from the record's original option — confirm at plan time.
- **B10+B13 packaging** — one combined UI slice or two; decide at plan time.
- **New big-run watches from B3** — (1) **slash-notation class shares** (`BRK/B`): the verbatim FMP lookup would read them Unresolved → not-rated, where the statements floor previously abstained; (2) **ticker-noise descriptions** ("NTDOF COM" tokenizes to the ticker token alone → could read Conflict against the real issuer name). Real Schwab shapes on the big run settle whether either needs a rider (slash↔dash normalization is the cheap one).
- **Big-run watches (construction leg)** — unchanged: lean-divergence / engine-bar / carried-stale-lean rates, construction-prompt fit (instrumented), overlay classification vs real OCC rows, 7b decided-range movement rate.
- **Research-loop activation obligation** — identity + source-text validation + period normalization before the pre-profit producer activates.
- **Standing** — unchanged carried list: live-run calibration watches (STI-reads-zero, YoY contiguity, outcome-leg watches), no A letters under grade-v2, big-run checklist, reasoning-pane DOM weight, encrypted portability round-trip, step-17 embedding, 600 s stress, scorecard display, dev-store residue, Keychain fail-soft, stage-and-swap import, chain both-maps invariant, four-part verdict bound, §1 open drafts, fraud-producer posture + the `hard_forensic_bar` consumer seam, fund-slice drafted constants, checkpoint/resume + the 6g input-delta validator.

## Where to start

**`/metis-plan-task` on B7** (investor-profile alignment): confirm the Settings-block scope first, then plan the `objective` field, the medium-to-high default mapping, the 7b prompt render, and the read-only Settings block if confirmed in. Then the B10+B13 pair, then piece 3 in its own session, then the big run per the locked plan.
