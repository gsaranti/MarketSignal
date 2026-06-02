# Current session handoff

## What happened

Planned → implemented → reviewed → committed the **real `MainAgent` adapter** slice (scoped deliberately to the adapter *only*, no execution gate). Swapped `StubMainAgent` for `ModelMainAgent` (`model_agent.rs`): Anthropic forced `tool_use` and OpenAI strict `json_schema` both feed one `ResponseEnvelope`; the **app layer mints `report_id`/`report_type`/`created_at`** so a model-fabricated timestamp can't reach the pipeline's RFC3339 parse. Model + key come from **env this slice** (`MARKET_SIGNAL_MAIN_AGENT_MODEL` + `OPENAI_API_KEY`/`ANTHROPIC_API_KEY`) — no gate; missing config → plain error.

The agent trait + pipeline stayed **synchronous**; the Tauri command became `async` and runs the blocking `reqwest::blocking` call through `spawn_blocking` to avoid a runtime-in-runtime panic. Two reviews: metis = **approve-with-nits** (fixed the blocking-on-tokio risk + a comment nit); a Codex/iris review then caught a fidelity gap both metis and I missed — the canonical `## Retrospective Audit` section was omitted from prompt *and* stub. Fixed both + added `strict: true` to the Anthropic tool for parity. Commits: **`491ad14` (pushed)**, **`6d85967` (local/unpushed)**.

## Current state

Slice **done, reviewed twice, all fixes landed**; `cargo test` green (lib 10 passed / 1 ignored + integration). `6d85967` is local — push when ready. `StubMainAgent` remains for the offline pipeline test. The **live HTTP path is unexercised by `cargo test`** — only the `#[ignore]`d `live_generate_smoke` or a keyed `npm run tauri dev` runs a real round-trip.

Deferred slices (carried forward): **execution gate**; **HTML persistence + PDF export**; **`list_reports`** command; **FMP/FRED/BLS data-source adapters**.

## Open questions

- **Live endpoint unverified.** OpenAI arm uses `max_completion_tokens` + ids `gpt-5`/`gpt-5-mini` (Codex confirmed shapes against live OpenAI docs, but no real call was run); the `spawn_blocking` fix is unexercised on the live path. Retire via `cargo test -- --ignored` or keyed `tauri dev`.
- **Env-slug vs display-name drift.** Config slugs (`claude-opus`, `gpt-5-mini`) differ from `docs/configuration.md` display names ("Claude Opus", "GPT-5 mini") — align when the Settings store lands.
- **For `/metis-reconcile`:** `report-structure.md` describes retrospective content in two places (inside Market Signal Thesis *and* the standalone Retrospective Audit section).
- **HTML-persistence path (Step 17)** — how rendered HTML returns to the backend for SQLite; lands with the HTML/PDF slice. *(carried)*
- **Scheduler tech** — `tokio`-timer + tray-resident weekly job; not built. *(carried)*

## Where to start

Run `/metis-plan-task` for the **execution gate** — the natural follow-on now a real model call exists: five-category pre-run validation (four agent models configured; both OpenAI + Anthropic tokens; Tavily + FMP credentials; network reachable) wired ahead of the command, plus the five-category Persistent Warning Area. Optional first: push `6d85967` and run the keyed live smoke to close the live-path question.
