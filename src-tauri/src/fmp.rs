//! Real Financial Modeling Prep adapter for the baseline market-data scan.
//!
//! The first data-source adapter behind the `MarketDataSource` trait
//! (`data_sources`). On FMP's free tier the provider is effectively an *equities*
//! API, so this adapter owns the equity-market half of the Step-6 baseline:
//! the market **indices** (Dow / S&P 500 / Nasdaq / Russell 2000), the **VIX**,
//! and **sector performance**. The macro / commodity internals the baseline also
//! lists — Treasury yields, the dollar index, oil, natural gas, gold — are gated
//! behind FMP premium (verified live: HTTP 402 "not available under your current
//! subscription") and are sourced from FRED in the macro slice instead. Each is a
//! canonical free FRED series; see `docs/data-sources.md` (amended to reflect this
//! split).
//!
//! Like `model_agent`, the HTTP call is synchronous (`reqwest::blocking`) so the
//! trait stays sync; the blocking work is offloaded via `spawn_blocking` at the
//! Tauri command seam. The key-as-query-param convention and the FMP error
//! detection (a rejected key is a 401, but FMP can also report an error in a 200
//! `{"Error Message": ...}` body) are the same ones `connection_test` verified
//! live (Jun 2026).
//!
//! Degradation policy: an auth failure (401/403) or a transport error is fatal —
//! the whole scan fails, which `jobs::run_job` records as a failed job
//! (`docs/scheduling.md §Offline Behavior`). A *per-symbol* failure (a premium
//! 402, a 404, an error body, an unexpected shape) skips only that symbol so the
//! rest of the scan still lands; the absent data simply does not reach the agent.

use std::collections::HashSet;
use std::time::Duration as StdDuration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{BaselineMarketData, MarketDataSource, Quote, SectorPerformance};

/// FMP's stable single-symbol quote endpoint — the one `connection_test`
/// exercises. The key rides as a query param, never an Authorization header.
const FMP_QUOTE_URL: &str = "https://financialmodelingprep.com/stable/quote";
/// FMP's sector-performance snapshot endpoint. Requires a `date` query param
/// (a dateless call returns HTTP 400).
const FMP_SECTOR_URL: &str = "https://financialmodelingprep.com/stable/sector-performance-snapshot";

/// Short timeout per request: the baseline scan issues several sequential calls,
/// none of which should park for the model adapter's 120s ceiling.
const FMP_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// How many days back to look for the most recent sector snapshot. The weekly job
/// fires Sunday 9am, when the latest snapshot is the prior Friday's, so a run must
/// walk back over the closed-market weekend (and any holiday) to the last trading
/// day that has data.
const SECTOR_LOOKBACK_DAYS: i64 = 5;

/// The four headline indices of the baseline scan (`docs/weekly-report-workflow
/// .md §Step 6`), paired with a display name used when FMP omits one. All four
/// are free-tier on FMP (verified live).
const INDEX_SYMBOLS: &[(&str, &str)] = &[
    ("^DJI", "Dow Jones Industrial Average"),
    ("^GSPC", "S&P 500"),
    ("^IXIC", "Nasdaq Composite"),
    ("^RUT", "Russell 2000"),
];

/// The free-tier market internals FMP serves. Only the VIX qualifies — the dollar
/// index, oil, natural gas, and gold are FMP-premium and come from FRED instead
/// (see the module header).
const INTERNAL_SYMBOLS: &[(&str, &str)] = &[("^VIX", "CBOE Volatility Index")];

/// FMP's quote object, trimmed to the fields the baseline needs. `name` is
/// optional (filled from the local label when absent); the percent-change field
/// is `changePercentage` on the stable API, with the legacy `changesPercentage`
/// accepted as an alias.
#[derive(Debug, Deserialize)]
struct FmpQuoteRaw {
    symbol: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    price: f64,
    #[serde(default, rename = "changePercentage", alias = "changesPercentage")]
    change_pct: f64,
}

/// Whether a status is an authentication failure — fatal for the whole scan (the
/// key itself is bad), as opposed to a per-symbol data issue that is skippable.
fn is_auth_failure(status: u16) -> bool {
    status == 401 || status == 403
}

/// FMP's error detection for a body that should be a JSON array on success: any
/// non-2xx is an error, and a 200 body that is an `{"Error Message": ...}` object
/// is an error too (FMP's premium / rate-limit conditions return this, sometimes
/// alongside a 402). On success returns the parsed JSON value for the caller to
/// shape. Auth failures (401/403) surface as errors here as well, but callers
/// check `is_auth_failure` *first* because auth is fatal, not skippable.
fn fmp_json(status: u16, body: &str) -> Result<Value> {
    if !(200..300).contains(&status) {
        bail!("Financial Modeling Prep returned HTTP {status}");
    }
    let value: Value = serde_json::from_str(body).context("parsing FMP response JSON")?;
    if let Value::Object(map) = &value {
        if let Some(msg) = map.get("Error Message") {
            bail!("Financial Modeling Prep returned an error: {msg}");
        }
    }
    Ok(value)
}

