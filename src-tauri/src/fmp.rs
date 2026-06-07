//! Real Financial Modeling Prep adapter for the baseline market-data scan.
//!
//! The first data-source adapter behind the `MarketDataSource` trait
//! (`data_sources`). On FMP's free tier the provider is effectively an *equities*
//! API, so this adapter owns the equity-market half of the Step-6 baseline:
//! the market **indices** (Dow / S&P 500 / Nasdaq / Russell 2000), the **VIX**,
//! **gold** (`GCUSD`, free on the quote endpoint), and **sector performance**, plus
//! each index's **multi-horizon performance** (weekly / MTD / YTD / 52-week range)
//! derived from FMP's free end-of-day history. The
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
//! Degradation policy. The guiding rule: **every failure degrades to a recorded gap, so
//! one flaky symbol or a whole-provider outage never throws away the rest of the scan.**
//! One pure function, `interpret_response`, classifies each response into a
//! [`Disposition`] — either a 2xx value to shape, or a `Gap(reason)` the loop records
//! and steps past:
//! - `OutOfScope` — a 402 (premium) or 404 (not found): FMP explicitly signals this one
//!   symbol is permanently absent. Excluded from the coverage denominator. (A 2xx that
//!   parses but carries *no* rows is instead an `Unavailable` gap — see `fetch_quotes` —
//!   so an empty response for an expected symbol still counts against coverage.)
//! - `Rejected` — auth (401/403) or a 200 `{"Error Message"}` rate-limit / plan body. A
//!   whole-provider condition, so the loop stops calling and records the remaining
//!   symbols as `Rejected` too.
//! - `Unavailable` — a 429 / 5xx that survived the retry layer, or a transport error.
//! - `Malformed` — a request-contract error (400/408/422/other non-2xx), an unparseable
//!   2xx body, or a response that won't shape into the expected array.
//!
//! No floor lives here anymore: a scan that resolves no index quotes returns an empty
//! `indices` group plus its gaps, and the central coverage gate
//! (`pipeline::enforce_coverage`) decides whether that's below the run's floor.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{
    emit_series_row, BaselineMarketData, DataGap, GapReason, GroupKind, IndexPerformance,
    MarketDataSource, Quote, SectorPerformance,
};
use crate::progress::RunContext;

/// FMP's stable single-symbol quote endpoint — the one `connection_test` exercises.
const FMP_QUOTE_URL: &str = "https://financialmodelingprep.com/stable/quote";
/// FMP's sector-performance snapshot endpoint. Requires a `date` query param
/// (a dateless call returns HTTP 400).
const FMP_SECTOR_URL: &str = "https://financialmodelingprep.com/stable/sector-performance-snapshot";

/// Short timeout per request: the baseline scan issues several sequential calls,
/// none of which should park for the model adapter's 120s ceiling.
const FMP_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// How many trading-day candidates back to probe for the most recent sector snapshot.
/// The weekly job fires Sunday 9am, when the latest snapshot is the prior Friday's;
/// `sector_candidate_dates` skips the closed-market weekend without spending a request,
/// so this budget covers weekdays (the holidays that actually need walking back over)
/// rather than being burned on the guaranteed-empty Saturday and Sunday.
const SECTOR_LOOKBACK_WEEKDAYS: usize = 5;

/// FMP's end-of-day historical-price endpoint (light: date + close). One call per
/// index over a trailing ~53-week window backs the multi-horizon `IndexPerformance`
/// (weekly / MTD / YTD / 52-week range) — free on the equities tier (probed live
/// Jun 2026, all four indices + the VIX return 200 with data).
const FMP_EOD_URL: &str = "https://financialmodelingprep.com/stable/historical-price-eod/light";

