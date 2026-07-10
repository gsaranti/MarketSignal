# Current session handoff

## What happened

**The Trade Opportunities docs-review pass (2 of 3) completed and its fixes are committed** (`c872d66`, pushed). A cold read of trade-opportunities.md + trade-opportunities-workflow.md plus the TO sections of the seven shared docs, under the same contract as the Portfolio pass. Eight findings applied: the four carry-forwards resolved — the **#14645 Mode caveat** added to the workflow legend (Portfolio's template verbatim); the **audit-record field set single-homed onto storage.md** (conviction decomposition generalized to per-item citing §6g + §5h, TO discovery/screening inputs folded in; both twins — §Storage and display and workflow **Step 9**, not Step 8 as previously noted — reduced to pointers; the opportunity-graph placement drift resolved as *own carried store, never an audit field*); **rf + 8/16/30 verified consistent** (no action); `profile` confirmed covered (no action) — plus four new: **Step 6's ranking/dedup call designated thinking** (was the only untagged model call); the **"same drafted constants" claim scoped to the forensic trips** with the two narrative-vs-reality trip rules (Portfolio ~1.5× outrun vs TO >70%-of-move `hype`) stated side by side as job-specific; **activist/congressional feeds removed from the discovery-feeder lists** (symbol-keyed on the current plan — per-candidate only, rationale recorded) and the missing **`ipos-calendar` discovery row added**; the **fifth shadow-ledger class (`retired-hypothesis`) named** in §Outcome learning + both per-class-reads parentheticals. Optionally-approved #10 also landed: the **Stooq cache refresh rule single-homed in storage.md** (calibratable note moved in; TO doc + workflow keep pointers). Anchor sweep over all 24 docs: 0 broken. User decided **no second TO pass and no diff re-read** — an external **Codex round on the TO docs** is the independent check instead.

## Current state

On `main` @ `c872d66`, tree clean, pushed. Docs review is 2 of 3 passes done: Portfolio ✓ → Trade Opportunities ✓ → **market report last, deliberately in a fresh session**. Two finding classes are promoted into the market-report pass's checklist: (1) **design-doc enumerations vs data-sources.md's report endpoint tables** (the drift class behind the congressional/`ipos-calendar` findings); (2) **every model-call / agent-stage contract fully specified** (the class behind the Step-6 mode gap). A **Codex review of the TO docs is queued** (user-run; lands in `iris-codex-last.md`, findings verified against the docs before agreeing). Fund-slice planning stays queued behind the review.

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

**Market-report docs-review pass** (fresh session, pass 3 of 3) — **enrichment-weighted, since the report is built and shipped**: deep cold read of the planned report enrichment (data-sources.md §Planned report enrichment + the report endpoint table's planned-paid rows, report-workflow.md §Step 3 / §Step 16 where the new signals enter the packet, storage.md §Baseline Snapshots) — internal consistency across those homes, tier-audit alignment of every planned endpoint, the engine-derived / outside-the-delta-engine exclusions holding, cadence-honest window sizing; then only a **light sweep** of the rest of the report group (report-workflow, report-structure, agents, analyst-skills, thesis-continuity, research-documents, export, run-tracking) for the two promoted checklist classes — no re-litigating settled as-built prose beyond those. When all three passes are done, `/metis-plan-task` against `docs/portfolio-analysis.md §Asset eligibility` (fund path) — first decision: the fund-form scenario-target methodology. The Codex TO round can land whenever; verify its findings against the docs before agreeing.
