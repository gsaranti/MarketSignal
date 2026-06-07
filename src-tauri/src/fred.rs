//! Real FRED (Federal Reserve Economic Data) adapter for the macro / commodity
//! half of the baseline market-data scan.
//!
//! The second data-source adapter behind the `MarketDataSource` trait
//! (`data_sources`), the sibling of `fmp`. It owns the Step-6 market internals FMP
//! does not serve on its free tier (verified live: HTTP 402 "premium"): the
//! **2Y / 10Y Treasury yields**, the **US dollar index**, and the **oil / natural
//! gas** commodity prices (`docs/weekly-report-workflow.md §Step 6`,
//! `docs/data-sources.md §FRED`). Each is a canonical free FRED daily series; the
//! results are appended to the baseline's `internals` group alongside FMP's VIX and
//! gold by the composite source. (Gold is served free on FMP via `GCUSD` and stays
//! there — FRED's former gold benchmark series were removed, so this adapter owns no
//! gold series.)
//!
//! It also owns the Step-6 **macro levels** group (`macro_levels`): the Fed-funds
//! target range as the policy-stance proxy (futures-implied expectations aren't on
//! FRED's free tier, and no other data source supplies them free), the 5y / 10y
//! inflation breakevens, U. Michigan consumer sentiment, and the PCE price index.
//! These are point-in-time levels reusing the same observations machinery, kept in a
//! group distinct from the market internals. Daily series (target range, breakevens)
//! report a day-over-day `change_pct`; monthly series (sentiment, PCE), month-over-
//! month.
//!
//! It additionally owns the Step-6 **economic-release calendar** (`calendar`): the
//! prior-week and upcoming US economic reports (CPI, PCE, jobs, GDP, …), built
//! from FRED's free *release-dates* schedule rather than the observations endpoint.
//! FMP's economic-calendar endpoint is premium-gated (verified live: HTTP 402), so the
//! schedule comes from FRED; the actual figures reach the model through the series
//! groups above, not the calendar. Unlike the series groups the calendar has no
//! completeness floor — an empty window is valid.
//!
//! Like `fmp`, the HTTP call is synchronous (`reqwest::blocking`) so the trait
//! stays sync; the blocking work is offloaded via `spawn_blocking` at the Tauri
//! command seam. The key rides as a query param (`api_key`), FRED's required
//! per-request credential (`docs/configuration.md` — FRED, unlike BLS/GDELT, is
//! not keyless).
//!
//! Degradation policy. The same rule as `fmp`: **every failure degrades to a recorded
//! gap, so one flaky series or a whole-provider outage never throws away the rest of the
//! scan.** One pure function, `interpret_response`, classifies each response into a
//! [`Disposition`] — a 2xx value to shape, or a `Gap(reason)`. FRED differs from FMP in
//! *how* it signals: a rejected key and a missing series are **both HTTP 400**,
//! distinguished only by the JSON `error_message`, so this classifies 400 by body (series
//! "does not exist" → `OutOfScope`; an `api_key` problem → `Rejected`; any other 400 →
//! `Malformed`) rather than by a status allowlist. A 429 / 5xx is `Unavailable`; a 2xx
//! whose observations are all FRED's `"."` gap marker is also `Unavailable` (no value
//! this run, not a permanent absence — only an explicit "does not exist" is `OutOfScope`).
//! No floor lives here — resolving no series leaves the groups empty plus their
//! gaps, and the central coverage gate (`pipeline::enforce_coverage`) decides the run's
//! floor.

use std::time::Duration as StdDuration;

