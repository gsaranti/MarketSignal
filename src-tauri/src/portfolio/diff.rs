//! The deterministic holdings-change diff (`docs/portfolio-workflow.md` §Step 4;
//! `docs/portfolio-analysis.md` §Holdings change tracking). Each run persists the
//! holdings snapshot it ran against ([`crate::portfolio::PortfolioRun::holdings`]); the
//! next run diffs the current Schwab pull against that prior snapshot **in the
//! application layer, before any model stage** — the same compute-don't-guess boundary
//! the rest of the pipeline holds. Every current position is tagged
//! new / increased / decreased / unchanged, and a symbol present last run but absent
//! now is an exited position surfaced in the roll-up.
//!
//! Positions match across runs by **symbol only** (case-insensitive): a `Position`
//! carries no CUSIP or lot id, so symbol is the sole stable identity (matching
//! [`crate::portfolio::dossier::prior_verdict_for`]'s lookup). Classification keys off
//! **quantity** — the clean signal for what the user did — with a small tolerance so a
//! floating-point re-serialization of an unchanged position doesn't read as a trim.

use std::collections::{HashMap, HashSet};

use crate::portfolio::{ExitedPosition, PositionChange, PositionDelta};
use crate::schwab::Holdings;

/// Relative tolerance (against the larger of the prior quantity and 1.0) for treating
/// two quantities as unchanged, so fractional-share and unit-quantity noise doesn't
/// masquerade as an add/trim. A calibratable starting value, like the engine's
/// constants — pinned here rather than frozen.
pub const QUANTITY_EPSILON: f64 = 1e-6;

/// The result of diffing the current holdings against the prior run's snapshot: a
/// per-current-position delta (keyed by uppercased symbol) plus the exited positions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HoldingsDiff {
    /// Per current position, keyed by uppercased symbol.
    deltas: HashMap<String, PositionDelta>,
    /// Symbols present in the prior snapshot but absent now, sorted by symbol for a
    /// deterministic order (the underlying map iteration is not ordered).
    pub exited: Vec<ExitedPosition>,
}

impl HoldingsDiff {
    /// The delta for one current position by symbol (case-insensitive). Falls back to a
    /// `new` delta for a symbol not in the current holdings — defensive only, since
    /// every current position is inserted when the diff is built.
    pub fn delta_for(&self, symbol: &str) -> PositionDelta {
        self.deltas
            .get(&symbol.to_ascii_uppercase())
            .cloned()
            .unwrap_or_else(PositionDelta::new_position)
    }
}

/// Diff the current holdings against the prior run's snapshot. With no prior snapshot
/// (the first run, or an unreadable prior run) every position is `new` and nothing is
/// exited.
pub fn diff_holdings(prior: Option<&Holdings>, current: &Holdings) -> HoldingsDiff {
    let prior_positions = prior.map(|h| h.positions.as_slice()).unwrap_or(&[]);
    let prior_by_symbol: HashMap<String, &crate::schwab::Position> = prior_positions
        .iter()
        .map(|p| (p.symbol.to_ascii_uppercase(), p))
        .collect();

    let mut deltas = HashMap::with_capacity(current.positions.len());
    for pos in &current.positions {
        let key = pos.symbol.to_ascii_uppercase();
        let delta = match prior_by_symbol.get(&key) {
            None => PositionDelta::new_position(),
            Some(prev) => PositionDelta {
                change: classify(prev.quantity, pos.quantity),
                prior_quantity: Some(prev.quantity),
                prior_cost_basis: Some(prev.cost_basis),
            },
        };
        deltas.insert(key, delta);
    }

    let current_symbols: HashSet<String> = current
        .positions
        .iter()
        .map(|p| p.symbol.to_ascii_uppercase())
        .collect();
    let mut exited: Vec<ExitedPosition> = prior_by_symbol
        .iter()
        .filter(|(sym, _)| !current_symbols.contains(*sym))
        .map(|(_, p)| ExitedPosition {
            symbol: p.symbol.clone(),
            description: p.description.clone(),
            prior_quantity: p.quantity,
            prior_cost_basis: p.cost_basis,
            prior_market_value: p.market_value,
        })
        .collect();
    exited.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    HoldingsDiff { deltas, exited }
}

