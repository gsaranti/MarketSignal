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
//! recent + upcoming large-cap reporters), and the **valuation + finer-rotation**
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
    emit_series_row, BaselineMarketData, Change, DataGap, EarningsEvent, GapReason, GroupKind,
    IndexPerformance, IndustrySnapshot, MarketDataSource, MarketRiskPremium, MoverCategory, Quote,
    SectorPe, SectorPerformance, StockMover,
};
use crate::cadence::ReportCadence;
use crate::progress::RunContext;

/// Base URL for FMP's stable API. The endpoint paths below are joined onto it in
/// [`FmpDataSource::get`]; a test redirects the whole adapter at a localhost mock via
/// [`FmpDataSource::with_base_url`], so the wire path (URL build → retry → interpret)
/// runs offline.
const FMP_BASE: &str = "https://financialmodelingprep.com/stable";

/// FMP's stable single-symbol quote endpoint — the one `connection_test` exercises.
const FMP_QUOTE_PATH: &str = "/quote";
/// FMP's sector-performance snapshot endpoint. Requires a `date` query param
/// (a dateless call returns HTTP 400).
const FMP_SECTOR_PATH: &str = "/sector-performance-snapshot";

/// Short timeout per request: the baseline scan issues several sequential calls,
/// none of which should park for the model adapter's 120s ceiling.
const FMP_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// How many trading-day candidates back to probe for the most recent sector snapshot.
/// A run can land on a weekend, when the latest snapshot is the prior Friday's;
/// `sector_candidate_dates` skips the closed-market weekend without spending a request,
/// so this budget covers weekdays (the holidays that actually need walking back over)
/// rather than being burned on a guaranteed-empty Saturday or Sunday.
const SECTOR_LOOKBACK_WEEKDAYS: usize = 5;

/// FMP's end-of-day historical-price endpoint (light: date + close). One call per
/// index over a trailing ~53-week window backs the multi-horizon `IndexPerformance`
/// (weekly / MTD / YTD / 52-week range) — free on the equities tier (probed live
/// Jun 2026, all four indices + the VIX return 200 with data).
const FMP_EOD_PATH: &str = "/historical-price-eod/light";

/// Trailing window requested for the EOD history: ~53 weeks, so the 52-week range and
/// the year-to-date anchor both sit inside the window with margin.
const EOD_LOOKBACK_DAYS: i64 = 371;

/// FMP's free market-mover lists — biggest gainers / losers and the most-active names.
/// Each returns the whole US mover list in one call (no params). NB the most-active path
/// is **plural** (`most-actives`); the singular `most-active` 404s (probed live Jun 2026).
const FMP_GAINERS_PATH: &str = "/biggest-gainers";
const FMP_LOSERS_PATH: &str = "/biggest-losers";
const FMP_MOST_ACTIVE_PATH: &str = "/most-actives";

/// FMP's free earnings calendar — every US ticker reporting in a date window. Free on a
/// ~1-month history window (probed live Jun 2026: forward dates return estimates with
/// null actuals, past dates return both).
const FMP_EARNINGS_PATH: &str = "/earnings-calendar";

/// FMP's free valuation + finer-rotation snapshots — all date-keyed like the
/// sector-performance snapshot (a dateless call returns HTTP 400), all free-tier (probed
/// live Jun 2026). `sector-pe-snapshot` is the per-sector aggregate P/E (a valuation
/// complement to `sector-performance-snapshot`); the two `industry-*` snapshots are the
/// finer ~130-industry cut (average move + aggregate P/E), joined by industry name.
const FMP_SECTOR_PE_PATH: &str = "/sector-pe-snapshot";
const FMP_INDUSTRY_PERF_PATH: &str = "/industry-performance-snapshot";
const FMP_INDUSTRY_PE_PATH: &str = "/industry-pe-snapshot";

/// FMP's free market-risk-premium endpoint — Damodaran's per-country equity-risk-premium
/// dataset (no params). Filtered to the US row; a near-static annual constant (probed live
/// Jun 2026: US total ERP ≈ 4.46%).
const FMP_RISK_PREMIUM_PATH: &str = "/market-risk-premium";

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

/// The plausible-aggregate ceiling for an industry P/E. Above this a multiple is treated as
/// a near-zero-earnings artifact rather than a valuation: FMP's aggregate P/E divides an
/// industry's summed price by its summed earnings, so an industry near an earnings trough can
/// report an absurd multiple (a live run surfaced `pe ≈ 461`) that reads to the model as a
/// real "expensive" level when it is noise from a denominator approaching zero. This is the
/// symmetric upper bound to the non-positive drop in [`industry_pe_map_from_value`] — both
/// withhold a meaningless figure (→ `None`) rather than fabricate or pass one. Calibrated
/// against the live distribution (`tuning_industry_pe_distribution_probe`, 2026-06-16): the
/// plausible band runs up to ~94 (NYSE REIT-Healthcare; NASDAQ tops at ~88, Construction)
/// and the clear denominator-near-zero artifact cluster begins ≥128 (up to ~465), leaving a
/// clean ~106→128 gap. The ceiling sits at 120 — inside that gap, ~8pt below the artifact
/// floor — so the borderline 100–106 aggregates (energy E&P, casinos at an earnings trough)
/// are **kept** as the genuine, if low-signal, cyclical-trough valuations they are, while the
/// ≥128 denominator artifacts stay dropped. Re-run the probe to re-tune.
const INDUSTRY_PE_MAX: f64 = 120.0;

/// The plausible-aggregate ceiling for a *sector* P/E — the symmetric upper bound to the
/// non-positive drop in [`sector_pe_from_value`], the same drop-to-`None` honesty stance as
/// [`INDUSTRY_PE_MAX`]. FMP's sector aggregate divides a sector's summed price by its summed
/// earnings, so the same denominator-near-zero artifact is possible (an aggregate inflated past
/// any plausible level by a sector at an earnings trough), and the non-positive case is *more*
/// reachable here than at the industry cut: a whole sector can carry net-negative trailing
/// aggregate earnings in a broad downturn (FMP reports `pe: 0.0` there), which would otherwise
/// pass through as a misleading near-zero "cheap" multiple. Calibrated against the live
/// distribution (`tuning_sector_pe_distribution_probe`, 2026-06-17 board snapshot): the
/// prediction held —
/// summing over far more constituents than an industry's makes the artifact tail *rarer* and the
/// plausible band *tighter*. Both boards showed zero sectors above the prior 120 ceiling and zero
/// non-positive, the highest plausible aggregate sitting at 85.2 (NASDAQ Consumer Cyclical; NYSE
/// topped at 45.6, Technology), with no artifact cluster at all — versus the industry cut's ~94
/// plausible max and ≥128 artifact floor. The ceiling drops 120 → 100: ~15pt of headroom above
/// the observed max so a genuine cyclical-trough sector multiple drifting past 85 is kept, while
/// the hundreds-magnitude denominator-near-zero artifact a future trough could still produce
/// (same mechanism as the industry ~461) stays dropped. Re-run the probe to re-tune.
const SECTOR_PE_MAX: f64 = 100.0;

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

/// Earnings-calendar window and filter: a cadence-sized lookback (the reporters since the
/// previous report) + the upcoming fortnight, then keep only large-cap reporters (quarterly
/// revenue estimate at or above the floor — no free index-membership list to filter by, so
/// revenue magnitude is the proxy), capped at the largest N by revenue estimate. Tunable
/// after a live run.
///
/// The *back* window scales with the run cadence ([`earnings_back_days`]): [`EARNINGS_BACK_DAYS`]
/// is the floor (a sub-weekly run still gets a week of recent context) and
/// [`EARNINGS_BACK_MAX_DAYS`] the cap (FMP's free earnings window is ~1 month). The *forward*
/// window stays fixed — "upcoming" is upcoming regardless of how long since the last report.
const EARNINGS_BACK_DAYS: i64 = 7;
const EARNINGS_BACK_MAX_DAYS: i64 = 31;
const EARNINGS_FWD_DAYS: i64 = 14;
const EARNINGS_MIN_REVENUE: f64 = 5_000_000_000.0;
const EARNINGS_MAX_ROWS: usize = 25;

/// The earnings-calendar lookback in whole days for this run: [`EARNINGS_BACK_DAYS`] on the
/// first report (no prior interval), else the elapsed interval rounded up and clamped to
/// `[EARNINGS_BACK_DAYS, EARNINGS_BACK_MAX_DAYS]`. The saturating `f64 as i64` cast makes a
/// non-finite or absurd interval clamp to the floor/cap rather than panic.
fn earnings_back_days(elapsed_days: Option<f64>) -> i64 {
    match elapsed_days {
        None => EARNINGS_BACK_DAYS,
        Some(d) => (d.ceil() as i64).clamp(EARNINGS_BACK_DAYS, EARNINGS_BACK_MAX_DAYS),
    }
}

/// The four headline indices of the baseline scan (`docs/report-workflow
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
    DataGap::new(
        GroupKind::Sectors,
        "sector-performance",
        "Sector Performance",
        reason,
    )
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
            change: Change::percent(r.change_pct),
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
    MOVER_FUND_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
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

/// Shape a successful sector-PE snapshot into typed rows. Every row's wire `exchange` must
/// match `expected_exchange` (the board the call was pinned to); a single mismatch fails the
/// whole leg as an error (→ a `Malformed` gap in the loop) rather than silently accepting
/// off-board rows — the guard against FMP ignoring the `exchange` query param and returning,
/// say, NASDAQ data for an NYSE request (which would otherwise duplicate one board and drop
/// the other with no gap). Rows are then labelled by their (validated) wire exchange and
/// deduplicated by (sector, exchange), keep first. Each kept row's aggregate `pe` is then
/// band-bounded to `(0.0, SECTOR_PE_MAX]` (see [`SECTOR_PE_MAX`] and the matching industry
/// drop in [`industry_pe_map_from_value`]): a non-positive aggregate (FMP's `0.0` for a sector
/// with no positive summed earnings) or one inflated past the ceiling by a near-zero earnings
/// base is dropped to `None` rather than passed as a misleading "cheap"/"expensive" multiple —
/// but the (sector, exchange) row itself survives, so the model still sees the sector was
/// scanned. A body that is not the expected array, or that carries an off-board row, is an error.
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

/// Shape a successful industry-PE snapshot into an `(industry, exchange) -> pe` map. Every
/// row's wire `exchange` must match `expected_exchange`; a mismatch fails the leg (the same
/// off-board guard as [`sector_pe_from_value`]). The map keys by (industry, exchange) so the
/// performance↔P/E join can only ever pair same-board figures. Out-of-band ratios are
/// dropped from both ends: FMP reports `pe: 0.0` (not null) for an industry with no positive
/// aggregate earnings, and an aggregate divided by a denominator approaching zero from above
/// inflates past any plausible level (a live run surfaced `pe ≈ 461`) — a P/E is only a
/// meaningful valuation inside `(0.0, INDUSTRY_PE_MAX]`, so an industry outside that band is
/// left out of the map and joins to `None`, rather than reaching the model as a misleading
/// near-zero "cheap" or absurdly-inflated "expensive" multiple. A body that is not the
/// expected array, or that carries an off-board row, is an error.
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
/// actually need walking back over) rather than being burned on the weekend. A run
/// landing on a weekend would otherwise spend its first one or two requests on the
/// guaranteed-empty Sat/Sun.
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

/// Live FMP adapter behind the `MarketDataSource` trait.
pub struct FmpDataSource {
    api_key: String,
    http: reqwest::blocking::Client,
    /// API origin the endpoint paths are joined onto. Defaults to [`FMP_BASE`]; an
    /// offline round-trip test overrides it via [`FmpDataSource::with_base_url`] to
    /// point the adapter at a localhost mock.
    base_url: String,
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
            base_url: FMP_BASE.to_string(),
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

