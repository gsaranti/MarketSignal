//! The deterministic financial-analysis engine (`docs/portfolio-analysis.md` §The
//! per-holding pipeline, step 2; `docs/local-models.md §Context-memory discipline` —
//! "Compute, don't guess"). Every *number* in a holding's verdict originates here:
//! the four sub-scores, the composite grade they roll up to, the scenario price
//! targets with their methodology, and the options-activity signal. The model
//! interprets these values; it never invents one — so a missing input becomes a gap,
//! never a fabricated level.
//!
//! All formulas are simple, bounded, and **calibratable** — the grade-weight
//! formula, the risk-tier thresholds, and the options-signal parameters are the
//! constants this slice deliberately leaves open to shadow-tune against live runs
//! rather than pinning (the durable plan-time parameters live in
//! [`crate::portfolio`]). They are gathered at the top of the module so the
//! calibration surface is one place.

use serde::{Deserialize, Serialize};

use crate::portfolio::{
    Action, ActionSizing, Grade, InvestorProfile, OptionsSignal, PriceTarget, PriceTargets,
    SubScores,
};
use crate::schwab::{OptionChain, OptionKind, Position};

// ---- Calibration surface (NOT pinned — shadow-tune against live runs) ---------

/// Composite-grade weights over the four sub-scores. `grade_from_subscores` divides
/// by their sum, so they need not total 1.0. A sub-score that could not be computed is
/// imputed to the neutral midpoint (50) by `analyze` before the roll-up, so a missing
/// input pulls the composite toward neutral rather than being dropped — an
/// impute-to-neutral, not a renormalization over the present sub-scores.
const W_QUALITY: f64 = 0.30;
const W_VALUATION: f64 = 0.25;
const W_MOMENTUM: f64 = 0.20;
const W_RISK: f64 = 0.25;

/// Composite-score cutoffs for each letter grade (0–100, higher better).
const GRADE_A: f64 = 85.0;
const GRADE_B: f64 = 70.0;
const GRADE_C: f64 = 55.0;
const GRADE_D: f64 = 40.0;

/// Evidence floor: the minimum number of computable sub-scores below which the
/// holding abstains rather than grading on too little (`docs/portfolio-analysis.md`
/// §Evidence floor).
const MIN_SUBSCORES_FOR_GRADE: usize = 2;

/// Fallback scenario half-bands (fraction of the base target) when realized
/// volatility can't be computed, for the end-of-year and end-of-month horizons.
const EOY_FALLBACK_BAND: f64 = 0.15;
const EOM_FALLBACK_BAND: f64 = 0.05;

// ---- Inputs ------------------------------------------------------------------

/// The normalized financial inputs the engine reasons over, assembled by the dossier
/// from FMP per-company data and SEC EDGAR facts (`docs/data-sources.md`). Every
/// field is optional: a source that can't resolve a line records it in [`Self::gaps`]
/// rather than supplying a fabricated value, so the engine grades over what is
/// actually present.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyFinancials {
    pub symbol: String,
    pub current_price: Option<f64>,
    pub market_cap: Option<f64>,
    pub shares_outstanding: Option<f64>,
    /// Most-recent and prior-period revenue (the growth numerator/denominator).
    pub revenue: Option<f64>,
    pub revenue_prior: Option<f64>,
    pub gross_profit: Option<f64>,
    pub operating_income: Option<f64>,
    pub net_income: Option<f64>,
    pub eps: Option<f64>,
    pub total_debt: Option<f64>,
    pub total_equity: Option<f64>,
    pub free_cash_flow: Option<f64>,
    pub pe_ratio: Option<f64>,
    pub ps_ratio: Option<f64>,
    pub pb_ratio: Option<f64>,
    /// Chronological closing prices (oldest first), for momentum and volatility.
    pub price_history: Vec<f64>,
    /// Tagged inputs a source could not resolve, carried into the prompt so the model
    /// reasons over what is absent rather than inferring it.
    pub gaps: Vec<String>,
}

/// The raw computed metrics behind the sub-scores — recorded on the run's audit so a
/// verdict's basis is inspectable, and rendered into the interpretation prompt so the
/// model reasons over real figures. Each is `None` when its inputs were missing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ComputedMetrics {
    pub net_margin: Option<f64>,
    pub gross_margin: Option<f64>,
    pub revenue_growth: Option<f64>,
    pub debt_to_equity: Option<f64>,
    pub return_volatility: Option<f64>,
    pub trailing_return: Option<f64>,
    pub pe_ratio: Option<f64>,
    pub ps_ratio: Option<f64>,
    pub pb_ratio: Option<f64>,
}

