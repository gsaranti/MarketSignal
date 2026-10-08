//! The **soft forensic flags** (`docs/portfolio-analysis.md` §Starting parameters;
//! `docs/portfolio-workflow.md` §Step 6b): four engine-computed reads that sit
//! beside the hard-forensic filings state on the holding's audit — Altman Z
//! below 1.8, Piotroski F at or below 3, TTM net income above 1.3× TTM operating
//! cash flow, and receivables or inventory growing faster than revenue year over
//! year on the latest quarter — computed from `financial-scores` and the
//! statements. They move the model's read of conviction and risk and never the
//! letter grade; they bind no action rule, so they carry no parameter stamp —
//! the thresholds are drafted consts, like the hard-forensic lookback.
//!
//! Every flag is three-state — `fired`, `clear`, or `unevaluable` naming the
//! missing input — and a missing input never reads clear (the technology-event
//! pre-flag's rule: an unevaluable read records its typed reason, never a fired
//! or clear flag).

use serde::{Deserialize, Serialize};

use crate::portfolio::engine::{self, CompanyFinancials};

// ---- Calibration surface (drafted — `docs/portfolio-analysis.md` §Starting
//      parameters) -----------------------------------------------------------------

/// Altman Z fires strictly below this.
pub const ALTMAN_Z_DISTRESS: f64 = 1.8;
/// Piotroski F fires at or below this.
pub const PIOTROSKI_WEAK: f64 = 3.0;
/// Against positive TTM operating cash flow, TTM net income fires strictly above
/// this multiple of it; against zero or negative cash flow the multiple inverts
/// (1.3× a negative cash flow sits below it), so net income fires when it exceeds
/// the cash flow at all — earnings better than cash, the flag's concern.
pub const NET_INCOME_TO_OPERATING_CASH_FLOW: f64 = 1.3;
/// Against positive revenue growth, a receivables or inventory line fires strictly
/// above this multiple of it; against zero or negative revenue growth the multiple
/// inverts, so a line fires on any growth at all — a build while sales fall.
pub const WORKING_CAPITAL_TO_REVENUE_GROWTH: f64 = 1.5;

/// Boundary slack on the threshold tests (the overlay's rule): a value computed
/// onto a documented boundary can land a few ULPs off it in f64, and the drafted
/// rules say "below", "at or below" and "above" — so each test tolerates the
/// rounding rather than letting it decide a calibration boundary. The slack is
/// relative to the magnitudes compared, floored at one: a ratio or a score
/// compares at the absolute slack, while statement sums in dollars — where a
/// decimal-exact 1.3× boundary sits several ULPs wide — compare at the slack
/// scaled to their size.
const BOUNDARY_EPS: f64 = 1e-9;

fn slack(value: f64, threshold: f64) -> f64 {
    BOUNDARY_EPS * value.abs().max(threshold.abs()).max(1.0)
}

/// `value > threshold`, tolerant of float rounding at the boundary.
fn exceeds(value: f64, threshold: f64) -> bool {
    value - threshold > slack(value, threshold)
}

/// `value < threshold`, tolerant of float rounding at the boundary.
fn below(value: f64, threshold: f64) -> bool {
    threshold - value > slack(value, threshold)
}

/// `value ≤ threshold`, tolerant of float rounding at the boundary.
fn at_most(value: f64, threshold: f64) -> bool {
    value - threshold <= slack(value, threshold)
}

// ---- Typed reads ------------------------------------------------------------------

/// A flag's three-state read. `Unevaluable` names every missing input, so the
/// audit says what the statements or the scores did not supply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SoftFlagState {
    Fired,
    Clear,
    Unevaluable { missing: Vec<String> },
}

impl SoftFlagState {
    pub fn fired(&self) -> bool {
        matches!(self, SoftFlagState::Fired)
    }
}

/// A provider-score flag: the served value beside its read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreFlag {
    pub value: Option<f64>,
    pub state: SoftFlagState,
}

/// Net income against operating cash flow, both on the TTM basis — four
/// contiguous newest quarterly prints per statement, the two windows aligned on
/// one newest period end (the overlay's cross-statement rule).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetIncomeVsOperatingCashFlow {
    pub ttm_net_income: Option<f64>,
    pub ttm_operating_cash_flow: Option<f64>,
    pub state: SoftFlagState,
}

