# Current session handoff

## What happened

**Codex round-8 docs review fully resolved** (fixes in `3094daa`, pushed). All 8 findings adversarially validated (8 parallel agents): 7 confirmed + 1 partial (P2-5, a stale round-7 leftover clause), zero refutations. Validation widened P1-1 (the v2 guards' raw-multiple fallback flips the percentile mapping **back to direct** — a mirror bug Codex missed), narrowed P2-2/P2-5, refuted P2-3's shadow-ledger sub-claim, and extended P1-3 and P2-3 to twins Codex's scope missed (ATO Quick Audit; Portfolio's caps). All 4 forks user-decided, all on the recommended option: **FCF rung dropped** (ladder = fwd EPS → fwd rev/sh, finite-positive eligibility, one diluted-share basis, `no-admissible-driver` floor reason); **quick paths recompute with fresh `DGS10`** (closed-form re-anchor on stored percentiles/drivers, both quick paths; ledger band stays frozen); **categorical Medium conviction ceiling** (soft triggers cap, hard exclude, ceiling binds *after* the raise — `final = min(base + raise, ceiling)`, correcting Codex's own formula); **typed `runway_evidence` + typed catalyst claim + derived realization basis** (never model-picked). Also: explicit inverse spread mapping (`spread_bear = P75`), Step-5a classification prefetch (5b reuses cache), total low-confidence archetype branch (Step-4 provisional fallback), event-route clause rewritten to schedule-speculatively. Fixes span 6 docs + the round-8 ledger table (counter → run 9) + 5 INDEX rows (3 updated, 2 new). Verified: 1,310 links / 0 broken, `cargo test` 633 passed, clippy clean, frontend untouched. BUILD.md checked — no update needed (round 8 tightened per-feature contracts below its altitude; its v2/tier lines stay accurate).

## Current state

`main` @ `3094daa` plus this handoff commit, pushed; tree clean apart from the untracked local `codex-review.md`. Nothing in flight. Queue head unchanged: **the fund-slice plan** (`/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility`) — its engine update carries the v2 function (now round-8-tightened) and per-branch tier assignment, plus the two named code prerequisites (ticker→CIK resolver, holdings book-level netting).

## Open questions

- **Fund-form scenario-target methodology (blocking)** — the fund-slice plan's first decision; must compose with the v2 function (fund gap = the missing per-share driver).
- **Local-suite scorecard display (carried, deferred)** — TO shadow + Portfolio outcome scorecard UI surfaces.
- **Encrypted-archive live round-trip (carried, optional)** — one passphrase export→import before the M5 move.
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data).
- **Keychain fail-soft candidate (carried)** — denied Keychain read blanks local warning categories for the session.
- **Stage-and-swap import hardening (carried)** — named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh + symbol/adjustment live-verify; `continuity_weight` bands, thresholds, budgets; FMP release→event strings join the paid-key checkpoint.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.
- **Legacy-docs broken anchor (carried, trivial)** — `legacy_docs/NOTES-FROM-RESTRUCTURING.md:15` → `docs/scheduling.md#job-controls` (heading gone).

## Where to start

Run **`/metis-plan-task` for the fund slice** against `docs/portfolio-analysis.md §Asset eligibility`, settling the **fund-form scenario-target methodology** first (the only blocking input). Treat the round-2 through round-8 typed contracts as spec, not open design (round 8 added: the inverse spread mapping + direct-mapping fallback, the EPS→rev/sh ladder eligibility contract + `no-admissible-driver`, the quick-path fresh-DGS10 re-anchor, the Medium conviction ceiling binding after the raise, the typed catalyst/`runway_evidence` carriers + derived basis, the 5a classification prefetch, the total low-confidence archetype branch). If another Codex round runs first, it is run 9, two-pass, triaged through `claude-code-fixes.md` — match by content, not finding ID (round 8's table is now the top section).
