# Current session handoff

## What happened

Cut release **v1.2.0** and shipped it. First **audited** everything on `main` since
`v1.1.1` (`e1d0406`→`9284e67`) to confirm the v2 local-suite logic (PR #44/#45) is
**dormant and prod-safe**: no UI entry point (zero `Portfolio`/`schwab`/`local_model`
refs in `src/`), gated behind the independent `LocalModels` gate, fixture-only — and the
report pipeline's behavior is **preserved** (shared-file changes are additive:
`embedding.rs`/`fmp.rs` zero-deletion, `vector_memory`/`pipeline` just thread
`MemoryNamespace::Report`, `config` local fields excluded from `validate`, `storage`
migrations additive/idempotent). Full gate green (cargo test 30, clippy, npm build, npm
test 91). Then bumped all **5 version anchors** 1.1.1→1.2.0, `npm run tauri build`
(`.app`+`.dmg`, version baked), installed to `/Applications` (replacing 1.1.1), committed
the bump (`ed663a6`, 5 files only — not the `.metis` changes), tagged **`v1.2.0`** on the
release-tip, pushed `main`+tag. Finally folded the 2026-06-28 web-research fetch-layer
decisions into **`.metis/BUILD.md`** (provisioning, rendered-retrieval, presence-vs-
connectivity gating; fixed "app-supervised"→"user-installed, app-supervised").

## Current state

On `main` @ `ed663a6`, in sync with origin. **Installed daily build is now v1.2.0**
(current). v1.2.0 ships #47's model-call resilience + run-log scrollbar fix + the
analytical-register restyle; the local-suite substrate rides along inert. **First launch
of v1.2.0 runs two additive/idempotent migrations** on prod data (`vector_memory`
`namespace` backfill + portfolio tables) — safe, flagged. **Uncommitted:**
`.metis/BUILD.md` (the web-research edits) + the pre-existing `.metis/CURRENT.md` change —
both left for the user to commit (`.metis` writes are user-run). The **bounded gated
retry** (empty-markdown/transient-stream, skip refusals, main agent + analysts) remains
**DEFERRED — designed only, NOT in v1.2.0**.

## Open questions

- Empty-markdown fault has no clean live repro; #47 now makes a recurrence legible — watch
  for it on prod runs.
- Commit the uncommitted `.metis/BUILD.md` (+ `CURRENT.md`) changes, or leave them?
- Source-quality registry / evidence-tiers still absent from BUILD.md (deliberately out of
  this session's scope) — add next if wanted. (Replaces the now-resolved "BUILD.md omits
  web-research decisions" question.)
- Standing M5-gated backlog unchanged: web-research provisioning/gating/UI + rendered-
  retrieval live validation, analytical-register restyle live-check, no new Tavily.

## Where to start

Implement the **bounded gated retry** (design in auto-memory
`model-call-empty-markdown-retry`: retry-once on empty-markdown / transient-stream, skip
refusals, main agent + analysts). It needs **its own** version bump + rebuild to ship —
#47 + the scrollbar fix already went out in v1.2.0, so the next release would carry the
retry alone unless other work lands first.
