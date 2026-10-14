//! FRED adapter for the macro / commodity half of the baseline scan: the yield, dollar,
//! oil / gas and spread internals, the macro levels group, and the economic-release
//! calendar (`docs/report-workflow.md §Step 3`, `docs/data-sources.md §FRED`). Every
//! failure degrades to a recorded gap; `pipeline::enforce_coverage` owns the floor.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{anyhow, Context, Result};
use chrono::{Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{
    emit_series_row, BaselineMarketData, CalendarEventClass, Change, ChangeKind, DataGap,
    EconomicRelease, EconomicSurprise, GapReason, GroupKind, MarketDataSource, Quote,
};
use crate::cadence::ReportCadence;
use crate::econ_calendar::{self, CalendarRow, EconomicCalendarSource};
use crate::progress::RunContext;

const FRED_BASE: &str = "https://api.stlouisfed.org/fred";

const FRED_OBSERVATIONS_PATH: &str = "/series/observations";

/// Per-request timeout; the scan issues several sequential calls, none of which should
/// park for the model adapter's 120s ceiling.
const FRED_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// Newest-first observations per series: enough to find the two most recent numeric
/// values across FRED's `"."` gaps (weekends / holidays) so the change resolves.
const OBSERVATION_LIMIT: &str = "10";

/// The FRED-owned market internals of the Step-3 baseline
/// (`docs/report-workflow.md §Step 3`) as `(series_id, display name, unit)`. The
/// `series_id` doubles as the quote `symbol`; the unit labels what `price` is quoted
/// in, so the model doesn't read a yield as a dollar figure.
const INTERNALS_SERIES: &[(&str, &str, &str)] = &[
    ("DGS2", "2-Year Treasury Yield", "percent"),
    ("DGS10", "10-Year Treasury Yield", "percent"),
    (
        "DTWEXBGS",
        "US Dollar Index (Broad)",
        "index (Jan 2006=100)",
    ),
    ("DCOILWTICO", "WTI Crude Oil", "USD per barrel"),
    ("DHHNGSP", "Henry Hub Natural Gas", "USD per million BTU"),
    // Credit + curve spreads (daily, market-priced) — the level is the signal.
    ("BAMLH0A0HYM2", "US High-Yield Corporate OAS", "percent"),
    ("BAMLC0A0CM", "US Investment-Grade Corporate OAS", "percent"),
    ("T10Y3M", "10-Year minus 3-Month Treasury Spread", "percent"),
    ("T10Y2Y", "10-Year minus 2-Year Treasury Spread", "percent"),
    // Volatility term structure (VXVCLS pairs with the FMP VIX for a backwardation read)
    // and the Nasdaq-100 vol gauge. Use VXNCLS (the CBOE VXN, ~20s), NOT the
    // similarly-named NASDAQVOLNDX — discontinued (frozen Jan 2026) and reading ~11,900,
    // an index level, not a vol gauge.
    (
        "VXVCLS",
        "CBOE S&P 500 3-Month Volatility (VXV)",
        "index points",
    ),
    (
        "VXNCLS",
        "CBOE NASDAQ-100 Volatility Index (VXN)",
        "index points",
    ),
    // Credit-quality dispersion: BBB (lowest IG rung) and single-B (mid HY) widen at
    // different speeds as risk appetite deteriorates.
    ("BAMLC0A4CBBB", "US Corporate BBB OAS", "percent"),
    ("BAMLH0A2HYB", "US High-Yield B OAS", "percent"),
];

/// The FRED-owned macro levels of the Step-3 baseline (`docs/report-workflow.md §Step
/// 3`), same `(series_id, display name, unit)` shape as the internals. Mixed daily,
/// weekly, monthly and quarterly series; `change` reads off the prior observation
/// accordingly, except GDP, whose change is annualized ([`ANNUALIZED_SERIES`]).
const MACRO_SERIES: &[(&str, &str, &str)] = &[
    (
        "DFEDTARU",
        "Fed Funds Target Range — Upper Limit",
        "percent",
    ),
    (
        "DFEDTARL",
        "Fed Funds Target Range — Lower Limit",
        "percent",
    ),
    ("T5YIE", "5-Year Breakeven Inflation Rate", "percent"),
    ("T10YIE", "10-Year Breakeven Inflation Rate", "percent"),
    (
        "UMCSENT",
        "U. Michigan Consumer Sentiment",
        "index (1966Q1=100)",
    ),
    ("PCEPI", "PCE Price Index", "index (2017=100)"),
    (
        "PPIFIS",
        "Producer Price Index (Final Demand)",
        "index (Nov 2009=100)",
    ),
    (
        "RSAFS",
        "Advance Retail Sales (Retail & Food Services)",
        "millions of USD",
    ),
    (
        "JTSJOL",
        "Job Openings: Total Nonfarm (JOLTS)",
        "thousands of openings",
    ),
    (
        "GDPC1",
        "Real Gross Domestic Product (growth annualized)",
        "billions of chained 2017 USD",
    ),
    // Forward-looking expectation gauges. GDPNow is already an annualized rate (SAAR),
    // so it is deliberately NOT in ANNUALIZED_SERIES; both are rate-valued, so their
    // change is a point delta (RATE_DELTA_SERIES) — a percent-of-prior on the nowcast
    // reads as a spurious ~128% "move".
    (
        "GDPNOW",
        "Atlanta Fed GDPNow — Real GDP Growth Nowcast (annualized rate)",
        "percent",
    ),
    (
        "EXPINF1YR",
        "Cleveland Fed 1-Year Expected Inflation",
        "percent",
    ),
    // Weekly/daily risk + cycle gauges (financial conditions, claims, liquidity, housing).
    (
        "NFCI",
        "Chicago Fed National Financial Conditions Index",
        "index (0 = average)",
    ),
    ("ANFCI", "Chicago Fed Adjusted NFCI", "index (0 = average)"),
    (
        "STLFSI4",
        "St. Louis Fed Financial Stress Index",
        "index (0 = normal)",
    ),
    ("ICSA", "Initial Jobless Claims", "persons"),
    (
        "CCSA",
        "Continued Jobless Claims (Insured Unemployment)",
        "persons",
    ),
    (
        "WALCL",
        "Fed Total Assets (Balance Sheet)",
        "millions of USD",
    ),
    ("MORTGAGE30US", "30-Year Fixed Mortgage Rate", "percent"),
];

/// Publication cadence of a FRED series, the axis of the freshness guard: `fetch_series`
/// requests the newest observations with no date bound, so a discontinued series still
/// resolves to a stale quote with no error, and [`observations_to_quote`] drops a quote
/// older than its cadence allows. Bounds are deliberately loose — sized to catch a
/// multi-month freeze, not normal publication lag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cadence {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
}

impl Cadence {
    /// Maximum acceptable staleness (today − latest observation date) in days before a
    /// series reads as discontinued. FRED dates each observation at its period start,
    /// so staleness peaks just before the next period's value publishes; each bound is
    /// sized to the laggiest member of its bucket. `tuning_freshness_headroom_probe`
    /// reports live headroom against these bounds.
    const fn max_staleness_days(self) -> i64 {
        match self {
            Cadence::Daily => 16, // laggiest daily is EIA oil/gas (~8d live); margin for weekends/holidays
            Cadence::Weekly => 28, // continued claims (CCSA) lag initial by a week → ~17–19d live peak; 4wk bound keeps margin
            Cadence::Monthly => 110, // JOLTS: ~6wk lag peaks ~95d before the next print
            Cadence::Quarterly => 230, // GDP: dated quarter-start, peaks ~209d before the next advance estimate
        }
    }

    /// Periods per year, the compounding exponent for annualizing a single-period change
    /// ([`ANNUALIZED_SERIES`]): a quarterly change `r` annualizes to `(1 + r)^4 − 1`.
    const fn periods_per_year(self) -> i32 {
        match self {
            Cadence::Daily => 252, // trading days
            Cadence::Weekly => 52,
            Cadence::Monthly => 12,
            Cadence::Quarterly => 4,
        }
    }
}

/// Cadence for every `INTERNALS_SERIES` + `MACRO_SERIES` id; the
/// `freshness_table_covers_every_series` parity test asserts the sets match exactly, so
/// a new series with no cadence fails offline rather than going unguarded.
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
    ("GDPNOW", Cadence::Quarterly),
    ("EXPINF1YR", Cadence::Monthly),
    ("NFCI", Cadence::Weekly),
    ("ANFCI", Cadence::Weekly),
    ("STLFSI4", Cadence::Weekly),
    ("ICSA", Cadence::Weekly),
    ("CCSA", Cadence::Weekly),
    ("WALCL", Cadence::Weekly),
    ("MORTGAGE30US", Cadence::Weekly),
];