/// Trailing window requested for the EOD history: ~53 weeks, so the 52-week range and
/// the year-to-date anchor both sit inside the window with margin.
const EOD_LOOKBACK_DAYS: i64 = 371;

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
/// **required**: a quote missing either fails the parse, which the loop records as a
/// `Malformed` gap rather than reaching the model as a false `0.0`. The change
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
/// **required** — a row missing either fails the parse, which `fetch_sectors` records as
/// a `Malformed` gap rather than dropping silently. The snapshot's `date` / `exchange` fields are
/// ignored.
#[derive(Debug, Deserialize)]
struct FmpSectorRaw {
    sector: String,
    #[serde(rename = "averageChange")]
    average_change: f64,
}

/// One row of FMP's EOD light history: the close (`price`) on a `date` (`"YYYY-MM-DD"`).
/// Both are required — a row missing either fails the parse, which the loop records as a
/// `Malformed` gap rather than dropping the row silently. The `symbol` / `volume` fields the endpoint also returns
/// are ignored.
#[derive(Debug, Deserialize)]
struct FmpEodRaw {
    date: String,
    price: f64,
}

/// One FMP response classified into what the loop should do with it — the single place
/// the degradation policy lives, now in terms of [`GapReason`] rather than a fatal
/// `Err`. Either a 2xx value to shape, or a gap the loop records and steps past.
enum Disposition {
    Value(Value),
    Gap(GapReason),
}