/// The engine's analyzed output for a holding that cleared the evidence floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineOutput {
    pub sub_scores: SubScores,
    pub grade: Grade,
    pub metrics: ComputedMetrics,
    pub price_targets: PriceTargets,
}

/// What the engine resolved to: an analysis, or an explicit abstention when the
/// evidence floor was not met (`docs/portfolio-analysis.md` §Evidence floor).
#[derive(Debug, Clone, PartialEq)]
pub enum EngineVerdict {
    Analyzed(Box<EngineOutput>),
    InsufficientEvidence(String),
}

// ---- The engine --------------------------------------------------------------

/// Analyze a holding's financials into sub-scores, a grade, and scenario targets — or
/// abstain. The evidence floor fails when there is no current price (nothing to
/// target or value against) or fewer than [`MIN_SUBSCORES_FOR_GRADE`] sub-scores are
/// computable; either is an explicit `insufficient-evidence`, never a low-conviction
/// guess.
pub fn analyze(fin: &CompanyFinancials) -> EngineVerdict {
    let metrics = compute_metrics(fin);

    let quality = quality_score(&metrics);
    let valuation = valuation_score(&metrics);
    let momentum = momentum_score(&metrics);
    let risk = risk_score(&metrics);

    let computed = [quality, valuation, momentum, risk]
        .iter()
        .filter(|s| s.is_some())
        .count();

    let Some(price) = fin.current_price else {
        return EngineVerdict::InsufficientEvidence(
            "no current price for the holding — cannot value or set targets".to_string(),
        );
    };
    if computed < MIN_SUBSCORES_FOR_GRADE {
        return EngineVerdict::InsufficientEvidence(format!(
            "only {computed} of 4 sub-scores computable (need {MIN_SUBSCORES_FOR_GRADE}); \
             financial inputs too sparse to grade"
        ));
    }

    // A missing sub-score takes the neutral midpoint (50) so the composite stays
    // defined; dividing by the full fixed weight sum keeps it on the same 0–100 scale
    // (an impute-to-neutral, not a renormalization over the present sub-scores). The
    // count gate above guarantees at least two are real, so this never grades on all
    // defaults.
    let sub_scores = SubScores {
        quality: quality.unwrap_or(50.0),
        valuation: valuation.unwrap_or(50.0),
        momentum: momentum.unwrap_or(50.0),
        risk: risk.unwrap_or(50.0),
    };
    let grade = grade_from_subscores(&sub_scores);
    let price_targets = scenario_targets(price, &metrics);

    EngineVerdict::Analyzed(Box::new(EngineOutput {
        sub_scores,
        grade,
        metrics,
        price_targets,
    }))
}

/// Roll the four sub-scores up to a letter grade through the fixed weights. Public so
/// a reviewer (and the live smoke) can assert the roll-up directly.
pub fn grade_from_subscores(s: &SubScores) -> Grade {
    let composite = (s.quality * W_QUALITY
        + s.valuation * W_VALUATION
        + s.momentum * W_MOMENTUM
        + s.risk * W_RISK)
        / (W_QUALITY + W_VALUATION + W_MOMENTUM + W_RISK);
    if composite >= GRADE_A {
        Grade::A
    } else if composite >= GRADE_B {
        Grade::B
    } else if composite >= GRADE_C {
        Grade::C
    } else if composite >= GRADE_D {
        Grade::D
    } else {
        Grade::F
    }
}

fn compute_metrics(fin: &CompanyFinancials) -> ComputedMetrics {
    let ratio = |num: Option<f64>, den: Option<f64>| match (num, den) {
        (Some(n), Some(d)) if d != 0.0 => Some(n / d),
        _ => None,
    };
    ComputedMetrics {
        net_margin: ratio(fin.net_income, fin.revenue),
        gross_margin: ratio(fin.gross_profit, fin.revenue),
        revenue_growth: match (fin.revenue, fin.revenue_prior) {
            (Some(now), Some(prior)) if prior > 0.0 => Some(now / prior - 1.0),
            _ => None,
        },
        debt_to_equity: ratio(fin.total_debt, fin.total_equity),
        return_volatility: return_volatility(&fin.price_history),
        trailing_return: trailing_return(&fin.price_history),
        pe_ratio: fin.pe_ratio,
        ps_ratio: fin.ps_ratio,
        pb_ratio: fin.pb_ratio,
    }
}

