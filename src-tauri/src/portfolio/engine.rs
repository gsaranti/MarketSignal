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

/// Composite-grade weights over the **letter** sub-scores (`docs/portfolio-analysis.md`
/// §Starting parameters — the settled ex-momentum re-weight: momentum is computed
/// alongside but lives **outside the letter**, re-homed to the market-setup read in
/// the conviction / positioning context, because a price move is the one grade input
/// that isn't a business fundamental). `grade_from_subscores` divides by their sum,
/// so they need not total 1.0. A sub-score that could not be computed is imputed to
/// the neutral midpoint (50) by `analyze` before the roll-up, so a missing input
/// pulls the composite toward neutral rather than being dropped — an
/// impute-to-neutral, not a renormalization over the present sub-scores.
const W_QUALITY: f64 = 0.40;
const W_VALUATION: f64 = 0.30;
const W_RISK: f64 = 0.30;

/// Composite-score cutoffs for each letter grade (0–100, higher better).
const GRADE_A: f64 = 85.0;
const GRADE_B: f64 = 70.0;
const GRADE_C: f64 = 55.0;
const GRADE_D: f64 = 40.0;

/// Evidence floor: the minimum number of computable **letter** sub-scores
/// (quality / valuation / risk — momentum is context, not a letter input) below
/// which the holding abstains rather than grading on too little
/// (`docs/portfolio-analysis.md` §Evidence floor).
const MIN_SUBSCORES_FOR_GRADE: usize = 2;

/// Fallback one-month scenario half-band (fraction of the base target) when
/// realized volatility can't be computed. The twelve-month band needs no fallback
/// under v2 — its bear/bull ARE the scenario prices.
const ONE_MONTH_FALLBACK_BAND: f64 = 0.05;

// -- v2 rate-anchored scenario-target function (`docs/portfolio-analysis.md`
//    §Starting parameters — the settled shape; every rate/return is a decimal ratio).

/// The trailing anchor window the scenario multiple re-anchors over, in quarters.
const ANCHOR_WINDOW_QUARTERS: usize = 12;

/// Fewer admissible anchor observations than this drops the rate correction entirely
/// (raw multiple percentiles, direct-mapped, recorded).
const MIN_ANCHOR_OBSERVATIONS: usize = 8;

/// A scenario whose `spread_s + DGS10_now` falls below this guard (a degenerate
/// reciprocal denominator) falls back to its raw multiple percentile, recorded.
const DEGENERATE_DENOMINATOR_EPS: f64 = 0.01;

/// Filing grace applied when a quarterly statement carries no filing date: the anchor
/// date is the period end plus this many days (the suite's freshness-basis constant).
const FILING_GRACE_DAYS: i64 = 45;

/// Sanity clamp on the implied annual driver growth versus the trailing print
/// (the v1 bound, retained by the v2 ladder).
const DRIVER_GROWTH_MIN: f64 = -0.25;
const DRIVER_GROWTH_MAX: f64 = 0.35;

/// The scenario-target function's parameter version, stamped on each run's audit so
/// target calibration never mixes v1 and v2 bases (`docs/portfolio-analysis.md`
/// §Outcome learning).
pub const SCENARIO_TARGET_PARAMETER_VERSION: &str = "targets-v2";

// -- Risk tiers and the capital-efficiency hurdle (`docs/portfolio-analysis.md`
//    §Starting parameters).

/// Tier-scaled hurdle premium over the run-level `DGS2` (decimal ratios).
const TIER_PREMIUM_LOW: f64 = 0.03;
const TIER_PREMIUM_MEDIUM: f64 = 0.05;
const TIER_PREMIUM_HIGH: f64 = 0.08;

/// Stock tier legs (Trade Opportunities' canonical constants — `docs/trade-opportunities.md`
/// §Starting parameters; Portfolio adopts the rule under its stated missing-input rule).
const TIER_HIGH_MAX_MCAP: f64 = 2.0e9;
const TIER_LOW_MIN_MCAP: f64 = 10.0e9;
const TIER_HIGH_MIN_ANNUAL_VOL: f64 = 0.40;
const TIER_LOW_MAX_ANNUAL_VOL: f64 = 0.25;
const TIER_HIGH_MIN_DEBT_EQUITY: f64 = 2.0;
const TIER_LOW_MAX_DEBT_EQUITY: f64 = 1.0;
const TIER_HIGH_MIN_DRAWDOWN: f64 = 0.50;

/// Loose annualization for the per-period (daily) realized volatility the engine
/// computes, used by the tier legs (√252 for daily bars).
const ANNUALIZATION_FACTOR: f64 = 15.87;

// -- Action-sizing floors (`docs/portfolio-analysis.md` §Starting parameters — the
//    settled add-family absolute target floors).

/// The minimum book weight an *add* action targets (pure current-weight multipliers
/// perpetuate historical accidents), and the *add aggressively* floor. The 25%
/// concentration cap always binds above.
const ADD_FLOOR_WEIGHT: f64 = 0.015;
const ADD_AGGRESSIVE_FLOOR_WEIGHT: f64 = 0.03;

/// Concentration cap: a single position is not steered above this share of the
/// account, and at-or-above it the add family leaves the feasible set.
const MAX_SINGLE_WEIGHT: f64 = 0.25;

// ---- Inputs ------------------------------------------------------------------

/// One dated observation (an ISO `YYYY-MM-DD` date and a value) — daily closes, rate
/// prints, sector-P/E samples. Dates sort lexicographically in this form, which the
/// latest-on-or-before joins rely on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DatedValue {
    pub date: String,
    pub value: f64,
}

/// The run-level rate anchors (`docs/portfolio-workflow.md` §Step 5): the `DGS2` and
/// `DGS10` prints plus the dated `DGS10` anchor-window history the v2 percentile join
/// reads. All values are **decimal ratios** (`4.5%` → `0.045` — the suite's shared
/// rate representation, normalized at the adapter seam).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RateAnchors {
    pub dgs2: f64,
    pub dgs10: f64,
    /// Dated `DGS10` observations covering the trailing anchor window plus alignment
    /// slack, sorted oldest-first.
    pub dgs10_history: Vec<DatedValue>,
    /// A degraded-input note when the anchor-window history request failed: the run
    /// proceeds with an empty admissible window — every spread observation
    /// inadmissible, the targets on their documented raw-percentile / carry fallback
    /// — never a new failure state (`docs/portfolio-analysis.md` §Starting
    /// parameters). Only the two run-level prints hard-fail (§Failure posture).
    #[serde(default)]
    pub history_gap: Option<String>,
}

/// One quarterly income-statement print (newest first in
/// [`CompanyFinancials::quarterly_income`]) — the trailing driver prints the v2
/// anchor window joins on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuarterlyIncomeRow {
    /// Period end, ISO date.
    pub period_end: String,
    /// The statement feed's filing date; absent, the anchor date falls back to the
    /// period end plus the drafted filing grace.
    pub filing_date: Option<String>,
    pub revenue: Option<f64>,
    pub eps_diluted: Option<f64>,
    pub diluted_shares: Option<f64>,
}