/// Interpret one FMP response by the full status × body matrix. Pure and total. Status
/// decides disposition first, with an explicit *skip allowlist* (402/404 → `OutOfScope`),
/// so a non-2xx is never reclassified by its body (a 402 with a JSON error body skips
/// just like a 402 with a plain-text body). Only on a 2xx is the body inspected, where
/// FMP's `{"Error Message"}` rate-limit / plan signal is a `Rejected` gap and an
/// unparseable body a `Malformed` gap — distinct from an empty "no data" array, which
/// parses fine and shapes to zero quotes.
fn interpret_response(status: u16, body: &str) -> Disposition {
    match status {
        200..=299 => match serde_json::from_str::<Value>(body) {
            Ok(value) => {
                if value.get("Error Message").and_then(Value::as_str).is_some() {
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

/// One gap for the `sectors` group, which is a whole-snapshot (no per-series symbols),
/// so it carries a synthetic series id / name rather than one per sector.
fn sector_gap(reason: GapReason) -> DataGap {
    DataGap::new(GroupKind::Sectors, "sector-performance", "Sector Performance", reason)
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

/// The ordered sector-snapshot candidate dates for a run: the most recent weekday on
/// or before `today`, then each prior weekday, up to `lookback` candidates. Weekends
/// are skipped without spending a request — FMP publishes no Saturday or Sunday
/// snapshot — so the lookback budget covers trading-day candidates (the holidays that
/// actually need walking back over) rather than being burned on the weekend. The weekly
/// job fires Sunday, so the old calendar walk spent its first two requests on the
/// guaranteed-empty Sun/Sat every run.
fn sector_candidate_dates(today: NaiveDate, lookback: usize) -> Vec<NaiveDate> {
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

/// Compute one index's multi-horizon performance from its EOD history (newest-first).
/// Returns `None` when the history is too short to anchor even the weekly return.
///
/// Each horizon's baseline is the most recent close on or before the horizon's start
/// date (an "as-of" lookup), so weekends and holidays don't skew the anchor: weekly off
/// 7 days back, month-to-date off the last close of the prior month, year-to-date off
/// the last close of the prior year. The 52-week range is the min/max close over the
/// trailing 365 days. Everything anchors to the latest close's own date — the report's
/// reference close — not the wall-clock run date.
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

    // Weekly is required (it anchors the shortest horizon); without a close a week back
    // there isn't enough history to report this index.
    let weekly_pct = pct(as_of(latest_date - Duration::days(7))?);
    // MTD / YTD soft-degrade to 0.0 when the window doesn't reach back to the anchor
    // (a fresh listing, or a short fetch) rather than dropping the whole index.
    let first_of_month = latest_date.with_day(1)?;
    let mtd_pct = as_of(first_of_month - Duration::days(1))
        .map(pct)
        .unwrap_or(0.0);
    let first_of_year = NaiveDate::from_ymd_opt(latest_date.year(), 1, 1)?;
    let ytd_pct = as_of(first_of_year - Duration::days(1))
        .map(pct)
        .unwrap_or(0.0);

    // 52-week range over the trailing 365 days (latest included).
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

/// Shape a successful EOD light response into one index's performance: parse the rows
/// into dated closes, sort newest-first defensively (FMP returns descending, but the
/// anchors must not depend on it), then compute the horizons. A body that is not the
/// expected array fails the parse; an empty or too-short history yields `None`.
fn eod_to_performance(value: Value, symbol: &str, name: &str) -> Result<Option<IndexPerformance>> {
    let raws: Vec<FmpEodRaw> = serde_json::from_value(value)
        .context("FMP EOD response did not match the expected array shape")?;
    let mut rows: Vec<(NaiveDate, f64)> = Vec::with_capacity(raws.len());
    for r in raws {
        let date = NaiveDate::parse_from_str(r.date.trim(), "%Y-%m-%d").with_context(|| {
            format!("FMP EOD returned an unparseable date {:?} for {symbol}", r.date)
        })?;
        rows.push((date, r.price));
    }
    rows.sort_by_key(|b| std::cmp::Reverse(b.0));
    Ok(index_performance_from_eod(symbol, name, &rows))
}

/// Live FMP adapter behind the `MarketDataSource` trait.
pub struct FmpDataSource {
    api_key: String,
    http: reqwest::blocking::Client,
    /// Run context for live progress + cooperative cancellation. Defaults to a no-op
    /// (tests / offline smokes); the live command path attaches the real one via
    /// [`FmpDataSource::with_context`].
    progress: Arc<RunContext>,
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
            progress: RunContext::noop(),
        })
    }

    /// Attach a live run context so the per-series scan streams a tracker row per
    /// request and stops making requests once a cancel is observed. Without it the
    /// adapter keeps its no-op context.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Resolve the adapter from the environment, for the live smoke and any
    /// caller that bypasses the gate. The execution gate (`config::validate`)
    /// runs ahead of this in the command path.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().fmp_key()?)
    }

    /// GET one FMP endpoint with the key as a query param, returning the status
    /// and raw body for `interpret_response` to judge. A transport error (the
    /// provider is unreachable) returns `Err` to the caller, which records it as an
    /// `Unavailable` gap rather than failing the scan.
    fn get(&self, url: &str, extra: &[(&str, &str)]) -> Result<(u16, String)> {
        let mut query: Vec<(&str, &str)> = vec![("apikey", self.api_key.as_str())];
        query.extend_from_slice(extra);
        crate::http_retry::send_with_retry("FMP", || self.http.get(url).query(&query))
    }

    /// Fetch one quote per symbol, recording a [`DataGap`] in `group` for any that don't
    /// resolve rather than failing the scan. `interpret_response` decides each response;
    /// a `Rejected` (auth / quota) is a whole-provider condition, so the loop stops
    /// calling and records the remaining symbols without hammering. A 2xx that won't
    /// shape into quotes is a `Malformed` gap; an empty "no data" array for an expected
    /// symbol is an `Unavailable` gap (no value this run), so it still counts against
    /// coverage rather than vanishing.
    fn fetch_quotes(
        &self,
        symbols: &[(&str, &str, &str)],
        group: GroupKind,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<Quote> {
        let mut out = Vec::with_capacity(symbols.len());
        let mut rejected = false;
        for (symbol, fallback_name, unit) in symbols {
            // Cancel checkpoint between series: stop hitting the API once a cancel is
            // requested. The series already fetched are kept; the run unwinds at the
            // pipeline's post-baseline checkpoint.
            if self.progress.is_cancelled() {
                break;
            }
            if rejected {
                // No request is made for a short-circuited series, so it gets no
                // tracker row — rows stay one-to-one with actual HTTP calls.
                gaps.push(DataGap::new(group, *symbol, *fallback_name, GapReason::Rejected));
                continue;
            }
            self.progress
                .request_started("FMP", group.as_str(), *symbol, *fallback_name);
            let gaps_before = gaps.len();
            let out_before = out.len();
            let disposition = match self.get(FMP_QUOTE_URL, &[("symbol", symbol)]) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable), // transport — unreachable
            };
            match disposition {
                Disposition::Value(value) => match quotes_from_value(value, fallback_name, unit) {
                    // An empty "no data" 2xx array for an expected symbol is a this-run
                    // absence, not silence: record it so it counts against coverage and
                    // shows in the manifest rather than vanishing from both.
                    Ok(quotes) if quotes.is_empty() => {
                        gaps.push(DataGap::new(group, *symbol, *fallback_name, GapReason::Unavailable))
                    }
                    Ok(quotes) => out.extend(quotes),
                    Err(_) => {
                        gaps.push(DataGap::new(group, *symbol, *fallback_name, GapReason::Malformed))
                    }
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
            );
        }
        out
    }

    /// Fetch the most recent sector-performance snapshot, walking back over weekday
    /// candidates (`sector_candidate_dates` skips the closed-market weekend) to the last
    /// trading day with data (holidays have none). A 404 / empty array means no snapshot
    /// for that date — try the prior weekday; a this-run failure (auth / quota / 5xx /
    /// transport / malformed) records one group-level `sectors` gap and stops walking
    /// back. If no candidate has a snapshot, returns empty with no gap — a quiet window,
    /// not a failure.
    fn fetch_sectors(&self, gaps: &mut Vec<DataGap>) -> Vec<SectorPerformance> {
        let today = Utc::now().date_naive();
        for date in sector_candidate_dates(today, SECTOR_LOOKBACK_WEEKDAYS) {
            // Cancel checkpoint: the date-walk can fire several probes, so stop here
            // rather than working through them after a cancel during an earlier group.
            if self.progress.is_cancelled() {
                return Vec::new();
            }
            // Each date probe is a real HTTP request, so each gets its own tracker row.
            let date_str = date.format("%Y-%m-%d").to_string();
            let name = format!("Sector performance ({date_str})");
            let group = GroupKind::Sectors.as_str();
            self.progress
                .request_started("FMP", group, date_str.as_str(), name.as_str());
            let disposition = match self.get(FMP_SECTOR_URL, &[("date", date_str.as_str())]) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            let finish = |status: &str| {
                self.progress
                    .request_finished("FMP", group, date_str.as_str(), name.as_str(), status, None)
            };
            match disposition {
                Disposition::Value(value) => match sectors_from_value(value) {
                    Ok(sectors) if !sectors.is_empty() => {
                        finish("ok");
                        return sectors;
                    }
                    // An empty array — no snapshot for this weekday; try the prior one.
                    Ok(_) => finish("empty"),
                    Err(_) => {
                        finish("malformed");
                        gaps.push(sector_gap(GapReason::Malformed));
                        return Vec::new();
                    }
                },
                // A legitimate per-date absence (404) — try the prior weekday.
                Disposition::Gap(GapReason::OutOfScope) => finish("out-of-scope"),
                // Auth / quota / 5xx / transport — the snapshot is unavailable this run.
                Disposition::Gap(reason) => {
                    finish(reason.as_str());
                    gaps.push(sector_gap(reason));
                    return Vec::new();
                }
            }
        }
        Vec::new()
    }

    /// Fetch each index's EOD history and shape it into multi-horizon performance, one
    /// `historical-price-eod/light` call per index over the trailing window. Additive
    /// enrichment over the required daily `indices` quotes, so a permanent absence
    /// (402 / 404) or a history too short to anchor is skipped *silently* — the daily
    /// quote already covers that symbol and a recurring premium gap would be noise. A
    /// this-run failure (auth / quota / 5xx / transport / malformed), by contrast, is
    /// recorded as a gap so the agent sees the enrichment was lost this week; a
    /// `Rejected` stops the loop, like the quote groups.
    fn fetch_index_performance(&self, gaps: &mut Vec<DataGap>) -> Vec<IndexPerformance> {
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
            self.progress
                .request_started("FMP", GroupKind::IndexPerformance.as_str(), symbol, name);
            let gaps_before = gaps.len();
            let out_before = out.len();
            let disposition = match self.get(
                FMP_EOD_URL,
                &[("symbol", symbol), ("from", from_s.as_str()), ("to", to_s.as_str())],
            ) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            match disposition {
                Disposition::Value(value) => match eod_to_performance(value, symbol, name) {
                    Ok(Some(perf)) => out.push(perf),
                    // Too short to anchor — skip silently; the daily quote still covers it.
                    Ok(None) => {}
                    Err(_) => gaps.push(DataGap::new(
                        GroupKind::IndexPerformance,
                        symbol,
                        name,
                        GapReason::Malformed,
                    )),
                },
                // Permanent absence (402/404) is silent for this additive group.
                Disposition::Gap(GapReason::OutOfScope) => {}
                Disposition::Gap(reason) => {
                    if reason == GapReason::Rejected {
                        rejected = true;
                    }
                    gaps.push(DataGap::new(GroupKind::IndexPerformance, symbol, name, reason));
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
            );
        }
        out
    }
}

impl MarketDataSource for FmpDataSource {
    fn baseline_scan(&self) -> Result<BaselineMarketData> {
        // Every group degrades to recorded gaps rather than failing: a thin or empty
        // `indices` group is no longer this adapter's call to abort on — the central
        // coverage gate (`pipeline::enforce_coverage`) decides the run's floor over the
        // merged baseline. So this scan returns `Ok` for all data outcomes; only a
        // catastrophic (non-data) fault would be an `Err`, and none arises here.
        let mut gaps = Vec::new();
        // Each fetch streams its own per-request tracker rows (one per series / date
        // probe / EOD-history call), so the scan emits no group-level summary rows.
        let indices = self.fetch_quotes(INDEX_SYMBOLS, GroupKind::Indices, &mut gaps);
        let internals = self.fetch_quotes(INTERNAL_SYMBOLS, GroupKind::Internals, &mut gaps);
        let sectors = self.fetch_sectors(&mut gaps);
        let index_performance = self.fetch_index_performance(&mut gaps);
        Ok(BaselineMarketData {
            indices,
            internals,
            sectors,
            index_performance,
            // FRED owns the macro levels and the economic-release calendar, and BLS the
            // labor levels; FMP contributes none of them.
            macro_levels: Vec::new(),
            labor_levels: Vec::new(),
            calendar: Vec::new(),
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
        // 2xx array (incl. the empty "no data" array) -> a value to shape.
        assert!(matches!(
            interpret_response(200, r#"[{"symbol":"^GSPC","price":1.0,"changePercentage":0.1}]"#),
            Disposition::Value(_)
        ));
        assert!(matches!(interpret_response(200, "[]"), Disposition::Value(_)));

        // Explicit skip allowlist: a legitimate per-symbol absence -> OutOfScope gap.
        assert!(matches!(
            interpret_response(402, "Premium Query Parameter"),
            Disposition::Gap(OutOfScope)
        ));
        assert!(matches!(interpret_response(404, ""), Disposition::Gap(OutOfScope)));

        // Auth -> Rejected; systemic 429/5xx -> Unavailable; request-contract -> Malformed
        // (a 400, e.g. a malformed sector date, degrades to a gap rather than skipping
        // silently).
        for status in [401, 403] {
            assert!(matches!(interpret_response(status, ""), Disposition::Gap(Rejected)), "HTTP {status}");
        }
        for status in [429, 500, 503] {
            assert!(matches!(interpret_response(status, ""), Disposition::Gap(Unavailable)), "HTTP {status}");
        }
        for status in [400, 408, 422] {
            assert!(matches!(interpret_response(status, ""), Disposition::Gap(Malformed)), "HTTP {status}");
        }

        // A 200 {"Error Message"} body (rate-limit / plan) -> Rejected...
        assert!(matches!(
            interpret_response(200, r#"{"Error Message":"Limit Reach"}"#),
            Disposition::Gap(Rejected)
        ));
        // ...but the SAME body on a non-2xx is classified by status, not body (402 skips).
        assert!(matches!(
            interpret_response(402, r#"{"Error Message":"Premium"}"#),
            Disposition::Gap(OutOfScope)
        ));
        // A 2xx that isn't valid JSON is a contract violation -> Malformed.
        assert!(matches!(interpret_response(200, "not json at all"), Disposition::Gap(Malformed)));
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
        // neither a false 0.0 nor a silent skip; the loop records a Malformed gap.
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
        // Fail-closed: a row missing averageChange fails the parse (a Malformed gap in
        // the loop), rather than being silently dropped as a false "flat" move.
        let v: Value = serde_json::from_str(
            r#"[{"sector":"Technology","averageChange":1.5},{"sector":"Energy"}]"#,
        )
        .unwrap();
        assert!(sectors_from_value(v).is_err());
    }

    #[test]
    fn sector_candidate_dates_skips_weekends_from_a_sunday() {
        // The weekly job fires Sunday: candidates skip Sat/Sun and start at the prior
        // Friday, then walk back over weekdays only.
        let sunday = NaiveDate::from_ymd_opt(2026, 6, 7).unwrap();
        assert_eq!(sunday.weekday(), Weekday::Sun, "fixture sanity");
        let got: Vec<String> = sector_candidate_dates(sunday, 5)
            .iter()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            got,
            ["2026-06-05", "2026-06-04", "2026-06-03", "2026-06-02", "2026-06-01"],
            "Sunday run starts at Fri 06-05 and walks back weekdays only"
        );
    }

    #[test]
    fn sector_candidate_dates_from_a_weekday_includes_today_then_skips_the_weekend() {
        // A Wednesday start includes Wednesday, Tue, Mon, then skips the weekend to the
        // prior Fri, Thu.
        let wednesday = NaiveDate::from_ymd_opt(2026, 6, 10).unwrap();
        assert_eq!(wednesday.weekday(), Weekday::Wed, "fixture sanity");
        let got: Vec<String> = sector_candidate_dates(wednesday, 5)
            .iter()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            got,
            ["2026-06-10", "2026-06-09", "2026-06-08", "2026-06-05", "2026-06-04"],
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
        assert!((p.weekly_pct - 10.0).abs() < 1e-9, "weekly {}", p.weekly_pct);
        assert!((p.mtd_pct - (15.0 / 95.0 * 100.0)).abs() < 1e-9, "mtd {}", p.mtd_pct);
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
        // Only the latest close (nothing a week back) can't anchor the weekly return.
        let only = vec![(NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(), 100.0)];
        assert!(index_performance_from_eod("^GSPC", "S&P 500", &only).is_none());
        assert!(index_performance_from_eod("^GSPC", "S&P 500", &[]).is_none());
    }

    #[test]
    fn eod_to_performance_parses_sorts_and_rejects_bad_shapes() {
        // Out-of-order rows are sorted newest-first before anchoring: latest is 06-10
        // (110), week-ago as-of 06-03 is 100 -> +10%.
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
        assert!((p.weekly_pct - 10.0).abs() < 1e-9, "weekly {}", p.weekly_pct);

        // A non-array body is a contract violation.
        let obj: Value = serde_json::from_str(r#"{"unexpected":true}"#).unwrap();
        assert!(eod_to_performance(obj, "^GSPC", "S&P 500").is_err());
        // An unparseable date fails closed rather than being dropped.
        let bad_date: Value = serde_json::from_str(r#"[{"date":"June 10","price":1.0}]"#).unwrap();
        assert!(eod_to_performance(bad_date, "^GSPC", "S&P 500").is_err());
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

        // Index performance (multi-horizon EOD enrichment) — dump and assert each index
        // resolved, the per-symbol discipline the quote groups use. Soft enrichment at
        // runtime, but the smoke holds it to the same bar so a regressed EOD path surfaces.
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
    }
}