/// One line's leg of the working-capital-build flag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "leg", rename_all = "kebab-case")]
pub enum LineLeg {
    /// The line's year-over-year growth, tested against the revenue growth.
    Evaluated { growth: f64, fired: bool },
    /// The year-ago line is zero (or negative): no growth is defined, and an
    /// issuer without the line — a no-inventory business — contributes no leg;
    /// the flag reads on the other line.
    NotApplicable,
    /// The line is absent from the feed on the latest quarter or its year-ago
    /// comparator — a missing input, so the flag reads unevaluable.
    Missing,
    /// The line was not tested because a shared input — the revenue growth or an
    /// aligned five-quarter window — was missing; the flag's `missing` names it.
    NotTested,
}

/// Receivables / inventory growth against revenue growth, year over year on the
/// latest quarter: the newest print against the same fiscal quarter one year
/// back (index 4 of five contiguous rows), on the income and balance-sheet
/// windows aligned on one newest period end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkingCapitalBuild {
    pub revenue_growth: Option<f64>,
    pub receivables: LineLeg,
    pub inventory: LineLeg,
    pub state: SoftFlagState,
}

/// The four soft forensic flags, persisted on the holding's audit beside the
/// hard-forensic record (`docs/storage.md` §Local Analysis Suite Storage).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoftForensicFlags {
    pub altman_z: ScoreFlag,
    pub piotroski: ScoreFlag,
    pub net_income_vs_operating_cash_flow: NetIncomeVsOperatingCashFlow,
    pub working_capital_build: WorkingCapitalBuild,
}

// ---- Computation ------------------------------------------------------------------

/// Compute the four flags over a stock's financials. The statement rows are read
/// in canonical order (newest first, the latest filing winning a duplicated
/// period — `engine::canonicalize_statements`) so a shuffled or duplicated feed
/// response cannot shift the TTM or year-over-year windows.
pub fn compute(fin: &CompanyFinancials) -> SoftForensicFlags {
    let mut fin = fin.clone();
    engine::canonicalize_statements(&mut fin);
    let scores = fin.financial_scores.unwrap_or_default();
    SoftForensicFlags {
        altman_z: score_flag(scores.altman_z, "Altman Z", |z| below(z, ALTMAN_Z_DISTRESS)),
        piotroski: score_flag(scores.piotroski, "Piotroski F", |p| at_most(p, PIOTROSKI_WEAK)),
        net_income_vs_operating_cash_flow: net_income_vs_operating_cash_flow(&fin),
        working_capital_build: working_capital_build(&fin),
    }
}

fn score_flag(value: Option<f64>, name: &str, fires: impl Fn(f64) -> bool) -> ScoreFlag {
    match value.filter(|v| v.is_finite()) {
        Some(v) => ScoreFlag {
            value: Some(v),
            state: if fires(v) { SoftFlagState::Fired } else { SoftFlagState::Clear },
        },
        None => ScoreFlag {
            value: None,
            state: SoftFlagState::Unevaluable {
                missing: vec![format!("{name} (financial-scores)")],
            },
        },
    }
}

fn contiguous<'a>(period_ends: impl IntoIterator<Item = &'a str>) -> bool {
    engine::quarters_contiguous(period_ends)
}

