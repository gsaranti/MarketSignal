//! The market-data source contract: a structured baseline-scan boundary.
//!
//! Mirrors the `agent` module's spine — the application layer owns all I/O, the
//! data source is a trait the orchestrator drives, and a deterministic stub
//! stands in for the live provider in offline tests. Three real adapters implement
//! this trait — `fmp` (equity indices, VIX, gold, sectors, and multi-horizon index
//! performance from end-of-day history), `fred` (the Treasury
//! yields, dollar index, and oil / natural-gas internals FMP's free tier omits, plus
//! the `macro_levels` group — Fed-funds target range, inflation breakevens, consumer
//! sentiment, PCE — and the economic-release `calendar`), and `bls` (the `labor_levels`
//! group — CPI, unemployment, payrolls, wages) — and the `CompositeMarketDataSource`
//! below runs them and merges them into one baseline.
//!
//! `BaselineMarketData` is the Step-6 baseline scan
//! (`docs/weekly-report-workflow.md §Step 6`), gathered before agent reasoning.
//! FMP fills indices, sectors, and the VIX + gold internals; FRED appends its
//! commodity / yield series to the same `internals` group and fills the
//! `macro_levels` group (Fed-funds target range, inflation breakevens, consumer
//! sentiment, PCE) and the economic-release `calendar` (the prior-week + upcoming US
//! reports, from FRED's free release-dates schedule — FMP's economic-calendar endpoint
//! is premium-gated); BLS fills the `labor_levels` group (CPI, unemployment, payrolls,
//! wages).

use serde::{Deserialize, Serialize};

/// One quoted instrument in the baseline scan: a market index or a market
/// internal (VIX, the dollar index, a commodity). `change_pct` is the percent
/// change the provider reports for the quote.
///
/// `unit` annotates **`price`** — the unit the level is quoted in ("index points",
/// "percent", "USD per barrel", "thousands of persons", …), supplied per series from
/// each adapter's own table rather than the wire, so the model reading the serialized
/// baseline can't misread an unlabeled level (a payroll count of thousands as ones, a
/// yield level as a dollar figure). It does **not** describe `change_pct`, which is a
/// percent for every series regardless of the level's unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub change_pct: f64,
    pub unit: String,
}

/// One sector's period performance, as a percentage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorPerformance {
    pub sector: String,
    pub change_pct: f64,
}

/// One entry in the Step-6 economic-release calendar: a scheduled or just-released US
/// economic report (`docs/weekly-report-workflow.md §Step 6` — the "CPI/PCE/jobs
/// calendar" and "major economic reports from the prior week"). Sourced from FRED's
/// free release-dates schedule (FMP's economic-calendar endpoint is premium-gated), so
/// it carries the release **name** and **date** but not the report's figures — those
/// reach the model through the `macro_levels` / `labor_levels` series quotes. `status`
/// is `"released"` for a date in the prior-week window or `"upcoming"` for a scheduled
/// future date. `expected` is the analyst-consensus slot reserved for a future paid
/// source — no free provider supplies US consensus, so it is always `None` on the FRED
/// path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomicRelease {
    pub release: String,
    pub date: String,
    pub status: String,
    pub expected: Option<f64>,
}

/// One index's multi-horizon performance, derived from FMP's end-of-day history
/// (`historical-price-eod`). Complements the daily `Quote` in `indices` with the
/// longer-horizon returns a *weekly* thesis needs: week-over-week, month-to-date, and
/// year-to-date returns, plus where the latest close sits inside its trailing 52-week
/// range. Every `*_pct` is a percent; `pct_from_52w_high` is ≤ 0 (distance below the
/// high). `low_52w` / `high_52w` are price levels in the index's own units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexPerformance {
    pub symbol: String,
    pub name: String,
    pub weekly_pct: f64,
    pub mtd_pct: f64,
    pub ytd_pct: f64,
    pub low_52w: f64,
    pub high_52w: f64,
    pub pct_from_52w_high: f64,
}

