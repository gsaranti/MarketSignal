//! The holdings source for the local Portfolio Analysis job
//! (`docs/schwab-integration.md`): Schwab supplies both the holdings and the option
//! chains the options-activity signal is computed from, so a connected account is a
//! hard precondition. Production wires the live adapter (`crate::schwab_live`); the
//! [`FixtureHoldingsSource`] is the test double and the `MARKET_SIGNAL_SCHWAB_FIXTURE`
//! escape hatch. Nothing downstream of the seam distinguishes the two.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::portfolio::AssetClass;

/// Cost basis and market value are account-currency totals (not per-share).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub description: String,
    pub asset_class: AssetClass,
    pub quantity: f64,
    pub cost_basis: f64,
    pub market_value: f64,
    /// Latest per-share price, when the source carried one.
    pub current_price: Option<f64>,
}

/// A snapshot of the holdings pulled from the connected account; the most recent pull
/// is stored (`docs/storage.md §Local Analysis Suite Storage`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Holdings {
    /// Book-level rows, one netted position per symbol once [`Holdings::normalized`]
    /// has run; every downstream contract consumes only these.
    pub positions: Vec<Position>,
    pub cash: f64,
    /// Sum of Schwab account liquidation values, the denominator for portfolio weights.
    /// The live adapter derives it only when an account has no explicit cash-position
    /// row and carries no liquidation value.
    pub account_total: f64,
    /// The pre-normalization per-source rows, retained for display and audit,
    /// including any explicit cash rows the live adapter folded into [`Holdings::cash`]
    /// (`docs/schwab-integration.md` §What is pulled). Empty on a not-yet-normalized
    /// pull.
    #[serde(default)]
    pub source_rows: Vec<Position>,
}