/// Map an FMP quote response (a single-symbol `/stable/quote` call returns a
/// one-element array) into typed quotes, falling back to `fallback_name` when FMP
/// omits the instrument name.
fn parse_quotes(status: u16, body: &str, fallback_name: &str) -> Result<Vec<Quote>> {
    let value = fmp_json(status, body)?;
    let raws: Vec<FmpQuoteRaw> = serde_json::from_value(value)
        .context("FMP quote response did not match the expected array shape")?;
    Ok(raws
        .into_iter()
        .map(|r| Quote {
            name: if r.name.trim().is_empty() {
                fallback_name.to_string()
            } else {
                r.name
            },
            symbol: r.symbol,
            price: r.price,
            change_pct: r.change_pct,
        })
        .collect())
}

/// Map an FMP sector-performance snapshot into typed rows. The snapshot is an
/// array of `{ date, sector, exchange, averageChange }`; `averageChange` is the
/// sector's percent move. Rows are deduplicated by sector name (the default call
/// returns one row per sector, but a per-exchange variant could repeat them).
fn parse_sectors(status: u16, body: &str) -> Result<Vec<SectorPerformance>> {
    let value = fmp_json(status, body)?;
    let arr = value
        .as_array()
        .ok_or_else(|| anyhow!("FMP sector response was not a JSON array"))?;
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let sector = item
            .get("sector")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if sector.is_empty() || !seen.insert(sector.clone()) {
            continue;
        }
        let change_pct = item
            .get("averageChange")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        out.push(SectorPerformance { sector, change_pct });
    }
    Ok(out)
}

/// Live FMP adapter behind the `MarketDataSource` trait.
pub struct FmpDataSource {
    api_key: String,
    http: reqwest::blocking::Client,
}

impl FmpDataSource {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(FMP_TIMEOUT)
            .build()
            .context("building the FMP HTTP client")?;
        Ok(Self { api_key, http })
    }

    /// Resolve the adapter from the environment, for the live smoke and any
    /// caller that bypasses the gate. The execution gate (`config::validate`)
    /// runs ahead of this in the command path.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().fmp_key()?)
    }

    /// GET one FMP endpoint with the key as a query param, returning the status
    /// and raw body for a pure parser to interpret. A transport error (the
    /// provider is unreachable) propagates as a fatal scan error.
    fn get(&self, url: &str, extra: &[(&str, &str)]) -> Result<(u16, String)> {
        let mut query: Vec<(&str, &str)> = vec![("apikey", self.api_key.as_str())];
        query.extend_from_slice(extra);
        let resp = self
            .http
            .get(url)
            .query(&query)
            .send()
            .context("sending FMP request")?;
        let status = resp.status().as_u16();
        let body = resp.text().context("reading FMP response body")?;
        Ok((status, body))
    }

    /// Fetch one quote per symbol. An auth failure is fatal (the key is bad); any
    /// other per-symbol failure (premium 402, 404, error body, unexpected shape)
    /// skips just that symbol so the rest of the scan still lands.
    fn fetch_quotes(&self, symbols: &[(&str, &str)]) -> Result<Vec<Quote>> {
        let mut out = Vec::with_capacity(symbols.len());
        for (symbol, fallback_name) in symbols {
            let (status, body) = self.get(FMP_QUOTE_URL, &[("symbol", symbol)])?;
            if is_auth_failure(status) {
                bail!("Financial Modeling Prep rejected the key (HTTP {status})");
            }
            match parse_quotes(status, &body, fallback_name) {
                Ok(quotes) => out.extend(quotes),
                Err(_) => continue,
            }
        }
        Ok(out)
    }

    /// Fetch the most recent sector-performance snapshot, walking back from today
    /// to the last trading day that has data (weekends / holidays have none). An
    /// auth failure is fatal; if no day in the window has a snapshot, the scan
    /// soft-degrades to no sector data rather than failing.
    fn fetch_sectors(&self) -> Result<Vec<SectorPerformance>> {
        let today = Utc::now().date_naive();
        for back in 0..=SECTOR_LOOKBACK_DAYS {
            let date = (today - Duration::days(back)).format("%Y-%m-%d").to_string();
            let (status, body) = self.get(FMP_SECTOR_URL, &[("date", date.as_str())])?;
            if is_auth_failure(status) {
                bail!("Financial Modeling Prep rejected the key (HTTP {status})");
            }
            if let Ok(sectors) = parse_sectors(status, &body) {
                if !sectors.is_empty() {
                    return Ok(sectors);
                }
            }
        }
        Ok(Vec::new())
    }
}

