# Current session handoff

## What happened

Shipped **scheduler slice 1 — the deterministic job-lifecycle core**, on the existing manual run path. Committed **`14fbada` (feature)** + **`b3788e4` (docs)**, both pushed to `origin/main`. New `src-tauri/src/jobs.rs` owns the lifecycle: a single-workflow **`RunGuard`** (`Arc<AtomicBool>`, `try_begin`→`RunToken` whose Drop frees the slot — 2nd run ⇒ **Skipped**); a **Tauri-free `run_job`** orchestrator that records every outcome to a new **`job_runs`** table; and **`failure_warning`**, which surfaces the non-blocking **`FailedJob`** category into the Persistent Warning Area (cleared on the next successful run). `check_configuration` now merges that warning in (gained `app: AppHandle`); `generate_report_manual` routes through `run_job` via a managed guard.

Two reviews passed: **Metis `approve-with-nits`** (unused `MainAgentInput` import — fixed) and **Codex** (clippy `large_enum_variant` on `JobOutcome::Successful(GeneratedReport)` — fixed by boxing the variant). Codex's catch exposed that test-only verification missed clippy, so a **Verification** section was added to `CLAUDE.md`/`AGENTS.md` and a **Development** section to the README, naming the canonical set (`cargo test` + `cargo clippy --all-targets --all-features` + `npm run build`).

## Current state

Working tree clean; both commits pushed. Verified: `cargo test` (26 lib pass / 1 ignored live + `generate_report` + 3 `job_lifecycle`), `cargo clippy --all-targets --all-features` clean, `npm run build` clean. Slice produces **Successful / Failed / Skipped** on the manual path; **`Missed`/`MissedScheduledJob` are NOT produced** (deferred to slice 2).

**One verification still unexercised:** manual `npm run tauri dev` was not run — both the gate's "unset an env var → warning row" check and the new "forced failure → `FailedJob` row in the warning area" check.

Deferred slices: **scheduler slice 2 (live timer)** — now the lead; **HTML persistence + PDF**; **`list_reports`**; **FMP/FRED/BLS data-source adapters** (would ground the empty `MainAgentInput`).

## Open questions

- **Agent-construction failure isn't a recorded Failed job** — `ModelMainAgent::new` is built in `spawn_blocking` *before* `run_job`, so a build failure surfaces inline but writes no `job_runs` row / `FailedJob` warning. Report-generation failures *are* captured. Low risk now (`new` only builds an HTTP client); revisit in slice 2. *(new)*
- **FailedJob dismissal** — shipped rule is "clears on next successful run"; no DB-backed dismiss, and `PersistentWarningArea.vue` has no dismiss control. Spec (`interface.md`) wants dismissible warnings. *(new)*
- **Network reachability** (Step-1 gate pre-check) — a run that fails offline is now captured as a Failed job, but the *proactive* pre-check is still not done. *(carried, partially addressed)*
- **Same-day filename collision** — `pipeline.rs:54` writes a date-based canonical `.md`, so two same-day runs overwrite the file while inserting separate DB rows. Rides with slice 2's local-time schedule model. Sibling to [[utc-vs-local-report-date]]. *(carried)*
- **UTC-vs-local report date** — `created_at` + filename, and now `job_runs` timestamps, are `Utc`-derived; decide with slice 2. ([[utc-vs-local-report-date]]) *(carried)*
- **Env-slug vs display-name drift** — gate parses config slugs; align with `docs/configuration.md` display names when a Settings store replaces the env substrate. *(carried)*
- **HTML-persistence path (Step 17)** — how rendered HTML returns to the backend for SQLite; lands with the HTML/PDF slice. *(carried)*

*(Resolved this session: the concurrent-run guard — now `RunGuard`.)*

## Where to start

Run `/metis-plan-task` for **scheduler slice 2 — the live timer**: the tokio **Sunday 9 AM local** timer, **tray runtime** (close ≠ quit), **missed-job detection + `MissedScheduledJob` production**, the **status / enable-disable UI**, and the **UTC-vs-local** + **same-day-filename** decisions that converge there. First, close slice 1's gap by running the deferred manual `tauri dev` smoke (forced failure → `FailedJob` row). Alternatives if deferring the timer: **data-source adapters**, **HTML persistence + PDF**, or **`list_reports`**.
