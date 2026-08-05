# Current session handoff

## What happened

**The FMP rate-limit hardening is COMPLETE** — planned with nine plan-time user rulings, built, internal review **approve** (no nits, first pass), one Codex round adopted (test-only: a stateful mid-backoff abort test pinning the chunked polling), committed `94e31b1`, BUILD/INDEX aligned `418afb6`, pushed.
The load-bearing facts: `http_retry` gained per-provider `RetryPolicy` (429 vs 5xx status-class schedules, shared attempt counter, per-policy Retry-After cap, ~250ms cancel-chunked abortable sleeps — abort returns the last attempt, the callers' `is_cancelled()` boundary checks route it); `FMP_RETRY` = the minute-crossing 429 ladder (1→32s doubling, 63s cumulative over 7 attempts, test-pinned >60s; 5xx stays short; 90s Retry-After cap) wired at both FMP chokepoints, quick check inheriting; `DEFAULT_RETRY` preserves legacy behavior byte-identically for all eleven non-FMP call sites; the Settings connection test stays single-shot deliberately.
**Probe evidence** (one-shot live burst, kept as the `#[ignore]`d `fmp_minute_limit_probe`): the paid per-minute limit arrives as **HTTP 429** ("Limit Reach" body, no Retry-After) and **tripped at call #61 in 2.1s** — a burst bucket well under the headline 200/min, so the suite's burst paths (quick-check sweep, outcome price refresh) will genuinely engage the ladder. The 200-body classification was correctly not built; `interpret_response`'s fatal doctrine stands.
Earlier this session, B10+B13 closed (`1bc21d2`) — the B-ruling build queue is empty.

## Current state

Queue unchanged: **review piece 3 (own session) → the big confirmation run.** No build items remain ahead of the run.

## Open questions

- **Big-run watches from B3** — (1) slash-notation class shares (`BRK/B`) read Unresolved → not-rated under the verbatim FMP lookup; (2) ticker-noise descriptions ("NTDOF COM") risk a false Conflict.
- **Big-run watches (construction leg)** — unchanged: lean-divergence / engine-bar / carried-stale-lean rates, construction-prompt fit (instrumented), overlay classification vs real OCC rows, 7b decided-range movement rate; the run banks B7's profile framing, the card-render watch covers the monitor strip + Setup caption, and the data-health read now also shows FMP 429-ladder engagements on the burst paths (ruled: no separate watch entry).
- **Research-loop activation obligation** — identity + source-text validation + period normalization before the pre-profit producer activates.
- **Standing** — unchanged carried list: live-run calibration watches (STI-reads-zero, YoY contiguity, outcome-leg watches), no A letters under grade-v2, big-run checklist, reasoning-pane DOM weight, encrypted portability round-trip, step-17 embedding, 600 s stress, scorecard display, dev-store residue, Keychain fail-soft, stage-and-swap import, chain both-maps invariant, four-part verdict bound, §1 open drafts, fraud-producer posture + the `hard_forensic_bar` consumer seam, fund-slice drafted constants, checkpoint/resume + the 6g input-delta validator.

## Where to start

**Review piece 3 — the value-chain correctness walk — in its own session** (the last pre-run item; a review, not a build). Then the single big confirmation run per the locked plan, in the dev app (process name `market-signal`), reading the data-health deep-history line for the Stooq-PoW / rung-order question and the new FMP ladder engagements.
