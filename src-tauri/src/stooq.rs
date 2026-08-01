//! Stooq — keyless daily OHLCV history (`docs/data-sources.md §Stooq`): the local
//! suite's deep per-holding price source, offloading FMP. Split-adjusted,
//! dividend-unadjusted daily bars as CSV; this slice reads the **dated closes** the
//! v2 anchor join, the drawdown read, and the fund risk legs consume.
//!
//! Like the other adapters it carries a base-URL seam so a localhost mock exercises
//! the full URL-build → fetch → parse path offline, and it is fail-soft at the
//! caller: a failed or empty history degrades to a tagged gap (the anchor window
//! then falls to its documented fallback), never a run failure.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::NaiveDate;

use crate::portfolio::engine::DatedValue;
use crate::progress::RunContext;

/// Stooq's CSV download host.
const STOOQ_BASE: &str = "https://stooq.com";

/// The daily-bars CSV path; symbols are query params.
const STOOQ_DAILY_PATH: &str = "/q/d/l/";

const STOOQ_TIMEOUT: Duration = Duration::from_secs(20);

/// Minimum spacing between Stooq requests — the doc-promised politeness
/// self-throttle (`docs/data-sources.md §Stooq`). Negligible against a run's model
/// time; only back-to-back fetches (e.g. a run of fast failures) ever wait.
const STOOQ_MIN_INTERVAL: Duration = Duration::from_secs(1);

/// Stooq's daily-hits throttle, classified distinctly: the notice arrives as an
/// HTTP **200 HTML body** in place of the daily-bars CSV — invisible to the
/// status-based retry layer by construction — and it is account/IP-wide, so the
/// first detection trips a run-wide breaker rather than burning one doomed request
/// per remaining holding (the 2026-07-31 F2 finding: 43 of 44 fetches failed this
/// way).
#[derive(Debug)]
pub struct StooqThrottled;

impl std::fmt::Display for StooqThrottled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Stooq daily-hits limit reached (HTML notice body in place of the daily-bars CSV)"
        )
    }
}

impl std::error::Error for StooqThrottled {}

/// The keyless Stooq daily-bar adapter.
pub struct StooqSource {
    http: reqwest::blocking::Client,
    base_url: String,
    progress: Arc<RunContext>,
    /// Set once the daily-hits throttle is detected; every later fetch this run
    /// skips the network and fails fast as throttled (no tracker row — a skipped
    /// fetch is not an HTTP call). The adapter is constructed per run, so the
    /// breaker resets naturally.
    throttled: AtomicBool,
    /// The last request's send time, for the politeness spacing.
    last_request: Mutex<Option<Instant>>,
}

