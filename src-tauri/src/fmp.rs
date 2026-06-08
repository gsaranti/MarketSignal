//! Real Financial Modeling Prep adapter for the baseline market-data scan.
//!
//! The first data-source adapter behind the `MarketDataSource` trait
//! (`data_sources`). On FMP's free tier the provider is effectively an *equities*
//! API, so this adapter owns the equity-market half of the Step-3 baseline:
//! the market **indices** (Dow / S&P 500 / Nasdaq / Russell 2000), the **VIX**,
//! **gold** and **silver** (`GCUSD` / `SIUSD`, free on the quote endpoint), **sector
//! performance**, each index's **multi-horizon performance** (weekly / MTD / YTD /
//! 52-week range) derived from FMP's free end-of-day history, the **market movers**
//! (biggest gainers / losers / most-active names), the **earnings calendar** (the
//! prior-week + upcoming large-cap reporters), and the **valuation + finer-rotation**
//! snapshots — per-sector P/E, the strongest / weakest industries (average move joined
//! with aggregate P/E), and the US equity-risk-premium. The
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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};
use serde::Deserialize;
use serde_json::Value;

use crate::data_sources::{
    emit_series_row, BaselineMarketData, DataGap, EarningsEvent, GapReason, GroupKind,
    IndexPerformance, IndustrySnapshot, MarketDataSource, MarketRiskPremium, MoverCategory, Quote,
    SectorPe, SectorPerformance, StockMover,
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

/// FMP's free market-mover lists — biggest gainers / losers and the most-active names.
/// Each returns the whole US mover list in one call (no params). NB the most-active path
/// is **plural** (`most-actives`); the singular `most-active` 404s (probed live Jun 2026).
const FMP_GAINERS_URL: &str = "https://financialmodelingprep.com/stable/biggest-gainers";
const FMP_LOSERS_URL: &str = "https://financialmodelingprep.com/stable/biggest-losers";
const FMP_MOST_ACTIVE_URL: &str = "https://financialmodelingprep.com/stable/most-actives";

/// FMP's free earnings calendar — every US ticker reporting in a date window. Free on a
/// ~1-month history window (probed live Jun 2026: forward dates return estimates with
/// null actuals, past dates return both).
const FMP_EARNINGS_URL: &str = "https://financialmodelingprep.com/stable/earnings-calendar";

/// FMP's free valuation + finer-rotation snapshots — all date-keyed like the
/// sector-performance snapshot (a dateless call returns HTTP 400), all free-tier (probed
/// live Jun 2026). `sector-pe-snapshot` is the per-sector aggregate P/E (a valuation
/// complement to `sector-performance-snapshot`); the two `industry-*` snapshots are the
/// finer ~130-industry cut (average move + aggregate P/E), joined by industry name.
const FMP_SECTOR_PE_URL: &str = "https://financialmodelingprep.com/stable/sector-pe-snapshot";
const FMP_INDUSTRY_PERF_URL: &str =
    "https://financialmodelingprep.com/stable/industry-performance-snapshot";
const FMP_INDUSTRY_PE_URL: &str = "https://financialmodelingprep.com/stable/industry-pe-snapshot";

/// FMP's free market-risk-premium endpoint — Damodaran's per-country equity-risk-premium
/// dataset (no params). Filtered to the US row; a near-static annual constant (probed live
/// Jun 2026: US total ERP ≈ 4.46%).
const FMP_RISK_PREMIUM_URL: &str = "https://financialmodelingprep.com/stable/market-risk-premium";

/// The exchanges the valuation snapshots are gathered for. FMP's sector / industry snapshots
/// are **exchange-specific** (verified live: a no-`exchange` call defaults to NASDAQ only;
/// `NYSE` and `AMEX` are also free). We pin both major boards so the model sees the
/// growth/tech-tilted NASDAQ read *and* the broader, more value-weighted NYSE read rather
/// than silently treating one exchange's valuation as whole-market. Each call is pinned to a
/// single exchange, so the per-industry performance↔P/E join is always within one exchange.
const SNAPSHOT_EXCHANGES: &[&str] = &["NASDAQ", "NYSE"];

/// Industry-snapshot cap: keep the `INDUSTRY_TOP_N` strongest and `INDUSTRY_TOP_N` weakest
/// industries by average move (FMP reports ~130 per exchange), applied **per exchange**, so
/// the finer-rotation read surfaces the extremes without flooding the packet with the flat
/// middle. Tunable after a live run.
const INDUSTRY_TOP_N: usize = 10;

/// The exact `country` label to keep from the market-risk-premium dataset. Exact-match, not
/// a substring — "United Kingdom" and "United Arab Emirates" also start with "United".
const RISK_PREMIUM_COUNTRY: &str = "United States";

/// Mover-list filters (the raw lists are dominated by sub-$1 micro-caps that are noise for
/// a market thesis). Keep only names priced at or above the floor, listed on a major US
/// exchange, then the top N per list in FMP's own ranking order (gainers/losers by percent
/// move, most-actives by volume). Tunable after a live run.
const MOVER_MIN_PRICE: f64 = 5.0;
const MOVER_TOP_N: usize = 10;
const MOVER_EXCHANGES: &[&str] = &["NASDAQ", "NYSE", "AMEX"];

/// Case-insensitive name fragments that mark a mover row as a fund / ETF / ETN or a
/// leveraged-and-inverse product rather than an individual company. The free mover lists
/// carry no fund flag and are dominated by leveraged ETFs (TQQQ, SOXS, "Daily Target 2X
/// …"), which would otherwise be sector-tagged as companies; this name heuristic is the
/// only free signal. It necessarily errs toward false negatives, not false positives — the
/// prompt's "a mover may be a fund" caveat is the backstop for products it misses, whereas
/// dropping a real company is the worse error. So markers must be fund-specific:
/// - `" etf"` / `" etn"` carry a leading space to match the suffix, not a substring of a
///   company name (e.g. "Aetna").
/// - The leverage tokens (`2x`/`3x`/`leveraged`/`inverse`/`ultrapro`/`ultrashort`), the
///   issuer names, and the `" etf"`/`" etn"` suffix already catch every leveraged
///   *directional* product, so bare `"bull"`/`"bear"` are deliberately NOT markers — they
///   would drop real companies like "Build-A-Bear Workshop" for no added coverage.
const MOVER_FUND_MARKERS: &[&str] = &[
    " etf", " etn", " fund", "2x", "3x", "leveraged", "inverse", "ultrapro", "ultrashort",
    "daily target", "proshares", "direxion", "graniteshares", "microsectors",
];

/// Earnings-calendar window and filter: the prior week + the upcoming fortnight, then keep
/// only large-cap reporters (quarterly revenue estimate at or above the floor — no free
/// index-membership list to filter by, so revenue magnitude is the proxy), capped at the
/// largest N by revenue estimate. Tunable after a live run.
const EARNINGS_BACK_DAYS: i64 = 7;
const EARNINGS_FWD_DAYS: i64 = 14;
const EARNINGS_MIN_REVENUE: f64 = 5_000_000_000.0;
const EARNINGS_MAX_ROWS: usize = 25;

/// The four headline indices of the baseline scan (`docs/weekly-report-workflow
/// .md §Step 3`), paired with a display name used when FMP omits one and the `price`
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
    ("SIUSD", "Silver", "USD per troy ounce"),
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

/// One row of FMP's mover lists (gainers / losers / most-actives share this shape).
/// `price` and the percent change are required; `name` / `exchange` fall back to empty
/// when absent. The percent change is `changesPercentage` (plural) on the mover lists,
/// with the singular `changePercentage` accepted as an alias — the inverse of the quote
/// endpoint's spelling (probed live Jun 2026). `volume` is not returned by these lists.
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

/// One row of FMP's earnings calendar. `symbol` and `date` are required; the EPS /
/// revenue estimate and actual fields are all optional — FMP omits actuals for dates that
/// haven't reported and can omit estimates for thinly-covered names.
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

/// One row of FMP's sector-PE snapshot. `sector`, `exchange`, and `pe` are all required — a
/// row missing any fails the parse (a `Malformed` gap in the loop) rather than dropping
/// silently. `exchange` is read from the wire (not assumed from the request) so the row is
/// labelled by the board FMP actually reported, even if the `exchange` query param were ever
/// ignored or regressed. `date` is ignored.
#[derive(Debug, Deserialize)]
struct FmpSectorPeRaw {
    sector: String,
    exchange: String,
    pe: f64,
}

/// One row of FMP's industry-performance snapshot. `industry`, `exchange`, and
/// `averageChange` are required; `exchange` is read from the wire (see [`FmpSectorPeRaw`]).
/// `date` is ignored.
#[derive(Debug, Deserialize)]
struct FmpIndustryPerfRaw {
    industry: String,
    exchange: String,
    #[serde(rename = "averageChange")]
    average_change: f64,
}

/// One row of FMP's industry-PE snapshot. `industry`, `exchange`, and `pe` are required;
/// `exchange` is read from the wire (see [`FmpSectorPeRaw`]). `date` is ignored.
#[derive(Debug, Deserialize)]
struct FmpIndustryPeRaw {
    industry: String,
    exchange: String,
    pe: f64,
}

/// One row of FMP's market-risk-premium dataset. `country` and both premiums are required;
/// the `continent` field is ignored.
#[derive(Debug, Deserialize)]
struct FmpRiskPremiumRaw {
    country: String,
    #[serde(rename = "countryRiskPremium")]
    country_risk_premium: f64,
    #[serde(rename = "totalEquityRiskPremium")]
    total_equity_risk_premium: f64,
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

/// One gap for the `sector-pe` group on `exchange` — like `sector_gap`, a whole-snapshot
/// group whose gap carries a synthetic, exchange-tagged series id / name rather than one per
/// sector (so a NASDAQ failure and an NYSE failure are distinct manifest entries).
fn sector_pe_gap(exchange: &str, reason: GapReason) -> DataGap {
    DataGap::new(
        GroupKind::SectorPe,
        format!("sector-pe-{}", exchange.to_ascii_lowercase()),
        format!("Sector P/E ({exchange})"),
        reason,
    )
}

/// One gap for the industry-performance leg of the `industries` group on `exchange`.
fn industry_perf_gap(exchange: &str, reason: GapReason) -> DataGap {
    DataGap::new(
        GroupKind::Industries,
        format!("industry-performance-{}", exchange.to_ascii_lowercase()),
        format!("Industry Performance ({exchange})"),
        reason,
    )
}

/// One gap for the industry-P/E leg of the `industries` group on `exchange`.
fn industry_pe_gap(exchange: &str, reason: GapReason) -> DataGap {
    DataGap::new(
        GroupKind::Industries,
        format!("industry-pe-{}", exchange.to_ascii_lowercase()),
        format!("Industry P/E ({exchange})"),
        reason,
    )
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

/// Shape a successful mover-list response into typed [`StockMover`]s tagged with the list's
/// `category`, falling back to the symbol when FMP omits the name. A body that is not the
/// expected array of mover rows is an error.
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

/// Whether a mover's name marks it as a fund / ETF / ETN or leveraged-inverse product
/// rather than an individual company — a [`MOVER_FUND_MARKERS`] substring match,
/// case-insensitive. Imperfect by nature (no free fund flag); the prompt caveat backs it.
fn is_fund_or_leveraged(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    MOVER_FUND_MARKERS.iter().any(|marker| lower.contains(marker))
}

/// Filter one raw mover list down to thesis-relevant individual-company names: priced at or
/// above [`MOVER_MIN_PRICE`] (strips the sub-$1 micro-caps the raw lists are dominated by),
/// listed on a [`MOVER_EXCHANGES`] exchange, and not a fund / leveraged-inverse ETF
/// ([`is_fund_or_leveraged`] — the raw lists are otherwise full of TQQQ/SOXS-type products
/// that aren't single-company signals), capped at the first [`MOVER_TOP_N`] in FMP's
/// ranking order (the order the list arrives in — by percent move for gainers/losers, by
/// volume for most-actives). Pure.
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

/// Shape a successful earnings-calendar response into typed [`EarningsEvent`]s. A body that
/// is not the expected array of earnings rows is an error.
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

/// Filter the raw earnings calendar to large-cap reporters: keep rows whose quarterly
/// revenue estimate clears [`EARNINGS_MIN_REVENUE`] (no free index-membership list to
/// filter by, so revenue magnitude is the large-cap proxy), ordered by that estimate
/// descending and capped at [`EARNINGS_MAX_ROWS`]. Rows without a revenue estimate are
/// dropped — they can't clear the floor and are overwhelmingly thinly-covered small-caps.
/// Pure.
fn filter_earnings(events: Vec<EarningsEvent>) -> Vec<EarningsEvent> {
    let mut kept: Vec<EarningsEvent> = events
        .into_iter()
        .filter(|e| e.revenue_estimated.is_some_and(|r| r >= EARNINGS_MIN_REVENUE))
        .collect();
    kept.sort_by(|a, b| {
        b.revenue_estimated
            .partial_cmp(&a.revenue_estimated)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    kept.truncate(EARNINGS_MAX_ROWS);
    kept
}

/// Shape a successful sector-PE snapshot into typed rows. Every row's wire `exchange` must
/// match `expected_exchange` (the board the call was pinned to); a single mismatch fails the
/// whole leg as an error (→ a `Malformed` gap in the loop) rather than silently accepting
/// off-board rows — the guard against FMP ignoring the `exchange` query param and returning,
/// say, NASDAQ data for an NYSE request (which would otherwise duplicate one board and drop
/// the other with no gap). Rows are then labelled by their (validated) wire exchange and
/// deduplicated by (sector, exchange), keep first. A body that is not the expected array, or
/// that carries an off-board row, is an error.
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
            out.push(SectorPe {
                sector: raw.sector,
                exchange: raw.exchange,
                pe: raw.pe,
            });
        }
    }
    Ok(out)
}

/// Shape a successful industry-performance snapshot into `(industry, exchange, average_change)`
/// rows. Every row's wire `exchange` must match `expected_exchange`; a mismatch fails the leg
/// (see [`sector_pe_from_value`] for the rationale — the same off-board guard). Rows are then
/// labelled by their validated wire exchange and deduplicated by (industry, exchange), keep
/// first, preserving arrival order. A body that is not the expected array, or that carries an
/// off-board row, is an error.
fn industry_perf_from_value(value: Value, expected_exchange: &str) -> Result<Vec<(String, String, f64)>> {
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

/// Shape a successful industry-PE snapshot into an `(industry, exchange) -> pe` map. Every
/// row's wire `exchange` must match `expected_exchange`; a mismatch fails the leg (the same
/// off-board guard as [`sector_pe_from_value`]). The map keys by (industry, exchange) so the
/// performance↔P/E join can only ever pair same-board figures. Non-positive ratios are
/// dropped: FMP reports `pe: 0.0` (not null) for an industry with no positive aggregate
/// earnings, and a P/E is only a meaningful valuation when positive — so such an industry is
/// left out of the map and joins to `None`, rather than reaching the model as a misleading
/// near-zero "cheap" multiple. A body that is not the expected array, or that carries an
/// off-board row, is an error.
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
        if raw.pe > 0.0 {
            map.entry((raw.industry, raw.exchange)).or_insert(raw.pe);
        }
    }
    Ok(map)
}

/// Join the industry-performance rows with the PE map into the capped finer-rotation read:
/// the [`INDUSTRY_TOP_N`] strongest and [`INDUSTRY_TOP_N`] weakest industries by average move
/// (FMP reports ~130 per exchange, mostly a flat middle), each carrying the wire `exchange`
/// from its performance row and its aggregate `pe` where the PE snapshot had it for that same
/// (industry, exchange) (`None` otherwise — a missing/failed PE call, a non-positive ratio, or
/// a board mismatch degrades to no valuation, never drops the rotation row). Keying the lookup
/// by (industry, exchange) means a row's P/E can never come from a different board than its
/// performance. The two slices never overlap: the bottom count is clamped to what's left after
/// the top, so a short list yields each industry once. Pure.
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
            }
        })
        .collect()
}

