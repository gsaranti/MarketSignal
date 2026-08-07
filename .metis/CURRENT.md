# Current session handoff

## What happened

**The Portfolio docs sweep completed and merged** (PR #66, squash `1a610b2`, branch `portfolio-docs-sweep` deleted) — the TO sweep's charter verbatim, walking `portfolio-workflow.md` Steps 1–9 + the quick check against the owning docs, six parallel passes with **targeted code reads** on built machinery. Six findings, two user rulings: the whole-book **fund fold is sector-only** (the country leg unbuilt *by decision*, though `etf/country-weightings` still feeds the per-holding tilt, so the eight other "sector / country" mentions are correct), and the **investor profile is docs-sharpened, not code-restricted** — it reaches no model call before 7b and intrinsic judgment stays profile-independent, but the loop's engine bounds a sizing delta by `available_cash` (inert under the fixed preset). The other four: the stale TO raise-contract parenthetical, the forked post-grace label closure, the selective work-list's no-prior-verdict leg, and the options signal (computed 6a, consumed 6f). **Three Codex rounds** then corrected the options-signal *consumer* — a defect the sweep's own fix introduced — the **option / bond cost-basis display contract**, and the bond error direction (an option's basis understated ~100×, a bond's **overstated** ~1000×); four claims were pushed back as out-of-charter. BUILD/INDEX aligned in-session. The only non-doc change was a `mod.rs` doc comment.

**Recorded decision:** pieces 2 and 3 are **not** being re-run wholesale. Piece 2 was already re-run post-v7 (`0d74434`) and piece 3 postdates it, so the unwalked delta is only two commits, both externally reviewed. Piece 3's finance-intent instrument waits until **after** the big run, where it can reason against real distributions; its fixes would otherwise perturb the very code the run is meant to confirm.

## Current state

Nothing in flight; working tree clean on `main`. The queue, in order:

1. **Scoped conformance check + off-spine doc pass** (agreed this session; an eight-way parallel fan-out was drafted and cancelled before running — re-plan next session). Framing: *confirm any doc change from this session that implies code work is implemented, and that the implementation is correct.* Correcting docs to match code implicitly ruled the code right, but each finding verified only the one site it cited — so re-check each changed behavior for consistency **across all paths**. Scope: the two unwalked commits (`cdb7977` piece-3 fix batch, `512d5ec` ET slice) plus the doc sections the spine-driven sweep never walked as a spine — `portfolio-analysis.md` §Storage and display and peers, `storage.md`, `schwab-integration.md`, `interface.md`, `configuration.md`. That gap is real, not theoretical: Codex's display-contract finding came from exactly there.
   **Test first — the one item with a concrete failure scenario:** does the unsuppressed option / bond cost basis escape the frontend's `multiplierUnverified` suppression into the **7a spine rows, the 7b construction prompt, or the outcome episode features**? `averagePrice` 3.50 × qty 2 on a $700 option wires `cost_basis` 7, so a fabricated ~+$693 unrealized gain would reach the model. `schwab-integration.md` says the derived total *is* what the roll-up and the action-sizing spine's unrealized P/L mean, and no Rust equivalent of the frontend gate was confirmed to exist.
2. **Long-doc-line cleanup** — now also covering the lines this session lengthened in `portfolio-workflow.md` and `portfolio-analysis.md`; `storage.md:187`'s unindented continuation is the pre-existing item.

Then the **big confirmation run** (dev app, process name `market-signal`). Any code fixes from item 1 should land and settle before it.

## Open questions

- **TO hard-trigger acceptance cases** — parked for the TO implementation slice (no other home): (1) carried + deep hard trigger + model `still-valid` → archived, no shadow entry; (2) identical through all three deep-pass routes (rotation / re-surface / ATO Deep Audit); (3) cheap-pass hard signal → warning only; (4) debut hard trigger → shadow rejection; (5) soft trigger → stand-in capped, model conviction preserved, no forced archival.
- **Big-run watches** — the carried set (Schwab `averagePrice` multiplier, `^GSPC` mapping, estimates ordering, SEC sub-annual durations, FMP in-progress-bar, sector-taxonomy joins, SHV-style labels, exchange codes/B3, OCC slash notation, boundary-day re-raise/force-include rates) plus the Portfolio two-arm divergence rates.
- **Design notes awaiting rulings** (piece-3 record) — carried-audit data-health mixing on selective runs; unbounded model-arm renders; "rated N" sidebar wording; `rate_prints.fetched_at` stamp; `ScoredLabel.labeled_at` / `run_date` staying UTC display stamps.
- **Two-arm follow-ups / research-loop activation obligation / standing list** — unchanged.

## Where to start

`/metis-session-start`, then queue item 1: re-plan the **scoped conformance check + off-spine doc pass** as a parallel fan-out, leading with the **cost-basis propagation trace** — the one hypothesis carrying a concrete failure scenario. Flag-only-incorrect still governs the doc half; the code half reports concerns with a failure scenario attached. Bring findings back for rulings before editing. Item 2 follows; the big run closes the block.
