# Current session handoff

## What happened

The **live Schwab OAuth slice landed on `main`** (squash `a3f9e40`, PR #48; branch
deleted). It replaces the fixture as the default holdings source behind the existing
`HoldingsSource` trait: a real GET-only `SchwabApiSource`, three-legged OAuth on the
self-signed HTTPS loopback `https://127.0.0.1:8182` (with a per-run `state` nonce and a
5-min bounded capture), the 30-min/7-day token lifecycle with a typed `ReauthRequired`
boundary, and a new `TokenStore` trait putting the app secret + tokens on the **macOS
Keychain rail** (client id is a non-secret in `app_settings`). Source selection is
connection-gated — live when connected, blocked otherwise, fixture only via the
`MARKET_SIGNAL_SCHWAB_FIXTURE` escape hatch. The `/chains` fetch is bounded; chain
faults/drift surface through the holding gap manifest rather than a silent `None`. New
deps: keyring / rcgen / tiny_http (+ chrono serde). Reviewed clean by the metis reviewer
and four Codex rounds; 550 tests + clippy green. **Not rebuilt/reinstalled — the installed
app is still v1.2.1; this is main-only.**

## Current state

On `main` @ `a3f9e40`, clean tree, nothing mid-implementation. The slice's deferred
follow-ons: (a) the **deterministic holdings-snapshot diff** into dossiers — now the
"next" build-order step in BUILD.md; (b) the **frontend Schwab Connect surface** —
button → `schwab_connect`, client_id/secret entry, and a `WarningKind::Schwab` warning
category (none exist yet; the backend command + rail do, but nothing drives them and the
client_secret can't be seeded without them). One design call locked deliberately: the
chain drift guard errors on **both** maps absent, not either, pending live confirmation
of Schwab's "always both maps" invariant. BUILD.md updated this session (Schwab ingestion
bullet + build order).

## Open questions

- **Live Schwab browser round-trip unrun** — only the interactive `#[ignore]`
  `schwab_oauth_live` exercises the real three-legged flow; needs real client_id+secret
  (M1-runnable, by the user). The one thing static review can't cover.
- **Chain both-maps invariant unconfirmed** — tighten the drift guard to either-absent
  once a live `/chains` response confirms both maps are always present.
- **Long/cold-start 600s stress (carried)** — daily length proven; only a long/cold-start
  report could near 600s or truncate.
- **local-model M5 pre-flight (carried)** — 122B load/backend, Ollama #14645, `format`
  constraint, `num_ctx`, throughput.
- **M5-calibration (carried)** — Stooq refresh, `continuity_weight` bands, Research-stale
  threshold, tripwires, DTO budget, leftover-budget ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog
  (carried)** — land with full Portfolio + TO.

## Where to start

Pick up the Portfolio build order: either the **holdings-snapshot diff** (prior-run
snapshot vs current pull → per-position new/increased/decreased/unchanged delta into each
dossier, exited names in the roll-up) or the **frontend Schwab Connect surface** — both
unblocked on the M1. If you instead want the merged slice in the *running* app, a version
bump + `npm run tauri build` ships it (currently main-only, installed app still v1.2.1).
