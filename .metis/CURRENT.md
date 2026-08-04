# Current session handoff

## What happened

**Outcome learning slice IMPLEMENTED and internally reviewed — uncommitted on `main`, awaiting the Codex round.**
First, BUILD's §Watches capture debt was cleared (driver-ladder raw-order watch dropped; `engine::canonicalize_statements` named as the shared home in the pre-profit as-built paragraph).
Then the slice, planned via `/metis-plan-task` with four flag decisions (selection UI, all recommended): **(1)** standing-thesis creation leg + self-correction read ship dormant until the 6g what-changed attribution validator; **(2)** proposal statistics deferred behind the ≥30-unique-matured-holdings bar (typed below-bar record ships); **(3)** price-bar cache joins the v3 portability archive; **(4)** fail-soft FMP `/profile` sector fetch added (one call per fresh-passed stock) + static sector→SPDR map.
Shipped: new `portfolio/outcome.rs` (branch-typed decision episodes w/ calibration snapshot, lifecycle engine, alignment table, label engine w/ next-session-close anchor / TR-primary / Winkler interval score / grace closures, derived reads), `portfolio_outcome_episodes` + `price_bars` stores, `HoldingAudit.hurdle` + `PortfolioRun.outcome` (serde-default), FMP profile + dividend-history fetches, job-pass integration (transactional run+episode persist; first production Portfolio-namespace durable-learning writer), portability **format v3** (entry sets v1=5/v2=6/v3=8), docs as-built notes in four files.
Internal review: **approve-with-nits**, all 14 criteria pass; three nits fixed post-review (one dividends pull per episode; corrupt-store load logged; SeriesCtx single-fetch-per-symbol-per-pass).
Verified after fixes: cargo 844 lib + 32 integration / 0 fail, clippy 0, npm build clean, 40 node + 188 vitest.

## Current state

The **whole slice sits uncommitted on `main`** (plus the separate BUILD §Watches amendment). Scope report (review-confirmed honest):
- **Deferred**: proposal statistics (later calibration slice); dormant thesis leg + self-correction (6g attribution validator); price-bar `last_requested_at` meta (TO render-time slice).
- **Handled differently**: post-maturity falsifier confirmations attach to the latest **matured episode** typed `post_maturity`, not the thesis ledger; terminal outcomes conservatively `terminal-unscorable` (no corporate-action feed); cohort reads aggregate-only (alignment tag persisted per episode, sliceable later); final-action strata stratify by action rung only — divergence-rationale stratification degenerate until 7b (reviewer's near-miss, no migration needed: `lean_divergence` persisted).

**For the Codex round's attention** (implementer findings): construction-read weight comparison keys on the **ledger's** pre-committed range, never the recomputed sizing band; the full aligned/contrary/partial/unknown alignment table is app-defined beyond the docs' two pinned cells (doc-commented, test-pinned); an uncovered resolvable benchmark leg holds the whole window pending within grace; episodes carry `lean == action` with `lean_divergence` reserved so 7b needs no episode migration.
**Capture debt (post-commit)**: BUILD §Local analysis suite / §What remains + INDEX rows for the slice.
Queue after convergence: the **7b construction stage** slice (picks up carried-action transition-rule validation + lean/action divergence).

## Open questions

- **Research-loop activation obligation** — holding-identity + source-text observation validation + period-normalization hard rule before the pre-profit producer activates (in `pre_profit.rs` doc comment + BUILD).
- **Live-run calibration watches** — STI-absent-reads-zero; YoY quarter-contiguity; now also the outcome leg's big-run watches: sector-resolution rates at 47-position scale, episode-debut volume, profile-call fail-soft behavior.
- **Debut gaps (self-resolve at the big run)** — rate-anchor family + pre-basis FundInfo read `unknown`.
- **No A letters under grade-v2** (META 84.0 vs ≥ 85) — normalization or the big run.
- **Carried unchanged:** big-run checklist in BUILD; reasoning-pane DOM weight; encrypted portability round-trip; step-17 embedding watch; 600 s stress; scorecard display; dev-store residue; Keychain fail-soft; stage-and-swap import; chain both-maps invariant; four-part verdict bound; §1 open drafts; fraud-producer posture; fund-slice drafted constants; checkpoint/resume + the 6g input-delta validator (docs-promised, unbuilt — now also gates the dormant outcome legs).

## Where to start

Run the **Codex review** on the uncommitted outcome-learning diff (`iris-codex-last.md` flow; verify every finding against code before agreeing — the four implementer findings above are the likely contention points), fix-and-converge, then commit the slice (one commit, `main`).
After the commit, catch up BUILD (§Local analysis suite as-built paragraph + §What remains: outcome learning done, 7b next) and INDEX (slice row + concept rows), then `/metis-plan-task` for the **7b construction stage**.
Verification set: `cd src-tauri && cargo test && cargo clippy --all-targets --all-features`; `npm run build`; `npm test`.
