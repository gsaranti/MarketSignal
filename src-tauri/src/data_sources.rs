//! The market-data source contract: a structured baseline-scan boundary.
//!
//! Mirrors the `agent` module's spine — the application layer owns all I/O, the
//! data source is a trait the orchestrator drives, and a deterministic stub
//! stands in for the live provider in offline tests. The real Financial Modeling
//! Prep adapter lives in `fmp` and implements this trait; the future FRED/BLS
//! adapters will join it behind the same boundary.
//!
//! `BaselineMarketData` is the Step-6 baseline scan
//! (`docs/weekly-report-workflow.md §Step 6`), gathered before agent reasoning.
//! This slice populates the FMP-owned groups — indices, internals, and sector
//! performance; the macro group (Treasury yields, Fed expectations, the
//! inflation / jobs calendar) is FRED/BLS's responsibility and arrives with that
//! adapter slice.

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

#[cfg(test)]
mod tests {
    use super::*;

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