use anyhow::{anyhow, Context, Result};
use chrono::{Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{
    BaselineMarketData, DataGap, EconomicRelease, GapReason, GroupKind, MarketDataSource, Quote,
};

/// FRED's observations endpoint — the series time-series the baseline reads.
const FRED_OBSERVATIONS_URL: &str = "https://api.stlouisfed.org/fred/series/observations";

/// Short timeout per request: the baseline scan issues several sequential calls,
/// none of which should park for the model adapter's 120s ceiling. Mirrors `fmp`.
const FRED_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// How many of the most recent observations to request per series. Newest-first,
/// enough to find the two most recent *numeric* values across FRED's `"."` gaps
/// (weekends / holidays / unpublished days) so the day-over-day change resolves.
const OBSERVATION_LIMIT: &str = "10";

/// The FRED-owned market internals of the Step-6 baseline (`docs/weekly-report
/// -workflow.md §Step 6`), paired with a display name and the `price` unit. Each is a
/// free FRED daily series; the FRED `series_id` doubles as the quote `symbol`. Yields
/// and the breakevens are quoted in percent; the dollar index is an index level; oil
/// and gas are dollar prices — the unit labels which, so the model doesn't read a yield
/// as a dollar figure.
///
/// The credit spreads (high-yield and investment-grade OAS) and the 10y−3m / 10y−2y
/// Treasury curve spreads join here too: daily, market-priced risk gauges feeding the
/// report's risk-posture and market-cycle reads. For these the **level** is the signal
/// — `change_pct` keeps the same percent-of-prior convention as every other series,
/// which is low-signal for a spread that can sit near zero or invert, so downstream
/// reasoning should lean on the level, not its percent move.
const INTERNALS_SERIES: &[(&str, &str, &str)] = &[
    ("DGS2", "2-Year Treasury Yield", "percent"),
    ("DGS10", "10-Year Treasury Yield", "percent"),
    ("DTWEXBGS", "US Dollar Index (Broad)", "index (Jan 2006=100)"),
    ("DCOILWTICO", "WTI Crude Oil", "USD per barrel"),
    ("DHHNGSP", "Henry Hub Natural Gas", "USD per million BTU"),
    // Credit + curve spreads (daily, market-priced) — the level is the signal.
    ("BAMLH0A0HYM2", "US High-Yield Corporate OAS", "percent"),
    ("BAMLC0A0CM", "US Investment-Grade Corporate OAS", "percent"),
    ("T10Y3M", "10-Year minus 3-Month Treasury Spread", "percent"),
    ("T10Y2Y", "10-Year minus 2-Year Treasury Spread", "percent"),
];

/// The FRED-owned macro levels of the Step-6 baseline (`docs/weekly-report
/// -workflow.md §Step 6`, the "Macro" group): the Fed-funds target range as the
/// policy-stance proxy (futures-implied expectations aren't on FRED's free tier), the
/// 5y / 10y inflation breakevens, U. Michigan consumer sentiment, the PCE price index,
/// and the headline activity reports — PPI, retail sales, job openings (JOLTS), and real
/// GDP — that supply the **actual readings** for the economic-release `calendar`'s
/// prior-week entries (so the report sees what each release printed, not just that it
/// landed). Mixed daily (target range, breakevens), monthly (sentiment, PCE, PPI, retail,
/// JOLTS) and quarterly (GDP) series; `change_pct` reads the change off the prior
/// observation accordingly. Same `(series_id, display name, unit)` shape as the internals
/// — the `series_id` doubles as the quote `symbol`, and the unit labels what each `price`
/// level is quoted in (percent, an index level with its base period, a dollar figure, or
/// a count).
///
/// The weekly risk/cycle gauges added alongside join this group too: the financial-
/// conditions composites (NFCI, the adjusted ANFCI, and the St. Louis stress index
/// STLFSI4), the weekly jobless-claims series (initial and continued), the Fed balance
/// sheet (WALCL), and the 30-year mortgage rate — each read the same way (latest level
/// + change off the prior observation).
const MACRO_SERIES: &[(&str, &str, &str)] = &[
    ("DFEDTARU", "Fed Funds Target Range — Upper Limit", "percent"),
    ("DFEDTARL", "Fed Funds Target Range — Lower Limit", "percent"),
    ("T5YIE", "5-Year Breakeven Inflation Rate", "percent"),
    ("T10YIE", "10-Year Breakeven Inflation Rate", "percent"),
    ("UMCSENT", "U. Michigan Consumer Sentiment", "index (1966Q1=100)"),
    ("PCEPI", "PCE Price Index", "index (2017=100)"),
    ("PPIFIS", "Producer Price Index (Final Demand)", "index (Nov 2009=100)"),
    (
        "RSAFS",
        "Advance Retail Sales (Retail & Food Services)",
        "millions of USD",
    ),
    ("JTSJOL", "Job Openings: Total Nonfarm (JOLTS)", "thousands of openings"),
    ("GDPC1", "Real Gross Domestic Product", "billions of chained 2017 USD"),
    // Weekly/daily risk + cycle gauges (financial conditions, claims, liquidity, housing).
    ("NFCI", "Chicago Fed National Financial Conditions Index", "index (0 = average)"),
    ("ANFCI", "Chicago Fed Adjusted NFCI", "index (0 = average)"),
    ("STLFSI4", "St. Louis Fed Financial Stress Index", "index (0 = normal)"),
    ("ICSA", "Initial Jobless Claims", "persons"),
    ("CCSA", "Continued Jobless Claims (Insured Unemployment)", "persons"),
    ("WALCL", "Fed Total Assets (Balance Sheet)", "millions of USD"),
    ("MORTGAGE30US", "30-Year Fixed Mortgage Rate", "percent"),
];

/// FRED's release-dates endpoint — the economic-release *schedule* the Step-6 calendar
/// reads, distinct from the series observations above. `include_release_dates_with_no_data`
/// surfaces upcoming (not-yet-released) dates; the realtime window bounds the dates to
/// the calendar span.
const FRED_RELEASE_DATES_URL: &str = "https://api.stlouisfed.org/fred/release/dates";

/// The economic-release calendar window: days back (prior-week reports already released)
/// and forward (the upcoming schedule) of today to keep. Applied both server-side (the
/// `realtime_start` / `realtime_end` query params) and again in `releases_to_calendar`.
const CALENDAR_BACK_DAYS: i64 = 10;
const CALENDAR_FWD_DAYS: i64 = 21;

/// The curated market-moving US economic releases of the Step-6 calendar, as
/// `(FRED release_id, display name)`. The ids are pinned against FRED's `releases`
/// catalog, and each is verified live by `fred_baseline_smoke` two ways — by **name**
/// against FRED's `releases` catalog (a wrong-but-valid id points at a different release,
/// which `release/dates` can't catch since it just echoes the queried id) and by a
/// **wide-window per-id probe** that it resolves to real dates (catching a retired id) —
/// the per-symbol-resolution discipline the series groups use, so a wrong or retired id
/// fails the smoke rather than silently thinning the calendar. FOMC is deliberately
/// excluded: FRED has no
/// scheduled-date calendar for the "FOMC Press Release" release, so requesting its
/// upcoming dates fabricates one row per day (live-verified); the Fed's policy stance is
/// instead carried by the Fed-funds target-range series in `macro_levels`.
const RELEASES: &[(u32, &str)] = &[
    (50, "Employment Situation"),
    (10, "Consumer Price Index"),
    (46, "Producer Price Index"),
    (54, "Personal Income and Outlays"),
    (9, "Advance Monthly Sales for Retail and Food Services"),
    (192, "Job Openings and Labor Turnover Survey"),
    (53, "Gross Domestic Product"),
];

/// FRED's observations response, trimmed to the one field the baseline needs. Each
/// observation's `value` is a **string** — a number like `"4.30"` or FRED's `"."`
/// gap marker for a day with no datum — so it is parsed (and `"."` skipped) when
/// shaping the quote, never deserialized as `f64` directly.
#[derive(Debug, Deserialize)]
struct FredObservations {
    observations: Vec<FredObservation>,
}

#[derive(Debug, Deserialize)]
struct FredObservation {
    value: String,
}

/// FRED's release-dates response, trimmed to the one field the calendar needs. Each
/// entry's `date` is the scheduled / actual release date (`"YYYY-MM-DD"`); the scoped
/// endpoint omits the release name, so the name rides from the `RELEASES` table.
#[derive(Debug, Deserialize)]
struct FredReleaseDates {
    release_dates: Vec<FredReleaseDate>,
}

#[derive(Debug, Deserialize)]
struct FredReleaseDate {
    date: String,
}

/// One FRED response classified into what the loop should do with it — the single place
/// the degradation policy lives, in terms of [`GapReason`] rather than a fatal `Err`.
enum Disposition {
    Value(Value),
    Gap(GapReason),
}

/// Interpret one FRED response by status × body. Pure and total. Unlike FMP's status
/// allowlist, FRED returns **400 for both** a rejected key and a missing series, so the
/// body's `error_message` disambiguates them: an explicit "does not exist" is an
/// `OutOfScope` per-series absence; an `api_key` problem is `Rejected`; any other 400 is
/// `Malformed` (fail-closed — a broken request degrades to a recorded gap, not a silent
/// skip). A 429 / 5xx is `Unavailable`; an unparseable 2xx body is `Malformed`.
fn interpret_response(status: u16, body: &str) -> Disposition {
    match status {
        200..=299 => match serde_json::from_str::<Value>(body) {
            Ok(value) => Disposition::Value(value),
            Err(_) => Disposition::Gap(GapReason::Malformed),
        },
        400 | 404 => {
            let msg = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|v| {
                    v.get("error_message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let lower = msg.to_ascii_lowercase();
            if lower.contains("does not exist") {
                Disposition::Gap(GapReason::OutOfScope) // explicit series absence
            } else if lower.contains("api_key") || lower.contains("api key") {
                Disposition::Gap(GapReason::Rejected) // key rejected
            } else {
                Disposition::Gap(GapReason::Malformed) // unrecognized 400 — fail closed
            }
        }
        429 | 500..=599 => Disposition::Gap(GapReason::Unavailable),
        _ => Disposition::Gap(GapReason::Malformed),
    }
}

/// Shape a successful observations response into one quote: the most recent numeric
/// observation is `price`, and `change_pct` is its percent change from the prior
/// numeric observation (day-over-day, consistent with FMP's quote change). Returns
/// `Ok(None)` when the series has no numeric observation in the window (all gaps) —
/// a per-series absence, not an error.
///
/// Fail-closed on the value: FRED's documented `"."` is the **only** skippable
/// marker. Any other non-numeric value — or one that parses to a non-finite float
/// (`NaN` / `inf`, which `f64::parse` accepts) — is a contract violation that fails
/// the scan rather than being silently dropped as a gap (which would let a stale
/// observation masquerade as current, or a `NaN` contaminate the change math). A
/// body that is not the expected observations shape is likewise an error.
fn observations_to_quote(
    value: Value,
    symbol: &str,
    name: &str,
    unit: &str,
) -> Result<Option<Quote>> {
    let raw: FredObservations = serde_json::from_value(value)
        .context("FRED observations response did not match the expected shape")?;
    // The most-recent numeric observations, newest-first; latest + prior is all the
    // change needs, so stop at two.
    let mut numeric: Vec<f64> = Vec::with_capacity(2);
    for obs in &raw.observations {
        let v = obs.value.trim();
        if v == "." {
            continue; // documented "no datum for this date" gap — skip
        }
        let parsed: f64 = v
            .parse()
            .ok()
            .filter(|n: &f64| n.is_finite())
            .ok_or_else(|| {
                anyhow!("FRED returned a non-numeric observation value {v:?} for series {symbol}")
            })?;
        numeric.push(parsed);
        if numeric.len() == 2 {
            break;
        }
    }
    let Some(&latest) = numeric.first() else {
        return Ok(None); // every observation was a "." gap — no recent datum
    };
    // Percent change off the prior numeric observation; a zero (or absent) prior
    // yields no change rather than a division by zero / spurious move.
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

/// Shape a FRED release-dates response into calendar entries for one release, keeping
/// only dates within the `[today − CALENDAR_BACK_DAYS, today + CALENDAR_FWD_DAYS]`
/// window and classifying each as `"released"` (before today) or `"upcoming"` (today or
/// later). The release `name` rides from the `RELEASES` table (the scoped endpoint omits
/// it). The window is also enforced server-side by the query's realtime params; the
/// re-check here keeps the function self-contained and testable. An unparseable date is a
/// contract violation that `fetch_calendar` records as a `Malformed` gap rather than
/// dropping silently.
fn releases_to_calendar(
    value: Value,
    name: &str,
    today: NaiveDate,
) -> Result<Vec<EconomicRelease>> {
    let raw: FredReleaseDates = serde_json::from_value(value)
        .context("FRED release/dates response did not match the expected shape")?;
    let earliest = today - Duration::days(CALENDAR_BACK_DAYS);
    let latest = today + Duration::days(CALENDAR_FWD_DAYS);
    let mut out = Vec::with_capacity(raw.release_dates.len());
    for rd in raw.release_dates {
        let date_str = rd.date.trim();
        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").with_context(|| {
            format!("FRED returned an unparseable release date {date_str:?} for release {name}")
        })?;
        if date < earliest || date > latest {
            continue; // outside the window (defensive — the query already bounds it)
        }
        let status = if date < today { "released" } else { "upcoming" };
        out.push(EconomicRelease {
            release: name.to_string(),
            date: date_str.to_string(),
            status: status.to_string(),
            expected: None,
        });
    }
    Ok(out)
}

/// Live FRED adapter behind the `MarketDataSource` trait.
pub struct FredDataSource {
    api_key: String,
    http: reqwest::blocking::Client,
}

impl FredDataSource {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(FRED_TIMEOUT)
            .build()
            .context("building the FRED HTTP client")?;
        Ok(Self { api_key, http })
    }

    /// Resolve the adapter from the environment, for the live smoke and any caller
    /// that bypasses the gate. The execution gate (`config::validate`) runs ahead
    /// of this in the command path.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().fred_key()?)
    }

    /// GET one series' most recent observations (newest-first), returning the
    /// status and raw body for `interpret_response` to judge. A transport error
    /// (the provider is unreachable) returns `Err` to the caller, which records it as an
    /// `Unavailable` gap rather than failing the scan.
    fn get(&self, series_id: &str) -> Result<(u16, String)> {
        crate::http_retry::send_with_retry("FRED", || {
            self.http.get(FRED_OBSERVATIONS_URL).query(&[
                ("series_id", series_id),
                ("api_key", self.api_key.as_str()),
                ("file_type", "json"),
                ("sort_order", "desc"),
                ("limit", OBSERVATION_LIMIT),
            ])
        })
    }

    /// Fetch one quote per FRED series in `series`, recording a [`DataGap`] in `group`
    /// for any that don't resolve rather than failing the scan. A "does not exist" 400 is
    /// an `OutOfScope` gap; an all-gap window is `Unavailable` (no value this run, so it
    /// counts against coverage); an `api_key` rejection is `Rejected`
    /// and — being a whole-provider condition — stops the loop, recording the rest
    /// without hammering; a systemic / unrecognized response or a body that won't shape
    /// is `Unavailable` / `Malformed`. Shared by the internals and macro-levels groups,
    /// which differ only in their series list and `group` tag.
    fn fetch_series(
        &self,
        series: &[(&str, &str, &str)],
        group: GroupKind,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<Quote> {
        let mut out = Vec::with_capacity(series.len());
        let mut rejected = false;
        for (series_id, name, unit) in series {
            if rejected {
                gaps.push(DataGap::new(group, *series_id, *name, GapReason::Rejected));
                continue;
            }
            let disposition = match self.get(series_id) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable), // transport — unreachable
            };
            match disposition {
                Disposition::Value(value) => match observations_to_quote(value, series_id, name, unit) {
                    Ok(Some(quote)) => out.push(quote),
                    // Every observation in the window was a "." gap — no value published
                    // this run, not a permanent/premium absence, so it counts against
                    // coverage (Unavailable), unlike an explicit "does not exist".
                    Ok(None) => gaps.push(DataGap::new(group, *series_id, *name, GapReason::Unavailable)),
                    Err(_) => gaps.push(DataGap::new(group, *series_id, *name, GapReason::Malformed)),
                },
                Disposition::Gap(reason) => {
                    if reason == GapReason::Rejected {
                        rejected = true;
                    }
                    gaps.push(DataGap::new(group, *series_id, *name, reason));
                }
            }
        }
        out
    }

    /// GET one release's scheduled dates within the calendar window, returning the status
    /// and raw body for `interpret_response`. `include_release_dates_with_no_data=true`
    /// surfaces *upcoming* (not-yet-released) dates; the realtime window bounds the dates
    /// to the calendar's `[start, end]` span server-side. A transport error returns `Err`
    /// to `fetch_calendar`, which records it as an `Unavailable` calendar gap.
    fn get_release_dates(
        &self,
        release_id: u32,
        realtime_start: &str,
        realtime_end: &str,
    ) -> Result<(u16, String)> {
        let id = release_id.to_string();
        crate::http_retry::send_with_retry("FRED release-dates", || {
            self.http.get(FRED_RELEASE_DATES_URL).query(&[
                ("release_id", id.as_str()),
                ("api_key", self.api_key.as_str()),
                ("file_type", "json"),
                ("include_release_dates_with_no_data", "true"),
                ("realtime_start", realtime_start),
                ("realtime_end", realtime_end),
                ("sort_order", "asc"),
            ])
        })
    }

    /// Gather the Step-6 economic-release calendar: each curated release's prior-week and
    /// upcoming dates within the window, shaped into `EconomicRelease`s. The calendar is
    /// an additive group with no floor, so it degrades quietly: a per-release "does not
    /// exist" 400 is silent (a permanent absence shouldn't clutter the manifest), while a
    /// this-run failure (auth / quota / 5xx / transport / malformed) is recorded as a
    /// `Calendar` gap so the agent knows the schedule was thinned this week. An empty
    /// result (a quiet window) is valid; the actual figures reach the model through the
    /// series groups regardless.
    fn fetch_calendar(&self, today: NaiveDate, gaps: &mut Vec<DataGap>) -> Vec<EconomicRelease> {
        let start = (today - Duration::days(CALENDAR_BACK_DAYS))
            .format("%Y-%m-%d")
            .to_string();
        let end = (today + Duration::days(CALENDAR_FWD_DAYS))
            .format("%Y-%m-%d")
            .to_string();
        let mut out = Vec::new();
        let mut rejected = false;
        for (release_id, name) in RELEASES {
            let id_str = release_id.to_string();
            if rejected {
                gaps.push(DataGap::new(GroupKind::Calendar, id_str, *name, GapReason::Rejected));
                continue;
            }
            let disposition = match self.get_release_dates(*release_id, &start, &end) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            match disposition {
                Disposition::Value(value) => match releases_to_calendar(value, name, today) {
                    Ok(entries) => out.extend(entries),
                    Err(_) => {
                        gaps.push(DataGap::new(GroupKind::Calendar, id_str, *name, GapReason::Malformed))
                    }
                },
                // Additive group: a permanent absence (does-not-exist) is silent.
                Disposition::Gap(GapReason::OutOfScope) => {}
                Disposition::Gap(reason) => {
                    if reason == GapReason::Rejected {
                        rejected = true;
                    }
                    gaps.push(DataGap::new(GroupKind::Calendar, id_str, *name, reason));
                }
            }
        }
        out
    }
}

