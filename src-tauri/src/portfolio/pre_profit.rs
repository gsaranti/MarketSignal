//! The **pre-profit execution / financing overlay** (`docs/portfolio-analysis.md`
//! §Starting parameters; `docs/portfolio-workflow.md` §Step 6b). A priced stock
//! that is not yet operating-profitable — or has no positive forward-EPS consensus
//! while burning cash — carries a deterministic financing read from its
//! statements: runway, margin progression, capital intensity, and dilution.
//!
//! The overlay is **conviction / risk / action context only** — never another grade
//! component, and never a license for the model to calculate a number: the engine
//! computes the statement legs, their states, and the rule consequences; the rule
//! consequences bind the engine arm (its own rung and feasible set observe them),
//! the model interpreting the evidence unrestricted, departures annotated.
//!
//! The **execution read** — the issuer's guidance against its delivered results —
//! has **no deterministic producer**: the research reports the issuer's operating
//! observations as dated, sourced prose, the engine computes no attainment from
//! them, and the execution leg types `unscorable` and enters no conjunction.
//! Severe deterioration reads from the statement legs alone.

use serde::{Deserialize, Serialize};

use crate::portfolio::engine::CompanyFinancials;

// ---- Calibration surface (NOT pinned — shadow-tune against live runs;
//      `docs/portfolio-analysis.md` §Starting parameters, all drafted) ----------

/// Financing-state runway bands, in months: `adequate` at or above 24, `watch` at
/// 12–<24, `constrained` below 12 (`not_burning` when TTM burn is zero,
/// `unscorable` when a required input is absent).
const RUNWAY_ADEQUATE_MONTHS: f64 = 24.0;
const RUNWAY_WATCH_MONTHS: f64 = 12.0;

/// Material dilution: split-adjusted diluted shares up at least 15% year over year.
const MATERIAL_DILUTION_YOY: f64 = 0.15;

/// Economics deterioration: the latest two-quarter average gross margin non-positive
/// AND at least 5 percentage points below the preceding two-quarter average.
const ECONOMICS_MARGIN_DROP_PP: f64 = 0.05;

/// The overlay's parameter version, stamped on every persisted overlay record so a
/// retune — or a rule correction that changes what a record means — stays
/// attributable (the suite's shared versioning discipline), and the checkpoint
/// resume gate refuses a trail stamped under another. `pre-profit-v2`: the
/// backfill obligation counts comparable (bound + actual) periods, so a v1
/// overlay's absent backfill attempt is not read as a v2 waiver.
/// `pre-profit-v3`: the guidance vintage policy (the 2026-08-24 review's
/// Codex I4) — the execution read pairs an actual only against ex-ante
/// guidance (dated on or before the period end and strictly before the
/// period's earliest actual), the latest such revision binding, and a
/// same-vintage conflict on either side drops the period; a v2 read could
/// pair a results release's restated guidance against its own actual and
/// selected among revisions by persistence order, so a v2 record's
/// execution read does not mean what a v3 read means. `pre-profit-v4`: the
/// reporting span is part of the comparison identity, so a full-year or
/// half-year bound can never attain against a quarter ending on the same day,
/// nor can unlike spans discharge one another's backfill depth.
/// `pre-profit-v5`: the conviction ceilings are gone — the engine authors no
/// conviction, so a repeated execution miss alone matches no rule and severe
/// deterioration binds the add-family bar and the exit-family-only rule
/// alone; a v4 record's `conviction_ceiling` field does not exist on this
/// shape. `pre-profit-v6`: the execution leg types `unscorable` — no producer
/// derives an attainment read from the observation history — and severe
/// deterioration reads from the statement legs alone (economics deterioration
/// plus constrained runway or material dilution), so a v5 record's execution
/// read and its severe state do not mean what a v6 record's mean.
pub const PRE_PROFIT_PARAMETER_VERSION: &str = "pre-profit-v6";

/// Boundary slack on the computed-ratio threshold tests: a value exactly on a
/// documented boundary (a 15% YoY share rise, a 20% miss) can evaluate a few ULPs
/// below its constant (`115.0 / 100.0 − 1.0 < 0.15` in f64), and the documented
/// rules say "at least" — so each ratio test allows this tolerance rather than
/// letting rounding decide a calibration boundary.
const BOUNDARY_EPS: f64 = 1e-9;

/// `value ≥ threshold`, tolerant of float rounding at the documented boundary.
fn at_least(value: f64, threshold: f64) -> bool {
    value >= threshold - BOUNDARY_EPS
}

// ---- Eligibility ---------------------------------------------------------------

/// Whether the stock enters the overlay (`docs/portfolio-analysis.md` §Starting
/// parameters): **TTM operating income ≤ 0**, or **no positive forward-EPS
/// consensus AND TTM free cash flow < 0**. Funds and `role_risk_only` holdings
/// never enter. When the eligibility inputs themselves are missing the holding is
/// **not entered** and the gap is recorded (`unscorable`), so no consequence
/// machinery fires off absent data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum PreProfitEligibility {
    Eligible { reasons: Vec<String> },
    NotEligible,
    Unscorable { missing: Vec<String> },
}

