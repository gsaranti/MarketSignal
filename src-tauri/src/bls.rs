//! BLS (Bureau of Labor Statistics) adapter for the Step-3 baseline's `labor_levels`
//! group (`docs/report-workflow.md §Step 3`, `docs/data-sources.md §BLS`). Keyless, so
//! outside the execution gate; the public v2 tier caps at 25 queries/day, which one
//! batched request per scan keeps comfortably under. Every failure degrades to a
//! recorded gap, and `labor_levels` is not a coverage-floor group.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{anyhow, Context, Result};
use chrono::Datelike;
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{
    BaselineMarketData, Change, DataGap, GapReason, GroupKind, MarketDataSource, Quote,
};
use crate::cadence::ReportCadence;
use crate::progress::RunContext;

/// BLS Public Data API v2 origin; the time-series endpoint path is joined onto it.
const BLS_BASE: &str = "https://api.bls.gov/publicAPI/v2";
const BLS_DATA_PATH: &str = "/timeseries/data/";

/// Timeout for the single batched request, so a hung provider doesn't park the scan.
const BLS_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// The BLS-owned labor levels of the Step-3 baseline (`docs/report-workflow.md
/// §Step 3`, `docs/data-sources.md §BLS`): `(series_id, display name, unit)`, with the
/// BLS `series_id` doubling as the quote `symbol`. Monthly series; `change` reads
/// month-over-month. The `unit` labels `price` so the model can't make a 1000× misread
/// of the payroll level (thousands of persons) or confuse the CPI index level with a
/// percent.
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

/// BLS response envelope, trimmed to the fields the baseline needs. `results` stays a
/// raw `Value` because BLS shapes it as `{}` on a not-processed request and
/// `{"series":[...]}` on success; it is parsed into `BlsResults` only after `status`
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

/// One observation in a series. Only `value` is read: BLS returns `data` newest-first,
/// so the shaper takes the leading entries without re-sorting.
#[derive(Debug, Deserialize)]
struct BlsDataPoint {
    value: String,
}

/// Interpret one BLS batch response by HTTP status × in-body `status` — the single
/// place the request-level degradation policy lives. `Err((reason, detail))` means the
/// whole batch failed; `detail` carries BLS's own message for the runtime log, the
/// structured `reason` is what the gap records. BLS answers HTTP 200 for errors too,
/// carrying the real outcome in the JSON `status`, so a `REQUEST_NOT_PROCESSED` (the
/// daily threshold or a malformed batch) degrades to `Rejected` here rather than
/// vanishing; an absent series within a successful batch is handled downstream.
fn interpret_response(http_status: u16, body: &str) -> Result<BlsResponse, (GapReason, String)> {
    if !(200..=299).contains(&http_status) {
        // BLS answers 200 even for rejected requests, so a non-2xx is a transport/server
        // fault.
        return Err((GapReason::Unavailable, format!("HTTP {http_status}")));
    }
    let resp: BlsResponse = match serde_json::from_str(body) {
        Ok(resp) => resp,
        Err(_) => return Err((GapReason::Malformed, "unparseable body".to_string())),
    };
    match resp.status.as_str() {
        "REQUEST_SUCCEEDED" => Ok(resp),
        other => Err((
            GapReason::Rejected,
            format!("{other}: {}", resp.message.join("; ")),
        )),
    }
}

/// Shape one series' observations into a quote: the most recent value is `price`, and
/// `change` is its percent change from the prior value. `Ok(None)` when the series has
/// no observation — a per-series absence, not an error. Fail-closed on the value: a
/// non-numeric observation, or one parsing to a non-finite float (`NaN` / `inf`, which
/// `f64::parse` accepts), is a contract violation recorded as a `Malformed` gap rather
/// than dropped silently, which would let a stale reading masquerade as current. BLS
/// signals "no datum" by omitting the observation, so there is no FRED-style `"."` to
/// skip.
fn series_to_quote(
    data: &[BlsDataPoint],
    symbol: &str,
    name: &str,
    unit: &str,
) -> Result<Option<Quote>> {
    // Latest + prior is all the change needs, so stop at two.
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
        change: Change::percent(change_pct),
        unit: unit.to_string(),
    }))
}

