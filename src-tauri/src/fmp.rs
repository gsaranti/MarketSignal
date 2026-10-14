//! Financial Modeling Prep adapter: the equity half of the baseline scan behind
//! `MarketDataSource`, the economic-calendar consensus feed, and the Portfolio
//! per-holding surface (`docs/data-sources.md`). Every failure degrades to a
//! recorded gap ([`Disposition`]) so one flaky symbol never throws away a scan;
//! the key rides as a query param, never an `Authorization` header.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{
    emit_series_row, BaselineMarketData, Change, DataGap, DealActivity, EarningsEvent, GapReason,
    GroupKind, IndexPerformance, IndustrySnapshot, IpoActivity, MarketDataSource,
    MarketRiskPremium, MoverCategory, PrimaryMarketActivity, Quote, SectorPe, SectorPerformance,
    StockMover,
};
use crate::cadence::ReportCadence;
use crate::econ_calendar::{self, CalendarRow, EconomicCalendarSource};
use crate::group_history::{self, DatedPoint, NotDerivable};
use crate::portfolio::evidence::{
    Chamber, CompanyEvidence, CongressionalTrade, DividendRow, EnterpriseValueRow, GradesConsensus,
    InsiderStatistics, InsiderTrade, MaDeal, MaWalk, MaWalkEnd, OwnerEarningsRow,
    PriceTargetConsensus, PriceTargetTrend, PriceTargetWindow, RatingAction, RatingsSnapshot,
    RatioLines, SegmentYear, SharesFloat, SplitRow, CONGRESSIONAL_TRADES_COUNT, DIVIDENDS_SHOWN,
    INSIDER_TRADES_COUNT, MA_PAGE_CAP, RATING_ACTIONS_COUNT, SEGMENT_FISCAL_YEARS,
};
use crate::primary_market::{self, FrothWindows, IpoRow, IpoStatus};
use crate::progress::RunContext;

/// Base URL for FMP's stable API; [`FmpDataSource::with_base_url`] redirects it at a
/// mock in tests.
const FMP_BASE: &str = "https://financialmodelingprep.com/stable";

const FMP_QUOTE_PATH: &str = "/quote";
/// Requires a `date` query param (a dateless call returns HTTP 400).
const FMP_SECTOR_PATH: &str = "/sector-performance-snapshot";

/// Per-request timeout; the scan issues many sequential calls, so none may park
/// for the model adapter's 120s ceiling.
const FMP_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// FMP's retry policy, shared by every FMP client (`fmp_news` included). The paid
/// plan's per-minute limit arrives as an HTTP 429 (`"Limit Reach"` body) with no
/// `Retry-After`, so the rate-limit ladder (1s doubling to 32s, 63s cumulative) is
/// sized to cross any minute window (pinned by test); 5xx keeps a short schedule so
/// a down provider fails fast. A server `Retry-After` is honored up to 90s.
pub(crate) const FMP_RETRY: crate::http_retry::RetryPolicy = crate::http_retry::RetryPolicy {
    rate_limit: crate::http_retry::Schedule {
        attempts: 7,
        base: StdDuration::from_secs(1),
    },
    server_error: crate::http_retry::Schedule {
        attempts: 3,
        base: StdDuration::from_secs(1),
    },
    retry_after_cap: StdDuration::from_secs(90),
};

/// Weekday candidates probed back for the latest sector snapshot. `sector_candidate_dates`
/// skips weekends without a request, so the budget covers holidays rather than being
/// burned on a guaranteed-empty Saturday or Sunday.
pub(crate) const SECTOR_LOOKBACK_WEEKDAYS: usize = 5;

/// End-of-day history (light: date + close), free-tier; one call per index backs the
/// multi-horizon `IndexPerformance`.
const FMP_EOD_PATH: &str = "/historical-price-eod/light";

/// ~53 weeks, so the 52-week range and the year-to-date anchor both sit inside the
/// window with margin.
const EOD_LOOKBACK_DAYS: i64 = 371;

/// Free mover lists. NB the most-active path is **plural** (`most-actives`); the
/// singular `most-active` 404s.
const FMP_GAINERS_PATH: &str = "/biggest-gainers";
const FMP_LOSERS_PATH: &str = "/biggest-losers";
const FMP_MOST_ACTIVE_PATH: &str = "/most-actives";

/// Free on a ~1-month history window; forward dates return estimates with null actuals.
const FMP_EARNINGS_PATH: &str = "/earnings-calendar";
/// Paid; the primary-market read counts its IPOs from it.
const FMP_IPOS_CALENDAR_PATH: &str = "/ipos-calendar";

/// Paid (a free key replies 402); the `econ_calendar` consensus feed.
const FMP_ECONOMIC_CALENDAR_PATH: &str = "/economic-calendar";

/// Free valuation and finer-rotation snapshots, all date-keyed (a dateless call returns
/// HTTP 400). The two `industry-*` snapshots are joined by industry name.
const FMP_SECTOR_PE_PATH: &str = "/sector-pe-snapshot";
const FMP_INDUSTRY_PERF_PATH: &str = "/industry-performance-snapshot";
const FMP_INDUSTRY_PE_PATH: &str = "/industry-pe-snapshot";

/// Paid trailing-history endpoints behind `group_history` (`docs/data-sources.md
/// §Report enrichment`), each keyed by one sector or industry on one exchange with
/// `from` / `to`; one row per trading day, newest first. The sector P/E history shares
/// [`FMP_HISTORICAL_SECTOR_PE_PATH`] with the Portfolio fund path.
const FMP_HISTORICAL_INDUSTRY_PE_PATH: &str = "/historical-industry-pe";
const FMP_HISTORICAL_SECTOR_PERF_PATH: &str = "/historical-sector-performance";
const FMP_HISTORICAL_INDUSTRY_PERF_PATH: &str = "/historical-industry-performance";

/// Free; Damodaran's per-country equity-risk-premium dataset (no params), a near-static
/// annual constant.
const FMP_RISK_PREMIUM_PATH: &str = "/market-risk-premium";

/// FMP's sector / industry snapshots are exchange-specific (a no-`exchange` call
/// defaults to NASDAQ only). Both major boards are pinned so the model sees the
/// growth-tilted NASDAQ read and the value-weighted NYSE read rather than treating one
/// exchange's valuation as whole-market; each call is pinned to a single exchange, so
/// the per-industry performance↔P/E join is always within one exchange.
const SNAPSHOT_EXCHANGES: &[&str] = &["NASDAQ", "NYSE"];

/// The strongest and weakest industries kept by average move, per exchange (FMP reports
/// ~130), so the finer-rotation read surfaces the extremes without the flat middle.
const INDUSTRY_TOP_N: usize = 10;

/// The plausible-aggregate ceiling for an industry P/E. FMP's aggregate P/E divides an
/// industry's summed price by its summed earnings, so an industry at an earnings trough
/// reports an absurd multiple (≈461 has been seen) that reads as "expensive" when it is
/// denominator noise; above the ceiling the figure is withheld (`None`), the symmetric
/// upper bound to the non-positive drop in [`industry_pe_map_from_value`]. Calibrated by
/// `tuning_industry_pe_distribution_probe`: the plausible band runs up to ~94 and the
/// artifact cluster begins ≥128, so 120 keeps the borderline 100–106 cyclical-trough
/// aggregates while dropping the artifacts. Re-run the probe to re-tune.
const INDUSTRY_PE_MAX: f64 = 120.0;

/// The plausible-aggregate ceiling for a sector P/E, the same stance as
/// [`INDUSTRY_PE_MAX`] and the symmetric upper bound to the non-positive drop in
/// [`sector_pe_from_value`]. The non-positive case is more reachable at the sector cut
/// (FMP reports `pe: 0.0` for net-negative aggregate earnings), while summing over more
/// constituents makes the artifact tail rarer and the plausible band tighter. Calibrated
/// by `tuning_sector_pe_distribution_probe`: the highest plausible aggregate sat at 85.2
/// with no artifact cluster, so 100 leaves headroom for a genuine trough multiple while
/// a hundreds-magnitude artifact stays dropped. Re-run the probe to re-tune.
const SECTOR_PE_MAX: f64 = 100.0;

/// Exact-match, not a substring — "United Kingdom" and "United Arab Emirates" also
/// start with "United".
const RISK_PREMIUM_COUNTRY: &str = "United States";

/// Mover-list filters: the raw lists are dominated by sub-$1 micro-caps, so keep names
/// at or above the price floor on a major US exchange, then the top N in FMP's own
/// ranking order (gainers/losers by percent move, most-actives by volume).
const MOVER_MIN_PRICE: f64 = 5.0;
const MOVER_TOP_N: usize = 10;
const MOVER_EXCHANGES: &[&str] = &["NASDAQ", "NYSE", "AMEX"];

/// Case-insensitive name fragments marking a mover row as a fund / ETF / ETN or a
/// leveraged-inverse product. The free mover lists carry no fund flag, so this name
/// heuristic is the only signal; it errs toward false negatives (the prompt's "a mover
/// may be a fund" caveat backs it) because dropping a real company is the worse error.
/// `" etf"` / `" etn"` carry a leading space to match the suffix, not a substring of a
/// company name ("Aetna"); bare `"bull"` / `"bear"` are deliberately not markers (they
/// would drop "Build-A-Bear Workshop" for no added coverage).
const MOVER_FUND_MARKERS: &[&str] = &[
    " etf",
    " etn",
    " fund",
    "2x",
    "3x",
    "leveraged",
    "inverse",
    "ultrapro",
    "ultrashort",
    "daily target",
    "proshares",
    "direxion",
    "graniteshares",
    "microsectors",
];

/// Earnings-calendar window and filter. The back window scales with the run cadence
/// ([`cadence_window_days`]) between the floor and the cap (FMP's free earnings window is
/// ~1 month); the forward window stays fixed. No free index-membership list exists, so
/// the quarterly revenue estimate is the large-cap proxy.
const EARNINGS_BACK_DAYS: i64 = 7;
const EARNINGS_BACK_MAX_DAYS: i64 = 31;
const EARNINGS_FWD_DAYS: i64 = 14;
const EARNINGS_MIN_REVENUE: f64 = 5_000_000_000.0;
const EARNINGS_MAX_ROWS: usize = 25;

/// The cadence-sized lookback in whole days: the floor on the first report, else the
/// elapsed interval rounded up and clamped. The saturating `f64 as i64` cast makes a
/// non-finite interval clamp rather than panic.
fn cadence_window_days(elapsed_days: Option<f64>) -> i64 {
    match elapsed_days {
        None => EARNINGS_BACK_DAYS,
        Some(d) => (d.ceil() as i64).clamp(EARNINGS_BACK_DAYS, EARNINGS_BACK_MAX_DAYS),
    }
}

/// The headline indices (`docs/report-workflow.md §Step 3`) with a fallback display
/// name and the `price` unit; FMP's quote object carries no unit, so it rides from the
/// table.
const INDEX_SYMBOLS: &[(&str, &str, &str)] = &[
    ("^DJI", "Dow Jones Industrial Average", "index points"),
    ("^GSPC", "S&P 500", "index points"),
    ("^IXIC", "Nasdaq Composite", "index points"),
    ("^RUT", "Russell 2000", "index points"),
];

/// The free-tier internals with their `price` unit; the dollar index, oil and natural
/// gas are FMP-premium and come from FRED instead.
const INTERNAL_SYMBOLS: &[(&str, &str, &str)] = &[
    ("^VIX", "CBOE Volatility Index", "index points"),
    ("GCUSD", "Gold", "USD per troy ounce"),
    ("SIUSD", "Silver", "USD per troy ounce"),
];

/// `price` and the percent change are required so a quote missing either fails the parse
/// (a `Malformed` gap) rather than reaching the model as a false `0.0`.
#[derive(Debug, Deserialize)]
struct FmpQuoteRaw {
    symbol: String,
    #[serde(default)]
    name: String,
    price: f64,
    #[serde(rename = "changePercentage", alias = "changesPercentage")]
    change_pct: f64,
}

/// Both fields are required so a short row fails the parse (a `Malformed` gap) rather
/// than dropping silently.
#[derive(Debug, Deserialize)]
struct FmpSectorRaw {
    sector: String,
    #[serde(rename = "averageChange")]
    average_change: f64,
}

/// The close (`price`) on a `date` (`"YYYY-MM-DD"`); both required so a short row fails
/// the parse rather than dropping silently.
#[derive(Debug, Deserialize)]
struct FmpEodRaw {
    date: String,
    price: f64,
}

/// The percent change is `changesPercentage` (plural) on the mover lists, the inverse of
/// the quote endpoint's spelling.
#[derive(Debug, Deserialize)]
struct FmpMoverRaw {
    symbol: String,
    #[serde(default)]
    name: String,
    price: f64,
    #[serde(rename = "changesPercentage", alias = "changePercentage")]
    change_pct: f64,
    #[serde(default)]
    exchange: String,
}

/// FMP omits actuals for dates that haven't reported and can omit estimates for
/// thinly-covered names.
#[derive(Debug, Deserialize)]
struct FmpEarningsRaw {
    symbol: String,
    date: String,
    #[serde(rename = "epsEstimated")]
    eps_estimated: Option<f64>,
    #[serde(rename = "epsActual")]
    eps_actual: Option<f64>,
    #[serde(rename = "revenueEstimated")]
    revenue_estimated: Option<f64>,
    #[serde(rename = "revenueActual")]
    revenue_actual: Option<f64>,
}

/// `exchange` is read from the wire, not assumed from the request, so the row is labelled
/// by the board FMP actually reported should the `exchange` query param ever be ignored.
#[derive(Debug, Deserialize)]
struct FmpSectorPeRaw {
    sector: String,
    exchange: String,
    pe: f64,
}

/// `exchange` is read from the wire (see [`FmpSectorPeRaw`]).
#[derive(Debug, Deserialize)]
struct FmpIndustryPerfRaw {
    industry: String,
    exchange: String,
    #[serde(rename = "averageChange")]
    average_change: f64,
}

/// `exchange` is read from the wire (see [`FmpSectorPeRaw`]).
#[derive(Debug, Deserialize)]
struct FmpIndustryPeRaw {
    industry: String,
    exchange: String,
    pe: f64,
}

#[derive(Debug, Deserialize)]
struct FmpRiskPremiumRaw {
    country: String,
    #[serde(rename = "countryRiskPremium")]
    country_risk_premium: f64,
    #[serde(rename = "totalEquityRiskPremium")]
    total_equity_risk_premium: f64,
}

/// One FMP response classified for the loop: a 2xx value to shape, or a gap the loop
/// records and steps past. The degradation policy lives in [`interpret_response`].
enum Disposition {
    Value(Value),
    Gap(GapReason),
}

/// Status decides first, so a non-2xx is never reclassified by its body; only a 2xx
/// body is inspected, where FMP's `{"Error Message"}` rate-limit / plan signal is a
/// `Rejected` gap. An empty "no data" array parses fine and shapes to zero rows.
fn interpret_response(status: u16, body: &str) -> Disposition {
    match status {
        200..=299 => match serde_json::from_str::<Value>(body) {
            Ok(value) => {
                if fmp_error_message(&value).is_some() {
                    Disposition::Gap(GapReason::Rejected) // rate-limit / plan signal
                } else {
                    Disposition::Value(value)
                }
            }
            Err(_) => Disposition::Gap(GapReason::Malformed),
        },
        402 | 404 => Disposition::Gap(GapReason::OutOfScope),
        401 | 403 => Disposition::Gap(GapReason::Rejected),
        429 | 500..=599 => Disposition::Gap(GapReason::Unavailable),
        _ => Disposition::Gap(GapReason::Malformed), // 400/408/422/other request-contract
    }
}

/// FMP's `{"Error Message": …}` rate-limit / plan envelope, single-homed so
/// [`interpret_response`] and [`http_gap_detail`] cannot drift on its shape.
fn fmp_error_message(v: &Value) -> Option<&str> {
    v.get("Error Message").and_then(Value::as_str)
}

/// Cap on the body text a gap detail embeds; FMP error sentences fit, the cap only
/// bites on an HTML error page or similar.
const GAP_DETAIL_BODY_CAP: usize = 300;

/// The sentence an HTTP-level gap leaves on its tracker row: the status plus FMP's own
/// error sentence when the body carries one, else a capped head of the body.
fn http_gap_detail(status: u16, body: &str) -> String {
    let text = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| fmp_error_message(&v).map(str::to_owned))
        .unwrap_or_else(|| body.trim().to_owned());
    if text.is_empty() {
        return format!("HTTP {status} (empty body)");
    }
    let (head, cut) = crate::data_sources::cap_chars(&text, GAP_DETAIL_BODY_CAP);
    if cut {
        format!("HTTP {status}: {head} …(truncated)")
    } else {
        format!("HTTP {status}: {head}")
    }
}

/// The `sectors` group is a whole snapshot, so its gap carries a synthetic series id
/// rather than one per sector.
fn sector_gap(reason: GapReason) -> DataGap {
    DataGap::new(
        GroupKind::Sectors,
        "sector-performance",
        "Sector Performance",
        reason,
    )
}

/// Exchange-tagged like the other snapshot gaps, so a NASDAQ failure and an NYSE
/// failure are distinct manifest entries.
fn sector_pe_gap(exchange: &str, reason: GapReason) -> DataGap {
    DataGap::new(
        GroupKind::SectorPe,
        format!("sector-pe-{}", exchange.to_ascii_lowercase()),
        format!("Sector P/E ({exchange})"),
        reason,
    )
}

fn industry_perf_gap(exchange: &str, reason: GapReason) -> DataGap {
    DataGap::new(
        GroupKind::Industries,
        format!("industry-performance-{}", exchange.to_ascii_lowercase()),
        format!("Industry Performance ({exchange})"),
        reason,
    )
}

fn industry_pe_gap(exchange: &str, reason: GapReason) -> DataGap {
    DataGap::new(
        GroupKind::Industries,
        format!("industry-pe-{}", exchange.to_ascii_lowercase()),
        format!("Industry P/E ({exchange})"),
        reason,
    )
}

/// A single-symbol `/quote` call returns a one-element array; the `unit` is stamped from
/// the request because FMP's quote object carries none.
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
            change: Change::percent(r.change_pct),
            unit: unit.to_string(),
        })
        .collect())
}

/// Deduplicated by sector name: a per-exchange variant of the call could repeat sectors.
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

fn movers_from_value(value: Value, category: MoverCategory) -> Result<Vec<StockMover>> {
    let raws: Vec<FmpMoverRaw> = serde_json::from_value(value)
        .context("FMP mover response did not match the expected array shape")?;
    Ok(raws
        .into_iter()
        .map(|r| StockMover {
            category,
            name: if r.name.trim().is_empty() {
                r.symbol.clone()
            } else {
                r.name
            },
            symbol: r.symbol,
            price: r.price,
            change_pct: r.change_pct,
            exchange: r.exchange,
        })
        .collect())
}

fn is_fund_or_leveraged(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    MOVER_FUND_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

/// The cap takes the first N in arrival order, which is FMP's own ranking.
fn filter_movers(movers: Vec<StockMover>) -> Vec<StockMover> {
    movers
        .into_iter()
        .filter(|m| {
            m.price >= MOVER_MIN_PRICE
                && MOVER_EXCHANGES.contains(&m.exchange.as_str())
                && !is_fund_or_leveraged(&m.name)
        })
        .take(MOVER_TOP_N)
        .collect()
}

fn earnings_from_value(value: Value) -> Result<Vec<EarningsEvent>> {
    let raws: Vec<FmpEarningsRaw> = serde_json::from_value(value)
        .context("FMP earnings response did not match the expected array shape")?;
    Ok(raws
        .into_iter()
        .map(|r| EarningsEvent {
            symbol: r.symbol,
            date: r.date,
            eps_estimated: r.eps_estimated,
            eps_actual: r.eps_actual,
            revenue_estimated: r.revenue_estimated,
            revenue_actual: r.revenue_actual,
        })
        .collect())
}

/// Rows without a revenue estimate are dropped: they cannot clear the floor and are
/// overwhelmingly thinly-covered small-caps.
fn filter_earnings(events: Vec<EarningsEvent>) -> Vec<EarningsEvent> {
    let mut kept: Vec<EarningsEvent> = events
        .into_iter()
        .filter(|e| {
            e.revenue_estimated
                .is_some_and(|r| r >= EARNINGS_MIN_REVENUE)
        })
        .collect();
    kept.sort_by(|a, b| {
        b.revenue_estimated
            .partial_cmp(&a.revenue_estimated)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    kept.truncate(EARNINGS_MAX_ROWS);
    kept
}

/// A single off-board row fails the whole leg (a `Malformed` gap) rather than silently
/// accepting it: were FMP to ignore the `exchange` param, one board would otherwise be
/// duplicated and the other dropped with no gap. A `pe` outside `(0.0, SECTOR_PE_MAX]`
/// is withheld as `None` while the row survives, so the model still sees the sector was
/// scanned.
fn sector_pe_from_value(value: Value, expected_exchange: &str) -> Result<Vec<SectorPe>> {
    let raws: Vec<FmpSectorPeRaw> = serde_json::from_value(value)
        .context("FMP sector-PE response did not match the expected array shape")?;
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(raws.len());
    for raw in raws {
        if raw.exchange != expected_exchange {
            anyhow::bail!(
                "FMP sector-PE returned exchange {:?} for an {expected_exchange:?} request — \
                 the exchange filter was ignored",
                raw.exchange
            );
        }
        if seen.insert((raw.sector.clone(), raw.exchange.clone())) {
            let pe = (raw.pe > 0.0 && raw.pe <= SECTOR_PE_MAX).then_some(raw.pe);
            out.push(SectorPe {
                sector: raw.sector,
                exchange: raw.exchange,
                pe,
                pe_history: None,
                trailing_return: None,
            });
        }
    }
    Ok(out)
}

/// `(industry, exchange, average_change)` rows, keep-first in arrival order; the same
/// off-board guard as [`sector_pe_from_value`].
fn industry_perf_from_value(
    value: Value,
    expected_exchange: &str,
) -> Result<Vec<(String, String, f64)>> {
    let raws: Vec<FmpIndustryPerfRaw> = serde_json::from_value(value)
        .context("FMP industry-performance response did not match the expected array shape")?;
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(raws.len());
    for raw in raws {
        if raw.exchange != expected_exchange {
            anyhow::bail!(
                "FMP industry-performance returned exchange {:?} for an {expected_exchange:?} \
                 request — the exchange filter was ignored",
                raw.exchange
            );
        }
        if seen.insert((raw.industry.clone(), raw.exchange.clone())) {
            out.push((raw.industry, raw.exchange, raw.average_change));
        }
    }
    Ok(out)
}

/// Keyed by (industry, exchange) so the performance↔P/E join can only pair same-board
/// figures; the same off-board guard as [`sector_pe_from_value`]. FMP reports `pe: 0.0`
/// (not null) for an industry with no positive aggregate earnings, so a ratio outside
/// `(0.0, INDUSTRY_PE_MAX]` is left out of the map and joins to `None`.
fn industry_pe_map_from_value(
    value: Value,
    expected_exchange: &str,
) -> Result<HashMap<(String, String), f64>> {
    let raws: Vec<FmpIndustryPeRaw> = serde_json::from_value(value)
        .context("FMP industry-PE response did not match the expected array shape")?;
    let mut map = HashMap::with_capacity(raws.len());
    for raw in raws {
        if raw.exchange != expected_exchange {
            anyhow::bail!(
                "FMP industry-PE returned exchange {:?} for an {expected_exchange:?} request — \
                 the exchange filter was ignored",
                raw.exchange
            );
        }
        if raw.pe > 0.0 && raw.pe <= INDUSTRY_PE_MAX {
            map.entry((raw.industry, raw.exchange)).or_insert(raw.pe);
        }
    }
    Ok(map)
}

/// A missing P/E degrades the row to `pe: None`, never drops it. The bottom count is
/// clamped to what's left after the top, so a short list yields each industry once.
fn top_bottom_industries(
    perf: Vec<(String, String, f64)>,
    pe: &HashMap<(String, String), f64>,
) -> Vec<IndustrySnapshot> {
    let mut sorted = perf;
    sorted.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let take_top = INDUSTRY_TOP_N.min(sorted.len());
    let take_bottom = INDUSTRY_TOP_N.min(sorted.len() - take_top);
    let mut chosen: Vec<(String, String, f64)> = Vec::with_capacity(take_top + take_bottom);
    chosen.extend_from_slice(&sorted[..take_top]);
    chosen.extend_from_slice(&sorted[sorted.len() - take_bottom..]);
    chosen
        .into_iter()
        .map(|(industry, exchange, change_pct)| {
            let pe = pe.get(&(industry.clone(), exchange.clone())).copied();
            IndustrySnapshot {
                industry,
                exchange,
                change_pct,
                pe,
                pe_history: None,
                trailing_return: None,
            }
        })
        .collect()
}

/// The trailing-history series behind `group_history`, each fetched per group on one
/// exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryFamily {
    SectorPe,
    SectorReturn,
    IndustryPe,
    IndustryReturn,
}

impl HistoryFamily {
    fn path(self) -> &'static str {
        match self {
            HistoryFamily::SectorPe => FMP_HISTORICAL_SECTOR_PE_PATH,
            HistoryFamily::SectorReturn => FMP_HISTORICAL_SECTOR_PERF_PATH,
            HistoryFamily::IndustryPe => FMP_HISTORICAL_INDUSTRY_PE_PATH,
            HistoryFamily::IndustryReturn => FMP_HISTORICAL_INDUSTRY_PERF_PATH,
        }
    }

    /// The query parameter and row field naming the group.
    fn name_key(self) -> &'static str {
        match self {
            HistoryFamily::SectorPe | HistoryFamily::SectorReturn => "sector",
            HistoryFamily::IndustryPe | HistoryFamily::IndustryReturn => "industry",
        }
    }

    fn group(self) -> GroupKind {
        match self {
            HistoryFamily::SectorPe | HistoryFamily::SectorReturn => GroupKind::SectorPe,
            HistoryFamily::IndustryPe | HistoryFamily::IndustryReturn => GroupKind::Industries,
        }
    }

    /// The series-id stem of the family's tracker rows and gaps.
    fn slug(self) -> &'static str {
        match self {
            HistoryFamily::SectorPe => "sector-pe-history",
            HistoryFamily::SectorReturn => "sector-return-history",
            HistoryFamily::IndustryPe => "industry-pe-history",
            HistoryFamily::IndustryReturn => "industry-return-history",
        }
    }

    /// For the family-level gap.
    fn label(self) -> &'static str {
        match self {
            HistoryFamily::SectorPe => "Sector P/E history",
            HistoryFamily::SectorReturn => "Sector return history",
            HistoryFamily::IndustryPe => "Industry P/E history",
            HistoryFamily::IndustryReturn => "Industry return history",
        }
    }

    /// For the per-group tracker row and row-level gap.
    fn read_label(self) -> &'static str {
        match self {
            HistoryFamily::SectorPe | HistoryFamily::IndustryPe => "P/E history",
            HistoryFamily::SectorReturn | HistoryFamily::IndustryReturn => "return history",
        }
    }
}

/// The fetch window and the ET session date the freshness check reads, shared by every
/// history fetch in one scan.
#[derive(Debug, Clone, Copy)]
struct HistoryClock {
    from: NaiveDate,
    to: NaiveDate,
    session: NaiveDate,
}

impl HistoryClock {
    fn at(now: chrono::DateTime<Utc>) -> Self {
        // The fetch range's upper bound deliberately stays UTC (a fetch bound, not a
        // session key); the freshness check reads the ET session (`market_clock`).
        let (from, to) = group_history::window(now.date_naive());
        Self {
            from,
            to,
            session: crate::market_clock::et_session_date(now),
        }
    }
}

/// A group name as a series-id token: lowercase ASCII alphanumerics, every other run `-`
/// (`Oil & Gas Midstream` → `oil-gas-midstream`).
fn series_token(name: &str) -> String {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

/// A whole-provider failure stops the family's loop, so the reads it did not reach share
/// this one manifest entry.
fn history_family_gap(family: HistoryFamily, exchange: &str, reason: GapReason) -> DataGap {
    DataGap::new(
        family.group(),
        format!("{}-{}", family.slug(), exchange.to_ascii_lowercase()),
        format!("{} ({exchange})", family.label()),
        reason,
    )
}

/// A row served under another exchange, or none, fails the whole body; a row for another
/// group, or without a datable date or usable value, is dropped. A daily `averageChange`
/// is usable when finite (the ±25-point guard is the derivation's).
fn history_points_from_value(
    value: &Value,
    family: HistoryFamily,
    name: &str,
    exchange: &str,
) -> Result<Vec<DatedPoint>> {
    let Some(rows) = value.as_array() else {
        anyhow::bail!("FMP {} response is not an array", family.path());
    };
    let parse_date = |date: &str| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok();
    if family == HistoryFamily::SectorPe {
        return Ok(sector_pe_rows_from_value(value, exchange)?
            .into_iter()
            .filter(|row| row.sector == name)
            .filter_map(|row| {
                Some(DatedPoint {
                    date: parse_date(&row.date)?,
                    value: row.pe,
                })
            })
            .collect());
    }
    let value_key = match family {
        HistoryFamily::IndustryPe => "pe",
        _ => "averageChange",
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let served = row.get("exchange").and_then(Value::as_str);
        if served != Some(exchange) {
            anyhow::bail!(
                "FMP {} returned exchange {} for an {exchange:?} request — the exchange filter \
                 was ignored",
                family.path(),
                served.map_or_else(|| "<absent>".to_string(), |s| format!("{s:?}"))
            );
        }
        if row.get(family.name_key()).and_then(Value::as_str) != Some(name) {
            continue;
        }
        let Some(date) = row.get("date").and_then(Value::as_str).and_then(parse_date) else {
            continue;
        };
        let reading = match row.get(value_key) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
            _ => None,
        };
        let usable = |v: &f64| match family {
            HistoryFamily::IndustryPe => v.is_finite() && *v > 0.0 && *v <= INDUSTRY_PE_MAX,
            _ => v.is_finite(),
        };
        let Some(reading) = reading.filter(usable) else {
            continue;
        };
        out.push(DatedPoint {
            date,
            value: reading,
        });
    }
    Ok(out)
}

/// Zero or one row in practice.
fn risk_premium_from_value(value: Value) -> Result<Vec<MarketRiskPremium>> {
    let raws: Vec<FmpRiskPremiumRaw> = serde_json::from_value(value)
        .context("FMP market-risk-premium response did not match the expected array shape")?;
    Ok(raws
        .into_iter()
        .filter(|r| r.country == RISK_PREMIUM_COUNTRY)
        .map(|r| MarketRiskPremium {
            country: r.country,
            country_risk_premium: r.country_risk_premium,
            total_equity_risk_premium: r.total_equity_risk_premium,
        })
        .collect())
}

/// The most recent weekday on or before `today`, then each prior weekday; FMP publishes
/// no weekend snapshot, so skipping Sat/Sun spends no request on them.
pub(crate) fn sector_candidate_dates(today: NaiveDate, lookback: usize) -> Vec<NaiveDate> {
    let mut out = Vec::with_capacity(lookback);
    let mut date = today;
    while out.len() < lookback {
        if !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
            out.push(date);
        }
        date -= Duration::days(1);
    }
    out
}

/// `rows` are newest-first. Each horizon's baseline is the most recent close on or
/// before the horizon's start (an "as-of" lookup), so weekends and holidays don't skew
/// the anchor, and everything anchors to the latest close's own date, not the wall-clock
/// run date. `None` when the history cannot anchor even the weekly return.
fn index_performance_from_eod(
    symbol: &str,
    name: &str,
    rows: &[(NaiveDate, f64)],
) -> Option<IndexPerformance> {
    let (latest_date, latest) = rows.first().copied()?;
    let as_of = |target: NaiveDate| rows.iter().find(|(d, _)| *d <= target).map(|(_, p)| *p);
    let pct = |base: f64| {
        if base != 0.0 {
            (latest - base) / base * 100.0
        } else {
            0.0
        }
    };

    let weekly_pct = pct(as_of(latest_date - Duration::days(7))?);
    // MTD / YTD soft-degrade to 0.0 when the window doesn't reach the anchor (a fresh
    // listing, or a short fetch) rather than dropping the whole index.
    let first_of_month = latest_date.with_day(1)?;
    let mtd_pct = as_of(first_of_month - Duration::days(1))
        .map(pct)
        .unwrap_or(0.0);
    let first_of_year = NaiveDate::from_ymd_opt(latest_date.year(), 1, 1)?;
    let ytd_pct = as_of(first_of_year - Duration::days(1))
        .map(pct)
        .unwrap_or(0.0);

    let cutoff = latest_date - Duration::days(365);
    let mut low_52w = latest;
    let mut high_52w = latest;
    for (_, p) in rows.iter().filter(|(d, _)| *d >= cutoff) {
        if *p < low_52w {
            low_52w = *p;
        }
        if *p > high_52w {
            high_52w = *p;
        }
    }

    Some(IndexPerformance {
        symbol: symbol.to_string(),
        name: name.to_string(),
        weekly_pct,
        mtd_pct,
        ytd_pct,
        low_52w,
        high_52w,
        pct_from_52w_high: pct(high_52w),
    })
}

/// Sorts newest-first defensively: FMP returns descending, but the anchors must not
/// depend on it.
fn eod_to_performance(value: Value, symbol: &str, name: &str) -> Result<Option<IndexPerformance>> {
    let raws: Vec<FmpEodRaw> = serde_json::from_value(value)
        .context("FMP EOD response did not match the expected array shape")?;
    let mut rows: Vec<(NaiveDate, f64)> = Vec::with_capacity(raws.len());
    for r in raws {
        let date = NaiveDate::parse_from_str(r.date.trim(), "%Y-%m-%d").with_context(|| {
            format!(
                "FMP EOD returned an unparseable date {:?} for {symbol}",
                r.date
            )
        })?;
        rows.push((date, r.price));
    }
    rows.sort_by_key(|b| std::cmp::Reverse(b.0));
    Ok(index_performance_from_eod(symbol, name, &rows))
}

pub struct FmpDataSource {
    api_key: String,
    http: reqwest::blocking::Client,
    base_url: String,
    /// Live progress and cooperative cancellation; a no-op until
    /// [`FmpDataSource::with_context`] attaches the real one.
    progress: Arc<RunContext>,
    retry: crate::http_retry::RetryPolicy,
}

impl FmpDataSource {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(FMP_TIMEOUT)
            .build()
            .context("building the FMP HTTP client")?;
        Ok(Self {
            api_key,
            http,
            base_url: FMP_BASE.to_string(),
            progress: RunContext::noop(),
            retry: FMP_RETRY,
        })
    }

    /// A trailing slash is trimmed so the joined path's leading slash doesn't double up.
    #[cfg(test)]
    pub(crate) fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// A millisecond-scale policy so the wiring test doesn't sleep the real ladder.
    #[cfg(test)]
    fn with_retry_policy(mut self, retry: crate::http_retry::RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// The execution gate (`config::validate`) runs ahead of this in the command path.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().fmp_key()?)
    }

    /// The key rides as a query param. A transport error is `Err`, which callers record
    /// as an `Unavailable` gap. The run's cancel flag is the retry abort signal, so a
    /// cancel mid-backoff stops retrying within ~250ms.
    fn get(&self, path: &str, extra: &[(&str, &str)]) -> Result<(u16, String)> {
        let mut query: Vec<(&str, &str)> = vec![("apikey", self.api_key.as_str())];
        query.extend_from_slice(extra);
        let url = format!("{}{path}", self.base_url);
        let abort = || self.progress.is_cancelled();
        crate::http_retry::send_with_retry_policy("FMP", &self.retry, Some(&abort), || {
            self.http.get(&url).query(&query)
        })
    }

    /// A `Rejected` (auth / quota) is a whole-provider condition, so the loop stops
    /// calling and records the remaining symbols. An empty "no data" array for an
    /// expected symbol is an `Unavailable` gap, so it still counts against coverage.
    fn fetch_quotes(
        &self,
        symbols: &[(&str, &str, &str)],
        group: GroupKind,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<Quote> {
        let mut out = Vec::with_capacity(symbols.len());
        let mut rejected = false;
        for (symbol, fallback_name, unit) in symbols {
            // The series already fetched are kept; the run unwinds at the pipeline's
            // post-baseline checkpoint.
            if self.progress.is_cancelled() {
                break;
            }
            if rejected {
                // No tracker row for a short-circuited series: rows stay one-to-one
                // with actual HTTP calls.
                gaps.push(DataGap::new(
                    group,
                    *symbol,
                    *fallback_name,
                    GapReason::Rejected,
                ));
                continue;
            }
            self.progress
                .request_started("FMP", group.as_str(), *symbol, *fallback_name);
            let gaps_before = gaps.len();
            let out_before = out.len();
            let (disposition, gap_detail) =
                self.get_interpreted(FMP_QUOTE_PATH, &[("symbol", symbol)]);
            match disposition {
                Disposition::Value(value) => match quotes_from_value(value, fallback_name, unit) {
                    Ok(quotes) if quotes.is_empty() => gaps.push(DataGap::new(
                        group,
                        *symbol,
                        *fallback_name,
                        GapReason::Unavailable,
                    )),
                    Ok(quotes) => out.extend(quotes),
                    Err(_) => gaps.push(DataGap::new(
                        group,
                        *symbol,
                        *fallback_name,
                        GapReason::Malformed,
                    )),
                },
                Disposition::Gap(reason) => {
                    if reason == GapReason::Rejected {
                        rejected = true;
                    }
                    gaps.push(DataGap::new(group, *symbol, *fallback_name, reason));
                }
            }
            emit_series_row(
                &self.progress,
                "FMP",
                group,
                symbol,
                fallback_name,
                gaps,
                gaps_before,
                out.len() > out_before,
                gap_detail,
            );
        }
        out
    }

    /// Walks back over weekday candidates to the last trading day with data: a 404 or
    /// empty array means no snapshot for that date, a this-run failure records one
    /// group-level gap and stops. No candidate with a snapshot returns empty with no gap
    /// (a quiet window, not a failure).
    fn fetch_sectors(&self, gaps: &mut Vec<DataGap>) -> Vec<SectorPerformance> {
        // The ET session, not the UTC date: the snapshots are keyed by trading day, and
        // an evening-ET run under a UTC date would spend its first candidate on a
        // session that has not traded (`market_clock`).
        let today = crate::market_clock::et_session_date(Utc::now());
        for date in sector_candidate_dates(today, SECTOR_LOOKBACK_WEEKDAYS) {
            if self.progress.is_cancelled() {
                return Vec::new();
            }
            // Each date probe is a real HTTP request, so each gets its own tracker row.
            let date_str = date.format("%Y-%m-%d").to_string();
            let name = format!("Sector performance ({date_str})");
            let group = GroupKind::Sectors.as_str();
            self.progress
                .request_started("FMP", group, date_str.as_str(), name.as_str());
            let disposition = match self.get(FMP_SECTOR_PATH, &[("date", date_str.as_str())]) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            let finish = |status: &str| {
                self.progress.request_finished(
                    "FMP",
                    group,
                    date_str.as_str(),
                    name.as_str(),
                    status,
                    None,
                )
            };
            match disposition {
                Disposition::Value(value) => match sectors_from_value(value) {
                    Ok(sectors) if !sectors.is_empty() => {
                        finish("ok");
                        return sectors;
                    }
                    Ok(_) => finish("empty"),
                    Err(_) => {
                        finish("malformed");
                        gaps.push(sector_gap(GapReason::Malformed));
                        return Vec::new();
                    }
                },
                Disposition::Gap(GapReason::OutOfScope) => finish("out-of-scope"),
                Disposition::Gap(reason) => {
                    finish(reason.as_str());
                    gaps.push(sector_gap(reason));
                    return Vec::new();
                }
            }
        }
        Vec::new()
    }

    /// Additive enrichment over the required daily quotes, so a permanent absence
    /// (402 / 404) or a history too short to anchor is skipped silently (the daily quote
    /// covers the symbol, and a recurring premium gap would be noise); a this-run failure
    /// is recorded as a gap, and a `Rejected` stops the loop like the quote groups.
    fn fetch_index_performance(&self, gaps: &mut Vec<DataGap>) -> Vec<IndexPerformance> {
        // Fetch-range upper bound deliberately stays UTC: a forward roll to the ET
        // session would ask for an untraded day and shift the lookback by one. Only
        // session-keyed reads convert (`market_clock`).
        let to = Utc::now().date_naive();
        let from = to - Duration::days(EOD_LOOKBACK_DAYS);
        let (from_s, to_s) = (
            from.format("%Y-%m-%d").to_string(),
            to.format("%Y-%m-%d").to_string(),
        );
        let mut out = Vec::with_capacity(INDEX_SYMBOLS.len());
        let mut rejected = false;
        for &(symbol, name, _) in INDEX_SYMBOLS {
            if self.progress.is_cancelled() {
                break;
            }
            if rejected {
                // No request made for a short-circuited symbol — no tracker row.
                gaps.push(DataGap::new(
                    GroupKind::IndexPerformance,
                    symbol,
                    name,
                    GapReason::Rejected,
                ));
                continue;
            }
            self.progress.request_started(
                "FMP",
                GroupKind::IndexPerformance.as_str(),
                symbol,
                name,
            );
            let gaps_before = gaps.len();
            let out_before = out.len();
            let (disposition, gap_detail) = self.get_interpreted(
                FMP_EOD_PATH,
                &[
                    ("symbol", symbol),
                    ("from", from_s.as_str()),
                    ("to", to_s.as_str()),
                ],
            );
            match disposition {
                Disposition::Value(value) => match eod_to_performance(value, symbol, name) {
                    Ok(Some(perf)) => out.push(perf),
                    Ok(None) => {}
                    Err(_) => gaps.push(DataGap::new(
                        GroupKind::IndexPerformance,
                        symbol,
                        name,
                        GapReason::Malformed,
                    )),
                },
                Disposition::Gap(GapReason::OutOfScope) => {}
                Disposition::Gap(reason) => {
                    if reason == GapReason::Rejected {
                        rejected = true;
                    }
                    gaps.push(DataGap::new(
                        GroupKind::IndexPerformance,
                        symbol,
                        name,
                        reason,
                    ));
                }
            }
            emit_series_row(
                &self.progress,
                "FMP",
                GroupKind::IndexPerformance,
                symbol,
                name,
                gaps,
                gaps_before,
                out.len() > out_before,
                gap_detail,
            );
        }
        out
    }

    /// Additive like `fetch_index_performance`: a permanent absence or an empty /
    /// all-filtered list is silent, a this-run failure records a gap.
    fn fetch_movers(&self, gaps: &mut Vec<DataGap>) -> Vec<StockMover> {
        let endpoints = [
            (
                MoverCategory::Gainer,
                FMP_GAINERS_PATH,
                "biggest-gainers",
                "Biggest Gainers",
            ),
            (
                MoverCategory::Loser,
                FMP_LOSERS_PATH,
                "biggest-losers",
                "Biggest Losers",
            ),
            (
                MoverCategory::MostActive,
                FMP_MOST_ACTIVE_PATH,
                "most-actives",
                "Most Active",
            ),
        ];
        let mut out = Vec::new();
        let mut rejected = false;
        for (category, url, series_id, name) in endpoints {
            if self.progress.is_cancelled() {
                break;
            }
            if rejected {
                // No request made for a short-circuited list — no tracker row.
                gaps.push(DataGap::new(
                    GroupKind::Movers,
                    series_id,
                    name,
                    GapReason::Rejected,
                ));
                continue;
            }
            self.progress
                .request_started("FMP", GroupKind::Movers.as_str(), series_id, name);
            let gaps_before = gaps.len();
            let out_before = out.len();
            let (disposition, gap_detail) = self.get_interpreted(url, &[]);
            match disposition {
                Disposition::Value(value) => match movers_from_value(value, category) {
                    Ok(movers) => out.extend(filter_movers(movers)),
                    Err(_) => gaps.push(DataGap::new(
                        GroupKind::Movers,
                        series_id,
                        name,
                        GapReason::Malformed,
                    )),
                },
                Disposition::Gap(GapReason::OutOfScope) => {}
                Disposition::Gap(reason) => {
                    if reason == GapReason::Rejected {
                        rejected = true;
                    }
                    gaps.push(DataGap::new(GroupKind::Movers, series_id, name, reason));
                }
            }
            emit_series_row(
                &self.progress,
                "FMP",
                GroupKind::Movers,
                series_id,
                name,
                gaps,
                gaps_before,
                out.len() > out_before,
                gap_detail,
            );
        }
        out
    }

    /// Additive like `fetch_movers`: a permanent absence or an empty / all-filtered
    /// window is silent, a this-run failure records one gap.
    fn fetch_earnings(&self, back_days: i64, gaps: &mut Vec<DataGap>) -> Vec<EarningsEvent> {
        if self.progress.is_cancelled() {
            return Vec::new();
        }
        // The ET session: both bounds are trading-day dates, so a UTC date on an evening
        // run slides the window forward a day and drops an event on its trailing edge.
        let today = crate::market_clock::et_session_date(Utc::now());
        let from = (today - Duration::days(back_days))
            .format("%Y-%m-%d")
            .to_string();
        let to = (today + Duration::days(EARNINGS_FWD_DAYS))
            .format("%Y-%m-%d")
            .to_string();
        let series_id = "earnings-calendar";
        let name = "Earnings Calendar";
        self.progress
            .request_started("FMP", GroupKind::Earnings.as_str(), series_id, name);
        let gaps_before = gaps.len();
        let (disposition, gap_detail) = self.get_interpreted(
            FMP_EARNINGS_PATH,
            &[("from", from.as_str()), ("to", to.as_str())],
        );
        let out = match disposition {
            Disposition::Value(value) => match earnings_from_value(value) {
                Ok(events) => filter_earnings(events),
                Err(_) => {
                    gaps.push(DataGap::new(
                        GroupKind::Earnings,
                        series_id,
                        name,
                        GapReason::Malformed,
                    ));
                    Vec::new()
                }
            },
            Disposition::Gap(GapReason::OutOfScope) => Vec::new(),
            Disposition::Gap(reason) => {
                gaps.push(DataGap::new(GroupKind::Earnings, series_id, name, reason));
                Vec::new()
            }
        };
        emit_series_row(
            &self.progress,
            "FMP",
            GroupKind::Earnings,
            series_id,
            name,
            gaps,
            gaps_before,
            !out.is_empty(),
            gap_detail,
        );
        out
    }

    /// The primary-market read (`docs/data-sources.md §Report enrichment`); `None` when
    /// neither side was read.
    fn fetch_primary_market(
        &self,
        windows: &FrothWindows,
        gaps: &mut Vec<DataGap>,
    ) -> Option<PrimaryMarketActivity> {
        let ipos = self.fetch_ipo_activity(windows, gaps);
        let deals = self.fetch_deal_activity(windows, gaps);
        primary_market::activity(windows, ipos, deals)
    }

    /// A key below the paid tier (402) or a 404 leaves the IPO side absent silently; a
    /// this-run failure, an unreadable body, a served offering with no readable date, or
    /// a calendar with no row in any window records one gap.
    fn fetch_ipo_activity(
        &self,
        windows: &FrothWindows,
        gaps: &mut Vec<DataGap>,
    ) -> Option<IpoActivity> {
        if self.progress.is_cancelled() {
            return None;
        }
        let from = windows.first_day().format("%Y-%m-%d").to_string();
        let to = windows.last_day().format("%Y-%m-%d").to_string();
        let group = GroupKind::PrimaryMarket;
        let (series_id, name) = ("ipos-calendar", "IPO Calendar");
        self.progress
            .request_started("FMP", group.as_str(), series_id, name);
        let gaps_before = gaps.len();
        let gap = |reason| DataGap::new(group, series_id, name, reason);
        let (disposition, mut detail) = self.get_interpreted(
            FMP_IPOS_CALENDAR_PATH,
            &[("from", from.as_str()), ("to", to.as_str())],
        );
        let out = match disposition {
            Disposition::Value(value) => match ipo_rows_from_value(&value) {
                Ok((rows, undated_offerings)) => {
                    let (read, reason) =
                        primary_market::ipo_activity(&rows, undated_offerings, windows);
                    if let Some(reason) = reason {
                        detail = Some(if undated_offerings > 0 {
                            format!("{undated_offerings} served offering(s) carried no readable date")
                        } else {
                            "no calendar row in any window".to_string()
                        });
                        gaps.push(gap(reason));
                    }
                    read
                }
                Err(e) => {
                    detail = Some(format!("{e:#}"));
                    gaps.push(gap(GapReason::Malformed));
                    None
                }
            },
            Disposition::Gap(GapReason::OutOfScope) => None,
            Disposition::Gap(reason) => {
                gaps.push(gap(reason));
                None
            }
        };
        emit_series_row(
            &self.progress,
            "FMP",
            group,
            series_id,
            name,
            gaps,
            gaps_before,
            out.is_some(),
            detail,
        );
        out
    }

    /// The M&A walk back to the windows' first day, one tracker row per page; a walk
    /// state with no deal read records one gap ([`primary_market::deal_activity`]).
    fn fetch_deal_activity(
        &self,
        windows: &FrothWindows,
        gaps: &mut Vec<DataGap>,
    ) -> Option<DealActivity> {
        let group = GroupKind::PrimaryMarket;
        let (series_id, name) = ("mergers-acquisitions-latest", "M&A Registrations");
        let since = windows.first_day().format("%Y-%m-%d").to_string();
        let walk = self.walk_mergers(&since, group.as_str(), series_id, name);
        let (read, gap) = primary_market::deal_activity(&walk, windows);
        if let Some(reason) = gap {
            gaps.push(DataGap::new(group, series_id, name, reason));
        }
        read
    }

    fn fetch_sector_pe(&self, gaps: &mut Vec<DataGap>) -> Vec<SectorPe> {
        let mut out = Vec::new();
        for exchange in SNAPSHOT_EXCHANGES {
            if self.progress.is_cancelled() {
                break;
            }
            out.extend(self.fetch_sector_pe_for_exchange(exchange, gaps));
        }
        out
    }

    /// The same date walk as `fetch_sectors`, pinned to `exchange`; additive, so an
    /// exhausted walk returns empty with no gap.
    fn fetch_sector_pe_for_exchange(
        &self,
        exchange: &str,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<SectorPe> {
        // The ET session, not the UTC date (see `fetch_sectors`).
        let today = crate::market_clock::et_session_date(Utc::now());
        for date in sector_candidate_dates(today, SECTOR_LOOKBACK_WEEKDAYS) {
            if self.progress.is_cancelled() {
                return Vec::new();
            }
            let date_str = date.format("%Y-%m-%d").to_string();
            let name = format!("Sector P/E {exchange} ({date_str})");
            let series_id = format!("sector-pe-{}-{date_str}", exchange.to_ascii_lowercase());
            let group = GroupKind::SectorPe.as_str();
            self.progress
                .request_started("FMP", group, series_id.as_str(), name.as_str());
            let disposition = match self.get(
                FMP_SECTOR_PE_PATH,
                &[("date", date_str.as_str()), ("exchange", exchange)],
            ) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            let finish = |status: &str| {
                self.progress.request_finished(
                    "FMP",
                    group,
                    series_id.as_str(),
                    name.as_str(),
                    status,
                    None,
                )
            };
            match disposition {
                Disposition::Value(value) => match sector_pe_from_value(value, exchange) {
                    Ok(rows) if !rows.is_empty() => {
                        finish("ok");
                        return rows;
                    }
                    Ok(_) => finish("empty"),
                    Err(_) => {
                        finish("malformed");
                        gaps.push(sector_pe_gap(exchange, GapReason::Malformed));
                        return Vec::new();
                    }
                },
                Disposition::Gap(GapReason::OutOfScope) => finish("out-of-scope"),
                Disposition::Gap(reason) => {
                    finish(reason.as_str());
                    gaps.push(sector_pe_gap(exchange, reason));
                    return Vec::new();
                }
            }
        }
        Vec::new()
    }

    fn fetch_industries(&self, gaps: &mut Vec<DataGap>) -> Vec<IndustrySnapshot> {
        let mut out = Vec::new();
        for exchange in SNAPSHOT_EXCHANGES {
            if self.progress.is_cancelled() {
                break;
            }
            out.extend(self.fetch_industries_for_exchange(exchange, gaps));
        }
        out
    }

    /// The industry-performance snapshot is the spine (the same date walk as
    /// `fetch_sectors`); the P/E snapshot is fetched for that same date and exchange and
    /// degrades independently, its failure leaving `pe: None` plus one gap rather than
    /// dropping the rotation read.
    fn fetch_industries_for_exchange(
        &self,
        exchange: &str,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<IndustrySnapshot> {
        // The ET session, not the UTC date (see `fetch_sectors`).
        let today = crate::market_clock::et_session_date(Utc::now());
        for date in sector_candidate_dates(today, SECTOR_LOOKBACK_WEEKDAYS) {
            if self.progress.is_cancelled() {
                return Vec::new();
            }
            let date_str = date.format("%Y-%m-%d").to_string();
            let name = format!("Industry performance {exchange} ({date_str})");
            let series_id = format!(
                "industry-performance-{}-{date_str}",
                exchange.to_ascii_lowercase()
            );
            let group = GroupKind::Industries.as_str();
            self.progress
                .request_started("FMP", group, series_id.as_str(), name.as_str());
            let disposition = match self.get(
                FMP_INDUSTRY_PERF_PATH,
                &[("date", date_str.as_str()), ("exchange", exchange)],
            ) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            let finish = |status: &str| {
                self.progress.request_finished(
                    "FMP",
                    group,
                    series_id.as_str(),
                    name.as_str(),
                    status,
                    None,
                )
            };
            match disposition {
                Disposition::Value(value) => match industry_perf_from_value(value, exchange) {
                    Ok(perf) if !perf.is_empty() => {
                        finish("ok");
                        let pe = self.fetch_industry_pe(date_str.as_str(), exchange, gaps);
                        return top_bottom_industries(perf, &pe);
                    }
                    Ok(_) => finish("empty"),
                    Err(_) => {
                        finish("malformed");
                        gaps.push(industry_perf_gap(exchange, GapReason::Malformed));
                        return Vec::new();
                    }
                },
                Disposition::Gap(GapReason::OutOfScope) => finish("out-of-scope"),
                Disposition::Gap(reason) => {
                    finish(reason.as_str());
                    gaps.push(industry_perf_gap(exchange, reason));
                    return Vec::new();
                }
            }
        }
        Vec::new()
    }

    /// The optional valuation join: any failure degrades to an empty map (`pe: None`), a
    /// this-run failure additionally recording one gap. Never aborts the group.
    fn fetch_industry_pe(
        &self,
        date_str: &str,
        exchange: &str,
        gaps: &mut Vec<DataGap>,
    ) -> HashMap<(String, String), f64> {
        if self.progress.is_cancelled() {
            return HashMap::new();
        }
        let name = format!("Industry P/E {exchange} ({date_str})");
        let series_id = format!("industry-pe-{}-{date_str}", exchange.to_ascii_lowercase());
        let group = GroupKind::Industries.as_str();
        self.progress
            .request_started("FMP", group, series_id.as_str(), name.as_str());
        let disposition = match self.get(
            FMP_INDUSTRY_PE_PATH,
            &[("date", date_str), ("exchange", exchange)],
        ) {
            Ok((status, body)) => interpret_response(status, &body),
            Err(_) => Disposition::Gap(GapReason::Unavailable),
        };
        let finish = |status: &str| {
            self.progress.request_finished(
                "FMP",
                group,
                series_id.as_str(),
                name.as_str(),
                status,
                None,
            )
        };
        match disposition {
            Disposition::Value(value) => match industry_pe_map_from_value(value, exchange) {
                Ok(map) if !map.is_empty() => {
                    finish("ok");
                    map
                }
                Ok(_) => {
                    finish("empty");
                    HashMap::new()
                }
                Err(_) => {
                    finish("malformed");
                    gaps.push(industry_pe_gap(exchange, GapReason::Malformed));
                    HashMap::new()
                }
            },
            Disposition::Gap(GapReason::OutOfScope) => {
                finish("out-of-scope");
                HashMap::new()
            }
            Disposition::Gap(reason) => {
                finish(reason.as_str());
                gaps.push(industry_pe_gap(exchange, reason));
                HashMap::new()
            }
        }
    }

    /// Per exchange, the P/E and return history of every sector the snapshot returned
    /// (`docs/data-sources.md §Report enrichment`); a read that was not derivable stays
    /// `None`.
    fn attach_sector_history(
        &self,
        rows: &mut [SectorPe],
        clock: HistoryClock,
        gaps: &mut Vec<DataGap>,
    ) {
        for exchange in SNAPSHOT_EXCHANGES {
            let names: Vec<String> = rows
                .iter()
                .filter(|r| r.exchange == *exchange)
                .map(|r| r.sector.clone())
                .collect();
            if names.is_empty() {
                continue;
            }
            let pe = self.fetch_history_family(
                HistoryFamily::SectorPe,
                exchange,
                &names,
                clock,
                group_history::pe_history,
                gaps,
            );
            let ret = self.fetch_history_family(
                HistoryFamily::SectorReturn,
                exchange,
                &names,
                clock,
                group_history::trailing_return,
                gaps,
            );
            for row in rows.iter_mut().filter(|r| r.exchange == *exchange) {
                row.pe_history = pe.get(&row.sector).cloned();
                row.trailing_return = ret.get(&row.sector).cloned();
            }
        }
    }

    /// As [`Self::attach_sector_history`], for the surfaced industries.
    fn attach_industry_history(
        &self,
        rows: &mut [IndustrySnapshot],
        clock: HistoryClock,
        gaps: &mut Vec<DataGap>,
    ) {
        for exchange in SNAPSHOT_EXCHANGES {
            let names: Vec<String> = rows
                .iter()
                .filter(|r| r.exchange == *exchange)
                .map(|r| r.industry.clone())
                .collect();
            if names.is_empty() {
                continue;
            }
            let pe = self.fetch_history_family(
                HistoryFamily::IndustryPe,
                exchange,
                &names,
                clock,
                group_history::pe_history,
                gaps,
            );
            let ret = self.fetch_history_family(
                HistoryFamily::IndustryReturn,
                exchange,
                &names,
                clock,
                group_history::trailing_return,
                gaps,
            );
            for row in rows.iter_mut().filter(|r| r.exchange == *exchange) {
                row.pe_history = pe.get(&row.industry).cloned();
                row.trailing_return = ret.get(&row.industry).cloned();
            }
        }
    }

    /// Fail-soft by cause: a 402 (a key below the paid tier) stops the loop silently; a
    /// rejected or unavailable response records one family-level gap and stops; a 404, an
    /// empty or unreadable body, or a series below the sample floor or freshness bound
    /// records one gap for that group alone and the loop moves on.
    fn fetch_history_family<T>(
        &self,
        family: HistoryFamily,
        exchange: &str,
        names: &[String],
        clock: HistoryClock,
        derive: fn(&[DatedPoint], NaiveDate) -> std::result::Result<T, NotDerivable>,
        gaps: &mut Vec<DataGap>,
    ) -> HashMap<String, T> {
        let mut out = HashMap::new();
        let from = clock.from.format("%Y-%m-%d").to_string();
        let to = clock.to.format("%Y-%m-%d").to_string();
        let group = family.group();
        for name in names {
            if self.progress.is_cancelled() {
                break;
            }
            let series_id = format!(
                "{}-{}-{}",
                family.slug(),
                exchange.to_ascii_lowercase(),
                series_token(name)
            );
            let display = format!("{name} {} ({exchange})", family.read_label());
            self.progress
                .request_started("FMP", group.as_str(), &series_id, &display);
            let gaps_before = gaps.len();
            let row_gap = |reason| DataGap::new(group, series_id.as_str(), display.as_str(), reason);
            let mut detail = None;
            let mut stop = false;
            let response = self.get(
                family.path(),
                &[
                    (family.name_key(), name.as_str()),
                    ("exchange", exchange),
                    ("from", from.as_str()),
                    ("to", to.as_str()),
                ],
            );
            match response {
                Ok((402, body)) => {
                    detail = Some(http_gap_detail(402, &body));
                    stop = true;
                }
                Ok((status, body)) => match interpret_response(status, &body) {
                    Disposition::Value(value) => {
                        match history_points_from_value(&value, family, name, exchange) {
                            Ok(points) if !points.is_empty() => {
                                match derive(&points, clock.session) {
                                    Ok(read) => {
                                        out.insert(name.clone(), read);
                                    }
                                    Err(why) => {
                                        detail = Some(why.detail());
                                        gaps.push(row_gap(GapReason::Unavailable));
                                    }
                                }
                            }
                            Ok(_) if value.as_array().is_some_and(Vec::is_empty) => {
                                detail = Some("empty response".to_string());
                                gaps.push(row_gap(GapReason::Unavailable));
                            }
                            Ok(_) => {
                                detail = Some("no served row was readable".to_string());
                                gaps.push(row_gap(GapReason::Malformed));
                            }
                            Err(e) => {
                                detail = Some(format!("{e:#}"));
                                gaps.push(row_gap(GapReason::Malformed));
                            }
                        }
                    }
                    Disposition::Gap(reason @ (GapReason::Rejected | GapReason::Unavailable)) => {
                        detail = Some(http_gap_detail(status, &body));
                        gaps.push(history_family_gap(family, exchange, reason));
                        stop = true;
                    }
                    Disposition::Gap(reason) => {
                        detail = Some(http_gap_detail(status, &body));
                        gaps.push(row_gap(reason));
                    }
                },
                Err(e) => {
                    detail = Some(format!("{e:#}"));
                    gaps.push(history_family_gap(family, exchange, GapReason::Unavailable));
                    stop = true;
                }
            }
            emit_series_row(
                &self.progress,
                "FMP",
                group,
                &series_id,
                &display,
                gaps,
                gaps_before,
                out.contains_key(name),
                detail,
            );
            if stop {
                break;
            }
        }
        out
    }

    /// Additive like `fetch_earnings`: a permanent absence or a no-US-row response is
    /// silent.
    fn fetch_market_risk_premium(&self, gaps: &mut Vec<DataGap>) -> Vec<MarketRiskPremium> {
        if self.progress.is_cancelled() {
            return Vec::new();
        }
        let series_id = "market-risk-premium";
        let name = "US Equity Risk Premium";
        self.progress.request_started(
            "FMP",
            GroupKind::MarketRiskPremium.as_str(),
            series_id,
            name,
        );
        let gaps_before = gaps.len();
        let (disposition, gap_detail) = self.get_interpreted(FMP_RISK_PREMIUM_PATH, &[]);
        let out = match disposition {
            Disposition::Value(value) => match risk_premium_from_value(value) {
                Ok(rows) => rows,
                Err(_) => {
                    gaps.push(DataGap::new(
                        GroupKind::MarketRiskPremium,
                        series_id,
                        name,
                        GapReason::Malformed,
                    ));
                    Vec::new()
                }
            },
            Disposition::Gap(GapReason::OutOfScope) => Vec::new(),
            Disposition::Gap(reason) => {
                gaps.push(DataGap::new(
                    GroupKind::MarketRiskPremium,
                    series_id,
                    name,
                    reason,
                ));
                Vec::new()
            }
        };
        emit_series_row(
            &self.progress,
            "FMP",
            GroupKind::MarketRiskPremium,
            series_id,
            name,
            gaps,
            gaps_before,
            !out.is_empty(),
            gap_detail,
        );
        out
    }

    /// The FMP half of a holding's evidence packet (`docs/portfolio-analysis.md`,
    /// `docs/data-sources.md §Local Analysis Suite Sources`): fail-soft per endpoint, each
    /// failure recording a tagged gap on the returned [`CompanyFinancials`]. The valuation
    /// multiples are left for the dossier to derive from market cap + SEC facts.
    fn fetch_quote_and_eod(
        &self,
        symbol: &str,
    ) -> crate::portfolio::engine::CompanyFinancials {
        use crate::portfolio::engine::CompanyFinancials;
        let mut fin = CompanyFinancials {
            symbol: symbol.to_string(),
            ..CompanyFinancials::default()
        };

        if self.progress.is_cancelled() {
            fin.gaps.push("company financials skipped (run cancelled)".to_string());
            return fin;
        }

        // 1) Quote. The row's status reflects the shaped outcome (ok only when a price
        // landed), and a gap carries its cause as the detail, so a quota rejection
        // paints "rejected" plus FMP's sentence rather than a benign non-result.
        if let Err(reason) = self.suite_get_shaped(
            "company-quote",
            symbol,
            "Company quote",
            FMP_QUOTE_PATH,
            &[("symbol", symbol)],
            |value| match company_quote_from_value(value) {
                Some(q) => {
                    fin.current_price = q.price;
                    fin.market_cap = q.market_cap;
                    fin.shares_outstanding = q.shares_outstanding;
                    fin.year_high = q.year_high;
                    fin.year_low = q.year_low;
                    match (q.price, q.unusable_price) {
                        (Some(_), _) => Shaped::ok(()),
                        // A served zero or negative print is no price.
                        (None, Some(served)) => {
                            let detail =
                                format!("FMP quote carried no usable price (served {served})");
                            fin.gaps.push(detail.clone());
                            Shaped::empty(()).with_detail(detail)
                        }
                        (None, None) => {
                            fin.gaps.push("FMP quote carried no price".to_string());
                            Shaped::empty(())
                        }
                    }
                }
                // A served-empty array (an unknown symbol) is an honest empty answer,
                // not drift; the parser folds both into `None`, so the split runs here.
                None if value.as_array().is_some_and(|a| a.is_empty()) => {
                    fin.gaps.push("FMP quote was empty".to_string());
                    Shaped::empty(())
                }
                None => {
                    fin.gaps.push("FMP quote was malformed".to_string());
                    Shaped::malformed(())
                        .with_detail("body was not the expected array shape — malformed or drifted response")
                }
            },
        ) {
            fin.gaps
                .push(format!("FMP quote unavailable ({})", reason.as_str()));
        }

        // 2) EOD price history for momentum + volatility.
        if self.progress.is_cancelled() {
            fin.gaps.push("price history skipped (run cancelled)".to_string());
            return fin;
        }
        // Fetch-range upper bound deliberately stays UTC (see `fetch_index_performance`).
        let to = Utc::now().date_naive();
        let from = to - Duration::days(COMPANY_EOD_LOOKBACK_DAYS);
        if let Err(reason) = self.suite_get_shaped(
            "company-eod",
            symbol,
            "Company price history",
            FMP_EOD_PATH,
            &[
                ("symbol", symbol),
                ("from", from.format("%Y-%m-%d").to_string().as_str()),
                ("to", to.format("%Y-%m-%d").to_string().as_str()),
            ],
            |value| match eod_prices_from_value(value) {
                Ok(history) if !history.is_empty() => {
                    fin.price_history = history;
                    Shaped::ok(())
                }
                // A served, non-empty array with no readable row is drift, not
                // emptiness (the parser drops date/price-less rows silently).
                Ok(_) if value.as_array().is_some_and(|a| !a.is_empty()) => {
                    fin.gaps.push("FMP price history was malformed".to_string());
                    Shaped::malformed(())
                        .with_detail("no served row was readable — malformed or drifted response")
                }
                Ok(_) => {
                    fin.gaps.push("FMP price history was empty".to_string());
                    Shaped::empty(())
                }
                Err(e) => {
                    fin.gaps.push("FMP price history was malformed".to_string());
                    Shaped::malformed(()).with_detail(format!("{e:#}"))
                }
            },
        ) {
            fin.gaps
                .push(format!("FMP price history unavailable ({})", reason.as_str()));
        }

        fin
    }

    /// The stock per-symbol surface, each endpoint fail-soft with a tagged gap; a
    /// missing consensus later abstains the holding under the `no-admissible-driver`
    /// floor reason rather than failing here.
    pub fn fetch_company_financials(
        &self,
        symbol: &str,
    ) -> crate::portfolio::engine::CompanyFinancials {
        let mut fin = self.fetch_quote_and_eod(symbol);
        fin.quarterly_income =
            self.fetch_quarterly_income(symbol, &mut fin.gaps, &mut fin.unit_issues);
        fin.quarterly_cash_flow =
            self.fetch_quarterly_cash_flow(symbol, &mut fin.gaps, &mut fin.unit_issues);
        fin.quarterly_balance_sheet =
            self.fetch_balance_sheet(symbol, &mut fin.gaps, &mut fin.unit_issues);
        let balance = BalanceSheetLines::from_newest(&fin.quarterly_balance_sheet);
        fin.total_debt = balance.total_debt;
        fin.total_equity = balance.total_equity;
        fin.cash_and_equivalents = balance.cash_and_equivalents;
        fin.short_term_investments = balance.short_term_investments;
        fin.financial_scores = self.fetch_financial_scores(symbol, &mut fin.gaps);
        fin.consensus = self.fetch_analyst_estimates(symbol, &mut fin.gaps);
        let dividends = self.fetch_dividend_pull(symbol, &mut fin.gaps);
        fin.ttm_dividends_per_share = dividends.ttm;
        fin.recent_dividends = dividends.recent;
        fin
    }

    /// The statement / consensus endpoints are stock surface, so a fund never logs
    /// their spurious gaps.
    pub fn fetch_fund_financials(
        &self,
        symbol: &str,
    ) -> crate::portfolio::engine::CompanyFinancials {
        let mut fin = self.fetch_quote_and_eod(symbol);
        let dividends = self.fetch_dividend_pull(symbol, &mut fin.gaps);
        fin.ttm_dividends_per_share = dividends.ttm;
        fin.recent_dividends = dividends.recent;
        fin
    }
}

/// Long enough for a momentum and volatility read, short enough to stay one light call.
const COMPANY_EOD_LOOKBACK_DAYS: i64 = 180;

struct CompanyQuote {
    /// Finite and strictly positive (`crate::portfolio::engine::usable_price`), else
    /// `None`.
    price: Option<f64>,
    /// A served `price` the usability test rejected, kept so the gap and tracker row
    /// can name what was served; `None` when the field was absent or usable.
    unusable_price: Option<f64>,
    market_cap: Option<f64>,
    shares_outstanding: Option<f64>,
    /// The served 52-week range; rendered as served, never a computed min / max.
    year_high: Option<f64>,
    year_low: Option<f64>,
}

/// `None` only when the body is not a non-empty array; individual missing fields stay
/// `None`. The usability rule is applied at this one parse because it is the seam every
/// quote consumer rides, two of which have no engine floor behind them, so a zero or
/// negative print never leaves the adapter as a price (`docs/portfolio-analysis.md`
/// §Evidence floor).
fn company_quote_from_value(value: &Value) -> Option<CompanyQuote> {
    let first = value.as_array()?.first()?;
    let served = first.get("price").and_then(Value::as_f64);
    let price = crate::portfolio::engine::usable_price(served);
    Some(CompanyQuote {
        price,
        unusable_price: served.filter(|_| price.is_none()),
        market_cap: first.get("marketCap").and_then(Value::as_f64),
        shares_outstanding: first.get("sharesOutstanding").and_then(Value::as_f64),
        year_high: first
            .get("yearHigh")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite()),
        year_low: first
            .get("yearLow")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite()),
    })
}

/// Parses a served date (`%Y-%m-%d`, non-zero-padded accepted) into its canonical
/// fixed-width render. The dated shapers key row readability on this and always store
/// the canonical form, never the source text: `"2026-9-30"` sorts after `"2026-12-31"`,
/// corrupting every lexicographic consumer (`DatedValue`'s ISO contract, the EOD
/// chronology sort, the quick check's since-`last_pass` compare).
fn canonical_date(date: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .map(|d| d.format("%Y-%m-%d").to_string())
}

/// A news row's `publishedDate` with its leading date token (before any space- or
/// `T`-separated time part) canonicalized, so the since-`from` lexicographic compare
/// sees a fixed-width prefix; `None` when the token is no calendar date. Shared by
/// [`symbol_news_from_value`] and the fetch site's drift guard so "readable" cannot
/// drift between the two.
fn canonical_news_date(row: &Value) -> Option<String> {
    let published = row.get("publishedDate").and_then(Value::as_str)?;
    let token = published.split([' ', 'T']).next().unwrap_or("");
    let canonical = canonical_date(token)?;
    Some(format!("{canonical}{}", &published[token.len()..]))
}

/// Chronological (oldest first) closes. A close is usable only when finite and strictly
/// positive ([`crate::portfolio::engine::usable_price`]), so a served zero or negative
/// print drops as an unreadable row rather than riding into a return or a drawdown.
fn eod_prices_from_value(value: &Value) -> Result<Vec<f64>> {
    let rows = value
        .as_array()
        .context("FMP EOD response did not match the expected array shape")?;
    let mut dated: Vec<(String, f64)> = Vec::with_capacity(rows.len());
    for row in rows {
        if let (Some(date), Some(price)) = (
            // The canonical date render: the sort below relies on fixed-width ISO
            // ordering.
            row.get("date").and_then(Value::as_str).and_then(canonical_date),
            crate::portfolio::engine::usable_price(row.get("price").and_then(Value::as_f64)),
        ) {
            dated.push((date, price));
        }
    }
    dated.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(dated.into_iter().map(|(_, p)| p).collect())
}

/// Dated chronological closes, the deep-history form the anchor join reads. The same
/// usability rule as [`eod_prices_from_value`]: this series feeds the outcome labels'
/// price-bar cache, where a zero entry close makes an episode's `price_return`
/// unreadable.
fn dated_eod_from_value(value: &Value) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
    let rows = value
        .as_array()
        .context("FMP EOD response did not match the expected array shape")?;
    let mut dated: Vec<crate::portfolio::engine::DatedValue> = rows
        .iter()
        .filter_map(|row| {
            match (
                // The canonical date render: `DatedValue` carries the fixed-width ISO
                // contract downstream.
                row.get("date").and_then(Value::as_str).and_then(canonical_date),
                crate::portfolio::engine::usable_price(row.get("price").and_then(Value::as_f64)),
            ) {
                (Some(date), Some(price)) => Some(crate::portfolio::engine::DatedValue {
                    date,
                    value: price,
                }),
                _ => None,
            }
        })
        .collect();
    dated.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(dated)
}

/// Dated chronological bars with share volume. A row is readable on the same rule as
/// [`dated_eod_from_value`]; its volume is kept only when finite and non-negative, so a
/// row serving none keeps its close and reads as a missing volume day.
fn dated_bars_from_value(value: &Value) -> Result<Vec<crate::portfolio::engine::DailyBar>> {
    let rows = value
        .as_array()
        .context("FMP EOD response did not match the expected array shape")?;
    let mut bars: Vec<crate::portfolio::engine::DailyBar> = rows
        .iter()
        .filter_map(|row| {
            let date = row.get("date").and_then(Value::as_str).and_then(canonical_date)?;
            let close =
                crate::portfolio::engine::usable_price(row.get("price").and_then(Value::as_f64))?;
            let volume = row
                .get("volume")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v >= 0.0);
            Some(crate::portfolio::engine::DailyBar {
                date,
                close,
                volume,
            })
        })
        .collect();
    bars.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(bars)
}

impl MarketDataSource for FmpDataSource {
    fn baseline_scan(&self, cadence: ReportCadence) -> Result<BaselineMarketData> {
        // Every group degrades to recorded gaps and the coverage gate
        // (`pipeline::enforce_coverage`) decides the run's floor, so the scan returns
        // `Ok` for all data outcomes. Each fetch streams its own per-request tracker
        // rows; the scan emits no group-level summary rows.
        let mut gaps = Vec::new();
        let indices = self.fetch_quotes(INDEX_SYMBOLS, GroupKind::Indices, &mut gaps);
        let internals = self.fetch_quotes(INTERNAL_SYMBOLS, GroupKind::Internals, &mut gaps);
        let sectors = self.fetch_sectors(&mut gaps);
        let index_performance = self.fetch_index_performance(&mut gaps);
        let movers = self.fetch_movers(&mut gaps);
        let window_days = cadence_window_days(cadence.elapsed_days());
        let earnings = self.fetch_earnings(window_days, &mut gaps);
        // The primary-market windows sit on the ET session date, as the earnings window does.
        let froth_windows =
            FrothWindows::new(crate::market_clock::et_session_date(Utc::now()), window_days);
        let primary_market = self.fetch_primary_market(&froth_windows, &mut gaps);
        let history_clock = HistoryClock::at(Utc::now());
        let mut sector_pe = self.fetch_sector_pe(&mut gaps);
        self.attach_sector_history(&mut sector_pe, history_clock, &mut gaps);
        let mut industries = self.fetch_industries(&mut gaps);
        self.attach_industry_history(&mut industries, history_clock, &mut gaps);
        let market_risk_premium = self.fetch_market_risk_premium(&mut gaps);
        Ok(BaselineMarketData {
            indices,
            internals,
            sectors,
            index_performance,
            movers,
            earnings,
            sector_pe,
            industries,
            market_risk_premium,
            // FRED, BLS and CFTC own these groups; FMP contributes none of them.
            macro_levels: Vec::new(),
            labor_levels: Vec::new(),
            calendar: Vec::new(),
            cot_positioning: Vec::new(),
            primary_market,
            gaps,
        })
    }
}

impl EconomicCalendarSource for FmpDataSource {
    /// A 402 / 404 (a key below the paid tier) is silent; a this-run failure or an
    /// unreadable body records one gap. Either way the calendar stays at names and dates.
    fn economic_calendar(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<CalendarRow> {
        if self.progress.is_cancelled() {
            return Vec::new();
        }
        let series_id = "economic-calendar";
        let name = "Economic Calendar (consensus)";
        let from = from.format("%Y-%m-%d").to_string();
        let to = to.format("%Y-%m-%d").to_string();
        self.progress
            .request_started("FMP", GroupKind::Calendar.as_str(), series_id, name);
        let gaps_before = gaps.len();
        let (disposition, gap_detail) = self.get_interpreted(
            FMP_ECONOMIC_CALENDAR_PATH,
            &[("country", "US"), ("from", from.as_str()), ("to", to.as_str())],
        );
        let out = match disposition {
            Disposition::Value(value) => match econ_calendar::rows_from_value(value) {
                Ok(rows) => rows,
                Err(_) => {
                    gaps.push(DataGap::new(
                        GroupKind::Calendar,
                        series_id,
                        name,
                        GapReason::Malformed,
                    ));
                    Vec::new()
                }
            },
            Disposition::Gap(GapReason::OutOfScope) => Vec::new(),
            Disposition::Gap(reason) => {
                gaps.push(DataGap::new(GroupKind::Calendar, series_id, name, reason));
                Vec::new()
            }
        };
        emit_series_row(
            &self.progress,
            "FMP",
            GroupKind::Calendar,
            series_id,
            name,
            gaps,
            gaps_before,
            !out.is_empty(),
            gap_detail,
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    #[test]
    fn cadence_window_days_floors_caps_and_scales() {
        assert_eq!(cadence_window_days(None), EARNINGS_BACK_DAYS);
        assert_eq!(cadence_window_days(Some(1.0)), EARNINGS_BACK_DAYS);
        assert_eq!(cadence_window_days(Some(3.2)), EARNINGS_BACK_DAYS);
        assert_eq!(cadence_window_days(Some(10.0)), 10);
        assert_eq!(cadence_window_days(Some(12.5)), 13);
        assert_eq!(cadence_window_days(Some(30.0)), 30);
        assert_eq!(cadence_window_days(Some(45.0)), EARNINGS_BACK_MAX_DAYS);
        assert_eq!(cadence_window_days(Some(120.0)), EARNINGS_BACK_MAX_DAYS);
        // Degenerate intervals (clock skew, non-finite) clamp to the floor, never panic.
        assert_eq!(cadence_window_days(Some(-5.0)), EARNINGS_BACK_DAYS);
        assert_eq!(cadence_window_days(Some(f64::NAN)), EARNINGS_BACK_DAYS);
        assert_eq!(cadence_window_days(Some(f64::INFINITY)), EARNINGS_BACK_MAX_DAYS);
    }

    #[test]
    fn interpret_response_covers_the_full_matrix() {
        use GapReason::*;
        assert!(matches!(
            interpret_response(
                200,
                r#"[{"symbol":"^GSPC","price":1.0,"changePercentage":0.1}]"#
            ),
            Disposition::Value(_)
        ));
        assert!(matches!(
            interpret_response(200, "[]"),
            Disposition::Value(_)
        ));

        assert!(matches!(
            interpret_response(402, "Premium Query Parameter"),
            Disposition::Gap(OutOfScope)
        ));
        assert!(matches!(
            interpret_response(404, ""),
            Disposition::Gap(OutOfScope)
        ));

        for status in [401, 403] {
            assert!(
                matches!(interpret_response(status, ""), Disposition::Gap(Rejected)),
                "HTTP {status}"
            );
        }
        for status in [429, 500, 503] {
            assert!(
                matches!(
                    interpret_response(status, ""),
                    Disposition::Gap(Unavailable)
                ),
                "HTTP {status}"
            );
        }
        for status in [400, 408, 422] {
            assert!(
                matches!(interpret_response(status, ""), Disposition::Gap(Malformed)),
                "HTTP {status}"
            );
        }

        assert!(matches!(
            interpret_response(200, r#"{"Error Message":"Limit Reach"}"#),
            Disposition::Gap(Rejected)
        ));
        // The same body on a non-2xx is classified by status, not body.
        assert!(matches!(
            interpret_response(402, r#"{"Error Message":"Premium"}"#),
            Disposition::Gap(OutOfScope)
        ));
        assert!(matches!(
            interpret_response(200, "not json at all"),
            Disposition::Gap(Malformed)
        ));
    }

    // ---- Offline round trip: adapter -> retry -> interpret -> domain output ----
    //
    // Single-reply scripts (no retryable status), so no backoff sleep is incurred;
    // retry mechanics live in `http_retry`'s tests.

    fn test_source(base_url: &str) -> FmpDataSource {
        FmpDataSource::new("test-key".to_string())
            .expect("build adapter")
            .with_base_url(base_url)
    }

    #[test]
    fn economic_calendar_reads_the_window_and_soft_fails_by_cause() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2026-10-02 12:30:00","country":"US","event":"Non Farm Payrolls (Sep)","impact":"High","estimate":90,"actual":29,"unit":"K"}]"#,
            },
            Canned::Reply {
                status: 402,
                headers: vec![],
                body: "Payment Required",
            },
            Canned::Reply {
                status: 403,
                headers: vec![],
                body: "Forbidden",
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"unexpected":true}"#,
            },
        ]);
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let source = test_source(&server.base_url).with_context(ctx);
        let from = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let mut gaps = Vec::new();

        let rows = source.economic_calendar(from, to, &mut gaps);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event, "Non Farm Payrolls (Sep)");
        assert_eq!((rows[0].estimate, rows[0].actual), (Some(90.0), Some(29.0)));
        assert!(gaps.is_empty());
        let target = &server.request_targets()[0];
        assert!(target.starts_with("/economic-calendar?"), "{target}");
        for param in ["country=US", "from=2026-09-01", "to=2026-11-02"] {
            assert!(target.contains(param), "{target}");
        }

        // A key below the paid tier: no rows and no gap — names and dates, silently.
        assert!(source.economic_calendar(from, to, &mut gaps).is_empty());
        assert!(gaps.is_empty(), "a plan-tier refusal records no gap");

        // A this-run failure and an unreadable body each record one Calendar gap.
        assert!(source.economic_calendar(from, to, &mut gaps).is_empty());
        assert!(source.economic_calendar(from, to, &mut gaps).is_empty());
        let recorded: Vec<(GroupKind, &str, GapReason)> = gaps
            .iter()
            .map(|g| (g.group, g.series_id.as_str(), g.reason))
            .collect();
        assert_eq!(
            recorded,
            [
                (GroupKind::Calendar, "economic-calendar", GapReason::Rejected),
                (GroupKind::Calendar, "economic-calendar", GapReason::Malformed),
            ]
        );
        let finished: Vec<(String, String)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished {
                    series_id, status, ..
                } => Some((series_id, status)),
                _ => None,
            })
            .collect();
        let statuses: Vec<&str> = finished.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(statuses, ["ok", "empty", "rejected", "malformed"]);
        assert!(finished.iter().all(|(id, _)| id == "economic-calendar"));
    }

    /// A live-shaped `historical-*` body: `n` daily rows ending at `end`, newest first (the
    /// served order), each valued `value` under `value_key`.
    fn history_body(
        name_key: &str,
        name: &str,
        exchange: &str,
        value_key: &str,
        n: usize,
        end: &str,
        value: f64,
    ) -> &'static str {
        let end = NaiveDate::parse_from_str(end, "%Y-%m-%d").unwrap();
        let rows: Vec<Value> = (0..n)
            .map(|i| {
                serde_json::json!({
                    "date": (end - Duration::days(i as i64)).format("%Y-%m-%d").to_string(),
                    name_key: name,
                    "exchange": exchange,
                    value_key: value,
                })
            })
            .collect();
        Box::leak(Value::Array(rows).to_string().into_boxed_str())
    }

    fn history_clock() -> HistoryClock {
        let day = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        HistoryClock {
            from: day("2025-10-12"),
            to: day("2026-10-12"),
            session: day("2026-10-12"),
        }
    }

    fn sector_row(sector: &str, exchange: &str) -> SectorPe {
        SectorPe {
            sector: sector.into(),
            exchange: exchange.into(),
            pe: Some(20.0),
            pe_history: None,
            trailing_return: None,
        }
    }

    fn industry_row(industry: &str, exchange: &str) -> IndustrySnapshot {
        IndustrySnapshot {
            industry: industry.into(),
            exchange: exchange.into(),
            change_pct: 1.0,
            pe: None,
            pe_history: None,
            trailing_return: None,
        }
    }

    /// N = 30 on 2026-10-12: prior 08-13..09-11, recent 09-12..10-11, upcoming 10-12..10-25.
    fn froth_windows() -> FrothWindows {
        FrothWindows::new(NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(), 30)
    }

    fn froth_source(base_url: &str) -> (FmpDataSource, Arc<crate::progress::RecordingReporter>) {
        let rec = Arc::new(crate::progress::RecordingReporter::default());
        let ctx = RunContext::new(
            "run",
            rec.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        (test_source(base_url).with_context(ctx), rec)
    }

    fn reply(status: u16, body: &'static str) -> Canned {
        Canned::Reply {
            status,
            headers: vec![],
            body,
        }
    }

    /// The live calendar's row shapes: two recent priced offerings, one prior, one scheduled,
    /// and a fund launch with no share count.
    const IPO_BODY: &str = r#"[
        {"symbol":"CTY","date":"2026-10-16","company":"City Therapeutics","exchange":"NASDAQ","actions":"Expected","shares":9722222,"priceRange":"17.00 - 19.00","marketCap":null,"dealSize":212430545},
        {"symbol":"TRXB","date":"2026-10-09","company":"TRex Bio Inc.","exchange":"NASDAQ","actions":"Priced","shares":8333334,"priceRange":null,"marketCap":113287167,"dealSize":116666676},
        {"symbol":"OAKL","date":"2026-10-07","company":"Oakmark ETF","exchange":"NYSE","actions":"Priced","shares":null,"priceRange":null,"marketCap":25,"dealSize":null},
        {"symbol":"ACCV","date":"2026-09-30","company":"Accelevation Holdings Corp.","exchange":"NASDAQ","actions":"Priced","shares":30000000,"priceRange":null,"marketCap":null,"dealSize":540000000},
        {"symbol":"LYNX","date":"2026-08-19","company":"Lyntris Inc.","exchange":"NASDAQ","actions":"Priced","shares":17000000,"priceRange":null,"marketCap":null,"dealSize":297500000}
    ]"#;

    /// One M&A page, newest first: a recent filing fanned out over two securities, a prior
    /// filing, and a row past the windows' first day that ends the walk.
    const MNA_BODY: &str = r#"[
        {"symbol":"IPXG","companyName":"Inflection Point VII","targetedSymbol":"LUNR","targetedCompanyName":"Intuitive Machines, Inc.","transactionDate":"2026-10-08","link":"https://sec.gov/a"},
        {"symbol":"IPXGW","companyName":"Inflection Point VII*","targetedSymbol":"LUNR","targetedCompanyName":"Intuitive Machines, Inc.","transactionDate":"2026-10-08","link":"https://sec.gov/a"},
        {"symbol":"OSTK","companyName":"Overstock","targetedSymbol":"FTHM","targetedCompanyName":"Fathom Holdings Inc.","transactionDate":"2026-08-17","link":"https://sec.gov/b"},
        {"symbol":"AMZN","companyName":"Amazon","targetedSymbol":"GSAT","targetedCompanyName":"Globalstar, Inc.","transactionDate":"2026-07-31","link":"https://sec.gov/c"}
    ]"#;

    fn full_mna_page(date: &str) -> &'static str {
        let rows: Vec<String> = (0..crate::portfolio::evidence::MA_PAGE_LIMIT)
            .map(|i| {
                format!(
                    r#"{{"symbol":"A{i}","companyName":"Acq {i}","targetedCompanyName":"Tgt {i}","transactionDate":"{date}","link":"L{i}"}}"#
                )
            })
            .collect();
        Box::leak(format!("[{}]", rows.join(",")).into_boxed_str())
    }

    #[test]
    fn primary_market_reads_both_feeds_with_one_row_per_request() {
        let server = MockHttp::serve(vec![reply(200, IPO_BODY), reply(200, MNA_BODY)]);
        let (source, rec) = froth_source(&server.base_url);
        let mut gaps = Vec::new();
        let group = source
            .fetch_primary_market(&froth_windows(), &mut gaps)
            .expect("both sides read");
        assert!(gaps.is_empty(), "{gaps:?}");
        assert_eq!(
            (group.recent.from.as_str(), group.recent.to.as_str()),
            ("2026-09-12", "2026-10-11")
        );
        let ipos = group.ipos.unwrap();
        assert_eq!(
            (ipos.recent_count, ipos.prior_count, ipos.upcoming_count),
            (2, 1, 1),
            "the fund launch carries no share count"
        );
        let names: Vec<&str> = ipos
            .notable
            .iter()
            .map(|n| n.symbol.as_deref().unwrap())
            .collect();
        assert_eq!(names, vec!["ACCV", "TRXB"]);
        assert_eq!(ipos.notable[0].deal_size_usd, Some(540_000_000.0));
        let deals = group.deals.unwrap();
        assert_eq!((deals.recent_count, deals.prior_count), (1, Some(1)));
        assert_eq!(deals.notable[0].target_symbol.as_deref(), Some("LUNR"));
        let targets = server.request_targets();
        assert!(targets[0].starts_with("/ipos-calendar?"), "{}", targets[0]);
        for param in ["from=2026-08-13", "to=2026-10-25"] {
            assert!(targets[0].contains(param), "{}", targets[0]);
        }
        assert!(targets[1].starts_with("/mergers-acquisitions-latest?"), "{}", targets[1]);
        assert!(targets[1].contains("page=0"), "{}", targets[1]);
        assert_eq!(targets.len(), 2, "the first page crossed the windows' first day");
        assert_eq!(
            recorded_rows(&rec),
            vec![
                ("ipos-calendar".to_string(), "ok".to_string()),
                ("mergers-acquisitions-latest".to_string(), "ok".to_string()),
            ]
        );
        let groups: Vec<String> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished { group, .. } => Some(group),
                _ => None,
            })
            .collect();
        assert_eq!(groups, vec!["primary-market", "primary-market"]);
    }

    #[test]
    fn primary_market_below_the_paid_tier_is_absent_silently() {
        let server = MockHttp::serve(vec![
            reply(402, "Payment Required"),
            reply(402, "Payment Required"),
        ]);
        let (source, _) = froth_source(&server.base_url);
        let mut gaps = Vec::new();
        assert_eq!(source.fetch_primary_market(&froth_windows(), &mut gaps), None);
        assert!(gaps.is_empty(), "{gaps:?}");
    }

    #[test]
    fn primary_market_ipo_side_fails_soft_by_cause_beside_the_deal_side() {
        let gap_ids = |gaps: &[DataGap]| -> Vec<(GroupKind, String, GapReason)> {
            gaps.iter()
                .map(|g| (g.group, g.series_id.clone(), g.reason))
                .collect()
        };
        let cases: [(Canned, GapReason); 3] = [
            (reply(403, "Forbidden"), GapReason::Rejected),
            (reply(200, "[]"), GapReason::Unavailable),
            (reply(200, r#"[{"symbol":"NODATE"}]"#), GapReason::Malformed),
        ];
        for (ipo_reply, reason) in cases {
            let server = MockHttp::serve(vec![ipo_reply, reply(200, MNA_BODY)]);
            let (source, _) = froth_source(&server.base_url);
            let mut gaps = Vec::new();
            let group = source
                .fetch_primary_market(&froth_windows(), &mut gaps)
                .expect("the deal side still reads");
            assert_eq!(group.ipos, None);
            assert!(group.deals.is_some());
            assert_eq!(
                gap_ids(&gaps),
                vec![(GroupKind::PrimaryMarket, "ipos-calendar".to_string(), reason)]
            );
        }
    }

    #[test]
    fn primary_market_withholds_a_side_whose_feed_served_a_row_it_could_not_read() {
        let gap_ids = |gaps: &[DataGap]| -> Vec<(String, GapReason)> {
            gaps.iter().map(|g| (g.series_id.clone(), g.reason)).collect()
        };
        // A priced offering with no symbol still counts, alone.
        let symbolless = r#"[
            {"date":"2026-10-01","company":"No Symbol Inc.","exchange":"NASDAQ","actions":"Priced","shares":5000000,"dealSize":80000000},
            {"symbol":"LYNX","date":"2026-08-19","company":"Lyntris Inc.","exchange":"NASDAQ","actions":"Priced","shares":17000000,"dealSize":297500000}
        ]"#;
        let server = MockHttp::serve(vec![reply(200, symbolless), reply(200, MNA_BODY)]);
        let (source, _) = froth_source(&server.base_url);
        let mut gaps = Vec::new();
        let ipos = source
            .fetch_primary_market(&froth_windows(), &mut gaps)
            .unwrap()
            .ipos
            .unwrap();
        assert_eq!((ipos.recent_count, ipos.prior_count), (1, 1));
        assert_eq!(ipos.notable[0].symbol, None);
        assert!(gaps.is_empty(), "{gaps:?}");

        // An offering the reader cannot date withholds the IPO read; a dateless row with no
        // share count (a fund launch) could never have counted, so it does not.
        let undated = r#"[
            {"symbol":"NODT","company":"Undated Corp.","exchange":"NYSE","actions":"Priced","shares":5000000},
            {"symbol":"ETFX","company":"Some ETF","exchange":"NYSE","actions":"Priced","shares":null},
            {"symbol":"LYNX","date":"2026-08-19","company":"Lyntris Inc.","exchange":"NASDAQ","actions":"Priced","shares":17000000}
        ]"#;
        let server = MockHttp::serve(vec![reply(200, undated), reply(200, MNA_BODY)]);
        let (source, rec) = froth_source(&server.base_url);
        let mut gaps = Vec::new();
        let group = source
            .fetch_primary_market(&froth_windows(), &mut gaps)
            .unwrap();
        assert_eq!(group.ipos, None);
        assert!(group.deals.is_some());
        assert_eq!(
            gap_ids(&gaps),
            vec![("ipos-calendar".to_string(), GapReason::Malformed)]
        );
        assert_eq!(recorded_rows(&rec)[0].1, "malformed");

        // Every M&A row is a deal row: one the walk served but could not read withholds the
        // deal read.
        let dateless = r#"[
            {"symbol":"IPXG","companyName":"Inflection Point VII","targetedSymbol":"LUNR","targetedCompanyName":"Intuitive Machines, Inc.","transactionDate":"2026-10-08","link":"https://sec.gov/a"},
            {"symbol":"ZZZ","companyName":"Undated Acquirer","targetedCompanyName":"Undated Target","link":"https://sec.gov/z"},
            {"symbol":"AMZN","companyName":"Amazon","targetedSymbol":"GSAT","targetedCompanyName":"Globalstar, Inc.","transactionDate":"2026-07-31","link":"https://sec.gov/c"}
        ]"#;
        let server = MockHttp::serve(vec![reply(200, IPO_BODY), reply(200, dateless)]);
        let (source, _) = froth_source(&server.base_url);
        let mut gaps = Vec::new();
        let group = source
            .fetch_primary_market(&froth_windows(), &mut gaps)
            .unwrap();
        assert_eq!(group.deals, None);
        assert!(group.ipos.is_some());
        assert_eq!(
            gap_ids(&gaps),
            vec![("mergers-acquisitions-latest".to_string(), GapReason::Malformed)]
        );
    }

    #[test]
    fn primary_market_a_later_page_with_no_readable_row_is_a_failed_page() {
        // The feed is newest first, so a wholly unreadable page after one that reached the
        // prior window holds only rows older than the recent window: the recent count stands,
        // the prior count is not derivable, and no gap is recorded (completion state 2).
        let server = MockHttp::serve(vec![
            reply(200, IPO_BODY),
            reply(200, full_mna_page("2026-09-01")),
            reply(200, r#"[{"symbol":"NODATE"}]"#),
        ]);
        let (source, rec) = froth_source(&server.base_url);
        let mut gaps = Vec::new();
        let deals = source
            .fetch_primary_market(&froth_windows(), &mut gaps)
            .unwrap()
            .deals
            .unwrap();
        assert_eq!((deals.recent_count, deals.prior_count), (0, None));
        assert!(gaps.is_empty(), "{gaps:?}");
        let statuses: Vec<String> = recorded_rows(&rec).into_iter().map(|r| r.1).collect();
        assert_eq!(statuses, vec!["ok", "ok", "malformed"]);
    }

    #[test]
    fn primary_market_deal_walk_maps_its_ending_onto_the_completion_states() {
        // State 2: a full page reaching into the prior window, then a failed page — the
        // recent count alone, no gap.
        let server = MockHttp::serve(vec![
            reply(200, IPO_BODY),
            reply(200, full_mna_page("2026-09-01")),
            reply(403, "Forbidden"),
        ]);
        let (source, rec) = froth_source(&server.base_url);
        let mut gaps = Vec::new();
        let deals = source
            .fetch_primary_market(&froth_windows(), &mut gaps)
            .unwrap()
            .deals
            .unwrap();
        assert_eq!((deals.recent_count, deals.prior_count), (0, None));
        assert!(gaps.is_empty(), "{gaps:?}");
        let statuses: Vec<String> = recorded_rows(&rec).into_iter().map(|r| r.1).collect();
        assert_eq!(statuses, vec!["ok", "ok", "rejected"]);

        // State 3: a full page inside the recent window, then a failed page — no deal read
        // and one gap carrying the failure's reason.
        let server = MockHttp::serve(vec![
            reply(200, IPO_BODY),
            reply(200, full_mna_page("2026-10-01")),
            reply(403, "Forbidden"),
        ]);
        let (source, _) = froth_source(&server.base_url);
        let mut gaps = Vec::new();
        let group = source
            .fetch_primary_market(&froth_windows(), &mut gaps)
            .unwrap();
        assert_eq!(group.deals, None);
        assert!(group.ipos.is_some());
        let recorded: Vec<(&str, GapReason)> = gaps
            .iter()
            .map(|g| (g.series_id.as_str(), g.reason))
            .collect();
        assert_eq!(recorded, vec![("mergers-acquisitions-latest", GapReason::Rejected)]);

        // An empty first page is a fault of the feed, never a quiet market.
        let server = MockHttp::serve(vec![reply(200, IPO_BODY), reply(200, "[]")]);
        let (source, _) = froth_source(&server.base_url);
        let mut gaps = Vec::new();
        let group = source
            .fetch_primary_market(&froth_windows(), &mut gaps)
            .unwrap();
        assert_eq!(group.deals, None);
        let recorded: Vec<(&str, GapReason)> = gaps
            .iter()
            .map(|g| (g.series_id.as_str(), g.reason))
            .collect();
        assert_eq!(recorded, vec![("mergers-acquisitions-latest", GapReason::Unavailable)]);
    }

    #[test]
    fn primary_market_skips_its_requests_once_the_run_is_cancelled() {
        let server = MockHttp::serve(vec![]);
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let ctx = RunContext::new(
            "run",
            Arc::new(crate::progress::RecordingReporter::default()),
            cancelled,
        );
        let source = test_source(&server.base_url).with_context(ctx);
        let mut gaps = Vec::new();
        assert_eq!(source.fetch_primary_market(&froth_windows(), &mut gaps), None);
        assert!(gaps.is_empty());
        assert!(server.request_targets().is_empty());
    }

    fn recorded_rows(rec: &crate::progress::RecordingReporter) -> Vec<(String, String)> {
        rec.messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished {
                    series_id, status, ..
                } => Some((series_id, status)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn history_clock_bounds_the_fetch_in_utc_and_reads_freshness_on_the_et_session() {
        // 02:00 UTC on Oct 13 is still Oct 12's evening in New York.
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-13T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let clock = HistoryClock::at(now);
        assert_eq!(clock.to, NaiveDate::from_ymd_opt(2026, 10, 13).unwrap());
        assert_eq!(clock.from, NaiveDate::from_ymd_opt(2025, 10, 13).unwrap());
        assert_eq!(clock.session, NaiveDate::from_ymd_opt(2026, 10, 12).unwrap());
    }

    #[test]
    fn series_token_slugs_a_group_name() {
        assert_eq!(series_token("Oil & Gas Midstream"), "oil-gas-midstream");
        assert_eq!(series_token("Banks - Regional"), "banks-regional");
        assert_eq!(series_token("Energy"), "energy");
    }

    #[test]
    fn history_points_shape_each_family_under_the_integrity_contract() {
        // Sector P/E rides the shared shaper: another sector's row, a non-positive print, a
        // print past SECTOR_PE_MAX and an undatable row all drop.
        let sector_pe = serde_json::json!([
            {"date":"2026-10-12","sector":"Energy","exchange":"NYSE","pe":17.1},
            {"date":"2026-10-09","sector":"Technology","exchange":"NYSE","pe":30.0},
            {"date":"2026-10-08","sector":"Energy","exchange":"NYSE","pe":0.0},
            {"date":"2026-10-07","sector":"Energy","exchange":"NYSE","pe":SECTOR_PE_MAX + 1.0},
            {"date":"not a date","sector":"Energy","exchange":"NYSE","pe":18.0}
        ]);
        let points =
            history_points_from_value(&sector_pe, HistoryFamily::SectorPe, "Energy", "NYSE")
                .unwrap();
        assert_eq!(
            points,
            [DatedPoint {
                date: NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
                value: 17.1
            }]
        );
        // Industry P/E: the INDUSTRY_PE_MAX band, a numeric string, another industry.
        let industry_pe = serde_json::json!([
            {"date":"2026-10-12","industry":"Oil & Gas Midstream","exchange":"NYSE","pe":110.0},
            {"date":"2026-10-09","industry":"Oil & Gas Midstream","exchange":"NYSE","pe":"15.5"},
            {"date":"2026-10-08","industry":"Oil & Gas Midstream","exchange":"NYSE","pe":INDUSTRY_PE_MAX + 1.0},
            {"date":"2026-10-07","industry":"Oil & Gas Refining","exchange":"NYSE","pe":12.0}
        ]);
        let points = history_points_from_value(
            &industry_pe,
            HistoryFamily::IndustryPe,
            "Oil & Gas Midstream",
            "NYSE",
        )
        .unwrap();
        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        assert_eq!(values, [110.0, 15.5]);
        // Performance: any finite move survives the shaper (the ±25 guard is the
        // derivation's); a non-finite string or a missing value drops.
        let perf = serde_json::json!([
            {"date":"2026-10-12","sector":"Energy","exchange":"NYSE","averageChange":30.0},
            {"date":"2026-10-09","sector":"Energy","exchange":"NYSE","averageChange":"NaN"},
            {"date":"2026-10-08","sector":"Energy","exchange":"NYSE"},
            {"date":"2026-10-07","sector":"Energy","exchange":"NYSE","averageChange":"-0.6"}
        ]);
        let points =
            history_points_from_value(&perf, HistoryFamily::SectorReturn, "Energy", "NYSE")
                .unwrap();
        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        assert_eq!(values, [30.0, -0.6]);
        // A row served under another board, or none, fails the whole body; so does a
        // non-array body.
        for family in [
            HistoryFamily::SectorPe,
            HistoryFamily::SectorReturn,
            HistoryFamily::IndustryPe,
            HistoryFamily::IndustryReturn,
        ] {
            let key = family.name_key();
            let off_board = serde_json::json!([
                {"date":"2026-10-12", key:"X", "exchange":"NASDAQ", "pe":10.0, "averageChange":0.1}
            ]);
            assert!(history_points_from_value(&off_board, family, "X", "NYSE").is_err());
            let boardless = serde_json::json!([
                {"date":"2026-10-12", key:"X", "pe":10.0, "averageChange":0.1}
            ]);
            assert!(history_points_from_value(&boardless, family, "X", "NYSE").is_err());
            let object = serde_json::json!({"Error":"drifted"});
            assert!(history_points_from_value(&object, family, "X", "NYSE").is_err());
        }
    }

    #[test]
    fn sector_history_attaches_both_reads_and_stops_silently_below_the_paid_tier() {
        let server = MockHttp::serve(vec![
            // Energy P/E: a full window → a read.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: history_body("sector", "Energy", "NYSE", "pe", 60, "2026-10-12", 17.0),
            },
            // Technology P/E: below the sample floor → that row's gap alone.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: history_body("sector", "Technology", "NYSE", "pe", 30, "2026-10-12", 30.0),
            },
            // Energy return: a key below the paid tier → the family stops, no gap.
            Canned::Reply {
                status: 402,
                headers: vec![],
                body: "Premium Query Parameter",
            },
        ]);
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let source = test_source(&server.base_url).with_context(ctx);
        let mut rows = vec![sector_row("Energy", "NYSE"), sector_row("Technology", "NYSE")];
        let mut gaps = Vec::new();

        source.attach_sector_history(&mut rows, history_clock(), &mut gaps);

        let energy = rows[0].pe_history.as_ref().expect("Energy P/E history");
        assert_eq!(energy.percentile, 100.0);
        assert_eq!((energy.min, energy.median, energy.max), (17.0, 17.0, 17.0));
        assert_eq!((energy.start, energy.latest), (17.0, 17.0));
        assert_eq!((energy.from.as_str(), energy.to.as_str()), ("2026-08-14", "2026-10-12"));
        assert_eq!(rows[1].pe_history, None);
        assert!(rows.iter().all(|r| r.trailing_return.is_none()));
        assert_eq!(rows[0].pe, Some(20.0), "the snapshot level is untouched");
        let recorded: Vec<(GroupKind, &str, GapReason)> = gaps
            .iter()
            .map(|g| (g.group, g.series_id.as_str(), g.reason))
            .collect();
        assert_eq!(
            recorded,
            [(
                GroupKind::SectorPe,
                "sector-pe-history-nyse-technology",
                GapReason::Unavailable
            )]
        );
        // NASDAQ carried no rows, so no request went there; the 402 stopped Technology's
        // return before it was asked.
        assert_eq!(
            server.request_paths(),
            [
                "/historical-sector-pe",
                "/historical-sector-pe",
                "/historical-sector-performance"
            ]
        );
        let target = &server.request_targets()[0];
        for param in ["sector=Energy", "exchange=NYSE", "from=2025-10-12", "to=2026-10-12"] {
            assert!(target.contains(param), "{target}");
        }
        assert_eq!(
            recorded_rows(&rec),
            [
                ("sector-pe-history-nyse-energy".to_string(), "ok".to_string()),
                (
                    "sector-pe-history-nyse-technology".to_string(),
                    "unavailable".to_string()
                ),
                (
                    "sector-return-history-nyse-energy".to_string(),
                    "empty".to_string()
                ),
            ]
        );
    }

    #[test]
    fn industry_history_stops_a_family_on_a_this_run_failure_and_gaps_a_row_otherwise() {
        let server = MockHttp::serve(vec![
            // NASDAQ P/E, Alpha: rejected → one family gap, Beta never asked.
            Canned::Reply {
                status: 403,
                headers: vec![],
                body: "Forbidden",
            },
            // NASDAQ return, Alpha: a drifted body → that row's malformed gap.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"unexpected":true}"#,
            },
            // NASDAQ return, Beta: a 404 → that row's out-of-scope gap.
            Canned::Reply {
                status: 404,
                headers: vec![],
                body: "Not Found",
            },
            // NYSE P/E, Oil & Gas Midstream: an empty body → that row's gap.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "[]",
            },
            // NYSE return, Oil & Gas Midstream: a full window of +0.1pp days → a read.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: history_body(
                    "industry",
                    "Oil & Gas Midstream",
                    "NYSE",
                    "averageChange",
                    60,
                    "2026-10-09",
                    0.1,
                ),
            },
        ]);
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let source = test_source(&server.base_url).with_context(ctx);
        let mut rows = vec![
            industry_row("Alpha", "NASDAQ"),
            industry_row("Beta", "NASDAQ"),
            industry_row("Oil & Gas Midstream", "NYSE"),
        ];
        let mut gaps = Vec::new();

        source.attach_industry_history(&mut rows, history_clock(), &mut gaps);

        assert!(rows.iter().all(|r| r.pe_history.is_none()));
        assert!(rows[0].trailing_return.is_none() && rows[1].trailing_return.is_none());
        let midstream = rows[2].trailing_return.as_ref().expect("a return read");
        assert!((midstream.return_pct - (1.001f64.powi(60) - 1.0) * 100.0).abs() < 1e-9);
        assert_eq!(midstream.to, "2026-10-09");
        let recorded: Vec<(GroupKind, &str, GapReason)> = gaps
            .iter()
            .map(|g| (g.group, g.series_id.as_str(), g.reason))
            .collect();
        assert_eq!(
            recorded,
            [
                (
                    GroupKind::Industries,
                    "industry-pe-history-nasdaq",
                    GapReason::Rejected
                ),
                (
                    GroupKind::Industries,
                    "industry-return-history-nasdaq-alpha",
                    GapReason::Malformed
                ),
                (
                    GroupKind::Industries,
                    "industry-return-history-nasdaq-beta",
                    GapReason::OutOfScope
                ),
                (
                    GroupKind::Industries,
                    "industry-pe-history-nyse-oil-gas-midstream",
                    GapReason::Unavailable
                ),
            ]
        );
        assert_eq!(gaps[0].series_name, "Industry P/E history (NASDAQ)");
        assert_eq!(gaps[3].series_name, "Oil & Gas Midstream P/E history (NYSE)");
        assert!(
            server.request_targets()[3].contains("industry=Oil+%26+Gas+Midstream")
                || server.request_targets()[3].contains("industry=Oil%20%26%20Gas%20Midstream"),
            "{}",
            server.request_targets()[3]
        );
        let statuses: Vec<String> = recorded_rows(&rec).into_iter().map(|(_, s)| s).collect();
        assert_eq!(
            statuses,
            ["rejected", "malformed", "out-of-scope", "unavailable", "ok"]
        );
    }

    #[test]
    fn history_family_stops_on_an_error_envelope_or_a_transport_failure() {
        let names = vec!["Energy".to_string(), "Technology".to_string()];
        let recorded = |gaps: &[DataGap]| -> Vec<(GroupKind, String, GapReason)> {
            gaps.iter()
                .map(|g| (g.group, g.series_id.clone(), g.reason))
                .collect()
        };

        // A 200 carrying FMP's error envelope is a rejection: one family gap, and the next
        // sector is never asked.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"Error Message":"Limit Reach . Please upgrade your plan"}"#,
        }]);
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let source = test_source(&server.base_url).with_context(ctx);
        let mut gaps = Vec::new();
        let reads = source.fetch_history_family(
            HistoryFamily::SectorPe,
            "NYSE",
            &names,
            history_clock(),
            group_history::pe_history,
            &mut gaps,
        );
        assert!(reads.is_empty());
        assert_eq!(server.attempts(), 1);
        assert_eq!(
            recorded(&gaps),
            [(
                GroupKind::SectorPe,
                "sector-pe-history-nyse".to_string(),
                GapReason::Rejected
            )]
        );
        assert_eq!(gaps[0].series_name, "Sector P/E history (NYSE)");
        let statuses: Vec<String> = recorded_rows(&rec).into_iter().map(|(_, s)| s).collect();
        assert_eq!(statuses, ["rejected"]);

        // A transport failure past the retry ladder: one unavailable family gap, the loop
        // stopped. The millisecond schedule keeps the retries instant.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let source = test_source("http://127.0.0.1:1")
            .with_retry_policy(crate::http_retry::RetryPolicy {
                rate_limit: crate::http_retry::Schedule {
                    attempts: 2,
                    base: StdDuration::from_millis(1),
                },
                server_error: crate::http_retry::Schedule {
                    attempts: 2,
                    base: StdDuration::from_millis(1),
                },
                retry_after_cap: StdDuration::from_millis(50),
            })
            .with_context(ctx);
        let mut gaps = Vec::new();
        let reads = source.fetch_history_family(
            HistoryFamily::SectorReturn,
            "NYSE",
            &names,
            history_clock(),
            group_history::trailing_return,
            &mut gaps,
        );
        assert!(reads.is_empty());
        assert_eq!(
            recorded(&gaps),
            [(
                GroupKind::SectorPe,
                "sector-return-history-nyse".to_string(),
                GapReason::Unavailable
            )]
        );
        let rows = recorded_rows(&rec);
        assert_eq!(rows.len(), 1, "the loop stopped after the first group");
        assert_eq!(rows[0].1, "unavailable");
    }

    #[test]
    fn history_attach_makes_no_request_after_a_cancel() {
        let server = MockHttp::serve(vec![]);
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        );
        let source = test_source(&server.base_url).with_context(ctx);
        let mut sectors = vec![sector_row("Energy", "NYSE")];
        let mut industries = vec![industry_row("Alpha", "NASDAQ")];
        let mut gaps = Vec::new();
        source.attach_sector_history(&mut sectors, history_clock(), &mut gaps);
        source.attach_industry_history(&mut industries, history_clock(), &mut gaps);
        assert_eq!(server.attempts(), 0);
        assert!(gaps.is_empty());
        assert!(sectors[0].pe_history.is_none() && industries[0].trailing_return.is_none());
    }

    #[test]
    fn fmp_ladder_crosses_a_minute_window_and_is_the_constructor_default() {
        // 1+2+4+8+16+32 = 63s, past any 60s rate window.
        assert_eq!(
            FMP_RETRY.cumulative_rate_limit_backoff(),
            StdDuration::from_secs(63)
        );
        assert!(FMP_RETRY.cumulative_rate_limit_backoff() > StdDuration::from_secs(60));
        let source = FmpDataSource::new("k".into()).expect("adapter");
        assert_eq!(source.retry, FMP_RETRY);
    }

    #[test]
    fn get_rides_the_injected_retry_policy_past_a_429_burst() {
        // Four 429s then a clean quote: the server-error budget (3) would stop at
        // attempt 3, so riding through proves `get` consumes the rate-limit class.
        let mut replies: Vec<Canned> = (0..4)
            .map(|_| Canned::Reply {
                status: 429,
                headers: vec![],
                body: "limit",
            })
            .collect();
        replies.push(Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"[{"symbol":"AAPL","name":"Apple","price":195.0,"changePercentage":0.5}]"#,
        });
        let server = MockHttp::serve(replies);
        let source =
            test_source(&server.base_url).with_retry_policy(crate::http_retry::RetryPolicy {
                rate_limit: crate::http_retry::Schedule {
                    attempts: 5,
                    base: StdDuration::from_millis(1),
                },
                server_error: crate::http_retry::Schedule {
                    attempts: 3,
                    base: StdDuration::from_millis(1),
                },
                retry_after_cap: StdDuration::from_millis(50),
            });
        let mut gaps = Vec::new();
        let quotes =
            source.fetch_quotes(&[("AAPL", "Apple", "pts")], GroupKind::Indices, &mut gaps);
        assert_eq!(server.attempts(), 5, "all four 429s must be retried past");
        assert_eq!(
            quotes.len(),
            1,
            "the ladder must ride through to the parsed quote; gaps={gaps:?}"
        );
        assert!(gaps.is_empty());
    }

    #[test]
    fn a_suite_row_carries_the_gap_reason_and_transport_detail() {
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        // An unroutable endpoint: a transport error on every attempt.
        let source = test_source("http://127.0.0.1:1")
            .with_retry_policy(crate::http_retry::RetryPolicy {
                rate_limit: crate::http_retry::Schedule {
                    attempts: 2,
                    base: StdDuration::from_millis(1),
                },
                server_error: crate::http_retry::Schedule {
                    attempts: 2,
                    base: StdDuration::from_millis(1),
                },
                retry_after_cap: StdDuration::from_millis(50),
            })
            .with_context(ctx);
        let mut gaps = Vec::new();
        let income = source.fetch_quarterly_income("AAPL", &mut gaps, &mut vec![]);
        assert!(income.is_empty());
        let finished: Vec<(String, Option<String>)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished { status, detail, .. } => {
                    Some((status, detail))
                }
                _ => None,
            })
            .collect();
        assert_eq!(finished.len(), 1, "one suite row for the one fetch");
        assert_eq!(finished[0].0, "unavailable", "the gap reason is the status");
        let detail = finished[0].1.as_deref().expect(
            "the transport error reaches the row's detail",
        );
        // The detail must never carry the request URL: the key rides as a query param,
        // and this string reaches stderr and the tracker.
        assert!(!detail.contains("test-key"), "{detail}");
        assert!(!detail.contains("apikey"), "{detail}");
    }

    #[test]
    fn a_suite_row_carries_the_http_level_cause_too() {
        // FMP's plan/quota signal arrives as HTTP 200 with an `{"Error Message"}` body,
        // a 403 as a rejection with a plain body; both must leave their cause on the row.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"Error Message": "Limit Reach. Please upgrade your plan."}"#,
            },
            Canned::Reply {
                status: 403,
                headers: vec![],
                body: "Forbidden",
            },
        ]);
        let source = test_source(&server.base_url).with_context(ctx);
        let mut gaps = Vec::new();
        assert!(source
            .fetch_quarterly_income("AAPL", &mut gaps, &mut vec![])
            .is_empty());
        assert!(source
            .fetch_quarterly_income("AAPL", &mut gaps, &mut vec![])
            .is_empty());
        let finished: Vec<(String, Option<String>)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished { status, detail, .. } => {
                    Some((status, detail))
                }
                _ => None,
            })
            .collect();
        assert_eq!(finished.len(), 2, "one suite row per fetch");
        assert_eq!(finished[0].0, "rejected", "{:?}", finished[0]);
        let detail = finished[0].1.as_deref().expect("the plan signal reaches the row");
        assert!(detail.contains("HTTP 200"), "{detail}");
        assert!(detail.contains("Limit Reach"), "{detail}");
        assert_eq!(finished[1].0, "rejected", "{:?}", finished[1]);
        let detail = finished[1].1.as_deref().expect("the 403 reaches the row");
        assert!(detail.contains("HTTP 403"), "{detail}");
        assert!(detail.contains("Forbidden"), "{detail}");
    }

    #[test]
    fn quote_and_eod_rows_reflect_the_shaped_outcome() {
        // A parseable body is not success: a row is ok only when data landed, and an
        // HTTP-level rejection carries its cause.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"Error Message": "Limit Reach. Please upgrade your plan."}"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "[]",
            },
        ]);
        let source = test_source(&server.base_url).with_context(ctx);
        let fin = source.fetch_quote_and_eod("AAPL");
        assert!(fin.current_price.is_none());
        assert!(fin.price_history.is_empty());
        let rows: Vec<(String, String, Option<String>)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished {
                    group,
                    status,
                    detail,
                    ..
                } => Some((group, status, detail)),
                _ => None,
            })
            .collect();
        let quote = rows.iter().find(|r| r.0 == "company-quote").expect("quote row");
        assert_eq!(quote.1, "rejected", "{quote:?}");
        assert!(
            quote.2.as_deref().is_some_and(|d| d.contains("Limit Reach")),
            "{quote:?}"
        );
        let eod = rows.iter().find(|r| r.0 == "company-eod").expect("eod row");
        assert_eq!(eod.1, "empty", "a parsed-but-empty history is not ok: {eod:?}");
    }

    #[test]
    fn a_non_positive_quote_print_is_no_usable_price_at_the_parse() {
        let shaped = |body: &str| {
            company_quote_from_value(&serde_json::from_str(body).unwrap())
                .expect("a non-empty array parses")
        };
        let zero = shaped(r#"[{"symbol":"AAPL","price":0,"marketCap":1.0}]"#);
        assert_eq!(zero.price, None);
        assert_eq!(zero.unusable_price, Some(0.0));
        assert_eq!(zero.market_cap, Some(1.0), "the other fields still land");
        let negative = shaped(r#"[{"symbol":"AAPL","price":-5.25}]"#);
        assert_eq!(negative.price, None);
        assert_eq!(negative.unusable_price, Some(-5.25));
        let usable = shaped(r#"[{"symbol":"AAPL","price":195.0}]"#);
        assert_eq!(usable.price, Some(195.0));
        assert_eq!(usable.unusable_price, None);
        let absent = shaped(r#"[{"symbol":"AAPL","marketCap":1.0}]"#);
        assert_eq!(absent.price, None);
        assert_eq!(absent.unusable_price, None, "absent is not unusable");
    }

    #[test]
    fn an_unusable_quote_print_lands_as_a_named_gap_on_every_consumer() {
        // Every row reads empty, never ok, never malformed.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            // fetch_quote_and_eod: a zero print, then the EOD leg.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","price":0,"marketCap":3.0e12}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "[]",
            },
            // fetch_live_price: a negative print.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","price":-1.5}]"#,
            },
            // fetch_commodity_quote: a zero print.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"GCUSD","price":0}]"#,
            },
        ]);
        let source = test_source(&server.base_url).with_context(ctx);
        let fin = source.fetch_quote_and_eod("AAPL");
        assert_eq!(fin.current_price, None, "a zero print is no price");
        assert_eq!(fin.market_cap, Some(3.0e12), "the other quote fields still land");
        assert!(
            fin.gaps
                .iter()
                .any(|g| g == "FMP quote carried no usable price (served 0)"),
            "{:?}",
            fin.gaps
        );
        let err = source
            .fetch_live_price("AAPL")
            .expect_err("a negative print is Err — the family types unknown");
        assert!(
            err.to_string().contains("no usable price for AAPL (served -1.5)"),
            "{err}"
        );
        let err = source
            .fetch_commodity_quote("GCUSD", chrono::NaiveDate::from_ymd_opt(2026, 8, 28).unwrap())
            .expect_err("a zero print is Err — the caller's typed gap");
        assert!(err.to_string().contains("no usable price (served 0)"), "{err}");

        let rows: Vec<(String, String, Option<String>)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished {
                    group,
                    status,
                    detail,
                    ..
                } => Some((group, status, detail)),
                _ => None,
            })
            .collect();
        for group in ["company-quote", "quick-quote", "commodity-quote"] {
            let row = rows.iter().find(|r| r.0 == group).expect(group);
            assert_eq!(row.1, "empty", "{row:?}");
            assert!(
                row.2
                    .as_deref()
                    .is_some_and(|d| d.contains("no usable price") && d.contains("(served ")),
                "{row:?}"
            );
        }
    }

    #[test]
    fn every_suite_row_carries_the_shaped_outcome_not_ok_on_parse() {
        // A parsed-but-priceless quote is "empty" while the caller still errors.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            // 1) fetch_quarterly_income: a served-but-empty array.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "[]",
            },
            // 2) fetch_live_price: a parsed quote carrying no price.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","marketCap":1.0}]"#,
            },
            // 3) fetch_live_price: an unshapeable (non-array) body.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"unexpected":"shape"}"#,
            },
            // 4) fetch_sector_pe_snapshot: the empty answer is a 200.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "[]",
            },
        ]);
        let source = test_source(&server.base_url).with_context(ctx);

        let mut gaps = Vec::new();
        assert!(source
            .fetch_quarterly_income("AAPL", &mut gaps, &mut vec![])
            .is_empty());
        assert_eq!(gaps.len(), 1, "the empty read still records its gap");
        assert!(
            source.fetch_live_price("AAPL").is_err(),
            "priceless stays Err"
        );
        assert!(
            source.fetch_live_price("AAPL").is_err(),
            "malformed stays Err"
        );
        assert_eq!(
            source
                .fetch_sector_pe_snapshot("NASDAQ", "2026-08-10")
                .expect("an empty snapshot is Ok(vec![])")
                .len(),
            0
        );

        let rows: Vec<(String, String, Option<String>)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished {
                    group,
                    status,
                    detail,
                    ..
                } => Some((group, status, detail)),
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 4, "{rows:?}");
        assert_eq!(rows[0].0, "company-income-q");
        assert_eq!(rows[0].1, "empty", "{:?}", rows[0]);
        assert_eq!(rows[1].0, "quick-quote");
        assert_eq!(rows[1].1, "empty", "a priceless quote is not ok: {:?}", rows[1]);
        assert_eq!(rows[2].0, "quick-quote");
        assert_eq!(rows[2].1, "malformed", "{:?}", rows[2]);
        assert!(
            rows[2].2.as_deref().is_some_and(|d| d.contains("malformed")),
            "the cause reaches the row: {:?}",
            rows[2]
        );
        assert_eq!(rows[3].0, "sector-pe");
        assert_eq!(
            rows[3].1, "empty",
            "the walk-back's empty 200 answer is visible on the row: {:?}",
            rows[3]
        );
    }

    #[test]
    fn schema_drift_reads_malformed_with_its_cause_never_benign_empty() {
        // The shared shapers fold a non-array body into an empty result and a
        // fieldless body into a default, so only the per-site drift checks keep a
        // drifted 200 from reading as a benign `empty` or `ok`.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            // 1) fetch_quarterly_income: valid JSON, wrong shape.
            Canned::Reply { status: 200, headers: vec![], body: r#"{"unexpected":"shape"}"# },
            // 2) fetch_balance_sheet: a served row with no reportedCurrency — the
            //    unit gate rejects the surface before any row is read (empty with
            //    the currency cause on the row).
            Canned::Reply { status: 200, headers: vec![], body: "[{}]" },
            // 3-6) fetch_fund_data: fieldless info, drifted profile, drifted
            // sectors, empty countries.
            Canned::Reply { status: 200, headers: vec![], body: "{}" },
            Canned::Reply { status: 200, headers: vec![], body: "5" },
            Canned::Reply { status: 200, headers: vec![], body: r#"{"x":1}"# },
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            // 7-8) fetch_quote_and_eod: both drifted.
            Canned::Reply { status: 200, headers: vec![], body: "{}" },
            Canned::Reply { status: 200, headers: vec![], body: r#"{"y":2}"# },
            // 9) fetch_sector_pe_snapshot: drifted, return contract unchanged.
            Canned::Reply { status: 200, headers: vec![], body: r#"{"z":3}"# },
        ]);
        let source = test_source(&server.base_url).with_context(ctx);

        let mut gaps = Vec::new();
        assert!(source
            .fetch_quarterly_income("AAPL", &mut gaps, &mut vec![])
            .is_empty());
        assert!(
            gaps.iter().any(|g| g.contains("were malformed")),
            "the gap names the drift, not emptiness: {gaps:?}"
        );
        assert!(source
            .fetch_balance_sheet("AAPL", &mut gaps, &mut vec![])
            .is_empty());
        let fund = source.fetch_fund_data("SPY");
        assert!(
            fund.gaps.iter().any(|g| g.contains("were malformed")),
            "{:?}",
            fund.gaps
        );
        // The drifted profile body must land its gap on the record itself, not only
        // the tracker row: `suite_get_shaped` folds malformed into Ok, so the shaper
        // pushes it.
        assert!(
            fund.gaps
                .iter()
                .any(|g| g.starts_with(FUND_PROFILE_GAP_PREFIX)
                    && g.contains("closed-end detection cannot run")),
            "{:?}",
            fund.gaps
        );
        let fin = source.fetch_quote_and_eod("AAPL");
        assert!(fin.current_price.is_none());
        assert!(
            source
                .fetch_sector_pe_snapshot("NASDAQ", "2026-08-10")
                .expect("a drifted body keeps the Ok(vec![]) walk-back contract")
                .is_empty()
        );

        let rows: Vec<(String, String, Option<String>)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished { group, status, detail, .. } => {
                    Some((group, status, detail))
                }
                _ => None,
            })
            .collect();
        let expect_status = |group: &str, status: &str| {
            let row = rows.iter().find(|r| r.0 == group).unwrap_or_else(|| panic!("{group} row"));
            assert_eq!(row.1, status, "{row:?}");
            row
        };
        let income = expect_status("company-income-q", "malformed");
        assert!(
            income.2.as_deref().is_some_and(|d| d.contains("non-array")),
            "the cause reaches the row: {income:?}"
        );
        let balance = expect_status("company-balance", "empty");
        assert!(
            balance.2.as_deref().is_some_and(|d| d.contains("not verified USD")),
            "the cause reaches the row: {balance:?}"
        );
        expect_status("fund-info", "empty");
        expect_status("fund-profile", "malformed");
        let sectors = expect_status("fund-sectors", "malformed");
        assert!(sectors.2.is_some(), "{sectors:?}");
        expect_status("fund-countries", "empty");
        let quote = expect_status("company-quote", "malformed");
        assert!(quote.2.is_some(), "{quote:?}");
        let eod = expect_status("company-eod", "malformed");
        assert!(
            eod.2.as_deref().is_some_and(|d| d.contains("array shape")),
            "the parser's cause reaches the row: {eod:?}"
        );
        let pe = expect_status("sector-pe", "malformed");
        assert!(pe.2.is_some(), "{pe:?}");
    }

    #[test]
    fn unreadable_rows_read_malformed_and_served_empty_reads_empty() {
        // The shapers drop unreadable rows silently, so the sites re-split on the
        // body: a served non-empty array with no readable row is drift, a served `[]`
        // (an unknown symbol, a non-fund) is the honest empty.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            // 1) income: rows served, none readable (dateless).
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"reportedCurrency":"USD"}]"#,
            },
            // 2) estimates: rows served, none datable → malformed.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "[{}]",
            },
            // 3) estimates: datable but past-only → the honest empty.
            Canned::Reply { status: 200, headers: vec![], body: r#"[{"date":"2020-01-01"}]"# },
            // 4-5) quote + EOD: both served empty.
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            // 6) quick-quote: served empty.
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            // 7-10) fund: info + profile served empty; sectors unreadable;
            // countries readable.
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            Canned::Reply { status: 200, headers: vec![], body: "[{}]" },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"country":"US","weightPercentage":"97%"}]"#,
            },
        ]);
        let source = test_source(&server.base_url).with_context(ctx);

        let mut gaps = Vec::new();
        assert!(source
            .fetch_quarterly_income("AAPL", &mut gaps, &mut vec![])
            .is_empty());
        assert!(
            gaps.iter().any(|g| g.contains("were malformed")),
            "{gaps:?}"
        );
        gaps.clear();
        assert!(source.fetch_analyst_estimates("AAPL", &mut gaps).is_none());
        assert!(gaps.iter().any(|g| g.contains("no datable rows")), "{gaps:?}");
        gaps.clear();
        assert!(source.fetch_analyst_estimates("AAPL", &mut gaps).is_none());
        assert!(
            gaps.iter().any(|g| g.contains("no forward-dated consensus")),
            "past-only stays the honest empty: {gaps:?}"
        );
        let fin = source.fetch_quote_and_eod("AAPL");
        assert!(
            fin.gaps.iter().any(|g| g == "FMP quote was empty"),
            "{:?}",
            fin.gaps
        );
        let err = source.fetch_live_price("AAPL").expect_err("no rows is still Err");
        assert!(err.to_string().contains("was empty"), "{err}");
        let fund = source.fetch_fund_data("SPY");
        assert!(
            fund.gaps
                .iter()
                .any(|g| g.contains("sector") && g.contains("were malformed")),
            "{:?}",
            fund.gaps
        );
        assert_eq!(fund.country_weights, vec![("US".to_string(), 0.97)]);

        let rows: Vec<(String, String, Option<String>)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished { group, status, detail, .. } => {
                    Some((group, status, detail))
                }
                _ => None,
            })
            .collect();
        let statuses: Vec<(&str, &str)> =
            rows.iter().map(|r| (r.0.as_str(), r.1.as_str())).collect();
        assert_eq!(
            statuses,
            vec![
                ("company-income-q", "malformed"),
                ("company-estimates", "malformed"),
                ("company-estimates", "empty"),
                ("company-quote", "empty"),
                ("company-eod", "empty"),
                ("quick-quote", "empty"),
                ("fund-info", "empty"),
                ("fund-profile", "empty"),
                ("fund-sectors", "malformed"),
                ("fund-countries", "ok"),
            ],
            "{rows:?}"
        );
        assert!(
            rows[0].2.as_deref().is_some_and(|d| d.contains("no served row was readable")),
            "{:?}",
            rows[0]
        );
        assert!(
            rows[1].2.as_deref().is_some_and(|d| d.contains("datable")),
            "{:?}",
            rows[1]
        );
    }

    #[test]
    fn strict_list_fetches_error_on_wholly_unreadable_bodies_but_not_filtered_ones() {
        // A wholly unreadable earnings/news body read as empty would type the quick
        // check's family FreshClear instead of Unknown, suppressing a selective rerun.
        // The news leg splits on readability, not row count: its shaper also
        // date-filters, and readable-but-older-than-`from` rows stay the honest empty.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            // 1) earnings: rows served, none readable.
            Canned::Reply { status: 200, headers: vec![], body: "[{}]" },
            // 2) news: rows served, none readable.
            Canned::Reply { status: 200, headers: vec![], body: "[{}]" },
            // 3) news: readable but pre-`from` — domain-filtered, honest empty.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"publishedDate":"2020-01-01 10:00:00","title":"old"}]"#,
            },
            // 4) deep EOD: rows served, none readable.
            Canned::Reply { status: 200, headers: vec![], body: "[{}]" },
            // 5-6) quote ok, EOD rows served but unreadable.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","price":5.0}]"#,
            },
            Canned::Reply { status: 200, headers: vec![], body: "[{}]" },
        ]);
        let source = test_source(&server.base_url).with_context(ctx);

        let err = source.fetch_symbol_earnings("AAPL").expect_err("drift is Err");
        assert!(err.to_string().contains("no served row was readable"), "{err}");
        let err = source
            .fetch_symbol_news_since("AAPL", "2026-08-01")
            .expect_err("drift is Err");
        assert!(err.to_string().contains("no served row was readable"), "{err}");
        assert!(
            source
                .fetch_symbol_news_since("AAPL", "2026-08-01")
                .expect("filtered-out old news stays the honest Ok(empty)")
                .is_empty()
        );
        assert!(source.fetch_dated_eod("AAPL", 30).is_err(), "drift is Err");
        let fin = source.fetch_quote_and_eod("AAPL");
        assert!(
            fin.gaps.iter().any(|g| g == "FMP price history was malformed"),
            "{:?}",
            fin.gaps
        );

        let statuses: Vec<(String, String)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished { group, status, .. } => {
                    Some((group, status))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            statuses,
            vec![
                ("quick-earnings".to_string(), "malformed".to_string()),
                ("news-leads".to_string(), "malformed".to_string()),
                ("news-leads".to_string(), "empty".to_string()),
                ("company-eod-deep".to_string(), "malformed".to_string()),
                ("company-quote".to_string(), "ok".to_string()),
                ("company-eod".to_string(), "malformed".to_string()),
            ],
            "{statuses:?}"
        );
    }

    #[test]
    fn noncanonical_dates_normalize_before_lexicographic_consumers() {
        // In source form "2026-9-30" sorts after "2026-12-31" and compares fresh
        // against an October last-pass date.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            // 1-2) quote ok; EOD with mixed-padding dates whose raw-string
            // sort would order them 2.0, 0.5, 1.0.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","price":5.0}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2026-9-30","price":1.0},{"date":"2026-12-31","price":2.0},{"date":"2026-8-01","price":0.5}]"#,
            },
            // 3) deep EOD: a non-zero-padded date must emit canonical ISO.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2026-9-30","price":1.0}]"#,
            },
            // 4) earnings: canonical output, so the quick check's last-pass
            // compare sees fixed-width ISO.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2026-9-30","epsActual":1.0}]"#,
            },
            // 5) news: a September row must NOT read fresh against an
            // October `from` (raw-string compare says it does).
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"publishedDate":"2026-9-30 10:00:00","title":"x"}]"#,
            },
        ]);
        let source = test_source(&server.base_url).with_context(ctx);

        let fin = source.fetch_quote_and_eod("AAPL");
        assert_eq!(
            fin.price_history,
            vec![0.5, 1.0, 2.0],
            "chronological, not raw-string, order"
        );
        let rows = source.fetch_dated_eod("AAPL", 30).expect("datable rows ride");
        assert_eq!(rows[0].date, "2026-09-30", "canonical fixed-width ISO out");
        let rows = source.fetch_symbol_earnings("AAPL").expect("datable rows ride");
        assert_eq!(rows[0].date, "2026-09-30");
        assert!(
            source
                .fetch_symbol_news_since("AAPL", "2026-10-01")
                .expect("readable September news against an October `from`")
                .is_empty(),
            "a September row must not read fresh against an October boundary"
        );
    }

    #[test]
    fn invalid_date_strings_are_unreadable_rows_not_ok_data() {
        // A garbage date like "not-a-date" compares lexicographically above any ISO
        // last-pass date ('n' > '2'), so as a readable row it could fabricate a fresh
        // earnings/news event through the quick check's gate.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            // 1) earnings: a string-dated row that is not a date.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"not-a-date","epsActual":1.0}]"#,
            },
            // 2) news: a blank publishedDate.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"publishedDate":"","title":"x"}]"#,
            },
            // 3) news: a non-date publishedDate (the lexicographic bypass).
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"publishedDate":"not-a-date","title":"x"}]"#,
            },
            // 4) deep EOD: a "bad"-dated priced row.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"bad","price":100.0}]"#,
            },
            // 5-6) quote ok; EOD leg with the same bad-dated row.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","price":5.0}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"bad","price":100.0}]"#,
            },
            // 7) earnings: one datable row among garbage still rides.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2026-08-05","epsActual":1.0},{"date":"junk","epsActual":2.0}]"#,
            },
            // 8) news: a `T`-separated datetime prefix is readable and fresh.
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"publishedDate":"2026-08-05T09:30:00","title":"t"}]"#,
            },
        ]);
        let source = test_source(&server.base_url).with_context(ctx);

        let err = source.fetch_symbol_earnings("AAPL").expect_err("garbage date is drift");
        assert!(err.to_string().contains("no served row was readable"), "{err}");
        assert!(source.fetch_symbol_news_since("AAPL", "2026-08-01").is_err());
        assert!(source.fetch_symbol_news_since("AAPL", "2026-08-01").is_err());
        assert!(source.fetch_dated_eod("AAPL", 30).is_err());
        let fin = source.fetch_quote_and_eod("AAPL");
        assert!(fin.price_history.is_empty());
        assert!(
            fin.gaps.iter().any(|g| g == "FMP price history was malformed"),
            "{:?}",
            fin.gaps
        );
        let rows = source
            .fetch_symbol_earnings("AAPL")
            .expect("a datable row among garbage still rides");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, "2026-08-05");
        let items = source
            .fetch_symbol_news_since("AAPL", "2026-08-01")
            .expect("a T-separated datetime is readable");
        assert_eq!(items.len(), 1);

        let statuses: Vec<(String, String)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished { group, status, .. } => {
                    Some((group, status))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            statuses,
            vec![
                ("quick-earnings".to_string(), "malformed".to_string()),
                ("news-leads".to_string(), "malformed".to_string()),
                ("news-leads".to_string(), "malformed".to_string()),
                ("company-eod-deep".to_string(), "malformed".to_string()),
                ("company-quote".to_string(), "ok".to_string()),
                ("company-eod".to_string(), "malformed".to_string()),
                ("quick-earnings".to_string(), "ok".to_string()),
                ("news-leads".to_string(), "ok".to_string()),
            ],
            "{statuses:?}"
        );
    }

    #[test]
    fn http_gap_detail_caps_and_names_the_empty_body() {
        let long = "x".repeat(1_000);
        let capped = http_gap_detail(500, &long);
        assert!(capped.starts_with("HTTP 500: "), "{capped}");
        assert!(capped.ends_with("…(truncated)"), "{capped}");
        assert!(capped.len() < 400, "{}", capped.len());
        assert_eq!(http_gap_detail(502, "  "), "HTTP 502 (empty body)");
    }

    #[test]
    fn company_quote_and_eod_parse_into_financials() {
        // The EOD body is served out of order and must come back chronological.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","name":"Apple","price":195.0,"marketCap":3.0e12,"sharesOutstanding":1.5e10}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","date":"2026-06-10","price":195.0,"volume":1},
                          {"symbol":"AAPL","date":"2026-06-03","price":180.0,"volume":1}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"reportedCurrency":"USD","date":"2026-03-31","filingDate":"2026-05-01","revenue":95.0e9,
                           "epsDiluted":1.55,"weightedAverageShsOutDil":1.5e10,
                           "netIncome":24.0e9,"grossProfit":44.0e9,"costOfRevenue":51.0e9,
                           "operatingIncome":29.0e9}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"reportedCurrency":"USD","date":"2026-03-31","freeCashFlow":20.0e9,
                           "operatingCashFlow":28.0e9,"capitalExpenditure":-8.0e9}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"reportedCurrency":"USD","date":"2026-03-31","totalDebt":110.0e9,"totalStockholdersEquity":62.0e9,"totalEquity":63.0e9,
                           "cashAndCashEquivalents":30.0e9,"shortTermInvestments":32.0e9,"netReceivables":60.0e9,"inventory":7.0e9}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","altmanZScore":9.32,"piotroskiScore":8}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2027-09-30","epsAvg":6.5,"epsLow":6.0,"epsHigh":7.0,
                           "revenueAvg":430e9,"revenueLow":420e9,"revenueHigh":440e9}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2026-05-10","adjDividend":0.26}]"#,
            },
        ]);
        let fin = test_source(&server.base_url).fetch_company_financials("AAPL");
        assert_eq!(fin.symbol, "AAPL");
        assert_eq!(fin.current_price, Some(195.0));
        assert_eq!(fin.market_cap, Some(3.0e12));
        assert_eq!(fin.shares_outstanding, Some(1.5e10));
        assert_eq!(fin.price_history, vec![180.0, 195.0]);
        assert_eq!(fin.quarterly_income.len(), 1);
        assert_eq!(fin.quarterly_income[0].net_income, Some(24.0e9));
        assert_eq!(fin.quarterly_income[0].gross_profit, Some(44.0e9));
        assert_eq!(fin.quarterly_income[0].cost_of_revenue, Some(51.0e9));
        assert_eq!(fin.quarterly_income[0].operating_income, Some(29.0e9));
        assert_eq!(fin.quarterly_cash_flow.len(), 1);
        assert_eq!(fin.quarterly_cash_flow[0].free_cash_flow, Some(20.0e9));
        assert_eq!(fin.quarterly_cash_flow[0].operating_cash_flow, Some(28.0e9));
        assert_eq!(fin.quarterly_cash_flow[0].capex, Some(-8.0e9));
        // Equity prefers the stockholders' (parent-only) line over totalEquity.
        assert_eq!(fin.total_debt, Some(110.0e9));
        assert_eq!(fin.total_equity, Some(62.0e9));
        assert_eq!(fin.cash_and_equivalents, Some(30.0e9));
        assert_eq!(fin.short_term_investments, Some(32.0e9));
        assert_eq!(fin.quarterly_balance_sheet.len(), 1);
        assert_eq!(fin.quarterly_balance_sheet[0].net_receivables, Some(60.0e9));
        assert_eq!(fin.quarterly_balance_sheet[0].inventory, Some(7.0e9));
        assert_eq!(
            fin.financial_scores,
            Some(crate::portfolio::engine::FinancialScores { altman_z: Some(9.32), piotroski: Some(8.0) })
        );
        assert_eq!(fin.consensus.as_ref().unwrap().eps_mid, Some(6.5));
        assert_eq!(fin.ttm_dividends_per_share, Some(0.26));
        assert!(fin.gaps.is_empty(), "a clean pull records no gap: {:?}", fin.gaps);
        assert_eq!(
            server.request_paths(),
            vec![
                "/quote",
                "/historical-price-eod/light",
                "/income-statement",
                "/cash-flow-statement",
                "/balance-sheet-statement",
                "/financial-scores",
                "/analyst-estimates",
                "/dividends"
            ]
        );
    }

    #[test]
    fn quick_check_adapters_round_trip_price_earnings_and_news() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","price":201.5}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","date":"2026-04-30","epsActual":1.61,"epsEstimated":1.55,"revenueActual":96.0e9},
                          {"symbol":"AAPL","date":"2026-07-30","epsActual":null,"epsEstimated":1.86,"revenueActual":null}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","publishedDate":"2026-08-01 14:13:36","title":"New chip ships","site":"example.com"},
                          {"symbol":"AAPL","publishedDate":"2026-07-01 09:00:00","title":"Old story","site":"example.com"}]"#,
            },
        ]);
        let src = test_source(&server.base_url);
        assert_eq!(src.fetch_live_price("AAPL").unwrap(), 201.5);
        let earnings = src.fetch_symbol_earnings("AAPL").unwrap();
        assert_eq!(earnings.len(), 2);
        assert_eq!(earnings[0].date, "2026-07-30");
        assert_eq!(earnings[1].eps_actual, Some(1.61));
        // The since filter holds client-side even if the server ignores `from`.
        let news = src.fetch_symbol_news_since("AAPL", "2026-07-20").unwrap();
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].title, "New chip ships");
        assert_eq!(
            server.request_paths(),
            vec!["/quote", "/earnings", "/news/stock"]
        );
    }

    #[test]
    fn quick_check_adapters_error_rather_than_silently_clearing() {
        // A premium gate / failed call must surface as `Err` — the quick check types
        // the family `unknown`, never a silent all-clear.
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 402, headers: vec![], body: "Payment Required" },
            Canned::Reply { status: 402, headers: vec![], body: "Payment Required" },
        ]);
        let src = test_source(&server.base_url);
        assert!(src.fetch_live_price("AAPL").is_err());
        assert!(src.fetch_symbol_earnings("AAPL").is_err());
    }

    #[test]
    fn quick_check_adapters_error_on_a_malformed_200_body() {
        // The fail-soft dividends read records its gap, so a malformed body can never
        // be mistaken for a confirmed non-payer downstream.
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: r#"{"message":"maintenance"}"# },
            Canned::Reply { status: 200, headers: vec![], body: r#"{"message":"maintenance"}"# },
            Canned::Reply { status: 200, headers: vec![], body: r#"{"message":"maintenance"}"# },
            Canned::Reply { status: 200, headers: vec![], body: r#"{"message":"maintenance"}"# },
        ]);
        let src = test_source(&server.base_url);
        assert!(src.fetch_symbol_earnings("AAPL").is_err());
        assert!(src.fetch_symbol_news_since("AAPL", "2026-07-20").is_err());
        assert!(src.fetch_analyst_estimates_strict("AAPL").is_err());
        let mut gaps = Vec::new();
        assert!(src.fetch_ttm_dividends("AAPL", &mut gaps).is_none());
        assert!(
            gaps.iter().any(|g| g.starts_with(DIVIDENDS_GAP_PREFIX)),
            "{gaps:?}"
        );
    }

    #[test]
    fn strict_estimates_split_undatable_rows_from_an_honest_past_only_set() {
        // `Ok(None)` is what the Revision family clears on, so undatable rows must be
        // `Err` or a stale verdict could carry uninspected behind a fabricated clear.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"soon","epsAvg":6.5}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2020-09-30","epsAvg":3.1}]"#,
            },
        ]);
        let src = test_source(&server.base_url);
        let err = src.fetch_analyst_estimates_strict("AAPL").unwrap_err();
        assert!(err.to_string().contains("undatable"), "{err}");
        assert!(src.fetch_analyst_estimates_strict("AAPL").unwrap().is_none());
    }

    #[test]
    fn ttm_dividend_pull_requests_the_full_history_margin() {
        // Future-dated declarations consume slots without landing in the window and a
        // monthly payer alone fills a 12-row cap, so a truncated pull silently
        // understates the trailing sum.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: "[]",
        }]);
        let src = test_source(&server.base_url);
        let mut gaps = Vec::new();
        assert!(src.fetch_ttm_dividends("AAPL", &mut gaps).is_none());
        assert!(gaps.is_empty(), "an empty body is the non-payer read: {gaps:?}");
        let targets = server.request_targets();
        assert!(
            targets[0].contains(&format!("limit={DIVIDEND_HISTORY_LIMIT}")),
            "{targets:?}"
        );
    }

    #[test]
    fn company_financials_degrade_to_gaps_on_premium_and_transport_failures() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 402,
                headers: vec![],
                body: "Payment Required",
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"unexpected":true}"#,
            },
        ]);
        let fin = test_source(&server.base_url).fetch_company_financials("AAPL");
        assert!(fin.current_price.is_none());
        assert!(fin.price_history.is_empty());
        assert_eq!(fin.gaps.len(), 8, "eight failed pulls, eight gaps: {:?}", fin.gaps);
    }

    #[test]
    fn income_shaping_falls_through_a_null_filing_date_to_the_legacy_spelling() {
        // A present-but-null `filingDate` must not suppress a valid legacy
        // `fillingDate` — the restatement tie-break depends on the date surviving.
        let value: Value = serde_json::from_str(
            r#"[{"date":"2026-03-31","filingDate":null,"fillingDate":"2026-05-01","revenue":1.0}]"#,
        )
        .unwrap();
        let rows = quarterly_income_from_value(&value);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filing_date.as_deref(), Some("2026-05-01"));
    }

    #[test]
    fn cash_flow_shaping_falls_through_a_null_ocf_to_the_alternate_spelling() {
        // Numeric-first per key: a present-but-null `operatingCashFlow` must still
        // fall through to `netCashProvidedByOperatingActivities`, or a derivable
        // FCF (and with it eligibility / runway) turns unscorable.
        let value: Value = serde_json::from_str(
            r#"[{"date":"2026-03-31","freeCashFlow":null,"operatingCashFlow":null,
                 "netCashProvidedByOperatingActivities":28.0e9,"capitalExpenditure":-8.0e9}]"#,
        )
        .unwrap();
        let rows = quarterly_cash_flow_from_value(&value);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].operating_cash_flow, Some(28.0e9));
        assert_eq!(rows[0].resolved_free_cash_flow(), Some(20.0e9));
    }

    #[test]
    fn company_financials_skip_the_network_when_already_cancelled() {
        use crate::progress::{NoopReporter, RunContext};
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        // An empty script: if the adapter made a request, the connect would hang/fail —
        // but a cancelled run must make none.
        let server = MockHttp::serve(vec![]);
        let ctx = RunContext::new(
            "run",
            Arc::new(NoopReporter),
            Arc::new(AtomicBool::new(true)), // already cancelled
        );
        let fin = test_source(&server.base_url)
            .with_context(ctx)
            .fetch_company_financials("AAPL");
        assert_eq!(server.attempts(), 0, "a cancelled run makes no request");
        assert!(fin.current_price.is_none());
        assert!(
            fin.gaps.iter().any(|g| g.contains("cancelled")),
            "the skip is recorded as a gap: {:?}",
            fin.gaps
        );
    }

    #[test]
    fn fetch_quotes_round_trips_a_200_into_a_quote() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"[{"symbol":"^GSPC","name":"S&P 500","price":5500.5,"changePercentage":0.42}]"#,
        }]);
        let source = test_source(&server.base_url);
        let mut gaps = Vec::new();
        let quotes = source.fetch_quotes(
            &[("^GSPC", "S&P 500", "index points")],
            GroupKind::Indices,
            &mut gaps,
        );
        assert_eq!(server.attempts(), 1, "one symbol => one request");
        let targets = server.request_targets();
        assert_eq!(server.request_paths(), ["/quote"]);
        assert!(
            targets[0].contains("symbol="),
            "the per-call query var must reach the wire: {targets:?}"
        );
        assert!(gaps.is_empty(), "a clean 200 records no gap");
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].symbol, "^GSPC");
        assert_eq!(quotes[0].name, "S&P 500");
        assert!((quotes[0].price - 5500.5).abs() < 1e-9);
        assert_eq!(quotes[0].unit, "index points");
    }

    #[test]
    fn fetch_quotes_round_trips_a_402_into_an_out_of_scope_gap() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 402,
            headers: vec![],
            body: "Premium Query Parameter",
        }]);
        let source = test_source(&server.base_url);
        let mut gaps = Vec::new();
        let quotes = source.fetch_quotes(
            &[("^GSPC", "S&P 500", "index points")],
            GroupKind::Indices,
            &mut gaps,
        );
        assert_eq!(server.attempts(), 1);
        assert_eq!(server.request_paths(), ["/quote"]);
        assert!(quotes.is_empty(), "a 402 yields no quote");
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].reason, GapReason::OutOfScope);
        assert_eq!(gaps[0].series_id, "^GSPC");
        assert_eq!(gaps[0].group, GroupKind::Indices);
    }

    #[test]
    fn quotes_from_value_maps_with_name_fallback_and_legacy_alias() {
        let v: Value = serde_json::from_str(
            r#"[{"symbol":"^GSPC","name":"S&P 500","price":5500.5,"changePercentage":0.42}]"#,
        )
        .unwrap();
        let quotes = quotes_from_value(v, "fallback", "index points").unwrap();
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].symbol, "^GSPC");
        assert_eq!(quotes[0].name, "S&P 500");
        assert!((quotes[0].price - 5500.5).abs() < 1e-9);
        assert!((quotes[0].change.value - 0.42).abs() < 1e-9);
        assert_eq!(
            quotes[0].change.kind,
            crate::data_sources::ChangeKind::Percent
        );
        assert_eq!(quotes[0].unit, "index points");

        let v2: Value =
            serde_json::from_str(r#"[{"symbol":"^DJI","price":40000.0,"changesPercentage":-1.5}]"#)
                .unwrap();
        let q2 = quotes_from_value(v2, "Dow Jones", "index points").unwrap();
        assert_eq!(q2[0].name, "Dow Jones");
        assert!((q2[0].change.value + 1.5).abs() < 1e-9);

        assert!(
            quotes_from_value(serde_json::from_str("[]").unwrap(), "x", "index points")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn quotes_from_value_requires_price_and_change() {
        let no_price: Value =
            serde_json::from_str(r#"[{"symbol":"^GSPC","changePercentage":0.4}]"#).unwrap();
        assert!(quotes_from_value(no_price, "x", "index points").is_err());
        let no_change: Value =
            serde_json::from_str(r#"[{"symbol":"^GSPC","price":5500.0}]"#).unwrap();
        assert!(quotes_from_value(no_change, "x", "index points").is_err());
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
        let v: Value = serde_json::from_str(
            r#"[{"sector":"Technology","averageChange":1.5},{"sector":"Energy"}]"#,
        )
        .unwrap();
        assert!(sectors_from_value(v).is_err());
    }

    #[test]
    fn sector_candidate_dates_skips_weekends_from_a_sunday() {
        let sunday = NaiveDate::from_ymd_opt(2026, 6, 7).unwrap();
        assert_eq!(sunday.weekday(), Weekday::Sun, "fixture sanity");
        let got: Vec<String> = sector_candidate_dates(sunday, 5)
            .iter()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            got,
            [
                "2026-06-05",
                "2026-06-04",
                "2026-06-03",
                "2026-06-02",
                "2026-06-01"
            ],
            "Sunday run starts at Fri 06-05 and walks back weekdays only"
        );
    }

    #[test]
    fn sector_candidate_dates_from_a_weekday_includes_today_then_skips_the_weekend() {
        let wednesday = NaiveDate::from_ymd_opt(2026, 6, 10).unwrap();
        assert_eq!(wednesday.weekday(), Weekday::Wed, "fixture sanity");
        let got: Vec<String> = sector_candidate_dates(wednesday, 5)
            .iter()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            got,
            [
                "2026-06-10",
                "2026-06-09",
                "2026-06-08",
                "2026-06-05",
                "2026-06-04"
            ],
            "weekday start includes today, then skips the weekend mid-walk"
        );
    }

    #[test]
    fn index_performance_from_eod_computes_all_horizons() {
        let row = |y, m, d, p: f64| (NaiveDate::from_ymd_opt(y, m, d).unwrap(), p);
        // Newest-first. Latest 06-10 @ 110; week-ago anchor (06-03) 100; last May close
        // (05-29) 95 anchors MTD; last 2025 close (12-31) 88 anchors YTD; 04-01 @ 120 is
        // the 52-week high; 02-01-2025 is before the 365-day cutoff and excluded.
        let rows = vec![
            row(2026, 6, 10, 110.0),
            row(2026, 6, 3, 100.0),
            row(2026, 5, 29, 95.0),
            row(2026, 4, 1, 120.0),
            row(2025, 12, 31, 88.0),
            row(2025, 8, 1, 70.0),
            row(2025, 2, 1, 60.0),
        ];
        let p = index_performance_from_eod("^GSPC", "S&P 500", &rows).expect("a performance");
        assert_eq!(p.symbol, "^GSPC");
        assert!(
            (p.weekly_pct - 10.0).abs() < 1e-9,
            "weekly {}",
            p.weekly_pct
        );
        assert!(
            (p.mtd_pct - (15.0 / 95.0 * 100.0)).abs() < 1e-9,
            "mtd {}",
            p.mtd_pct
        );
        assert!((p.ytd_pct - 25.0).abs() < 1e-9, "ytd {}", p.ytd_pct);
        assert!((p.low_52w - 70.0).abs() < 1e-9, "low {}", p.low_52w);
        assert!((p.high_52w - 120.0).abs() < 1e-9, "high {}", p.high_52w);
        assert!(
            (p.pct_from_52w_high - (-10.0 / 120.0 * 100.0)).abs() < 1e-9,
            "from_high {}",
            p.pct_from_52w_high
        );
    }

    #[test]
    fn index_performance_from_eod_too_short_is_none() {
        let only = vec![(NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(), 100.0)];
        assert!(index_performance_from_eod("^GSPC", "S&P 500", &only).is_none());
        assert!(index_performance_from_eod("^GSPC", "S&P 500", &[]).is_none());
    }

    #[test]
    fn eod_to_performance_parses_sorts_and_rejects_bad_shapes() {
        // Served out of order: latest is 06-10 (110), week-ago as-of 06-03 is 100.
        let v: Value = serde_json::from_str(
            r#"[
                {"symbol":"^GSPC","date":"2026-06-03","price":100.0,"volume":1},
                {"symbol":"^GSPC","date":"2026-06-10","price":110.0,"volume":1},
                {"symbol":"^GSPC","date":"2026-06-02","price":99.0,"volume":1}
            ]"#,
        )
        .unwrap();
        let p = eod_to_performance(v, "^GSPC", "S&P 500")
            .unwrap()
            .expect("a performance");
        assert!(
            (p.weekly_pct - 10.0).abs() < 1e-9,
            "weekly {}",
            p.weekly_pct
        );

        let obj: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(eod_to_performance(obj, "^GSPC", "S&P 500").is_err());
        let bad_date: Value = serde_json::from_str(r#"[{"date":"June 10","price":1.0}]"#).unwrap();
        assert!(eod_to_performance(bad_date, "^GSPC", "S&P 500").is_err());
    }

    #[test]
    fn movers_parse_tags_category_and_filters_noise() {
        let body = serde_json::json!([
            {"symbol":"NVDA","name":"NVIDIA","price":142.0,"changesPercentage":4.2,"exchange":"NASDAQ"},
            {"symbol":"SCAG","name":"Scage","price":0.84,"changesPercentage":194.0,"exchange":"NASDAQ"},
            {"symbol":"OTCX","name":"OTC Co","price":50.0,"changesPercentage":9.0,"exchange":"OTC"},
            {"symbol":"AAPL","name":"Apple","price":210.0,"changesPercentage":2.0,"exchange":"NYSE"}
        ]);
        let parsed = movers_from_value(body, MoverCategory::Gainer).unwrap();
        assert_eq!(parsed.len(), 4);
        assert!(parsed.iter().all(|m| m.category == MoverCategory::Gainer));
        let filtered = filter_movers(parsed);
        let symbols: Vec<&str> = filtered.iter().map(|m| m.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["NVDA", "AAPL"]);
    }

    #[test]
    fn movers_singular_alias_parses_and_name_falls_back() {
        let body = serde_json::json!([{"symbol":"MSFT","price":410.0,"changePercentage":1.5,"exchange":"NASDAQ"}]);
        let parsed = movers_from_value(body, MoverCategory::MostActive).unwrap();
        assert_eq!(parsed[0].name, "MSFT");
        assert_eq!(parsed[0].change_pct, 1.5);
    }

    #[test]
    fn movers_filter_excludes_funds_and_leveraged_etfs() {
        let movers = vec![
            StockMover {
                category: MoverCategory::Gainer,
                symbol: "NVDA".into(),
                name: "NVIDIA Corporation".into(),
                price: 142.0,
                change_pct: 4.0,
                exchange: "NASDAQ".into(),
            },
            StockMover {
                category: MoverCategory::Gainer,
                symbol: "TQQQ".into(),
                name: "ProShares - UltraPro QQQ".into(),
                price: 60.0,
                change_pct: 5.0,
                exchange: "NASDAQ".into(),
            },
            StockMover {
                category: MoverCategory::Gainer,
                symbol: "SOXS".into(),
                name: "Direxion Daily Semiconductor Bear 3X ETF".into(),
                price: 7.0,
                change_pct: 9.0,
                exchange: "AMEX".into(),
            },
            StockMover {
                category: MoverCategory::Gainer,
                symbol: "AAL".into(),
                name: "American Airlines Group Inc.".into(),
                price: 14.0,
                change_pct: 1.5,
                exchange: "NASDAQ".into(),
            },
        ];
        let kept = filter_movers(movers);
        let symbols: Vec<&str> = kept.iter().map(|m| m.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["NVDA", "AAL"]);
    }

    #[test]
    fn movers_filter_keeps_companies_with_bull_or_bear_in_name() {
        // Build-A-Bear stays; the leveraged directional ETF is still caught by
        // "direxion" / "3x" / " etf".
        let movers = vec![
            StockMover {
                category: MoverCategory::Gainer,
                symbol: "BBW".into(),
                name: "Build-A-Bear Workshop, Inc.".into(),
                price: 40.0,
                change_pct: 6.0,
                exchange: "NYSE".into(),
            },
            StockMover {
                category: MoverCategory::Gainer,
                symbol: "SOXL".into(),
                name: "Direxion Daily Semiconductor Bull 3X ETF".into(),
                price: 25.0,
                change_pct: 8.0,
                exchange: "AMEX".into(),
            },
        ];
        let kept = filter_movers(movers);
        let symbols: Vec<&str> = kept.iter().map(|m| m.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["BBW"]);
    }

    #[test]
    fn movers_filter_caps_at_top_n() {
        let many: Vec<StockMover> = (0..MOVER_TOP_N + 5)
            .map(|i| StockMover {
                category: MoverCategory::MostActive,
                symbol: format!("T{i}"),
                name: format!("Ticker {i}"),
                price: 100.0,
                change_pct: 1.0,
                exchange: "NYSE".into(),
            })
            .collect();
        assert_eq!(filter_movers(many).len(), MOVER_TOP_N);
    }

    #[test]
    fn earnings_filter_keeps_large_caps_sorted_by_revenue() {
        let body = serde_json::json!([
            {"symbol":"ADBE","date":"2026-06-11","epsEstimated":5.83,"epsActual":null,
             "revenueEstimated":6453568000.0,"revenueActual":null},
            {"symbol":"DOCU","date":"2026-06-04","epsEstimated":0.99,"epsActual":1.09,
             "revenueEstimated":830235000.0,"revenueActual":840000000.0},
            {"symbol":"BIG","date":"2026-06-10","epsEstimated":1.0,"epsActual":null,
             "revenueEstimated":20000000000.0,"revenueActual":null},
            {"symbol":"NOREV","date":"2026-06-10","epsEstimated":0.1,"epsActual":null,
             "revenueEstimated":null,"revenueActual":null}
        ]);
        let parsed = earnings_from_value(body).unwrap();
        assert_eq!(parsed.len(), 4);
        let filtered = filter_earnings(parsed);
        let symbols: Vec<&str> = filtered.iter().map(|e| e.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["BIG", "ADBE"]); // DOCU (<$5B) + NOREV (no estimate) dropped
        let adbe = filtered.iter().find(|e| e.symbol == "ADBE").unwrap();
        assert!(adbe.eps_actual.is_none() && adbe.eps_estimated == Some(5.83));
    }

    #[test]
    fn earnings_filter_caps_at_max_rows() {
        let many: Vec<EarningsEvent> = (0..EARNINGS_MAX_ROWS + 10)
            .map(|i| EarningsEvent {
                symbol: format!("S{i}"),
                date: "2026-06-10".into(),
                eps_estimated: Some(1.0),
                eps_actual: None,
                revenue_estimated: Some(EARNINGS_MIN_REVENUE + i as f64),
                revenue_actual: None,
            })
            .collect();
        assert_eq!(filter_earnings(many).len(), EARNINGS_MAX_ROWS);
    }

    #[test]
    fn sector_pe_from_value_labels_by_wire_exchange_and_dedupes() {
        let v = serde_json::json!([
            {"date":"2026-06-05","sector":"Technology","exchange":"NASDAQ","pe":38.4},
            {"date":"2026-06-05","sector":"Energy","exchange":"NASDAQ","pe":12.1},
            {"date":"2026-06-05","sector":"Technology","exchange":"NASDAQ","pe":99.0}
        ]);
        let out = sector_pe_from_value(v, "NASDAQ").unwrap();
        assert_eq!(out.len(), 2); // the duplicate (Technology, NASDAQ) is dropped
        assert_eq!(
            (out[0].sector.as_str(), out[0].exchange.as_str()),
            ("Technology", "NASDAQ")
        );
        assert_eq!(out[0].pe, Some(38.4)); // first kept, not 99.0; both in-band
        assert_eq!(
            (out[1].sector.as_str(), out[1].exchange.as_str()),
            ("Energy", "NASDAQ")
        );
    }

    #[test]
    fn sector_pe_from_value_drops_out_of_band_pe_to_none_keeping_the_row() {
        let v = serde_json::json!([
            {"sector":"Technology","exchange":"NASDAQ","pe":38.4},
            {"sector":"Energy","exchange":"NASDAQ","pe":0.0},
            {"sector":"Utilities","exchange":"NASDAQ","pe":-5.0},
            {"sector":"Materials","exchange":"NASDAQ","pe":SECTOR_PE_MAX + 0.1}
        ]);
        let out = sector_pe_from_value(v, "NASDAQ").unwrap();
        assert_eq!(
            out.len(),
            4,
            "every row survives — only the pe is dropped to None"
        );
        assert_eq!(out[0].pe, Some(38.4));
        assert_eq!(out[1].pe, None, "non-positive 0.0 → None");
        assert_eq!(out[2].pe, None, "negative → None");
        assert_eq!(out[3].pe, None, "above SECTOR_PE_MAX → None");
        // The boundary itself is in-band (inclusive upper bound).
        let edge = serde_json::json!([{"sector":"X","exchange":"NASDAQ","pe":SECTOR_PE_MAX}]);
        assert_eq!(
            sector_pe_from_value(edge, "NASDAQ").unwrap()[0].pe,
            Some(SECTOR_PE_MAX)
        );
    }

    #[test]
    fn sector_pe_from_value_rejects_off_board_rows() {
        let v = serde_json::json!([
            {"sector":"Technology","exchange":"NYSE","pe":24.6},
            {"sector":"Energy","exchange":"NASDAQ","pe":12.1}
        ]);
        assert!(sector_pe_from_value(v, "NYSE").is_err());
    }

    #[test]
    fn sector_pe_from_value_requires_exchange_and_pe() {
        assert!(sector_pe_from_value(
            serde_json::json!([{"sector":"Technology","pe":1.0}]),
            "NASDAQ"
        )
        .is_err());
        assert!(sector_pe_from_value(
            serde_json::json!([{"sector":"Technology","exchange":"NASDAQ"}]),
            "NASDAQ"
        )
        .is_err());
    }

    fn pe_map(rows: &[(&str, &str, f64)]) -> HashMap<(String, String), f64> {
        rows.iter()
            .map(|(ind, ex, pe)| ((ind.to_string(), ex.to_string()), *pe))
            .collect()
    }

    #[test]
    fn industries_join_caps_top_and_bottom_and_attaches_pe() {
        // Five rows: fewer than 2×N, so the top and bottom slices must not double-count.
        let perf = vec![
            ("Semiconductors".to_string(), "NASDAQ".to_string(), 4.0),
            ("Banks".to_string(), "NASDAQ".to_string(), 1.0),
            ("Utilities".to_string(), "NASDAQ".to_string(), 0.0),
            ("Airlines".to_string(), "NASDAQ".to_string(), -2.0),
            ("Biotech".to_string(), "NASDAQ".to_string(), -5.0),
        ];
        let pe = pe_map(&[
            ("Semiconductors", "NASDAQ", 41.2),
            ("Biotech", "NASDAQ", 18.0),
        ]);
        let out = top_bottom_industries(perf, &pe);
        let names: Vec<&str> = out.iter().map(|i| i.industry.as_str()).collect();
        assert_eq!(
            names,
            [
                "Semiconductors",
                "Banks",
                "Utilities",
                "Airlines",
                "Biotech"
            ]
        );
        assert!(out.iter().all(|i| i.exchange == "NASDAQ"));
        assert_eq!(out[0].pe, Some(41.2));
        assert_eq!(out[1].pe, None);
        assert_eq!(out[4].pe, Some(18.0));
    }

    #[test]
    fn industries_join_is_same_exchange_only() {
        let perf = vec![("Semiconductors".to_string(), "NYSE".to_string(), 3.0)];
        let pe = pe_map(&[("Semiconductors", "NASDAQ", 63.9)]); // NASDAQ PE only
        let out = top_bottom_industries(perf, &pe);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].exchange, "NYSE");
        assert_eq!(out[0].pe, None, "NASDAQ PE must not attach to the NYSE row");
    }

    #[test]
    fn industries_join_picks_extremes_when_list_exceeds_cap() {
        // 2*INDUSTRY_TOP_N + 4 industries, so a flat middle exists to drop.
        let mut perf: Vec<(String, String, f64)> = (0..2 * INDUSTRY_TOP_N + 4)
            .map(|i| (format!("Ind{i}"), "NYSE".to_string(), i as f64)) // ascending move
            .collect();
        perf.reverse(); // arrival order need not be sorted
        let out = top_bottom_industries(perf, &HashMap::new());
        assert_eq!(out.len(), 2 * INDUSTRY_TOP_N);
        assert_eq!(
            out.first().unwrap().industry,
            format!("Ind{}", 2 * INDUSTRY_TOP_N + 3)
        );
        assert_eq!(out.last().unwrap().industry, "Ind0");
        let unique: HashSet<&str> = out.iter().map(|i| i.industry.as_str()).collect();
        assert_eq!(unique.len(), out.len());
    }

    #[test]
    fn industry_perf_and_pe_parse_keep_wire_exchange_and_dedupe() {
        let perf_v = serde_json::json!([
            {"date":"2026-06-05","industry":"Semiconductors","exchange":"NASDAQ","averageChange":2.4},
            {"date":"2026-06-05","industry":"Banks","exchange":"NASDAQ","averageChange":1.1},
            {"date":"2026-06-05","industry":"Semiconductors","exchange":"NASDAQ","averageChange":-1.0}
        ]);
        let perf = industry_perf_from_value(perf_v, "NASDAQ").unwrap();
        assert_eq!(perf.len(), 2); // the duplicate (Semiconductors, NASDAQ) is dropped
        assert_eq!(
            (perf[0].0.as_str(), perf[0].1.as_str()),
            ("Semiconductors", "NASDAQ")
        );
        assert!((perf[0].2 - 2.4).abs() < 1e-9); // first kept, not -1.0
        let pe_v = serde_json::json!([{"industry":"Semiconductors","exchange":"NASDAQ","pe":41.2}]);
        let pe = industry_pe_map_from_value(pe_v, "NASDAQ").unwrap();
        assert!((pe[&("Semiconductors".to_string(), "NASDAQ".to_string())] - 41.2).abs() < 1e-9);
    }

    #[test]
    fn industry_snapshots_reject_off_board_rows() {
        let perf_v = serde_json::json!([{"industry":"Semiconductors","exchange":"NASDAQ","averageChange":2.4}]);
        assert!(industry_perf_from_value(perf_v, "NYSE").is_err());
        let pe_v = serde_json::json!([{"industry":"Semiconductors","exchange":"NASDAQ","pe":41.2}]);
        assert!(industry_pe_map_from_value(pe_v, "NYSE").is_err());
    }

    #[test]
    fn industry_pe_map_drops_non_positive_ratios() {
        let v = serde_json::json!([
            {"industry":"Oil & Gas Energy","exchange":"NASDAQ","pe":0.0},
            {"industry":"Biotech","exchange":"NASDAQ","pe":-3.0},
            {"industry":"Semiconductors","exchange":"NASDAQ","pe":63.9}
        ]);
        let map = industry_pe_map_from_value(v, "NASDAQ").unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&("Semiconductors".to_string(), "NASDAQ".to_string())));
        let perf = vec![
            ("Oil & Gas Energy".to_string(), "NASDAQ".to_string(), -16.3),
            ("Semiconductors".to_string(), "NASDAQ".to_string(), -5.5),
        ];
        let joined = top_bottom_industries(perf, &map);
        let oil = joined
            .iter()
            .find(|i| i.industry == "Oil & Gas Energy")
            .unwrap();
        assert_eq!(oil.pe, None);
        let semi = joined
            .iter()
            .find(|i| i.industry == "Semiconductors")
            .unwrap();
        assert_eq!(semi.pe, Some(63.9));
    }

    #[test]
    fn industry_pe_map_drops_implausibly_high_ratios() {
        let v = serde_json::json!([
            {"industry":"Software Application","exchange":"NASDAQ","pe":461.0},
            {"industry":"Semiconductors","exchange":"NASDAQ","pe":63.9}
        ]);
        let map = industry_pe_map_from_value(v, "NASDAQ").unwrap();
        assert_eq!(map.len(), 1);
        assert!(!map.contains_key(&("Software Application".to_string(), "NASDAQ".to_string())));
        let perf = vec![
            (
                "Software Application".to_string(),
                "NASDAQ".to_string(),
                12.4,
            ),
            ("Semiconductors".to_string(), "NASDAQ".to_string(), -5.5),
        ];
        let joined = top_bottom_industries(perf, &map);
        let soft = joined
            .iter()
            .find(|i| i.industry == "Software Application")
            .unwrap();
        assert_eq!(soft.pe, None);
        let semi = joined
            .iter()
            .find(|i| i.industry == "Semiconductors")
            .unwrap();
        assert_eq!(semi.pe, Some(63.9));
    }

    #[test]
    fn industry_pe_map_band_is_closed_at_the_ceiling() {
        // Pins the gate's `<=` against an accidental flip to `<` when the ceiling is tuned.
        let just_over = INDUSTRY_PE_MAX + 0.01;
        let v = serde_json::json!([
            {"industry":"At Ceiling","exchange":"NASDAQ","pe":INDUSTRY_PE_MAX},
            {"industry":"Just Over","exchange":"NASDAQ","pe":just_over}
        ]);
        let map = industry_pe_map_from_value(v, "NASDAQ").unwrap();
        assert_eq!(
            map.get(&("At Ceiling".to_string(), "NASDAQ".to_string()))
                .copied(),
            Some(INDUSTRY_PE_MAX),
        );
        assert!(!map.contains_key(&("Just Over".to_string(), "NASDAQ".to_string())));
    }

    #[test]
    fn risk_premium_filters_to_us_exactly() {
        let v = serde_json::json!([
            {"country":"United States","continent":"North America","countryRiskPremium":0.23,"totalEquityRiskPremium":4.46},
            {"country":"United Kingdom","continent":"Europe","countryRiskPremium":0.78,"totalEquityRiskPremium":5.01},
            {"country":"United Arab Emirates","continent":"Asia","countryRiskPremium":0.64,"totalEquityRiskPremium":4.87}
        ]);
        let out = risk_premium_from_value(v).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].country, "United States");
        assert!((out[0].total_equity_risk_premium - 4.46).abs() < 1e-9);
    }

    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY"]
    fn fmp_baseline_smoke() {
        let src = FmpDataSource::from_env().expect("FMP_API_KEY set");
        let data = src.baseline_scan(ReportCadence::default()).expect("live baseline scan");

        // Print the resolved mapping (run with `-- --ignored --nocapture`).
        let dump = |label: &str, quotes: &[Quote]| {
            eprintln!("{label} ({}):", quotes.len());
            for q in quotes {
                eprintln!(
                    "  {:<10} {:<28} price={:<12} change={:<10} unit={}",
                    q.symbol, q.name, q.price, q.change.value, q.unit
                );
            }
        };
        dump("indices", &data.indices);
        dump("internals", &data.internals);
        eprintln!("sectors ({}):", data.sectors.len());
        for s in &data.sectors {
            eprintln!("  {:<24} change_pct={}", s.sector, s.change_pct);
        }

        // Every named symbol must resolve individually: a group-level check lets one
        // symbol leaving the free tier hide behind its siblings.
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

        // Soft enrichment at runtime, but the smoke holds each index to the per-symbol
        // bar so a regressed EOD path surfaces.
        eprintln!("index_performance ({}):", data.index_performance.len());
        for p in &data.index_performance {
            eprintln!(
                "  {:<8} {:<24} wk={:<8.2} mtd={:<8.2} ytd={:<8.2} 52w=[{:.2}, {:.2}] from_high={:.2}",
                p.symbol, p.name, p.weekly_pct, p.mtd_pct, p.ytd_pct, p.low_52w, p.high_52w,
                p.pct_from_52w_high
            );
        }
        for (sym, _, _) in INDEX_SYMBOLS {
            assert!(
                data.index_performance.iter().any(|p| p.symbol == *sym),
                "index_performance: {sym} did not resolve"
            );
        }

        // A trading day always has large-cap movers and reporters in the window, so an
        // empty list means the endpoint left the free tier or the filters are too tight.
        eprintln!("movers ({}):", data.movers.len());
        for m in &data.movers {
            eprintln!(
                "  {:<10} {:<28} {:<12} change_pct={:<8} {}",
                m.symbol,
                m.name,
                format!("{:?}", m.category),
                m.change_pct,
                m.exchange
            );
        }
        eprintln!("earnings ({}):", data.earnings.len());
        for e in &data.earnings {
            eprintln!(
                "  {:<8} {} eps_est={:?} eps_act={:?} rev_est={:?}",
                e.symbol, e.date, e.eps_estimated, e.eps_actual, e.revenue_estimated
            );
        }
        assert!(
            !data.movers.is_empty(),
            "no movers resolved — the mover lists may have left the free tier or the filters are too tight"
        );
        assert!(
            !data.earnings.is_empty(),
            "no earnings resolved — the calendar may have left the free tier or the revenue floor is too high"
        );

        // Sanity-check magnitude, not mere existence: a stale / wrong value still
        // "resolves".
        eprintln!("sector_pe ({}):", data.sector_pe.len());
        for s in &data.sector_pe {
            eprintln!("  {:<8} {:<24} pe={:?}", s.exchange, s.sector, s.pe);
        }
        eprintln!("industries ({}):", data.industries.len());
        for i in &data.industries {
            eprintln!(
                "  {:<8} {:<32} change_pct={:<8.2} pe={:?}",
                i.exchange, i.industry, i.change_pct, i.pe
            );
        }
        eprintln!("market_risk_premium ({}):", data.market_risk_premium.len());
        for r in &data.market_risk_premium {
            eprintln!(
                "  {:<16} crp={:.2} total_erp={:.2}",
                r.country, r.country_risk_premium, r.total_equity_risk_premium
            );
        }
        assert!(!data.sector_pe.is_empty(), "no sector P/E rows resolved");
        assert!(
            data.sector_pe
                .iter()
                .any(|s| s.pe.is_some_and(|pe| pe.is_finite() && pe > 0.0)),
            "no sector carried a finite positive in-band P/E — the snapshot may have regressed"
        );
        assert!(!data.industries.is_empty(), "no industry rows resolved");
        assert!(
            data.industries.iter().any(|i| i.pe.is_some()),
            "no industry carried a P/E — the industry-PE join may have regressed"
        );
        // Both boards must resolve; a silent drop of one would hide behind the other.
        for ex in SNAPSHOT_EXCHANGES {
            assert!(
                data.sector_pe.iter().any(|s| s.exchange == *ex),
                "sector_pe missing the {ex} board — it may have left the free tier"
            );
            assert!(
                data.industries.iter().any(|i| i.exchange == *ex),
                "industries missing the {ex} board — it may have left the free tier"
            );
        }
        let us = data
            .market_risk_premium
            .iter()
            .find(|r| r.country == RISK_PREMIUM_COUNTRY)
            .expect("US equity-risk-premium did not resolve");
        assert!(
            (2.0..=10.0).contains(&us.total_equity_risk_premium),
            "US total ERP {} outside the sane 2-10% range — the dataset or filter may have regressed",
            us.total_equity_risk_premium
        );
    }

    /// Calibration aid for [`INDUSTRY_PE_MAX`], not a gate: prints each board's raw
    /// aggregate P/E distribution sorted high→low, flagging the above-ceiling and
    /// non-positive rows production drops. Set the ceiling above the highest plausible
    /// in-band aggregate but below the artifact cluster. Hits the live API (≤2 calls per
    /// board); run once per change:
    ///   source <your-keys-env-file> && cargo test --manifest-path \
    ///     src-tauri/Cargo.toml tuning_industry_pe_distribution_probe -- --ignored --nocapture
    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY. Calibration aid, not a gate — \
                run with `-- --ignored --nocapture` to read the industry-P/E distribution."]
    fn tuning_industry_pe_distribution_probe() {
        let src = FmpDataSource::from_env().expect("FMP_API_KEY set");
        let today = Utc::now().date_naive();
        eprintln!(
            "industry P/E distribution (today = {today}); current INDUSTRY_PE_MAX = {INDUSTRY_PE_MAX}; \
             set the ceiling above the highest plausible in-band aggregate, below the artifact cluster:"
        );
        for exchange in SNAPSHOT_EXCHANGES {
            let mut resolved = false;
            for date in sector_candidate_dates(today, SECTOR_LOOKBACK_WEEKDAYS) {
                let date_str = date.format("%Y-%m-%d").to_string();
                let (status, body) = src
                    .get(
                        FMP_INDUSTRY_PE_PATH,
                        &[("date", date_str.as_str()), ("exchange", exchange)],
                    )
                    .expect("industry-pe fetch");
                let value = match interpret_response(status, &body) {
                    Disposition::Value(v) => v,
                    Disposition::Gap(reason) => {
                        eprintln!(
                            "  {exchange} {date_str}: gap ({reason:?}) — trying earlier date"
                        );
                        continue;
                    }
                };
                let raws: Vec<FmpIndustryPeRaw> = match serde_json::from_value(value) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("  {exchange} {date_str}: malformed ({e}) — trying earlier date");
                        continue;
                    }
                };
                if raws.is_empty() {
                    continue;
                }
                // Bypass the band filter this probe calibrates, but honor the exchange guard
                // production enforces, or a response where FMP ignored `exchange` would
                // pollute one board's distribution with the other's.
                let total = raws.len();
                let mut rows: Vec<(String, f64)> = raws
                    .into_iter()
                    .filter(|r| r.exchange == *exchange)
                    .map(|r| (r.industry, r.pe))
                    .collect();
                let off_board = total - rows.len();
                rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let above = rows.iter().filter(|(_, pe)| *pe > INDUSTRY_PE_MAX).count();
                let non_positive = rows.iter().filter(|(_, pe)| *pe <= 0.0).count();
                let max_in_band = rows
                    .iter()
                    .map(|(_, pe)| *pe)
                    .filter(|pe| *pe > 0.0 && *pe <= INDUSTRY_PE_MAX)
                    .fold(f64::NAN, f64::max);
                eprintln!(
                    "\n=== {exchange} ({date_str}) — {n} industries: {above} above ceiling, \
                     {non_positive} non-positive, {off_board} off-board ignored, max in-band = {max_in_band:.1} ===",
                    n = rows.len(),
                );
                for (industry, pe) in &rows {
                    let flag = if *pe > INDUSTRY_PE_MAX {
                        " DROP>ceiling"
                    } else if *pe <= 0.0 {
                        " DROP<=0"
                    } else {
                        ""
                    };
                    eprintln!("  pe={pe:>8.1}  {industry}{flag}");
                }
                resolved = true;
                break;
            }
            if !resolved {
                eprintln!("  {exchange}: no industry-P/E data resolved over the candidate window");
            }
        }
    }

    /// The sector-cut sibling of `tuning_industry_pe_distribution_probe`, calibrating
    /// [`SECTOR_PE_MAX`]; a sector aggregate sums over far more constituents, so expect a
    /// tighter plausible band and a rarer artifact tail. Hits the live API (≤2 calls per
    /// board); run once per change:
    ///   source <your-keys-env-file> && cargo test --manifest-path \
    ///     src-tauri/Cargo.toml tuning_sector_pe_distribution_probe -- --ignored --nocapture
    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY. Calibration aid, not a gate — \
                run with `-- --ignored --nocapture` to read the sector-P/E distribution."]
    fn tuning_sector_pe_distribution_probe() {
        let src = FmpDataSource::from_env().expect("FMP_API_KEY set");
        let today = Utc::now().date_naive();
        eprintln!(
            "sector P/E distribution (today = {today}); current SECTOR_PE_MAX = {SECTOR_PE_MAX}; \
             set the ceiling above the highest plausible in-band aggregate, below any artifact cluster:"
        );
        for exchange in SNAPSHOT_EXCHANGES {
            let mut resolved = false;
            for date in sector_candidate_dates(today, SECTOR_LOOKBACK_WEEKDAYS) {
                let date_str = date.format("%Y-%m-%d").to_string();
                let (status, body) = src
                    .get(
                        FMP_SECTOR_PE_PATH,
                        &[("date", date_str.as_str()), ("exchange", exchange)],
                    )
                    .expect("sector-pe fetch");
                let value = match interpret_response(status, &body) {
                    Disposition::Value(v) => v,
                    Disposition::Gap(reason) => {
                        eprintln!(
                            "  {exchange} {date_str}: gap ({reason:?}) — trying earlier date"
                        );
                        continue;
                    }
                };
                let raws: Vec<FmpSectorPeRaw> = match serde_json::from_value(value) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("  {exchange} {date_str}: malformed ({e}) — trying earlier date");
                        continue;
                    }
                };
                if raws.is_empty() {
                    continue;
                }
                // Bypass the band filter this probe calibrates, but honor the exchange guard
                // production enforces (see the industry probe).
                let total = raws.len();
                let mut rows: Vec<(String, f64)> = raws
                    .into_iter()
                    .filter(|r| r.exchange == *exchange)
                    .map(|r| (r.sector, r.pe))
                    .collect();
                let off_board = total - rows.len();
                rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let above = rows.iter().filter(|(_, pe)| *pe > SECTOR_PE_MAX).count();
                let non_positive = rows.iter().filter(|(_, pe)| *pe <= 0.0).count();
                let max_in_band = rows
                    .iter()
                    .map(|(_, pe)| *pe)
                    .filter(|pe| *pe > 0.0 && *pe <= SECTOR_PE_MAX)
                    .fold(f64::NAN, f64::max);
                eprintln!(
                    "\n=== {exchange} ({date_str}) — {n} sectors: {above} above ceiling, \
                     {non_positive} non-positive, {off_board} off-board ignored, max in-band = {max_in_band:.1} ===",
                    n = rows.len(),
                );
                for (sector, pe) in &rows {
                    let flag = if *pe > SECTOR_PE_MAX {
                        " DROP>ceiling"
                    } else if *pe <= 0.0 {
                        " DROP<=0"
                    } else {
                        ""
                    };
                    eprintln!("  pe={pe:>8.1}  {sector}{flag}");
                }
                resolved = true;
                break;
            }
            if !resolved {
                eprintln!("  {exchange}: no sector-P/E data resolved over the candidate window");
            }
        }
    }

    /// One-shot burst probe for the shape of the paid plan's per-minute limit: fires up
    /// to ~240 cheap quote calls across 4 threads inside one minute and records the first
    /// rejection's status, rate-limit headers and body head (an HTTP 429 the retry ladder
    /// covers as-is; a 200 `Error Message` body needs a body classification). Burns ~1
    /// minute of the paid budget; run:
    ///   source <your-keys-env-file> && cargo test --manifest-path \
    ///     src-tauri/Cargo.toml fmp_minute_limit_probe -- --ignored --nocapture
    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY. Bursts past 200/min to observe the limiter's shape."]
    fn fmp_minute_limit_probe() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Mutex;
        use std::time::Instant;

        const CAP: usize = 240;
        const THREADS: usize = 4;

        let key = crate::config::AppConfig::from_env()
            .fmp_key()
            .expect("FMP_API_KEY set");
        let start = Instant::now();
        let sent = AtomicUsize::new(0);
        let done = AtomicBool::new(false);
        let rejection: Mutex<Option<String>> = Mutex::new(None);

        std::thread::scope(|s| {
            for _ in 0..THREADS {
                s.spawn(|| {
                    let http = reqwest::blocking::Client::builder()
                        .timeout(FMP_TIMEOUT)
                        .build()
                        .expect("http client");
                    let url = format!("{FMP_BASE}/quote");
                    loop {
                        if done.load(Ordering::SeqCst) {
                            return;
                        }
                        let n = sent.fetch_add(1, Ordering::SeqCst);
                        if n >= CAP {
                            return;
                        }
                        let q = [("symbol", "AAPL"), ("apikey", key.as_str())];
                        match http.get(&url).query(&q).send() {
                            Ok(resp) => {
                                let status = resp.status().as_u16();
                                let retry_after = resp
                                    .headers()
                                    .get(reqwest::header::RETRY_AFTER)
                                    .and_then(|v| v.to_str().ok())
                                    .map(str::to_owned);
                                let ratelimit: Vec<String> = resp
                                    .headers()
                                    .iter()
                                    .filter(|(k, _)| {
                                        k.as_str().to_ascii_lowercase().contains("ratelimit")
                                    })
                                    .map(|(k, v)| {
                                        format!("{k}: {}", v.to_str().unwrap_or("<bin>"))
                                    })
                                    .collect();
                                let body = resp.text().unwrap_or_default();
                                let is_reject = status != 200
                                    || body.contains("Error Message")
                                    || body.to_ascii_lowercase().contains("limit");
                                if is_reject {
                                    let head: String = body.chars().take(400).collect();
                                    *rejection.lock().unwrap() = Some(format!(
                                        "call #{n} at {:.1}s: HTTP {status}\n  Retry-After: {retry_after:?}\n  rate-limit headers: {ratelimit:?}\n  body head: {head}",
                                        start.elapsed().as_secs_f32(),
                                    ));
                                    done.store(true, Ordering::SeqCst);
                                    return;
                                }
                            }
                            Err(e) => {
                                *rejection.lock().unwrap() = Some(format!(
                                    "call #{n} at {:.1}s: transport error: {e}",
                                    start.elapsed().as_secs_f32(),
                                ));
                                done.store(true, Ordering::SeqCst);
                                return;
                            }
                        }
                    }
                });
            }
        });

        let total = sent.load(Ordering::SeqCst).min(CAP);
        let rejection = rejection.lock().unwrap();
        match rejection.as_ref() {
            Some(r) => eprintln!(
                "\n=== FIRST REJECTION after {total} calls in {:.1}s ===\n{r}",
                start.elapsed().as_secs_f32(),
            ),
            None => eprintln!(
                "\n=== NO REJECTION: {total} calls completed clean in {:.1}s — limiter not tripped ===",
                start.elapsed().as_secs_f32(),
            ),
        }
    }

    /// Free-vs-premium probe for candidate endpoints (200 ≈ free, 402 = premium, 404 =
    /// wrong path); prints a body sample so the real field names can be read before an
    /// adapter is written. Hits the live API (~15 calls); run:
    ///   source <your-keys-env-file> && cargo test --manifest-path \
    ///     src-tauri/Cargo.toml fmp_freetier_probe -- --ignored --nocapture
    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY. Probes candidate endpoints' free tier."]
    fn fmp_freetier_probe() {
        use chrono::{Datelike, Duration, Utc, Weekday};

        let key = crate::config::AppConfig::from_env()
            .fmp_key()
            .expect("FMP_API_KEY set");
        let http = reqwest::blocking::Client::builder()
            .timeout(FMP_TIMEOUT)
            .build()
            .expect("http client");

        // A recent trading day for the date-keyed snapshot endpoints (walk back over
        // the weekend), and a ~3-week window straddling it for the calendar.
        let mut day = Utc::now().date_naive();
        while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            day -= Duration::days(1);
        }
        let date = day.format("%Y-%m-%d").to_string();
        let from = (day - Duration::days(7)).format("%Y-%m-%d").to_string();
        let to = (day + Duration::days(14)).format("%Y-%m-%d").to_string();

        let probe = |label: &str, url: &str, extra: &[(&str, &str)]| {
            let mut q: Vec<(&str, &str)> = vec![("apikey", key.as_str())];
            q.extend_from_slice(extra);
            match http.get(url).query(&q).send() {
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    let verdict = match status.as_u16() {
                        200 if body.contains("Error Message") => "200-but-Error-body",
                        200 => "FREE?",
                        402 => "PREMIUM",
                        other => return eprintln!("\n=== {label} [{other}] ===\n{body}"),
                    };
                    let mut shown: String = body.chars().take(700).collect();
                    if body.chars().count() > 700 {
                        shown.push_str(" …(truncated)");
                    }
                    eprintln!("\n=== {label} [{status}] {verdict} ===\n{shown}");
                }
                Err(e) => eprintln!("\n=== {label} [transport error] ===\n{e}"),
            }
        };

        let base = "https://financialmodelingprep.com/stable";
        probe("biggest-gainers", &format!("{base}/biggest-gainers"), &[]);
        probe("biggest-losers", &format!("{base}/biggest-losers"), &[]);
        probe("most-active", &format!("{base}/most-active"), &[]);
        probe(
            "most-actives (plural alias)",
            &format!("{base}/most-actives"),
            &[],
        );
        probe(
            "earnings-calendar",
            &format!("{base}/earnings-calendar"),
            &[("from", from.as_str()), ("to", to.as_str())],
        );
        probe("sp-500 constituents", &format!("{base}/sp-500"), &[]);
        probe("dow-jones constituents", &format!("{base}/dow-jones"), &[]);
        probe(
            "sector-pe-snapshot",
            &format!("{base}/sector-pe-snapshot"),
            &[("date", date.as_str())],
        );
        probe(
            "industry-performance-snapshot",
            &format!("{base}/industry-performance-snapshot"),
            &[("date", date.as_str())],
        );
        probe(
            "industry-pe-snapshot",
            &format!("{base}/industry-pe-snapshot"),
            &[("date", date.as_str())],
        );
        probe(
            "market-risk-premium",
            &format!("{base}/market-risk-premium"),
            &[],
        );
        probe(
            "commodities-quote GCUSD (gold)",
            &format!("{base}/commodities-quote"),
            &[("symbol", "GCUSD")],
        );
        probe(
            "commodities-quote HGUSD (copper)",
            &format!("{base}/commodities-quote"),
            &[("symbol", "HGUSD")],
        );
        probe(
            "commodities-quote SIUSD (silver)",
            &format!("{base}/commodities-quote"),
            &[("symbol", "SIUSD")],
        );
        probe("all-index-quotes", &format!("{base}/all-index-quotes"), &[]);

        // Constituent lists use the `*-constituent` paths; the bare index names 404.
        probe(
            "sp500-constituent",
            &format!("{base}/sp500-constituent"),
            &[],
        );
        probe(
            "dowjones-constituent",
            &format!("{base}/dowjones-constituent"),
            &[],
        );
        probe(
            "nasdaq-constituent",
            &format!("{base}/nasdaq-constituent"),
            &[],
        );
        probe(
            "batch-index-quotes",
            &format!("{base}/batch-index-quotes"),
            &[],
        );
        probe(
            "quote HGUSD (copper)",
            &format!("{base}/quote"),
            &[("symbol", "HGUSD")],
        );
        probe(
            "quote SIUSD (silver)",
            &format!("{base}/quote"),
            &[("symbol", "SIUSD")],
        );
    }

    /// Free-vs-premium probe for FMP's news endpoints, the conventions of
    /// `fmp_freetier_probe`; prints body samples so the field mapping and paging can be
    /// read off the wire. Hits the live API (~5 calls); run once:
    ///   source <your-keys-env-file> && cargo test --manifest-path \
    ///     src-tauri/Cargo.toml fmp_news_probe -- --ignored --nocapture
    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY. Probes news endpoints' free tier."]
    fn fmp_news_probe() {
        let key = crate::config::AppConfig::from_env()
            .fmp_key()
            .expect("FMP_API_KEY set");
        let http = reqwest::blocking::Client::builder()
            .timeout(FMP_TIMEOUT)
            .build()
            .expect("http client");

        let probe = |label: &str, url: &str, extra: &[(&str, &str)]| {
            let mut q: Vec<(&str, &str)> = vec![("apikey", key.as_str())];
            q.extend_from_slice(extra);
            match http.get(url).query(&q).send() {
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    let verdict = match status.as_u16() {
                        200 if body.contains("Error Message") => "200-but-Error-body",
                        200 => "FREE?",
                        402 => "PREMIUM",
                        other => return eprintln!("\n=== {label} [{other}] ===\n{body}"),
                    };
                    let mut shown: String = body.chars().take(1200).collect();
                    if body.chars().count() > 1200 {
                        shown.push_str(" …(truncated)");
                    }
                    eprintln!("\n=== {label} [{status}] {verdict} ===\n{shown}");
                }
                Err(e) => eprintln!("\n=== {label} [transport error] ===\n{e}"),
            }
        };

        let base = "https://financialmodelingprep.com/stable";
        // Explicit paging params, to learn whether `page` / `limit` are honored.
        probe(
            "fmp-articles (page+limit)",
            &format!("{base}/fmp-articles"),
            &[("page", "0"), ("limit", "5")],
        );
        probe(
            "news/general-latest",
            &format!("{base}/news/general-latest"),
            &[("page", "0"), ("limit", "5")],
        );
        probe(
            "news/stock-latest",
            &format!("{base}/news/stock-latest"),
            &[("page", "0"), ("limit", "5")],
        );
        probe(
            "news/stock (symbol-scoped)",
            &format!("{base}/news/stock"),
            &[("symbols", "AAPL"), ("limit", "5")],
        );
        probe(
            "news/press-releases-latest",
            &format!("{base}/news/press-releases-latest"),
            &[("page", "0"), ("limit", "5")],
        );
    }

    /// What `/profile` and `/etf/info` return for a closed-end fund: three known CEFs
    /// (PDI, GAB, BST) plus SPY as the open-end control, the conventions of
    /// `fmp_freetier_probe`. Hits the live API (8 calls); run once:
    ///   source <your-keys-env-file> && cargo test --manifest-path \
    ///     src-tauri/Cargo.toml fmp_cef_probe -- --ignored --nocapture
    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY. Probes CEF profile/etf-info shapes."]
    fn fmp_cef_probe() {
        let key = crate::config::AppConfig::from_env()
            .fmp_key()
            .expect("FMP_API_KEY set");
        let http = reqwest::blocking::Client::builder()
            .timeout(FMP_TIMEOUT)
            .build()
            .expect("http client");

        let probe = |label: &str, url: &str, extra: &[(&str, &str)]| {
            let mut q: Vec<(&str, &str)> = vec![("apikey", key.as_str())];
            q.extend_from_slice(extra);
            match http.get(url).query(&q).send() {
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    let mut shown: String = body.chars().take(1500).collect();
                    if body.chars().count() > 1500 {
                        shown.push_str(" …(truncated)");
                    }
                    eprintln!("\n=== {label} [{status}] ===\n{shown}");
                }
                Err(e) => eprintln!("\n=== {label} [transport error] ===\n{e}"),
            }
        };

        let base = "https://financialmodelingprep.com/stable";
        for symbol in ["PDI", "GAB", "BST", "SPY"] {
            probe(
                &format!("profile {symbol}"),
                &format!("{base}{FMP_PROFILE_PATH}"),
                &[("symbol", symbol)],
            );
            probe(
                &format!("etf/info {symbol}"),
                &format!("{base}{FMP_ETF_INFO_PATH}"),
                &[("symbol", symbol)],
            );
        }
    }
}

// ---- Local-suite per-holding surface (`docs/data-sources.md §Portfolio Analysis
// — endpoint surface`): each endpoint fail-soft, recording a tagged gap rather than
// failing, and every actual call streaming one tracker row.

const FMP_INCOME_PATH: &str = "/income-statement";
/// Per-symbol, distinct from the report's market-wide `/earnings-calendar`.
const FMP_SYMBOL_EARNINGS_PATH: &str = "/earnings";
const FMP_NEWS_STOCK_SYMBOL_PATH: &str = "/news/stock";
/// Enough to cover the window since the last full pass with room to spare.
const SYMBOL_EARNINGS_LIMIT: &str = "8";
const SYMBOL_NEWS_LIMIT: &str = "20";
const FMP_BALANCE_SHEET_PATH: &str = "/balance-sheet-statement";
/// Stock surface only (`docs/portfolio-analysis.md` §Starting parameters).
const FMP_CASH_FLOW_PATH: &str = "/cash-flow-statement";
/// Altman Z + Piotroski; stock surface only (`docs/portfolio-analysis.md` §Starting
/// parameters).
const FMP_FINANCIAL_SCORES_PATH: &str = "/financial-scores";
const FMP_ANALYST_ESTIMATES_PATH: &str = "/analyst-estimates";
/// Must exceed the served forward-year count (~5–6) with margin: the endpoint pages
/// farthest-future-first, so a too-small limit cuts the nearest forward year off.
const ESTIMATES_PAGE_LIMIT: &str = "10";
const FMP_DIVIDENDS_PATH: &str = "/dividends";
const FMP_PROFILE_PATH: &str = "/profile";
/// A monthly payer over a 13-month window needs ~15 rows, and the newest rows can be
/// future-dated declarations that consume slots without landing in the window; the
/// margin covers both plus specials, since a truncated pull silently understates the
/// trailing sum.
const DIVIDEND_HISTORY_LIMIT: &str = "60";

/// The quick check matches this prefix to tell a failed retrieval (keep the stored
/// payout leg) from a genuine non-payer (`None` with no gap, which must reach the
/// hurdle as zero).
pub const DIVIDENDS_GAP_PREFIX: &str = "FMP dividends unavailable";

/// The quick check treats these legs as bearing on equity funds alone, so a non-equity
/// fund's empty weightings are the expected shape, never a failed leg
/// (`docs/portfolio-analysis.md` §Evidence floor, §The quick check).
pub const FUND_SECTOR_WEIGHTS_GAP_PREFIX: &str = "FMP sector weightings";
pub const FUND_COUNTRY_WEIGHTS_GAP_PREFIX: &str = "FMP country weightings";

/// The closed-end detection's source leg.
pub const FUND_PROFILE_GAP_PREFIX: &str = "FMP profile (fund structure)";
/// The quick check's mandate comparison runs only when this leg is healthy: an
/// unreadable mandate could fake an asset-class transition.
pub const FUND_INFO_GAP_PREFIX: &str = "FMP etf/info";
const FMP_ETF_INFO_PATH: &str = "/etf/info";
const FMP_ETF_SECTOR_WEIGHTS_PATH: &str = "/etf/sector-weightings";
const FMP_ETF_COUNTRY_WEIGHTS_PATH: &str = "/etf/country-weightings";
const FMP_SECTOR_PE_SNAPSHOT_PATH: &str = "/sector-pe-snapshot";
const FMP_HISTORICAL_SECTOR_PE_PATH: &str = "/historical-sector-pe";

// The per-holding evidence surface (`docs/data-sources.md` §Portfolio Analysis —
// endpoint surface; `crate::portfolio::evidence`): rows rendered as served and never
// read by the engine. Only the paths the block renders are pulled: the TTM pair, not
// the period key-metrics / ratios rows; `grades` and `grades-consensus`, not
// `grades-historical`; `ratings-snapshot`, not `ratings-historical`.
const FMP_RATIOS_TTM_PATH: &str = "/ratios-ttm";
const FMP_KEY_METRICS_TTM_PATH: &str = "/key-metrics-ttm";
const FMP_OWNER_EARNINGS_PATH: &str = "/owner-earnings";
const FMP_ENTERPRISE_VALUES_PATH: &str = "/enterprise-values";
const FMP_PRICE_TARGET_CONSENSUS_PATH: &str = "/price-target-consensus";
const FMP_PRICE_TARGET_SUMMARY_PATH: &str = "/price-target-summary";
const FMP_GRADES_PATH: &str = "/grades";
const FMP_GRADES_CONSENSUS_PATH: &str = "/grades-consensus";
const FMP_RATINGS_SNAPSHOT_PATH: &str = "/ratings-snapshot";
const FMP_INSIDER_SEARCH_PATH: &str = "/insider-trading/search";
const FMP_INSIDER_STATISTICS_PATH: &str = "/insider-trading/statistics";
const FMP_SENATE_TRADES_PATH: &str = "/senate-trades";
const FMP_HOUSE_TRADES_PATH: &str = "/house-trades";
const FMP_SHARES_FLOAT_PATH: &str = "/shares-float";
const FMP_PRODUCT_SEGMENTATION_PATH: &str = "/revenue-product-segmentation";
const FMP_GEOGRAPHIC_SEGMENTATION_PATH: &str = "/revenue-geographic-segmentation";
const FMP_SPLITS_PATH: &str = "/splits";
const FMP_MERGERS_LATEST_PATH: &str = "/mergers-acquisitions-latest";
/// Rows requested from the dated list feeds before the window filter and the count
/// cap; one page each.
const EVIDENCE_LIST_LIMIT: &str = "100";
/// Eight reported quarters plus a year of room for the upcoming announcement rows the
/// feed leads with.
const EVIDENCE_EARNINGS_LIMIT: &str = "16";
const EVIDENCE_LATEST_ROW_LIMIT: &str = "4";

/// The anchor window (12) plus the four extra quarters its oldest TTM print needs.
const INCOME_QUARTERS_LIMIT: &str = "16";

/// The commodity cyclical's mid-cycle window (`docs/trade-opportunities.md` §Starting
/// parameters).
const ANNUAL_INCOME_YEARS_LIMIT: &str = "5";

/// The income pull's sixteen, so the factor reads (`portfolio::factors`) carry an
/// own-history series as deep as the income-side ones; the pre-profit overlay reads
/// the newest four.
const CASH_FLOW_QUARTERS_LIMIT: &str = "16";

/// The income pull's sixteen: the soft forensic flags' year-over-year comparator sits
/// at index 4 under the contiguity gate, and the factor reads (`portfolio::factors`)
/// need the depth. FETCHED VALUES renders the latest eight, joined to the income rows
/// by period end (`docs/portfolio-workflow.md` §Step 6c).
const BALANCE_SHEET_QUARTERS_LIMIT: &str = "16";

/// The tracker-row status a suite fetch's parse earned; HTTP-level gaps never reach
/// this enum, keeping the `GapReason` vocabulary on the row.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RowShape {
    Ok,
    /// The body parsed but carried nothing usable.
    Empty,
    /// The body was served but the schema couldn't read it.
    Malformed,
}

impl RowShape {
    fn as_str(self) -> &'static str {
        match self {
            RowShape::Ok => "ok",
            RowShape::Empty => "empty",
            RowShape::Malformed => "malformed",
        }
    }
}

struct Shaped<T> {
    value: T,
    shape: RowShape,
    detail: Option<String>,
}

impl<T> Shaped<T> {
    fn ok(value: T) -> Self {
        Self {
            value,
            shape: RowShape::Ok,
            detail: None,
        }
    }

    fn empty(value: T) -> Self {
        Self {
            value,
            shape: RowShape::Empty,
            detail: None,
        }
    }

    fn malformed(value: T) -> Self {
        Self {
            value,
            shape: RowShape::Malformed,
            detail: None,
        }
    }

    fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl FmpDataSource {
    /// The disposition plus, on a gap, the sentence naming why ([`http_gap_detail`],
    /// or the reqwest chain on a transport error); the one home for the
    /// cause-on-the-row contract.
    fn get_interpreted(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> (Disposition, Option<String>) {
        match self.get(path, query) {
            Ok((status, body)) => {
                let disposition = interpret_response(status, &body);
                let detail = match &disposition {
                    Disposition::Value(_) => None,
                    Disposition::Gap(_) => Some(http_gap_detail(status, &body)),
                };
                (disposition, detail)
            }
            Err(e) => (
                Disposition::Gap(GapReason::Unavailable),
                Some(format!("{e:#}")),
            ),
        }
    }

    /// The tracker row reflects the shaped outcome: `ok` only when the caller's parse
    /// landed usable data, else `empty` / `malformed` with its cause as the detail. An
    /// HTTP-level gap keeps the `GapReason` label as the status and its cause as the
    /// detail, and returns `Err(reason)` so the caller writes its own gap message.
    fn suite_get_shaped<T>(
        &self,
        kind: &str,
        symbol: &str,
        label: &str,
        path: &str,
        extra: &[(&str, &str)],
        shape: impl FnOnce(&Value) -> Shaped<T>,
    ) -> Result<T, GapReason> {
        if self.progress.is_cancelled() {
            return Err(GapReason::Unavailable);
        }
        self.progress.request_started("FMP", kind, symbol, label);
        let (disposition, gap_detail) = self.get_interpreted(path, extra);
        match disposition {
            Disposition::Value(value) => {
                let shaped = shape(&value);
                self.progress.request_finished(
                    "FMP",
                    kind,
                    symbol,
                    label,
                    shaped.shape.as_str(),
                    shaped.detail,
                );
                Ok(shaped.value)
            }
            Disposition::Gap(reason) => {
                self.progress
                    .request_finished("FMP", kind, symbol, label, reason.as_str(), gap_detail);
                Err(reason)
            }
        }
    }

    /// Newest first; a gap leaves the list empty with a tagged reason.
    pub fn fetch_quarterly_income(
        &self,
        symbol: &str,
        gaps: &mut Vec<String>,
        unit_issues: &mut Vec<crate::portfolio::engine::StatementUnitIssue>,
    ) -> Vec<crate::portfolio::engine::QuarterlyIncomeRow> {
        match self.suite_get_shaped(
            "company-income-q",
            symbol,
            "Quarterly income statements",
            FMP_INCOME_PATH,
            &[
                ("symbol", symbol),
                ("period", "quarter"),
                ("limit", INCOME_QUARTERS_LIMIT),
            ],
            // The shared shaper folds a non-array body into an empty list and drops
            // unreadable rows silently, so both drift checks run here: schema drift and
            // a served array with no readable row read `malformed`, never `empty`.
            |value| {
                let Some(body) = value.as_array() else {
                    gaps.push("FMP quarterly income statements were malformed".to_string());
                    return Shaped::malformed(vec![])
                        .with_detail("non-array body — malformed or drifted response");
                };
                if let Some(issue) = statement_unit_issue(value, "quarterly income statements") {
                    let detail = issue.to_string();
                    gaps.push(detail.clone());
                    unit_issues.push(issue);
                    return Shaped::empty(vec![]).with_detail(detail);
                }
                match quarterly_income_from_value(value) {
                    rows if !rows.is_empty() => Shaped::ok(rows),
                    rows if body.is_empty() => {
                        gaps.push("FMP quarterly income statements were empty".to_string());
                        Shaped::empty(rows)
                    }
                    rows => {
                        gaps.push("FMP quarterly income statements were malformed".to_string());
                        Shaped::malformed(rows)
                            .with_detail("no served row was readable — malformed or drifted response")
                    }
                }
            },
        ) {
            Ok(rows) => rows,
            Err(reason) => {
                gaps.push(format!(
                    "FMP quarterly income statements unavailable ({})",
                    reason.as_str()
                ));
                vec![]
            }
        }
    }

    /// The commodity cyclical's mid-cycle margin window (`portfolio::engine`); the same
    /// fail-soft and drift rules as the quarterly pull, a gap leaving the override to
    /// fall back to the shared ladder.
    pub fn fetch_annual_income(
        &self,
        symbol: &str,
        gaps: &mut Vec<String>,
        unit_issues: &mut Vec<crate::portfolio::engine::StatementUnitIssue>,
    ) -> Vec<crate::portfolio::engine::AnnualIncomeRow> {
        match self.suite_get_shaped(
            "company-income-a",
            symbol,
            "Annual income statements",
            FMP_INCOME_PATH,
            &[
                ("symbol", symbol),
                ("period", "annual"),
                ("limit", ANNUAL_INCOME_YEARS_LIMIT),
            ],
            |value| {
                let Some(body) = value.as_array() else {
                    gaps.push("FMP annual income statements were malformed".to_string());
                    return Shaped::malformed(vec![])
                        .with_detail("non-array body — malformed or drifted response");
                };
                if let Some(issue) = statement_unit_issue(value, "annual income statements") {
                    let detail = issue.to_string();
                    gaps.push(detail.clone());
                    unit_issues.push(issue);
                    return Shaped::empty(vec![]).with_detail(detail);
                }
                match annual_income_from_value(value) {
                    rows if !rows.is_empty() => Shaped::ok(rows),
                    rows if body.is_empty() => {
                        gaps.push("FMP annual income statements were empty".to_string());
                        Shaped::empty(rows)
                    }
                    rows => {
                        gaps.push("FMP annual income statements were malformed".to_string());
                        Shaped::malformed(rows)
                            .with_detail("no served row was readable — malformed or drifted response")
                    }
                }
            },
        ) {
            Ok(rows) => rows,
            Err(reason) => {
                gaps.push(format!(
                    "FMP annual income statements unavailable ({})",
                    reason.as_str()
                ));
                vec![]
            }
        }
    }

    /// Newest first (`docs/portfolio-analysis.md` §Starting parameters); the newest
    /// row's lines are read off by [`BalanceSheetLines::from_newest`]. A gap leaves the
    /// list empty with a tagged reason.
    pub fn fetch_balance_sheet(
        &self,
        symbol: &str,
        gaps: &mut Vec<String>,
        unit_issues: &mut Vec<crate::portfolio::engine::StatementUnitIssue>,
    ) -> Vec<crate::portfolio::engine::QuarterlyBalanceSheetRow> {
        match self.suite_get_shaped(
            "company-balance",
            symbol,
            "Balance sheet",
            FMP_BALANCE_SHEET_PATH,
            &[
                ("symbol", symbol),
                ("period", "quarter"),
                ("limit", BALANCE_SHEET_QUARTERS_LIMIT),
            ],
            // Both drift checks, as in `fetch_quarterly_income`.
            |value| {
                let Some(body) = value.as_array() else {
                    gaps.push("FMP balance sheet was empty or malformed".to_string());
                    return Shaped::malformed(vec![])
                        .with_detail("body was not the expected array shape — malformed or drifted response");
                };
                if let Some(issue) = statement_unit_issue(value, "balance sheet") {
                    let detail = issue.to_string();
                    gaps.push(detail.clone());
                    unit_issues.push(issue);
                    return Shaped::empty(vec![]).with_detail(detail);
                }
                match quarterly_balance_sheet_from_value(value) {
                    rows if !rows.is_empty() => Shaped::ok(rows),
                    rows if body.is_empty() => {
                        gaps.push("FMP balance sheet was empty or malformed".to_string());
                        Shaped::empty(rows)
                    }
                    rows => {
                        gaps.push("FMP balance sheet was empty or malformed".to_string());
                        Shaped::malformed(rows)
                            .with_detail("no served row was readable — malformed or drifted response")
                    }
                }
            },
        ) {
            Ok(rows) => rows,
            Err(reason) => {
                gaps.push(format!("FMP balance sheet unavailable ({})", reason.as_str()));
                vec![]
            }
        }
    }

    /// Fail-soft to `None` with a tagged gap; a served row carrying neither score is no
    /// usable data, so the flags read unevaluable rather than clear.
    pub fn fetch_financial_scores(
        &self,
        symbol: &str,
        gaps: &mut Vec<String>,
    ) -> Option<crate::portfolio::engine::FinancialScores> {
        use crate::portfolio::engine::FinancialScores;
        match self.suite_get_shaped(
            "company-scores",
            symbol,
            "Financial scores",
            FMP_FINANCIAL_SCORES_PATH,
            &[("symbol", symbol)],
            |value| match financial_scores_from_value(value) {
                Some(scores) if scores != FinancialScores::default() => Shaped::ok(Some(scores)),
                Some(_) => {
                    gaps.push("FMP financial scores were empty or malformed".to_string());
                    Shaped::empty(None)
                }
                None => {
                    gaps.push("FMP financial scores were empty or malformed".to_string());
                    if value.as_array().is_some_and(|a| a.is_empty()) {
                        Shaped::empty(None)
                    } else {
                        Shaped::malformed(None).with_detail(
                            "body was not the expected array shape — malformed or drifted response",
                        )
                    }
                }
            },
        ) {
            Ok(scores) => scores,
            Err(reason) => {
                gaps.push(format!("FMP financial scores unavailable ({})", reason.as_str()));
                None
            }
        }
    }

    /// Newest first (`docs/portfolio-analysis.md` §Starting parameters); a gap leaves
    /// the list empty with a tagged reason.
    pub fn fetch_quarterly_cash_flow(
        &self,
        symbol: &str,
        gaps: &mut Vec<String>,
        unit_issues: &mut Vec<crate::portfolio::engine::StatementUnitIssue>,
    ) -> Vec<crate::portfolio::engine::QuarterlyCashFlowRow> {
        match self.suite_get_shaped(
            "company-cashflow-q",
            symbol,
            "Quarterly cash-flow statements",
            FMP_CASH_FLOW_PATH,
            &[
                ("symbol", symbol),
                ("period", "quarter"),
                ("limit", CASH_FLOW_QUARTERS_LIMIT),
            ],
            // Both drift checks, as in `fetch_quarterly_income`.
            |value| {
                let Some(body) = value.as_array() else {
                    gaps.push("FMP quarterly cash-flow statements were malformed".to_string());
                    return Shaped::malformed(vec![])
                        .with_detail("non-array body — malformed or drifted response");
                };
                if let Some(issue) = statement_unit_issue(value, "quarterly cash-flow statements") {
                    let detail = issue.to_string();
                    gaps.push(detail.clone());
                    unit_issues.push(issue);
                    return Shaped::empty(vec![]).with_detail(detail);
                }
                match quarterly_cash_flow_from_value(value) {
                    rows if !rows.is_empty() => Shaped::ok(rows),
                    rows if body.is_empty() => {
                        gaps.push("FMP quarterly cash-flow statements were empty".to_string());
                        Shaped::empty(rows)
                    }
                    rows => {
                        gaps.push("FMP quarterly cash-flow statements were malformed".to_string());
                        Shaped::malformed(rows)
                            .with_detail("no served row was readable — malformed or drifted response")
                    }
                }
            },
        ) {
            Ok(rows) => rows,
            Err(reason) => {
                gaps.push(format!(
                    "FMP quarterly cash-flow statements unavailable ({})",
                    reason.as_str()
                ));
                vec![]
            }
        }
    }

    /// The NTM forward consensus ([`consensus_from_value`]), fail-soft to `None` with a
    /// tagged gap. A too-small page limit would cut the nearest forward year
    /// ([`ESTIMATES_PAGE_LIMIT`]) and the NTM read silently becomes a full-weight
    /// far-year read.
    pub fn fetch_analyst_estimates(
        &self,
        symbol: &str,
        gaps: &mut Vec<String>,
    ) -> Option<crate::portfolio::engine::ConsensusEstimate> {
        // The ET session: the blend time-weights by twelve-month overlap from here, so
        // a UTC date on an evening run shifts it a day and, at a fiscal-year boundary,
        // the row selection itself.
        let today = crate::market_clock::et_session_date(Utc::now())
            .format("%Y-%m-%d")
            .to_string();
        match self.suite_get_shaped(
            "company-estimates",
            symbol,
            "Analyst estimates",
            FMP_ANALYST_ESTIMATES_PATH,
            &[("symbol", symbol), ("period", "annual"), ("limit", ESTIMATES_PAGE_LIMIT)],
            // `consensus_from_value` folds a non-array body into `None` and drops
            // undatable rows silently, so the drift checks run here: a non-empty body
            // with no datable row is drift, a past-only or empty body the honest empty.
            |value| {
                let Some(body) = value.as_array() else {
                    gaps.push("FMP analyst estimates were malformed (non-array body)".to_string());
                    return Shaped::malformed(None)
                        .with_detail("non-array body — malformed or drifted response");
                };
                match consensus_from_value(value, &today) {
                    Some(c) => Shaped::ok(Some(c)),
                    None => {
                        let any_datable = body.iter().any(|row| {
                            row.get("date").and_then(Value::as_str).is_some_and(|d| {
                                chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok()
                            })
                        });
                        if body.is_empty() || any_datable {
                            gaps.push(
                                "FMP analyst estimates carried no forward-dated consensus \
                                 (a past fiscal-year row is never used as forward)"
                                    .to_string(),
                            );
                            Shaped::empty(None)
                        } else {
                            gaps.push(
                                "FMP analyst estimates were malformed (no datable rows)"
                                    .to_string(),
                            );
                            Shaped::malformed(None).with_detail(
                                "no served row carried a datable date — malformed or drifted response",
                            )
                        }
                    }
                }
            },
        ) {
            Ok(consensus) => consensus,
            Err(reason) => {
                gaps.push(format!("FMP analyst estimates unavailable ({})", reason.as_str()));
                None
            }
        }
    }

    /// The strict form of [`Self::fetch_analyst_estimates`]: a failed retrieval is
    /// `Err` (the caller types the family `unknown`), a body carrying no forward-dated
    /// consensus an honest `Ok(None)`.
    pub fn fetch_analyst_estimates_strict(
        &self,
        symbol: &str,
    ) -> Result<Option<crate::portfolio::engine::ConsensusEstimate>> {
        // The ET session (see `fetch_analyst_estimates`).
        let today = crate::market_clock::et_session_date(Utc::now())
            .format("%Y-%m-%d")
            .to_string();
        match self.suite_get_shaped(
            "company-estimates",
            symbol,
            "Analyst estimates",
            FMP_ANALYST_ESTIMATES_PATH,
            &[("symbol", symbol), ("period", "annual"), ("limit", ESTIMATES_PAGE_LIMIT)],
            |value| {
                if !value.is_array() {
                    let msg = "FMP analyst estimates returned a non-array body — malformed or \
                               drifted response";
                    return Shaped::malformed(Err(anyhow::anyhow!(msg))).with_detail(msg);
                }
                // The shared shaper drops undatable rows silently, but here `Ok(None)` is
                // the read the Revision family clears on, so a body of unreadable dates
                // must surface as `Err`. Non-zero-padded dates parse fine and stay valid.
                for row in value.as_array().into_iter().flatten() {
                    let Some(d) = row.get("date").and_then(Value::as_str) else {
                        let msg = "an estimates row carried no date — malformed or drifted \
                                   response";
                        return Shaped::malformed(Err(anyhow::anyhow!(msg))).with_detail(msg);
                    };
                    if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
                        let msg = format!(
                            "an estimates row carried an undatable date {d:?} — malformed or \
                             drifted response"
                        );
                        return Shaped::malformed(Err(anyhow::anyhow!(msg.clone())))
                            .with_detail(msg);
                    }
                }
                match consensus_from_value(value, &today) {
                    Some(c) => Shaped::ok(Ok(Some(c))),
                    None => Shaped::empty(Ok(None)),
                }
            },
        ) {
            Ok(result) => result,
            Err(reason) => {
                anyhow::bail!("FMP analyst estimates unavailable ({})", reason.as_str())
            }
        }
    }

    /// Trailing, not a forward estimate. `None` with no gap for a non-payer; a failed
    /// call records the gap.
    pub fn fetch_ttm_dividends(&self, symbol: &str, gaps: &mut Vec<String>) -> Option<f64> {
        self.fetch_dividend_pull(symbol, gaps).ttm
    }

    /// Fail-soft to the empty pull with a tagged gap; a confirmed non-payer is the
    /// empty pull with no gap (`docs/portfolio-workflow.md` §Step 6c).
    pub fn fetch_dividend_pull(&self, symbol: &str, gaps: &mut Vec<String>) -> DividendPull {
        // The ET session: the window is bounded on both sides against rows dated by
        // market day, so a UTC date on an evening run both admits a next-session
        // declaration and slides the 365-day cutoff off the oldest in-window payment.
        let today = crate::market_clock::et_session_date(Utc::now());
        match self.suite_get_shaped(
            "company-dividends",
            symbol,
            "Dividend history",
            FMP_DIVIDENDS_PATH,
            &[("symbol", symbol), ("limit", DIVIDEND_HISTORY_LIMIT)],
            // Any unreadable body must record the gap: `None` with no gap is the
            // confirmed-non-payer contract, and a drifted body must never read as a
            // dividend elimination downstream.
            |value| match dividends_from_value(value, today) {
                Ok(pull) if pull.ttm.is_some() || !pull.recent.is_empty() => Shaped::ok(pull),
                // The confirmed non-payer, deliberately no gap.
                Ok(pull) => Shaped::empty(pull),
                Err(e) => {
                    let detail = format!("{e:#}");
                    gaps.push(format!("{DIVIDENDS_GAP_PREFIX} ({e})"));
                    Shaped::malformed(DividendPull::default()).with_detail(detail)
                }
            },
        ) {
            Ok(pull) => pull,
            Err(reason) => {
                gaps.push(format!("{DIVIDENDS_GAP_PREFIX} ({})", reason.as_str()));
                DividendPull::default()
            }
        }
    }

    /// Fail-soft, split three ways (`docs/portfolio-analysis.md §Asset eligibility`):
    /// a resolved body; a definitive 2xx no-such-symbol (`Unresolved`); or any gate,
    /// transport failure, or unreadable body (`Unverified`, never mistaken for a
    /// missing listing).
    pub fn fetch_profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
        use crate::portfolio::listing::ProfileLookup;
        match self.suite_get_shaped(
            "company-profile",
            symbol,
            "Company profile (identity)",
            FMP_PROFILE_PATH,
            &[("symbol", symbol)],
            |value| {
                let lookup = profile_identity_from_value(value);
                match &lookup {
                    ProfileLookup::Resolved(_) => Shaped::ok(lookup),
                    ProfileLookup::Unresolved => Shaped::empty(lookup),
                    ProfileLookup::Unverified(msg) => {
                        let detail = msg.clone();
                        Shaped::malformed(lookup).with_detail(detail)
                    }
                }
            },
        ) {
            Ok(lookup) => lookup,
            Err(reason) => {
                ProfileLookup::Unverified(format!("FMP profile unavailable ({})", reason.as_str()))
            }
        }
    }

    /// The suite's only deep price source; the bulk per-holding load deliberately
    /// rides the paid FMP key, which the 429 ladder covers.
    pub fn fetch_dated_eod(
        &self,
        symbol: &str,
        lookback_days: i64,
    ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
        // Fetch-range upper bound deliberately stays UTC (see `fetch_index_performance`).
        let to = Utc::now().date_naive();
        let from = (to - Duration::days(lookback_days))
            .format("%Y-%m-%d")
            .to_string();
        let to = to.format("%Y-%m-%d").to_string();
        match self.suite_get_shaped(
            "company-eod-deep",
            symbol,
            "Deep price history",
            FMP_EOD_PATH,
            &[("symbol", symbol), ("from", &from), ("to", &to)],
            |value| match dated_eod_from_value(value) {
                Ok(rows) if !rows.is_empty() => Shaped::ok(Ok(rows)),
                // Drift, not emptiness: `Err` so the caller records the labeled
                // fallback rather than reading an honest no-history.
                Ok(_) if value.as_array().is_some_and(|a| !a.is_empty()) => {
                    let msg = "no served row was readable — malformed or drifted response";
                    Shaped::malformed(Err(anyhow::anyhow!("FMP dated EOD: {msg}")))
                        .with_detail(msg)
                }
                Ok(rows) => Shaped::empty(Ok(rows)),
                Err(e) => {
                    let detail = format!("{e:#}");
                    Shaped::malformed(Err(e)).with_detail(detail)
                }
            },
        ) {
            Ok(result) => result,
            Err(reason) => {
                anyhow::bail!("FMP dated EOD unavailable ({})", reason.as_str())
            }
        }
    }

    /// [`Self::fetch_dated_eod`] keeping each row's share volume for the tradability
    /// flag (`portfolio::tradability`).
    pub fn fetch_dated_bars(
        &self,
        symbol: &str,
        lookback_days: i64,
    ) -> Result<Vec<crate::portfolio::engine::DailyBar>> {
        // Fetch-range upper bound deliberately stays UTC (see `fetch_index_performance`).
        let to = Utc::now().date_naive();
        let from = (to - Duration::days(lookback_days))
            .format("%Y-%m-%d")
            .to_string();
        let to = to.format("%Y-%m-%d").to_string();
        match self.suite_get_shaped(
            "company-eod-bars",
            symbol,
            "Deep price and volume history",
            FMP_EOD_PATH,
            &[("symbol", symbol), ("from", &from), ("to", &to)],
            |value| match dated_bars_from_value(value) {
                Ok(rows) if !rows.is_empty() => Shaped::ok(Ok(rows)),
                // Drift, not emptiness.
                Ok(_) if value.as_array().is_some_and(|a| !a.is_empty()) => {
                    let msg = "no served row was readable — malformed or drifted response";
                    Shaped::malformed(Err(anyhow::anyhow!("FMP dated bars: {msg}")))
                        .with_detail(msg)
                }
                Ok(rows) => Shaped::empty(Ok(rows)),
                Err(e) => {
                    let detail = format!("{e:#}");
                    Shaped::malformed(Err(e)).with_detail(detail)
                }
            },
        ) {
            Ok(result) => result,
            Err(reason) => {
                anyhow::bail!("FMP dated bars unavailable ({})", reason.as_str())
            }
        }
    }

    /// Gold, the one commodity priced through FMP (`docs/data-sources.md §Portfolio
    /// Analysis — endpoint surface`). Level basis, dated on the run's pinned ET
    /// `session`: the quote is live at fetch time, not a dated close. `Err` is the
    /// caller's typed gap, never a run failure.
    pub fn fetch_commodity_quote(
        &self,
        symbol: &str,
        session: chrono::NaiveDate,
    ) -> Result<crate::portfolio::engine::DatedValue> {
        match self.suite_get_shaped(
            "commodity-quote",
            symbol,
            "Commodity quote",
            FMP_QUOTE_PATH,
            &[("symbol", symbol)],
            |value| match company_quote_from_value(value) {
                Some(CompanyQuote {
                    price: Some(price), ..
                }) => Shaped::ok(Ok(price)),
                // A served zero or negative print is no price.
                Some(CompanyQuote {
                    unusable_price: Some(served),
                    ..
                }) => {
                    let msg = format!(
                        "FMP commodity quote for {symbol} carried no usable price (served {served})"
                    );
                    Shaped::empty(Err(msg.clone())).with_detail(msg)
                }
                None if value.as_array().is_some_and(|a| a.is_empty()) => Shaped::empty(Err(
                    format!("FMP commodity quote for {symbol} carried no price"),
                ))
                .with_detail("quote array was empty"),
                _ => Shaped::malformed(Err(format!(
                    "FMP commodity quote for {symbol} carried no price"
                )))
                .with_detail("no readable price — malformed or drifted response"),
            },
        ) {
            Ok(Ok(price)) => Ok(crate::portfolio::engine::DatedValue {
                date: session.format("%Y-%m-%d").to_string(),
                value: price,
            }),
            Ok(Err(msg)) => Err(anyhow::anyhow!(msg)),
            Err(reason) => anyhow::bail!(
                "FMP commodity quote for {symbol} unavailable ({})",
                reason.as_str()
            ),
        }
    }

    /// `Err` on a gate, transport failure, or a body carrying no price, so the quick
    /// check types the family `unknown` rather than clearing it silently
    /// (`docs/portfolio-analysis.md` §The quick check).
    pub fn fetch_live_price(&self, symbol: &str) -> Result<f64> {
        match self.suite_get_shaped(
            "quick-quote",
            symbol,
            "Live quote",
            FMP_QUOTE_PATH,
            &[("symbol", symbol)],
            |value| match company_quote_from_value(value) {
                Some(q) => match (q.price, q.unusable_price) {
                    (Some(p), _) => Shaped::ok(Ok(p)),
                    // A served zero or negative print is a failed refresh, never a
                    // price the band monitor could read as a crossing.
                    (None, Some(served)) => {
                        let msg = format!(
                            "FMP quote carried no usable price for {symbol} (served {served})"
                        );
                        Shaped::empty(Err(anyhow::anyhow!(msg.clone()))).with_detail(msg)
                    }
                    (None, None) => Shaped::empty(Err(anyhow::anyhow!(
                        "FMP quote carried no price for {symbol}"
                    ))),
                },
                // A served-empty array is an honest empty answer, not drift; the caller
                // still gets `Err` either way.
                None if value.as_array().is_some_and(|a| a.is_empty()) => Shaped::empty(Err(
                    anyhow::anyhow!("FMP quote was empty for {symbol}"),
                )),
                None => {
                    let msg = format!("FMP quote was malformed for {symbol}");
                    Shaped::malformed(Err(anyhow::anyhow!(msg.clone()))).with_detail(msg)
                }
            },
        ) {
            Ok(result) => result,
            Err(reason) => {
                anyhow::bail!("FMP quote unavailable ({})", reason.as_str())
            }
        }
    }

    /// Newest first. `Err` on a failed retrieval (the caller types the family
    /// `unknown`); an empty list is an honest no-rows read.
    pub fn fetch_symbol_earnings(&self, symbol: &str) -> Result<Vec<SymbolEarningsRow>> {
        self.fetch_symbol_earnings_as("quick-earnings", symbol, SYMBOL_EARNINGS_LIMIT)
    }

    fn fetch_symbol_earnings_as(
        &self,
        kind: &str,
        symbol: &str,
        limit: &str,
    ) -> Result<Vec<SymbolEarningsRow>> {
        match self.suite_get_shaped(
            kind,
            symbol,
            "Earnings rows",
            FMP_SYMBOL_EARNINGS_PATH,
            &[("symbol", symbol), ("limit", limit)],
            |value| match symbol_earnings_from_value(value) {
                Ok(rows) if !rows.is_empty() => Shaped::ok(Ok(rows)),
                // Drift, not emptiness: `Err` so the quick check types the family
                // `unknown` instead of clearing it on a drifted body.
                Ok(_) if value.as_array().is_some_and(|a| !a.is_empty()) => {
                    let msg = "no served row was readable — malformed or drifted response";
                    Shaped::malformed(Err(anyhow::anyhow!("FMP earnings: {msg}")))
                        .with_detail(msg)
                }
                Ok(rows) => Shaped::empty(Ok(rows)),
                Err(e) => {
                    let detail = format!("{e:#}");
                    Shaped::malformed(Err(e)).with_detail(detail)
                }
            },
        ) {
            Ok(result) => result,
            Err(reason) => {
                anyhow::bail!("FMP earnings unavailable ({})", reason.as_str())
            }
        }
    }

    /// `from` is an ISO date. `Err` on a failed retrieval; fail-soft at the caller.
    pub fn fetch_symbol_news_since(
        &self,
        symbol: &str,
        from: &str,
    ) -> Result<Vec<SymbolNewsItem>> {
        match self.suite_get_shaped(
            "news-leads",
            symbol,
            "Symbol news",
            FMP_NEWS_STOCK_SYMBOL_PATH,
            &[
                ("symbols", symbol),
                ("from", from),
                ("limit", SYMBOL_NEWS_LIMIT),
            ],
            |value| match symbol_news_from_value(value, from) {
                Ok(items) if !items.is_empty() => Shaped::ok(Ok(items)),
                Ok(items) => {
                    // Zero items splits on readability, not row count: the shaper's date
                    // filter is domain logic, so a body of readable-but-older-than-`from`
                    // rows is the honest empty; only a non-empty body with no readable
                    // row is drift.
                    let any_readable = value.as_array().is_some_and(|a| {
                        a.iter().any(|row| canonical_news_date(row).is_some())
                    });
                    if value.as_array().is_some_and(|a| !a.is_empty()) && !any_readable {
                        let msg = "no served row was readable — malformed or drifted response";
                        Shaped::malformed(Err(anyhow::anyhow!("FMP news/stock: {msg}")))
                            .with_detail(msg)
                    } else {
                        Shaped::empty(Ok(items))
                    }
                }
                Err(e) => {
                    let detail = format!("{e:#}");
                    Shaped::malformed(Err(e)).with_detail(detail)
                }
            },
        ) {
            Ok(result) => result,
            Err(reason) => {
                anyhow::bail!("FMP news/stock unavailable ({})", reason.as_str())
            }
        }
    }

    /// The full pass's per-fund surface (`docs/portfolio-analysis.md` §Asset
    /// eligibility), each endpoint fail-soft to a tagged gap on the returned record.
    pub fn fetch_fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
        self.fetch_fund_data_inner(symbol, true)
    }

    /// The same surface without the `profile` leg: the sweep never re-runs closed-end
    /// detection, so fetching it would spend an unconsumed request and, on failure,
    /// push a detection gap into a sweep that runs no detection (`docs/data-sources.md`
    /// §Portfolio Analysis — endpoint surface).
    pub fn fetch_fund_refresh_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
        self.fetch_fund_data_inner(symbol, false)
    }

    fn fetch_fund_data_inner(
        &self,
        symbol: &str,
        with_profile: bool,
    ) -> crate::portfolio::fund::FundData {
        let mut fund = crate::portfolio::fund::FundData {
            symbol: symbol.to_string(),
            ..Default::default()
        };
        if let Err(reason) = self.suite_get_shaped(
            "fund-info",
            symbol,
            "Fund metadata",
            FMP_ETF_INFO_PATH,
            &[("symbol", symbol)],
            |value| {
                // A served-empty array (no fund info) is an honest empty, not drift.
                if value.as_array().is_some_and(|a| a.is_empty()) {
                    return Shaped::empty(());
                }
                if !fund_info_into(value, &mut fund) {
                    return Shaped::malformed(()).with_detail("FMP etf/info was malformed");
                }
                // A readable body that filled nothing (`{}`) is no usable data.
                if fund.name.is_none()
                    && fund.asset_class.is_none()
                    && fund.expense_ratio.is_none()
                    && fund.aum.is_none()
                    && fund.nav.is_none()
                {
                    Shaped::empty(())
                } else {
                    Shaped::ok(())
                }
            },
        ) {
            fund.gaps
                .push(format!("{FUND_INFO_GAP_PREFIX} unavailable ({})", reason.as_str()));
        }
        // The `/profile` read closed-end detection consumes (`isFund` plus the
        // description's closed-end fragment). CEFs are exactly where `etf/info` serves
        // `[]`, so the structure signal must come from the profile; a failed read
        // records the gap and detection reads not-a-CEF rather than guessing.
        if with_profile {
            if let Err(reason) = self.suite_get_shaped(
                "fund-profile",
                symbol,
                "Fund structure profile",
                FMP_PROFILE_PATH,
                &[("symbol", symbol)],
                |value| {
                    // Every answer that leaves detection unable to run says so on the
                    // record; the gaps are pushed here, not on the outer Err arm, because
                    // `suite_get_shaped` folds served bodies, malformed included, into
                    // `Ok`. An empty array is FMP's honest "no profile", still recorded
                    // since this is the one read closed-end detection keys on.
                    if value.as_array().is_some_and(|a| a.is_empty()) {
                        fund.gaps.push(format!(
                            "{FUND_PROFILE_GAP_PREFIX} was empty — closed-end \
                             detection cannot run"
                        ));
                        return Shaped::empty(());
                    }
                    let Some(obj) = value
                        .as_array()
                        .and_then(|a| a.first())
                        .or(Some(value))
                        .filter(|o| o.is_object())
                    else {
                        fund.gaps.push(format!(
                            "{FUND_PROFILE_GAP_PREFIX} was malformed — closed-end \
                             detection cannot run"
                        ));
                        return Shaped::malformed(())
                            .with_detail("FMP profile body unreadable (drifted response shape)");
                    };
                    // Every real profile carries the boolean `isFund`, so a served object
                    // without a readable flag is drift on the one field detection keys on.
                    let Some(is_fund) = obj.get("isFund").and_then(Value::as_bool) else {
                        fund.gaps.push(format!(
                            "{FUND_PROFILE_GAP_PREFIX} was malformed — closed-end \
                             detection cannot run"
                        ));
                        return Shaped::malformed(())
                            .with_detail("profile served without a readable isFund flag");
                    };
                    fund.profile_is_fund = Some(is_fund);
                    fund.profile_description = obj
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from);
                    // A fund-flagged profile with no description leaves the detection
                    // conjunction unanswerable; a definitive `isFund: false` is the one
                    // silent answer, nothing to detect.
                    if is_fund && fund.profile_description.is_none() {
                        fund.gaps.push(format!(
                            "{FUND_PROFILE_GAP_PREFIX} carried no description — \
                             closed-end detection cannot distinguish a CEF from an \
                             open-end fund"
                        ));
                    }
                    Shaped::ok(())
                },
            ) {
                fund.gaps.push(format!(
                    "{FUND_PROFILE_GAP_PREFIX} unavailable ({}) — closed-end detection \
                     cannot run",
                    reason.as_str()
                ));
            }
        }
        match self.suite_get_shaped(
            "fund-sectors",
            symbol,
            "Fund sector weightings",
            FMP_ETF_SECTOR_WEIGHTS_PATH,
            &[("symbol", symbol)],
            // Drift checks before the shaper, which folds non-array into an empty list
            // and drops unreadable rows, so a drifted body never reads "were empty".
            |value| {
                let Some(body) = value.as_array() else {
                    fund.gaps
                        .push(format!("{FUND_SECTOR_WEIGHTS_GAP_PREFIX} were malformed"));
                    return Shaped::malformed(vec![])
                        .with_detail("non-array body — malformed or drifted response");
                };
                let weights = weights_from_value(value, "sector");
                if !weights.is_empty() {
                    Shaped::ok(weights)
                } else if body.is_empty() {
                    fund.gaps
                        .push(format!("{FUND_SECTOR_WEIGHTS_GAP_PREFIX} were empty"));
                    Shaped::empty(weights)
                } else {
                    fund.gaps
                        .push(format!("{FUND_SECTOR_WEIGHTS_GAP_PREFIX} were malformed"));
                    Shaped::malformed(weights)
                        .with_detail("no served row was readable — malformed or drifted response")
                }
            },
        ) {
            Ok(weights) => fund.sector_weights = weights,
            Err(reason) => fund.gaps.push(format!(
                "{FUND_SECTOR_WEIGHTS_GAP_PREFIX} unavailable ({})",
                reason.as_str()
            )),
        }
        match self.suite_get_shaped(
            "fund-countries",
            symbol,
            "Fund country weightings",
            FMP_ETF_COUNTRY_WEIGHTS_PATH,
            &[("symbol", symbol)],
            // The same drift splits as the sector weightings.
            |value| {
                let Some(body) = value.as_array() else {
                    fund.gaps
                        .push(format!("{FUND_COUNTRY_WEIGHTS_GAP_PREFIX} were malformed"));
                    return Shaped::malformed(vec![])
                        .with_detail("non-array body — malformed or drifted response");
                };
                let weights = weights_from_value(value, "country");
                if !weights.is_empty() {
                    Shaped::ok(weights)
                } else if body.is_empty() {
                    fund.gaps
                        .push(format!("{FUND_COUNTRY_WEIGHTS_GAP_PREFIX} were empty"));
                    Shaped::empty(weights)
                } else {
                    fund.gaps
                        .push(format!("{FUND_COUNTRY_WEIGHTS_GAP_PREFIX} were malformed"));
                    Shaped::malformed(weights)
                        .with_detail("no served row was readable — malformed or drifted response")
                }
            },
        ) {
            Ok(weights) => fund.country_weights = weights,
            Err(reason) => fund.gaps.push(format!(
                "{FUND_COUNTRY_WEIGHTS_GAP_PREFIX} unavailable ({})",
                reason.as_str()
            )),
        }
        fund
    }

    /// Run-level, shared across funds (`docs/data-sources.md`). `date` is one candidate
    /// ET session; the caller walks back over earlier weekdays until one serves, one
    /// call per candidate tried. The walk exists because the empty answer is a 200,
    /// and an unnoticed empty surface would silently abstain every priced fund; a
    /// weekday holiday still serves a full snapshot with carried values, so the second
    /// candidate is a safety net rather than an expected cost.
    pub fn fetch_sector_pe_snapshot(
        &self,
        exchange: &str,
        date: &str,
    ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        match self.suite_get_shaped(
            "sector-pe",
            exchange,
            "Sector P/E snapshot",
            FMP_SECTOR_PE_SNAPSHOT_PATH,
            &[("exchange", exchange), ("date", date)],
            // A non-array body, a served array with no readable row, or an off-board
            // row is drift, `malformed` on the row; the return contract deliberately
            // stays `Ok(vec![])` so the bounded date walk-back is unchanged.
            |value| {
                let Some(body) = value.as_array() else {
                    return Shaped::malformed(vec![])
                        .with_detail("non-array body — malformed or drifted response");
                };
                match sector_pe_rows_from_value(value, exchange) {
                    Err(e) => Shaped::malformed(vec![]).with_detail(format!("{e:#}")),
                    Ok(rows) if !rows.is_empty() => Shaped::ok(rows),
                    Ok(rows) if body.is_empty() => Shaped::empty(rows),
                    Ok(rows) => Shaped::malformed(rows)
                        .with_detail("no served row was readable — malformed or drifted response"),
                }
            },
        ) {
            Ok(rows) => Ok(rows),
            Err(reason) => anyhow::bail!(
                "FMP sector-pe-snapshot unavailable for {exchange} ({})",
                reason.as_str()
            ),
        }
    }

    /// Memoized by the caller across funds (`docs/data-sources.md`).
    pub fn fetch_historical_sector_pe(
        &self,
        sector: &str,
        exchange: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        match self.suite_get_shaped(
            "sector-pe-history",
            sector,
            "Sector P/E history",
            FMP_HISTORICAL_SECTOR_PE_PATH,
            &[
                ("sector", sector),
                ("exchange", exchange),
                ("from", from),
                ("to", to),
            ],
            // The same drift splits as the snapshot; the return contract stays.
            |value| {
                let Some(body) = value.as_array() else {
                    return Shaped::malformed(vec![])
                        .with_detail("non-array body — malformed or drifted response");
                };
                match sector_pe_rows_from_value(value, exchange) {
                    Err(e) => Shaped::malformed(vec![]).with_detail(format!("{e:#}")),
                    Ok(rows) if !rows.is_empty() => Shaped::ok(rows),
                    Ok(rows) if body.is_empty() => Shaped::empty(rows),
                    Ok(rows) => Shaped::malformed(rows)
                        .with_detail("no served row was readable — malformed or drifted response"),
                }
            },
        ) {
            Ok(rows) => Ok(rows),
            Err(reason) => anyhow::bail!(
                "FMP historical-sector-pe unavailable for {sector}/{exchange} ({})",
                reason.as_str()
            ),
        }
    }
}

/// The lines the single-row consumers read off the newest quarterly print
/// (`docs/portfolio-analysis.md` §Starting parameters).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BalanceSheetLines {
    pub total_debt: Option<f64>,
    pub total_equity: Option<f64>,
    pub cash_and_equivalents: Option<f64>,
    pub short_term_investments: Option<f64>,
}

impl BalanceSheetLines {
    /// The newest row by period end, an equal-date conflict keeping the first-served
    /// row: exactly the row `engine::canonicalize_statements` retains, so the flat
    /// lines and the retained row can never come from different prints.
    pub fn from_newest(rows: &[crate::portfolio::engine::QuarterlyBalanceSheetRow]) -> Self {
        // `min_by` under the descending comparator returns the first of equal rows,
        // where `max_by` would return the last.
        rows.iter()
            .min_by(|a, b| {
                b.period_end
                    .cmp(&a.period_end)
                    .then_with(|| b.filing_date.cmp(&a.filing_date))
            })
            .map(|r| Self {
                total_debt: r.total_debt,
                total_equity: r.total_equity,
                cash_and_equivalents: r.cash_and_equivalents,
                short_term_investments: r.short_term_investments,
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SymbolEarningsRow {
    /// The announcement date, ISO.
    pub date: String,
    pub eps_actual: Option<f64>,
    pub eps_estimated: Option<f64>,
    pub revenue_actual: Option<f64>,
}

/// Newest first; a dateless row is skipped. A non-array body is `Err`, never an empty
/// success, so the quick check types the family `unknown` rather than reading "no new
/// evidence" off a body it couldn't interpret.
fn symbol_earnings_from_value(value: &Value) -> Result<Vec<SymbolEarningsRow>> {
    let Some(rows) = value.as_array() else {
        anyhow::bail!("FMP earnings returned a non-array body — malformed or drifted response");
    };
    let mut out: Vec<SymbolEarningsRow> = rows
        .iter()
        .filter_map(|row| {
            // The canonical date render: the quick check compares this field
            // lexicographically against its last-pass date.
            let date = row.get("date").and_then(Value::as_str).and_then(canonical_date)?;
            Some(SymbolEarningsRow {
                date,
                eps_actual: row.get("epsActual").and_then(Value::as_f64),
                eps_estimated: row.get("epsEstimated").and_then(Value::as_f64),
                revenue_actual: row.get("revenueActual").and_then(Value::as_f64),
            })
        })
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SymbolNewsItem {
    /// `YYYY-MM-DD HH:MM:SS` as FMP serves it; date-prefix comparable.
    pub published_date: String,
    pub title: String,
    pub site: Option<String>,
    /// A lead the research loop may deep-read, never evidence; `None` on rows the
    /// wire served without one.
    #[serde(default)]
    pub url: Option<String>,
}

/// Keeps items published on or after `from` client-side: the "fresh since the last
/// full pass" test must not lean on the server-side `from` filter alone. A non-array
/// body is `Err`, never an empty success (see [`symbol_earnings_from_value`]).
fn symbol_news_from_value(value: &Value, from: &str) -> Result<Vec<SymbolNewsItem>> {
    let Some(rows) = value.as_array() else {
        anyhow::bail!("FMP news/stock returned a non-array body — malformed or drifted response");
    };
    Ok(rows
        .iter()
        .filter_map(|row| {
            // The canonical prefix, so the lexicographic filter compares fixed-width
            // dates ("2026-08-01 09:30:00" >= "2026-07-20").
            let published_date = canonical_news_date(row)?;
            if published_date.as_str() < from {
                return None;
            }
            Some(SymbolNewsItem {
                published_date,
                title: row
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                site: row.get("site").and_then(Value::as_str).map(str::to_string),
                url: row.get("url").and_then(Value::as_str).map(str::to_string),
            })
        })
        .collect())
}

/// Reject a whole served statement surface if any row has foreign or unknown
/// denomination. Filtering individual rows could silently build a different TTM
/// window, or combine a foreign balance sheet with USD cash flows.
fn statement_unit_issue(
    value: &Value,
    surface: &str,
) -> Option<crate::portfolio::engine::StatementUnitIssue> {
    value.as_array()?.iter().find_map(|row| {
        let currency = row
            .get("reportedCurrency")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty());
        if currency.is_some_and(|c| c.eq_ignore_ascii_case("USD")) {
            None
        } else {
            Some(crate::portfolio::engine::StatementUnitIssue {
                surface: surface.into(),
                reported_currency: currency.map(str::to_string),
            })
        }
    })
}

/// Newest first. A row without a datable period date is skipped, and dates are stored
/// as their canonical render ([`canonical_date`]) so the sort and the restatement
/// tie-break (`engine::canonicalize_statements`) stay lexicographic. Equity prefers
/// `totalStockholdersEquity` (the parent-only line P/B reads) over `totalEquity`
/// (which folds in minority interest).
fn quarterly_balance_sheet_from_value(
    value: &Value,
) -> Vec<crate::portfolio::engine::QuarterlyBalanceSheetRow> {
    let Some(rows) = value.as_array() else {
        return vec![];
    };
    rows.iter()
        .filter_map(|row| {
            let period_end = row
                .get("date")
                .and_then(Value::as_str)
                .and_then(canonical_date)?;
            Some(crate::portfolio::engine::QuarterlyBalanceSheetRow {
                period_end,
                // Datable-string-first per key — see quarterly_cash_flow_from_value.
                filing_date: row
                    .get("filingDate")
                    .and_then(Value::as_str)
                    .and_then(canonical_date)
                    .or_else(|| {
                        row.get("fillingDate")
                            .and_then(Value::as_str)
                            .and_then(canonical_date)
                    }),
                total_debt: row.get("totalDebt").and_then(Value::as_f64),
                // Numeric-first per key: a present-but-null preferred line must
                // still fall through to the alternate, so the fallback runs after
                // `as_f64`.
                total_equity: row
                    .get("totalStockholdersEquity")
                    .and_then(Value::as_f64)
                    .or_else(|| row.get("totalEquity").and_then(Value::as_f64)),
                cash_and_equivalents: row
                    .get("cashAndCashEquivalents")
                    .and_then(Value::as_f64),
                short_term_investments: row
                    .get("shortTermInvestments")
                    .and_then(Value::as_f64),
                net_receivables: row.get("netReceivables").and_then(Value::as_f64),
                inventory: row.get("inventory").and_then(Value::as_f64),
                total_assets: row.get("totalAssets").and_then(Value::as_f64),
            })
        })
        .collect()
}

/// `None` only when the body is not a non-empty array; a missing or non-numeric
/// score stays `None`.
fn financial_scores_from_value(value: &Value) -> Option<crate::portfolio::engine::FinancialScores> {
    let first = value.as_array()?.first()?;
    Some(crate::portfolio::engine::FinancialScores {
        altman_z: first.get("altmanZScore").and_then(Value::as_f64),
        piotroski: first.get("piotroskiScore").and_then(Value::as_f64),
    })
}

/// Newest first; the same date rules as [`quarterly_balance_sheet_from_value`].
fn quarterly_cash_flow_from_value(
    value: &Value,
) -> Vec<crate::portfolio::engine::QuarterlyCashFlowRow> {
    let Some(rows) = value.as_array() else {
        return vec![];
    };
    rows.iter()
        .filter_map(|row| {
            let period_end = row
                .get("date")
                .and_then(Value::as_str)
                .and_then(canonical_date)?;
            Some(crate::portfolio::engine::QuarterlyCashFlowRow {
                period_end,
                // Datable-string-first per key: an undatable `filingDate` falls through
                // to the legacy `fillingDate` spelling, and to `None` (the period-end +
                // grace anchor) when neither parses.
                filing_date: row
                    .get("filingDate")
                    .and_then(Value::as_str)
                    .and_then(canonical_date)
                    .or_else(|| {
                        row.get("fillingDate")
                            .and_then(Value::as_str)
                            .and_then(canonical_date)
                    }),
                free_cash_flow: row.get("freeCashFlow").and_then(Value::as_f64),
                // Numeric-first per key: a present-but-null preferred line must still
                // fall through to the alternate spelling.
                operating_cash_flow: row
                    .get("operatingCashFlow")
                    .and_then(Value::as_f64)
                    .or_else(|| {
                        row.get("netCashProvidedByOperatingActivities")
                            .and_then(Value::as_f64)
                    }),
                capex: row.get("capitalExpenditure").and_then(Value::as_f64),
                depreciation_and_amortization: row
                    .get("depreciationAndAmortization")
                    .and_then(Value::as_f64),
            })
        })
        .collect()
}

/// Newest first. A row without a datable period date is skipped, and dates are stored
/// as their canonical render ([`canonical_date`]): the sort, the restatement
/// tie-break and the anchor date against the ISO closes are all lexicographic, and
/// the feed family serves non-zero-padded dates on some rows.
fn quarterly_income_from_value(value: &Value) -> Vec<crate::portfolio::engine::QuarterlyIncomeRow> {
    let Some(rows) = value.as_array() else {
        return vec![];
    };
    rows.iter()
        .filter_map(|row| {
            let period_end = row
                .get("date")
                .and_then(Value::as_str)
                .and_then(canonical_date)?;
            // Datable-string-first per key: a null or undatable `filingDate` must still
            // fall through to the legacy `fillingDate` spelling, or a restated row
            // loses its tie-break date.
            let filing_date = row
                .get("filingDate")
                .and_then(Value::as_str)
                .and_then(canonical_date)
                .or_else(|| {
                    row.get("fillingDate")
                        .and_then(Value::as_str)
                        .and_then(canonical_date)
                });
            Some(crate::portfolio::engine::QuarterlyIncomeRow {
                period_end,
                filing_date,
                revenue: row.get("revenue").and_then(Value::as_f64),
                eps_diluted: row
                    .get("epsDiluted")
                    .and_then(Value::as_f64)
                    .or_else(|| row.get("epsdiluted").and_then(Value::as_f64)),
                diluted_shares: row
                    .get("weightedAverageShsOutDil")
                    .and_then(Value::as_f64),
                net_income: row.get("netIncome").and_then(Value::as_f64),
                gross_profit: row.get("grossProfit").and_then(Value::as_f64),
                cost_of_revenue: row.get("costOfRevenue").and_then(Value::as_f64),
                operating_income: row.get("operatingIncome").and_then(Value::as_f64),
                research_and_development: row
                    .get("researchAndDevelopmentExpenses")
                    .and_then(Value::as_f64),
                income_before_tax: row.get("incomeBeforeTax").and_then(Value::as_f64),
                income_tax_expense: row.get("incomeTaxExpense").and_then(Value::as_f64),
            })
        })
        .collect()
}

/// The quarterly shaper's date rules, keeping the two lines the mid-cycle margin
/// reads; order and duplicates are left to that read's canonicalization.
fn annual_income_from_value(value: &Value) -> Vec<crate::portfolio::engine::AnnualIncomeRow> {
    let Some(rows) = value.as_array() else {
        return vec![];
    };
    rows.iter()
        .filter_map(|row| {
            let period_end = row
                .get("date")
                .and_then(Value::as_str)
                .and_then(canonical_date)?;
            let filing_date = row
                .get("filingDate")
                .and_then(Value::as_str)
                .and_then(canonical_date)
                .or_else(|| {
                    row.get("fillingDate")
                        .and_then(Value::as_str)
                        .and_then(canonical_date)
                });
            Some(crate::portfolio::engine::AnnualIncomeRow {
                period_end,
                filing_date,
                revenue: row.get("revenue").and_then(Value::as_f64),
                net_income: row.get("netIncome").and_then(Value::as_f64),
            })
        })
        .collect()
}

/// The next-twelve-months consensus: the two nearest coming fiscal-year rows blended
/// by their overlap with the rolling twelve-month window, so a mostly-reported
/// current fiscal year contributes only its remaining months instead of masquerading
/// as the forward year (`docs/portfolio-analysis.md` §Starting parameters). No
/// forward-dated row is `None`: a past fiscal-year estimate masquerading as forward
/// would bypass the driver ladder's `no-admissible-driver` abstention.
fn consensus_from_value(
    value: &Value,
    today: &str,
) -> Option<crate::portfolio::engine::ConsensusEstimate> {
    let rows = value.as_array()?;
    let date_of = |row: &Value| {
        row.get("date")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    // Numeric-first per key: a present-but-null stable key must still fall through
    // to the legacy spelling.
    let field = |row: &Value, stable: &str, legacy: &str| {
        row.get(stable)
            .and_then(Value::as_f64)
            .or_else(|| row.get(legacy).and_then(Value::as_f64))
    };
    // Filter / sort / dedup on the parsed date, never the source text: a raw-string
    // sort of a non-zero-padded date would reverse the near/far legs and invert the
    // blend. An undatable row never enters the forward set.
    let today_parsed = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    let mut forward: Vec<(chrono::NaiveDate, &Value)> = rows
        .iter()
        .filter_map(|r| {
            let d = chrono::NaiveDate::parse_from_str(&date_of(r), "%Y-%m-%d").ok()?;
            (d >= today_parsed).then_some((d, r))
        })
        .collect();
    forward.sort_by_key(|(d, _)| *d);
    // A duplicated period must not masquerade as near + far: the blend would re-read
    // the same year at both weights and exclude the true following fiscal year.
    forward.dedup_by_key(|(d, _)| *d);
    let (near_date, near): (chrono::NaiveDate, &Value) = *forward.first()?;
    let far_row: Option<(chrono::NaiveDate, &Value)> = forward.get(1).copied();
    let far: Option<&Value> = far_row.map(|(_, r)| r);

    // The near row's weight is the share of the rolling twelve months its fiscal year
    // still covers; the far row carries the rest. At the boundary the weight is 0 and
    // the value is entirely the far row's while `period_end` still names the near
    // row; `near_weight` on the record keys the provenance.
    let near_weight = match far {
        Some(_) => ((near_date - today_parsed).num_days().clamp(0, 365) as f64) / 365.0,
        None => 1.0,
    };
    let blended = near_weight < 1.0;
    let blend = |stable: &str, legacy: &str| -> Option<f64> {
        let n = field(near, stable, legacy);
        // An inactive blend reads the near row alone: a far fiscal year must never
        // leak in at full weight through a missing near leg.
        if !blended {
            return n;
        }
        let f = far.and_then(|r| field(r, stable, legacy));
        match (n, f) {
            (Some(n), Some(f)) => Some(near_weight * n + (1.0 - near_weight) * f),
            // Inside an active blend, a leg only one row carries is used alone
            // rather than dropped.
            (Some(n), None) => Some(n),
            (None, f) => f,
        }
    };
    // Per-field corroboration counts the rows that actually contribute, mirroring
    // `blend`'s arms rather than mere presence: at the fiscal-year boundary the near
    // weight is exactly 0, so a present-but-weightless near row must not count as a
    // second estimate.
    let rows_carrying = |stable: &str, legacy: &str| -> u8 {
        let n = field(near, stable, legacy).is_some();
        if !blended {
            return n as u8;
        }
        let f = far.and_then(|r| field(r, stable, legacy)).is_some();
        match (n, f) {
            (true, true) => 1 + (near_weight > 0.0) as u8,
            (true, false) | (false, true) => 1,
            (false, false) => 0,
        }
    };
    let eps_period = |date: chrono::NaiveDate,
                      row: &Value,
                      ntm_weight: f64|
     -> crate::portfolio::engine::ConsensusEpsPeriod {
        crate::portfolio::engine::ConsensusEpsPeriod {
            period_end: date.format("%Y-%m-%d").to_string(),
            eps_mid: field(row, "epsAvg", "estimatedEpsAvg")
                .filter(|value| value.is_finite()),
            ntm_weight,
        }
    };
    let mut eps_periods = vec![eps_period(
        near_date,
        near,
        if blended { near_weight } else { 1.0 },
    )];
    if let Some((far_date, far)) = far_row.filter(|_| blended) {
        eps_periods.push(eps_period(far_date, far, 1.0 - near_weight));
    }
    // The growth is the rows' own raw mids, not the NTM blend's
    // (`docs/portfolio-analysis.md` §Starting parameters).
    let growth_rows = |stable: &str, legacy: &str| crate::portfolio::engine::ConsensusGrowthRows {
        near: field(near, stable, legacy).filter(|v| v.is_finite()),
        far: far.and_then(|r| field(r, stable, legacy)).filter(|v| v.is_finite()),
    };

    Some(crate::portfolio::engine::ConsensusEstimate {
        period_end: date_of(near),
        eps_low: blend("epsLow", "estimatedEpsLow"),
        eps_mid: blend("epsAvg", "estimatedEpsAvg"),
        eps_high: blend("epsHigh", "estimatedEpsHigh"),
        revenue_low: blend("revenueLow", "estimatedRevenueLow"),
        revenue_mid: blend("revenueAvg", "estimatedRevenueAvg"),
        revenue_high: blend("revenueHigh", "estimatedRevenueHigh"),
        periods_used: if blended { 2 } else { 1 },
        near_weight,
        eps_mid_rows: rows_carrying("epsAvg", "estimatedEpsAvg"),
        revenue_mid_rows: rows_carrying("revenueAvg", "estimatedRevenueAvg"),
        eps_periods,
        eps_growth_rows: growth_rows("epsAvg", "estimatedEpsAvg"),
        revenue_growth_rows: growth_rows("revenueAvg", "estimatedRevenueAvg"),
    })
}

/// The trailing-twelve-month per-share sum and the latest payments as reported,
/// newest first, at most [`DIVIDENDS_SHOWN`]. `ttm` is bounded on both sides: a
/// future-dated declaration is not paid and would inflate the trailing-return leg.
/// `None` when no row lands in the window, so the leg adds nothing rather than a
/// fabricated yield.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DividendPull {
    pub ttm: Option<f64>,
    pub recent: Vec<DividendRow>,
}

/// An older payment whose amount is unreadable is left out of `recent`; only an
/// unreadable row inside the trailing window bails.
fn dividends_from_value(value: &Value, today: chrono::NaiveDate) -> Result<DividendPull> {
    let Some(rows) = value.as_array() else {
        anyhow::bail!("non-array body — malformed or drifted response");
    };
    let mut recent: Vec<DividendRow> = Vec::new();
    // Exclusive lower bound: inclusive on both ends the window would span 366 days,
    // and a payment dated exactly `today − 365` would ride beside today's as a fifth
    // quarterly print.
    let cutoff = today - Duration::days(365);
    let mut sum = 0.0;
    let mut any = false;
    for row in rows {
        // An unreadable row is `Err`, never a silent skip: `Ok(None)` must mean the
        // body affirmatively shows no trailing dividends, or a string-typed amount
        // would masquerade as a confirmed dividend elimination downstream.
        let Some(date) = row.get("date").and_then(Value::as_str) else {
            anyhow::bail!("a dividend row carried no date — malformed or drifted response");
        };
        let Ok(parsed) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
            anyhow::bail!(
                "a dividend row carried a non-ISO date {date:?} — malformed or drifted response"
            );
        };
        // Numeric-first per key: a present-but-null `adjDividend` beside a numeric
        // `dividend` must read the amount, not take the unreadable-row bail path.
        let amount = row
            .get("adjDividend")
            .and_then(Value::as_f64)
            .or_else(|| row.get("dividend").and_then(Value::as_f64));
        // Every row on or before today with a readable amount, whatever its age (an
        // annual payer's last four span years).
        if parsed <= today {
            if let Some(a) = amount.filter(|a| a.is_finite()) {
                recent.push(DividendRow {
                    date: parsed.format("%Y-%m-%d").to_string(),
                    amount: a,
                    payment_date: served_date(row, "paymentDate"),
                });
            }
        }
        // Window on the parsed date, never the source text: a non-zero-padded date
        // compares lexicographically outside the window.
        if parsed <= cutoff || parsed > today {
            continue;
        }
        let Some(a) = amount else {
            anyhow::bail!(
                "an in-window dividend row carried no numeric amount — malformed or drifted response"
            );
        };
        sum += a;
        any = true;
    }
    // Every amount is finite, but their sum need not be; an overflowed sum is a
    // drifted body, `Err` so the leg reads zero with its recorded gap, never a
    // figure and never a silent non-payer.
    if any && !sum.is_finite() {
        anyhow::bail!(
            "the in-window dividend amounts overflowed — malformed or drifted response"
        );
    }
    recent.sort_by(|a, b| b.date.cmp(&a.date));
    recent.truncate(DIVIDENDS_SHOWN);
    Ok(DividendPull {
        ttm: any.then_some(sum),
        recent,
    })
}

// ---- The per-holding evidence surface (`crate::portfolio::evidence`) ------

/// A served date-ish string's calendar-date prefix, canonical (`2026-06-05
/// 08:13:10` → `2026-06-05`); `None` where no datable token leads it.
fn canonical_date_prefix(s: &str) -> Option<String> {
    let token = s.trim().split([' ', 'T']).next()?;
    canonical_date(token)
}

/// A non-blank served string field, trimmed.
fn served_str(row: &Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A finite served number.
fn served_f64(row: &Value, key: &str) -> Option<f64> {
    row.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
}

/// A served non-negative whole number (integers and whole floats alike).
fn served_u64(row: &Value, key: &str) -> Option<u64> {
    served_f64(row, key)
        .filter(|v| *v >= 0.0 && v.fract() == 0.0)
        .map(|v| v as u64)
}

/// A served whole number.
fn served_i64(row: &Value, key: &str) -> Option<i64> {
    served_f64(row, key)
        .filter(|v| v.fract() == 0.0)
        .map(|v| v as i64)
}

/// A served year, whether the feed stamps it as a string or a number.
fn served_year(row: &Value, key: &str) -> Option<String> {
    served_str(row, key).or_else(|| served_i64(row, key).map(|y| y.to_string()))
}

fn served_date(row: &Value, key: &str) -> Option<String> {
    served_str(row, key).and_then(|s| canonical_date_prefix(&s))
}

/// The rows of an array body — `Err` on any other shape (drift, never an
/// empty success).
fn evidence_rows<'a>(value: &'a Value, what: &str) -> Result<&'a [Value]> {
    value.as_array().map(Vec::as_slice).ok_or_else(|| {
        anyhow::anyhow!("FMP {what} returned a non-array body — malformed or drifted response")
    })
}

/// The readable rows of an array body under `row`; a non-empty body with no
/// readable row is drift, never an honest empty.
fn readable_rows<T>(
    value: &Value,
    what: &str,
    row: impl Fn(&Value) -> Option<T>,
) -> Result<Vec<T>> {
    let rows = evidence_rows(value, what)?;
    let out: Vec<T> = rows.iter().filter_map(row).collect();
    if out.is_empty() && !rows.is_empty() {
        anyhow::bail!("FMP {what}: no served row was readable — malformed or drifted response");
    }
    Ok(out)
}

/// The one object of an array-of-one body (FMP's single-row shape) or a bare
/// object; an empty array is the honest empty (`Ok(None)`), anything else
/// drift.
fn evidence_object<'a>(value: &'a Value, what: &str) -> Result<Option<&'a Value>> {
    if let Some(rows) = value.as_array() {
        return match rows.first() {
            None => Ok(None),
            Some(row) if row.is_object() => Ok(Some(row)),
            Some(_) => anyhow::bail!(
                "FMP {what}: the served row was not an object — malformed or drifted response"
            ),
        };
    }
    if value.is_object() {
        return Ok(Some(value));
    }
    anyhow::bail!("FMP {what} returned a non-array body — malformed or drifted response")
}

/// `ratios-ttm` → the P/E and P/B lines.
fn ttm_ratios_from_value(value: &Value) -> Result<Option<RatioLines>> {
    Ok(evidence_object(value, "ratios-ttm")?.map(|o| RatioLines {
        pe: served_f64(o, "priceToEarningsRatioTTM"),
        pb: served_f64(o, "priceToBookRatioTTM"),
        ..Default::default()
    }))
}

/// `key-metrics-ttm` → the EV/EBITDA, EV/sales, FCF yield, ROIC, ROE and net
/// debt/EBITDA lines.
fn ttm_key_metrics_from_value(value: &Value) -> Result<Option<RatioLines>> {
    Ok(
        evidence_object(value, "key-metrics-ttm")?.map(|o| RatioLines {
            ev_to_ebitda: served_f64(o, "evToEBITDATTM"),
            ev_to_sales: served_f64(o, "evToSalesTTM"),
            fcf_yield: served_f64(o, "freeCashFlowYieldTTM"),
            roic: served_f64(o, "returnOnInvestedCapitalTTM"),
            roe: served_f64(o, "returnOnEquityTTM"),
            net_debt_to_ebitda: served_f64(o, "netDebtToEBITDATTM"),
            ..Default::default()
        }),
    )
}

/// The eight lines off the two pulls — each half from its own feed.
fn merge_ratio_lines(ratios: Option<RatioLines>, metrics: Option<RatioLines>) -> RatioLines {
    let r = ratios.unwrap_or_default();
    let m = metrics.unwrap_or_default();
    RatioLines {
        pe: r.pe,
        pb: r.pb,
        ev_to_ebitda: m.ev_to_ebitda,
        ev_to_sales: m.ev_to_sales,
        fcf_yield: m.fcf_yield,
        roic: m.roic,
        roe: m.roe,
        net_debt_to_ebitda: m.net_debt_to_ebitda,
    }
}

/// `owner-earnings` → the latest row by period end.
fn owner_earnings_from_value(value: &Value) -> Result<Option<OwnerEarningsRow>> {
    let rows = readable_rows(value, "owner-earnings", |row| {
        let period_end = served_date(row, "date")?;
        let period = match (served_year(row, "fiscalYear"), served_str(row, "period")) {
            (Some(year), Some(period)) => Some(format!("FY{year} {period}")),
            _ => None,
        };
        Some(OwnerEarningsRow {
            period_end,
            period,
            owners_earnings: served_f64(row, "ownersEarnings"),
            per_share: served_f64(row, "ownersEarningsPerShare"),
        })
    })?;
    Ok(rows
        .into_iter()
        .max_by(|a, b| a.period_end.cmp(&b.period_end)))
}

/// `enterprise-values` → the latest row by date.
fn enterprise_value_from_value(value: &Value) -> Result<Option<EnterpriseValueRow>> {
    let rows = readable_rows(value, "enterprise-values", |row| {
        Some(EnterpriseValueRow {
            date: served_date(row, "date")?,
            enterprise_value: served_f64(row, "enterpriseValue"),
            market_cap: served_f64(row, "marketCapitalization"),
            total_debt: served_f64(row, "addTotalDebt"),
            cash: served_f64(row, "minusCashAndCashEquivalents"),
        })
    })?;
    Ok(rows.into_iter().max_by(|a, b| a.date.cmp(&b.date)))
}

fn price_target_consensus_from_value(value: &Value) -> Result<Option<PriceTargetConsensus>> {
    Ok(
        evidence_object(value, "price-target-consensus")?.map(|o| PriceTargetConsensus {
            high: served_f64(o, "targetHigh"),
            low: served_f64(o, "targetLow"),
            median: served_f64(o, "targetMedian"),
            consensus: served_f64(o, "targetConsensus"),
        }),
    )
}

/// `price-target-summary` → the month, quarter and year windows.
fn price_target_trend_from_value(value: &Value) -> Result<Option<PriceTargetTrend>> {
    let window = |o: &Value, count: &str, average: &str| PriceTargetWindow {
        count: served_u64(o, count),
        average: served_f64(o, average),
    };
    Ok(
        evidence_object(value, "price-target-summary")?.map(|o| PriceTargetTrend {
            last_month: window(o, "lastMonthCount", "lastMonthAvgPriceTarget"),
            last_quarter: window(o, "lastQuarterCount", "lastQuarterAvgPriceTarget"),
            last_year: window(o, "lastYearCount", "lastYearAvgPriceTarget"),
        }),
    )
}

fn grades_consensus_from_value(value: &Value) -> Result<Option<GradesConsensus>> {
    Ok(
        evidence_object(value, "grades-consensus")?.map(|o| GradesConsensus {
            strong_buy: served_u64(o, "strongBuy"),
            buy: served_u64(o, "buy"),
            hold: served_u64(o, "hold"),
            sell: served_u64(o, "sell"),
            strong_sell: served_u64(o, "strongSell"),
            consensus: served_str(o, "consensus"),
        }),
    )
}

/// `grades` → the rating actions dated on or after `since`, newest first, at
/// most [`RATING_ACTIONS_COUNT`]. A row needs a date and a firm to be read.
fn rating_actions_from_value(value: &Value, since: &str) -> Result<Vec<RatingAction>> {
    let mut rows = readable_rows(value, "grades", |row| {
        Some(RatingAction {
            date: served_date(row, "date")?,
            firm: served_str(row, "gradingCompany")?,
            previous_grade: served_str(row, "previousGrade"),
            new_grade: served_str(row, "newGrade"),
            action: served_str(row, "action"),
        })
    })?;
    rows.retain(|r| r.date.as_str() >= since);
    rows.sort_by(|a, b| b.date.cmp(&a.date));
    rows.truncate(RATING_ACTIONS_COUNT);
    Ok(rows)
}

fn ratings_snapshot_from_value(value: &Value) -> Result<Option<RatingsSnapshot>> {
    Ok(
        evidence_object(value, "ratings-snapshot")?.map(|o| RatingsSnapshot {
            rating: served_str(o, "rating"),
            overall: served_i64(o, "overallScore"),
            discounted_cash_flow: served_i64(o, "discountedCashFlowScore"),
            return_on_equity: served_i64(o, "returnOnEquityScore"),
            return_on_assets: served_i64(o, "returnOnAssetsScore"),
            debt_to_equity: served_i64(o, "debtToEquityScore"),
            price_to_earnings: served_i64(o, "priceToEarningsScore"),
            price_to_book: served_i64(o, "priceToBookScore"),
        }),
    )
}

/// `insider-trading/search` → the trades transacted on or after `since`,
/// newest first, at most [`INSIDER_TRADES_COUNT`]. A row needs a transaction
/// date and a reporting name to be read.
fn insider_trades_from_value(value: &Value, since: &str) -> Result<Vec<InsiderTrade>> {
    let mut rows = readable_rows(value, "insider-trading/search", |row| {
        Some(InsiderTrade {
            transaction_date: served_date(row, "transactionDate")?,
            filing_date: served_date(row, "filingDate"),
            name: served_str(row, "reportingName")?,
            owner_type: served_str(row, "typeOfOwner"),
            transaction_type: served_str(row, "transactionType"),
            shares: served_f64(row, "securitiesTransacted"),
            price: served_f64(row, "price"),
        })
    })?;
    rows.retain(|r| r.transaction_date.as_str() >= since);
    rows.sort_by(|a, b| b.transaction_date.cmp(&a.transaction_date));
    rows.truncate(INSIDER_TRADES_COUNT);
    Ok(rows)
}

/// `insider-trading/statistics` → the latest quarter's row.
fn insider_statistics_from_value(value: &Value) -> Result<Option<InsiderStatistics>> {
    let rows = readable_rows(value, "insider-trading/statistics", |row| {
        let year = served_i64(row, "year")?;
        Some(InsiderStatistics {
            year: Some(year),
            quarter: served_i64(row, "quarter"),
            acquired_transactions: served_u64(row, "acquiredTransactions"),
            disposed_transactions: served_u64(row, "disposedTransactions"),
            total_acquired: served_f64(row, "totalAcquired"),
            total_disposed: served_f64(row, "totalDisposed"),
        })
    })?;
    Ok(rows.into_iter().max_by_key(|r| (r.year, r.quarter)))
}

/// `senate-trades` / `house-trades` → every readable row of one chamber (the
/// merge, the window and the cap are [`merge_congressional_trades`]'s). A row
/// needs a transaction date and a name — the first and last names, else the
/// office — to be read.
fn congressional_trades_from_value(
    value: &Value,
    chamber: Chamber,
) -> Result<Vec<CongressionalTrade>> {
    let what = match chamber {
        Chamber::Senate => "senate-trades",
        Chamber::House => "house-trades",
    };
    readable_rows(value, what, |row| {
        let name = match (served_str(row, "firstName"), served_str(row, "lastName")) {
            (Some(first), Some(last)) => format!("{first} {last}"),
            (Some(one), None) | (None, Some(one)) => one,
            (None, None) => served_str(row, "office")?,
        };
        Some(CongressionalTrade {
            chamber,
            transaction_date: served_date(row, "transactionDate")?,
            disclosure_date: served_date(row, "disclosureDate"),
            name,
            owner: served_str(row, "owner"),
            kind: served_str(row, "type"),
            amount: served_str(row, "amount"),
        })
    })
}

/// Both chambers merged: transacted on or after `since`, newest first (the
/// Senate ahead of the House on one date), at most
/// [`CONGRESSIONAL_TRADES_COUNT`].
fn merge_congressional_trades(
    senate: Vec<CongressionalTrade>,
    house: Vec<CongressionalTrade>,
    since: &str,
) -> Vec<CongressionalTrade> {
    let mut rows: Vec<CongressionalTrade> = senate
        .into_iter()
        .chain(house)
        .filter(|r| r.transaction_date.as_str() >= since)
        .collect();
    rows.sort_by(|a, b| {
        b.transaction_date
            .cmp(&a.transaction_date)
            .then_with(|| a.chamber.label().cmp(b.chamber.label()).reverse())
    });
    rows.truncate(CONGRESSIONAL_TRADES_COUNT);
    rows
}

fn shares_float_from_value(value: &Value) -> Result<Option<SharesFloat>> {
    Ok(
        evidence_object(value, "shares-float")?.map(|o| SharesFloat {
            date: served_str(o, "date"),
            free_float_percent: served_f64(o, "freeFloat"),
            float_shares: served_f64(o, "floatShares"),
            outstanding_shares: served_f64(o, "outstandingShares"),
        }),
    )
}

/// A revenue segmentation feed (`period=annual`) → the latest
/// [`SEGMENT_FISCAL_YEARS`] fiscal years, newest first, each year's segments
/// largest first. A row needs a date and a `data` object to be read.
fn segment_years_from_value(value: &Value, what: &str) -> Result<Vec<SegmentYear>> {
    let mut rows = readable_rows(value, what, |row| {
        let period_end = served_date(row, "date")?;
        let data = row.get("data")?.as_object()?;
        let mut segments: Vec<(String, f64)> = data
            .iter()
            .filter_map(|(name, v)| {
                v.as_f64()
                    .filter(|x| x.is_finite())
                    .map(|x| (name.clone(), x))
            })
            .collect();
        segments.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Some(SegmentYear {
            fiscal_year: served_i64(row, "fiscalYear"),
            period_end,
            segments,
        })
    })?;
    rows.sort_by(|a, b| b.period_end.cmp(&a.period_end));
    rows.truncate(SEGMENT_FISCAL_YEARS);
    Ok(rows)
}

/// `splits` → the rows newest first. A row needs a date and a positive
/// numerator and denominator to be read.
fn splits_from_value(value: &Value) -> Result<Vec<SplitRow>> {
    let mut rows = readable_rows(value, "splits", |row| {
        Some(SplitRow {
            date: served_date(row, "date")?,
            numerator: served_f64(row, "numerator").filter(|n| *n > 0.0)?,
            denominator: served_f64(row, "denominator").filter(|d| *d > 0.0)?,
        })
    })?;
    rows.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(rows)
}

/// One page of `mergers-acquisitions-latest` → its deals. A row needs a
/// transaction date to be read.
fn ma_deals_from_value(value: &Value) -> Result<Vec<MaDeal>> {
    readable_rows(value, "mergers-acquisitions-latest", |row| {
        Some(MaDeal {
            acquirer_symbol: served_str(row, "symbol"),
            acquirer_name: served_str(row, "companyName"),
            target_symbol: served_str(row, "targetedSymbol"),
            target_name: served_str(row, "targetedCompanyName"),
            transaction_date: served_date(row, "transactionDate")?,
            link: served_str(row, "link"),
        })
    })
}

/// `ipos-calendar` → its readable rows and the count of served offerings no reader could
/// date. A row needs a listing date to be read; a served row with a share count but no
/// readable date is an offering the counts cannot place. A share count or a deal size that
/// is absent, non-finite or not positive reads as `None`. A non-empty body with no readable
/// row is drift, never an honest empty.
fn ipo_rows_from_value(value: &Value) -> Result<(Vec<IpoRow>, usize)> {
    let rows = evidence_rows(value, "ipos-calendar")?;
    let mut out = Vec::new();
    let mut undated_offerings = 0;
    for row in rows {
        let shares = served_f64(row, "shares").filter(|s| *s > 0.0);
        let Some(date) = served_date(row, "date") else {
            undated_offerings += usize::from(shares.is_some());
            continue;
        };
        out.push(IpoRow {
            symbol: served_str(row, "symbol"),
            company: served_str(row, "company"),
            date,
            exchange: served_str(row, "exchange"),
            status: IpoStatus::from_served(served_str(row, "actions").as_deref()),
            shares,
            deal_size: served_f64(row, "dealSize").filter(|s| *s > 0.0),
        });
    }
    if out.is_empty() && !rows.is_empty() {
        anyhow::bail!("FMP ipos-calendar: no served row was readable — malformed or drifted response");
    }
    Ok((out, undated_offerings))
}

struct EvidenceLeg {
    kind: &'static str,
    label: &'static str,
    path: &'static str,
    name: &'static str,
}

impl FmpDataSource {
    /// An honest empty is `empty` with no gap; a malformed body and an HTTP-level gap
    /// each tag `gaps` with the leg's name. A cancel already requested skips the
    /// request (no row, a skipped gap).
    fn evidence_leg<T>(
        &self,
        leg: EvidenceLeg,
        symbol: &str,
        extra: &[(&str, &str)],
        gaps: &mut Vec<String>,
        parse: impl FnOnce(&Value) -> Result<Option<T>>,
    ) -> Option<T> {
        let EvidenceLeg {
            kind,
            label,
            path,
            name,
        } = leg;
        if self.progress.is_cancelled() {
            gaps.push(format!("FMP {name} skipped (run cancelled)"));
            return None;
        }
        match self.suite_get_shaped(kind, symbol, label, path, extra, |value| {
            match parse(value) {
                Ok(Some(v)) => Shaped::ok(Some(v)),
                Ok(None) => Shaped::empty(None),
                Err(e) => {
                    let detail = format!("{e:#}");
                    gaps.push(format!("FMP {name} malformed ({e})"));
                    Shaped::malformed(None).with_detail(detail)
                }
            }
        }) {
            Ok(v) => v,
            Err(reason) => {
                gaps.push(format!("FMP {name} unavailable ({})", reason.as_str()));
                None
            }
        }
    }

    /// The per-holding evidence surface (`crate::portfolio::evidence`), one tracker
    /// row per leg, every leg fail-soft to a tagged gap on the record. `session` is
    /// the run's ET session date; the windows count back from it.
    pub fn fetch_company_evidence(&self, symbol: &str, session: NaiveDate) -> CompanyEvidence {
        use crate::portfolio::evidence::{
            window_start, CONGRESSIONAL_TRADES_WINDOW_MONTHS, INSIDER_TRADES_WINDOW_MONTHS,
            RATING_ACTIONS_WINDOW_MONTHS,
        };
        let mut e = CompanyEvidence::empty(symbol);
        if self.progress.is_cancelled() {
            e.gaps
                .push("company evidence skipped (run cancelled)".to_string());
            return e;
        }
        let by_symbol = [("symbol", symbol)];
        let listed = [("symbol", symbol), ("limit", EVIDENCE_LIST_LIMIT)];
        let latest = [("symbol", symbol), ("limit", EVIDENCE_LATEST_ROW_LIMIT)];
        let annual = [("symbol", symbol), ("period", "annual")];
        let ratios = self.evidence_leg(
            EvidenceLeg {
                kind: "company-ratios-ttm",
                label: "TTM ratios",
                path: FMP_RATIOS_TTM_PATH,
                name: "ratios-ttm",
            },
            symbol,
            &by_symbol,
            &mut e.gaps,
            ttm_ratios_from_value,
        );
        let metrics = self.evidence_leg(
            EvidenceLeg {
                kind: "company-key-metrics-ttm",
                label: "TTM key metrics",
                path: FMP_KEY_METRICS_TTM_PATH,
                name: "key-metrics-ttm",
            },
            symbol,
            &by_symbol,
            &mut e.gaps,
            ttm_key_metrics_from_value,
        );
        e.ratios = merge_ratio_lines(ratios, metrics);
        e.owner_earnings = self.evidence_leg(
            EvidenceLeg {
                kind: "company-owner-earnings",
                label: "Owner earnings",
                path: FMP_OWNER_EARNINGS_PATH,
                name: "owner-earnings",
            },
            symbol,
            &latest,
            &mut e.gaps,
            owner_earnings_from_value,
        );
        e.enterprise_value = self.evidence_leg(
            EvidenceLeg {
                kind: "company-enterprise-value",
                label: "Enterprise value",
                path: FMP_ENTERPRISE_VALUES_PATH,
                name: "enterprise-values",
            },
            symbol,
            &latest,
            &mut e.gaps,
            enterprise_value_from_value,
        );
        e.price_target = self.evidence_leg(
            EvidenceLeg {
                kind: "company-price-target",
                label: "Price-target consensus",
                path: FMP_PRICE_TARGET_CONSENSUS_PATH,
                name: "price-target-consensus",
            },
            symbol,
            &by_symbol,
            &mut e.gaps,
            price_target_consensus_from_value,
        );
        e.price_target_trend = self.evidence_leg(
            EvidenceLeg {
                kind: "company-price-target-trend",
                label: "Price-target summary",
                path: FMP_PRICE_TARGET_SUMMARY_PATH,
                name: "price-target-summary",
            },
            symbol,
            &by_symbol,
            &mut e.gaps,
            price_target_trend_from_value,
        );
        let rating_since = window_start(session, RATING_ACTIONS_WINDOW_MONTHS);
        e.rating_actions = self
            .evidence_leg(
                EvidenceLeg {
                    kind: "company-grades",
                    label: "Rating actions",
                    path: FMP_GRADES_PATH,
                    name: "grades",
                },
                symbol,
                &listed,
                &mut e.gaps,
                |v| {
                    rating_actions_from_value(v, &rating_since)
                        .map(|rows| (!rows.is_empty()).then_some(rows))
                },
            )
            .unwrap_or_default();
        e.grades_consensus = self.evidence_leg(
            EvidenceLeg {
                kind: "company-grades-consensus",
                label: "Grades consensus",
                path: FMP_GRADES_CONSENSUS_PATH,
                name: "grades-consensus",
            },
            symbol,
            &by_symbol,
            &mut e.gaps,
            grades_consensus_from_value,
        );
        e.ratings_snapshot = self.evidence_leg(
            EvidenceLeg {
                kind: "company-ratings-snapshot",
                label: "Ratings snapshot",
                path: FMP_RATINGS_SNAPSHOT_PATH,
                name: "ratings-snapshot",
            },
            symbol,
            &by_symbol,
            &mut e.gaps,
            ratings_snapshot_from_value,
        );
        let insider_since = window_start(session, INSIDER_TRADES_WINDOW_MONTHS);
        e.insider_trades = self
            .evidence_leg(
                EvidenceLeg {
                    kind: "company-insider-trades",
                    label: "Insider trades",
                    path: FMP_INSIDER_SEARCH_PATH,
                    name: "insider-trading/search",
                },
                symbol,
                &listed,
                &mut e.gaps,
                |v| {
                    insider_trades_from_value(v, &insider_since)
                        .map(|rows| (!rows.is_empty()).then_some(rows))
                },
            )
            .unwrap_or_default();
        e.insider_statistics = self.evidence_leg(
            EvidenceLeg {
                kind: "company-insider-statistics",
                label: "Insider statistics",
                path: FMP_INSIDER_STATISTICS_PATH,
                name: "insider-trading/statistics",
            },
            symbol,
            &by_symbol,
            &mut e.gaps,
            insider_statistics_from_value,
        );
        let senate = self
            .evidence_leg(
                EvidenceLeg {
                    kind: "company-senate-trades",
                    label: "Senate trades",
                    path: FMP_SENATE_TRADES_PATH,
                    name: "senate-trades",
                },
                symbol,
                &listed,
                &mut e.gaps,
                |v| {
                    congressional_trades_from_value(v, Chamber::Senate)
                        .map(|rows| (!rows.is_empty()).then_some(rows))
                },
            )
            .unwrap_or_default();
        let house = self
            .evidence_leg(
                EvidenceLeg {
                    kind: "company-house-trades",
                    label: "House trades",
                    path: FMP_HOUSE_TRADES_PATH,
                    name: "house-trades",
                },
                symbol,
                &listed,
                &mut e.gaps,
                |v| {
                    congressional_trades_from_value(v, Chamber::House)
                        .map(|rows| (!rows.is_empty()).then_some(rows))
                },
            )
            .unwrap_or_default();
        e.congressional_trades = merge_congressional_trades(
            senate,
            house,
            &window_start(session, CONGRESSIONAL_TRADES_WINDOW_MONTHS),
        );
        e.float = self.evidence_leg(
            EvidenceLeg {
                kind: "company-float",
                label: "Shares float",
                path: FMP_SHARES_FLOAT_PATH,
                name: "shares-float",
            },
            symbol,
            &by_symbol,
            &mut e.gaps,
            shares_float_from_value,
        );
        e.product_segments = self
            .evidence_leg(
                EvidenceLeg {
                    kind: "company-product-segments",
                    label: "Revenue by product",
                    path: FMP_PRODUCT_SEGMENTATION_PATH,
                    name: "revenue-product-segmentation",
                },
                symbol,
                &annual,
                &mut e.gaps,
                |v| {
                    segment_years_from_value(v, "revenue-product-segmentation")
                        .map(|rows| (!rows.is_empty()).then_some(rows))
                },
            )
            .unwrap_or_default();
        e.geographic_segments = self
            .evidence_leg(
                EvidenceLeg {
                    kind: "company-geographic-segments",
                    label: "Revenue by geography",
                    path: FMP_GEOGRAPHIC_SEGMENTATION_PATH,
                    name: "revenue-geographic-segmentation",
                },
                symbol,
                &annual,
                &mut e.gaps,
                |v| {
                    segment_years_from_value(v, "revenue-geographic-segmentation")
                        .map(|rows| (!rows.is_empty()).then_some(rows))
                },
            )
            .unwrap_or_default();
        e.splits = self
            .evidence_leg(
                EvidenceLeg {
                    kind: "company-splits",
                    label: "Splits",
                    path: FMP_SPLITS_PATH,
                    name: "splits",
                },
                symbol,
                &by_symbol,
                &mut e.gaps,
                |v| splits_from_value(v).map(|rows| (!rows.is_empty()).then_some(rows)),
            )
            .unwrap_or_default();
        if self.progress.is_cancelled() {
            e.gaps
                .push("FMP earnings skipped (run cancelled)".to_string());
            return e;
        }
        match self.fetch_symbol_earnings_as("earnings-surprises", symbol, EVIDENCE_EARNINGS_LIMIT) {
            Ok(rows) => e.earnings = rows,
            Err(err) => e.gaps.push(format!("{err}")),
        }
        e
    }

    /// The deals inside the trailing window and the walk's gap: a first-page failure
    /// leaves no feed; a later page's failure or the page cap leaves the rows gathered
    /// with the gap saying so. Wholly fail-soft.
    pub fn fetch_recent_mergers(&self, session: NaiveDate) -> (Vec<MaDeal>, Option<String>) {
        use crate::portfolio::evidence::{window_start, MA_WINDOW_MONTHS};
        let since = window_start(session, MA_WINDOW_MONTHS);
        let MaWalk { mut deals, end, .. } =
            self.walk_mergers(&since, "mergers-latest", "market", "M&A feed");
        deals.retain(|d| d.transaction_date.as_str() >= since.as_str());
        let gap = match end {
            MaWalkEnd::Boundary | MaWalkEnd::FeedEnd | MaWalkEnd::EmptyFeed => None,
            MaWalkEnd::Cap => Some(format!(
                "FMP M&A feed walk reached its {MA_PAGE_CAP}-page cap before the window's boundary"
            )),
            MaWalkEnd::Failed { page, detail, .. } => Some(ma_walk_gap(page, &detail)),
            MaWalkEnd::Cancelled => Some("FMP M&A feed walk skipped (run cancelled)".to_string()),
        };
        (deals, gap)
    }

    /// The one M&A walk Portfolio's run-level feed and the report's primary-market read
    /// share: newest first, stopping on a page holding a row dated before `since`
    /// (`YYYY-MM-DD`), on a page served short of [`MA_PAGE_LIMIT`] (measured on the
    /// rows served, not the rows read), on a failed page, or once [`MA_PAGE_CAP`] pages
    /// are spent. One tracker row per page.
    fn walk_mergers(&self, since: &str, kind: &str, symbol: &str, label: &str) -> MaWalk {
        use crate::portfolio::evidence::MA_PAGE_LIMIT;
        let mut deals: Vec<MaDeal> = Vec::new();
        let mut unreadable = 0;
        for page in 0..MA_PAGE_CAP {
            if self.progress.is_cancelled() {
                let end = MaWalkEnd::Cancelled;
                return MaWalk { deals, unreadable, end };
            }
            let page_param = page.to_string();
            let limit_param = MA_PAGE_LIMIT.to_string();
            let served = self.suite_get_shaped(
                kind,
                symbol,
                label,
                FMP_MERGERS_LATEST_PATH,
                &[("page", &page_param), ("limit", &limit_param)],
                |value| match ma_deals_from_value(value) {
                    // The page's row count is the served count: a dropped unreadable
                    // row must not read as a short page and end the walk.
                    Ok(rows) if rows.is_empty() => Shaped::empty(Ok((rows, served_row_count(value)))),
                    Ok(rows) => Shaped::ok(Ok((rows, served_row_count(value)))),
                    Err(e) => {
                        let detail = format!("{e:#}");
                        Shaped::malformed(Err(detail.clone())).with_detail(detail)
                    }
                },
            );
            let (rows, served_rows) = match served {
                Ok(Ok(page_rows)) => page_rows,
                Ok(Err(detail)) => {
                    let reason = GapReason::Malformed;
                    let end = MaWalkEnd::Failed { page, reason, detail };
                    return MaWalk { deals, unreadable, end };
                }
                Err(reason) => {
                    let detail = reason.as_str().to_string();
                    let end = MaWalkEnd::Failed { page, reason, detail };
                    return MaWalk { deals, unreadable, end };
                }
            };
            if page == 0 && served_rows == 0 {
                let end = MaWalkEnd::EmptyFeed;
                return MaWalk { deals, unreadable, end };
            }
            unreadable += served_rows.saturating_sub(rows.len());
            let left_the_window = rows.iter().any(|r| r.transaction_date.as_str() < since);
            deals.extend(rows);
            if left_the_window {
                let end = MaWalkEnd::Boundary;
                return MaWalk { deals, unreadable, end };
            }
            if served_rows < MA_PAGE_LIMIT {
                let end = MaWalkEnd::FeedEnd;
                return MaWalk { deals, unreadable, end };
            }
        }
        let end = MaWalkEnd::Cap;
        MaWalk { deals, unreadable, end }
    }
}

/// Readable or not; the walk's page measure.
fn served_row_count(value: &Value) -> usize {
    value.as_array().map(Vec::len).unwrap_or(0)
}

/// No feed at all off the first page, a truncated feed off a later one.
fn ma_walk_gap(page: usize, detail: &str) -> String {
    if page == 0 {
        format!("FMP M&A feed unavailable ({detail})")
    } else {
        format!("FMP M&A feed truncated at page {page} ({detail})")
    }
}

/// Only FMP's definitive no-such-symbol shape, an empty array, reads `Unresolved`;
/// any other non-object body is `Unverified`, so an unreadable shape can never
/// terminally not-rate a holding (`docs/portfolio-analysis.md` §Asset eligibility).
/// A present object with blank or missing fields resolves with those fields `None`.
fn profile_identity_from_value(value: &Value) -> crate::portfolio::listing::ProfileLookup {
    use crate::portfolio::listing::{ProfileIdentity, ProfileLookup};
    if value.as_array().is_some_and(|a| a.is_empty()) {
        return ProfileLookup::Unresolved;
    }
    let Some(obj) = value
        .as_array()
        .and_then(|a| a.first())
        .or(Some(value))
        .filter(|o| o.is_object())
    else {
        return ProfileLookup::Unverified(
            "FMP profile body unreadable (drifted response shape)".to_string(),
        );
    };
    let field = |key: &str| {
        obj.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    ProfileLookup::Resolved(ProfileIdentity {
        currency: field("currency"),
        is_adr: obj.get("isAdr").and_then(Value::as_bool),
        company_name: field("companyName"),
        website: field("website"),
        exchange: field("exchange"),
        sector: field("sector"),
        industry: field("industry"),
        beta: obj.get("beta").and_then(Value::as_f64).filter(|b| b.is_finite()),
    })
}

/// The expense ratio arrives in percent units (0.09 = 9 bps) and normalizes to a
/// decimal ratio at this seam. Returns whether the body was readable, so the
/// caller's tracker row can distinguish a shaped fill from a malformed body.
fn fund_info_into(value: &Value, fund: &mut crate::portfolio::fund::FundData) -> bool {
    let obj = value.as_array().and_then(|a| a.first()).or(Some(value));
    let Some(obj) = obj.filter(|o| o.is_object()) else {
        fund.gaps.push("FMP etf/info was malformed".to_string());
        return false;
    };
    // Blank strings normalize to `None` here: the sweep's comparability gates key on
    // presence, and a blank asset class read as "present" would fabricate an overlay
    // clear.
    let clean_str = |key: &str| {
        obj.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    fund.name = clean_str("name");
    fund.asset_class = clean_str("assetClass");
    fund.expense_ratio = obj
        .get("expenseRatio")
        .and_then(Value::as_f64)
        .map(|percent| percent / 100.0);
    // Numeric-first per key: a drifted `aum: null` beside `assetsUnderManagement` must
    // not erase the value.
    fund.aum = obj
        .get("aum")
        .and_then(Value::as_f64)
        .or_else(|| obj.get("assetsUnderManagement").and_then(Value::as_f64));
    fund.nav = obj.get("nav").and_then(Value::as_f64);
    true
}

/// `(label, fraction)` pairs. `weightPercentage` is percent by wire contract, served
/// as `"97.82%"` or numeric `1.8` alike, so every value divides by 100 with no unit
/// heuristic: a sum- or element-shaped guess misreads exactly the sparse sets where
/// the stakes are highest (a lone `1.4` row read as a fraction priced a whole fund
/// off a 1.4% sleeve), while a fraction-served set would read tiny and trip the
/// coverage guards, the fail-safe direction. A served percent is usable only when
/// finite and within `0..=100` (`docs/data-sources.md` §Financial Modeling Prep):
/// `f64::from_str` accepts `"NaN"` / `"inf"`, and a NaN United States row would read
/// as a 100% US share through `us_share`'s `min(1.0)`.
fn weights_from_value(value: &Value, label_key: &str) -> Vec<(String, f64)> {
    let Some(rows) = value.as_array() else {
        return vec![];
    };
    rows.iter()
        .filter_map(|row| {
            let label = row.get(label_key).and_then(Value::as_str)?.to_string();
            let raw = row.get("weightPercentage")?;
            let percent = match raw {
                Value::Number(n) => n.as_f64()?,
                Value::String(s) => s.trim().trim_end_matches('%').parse::<f64>().ok()?,
                _ => return None,
            };
            (percent.is_finite() && (0.0..=100.0).contains(&percent))
                .then(|| (label, percent / 100.0))
        })
        .collect()
}

/// The suite's dated-row rule (`docs/data-sources.md` §Financial Modeling Prep): the
/// date is stored canonical ([`canonical_date`]) and a dateless row is dropped, since
/// a dateless row stored as `""` would sort before every sample date and hold a
/// history slot. The same off-board guard as [`sector_pe_from_value`]: a row served
/// under another board, or none, fails the whole body naming the served board. A
/// `pe` is usable only when finite and inside `(0.0, SECTOR_PE_MAX]`; the fund
/// type's `pe` is required, so a dropped print holds no slot and the history
/// sampler falls to the next in-quarter print.
fn sector_pe_rows_from_value(
    value: &Value,
    requested_exchange: &str,
) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
    let Some(rows) = value.as_array() else {
        return Ok(vec![]);
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let served = row.get("exchange").and_then(Value::as_str);
        if served != Some(requested_exchange) {
            anyhow::bail!(
                "FMP sector-P/E returned exchange {} for an {requested_exchange:?} request — \
                 the exchange filter was ignored",
                served.map_or_else(|| "<absent>".to_string(), |s| format!("{s:?}"))
            );
        }
        let Some(sector) = row.get("sector").and_then(Value::as_str) else {
            continue;
        };
        let Some(date) = row.get("date").and_then(Value::as_str).and_then(canonical_date) else {
            continue;
        };
        let pe = match row.get("pe") {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
            _ => None,
        };
        let Some(pe) = pe.filter(|p| p.is_finite() && *p > 0.0 && *p <= SECTOR_PE_MAX) else {
            continue;
        };
        out.push(crate::portfolio::fund::SectorPe {
            sector: sector.to_string(),
            exchange: requested_exchange.to_string(),
            date,
            pe,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod suite_tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    fn source(base: &str) -> FmpDataSource {
        FmpDataSource::new("test-key".to_string())
            .unwrap()
            .with_base_url(base)
    }

    #[test]
    fn quarterly_income_rows_parse_with_filing_dates() {
        let body = r#"[
          {"reportedCurrency":"USD","date":"2026-03-31","filingDate":"2026-05-01","revenue":95000000000.0,
           "epsDiluted":1.55,"weightedAverageShsOutDil":15000000000.0},
          {"reportedCurrency":"USD","date":"2025-12-31","fillingDate":"2026-01-30","revenue":120000000000.0,
           "epsdiluted":2.10,"weightedAverageShsOutDil":15100000000.0}
        ]"#;
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body,
        }]);
        let mut gaps = vec![];
        let rows = source(&server.base_url).fetch_quarterly_income("AAPL", &mut gaps, &mut vec![]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].period_end, "2026-03-31");
        assert_eq!(rows[0].filing_date.as_deref(), Some("2026-05-01"));
        // Both key spellings parse (epsDiluted / epsdiluted, filingDate / fillingDate).
        assert_eq!(rows[1].eps_diluted, Some(2.10));
        assert_eq!(rows[1].filing_date.as_deref(), Some("2026-01-30"));
        assert!(gaps.is_empty());
        assert_eq!(server.request_paths(), vec!["/income-statement"]);
    }

    #[test]
    fn annual_income_reads_the_five_fiscal_years_and_their_margin_lines() {
        let body = r#"[
          {"reportedCurrency":"USD","date":"2025-12-31","filingDate":"2026-02-20","revenue":40.0e9,"netIncome":4.0e9},
          {"reportedCurrency":"USD","date":"2024-12-31","fillingDate":"2025-02-21","revenue":36.0e9,"netIncome":null},
          {"reportedCurrency":"USD","date":"FY2023","revenue":30.0e9,"netIncome":2.0e9}
        ]"#;
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body,
        }]);
        let mut gaps = vec![];
        let rows = source(&server.base_url).fetch_annual_income("FCX", &mut gaps, &mut vec![]);
        assert_eq!(rows.len(), 2, "the undatable row is unreadable: {rows:?}");
        assert_eq!(rows[0].period_end, "2025-12-31");
        assert_eq!(rows[0].filing_date.as_deref(), Some("2026-02-20"));
        assert_eq!(rows[0].revenue, Some(40.0e9));
        assert_eq!(rows[0].net_income, Some(4.0e9));
        // The legacy filing-date spelling parses; a null line stays absent.
        assert_eq!(rows[1].filing_date.as_deref(), Some("2025-02-21"));
        assert_eq!(rows[1].net_income, None);
        assert!(gaps.is_empty(), "{gaps:?}");
        let target = &server.request_targets()[0];
        assert!(target.starts_with("/income-statement"), "{target}");
        assert!(target.contains("period=annual"), "{target}");
        assert!(target.contains("limit=5"), "{target}");
    }

    #[test]
    fn annual_income_fails_soft_on_drift_and_a_foreign_currency() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"rows":[]}"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"reportedCurrency":"CAD","date":"2025-12-31","revenue":1.0,"netIncome":0.1}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"reportedCurrency":"USD","revenue":1.0}]"#,
            },
        ]);
        let source = source(&server.base_url);
        let mut gaps = vec![];
        let mut unit_issues = vec![];
        assert!(source
            .fetch_annual_income("FCX", &mut gaps, &mut unit_issues)
            .is_empty());
        assert!(source
            .fetch_annual_income("FCX", &mut gaps, &mut unit_issues)
            .is_empty());
        assert!(source
            .fetch_annual_income("FCX", &mut gaps, &mut unit_issues)
            .is_empty());
        assert_eq!(gaps.len(), 3, "{gaps:?}");
        assert!(gaps[0].contains("annual income statements were malformed"), "{gaps:?}");
        assert!(gaps[2].contains("annual income statements were malformed"), "{gaps:?}");
        assert_eq!(unit_issues.len(), 1, "{unit_issues:?}");
        assert_eq!(unit_issues[0].reported_currency.as_deref(), Some("CAD"));
    }

    #[test]
    fn dated_bars_keep_the_volume_and_read_an_unusable_one_as_missing() {
        let body = r#"[
          {"date":"2026-07-15","price":196.2,"volume":1200000},
          {"date":"2026-07-10","price":192.5,"volume":-5},
          {"date":"2026-07-14","price":195.0},
          {"date":"2026-07-11","price":193.0,"volume":0},
          {"date":"2026-07-09","price":0,"volume":900000},
          {"date":"broken","price":191.0,"volume":800000}
        ]"#;
        let server = MockHttp::serve(vec![Canned::Reply { status: 200, headers: vec![], body }]);
        let bars = source(&server.base_url)
            .fetch_dated_bars("AAPL", 1_600)
            .unwrap();
        // The unusable close and the undatable row drop; a bad or absent
        // volume keeps its close as a missing volume day; a served zero stays.
        let read: Vec<(&str, Option<f64>)> =
            bars.iter().map(|b| (b.date.as_str(), b.volume)).collect();
        assert_eq!(
            read,
            vec![
                ("2026-07-10", None),
                ("2026-07-11", Some(0.0)),
                ("2026-07-14", None),
                ("2026-07-15", Some(1_200_000.0)),
            ]
        );
        assert!((bars[3].close - 196.2).abs() < 1e-9);
        let target = &server.request_targets()[0];
        assert!(target.starts_with("/historical-price-eod/light"), "{target}");
        assert!(target.contains("from="), "{target}");
    }

    #[test]
    fn dated_bars_read_drift_as_an_error_never_an_empty_history() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2026-08-02","price":0,"volume":10}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "[]",
            },
        ]);
        let source = source(&server.base_url);
        let err = source.fetch_dated_bars("AAPL", 30).unwrap_err().to_string();
        assert!(err.contains("no served row was readable"), "{err}");
        assert!(source
            .fetch_dated_bars("AAPL", 30)
            .expect("an empty body is an honest no-history")
            .is_empty());
    }

    #[test]
    fn quarterly_statement_dates_store_the_canonical_render() {
        // As source text "2026-9-30" sorts after "2026-12-31" under every lexicographic
        // consumer, the run reads non-contiguous and TTM adoption fails onto the
        // annual basis.
        let income = r#"[
          {"reportedCurrency":"USD","date":"2026-9-30","filingDate":"2026-11-5","revenue":1.0},
          {"reportedCurrency":"USD","date":"2026-6-30","filingDate":"soon","fillingDate":"2026-8-1","revenue":2.0},
          {"reportedCurrency":"USD","date":"2026-3-31","filingDate":"never","fillingDate":"later","revenue":3.0},
          {"reportedCurrency":"USD","date":"Q4 2025","filingDate":"2026-02-01","revenue":4.0}
        ]"#;
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: income,
        }]);
        let mut gaps = vec![];
        let rows = source(&server.base_url).fetch_quarterly_income("AAPL", &mut gaps, &mut vec![]);
        assert_eq!(
            rows.len(),
            3,
            "the undatable-period row is unreadable: {rows:?}"
        );
        assert_eq!(rows[0].period_end, "2026-09-30");
        assert_eq!(rows[0].filing_date.as_deref(), Some("2026-11-05"));
        // Datable-string-first: an undatable `filingDate` falls through to the
        // legacy spelling...
        assert_eq!(rows[1].period_end, "2026-06-30");
        assert_eq!(rows[1].filing_date.as_deref(), Some("2026-08-01"));
        // ...and to `None` when neither parses — the period-end + grace anchor
        // takes over, never the source text.
        assert_eq!(rows[2].period_end, "2026-03-31");
        assert_eq!(rows[2].filing_date, None);
        assert!(gaps.is_empty(), "{gaps:?}");

        // The cash-flow shaper holds the same rule; an impossible calendar date
        // is as unreadable as a non-date.
        let cash = r#"[
          {"reportedCurrency":"USD","date":"2026-9-30","fillingDate":"2026-11-5","freeCashFlow":1.0},
          {"reportedCurrency":"USD","date":"2026-06-31","freeCashFlow":2.0}
        ]"#;
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: cash,
        }]);
        let mut gaps = vec![];
        let rows =
            source(&server.base_url).fetch_quarterly_cash_flow("AAPL", &mut gaps, &mut vec![]);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].period_end, "2026-09-30");
        assert_eq!(rows[0].filing_date.as_deref(), Some("2026-11-05"));
        assert!(gaps.is_empty(), "{gaps:?}");
    }

    #[test]
    fn quarterly_statement_dates_all_undatable_read_malformed() {
        // An undatable date is unreadable exactly as a dateless row is, never a
        // benign `empty`.
        let body = r#"[{"reportedCurrency":"USD","date":"soon","revenue":1.0},{"reportedCurrency":"USD","date":"Q3","revenue":2.0}]"#;
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body,
        }]);
        let mut gaps = vec![];
        let rows = source(&server.base_url).fetch_quarterly_income("AAPL", &mut gaps, &mut vec![]);
        assert!(rows.is_empty(), "{rows:?}");
        assert_eq!(
            gaps,
            vec!["FMP quarterly income statements were malformed".to_string()]
        );

        let body = r#"[{"reportedCurrency":"USD","date":"soon","freeCashFlow":1.0}]"#;
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body,
        }]);
        let mut gaps = vec![];
        let rows =
            source(&server.base_url).fetch_quarterly_cash_flow("AAPL", &mut gaps, &mut vec![]);
        assert!(rows.is_empty(), "{rows:?}");
        assert_eq!(
            gaps,
            vec!["FMP quarterly cash-flow statements were malformed".to_string()]
        );
    }

    #[test]
    fn quarterly_statement_dates_unpadded_still_adopt_the_ttm_basis() {
        // Four consecutive quarters served with mixed padding: as source text
        // `quarters_contiguous` read the misordered run as a gap and adoption failed
        // onto the annual basis with nothing wrong in the data.
        let body = r#"[
          {"date":"2026-12-31","revenue":10.0,"netIncome":1.0},
          {"date":"2026-9-30","revenue":10.0,"netIncome":1.0},
          {"date":"2026-6-30","revenue":10.0,"netIncome":1.0},
          {"date":"2026-3-31","revenue":10.0,"netIncome":1.0}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let mut fin = crate::portfolio::engine::CompanyFinancials {
            quarterly_income: quarterly_income_from_value(&value),
            ..Default::default()
        };
        assert!(
            crate::portfolio::dossier::apply_ttm_statement_basis(&mut fin),
            "{:?}",
            fin.quarterly_income
        );
        let ends: Vec<&str> = fin.quarterly_income.iter().map(|r| r.period_end.as_str()).collect();
        assert_eq!(ends, ["2026-12-31", "2026-09-30", "2026-06-30", "2026-03-31"]);
        assert_eq!(fin.statement_basis, Some(crate::portfolio::StatementBasis::Ttm));
        assert_eq!(fin.revenue, Some(40.0));
    }

    #[test]
    fn quarterly_income_null_stable_eps_falls_through_to_the_legacy_spelling() {
        // FMP serves JSON nulls: a present-but-null `epsDiluted` must not block the
        // legacy `epsdiluted` value.
        let value: Value = serde_json::from_str(
            r#"[{"date":"2026-03-31","epsDiluted":null,"epsdiluted":1.55,"revenue":95e9}]"#,
        )
        .unwrap();
        let rows = quarterly_income_from_value(&value);
        assert_eq!(rows[0].eps_diluted, Some(1.55));
    }

    #[test]
    fn balance_sheet_null_stockholders_equity_falls_through_to_total_equity() {
        // FMP serves JSON nulls: a present-but-null preferred line must not block
        // the alternate key.
        let value: Value = serde_json::from_str(
            r#"[{"date":"2026-03-31","totalDebt":null,"totalStockholdersEquity":null,"totalEquity":63.0e9}]"#,
        )
        .unwrap();
        let lines = BalanceSheetLines::from_newest(&quarterly_balance_sheet_from_value(&value));
        assert_eq!(lines.total_equity, Some(63.0e9));
        assert_eq!(lines.total_debt, None);
    }

    #[test]
    fn balance_sheet_rows_keep_every_dated_print_and_the_newest_feeds_the_lines() {
        let value: Value = serde_json::from_str(
            r#"[
            {"date":"2026-03-31","filingDate":"2026-05-01","totalDebt":110.0e9,"totalStockholdersEquity":62.0e9,
             "cashAndCashEquivalents":30.0e9,"shortTermInvestments":32.0e9,"netReceivables":60.0e9,"inventory":7.0e9},
            {"date":"2025-03-31","totalDebt":100.0e9,"totalEquity":58.0e9,"cashAndCashEquivalents":28.0e9,
             "netReceivables":50.0e9,"inventory":6.0e9},
            {"noDate":true}
        ]"#,
        )
        .unwrap();
        let rows = quarterly_balance_sheet_from_value(&value);
        assert_eq!(rows.len(), 2, "the dateless row is unreadable and dropped");
        assert_eq!(rows[0].period_end, "2026-03-31");
        assert_eq!(rows[0].filing_date.as_deref(), Some("2026-05-01"));
        assert_eq!(rows[0].net_receivables, Some(60.0e9));
        assert_eq!(rows[0].inventory, Some(7.0e9));
        assert_eq!(rows[1].total_equity, Some(58.0e9), "totalEquity is the fallback");
        // The lines come off the newest row by period end, whichever order the
        // wire served.
        let mut reversed = rows.clone();
        reversed.reverse();
        let lines = BalanceSheetLines::from_newest(&reversed);
        assert_eq!(lines.total_debt, Some(110.0e9));
        assert_eq!(lines.short_term_investments, Some(32.0e9));
        assert_eq!(BalanceSheetLines::from_newest(&[]), BalanceSheetLines::default());
    }

    #[test]
    fn statement_rows_keep_the_factor_machinerys_lines_and_absent_reads_none() {
        // An absent or null line reads `None`.
        let income: Value = serde_json::from_str(
            r#"[
            {"date":"2026-03-31","revenue":95e9,"researchAndDevelopmentExpenses":8.5e9,
             "incomeBeforeTax":30e9,"incomeTaxExpense":4.8e9},
            {"date":"2025-12-31","revenue":124e9,"researchAndDevelopmentExpenses":null}
        ]"#,
        )
        .unwrap();
        let rows = quarterly_income_from_value(&income);
        assert_eq!(rows[0].research_and_development, Some(8.5e9));
        assert_eq!(rows[0].income_before_tax, Some(30e9));
        assert_eq!(rows[0].income_tax_expense, Some(4.8e9));
        assert_eq!(rows[1].research_and_development, None);
        assert_eq!(rows[1].income_before_tax, None);
        assert_eq!(rows[1].income_tax_expense, None);

        let balance: Value = serde_json::from_str(
            r#"[{"date":"2026-03-31","totalAssets":344e9},{"date":"2025-12-31"}]"#,
        )
        .unwrap();
        let rows = quarterly_balance_sheet_from_value(&balance);
        assert_eq!(rows[0].total_assets, Some(344e9));
        assert_eq!(rows[1].total_assets, None);

        let cash: Value = serde_json::from_str(
            r#"[{"date":"2026-03-31","depreciationAndAmortization":3.1e9},{"date":"2025-12-31"}]"#,
        )
        .unwrap();
        let rows = quarterly_cash_flow_from_value(&cash);
        assert_eq!(rows[0].depreciation_and_amortization, Some(3.1e9));
        assert_eq!(rows[1].depreciation_and_amortization, None);
    }

    #[test]
    fn statement_pulls_request_sixteen_quarters_on_every_leg() {
        assert_eq!(INCOME_QUARTERS_LIMIT, "16");
        assert_eq!(BALANCE_SHEET_QUARTERS_LIMIT, "16");
        assert_eq!(CASH_FLOW_QUARTERS_LIMIT, "16");
    }

    #[test]
    fn an_equal_date_duplicate_feeds_the_lines_from_the_row_canonicalization_keeps() {
        // Two prints for one period with the same filing date and conflicting
        // values: canonicalization keeps the first served; the flat lines must
        // come from that same row, never the other.
        let value: Value = serde_json::from_str(
            r#"[
            {"date":"2026-03-31","filingDate":"2026-05-01","totalDebt":1.0e9,"cashAndCashEquivalents":5.0e9},
            {"date":"2026-03-31","filingDate":"2026-05-01","totalDebt":2.0e9,"cashAndCashEquivalents":6.0e9},
            {"date":"2025-12-31","filingDate":"2026-02-01","totalDebt":3.0e9,"cashAndCashEquivalents":7.0e9}
        ]"#,
        )
        .unwrap();
        let rows = quarterly_balance_sheet_from_value(&value);
        let lines = BalanceSheetLines::from_newest(&rows);
        let mut fin = crate::portfolio::engine::CompanyFinancials {
            quarterly_balance_sheet: rows,
            ..Default::default()
        };
        crate::portfolio::engine::canonicalize_statements(&mut fin);
        assert_eq!(fin.quarterly_balance_sheet.len(), 2);
        let kept = &fin.quarterly_balance_sheet[0];
        assert_eq!(kept.total_debt, Some(1.0e9), "first served wins the tie");
        assert_eq!(lines.total_debt, kept.total_debt);
        assert_eq!(lines.cash_and_equivalents, kept.cash_and_equivalents);
    }

    #[test]
    fn financial_scores_round_trip_and_degrade_to_gaps() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"AAPL","altmanZScore":9.32,"piotroskiScore":8}]"#,
            },
            Canned::Reply { status: 200, headers: vec![], body: r#"[{"symbol":"AAPL"}]"# },
            Canned::Reply { status: 200, headers: vec![], body: r#"{"unexpected":true}"# },
            Canned::Reply { status: 402, headers: vec![], body: "Payment Required" },
        ]);
        let src = source(&server.base_url);
        let mut gaps = vec![];
        let scores = src.fetch_financial_scores("AAPL", &mut gaps).expect("scores");
        assert_eq!(scores.altman_z, Some(9.32));
        assert_eq!(scores.piotroski, Some(8.0));
        assert!(gaps.is_empty(), "{gaps:?}");
        // A row carrying neither score is no usable data — a gap, never a clear.
        let mut gaps = vec![];
        assert!(src.fetch_financial_scores("AAPL", &mut gaps).is_none());
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        // A malformed body and the premium gate degrade the same way.
        let mut gaps = vec![];
        assert!(src.fetch_financial_scores("AAPL", &mut gaps).is_none());
        assert!(gaps[0].contains("empty or malformed"), "{gaps:?}");
        let mut gaps = vec![];
        assert!(src.fetch_financial_scores("AAPL", &mut gaps).is_none());
        assert!(gaps[0].contains("unavailable"), "{gaps:?}");
        assert_eq!(server.request_paths(), vec!["/financial-scores"; 4]);
    }

    #[test]
    fn balance_sheet_gaps_on_a_malformed_body_and_a_premium_gate() {
        // Malformed body (not the expected array) → a tagged gap, both lines None.
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: r#"{"unexpected":true}"# },
            Canned::Reply { status: 402, headers: vec![], body: "Payment Required" },
        ]);
        let src = source(&server.base_url);
        let mut gaps = vec![];
        assert!(src.fetch_balance_sheet("AAPL", &mut gaps, &mut vec![]).is_empty());
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        // Premium gate (402) → the same fail-soft shape with the gated reason.
        let mut gaps = vec![];
        assert!(src.fetch_balance_sheet("AAPL", &mut gaps, &mut vec![]).is_empty());
        assert!(gaps[0].contains("unavailable"), "{gaps:?}");
    }

    #[test]
    fn consensus_blends_the_two_nearest_forward_years_by_ntm_overlap() {
        // Today 2026-07-16; the near fiscal year ends 2026-09-30 (76 days out), the far
        // one 2027-09-30, so the near row weighs 76/365 and the far row the rest. The
        // stale 2025 row never enters.
        let body = r#"[
          {"date":"2025-09-30","epsAvg":6.1,"epsLow":5.9,"epsHigh":6.3,
           "revenueAvg":400e9,"revenueLow":390e9,"revenueHigh":410e9},
          {"date":"2027-09-30","epsAvg":7.4,"epsLow":7.0,"epsHigh":7.8,
           "revenueAvg":460e9,"revenueLow":450e9,"revenueHigh":470e9},
          {"date":"2026-09-30","epsAvg":6.8,"epsLow":6.5,"epsHigh":7.1,
           "revenueAvg":430e9,"revenueLow":420e9,"revenueHigh":440e9}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let c = consensus_from_value(&value, "2026-07-16").unwrap();
        assert_eq!(c.period_end, "2026-09-30", "the near row names the period");
        assert_eq!(c.periods_used, 2);
        let w = 76.0 / 365.0;
        assert!((c.near_weight - w).abs() < 1e-12);
        assert!((c.eps_mid.unwrap() - (w * 6.8 + (1.0 - w) * 7.4)).abs() < 1e-9);
        assert!((c.eps_low.unwrap() - (w * 6.5 + (1.0 - w) * 7.0)).abs() < 1e-9);
        assert!((c.revenue_high.unwrap() - (w * 440e9 + (1.0 - w) * 470e9)).abs() < 1.0);
        assert_eq!(
            c.eps_periods,
            vec![
                crate::portfolio::engine::ConsensusEpsPeriod {
                    period_end: "2026-09-30".into(),
                    eps_mid: Some(6.8),
                    ntm_weight: w,
                },
                crate::portfolio::engine::ConsensusEpsPeriod {
                    period_end: "2027-09-30".into(),
                    eps_mid: Some(7.4),
                    ntm_weight: 1.0 - w,
                },
            ]
        );
    }

    #[test]
    fn consensus_orders_on_parsed_dates_never_the_source_text() {
        // "2026-9-30" sorts lexicographically after "2026-12-31": a raw-string sort
        // would make December the near leg and invert the blend's weights.
        let body = r#"[
          {"date":"2026-12-31","epsAvg":9.0},
          {"date":"2026-9-30","epsAvg":6.0}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let c = consensus_from_value(&value, "2026-08-05").unwrap();
        assert_eq!(c.period_end, "2026-9-30", "the September row is near");
        assert_eq!(c.periods_used, 2);
        // near_weight = days(2026-09-30 − 2026-08-05)/365 = 56/365; the blend
        // must lean far (December), not the reverse.
        assert!((c.near_weight - 56.0 / 365.0).abs() < 1e-9, "{}", c.near_weight);
        let expected = c.near_weight * 6.0 + (1.0 - c.near_weight) * 9.0;
        assert!((c.eps_mid.unwrap() - expected).abs() < 1e-9);
        assert_eq!(
            c.eps_periods
                .iter()
                .map(|period| period.period_end.as_str())
                .collect::<Vec<_>>(),
            ["2026-09-30", "2026-12-31"],
            "revision identities are canonical even when the wire date is not"
        );
        // An undatable row never enters the forward set.
        let junk: Value = serde_json::from_str(
            r#"[{"date":"soon","epsAvg":9.9},{"date":"2026-09-30","epsAvg":6.0}]"#,
        )
        .unwrap();
        let c = consensus_from_value(&junk, "2026-08-05").unwrap();
        assert_eq!(c.periods_used, 1);
        assert_eq!(c.eps_mid, Some(6.0));
    }

    #[test]
    fn consensus_with_one_forward_row_keeps_single_row_semantics() {
        let body = r#"[
          {"date":"2025-09-30","epsAvg":6.1},
          {"date":"2026-09-30","epsAvg":6.8,"epsLow":6.5,"epsHigh":7.1}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let c = consensus_from_value(&value, "2026-07-16").unwrap();
        assert_eq!(c.period_end, "2026-09-30");
        assert_eq!(c.periods_used, 1);
        assert!((c.near_weight - 1.0).abs() < 1e-12);
        assert_eq!(c.eps_mid, Some(6.8));
        assert_eq!(c.eps_periods.len(), 1);
        assert_eq!(c.eps_periods[0].ntm_weight, 1.0);
    }

    #[test]
    fn consensus_blend_uses_a_leg_only_one_row_carries() {
        // The far row publishes no low/high: the mid blends, the spread legs fall to
        // the near row's values alone rather than dropping to None.
        let body = r#"[
          {"date":"2026-09-30","epsAvg":6.8,"epsLow":6.5,"epsHigh":7.1},
          {"date":"2027-09-30","epsAvg":7.4}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let c = consensus_from_value(&value, "2026-07-16").unwrap();
        assert_eq!(c.periods_used, 2);
        let w = 76.0 / 365.0;
        assert!((c.eps_mid.unwrap() - (w * 6.8 + (1.0 - w) * 7.4)).abs() < 1e-9);
        assert_eq!(c.eps_low, Some(6.5));
        assert_eq!(c.eps_high, Some(7.1));
        // Both rows carried the EPS mid; neither carried revenue — the per-field
        // corroboration counts record exactly that, not the blend count.
        assert_eq!(c.eps_mid_rows, 2);
        assert_eq!(c.revenue_mid_rows, 0);
    }

    #[test]
    fn consensus_counts_field_carrying_rows_not_blended_rows() {
        // Two forward rows blend (`periods_used = 2`) but only the near row carries an
        // EPS mid; the clamp release must see one EPS row, not two.
        let body = r#"[
          {"date":"2026-09-30","epsAvg":6.8,"revenueAvg":1.0e9},
          {"date":"2027-09-30","revenueAvg":1.1e9}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let c = consensus_from_value(&value, "2026-07-16").unwrap();
        assert_eq!(c.periods_used, 2);
        assert_eq!(c.eps_mid_rows, 1, "one estimate must not masquerade as two");
        assert_eq!(c.revenue_mid_rows, 2);
        assert_eq!(c.eps_mid, Some(6.8), "the single leg is still used alone");
    }

    #[test]
    fn consensus_boundary_day_near_row_carries_no_corroboration() {
        // `today == near_date`: the near weight is exactly 0, so a present-but-
        // weightless near row must not count as a second estimate. The fallback stays
        // a real contributor: revenue exists only on the near row, is used alone
        // despite the zero weight, and counts as one.
        let body = r#"[
          {"date":"2026-09-30","epsAvg":6.8,"revenueAvg":1.0e9},
          {"date":"2027-09-30","epsAvg":7.4}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let c = consensus_from_value(&value, "2026-09-30").unwrap();
        assert_eq!(c.periods_used, 2);
        assert!((c.near_weight - 0.0).abs() < 1e-12);
        assert_eq!(c.eps_mid, Some(7.4), "the value is entirely the far row's");
        assert_eq!(c.eps_mid_rows, 1, "a weightless near row is not corroboration");
        assert_eq!(c.revenue_mid_rows, 1, "the used-alone fallback still contributes");
        assert_eq!(c.revenue_mid, Some(1.0e9));
    }

    #[test]
    fn consensus_far_fiscal_year_beyond_the_window_is_unused() {
        // The near fiscal year ends ≥ 12 months out (just past a FY end): the rolling
        // window lies entirely inside it, so the far row carries no weight — not even
        // through a leg the near row lacks (a far year must never leak in at full
        // weight when the blend is inactive).
        let body = r#"[
          {"date":"2027-09-30","epsAvg":6.8,"epsHigh":7.1},
          {"date":"2028-09-30","epsAvg":7.4,"epsLow":7.0,"epsHigh":7.8}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let c = consensus_from_value(&value, "2026-09-29").unwrap();
        assert_eq!(c.periods_used, 1);
        assert!((c.near_weight - 1.0).abs() < 1e-12);
        assert_eq!(c.eps_mid, Some(6.8));
        assert_eq!(c.eps_low, None, "the far row's lone leg must not leak in");
    }

    #[test]
    fn consensus_without_a_forward_row_is_none_never_a_stale_row() {
        // Only past fiscal-year rows: a stale estimate must not masquerade as forward
        // consensus — the driver ladder abstains under `no-admissible-driver` instead.
        let body = r#"[
          {"date":"2025-09-30","epsAvg":6.1,"epsLow":5.9,"epsHigh":6.3},
          {"date":"2026-06-30","epsAvg":6.5,"epsLow":6.2,"epsHigh":6.8}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        assert!(consensus_from_value(&value, "2026-07-16").is_none());
    }

    #[test]
    fn consensus_null_stable_keys_fall_through_to_legacy_values() {
        // Both rows carry present-but-null stable keys beside numeric legacy
        // spellings — the blend must read the legacy values, not None them.
        let body = r#"[
          {"date":"2026-09-30","epsAvg":null,"estimatedEpsAvg":6.8},
          {"date":"2027-09-30","epsAvg":null,"estimatedEpsAvg":7.4}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let c = consensus_from_value(&value, "2026-07-16").unwrap();
        assert_eq!(c.periods_used, 2);
        let w = 76.0 / 365.0;
        assert!((c.eps_mid.unwrap() - (w * 6.8 + (1.0 - w) * 7.4)).abs() < 1e-9);
    }

    #[test]
    fn consensus_dedups_duplicate_fiscal_period_rows() {
        // Two rows share the near fiscal-period date: the duplicate must not
        // masquerade as the far year — the blend reads the true following fiscal
        // year, never the same year at both weights.
        let body = r#"[
          {"date":"2026-09-30","epsAvg":6.8},
          {"date":"2026-09-30","epsAvg":6.9},
          {"date":"2027-09-30","epsAvg":7.4}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let c = consensus_from_value(&value, "2026-07-16").unwrap();
        assert_eq!(c.periods_used, 2);
        let w = 76.0 / 365.0;
        assert!(
            (c.eps_mid.unwrap() - (w * 6.8 + (1.0 - w) * 7.4)).abs() < 1e-9,
            "the far leg must be the following fiscal year, not the duplicate"
        );
    }

    #[test]
    fn dated_eod_round_trips_sorted_dated_closes() {
        // The dated form keeps the dates the undated read discards; the anchor's
        // latest-on-or-before join needs them.
        let body = r#"[
          {"date":"2026-07-14","price":195.0},
          {"date":"2026-07-10","price":192.5},
          {"date":"broken"},
          {"date":"2026-07-15","price":196.2}
        ]"#;
        let server = MockHttp::serve(vec![Canned::Reply { status: 200, headers: vec![], body }]);
        let closes = source(&server.base_url)
            .fetch_dated_eod("AAPL", 1_600)
            .unwrap();
        assert_eq!(closes.len(), 3, "the broken row is skipped");
        assert_eq!(closes[0].date, "2026-07-10");
        assert_eq!(closes[2].date, "2026-07-15");
        assert!((closes[2].value - 196.2).abs() < 1e-9);
        let target = &server.request_targets()[0];
        assert!(target.starts_with("/historical-price-eod/light"), "{target}");
        assert!(target.contains("symbol=AAPL"), "{target}");
        assert!(target.contains("from="), "{target}");
    }

    #[test]
    fn ttm_dividends_sum_only_the_trailing_window() {
        // Bounded on both sides: the 2024 row is older than twelve months, and the
        // announced future payment (2026-08-10) is not yet trailing — including it
        // would inflate the trailing-return leg.
        let body = r#"[
          {"date":"2026-08-10","adjDividend":0.27},
          {"date":"2026-05-10","adjDividend":0.26},
          {"date":"2026-02-10","dividend":0.25},
          {"date":"2024-11-10","adjDividend":0.24}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 16).unwrap();
        let ttm = dividends_from_value(&value, today)
            .map(|pull| pull.ttm)
            .unwrap()
            .unwrap();
        assert!(
            (ttm - 0.51).abs() < 1e-9,
            "{ttm}: the 2024 row is outside the window and the future row is excluded"
        );
        // No rows in the window → None, never a fabricated yield.
        let stale: Value = serde_json::from_str(r#"[{"date":"2020-01-01","dividend":1.0}]"#).unwrap();
        assert!(dividends_from_value(&stale, today).map(|pull| pull.ttm).unwrap().is_none());
    }

    #[test]
    fn ttm_dividend_window_is_365_days_exclusive_lower_bound() {
        // Exactly `today − 365` is OUT: inclusive-both-ends the window spans 366
        // distinct days, and a quarterly payer with a year-ago ex-date landing
        // on the boundary would sum FIVE payments into "trailing twelve months"
        // (an annual payer would double). `today` itself is in.
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
        let body = r#"[
          {"date":"2026-08-12","adjDividend":0.25},
          {"date":"2026-05-11","adjDividend":0.25},
          {"date":"2026-02-09","adjDividend":0.25},
          {"date":"2025-11-10","adjDividend":0.25},
          {"date":"2025-08-12","adjDividend":0.25}
        ]"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let ttm = dividends_from_value(&value, today)
            .map(|pull| pull.ttm)
            .unwrap()
            .unwrap();
        assert!(
            (ttm - 1.0).abs() < 1e-9,
            "{ttm}: the 2025-08-12 boundary row (today − 365) must be excluded"
        );
    }

    #[test]
    fn ttm_dividends_reject_unreadable_rows_rather_than_reading_non_payer() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 3).unwrap();
        // An in-window row with a string-typed amount is unreadable — `Err`,
        // never `None` (which downstream reads as a confirmed non-payer, i.e. a
        // dividend elimination).
        let v: Value =
            serde_json::from_str(r#"[{"date":"2026-05-10","adjDividend":"0.26"}]"#).unwrap();
        assert!(dividends_from_value(&v, today)
            .map(|pull| pull.ttm)
            .is_err());
        // A dateless row cannot be windowed — likewise.
        let v: Value = serde_json::from_str(r#"[{"adjDividend":0.26}]"#).unwrap();
        assert!(dividends_from_value(&v, today)
            .map(|pull| pull.ttm)
            .is_err());
        // A non-ISO date compares lexicographically as out-of-window — it must
        // error, never slide into a false non-payer.
        let v: Value =
            serde_json::from_str(r#"[{"date":"not-a-date","adjDividend":0.26}]"#).unwrap();
        assert!(dividends_from_value(&v, today)
            .map(|pull| pull.ttm)
            .is_err());
        // A non-zero-padded (but real) date parses and windows on the PARSED
        // value — as text it sorts after today and would silently drop the
        // in-window payment.
        let v: Value =
            serde_json::from_str(r#"[{"date":"2026-5-10","adjDividend":0.26}]"#).unwrap();
        assert_eq!(
            dividends_from_value(&v, today)
                .map(|pull| pull.ttm)
                .unwrap(),
            Some(0.26)
        );
        // An affirmatively empty body is the real non-payer…
        let v: Value = serde_json::from_str("[]").unwrap();
        assert_eq!(
            dividends_from_value(&v, today)
                .map(|pull| pull.ttm)
                .unwrap(),
            None
        );
        // …and junk on an out-of-window row is irrelevant, not a failure.
        let v: Value =
            serde_json::from_str(r#"[{"date":"2020-01-10","adjDividend":"junk"}]"#).unwrap();
        assert_eq!(
            dividends_from_value(&v, today)
                .map(|pull| pull.ttm)
                .unwrap(),
            None
        );
    }

    #[test]
    fn ttm_dividends_reject_an_overflowed_in_window_sum_as_a_drifted_body() {
        // Two feed-extreme in-window prints overflow their sum to inf, which would ride
        // the required `forward_dividends` into a persisted `null`; an overflowed sum
        // is a drifted body, so the leg reads zero with the gap.
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 3).unwrap();
        let body = r#"[{"date":"2026-05-10","adjDividend":1.7976931348623157e308},
                       {"date":"2026-02-10","adjDividend":1.7976931348623157e308}]"#;
        let v: Value = serde_json::from_str(body).unwrap();
        let err = dividends_from_value(&v, today)
            .map(|pull| pull.ttm)
            .unwrap_err()
            .to_string();
        assert!(err.contains("overflowed"), "{err}");
        // One extreme print alone is a finite (if absurd) sum — not this guard's case.
        let v: Value =
            serde_json::from_str(r#"[{"date":"2026-05-10","adjDividend":1.7976931348623157e308}]"#)
                .unwrap();
        assert_eq!(
            dividends_from_value(&v, today)
                .map(|pull| pull.ttm)
                .unwrap(),
            Some(f64::MAX)
        );

        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![Canned::Reply { status: 200, headers: vec![], body }]);
        let mut gaps = Vec::new();
        let ttm = source(&server.base_url)
            .with_context(ctx)
            .fetch_ttm_dividends("XOM", &mut gaps);
        assert_eq!(ttm, None);
        assert!(
            gaps.iter().any(|g| g.starts_with(DIVIDENDS_GAP_PREFIX) && g.contains("overflowed")),
            "{gaps:?}"
        );
        let finished = finished_rows(&rec);
        assert_eq!(finished.len(), 1, "{finished:?}");
        assert_eq!(finished[0].0, "company-dividends", "{:?}", finished[0]);
        assert_eq!(finished[0].1, "malformed", "{:?}", finished[0]);
        assert!(
            finished[0].2.as_deref().is_some_and(|d| d.contains("overflowed")),
            "{:?}",
            finished[0]
        );
    }

    #[test]
    fn eod_closes_that_are_not_usable_prices_are_dropped_at_both_parses() {
        // A zero or negative close is not a price: it would ride into returns and
        // drawdowns, and a zero entry close makes an episode's required
        // `price_return` a persisted `null`.
        let body: Value = serde_json::from_str(
            r#"[{"date":"2026-08-03","price":195.0},
                {"date":"2026-08-02","price":0},
                {"date":"2026-08-01","price":-3.5}]"#,
        )
        .unwrap();
        let dated = dated_eod_from_value(&body).unwrap();
        assert_eq!(dated.len(), 1, "{dated:?}");
        assert_eq!(dated[0].date, "2026-08-03");
        assert_eq!(eod_prices_from_value(&body).unwrap(), vec![195.0]);
        // An all-unusable body takes the deep pull's existing drift branch —
        // `Err` with a `malformed` row, never an honest no-history.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"[{"date":"2026-08-02","price":0}]"#,
        }]);
        let err = source(&server.base_url)
            .with_context(ctx)
            .fetch_dated_eod("AAPL", 30)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no served row was readable"), "{err}");
        let finished = finished_rows(&rec);
        assert_eq!(finished.len(), 1, "{finished:?}");
        assert_eq!(finished[0].1, "malformed", "{:?}", finished[0]);
    }

    #[test]
    fn ttm_dividends_null_adj_amount_falls_through_to_the_plain_amount() {
        // A present-but-null `adjDividend` beside a numeric `dividend` is a
        // readable row — it must sum, not take the unreadable-row bail path.
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 16).unwrap();
        let v: Value =
            serde_json::from_str(r#"[{"date":"2026-05-10","adjDividend":null,"dividend":0.26}]"#)
                .unwrap();
        assert_eq!(
            dividends_from_value(&v, today)
                .map(|pull| pull.ttm)
                .unwrap(),
            Some(0.26)
        );
    }

    #[test]
    fn fund_info_null_aum_falls_through_to_assets_under_management() {
        // Live serves only `assetsUnderManagement`; a drifted `aum: null` beside
        // it must not erase the value the fallback exists to read.
        let v: Value = serde_json::from_str(
            r#"[{"name":"SPDR S&P 500","aum":null,"assetsUnderManagement":5.4e11}]"#,
        )
        .unwrap();
        let mut fund = crate::portfolio::fund::FundData::default();
        fund_info_into(&v, &mut fund);
        assert_eq!(fund.aum, Some(5.4e11));
    }

    #[test]
    fn profile_identity_reads_array_of_one_or_bare_object() {
        use crate::portfolio::listing::ProfileLookup;
        let v: Value = serde_json::from_str(
            r#"[{"symbol":"AAPL","isAdr": true, "currency": "USD", "companyName":"Apple Inc.","website":"https://www.apple.com","exchange":"NASDAQ","sector":"Technology"}]"#,
        )
        .unwrap();
        let ProfileLookup::Resolved(identity) = profile_identity_from_value(&v) else {
            panic!("expected resolved");
        };
        assert_eq!(identity.company_name.as_deref(), Some("Apple Inc."));
        assert_eq!(identity.website.as_deref(), Some("https://www.apple.com"));
        assert_eq!(identity.exchange.as_deref(), Some("NASDAQ"));
        assert_eq!(identity.sector.as_deref(), Some("Technology"));
        assert_eq!(identity.currency.as_deref(), Some("USD"));
        assert_eq!(identity.is_adr, Some(true));
        assert_eq!(identity.beta, None, "an omitted beta reads None");
        let bare: Value = serde_json::from_str(r#"{"sector":"Energy","beta":1.086}"#).unwrap();
        let ProfileLookup::Resolved(identity) = profile_identity_from_value(&bare) else {
            panic!("expected resolved");
        };
        assert_eq!(identity.sector.as_deref(), Some("Energy"));
        assert_eq!(identity.beta, Some(1.086));
        // A null or non-numeric beta is no beta, never a fabricated zero.
        for body in [r#"{"beta":null}"#, r#"{"beta":"1.1"}"#] {
            let v: Value = serde_json::from_str(body).unwrap();
            let ProfileLookup::Resolved(identity) = profile_identity_from_value(&v) else {
                panic!("expected resolved");
            };
            assert_eq!(identity.beta, None, "{body}");
        }
    }

    #[test]
    fn profile_identity_only_an_empty_array_reads_unresolved() {
        use crate::portfolio::listing::ProfileLookup;
        // The definitive no-such-symbol shape — the ONLY body that may route a
        // holding terminal as "no resolution".
        let empty: Value = serde_json::from_str("[]").unwrap();
        assert_eq!(profile_identity_from_value(&empty), ProfileLookup::Unresolved);
        // Drifted / malformed-but-valid-JSON shapes are unverified — they proceed
        // degraded, never terminally not-rate a holding.
        for body in ["42", "null", "\"oops\"", "[42]"] {
            let junk: Value = serde_json::from_str(body).unwrap();
            assert!(
                matches!(
                    profile_identity_from_value(&junk),
                    ProfileLookup::Unverified(_)
                ),
                "{body} should read unverified"
            );
        }
        // A present object with blank / missing fields resolves — the fields read
        // `None` (unverifiable upstream), never a missing-listing signal.
        let blank: Value =
            serde_json::from_str(r#"[{"symbol":"AAPL","companyName":"  ","sector":"  "}]"#)
                .unwrap();
        let ProfileLookup::Resolved(identity) = profile_identity_from_value(&blank) else {
            panic!("expected resolved");
        };
        assert_eq!(identity.company_name, None);
        assert_eq!(identity.exchange, None);
        assert_eq!(identity.sector, None);
    }

    #[test]
    fn fund_data_parses_info_and_normalized_weightings() {
        let info = r#"[{"symbol":"VTI","name":"Vanguard Total Stock Market ETF",
            "assetClass":"Equity","expenseRatio":0.03,"aum":4.0e11,"nav":280.5}]"#;
        let sectors = r#"[
            {"sector":"Technology","weightPercentage":"32.5%"},
            {"sector":"Financial Services","weightPercentage":"13.2%"}
        ]"#;
        let countries = r#"[{"country":"United States","weightPercentage":"99.4%"}]"#;
        let profile = r#"[{"symbol":"VTI","isFund":false,
            "description":"Tracks the total US equity market."}]"#;
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: info },
            Canned::Reply { status: 200, headers: vec![], body: profile },
            Canned::Reply { status: 200, headers: vec![], body: sectors },
            Canned::Reply { status: 200, headers: vec![], body: countries },
        ]);
        let fund = source(&server.base_url).fetch_fund_data("VTI");
        assert_eq!(fund.asset_class.as_deref(), Some("Equity"));
        // Percent-unit expense ratio normalizes to a decimal ratio at the seam.
        assert!((fund.expense_ratio.unwrap() - 0.0003).abs() < 1e-12);
        assert_eq!(fund.nav, Some(280.5));
        assert_eq!(fund.profile_is_fund, Some(false));
        assert!(fund
            .profile_description
            .as_deref()
            .is_some_and(|d| d.contains("total US equity")));
        // Percent-string weights normalize to fractions.
        assert!((fund.sector_weights[0].1 - 0.325).abs() < 1e-9);
        assert!((fund.country_weights[0].1 - 0.994).abs() < 1e-9);
        assert!(fund.gaps.is_empty(), "{:?}", fund.gaps);
    }

    #[test]
    fn weights_are_percent_by_wire_contract_in_both_forms() {
        // Every value divides by 100, sparse or full, suffixed or not.
        let numeric: Value = serde_json::from_str(
            r#"[{"sector":"Technology","weightPercentage":32.5},
                {"sector":"Basic Materials","weightPercentage":1.8}]"#,
        )
        .unwrap();
        let out = weights_from_value(&numeric, "sector");
        assert!((out[0].1 - 0.325).abs() < 1e-12, "{:?}", out);
        assert!((out[1].1 - 0.018).abs() < 1e-12, "{:?}", out);
        let suffixed: Value =
            serde_json::from_str(r#"[{"sector":"Technology","weightPercentage":"1.4%"}]"#).unwrap();
        let out = weights_from_value(&suffixed, "sector");
        assert!((out[0].1 - 0.014).abs() < 1e-12, "{:?}", out);
        // The sparse numeric row divides too: 1.4 kept as a fraction priced a fund
        // off a 1.4% sleeve.
        let sparse: Value =
            serde_json::from_str(r#"[{"sector":"Technology","weightPercentage":1.4}]"#).unwrap();
        let out = weights_from_value(&sparse, "sector");
        assert!((out[0].1 - 0.014).abs() < 1e-12, "{:?}", out);
    }

    #[test]
    fn weight_rows_outside_the_finite_0_to_100_range_are_dropped() {
        // `f64::from_str` accepts "NaN" / "inf" and overflows "1e999"; a row is usable
        // only finite and within 0..=100, the endpoints included.
        let body: Value = serde_json::from_str(
            r#"[{"sector":"Technology","weightPercentage":"NaN%"},
                {"sector":"Energy","weightPercentage":"inf"},
                {"sector":"Utilities","weightPercentage":"1e999"},
                {"sector":"Financials","weightPercentage":"-5"},
                {"sector":"Health Care","weightPercentage":250},
                {"sector":"Real Estate","weightPercentage":0},
                {"sector":"Industrials","weightPercentage":"100%"}]"#,
        )
        .unwrap();
        let out = weights_from_value(&body, "sector");
        assert_eq!(
            out,
            vec![("Real Estate".to_string(), 0.0), ("Industrials".to_string(), 1.0)],
            "{out:?}"
        );
    }

    #[test]
    fn a_nan_us_weight_row_no_longer_reads_as_full_us_exposure() {
        // End to end: the shaper drops the NaN United States row, so the US share
        // reads 0% where the NaN row would read 100% and pass the ≥70% guard, and an
        // all-unreadable weighting body reads `malformed`, never a benign empty.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"symbol":"VXUS","name":"Total International Stock","assetClass":"Equity","expenseRatio":0.05}]"#,
            },
            Canned::Reply { status: 200, headers: vec![], body: r#"[{"isFund":false}]"# },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"sector":"Technology","weightPercentage":"NaN%"}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"country":"United States","weightPercentage":"NaN%"},
                          {"country":"Japan","weightPercentage":"40%"}]"#,
            },
        ]);
        let fund = source(&server.base_url).with_context(ctx).fetch_fund_data("VXUS");
        assert!(fund.sector_weights.is_empty(), "{:?}", fund.sector_weights);
        assert_eq!(fund.country_weights, vec![("Japan".to_string(), 0.40)]);
        let us = crate::portfolio::fund::classify(&fund).us_share;
        assert_eq!(us, Some(0.0), "the NaN row is not a weight: {us:?}");
        assert!(
            fund.gaps
                .iter()
                .any(|g| g.starts_with(FUND_SECTOR_WEIGHTS_GAP_PREFIX) && g.contains("were malformed")),
            "{:?}",
            fund.gaps
        );
        assert!(
            !fund.gaps.iter().any(|g| g.starts_with(FUND_COUNTRY_WEIGHTS_GAP_PREFIX)),
            "one readable row is a served set: {:?}",
            fund.gaps
        );
        let finished = finished_rows(&rec);
        let sectors = finished
            .iter()
            .find(|r| r.0 == "fund-sectors")
            .expect("the sector-weightings row finished");
        assert_eq!(sectors.1, "malformed", "{sectors:?}");
        assert!(
            sectors.2.as_deref().is_some_and(|d| d.contains("no served row was readable")),
            "{sectors:?}"
        );
        let countries = finished
            .iter()
            .find(|r| r.0 == "fund-countries")
            .expect("the country-weightings row finished");
        assert_eq!(countries.1, "ok", "a partially dropped body keeps its ok row: {countries:?}");
    }

    #[test]
    fn fund_info_blank_strings_normalize_to_none() {
        // Blank reads as absent: the sweep's comparability gates key on `is_some()`,
        // and a blank name would fabricate an overlay clear.
        let info = r#"[{"symbol":"VTI","name":"   ","assetClass":"","expenseRatio":0.03}]"#;
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: info },
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
        ]);
        let fund = source(&server.base_url).fetch_fund_data("VTI");
        assert_eq!(fund.name, None);
        assert_eq!(fund.asset_class, None);
    }

    #[test]
    fn fund_data_records_gaps_per_failed_endpoint() {
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 402, headers: vec![], body: "premium" },
            Canned::Reply { status: 402, headers: vec![], body: "premium" },
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            Canned::Reply { status: 500, headers: vec![], body: "oops" },
        ]);
        let fund = source(&server.base_url).fetch_fund_data("VTI");
        assert!(fund.asset_class.is_none());
        assert_eq!(fund.gaps.len(), 4, "{:?}", fund.gaps);
        // The failed profile leg names what it cost: closed-end detection.
        assert!(
            fund.gaps
                .iter()
                .any(|g| g.starts_with(FUND_PROFILE_GAP_PREFIX)
                    && g.contains("closed-end detection cannot run")),
            "{:?}",
            fund.gaps
        );
    }

    #[test]
    fn fund_profile_shapes_all_leave_a_detection_trail() {
        // Every profile answer that leaves closed-end detection unable to run must say
        // so on the record; the one silent answer is a definitive `isFund: false`.
        let case = |profile_body: &'static str| {
            let server = MockHttp::serve(vec![
                Canned::Reply { status: 200, headers: vec![], body: "[]" },
                Canned::Reply { status: 200, headers: vec![], body: profile_body },
                Canned::Reply { status: 200, headers: vec![], body: "[]" },
                Canned::Reply { status: 200, headers: vec![], body: "[]" },
            ]);
            source(&server.base_url).fetch_fund_data("PDI")
        };
        let profile_gap_containing = |fund: &crate::portfolio::fund::FundData, needle: &str| {
            fund.gaps
                .iter()
                .any(|g| g.starts_with(FUND_PROFILE_GAP_PREFIX) && g.contains(needle))
        };

        let empty = case("[]");
        assert!(profile_gap_containing(&empty, "was empty"), "{:?}", empty.gaps);
        let fieldless = case("[{}]");
        assert!(profile_gap_containing(&fieldless, "was malformed"), "{:?}", fieldless.gaps);
        let flag_drift = case(r#"[{"isFund":"yes","description":"a closed-end fund"}]"#);
        assert!(profile_gap_containing(&flag_drift, "was malformed"), "{:?}", flag_drift.gaps);
        assert_eq!(flag_drift.profile_is_fund, None, "a drifted flag never fills");
        let flagged_no_text = case(r#"[{"isFund":true}]"#);
        assert!(
            profile_gap_containing(&flagged_no_text, "carried no description"),
            "{:?}",
            flagged_no_text.gaps
        );
        assert_eq!(flagged_no_text.profile_is_fund, Some(true));
        let definitive_not_fund = case(r#"[{"isFund":false}]"#);
        assert!(
            !definitive_not_fund.gaps.iter().any(|g| g.starts_with(FUND_PROFILE_GAP_PREFIX)),
            "isFund:false is a definitive answer, not a gap: {:?}",
            definitive_not_fund.gaps
        );
        assert_eq!(definitive_not_fund.profile_is_fund, Some(false));
    }

    #[test]
    fn the_sweep_fund_refresh_never_fetches_the_profile() {
        // Exactly three requests land (info, sectors, countries). A fourth would shift
        // the served bodies one leg late and fail the countries call on the exhausted
        // mock, so the two clean "were empty" weighting gaps pin the request count.
        let info = r#"[{"symbol":"VTI","name":"Vanguard Total Stock Market ETF",
            "assetClass":"Equity","expenseRatio":0.03}]"#;
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: info },
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
        ]);
        let fund = source(&server.base_url).fetch_fund_refresh_data("VTI");
        assert_eq!(fund.asset_class.as_deref(), Some("Equity"));
        assert_eq!(fund.profile_is_fund, None);
        assert_eq!(fund.profile_description, None);
        assert_eq!(fund.gaps.len(), 2, "{:?}", fund.gaps);
        assert!(
            fund.gaps.iter().all(|g| g.contains("were empty")),
            "only the served-empty weighting gaps — no profile or transport gap: {:?}",
            fund.gaps
        );
    }

    #[test]
    fn fund_sector_pe_rows_hold_the_sibling_adapters_integrity_contract() {
        // A `pe` is usable only finite and inside (0, SECTOR_PE_MAX], the ceiling
        // itself kept; a string P/E still parses.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let in_band = r#"[
          {"date":"2026-07-15","sector":"Technology","exchange":"NYSE","pe":30.1},
          {"date":"2026-07-15","sector":"Energy","exchange":"NYSE","pe":"11.4"},
          {"date":"2026-07-15","sector":"Utilities","exchange":"NYSE","pe":100.0},
          {"date":"2026-07-15","sector":"Software","exchange":"NYSE","pe":461.0},
          {"date":"2026-07-15","sector":"Financials","exchange":"NYSE","pe":0.0},
          {"date":"2026-07-15","sector":"Materials","exchange":"NYSE","pe":-3.2},
          {"date":"2026-07-15","sector":"Health Care","exchange":"NYSE","pe":"NaN"},
          {"date":"2026-07-15","sector":"Broken","exchange":"NYSE"}
        ]"#;
        // An off-board row or an absent exchange reads the whole body malformed, the
        // served board named, the Ok(vec![]) return keeping the walk-back unchanged.
        let off_board = r#"[
          {"date":"2026-07-15","sector":"Technology","exchange":"NYSE","pe":30.1},
          {"date":"2026-07-15","sector":"Energy","exchange":"NASDAQ","pe":11.4}
        ]"#;
        let absent_board = r#"[
          {"date":"2026-06-15","sector":"Technology","exchange":"NYSE","pe":30.1},
          {"date":"2026-03-16","sector":"Technology","pe":29.5}
        ]"#;
        let all_out_of_band = r#"[
          {"date":"2026-06-15","sector":"Technology","exchange":"NYSE","pe":461.0},
          {"date":"2026-03-16","sector":"Technology","exchange":"NYSE","pe":0.0}
        ]"#;
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: in_band },
            Canned::Reply { status: 200, headers: vec![], body: off_board },
            Canned::Reply { status: 200, headers: vec![], body: absent_board },
            Canned::Reply { status: 200, headers: vec![], body: all_out_of_band },
        ]);
        let source = source(&server.base_url).with_context(ctx);

        let rows = source.fetch_sector_pe_snapshot("NYSE", "2026-07-15").unwrap();
        let kept: Vec<(&str, f64)> = rows.iter().map(|r| (r.sector.as_str(), r.pe)).collect();
        assert_eq!(
            kept,
            vec![("Technology", 30.1), ("Energy", 11.4), ("Utilities", 100.0)],
            "{rows:?}"
        );
        assert!(rows.iter().all(|r| r.exchange == "NYSE"), "{rows:?}");

        let rows = source.fetch_sector_pe_snapshot("NYSE", "2026-07-15").unwrap();
        assert!(rows.is_empty(), "an off-board body serves no print: {rows:?}");
        let rows = source
            .fetch_historical_sector_pe("Technology", "NYSE", "2022-01-01", "2026-07-15")
            .unwrap();
        assert!(rows.is_empty(), "an exchange-less row reads the body malformed: {rows:?}");
        let rows = source
            .fetch_historical_sector_pe("Technology", "NYSE", "2022-01-01", "2026-07-15")
            .unwrap();
        assert!(rows.is_empty(), "{rows:?}");

        let finished = finished_rows(&rec);
        assert_eq!(finished.len(), 4, "{finished:?}");
        assert_eq!(finished[0].1, "ok", "a partially dropped body keeps its ok row: {:?}", finished[0]);
        assert_eq!(finished[1].1, "malformed", "{:?}", finished[1]);
        assert!(
            finished[1].2.as_deref().is_some_and(|d| d.contains("\"NASDAQ\"")
                && d.contains("\"NYSE\" request")
                && d.contains("exchange filter was ignored")),
            "{:?}",
            finished[1]
        );
        assert_eq!(finished[2].1, "malformed", "{:?}", finished[2]);
        assert!(
            finished[2].2.as_deref().is_some_and(|d| d.contains("<absent>")),
            "{:?}",
            finished[2]
        );
        assert_eq!(finished[3].1, "malformed", "{:?}", finished[3]);
        assert!(
            finished[3].2.as_deref().is_some_and(|d| d.contains("no served row was readable")),
            "{:?}",
            finished[3]
        );
    }

    /// The tracker rows a source under `ctx` finished, as (group, status, detail).
    fn finished_rows(
        rec: &crate::progress::RecordingReporter,
    ) -> Vec<(String, String, Option<String>)> {
        rec.messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::RequestFinished {
                    group,
                    status,
                    detail,
                    ..
                } => Some((group, status, detail)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn sector_pe_rows_store_the_canonical_date_and_drop_undatable_rows() {
        // Stored as served, "2026-6-15" sorts after "2026-12-31" under the sampler's
        // lexicographic compare, and a dateless row reads as `""`, before every sample
        // date, holding a history slot wherever no dated print qualified.
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let history = r#"[
          {"date":"2026-6-15","sector":"Technology","exchange":"NYSE","pe":30.1},
          {"date":"2026-03-16","sector":"Technology","exchange":"NYSE","pe":29.5},
          {"sector":"Technology","exchange":"NYSE","pe":28.0},
          {"date":"Q4 2025","sector":"Technology","exchange":"NYSE","pe":27.0},
          {"date":"2025-06-31","sector":"Technology","exchange":"NYSE","pe":26.0}
        ]"#;
        let snapshot = r#"[
          {"date":"2026-7-15","sector":"Technology","exchange":"NYSE","pe":30.1},
          {"sector":"Energy","exchange":"NYSE","pe":11.4}
        ]"#;
        let undatable = r#"[
          {"sector":"Technology","exchange":"NYSE","pe":28.0},
          {"date":"soon","sector":"Energy","exchange":"NYSE","pe":11.4}
        ]"#;
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: history },
            Canned::Reply { status: 200, headers: vec![], body: snapshot },
            Canned::Reply { status: 200, headers: vec![], body: undatable },
            Canned::Reply { status: 200, headers: vec![], body: undatable },
        ]);
        let source = source(&server.base_url).with_context(ctx);

        let rows = source
            .fetch_historical_sector_pe("Technology", "NYSE", "2022-01-01", "2026-07-15")
            .unwrap();
        let dates: Vec<&str> = rows.iter().map(|r| r.date.as_str()).collect();
        assert_eq!(
            dates,
            vec!["2026-06-15", "2026-03-16"],
            "canonical render kept, the dateless / non-date / impossible-date rows dropped: {rows:?}"
        );
        let rows = source.fetch_sector_pe_snapshot("NYSE", "2026-07-15").unwrap();
        assert_eq!(rows.len(), 1, "the snapshot drops its dateless row too: {rows:?}");
        assert_eq!(rows[0].date, "2026-07-15");

        // A served array with no readable row is `malformed` on both endpoints, never a
        // benign `empty`, while the `Ok(vec![])` return keeps the walk-back unchanged.
        let rows = source
            .fetch_historical_sector_pe("Technology", "NYSE", "2022-01-01", "2026-07-15")
            .unwrap();
        assert!(rows.is_empty(), "{rows:?}");
        let rows = source.fetch_sector_pe_snapshot("NYSE", "2026-07-15").unwrap();
        assert!(rows.is_empty(), "{rows:?}");
        let finished = finished_rows(&rec);
        assert_eq!(finished.len(), 4, "{finished:?}");
        assert_eq!(finished[0].1, "ok", "{:?}", finished[0]);
        assert_eq!(finished[1].1, "ok", "{:?}", finished[1]);
        for row in &finished[2..] {
            assert_eq!(row.1, "malformed", "{row:?}");
            assert!(
                row.2
                    .as_deref()
                    .is_some_and(|d| d.contains("no served row was readable")),
                "{row:?}"
            );
        }
        assert_eq!(finished[2].0, "sector-pe-history");
        assert_eq!(finished[3].0, "sector-pe");
    }
    #[test]
    fn statement_currency_guards_block_foreign_unknown_and_mixed_surfaces() {
        use crate::portfolio::{dossier, engine};
        for currency in [Some("USD"), Some("TWD"), None] {
            let scale = if currency == Some("TWD") { 30.0 } else { 1.0 };
            let rows: Vec<Value> = ["2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30"]
                .iter()
                .map(|date| {
                    serde_json::json!({
                        "date": date, "reportedCurrency": currency,
                        "revenue": 2.5e9 * scale, "netIncome": 0.25e9 * scale,
                        "grossProfit": 1.25e9 * scale, "epsDiluted": 1.25 * scale,
                        "weightedAverageShsOutDil": 200e6
                    })
                })
                .collect();
            let body = serde_json::to_string(&rows).unwrap();
            let server = MockHttp::serve(vec![Canned::Reply {
                status: 200,
                headers: vec![],
                body: Box::leak(body.into_boxed_str()),
            }]);
            let mut fin = engine::CompanyFinancials {
                symbol: "UNITTEST".into(),
                current_price: Some(100.0),
                market_cap: Some(20e9),
                ..Default::default()
            };
            fin.quarterly_income = source(&server.base_url).fetch_quarterly_income(
                "UNITTEST",
                &mut fin.gaps,
                &mut fin.unit_issues,
            );
            let adopted = dossier::apply_ttm_statement_basis(&mut fin);
            let fin = dossier::merge_financials(fin, &crate::sec::CompanyFacts::default(), adopted);
            if currency == Some("USD") {
                assert_eq!(fin.pe_ratio, Some(20.0));
                assert!(fin.unit_issues.is_empty());
            } else {
                assert!(fin.quarterly_income.is_empty());
                assert_eq!(fin.pe_ratio, None);
                assert_eq!(fin.unit_issues[0].reported_currency.as_deref(), currency);
                assert!(
                    matches!(engine::analyze(&fin, &engine::RateAnchors::default(), Default::default()), engine::EngineVerdict::InsufficientEvidence(reason) if reason.contains("unsupported financial units"))
                );
            }
        }
        for foreign in [r#""CAD""#, "null"] {
            let body = format!(
                r#"[{{"date":"2026-06-30","reportedCurrency":"USD","freeCashFlow":10}},{{"date":"2026-03-31","reportedCurrency":{foreign},"freeCashFlow":300}}]"#
            );
            let balance = format!(
                r#"[{{"reportedCurrency":{foreign},"totalDebt":300,"totalStockholdersEquity":600}}]"#
            );
            let server = MockHttp::serve(vec![
                Canned::Reply {
                    status: 200,
                    headers: vec![],
                    body: Box::leak(body.into_boxed_str()),
                },
                Canned::Reply {
                    status: 200,
                    headers: vec![],
                    body: Box::leak(balance.into_boxed_str()),
                },
            ]);
            let src = source(&server.base_url);
            let mut gaps = vec![];
            let mut issues = vec![];
            assert!(src
                .fetch_quarterly_cash_flow("UNITTEST", &mut gaps, &mut issues)
                .is_empty());
            assert!(src
                .fetch_balance_sheet("UNITTEST", &mut gaps, &mut issues)
                .is_empty());
            assert_eq!(issues.len(), 2);
        }
    }

    // ---- The per-holding evidence surface (`crate::portfolio::evidence`) ----

    fn body(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_ttm_ratio_pair_merges_into_the_eight_lines() {
        let ratios = ttm_ratios_from_value(&body(
            r#"[{"symbol":"AAPL","priceToEarningsRatioTTM":32.89,"priceToBookRatioTTM":47.37}]"#,
        ))
        .unwrap();
        let metrics = ttm_key_metrics_from_value(&body(
            r#"[{"evToEBITDATTM":23.42,"evToSalesTTM":8.13,"freeCashFlowYieldTTM":0.0312,
                "returnOnInvestedCapitalTTM":0.452,"returnOnEquityTTM":1.453,"netDebtToEBITDATTM":0.484}]"#,
        ))
        .unwrap();
        let lines = merge_ratio_lines(ratios, metrics);
        assert_eq!(lines.pe, Some(32.89));
        assert_eq!(lines.pb, Some(47.37));
        assert_eq!(lines.ev_to_ebitda, Some(23.42));
        assert_eq!(lines.fcf_yield, Some(0.0312));
        assert_eq!(lines.net_debt_to_ebitda, Some(0.484));
        // One pull gapped: its half stays None, the other half lands.
        let half = merge_ratio_lines(
            None,
            ttm_key_metrics_from_value(&body(r#"[{"evToSalesTTM":8.13}]"#)).unwrap(),
        );
        assert_eq!(half.pe, None);
        assert_eq!(half.ev_to_sales, Some(8.13));
        assert!(!half.is_empty());
        // An empty array is the honest empty; a non-object row and a bare
        // string are drift.
        assert_eq!(ttm_ratios_from_value(&body("[]")).unwrap(), None);
        assert!(ttm_ratios_from_value(&body(r#""nope""#)).is_err());
        assert!(ttm_ratios_from_value(&body("[1]")).is_err());
    }

    #[test]
    fn the_latest_row_wins_for_owner_earnings_enterprise_value_and_insider_statistics() {
        let oe = owner_earnings_from_value(&body(
            r#"[{"fiscalYear":"2024","period":"Q4","date":"2024-09-28","ownersEarnings":2.0e10,"ownersEarningsPerShare":1.3},
                {"fiscalYear":2025,"period":"Q1","date":"2024-12-28","ownersEarnings":27655035250,"ownersEarningsPerShare":1.83}]"#,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(oe.period_end, "2024-12-28");
        assert_eq!(
            oe.period.as_deref(),
            Some("FY2025 Q1"),
            "a numeric fiscal year reads too"
        );
        assert_eq!(oe.per_share, Some(1.83));
        let ev = enterprise_value_from_value(&body(
            r#"[{"date":"2024-09-28","enterpriseValue":3571846329570,"marketCapitalization":3495160329570,
                "addTotalDebt":106629000000,"minusCashAndCashEquivalents":29943000000}]"#,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(ev.date, "2024-09-28");
        assert_eq!(ev.total_debt, Some(106_629_000_000.0));
        let st = insider_statistics_from_value(&body(
            r#"[{"year":2026,"quarter":1,"acquiredTransactions":2,"disposedTransactions":9},
                {"year":2026,"quarter":2,"acquiredTransactions":5,"disposedTransactions":35,"totalAcquired":272855,"totalDisposed":880558}]"#,
        ))
        .unwrap()
        .unwrap();
        assert_eq!((st.year, st.quarter), (Some(2026), Some(2)));
        assert_eq!(st.disposed_transactions, Some(35));
        // A body of only unreadable rows is drift, never an honest empty.
        assert!(owner_earnings_from_value(&body(r#"[{"ownersEarnings":1.0}]"#)).is_err());
        assert!(enterprise_value_from_value(&body("[]")).unwrap().is_none());
    }

    #[test]
    fn the_street_rows_read_the_reference_bodies() {
        let t = price_target_consensus_from_value(&body(
            r#"[{"symbol":"AAPL","targetHigh":400,"targetLow":253,"targetConsensus":323.82,"targetMedian":325}]"#,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(
            (t.high, t.low, t.median, t.consensus),
            (Some(400.0), Some(253.0), Some(325.0), Some(323.82))
        );
        let tr = price_target_trend_from_value(&body(
            r#"[{"lastMonthCount":3,"lastMonthAvgPriceTarget":380,"lastQuarterCount":10,"lastQuarterAvgPriceTarget":322.6,
                "lastYearCount":58,"lastYearAvgPriceTarget":293.43}]"#,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(tr.last_month.count, Some(3));
        assert_eq!(tr.last_year.average, Some(293.43));
        let g = grades_consensus_from_value(&body(
            r#"[{"strongBuy":1,"buy":69,"hold":33,"sell":7,"strongSell":0,"consensus":"Buy"}]"#,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(g.buy, Some(69));
        assert_eq!(g.strong_sell, Some(0));
        assert_eq!(g.consensus.as_deref(), Some("Buy"));
        let r = ratings_snapshot_from_value(&body(
            r#"[{"rating":"B","overallScore":3,"discountedCashFlowScore":3,"returnOnEquityScore":5,"returnOnAssetsScore":5,
                "debtToEquityScore":1,"priceToEarningsScore":2,"priceToBookScore":1}]"#,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(r.rating.as_deref(), Some("B"));
        assert_eq!(r.price_to_book, Some(1));
        let f = shares_float_from_value(&body(
            r#"[{"date":"2026-06-05 08:13:10","freeFloat":99.83,"floatShares":14662534368,"outstandingShares":14687356000}]"#,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(f.free_float_percent, Some(99.83));
        assert_eq!(f.date.as_deref(), Some("2026-06-05 08:13:10"));
        assert_eq!(f.float_shares, Some(14_662_534_368.0));
    }

    #[test]
    fn the_dated_lists_window_sort_and_cap_at_the_ruled_counts() {
        // Rating actions: eleven in-window rows served oldest first plus one
        // outside the window — ten kept, newest first.
        let mut rows: Vec<String> = (1..=11)
            .map(|m| format!(r#"{{"date":"2026-{m:02}-01","gradingCompany":"Firm {m}","newGrade":"Buy","action":"upgrade"}}"#))
            .collect();
        rows.push(r#"{"date":"2025-01-15","gradingCompany":"Old","newGrade":"Sell","action":"downgrade"}"#.into());
        rows.push(r#"{"gradingCompany":"Dateless"}"#.into());
        let actions =
            rating_actions_from_value(&body(&format!("[{}]", rows.join(","))), "2025-10-08")
                .unwrap();
        assert_eq!(actions.len(), RATING_ACTIONS_COUNT);
        assert_eq!(actions[0].date, "2026-11-01");
        assert_eq!(actions[9].date, "2026-02-01");
        assert!(actions
            .iter()
            .all(|a| a.firm != "Old" && a.firm != "Dateless"));
        // Insider trades: the window keys on the transaction date; a row
        // without a reporting name is unreadable.
        let trades = insider_trades_from_value(
            &body(
                r#"[{"transactionDate":"2026-03-01","reportingName":"EARLY","transactionType":"S-Sale"},
                    {"transactionDate":"2026-06-05","filingDate":"2026-06-06","reportingName":"BOLDUC JOHN","typeOfOwner":"director",
                     "transactionType":"P-Purchase","securitiesTransacted":3570,"price":6.77},
                    {"transactionDate":"2026-09-01","reportingName":"LATE","transactionType":"A-Award"},
                    {"transactionDate":"2026-09-02"}]"#,
            ),
            "2026-04-08",
        )
        .unwrap();
        assert_eq!(
            trades.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["LATE", "BOLDUC JOHN"]
        );
        assert_eq!(trades[1].shares, Some(3570.0));
        assert_eq!(trades[1].filing_date.as_deref(), Some("2026-06-06"));
        // Congressional: both chambers merged newest first, the Senate ahead
        // on one date, the window on the transaction date, the cap at ten.
        let senate = congressional_trades_from_value(
            &body(
                r#"[{"transactionDate":"2026-04-17","disclosureDate":"2026-05-07","firstName":"Shelley","lastName":"Moore Capito",
                     "owner":"Spouse","type":"Sale","amount":"$1,001 - $15,000"},
                    {"transactionDate":"2025-02-01","office":"Someone Old","type":"Purchase"}]"#,
            ),
            Chamber::Senate,
        )
        .unwrap();
        let house = congressional_trades_from_value(
            &body(r#"[{"transactionDate":"2026-04-17","office":"Tim Walberg","owner":"Joint","type":"Purchase","amount":"$15,001 - $50,000"}]"#),
            Chamber::House,
        )
        .unwrap();
        assert_eq!(
            senate.len(),
            2,
            "the chamber parser keeps every readable row"
        );
        let merged = merge_congressional_trades(senate, house, "2025-10-08");
        assert_eq!(merged.len(), 2);
        assert_eq!(
            (merged[0].chamber, merged[0].name.as_str()),
            (Chamber::Senate, "Shelley Moore Capito")
        );
        assert_eq!(
            (merged[1].chamber, merged[1].name.as_str()),
            (Chamber::House, "Tim Walberg")
        );
        // Segments: the latest two fiscal years, each year's segments largest
        // first; a row without a `data` object is unreadable.
        let years = segment_years_from_value(
            &body(
                r#"[{"fiscalYear":2022,"period":"FY","date":"2022-09-24","data":{"Mac":40.0e9}},
                    {"fiscalYear":2024,"period":"FY","date":"2024-09-28","data":{"Mac":29984000000,"iPhone":201183000000,"Service":96169000000}},
                    {"fiscalYear":2023,"period":"FY","date":"2023-09-30","data":{"Mac":29.4e9,"iPhone":200.6e9}},
                    {"fiscalYear":2021,"period":"FY","date":"2021-09-25"}]"#,
            ),
            "revenue-product-segmentation",
        )
        .unwrap();
        assert_eq!(
            years.iter().map(|y| y.fiscal_year).collect::<Vec<_>>(),
            [Some(2024), Some(2023)]
        );
        assert_eq!(
            years[0]
                .segments
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>(),
            ["iPhone", "Service", "Mac"]
        );
        // Splits: newest first; a zero leg is unreadable.
        let splits = splits_from_value(&body(
            r#"[{"date":"2014-06-09","numerator":7,"denominator":1},{"date":"2020-08-31","numerator":4,"denominator":1},
                {"date":"2000-06-21","numerator":2,"denominator":0}]"#,
        ))
        .unwrap();
        assert_eq!(
            splits.iter().map(|s| s.date.as_str()).collect::<Vec<_>>(),
            ["2020-08-31", "2014-06-09"]
        );
        // M&A rows: the transaction date keys the row; the names and symbols
        // ride as served.
        let deals = ma_deals_from_value(&body(
            r#"[{"symbol":"GNL-PE","companyName":"Global Net Lease, Inc.","targetedCompanyName":"Modiv Industrial, Inc.",
                 "targetedSymbol":"MDV","transactionDate":"2026-06-01","link":"https://www.sec.gov/x"},{"symbol":"NODATE"}]"#,
        ))
        .unwrap();
        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].target_symbol.as_deref(), Some("MDV"));
        assert_eq!(
            deals[0].acquirer_name.as_deref(),
            Some("Global Net Lease, Inc.")
        );
    }

    #[test]
    fn the_dividend_read_keeps_the_latest_payments_beside_the_trailing_sum() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let pull = dividends_from_value(
            &body(
                r#"[{"date":"2026-11-10","dividend":0.27,"paymentDate":"2026-11-13"},
                    {"date":"2026-08-11","dividend":0.26,"adjDividend":0.26,"paymentDate":"2026-08-14"},
                    {"date":"2026-05-12","dividend":0.26},
                    {"date":"2026-02-10","dividend":0.25},
                    {"date":"2025-11-11","dividend":0.25},
                    {"date":"2025-08-12","dividend":0.25},
                    {"date":"2024-08-12","dividend":null}]"#,
            ),
            today,
        )
        .unwrap();
        // The trailing sum: the four in-window payments (the future
        // declaration and the year-old one out).
        assert!((pull.ttm.unwrap() - 1.02).abs() < 1e-9, "{:?}", pull.ttm);
        // The latest four on or before today, newest first, the payment date
        // where served; the older amount-less row simply does not list.
        assert_eq!(
            pull.recent
                .iter()
                .map(|r| (r.date.as_str(), r.amount))
                .collect::<Vec<_>>(),
            [
                ("2026-08-11", 0.26),
                ("2026-05-12", 0.26),
                ("2026-02-10", 0.25),
                ("2025-11-11", 0.25)
            ]
        );
        assert_eq!(pull.recent[0].payment_date.as_deref(), Some("2026-08-14"));
        assert_eq!(pull.recent[1].payment_date, None);
        // A non-payer: no payment at all, no sum, no rows, no error.
        let none = dividends_from_value(&body("[]"), today).unwrap();
        assert_eq!(none, DividendPull::default());
    }

    #[test]
    fn company_evidence_rides_one_tracker_row_per_leg_and_degrades_leg_by_leg() {
        // The eighteen requests in issue order; two degrade (a premium gate on
        // the key metrics, an unreadable price-target body) and tag the record
        // while every other leg lands.
        let ok = |body: &'static str| Canned::Reply {
            status: 200,
            headers: vec![],
            body,
        };
        let server = MockHttp::serve(vec![
            ok(r#"[{"priceToEarningsRatioTTM":32.89,"priceToBookRatioTTM":47.37}]"#),
            Canned::Reply {
                status: 402,
                headers: vec![],
                body: "Payment Required",
            },
            ok(
                r#"[{"fiscalYear":"2025","period":"Q1","date":"2024-12-28","ownersEarnings":27655035250,"ownersEarningsPerShare":1.83}]"#,
            ),
            ok("[]"),
            ok(r#""nope""#),
            ok(r#"[{"lastMonthCount":3,"lastMonthAvgPriceTarget":380}]"#),
            ok(
                r#"[{"date":"2026-05-26","gradingCompany":"B of A Securities","previousGrade":"Buy","newGrade":"Buy","action":"maintain"}]"#,
            ),
            ok(r#"[{"strongBuy":1,"buy":69,"hold":33,"sell":7,"strongSell":0,"consensus":"Buy"}]"#),
            ok(r#"[{"rating":"B","overallScore":3}]"#),
            ok(
                r#"[{"transactionDate":"2026-06-05","filingDate":"2026-06-05","reportingName":"BOLDUC JOHN","typeOfOwner":"director","transactionType":"P-Purchase","securitiesTransacted":3570,"price":6.77}]"#,
            ),
            ok(r#"[{"year":2026,"quarter":2,"acquiredTransactions":5,"disposedTransactions":35}]"#),
            ok(
                r#"[{"transactionDate":"2026-04-17","disclosureDate":"2026-05-07","firstName":"Shelley","lastName":"Moore Capito","owner":"Spouse","type":"Sale","amount":"$1,001 - $15,000"}]"#,
            ),
            ok("[]"),
            ok(
                r#"[{"date":"2026-06-05 08:13:10","freeFloat":99.83,"floatShares":14662534368,"outstandingShares":14687356000}]"#,
            ),
            ok(
                r#"[{"fiscalYear":2024,"period":"FY","date":"2024-09-28","data":{"Mac":29984000000,"iPhone":201183000000}}]"#,
            ),
            ok(
                r#"[{"fiscalYear":2024,"period":"FY","date":"2024-09-28","data":{"Americas Segment":167045000000}}]"#,
            ),
            ok(r#"[{"date":"2020-08-31","numerator":4,"denominator":1}]"#),
            ok(
                r#"[{"date":"2026-07-22","epsActual":1.1,"epsEstimated":1.0,"revenueActual":9.0e10}]"#,
            ),
        ]);
        let src = source(&server.base_url);
        let session = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let e = src.fetch_company_evidence("AAPL", session);
        assert_eq!(e.symbol, "AAPL");
        assert_eq!(e.ratios.pe, Some(32.89));
        assert_eq!(e.ratios.ev_to_ebitda, None, "the gated half stays empty");
        assert_eq!(
            e.owner_earnings.as_ref().map(|o| o.period_end.as_str()),
            Some("2024-12-28")
        );
        assert!(e.enterprise_value.is_none(), "an honest empty");
        assert!(e.price_target.is_none(), "an unreadable body");
        assert_eq!(
            e.price_target_trend
                .as_ref()
                .and_then(|t| t.last_month.count),
            Some(3)
        );
        assert_eq!(e.rating_actions.len(), 1);
        assert_eq!(e.grades_consensus.as_ref().and_then(|g| g.buy), Some(69));
        assert_eq!(
            e.ratings_snapshot
                .as_ref()
                .and_then(|r| r.rating.as_deref()),
            Some("B")
        );
        assert_eq!(e.insider_trades.len(), 1);
        assert_eq!(
            e.insider_statistics.as_ref().and_then(|s| s.quarter),
            Some(2)
        );
        assert_eq!(e.congressional_trades.len(), 1);
        assert_eq!(e.congressional_trades[0].name, "Shelley Moore Capito");
        assert_eq!(
            e.float.as_ref().and_then(|f| f.free_float_percent),
            Some(99.83)
        );
        assert_eq!(e.product_segments[0].segments[0].0, "iPhone");
        assert_eq!(e.geographic_segments.len(), 1);
        assert_eq!(e.splits[0].numerator, 4.0);
        assert_eq!(e.earnings.len(), 1);
        assert_eq!(e.gaps.len(), 2, "{:?}", e.gaps);
        assert!(
            e.gaps[0].contains("key-metrics-ttm unavailable"),
            "{:?}",
            e.gaps
        );
        assert!(
            e.gaps[1].contains("price-target-consensus malformed"),
            "{:?}",
            e.gaps
        );
        assert_eq!(
            server.request_paths(),
            vec![
                "/ratios-ttm",
                "/key-metrics-ttm",
                "/owner-earnings",
                "/enterprise-values",
                "/price-target-consensus",
                "/price-target-summary",
                "/grades",
                "/grades-consensus",
                "/ratings-snapshot",
                "/insider-trading/search",
                "/insider-trading/statistics",
                "/senate-trades",
                "/house-trades",
                "/shares-float",
                "/revenue-product-segmentation",
                "/revenue-geographic-segmentation",
                "/splits",
                "/earnings",
            ]
        );
    }

    fn ma_page(n: usize, date: &str) -> &'static str {
        let rows: Vec<String> = (0..n)
            .map(|i| {
                format!(r#"{{"symbol":"S{i}","targetedSymbol":"T{i}","transactionDate":"{date}"}}"#)
            })
            .collect();
        Box::leak(format!("[{}]", rows.join(",")).into_boxed_str())
    }

    #[test]
    fn the_ma_walk_stops_at_a_short_page_the_window_boundary_a_failure_and_the_cap() {
        use crate::portfolio::evidence::{MA_PAGE_CAP, MA_PAGE_LIMIT};
        let ok = |body: &'static str| Canned::Reply {
            status: 200,
            headers: vec![],
            body,
        };
        let session = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let full = ma_page(MA_PAGE_LIMIT, "2026-09-01");
        // A short first page ends the walk: one request, no gap.
        let server = MockHttp::serve(vec![ok(
            r#"[{"symbol":"GNL-PE","companyName":"Global Net Lease, Inc.","targetedSymbol":"MDV","targetedCompanyName":"Modiv Industrial, Inc.","transactionDate":"2026-06-01","link":"https://sec.gov/x"}]"#,
        )]);
        let (deals, gap) = source(&server.base_url).fetch_recent_mergers(session);
        assert_eq!(deals.len(), 1);
        assert_eq!(gap, None);
        assert_eq!(server.request_paths(), vec!["/mergers-acquisitions-latest"]);
        // A full page, then a page whose rows leave the window: two requests,
        // the in-window rows kept, the old row dropped, no gap.
        let server = MockHttp::serve(vec![
            ok(full),
            ok(
                r#"[{"symbol":"A","targetedSymbol":"B","transactionDate":"2026-01-02"},{"symbol":"C","targetedSymbol":"D","transactionDate":"2025-01-02"}]"#,
            ),
        ]);
        let (deals, gap) = source(&server.base_url).fetch_recent_mergers(session);
        assert_eq!(deals.len(), MA_PAGE_LIMIT + 1);
        assert_eq!(gap, None);
        assert_eq!(server.request_paths().len(), 2);
        // A full page with one unreadable (dateless) row is still a full page: the
        // walk continues rather than ending on the 99 rows it could read.
        let full99 = ma_page(MA_PAGE_LIMIT - 1, "2026-09-01");
        let mixed: &'static str = Box::leak(
            format!("{},{{\"symbol\":\"NODATE\"}}]", &full99[..full99.len() - 1]).into_boxed_str(),
        );
        let server = MockHttp::serve(vec![
            ok(mixed),
            ok(r#"[{"symbol":"A","targetedSymbol":"B","transactionDate":"2026-01-02"}]"#),
        ]);
        let (deals, gap) = source(&server.base_url).fetch_recent_mergers(session);
        assert_eq!(deals.len(), MA_PAGE_LIMIT, "99 readable rows plus the second page's one");
        assert_eq!(gap, None);
        assert_eq!(server.request_paths().len(), 2, "the mixed page did not end the walk");
        // A failure on a later page keeps the rows gathered and says so; on
        // the first page there is no feed.
        let server = MockHttp::serve(vec![
            ok(full),
            Canned::Reply {
                status: 402,
                headers: vec![],
                body: "Payment Required",
            },
        ]);
        let (deals, gap) = source(&server.base_url).fetch_recent_mergers(session);
        assert_eq!(deals.len(), MA_PAGE_LIMIT);
        assert!(
            gap.as_deref()
                .is_some_and(|g| g.contains("truncated at page 1")),
            "{gap:?}"
        );
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 402,
            headers: vec![],
            body: "Payment Required",
        }]);
        let (deals, gap) = source(&server.base_url).fetch_recent_mergers(session);
        assert!(deals.is_empty());
        assert!(
            gap.as_deref().is_some_and(|g| g.contains("unavailable")),
            "{gap:?}"
        );
        // The cap: full in-window pages to the cap — the rows gathered and a
        // gap naming the cap.
        let server = MockHttp::serve((0..MA_PAGE_CAP).map(|_| ok(full)).collect());
        let (deals, gap) = source(&server.base_url).fetch_recent_mergers(session);
        assert_eq!(deals.len(), MA_PAGE_CAP * MA_PAGE_LIMIT);
        assert!(
            gap.as_deref().is_some_and(|g| g.contains("page cap")),
            "{gap:?}"
        );
        assert_eq!(server.request_paths().len(), MA_PAGE_CAP);
    }
}
