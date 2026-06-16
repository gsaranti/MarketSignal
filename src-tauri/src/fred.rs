//! Real FRED (Federal Reserve Economic Data) adapter for the macro / commodity
//! half of the baseline market-data scan.
//!
//! The second data-source adapter behind the `MarketDataSource` trait
//! (`data_sources`), the sibling of `fmp`. It owns the Step-3 market internals FMP
//! does not serve on its free tier (verified live: HTTP 402 "premium"): the
//! **2Y / 10Y Treasury yields**, the **US dollar index**, and the **oil / natural
//! gas** commodity prices (`docs/weekly-report-workflow.md §Step 3`,
//! `docs/data-sources.md §FRED`). Each is a canonical free FRED daily series; the
//! results are appended to the baseline's `internals` group alongside FMP's VIX and
//! gold by the composite source. (Gold is served free on FMP via `GCUSD` and stays
//! there — FRED's former gold benchmark series were removed, so this adapter owns no
//! gold series.)
//!
//! It also owns the Step-3 **macro levels** group (`macro_levels`): the Fed-funds
//! target range as the policy-stance proxy (futures-implied expectations aren't on
//! FRED's free tier, and no other data source supplies them free), the 5y / 10y
//! inflation breakevens, U. Michigan consumer sentiment, and the PCE price index.
//! These are point-in-time levels reusing the same observations machinery, kept in a
//! group distinct from the market internals. Daily series (target range, breakevens)
//! report a day-over-day `change_pct`; monthly series (sentiment, PCE), month-over-
//! month.
//!
//! It additionally owns the Step-3 **economic-release calendar** (`calendar`): the
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

use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{anyhow, Context, Result};
use chrono::{Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{
    emit_series_row, BaselineMarketData, DataGap, EconomicRelease, GapReason, GroupKind,
    MarketDataSource, Quote,
};
use crate::progress::RunContext;

/// Base URL for FRED's API. The endpoint paths below are joined onto it in each
/// request helper; a test redirects the whole adapter at a localhost mock via
/// [`FredDataSource::with_base_url`], so the wire path runs offline.
const FRED_BASE: &str = "https://api.stlouisfed.org/fred";

/// FRED's observations endpoint — the series time-series the baseline reads.
const FRED_OBSERVATIONS_PATH: &str = "/series/observations";

/// Short timeout per request: the baseline scan issues several sequential calls,
/// none of which should park for the model adapter's 120s ceiling. Mirrors `fmp`.
const FRED_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// How many of the most recent observations to request per series. Newest-first,
/// enough to find the two most recent *numeric* values across FRED's `"."` gaps
/// (weekends / holidays / unpublished days) so the day-over-day change resolves.
const OBSERVATION_LIMIT: &str = "10";

/// The FRED-owned market internals of the Step-3 baseline (`docs/weekly-report
/// -workflow.md §Step 3`), paired with a display name and the `price` unit. Each is a
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
    // Volatility term structure (pair VXVCLS with the FMP VIX for a backwardation read)
    // and the Nasdaq-100 implied-vol gauge; the level is the signal, like the spreads
    // above. NB use VXNCLS (the CBOE VXN, ~20s), NOT the similarly-named NASDAQVOLNDX —
    // that series was discontinued (frozen Jan 2026) and reads ~11,900, an index level not
    // a vol gauge (live-verified Jun 2026).
    ("VXVCLS", "CBOE S&P 500 3-Month Volatility (VXV)", "index points"),
    ("VXNCLS", "CBOE NASDAQ-100 Volatility Index (VXN)", "index points"),
    // Credit-quality dispersion on top of the aggregate HY/IG OAS: BBB (lowest IG rung)
    // and single-B (mid HY) widen at different speeds as risk appetite deteriorates.
    ("BAMLC0A4CBBB", "US Corporate BBB OAS", "percent"),
    ("BAMLH0A2HYB", "US High-Yield B OAS", "percent"),
];

/// The FRED-owned macro levels of the Step-3 baseline (`docs/weekly-report
/// -workflow.md §Step 3`, the "Macro" group): the Fed-funds target range as the
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

/// Publication cadence of a FRED series — the axis the freshness guard reads.
/// `fetch_series` requests the newest observations with **no date bound**, so a
/// **discontinued / frozen** series (its last datum published months ago — the
/// `NASDAQVOLNDX` class of bug, see the internals doc-comment above) still "resolves"
/// to a stale quote with no error. The guard ([`observations_to_quote`]) drops a quote
/// whose latest observation is older than its cadence allows, so a frozen series
/// becomes a recorded `Unavailable` gap rather than feeding a months-old level into
/// the baseline. Bounds are deliberately **loose** — sized to catch a multi-month
/// freeze, not to nitpick normal publication lag (JOLTS prints ~6 weeks after its
/// reference month; GDP a month after quarter-end with later revisions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cadence {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
}

