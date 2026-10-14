//! Shared retry-with-backoff for the HTTP data adapters: bounded exponential
//! backoff on a 429, a 5xx, or a transport error (including a body dropped
//! mid-read), leaving each adapter's `interpret_response` the final call on
//! the last attempt. Provider limits that arrive as an HTTP 200 body (FMP's
//! `{"Error Message": …}`, BLS's `REQUEST_NOT_PROCESSED`) are deliberately not
//! retried here: they signal quota, key or plan conditions a seconds-scale
//! retry can't clear, and body semantics stay in the adapters. GDELT is not
//! routed through this: its escalating IP lockout makes retrying a 429
//! actively harmful. Every guarded request is an idempotent read.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::{RequestBuilder, Response};

/// One failure class's total attempts and exponential base: the wait after
/// attempt `k` is `base × 2^(k−1)`. No jitter: a single on-demand client is
/// not a thundering herd.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub attempts: u32,
    pub base: Duration,
}

/// Split by failure class because the right total wait differs by an order of
/// magnitude: riding out a per-minute rate window needs a ladder that crosses
/// the minute boundary, while a down server should fail fast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Governs HTTP 429.
    pub rate_limit: Schedule,
    /// Governs 5xx, transport errors, and dropped-mid-body reads.
    pub server_error: Schedule,
    /// Ceilings a server-supplied `Retry-After`, so a hostile or mistaken
    /// header can't park the whole scan behind one request.
    pub retry_after_cap: Duration,
}

impl RetryPolicy {
    /// Total sleep a fully-exhausted rate-limit ladder incurs, so a provider
    /// policy can pin its window-crossing guarantee in a test without sleeping.
    pub fn cumulative_rate_limit_backoff(&self) -> Duration {
        (1..self.rate_limit.attempts)
            .map(|attempt| self.rate_limit.base * 2u32.pow(attempt - 1))
            .sum()
    }
}

/// Used by every adapter that doesn't pass its own.
pub const DEFAULT_RETRY: RetryPolicy = RetryPolicy {
    rate_limit: Schedule {
        attempts: 3,
        base: Duration::from_secs(1),
    },
    server_error: Schedule {
        attempts: 3,
        base: Duration::from_secs(1),
    },
    retry_after_cap: Duration::from_secs(30),
};

/// Bounds cancel latency during a long (up to 32s) rate-limit wait.
const ABORT_POLL: Duration = Duration::from_millis(250);

