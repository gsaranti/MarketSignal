# Current session handoff

## What happened

Short session, two closures. **(1)** The **first live daily report on installed
v1.2.1 ran clean** — so the report-job token/timeout/audit caps (main 32k, analysts
20k, streaming total-timeout 600s, audit body 20k/report) are now **live-verified at
daily length**, no truncation, nowhere near the timeout. They had shipped
offline-verified only. **(2)** Web-verified the **current Schwab developer-portal
registration flow** and the user **applied for Trader API – Individual** — the correct
and sole entitlement for a personal-use app over one's own account (covers holdings +
option-chain market data; *not* Commercial). Learned the two-level distinction and the
callback/approval gotchas (in memory `schwab-api-registration`).

## Current state

On `main` @ `31beaa3`, installed build v1.2.1. **Development is intentionally PAUSED,
gated on the Schwab developer account going fully live** — per the user's call, nothing
proceeds until: Trader API – Individual access approved, AND an app created under it with
**both** sub-products (`Accounts and Trading Production` + `Market Data Production`),
callback exactly `https://127.0.0.1:8182` (no trailing slash), flipped from
`Approved – Pending` to `Ready For Use`. Approval SLA ~a few days (up to ~a month if it
stalls → email traderapi@schwab.com). ⚠️ Schwab intermittently rejects `127.0.0.1`
callbacks — if Create-App errors, that's the likely cause, not our config. Full detail in
memory `schwab-api-registration`.

## Open questions

- **Long/cold-start 600s stress (narrowed from prior 32k/600s item):** daily length is
  now proven clean; a long/cold-start report is the only case that could still near 600s
  or truncate. If it ever does, revisit (raise timeout, or the flagged `read_timeout`
  idle-timeout refactor across the 3 streaming client builders).
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

**Do not start development until the Schwab app is `Ready For Use`.** Once it is: begin
the **live Schwab OAuth slice** — loopback OAuth + holdings pull, verifying code-vs-doc
against `schwab-integration.md` and PR #45's fixture engine math first, then wire the
deterministic holdings-snapshot diff into each dossier.