// ---- Statement-derived inputs ---------------------------------------------------

/// The structured (statement) leg the engine computes at Step 6b — every field
/// `None` when its inputs were missing, recorded in the overlay's unscorable gaps
/// rather than fabricated (`docs/portfolio-workflow.md` §Step 6b).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StatementInputs {
    /// TTM operating income (four newest quarterly prints summed) — eligibility arm 1.
    pub ttm_operating_income: Option<f64>,
    /// TTM free cash flow (reported line first, else derived OCF − |capex| per row).
    pub ttm_free_cash_flow: Option<f64>,
    /// Whether a finite positive forward-EPS consensus exists (the driver ladder's
    /// rung-1 test) — eligibility arm 2 reads its absence.
    pub has_positive_eps_consensus: bool,
    /// Liquid resources = cash and cash equivalents + short-term investments (an
    /// absent short-term-investments line reads as zero; absent cash is a gap).
    pub liquid_resources: Option<f64>,
    /// TTM cash burn = max(0, −TTM free cash flow).
    pub ttm_cash_burn: Option<f64>,
    /// Runway months = 12 × liquid resources ÷ TTM cash burn (`None` when not
    /// burning or unscorable).
    pub runway_months: Option<f64>,
    /// TTM |capex| ÷ TTM revenue — context only; no rule consumes it.
    pub ttm_capex_intensity: Option<f64>,
    /// Split-adjusted year-over-year diluted-share change (the statement feed's
    /// share counts are retroactively split-adjusted — verified against NVDA's
    /// 2024 10:1 split, 2026-08-03: all 16 quarters read on the post-split basis).
    pub diluted_share_change_yoy: Option<f64>,
    /// The latest two-quarter average gross margin…
    pub gross_margin_recent_2q: Option<f64>,
    /// …and its change from the preceding two-quarter average (decimal points).
    pub gross_margin_change_2q: Option<f64>,
}

// ---- Derived states --------------------------------------------------------------

/// The financing state over the runway bands (`docs/portfolio-analysis.md`
/// §Starting parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FinancingState {
    NotBurning,
    Adequate,
    Watch,
    Constrained,
    /// Runway could not be computed — the default so an empty record never
    /// fabricates a state.
    #[default]
    Unscorable,
}

/// The overlay's execution leg — the issuer's guidance against its delivered
/// results. It has **no deterministic producer** (`docs/portfolio-analysis.md`
/// §Starting parameters): the research reports the issuer's operating
/// observations as prose, the engine computes no attainment, so the leg carries
/// its one state and enters no conjunction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ExecutionLeg {
    #[default]
    Unscorable,
}

/// The overlay's deterministic rule consequences — separately attributed from the
/// forensic rules (`docs/portfolio-analysis.md` §Starting parameters): constrained
/// runway → the add-family bar; severe deterioration → the add-family bar and the
/// exit-family-only action rule. Each binds the **engine arm** (its own action
/// rung and feasible set observe them); the model's conviction and rung are
/// unrestricted, with departures recorded as annotations. The engine caps no
/// conviction: it authors none.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverlayConsequences {
    pub bar_add_family: bool,
    /// Severe deterioration's exit-family-only rule ({trim, sell all}) — it
    /// binds the engine arm's own rung and feasible set and renders as an engine
    /// rule; the model's rung is unrestricted, departures annotated.
    pub exit_family_only: bool,
    /// The engine-matched rules, recorded so a clamped value is reconstructable
    /// (the audit's matched-cap-rule leg).
    pub matched_rules: Vec<String>,
}

/// The complete persisted overlay record — computed for **every priced stock** (the
/// eligibility result persists even when the stock does not enter), carried on the
/// holding's audit row (`docs/storage.md §Local Analysis Suite Storage`). States and
/// consequences are meaningful only under `eligibility == Eligible`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreProfitOverlay {
    pub eligibility: PreProfitEligibility,
    pub statement_inputs: StatementInputs,
    pub financing_state: FinancingState,
    /// The execution leg, `unscorable` on every record — it has no producer.
    pub execution: ExecutionLeg,
    /// `None` = the margin legs were unscorable.
    pub economics_deterioration: Option<bool>,
    /// `None` = the dilution leg was unscorable.
    pub material_dilution: Option<bool>,
    /// Economics deterioration plus at least one of constrained runway and
    /// material dilution — statement legs alone; financing plus dilution without
    /// the economics leg cannot manufacture it, and the execution leg enters no
    /// conjunction.
    pub severe_deterioration: bool,
    pub consequences: OverlayConsequences,
    pub unscorable_gaps: Vec<String>,
    pub parameter_version: String,
}

impl PreProfitOverlay {
    pub fn is_eligible(&self) -> bool {
        matches!(self.eligibility, PreProfitEligibility::Eligible { .. })
    }
}

// ---- Computation -----------------------------------------------------------------

