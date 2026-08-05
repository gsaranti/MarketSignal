# Current session handoff

## What happened

**The Portfolio-page polish micro-slice SHIPPED** — committed directly to `main` (`f24c852`, no PR; display-only, one Codex round).
The four F9 items landed in `PortfolioView.vue` + its spec: the wrap-safe key-figure-strip lattice (1px flex gap over a hairline background, cells repaint paper, a partial final row stretches — an app-side extension of the kit's single-row `.keyfig`, replacing the border idiom that dropped row-1 bottom rules), the card-header **position block** (Price / Avg cost / Cost basis above Unrealized on both full-card branches, "—" on unreported inputs), **sub-2% weight-band decimal precision** (band-max rule), and Hold's **"maintain"** phrasing (`bandVerb`, both action-line copies).
Five plan assumptions were settled by selection UI pre-implementation (scope = #8 only, header placement, band-max rule, "maintain" wording, app-side-only DS extension).
The Codex round's medium short-position finding (signed cost basis renders "—") was **disputed on engine-reachability evidence** — net-short short-circuits to not-rated *before* class routing, cards are run-anchored, side reversals force-include — with its regression-case half adopted (net-short row → reduced card, no `.hc-position`, spec-pinned); Codex approved.
Verified: cargo 890 lib + integration 0 fail, clippy 0, npm build, 40 node + 196 vitest, plus a **live dev-app visual check** (run `3b21ae85`, both themes, 7+1 and 5+3 wrap widths).
For the record: a band straddling 2% (e.g. 1.8–2.2%) renders "2–2%" under the locked rule — inherent, watch only if such a band appears live.

## Current state

**No capture debt** — BUILD (§What remains narrowed to the one remaining slice) and INDEX (suite status paragraph + first-live-run record row stamped `#8 landed`) updated in-session and committed with this handoff.
Queue: the block's remainder is **only the section-scoped footer + report-nav slice** (user-settled design 2026-07-31, three parts: a sidebar "Latest Market Report" nav entry, "Generate now" rendered only on the report view, and the footer LAST RUN readout scoped to the active section's job via a `job_type` filter in `jobs::job_status`; `docs/interface.md §Main Layout` is the canonical home to amend when it lands; the footer defect was re-observed live this session) — then the single big confirmation run.

## Open questions

- **New big-run watches (construction leg)** — lean-divergence / engine-bar / carried-stale-lean rates at 47-position scale; construction-prompt fit in the shared 131k `num_ctx` (settled: compress digests, never `num_ctx`); overlay classification against real Schwab OCC rows; the 7b sizing-only decided-range movement rate that would justify a band-relative episode trigger. Live 122B construction behavior is wholly unexercised (all Codex rounds deferred it).
- **Research-loop activation obligation** — identity + source-text validation + period normalization before the pre-profit producer activates.
- **Standing** — unchanged carried list: live-run calibration watches (STI-reads-zero, YoY contiguity, outcome-leg watches), no A letters under grade-v2, big-run checklist, reasoning-pane DOM weight, encrypted portability round-trip, step-17 embedding, 600 s stress, scorecard display, dev-store residue, Keychain fail-soft, stage-and-swap import, chain both-maps invariant, four-part verdict bound, §1 open drafts, fraud-producer posture, fund-slice drafted constants, checkpoint/resume + the 6g input-delta validator (still gates the outcome slice's dormant legs).

## Where to start

`/metis-plan-task` for the section-scoped footer + report-nav slice — the last item in the block before the big confirmation run.
It touches Rust (`jobs::job_status` gains a `job_type` filter), so the plan's verification command must name clippy alongside `cargo test` per CLAUDE.md.