/// A `BaselineMarketData` whose manifest records every `LABOR_SERIES` entry as a gap of
/// `reason` — the all-or-nothing degradation for a batch-level BLS failure. Labor isn't
/// a coverage-floor group, so the run continues.
fn all_labor_gapped(reason: GapReason) -> BaselineMarketData {
    BaselineMarketData {
        gaps: LABOR_SERIES
            .iter()
            .map(|(id, name, _)| DataGap::new(GroupKind::LaborLevels, *id, *name, reason))
            .collect(),
        ..Default::default()
    }
}

/// Match the parsed BLS results against `LABOR_SERIES` and shape each into a quote,
/// recording a `labor_levels` [`DataGap`] for any that don't resolve. A requested series
/// present with empty `data` is an `Unavailable` gap (no value this run; BLS is keyless
/// with no premium tier, so never `OutOfScope`). A series omitted from the response
/// entirely is a `Malformed` gap: BLS returns invalid series as explicit empty entries,
/// so an omission means a truncated / anomalous response. A non-numeric observation is
/// likewise `Malformed`.
fn assemble_labor_levels(results: &BlsResults) -> (Vec<Quote>, Vec<DataGap>) {
    let mut labor_levels = Vec::with_capacity(LABOR_SERIES.len());
    let mut gaps = Vec::new();
    for (id, name, unit) in LABOR_SERIES {
        let Some(series) = results.series.iter().find(|s| s.series_id == *id) else {
            gaps.push(DataGap::new(
                GroupKind::LaborLevels,
                *id,
                *name,
                GapReason::Malformed,
            ));
            continue;
        };
        match series_to_quote(&series.data, id, name, unit) {
            Ok(Some(quote)) => labor_levels.push(quote),
            Ok(None) => gaps.push(DataGap::new(
                GroupKind::LaborLevels,
                *id,
                *name,
                GapReason::Unavailable,
            )),
            Err(_) => gaps.push(DataGap::new(
                GroupKind::LaborLevels,
                *id,
                *name,
                GapReason::Malformed,
            )),
        }
    }
    (labor_levels, gaps)
}

/// The `(startyear, endyear)` request window: the prior and current calendar year, so
/// at least two monthly observations are in range even in January and the
/// month-over-month change resolves.
fn year_window(current_year: i32) -> (String, String) {
    ((current_year - 1).to_string(), current_year.to_string())
}

/// Live BLS adapter behind the `MarketDataSource` trait.
pub struct BlsDataSource {
    http: reqwest::blocking::Client,
    base_url: String,
    progress: Arc<RunContext>,
}

impl BlsDataSource {
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(BLS_TIMEOUT)
            .build()
            .context("building the BLS HTTP client")?;
        Ok(Self {
            http,
            base_url: BLS_BASE.to_string(),
            progress: RunContext::noop(),
        })
    }

    /// Point the adapter at a localhost mock so the wire path runs offline; a trailing
    /// slash is trimmed so the joined path's leading slash doesn't double up.
    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// Attach a live run context so the batch streams a tracker row and honors a cancel
    /// observed before the POST.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// POST the full labor-series batch in one request. A transport error returns `Err`,
    /// which `gather_labor` degrades to a full gap set.
    fn post(&self) -> Result<(u16, String)> {
        let ids: Vec<&str> = LABOR_SERIES.iter().map(|(id, _, _)| *id).collect();
        let (start, end) = year_window(chrono::Local::now().year());
        let payload = serde_json::json!({
            "seriesid": ids,
            "startyear": start,
            "endyear": end,
        });
        let url = format!("{}{BLS_DATA_PATH}", self.base_url);
        crate::http_retry::send_with_retry("BLS", || self.http.post(&url).json(&payload))
    }
}

impl MarketDataSource for BlsDataSource {
    fn baseline_scan(&self, _cadence: ReportCadence) -> Result<BaselineMarketData> {
        // Cancel before the single request: skip the POST and let the pipeline's
        // post-baseline checkpoint unwind the run.
        if self.progress.is_cancelled() {
            return Ok(BaselineMarketData::default());
        }
        // One batched POST, so the tracker shows exactly one request row; its status is
        // `ok` when the batch returned data, otherwise the batch-level failure reason.
        let group = GroupKind::LaborLevels.as_str();
        let id = "labor-batch";
        let name = "Labor series (CPI, unemployment, payrolls, wages)";
        self.progress.request_started("BLS", group, id, name);
        let data = self.gather_labor();
        let status = if !data.labor_levels.is_empty() {
            "ok"
        } else {
            data.gaps
                .first()
                .map(|g| g.reason.as_str())
                .unwrap_or("empty")
        };
        self.progress
            .request_finished("BLS", group, id, name, status, None);
        Ok(data)
    }
}

