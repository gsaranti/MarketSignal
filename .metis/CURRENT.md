# Current session handoff

## What happened

**The 7b construction stage is IMPLEMENTED and internally reviewed — uncommitted on `main`.**
Eight planning assumptions were user-settled up front (all recommended options): intrinsic-bars-only 6f narrowing, `lean` as an added `Option` field, sector-level-only overlap, the three doc-named context causes (carve-out = first two only), dormant hard-forensic wiring, sector identity from existing reads fail-soft, construction-local retry, compress-digests-on-overrun.
As built (prompt contract **portfolio-v6**): 6f now authors the **standalone lean** (full ladder; only severe pre-profit deterioration restricts to the exit family — the feasible-set bars moved to construction), the role-risk 6f call no longer authors an action, and the new `portfolio/construction.rs` owns Step 7a+7b — spine rows + aggregates (fund-folded sector table, clusters, OCC not-rated notional), the joint-feasibility solver with typed violations, the per-holding-narrowed schema, and both prompts. `job.rs` wires aggregates → construct → validate → **one named-violation re-run** → merge (final action, `sizing_from_range` deltas + `sizing_rationale`, action-half `ActionWhatChanged`); episodes record `lean` + `lean_divergence` (no schema migration); frontend renders the lean tag, rationale, action-half line, and the roll-up construction view.
Two app-stamped attribution paths were forced by the closed cause vocabulary (a failing test surfaced the second): an **engine-barred lean** (`engine-bar:`) and an **action reverting to an unchanged lean** (moved-context, cause-less).
Internal reviewer verdict: **approve-with-nits**, all criteria pass; both nits fixed (typed `BookAggregates` in `types.ts`; the constrained-cash draw-down comment in the solver).
Verified: cargo 910/0, clippy 0, npm build, 40 node + 192 vitest (4 new specs).

## Current state

**All work is uncommitted on `main`**: new `src-tauri/src/portfolio/construction.rs` + edits to `mod.rs`, `engine.rs`, `pipeline.rs`, `job.rs`, `outcome.rs`, `dossier.rs`, `quick_check.rs`, `store.rs`, `src/types.ts`, `src/components/PortfolioView.vue`, `tests/components/PortfolioView.spec.ts` (~1,180 insertions).
Queue: branch-commit → external Codex rounds → merge → the post-merge capture catch-up (BUILD/INDEX/docs) → then the block's remainder is only the two display-only UI micro-slices before the big confirmation run.
Codex-round pokes worth pre-verifying: the carried-row episode case (construction-moved carried action ⇒ `lean_divergence: None` — deliberate: stale lean, `vintage_fresh: false`, the cause rides `ActionWhatChanged`), and `WEIGHT_EPS = 0.005` doing double duty (range slack + implied-book drift).

## Open questions

- **Docs-capture debt (post-merge)** — the reversion-to-lean app-stamp rule; §Starting parameters homes for `OVERLAP_CLUSTER_MIN_WEIGHT` 0.20 / `OVERSIZED_MIN_WEIGHT` 0.15 / `WEIGHT_EPS` 0.005; the carried-row `lean_divergence: None` bucketing note in §Outcome learning; BUILD/INDEX as-built rows; the constrained-cash draw-down obligation when the configurable profile lands.
- **New big-run watches** — lean-divergence + engine-bar rates at 47-position scale; whether the construction prompt fits the shared 131k `num_ctx` (settled response: compress digests, never `num_ctx`).
- **Research-loop activation obligation** — identity + source-text validation + period normalization before the pre-profit producer activates.
- **Standing** — live-run calibration watches (STI-reads-zero, YoY contiguity, outcome-leg watches); debut gaps self-resolve at the big run; no A letters under grade-v2 (META 84.0 vs ≥ 85); carried unchanged: big-run checklist; reasoning-pane DOM weight; encrypted portability round-trip; step-17 embedding; 600 s stress; scorecard display; dev-store residue; Keychain fail-soft; stage-and-swap import; chain both-maps invariant; four-part verdict bound; §1 open drafts; fraud-producer posture; fund-slice drafted constants; checkpoint/resume + the 6g input-delta validator (still gates the outcome slice's dormant legs).

## Where to start

**Branch-commit the working tree** (suggested branch `construction-stage`, one commit), then run the external **Codex review rounds** (findings land in `iris-codex-last.md`, gitignored, overwritten per round — verify every finding against code before agreeing, then fix).
Verification set per round: `cd src-tauri && cargo test && cargo clippy --all-targets --all-features`; `npm run build`; `npm test`.
After convergence: merge, then the capture catch-up.
