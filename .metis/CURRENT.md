# Current session handoff

## What happened

**Both local-suite strategy-addition rounds reviewed, fixed, committed, and frozen — TO at `12d652c`, Portfolio Analysis at `8313b39` (both pushed).** Each round followed the same shape: a user+Codex session (outside Metis) authored the additions, this session independently reviewed them against the diffs, flagged gaps, and Codex fixed them same-session before commit. **TO (`12d652c`)**: six mechanisms — the `research_target_scenario` claim-id expression-tree target bridge (structured-only counterfactual retained), the `thesis_milestone_plan` DAG + `milestone-chain` realization basis, the archetype-neutral conviction raise (verified convergent with Portfolio's invariant), discovery coverage rotation + ledger (6th TO store), the limited-history evidence path, and the research-watchlist refresh lane; review gaps fixed = milestone condition-state `condition_id` supersession rule + the Step-5f contract sentence. **Portfolio (`8313b39`)**: two mechanisms — the **held-name research refresh lane** (Step-6 *pre-loop*, never the engine-only quick check; drafted cap 2/run, fixed; typed `material_update`/`no_material_change`/`unscorable`; a validated update only invalidates reuse / force-includes the normal full pass) and the **pre-profit execution/financing overlay** (deterministic statement-derived eligibility; engine-computed runway/margin/capex/dilution + app-validated typed operating observations; repeated miss → Medium cap, constrained runway → add bar, conjunctive severe state → Low cap + `{trim, sell all}` lean; grade untouched); review gaps fixed = the `late-invalidated` slot rule (6b pre-flag timing), the bounded latest-four-period cold-start backfill, and the `polarity` field closing a latent cost-guidance miss bug. A Portfolio scenario bridge was **deliberately deferred** pending frozen-date replay evidence. Both rounds: human logic-flow docs added under `logic-flow-docs/`, BUILD.md/INDEX.md caught up (user-run via Codex, verified accurate), links checked (1,379 then 1,387 / 0 broken), `git diff --check` clean, docs-only.

## Current state

`main` at `8313b39` plus this handoff commit, pushed; tree clean apart from the gitignored local Codex files. Nothing in flight. **Frozen-spec baseline = `8313b39`** (rounds 2–10 + six TO additions + two Portfolio additions); both jobs' strategies are declared done pre-implementation. The Portfolio additions are **stock-scoped** (funds and `role_risk_only` excluded), so the fund-slice inputs are unchanged. Queue head unchanged: the **fund-slice plan** (`/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility`) — engine update carries the v2 function (round-10-tightened) and per-branch tier assignment; named code prerequisites: ticker→CIK resolver, holdings book-level netting.

## Open questions

- **Fund-form scenario-target methodology (blocking)** — the fund-slice plan's first decision; must compose with the v2 function.
- **Strategy-additions external review (optional)** — the `12d652c` + `8313b39` mechanisms had only in-session review, no external round; a scoped Codex round 11 remains available (two-pass, via `claude-code-fixes.md`, match by content).
- **Fraud-producer posture (carried, review-optional)** — research-fed `forensic_event`, tier-0 lineage; override only if a different sourcing posture is wanted.
- **Local-suite scorecard display (carried, deferred)** — TO shadow + Portfolio outcome scorecard UI surfaces.
- **Encrypted-archive live round-trip (carried, optional)** — one passphrase export→import before the M5 move.
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data).
- **Keychain fail-soft candidate (carried)** — denied Keychain read blanks local warning categories for the session.
- **Stage-and-swap import hardening (carried)** — named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh + symbol/adjustment live-verify; `continuity_weight` bands, thresholds, budgets; FMP release→event strings join the paid-key checkpoint; the new drafted constants join the calibration set (TO coverage window + watchlist-refresh cap; Portfolio held-name refresh cap + overlay runway bands / 20% material miss / 5pt margin / 15% dilution).
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.
- **Legacy-docs broken anchor (carried, trivial)** — `legacy_docs/NOTES-FROM-RESTRUCTURING.md:15` → `docs/scheduling.md#job-controls` (heading gone).

## Where to start

Run **`/metis-plan-task` for the fund slice** against `docs/portfolio-analysis.md §Asset eligibility`, settling the **fund-form scenario-target methodology** first (the only blocking input). Treat the typed contracts as frozen spec at **`8313b39`** — rounds 2–10, the six TO additions, and the two Portfolio additions (held-name refresh lane, pre-profit overlay — both stock-scoped, so they ride the fund slice's engine update untouched); both strategies are done pre-implementation — no further strategy work before code. If a Codex round runs despite the freeze, it is round 11, two-pass, triaged through `claude-code-fixes.md` — match by content, not finding ID.
