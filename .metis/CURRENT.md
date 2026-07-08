# Current session handoff

## What happened

**The `.metis/` files were audited and trimmed** (commit `7e36c37`, pushed). BUILD.md and INDEX.md were audited against the metis-build-spec and metis-reconcile skills on three goals — workflow effectiveness (know what's built vs not), no duplication of `docs/`, size — and both failed 2 and 3: roughly half of each was doc transcription at ~2.5× the intended size. The trim: **BUILD.md 6.5k→4.0k words** — transcription blocks (portability mechanics, the Portfolio/TO designed-feature tours, control specifics) replaced with status + a named **invariants bullet** + doc pointers; the PR-by-PR build-order chain replaced by an As-built summary and a **§What remains** queue. **INDEX.md 5.5k→3.0k words** — fat parentheticals slimmed to lookup entries; all 161 rows and every file§section pointer preserved (mechanically verified); a built/designed status note added for the local suite; header now states it is hand-maintained, pointers-not-summaries. Cut discipline held: every deletion verified present in the cited doc first; BUILD-only records kept condensed (Keychain rail/first-paint, `@page`, partial-files residue, Schwab read-only rationale, rustls acceptor, `MARKET_SIGNAL_SCHWAB_FIXTURE`). A review pass caught two trim errors (view-toggle wrongly implied built; fixture env var dropped) — both fixed pre-commit. Conventions captured in the `build-md-compact-as-built` memory.

## Current state

On `main` @ `7e36c37` (this handoff commit pending), tree clean. Nothing is mid-flow. Installed app = **v1.3.0**; the no-build rule remains in effect until full Portfolio + Trade Opportunities land. If the first v1.3.0 launch hasn't happened yet: **Always Allow** the Keychain prompt (window looks frozen until answered). New session-end habit: when a slice lands, update BUILD.md's As-built sentence **and** its §What remains list.

## Open questions

- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data to render the holdings table).
- **Keychain fail-soft candidate (carried)** — a denied Keychain read errors the whole `check_local_configuration` report, blanking the local warning categories for the session; fail-soft (tokens-read failure → not-connected, not Err) with the v2 wiring.
- **Stage-and-swap import hardening (carried)** — mid-import I/O failure can leave partial files; named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage; check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Back to the standing order: **full Portfolio (funds)** — `/metis-plan-task` against `docs/portfolio-analysis.md` §Asset eligibility (fund path); BUILD.md's §What remains carries the queue. Then Local-models Settings section (the clear path for the shipped warning band) / sidebar Portfolio-runs history → Trade Opportunities.
