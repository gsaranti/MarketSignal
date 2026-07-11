# Current session handoff

## What happened

The queued **docs/ sentence-per-line reflow shipped**. All 24 `docs/*.md` reflowed to semantic line breaks (one sentence per line, continuation sentences at the bullet's content column; headings/fences/tables untouched) — commit `60d6878`, format-only. Zero content change was verified four ways: token-stream identity, byte-identical verbatim regions + idempotence, whitespace-normalized markdown-it render equivalence on all 24 files, and the anchor sweep (1054 relative links, 0 broken — five more links than before only because joining made previously line-split links visible to a line-based scan). Follow-up `2ad5378` added `.git-blame-ignore-revs` (carrying the reflow hash), set `git config blame.ignoreRevsFile` (verified working), and added a **Docs formatting** convention section to CLAUDE.md so future edits maintain the format. Both commits pushed. Note for the M5 move: `blame.ignoreRevsFile` is *local* git config — re-run `git config blame.ignoreRevsFile .git-blame-ignore-revs` on any fresh clone.

## Current state

`main` @ `2ad5378`, pushed, clean tree. The reflow task is fully closed — nothing in flight. Queue head: **the fund-slice plan** — `/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility`, spec-stable and audited to convergence; starts on explicit user go-ahead.

## Open questions

- **Fund-form scenario-target methodology (blocking)** — what a *priced* fund's scenario targets derive from; the fund-slice plan's first decision.
- **Local-suite scorecard display (carried, deferred)** — whether the TO shadow scorecard and Portfolio outcome scorecard get UI surfaces.
- **Encrypted-archive live round-trip (carried, optional)** — one passphrase export→import before the M5 move.
- **Dev-app sanity residue (carried)** — table-head glyph hit-target click (needs portfolio data).
- **Keychain fail-soft candidate (carried)** — a denied Keychain read blanks the local warning categories for the session.
- **Stage-and-swap import hardening (carried)** — mid-import I/O failure can leave partial files; named, not scheduled.
- **First post-v0.31.2 Ollama release (carried)** — #14645 fix + `think:true`+`tools` coverage; check before pinning the M5 version.
- **Chain both-maps invariant (carried)** — tighten once a live `/chains` response confirms both maps.
- **Long/cold-start 600s stress (carried)** — only a long/cold-start report could near 600s.
- **Local-model M5 pre-flight + M5-calibration (carried)** — 122B load/backend, `num_ctx`, throughput, long-context probe; Stooq refresh, `continuity_weight` bands, thresholds, budgets.
- **Four-part verdict + bidirectional-conviction bound; §1 open drafts; M5-gated backlog (carried)** — land with full Portfolio + TO.

## Where to start

Run **`/metis-plan-task` for the fund slice** against `docs/portfolio-analysis.md §Asset eligibility`. The plan's first decision is the **fund-form scenario-target methodology** (what a priced fund's scenario targets derive from) — settle it before anything else in the plan. All docs edits from here on follow the sentence-per-line convention now in CLAUDE.md §Docs formatting.
