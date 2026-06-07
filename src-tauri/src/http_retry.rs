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
}