    /// GET one FMP endpoint (a `/path` joined onto [`Self::base_url`]) with the key as
    /// a query param, returning the status and raw body for `interpret_response` to
    /// judge. A transport error (the provider is unreachable) returns `Err` to the
    /// caller, which records it as an `Unavailable` gap rather than failing the scan.
    fn get(&self, path: &str, extra: &[(&str, &str)]) -> Result<(u16, String)> {
        let mut query: Vec<(&str, &str)> = vec![("apikey", self.api_key.as_str())];
        query.extend_from_slice(extra);
        let url = format!("{}{path}", self.base_url);
        crate::http_retry::send_with_retry("FMP", || self.http.get(&url).query(&query))
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
            let disposition = match self.get(FMP_QUOTE_PATH, &[("symbol", symbol)]) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable), // transport — unreachable
            };
            match disposition {
                Disposition::Value(value) => match quotes_from_value(value, fallback_name, unit) {
                    // An empty "no data" 2xx array for an expected symbol is a this-run
                    // absence, not silence: record it so it counts against coverage and
                    // shows in the manifest rather than vanishing from both.
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
    /// recorded as a gap so the agent sees the enrichment was lost on this run; a
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
            self.progress.request_started(
                "FMP",
                GroupKind::IndexPerformance.as_str(),
                symbol,
                name,
            );
            let gaps_before = gaps.len();
            let out_before = out.len();
            let disposition = match self.get(
                FMP_EOD_PATH,
                &[
                    ("symbol", symbol),
                    ("from", from_s.as_str()),
                    ("to", to_s.as_str()),
                ],
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
            let disposition = match self.get(url, &[]) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable),
            };
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

    /// Fetch the earnings calendar over the recent + upcoming-fortnight window in one
    /// call, then filter to large-cap reporters. Additive and non-floor like `movers`: a
    /// permanent absence or an empty / all-filtered window is silent; a this-run failure
    /// (auth / quota / 5xx / transport / malformed) records one `Earnings` gap.
    fn fetch_earnings(&self, back_days: i64, gaps: &mut Vec<DataGap>) -> Vec<EarningsEvent> {
        if self.progress.is_cancelled() {
            return Vec::new();
        }
        let today = Utc::now().date_naive();
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
        let disposition = match self.get(
            FMP_EARNINGS_PATH,
            &[("from", from.as_str()), ("to", to.as_str())],
        ) {
            Ok((status, body)) => interpret_response(status, &body),
            Err(_) => Disposition::Gap(GapReason::Unavailable),
        };
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
    fn fetch_sector_pe_for_exchange(
        &self,
        exchange: &str,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<SectorPe> {
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
        self.progress.request_started(
            "FMP",
            GroupKind::MarketRiskPremium.as_str(),
            series_id,
            name,
        );
        let gaps_before = gaps.len();
        let disposition = match self.get(FMP_RISK_PREMIUM_PATH, &[]) {
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
        );
        out
    }

    /// Per-company financials for the local Portfolio Analysis job
    /// (`docs/portfolio-analysis.md`, `docs/data-sources.md §Local Analysis Suite
    /// Sources`). Distinct from the baseline market scan: this pulls one equity's
    /// quote (price / market cap / shares) and recent EOD price history — the FMP
    /// half of a holding's evidence packet. Each endpoint is fail-soft: a 402 (premium
    /// gate), a transport error, or a malformed body records a tagged gap on the
    /// returned [`CompanyFinancials`] rather than failing, and the dossier fills the
    /// statement lines (revenue, margins, equity) from keyless SEC EDGAR. The
    /// valuation multiples are left for the dossier to derive from market cap + SEC
    /// facts ("compute, don't guess"). One tracker row per actual call.
    fn fetch_quote_and_eod(
        &self,
        symbol: &str,
    ) -> crate::portfolio::engine::CompanyFinancials {
        use crate::portfolio::engine::CompanyFinancials;
        let mut fin = CompanyFinancials {
            symbol: symbol.to_string(),
            ..CompanyFinancials::default()
        };

        // Cancel checkpoint before the first request, mirroring `fetch_quotes`: a cancel
        // already requested skips the network entirely (no request, so no tracker row).
        if self.progress.is_cancelled() {
            fin.gaps.push("company financials skipped (run cancelled)".to_string());
            return fin;
        }

        // 1) Quote: current price, market cap, shares outstanding.
        self.progress
            .request_started("FMP", "company-quote", symbol, "Company quote");
        let quote_disp = match self.get(FMP_QUOTE_PATH, &[("symbol", symbol)]) {
            Ok((status, body)) => interpret_response(status, &body),
            Err(_) => Disposition::Gap(GapReason::Unavailable),
        };
        match quote_disp {
            Disposition::Value(value) => match company_quote_from_value(&value) {
                Some(q) => {
                    fin.current_price = q.price;
                    fin.market_cap = q.market_cap;
                    fin.shares_outstanding = q.shares_outstanding;
                    if q.price.is_none() {
                        fin.gaps.push("FMP quote carried no price".to_string());
                    }
                }
                None => fin.gaps.push("FMP quote was malformed".to_string()),
            },
            Disposition::Gap(reason) => fin
                .gaps
                .push(format!("FMP quote unavailable ({})", reason.as_str())),
        }
        self.progress.request_finished(
            "FMP",
            "company-quote",
            symbol,
            "Company quote",
            if fin.current_price.is_some() { "ok" } else { "empty" },
            None,
        );

        // 2) EOD price history for momentum + volatility.
        // Cancel checkpoint between the two requests: a cancel after the quote skips the
        // EOD call rather than spending it.
        if self.progress.is_cancelled() {
            fin.gaps.push("price history skipped (run cancelled)".to_string());
            return fin;
        }
        let to = Utc::now().date_naive();
        let from = to - Duration::days(COMPANY_EOD_LOOKBACK_DAYS);
        self.progress
            .request_started("FMP", "company-eod", symbol, "Company price history");
        let eod_disp = match self.get(
            FMP_EOD_PATH,
            &[
                ("symbol", symbol),
                ("from", from.format("%Y-%m-%d").to_string().as_str()),
                ("to", to.format("%Y-%m-%d").to_string().as_str()),
            ],
        ) {
            Ok((status, body)) => interpret_response(status, &body),
            Err(_) => Disposition::Gap(GapReason::Unavailable),
        };
        match eod_disp {
            Disposition::Value(value) => match eod_prices_from_value(&value) {
                Ok(history) if !history.is_empty() => fin.price_history = history,
                Ok(_) => fin.gaps.push("FMP price history was empty".to_string()),
                Err(_) => fin.gaps.push("FMP price history was malformed".to_string()),
            },
            Disposition::Gap(reason) => fin
                .gaps
                .push(format!("FMP price history unavailable ({})", reason.as_str())),
        }
        self.progress.request_finished(
            "FMP",
            "company-eod",
            symbol,
            "Company price history",
            if fin.price_history.is_empty() { "empty" } else { "ok" },
            None,
        );

        fin
    }

    /// The full **stock** per-symbol surface: the quote + EOD core plus the v2
    /// target surface — quarterly income prints (the anchor window's trailing
    /// driver source and the TTM statement basis), quarterly cash-flow prints (the
    /// pre-profit overlay's burn / runway / capex legs), the latest balance sheet
    /// (the leverage leg, the P/B denominator, and the runway's liquid-resource
    /// lines), the forward consensus (the driver ladder), and the trailing
    /// dividends (the total-return leg). Each fail-soft with a tagged gap; a
    /// missing consensus later abstains the holding under the named
    /// `no-admissible-driver` floor reason rather than failing here.
    pub fn fetch_company_financials(
        &self,
        symbol: &str,
    ) -> crate::portfolio::engine::CompanyFinancials {
        let mut fin = self.fetch_quote_and_eod(symbol);
        fin.quarterly_income = self.fetch_quarterly_income(symbol, &mut fin.gaps);
        fin.quarterly_cash_flow = self.fetch_quarterly_cash_flow(symbol, &mut fin.gaps);
        let balance = self.fetch_balance_sheet(symbol, &mut fin.gaps);
        fin.total_debt = balance.total_debt;
        fin.total_equity = balance.total_equity;
        fin.cash_and_equivalents = balance.cash_and_equivalents;
        fin.short_term_investments = balance.short_term_investments;
        fin.consensus = self.fetch_analyst_estimates(symbol, &mut fin.gaps);
        fin.ttm_dividends_per_share = self.fetch_ttm_dividends(symbol, &mut fin.gaps);
        fin
    }

    /// The **fund** flavor of the per-symbol pull: the quote + EOD core plus the
    /// dividend history (the TTM distributions the fund-form total return adds) —
    /// the statement / consensus endpoints are stock surface, so a fund never logs
    /// their spurious gaps.
    pub fn fetch_fund_financials(
        &self,
        symbol: &str,
    ) -> crate::portfolio::engine::CompanyFinancials {
        let mut fin = self.fetch_quote_and_eod(symbol);
        fin.ttm_dividends_per_share = self.fetch_ttm_dividends(symbol, &mut fin.gaps);
        fin
    }
}

/// How many days of EOD history the per-company pull requests — long enough for a
/// meaningful momentum and volatility read, short enough to stay one light call.
const COMPANY_EOD_LOOKBACK_DAYS: i64 = 180;

/// The fields the per-company quote contributes, pulled from FMP's `/quote` body.
struct CompanyQuote {
    price: Option<f64>,
    market_cap: Option<f64>,
    shares_outstanding: Option<f64>,
}

/// Shape an FMP `/quote` array body into a [`CompanyQuote`]. `None` only when the body
/// is not the expected non-empty array; individual missing fields stay `None`. Pure,
/// so the contract is unit-testable offline.
fn company_quote_from_value(value: &Value) -> Option<CompanyQuote> {
    let first = value.as_array()?.first()?;
    Some(CompanyQuote {
        price: first.get("price").and_then(Value::as_f64),
        market_cap: first.get("marketCap").and_then(Value::as_f64),
        shares_outstanding: first.get("sharesOutstanding").and_then(Value::as_f64),
    })
}

/// Shape an FMP `/historical-price-eod/light` array body into chronological (oldest
/// first) closing prices. A non-array body is a contract error; rows are sorted by
/// date ascending so the engine's first/last read is a real start/end. Pure.
fn eod_prices_from_value(value: &Value) -> Result<Vec<f64>> {
    let rows = value
        .as_array()
        .context("FMP EOD response did not match the expected array shape")?;
    let mut dated: Vec<(&str, f64)> = Vec::with_capacity(rows.len());
    for row in rows {
        if let (Some(date), Some(price)) = (
            row.get("date").and_then(Value::as_str),
            row.get("price").and_then(Value::as_f64),
        ) {
            dated.push((date, price));
        }
    }
    dated.sort_by(|a, b| a.0.cmp(b.0));
    Ok(dated.into_iter().map(|(_, p)| p).collect())
}

/// Shape an FMP `/historical-price-eod/light` array body into **dated** chronological
/// closes — the deep-history form the v2 anchor join reads when this endpoint serves
/// as the Stooq fallback ([`FmpDataSource::fetch_dated_eod`]). Pure.
fn dated_eod_from_value(value: &Value) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
    let rows = value
        .as_array()
        .context("FMP EOD response did not match the expected array shape")?;
    let mut dated: Vec<crate::portfolio::engine::DatedValue> = rows
        .iter()
        .filter_map(|row| {
            match (
                row.get("date").and_then(Value::as_str),
                row.get("price").and_then(Value::as_f64),
            ) {
                (Some(date), Some(price)) => Some(crate::portfolio::engine::DatedValue {
                    date: date.to_string(),
                    value: price,
                }),
                _ => None,
            }
        })
        .collect();
    dated.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(dated)
}

impl MarketDataSource for FmpDataSource {
    fn baseline_scan(&self, cadence: ReportCadence) -> Result<BaselineMarketData> {
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
        let earnings = self.fetch_earnings(earnings_back_days(cadence.elapsed_days()), &mut gaps);
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
            // FRED owns the macro levels and the economic-release calendar, BLS the
            // labor levels, and CFTC the COT positioning; FMP contributes none of them.
            macro_levels: Vec::new(),
            labor_levels: Vec::new(),
            calendar: Vec::new(),
            cot_positioning: Vec::new(),
            gaps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    #[test]
    fn earnings_back_days_floors_caps_and_scales() {
        // First report (no interval) → the floor.
        assert_eq!(earnings_back_days(None), EARNINGS_BACK_DAYS);
        // Sub-weekly run floors to the default week of context.
        assert_eq!(earnings_back_days(Some(1.0)), EARNINGS_BACK_DAYS);
        // A multi-week run rounds up to ~its interval, above the floor and below the cap.
        assert_eq!(earnings_back_days(Some(10.0)), 10);
        assert_eq!(earnings_back_days(Some(30.0)), 30);
        // A longer gap is capped at FMP's ~1-month free window.
        assert_eq!(earnings_back_days(Some(45.0)), EARNINGS_BACK_MAX_DAYS);
        assert_eq!(earnings_back_days(Some(120.0)), EARNINGS_BACK_MAX_DAYS);
        // Degenerate intervals (clock skew, non-finite) clamp to the floor, never panic.
        assert_eq!(earnings_back_days(Some(-5.0)), EARNINGS_BACK_DAYS);
        assert_eq!(earnings_back_days(Some(f64::NAN)), EARNINGS_BACK_DAYS);
        assert_eq!(earnings_back_days(Some(f64::INFINITY)), EARNINGS_BACK_MAX_DAYS);
    }

    #[test]
    fn interpret_response_covers_the_full_matrix() {
        use GapReason::*;
        // 2xx array (incl. the empty "no data" array) -> a value to shape.
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

        // Explicit skip allowlist: a legitimate per-symbol absence -> OutOfScope gap.
        assert!(matches!(
            interpret_response(402, "Premium Query Parameter"),
            Disposition::Gap(OutOfScope)
        ));
        assert!(matches!(
            interpret_response(404, ""),
            Disposition::Gap(OutOfScope)
        ));

        // Auth -> Rejected; systemic 429/5xx -> Unavailable; request-contract -> Malformed
        // (a 400, e.g. a malformed sector date, degrades to a gap rather than skipping
        // silently).
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
        assert!(matches!(
            interpret_response(200, "not json at all"),
            Disposition::Gap(Malformed)
        ));
    }

    // ---- Offline round trip: adapter -> retry -> interpret -> domain output ----
    //
    // `interpret_response` above pins the status/body matrix as a pure function; these
    // drive the *whole* `get` -> `send_with_retry` -> `interpret_response` ->
    // `quotes_from_value` path against a localhost mock (`crate::test_http`), the path a
    // live FMP key was previously the only way to exercise. `with_base_url` redirects
    // every endpoint at the mock; `fetch_quotes` with a single symbol is the smallest
    // fetch that round-trips one request. Single-reply scripts (no retryable status), so
    // no `BASE_BACKOFF` sleep is incurred — retry mechanics live in `http_retry`'s tests.

    fn test_source(base_url: &str) -> FmpDataSource {
        FmpDataSource::new("test-key".to_string())
            .expect("build adapter")
            .with_base_url(base_url)
    }

    #[test]
    fn company_quote_and_eod_parse_into_financials() {
        // The per-company pull makes six calls — quote, EOD, quarterly income,
        // balance sheet, analyst estimates, dividends — so the mock scripts six
        // replies. Quote carries price/market cap/shares; EOD is returned out of
        // order and must come back chronological so the engine's first/last is
        // honest; the v2 surface (statements, balance sheet, consensus, dividends)
        // parses into its typed fields.
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
                body: r#"[{"date":"2026-03-31","filingDate":"2026-05-01","revenue":95.0e9,
                           "epsDiluted":1.55,"weightedAverageShsOutDil":1.5e10,
                           "netIncome":24.0e9,"grossProfit":44.0e9,"costOfRevenue":51.0e9,
                           "operatingIncome":29.0e9}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2026-03-31","freeCashFlow":20.0e9,
                           "operatingCashFlow":28.0e9,"capitalExpenditure":-8.0e9}]"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"[{"date":"2026-03-31","totalDebt":110.0e9,"totalStockholdersEquity":62.0e9,"totalEquity":63.0e9,
                           "cashAndCashEquivalents":30.0e9,"shortTermInvestments":32.0e9}]"#,
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
        // Chronological: the older 180 first, the newer 195 last.
        assert_eq!(fin.price_history, vec![180.0, 195.0]);
        // The v2 surface parses into its typed fields, the statement lines included.
        assert_eq!(fin.quarterly_income.len(), 1);
        assert_eq!(fin.quarterly_income[0].net_income, Some(24.0e9));
        assert_eq!(fin.quarterly_income[0].gross_profit, Some(44.0e9));
        assert_eq!(fin.quarterly_income[0].cost_of_revenue, Some(51.0e9));
        assert_eq!(fin.quarterly_income[0].operating_income, Some(29.0e9));
        // The cash-flow leg parses into its typed rows (the pre-profit surface).
        assert_eq!(fin.quarterly_cash_flow.len(), 1);
        assert_eq!(fin.quarterly_cash_flow[0].free_cash_flow, Some(20.0e9));
        assert_eq!(fin.quarterly_cash_flow[0].operating_cash_flow, Some(28.0e9));
        assert_eq!(fin.quarterly_cash_flow[0].capex, Some(-8.0e9));
        // The balance sheet fills the leverage leg; equity prefers the
        // stockholders' (parent-only) line over totalEquity. The liquid-resource
        // lines ride beside them.
        assert_eq!(fin.total_debt, Some(110.0e9));
        assert_eq!(fin.total_equity, Some(62.0e9));
        assert_eq!(fin.cash_and_equivalents, Some(30.0e9));
        assert_eq!(fin.short_term_investments, Some(32.0e9));
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
                "/analyst-estimates",
                "/dividends"
            ]
        );
    }

    #[test]
    fn quick_check_adapters_round_trip_price_earnings_and_news() {
        // The quick check's three per-symbol pulls: the bare live price, the
        // earnings rows (newest first), and the since-filtered symbol news.
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
        // Newest first regardless of the feed's order.
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
        // A non-array 200 (schema drift, an error object the gate didn't catch)
        // must surface as `Err`, never read as "no new evidence" — earnings,
        // news, and the strict consensus read alike; the fail-soft dividends
        // read records its gap, so a malformed body can never be mistaken for
        // a confirmed non-payer (a dividend elimination) downstream.
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
    fn company_financials_degrade_to_gaps_on_premium_and_transport_failures() {
        // Quote 402 (premium gate) then EOD malformed body: both degrade to gaps, never
        // a fabricated level, and the engine grades over what SEC supplies instead.
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
        // Seven endpoints, seven tagged gaps — the v2-surface calls and the
        // pre-profit cash-flow leg degrade the same way the quote and EOD do.
        assert_eq!(fin.gaps.len(), 7, "seven failed pulls, seven gaps: {:?}", fin.gaps);
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
        // A premium-gated endpoint replies 402: the wire path must classify it as an
        // OutOfScope gap and yield no quote — the status/body split rides the real socket.
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
        // The requested symbol's unit rides onto the quote from the table, not the wire.
        assert_eq!(quotes[0].unit, "index points");

        // No name -> local fallback; legacy `changesPercentage` accepted.
        let v2: Value =
            serde_json::from_str(r#"[{"symbol":"^DJI","price":40000.0,"changesPercentage":-1.5}]"#)
                .unwrap();
        let q2 = quotes_from_value(v2, "Dow Jones", "index points").unwrap();
        assert_eq!(q2[0].name, "Dow Jones");
        assert!((q2[0].change.value + 1.5).abs() < 1e-9);

        // An empty array is "no quotes", not an error.
        assert!(
            quotes_from_value(serde_json::from_str("[]").unwrap(), "x", "index points")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn quotes_from_value_requires_price_and_change() {
        // A required field absent (schema drift / partial response) fails the parse —
        // neither a false 0.0 nor a silent skip; the loop records a Malformed gap.
        let no_price: Value =
            serde_json::from_str(r#"[{"symbol":"^GSPC","changePercentage":0.4}]"#).unwrap();
        assert!(quotes_from_value(no_price, "x", "index points").is_err());
        let no_change: Value =
            serde_json::from_str(r#"[{"symbol":"^GSPC","price":5500.0}]"#).unwrap();
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
        // A run on a Sunday: candidates skip Sat/Sun and start at the prior
        // Friday, then walk back over weekdays only.
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
        assert!(
            (p.weekly_pct - 10.0).abs() < 1e-9,
            "weekly {}",
            p.weekly_pct
        );

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
        let body = serde_json::json!([{"symbol":"MSFT","price":410.0,"changePercentage":1.5,"exchange":"NASDAQ"}]);
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
        // Regression: bare "bull "/"bear " markers would drop real companies. Build-A-Bear
        // stays; the leveraged directional ETF is still caught (by "direxion" / "3x" /
        // " etf"), so dropping the bare markers cost no coverage.
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
        // The band `(0.0, SECTOR_PE_MAX]`: a non-positive aggregate (FMP's 0.0 / a negative for
        // a sector with no positive summed earnings) and one inflated past the ceiling both drop
        // the *pe* to `None` — but the (sector, exchange) row survives so the model still sees
        // the sector was scanned. An in-band value rides through as `Some`.
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
        assert_eq!(
            out.first().unwrap().industry,
            format!("Ind{}", 2 * INDUSTRY_TOP_N + 3)
        );
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
        // An industry near an earnings trough divides by a denominator approaching zero from
        // above, inflating its aggregate P/E past any plausible level (a live run surfaced
        // pe ≈ 461). Anything above INDUSTRY_PE_MAX is dropped — the symmetric upper bound to
        // the non-positive drop — so the join yields None rather than an absurd "expensive"
        // multiple, while an in-band aggregate survives.
        let v = serde_json::json!([
            {"industry":"Software Application","exchange":"NASDAQ","pe":461.0},
            {"industry":"Semiconductors","exchange":"NASDAQ","pe":63.9}
        ]);
        let map = industry_pe_map_from_value(v, "NASDAQ").unwrap();
        assert_eq!(map.len(), 1);
        assert!(!map.contains_key(&("Software Application".to_string(), "NASDAQ".to_string())));
        // The join carries None for the dropped industry, Some for the in-band one.
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
        // The upper bound is inclusive: `(0.0, INDUSTRY_PE_MAX]`. An aggregate sitting exactly
        // on INDUSTRY_PE_MAX is still a plausible valuation and is kept; the first multiple
        // *past* it is dropped. This pins the gate's `<=` against an accidental flip to `<`
        // (which would silently drop the boundary value) if the ceiling is ever tuned.
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

    /// The grade-band shadow-tune's calibration-surface refresh (the slice's step 6,
    /// a user-approved bounded live spend): re-derive the statement-based metric
    /// surface for the exported run's priced stocks through the NEW fetch path —
    /// quote + quarterly income + balance sheet, three calls per stock, the TTM
    /// statement basis + balance-sheet leg applied exactly as the live job would —
    /// and write symbol → `ComputedMetrics` JSON for the band sweep to read beside
    /// the as-persisted surface. Volatility / trailing return carry over from the
    /// persisted audit (the price surface didn't change basis). Call count is
    /// printed for the evidence record.
    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY, MARKET_SIGNAL_RUN_JSON, MARKET_SIGNAL_REFRESHED_METRICS_OUT"]
    fn probe_refresh_statement_surface_for_band_tune() {
        let run_path = std::env::var("MARKET_SIGNAL_RUN_JSON").expect("MARKET_SIGNAL_RUN_JSON");
        let out_path = std::env::var("MARKET_SIGNAL_REFRESHED_METRICS_OUT")
            .expect("MARKET_SIGNAL_REFRESHED_METRICS_OUT");
        let run: crate::portfolio::PortfolioRun =
            serde_json::from_str(&std::fs::read_to_string(&run_path).unwrap()).unwrap();
        let src = FmpDataSource::from_env().expect("FMP_API_KEY set");
        let mut refreshed: std::collections::BTreeMap<
            String,
            crate::portfolio::engine::ComputedMetrics,
        > = Default::default();
        let mut calls = 0usize;
        for v in &run.verdicts {
            let crate::portfolio::VerdictDisposition::Priced(g) = &v.disposition else {
                continue;
            };
            if g.fund_class_label.is_some() {
                continue;
            }
            let audit = run
                .audit
                .iter()
                .find(|a| a.symbol == v.symbol)
                .expect("audit row");
            let mut fin = crate::portfolio::engine::CompanyFinancials {
                symbol: v.symbol.clone(),
                ..Default::default()
            };
            if let Ok((status, body)) = src.get(FMP_QUOTE_PATH, &[("symbol", v.symbol.as_str())])
            {
                if let Disposition::Value(value) = interpret_response(status, &body) {
                    if let Some(q) = company_quote_from_value(&value) {
                        fin.market_cap = q.market_cap;
                        fin.current_price = q.price;
                    }
                }
            }
            let mut gaps = vec![];
            fin.quarterly_income = src.fetch_quarterly_income(&v.symbol, &mut gaps);
            let balance = src.fetch_balance_sheet(&v.symbol, &mut gaps);
            fin.total_debt = balance.total_debt;
            fin.total_equity = balance.total_equity;
            calls += 3;
            let basis = crate::portfolio::dossier::apply_ttm_statement_basis(&mut fin);
            let merged = crate::portfolio::dossier::merge_financials(
                fin,
                &crate::sec::CompanyFacts::default(),
                basis,
            );
            let mut m = crate::portfolio::engine::compute_metrics(&merged);
            m.return_volatility = audit.metrics.return_volatility;
            m.trailing_return = audit.metrics.trailing_return;
            eprintln!(
                "{}: ttm={} nm={:?} gm={:?} rg={:?} de={:?} pe={:?} ps={:?} pb={:?} gaps={:?}",
                v.symbol,
                basis,
                m.net_margin,
                m.gross_margin,
                m.revenue_growth,
                m.debt_to_equity,
                m.pe_ratio,
                m.ps_ratio,
                m.pb_ratio,
                gaps
            );
            refreshed.insert(v.symbol.clone(), m);
            // Politeness pacing between symbols — a probe, not a throughput race.
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        std::fs::write(&out_path, serde_json::to_string_pretty(&refreshed).unwrap()).unwrap();
        eprintln!(
            "{} stocks refreshed, {} HTTP calls → {}",
            refreshed.len(),
            calls,
            out_path
        );
    }

    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY"]
    fn fmp_baseline_smoke() {
        let src = FmpDataSource::from_env().expect("FMP_API_KEY set");
        let data = src.baseline_scan(ReportCadence::default()).expect("live baseline scan");

        // Print the resolved mapping so a maintainer can confirm each symbol /
        // endpoint actually came back (run with `-- --ignored --nocapture`); the
        // offline tests can only check fixture shapes, not the live symbols.
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

    /// Industry-P/E distribution probe — dumps the live per-exchange aggregate P/E
    /// distribution so [`INDUSTRY_PE_MAX`] can be re-tuned from real data rather than
    /// guessed. Mirrors `tuning_freshness_headroom_probe` on the FRED side: it only
    /// reports (no assertions), and unlike `fmp_baseline_smoke` it is a calibration aid,
    /// not a gate. For each board it walks the same weekday candidates production uses,
    /// takes the first date that resolves, and prints every industry's raw aggregate P/E
    /// sorted high→low, flagging which fall above the current ceiling (`> INDUSTRY_PE_MAX`,
    /// the trough-artifact band) and which are non-positive (`<= 0.0`, no positive aggregate
    /// earnings) — both of which the production map drops to `None`. Set the ceiling above
    /// the highest *plausible* in-band aggregate but below the artifact cluster. Hits the
    /// live API (≤2 calls per board, trivial against the 250/day free cap); run once per
    /// change:
    ///   source ~/.config/market-signal/keys.env && cargo test --manifest-path \
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
                // The probe deliberately bypasses the *band* filter (the `>0` / `<=ceiling` drop
                // it exists to calibrate) so the full artifact tail stays visible — but it must
                // still honor the *exchange* guard production enforces (`industry_pe_map_from_value`
                // bails on an off-board row), or a response where FMP ignored the `exchange` filter
                // (a no-`exchange` call silently defaults to NASDAQ) would pollute one board's
                // distribution with the other's and tune the ceiling against corrupted evidence.
                // Keep only matching-board rows; surface any off-board count so misbehavior is loud.
                let total = raws.len();
                let mut rows: Vec<(String, f64)> = raws
                    .into_iter()
                    .filter(|r| r.exchange == *exchange)
                    .map(|r| (r.industry, r.pe))
                    .collect();
                let off_board = total - rows.len();
                // Sort high→low so the artifact tail is obvious at the top.
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

    /// Sector-P/E distribution probe — the sector-cut sibling of
    /// `tuning_industry_pe_distribution_probe`, so [`SECTOR_PE_MAX`] can be re-tuned from real
    /// data rather than the conservative industry-shared 120.0 it ships at. Same shape: report
    /// only (no assertions, a calibration aid not a gate), walk the production weekday
    /// candidates per board, take the first date that resolves, and print every sector's raw
    /// aggregate P/E sorted high→low, flagging those above the current ceiling (the
    /// trough-artifact band) and the non-positive ones (no positive aggregate earnings) — both
    /// of which production drops to `None`. A sector aggregate sums over far more constituents
    /// than an industry's, so expect a *tighter* plausible band and a *rarer* artifact tail;
    /// set the ceiling above the highest plausible in-band sector aggregate, below any artifact
    /// cluster. Hits the live API (≤2 calls per board, trivial against the 250/day free cap);
    /// run once per change:
    ///   source ~/.config/market-signal/keys.env && cargo test --manifest-path \
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
                // Bypass the *band* filter (the drop this probe exists to calibrate) but honor
                // the *exchange* guard production enforces (`sector_pe_from_value` bails on an
                // off-board row), or a response where FMP ignored the `exchange` filter would
                // pollute one board's distribution with the other's. Surface any off-board count.
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
        probe(
            "most-actives (plural alias)",
            &format!("{base}/most-actives"),
            &[],
        );
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
        probe(
            "market-risk-premium",
            &format!("{base}/market-risk-premium"),
            &[],
        );
        // Commodities: gold already free via /quote; do copper/silver resolve on free?
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
        // 1-call consolidation candidate: does it cover the 4 indices (and ^VIX)?
        probe("all-index-quotes", &format!("{base}/all-index-quotes"), &[]);

        // --- Corrected paths for the endpoints that 404'd above (404 = wrong path,
        // not premium; premium is 402, which none of these returned) ---
        // Constituent lists use the `*-constituent` paths, not the bare index name.
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
        // Batch index quotes — the likely real name of the 1-call index consolidation.
        probe(
            "batch-index-quotes",
            &format!("{base}/batch-index-quotes"),
            &[],
        );
        // Copper / silver via the generic quote endpoint we already use for GCUSD gold.
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

    /// Free-vs-premium probe for FMP's news endpoints — the Step-7 company-news
    /// follow-on. `fmp-articles` (FMP's own ticker-tagged editorial feed) is the
    /// expected-free candidate (per docs screenshots, never live-verified); the
    /// `news/*` family was recorded as premium from docs research, also never probed.
    /// Same conventions as `fmp_freetier_probe`: 200 ≈ free, 402 = premium, 404 =
    /// wrong path. Prints body samples so the adapter's field mapping (title / link /
    /// site / date / content / tickers) and pagination behavior can be read off the
    /// wire. Hits the live API (~5 one-shot calls, trivial against the 250/day cap);
    /// run once:
    ///   source ~/.config/market-signal/keys.env && cargo test --manifest-path \
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
        // The candidate: FMP's own editorial articles. Probe with explicit paging
        // params so the adapter knows whether `page`/`limit` are honored.
        probe(
            "fmp-articles (page+limit)",
            &format!("{base}/fmp-articles"),
            &[("page", "0"), ("limit", "5")],
        );
        // Re-confirm the news/* family tiers (recorded premium from docs research,
        // never live-probed).
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
}

// ---- Local-suite per-holding surface (`docs/data-sources.md §Portfolio Analysis
// — endpoint surface`): the statement / consensus / dividend / fund endpoints the
// fund slice widened the adapter with. Each is fail-soft — a premium gate, transport
// error, or malformed body records a tagged gap rather than failing — and every
// actual call streams one tracker row.

/// FMP endpoint paths added by the fund slice (all on the `/stable` base).
const FMP_INCOME_QUARTERLY_PATH: &str = "/income-statement";
/// Per-symbol earnings rows (actual vs estimate) — the quick check's
/// new-earnings-actual evidence leg (`docs/portfolio-analysis.md` §Starting
/// parameters; distinct from the report's market-wide `/earnings-calendar`).
const FMP_SYMBOL_EARNINGS_PATH: &str = "/earnings";
/// Symbol-scoped stock news — the quick check's qualifying-news-seed leg, pulled
/// only for holdings carrying a standing technology-class falsifier.
const FMP_NEWS_STOCK_SYMBOL_PATH: &str = "/news/stock";
/// How many per-symbol earnings rows the quick check reads — enough to cover the
/// window since the last full pass with room to spare.
const SYMBOL_EARNINGS_LIMIT: &str = "8";
/// News items requested per tech-flagged holding.
const SYMBOL_NEWS_LIMIT: &str = "20";
const FMP_BALANCE_SHEET_PATH: &str = "/balance-sheet-statement";
/// Quarterly cash-flow statements — the pre-profit overlay's TTM burn / runway /
/// capex source (`docs/portfolio-analysis.md` §Starting parameters); stock surface
/// only, like the other statements.
const FMP_CASH_FLOW_PATH: &str = "/cash-flow-statement";
const FMP_ANALYST_ESTIMATES_PATH: &str = "/analyst-estimates";
const FMP_DIVIDENDS_PATH: &str = "/dividends";
/// The company profile — the outcome episodes' sector-label source
/// (`docs/portfolio-analysis.md §Outcome learning` — the entry-stamped sector
/// identity).
const FMP_PROFILE_PATH: &str = "/profile";
/// Dividend rows requested for the label-time history pull — a monthly payer over
/// a 13-month window needs ~15; the margin covers specials.
const DIVIDEND_HISTORY_LIMIT: &str = "60";

/// The dividend-history gap's stable prefix ([`FmpDataSource::fetch_ttm_dividends`]):
/// the quick check matches it to tell a failed retrieval (gap recorded, keep the
/// stored payout leg) from a genuine non-payer (`None` with no gap — a real
/// dividend elimination that must reach the hurdle as zero).
pub const DIVIDENDS_GAP_PREFIX: &str = "FMP dividends unavailable";

/// Stable prefixes of the fund-weightings gap messages
/// ([`FmpDataSource::fetch_fund_data`]): the quick check treats these legs as
/// bearing on **equity** funds alone — no series in the closed ledger surface
/// reads exposure, so a non-equity fund's empty equity weightings are the
/// expected shape, never a failed leg
/// (`docs/portfolio-analysis.md` §Evidence floor, §The quick check).
pub const FUND_SECTOR_WEIGHTS_GAP_PREFIX: &str = "FMP sector weightings";
pub const FUND_COUNTRY_WEIGHTS_GAP_PREFIX: &str = "FMP country weightings";

/// The `etf/info` gap's stable prefix ([`FmpDataSource::fetch_fund_data`]): the
/// quick check's coarse mandate comparison runs only when this leg is healthy —
/// an unreadable mandate could fake an asset-class transition.
pub const FUND_INFO_GAP_PREFIX: &str = "FMP etf/info";
const FMP_ETF_INFO_PATH: &str = "/etf/info";
const FMP_ETF_SECTOR_WEIGHTS_PATH: &str = "/etf/sector-weightings";
const FMP_ETF_COUNTRY_WEIGHTS_PATH: &str = "/etf/country-weightings";
const FMP_SECTOR_PE_SNAPSHOT_PATH: &str = "/sector-pe-snapshot";
const FMP_HISTORICAL_SECTOR_PE_PATH: &str = "/historical-sector-pe";

/// Quarters of income-statement history requested — the v2 anchor window (12) plus
/// the four extra quarters its oldest TTM print needs.
const INCOME_QUARTERS_LIMIT: &str = "16";

/// Quarters of cash-flow history requested — the pre-profit TTM window (4) plus a
/// year of slack so a missing newest print doesn't strand the sum.
const CASH_FLOW_QUARTERS_LIMIT: &str = "8";

impl FmpDataSource {
    /// One suite GET with a tracker row and the shared fail-soft disposition.
    fn suite_get(
        &self,
        kind: &str,
        symbol: &str,
        label: &str,
        path: &str,
        extra: &[(&str, &str)],
    ) -> Disposition {
        if self.progress.is_cancelled() {
            return Disposition::Gap(GapReason::Unavailable);
        }
        self.progress.request_started("FMP", kind, symbol, label);
        let disposition = match self.get(path, extra) {
            Ok((status, body)) => interpret_response(status, &body),
            Err(_) => Disposition::Gap(GapReason::Unavailable),
        };
        let status = match &disposition {
            Disposition::Value(_) => "ok",
            Disposition::Gap(_) => "empty",
        };
        self.progress
            .request_finished("FMP", kind, symbol, label, status, None);
        disposition
    }

    /// Quarterly income prints (newest first) — the v2 anchor window's trailing
    /// driver source. Fail-soft: a gap leaves the list empty with a tagged reason.
    pub fn fetch_quarterly_income(
        &self,
        symbol: &str,
        gaps: &mut Vec<String>,
    ) -> Vec<crate::portfolio::engine::QuarterlyIncomeRow> {
        match self.suite_get(
            "company-income-q",
            symbol,
            "Quarterly income statements",
            FMP_INCOME_QUARTERLY_PATH,
            &[
                ("symbol", symbol),
                ("period", "quarter"),
                ("limit", INCOME_QUARTERS_LIMIT),
            ],
        ) {
            Disposition::Value(value) => match quarterly_income_from_value(&value) {
                rows if !rows.is_empty() => rows,
                _ => {
                    gaps.push("FMP quarterly income statements were empty".to_string());
                    vec![]
                }
            },
            Disposition::Gap(reason) => {
                gaps.push(format!(
                    "FMP quarterly income statements unavailable ({})",
                    reason.as_str()
                ));
                vec![]
            }
        }
    }

    /// The latest quarterly balance sheet — the risk sub-score's leverage leg
    /// (`totalDebt` / equity) and the P/B denominator, FMP-first with the SEC annual
    /// equity as fallback (`docs/portfolio-analysis.md` §Starting parameters — the
    /// grade-band slice's F5 closure; before it, `total_debt` had no source at all and
    /// the risk read rested on volatility alone). Fail-soft: a gap leaves both `None`
    /// with a tagged reason.
    pub fn fetch_balance_sheet(&self, symbol: &str, gaps: &mut Vec<String>) -> BalanceSheetLines {
        match self.suite_get(
            "company-balance",
            symbol,
            "Balance sheet",
            FMP_BALANCE_SHEET_PATH,
            &[("symbol", symbol), ("period", "quarter"), ("limit", "1")],
        ) {
            Disposition::Value(value) => match balance_sheet_from_value(&value) {
                Some(lines) => lines,
                None => {
                    gaps.push("FMP balance sheet was empty or malformed".to_string());
                    BalanceSheetLines::default()
                }
            },
            Disposition::Gap(reason) => {
                gaps.push(format!("FMP balance sheet unavailable ({})", reason.as_str()));
                BalanceSheetLines::default()
            }
        }
    }

    /// Quarterly cash-flow prints (newest first) — the pre-profit overlay's TTM burn /
    /// runway / capex-intensity source (`docs/portfolio-analysis.md` §Starting
    /// parameters). Fail-soft: a gap leaves the list empty with a tagged reason.
    pub fn fetch_quarterly_cash_flow(
        &self,
        symbol: &str,
        gaps: &mut Vec<String>,
    ) -> Vec<crate::portfolio::engine::QuarterlyCashFlowRow> {
        match self.suite_get(
            "company-cashflow-q",
            symbol,
            "Quarterly cash-flow statements",
            FMP_CASH_FLOW_PATH,
            &[
                ("symbol", symbol),
                ("period", "quarter"),
                ("limit", CASH_FLOW_QUARTERS_LIMIT),
            ],
        ) {
            Disposition::Value(value) => match quarterly_cash_flow_from_value(&value) {
                rows if !rows.is_empty() => rows,
                _ => {
                    gaps.push("FMP quarterly cash-flow statements were empty".to_string());
                    vec![]
                }
            },
            Disposition::Gap(reason) => {
                gaps.push(format!(
                    "FMP quarterly cash-flow statements unavailable ({})",
                    reason.as_str()
                ));
                vec![]
            }
        }
    }

    /// The NTM forward consensus (the time-weighted blend of the two nearest coming
    /// fiscal-year rows — [`consensus_from_value`]) — the v2 driver ladder's source.
    /// Fail-soft to `None` with a tagged gap.
    pub fn fetch_analyst_estimates(
        &self,
        symbol: &str,
        gaps: &mut Vec<String>,
    ) -> Option<crate::portfolio::engine::ConsensusEstimate> {
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        match self.suite_get(
            "company-estimates",
            symbol,
            "Analyst estimates",
            FMP_ANALYST_ESTIMATES_PATH,
            &[("symbol", symbol), ("period", "annual"), ("limit", "6")],
        ) {
            Disposition::Value(value) => match consensus_from_value(&value, &today) {
                Some(c) => Some(c),
                None => {
                    gaps.push(
                        "FMP analyst estimates carried no forward-dated consensus \
                         (a past fiscal-year row is never used as forward)"
                            .to_string(),
                    );
                    None
                }
            },
            Disposition::Gap(reason) => {
                gaps.push(format!("FMP analyst estimates unavailable ({})", reason.as_str()));
                None
            }
        }
    }

    /// The strict form of [`Self::fetch_analyst_estimates`] for the quick check's
    /// revision preflight: a failed retrieval is `Err` (the caller types the family
    /// `unknown`), while a successful body carrying no forward-dated consensus is an
    /// honest `Ok(None)` — the typed split the fail-soft gap-list form can't offer
    /// without string-matching its message text.
    pub fn fetch_analyst_estimates_strict(
        &self,
        symbol: &str,
    ) -> Result<Option<crate::portfolio::engine::ConsensusEstimate>> {
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        match self.suite_get(
            "company-estimates",
            symbol,
            "Analyst estimates",
            FMP_ANALYST_ESTIMATES_PATH,
            &[("symbol", symbol), ("period", "annual"), ("limit", "6")],
        ) {
            Disposition::Value(value) => {
                // Strict shape check: a malformed 200 must surface as a failed
                // retrieval (family `unknown`), never read as "no consensus".
                if !value.is_array() {
                    anyhow::bail!(
                        "FMP analyst estimates returned a non-array body — malformed or drifted response"
                    );
                }
                Ok(consensus_from_value(&value, &today))
            }
            Disposition::Gap(reason) => {
                anyhow::bail!("FMP analyst estimates unavailable ({})", reason.as_str())
            }
        }
    }

    /// Trailing-twelve-month dividends per share — the forward-dividend estimate the
    /// twelve-month total return adds. `None` (with no gap) for a non-payer; a failed
    /// call records the gap.
    pub fn fetch_ttm_dividends(&self, symbol: &str, gaps: &mut Vec<String>) -> Option<f64> {
        let today = Utc::now().date_naive();
        match self.suite_get(
            "company-dividends",
            symbol,
            "Dividend history",
            FMP_DIVIDENDS_PATH,
            &[("symbol", symbol), ("limit", "12")],
        ) {
            // Any unreadable body — non-array, a dateless row, an in-window row
            // with a non-numeric amount — must record the gap: `None` with no
            // gap is the confirmed-non-payer contract, and a drifted body must
            // never read as a dividend elimination downstream.
            Disposition::Value(value) => match ttm_dividends_from_value(&value, today) {
                Ok(v) => v,
                Err(e) => {
                    gaps.push(format!("{DIVIDENDS_GAP_PREFIX} ({e})"));
                    None
                }
            },
            Disposition::Gap(reason) => {
                gaps.push(format!("{DIVIDENDS_GAP_PREFIX} ({})", reason.as_str()));
                None
            }
        }
    }

    /// The profile's identity fields for a stock — one fetch feeding the
    /// listing-resolution guard (issuer name / exchange —
    /// `docs/portfolio-analysis.md §Asset eligibility`) and the outcome episodes'
    /// entry-stamped sector identity. Fail-soft, split three ways: a resolved
    /// body; a definitive 2xx no-such-symbol (`Unresolved` — FMP answered and
    /// knows no profile); or any gate, transport failure, or unreadable body
    /// (`Unverified` — never mistaken for a missing listing).
    pub fn fetch_profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
        use crate::portfolio::listing::ProfileLookup;
        match self.suite_get(
            "company-profile",
            symbol,
            "Company profile (identity)",
            FMP_PROFILE_PATH,
            &[("symbol", symbol)],
        ) {
            Disposition::Value(value) => profile_identity_from_value(&value),
            Disposition::Gap(reason) => {
                ProfileLookup::Unverified(format!("FMP profile unavailable ({})", reason.as_str()))
            }
        }
    }

    /// The dated per-share dividend history within `[from, to]` — the outcome
    /// labels' total-return leg (`docs/portfolio-analysis.md §Outcome learning`:
    /// the window's cash dividends summed without reinvestment). Strict like the
    /// TTM read: a malformed or drifted body is `Err` (the caller records the
    /// labeled price-only fallback), never a silent zero that would read as a
    /// dividend elimination.
    pub fn fetch_dividend_history(
        &self,
        symbol: &str,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
        match self.suite_get(
            "company-dividends-history",
            symbol,
            "Dividend history (outcome labels)",
            FMP_DIVIDENDS_PATH,
            &[("symbol", symbol), ("limit", DIVIDEND_HISTORY_LIMIT)],
        ) {
            Disposition::Value(value) => dividend_history_from_value(&value, from, to),
            Disposition::Gap(reason) => {
                anyhow::bail!("FMP dividend history unavailable ({})", reason.as_str())
            }
        }
    }

    /// Deep **dated** daily closes over `lookback_days` — the anchor join's fallback
    /// price source when Stooq, the primary deep-history source, is throttled or
    /// unavailable (`docs/data-sources.md §Stooq`). Spent only on the fallback path,
    /// so the dispersal principle — the bulk per-holding price load stays off the
    /// shared FMP key — holds.
    pub fn fetch_dated_eod(
        &self,
        symbol: &str,
        lookback_days: i64,
    ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
        let to = Utc::now().date_naive();
        let from = (to - Duration::days(lookback_days))
            .format("%Y-%m-%d")
            .to_string();
        let to = to.format("%Y-%m-%d").to_string();
        match self.suite_get(
            "company-eod-deep",
            symbol,
            "Deep price history (Stooq fallback)",
            FMP_EOD_PATH,
            &[("symbol", symbol), ("from", &from), ("to", &to)],
        ) {
            Disposition::Value(value) => dated_eod_from_value(&value),
            Disposition::Gap(reason) => {
                anyhow::bail!("FMP dated EOD unavailable ({})", reason.as_str())
            }
        }
    }

    /// The bare per-symbol live price — the quick check's per-holding price refresh
    /// (`docs/portfolio-analysis.md` §The quick check). `Err` on a gate, transport
    /// failure, or a body carrying no price, so the caller types the family
    /// `unknown` rather than clearing it silently.
    pub fn fetch_live_price(&self, symbol: &str) -> Result<f64> {
        match self.suite_get(
            "quick-quote",
            symbol,
            "Live quote",
            FMP_QUOTE_PATH,
            &[("symbol", symbol)],
        ) {
            Disposition::Value(value) => company_quote_from_value(&value)
                .and_then(|q| q.price)
                .with_context(|| format!("FMP quote carried no price for {symbol}")),
            Disposition::Gap(reason) => {
                anyhow::bail!("FMP quote unavailable ({})", reason.as_str())
            }
        }
    }

    /// Per-symbol earnings rows, newest first — the quick check's
    /// new-earnings-actual evidence leg. `Err` on a failed retrieval (the caller
    /// types the family `unknown`); an empty list is an honest no-rows read.
    pub fn fetch_symbol_earnings(&self, symbol: &str) -> Result<Vec<SymbolEarningsRow>> {
        match self.suite_get(
            "quick-earnings",
            symbol,
            "Earnings rows",
            FMP_SYMBOL_EARNINGS_PATH,
            &[("symbol", symbol), ("limit", SYMBOL_EARNINGS_LIMIT)],
        ) {
            Disposition::Value(value) => symbol_earnings_from_value(&value),
            Disposition::Gap(reason) => {
                anyhow::bail!("FMP earnings unavailable ({})", reason.as_str())
            }
        }
    }

    /// Symbol-scoped stock news since `from` (ISO date) — the quick check's
    /// qualifying-news-seed leg, pulled **only** for holdings carrying a standing
    /// technology-class falsifier. `Err` on a failed retrieval.
    pub fn fetch_symbol_news_since(
        &self,
        symbol: &str,
        from: &str,
    ) -> Result<Vec<SymbolNewsItem>> {
        match self.suite_get(
            "quick-news",
            symbol,
            "Symbol news",
            FMP_NEWS_STOCK_SYMBOL_PATH,
            &[
                ("symbols", symbol),
                ("from", from),
                ("limit", SYMBOL_NEWS_LIMIT),
            ],
        ) {
            Disposition::Value(value) => symbol_news_from_value(&value, from),
            Disposition::Gap(reason) => {
                anyhow::bail!("FMP news/stock unavailable ({})", reason.as_str())
            }
        }
    }

    /// The per-fund metadata surface: `etf/info` plus the sector / country
    /// weightings (`docs/portfolio-analysis.md` §Asset eligibility). Each endpoint
    /// fail-softs to a tagged gap on the returned record.
    pub fn fetch_fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
        let mut fund = crate::portfolio::fund::FundData {
            symbol: symbol.to_string(),
            ..Default::default()
        };
        match self.suite_get(
            "fund-info",
            symbol,
            "Fund metadata",
            FMP_ETF_INFO_PATH,
            &[("symbol", symbol)],
        ) {
            Disposition::Value(value) => fund_info_into(&value, &mut fund),
            Disposition::Gap(reason) => fund
                .gaps
                .push(format!("{FUND_INFO_GAP_PREFIX} unavailable ({})", reason.as_str())),
        }
        match self.suite_get(
            "fund-sectors",
            symbol,
            "Fund sector weightings",
            FMP_ETF_SECTOR_WEIGHTS_PATH,
            &[("symbol", symbol)],
        ) {
            Disposition::Value(value) => {
                fund.sector_weights = weights_from_value(&value, "sector");
                if fund.sector_weights.is_empty() {
                    fund.gaps
                        .push(format!("{FUND_SECTOR_WEIGHTS_GAP_PREFIX} were empty"));
                }
            }
            Disposition::Gap(reason) => fund.gaps.push(format!(
                "{FUND_SECTOR_WEIGHTS_GAP_PREFIX} unavailable ({})",
                reason.as_str()
            )),
        }
        match self.suite_get(
            "fund-countries",
            symbol,
            "Fund country weightings",
            FMP_ETF_COUNTRY_WEIGHTS_PATH,
            &[("symbol", symbol)],
        ) {
            Disposition::Value(value) => {
                fund.country_weights = weights_from_value(&value, "country");
                if fund.country_weights.is_empty() {
                    fund.gaps
                        .push(format!("{FUND_COUNTRY_WEIGHTS_GAP_PREFIX} were empty"));
                }
            }
            Disposition::Gap(reason) => fund.gaps.push(format!(
                "{FUND_COUNTRY_WEIGHTS_GAP_PREFIX} unavailable ({})",
                reason.as_str()
            )),
        }
        fund
    }

    /// The per-sector aggregate P/E snapshot for one exchange (run-level, shared
    /// across funds; `docs/data-sources.md` — one call per exchange). `date` is the
    /// most recent weekday, computed by the caller; a weekday market holiday still
    /// returns a full snapshot (2026-07-03 and Juneteenth served with carried
    /// values — live-verified 2026-07-16), so the keying needs no holiday case.
    pub fn fetch_sector_pe_snapshot(
        &self,
        exchange: &str,
        date: &str,
    ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        match self.suite_get(
            "sector-pe",
            exchange,
            "Sector P/E snapshot",
            FMP_SECTOR_PE_SNAPSHOT_PATH,
            &[("exchange", exchange), ("date", date)],
        ) {
            Disposition::Value(value) => Ok(sector_pe_rows_from_value(&value, exchange)),
            Disposition::Gap(reason) => anyhow::bail!(
                "FMP sector-pe-snapshot unavailable for {exchange} ({})",
                reason.as_str()
            ),
        }
    }

    /// The trailing per-sector P/E history for one sector × exchange (memoized by
    /// the caller across funds — `docs/data-sources.md`, the historical-sector-pe
    /// row's cardinality).
    pub fn fetch_historical_sector_pe(
        &self,
        sector: &str,
        exchange: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        match self.suite_get(
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
        ) {
            Disposition::Value(value) => Ok(sector_pe_rows_from_value(&value, exchange)),
            Disposition::Gap(reason) => anyhow::bail!(
                "FMP historical-sector-pe unavailable for {sector}/{exchange} ({})",
                reason.as_str()
            ),
        }
    }
}

/// The balance-sheet lines the per-holding pull consumes (`fetch_balance_sheet`):
/// the leverage / P/B legs plus the pre-profit runway numerator's liquid-resource
/// lines (`docs/portfolio-analysis.md` §Starting parameters).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BalanceSheetLines {
    pub total_debt: Option<f64>,
    pub total_equity: Option<f64>,
    pub cash_and_equivalents: Option<f64>,
    pub short_term_investments: Option<f64>,
}

/// One per-symbol earnings row (`fetch_symbol_earnings`) — the announcement date and
/// the actual-vs-estimate legs the quick check's evidence-event leg reads.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SymbolEarningsRow {
    /// The announcement date, ISO.
    pub date: String,
    pub eps_actual: Option<f64>,
    pub eps_estimated: Option<f64>,
    pub revenue_actual: Option<f64>,
}

/// Shape an FMP `/earnings?symbol=` array body into rows, newest first. A row
/// without a date is skipped (nothing to key the event on). A non-array 200 body
/// is schema drift or a malformed response — `Err`, never an empty success, so
/// the quick check types the family `unknown` rather than reading "no new
/// evidence" off a body it couldn't interpret.
fn symbol_earnings_from_value(value: &Value) -> Result<Vec<SymbolEarningsRow>> {
    let Some(rows) = value.as_array() else {
        anyhow::bail!("FMP earnings returned a non-array body — malformed or drifted response");
    };
    let mut out: Vec<SymbolEarningsRow> = rows
        .iter()
        .filter_map(|row| {
            let date = row.get("date").and_then(Value::as_str)?.to_string();
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

/// One symbol-scoped news item (`fetch_symbol_news_since`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SymbolNewsItem {
    /// `YYYY-MM-DD HH:MM:SS` as FMP serves it; date-prefix comparable.
    pub published_date: String,
    pub title: String,
    pub site: Option<String>,
}

/// Shape an FMP `/news/stock?symbols=` array body, keeping items published on or
/// after `from` (belt and braces over the server-side `from` filter — the leg's
/// "fresh since the last full pass" test must not lean on a remote filter alone).
/// A non-array 200 body is `Err`, never an empty success (see
/// [`symbol_earnings_from_value`]).
fn symbol_news_from_value(value: &Value, from: &str) -> Result<Vec<SymbolNewsItem>> {
    let Some(rows) = value.as_array() else {
        anyhow::bail!("FMP news/stock returned a non-array body — malformed or drifted response");
    };
    Ok(rows
        .iter()
        .filter_map(|row| {
            let published_date = row
                .get("publishedDate")
                .and_then(Value::as_str)?
                .to_string();
            // Date-prefix compare: "2026-08-01 09:30:00" >= "2026-07-20".
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
            })
        })
        .collect())
}

/// Shape an FMP `/balance-sheet-statement` array body into [`BalanceSheetLines`] from
/// its newest row. `None` only when the body is not the expected non-empty array;
/// individual missing lines stay `None`. Equity prefers `totalStockholdersEquity`
/// (the parent-only line P/B conventionally reads) over `totalEquity` (which folds in
/// minority interest). Pure, so the contract is unit-testable offline.
fn balance_sheet_from_value(value: &Value) -> Option<BalanceSheetLines> {
    let first = value.as_array()?.first()?;
    Some(BalanceSheetLines {
        total_debt: first.get("totalDebt").and_then(Value::as_f64),
        // Numeric-first per key: a present-but-null preferred line must still
        // fall through to the alternate, so the fallback runs after `as_f64`.
        total_equity: first
            .get("totalStockholdersEquity")
            .and_then(Value::as_f64)
            .or_else(|| first.get("totalEquity").and_then(Value::as_f64)),
        cash_and_equivalents: first
            .get("cashAndCashEquivalents")
            .and_then(Value::as_f64),
        short_term_investments: first
            .get("shortTermInvestments")
            .and_then(Value::as_f64),
    })
}

/// Shape quarterly `/cash-flow-statement` rows (newest first) — the pre-profit
/// overlay's burn / runway / capex source. A row without a period date is skipped.
fn quarterly_cash_flow_from_value(
    value: &Value,
) -> Vec<crate::portfolio::engine::QuarterlyCashFlowRow> {
    let Some(rows) = value.as_array() else {
        return vec![];
    };
    rows.iter()
        .filter_map(|row| {
            let period_end = row.get("date").and_then(Value::as_str)?.to_string();
            Some(crate::portfolio::engine::QuarterlyCashFlowRow {
                period_end,
                filing_date: row
                    .get("filingDate")
                    .and_then(Value::as_str)
                    .or_else(|| row.get("fillingDate").and_then(Value::as_str))
                    .map(str::to_string),
                free_cash_flow: row.get("freeCashFlow").and_then(Value::as_f64),
                // Numeric-first per key (the balance-sheet shaper's rule): a
                // present-but-null preferred line must still fall through to the
                // alternate spelling, so the fallback runs after `as_f64`.
                operating_cash_flow: row
                    .get("operatingCashFlow")
                    .and_then(Value::as_f64)
                    .or_else(|| {
                        row.get("netCashProvidedByOperatingActivities")
                            .and_then(Value::as_f64)
                    }),
                capex: row.get("capitalExpenditure").and_then(Value::as_f64),
            })
        })
        .collect()
}

/// Shape quarterly `/income-statement` rows (newest first). Lenient key spellings
/// pinned by fixtures; live-verified 2026-07-16 — the feed serves the stable
/// spellings (`filingDate` / `epsDiluted` / `weightedAverageShsOutDil`) and the
/// full 16 rows on `limit=16`.
fn quarterly_income_from_value(value: &Value) -> Vec<crate::portfolio::engine::QuarterlyIncomeRow> {
    let Some(rows) = value.as_array() else {
        return vec![];
    };
    rows.iter()
        .filter_map(|row| {
            let period_end = row.get("date").and_then(Value::as_str)?.to_string();
            // String-first per key (the numeric-first rule's string form): a
            // present-but-null `filingDate` must still fall through to the legacy
            // `fillingDate` spelling, or a restated row loses its tie-break date.
            let filing_date = row
                .get("filingDate")
                .and_then(Value::as_str)
                .or_else(|| row.get("fillingDate").and_then(Value::as_str))
                .map(|s| s.to_string());
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
            })
        })
        .collect()
}

/// The **next-twelve-months (NTM) consensus read**: blend the two nearest **coming**
/// fiscal-year estimate rows, each weighted by its overlap with the rolling
/// twelve-month forward window — so a mostly-reported current fiscal year (whose
/// consensus ≈ the trailing print) contributes only its remaining months instead of
/// masquerading as the forward year, the live-run flat-target mechanism
/// (`docs/portfolio-analysis.md` §Starting parameters; the 2026-07-31 F1 finding).
/// A single forward row keeps the prior single-row semantics. **No forward-dated
/// row → `None`**: a past fiscal-year estimate is not a forward consensus, and
/// letting it masquerade as one would bypass the driver ladder's
/// `no-admissible-driver` abstention with a stale print. Accepts both the stable
/// (`epsAvg`) and legacy (`estimatedEpsAvg`) spellings; live 2026-07-16 the feed
/// serves the stable ones.
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
    // Numeric-first per key (the balance-sheet shaper's rule): a present-but-null
    // stable key must still fall through to the legacy spelling.
    let field = |row: &Value, stable: &str, legacy: &str| {
        row.get(stable)
            .and_then(Value::as_f64)
            .or_else(|| row.get(legacy).and_then(Value::as_f64))
    };
    let mut forward: Vec<&Value> = rows
        .iter()
        .filter(|r| date_of(r).as_str() >= today)
        .collect();
    forward.sort_by_key(|r| date_of(r));
    // One row per fiscal-period date: a duplicated period must not masquerade as
    // near + far — the blend would re-read the same year at both weights and
    // exclude the true following fiscal year while recording `periods_used = 2`.
    forward.dedup_by_key(|r| date_of(r));
    let near: &Value = forward.first()?;
    let far: Option<&&Value> = forward.get(1);

    // The near row's weight = the share of the rolling twelve months its fiscal year
    // still covers (days to its period end / 365, clamped); the far row carries the
    // rest. An unparseable date falls back to the near row alone — the prior
    // single-row semantics, never a fabricated blend. At the boundary (the near
    // fiscal year ends today) the weight is 0 — the value is entirely the far
    // row's while `period_end` still names the near row; `near_weight` on the
    // record is what keys the provenance.
    let near_weight = match (
        far,
        chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d"),
        chrono::NaiveDate::parse_from_str(&date_of(near), "%Y-%m-%d"),
    ) {
        (Some(_), Ok(t), Ok(n)) => ((n - t).num_days().clamp(0, 365) as f64) / 365.0,
        _ => 1.0,
    };
    let blended = near_weight < 1.0;
    let blend = |stable: &str, legacy: &str| -> Option<f64> {
        let n = field(near, stable, legacy);
        // An inactive blend (single forward row, a far year wholly beyond the
        // window, or unparseable dates) reads the near row alone — a far fiscal
        // year must never leak in at full weight through a missing near leg.
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
    })
}

