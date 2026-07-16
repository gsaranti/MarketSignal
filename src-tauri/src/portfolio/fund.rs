//! The fund path (`docs/portfolio-analysis.md` §Asset eligibility): strategy
//! classification at loop time from `etf/info` + weightings, the reduced fund
//! computation — expense drag, exposure tilt, and the **exposure-priced valuation**
//! (a covered-weight-renormalized harmonic composite over the per-sector aggregate
//! P/E, read against its own constant-current-mix history) — and the **fund-form v2
//! scenario targets** (the settled fund-form bullet in §Starting parameters: the
//! shared spread-anchored core over the composite, driver flat, distributions in the
//! total return). Every class the pipeline is structurally unable to price returns
//! the typed `role_risk_only` readout; genuinely missing data abstains under the
//! evidence floor's fund analog. The classification is deterministic and a class the
//! engine can't price honestly degrades — never a fabricated number.

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::portfolio::engine::{
    self, AnchorObservation, CompanyFinancials, ComputedMetrics, DatedValue, EngineOutput,
    RateAnchors, TargetMeta,
};
use crate::portfolio::SubScores;

// ---- Calibration surface (drafted starting values, shadow-tuned) ---------------

/// An equity fund's exposure must be substantially in the composite's market for the
/// exposure-priced valuation to be an honest read (`docs/portfolio-analysis.md`
/// §Asset eligibility, drafted ≥ 70% US by country weightings).
pub const US_EXPOSURE_GUARD: f64 = 0.70;

/// Minimum share of fund weight in P/E-usable sectors below which the valuation is
/// recorded as a gap rather than lettered off a sliver (drafted ≥ 70%).
pub const PE_COVERAGE_GUARD: f64 = 0.70;

/// Minimum constant-mix history samples for the vs-own-history valuation read and
/// the fund-form anchor window (mirrors the stock function's observation floor).
const MIN_COMPOSITE_HISTORY: usize = 8;

/// How many quarterly samples the constant-mix history takes (the shared ~12-quarter
/// anchor window).
const HISTORY_SAMPLE_QUARTERS: usize = 12;

/// Country labels counted as US exposure in `etf/country-weightings` payloads.
const US_LABELS: &[&str] = &["united states", "united states of america", "usa", "u.s.", "us"];

/// Name / mandate fragments that deterministically flag a structurally
/// path-dependent vehicle (leveraged / inverse daily-reset products) — the same
/// screen the report's movers list applies.
const STRUCTURAL_FLAG_FRAGMENTS: &[&str] = &[
    "2x", "3x", "-1x", "-2x", "-3x", "ultra", "inverse", "leveraged", "daily bear", "daily bull",
];

// ---- Inputs --------------------------------------------------------------------

/// Per-fund metadata from FMP `etf/info` + the sector / country weightings
/// (`docs/data-sources.md §Portfolio Analysis — endpoint surface`). Every field is
/// optional; a source that can't resolve a line records a gap.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FundData {
    pub symbol: String,
    pub name: Option<String>,
    /// The `etf/info` asset-class / mandate string (e.g. "Equity", "Fixed Income").
    pub asset_class: Option<String>,
    /// Expense ratio as a decimal ratio (0.0009 for 9 bps).
    pub expense_ratio: Option<f64>,
    pub aum: Option<f64>,
    pub nav: Option<f64>,
    /// Sector weights as fractions (0–1), from `etf/sector-weightings`.
    pub sector_weights: Vec<(String, f64)>,
    /// Country weights as fractions (0–1), from `etf/country-weightings`.
    pub country_weights: Vec<(String, f64)>,
    pub gaps: Vec<String>,
}

/// One per-sector aggregate P/E print (exchange-tagged), from `sector-pe-snapshot`
/// or a `historical-sector-pe` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorPe {
    pub sector: String,
    pub exchange: String,
    pub date: String,
    pub pe: f64,
}

// ---- Strategy classification -----------------------------------------------------

/// The deterministic loop-time strategy class (`docs/portfolio-analysis.md` §Asset
/// eligibility): the asset class routes the computation because one generic fund
/// valuation cannot grade every vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FundStrategyClass {
    Equity,
    Bond,
    Commodity,
    LeveragedInverse,
    Unknown,
}

/// The classification result: the class, the structural flag, the US share where
/// readable, and — where the class is unpriceable — the typed role reason.
#[derive(Debug, Clone, PartialEq)]
pub struct FundClassification {
    pub class: FundStrategyClass,
    pub structural_flag: bool,
    pub us_share: Option<f64>,
    /// The card's classification label (e.g. "US equity fund", "bond fund").
    pub class_label: String,
    /// `None` when the exposure-priced path applies; `Some(reason)` when the class
    /// routes to `role_risk_only`.
    pub role_reason: Option<String>,
}

