# Current session handoff

## What happened

**A second review of the data-portability slice (PR #53) ran, and every finding shipped as PR #54** (squash `5df6668`). The audit's verdict on #53 was sound — the pre-check↔schema-constraint mirror was independently re-derived and confirmed complete — but it surfaced one durability item and a tail of hardening/UX/test gaps, all now fixed: **Argon2id cost parameters frozen in code** (byte-identical to the old `Argon2::default()`, pinned by a KDF test vector; a drifted crate default would have stranded every encrypted archive as "wrong passphrase"), **bounded archive reads** (4 GiB entries ceiling on actually-inflated bytes + a 16 MiB `manifest.json` bound — the manifest bound was the external Codex round's catch: the first ceiling didn't cover the earlier read), **write-then-rename export** (no truncated zip on disk-full; failed rename cleans the `.partial`), **all five `db/*.ndjson` entries now required and manifest-vouched** (truncated archive refuses, never imports a table as zero rows), **the replace-all dialog now shows the picked archive's created date + counts** (new optional ConfirmDialog `detail` paragraph; the previously-unused `ArchiveInfo` now feeds it), schema-side pointer comments coupling exported-table constraints to the import pre-checks, and **+10 tests** (format-version rejection, missing table entry, three-way path-traversal tamper, both ceilings, KDF vector; ConfirmDialog detail; App-level import-fork wiring). `docs/data-portability.md` amended as-built. Verification green: 633 cargo tests, clippy 0, `npm run build`, 40 Node + 152 Vitest.

## Current state

On `main` @ `5df6668`, tree clean (this handoff commit pending). No build — the no-build-until-Portfolio-and-TO rule holds; **ten merged-unbuilt PRs** now ride main, installed app stays v1.2.1. Data portability remains **never exercised in the live GUI**; the dev-app sanity pass grew again (it now also covers the dialog's archive-detail paragraph). Nothing is mid-flow.

## Open questions

- **Dev-app sanity pass (carried, grew)** — Data section + ConfirmDialog render incl. the new archive-detail paragraph, backend open-dialog capability check, table-head glyph hit-target click.
- **Stage-and-swap import hardening (carried)** — mid-import I/O failure can still leave partial files (DB transactional; recovery = re-run the archive). Named candidate, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — does it ship the #14645 fix and cover `think:true`+`tools`? Check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten the drift guard once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Next slice per the standing order: **full Portfolio (funds)**, then Local-models Settings section / sidebar Portfolio-runs history → Trade Opportunities. Before or alongside: fold the dev-app sanity pass into the next dev-app open. `/metis-plan-task` against `docs/portfolio-analysis.md` §Asset eligibility (fund path) when ready.