impl MarketDataSource for FredDataSource {
    fn baseline_scan(&self) -> Result<BaselineMarketData> {
        // Each series degrades to a recorded gap rather than failing the scan; an empty
        // `internals` or `macro_levels` group is no longer this adapter's call to abort
        // on — the central coverage gate (`pipeline::enforce_coverage`) decides the run's
        // floor over the merged baseline. So this scan returns `Ok` for all data
        // outcomes. FRED owns the internals + macro groups and the calendar; indices /
        // sectors / labor are left empty for the composite to fill from FMP / BLS.
        let mut gaps = Vec::new();
        let internals = self.fetch_series(INTERNALS_SERIES, GroupKind::Internals, &mut gaps);
        let macro_levels = self.fetch_series(MACRO_SERIES, GroupKind::MacroLevels, &mut gaps);
        let calendar = self.fetch_calendar(Utc::now().date_naive(), &mut gaps);
        Ok(BaselineMarketData {
            indices: Vec::new(),
            internals,
            sectors: Vec::new(),
            macro_levels,
            labor_levels: Vec::new(),
            calendar,
            index_performance: Vec::new(),
            gaps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_response_covers_the_full_matrix() {
        use GapReason::*;
        // 2xx observations body -> a value to shape.
        assert!(matches!(
            interpret_response(200, r#"{"observations":[{"date":"2026-06-04","value":"4.30"}]}"#),
            Disposition::Value(_)
        ));

        // A 400 whose error_message says the series is absent -> OutOfScope per-series gap.
        let absent = r#"{"error_code":400,"error_message":"Bad Request. The series does not exist."}"#;
        assert!(matches!(interpret_response(400, absent), Disposition::Gap(OutOfScope)));

        // A 400 whose error_message is an api_key problem -> Rejected (key rejected).
        let bad_key = r#"{"error_code":400,"error_message":"Bad Request. The value for variable api_key is not registered, is not active, or is otherwise invalid."}"#;
        assert!(matches!(interpret_response(400, bad_key), Disposition::Gap(Rejected)));

        // An unrecognized 400 (empty / unfamiliar message) fails closed as Malformed
        // rather than being misread as a missing series.
        assert!(matches!(interpret_response(400, "{}"), Disposition::Gap(Malformed)));

        // Systemic statuses -> Unavailable regardless of body.
        for status in [429, 500, 503] {
            assert!(matches!(interpret_response(status, ""), Disposition::Gap(Unavailable)), "HTTP {status}");
        }

        // A 2xx that isn't valid JSON is a contract violation -> Malformed.
        assert!(matches!(interpret_response(200, "not json at all"), Disposition::Gap(Malformed)));
    }

    #[test]
    fn observations_to_quote_takes_latest_and_computes_change() {
        // Newest-first: latest 4.30, prior 4.20 -> +2.38% (off the prior value).
        let v: Value = serde_json::from_str(
            r#"{"observations":[
                {"date":"2026-06-04","value":"4.30"},
                {"date":"2026-06-03","value":"4.20"}
            ]}"#,
        )
        .unwrap();
        let q = observations_to_quote(v, "DGS10", "10-Year Treasury Yield", "percent")
            .unwrap()
            .expect("a quote");
        assert_eq!(q.symbol, "DGS10");
        assert_eq!(q.name, "10-Year Treasury Yield");
        assert!((q.price - 4.30).abs() < 1e-9);
        assert!((q.change_pct - (0.10 / 4.20 * 100.0)).abs() < 1e-9);
        // The series' unit rides onto the quote from the table, labelling `price`.
        assert_eq!(q.unit, "percent");
    }

    #[test]
    fn observations_to_quote_skips_gap_markers() {
        // FRED's "." gap markers are skipped: the latest *numeric* value wins, and
        // the change is computed off the next numeric value past the gap.
        let v: Value = serde_json::from_str(
            r#"{"observations":[
                {"date":"2026-06-07","value":"."},
                {"date":"2026-06-06","value":"."},
                {"date":"2026-06-05","value":"78.00"},
                {"date":"2026-06-04","value":"80.00"}
            ]}"#,
        )
        .unwrap();
        let q = observations_to_quote(v, "DCOILWTICO", "WTI Crude Oil", "USD per barrel")
            .unwrap()
            .expect("a quote past the gaps");
        assert!((q.price - 78.0).abs() < 1e-9);
        assert!((q.change_pct - (-2.0 / 80.0 * 100.0)).abs() < 1e-9);
    }