impl BlsDataSource {
    /// Gather the labor group, degrading any request- or shape-level failure to a full
    /// gap set rather than failing the scan; labor isn't a coverage-floor group.
    fn gather_labor(&self) -> BaselineMarketData {
        let outcome = match self.post() {
            Ok((status, body)) => interpret_response(status, &body),
            Err(_) => Err((GapReason::Unavailable, "transport error".to_string())),
        };
        let resp = match outcome {
            Ok(resp) => resp,
            Err((reason, detail)) => {
                eprintln!(
                    "BLS labor batch unavailable ({detail}); recording all labor series as gaps"
                );
                return all_labor_gapped(reason);
            }
        };
        let results: BlsResults = match serde_json::from_value(resp.results) {
            Ok(results) => results,
            Err(_) => {
                eprintln!("BLS Results did not match the expected shape; recording all labor series as gaps");
                return all_labor_gapped(GapReason::Malformed);
            }
        };

        // BLS fills only labor_levels; the composite fills the other groups from FMP / FRED.
        let (labor_levels, gaps) = assemble_labor_levels(&results);
        BaselineMarketData {
            labor_levels,
            gaps,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    fn data_points(json: &str) -> Vec<BlsDataPoint> {
        serde_json::from_str(json).unwrap()
    }

    /// `(series_id, data-array JSON)` pairs shaped as the live `Results` object.
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
        let ok = r#"{"status":"REQUEST_SUCCEEDED","message":[],"Results":{"series":[]}}"#;
        assert!(interpret_response(200, ok).is_ok());

        // BLS reports errors at HTTP 200; its message rides into the detail.
        let not_processed = r#"{"status":"REQUEST_NOT_PROCESSED","message":["daily threshold for number of requests exceeded"],"Results":{}}"#;
        let (reason, detail) = interpret_response(200, not_processed).unwrap_err();
        assert_eq!(reason, GapReason::Rejected);
        assert!(detail.contains("daily threshold"), "{detail}");

        let (reason, _) = interpret_response(200, "not json at all").unwrap_err();
        assert_eq!(reason, GapReason::Malformed);

        for status in [429, 500, 503] {
            let (reason, _) = interpret_response(status, "").unwrap_err();
            assert_eq!(reason, GapReason::Unavailable, "HTTP {status}");
        }
    }

    // ---- Offline round trip: adapter -> retry -> interpret -> domain output ----
    //
    // The mock serves a POST round trip with the body unread in the socket buffer; one
    // canned reply means no `BASE_BACKOFF` sleep is incurred.

    fn test_source(base_url: &str) -> BlsDataSource {
        BlsDataSource::new()
            .expect("build adapter")
            .with_base_url(base_url)
    }

    #[test]
    fn gather_labor_round_trips_a_succeeded_batch_into_quotes() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"status":"REQUEST_SUCCEEDED","message":[],"Results":{"series":[
                {"seriesID":"CUUR0000SA0","data":[{"value":"320.5"}]},
                {"seriesID":"LNS14000000","data":[{"value":"4.1"}]},
                {"seriesID":"CES0000000001","data":[{"value":"159000"}]},
                {"seriesID":"CES0500000003","data":[{"value":"35.5"}]}
            ]}}"#,
        }]);
        let source = test_source(&server.base_url);
        let data = source.gather_labor();
        assert_eq!(server.attempts(), 1, "BLS is one batched POST");
        assert_eq!(server.request_paths(), ["/timeseries/data/"]);
        assert!(data.gaps.is_empty(), "every series resolved");
        assert_eq!(data.labor_levels.len(), LABOR_SERIES.len());
    }

    #[test]
    fn gather_labor_round_trips_a_non_2xx_into_a_full_gap_set() {
        // A 404 is non-retryable, so the round trip stays a single sleepless request.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 404,
            headers: vec![],
            body: "not found",
        }]);
        let source = test_source(&server.base_url);
        let data = source.gather_labor();
        assert_eq!(server.attempts(), 1);
        assert_eq!(server.request_paths(), ["/timeseries/data/"]);
        assert!(data.labor_levels.is_empty());
        assert_eq!(data.gaps.len(), LABOR_SERIES.len());
        assert!(data.gaps.iter().all(|g| g.reason == GapReason::Unavailable));
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
        assert!((q.change.value - (1.5 / 319.0 * 100.0)).abs() < 1e-9);
        assert_eq!(q.unit, "index (1982-84=100)");
    }

    #[test]
    fn series_to_quote_empty_data_is_a_skip_not_an_error() {
        let data = data_points("[]");
        assert!(series_to_quote(&data, "LNS14000000", "x", "percent")
            .unwrap()
            .is_none());
    }

    #[test]
    fn series_to_quote_single_value_has_no_change() {
        let data = data_points(r#"[{"year":"2026","period":"M04","value":"4.1"}]"#);
        let q = series_to_quote(&data, "LNS14000000", "Unemployment Rate", "percent")
            .unwrap()
            .expect("a quote");
        assert!((q.price - 4.1).abs() < 1e-9);
        assert_eq!(q.change.value, 0.0);
    }

    #[test]
    fn series_to_quote_rejects_nonnumeric_and_nonfinite_values() {
        // NaN / inf parse as floats, so non-finite values must fail closed too.
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
    fn assemble_labor_levels_records_a_gap_for_an_omitted_series() {
        let with_data = r#"[{"year":"2026","period":"M04","value":"1.0"}]"#;

        let full: Vec<(&str, &str)> = LABOR_SERIES
            .iter()
            .map(|(id, _, _)| (*id, with_data))
            .collect();
        let (quotes, gaps) = assemble_labor_levels(&results_with(&full));
        assert_eq!(quotes.len(), LABOR_SERIES.len());
        assert!(gaps.is_empty());

        // BLS returns invalid series as explicit empty entries, never by omission, so an
        // omission is a truncated / anomalous response.
        let omitted = &full[..full.len() - 1];
        let (quotes, gaps) = assemble_labor_levels(&results_with(omitted));
        assert_eq!(quotes.len(), LABOR_SERIES.len() - 1);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].reason, GapReason::Malformed);
        assert_eq!(gaps[0].series_id, LABOR_SERIES[LABOR_SERIES.len() - 1].0);
    }

    #[test]
    fn assemble_labor_levels_records_an_unavailable_gap_for_an_empty_series() {
        let with_data = r#"[{"year":"2026","period":"M04","value":"1.0"}]"#;
        let mut entries: Vec<(&str, &str)> = LABOR_SERIES
            .iter()
            .map(|(id, _, _)| (*id, with_data))
            .collect();
        entries[0].1 = "[]"; // first series returns no observations
        let (quotes, gaps) = assemble_labor_levels(&results_with(&entries));
        assert_eq!(quotes.len(), LABOR_SERIES.len() - 1);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].reason, GapReason::Unavailable);
        assert_eq!(gaps[0].series_id, LABOR_SERIES[0].0);
    }

    #[test]
    fn year_window_spans_prior_and_current_year() {
        assert_eq!(year_window(2026), ("2025".to_string(), "2026".to_string()));
    }

    #[test]
    #[ignore = "hits the live BLS API"]
    fn bls_baseline_smoke() {
        let src = BlsDataSource::new().expect("BLS client builds");
        let data = src.baseline_scan(ReportCadence::default()).expect("live baseline scan");

        // Print the resolved mapping (run with `-- --ignored --nocapture`); this is where
        // a removed or renamed series id surfaces.
        eprintln!("labor_levels ({}):", data.labor_levels.len());
        for q in &data.labor_levels {
            eprintln!(
                "  {:<16} {:<42} price={:<12} change={:<10} unit={}",
                q.symbol, q.name, q.price, q.change.value, q.unit
            );
        }

        // Assert it resolves in full so a renamed / discontinued series fails the smoke
        // loudly rather than thinning the baseline unnoticed.
        assert_eq!(
            data.labor_levels.len(),
            LABOR_SERIES.len(),
            "a labor series did not resolve"
        );
    }
}