/// The baseline market-data scan handed to the main agent as part of its input.
/// Empty vectors are valid — a provider that returns no data for a group leaves
/// it empty rather than failing the whole scan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BaselineMarketData {
    pub indices: Vec<Quote>,
    pub internals: Vec<Quote>,
    pub sectors: Vec<SectorPerformance>,
    /// Step-6 macro levels (Fed-funds target range, inflation breakevens, consumer
    /// sentiment, PCE, plus the headline activity reports — PPI, retail sales, JOLTS,
    /// real GDP — that back the `calendar`'s prior-week entries) — point-in-time FRED
    /// series, kept distinct from the market `internals`. Same `Quote` shape: `price` is
    /// the latest level and `change_pct` its change from the prior observation
    /// (day-over-day, month-over-month, or quarter-over-quarter by series frequency).
    pub macro_levels: Vec<Quote>,
    /// Step-6 labor levels (CPI, unemployment rate, nonfarm payrolls, average hourly
    /// earnings) — point-in-time BLS series, kept distinct from the FRED `macro_levels`
    /// by source and concern. Same `Quote` shape: `price` is the latest reported level
    /// and `change_pct` its month-over-month change from the prior reading.
    pub labor_levels: Vec<Quote>,
    /// Step-6 economic-release calendar (`docs/weekly-report-workflow.md §Step 6`): the
    /// prior-week and upcoming US economic reports (CPI, PCE, jobs, GDP, …) as a
    /// release schedule from FRED's free release-dates endpoint. A schedule of names +
    /// dates, not figures — the actual readings reach the model via `macro_levels` /
    /// `labor_levels`. Empty is valid (a quiet window, or the calendar soft-degraded); it
    /// carries no completeness floor, unlike the series groups.
    pub calendar: Vec<EconomicRelease>,
    /// Step-6 multi-horizon index performance, derived from FMP's end-of-day history
    /// (`historical-price-eod`): week-over-week / MTD / YTD returns and 52-week-range
    /// position per index, enriching the daily `indices` quotes. Empty is valid — like
    /// the `calendar` it carries no completeness floor and soft-degrades if the history
    /// fetch fails, since the daily `indices` quotes already satisfy Step 6.
    pub index_performance: Vec<IndexPerformance>,
}

/// The data-source stage. One method: gather the required baseline scan. Sync,
/// like the `MainAgent` trait — the blocking HTTP call inside the real adapter is
/// offloaded via `spawn_blocking` at the Tauri command seam.
pub trait MarketDataSource {
    fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData>;
}

/// Deterministic offline stand-in for the real data adapter. Returns a small,
/// fixed baseline so the pipeline and its tests run without live keys.
#[derive(Debug, Default)]
pub struct StubMarketDataSource;

impl MarketDataSource for StubMarketDataSource {
    fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
        Ok(BaselineMarketData {
            indices: vec![
                Quote {
                    symbol: "^GSPC".into(),
                    name: "S&P 500".into(),
                    price: 5_500.0,
                    change_pct: 0.4,
                    unit: "index points".into(),
                },
                Quote {
                    symbol: "^IXIC".into(),
                    name: "Nasdaq Composite".into(),
                    price: 17_800.0,
                    change_pct: 0.6,
                    unit: "index points".into(),
                },
            ],
            internals: vec![Quote {
                symbol: "^VIX".into(),
                name: "CBOE Volatility Index".into(),
                price: 14.2,
                change_pct: -1.1,
                unit: "index points".into(),
            }],
            sectors: vec![
                SectorPerformance {
                    sector: "Technology".into(),
                    change_pct: 1.2,
                },
                SectorPerformance {
                    sector: "Energy".into(),
                    change_pct: -0.8,
                },
            ],
            macro_levels: vec![Quote {
                symbol: "DFEDTARU".into(),
                name: "Fed Funds Target Range — Upper Limit".into(),
                price: 4.5,
                change_pct: 0.0,
                unit: "percent".into(),
            }],
            labor_levels: vec![Quote {
                symbol: "LNS14000000".into(),
                name: "Unemployment Rate".into(),
                price: 4.1,
                change_pct: 0.0,
                unit: "percent".into(),
            }],
            calendar: vec![EconomicRelease {
                release: "Employment Situation".into(),
                date: "2026-06-05".into(),
                status: "released".into(),
                expected: None,
            }],
            index_performance: vec![IndexPerformance {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                weekly_pct: 1.1,
                mtd_pct: 2.3,
                ytd_pct: 8.4,
                low_52w: 4_200.0,
                high_52w: 5_600.0,
                pct_from_52w_high: -1.8,
            }],
        })
    }
}

/// Compose two `MarketDataSource`s into one baseline scan: run the `primary`
/// (e.g. FMP — indices, sectors, VIX, gold), then the `secondary`, and merge them
/// across every group (`indices`, `internals`, `sectors`, `macro_levels`,
/// `labor_levels`, `calendar`). Both contributions are required: either child's failure
/// propagates, so a secondary failure fails the run exactly as a primary failure does
/// (the secondaries now source non-optional Step-6 series — `docs/configuration.md`).
/// Order is primary-then-secondary, so the merged `internals` reads the primary's
/// quotes first. Sources compose by nesting: the run path wraps FMP+FRED, then nests
/// `bls` as the outer secondary to fold in the `labor_levels` group.
pub struct CompositeMarketDataSource<P, S> {
    pub primary: P,
    pub secondary: S,
}

impl<P, S> CompositeMarketDataSource<P, S> {
    pub fn new(primary: P, secondary: S) -> Self {
        Self { primary, secondary }
    }
}

