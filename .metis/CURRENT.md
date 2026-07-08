# Current session handoff

## What happened

**The data-portability slice shipped end-to-end**: planned, implemented, triple-reviewed, and merged as **PR #53** (squash `792aeb9`) — banked slice #9. New `src-tauri/src/portability.rs` (structured versioned zip: five durable tables as NDJSON + report/research files, size+sha256 manifest; optional AES-256-GCM/Argon2id whole-container encryption; import = fresh-load / replace-all-with-confirmation, **everything validated pre-destructively** — bidirectional manifest verification, embeddings decoded, uniqueness/cardinality pre-checked, `markdown_path` re-derived, dangling reports skipped with their summary + snapshot cascade); three slot-guarded commands (`RunKind::DataPortability`); Settings **Data** section + post-import full refetch in App. The design package gained its first **confirmation dialog** (user generated it in Claude Design; grafted per the graft-not-swap rule: `--scrim` token + `.dialog-*` into `colors_and_type.css`, preview checked in; the export's regenerated bundle had again dropped `--accent-text` and the `.btn:disabled` extension — drift confirmed, graft-only held). Review trail: metis reviewer approve-with-nits (3 nits applied) → Codex round 1 (manifest-bypass + mid-transaction constraint hazard, both fixed with re-checksummed tamper tests) → Codex round 2 clean. Verification green: 627 cargo tests, clippy 0, `npm run build`, 148 Vitest + 40 Node.

## Current state

On `main` @ `792aeb9`, working tree clean (this handoff commit pending). No build — the no-build-until-Portfolio-and-TO rule holds; nine slices banked, installed app stays v1.2.1. Data portability is **built but never exercised in the live GUI** — the dialog capability question (backend-driven open dialog) and the section's visual render are unverified. BUILD.md, `docs/data-portability.md`, and INDEX.md were all flipped to as-built this session (status headers, settled primitive + flow details, the pre-destructive-validation contract, verification section, and a new INDEX concept line for the confirmation dialog) — no doc staleness carries forward.

## Open questions

- **Stage-and-swap import hardening (new, deferred)** — a mid-import I/O failure during the file phase can leave partial files (DB stays transactional; recovery = re-run the intact archive). Named candidate, not scheduled.
- **Dev-app sanity pass (carried, grew)** — Data section + ConfirmDialog render, backend open-dialog capability check, and the table-head glyph hit-target click.
- **First post-v0.31.2 Ollama release (carried)** — does it ship the #14645 fix and cover `think:true`+`tools`? Check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten the drift guard once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Next slice per the standing order: **full Portfolio (funds)**, then Local-models Settings section / sidebar Portfolio-runs history → Trade Opportunities. Before or alongside: fold the dev-app sanity pass into the next dev-app open (Data section + ConfirmDialog render, open-dialog capability, table-head glyph). `/metis-plan-task` against `docs/portfolio-analysis.md` §Asset eligibility (fund path) when ready.