/// The forward consensus for the nearest coming fiscal year — the v2 driver ladder's
/// source (`analyst-estimates`). Mid is the consensus average; low / high bound the
/// bear / bull scenario drivers where the spread is published.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConsensusEstimate {
    /// The estimate's fiscal-period end, ISO date.
    pub period_end: String,
    pub eps_low: Option<f64>,
    pub eps_mid: Option<f64>,
    pub eps_high: Option<f64>,
    pub revenue_low: Option<f64>,
    pub revenue_mid: Option<f64>,
    pub revenue_high: Option<f64>,
}

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
    /// Deep **dated** daily closes (oldest first) — the v2 anchor join's price side
    /// and the drawdown read (`Stooq`, `docs/data-sources.md §Stooq`).
    #[serde(default)]
    pub daily_closes: Vec<DatedValue>,
    /// Trailing quarterly income prints, newest first — the v2 anchor window's
    /// trailing driver source (needs ~4 extra quarters beyond the window for TTM).
    #[serde(default)]
    pub quarterly_income: Vec<QuarterlyIncomeRow>,
    /// The forward consensus (nearest coming fiscal year) — the v2 driver ladder.
    #[serde(default)]
    pub consensus: Option<ConsensusEstimate>,
    /// Trailing-twelve-month dividends per share — the forward-dividend estimate the
    /// twelve-month total return adds (a sustainable basis, never a raw special).
    #[serde(default)]
    pub ttm_dividends_per_share: Option<f64>,
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
    /// Fund context: the reported expense ratio (decimal), where the holding is a
    /// fund. `#[serde(default)]` keeps pre-field audits decodable.
    #[serde(default)]
    pub expense_ratio: Option<f64>,
    /// Fund context: price-vs-NAV premium (decimal; meaningful on the closed-end
    /// form, context elsewhere).
    #[serde(default)]
    pub nav_premium: Option<f64>,
    /// Fund context: the share of fund weight the exposure-priced composite actually
    /// prices (`docs/portfolio-analysis.md` §Asset eligibility — the uncovered share
    /// is reported beside the read, never averaged in).
    #[serde(default)]
    pub composite_coverage: Option<f64>,
}

/// The three-state hurdle read plus the scenario total returns it tested
/// (`docs/portfolio-analysis.md` §Starting parameters — the dead-money hurdle).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HurdleRead {
    pub state: crate::portfolio::HurdleState,
    /// The tier-scaled hurdle rate tested (decimal ratio): `DGS2 + tier premium`.
    pub hurdle_rate: Option<f64>,
    /// Twelve-month scenario **total returns** (price + forward dividends), decimal.
    pub tr_bear: Option<f64>,
    pub tr_base: Option<f64>,
    pub tr_bull: Option<f64>,
    /// Whether the base-case total return clears the hurdle as a **point test** — the
    /// new-money admission read (entry decision; exit-side dispersion tolerance never
    /// licenses new capital).
    pub admits_new_money: bool,
}

/// How the v2 scenario targets were derived — recorded on the audit so a target's
/// basis (and any fallback) is inspectable, and versioned so calibration never mixes
/// target bases.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TargetMeta {
    /// The ladder rung: "consensus forward EPS", "consensus forward revenue per
    /// share", or "fund exposure composite".
    pub driver_rung: String,
    /// True when the multiples were spread-anchored on the DGS10 history; false on
    /// the raw-percentile fallback (window below the observation floor).
    pub rate_anchored: bool,
    pub anchor_observations: usize,
    /// True when the driver was held flat across scenarios (no published consensus
    /// spread, or the fund form's construction).
    pub flat_driver: bool,
    /// Scenarios that individually fell back to their raw multiple percentile on the
    /// degenerate-denominator guard.
    pub degenerate_scenarios: usize,
    /// True when the finished prices needed the defensive monotonicity repair.
    pub monotonicity_repaired: bool,
    /// True when no anchor observation existed at all and the current multiple was
    /// carried (recorded, never silent).
    pub current_multiple_carry: bool,
    pub parameter_version: String,
}

/// The engine's analyzed output for a holding that cleared the evidence floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineOutput {
    pub sub_scores: SubScores,
    pub grade: Grade,
    pub metrics: ComputedMetrics,
    pub price_targets: PriceTargets,
    /// The deterministic per-branch risk tier (`docs/portfolio-analysis.md` §Starting
    /// parameters), with any tier-input gaps logged beside it.
    pub risk_tier: crate::portfolio::RiskTier,
    pub tier_gaps: Vec<String>,
    /// The capital-efficiency / dead-money read over the scenario total returns.
    pub hurdle: HurdleRead,
    /// How the scenario targets were derived (rung, fallbacks, version).
    pub target_meta: TargetMeta,
    /// True when the letter rests on an imputed (neutral-50) sub-score — surfaced as
    /// the visible low-confidence marker beside the letter.
    pub low_confidence_grade: bool,
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
pub fn analyze(fin: &CompanyFinancials, rates: &RateAnchors) -> EngineVerdict {
    let metrics = compute_metrics(fin);

    let quality = quality_score(&metrics);
    let valuation = valuation_score(&metrics);
    let momentum = momentum_score(&metrics);
    let risk = risk_score(&metrics);

    // The evidence floor counts **letter** sub-scores only — momentum is computed
    // alongside but lives outside the letter (`docs/portfolio-analysis.md` §Starting
    // parameters, the settled ex-momentum re-weight).
    let computed = [quality, valuation, risk]
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
            "only {computed} of 3 letter sub-scores computable (need {MIN_SUBSCORES_FOR_GRADE}); \
             financial inputs too sparse to grade"
        ));
    }

    // The v2 rate-anchored scenario targets. No admissible driver on any ladder rung
    // is the named evidence-floor reason (`docs/portfolio-analysis.md` §Evidence floor).
    let bundle = match scenario_targets_v2(price, fin, rates, &metrics) {
        TargetOutcome::Computed(b) => b,
        TargetOutcome::NoAdmissibleDriver => {
            return EngineVerdict::InsufficientEvidence(
                "no-admissible-driver: no positive forward-EPS consensus and no computable \
                 forward revenue per share on any ladder rung"
                    .to_string(),
            );
        }
    };

    // A missing sub-score takes the neutral midpoint (50) so the composite stays
    // defined; dividing by the full fixed weight sum keeps it on the same 0–100 scale
    // (an impute-to-neutral, not a renormalization over the present sub-scores). The
    // count gate above guarantees at least two are real, so this never grades on all
    // defaults — and a letter resting on any imputed axis carries the visible
    // low-confidence marker.
    let low_confidence_grade = quality.is_none() || valuation.is_none() || risk.is_none();
    let sub_scores = SubScores {
        quality: quality.unwrap_or(50.0),
        valuation: valuation.unwrap_or(50.0),
        momentum: momentum.unwrap_or(50.0),
        risk: risk.unwrap_or(50.0),
    };
    let grade = grade_from_subscores(&sub_scores);

    // Deterministic per-branch tier assignment, then the tier-scaled hurdle over the
    // scenario total returns — assigned before anything downstream consumes it
    // (`docs/portfolio-workflow.md` §Step 6b).
    let (risk_tier, tier_gaps) = assign_stock_tier(fin, &metrics);
    let hurdle = hurdle_read(&bundle.scenario, rates.dgs2, risk_tier);

    EngineVerdict::Analyzed(Box::new(EngineOutput {
        sub_scores,
        grade,
        metrics,
        price_targets: bundle.targets,
        risk_tier,
        tier_gaps,
        hurdle,
        target_meta: bundle.meta,
        low_confidence_grade,
    }))
}

/// Roll the **letter** sub-scores (quality / valuation / risk) up to a letter grade
/// through the fixed ex-momentum weights — momentum is context, never a letter input
/// (`docs/portfolio-analysis.md` §Starting parameters). Public so a reviewer (and the
/// live smoke) can assert the roll-up directly.
pub fn grade_from_subscores(s: &SubScores) -> Grade {
    let composite = (s.quality * W_QUALITY + s.valuation * W_VALUATION + s.risk * W_RISK)
        / (W_QUALITY + W_VALUATION + W_RISK);
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
        // Fund context fields — set by the fund path only.
        expense_ratio: None,
        nav_premium: None,
        composite_coverage: None,
    }
}