/// Linearly map `value` from `[lo, hi]` onto a 0–100 score, clamped at the ends.
/// `lo` maps to 0 and `hi` to 100 (pass `lo > hi` to invert — lower input scores
/// higher).
fn scale(value: f64, lo: f64, hi: f64) -> f64 {
    let t = (value - lo) / (hi - lo);
    (t.clamp(0.0, 1.0)) * 100.0
}

/// Average the present components, or `None` when none are present.
fn average(parts: &[Option<f64>]) -> Option<f64> {
    let present: Vec<f64> = parts.iter().filter_map(|p| *p).collect();
    if present.is_empty() {
        None
    } else {
        Some(present.iter().sum::<f64>() / present.len() as f64)
    }
}

/// Quality (higher better): profitability and cash generation.
fn quality_score(m: &ComputedMetrics) -> Option<f64> {
    average(&[
        m.net_margin.map(|x| scale(x, 0.0, 0.25)),
        m.gross_margin.map(|x| scale(x, 0.20, 0.60)),
    ])
}

/// Valuation (higher better == cheaper): inverted multiples. A negative P/E (no
/// earnings) is not "cheap" — it scores low rather than off the scale.
fn valuation_score(m: &ComputedMetrics) -> Option<f64> {
    let pe = m.pe_ratio.map(|x| if x <= 0.0 { 20.0 } else { scale(x, 40.0, 10.0) });
    average(&[
        pe,
        m.ps_ratio.map(|x| scale(x, 10.0, 1.0)),
        m.pb_ratio.map(|x| scale(x, 8.0, 1.0)),
    ])
}

/// Momentum (higher better): trailing price return over the available history.
fn momentum_score(m: &ComputedMetrics) -> Option<f64> {
    m.trailing_return.map(|r| scale(r, -0.30, 0.30))
}

/// Risk (higher == safer): low realized volatility and low leverage.
fn risk_score(m: &ComputedMetrics) -> Option<f64> {
    average(&[
        // 0% per-period vol → 100 (safest); 4%+ → 0.
        m.return_volatility.map(|v| scale(v, 0.04, 0.0)),
        // No leverage → 100; debt 2× equity or more → 0.
        m.debt_to_equity.map(|d| scale(d, 2.0, 0.0)),
    ])
}

/// Simple total return from the first to the last close.
fn trailing_return(history: &[f64]) -> Option<f64> {
    match (history.first(), history.last()) {
        (Some(&first), Some(&last)) if history.len() >= 2 && first > 0.0 => Some(last / first - 1.0),
        _ => None,
    }
}