impl Holdings {
    /// Same-symbol rows across granted accounts (and manual supplements) consolidate
    /// into one book-level position per symbol (`docs/schwab-integration.md` §What is
    /// pulled). The uppercased symbol is the sole position identity; quantities,
    /// market values, and cost-basis totals each sum across rows, never a
    /// share-weighted average price, so dollar gain stays additive and the totals
    /// stay well-defined at zero net quantity; the long/short side comes from the
    /// netted quantity; identity fields come from the first row seen.
    ///
    /// Idempotent: a snapshot that already carries source rows is returned unchanged,
    /// so a re-normalization can never lose the raw rows.
    ///
    /// The lots are finite by the parser; their sums are not. A netted total that is
    /// not finite is `Err` naming the symbol, so the pull fails before any per-holding
    /// work rather than the run refusing at persist hours later.
    pub fn normalized(mut self) -> Result<Holdings> {
        if !self.source_rows.is_empty() {
            return Ok(self);
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
        for key in &order {
            let p = &merged[key];
            if !(p.quantity.is_finite() && p.cost_basis.is_finite() && p.market_value.is_finite()) {
                anyhow::bail!(
                    "holdings normalization: {key}'s netted quantity / cost basis / market value \
                     did not finish as a finite number — the lots' sums overflowed"
                );
            }
        }
        // The book's own sums are as unbounded as a lot's, and persist as required
        // floats.
        if !(self.cash.is_finite() && self.account_total.is_finite()) {
            anyhow::bail!(
                "holdings normalization: the book's summed cash / account total did not finish \
                 as a finite number — the accounts' sums overflowed"
            );
        }
        self.positions = order
            .into_iter()
            .map(|k| merged.remove(&k).expect("every ordered key was inserted"))
            .collect();
        self.source_rows = raw;
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OptionKind {
    Call,
    Put,
}

/// One contract row from an option chain: the fields the activity signal reads, plus
/// the delta the same-underlying option overlay matches on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionQuote {
    pub kind: OptionKind,
    pub strike: f64,
    /// ISO date (`YYYY-MM-DD`).
    pub expiry: String,
    pub volume: f64,
    pub open_interest: f64,
    pub implied_volatility: Option<f64>,
    /// The overlay's greek leg (`docs/portfolio-analysis.md` §The per-holding pipeline
    /// Step 1); the activity signal never reads it. `None` on the −999 sentinel or an
    /// out-of-range value.
    #[serde(default)]
    pub delta: Option<f64>,
}

/// A held option contract's identity, decoded from its OCC symbol
/// (`ROOT[padding]YYMMDDC|P########`, strike in thousandths), the symbol link the
/// same-underlying option overlay is built on (`docs/portfolio-workflow.md §Step 6a`).
/// A symbol that does not decode leaves the leg unrecognized, never a guessed identity.
#[derive(Debug, Clone, PartialEq)]
pub struct OccContract {
    /// Uppercased; a non-standard adjusted root ("AAPL1") decodes as itself and simply
    /// fails the symbol link.
    pub underlying: String,
    /// ISO date (`YYYY-MM-DD`).
    pub expiry: String,
    pub kind: OptionKind,
    pub strike: f64,
}

/// Decode an OCC option symbol. `None` unless the trailing 15 characters are a
/// calendar-valid `YYMMDD`, a `C`/`P`, and an 8-digit strike, with a non-empty
/// root before them.
pub fn parse_occ_symbol(symbol: &str) -> Option<OccContract> {
    let s = symbol.trim();
    if s.len() < 16 || !s.is_ascii() {
        return None;
    }
    let (root, tail) = s.split_at(s.len() - 15);
    let root = root.trim_end();
    let (date, rest) = tail.split_at(6);
    let (kind, strike_digits) = rest.split_at(1);
    if root.is_empty()
        || !date.bytes().all(|b| b.is_ascii_digit())
        || !strike_digits.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let kind = match kind {
        "C" => OptionKind::Call,
        "P" => OptionKind::Put,
        _ => return None,
    };
    // OCC dates are two-digit years in the 2000s.
    let expiry = format!("20{}-{}-{}", &date[..2], &date[2..4], &date[4..6]);
    chrono::NaiveDate::parse_from_str(&expiry, "%Y-%m-%d").ok()?;
    let strike = strike_digits.parse::<f64>().ok()? / 1000.0;
    if strike <= 0.0 {
        return None;
    }
    Some(OccContract {
        underlying: root.to_ascii_uppercase(),
        expiry,
        kind,
        strike,
    })
}

/// The raw rows the options-activity signal is computed from
/// (`docs/schwab-integration.md`); the signal itself lives in
/// [`crate::portfolio::engine`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionChain {
    pub underlying: String,
    /// The underlying's spot price when the chain was pulled, for an at-the-money read.
    pub underlying_price: Option<f64>,
    pub contracts: Vec<OptionQuote>,
}

/// The seam Portfolio Analysis pulls holdings and chains through. A connected Schwab
/// account is required; manual import only supplements holdings and does not satisfy
/// the gate (`docs/schwab-integration.md` §A connected Schwab account is required).
pub trait HoldingsSource {
    fn holdings(&self) -> Result<Holdings>;
    /// `None` when the source has no chain for the symbol.
    fn option_chain(&self, symbol: &str) -> Result<Option<OptionChain>>;
    /// The chain rows at one strike across a date window: the option overlay's
    /// targeted delta fetch, so the activity signal's bounded NTM query stays untouched
    /// (`docs/data-sources.md`). The default `None` means the source has no targeted
    /// path; the overlay's delta then reads as a typed gap, never a fabricated greek.
    fn option_chain_at_strike(
        &self,
        _symbol: &str,
        _strike: f64,
        _from: &str,
        _to: &str,
    ) -> Result<Option<OptionChain>> {
        Ok(None)
    }
    /// The default `option_chain_at_strike` answers `Ok(None)` without a wire call,
    /// indistinguishable from a live empty answer; this probe lets the audit label a
    /// consulted-but-empty targeted fetch without claiming a request the stub never
    /// made.
    fn supports_targeted_chain(&self) -> bool {
        false
    }
}

/// An offline, deterministic single-equity fixture: one stock position plus a small
/// option chain, so the whole pipeline runs with no Schwab connection.
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
        // Slightly more put volume/OI than call, and richer put IV: a mild
        // hedging-demand tilt the activity signal should surface.
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
                    delta: Some(0.52),
                },
                OptionQuote {
                    kind: OptionKind::Call,
                    strike: 205.0,
                    expiry: "2026-07-17".to_string(),
                    volume: 2_500.0,
                    open_interest: 8_000.0,
                    implied_volatility: Some(0.29),
                    delta: Some(0.30),
                },
                OptionQuote {
                    kind: OptionKind::Put,
                    strike: 195.0,
                    expiry: "2026-07-17".to_string(),
                    volume: 5_200.0,
                    open_interest: 15_000.0,
                    implied_volatility: Some(0.31),
                    delta: Some(-0.48),
                },
                OptionQuote {
                    kind: OptionKind::Put,
                    strike: 185.0,
                    expiry: "2026-07-17".to_string(),
                    volume: 3_100.0,
                    open_interest: 9_500.0,
                    implied_volatility: Some(0.34),
                    delta: Some(-0.30),
                },
            ],
        };
        Self { holdings, chain }
    }
}