/// Sum the per-share dividends dated within the trailing twelve months of `today` —
/// **bounded on both sides**: rows dated after `today` (announced-but-unpaid
/// declarations the dividends feed carries) are excluded, since "trailing" means
/// paid and a future declaration would inflate the trailing-return leg. `None` when
/// no row lands in the window (a non-payer, or a stale record) — the total-return
/// leg then adds nothing rather than a fabricated yield.
fn ttm_dividends_from_value(value: &Value, today: chrono::NaiveDate) -> Result<Option<f64>> {
    let Some(rows) = value.as_array() else {
        anyhow::bail!("non-array body — malformed or drifted response");
    };
    let cutoff = today - Duration::days(365);
    let mut sum = 0.0;
    let mut any = false;
    for row in rows {
        // A row the parser cannot read is `Err`, never a silent skip: `Ok(None)`
        // must mean the body affirmatively shows no trailing dividends — a
        // missing or unparseable date can't be windowed, and an in-window row
        // whose amount is non-numeric (a string-typed "0.26") would otherwise
        // masquerade as a confirmed dividend elimination downstream.
        let Some(date) = row.get("date").and_then(Value::as_str) else {
            anyhow::bail!("a dividend row carried no date — malformed or drifted response");
        };
        let Ok(parsed) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
            anyhow::bail!(
                "a dividend row carried a non-ISO date {date:?} — malformed or drifted response"
            );
        };
        // Window on the PARSED date, never the source text: chrono accepts
        // non-zero-padded fields ("2026-5-10"), which compare lexicographically
        // outside the window and would silently drop an in-window payment.
        if parsed < cutoff || parsed > today {
            continue;
        }
        // Numeric-first per key: a present-but-null `adjDividend` beside a numeric
        // `dividend` must read the amount, not take the unreadable-row bail path.
        let amount = row
            .get("adjDividend")
            .and_then(Value::as_f64)
            .or_else(|| row.get("dividend").and_then(Value::as_f64));
        let Some(a) = amount else {
            anyhow::bail!(
                "an in-window dividend row carried no numeric amount — malformed or drifted response"
            );
        };
        sum += a;
        any = true;
    }
    Ok(any.then_some(sum))
}

