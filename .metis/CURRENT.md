# Current session handoff

## What happened

**The live Schwab browser round-trip succeeded** — the user ran Connect from the dev build with real credentials: Safari's self-signed-cert interstitial (Show Details → "visit this website") passed cleanly into the new rustls-0.23 acceptor, the code capture and token exchange completed, and the account reports connected. Two expected-UX facts confirmed: the interstitial is by design (Schwab mandates an HTTPS callback; no CA signs 127.0.0.1), and the macOS Keychain ACL prompt for `market-signal-schwab` recurs after every rebuild because builds are ad-hoc signed (no stable code identity) — "Always Allow" only sticks per binary. The test also surfaced a real bug, **fixed and shipped direct to `main` (`0645351`, pushed)**: the footer showed "Generating report…" during a connect because `schwab_connect` holds the single global `RunGuard` and the footer had one hardcoded label. The guard slot now carries a `RunKind` (`report`/`portfolio`/`schwab-connect`) surfaced as `job_status.running_kind` (kebab-case wire contract test-pinned); the footer labels the in-flight work honestly, shows the connect row immediately on click, and re-reads `job_status` when the connect settles. The Settings connecting hint now spells out Safari's exact bypass steps. Gate green: cargo test 578 + clippy, npm build, npm test 40+107.

## Current state

On `main` @ `0645351`, pushed, clean tree, nothing mid-implementation. **Five slices now accumulated on `main` awaiting a build — #48 (OAuth) + #49 (diff) + #50 (Connect) + #51 (audit remediation) + `0645351` (footer/RunKind); installed app still v1.2.1.** Schwab account is live-connected on the dev data dir. Still deferred: proactive warning-band render of `WarningKind::Schwab`/`LocalModels`, and the Portfolio-page frontend (renders the #49 diff). Informational, accepted: an abandoned connect parks the run slot for up to 5 min with no cancel control.

## Open questions

- **Chain both-maps invariant unconfirmed (carried)** — OAuth is live but `/chains` still unexercised; tighten the drift guard to either-absent once a live response confirms both maps present.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s or truncate.
- **local-model M5 pre-flight (carried)** — 122B load/backend, Ollama #14645, `format` constraint, `num_ctx`, throughput.
- **M5-calibration (carried)** — Stooq refresh, `continuity_weight` bands, Research-stale threshold, tripwires, DTO budget, leftover-budget ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Two clean options: (1) **ship the five accumulated `main` slices into the running app** — 5-anchor version bump + `npm run tauri build` (the live Schwab connection is already proven, so the built app is immediately usable); or (2) the **next Portfolio slice** via `/metis-plan-task` — the **Portfolio-page frontend** (renders the #49 diff + surfaces the local-suite warning band, incl. Schwab) or **full Portfolio (funds)**.