impl StooqSource {
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(STOOQ_TIMEOUT)
            .build()
            .context("building the Stooq HTTP client")?;
        Ok(Self {
            http,
            base_url: STOOQ_BASE.to_string(),
            progress: RunContext::noop(),
            throttled: AtomicBool::new(false),
            last_request: Mutex::new(None),
        })
    }

    /// Whether the run-wide daily-hits breaker has tripped.
    pub fn is_throttled(&self) -> bool {
        self.throttled.load(Ordering::Relaxed)
    }

    /// Attach a live run context so each fetch streams a tracker row.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Point the adapter at a mock base URL for the offline round-trip test.
    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// The politeness spacing: sleep out the remainder of [`STOOQ_MIN_INTERVAL`]
    /// since the last request, then stamp now. Skipped entirely in tests (the mock
    /// round-trips shouldn't wait).
    fn pace(&self) {
        let mut last = self.last_request.lock().expect("stooq pacing lock");
        if !cfg!(test) {
            if let Some(prev) = *last {
                let elapsed = prev.elapsed();
                if elapsed < STOOQ_MIN_INTERVAL {
                    std::thread::sleep(STOOQ_MIN_INTERVAL - elapsed);
                }
            }
        }
        *last = Some(Instant::now());
    }

    /// Daily closes for a symbol over `[from, to]`, oldest first. A US listing maps
    /// to Stooq's `<symbol>.us` identity (`docs/data-sources.md §Stooq`); a symbol
    /// already carrying a venue suffix (or an index like `^spx`) passes through.
    pub fn daily_closes(
        &self,
        symbol: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<DatedValue>> {
        if self.progress.is_cancelled() {
            anyhow::bail!("Stooq fetch skipped (run cancelled)");
        }
        // The run-wide breaker: after one throttle detection, every later fetch fails
        // fast as throttled — the daily-hits limit is account/IP-wide, so retrying
        // per holding only burns requests (and goodwill). No tracker row: a skipped
        // fetch is not an HTTP call.
        if self.is_throttled() {
            return Err(anyhow::Error::new(StooqThrottled)
                .context(format!("Stooq skipped for {symbol} (throttled earlier this run)")));
        }
        self.pace();
        let stooq_symbol = stooq_symbol(symbol);
        let url = format!("{}{STOOQ_DAILY_PATH}", self.base_url);
        self.progress
            .request_started("Stooq", "daily-bars", symbol, "Daily price history");
        let result = (|| -> Result<Vec<DatedValue>> {
            let (status, body) = crate::http_retry::send_with_retry("Stooq", || {
                self.http.get(&url).query(&[
                    ("s", stooq_symbol.as_str()),
                    ("d1", &from.format("%Y%m%d").to_string()),
                    ("d2", &to.format("%Y%m%d").to_string()),
                    ("i", "d"),
                ])
            })?;
            if !(200..300).contains(&status) {
                anyhow::bail!("Stooq returned {status} for {symbol}");
            }
            let closes = parse_daily_csv(&body)?;
            if closes.is_empty() {
                anyhow::bail!("Stooq returned no daily bars for {symbol}");
            }
            Ok(closes)
        })();
        if let Err(e) = &result {
            if e.downcast_ref::<StooqThrottled>().is_some() {
                self.throttled.store(true, Ordering::Relaxed);
            }
        }
        match &result {
            Ok(_) => self.progress.request_finished(
                "Stooq",
                "daily-bars",
                symbol,
                "Daily price history",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "Stooq",
                "daily-bars",
                symbol,
                "Daily price history",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }
}

/// Stooq's symbol identity for a US listing: lowercase plus the `.us` venue suffix;
/// a symbol already carrying a dot (a venue suffix) or a caret (an index) passes
/// through lowercased.
fn stooq_symbol(symbol: &str) -> String {
    let lower = symbol.to_ascii_lowercase();
    if lower.contains('.') || lower.starts_with('^') {
        lower
    } else {
        format!("{lower}.us")
    }
}

/// Parse Stooq's daily CSV (`Date,Open,High,Low,Close,Volume`, header first) into
/// dated closes, oldest first. A malformed row is skipped rather than failing the
/// whole history; a body with no header at all is malformed — and an **HTML body in
/// the CSV's place is the daily-hits throttle notice** (it rides HTTP 200, so only
/// this seam can see it), classified as [`StooqThrottled`] for the run-wide breaker.
fn parse_daily_csv(body: &str) -> Result<Vec<DatedValue>> {
    let mut lines = body.lines();
    let header = lines.next().context("empty Stooq body")?;
    if !header.to_ascii_lowercase().starts_with("date,") {
        let head = body.trim_start().get(..256).unwrap_or(body.trim_start());
        if head.starts_with('<') || head.to_ascii_lowercase().contains("<html") {
            return Err(anyhow::Error::new(StooqThrottled));
        }
        anyhow::bail!("Stooq body did not start with the daily-bars CSV header");
    }
    let mut out = Vec::new();
    for line in lines {
        let mut cols = line.split(',');
        let (Some(date), Some(close)) = (cols.next(), cols.nth(3)) else {
            continue;
        };
        if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            continue;
        }
        if let Ok(value) = close.trim().parse::<f64>() {
            out.push(DatedValue {
                date: date.to_string(),
                value,
            });
        }
    }
    out.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    const CSV: &str = "Date,Open,High,Low,Close,Volume\n\
        2026-07-13,192.0,196.0,191.5,195.0,1000000\n\
        2026-07-10,190.0,193.0,189.0,192.5,900000\n\
        bad,row\n\
        2026-07-14,195.0,197.0,194.0,196.2,1100000\n";

    #[test]
    fn parses_and_sorts_daily_closes_skipping_malformed_rows() {
        let closes = parse_daily_csv(CSV).unwrap();
        assert_eq!(closes.len(), 3);
        assert_eq!(closes[0].date, "2026-07-10");
        assert_eq!(closes[2].date, "2026-07-14");
        assert!((closes[2].value - 196.2).abs() < 1e-9);
    }

    #[test]
    fn us_symbols_map_to_the_dot_us_identity() {
        assert_eq!(stooq_symbol("AAPL"), "aapl.us");
        assert_eq!(stooq_symbol("^SPX"), "^spx");
        assert_eq!(stooq_symbol("HG.F"), "hg.f");
    }

    #[test]
    fn fetch_round_trips_the_csv_and_builds_the_query() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: CSV,
        }]);
        let stooq = StooqSource::new().unwrap().with_base_url(&server.base_url);
        let closes = stooq
            .daily_closes(
                "AAPL",
                NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 7, 15).unwrap(),
            )
            .unwrap();
        assert_eq!(closes.len(), 3);
        let target = &server.request_targets()[0];
        assert!(target.starts_with("/q/d/l/"), "{target}");
        assert!(target.contains("s=aapl.us"), "{target}");
        assert!(target.contains("i=d"), "{target}");
    }

    #[test]
    fn a_non_2xx_or_empty_history_is_an_error_for_the_caller_to_fail_soft() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 404,
            headers: vec![],
            body: "not found",
        }]);
        let stooq = StooqSource::new().unwrap().with_base_url(&server.base_url);
        assert!(stooq
            .daily_closes(
                "ZZZZ",
                NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 7, 15).unwrap(),
            )
            .is_err());
        // A plain non-CSV, non-HTML body stays an ordinary parse error, not a
        // throttle classification.
        assert!(parse_daily_csv("nope,nothing")
            .unwrap_err()
            .downcast_ref::<StooqThrottled>()
            .is_none());
    }

    #[test]
    fn an_html_notice_body_classifies_as_the_throttle_and_trips_the_breaker() {
        // The daily-hits notice rides HTTP 200 with an HTML body in the CSV's place
        // (the 2026-07-31 live-run signature), so only the parse seam can see it.
        // One detection must trip the run-wide breaker: the next call fails fast as
        // throttled with no HTTP request spent.
        let html = "<html><body>Przekroczony dzienny limit wywolan</body></html>";
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: html,
        }]);
        let stooq = StooqSource::new().unwrap().with_base_url(&server.base_url);
        let from = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 7, 15).unwrap();
        let err = stooq.daily_closes("AAPL", from, to).unwrap_err();
        assert!(err.downcast_ref::<StooqThrottled>().is_some(), "{err}");
        assert!(stooq.is_throttled());
        // The breaker short-circuits: one canned reply was consumed, and the second
        // call still errors as throttled without reaching the server.
        let err = stooq.daily_closes("MSFT", from, to).unwrap_err();
        assert!(err.downcast_ref::<StooqThrottled>().is_some(), "{err}");
        assert_eq!(server.request_targets().len(), 1, "no second HTTP request");
    }
}