/// Classify a fund's strategy deterministically from its `etf/info` metadata and
/// weightings. Made at loop time — Step 3's eligibility used only Schwab instrument
/// identity (`docs/portfolio-workflow.md` §Step 3).
pub fn classify(fund: &FundData) -> FundClassification {
    let name_blob = format!(
        "{} {}",
        fund.name.as_deref().unwrap_or_default(),
        fund.asset_class.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();
    let structural_flag = STRUCTURAL_FLAG_FRAGMENTS
        .iter()
        .any(|f| name_blob.contains(f));
    if structural_flag {
        return FundClassification {
            class: FundStrategyClass::LeveragedInverse,
            structural_flag: true,
            us_share: us_share(fund),
            class_label: "leveraged / inverse vehicle".to_string(),
            role_reason: Some(
                "structurally path-dependent (leveraged / inverse daily reset) — a \
                 buy-and-hold read is structurally unsound"
                    .to_string(),
            ),
        };
    }

    let class_str = fund.asset_class.as_deref().unwrap_or("").to_ascii_lowercase();
    let class = if class_str.contains("equity") || class_str.contains("stock") {
        FundStrategyClass::Equity
    } else if class_str.contains("fixed income") || class_str.contains("bond") {
        FundStrategyClass::Bond
    } else if class_str.contains("commodity") {
        FundStrategyClass::Commodity
    } else if !fund.sector_weights.is_empty() {
        // No usable class string, but sector weightings exist — the equity path's
        // fuel; adopted with the assumption recorded by the caller's gap manifest.
        FundStrategyClass::Equity
    } else {
        FundStrategyClass::Unknown
    };

    let us = us_share(fund);
    match class {
        FundStrategyClass::Bond => FundClassification {
            class,
            structural_flag: false,
            us_share: us,
            class_label: "bond fund".to_string(),
            role_reason: Some(
                "bond fund — the on-plan surface carries no duration / credit / curve \
                 data to price it honestly (valuation recorded as a gap)"
                    .to_string(),
            ),
        },
        FundStrategyClass::Commodity => FundClassification {
            class,
            structural_flag: false,
            us_share: us,
            class_label: "commodity fund".to_string(),
            role_reason: Some(
                "commodity fund — no honest exposure-priced valuation on the on-plan \
                 surface (valuation recorded as a gap)"
                    .to_string(),
            ),
        },
        FundStrategyClass::Unknown => FundClassification {
            class,
            structural_flag: false,
            us_share: us,
            class_label: "fund with unresolved strategy class".to_string(),
            role_reason: Some(
                "strategy class unresolved and no usable sector weightings — the \
                 exposure-priced valuation has no input"
                    .to_string(),
            ),
        },
        FundStrategyClass::Equity => {
            if fund.sector_weights.is_empty() {
                FundClassification {
                    class,
                    structural_flag: false,
                    us_share: us,
                    class_label: "equity fund without usable weightings".to_string(),
                    role_reason: Some(
                        "no usable sector weighting set — the exposure-priced \
                         valuation has no input (the mutual-fund degrade)"
                            .to_string(),
                    ),
                }
            } else if us.map(|s| s < US_EXPOSURE_GUARD).unwrap_or(false) {
                FundClassification {
                    class,
                    structural_flag: false,
                    us_share: us,
                    class_label: "ex-US equity fund".to_string(),
                    role_reason: Some(format!(
                        "US exposure {:.0}% below the ≥ {:.0}% guard — an \
                         exchange-tagged US sector P/E is not an honest read on an \
                         international fund",
                        us.unwrap_or(0.0) * 100.0,
                        US_EXPOSURE_GUARD * 100.0
                    )),
                }
            } else {
                FundClassification {
                    class,
                    structural_flag: false,
                    us_share: us,
                    class_label: "US equity fund".to_string(),
                    role_reason: None,
                }
            }
        }
        FundStrategyClass::LeveragedInverse => unreachable!("handled above"),
    }
}

/// The fund's US share from its country weightings, `None` when none are reported.
fn us_share(fund: &FundData) -> Option<f64> {
    if fund.country_weights.is_empty() {
        return None;
    }
    Some(
        fund.country_weights
            .iter()
            .filter(|(c, _)| US_LABELS.contains(&c.to_ascii_lowercase().trim()))
            .map(|(_, w)| w)
            .sum(),
    )
}

// ---- The exposure-priced composite ------------------------------------------------

/// Blend the exchange-tagged per-sector P/Es into one per-sector read: the NYSE and
/// NASDAQ sector **earnings yields** averaged per sector (`docs/portfolio-analysis.md`
/// §Asset eligibility — the defined exchange blend), with a non-positive P/E excluded
/// as unusable rather than averaged in.
pub fn blend_sector_pes(rows: &[SectorPe]) -> HashMap<String, f64> {
    let mut yields: HashMap<String, Vec<f64>> = HashMap::new();
    for row in rows {
        if row.pe > 0.0 && row.pe.is_finite() {
            yields
                .entry(row.sector.to_ascii_lowercase())
                .or_default()
                .push(1.0 / row.pe);
        }
    }
    yields
        .into_iter()
        .map(|(sector, ys)| {
            let avg_yield = ys.iter().sum::<f64>() / ys.len() as f64;
            (sector, 1.0 / avg_yield)
        })
        .collect()
}

/// The covered-weight-renormalized composite earnings yield
/// (`docs/portfolio-analysis.md` §Asset eligibility): `Σ(wᵢ ÷ PEᵢ) ÷ Σwᵢ` across the
/// sectors with a usable P/E, so uncovered weight neither reads as zero earnings nor
/// lets a small priced slice extrapolate across the whole fund.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositeYield {
    /// The composite earnings yield (decimal ratio).
    pub yield_value: f64,
    /// Covered share of total fund weight — the coverage-guard input.
    pub covered_share: f64,
}

