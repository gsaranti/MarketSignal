# Current session handoff

## What happened

**Planned, implemented, reviewed, and shipped per-adapter offline round-trip coverage behind a base-URL injection seam** — the most substantive carried thread. Each of the five retry-routed adapters (FMP, FRED, BLS, Tavily, fmp_news) gained a **test-only** `#[cfg(test)] with_base_url` seam: a private `base_url` field defaulting to the production const, with the `*_URL` consts split into a `*_BASE` origin + `/path` suffixes joined in each request helper — **no production API surface added**. The localhost mock server (`MockHttp`/`Canned`) was lifted out of `http_retry.rs`'s test module into a shared `#[cfg(test)] mod test_http`; it now **records each request's target** and exposes `request_targets()` / `request_paths()`. **11 round-trip tests** (happy-path + one error-status per adapter) drive the full wire path — URL build → `send_with_retry` → `interpret_response` → domain output — offline, asserting the **exact** endpoint path (`assert_eq!(request_paths(), […])`) and, where it varies, the per-call query var reaching the wire. Single-reply, non-retryable error statuses, so **no `BASE_BACKOFF` sleeps**; retry mechanics stay in `http_retry`. **GDELT excluded by design** (single-shot fail-soft). Reviewed by `metis-task-reviewer` (**approve**) plus **two external Codex rounds**, both addressed: round 1 — mock discarded the request → added target recording; round 2 — `starts_with` allowed a suffix regression (`/quote-v2`) → tightened to exact `request_paths` `assert_eq!`. Squash-merged to **`main` @ `481c60e`**, pushed to `origin/main`, feature branch deleted.

## Current state

**Clean and shipped.** HEAD = `481c60e`, working tree clean, `main` in sync with `origin/main`. Verified green: `cd src-tauri && cargo test` (338 lib tests incl. the 11 new round-trips + all integration) and `cargo clippy --all-targets --all-features` (warning-free). Nothing in flight.

## Open questions

- *(carried)* Truncation telemetry table (`document_truncations`, `main` @ `d7eb644`) **ships but has no reader/UI consumer** — only inspection path is raw SQL. The deferred GPT-5-mini extraction-stage revisit waits on the table **accumulating real evidence** that overflow is common (not on more code); an independent higher cap is the one-line follow-up only if that history must outlive the 30-report cascade window.
- *(carried, low)* FMP free `industries`-P/E wire noise — decide whether to clamp/flag implausible P/E values (e.g. pe=461) or leave them to the agent's judgment.
- *(carried, low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide; esbuild/vite advisory.

## Where to start

Nothing owed — `main` is clean and pushed, and `BUILD.md` is updated to mark the base-URL seam + `test_http` mock + per-adapter offline round-trips **shipped** (`main` @ `481c60e`, in both the adapters bullet and the testing section). The per-adapter offline round-trip thread is now **resolved** (was the most substantive carried item). Pick a next slice from the carried low-priority list (FMP `industries`-P/E clamp; FRED freshness tuning) or the truncation-telemetry reader/UI consumer.