/// Population standard deviation of simple period-over-period returns.
fn return_volatility(history: &[f64]) -> Option<f64> {
    if history.len() < 3 {
        return None;
    }
    let returns: Vec<f64> = history
        .windows(2)
        .filter_map(|w| if w[0] > 0.0 { Some(w[1] / w[0] - 1.0) } else { None })
        .collect();
    if returns.len() < 2 {
        return None;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    Some(var.sqrt())
}

/// Scenario end-of-month and end-of-year targets from the spot price, an expected
/// annual return blended from growth and a modest drift, and a volatility-scaled
/// bull/bear band — methodology recorded on each target so the basis is inspectable.
///
/// This is the v1 drift formula. The settled replacement is the v2 rate-anchored
/// forward-multiple function (driver ladder × DGS10 spread-anchored scenario
/// multiples) specified in docs/portfolio-analysis.md §Starting parameters —
/// docs-lead-code; it lands with the full Portfolio slice alongside the
/// per-branch risk-tier assignment the same section defines.
fn scenario_targets(price: f64, m: &ComputedMetrics) -> PriceTargets {
    // Expected annual drift: half the revenue growth (a deliberately conservative
    // pass-through) plus a 2% baseline, bounded so a single noisy input can't imply an
    // extreme target.
    let growth = m.revenue_growth.unwrap_or(0.0);
    let annual = (0.5 * growth + 0.02).clamp(-0.25, 0.35);

    // Per-period volatility annualizes loosely; fall back to a fixed band when absent.
    let eoy_band = m
        .return_volatility
        .map(|v| (v * 6.0).clamp(0.05, 0.40))
        .unwrap_or(EOY_FALLBACK_BAND);
    let eom_band = m
        .return_volatility
        .map(|v| (v * 2.0).clamp(0.02, 0.15))
        .unwrap_or(EOM_FALLBACK_BAND);

    let eoy_base = price * (1.0 + annual);
    let eom_base = price * (1.0 + annual / 12.0);

    let methodology = |horizon: &str, band: f64| {
        format!(
            "{horizon} base = spot × (1 + expected annual return {:.1}% scaled to horizon); \
             bull/bear = base ± {:.1}% from realized volatility",
            annual * 100.0,
            band * 100.0
        )
    };

    PriceTargets {
        end_of_month: Some(PriceTarget {
            base: eom_base,
            bear: eom_base * (1.0 - eom_band),
            bull: eom_base * (1.0 + eom_band),
            methodology: methodology("End-of-month", eom_band),
        }),
        end_of_year: Some(PriceTarget {
            base: eoy_base,
            bear: eoy_base * (1.0 - eoy_band),
            bull: eoy_base * (1.0 + eoy_band),
            methodology: methodology("End-of-year", eoy_band),
        }),
    }
}

// ---- Options-activity signal (kept out of the grade) -------------------------

/// Compute the per-stock options-activity signal from the chain (`docs/schwab-integration.md`).
/// A rough activity *proxy* — put/call by volume and open interest, an at-the-money
/// IV read, and the put-minus-call IV skew — **never folded into the grade
/// sub-scores** until calibration shows it adds value; it grounds the narrative read
/// only. Any field is `None` when the chain lacked the data.
pub fn options_signal(chain: &OptionChain) -> OptionsSignal {
    let sum = |kind: OptionKind, f: fn(&crate::schwab::OptionQuote) -> f64| -> f64 {
        chain
            .contracts
            .iter()
            .filter(|c| c.kind == kind)
            .map(f)
            .sum()
    };
    let call_vol = sum(OptionKind::Call, |c| c.volume);
    let put_vol = sum(OptionKind::Put, |c| c.volume);
    let call_oi = sum(OptionKind::Call, |c| c.open_interest);
    let put_oi = sum(OptionKind::Put, |c| c.open_interest);

    let ratio = |put: f64, call: f64| if call > 0.0 { Some(put / call) } else { None };

    let avg_iv = |kind: OptionKind| -> Option<f64> {
        let ivs: Vec<f64> = chain
            .contracts
            .iter()
            .filter(|c| c.kind == kind)
            .filter_map(|c| c.implied_volatility)
            .collect();
        if ivs.is_empty() {
            None
        } else {
            Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
        }
    };
    let call_iv = avg_iv(OptionKind::Call);
    let put_iv = avg_iv(OptionKind::Put);
    let implied_volatility = average(&[call_iv, put_iv]);
    let iv_skew = match (put_iv, call_iv) {
        (Some(p), Some(c)) => Some(p - c),
        _ => None,
    };

    OptionsSignal {
        put_call_volume: ratio(put_vol, call_vol),
        put_call_open_interest: ratio(put_oi, call_oi),
        implied_volatility,
        iv_skew,
    }
}

// ---- Action sizing -----------------------------------------------------------

/// Derive the deterministic action sizing once the model has chosen the action rung
/// (`docs/portfolio-analysis.md` §The holding verdict). Each rung maps to a target
/// portfolio-weight band relative to the position's current weight; the share/dollar
/// delta reaches the band's midpoint. An `add`/`add aggressively` is bounded by the
/// profile's available cash when it sets a finite cap; `None` cash is unconstrained
/// (the fixed preset's stance). Calibratable: the per-rung band steps are an open
/// parameter, not pinned. No orders are placed.
pub fn size_action(
    action: Action,
    position: &Position,
    profile: &InvestorProfile,
    account_total: f64,
) -> ActionSizing {
    let current_weight = if account_total > 0.0 {
        position.market_value / account_total
    } else {
        0.0
    };

    // Per-rung multiplicative target around the current weight (a simple, legible
    // ladder; concentration is bounded by the cap below).
    let (low_mult, high_mult) = match action {
        Action::SellAll => (0.0, 0.0),
        Action::Trim => (0.4, 0.7),
        Action::Hold => (0.9, 1.1),
        Action::Add => (1.2, 1.6),
        Action::AddAggressively => (1.6, 2.2),
    };
    // Concentration cap: a single position is not steered above 25% of the account.
    const MAX_SINGLE_WEIGHT: f64 = 0.25;
    let target_low = (current_weight * low_mult).min(MAX_SINGLE_WEIGHT);
    let target_high = (current_weight * high_mult).min(MAX_SINGLE_WEIGHT);
    let target_mid = (target_low + target_high) / 2.0;

    let (est_dollar_delta, est_share_delta) = match position.current_price {
        Some(price) if account_total > 0.0 && price > 0.0 => {
            let target_value = target_mid * account_total;
            let mut dollar_delta = target_value - position.market_value;
            // A buy is bounded by the profile's cash cap when set; `None` means cash is
            // unconstrained (the fixed preset's stance — the user may hold cash the app
            // can't see), so adds are not gated on observed Schwab cash
            // (`docs/configuration.md` §Investor Profile). INFINITY ⇒ no cap.
            if dollar_delta > 0.0 {
                dollar_delta = dollar_delta.min(profile.available_cash.unwrap_or(f64::INFINITY));
            }
            (Some(dollar_delta), Some(dollar_delta / price))
        }
        _ => (None, None),
    };

    ActionSizing {
        target_weight_low: target_low,
        target_weight_high: target_high,
        est_share_delta,
        est_dollar_delta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::AssetClass;
    use crate::schwab::{OptionQuote, OptionKind};

    /// A healthy large-cap: strong margins, modest leverage, an uptrend — a clear A/B.
    fn strong() -> CompanyFinancials {
        CompanyFinancials {
            symbol: "AAPL".into(),
            current_price: Some(195.0),
            market_cap: Some(3.0e12),
            shares_outstanding: Some(1.5e10),
            revenue: Some(400.0),
            revenue_prior: Some(360.0),
            gross_profit: Some(180.0),
            operating_income: Some(120.0),
            net_income: Some(100.0),
            eps: Some(6.5),
            total_debt: Some(100.0),
            total_equity: Some(200.0),
            free_cash_flow: Some(95.0),
            pe_ratio: Some(28.0),
            ps_ratio: Some(7.5),
            pb_ratio: Some(6.0),
            price_history: vec![170.0, 175.0, 180.0, 188.0, 195.0],
            gaps: vec![],
        }
    }

    #[test]
    fn strong_company_grades_and_computes_targets() {
        match analyze(&strong()) {
            EngineVerdict::Analyzed(out) => {
                // Every sub-score is in range; the grade is a deterministic roll-up.
                for s in [
                    out.sub_scores.quality,
                    out.sub_scores.valuation,
                    out.sub_scores.momentum,
                    out.sub_scores.risk,
                ] {
                    assert!((0.0..=100.0).contains(&s), "{s}");
                }
                // Strong margins + uptrend keep it out of the failing tiers.
                assert!(matches!(out.grade, Grade::A | Grade::B | Grade::C), "{:?}", out.grade);
                let eoy = out.price_targets.end_of_year.unwrap();
                assert!(eoy.bear < eoy.base && eoy.base < eoy.bull, "ordered scenarios");
                assert!(eoy.methodology.contains("base = spot"));
            }
            other => panic!("expected an analysis, got {other:?}"),
        }
    }

    #[test]
    fn grade_is_deterministic_for_the_same_inputs() {
        let a = analyze(&strong());
        let b = analyze(&strong());
        assert_eq!(a, b, "same financials always grade identically");
    }

    #[test]
    fn missing_price_abstains_below_the_evidence_floor() {
        let mut fin = strong();
        fin.current_price = None;
        match analyze(&fin) {
            EngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("no current price"), "{reason}");
            }
            other => panic!("expected abstention, got {other:?}"),
        }
    }

    #[test]
    fn too_few_subscores_abstains() {
        // Only a price and a single multiple — one sub-score (valuation) at most.
        let fin = CompanyFinancials {
            symbol: "X".into(),
            current_price: Some(50.0),
            ps_ratio: Some(3.0),
            ..CompanyFinancials::default()
        };
        match analyze(&fin) {
            EngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("sub-scores"), "{reason}");
            }
            other => panic!("expected abstention, got {other:?}"),
        }
    }

    #[test]
    fn grade_bands_are_monotone() {
        let f = |v: f64| {
            grade_from_subscores(&SubScores {
                quality: v,
                valuation: v,
                momentum: v,
                risk: v,
            })
        };
        assert_eq!(f(95.0), Grade::A);
        assert_eq!(f(72.0), Grade::B);
        assert_eq!(f(60.0), Grade::C);
        assert_eq!(f(45.0), Grade::D);
        assert_eq!(f(10.0), Grade::F);
    }

    #[test]
    fn options_signal_reads_put_skew_from_the_chain() {
        let chain = OptionChain {
            underlying: "AAPL".into(),
            underlying_price: Some(195.0),
            contracts: vec![
                OptionQuote {
                    kind: OptionKind::Call,
                    strike: 195.0,
                    expiry: "2026-07-17".into(),
                    volume: 1000.0,
                    open_interest: 5000.0,
                    implied_volatility: Some(0.25),
                },
                OptionQuote {
                    kind: OptionKind::Put,
                    strike: 195.0,
                    expiry: "2026-07-17".into(),
                    volume: 2000.0,
                    open_interest: 9000.0,
                    implied_volatility: Some(0.33),
                },
            ],
        };
        let sig = options_signal(&chain);
        assert!((sig.put_call_volume.unwrap() - 2.0).abs() < 1e-9);
        assert!((sig.put_call_open_interest.unwrap() - 1.8).abs() < 1e-9);
        // Puts richer than calls → positive skew (a hedging-demand tell).
        assert!(sig.iv_skew.unwrap() > 0.0);
    }

    #[test]
    fn size_action_caps_a_buy_by_available_cash_and_concentration() {
        let position = Position {
            symbol: "AAPL".into(),
            description: "Apple".into(),
            asset_class: AssetClass::Stock,
            quantity: 100.0,
            cost_basis: 14_000.0,
            market_value: 19_500.0,
            current_price: Some(195.0),
        };
        let mut profile = InvestorProfile::default_fixture();
        profile.available_cash = Some(1_000.0); // tight cash
        let sizing = size_action(Action::AddAggressively, &position, &profile, 29_500.0);
        // The dollar delta to add is capped by the $1,000 cash on hand.
        assert!(sizing.est_dollar_delta.unwrap() <= 1_000.0 + 1e-6);
        // The target band never steers a single name above the 25% concentration cap.
        assert!(sizing.target_weight_high <= 0.25 + 1e-9);
    }

    #[test]
    fn size_action_unconstrained_cash_does_not_cap_a_buy() {
        // A small position with lots of headroom, so add-aggressively wants to buy.
        let position = Position {
            symbol: "AAPL".into(),
            description: "Apple".into(),
            asset_class: AssetClass::Stock,
            quantity: 10.0,
            cost_basis: 900.0,
            market_value: 1_000.0,
            current_price: Some(100.0),
        };
        let account_total = 100_000.0;
        // The fixed preset (available_cash: None) treats cash as unconstrained — the buy
        // is sized to the concentration-bounded band, not clamped by observed cash.
        let unconstrained = size_action(
            Action::AddAggressively,
            &position,
            &InvestorProfile::default_fixture(),
            account_total,
        );
        // A tight finite cap clamps the very same buy far smaller.
        let mut tight = InvestorProfile::default_fixture();
        tight.available_cash = Some(500.0);
        let capped = size_action(Action::AddAggressively, &position, &tight, account_total);
        assert!(
            unconstrained.est_dollar_delta.unwrap() > capped.est_dollar_delta.unwrap(),
            "unconstrained cash must not clamp the buy the way a finite cap does"
        );
        assert!(capped.est_dollar_delta.unwrap() <= 500.0 + 1e-6);
    }

    #[test]
    fn size_action_sell_all_targets_zero_weight() {
        let position = Position {
            symbol: "AAPL".into(),
            description: "Apple".into(),
            asset_class: AssetClass::Stock,
            quantity: 100.0,
            cost_basis: 14_000.0,
            market_value: 19_500.0,
            current_price: Some(195.0),
        };
        let sizing = size_action(
            Action::SellAll,
            &position,
            &InvestorProfile::default_fixture(),
            29_500.0,
        );
        assert_eq!(sizing.target_weight_high, 0.0);
        // Selling the whole position is a negative dollar delta of its full value.
        assert!(sizing.est_dollar_delta.unwrap() < 0.0);
    }
}