pub fn composite_yield(
    weights: &[(String, f64)],
    blended_pe: &HashMap<String, f64>,
) -> Option<CompositeYield> {
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return None;
    }
    let mut covered = 0.0;
    let mut sum = 0.0;
    for (sector, w) in weights {
        if let Some(pe) = blended_pe.get(&sector.to_ascii_lowercase()) {
            covered += w;
            sum += w / pe;
        }
    }
    if covered <= 0.0 {
        return None;
    }
    Some(CompositeYield {
        yield_value: sum / covered,
        covered_share: covered / total,
    })
}

/// The constant-current-mix composite yield history (`docs/portfolio-analysis.md`
/// §Asset eligibility): today's weights over the historical sector multiples, sampled
/// at the trailing quarter ends, under the same blend / renormalization / coverage
/// convention as the snapshot — so the vs-own-history read compares like to like. A
/// sample date whose coverage falls below the guard is skipped rather than composed
/// off a sliver.
pub fn composite_yield_history(
    weights: &[(String, f64)],
    history: &HashMap<String, Vec<SectorPe>>,
    as_of: NaiveDate,
) -> Vec<DatedValue> {
    let mut out = Vec::new();
    for q in 1..=HISTORY_SAMPLE_QUARTERS {
        let sample_date = quarter_end_before(as_of, q);
        let date_str = sample_date.format("%Y-%m-%d").to_string();
        // Per sector: the latest print on or before the sample date, per exchange,
        // then the same blend as the snapshot.
        let mut rows: Vec<SectorPe> = Vec::new();
        for (sector, prints) in history {
            let mut latest_by_exchange: HashMap<&str, &SectorPe> = HashMap::new();
            for p in prints {
                if p.date.as_str() <= date_str.as_str() {
                    let slot = latest_by_exchange.entry(p.exchange.as_str()).or_insert(p);
                    if p.date > slot.date {
                        *slot = p;
                    }
                }
            }
            for p in latest_by_exchange.values() {
                rows.push(SectorPe {
                    sector: sector.clone(),
                    exchange: p.exchange.clone(),
                    date: p.date.clone(),
                    pe: p.pe,
                });
            }
        }
        let blended = blend_sector_pes(&rows);
        if let Some(c) = composite_yield(weights, &blended) {
            if c.covered_share >= PE_COVERAGE_GUARD {
                out.push(DatedValue {
                    date: date_str,
                    value: c.yield_value,
                });
            }
        }
    }
    out.sort_by(|a, b| a.date.cmp(&b.date));
    out
}

/// The `q`-th calendar quarter end strictly before `as_of` (q = 1 is the most recent).
fn quarter_end_before(as_of: NaiveDate, q: usize) -> NaiveDate {
    use chrono::Datelike;
    let mut year = as_of.year();
    // The most recent completed quarter end ≤ as_of.
    let mut month_end = match as_of.month() {
        1..=3 => {
            year -= 1;
            12
        }
        4..=6 => 3,
        7..=9 => 6,
        _ => 9,
    };
    for _ in 1..q {
        if month_end == 3 {
            month_end = 12;
            year -= 1;
        } else {
            month_end -= 3;
        }
    }
    let day = match month_end {
        3 => 31,
        6 => 30,
        9 => 30,
        _ => 31,
    };
    NaiveDate::from_ymd_opt(year, month_end, day).expect("valid quarter end")
}

