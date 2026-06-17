# Current session handoff

## What happened

**Reshaped the analyst skills library and shipped its structured-output channel.** Three coupled moves, one squashed commit (`913be13`): (1) **dropped two-phase progressive disclosure** — all 16 skills now ship in full in one generation pass (`format_skill_library` renders the whole `CATALOG`); the entire selection subsystem is deleted (`SkillSelection`/schema/request builders, `select_skills`/`run_skill_selection`/`call_nonstreaming`, `skills::{frontmatter_catalog,catalog_names,select_bodies}`). Rationale the next session shouldn't re-litigate: bodies are ~150 tokens each (~2.4k for all 16) and the phase-1 call re-sent the *entire* packet just to save the ~320-token frontmatter — net-negative. (2) Added a per-skill **`output` verdict shape as a forcing-function** — rendered `Verdict to produce —`, folded into the thesis prose, **not parsed and not persisted** (the user's explicit pick over persist/UI/analyst-fed). (3) **Audit-grounded the 16 lenses** against a code-explorer's inventory of the live Step-3 scan: bodies came back clean (no stale series ref, no "% move" on a point-delta rate), and three gathered-but-unused signals were wired in — Russell 2000 / `index_performance` (Breadth + Time Horizon), earnings estimate-vs-actual surprises (Consensus + Narrative), semiconductor sector→industry (AI Infra).

## Current state

**Shipped and pushed.** `main` = `913be13`, working tree clean, nothing in flight. Touched `skills.rs` + `model_agent.rs` + `docs/analyst-skills.md` (+186/−442 — the deletions are the removed selection subsystem). Verified green: `cargo test` **381 lib** + all integration suites; `cargo clippy --all-targets --all-features` clean. metis-task-reviewer **approved** (clean, no nits); an external Codex pass validated the audit fixes + body/output split. **Docs reconciled this session:** `docs/analyst-skills.md` amended to as-built, and `.metis/BUILD.md`'s `agents` bullet rewritten off the now-deleted progressive-disclosure mechanism.

## Open questions

- *(deferred, larger)* Extend skills to the **three Bull/Bear/Balanced analysts** — the doc's full consumer set; the largest remaining skills slice. The one narrowing still live.
- *(deferred)* The **richer persisted/parsed structured-output channel** — we shipped forcing-function-only; persist / UI / analyst-fed all remain options.
- *(Codex-flagged, needs a live run)* **Empirical calibration** — read a few generated reports to see which lenses actually improve the thesis, which get ignored, which create repetitive language. The real validation of "all 16 every time"; no test can catch prose dilution.
- *(carried, low)* `SECTOR_PE_MAX` (120) calibration via the `#[ignore]`d `tuning_sector_pe_distribution_probe`; `cargo fmt` dirty repo-wide + esbuild/vite advisory.

## Where to start

`main` is clean and pushed; BUILD.md and `docs/analyst-skills.md` are current. Nothing owed — open a fresh direction. Natural next frontiers: **extend skills to the three analysts** (largest remaining slice), the **persisted/parsed structured-output channel**, or the **empirical calibration** (needs a live run + reading real reports). `SECTOR_PE_MAX` stays the low-priority leftover.