/// Shape a `/profile` body (array-of-one or bare object) into the guard's lookup.
/// Pure. Only FMP's definitive no-such-symbol shape — an **empty array** — reads
/// `Unresolved`; any other non-object body (a drifted or malformed-but-valid-JSON
/// response) is `Unverified`, so an unreadable shape can never terminally not-rate
/// a holding (`docs/portfolio-analysis.md` §Asset eligibility). A present object
/// with blank or missing fields resolves with those fields `None` (the guard types
/// them unverifiable, and the sector consumer types `sector-unscorable`).
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
        company_name: field("companyName"),
        exchange: field("exchange"),
        sector: field("sector"),
    })
}

/// Shape a `/dividends` body into dated per-share amounts within `[from, to]`,
/// oldest first. Strict like [`ttm_dividends_from_value`]: an unreadable row is
/// `Err`, never a silent skip — a dropped in-window payment would understate the
/// total-return label without a trace. Pure.
fn dividend_history_from_value(
    value: &Value,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
    let Some(rows) = value.as_array() else {
        anyhow::bail!("non-array body — malformed or drifted response");
    };
    let mut out: Vec<crate::portfolio::engine::DatedValue> = Vec::new();
    for row in rows {
        let Some(date) = row.get("date").and_then(Value::as_str) else {
            anyhow::bail!("a dividend row carried no date — malformed or drifted response");
        };
        let Ok(parsed) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
            anyhow::bail!(
                "a dividend row carried a non-ISO date {date:?} — malformed or drifted response"
            );
        };
        // Window on the PARSED date, never the source text (non-zero-padded
        // fields compare lexicographically outside the window).
        if parsed < from || parsed > to {
            continue;
        }
        // Numeric-first per key: a present-but-null `adjDividend` beside a numeric
        // `dividend` must read the amount, not take the unreadable-row bail path.
        let amount = row
            .get("adjDividend")
            .and_then(Value::as_f64)
            .or_else(|| row.get("dividend").and_then(Value::as_f64));
        let Some(a) = amount else {
            anyhow::bail!(
                "an in-window dividend row carried no numeric amount — malformed or drifted response"
            );
        };
        out.push(crate::portfolio::engine::DatedValue {
            date: parsed.format("%Y-%m-%d").to_string(),
            value: a,
        });
    }
    out.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(out)
}

