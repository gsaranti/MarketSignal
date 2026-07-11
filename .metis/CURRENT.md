# Current session handoff

## What happened

**The Codex full-corpus docs round landed and Pass 1 was fully processed.** `iris-codex-last.md` came back with 29 findings (18 High / 11 Medium): Pass 1 = P1–P12 (Portfolio + shared local-suite contracts), Pass 2 = T1–T10 (Trade Opportunities), Pass 3 = R1–R7 (market report) plus one non-counted breadth clarification. Worked one finding at a time — each verified against the docs before agreeing, each fix user-approved before applying. **All 12 Pass-1 findings confirmed and fixed** (P3 reframed for build status: portability is built, the suite stores are designed-not-built, so the real defect was the "automatically covers" promise vs the closed five-entry import — fixed as a binding format-extension rule + versioned import entry-set). The design-bearing calls, beyond spec repair: insufficient-evidence exits at Step 6b with full exit-state semantics (P1); **resume is its own entry path** pinning the run's snapshot/context/versions, drafted 48h window (P2); a deterministic **listing-resolution guard** on stocks (P4); **app-enforced numeric equality** for engine-owned values in model schemas (P5, canonical in local-models.md); per-job Schwab chain lifecycle + TO Step-8 fresh fail-soft holdings pull (P6); research agendas **orchestrator-assembled**, "one call per topic" = one isolated conversation (P7); **investor profile removed from the intrinsic path** — profile-independence declared (P9); scheduling failure classification delegated to owning workflows (P11); a shared **embedding-response validator** (P12).

## Current state

**Tree is DIRTY** — the Pass-1 fixes are uncommitted, spanning 11 docs (portfolio-analysis, portfolio-workflow, trade-opportunities-workflow, data-portability, storage, data-sources, schwab-integration, local-models, web-research, configuration, scheduling). Commit as one Pass-1 fix batch after an **anchor sweep** (many new cross-links, e.g. `#the-local-model-adapter-seam`, `#failure-posture`, `#step-8-holdings-cross-reference`, `#offline-behavior`). **T1 is verified CONFIRMED with a pending, unapplied 3-edit fix**: (1) scope TO-workflow Step 6 to survivor-set assembly — the matrix is final only after Step 7; (2) Step 7's cheap-sweep bullet gains the deterministic risk tier (aligning with trade-opportunities.md:176, which refreshes it while the workflow omits it) plus a **final-assembly contract** — re-place carried still-valid names by refreshed tier/horizon, deterministic in-cell insertion by frozen conviction, completeness re-validation over the union of survivors + carried; (3) one clause at trade-opportunities.md:176: a refreshed tier re-places the card in its new cell. T2–T10, R1–R7, and the non-counted breadth clarification are unprocessed. Fund-slice planning stays queued behind finishing the Codex round.

## Open questions

- **Fund-form scenario-target methodology (blocking)** — what a *priced* fund's scenario targets derive from; the fund-slice plan's first decision.
- **Local-suite scorecard display (carried, deferred)** — whether the TO shadow scorecard and Portfolio outcome scorecard get UI surfaces.
- **Encrypted-archive live round-trip (carried, optional)** — one passphrase export→import before the M5 move.
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data).
- **Keychain fail-soft candidate (carried)** — a denied Keychain read blanks the local warning categories for the session.
- **Stage-and-swap import hardening (carried)** — mid-import I/O failure can leave partial files; named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage; check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Anchor-sweep and commit the Pass-1 fix batch if the wrap-up didn't. Then resume the Codex round in `iris-codex-last.md`, one finding at a time, verifying each against the docs and applying only user-approved fixes: first the pending **T1** proposal (Current state, above), then **T2–T10**, then **R1–R7** + the non-counted breadth clarification (note R7 is P12's report-side twin — home its validator at report Step 4). Then `/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility` (fund path).
