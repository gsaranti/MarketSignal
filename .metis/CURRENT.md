# Current session handoff

## What happened

**Codex round-7 docs review fully resolved** (`4bd0f40`, pushed). All 9 findings adversarially validated (9 parallel agents): 7 confirmed, 2 partial — P1-3's "no condition object" half refuted (Portfolio already imports TO's condition-identity contract; the real gap was executability validation), P1-6's informs-never-gates conflict overstated (that rule is scoped to the tier gradient). All 7 forks user-decided — six on the recommended option, and **P1-1 against recommendation: the user chose to define the discounted target function now**, then picked the **rate-anchored-multiple shape** via a second selector: the **v2 scenario-target function** (per-share driver ladder fwd EPS → FCF/sh → rev/sh, consensus low/mid/high × `DGS10` spread-anchored P25/50/75 multiples w/ ε / min-obs guards; TR adds forward dividends = the dead-money decomposition as one function; TO archetype driver overrides; fund-form stays the open item). Also settled: Portfolio risk-tier assignment (TO rule adopted w/ missing-input rule + fund mapping, Step 6b assigns); the re-check-class **resolution contract** (TO 3c canonical — filing + leading-metric claims validated at 5h, Portfolio 6g rides it); **rate-anchor hard-fail** pre-per-item (quick paths cached-print ≤ ~1 week); TO research cache = **document-level** (URL-keyed immutable vintage, searches always live; storage home + portability); the **evidence-floor freshness basis** (`fresh`/`stale`/`freshness-unscorable`); the **archive-price third maintenance population** (job-time owner); watchlist cap eviction (**lowest-score-first**, `capacity-evicted` retirement episode). Fixes: 9 docs + an `engine.rs` docs-lead-code comment + the round-7 ledger table (counter → run 8) + 10 INDEX rows; BUILD.md §What remains now names the v2 function + tier assignment inside the fund slice's engine update. Verified: 1,274 links / 0 broken, `cargo test` 633 passed, clippy clean (frontend untouched).

## Current state

`main` @ `4bd0f40` plus this handoff commit, pushed; tree clean apart from the untracked local `codex-review.md`. Nothing in flight. Queue head unchanged: **the fund-slice plan** (`/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility`) — its engine update now also carries the round-7-settled v2 target function and per-branch tier assignment, alongside the two named code prerequisites (ticker→CIK resolver, holdings book-level netting).

## Open questions

- **Fund-form scenario-target methodology (blocking)** — the fund-slice plan's first decision; must compose with the v2 function (whose fund gap is exactly the missing per-share driver).
- **Local-suite scorecard display (carried, deferred)** — TO shadow + Portfolio outcome scorecard UI surfaces.
- **Encrypted-archive live round-trip (carried, optional)** — one passphrase export→import before the M5 move.
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data).
- **Keychain fail-soft candidate (carried)** — denied Keychain read blanks local warning categories for the session.
- **Stage-and-swap import hardening (carried)** — named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh + benchmark/futures symbol/adjustment live-verify incl. gold `gc.f` / silver `si.f`; `continuity_weight` bands, thresholds, budgets; the release→event map's FMP strings join the paid-key checkpoint.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.
- **Legacy-docs broken anchor (carried, trivial)** — `legacy_docs/NOTES-FROM-RESTRUCTURING.md:15` → `docs/scheduling.md#job-controls` (heading gone); repair or retire whenever convenient.

## Where to start

Run **`/metis-plan-task` for the fund slice** against `docs/portfolio-analysis.md §Asset eligibility`; settle the **fund-form scenario-target methodology** first (the only remaining blocking input — it must compose with the v2 rate-anchored function). Treat the round-2 through round-7 typed contracts as spec, not open design (round 7 added: the v2 target function + archetype driver overrides, the Portfolio tier assignment, the resolution contract, the rate-anchor hard-fail + rate-cache max age, the document-level research cache, the evidence-floor freshness basis, the archive-price population, the `capacity-evicted` eviction), and account for the two named code prerequisites plus the slice's engine update carrying v2 + tier. If another Codex round runs first, it is run 8, two-pass, triaged through `claude-code-fixes.md` — match by content, not finding ID (round 7's table is now the top section).
