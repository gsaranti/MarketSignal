# Current session handoff

## What happened

**Recent-reports backend planned → implemented → reviewed (Metis ×2 + Codex ×3) → committed + pushed to `main` (`ab73eaa`).** The sidebar's single static "Latest report" row became a real descending list of up to 30 persisted reports; selecting a row loads that issue's Markdown into the pane, and the toolbar reflects the selected issue (+ "Latest" tag). Load-bearing shape: two **Tauri-free `pipeline` read fns** (`list_reports`, `load_report`) behind thin commands — mirrors the `generate_report` spine; `storage::list_recent_reports` (newest-first, `summary_json` round-trip, `rowid` tiebreak, `RECENT_REPORTS_LIMIT=30`) + `get_report_record` (unknown id / missing file → typed errors, not panics). Frontend: `reports` list + `selectedReportId`/`selectedReport`, loaded on mount/after-generate/job-finished/focus.

Two **review-driven fixes worth not re-breaking**: (1) list-load failures live in a **separate `reportsError` channel** from selected-report load failures (`reportError`) — conflating them let a failed list refresh mask a valid loaded report pane (Codex P2); (2) a freshly generated/finished report is **optimistically inserted** into the sidebar (deduped + `.slice(0, 30)` capped) so it never lags the pane on a failed refresh (Codex P3 was the missing cap).

## Current state

On **`main` at `ab73eaa`**, working tree clean, **no work in flight**. Verified green: `cargo test`, `cargo clippy --all-targets --all-features`, `npm run build`. Real DB still `weekly_job_enabled=false` (disposable DBs for screenshots; [[gui-screenshot-audit]], [[live-model-smoke]]). Kit deviations + Archive decisions remain intentional ([[design-kit-deviations]]) — don't revert.

## Open questions

- **Queued design follow-ups** (pick next): full **Export** (MD + PDF — toolbar Export button returns); Settings **"Test connection"**; **dark-mode** wiring (tokens exist, unwired). [[ux-polish-round3-scope]] *(recent-reports now done — removed from this list)*
- **Retention-cascade enforcement** — this slice caps *display* at 30 via `LIMIT`; nothing yet **prunes** the 31st+ report (Markdown/HTML/vector summary, durable-learnings survival). Separate task per `BUILD.md`.
- *(new)* **Frontend component tests** — no Vue test harness exists (verification is type-check + Vite + Rust tests); the list-refresh-failure / stale-error paths are untested at the component level. Stand up Vitest + Vue Test Utils when wanted.
- *(new)* **Latent dark-mode contrast** — `.row-meta.is-error` and the report error-label use `--accent`, ~3.4–3.8:1 on the dark palette (sub-AA). Unreachable today (dark mode unwired); resolve at design-system level *if* the dark-mode follow-up ships. Same family as [[design-system-ink3-contrast]].
- *(new, minor)* **Silent stale-list on refresh failure** — when `list_reports` fails but a list already exists, the old list is kept with no indicator (by design); a user gets no signal the refresh failed.
- **Step-5 auto-archive** (move-to-archive at job start) — not built; keeps the Archive view empty in practice.
- **Report-body fidelity ceiling** — `MarkdownIt({ html:false })` can't render the kit's figures/charts/analyst-grid.
- *(carried minors)* `--ink-3` caption AA gap [[design-system-ink3-contrast]]; configured-state ~6px toolbar/sidebar-header offset; footer blocked-reason hover-only; session-scoped warning collapse; no clear-a-saved-credential affordance; plaintext secrets/Keychain; inbox parse-failure error state; inbox/archive folders materialize only on reveal; `is_running` event-driven only; Step-1 network pre-check; HTML-persistence (Step 17).

## Where to start

Resume on **`main`** (clean; no work in flight). Pick the next build target — **full Export**, **Test-connection**, **dark-mode wiring**, or **retention-cascade enforcement** — and `/metis-plan-task` it. **Do not revert** the documented kit/Archive deviations, nor the recent-reports decisions: the **two-channel error split** (`reportsError` vs `reportError`) and the **optimistic-insert cap** (`.slice(0, 30)`) — both close real review findings.
