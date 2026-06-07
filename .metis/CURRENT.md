# Current session handoff

## What happened

**Shipped the live job run tracker — squash-merged to `main` as `6be21ce` (PR #11).** An in-session UI surface (an undocumented feature, outside the 17-step spec) replaces the report pane while a weekly-report run is in flight: per-step progress, **one tracker row per actual HTTP request** (in-flight → pass/fail with reason; short-circuited series emit none; sectors per-date, index-perf per-symbol, BLS one batch), the main agent's report **streamed token-by-token**, and **cancel at any point**. Added a fifth `JobState::Cancelled` (recorded like Skipped — no report, no warning).

**Load-bearing decisions (don't relitigate; full architecture in `.metis/BUILD.md` + `docs/run-tracking.md`):**
- **Tauri-free `progress` seam** (`ProgressReporter` + per-run `RunContext`) injected via `with_context` builders, so the `MarketDataSource`/`MainAgent` trait signatures stay unchanged and a no-op context keeps the spine offline-testable.
- **Token streaming is a pure side-channel** — the full envelope is accumulated and parsed exactly as the non-streaming path, so the (resumable) decoder can't corrupt the report. Don't fold streaming into the structured parse.
- Run log is **in-session, latest-run-only, session-scoped** ("Back to report" keeps it; cleared only by the next run or app quit). Retries are deliberately one logical request.

Reviews: metis-task-reviewer (approve-with-nits) + Codex ×3 rounds, all findings fixed (UI stranded by a history-write failure; a competing run clearing a cancel; missing sector cancel checkpoint; 1:1 rows; a stale log masking a new failure; reports auto-select clobbering a surfaced error).

## Current state

On **`main`** at **`6be21ce`**, merged + pulled, **working tree clean, nothing in flight**, branch deleted. Verified **offline**: `cargo test` (160 lib + 11 integration) + `cargo clippy` clean + `npm run build`. Visually inspected via a dev-only mock-event preview (since removed). **The live OpenAI/Anthropic `stream: true` wire format is NOT verified** — streaming tests use synthetic fixtures only.

## Open questions

- **Tracker live-SSE smoke unrun** — confirm the real OpenAI/Anthropic streamed-response shapes (the `stream_delta` paths) with a `tauri dev` run + real keys before relying on the live token stream.
- *(carried, untouched)* **`COVERAGE_FLOOR = 0.6`** — the Russell "2-of-3 majors" fix is a named must-have set, not a higher constant. **Slice (B)** degraded-but-successful *reader* signal still missing (the tracker shows a *running* job, not a *degraded past report*). **`wiremock` / in-loop gap** offline coverage still deferred. **Step-7 news funnel** never run live (Tavily/OpenAI keys + cool GDELT IP).
- *(low / parked)* filter-prompt snippets; retention-cascade + step-5 auto-archive; calendar `expected` consensus; GDP not annualized; no Vue component-test harness; `cargo fmt` dirty repo-wide.

## Where to start

Forward slice is still **Step 8: research routing** (`/metis-plan-task`) — the fixed Claude Sonnet router turns the 7b clusters + the richer baseline into a bounded research plan. Alternatives: **slice (B)** the degraded-job reader signal (self-contained frontend), the **Step-7 news-funnel live smokes**, or the parked retention-cascade. Quick win regardless: the tracker's **live-SSE smoke** above.
