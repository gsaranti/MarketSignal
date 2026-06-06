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
//! Degradation policy. The guiding rule: **skip only when FMP explicitly signals an
//! absence; fail on anything we can't understand.** Disposition is decided by status
//! first (`classify_status`), with an explicit *skip allowlist*.
//! - **Fatal** — auth (401/403); a systemic failure (a 429 rate limit, a 5xx, or a
//!   200 `{"Error Message"}` body — FMP's rate-limit / plan signal); a request-contract
//!   error (400/408/422/any other non-2xx), so a broken request fails loudly instead
//!   of vanishing into empty data; a **malformed 2xx body** that won't parse into the
//!   expected shape (a contract violation, distinct from an empty "no data" array); or
//!   a transport error. The whole scan fails, which `jobs::run_job` records as a failed
//!   job (`docs/scheduling.md §Offline Behavior`).
//! - **Per-symbol skip** — a 402 (premium) or 404 (not found): FMP explicitly signals
//!   this one symbol is absent, so it is skipped and the rest of the scan lands. An
//!   empty "no data" array likewise contributes nothing but is not an error.
//! - **Floor** — even with skips, a scan that resolves *no* index quotes at all fails
//!   rather than returning an empty baseline (Step 6 is not optional).

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
/// optional (filled from the local label when absent), but `price` and the
/// percent change are **required**: a quote missing either fails the parse so the
/// symbol is skipped rather than reaching the model as a false `0.0`. The change
/// field is `changePercentage` on the stable API, with the legacy
/// `changesPercentage` accepted as an alias.
#[derive(Debug, Deserialize)]
struct FmpQuoteRaw {
    symbol: String,
    #[serde(default)]
    name: String,
    price: f64,
    #[serde(rename = "changePercentage", alias = "changesPercentage")]
    change_pct: f64,
}

/// What to do with an HTTP status, before looking at the body. The skippable set
/// is an explicit *allowlist* — only a premium-gated symbol (402) or a not-found
/// symbol (404) is a legitimate per-symbol absence. Every other non-2xx is fatal,
/// including request-contract errors (400/408/422/…), so a broken request can't
/// silently masquerade as a missing symbol (a malformed sector `date`, say, returns
/// HTTP 400 — which must fail loudly, not yield an empty sector group).
#[derive(Debug, PartialEq, Eq)]
enum Disposition {
    /// 2xx — parse the body (a 200 error-body is still caught by `fmp_error_message`).
    Process,
    /// A legitimate per-symbol absence — skip just this symbol, keep the scan going.
    SkipSymbol,
    /// The key was rejected (401/403) — fatal.
    FatalAuth,
    /// The provider is failing for every request — a rate limit (429) or a 5xx — fatal.
    FatalSystemic,
    /// A request-level error (400/408/422/any other non-2xx) — fatal, since it
    /// signals a broken request contract rather than a missing symbol.
    FatalRequest,
}

fn classify_status(status: u16) -> Disposition {
    match status {
        200..=299 => Disposition::Process,
        401 | 403 => Disposition::FatalAuth,
        402 | 404 => Disposition::SkipSymbol,
        429 | 500..=599 => Disposition::FatalSystemic,
        _ => Disposition::FatalRequest,
    }
}

