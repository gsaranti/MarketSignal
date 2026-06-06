//! Real BLS (Bureau of Labor Statistics) adapter for the labor half of the
//! baseline market-data scan.
//!
//! The third data-source adapter behind the `MarketDataSource` trait
//! (`data_sources`), sibling to `fmp` and `fred`. It owns the Step-6 **labor levels**
//! group (`labor_levels`): the CPI-U headline index, the U-3 unemployment rate, total
//! nonfarm payrolls, and average hourly earnings (`docs/weekly-report-workflow.md
//! §Step 6`, `docs/data-sources.md §BLS`). These are point-in-time monthly levels
//! reusing the same quote shape as `fred`'s macro levels, kept in a group distinct
//! from the FRED `macro_levels` by source and concern. `price` is the latest reported
//! level; `change_pct` is its month-over-month change from the prior reading.
//!
//! Unlike FMP and FRED, BLS is **keyless** (`docs/configuration.md` — BLS/GDELT need
//! no credential), so `BlsDataSource::new()` takes no key and BLS is not part of the
//! execution gate. The trade-off is the public v2 tier's 25-query/day cap, which the
//! once-weekly job sits comfortably under; a single batched request per scan keeps the
//! burn at one query.
//!
//! Like `fmp`/`fred`, the HTTP call is synchronous (`reqwest::blocking`) so the trait
//! stays sync; the blocking work is offloaded via `spawn_blocking` at the Tauri
//! command seam.
//!
//! Degradation policy. The same rule as the sibling adapters: **skip only when BLS
//! signals a per-series absence; fail on anything we can't understand.** BLS differs
//! from FRED in *how* it signals: it answers HTTP 200 even for a rejected request,
//! reporting the outcome in the JSON `status` field (`REQUEST_SUCCEEDED` vs
//! `REQUEST_NOT_PROCESSED`) with a human `message`. So `interpret_response` classifies
//! by that in-body status — a not-processed request (malformed batch or the daily
//! threshold) is systemic-fatal, fail-closed — rather than by HTTP status alone. An
//! explicit per-series absence — a requested series returned with an empty `data`
//! array — is a soft per-series skip, like FRED's `"."` gaps. A series *omitted* from
//! a successful response is different: BLS returns invalid series as explicit empty
//! entries, so an omission signals a truncated / anomalous response and fails the scan
//! (fail-closed). The per-group completeness floor backstops both, failing if the whole
//! `labor_levels` group comes back empty (Step 6 is not optional).

use std::time::Duration as StdDuration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Datelike;
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{BaselineMarketData, MarketDataSource, Quote};

/// BLS Public Data API v2 time-series endpoint — a JSON POST batch of series ids.
const BLS_DATA_URL: &str = "https://api.bls.gov/publicAPI/v2/timeseries/data/";

/// Short timeout for the single batched request, matching the sibling adapters'
/// ceiling so a hung provider doesn't park the scan.
const BLS_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// The BLS-owned labor levels of the Step-6 baseline (`docs/weekly-report-workflow.md
/// §Step 6`, `docs/data-sources.md §BLS`): the CPI-U headline index (NSA, all items),
/// the U-3 unemployment rate, total nonfarm payroll employment, and average hourly
/// earnings for total private. Monthly series; `change_pct` reads month-over-month.
/// `(series_id, display name, unit)`, with the BLS `series_id` doubling as the quote
/// `symbol` — the same shape as `fred`'s series tables.
///
/// The `unit` labels `price` so the model reading the serialized baseline can't make a
/// 1000× misread of the payroll level (counted in thousands of persons) or confuse the
/// CPI index level with a percent. (This unit previously rode inline in the display
/// name as a stopgap; it now lives in the `Quote.unit` field across all three
/// adapters.)
const LABOR_SERIES: &[(&str, &str, &str)] = &[
    (
        "CUUR0000SA0",
        "Consumer Price Index (CPI-U, All Items)",
        "index (1982-84=100)",
    ),
    ("LNS14000000", "Unemployment Rate", "percent"),
    (
        "CES0000000001",
        "Total Nonfarm Payrolls",
        "thousands of persons",
    ),
    (
        "CES0500000003",
        "Average Hourly Earnings, Total Private",
        "USD per hour",
    ),
];

