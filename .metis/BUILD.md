# BUILD — Market Signal

## What we're building

Market Signal is a local-first macOS desktop app (Tauri 2 / Rust backend, Vue 3 frontend) that runs one recurring job — a **Weekly Market Report** generated every Sunday 9:00 AM local — and presents an evolving, professional market thesis rather than reactive daily commentary. A deterministic Rust pipeline gathers market data, macro data, and news; a constrained set of LLM "agents" reason over a curated packet to produce a Markdown report; the app renders it to HTML for display/PDF and keeps long-term continuity through vector memory. Everything runs on the user's machine except external API/model calls. The repo today is the stock Tauri+Vue scaffold (`src-tauri/src/lib.rs:2-5` is still the `greet` demo); this brief describes the system to build on top of it.

## The load-bearing decision: the app layer orchestrates; agents are pure stages

The single call the rest of the architecture is most sensitive to is the **boundary between the deterministic application layer and the agents**. Per `docs/agents.md`: *"The pipeline is not tool-driven by the main agent"* and *"The application layer executes external API calls and deterministic data retrieval."* Agents never touch the network, the database, or the filesystem. Each agent stage is a pure function: **structured input → structured output**. The Rust app layer owns the entire 17-step control flow (`docs/weekly-report-workflow.md`), all I/O, all limits, and all persistence; agents only consume the packet handed to them and emit schema-conformant results.

Commit to this as the spine. Concretely:

- Every agent stage sits behind a Rust trait (e.g. `MainAgent`, `AnalystAgent`, `HeadlineFilter`, `ResearchRouter`) whose method takes a typed request and returns a typed, validated response. The model HTTP call is an implementation detail of the adapter, swappable for a deterministic stub. **These trait methods are synchronous** — agent stages are pure `fn`s; the blocking provider HTTP call (`reqwest::blocking`) is offloaded via `spawn_blocking` at the application-layer seam (the Tauri command), so `tokio`/async lives only in app-layer I/O — the bounded research executor and the scheduler — never in the agent trait itself. (Established by the real-adapter slice; see `src-tauri/src/model_agent.rs` and `generate_report_manual` in `lib.rs`.)
- **Research planning is the router's job, not the main agent's** (resolved in walk C1): the fixed routing model produces the executable research plan at Step 8; the application layer executes it (Step 9); the main agent only shapes research indirectly through the inputs it surfaces, then builds the condensed packet at Step 10. Downstream code must not give the main agent a live tool loop.
- Research execution is hard-bounded — *"maximum 50 research requests per job ... maximum duration of 30 minutes for the research phase ... maximum dynamic-branching depth of 2"* (`docs/weekly-report-workflow.md §Step 9`). These bounds live in the executor, not the model.

Why it's load-bearing: this boundary decides the module graph, the testing strategy (agents become offline-stubbable pure functions), the data contracts (the research packet and each analyst's output schema are the API between halves), and the safety model (no unbounded agent I/O). If it were instead a tool-calling agent driving the run, the data model, the limits, and the test approach would all be different.

## Data model & storage

Three stores, by responsibility (`docs/storage.md`):

- **Filesystem** — canonical Markdown reports, named `YYYY-MM-DD-market-signal-weekly-report.md` (`docs/export.md §Export Naming`), plus the `/research-inbox` and `/research-archive` folders.
- **SQLite** — report records, report metadata, generated HTML, job history, warning states. The structured **report-summary metadata** is a JSON object the main agent populates; required fields are `report_id` (UUID), `report_type` (always `weekly_market`), `created_at` (ISO-8601), **`risk_posture`** ∈ {risk-on, risk-off, mixed}, **`market_cycle`** ∈ {late-cycle, recessionary, recovery}, `thesis_stance` ∈ {bullish, bearish, mixed, uncertain}, `header_summary_bullets` (3–6). The regime field was split into the two orthogonal axes `risk_posture`/`market_cycle` in walk Q2 — schema code must carry both, not a single `market_regime`. Optional arrays: `key_risks`, `unresolved_questions`, `forward_outlook_themes`.
- **LanceDB** — long-term semantic memory: one embedding per report summary and one per durable learning (`text-embedding-3-large`), each an atomic unit (no chunking). Retention asymmetry is deliberate and must be honored in deletion code: only the most recent **30 reports** are kept (deleting a report cascades its Markdown, HTML, metadata, and vector *summary* reference together), **but durable learnings survive report deletion**.

One deliberate exception to *persisted config lives in SQLite*: the **Light/Dark appearance** preference is stored in webview `localStorage`, not `app_settings` — it is pure presentation with no backend consumer (agents never see HTML; PDF export reuses the same themed DOM), and a synchronous pre-mount read in `main.ts` avoids a first-paint theme flash. `src/theme.ts` is its only writer.

## Module boundaries

