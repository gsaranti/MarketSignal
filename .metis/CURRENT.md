# Current session handoff

## What happened

**Pivoted the product from auto-scheduled to manual / on-demand report generation**, and **renamed the report "Weekly Market Report" → "Market Signal Report"** (a full rename including the stored contracts). Decisions locked with the user: pure manual (no scheduler, timer, tray, or reminder — the user picks cadence); **keep** the Failed-job warning; generalize the report's analytical window to cadence-agnostic ("developments since the previous report", leaning on the existing cadence-honest delta view); rename `report_type` `weekly_market` → `market_signal` and the filename `…-weekly-report.md` → `…-report.md` **with a one-time migration**; drop the tray / must-stay-running requirement; and make network unreachability a **Failed run, not a pre-run gate** (this **closes the old "network pre-flight" open question** as won't-build). The rationale: a tray-resident desktop app is an unreliable scheduler, and the whole *Missed*-state machinery only instrumented that unreliability.

This session was a **documentation-only pass** executing that decision: 14 `docs/` files rewritten + `.metis/BUILD.md` and `INDEX.md` reconciled to the target spine. Job states went 5→4 (Missed dropped); the Persistent Warning Area 5→4 (MissedScheduledJob dropped, FailedJob kept). metis-task-reviewer returned **approve-with-nits** (fixed a BUILD first-vertical-slice filename contradiction + incidental cadence residue); an external **Codex** pass then caught two `analyst-skills.md` "this week's" lines my survey grep had filtered out (fixed). **No code was touched.**

## Current state

`main` = **`10667b9`** (squash-merge of the docs pivot off `docs/manual-only-pivot`) + this handoff commit. Working tree otherwise clean. **`main` is ahead of `origin/main`** — not pushed (the user asked to `pull`, not push; awaiting a push decision).

`BUILD.md` is at the **target spine** and carries a **"Pending code slices"** note (`§Scheduling & runtime`) — the docs describe the destination; the code still carries the old scheduled model. The four queued code slices:
1. Remove the Rust timer, `decide_scheduled_run`, `ScheduledRun`, the scheduled `lib.rs` command path, missed-detection, the enabled flag, and the `MissedScheduledJob` warning kind (+ `jobs::missed_warning`).
2. Remove the Tauri tray / must-stay-running wiring.
3. Run the `weekly_market`→`market_signal` `report_type` + filename rename **migration** (idempotent, one-time — see `docs/storage.md §Legacy Naming Migration`).
4. Drop "weekly"/"prior week" from the agent prompts (`model_agent.rs`, `analyst_agent.rs`).

The analyst trait, the manual command path, and the cadence-honest delta view **already match** the target and need no change.

## Open questions

- *(live, needs a run)* **Empirical skills calibration** — read generated reports to see which of the 16 lenses actually improve the thesis and the analyst reviews, which get ignored, and whether prose-only delivery creates repetitive language across the 16. The sole named skills follow-on; no test catches prose dilution.
- *(migration care)* Slice 3's rename migration mutates existing rows/files — it must be idempotent and update stored file paths, not just rename on disk (spec'd in `storage.md §Legacy Naming Migration`).

## Where to start

The docs/spine pivot is **landed on `main` (not pushed)**. First decide whether to **push** `main` to `origin`. Then plan + implement the **first code slice — scheduler removal** (slice 1): it's the self-contained one that unblocks the rest, and a clean `/metis-plan-task` target. Run the rename **migration** (slice 3) carefully for idempotency.
