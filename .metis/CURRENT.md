# Current session handoff

## What happened

**Codex round-10 docs review fully resolved as a closure task** (adjudicate + fix only — no eleventh corpus review; fixes committed this session). All 8 findings adjudicated by 8 parallel verification agents against exact text: **4 Accepted, 4 Narrowed, zero refuted** — with two review premises corrected in adjudication (P1-1: restatement/auditor-change were *not* already sourced, so the fix rescoped to the whole hard set; P2-1: storage/§Outcome-learning already read gate-reject-scoped — the sole offender was 5h's every-holdout sentence). Landings: **P1-1** typed `forensic_event` producer single-homed at TO-workflow §5c (filing kinds = item-classified EDGAR 8-K Items 4.01/4.02, CIK-degradable; **fraud = research-fed, tier-0 primary-source-validated — a derived decision, not a user fork**, recorded as deliberate design); **P1-2** DGS10 anchor-window acquisition (one date-ranged FRED request, Portfolio DGS10 row) + the dated join in the v2 bullet (filing-date anchor; latest-on-or-before close/DGS10; failed history → existing raw-percentile fallback) + stored percentiles named in storage's run audit; **P1-3** suite-wide decimal-ratio representation bullet (percent ÷ 100 at the adapter seam, `Npts` = N⁄100, ε = 0.01; report-side untouched); **P1-4** `fresh_clear` broadened (a successful no-new-observation check vouches; union stays 3 states, no storage/force-include ripple); **P2-1** per-class shadow episode content (full vector = `gate-reject` only; abstention = named floor gaps; forensic/`hype` exclusion = gate-reject-class with its tripped trigger); **P2-2** §Why-the-funnel stale duplicate → soft-cap/hard-exclude + pointer; **P2-3** Step-9 learning-embed leg DTO-only (newly-matured-since-prior-DTO, exactly once; ATO Deep embeds touched summaries only); **P2-4** graph honestly **partially bounded** (live picks deliberately uncapped, mirroring the no-output-cap matrix). Fixes span 6 docs + the round-10 ledger (disposition table, constants bullet, 2 deliberate-design lines). Closure-review agent: 6/6 checks PASS, zero introduced defects. Verified: **1,373 links / 0 broken**, `git diff --check` clean, tables valid; docs-only diff — cargo/npm gates n/a. Origin split: P1-2/P1-4/P2-4 follow-ons from rounds 7/2/3, P2-2 a round-8 stale duplicate, the rest pre-existing.

## Current state

`main` at the round-10 fixes commit plus this handoff commit, pushed; tree clean apart from the untracked local `codex-review.md`. Nothing in flight. **User-declared intent: freeze the affected documentation contracts and proceed to implementation — no round 11 planned.** Queue head unchanged: **the fund-slice plan** (`/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility`) — its engine update carries the v2 function (now round-10-tightened: decimal representation, dated anchor join, DGS10 history load) and per-branch tier assignment, plus the two named code prerequisites (ticker→CIK resolver, holdings book-level netting).

## Open questions

- **Fund-form scenario-target methodology (blocking)** — the fund-slice plan's first decision; must compose with the v2 function (fund gap = the missing per-share driver).
- **Fraud-producer posture (new, review-optional)** — round 10 derived it (research-fed `forensic_event`, tier-0 lineage) rather than forking to the user; override only if a different sourcing posture is wanted.
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

Run **`/metis-plan-task` for the fund slice** against `docs/portfolio-analysis.md §Asset eligibility`, settling the **fund-form scenario-target methodology** first (the only blocking input). Treat the round-2 through round-10 typed contracts as frozen spec, not open design (round 10 added: the `forensic_event` producer, the DGS10 anchor-window acquisition + dated join, the suite-wide decimal-ratio representation, the broadened `fresh_clear`, per-class shadow episode content, DTO-only learning embeds, the partially-bounded graph). If another Codex round runs despite the freeze, it is run 11, two-pass, triaged through `claude-code-fixes.md` — match by content, not finding ID (round 10's table is now the top section).
