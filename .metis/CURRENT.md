# Current session handoff

## What happened

**Shipped in-loop offline HTTP coverage for `http_retry::send_with_retry` to `main` (`3ce9534`, squash-merged + pushed).** The retry/backoff/body-reread *loop* was previously exercised only by the live `#[ignore]`d API smokes — its own tests covered just the pure `is_retryable`/`backoff` helpers. Added **5 round-trip tests** driving the loop against a **hand-rolled `std::net::TcpListener` mock** (a `MockHttp` harness in the test module; no new crate): retry-to-success (+ the `retry_after_of` parse path), non-retryable passthrough, exhaustion-returns-the-*last*-attempt (distinct `down 1/2/3` bodies make the assertion discriminate), dropped-body re-read (`:99-103`), and transport-error exhaustion (`:109-116`). Not `#[ignore]`d — they run in the normal `cargo test` loop (~3s wall-clock, parallel). **Two non-obvious lessons worth carrying:** (1) `send_with_retry(label, build)`'s caller-supplied `build` closure lets a test point at `127.0.0.1` with **zero production change** — no adapter base-URL injection needed. (2) The dropped-body reply is served as **HTTP 200 deliberately**, so a non-retryable status can't mask the bug — the *only* path to a 2nd attempt is the `resp.text()` Err branch, which is exactly what the test pins. Two forks settled with the user up front: hand-rolled `TcpListener` over wiremock/httpmock (anti-bloat ethos), and `http_retry`-loop-only scope. Review verdict was *approve-with-nits*; both nits (TOCTOU comment on the transport test, the `DropBody`-200 rationale) folded in before merge.

## Current state

HEAD is **`3ce9534`**, working tree clean, in sync with `origin/main`. **Nothing in flight — the feature is complete.** The full plan→implement→review→ship loop closed this session. Only `src-tauri/src/http_retry.rs` changed (additive, +250 lines inside `#[cfg(test)]`).

## Open questions

- *(narrowed this session, not closed)* The "wiremock / in-loop offline gap" now has coverage for the **`http_retry` loop**; the broader half — **per-adapter request/response round-trips** behind a base-URL injection seam — stays **deliberately deferred** (each adapter's `interpret_response(status, body)` parsing is already fixture-covered offline, so marginal value is low; the live wires remain the only coverage of the actual adapter→retry→interpret wiring).
- *(carried)* `fmp_baseline_smoke` unrun since quota reset (likely runnable now); the **conditional GPT-5-mini extraction stage** is the largest remaining *reserved, not built* feature; esbuild/vite advisory parked; wiremock-vs-live note above.
- *(low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide.

## Where to start

Nothing owed — the http_retry offline coverage shipped this session. Pick the next carried item: the **conditional GPT-5-mini extraction stage** is the meatiest reserved feature; or extend this session's thread with **per-adapter offline round-trips** (the deferred half — needs a base-URL injection seam on the adapters); or run `fmp_baseline_smoke` now that quota has reset.