/// Classify a quantity move as increased / decreased / unchanged, treating a change
/// within [`QUANTITY_EPSILON`] (relative to the larger of the prior size and 1.0) as
/// unchanged.
///
/// A **same-sign** move compares position *size* (absolute quantity), so a short reads
/// as "what the user did": adding to a short (−50 → −100) is an increase and buying part
/// of it back (−100 → −50) is a decrease. For a long position (the equity slice's case)
/// this is identical to comparing the raw quantities.
///
/// A **sign flip** — the live source reports one net `quantity = longQuantity −
/// shortQuantity` ([`crate::schwab`]), so a symbol can go net-long → net-short between
/// pulls — is a material reversal, **never** unchanged even at equal magnitude. Its
/// direction is read from the signed swing (net-long → net-short is a decrease in long
/// exposure), and the dossier/prompt surface the full `prior → now` quantities so the
/// reversal is legible.
fn classify(prior_qty: f64, current_qty: f64) -> PositionChange {
    let scale = prior_qty.abs().max(1.0);
    let flipped = prior_qty != 0.0
        && current_qty != 0.0
        && prior_qty.is_sign_positive() != current_qty.is_sign_positive();
    let delta = if flipped {
        current_qty - prior_qty
    } else {
        current_qty.abs() - prior_qty.abs()
    };
    if delta.abs() <= QUANTITY_EPSILON * scale {
        PositionChange::Unchanged
    } else if delta > 0.0 {
        PositionChange::Increased
    } else {
        PositionChange::Decreased
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::AssetClass;
    use crate::schwab::Position;

    fn pos(symbol: &str, quantity: f64) -> Position {
        Position {
            symbol: symbol.into(),
            description: format!("{symbol} Inc."),
            asset_class: AssetClass::Stock,
            quantity,
            cost_basis: quantity * 100.0,
            market_value: quantity * 120.0,
            current_price: Some(120.0),
        }
    }

    fn holdings(positions: Vec<Position>) -> Holdings {
        let account_total = positions.iter().map(|p| p.market_value).sum::<f64>();
        Holdings {
            positions,
            cash: 0.0,
            account_total,
        }
    }

    #[test]
    fn no_prior_snapshot_tags_every_position_new() {
        let current = holdings(vec![pos("AAPL", 100.0), pos("MSFT", 50.0)]);
        let diff = diff_holdings(None, &current);
        assert_eq!(diff.delta_for("AAPL").change, PositionChange::New);
        assert_eq!(diff.delta_for("MSFT").change, PositionChange::New);
        assert!(diff.delta_for("AAPL").prior_quantity.is_none());
        assert!(diff.exited.is_empty());
    }

    #[test]
    fn increased_decreased_and_unchanged_are_classified_by_quantity() {
        let prior = holdings(vec![pos("AAPL", 100.0), pos("MSFT", 50.0), pos("GOOG", 10.0)]);
        let current = holdings(vec![pos("AAPL", 140.0), pos("MSFT", 30.0), pos("GOOG", 10.0)]);
        let diff = diff_holdings(Some(&prior), &current);
        assert_eq!(diff.delta_for("AAPL").change, PositionChange::Increased);
        assert_eq!(diff.delta_for("MSFT").change, PositionChange::Decreased);
        assert_eq!(diff.delta_for("GOOG").change, PositionChange::Unchanged);
        // The prior quantity/cost basis ride along for an existing position.
        assert_eq!(diff.delta_for("AAPL").prior_quantity, Some(100.0));
        assert_eq!(diff.delta_for("AAPL").prior_cost_basis, Some(10_000.0));
        assert!(diff.exited.is_empty());
    }

    #[test]
    fn a_tiny_fractional_wobble_reads_as_unchanged_not_a_trim() {
        let prior = holdings(vec![pos("VOO", 12.5)]);
        let current = holdings(vec![pos("VOO", 12.5 + 1e-9)]);
        assert_eq!(
            diff_holdings(Some(&prior), &current).delta_for("VOO").change,
            PositionChange::Unchanged
        );
    }

    #[test]
    fn a_symbol_absent_now_is_exited() {
        let prior = holdings(vec![pos("AAPL", 100.0), pos("MSFT", 50.0)]);
        let current = holdings(vec![pos("AAPL", 100.0)]);
        let diff = diff_holdings(Some(&prior), &current);
        assert_eq!(diff.exited.len(), 1);
        assert_eq!(diff.exited[0].symbol, "MSFT");
        assert_eq!(diff.exited[0].prior_quantity, 50.0);
        // The surviving position is not exited and reads unchanged.
        assert_eq!(diff.delta_for("AAPL").change, PositionChange::Unchanged);
    }

    #[test]
    fn short_positions_classify_by_size_not_signed_delta() {
        // A short position: adding to it (more negative) is an increase; buying part of
        // it back (toward zero) is a decrease — the opposite of what the raw signed
        // delta would say.
        let prior = holdings(vec![pos("SQQQ", -100.0)]);
        let bigger_short = holdings(vec![pos("SQQQ", -150.0)]);
        let smaller_short = holdings(vec![pos("SQQQ", -40.0)]);
        assert_eq!(
            diff_holdings(Some(&prior), &bigger_short).delta_for("SQQQ").change,
            PositionChange::Increased
        );
        assert_eq!(
            diff_holdings(Some(&prior), &smaller_short).delta_for("SQQQ").change,
            PositionChange::Decreased
        );
    }

    #[test]
    fn a_sign_flip_is_a_change_not_unchanged() {
        // The live source reports one net quantity (long − short), so a symbol can flip
        // from net long to net short between pulls at equal magnitude. That reversal must
        // not read as unchanged — the abs-magnitude compare alone would call it so.
        let long_then_short = diff_holdings(
            Some(&holdings(vec![pos("XYZ", 100.0)])),
            &holdings(vec![pos("XYZ", -100.0)]),
        );
        assert_eq!(
            long_then_short.delta_for("XYZ").change,
            PositionChange::Decreased,
            "net long → net short is a decrease in long exposure, not unchanged"
        );
        let short_then_long = diff_holdings(
            Some(&holdings(vec![pos("XYZ", -50.0)])),
            &holdings(vec![pos("XYZ", 50.0)]),
        );
        assert_eq!(
            short_then_long.delta_for("XYZ").change,
            PositionChange::Increased,
            "net short → net long is an increase"
        );
    }

    #[test]
    fn symbol_matching_is_case_insensitive() {
        let prior = holdings(vec![pos("aapl", 100.0)]);
        let current = holdings(vec![pos("AAPL", 130.0)]);
        let diff = diff_holdings(Some(&prior), &current);
        assert_eq!(diff.delta_for("AAPL").change, PositionChange::Increased);
        assert!(diff.exited.is_empty(), "case-only difference is not an exit");
    }

    #[test]
    fn exited_positions_are_sorted_deterministically() {
        let prior = holdings(vec![pos("ZM", 5.0), pos("AMD", 5.0), pos("NKE", 5.0)]);
        let current = holdings(vec![]);
        let diff = diff_holdings(Some(&prior), &current);
        let symbols: Vec<&str> = diff.exited.iter().map(|e| e.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["AMD", "NKE", "ZM"]);
    }
}
