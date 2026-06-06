//! The market-data source contract: a structured baseline-scan boundary.
//!
//! Mirrors the `agent` module's spine — the application layer owns all I/O, the
//! data source is a trait the orchestrator drives, and a deterministic stub
//! stands in for the live provider in offline tests. Two real adapters implement
//! this trait — `fmp` (equity indices, VIX, gold, sectors) and `fred` (the Treasury
//! yields, dollar index, and the oil / natural-gas internals FMP's free tier omits)
//! — and the `CompositeMarketDataSource` below runs both and merges them into one
//! baseline.
//!
//! `BaselineMarketData` is the Step-6 baseline scan
//! (`docs/weekly-report-workflow.md §Step 6`), gathered before agent reasoning.
//! FMP fills indices, sectors, and the VIX + gold internals; FRED appends its
//! series to the same `internals` group. The remaining Step-6 macro group (Fed
//! expectations,
//! the CPI / PCE / jobs calendar, inflation expectations, consumer confidence) and
//! BLS labor data are a later slice.

use serde::{Deserialize, Serialize};

/// One quoted instrument in the baseline scan: a market index or a market
/// internal (VIX, the dollar index, a commodity). `change_pct` is the percent
/// change the provider reports for the quote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub change_pct: f64,
}

/// One sector's period performance, as a percentage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorPerformance {
    pub sector: String,
    pub change_pct: f64,
}

/// The baseline market-data scan handed to the main agent as part of its input.
/// Empty vectors are valid — a provider that returns no data for a group leaves
/// it empty rather than failing the whole scan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BaselineMarketData {
    pub indices: Vec<Quote>,
    pub internals: Vec<Quote>,
    pub sectors: Vec<SectorPerformance>,
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
                },
                Quote {
                    symbol: "^IXIC".into(),
                    name: "Nasdaq Composite".into(),
                    price: 17_800.0,
                    change_pct: 0.6,
                },
            ],
            internals: vec![Quote {
                symbol: "^VIX".into(),
                name: "CBOE Volatility Index".into(),
                price: 14.2,
                change_pct: -1.1,
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
        })
    }
}

/// Compose two `MarketDataSource`s into one baseline scan: run the `primary`
/// (FMP — indices, sectors, VIX, gold), then the `secondary` (FRED — its internals), and
/// merge them group-by-group. Both contributions are required: either child's
/// failure propagates, so a FRED failure fails the run exactly as an FMP failure
/// does (FRED now sources non-optional Step-6 series — `docs/configuration.md`).
/// Order is primary-then-secondary, so the merged `internals` reads FMP's (VIX,
/// gold) first, then the FRED series.
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
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub that contributes only `internals` — the shape `fred::FredDataSource`
    /// returns, so the composite merge can be exercised offline.
    struct InternalsOnlyStub(Vec<Quote>);

    impl MarketDataSource for InternalsOnlyStub {
        fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
            Ok(BaselineMarketData {
                internals: self.0.clone(),
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
        }
    }

    #[test]
    fn composite_merges_both_sources_into_one_baseline() {
        // Primary (FMP-shaped) carries indices + a VIX internal + sectors; the
        // secondary (FRED-shaped) adds two more internals. The merge keeps every
        // group and orders the internals primary-first.
        let fred = InternalsOnlyStub(vec![quote("DGS10"), quote("DTWEXBGS")]);
        let composite = CompositeMarketDataSource::new(StubMarketDataSource, fred);
        let data = composite.baseline_scan().unwrap();

        assert!(!data.indices.is_empty(), "primary indices survive");
        assert!(!data.sectors.is_empty(), "primary sectors survive");
        // VIX (from the stub) first, then the two FRED series.
        assert_eq!(data.internals.len(), 3);
        assert_eq!(data.internals[0].symbol, "^VIX");
        assert_eq!(data.internals[1].symbol, "DGS10");
        assert_eq!(data.internals[2].symbol, "DTWEXBGS");
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

        // The whole packet serializes and parses back unchanged — the contract
        // the agent input and the model prompt both lean on.
        let json = serde_json::to_string(&data).unwrap();
        let back: BaselineMarketData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }
}
