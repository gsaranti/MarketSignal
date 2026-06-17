# Current session handoff

## What happened

Built the **analyst skills library via progressive disclosure** — the deferred follow-on now that the analyst stage ships. Scoped down via **four decisions made with the user before planning** (the agreed brief, not drift): **consumers — main agent only** (not the three analysts); **mechanism — two-phase** (non-streaming selection call → streaming generation); **content — 3 of 16 skills** authored; **output — skills steer the synthesis prose** (no per-skill output channel). Shipped to `main` @ `8ff881b` (branch → squash-merge → push → branch deleted). New `skills.rs` is the data-driven catalog (frontmatter shown in phase 1, full bodies supplied in phase 2 with an `enum`-constrained selection so the model can't request an unauthored skill); `model_agent.rs` gained the selection call before the existing streaming generation. Selection is **fail-soft** — a failed/cancelled selection degrades to no skills so it never costs the report, the deliberate contrast with the not-fail-soft analyst stage. metis-task-reviewer **approved** (re-ran verification, every criterion passed, scope report honest, no unsurfaced reductions). The mechanism is complete and additive — the other 13 skills are pure `CATALOG` appends.

## Current state

**Shipped and pushed.** `main` = `8ff881b`, working tree clean, nothing in flight. Touched only `skills.rs` (new — `Skill`/`CATALOG` + `frontmatter_catalog`/`catalog_names`/`select_bodies`), `model_agent.rs` (selection schema + per-provider request builders + `format_selected_skills` + `select_skills`/`run_skill_selection`/`call_nonstreaming` + `generate()` wiring + `SYSTEM_PROMPT` directive), and one `lib.rs` module line. `MainAgentInput`, the pipeline, Settings, and the frontend are untouched (selection happens inside `generate()` from data already in the input). Verified green: `cargo test` **389 lib** (377→389, +12) + all integration suites; `cargo clippy --all-targets --all-features` clean. No `npm` (frontend untouched). `#[ignore]`d `live_skill_selection_smoke` not run. **BUILD.md updated this session** — the `agents` module bullet now records the as-built skills mechanism, the fail-soft posture, the no-tracker-row call, and the three deviations.

## Open questions

- *(doc not yet amended)* `docs/analyst-skills.md` still describes skills as serving **analysts + main agent** with a **per-skill output schema**; we shipped **main-agent-only / steer-prose / 3-of-16**. BUILD.md records the deviation; whether to amend the doc itself is open.
- *(deferred)* The remaining **13 of 16 skills** — content-only follow-on appending `CATALOG` entries (mechanism complete). And the **per-skill structured-output channel** — the richer contract steer-prose deferred.
- *(deferred, larger)* Wire skills into the **three analysts** too — the doc's full consumer set, a separate slice if wanted.
- *(carried, low)* `SECTOR_PE_MAX` (120) calibration via live `tuning_sector_pe_distribution_probe`; `cargo fmt` dirty repo-wide + esbuild/vite advisory.

## Where to start

`main` is clean and pushed; BUILD.md is current. Nothing owed — open a fresh direction. Natural next frontiers, in rough order: **author the remaining 13 skills** (pure `CATALOG` additions — the highest-leverage content follow-on), **extend skills to the three analysts**, or **amend `docs/analyst-skills.md`** to match the as-built deviations. The parked `SECTOR_PE_MAX` calibration remains the low-priority leftover.