/// Fill a [`crate::portfolio::fund::FundData`] from an `etf/info` body (array-of-one
/// or bare object). The expense ratio arrives in **percent units** (0.09 = 9 bps)
/// and normalizes to a decimal ratio at this seam — live-verified 2026-07-16
/// (SPY 0.09 / ARKK 0.75 / VFIAX 0.04, mutual funds served too). The live body
/// carries `assetsUnderManagement` and no `aum` key — the fallback chain covers it.
fn fund_info_into(value: &Value, fund: &mut crate::portfolio::fund::FundData) {
    let obj = value.as_array().and_then(|a| a.first()).or(Some(value));
    let Some(obj) = obj.filter(|o| o.is_object()) else {
        fund.gaps.push("FMP etf/info was malformed".to_string());
        return;
    };
    // Blank / whitespace-only strings normalize to `None` at this seam: the
    // sweep's comparability gates key on presence, and a blank name or asset
    // class read as "present" would fabricate a stored-true → fresh-false
    // overlay clear or a fallback-shaped classification comparison.
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
    // Numeric-first per key: live serves only `assetsUnderManagement`, so a drifted
    // `aum: null` beside it must not erase the value the fallback exists to read.
    fund.aum = obj
        .get("aum")
        .and_then(Value::as_f64)
        .or_else(|| obj.get("assetsUnderManagement").and_then(Value::as_f64));
    fund.nav = obj.get("nav").and_then(Value::as_f64);
}

