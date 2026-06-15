# Current session handoff

## What happened

**Step-2 main-agent prior-report-context slice shipped** — planned, implemented, reviewed (`approve-with-nits`), squash-merged to **`main` @ `ea7a68f`**, pushed to `origin/main`, branch `step2-prior-report-context` deleted. The main agent now receives the recent prior reports — structured summary metadata **plus head-truncated Markdown bodies** — on a new top-level `MainAgentInput.recent_reports` channel (`Vec<RecentReport>`), loaded best-effort by `pipeline::load_recent_reports_for_audit` via a new `storage::list_recent_reports_with_paths`. This closes the carried "Step-2 prior-report-context" item and the "recent **Markdown** bodies belong to a future slice" deferral (BUILD.md line 31).

**The Retrospective Audit gate was firmed** — re-pointed from the soft `audit_memory[summary]`-fragment proxy onto this structural channel: the section is written iff `recent_reports` is present; `audit_memory` narrowed to a *steering* input (its `[learning]`/`[summary]` fragments point at what to scrutinise, no longer license the section). The gate stays **prompt-level** (the model writes the Markdown), but its *input* is now deterministic (recent_reports present ⇔ ≥1 prior report), not a vector-recall proxy. Reviewer judged this equivalence, not reduction — closing the carried "firm the audit summary-gate from prompt-soft to structural" forward-pointer.

## Current state

On **`main` @ `ea7a68f`**, synced with `origin/main`, **nothing in flight**. Backend gate green: `cargo test` **347/0/14**, `cargo clippy --all-targets --all-features` clean (reviewer re-verified on a forced recompile). Four files changed (`agent.rs`, `storage.rs`, `pipeline.rs`, `model_agent.rs`). No frontend, no live API spend. New app-layer tunables: `MAIN_AGENT_RECENT_REPORTS=3`, `RECENT_REPORT_BODY_CAP=12_000`.

## Open questions

- **Three review nits, all tracked follow-ons** (merged as-is, none blocking):
  - `audit_memory` ↔ `recent_reports` overlap — top recent reports can ride both as `[summary]` fragments and as full bodies; cheap (a summary line next to the body), the only fix is brittle fragment-parsing, and the system prompt already disambiguates them.
  - Two parallel recent-report loaders (`load_recent_report_context` for the router / `load_recent_reports_for_audit` for the agent) share ordering/cap/fail-soft shape but no code — consolidate when a third consumer appears, not before.
  - `MAIN_AGENT_RECENT_REPORTS` (3) / `RECENT_REPORT_BODY_CAP` (12_000) unvalidated — join the tuning bundle.
- *(carried)* tuning bundle (`MEMORY_TOP_K=5`, `LEARNINGS_PER_REPORT_CAP=5`, `LEARNING_DEDUP_THRESHOLD=0.93` unvalidated vs real `text-embedding-3-large` geometry, inbox caps, `COVERAGE_FLOOR=0.6`); `StubEmbedder` unfit for cosine-threshold tests (promote a shared `BasisEmbedder`/`DistinctEmbedder`); Tauri mocking unbuilt — next SFC spec (`Settings`/`App`, which call `invoke()`) needs an `@tauri-apps/api` mock; esbuild/vite advisory (3 high-sev, vite-8 breaking) parked; `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; wiremock / in-loop offline gap; conditional GPT-5-mini extraction stage; optional ` ```chart ` doc note.
- *(low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide.

## Where to start

Nothing owed. Consider a **BUILD.md refresh** first (the Step-2 bodies + the firmed audit gate — see Open questions). Then, most concrete remaining: establish the **Tauri-mock SFC pattern** (a second spec for `Settings.vue`/`App.vue`, needs an `@tauri-apps/api` mock); the **loader consolidation** if you'd rather collapse the two recent-report reads now; or the low-effort ` ```chart ` doc note.