/// Falls back to the tightest bound (`Daily`) for an unmapped id, so an unmapped series
/// is guarded rather than slipping through; the parity test makes the fallback
/// unreachable, and it exists only to keep the fail-soft scan panic-free.
fn cadence_for(series_id: &str) -> Cadence {
    FRESHNESS
        .iter()
        .find(|(id, _)| *id == series_id)
        .map(|(_, c)| *c)
        .unwrap_or(Cadence::Daily)
}

/// Series whose change is reported annualized: FRED's `GDPC1` is a quarterly level, but
/// the headline figure is the quarter-over-quarter change compounded to an annual rate
/// (`(1 + qoq)^4 − 1`, the BEA convention). The `price` level is untouched.
const ANNUALIZED_SERIES: &[&str] = &["GDPC1"];

fn is_annualized(series_id: &str) -> bool {
    ANNUALIZED_SERIES.contains(&series_id)
}

/// Series whose change is the point change (`latest − prior`) rather than a percent of
/// the prior: their level is already a rate or a zero-centered index, where the market
/// convention is the percentage-point move and a percent-of-prior is unstable near zero
/// (GDPNow's SAAR nowcast would read as a spurious ~128% "move"). Disjoint from
/// [`ANNUALIZED_SERIES`]; [`ChangeKind::PointDelta`] rides on `change.kind` so the figure
/// carries its unit without a display-name marker.
const RATE_DELTA_SERIES: &[&str] = &[
    // Percent-quoted rates: a point delta reads as percentage points.
    "DGS2",
    "DGS10",
    "BAMLH0A0HYM2",
    "BAMLC0A0CM",
    "T10Y3M",
    "T10Y2Y",
    "BAMLC0A4CBBB",
    "BAMLH0A2HYB",
    "DFEDTARU",
    "DFEDTARL",
    "T5YIE",
    "T10YIE",
    "EXPINF1YR",
    "MORTGAGE30US",
    "GDPNOW",
    // Zero-centered conditions indices: a point delta reads in the index's own units.
    "NFCI",
    "ANFCI",
    "STLFSI4",
];

fn is_rate_delta(series_id: &str) -> bool {
    RATE_DELTA_SERIES.contains(&series_id)
}

fn change_kind_for(series_id: &str) -> ChangeKind {
    if is_annualized(series_id) {
        ChangeKind::Annualized
    } else if is_rate_delta(series_id) {
        ChangeKind::PointDelta
    } else {
        ChangeKind::Percent
    }
}

const FRED_RELEASE_DATES_PATH: &str = "/release/dates";

/// The economic-release calendar's lookback floor in days. The lookback scales with the
/// run cadence up to [`CALENDAR_BACK_MAX_DAYS`] ([`calendar_back_days`]) so a monthly run
/// keeps the whole gap's releases; the forward window ([`CALENDAR_FWD_DAYS`]) stays fixed.
const CALENDAR_BACK_DAYS: i64 = 10;

/// Max age of the newest numeric print [`FredDataSource::latest_rate_dated`] accepts as
/// a current rate anchor: the "." markers on weekends / holidays make a short lookback
/// legitimate; past this bound the latest print is stale, and the read fails onto the
/// callers' hard-fail / unknown postures.
const RATE_ANCHOR_MAX_AGE_DAYS: i64 = 10;
const CALENDAR_BACK_MAX_DAYS: i64 = 45;
const CALENDAR_FWD_DAYS: i64 = 21;

/// [`CALENDAR_BACK_DAYS`] on the first report (no prior interval), else the elapsed
/// interval rounded up and clamped to the floor / cap. The saturating `f64 as i64` cast
/// makes a non-finite interval clamp rather than panic.
fn calendar_back_days(elapsed_days: Option<f64>) -> i64 {
    match elapsed_days {
        None => CALENDAR_BACK_DAYS,
        Some(d) => (d.ceil() as i64).clamp(CALENDAR_BACK_DAYS, CALENDAR_BACK_MAX_DAYS),
    }
}

/// The curated market-moving US economic releases of the Step-3 calendar, as
/// `(FRED release_id, display name)`; `fred_baseline_smoke` verifies each id live. FOMC
/// is deliberately excluded: FRED has no scheduled-date calendar for its release, so
/// requesting upcoming dates fabricates one row per day; the policy stance rides on the
/// Fed-funds target-range series in `macro_levels` instead.
const RELEASES: &[(u32, &str)] = &[
    (50, "Employment Situation"),
    (10, "Consumer Price Index"),
    (46, "Producer Price Index"),
    (54, "Personal Income and Outlays"),
    (9, "Advance Monthly Sales for Retail and Food Services"),
    (192, "Job Openings and Labor Turnover Survey"),
    (53, "Gross Domestic Product"),
];

/// FRED's observations response. Each `value` is a string — a number or FRED's `"."`
/// gap marker — so it is parsed when shaping the quote, never deserialized as `f64`.
#[derive(Debug, Deserialize)]
struct FredObservations {
    observations: Vec<FredObservation>,
}

#[derive(Debug, Deserialize)]
struct FredObservation {
    date: String,
    value: String,
}

/// FRED's release-dates response; the scoped endpoint omits the release name, which
/// rides from the `RELEASES` table.
#[derive(Debug, Deserialize)]
struct FredReleaseDates {
    release_dates: Vec<FredReleaseDate>,
}

#[derive(Debug, Deserialize)]
struct FredReleaseDate {
    date: String,
}

/// One FRED response classified into a value to shape or a recorded gap.
enum Disposition {
    Value(Value),
    Gap(GapReason),
}

/// Interpret one FRED response by status × body. FRED returns 400 for both a rejected
/// key and a missing series, so the body's `error_message` disambiguates: "does not
/// exist" is `OutOfScope`, an `api_key` problem is `Rejected`, any other 400 is
/// `Malformed` (fail-closed). A 429 / 5xx is `Unavailable`.
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
                Disposition::Gap(GapReason::OutOfScope)
            } else if lower.contains("api_key") || lower.contains("api key") {
                Disposition::Gap(GapReason::Rejected)
            } else {
                Disposition::Gap(GapReason::Malformed)
            }
        }
        429 | 500..=599 => Disposition::Gap(GapReason::Unavailable),
        _ => Disposition::Gap(GapReason::Malformed),
    }
}