impl<P: MarketDataSource, S: MarketDataSource> MarketDataSource
    for CompositeMarketDataSource<P, S>
{
    fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
        let mut merged = self.primary.baseline_scan()?;
        let extra = self.secondary.baseline_scan()?;
        merged.indices.extend(extra.indices);
        merged.internals.extend(extra.internals);
        merged.sectors.extend(extra.sectors);
        merged.macro_levels.extend(extra.macro_levels);
        merged.labor_levels.extend(extra.labor_levels);
        merged.calendar.extend(extra.calendar);
        merged.index_performance.extend(extra.index_performance);
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub shaped like the secondary sources (`fred` + `bls`) — it contributes the
    /// `internals` / `macro_levels` (FRED) and `labor_levels` (BLS) groups the
    /// secondaries own, so the composite merge can be exercised offline.
    struct FredShapedStub {
        internals: Vec<Quote>,
        macro_levels: Vec<Quote>,
        labor_levels: Vec<Quote>,
        calendar: Vec<EconomicRelease>,
    }

    impl MarketDataSource for FredShapedStub {
        fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
            Ok(BaselineMarketData {
                internals: self.internals.clone(),
                macro_levels: self.macro_levels.clone(),
                labor_levels: self.labor_levels.clone(),
                calendar: self.calendar.clone(),
                ..Default::default()
            })
        }
    }

    /// A stub whose scan always fails, to check that a secondary failure propagates.
    struct FailingStub;

    impl MarketDataSource for FailingStub {
        fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
            anyhow::bail!("provider down")
        }
    }

    fn quote(symbol: &str) -> Quote {
        Quote {
            symbol: symbol.into(),
            name: symbol.into(),
            price: 1.0,
            change_pct: 0.0,
            unit: "index points".into(),
        }
    }

    #[test]
    fn composite_merges_both_sources_into_one_baseline() {
        // Primary (FMP-shaped) carries indices + a VIX internal + sectors + one macro
        // level (DFEDTARU) + one labor level (LNS14000000); the secondary adds two more
        // internals, a macro level, and a labor level. The merge keeps every group and
        // orders each primary-first.
        let secondary = FredShapedStub {
            internals: vec![quote("DGS10"), quote("DTWEXBGS")],
            macro_levels: vec![quote("T10YIE")],
            labor_levels: vec![quote("CUUR0000SA0")],
            calendar: vec![EconomicRelease {
                release: "Consumer Price Index".into(),
                date: "2026-06-10".into(),
                status: "upcoming".into(),
                expected: None,
            }],
        };
        let composite = CompositeMarketDataSource::new(StubMarketDataSource, secondary);
        let data = composite.baseline_scan().unwrap();

        assert!(!data.indices.is_empty(), "primary indices survive");
        assert!(!data.sectors.is_empty(), "primary sectors survive");
        // VIX (from the stub) first, then the two FRED series.
        assert_eq!(data.internals.len(), 3);
        assert_eq!(data.internals[0].symbol, "^VIX");
        assert_eq!(data.internals[1].symbol, "DGS10");
        assert_eq!(data.internals[2].symbol, "DTWEXBGS");
        // macro_levels merges primary-first too: the stub's DFEDTARU, then FRED's.
        assert_eq!(data.macro_levels.len(), 2);
        assert_eq!(data.macro_levels[0].symbol, "DFEDTARU");
        assert_eq!(data.macro_levels[1].symbol, "T10YIE");
        // labor_levels merges primary-first too: the stub's LNS14000000, then the
        // secondary's BLS series.
        assert_eq!(data.labor_levels.len(), 2);
        assert_eq!(data.labor_levels[0].symbol, "LNS14000000");
        assert_eq!(data.labor_levels[1].symbol, "CUUR0000SA0");
        // calendar merges primary-first too: the stub's Employment Situation, then the
        // secondary's CPI release.
        assert_eq!(data.calendar.len(), 2);
        assert_eq!(data.calendar[0].release, "Employment Situation");
        assert_eq!(data.calendar[1].release, "Consumer Price Index");
    }

    #[test]
    fn composite_propagates_a_secondary_failure() {
        // A FRED-side failure must fail the whole scan, since FRED sources
        // non-optional baseline series.
        let composite = CompositeMarketDataSource::new(StubMarketDataSource, FailingStub);
        assert!(composite.baseline_scan().is_err());
    }

    #[test]
    fn stub_populates_groups_and_round_trips() {
        let data = StubMarketDataSource.baseline_scan().unwrap();
        assert!(!data.indices.is_empty());
        assert!(!data.internals.is_empty());
        assert!(!data.sectors.is_empty());
        assert!(!data.macro_levels.is_empty());
        assert!(!data.labor_levels.is_empty());
        assert!(!data.calendar.is_empty());
        assert!(!data.index_performance.is_empty());

        // The whole packet serializes and parses back unchanged — the contract
        // the agent input and the model prompt both lean on.
        let json = serde_json::to_string(&data).unwrap();
        let back: BaselineMarketData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }
}
