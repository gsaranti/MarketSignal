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
//! per-holding **action-sizing spine rows**, each carrying the **engine action
//! set**: the engine's feasible set for a fresh holding, the **carried-action
//! transition set** (toward *hold* only, plus the aggregate-gated context-trim
//! carve-out) for a carried one — since `portfolio-v7` prompt evidence and the
//! annotation bound, never a schema bar.
//!
//! **Step 7b is the construction model call** (built in [`super::pipeline`] /
//! [`super::job`] over this module's contract): the 122B reconciles each holding's
//! standalone lean against the aggregates into its final action + target-weight
//! range and the portfolio-level view. This module owns the **schema**
//! ([`construction_schema`] — per-holding action enums are structural), the
//! **prompt text**, and the deterministic **joint-feasibility validation**
//! ([`validate_construction`]) — split two ways since `portfolio-v7`: the
//! self-coherence checks (sell-all 0–0, range ordering,
//! stated-range-contains-implied-weight) and the app-validated action-half
//! attributions still return violations — the caller re-runs the synthesis once
//! with them named, and persisting incoherence fails the run — while the
//! engine-bound checks (rung band, concentration cap, funding, the transition
//! rule) record as `engine_bound_annotations`, never a violation
//! (`docs/portfolio-analysis.md` §Portfolio roll-up and construction).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::portfolio::engine;
use crate::portfolio::outcome::DecisionEpisode;
use crate::portfolio::{
    carried_action, Action, ActionAttribution, ActionSource, ActionWhatChanged, AssetClass,
    ContextCause, Conviction, ExitedPosition, ExposureWeight, Grade, HoldingAudit, HoldingVerdict,
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

/// Rounding tolerance for the **rung-band bound** check alone: the bands are
/// printed into the prompt at four decimals, so a model echoing a printed edge
/// can sit half a print-step off the exact engine value. Range ordering and the
/// sell-all zero range compare the model's own numbers against each other /
/// against literal zero — no printed value enters — so those checks are exact.
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
/// — the engine-known decision surface the construction call reads, persisted
/// with the roll-up so the chosen actions stay auditable against the engine's
/// read they were chosen beside (annotation bounds since `portfolio-v7`).
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
    /// The standalone lean (priced branch; `None` on `role_risk_only` — and
    /// `None` on a carried row, whose stale lean rides `prior_lean` and the
    /// carried verdict instead, so the divergence machinery applies to fresh
    /// rows only).
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
    /// The engine action set: the feasible set (fresh), the transition set
    /// (carried), or the reduced set (fresh `role_risk_only`) — rendered as the
    /// prompt's ENGINE SET, annotation-bounding only since `portfolio-v7`.
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

/// The carried-action transition set (`docs/portfolio-analysis.md` §Triggering) —
/// the engine's rule for a carried holding: re-affirm the carried action or move
/// it stepwise **toward *hold***, never away from it on either side of the ladder
/// — with the one carve-out that fresh whole-book aggregates may move a carried
/// *hold* or add-family action to ***trim*** (never *sell all*), gated at
/// validation on a concentration / overlap attribution. Since `portfolio-v7` the
/// set binds the engine arm alone — it renders as the carried row's ENGINE SET,
/// an outside-the-set choice persisting with an engine-bound annotation. Returns
/// the engine set plus whether `trim` entered only via the carve-out.
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

/// One parsed OCC option identity.
pub struct OccOption {
    pub root: String,
    /// Expiry as `YYYY-MM-DD` (OCC dates are post-2000 by construction).
    pub expiry: String,
    pub is_call: bool,
    pub strike: f64,
}

/// Parse an OCC-format option symbol (root + `YYMMDD` + `C`/`P` + 8-digit
/// strike × 1000, spaces tolerated). `None` on anything else — a consumer then
/// records a typed gap, never a guessed number.
pub fn occ_parts(symbol: &str) -> Option<OccOption> {
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
    strike.parse::<f64>().ok().map(|s| OccOption {
        root: root.to_string(),
        expiry: format!("20{}-{}-{}", &date[0..2], &date[2..4], &date[4..6]),
        is_call: cp == "C",
        strike: s / 1000.0,
    })
}

/// The strike alone — the not-rated notional's leg.
pub fn occ_strike(symbol: &str) -> Option<f64> {
    occ_parts(symbol).map(|o| o.strike)
}

/// Classify the holdings snapshot's same-underlying option overlay for one
/// equity position (`docs/portfolio-analysis.md` §The per-holding pipeline —
/// the dossier's typed-overlay contract, honored here from the snapshot's OCC
/// rows: covered-call / protective-put / collar / other, **a naked or partial
/// short call never reads as covered**, coverage ratio + strike / expiry
/// carried; per-leg delta stays unavailable at 7a — no chain read here).
/// `None` when no held option shares the underlying.
fn same_underlying_overlay(
    underlying: &crate::schwab::Position,
    positions: &[crate::schwab::Position],
) -> Option<String> {
    let shares = underlying.quantity;
    // Zero-net rows survive holdings normalization by design — a zero leg is
    // no exposure, so it never enters the overlay.
    let legs: Vec<(OccOption, f64)> = positions
        .iter()
        .filter(|p| p.asset_class == AssetClass::OptionContract)
        .filter_map(|p| occ_parts(&p.symbol).map(|o| (o, p.quantity)))
        .filter(|(o, q)| *q != 0.0 && o.root.eq_ignore_ascii_case(&underlying.symbol))
        .collect();
    if legs.is_empty() {
        return None;
    }
    // Aggregate the two hedge-shaped sides, keeping every leg's strike / expiry
    // (the doc contract is per-leg); everything else is `other`, per-leg too.
    let mut short_call = 0.0_f64; // contracts
    let mut long_put = 0.0_f64;
    let mut call_legs: Vec<(&OccOption, f64)> = Vec::new();
    let mut put_legs: Vec<(&OccOption, f64)> = Vec::new();
    let mut other_legs: Vec<(&OccOption, f64)> = Vec::new();
    for (o, qty) in &legs {
        match (o.is_call, *qty < 0.0) {
            (true, true) if shares > 0.0 => {
                short_call += -qty;
                call_legs.push((o, -qty));
            }
            (false, false) if shares > 0.0 => {
                long_put += *qty;
                put_legs.push((o, *qty));
            }
            _ => other_legs.push((o, *qty)),
        }
    }
    // Strikes render exactly (OCC encodes strikes × 1000 — thousandths of a
    // dollar) — never rounded to a fabricated value.
    let fmt_strike = |s: f64| -> String {
        let t = format!("{s:.3}");
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    let side_detail = |side: &[(&OccOption, f64)]| -> String {
        match side {
            [(o, _)] => format!(" @{} exp {}", fmt_strike(o.strike), o.expiry),
            _ => {
                let per: Vec<String> = side
                    .iter()
                    .map(|(o, q)| format!("{q:.0}×@{} exp {}", fmt_strike(o.strike), o.expiry))
                    .collect();
                format!(" [{}]", per.join(", "))
            }
        }
    };
    let mut notes: Vec<String> = Vec::new();
    if short_call > 0.0 {
        let call_shares = short_call * 100.0;
        if call_shares <= shares {
            notes.push(format!(
                "covered call ({:.0} contract{}{} over ~{:.0}% of shares)",
                short_call,
                if short_call == 1.0 { "" } else { "s" },
                side_detail(&call_legs),
                (call_shares / shares * 100.0).round(),
            ));
        } else {
            // A naked or partial short call must never read as covered.
            notes.push(format!(
                "short call only ~{:.0}% covered ({:.0} contract{}{} vs {:.0} shares — the \
                 remainder is naked)",
                (shares / call_shares * 100.0).round(),
                short_call,
                if short_call == 1.0 { "" } else { "s" },
                side_detail(&call_legs),
                shares,
            ));
        }
    }
    if long_put > 0.0 {
        let put_shares = long_put * 100.0;
        // Coverage is never capped: puts beyond the held count are net bearish
        // exposure, not hedge, and must read as such.
        let excess = if put_shares > shares {
            " — protection beyond the held count is net bearish exposure, not hedge"
        } else {
            ""
        };
        notes.push(format!(
            "protective put ({:.0} contract{}{} protecting ~{:.0}% of shares{excess})",
            long_put,
            if long_put == 1.0 { "" } else { "s" },
            side_detail(&put_legs),
            (put_shares / shares * 100.0).round(),
        ));
    }
    let mut rendered = if short_call > 0.0 && long_put > 0.0 {
        format!("collar: {}", notes.join(" + "))
    } else {
        notes.join("; ")
    };
    if !other_legs.is_empty() {
        let per: Vec<String> = other_legs
            .iter()
            .map(|(o, q)| {
                format!(
                    "{} {} ×{:.0} @{} exp {}",
                    if *q < 0.0 { "short" } else { "long" },
                    if o.is_call { "call" } else { "put" },
                    q.abs(),
                    fmt_strike(o.strike),
                    o.expiry
                )
            })
            .collect();
        let other_note = format!(
            "other same-underlying leg{}: {} (net delta unscored — no chain read at 7a)",
            if other_legs.len() == 1 { "" } else { "s" },
            per.join(", ")
        );
        if rendered.is_empty() {
            rendered = other_note;
        } else {
            rendered = format!("{rendered}; {other_note}");
        }
    }
    // Post-filter the sides can't all be empty, but never emit a blank overlay.
    if rendered.is_empty() {
        return None;
    }
    Some(rendered)
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
        // Disposition-gated, not class-gated: a not-rated *stock* (guard-terminal
        // listing outcomes) rides the NOT-RATED surface below — folding its
        // weight into the sector table too would present the same exposure
        // twice and overstate `unknown_sector_weight`'s gradeable-weight meaning.
        let not_rated_verdict = matches!(verdict.disposition, VerdictDisposition::NotRated { .. });
        if position.asset_class.is_gradeable() && weight != 0.0 && !not_rated_verdict {
            if is_fund {
                if let Some(weights) = inp.fund_sector_weights.get(&key) {
                    let mut covered = 0.0;
                    for (sector, w) in weights {
                        // A per-sector share can never exceed 1: the clamp bounds
                        // a percent-served weighting misread as fractions, which
                        // would otherwise inflate the fold ~100× and fabricate
                        // overlap clusters.
                        let w = w.min(1.0);
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
                    // sector only; the remainder is honestly unknown. Same ≤1
                    // clamp as the fresh-weights fold.
                    let w = w.min(1.0);
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
                        // Insertion order, like every other "latest episode"
                        // selection (`outcome::plan_episodes`): the loaded vec is
                        // `id`-ordered, and under a backwards clock step
                        // `max_by(anchor_at)` inherited a stale predecessor's sector.
                        inp.episodes
                            .iter()
                            .rfind(|e| e.symbol.eq_ignore_ascii_case(&position.symbol))
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
    /// final action departs a lean the engine set still contains.
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
/// (`docs/portfolio-workflow.md` §Step 7b Returns). The full call decodes this
/// **strictly** — a response missing the portfolio-level envelope fails the
/// decode rather than defaulting to blanks; the holdings-only repair response
/// decodes through [`RepairResponse`] instead, so the two contracts never share
/// leniency.
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

/// The repair re-run's decoded response: holdings only — corrected objects for
/// the violating names, nothing else (the first draft's envelope is reused, so
/// its schema never demands one). A dedicated type rather than leniency on
/// [`ConstructionDraft`], so the full call keeps its strict envelope decode.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RepairResponse {
    pub holdings: BTreeMap<String, HoldingProposalDraft>,
}

/// The named-violation repair context — `Some` only on the single re-run
/// (`docs/portfolio-analysis.md` §Portfolio roll-up and construction). The re-run
/// asks for corrected objects **only for the violating names**: the required
/// output shrinks exactly when the violation list is longest, instead of the
/// recovery path demanding a larger answer than the attempt it rescues
/// (`docs/verification/2026-08-10-big-run-attempt-1.md` §Fix candidates 2).
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionRepair {
    /// The violating symbols (spine casing) — the repair schema's required set.
    pub symbols: Vec<String>,
    /// The rendered `- symbol: …` violation lines the model must fix — scoped
    /// to the symbols above: a non-spine key's violation is repaired
    /// deterministically by the overlay, and naming it here would demand a fix
    /// the narrowed schema gives the model no slot to author.
    pub violations: String,
    /// A compact rendering of the plan the overlay keeps
    /// ([`render_prior_plan`]) — every kept holding retains its previous
    /// proposal, and the corrected weights must cohere with them
    /// simultaneously, so the model must see the true book it repairs into.
    pub prior_plan: String,
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
    /// The plan needed the single named-violation repair pass
    /// (`docs/portfolio-workflow.md` §Step 7b) — a model re-run scoped to the
    /// violating names, or, when every violation named a non-spine key, the
    /// deterministic drop that needs no model call.
    #[serde(default)]
    pub retried: bool,
    /// Engine-bound findings recorded against the model's plan — a rung outside
    /// the engine's offered set, a range outside its rung band, a concentration-cap
    /// breach, unfunded buys. Since `portfolio-v7` these **annotate, never
    /// enforce**: the plan persists as authored and the divergence renders beside
    /// it (`docs/portfolio-analysis.md` §Portfolio roll-up and construction).
    /// `#[serde(default)]` for pre-v7 runs.
    #[serde(default)]
    pub engine_bound_annotations: Vec<String>,
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
    DuplicateHolding { symbol: String },
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

impl Violation {
    /// The symbol the violation names — the repair re-run's scoping key. Every
    /// *enforced* variant carries one; `UnfundedBuys` is the lone symbol-less
    /// variant, and under `portfolio-v7` it is annotation-only, so it can never
    /// reach the repair path.
    pub fn symbol(&self) -> Option<&str> {
        match self {
            Violation::MissingHolding { symbol }
            | Violation::UnknownHolding { symbol }
            | Violation::DuplicateHolding { symbol }
            | Violation::UnparseableAction { symbol, .. }
            | Violation::ActionOutsideOffered { symbol, .. }
            | Violation::RangeInverted { symbol }
            | Violation::RangeOutsideRungBand { symbol, .. }
            | Violation::SellAllNonZeroRange { symbol }
            | Violation::ImpliedWeightOutsideRange { symbol, .. }
            | Violation::CapBreach { symbol, .. }
            | Violation::ContextTrimUnattributed { symbol }
            | Violation::DivergenceMissing { symbol, .. }
            | Violation::UnknownContextCause { symbol, .. }
            | Violation::ContextCauseUnsupported { symbol, .. }
            | Violation::WhatChangedMissing { symbol, .. }
            | Violation::UnknownAttribution { symbol, .. }
            | Violation::IntrinsicAttributionUnsupported { symbol }
            | Violation::ContextCauseRequired { symbol } => Some(symbol),
            Violation::UnfundedBuys { .. } => None,
        }
    }
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
            Violation::DuplicateHolding { symbol } => {
                write!(
                    f,
                    "{symbol}: proposed more than once (case-variant keys) — one proposal per holding"
                )
            }
            Violation::UnparseableAction { symbol, action } => {
                write!(f, "{symbol}: action '{action}' is not a ladder rung")
            }
            Violation::ActionOutsideOffered { symbol, action, offered } => {
                let offered: Vec<&str> = offered.iter().map(Action::as_kebab).collect();
                write!(
                    f,
                    "{symbol}: action '{}' departs the engine set [{}]",
                    action.as_kebab(),
                    offered.join(", ")
                )
            }
            Violation::RangeInverted { symbol } => {
                write!(
                    f,
                    "{symbol}: target-weight range inverted or negative (0 ≤ low ≤ high required)"
                )
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
/// stated range (the coherence rail); the concentration-cap read records as an
/// engine-bound annotation.
pub fn validate_construction(
    draft: &ConstructionDraft,
    agg: &BookAggregates,
    holdings: &Holdings,
    profile: &InvestorProfile,
) -> Result<ValidatedConstruction, Vec<Violation>> {
    let mut violations: Vec<Violation> = Vec::new();
    // Engine-bound findings — recorded on the view, never enforced (the v7
    // no-restrictions contract): the model's plan stands as authored and these
    // render beside it. Coherence and attribution checks stay in `violations`.
    let mut annotations: Vec<Violation> = Vec::new();
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
    // Case-folded dedup: the schema can't bar extra keys, and a case-variant
    // pair ("AAA" / "aaa") would resolve to one spine row twice — double-
    // counting the implied book and external funding while the map silently
    // keeps only one action.
    let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (symbol, proposal) in &draft.holdings {
        let Some(row) = spine_by_symbol.get(&symbol.to_ascii_uppercase()).copied() else {
            continue;
        };
        if !seen_keys.insert(symbol.to_ascii_uppercase()) {
            violations.push(Violation::DuplicateHolding {
                symbol: row.symbol.clone(),
            });
            continue;
        }
        let Some(action) = parse_action(&proposal.action) else {
            violations.push(Violation::UnparseableAction {
                symbol: row.symbol.clone(),
                action: proposal.action.clone(),
            });
            continue;
        };
        if !row.offered.contains(&action) {
            annotations.push(Violation::ActionOutsideOffered {
                symbol: row.symbol.clone(),
                action,
                offered: row.offered.clone(),
            });
        }
        let (low, high) = (proposal.target_weight_low, proposal.target_weight_high);
        if !(low.is_finite() && high.is_finite()) || low > high || low < 0.0 {
            violations.push(Violation::RangeInverted {
                symbol: row.symbol.clone(),
            });
            continue;
        }
        if action == Action::SellAll && high > 0.0 {
            violations.push(Violation::SellAllNonZeroRange {
                symbol: row.symbol.clone(),
            });
            continue;
        }
        let band = engine::rung_band(action, row.current_weight);
        if low < band.0 - STRUCT_EPS || high > band.1 + STRUCT_EPS {
            annotations.push(Violation::RangeOutsideRungBand {
                symbol: row.symbol.clone(),
                action,
                low,
                high,
                band,
            });
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
            annotations.push(Violation::UnfundedBuys {
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
            annotations.push(Violation::CapBreach {
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
                // plan actually raises proceeds. The move's baseline is the lean
                // where one exists (fresh priced rows), else the prior action —
                // role-risk and carried rows carry no lean, and an
                // `unwrap_or(false)` here made a truthful cash-freed attribution
                // structurally rejectable on exactly those rows — else hold (a
                // debut's neutral baseline).
                ContextCause::CashFreed => {
                    let baseline = x.row.lean.or(x.row.prior_action).unwrap_or(Action::Hold);
                    sells > DOLLAR_EPS && rung_index(x.action) > rung_index(baseline)
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
                // Construction **accepted** this run's standalone lean (the
                // unchanged-lean case is the reversion stamp above, so here the
                // lean itself moved): the change off the prior action is the
                // intrinsic read's own move, and the app stamps `moved-intrinsic`
                // deterministically, superseding any model claim — the same
                // precedent as the reversion stamp. Step 7b owes a model
                // attribution only where it **overruled** the lean, the only
                // thing that stage actually reconciled (ruled 2026-08-11,
                // `docs/verification/2026-08-10-big-run-attempt-1.md`
                // §Disposition).
                let accepted_lean = !x.row.carried && x.row.lean == Some(x.action);
                if accepted_lean {
                    what_changed = Some(ActionWhatChanged {
                        attribution: ActionAttribution::MovedIntrinsic,
                        cause: None,
                        note: "construction accepted this run's standalone lean — the \
                               intrinsic read itself moved off the prior action"
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
            engine_bound_annotations: annotations.iter().map(|v| v.to_string()).collect(),
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
                // Known miss, accepted (ruled 2026-08-05, piece-3 walk): a
                // pre-lean-era carried verdict has `lean: None` and skips the
                // stamp even when construction moves off its action — the
                // ambiguity ages out with the pre-construction blobs.
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

// ---- The named-violation repair ------------------------------------------------

/// The repair re-run's scope: the violating symbols that resolve to spine rows,
/// spine-cased, in spine order, deduped. A violation naming a non-spine key
/// (`UnknownHolding`) contributes nothing here — there is no valid object a model
/// could author for a non-holding, so those keys are dropped deterministically by
/// [`overlay_repair`] instead. An empty scope therefore means every violation is
/// droppable and the repair needs no model call.
pub fn repair_scope(violations: &[Violation], spine: &[SizingSpineRow]) -> Vec<String> {
    spine
        .iter()
        .filter(|row| {
            violations.iter().any(|v| {
                v.symbol()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&row.symbol))
            })
        })
        .map(|row| row.symbol.clone())
        .collect()
}

/// Whether a first-draft key survives the overlay: outside the repair scope
/// (its holding is not being corrected) and resolving to a spine row (a key
/// naming no holding is deterministically dropped). Shared by
/// [`overlay_repair`] and [`render_prior_plan`], so the plan the repair prompt
/// calls kept is exactly the plan the overlay keeps.
fn overlay_keeps(key: &str, spine: &[SizingSpineRow], repair_symbols: &[String]) -> bool {
    !repair_symbols.iter().any(|s| s.eq_ignore_ascii_case(key))
        && spine.iter().any(|r| r.symbol.eq_ignore_ascii_case(key))
}

/// Render the plan the overlay will actually keep — the first draft minus the
/// repair scope and minus non-spine keys — one line per holding (action +
/// stated range), so the model sees the true book its corrected objects must
/// cohere with: the implied post-action weights are book-coupled. Rendering a
/// key the overlay drops would tell the model a phantom position is kept, and
/// it would size its corrections to cohere with weight that will not exist
/// (attempt-1 review sweep).
pub fn render_prior_plan(
    draft: &ConstructionDraft,
    spine: &[SizingSpineRow],
    repair_symbols: &[String],
) -> String {
    let mut out = String::new();
    for (symbol, p) in &draft.holdings {
        if !overlay_keeps(symbol, spine, repair_symbols) {
            continue;
        }
        out.push_str(&format!(
            "- {}: {} [{:.4}\u{2013}{:.4}]\n",
            symbol, p.action, p.target_weight_low, p.target_weight_high
        ));
    }
    out
}

/// Overlay the repair response onto the first draft: a corrected object replaces
/// every case-variant of its symbol, a key resolving to no spine row is dropped
/// (the `UnknownHolding` / `DuplicateHolding` repairs — an overlay alone cannot
/// delete a bad key), and the first draft's envelope is kept (the repair response
/// is holdings-only). Scope is enforced both ways: a corrected object for a
/// holding *outside* the repair scope is ignored — the documented contract is
/// that every un-named holding keeps its first-draft proposal, and the grammar
/// cannot bar extra keys (that reachability is exactly why `UnknownHolding`
/// exists on the full call). Corrected keys insert under the scope's (spine)
/// casing: two in-scope case variants would otherwise land as distinct map keys
/// and re-fail whole-book validation as `DuplicateHolding`, burning the single
/// repair pass on a shape the app collapses deterministically — BTreeMap
/// iteration means the lexicographically greater variant's object wins;
/// response emission order is not preserved by decoding, and determinism, not
/// recency, is the contract. The merged draft is then re-validated **whole** — the
/// implied post-action weights are book-coupled through the implied total, so a
/// per-symbol re-check would miss a repair that breaks a previously-clean
/// holding's containment.
pub fn overlay_repair(
    first: &ConstructionDraft,
    corrected: BTreeMap<String, HoldingProposalDraft>,
    spine: &[SizingSpineRow],
    repair_symbols: &[String],
) -> ConstructionDraft {
    let mut holdings = first.holdings.clone();
    holdings.retain(|key, _| overlay_keeps(key, spine, repair_symbols));
    for (key, proposal) in corrected {
        if let Some(canonical) = repair_symbols.iter().find(|s| s.eq_ignore_ascii_case(&key)) {
            holdings.insert(canonical.clone(), proposal);
        }
    }
    ConstructionDraft {
        holdings,
        risk_posture: first.risk_posture.clone(),
        deployment_stance: first.deployment_stance.clone(),
        concentration_read: first.concentration_read.clone(),
        closed_positions_note: first.closed_positions_note.clone(),
    }
}

// ---- The construction schema ---------------------------------------------------

/// The JSON Schema handed to Ollama's `format` for the construction call — one
/// required property per actionable holding, each holding's `action` enum listing
/// the **full ladder** since `portfolio-v7` — the engine's allowed set (feasible /
/// transition / reduced spine) renders into the prompt as the engine arm's own
/// read, an outside-the-set rung persisting with an engine-bound annotation,
/// never a schema bar (`docs/portfolio-workflow.md` §Step 7b).
/// The fields the model must return for **every** holding in the plan. The schema's
/// per-holding `required` set is built from this list and the prompt-declaration test
/// reads it, so a field added here fails that test until the prompt declares it —
/// the drift Finding 2 describes, closed structurally rather than by discipline
/// (`docs/verification/2026-08-10-big-run-attempt-1.md`).
pub const PLAN_ENVELOPE_KEYS: [&str; 5] = [
    "holdings",
    "risk_posture",
    "deployment_stance",
    "concentration_read",
    "closed_positions_note",
];

pub const PER_HOLDING_PLAN_KEYS: [&str; 9] = [
    "action",
    "target_weight_low",
    "target_weight_high",
    "rationale",
    "divergence_cause",
    "divergence_note",
    "changed_attribution",
    "changed_cause",
    "changed_note",
];

/// The repair re-run's envelope: holdings only — the first draft's envelope is
/// reused, so re-demanding it would spend the exact output budget the repair
/// exists to save. The repair contract sentence and schema `required` are both
/// built from this, mirroring [`PLAN_ENVELOPE_KEYS`].
pub const REPAIR_ENVELOPE_KEYS: [&str; 1] = ["holdings"];

/// One holding's plan-object schema — identical for every row since
/// `portfolio-v7`: the action enum lists the full ladder, the engine's offered
/// set rendering into the prompt as its own arm's read, an outside-the-set rung
/// recorded as an engine-bound annotation, never a schema bar
/// (`docs/portfolio-analysis.md` §Portfolio roll-up and construction).
fn holding_plan_schema() -> Value {
    let causes = ["became-oversized", "overlap-emerged", "cash-freed"];
    let mut cause_or_null: Vec<Value> = causes.iter().map(|c| json!(c)).collect();
    cause_or_null.push(Value::Null);
    let attribution_or_null = vec![json!("moved-intrinsic"), json!("moved-context"), Value::Null];
    let offered: Vec<&str> = [
        Action::SellAll,
        Action::Trim,
        Action::Hold,
        Action::Add,
        Action::AddAggressively,
    ]
    .iter()
    .map(Action::as_kebab)
    .collect();
    json!({
        "type": "object",
        "properties": {
            "action": { "type": "string", "enum": offered },
            "target_weight_low": { "type": "number", "minimum": 0 },
            "target_weight_high": { "type": "number", "minimum": 0 },
            "rationale": { "type": "string" },
            "divergence_cause": { "type": ["string", "null"], "enum": cause_or_null },
            "divergence_note": { "type": ["string", "null"] },
            "changed_attribution": { "type": ["string", "null"], "enum": attribution_or_null },
            "changed_cause": { "type": ["string", "null"], "enum": cause_or_null },
            "changed_note": { "type": ["string", "null"] }
        },
        "required": PER_HOLDING_PLAN_KEYS
    })
}

/// The `holdings` object schema over the given rows — one required property per
/// row, shared by the full and repair schemas.
fn holdings_object_schema<'a>(rows: impl Iterator<Item = &'a SizingSpineRow>) -> Value {
    let mut holding_props = serde_json::Map::new();
    let mut required_symbols: Vec<Value> = Vec::new();
    for row in rows {
        holding_props.insert(row.symbol.clone(), holding_plan_schema());
        required_symbols.push(json!(row.symbol));
    }
    json!({
        "type": "object",
        "properties": Value::Object(holding_props),
        "required": required_symbols
    })
}

pub fn construction_schema(spine: &[SizingSpineRow]) -> Value {
    json!({
        "type": "object",
        "properties": {
            "holdings": holdings_object_schema(spine.iter()),
            "risk_posture": { "type": "string" },
            "deployment_stance": { "type": "string" },
            "concentration_read": { "type": "string" },
            "closed_positions_note": { "type": ["string", "null"] }
        },
        "required": PLAN_ENVELOPE_KEYS
    })
}

/// The repair re-run's schema: corrected objects for the violating names only —
/// the spine filtered to the repair scope, the envelope reduced to `holdings`.
/// The subset `required` list is what shrinks the demanded output exactly when
/// the violation list is longest.
pub fn construction_repair_schema(spine: &[SizingSpineRow], repair_symbols: &[String]) -> Value {
    let rows = spine.iter().filter(|row| {
        repair_symbols
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&row.symbol))
    });
    json!({
        "type": "object",
        "properties": { "holdings": holdings_object_schema(rows) },
        "required": REPAIR_ENVELOPE_KEYS
    })
}

// ---- Prompt construction (pure, testable) --------------------------------------

/// The construction call's system prompt. `repair` selects the closing response
/// contract: the full-plan declaration, or the repair re-run's
/// corrected-objects-only declaration ([`construction_repair_response_contract`]).
pub fn construction_system_prompt(repair: bool) -> String {
    let contract = if repair {
        construction_repair_response_contract()
    } else {
        construction_response_contract()
    };
    format!(
        "You are the portfolio-construction stage of a prescriptive portfolio review. \
     Every holding has already been analyzed in isolation — its grade, conviction, \
     scenario targets, and a STANDALONE ACTION LEAN (what the action would be if the \
     holding stood alone). Your job is the one judgment that needs the whole book: \
     reconcile each holding's lean against the whole-book aggregates — concentration, \
     sector exposure and overlap clusters, cash, the not-rated positions' exposure — \
     into its FINAL ACTION and a target portfolio-weight range, and write the \
     portfolio-level view. Express every target weight as a DECIMAL FRACTION of the \
     book (write 0.065 for 6.5%). Your action choice is UNRESTRICTED — the full \
     ladder is open on every holding. Each holding lists the ENGINE set: the rungs \
     and fraction bands the engine's own mechanical rules would offer (a carried \
     holding's engine set reflects the transition rule — toward hold only, plus a \
     context trim needing a real concentration or overlap reason). Treat the engine \
     set as evidence, not a bar: departing it is legitimate, and the app records the \
     departure beside your plan rather than rejecting it. Two things must still \
     COHERE: a sell-all range is 0–0, and your proposed weights must be arithmetically \
     consistent when they hold SIMULTANEOUSLY (the app solves the implied post-action \
     book; a stated range the implied weight falls outside is incoherent and comes \
     back once, named). Where your final action \
     departs a holding's lean, say why with a divergence_cause from the vocabulary; \
     where an action changed against its baseline, attribute it (moved-intrinsic or \
     moved-context with a cause) — every context claim is checked against the real \
     aggregates. A dead-money loser is a legitimate source of redeployable cash: \
     raising cash from one may cite the possible tax benefit of realizing the loss \
     and the redeployment optionality of the proceeds as supporting rationale, framed \
     high-level (the user acts on the specifics). Do NOT invent numbers: every figure \
     you cite must come from the aggregates given. {}",
        contract
    )
}

/// The response-contract sentence, generated from the same constants the schema's
/// `required` sets are built from, so the declaration cannot drift from what is
/// enforced. Generated rather than asserted: `action`, `rationale` and
/// `divergence_cause` all appear in the instructional prose above this clause, so a
/// containment test over the whole prompt cannot distinguish a real declaration from
/// an incidental mention (`docs/verification/2026-08-10-big-run-attempt-1.md`
/// §Finding 2).
pub fn construction_response_contract() -> String {
    format!(
        "Respond with a single JSON object carrying exactly these keys: {}. \
         `holdings` is keyed by ticker with one entry per holding listed above, each \
         carrying exactly: {} — the nullable ones present, holding null where they do \
         not apply. The response format is enforced by the decoder, so spend no \
         reasoning on shape — put it into the plan.",
        PLAN_ENVELOPE_KEYS.join(", "),
        PER_HOLDING_PLAN_KEYS.join(", "),
    )
}

/// The repair re-run's response contract — generated from [`REPAIR_ENVELOPE_KEYS`]
/// the same way the full contract is generated from [`PLAN_ENVELOPE_KEYS`], so the
/// declaration cannot drift from what the repair schema enforces.
pub fn construction_repair_response_contract() -> String {
    format!(
        "Respond with a single JSON object carrying exactly these keys: {}. \
         `holdings` is keyed by ticker with one corrected entry per holding named in \
         the VALIDATION FAILURE block — those holdings ONLY; every other holding \
         keeps its previous proposal exactly as already stated — each entry carrying \
         exactly: {} — the nullable ones present, holding null where they do not \
         apply. The response format is enforced by the decoder, so spend no \
         reasoning on shape — put it into the corrections.",
        REPAIR_ENVELOPE_KEYS.join(", "),
        PER_HOLDING_PLAN_KEYS.join(", "),
    )
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
    // Each engine-set action with its engine band at this row's current weight,
    // as decimal fractions of the book — the engine's numeric read, rendered so
    // the model never has to guess the bands its departures are annotated
    // against (`docs/portfolio-workflow.md` §Step 7b).
    let offered: Vec<String> = row
        .offered
        .iter()
        .map(|a| {
            let (lo, hi) = engine::rung_band(*a, row.current_weight);
            format!("{} {:.4}\u{2013}{:.4}", a.as_kebab(), lo, hi)
        })
        .collect();
    parts.push(format!("ENGINE SET [{}]", offered.join(", ")));
    if row.context_trim_carveout {
        parts.push(
            "the engine set admits trim only with a became-oversized / overlap-emerged \
             attribution"
                .into(),
        );
    }
    format!("- {}: {}\n", row.symbol, parts.join("; "))
}

/// The construction call's user prompt: the aggregates, the per-holding digests,
/// the exited names, the house view, and the investor profile — plus, on the
/// single re-run, the repair block: the named violations, the first draft's plan
/// (kept for every holding not named), and the corrected-objects-only scope.
pub fn construction_user_prompt(
    agg: &BookAggregates,
    exited: &[ExitedPosition],
    house_view_sections: Option<&str>,
    profile: &InvestorProfile,
    repair: Option<&ConstructionRepair>,
) -> String {
    let mut p = String::new();
    if let Some(r) = repair {
        p.push_str(&format!(
            "VALIDATION FAILURE — your previous proposal violated the construction \
             contract on the holdings named below. Return corrected objects for ONLY \
             these holdings: {}. Every holding in THE KEPT PLAN keeps its previous \
             proposal exactly as stated, so your corrected weights must cohere with \
             those kept proposals SIMULTANEOUSLY on the implied post-action book. Any \
             key from your previous response naming no actual holding has been \
             removed and is not part of the book — do not re-emit it.\nViolations to \
             fix:\n{}\n\nTHE KEPT PLAN (kept verbatim; your corrected objects replace \
             only the holdings named above):\n{}\n",
            r.symbols.join(", "),
            r.violations,
            r.prior_plan,
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

    p.push_str(
        "\nHOLDINGS (full ladder open on every holding; ENGINE SET = the engine's own \
         mechanical read, given as evidence):\n",
    );
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
        "\nINVESTOR PROFILE: objective {}, risk tolerance {}, horizon {}, taxable {}, cash {}\n",
        profile.objective.label(),
        profile.risk_tolerance.label(),
        profile.horizon.label(),
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
    fn a_range_outside_the_rung_band_is_annotated_never_enforced() {
        // The v7 contract: an engine-band departure records on the view and the
        // plan persists as authored — a coherent range outside the rung band is
        // the model's opinion, not an error.
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
                // Hold band at 10% weight is [9%, 11%] — the range sits well above
                // it, but stays coherent with the implied post-action book.
                target_weight_low: 0.16,
                target_weight_high: 0.20,
                rationale: "x".into(),
                divergence_cause: None,
                divergence_note: None,
                changed_attribution: None,
                changed_cause: None,
                changed_note: None,
            },
        )]);
        let out =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .expect("an engine-band departure is an annotation, not a violation");
        assert_eq!(out.actions["AAA"].target_weight_low, 0.16);
        assert!(
            out.view
                .engine_bound_annotations
                .iter()
                .any(|a| a.contains("engine band")),
            "{:?}",
            out.view.engine_bound_annotations
        );
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
                // Below even STRUCT_EPS: the sell-all zero range is exact.
                target_weight_high: 0.00005,
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
                // Inverted by less than STRUCT_EPS: ordering is exact.
                target_weight_low: 0.10005,
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
    fn an_action_outside_the_engine_set_is_annotated_never_enforced() {
        // A carried hold's engine (transition) set is {trim*, hold} — the model
        // picks 'add' anyway. The v7 contract: the pick stands, the departure
        // records on the view.
        let mut row = spine_row("AAA", 0.10, transition_actions(Action::Hold).0);
        row.carried = true;
        row.context_trim_carveout = true;
        row.lean = None;
        // No prior action: the what-changed attribution requirement (an honesty
        // check, still enforced) stays out of this test's way.
        row.prior_action = None;
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
        let out =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .expect("an engine-set departure is an annotation, not a violation");
        assert_eq!(out.actions["AAA"].action, Action::Add);
        assert!(
            out.view
                .engine_bound_annotations
                .iter()
                .any(|a| a.contains("engine set")),
            "{:?}",
            out.view.engine_bound_annotations
        );
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
    fn cash_freed_validates_on_a_lean_less_add_side_move() {
        // Role-risk and carried rows carry no lean; the pre-fix check
        // `unwrap_or(false)` made a truthful cash-freed attribution
        // structurally rejectable on exactly those rows (reachable since v7
        // opened role-risk adds, departures annotated). The baseline falls back
        // to the prior action.
        let mut add_row = spine_row("AAA", 0.05, vec![Action::Trim, Action::Hold, Action::Add]);
        add_row.lean = None; // role-risk / carried shape
        add_row.prior_action = Some(Action::Hold);
        let mut trim_row = spine_row("BBB", 0.10, vec![Action::SellAll, Action::Trim, Action::Hold]);
        trim_row.lean = Some(Action::Trim);
        trim_row.prior_action = Some(Action::Trim);
        let spine = vec![add_row, trim_row];
        let holdings = Holdings {
            cash: 85_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let (a_low, a_high) = engine::rung_band(Action::Add, 0.05);
        let add = HoldingProposalDraft {
            action: "add".into(),
            target_weight_low: a_low,
            target_weight_high: a_high,
            rationale: "redeploy the trim's proceeds".into(),
            divergence_cause: None,
            divergence_note: None,
            changed_attribution: Some("moved-context".into()),
            changed_cause: Some("cash-freed".into()),
            changed_note: Some("BBB's trim raises proceeds".into()),
        };
        let (t_low, t_high) = engine::rung_band(Action::Trim, 0.10);
        let mut trim = hold_proposal(0.10);
        trim.action = "trim".into();
        trim.target_weight_low = t_low;
        trim.target_weight_high = t_high;
        let draft = draft_for(vec![("AAA", add), ("BBB", trim)]);
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture());
        match out {
            Ok(v) => {
                let wc = v.actions["AAA"].what_changed.as_ref().unwrap();
                assert_eq!(wc.cause, Some(ContextCause::CashFreed));
            }
            Err(violations) => {
                assert!(
                    !violations.iter().any(|v| matches!(
                        v,
                        Violation::ContextCauseUnsupported { symbol, .. } if symbol == "AAA"
                    )),
                    "a truthful lean-less cash-freed attribution must validate: {violations:?}"
                );
                panic!("unexpected unrelated violations: {violations:?}");
            }
        }
    }

    #[test]
    fn case_variant_duplicate_proposals_are_a_typed_violation() {
        // The schema can't bar extra keys; "AAA" and "aaa" both resolve to one
        // spine row — un-deduped they double-count the implied book and
        // external funding while the map silently keeps one action.
        let spine = vec![spine_row("AAA", 0.05, vec![Action::Trim, Action::Hold, Action::Add])];
        let holdings = Holdings {
            cash: 85_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let draft = draft_for(vec![("AAA", hold_proposal(0.05)), ("aaa", hold_proposal(0.05))]);
        let violations =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, Violation::DuplicateHolding { .. })),
            "{violations:?}"
        );
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
    fn an_accepted_lean_move_is_app_stamped_moved_intrinsic() {
        // The class-A shape from the 2026-08-10 attempt: the action moved off the
        // prior run's action, construction accepted this run's lean, the model
        // omitted `changed_attribution`. Ruled 2026-08-11: the app stamps
        // `moved-intrinsic` deterministically — Step 7b owes a model attribution
        // only where it overruled the lean.
        let mut row = spine_row("AAA", 0.05, vec![Action::SellAll, Action::Trim, Action::Hold]);
        row.lean = Some(Action::Hold);
        row.prior_lean = Some(Action::SellAll);
        row.prior_action = Some(Action::SellAll);
        let spine = vec![row];
        let holdings = Holdings {
            cash: 95_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let draft = draft_for(vec![("AAA", hold_proposal(0.05))]);
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
            .expect("an accepted-lean move validates without a model attribution");
        let wc = out.actions["AAA"].what_changed.as_ref().unwrap();
        assert_eq!(wc.attribution, ActionAttribution::MovedIntrinsic);
        assert_eq!(wc.cause, None);
        assert!(wc.note.contains("accepted"), "{}", wc.note);
        assert!(out.actions["AAA"].lean_divergence.is_none());
    }

    #[test]
    fn a_bogus_claim_on_an_accepted_lean_move_is_superseded_not_a_violation() {
        // The class-B shape: same accepted-lean move, but the model asserts a
        // context cause that maps to no real aggregate (`cash-freed` with no
        // proceeds). The app stamp supersedes the claim — exactly as the
        // neighbouring reversion stamp supersedes one — instead of failing the
        // book over a field the stage never needed.
        let mut row = spine_row("AAA", 0.05, vec![Action::SellAll, Action::Trim, Action::Hold]);
        row.lean = Some(Action::Hold);
        row.prior_lean = Some(Action::SellAll);
        row.prior_action = Some(Action::SellAll);
        let spine = vec![row];
        let holdings = Holdings {
            cash: 95_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let mut proposal = hold_proposal(0.05);
        proposal.changed_attribution = Some("moved-context".into());
        proposal.changed_cause = Some("cash-freed".into());
        proposal.changed_note = Some("redeploying freed cash".into());
        let draft = draft_for(vec![("AAA", proposal)]);
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
            .expect("the app stamp supersedes the bogus claim");
        let wc = out.actions["AAA"].what_changed.as_ref().unwrap();
        assert_eq!(wc.attribution, ActionAttribution::MovedIntrinsic);
        assert_eq!(wc.cause, None);
    }

    #[test]
    fn an_overruled_lean_still_demands_a_model_attribution() {
        // The stamp's boundary: the final action departs BOTH the prior action
        // and this run's lean — the only case Step 7b actually reconciled, so the
        // attribution demand stands exactly as before the ruling.
        let mut row = spine_row("AAA", 0.05, vec![Action::SellAll, Action::Trim, Action::Hold]);
        row.lean = Some(Action::Hold);
        row.prior_lean = Some(Action::SellAll);
        row.prior_action = Some(Action::SellAll);
        let spine = vec![row];
        let holdings = Holdings {
            cash: 95_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let (low, high) = engine::rung_band(Action::Trim, 0.05);
        let draft = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "trim".into(),
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
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, Violation::WhatChangedMissing { symbol, .. } if symbol == "AAA")),
            "{violations:?}"
        );
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
    fn an_implied_cap_breach_is_annotated_never_enforced() {
        // A 24% position pushed past the concentration cap: the v7 contract records
        // both the band departure and the cap breach on the view; the plan stands.
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
                // Hold band at 24%: [21.6%, 25%] (clamped). Mid 27% implies ~26.2%
                // of the implied book — coherent with the stated range, above the cap.
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
        let out =
            validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
                .expect("band + cap departures annotate, never enforce");
        let notes = &out.view.engine_bound_annotations;
        assert!(notes.iter().any(|a| a.contains("engine band")), "{notes:?}");
        assert!(notes.iter().any(|a| a.contains("concentration cap")), "{notes:?}");
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
        // Unconstrained preset: the buy is external funding, no annotation.
        let out = validate_construction(&draft, &agg, &holdings, &InvestorProfile::default_fixture())
            .expect("unconstrained cash admits the buy");
        assert!(out.view.external_funding.unwrap() > 0.0);
        assert!(out.view.engine_bound_annotations.is_empty());
        // A constraining profile with no cash records the funding gap — an
        // annotation on the view since v7, never a rejection.
        let constrained = InvestorProfile {
            available_cash: Some(0.0),
            ..InvestorProfile::default_fixture()
        };
        let out = validate_construction(&draft, &agg, &holdings, &constrained)
            .expect("an unfunded buy annotates, never enforces");
        assert!(
            out.view
                .engine_bound_annotations
                .iter()
                .any(|a| a.contains("funding")),
            "{:?}",
            out.view.engine_bound_annotations
        );
    }

    // ---- the schema -------------------------------------------------------------

    #[test]
    fn construction_schema_offers_the_full_ladder_to_every_holding() {
        // The v7 unrestricted contract: no per-holding narrowing — the engine set
        // is prompt evidence, and a departure annotates.
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
        let full = vec!["sell-all", "trim", "hold", "add", "add-aggressively"];
        for symbol in ["AAA", "BBB"] {
            let actions: Vec<&str> = schema["properties"]["holdings"]["properties"][symbol]
                ["properties"]["action"]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(actions, full, "{symbol} must see the full ladder");
        }
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
                model_view: None,
                engine_view: None,
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

        // A guard-terminal not-rated STOCK (class-gradeable, verdict not-rated)
        // rides the NOT-RATED surface only — folding its weight into the sector
        // table too would present the same exposure twice and overstate the
        // unknown bucket's gradeable-weight meaning.
        let not_rated_stock = HoldingVerdict {
            symbol: "XX".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated {
                reason: "unsupported listing".into(),
            },
            thesis_ledger: None,
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
        };
        let verdicts2 = vec![graded("AAA", Action::Hold), not_rated_stock];
        let holdings2 = Holdings {
            positions: vec![
                position("AAA", AssetClass::Stock, 20_000.0),
                position("XX", AssetClass::Stock, 6_000.0),
            ],
            cash: 74_000.0,
            account_total: 100_000.0,
            source_rows: vec![],
        };
        let agg2 = build_aggregates(&AggregateInputs {
            holdings: &holdings2,
            verdicts: &verdicts2,
            audits: &[],
            prior_verdicts: None,
            carried: &carried,
            over_age: &over_age,
            stock_sectors: &stock_sectors,
            fund_sector_weights: &fund_weights,
            episodes: &[],
            profile: &InvestorProfile::default_fixture(),
        });
        assert!(
            (agg2.unknown_sector_weight - 0.0).abs() < 1e-9,
            "the not-rated stock's weight must not land in the unknown bucket: {}",
            agg2.unknown_sector_weight
        );
        assert_eq!(agg2.not_rated.len(), 1);
        assert!((agg2.not_rated[0].weight - 0.06).abs() < 1e-9);
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
                model_view: None,
                engine_view: None,
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
        // A long call over a long position is neither hedge pattern — `other`.
        let equity2 = position("NVDA", AssetClass::Stock, 40_000.0);
        let long_call = position("NVDA  260117C00900000", AssetClass::OptionContract, 800.0);
        let positions2 = vec![equity2.clone(), long_call];
        let overlay2 = same_underlying_overlay(&equity2, &positions2).unwrap();
        assert!(overlay2.contains("other same-underlying leg"), "{overlay2}");
        // `other` legs keep their per-leg detail too — a January and a June
        // call must not read identically.
        assert!(overlay2.contains("long call ×100 @900 exp 2026-01-17"), "{overlay2}");
    }

    #[test]
    fn zero_net_legs_never_render_and_edge_details_stay_exact() {
        // A zero-net option row (retained by holdings normalization) is no
        // exposure: alone it yields no overlay at all — never `×0` or a blank.
        let equity = position("AAPL", AssetClass::Stock, 10_000.0);
        let mut zero_put = position("AAPL  260117P00150000", AssetClass::OptionContract, 0.0);
        zero_put.quantity = 0.0;
        let mut zero_call = position("AAPL  260117C00250000", AssetClass::OptionContract, 0.0);
        zero_call.quantity = 0.0;
        let positions = vec![equity.clone(), zero_put, zero_call];
        assert!(same_underlying_overlay(&equity, &positions).is_none());

        // A thousandth-dollar OCC strike renders exactly (44.375, never 44.38).
        let mut equity = position("XYZ", AssetClass::Stock, 10_000.0);
        equity.quantity = 100.0;
        let mut call = position("XYZ   260117C00044375", AssetClass::OptionContract, 200.0);
        call.quantity = -1.0;
        let positions = vec![equity.clone(), call];
        let overlay = same_underlying_overlay(&equity, &positions).unwrap();
        assert!(overlay.contains("@44.375"), "{overlay}");

        // Puts beyond the held count read uncapped, flagged as net bearish
        // exposure rather than silently clamped to 100%.
        let mut equity = position("MSFT", AssetClass::Stock, 40_000.0);
        equity.quantity = 100.0;
        let mut puts = position("MSFT  260619P00350000", AssetClass::OptionContract, 900.0);
        puts.quantity = 3.0;
        let positions = vec![equity.clone(), puts];
        let overlay = same_underlying_overlay(&equity, &positions).unwrap();
        assert!(overlay.contains("~300% of shares"), "{overlay}");
        assert!(overlay.contains("net bearish exposure"), "{overlay}");
    }

    #[test]
    fn a_partial_short_call_never_reads_covered_and_a_collar_combines() {
        // 2 short contracts (200 shares) over 50 held shares: only ~25% covered
        // — the doc contract's "a naked short call must never read as covered".
        let mut equity = position("AAPL", AssetClass::Stock, 10_000.0);
        equity.quantity = 50.0;
        let mut short_calls = position("AAPL  260117C00200000", AssetClass::OptionContract, 400.0);
        short_calls.quantity = -2.0;
        let positions = vec![equity.clone(), short_calls];
        let overlay = same_underlying_overlay(&equity, &positions).unwrap();
        assert!(!overlay.contains("covered call"), "{overlay}");
        assert!(overlay.contains("short call only ~25% covered"), "{overlay}");
        assert!(overlay.contains("naked"), "{overlay}");
        assert!(overlay.contains("@200 exp 2026-01-17"), "{overlay}");

        // A fully covered short call plus a long put over the same shares reads
        // as one collar, both coverage legs shown.
        let equity = position("MSFT", AssetClass::Stock, 40_000.0);
        let mut short_call = position("MSFT  260619C00450000", AssetClass::OptionContract, 900.0);
        short_call.quantity = -1.0;
        let mut long_put = position("MSFT  260619P00350000", AssetClass::OptionContract, 700.0);
        long_put.quantity = 1.0;
        let positions = vec![equity.clone(), short_call, long_put];
        let overlay = same_underlying_overlay(&equity, &positions).unwrap();
        assert!(overlay.starts_with("collar:"), "{overlay}");
        assert!(overlay.contains("covered call"), "{overlay}");
        assert!(overlay.contains("protective put"), "{overlay}");
    }

    #[test]
    fn a_multi_leg_ladder_keeps_every_strike_and_fractional_strikes_render_exactly() {
        // Two short-call legs at different strikes / expiries over 200 shares:
        // the per-leg contract keeps both, and the 447.5 strike is never
        // rounded to a fabricated whole dollar.
        let mut equity = position("AAPL", AssetClass::Stock, 40_000.0);
        equity.quantity = 200.0;
        let mut near = position("AAPL  260117C00447500", AssetClass::OptionContract, 400.0);
        near.quantity = -1.0;
        let mut far = position("AAPL  260320C00500000", AssetClass::OptionContract, 300.0);
        far.quantity = -1.0;
        let positions = vec![equity.clone(), near, far];
        let overlay = same_underlying_overlay(&equity, &positions).unwrap();
        assert!(overlay.contains("covered call"), "{overlay}");
        assert!(overlay.contains("1×@447.5 exp 2026-01-17"), "{overlay}");
        assert!(overlay.contains("1×@500 exp 2026-03-20"), "{overlay}");
        assert!(overlay.contains("~100% of shares"), "{overlay}");
    }

    #[test]
    fn a_negative_sell_all_range_is_rejected() {
        // [-0.00005, -0.00005] passes ordering, the high > 0 sell-all check,
        // and the band tolerance — the exact non-negativity check must trip.
        let spine = vec![spine_row("AAA", 0.10, vec![Action::SellAll, Action::Trim, Action::Hold])];
        let holdings = Holdings {
            cash: 90_000.0,
            account_total: 100_000.0,
            ..holdings_for(&spine, 0.0)
        };
        let agg = agg_for(spine);
        let negative = draft_for(vec![(
            "AAA",
            HoldingProposalDraft {
                action: "sell-all".into(),
                target_weight_low: -0.00005,
                target_weight_high: -0.00005,
                rationale: "x".into(),
                divergence_cause: None,
                divergence_note: None,
                changed_attribution: None,
                changed_cause: None,
                changed_note: None,
            },
        )]);
        let violations =
            validate_construction(&negative, &agg, &holdings, &InvestorProfile::default_fixture())
                .unwrap_err();
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::RangeInverted { symbol } if symbol == "AAA")));
        // The schema also grammar-bars negatives at decode.
        let schema = construction_schema(&agg_for(vec![spine_row(
            "AAA",
            0.10,
            vec![Action::Hold],
        )]).spine);
        let props = &schema["properties"]["holdings"]["properties"]["AAA"]["properties"];
        assert_eq!(props["target_weight_low"]["minimum"], 0);
        assert_eq!(props["target_weight_high"]["minimum"], 0);
    }

    // ---- prompts ---------------------------------------------------------------

    #[test]
    fn construction_prompts_carry_the_load_bearing_content() {
        let sys = construction_system_prompt(false);
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
        // Each engine-set rung carries its numeric band at the row's current
        // weight (0.18) — rendered as the engine arm's own read, evidence the
        // model weighs rather than a bar it is validated against (v7).
        assert!(
            p.contains("ENGINE SET [sell-all 0.0000\u{2013}0.0000, trim 0.0720\u{2013}0.1260, hold 0.1620\u{2013}0.1980]"),
            "{p}"
        );
        assert!(p.contains("full ladder open on every holding"), "{p}");
        assert!(p.contains("single-position concentration cap 25% (0.25 as a fraction)"));
        assert!(p.contains("OVERLAP CLUSTERS"));
        assert!(p.contains("GONE"));
        assert!(p.contains("House view text"));
        assert!(p.contains("unconstrained"));
        // The B7-aligned profile line: the objective clause and the exact
        // medium-to-high risk framing the big run banks (the shared label()
        // source — `docs/configuration.md` §Investor Profile).
        assert!(p.contains(
            "INVESTOR PROFILE: objective maximize profit (total return; no income or \
             capital-preservation mandate), risk tolerance aggressive (medium-to-high), \
             horizon long-term"
        ), "{p}");
        assert!(!p.contains("VALIDATION FAILURE"));

        let repair = ConstructionRepair {
            symbols: vec!["AAA".into()],
            violations: "- AAA: sell-all must carry a 0-0 weight range".into(),
            prior_plan: "- AAA: hold [0.1620\u{2013}0.1980]\n".into(),
        };
        let retry =
            construction_user_prompt(&agg, &exited, Some("House view text"), &profile, Some(&repair));
        assert!(retry.starts_with("VALIDATION FAILURE"));
        assert!(retry.contains("sell-all must carry a 0-0 weight range"));
        // The repair block carries the corrected-objects-only scope and the kept
        // plan the corrections must cohere with — plus the removed-keys notice,
        // so a dropped phantom key is never re-emitted to "fix" its violation.
        assert!(retry.contains("ONLY these holdings: AAA"), "{retry}");
        assert!(retry.contains("THE KEPT PLAN"), "{retry}");
        assert!(retry.contains("do not re-emit it"), "{retry}");
        assert!(retry.contains("- AAA: hold [0.1620\u{2013}0.1980]"), "{retry}");

        // The repair system prompt swaps in the corrected-objects contract.
        let repair_sys = construction_system_prompt(true);
        assert!(
            repair_sys.contains(&construction_repair_response_contract()),
            "{repair_sys}"
        );
        assert!(!repair_sys.contains(&construction_response_contract()));
    }

    // ---- the named-violation repair ---------------------------------------------

    #[test]
    fn repair_scope_is_spine_cased_deduped_and_skips_non_spine_keys() {
        let spine = vec![
            spine_row("AAA", 0.10, vec![Action::Hold]),
            spine_row("BBB", 0.10, vec![Action::Hold]),
            spine_row("CCC", 0.10, vec![Action::Hold]),
        ];
        let violations = vec![
            Violation::RangeInverted { symbol: "bbb".into() },
            Violation::SellAllNonZeroRange { symbol: "BBB".into() },
            Violation::MissingHolding { symbol: "AAA".into() },
            Violation::UnknownHolding { symbol: "ZZZT".into() },
        ];
        assert_eq!(
            repair_scope(&violations, &spine),
            vec!["AAA".to_string(), "BBB".to_string()],
            "spine order, spine casing, deduped; the non-spine key contributes nothing"
        );
        // A pure-unknown violation list yields the empty scope — the deterministic
        // no-model-call repair.
        let unknown_only = vec![Violation::UnknownHolding { symbol: "ZZZT".into() }];
        assert!(repair_scope(&unknown_only, &spine).is_empty());
    }

    fn proposal(action: &str, low: f64, high: f64) -> HoldingProposalDraft {
        HoldingProposalDraft {
            action: action.into(),
            target_weight_low: low,
            target_weight_high: high,
            rationale: "r".into(),
            divergence_cause: None,
            divergence_note: None,
            changed_attribution: None,
            changed_cause: None,
            changed_note: None,
        }
    }

    #[test]
    fn overlay_repair_replaces_scoped_keys_drops_non_spine_keys_and_keeps_the_envelope() {
        let spine = vec![
            spine_row("AAA", 0.10, vec![Action::Hold]),
            spine_row("BBB", 0.10, vec![Action::Hold]),
        ];
        let mut first_holdings = BTreeMap::new();
        first_holdings.insert("aaa".to_string(), proposal("hold", 0.9, 0.95));
        first_holdings.insert("BBB".to_string(), proposal("hold", 0.08, 0.12));
        first_holdings.insert("ZZZT".to_string(), proposal("hold", 0.0, 0.01));
        let first = ConstructionDraft {
            holdings: first_holdings,
            risk_posture: "kept".into(),
            deployment_stance: "kept".into(),
            concentration_read: "kept".into(),
            closed_positions_note: Some("kept".into()),
        };
        let mut corrected = BTreeMap::new();
        corrected.insert("AAA".to_string(), proposal("hold", 0.08, 0.12));

        let merged = overlay_repair(&first, corrected, &spine, &["AAA".to_string()]);
        // The corrected object replaced the case-variant key rather than
        // coexisting with it.
        assert_eq!(
            merged.holdings.keys().collect::<Vec<_>>(),
            vec!["AAA", "BBB"],
            "case-variant replaced, non-spine key dropped"
        );
        assert_eq!(merged.holdings["AAA"].target_weight_low, 0.08);
        assert_eq!(
            merged.holdings["BBB"].target_weight_low, 0.08,
            "an un-named holding keeps its first-draft proposal"
        );
        // The repair response is holdings-only: the first draft's envelope rides.
        assert_eq!(merged.risk_posture, "kept");
        assert_eq!(merged.closed_positions_note.as_deref(), Some("kept"));

        // Scope is enforced both ways: a corrected object for an un-named spine
        // holding is ignored (the documented contract — every other holding
        // keeps its first-draft proposal), and a non-spine stray is dropped.
        let mut over_reach = BTreeMap::new();
        over_reach.insert("AAA".to_string(), proposal("hold", 0.08, 0.12));
        over_reach.insert("BBB".to_string(), proposal("sell-all", 0.0, 0.0));
        over_reach.insert("YYYT".to_string(), proposal("hold", 0.0, 0.01));
        let merged = overlay_repair(&first, over_reach, &spine, &["AAA".to_string()]);
        assert_eq!(merged.holdings.keys().collect::<Vec<_>>(), vec!["AAA", "BBB"]);
        assert_eq!(
            merged.holdings["BBB"].action, "hold",
            "an unsolicited correction outside the scope is ignored"
        );
    }

    #[test]
    fn overlay_repair_collapses_case_variant_corrected_keys() {
        // The narrowed schema cannot bar extra keys, so a repair response can
        // carry two case variants of one scoped name. Both must land on the
        // spine-cased key — as distinct map keys they would re-fail whole-book
        // validation as DuplicateHolding and burn the single repair pass.
        let spine = vec![spine_row("AAA", 0.10, vec![Action::Hold])];
        let mut first_holdings = BTreeMap::new();
        first_holdings.insert("AAA".to_string(), proposal("hold", 0.9, 0.95));
        let first = ConstructionDraft {
            holdings: first_holdings,
            risk_posture: "kept".into(),
            deployment_stance: "kept".into(),
            concentration_read: "kept".into(),
            closed_positions_note: None,
        };
        let mut corrected = BTreeMap::new();
        corrected.insert("AAA".to_string(), proposal("hold", 0.08, 0.12));
        corrected.insert("aaa".to_string(), proposal("trim", 0.04, 0.07));
        let merged = overlay_repair(&first, corrected, &spine, &["AAA".to_string()]);
        assert_eq!(
            merged.holdings.keys().collect::<Vec<_>>(),
            vec!["AAA"],
            "case variants collapse onto the spine casing"
        );
        // Deterministic winner: BTreeMap iteration visits "AAA" then "aaa", so
        // the lexicographically greater variant overwrites — decoding does not
        // preserve emission order, and determinism (not recency) is the
        // contract the merge makes.
        assert_eq!(merged.holdings["AAA"].action, "trim");
    }

    #[test]
    fn render_prior_plan_renders_exactly_the_kept_set() {
        // The kept-plan rendering must match the overlay's kept set: a scoped
        // (being-corrected) key and a non-spine (deterministically dropped) key
        // rendered as "kept" would make the model cohere its corrections with
        // weight that will not exist in the merged book.
        let spine = vec![
            spine_row("AAA", 0.10, vec![Action::Hold]),
            spine_row("BBB", 0.10, vec![Action::Hold]),
        ];
        let mut holdings = BTreeMap::new();
        holdings.insert("AAA".to_string(), proposal("hold", 0.09, 0.11));
        holdings.insert("bbb".to_string(), proposal("trim", 0.04, 0.07));
        holdings.insert("ZZZT".to_string(), proposal("hold", 0.0, 0.05));
        let draft = ConstructionDraft {
            holdings,
            risk_posture: String::new(),
            deployment_stance: String::new(),
            concentration_read: String::new(),
            closed_positions_note: None,
        };
        let rendered = render_prior_plan(&draft, &spine, &["AAA".to_string()]);
        assert!(!rendered.contains("AAA"), "scoped key is not kept: {rendered}");
        assert!(!rendered.contains("ZZZT"), "non-spine key is not kept: {rendered}");
        assert_eq!(rendered, "- bbb: trim [0.0400\u{2013}0.0700]\n");
    }

    #[test]
    fn a_repair_can_break_a_clean_holdings_containment_and_still_fails_whole() {
        // The book-coupling case the whole re-validation exists for: the
        // corrected object is coherent for its own name, but its implied-book
        // shift drags a previously-clean holding's implied weight below that
        // holding's stated floor — the fresh violation fails the merged plan
        // (and at the job seam the single-re-run rule then degraded-persists).
        let spine = vec![
            spine_row("AAA", 0.20, vec![Action::SellAll, Action::Trim, Action::Hold]),
            spine_row("BBB", 0.20, vec![Action::Hold]),
        ];
        let holdings = holdings_for(&spine, 60_000.0);
        let agg = agg_for(spine);
        let profile = InvestorProfile::default_fixture();
        // Draft 1: AAA trips the sell-all rail (book-neutral, one name); BBB is
        // a coherent hold at its band.
        let mut bad = hold_proposal(0.20);
        bad.action = "sell-all".into();
        bad.target_weight_low = 0.10;
        bad.target_weight_high = 0.12;
        let first = draft_for(vec![("AAA", bad), ("BBB", hold_proposal(0.20))]);
        let violations = validate_construction(&first, &agg, &holdings, &profile).unwrap_err();
        assert!(
            violations.iter().all(|v| v.symbol() == Some("AAA")),
            "{violations:?}"
        );
        let scope = repair_scope(&violations, &agg.spine);
        assert_eq!(scope, vec!["AAA".to_string()]);
        // The corrected AAA holds its own containment (implied ≈ 0.31 inside
        // [0.28, 0.44]) but funds a large add, inflating the implied total so
        // BBB's implied weight (≈ 0.172) falls below its stated 0.18 floor.
        let mut fix = hold_proposal(0.20);
        fix.target_weight_low = 0.28;
        fix.target_weight_high = 0.44;
        let mut corrected = BTreeMap::new();
        corrected.insert("AAA".to_string(), fix);
        let merged = overlay_repair(&first, corrected, &agg.spine, &scope);
        let fresh = validate_construction(&merged, &agg, &holdings, &profile).unwrap_err();
        assert!(
            fresh.iter().any(|v| matches!(
                v,
                Violation::ImpliedWeightOutsideRange { symbol, .. } if symbol == "BBB"
            )),
            "the fresh violation lands on the previously-clean name: {fresh:?}"
        );
        assert!(fresh.iter().all(|v| v.symbol() != Some("AAA")), "{fresh:?}");
    }

    #[test]
    fn the_full_call_decodes_the_envelope_strictly_and_the_repair_decodes_holdings_only() {
        // The strict boundary: a full response missing the portfolio-level
        // envelope must fail decode (schema drift or a daemon ignoring `format`
        // must not persist a blank construction view), while the repair
        // response is holdings-only by contract through its own type.
        let holdings_only = r#"{"holdings":{}}"#;
        assert!(
            serde_json::from_str::<ConstructionDraft>(holdings_only).is_err(),
            "a full-call decode must reject a missing envelope"
        );
        assert!(serde_json::from_str::<RepairResponse>(holdings_only).is_ok());
    }

    #[test]
    fn the_repair_schema_requires_only_the_violating_names() {
        let spine = vec![
            spine_row("AAA", 0.10, vec![Action::Hold]),
            spine_row("BBB", 0.10, vec![Action::Hold]),
            spine_row("CCC", 0.10, vec![Action::Hold]),
        ];
        let schema = construction_repair_schema(&spine, &["BBB".to_string()]);
        assert_eq!(
            schema["required"],
            serde_json::json!(REPAIR_ENVELOPE_KEYS),
            "the repair envelope is holdings-only"
        );
        let holdings = &schema["properties"]["holdings"];
        assert_eq!(holdings["required"], serde_json::json!(["BBB"]));
        assert!(holdings["properties"].get("AAA").is_none());
        // The per-holding object keeps the full-ladder v7 shape.
        assert_eq!(
            holdings["properties"]["BBB"]["required"],
            serde_json::json!(PER_HOLDING_PLAN_KEYS)
        );
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
                model_view: None,
                engine_view: None,
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
