//! Real Financial Modeling Prep adapter for the baseline market-data scan.
//!
//! The first data-source adapter behind the `MarketDataSource` trait
//! (`data_sources`). On FMP's free tier the provider is effectively an *equities*
//! API, so this adapter owns the equity-market half of the Step-6 baseline:
//! the market **indices** (Dow / S&P 500 / Nasdaq / Russell 2000), the **VIX**,
//! **gold** (`GCUSD`, free on the quote endpoint), and **sector performance**. The
//! remaining macro / commodity internals — Treasury yields, the dollar index, oil,
//! and natural gas — are gated behind FMP premium (verified live: HTTP 402 "not
//! available under your current subscription") and are sourced from FRED instead.
//! Each is a canonical free FRED series; see `docs/data-sources.md` (amended to
//! reflect this split).
//!
//! Like `model_agent`, the HTTP call is synchronous (`reqwest::blocking`) so the
//! trait stays sync; the blocking work is offloaded via `spawn_blocking` at the
//! Tauri command seam. The key rides as a query param, never an Authorization
//! header — the convention `connection_test` verified live (Jun 2026).
//!
//! Degradation policy. The guiding rule: **skip only when FMP explicitly signals an
//! absence; fail on anything we can't understand.** One pure function,
//! `interpret_response`, decides this for every response:
//! - **Fatal** (`Err`) — auth (401/403); a systemic failure (a 429 rate limit, a 5xx,
//!   or a 200 `{"Error Message"}` body — FMP's rate-limit / plan signal); a
//!   request-contract error (400/408/422/any other non-2xx), so a broken request fails
//!   loudly instead of vanishing into empty data; a malformed 2xx body that won't parse;
//!   or a transport error. The whole scan fails, which `jobs::run_job` records as a
//!   failed job (`docs/scheduling.md §Offline Behavior`).
//! - **Per-symbol skip** (`Ok(None)`) — a 402 (premium) or 404 (not found): FMP
//!   explicitly signals this one symbol is absent, so it is skipped and the rest of the
//!   scan lands. An empty "no data" array (`Ok(Some([]))`) likewise contributes nothing
//!   but is not an error.
//! - **Floor** — even with skips, a scan that resolves *no* index quotes at all fails
//!   rather than returning an empty baseline (Step 6 is not optional).

use std::collections::HashSet;
use std::time::Duration as StdDuration;

use anyhow::{bail, Context, Result};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{BaselineMarketData, MarketDataSource, Quote, SectorPerformance};

/// FMP's stable single-symbol quote endpoint — the one `connection_test` exercises.
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
/// .md §Step 6`), paired with a display name used when FMP omits one and the `price`
/// unit. All four are free-tier on FMP (verified live). The unit rides from the table,
/// not the wire — FMP's quote object carries no unit — and labels the level for the
/// model the same way `fred`'s and `bls`'s series tables do.
const INDEX_SYMBOLS: &[(&str, &str, &str)] = &[
    ("^DJI", "Dow Jones Industrial Average", "index points"),
    ("^GSPC", "S&P 500", "index points"),
    ("^IXIC", "Nasdaq Composite", "index points"),
    ("^RUT", "Russell 2000", "index points"),
];

/// The free-tier market internals FMP serves: the VIX and gold (`GCUSD`, verified
/// live on the free quote endpoint), each with its `price` unit. The dollar index,
/// oil, and natural gas are FMP-premium and come from FRED instead (see the module
/// header).
const INTERNAL_SYMBOLS: &[(&str, &str, &str)] = &[
    ("^VIX", "CBOE Volatility Index", "index points"),
    ("GCUSD", "Gold", "USD per troy ounce"),
];

/// FMP's quote object, trimmed to the fields the baseline needs. `name` is optional
/// (filled from the local label when absent), but `price` and the percent change are
/// **required**: a quote missing either fails the parse, which the scan treats as a
/// fatal contract violation rather than reaching the model as a false `0.0`. The change
/// field is `changePercentage` on the stable API, with the legacy `changesPercentage`
/// accepted as an alias.
#[derive(Debug, Deserialize)]
struct FmpQuoteRaw {
    symbol: String,
    #[serde(default)]
    name: String,
    price: f64,
    #[serde(rename = "changePercentage", alias = "changesPercentage")]
    change_pct: f64,
}