/// Compute the overlay for a priced stock: the statement leg, eligibility, and —
/// when eligible — the derived statement states and rule consequences.
pub fn compute_overlay(fin: &CompanyFinancials) -> PreProfitOverlay {
    let mut gaps: Vec<String> = Vec::new();
    let inputs = statement_inputs(fin, &mut gaps);
    let eligibility = eligibility(&inputs);

    let eligible = matches!(eligibility, PreProfitEligibility::Eligible { .. });
    let (financing_state, economics_deterioration, material_dilution) = if eligible {
        (
            financing_state(&inputs),
            economics_deterioration(&inputs),
            material_dilution(&inputs),
        )
    } else {
        (FinancingState::Unscorable, None, None)
    };

    let severe = eligible
        && severe_deterioration(financing_state, economics_deterioration, material_dilution);
    let consequences = if eligible {
        derive_consequences(financing_state, severe)
    } else {
        OverlayConsequences::default()
    };

    PreProfitOverlay {
        eligibility,
        statement_inputs: inputs,
        financing_state,
        execution: ExecutionLeg::Unscorable,
        economics_deterioration,
        material_dilution,
        severe_deterioration: severe,
        consequences,
        unscorable_gaps: gaps,
        parameter_version: PRE_PROFIT_PARAMETER_VERSION.to_string(),
    }
}

