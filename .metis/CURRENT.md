# Current session handoff

## What happened

Redesigned (docs-only, committed + pushed) the **Trade Opportunities** feature
into **two on-demand jobs sharing one page** — **Discover (DTO)** (discovery +
deep-research of candidates + a cheap engine sweep of the rest) and **Audit
(ATO)** (user-selected re-evaluation forking to **Quick Audit** = the cheap
re-derivation, or **Deep Audit** = the full per-candidate loop). The load-bearing
flip from last session's archive model: **only a deep re-evaluation can archive;
the cheap re-derivation never does** — it refreshes the quant read and raises a
non-destructive **attention warning** (this **retires** the prior "every-run cheap
upside re-derivation can fail → archive" model). DTO spends the deep-research
budget **new candidates first**, leftover on **re-surfaced** existing
opportunities (run in the **Step-5 loop** as carried-forward candidates); **Step 7
stays deterministic**; the matrix is **self-flagging, not self-cleaning**. Added:
three card badges (Consider Deep Audit / Deep-researched today / Research stale),
time-aware **`continuity_weight`** on deep passes, a **price-only Stooq render
floor** (shared daily-bar cache, 8 PM-ET refresh) with the **FMP `quote` a
job-time input** (not persisted). Gate decision: **uniform presence** for all
local jobs, with one carve-out — Quick Audit (engine-only) skips the run-gate
**daemon-connectivity check + SearXNG probe**; an earlier "full Quick-Audit
exemption from Schwab/presence" was **reconsidered and reverted** (~11-spot doc
footprint, and Quick Audit uses no Schwab/web data anyway — don't re-propose).
Reviewed exhaustively — **6 Codex rounds** + 2 fresh-eyes consistency subagents
(CLEAN) + anchor check (288 resolve), all resolved.

## Current state

On `main` @ `eebab53` ("Docs: split Trade Opportunities into Discover (DTO) +
Audit (ATO) jobs"), pushed, in sync with origin. Docs-only — 9 files (7 corpus:
`trade-opportunities.md`, `-workflow.md`, `configuration.md`, `interface.md`,
`storage.md`, `local-models.md`, `web-research.md`; + **`BUILD.md`** +
**`INDEX.md`** refreshed to the DTO/ATO model). The redesign is **specified, not
implemented** — design for the not-yet-built full Trade Opportunities, which is
*later* in build order. Corpus dev-ready; build order unchanged.

## Open questions

- **M5-calibration (expanded this session):** new constants to live-tune — the
  **8 PM-ET / 24h** Stooq refresh, **~4-week** `continuity_weight` bands +
  Research-stale threshold, high-bar **tripwire** thresholds, the **DTO
  deep-research budget** default, leftover-budget oldest-N ordering — alongside
  the carried archive constants (retention 100, upside-exhausted threshold).
- **Four-part verdict model + bidirectional-conviction bound** (carried): lands
  when full Portfolio + TO are built.
- §1 **genuinely-open drafts** (carried): dead-money hurdle, feasible-set
  bounding; TO risk-tier / horizon / hypothesis-score / quota / gate tables —
  per-slice starting values.
- Standing **M5-gated backlog** (carried): web-research provisioning / gating / UI
  + rendered-retrieval, analytical-register live-check, no new Tavily, local-model
  live quality / FMP-tier.

## Where to start

Begin the **live Schwab OAuth slice** (next in build order — unaffected by this
session's docs work; `schwab-integration.md` audited clean: OAuth loopback,
30-min/7-day tokens, Keychain, positions + option chains). The DTO/ATO redesign
(like the archive + four-part verdict) lands **later**, when full Portfolio + TO
are built. **Check code-vs-doc first** on any Portfolio/TO formula — PR #45
already implements the MVP engine math.
