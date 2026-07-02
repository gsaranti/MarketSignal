# Current session handoff

## What happened

The **frontend Schwab Connect surface landed on `main`** — squash `03a819f`, PR #50,
branch deleted. Closed the two credential write-paths that had made a connection
un-seedable: `save_schwab_credentials` (client id → `app_settings`, secret → Keychain,
secret written only when non-blank; **a client-id change clears the now-stale token set**,
a secret rotation does not), `schwab_status` (client id + `secret_configured` + connection
state not-connected/connected/expired + refresh expiry, from **local token state only**,
secret never returned), `schwab_disconnect` (clears tokens, keeps creds). **`WarningKind::Schwab`**
is now produced + consumed at the local run-gate via a new `schwab_gate` (parallel to
`local_gate`), kept **off the cloud `validate` gate** — a disconnected Schwab account
blocks only the local jobs, never the report (the load-bearing correction to the plan,
which had said to append it to `check_configuration`; that would have gated the report).
Frontend: a "Charles Schwab connection" Settings section (generic-chrome/monochrome), a
parallel `SchwabStatus` type (not folded into the closed 5-key credential union),
Save/Connect/Disconnect with save-before-connect + busy/connecting gating; App.vue wiring.
**Scoping discovery: no Portfolio page UI exists at all**, so the #49 diff change-tag +
exited-names render is not in this slice. Cleared metis review (approve-with-nits) + two
Codex rounds; all findings fixed. Gate green: cargo test 566 + clippy; npm build + npm
test 105.

## Current state

On `main` @ `03a819f`, clean tree, only `main` locally, nothing mid-implementation.
BUILD.md build-order line updated this session (Connect surface **done — PR #50**; "next"
advances). **Three slices now accumulated on `main` awaiting a build — PR #48 (OAuth) +
#49 (diff) + #50 (Connect); installed app still v1.2.1** — a 5-anchor version bump +
`npm run tauri build` ships them. Deferred: the **proactive warning-band render** of
`WarningKind::Schwab` (shared 1:1 with `LocalModels` — both variants are produced at their
run-gates but neither renders in the warning band yet; lands with the local-suite
frontend), and the **Portfolio-page frontend** itself (no local-suite UI exists), which is
where the #49 diff would render.

## Open questions

- **Live Schwab browser round-trip — now RUNNABLE, still unrun** — creds are seedable in
  the UI (PR #50); the real three-legged flow needs real client_id+secret on the M1
  (`#[ignore]` `schwab_oauth_live`, or the live Connect button).
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

Two clean options: (1) **ship the accumulated PRs #48/#49/#50 into the running app** — a
5-anchor version bump + `npm run tauri build`, which also unblocks the live Schwab
round-trip test on the M1; or (2) the **next Portfolio slice** via `/metis-plan-task` —
either **full Portfolio (funds)** (the reduced ETF/mutual-fund path) or the
**Portfolio-page frontend** (renders the #49 diff and surfaces the local-suite warning
band, incl. Schwab).
