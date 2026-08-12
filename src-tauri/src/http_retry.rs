//! Shared retry-with-backoff for the gated HTTP data adapters.
//!
//! The baseline scan fires ~30 sequential requests across FMP, FRED, BLS and
//! Tavily on each report run; a single transient 429 / 5xx / dropped connection
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

/// One failure-class schedule: how many total attempts the class allows and the
/// exponential base for the waits between them (wait after attempt `k` is
/// `base × 2^(k−1)`). Fixed schedule (no jitter): a single on-demand client is not
/// a thundering herd, so exponential doubling is enough without a `rand` dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub attempts: u32,
    pub base: Duration,
}

/// Per-provider retry policy, split by failure class: `rate_limit` governs HTTP
/// 429, `server_error` governs 5xx, transport errors, and dropped-mid-body reads.
/// The split exists because the right total wait differs by an order of magnitude —
/// riding out a per-minute rate window needs a ladder that crosses the minute
/// boundary, while a genuinely-down server should fail fast rather than stall a
/// multi-request scan behind minute-long waits (FMP's policy lives in `fmp.rs`).
/// `retry_after_cap` ceilings a server-supplied `Retry-After`, so a hostile or
/// mistaken header can't park the whole scan behind one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub rate_limit: Schedule,
    pub server_error: Schedule,
    pub retry_after_cap: Duration,
}

impl RetryPolicy {
    /// Total sleep a fully-exhausted rate-limit ladder incurs — the sum of every
    /// wait between attempts. Lets a provider policy pin its window-crossing
    /// guarantee in a test without sleeping it.
    pub fn cumulative_rate_limit_backoff(&self) -> Duration {
        (1..self.rate_limit.attempts)
            .map(|attempt| self.rate_limit.base * 2u32.pow(attempt - 1))
            .sum()
    }
}

/// The default policy — exactly the pre-policy behavior, used by every adapter
/// that doesn't pass its own: 3 attempts, 1s exponential base (1s → 2s waits)
/// for both classes, 30s `Retry-After` cap.
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

/// How often an abortable backoff sleep polls the abort closure. Bounds cancel
/// latency during a long (up to 32s) rate-limit wait.
const ABORT_POLL: Duration = Duration::from_millis(250);

