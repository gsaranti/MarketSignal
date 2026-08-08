# Current session handoff

## What happened

**The long-doc-line cleanup — the pre-run block's last item — is built, reviewed, merged and pushed** (`5fde483`; BUILD alignment `a9dab3a`).

Its blocking scope question resolved **against the way it was framed**: multi-sentence lines are not the defect (0–12% of long lines — the corpus already satisfied sentence-per-line). Sentences had absorbed clauses instead of the docs absorbing sentences, leaving `storage.md`'s run-audit record **one 6,323-character sentence** carrying ~15 separately-checkable claims. **A character ceiling was rejected as the fix**, and that is the durable part: breaking such a sentence at its clauses yields fragments meaningless without their neighbours, so the grep hit gets smaller and less useful in the same proportion. The retrievable unit has to be a self-contained *proposition* — split sentences into sentences, and the existing convention gives each its own line for free. `CLAUDE.md §Docs formatting` gained a **judgment rule, not a ceiling**, deliberately unenforceable by script.

99 prose lines were split across ten docs; **one Codex round, approved**, after two valid P2 fixes — an exhaustive inventory weakened to "Some", and a verb-less "Plus …" fragment, the exact anti-pattern the new rule names. Both classes were swept for siblings, not fixed only where cited.

## Current state

Nothing in flight. Tree clean, `main` == `origin/main` at `a9dab3a`, no other branches. Gates green at the tip (docs-only diff; Codex re-ran them): cargo **1018 + 32**, clippy clean, `npm run build` clean, **46 node + 225 vitest**.

Queue: **BUILD.md + INDEX.md audit → the big confirmation run.**

Audit scope, measured this session: BUILD.md is **~21k tokens** against the ~1.5–4.5k as-built brief it declares. The brief is healthy (first six sections = 280 lines); the drift is **§Local analysis suite (667 lines) + §What remains (255) — 76% of the file** — and §Local analysis suite is a slice-by-slice chronicle its own header rules out ("not the construction history"). INDEX.md has **15 rows over 1,000 chars, longest 3,209**, against a header calling rows "lookup pointers, not summaries". Both audit against a charter they declare themselves — a conformance check, not a taste argument.

**The 600–1,000-char docs band is not queued work** — the new rule binds on touch. Do not re-propose it as a slice.

## Open questions

- **Audit ruling 1 — BUILD.md's slice chronicle compressed in place, or moved out?** Git and `docs/verification/` already carry the construction history and the header disclaims it, so compression is the standing read; the ruling is unmade.
- **Audit ruling 2 — INDEX.md is a rewrite-in-place, never a prune** (no-row-deletion rule): rows get shorter, none disappear. Confirm before starting.
- **TO hard-trigger acceptance cases** — five parked for the TO implementation slice, no other home: carried + deep hard trigger → archived with no shadow entry; identical through all three deep-pass routes; cheap-pass hard signal → warning only; debut hard trigger → shadow rejection; soft trigger → stand-in capped, conviction preserved, no forced archival.
- **Big-run watches** — the carried set enumerated in BUILD.md §What remains, plus the conformance block's four: sector-P/E walk-back depth; the risk-tier distribution now negative-book issuers take High; priced-fund ledger flag rates on the shared 180-day window; the basis-flip rate on a one-quarter feed gap.
- **Design notes awaiting rulings** (piece-3 record) — carried-audit data-health mixing on selective runs; unbounded model-arm renders; "rated N" sidebar wording; `rate_prints.fetched_at` stamp; `ScoredLabel.labeled_at` / `run_date` staying UTC display stamps.
- **Live-evidence caveat** — the sector-P/E walk-back's "holidays serve carried values" warrant rests on the adapter's recorded 2026-07-16 verification, not re-probed; if invalidated, the cardinality claim moves with it.
- **Two items for a later ruling round** (BUILD.md §What remains) — structured warning items (a `WarningCategory` contract change) and the Settings tree's completeness gap in `interface.md`.
- **Two-arm follow-ups / research-loop activation obligation / standing list** — unchanged.

## Where to start

Settle the two audit rulings, then run the **BUILD.md + INDEX.md audit**, each file against the charter its own header states. Then the **big confirmation run** (dev app, process name `market-signal`), reading `data-health` early: Stooq's PoW interstitial may have made the FMP dated-EOD rung de facto primary, and the run's evidence decides the rung-order slice.

**BUILD.md §What remains still reads "Next: the single big confirmation run"** — it does not yet carry the audit. Correct that as part of the audit.
