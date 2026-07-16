//! The holdings source for the local Portfolio Analysis job
//! (`docs/schwab-integration.md`). Schwab supplies both the holdings *and* the live
//! option chains from which the deterministic options-activity signal is computed,
//! so a connected account is a hard precondition for the job.
//!
//! This slice runs against a **fixture** source ([`FixtureHoldingsSource`]) behind
//! the [`HoldingsSource`] trait — a single equity position plus a stub option chain,
//! entirely offline — so the per-holding pipeline can be validated for quality and
//! runtime before the live OAuth integration lands. The live Schwab Trader API
//! adapter (the OAuth loopback, the 30-min/7-day token lifecycle, Keychain token
//! storage) implements the same trait in a later slice, so nothing downstream of
//! this seam changes when it does.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::portfolio::AssetClass;

/// One position in the user's account: identity, asset class, size, and value.
/// Cost basis and market value are account-currency totals (not per-share).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub description: String,
    pub asset_class: AssetClass,
    pub quantity: f64,
    /// Total cost basis of the position (all shares).
    pub cost_basis: f64,
    /// Current market value of the position.
    pub market_value: f64,
    /// Latest per-share price, when the source carried one.
    pub current_price: Option<f64>,
}

/// A snapshot of the holdings pulled from the connected account: the positions, the
/// cash / buying power, and the account's total value. The most recent pull is
/// stored so the portfolio is viewable without re-fetching (`docs/storage.md
/// §Local Analysis Suite Storage`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Holdings {
    /// The normalized **book-level** rows — one netted position per symbol once
    /// [`Holdings::normalized`] has run at snapshot assembly. Every downstream
    /// contract (the diff, the per-holding loop, the roll-up) consumes only these.
    pub positions: Vec<Position>,
    pub cash: f64,
    /// Sum of position market values plus cash — the denominator for portfolio weights.
    pub account_total: f64,
    /// The pre-normalization per-source rows, retained for display and audit
    /// (`docs/schwab-integration.md` §What is pulled). Empty on a snapshot assembled
    /// before normalization existed (`#[serde(default)]`) and on a not-yet-normalized
    /// pull.
    #[serde(default)]
    pub source_rows: Vec<Position>,
}

impl Holdings {
    /// The holdings-normalization step (`docs/schwab-integration.md` §What is pulled):
    /// same-symbol rows across granted accounts (and manual supplements) consolidate
    /// into **one book-level position per symbol** before anything downstream reads
    /// them. The uppercased symbol is the suite's sole position identity; signed
    /// quantities, market values, and signed cost-basis totals each **sum** across
    /// rows — never a share-weighted average price, so dollar gain stays additive
    /// (Σ market value − Σ cost basis) and the totals stay well-defined even at zero
    /// net quantity — and the position's long/short side comes from the **netted**
    /// quantity. Identity fields (description, asset class) come from the first row
    /// seen for the symbol; the pre-normalization rows are retained on
    /// [`Holdings::source_rows`].
    ///
    /// Idempotent: a snapshot that already carries source rows is already normalized
    /// and is returned unchanged, so a re-normalization can never lose the raw rows.
    pub fn normalized(mut self) -> Holdings {
        if !self.source_rows.is_empty() {
            return self;
        }
        let raw = std::mem::take(&mut self.positions);
        let mut order: Vec<String> = Vec::new();
        let mut merged: std::collections::HashMap<String, Position> = std::collections::HashMap::new();
        for row in &raw {
            let key = row.symbol.to_ascii_uppercase();
            match merged.get_mut(&key) {
                None => {
                    let mut netted = row.clone();
                    netted.symbol = key.clone();
                    order.push(key.clone());
                    merged.insert(key, netted);
                }
                Some(existing) => {
                    existing.quantity += row.quantity;
                    existing.cost_basis += row.cost_basis;
                    existing.market_value += row.market_value;
                    if existing.current_price.is_none() {
                        existing.current_price = row.current_price;
                    }
                }
            }
        }
        self.positions = order
            .into_iter()
            .map(|k| merged.remove(&k).expect("every ordered key was inserted"))
            .collect();
        self.source_rows = raw;
        self
    }
}

/// Whether an option contract is a call or a put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OptionKind {
    Call,
    Put,
}

/// One contract row from an option chain — the fields the activity signal reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionQuote {
    pub kind: OptionKind,
    pub strike: f64,
    /// Expiry as an ISO date (`YYYY-MM-DD`); kept as a string since this slice does
    /// no date arithmetic over it.
    pub expiry: String,
    pub volume: f64,
    pub open_interest: f64,
    pub implied_volatility: Option<f64>,
}

