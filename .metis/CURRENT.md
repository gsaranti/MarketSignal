# Current session handoff

## What happened

The **Schwab external long pole is gone.** The Trader API – Individual entitlement was
approved, then the developer app **"Market Signal" was created and got immediate
`Ready For Use`** (no multi-day wait). App Details confirm every load-bearing field: both
`Accounts and Trading Production` **and** `Market Data Production` attached (option-chains
won't 403), callback `https://127.0.0.1:8182` exact/no-slash, Order Limit 120, Client ID +
Secret issued. The intermittent `127.0.0.1`-rejection gotcha did not bite. **Development is
now UNBLOCKED.** Also worked the credential risk model with the user: Schwab has **no
read-only scope** (the product bundles read+trade), **but no money-movement endpoints exist**
— cash/securities cannot be transferred out via this API (money movement is a separate
Advisor Services API); three-legged OAuth additionally gates on the user's brokerage login +
loopback consent, so a leaked Secret alone is bounded. Verified no credentials leaked into
the chat. Memory updated (`schwab-api-registration`, `MEMORY.md`).

## Current state

On `main` @ `31beaa3`, installed build v1.2.1. No code written this session — the work was
portal registration + risk analysis. The next build step is the **live Schwab OAuth slice**
(loopback OAuth on `https://127.0.0.1:8182` + holdings pull). It is **buildable now on the
M1** — OAuth + holdings ingestion is Rust/network, not local-model; only the *model
interpretation* of holdings is M5-gated. A read-only audit was offered (read
`schwab-integration.md` + map PR #45 stubs vs the live path) but not yet run; no slice plan
drafted.

## Open questions

- **Long/cold-start 600s stress (carried):** daily length proven clean; a long/cold-start
  report is the only case that could still near 600s or truncate. Revisit only if it does
  (raise timeout, or the flagged `read_timeout` idle-timeout refactor across the 3 streaming
  client builders).
- **local-model runtime pre-flight (M5, carried):** does the 122B load, on which backend
  (MLX vs Metal/GGUF `mmproj`); is #14645 fixed; does `format` constrain; set `num_ctx`;
  measure throughput (`docs/local-model-operations.md`).
- **M5-calibration (carried):** Stooq refresh, `continuity_weight` bands + Research-stale
  threshold, tripwires, DTO deep-research budget, leftover-budget ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound (carried):** lands with full
  Portfolio + TO.
- **§1 open drafts (carried):** dead-money hurdle, feasible-set bounding; TO risk-tier /
  horizon / hypothesis-score / quota / gate tables.
- **M5-gated backlog (carried):** web-research provisioning / gating / UI +
  rendered-retrieval, analytical-register live-check, no new Tavily, FMP-tier.

## Where to start

Begin the **live Schwab OAuth slice.** First action: read `docs/schwab-integration.md`
end-to-end and audit what PR #45 stubbed (fixture Schwab source) vs. what the live path needs
— loopback HTTPS server + self-signed cert, auth-code→token exchange, Keychain token storage,
30-min/7-day refresh. Verify code-vs-doc + PR #45's fixture-engine math first, then plan. Bake
in two safety invariants as written design constraints: the Schwab adapter is **GET-only (no
order/trading surface)** and **tokens never hit logs or the run tracker.** Then wire the
deterministic holdings-snapshot diff into each dossier. (Minor deferred doc nit to fix while
wiring: `schwab-integration.md`'s "chains free on the individual Trader API" is imprecise —
Market Data is a separately-registered product.)
