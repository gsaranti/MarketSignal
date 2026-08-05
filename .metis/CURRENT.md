# Current session handoff

## What happened

**The two-arm verdict (`portfolio-v7`) was ruled, planned, and implemented — UNCOMMITTED.**
The user deliberately repositioned the Portfolio job ("this tool is about the model, not the engine"): every priced verdict now carries an **engine baseline arm** (existing sub-scores/letter/targets + new mechanical outlook/conviction/action stand-ins) beside an **unrestricted model arm** (own sub-scores, derived letter, freely-authored 1-/12-mo bands, conviction, outlook, lean, 7b action), a **retrospective** prompt block (deliberately reversing the v4 anchoring guard), and a **deterministic scoreboard** in outcome learning.
Internal review: approve-with-nits (nits applied).
**Codex round 1: all three P1s + the P2 verified real and fixed** — overlay prompt language reframed to engine-rule/evidence (no binding language at the model), the engine arm's conviction now observes its own pre-profit ceiling via `clamp_conviction` (re-scoped, not retired), the retrospective now renders the TRUE realized move off the prior run's authoring spot (`PriorHolding.spot`) with target reads relabeled as distances, the scoreboard head-to-head recomputed over the **paired population only** (`HeadToHeadRead` — both arms, same episodes), and the doc contradictions fixed incl. `local-models.md` §Context-memory discipline (Portfolio two-arm carve-out; **TO keeps the single-arm echo-validation rule**).
All gates green after fixes: cargo 934 lib + integration / 0 fail, clippy 0, npm build, 40 node + 217 vitest.

## Current state

**Codex round 2 arrived with further findings — deliberately unread** (context budget). Queue: verify → fix → re-gate → user commits → BUILD/INDEX alignment → review piece 3 → the big confirmation run (which banks the first two-arm vintage; new watches: prompt fit under the retrospective via B12 instrumentation, feasibility-annotation rates, model-vs-engine divergence rates).

**The ruling set that governs finding-triage** (verify every Codex claim against these before agreeing — round 1 precedent: all findings were real, but this design deliberately removed things reviewers read as bugs):
1. Model arm **structurally validated only** — no value bound, cap, clamp, or feasible-set bar; every former restriction is an **annotation** (7b: only self-coherence enforces — sell-all 0–0, range ordering, implied-weight-in-range — with the single named re-run).
2. **Model values never feed deterministic consumers** (quick check, hurdle, monitor stamps, ledger eval, labels read engine only).
3. Engine arm **obeys its own bars** (feasible set, ceilings); caps bind it, annotate the model.
4. Model letter **derived** from model sub-scores via shared cutoffs; model arm **priced-branch only**; stand-in formulas as drafted (21/126/252 @ 2/5/8%; degradation count 0/1–2/≥3; rung rule, never add-aggressively).
5. Head-to-head comparisons **paired-population only**; per-arm reads keep full populations.
6. Conviction-raise triple retired unbuilt; anti-reflexivity survives only in ledger validation.

Files (`-chat.md` = full exchange, `-last.md` = latest message only, both overwritten): round-2 findings = `iris-codex-last.md` (the latest Codex review), with `iris-codex-chat.md` the full Codex exchange for context. Full prior Claude Code conversation = `iris-claude-code-chat.md` — **grep for specifics, never load whole**. Verification = `cd src-tauri && cargo test && cargo clippy --all-targets --all-features; npm run build; npm test`. Deferred: demo-mode visual card check (rides the big run's card-render watch).

## Open questions

- **Two-arm follow-ups** — engine stand-in constants are drafted (calibratable against the scoreboard); model sub-scores/conviction recorded but unscored (predictor-quality read deferred behind the ≥30 bar).
- **Big-run watches** — unchanged carried set (B3 slash-notation + ticker-noise, construction-leg rates, FMP 429-ladder engagements, Stooq-PoW rung order) **plus** the two-arm additions above.
- **Research-loop activation obligation** — identity + source-text validation + period normalization before the pre-profit producer activates; the model arm's diet gains research findings when the loop lands (schema seam left open).
- **Standing** — unchanged carried list from prior sessions (live-run calibration watches, no A letters under grade-v2, reasoning-pane DOM weight, encrypted portability round-trip, step-17 embedding, 600 s stress, checkpoint/resume + the 6g input-delta validator, etc.).

## Where to start

`/metis-session-start`, then read `iris-codex-last.md` (round 2). Verify each finding against the ruling set above before agreeing; fix what's real, dispute what contradicts the rulings, re-run the four verification gates. Then the user commits the whole slice, BUILD/INDEX get aligned to v7, and the queue resumes: review piece 3 (own session) → the big confirmation run in the dev app (process name `market-signal`).