/// The fund half of a holding's dossier: the per-fund metadata plus the run-level
/// sector-P/E surface the fund engine reads. Assembled by the job (which memoizes
/// the sector-P/E snapshot and per-sector histories across funds —
/// `docs/portfolio-workflow.md` §Step 6a) so the engine stays pure.
#[derive(Debug, Clone, PartialEq)]
pub struct FundContext {
    pub fund: FundData,
    pub sector_pe: Vec<SectorPe>,
    /// Keyed by lowercase sector label.
    pub sector_pe_history: HashMap<String, Vec<SectorPe>>,
    pub as_of: NaiveDate,
}

// ---- The fund engine ---------------------------------------------------------------

/// The engine-computed half of a `role_risk_only` readout — the model authors only
/// the role prose on top of this.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoleRiskReadout {
    pub class_label: String,
    /// Top exposure weights (sector where present, else country).
    pub exposure_tilt: Vec<(String, f64)>,
    pub expense_ratio: Option<f64>,
    /// Annualized realized volatility, where computable.
    pub observable_risk: Option<f64>,
    pub structural_flag: bool,
    pub evidence_gaps: Vec<String>,
}

/// What the fund engine resolved to: the priced branch (the shared [`EngineOutput`]),
/// the typed role / risk readout, or the evidence floor's fund-analog abstention.
#[derive(Debug, Clone, PartialEq)]
pub enum FundEngineVerdict {
    Priced(Box<EngineOutput>),
    RoleRiskOnly(Box<RoleRiskReadout>),
    InsufficientEvidence(String),
}

/// Everything the fund engine reads, assembled by the job (the engine stays pure).
pub struct FundEngineInputs<'a> {
    pub fund: &'a FundData,
    /// The fund's quote / price surface (quote, dated closes, TTM distributions ride
    /// the same per-symbol financials pull a stock uses).
    pub financials: &'a CompanyFinancials,
    /// Today's `sector-pe-snapshot` rows (both exchanges).
    pub sector_pe: &'a [SectorPe],
    /// Per-sector `historical-sector-pe` rows (both exchanges), keyed by lowercase
    /// sector label.
    pub sector_pe_history: &'a HashMap<String, Vec<SectorPe>>,
    pub rates: &'a RateAnchors,
    pub as_of: NaiveDate,
}