fn net_income_vs_operating_cash_flow(fin: &CompanyFinancials) -> NetIncomeVsOperatingCashFlow {
    let income = &fin.quarterly_income;
    let cash = &fin.quarterly_cash_flow;
    let mut missing: Vec<String> = Vec::new();

    let income4 = income.len() >= 4 && contiguous(income[..4].iter().map(|r| r.period_end.as_str()));
    let cash4 = cash.len() >= 4 && contiguous(cash[..4].iter().map(|r| r.period_end.as_str()));

    // TTM sums over the four newest quarters — `None` unless all four carry the
    // line (a partial sum would misstate the trailing year).
    let ttm_net_income: Option<f64> = income4
        .then(|| income[..4].iter().map(|r| r.net_income).sum::<Option<f64>>())
        .flatten();
    if ttm_net_income.is_none() {
        missing.push(
            "TTM net income (four contiguous quarterly income prints carrying the line)".into(),
        );
    }
    let ttm_operating_cash_flow: Option<f64> = cash4
        .then(|| cash[..4].iter().map(|r| r.operating_cash_flow).sum::<Option<f64>>())
        .flatten();
    if ttm_operating_cash_flow.is_none() {
        missing.push(
            "TTM operating cash flow (four contiguous quarterly cash-flow prints carrying the line)"
                .into(),
        );
    }
    // The cross-statement ratio: both windows must cover the SAME trailing year
    // — matching newest period ends on two contiguous windows aligns them whole.
    let aligned = income4 && cash4 && income[0].period_end == cash[0].period_end;
    if ttm_net_income.is_some() && ttm_operating_cash_flow.is_some() && !aligned {
        missing.push("income and cash-flow windows aligned on one newest period end".into());
    }

    let state = match (ttm_net_income, ttm_operating_cash_flow) {
        (Some(net_income), Some(operating_cash_flow)) if aligned => {
            let threshold = if operating_cash_flow > 0.0 {
                NET_INCOME_TO_OPERATING_CASH_FLOW * operating_cash_flow
            } else {
                operating_cash_flow
            };
            if exceeds(net_income, threshold) {
                SoftFlagState::Fired
            } else {
                SoftFlagState::Clear
            }
        }
        _ => SoftFlagState::Unevaluable { missing },
    };
    NetIncomeVsOperatingCashFlow {
        ttm_net_income,
        ttm_operating_cash_flow,
        state,
    }
}

