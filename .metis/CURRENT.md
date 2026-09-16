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
The fix list's 24 open entries await the ruling round; only 8.3 (experiment order: non-thinking synthesis first, then action, after the packet fixes; interpretation stays thinking-on) is ruled.
The standing single-big-run bar is now preceded by a fixed-evidence evaluation gate (fix list §8) if the user ratifies it.

## Open questions

- Ruling round on the fix list — 1.2's guard form (narrow prose-mismatch, not a universal decimal>1 rule), 1.3 app-assigned margins (per series or per series+vehicle), 3.1 `price_target_rationale` ownership (rename moves `checkpoint-v9`), 4.3 whether an app-built no-evidence object counts as answered or a gap, 5.2's stamp (evidence-floor, checkpoint, or both).
- Whether to add a short erratum under the Claude sections of the attempt-6 record for Codex's seven corrections (record committed as-is).
- ARKF now prices (was `role_risk_only` on attempt 5) under the 2026-09-15 rulings — confirm intended.
- Carried: stop rule 1 (unit-exclusion rate) unread; per-domain denied share and the SearXNG engine set at book scale; the deferred watches listed at the fix list's end.

## Where to start

Start with the ruling round on `docs/verification/2026-09-15-local-model-call-fixes.md`: present each open entry as a decision with its recommendation, record `Ruled 2026-09-…:` lines in that file, then `/metis-plan-task` the first slice (fix list §1 ledger conditions and §2 action packet are the highest-consequence groups; §8.1's fixed evidence set from the six persisted holdings is the verification substrate).
Do not launch or propose another run; the next attempt re-wipes the dev store first.