/// A symbol's option chain — the raw rows the deterministic options-activity signal
/// (put/call + IV/skew) is computed from (`docs/schwab-integration.md`). The signal
/// itself is computed in [`crate::portfolio::engine`], not here: this is the data,
/// not the interpretation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionChain {
    pub underlying: String,
    /// The underlying's spot price when the chain was pulled, for an at-the-money read.
    pub underlying_price: Option<f64>,
    pub contracts: Vec<OptionQuote>,
}

/// The seam Portfolio Analysis pulls holdings and chains through. A connected Schwab
/// account is required to run the job; manual import only supplements holdings and
/// does not satisfy the gate (`docs/schwab-integration.md` §A connected Schwab
/// account is required). The live OAuth adapter and this fixture both implement it,
/// so the pipeline never depends on which is behind the trait.
pub trait HoldingsSource {
    /// Pull the current holdings snapshot.
    fn holdings(&self) -> Result<Holdings>;
    /// Pull the option chain for one underlying, or `None` when the source has none
    /// (a symbol with no listed options, or a not-yet-implemented live path).
    fn option_chain(&self, symbol: &str) -> Result<Option<OptionChain>>;
}

/// An offline fixture holdings source for the single-equity slice: one stock
/// position plus a small but realistic option chain, so the whole pipeline runs with
/// no Schwab connection, no OAuth, and no network. Deterministic — the same fixture
/// every call — so a run's quality and runtime can be validated repeatably.
pub struct FixtureHoldingsSource {
    holdings: Holdings,
    chain: OptionChain,
}

impl Default for FixtureHoldingsSource {
    fn default() -> Self {
        let position = Position {
            symbol: "AAPL".to_string(),
            description: "Apple Inc.".to_string(),
            asset_class: AssetClass::Stock,
            quantity: 100.0,
            cost_basis: 14_000.0,
            market_value: 19_500.0,
            current_price: Some(195.0),
        };
        let holdings = Holdings {
            positions: vec![position],
            cash: 10_000.0,
            account_total: 29_500.0,
            source_rows: vec![],
        };
        // A compact near-dated chain: slightly more put volume/OI than call, and puts
        // carrying richer IV than calls — i.e. a mild hedging-demand tilt the activity
        // signal should surface (without it feeding the grade).
        let chain = OptionChain {
            underlying: "AAPL".to_string(),
            underlying_price: Some(195.0),
            contracts: vec![
                OptionQuote {
                    kind: OptionKind::Call,
                    strike: 195.0,
                    expiry: "2026-07-17".to_string(),
                    volume: 4_000.0,
                    open_interest: 12_000.0,
                    implied_volatility: Some(0.27),
                },
                OptionQuote {
                    kind: OptionKind::Call,
                    strike: 205.0,
                    expiry: "2026-07-17".to_string(),
                    volume: 2_500.0,
                    open_interest: 8_000.0,
                    implied_volatility: Some(0.29),
                },
                OptionQuote {
                    kind: OptionKind::Put,
                    strike: 195.0,
                    expiry: "2026-07-17".to_string(),
                    volume: 5_200.0,
                    open_interest: 15_000.0,
                    implied_volatility: Some(0.31),
                },
                OptionQuote {
                    kind: OptionKind::Put,
                    strike: 185.0,
                    expiry: "2026-07-17".to_string(),
                    volume: 3_100.0,
                    open_interest: 9_500.0,
                    implied_volatility: Some(0.34),
                },
            ],
        };
        Self { holdings, chain }
    }
}

impl FixtureHoldingsSource {
    /// The single-equity fixture (the [`Default`] holdings + chain).
    pub fn new() -> Self {
        Self::default()
    }

    /// A fixture over an explicit holdings snapshot — for tests that need the holdings
    /// to vary across runs (e.g. exercising the Step-4 holdings-change diff, which the
    /// deterministic single-equity [`Default`] cannot). Reuses the default option
    /// chain, served only for its AAPL underlying; any other symbol reports no chain,
    /// exactly as a live source would for an un-optioned name.
    pub fn with_holdings(holdings: Holdings) -> Self {
        Self {
            holdings,
            chain: Self::default().chain,
        }
    }
}

impl HoldingsSource for FixtureHoldingsSource {
    fn holdings(&self) -> Result<Holdings> {
        Ok(self.holdings.clone())
    }

