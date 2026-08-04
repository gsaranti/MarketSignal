//! The Step-7 portfolio roll-up and construction stage
//! (`docs/portfolio-analysis.md` §Portfolio roll-up and construction;
//! `docs/portfolio-workflow.md` §Step 7a, §Step 7b): the two things the per-holding
//! loop structurally cannot do, because it decides each holding before any other
//! holding's verdict exists.
//!
//! **Step 7a is deterministic** ([`build_aggregates`]): the whole-book aggregates —
//! sector / exposure table with fund weightings folded in at the sector level
//! (single-name look-through is off-plan), sector-level overlap clusters, the
//! not-rated positions' risk / exposure contribution (market value + signed
//! notional; duration / credit / delta ride as typed gaps), cash — plus the
//! per-holding **action-sizing spine rows**, each carrying the construction-allowed
//! action set: the engine-bounded feasible set for a fresh holding, the
//! **carried-action transition set** (toward *hold* only, plus the aggregate-gated
//! context-trim carve-out) for a carried one.
//!
//! **Step 7b is the construction model call** (built in [`super::pipeline`] /
//! [`super::job`] over this module's contract): the 122B reconciles each holding's
//! standalone lean against the aggregates into its final action + target-weight
//! range and the portfolio-level view. This module owns the **schema**
//! ([`construction_schema`] — per-holding action enums are structural), the
//! **prompt text**, and the deterministic **joint-feasibility validation**
//! ([`validate_construction`]): the implied post-action book, the range / rung-band
//! / concentration checks, the transition rule, and the app-validated action-half
//! attributions. A failing validation names its violations; the caller re-runs the
//! synthesis once with them named, and a persisting infeasibility fails the run
//! (`docs/portfolio-analysis.md` §Portfolio roll-up and construction).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::portfolio::engine;
use crate::portfolio::outcome::DecisionEpisode;
use crate::portfolio::{
    Action, ActionAttribution, ActionSource, ActionWhatChanged, AssetClass, ContextCause,
    Conviction, ExitedPosition, ExposureWeight, Grade, HoldingAudit, HoldingVerdict,
    HurdleState, InvestorProfile, PositionChange, RiskTier, VerdictDisposition,
};
use crate::schwab::Holdings;

// ---- Drafted constants (calibratable) ------------------------------------------

/// A sector counts as an **overlap cluster** when at least two holdings contribute
/// to it and its combined (direct + fund-folded) weight reaches this share of the
/// book. Drafted, calibratable — the docs name the mechanism (an exposure-level
/// cluster), not the number.
pub const OVERLAP_CLUSTER_MIN_WEIGHT: f64 = 0.20;

/// The minimum current weight at which a **`became-oversized`** context claim is
/// checkable-true (`docs/portfolio-analysis.md` §Portfolio roll-up — a context
/// attribution "must map to a real aggregate"): a claim on a position under this
/// share of the book is rejected. Drafted, calibratable; the 25% concentration cap
/// always qualifies above it.
pub const OVERSIZED_MIN_WEIGHT: f64 = 0.15;

/// A not-rated position at or above this share of the account **drives** the
/// whole-book aggregates; below it, it is recorded but not material
/// (`docs/portfolio-analysis.md` §Starting parameters — drafted ≥ 5%).
pub const NOT_RATED_MATERIAL_MIN_WEIGHT: f64 = 0.05;

/// Drift tolerance for the **implied-book** weight checks (fractions of the
/// book): the implied post-action weights carry the solve's mid-of-range drift,
/// so these comparisons need real slack. Wide enough for a roughly balanced
/// plan; tight enough that a real breach still trips. Calibratable.
pub const WEIGHT_EPS: f64 = 0.005;

/// Rounding tolerance for the **per-holding structural** checks — range
/// ordering, the sell-all zero range, the rung-band bounds. These compare the
/// model's own proposed numbers against exact engine values (no drift enters),
/// so the tolerance covers decimal rounding only: a sell-all may not retain
/// 0.5% of the book, and an inverted range may not pass, under the wide eps.
pub const STRUCT_EPS: f64 = 1e-4;

/// Dollar tolerance for the profile-gated funded-by-trims check.
const DOLLAR_EPS: f64 = 1.0;

// ---- The per-holding sizing-spine row ------------------------------------------

/// Which verdict branch a spine row rides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpineBranch {
    Priced,
    RoleRisk,
}

/// One holding's action-sizing spine inputs (`docs/portfolio-workflow.md` §Step 7a)
/// — the engine-known decision surface the construction call chooses within,
/// persisted with the roll-up so the chosen actions stay auditable against the
/// bounds they were chosen under.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SizingSpineRow {
    pub symbol: String,
    pub asset_class: AssetClass,
    pub branch: SpineBranch,
    /// The position's share of the account (market value ÷ account total).
    pub current_weight: f64,
    pub market_value: f64,
    pub current_price: Option<f64>,
    /// Headroom to the 25% single-position concentration cap (≥ 0).
    pub concentration_headroom: f64,
    /// Twelve-month base-case target vs the current price (`base ⁄ price − 1`);
    /// `None` where either leg is missing (`role_risk_only`, no targets).
    pub upside_downside: Option<f64>,
    /// The three-state capital-efficiency read — only `fails` is dead money.
    pub dead_money: Option<HurdleState>,
    /// Unrealized P/L (market value − total cost basis): a harvestable loss or a
    /// taxable gain, by sign.
    pub unrealized_pl: Option<f64>,
    pub risk_tier: Option<RiskTier>,
    pub grade: Option<Grade>,
    pub conviction: Option<Conviction>,
    /// The standalone lean (priced branch; `None` on `role_risk_only`). On a
    /// carried row this is the *prior* pass's lean — stale, so the divergence
    /// machinery applies to fresh rows only.
    pub lean: Option<Action>,
    /// The prior lean where one is comparable (the prior verdict's lean, falling
    /// back to its action for pre-construction runs whose action *was* the lean) —
    /// the moved-intrinsic attribution's comparison baseline.
    pub prior_lean: Option<Action>,
    /// The action-change baseline: a fresh row's prior-run action; a carried row's
    /// carried action (what its card currently stands on, post any rule demotion).
    pub prior_action: Option<Action>,
    pub position_change: PositionChange,
    pub carried: bool,
    pub over_age: bool,
    /// The carried add-family action was rule-demoted to *hold* before this stage.
    pub rule_demoted: bool,
    /// The pre-profit overlay rule in force, rendered (`None` when none).
    pub pre_profit_rule: Option<String>,
    /// The hard-forensic add-family bar (`docs/portfolio-analysis.md` §Starting
    /// parameters). **Dormant wiring**: no producer persists a hard-forensic state
    /// yet (the typed event producers are unbuilt), so this is structurally
    /// `false` until one lands — same posture as the outcome slice's
    /// standing-thesis leg.
    pub hard_forensic_bar: bool,
    /// The holding's sector label where one resolved (fail-soft — the `unknown`
    /// bucket carries the rest).
    pub sector: Option<String>,
    /// The construction-allowed action set: the engine-bounded feasible set
    /// (fresh), the transition set (carried), or the reduced spine (fresh
    /// `role_risk_only`).
    pub offered: Vec<Action>,
    /// `trim` entered `offered` only through the carried-name context-trim
    /// carve-out — choosing it requires a validated concentration / overlap
    /// attribution (`docs/portfolio-analysis.md` §Triggering).
    pub context_trim_carveout: bool,
    /// The tax framing note (profile-driven, by P/L sign); `None` when the
    /// profile is not tax-sensitive or the P/L is unknown.
    pub tax_note: Option<String>,
    /// `role_risk_only` decision inputs (all `None` / empty on `priced`): this
    /// branch's action is authored **wholly at 7b** ([`crate::portfolio::RoleRiskVerdict`]),
    /// so the verdict's engine + model reads must reach the construction call —
    /// the deterministic classification label and the model's role read…
    #[serde(default)]
    pub class_label: Option<String>,
    #[serde(default)]
    pub role_summary: Option<String>,
    /// …the expense ratio as an annual return headwind (fraction), where reported…
    #[serde(default)]
    pub expense_drag: Option<f64>,
    /// …annualized realized volatility (fraction), where computable…
    #[serde(default)]
    pub observable_risk: Option<f64>,
    /// …the structurally-path-dependent flag (leveraged / inverse / option-overlay
    /// vehicles)…
    #[serde(default)]
    pub structural_flag: bool,
    /// …the top exposure weights (sector or country; capped at three)…
    #[serde(default)]
    pub exposure_tilt: Vec<ExposureWeight>,
    /// …and the typed evidence gaps — this branch's confidence surface.
    #[serde(default)]
    pub evidence_gaps: Vec<String>,
    /// The same-underlying option-overlay read (`docs/portfolio-analysis.md`
    /// §Portfolio action — a covered call caps the upside the targets imply; a
    /// protective put trims the downside), classified deterministically from the
    /// holdings snapshot's OCC option rows with a share-coverage estimate. Both
    /// branches carry it; `None` when no held option shares the underlying.
    #[serde(default)]
    pub option_overlay: Option<String>,
}

/// One sector row of the whole-book exposure table: direct (stock) weight plus the
/// fund-folded weight, with the contributing symbols.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorExposureRow {
    pub sector: String,
    pub direct_weight: f64,
    pub fund_weight: f64,
    pub holdings: Vec<String>,
}

impl SectorExposureRow {
    pub fn total(&self) -> f64 {
        self.direct_weight + self.fund_weight
    }
}

/// A sector-level overlap cluster — several holdings sharing one exposure above
/// the threshold, so they size down together
/// (`docs/portfolio-analysis.md` §Portfolio roll-up and construction).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlapCluster {
    pub sector: String,
    pub combined_weight: f64,
    pub symbols: Vec<String>,
}

/// A not-rated position's risk / exposure contribution — computed from what the
/// position payload actually carries (market value + signed notional), the
/// unsourceable analytics riding as typed gaps, never computed numbers
/// (`docs/portfolio-analysis.md` §Portfolio roll-up and construction).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotRatedContribution {
    pub symbol: String,
    pub asset_class: AssetClass,
    /// Signed share of the account (a net-short position reads negative).
    pub weight: f64,
    pub market_value: f64,
    /// Option-contract notional (quantity × strike × 100 via the OCC symbol);
    /// `None` where no notional is derivable.
    pub signed_notional: Option<f64>,
    /// Whether the position is material to the aggregates (≥ the drafted 5% bar).
    pub material: bool,
    /// The typed analytics gaps (duration / credit / standalone delta — no
    /// on-plan source carries them).
    pub gaps: Vec<String>,
}

/// The Step-7a whole-book aggregates + per-holding spine, persisted with the
/// roll-up (`docs/storage.md §Local Analysis Suite Storage` — the portfolio
/// roll-up).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookAggregates {
    pub spine: Vec<SizingSpineRow>,
    /// Sector exposure, gradeable holdings only, sorted by total weight
    /// descending; fund exposure folded in at the sector level.
    pub sector_exposure: Vec<SectorExposureRow>,
    /// Gradeable weight whose sector could not be resolved (fail-soft bucket).
    pub unknown_sector_weight: f64,
    pub overlap_clusters: Vec<OverlapCluster>,
    pub not_rated: Vec<NotRatedContribution>,
    pub cash_weight: f64,
    pub top_position_weight: f64,
    /// The typed deferral: price-correlation clustering is not computed — overlap
    /// aggregates at the sector / exposure level only (single-name look-through
    /// off-plan; correlation deferred).
    pub correlation_note: String,
}

// ---- Transition set (carried holdings) -----------------------------------------

/// Ladder position for the stepwise-toward-hold walk.
fn rung_index(a: Action) -> i8 {
    match a {
        Action::SellAll => 0,
        Action::Trim => 1,
        Action::Hold => 2,
        Action::Add => 3,
        Action::AddAggressively => 4,
    }
}

fn action_at(idx: i8) -> Action {
    match idx {
        0 => Action::SellAll,
        1 => Action::Trim,
        2 => Action::Hold,
        3 => Action::Add,
        _ => Action::AddAggressively,
    }
}

/// The carried-action transition set (`docs/portfolio-analysis.md` §Triggering):
/// the roll-up may re-affirm the carried action or move it stepwise **toward
/// *hold***, never away from it on either side of the ladder — with the one
/// carve-out that fresh whole-book aggregates may move a carried *hold* or
/// add-family action to ***trim*** (never *sell all*), gated at validation on a
/// concentration / overlap attribution. Returns the allowed set plus whether
/// `trim` entered only via the carve-out.
pub fn transition_actions(carried: Action) -> (Vec<Action>, bool) {
    let from = rung_index(carried);
    let hold = rung_index(Action::Hold);
    let (lo, hi) = (from.min(hold), from.max(hold));
    let mut set: Vec<Action> = (lo..=hi).map(action_at).collect();
    // Keep ladder order stable: exit side ascending toward add side.
    set.sort_by_key(|a| rung_index(*a));
    let mut carveout = false;
    if matches!(carried, Action::Hold | Action::Add | Action::AddAggressively)
        && !set.contains(&Action::Trim)
    {
        set.insert(0, Action::Trim);
        carveout = true;
    }
    (set, carveout)
}

// ---- OCC option-symbol parsing ---------------------------------------------------

