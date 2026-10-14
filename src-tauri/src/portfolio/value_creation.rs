//! The **value-creation read** — the shared engine's second engine lens
//! (`docs/trade-opportunities.md` §The lenses, §Starting parameters;
//! `docs/trade-opportunities-workflow.md` §Step 5c): whether the business earns
//! more on its capital than that capital costs (ROIC against a CAPM-based cost of
//! capital), with **R&D capitalized** so a research-heavy name isn't mis-scored as
//! unprofitable; the reinvestment rate and the growth it compounds at
//! (g ≈ ROIC × reinvestment); owner earnings; and the measurable moat proxies the
//! thesis document reads — classifying the moat by source stays the model's.
//!
//! Pure over [`CompanyFinancials`], the profile sector and the run-level `DGS10`,
//! so every leg is unit-testable offline. A leg whose inputs are missing reads
//! absent with a typed reason ([`ValueCreationGap`]), never a guessed value.
//!
//! The module also owns the shared engine's own-history unit, the trailing-twelve-
//! month statement window ([`ttm_windows`]), which the factor composite
//! (`portfolio::factors`) reads too.

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::engine::{quarters_contiguous, CompanyFinancials, FILING_GRACE_DAYS};

// ---------------------------------------------------------------------------
// Calibration surface — every value drafted, calibratable
// (`docs/trade-opportunities.md` §Starting parameters, the value-creation row).
// ---------------------------------------------------------------------------

/// Straight-line amortizable life of R&D, in years — uniform across names, and
/// exact from the sixteen quarters the income pull carries (the current year plus
/// the three behind it).
pub const RND_LIFE_YEARS: usize = 3;

/// The tax rate NOPAT applies when no usable effective rate exists.
pub const STATUTORY_TAX_RATE: f64 = 0.21;

/// The effective tax rate's admissible range (tax expense over pre-tax income);
/// a rate outside it — a one-off charge, a benefit year — takes the statutory rate.
pub const EFFECTIVE_TAX_RATE_RANGE: (f64, f64) = (0.0, 0.40);

/// The equity risk premium the cost of equity prices — Damodaran's implied US
/// premium at the start of 2026 was 4.23%.
pub const EQUITY_RISK_PREMIUM: f64 = 0.0425;

/// The Blume adjustment's weight on the served beta: adjusted β =
/// `BLUME_WEIGHT × β + (1 − BLUME_WEIGHT)`, shrinking toward the market's 1.0.
pub const BLUME_WEIGHT: f64 = 0.67;

/// The pre-tax cost of debt's spread over `DGS10`.
pub const DEBT_SPREAD_OVER_RF: f64 = 0.015;

/// The fewest own-history windows the moat proxies' stability and persistence
/// reads stand on. It counts windows including today's, where the factor
/// composite's own-history minimum counts prior points only.
pub const MOAT_MIN_WINDOWS: usize = 8;

// ---------------------------------------------------------------------------
// The trailing-twelve-month window — the shared engine's own-history unit.
// ---------------------------------------------------------------------------

/// One trailing-twelve-month statement window ending at a quarter. Window 0 ends
/// at the newest quarter; window k ends k quarters earlier, inside the newest
/// contiguous run of quarterly income prints. Flow lines sum the window's four
/// quarters (`None` when any quarter lacks the line); balance-sheet lines are the
/// instant at the window's period end (`None` when no balance sheet carries it).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TtmWindow {
    /// The window's newest quarter end, ISO date.
    pub period_end: String,
    /// When the window became market-observable: the newest quarter's filing
    /// date, else its period end plus the filing grace. `None` where neither
    /// resolves.
    pub anchor_date: Option<String>,
    pub revenue: Option<f64>,
    /// Each quarter's reported gross profit, else revenue − cost of revenue (the
    /// TTM statement basis's per-quarter rule).
    pub gross_profit: Option<f64>,
    pub operating_income: Option<f64>,
    pub net_income: Option<f64>,
    pub research_and_development: Option<f64>,
    pub income_before_tax: Option<f64>,
    pub income_tax_expense: Option<f64>,
    /// Each quarter's resolved free cash flow (the reported line, else operating
    /// cash flow − |capex|).
    pub free_cash_flow: Option<f64>,
    /// Capital expenditure as a magnitude (the feed serves it signed).
    pub capex: Option<f64>,
    pub depreciation_and_amortization: Option<f64>,
    /// The newest quarter's diluted weighted share count.
    pub diluted_shares: Option<f64>,
    pub total_assets: Option<f64>,
    pub total_debt: Option<f64>,
    pub total_equity: Option<f64>,
    /// Cash and equivalents plus short-term investments; `None` only where the
    /// cash line itself is absent (an absent short-term-investments line adds
    /// nothing).
    pub cash: Option<f64>,
}

/// Sum a window's per-quarter values; `None` when any quarter lacks one.
fn sum_all(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    values.into_iter().sum()
}

