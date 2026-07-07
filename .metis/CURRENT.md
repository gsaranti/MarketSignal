# Current session handoff

## What happened

**The Portfolio-page frontend shipped — PR #52, squash-merged to `main` (`03728aa`).** Two pre-implementation decisions rewrote the docs (6 files): the trigger model became **two independent buttons** — Run analysis always pulls fresh holdings itself; **Pull holdings** is view-only (persisted latest-pull snapshot, **never the diff baseline**, gated on Schwab only so it works today with no local models) — and the display model was pinned: four states, a fresher pull renders as a stamped section **above** the run-anchored cards (never merged into them), churn tagged **presence-only by symbol**. The slice: `PortfolioView.vue` (graded/not-rated/insufficient cards, key-figure strip, roll-up with the #49 exited render, `.ana-sortbar` sort bar), backend `pull_holdings`/`latest_portfolio_run`/`latest_holdings_pull` (`RunKind::HoldingsPull` → "holdings-pull") + `check_local_configuration` (presence-only schwab/local-models → warning band; cloud report gate untouched), the shared tracker placed on the run's **owning page** (no /8 fraction for portfolio runs), and a `.ana-tag` design-package extension. Review: metis approve-with-nits (applied) + two Codex rounds — notably the **optional fast tier no longer gates** (blank fast falls back to the reasoner for distillation; the docs-vs-code gate discrepancy is resolved). Gate: cargo test 616 + clippy, npm build, npm test 40+130.

## Current state

On `main` @ `03728aa`, pushed, clean tree, nothing mid-implementation. **Six slices now accumulated on `main` awaiting a build — #48 (OAuth) + #49 (diff) + #50 (Connect) + #51 (audit remediation) + `0645351` (footer/RunKind) + #52 (Portfolio page); installed app still v1.2.1.** Deferred out of #52 (flagged in the PR): sidebar Portfolio-runs history (`store::list_recent_runs` unwired), a portfolio-specific tracker layout (per-holding grouping — full-Portfolio slice), and card fields the narrow slice doesn't persist (thesis anchor, standalone lean, action rationale, dead-money read, monitor). Two knowns: the **local-models warning has no in-app clear path** (the Local-analysis-models Settings section is unbuilt — a dev launch on this M1 shows it proactively now), and `check_local_configuration` reads the Keychain at startup, so ad-hoc dev rebuilds see the ACL prompt at launch.

## Open questions

- **Chain both-maps invariant unconfirmed (carried)** — OAuth is live but `/chains` still unexercised; tighten the drift guard to either-absent once a live response confirms both maps present.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s or truncate.
- **local-model M5 pre-flight (carried)** — 122B load/backend, Ollama #14645, `format` constraint, `num_ctx`, throughput.
- **M5-calibration (carried)** — Stooq refresh, `continuity_weight` bands, Research-stale threshold, tripwires, DTO budget, leftover-budget ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Two clean options: (1) **ship the six accumulated `main` slices** — 5-anchor version bump + `npm run tauri build` (the band showing local-suite warnings on launch is the new proactive render working, not a regression); or (2) the next slice via `/metis-plan-task` — the **Local analysis models Settings section** (gives the new proactive local-models warning its in-app clear path — the most user-visible gap #52 opened), the **sidebar Portfolio-runs history**, or **full Portfolio (funds)**.
