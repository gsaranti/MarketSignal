# Current session handoff

## What happened

The **holdings-snapshot diff (Portfolio workflow Step 4) landed on `main`** — squash
`be1da53`, PR #49; branch deleted. A new `portfolio::diff` module computes a
deterministic **app-layer** diff of the current Schwab pull vs the prior run's persisted
snapshot: each position tagged **new / increased / decreased / unchanged** (by *position
size* / absolute quantity, so shorts and net long↔short reversals read right; **cost
basis is corroborating context, not a second axis**; matched by **symbol only** —
`Position` carries no CUSIP/lot id), and **exited** names surfaced in the roll-up.
Threaded into `HoldingDossier` + the interpretation prompt, plus an app-set
`position_change` tag on **every** `HoldingVerdict` (graded or not) and `exited` on
`PortfolioRollUp` (both `#[serde(default)]`). No new storage — the snapshot already
persists in `PortfolioRun.holdings`. Docs (`portfolio-analysis.md`,
`portfolio-workflow.md`) reconciled to the "by quantity" rule. Reviewed clean: metis
reviewer (approve-with-nits) + two Codex rounds — one caught a real sign-flip regression
the `abs()` short-fix had introduced (fixed + regression-tested). Full backend gate green
(560 tests + clippy). **Backend-only; installed app still v1.2.1.**

## Current state

On `main` @ `be1da53`, clean tree, nothing mid-implementation. BUILD.md build-order line
updated this session (diff **done — PR #49**; "next" advances). The still-open Schwab
follow-on is the **frontend Schwab Connect surface** — button → `schwab_connect`,
client_id/secret entry, a `WarningKind::Schwab` category (none exist; the backend command
+ Keychain rail do, but nothing drives them, so the client_secret can't be seeded). This
diff shipped backend-only; the **frontend render of the change tag + exited names** is
deferred and pairs naturally with that Connect surface.

## Open questions

- **Live Schwab browser round-trip unrun (carried)** — only the `#[ignore]`
  `schwab_oauth_live` exercises the real three-legged flow; needs real client_id+secret
  (M1-runnable, by the user).
- **Chain both-maps invariant unconfirmed (carried)** — tighten the drift guard to
  either-absent once a live `/chains` response confirms both maps are always present.
- **Long/cold-start 600s stress (carried)** — daily length proven; only a long/cold-start
  report could near 600s or truncate.
- **local-model M5 pre-flight (carried)** — 122B load/backend, Ollama #14645, `format`
  constraint, `num_ctx`, throughput.
- **M5-calibration (carried)** — Stooq refresh, `continuity_weight` bands, Research-stale
  threshold, tripwires, DTO budget, leftover-budget ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog
  (carried)** — land with full Portfolio + TO.

## Where to start

Pick up the Portfolio build order via `/metis-plan-task`: either **full Portfolio
(funds)** — the reduced ETF/mutual-fund path — or the deferred **frontend Schwab Connect
surface** (which would also render this diff's change tag + exited names). Alternatively,
to get the accumulated main-only work (PR #48 OAuth + PR #49 diff) into the *running* app,
a version bump + `npm run tauri build` ships it (installed app still v1.2.1).