/// Linearly map `value` from `[lo, hi]` onto a 0–100 score, clamped at the ends.
/// `lo` maps to 0 and `hi` to 100 (pass `lo > hi` to invert — lower input scores
/// higher).
pub(crate) fn scale(value: f64, lo: f64, hi: f64) -> f64 {
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
pub(crate) fn return_volatility(history: &[f64]) -> Option<f64> {
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

// ---- v2 rate-anchored scenario-target function ---------------------------------
//
// `docs/portfolio-analysis.md` §Starting parameters — the settled shape: per-share
// driver × scenario multiple, the multiple re-anchoring the driver multiple's own
// history on the run-level DGS10 through the spread percentiles (inverse mapping),
// with the documented guards and fallbacks. The fund form (the settled fund-form
// bullet) shares the core through `spread_anchored_scenarios`.

/// One admissible anchor-window observation: the driver-yield spread over the
/// contemporaneous DGS10, and the raw multiple for the fallback paths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorObservation {
    pub spread: f64,
    pub raw_multiple: f64,
}

/// The scenario set the core computes: three prices, their twelve-month total
/// returns, and the fallback record.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioSet {
    pub bear: f64,
    pub base: f64,
    pub bull: f64,
    pub tr_bear: f64,
    pub tr_base: f64,
    pub tr_bull: f64,
    pub rate_anchored: bool,
    pub anchor_observations: usize,
    pub degenerate_scenarios: usize,
    pub monotonicity_repaired: bool,
    pub current_multiple_carry: bool,
}

/// What the v2 wrapper resolved to: a computed bundle, or the named
/// `no-admissible-driver` evidence-floor reason.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetOutcome {
    // Boxed: the bundle dwarfs the unit variant.
    Computed(Box<TargetBundle>),
    NoAdmissibleDriver,
}

/// The v2 function's full output: the persisted targets plus the scenario set (the
/// hurdle's input) and the derivation record.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetBundle {
    pub targets: PriceTargets,
    pub scenario: ScenarioSet,
    pub meta: TargetMeta,
}

/// The latest value in a dated, oldest-first series on or before `date` (ISO dates
/// compare lexicographically). `None` when the series is empty or starts after.
pub fn latest_on_or_before(series: &[DatedValue], date: &str) -> Option<f64> {
    let idx = series.partition_point(|d| d.date.as_str() <= date);
    idx.checked_sub(1).map(|i| series[i].value)
}

/// Linear-interpolated percentile (`p` in 0..=1) over an unsorted sample.
fn percentile(values: &[f64], p: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("finite percentile inputs"));
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = p * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// The shared v2 core (`docs/portfolio-analysis.md` §Starting parameters): scenario
/// prices from per-scenario drivers × spread-anchored multiples. With enough anchor
/// observations the multiples come from the spread percentiles under the **inverse**
/// mapping (`spread_bear = P75 … spread_bull = P25`, a wider spread being a cheaper
/// multiple), each scenario guarded against a degenerate reciprocal denominator;
/// below the observation floor the rate correction drops entirely and the mapping
/// flips back to **direct** raw-multiple percentiles (the cheap multiple is the bear
/// multiple in both domains); with no observations at all the current multiple is
/// carried (recorded — scenario spread then comes from driver dispersion alone).
/// Scenario identity comes from the mapping, never from sorting the finished prices —
/// the monotonicity sort is a recorded defensive repair only.
pub fn spread_anchored_scenarios(
    spot: f64,
    drivers: [f64; 3],
    observations: &[AnchorObservation],
    dgs10_now: f64,
    forward_income_per_share: f64,
) -> ScenarioSet {
    let n = observations.len();
    let mut degenerate = 0usize;
    let mut current_multiple_carry = false;

    let multiples: [f64; 3] = if n >= MIN_ANCHOR_OBSERVATIONS {
        let spreads: Vec<f64> = observations.iter().map(|o| o.spread).collect();
        let raws: Vec<f64> = observations.iter().map(|o| o.raw_multiple).collect();
        // Inverse mapping in the spread domain; the raw fallback maps direct.
        let spread_ps = [
            percentile(&spreads, 0.75), // bear
            percentile(&spreads, 0.50), // base
            percentile(&spreads, 0.25), // bull
        ];
        let raw_ps = [
            percentile(&raws, 0.25), // bear
            percentile(&raws, 0.50), // base
            percentile(&raws, 0.75), // bull
        ];
        let mut ms = [0.0; 3];
        for s in 0..3 {
            let denom = spread_ps[s] + dgs10_now;
            if denom < DEGENERATE_DENOMINATOR_EPS {
                degenerate += 1;
                ms[s] = raw_ps[s];
            } else {
                ms[s] = 1.0 / denom;
            }
        }
        ms
    } else if n >= 1 {
        let raws: Vec<f64> = observations.iter().map(|o| o.raw_multiple).collect();
        [
            percentile(&raws, 0.25),
            percentile(&raws, 0.50),
            percentile(&raws, 0.75),
        ]
    } else {
        // No anchor history at all: carry the spot's own multiple on the base driver,
        // so scenario spread comes from driver dispersion alone — recorded.
        current_multiple_carry = true;
        let carry = spot / drivers[1];
        [carry, carry, carry]
    };

    let mut prices = [
        drivers[0] * multiples[0],
        drivers[1] * multiples[1],
        drivers[2] * multiples[2],
    ];
    // Defensive repair only — a residual crossing remains possible through the
    // fallback seams (one scenario rate-anchored, another raw).
    let monotonicity_repaired = !(prices[0] <= prices[1] && prices[1] <= prices[2]);
    if monotonicity_repaired {
        prices.sort_by(|a, b| a.partial_cmp(b).expect("finite scenario prices"));
    }

    let tr = |p: f64| (p + forward_income_per_share) / spot - 1.0;
    ScenarioSet {
        bear: prices[0],
        base: prices[1],
        bull: prices[2],
        tr_bear: tr(prices[0]),
        tr_base: tr(prices[1]),
        tr_bull: tr(prices[2]),
        rate_anchored: n >= MIN_ANCHOR_OBSERVATIONS,
        anchor_observations: n,
        degenerate_scenarios: degenerate,
        monotonicity_repaired,
        current_multiple_carry,
    }
}