/// One row of FMP's sector-performance snapshot. `sector` and `averageChange` are
/// **required** — a row missing either fails the parse (a fatal contract violation),
/// rather than being silently dropped. The snapshot's `date` / `exchange` fields are
/// ignored.
#[derive(Debug, Deserialize)]
struct FmpSectorRaw {
    sector: String,
    #[serde(rename = "averageChange")]
    average_change: f64,
}

/// Interpret one FMP response by the full status × body matrix — the single place the
/// degradation policy lives. Pure and total:
/// - `Err(..)` — fatal (auth, systemic, request-contract, a 200 `{"Error Message"}`
///   body, or an unparseable body): fail the whole scan.
/// - `Ok(None)` — a legitimate per-symbol absence (402 premium / 404 not found): skip.
/// - `Ok(Some(value))` — a successful 2xx JSON value (an array, possibly empty) for the
///   caller to shape; an empty array is `Ok(Some([]))` ("no data"), not an error.
///
/// Status decides disposition first, with an explicit *skip allowlist* (402/404), so a
/// non-2xx is never reclassified by its body (a 402 with a JSON error body must skip
/// just like a 402 with a plain-text body). Only on a 2xx is the body inspected, where
/// FMP's `{"Error Message"}` rate-limit / plan signal and an unparseable body are both
/// fatal — distinct from an empty "no data" array, which parses fine.
fn interpret_response(status: u16, body: &str) -> Result<Option<Value>> {
    match status {
        200..=299 => {} // fall through to body handling
        402 | 404 => return Ok(None),
        401 | 403 => bail!("Financial Modeling Prep rejected the key (HTTP {status})"),
        429 | 500..=599 => bail!(
            "Financial Modeling Prep is unavailable (HTTP {status}) — failing the scan \
             rather than returning a partial baseline"
        ),
        _ => bail!(
            "Financial Modeling Prep rejected the request (HTTP {status}) — failing the scan \
             rather than masking a broken request as missing data"
        ),
    }
    let value: Value = serde_json::from_str(body)
        .context("Financial Modeling Prep returned an unparseable 2xx body")?;
    if let Some(msg) = value.get("Error Message").and_then(Value::as_str) {
        bail!(
            "Financial Modeling Prep returned an error response (\"{msg}\") — failing the scan \
             rather than returning a partial baseline"
        );
    }
    Ok(Some(value))
}

/// Shape a successful quote response (a single-symbol `/stable/quote` call returns a
/// one-element array) into typed quotes, falling back to `fallback_name` when FMP omits
/// the instrument name and stamping each with the requested symbol's `unit` (FMP's quote
/// object carries none). A body that is not the expected array of quotes is an error.
fn quotes_from_value(value: Value, fallback_name: &str, unit: &str) -> Result<Vec<Quote>> {
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
            unit: unit.to_string(),
        })
        .collect())
}

