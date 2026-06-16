//! Shared retry-with-backoff for the gated HTTP data adapters.
//!
//! The baseline scan fires ~30 sequential requests across FMP, FRED, BLS and
//! Tavily on a once-weekly job; a single transient 429 / 5xx / dropped connection
//! should not fail the whole report. This wraps a request in a bounded exponential
//! backoff that retries the transient *HTTP-status / transport* failures — an HTTP-429
//! rate limit, a 5xx, or a transport error (including a connection dropped mid-body) —
//! leaving every adapter's `interpret_response` to make the final fatal-vs-skip call on
//! whatever the last attempt returns.
//!
//! It does **not** retry provider rate/plan limits that arrive as an HTTP **200** body —
//! FMP's `{"Error Message": …}` and BLS's `REQUEST_NOT_PROCESSED`. Those are classified
//! downstream by each adapter and left deliberately fatal: in practice they signal a
//! daily-quota exhaustion, an invalid key, a plan gate, or a malformed batch — hard
//! conditions a seconds-scale retry can't clear, and the 200 body can't reliably be told
//! apart from a transient burst. Keeping provider body semantics in the adapters, not in
//! this generic layer, is the status/body split the adapters are built on.
//!
//! GDELT is deliberately *not* routed through this: its escalating IP lockout means
//! retrying a 429 is actively harmful, so it keeps its single-shot fail-soft (see
//! `gdelt`). The requests this guards are all idempotent reads (GETs and read-only
//! POST queries), so retrying is safe.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::{RequestBuilder, Response};

/// Max attempts (1 initial + up to 2 retries) and the base backoff. Fixed schedule
/// (no jitter): a single weekly-job client is not a thundering herd, so 1s → 2s is
/// enough to ride out a brief rate limit without pulling in a `rand` dependency.
const MAX_ATTEMPTS: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_secs(1);

/// Ceiling on a server-supplied `Retry-After`, so a hostile or mistaken header can't
/// park the whole scan behind one request.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(30);