/// Every TTM window inside the newest contiguous run of quarterly income prints,
/// newest first. The rows are put in the canonical order locally (newest period
/// end first, the latest filing winning a duplicated period —
/// `engine::canonicalize_statements`'s rule), so the read never depends on wire
/// order; a run broken by a skipped quarter stops at the break.
pub fn ttm_windows(fin: &CompanyFinancials) -> Vec<TtmWindow> {
    let mut income: Vec<_> = fin.quarterly_income.iter().collect();
    income.sort_by(|a, b| {
        b.period_end
            .cmp(&a.period_end)
            .then_with(|| b.filing_date.cmp(&a.filing_date))
    });
    income.dedup_by(|a, b| a.period_end == b.period_end);
    // The newest contiguous run: extend while the prefix stays consecutive.
    let mut run = 0;
    while run < income.len()
        && quarters_contiguous(income[..=run].iter().map(|r| r.period_end.as_str()))
    {
        run += 1;
    }
    let income = &income[..run];

    // Cash-flow and balance-sheet rows by period end, the latest filing winning
    // a duplicated period.
    let mut cash_flow = HashMap::new();
    for row in &fin.quarterly_cash_flow {
        let keep = cash_flow
            .get(row.period_end.as_str())
            .is_none_or(|kept: &&super::engine::QuarterlyCashFlowRow| {
                row.filing_date > kept.filing_date
            });
        if keep {
            cash_flow.insert(row.period_end.as_str(), row);
        }
    }
    let mut balance = HashMap::new();
    for row in &fin.quarterly_balance_sheet {
        let keep = balance
            .get(row.period_end.as_str())
            .is_none_or(|kept: &&super::engine::QuarterlyBalanceSheetRow| {
                row.filing_date > kept.filing_date
            });
        if keep {
            balance.insert(row.period_end.as_str(), row);
        }
    }

    income
        .windows(4)
        .map(|quarters| {
            let newest = quarters[0];
            let flows: Vec<_> = quarters
                .iter()
                .map(|q| cash_flow.get(q.period_end.as_str()).copied())
                .collect();
            let instant = balance.get(newest.period_end.as_str()).copied();
            let anchor_date = newest.filing_date.clone().or_else(|| {
                NaiveDate::parse_from_str(&newest.period_end, "%Y-%m-%d")
                    .ok()
                    .and_then(|d| d.checked_add_signed(chrono::Duration::days(FILING_GRACE_DAYS)))
                    .map(|d| d.format("%Y-%m-%d").to_string())
            });
            TtmWindow {
                period_end: newest.period_end.clone(),
                anchor_date,
                revenue: sum_all(quarters.iter().map(|q| q.revenue)),
                gross_profit: sum_all(quarters.iter().map(|q| {
                    q.gross_profit
                        .or_else(|| Some(q.revenue? - q.cost_of_revenue?))
                })),
                operating_income: sum_all(quarters.iter().map(|q| q.operating_income)),
                net_income: sum_all(quarters.iter().map(|q| q.net_income)),
                research_and_development: sum_all(
                    quarters.iter().map(|q| q.research_and_development),
                ),
                income_before_tax: sum_all(quarters.iter().map(|q| q.income_before_tax)),
                income_tax_expense: sum_all(quarters.iter().map(|q| q.income_tax_expense)),
                free_cash_flow: sum_all(
                    flows.iter().map(|f| f.and_then(|f| f.resolved_free_cash_flow())),
                ),
                capex: sum_all(flows.iter().map(|f| f.and_then(|f| f.capex).map(f64::abs))),
                depreciation_and_amortization: sum_all(
                    flows.iter().map(|f| f.and_then(|f| f.depreciation_and_amortization)),
                ),
                diluted_shares: newest.diluted_shares,
                total_assets: instant.and_then(|b| b.total_assets),
                total_debt: instant.and_then(|b| b.total_debt),
                total_equity: instant.and_then(|b| b.total_equity),
                cash: instant.and_then(|b| {
                    b.cash_and_equivalents
                        .map(|c| c + b.short_term_investments.unwrap_or(0.0))
                }),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The read.
// ---------------------------------------------------------------------------

/// Where the tax rate NOPAT applies came from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaxRateBasis {
    /// TTM income-tax expense over TTM pre-tax income, inside the admissible range.
    Effective,
    /// [`STATUTORY_TAX_RATE`] — no usable effective rate.
    Statutory,
}

/// The R&D capitalization the read stands on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum RndCapitalization {
    /// No quarter carries a non-zero R&D line (the feed serves a zero for a
    /// name with no R&D) — nothing to capitalize, and no adjustment.
    NotReported,
    /// Straight-line over [`RND_LIFE_YEARS`]: the research asset (unamortized past
    /// R&D) joins invested capital, and this year's R&D less the year's
    /// amortization joins NOPAT and owner earnings.
    Capitalized {
        /// This TTM year's R&D expense.
        ttm_rnd: f64,
        research_asset: f64,
        amortization: f64,
    },
    /// R&D is reported but the life's annual history isn't — the reads stand
    /// unadjusted, with the reason.
    Unavailable { reason: String },
}

impl RndCapitalization {
    /// This year's R&D less its amortization — the adjustment NOPAT, owner
    /// earnings and reinvestment add; zero when nothing is capitalized.
    fn net_investment(&self) -> f64 {
        match self {
            Self::Capitalized { ttm_rnd, amortization, .. } => ttm_rnd - amortization,
            _ => 0.0,
        }
    }

    fn research_asset(&self) -> f64 {
        match self {
            Self::Capitalized { research_asset, .. } => *research_asset,
            _ => 0.0,
        }
    }
}

/// The cost of capital the ROIC spread subtracts: a WACC over a CAPM cost of
/// equity and an after-tax cost of debt, weighted by market equity against book
/// debt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostOfCapital {
    pub rate: f64,
    pub cost_of_equity: f64,
    pub after_tax_cost_of_debt: f64,
    pub equity_weight: f64,
    /// The Blume-adjusted beta the cost of equity priced.
    pub adjusted_beta: f64,
}

/// The measurable moat proxies — features the thesis document reads, never a
/// classification.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MoatProxies {
    /// TTM gross margin.
    pub gross_margin: Option<f64>,
    /// The gross margin less its sector's anchor (`portfolio::factors`; the
    /// market-wide anchor where the sector is unmapped).
    pub gross_margin_vs_sector: Option<f64>,
    /// The population standard deviation of the gross margin across the own-history
    /// windows — lower is steadier; `None` below [`MOAT_MIN_WINDOWS`].
    pub gross_margin_stability: Option<f64>,
    /// The share of own-history windows whose unadjusted ROIC exceeds today's cost
    /// of capital; `None` below [`MOAT_MIN_WINDOWS`] or without a cost of capital.
    pub roic_persistence: Option<f64>,
    /// TTM R&D over TTM revenue; `None` where no R&D line is reported.
    pub rnd_intensity: Option<f64>,
}

