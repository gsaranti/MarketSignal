# Current session handoff

## What happened

A model-selection review session, no code. Discussed what qualities the local reasoner actually needs (contract discipline over generic reasoning benchmarks; long-context faithfulness; groundedness; world-knowledge breadth; tool use; throughput as a quality multiplier), then ran an adversarially-verified deep-research web survey (2026-07-07) on whether `Qwen3.5-122B-A10B` still leads the ~80–130B class. **Verdict: keep** — no challenger survived verification; GLM-Air-class disqualified on *Ollama serving fidelity*, with official-library GLM-5/5.1 named the future re-benchmark candidate. Status moves: **the #14645 fix merged upstream 2026-07-07** (PR #15901) but sits in **no tagged release** (latest v0.31.2 is one day older) — the thinking-on `format` rule stands, and an uncorroborated `think:true`+`tools` format-ignored mode needs its own repro; the **mmproj failure is scoped to imported GGUFs** — an official-library text-only pull is clean; **still no 122B MLX** through v0.31.2; memory fit confirmed (~84–89 GB incl. KV + embedder); **no published RULER data exists** → an in-house long-context probe joined the M5 pre-flight; new rule: pin the Ollama version, upgrades are re-verification events. All folded into `docs/local-model-operations.md` with dated `[verified 2026-07-07]` tags — committed + pushed **`bb10761`** (docs-only, no build gate).

## Current state

On `main` @ `bb10761`, pushed, clean tree, nothing mid-implementation. Eight slices still banked on `main` (#48/#49/#50/#51, `0645351`, #52, `d488416`, `1e04cb8`); installed app stays v1.2.1 by decision — **no new version/build until Portfolio Analysis and Trade Opportunities are both complete**. Unverified detail carried: the table-head glyph hit-target fix is CSS-only — confirm with one click next time the dev app is open. Carried #52 deferrals unchanged: sidebar Portfolio-runs history, portfolio-specific tracker layout, unpersisted card fields, and the local-models warning still lacking its in-app clear path (Local-analysis-models Settings section unbuilt).

## Open questions

- **First post-v0.31.2 Ollama release (new)** — does it ship the #14645 fix (PR #15901), and does that fix also cover the reported `think:true`+`tools` format-ignored mode? Check before pinning the M5 version.
- **Chain both-maps invariant unconfirmed (carried)** — `/chains` still unexercised live; tighten the drift guard to either-absent once a live response confirms both maps present.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s or truncate.
- **local-model M5 pre-flight (carried; refreshed 2026-07-07)** — 122B load/backend, #14645-fix-shipped check, `num_ctx`, throughput, plus the new in-house long-context probe.
- **M5-calibration (carried)** — Stooq refresh, `continuity_weight` bands, Research-stale threshold, tripwires, DTO budget, leftover-budget ordering, archive retention.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Unchanged: **no interim build** — the next step is the next slice via `/metis-plan-task`: the **Local analysis models Settings section** (still the most user-visible gap), the **sidebar Portfolio-runs history**, or **full Portfolio (funds)** — then Trade Opportunities. (For the eventual suite-complete build: launch-time local-suite warnings are the proactive render working, not a regression; click a table-head glyph to confirm the CSS hit-target fix live.)