impl Cadence {
    /// Maximum acceptable staleness (today − latest observation date), in days,
    /// before a series reads as discontinued rather than merely lagging.
    ///
    /// FRED dates each observation at its period **start**, so staleness peaks just
    /// before the *next* period's value is published — that peak, not the cadence
    /// interval, sets the bound. The monthly/quarterly bounds are sized to the laggiest
    /// member of each bucket (JOLTS lags ~6 weeks; GDP's Qn advance estimate lands ~1
    /// month after Qn ends, ~7 months after Qn started), so the guard is coarse for slow
    /// series by design — it reliably catches a multi-month freeze, not a one-cycle
    /// delay. The daily/weekly bounds stay tight, where the guard has the most value.
    /// The `#[ignore]`d `tuning_freshness_headroom_probe` reports the live headroom
    /// against these bounds so they can be re-tuned from real lag rather than guessed.
    const fn max_staleness_days(self) -> i64 {
        match self {
            Cadence::Daily => 16, // business-day series + weekends/holidays/lag (DTWEXBGS lags ~1wk)
            Cadence::Weekly => 21, // one-week cadence + publication lag + a holiday week
            Cadence::Monthly => 110, // JOLTS: ~6wk lag peaks ~95d before the next print
            Cadence::Quarterly => 230, // GDP: dated quarter-start, peaks ~209d before the next advance estimate
        }
    }
}

/// Cadence for every `INTERNALS_SERIES` + `MACRO_SERIES` id, the freshness guard's
/// lookup table. The `freshness_table_covers_every_series` parity test asserts this
/// set equals the two series tables exactly, so a new series with no cadence fails
/// offline CI rather than going unguarded.
const FRESHNESS: &[(&str, Cadence)] = &[
    // Internals — all daily, market-priced series.
    ("DGS2", Cadence::Daily),
    ("DGS10", Cadence::Daily),
    ("DTWEXBGS", Cadence::Daily),
    ("DCOILWTICO", Cadence::Daily),
    ("DHHNGSP", Cadence::Daily),
    ("BAMLH0A0HYM2", Cadence::Daily),
    ("BAMLC0A0CM", Cadence::Daily),
    ("T10Y3M", Cadence::Daily),
    ("T10Y2Y", Cadence::Daily),
    ("VXVCLS", Cadence::Daily),
    ("VXNCLS", Cadence::Daily),
    ("BAMLC0A4CBBB", Cadence::Daily),
    ("BAMLH0A2HYB", Cadence::Daily),
    // Macro levels — mixed cadence.
    ("DFEDTARU", Cadence::Daily),
    ("DFEDTARL", Cadence::Daily),
    ("T5YIE", Cadence::Daily),
    ("T10YIE", Cadence::Daily),
    ("UMCSENT", Cadence::Monthly),
    ("PCEPI", Cadence::Monthly),
    ("PPIFIS", Cadence::Monthly),
    ("RSAFS", Cadence::Monthly),
    ("JTSJOL", Cadence::Monthly),
    ("GDPC1", Cadence::Quarterly),
    ("NFCI", Cadence::Weekly),
    ("ANFCI", Cadence::Weekly),
    ("STLFSI4", Cadence::Weekly),
    ("ICSA", Cadence::Weekly),
    ("CCSA", Cadence::Weekly),
    ("WALCL", Cadence::Weekly),
    ("MORTGAGE30US", Cadence::Weekly),
];

/// Look up a series' publication cadence. Falls back to the tightest bound (`Daily`)
/// for an unmapped id — fail-tight, so an accidentally-unmapped series is guarded (and
/// surfaces as a dropped / `Unavailable` gap) rather than slipping through unguarded.
/// The `freshness_table_covers_every_series` parity test guarantees every shipped
/// series is mapped, so the fallback is unreachable in practice; it exists only to keep
/// the production path panic-free (the fail-soft scan must never abort on a lookup).
fn cadence_for(series_id: &str) -> Cadence {
    FRESHNESS
        .iter()
        .find(|(id, _)| *id == series_id)
        .map(|(_, c)| *c)
        .unwrap_or(Cadence::Daily)
}

/// FRED's release-dates endpoint — the economic-release *schedule* the Step-3 calendar
/// reads, distinct from the series observations above. `include_release_dates_with_no_data`
/// surfaces upcoming (not-yet-released) dates; the realtime window bounds the dates to
/// the calendar span.
const FRED_RELEASE_DATES_PATH: &str = "/release/dates";

