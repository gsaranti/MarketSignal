# Current session handoff

## What happened

**Attempt 6 launched and user-ended at 6 of 47; the local model calls are now the priority** (2026-09-15).
The bring-up ran clean from a Terminal-direct session (ancestry printed; Desktop grant held; fresh binary confirmed) and the run confirmed the debut stamps `portfolio-v35` / `checkpoint-v9` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4` off the persisted header.
The user re-imported the prod corpus just before launch and the house view and DIA's ledger both read the 2026-09-16 report.
Six holdings completed (TSLA, PSX, SPMO, ARKF, DIA, PGNY, ~21 min each, 0 hard failures, 2 recovered retries, 0 unreconciled topics, 0 unit/overlay exclusions; NIO never reached) before the user cancelled cooperatively at 04:19:48Z (`job_runs` id 6, the first terminal row) because the reasoning-stream reads showed the calls not yet acceptable.
The v35 bundle cut synthesis re-think markers ~65% per holding but left the action call untouched (ENGINE SET clause on 6 of 6) and interpretation bimodal (0–4 markers on four holdings, 20 and 46 on PSX and ARKF).
The persisted ledgers carry implausible or mis-unit quant cores on 3 of 6 holdings, tax leaked into DIA's rationale, and accepted research text carries temporal contradictions.
Record: `docs/verification/2026-09-15-big-run-attempt-6-findings.md` (Claude read grouped needs-fix vs notes, plus a Codex analysis appended and verified — its seven corrections to the Claude read all held).
Work list: `docs/verification/2026-09-15-local-model-call-fixes.md` (25 entries in 8 groups, Codex's review folded in); both pushed (`2805d8e`).
Infrastructure spun down; run logs and store extracts copied to `~/Downloads/market-signal-attempt-6-logs/`.

## Current state

Working tree clean, `main` in sync with `origin/main` after this session-end commit.
Nothing running.
The dev store is NOT the debut: it holds the attempt-6 header, six holding rows, 52 `web_source_state` rows and `job_runs` max 6; the next attempt must re-wipe (drop `web_source_state`, clear checkpoints/holdings; keep reports/vectors/baselines/job_runs).
`portfolio-v35` has now run, so the fix-list changes land as `portfolio-v36`; checkpoint and evidence-floor stamps move where an entry changes a persisted shape (confirm at plan).
Scope and direction are RULED (2026-09-15, recorded in the fix list): first slice = §1 ledger conditions + §2 action packet (one plan, one `portfolio-v36` stamp); the fixed-evidence evaluation gate (§8.1/8.2) is adopted and precedes any book-scale attempt; 2.1 (tax/P&L withheld from the action packet, caveat app-rendered after the rung) and 4.4 (fund fit judgment moves to interpretation) are adopted; the six "not adopted" items are ratified; 8.3's experiment order stands.
Every other open entry in the fix list is ruled as a flag of its slice's plan, not before it.
BUILD §What remains' pre-run bar needs one sentence for the gate — not yet written (user-run).

## Open questions

- Plan flags for the §1+§2 slice, to be ruled on the plan: 1.2's guard form (narrow prose-mismatch), 1.3 margin policy granularity (per series, or per series+vehicle), 1.4's qualifier handling, 2.2/2.3 wording; the later slices carry 3.1 field ownership (a rename moves `checkpoint-v9`), 4.3 no-evidence semantics, 5.2's stamp.
- Whether to add a short erratum under the Claude sections of the attempt-6 record for Codex's seven corrections (record committed as-is).
- ARKF now prices (was `role_risk_only` on attempt 5) under the 2026-09-15 rulings — confirm intended.
- Carried: stop rule 1 (unit-exclusion rate) unread; per-domain denied share and the SearXNG engine set at book scale; the deferred watches listed at the fix list's end.

## Where to start

`/metis-plan-task` the first slice: fix list §1 (ledger conditions) + §2 (action packet) from `docs/verification/2026-09-15-local-model-call-fixes.md`, with §8.1's fixed evidence set (reconstructed packets from the six persisted holdings in the dev store, extracts in `~/Downloads/market-signal-attempt-6-logs/store-extracts/`) as the verification substrate and `cargo test` + clippy named in the verification command; surface every open §1/§2 entry as a plan flag for the user to rule before implement.
Do not launch or propose another run; the next attempt re-wipes the dev store first.