/// BLS response envelope, trimmed to the fields the baseline needs. `status` and
/// `message` are always present (even on a rejected request); `results` is left as a
/// raw `Value` because BLS shapes it as `{}` on a not-processed request and
/// `{"series":[...]}` on success — deserializing it into a struct unconditionally
/// would fail on the former, so it is parsed into `BlsResults` only after `status`
/// confirms success.
#[derive(Debug, Deserialize)]
struct BlsResponse {
    status: String,
    #[serde(default)]
    message: Vec<String>,
    #[serde(default, rename = "Results")]
    results: Value,
}

#[derive(Debug, Default, Deserialize)]
struct BlsResults {
    #[serde(default)]
    series: Vec<BlsSeries>,
}

#[derive(Debug, Deserialize)]
struct BlsSeries {
    #[serde(rename = "seriesID")]
    series_id: String,
    #[serde(default)]
    data: Vec<BlsDataPoint>,
}

/// One observation in a series. Only `value` is read (a number like `"320.321"`);
/// `year`/`period` are ignored — BLS returns `data` newest-first, so the shaper takes
/// the leading entries without re-sorting, the same trust `fred` places in its
/// `sort_order=desc`.
#[derive(Debug, Deserialize)]
struct BlsDataPoint {
    value: String,
}

/// Interpret one BLS batch response by HTTP status × in-body `status` — the single
/// place the request-level degradation policy lives. Pure and total:
/// - `Ok(resp)` — a `REQUEST_SUCCEEDED` 2xx, for the caller to read series from.
/// - `Err(..)` — fatal: a non-2xx (transport / server), a `REQUEST_NOT_PROCESSED`
///   (malformed batch, rejected/over-limit key, or the keyless daily threshold), an
///   unrecognized `status`, or an unparseable body.
///
/// Unlike FMP's status allowlist and FRED's by-body 400 split, BLS answers **HTTP 200
/// for errors too**, carrying the real outcome in the JSON `status`. So a not-processed
/// request fails closed here rather than vanishing into missing data; a genuinely
/// absent *series* is handled downstream (empty `data` → per-series skip), not here.
fn interpret_response(http_status: u16, body: &str) -> Result<BlsResponse> {
    if !(200..=299).contains(&http_status) {
        // BLS normally answers 200 even for rejected requests, so a non-2xx is a
        // transport/server fault: systemic-fatal, like FRED's 429 / 5xx.
        bail!(
            "BLS is unavailable (HTTP {http_status}) — failing the scan rather than returning a \
             partial baseline"
        );
    }
    let resp: BlsResponse =
        serde_json::from_str(body).context("BLS returned an unparseable body")?;
    match resp.status.as_str() {
        "REQUEST_SUCCEEDED" => Ok(resp),
        other => bail!(
            "BLS did not process the request (status {other}: {}) — failing the scan rather than \
             masking it as missing data",
            resp.message.join("; ")
        ),
    }
}

/// Shape one series' observations into a quote: the most recent value is `price`, and
/// `change_pct` is its percent change from the prior value (month-over-month for these
/// monthly series). Returns `Ok(None)` when the series has no observation (empty
/// `data`) — a per-series absence, not an error.
///
/// Fail-closed on the value: a present BLS observation is expected numeric, so any
/// non-numeric value — or one that parses to a non-finite float (`NaN` / `inf`, which
/// `f64::parse` accepts) — is a contract violation that fails the scan rather than
/// being silently dropped (which would let a stale reading masquerade as current, or a
/// `NaN` contaminate the change math). BLS signals "no datum" by omitting the
/// observation, not with a sentinel string, so there is no FRED-style `"."` to skip.
fn series_to_quote(
    data: &[BlsDataPoint],
    symbol: &str,
    name: &str,
    unit: &str,
) -> Result<Option<Quote>> {
    // The most-recent numeric observations, newest-first; latest + prior is all the
    // change needs, so stop at two.
    let mut numeric: Vec<f64> = Vec::with_capacity(2);
    for dp in data {
        let v = dp.value.trim();
        let parsed: f64 = v
            .parse()
            .ok()
            .filter(|n: &f64| n.is_finite())
            .ok_or_else(|| {
                anyhow!("BLS returned a non-numeric observation value {v:?} for series {symbol}")
            })?;
        numeric.push(parsed);
        if numeric.len() == 2 {
            break;
        }
    }
    let Some(&latest) = numeric.first() else {
        return Ok(None); // no observation in the window — per-series absence
    };
    // Percent change off the prior observation; a zero (or absent) prior yields no
    // change rather than a division by zero / spurious move.
    let change_pct = match numeric.get(1) {
        Some(&prev) if prev != 0.0 => (latest - prev) / prev * 100.0,
        _ => 0.0,
    };
    Ok(Some(Quote {
        symbol: symbol.to_string(),
        name: name.to_string(),
        price: latest,
        change_pct,
        unit: unit.to_string(),
    }))
}