/// The economic-release calendar window: days back (prior-week reports already released)
/// and forward (the upcoming schedule) of today to keep. Applied both server-side (the
/// `realtime_start` / `realtime_end` query params) and again in `releases_to_calendar`.
const CALENDAR_BACK_DAYS: i64 = 10;
const CALENDAR_FWD_DAYS: i64 = 21;

/// The curated market-moving US economic releases of the Step-3 calendar, as
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

/// FRED's observations response, trimmed to the two fields the baseline needs. Each
/// observation's `value` is a **string** — a number like `"4.30"` or FRED's `"."`
/// gap marker for a day with no datum — so it is parsed (and `"."` skipped) when
/// shaping the quote, never deserialized as `f64` directly. The `date` (the
/// observation's period, `"YYYY-MM-DD"`) is read off the latest numeric observation
/// for the freshness guard: a frozen / discontinued series resolves to a stale value
/// with no error, so its age is the only signal it has gone dark.
#[derive(Debug, Deserialize)]
struct FredObservations {
    observations: Vec<FredObservation>,
}

#[derive(Debug, Deserialize)]
struct FredObservation {
    date: String,
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
/// `Ok(None)` when the series has no usable current datum — either no numeric
/// observation in the window (all gaps), or a **stale** latest observation (older
/// than `cadence`'s bound relative to `today`): a per-series absence, not an error.
///
/// The freshness drop closes the frozen-series hole: `get` requests the newest
/// observations with no date bound, so a discontinued series (the `NASDAQVOLNDX`
/// class) resolves to a months-old value with no error. Dating the latest numeric
/// observation against its [`Cadence`] catches that — the stale value is dropped to
/// `Ok(None)`, recorded downstream as an `Unavailable` gap exactly like an all-gap
/// window, rather than masquerading as a current level. `today` is injected (not read
/// from the clock here) to keep the shaper pure and testable, mirroring
/// [`releases_to_calendar`].
///
/// Fail-closed on the value: FRED's documented `"."` is the **only** skippable
/// marker. Any other non-numeric value — or one that parses to a non-finite float
/// (`NaN` / `inf`, which `f64::parse` accepts) — is a contract violation that fails
/// the scan rather than being silently dropped as a gap (which would let a stale
/// observation masquerade as current, or a `NaN` contaminate the change math). An
/// unparseable `date` on the latest numeric observation is likewise an error (the
/// freshness guard can't judge it), and a body that is not the expected observations
/// shape is too.
fn observations_to_quote(
    value: Value,
    symbol: &str,
    name: &str,
    unit: &str,
    cadence: Cadence,
    today: NaiveDate,
) -> Result<Option<Quote>> {
    let raw: FredObservations = serde_json::from_value(value)
        .context("FRED observations response did not match the expected shape")?;
    // The most-recent numeric observations, newest-first; latest + prior is all the
    // change needs, so stop at two. The latest's date is captured for the freshness
    // guard (fail-closed if it won't parse).
    let mut numeric: Vec<f64> = Vec::with_capacity(2);
    let mut latest_date: Option<NaiveDate> = None;
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
        if latest_date.is_none() {
            // The first numeric row is the latest; date it for the freshness check.
            let d = NaiveDate::parse_from_str(obs.date.trim(), "%Y-%m-%d").with_context(|| {
                format!(
                    "FRED returned an unparseable observation date {:?} for series {symbol}",
                    obs.date
                )
            })?;
            latest_date = Some(d);
        }
        numeric.push(parsed);
        if numeric.len() == 2 {
            break;
        }
    }
    let Some(&latest) = numeric.first() else {
        return Ok(None); // every observation was a "." gap — no recent datum
    };
    // Freshness guard: drop a frozen / discontinued series whose latest datum is staler
    // than its cadence allows, so a months-old level can't read as current. `latest_date`
    // is `Some` whenever `numeric.first()` is (both set on the same row).
    if let Some(d) = latest_date {
        let staleness = (today - d).num_days();
        if staleness > cadence.max_staleness_days() {
            return Ok(None);
        }
    }
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
    /// API origin the endpoint paths are joined onto. Defaults to [`FRED_BASE`]; an
    /// offline round-trip test overrides it via [`FredDataSource::with_base_url`].
    base_url: String,
    /// Run context for live progress + cooperative cancellation; a no-op by default
    /// (tests / smokes), the live one attached via [`FredDataSource::with_context`].
    progress: Arc<RunContext>,
}

