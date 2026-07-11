# Current session handoff

## What happened

**Codex round-3 docs review fully resolved.** All 10 findings in `codex-review.md` were adversarially validated (10 parallel agents, docs *and* code): 7 confirmed, 1 partial (P1-3, "no legal state" overstated), 1 refuted-as-stated (P2-4 — clause tightened anyway), and 1 confirmed **plus a latent code bug** (P1-1: `schwab_live.rs` concatenates positions across granted accounts while `diff.rs` treats symbol as the sole identity — silent last-row-wins for multi-account users). The user decided 8 forks via selector (recommended option on all): **book-level netting per symbol** (docs now; code = named prerequisite of the fund slice, alongside the CIK resolver); Portfolio post-maturity = **no active episode** (falsifier confirmations → thesis ledger); TO maintenance gains a **conditional filing-cadence rider** (earnings row as trigger); DTO/Deep-Audit **resume pin set** (Step-2/3/4 outputs + versions, shared ~48h window; Quick Audit never checkpoints); graph node status **`departed`** (visible tombstone, never a feeder, excluded from shadow scoring); all-outcome **`resolution_mode`** (renamed from loser-scoped "failure mode"); **`EconomicRelease.surprises[]`** per-event vector; Step-16/router prose fixed to P/E-range-paired-with-cumulative-return. Fixes landed across 8 docs + `claude-code-fixes.md` (round-3 disposition table + do-not-re-flag notes + a warning that **finding IDs restart per round** — three IDs collided with unrelated round-2 rows) + INDEX.md (1 row added, 6 amended) + BUILD.md (as-built netting gap + §What remains prerequisites) — commit `6d7e730`, pushed. Verified: 1,142 relative links / 0 broken.

## Current state

`main` @ `6d7e730`, pushed; tree clean apart from the untracked local `codex-review.md`. Nothing in flight. Queue head unchanged: **the fund-slice plan** (`/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility`) — the plan must now also account for the slice's **two named code prerequisites**: the ticker→CIK resolver and the holdings book-level netting step (the netting also fixes the latent multi-account diff bug).

## Open questions

- **Fund-form scenario-target methodology (blocking)** — the fund-slice plan's first decision.
- **Local-suite scorecard display (carried, deferred)** — TO shadow + Portfolio outcome scorecard UI surfaces.
- **Encrypted-archive live round-trip (carried, optional)** — one passphrase export→import before the M5 move.
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data).
- **Keychain fail-soft candidate (carried)** — denied Keychain read blanks local warning categories for the session.
- **Stage-and-swap import hardening (carried)** — named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh + benchmark/futures symbol/adjustment live-verify, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Run **`/metis-plan-task` for the fund slice** against `docs/portfolio-analysis.md §Asset eligibility`; settle the **fund-form scenario-target methodology** first. Treat the round-2 and round-3 typed contracts as spec, not open design, and account for the two named code prerequisites (CIK resolver, holdings netting). If another Codex round runs first, it triages through `claude-code-fixes.md` — and match by content, not finding ID (IDs restart every round).