/// Per-group completeness floor for the BLS scan. The single Step-6 group BLS owns —
/// `labor_levels` — is non-optional, so an **empty group** fails the scan rather than
/// handing the agent an incomplete baseline. This is distinct from a single absent
/// series, which soft-skips (the gold lesson): a whole group coming back empty means
/// the provider is unreachable, rate-limited (the keyless daily cap), or every series
/// was renamed at once — none of which should pass silently. Mirrors `fred`'s and
/// `fmp`'s required-group floors, and keeps the runtime in step with the smoke, which
/// asserts the group resolves in full.
///
/// Pure, so the floor is unit-testable without an HTTP round-trip — the live scan is
/// otherwise exercised only by the ignored smoke.
fn check_completeness(labor_levels: &[Quote]) -> Result<()> {
    if labor_levels.is_empty() {
        bail!(
            "BLS baseline scan resolved no labor-levels series (CPI, unemployment, payrolls, \
             wages) — the data provider is unreachable, rate-limited, or returned an \
             unrecognized response"
        );
    }
    Ok(())
}

/// Match the parsed BLS results against the authoritative `LABOR_SERIES` list and shape
/// each into a quote, preserving the const's display names and order. Split from the
/// HTTP-bound `baseline_scan` so the omission policy below is unit-testable without a
/// round-trip.
///
/// Two absence cases, deliberately handled differently to honor the fail-closed policy:
/// - A requested series **present but with empty `data`** is an explicit per-series
///   absence (`series_to_quote` returns `None`) — a soft skip, like FRED's all-gap
///   series (the gold lesson).
/// - A requested series **omitted from the response entirely** is not a signal BLS
///   sends in normal operation (it returns invalid series as explicit empty entries),
///   so an omission means a truncated / anomalous response and fails the scan rather
///   than silently thinning the baseline.
fn assemble_labor_levels(results: &BlsResults) -> Result<Vec<Quote>> {
    let mut labor_levels = Vec::with_capacity(LABOR_SERIES.len());
    for (id, name, unit) in LABOR_SERIES {
        let Some(series) = results.series.iter().find(|s| s.series_id == *id) else {
            bail!(
                "BLS omitted requested series {id} from a successful response — failing the \
                 scan rather than masking a truncated response as missing data"
            );
        };
        if let Some(quote) = series_to_quote(&series.data, id, name, unit)? {
            labor_levels.push(quote);
        }
    }
    Ok(labor_levels)
}

/// The `(startyear, endyear)` request window: the prior and current calendar year.
/// Two years guarantees at least two monthly observations are in range even in
/// January (when the current year has only one), so the month-over-month change
/// resolves. Pure over the year so the boundary is unit-testable without the clock.
fn year_window(current_year: i32) -> (String, String) {
    ((current_year - 1).to_string(), current_year.to_string())
}

/// Live BLS adapter behind the `MarketDataSource` trait.
pub struct BlsDataSource {
    http: reqwest::blocking::Client,
}

impl BlsDataSource {
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(BLS_TIMEOUT)
            .build()
            .context("building the BLS HTTP client")?;
        Ok(Self { http })
    }

    /// POST the full labor-series batch in one request, returning the status and raw
    /// body for `interpret_response` to judge. A transport error (the provider is
    /// unreachable) propagates as a fatal scan error.
    fn post(&self) -> Result<(u16, String)> {
        let ids: Vec<&str> = LABOR_SERIES.iter().map(|(id, _, _)| *id).collect();
        let (start, end) = year_window(chrono::Local::now().year());
        let payload = serde_json::json!({
            "seriesid": ids,
            "startyear": start,
            "endyear": end,
        });
        let resp = self
            .http
            .post(BLS_DATA_URL)
            .json(&payload)
            .send()
            .context("sending BLS request")?;
        let status = resp.status().as_u16();
        let body = resp.text().context("reading BLS response body")?;
        Ok((status, body))
    }
}

