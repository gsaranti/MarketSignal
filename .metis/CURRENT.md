# Current session handoff

## What happened

**The conformance ruling round completed, merged to `main`, and pushed** (`ae42703`; three squash merges — `859bef4` Batch A, `4ea18c8` Batch B, `e52a0cc` Batch C, plus the Metis alignment and a post-merge correction).
All **23** findings the scoped conformance check carried were ruled in one pass and built in three independently-ruled batches: the run-gating twelve first, then the four that gate nothing plus the embedding validator, then six doc-only corrections.

**Two findings did not rule the way they were carried** — the statement-basis flip was nominated accept-with-note and ruled a **defect** on the Tier-1 fund fix's precedent, and the `/chains` cardinality item ruled one way rather than either, since a non-gradeable class reads none of the retrieval spent on it.
**Two pushbacks were made and granted:** verbose-but-true credential rows are not incorrect information, and reports' insertion order is deliberately **not portable**.

The load-bearing outcomes are two systemic classes swept whole, each now with a **citable home** so a later pass checks conformance against one statement instead of re-deriving an inventory — session-keyed dates on the ET session (`docs/data-sources.md` intro; fetch-range bounds deliberately UTC, annotated in place), and identity-or-lifecycle selections on insertion order (date-ordered report reads deliberately unmoved). As-built detail is in BUILD.md §Local analysis suite; full dispositions and all review rounds in `docs/verification/2026-08-07-scoped-conformance-check.md`.

**The block's durable lesson, recorded in BUILD.md:** the code fixes held from the first review round, and **every later finding across five rounds was the accuracy of a claim made around them** — enumerated exits standing in for an invariant, a shared presence test for two renderers, "all four routes" for three, equivalence claims for a narrow guarantee, a pin whose un-pruned assertion the prose then quoted four times, and the same over-broad shape reappearing in the Metis summary of those very rounds. Operative rule: when a round corrects a claim, re-check the test it cites, and treat any headline needing a trailing caveat as not yet true.

## Current state

Nothing in flight. Working tree clean, `main` == `origin/main` at `ae42703`, no branches besides `main`.
Verification at the tip: cargo **1018 lib + 32 integration / 0 fail**, clippy **0 warnings**, `npm run build` clean, **46 node + 225 vitest**.

Queue to the big run:

1. **The long-doc-line cleanup** — blocked on a scope definition (see Open questions). It is the last item before the run.
2. **The single big confirmation run** (dev app, process name `market-signal`) — banks every stacked runtime confirmation. Its watch set now also carries this block's probes: sector-P/E walk-back depth, the risk-tier distribution now negative-book issuers take High, priced-fund ledger flag rates on the shared 180-day window, and the basis-flip rate on a one-quarter feed gap (now gated, so what the run measures is how often the gate fires and how much of the valuation surface it types unevaluable).

Two items were **recorded for a later ruling round rather than absorbed**, both in BUILD.md §What remains: structured warning items (emitting missing credentials as structured items from both gates instead of composed prose — a `WarningCategory` contract change), and the Settings tree's **completeness** gap (`interface.md` omits two built panels, Data and the document-truncation diagnostics, while listing three designed-and-unbuilt ones), which the doc half's flag-only-incorrect charter excluded.

## Open questions

- **Long-doc-line cleanup scope — needs a definition before it can start.** Under the project's sentence-per-line convention a long line holding one long sentence is already conformant, so the item must target something narrower (multi-sentence lines? a character ceiling?). `docs/storage.md`'s price-bar-cache paragraph is the named pre-existing case; this block lengthened lines in `data-sources.md`, `portfolio-analysis.md` and `storage.md` too.
- **TO hard-trigger acceptance cases** — five cases parked for the TO implementation slice (no other home): carried + deep hard trigger → archived with no shadow entry; identical through all three deep-pass routes; cheap-pass hard signal → warning only; debut hard trigger → shadow rejection; soft trigger → stand-in capped, conviction preserved, no forced archival.
- **Big-run watches** — the carried set (Schwab `averagePrice` multiplier, `^GSPC` mapping, estimates ordering, SEC sub-annual durations, FMP in-progress-bar, sector-taxonomy joins, SHV-style labels, exchange codes/B3, OCC slash notation, boundary-day rates, two-arm divergence rates) plus this block's four above.
- **Design notes awaiting rulings** (piece-3 record) — carried-audit data-health mixing on selective runs; unbounded model-arm renders; "rated N" sidebar wording; `rate_prints.fetched_at` stamp; `ScoredLabel.labeled_at` / `run_date` staying UTC display stamps.
- **Live-evidence caveat** — the sector-P/E walk-back's "holidays serve carried values" warrant rests on the adapter's recorded 2026-07-16 verification, not re-probed; if invalidated, the cardinality claim moves with it.
- **Two-arm follow-ups / research-loop activation obligation / standing list** — unchanged.

## Where to start

Settle the **long-doc-line cleanup's scope** first — it is one question (what counts as too long, given sentence-per-line already permits long single sentences) and it is the only thing standing between here and the run. Do that cleanup, then start the **big confirmation run** in the dev app and read `data-health` early: Stooq's PoW interstitial may have made the FMP dated-EOD rung de facto primary, and the run's evidence is what decides the rung-order slice.
