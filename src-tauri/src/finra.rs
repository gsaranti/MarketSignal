//! FINRA consolidated Equity Short Interest — the run-level short-interest
//! file behind Portfolio Analysis's per-holding **risk / squeeze-context**
//! positioning read (`docs/data-sources.md §FINRA`). Keyless, biweekly
//! (mid- and end-of-month settlement, disseminated ~7 business days later),
//! fetched **once per run** and looked up per holding; wholly fail-soft.
//!
//! Retrieval is two keyless GETs: a **latest-settlement-date discovery**, then
//! the static CDN file `shrt{YYYYMMDD}.csv` (pipe-delimited despite the
//! extension). The settlement date is FINRA-designated — mid-month is the 15th
//! *or the preceding business day*, end-of-month the *last business day* — so
//! the date is discovered, never computed locally. Discovery prefers the
//! partitions endpoint (`api.finra.org` answered keyless when live-verified
//! 2026-08-21, though FINRA's docs describe an OAuth model) and falls back to
//! scanning the public files page for `shrt########.csv` links, so an
//! enforcement change on the API shifts the discovery route without touching
//! the file fetch. A missing CDN date returns HTTP **403**, not 404; any
//! non-2xx reads as not-published.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};

use crate::progress::RunContext;

const FINRA_PARTITIONS_URL: &str =
    "https://api.finra.org/partitions/group/otcMarket/name/consolidatedShortInterest";
const FINRA_FILES_PAGE_URL: &str =
    "https://www.finra.org/finra-data/browse-catalog/equity-short-interest/files";
const FINRA_CDN_BASE: &str = "https://cdn.finra.org/equity/otcmarket/biweekly";

/// Generous for a ~2 MB consolidated file.
const FINRA_TIMEOUT: StdDuration = StdDuration::from_secs(30);

/// One symbol's row from the consolidated file — level, trend, and
/// days-to-cover, exactly the read the docs name. `settlement_date` is the
/// row's own ISO date (the file repeats it per row).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShortInterestRead {
    pub settlement_date: String,
    pub current_short_interest: f64,
    pub previous_short_interest: Option<f64>,
    pub average_daily_volume: Option<f64>,
    pub days_to_cover: Option<f64>,
}

/// The parsed consolidated file: the discovered settlement date plus a
/// symbol-keyed map (uppercased) for the per-holding local lookup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShortInterestFile {
    /// The discovered settlement date, `YYYY-MM-DD`.
    pub settlement_date: String,
    pub by_symbol: HashMap<String, ShortInterestRead>,
}

impl ShortInterestFile {
    /// Look up an account symbol using FINRA's reporting key convention.
    ///
    /// FINRA instructs reporters to remove spaces and special characters from
    /// issue symbols, so broker spellings such as Schwab's `BRK/B` must resolve
    /// against the consolidated file's `BRKB` key. The account-owned symbol is
    /// not rewritten; normalization is confined to this adapter boundary.
    pub fn lookup(&self, account_symbol: &str) -> Option<&ShortInterestRead> {
        let key = finra_symbol_key(account_symbol);
        if key.is_empty() {
            None
        } else {
            self.by_symbol.get(&key)
        }
    }
}