/// Parse an OCC-format option symbol (root + `YYMMDD` + `C`/`P` + 8-digit
/// strike × 1000, spaces tolerated) into `(underlying root, call?, strike)`.
/// `None` on anything else — a consumer then records a typed gap, never a
/// guessed number.
pub fn occ_parts(symbol: &str) -> Option<(String, bool, f64)> {
    let compact: String = symbol.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 16 {
        return None;
    }
    let (root, tail) = compact.split_at(compact.len() - 15);
    let (date, rest) = tail.split_at(6);
    let (cp, strike) = rest.split_at(1);
    if !date.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !matches!(cp, "C" | "P") {
        return None;
    }
    if strike.len() != 8 || !strike.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    strike
        .parse::<f64>()
        .ok()
        .map(|s| (root.to_string(), cp == "C", s / 1000.0))
}

/// The strike alone — the not-rated notional's leg.
pub fn occ_strike(symbol: &str) -> Option<f64> {
    occ_parts(symbol).map(|(_, _, strike)| strike)
}

/// Classify the holdings snapshot's same-underlying option overlay for one
/// equity position (`docs/portfolio-analysis.md` §Portfolio action): each held
/// OCC option row on the same root reads as a **covered call** (short call over
/// a long position) or **protective put** (long put over a long position), with
/// a share-coverage estimate; anything else renders as an unclassified
/// same-underlying option. `None` when no held option shares the underlying.
fn same_underlying_overlay(
    underlying: &crate::schwab::Position,
    positions: &[crate::schwab::Position],
) -> Option<String> {
    let shares = underlying.quantity;
    let mut notes: Vec<String> = Vec::new();
    for p in positions {
        if p.asset_class != AssetClass::OptionContract {
            continue;
        }
        let Some((root, is_call, _)) = occ_parts(&p.symbol) else {
            continue;
        };
        if !root.eq_ignore_ascii_case(&underlying.symbol) {
            continue;
        }
        let label = match (is_call, p.quantity < 0.0, shares > 0.0) {
            (true, true, true) => "covered call",
            (false, false, true) => "protective put",
            _ => "same-underlying option",
        };
        let coverage = if shares > 0.0 {
            format!(
                ", ~{:.0}% of shares",
                (p.quantity.abs() * 100.0 / shares * 100.0).round()
            )
        } else {
            String::new()
        };
        notes.push(format!(
            "{label} ({:.0} contract{}{coverage})",
            p.quantity.abs(),
            if p.quantity.abs() == 1.0 { "" } else { "s" },
        ));
    }
    if notes.is_empty() {
        None
    } else {
        Some(notes.join("; "))
    }
}

// ---- Step 7a: the aggregate builder --------------------------------------------

/// Everything [`build_aggregates`] reads. All references — the builder is pure.
pub struct AggregateInputs<'a> {
    pub holdings: &'a Holdings,
    pub verdicts: &'a [HoldingVerdict],
    pub audits: &'a [HoldingAudit],
    /// The prior run's verdicts (`None` on a first run) — the fresh rows'
    /// action-change baseline.
    pub prior_verdicts: Option<&'a [HoldingVerdict]>,
    /// Carried symbols (uppercase) — a selective run's unselected tail.
    pub carried: &'a HashSet<String>,
    /// Carried symbols the over-age rule applies to (uppercase).
    pub over_age: &'a HashSet<String>,
    /// Fresh-passed stocks' sector labels (uppercase symbol → label), from the
    /// outcome slice's fail-soft profile read.
    pub stock_sectors: &'a HashMap<String, Option<String>>,
    /// Fresh-passed funds' full sector weightings (uppercase symbol → weights).
    pub fund_sector_weights: &'a HashMap<String, Vec<(String, f64)>>,
    /// The episode store — a carried stock's sector rides its latest episode's
    /// entry-stamped identity (the "existing reads, fail-soft" rule; unresolved →
    /// the unknown bucket).
    pub episodes: &'a [DecisionEpisode],
    pub profile: &'a InvestorProfile,
}

/// Build the Step-7a aggregates + spine ([`BookAggregates`]). Deterministic; every
/// number from the payload, the verdicts, and the persisted audits.
pub fn build_aggregates(inp: &AggregateInputs<'_>) -> BookAggregates {
    let total = inp.holdings.account_total;
    let weight_of = |mv: f64| if total > 0.0 { mv / total } else { 0.0 };

    let mut spine: Vec<SizingSpineRow> = Vec::new();
    let mut not_rated: Vec<NotRatedContribution> = Vec::new();
    // sector label → (direct, fund, contributors)
    let mut sectors: BTreeMap<String, (f64, f64, Vec<String>)> = BTreeMap::new();
    let mut unknown_sector_weight = 0.0_f64;

    for position in &inp.holdings.positions {
        let key = position.symbol.to_ascii_uppercase();
        let weight = weight_of(position.market_value);
        let Some(verdict) = inp
            .verdicts
            .iter()
            .find(|v| v.symbol.eq_ignore_ascii_case(&position.symbol))
        else {
            continue;
        };
        let audit = inp
            .audits
            .iter()
            .find(|a| a.symbol.eq_ignore_ascii_case(&position.symbol));
        let carried = inp.carried.contains(&key);
        let over_age = inp.over_age.contains(&key);
        let prior_v = inp
            .prior_verdicts
            .and_then(|pv| pv.iter().find(|v| v.symbol.eq_ignore_ascii_case(&position.symbol)));

        // Sector resolution (fail-soft): fresh stock → profile read; carried
        // stock → latest episode's entry stamp; fund → weightings (fresh) or the
        // persisted top-sector comparator (carried); anything unresolved → the
        // unknown bucket.
        let is_fund = matches!(position.asset_class, AssetClass::Etf | AssetClass::MutualFund);
        let mut row_sector: Option<String> = None;
        if position.asset_class.is_gradeable() && weight != 0.0 {
            if is_fund {
                if let Some(weights) = inp.fund_sector_weights.get(&key) {
                    let mut covered = 0.0;
                    for (sector, w) in weights {
                        covered += w;
                        let entry = sectors.entry(sector.clone()).or_default();
                        entry.1 += weight * w;
                        entry.2.push(position.symbol.clone());
                    }
                    unknown_sector_weight += weight * (1.0 - covered).max(0.0);
                } else if let Some((sector, w)) = audit
                    .and_then(|a| a.fund_exposure.as_ref())
                    .and_then(|f| f.top_sector.clone())
                {
                    // A carried fund's persisted comparator carries the top
                    // sector only; the remainder is honestly unknown.
                    let entry = sectors.entry(sector).or_default();
                    entry.1 += weight * w;
                    entry.2.push(position.symbol.clone());
                    unknown_sector_weight += weight * (1.0 - w).max(0.0);
                } else {
                    unknown_sector_weight += weight;
                }
            } else {
                let label: Option<String> = inp
                    .stock_sectors
                    .get(&key)
                    .cloned()
                    .flatten()
                    .or_else(|| {
                        inp.episodes
                            .iter()
                            .filter(|e| e.symbol.eq_ignore_ascii_case(&position.symbol))
                            .max_by(|a, b| a.anchor_at.cmp(&b.anchor_at))
                            .and_then(|e| e.sector.sector.clone())
                    });
                match &label {
                    Some(sector) => {
                        let entry = sectors.entry(sector.clone()).or_default();
                        entry.0 += weight;
                        entry.2.push(position.symbol.clone());
                    }
                    None => unknown_sector_weight += weight,
                }
                row_sector = label;
            }
        }

        match &verdict.disposition {
            VerdictDisposition::Priced(g) => {
                let overlay_rules = audit
                    .and_then(|a| a.pre_profit.as_ref())
                    .filter(|o| o.is_eligible())
                    .map(|o| &o.consequences);
                let pre_profit_rule = overlay_rules.map(|r| {
                    if r.exit_family_only {
                        "severe deterioration — exit family only".to_string()
                    } else if r.bar_add_family {
                        "constrained runway — add family barred".to_string()
                    } else {
                        "overlay entered — no action rule in force".to_string()
                    }
                });
                let lean = g.lean.unwrap_or(g.action);
                let (offered, carveout) = if carried {
                    transition_actions(g.action)
                } else {
                    let hurdle = audit.and_then(|a| a.hurdle.clone()).unwrap_or_default();
                    (
                        engine::feasible_actions(g.grade, &hurdle, weight, overlay_rules),
                        false,
                    )
                };
                let prior_action = if carried {
                    Some(g.action)
                } else {
                    prior_v.and_then(carried_action)
                };
                let prior_lean = if carried {
                    Some(lean)
                } else {
                    prior_v.and_then(|v| match &v.disposition {
                        VerdictDisposition::Priced(pg) => Some(pg.lean.unwrap_or(pg.action)),
                        _ => None,
                    })
                };
                spine.push(SizingSpineRow {
                    symbol: position.symbol.clone(),
                    asset_class: position.asset_class,
                    branch: SpineBranch::Priced,
                    current_weight: weight,
                    market_value: position.market_value,
                    current_price: position.current_price,
                    concentration_headroom: (engine::MAX_SINGLE_WEIGHT - weight).max(0.0),
                    upside_downside: g.price_targets.twelve_month.as_ref().and_then(|t| {
                        position
                            .current_price
                            .filter(|p| *p > 0.0)
                            .map(|p| t.base / p - 1.0)
                    }),
                    dead_money: g.dead_money,
                    unrealized_pl: Some(position.market_value - position.cost_basis),
                    risk_tier: g.risk_tier,
                    grade: Some(g.grade),
                    conviction: Some(g.conviction),
                    lean: if carried { None } else { Some(lean) },
                    prior_lean,
                    prior_action,
                    position_change: verdict.position_change,
                    carried,
                    over_age,
                    rule_demoted: verdict.action_source == crate::portfolio::ActionSource::RuleDemoted,
                    pre_profit_rule,
                    hard_forensic_bar: false,
                    sector: row_sector,
                    offered,
                    context_trim_carveout: carveout,
                    tax_note: tax_note(inp.profile, position.market_value - position.cost_basis),
                    class_label: None,
                    role_summary: None,
                    expense_drag: None,
                    observable_risk: None,
                    structural_flag: false,
                    exposure_tilt: Vec::new(),
                    evidence_gaps: Vec::new(),
                    option_overlay: same_underlying_overlay(position, &inp.holdings.positions),
                });
            }
            VerdictDisposition::RoleRiskOnly(r) => {
                let (offered, carveout) = if carried {
                    transition_actions(r.action)
                } else {
                    (crate::portfolio::ROLE_RISK_ACTIONS.to_vec(), false)
                };
                let prior_action = if carried {
                    Some(r.action)
                } else {
                    prior_v.and_then(carried_action)
                };
                spine.push(SizingSpineRow {
                    symbol: position.symbol.clone(),
                    asset_class: position.asset_class,
                    branch: SpineBranch::RoleRisk,
                    current_weight: weight,
                    market_value: position.market_value,
                    current_price: position.current_price,
                    concentration_headroom: (engine::MAX_SINGLE_WEIGHT - weight).max(0.0),
                    upside_downside: None,
                    dead_money: None,
                    unrealized_pl: Some(position.market_value - position.cost_basis),
                    risk_tier: None,
                    grade: None,
                    conviction: None,
                    lean: None,
                    prior_lean: None,
                    prior_action,
                    position_change: verdict.position_change,
                    carried,
                    over_age,
                    rule_demoted: verdict.action_source == crate::portfolio::ActionSource::RuleDemoted,
                    pre_profit_rule: None,
                    hard_forensic_bar: false,
                    sector: None,
                    offered,
                    context_trim_carveout: carveout,
                    tax_note: tax_note(inp.profile, position.market_value - position.cost_basis),
                    class_label: Some(r.class_label.clone()),
                    role_summary: Some(r.role_summary.clone()),
                    expense_drag: r.expense_drag,
                    observable_risk: r.observable_risk,
                    structural_flag: r.structural_flag,
                    exposure_tilt: r.exposure_tilt.iter().take(3).cloned().collect(),
                    evidence_gaps: r.evidence_gaps.clone(),
                    option_overlay: same_underlying_overlay(position, &inp.holdings.positions),
                });
            }
            VerdictDisposition::NotRated { .. } => {
                let mut gaps: Vec<String> = Vec::new();
                let signed_notional = match position.asset_class {
                    AssetClass::OptionContract => {
                        gaps.push(
                            "standalone option delta: no on-plan source".to_string(),
                        );
                        match occ_strike(&position.symbol) {
                            Some(strike) => Some(position.quantity * strike * 100.0),
                            None => {
                                gaps.push(
                                    "notional: OCC symbol unparseable".to_string(),
                                );
                                None
                            }
                        }
                    }
                    AssetClass::FixedIncome => {
                        gaps.push(
                            "fixed-income duration / credit: no on-plan source".to_string(),
                        );
                        None
                    }
                    _ => None,
                };
                not_rated.push(NotRatedContribution {
                    symbol: position.symbol.clone(),
                    asset_class: position.asset_class,
                    weight,
                    market_value: position.market_value,
                    signed_notional,
                    material: weight.abs() >= NOT_RATED_MATERIAL_MIN_WEIGHT,
                    gaps,
                });
            }
            VerdictDisposition::InsufficientEvidence { .. } => {
                // An abstention carries no action — nothing to construct; its
                // sector exposure was folded above like any gradeable position.
            }
        }
    }

    let mut sector_exposure: Vec<SectorExposureRow> = sectors
        .into_iter()
        .map(|(sector, (direct, fund, mut holdings))| {
            holdings.dedup();
            SectorExposureRow {
                sector,
                direct_weight: direct,
                fund_weight: fund,
                holdings,
            }
        })
        .collect();
    sector_exposure.sort_by(|a, b| {
        b.total()
            .partial_cmp(&a.total())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let overlap_clusters: Vec<OverlapCluster> = sector_exposure
        .iter()
        .filter(|row| row.holdings.len() >= 2 && row.total() >= OVERLAP_CLUSTER_MIN_WEIGHT)
        .map(|row| OverlapCluster {
            sector: row.sector.clone(),
            combined_weight: row.total(),
            symbols: row.holdings.clone(),
        })
        .collect();

    let top_position_weight = inp
        .holdings
        .positions
        .iter()
        .map(|p| weight_of(p.market_value))
        .fold(0.0_f64, f64::max);

    BookAggregates {
        spine,
        sector_exposure,
        unknown_sector_weight,
        overlap_clusters,
        not_rated,
        cash_weight: weight_of(inp.holdings.cash),
        top_position_weight,
        correlation_note: "price-correlation clustering deferred — overlap aggregates at the \
                           sector / exposure level only (single-name look-through off-plan)"
            .to_string(),
    }
}

fn carried_action(verdict: &HoldingVerdict) -> Option<Action> {
    match &verdict.disposition {
        VerdictDisposition::Priced(g) => Some(g.action),
        VerdictDisposition::RoleRiskOnly(r) => Some(r.action),
        _ => None,
    }
}

fn tax_note(profile: &InvestorProfile, unrealized_pl: f64) -> Option<String> {
    if !profile.tax_sensitive {
        return None;
    }
    Some(if unrealized_pl >= 0.0 {
        "taxable gain if realized (account type / rate unmodeled — a user \
         consideration, never an analytical vote)"
            .to_string()
    } else {
        "possible tax benefit of realizing the loss (account type / rate unmodeled \
         — a user consideration, never an analytical vote)"
            .to_string()
    })
}

// ---- The construction call's response contract ---------------------------------

/// One holding's proposal, as decoded from the model (flat fields — friendlier to
/// grammar-constrained decoding than nested optional objects). Structural claims
/// (`action`, causes, attribution) arrive as strings and are app-validated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingProposalDraft {
    pub action: String,
    pub target_weight_low: f64,
    pub target_weight_high: f64,
    /// The sizing rationale (the card's action rationale line).
    pub rationale: String,
    /// The divergence-from-lean context cause — required (and validated) when the
    /// final action departs a lean the offered set still contains.
    #[serde(default)]
    pub divergence_cause: Option<String>,
    #[serde(default)]
    pub divergence_note: Option<String>,
    /// The action half of the what-changed audit — required when the action
    /// changed against its baseline.
    #[serde(default)]
    pub changed_attribution: Option<String>,
    #[serde(default)]
    pub changed_cause: Option<String>,
    #[serde(default)]
    pub changed_note: Option<String>,
}

/// The construction call's decoded response
/// (`docs/portfolio-workflow.md` §Step 7b Returns).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstructionDraft {
    /// Per-holding proposals, keyed by symbol (exactly the spine's actionable
    /// rows — enforced by the schema's `required` list and re-validated).
    pub holdings: BTreeMap<String, HoldingProposalDraft>,
    pub risk_posture: String,
    /// What to trim to fund which adds — including raising cash from a
    /// dead-money loser, framed high-level.
    pub deployment_stance: String,
    pub concentration_read: String,
    #[serde(default)]
    pub closed_positions_note: Option<String>,
}

