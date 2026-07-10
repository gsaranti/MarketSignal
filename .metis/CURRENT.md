# Current session handoff

## What happened

**The Trade Opportunities design docs went through a full strategy audit and were hardened to convergence** — a 10-finding Fable audit, all findings landed, then three external Codex review rounds each verified against the docs and folded in, ending in an explicit sign-off from both reviewers (commit `6577097`, pushed to main). The load-bearing additions: a **shadow outcome ledger** (calibration learns from the names the funnel *turned away* — typed decision episodes with full gate vectors, per-class picked-vs-rejected spreads, a strict outcome-measurement contract with typed terminal outcomes); the leading-metric gate reshaped to **metric families** (archetype-mapped, seasonal comparability, per-family minimum history); **falsifier re-check classes + persistence semantics** so the cheap sweep tests each thesis's own break-conditions; the horizon split into `expected_thesis_realization` (sets the cell) vs `business_runway` (feeds conviction); the entry-asymmetry threshold defined (DGS2-anchored, tier-scaled); a **maintenance-priority rotation slice** of the deep budget so the live matrix self-refreshes; the factor-distribution store demoted to **diagnostic-only** (a convenience sample is never a market percentile — the one place the original design was wrong); a three-partition episode library; propose-only calibration; a SUE-standardized continuation feeder; and an implied-expectations range read. BUILD.md's TO bullet + §What remains status tag and 14 INDEX rows were updated in the same commit.

## Current state

On `main` @ `6577097`, tree clean, pushed. A docs-only session — TO remains **designed, not built**, but its investment logic is now **settled and ready for implementation planning**; the build queue is unchanged (full Portfolio funds first). Installed app = v1.3.0; the no-build rule holds until Portfolio + TO land. Deliberate scope choices to remember: `interface.md` was left untouched (no UI surface committed for the shadow scorecard or rotation picks), episode-library curation (~40–60 dated episodes) is a build-time task inside TO implementation, and the representative-universe factor snapshot is a named calibration-tier upgrade path, not scheduled work.

## Open questions

- **TO display decisions (new, deferred)** — whether the shadow scorecard (spread / false-negative flags) and rotation picks get any UI surface; deliberately not committed to `interface.md`.
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

The standing order is unchanged: **full Portfolio (funds)** — `/metis-plan-task` against `docs/portfolio-analysis.md` §Asset eligibility (fund path); BUILD.md §What remains carries the queue. Trade Opportunities now waits design-settled behind it (then the Local-models Settings section / sidebar Portfolio-runs history → TO implementation planning).