/// FINRA short-interest reporting accepts uppercase issue symbols after all
/// spaces and special characters have been removed.
fn finra_symbol_key(symbol: &str) -> String {
    symbol
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Live FINRA short-interest adapter. Keyless; no execution-gate presence.
pub struct FinraDataSource {
    http: reqwest::blocking::Client,
    partitions_url: String,
    files_page_url: String,
    cdn_base: String,
    progress: Arc<RunContext>,
}

impl FinraDataSource {
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(FINRA_TIMEOUT)
            .build()
            .context("building the FINRA HTTP client")?;
        Ok(Self {
            http,
            partitions_url: FINRA_PARTITIONS_URL.to_string(),
            files_page_url: FINRA_FILES_PAGE_URL.to_string(),
            cdn_base: FINRA_CDN_BASE.to_string(),
            progress: RunContext::noop(),
        })
    }

    /// Redirect every call at a localhost mock so the wire path runs offline.
    /// Test-only.
    #[cfg(test)]
    fn with_urls(mut self, partitions: &str, files_page: &str, cdn_base: &str) -> Self {
        self.partitions_url = partitions.to_string();
        self.files_page_url = files_page.to_string();
        self.cdn_base = cdn_base.trim_end_matches('/').to_string();
        self
    }

    /// Attach a live run context so each fetch streams a tracker row.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Fetch and parse the latest consolidated short-interest file. `Err` is
    /// the caller's typed gap (discovery exhausted, transport, non-2xx, or a
    /// file whose header no longer carries the needed columns).
    pub fn short_interest(&self) -> Result<ShortInterestFile> {
        if self.progress.is_cancelled() {
            anyhow::bail!("FINRA fetch skipped (run cancelled)");
        }
        let date = self.discover_latest_date()?;
        let compact: String = date.chars().filter(char::is_ascii_digit).collect();
        let url = format!("{}/shrt{compact}.csv", self.cdn_base);
        let body = self.tracked_get(&url, "short-interest-file", "FINRA short-interest file")?;
        let by_symbol = parse_short_interest_file(&body, &date)?;
        Ok(ShortInterestFile {
            settlement_date: date,
            by_symbol,
        })
    }

    /// The latest available settlement date (`YYYY-MM-DD`): the partitions
    /// endpoint first, the files-page scan as the coded fallback.
    fn discover_latest_date(&self) -> Result<String> {
        let partitions = self
            .tracked_get(
                &self.partitions_url,
                "short-interest-partitions",
                "FINRA settlement-date discovery",
            )
            .and_then(|body| parse_partitions(&body));
        match partitions {
            Ok(date) => Ok(date),
            Err(partitions_err) => {
                let html = self
                    .tracked_get(
                        &self.files_page_url,
                        "short-interest-files-page",
                        "FINRA files-page fallback",
                    )
                    .with_context(|| {
                        format!("partitions discovery failed ({partitions_err}); files-page fallback")
                    })?;
                scrape_latest_file_date(&html).with_context(|| {
                    format!(
                        "partitions discovery failed ({partitions_err}) and the files page \
                         carried no shrt########.csv link"
                    )
                })
            }
        }
    }

    /// One tracked GET: a tracker request row per actual HTTP call, non-2xx
    /// surfaced as the error the caller's gap carries.
    fn tracked_get(&self, url: &str, endpoint: &str, label: &str) -> Result<String> {
        self.progress
            .request_started("FINRA", endpoint, "biweekly", label);
        let result = (|| -> Result<String> {
            let (status, body) =
                crate::http_retry::send_with_retry("FINRA", || self.http.get(url))?;
            if !(200..300).contains(&status) {
                // A missing CDN date answers 403, not 404 (live-verified
                // 2026-08-21): any non-2xx is "not published or unavailable".
                anyhow::bail!("FINRA request returned {status}");
            }
            Ok(body)
        })();
        match &result {
            Ok(_) => self
                .progress
                .request_finished("FINRA", endpoint, "biweekly", label, "ok", None),
            Err(e) => self.progress.request_finished(
                "FINRA",
                endpoint,
                "biweekly",
                label,
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }
}

/// The newest `availablePartitions` date from the partitions response —
/// entries arrive most-recent first, but the maximum is taken so an ordering
/// change cannot serve a stale file. Calendar-validated; a response carrying
/// no valid date is drift, `Err`.
fn parse_partitions(body: &str) -> Result<String> {
    let json: serde_json::Value =
        serde_json::from_str(body).context("parsing the FINRA partitions response")?;
    json.get("availablePartitions")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| p.get("partitions").and_then(serde_json::Value::as_str))
        .filter(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
        .max()
        .map(str::to_string)
        .context("FINRA partitions response carried no valid settlement date")
}

/// The newest settlement date among the files page's `shrt{YYYYMMDD}.csv`
/// links, returned as `YYYY-MM-DD`; `None` when no calendar-valid link exists.
fn scrape_latest_file_date(html: &str) -> Option<String> {
    let mut best: Option<String> = None;
    let mut from = 0;
    while let Some(found) = html[from..].find("shrt") {
        let at = from + found + "shrt".len();
        from = at;
        let Some(candidate) = html.get(at..at + 8) else {
            continue;
        };
        if !candidate.bytes().all(|b| b.is_ascii_digit())
            || !html[at + 8..].starts_with(".csv")
            || chrono::NaiveDate::parse_from_str(candidate, "%Y%m%d").is_err()
        {
            continue;
        }
        let iso = format!(
            "{}-{}-{}",
            &candidate[..4],
            &candidate[4..6],
            &candidate[6..8]
        );
        if best.as_deref().is_none_or(|b| iso.as_str() > b) {
            best = Some(iso);
        }
    }
    best
}

/// Parse the pipe-delimited consolidated file into a symbol-keyed map. Column
/// positions are resolved from the header row by name — a header missing the
/// symbol, current-short-interest, or settlement-date column is structure
/// drift, `Err`. Values are **semantically validated**, not just parsed:
/// quantities must be finite and non-negative (Rust's `f64` parser accepts
/// "NaN"/"inf" strings, which must never reach persistence or a prompt), and
/// each row's settlement date must be the discovered `expected_settlement` —
/// a mismatched or malformed date is drift, the row skipped. Optional numeric
/// fields (`previous…`, ADV, days-to-cover) tolerate the file's routinely
/// empty cells, an invalid value reading absent; a row whose required fields
/// do not validate is skipped, and a file yielding **zero** rows is drift
/// rather than an empty success.
fn parse_short_interest_file(
    text: &str,
    expected_settlement: &str,
) -> Result<HashMap<String, ShortInterestRead>> {
    let mut lines = text.lines();
    let header = lines.next().context("FINRA file was empty")?;
    let cols: Vec<&str> = header.split('|').map(str::trim).collect();
    let col = |name: &str| cols.iter().position(|c| *c == name);
    let (Some(sym), Some(current), Some(settle)) = (
        col("symbolCode"),
        col("currentShortPositionQuantity"),
        col("settlementDate"),
    ) else {
        anyhow::bail!("FINRA file header missing a required column — structure drift: {header}");
    };
    let previous = col("previousShortPositionQuantity");
    let adv = col("averageDailyVolumeQuantity");
    let dtc = col("daysToCoverQuantity");

    let mut by_symbol = HashMap::new();
    for line in lines {
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        let get = |i: usize| fields.get(i).copied().unwrap_or("");
        // A quantity is only a quantity when finite and non-negative.
        let num = |i: Option<usize>| {
            i.and_then(|i| get(i).parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
        };
        let symbol = get(sym);
        let Some(current_si) = num(Some(current)) else {
            continue;
        };
        let settlement = get(settle);
        if symbol.is_empty() || settlement != expected_settlement {
            continue;
        }
        by_symbol.insert(
            symbol.to_ascii_uppercase(),
            ShortInterestRead {
                settlement_date: settlement.to_string(),
                current_short_interest: current_si,
                previous_short_interest: num(previous),
                average_daily_volume: num(adv),
                days_to_cover: num(dtc),
            },
        );
    }
    if by_symbol.is_empty() {
        anyhow::bail!("FINRA file parsed to zero rows — structure drift");
    }
    Ok(by_symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    /// Header + rows verbatim in the live file's shape (captured 2026-08-21):
    /// pipe-delimited, empty `stockSplitFlag` / `revisionFlag` cells routine.
    const FILE_BODY: &str = "accountingYearMonthNumber|symbolCode|issueName|issuerServicesGroupExchangeCode|marketClassCode|currentShortPositionQuantity|previousShortPositionQuantity|stockSplitFlag|averageDailyVolumeQuantity|daysToCoverQuantity|revisionFlag|changePercent|changePreviousNumber|settlementDate\n\
        20260731|A|Agilent Technologies Inc.|A|NYSE|5749623|7538437||2301495|2.50||-23.73|-1788814|2026-07-31\n\
        20260731|aapl|Apple Inc.|AAPL|NNM|108000000|101000000||55000000|1.96||6.93|7000000|2026-07-31\n\
        20260731|NOADV|Thin Name|N|OTC|1200||||||||2026-07-31\n";

    const PARTITIONS_BODY: &str = r#"{"datasetGroup":"otcMarket","datasetName":"consolidatedShortInterest","partitionFields":["settlementDate"],"availablePartitions":[{"partitions":"2026-07-31"},{"partitions":"2026-07-15"},{"partitions":"2026-06-30"}]}"#;

    #[test]
    fn parses_the_file_by_header_name_with_empty_cells_tolerated() {
        let map = parse_short_interest_file(FILE_BODY, "2026-07-31").unwrap();
        assert_eq!(map.len(), 3);
        let a = &map["A"];
        assert_eq!(a.settlement_date, "2026-07-31");
        assert_eq!(a.current_short_interest, 5_749_623.0);
        assert_eq!(a.previous_short_interest, Some(7_538_437.0));
        assert_eq!(a.average_daily_volume, Some(2_301_495.0));
        assert_eq!(a.days_to_cover, Some(2.50));
        // Symbols key uppercased, so the per-holding lookup is case-stable.
        assert!(map.contains_key("AAPL"));
        // Empty optional cells read as absent, never zero.
        let thin = &map["NOADV"];
        assert_eq!(thin.current_short_interest, 1_200.0);
        assert_eq!(thin.previous_short_interest, None);
        assert_eq!(thin.average_daily_volume, None);
        assert_eq!(thin.days_to_cover, None);
    }

    #[test]
    fn lookup_uses_finras_separator_free_symbol_key_without_rewriting_identity() {
        let read = ShortInterestRead {
            settlement_date: "2026-07-31".into(),
            current_short_interest: 10_000.0,
            previous_short_interest: Some(9_000.0),
            average_daily_volume: Some(2_000.0),
            days_to_cover: Some(5.0),
        };
        let file = ShortInterestFile {
            settlement_date: "2026-07-31".into(),
            by_symbol: HashMap::from([("BRKB".into(), read.clone())]),
        };

        assert_eq!(file.lookup("BRK/B"), Some(&read));
        assert_eq!(file.lookup("brk.b"), Some(&read));
        assert_eq!(file.lookup("BRK B"), Some(&read));
        assert_eq!(file.lookup("BRK/A"), None);
        assert_eq!(file.lookup("///"), None);
    }

    #[test]
    fn header_columns_resolve_by_name_not_position() {
        // Reordered columns still parse — positions come from the header.
        let reordered = "symbolCode|settlementDate|currentShortPositionQuantity|daysToCoverQuantity\n\
            XYZ|2026-07-31|5000|1.10\n";
        let map = parse_short_interest_file(reordered, "2026-07-31").unwrap();
        assert_eq!(map["XYZ"].current_short_interest, 5_000.0);
        assert_eq!(map["XYZ"].days_to_cover, Some(1.10));
        assert_eq!(map["XYZ"].previous_short_interest, None);
    }

    #[test]
    fn header_drift_and_zero_rows_error_rather_than_reading_empty() {
        // A header missing a required column is drift.
        assert!(parse_short_interest_file("symbolCode|somethingElse\nA|1\n", "2026-07-31").is_err());
        // A well-formed header whose rows all fail to parse is drift too.
        let no_rows = "symbolCode|currentShortPositionQuantity|settlementDate\n\
            A|not-a-number|2026-07-31\n||\n";
        assert!(parse_short_interest_file(no_rows, "2026-07-31").is_err());
        // Empty input is drift.
        assert!(parse_short_interest_file("", "2026-07-31").is_err());
    }

    #[test]
    fn semantic_validation_rejects_nan_negatives_and_foreign_dates() {
        // Rust's f64 parser accepts "NaN"/"inf" strings — semantic validation
        // is what keeps them (and negative quantities, or a row dated off the
        // discovered settlement) out of persistence and the prompt.
        let body = "symbolCode|currentShortPositionQuantity|previousShortPositionQuantity|settlementDate\n\
            NANNY|NaN|100|2026-07-31\n\
            NEG|-5|100|2026-07-31\n\
            STALE|1000|100|2026-07-15\n\
            BAD|1000|100|not-a-date\n\
            OK|1000|inf|2026-07-31\n";
        let map = parse_short_interest_file(body, "2026-07-31").unwrap();
        assert_eq!(map.len(), 1, "{map:?}");
        let ok = &map["OK"];
        assert_eq!(ok.current_short_interest, 1000.0);
        assert_eq!(ok.previous_short_interest, None, "an infinite value is no quantity");
    }

    #[test]
    fn partitions_takes_the_maximum_valid_date() {
        assert_eq!(parse_partitions(PARTITIONS_BODY).unwrap(), "2026-07-31");
        // Ordering must not matter, and invalid dates never win.
        let unordered = r#"{"availablePartitions":[{"partitions":"2026-06-30"},{"partitions":"2026-99-99"},{"partitions":"2026-07-31"}]}"#;
        assert_eq!(parse_partitions(unordered).unwrap(), "2026-07-31");
        assert!(parse_partitions(r#"{"availablePartitions":[]}"#).is_err());
        assert!(parse_partitions("{not json").is_err());
        assert!(parse_partitions(r#"{"other":true}"#).is_err());
    }

    #[test]
    fn files_page_scan_takes_the_newest_link() {
        let html = r#"<a href="https://cdn.finra.org/equity/otcmarket/biweekly/shrt20260715.csv">mid</a>
            <a href="https://cdn.finra.org/equity/otcmarket/biweekly/shrt20260731.csv">eom</a>
            <a href="shrt2026.csv">short</a> <a href="shrt20269999.csv">invalid</a>"#;
        assert_eq!(scrape_latest_file_date(html).unwrap(), "2026-07-31");
        assert!(scrape_latest_file_date("<html>no links</html>").is_none());
    }

    #[test]
    fn wire_round_trip_partitions_then_file() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![("Content-Type", "application/json")],
                body: PARTITIONS_BODY,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: FILE_BODY,
            },
        ]);
        let base = server.base_url.trim_end_matches('/').to_string();
        let file = FinraDataSource::new()
            .unwrap()
            .with_urls(&base, &base, &base)
            .short_interest()
            .unwrap();
        assert_eq!(file.settlement_date, "2026-07-31");
        assert_eq!(file.by_symbol["AAPL"].days_to_cover, Some(1.96));
    }

    #[test]
    fn discovery_falls_back_to_the_files_page_and_a_missing_file_errors() {
        // Partitions 404 (a terminal status — a 5xx would be retried by the
        // shared backoff and consume the canned replies) → files-page scrape
        // serves the date → file fetch.
        let page = r#"<a href="/shrt20260731.csv">latest</a>"#;
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 404, headers: vec![], body: "gone" },
            Canned::Reply { status: 200, headers: vec![], body: page },
            Canned::Reply { status: 200, headers: vec![], body: FILE_BODY },
        ]);
        let base = server.base_url.trim_end_matches('/').to_string();
        let file = FinraDataSource::new()
            .unwrap()
            .with_urls(&base, &base, &base)
            .short_interest()
            .unwrap();
        assert_eq!(file.settlement_date, "2026-07-31");

        // A 403 on the CDN file (the not-published answer) is the caller's gap.
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: PARTITIONS_BODY },
            Canned::Reply { status: 403, headers: vec![], body: "denied" },
        ]);
        let base = server.base_url.trim_end_matches('/').to_string();
        assert!(FinraDataSource::new()
            .unwrap()
            .with_urls(&base, &base, &base)
            .short_interest()
            .is_err());
    }

    /// Live smoke against the real endpoints — run manually:
    /// `cargo test finra_short_interest_smoke -- --ignored --nocapture`.
    /// This is where a discovery-route enforcement change or a file-format
    /// shift (the proposed weekly cadence, SR-FINRA-2026-012) surfaces.
    #[test]
    #[ignore = "hits the live FINRA endpoints"]
    fn finra_short_interest_smoke() {
        let file = FinraDataSource::new().unwrap().short_interest().unwrap();
        eprintln!(
            "FINRA short interest: {} rows, settlement {}",
            file.by_symbol.len(),
            file.settlement_date
        );
        assert!(file.by_symbol.len() > 10_000, "consolidated file is ~22k rows");
    }
}
