# Current session handoff

## What happened

A short interstitial session — no code or docs changes. The one outcome: the **docs/ prose-format problem was analyzed and decided**. The corpus's one-line paragraphs (716 lines >200 chars, 58 >2,000, max ~5.6k in `storage.md`/`portfolio-analysis.md`) amplify every line-based diff (a one-word edit re-emits the whole paragraph twice — the review-round tax) and every grep hit (one lookup match can return ~1,400 tokens). Reading whole files costs the same either way — the waste is tool-result amplification, not tokenization. Decision: reflow all of `docs/` to **sentence-per-line** (semantic line breaks), chosen over fixed-column wrap because edits then diff as exactly one line and never cascade a paragraph rewrap. Queued as next session's first task — deliberately landing in the clean window between the closed Codex round and the fund-slice work, so the mechanical diff drowns no content review.

## Current state

`main` @ `26b7b0c`, pushed; the only working-tree change is this handoff rewrite. Nothing in flight. Queue: **(1) the docs reflow** — one sentence per line; 2-space continuation indents keep follow-on sentences inside their bullet; never touch headings, fenced blocks, or tables (rendering is unchanged — Markdown folds single newlines). One standalone commit with **zero content changes**; add its hash to `.git-blame-ignore-revs` (+ `git config blame.ignoreRevsFile .git-blame-ignore-revs`); add a one-line convention to CLAUDE.md so future edits maintain it; verify with the anchor sweep (~1030 links, expect 0 broken) and a rendered-output spot-check. **(2) the fund-slice plan** — unblocked, audited to convergence, spec-stable after the Codex round; **starts only on explicit user go-ahead, never as a follow-on to (1)**.

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

Apply the **sentence-per-line reflow to every file in `docs/`** (mechanics in *Current state*), commit it standalone, and **stop there — that is the whole task**. The fund-slice plan (`/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility`, first decision = the **fund-form scenario-target methodology**) is a separate task the user starts explicitly; do not flow into it after the reflow.