/// Whether an HTTP status is worth retrying: a 429 rate limit or any 5xx server
/// error. Everything else — 2xx success, a 4xx contract error, an auth failure — is
/// returned to the caller's `interpret_response` unchanged, since retrying would not
/// change the outcome.
pub fn is_retryable(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// The wait before the next attempt. `attempt` is 1-based (the wait *after* attempt 1
/// fails, before attempt 2): exponential off the class schedule's base. A server
/// `Retry-After` overrides only when it is *longer* than the exponential default,
/// capped by the policy's `retry_after_cap`.
fn backoff(base: Duration, attempt: u32, retry_after: Option<Duration>, cap: Duration) -> Duration {
    let exp = base * 2u32.pow(attempt - 1);
    match retry_after {
        Some(ra) if ra > exp => ra.min(cap),
        _ => exp,
    }
}

/// Sleep `total`, polling `abort` in ~250ms slices; returns `true` if aborted
/// before the wait completed. Without an abort closure it is one plain sleep.
fn sleep_abortable(total: Duration, abort: Option<&dyn Fn() -> bool>) -> bool {
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

/// Send a request with the default policy — the pre-policy behavior, unchanged for
/// every adapter that has no provider-specific schedule.
pub fn send_with_retry(label: &str, build: impl Fn() -> RequestBuilder) -> Result<(u16, String)> {
    send_with_retry_policy(label, &DEFAULT_RETRY, None, build)
}

/// Send a request with bounded retry on retryable failures, returning the final
/// `(status, body)` for the caller to interpret. `build` produces a fresh request
/// each attempt (a `RequestBuilder` is consumed by `send`). A retryable status
/// triggers a backoff-and-retry under its failure class's schedule — 429 rides
/// `policy.rate_limit`, 5xx / transport / dropped-body rides `policy.server_error`
/// — and the last attempt's result is returned regardless, so the caller's
/// `interpret_response` still decides fatal-vs-skip. The attempt counter is shared
/// across classes: each failure's own class decides whether another attempt
/// remains and how long the wait is, so a 5xx arriving mid-ladder stops as soon
/// as the server-error budget is spent. `label` names the provider for error
/// context.
///
/// `abort` (when provided) is polled during backoff sleeps in ~250ms slices; on
/// abort the loop stops retrying and returns the last attempt as-is — the caller's
/// own cancellation boundary check (every adapter polls `is_cancelled()` between
/// requests) is what routes the cancel, keeping this layer ignorant of
/// cancellation semantics.
///
/// Runs on a blocking thread (the adapters are driven via `spawn_blocking`), so the
/// `std::thread::sleep` between attempts is safe — no async runtime is parked.
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
                        // The one place ladder engagement is observable — the
                        // tracker's request rows can't see retries (one row per
                        // logical request), so a ridden 429 ladder was invisible
                        // outside a run row a failed run never writes
                        // (`docs/verification/2026-08-10-big-run-attempt-1.md`
                        // §Residue).
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
                // Reading the body can still fail on a connection dropped mid-stream — a
                // transient transport error like a failed `send`, so retry it the same way
                // rather than failing a response we could re-fetch.
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
                // Strip the URL before the error is printed OR returned: FMP and
                // FRED carry their API keys as query params, and a reqwest
                // transport error's Display includes the full URL — so an
                // unstripped error leaks the credential into stderr, the
                // tracker-row detail, and the persisted `job_runs.detail` its
                // context chain can reach.
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
        // Exponential by 1-based attempt: 1s, then 2s.
        assert_eq!(backoff(BASE, 1, None, CAP), Duration::from_secs(1));
        assert_eq!(backoff(BASE, 2, None, CAP), Duration::from_secs(2));
        // A longer Retry-After wins over the exponential default...
        assert_eq!(
            backoff(BASE, 1, Some(Duration::from_secs(5)), CAP),
            Duration::from_secs(5)
        );
        // ...a shorter one does not shrink the backoff...
        assert_eq!(
            backoff(BASE, 2, Some(Duration::from_secs(1)), CAP),
            Duration::from_secs(2)
        );
        // ...and a hostile Retry-After is capped by the policy's cap.
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
        // FMP and FRED ride their API keys as query params, and a reqwest
        // transport error's Display includes the request URL — the Err arm must
        // strip it before the error is printed or returned (the context chain
        // reaches the tracker-row detail and the persisted job detail).
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

    /// A millisecond-scale policy so class-split and abort tests run fast; the
    /// real sleeps stay covered by the legacy-schedule tests below.
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
        // Four 429s then a 200: with rate_limit.attempts=5 the ladder rides all
        // the way to success — five attempts, even though server_error would have
        // stopped at two.
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
        // Two 503s under server_error.attempts=2: the second attempt is the last —
        // the larger rate-limit budget does not apply to a 5xx.
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
        // The abort closure fires immediately: the first 429 is returned as-is —
        // no second attempt — so the caller's own cancellation boundary check can
        // route it.
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
        // The abort arms only AFTER the backoff sleep has begun (time-armed
        // closure), pinning the chunked polling itself: an implementation that
        // checked abort once upfront and then slept the whole wait would blow
        // the elapsed bound AND make the second request. The single backoff
        // wait is 8s; the abort arms at ~300ms, so the ~250ms poll slices must
        // notice it within roughly half a second.
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

    // ---- In-loop offline coverage for the `send_with_retry` round trip ----
    //
    // These exercise the retry/backoff/body-reread loop against a real localhost
    // socket — the one path a live API key was previously the only thing to run.
    // Each test supplies its own `build` closure pointing at a throwaway server, so
    // no adapter's hardcoded endpoint is involved. They are *not* `#[ignore]`d: they
    // run in the normal `cargo test` loop. They do incur the real default-policy
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
        // the default budget and hand back the final (status, body) — not an Err.
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
