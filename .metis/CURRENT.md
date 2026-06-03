# Current session handoff

## What happened

A verification + reconcile session — no new feature code. **Closed two open questions** carried from the real-`MainAgent`-adapter slice:

1. **Live path proven.** Ran the `#[ignore]`d `live_generate_smoke` against both provider arms — `gpt-5-mini` (OpenAI, 29s) and `claude-haiku` (Anthropic, 64s), both green. This confirmed the previously-doc-only OpenAI shapes (`max_completion_tokens`, `gpt-5`/`gpt-5-mini` ids, strict `json_schema`) **and** the parity-added Anthropic tool `strict: true` flag (no 400). Then exercised the `spawn_blocking` runtime-in-runtime guard via the live GUI (`tauri dev` → "Generate report"): report rendered, no panic, and **persistence confirmed** — real `reports` row (both split axes `risk_posture`+`market_cycle`, `markdown_path`, `summary_json`) + canonical `.md` file.
2. **Retrospective duplication reconciled.** `report-structure.md` had retrospective content in two homes; removed the orphan clause from `## Market Signal Thesis` so the standalone `## Retrospective Audit` is the single home — doc caught up to code (no impl change). Commit **`ab797a3` (pushed)**. Last session's **`6d85967` is now pushed** too.

## Current state

Adapter slice is now **fully verified live**, end to end through the real UI. Working tree clean; nothing in flight. Keys live **outside the repo** at `~/.config/market-signal/keys.env` (sourced per-command, never exported); the live smoke and GUI path only fire with `--ignored`+keys or an app launch — routine `cargo test`/CI never trigger them. DB + `reports/` live under the bundle-id app-data dir `~/Library/Application Support/com.georgesarantinos.market-signal/`. Smoke/keys/DB-path details saved to agent memory ([[live-model-smoke]]).

Deferred slices (carried): **execution gate**; **HTML persistence + PDF export**; **`list_reports`** command; **FMP/FRED/BLS data-source adapters**.

## Open questions

- **Env-slug vs display-name drift.** Config slugs (`claude-opus`, `gpt-5-mini`) differ from `docs/configuration.md` display names — align when the Settings store lands. *(carried)*
- **HTML-persistence path (Step 17)** — how rendered HTML returns to the backend for SQLite; lands with the HTML/PDF slice. *(carried)*
- **Scheduler tech** — `tokio`-timer + tray-resident weekly job; not built. *(carried)*
- **UTC-vs-local report date.** `created_at` + the `YYYY-MM-DD` filename are `chrono::Utc::now()`-derived, so an evening-local run dates a day ahead (our 17:31-local run → `2026-06-03`). Spec frames dates as local; the scheduled 9 AM-local job never trips it. Decide local-vs-UTC with the scheduler slice. ([[utc-vs-local-report-date]])

## Where to start

Run `/metis-plan-task` for the **execution gate** — five-category pre-run validation (four agent models configured; both OpenAI + Anthropic tokens; Tavily + FMP credentials; network reachable) wired ahead of `generate_report_manual`, plus the five-category Persistent Warning Area. It now stands on a verified call path; the live run also gave a concrete shape for the `from_env()` "missing model/key → plain error" path the gate replaces with structured validation.
