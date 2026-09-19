# Current session handoff

## Active task

The single big confirmation run

## What happened

All pre-attempt-7 findings are landed and committed — entries 4–8 and 11, and entry 3 pulled forward last (`d7fa5dd`): claims now carry separate retrieval time, reported publication metadata, and source-stated fact period through research, consolidation, reuse, and persistence (`portfolio-v47` / `checkpoint-v14` / portability format 10).
Each Codex-built slice was independently reviewed against the code with its full gate re-run green, and no pre-release backward-compat was introduced (serde defaults confined to provider/model wire JSON; retired checkpoint/archive shapes refused, not migrated).
This session then, at the user's request, **re-wiped the dev store to a clean debut ahead of attempt 7** — so the store no longer holds attempt 6 and needs no further wipe at bring-up.

## Current state

No implementation task is in flight; the confirmation-run BUILD item stays incomplete, its live acceptance pending the user-named launch session.
BUILD is unchanged — these prerequisites are not separate BUILD items.
Debut stamps: `portfolio-v47` / `checkpoint-v14` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`; portability format 10; the next prompt/output-schema change is `portfolio-v48`.
The attempt-7 acceptance/readout protocol is entry 11 of `docs/verification/2026-09-17-open-findings.md`, whose §Verification-on-attempt-7 carries every landed entry's checks (measurement semantics single-homed in `docs/local-models.md`; the claim-date contract in `docs/portfolio-analysis.md` under research reuse — different periods stay distinct observations, an explicit same-period revision supersedes, later publication alone never resolves a conflict, unknown/incomparable periods retain uncertainty).
Acceptance = every applicable correctness check passes + whole-book completion with no failed holding; justified not-rated/insufficient-evidence outcomes are reported separately, unexercised checks stay unverified, and elapsed time has no pass/fail limit.
The dev store is now a clean debut (re-wiped this session): `portfolio_runs` / `portfolio_checkpoints` / `portfolio_checkpoint_holdings` / `portfolio_research_seeds` / `portfolio_outcome_episodes` / `portfolio_quick_checks` / `holdings_pulls` / `price_bars` / `web_documents` cleared, `web_source_state` **dropped** (recreated by `init_schema` on next dev-app start), the `portfolio` vector-memory namespace cleared.
Continuity kept — 30 reports / 14 baselines / 69 report vectors / `job_runs` max id 6 (attempt 7 → id 7) / 16 `app_settings`; the prod store is untouched and `PRAGMA integrity_check` is `ok`; attempt-6 residue is archived at `~/Downloads/market-signal-attempt-6-logs/`.
Entries 9 and 10 (sampling-profile comparison; non-thinking experiments) follow attempt 7 and need a rebuilt harness; the conditions-display slice remains separate and unscheduled (`conditions` still typed `unknown[]`).
Known stale SYNTHESIS scheduling, pipeline-length, and LanceDB claims remain unreconciled.

## Open questions

None — the retrieval-age and model-reconciliation rulings (entry 3) and the acceptance protocol (entry 11) are captured in code and canonical docs.

## Where to start

When the user names the launch session, the dev store is already a clean debut, so bring-up goes straight to the store pre-check (expect the clean-debut counts with `web_source_state` absent until the fresh dev app recreates it), then Ollama / OrbStack / SearXNG+Serper per the usual bring-up.
Read entry 11's protocol and the findings doc's §Verification-on-attempt-7, including entry 3's live factual-dating and conflict checks.
This handoff authorizes no live read or harness run: attempt 7 launches only when the user names the session — do not propose it.