/// FMP signals rate-limit / plan conditions as a **200** body that is an
/// `{"Error Message": ...}` object — distinct from a 200 *array* (real data, possibly
/// an empty `[]` for "no data"). On a successful status that is an abnormal,
/// scan-level condition the caller treats as **fatal**, never a per-symbol miss (a
/// per-symbol absence is an empty array, not an error object).
///
/// Gated on a 2xx status on purpose: a non-2xx is already classified by its status
/// (402 premium / 404 → per-symbol skip; 429 / 5xx → systemic-fatal), so the *body*
/// encoding must not change a non-2xx disposition — otherwise a 402 with a JSON error
/// body would be fatal while the same 402 with a plain-text body would skip. Returns
/// the message (on a 2xx) so the failure carries FMP's own wording; we don't
/// string-match it — rate-limit, plan, and bad-request all warrant failing the scan.
fn fmp_error_message(status: u16, body: &str) -> Option<String> {
    if !(200..300).contains(&status) {
        return None;
    }
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("Error Message")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Parse an FMP body that should be a JSON array on success. Any non-2xx is an
/// error; the 200-`Error Message` case is handled by the caller via
/// `fmp_error_message` (it is fatal, not a parse failure). Auth (401/403) is also a
/// non-2xx here, but callers check `is_auth_failure` *first* because auth is fatal.
fn fmp_json(status: u16, body: &str) -> Result<Value> {
    if !(200..300).contains(&status) {
        bail!("Financial Modeling Prep returned HTTP {status}");
    }
    serde_json::from_str(body).context("parsing FMP response JSON")
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
        if sector.is_empty() {
            continue;
        }
        // A row without a usable numeric averageChange is dropped, not reported as
        // a false 0.0 ("flat") move.
        let change_pct = match item.get("averageChange").and_then(Value::as_f64) {
            Some(v) => v,
            None => continue,
        };
        if !seen.insert(sector.clone()) {
            continue;
        }
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

    /// Fetch one quote per symbol. Disposition is decided by `classify_status`:
    /// a 402/404 skips just that symbol; auth, systemic, and request-contract errors
    /// fail the whole scan; a 2xx is parsed (a 200 error-body or a shape mismatch is
    /// then handled). So the rest of the scan lands around a legitimately-absent
    /// symbol, but a broken request or a failing provider fails loudly.
    fn fetch_quotes(&self, symbols: &[(&str, &str)]) -> Result<Vec<Quote>> {
        let mut out = Vec::with_capacity(symbols.len());
        for (symbol, fallback_name) in symbols {
            let (status, body) = self.get(FMP_QUOTE_URL, &[("symbol", symbol)])?;
            match classify_status(status) {
                Disposition::SkipSymbol => continue,
                Disposition::FatalAuth => {
                    bail!("Financial Modeling Prep rejected the key (HTTP {status})")
                }
                Disposition::FatalSystemic => bail!(
                    "Financial Modeling Prep is unavailable (HTTP {status}) — failing the scan \
                     rather than returning a partial baseline"
                ),
                Disposition::FatalRequest => bail!(
                    "Financial Modeling Prep rejected the request (HTTP {status}) — failing the \
                     scan rather than masking a broken request as a missing symbol"
                ),
                Disposition::Process => {
                    if let Some(msg) = fmp_error_message(status, &body) {
                        bail!(
                            "Financial Modeling Prep returned an error response (\"{msg}\") — \
                             failing the scan rather than returning a partial baseline"
                        );
                    }
                    // A 2xx that won't parse into quotes is a contract violation, not a
                    // legitimate absence (which is an empty array) — fail rather than
                    // silently dropping the symbol and hiding schema drift.
                    let quotes = parse_quotes(status, &body, fallback_name).with_context(|| {
                        format!(
                            "Financial Modeling Prep returned an unparseable quote for {symbol} \
                             (HTTP {status}) — failing the scan rather than dropping it silently"
                        )
                    })?;
                    out.extend(quotes);
                }
            }
        }
        Ok(out)
    }

    /// Fetch the most recent sector-performance snapshot, walking back from today
    /// to the last trading day that has data (weekends / holidays have none).
    /// Disposition follows `classify_status`: a 404 for a date with no snapshot skips
    /// to the previous day; auth, systemic, and request-contract errors (e.g. a 400
    /// from a malformed `date`) fail the scan loudly; a 2xx with data returns. If no
    /// day in the window has a snapshot, the scan soft-degrades to no sector data.
    fn fetch_sectors(&self) -> Result<Vec<SectorPerformance>> {
        let today = Utc::now().date_naive();
        for back in 0..=SECTOR_LOOKBACK_DAYS {
            let date = (today - Duration::days(back)).format("%Y-%m-%d").to_string();
            let (status, body) = self.get(FMP_SECTOR_URL, &[("date", date.as_str())])?;
            match classify_status(status) {
                Disposition::SkipSymbol => continue, // no snapshot for this date — try the prior day
                Disposition::FatalAuth => {
                    bail!("Financial Modeling Prep rejected the key (HTTP {status})")
                }
                Disposition::FatalSystemic => bail!(
                    "Financial Modeling Prep is unavailable (HTTP {status}) — failing the scan \
                     rather than returning a partial baseline"
                ),
                Disposition::FatalRequest => bail!(
                    "Financial Modeling Prep rejected the request (HTTP {status}) — failing the \
                     scan rather than masking a broken request as missing sector data"
                ),
                Disposition::Process => {
                    if let Some(msg) = fmp_error_message(status, &body) {
                        bail!(
                            "Financial Modeling Prep returned an error response (\"{msg}\") — \
                             failing the scan rather than returning a partial baseline"
                        );
                    }
                    // A 2xx that won't parse is a contract violation -> fatal; an empty
                    // array is just "no snapshot for this date" -> try the prior day.
                    let sectors = parse_sectors(status, &body).with_context(|| {
                        format!(
                            "Financial Modeling Prep returned an unparseable sector snapshot for \
                             {date} (HTTP {status}) — failing the scan rather than masking it"
                        )
                    })?;
                    if !sectors.is_empty() {
                        return Ok(sectors);
                    }
                }
            }
        }
        Ok(Vec::new())
    }
}

impl MarketDataSource for FmpDataSource {
    fn baseline_scan(&self) -> Result<BaselineMarketData> {
        let indices = self.fetch_quotes(INDEX_SYMBOLS)?;
        // Completeness floor: per-symbol failures soft-skip, but resolving *no*
        // index quotes at all means the provider is unreachable, rate-limited, or
        // returning an unrecognized shape — fail the scan rather than hand the
        // agent an empty, ungrounded baseline (Step 6 is not optional). Checked on
        // indices because the report's Index Picture structurally needs them; an
        // empty VIX or sector list still soft-degrades.
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
        let quotes = parse_quotes(
            200,
            r#"[{"symbol":"^DJI","price":40000.0,"changePercentage":0.1}]"#,
            "Dow Jones",
        )
        .unwrap();
        assert_eq!(quotes[0].name, "Dow Jones");
    }

    #[test]
    fn quote_missing_a_required_numeric_is_an_error() {
        // A required field absent (schema drift / partial response) fails the parse,
        // which fetch_quotes treats as a fatal contract violation — neither a false
        // 0.0 nor a silent per-symbol skip.
        let no_price = parse_quotes(200, r#"[{"symbol":"^GSPC","changePercentage":0.4}]"#, "x");
        assert!(no_price.is_err(), "missing price should error");
        let no_change = parse_quotes(200, r#"[{"symbol":"^GSPC","price":5500.0}]"#, "x");
        assert!(no_change.is_err(), "missing changePercentage should error");
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
    fn classify_status_maps_statuses_to_dispositions() {
        use Disposition::*;
        assert_eq!(classify_status(200), Process);
        // Explicit skip allowlist: only premium-gated / not-found symbols skip.
        assert_eq!(classify_status(402), SkipSymbol);
        assert_eq!(classify_status(404), SkipSymbol);
        assert_eq!(classify_status(401), FatalAuth);
        assert_eq!(classify_status(403), FatalAuth);
        // Systemic: rate limit + server errors hit every request.
        assert_eq!(classify_status(429), FatalSystemic);
        assert_eq!(classify_status(500), FatalSystemic);
        assert_eq!(classify_status(503), FatalSystemic);
        // Everything else non-2xx is a request-contract failure, NOT a silent skip:
        // a broken request (e.g. a malformed sector date -> 400) fails the scan
        // loudly rather than masquerading as a missing symbol.
        assert_eq!(classify_status(400), FatalRequest);
        assert_eq!(classify_status(408), FatalRequest);
        assert_eq!(classify_status(422), FatalRequest);
    }

    #[test]
    fn error_message_body_is_fatal_only_on_a_successful_status() {
        let body = r#"{"Error Message":"Limit Reach. Please upgrade your plan or visit our documentation."}"#;
        // A 200 with an error body is FMP's rate-limit / plan signal -> fatal.
        assert_eq!(
            fmp_error_message(200, body).as_deref(),
            Some("Limit Reach. Please upgrade your plan or visit our documentation.")
        );
        // The SAME body on a non-2xx must not be promoted to fatal: the status
        // already classifies it (402 premium / 404 -> per-symbol skip), so the body
        // encoding cannot change a non-2xx disposition.
        assert!(fmp_error_message(402, body).is_none());
        assert!(fmp_error_message(404, body).is_none());
        // A normal array — including the empty "no data" array — is data, not an
        // error, so it is never misread as a fatal condition.
        assert!(fmp_error_message(
            200,
            r#"[{"symbol":"^GSPC","price":1.0,"changePercentage":0.1}]"#
        )
        .is_none());
        assert!(fmp_error_message(200, "[]").is_none());
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
    fn sector_row_missing_average_change_is_skipped() {
        // A row without a usable averageChange is dropped, not zeroed to "flat".
        let body = r#"[
            {"sector":"Technology","exchange":"NASDAQ","averageChange":1.5},
            {"sector":"Energy","exchange":"NASDAQ"}
        ]"#;
        let sectors = parse_sectors(200, body).unwrap();
        assert_eq!(sectors.len(), 1);
        assert_eq!(sectors[0].sector, "Technology");
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