/// The trailing-twelve-month driver print anchored at each of the newest
/// [`ANCHOR_WINDOW_QUARTERS`] quarters, joined to the dated closes and DGS10 history
/// (`docs/portfolio-analysis.md` §Starting parameters — the dated anchor join): each
/// admissible quarter anchors on its filing date (period end + the filing grace when
/// absent), reads the latest close on or before that date, and the latest published
/// DGS10 on or before the same date. A quarter whose trailing print is not finite
/// and positive is excluded (an economically invalid multiple observation).
fn stock_anchor_observations(
    fin: &CompanyFinancials,
    rates: &RateAnchors,
    use_eps: bool,
) -> Vec<AnchorObservation> {
    use chrono::NaiveDate;
    let q = &fin.quarterly_income;
    let mut out = Vec::new();
    for i in 0..ANCHOR_WINDOW_QUARTERS.min(q.len()) {
        // TTM print: this quarter plus the three before it (rows are newest-first).
        if i + 4 > q.len() {
            break;
        }
        let window = &q[i..i + 4];
        let ttm: Option<f64> = if use_eps {
            window.iter().map(|r| r.eps_diluted).sum()
        } else {
            let revenue: Option<f64> = window.iter().map(|r| r.revenue).sum();
            let shares = window[0]
                .diluted_shares
                .or_else(|| q.first().and_then(|r| r.diluted_shares))
                .or(fin.shares_outstanding);
            match (revenue, shares) {
                (Some(rev), Some(sh)) if sh > 0.0 => Some(rev / sh),
                _ => None,
            }
        };
        let Some(ttm) = ttm else { continue };
        if !ttm.is_finite() || ttm <= 0.0 {
            continue;
        }
        let anchor_date = match &window[0].filing_date {
            Some(d) => d.clone(),
            None => match NaiveDate::parse_from_str(&window[0].period_end, "%Y-%m-%d") {
                Ok(d) => (d + chrono::Duration::days(FILING_GRACE_DAYS))
                    .format("%Y-%m-%d")
                    .to_string(),
                Err(_) => continue,
            },
        };
        let Some(close) = latest_on_or_before(&fin.daily_closes, &anchor_date) else {
            continue;
        };
        if close <= 0.0 {
            continue;
        }
        let Some(dgs10_t) = latest_on_or_before(&rates.dgs10_history, &anchor_date) else {
            continue;
        };
        let yield_t = ttm / close;
        out.push(AnchorObservation {
            spread: yield_t - dgs10_t,
            raw_multiple: close / ttm,
        });
    }
    out
}

/// The v2 driver ladder (`docs/portfolio-analysis.md` §Starting parameters): pick the
/// per-share fundamental deterministically — consensus forward EPS where a positive
/// consensus exists, else consensus forward revenue per share on the latest reported
/// diluted share count — with each scenario driver's implied growth clamped by the v1
/// sanity bound against the trailing print, and a missing published spread holding
/// the driver flat (recorded). Returns `(drivers[bear,base,bull], rung label,
/// use_eps, flat_driver)`, or `None` when no rung is admissible.
fn driver_ladder(fin: &CompanyFinancials) -> Option<([f64; 3], &'static str, bool, bool)> {
    let c = fin.consensus.as_ref();

    // Trailing TTM prints for the growth clamp (newest four quarters).
    let ttm_eps: Option<f64> = (fin.quarterly_income.len() >= 4)
        .then(|| fin.quarterly_income[..4].iter().map(|r| r.eps_diluted).sum())
        .flatten();
    let latest_shares = fin
        .quarterly_income
        .first()
        .and_then(|r| r.diluted_shares)
        .or(fin.shares_outstanding);
    let ttm_rev_ps: Option<f64> = match (
        (fin.quarterly_income.len() >= 4)
            .then(|| fin.quarterly_income[..4].iter().map(|r| r.revenue).sum::<Option<f64>>())
            .flatten(),
        latest_shares,
    ) {
        (Some(rev), Some(sh)) if sh > 0.0 => Some(rev / sh),
        _ => None,
    };

    // Clamp a scenario driver's implied growth vs the trailing print (only where the
    // trailing print is positive, so growth is definable); a non-positive scenario
    // driver falls back to the base value rather than pricing a negative driver.
    let clamp = |driver: f64, trailing: Option<f64>, base: f64| -> f64 {
        let d = match trailing {
            Some(t) if t > 0.0 => {
                driver.clamp(t * (1.0 + DRIVER_GROWTH_MIN), t * (1.0 + DRIVER_GROWTH_MAX))
            }
            _ => driver,
        };
        if d.is_finite() && d > 0.0 {
            d
        } else {
            base
        }
    };

    // Rung 1: forward EPS, eligible only on a finite positive consensus mid.
    if let Some(mid) = c.and_then(|c| c.eps_mid).filter(|m| m.is_finite() && *m > 0.0) {
        let base = clamp(mid, ttm_eps, mid);
        let flat = c.map(|c| c.eps_low.is_none() || c.eps_high.is_none()).unwrap_or(true);
        let low = c.and_then(|c| c.eps_low).unwrap_or(mid);
        let high = c.and_then(|c| c.eps_high).unwrap_or(mid);
        let drivers = [
            clamp(low, ttm_eps, base),
            base,
            clamp(high, ttm_eps, base),
        ];
        return Some((drivers, "consensus forward EPS", true, flat));
    }

    // Rung 2: forward revenue per share on the latest reported diluted count.
    if let (Some(mid), Some(sh)) = (
        c.and_then(|c| c.revenue_mid).filter(|m| m.is_finite() && *m > 0.0),
        latest_shares.filter(|s| *s > 0.0),
    ) {
        let base = clamp(mid / sh, ttm_rev_ps, mid / sh);
        let flat = c
            .map(|c| c.revenue_low.is_none() || c.revenue_high.is_none())
            .unwrap_or(true);
        let low = c.and_then(|c| c.revenue_low).map(|v| v / sh).unwrap_or(mid / sh);
        let high = c.and_then(|c| c.revenue_high).map(|v| v / sh).unwrap_or(mid / sh);
        let drivers = [
            clamp(low, ttm_rev_ps, base),
            base,
            clamp(high, ttm_rev_ps, base),
        ];
        return Some((drivers, "consensus forward revenue per share", false, flat));
    }

    None
}

/// The v2 rate-anchored scenario-target function for a **priced stock**
/// (`docs/portfolio-analysis.md` §Starting parameters): driver ladder → dated anchor
/// join → spread-percentile multiples (inverse map, guarded) → scenario prices and
/// total returns. The twelve-month target's bear/bull **are** the scenario prices;
/// the one-month leg keeps the v1 mechanics (base = spot × (1 + PR_base ⁄ 12), the
/// price-return leg, dividends excluded; volatility-scaled bands with the fixed
/// fallbacks).
pub fn scenario_targets_v2(
    spot: f64,
    fin: &CompanyFinancials,
    rates: &RateAnchors,
    m: &ComputedMetrics,
) -> TargetOutcome {
    let Some((drivers, rung, use_eps, flat_driver)) = driver_ladder(fin) else {
        return TargetOutcome::NoAdmissibleDriver;
    };

    let observations = stock_anchor_observations(fin, rates, use_eps);
    let forward_dividends = fin.ttm_dividends_per_share.unwrap_or(0.0);
    let scenario =
        spread_anchored_scenarios(spot, drivers, &observations, rates.dgs10, forward_dividends);

    let targets = build_price_targets(spot, &scenario, m, rung, flat_driver);
    let meta = TargetMeta {
        driver_rung: rung.to_string(),
        rate_anchored: scenario.rate_anchored,
        anchor_observations: scenario.anchor_observations,
        flat_driver,
        degenerate_scenarios: scenario.degenerate_scenarios,
        monotonicity_repaired: scenario.monotonicity_repaired,
        current_multiple_carry: scenario.current_multiple_carry,
        parameter_version: SCENARIO_TARGET_PARAMETER_VERSION.to_string(),
    };
    TargetOutcome::Computed(Box::new(TargetBundle {
        targets,
        scenario,
        meta,
    }))
}