/// Shape a successful sector snapshot into typed rows, deduplicated by sector name (the
/// default call returns one row per sector, but a per-exchange variant could repeat
/// them). A body that is not the expected array of sector rows is an error.
fn sectors_from_value(value: Value) -> Result<Vec<SectorPerformance>> {
    let raws: Vec<FmpSectorRaw> = serde_json::from_value(value)
        .context("FMP sector response did not match the expected array shape")?;
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(raws.len());
    for raw in raws {
        if seen.insert(raw.sector.clone()) {
            out.push(SectorPerformance {
                sector: raw.sector,
                change_pct: raw.average_change,
            });
        }
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
    /// and raw body for `interpret_response` to judge. A transport error (the
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

    /// Fetch one quote per symbol. `interpret_response` decides each response: a
    /// 402/404 skips just that symbol; auth / systemic / request-contract / malformed
    /// responses fail the whole scan; a 2xx array is shaped into quotes. So the rest of
    /// the scan lands around a legitimately-absent symbol, but anything we can't
    /// understand fails loudly.
    fn fetch_quotes(&self, symbols: &[(&str, &str, &str)]) -> Result<Vec<Quote>> {
        let mut out = Vec::with_capacity(symbols.len());
        for (symbol, fallback_name, unit) in symbols {
            let (status, body) = self.get(FMP_QUOTE_URL, &[("symbol", symbol)])?;
            if let Some(value) = interpret_response(status, &body)? {
                out.extend(quotes_from_value(value, fallback_name, unit)?);
            }
        }
        Ok(out)
    }

    /// Fetch the most recent sector-performance snapshot, walking back from today to the
    /// last trading day with data (weekends / holidays have none). `interpret_response`
    /// decides each date's response: a 404 (or an empty array) means no snapshot for that
    /// date — try the prior day; auth / systemic / request-contract / malformed responses
    /// fail the scan; a 2xx with rows returns. If no day in the window has a snapshot, the
    /// scan soft-degrades to no sector data.
    fn fetch_sectors(&self) -> Result<Vec<SectorPerformance>> {
        let today = Utc::now().date_naive();
        for back in 0..=SECTOR_LOOKBACK_DAYS {
            let date = (today - Duration::days(back)).format("%Y-%m-%d").to_string();
            let (status, body) = self.get(FMP_SECTOR_URL, &[("date", date.as_str())])?;
            if let Some(value) = interpret_response(status, &body)? {
                let sectors = sectors_from_value(value)?;
                if !sectors.is_empty() {
                    return Ok(sectors);
                }
            }
            // None (404) or an empty array — no snapshot for this date; try the prior day.
        }
        Ok(Vec::new())
    }
}

impl MarketDataSource for FmpDataSource {
    fn baseline_scan(&self) -> Result<BaselineMarketData> {
        let indices = self.fetch_quotes(INDEX_SYMBOLS)?;
        // Completeness floor: per-symbol failures soft-skip, but resolving *no* index
        // quotes at all means the provider is unreachable, rate-limited, or returning an
        // unrecognized shape — fail the scan rather than hand the agent an empty,
        // ungrounded baseline (Step 6 is not optional). Checked on indices because the
        // report's Index Picture structurally needs them; an empty VIX or sector list
        // still soft-degrades.
        if indices.is_empty() {
            bail!(
                "FMP baseline scan resolved no index quotes — the data provider is \
                 unreachable, rate-limited, or returned an unrecognized response"
            );
        }
        Ok(BaselineMarketData {
            indices,
            internals: self.fetch_quotes(INTERNAL_SYMBOLS)?,
            sectors: self.fetch_sectors()?,
            // FRED owns the macro levels and BLS the labor levels; FMP contributes
            // neither.
            macro_levels: Vec::new(),
            labor_levels: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_response_covers_the_full_matrix() {
        // 2xx array (incl. the empty "no data" array) -> Some(value) to shape.
        assert!(interpret_response(200, r#"[{"symbol":"^GSPC","price":1.0,"changePercentage":0.1}]"#)
            .unwrap()
            .is_some());
        assert!(interpret_response(200, "[]").unwrap().is_some());

        // Explicit skip allowlist: a legitimate per-symbol absence -> None.
        assert!(interpret_response(402, "Premium Query Parameter").unwrap().is_none());
        assert!(interpret_response(404, "").unwrap().is_none());

        // Auth / systemic / request-contract -> fatal, regardless of body. In
        // particular a 400 (e.g. a malformed sector date) fails loudly rather than
        // silently skipping.
        for status in [401, 403, 429, 500, 503, 400, 408, 422] {
            assert!(interpret_response(status, "").is_err(), "HTTP {status} should be fatal");
        }

        // A 200 {"Error Message"} body (rate-limit / plan) is fatal...
        assert!(interpret_response(200, r#"{"Error Message":"Limit Reach"}"#).is_err());
        // ...but the SAME body on a non-2xx is classified by status, not body (402 skips).
        assert!(interpret_response(402, r#"{"Error Message":"Premium"}"#).unwrap().is_none());
        // A 2xx that isn't valid JSON is a contract violation -> fatal.
        assert!(interpret_response(200, "not json at all").is_err());
    }

    #[test]
    fn quotes_from_value_maps_with_name_fallback_and_legacy_alias() {
        let v: Value =
            serde_json::from_str(r#"[{"symbol":"^GSPC","name":"S&P 500","price":5500.5,"changePercentage":0.42}]"#)
                .unwrap();
        let quotes = quotes_from_value(v, "fallback", "index points").unwrap();
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].symbol, "^GSPC");
        assert_eq!(quotes[0].name, "S&P 500");
        assert!((quotes[0].price - 5500.5).abs() < 1e-9);
        assert!((quotes[0].change_pct - 0.42).abs() < 1e-9);
        // The requested symbol's unit rides onto the quote from the table, not the wire.
        assert_eq!(quotes[0].unit, "index points");

        // No name -> local fallback; legacy `changesPercentage` accepted.
        let v2: Value =
            serde_json::from_str(r#"[{"symbol":"^DJI","price":40000.0,"changesPercentage":-1.5}]"#).unwrap();
        let q2 = quotes_from_value(v2, "Dow Jones", "index points").unwrap();
        assert_eq!(q2[0].name, "Dow Jones");
        assert!((q2[0].change_pct + 1.5).abs() < 1e-9);

        // An empty array is "no quotes", not an error.
        assert!(quotes_from_value(serde_json::from_str("[]").unwrap(), "x", "index points").unwrap().is_empty());
    }

    #[test]
    fn quotes_from_value_requires_price_and_change() {
        // A required field absent (schema drift / partial response) fails the parse —
        // neither a false 0.0 nor a silent skip; the loop fails the scan.
        let no_price: Value =
            serde_json::from_str(r#"[{"symbol":"^GSPC","changePercentage":0.4}]"#).unwrap();
        assert!(quotes_from_value(no_price, "x", "index points").is_err());
        let no_change: Value = serde_json::from_str(r#"[{"symbol":"^GSPC","price":5500.0}]"#).unwrap();
        assert!(quotes_from_value(no_change, "x", "index points").is_err());
        // A non-array 2xx body (object) is also malformed.
        let object: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(quotes_from_value(object, "x", "index points").is_err());
    }

    #[test]
    fn sectors_from_value_maps_and_dedupes_by_sector() {
        let v: Value = serde_json::from_str(
            r#"[
                {"date":"2026-06-04","sector":"Technology","exchange":"NASDAQ","averageChange":1.2619},
                {"date":"2026-06-04","sector":"Energy","exchange":"NASDAQ","averageChange":-0.1942}
            ]"#,
        )
        .unwrap();
        let sectors = sectors_from_value(v).unwrap();
        assert_eq!(sectors.len(), 2);
        assert_eq!(sectors[0].sector, "Technology");
        assert!((sectors[0].change_pct - 1.2619).abs() < 1e-9);
        assert!((sectors[1].change_pct + 0.1942).abs() < 1e-9);

        // A per-exchange variant could repeat a sector; only the first is kept.
        let dup: Value = serde_json::from_str(
            r#"[
                {"sector":"Technology","exchange":"NASDAQ","averageChange":1.0},
                {"sector":"Technology","exchange":"NYSE","averageChange":2.0}
            ]"#,
        )
        .unwrap();
        let d = sectors_from_value(dup).unwrap();
        assert_eq!(d.len(), 1);
        assert!((d[0].change_pct - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sectors_from_value_requires_average_change() {
        // Fail-closed: a row missing averageChange fails the parse (fatal in the loop),
        // rather than being silently dropped as a false "flat" move.
        let v: Value = serde_json::from_str(
            r#"[{"sector":"Technology","averageChange":1.5},{"sector":"Energy"}]"#,
        )
        .unwrap();
        assert!(sectors_from_value(v).is_err());
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
                    "  {:<10} {:<28} price={:<12} change_pct={:<10} unit={}",
                    q.symbol, q.name, q.price, q.change_pct, q.unit
                );
            }
        };
        dump("indices", &data.indices);
        dump("internals", &data.internals);
        eprintln!("sectors ({}):", data.sectors.len());
        for s in &data.sectors {
            eprintln!("  {:<24} change_pct={}", s.sector, s.change_pct);
        }

        // Every named symbol must resolve individually — not just "the group is
        // non-empty". A group-level check lets one symbol leaving the free tier
        // (e.g. GCUSD going premium) hide behind its siblings; the per-symbol
        // assert is what actually catches a symbol regressing, the lesson of the
        // removed FRED gold series.
        let assert_resolved = |label: &str, quotes: &[Quote], symbols: &[(&str, &str, &str)]| {
            for (sym, _, _) in symbols {
                assert!(
                    quotes.iter().any(|q| q.symbol == *sym),
                    "{label}: {sym} did not resolve — it may have left FMP's free tier"
                );
            }
        };
        assert_resolved("indices", &data.indices, INDEX_SYMBOLS);
        assert_resolved("internals", &data.internals, INTERNAL_SYMBOLS);
        assert!(!data.sectors.is_empty(), "no sector rows resolved");
    }
}
