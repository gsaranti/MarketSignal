//! Real BLS (Bureau of Labor Statistics) adapter for the labor half of the
//! baseline market-data scan.
//!
//! The third data-source adapter behind the `MarketDataSource` trait
//! (`data_sources`), sibling to `fmp` and `fred`. It owns the Step-3 **labor levels**
//! group (`labor_levels`): the CPI-U headline index, the U-3 unemployment rate, total
//! nonfarm payrolls, and average hourly earnings (`docs/weekly-report-workflow.md
//! §Step 3`, `docs/data-sources.md §BLS`). These are point-in-time monthly levels
//! reusing the same quote shape as `fred`'s macro levels, kept in a group distinct
//! from the FRED `macro_levels` by source and concern. `price` is the latest reported
//! level; `change` is its month-over-month change from the prior reading.
//!
//! Unlike FMP and FRED, BLS is **keyless** (`docs/configuration.md` — BLS/GDELT need
//! no credential), so `BlsDataSource::new()` takes no key and BLS is not part of the
//! execution gate. The trade-off is the public v2 tier's 25-query/day cap; a single
//! batched request per scan keeps the burn at one query, so even frequent on-demand
//! runs sit comfortably under it.
//!
//! Like `fmp`/`fred`, the HTTP call is synchronous (`reqwest::blocking`) so the trait
//! stays sync; the blocking work is offloaded via `spawn_blocking` at the Tauri
//! command seam.
//!
//! Degradation policy. The same rule as the sibling adapters: **every failure degrades
//! to a recorded gap rather than failing the scan.** BLS is one batched POST, so a
//! request-level failure is all-or-nothing — every labor series degrades to a gap of the
//! same reason at once. BLS differs from FRED in *how* it signals: it answers HTTP 200
//! even for a rejected request, reporting the outcome in the JSON `status` field
//! (`REQUEST_SUCCEEDED` vs `REQUEST_NOT_PROCESSED`) with a human `message`. So
//! `interpret_response` classifies by that in-body status — a not-processed request (the
//! daily threshold or a malformed batch) is `Rejected`, a non-2xx `Unavailable`, an
//! unparseable body `Malformed`. Within a successful batch, a requested series returned
//! with an empty `data` array is an `Unavailable` per-series absence (no value this run —
//! BLS is keyless, so there's no permanent/premium tier to call it `OutOfScope`), and a
//! series *omitted* entirely is a `Malformed` gap (BLS returns invalid series as explicit
//! empty entries, so an omission signals a truncated / anomalous response). No floor lives
//! here — `labor_levels` isn't a coverage-floor group, so a total BLS outage degrades
//! the report (recorded in the manifest) rather than blocking it; the central coverage
//! gate (`pipeline::enforce_coverage`) owns the run's floor.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{anyhow, Context, Result};
use chrono::Datelike;
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{
    BaselineMarketData, Change, DataGap, GapReason, GroupKind, MarketDataSource, Quote,
};
use crate::progress::RunContext;

/// BLS Public Data API v2 time-series endpoint — a JSON POST batch of series ids.
/// Base URL for BLS's public timeseries API. The single endpoint path below is joined
/// onto it in [`BlsDataSource::post`]; a test redirects the adapter at a localhost mock
/// via [`BlsDataSource::with_base_url`], so the wire path runs offline.
const BLS_BASE: &str = "https://api.bls.gov/publicAPI/v2";
const BLS_DATA_PATH: &str = "/timeseries/data/";

/// Short timeout for the single batched request, matching the sibling adapters'
/// ceiling so a hung provider doesn't park the scan.
const BLS_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// The BLS-owned labor levels of the Step-3 baseline (`docs/weekly-report-workflow.md
/// §Step 3`, `docs/data-sources.md §BLS`): the CPI-U headline index (NSA, all items),
/// the U-3 unemployment rate, total nonfarm payroll employment, and average hourly
/// earnings for total private. Monthly series; `change` reads month-over-month.
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
/// - `Err((reason, detail))` — the whole batch failed: a non-2xx (`Unavailable`), an
///   unparseable body (`Malformed`), or a `REQUEST_NOT_PROCESSED` / unrecognized status
///   (`Rejected` — the daily threshold or a malformed batch). `detail` carries BLS's own
///   message for the runtime log; the structured `reason` is what the gap records.
///
/// Unlike FMP's status allowlist and FRED's by-body 400 split, BLS answers **HTTP 200
/// for errors too**, carrying the real outcome in the JSON `status`. So a not-processed
/// request degrades to a `Rejected` batch here rather than vanishing; a genuinely absent
/// *series* within a successful batch is handled downstream (empty `data`), not here.
fn interpret_response(http_status: u16, body: &str) -> Result<BlsResponse, (GapReason, String)> {
    if !(200..=299).contains(&http_status) {
        // BLS normally answers 200 even for rejected requests, so a non-2xx is a
        // transport/server fault: a this-run outage, like FRED's 429 / 5xx.
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
/// `change` is its percent change from the prior value (month-over-month for these
/// monthly series). Returns `Ok(None)` when the series has no observation (empty
/// `data`) — a per-series absence, not an error.
///
/// Fail-closed on the value: a present BLS observation is expected numeric, so any
/// non-numeric value — or one that parses to a non-finite float (`NaN` / `inf`, which
/// `f64::parse` accepts) — is a contract violation that `assemble_labor_levels` records
/// as a `Malformed` gap rather than dropping silently (which would let a stale reading
/// masquerade as current, or a `NaN` contaminate the change math). BLS signals "no datum" by omitting the
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
        change: Change::percent(change_pct),
        unit: unit.to_string(),
    }))
}