    #[test]
    fn observations_to_quote_all_gaps_is_a_skip_not_an_error() {
        // A series with no numeric observation in the window returns Ok(None) (a skip,
        // not an error) — the caller then records it as an Unavailable gap.
        let v: Value =
            serde_json::from_str(r#"{"observations":[{"date":"2026-06-07","value":"."}]}"#).unwrap();
        assert!(observations_to_quote(v, "DGS2", "x", "percent").unwrap().is_none());
    }

    #[test]
    fn observations_to_quote_single_value_has_no_change() {
        // One numeric observation -> a quote with a 0.0 change (no prior to diff).
        let v: Value =
            serde_json::from_str(r#"{"observations":[{"date":"2026-06-04","value":"4.30"}]}"#)
                .unwrap();
        let q = observations_to_quote(v, "DGS2", "2-Year Treasury Yield", "percent")
            .unwrap()
            .expect("a quote");
        assert!((q.price - 4.30).abs() < 1e-9);
        assert_eq!(q.change_pct, 0.0);
    }

    #[test]
    fn observations_to_quote_rejects_a_malformed_body() {
        // A 2xx body without the `observations` array is a contract violation.
        let v: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(observations_to_quote(v, "DGS2", "x", "percent").is_err());
    }

    #[test]
    fn observations_to_quote_rejects_nonnumeric_and_nonfinite_values() {
        // "." is the only skippable marker. A non-numeric value — or one that parses
        // to a non-finite float (NaN / inf, which f64::parse accepts) — returns an error
        // the caller records as a Malformed gap, not a silent drop. Otherwise a stale
        // value could read as current, or a NaN could contaminate the change math.
        for bad in ["garbage", "NaN", "inf", "-inf", "infinity"] {
            let v: Value = serde_json::from_str(&format!(
                r#"{{"observations":[{{"date":"2026-06-04","value":"{bad}"}}]}}"#
            ))
            .unwrap();
            assert!(
                observations_to_quote(v, "DGS2", "x", "percent").is_err(),
                "value {bad:?} must fail closed, not skip"
            );
        }
    }

    #[test]
    fn releases_to_calendar_classifies_status_and_windows() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 6).unwrap();
        // Window is [2026-05-27, 2026-06-27]. A too-old date and a too-far-ahead date
        // are dropped; a past date is "released", today and a future in-window date are
        // "upcoming".
        let v: Value = serde_json::from_str(
            r#"{"release_dates":[
                {"release_id":10,"date":"2026-05-01"},
                {"release_id":10,"date":"2026-06-05"},
                {"release_id":10,"date":"2026-06-06"},
                {"release_id":10,"date":"2026-06-10"},
                {"release_id":10,"date":"2026-07-14"}
            ]}"#,
        )
        .unwrap();
        let cal = releases_to_calendar(v, "Consumer Price Index", today).unwrap();
        assert_eq!(cal.len(), 3, "out-of-window dates dropped: {cal:?}");
        assert_eq!(cal[0].date, "2026-06-05");
        assert_eq!(cal[0].status, "released");
        assert_eq!(cal[0].release, "Consumer Price Index");
        assert!(cal[0].expected.is_none(), "no free consensus on the FRED path");
        // Today counts as upcoming — not yet confirmed released at an arbitrary run time.
        assert_eq!(cal[1].date, "2026-06-06");
        assert_eq!(cal[1].status, "upcoming");
        assert_eq!(cal[2].date, "2026-06-10");
        assert_eq!(cal[2].status, "upcoming");
    }

    #[test]
    fn releases_to_calendar_empty_is_empty() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 6).unwrap();
        let v: Value = serde_json::from_str(r#"{"release_dates":[]}"#).unwrap();
        assert!(releases_to_calendar(v, "x", today).unwrap().is_empty());
    }

    #[test]
    fn releases_to_calendar_rejects_malformed_body_and_bad_date() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 6).unwrap();
        // A body without the `release_dates` array is a contract violation.
        let bad_shape: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(releases_to_calendar(bad_shape, "x", today).is_err());
        // An unparseable date fails closed rather than being silently dropped.
        let bad_date: Value =
            serde_json::from_str(r#"{"release_dates":[{"date":"June 6th"}]}"#).unwrap();
        assert!(releases_to_calendar(bad_date, "x", today).is_err());
    }

    #[test]
    #[ignore = "hits the live FRED API; set FRED_API_KEY"]
    fn fred_baseline_smoke() {
        let src = FredDataSource::from_env().expect("FRED_API_KEY set");
        let data = src.baseline_scan().expect("live baseline scan");

        // Print the resolved mapping so a maintainer can confirm each series came
        // back (run with `-- --ignored --nocapture`); the offline tests can only
        // check fixture shapes, not the live series — this is where a removed or
        // renamed series id surfaces (the lesson of the original gold id, since
        // moved to FMP).
        let dump = |label: &str, quotes: &[Quote]| {
            eprintln!("{label} ({}):", quotes.len());
            for q in quotes {
                eprintln!(
                    "  {:<20} {:<34} price={:<12} change_pct={:<10} unit={}",
                    q.symbol, q.name, q.price, q.change_pct, q.unit
                );
            }
        };
        dump("internals", &data.internals);
        dump("macro_levels", &data.macro_levels);

        // Both groups are non-optional Step-6 baseline data. Assert each resolves in
        // full so a silently dropped (renamed / discontinued) series fails the smoke
        // loudly rather than thinning the baseline unnoticed — the per-symbol-assert
        // discipline `fmp_baseline_smoke` uses for its free-tier-sensitive symbols.
        assert_eq!(
            data.internals.len(),
            INTERNALS_SERIES.len(),
            "an internals series did not resolve"
        );
        assert_eq!(
            data.macro_levels.len(),
            MACRO_SERIES.len(),
            "a macro series did not resolve"
        );

        // The economic-release calendar is additive (no completeness floor), so assert it
        // resolved *something* and that every entry is well-formed — a curated release
        // name, a released/upcoming status, and an in-window date. A wrong / retired
        // release_id surfaces in the per-release dump below by contributing no rows; a
        // strict per-id assert would be flaky for the lower-cadence releases (GDP and
        // other quarterly reports) that legitimately have no date in a given ~month
        // window.
        eprintln!("calendar ({}):", data.calendar.len());
        for c in &data.calendar {
            eprintln!("  {:<9} {:<46} {}", c.status, c.release, c.date);
        }
        assert!(
            !data.calendar.is_empty(),
            "the economic-release calendar resolved no releases"
        );
        let names: std::collections::HashSet<&str> = RELEASES.iter().map(|(_, n)| *n).collect();
        for c in &data.calendar {
            assert!(
                names.contains(c.release.as_str()),
                "calendar carried an uncurated release {:?}",
                c.release
            );
            assert!(
                c.status == "released" || c.status == "upcoming",
                "calendar entry has a bad status {:?}",
                c.status
            );
            assert!(
                NaiveDate::parse_from_str(&c.date, "%Y-%m-%d").is_ok(),
                "calendar entry has an unparseable date {:?}",
                c.date
            );
        }

        // Per-id validation — two distinct failure modes, each caught explicitly:
        //  (1) wrong-but-valid id (points at a *different* real release): `release/dates`
        //      can't catch it — it echoes whatever id you query — so verify each id by
        //      name against FRED's `releases` catalog.
        //  (2) retired / scheduleless id: verify each resolves to >=1 real date over a
        //      wide window (the windowed calendar can't — a low-cadence release like GDP
        //      legitimately has no date in the ±~month window).
        // Together these are the per-symbol-resolution discipline the series smokes use.
        #[derive(Deserialize)]
        struct Catalog {
            releases: Vec<CatalogEntry>,
        }
        #[derive(Deserialize)]
        struct CatalogEntry {
            id: u32,
            name: String,
        }
        let catalog_body = src
            .http
            .get("https://api.stlouisfed.org/fred/releases")
            .query(&[
                ("api_key", src.api_key.as_str()),
                ("file_type", "json"),
                ("limit", "1000"),
            ])
            .send()
            .expect("releases catalog request")
            .text()
            .expect("releases catalog body");
        let catalog: Catalog =
            serde_json::from_str(&catalog_body).expect("releases catalog shape");
        let id_to_name: std::collections::HashMap<u32, &str> = catalog
            .releases
            .iter()
            .map(|r| (r.id, r.name.as_str()))
            .collect();

        let today = Utc::now().date_naive();
        let wide_start = (today - Duration::days(400)).format("%Y-%m-%d").to_string();
        let wide_end = today.format("%Y-%m-%d").to_string();
        for (id, name) in RELEASES {
            // (1) the id is the release we think it is.
            assert_eq!(
                id_to_name.get(id),
                Some(name),
                "release id {id} maps to {:?} in FRED's catalog, not {name:?} — wrong id",
                id_to_name.get(id)
            );
            // (2) it resolves to real scheduled dates.
            let (status, body) = src
                .get_release_dates(*id, &wide_start, &wide_end)
                .expect("release-dates request");
            let value = match interpret_response(status, &body) {
                Disposition::Value(v) => v,
                Disposition::Gap(reason) => {
                    panic!("release {id} ({name}) did not resolve ({reason:?}) — wrong/retired id")
                }
            };
            let parsed: FredReleaseDates =
                serde_json::from_value(value).expect("release-dates shape");
            assert!(
                !parsed.release_dates.is_empty(),
                "release {id} ({name}) resolved no dates over a wide window — retired id"
            );
        }
    }
}
