# Current session handoff

## What happened

**TO strategy additions reviewed, fixed, committed (`12d652c`, pushed) — TO strategy declared frozen.** A user+Codex session (outside Metis) had added six mechanisms across 5 docs: the **`research_target_scenario`** target bridge (sourced claim-id expression trees — no free numeric literals; app validates, engine computes; structured-only counterfactual retained; ~4-week decay), the **`thesis_milestone_plan`** (validated milestone DAG; new `milestone-chain` realization basis — double-over-horizon H = earliest payoff date, floor 3mo; boundary-straddle → Mid), the **archetype-neutral conviction raise** (verified to *converge* with Portfolio's existing invariant — Portfolio already required key-driver linkage, never had an archetype term), **discovery coverage rotation** + persisted coverage ledger (6th TO store, ~4-week calendar window), the **limited-history evidence path** (new-listing / spin-off / new-perimeter only; direct/recast/proxy comparability; gates unchanged; not user-configurable), and the **research-watchlist refresh lane** (drafted 1 research-class node/run, inside the discovery ceiling). This session's independent review confirmed all six hold the suite invariants (engine computes every number, model-proposes/app-validates, cheap-path-never-archives, anti-reflexivity/no-double-count) and surfaced two gaps, both fixed same-session: milestone completion-condition state now keys to its own **`condition_id`** under the falsifier structural-identity/supersession rule (canonical at trade-opportunities.md §The opportunity), and the Step-5f shared-contract sentence names all three TO-only extension legs. BUILD.md + INDEX.md caught up (user-run via Codex; verified accurate). New **`logic-flow-docs/`** (human-readable TO logic flow, outside the contract corpus) now tracked. Verified: 1,379 links / 0 broken, `git diff --check` clean; docs-only diff — cargo/npm gates n/a.

## Current state

`main` at `12d652c`, pushed; tree clean apart from the gitignored local Codex files. Nothing in flight. **Frozen-spec baseline moved from round-10 to `12d652c`** (rounds 2–10 plus the six TO additions). Portfolio-side contracts did **not** move — `research_target_scenario` is deliberately TO-only, and the raise change converged the two jobs' invariants rather than diverging them — so the fund-slice inputs are unchanged. Queue head unchanged: the **fund-slice plan** (`/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility`) — engine update carries the v2 function (round-10-tightened) and per-branch tier assignment; named code prerequisites: ticker→CIK resolver, holdings book-level netting.

## Open questions

- **Fund-form scenario-target methodology (blocking)** — the fund-slice plan's first decision; must compose with the v2 function.
- **TO-additions external review (new, optional)** — the six `12d652c` mechanisms had only this session's in-repo review, no external round; a scoped Codex round 11 remains available if wanted (two-pass, via `claude-code-fixes.md`, match by content).
- **Fraud-producer posture (carried, review-optional)** — research-fed `forensic_event`, tier-0 lineage; override only if a different sourcing posture is wanted.
- **Local-suite scorecard display (carried, deferred)** — TO shadow + Portfolio outcome scorecard UI surfaces.
- **Encrypted-archive live round-trip (carried, optional)** — one passphrase export→import before the M5 move.
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data).
- **Keychain fail-soft candidate (carried)** — denied Keychain read blanks local warning categories for the session.
- **Stage-and-swap import hardening (carried)** — named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh + symbol/adjustment live-verify; `continuity_weight` bands, thresholds, budgets; FMP release→event strings join the paid-key checkpoint; the new coverage-window / refresh-cap drafted defaults join the calibration set.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.
- **Legacy-docs broken anchor (carried, trivial)** — `legacy_docs/NOTES-FROM-RESTRUCTURING.md:15` → `docs/scheduling.md#job-controls` (heading gone).

## Where to start

Run **`/metis-plan-task` for the fund slice** against `docs/portfolio-analysis.md §Asset eligibility`, settling the **fund-form scenario-target methodology** first (the only blocking input). Treat the typed contracts as frozen spec at **`12d652c`** — rounds 2–10 plus the six TO additions (target bridge, milestone plan / `milestone-chain` basis, archetype-neutral raise, coverage ledger, limited-history path, watchlist refresh lane); TO strategy additions are done — no further TO strategy work before implementation. If a Codex round runs despite the freeze, it is round 11, two-pass, triaged through `claude-code-fixes.md` — match by content, not finding ID.