    fn option_chain(&self, symbol: &str) -> Result<Option<OptionChain>> {
        // The fixture serves its one chain for the underlying it holds; any other
        // symbol has none, exactly as a live source would answer for an un-optioned name.
        if symbol.eq_ignore_ascii_case(&self.chain.underlying) {
            Ok(Some(self.chain.clone()))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_holds_one_gradeable_equity_with_a_consistent_total() {
        let src = FixtureHoldingsSource::new();
        let h = src.holdings().unwrap();
        assert_eq!(h.positions.len(), 1);
        let p = &h.positions[0];
        assert_eq!(p.symbol, "AAPL");
        assert!(p.asset_class.is_gradeable());
        // account_total is the positions + cash, so portfolio weights are well-defined.
        let positions_value: f64 = h.positions.iter().map(|p| p.market_value).sum();
        assert!((h.account_total - (positions_value + h.cash)).abs() < 1e-6);
    }

    fn row(symbol: &str, quantity: f64, cost_basis: f64, market_value: f64) -> Position {
        Position {
            symbol: symbol.into(),
            description: format!("{symbol} Inc."),
            asset_class: AssetClass::Stock,
            quantity,
            cost_basis,
            market_value,
            current_price: Some(if quantity != 0.0 { market_value / quantity } else { 0.0 }),
        }
    }

    fn snapshot(positions: Vec<Position>) -> Holdings {
        let account_total = positions.iter().map(|p| p.market_value).sum::<f64>();
        Holdings {
            positions,
            cash: 0.0,
            account_total,
            source_rows: vec![],
        }
    }

    #[test]
    fn normalization_nets_same_symbol_rows_across_accounts() {
        // Two accounts each hold AAPL: quantities, cost-basis totals, and market
        // values each sum — never a share-weighted average price.
        let h = snapshot(vec![
            row("AAPL", 100.0, 14_000.0, 19_500.0),
            row("MSFT", 10.0, 3_000.0, 4_200.0),
            row("aapl", 50.0, 8_000.0, 9_750.0),
        ])
        .normalized();
        assert_eq!(h.positions.len(), 2, "same-symbol rows merge case-insensitively");
        let aapl = &h.positions[0];
        assert_eq!(aapl.symbol, "AAPL");
        assert_eq!(aapl.quantity, 150.0);
        assert_eq!(aapl.cost_basis, 22_000.0);
        assert_eq!(aapl.market_value, 29_250.0);
        // First-seen order is preserved and the raw per-source rows are retained.
        assert_eq!(h.positions[1].symbol, "MSFT");
        assert_eq!(h.source_rows.len(), 3);
    }

    #[test]
    fn normalization_reads_side_from_the_netted_quantity() {
        // A long in one account and a larger short in another read as their true net
        // side — one net-short book-level position, never two positions.
        let h = snapshot(vec![
            row("XYZ", 100.0, 10_000.0, 12_000.0),
            row("XYZ", -140.0, -14_000.0, -16_800.0),
        ])
        .normalized();
        assert_eq!(h.positions.len(), 1);
        let net = &h.positions[0];
        assert_eq!(net.quantity, -40.0);
        assert_eq!(net.cost_basis, -4_000.0);
        assert_eq!(net.market_value, -4_800.0);
    }

    #[test]
    fn normalization_keeps_dollar_gain_additive_at_zero_net_quantity() {
        // A fully offset book (zero net shares) keeps summed signed totals, so
        // Σ market value − Σ cost basis still equals the aggregate unrealized P/L —
        // a per-unit average price would be undefined here.
        let h = snapshot(vec![
            row("HEDG", 100.0, 9_000.0, 11_000.0),
            row("HEDG", -100.0, -10_000.0, -11_000.0),
        ])
        .normalized();
        let net = &h.positions[0];
        assert_eq!(net.quantity, 0.0);
        assert_eq!(net.market_value - net.cost_basis, 1_000.0);
    }

    #[test]
    fn normalization_is_idempotent_and_leaves_unique_symbols_intact() {
        let once = snapshot(vec![
            row("AAPL", 100.0, 14_000.0, 19_500.0),
            row("MSFT", 10.0, 3_000.0, 4_200.0),
        ])
        .normalized();
        let twice = once.clone().normalized();
        assert_eq!(once, twice, "re-normalizing an already-normalized snapshot is a no-op");
        assert_eq!(once.positions.len(), 2);
        assert_eq!(once.account_total, 23_700.0);
    }

    #[test]
    fn fixture_serves_its_chain_only_for_the_underlying_it_holds() {
        let src = FixtureHoldingsSource::new();
        let chain = src.option_chain("aapl").unwrap().expect("case-insensitive match");
        assert_eq!(chain.underlying, "AAPL");
        assert!(chain.contracts.iter().any(|c| c.kind == OptionKind::Put));
        assert!(src.option_chain("MSFT").unwrap().is_none());
    }
}
