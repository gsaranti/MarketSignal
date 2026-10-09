# Current session handoff

## Active task

The self-review — next up; nothing started.

## What happened

Outcome learning landed in one task (`1254586`, pushed) and the item is complete.
The episode store is the price record: `portfolio_episodes` plus the checks written onto them once each in `portfolio_episode_checks`, both insert-only; store init drops the retired decision-episode table, so every store started fresh.
`outcome.rs` is a pure core with no Portfolio types, so Trade Opportunities reuses it.
The accuracy pass runs at Step 5 on both entry paths: a horizon is due strictly after its date; each symbol takes one fresh dated-EOD refresh through the price-bar cache; an empty serve is unscorable at once; a store error is fail-soft (`DataHealth.accuracy_gap`).
Step 7 opens episodes inside the persist, and `PortfolioRun.accuracy` carries the scores the card renders.
The job no longer touches vector memory, and the gate needs the reasoner alone.
Stamps: archive 20 only.
The Metis reviewer rejected once (an empty served series read as a failed refresh), then approved; Codex approved with no findings.
The 20 plan rulings and the review round's 4 are in the commit message.

## Current state

Nothing in flight.
Outcome learning is in Built; the self-review is next and unplanned.
The accuracy record it reads comes off `run_accuracy_pass` in `job.rs` (`AccuracyPass`), today consumed only at Step 7 — the loop does not see it yet.
Each score carries its last-moved check id (run order), but no analysis record carries the check high-water mark the read-by-the-prior-analysis word needs; that is the self-review's to add, with its stamps.
The dev-run batch (every Portfolio item's dev run) waits behind the self-review and the removal sweep, ahead of the big run, which is the user's call.
Residue carried (do not re-raise): `GradeBranch` and `TargetHorizons::label` sit under `allow(dead_code)` for the self-review; `embedding::LocalEmbedder` has no production caller and goes with the sweep's embedding path (the namespaces, `prune_runs`' vector cleanup, the Settings embedder-change wipe, the roster field, the archive's embedders manifest); `ResearchSeed` / `news_seeds` name the leads until the sweep; `engine::refine_targets_with_assumption` and pre_profit's evidenced overlay path park for the sweep; the hand-rolled retry gate in `LocalAnalyst::distill` mirrors `RetryOnce::run` by shape and must move with it; the harness fixtures carry no quarterly, balance, score or target-stamp rows, and the live harness's TSLA header still reads "(name unavailable)" until the fixed-evidence set is rebuilt from a post-redesign run, after the dev-run batch; the fixed-evidence lexicon scan reads provider-served words as app prose; data-portability.md §Import flow says v2–v10 are refused where the code refuses v2–v11.
Every stamp bump touches `prompt_version_is_stamped_for_the_model_arm_domain_gate` in `portfolio/pipeline.rs`.
The offscreen card-render recipe lives in session memory, not the repo.

## Open questions

None.

## Where to start

`/metis-session-start`, then `/metis-plan-task` for the self-review: read portfolio-workflow.md §Step 6e and §Step 6b, and portfolio-analysis.md §The per-holding pipeline, §Outcome learning (the accuracy record's reach into the review) and §Starting parameters first; the realized block is `engine::RealizedEngineData`; how the review knows which checks the prior analysis read is a plan-time flag; every flag and assumption through the selector.
Never propose the run.