/// Analyze a fund holding down the reduced path (`docs/portfolio-analysis.md` §Asset
/// eligibility): classify, then either the exposure-priced equity-fund computation —
/// real valuation / risk, neutral-imputed quality, the fund-form v2 targets, the fund
/// tier, the hurdle — or the typed `role_risk_only` readout; genuinely missing
/// floor-bearing data abstains instead.
pub fn analyze_fund(inp: &FundEngineInputs) -> FundEngineVerdict {
    let fund = inp.fund;
    let fin = inp.financials;

    // The evidence floor's fund analog, floor-bearing legs first: a current quote /
    // NAV and the `etf/info` surface.
    let Some(spot) = fin.current_price.or(fund.nav) else {
        return FundEngineVerdict::InsufficientEvidence(
            "no current quote or NAV for the fund — nothing to value against".to_string(),
        );
    };
    let info_present = fund.asset_class.is_some()
        || fund.expense_ratio.is_some()
        || fund.name.is_some()
        || !fund.sector_weights.is_empty();
    if !info_present {
        return FundEngineVerdict::InsufficientEvidence(
            "fund metadata (etf/info) unavailable — the fund analog's floor-bearing \
             input is missing"
                .to_string(),
        );
    }

    let classification = classify(fund);
    let vol = per_period_volatility(fin);
    let annual_vol = vol.map(|v| v * 15.87);
    let drawdown = engine::max_drawdown(&fin.daily_closes, &fin.price_history);

    // A structurally unpriceable class takes the typed role / risk readout — never
    // `insufficient-evidence` (the evidence isn't deficient; the class is).
    if let Some(reason) = &classification.role_reason {
        let tilt = if !fund.sector_weights.is_empty() {
            top_weights(&fund.sector_weights)
        } else {
            top_weights(&fund.country_weights)
        };
        let mut gaps = fund.gaps.clone();
        gaps.push(reason.clone());
        return FundEngineVerdict::RoleRiskOnly(Box::new(RoleRiskReadout {
            class_label: classification.class_label,
            exposure_tilt: tilt,
            expense_ratio: fund.expense_ratio,
            observable_risk: annual_vol,
            structural_flag: classification.structural_flag,
            evidence_gaps: gaps,
        }));
    }

    // The priced equity-fund path: the exposure-priced valuation under its coverage
    // guard, read against its own constant-current-mix history.
    let blended_now = blend_sector_pes(inp.sector_pe);
    let Some(composite) = composite_yield(&fund.sector_weights, &blended_now) else {
        return FundEngineVerdict::InsufficientEvidence(
            "no P/E-usable sector overlap between the fund's weightings and the \
             sector-P/E snapshot"
                .to_string(),
        );
    };
    if composite.covered_share < PE_COVERAGE_GUARD {
        return FundEngineVerdict::InsufficientEvidence(format!(
            "fund valuation coverage {:.0}% below the ≥ {:.0}% P/E-usable guard — \
             valuation recorded as a gap rather than lettered off a sliver",
            composite.covered_share * 100.0,
            PE_COVERAGE_GUARD * 100.0
        ));
    }
    let history = composite_yield_history(&fund.sector_weights, inp.sector_pe_history, inp.as_of);
    if history.len() < MIN_COMPOSITE_HISTORY {
        return FundEngineVerdict::InsufficientEvidence(format!(
            "only {} constant-mix composite history samples (need {MIN_COMPOSITE_HISTORY}) — \
             the vs-own-history valuation read has no basis",
            history.len()
        ));
    }

    // Valuation: what the mix costs now versus what it has cost — the percentile rank
    // of the current composite yield in its own history (a higher yield is cheaper).
    let below = history
        .iter()
        .filter(|h| h.value <= composite.yield_value)
        .count();
    let valuation = (below as f64 / history.len() as f64) * 100.0;

    // Risk: realized volatility plus drawdown (higher = safer, like the stock leg).
    let risk = {
        let vol_leg = vol.map(|v| engine::scale(v, 0.04, 0.0));
        let dd_leg = drawdown.map(|d| engine::scale(d, 0.6, 0.0));
        match (vol_leg, dd_leg) {
            (None, None) => None,
            (a, b) => Some((a.unwrap_or(50.0) + b.unwrap_or(50.0)) / 2.0),
        }
    };
    let Some(risk) = risk else {
        return FundEngineVerdict::InsufficientEvidence(
            "no price history for the fund's risk read — the second real fund \
             sub-score has no input"
                .to_string(),
        );
    };

    // Momentum rides as context (outside the letter), like the stock path.
    let momentum = trailing_return(fin).map(|r| engine::scale(r, -0.30, 0.30));

    // The letter: real valuation / risk + the neutral-imputed absent quality axis —
    // the priced-fund grade contract, with the visible low-confidence marker.
    let sub_scores = SubScores {
        quality: 50.0,
        valuation,
        momentum: momentum.unwrap_or(50.0),
        risk,
    };
    let grade = engine::grade_from_subscores(&sub_scores);

    // The fund-form v2 targets: driver = spot × composite yield (flat), the anchor
    // spreads from the constant-mix history against the dated DGS10 join, TTM
    // distributions in the total return.
    let implied_eps = spot * composite.yield_value;
    let observations: Vec<AnchorObservation> = history
        .iter()
        .filter_map(|h| {
            let dgs10_t = engine::latest_on_or_before(&inp.rates.dgs10_history, &h.date)?;
            Some(AnchorObservation {
                spread: h.value - dgs10_t,
                raw_multiple: 1.0 / h.value,
            })
        })
        .collect();
    let distributions = fin.ttm_dividends_per_share.unwrap_or(0.0);
    let scenario = engine::spread_anchored_scenarios(
        spot,
        [implied_eps, implied_eps, implied_eps],
        &observations,
        inp.rates.dgs10,
        distributions,
    );

    let mut metrics = base_metrics(fin);
    metrics.expense_ratio = fund.expense_ratio;
    metrics.nav_premium = fund.nav.and_then(|nav| {
        (nav > 0.0).then(|| spot / nav - 1.0)
    });
    metrics.composite_coverage = Some(composite.covered_share);

    // The uncovered slice is reported beside the read, never averaged in
    // (`docs/portfolio-analysis.md` §Asset eligibility), and a US-exposure guard
    // that could not be verified is a stated premise, not a verified one.
    let mut engine_notes: Vec<String> = Vec::new();
    if composite.covered_share < 1.0 {
        engine_notes.push(format!(
            "composite P/E coverage {:.0}% of fund weight — the uncovered {:.0}% is \
             reported beside the valuation read, never averaged in",
            composite.covered_share * 100.0,
            (1.0 - composite.covered_share) * 100.0
        ));
    }
    if classification.us_share.is_none() {
        engine_notes.push(
            "US-exposure guard unverifiable (no country weightings) — the ≥ 70% US \
             premise is assumed, not verified"
                .to_string(),
        );
    }

    let targets = engine::build_price_targets(
        spot,
        &scenario,
        &metrics,
        "fund exposure composite",
        true,
    );
    let tier = engine::assign_fund_tier(false, annual_vol, drawdown);
    let hurdle = engine::hurdle_read(&scenario, inp.rates.dgs2, tier);
    let meta = TargetMeta {
        driver_rung: "fund exposure composite".to_string(),
        rate_anchored: scenario.rate_anchored,
        anchor_observations: scenario.anchor_observations,
        flat_driver: true,
        degenerate_scenarios: scenario.degenerate_scenarios,
        monotonicity_repaired: scenario.monotonicity_repaired,
        current_multiple_carry: scenario.current_multiple_carry,
        parameter_version: engine::SCENARIO_TARGET_PARAMETER_VERSION.to_string(),
    };

    FundEngineVerdict::Priced(Box::new(EngineOutput {
        sub_scores,
        grade,
        metrics,
        price_targets: targets,
        risk_tier: tier,
        tier_gaps: engine_notes,
        hurdle,
        target_meta: meta,
        // The quality axis is structurally absent and neutral-imputed — the letter
        // always carries the visible low-confidence marker on this branch.
        low_confidence_grade: true,
    }))
}