/// A value-creation leg that could not be computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ValueCreationLeg {
    Nopat,
    InvestedCapital,
    Roic,
    CostOfCapital,
    RoicSpread,
    OwnerEarnings,
    Reinvestment,
    Growth,
}

/// One absent leg and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueCreationGap {
    pub leg: ValueCreationLeg,
    pub reason: String,
}

/// The value-creation read. Every value is a decimal ratio or a currency level as
/// the statements report it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueCreationRead {
    /// The tax rate NOPAT applied and where it came from; `None` without a TTM
    /// window.
    pub tax_rate: Option<(f64, TaxRateBasis)>,
    pub rnd: RndCapitalization,
    /// EBIT × (1 − t) plus this year's R&D less its amortization.
    pub nopat: Option<f64>,
    /// Book debt + book equity − cash + the research asset.
    pub invested_capital: Option<f64>,
    pub roic: Option<f64>,
    /// The CAPM cost of equity at the run's `DGS10` and the Blume-adjusted
    /// profile beta, wherever both are served — the ROE spread's hurdle
    /// (`portfolio::factors`), standing without the debt line the WACC needs.
    pub cost_of_equity: Option<f64>,
    pub cost_of_capital: Option<CostOfCapital>,
    /// ROIC − the cost of capital.
    pub roic_spread: Option<f64>,
    /// TTM free cash flow plus this year's R&D less its amortization.
    pub owner_earnings: Option<f64>,
    /// (|capex| − D&A + R&D − R&D amortization) ÷ NOPAT, over a positive NOPAT.
    pub reinvestment_rate: Option<f64>,
    /// g ≈ ROIC × the reinvestment rate.
    pub growth: Option<f64>,
    pub moat: MoatProxies,
    pub gaps: Vec<ValueCreationGap>,
}

fn gap(gaps: &mut Vec<ValueCreationGap>, leg: ValueCreationLeg, reason: &str) {
    gaps.push(ValueCreationGap { leg, reason: reason.to_string() });
}

/// The tax rate NOPAT applies: the window's effective rate inside
/// [`EFFECTIVE_TAX_RATE_RANGE`], else the statutory rate.
fn tax_rate(window: &TtmWindow) -> (f64, TaxRateBasis) {
    match (window.income_tax_expense, window.income_before_tax) {
        (Some(tax), Some(pretax)) if pretax > 0.0 => {
            let rate = tax / pretax;
            let (lo, hi) = EFFECTIVE_TAX_RATE_RANGE;
            if rate.is_finite() && (lo..=hi).contains(&rate) {
                (rate, TaxRateBasis::Effective)
            } else {
                (STATUTORY_TAX_RATE, TaxRateBasis::Statutory)
            }
        }
        _ => (STATUTORY_TAX_RATE, TaxRateBasis::Statutory),
    }
}

/// Whether any quarter carries a non-zero R&D line.
fn reports_rnd(fin: &CompanyFinancials) -> bool {
    fin.quarterly_income
        .iter()
        .any(|q| q.research_and_development.is_some_and(|r| r != 0.0))
}

/// Straight-line R&D capitalization over [`RND_LIFE_YEARS`]. The life needs the
/// current TTM year and the `RND_LIFE_YEARS` years behind it (windows 0, 4, 8, …).
fn capitalize_rnd(fin: &CompanyFinancials, windows: &[TtmWindow]) -> RndCapitalization {
    if !reports_rnd(fin) {
        return RndCapitalization::NotReported;
    }
    let years: Vec<Option<f64>> = (0..=RND_LIFE_YEARS)
        .map(|j| windows.get(4 * j).and_then(|w| w.research_and_development))
        .collect();
    let Some(years) = years.into_iter().collect::<Option<Vec<f64>>>() else {
        return RndCapitalization::Unavailable {
            reason: format!(
                "the {RND_LIFE_YEARS}-year life needs {} contiguous quarters of R&D",
                4 * (RND_LIFE_YEARS + 1)
            ),
        };
    };
    let life = RND_LIFE_YEARS as f64;
    let research_asset = (0..RND_LIFE_YEARS)
        .map(|j| years[j] * (life - j as f64) / life)
        .sum();
    let amortization = (1..=RND_LIFE_YEARS).map(|j| years[j] / life).sum();
    RndCapitalization::Capitalized { ttm_rnd: years[0], research_asset, amortization }
}