/// The validated, persisted portfolio-level view (rides the roll-up).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstructionView {
    pub risk_posture: String,
    pub deployment_stance: String,
    pub concentration_read: String,
    #[serde(default)]
    pub closed_positions_note: Option<String>,
    /// Net new dollars the plan implies = total buys − total disposition proceeds
    /// (trims **and** sell-alls), computed by the joint-feasibility solve — a
    /// negative value is **net cash raised**, not funding
    /// (`docs/portfolio-analysis.md` §Portfolio roll-up and construction). `None`
    /// when the book was unsizable (no account total).
    pub external_funding: Option<f64>,
    /// The implied post-action book total the weights were validated against.
    pub implied_total: Option<f64>,
    /// The single named-violation re-run was used
    /// (`docs/portfolio-workflow.md` §Step 7b).
    #[serde(default)]
    pub retried: bool,
}

/// One holding's validated construction outcome, merged onto its verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedHolding {
    pub action: Action,
    pub target_weight_low: f64,
    pub target_weight_high: f64,
    pub rationale: String,
    pub what_changed: Option<ActionWhatChanged>,
    /// The divergence-from-lean record for the outcome episode (`None` =
    /// matched, or no fresh lean to compare).
    pub lean_divergence: Option<String>,
}

/// The whole validated construction result.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedConstruction {
    /// Keyed by uppercase symbol.
    pub actions: HashMap<String, ValidatedHolding>,
    pub view: ConstructionView,
}

// ---- Joint-feasibility validation ----------------------------------------------

/// One typed violation of the joint-feasibility / attribution contract. The
/// `Display` text is what the single re-run names to the model.
#[derive(Debug, Clone, PartialEq)]
pub enum Violation {
    MissingHolding { symbol: String },
    UnknownHolding { symbol: String },
    UnparseableAction { symbol: String, action: String },
    ActionOutsideOffered { symbol: String, action: Action, offered: Vec<Action> },
    RangeInverted { symbol: String },
    RangeOutsideRungBand { symbol: String, action: Action, low: f64, high: f64, band: (f64, f64) },
    SellAllNonZeroRange { symbol: String },
    ImpliedWeightOutsideRange { symbol: String, implied: f64, low: f64, high: f64 },
    CapBreach { symbol: String, implied: f64 },
    UnfundedBuys { buys: f64, available: f64 },
    ContextTrimUnattributed { symbol: String },
    DivergenceMissing { symbol: String, lean: Action, action: Action },
    UnknownContextCause { symbol: String, cause: String },
    ContextCauseUnsupported { symbol: String, cause: ContextCause },
    WhatChangedMissing { symbol: String, prior: Action, action: Action },
    UnknownAttribution { symbol: String, value: String },
    IntrinsicAttributionUnsupported { symbol: String },
    ContextCauseRequired { symbol: String },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::MissingHolding { symbol } => {
                write!(f, "{symbol}: no proposal — every listed holding needs one")
            }
            Violation::UnknownHolding { symbol } => {
                write!(f, "{symbol}: proposed but not a holding this run constructs")
            }
            Violation::UnparseableAction { symbol, action } => {
                write!(f, "{symbol}: action '{action}' is not a ladder rung")
            }
            Violation::ActionOutsideOffered { symbol, action, offered } => {
                let offered: Vec<&str> = offered.iter().map(Action::as_kebab).collect();
                write!(
                    f,
                    "{symbol}: action '{}' is outside its allowed set [{}]",
                    action.as_kebab(),
                    offered.join(", ")
                )
            }
            Violation::RangeInverted { symbol } => {
                write!(f, "{symbol}: target_weight_low exceeds target_weight_high")
            }
            Violation::RangeOutsideRungBand { symbol, action, low, high, band } => write!(
                f,
                "{symbol}: proposed range [{:.4}, {:.4}] sits outside the '{}' rung's \
                 engine band [{:.4}, {:.4}]",
                low,
                high,
                action.as_kebab(),
                band.0,
                band.1
            ),
            Violation::SellAllNonZeroRange { symbol } => {
                write!(f, "{symbol}: a sell-all target range must be 0–0")
            }
            Violation::ImpliedWeightOutsideRange { symbol, implied, low, high } => write!(
                f,
                "{symbol}: the jointly implied post-action weight {:.4} lands outside the \
                 stated range [{:.4}, {:.4}] — the plan does not hold simultaneously",
                implied, low, high
            ),
            Violation::CapBreach { symbol, implied } => write!(
                f,
                "{symbol}: implied post-action weight {:.4} breaches the 25% concentration cap",
                implied
            ),
            Violation::UnfundedBuys { buys, available } => write!(
                f,
                "buys of {:.0} exceed the trims-plus-available-cash funding of {:.0} \
                 (the profile constrains cash)",
                buys, available
            ),
            Violation::ContextTrimUnattributed { symbol } => write!(
                f,
                "{symbol}: a carried-name context trim needs a validated became-oversized \
                 or overlap-emerged attribution"
            ),
            Violation::DivergenceMissing { symbol, lean, action } => write!(
                f,
                "{symbol}: final action '{}' departs the standalone lean '{}' with no \
                 divergence_cause (became-oversized / overlap-emerged / cash-freed)",
                action.as_kebab(),
                lean.as_kebab()
            ),
            Violation::UnknownContextCause { symbol, cause } => {
                write!(f, "{symbol}: context cause '{cause}' is not in the vocabulary")
            }
            Violation::ContextCauseUnsupported { symbol, cause } => write!(
                f,
                "{symbol}: context cause '{}' maps to no real aggregate (the claim must be \
                 checkable against the whole-book aggregates)",
                cause.as_kebab()
            ),
            Violation::WhatChangedMissing { symbol, prior, action } => write!(
                f,
                "{symbol}: action moved '{}' → '{}' with no changed_attribution \
                 (moved-intrinsic / moved-context)",
                prior.as_kebab(),
                action.as_kebab()
            ),
            Violation::UnknownAttribution { symbol, value } => {
                write!(f, "{symbol}: changed_attribution '{value}' is not in the vocabulary")
            }
            Violation::IntrinsicAttributionUnsupported { symbol } => write!(
                f,
                "{symbol}: moved-intrinsic claimed but the intrinsic read did not move \
                 (the lean and its feasible set are unchanged) — attribute to context or \
                 re-affirm"
            ),
            Violation::ContextCauseRequired { symbol } => write!(
                f,
                "{symbol}: a moved-context attribution needs a changed_cause from the \
                 vocabulary"
            ),
        }
    }
}