/// Whether an HTTP status is worth retrying: a 429 rate limit or any 5xx server
/// error. Everything else — 2xx success, a 4xx contract error, an auth failure — is
/// returned to the caller's `interpret_response` unchanged, since retrying would not
/// change the outcome.
pub fn is_retryable(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// The wait before the next attempt. `attempt` is 1-based (the wait *after* attempt 1
/// fails, before attempt 2): exponential off `BASE_BACKOFF`. A server `Retry-After`
/// overrides only when it is *longer* than the exponential default, capped by
/// `RETRY_AFTER_CAP`.
fn backoff(attempt: u32, retry_after: Option<Duration>) -> Duration {
    let exp = BASE_BACKOFF * 2u32.pow(attempt - 1);
    match retry_after {
        Some(ra) if ra > exp => ra.min(RETRY_AFTER_CAP),
        _ => exp,
    }
}

/// Parse a `Retry-After` header as whole seconds when present and numeric. The
/// HTTP-date form is ignored (the providers we hit send seconds), falling back to the
/// exponential default.
fn retry_after_of(resp: &Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Send a request with bounded retry on retryable failures, returning the final
/// `(status, body)` for the caller to interpret. `build` produces a fresh request
/// each attempt (a `RequestBuilder` is consumed by `send`). A transport error or a
/// retryable status triggers a backoff-and-retry up to `MAX_ATTEMPTS`; the last
/// attempt's result is returned regardless, so the caller's `interpret_response`
/// still decides fatal-vs-skip. `label` names the provider for error context.
///
/// Runs on a blocking thread (the adapters are driven via `spawn_blocking`), so the
/// `std::thread::sleep` between attempts is safe — no async runtime is parked.
pub fn send_with_retry(label: &str, build: impl Fn() -> RequestBuilder) -> Result<(u16, String)> {
    let mut attempt = 1;
    loop {
        match build().send() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if is_retryable(status) && attempt < MAX_ATTEMPTS {
                    let wait = backoff(attempt, retry_after_of(&resp));
                    std::thread::sleep(wait);
                    attempt += 1;
                    continue;
                }
                // Reading the body can still fail on a connection dropped mid-stream — a
                // transient transport error like a failed `send`, so retry it the same way
                // rather than failing a response we could re-fetch.
                match resp.text() {
                    Ok(body) => return Ok((status, body)),
                    Err(_) if attempt < MAX_ATTEMPTS => {
                        std::thread::sleep(backoff(attempt, None));
                        attempt += 1;
                        continue;
                    }
                    Err(e) => {
                        return Err(e).with_context(|| format!("reading {label} response body"))
                    }
                }
            }
            Err(e) => {
                if attempt < MAX_ATTEMPTS {
                    std::thread::sleep(backoff(attempt, None));
                    attempt += 1;
                    continue;
                }
                return Err(e).with_context(|| format!("sending {label} request"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};
    use std::net::TcpListener;

    #[test]
    fn is_retryable_covers_429_and_5xx_only() {
        assert!(is_retryable(429));
        for s in [500, 502, 503, 504, 599] {
            assert!(is_retryable(s), "HTTP {s} should retry");
        }
        for s in [200, 204, 301, 400, 401, 403, 404, 408, 422] {
            assert!(!is_retryable(s), "HTTP {s} should not retry");
        }
    }

    #[test]
    fn backoff_is_exponential_and_retry_after_aware() {
        // Exponential by 1-based attempt: 1s, then 2s.
        assert_eq!(backoff(1, None), Duration::from_secs(1));
        assert_eq!(backoff(2, None), Duration::from_secs(2));
        // A longer Retry-After wins over the exponential default...
        assert_eq!(
            backoff(1, Some(Duration::from_secs(5))),
            Duration::from_secs(5)
        );
        // ...a shorter one does not shrink the backoff...
        assert_eq!(
            backoff(2, Some(Duration::from_secs(1))),
            Duration::from_secs(2)
        );
        // ...and a hostile Retry-After is capped.
        assert_eq!(backoff(1, Some(Duration::from_secs(9999))), RETRY_AFTER_CAP);
    }

    // ---- In-loop offline coverage for the `send_with_retry` round trip ----
    //
    // These exercise the retry/backoff/body-reread loop against a real localhost
    // socket — the one path a live API key was previously the only thing to run.
    // Each test supplies its own `build` closure pointing at a throwaway server, so
    // no adapter's hardcoded endpoint is involved. They are *not* `#[ignore]`d: they
    // run in the normal `cargo test` loop. They do incur the real `BASE_BACKOFF`
    // sleeps (1s, then 2s), but cargo runs tests in parallel, so the suite's added
    // wall-clock is the slowest single case (~3s), not their sum. The localhost mock
    // server (`MockHttp` / `Canned`) lives in `crate::test_http`, shared with the
    // per-adapter offline round-trip tests.

    #[test]
    fn retries_past_a_retryable_status_to_success() {
        // A 429 (carrying `Retry-After`, so `retry_after_of`'s parse path runs) then a
        // 200: the loop must back off, retry, and return the *second* attempt's body.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 429,
                headers: vec![("Retry-After", "0")],
                body: "rate limited",
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "ok body",
            },
        ]);
        let client = reqwest::blocking::Client::new();
        let url = server.base_url.clone();
        let (status, body) =
            send_with_retry("test", || client.get(url.as_str())).expect("retry reaches success");
        assert_eq!(status, 200);
        assert_eq!(body, "ok body");
        assert_eq!(server.attempts(), 2, "should have retried exactly once");
    }

    #[test]
    fn returns_a_non_retryable_status_without_retrying() {
        // A 404 is not retryable: returned immediately, body intact, one attempt only.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 404,
            headers: vec![],
            body: "nope",
        }]);
        let client = reqwest::blocking::Client::new();
        let url = server.base_url.clone();
        let (status, body) = send_with_retry("test", || client.get(url.as_str()))
            .expect("non-retryable status returns Ok");
        assert_eq!(status, 404);
        assert_eq!(body, "nope");
        assert_eq!(server.attempts(), 1, "a 404 must not be retried");
    }

    #[test]
    fn exhausts_attempts_and_returns_the_last_response() {
        // Persistent 503: every attempt fails retryably. The loop must give up after
        // MAX_ATTEMPTS and hand back the final (status, body) — not an Err.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 503,
                headers: vec![],
                body: "down 1",
            },
            Canned::Reply {
                status: 503,
                headers: vec![],
                body: "down 2",
            },
            Canned::Reply {
                status: 503,
                headers: vec![],
                body: "down 3",
            },
        ]);
        let client = reqwest::blocking::Client::new();
        let url = server.base_url.clone();
        let (status, body) = send_with_retry("test", || client.get(url.as_str()))
            .expect("exhaustion returns Ok(last attempt)");
        assert_eq!(status, 503);
        assert_eq!(body, "down 3", "the last attempt's body must be the one returned");
        assert_eq!(server.attempts(), MAX_ATTEMPTS as usize);
    }

    #[test]
    fn rereads_the_body_after_a_dropped_connection() {
        // The first reply declares 100 bytes but sends 4 then closes, so
        // `Response::text()` errors. The loop treats that read failure as transient
        // and retries to a clean 200.
        let server = MockHttp::serve(vec![
            Canned::DropBody {
                content_length: 100,
                partial: "frag",
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "full body",
            },
        ]);
        let client = reqwest::blocking::Client::new();
        let url = server.base_url.clone();
        let (status, body) = send_with_retry("test", || client.get(url.as_str()))
            .expect("body reread reaches success");
        assert_eq!(status, 200);
        assert_eq!(body, "full body");
        assert_eq!(server.attempts(), 2);
    }

    #[test]
    fn retries_then_surfaces_a_transport_error() {
        // Bind a port, learn it, then drop the listener so every connection is
        // refused. The send() error path must retry and finally surface an Err.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        // Drop the listener so the port refuses connections. There is a narrow TOCTOU
        // window — the OS could reassign this ephemeral port before the client connects
        // — but on localhost that is vanishingly rare, and the only consequence would be
        // a flaky failure here, never a false pass.
        drop(listener);
        let url = format!("http://127.0.0.1:{port}/");
        let client = reqwest::blocking::Client::new();
        let result = send_with_retry("test", || client.get(url.as_str()));
        assert!(
            result.is_err(),
            "a persistent transport error must surface as Err"
        );
    }
}