- **`app` (Rust orchestrator)** — the 17-step pipeline, the bounded research executor, the scheduler, validation/gating, and warning-state management. This is where determinism lives.
- **`adapters` (Rust)** — `data_sources` (FMP/FRED/BLS REST via reqwest — FMP serves equity-market data (indices, VIX, gold, sector performance, company financials), FRED the macro series including the **dollar index, commodities (oil, natural gas), and Treasury yields**, BLS labor; data access stays in Rust, no OpenBB/Python sidecar; Tavily + GDELT for news) and `models` (OpenAI + Anthropic HTTP). Live-verified 2026-06-05: FMP's free tier gates the dollar index / oil / gas behind premium, which is why those market-internal series moved to FRED. Fixed internal stages are pinned here: GPT-5 mini (headline filtering, data extraction), Claude Sonnet (research routing), `text-embedding-3-large` (embeddings) — non-configurable, distinct from the four user-selectable agent models.
- **`agents` (prompt + schema contracts)** — main agent, Bull/Bear/Balanced analysts (run **concurrently**, no ordering dependency — walk Q6), and the **16 analyst skills as a shared library** surfaced by progressive disclosure: agents first receive each skill's frontmatter, then request the full skill (prompt + output schema) on demand from the packet (walk Q1).
- **`frontend` (Vue 3)** — Latest Report View, Recent Reports Sidebar, Research Documents, Persistent Warning Area, Settings (`docs/interface.md`). All UI is built against the design system in `market-signal-design-system/` (its `SKILL.md` and tokens are binding per project CLAUDE.md). Markdown→HTML rendering uses **markdown-it**, which is JS — so HTML generation lives on the frontend/webview side; the backend persists the rendered HTML. Agents never see HTML.

The existing scaffold gives us exactly one seam to extend: the Tauri command boundary (`invoke_handler` at `src-tauri/src/lib.rs:11`). Cargo deps to add: a SQLite layer (`rusqlite` or `sqlx`), `lancedb`, `reqwest`, `tokio`, `uuid`, `chrono`, and the Tauri tray/notification features.

## Scheduling & runtime

A tray-resident app (closing the window must not quit it) runs a Rust timer that fires the weekly job at Sunday 9 AM local. Honesty about limits is part of the spec: jobs run only while the app is running; sleep ⇒ **missed** (never retroactively replayed); offline/unreachable provider ⇒ **failed**; a second concurrent run ⇒ **skipped** (single workflow at a time). Missed detection happens on next open by comparing last-run against the expected window. Manual execution reuses the identical workflow and validation.

The **execution gate** (`docs/configuration.md`, `docs/weekly-report-workflow.md §Step 1`) blocks any run until: all four agent models are configured; **both** OpenAI and Anthropic tokens exist (always required — the fixed internal stages span both providers, walk C2); the required external credentials (Tavily, FMP, and FRED) are present; and the network is reachable. Failures surface in the **Persistent Warning Area**, which now has **five** de-duplicating categories (walk Q4): missing agent configuration, missing API tokens, missing provider credentials, failed jobs, missed scheduled jobs.

## Testing approach

The spine makes the pipeline testable offline: because agents and data adapters are traits, the orchestrator can be driven end-to-end against deterministic stubs and fixture packets, with no live keys. Cover: the bounded-research executor's three limits; the 30-report retention cascade *and* durable-learning survival; the validation gate's pass/block matrix; missed-vs-failed-vs-skipped state transitions; and research-inbox **fail-soft** parsing (an unparseable file is skipped + logged, left in the inbox, job continues — walk Q5). Real-provider adapters get thin integration tests behind a feature flag; UI components get component tests against the design system.

## First vertical slice: manual report, stub agent, end to end

The thinnest runnable pass that exercises the load-bearing boundary without touching the financial-data adapters or live API keys:

1. **One Tauri command** — replace the scaffold `greet` with `generate_report_manual` in `src-tauri/src/lib.rs`, wired into `invoke_handler`.
2. **One agent stage, stubbed** — a `MainAgent` trait with a deterministic stub impl that returns a fixed structured result: a small valid report Markdown body plus a report-summary object (`report_id`, `report_type="weekly_market"`, `created_at`, `risk_posture`, `market_cycle`, `thesis_stance`, `header_summary_bullets`). This proves the structured-in/out contract that the real adapters will later satisfy.
3. **One database write + one file write** — create the SQLite `reports` table and insert the record + summary JSON; write the canonical `YYYY-MM-DD-market-signal-weekly-report.md` to the reports directory.
4. **One screen** — the Vue **Latest Report View** calls the command, receives the Markdown, and renders it via markdown-it (styled with design-system tokens).
5. **One passing test** — a Rust integration test that invokes the pipeline with the stub and asserts both the SQLite row (with `risk_posture` + `market_cycle` populated) and the `.md` file exist.

This slice runs offline, instantiates the app→agent-stage→persist→render flow that the whole system is built on, and turns the architecture's central bet (deterministic orchestration around a pure agent stage) into something executable. The immediate next slices: swap the stub for a real OpenAI/Anthropic `MainAgent` adapter (and the config/token gate); persist generated HTML + add PDF export; then build out the data-source adapters (FMP/FRED/BLS REST).