/// Shape an observations response into one quote: the latest numeric observation is
/// `price`, and `change` is its move from the prior one per `change_kind`. `Ok(None)`
/// means no usable current datum — every observation a `"."` gap, or the latest one
/// staler than `cadence` allows relative to `today` (a frozen series resolves to an old
/// value with no error) — a per-series absence, not an error. `today` is injected to
/// keep the shaper pure. Fail-closed on the value: `"."` is the only skippable marker;
/// any other non-numeric or non-finite value (`f64::parse` accepts `NaN` / `inf`), or an
/// unparseable latest date, is an error rather than a silent drop.
fn observations_to_quote(
    value: Value,
    symbol: &str,
    name: &str,
    unit: &str,
    cadence: Cadence,
    today: NaiveDate,
    change_kind: ChangeKind,
) -> Result<Option<Quote>> {
    let raw: FredObservations = serde_json::from_value(value)
        .context("FRED observations response did not match the expected shape")?;
    let mut numeric: Vec<f64> = Vec::with_capacity(2);
    let mut latest_date: Option<NaiveDate> = None;
    for obs in &raw.observations {
        let v = obs.value.trim();
        if v == "." {
            continue;
        }
        let parsed: f64 = v
            .parse()
            .ok()
            .filter(|n: &f64| n.is_finite())
            .ok_or_else(|| {
                anyhow!("FRED returned a non-numeric observation value {v:?} for series {symbol}")
            })?;
        if latest_date.is_none() {
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
        return Ok(None);
    };
    // Freshness guard; `latest_date` is `Some` whenever `numeric.first()` is.
    if let Some(d) = latest_date {
        let staleness = (today - d).num_days();
        if staleness > cadence.max_staleness_days() {
            return Ok(None);
        }
    }
    // A zero or absent prior yields no change rather than a division by zero. Annualizing
    // needs a strictly positive prior as the ratio base; the point-delta path is
    // division-free, so it stays correct for a series that sits near or crosses zero.
    let value = match (change_kind, numeric.get(1)) {
        (ChangeKind::Annualized, Some(&prev)) if prev > 0.0 => {
            ((latest / prev).powi(cadence.periods_per_year()) - 1.0) * 100.0
        }
        (ChangeKind::PointDelta, Some(&prev)) => latest - prev,
        (ChangeKind::Percent, Some(&prev)) if prev != 0.0 => (latest - prev) / prev * 100.0,
        _ => 0.0,
    };
    Ok(Some(Quote {
        symbol: symbol.to_string(),
        name: name.to_string(),
        price: latest,
        change: Change {
            value,
            kind: change_kind,
        },
        unit: unit.to_string(),
    }))
}

/// Shape a release-dates response into calendar entries for one release within
/// `[today − back_days, today + CALENDAR_FWD_DAYS]`, each `"released"` (before today) or
/// `"upcoming"` (today or later). An unparseable date is an error, which
/// `fetch_calendar` records as a `Malformed` gap rather than dropping silently.
fn releases_to_calendar(
    value: Value,
    name: &str,
    today: NaiveDate,
    back_days: i64,
    surprises: impl Fn(NaiveDate) -> Vec<EconomicSurprise>,
) -> Result<Vec<EconomicRelease>> {
    let raw: FredReleaseDates = serde_json::from_value(value)
        .context("FRED release/dates response did not match the expected shape")?;
    // The back window must be the caller's cadence-scaled one: re-filtering on the fixed
    // floor would silently drop released entries the scaled query deliberately kept.
    let earliest = today - Duration::days(back_days);
    let latest = today + Duration::days(CALENDAR_FWD_DAYS);
    let mut out = Vec::with_capacity(raw.release_dates.len());
    for rd in raw.release_dates {
        let date_str = rd.date.trim();
        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").with_context(|| {
            format!("FRED returned an unparseable release date {date_str:?} for release {name}")
        })?;
        if date < earliest || date > latest {
            continue; // defensive — the query's realtime params already bound the window
        }
        let status = if date < today { "released" } else { "upcoming" };
        out.push(EconomicRelease {
            release: name.to_string(),
            date: date_str.to_string(),
            status: status.to_string(),
            class: CalendarEventClass::Release,
            surprises: surprises(date),
        });
    }
    Ok(out)
}

