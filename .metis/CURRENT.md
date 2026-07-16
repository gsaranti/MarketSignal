# Current session handoff

## What happened

**Open-questions triage — no feature slice; two items closed, one advanced, all direct to `main` (`fb56a7e`, `78df109`).**
The legacy-docs broken anchor is fixed (`NOTES-FROM-RESTRUCTURING.md:15` → `scheduling.md#generating-a-report`).
Ollama: v0.32.0/v0.32.1 shipped; the #14645 fix (PR #15901) is verified an ancestor of the v0.32.0 tag; still no 122B MLX through v0.32.1; `docs/local-model-operations.md` re-tagged `[verified 2026-07-16]` — the thinking-on rule stands until the fix is verified to *behave* on the pinned version (≥ v0.32.0) on the M5.
**FMP: the user upgraded the subscription (same key) and the paid-key shape checkpoint ran and CLOSED** (~27 live GETs, `78df109`): audit #8's buckets confirmed live (allowed → 200, blocked → 402, `analyst-estimates` annual-only verbatim); every fund-slice shape assumption held (expense-ratio percent-units `/100`, stable spellings, `etf/info` serves `assetsUnderManagement` with no `aum` key — the fallback covers it, mutual funds served); the `sector-pe-snapshot` holiday-keying concern dissolved (weekday market holidays return full carried snapshots); the release→event map was corrected + live-verified (trailing period-suffix strip then exact base match; `PPI MoM` → `Producer Price Index MoM`; `JOLTs` casing; the drafted `Core PPI MoM` row removed — no live counterpart; NFP unit `K` / JOLTs `M`; CPI alias rows documented inert); the other enrichment shapes (`averageChange` in percentage points, IPO/M&A feeds) verified too.
Details are single-homed in `docs/data-sources.md`, the `fmp.rs` seam comments, and the `78df109` commit message.
Verification green: cargo test 690/0, clippy clean, `npm run build`.

## Current state

Nothing in flight — clean tree on `main` at `78df109`.
The FMP key is now paid-tier: Trade Opportunities implementation will code against live-verified shapes, and the planned report-enrichment slice (`data-sources.md §Planned report enrichment`) is no longer key-blocked — it stays unqueued; sequencing it is the user's call.
`INDEX.md`'s planned-enrichment row still says "pending paid-key verification" (user-run catch-up when convenient).

## Open questions

- **Hurdle × rate-anchored-multiple tightness (M5-calibration)** — the strong test fixture lands dead-money; the bars may bind harder than intended on real names.
- **Fraud-producer posture (carried, review-optional)** — research-fed `forensic_event`, tier-0 lineage.
- **Ollama pin + #14645 behavioral verify (folds into the M5 pre-flight)** — pin ≥ v0.32.0 (the fix ships there); shipping ≠ enforcement given the probabilistic failure, so the schema-integrity check on the pinned version decides when non-thinking distillation unlocks.
- **Local-suite scorecard display (carried, deferred)**; **encrypted-archive live round-trip (carried, optional)**; **dev-app sanity residue (carried)**; **Keychain fail-soft candidate (carried)**; **stage-and-swap import hardening (carried)**; **chain both-maps invariant (carried)**; **long/cold-start 600s stress (carried)** — all unchanged.
- **Local-model M5 pre-flight + M5-calibration (carried)** — the prior list plus the fund slice's drafted constants (CIK-cache staleness, coverage/US guards, tier premiums, add floors); the FMP shape checkpoint is off this list (closed this session).
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with the remaining Portfolio depth slices + TO.

## Where to start

**Run `/metis-plan-task` for the next queue item (BUILD §What remains): the Local-analysis-models Settings section + sidebar Portfolio-runs history.**
The Settings section's named code prerequisite is the provider-credential save split out of the token-gated cloud save (`configuration.md §API Tokens`), and it is the in-app clear path for the shipped presence warning.
After that: Trade Opportunities (design settled 2026-07-09; the paid key is now live-verified, so implementation planning codes against verified shapes).