/// The CAPM cost of equity and the Blume-adjusted beta it priced: `DGS10` plus
/// (0.67β + 0.33) × the equity risk premium; `None` without a finite `DGS10` or a
/// served beta.
fn capm_cost_of_equity(fin: &CompanyFinancials, dgs10: f64) -> Option<(f64, f64)> {
    let beta = fin.beta.filter(|b| b.is_finite() && dgs10.is_finite())?;
    let adjusted_beta = BLUME_WEIGHT * beta + (1.0 - BLUME_WEIGHT);
    Some((dgs10 + adjusted_beta * EQUITY_RISK_PREMIUM, adjusted_beta))
}

/// The cost of capital at the run's `DGS10`: a CAPM cost of equity over the
/// Blume-adjusted beta and [`EQUITY_RISK_PREMIUM`], and an after-tax cost of debt
/// at [`DEBT_SPREAD_OVER_RF`] over the rate, weighted by market equity against
/// book debt.
fn cost_of_capital(
    fin: &CompanyFinancials,
    window: &TtmWindow,
    dgs10: f64,
    tax: f64,
) -> Result<CostOfCapital, &'static str> {
    if !dgs10.is_finite() {
        return Err("no usable DGS10 print");
    }
    let (cost_of_equity, adjusted_beta) = capm_cost_of_equity(fin, dgs10).ok_or("no served beta")?;
    let equity = fin
        .market_cap
        .filter(|m| m.is_finite() && *m > 0.0)
        .ok_or("no positive market capitalization")?;
    let debt = window
        .total_debt
        .filter(|d| d.is_finite() && *d >= 0.0)
        .ok_or("no balance-sheet debt line")?;
    let after_tax_cost_of_debt = (dgs10 + DEBT_SPREAD_OVER_RF) * (1.0 - tax);
    let equity_weight = equity / (equity + debt);
    Ok(CostOfCapital {
        rate: equity_weight * cost_of_equity + (1.0 - equity_weight) * after_tax_cost_of_debt,
        cost_of_equity,
        after_tax_cost_of_debt,
        equity_weight,
        adjusted_beta,
    })
}

/// A window's book invested capital (debt + equity − cash), before the research
/// asset; `None` where a line is missing.
fn book_invested_capital(window: &TtmWindow) -> Option<f64> {
    Some(window.total_debt? + window.total_equity? - window.cash?)
}

