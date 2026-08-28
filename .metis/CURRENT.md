# Current session handoff

## What happened

**A1–A4 resolved and shipped** (`41ae9ff`, docs only): the four
Priority-3 alignment majors from the 2026-08-24 review. The 6c failure
sentence now states the canonical posture (no partial run row;
checkpointed holdings restore on resume); the interpretation and action
"exact inputs" lists carry every section the code renders, plus the
role-risk branch's own render set; the sub-distillation drop trigger is
the **per-holding budget of pass-level map calls** (`SUB_DISTILLATION_CAP`),
not a reduce-overflow check — the same wording corrected in
`portfolio-analysis.md`, the TO logic-flow doc, and what-the-cap-counts in
`web-research.md` / `configuration.md`. Re-verification found the
**exhausted-budget edge breaks "never the topic's seeded status"**: a topic
whose every pass drops is named unreconciled and its seed deleted
(`distill.rs:806-846`, `job.rs:1889`). **Ruled:** code fix (route the
fully-dropped topic's prior through the reduce as a dormant prior, own
vintage; no `PROMPT_VERSION` event) **queued behind the big run**; docs
state the edge as built; the watch set reads the run's gaps for
`dropped at the sub-distillation cap`. Three Codex rounds added three
previously undocumented fail-soft postures to the canonical docs —
checkpoint writes, the unreconciled-seed delete, the resume loader's
unreadable-row skip — and bounded the twice-rendered note to a prior
verdict. Record: review record §A1–§A4 (resolved 2026-08-27).

## Current state

Nothing in flight; `main` at `41ae9ff`, tree clean, pushed. Pre-run list
complete; the **big confirmation run remains unblocked**. Behind the run,
severity order: the **flag-2 seed fix** (promoted by any cap hit), the
priority-1 minors, Codex's I1–I9 (unverified by a Claude session).
Carried untouched: `/api/tags` probes on the 600 s backstop; seed passes
the whole prior ledger per topic (doc↔code drift vs
`portfolio-workflow.md` §Step 6c); 6g qualitative trips un-trip unless
re-researched.

## Open questions

- **When to launch the big run** — nothing blocks it; user-launched.
- **Docs register ruling** — Codex rounds 2–3 pulled on whether every
  mirror must restate a fail-soft store-write posture; the corpus has many
  unqualified "persists" / "is deleted" over fail-soft writes. Candidate
  ruling: mirrors state the rule, postures live once in §Failure posture.
- **Fix grouping** — one-at-a-time vs batching the minors; if the
  accumulator-resume minor is taken, retry events and prompt usage ride
  `CheckpointAccumulators` together.
- **One-month band** — unscaled daily vol × 2 marked "v1 mechanics":
  deliberate retention or √t scaling?
- **Core-beside-statement render** (6c seed / 6d citation list) — a
  `PROMPT_VERSION` event needing its own decision.
- **INDEX.md gap** — no §Verification records row for the 2026-08-24
  large-scale review record (user-run edit).
- **Untraced minor** — `configuration.md:214` says the distillation shape
  and tier count reach the audit; `DistilledResearch.shape` flows to
  `pipeline.rs:332`, not followed to a persisted field.
- Carried: runtime auto-start/spin-down; the 6e supersede leg
  structurally dead; channel promotion criteria; research budgets
  calibrate on the run.

## Where to start

`/metis-session-start`, then either launch the **big confirmation run**
(user-run; read `data-health` early — fired-retry events and
sub-distillation-cap gaps are both zero on the healthy read) or, if not
running yet, `/metis-plan-task` a priority-1 minor. Take the flag-2 seed
fix only on a cap hit or a user promotion. Keep the loop: plan →
implement → review → Codex → commit, and mark the finding in the record.