impl FixtureHoldingsSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reuses the default option chain, served only for its AAPL underlying; any other
    /// symbol reports no chain.
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
    fn normalization_refuses_a_netted_sum_that_is_not_finite() {
        // Two parser-finite lots whose netted market value overflows.
        let err = snapshot(vec![
            row("AAPL", 1.0, 1.0, f64::MAX),
            row("aapl", 1.0, 1.0, f64::MAX),
        ])
        .normalized()
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("AAPL") && err.contains("not finish as a finite number"),
            "{err}"
        );
        // The book's sums too: every position finite, the cash and account total not.
        let book = Holdings {
            positions: vec![row("AAPL", 1.0, 1.0, 1.0)],
            cash: f64::MAX,
            account_total: f64::INFINITY,
            source_rows: vec![],
        };
        let err = book.normalized().unwrap_err().to_string();
        assert!(err.contains("cash / account total"), "{err}");
    }

    #[test]
    fn normalization_nets_same_symbol_rows_across_accounts() {
        let h = snapshot(vec![
            row("AAPL", 100.0, 14_000.0, 19_500.0),
            row("MSFT", 10.0, 3_000.0, 4_200.0),
            row("aapl", 50.0, 8_000.0, 9_750.0),
        ])
        .normalized().unwrap();
        assert_eq!(h.positions.len(), 2, "same-symbol rows merge case-insensitively");
        let aapl = &h.positions[0];
        assert_eq!(aapl.symbol, "AAPL");
        assert_eq!(aapl.quantity, 150.0);
        assert_eq!(aapl.cost_basis, 22_000.0);
        assert_eq!(aapl.market_value, 29_250.0);
        assert_eq!(h.positions[1].symbol, "MSFT");
        assert_eq!(h.source_rows.len(), 3);
    }

    #[test]
    fn normalization_reads_side_from_the_netted_quantity() {
        let h = snapshot(vec![
            row("XYZ", 100.0, 10_000.0, 12_000.0),
            row("XYZ", -140.0, -14_000.0, -16_800.0),
        ])
        .normalized().unwrap();
        assert_eq!(h.positions.len(), 1);
        let net = &h.positions[0];
        assert_eq!(net.quantity, -40.0);
        assert_eq!(net.cost_basis, -4_000.0);
        assert_eq!(net.market_value, -4_800.0);
    }

    #[test]
    fn normalization_keeps_dollar_gain_additive_at_zero_net_quantity() {
        let h = snapshot(vec![
            row("HEDG", 100.0, 9_000.0, 11_000.0),
            row("HEDG", -100.0, -10_000.0, -11_000.0),
        ])
        .normalized().unwrap();
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
        .normalized().unwrap();
        let twice = once.clone().normalized().unwrap();
        assert_eq!(once, twice, "re-normalizing an already-normalized snapshot is a no-op");
        assert_eq!(once.positions.len(), 2);
        assert_eq!(once.account_total, 23_700.0);
    }

    #[test]
    fn occ_symbols_decode_and_malformed_ones_stay_unrecognized() {
        // The standard padded form (root space-padded to six characters).
        let c = parse_occ_symbol("AAPL  270115C00210000").expect("decodes");
        assert_eq!(c.underlying, "AAPL");
        assert_eq!(c.expiry, "2027-01-15");
        assert_eq!(c.kind, OptionKind::Call);
        assert_eq!(c.strike, 210.0);
        let c = parse_occ_symbol("F260918P00011500").expect("decodes");
        assert_eq!(c.underlying, "F");
        assert_eq!(c.kind, OptionKind::Put);
        assert_eq!(c.strike, 11.5);
        // A plain ticker, a calendar-impossible date, a bad side letter, a non-digit
        // strike, and an empty root.
        for bad in [
            "AAPL",
            "AAPL  279915C00210000",
            "AAPL  270115X00210000",
            "AAPL  270115C0021000O",
            "      270115C00210000",
        ] {
            assert!(parse_occ_symbol(bad).is_none(), "{bad}");
        }
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