/// The statement leg (`docs/portfolio-workflow.md` §Step 6b): every value from
/// comparable quarterly statements, every missing input a recorded gap. The rows
/// are canonicalized first — sorted newest-first and deduplicated by period end,
/// the shared statement policy (`engine::canonicalize_statements`) held here
/// locally so the overlay stays order-independent standalone — so an
/// out-of-order or duplicated feed response cannot shift the TTM / YoY / 2q
/// windows.
fn statement_inputs(fin: &CompanyFinancials, gaps: &mut Vec<String>) -> StatementInputs {
    // Sort newest-first with the latest filing winning a duplicated period (a
    // restatement served twice must resolve to the restated print, never to wire
    // order); `dedup_by` keeps the first of equal periods. The residual — equal
    // period AND equal/absent filing dates with different values — falls back to
    // first-served, the TTM basis's existing behavior.
    let mut income: Vec<&crate::portfolio::engine::QuarterlyIncomeRow> =
        fin.quarterly_income.iter().collect();
    income.sort_by(|a, b| {
        b.period_end
            .cmp(&a.period_end)
            .then_with(|| b.filing_date.cmp(&a.filing_date))
    });
    income.dedup_by(|a, b| a.period_end == b.period_end);
    let income = &income;
    let mut cash_flow: Vec<&crate::portfolio::engine::QuarterlyCashFlowRow> =
        fin.quarterly_cash_flow.iter().collect();
    cash_flow.sort_by(|a, b| {
        b.period_end
            .cmp(&a.period_end)
            .then_with(|| b.filing_date.cmp(&a.filing_date))
    });
    cash_flow.dedup_by(|a, b| a.period_end == b.period_end);
    let cash_flow = &cash_flow;

    // Fixed-width windows are honest only over consecutive quarters — a feed
    // gap would silently stretch a "TTM" (or misdate the YoY pair) rather than
    // fail it, so each window is gated on contiguity and degrades to the same
    // unscorable-gap path a missing print takes.
    let income4_ok = income.len() >= 4
        && crate::portfolio::engine::quarters_contiguous(
            income[..4].iter().map(|r| r.period_end.as_str()),
        );
    let income5_ok = income.len() >= 5
        && crate::portfolio::engine::quarters_contiguous(
            income[..5].iter().map(|r| r.period_end.as_str()),
        );
    let cash4_ok = cash_flow.len() >= 4
        && crate::portfolio::engine::quarters_contiguous(
            cash_flow[..4].iter().map(|r| r.period_end.as_str()),
        );

    // TTM sums over the four newest quarters — `None` unless all four carry the line
    // (a partial sum would misstate the trailing year).
    let ttm_operating_income: Option<f64> = income4_ok
        .then(|| income[..4].iter().map(|r| r.operating_income).sum())
        .flatten();
    if ttm_operating_income.is_none() {
        gaps.push(
            "pre-profit: TTM operating income unscorable (missing or non-contiguous quarterly prints)"
                .into(),
        );
    }

    let ttm_free_cash_flow: Option<f64> = cash4_ok
        .then(|| {
            cash_flow[..4]
                .iter()
                .map(|r| r.resolved_free_cash_flow())
                .sum()
        })
        .flatten();
    if ttm_free_cash_flow.is_none() {
        gaps.push(
            "pre-profit: TTM free cash flow unscorable (missing or non-contiguous cash-flow prints)"
                .into(),
        );
    }

    let has_positive_eps_consensus = fin
        .consensus
        .as_ref()
        .and_then(|c| c.eps_mid)
        .filter(|m| m.is_finite() && *m > 0.0)
        .is_some();

    // Liquid resources: cash required; an absent short-term-investments line reads
    // as zero (a genuinely-zero and an unreported STI line are indistinguishable at
    // the adapter — the convention is recorded here, not silently).
    let liquid_resources = fin
        .cash_and_equivalents
        .map(|c| c + fin.short_term_investments.unwrap_or(0.0));
    if liquid_resources.is_none() {
        gaps.push("pre-profit: liquid resources unscorable (no cash line)".into());
    }

    let ttm_cash_burn = ttm_free_cash_flow.map(|f| (-f).max(0.0));
    let runway_months = match (liquid_resources, ttm_cash_burn) {
        (Some(liquid), Some(burn)) if burn > 0.0 => Some(12.0 * liquid / burn),
        _ => None,
    };

    let ttm_capex: Option<f64> = cash4_ok
        .then(|| {
            cash_flow[..4]
                .iter()
                .map(|r| r.capex.map(f64::abs))
                .sum::<Option<f64>>()
        })
        .flatten();
    let ttm_revenue: Option<f64> = income4_ok
        .then(|| income[..4].iter().map(|r| r.revenue).sum())
        .flatten();
    // The one CROSS-statement ratio: numerator (cash-flow window) and
    // denominator (income window) must cover the SAME trailing year — each
    // window is internally contiguous, but a feed serving cash flow one
    // quarter behind income would divide mismatched periods. Matching newest
    // period-ends on two 4-contiguous windows aligns them whole.
    let windows_aligned = income4_ok
        && cash4_ok
        && income[0].period_end == cash_flow[0].period_end;
    let ttm_capex_intensity = match (ttm_capex, ttm_revenue) {
        (Some(capex), Some(rev)) if rev > 0.0 && windows_aligned => Some(capex / rev),
        _ => None,
    };

    // Split-adjusted YoY diluted-share change: newest quarter vs the same fiscal
    // quarter one year back (index 4, newest-first — index arithmetic that is
    // only a year apart when rows 0..=4 are contiguous). The feed's share counts
    // are retroactively split-adjusted (NVDA 10:1 verified 2026-08-03).
    let diluted_share_change_yoy = match (
        income5_ok.then(|| income[0].diluted_shares).flatten(),
        income5_ok.then(|| income[4].diluted_shares).flatten(),
    ) {
        (Some(now), Some(prior)) if prior > 0.0 => Some(now / prior - 1.0),
        _ => None,
    };
    if diluted_share_change_yoy.is_none() {
        gaps.push("pre-profit: YoY diluted-share change unscorable".into());
    }

    // Two-quarter average gross margins: quarters 0–1 vs 2–3, each quarter's margin
    // from gross profit (or revenue − cost of revenue) over positive revenue.
    let quarter_margin = |r: &crate::portfolio::engine::QuarterlyIncomeRow| -> Option<f64> {
        let rev = r.revenue.filter(|v| *v > 0.0)?;
        let gp = r.gross_profit.or(match (r.revenue, r.cost_of_revenue) {
            (Some(rev), Some(cor)) => Some(rev - cor),
            _ => None,
        })?;
        Some(gp / rev)
    };
    let two_q_avg = |a: usize, b: usize| -> Option<f64> {
        match (
            income.get(a).and_then(|r| quarter_margin(r)),
            income.get(b).and_then(|r| quarter_margin(r)),
        ) {
            (Some(x), Some(y)) => Some((x + y) / 2.0),
            _ => None,
        }
    };
    // The 2q-vs-2q progression compares quarters 0–1 against 2–3, so it needs
    // the same contiguous four-row run the TTM sums verified.
    let gross_margin_recent_2q = income4_ok.then(|| two_q_avg(0, 1)).flatten();
    let gross_margin_preceding_2q = income4_ok.then(|| two_q_avg(2, 3)).flatten();
    let gross_margin_change_2q = match (gross_margin_recent_2q, gross_margin_preceding_2q) {
        (Some(recent), Some(prec)) => Some(recent - prec),
        _ => None,
    };
    if gross_margin_change_2q.is_none() {
        gaps.push("pre-profit: two-quarter gross-margin progression unscorable".into());
    }

    StatementInputs {
        ttm_operating_income,
        ttm_free_cash_flow,
        has_positive_eps_consensus,
        liquid_resources,
        ttm_cash_burn,
        runway_months,
        ttm_capex_intensity,
        diluted_share_change_yoy,
        gross_margin_recent_2q,
        gross_margin_change_2q,
    }
}

/// The eligibility rule over the statement inputs. Arm 2's consensus leg is always
/// computable (a present-and-positive consensus decisively closes the arm); only a
/// missing TTM free cash flow can leave it open.
fn eligibility(inputs: &StatementInputs) -> PreProfitEligibility {
    let arm_operating = inputs.ttm_operating_income.map(|v| v <= 0.0);
    let arm_burn = if inputs.has_positive_eps_consensus {
        Some(false)
    } else {
        inputs.ttm_free_cash_flow.map(|v| v < 0.0)
    };

    let mut reasons = Vec::new();
    if arm_operating == Some(true) {
        reasons.push("TTM operating income non-positive".to_string());
    }
    if arm_burn == Some(true) {
        reasons.push("no positive forward-EPS consensus and negative TTM free cash flow".to_string());
    }
    if !reasons.is_empty() {
        return PreProfitEligibility::Eligible { reasons };
    }

    let mut missing = Vec::new();
    if arm_operating.is_none() {
        missing.push("TTM operating income".to_string());
    }
    if arm_burn.is_none() {
        missing.push("TTM free cash flow".to_string());
    }
    if !missing.is_empty() {
        PreProfitEligibility::Unscorable { missing }
    } else {
        PreProfitEligibility::NotEligible
    }
}