impl MarketDataSource for BlsDataSource {
    fn baseline_scan(&self) -> Result<BaselineMarketData> {
        let (status, body) = self.post()?;
        let resp = interpret_response(status, &body)?;
        let results: BlsResults = serde_json::from_value(resp.results)
            .context("BLS Results did not match the expected shape")?;

        // Match the expected series, shaping each. Omission fails closed; an explicit
        // empty-data series soft-skips — see `assemble_labor_levels`.
        let labor_levels = assemble_labor_levels(&results)?;

        // The one group BLS owns is a non-optional Step-6 group, so an empty group fails
        // the scan rather than handing the agent an incomplete baseline. BLS fills only
        // labor_levels; the other groups are left empty for the composite to fill from
        // FMP / FRED.
        check_completeness(&labor_levels)?;
        Ok(BaselineMarketData {
            labor_levels,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a series' `data` slice from a JSON array fixture, the way the live parse
    /// would; extra fields (`year`/`period`/`footnotes`) are tolerated and ignored.
    fn data_points(json: &str) -> Vec<BlsDataPoint> {
        serde_json::from_str(json).unwrap()
    }

    /// Build a `BlsResults` from `(series_id, data-array-json)` pairs, the way the live
    /// parse would shape the `Results` object.
    fn results_with(entries: &[(&str, &str)]) -> BlsResults {
        let series = entries
            .iter()
            .map(|(id, data)| format!(r#"{{"seriesID":"{id}","data":{data}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        serde_json::from_str(&format!(r#"{{"series":[{series}]}}"#)).unwrap()
    }

    #[test]
    fn interpret_response_covers_the_full_matrix() {
        // A 2xx REQUEST_SUCCEEDED body -> Ok, for the caller to read series from.
        let ok = r#"{"status":"REQUEST_SUCCEEDED","message":[],"Results":{"series":[]}}"#;
        assert!(interpret_response(200, ok).is_ok());

        // A 2xx REQUEST_NOT_PROCESSED (BLS reports errors at HTTP 200) -> fatal, with
        // the BLS message surfaced rather than masked as missing data.
        let not_processed = r#"{"status":"REQUEST_NOT_PROCESSED","message":["daily threshold for number of requests exceeded"],"Results":{}}"#;
        let err = interpret_response(200, not_processed).unwrap_err().to_string();
        assert!(err.contains("daily threshold"), "{err}");

        // A 2xx body that isn't valid JSON is a contract violation -> fatal.
        assert!(interpret_response(200, "not json at all").is_err());

        // Non-2xx transport/server statuses are fatal regardless of body.
        for status in [429, 500, 503] {
            assert!(
                interpret_response(status, "").is_err(),
                "HTTP {status} should be fatal"
            );
        }
    }

    #[test]
    fn series_to_quote_takes_latest_and_computes_change() {
        // Newest-first: latest 320.5, prior 319.0 -> +0.47% (off the prior value).
        let data = data_points(
            r#"[
                {"year":"2026","period":"M04","value":"320.5"},
                {"year":"2026","period":"M03","value":"319.0"}
            ]"#,
        );
        let q = series_to_quote(&data, "CUUR0000SA0", "CPI-U", "index (1982-84=100)")
            .unwrap()
            .expect("a quote");
        assert_eq!(q.symbol, "CUUR0000SA0");
        assert_eq!(q.name, "CPI-U");
        assert!((q.price - 320.5).abs() < 1e-9);
        assert!((q.change_pct - (1.5 / 319.0 * 100.0)).abs() < 1e-9);
        // The series' unit rides onto the quote from the table, labelling `price`.
        assert_eq!(q.unit, "index (1982-84=100)");
    }

    #[test]
    fn series_to_quote_empty_data_is_a_skip_not_an_error() {
        // A series with no observation contributes nothing, but is not an error — the
        // per-series absence the floor tolerates (a renamed/absent series id).
        let data = data_points("[]");
        assert!(series_to_quote(&data, "LNS14000000", "x", "percent")
            .unwrap()
            .is_none());
    }

    #[test]
    fn series_to_quote_single_value_has_no_change() {
        // One observation -> a quote with a 0.0 change (no prior to diff).
        let data = data_points(r#"[{"year":"2026","period":"M04","value":"4.1"}]"#);
        let q = series_to_quote(&data, "LNS14000000", "Unemployment Rate", "percent")
            .unwrap()
            .expect("a quote");
        assert!((q.price - 4.1).abs() < 1e-9);
        assert_eq!(q.change_pct, 0.0);
    }

    #[test]
    fn series_to_quote_rejects_nonnumeric_and_nonfinite_values() {
        // A present BLS value is expected numeric. A non-numeric value — or one that
        // parses to a non-finite float (NaN / inf, which f64::parse accepts) — fails
        // closed rather than being silently dropped (no FRED-style "." gap here).
        for bad in ["garbage", "NaN", "inf", "-inf", "infinity"] {
            let data = data_points(&format!(
                r#"[{{"year":"2026","period":"M04","value":"{bad}"}}]"#
            ));
            assert!(
                series_to_quote(&data, "CES0000000001", "x", "thousands of persons").is_err(),
                "value {bad:?} must fail closed, not skip"
            );
        }
    }

    #[test]
    fn assemble_labor_levels_requires_every_series_present() {
        let with_data = r#"[{"year":"2026","period":"M04","value":"1.0"}]"#;

        // All requested series present -> a quote each.
        let full: Vec<(&str, &str)> =
            LABOR_SERIES.iter().map(|(id, _, _)| (*id, with_data)).collect();
        assert_eq!(
            assemble_labor_levels(&results_with(&full)).unwrap().len(),
            LABOR_SERIES.len()
        );

        // A requested series omitted from the response -> fail closed. BLS returns
        // invalid series as explicit empty entries, never by omission, so an omission is
        // a truncated / anomalous response that must not silently thin the baseline.
        let omitted = &full[..full.len() - 1];
        let err = assemble_labor_levels(&results_with(omitted))
            .unwrap_err()
            .to_string();
        assert!(err.contains("omitted"), "{err}");
    }

    #[test]
    fn assemble_labor_levels_soft_skips_an_explicit_empty_series() {
        // A requested series present but with empty `data` is an explicit per-series
        // absence (the gold-lesson case) — skipped, not fatal; the others still resolve.
        let with_data = r#"[{"year":"2026","period":"M04","value":"1.0"}]"#;
        let mut entries: Vec<(&str, &str)> =
            LABOR_SERIES.iter().map(|(id, _, _)| (*id, with_data)).collect();
        entries[0].1 = "[]"; // first series returns no observations
        let quotes = assemble_labor_levels(&results_with(&entries)).unwrap();
        assert_eq!(quotes.len(), LABOR_SERIES.len() - 1);
    }

    #[test]
    fn check_completeness_requires_the_group_nonempty() {
        let q = Quote {
            symbol: "LNS14000000".into(),
            name: "Unemployment Rate".into(),
            price: 4.1,
            change_pct: 0.0,
            unit: "percent".into(),
        };
        assert!(check_completeness(&[q]).is_ok());
        let err = check_completeness(&[]).unwrap_err().to_string();
        assert!(err.contains("labor-levels"), "{err}");
    }

    #[test]
    fn year_window_spans_prior_and_current_year() {
        // Two-year window so January (one current-year month) still has a prior month
        // in range for the month-over-month change.
        assert_eq!(year_window(2026), ("2025".to_string(), "2026".to_string()));
    }

    #[test]
    #[ignore = "hits the live BLS API"]
    fn bls_baseline_smoke() {
        let src = BlsDataSource::new().expect("BLS client builds");
        let data = src.baseline_scan().expect("live baseline scan");

        // Print the resolved mapping so a maintainer can confirm each series came back
        // (run with `-- --ignored --nocapture`); the offline tests can only check
        // fixture shapes, not the live series — this is where a removed or renamed
        // series id surfaces (the lesson of the original FRED gold id).
        eprintln!("labor_levels ({}):", data.labor_levels.len());
        for q in &data.labor_levels {
            eprintln!(
                "  {:<16} {:<42} price={:<12} change_pct={:<10} unit={}",
                q.symbol, q.name, q.price, q.change_pct, q.unit
            );
        }

        // labor_levels is non-optional Step-6 baseline data. Assert it resolves in full
        // so a silently dropped (renamed / discontinued) series fails the smoke loudly
        // rather than thinning the baseline unnoticed — the per-symbol-assert discipline
        // the sibling smokes use.
        assert_eq!(
            data.labor_levels.len(),
            LABOR_SERIES.len(),
            "a labor series did not resolve"
        );
    }
}