/// Render a [`ScenarioSet`] into the persisted [`PriceTargets`]: the twelve-month
/// target carries the scenario prices; the one-month leg keeps the v1 mechanics.
/// Shared by the stock and fund forms.
pub fn build_price_targets(
    spot: f64,
    scenario: &ScenarioSet,
    m: &ComputedMetrics,
    rung: &str,
    flat_driver: bool,
) -> PriceTargets {
    let pr_base = scenario.base / spot - 1.0;
    let om_base = spot * (1.0 + pr_base / 12.0);
    let om_band = m
        .return_volatility
        .map(|v| (v * 2.0).clamp(0.02, 0.15))
        .unwrap_or(ONE_MONTH_FALLBACK_BAND);

    let anchor_note = if scenario.current_multiple_carry {
        "no anchor history — current multiple carried".to_string()
    } else if scenario.rate_anchored {
        format!(
            "DGS10 spread-anchored P75/P50/P25 multiples over {} quarterly anchors (inverse map{}{})",
            scenario.anchor_observations,
            if scenario.degenerate_scenarios > 0 {
                "; degenerate-denominator raw fallback on some scenario(s)"
            } else {
                ""
            },
            if scenario.monotonicity_repaired {
                "; monotonicity repaired"
            } else {
                ""
            },
        )
    } else {
        format!(
            "raw multiple percentiles P25/P50/P75 (direct map; window below the \
             {MIN_ANCHOR_OBSERVATIONS}-observation floor at {} anchors)",
            scenario.anchor_observations
        )
    };
    let driver_note = if flat_driver {
        format!("{rung}, held flat across scenarios")
    } else {
        format!("{rung} low/mid/high")
    };

    PriceTargets {
        one_month: Some(PriceTarget {
            base: om_base,
            bear: om_base * (1.0 - om_band),
            bull: om_base * (1.0 + om_band),
            methodology: format!(
                "One-month (rolling) base = spot × (1 + PR_base/12), the twelve-month \
                 price-return leg prorated (v1 mechanics, dividends excluded); bull/bear \
                 ± {:.1}% from realized volatility [{}]",
                om_band * 100.0,
                SCENARIO_TARGET_PARAMETER_VERSION
            ),
        }),
        twelve_month: Some(PriceTarget {
            base: scenario.base,
            bear: scenario.bear,
            bull: scenario.bull,
            methodology: format!(
                "Twelve-month (rolling) scenarios = {driver_note} × {anchor_note} [{}]",
                SCENARIO_TARGET_PARAMETER_VERSION
            ),
        }),
    }
}

// ---- Risk tier and the capital-efficiency hurdle --------------------------------

/// Deterministic stock risk-tier assignment — Trade Opportunities' canonical
/// High / Low / else-Medium rule (`docs/trade-opportunities.md` §Starting parameters)
/// under Portfolio's **stated missing-input rule**: a leg whose input this job's
/// surface doesn't carry (event exposure; liquidity is enriching here) simply cannot
/// trigger, and a holding whose tier inputs are wholesale missing reads **Medium with
/// a logged tier-input gap** — the neutral-imputation stance, never a fabricated
/// High or Low.
pub fn assign_stock_tier(
    fin: &CompanyFinancials,
    m: &ComputedMetrics,
) -> (crate::portfolio::RiskTier, Vec<String>) {
    use crate::portfolio::RiskTier;
    let annual_vol = m.return_volatility.map(|v| v * ANNUALIZATION_FACTOR);
    let drawdown = max_drawdown(&fin.daily_closes, &fin.price_history);
    let profitable = fin.net_income.or(fin.operating_income).map(|v| v > 0.0);

    let inputs_present = [
        fin.market_cap.is_some(),
        annual_vol.is_some(),
        m.debt_to_equity.is_some(),
        profitable.is_some(),
        drawdown.is_some(),
    ];
    if !inputs_present.iter().any(|p| *p) {
        return (
            RiskTier::Medium,
            vec!["risk tier: every tier input missing — Medium imputed (logged gap)".to_string()],
        );
    }

    let high = fin.market_cap.map(|c| c < TIER_HIGH_MAX_MCAP).unwrap_or(false)
        || annual_vol.map(|v| v > TIER_HIGH_MIN_ANNUAL_VOL).unwrap_or(false)
        || m.debt_to_equity.map(|d| d > TIER_HIGH_MIN_DEBT_EQUITY).unwrap_or(false)
        || profitable.map(|p| !p).unwrap_or(false)
        || drawdown.map(|d| d > TIER_HIGH_MIN_DRAWDOWN).unwrap_or(false);
    if high {
        return (crate::portfolio::RiskTier::High, vec![]);
    }

    // The Low conjunction requires each surface-carried leg present *and* passing;
    // the liquidity leg is absent from this job's surface, so it neither blocks nor
    // triggers (the missing-input rule).
    let low = fin.market_cap.map(|c| c > TIER_LOW_MIN_MCAP).unwrap_or(false)
        && profitable.unwrap_or(false)
        && m.debt_to_equity.map(|d| d < TIER_LOW_MAX_DEBT_EQUITY).unwrap_or(false)
        && annual_vol.map(|v| v < TIER_LOW_MAX_ANNUAL_VOL).unwrap_or(false);
    if low {
        (RiskTier::Low, vec![])
    } else {
        (RiskTier::Medium, vec![])
    }
}

/// Deterministic **priced-equity-fund** tier mapping (`docs/portfolio-analysis.md`
/// §Starting parameters, drafted): High on a **leveraged / inverse** structural flag,
/// annualized volatility > 40%, or maximum drawdown > 50%; Low on volatility < 25%
/// with **no structural flag of either kind** — an option-overlay flag bars Low
/// without forcing High (the doc keys the High leg to leveraged / inverse
/// specifically, while Low requires no structural flag at all); else Medium.
pub fn assign_fund_tier(
    leveraged_inverse: bool,
    structural_flag: bool,
    annual_vol: Option<f64>,
    drawdown: Option<f64>,
) -> crate::portfolio::RiskTier {
    use crate::portfolio::RiskTier;
    if leveraged_inverse
        || annual_vol.map(|v| v > TIER_HIGH_MIN_ANNUAL_VOL).unwrap_or(false)
        || drawdown.map(|d| d > TIER_HIGH_MIN_DRAWDOWN).unwrap_or(false)
    {
        RiskTier::High
    } else if !structural_flag
        && annual_vol.map(|v| v < TIER_LOW_MAX_ANNUAL_VOL).unwrap_or(false)
    {
        RiskTier::Low
    } else {
        RiskTier::Medium
    }
}

/// Maximum peak-to-trough drawdown over the available history (dated closes when
/// present, else the undated history), as a positive fraction. `None` on too little
/// history.
pub fn max_drawdown(dated: &[DatedValue], undated: &[f64]) -> Option<f64> {
    let closes: Vec<f64> = if !dated.is_empty() {
        dated.iter().map(|d| d.value).collect()
    } else {
        undated.to_vec()
    };
    if closes.len() < 2 {
        return None;
    }
    let mut peak = f64::MIN;
    let mut worst = 0.0_f64;
    for c in closes {
        if c > peak {
            peak = c;
        }
        if peak > 0.0 {
            worst = worst.max(1.0 - c / peak);
        }
    }
    Some(worst)
}

/// The tier-scaled hurdle premium (decimal ratio).
pub fn tier_premium(tier: crate::portfolio::RiskTier) -> f64 {
    match tier {
        crate::portfolio::RiskTier::Low => TIER_PREMIUM_LOW,
        crate::portfolio::RiskTier::Medium => TIER_PREMIUM_MEDIUM,
        crate::portfolio::RiskTier::High => TIER_PREMIUM_HIGH,
    }
}