/// The joint-feasibility solve + attribution validation
/// (`docs/portfolio-analysis.md` §Portfolio roll-up and construction). Returns the
/// validated result or the typed violation list the single re-run names.
///
/// The solve applies every proposed adjustment at once against the current
/// account total, holds the book identity (position values plus the cash residual
/// account for the implied book, with the profile's stated external-funding
/// assumption an explicit line), and validates each final weight against its
/// stated range and the concentration cap.
pub fn validate_construction(
    draft: &ConstructionDraft,
    agg: &BookAggregates,
    holdings: &Holdings,
    profile: &InvestorProfile,
) -> Result<ValidatedConstruction, Vec<Violation>> {
    let mut violations: Vec<Violation> = Vec::new();
    let total = holdings.account_total;

    let spine_by_symbol: HashMap<String, &SizingSpineRow> = agg
        .spine
        .iter()
        .map(|r| (r.symbol.to_ascii_uppercase(), r))
        .collect();
    // Coverage both ways.
    for row in &agg.spine {
        if !draft
            .holdings
            .keys()
            .any(|k| k.eq_ignore_ascii_case(&row.symbol))
        {
            violations.push(Violation::MissingHolding {
                symbol: row.symbol.clone(),
            });
        }
    }
    for symbol in draft.holdings.keys() {
        if !spine_by_symbol.contains_key(&symbol.to_ascii_uppercase()) {
            violations.push(Violation::UnknownHolding {
                symbol: symbol.clone(),
            });
        }
    }

    // First pass: parse actions + per-holding structural checks + the implied
    // book's buy/sell totals (needed before the cash-freed attribution check).
    struct Parsed<'a> {
        row: &'a SizingSpineRow,
        proposal: &'a HoldingProposalDraft,
        action: Action,
        mid_value: f64,
    }
    let mut parsed: Vec<Parsed<'_>> = Vec::new();
    let mut buys = 0.0_f64;
    let mut sells = 0.0_f64;
    for (symbol, proposal) in &draft.holdings {
        let Some(row) = spine_by_symbol.get(&symbol.to_ascii_uppercase()).copied() else {
            continue;
        };
        let Some(action) = parse_action(&proposal.action) else {
            violations.push(Violation::UnparseableAction {
                symbol: row.symbol.clone(),
                action: proposal.action.clone(),
            });
            continue;
        };
        if !row.offered.contains(&action) {
            violations.push(Violation::ActionOutsideOffered {
                symbol: row.symbol.clone(),
                action,
                offered: row.offered.clone(),
            });
            continue;
        }
        let (low, high) = (proposal.target_weight_low, proposal.target_weight_high);
        if !(low.is_finite() && high.is_finite()) || low > high + STRUCT_EPS {
            violations.push(Violation::RangeInverted {
                symbol: row.symbol.clone(),
            });
            continue;
        }
        if action == Action::SellAll && high > STRUCT_EPS {
            violations.push(Violation::SellAllNonZeroRange {
                symbol: row.symbol.clone(),
            });
            continue;
        }
        let band = engine::rung_band(action, row.current_weight);
        if low < band.0 - STRUCT_EPS || high > band.1 + STRUCT_EPS {
            violations.push(Violation::RangeOutsideRungBand {
                symbol: row.symbol.clone(),
                action,
                low,
                high,
                band,
            });
            continue;
        }
        let mid = (low + high) / 2.0;
        let mid_value = if total > 0.0 { mid * total } else { row.market_value };
        let delta = mid_value - row.market_value;
        if delta > 0.0 {
            buys += delta;
        } else {
            sells += -delta;
        }
        parsed.push(Parsed {
            row,
            proposal,
            action,
            mid_value,
        });
    }

    // The implied post-action book: proposed values at once; disposition proceeds
    // land in cash; buys beyond proceeds are the profile's stated external
    // funding (never observed cash) — `docs/portfolio-analysis.md` §Portfolio
    // roll-up and construction. Note the constrained-cash asymmetry: a profile
    // with `available_cash` gates the buys below but the cash residual is never
    // drawn down here — funded buys still enter the implied book as external
    // growth. Inert under the fixed preset (cash unconstrained); when a
    // configurable profile makes the cash cap real, this solve must draw
    // `implied_cash` down by the cash-funded share of the buys.
    let external_funding = buys - sells;
    let unchanged_value: f64 = holdings
        .positions
        .iter()
        .filter(|p| {
            !parsed
                .iter()
                .any(|x| x.row.symbol.eq_ignore_ascii_case(&p.symbol))
        })
        .map(|p| p.market_value)
        .sum();
    let implied_cash = holdings.cash + (sells - buys).max(0.0);
    let implied_total: f64 =
        parsed.iter().map(|x| x.mid_value).sum::<f64>() + unchanged_value + implied_cash;

    if let Some(available) = profile.available_cash {
        if buys > sells + available + DOLLAR_EPS {
            violations.push(Violation::UnfundedBuys {
                buys,
                available: sells + available,
            });
        }
    }

    // Second pass: implied-weight + cap checks and the attribution validations
    // (which need the plan-level `sells` total for the cash-freed cause).
    let mut actions: HashMap<String, ValidatedHolding> = HashMap::new();
    for x in &parsed {
        let symbol = x.row.symbol.clone();
        let implied_weight = if implied_total > 0.0 {
            x.mid_value / implied_total
        } else {
            0.0
        };
        if implied_weight
            < x.proposal.target_weight_low - WEIGHT_EPS
            || implied_weight > x.proposal.target_weight_high + WEIGHT_EPS
        {
            violations.push(Violation::ImpliedWeightOutsideRange {
                symbol: symbol.clone(),
                implied: implied_weight,
                low: x.proposal.target_weight_low,
                high: x.proposal.target_weight_high,
            });
        }
        if implied_weight > engine::MAX_SINGLE_WEIGHT + WEIGHT_EPS {
            violations.push(Violation::CapBreach {
                symbol: symbol.clone(),
                implied: implied_weight,
            });
        }

        // A context cause's checkable-aggregate validation, shared by divergence
        // and what-changed claims.
        let cause_supported = |cause: ContextCause| -> bool {
            match cause {
                ContextCause::BecameOversized => x.row.current_weight >= OVERSIZED_MIN_WEIGHT,
                ContextCause::OverlapEmerged => agg
                    .overlap_clusters
                    .iter()
                    .any(|c| c.symbols.iter().any(|s| s.eq_ignore_ascii_case(&symbol))),
                // Freed cash supports an add-side move only, and only when the
                // plan actually raises proceeds.
                ContextCause::CashFreed => {
                    sells > DOLLAR_EPS
                        && x.row
                            .lean
                            .map(|lean| rung_index(x.action) > rung_index(lean))
                            .unwrap_or(false)
                }
            }
        };

        // Divergence from the standalone lean — fresh priced rows only (a
        // carried row's lean is stale, and `role_risk_only` has none).
        let mut lean_divergence: Option<String> = None;
        if let (false, Some(lean)) = (x.row.carried, x.row.lean) {
            if x.action != lean {
                if !x.row.offered.contains(&lean) {
                    // The engine bar is app-known — stamped deterministically,
                    // no model attribution required.
                    lean_divergence = Some(format!(
                        "engine-bar: lean '{}' outside the feasible set",
                        lean.as_kebab()
                    ));
                } else {
                    match x.proposal.divergence_cause.as_deref().map(str::trim) {
                        None | Some("") => violations.push(Violation::DivergenceMissing {
                            symbol: symbol.clone(),
                            lean,
                            action: x.action,
                        }),
                        Some(raw) => match ContextCause::parse(raw) {
                            None => violations.push(Violation::UnknownContextCause {
                                symbol: symbol.clone(),
                                cause: raw.to_string(),
                            }),
                            Some(cause) if !cause_supported(cause) => {
                                violations.push(Violation::ContextCauseUnsupported {
                                    symbol: symbol.clone(),
                                    cause,
                                })
                            }
                            Some(cause) => {
                                lean_divergence =
                                    Some(format!("portfolio-context ({})", cause.as_kebab()));
                            }
                        },
                    }
                }
            }
        }

        // The action half of the what-changed audit — required when the action
        // moved against its baseline (`docs/portfolio-analysis.md` §What changed).
        let mut what_changed: Option<ActionWhatChanged> = None;
        if let Some(prior) = x.row.prior_action {
            if x.action != prior {
                // A **reversion to an unchanged lean** is app-stamped, never a
                // model claim: the prior action diverged from the lean on
                // portfolio context (a construction-era divergence, or a prior
                // exit call the fresh pass no longer supports), and the final
                // action re-converging to a lean that did not move means that
                // context lapsed — a state the closed cause vocabulary cannot
                // express and the app can verify deterministically.
                let reverted_to_lean = !x.row.carried
                    && x.row.lean == Some(x.action)
                    && x.row.lean == x.row.prior_lean;
                if reverted_to_lean {
                    what_changed = Some(ActionWhatChanged {
                        attribution: ActionAttribution::MovedContext,
                        cause: None,
                        note: "final action re-converged to the standalone lean — the \
                               prior divergence's portfolio context no longer binds"
                            .to_string(),
                    });
                    actions.insert(
                        symbol.to_ascii_uppercase(),
                        ValidatedHolding {
                            action: x.action,
                            target_weight_low: x.proposal.target_weight_low,
                            target_weight_high: x.proposal.target_weight_high,
                            rationale: x.proposal.rationale.clone(),
                            what_changed,
                            lean_divergence,
                        },
                    );
                    continue;
                }
                // The carried-name context-trim carve-out: `trim` reached the
                // offered set only through the carve-out, so the claim must be a
                // validated concentration / overlap attribution — cash-freed
                // never licenses it (`docs/portfolio-analysis.md` §Triggering).
                let carveout_trim =
                    x.row.carried && x.action == Action::Trim && x.row.context_trim_carveout;
                match x.proposal.changed_attribution.as_deref().map(str::trim) {
                    None | Some("") => violations.push(Violation::WhatChangedMissing {
                        symbol: symbol.clone(),
                        prior,
                        action: x.action,
                    }),
                    Some("moved-intrinsic") => {
                        // Valid only when the intrinsic read actually moved: the
                        // lean departed its prior value, or the feasible set no
                        // longer offers the prior action (an intrinsic-input move
                        // — dead money, a grade drop, an overlay rule).
                        let intrinsic_moved = !x.row.carried
                            && (x.row.lean != x.row.prior_lean
                                || !x.row.offered.contains(&prior));
                        if carveout_trim || !intrinsic_moved {
                            violations.push(if carveout_trim {
                                Violation::ContextTrimUnattributed {
                                    symbol: symbol.clone(),
                                }
                            } else {
                                Violation::IntrinsicAttributionUnsupported {
                                    symbol: symbol.clone(),
                                }
                            });
                        } else {
                            what_changed = Some(ActionWhatChanged {
                                attribution: ActionAttribution::MovedIntrinsic,
                                cause: None,
                                note: x
                                    .proposal
                                    .changed_note
                                    .clone()
                                    .unwrap_or_default(),
                            });
                        }
                    }
                    Some("moved-context") => {
                        match x.proposal.changed_cause.as_deref().map(str::trim) {
                            None | Some("") => violations.push(Violation::ContextCauseRequired {
                                symbol: symbol.clone(),
                            }),
                            Some(raw) => match ContextCause::parse(raw) {
                                None => violations.push(Violation::UnknownContextCause {
                                    symbol: symbol.clone(),
                                    cause: raw.to_string(),
                                }),
                                Some(cause) => {
                                    let carveout_ok = !carveout_trim
                                        || matches!(
                                            cause,
                                            ContextCause::BecameOversized
                                                | ContextCause::OverlapEmerged
                                        );
                                    if !cause_supported(cause) || !carveout_ok {
                                        violations.push(if carveout_ok {
                                            Violation::ContextCauseUnsupported {
                                                symbol: symbol.clone(),
                                                cause,
                                            }
                                        } else {
                                            Violation::ContextTrimUnattributed {
                                                symbol: symbol.clone(),
                                            }
                                        });
                                    } else {
                                        what_changed = Some(ActionWhatChanged {
                                            attribution: ActionAttribution::MovedContext,
                                            cause: Some(cause),
                                            note: x
                                                .proposal
                                                .changed_note
                                                .clone()
                                                .unwrap_or_default(),
                                        });
                                    }
                                }
                            },
                        }
                    }
                    Some(other) => violations.push(Violation::UnknownAttribution {
                        symbol: symbol.clone(),
                        value: other.to_string(),
                    }),
                }
            }
        }

        actions.insert(
            symbol.to_ascii_uppercase(),
            ValidatedHolding {
                action: x.action,
                target_weight_low: x.proposal.target_weight_low,
                target_weight_high: x.proposal.target_weight_high,
                rationale: x.proposal.rationale.clone(),
                what_changed,
                lean_divergence,
            },
        );
    }

    if !violations.is_empty() {
        return Err(violations);
    }
    Ok(ValidatedConstruction {
        actions,
        view: ConstructionView {
            risk_posture: draft.risk_posture.trim().to_string(),
            deployment_stance: draft.deployment_stance.trim().to_string(),
            concentration_read: draft.concentration_read.trim().to_string(),
            closed_positions_note: draft
                .closed_positions_note
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            external_funding: (total > 0.0).then_some(external_funding),
            implied_total: (total > 0.0).then_some(implied_total),
            retried: false,
        },
    })
}

// ---- The construction merge ----------------------------------------------------

/// Merge the validated construction result onto the run's verdicts: the final
/// action, the range-derived sizing (deltas recomputed deterministically — one
/// home, [`engine::sizing_from_range`]), and the action half of the what-changed
/// audit. Returns the per-symbol divergence-from-lean records for the outcome
/// episodes (uppercase symbol → line).
///
/// Two per-row rules ride the merge:
/// - **A construction-moved action is a model decision.** Where a carried action
///   had been rule-demoted (the over-age add → hold rule) and the model validly
///   moved it off the demoted rung (the context-trim carve-out), the demotion
///   stamp no longer describes the final action — `action_source` restores to
///   `model-chosen`, so the rule-demoted episode class holds only actions a rule
///   actually set (`docs/portfolio-analysis.md` §Outcome learning).
/// - **A carried row's stale lean is stamped, never left ambiguous.** Carried
///   rows skip divergence validation (their lean was authored at an older
///   vintage), so where the stale lean differs from the final action the app
///   stamps a typed `carried-stale-lean` record — keeping `None` = matched on
///   the episode's `lean_divergence` contract.
pub fn merge_validated_actions(
    verdicts: &mut [HoldingVerdict],
    actions: &HashMap<String, ValidatedHolding>,
    holdings: &Holdings,
    profile: &InvestorProfile,
    carried: &HashSet<String>,
) -> HashMap<String, String> {
    let mut lean_divergence_by_symbol: HashMap<String, String> = HashMap::new();
    for v in verdicts.iter_mut() {
        let key = v.symbol.to_ascii_uppercase();
        let Some(vh) = actions.get(&key) else {
            continue;
        };
        let Some(position) = holdings
            .positions
            .iter()
            .find(|p| p.symbol.eq_ignore_ascii_case(&v.symbol))
        else {
            continue;
        };
        let mut sizing = engine::sizing_from_range(
            vh.target_weight_low,
            vh.target_weight_high,
            position,
            profile,
            holdings.account_total,
        );
        sizing.sizing_rationale = Some(vh.rationale.clone());
        let pre = match &mut v.disposition {
            VerdictDisposition::Priced(g) => {
                let pre = g.action;
                g.action = vh.action;
                g.action_sizing = sizing;
                g.action_what_changed = vh.what_changed.clone();
                Some(pre)
            }
            VerdictDisposition::RoleRiskOnly(r) => {
                let pre = r.action;
                r.action = vh.action;
                r.action_sizing = sizing;
                r.action_what_changed = vh.what_changed.clone();
                Some(pre)
            }
            _ => None,
        };
        if v.action_source == ActionSource::RuleDemoted
            && pre.is_some_and(|p| p != vh.action)
        {
            v.action_source = ActionSource::ModelChosen;
        }
        if let Some(d) = &vh.lean_divergence {
            lean_divergence_by_symbol.insert(key, d.clone());
        } else if carried.contains(&key) {
            if let VerdictDisposition::Priced(g) = &v.disposition {
                if g.lean.is_some_and(|l| l != vh.action) {
                    lean_divergence_by_symbol.insert(
                        key,
                        "carried-stale-lean: lean authored at an older vintage — not \
                         reconciled against this run's aggregates"
                            .to_string(),
                    );
                }
            }
        }
    }
    lean_divergence_by_symbol
}

fn parse_action(s: &str) -> Option<Action> {
    match s.trim() {
        "sell-all" => Some(Action::SellAll),
        "trim" => Some(Action::Trim),
        "hold" => Some(Action::Hold),
        "add" => Some(Action::Add),
        "add-aggressively" => Some(Action::AddAggressively),
        _ => None,
    }
}