/// Shape a weightings array (`[{sector|country, weightPercentage}]`) into
/// `(label, fraction)` pairs. Weights arrive as `"25.53%"` strings or numbers; a set
/// whose values exceed 1.5 reads as percent and normalizes to fractions.
fn weights_from_value(value: &Value, label_key: &str) -> Vec<(String, f64)> {
    let Some(rows) = value.as_array() else {
        return vec![];
    };
    let mut out: Vec<(String, f64)> = rows
        .iter()
        .filter_map(|row| {
            let label = row.get(label_key).and_then(Value::as_str)?.to_string();
            let raw = row.get("weightPercentage")?;
            let weight = match raw {
                Value::Number(n) => n.as_f64()?,
                Value::String(s) => s.trim().trim_end_matches('%').parse::<f64>().ok()?,
                _ => return None,
            };
            Some((label, weight))
        })
        .collect();
    if out.iter().any(|(_, w)| *w > 1.5) {
        for (_, w) in &mut out {
            *w /= 100.0;
        }
    }
    out
}

/// Shape `sector-pe-snapshot` / `historical-sector-pe` rows; a row without a usable
/// P/E is skipped, and a missing exchange echoes the requested one.
fn sector_pe_rows_from_value(
    value: &Value,
    requested_exchange: &str,
) -> Vec<crate::portfolio::fund::SectorPe> {
    let Some(rows) = value.as_array() else {
        return vec![];
    };
    rows.iter()
        .filter_map(|row| {
            let sector = row.get("sector").and_then(Value::as_str)?.to_string();
            let date = row
                .get("date")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let pe = match row.get("pe") {
                Some(Value::Number(n)) => n.as_f64()?,
                Some(Value::String(s)) => s.trim().parse::<f64>().ok()?,
                _ => return None,
            };
            let exchange = row
                .get("exchange")
                .and_then(Value::as_str)
                .unwrap_or(requested_exchange)
                .to_string();
            Some(crate::portfolio::fund::SectorPe {
                sector,
                exchange,
                date,
                pe,
            })
        })
        .collect()
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
          {"date":"2026-03-31","filingDate":"2026-05-01","revenue":95000000000.0,
           "epsDiluted":1.55,"weightedAverageShsOutDil":15000000000.0},
          {"date":"2025-12-31","fillingDate":"2026-01-30","revenue":120000000000.0,
           "epsdiluted":2.10,"weightedAverageShsOutDil":15100000000.0}
        ]"#;
        let server = MockHttp::serve(vec![Canned::Reply { status: 200, headers: vec![], body }]);
        let mut gaps = vec![];
        let rows = source(&server.base_url).fetch_quarterly_income("AAPL", &mut gaps);
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
    fn quarterly_income_null_stable_eps_falls_through_to_the_legacy_spelling() {
        // FMP serves JSON nulls: a present-but-null `epsDiluted` must not block
        // the legacy `epsdiluted` value (the balance-sheet shaper's rule).
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
        let lines = balance_sheet_from_value(&value).unwrap();
        assert_eq!(lines.total_equity, Some(63.0e9));
        assert_eq!(lines.total_debt, None);
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
        let lines = src.fetch_balance_sheet("AAPL", &mut gaps);
        assert_eq!(lines, BalanceSheetLines::default());
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        // Premium gate (402) → the same fail-soft shape with the gated reason.
        let mut gaps = vec![];
        let lines = src.fetch_balance_sheet("AAPL", &mut gaps);
        assert_eq!(lines, BalanceSheetLines::default());
        assert!(gaps[0].contains("unavailable"), "{gaps:?}");
    }

    #[test]
    fn consensus_blends_the_two_nearest_forward_years_by_ntm_overlap() {
        // Today 2026-07-16; the near fiscal year ends 2026-09-30 (76 days out, mostly
        // reported — its consensus ≈ the trailing print), the far one 2027-09-30. The
        // NTM read weights the near row by its remaining share of the rolling twelve
        // months (76/365) and the far row by the rest — so real forward growth
        // reaches the driver instead of the near-realized current year (the
        // 2026-07-31 F1 flat-target mechanism). The stale 2025 row never enters.
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
        // The Stooq-fallback form keeps the dates the undated per-company EOD read
        // discards — the v2 anchor join needs them for the latest-on-or-before join.
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
        let ttm = ttm_dividends_from_value(&value, today).unwrap().unwrap();
        assert!(
            (ttm - 0.51).abs() < 1e-9,
            "{ttm}: the 2024 row is outside the window and the future row is excluded"
        );
        // No rows in the window → None, never a fabricated yield.
        let stale: Value = serde_json::from_str(r#"[{"date":"2020-01-01","dividend":1.0}]"#).unwrap();
        assert!(ttm_dividends_from_value(&stale, today).unwrap().is_none());
    }

    #[test]
    fn ttm_dividends_reject_unreadable_rows_rather_than_reading_non_payer() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 3).unwrap();
        // An in-window row with a string-typed amount is unreadable — `Err`,
        // never `None` (which downstream reads as a confirmed non-payer, i.e. a
        // dividend elimination).
        let v: Value =
            serde_json::from_str(r#"[{"date":"2026-05-10","adjDividend":"0.26"}]"#).unwrap();
        assert!(ttm_dividends_from_value(&v, today).is_err());
        // A dateless row cannot be windowed — likewise.
        let v: Value = serde_json::from_str(r#"[{"adjDividend":0.26}]"#).unwrap();
        assert!(ttm_dividends_from_value(&v, today).is_err());
        // A non-ISO date compares lexicographically as out-of-window — it must
        // error, never slide into a false non-payer.
        let v: Value =
            serde_json::from_str(r#"[{"date":"not-a-date","adjDividend":0.26}]"#).unwrap();
        assert!(ttm_dividends_from_value(&v, today).is_err());
        // A non-zero-padded (but real) date parses and windows on the PARSED
        // value — as text it sorts after today and would silently drop the
        // in-window payment.
        let v: Value =
            serde_json::from_str(r#"[{"date":"2026-5-10","adjDividend":0.26}]"#).unwrap();
        assert_eq!(ttm_dividends_from_value(&v, today).unwrap(), Some(0.26));
        // An affirmatively empty body is the real non-payer…
        let v: Value = serde_json::from_str("[]").unwrap();
        assert_eq!(ttm_dividends_from_value(&v, today).unwrap(), None);
        // …and junk on an out-of-window row is irrelevant, not a failure.
        let v: Value =
            serde_json::from_str(r#"[{"date":"2020-01-10","adjDividend":"junk"}]"#).unwrap();
        assert_eq!(ttm_dividends_from_value(&v, today).unwrap(), None);
    }

    #[test]
    fn ttm_dividends_null_adj_amount_falls_through_to_the_plain_amount() {
        // A present-but-null `adjDividend` beside a numeric `dividend` is a
        // readable row — it must sum, not take the unreadable-row bail path.
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 16).unwrap();
        let v: Value =
            serde_json::from_str(r#"[{"date":"2026-05-10","adjDividend":null,"dividend":0.26}]"#)
                .unwrap();
        assert_eq!(ttm_dividends_from_value(&v, today).unwrap(), Some(0.26));
    }

    #[test]
    fn dividend_history_windows_sorts_and_stays_strict() {
        let from = chrono::NaiveDate::from_ymd_opt(2025, 6, 3).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 6, 3).unwrap();
        let body = r#"[
          {"date":"2026-08-10","adjDividend":0.27},
          {"date":"2026-02-10","dividend":0.25},
          {"date":"2025-11-10","adjDividend":0.24},
          {"date":"2025-01-10","adjDividend":0.23}
        ]"#;
        let v: Value = serde_json::from_str(body).unwrap();
        let rows = dividend_history_from_value(&v, from, to).unwrap();
        // Only the two in-window rows, oldest first.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].date, "2025-11-10");
        assert!((rows[1].value - 0.25).abs() < 1e-12);
        // Strict like the TTM read: an unreadable in-window amount is Err (the
        // caller records the labeled price-only fallback), never a silent zero.
        let bad: Value =
            serde_json::from_str(r#"[{"date":"2025-11-10","adjDividend":"0.24"}]"#).unwrap();
        assert!(dividend_history_from_value(&bad, from, to).is_err());
        // An empty body is a genuine non-payer window.
        let empty: Value = serde_json::from_str("[]").unwrap();
        assert!(dividend_history_from_value(&empty, from, to).unwrap().is_empty());
    }

    #[test]
    fn dividend_history_null_adj_amount_falls_through_to_the_plain_amount() {
        let from = chrono::NaiveDate::from_ymd_opt(2025, 6, 3).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 6, 3).unwrap();
        let v: Value =
            serde_json::from_str(r#"[{"date":"2025-11-10","adjDividend":null,"dividend":0.24}]"#)
                .unwrap();
        let rows = dividend_history_from_value(&v, from, to).unwrap();
        assert_eq!(rows.len(), 1);
        assert!((rows[0].value - 0.24).abs() < 1e-12);
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
            r#"[{"symbol":"AAPL","companyName":"Apple Inc.","exchange":"NASDAQ","sector":"Technology"}]"#,
        )
        .unwrap();
        let ProfileLookup::Resolved(identity) = profile_identity_from_value(&v) else {
            panic!("expected resolved");
        };
        assert_eq!(identity.company_name.as_deref(), Some("Apple Inc."));
        assert_eq!(identity.exchange.as_deref(), Some("NASDAQ"));
        assert_eq!(identity.sector.as_deref(), Some("Technology"));
        let bare: Value = serde_json::from_str(r#"{"sector":"Energy"}"#).unwrap();
        let ProfileLookup::Resolved(identity) = profile_identity_from_value(&bare) else {
            panic!("expected resolved");
        };
        assert_eq!(identity.sector.as_deref(), Some("Energy"));
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
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: info },
            Canned::Reply { status: 200, headers: vec![], body: sectors },
            Canned::Reply { status: 200, headers: vec![], body: countries },
        ]);
        let fund = source(&server.base_url).fetch_fund_data("VTI");
        assert_eq!(fund.asset_class.as_deref(), Some("Equity"));
        // Percent-unit expense ratio normalizes to a decimal ratio at the seam.
        assert!((fund.expense_ratio.unwrap() - 0.0003).abs() < 1e-12);
        assert_eq!(fund.nav, Some(280.5));
        // Percent-string weights normalize to fractions.
        assert!((fund.sector_weights[0].1 - 0.325).abs() < 1e-9);
        assert!((fund.country_weights[0].1 - 0.994).abs() < 1e-9);
        assert!(fund.gaps.is_empty(), "{:?}", fund.gaps);
    }

    #[test]
    fn fund_info_blank_strings_normalize_to_none() {
        // "" / whitespace-only name or assetClass reads as absent, never
        // present: the sweep's comparability gates key on `is_some()`, and a
        // blank name would fabricate a stored-true → fresh-false overlay clear
        // while a blank asset class would dodge the degraded-family path.
        let info = r#"[{"symbol":"VTI","name":"   ","assetClass":"","expenseRatio":0.03}]"#;
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: info },
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
            Canned::Reply { status: 200, headers: vec![], body: "[]" },
            Canned::Reply { status: 500, headers: vec![], body: "oops" },
        ]);
        let fund = source(&server.base_url).fetch_fund_data("VTI");
        assert!(fund.asset_class.is_none());
        assert_eq!(fund.gaps.len(), 3, "{:?}", fund.gaps);
    }

    #[test]
    fn sector_pe_rows_parse_and_echo_the_exchange() {
        let body = r#"[
          {"date":"2026-07-15","sector":"Technology","exchange":"NYSE","pe":30.1},
          {"date":"2026-07-15","sector":"Energy","pe":"11.4"},
          {"date":"2026-07-15","sector":"Broken"}
        ]"#;
        let server = MockHttp::serve(vec![Canned::Reply { status: 200, headers: vec![], body }]);
        let rows = source(&server.base_url)
            .fetch_sector_pe_snapshot("NYSE", "2026-07-15")
            .unwrap();
        assert_eq!(rows.len(), 2, "the row without a usable P/E is skipped");
        assert_eq!(rows[1].exchange, "NYSE", "missing exchange echoes the request");
        assert!((rows[1].pe - 11.4).abs() < 1e-9, "string P/E parses");
    }
}