/// A `BaselineMarketData` whose `labor_levels` is empty and whose manifest records every
/// `LABOR_SERIES` entry as a gap of `reason` — the all-or-nothing degradation for a
/// batch-level BLS failure (a non-2xx, a rejected/over-limit request, an unparseable or
/// mis-shaped body). Labor isn't a coverage-floor group, so the run continues with this
/// recorded in the manifest.
fn all_labor_gapped(reason: GapReason) -> BaselineMarketData {
    BaselineMarketData {
        gaps: LABOR_SERIES
            .iter()
            .map(|(id, name, _)| DataGap::new(GroupKind::LaborLevels, *id, *name, reason))
            .collect(),
        ..Default::default()
    }
}

/// Match the parsed BLS results against the authoritative `LABOR_SERIES` list and shape
/// each into a quote, recording a `labor_levels` [`DataGap`] for any that don't resolve
/// rather than failing the scan. Split from the HTTP-bound `baseline_scan` so the
/// per-series policy below is unit-testable without a round-trip.
///
/// Two absence cases, tagged differently:
/// - A requested series **present but with empty `data`** (`series_to_quote` returns
///   `None`) is no value this run — an `Unavailable` gap that counts against coverage,
///   the same as FRED's all-gap window. (Not `OutOfScope`: BLS is keyless with no
///   premium tier, so an empty window is a this-run absence, not a permanent one.)
/// - A requested series **omitted from the response entirely** is not a signal BLS sends
///   in normal operation (it returns invalid series as explicit empty entries), so an
///   omission means a truncated / anomalous response — a `Malformed` gap. A non-numeric
///   observation is likewise `Malformed`.
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
            // Present but empty `data` for an expected series — no value this run, not a
            // permanent absence; counts against coverage (Unavailable).
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
    /// API origin the endpoint path is joined onto. Defaults to [`BLS_BASE`]; an offline
    /// round-trip test overrides it via [`BlsDataSource::with_base_url`].
    base_url: String,
    /// Run context for live progress + cooperative cancellation; a no-op by default
    /// (tests / smokes), the live one attached via [`BlsDataSource::with_context`].
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

    /// Redirect the adapter at an alternate API origin (a localhost mock) so the wire
    /// path runs offline. Test-only; a trailing slash is trimmed so the joined path's
    /// leading slash doesn't double up.
    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// Attach a live run context. BLS is a single batched request, so it streams one
    /// tracker row per labor series after the batch resolves (the per-series status
    /// comes from the assembled gaps), and honors a cancel observed before the POST.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// POST the full labor-series batch in one request, returning the status and raw
    /// body for `interpret_response` to judge. A transport error (the provider is
    /// unreachable) returns `Err` to `baseline_scan`, which degrades the whole labor
    /// group to gaps rather than failing the scan.
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
    fn baseline_scan(&self) -> Result<BaselineMarketData> {
        // Cancel before the single request: skip the POST and let the pipeline's
        // post-baseline checkpoint unwind the run.
        if self.progress.is_cancelled() {
            return Ok(BaselineMarketData::default());
        }
        // BLS is one batched POST for all labor series, so the tracker shows exactly one
        // request row (not one per series). Its status reflects the request outcome: `ok`
        // when the batch returned data, otherwise the batch-level failure reason. The
        // per-series gaps still ride into the data manifest for the agent.
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
    /// gap set. One batched POST, so a request-level failure (transport, non-2xx,
    /// rejected, or a mis-shaped body) degrades the whole labor group to gaps at once
    /// rather than failing the scan; the central coverage gate owns the run's floor, and
    /// labor isn't a floor group. So this returns a value for all data outcomes — the
    /// per-series tracker rows are layered on by `baseline_scan`.
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

        // Match the expected series, shaping each; omission / empty-data / non-numeric
        // each record a gap — see `assemble_labor_levels`. BLS fills only labor_levels;
        // the other groups are left empty for the composite to fill from FMP / FRED.
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

        // A 2xx REQUEST_NOT_PROCESSED (BLS reports errors at HTTP 200) -> a Rejected
        // batch, with the BLS message preserved in the detail for the runtime log.
        let not_processed = r#"{"status":"REQUEST_NOT_PROCESSED","message":["daily threshold for number of requests exceeded"],"Results":{}}"#;
        let (reason, detail) = interpret_response(200, not_processed).unwrap_err();
        assert_eq!(reason, GapReason::Rejected);
        assert!(detail.contains("daily threshold"), "{detail}");

        // A 2xx body that isn't valid JSON is a contract violation -> Malformed.
        let (reason, _) = interpret_response(200, "not json at all").unwrap_err();
        assert_eq!(reason, GapReason::Malformed);

        // Non-2xx transport/server statuses -> Unavailable regardless of body.
        for status in [429, 500, 503] {
            let (reason, _) = interpret_response(status, "").unwrap_err();
            assert_eq!(reason, GapReason::Unavailable, "HTTP {status}");
        }
    }

    // ---- Offline round trip: adapter -> retry -> interpret -> domain output ----
    //
    // The matrix above pins `interpret_response` as a pure function; these drive the
    // whole `post` -> `send_with_retry` -> `interpret_response` -> `assemble_labor_levels`
    // path against a localhost mock (`crate::test_http`). BLS is the first POST adapter,
    // so this also confirms the mock serves a POST round trip (the body sits unread in the
    // socket buffer; the reply still completes). One batched request, one canned reply, so
    // no `BASE_BACKOFF` sleep is incurred.

    fn test_source(base_url: &str) -> BlsDataSource {
        BlsDataSource::new()
            .expect("build adapter")
            .with_base_url(base_url)
    }

    #[test]
    fn gather_labor_round_trips_a_succeeded_batch_into_quotes() {
        // One datum per labor series -> a quote each, no gaps.
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
        // A non-2xx is a this-run outage: the whole labor group degrades to Unavailable
        // gaps, none of which fails the scan (labor isn't a floor group). A 404 is
        // non-retryable, so the round trip stays a single sleepless request (5xx/429
        // retry mechanics are covered in `http_retry`).
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
        // The series' unit rides onto the quote from the table, labelling `price`.
        assert_eq!(q.unit, "index (1982-84=100)");
    }

    #[test]
    fn series_to_quote_empty_data_is_a_skip_not_an_error() {
        // A series with no observation returns Ok(None) (a skip, not an error) — the
        // caller then records it as an Unavailable gap.
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
        assert_eq!(q.change.value, 0.0);
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
    fn assemble_labor_levels_records_a_gap_for_an_omitted_series() {
        let with_data = r#"[{"year":"2026","period":"M04","value":"1.0"}]"#;

        // All requested series present -> a quote each, no gaps.
        let full: Vec<(&str, &str)> = LABOR_SERIES
            .iter()
            .map(|(id, _, _)| (*id, with_data))
            .collect();
        let (quotes, gaps) = assemble_labor_levels(&results_with(&full));
        assert_eq!(quotes.len(), LABOR_SERIES.len());
        assert!(gaps.is_empty());

        // A requested series omitted from the response -> a Malformed gap, the rest
        // resolve. BLS returns invalid series as explicit empty entries, never by
        // omission, so an omission is a truncated / anomalous response.
        let omitted = &full[..full.len() - 1];
        let (quotes, gaps) = assemble_labor_levels(&results_with(omitted));
        assert_eq!(quotes.len(), LABOR_SERIES.len() - 1);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].reason, GapReason::Malformed);
        assert_eq!(gaps[0].series_id, LABOR_SERIES[LABOR_SERIES.len() - 1].0);
    }

    #[test]
    fn assemble_labor_levels_records_an_unavailable_gap_for_an_empty_series() {
        // A requested series present but with empty `data` is no value this run — an
        // Unavailable gap that counts against coverage; the others still resolve.
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
                "  {:<16} {:<42} price={:<12} change={:<10} unit={}",
                q.symbol, q.name, q.price, q.change.value, q.unit
            );
        }

        // labor_levels is non-optional Step-3 baseline data. Assert it resolves in full
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