// ---- The construction schema ---------------------------------------------------

/// The JSON Schema handed to Ollama's `format` for the construction call — one
/// required property per actionable holding, each holding's `action` enum listing
/// exactly its allowed set (feasible / transition / reduced spine), so a barred
/// rung is structurally unreachable, mirroring the 6f narrowing
/// (`docs/portfolio-workflow.md` §Step 7b).
pub fn construction_schema(spine: &[SizingSpineRow]) -> Value {
    let causes = ["became-oversized", "overlap-emerged", "cash-freed"];
    let mut cause_or_null: Vec<Value> = causes.iter().map(|c| json!(c)).collect();
    cause_or_null.push(Value::Null);
    let attribution_or_null = vec![json!("moved-intrinsic"), json!("moved-context"), Value::Null];

    let mut holding_props = serde_json::Map::new();
    let mut required_symbols: Vec<Value> = Vec::new();
    for row in spine {
        let offered: Vec<&str> = row.offered.iter().map(Action::as_kebab).collect();
        holding_props.insert(
            row.symbol.clone(),
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": offered },
                    "target_weight_low": { "type": "number" },
                    "target_weight_high": { "type": "number" },
                    "rationale": { "type": "string" },
                    "divergence_cause": { "type": ["string", "null"], "enum": cause_or_null },
                    "divergence_note": { "type": ["string", "null"] },
                    "changed_attribution": { "type": ["string", "null"], "enum": attribution_or_null },
                    "changed_cause": { "type": ["string", "null"], "enum": cause_or_null },
                    "changed_note": { "type": ["string", "null"] }
                },
                "required": [
                    "action", "target_weight_low", "target_weight_high", "rationale",
                    "divergence_cause", "divergence_note",
                    "changed_attribution", "changed_cause", "changed_note"
                ]
            }),
        );
        required_symbols.push(json!(row.symbol));
    }

    json!({
        "type": "object",
        "properties": {
            "holdings": {
                "type": "object",
                "properties": Value::Object(holding_props),
                "required": required_symbols
            },
            "risk_posture": { "type": "string" },
            "deployment_stance": { "type": "string" },
            "concentration_read": { "type": "string" },
            "closed_positions_note": { "type": ["string", "null"] }
        },
        "required": [
            "holdings", "risk_posture", "deployment_stance",
            "concentration_read", "closed_positions_note"
        ]
    })
}

// ---- Prompt construction (pure, testable) --------------------------------------

/// The construction call's system prompt.
pub fn construction_system_prompt() -> String {
    "You are the portfolio-construction stage of a prescriptive portfolio review. \
     Every holding has already been analyzed in isolation — its grade, conviction, \
     scenario targets, and a STANDALONE ACTION LEAN (what the action would be if the \
     holding stood alone). Your job is the one judgment that needs the whole book: \
     reconcile each holding's lean against the whole-book aggregates — concentration, \
     sector exposure and overlap clusters, cash, the not-rated positions' exposure — \
     into its FINAL ACTION and a target portfolio-weight range, and write the \
     portfolio-level view. Express every target weight as a DECIMAL FRACTION of the \
     book (write 0.065 for 6.5%). Rules you must hold: choose each action only from \
     that holding's ALLOWED set (a carried holding's set enforces the transition \
     rule — toward hold only, plus a context trim that needs a real concentration or \
     overlap reason). Each allowed action is listed with its engine band as a \
     fraction range — keep target_weight_low/high inside the chosen action's band \
     (a sell-all range is 0–0) — and propose weights that can hold SIMULTANEOUSLY, \
     each under the single-position concentration cap: the app solves the implied \
     post-action book and rejects a jointly infeasible plan. Where your final action \
     departs a holding's lean, say why with a divergence_cause from the vocabulary; \
     where an action changed against its baseline, attribute it (moved-intrinsic or \
     moved-context with a cause) — every context claim is checked against the real \
     aggregates. A dead-money loser is a legitimate source of redeployable cash: \
     raising cash from one may cite the possible tax benefit of realizing the loss \
     and the redeployment optionality of the proceeds as supporting rationale, framed \
     high-level (the user acts on the specifics). Do NOT invent numbers: every figure \
     you cite must come from the aggregates given. Respond only with the required \
     JSON object."
        .to_string()
}

/// Render one spine row's digest line for the user prompt.
fn spine_digest(row: &SizingSpineRow) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("weight {:.1}%", row.current_weight * 100.0));
    if let Some(g) = row.grade {
        parts.push(format!("grade {}", g.as_str()));
    }
    if let Some(c) = row.conviction {
        parts.push(format!("conviction {c:?}").to_lowercase());
    }
    if let Some(l) = row.lean {
        parts.push(format!("lean {}", l.as_kebab()));
    }
    // The moved-intrinsic attribution's comparison baseline — the model can't
    // attribute a change to a moved lean it can't see.
    if let (false, Some(p)) = (row.carried, row.prior_lean) {
        if row.lean != Some(p) {
            parts.push(format!("prior lean {}", p.as_kebab()));
        }
    }
    if let Some(u) = row.upside_downside {
        parts.push(format!("12m base {:+.1}%", u * 100.0));
    }
    if let Some(d) = row.dead_money {
        if d == HurdleState::Fails {
            parts.push("DEAD MONEY (hurdle fails)".to_string());
        }
    }
    if let Some(t) = row.risk_tier {
        parts.push(format!("tier {}", t.as_str()));
    }
    if let Some(pl) = row.unrealized_pl {
        parts.push(format!("unrealized P/L {:+.0}", pl));
    }
    if let Some(s) = &row.sector {
        parts.push(format!("sector {s}"));
    }
    // The role_risk_only decision surface — 7b is this branch's sole action
    // author, so the verdict's reads ride the digest.
    if let Some(cl) = &row.class_label {
        parts.push(format!("class: {cl}"));
    }
    if let Some(rs) = &row.role_summary {
        parts.push(format!("role: {rs}"));
    }
    if let Some(e) = row.expense_drag {
        parts.push(format!("expense drag {:.2}%/yr", e * 100.0));
    }
    if let Some(v) = row.observable_risk {
        parts.push(format!("realized vol {:.0}%", v * 100.0));
    }
    if row.structural_flag {
        parts.push("structurally path-dependent".to_string());
    }
    if !row.exposure_tilt.is_empty() {
        let tilt: Vec<String> = row
            .exposure_tilt
            .iter()
            .map(|w| format!("{} {:.0}%", w.label, w.weight * 100.0))
            .collect();
        parts.push(format!("tilt: {}", tilt.join(", ")));
    }
    if !row.evidence_gaps.is_empty() {
        parts.push(format!("evidence gaps: {}", row.evidence_gaps.join("; ")));
    }
    if let Some(o) = &row.option_overlay {
        parts.push(format!("overlay: {o}"));
    }
    if row.carried {
        parts.push(format!(
            "CARRIED verdict{}{}",
            if row.over_age { " (over-age)" } else { "" },
            if row.rule_demoted {
                ", action rule-demoted to hold"
            } else {
                ""
            }
        ));
        if let Some(p) = row.prior_action {
            parts.push(format!("carried action {}", p.as_kebab()));
        }
    } else if let Some(p) = row.prior_action {
        parts.push(format!("prior action {}", p.as_kebab()));
    }
    if let Some(rule) = &row.pre_profit_rule {
        parts.push(format!("pre-profit: {rule}"));
    }
    if let Some(t) = &row.tax_note {
        parts.push(format!("tax: {t}"));
    }
    // Each allowed action with its engine band at this row's current weight, as
    // decimal fractions of the book — the numeric bounds the contract holds the
    // model to (`docs/portfolio-workflow.md` §Step 7b: the model must not guess
    // the bands it is validated against).
    let offered: Vec<String> = row
        .offered
        .iter()
        .map(|a| {
            let (lo, hi) = engine::rung_band(*a, row.current_weight);
            format!("{} {:.4}\u{2013}{:.4}", a.as_kebab(), lo, hi)
        })
        .collect();
    parts.push(format!("ALLOWED [{}]", offered.join(", ")));
    if row.context_trim_carveout {
        parts.push("trim allowed only with a became-oversized / overlap-emerged attribution".into());
    }
    format!("- {}: {}\n", row.symbol, parts.join("; "))
}