fn working_capital_build(fin: &CompanyFinancials) -> WorkingCapitalBuild {
    let income = &fin.quarterly_income;
    let balance = &fin.quarterly_balance_sheet;
    let mut missing: Vec<String> = Vec::new();

    // Year over year on the latest quarter: index 0 against index 4, an index
    // arithmetic that is only a year apart when rows 0..=4 are contiguous.
    let income5 = income.len() >= 5 && contiguous(income[..5].iter().map(|r| r.period_end.as_str()));
    let balance5 =
        balance.len() >= 5 && contiguous(balance[..5].iter().map(|r| r.period_end.as_str()));
    let aligned = income5 && balance5 && income[0].period_end == balance[0].period_end;

    let revenue_growth = match (
        income5.then(|| income[0].revenue).flatten(),
        income5.then(|| income[4].revenue).flatten(),
    ) {
        (Some(now), Some(prior)) if prior > 0.0 => Some(now / prior - 1.0),
        _ => None,
    };
    if revenue_growth.is_none() {
        missing.push(
            "year-over-year revenue growth (five contiguous quarterly income prints with a positive year-ago revenue)"
                .into(),
        );
    }
    if !balance5 {
        missing.push("five contiguous quarterly balance sheets".into());
    } else if income5 && !aligned {
        missing.push("income and balance-sheet windows aligned on one newest period end".into());
    }

    let (receivables, inventory) = match revenue_growth {
        Some(revenue_growth) if balance5 && aligned => {
            let leg = |now: Option<f64>, prior: Option<f64>| match (now, prior) {
                (Some(now), Some(prior)) if prior > 0.0 => {
                    let growth = now / prior - 1.0;
                    let fired = if revenue_growth > 0.0 {
                        exceeds(growth, WORKING_CAPITAL_TO_REVENUE_GROWTH * revenue_growth)
                    } else {
                        exceeds(growth, 0.0)
                    };
                    LineLeg::Evaluated { growth, fired }
                }
                (Some(_), Some(_)) => LineLeg::NotApplicable,
                _ => LineLeg::Missing,
            };
            (
                leg(balance[0].net_receivables, balance[4].net_receivables),
                leg(balance[0].inventory, balance[4].inventory),
            )
        }
        _ => (LineLeg::NotTested, LineLeg::NotTested),
    };
    if receivables == LineLeg::Missing {
        missing.push("net receivables on the latest quarter and its year-ago comparator".into());
    }
    if inventory == LineLeg::Missing {
        missing.push("inventory on the latest quarter and its year-ago comparator".into());
    }
    if receivables == LineLeg::NotApplicable && inventory == LineLeg::NotApplicable {
        missing.push("a positive year-ago receivables or inventory line".into());
    }

    let state = if !missing.is_empty() {
        SoftFlagState::Unevaluable { missing }
    } else if [&receivables, &inventory]
        .iter()
        .any(|leg| matches!(leg, LineLeg::Evaluated { fired: true, .. }))
    {
        SoftFlagState::Fired
    } else {
        SoftFlagState::Clear
    };
    WorkingCapitalBuild {
        revenue_growth,
        receivables,
        inventory,
        state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::engine::{
        FinancialScores, QuarterlyBalanceSheetRow, QuarterlyCashFlowRow, QuarterlyIncomeRow,
    };

    const ENDS: [&str; 5] = ["2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30", "2025-06-30"];

    fn income(period: &str, revenue: f64, net_income: f64) -> QuarterlyIncomeRow {
        QuarterlyIncomeRow {
            period_end: period.into(),
            revenue: Some(revenue),
            net_income: Some(net_income),
            ..Default::default()
        }
    }

    fn cash(period: &str, operating_cash_flow: f64) -> QuarterlyCashFlowRow {
        QuarterlyCashFlowRow {
            period_end: period.into(),
            operating_cash_flow: Some(operating_cash_flow),
            ..Default::default()
        }
    }

    fn balance(period: &str, receivables: Option<f64>, inventory: Option<f64>) -> QuarterlyBalanceSheetRow {
        QuarterlyBalanceSheetRow {
            period_end: period.into(),
            net_receivables: receivables,
            inventory,
            ..Default::default()
        }
    }

    /// A clean issuer: revenue 100 a quarter growing to 110 on the latest, net
    /// income 10 a quarter, operating cash flow 12, receivables 10 → 11,
    /// inventory 5 → 5.2.
    fn clean() -> CompanyFinancials {
        CompanyFinancials {
            quarterly_income: ENDS
                .iter()
                .enumerate()
                .map(|(i, end)| income(end, if i == 0 { 110.0 } else { 100.0 }, 10.0))
                .collect(),
            quarterly_cash_flow: ENDS.iter().map(|end| cash(end, 12.0)).collect(),
            quarterly_balance_sheet: ENDS
                .iter()
                .enumerate()
                .map(|(i, end)| {
                    if i == 0 {
                        balance(end, Some(11.0), Some(5.2))
                    } else {
                        balance(end, Some(10.0), Some(5.0))
                    }
                })
                .collect(),
            financial_scores: Some(FinancialScores {
                altman_z: Some(3.2),
                piotroski: Some(7.0),
            }),
            ..Default::default()
        }
    }

    fn unevaluable_names(state: &SoftFlagState, needle: &str) -> bool {
        matches!(state, SoftFlagState::Unevaluable { missing } if missing.iter().any(|m| m.contains(needle)))
    }

    #[test]
    fn a_clean_issuer_reads_clear_on_every_flag() {
        let flags = compute(&clean());
        assert_eq!(flags.altman_z.state, SoftFlagState::Clear);
        assert_eq!(flags.piotroski.state, SoftFlagState::Clear);
        assert_eq!(flags.net_income_vs_operating_cash_flow.state, SoftFlagState::Clear);
        assert_eq!(flags.working_capital_build.state, SoftFlagState::Clear);
        assert_eq!(flags.net_income_vs_operating_cash_flow.ttm_net_income, Some(40.0));
        assert_eq!(flags.net_income_vs_operating_cash_flow.ttm_operating_cash_flow, Some(48.0));
        assert!(
            (flags.working_capital_build.revenue_growth.unwrap() - 0.1).abs() < 1e-12
        );
    }

    #[test]
    fn altman_and_piotroski_fire_on_their_drafted_boundaries_and_never_clear_when_missing() {
        let z = |value: Option<f64>| {
            let mut fin = clean();
            fin.financial_scores = Some(FinancialScores { altman_z: value, piotroski: Some(7.0) });
            compute(&fin).altman_z
        };
        assert!(z(Some(1.79)).state.fired());
        assert_eq!(z(Some(1.8)).state, SoftFlagState::Clear, "strictly below 1.8");
        assert_eq!(z(Some(1.81)).state, SoftFlagState::Clear);
        assert_eq!(z(Some(1.8 - 0.1 + 0.1)).state, SoftFlagState::Clear, "rounding at the boundary");
        let missing = z(None);
        assert_eq!(missing.value, None);
        assert!(unevaluable_names(&missing.state, "Altman Z"));
        assert!(unevaluable_names(&z(Some(f64::NAN)).state, "Altman Z"));

        let p = |value: Option<f64>| {
            let mut fin = clean();
            fin.financial_scores = Some(FinancialScores { altman_z: Some(3.0), piotroski: value });
            compute(&fin).piotroski
        };
        assert!(p(Some(3.0)).state.fired(), "at or below 3");
        assert!(p(Some(0.0)).state.fired());
        assert_eq!(p(Some(4.0)).state, SoftFlagState::Clear);
        assert!(unevaluable_names(&p(None).state, "Piotroski"));

        // The whole scores call gapped: both score flags unevaluable, the
        // statement flags untouched.
        let mut fin = clean();
        fin.financial_scores = None;
        let flags = compute(&fin);
        assert!(unevaluable_names(&flags.altman_z.state, "financial-scores"));
        assert!(unevaluable_names(&flags.piotroski.state, "financial-scores"));
        assert_eq!(flags.net_income_vs_operating_cash_flow.state, SoftFlagState::Clear);
    }

    #[test]
    fn net_income_against_operating_cash_flow_reads_the_ttm_basis() {
        // TTM net income 40 against TTM operating cash flow 30: 40 > 39 fires.
        let mut fin = clean();
        fin.quarterly_cash_flow = ENDS.iter().map(|end| cash(end, 7.5)).collect();
        let read = compute(&fin).net_income_vs_operating_cash_flow;
        assert!(read.state.fired());
        assert_eq!(read.ttm_operating_cash_flow, Some(30.0));
        // Exactly 1.3× is not above it: 52 against 40.
        let mut fin = clean();
        fin.quarterly_income = ENDS
            .iter()
            .enumerate()
            .map(|(i, end)| income(end, if i == 0 { 110.0 } else { 100.0 }, 13.0))
            .collect();
        fin.quarterly_cash_flow = ENDS.iter().map(|end| cash(end, 10.0)).collect();
        assert_eq!(compute(&fin).net_income_vs_operating_cash_flow.state, SoftFlagState::Clear);
    }

    #[test]
    fn a_decimal_exact_boundary_in_dollars_never_fires_on_rounding() {
        // Four quarters of operating cash flow 100,000,001.10 and net income
        // 130,000,001.43 — exactly 1.3× in decimal — land a few ULPs over the
        // product in f64; the magnitude-aware slack reads them as the boundary,
        // and a whole dollar over it still fires.
        let with = |quarterly_net_income: f64| {
            let mut fin = clean();
            fin.quarterly_income = ENDS
                .iter()
                .enumerate()
                .map(|(i, end)| income(end, if i == 0 { 110.0 } else { 100.0 }, quarterly_net_income))
                .collect();
            fin.quarterly_cash_flow = ENDS.iter().map(|end| cash(end, 100_000_001.10)).collect();
            compute(&fin).net_income_vs_operating_cash_flow.state
        };
        assert_eq!(with(130_000_001.43), SoftFlagState::Clear);
        assert!(with(130_000_002.43).fired());
    }

    #[test]
    fn against_non_positive_cash_flow_net_income_fires_on_any_excess() {
        let with = |quarterly_net_income: f64, quarterly_cash_flow: f64| {
            let mut fin = clean();
            fin.quarterly_income = ENDS
                .iter()
                .enumerate()
                .map(|(i, end)| income(end, if i == 0 { 110.0 } else { 100.0 }, quarterly_net_income))
                .collect();
            fin.quarterly_cash_flow = ENDS.iter().map(|end| cash(end, quarterly_cash_flow)).collect();
            compute(&fin).net_income_vs_operating_cash_flow.state
        };
        // TTM −20 against −40: earnings better than cash, fired.
        assert!(with(-5.0, -10.0).fired());
        // TTM −45 against −40: earnings worse than cash, clear — the literal
        // 1.3× multiple would have fired here (−45 > −52).
        assert_eq!(with(-11.25, -10.0), SoftFlagState::Clear);
        // TTM −38 against −40: above by any amount fires.
        assert!(with(-9.5, -10.0).fired());
        // Zero cash flow: positive net income fires, zero stays clear.
        assert!(with(0.25, 0.0).fired());
        assert_eq!(with(0.0, 0.0), SoftFlagState::Clear);
    }

    #[test]
    fn net_income_against_operating_cash_flow_is_unevaluable_on_a_gap_or_a_misaligned_window() {
        // A net-income line missing inside the window.
        let mut fin = clean();
        fin.quarterly_income[2].net_income = None;
        let read = compute(&fin).net_income_vs_operating_cash_flow;
        assert_eq!(read.ttm_net_income, None);
        assert!(unevaluable_names(&read.state, "TTM net income"));
        // A non-contiguous income window (a skipped quarter).
        let mut fin = clean();
        fin.quarterly_income.remove(1);
        assert!(unevaluable_names(
            &compute(&fin).net_income_vs_operating_cash_flow.state,
            "TTM net income"
        ));
        // Cash flow served one quarter behind income: both sums exist, the
        // windows do not cover the same year.
        let mut fin = clean();
        fin.quarterly_cash_flow.remove(0);
        fin.quarterly_cash_flow.push(cash("2025-03-31", 12.0));
        let read = compute(&fin).net_income_vs_operating_cash_flow;
        assert_eq!(read.ttm_net_income, Some(40.0));
        assert_eq!(read.ttm_operating_cash_flow, Some(48.0));
        assert!(unevaluable_names(&read.state, "aligned"));
    }

    #[test]
    fn working_capital_build_compares_year_over_year_on_the_latest_quarter() {
        // Revenue +10%: receivables +15% sits on the 1.5× boundary (clear),
        // +16% is above it (fired); inventory +4% stays clear.
        let mut fin = clean();
        fin.quarterly_balance_sheet[0] = balance(ENDS[0], Some(11.5), Some(5.2));
        let read = compute(&fin).working_capital_build;
        assert_eq!(read.state, SoftFlagState::Clear);
        assert!(matches!(read.receivables, LineLeg::Evaluated { fired: false, .. }));
        let mut fin = clean();
        fin.quarterly_balance_sheet[0] = balance(ENDS[0], Some(11.6), Some(5.2));
        let read = compute(&fin).working_capital_build;
        assert!(read.state.fired());
        assert!(matches!(read.receivables, LineLeg::Evaluated { fired: true, .. }));
        assert!(matches!(read.inventory, LineLeg::Evaluated { fired: false, .. }));
        // The inventory line alone can fire it.
        let mut fin = clean();
        fin.quarterly_balance_sheet[0] = balance(ENDS[0], Some(10.5), Some(6.0));
        assert!(compute(&fin).working_capital_build.state.fired());
    }

    #[test]
    fn against_falling_revenue_a_line_fires_on_any_growth_and_never_on_a_smaller_fall() {
        let with_revenue_and_receivables = |latest_revenue: f64, latest_receivables: f64| {
            let mut fin = clean();
            fin.quarterly_income[0] = income(ENDS[0], latest_revenue, 10.0);
            fin.quarterly_balance_sheet[0] = balance(ENDS[0], Some(latest_receivables), Some(5.0));
            compute(&fin).working_capital_build
        };
        // Revenue −10%: receivables +5% is a build while sales fall.
        assert!(with_revenue_and_receivables(90.0, 10.5).state.fired());
        // Receivables −12% against revenue −10% is a bigger fall, not a build —
        // the literal 1.5× multiple would have fired here (−12% > −15%).
        assert_eq!(with_revenue_and_receivables(90.0, 8.8).state, SoftFlagState::Clear);
        assert_eq!(with_revenue_and_receivables(90.0, 10.0).state, SoftFlagState::Clear);
        // Flat revenue: any growth fires.
        assert!(with_revenue_and_receivables(100.0, 10.1).state.fired());
    }

    #[test]
    fn a_zero_year_ago_line_is_not_applicable_and_the_other_line_still_reads() {
        // A no-inventory issuer: inventory zero on both quarters contributes no
        // leg, and the flag reads on receivables alone.
        let mut fin = clean();
        for row in &mut fin.quarterly_balance_sheet {
            row.inventory = Some(0.0);
        }
        let read = compute(&fin).working_capital_build;
        assert_eq!(read.inventory, LineLeg::NotApplicable);
        assert_eq!(read.state, SoftFlagState::Clear);
        fin.quarterly_balance_sheet[0].net_receivables = Some(12.0);
        assert!(compute(&fin).working_capital_build.state.fired());
        // Inventory appearing from a zero year-ago line defines no growth either.
        fin.quarterly_balance_sheet[0] = balance(ENDS[0], Some(11.0), Some(3.0));
        let read = compute(&fin).working_capital_build;
        assert_eq!(read.inventory, LineLeg::NotApplicable);
        assert_eq!(read.state, SoftFlagState::Clear);
        // Both lines zero a year ago: nothing to read.
        for row in &mut fin.quarterly_balance_sheet {
            row.net_receivables = Some(0.0);
        }
        let read = compute(&fin).working_capital_build;
        assert_eq!(read.receivables, LineLeg::NotApplicable);
        assert!(unevaluable_names(&read.state, "positive year-ago"));
    }

    #[test]
    fn a_missing_line_or_window_reads_unevaluable_never_clear() {
        // The inventory line absent from the year-ago print: receivables still
        // evaluate, the flag does not.
        let mut fin = clean();
        fin.quarterly_balance_sheet[4].inventory = None;
        let read = compute(&fin).working_capital_build;
        assert_eq!(read.inventory, LineLeg::Missing);
        assert!(matches!(read.receivables, LineLeg::Evaluated { .. }));
        assert!(unevaluable_names(&read.state, "inventory"));
        // Only four balance sheets: no year-ago comparator.
        let mut fin = clean();
        fin.quarterly_balance_sheet.pop();
        let read = compute(&fin).working_capital_build;
        assert_eq!(read.receivables, LineLeg::NotTested);
        assert!(unevaluable_names(&read.state, "five contiguous quarterly balance sheets"));
        // The balance window one quarter behind the income window.
        let mut fin = clean();
        fin.quarterly_balance_sheet.remove(0);
        fin.quarterly_balance_sheet.push(balance("2025-03-31", Some(10.0), Some(5.0)));
        assert!(unevaluable_names(&compute(&fin).working_capital_build.state, "aligned"));
        // A zero year-ago revenue defines no revenue growth.
        let mut fin = clean();
        fin.quarterly_income[4].revenue = Some(0.0);
        let read = compute(&fin).working_capital_build;
        assert_eq!(read.revenue_growth, None);
        assert_eq!(read.receivables, LineLeg::NotTested);
        assert!(unevaluable_names(&read.state, "revenue growth"));
    }

    #[test]
    fn rows_read_in_canonical_order_whatever_the_wire_served() {
        let mut fin = clean();
        fin.quarterly_balance_sheet[0] = balance(ENDS[0], Some(11.6), Some(5.2));
        let expected = compute(&fin);
        assert!(expected.working_capital_build.state.fired());
        // Shuffle every statement and duplicate a print with an older filing.
        fin.quarterly_income.reverse();
        fin.quarterly_cash_flow.rotate_left(2);
        fin.quarterly_balance_sheet.reverse();
        let mut stale = balance(ENDS[0], Some(10.0), Some(5.0));
        stale.filing_date = Some("2026-07-01".into());
        fin.quarterly_balance_sheet[4].filing_date = Some("2026-08-01".into());
        fin.quarterly_balance_sheet.push(stale);
        assert_eq!(compute(&fin), expected);
    }

    #[test]
    fn flags_round_trip_through_json_with_tagged_states() {
        let mut fin = clean();
        fin.quarterly_balance_sheet[4].inventory = None;
        fin.financial_scores = Some(FinancialScores { altman_z: Some(1.2), piotroski: None });
        let flags = compute(&fin);
        let json = serde_json::to_value(&flags).unwrap();
        assert_eq!(json["altman_z"]["state"]["state"], "fired");
        assert_eq!(json["piotroski"]["state"]["state"], "unevaluable");
        assert_eq!(json["working_capital_build"]["inventory"]["leg"], "missing");
        assert_eq!(json["working_capital_build"]["receivables"]["leg"], "evaluated");
        let back: SoftForensicFlags = serde_json::from_value(json).unwrap();
        assert_eq!(back, flags);
    }
}