/// The financing state over the runway bands.
fn financing_state(inputs: &StatementInputs) -> FinancingState {
    let Some(burn) = inputs.ttm_cash_burn else {
        return FinancingState::Unscorable;
    };
    if burn == 0.0 {
        return FinancingState::NotBurning;
    }
    match inputs.runway_months {
        Some(months) if at_least(months, RUNWAY_ADEQUATE_MONTHS) => FinancingState::Adequate,
        Some(months) if at_least(months, RUNWAY_WATCH_MONTHS) => FinancingState::Watch,
        Some(_) => FinancingState::Constrained,
        None => FinancingState::Unscorable,
    }
}

/// Economics deterioration: recent two-quarter average gross margin non-positive
/// AND ≥ 5pp below the preceding two-quarter average; `None` when unscorable.
fn economics_deterioration(inputs: &StatementInputs) -> Option<bool> {
    match (inputs.gross_margin_recent_2q, inputs.gross_margin_change_2q) {
        (Some(recent), Some(change)) => {
            Some(recent <= 0.0 && at_least(-change, ECONOMICS_MARGIN_DROP_PP))
        }
        _ => None,
    }
}

/// Material dilution: split-adjusted diluted shares up ≥ 15% YoY; `None` unscorable.
fn material_dilution(inputs: &StatementInputs) -> Option<bool> {
    inputs
        .diluted_share_change_yoy
        .map(|change| at_least(change, MATERIAL_DILUTION_YOY))
}

/// Severe deterioration, statement legs alone: economics deterioration plus at
/// least one of constrained runway and material dilution. Financing plus dilution
/// without the economics leg cannot manufacture it, and the execution leg —
/// unscorable, having no producer — enters no conjunction.
fn severe_deterioration(
    financing: FinancingState,
    economics: Option<bool>,
    dilution: Option<bool>,
) -> bool {
    economics == Some(true) && (financing == FinancingState::Constrained || dilution == Some(true))
}

