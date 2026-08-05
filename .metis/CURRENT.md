# Current session handoff

## What happened

**Pre-big-run review piece 1 (of 3) COMPLETE** — every finding in `docs/verification/2026-07-31-first-live-portfolio-run.md` verified handled in code with file:line evidence (four parallel verification passes over F1–F10 + the nine follow-up candidates; all confirmed landed, including the doc amendments).
Codex ran the same piece and surfaced **one real Medium**: key-first FMP fallback chains where a present-but-null preferred key suppressed a valid legacy value — against the codebase's own declared numeric-first convention — plus a missing fiscal-period dedup in the NTM consensus selector (duplicate same-date rows could blend one year at both weights and stamp `periods_used=2`).
Confirmed and **fixed, `6ee69a2` direct to main**: numeric-first at **five** sites (Codex cited three; the review added `dividend_history_from_value` — where the null-adj row took the whole-read bail path — and fund `aum`, where live serves only the fallback key), `dedup_by_key` after the forward sort, six pinning tests, and the stale `StreamRole` "mirror" comment corrected to "superset".
One internal observation (`data_health: None` off the full-pass path) was Codex-refuted — the cited sites are `#[cfg(test)]` fixtures — and retracted.
Verified: cargo 897 lib + integration / 0 fail, clippy 0, npm build, 40 node + 202 vitest. Codex approved the fix round.

## Current state

**No capture debt** — the fix is adapter-internal (numeric-first was already the stated convention; no doc contract moved), so BUILD/INDEX deliberately untouched.
Queue: **review pieces 2 and 3, then the single big confirmation run.**
Piece 2 = code-vs-docs conformance walk of the Portfolio Analysis job (flag divergences, propose per-divergence verdicts batched for user decision, doc edits follow single-home discipline).
Piece 3 = correctness walk of the deterministic value chain (statement canonicalization → TTM basis → sub-scores → targets/hurdle → feasible set → construction merge → outcome labels) hunting bugs that degrade analysis.
Each piece runs in its own session by user decision.

## Open questions

- **New big-run watches (construction leg)** — lean-divergence / engine-bar / carried-stale-lean rates at 47-position scale; construction-prompt fit in the shared 131k `num_ctx` (settled: compress digests, never `num_ctx`); overlay classification against real Schwab OCC rows; the 7b sizing-only decided-range movement rate that would justify a band-relative episode trigger. Live 122B construction behavior is wholly unexercised.
- **Research-loop activation obligation** — identity + source-text validation + period normalization before the pre-profit producer activates.
- **Standing** — unchanged carried list: live-run calibration watches (STI-reads-zero, YoY contiguity, outcome-leg watches), no A letters under grade-v2, big-run checklist, reasoning-pane DOM weight, encrypted portability round-trip, step-17 embedding, 600 s stress, scorecard display, dev-store residue, Keychain fail-soft, stage-and-swap import, chain both-maps invariant, four-part verdict bound, §1 open drafts, fraud-producer posture, fund-slice drafted constants, checkpoint/resume + the 6g input-delta validator (still gates the outcome slice's dormant legs).

## Where to start

**Run review piece 2** — fresh session, walk the Portfolio Analysis code's logical flow against the docs' documented logic (`portfolio-analysis.md`, `portfolio-workflow.md`, and the storage/interface/data-sources sections it touches); flag every divergence with a proposed verdict (code right vs docs right) and batch for user decision before editing either side.
After piece 2: piece 3 (correctness walk), then the big confirmation run per the locked plan.