/// Live FRED adapter behind the `MarketDataSource` trait.
pub struct FredDataSource {
    api_key: String,
    http: reqwest::blocking::Client,
    /// Defaults to [`FRED_BASE`]; tests redirect it at a localhost mock.
    base_url: String,
    /// A no-op unless attached via [`FredDataSource::with_context`].
    progress: Arc<RunContext>,
    /// The consensus feed the calendar joins against; `None` leaves the calendar at
    /// names and dates.
    consensus: Option<Box<dyn EconomicCalendarSource>>,
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
            consensus: None,
        })
    }

    /// Redirect the adapter at a localhost mock so the wire path runs offline.
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

    /// Attach the consensus feed the economic-release calendar joins against
    /// (`docs/data-sources.md §Report enrichment`).
    pub fn with_consensus(mut self, source: impl EconomicCalendarSource + 'static) -> Self {
        self.consensus = Some(Box::new(source));
        self
    }

    /// For the live smoke and any caller that bypasses the execution gate
    /// (`config::validate`), which runs ahead of this in the command path.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().fred_key()?)
    }

    /// The latest numeric print of one rate series as a decimal ratio (`4.5` → `0.045`,
    /// normalized at the adapter seam; `docs/portfolio-analysis.md` §Starting
    /// parameters). Errors on transport failure, a non-2xx, or no numeric observation;
    /// the caller owns the posture (`docs/portfolio-analysis.md` §Failure posture).
    pub fn latest_rate_decimal(&self, series_id: &str) -> Result<f64> {
        self.latest_rate_dated(series_id).map(|d| d.value)
    }

    /// [`Self::latest_rate_decimal`] with the print's observation date, the as-of date
    /// the persisted rate cache ages against (`docs/portfolio-analysis.md` §Starting
    /// parameters).
    pub fn latest_rate_dated(
        &self,
        series_id: &str,
    ) -> Result<crate::portfolio::engine::DatedValue> {
        if self.progress.is_cancelled() {
            anyhow::bail!("rate fetch skipped (run cancelled)");
        }
        self.progress
            .request_started("FRED", "suite-rate", series_id, "Rate anchor");
        let result = (|| -> Result<crate::portfolio::engine::DatedValue> {
            let (status, body) = self.get(series_id)?;
            if !(200..300).contains(&status) {
                anyhow::bail!("FRED returned {status} for {series_id}");
            }
            let parsed: FredObservations =
                serde_json::from_str(&body).context("parsing FRED observations")?;
            let latest = parsed
                .observations
                .iter()
                .find_map(|o| {
                    o.value
                        .parse::<f64>()
                        .ok()
                        // A parsed "NaN" / "inf" must read as a missing print, never a rate.
                        .filter(|v| v.is_finite())
                        .map(|percent| crate::portfolio::engine::DatedValue {
                            date: o.date.clone(),
                            value: percent / 100.0,
                        })
                })
                .with_context(|| format!("no numeric observation for {series_id}"))?;
            // Skipping "." markers to the prior print is correct FRED semantics, but the
            // fallback is bounded so a long unpublished run never serves an arbitrarily
            // stale print as the current anchor. The floor dates on the ET session, not
            // the UTC date: observation dates are market days, so a UTC date on an
            // evening-ET run slides the floor a day forward and reads a print sitting
            // exactly on the bound as stale.
            let fresh_floor = crate::market_clock::et_session_date(chrono::Utc::now())
                - chrono::Duration::days(RATE_ANCHOR_MAX_AGE_DAYS);
            let print_date = chrono::NaiveDate::parse_from_str(&latest.date, "%Y-%m-%d")
                .with_context(|| {
                    format!("undatable observation date {:?} for {series_id}", latest.date)
                })?;
            if print_date < fresh_floor {
                anyhow::bail!(
                    "latest numeric print for {series_id} is stale ({} — older than \
                     {RATE_ANCHOR_MAX_AGE_DAYS} days)",
                    latest.date
                );
            }
            Ok(latest)
        })();
        match &result {
            Ok(_) => self
                .progress
                .request_finished("FRED", "suite-rate", series_id, "Rate anchor", "ok", None),
            Err(e) => self.progress.request_finished(
                "FRED",
                "suite-rate",
                series_id,
                "Rate anchor",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }

    /// One date-ranged history request for a rate series as dated decimal observations,
    /// oldest first (`docs/data-sources.md §Portfolio Analysis — endpoint surface`).
    pub fn rate_history_decimal(
        &self,
        series_id: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
        self.dated_window(series_id, "suite-rate-history", "Rate history", 100.0, from, to)
    }

    /// One date-ranged window for a commodity series — dated observations, oldest first,
    /// on the series' own published units. Deliberately NOT [`Self::rate_history_decimal`]:
    /// the rate normalization (`/100`) applied to a price level would fabricate the value
    /// (`docs/data-sources.md §Portfolio Analysis — endpoint surface`).
    pub fn level_window(
        &self,
        series_id: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
        self.dated_window(series_id, "suite-commodity", "Commodity series", 1.0, from, to)
    }

    /// The shared date-ranged fetch behind the rate-history and commodity-level windows:
    /// dated observations, oldest first, each value divided by `divisor` (100.0 for the
    /// rate normalization, 1.0 for levels).
    fn dated_window(
        &self,
        series_id: &str,
        kind: &str,
        label: &str,
        divisor: f64,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
        if self.progress.is_cancelled() {
            anyhow::bail!("{label} fetch skipped (run cancelled)");
        }
        self.progress.request_started("FRED", kind, series_id, label);
        let result = (|| -> Result<Vec<crate::portfolio::engine::DatedValue>> {
            let url = format!("{}{FRED_OBSERVATIONS_PATH}", self.base_url);
            let (status, body) = crate::http_retry::send_with_retry("FRED", || {
                self.http.get(&url).query(&[
                    ("series_id", series_id),
                    ("api_key", self.api_key.as_str()),
                    ("file_type", "json"),
                    ("sort_order", "asc"),
                    ("observation_start", &from.format("%Y-%m-%d").to_string()),
                    ("observation_end", &to.format("%Y-%m-%d").to_string()),
                ])
            })?;
            if !(200..300).contains(&status) {
                anyhow::bail!("FRED returned {status} for {series_id} history");
            }
            let parsed: FredObservations =
                serde_json::from_str(&body).context("parsing FRED history observations")?;
            let mut out: Vec<crate::portfolio::engine::DatedValue> = parsed
                .observations
                .into_iter()
                .filter_map(|o| {
                    o.value
                        .parse::<f64>()
                        .ok()
                        // `f64::parse` accepts "NaN" / "inf", and a non-finite point
                        // would panic the percentile sort downstream.
                        .filter(|v| v.is_finite())
                        .map(|raw| crate::portfolio::engine::DatedValue {
                            date: o.date,
                            value: raw / divisor,
                        })
                })
                .collect();
            out.sort_by(|a, b| a.date.cmp(&b.date));
            Ok(out)
        })();
        match &result {
            Ok(_) => self
                .progress
                .request_finished("FRED", kind, series_id, label, "ok", None),
            Err(e) => self.progress.request_finished(
                "FRED",
                kind,
                series_id,
                label,
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }

    /// GET one series' newest observations; a transport error is `Err`, which the caller
    /// records as an `Unavailable` gap rather than failing the scan.
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

    /// Fetch one quote per series, recording a [`DataGap`] in `group` for any that
    /// doesn't resolve rather than failing the scan. An all-gap or stale window is
    /// `Unavailable` (it counts against coverage, unlike an `OutOfScope` absence); a
    /// `Rejected` key is a whole-provider condition, so it stops the loop and records
    /// the rest without hammering.
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
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            match disposition {
                Disposition::Value(value) => match observations_to_quote(
                    value,
                    series_id,
                    name,
                    unit,
                    cadence_for(series_id),
                    today,
                    change_kind_for(series_id),
                ) {
                    Ok(Some(quote)) => out.push(quote),
                    Ok(None) => gaps.push(DataGap::new(
                        group,
                        *series_id,
                        *name,
                        GapReason::Unavailable,
                    )),
                    Err(_) => {
                        gaps.push(DataGap::new(group, *series_id, *name, GapReason::Malformed))
                    }
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
                None,
            );
        }
        out
    }

    /// GET one release's scheduled dates. `include_release_dates_with_no_data=true` is
    /// what surfaces upcoming (not-yet-released) dates; the realtime window bounds the
    /// dates server-side.
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

    /// Gather the economic-release calendar. An additive group with no floor: a
    /// per-release "does not exist" is silent (a permanent absence shouldn't clutter the
    /// manifest), a this-run failure is recorded as a `Calendar` gap, and an empty
    /// window is valid. With a consensus feed attached, one fetch over the window
    /// precedes the loop, and the Fed / FOMC events are appended after the releases.
    fn fetch_calendar(
        &self,
        today: NaiveDate,
        back_days: i64,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<EconomicRelease> {
        let start_date = today - Duration::days(back_days);
        let end_date = today + Duration::days(CALENDAR_FWD_DAYS);
        let start = start_date.format("%Y-%m-%d").to_string();
        let end = end_date.format("%Y-%m-%d").to_string();
        let consensus = match &self.consensus {
            Some(source) if !self.progress.is_cancelled() => {
                let (from, to) = econ_calendar::fetch_window(start_date, end_date);
                source.economic_calendar(from, to, gaps)
            }
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        // The releases with a FRED date in the window — the ones the drift check reads.
        let mut dated: Vec<u32> = Vec::new();
        let mut rejected = false;
        for (release_id, name) in RELEASES {
            if self.progress.is_cancelled() {
                break;
            }
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
                Disposition::Value(value) => match releases_to_calendar(
                    value,
                    name,
                    today,
                    back_days,
                    |date| econ_calendar::surprises_for(*release_id, date, &consensus),
                ) {
                    Ok(entries) => {
                        if !entries.is_empty() {
                            dated.push(*release_id);
                        }
                        out.extend(entries);
                    }
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
                    gaps.push(DataGap::new(
                        GroupKind::Calendar,
                        id_str.as_str(),
                        *name,
                        reason,
                    ));
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
                None,
            );
        }
        self.emit_event_map_drift(&dated, &consensus, gaps);
        out.extend(econ_calendar::fed_events(&consensus, start_date, end_date, today));
        out
    }

    /// When the consensus fetch returned rows but a mapped event of a release dated in
    /// the window appears in none of them, one tracker row names the missing events — a
    /// renamed FMP event otherwise fails soft with no trace. The prompt never sees it.
    fn emit_event_map_drift(&self, dated: &[u32], consensus: &[CalendarRow], gaps: &[DataGap]) {
        let missing = econ_calendar::absent_events(dated, consensus);
        if missing.is_empty() {
            return;
        }
        let series_id = "economic-calendar-map";
        let name = "Economic Calendar (event map)";
        self.progress
            .request_started("FMP", GroupKind::Calendar.as_str(), series_id, name);
        emit_series_row(
            &self.progress,
            "FMP",
            GroupKind::Calendar,
            series_id,
            name,
            gaps,
            gaps.len(),
            false,
            Some(format!(
                "no FMP print carries {} in the window — the event map may need re-verifying",
                missing.join(", ")
            )),
        );
    }
}

impl MarketDataSource for FredDataSource {
    fn baseline_scan(&self, cadence: ReportCadence) -> Result<BaselineMarketData> {
        // Each series degrades to a recorded gap; `pipeline::enforce_coverage` decides the
        // floor over the merged baseline, so the scan returns `Ok` for all data outcomes.
        let mut gaps = Vec::new();
        // One `today` anchors both the freshness guard and the calendar window. It is the
        // ET session date: every observation date FRED serves is a market day, so a UTC
        // date on an evening-ET run ages each series by one and drops a print sitting
        // exactly on its staleness bound to a gap.
        let today = crate::market_clock::et_session_date(Utc::now());
        let internals = self.fetch_series(INTERNALS_SERIES, GroupKind::Internals, today, &mut gaps);
        let macro_levels =
            self.fetch_series(MACRO_SERIES, GroupKind::MacroLevels, today, &mut gaps);
        let calendar =
            self.fetch_calendar(today, calendar_back_days(cadence.elapsed_days()), &mut gaps);
        Ok(BaselineMarketData {
            indices: Vec::new(),
            internals,
            sectors: Vec::new(),
            macro_levels,
            labor_levels: Vec::new(),
            calendar,
            index_performance: Vec::new(),
            // FMP, BLS and CFTC own the remaining groups; the composite fills them.
            movers: Vec::new(),
            earnings: Vec::new(),
            sector_pe: Vec::new(),
            industries: Vec::new(),
            market_risk_premium: Vec::new(),
            cot_positioning: Vec::new(),
            primary_market: None,
            gaps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_sources::{BeatMiss, SurpriseRelation};
    use crate::test_http::{Canned, MockHttp};

    #[test]
    fn calendar_back_days_floors_caps_and_scales() {
        assert_eq!(calendar_back_days(None), CALENDAR_BACK_DAYS);
        assert_eq!(calendar_back_days(Some(2.0)), CALENDAR_BACK_DAYS);
        assert_eq!(calendar_back_days(Some(20.0)), 20);
        assert_eq!(calendar_back_days(Some(90.0)), CALENDAR_BACK_MAX_DAYS);
        assert_eq!(calendar_back_days(Some(-3.0)), CALENDAR_BACK_DAYS);
        assert_eq!(calendar_back_days(Some(f64::NAN)), CALENDAR_BACK_DAYS);
        assert_eq!(calendar_back_days(Some(f64::INFINITY)), CALENDAR_BACK_MAX_DAYS);
    }

    // Near the offline fixtures' early-June-2026 dates, so they read fresh in the tests
    // that aren't about freshness.
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
        // A duplicate id would mask a missing entry.
        assert_eq!(
            FRESHNESS.len(),
            freshness.len(),
            "FRESHNESS has duplicate series ids"
        );
    }

    #[test]
    fn forward_expectation_gauges_are_present_and_gdpnow_is_not_annualized() {
        let macro_ids: Vec<&str> = MACRO_SERIES.iter().map(|(id, _, _)| *id).collect();
        assert!(
            macro_ids.contains(&"GDPNOW"),
            "GDPNow missing from MACRO_SERIES"
        );
        assert!(
            macro_ids.contains(&"EXPINF1YR"),
            "expected-inflation missing from MACRO_SERIES"
        );
        assert_eq!(cadence_for("GDPNOW"), Cadence::Quarterly);
        assert_eq!(cadence_for("EXPINF1YR"), Cadence::Monthly);
        // Pins the deliberate exclusion against an "annualize every quarterly series" refactor.
        assert!(
            !is_annualized("GDPNOW"),
            "GDPNow is already SAAR — must not be annualized"
        );
        assert!(!is_annualized("EXPINF1YR"));
    }

    /// The newest numeric observation's date, selected exactly as [`observations_to_quote`]
    /// does: skip `"."` markers and require a finite value, returning `None` at a
    /// non-numeric row rather than searching on past it.
    fn latest_numeric_observation_date(value: &Value) -> Option<NaiveDate> {
        let observations = value.get("observations")?.as_array()?;
        for obs in observations {
            let raw = obs.get("value")?.as_str()?.trim();
            if raw == "." {
                continue;
            }
            raw.parse::<f64>().ok().filter(|n: &f64| n.is_finite())?;
            let date_str = obs.get("date")?.as_str()?.trim();
            return NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok();
        }
        None
    }

    #[test]
    fn latest_numeric_observation_date_skips_gaps() {
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
        let all_gaps: Value =
            serde_json::from_str(r#"{"observations":[{"date":"2026-06-07","value":"."}]}"#)
                .unwrap();
        assert!(latest_numeric_observation_date(&all_gaps).is_none());
        let bad: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(latest_numeric_observation_date(&bad).is_none());
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
        assert!(matches!(
            interpret_response(
                200,
                r#"{"observations":[{"date":"2026-06-04","value":"4.30"}]}"#
            ),
            Disposition::Value(_)
        ));

        let absent =
            r#"{"error_code":400,"error_message":"Bad Request. The series does not exist."}"#;
        assert!(matches!(
            interpret_response(400, absent),
            Disposition::Gap(OutOfScope)
        ));

        let bad_key = r#"{"error_code":400,"error_message":"Bad Request. The value for variable api_key is not registered, is not active, or is otherwise invalid."}"#;
        assert!(matches!(
            interpret_response(400, bad_key),
            Disposition::Gap(Rejected)
        ));

        assert!(matches!(
            interpret_response(400, "{}"),
            Disposition::Gap(Malformed)
        ));

        for status in [429, 500, 503] {
            assert!(
                matches!(
                    interpret_response(status, ""),
                    Disposition::Gap(Unavailable)
                ),
                "HTTP {status}"
            );
        }

        assert!(matches!(
            interpret_response(200, "not json at all"),
            Disposition::Gap(Malformed)
        ));
    }

    // ---- Offline round trip: adapter -> retry -> interpret -> domain output ----
    //
    // Single-reply scripts, so no `BASE_BACKOFF` sleep is incurred.

    fn test_source(base_url: &str) -> FredDataSource {
        FredDataSource::new("test-key".to_string())
            .expect("build adapter")
            .with_base_url(base_url)
    }

    /// A canned consensus feed: serves its rows (or records a gap and serves none, like a
    /// failed fetch) and records each requested window.
    struct CannedConsensus {
        rows: Vec<crate::econ_calendar::CalendarRow>,
        fail: bool,
        windows: Arc<std::sync::Mutex<Vec<(NaiveDate, NaiveDate)>>>,
    }

    impl EconomicCalendarSource for CannedConsensus {
        fn economic_calendar(
            &self,
            from: NaiveDate,
            to: NaiveDate,
            gaps: &mut Vec<DataGap>,
        ) -> Vec<crate::econ_calendar::CalendarRow> {
            self.windows.lock().unwrap().push((from, to));
            if self.fail {
                gaps.push(DataGap::new(
                    GroupKind::Calendar,
                    "economic-calendar",
                    "Economic Calendar (consensus)",
                    GapReason::Unavailable,
                ));
                return Vec::new();
            }
            self.rows.clone()
        }
    }

    /// The seven release-dates replies, in `RELEASES` order: Employment Situation and
    /// Personal Income released, JOLTS and GDP upcoming, the rest quiet.
    fn calendar_replies() -> Vec<Canned> {
        let reply = |body| Canned::Reply {
            status: 200,
            headers: vec![],
            body,
        };
        vec![
            reply(r#"{"release_dates":[{"release_id":50,"date":"2026-10-02"}]}"#),
            reply(r#"{"release_dates":[]}"#),
            reply(r#"{"release_dates":[]}"#),
            reply(r#"{"release_dates":[{"release_id":54,"date":"2026-09-30"}]}"#),
            reply(r#"{"release_dates":[]}"#),
            reply(r#"{"release_dates":[{"release_id":192,"date":"2026-10-14"}]}"#),
            reply(r#"{"release_dates":[{"release_id":53,"date":"2026-10-29"}]}"#),
        ]
    }

    fn consensus_rows() -> Vec<crate::econ_calendar::CalendarRow> {
        crate::econ_calendar::rows_from_value(serde_json::json!([
            {"date": "2026-10-02 12:30:00", "country": "US", "event": "Non Farm Payrolls (Sep)",
             "impact": "High", "estimate": 90, "actual": 29},
            {"date": "2026-10-02 12:30:00", "country": "US", "event": "Unemployment Rate (Sep)",
             "impact": "High", "estimate": 4.1, "actual": 4.2},
            {"date": "2026-09-30 12:30:00", "country": "US", "event": "PCE Price Index MoM (Aug)",
             "impact": "Medium", "estimate": null, "actual": 0.3},
            {"date": "2026-09-30 12:30:00", "country": "US", "event": "Personal Income MoM (Aug)",
             "impact": "High", "estimate": 0.4, "actual": 0.1},
            {"date": "2026-10-29 12:30:00", "country": "US", "event": "GDP Growth Rate QoQ (Q3)",
             "impact": "High", "estimate": 0.9, "actual": null},
            {"date": "2026-10-07 18:00:00", "country": "US", "event": "FOMC Minutes",
             "impact": "High", "estimate": null, "actual": null},
            {"date": "2026-10-28 18:00:00", "country": "US", "event": "Fed Interest Rate Decision",
             "impact": "High", "estimate": null, "actual": null},
            {"date": "2026-11-18 19:00:00", "country": "US", "event": "FOMC Minutes",
             "impact": "High", "estimate": null, "actual": null}
        ]))
        .unwrap()
    }

    #[test]
    fn the_calendar_joins_each_release_to_its_consensus_and_appends_the_fed_events() {
        let server = MockHttp::serve(calendar_replies());
        let windows = Arc::new(std::sync::Mutex::new(Vec::new()));
        let source = test_source(&server.base_url).with_consensus(CannedConsensus {
            rows: consensus_rows(),
            fail: false,
            windows: windows.clone(),
        });
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let mut gaps = Vec::new();
        let cal = source.fetch_calendar(today, 45, &mut gaps);
        assert!(gaps.is_empty(), "{gaps:?}");
        // One consensus fetch over the FRED window, padded a day on each side.
        assert_eq!(
            *windows.lock().unwrap(),
            [(
                NaiveDate::from_ymd_opt(2026, 8, 27).unwrap(),
                NaiveDate::from_ymd_opt(2026, 11, 3).unwrap()
            )]
        );
        let entry = |name: &str| cal.iter().find(|e| e.release == name).unwrap();

        let jobs = entry("Employment Situation");
        let events: Vec<_> = jobs
            .surprises
            .iter()
            .map(|s| (s.event.as_str(), s.relation, s.beat_miss))
            .collect();
        assert_eq!(
            events,
            [
                ("Non Farm Payrolls", Some(SurpriseRelation::Below), Some(BeatMiss::Miss)),
                ("Unemployment Rate", Some(SurpriseRelation::InLine), None),
            ]
        );
        // A null estimate drops that print alone; its sibling still carries.
        let income = entry("Personal Income and Outlays");
        assert_eq!(income.surprises.len(), 1);
        assert_eq!(income.surprises[0].event, "Personal Income MoM");
        assert_eq!(income.surprises[0].beat_miss, Some(BeatMiss::Miss));
        let jolts = entry("Job Openings and Labor Turnover Survey");
        assert_eq!(jolts.status, "upcoming");
        assert!(jolts.surprises.is_empty());
        let gdp = entry("Gross Domestic Product");
        assert_eq!(gdp.surprises.len(), 1);
        assert_eq!((gdp.surprises[0].estimate, gdp.surprises[0].actual), (0.9, None));
        let fed: Vec<_> = cal
            .iter()
            .filter(|e| e.class == CalendarEventClass::Fed)
            .map(|e| (e.release.as_str(), e.date.as_str(), e.status.as_str()))
            .collect();
        assert_eq!(
            fed,
            [
                ("FOMC Minutes", "2026-10-07", "released"),
                ("Fed Interest Rate Decision", "2026-10-28", "upcoming"),
            ]
        );
        assert!(cal[..4].iter().all(|e| e.class == CalendarEventClass::Release));
    }

    /// The `(series_id, status, detail)` of every finished FMP calendar tracker row.
    fn fmp_calendar_rows(
        rec: &crate::progress::RecordingReporter,
    ) -> Vec<(String, String, Option<String>)> {
        rec.messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished {
                    provider,
                    series_id,
                    status,
                    detail,
                    ..
                } if provider == "FMP" => Some((series_id, status, detail)),
                _ => None,
            })
            .collect()
    }

    fn recording_context() -> (Arc<crate::progress::RecordingReporter>, Arc<RunContext>) {
        let rec = Arc::new(crate::progress::RecordingReporter::default());
        let ctx = RunContext::new(
            "run",
            rec.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        (rec, ctx)
    }

    #[test]
    fn a_mapped_event_missing_from_the_whole_window_raises_one_event_map_row() {
        // The fixture feed carries none of the five named prints while each of their
        // releases has a FRED date in the window; CPI has no date, so its events go unchecked.
        let server = MockHttp::serve(calendar_replies());
        let (rec, ctx) = recording_context();
        let source = test_source(&server.base_url)
            .with_context(ctx)
            .with_consensus(CannedConsensus {
                rows: consensus_rows(),
                fail: false,
                windows: Arc::new(std::sync::Mutex::new(Vec::new())),
            });
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let mut gaps = Vec::new();
        source.fetch_calendar(today, 45, &mut gaps);
        let rows = fmp_calendar_rows(&rec);
        assert_eq!(rows.len(), 1, "{rows:?}");
        let (series_id, status, detail) = &rows[0];
        assert_eq!(series_id, "economic-calendar-map");
        assert_eq!(status, "empty");
        assert_eq!(
            detail.as_deref(),
            Some(
                "no FMP print carries Average Hourly Earnings MoM, Core PCE Price Index MoM, \
Personal Spending MoM, JOLTs Job Openings, GDP Price Index QoQ in the window — the event map \
may need re-verifying"
            )
        );
        assert!(gaps.is_empty(), "the signal is a tracker row, never a manifest gap");
    }

    #[test]
    fn a_window_carrying_every_mapped_event_raises_no_event_map_row() {
        let server = MockHttp::serve(calendar_replies());
        let (rec, ctx) = recording_context();
        let mut rows = consensus_rows();
        rows.extend(
            crate::econ_calendar::rows_from_value(serde_json::json!([
                {"date": "2026-10-02 12:30:00", "event": "Average Hourly Earnings MoM (Sep)"},
                {"date": "2026-09-30 12:30:00", "event": "Core PCE Price Index MoM (Aug)"},
                {"date": "2026-09-30 12:30:00", "event": "Personal Spending MoM (Aug)"},
                {"date": "2026-09-29 14:00:00", "event": "JOLTs Job Openings (Aug)"},
                {"date": "2026-10-29 12:30:00", "event": "GDP Price Index QoQ (Q3)"}
            ]))
            .unwrap(),
        );
        let source = test_source(&server.base_url)
            .with_context(ctx)
            .with_consensus(CannedConsensus {
                rows,
                fail: false,
                windows: Arc::new(std::sync::Mutex::new(Vec::new())),
            });
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        source.fetch_calendar(today, 45, &mut Vec::new());
        assert!(fmp_calendar_rows(&rec).is_empty());
    }

    #[test]
    fn a_failed_consensus_fetch_leaves_the_calendar_at_names_and_dates() {
        let server = MockHttp::serve(calendar_replies());
        let source = test_source(&server.base_url).with_consensus(CannedConsensus {
            rows: consensus_rows(),
            fail: true,
            windows: Arc::new(std::sync::Mutex::new(Vec::new())),
        });
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let mut gaps = Vec::new();
        let cal = source.fetch_calendar(today, 45, &mut gaps);
        assert_eq!(cal.len(), 4, "the FRED releases alone, no Fed events");
        assert!(cal.iter().all(|e| e.surprises.is_empty()));
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].series_id, "economic-calendar");
    }

    #[test]
    fn without_a_consensus_feed_the_calendar_is_names_and_dates() {
        let server = MockHttp::serve(calendar_replies());
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let mut gaps = Vec::new();
        let cal = test_source(&server.base_url).fetch_calendar(today, 45, &mut gaps);
        assert_eq!(cal.len(), 4);
        assert!(cal
            .iter()
            .all(|e| e.class == CalendarEventClass::Release && e.surprises.is_empty()));
    }

    #[test]
    fn every_mapped_release_id_is_a_tracked_fred_release() {
        // A row naming an id outside `RELEASES` could never join.
        for m in crate::econ_calendar::EVENT_MAP {
            assert!(
                RELEASES.iter().any(|(id, _)| *id == m.release_id),
                "{} names release {}",
                m.event,
                m.release_id
            );
        }
        for (id, name) in RELEASES {
            assert!(
                crate::econ_calendar::EVENT_MAP.iter().any(|m| m.release_id == *id),
                "{name} has no map row"
            );
        }
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
        assert!(
            targets[0].contains("series_id="),
            "the per-call query var must reach the wire: {targets:?}"
        );
        assert!(gaps.is_empty());
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].symbol, "DGS10");
        assert!((quotes[0].price - 4.30).abs() < 1e-9);
        assert_eq!(quotes[0].unit, "percent");
    }

    #[test]
    fn fetch_series_round_trips_an_absent_series_into_an_out_of_scope_gap() {
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
        // The second endpoint builds its own URL, so it must rebase onto the mock too.
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
        assert_eq!(server.request_paths(), ["/release/dates"]);
        assert_eq!(status, 200);
        assert!(body.contains("\"release_id\":10"));
    }

    #[test]
    fn observations_to_quote_takes_latest_and_computes_change() {
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
            ChangeKind::Percent,
        )
        .unwrap()
        .expect("a quote");
        assert_eq!(q.symbol, "DGS10");
        assert_eq!(q.name, "10-Year Treasury Yield");
        assert!((q.price - 4.30).abs() < 1e-9);
        assert!((q.change.value - (0.10 / 4.20 * 100.0)).abs() < 1e-9);
        assert_eq!(q.unit, "percent");
    }

    #[test]
    fn observations_to_quote_annualizes_a_quarterly_rate() {
        // 23500 from 23300 is a ~0.86% QoQ move that annualizes to ~3.5%.
        let v: Value = serde_json::from_str(
            r#"{"observations":[
                {"date":"2026-01-01","value":"23500"},
                {"date":"2025-10-01","value":"23300"}
            ]}"#,
        )
        .unwrap();
        let q = observations_to_quote(
            v,
            "GDPC1",
            "Real Gross Domestic Product (growth annualized)",
            "billions of chained 2017 USD",
            Cadence::Quarterly,
            fresh_today(),
            ChangeKind::Annualized,
        )
        .unwrap()
        .expect("a quote");
        assert!((q.price - 23500.0).abs() < 1e-9);
        let expected = ((23500.0_f64 / 23300.0).powi(4) - 1.0) * 100.0;
        assert!((q.change.value - expected).abs() < 1e-9);
        let raw_qoq = (23500.0_f64 - 23300.0) / 23300.0 * 100.0;
        assert!(
            q.change.value > raw_qoq * 3.0,
            "annualized {} should be ~4x the raw QoQ {}",
            q.change.value,
            raw_qoq
        );
    }

    #[test]
    fn observations_to_quote_leaves_a_nonannualized_series_unchanged() {
        let body = r#"{"observations":[
                {"date":"2026-01-01","value":"23500"},
                {"date":"2025-10-01","value":"23300"}
            ]}"#;
        let v: Value = serde_json::from_str(body).unwrap();
        let q = observations_to_quote(
            v,
            "PCEPI",
            "x",
            "index",
            Cadence::Quarterly,
            fresh_today(),
            ChangeKind::Percent,
        )
        .unwrap()
        .expect("a quote");
        let raw_qoq = (23500.0_f64 - 23300.0) / 23300.0 * 100.0;
        assert!((q.change.value - raw_qoq).abs() < 1e-9);
        // Keyed to the series, not the cadence.
        assert!(is_annualized("GDPC1"));
        assert!(!is_annualized("PCEPI"));
        assert_eq!(Cadence::Quarterly.periods_per_year(), 4);
    }

    #[test]
    fn change_kind_for_maps_each_series_to_its_treatment() {
        assert_eq!(change_kind_for("GDPC1"), ChangeKind::Annualized);
        assert_eq!(change_kind_for("GDPNOW"), ChangeKind::PointDelta);
        assert_eq!(change_kind_for("DGS10"), ChangeKind::PointDelta);
        assert_eq!(change_kind_for("NFCI"), ChangeKind::PointDelta);
        assert_eq!(change_kind_for("PCEPI"), ChangeKind::Percent);
        assert_eq!(change_kind_for("UMCSENT"), ChangeKind::Percent);
    }

    #[test]
    fn rate_delta_and_annualized_sets_are_disjoint() {
        // `change_kind_for`'s if/else-if would hide an overlap silently.
        for id in ANNUALIZED_SERIES {
            assert!(
                !RATE_DELTA_SERIES.contains(id),
                "{id} is in both ANNUALIZED_SERIES and RATE_DELTA_SERIES"
            );
        }
    }

    #[test]
    fn rate_delta_series_are_rate_or_centered_index_units() {
        // Guards against a level / price / count series sneaking into the point-delta set.
        for id in RATE_DELTA_SERIES {
            let unit = INTERNALS_SERIES
                .iter()
                .chain(MACRO_SERIES)
                .find(|(sid, _, _)| sid == id)
                .map(|(_, _, unit)| *unit)
                .unwrap_or_else(|| {
                    panic!("{id} is in RATE_DELTA_SERIES but not in any series table")
                });
            assert!(
                unit == "percent" || unit.contains("0 ="),
                "{id} has unit {unit:?} — a point-delta series must be a percent rate or a centered index"
            );
        }
    }

    #[test]
    fn observations_to_quote_reports_a_point_delta_for_a_rate_series() {
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
            ChangeKind::PointDelta,
        )
        .unwrap()
        .expect("a quote");
        assert!(
            (q.change.value - 0.10).abs() < 1e-9,
            "point delta, not {}",
            q.change.value
        );
        assert_eq!(q.change.kind, ChangeKind::PointDelta);
        assert_eq!(q.name, "10-Year Treasury Yield");
    }

    #[test]
    fn observations_to_quote_point_delta_is_signed_and_stable_across_zero() {
        // A centered index crossing zero (−0.05 → +0.10) reports +0.15, where a
        // percent-of-prior would blow up / flip sign.
        let v: Value = serde_json::from_str(
            r#"{"observations":[
                {"date":"2026-06-04","value":"0.10"},
                {"date":"2026-06-03","value":"-0.05"}
            ]}"#,
        )
        .unwrap();
        let q = observations_to_quote(
            v,
            "NFCI",
            "Chicago Fed National Financial Conditions Index",
            "index (0 = average)",
            Cadence::Weekly,
            fresh_today(),
            ChangeKind::PointDelta,
        )
        .unwrap()
        .expect("a quote");
        assert!(
            (q.change.value - 0.15).abs() < 1e-9,
            "signed level delta, not {}",
            q.change.value
        );
        assert_eq!(q.change.kind, ChangeKind::PointDelta);
        assert_eq!(q.name, "Chicago Fed National Financial Conditions Index");
    }

    #[test]
    fn a_point_delta_quote_serializes_self_describing_with_no_name_marker() {
        // Pins kind mapping → serde end to end so a `(Δ pp)` name marker can't creep back.
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
            change_kind_for("DGS10"),
        )
        .unwrap()
        .expect("a quote");
        let json = serde_json::to_string_pretty(&q).unwrap();
        assert!(
            json.contains("\"kind\": \"point_delta\""),
            "the change unit is self-describing in the JSON: {json}"
        );
        assert!(
            !json.contains("(Δ"),
            "the display name must carry no point-delta marker: {json}"
        );
    }

    #[test]
    fn observations_to_quote_skips_gap_markers() {
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
            ChangeKind::Percent,
        )
        .unwrap()
        .expect("a quote past the gaps");
        assert!((q.price - 78.0).abs() < 1e-9);
        assert!((q.change.value - (-2.0 / 80.0 * 100.0)).abs() < 1e-9);
    }

    #[test]
    fn observations_to_quote_all_gaps_is_a_skip_not_an_error() {
        let v: Value =
            serde_json::from_str(r#"{"observations":[{"date":"2026-06-07","value":"."}]}"#)
                .unwrap();
        assert!(observations_to_quote(
            v,
            "DGS2",
            "x",
            "percent",
            Cadence::Daily,
            fresh_today(),
            ChangeKind::Percent
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn observations_to_quote_single_value_has_no_change() {
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
            ChangeKind::Percent,
        )
        .unwrap()
        .expect("a quote");
        assert!((q.price - 4.30).abs() < 1e-9);
        assert_eq!(q.change.value, 0.0);
    }

    #[test]
    fn observations_to_quote_rejects_a_malformed_body() {
        let v: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(observations_to_quote(
            v,
            "DGS2",
            "x",
            "percent",
            Cadence::Daily,
            fresh_today(),
            ChangeKind::Percent
        )
        .is_err());
    }

    #[test]
    fn observations_to_quote_rejects_nonnumeric_and_nonfinite_values() {
        for bad in ["garbage", "NaN", "inf", "-inf", "infinity"] {
            let v: Value = serde_json::from_str(&format!(
                r#"{{"observations":[{{"date":"2026-06-04","value":"{bad}"}}]}}"#
            ))
            .unwrap();
            assert!(
                observations_to_quote(
                    v,
                    "DGS2",
                    "x",
                    "percent",
                    Cadence::Daily,
                    fresh_today(),
                    ChangeKind::Percent
                )
                .is_err(),
                "value {bad:?} must fail closed, not skip"
            );
        }
    }

    #[test]
    fn observations_to_quote_drops_a_stale_latest_observation() {
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
            ChangeKind::Percent,
        )
        .unwrap();
        assert!(q.is_none(), "a stale daily series must drop, not resolve");
    }

    #[test]
    fn observations_to_quote_freshness_is_a_closed_band_at_the_cadence_bound() {
        // Built on the cadence's own const so a re-tune can't silently invalidate the test.
        let today = fresh_today();
        let bound = Cadence::Daily.max_staleness_days();
        let at_bound = (today - Duration::days(bound))
            .format("%Y-%m-%d")
            .to_string();
        let past_bound = (today - Duration::days(bound + 1))
            .format("%Y-%m-%d")
            .to_string();

        let exactly = serde_json::from_str::<Value>(&format!(
            r#"{{"observations":[{{"date":"{at_bound}","value":"4.30"}}]}}"#
        ))
        .unwrap();
        assert!(
            observations_to_quote(
                exactly,
                "DGS10",
                "x",
                "percent",
                Cadence::Daily,
                today,
                ChangeKind::Percent
            )
            .unwrap()
            .is_some(),
            "an observation exactly at the staleness bound is kept"
        );

        let over = serde_json::from_str::<Value>(&format!(
            r#"{{"observations":[{{"date":"{past_bound}","value":"4.30"}}]}}"#
        ))
        .unwrap();
        assert!(
            observations_to_quote(
                over,
                "DGS10",
                "x",
                "percent",
                Cadence::Daily,
                today,
                ChangeKind::Percent
            )
            .unwrap()
            .is_none(),
            "an observation one day past the bound is dropped"
        );
    }

    #[test]
    fn observations_to_quote_freshness_respects_per_cadence_bounds() {
        // 100 days is within the Monthly bound (110) but past the Daily bound (16).
        let today = fresh_today();
        let old = (today - Duration::days(100)).format("%Y-%m-%d").to_string();
        let body = format!(r#"{{"observations":[{{"date":"{old}","value":"4.30"}}]}}"#);

        let monthly = serde_json::from_str::<Value>(&body).unwrap();
        assert!(
            observations_to_quote(
                monthly,
                "UMCSENT",
                "x",
                "index",
                Cadence::Monthly,
                today,
                ChangeKind::Percent
            )
            .unwrap()
            .is_some(),
            "a 100-day-old monthly observation is within the monthly bound"
        );
        let daily = serde_json::from_str::<Value>(&body).unwrap();
        assert!(
            observations_to_quote(
                daily,
                "DGS10",
                "x",
                "percent",
                Cadence::Daily,
                today,
                ChangeKind::Percent
            )
            .unwrap()
            .is_none(),
            "the same 100-day-old observation is stale for a daily series"
        );
    }

    #[test]
    fn observations_to_quote_rejects_an_unparseable_latest_date() {
        let v: Value =
            serde_json::from_str(r#"{"observations":[{"date":"June 4th","value":"4.30"}]}"#)
                .unwrap();
        assert!(
            observations_to_quote(
                v,
                "DGS10",
                "x",
                "percent",
                Cadence::Daily,
                fresh_today(),
                ChangeKind::Percent
            )
            .is_err(),
            "an unparseable latest-observation date must fail closed"
        );
    }

    #[test]
    fn fetch_series_records_a_stale_series_as_an_unavailable_gap() {
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
        // Window is [2026-05-27, 2026-06-27].
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
        let cal = releases_to_calendar(v, "Consumer Price Index", today, CALENDAR_BACK_DAYS, |_| {
            Vec::new()
        })
        .unwrap();
        assert_eq!(cal.len(), 3, "out-of-window dates dropped: {cal:?}");
        assert_eq!(cal[0].date, "2026-06-05");
        assert_eq!(cal[0].status, "released");
        assert_eq!(cal[0].release, "Consumer Price Index");
        // Today counts as upcoming — not yet confirmed released at an arbitrary run time.
        assert_eq!(cal[1].date, "2026-06-06");
        assert_eq!(cal[1].status, "upcoming");
        assert_eq!(cal[2].date, "2026-06-10");
        assert_eq!(cal[2].status, "upcoming");
    }

    #[test]
    fn latest_rate_anchor_skips_markers_but_bounds_the_fallback() {
        // Dates build on the ET session, the basis the bound itself uses: under a UTC
        // date the stale fixture lands exactly on the ET-dated floor during the
        // evening-ET hours after the UTC rollover and reads fresh.
        let today = crate::market_clock::et_session_date(chrono::Utc::now());
        let recent = (today - Duration::days(2)).format("%Y-%m-%d").to_string();
        let marker = today.format("%Y-%m-%d").to_string();
        let fresh_body = format!(
            r#"{{"observations":[{{"date":"{marker}","value":"."}},{{"date":"{recent}","value":"4.30"}}]}}"#
        );
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: Box::leak(fresh_body.into_boxed_str()),
        }]);
        let d = test_source(&server.base_url).latest_rate_dated("DGS10").unwrap();
        assert!((d.value - 0.043).abs() < 1e-12);
        assert_eq!(d.date, recent);

        let stale = (today - Duration::days(RATE_ANCHOR_MAX_AGE_DAYS + 1))
            .format("%Y-%m-%d")
            .to_string();
        let stale_body =
            format!(r#"{{"observations":[{{"date":"{stale}","value":"4.30"}}]}}"#);
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: Box::leak(stale_body.into_boxed_str()),
        }]);
        let err = test_source(&server.base_url)
            .latest_rate_dated("DGS10")
            .unwrap_err();
        assert!(err.to_string().contains("stale"), "{err}");
    }

    #[test]
    fn level_window_preserves_published_levels_and_sorts_oldest_first() {
        let body = r#"{"observations":[
            {"date":"2026-08-18","value":"78.40"},
            {"date":"2025-08-20","value":"82.10"},
            {"date":"2026-01-05","value":"."}
        ]}"#;
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body,
        }]);
        let from = NaiveDate::from_ymd_opt(2025, 8, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let window = test_source(&server.base_url)
            .level_window("DCOILWTICO", from, to)
            .unwrap();
        assert_eq!(window.len(), 2, "the '.' marker row drops: {window:?}");
        assert_eq!(window[0].date, "2025-08-20");
        assert!((window[0].value - 82.10).abs() < 1e-12, "level, not /100");
        assert_eq!(window[1].date, "2026-08-18");
        assert!((window[1].value - 78.40).abs() < 1e-12);

        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"observations":[{"date":"2026-08-18","value":"4.30"}]}"#,
        }]);
        let rates = test_source(&server.base_url)
            .rate_history_decimal("DGS10", from, to)
            .unwrap();
        assert!((rates[0].value - 0.043).abs() < 1e-12, "rates still normalize");
    }

    #[test]
    fn releases_to_calendar_honors_the_scaled_back_window() {
        // A release 25 days back is kept under a 45-day window, dropped under the 10-day floor.
        let today = NaiveDate::from_ymd_opt(2026, 6, 6).unwrap();
        let v = || -> Value {
            serde_json::from_str(r#"{"release_dates":[{"release_id":10,"date":"2026-05-12"}]}"#)
                .unwrap()
        };
        let scaled = releases_to_calendar(v(), "CPI", today, 45, |_| Vec::new()).unwrap();
        assert_eq!(scaled.len(), 1, "in-window under the scaled lookback: {scaled:?}");
        assert_eq!(scaled[0].status, "released");
        let floor = releases_to_calendar(v(), "CPI", today, CALENDAR_BACK_DAYS, |_| Vec::new()).unwrap();
        assert!(floor.is_empty(), "outside the 10-day floor: {floor:?}");
    }

    #[test]
    fn releases_to_calendar_empty_is_empty() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 6).unwrap();
        let v: Value = serde_json::from_str(r#"{"release_dates":[]}"#).unwrap();
        assert!(releases_to_calendar(v, "x", today, CALENDAR_BACK_DAYS, |_| Vec::new())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn releases_to_calendar_rejects_malformed_body_and_bad_date() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 6).unwrap();
        let bad_shape: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(releases_to_calendar(bad_shape, "x", today, CALENDAR_BACK_DAYS, |_| Vec::new()).is_err());
        let bad_date: Value =
            serde_json::from_str(r#"{"release_dates":[{"date":"June 6th"}]}"#).unwrap();
        assert!(releases_to_calendar(bad_date, "x", today, CALENDAR_BACK_DAYS, |_| Vec::new()).is_err());
    }

    #[test]
    #[ignore = "hits the live FRED API; set FRED_API_KEY"]
    fn fred_baseline_smoke() {
        let src = FredDataSource::from_env().expect("FRED_API_KEY set");
        let data = src.baseline_scan(ReportCadence::default()).expect("live baseline scan");

        // Print the resolved mapping (run with `-- --ignored --nocapture`): a removed or
        // renamed series id surfaces here, not in the offline fixture tests.
        let dump = |label: &str, quotes: &[Quote]| {
            eprintln!("{label} ({}):", quotes.len());
            for q in quotes {
                eprintln!(
                    "  {:<20} {:<34} price={:<12} change={:<10} kind={:<11?} unit={}",
                    q.symbol, q.name, q.price, q.change.value, q.change.kind, q.unit
                );
            }
        };
        dump("internals", &data.internals);
        dump("macro_levels", &data.macro_levels);

        // Each group must resolve in full, so a renamed / discontinued series fails the
        // smoke loudly rather than thinning the baseline unnoticed.
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

        // The count asserts prove each series resolves, but a frozen series still
        // resolves to a stale value; re-fetch each (FRED is 120 req/min with no daily
        // cap) and assert its latest numeric observation is fresh for its cadence.
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

        // The calendar has no completeness floor, so assert it resolved something and
        // that every entry is well-formed; a strict per-id assert would be flaky for the
        // quarterly releases that legitimately have no date in a ~month window.
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

        // Per-id validation, two failure modes:
        //  (1) a wrong-but-valid id points at a different real release, and `release/dates`
        //      echoes whatever id is queried, so verify each by name against the catalog;
        //  (2) a retired id resolves to no dates, which only a wide window can show (a
        //      quarterly release legitimately has none in the ±~month window).
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
        let catalog: Catalog = serde_json::from_str(&catalog_body).expect("releases catalog shape");
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
        // Reports headroom (bound − staleness) per series, tightest first, so the
        // `max_staleness_days` values are re-tuned from real lag; it only reports.
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