/// The construction call's user prompt: the aggregates, the per-holding digests,
/// the exited names, the house view, and the investor profile — plus, on the
/// single re-run, the named violations of the failed attempt.
pub fn construction_user_prompt(
    agg: &BookAggregates,
    exited: &[ExitedPosition],
    house_view_sections: Option<&str>,
    profile: &InvestorProfile,
    violations: Option<&str>,
) -> String {
    let mut p = String::new();
    if let Some(v) = violations {
        p.push_str(&format!(
            "VALIDATION FAILURE — your previous proposal violated the construction \
             contract. Fix every violation below and return the corrected full plan:\n{v}\n\n"
        ));
    }
    p.push_str(&format!(
        "PORTFOLIO: cash {:.1}% of the book; largest position {:.1}%; \
         single-position concentration cap {:.0}% ({:.2} as a fraction).\n",
        agg.cash_weight * 100.0,
        agg.top_position_weight * 100.0,
        engine::MAX_SINGLE_WEIGHT * 100.0,
        engine::MAX_SINGLE_WEIGHT,
    ));
    if !agg.sector_exposure.is_empty() {
        p.push_str("\nSECTOR EXPOSURE (direct + fund-folded):\n");
        for row in &agg.sector_exposure {
            p.push_str(&format!(
                "- {}: {:.1}% (direct {:.1}%, via funds {:.1}%) — {}\n",
                row.sector,
                row.total() * 100.0,
                row.direct_weight * 100.0,
                row.fund_weight * 100.0,
                row.holdings.join(", ")
            ));
        }
        if agg.unknown_sector_weight > 0.0 {
            p.push_str(&format!(
                "- (unresolved sector): {:.1}%\n",
                agg.unknown_sector_weight * 100.0
            ));
        }
    }
    if !agg.overlap_clusters.is_empty() {
        p.push_str("\nOVERLAP CLUSTERS (holdings sharing one exposure — size down together):\n");
        for c in &agg.overlap_clusters {
            p.push_str(&format!(
                "- {} at {:.1}%: {}\n",
                c.sector,
                c.combined_weight * 100.0,
                c.symbols.join(", ")
            ));
        }
    }
    if !agg.not_rated.is_empty() {
        p.push_str("\nNOT-RATED POSITIONS (graded nowhere, but real exposure):\n");
        for n in &agg.not_rated {
            p.push_str(&format!(
                "- {} ({:?}): {:.1}% of the book{}{}{}\n",
                n.symbol,
                n.asset_class,
                n.weight * 100.0,
                n.signed_notional
                    .map(|x| format!(", signed notional {x:.0}"))
                    .unwrap_or_default(),
                if n.material { "" } else { " (immaterial)" },
                if n.gaps.is_empty() {
                    String::new()
                } else {
                    format!("; gaps: {}", n.gaps.join(", "))
                },
            ));
        }
    }
    p.push_str(&format!("\nNOTE: {}\n", agg.correlation_note));

    p.push_str("\nHOLDINGS (choose each final action from its ALLOWED set):\n");
    for row in &agg.spine {
        p.push_str(&spine_digest(row));
    }

    if !exited.is_empty() {
        let names: Vec<&str> = exited.iter().map(|e| e.symbol.as_str()).collect();
        p.push_str(&format!(
            "\nPOSITIONS CLOSED SINCE LAST RUN (acknowledge in closed_positions_note): {}\n",
            names.join(", ")
        ));
    }
    if let Some(sections) = house_view_sections {
        p.push_str(&format!(
            "\nMARKET SIGNAL HOUSE VIEW (market-setup context for the portfolio-level \
             view — never by itself a per-holding exit reason):\n{sections}\n"
        ));
    }
    p.push_str(&format!(
        "\nINVESTOR PROFILE: risk tolerance {:?}, horizon {:?}, taxable {}, cash {}\n",
        profile.risk_tolerance,
        profile.horizon,
        profile.tax_sensitive,
        profile
            .available_cash
            .map(|c| format!("{c:.0}"))
            .unwrap_or_else(|| "unconstrained (external funding is the profile's stated \
                                assumption — the plan's net new dollars are computed and \
                                shown)".to_string()),
    ));
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{ActionSource, InvestorProfile};
    use crate::schwab::Position;

    fn position(symbol: &str, asset_class: AssetClass, mv: f64) -> Position {
        Position {
            symbol: symbol.into(),
            description: symbol.into(),
            asset_class,
            quantity: 100.0,
            cost_basis: mv * 0.8,
            market_value: mv,
            current_price: Some(mv / 100.0),
        }
    }

    fn spine_row(symbol: &str, weight: f64, offered: Vec<Action>) -> SizingSpineRow {
        SizingSpineRow {
            symbol: symbol.into(),
            asset_class: AssetClass::Stock,
            branch: SpineBranch::Priced,
            current_weight: weight,
            market_value: weight * 100_000.0,
            current_price: Some(100.0),
            concentration_headroom: (engine::MAX_SINGLE_WEIGHT - weight).max(0.0),
            upside_downside: Some(0.1),
            dead_money: Some(HurdleState::Indeterminate),
            unrealized_pl: Some(1_000.0),
            risk_tier: Some(RiskTier::Medium),
            grade: Some(Grade::B),
            conviction: Some(Conviction::Medium),
            lean: Some(Action::Hold),
            prior_lean: Some(Action::Hold),
            prior_action: Some(Action::Hold),
            position_change: PositionChange::Unchanged,
            carried: false,
            over_age: false,
            rule_demoted: false,
            pre_profit_rule: None,
            hard_forensic_bar: false,
            sector: Some("Technology".into()),
            class_label: None,
            role_summary: None,
            expense_drag: None,
            observable_risk: None,
            structural_flag: false,
            exposure_tilt: Vec::new(),
            evidence_gaps: Vec::new(),
            option_overlay: None,
            offered,
            context_trim_carveout: false,
            tax_note: None,
        }
    }

    fn holdings_for(spine: &[SizingSpineRow], cash: f64) -> Holdings {
        let positions: Vec<Position> = spine
            .iter()
            .map(|r| Position {
                symbol: r.symbol.clone(),
                description: r.symbol.clone(),
                asset_class: r.asset_class,
                quantity: 100.0,
                cost_basis: r.market_value * 0.8,
                market_value: r.market_value,
                current_price: r.current_price,
            })
            .collect();
        let account_total = positions.iter().map(|p| p.market_value).sum::<f64>() + cash;
        Holdings {
            positions,
            cash,
            account_total,
            source_rows: vec![],
        }
    }

    fn agg_for(spine: Vec<SizingSpineRow>) -> BookAggregates {
        BookAggregates {
            spine,
            sector_exposure: vec![],
            unknown_sector_weight: 0.0,
            overlap_clusters: vec![],
            not_rated: vec![],
            cash_weight: 0.1,
            top_position_weight: 0.2,
            correlation_note: "deferred".into(),
        }
    }

    fn hold_proposal(weight: f64) -> HoldingProposalDraft {
        let (low, high) = engine::rung_band(Action::Hold, weight);
        HoldingProposalDraft {
            action: "hold".into(),
            target_weight_low: low,
            target_weight_high: high,
            rationale: "re-affirm".into(),
            divergence_cause: None,
            divergence_note: None,
            changed_attribution: None,
            changed_cause: None,
            changed_note: None,
        }
    }

    fn draft_for(entries: Vec<(&str, HoldingProposalDraft)>) -> ConstructionDraft {
        ConstructionDraft {
            holdings: entries.into_iter().map(|(s, p)| (s.to_string(), p)).collect(),
            risk_posture: "balanced".into(),
            deployment_stance: "no changes".into(),
            concentration_read: "no breaches".into(),
            closed_positions_note: None,
        }
    }

    // ---- transition sets --------------------------------------------------------

    #[test]
    fn transition_sets_move_toward_hold_only_with_the_trim_carveout() {
        let (set, carve) = transition_actions(Action::AddAggressively);
        assert_eq!(
            set,
            vec![Action::Trim, Action::Hold, Action::Add, Action::AddAggressively]
        );
        assert!(carve, "trim reachable only via the carve-out");

        let (set, carve) = transition_actions(Action::Add);
        assert_eq!(set, vec![Action::Trim, Action::Hold, Action::Add]);
        assert!(carve);

        let (set, carve) = transition_actions(Action::Hold);
        assert_eq!(set, vec![Action::Trim, Action::Hold]);
        assert!(carve);

        // Exit-family carries reach hold without any carve-out — and sell-all is
        // never synthesized from a carried trim.
        let (set, carve) = transition_actions(Action::Trim);
        assert_eq!(set, vec![Action::Trim, Action::Hold]);
        assert!(!carve);
        let (set, carve) = transition_actions(Action::SellAll);
        assert_eq!(set, vec![Action::SellAll, Action::Trim, Action::Hold]);
        assert!(!carve);
    }

    // ---- OCC notional -----------------------------------------------------------

    #[test]
    fn occ_strike_parses_standard_and_compact_forms() {
        // Standard OCC: padded root + YYMMDD + C/P + 8-digit strike ×1000.
        assert_eq!(occ_strike("AAPL  260117C00200000"), Some(200.0));
        assert_eq!(occ_strike("SPXW260117P04500500"), Some(4500.5));
        assert_eq!(occ_strike("AAPL"), None);
        assert_eq!(occ_strike("AAPL 260117X00200000"), None);
    }

    // ---- the solver -------------------------------------------------------------

    #[test]
    fn a_reaffirming_plan_validates_with_zero_external_funding() {
        let spine = vec![
            spine_row("AAA", 0.10, vec![Action::SellAll, Action::Trim, Action::Hold]),
            spine_row("BBB", 0.20, vec![Action::SellAll, Action::Trim, Action::Hold]),
        ];
        let holdings = holdings_for(&spine, 70_000.0 * 100.0 / 100.0);
        // holdings_for: mv are weight*100k → AAA 10k, BBB 20k, cash 7000000/100? — use
        // explicit cash so weights match the rows' stated fractions.
        let holdings = Holdings {
            cash: 70_000.0,
            account_total: 100_000.0,
            ..holdings
        };
        let agg = agg_for(spine);
        let draft = draft_for(vec![
            ("AAA", hold_proposal(0.10)),
            ("BBB", hold_proposal(0.20)),
        ]);
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
            .expect("plan validates");
        assert_eq!(out.view.external_funding, Some(0.0));
        assert_eq!(out.actions.len(), 2);
        assert_eq!(out.actions["AAA"].action, Action::Hold);
        assert!(out.actions["AAA"].what_changed.is_none());
        assert!(out.actions["AAA"].lean_divergence.is_none());
    }

    #[test]
    fn external_funding_is_negative_when_the_plan_raises_cash() {
        let mut row = spine_row("AAA", 0.20, vec![Action::SellAll, Action::Trim, Action::Hold]);
        row.lean = Some(Action::Trim);
        row.prior_lean = Some(Action::Trim);
        row.prior_action = Some(Action::Trim);
        let spine = vec![row];
        let holdings = Holdings {
            cash: 80_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let (low, high) = engine::rung_band(Action::Trim, 0.20);
        let draft = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "trim".into(),
                target_weight_low: low,
                target_weight_high: high,
                rationale: "trim the oversized sleeve".into(),
                divergence_cause: None,
                divergence_note: None,
                changed_attribution: None,
                changed_cause: None,
                changed_note: None,
            },
        )]);
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
            .expect("plan validates");
        let funding = out.view.external_funding.unwrap();
        assert!(funding < 0.0, "a net trim raises cash: {funding}");
        // The implied book total is unchanged — proceeds land in cash.
        assert!((out.view.implied_total.unwrap() - 100_000.0).abs() < 1.0);
    }

    #[test]
    fn a_range_outside_the_rung_band_is_a_named_violation() {
        let spine = vec![spine_row("AAA", 0.10, vec![Action::SellAll, Action::Trim, Action::Hold])];
        let holdings = Holdings {
            cash: 90_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let draft = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "hold".into(),
                // Hold band at 10% weight is [9%, 11%] — 20% is outside it.
                target_weight_low: 0.18,
                target_weight_high: 0.20,
                rationale: "x".into(),
                divergence_cause: None,
                divergence_note: None,
                changed_attribution: None,
                changed_cause: None,
                changed_note: None,
            },
        )]);
        let violations =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::RangeOutsideRungBand { symbol, .. } if symbol == "AAA")));
        // The Display text names the band, so the re-run can fix it.
        let text = violations[0].to_string();
        assert!(text.contains("engine band"), "{text}");
    }

    #[test]
    fn structural_checks_use_the_tight_epsilon_not_the_drift_tolerance() {
        // A sell-all retaining 0.4% of the book and a range inverted by 0.4%
        // both sat inside the implied-book drift tolerance (0.5%); the
        // per-holding structural checks trip on anything past decimal rounding.
        let spine = vec![spine_row("AAA", 0.10, vec![Action::SellAll, Action::Trim, Action::Hold])];
        let holdings = Holdings {
            cash: 90_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let sell_all_residue = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "sell-all".into(),
                target_weight_low: 0.0,
                target_weight_high: 0.004,
                rationale: "x".into(),
                divergence_cause: None,
                divergence_note: None,
                changed_attribution: None,
                changed_cause: None,
                changed_note: None,
            },
        )]);
        let violations =
            validate_construction(&sell_all_residue, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::SellAllNonZeroRange { symbol } if symbol == "AAA")));

        let inverted = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "hold".into(),
                target_weight_low: 0.104,
                target_weight_high: 0.100,
                rationale: "x".into(),
                divergence_cause: None,
                divergence_note: None,
                changed_attribution: None,
                changed_cause: None,
                changed_note: None,
            },
        )]);
        let violations =
            validate_construction(&inverted, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::RangeInverted { symbol } if symbol == "AAA")));
    }

    #[test]
    fn a_missing_holding_and_an_unknown_symbol_are_violations() {
        let spine = vec![spine_row("AAA", 0.10, vec![Action::Hold])];
        let holdings = Holdings {
            cash: 90_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let draft = draft_for(vec![("ZZZ", hold_proposal(0.10))]);
        let violations =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::MissingHolding { symbol } if symbol == "AAA")));
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::UnknownHolding { symbol } if symbol == "ZZZ")));
    }

    #[test]
    fn an_action_outside_the_offered_set_is_a_violation() {
        // A carried hold's transition set is {trim*, hold} — 'add' is barred.
        let mut row = spine_row("AAA", 0.10, transition_actions(Action::Hold).0);
        row.carried = true;
        row.context_trim_carveout = true;
        row.lean = None;
        let spine = vec![row];
        let holdings = Holdings {
            cash: 90_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let (low, high) = engine::rung_band(Action::Add, 0.10);
        let draft = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "add".into(),
                target_weight_low: low,
                target_weight_high: high,
                rationale: "x".into(),
                divergence_cause: None,
                divergence_note: None,
                changed_attribution: None,
                changed_cause: None,
                changed_note: None,
            },
        )]);
        let violations =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::ActionOutsideOffered { symbol, .. } if symbol == "AAA")));
    }

    #[test]
    fn a_carried_context_trim_needs_an_oversized_or_overlap_attribution() {
        let (offered, carve) = transition_actions(Action::Hold);
        let mut row = spine_row("AAA", 0.18, offered);
        row.carried = true;
        row.context_trim_carveout = carve;
        row.lean = None;
        row.prior_action = Some(Action::Hold);
        let spine = vec![row];
        let holdings = Holdings {
            cash: 82_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let (low, high) = engine::rung_band(Action::Trim, 0.18);
        let base = HoldingProposalDraft {
            action: "trim".into(),
            target_weight_low: low,
            target_weight_high: high,
            rationale: "concentration".into(),
            divergence_cause: None,
            divergence_note: None,
            changed_attribution: None,
            changed_cause: None,
            changed_note: None,
        };
        // No attribution at all → missing what-changed.
        let draft = draft_for(vec![("AAA", base.clone())]);
        let violations =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::WhatChangedMissing { symbol, .. } if symbol == "AAA")));

        // A cash-freed attribution never licenses the carve-out trim.
        let mut wrong = base.clone();
        wrong.changed_attribution = Some("moved-context".into());
        wrong.changed_cause = Some("cash-freed".into());
        wrong.changed_note = Some("freed cash".into());
        let draft = draft_for(vec![("AAA", wrong)]);
        let violations =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::ContextTrimUnattributed { symbol } if symbol == "AAA")));

        // became-oversized on an 18% position is checkable-true → validates.
        let mut ok = base;
        ok.changed_attribution = Some("moved-context".into());
        ok.changed_cause = Some("became-oversized".into());
        ok.changed_note = Some("18% of the book".into());
        let draft = draft_for(vec![("AAA", ok)]);
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
            .expect("validated carve-out trim");
        let wc = out.actions["AAA"].what_changed.as_ref().unwrap();
        assert_eq!(wc.attribution, ActionAttribution::MovedContext);
        assert_eq!(wc.cause, Some(ContextCause::BecameOversized));
    }

    #[test]
    fn a_reversion_to_an_unchanged_lean_is_app_stamped_not_model_claimed() {
        // Prior action trim, lean hold then and now (a lapsed context divergence):
        // the fresh pass re-converges to the lean — the app stamps the
        // moved-context record itself and ignores the model's attribution fields.
        let mut row = spine_row("AAA", 0.05, vec![Action::SellAll, Action::Trim, Action::Hold]);
        row.lean = Some(Action::Hold);
        row.prior_lean = Some(Action::Hold);
        row.prior_action = Some(Action::Trim);
        let spine = vec![row];
        let holdings = Holdings {
            cash: 95_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let mut proposal = hold_proposal(0.05);
        // A model mis-claim on the reversion path is ignored, not a violation.
        proposal.changed_attribution = Some("moved-intrinsic".into());
        let draft = draft_for(vec![("AAA", proposal)]);
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
            .expect("the reversion validates without a model attribution");
        let wc = out.actions["AAA"].what_changed.as_ref().unwrap();
        assert_eq!(wc.attribution, ActionAttribution::MovedContext);
        assert_eq!(wc.cause, None);
        assert!(wc.note.contains("re-converged"), "{}", wc.note);
    }

    #[test]
    fn an_oversized_claim_on_a_small_position_is_rejected() {
        let mut row = spine_row("AAA", 0.03, vec![Action::SellAll, Action::Trim, Action::Hold]);
        row.lean = Some(Action::Hold);
        row.prior_action = Some(Action::Hold);
        let spine = vec![row];
        let holdings = Holdings {
            cash: 97_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let (low, high) = engine::rung_band(Action::Trim, 0.03);
        let draft = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "trim".into(),
                target_weight_low: low,
                target_weight_high: high,
                rationale: "x".into(),
                divergence_cause: Some("became-oversized".into()),
                divergence_note: None,
                changed_attribution: Some("moved-context".into()),
                changed_cause: Some("became-oversized".into()),
                changed_note: None,
            },
        )]);
        let violations =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations.iter().any(|v| matches!(
            v,
            Violation::ContextCauseUnsupported { symbol, cause: ContextCause::BecameOversized }
                if symbol == "AAA"
        )));
    }

    #[test]
    fn a_divergence_from_an_offered_lean_needs_a_cause_and_an_engine_bar_is_stamped() {
        // Lean hold, offered includes hold, model picks trim → needs a cause.
        let mut row = spine_row("AAA", 0.16, vec![Action::SellAll, Action::Trim, Action::Hold]);
        row.lean = Some(Action::Hold);
        row.prior_lean = Some(Action::Hold);
        row.prior_action = None; // new holding — no what-changed leg.
        let spine = vec![row];
        let holdings = Holdings {
            cash: 84_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine.clone());
        let (low, high) = engine::rung_band(Action::Trim, 0.16);
        let mut proposal = HoldingProposalDraft {
            action: "trim".into(),
            target_weight_low: low,
            target_weight_high: high,
            rationale: "x".into(),
            divergence_cause: None,
            divergence_note: None,
            changed_attribution: None,
            changed_cause: None,
            changed_note: None,
        };
        let draft = draft_for(vec![("AAA", proposal.clone())]);
        let violations =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::DivergenceMissing { symbol, .. } if symbol == "AAA")));

        proposal.divergence_cause = Some("became-oversized".into());
        let draft = draft_for(vec![("AAA", proposal)]);
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
            .expect("attributed divergence validates");
        assert_eq!(
            out.actions["AAA"].lean_divergence.as_deref(),
            Some("portfolio-context (became-oversized)")
        );

        // A lean the feasible set bars is an app-stamped engine-bar divergence —
        // no model attribution required.
        let mut barred = spine_row("BBB", 0.10, vec![Action::SellAll, Action::Trim, Action::Hold]);
        barred.lean = Some(Action::Add);
        barred.prior_action = None;
        let spine2 = vec![barred];
        let holdings2 = Holdings {
            cash: 90_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine2, 0.0)
        };
        let agg2 = agg_for(spine2);
        let draft2 = draft_for(vec![("BBB", hold_proposal(0.10))]);
        let out2 =
            validate_construction(&draft2, &agg2, &holdings2, &InvestorProfile::default_fixture())
                .expect("engine-bar divergence validates without model attribution");
        let d = out2.actions["BBB"].lean_divergence.as_deref().unwrap();
        assert!(d.starts_with("engine-bar:"), "{d}");
    }

    #[test]
    fn an_implied_cap_breach_is_a_violation() {
        // A 24% position proposed to hold (band up to ~26.4% clamps at 25%) whose
        // upper range the model pushes past the cap.
        let mut row = spine_row("AAA", 0.24, vec![Action::SellAll, Action::Trim, Action::Hold]);
        row.lean = Some(Action::Hold);
        let spine = vec![row];
        let holdings = Holdings {
            cash: 76_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let draft = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "hold".into(),
                // Hold band at 24%: [21.6%, 25%] (clamped) — proposing the top.
                target_weight_low: 0.26,
                target_weight_high: 0.28,
                rationale: "x".into(),
                divergence_cause: None,
                divergence_note: None,
                changed_attribution: None,
                changed_cause: None,
                changed_note: None,
            },
        )]);
        let violations =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        // Both the band check and (were it to pass) the cap check would catch it;
        // the band check fires first.
        assert!(!violations.is_empty());
    }

    #[test]
    fn constrained_cash_gates_unfunded_buys() {
        let mut row = spine_row("AAA", 0.02, vec![Action::Hold, Action::Add]);
        row.lean = Some(Action::Add);
        row.prior_lean = Some(Action::Add);
        row.prior_action = Some(Action::Add);
        let spine = vec![row];
        let holdings = Holdings {
            cash: 98_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let (low, high) = engine::rung_band(Action::Add, 0.02);
        let draft = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "add".into(),
                target_weight_low: low,
                target_weight_high: high,
                rationale: "add".into(),
                divergence_cause: None,
                divergence_note: None,
                changed_attribution: None,
                changed_cause: None,
                changed_note: None,
            },
        )]);
        // Unconstrained preset: the buy is external funding, not a violation.
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
            .expect("unconstrained cash admits the buy");
        assert!(out.view.external_funding.unwrap() > 0.0);
        // A constraining profile with no cash gates it.
        let constrained = InvestorProfile {
            available_cash: Some(0.0),
            ..InvestorProfile::default_fixture()
        };
        let violations = validate_construction(&draft, &agg, &holdings, &constrained).unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::UnfundedBuys { .. })));
    }

    // ---- the schema -------------------------------------------------------------

    #[test]
    fn construction_schema_narrows_each_holding_to_its_offered_set() {
        let spine = vec![
            spine_row("AAA", 0.10, vec![Action::SellAll, Action::Trim, Action::Hold]),
            spine_row("BBB", 0.05, transition_actions(Action::Add).0),
        ];
        let schema = construction_schema(&spine);
        let req: Vec<&str> = schema["properties"]["holdings"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(req, vec!["AAA", "BBB"]);
        let aaa: Vec<&str> = schema["properties"]["holdings"]["properties"]["AAA"]["properties"]
            ["action"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(aaa, vec!["sell-all", "trim", "hold"]);
        let bbb: Vec<&str> = schema["properties"]["holdings"]["properties"]["BBB"]["properties"]
            ["action"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(bbb, vec!["trim", "hold", "add"]);
        // The top level requires the portfolio view.
        let top: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(top.contains(&"risk_posture"));
        assert!(top.contains(&"deployment_stance"));
    }

    // ---- aggregates -------------------------------------------------------------

    #[test]
    fn aggregates_fold_fund_sectors_and_flag_overlap_clusters() {
        use crate::portfolio::{
            ActionSizing, GradedVerdict, HoldingVerdict, HorizonOutlook, HorizonRead,
            OptionsSignal, PriceTargets, SubScores,
        };
        let graded = |symbol: &str, action: Action| HoldingVerdict {
            symbol: symbol.into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::Priced(Box::new(GradedVerdict {
                grade: Grade::B,
                sub_scores: SubScores { quality: 70.0, valuation: 60.0, momentum: 55.0, risk: 65.0 },
                action,
                lean: Some(action),
                action_sizing: ActionSizing {
                    target_weight_low: 0.0,
                    target_weight_high: 0.0,
                    est_share_delta: None,
                    est_dollar_delta: None,
                    sizing_rationale: None,
                },
                conviction: Conviction::Medium,
                horizon_outlook: HorizonOutlook {
                    short: HorizonRead::Neutral,
                    mid: HorizonRead::Neutral,
                    long: HorizonRead::Neutral,
                },
                price_targets: PriceTargets { one_month: None, twelve_month: None },
                price_target_rationale: String::new(),
                options_signal: OptionsSignal {
                    put_call_volume: None,
                    put_call_open_interest: None,
                    implied_volatility: None,
                    iv_skew: None,
                },
                risk_tier: Some(RiskTier::Medium),
                dead_money: Some(HurdleState::Indeterminate),
                low_confidence_grade: false,
                fund_class_label: None,
                structural_flag: false,
                financial_summary: String::new(),
                what_changed: String::new(),
                action_what_changed: None,
            })),
            thesis_ledger: None,
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
        };
        let mut fund_verdict = graded("FND", Action::Hold);
        fund_verdict.asset_class = AssetClass::Etf;
        let option_verdict = HoldingVerdict {
            symbol: "AAPL  260117C00200000".into(),
            asset_class: AssetClass::OptionContract,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated { reason: "option".into() },
            thesis_ledger: None,
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
        };
        let verdicts = vec![graded("AAA", Action::Hold), fund_verdict, option_verdict];
        let holdings = Holdings {
            positions: vec![
                position("AAA", AssetClass::Stock, 20_000.0),
                position("FND", AssetClass::Etf, 20_000.0),
                position("AAPL  260117C00200000", AssetClass::OptionContract, 6_000.0),
            ],
            cash: 54_000.0,
            account_total: 100_000.0,
            source_rows: vec![],
        };
        let stock_sectors: HashMap<String, Option<String>> =
            [("AAA".to_string(), Some("Technology".to_string()))].into();
        let fund_weights: HashMap<String, Vec<(String, f64)>> = [(
            "FND".to_string(),
            vec![("Technology".to_string(), 0.5), ("Financials".to_string(), 0.3)],
        )]
        .into();
        let carried = HashSet::new();
        let over_age = HashSet::new();
        let agg = build_aggregates(&AggregateInputs {
            holdings: &holdings,
            verdicts: &verdicts,
            audits: &[],
            prior_verdicts: None,
            carried: &carried,
            over_age: &over_age,
            stock_sectors: &stock_sectors,
            fund_sector_weights: &fund_weights,
            episodes: &[],
            profile: &InvestorProfile::default_fixture(),
        });
        // Technology: 20% direct + 20%×0.5 fund-folded = 30% — a two-contributor
        // cluster above the 20% threshold.
        let tech = agg
            .sector_exposure
            .iter()
            .find(|r| r.sector == "Technology")
            .unwrap();
        assert!((tech.direct_weight - 0.20).abs() < 1e-9);
        assert!((tech.fund_weight - 0.10).abs() < 1e-9);
        assert_eq!(tech.holdings, vec!["AAA", "FND"]);
        assert_eq!(agg.overlap_clusters.len(), 1);
        assert_eq!(agg.overlap_clusters[0].sector, "Technology");
        // The fund's uncovered 20% share of its weight rides the unknown bucket.
        assert!((agg.unknown_sector_weight - 0.20 * 0.2).abs() < 1e-9);
        // The option position contributes its notional and typed gaps.
        assert_eq!(agg.not_rated.len(), 1);
        let opt = &agg.not_rated[0];
        assert_eq!(opt.signed_notional, Some(100.0 * 200.0 * 100.0));
        assert!(opt.material, "6% of the book is above the 5% bar");
        assert!(opt.gaps.iter().any(|g| g.contains("delta")));
        // Spine rows exist for the two gradeable holdings only.
        assert_eq!(agg.spine.len(), 2);
        assert!(agg.spine.iter().all(|r| !r.offered.is_empty()));
    }

    #[test]
    fn carried_rows_get_transition_sets_and_fresh_rows_feasible_sets() {
        use crate::portfolio::{
            ActionSizing, GradedVerdict, HoldingVerdict, HorizonOutlook, HorizonRead,
            OptionsSignal, PriceTargets, SubScores,
        };
        let graded = |symbol: &str, action: Action| HoldingVerdict {
            symbol: symbol.into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::Priced(Box::new(GradedVerdict {
                grade: Grade::B,
                sub_scores: SubScores { quality: 70.0, valuation: 60.0, momentum: 55.0, risk: 65.0 },
                action,
                lean: Some(action),
                action_sizing: ActionSizing {
                    target_weight_low: 0.0,
                    target_weight_high: 0.0,
                    est_share_delta: None,
                    est_dollar_delta: None,
                    sizing_rationale: None,
                },
                conviction: Conviction::Medium,
                horizon_outlook: HorizonOutlook {
                    short: HorizonRead::Neutral,
                    mid: HorizonRead::Neutral,
                    long: HorizonRead::Neutral,
                },
                price_targets: PriceTargets { one_month: None, twelve_month: None },
                price_target_rationale: String::new(),
                options_signal: OptionsSignal {
                    put_call_volume: None,
                    put_call_open_interest: None,
                    implied_volatility: None,
                    iv_skew: None,
                },
                risk_tier: Some(RiskTier::Medium),
                dead_money: Some(HurdleState::Indeterminate),
                low_confidence_grade: false,
                fund_class_label: None,
                structural_flag: false,
                financial_summary: String::new(),
                what_changed: String::new(),
                action_what_changed: None,
            })),
            thesis_ledger: None,
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
        };
        let verdicts = vec![graded("AAA", Action::Add), graded("BBB", Action::Hold)];
        let holdings = Holdings {
            positions: vec![
                position("AAA", AssetClass::Stock, 10_000.0),
                position("BBB", AssetClass::Stock, 10_000.0),
            ],
            cash: 80_000.0,
            account_total: 100_000.0,
            source_rows: vec![],
        };
        let carried: HashSet<String> = ["AAA".to_string()].into();
        let over_age = HashSet::new();
        let stock_sectors = HashMap::new();
        let fund_weights = HashMap::new();
        let agg = build_aggregates(&AggregateInputs {
            holdings: &holdings,
            verdicts: &verdicts,
            audits: &[],
            prior_verdicts: None,
            carried: &carried,
            over_age: &over_age,
            stock_sectors: &stock_sectors,
            fund_sector_weights: &fund_weights,
            episodes: &[],
            profile: &InvestorProfile::default_fixture(),
        });
        let aaa = agg.spine.iter().find(|r| r.symbol == "AAA").unwrap();
        assert!(aaa.carried);
        assert_eq!(aaa.offered, transition_actions(Action::Add).0);
        assert!(aaa.context_trim_carveout);
        assert_eq!(aaa.prior_action, Some(Action::Add));
        assert!(aaa.lean.is_none(), "a carried row's lean is stale — not offered");
        let bbb = agg.spine.iter().find(|r| r.symbol == "BBB").unwrap();
        assert!(!bbb.carried);
        // No audit → default (unscorable) hurdle → the add family is not offered.
        assert_eq!(bbb.offered, vec![Action::SellAll, Action::Trim, Action::Hold]);
        // With no sector sources both stocks ride the unknown bucket.
        assert!((agg.unknown_sector_weight - 0.20).abs() < 1e-9);
    }

    #[test]
    fn role_risk_rows_carry_their_decision_surface_and_overlays_classify() {
        use crate::portfolio::{ActionSizing, RoleRiskVerdict};
        let role_risk = HoldingVerdict {
            symbol: "BND".into(),
            asset_class: AssetClass::Etf,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::RoleRiskOnly(Box::new(RoleRiskVerdict {
                class_label: "bond fund".into(),
                role_summary: "core fixed-income sleeve".into(),
                exposure_tilt: vec![
                    ExposureWeight { label: "Treasuries".into(), weight: 0.6 },
                    ExposureWeight { label: "Corporates".into(), weight: 0.25 },
                    ExposureWeight { label: "MBS".into(), weight: 0.1 },
                    ExposureWeight { label: "Cash".into(), weight: 0.05 },
                ],
                expense_drag: Some(0.0035),
                observable_risk: Some(0.06),
                structural_flag: false,
                evidence_gaps: vec!["no constituent look-through".into()],
                action: Action::Hold,
                action_sizing: ActionSizing {
                    target_weight_low: 0.0,
                    target_weight_high: 0.0,
                    est_share_delta: None,
                    est_dollar_delta: None,
                    sizing_rationale: None,
                },
                what_changed: String::new(),
                action_what_changed: None,
            })),
            thesis_ledger: None,
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
        };
        let equity = merge_verdict("AAPL", Action::Hold, Some(Action::Hold), ActionSource::ModelChosen);
        let verdicts = vec![role_risk, equity];
        // A short call on the held equity — 1 contract over 100 shares. The
        // option position needs no verdict to classify: the overlay scans the
        // snapshot's positions directly.
        let mut short_call = position("AAPL  260117C00200000", AssetClass::OptionContract, 500.0);
        short_call.quantity = -1.0;
        let holdings = Holdings {
            positions: vec![
                position("BND", AssetClass::Etf, 10_000.0),
                position("AAPL", AssetClass::Stock, 20_000.0),
                short_call,
            ],
            cash: 69_500.0,
            account_total: 100_000.0,
            source_rows: vec![],
        };
        let carried = HashSet::new();
        let over_age = HashSet::new();
        let stock_sectors = HashMap::new();
        let fund_weights = HashMap::new();
        let agg = build_aggregates(&AggregateInputs {
            holdings: &holdings,
            verdicts: &verdicts,
            audits: &[],
            prior_verdicts: None,
            carried: &carried,
            over_age: &over_age,
            stock_sectors: &stock_sectors,
            fund_sector_weights: &fund_weights,
            episodes: &[],
            profile: &InvestorProfile::default_fixture(),
        });

        let bnd = agg.spine.iter().find(|r| r.symbol == "BND").unwrap();
        assert_eq!(bnd.branch, SpineBranch::RoleRisk);
        assert_eq!(bnd.class_label.as_deref(), Some("bond fund"));
        assert_eq!(bnd.role_summary.as_deref(), Some("core fixed-income sleeve"));
        assert_eq!(bnd.expense_drag, Some(0.0035));
        assert_eq!(bnd.observable_risk, Some(0.06));
        assert_eq!(bnd.exposure_tilt.len(), 3, "tilt capped at three");
        assert_eq!(bnd.evidence_gaps.len(), 1);
        let digest = spine_digest(bnd);
        assert!(digest.contains("class: bond fund"), "{digest}");
        assert!(digest.contains("role: core fixed-income sleeve"), "{digest}");
        assert!(digest.contains("expense drag 0.35%/yr"), "{digest}");
        assert!(digest.contains("realized vol 6%"), "{digest}");
        assert!(digest.contains("tilt: Treasuries 60%, Corporates 25%, MBS 10%"), "{digest}");
        assert!(digest.contains("evidence gaps: no constituent look-through"), "{digest}");

        // The equity row reads the covered call: 1 contract × 100 over 100
        // shares (the fixture's quantity) = ~100% of shares.
        let aapl = agg.spine.iter().find(|r| r.symbol == "AAPL").unwrap();
        let overlay = aapl.option_overlay.as_deref().unwrap();
        assert!(overlay.contains("covered call"), "{overlay}");
        assert!(overlay.contains("~100% of shares"), "{overlay}");
        assert!(spine_digest(aapl).contains("overlay: covered call"));
        // The fund shares no underlying with the option.
        assert!(bnd.option_overlay.is_none());
    }

    #[test]
    fn a_long_put_over_a_long_position_reads_protective() {
        let equity = position("MSFT", AssetClass::Stock, 40_000.0);
        let long_put = position("MSFT  260117P00300000", AssetClass::OptionContract, 800.0);
        let positions = vec![equity.clone(), long_put];
        let overlay = same_underlying_overlay(&equity, &positions).unwrap();
        assert!(overlay.contains("protective put"), "{overlay}");
        // A long call over a long position is neither pattern.
        let equity2 = position("NVDA", AssetClass::Stock, 40_000.0);
        let long_call = position("NVDA  260117C00900000", AssetClass::OptionContract, 800.0);
        let positions2 = vec![equity2.clone(), long_call];
        let overlay2 = same_underlying_overlay(&equity2, &positions2).unwrap();
        assert!(overlay2.contains("same-underlying option"), "{overlay2}");
    }

    // ---- prompts ---------------------------------------------------------------

    #[test]
    fn construction_prompts_carry_the_load_bearing_content() {
        let sys = construction_system_prompt();
        assert!(sys.contains("STANDALONE ACTION LEAN"));
        assert!(sys.contains("toward hold only"));
        assert!(sys.contains("SIMULTANEOUSLY"));
        assert!(sys.contains("DECIMAL FRACTION"));

        let mut row = spine_row("AAA", 0.18, vec![Action::SellAll, Action::Trim, Action::Hold]);
        row.tax_note = Some("taxable gain if realized".into());
        let mut agg = agg_for(vec![row]);
        agg.overlap_clusters.push(OverlapCluster {
            sector: "Technology".into(),
            combined_weight: 0.3,
            symbols: vec!["AAA".into(), "FND".into()],
        });
        let exited = vec![ExitedPosition {
            symbol: "GONE".into(),
            description: "Gone Inc".into(),
            prior_quantity: 10.0,
            prior_cost_basis: 1_000.0,
            prior_market_value: 1_200.0,
        }];
        let profile = InvestorProfile::default_fixture();
        let p = construction_user_prompt(&agg, &exited, Some("House view text"), &profile, None);
        assert!(p.contains("- AAA:"));
        // Each allowed action carries its numeric engine band at the row's
        // current weight (0.18): the model is validated against these bounds,
        // so it must see them.
        assert!(
            p.contains("ALLOWED [sell-all 0.0000\u{2013}0.0000, trim 0.0720\u{2013}0.1260, hold 0.1620\u{2013}0.1980]"),
            "{p}"
        );
        assert!(p.contains("single-position concentration cap 25% (0.25 as a fraction)"));
        assert!(p.contains("OVERLAP CLUSTERS"));
        assert!(p.contains("GONE"));
        assert!(p.contains("House view text"));
        assert!(p.contains("unconstrained"));
        assert!(!p.contains("VALIDATION FAILURE"));

        let retry = construction_user_prompt(
            &agg,
            &exited,
            Some("House view text"),
            &profile,
            Some("AAA: action 'add' is outside its allowed set"),
        );
        assert!(retry.starts_with("VALIDATION FAILURE"));
        assert!(retry.contains("outside its allowed set"));
    }

    // ---- the construction merge ---------------------------------------------------

    fn merge_verdict(
        symbol: &str,
        action: Action,
        lean: Option<Action>,
        source: ActionSource,
    ) -> HoldingVerdict {
        use crate::portfolio::{
            ActionSizing, GradedVerdict, HoldingVerdict, HorizonOutlook, HorizonRead,
            OptionsSignal, PriceTargets, SubScores,
        };
        HoldingVerdict {
            symbol: symbol.into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::Priced(Box::new(GradedVerdict {
                grade: Grade::B,
                sub_scores: SubScores { quality: 70.0, valuation: 60.0, momentum: 55.0, risk: 65.0 },
                action,
                lean,
                action_sizing: ActionSizing {
                    target_weight_low: 0.0,
                    target_weight_high: 0.0,
                    est_share_delta: None,
                    est_dollar_delta: None,
                    sizing_rationale: None,
                },
                conviction: Conviction::Medium,
                horizon_outlook: HorizonOutlook {
                    short: HorizonRead::Neutral,
                    mid: HorizonRead::Neutral,
                    long: HorizonRead::Neutral,
                },
                price_targets: PriceTargets { one_month: None, twelve_month: None },
                price_target_rationale: String::new(),
                options_signal: OptionsSignal {
                    put_call_volume: None,
                    put_call_open_interest: None,
                    implied_volatility: None,
                    iv_skew: None,
                },
                risk_tier: Some(RiskTier::Medium),
                dead_money: Some(HurdleState::Indeterminate),
                low_confidence_grade: false,
                fund_class_label: None,
                structural_flag: false,
                financial_summary: String::new(),
                what_changed: String::new(),
                action_what_changed: None,
            })),
            thesis_ledger: None,
            analyzed_at: None,
            action_source: source,
        }
    }

    fn merge_fixture(action: Action) -> (Holdings, HashMap<String, ValidatedHolding>) {
        let holdings = Holdings {
            positions: vec![position("AAA", AssetClass::Stock, 10_000.0)],
            cash: 90_000.0,
            account_total: 100_000.0,
            source_rows: vec![],
        };
        let actions: HashMap<String, ValidatedHolding> = [(
            "AAA".to_string(),
            ValidatedHolding {
                action,
                target_weight_low: 0.04,
                target_weight_high: 0.07,
                rationale: "concentration".into(),
                what_changed: None,
                lean_divergence: None,
            },
        )]
        .into();
        (holdings, actions)
    }

    #[test]
    fn merge_restores_model_chosen_when_construction_moves_a_demoted_action() {
        // The over-age demoted hold, moved to trim by a validated carve-out:
        // the final action is a model decision, so the demotion stamp no longer
        // applies — and the carried stale lean (add ≠ trim) gets the app stamp.
        let (holdings, actions) = merge_fixture(Action::Trim);
        let mut verdicts = vec![merge_verdict(
            "AAA",
            Action::Hold,
            Some(Action::Add),
            ActionSource::RuleDemoted,
        )];
        let carried: HashSet<String> = ["AAA".to_string()].into();
        let profile = InvestorProfile::default_fixture();
        let divergence =
            merge_validated_actions(&mut verdicts, &actions, &holdings, &profile, &carried);
        let v = &verdicts[0];
        assert_eq!(v.action_source, ActionSource::ModelChosen);
        match &v.disposition {
            VerdictDisposition::Priced(g) => {
                assert_eq!(g.action, Action::Trim);
                assert_eq!(g.action_sizing.target_weight_low, 0.04);
                assert_eq!(g.action_sizing.sizing_rationale.as_deref(), Some("concentration"));
            }
            _ => panic!("expected priced"),
        }
        assert!(divergence.get("AAA").unwrap().starts_with("carried-stale-lean:"));
    }

    #[test]
    fn merge_keeps_rule_demoted_on_a_reaffirmed_demoted_hold() {
        // Construction re-affirms the demoted hold: no model decision moved the
        // action, so the demotion stamp stands; the stale lean still stamps.
        let (holdings, actions) = merge_fixture(Action::Hold);
        let mut verdicts = vec![merge_verdict(
            "AAA",
            Action::Hold,
            Some(Action::Add),
            ActionSource::RuleDemoted,
        )];
        let carried: HashSet<String> = ["AAA".to_string()].into();
        let profile = InvestorProfile::default_fixture();
        let divergence =
            merge_validated_actions(&mut verdicts, &actions, &holdings, &profile, &carried);
        assert_eq!(verdicts[0].action_source, ActionSource::RuleDemoted);
        assert!(divergence.get("AAA").unwrap().starts_with("carried-stale-lean:"));
    }

    #[test]
    fn merge_stamps_nothing_for_a_fresh_matched_row() {
        // A fresh row whose validated divergence is `None` (matched) stays
        // unstamped — `None` = matched holds on the episode contract.
        let (holdings, actions) = merge_fixture(Action::Hold);
        let mut verdicts = vec![merge_verdict(
            "AAA",
            Action::Hold,
            Some(Action::Hold),
            ActionSource::ModelChosen,
        )];
        let carried: HashSet<String> = HashSet::new();
        let profile = InvestorProfile::default_fixture();
        let divergence =
            merge_validated_actions(&mut verdicts, &actions, &holdings, &profile, &carried);
        assert!(divergence.is_empty());
        assert_eq!(verdicts[0].action_source, ActionSource::ModelChosen);
    }
}
