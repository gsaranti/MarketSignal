//! Stooq — keyless daily OHLCV history (`docs/data-sources.md §Stooq`): the local
//! suite's deep per-holding price source, offloading FMP. Split-adjusted,
//! dividend-unadjusted daily bars as CSV; this slice reads the **dated closes** the
//! v2 anchor join, the drawdown read, and the fund risk legs consume.
//!
//! Like the other adapters it carries a base-URL seam so a localhost mock exercises
//! the full URL-build → fetch → parse path offline, and it is fail-soft at the
//! caller: a failed or empty history degrades to a tagged gap (the anchor window
//! then falls to its documented fallback), never a run failure.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::NaiveDate;

use crate::portfolio::engine::DatedValue;
use crate::progress::RunContext;

/// Stooq's CSV download host.
const STOOQ_BASE: &str = "https://stooq.com";

/// The daily-bars CSV path; symbols are query params.
const STOOQ_DAILY_PATH: &str = "/q/d/l/";

const STOOQ_TIMEOUT: Duration = Duration::from_secs(20);

/// The keyless Stooq daily-bar adapter.
pub struct StooqSource {
    http: reqwest::blocking::Client,
    base_url: String,
    progress: Arc<RunContext>,
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
        })
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
/// whole history; a body with no header at all is malformed.
fn parse_daily_csv(body: &str) -> Result<Vec<DatedValue>> {
    let mut lines = body.lines();
    let header = lines.next().context("empty Stooq body")?;
    if !header.to_ascii_lowercase().starts_with("date,") {
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
    }
}
