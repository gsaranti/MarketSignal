# Current session handoff

## What happened

**Archive view (Research Archive surface) planned → implemented → reviewed → committed + pushed directly to `main` (`720a104`, no PR).** Surfaces `/research-archive` as the read-only-for-adding twin of the Inbox. Load-bearing shape: generalized `ResearchInbox.vue` into a shared **`ResearchDocuments.vue`** (title / copy / reveal-affordance as props) mounted for both inbox + archive — generalized, not duplicated; folded the inbox-specific `research.rs` core into a **folder-neutral** one (`list_folder` / `delete_folder_document`, neutral error msg) reused by both; added `research_archive_dir` + `list/delete/reveal_research_archive` commands; third sidebar nav item with an archive-count badge. Spec calls honored: per-row **delete kept** (docs allow delete from either folder), the **add affordance dropped** (no manual archive), reveal → a quiet `btn-secondary` **"Show in Finder"**; **no invented kit features** (no restore / archived-date / item-ID — the kit has no distinct Archive screen, so the Inbox treatment is the reference). Reviewer verdict **approve-with-nits**: nit#1 (a narrow doc-comment) fixed; nit#2 deferred. Verified: `cargo test` + `clippy` clean, `npm run build`, live GUI screenshots (empty + populated archive, nav badge "4").

## Current state

On **`main` at `720a104`** — Archive shipped, working tree clean, **no implementation in flight**. The Archive view is **forward-built**: it reads empty in normal use until the Step-5 move-to-archive pipeline exists (its empty-state copy says so). Kit deviations remain intentional + documented ([[design-kit-deviations]]) — don't revert. Real DB still `weekly_job_enabled=false` (disposable DBs used + restored; [[gui-screenshot-audit]], [[live-model-smoke]]).

## Open questions

- **Queued design follow-ups** (pick next): recent-reports backend (real 30-row list — sidebar header → "Recent reports", report toolbar reflects the selected issue); full **Export** (MD + PDF — the toolbar Export button returns); Settings **"Test connection"**; **dark-mode** wiring (tokens exist, unwired). [[ux-polish-round3-scope]]
- **Step-5 auto-archive** (move-to-archive at job start) — not built; until it ships the new Archive view stays empty in practice.
- **`--ink-3` caption AA gap** (~4.3:1) — systemic design-package call still deferred. [[design-system-ink3-contrast]]
- **Report-body fidelity ceiling** — `MarkdownIt({ html:false })` can't render the kit's figures/charts/analyst-grid; reports stay prose + tables + lists unless the report contract changes.
- *(deferred this session)* nit#2 — the two `ResearchDocuments` mounts carry inline copy props; lift to a config object only if a 3rd research surface appears (YAGNI for two).
- *(carried minors)* configured (no-warning) state ~6px toolbar-vs-sidebar-header top offset; footer blocked-reason hover-only; warning collapse session-scoped; clear-a-saved-credential affordance; plaintext secrets/Keychain; inbox parse-failure error state; inbox/archive folders materialize only on reveal; inbox badge freshness; `is_running` event-driven only; agent-construction failure not a recorded Failed job; Step-1 network pre-check; HTML-persistence (Step 17).

## Where to start

Resume on **`main`** (clean; no work in flight). Pick the next build target from the queued design follow-ups (recent-reports backend, full Export, Test-connection, dark mode) and `/metis-plan-task` it. **Do not revert** the documented kit deviations (status-band warning, region tints, Settings-only schedule toggle, no serif H2 on Settings/Inbox, 13px semibold surface titles, collapsible warning band, masthead titlebar) or the new **Archive decisions** (no manual-archive add affordance, per-row delete kept, "Show in Finder" secondary button, archive-count badge).