/// The deterministic rule consequences (`docs/portfolio-analysis.md` §Starting
/// parameters) — the constrained-runway add-family bar and the
/// severe-deterioration exit restriction, each binding the engine's own rung.
fn derive_consequences(financing: FinancingState, severe: bool) -> OverlayConsequences {
    let mut c = OverlayConsequences::default();
    if financing == FinancingState::Constrained {
        c.bar_add_family = true;
        c.matched_rules
            .push("constrained-runway → add family barred".to_string());
    }
    if severe {
        c.bar_add_family = true;
        c.exit_family_only = true;
        c.matched_rules.push(
            "severe-deterioration → engine-arm rules: add family barred, exit family \
             only {trim, sell all}"
                .to_string(),
        );
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::engine::{
        CompanyFinancials, ConsensusEstimate, QuarterlyCashFlowRow, QuarterlyIncomeRow,
    };

    /// A quarterly income row with the overlay-relevant lines set.
    fn income_row(
        period: &str,
        operating_income: Option<f64>,
        revenue: Option<f64>,
        gross_profit: Option<f64>,
        diluted_shares: Option<f64>,
    ) -> QuarterlyIncomeRow {
        QuarterlyIncomeRow {
            period_end: period.to_string(),
            operating_income,
            revenue,
            gross_profit,
            diluted_shares,
            ..Default::default()
        }
    }

    fn cash_row(period: &str, fcf: Option<f64>) -> QuarterlyCashFlowRow {
        QuarterlyCashFlowRow {
            period_end: period.to_string(),
            free_cash_flow: fcf,
            ..Default::default()
        }
    }

    /// A burning pre-profit stock: negative TTM operating income and FCF, cash on
    /// hand, flat-ish margins, 8 quarters of prints.
    fn burning_stock() -> CompanyFinancials {
        let periods = [
            "2026-06-30",
            "2026-03-31",
            "2025-12-31",
            "2025-09-30",
            "2025-06-30",
            "2025-03-31",
            "2024-12-31",
            "2024-09-30",
        ];
        CompanyFinancials {
            symbol: "BURN".into(),
            quarterly_income: periods
                .iter()
                .map(|p| {
                    income_row(p, Some(-50.0e6), Some(100.0e6), Some(20.0e6), Some(100.0e6))
                })
                .collect(),
            quarterly_cash_flow: periods.iter().map(|p| cash_row(p, Some(-40.0e6))).collect(),
            cash_and_equivalents: Some(200.0e6),
            short_term_investments: Some(120.0e6),
            consensus: None,
            ..Default::default()
        }
    }

    // ---- Eligibility ----

    #[test]
    fn eligible_on_negative_operating_income() {
        let overlay = compute_overlay(&burning_stock());
        assert!(overlay.is_eligible());
        match &overlay.eligibility {
            PreProfitEligibility::Eligible { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("operating income")), "{reasons:?}");
            }
            other => panic!("expected eligible, got {other:?}"),
        }
    }

    #[test]
    fn eligible_on_burn_arm_without_consensus() {
        let mut fin = burning_stock();
        // Positive operating income closes arm 1; no consensus + negative FCF
        // keeps arm 2 open.
        for row in &mut fin.quarterly_income {
            row.operating_income = Some(10.0e6);
        }
        let overlay = compute_overlay(&fin);
        match &overlay.eligibility {
            PreProfitEligibility::Eligible { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("free cash flow")), "{reasons:?}");
            }
            other => panic!("expected eligible via the burn arm, got {other:?}"),
        }
    }

    #[test]
    fn positive_eps_consensus_closes_the_burn_arm() {
        let mut fin = burning_stock();
        for row in &mut fin.quarterly_income {
            row.operating_income = Some(10.0e6);
        }
        fin.consensus = Some(ConsensusEstimate {
            eps_mid: Some(1.2),
            ..Default::default()
        });
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.eligibility, PreProfitEligibility::NotEligible);
        assert!(overlay.consequences.matched_rules.is_empty());
    }

    #[test]
    fn missing_inputs_are_unscorable_not_entered() {
        // No statements at all: both arms unresolvable → not entered, gap named.
        let fin = CompanyFinancials {
            symbol: "GAPPY".into(),
            ..Default::default()
        };
        let overlay = compute_overlay(&fin);
        match &overlay.eligibility {
            PreProfitEligibility::Unscorable { missing } => {
                assert!(missing.iter().any(|m| m.contains("operating income")));
                assert!(missing.iter().any(|m| m.contains("free cash flow")));
            }
            other => panic!("expected unscorable, got {other:?}"),
        }
        assert!(!overlay.is_eligible());
        assert_eq!(overlay.financing_state, FinancingState::Unscorable);
        assert!(overlay.consequences.matched_rules.is_empty());
    }

    #[test]
    fn profitable_arm_false_with_unscorable_burn_arm_is_unscorable() {
        // Arm 1 decisively false, arm 2 open (no consensus, no cash-flow prints):
        // OR over {false, unknown} = unknown → not entered with the gap recorded.
        let mut fin = burning_stock();
        for row in &mut fin.quarterly_income {
            row.operating_income = Some(10.0e6);
        }
        fin.quarterly_cash_flow.clear();
        let overlay = compute_overlay(&fin);
        assert!(matches!(
            overlay.eligibility,
            PreProfitEligibility::Unscorable { .. }
        ));
    }

    // ---- Financing state ----

    #[test]
    fn financing_state_bands() {
        let mut fin = burning_stock();
        // TTM burn = 160M; liquid = 320M → runway 24.0 months exactly → adequate.
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.financing_state, FinancingState::Adequate);
        assert_eq!(overlay.statement_inputs.runway_months, Some(24.0));

        // Liquid 200M → runway 15 months → watch.
        fin.cash_and_equivalents = Some(200.0e6);
        fin.short_term_investments = None;
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.financing_state, FinancingState::Watch);

        // Liquid 100M → runway 7.5 months → constrained (and the add bar).
        fin.cash_and_equivalents = Some(100.0e6);
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.financing_state, FinancingState::Constrained);
        assert!(overlay.consequences.bar_add_family);

        // Positive FCF → not burning, no runway.
        for row in &mut fin.quarterly_cash_flow {
            row.free_cash_flow = Some(5.0e6);
        }
        // Keep eligibility via arm 1 (operating income stays negative).
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.financing_state, FinancingState::NotBurning);
        assert_eq!(overlay.statement_inputs.runway_months, None);

        // No cash line while burning → unscorable.
        for row in &mut fin.quarterly_cash_flow {
            row.free_cash_flow = Some(-40.0e6);
        }
        fin.cash_and_equivalents = None;
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.financing_state, FinancingState::Unscorable);
    }

    #[test]
    fn fcf_derives_from_ocf_minus_capex_when_unreported() {
        let row = QuarterlyCashFlowRow {
            period_end: "2026-06-30".into(),
            filing_date: None,
            free_cash_flow: None,
            operating_cash_flow: Some(10.0),
            capex: Some(-30.0), // FMP's negative-outflow convention
        };
        assert_eq!(row.resolved_free_cash_flow(), Some(-20.0));
        let row = QuarterlyCashFlowRow {
            capex: Some(30.0), // positive-outflow convention tolerated
            ..row
        };
        assert_eq!(row.resolved_free_cash_flow(), Some(-20.0));
    }

    // ---- Statement legs ----

    #[test]
    fn dilution_and_margin_legs() {
        let mut fin = burning_stock();
        // Shares: newest 130M vs 100M a year back → +30% → material dilution.
        fin.quarterly_income[0].diluted_shares = Some(130.0e6);
        // Margins: recent 2q avg −10%, preceding 2q avg +20% → non-positive and
        // −30pp → economics deterioration.
        fin.quarterly_income[0].gross_profit = Some(-10.0e6);
        fin.quarterly_income[1].gross_profit = Some(-10.0e6);
        fin.quarterly_income[2].gross_profit = Some(20.0e6);
        fin.quarterly_income[3].gross_profit = Some(20.0e6);
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.material_dilution, Some(true));
        assert_eq!(overlay.economics_deterioration, Some(true));
        // Economics + dilution → severe, statement legs alone.
        assert!(overlay.severe_deterioration);
        assert!(overlay.consequences.bar_add_family);
        assert!(overlay.consequences.exit_family_only);
    }

    #[test]
    fn financing_plus_dilution_alone_is_not_severe() {
        let mut fin = burning_stock();
        fin.cash_and_equivalents = Some(50.0e6); // constrained runway
        fin.short_term_investments = None;
        fin.quarterly_income[0].diluted_shares = Some(130.0e6); // material dilution
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.financing_state, FinancingState::Constrained);
        assert_eq!(overlay.material_dilution, Some(true));
        assert_eq!(overlay.economics_deterioration, Some(false));
        assert!(!overlay.severe_deterioration);
        // The constrained-runway bar still stands on its own.
        assert!(overlay.consequences.bar_add_family);
        assert!(!overlay.consequences.exit_family_only);
    }

    /// The economics leg alone: the latest two quarters' margin non-positive and
    /// 30pp below the preceding two, with healthy runway and shares.
    fn economics_deteriorated() -> CompanyFinancials {
        let mut fin = burning_stock();
        fin.quarterly_income[0].gross_profit = Some(-10.0e6);
        fin.quarterly_income[1].gross_profit = Some(-10.0e6);
        fin.quarterly_income[2].gross_profit = Some(20.0e6);
        fin.quarterly_income[3].gross_profit = Some(20.0e6);
        fin
    }

    #[test]
    fn economics_deterioration_alone_is_not_severe() {
        let overlay = compute_overlay(&economics_deteriorated());
        assert_eq!(overlay.economics_deterioration, Some(true));
        assert_eq!(overlay.financing_state, FinancingState::Adequate);
        assert_eq!(overlay.material_dilution, Some(false));
        assert!(!overlay.severe_deterioration);
        assert!(overlay.consequences.matched_rules.is_empty());
    }

    #[test]
    fn economics_plus_constrained_runway_is_severe_and_binds_the_exit_family() {
        let mut fin = economics_deteriorated();
        fin.cash_and_equivalents = Some(50.0e6); // constrained runway
        fin.short_term_investments = None;
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.financing_state, FinancingState::Constrained);
        assert!(overlay.severe_deterioration);
        assert!(overlay.consequences.bar_add_family);
        assert!(overlay.consequences.exit_family_only);
        assert_eq!(overlay.consequences.matched_rules.len(), 2, "{:?}", overlay.consequences.matched_rules);
    }

    #[test]
    fn the_execution_leg_is_unscorable_and_enters_no_conjunction() {
        // Constrained runway alone: the leg has no producer, enters no
        // conjunction, and the record says so.
        let mut fin = burning_stock();
        fin.cash_and_equivalents = Some(50.0e6);
        fin.short_term_investments = None;
        let overlay = compute_overlay(&fin);
        assert_eq!(overlay.execution, ExecutionLeg::Unscorable);
        assert_eq!(overlay.financing_state, FinancingState::Constrained);
        assert!(!overlay.severe_deterioration);
        assert!(overlay.consequences.bar_add_family, "the runway bar stands alone");
        assert!(!overlay.consequences.exit_family_only);
        // The leg's one state is what persists.
        let json = serde_json::to_value(&overlay).unwrap();
        assert_eq!(json["execution"], serde_json::json!({ "state": "unscorable" }));
    }

    #[test]
    fn the_overlay_stamp_is_pre_profit_v6() {
        // The statement-only severe rule and the unscorable execution leg change
        // what a persisted record's severe state means, so the stamp moves and
        // the resume gate refuses a v5 trail.
        assert_eq!(PRE_PROFIT_PARAMETER_VERSION, "pre-profit-v6");
        let overlay = compute_overlay(&burning_stock());
        assert_eq!(overlay.parameter_version, "pre-profit-v6");
    }

    #[test]
    fn capex_intensity_reads_magnitude_over_ttm_revenue() {
        let mut fin = burning_stock();
        for row in &mut fin.quarterly_cash_flow {
            row.capex = Some(-10.0e6);
        }
        let overlay = compute_overlay(&fin);
        // 40M |capex| over 400M TTM revenue.
        assert_eq!(overlay.statement_inputs.ttm_capex_intensity, Some(0.1));
    }

    #[test]
    fn capex_intensity_requires_period_aligned_windows() {
        // Each statement window is internally contiguous, but cash flow trails
        // income by one quarter — the cross-statement ratio would divide
        // mismatched trailing years, so it gaps instead. The single-source
        // reads keep their own windows.
        let mut fin = burning_stock();
        for row in &mut fin.quarterly_cash_flow {
            row.capex = Some(-10.0e6);
        }
        fin.quarterly_cash_flow.remove(0); // newest cash quarter missing: 2026-03-31 leads
        let overlay = compute_overlay(&fin);
        assert_eq!(
            overlay.statement_inputs.ttm_capex_intensity, None,
            "shifted-but-contiguous windows must not divide"
        );
        assert!(overlay.statement_inputs.ttm_operating_income.is_some());
        assert!(overlay.statement_inputs.ttm_free_cash_flow.is_some());
    }

    // ---- Clamp + schema labels ----

    // ---- Canonicalization + boundaries ----

    #[test]
    fn statement_windows_survive_shuffled_and_duplicated_rows() {
        // A history whose halves genuinely differ — recently loss-making after a
        // profitable past — so window composition is order-sensitive: raw wire
        // order reversed would read the OLD profitable quarters as the TTM.
        let mut fin = burning_stock();
        for (i, row) in fin.quarterly_income.iter_mut().enumerate() {
            row.operating_income = Some(if i < 4 { -50.0e6 } else { 100.0e6 });
            row.diluted_shares = Some(if i < 4 { 130.0e6 } else { 100.0e6 });
        }
        for (i, row) in fin.quarterly_cash_flow.iter_mut().enumerate() {
            row.free_cash_flow = Some(if i < 4 { -40.0e6 } else { 90.0e6 });
        }
        let canonical = compute_overlay(&fin);
        assert!(canonical.is_eligible());

        // The same prints served out of order with two periods duplicated must
        // produce the identical overlay read.
        let mut shuffled_fin = fin.clone();
        shuffled_fin.quarterly_income.reverse();
        shuffled_fin.quarterly_cash_flow.reverse();
        let dup_income = shuffled_fin.quarterly_income[0].clone();
        shuffled_fin.quarterly_income.insert(3, dup_income);
        let dup_cash = shuffled_fin.quarterly_cash_flow[0].clone();
        shuffled_fin.quarterly_cash_flow.insert(2, dup_cash);
        let shuffled = compute_overlay(&shuffled_fin);
        assert_eq!(shuffled.eligibility, canonical.eligibility);
        assert_eq!(shuffled.statement_inputs, canonical.statement_inputs);
        assert_eq!(shuffled.financing_state, canonical.financing_state);
    }

    #[test]
    fn conflicting_duplicate_periods_resolve_to_the_latest_filing_not_wire_order() {
        // The same quarter served twice with different prints (a restatement): the
        // later-filed print must win in either arrival order.
        let mut fin = burning_stock();
        fin.quarterly_income[0].filing_date = Some("2026-07-01".into());
        let mut restated_income = fin.quarterly_income[0].clone();
        restated_income.operating_income = Some(-80.0e6);
        restated_income.filing_date = Some("2026-08-01".into());
        fin.quarterly_cash_flow[0].filing_date = Some("2026-07-01".into());
        let mut restated_cash = fin.quarterly_cash_flow[0].clone();
        restated_cash.free_cash_flow = Some(-70.0e6);
        restated_cash.filing_date = Some("2026-08-01".into());

        // Arrival order A: originals first, restatements appended at the tail.
        let mut fin_a = fin.clone();
        fin_a.quarterly_income.push(restated_income.clone());
        fin_a.quarterly_cash_flow.push(restated_cash.clone());
        // Arrival order B: restatements served at the head.
        let mut fin_b = fin.clone();
        fin_b.quarterly_income.insert(0, restated_income);
        fin_b.quarterly_cash_flow.insert(0, restated_cash);

        let a = compute_overlay(&fin_a);
        let b = compute_overlay(&fin_b);
        assert_eq!(a.statement_inputs, b.statement_inputs);
        // TTM operating income reads the restated −80M print: −80 + 3 × −50.
        assert_eq!(a.statement_inputs.ttm_operating_income, Some(-230.0e6));
        // TTM FCF reads the restated −70M print: burn = 70 + 3 × 40.
        assert_eq!(a.statement_inputs.ttm_cash_burn, Some(190.0e6));
    }

    #[test]
    fn documented_boundaries_match_exactly_despite_float_rounding() {
        // Dilution exactly at the 15% boundary: 115/100 − 1 rounds a few ULPs
        // below 0.15 in f64 — the documented "at least 15%" must still match.
        let mut fin = burning_stock();
        fin.quarterly_income[0].diluted_shares = Some(115.0e6);
        for row in fin.quarterly_income[1..].iter_mut() {
            row.diluted_shares = Some(100.0e6);
        }
        let overlay = compute_overlay(&fin);
        assert!(
            (overlay.statement_inputs.diluted_share_change_yoy.unwrap() - 0.15).abs() < 1e-9
        );
        assert_eq!(overlay.material_dilution, Some(true));
    }

    // ---- Serde stability ----

    #[test]
    fn overlay_round_trips_through_json() {
        let overlay = compute_overlay(&burning_stock());
        let json = serde_json::to_string(&overlay).expect("serialize");
        let back: PreProfitOverlay = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(overlay, back);
    }
}
