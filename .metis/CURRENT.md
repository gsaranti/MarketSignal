# Current session handoff

## What happened

A housekeeping session: **BUILD.md's queue was cleaned up** to reflect that
the Portfolio completion block is complete. The finished block was removed
from §Remaining (queue renumbered: research loop 1, big run 2, Trade
Opportunities 3), and the two facts only it carried were re-homed — the
realized-outcome exclusion list (grade normalization, the calibration
proposals, the derive-reads strata) now sits in the §What remains intro
parenthetical, and the 2026-08-15 tunnel-vision conformance-walk record is
cited from §Built's pre-run correctness program bullet. Two stale
cross-references fixed: the big-run item now waits only on the research
loop, and §Deferred by decision says the FINRA/CBOE legs *landed with* the
block. Net shrink 6,310 → 6,192 words, holding the stationary-size ruling.
No code touched; gates unchanged from the fund-depth commit (cargo test
1141/0, clippy clean, npm build clean, npm test 46 + 239).

## Current state

Nothing in flight. BUILD's remaining queue reads: 1 live research loop,
2 big confirmation run, 3 Trade Opportunities.

## Open questions

- Big-run watch set still needs its standing additions (research-loop,
  pre-profit-activation, CBOE-backdrop, narrative-comparator lines), the two
  adopted 2026-08-21 (prompt-stamp note targets **portfolio-v11**; Step-6a
  retrieval structurally empty on the first post-slice run), plus the
  fund-depth slice's **Schwab-CEF-typing watch** (does a held CEF arrive
  `COLLECTIVE_INVESTMENT` → fund path, or `EQUITY` → stock path, which would
  floor-abstain it before detection).

## Where to start

BUILD item 1: **the live research loop** — the final Portfolio Analysis
slice, inside the pre-run bar per the 2026-08-20 widening. It must discharge
the pre-profit producer's activation obligations (§Standing constraints)
before connecting the producer; the cache model (always-run seed-and-merge)
and the disconfirming-fetch placement (once per holding after its topics)
are already ruled. The big confirmation run follows — fold the watch-set
additions in before it.