/// The top exposure weights for the readout's tilt line, largest first, capped at 5.
fn top_weights(weights: &[(String, f64)]) -> Vec<(String, f64)> {
    let mut sorted = weights.to_vec();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.truncate(5);
    sorted
}

/// Per-period volatility over whichever price history is present (dated preferred).
fn per_period_volatility(fin: &CompanyFinancials) -> Option<f64> {
    let closes: Vec<f64> = if !fin.daily_closes.is_empty() {
        fin.daily_closes.iter().map(|d| d.value).collect()
    } else {
        fin.price_history.clone()
    };
    engine::return_volatility(&closes)
}

/// Trailing return over whichever price history is present.
fn trailing_return(fin: &CompanyFinancials) -> Option<f64> {
    let closes: Vec<f64> = if !fin.daily_closes.is_empty() {
        fin.daily_closes.iter().map(|d| d.value).collect()
    } else {
        fin.price_history.clone()
    };
    match (closes.first(), closes.last()) {
        (Some(&first), Some(&last)) if closes.len() >= 2 && first > 0.0 => Some(last / first - 1.0),
        _ => None,
    }
}

/// The fund's base metrics (price-derived legs only; the statement legs stay `None`).
fn base_metrics(fin: &CompanyFinancials) -> ComputedMetrics {
    ComputedMetrics {
        return_volatility: per_period_volatility(fin),
        trailing_return: trailing_return(fin),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weights() -> Vec<(String, f64)> {
        vec![
            ("Technology".to_string(), 0.50),
            ("Financial Services".to_string(), 0.30),
            ("Energy".to_string(), 0.20),
        ]
    }

    fn snapshot() -> Vec<SectorPe> {
        let mut rows = Vec::new();
        for (sector, nyse, nasdaq) in [
            ("Technology", 30.0, 34.0),
            ("Financial Services", 14.0, 16.0),
            ("Energy", 11.0, 13.0),
        ] {
            rows.push(SectorPe {
                sector: sector.to_string(),
                exchange: "NYSE".to_string(),
                date: "2026-07-15".to_string(),
                pe: nyse,
            });
            rows.push(SectorPe {
                sector: sector.to_string(),
                exchange: "NASDAQ".to_string(),
                date: "2026-07-15".to_string(),
                pe: nasdaq,
            });
        }
        rows
    }

    fn history() -> HashMap<String, Vec<SectorPe>> {
        // Quarterly prints back through 2022 for each sector, both exchanges, so
        // every sampled quarter finds a print on or before it.
        let mut map: HashMap<String, Vec<SectorPe>> = HashMap::new();
        let dates = [
            "2022-09-15", "2022-12-15", "2023-03-15", "2023-06-15", "2023-09-15",
            "2023-12-15", "2024-03-15", "2024-06-15", "2024-09-15", "2024-12-15",
            "2025-03-15", "2025-06-15", "2025-09-15", "2025-12-15", "2026-03-15",
            "2026-06-15",
        ];
        for (sector, base_pe) in [
            ("Technology", 26.0),
            ("Financial Services", 13.0),
            ("Energy", 10.0),
        ] {
            let mut prints = Vec::new();
            for (i, date) in dates.iter().enumerate() {
                for exchange in ["NYSE", "NASDAQ"] {
                    prints.push(SectorPe {
                        sector: sector.to_string(),
                        exchange: exchange.to_string(),
                        date: date.to_string(),
                        pe: base_pe + 0.2 * i as f64,
                    });
                }
            }
            map.insert(sector.to_ascii_lowercase(), prints);
        }
        map
    }

    fn fund() -> FundData {
        FundData {
            symbol: "VTI".to_string(),
            name: Some("Total US Market ETF".to_string()),
            asset_class: Some("Equity".to_string()),
            expense_ratio: Some(0.0003),
            aum: Some(4.0e11),
            nav: Some(280.0),
            sector_weights: weights(),
            country_weights: vec![("United States".to_string(), 0.99)],
            gaps: vec![],
        }
    }

    fn financials(price: f64) -> CompanyFinancials {
        CompanyFinancials {
            symbol: "VTI".to_string(),
            current_price: Some(price),
            price_history: vec![250.0, 260.0, 270.0, 282.0],
            daily_closes: vec![
                DatedValue { date: "2026-04-01".into(), value: 250.0 },
                DatedValue { date: "2026-05-01".into(), value: 260.0 },
                DatedValue { date: "2026-06-01".into(), value: 270.0 },
                DatedValue { date: "2026-07-15".into(), value: price },
            ],
            ttm_dividends_per_share: Some(3.6),
            ..Default::default()
        }
    }

    fn rates() -> RateAnchors {
        let dates = [
            "2022-09-01", "2023-01-01", "2023-06-01", "2024-01-01", "2024-06-01",
            "2025-01-01", "2025-06-01", "2026-01-01", "2026-06-01",
        ];
        RateAnchors {
            dgs2: 0.04,
            dgs10: 0.045,
            dgs10_history: dates
                .iter()
                .map(|d| DatedValue { date: d.to_string(), value: 0.04 })
                .collect(),
        }
    }

    fn as_of() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 16).unwrap()
    }

    #[test]
    fn blend_averages_exchange_yields_not_pes() {
        let blended = blend_sector_pes(&snapshot());
        // Technology: yields 1/30 and 1/34 average to ~0.031373 → PE ≈ 31.875.
        let tech = blended.get("technology").unwrap();
        assert!((tech - 2.0 / (1.0 / 30.0 + 1.0 / 34.0)).abs() < 1e-9);
    }

    #[test]
    fn composite_renormalizes_over_covered_weight_only() {
        let mut blended = blend_sector_pes(&snapshot());
        blended.remove("energy"); // 20% of weight now uncovered
        let c = composite_yield(&weights(), &blended).unwrap();
        assert!((c.covered_share - 0.80).abs() < 1e-9);
        // The composite is over the covered weight, renormalized by it — the
        // uncovered slice neither reads as zero earnings nor extrapolates.
        let tech_pe = 2.0 / (1.0 / 30.0 + 1.0 / 34.0);
        let fin_pe = 2.0 / (1.0 / 14.0 + 1.0 / 16.0);
        let expected = (0.5 / tech_pe + 0.3 / fin_pe) / 0.8;
        assert!((c.yield_value - expected).abs() < 1e-12);
    }

    #[test]
    fn classification_routes_the_unpriceable_classes_to_role_risk() {
        let mut leveraged = fund();
        leveraged.name = Some("Ultra 3x Daily Bull".to_string());
        assert!(classify(&leveraged).role_reason.is_some());
        assert!(classify(&leveraged).structural_flag);

        let mut bond = fund();
        bond.asset_class = Some("Fixed Income".to_string());
        let c = classify(&bond);
        assert_eq!(c.class, FundStrategyClass::Bond);
        assert!(c.role_reason.is_some());

        let mut intl = fund();
        intl.country_weights = vec![
            ("United States".to_string(), 0.40),
            ("Japan".to_string(), 0.60),
        ];
        let c = classify(&intl);
        assert!(c.role_reason.is_some(), "below the US-exposure guard");

        let mut weightless = fund();
        weightless.asset_class = Some("Equity".to_string());
        weightless.sector_weights = vec![];
        assert!(classify(&weightless).role_reason.is_some(), "the mutual-fund degrade");

        assert!(classify(&fund()).role_reason.is_none(), "the priced US equity fund");
    }

    #[test]
    fn priced_equity_fund_gets_the_fund_grade_contract_and_targets() {
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &fund(),
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::Priced(out) => {
                // The absent quality axis is neutral-imputed and the letter carries
                // the visible low-confidence marker.
                assert_eq!(out.sub_scores.quality, 50.0);
                assert!(out.low_confidence_grade);
                // Real valuation (vs-own-history percentile) and risk.
                assert!((0.0..=100.0).contains(&out.sub_scores.valuation));
                assert!((0.0..=100.0).contains(&out.sub_scores.risk));
                // Fund-form v2 targets: flat composite driver, versioned methodology.
                let tm = out.price_targets.twelve_month.as_ref().unwrap();
                assert!(tm.methodology.contains("fund exposure composite"), "{}", tm.methodology);
                assert!(out.target_meta.flat_driver);
                assert!(out.target_meta.rate_anchored, "12 history samples anchor");
                assert!(tm.bear <= tm.base && tm.base <= tm.bull);
                // Tier + hurdle exist on the priced branch.
                assert_ne!(
                    out.hurdle.state,
                    crate::portfolio::HurdleState::Unscorable
                );
                // Full coverage: recorded on the metrics, and no uncovered-share note.
                assert_eq!(out.metrics.composite_coverage, Some(1.0));
                assert!(out.tier_gaps.is_empty(), "{:?}", out.tier_gaps);
                // NAV premium is computed as context (282 / 280 − 1).
                assert!((out.metrics.nav_premium.unwrap() - (282.0 / 280.0 - 1.0)).abs() < 1e-9);
                assert_eq!(out.metrics.expense_ratio, Some(0.0003));
            }
            other => panic!("expected the priced fund branch, got {other:?}"),
        }
    }

    #[test]
    fn partial_coverage_grades_with_the_uncovered_share_reported() {
        // 80% of weight is P/E-usable (above the ≥70% guard): the fund grades, the
        // coverage rides the metrics, and the uncovered slice is a recorded note —
        // reported beside the read, never averaged in.
        let mut partial = fund();
        partial.sector_weights = vec![
            ("Technology".to_string(), 0.50),
            ("Financial Services".to_string(), 0.30),
            ("Utilities".to_string(), 0.20), // not in the snapshot or history
        ];
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &partial,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::Priced(out) => {
                assert!((out.metrics.composite_coverage.unwrap() - 0.80).abs() < 1e-9);
                assert!(
                    out.tier_gaps.iter().any(|g| g.contains("composite P/E coverage")),
                    "the uncovered share must be a recorded note: {:?}",
                    out.tier_gaps
                );
            }
            other => panic!("expected the priced branch, got {other:?}"),
        }

        // A fund with no country weightings prices on the assumed US premise — the
        // unverifiable guard is a recorded note, never a silent pass.
        let mut no_countries = fund();
        no_countries.country_weights = vec![];
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &no_countries,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::Priced(out) => {
                assert!(
                    out.tier_gaps.iter().any(|g| g.contains("US-exposure guard")),
                    "{:?}",
                    out.tier_gaps
                );
            }
            other => panic!("expected the priced branch, got {other:?}"),
        }
    }

    #[test]
    fn coverage_below_the_guard_abstains_rather_than_lettering_a_sliver() {
        let mut thin = fund();
        // Only 50% of weight is in sectors the snapshot prices.
        thin.sector_weights = vec![
            ("Technology".to_string(), 0.50),
            ("Utilities".to_string(), 0.50), // not in the snapshot
        ];
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &thin,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("coverage"), "{reason}");
            }
            other => panic!("expected the coverage abstention, got {other:?}"),
        }
    }

    #[test]
    fn bond_fund_returns_the_role_risk_readout() {
        let mut bond = fund();
        bond.asset_class = Some("Fixed Income".to_string());
        bond.sector_weights = vec![];
        let fin = financials(100.0);
        let inputs = FundEngineInputs {
            fund: &bond,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::RoleRiskOnly(r) => {
                assert_eq!(r.class_label, "bond fund");
                assert!(!r.structural_flag);
                assert!(r.observable_risk.is_some(), "vol from price history");
                assert!(!r.exposure_tilt.is_empty(), "country tilt stands in");
                assert!(r.evidence_gaps.iter().any(|g| g.contains("duration")));
            }
            other => panic!("expected role_risk_only, got {other:?}"),
        }
    }

    #[test]
    fn missing_quote_and_missing_info_abstain_under_the_fund_floor() {
        let mut fin = financials(282.0);
        fin.current_price = None;
        let mut no_nav = fund();
        no_nav.nav = None;
        let inputs = FundEngineInputs {
            fund: &no_nav,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        assert!(matches!(
            analyze_fund(&inputs),
            FundEngineVerdict::InsufficientEvidence(_)
        ));

        let bare = FundData { symbol: "XXX".into(), ..Default::default() };
        let fin = financials(50.0);
        let inputs = FundEngineInputs {
            fund: &bare,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("etf/info"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn quarter_end_walkback_is_correct() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 16).unwrap();
        assert_eq!(quarter_end_before(d, 1).to_string(), "2026-06-30");
        assert_eq!(quarter_end_before(d, 2).to_string(), "2026-03-31");
        assert_eq!(quarter_end_before(d, 5).to_string(), "2025-06-30");
        let jan = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        assert_eq!(quarter_end_before(jan, 1).to_string(), "2025-12-31");
    }
}
