# Current session handoff

## What happened

A **full security audit of the Schwab auth surface** (PRs #48+#50, dev-complete, pre-live-round-trip) ran this session: **clean verdict — no exploitable vulnerability, live round-trip safe to run**. Verified negatives worth keeping: no leak channel for secret/tokens/auth-code anywhere (no logging framework in-tree; progress seam has no URL field; the Schwab source is deliberately un-wrapped by `RunContext`), outbound TLS clean (rustls 0.23, zero escape hatches), state nonce 122-bit CSPRNG single-use fail-closed, GET-only pinned by test. Four low-severity hardening findings were then **fixed and merged same session — PR #51** (squash `4109dbc`, branch deleted): (F1) `tiny_http` — which hard-pinned the EOL rustls 0.20.9/ring 0.16.20 stack, RUSTSEC-2024-0336 — replaced by a new `loopback_https.rs` one-shot blocking acceptor on rustls 0.23 (`default-features=false` + ring; no aws-lc-rs; EOL subtree verified gone via `cargo tree -i`), capture behavior preserved fail-closed and now covered by **offline real-TLS round-trip tests** (previously live-only); (F2) code-bearing redirect target redacted from `parse_redirect_code`'s error context; (F3) redacting manual `Debug` on `SchwabTokens`/`TokenResponse`; (F4) token-endpoint error body capped at 300 chars. Metis review approve-with-nits; all nits fixed (mid-request deadline check, mismatch wording, redundant filter). Deliberate wire deltas: close-tab page now `text/html` (was mis-served as `text/plain`), `Connection: close`, no Server/Date headers. Gate green: cargo test 576 + clippy clean; npm build.

## Current state

On `main` @ `4109dbc`, clean tree, only `main` locally, nothing mid-implementation. **Four slices now accumulated on `main` awaiting a build — #48 (OAuth) + #49 (diff) + #50 (Connect) + #51 (audit remediation); installed app still v1.2.1** — a 5-anchor version bump + `npm run tauri build` ships them. Still deferred: proactive warning-band render of `WarningKind::Schwab`/`LocalModels`, and the Portfolio-page frontend (renders the #49 diff). Informational audit items deliberately not acted on: no request timeout on the two Schwab reqwest clients (robustness), PKCE absent (fine for a confidential client; optional if Schwab supports it), disconnect is local-delete only (no server-side revocation).

## Open questions

- **Live Schwab browser round-trip — runnable, still unrun** — real client_id+secret on the M1 (`#[ignore]` `schwab_oauth_live` or the live Connect button); now also the first real-browser exercise of the new rustls-0.23 acceptor (interstitial handshake aborts, speculative connections).
- **Chain both-maps invariant unconfirmed (carried)** — tighten the drift guard to either-absent once a live `/chains` response confirms both maps present.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s or truncate.
- **local-model M5 pre-flight (carried)** — 122B load/backend, Ollama #14645, `format` constraint, `num_ctx`, throughput.
- **M5-calibration (carried)** — Stooq refresh, `continuity_weight` bands, Research-stale threshold, tripwires, DTO budget, leftover-budget ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Two clean options: (1) **ship the four accumulated PRs #48/#49/#50/#51 into the running app** — 5-anchor version bump + `npm run tauri build`, which also unblocks the live Schwab round-trip on the M1; or (2) the **next Portfolio slice** via `/metis-plan-task` — **full Portfolio (funds)** or the **Portfolio-page frontend** (renders the #49 diff + surfaces the local-suite warning band, incl. Schwab).
