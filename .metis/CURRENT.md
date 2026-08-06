# Current session handoff

## What happened

**The two-arm verdict (`portfolio-v7`) converged and MERGED TO MAIN** — PR #61 squash `78187cb`, feature branch deleted, internal approve + eleven Codex rounds to an approved convergence. Round 2 fixed the last functional findings: the retrospective's price comparisons now cross bases through the outcome slice's **split-safe anchor-close bridge** (`outcome::anchor_session_close` promoted `pub(crate)`; excluded-not-guessed without an anchor bar), and selective carries date by **effective vintage** (`PriorHolding.vintage`). Rounds 3–11 converged the policy language — the 27-finding exhaustive audit, the sizing-ownership corrections (the model authors the 7b range; the engine derives only the deltas), the original "every number comes from the engine" invariant unearthed and rewritten, and finally the boundary statement **single-homed**: *model-arm judgment values never alter or bind the engine baseline* lives only at `portfolio-analysis.md` §The holding verdict (intentional downstream consumers + the typed validated channels named there); every other doc/module mention is a one-line pointer — the structural fix for the restatement drift that fueled the tail rounds. BUILD/INDEX aligned in-branch (invariant bullet, spine/pre-profit/construction v6 claims, the two-arm as-built paragraph, the first-two-arm-vintage watch). Gates green throughout: cargo 969 / 0 fail, clippy 0, npm build, 40 node + 217 vitest.

## Current state

Nothing mid-build; main is clean at the squash commit. **The user set a four-item pre-big-run queue (2026-08-05):**

1. **Re-run review piece 2** — the code-vs-docs conformance walk — against post-v7 main (heavy doc+code churn since). Method + prior dispositions: `docs/verification/2026-08-04-piece2-conformance-walk.md`.
2. **Review piece 3** — the value-chain correctness walk, its own session.
3. **TO docs audit for model decision power** — parity with Portfolio's two-arm philosophy. NOTE: this is a deliberate *revisit* of the kept single-arm carve-out (`local-models.md` §Context-memory discipline scopes two-arm to Portfolio; TO's echo-validation + raise/cap decomposition were repeatedly preserved as-designed during rounds 4–10). A design pass producing rulings, not a conformance sweep — cheaper pre-build.
4. **Cleanup: BUILD.md, INDEX.md, and the extremely long doc lines** — under the sentence-per-line convention long lines ARE long sentences, so this is content-preserving sentence surgery, not reflow; format-only commits go in `.git-blame-ignore-revs`.

Then the **big confirmation run** (dev app, process name `market-signal`), banking the stacked confirmations plus the two-arm watches: retrospective/model-arm prompt fit via B12, feasibility-annotation rates, model-vs-engine divergence rates, the paired card render. The demo-mode visual card check rides that run's card-render watch.

## Open questions

- **Two-arm follow-ups** — engine stand-in constants drafted (calibratable against the scoreboard); model sub-scores/conviction recorded but unscored behind the ≥30 bar.
- **Big-run watches** — the carried set (B3 slash-notation + ticker-noise, construction-leg rates, FMP 429-ladder engagements, Stooq-PoW rung order) plus the two-arm additions above.
- **Research-loop activation obligation** — identity + source-text validation + period normalization before the pre-profit producer activates; the model arm's diet gains research findings when the loop lands.
- **Standing** — unchanged carried list (live-run calibration watches, no A letters under grade-v2, reasoning-pane DOM weight, encrypted portability round-trip, step-17 embedding, 600 s stress, checkpoint/resume + the 6g input-delta validator, etc.).

## Where to start

`/metis-session-start`, then queue item 1: re-run the piece-2 conformance walk against post-v7 main, following the method in `docs/verification/2026-08-04-piece2-conformance-walk.md` (parallel passes → A/B/C triage → fixes/rulings/corrections). Items 2–4 follow in order, each its own session; the big run closes the block.