/// The three-state capital-efficiency / dead-money read over the scenario total
/// returns (`docs/portfolio-analysis.md` §Starting parameters): **clears** when even
/// the bear case clears the hurdle, **fails** when even the bull case misses it (dead
/// money), **indeterminate** otherwise; the base-case **point test** is the separate
/// new-money admission read (entry decision — dispersion tolerance is exit-side
/// hysteresis, never a license for new capital).
pub fn hurdle_read(
    scenario: &ScenarioSet,
    dgs2: f64,
    tier: crate::portfolio::RiskTier,
) -> HurdleRead {
    use crate::portfolio::HurdleState;
    let hurdle = dgs2 + tier_premium(tier);
    let state = if scenario.tr_bear >= hurdle {
        HurdleState::Clears
    } else if scenario.tr_bull < hurdle {
        HurdleState::Fails
    } else {
        HurdleState::Indeterminate
    };
    HurdleRead {
        state,
        hurdle_rate: Some(hurdle),
        tr_bear: Some(scenario.tr_bear),
        tr_base: Some(scenario.tr_base),
        tr_bull: Some(scenario.tr_bull),
        admits_new_money: scenario.tr_base >= hurdle,
    }
}

/// Bound the feasible action set from engine-known inputs only
/// (`docs/portfolio-analysis.md` §Starting parameters — the feasible-set rule;
/// conviction is model-authored, so it can't pre-gate). The add family is offered
/// only when the new-money admission point test passes, the hurdle isn't `fails`
/// (dead money drops the family a fortiori at any grade), the grade isn't F, and the
/// position sits under the concentration cap; *add aggressively* additionally needs
/// an A/B grade with headroom. Every grade test reads the momentum-free letter.
pub fn feasible_actions(
    grade: Grade,
    hurdle: &HurdleRead,
    current_weight: f64,
) -> Vec<Action> {
    use crate::portfolio::HurdleState;
    let mut set = vec![Action::SellAll, Action::Trim, Action::Hold];
    let dead_money = hurdle.state == HurdleState::Fails;
    let add_ok = hurdle.admits_new_money
        && !dead_money
        && grade != Grade::F
        && current_weight < MAX_SINGLE_WEIGHT;
    if add_ok {
        set.push(Action::Add);
        if matches!(grade, Grade::A | Grade::B) {
            set.push(Action::AddAggressively);
        }
    }
    set
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
    // ladder; concentration is bounded by the cap below). The add-family rungs carry
    // **absolute target floors** — the multiplier band or the floor, whichever is
    // higher (`docs/portfolio-analysis.md` §Starting parameters: pure current-weight
    // multipliers perpetuate historical accidents) — while trim / hold stay relative.
    let (low_mult, high_mult, floor) = match action {
        Action::SellAll => (0.0, 0.0, 0.0),
        Action::Trim => (0.4, 0.7, 0.0),
        Action::Hold => (0.9, 1.1, 0.0),
        Action::Add => (1.2, 1.6, ADD_FLOOR_WEIGHT),
        Action::AddAggressively => (1.6, 2.2, ADD_AGGRESSIVE_FLOOR_WEIGHT),
    };
    let target_low = (current_weight * low_mult).max(floor).min(MAX_SINGLE_WEIGHT);
    let target_high = (current_weight * high_mult).max(floor).min(MAX_SINGLE_WEIGHT);
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
    use crate::portfolio::{AssetClass, HurdleState, RiskTier};
    use crate::schwab::{OptionQuote, OptionKind};

    /// Quarterly dates walking back from mid-2026, newest first.
    fn quarter_ends(n: usize) -> Vec<String> {
        let anchors = [
            "2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30",
            "2025-06-30", "2025-03-31", "2024-12-31", "2024-09-30",
            "2024-06-30", "2024-03-31", "2023-12-31", "2023-09-30",
            "2023-06-30", "2023-03-31", "2022-12-31", "2022-09-30",
        ];
        anchors.iter().take(n).map(|s| s.to_string()).collect()
    }

    /// The run-level rate fixture: DGS2 4%, DGS10 4.5%, and a dated DGS10 history
    /// covering the anchor window (all decimal ratios).
    pub(crate) fn rates() -> RateAnchors {
        let history = quarter_ends(16)
            .into_iter()
            .rev()
            .map(|date| DatedValue { date, value: 0.04 })
            .collect();
        RateAnchors {
            dgs2: 0.04,
            dgs10: 0.045,
            dgs10_history: history,
            history_gap: None,
        }
    }

    /// A healthy large-cap with a full v2 surface: 16 quarterly prints, dated closes,
    /// and a forward consensus — the driver ladder's rung 1.
    pub(crate) fn strong() -> CompanyFinancials {
        let ends = quarter_ends(16);
        let quarterly_income = ends
            .iter()
            .enumerate()
            .map(|(i, end)| QuarterlyIncomeRow {
                period_end: end.clone(),
                filing_date: None, // period end + the 45-day grace anchors the join
                revenue: Some(100.0 - i as f64),
                eps_diluted: Some(1.55 - 0.01 * i as f64),
                diluted_shares: Some(1.5e10),
            })
            .collect();
        // Dated closes: one per quarter end plus a recent print, rising over time.
        let mut daily_closes: Vec<DatedValue> = ends
            .iter()
            .rev()
            .enumerate()
            .map(|(i, end)| DatedValue {
                date: end.clone(),
                value: 130.0 + 4.0 * i as f64,
            })
            .collect();
        daily_closes.push(DatedValue { date: "2026-07-15".into(), value: 195.0 });
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
            daily_closes,
            quarterly_income,
            consensus: Some(ConsensusEstimate {
                period_end: "2027-06-30".into(),
                eps_low: Some(6.0),
                eps_mid: Some(6.5),
                eps_high: Some(7.0),
                revenue_low: Some(420.0e9),
                revenue_mid: Some(430.0e9),
                revenue_high: Some(440.0e9),
            }),
            ttm_dividends_per_share: Some(1.0),
            gaps: vec![],
        }
    }

    #[test]
    fn strong_company_grades_and_computes_v2_targets() {
        match analyze(&strong(), &rates()) {
            EngineVerdict::Analyzed(out) => {
                for s in [
                    out.sub_scores.quality,
                    out.sub_scores.valuation,
                    out.sub_scores.momentum,
                    out.sub_scores.risk,
                ] {
                    assert!((0.0..=100.0).contains(&s), "{s}");
                }
                assert!(matches!(out.grade, Grade::A | Grade::B | Grade::C), "{:?}", out.grade);
                // The twelve-month target is the v2 scenario set: rate-anchored over
                // the full window, ordered, methodology versioned.
                let tm = out.price_targets.twelve_month.as_ref().unwrap();
                assert!(tm.bear <= tm.base && tm.base <= tm.bull, "ordered scenarios");
                assert!(tm.methodology.contains("spread-anchored"), "{}", tm.methodology);
                assert!(tm.methodology.contains(SCENARIO_TARGET_PARAMETER_VERSION));
                assert!(out.target_meta.rate_anchored);
                assert_eq!(out.target_meta.anchor_observations, ANCHOR_WINDOW_QUARTERS);
                assert_eq!(out.target_meta.driver_rung, "consensus forward EPS");
                assert!(!out.target_meta.flat_driver, "published low/high spread");
                // The one-month leg keeps the v1 mechanics off the price-return leg.
                let om = out.price_targets.one_month.as_ref().unwrap();
                let pr_base = tm.base / 195.0 - 1.0;
                assert!((om.base - 195.0 * (1.0 + pr_base / 12.0)).abs() < 1e-9);
                // The hurdle read is computed off the scenario TRs with the tier premium.
                assert_ne!(out.hurdle.state, crate::portfolio::HurdleState::Unscorable);
                assert!(out.hurdle.hurdle_rate.unwrap() > 0.04);
                // Grade rests on three real letter sub-scores — no low-confidence marker.
                assert!(!out.low_confidence_grade);
            }
            other => panic!("expected an analysis, got {other:?}"),
        }
    }

    #[test]
    fn grade_is_deterministic_for_the_same_inputs() {
        let a = analyze(&strong(), &rates());
        let b = analyze(&strong(), &rates());
        assert_eq!(a, b, "same financials always grade identically");
    }

    #[test]
    fn missing_price_abstains_below_the_evidence_floor() {
        let mut fin = strong();
        fin.current_price = None;
        match analyze(&fin, &rates()) {
            EngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("no current price"), "{reason}");
            }
            other => panic!("expected abstention, got {other:?}"),
        }
    }

    #[test]
    fn too_few_subscores_abstains() {
        // Only a price and a single multiple — one letter sub-score (valuation) at most.
        let fin = CompanyFinancials {
            symbol: "X".into(),
            current_price: Some(50.0),
            ps_ratio: Some(3.0),
            ..CompanyFinancials::default()
        };
        match analyze(&fin, &rates()) {
            EngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("sub-scores"), "{reason}");
            }
            other => panic!("expected abstention, got {other:?}"),
        }
    }

    #[test]
    fn no_admissible_driver_is_the_named_floor_reason() {
        // A gradeable surface with no consensus at all: neither ladder rung is
        // admissible, so the holding abstains under the named reason rather than
        // pricing off nothing (`docs/portfolio-analysis.md` §Evidence floor).
        let mut fin = strong();
        fin.consensus = None;
        match analyze(&fin, &rates()) {
            EngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("no-admissible-driver"), "{reason}");
            }
            other => panic!("expected abstention, got {other:?}"),
        }
    }

    #[test]
    fn negative_eps_consensus_skips_to_the_revenue_rung() {
        let mut fin = strong();
        let c = fin.consensus.as_mut().unwrap();
        c.eps_mid = Some(-0.50); // pre-profit: reciprocal-yield math is meaningless
        c.eps_low = None;
        c.eps_high = None;
        match analyze(&fin, &rates()) {
            EngineVerdict::Analyzed(out) => {
                assert_eq!(
                    out.target_meta.driver_rung,
                    "consensus forward revenue per share"
                );
            }
            other => panic!("expected the revenue rung, got {other:?}"),
        }
    }

    #[test]
    fn missing_consensus_spread_holds_the_driver_flat() {
        let mut fin = strong();
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = None;
        c.eps_high = None;
        match analyze(&fin, &rates()) {
            EngineVerdict::Analyzed(out) => {
                assert!(out.target_meta.flat_driver);
                let tm = out.price_targets.twelve_month.unwrap();
                assert!(tm.methodology.contains("held flat"), "{}", tm.methodology);
                // Scenario spread then comes from the multiple axis alone.
                assert!(tm.bear <= tm.base && tm.base <= tm.bull);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn driver_growth_is_clamped_by_the_sanity_bound() {
        // Trailing TTM EPS ≈ 6.14 (1.55+1.54+1.53+1.52); a wild 20.0 consensus mid
        // clamps to ttm × 1.35, and a collapsed 1.0 clamps to ttm × 0.75.
        let ttm: f64 = 1.55 + 1.54 + 1.53 + 1.52;
        let mut fin = strong();
        fin.consensus.as_mut().unwrap().eps_mid = Some(20.0);
        let (drivers, ..) = driver_ladder(&fin).unwrap();
        assert!((drivers[1] - ttm * (1.0 + DRIVER_GROWTH_MAX)).abs() < 1e-9);
        fin.consensus.as_mut().unwrap().eps_mid = Some(1.0);
        let (drivers, ..) = driver_ladder(&fin).unwrap();
        assert!((drivers[1] - ttm * (1.0 + DRIVER_GROWTH_MIN)).abs() < 1e-9);
    }

    #[test]
    fn inverse_spread_mapping_orders_the_multiples() {
        // Nine spread observations from wide (cheap) to narrow (rich): the inverse
        // mapping must give M_bear ≤ M_base ≤ M_bull without sorting prices.
        let observations: Vec<AnchorObservation> = (0..9)
            .map(|i| {
                let spread = 0.06 - 0.005 * i as f64; // 6% down to 2%
                AnchorObservation { spread, raw_multiple: 1.0 / (spread + 0.045) }
            })
            .collect();
        let s = spread_anchored_scenarios(100.0, [5.0, 5.0, 5.0], &observations, 0.045, 0.0);
        assert!(s.rate_anchored);
        assert_eq!(s.degenerate_scenarios, 0);
        assert!(!s.monotonicity_repaired, "inverse map orders without repair");
        assert!(s.bear < s.base && s.base < s.bull);
    }

    #[test]
    fn degenerate_denominator_falls_back_per_scenario_and_is_recorded() {
        // Spreads near −DGS10: the reciprocal denominator collapses below ε, so the
        // guarded scenarios take their raw multiple percentiles instead.
        let observations: Vec<AnchorObservation> = (0..9)
            .map(|i| AnchorObservation {
                spread: -0.041 - 0.0005 * i as f64, // denom = spread + 0.045 < 0.01
                raw_multiple: 20.0 + i as f64,
            })
            .collect();
        let s = spread_anchored_scenarios(100.0, [5.0, 5.0, 5.0], &observations, 0.045, 0.0);
        assert!(s.rate_anchored);
        assert_eq!(s.degenerate_scenarios, 3, "every scenario hit the ε guard");
        assert!(s.bear <= s.base && s.base <= s.bull, "direct raw map holds the order");
    }

    #[test]
    fn a_thin_window_drops_the_rate_correction() {
        let observations: Vec<AnchorObservation> = (0..5)
            .map(|i| AnchorObservation { spread: 0.01, raw_multiple: 18.0 + i as f64 })
            .collect();
        let s = spread_anchored_scenarios(100.0, [5.0, 5.5, 6.0], &observations, 0.045, 0.0);
        assert!(!s.rate_anchored, "below the 8-observation floor");
        assert_eq!(s.anchor_observations, 5);
        assert!(!s.current_multiple_carry);
        assert!(s.bear <= s.base && s.base <= s.bull);
    }

    #[test]
    fn no_anchor_history_carries_the_current_multiple() {
        let s = spread_anchored_scenarios(100.0, [6.0, 6.5, 7.0], &[], 0.045, 2.0);
        assert!(s.current_multiple_carry);
        // Carry multiple = spot / base driver, so the base scenario lands on spot and
        // the spread comes from driver dispersion alone.
        assert!((s.base - 100.0).abs() < 1e-9);
        assert!(s.bear < s.base && s.base < s.bull);
        // TR decomposition: (P + forward income) / spot − 1.
        assert!((s.tr_base - (100.0 + 2.0) / 100.0 + 1.0).abs() < 1e-9);
    }

    #[test]
    fn grade_bands_are_monotone_and_momentum_free() {
        let f = |v: f64, momentum: f64| {
            grade_from_subscores(&SubScores {
                quality: v,
                valuation: v,
                momentum,
                risk: v,
            })
        };
        assert_eq!(f(95.0, 0.0), Grade::A);
        assert_eq!(f(72.0, 0.0), Grade::B);
        assert_eq!(f(60.0, 0.0), Grade::C);
        assert_eq!(f(45.0, 0.0), Grade::D);
        assert_eq!(f(10.0, 0.0), Grade::F);
        // Momentum no longer moves the letter — the settled ex-momentum re-weight.
        assert_eq!(f(72.0, 0.0), f(72.0, 100.0));
    }

    #[test]
    fn stock_tier_legs_trigger_and_default_per_the_missing_input_rule() {
        let fin = strong();
        let m = compute_metrics(&fin);
        // The strong large-cap: profitable, low leverage, low vol → Low.
        let (tier, gaps) = assign_stock_tier(&fin, &m);
        assert_eq!(tier, RiskTier::Low, "gaps: {gaps:?}");

        // A small cap trips a High leg regardless of the rest.
        let mut small = strong();
        small.market_cap = Some(1.0e9);
        let (tier, _) = assign_stock_tier(&small, &compute_metrics(&small));
        assert_eq!(tier, RiskTier::High);

        // Unprofitable trips High.
        let mut lossy = strong();
        lossy.net_income = Some(-5.0);
        lossy.operating_income = Some(-3.0);
        let (tier, _) = assign_stock_tier(&lossy, &compute_metrics(&lossy));
        assert_eq!(tier, RiskTier::High);

        // Wholesale-missing inputs read Medium with a logged gap — never a
        // fabricated High or Low.
        let empty = CompanyFinancials { symbol: "X".into(), ..Default::default() };
        let (tier, gaps) = assign_stock_tier(&empty, &compute_metrics(&empty));
        assert_eq!(tier, RiskTier::Medium);
        assert!(!gaps.is_empty());
    }

    #[test]
    fn fund_tier_maps_flag_vol_and_drawdown() {
        assert_eq!(assign_fund_tier(true, true, Some(0.10), Some(0.05)), RiskTier::High);
        assert_eq!(assign_fund_tier(false, false, Some(0.45), None), RiskTier::High);
        assert_eq!(assign_fund_tier(false, false, Some(0.30), Some(0.60)), RiskTier::High);
        assert_eq!(assign_fund_tier(false, false, Some(0.12), Some(0.15)), RiskTier::Low);
        // An option-overlay structural flag bars Low without forcing High.
        assert_eq!(assign_fund_tier(false, true, Some(0.12), Some(0.15)), RiskTier::Medium);
        assert_eq!(assign_fund_tier(false, false, Some(0.30), Some(0.20)), RiskTier::Medium);
        assert_eq!(assign_fund_tier(false, false, None, None), RiskTier::Medium);
    }

    #[test]
    fn hurdle_read_is_three_state_with_exit_side_hysteresis() {
        let scenario = |bear: f64, base: f64, bull: f64| ScenarioSet {
            bear: 0.0, base: 0.0, bull: 0.0,
            tr_bear: bear, tr_base: base, tr_bull: bull,
            rate_anchored: true, anchor_observations: 12,
            degenerate_scenarios: 0, monotonicity_repaired: false,
            current_multiple_carry: false,
        };
        // Hurdle = dgs2 0.04 + medium 0.05 = 0.09.
        let clears = hurdle_read(&scenario(0.10, 0.15, 0.20), 0.04, RiskTier::Medium);
        assert_eq!(clears.state, HurdleState::Clears);
        assert!(clears.admits_new_money);
        // Even the bull case misses → dead money.
        let fails = hurdle_read(&scenario(-0.05, 0.00, 0.05), 0.04, RiskTier::Medium);
        assert_eq!(fails.state, HurdleState::Fails);
        assert!(!fails.admits_new_money);
        // A base below the hurdle inside its own dispersion proves nothing — but the
        // point test still refuses new money.
        let indet = hurdle_read(&scenario(0.02, 0.07, 0.20), 0.04, RiskTier::Medium);
        assert_eq!(indet.state, HurdleState::Indeterminate);
        assert!(!indet.admits_new_money);
        // The admission point test can pass inside an indeterminate hurdle.
        let admit = hurdle_read(&scenario(0.02, 0.12, 0.20), 0.04, RiskTier::Medium);
        assert_eq!(admit.state, HurdleState::Indeterminate);
        assert!(admit.admits_new_money);
    }

    #[test]
    fn feasible_set_bounds_the_add_family() {
        let read = |state, admits| HurdleRead {
            state,
            hurdle_rate: Some(0.09),
            tr_bear: None, tr_base: None, tr_bull: None,
            admits_new_money: admits,
        };
        // A clean A-grade with headroom offers the full ladder.
        let full = feasible_actions(Grade::A, &read(HurdleState::Clears, true), 0.05);
        assert!(full.contains(&Action::Add) && full.contains(&Action::AddAggressively));
        // Dead money drops the add family at any grade; hold stays (hysteresis).
        let dead = feasible_actions(Grade::A, &read(HurdleState::Fails, false), 0.05);
        assert!(!dead.contains(&Action::Add));
        assert!(dead.contains(&Action::Hold));
        // Grade F bars the family; a C-grade passing admission gets add but never
        // add-aggressively (A/B only).
        assert!(!feasible_actions(Grade::F, &read(HurdleState::Clears, true), 0.05)
            .contains(&Action::Add));
        let c = feasible_actions(Grade::C, &read(HurdleState::Indeterminate, true), 0.05);
        assert!(c.contains(&Action::Add) && !c.contains(&Action::AddAggressively));
        // At the concentration cap the add family leaves the set.
        assert!(!feasible_actions(Grade::A, &read(HurdleState::Clears, true), 0.26)
            .contains(&Action::Add));
    }

    #[test]
    fn add_targets_get_absolute_floors() {
        // A tiny 0.5% position: pure multipliers would cap the add around 0.8%, so
        // the absolute floors lift the band (`docs/portfolio-analysis.md` §Starting
        // parameters).
        let position = Position {
            symbol: "AAPL".into(),
            description: "Apple".into(),
            asset_class: AssetClass::Stock,
            quantity: 5.0,
            cost_basis: 450.0,
            market_value: 500.0,
            current_price: Some(100.0),
        };
        let sizing = size_action(
            Action::Add,
            &position,
            &InvestorProfile::default_fixture(),
            100_000.0,
        );
        assert!(sizing.target_weight_low >= ADD_FLOOR_WEIGHT - 1e-12);
        let aggressive = size_action(
            Action::AddAggressively,
            &position,
            &InvestorProfile::default_fixture(),
            100_000.0,
        );
        assert!(aggressive.target_weight_low >= ADD_AGGRESSIVE_FLOOR_WEIGHT - 1e-12);
        // Trim stays purely relative — no floor lifts a reduction.
        let trim = size_action(Action::Trim, &position, &InvestorProfile::default_fixture(), 100_000.0);
        assert!(trim.target_weight_high < ADD_FLOOR_WEIGHT);
    }

    #[test]
    fn dated_join_and_drawdown_helpers_behave() {
        let series = vec![
            DatedValue { date: "2026-01-01".into(), value: 1.0 },
            DatedValue { date: "2026-02-01".into(), value: 2.0 },
            DatedValue { date: "2026-03-01".into(), value: 3.0 },
        ];
        assert_eq!(latest_on_or_before(&series, "2026-02-15"), Some(2.0));
        assert_eq!(latest_on_or_before(&series, "2026-03-01"), Some(3.0));
        assert_eq!(latest_on_or_before(&series, "2025-12-31"), None);
        // Percentiles interpolate linearly.
        assert!((percentile(&[1.0, 2.0, 3.0, 4.0, 5.0], 0.5) - 3.0).abs() < 1e-12);
        assert!((percentile(&[1.0, 2.0], 0.25) - 1.25).abs() < 1e-12);
        // Max drawdown: peak 100 → trough 60 = 40%.
        let closes = vec![80.0, 100.0, 70.0, 60.0, 90.0];
        assert!((max_drawdown(&[], &closes).unwrap() - 0.40).abs() < 1e-12);
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