impl FredDataSource {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(FRED_TIMEOUT)
            .build()
            .context("building the FRED HTTP client")?;
        Ok(Self {
            api_key,
            http,
            base_url: FRED_BASE.to_string(),
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

    /// Attach a live run context so the per-series scan streams a tracker row per
    /// request and stops making requests once a cancel is observed.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
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
        let url = format!("{}{FRED_OBSERVATIONS_PATH}", self.base_url);
        crate::http_retry::send_with_retry("FRED", || {
            self.http.get(&url).query(&[
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
    /// an `OutOfScope` gap; an all-gap window — or a **stale** latest observation, dated
    /// against the series' cadence relative to `today` — is `Unavailable` (no usable value
    /// this run, so it counts against coverage); an `api_key` rejection is `Rejected`
    /// and — being a whole-provider condition — stops the loop, recording the rest
    /// without hammering; a systemic / unrecognized response or a body that won't shape
    /// is `Unavailable` / `Malformed`. Shared by the internals and macro-levels groups,
    /// which differ only in their series list and `group` tag.
    fn fetch_series(
        &self,
        series: &[(&str, &str, &str)],
        group: GroupKind,
        today: NaiveDate,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<Quote> {
        let mut out = Vec::with_capacity(series.len());
        let mut rejected = false;
        for (series_id, name, unit) in series {
            if self.progress.is_cancelled() {
                break;
            }
            if rejected {
                // No request made for a short-circuited series — no tracker row.
                gaps.push(DataGap::new(group, *series_id, *name, GapReason::Rejected));
                continue;
            }
            self.progress
                .request_started("FRED", group.as_str(), *series_id, *name);
            let gaps_before = gaps.len();
            let out_before = out.len();
            let disposition = match self.get(series_id) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable), // transport — unreachable
            };
            match disposition {
                Disposition::Value(value) => match observations_to_quote(
                    value,
                    series_id,
                    name,
                    unit,
                    cadence_for(series_id),
                    today,
                ) {
                    Ok(Some(quote)) => out.push(quote),
                    // No usable current value — every observation was a "." gap, or the
                    // latest numeric one is staler than its cadence allows (a frozen /
                    // discontinued series). Not a permanent/premium absence, so it counts
                    // against coverage (Unavailable), unlike an explicit "does not exist".
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
            emit_series_row(
                &self.progress,
                "FRED",
                group,
                series_id,
                name,
                gaps,
                gaps_before,
                out.len() > out_before,
            );
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
        let url = format!("{}{FRED_RELEASE_DATES_PATH}", self.base_url);
        crate::http_retry::send_with_retry("FRED release-dates", || {
            self.http.get(&url).query(&[
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

    /// Gather the Step-3 economic-release calendar: each curated release's prior-week and
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
            if self.progress.is_cancelled() {
                break;
            }
            // `id_str` is borrowed (not moved) into the gaps so it survives for the
            // tracker row emitted at the end of the iteration.
            let id_str = release_id.to_string();
            if rejected {
                // No request made for a short-circuited release — no tracker row.
                gaps.push(DataGap::new(
                    GroupKind::Calendar,
                    id_str.as_str(),
                    *name,
                    GapReason::Rejected,
                ));
                continue;
            }
            self.progress.request_started(
                "FRED release-dates",
                GroupKind::Calendar.as_str(),
                &id_str,
                *name,
            );
            let gaps_before = gaps.len();
            let out_before = out.len();
            let disposition = match self.get_release_dates(*release_id, &start, &end) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            match disposition {
                Disposition::Value(value) => match releases_to_calendar(value, name, today) {
                    Ok(entries) => out.extend(entries),
                    Err(_) => gaps.push(DataGap::new(
                        GroupKind::Calendar,
                        id_str.as_str(),
                        *name,
                        GapReason::Malformed,
                    )),
                },
                // Additive group: a permanent absence (does-not-exist) is silent.
                Disposition::Gap(GapReason::OutOfScope) => {}
                Disposition::Gap(reason) => {
                    if reason == GapReason::Rejected {
                        rejected = true;
                    }
                    gaps.push(DataGap::new(GroupKind::Calendar, id_str.as_str(), *name, reason));
                }
            }
            emit_series_row(
                &self.progress,
                "FRED release-dates",
                GroupKind::Calendar,
                &id_str,
                name,
                gaps,
                gaps_before,
                out.len() > out_before,
            );
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
        // One `today` anchors both the freshness guard (per-series staleness) and the
        // calendar window, so the whole scan reads against a single clock sample.
        let today = Utc::now().date_naive();
        let internals = self.fetch_series(INTERNALS_SERIES, GroupKind::Internals, today, &mut gaps);
        let macro_levels = self.fetch_series(MACRO_SERIES, GroupKind::MacroLevels, today, &mut gaps);
        let calendar = self.fetch_calendar(today, &mut gaps);
        Ok(BaselineMarketData {
            indices: Vec::new(),
            internals,
            sectors: Vec::new(),
            macro_levels,
            labor_levels: Vec::new(),
            calendar,
            index_performance: Vec::new(),
            // FMP owns the equity-market movers, earnings calendar, and valuation +
            // finer-rotation snapshots; FRED contributes none of them.
            movers: Vec::new(),
            earnings: Vec::new(),
            sector_pe: Vec::new(),
            industries: Vec::new(),
            market_risk_premium: Vec::new(),
            gaps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    // `Cadence`, `FRESHNESS`, `cadence_for`, and `Cadence::max_staleness_days` now live
    // in the production module above — the freshness guard runs in `observations_to_quote`,
    // not just the smoke — and are reached here through `use super::*`. A fixed "today"
    // near the offline fixtures' early-June-2026 dates keeps them fresh for the tests that
    // aren't about freshness; the freshness behavior has its own dedicated tests below.
    fn fresh_today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 8).unwrap()
    }

    #[test]
    fn freshness_table_covers_every_series() {
        use std::collections::HashSet;
        let series: HashSet<&str> = INTERNALS_SERIES
            .iter()
            .chain(MACRO_SERIES)
            .map(|(id, _, _)| *id)
            .collect();
        let freshness: HashSet<&str> = FRESHNESS.iter().map(|(id, _)| *id).collect();
        let missing: Vec<&str> = series.difference(&freshness).copied().collect();
        let stray: Vec<&str> = freshness.difference(&series).copied().collect();
        assert!(
            missing.is_empty(),
            "series with no FRESHNESS cadence (add them): {missing:?}"
        );
        assert!(
            stray.is_empty(),
            "FRESHNESS entries for unknown series (remove them): {stray:?}"
        );
        // No duplicate ids in the table (a dup would mask a missing entry).
        assert_eq!(
            FRESHNESS.len(),
            freshness.len(),
            "FRESHNESS has duplicate series ids"
        );
    }

    /// The newest *numeric* observation's date in a FRED observations response, selecting
    /// the value **exactly as [`observations_to_quote`] does**: skip FRED's `"."` gap
    /// markers (newest-first, so the first non-`"."` row wins), and require that winning
    /// value to parse to a **finite `f64`** — production rejects a non-numeric / non-finite
    /// value (`"garbage"`, `"NaN"`, `"inf"`) by failing closed rather than skipping past it,
    /// so this stops at the same row and returns `None` rather than dating a value the
    /// baseline would never carry. `None` also when the body has no numeric observation or
    /// doesn't match the observations shape; the freshness guard turns that `None` into a
    /// loud panic, mirroring production's fail-closed. Used to date the value the baseline
    /// would carry, catching a series frozen months ago.
    fn latest_numeric_observation_date(value: &Value) -> Option<NaiveDate> {
        let observations = value.get("observations")?.as_array()?;
        for obs in observations {
            let raw = obs.get("value")?.as_str()?.trim();
            if raw == "." {
                continue; // documented gap — not a real datum
            }
            // Mirror observations_to_quote: the latest non-gap value must be a finite
            // number, else it's a contract violation — fail closed, don't let a malformed
            // value masquerade as a fresh datum. Production errors here rather than skipping
            // to the next row, so this returns `None` at the same row instead of searching on.
            raw.parse::<f64>().ok().filter(|n: &f64| n.is_finite())?;
            let date_str = obs.get("date")?.as_str()?.trim();
            return NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok();
        }
        None
    }

    #[test]
    fn latest_numeric_observation_date_skips_gaps() {
        // Newest-first with leading "." gaps: the first numeric row dates the value.
        let v: Value = serde_json::from_str(
            r#"{"observations":[
                {"date":"2026-06-07","value":"."},
                {"date":"2026-06-06","value":"."},
                {"date":"2026-06-05","value":"78.00"},
                {"date":"2026-06-04","value":"80.00"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            latest_numeric_observation_date(&v),
            Some(NaiveDate::from_ymd_opt(2026, 6, 5).unwrap())
        );
        // All gaps -> no dated value.
        let all_gaps: Value =
            serde_json::from_str(r#"{"observations":[{"date":"2026-06-07","value":"."}]}"#).unwrap();
        assert!(latest_numeric_observation_date(&all_gaps).is_none());
        // Wrong shape -> None, not a panic.
        let bad: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(latest_numeric_observation_date(&bad).is_none());
        // A non-numeric / non-finite latest value fails closed -> None (the smoke then
        // panics), mirroring observations_to_quote's rejection. It must not date the bogus
        // value, nor skip past it to an older numeric row.
        for bad in ["garbage", "NaN", "inf", "-inf", "infinity"] {
            let v: Value = serde_json::from_str(&format!(
                r#"{{"observations":[{{"date":"2026-06-05","value":"{bad}"}},{{"date":"2026-06-04","value":"4.30"}}]}}"#
            ))
            .unwrap();
            assert!(
                latest_numeric_observation_date(&v).is_none(),
                "value {bad:?} must fail closed, not date a bogus or older value"
            );
        }
    }

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

    // ---- Offline round trip: adapter -> retry -> interpret -> domain output ----
    //
    // The matrix above pins `interpret_response` as a pure function; these drive the
    // whole `get`/`get_release_dates` -> `send_with_retry` -> `interpret_response` path
    // against a localhost mock (`crate::test_http`). FRED has two endpoints, each
    // building its own URL, so each is rebased and covered: `fetch_series` (observations,
    // through to the shaped `Quote`) and `get_release_dates` (the release-dates wire).
    // Single-reply scripts, so no `BASE_BACKOFF` sleep is incurred.

    fn test_source(base_url: &str) -> FredDataSource {
        FredDataSource::new("test-key".to_string())
            .expect("build adapter")
            .with_base_url(base_url)
    }

    #[test]
    fn fetch_series_round_trips_a_200_into_a_quote() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"observations":[{"date":"2026-06-04","value":"4.30"},{"date":"2026-06-03","value":"4.20"}]}"#,
        }]);
        let source = test_source(&server.base_url);
        let mut gaps = Vec::new();
        let quotes = source.fetch_series(
            &[("DGS10", "10-Year Treasury Yield", "percent")],
            GroupKind::MacroLevels,
            fresh_today(),
            &mut gaps,
        );
        assert_eq!(server.attempts(), 1, "one series => one request");
        let targets = server.request_targets();
        assert_eq!(server.request_paths(), ["/series/observations"]);
        assert!(targets[0].contains("series_id="), "the per-call query var must reach the wire: {targets:?}");
        assert!(gaps.is_empty());
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].symbol, "DGS10");
        assert!((quotes[0].price - 4.30).abs() < 1e-9);
        assert_eq!(quotes[0].unit, "percent");
    }

    #[test]
    fn fetch_series_round_trips_an_absent_series_into_an_out_of_scope_gap() {
        // A 400 "series does not exist" must classify as an OutOfScope gap over the wire.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 400,
            headers: vec![],
            body: r#"{"error_code":400,"error_message":"Bad Request. The series does not exist."}"#,
        }]);
        let source = test_source(&server.base_url);
        let mut gaps = Vec::new();
        let quotes = source.fetch_series(
            &[("NOPE", "Missing Series", "percent")],
            GroupKind::MacroLevels,
            fresh_today(),
            &mut gaps,
        );
        assert_eq!(server.attempts(), 1);
        assert_eq!(server.request_paths(), ["/series/observations"]);
        assert!(quotes.is_empty());
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].reason, GapReason::OutOfScope);
        assert_eq!(gaps[0].series_id, "NOPE");
    }

    #[test]
    fn get_release_dates_round_trips_through_the_rebased_endpoint() {
        // The second endpoint builds its own URL; this proves it is rebased onto the
        // mock and the (status, body) rides back through the retry seam intact.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"release_dates":[{"release_id":10,"date":"2026-06-11"}]}"#,
        }]);
        let source = test_source(&server.base_url);
        let (status, body) = source
            .get_release_dates(10, "2026-06-08", "2026-06-22")
            .expect("release-dates request reaches the mock");
        assert_eq!(server.attempts(), 1);
        // The distinct second-endpoint path proves it rebased onto the mock, not just
        // that some FRED URL did — a `FRED_RELEASE_DATES_PATH` typo would fail here.
        assert_eq!(server.request_paths(), ["/release/dates"]);
        assert_eq!(status, 200);
        assert!(body.contains("\"release_id\":10"));
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
        let q = observations_to_quote(
            v,
            "DGS10",
            "10-Year Treasury Yield",
            "percent",
            Cadence::Daily,
            fresh_today(),
        )
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
        let q = observations_to_quote(
            v,
            "DCOILWTICO",
            "WTI Crude Oil",
            "USD per barrel",
            Cadence::Daily,
            fresh_today(),
        )
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
        assert!(
            observations_to_quote(v, "DGS2", "x", "percent", Cadence::Daily, fresh_today())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn observations_to_quote_single_value_has_no_change() {
        // One numeric observation -> a quote with a 0.0 change (no prior to diff).
        let v: Value =
            serde_json::from_str(r#"{"observations":[{"date":"2026-06-04","value":"4.30"}]}"#)
                .unwrap();
        let q = observations_to_quote(
            v,
            "DGS2",
            "2-Year Treasury Yield",
            "percent",
            Cadence::Daily,
            fresh_today(),
        )
        .unwrap()
        .expect("a quote");
        assert!((q.price - 4.30).abs() < 1e-9);
        assert_eq!(q.change_pct, 0.0);
    }

    #[test]
    fn observations_to_quote_rejects_a_malformed_body() {
        // A 2xx body without the `observations` array is a contract violation.
        let v: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(
            observations_to_quote(v, "DGS2", "x", "percent", Cadence::Daily, fresh_today()).is_err()
        );
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
                observations_to_quote(v, "DGS2", "x", "percent", Cadence::Daily, fresh_today())
                    .is_err(),
                "value {bad:?} must fail closed, not skip"
            );
        }
    }

    #[test]
    fn observations_to_quote_drops_a_stale_latest_observation() {
        // A frozen / discontinued series resolves to an old value with no error; dated
        // against its cadence it is too stale, so it drops to Ok(None) (an Unavailable gap
        // downstream) rather than feeding a months-old level into the baseline.
        let v: Value = serde_json::from_str(
            r#"{"observations":[
                {"date":"2026-01-02","value":"4.30"},
                {"date":"2026-01-01","value":"4.20"}
            ]}"#,
        )
        .unwrap();
        // ~157 days stale vs the Daily bound (16) on 2026-06-08.
        let q = observations_to_quote(
            v,
            "DGS10",
            "10-Year Treasury Yield",
            "percent",
            Cadence::Daily,
            fresh_today(),
        )
        .unwrap();
        assert!(q.is_none(), "a stale daily series must drop, not resolve");
    }

    #[test]
    fn observations_to_quote_freshness_is_a_closed_band_at_the_cadence_bound() {
        // The guard keeps an observation exactly at the bound and drops one a day past it,
        // pinning the inclusive `<=` and referencing the cadence's own const so a re-tune
        // can't silently invalidate the test (mirrors the FMP industry-P/E closed band).
        let today = fresh_today();
        let bound = Cadence::Daily.max_staleness_days();
        let at_bound = (today - Duration::days(bound)).format("%Y-%m-%d").to_string();
        let past_bound = (today - Duration::days(bound + 1))
            .format("%Y-%m-%d")
            .to_string();

        let exactly = serde_json::from_str::<Value>(&format!(
            r#"{{"observations":[{{"date":"{at_bound}","value":"4.30"}}]}}"#
        ))
        .unwrap();
        assert!(
            observations_to_quote(exactly, "DGS10", "x", "percent", Cadence::Daily, today)
                .unwrap()
                .is_some(),
            "an observation exactly at the staleness bound is kept"
        );

        let over = serde_json::from_str::<Value>(&format!(
            r#"{{"observations":[{{"date":"{past_bound}","value":"4.30"}}]}}"#
        ))
        .unwrap();
        assert!(
            observations_to_quote(over, "DGS10", "x", "percent", Cadence::Daily, today)
                .unwrap()
                .is_none(),
            "an observation one day past the bound is dropped"
        );
    }

    #[test]
    fn observations_to_quote_freshness_respects_per_cadence_bounds() {
        // The same age reads as fresh for a slow cadence and stale for a fast one: a
        // ~100-day-old observation is within the Monthly bound (110) but past the Daily
        // bound (16), so cadence — not just age — decides the drop.
        let today = fresh_today();
        let old = (today - Duration::days(100)).format("%Y-%m-%d").to_string();
        let body = format!(r#"{{"observations":[{{"date":"{old}","value":"4.30"}}]}}"#);

        let monthly = serde_json::from_str::<Value>(&body).unwrap();
        assert!(
            observations_to_quote(monthly, "UMCSENT", "x", "index", Cadence::Monthly, today)
                .unwrap()
                .is_some(),
            "a 100-day-old monthly observation is within the monthly bound"
        );
        let daily = serde_json::from_str::<Value>(&body).unwrap();
        assert!(
            observations_to_quote(daily, "DGS10", "x", "percent", Cadence::Daily, today)
                .unwrap()
                .is_none(),
            "the same 100-day-old observation is stale for a daily series"
        );
    }

    #[test]
    fn observations_to_quote_rejects_an_unparseable_latest_date() {
        // The latest numeric observation must carry a parseable date — the freshness guard
        // can't judge an unparseable one, so it fails closed (a Malformed gap downstream)
        // rather than letting an undateable value through.
        let v: Value =
            serde_json::from_str(r#"{"observations":[{"date":"June 4th","value":"4.30"}]}"#)
                .unwrap();
        assert!(
            observations_to_quote(v, "DGS10", "x", "percent", Cadence::Daily, fresh_today())
                .is_err(),
            "an unparseable latest-observation date must fail closed"
        );
    }

    #[test]
    fn fetch_series_records_a_stale_series_as_an_unavailable_gap() {
        // End to end over the wire: a series whose latest observation is months old drops
        // to an Unavailable gap (counts against coverage), not a quote — the production
        // freshness guard exercised through the adapter, not just the pure shaper.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"observations":[{"date":"2026-01-02","value":"4.30"},{"date":"2026-01-01","value":"4.20"}]}"#,
        }]);
        let source = test_source(&server.base_url);
        let mut gaps = Vec::new();
        let quotes = source.fetch_series(
            &[("DGS10", "10-Year Treasury Yield", "percent")],
            GroupKind::Internals,
            fresh_today(),
            &mut gaps,
        );
        assert!(quotes.is_empty(), "a stale series resolves to no quote");
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].reason, GapReason::Unavailable);
        assert_eq!(gaps[0].series_id, "DGS10");
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

        // Both groups are non-optional Step-3 baseline data. Assert each resolves in
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

        let today = Utc::now().date_naive();

        // Freshness guard — the count asserts above prove each series *resolves*, but a
        // discontinued / frozen series still resolves to a stale value (it just stops
        // getting new observations, so the no-date-bound `get` returns its last real
        // datum from months ago — the `NASDAQVOLNDX` class of bug). Re-fetch each series
        // (FRED is 120 req/min with no daily cap) and assert its latest *numeric*
        // observation is recent enough for its cadence, so a series going discontinued
        // fails the smoke loudly rather than feeding a stale level into the baseline.
        eprintln!("freshness (today = {today}):");
        for (series_id, name, _unit) in INTERNALS_SERIES.iter().chain(MACRO_SERIES) {
            let (status, body) = src.get(series_id).expect("freshness re-fetch");
            let value = match interpret_response(status, &body) {
                Disposition::Value(v) => v,
                Disposition::Gap(reason) => {
                    panic!("series {series_id} ({name}) did not resolve on re-fetch ({reason:?})")
                }
            };
            let date = latest_numeric_observation_date(&value).unwrap_or_else(|| {
                panic!("series {series_id} ({name}) had no numeric observation to date")
            });
            let staleness = (today - date).num_days();
            let cadence = cadence_for(series_id);
            let bound = cadence.max_staleness_days();
            eprintln!(
                "  {series_id:<14} {name:<34} latest={date} stale={staleness:>4}d \
                 (<= {bound}d, {cadence:?})"
            );
            assert!(
                staleness <= bound,
                "series {series_id} ({name}) latest observation {date} is {staleness} days \
                 stale (> {bound} for {cadence:?}) — likely discontinued; check the id"
            );
        }

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

    #[test]
    #[ignore = "hits the live FRED API; set FRED_API_KEY. Calibration aid, not a gate — \
                run with `-- --ignored --nocapture` to read the headroom table."]
    fn tuning_freshness_headroom_probe() {
        // Reports each series' live staleness against its cadence bound (headroom =
        // bound − staleness), tightest first, so the four `max_staleness_days` values can
        // be re-tuned from real publication lag rather than guessed. Unlike the freshness
        // assert in `fred_baseline_smoke` (the gate), this only reports — a series with
        // thin or negative headroom is a signal to investigate / re-tune, not a failure.
        // Tighten a bound only above the live max its members reach here.
        let src = FredDataSource::from_env().expect("FRED_API_KEY set");
        let today = Utc::now().date_naive();
        let mut rows: Vec<(i64, &str, &str, Cadence, i64, NaiveDate)> = Vec::new();
        for (series_id, name, _unit) in INTERNALS_SERIES.iter().chain(MACRO_SERIES) {
            let (status, body) = src.get(series_id).expect("freshness re-fetch");
            let value = match interpret_response(status, &body) {
                Disposition::Value(v) => v,
                Disposition::Gap(reason) => {
                    panic!("series {series_id} ({name}) did not resolve on re-fetch ({reason:?})")
                }
            };
            let date = latest_numeric_observation_date(&value).unwrap_or_else(|| {
                panic!("series {series_id} ({name}) had no numeric observation to date")
            });
            let cadence = cadence_for(series_id);
            let staleness = (today - date).num_days();
            let headroom = cadence.max_staleness_days() - staleness;
            rows.push((headroom, series_id, name, cadence, staleness, date));
        }
        rows.sort_by_key(|r| r.0); // tightest headroom first
        eprintln!("freshness headroom (today = {today}); tighten a bound only above the live max:");
        for (headroom, series_id, name, cadence, staleness, date) in &rows {
            eprintln!(
                "  headroom={headroom:>4}d  {series_id:<14} {name:<34} \
                 stale={staleness:>4}d / {bound}d {cadence:?} (latest={date})",
                bound = cadence.max_staleness_days(),
            );
        }
    }
}
