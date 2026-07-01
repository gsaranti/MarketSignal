# Current session handoff

## What happened

A **tuning session**: context-window analysis of the Market Signal report job →
raised the report-job token/timeout/audit budgets, **shipped to `main`** in two
commits — `65a9fa6` (functional) + `50d6ffc` (doc-comment reconciliation).
Working tree clean; `cargo test` + `clippy --all-targets --all-features` green
(555 pass / 22 ignored-and-skipped → no live spend). Codex-reviewed: no functional
issues; I additionally fixed a stale module-header comment it missed.

**What changed (`65a9fa6`):** main-agent `*_MAX_TOKENS` 24k→32k, analyst 16k→20k,
streaming **total** HTTP timeout 300→600s (`model_agent.rs`),
`RECENT_REPORT_BODY_CAP` 12k→20k chars **per report ×3** (`pipeline.rs`). The three
limits move together — output caps (thinking+body share `max_tokens`) and the total
streaming timeout must, or a long report trades a `max_tokens` truncation for an HTTP
timeout. Router (4096/120s) + headline filter (8192/120s) deliberately left as-is.
(Detail in memory `token-budget-tuning.md`.)

**Settled (don't re-derive):** both analyst arms **stream** (OpenAI on the Responses
API via the shared SSE reader — the old "OpenAI arm non-streaming" header was stale,
now fixed). The streaming `.timeout()` is a **total** request timeout → 600s ≈ ~18k
Opus tokens at ~55–65 tok/s.

## Current state

On `main` @ `50d6ffc`, pushed, in sync. Changes are **offline-verified only** —
behavior-preserving, but no live run has exercised the 32k cap / 600s timeout / 20k
audit cap yet. **NEXT is a release:** bump app version 1.2.0→**1.2.1** + new build
carrying this tuning (user's call — "if good to go"). Last session's
`local-model-operations` doc work is done + M5-gated (memory
`local-model-operational-reference.md`); build order otherwise unchanged.

## Open questions

- **NEW — live-calibration of the 32k/600s values:** the next long-cadence report is
  the real check — does it stay under 600s and not truncate? If it nears the ceiling,
  revisit (raise timeout, or the flagged `read_timeout` idle-timeout refactor across
  the 3 streaming client builders). Normal daily runs (~16k tokens) sit well inside.
- **local-model runtime pre-flight (M5, carried):** does the 122B load, on which
  backend (MLX vs Metal/GGUF `mmproj`); is #14645 fixed (else keep `format` calls
  thinking-on); does `format` constrain; set `num_ctx`; measure throughput. Checklist
  in `docs/local-model-operations.md`.
- **M5-calibration (carried):** Stooq 8 PM-ET / 24h refresh, ~4wk `continuity_weight`
  bands + Research-stale threshold, tripwires, DTO deep-research budget, leftover-budget
  oldest-N ordering, archive retention 100 + upside-exhausted threshold.
- **Four-part verdict model + bidirectional-conviction bound (carried):** lands when
  full Portfolio + TO are built.
- **§1 genuinely-open drafts (carried):** dead-money hurdle, feasible-set bounding; TO
  risk-tier / horizon / hypothesis-score / quota / gate tables.
- **M5-gated backlog (carried):** web-research provisioning / gating / UI +
  rendered-retrieval, analytical-register live-check, no new Tavily, FMP-tier.

## Where to start

**Bump app version 1.2.0→1.2.1 + new build**, carrying the token-budget tuning over
v1.2.0 (`ed663a6`). Follow memory `release-build-install`: move the **5 version
anchors** together, tag the **release-tip** commit (not a trailing metis handoff).
Recommend **one live daily report on this code first** (user runs daily anyway) to
confirm the caps/timeout behave live before cutting the tag — low-risk either way.
After the release, the **live Schwab OAuth slice** remains the next build-order step
(`schwab-integration.md` audited clean; check code-vs-doc — PR #45 has the MVP engine
math).