impl MarketDataSource for FmpDataSource {
    fn baseline_scan(&self) -> Result<BaselineMarketData> {
        Ok(BaselineMarketData {
            indices: self.fetch_quotes(INDEX_SYMBOLS)?,
            internals: self.fetch_quotes(INTERNAL_SYMBOLS)?,
            sectors: self.fetch_sectors()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_quote_array_into_typed_quotes() {
        let body = r#"[{"symbol":"^GSPC","name":"S&P 500","price":5500.5,"changePercentage":0.42}]"#;
        let quotes = parse_quotes(200, body, "fallback").unwrap();
        assert_eq!(quotes.len(), 1);
        let q = &quotes[0];
        assert_eq!(q.symbol, "^GSPC");
        assert_eq!(q.name, "S&P 500");
        assert!((q.price - 5500.5).abs() < 1e-9);
        assert!((q.change_pct - 0.42).abs() < 1e-9);
    }

    #[test]
    fn quote_falls_back_to_local_name_when_fmp_omits_it() {
        let quotes =
            parse_quotes(200, r#"[{"symbol":"^DJI","price":40000.0}]"#, "Dow Jones").unwrap();
        assert_eq!(quotes[0].name, "Dow Jones");
    }

    #[test]
    fn quote_accepts_the_legacy_changes_percentage_alias() {
        let quotes = parse_quotes(
            200,
            r#"[{"symbol":"^VIX","price":14.0,"changesPercentage":-1.5}]"#,
            "VIX",
        )
        .unwrap();
        assert!((quotes[0].change_pct + 1.5).abs() < 1e-9);
    }

    #[test]
    fn empty_quote_array_is_no_quotes_not_an_error() {
        assert!(parse_quotes(200, "[]", "x").unwrap().is_empty());
    }

    #[test]
    fn auth_failure_is_classified_fatal() {
        assert!(is_auth_failure(401));
        assert!(is_auth_failure(403));
        assert!(!is_auth_failure(402)); // premium is per-symbol skippable, not fatal
        assert!(!is_auth_failure(200));
    }

    #[test]
    fn quote_200_with_error_message_is_an_error() {
        // The case a status-only check misses: HTTP 200, error in the body.
        let body = r#"{"Error Message":"Invalid API KEY. Please retry or visit our documentation"}"#;
        let err = parse_quotes(200, body, "x").unwrap_err();
        assert!(err.to_string().contains("error"), "{err}");
    }

    #[test]
    fn quote_premium_402_is_an_error_so_fetch_can_skip_it() {
        // A premium-gated symbol returns 402; parse_quotes surfaces it as an error
        // and fetch_quotes skips that one symbol rather than aborting the scan.
        let err = parse_quotes(402, "Premium Query Parameter", "x").unwrap_err();
        assert!(err.to_string().contains("402"), "{err}");
    }

    #[test]
    fn parses_sector_snapshot_into_typed_rows() {
        let body = r#"[
            {"date":"2026-06-04","sector":"Technology","exchange":"NASDAQ","averageChange":1.2619},
            {"date":"2026-06-04","sector":"Energy","exchange":"NASDAQ","averageChange":-0.1942}
        ]"#;
        let sectors = parse_sectors(200, body).unwrap();
        assert_eq!(sectors.len(), 2);
        assert_eq!(sectors[0].sector, "Technology");
        assert!((sectors[0].change_pct - 1.2619).abs() < 1e-9);
        assert!((sectors[1].change_pct + 0.1942).abs() < 1e-9);
    }

    #[test]
    fn sector_rows_are_deduped_by_sector_name() {
        // A per-exchange variant could repeat a sector; only the first is kept.
        let body = r#"[
            {"sector":"Technology","exchange":"NASDAQ","averageChange":1.0},
            {"sector":"Technology","exchange":"NYSE","averageChange":2.0}
        ]"#;
        let sectors = parse_sectors(200, body).unwrap();
        assert_eq!(sectors.len(), 1);
        assert!((sectors[0].change_pct - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sector_non_2xx_is_an_error() {
        assert!(parse_sectors(400, "").is_err());
    }

    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY"]
    fn fmp_baseline_smoke() {
        let src = FmpDataSource::from_env().expect("FMP_API_KEY set");
        let data = src.baseline_scan().expect("live baseline scan");

        // Print the resolved mapping so a maintainer can confirm each symbol /
        // endpoint actually came back (run with `-- --ignored --nocapture`); the
        // offline tests can only check fixture shapes, not the live symbols.
        let dump = |label: &str, quotes: &[Quote]| {
            eprintln!("{label} ({}):", quotes.len());
            for q in quotes {
                eprintln!(
                    "  {:<10} {:<28} price={:<12} change_pct={}",
                    q.symbol, q.name, q.price, q.change_pct
                );
            }
        };
        dump("indices", &data.indices);
        dump("internals", &data.internals);
        eprintln!("sectors ({}):", data.sectors.len());
        for s in &data.sectors {
            eprintln!("  {:<24} change_pct={}", s.sector, s.change_pct);
        }

        // Each FMP-owned group should resolve to at least one live row on the free
        // tier — an empty group means the symbols or the endpoint did not map.
        assert!(!data.indices.is_empty(), "no index quotes resolved");
        assert!(!data.internals.is_empty(), "no VIX quote resolved");
        assert!(!data.sectors.is_empty(), "no sector rows resolved");
    }
}
