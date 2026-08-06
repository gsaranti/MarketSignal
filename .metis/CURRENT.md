# Current session handoff

## What happened

**Queue item 2 — review piece 3, the deterministic value-chain correctness walk — COMPLETE and MERGED TO MAIN** (PR #63 squash `cdb7977`, branch deleted). Eight parallel passes by value-chain segment (correctness vs finance intent — not doc conformance): 33 raw findings → **30 verified** (3 cross-pass convergences), **eleven user rulings all to the recommended disposition**, and the 25-item fix batch applied same-session. Headlines: the house-view truncate panic, the 366-day TTM dividend window, `^spx`→`^GSPC` fallback mapping, monotonic date-keyed observation identity + ack-clear + same-id corrected-clean reset, the **signed P/E derive (`grade-v2.1`** — input semantics, no band move), the quarter-contiguity guard at five window sites, class-aware 6g executability, **always-percent fund weights** (wire contract settled by the FMP reference) + **absolute composite coverage**, the role-risk full pass computing the price-derived ledger legs, **insertion-order run identity** (`id`-primary store queries; `created_at` = display), parsed-date consensus ordering + the strict undatable-row split, the FRED 10-day anchor bound, duration-phrase guards for "short"/"ultra", and option/bond cost-basis render suppression. Internal review approve-with-nits (fixed) + **three Codex rounds (8→6→2) to approval**. Record: `docs/verification/2026-08-05-piece3-value-chain-walk.md`. Gates at merge: cargo 973 lib + 32 integration / 0 fail, clippy 0, npm build, 40 node + 222 vitest.

## Current state

Nothing mid-build; main is clean at the squash commit. The queue, in order:

1. **The ET-dating / outcome-hardening slice** — ruled and fully specced in the piece-3 record (§The ruled follow-up slice): ET session dating via `market_clock` for the per-holding evidence boundary and the outcome entry anchor + basis bridge (rule the news-leg inclusivity and the frontend stale-tag boundary with it), the bear-line bridge keyed at the **intrinsic vintage** (excluded-not-guessed when uncovered), and the price-bar fetch range from the symbol's **earliest active-episode anchor**. Own session, before the big run.
2. **TO docs audit for model decision power** — the design pass on the kept single-arm carve-out (queue item 3, unchanged).
3. **Cleanup: the extremely long doc lines** — content-preserving sentence surgery under the sentence-per-line convention (format-only commits go in `.git-blame-ignore-revs`). BUILD/INDEX piece-3 alignment was done post-session-end 2026-08-06 (as-built batch entry, grade-v2.1 + run-identity + contiguity clauses, the ET-slice queue rewrite, the piece-3 INDEX row), so only the line-length surgery remains here.

Then the **big confirmation run** (dev app, process name `market-signal`), banking the stacked confirmations plus the expanded watch/probe set.

## Open questions

- **Big-run watches/probes** — the carried set plus piece 3's additions: Schwab `averagePrice` multiplier (sizes the real cost-basis fix behind the render suppression), `^GSPC`-mapping sufficiency, analyst-estimates page ordering, SEC sub-annual durations, FMP in-progress-bar behavior, sector-label taxonomy joins, SHV-style short-screen mislabels, exchange-code strings (B3), OCC slash notation.
- **Design notes awaiting rulings** (piece-3 record) — carried-audit data-health mixing on selective runs; unbounded model-arm renders; "rated N" sidebar wording; `rate_prints.fetched_at` stamp.
- **Two-arm follow-ups / research-loop activation obligation / standing list** — unchanged from the prior handoff.

## Where to start

`/metis-session-start`, then the ET-dating / outcome-hardening slice: plan it against the piece-3 record's §The ruled follow-up slice (the design is ruled — this is a build session, so `/metis-plan-task` fits). Items 2–3 follow in order; the big run closes the block.
