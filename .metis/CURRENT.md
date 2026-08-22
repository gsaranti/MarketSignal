# Current session handoff

## What happened

The **fund-depth slice shipped** (`39c281e`) — the Portfolio completion
block's last bullet, and the block is now **complete**. Four rulings
2026-08-21: the flat-driver fund target form is the **settled design**
(closing conformance-walk R27; a scenario-differentiated formula returns only
on realized-outcome evidence); N-PORT stays deferred; the CEF price-vs-NAV
read is prompt evidence + card only; and the CEF leg was re-scoped to
**detection + gap-honest seam** after a live probe found FMP `etf/info`
serves closed-end funds an empty body — detection reads the one-per-fund
`profile` (isFund AND a closed-end description fragment; full-pass only), a
real CEF now takes `role_risk_only` labeled "closed-end fund". One metis
round + five Codex rounds to approval; every round's dispositions are in
`docs/verification/2026-08-21-fund-depth-rulings.md`. **BUILD was then
compacted** (8,343 → 6,310 words) under a new standing ruling: a landed slice
enters §Built as ONE bullet + record pointer, never a paragraph; body carries
decision + why, mechanics live in docs; size stays roughly stationary. Two
formerly BUILD-only facts were re-homed (`export.md §PDF Export` gained the
`@page` gotcha, `schwab-integration.md` the rustls-acceptor rationale).

## Current state

Nothing in flight. The slice's gates at commit: cargo test 1141/0, clippy
clean, npm build clean, npm test 46 + 239 (the PortfolioView spec exists —
CLAUDE.md's spec list was stale and is fixed). The BUILD + INDEX absorption
and compaction committed with this handoff.

## Open questions

- Big-run watch set still needs its standing additions (research-loop,
  pre-profit-activation, CBOE-backdrop, narrative-comparator lines), the two
  adopted 2026-08-21 (prompt-stamp note targets **portfolio-v11**; Step-6a
  retrieval structurally empty on the first post-slice run), plus the
  fund-depth slice's **Schwab-CEF-typing watch** (does a held CEF arrive
  `COLLECTIVE_INVESTMENT` → fund path, or `EQUITY` → stock path, which would
  floor-abstain it before detection).

## Where to start

BUILD item 2: **the live research loop** — the final Portfolio Analysis
slice, inside the pre-run bar per the 2026-08-20 widening. It must discharge
the pre-profit producer's activation obligations (§Standing constraints)
before connecting the producer; the cache model (always-run seed-and-merge)
and the disconfirming-fetch placement (once per holding after its topics)
are already ruled. The big confirmation run follows — fold the watch-set
additions in before it.