/// Population standard deviation; `None` below [`MOAT_MIN_WINDOWS`] values.
fn stability(values: &[f64]) -> Option<f64> {
    if values.len() < MOAT_MIN_WINDOWS {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    Some((values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt())
}

/// The value-creation read for one stock at the run's `DGS10` (a decimal ratio).
/// `sector` is the profile's sector label, read only for the gross-margin anchor.
pub fn value_creation_read(
    fin: &CompanyFinancials,
    sector: Option<&str>,
    dgs10: f64,
) -> ValueCreationRead {
    use ValueCreationLeg as Leg;
    let windows = ttm_windows(fin);
    let mut gaps = Vec::new();
    let cost_of_equity = capm_cost_of_equity(fin, dgs10).map(|(rate, _)| rate);
    let Some(w0) = windows.first() else {
        let reason = "no contiguous four-quarter income window";
        for leg in [
            Leg::Nopat,
            Leg::InvestedCapital,
            Leg::Roic,
            Leg::CostOfCapital,
            Leg::RoicSpread,
            Leg::OwnerEarnings,
            Leg::Reinvestment,
            Leg::Growth,
        ] {
            gap(&mut gaps, leg, reason);
        }
        return ValueCreationRead {
            tax_rate: None,
            rnd: if reports_rnd(fin) {
                RndCapitalization::Unavailable { reason: reason.to_string() }
            } else {
                RndCapitalization::NotReported
            },
            nopat: None,
            invested_capital: None,
            roic: None,
            cost_of_equity,
            cost_of_capital: None,
            roic_spread: None,
            owner_earnings: None,
            reinvestment_rate: None,
            growth: None,
            moat: MoatProxies::default(),
            gaps,
        };
    };

    let (tax, tax_basis) = tax_rate(w0);
    let rnd = capitalize_rnd(fin, &windows);

    let nopat = w0
        .operating_income
        .map(|ebit| ebit * (1.0 - tax) + rnd.net_investment());
    if nopat.is_none() {
        gap(&mut gaps, Leg::Nopat, "no TTM operating income");
    }

    let invested_capital = book_invested_capital(w0).map(|ic| ic + rnd.research_asset());
    if invested_capital.is_none() {
        gap(&mut gaps, Leg::InvestedCapital, "no balance-sheet debt, equity or cash line");
    }

    let roic = match (nopat, invested_capital) {
        (Some(n), Some(ic)) if ic > 0.0 => Some(n / ic),
        (Some(_), Some(_)) => {
            gap(&mut gaps, Leg::Roic, "non-positive invested capital");
            None
        }
        _ => {
            gap(&mut gaps, Leg::Roic, "no NOPAT or invested capital");
            None
        }
    };

    let cost_of_capital = match cost_of_capital(fin, w0, dgs10, tax) {
        Ok(c) => Some(c),
        Err(reason) => {
            gap(&mut gaps, Leg::CostOfCapital, reason);
            None
        }
    };

    let roic_spread = match (roic, &cost_of_capital) {
        (Some(r), Some(c)) => Some(r - c.rate),
        _ => {
            gap(&mut gaps, Leg::RoicSpread, "no ROIC or cost of capital");
            None
        }
    };

    let owner_earnings = w0.free_cash_flow.map(|fcf| fcf + rnd.net_investment());
    if owner_earnings.is_none() {
        gap(&mut gaps, Leg::OwnerEarnings, "no TTM free cash flow");
    }

    let reinvestment = match (w0.capex, w0.depreciation_and_amortization) {
        (Some(capex), Some(da)) => Some(capex - da + rnd.net_investment()),
        _ => None,
    };
    let reinvestment_rate = match (reinvestment, nopat) {
        (Some(r), Some(n)) if n > 0.0 => Some(r / n),
        (Some(_), Some(_)) => {
            gap(&mut gaps, Leg::Reinvestment, "non-positive NOPAT");
            None
        }
        (None, _) => {
            gap(&mut gaps, Leg::Reinvestment, "no TTM capex or D&A");
            None
        }
        (Some(_), None) => {
            gap(&mut gaps, Leg::Reinvestment, "no NOPAT");
            None
        }
    };
    let growth = match (roic, reinvestment_rate) {
        (Some(r), Some(rate)) => Some(r * rate),
        _ => {
            gap(&mut gaps, Leg::Growth, "no ROIC or reinvestment rate");
            None
        }
    };

    let margin = |w: &TtmWindow| match (w.gross_profit, w.revenue) {
        (Some(gp), Some(rev)) if rev > 0.0 => Some(gp / rev),
        _ => None,
    };
    let gross_margin = margin(w0);
    let margins: Vec<f64> = windows.iter().filter_map(margin).collect();
    let roic_persistence = cost_of_capital.as_ref().and_then(|c| {
        let unadjusted: Vec<f64> = windows
            .iter()
            .filter_map(|w| match (w.operating_income, book_invested_capital(w)) {
                (Some(ebit), Some(ic)) if ic > 0.0 => Some(ebit * (1.0 - tax) / ic),
                _ => None,
            })
            .collect();
        (unadjusted.len() >= MOAT_MIN_WINDOWS).then(|| {
            unadjusted.iter().filter(|r| **r > c.rate).count() as f64 / unadjusted.len() as f64
        })
    });
    let rnd_intensity = match (&rnd, w0.research_and_development, w0.revenue) {
        (RndCapitalization::NotReported, _, _) => None,
        (_, Some(r), Some(rev)) if rev > 0.0 => Some(r / rev),
        _ => None,
    };
    let moat = MoatProxies {
        gross_margin,
        gross_margin_vs_sector: gross_margin
            .map(|gm| gm - super::factors::sector_anchors(sector).gross_margin),
        gross_margin_stability: stability(&margins),
        roic_persistence,
        rnd_intensity,
    };

    ValueCreationRead {
        tax_rate: Some((tax, tax_basis)),
        rnd,
        nopat,
        invested_capital,
        roic,
        cost_of_equity,
        cost_of_capital,
        roic_spread,
        owner_earnings,
        reinvestment_rate,
        growth,
        moat,
        gaps,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::portfolio::engine::{
        DatedValue, QuarterlyBalanceSheetRow, QuarterlyCashFlowRow, QuarterlyIncomeRow,
    };

    /// `n` consecutive quarter ends, newest first, from 2026-06-30.
    pub(crate) fn quarter_ends(n: usize) -> Vec<String> {
        let mut ends = Vec::new();
        let (mut year, mut q) = (2026, 2);
        for _ in 0..n {
            let end = match q {
                1 => format!("{year}-03-31"),
                2 => format!("{year}-06-30"),
                3 => format!("{year}-09-30"),
                _ => format!("{year}-12-31"),
            };
            ends.push(end);
            q -= 1;
            if q == 0 {
                q = 4;
                year -= 1;
            }
        }
        ends
    }

    /// The plain fixture: sixteen identical quarters — revenue 100, gross profit
    /// 40, operating income 20, net income 15, R&D 10, pre-tax 18 against tax 3.6
    /// (a 20% effective rate); free cash flow 12 on capex 5 and D&A 4; total
    /// assets 1,000, debt 200, equity 500, cash 100; beta 1.2, market cap 4,000
    /// on 100 diluted shares.
    pub(crate) fn plain_name() -> CompanyFinancials {
        let ends = quarter_ends(16);
        CompanyFinancials {
            symbol: "PLN".into(),
            current_price: Some(40.0),
            market_cap: Some(4000.0),
            shares_outstanding: Some(100.0),
            beta: Some(1.2),
            quarterly_income: ends
                .iter()
                .map(|e| QuarterlyIncomeRow {
                    period_end: e.clone(),
                    revenue: Some(100.0),
                    cost_of_revenue: Some(60.0),
                    gross_profit: Some(40.0),
                    operating_income: Some(20.0),
                    net_income: Some(15.0),
                    research_and_development: Some(10.0),
                    income_before_tax: Some(18.0),
                    income_tax_expense: Some(3.6),
                    diluted_shares: Some(100.0),
                    ..Default::default()
                })
                .collect(),
            quarterly_cash_flow: ends
                .iter()
                .map(|e| QuarterlyCashFlowRow {
                    period_end: e.clone(),
                    free_cash_flow: Some(12.0),
                    operating_cash_flow: Some(17.0),
                    capex: Some(-5.0),
                    depreciation_and_amortization: Some(4.0),
                    ..Default::default()
                })
                .collect(),
            quarterly_balance_sheet: ends
                .iter()
                .map(|e| QuarterlyBalanceSheetRow {
                    period_end: e.clone(),
                    total_assets: Some(1000.0),
                    total_debt: Some(200.0),
                    total_equity: Some(500.0),
                    cash_and_equivalents: Some(100.0),
                    short_term_investments: Some(0.0),
                    ..Default::default()
                })
                .collect(),
            daily_closes: Vec::<DatedValue>::new(),
            ..Default::default()
        }
    }

    pub(crate) fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[track_caller]
    fn close_to(actual: Option<f64>, expected: f64) {
        let a = actual.unwrap_or_else(|| panic!("expected {expected}, got None"));
        assert!(approx(a, expected), "expected {expected}, got {a}");
    }

    #[test]
    fn windows_sum_four_quarters_and_read_the_instant_at_the_window_end() {
        let windows = ttm_windows(&plain_name());
        assert_eq!(windows.len(), 13, "sixteen quarters carry thirteen windows");
        let w0 = &windows[0];
        assert_eq!(w0.period_end, "2026-06-30");
        close_to(w0.revenue, 400.0);
        close_to(w0.gross_profit, 160.0);
        close_to(w0.free_cash_flow, 48.0);
        close_to(w0.capex, 20.0);
        close_to(w0.depreciation_and_amortization, 16.0);
        close_to(w0.total_assets, 1000.0);
        close_to(w0.cash, 100.0);
        // No filing date: the anchor is the period end plus the 45-day grace.
        assert_eq!(w0.anchor_date.as_deref(), Some("2026-08-14"));
        assert_eq!(windows[12].period_end, "2023-06-30");
    }

    #[test]
    fn windows_read_the_canonical_order_and_stop_at_a_skipped_quarter() {
        let mut shuffled = plain_name();
        shuffled.quarterly_income.reverse();
        shuffled.quarterly_cash_flow.reverse();
        assert_eq!(ttm_windows(&shuffled), ttm_windows(&plain_name()));

        // A restatement served twice: the later filing wins.
        let mut restated = plain_name();
        let mut row = restated.quarterly_income[0].clone();
        row.revenue = Some(130.0);
        row.filing_date = Some("2026-09-01".into());
        restated.quarterly_income[0].filing_date = Some("2026-08-01".into());
        restated.quarterly_income.push(row);
        close_to(ttm_windows(&restated)[0].revenue, 430.0);

        // A skipped quarter ends the run: quarters behind the gap are not read.
        let mut gapped = plain_name();
        gapped.quarterly_income.remove(6);
        assert_eq!(ttm_windows(&gapped).len(), 3, "six contiguous quarters carry three windows");
    }

    #[test]
    fn a_window_missing_a_line_on_any_quarter_reads_none_for_that_line() {
        let mut fin = plain_name();
        fin.quarterly_income[2].operating_income = None;
        fin.quarterly_cash_flow.remove(1);
        fin.quarterly_balance_sheet.remove(0);
        let w0 = &ttm_windows(&fin)[0];
        assert_eq!(w0.operating_income, None);
        assert_eq!(w0.free_cash_flow, None, "a quarter with no cash-flow row gaps the sum");
        assert_eq!(w0.total_assets, None, "no balance sheet at the window end");
        close_to(w0.revenue, 400.0);
    }

    #[test]
    fn the_plain_name_reads_every_leg() {
        let read = value_creation_read(&plain_name(), Some("Technology"), 0.04);
        assert!(read.gaps.is_empty(), "{:?}", read.gaps);
        let (tax, basis) = read.tax_rate.unwrap();
        assert!(approx(tax, 0.2));
        assert_eq!(basis, TaxRateBasis::Effective);
        // Flat R&D of 40 a year: the research asset is 40 + 40·2/3 + 40·1/3 = 80,
        // amortization (40 + 40 + 40) / 3 = 40 — no net adjustment to NOPAT.
        assert_eq!(
            read.rnd,
            RndCapitalization::Capitalized { ttm_rnd: 40.0, research_asset: 80.0, amortization: 40.0 }
        );
        close_to(read.nopat, 64.0); // 80 × (1 − 0.2) + (40 − 40)
        close_to(read.invested_capital, 680.0); // 200 + 500 − 100 + 80
        close_to(read.roic, 64.0 / 680.0);
        let coc = read.cost_of_capital.clone().unwrap();
        let adjusted_beta = 0.67 * 1.2 + 0.33;
        let ke = 0.04 + adjusted_beta * 0.0425;
        let kd = (0.04 + 0.015) * 0.8;
        let we = 4000.0 / 4200.0;
        assert!(approx(coc.adjusted_beta, adjusted_beta));
        assert!(approx(coc.cost_of_equity, ke));
        assert!(approx(coc.after_tax_cost_of_debt, kd));
        assert!(approx(coc.equity_weight, we));
        assert!(approx(coc.rate, we * ke + (1.0 - we) * kd));
        close_to(read.roic_spread, 64.0 / 680.0 - coc.rate);
        close_to(read.owner_earnings, 48.0); // FCF 48 + (40 − 40)
        close_to(read.reinvestment_rate, (20.0 - 16.0) / 64.0);
        close_to(read.growth, 64.0 / 680.0 * (4.0 / 64.0));
        close_to(read.moat.gross_margin, 0.4);
        close_to(read.moat.gross_margin_vs_sector, 0.4 - 0.538);
        close_to(read.moat.gross_margin_stability, 0.0);
        // Unadjusted ROIC 80 × 0.8 / 600 ≈ 10.7% clears the ≈ 8.6% cost of
        // capital in every one of the thirteen windows.
        close_to(read.moat.roic_persistence, 1.0);
        close_to(read.moat.rnd_intensity, 0.1);
    }

    #[test]
    fn capitalizing_rising_rnd_lifts_roic_over_the_unadjusted_read() {
        // R&D of 50 / 40 / 30 / 20 a quarter across the four years: annual
        // 200, 160, 120, 80, newest first.
        let mut fin = plain_name();
        for (i, row) in fin.quarterly_income.iter_mut().enumerate() {
            row.research_and_development = Some([50.0, 40.0, 30.0, 20.0][i / 4]);
        }
        let read = value_creation_read(&fin, Some("Technology"), 0.04);
        let asset = 200.0 + 160.0 * 2.0 / 3.0 + 120.0 / 3.0;
        let amortization = (160.0 + 120.0 + 80.0) / 3.0;
        let RndCapitalization::Capitalized { ttm_rnd, research_asset, amortization: amort } =
            read.rnd.clone()
        else {
            panic!("{:?}", read.rnd);
        };
        assert!(approx(ttm_rnd, 200.0));
        assert!(approx(research_asset, asset));
        assert!(approx(amort, amortization));
        let nopat = 80.0 * 0.8 + (200.0 - amortization);
        close_to(read.nopat, nopat);
        close_to(read.roic, nopat / (600.0 + asset));
        let unadjusted = 80.0 * 0.8 / 600.0;
        assert!(read.roic.unwrap() > unadjusted, "{:?} vs {unadjusted}", read.roic);
        close_to(read.owner_earnings, 48.0 + (200.0 - amortization));
    }

    #[test]
    fn rnd_without_the_lifes_history_reads_unadjusted_and_says_why() {
        let mut fin = plain_name();
        for row in fin.quarterly_income.iter_mut().skip(8) {
            row.research_and_development = None;
        }
        let read = value_creation_read(&fin, None, 0.04);
        assert!(matches!(read.rnd, RndCapitalization::Unavailable { .. }), "{:?}", read.rnd);
        close_to(read.nopat, 64.0);
        close_to(read.invested_capital, 600.0);
        close_to(read.moat.rnd_intensity, 0.1);
    }

    #[test]
    fn a_name_reporting_no_rnd_line_takes_no_adjustment() {
        let mut fin = plain_name();
        for row in &mut fin.quarterly_income {
            row.research_and_development = None;
        }
        let read = value_creation_read(&fin, None, 0.04);
        assert_eq!(read.rnd, RndCapitalization::NotReported);
        close_to(read.invested_capital, 600.0);
        assert_eq!(read.moat.rnd_intensity, None);
        // Unmapped sector: the gross margin compares against the market-wide row.
        close_to(read.moat.gross_margin_vs_sector, 0.4 - 0.344);
    }

    #[test]
    fn an_all_zero_rnd_line_reads_not_reported_on_any_history_depth() {
        // The feed serves 0 for a name with no R&D: no capitalization, no
        // intensity, and no "needs sixteen quarters" reason on short history.
        let mut fin = plain_name();
        for row in &mut fin.quarterly_income {
            row.research_and_development = Some(0.0);
        }
        let full = value_creation_read(&fin, None, 0.04);
        assert_eq!(full.rnd, RndCapitalization::NotReported);
        assert_eq!(full.moat.rnd_intensity, None);
        close_to(full.invested_capital, 600.0);
        fin.quarterly_income.truncate(8);
        assert_eq!(value_creation_read(&fin, None, 0.04).rnd, RndCapitalization::NotReported);
        // With no window at all, the label still tells a name with no R&D from
        // one whose R&D couldn't be read.
        let mut no_window = plain_name();
        no_window.quarterly_income.truncate(3);
        assert!(matches!(
            value_creation_read(&no_window, None, 0.04).rnd,
            RndCapitalization::Unavailable { .. }
        ));
        for row in &mut no_window.quarterly_income {
            row.research_and_development = Some(0.0);
        }
        assert_eq!(value_creation_read(&no_window, None, 0.04).rnd, RndCapitalization::NotReported);
    }

    #[test]
    fn a_bank_shaped_name_reads_its_unbuildable_legs_absent() {
        // No gross profit or cost of revenue, no operating income, no R&D line,
        // no capex or D&A — revenue, net income and free cash flow only.
        let mut fin = plain_name();
        for row in &mut fin.quarterly_income {
            row.gross_profit = None;
            row.cost_of_revenue = None;
            row.operating_income = None;
            row.research_and_development = None;
        }
        for row in &mut fin.quarterly_cash_flow {
            row.capex = None;
            row.depreciation_and_amortization = None;
        }
        let read = value_creation_read(&fin, Some("Financial Services"), 0.04);
        assert_eq!(read.rnd, RndCapitalization::NotReported);
        assert_eq!(read.nopat, None);
        assert_eq!(read.roic, None);
        assert_eq!(read.roic_spread, None);
        assert_eq!(read.reinvestment_rate, None);
        assert_eq!(read.growth, None);
        assert_eq!(read.moat.gross_margin, None);
        assert_eq!(read.moat.gross_margin_vs_sector, None);
        assert_eq!(read.moat.rnd_intensity, None);
        close_to(read.owner_earnings, 48.0);
        assert!(read.cost_of_capital.is_some(), "the cost of capital needs no income line");
        let legs: Vec<_> = read.gaps.iter().map(|g| g.leg).collect();
        for leg in [
            ValueCreationLeg::Nopat,
            ValueCreationLeg::Roic,
            ValueCreationLeg::RoicSpread,
            ValueCreationLeg::Reinvestment,
            ValueCreationLeg::Growth,
        ] {
            assert!(legs.contains(&leg), "{leg:?} missing from {legs:?}");
        }
    }

    #[test]
    fn the_cost_of_capital_needs_a_beta_a_rate_and_a_debt_line() {
        let mut fin = plain_name();
        fin.beta = None;
        let read = value_creation_read(&fin, None, 0.04);
        assert_eq!(read.cost_of_capital, None);
        assert_eq!(read.roic_spread, None);
        assert_eq!(read.moat.roic_persistence, None);
        assert!(read.roic.is_some(), "ROIC stands without a cost of capital");
        assert!(read
            .gaps
            .iter()
            .any(|g| g.leg == ValueCreationLeg::CostOfCapital && g.reason == "no served beta"));
        let read = value_creation_read(&plain_name(), None, f64::NAN);
        assert_eq!(read.cost_of_capital, None);
        // Zero debt is all-equity, never a gap.
        let mut fin = plain_name();
        for row in &mut fin.quarterly_balance_sheet {
            row.total_debt = Some(0.0);
        }
        let coc = value_creation_read(&fin, None, 0.04).cost_of_capital.unwrap();
        assert!(approx(coc.equity_weight, 1.0));
        assert!(approx(coc.rate, coc.cost_of_equity));
    }

    #[test]
    fn the_cost_of_equity_stands_without_the_debt_line_the_wacc_needs() {
        let mut fin = plain_name();
        for row in &mut fin.quarterly_balance_sheet {
            row.total_debt = None;
        }
        let read = value_creation_read(&fin, None, 0.04);
        assert_eq!(read.cost_of_capital, None);
        close_to(read.cost_of_equity, 0.04 + (0.67 * 1.2 + 0.33) * 0.0425);
        // No window at all: the cost of equity still reads off the beta and rate.
        let mut bare = plain_name();
        bare.quarterly_income.clear();
        assert!(value_creation_read(&bare, None, 0.04).cost_of_equity.is_some());
        let mut no_beta = plain_name();
        no_beta.beta = None;
        assert_eq!(value_creation_read(&no_beta, None, 0.04).cost_of_equity, None);
        assert_eq!(value_creation_read(&plain_name(), None, f64::NAN).cost_of_equity, None);
    }

    #[test]
    fn an_out_of_range_or_loss_year_tax_rate_takes_the_statutory_rate() {
        let mut fin = plain_name();
        for row in &mut fin.quarterly_income {
            row.income_tax_expense = Some(9.0); // 50% of 18
        }
        assert_eq!(
            value_creation_read(&fin, None, 0.04).tax_rate,
            Some((STATUTORY_TAX_RATE, TaxRateBasis::Statutory))
        );
        for row in &mut fin.quarterly_income {
            row.income_before_tax = Some(-5.0);
        }
        assert_eq!(
            value_creation_read(&fin, None, 0.04).tax_rate,
            Some((STATUTORY_TAX_RATE, TaxRateBasis::Statutory))
        );
    }

    #[test]
    fn non_positive_invested_capital_leaves_roic_absent() {
        let mut fin = plain_name();
        for row in &mut fin.quarterly_balance_sheet {
            row.total_equity = Some(-900.0);
        }
        let read = value_creation_read(&fin, None, 0.04);
        assert_eq!(read.roic, None);
        assert!(read
            .gaps
            .iter()
            .any(|g| g.leg == ValueCreationLeg::Roic && g.reason == "non-positive invested capital"));
    }

    #[test]
    fn no_income_window_reads_every_leg_absent() {
        let fin = CompanyFinancials { beta: Some(1.0), market_cap: Some(1.0), ..Default::default() };
        let read = value_creation_read(&fin, None, 0.04);
        assert_eq!(read.tax_rate, None);
        assert_eq!(read.roic, None);
        assert_eq!(read.cost_of_capital, None);
        assert_eq!(read.gaps.len(), 8);
    }
}
