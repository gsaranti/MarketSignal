# Current session handoff

## What happened

A **release session**: cut **v1.2.1** carrying last session's report-job
token/timeout/audit tuning (`65a9fa6`+`50d6ffc`) over v1.2.0. Bumped the **5
version anchors** 1.2.0→1.2.1, re-ran the full gate green (cargo test 555/0/22,
clippy clean, npm build + 40 node + 91 vitest), committed the release-tip
`31beaa3`, built via `npm run tauri build`, installed to `/Applications` over
v1.2.0 (quarantine clean, `getVersion` verified 1.2.1), tagged annotated
`v1.2.1` on `31beaa3`, pushed commit + tag. **No functional change beyond the
version anchors** — v1.2.1 is a version-only bump. Per user's call, the
recommended **pre-tag live report was skipped**. (Detail in memory
`release-build-install` + `token-budget-tuning`.)

## Current state

On `main` @ `31beaa3`, pushed, in sync; tag `v1.2.1` on the release-tip;
installed build is v1.2.1. The **32k / 600s / 20k caps are IN the shipped build
but offline-verified only** — no live run has exercised them yet. Next
build-order step is the **live Schwab OAuth slice** (`schwab-integration.md`
audited clean; PR #45 has the MVP engine math — check code-vs-doc before wiring).

## Open questions

- **Live-calibration of 32k/600s (now armed):** the **first daily/long report on
  installed v1.2.1** is the real check — stays under 600s, no truncation? If it
  nears the ceiling, revisit (raise timeout, or the flagged `read_timeout`
  idle-timeout refactor across the 3 streaming client builders). ~16k-token daily
  runs sit well inside.
- **local-model runtime pre-flight (M5, carried):** does the 122B load, on which
  backend (MLX vs Metal/GGUF `mmproj`); is #14645 fixed; does `format` constrain;
  set `num_ctx`; measure throughput (`docs/local-model-operations.md`).
- **M5-calibration (carried):** Stooq refresh, `continuity_weight` bands +
  Research-stale threshold, tripwires, DTO deep-research budget, leftover-budget
  ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound (carried):** lands with
  full Portfolio + TO.
- **§1 open drafts (carried):** dead-money hurdle, feasible-set bounding; TO
  risk-tier / horizon / hypothesis-score / quota / gate tables.
- **M5-gated backlog (carried):** web-research provisioning / gating / UI +
  rendered-retrieval, analytical-register live-check, no new Tavily, FMP-tier.

## Where to start

**Run one live daily report on v1.2.1 first** (you run daily anyway) to close the
32k/600s calibration — watch the run tracker for near-600s / truncation. Then the
next build-order step is the **live Schwab OAuth slice** (+ deterministic
holdings-snapshot diff into each dossier); confirm code-vs-doc against
`schwab-integration.md` / PR #45 before wiring.