pub fn is_retryable(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// `attempt` is 1-based: the wait after attempt 1 fails, before attempt 2. A
/// server `Retry-After` overrides only when *longer* than the exponential
/// default, capped by `cap`.
fn backoff(base: Duration, attempt: u32, retry_after: Option<Duration>, cap: Duration) -> Duration {
    let exp = base * 2u32.pow(attempt - 1);
    match retry_after {
        Some(ra) if ra > exp => ra.min(cap),
        _ => exp,
    }
}

/// Returns `true` if aborted before the wait completed. Also the streaming
/// agents' retry pause (`model_agent::retry_stream_once`).
pub(crate) fn sleep_abortable(total: Duration, abort: Option<&dyn Fn() -> bool>) -> bool {
    let Some(abort) = abort else {
        std::thread::sleep(total);
        return false;
    };
    let deadline = std::time::Instant::now() + total;
    loop {
        if abort() {
            return true;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        std::thread::sleep((deadline - now).min(ABORT_POLL));
    }
}

/// Whole seconds only; the HTTP-date form is ignored (the providers we hit
/// send seconds). Also read by the streaming agents' retry (`model_agent`).
pub(crate) fn retry_after_of(resp: &Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

pub fn send_with_retry(label: &str, build: impl Fn() -> RequestBuilder) -> Result<(u16, String)> {
    send_with_retry_policy(label, &DEFAULT_RETRY, None, build)
}

/// Returns the last attempt's `(status, body)` regardless, so the caller's
/// `interpret_response` still decides fatal-vs-skip. `build` produces a fresh
/// request each attempt (a `RequestBuilder` is consumed by `send`). The
/// attempt counter is shared across classes: each failure's own class decides
/// whether another attempt remains, so a 5xx arriving mid-ladder stops as
/// soon as the server-error budget is spent. On `abort` the loop returns the
/// last attempt as-is; the caller's own cancellation check routes the cancel,
/// keeping this layer ignorant of cancellation semantics. Runs on a blocking
/// thread, so `std::thread::sleep` parks no async runtime.
pub fn send_with_retry_policy(
    label: &str,
    policy: &RetryPolicy,
    abort: Option<&dyn Fn() -> bool>,
    build: impl Fn() -> RequestBuilder,
) -> Result<(u16, String)> {
    let mut attempt = 1;
    loop {
        match build().send() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if is_retryable(status) {
                    let class = if status == 429 {
                        policy.rate_limit
                    } else {
                        policy.server_error
                    };
                    if attempt < class.attempts {
                        let wait = backoff(
                            class.base,
                            attempt,
                            retry_after_of(&resp),
                            policy.retry_after_cap,
                        );
                        // The one place ladder engagement is observable: the
                        // tracker's request rows can't see retries (one row per
                        // logical request).
                        eprintln!(
                            "[http-retry] {label}: HTTP {status} on attempt {attempt}, \
                             backing off {}ms",
                            wait.as_millis()
                        );
                        if !sleep_abortable(wait, abort) {
                            attempt += 1;
                            continue;
                        }
                        // Aborted mid-backoff: fall through and return this
                        // attempt's (status, body) unretried.
                    }
                }
                // A connection dropped mid-body is as transient as a failed `send`,
                // so it retries the same way.
                match resp.text() {
                    Ok(body) => return Ok((status, body)),
                    Err(_) if attempt < policy.server_error.attempts => {
                        let wait = backoff(
                            policy.server_error.base,
                            attempt,
                            None,
                            policy.retry_after_cap,
                        );
                        eprintln!(
                            "[http-retry] {label}: response body dropped on attempt {attempt}, \
                             backing off {}ms",
                            wait.as_millis()
                        );
                        if sleep_abortable(wait, abort) {
                            return Err(anyhow::anyhow!(
                                "aborted while retrying a dropped {label} response body"
                            ));
                        }
                        attempt += 1;
                        continue;
                    }
                    Err(e) => {
                        return Err(e).with_context(|| format!("reading {label} response body"))
                    }
                }
            }
            Err(e) => {
                // FMP and FRED carry their API keys as query params, and a reqwest
                // transport error's Display includes the full URL, so an unstripped
                // error leaks the credential into stderr, the tracker-row detail,
                // and the persisted `job_runs.detail`.
                let e = e.without_url();
                if attempt < policy.server_error.attempts {
                    let wait = backoff(
                        policy.server_error.base,
                        attempt,
                        None,
                        policy.retry_after_cap,
                    );
                    eprintln!(
                        "[http-retry] {label}: transport error on attempt {attempt}, backing \
                         off {}ms ({e})",
                        wait.as_millis()
                    );
                    if sleep_abortable(wait, abort) {
                        return Err(e).with_context(|| format!("sending {label} request"));
                    }
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

    const BASE: Duration = Duration::from_secs(1);
    const CAP: Duration = Duration::from_secs(30);

    #[test]
    fn backoff_is_exponential_and_retry_after_aware() {
        assert_eq!(backoff(BASE, 1, None, CAP), Duration::from_secs(1));
        assert_eq!(backoff(BASE, 2, None, CAP), Duration::from_secs(2));
        assert_eq!(
            backoff(BASE, 1, Some(Duration::from_secs(5)), CAP),
            Duration::from_secs(5)
        );
        // A shorter Retry-After does not shrink the backoff.
        assert_eq!(
            backoff(BASE, 2, Some(Duration::from_secs(1)), CAP),
            Duration::from_secs(2)
        );
        assert_eq!(backoff(BASE, 1, Some(Duration::from_secs(9999)), CAP), CAP);
    }

    #[test]
    fn cumulative_rate_limit_backoff_sums_the_ladder() {
        // 3 attempts at 1s base: waits of 1s + 2s.
        assert_eq!(
            DEFAULT_RETRY.cumulative_rate_limit_backoff(),
            Duration::from_secs(3)
        );
    }

    #[test]
    fn a_transport_error_never_carries_the_query_string() {
        let client = reqwest::blocking::Client::new();
        let err = send_with_retry_policy(
            "FMP",
            &RetryPolicy {
                rate_limit: Schedule {
                    attempts: 2,
                    base: Duration::from_millis(1),
                },
                server_error: Schedule {
                    attempts: 2,
                    base: Duration::from_millis(1),
                },
                retry_after_cap: Duration::from_millis(10),
            },
            None,
            // Unroutable port: connection refused on every attempt.
            || client.get("http://127.0.0.1:1/quote?apikey=sekrit-value"),
        )
        .unwrap_err();
        let chain = format!("{err:#}");
        assert!(!chain.contains("sekrit-value"), "{chain}");
        assert!(!chain.contains("apikey"), "{chain}");
        assert!(chain.contains("sending FMP request"), "{chain}");
    }

    /// Millisecond-scale so the class-split and abort tests run fast; the real
    /// sleeps stay covered by the default-policy tests below.
    fn fast_policy(rate_limit_attempts: u32, server_error_attempts: u32) -> RetryPolicy {
        RetryPolicy {
            rate_limit: Schedule {
                attempts: rate_limit_attempts,
                base: Duration::from_millis(1),
            },
            server_error: Schedule {
                attempts: server_error_attempts,
                base: Duration::from_millis(1),
            },
            retry_after_cap: Duration::from_millis(50),
        }
    }

    #[test]
    fn rate_limit_class_outlasts_the_server_error_budget() {
        // Four 429s then a 200: five attempts under rate_limit.attempts=5, where
        // server_error would have stopped at two.
        let replies: Vec<Canned> = (0..4)
            .map(|_| Canned::Reply {
                status: 429,
                headers: vec![],
                body: "limit",
            })
            .chain(std::iter::once(Canned::Reply {
                status: 200,
                headers: vec![],
                body: "ok",
            }))
            .collect();
        let server = MockHttp::serve(replies);
        let client = reqwest::blocking::Client::new();
        let url = server.base_url.clone();
        let (status, body) =
            send_with_retry_policy("test", &fast_policy(5, 2), None, || client.get(url.as_str()))
                .expect("ladder reaches success");
        assert_eq!((status, body.as_str()), (200, "ok"));
        assert_eq!(server.attempts(), 5);
    }

    #[test]
    fn a_server_error_stops_at_its_own_class_budget() {
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
        ]);
        let client = reqwest::blocking::Client::new();
        let url = server.base_url.clone();
        let (status, body) =
            send_with_retry_policy("test", &fast_policy(5, 2), None, || client.get(url.as_str()))
                .expect("exhaustion returns Ok(last attempt)");
        assert_eq!((status, body.as_str()), (503, "down 2"));
        assert_eq!(server.attempts(), 2);
    }

    #[test]
    fn abort_stops_retrying_and_returns_the_last_attempt() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 429,
                headers: vec![],
                body: "limit",
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "never reached",
            },
        ]);
        let client = reqwest::blocking::Client::new();
        let url = server.base_url.clone();
        let abort = || true;
        let (status, body) = send_with_retry_policy("test", &fast_policy(5, 2), Some(&abort), || {
            client.get(url.as_str())
        })
        .expect("abort returns the last attempt");
        assert_eq!((status, body.as_str()), (429, "limit"));
        assert_eq!(server.attempts(), 1, "abort must prevent the retry");
    }

    #[test]
    fn abort_mid_backoff_returns_promptly_without_a_second_request() {
        // The abort arms only after the 8s backoff sleep has begun, pinning the
        // chunked polling: an implementation that checked abort once upfront
        // and slept the whole wait would blow the elapsed bound.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 429,
                headers: vec![],
                body: "limit",
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "never reached",
            },
        ]);
        let client = reqwest::blocking::Client::new();
        let url = server.base_url.clone();
        let policy = RetryPolicy {
            rate_limit: Schedule {
                attempts: 2,
                base: Duration::from_secs(8),
            },
            server_error: Schedule {
                attempts: 2,
                base: Duration::from_secs(8),
            },
            retry_after_cap: Duration::from_millis(50),
        };
        let armed = std::time::Instant::now();
        let abort = move || armed.elapsed() > Duration::from_millis(300);
        let (status, body) = send_with_retry_policy("test", &policy, Some(&abort), || {
            client.get(url.as_str())
        })
        .expect("abort returns the last attempt");
        let elapsed = armed.elapsed();
        assert_eq!((status, body.as_str()), (429, "limit"));
        assert_eq!(server.attempts(), 1, "abort must prevent the second request");
        assert!(
            elapsed < Duration::from_secs(4),
            "abort mid-backoff must return promptly, not sleep out the full 8s wait (elapsed {elapsed:?})"
        );
    }

    // ---- Default-policy round trips against a localhost socket ----
    //
    // Not `#[ignore]`d: they incur the real default-policy sleeps (1s, then
    // 2s), but cargo runs tests in parallel, so the suite's added wall-clock
    // is the slowest single case, not their sum.

    #[test]
    fn retries_past_a_retryable_status_to_success() {
        // The 429 carries `Retry-After` so `retry_after_of`'s parse path runs.
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
        assert_eq!(
            body, "down 3",
            "the last attempt's body must be the one returned"
        );
        assert_eq!(
            server.attempts(),
            DEFAULT_RETRY.server_error.attempts as usize
        );
    }

    #[test]
    fn rereads_the_body_after_a_dropped_connection() {
        // The first reply declares 100 bytes but sends 4 then closes, so
        // `Response::text()` errors.
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
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        // Dropped so the port refuses connections. The OS could reassign the
        // ephemeral port before the client connects, but on localhost that is
        // vanishingly rare, and the consequence is a flaky failure, never a
        // false pass.
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
