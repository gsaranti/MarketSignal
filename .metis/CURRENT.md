# Current session handoff

## What happened

**v1.3.0 was built, released, and installed** — the user made a deliberate exception to the no-build-until-Portfolio-and-TO rule: the data-portability slice only serves the M5 migration if it's in the installed app before the hardware swap. Version bump across the five anchors (commit `f4a5829`, tag `v1.3.0`, both pushed), full verification green (633 cargo tests, clippy 0, `npm run build`, 40 Node + 152 Vitest), `/Applications` now runs 1.3.0 over the prod store. All ten banked PRs shipped; Schwab auth rides idle until v2. The user chose **ship-as-is on the warning band**: the permanent LOCAL MODELS presence warning (no in-app clear path until the v2 Settings section) and the Schwab warning that will appear when the Keychain refresh token lapses (~Jul 14) are accepted. **Data portability was GUI-exercised for the first time, in the release binary** against sandboxed copies of the prod store (`MARKET_SIGNAL_DATA_DIR`): export → valid archive (manifest counts exact), fresh-load import with `markdown_path` re-derivation, replace-all surfacing the #54 ConfirmDialog with the archive-detail paragraph, and the single-run-slot guard rejecting a concurrent import — all passed. Discovered en route: the **Keychain ACL prompt (recurs per ad-hoc rebuild) blocks first paint** — up to 3 sequential prompts from sync startup reads (`check_local_configuration` + `schwab_status`×2), blank window until answered; Deny is safe (fail-soft), keyring service is shared dev↔prod.

## Current state

On `main` @ `f4a5829` (this handoff commit pending). Installed app = **v1.3.0**; the no-build rule **resumes** until full Portfolio + Trade Opportunities land. Nothing is mid-flow. The user should **Always Allow** the Keychain prompt on their first v1.3.0 launch (one-time per rebuild; window looks frozen until answered).

## Open questions

- **Dev-app sanity pass — mostly closed this session** (Data section ✓, ConfirmDialog archive-detail ✓, backend open-dialog capability ✓). Residue: table-head glyph hit-target click (needs portfolio data to render the holdings table).
- **New small candidate** — a denied Keychain read errors the whole `check_local_configuration` report, blanking the local warning categories for the session; consider fail-soft (tokens-read failure → not-connected, not Err) with the v2 wiring.
- **Stage-and-swap import hardening (carried)** — mid-import I/O failure can leave partial files; named candidate, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage; check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Back to the standing order: **full Portfolio (funds)** — `/metis-plan-task` against `docs/portfolio-analysis.md` §Asset eligibility (fund path). Then Local-models Settings section (now also the clear path for the shipped warning band) / sidebar Portfolio-runs history → Trade Opportunities.