/// Shape a successful market-risk-premium response, filtering to the US row
/// ([`RISK_PREMIUM_COUNTRY`], exact match). Zero or one row in practice. A body that is not
/// the expected array is an error.
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

    /// Fetch the three mover lists (gainers / losers / most-actives), one call each, and
    /// shape + filter them into tagged [`StockMover`]s. Additive enrichment like
    /// `index_performance`: a permanent absence (402/404) or an empty / all-filtered list is
    /// skipped silently — the breadth read sits on top of the required index/internals
    /// grounding — while a this-run failure (auth / quota / 5xx / transport / malformed)
    /// records a `Movers` gap so the agent sees the loss; a `Rejected` stops the loop and
    /// records the remaining lists, like the quote groups.
    fn fetch_movers(&self, gaps: &mut Vec<DataGap>) -> Vec<StockMover> {
        let endpoints = [
            (MoverCategory::Gainer, FMP_GAINERS_URL, "biggest-gainers", "Biggest Gainers"),
            (MoverCategory::Loser, FMP_LOSERS_URL, "biggest-losers", "Biggest Losers"),
            (MoverCategory::MostActive, FMP_MOST_ACTIVE_URL, "most-actives", "Most Active"),
        ];
        let mut out = Vec::new();
        let mut rejected = false;
        for (category, url, series_id, name) in endpoints {
            if self.progress.is_cancelled() {
                break;
            }
            if rejected {
                // No request made for a short-circuited list — no tracker row.
                gaps.push(DataGap::new(GroupKind::Movers, series_id, name, GapReason::Rejected));
                continue;
            }
            self.progress
                .request_started("FMP", GroupKind::Movers.as_str(), series_id, name);
            let gaps_before = gaps.len();
            let out_before = out.len();
            let disposition = match self.get(url, &[]) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            match disposition {
                Disposition::Value(value) => match movers_from_value(value, category) {
                    Ok(movers) => out.extend(filter_movers(movers)),
                    Err(_) => {
                        gaps.push(DataGap::new(GroupKind::Movers, series_id, name, GapReason::Malformed))
                    }
                },
                // Permanent absence (402/404) is silent for this additive group.
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
            );
        }
        out
    }

    /// Fetch the earnings calendar over the prior-week + upcoming-fortnight window in one
    /// call, then filter to large-cap reporters. Additive and non-floor like `movers`: a
    /// permanent absence or an empty / all-filtered window is silent; a this-run failure
    /// (auth / quota / 5xx / transport / malformed) records one `Earnings` gap.
    fn fetch_earnings(&self, gaps: &mut Vec<DataGap>) -> Vec<EarningsEvent> {
        if self.progress.is_cancelled() {
            return Vec::new();
        }
        let today = Utc::now().date_naive();
        let from = (today - Duration::days(EARNINGS_BACK_DAYS))
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
        let disposition =
            match self.get(FMP_EARNINGS_URL, &[("from", from.as_str()), ("to", to.as_str())]) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
        let out = match disposition {
            Disposition::Value(value) => match earnings_from_value(value) {
                Ok(events) => filter_earnings(events),
                Err(_) => {
                    gaps.push(DataGap::new(GroupKind::Earnings, series_id, name, GapReason::Malformed));
                    Vec::new()
                }
            },
            // Permanent absence (402/404) is silent for this additive group.
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
        );
        out
    }

    /// Fetch the per-sector P/E for each exchange in [`SNAPSHOT_EXCHANGES`] (NASDAQ + NYSE),
    /// accumulating the exchange-tagged rows so the model sees the growth and value reads
    /// side by side. Each exchange walks independently via [`Self::fetch_sector_pe_for_exchange`].
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

    /// Fetch one exchange's most recent sector-PE snapshot, walking back over weekday
    /// candidates like `fetch_sectors` (the snapshot is date-keyed, and weekends / holidays
    /// have none). The call is pinned to `exchange`. Additive and non-floor: a 404 / empty
    /// array for a date means no snapshot — try the prior weekday; a this-run failure
    /// (auth / quota / 5xx / transport / malformed) records one exchange-tagged `sector-pe`
    /// gap and stops walking; an exhausted walk returns empty with no gap.
    fn fetch_sector_pe_for_exchange(&self, exchange: &str, gaps: &mut Vec<DataGap>) -> Vec<SectorPe> {
        let today = Utc::now().date_naive();
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
            let disposition =
                match self.get(FMP_SECTOR_PE_URL, &[("date", date_str.as_str()), ("exchange", exchange)]) {
                    Ok((status, body)) => interpret_response(status, &body),
                    Err(_) => Disposition::Gap(GapReason::Unavailable),
                };
            let finish = |status: &str| {
                self.progress
                    .request_finished("FMP", group, series_id.as_str(), name.as_str(), status, None)
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

    /// Fetch the finer-rotation read for each exchange in [`SNAPSHOT_EXCHANGES`], accumulating
    /// each exchange's top/bottom industries (so the NASDAQ growth and NYSE value rotations
    /// are both surfaced, the cap applied per exchange).
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

    /// Fetch one exchange's finer-rotation read: walk weekday candidates for the
    /// industry-performance snapshot (the spine), then on the first date with data fetch the
    /// industry-PE snapshot for that same date and exchange and join them by industry name.
    /// Both calls are pinned to `exchange`, so the performance↔P/E join is within one
    /// exchange. Additive and non-floor: a performance this-run failure records one
    /// exchange-tagged `industry-performance` gap and stops; an exhausted walk returns empty
    /// with no gap. The PE leg degrades independently — its failure leaves the industries with
    /// `pe: None` plus one recorded `industry-pe` gap rather than dropping the rotation read.
    fn fetch_industries_for_exchange(
        &self,
        exchange: &str,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<IndustrySnapshot> {
        let today = Utc::now().date_naive();
        for date in sector_candidate_dates(today, SECTOR_LOOKBACK_WEEKDAYS) {
            if self.progress.is_cancelled() {
                return Vec::new();
            }
            let date_str = date.format("%Y-%m-%d").to_string();
            let name = format!("Industry performance {exchange} ({date_str})");
            let series_id = format!("industry-performance-{}-{date_str}", exchange.to_ascii_lowercase());
            let group = GroupKind::Industries.as_str();
            self.progress
                .request_started("FMP", group, series_id.as_str(), name.as_str());
            let disposition = match self.get(
                FMP_INDUSTRY_PERF_URL,
                &[("date", date_str.as_str()), ("exchange", exchange)],
            ) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
            let finish = |status: &str| {
                self.progress
                    .request_finished("FMP", group, series_id.as_str(), name.as_str(), status, None)
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

    /// Fetch one exchange's industry-PE snapshot for the date the performance leg resolved —
    /// the optional valuation join. Any failure or emptiness degrades to an empty map (the
    /// industries carry `pe: None`); a this-run failure additionally records one exchange-tagged
    /// `industry-pe` gap so the agent sees valuation was lost. Never aborts the group.
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
        let disposition = match self.get(FMP_INDUSTRY_PE_URL, &[("date", date_str), ("exchange", exchange)]) {
            Ok((status, body)) => interpret_response(status, &body),
            Err(_) => Disposition::Gap(GapReason::Unavailable),
        };
        let finish = |status: &str| {
            self.progress
                .request_finished("FMP", group, series_id.as_str(), name.as_str(), status, None)
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

    /// Fetch the US equity-risk-premium in one call (no date), then filter to the US row.
    /// Additive and non-floor like `earnings`: a permanent absence or an empty / no-US-row
    /// response is silent; a this-run failure (auth / quota / 5xx / transport / malformed)
    /// records one `market-risk-premium` gap.
    fn fetch_market_risk_premium(&self, gaps: &mut Vec<DataGap>) -> Vec<MarketRiskPremium> {
        if self.progress.is_cancelled() {
            return Vec::new();
        }
        let series_id = "market-risk-premium";
        let name = "US Equity Risk Premium";
        self.progress
            .request_started("FMP", GroupKind::MarketRiskPremium.as_str(), series_id, name);
        let gaps_before = gaps.len();
        let disposition = match self.get(FMP_RISK_PREMIUM_URL, &[]) {
            Ok((status, body)) => interpret_response(status, &body),
            Err(_) => Disposition::Gap(GapReason::Unavailable),
        };
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
                gaps.push(DataGap::new(GroupKind::MarketRiskPremium, series_id, name, reason));
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
        );
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
        let movers = self.fetch_movers(&mut gaps);
        let earnings = self.fetch_earnings(&mut gaps);
        let sector_pe = self.fetch_sector_pe(&mut gaps);
        let industries = self.fetch_industries(&mut gaps);
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
    fn movers_parse_tags_category_and_filters_noise() {
        // The mover lists key the move as `changesPercentage` (plural); parsing stamps the
        // list's category. filter_movers drops the sub-$5 micro-cap and the off-exchange
        // row, keeping major-exchange names in FMP's arrival order.
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
        // The quote-endpoint spelling `changePercentage` (singular) is accepted as an
        // alias, and a missing name falls back to the symbol.
        let body =
            serde_json::json!([{"symbol":"MSFT","price":410.0,"changePercentage":1.5,"exchange":"NASDAQ"}]);
        let parsed = movers_from_value(body, MoverCategory::MostActive).unwrap();
        assert_eq!(parsed[0].name, "MSFT");
        assert_eq!(parsed[0].change_pct, 1.5);
    }

    #[test]
    fn movers_filter_excludes_funds_and_leveraged_etfs() {
        // The raw lists are dominated by leveraged/inverse ETFs that clear the price +
        // exchange gate but aren't single-company signals; the name heuristic drops them
        // while keeping ordinary companies.
        let movers = vec![
            StockMover { category: MoverCategory::Gainer, symbol: "NVDA".into(),
                name: "NVIDIA Corporation".into(), price: 142.0, change_pct: 4.0, exchange: "NASDAQ".into() },
            StockMover { category: MoverCategory::Gainer, symbol: "TQQQ".into(),
                name: "ProShares - UltraPro QQQ".into(), price: 60.0, change_pct: 5.0, exchange: "NASDAQ".into() },
            StockMover { category: MoverCategory::Gainer, symbol: "SOXS".into(),
                name: "Direxion Daily Semiconductor Bear 3X ETF".into(), price: 7.0, change_pct: 9.0, exchange: "AMEX".into() },
            StockMover { category: MoverCategory::Gainer, symbol: "AAL".into(),
                name: "American Airlines Group Inc.".into(), price: 14.0, change_pct: 1.5, exchange: "NASDAQ".into() },
        ];
        let kept = filter_movers(movers);
        let symbols: Vec<&str> = kept.iter().map(|m| m.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["NVDA", "AAL"]);
    }

    #[test]
    fn movers_filter_keeps_companies_with_bull_or_bear_in_name() {
        // Regression: bare "bull "/"bear " markers would drop real companies. Build-A-Bear
        // stays; the leveraged directional ETF is still caught (by "direxion" / "3x" /
        // " etf"), so dropping the bare markers cost no coverage.
        let movers = vec![
            StockMover { category: MoverCategory::Gainer, symbol: "BBW".into(),
                name: "Build-A-Bear Workshop, Inc.".into(), price: 40.0, change_pct: 6.0, exchange: "NYSE".into() },
            StockMover { category: MoverCategory::Gainer, symbol: "SOXL".into(),
                name: "Direxion Daily Semiconductor Bull 3X ETF".into(), price: 25.0, change_pct: 8.0, exchange: "AMEX".into() },
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
        // A forward row (null actuals) and a past row (both) are kept when large-cap; the
        // sub-threshold and missing-revenue rows are dropped; output is revenue-descending.
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
        // A response whose rows all match the requested board is labelled by the (validated)
        // wire exchange and deduped by (sector, exchange), keep first.
        let v = serde_json::json!([
            {"date":"2026-06-05","sector":"Technology","exchange":"NASDAQ","pe":38.4},
            {"date":"2026-06-05","sector":"Energy","exchange":"NASDAQ","pe":12.1},
            {"date":"2026-06-05","sector":"Technology","exchange":"NASDAQ","pe":99.0}
        ]);
        let out = sector_pe_from_value(v, "NASDAQ").unwrap();
        assert_eq!(out.len(), 2); // the duplicate (Technology, NASDAQ) is dropped
        assert_eq!((out[0].sector.as_str(), out[0].exchange.as_str()), ("Technology", "NASDAQ"));
        assert!((out[0].pe - 38.4).abs() < 1e-9); // first kept, not 99.0
        assert_eq!((out[1].sector.as_str(), out[1].exchange.as_str()), ("Energy", "NASDAQ"));
    }

    #[test]
    fn sector_pe_from_value_rejects_off_board_rows() {
        // The guard against FMP ignoring the exchange filter: an NYSE request that comes back
        // with a NASDAQ row fails the whole leg (→ a Malformed gap) rather than silently
        // accepting off-board data, which would duplicate one board and drop the other.
        let v = serde_json::json!([
            {"sector":"Technology","exchange":"NYSE","pe":24.6},
            {"sector":"Energy","exchange":"NASDAQ","pe":12.1}
        ]);
        assert!(sector_pe_from_value(v, "NYSE").is_err());
    }

    #[test]
    fn sector_pe_from_value_requires_exchange_and_pe() {
        // Fail-closed: a row missing the wire exchange OR pe fails the parse (a Malformed gap
        // in the loop) rather than being stamped with a guessed exchange or a false 0.0.
        assert!(
            sector_pe_from_value(serde_json::json!([{"sector":"Technology","pe":1.0}]), "NASDAQ").is_err()
        );
        assert!(sector_pe_from_value(
            serde_json::json!([{"sector":"Technology","exchange":"NASDAQ"}]),
            "NASDAQ"
        )
        .is_err());
    }

    /// Build an industry-PE map fixture keyed by (industry, exchange), as the wire-keyed map.
    fn pe_map(rows: &[(&str, &str, f64)]) -> HashMap<(String, String), f64> {
        rows.iter()
            .map(|(ind, ex, pe)| ((ind.to_string(), ex.to_string()), *pe))
            .collect()
    }

    #[test]
    fn industries_join_caps_top_and_bottom_and_attaches_pe() {
        // Five performance rows; INDUSTRY_TOP_N caps each side, but with only 5 rows the
        // top-N and bottom-N slices must not double-count — assert no industry repeats and
        // the strongest + weakest are present, sorted move-descending. The exchange label and
        // the PE both come from the wire row, joined on (industry, exchange).
        let perf = vec![
            ("Semiconductors".to_string(), "NASDAQ".to_string(), 4.0),
            ("Banks".to_string(), "NASDAQ".to_string(), 1.0),
            ("Utilities".to_string(), "NASDAQ".to_string(), 0.0),
            ("Airlines".to_string(), "NASDAQ".to_string(), -2.0),
            ("Biotech".to_string(), "NASDAQ".to_string(), -5.0),
        ];
        let pe = pe_map(&[("Semiconductors", "NASDAQ", 41.2), ("Biotech", "NASDAQ", 18.0)]);
        let out = top_bottom_industries(perf, &pe);
        let names: Vec<&str> = out.iter().map(|i| i.industry.as_str()).collect();
        assert_eq!(names, ["Semiconductors", "Banks", "Utilities", "Airlines", "Biotech"]);
        assert!(out.iter().all(|i| i.exchange == "NASDAQ"));
        // PE joins where present; absent industries carry None rather than dropping the row.
        assert_eq!(out[0].pe, Some(41.2));
        assert_eq!(out[1].pe, None);
        assert_eq!(out[4].pe, Some(18.0));
    }

    #[test]
    fn industries_join_is_same_exchange_only() {
        // A PE that exists for the same industry on a DIFFERENT board must not attach: the
        // (industry, exchange) key keeps a row's P/E and performance on the same board.
        let perf = vec![("Semiconductors".to_string(), "NYSE".to_string(), 3.0)];
        let pe = pe_map(&[("Semiconductors", "NASDAQ", 63.9)]); // NASDAQ PE only
        let out = top_bottom_industries(perf, &pe);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].exchange, "NYSE");
        assert_eq!(out[0].pe, None, "NASDAQ PE must not attach to the NYSE row");
    }

    #[test]
    fn industries_join_picks_extremes_when_list_exceeds_cap() {
        // 2*INDUSTRY_TOP_N + 4 industries: keep only the N strongest and N weakest, drop the
        // flat middle, with no overlap.
        let mut perf: Vec<(String, String, f64)> = (0..2 * INDUSTRY_TOP_N + 4)
            .map(|i| (format!("Ind{i}"), "NYSE".to_string(), i as f64)) // ascending move
            .collect();
        perf.reverse(); // arrival order need not be sorted
        let out = top_bottom_industries(perf, &HashMap::new());
        assert_eq!(out.len(), 2 * INDUSTRY_TOP_N);
        // Strongest is the highest move; weakest is the lowest; the middle is dropped.
        assert_eq!(out.first().unwrap().industry, format!("Ind{}", 2 * INDUSTRY_TOP_N + 3));
        assert_eq!(out.last().unwrap().industry, "Ind0");
        // No industry appears twice.
        let unique: HashSet<&str> = out.iter().map(|i| i.industry.as_str()).collect();
        assert_eq!(unique.len(), out.len());
    }

    #[test]
    fn industry_perf_and_pe_parse_keep_wire_exchange_and_dedupe() {
        // A matching-board response is labelled by its (validated) wire exchange and deduped by
        // (industry, exchange), keep first.
        let perf_v = serde_json::json!([
            {"date":"2026-06-05","industry":"Semiconductors","exchange":"NASDAQ","averageChange":2.4},
            {"date":"2026-06-05","industry":"Banks","exchange":"NASDAQ","averageChange":1.1},
            {"date":"2026-06-05","industry":"Semiconductors","exchange":"NASDAQ","averageChange":-1.0}
        ]);
        let perf = industry_perf_from_value(perf_v, "NASDAQ").unwrap();
        assert_eq!(perf.len(), 2); // the duplicate (Semiconductors, NASDAQ) is dropped
        assert_eq!((perf[0].0.as_str(), perf[0].1.as_str()), ("Semiconductors", "NASDAQ"));
        assert!((perf[0].2 - 2.4).abs() < 1e-9); // first kept, not -1.0
        let pe_v = serde_json::json!([{"industry":"Semiconductors","exchange":"NASDAQ","pe":41.2}]);
        let pe = industry_pe_map_from_value(pe_v, "NASDAQ").unwrap();
        assert!((pe[&("Semiconductors".to_string(), "NASDAQ".to_string())] - 41.2).abs() < 1e-9);
    }

    #[test]
    fn industry_snapshots_reject_off_board_rows() {
        // Same off-board guard as sector P/E, for both industry legs.
        let perf_v = serde_json::json!([{"industry":"Semiconductors","exchange":"NASDAQ","averageChange":2.4}]);
        assert!(industry_perf_from_value(perf_v, "NYSE").is_err());
        let pe_v = serde_json::json!([{"industry":"Semiconductors","exchange":"NASDAQ","pe":41.2}]);
        assert!(industry_pe_map_from_value(pe_v, "NYSE").is_err());
    }

    #[test]
    fn industry_pe_map_drops_non_positive_ratios() {
        // FMP reports pe: 0.0 (or negative) for an industry with no positive aggregate
        // earnings; those are dropped so the join yields None (no meaningful P/E) rather
        // than a misleading near-zero "cheap" multiple reaching the model.
        let v = serde_json::json!([
            {"industry":"Oil & Gas Energy","exchange":"NASDAQ","pe":0.0},
            {"industry":"Biotech","exchange":"NASDAQ","pe":-3.0},
            {"industry":"Semiconductors","exchange":"NASDAQ","pe":63.9}
        ]);
        let map = industry_pe_map_from_value(v, "NASDAQ").unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&("Semiconductors".to_string(), "NASDAQ".to_string())));
        // The join carries None for the dropped industries.
        let perf = vec![
            ("Oil & Gas Energy".to_string(), "NASDAQ".to_string(), -16.3),
            ("Semiconductors".to_string(), "NASDAQ".to_string(), -5.5),
        ];
        let joined = top_bottom_industries(perf, &map);
        let oil = joined.iter().find(|i| i.industry == "Oil & Gas Energy").unwrap();
        assert_eq!(oil.pe, None);
        let semi = joined.iter().find(|i| i.industry == "Semiconductors").unwrap();
        assert_eq!(semi.pe, Some(63.9));
    }

    #[test]
    fn risk_premium_filters_to_us_exactly() {
        // Exact-match: "United Kingdom" / "United Arab Emirates" share the "United" prefix
        // but must not pass; only "United States" survives.
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

        // Movers + earnings: the micro-breadth groups. Dump and assert each resolved at
        // least one filtered row — a trading day always has large-cap movers and reporters
        // in the window, so empty means the endpoint left the free tier or the filters are
        // too tight. (Silver is asserted by `assert_resolved` over INTERNAL_SYMBOLS above.)
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

        // Valuation + finer-rotation groups. Dump, assert each resolved, and sanity-check
        // magnitude (not mere existence) — the lesson of the frozen NASDAQVOLNDX series:
        // a stale / wrong value still "resolves", so the smoke pins it to a sane range.
        eprintln!("sector_pe ({}):", data.sector_pe.len());
        for s in &data.sector_pe {
            eprintln!("  {:<8} {:<24} pe={:.2}", s.exchange, s.sector, s.pe);
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
            data.sector_pe.iter().any(|s| s.pe.is_finite() && s.pe > 0.0),
            "no sector carried a finite positive P/E — the snapshot may have regressed"
        );
        assert!(!data.industries.is_empty(), "no industry rows resolved");
        assert!(
            data.industries.iter().any(|i| i.pe.is_some()),
            "no industry carried a P/E — the industry-PE join may have regressed"
        );
        // Both boards must resolve — a silent drop of one exchange would otherwise hide behind
        // the other and re-introduce the single-exchange-as-aggregate bias this layer fixes.
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

    /// Free-vs-premium probe for candidate Step-3 baseline endpoints whose tier the
    /// FMP docs (403 to scrapers, identical boilerplate per page) won't settle.
    /// Prints the HTTP status (200 ≈ free, 402 = premium) plus a sample of the body so
    /// a maintainer can read the real field names before any adapter is written. Hits
    /// the live API (~15 one-shot calls, trivial against the 250/day free cap); run:
    ///   source ~/.config/market-signal/keys.env && cargo test --manifest-path \
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
        // Movers — expected FREE; confirm exact %-change key + whether sector/exchange present.
        probe("biggest-gainers", &format!("{base}/biggest-gainers"), &[]);
        probe("biggest-losers", &format!("{base}/biggest-losers"), &[]);
        probe("most-active", &format!("{base}/most-active"), &[]);
        probe("most-actives (plural alias)", &format!("{base}/most-actives"), &[]);
        // Earnings calendar — docs say "Free: historical up to 1 month"; confirm forward dates populate.
        probe(
            "earnings-calendar",
            &format!("{base}/earnings-calendar"),
            &[("from", from.as_str()), ("to", to.as_str())],
        );
        // Constituent lists (keystone: free ticker→sector map). Confirm free + sector field.
        probe("sp-500 constituents", &format!("{base}/sp-500"), &[]);
        probe("dow-jones constituents", &format!("{base}/dow-jones"), &[]);
        // Sector/industry valuation + finer rotation — date-keyed.
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
        // Valuation context constant (near-static, per-country ERP).
        probe("market-risk-premium", &format!("{base}/market-risk-premium"), &[]);
        // Commodities: gold already free via /quote; do copper/silver resolve on free?
        probe("commodities-quote GCUSD (gold)", &format!("{base}/commodities-quote"), &[("symbol", "GCUSD")]);
        probe("commodities-quote HGUSD (copper)", &format!("{base}/commodities-quote"), &[("symbol", "HGUSD")]);
        probe("commodities-quote SIUSD (silver)", &format!("{base}/commodities-quote"), &[("symbol", "SIUSD")]);
        // 1-call consolidation candidate: does it cover the 4 indices (and ^VIX)?
        probe("all-index-quotes", &format!("{base}/all-index-quotes"), &[]);

        // --- Corrected paths for the endpoints that 404'd above (404 = wrong path,
        // not premium; premium is 402, which none of these returned) ---
        // Constituent lists use the `*-constituent` paths, not the bare index name.
        probe("sp500-constituent", &format!("{base}/sp500-constituent"), &[]);
        probe("dowjones-constituent", &format!("{base}/dowjones-constituent"), &[]);
        probe("nasdaq-constituent", &format!("{base}/nasdaq-constituent"), &[]);
        // Batch index quotes — the likely real name of the 1-call index consolidation.
        probe("batch-index-quotes", &format!("{base}/batch-index-quotes"), &[]);
        // Copper / silver via the generic quote endpoint we already use for GCUSD gold.
        probe("quote HGUSD (copper)", &format!("{base}/quote"), &[("symbol", "HGUSD")]);
        probe("quote SIUSD (silver)", &format!("{base}/quote"), &[("symbol", "SIUSD")]);
    }
}
