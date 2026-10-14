//! Portfolio Analysis — the local-suite job that grades the user's holdings and
//! recommends an action for each (`docs/portfolio-analysis.md`). This module root
//! holds the domain types the stages exchange: the holding verdict and its parts,
//! the investor profile, and the durable plan-time parameters.

pub mod diff;
#[cfg(test)]
mod fixed_evidence;
/// Forwards the verdict-stage methods to the offline [`pipeline::StubAnalyst`] for
/// a test double that scripts research or consolidation alone; the double writes
/// `interpret` itself. One macro, so the seam's shape changes in one place.
#[cfg(test)]
macro_rules! forward_stub_verdict_calls {
    () => {
        fn interpret_role_risk(
            &self,
            input: &$crate::portfolio::pipeline::RoleRiskInput,
        ) -> anyhow::Result<String> {
            $crate::portfolio::pipeline::StubAnalyst.interpret_role_risk(input)
        }
        fn decide_action(
            &self,
            input: &$crate::portfolio::pipeline::ActionInput,
        ) -> anyhow::Result<$crate::portfolio::ActionDecision> {
            $crate::portfolio::pipeline::StubAnalyst.decide_action(input)
        }
        fn fast_id(&self) -> String {
            "fast".into()
        }
        fn reasoner_id(&self) -> String {
            "reasoner".into()
        }
    };
}

pub mod distill;
pub mod dossier;
pub mod engine;
pub mod evidence;
pub mod factor_store;
pub mod factors;
pub mod fund;
pub mod job;
pub mod listing;
pub mod outcome;
pub mod pipeline;
pub mod pre_profit;
pub mod price_action;
pub mod quick_check;
pub mod research;
pub mod review;
pub mod sector;
pub mod soft_forensic;
pub mod store;
pub mod tradability;
pub mod value_creation;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The tracker step key for one holding's pass — the single home for the
/// `holding-{SYMBOL}` format, shared by the job's step rows ([`job`]) and the
/// pipeline's step-scoped reasoning stream ([`pipeline`]) so streamed thinking
/// lands on the step the run tracker shows for that holding.
pub fn holding_step_key(symbol: &str) -> String {
    format!("holding-{symbol}")
}

// ---- Durable plan-time parameters --------------------------------------------
//
// Pinned because they shape retention and the house-view loader; the grade
// weights, risk-tier thresholds and options-signal parameters stay calibratable
// in `engine`.

/// How many Portfolio Analysis runs are retained (newest-N), pruned independently
/// of report retention and of Trade Opportunities (`docs/storage.md §Local Analysis
/// Suite Storage`). The number bounds the sidebar's `list_run_summaries` blob parse
/// as well as disk.
pub const PORTFOLIO_RUN_RETENTION: u32 = 30;

/// How many recent Market Signal reports load as the house-view context for a
/// holding's dossier (`docs/portfolio-analysis.md`), matching the research router's
/// window (`pipeline::ROUTER_RECENT_REPORTS`).
pub const HOUSE_VIEW_RECENT_REPORTS: u32 = 3;

// ---- Investor profile --------------------------------------------------------

/// The configured investor profile that personalizes the *action*, never the
/// intrinsic verdict (`docs/portfolio-analysis.md` §Intrinsic verdict,
/// `docs/configuration.md` §Investor Profile). It reaches the model at the
/// per-holding action call only ([`ActionDecision`]); Settings shows the fixed
/// preset ([`InvestorProfile::default_fixture`]) read-only via [`Self::display`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestorProfile {
    pub objective: ProfileObjective,
    pub risk_tolerance: RiskTolerance,
    pub horizon: ProfileHorizon,
    /// Whether the rationale may flag tax consequences as a caveat; never an
    /// action input, and no account type, tax lots or rates are modeled.
    pub tax_sensitive: bool,
    /// Cash available for new purchases, in account currency; `None` means cash
    /// is unconstrained. No stage reads it today — cash bounds are whole-book
    /// work (`docs/configuration.md` §Investor Profile).
    pub available_cash: Option<f64>,
}

impl InvestorProfile {
    /// The documented fixed preset (`docs/configuration.md` §Investor Profile).
    pub fn default_fixture() -> Self {
        Self {
            objective: ProfileObjective::MaximizeProfit,
            risk_tolerance: RiskTolerance::Aggressive,
            horizon: ProfileHorizon::LongTerm,
            tax_sensitive: true,
            available_cash: None,
        }
    }

    /// The read-only Settings rows, composed here so the Settings block and the
    /// action call's prompt share one label source.
    pub fn display(&self) -> InvestorProfileDisplay {
        InvestorProfileDisplay {
            objective: self.objective.label().to_string(),
            risk_tolerance: self.risk_tolerance.label().to_string(),
            horizon: self.horizon.label().to_string(),
            tax: if self.tax_sensitive {
                "tax-aware — tax consequences are an optional caveat, with no effect on \
                 the action; account type, tax lots, holding periods, and rates are unmodeled"
                    .to_string()
            } else {
                "tax-exempt — no tax consideration applied".to_string()
            },
            cash: match self.available_cash {
                Some(cap) => format!("capped at {cap:.0} (account currency)"),
                None => "unconstrained — adds are never gated on observed Schwab cash"
                    .to_string(),
            },
        }
    }
}

/// The ready-to-render read-only Settings rows for the investor profile
/// ([`InvestorProfile::display`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestorProfileDisplay {
    pub objective: String,
    pub risk_tolerance: String,
    pub horizon: String,
    pub tax: String,
    pub cash: String,
}

/// The investor's return objective (`docs/configuration.md` §Investor Profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileObjective {
    MaximizeProfit,
}

impl ProfileObjective {
    /// The shared prompt/Settings label ([`InvestorProfile::display`]).
    pub fn label(self) -> &'static str {
        match self {
            Self::MaximizeProfit => {
                "maximize profit (total return; no income or capital-preservation mandate)"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskTolerance {
    Conservative,
    Moderate,
    Aggressive,
}

impl RiskTolerance {
    /// The shared prompt/Settings label ([`InvestorProfile::display`]); the
    /// aggressive rung carries the preset's "medium-to-high" framing
    /// (`docs/configuration.md` §Investor Profile).
    pub fn label(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Moderate => "moderate",
            Self::Aggressive => "aggressive (medium-to-high)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileHorizon {
    ShortTerm,
    MediumTerm,
    LongTerm,
}

impl ProfileHorizon {
    /// The shared prompt/Settings label ([`InvestorProfile::display`]).
    pub fn label(self) -> &'static str {
        match self {
            Self::ShortTerm => "short-term",
            Self::MediumTerm => "medium-term",
            Self::LongTerm => "long-term (durable multi-quarter / multi-year theses)",
        }
    }
}

// ---- Asset eligibility -------------------------------------------------------

/// A position's asset class, decided before analysis (`docs/portfolio-analysis.md`
/// §Asset eligibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetClass {
    Stock,
    Etf,
    MutualFund,
    OptionContract,
    FixedIncome,
    Cash,
    Other,
}

impl AssetClass {
    /// Whether the equity pipeline can grade this class: stocks fully, funds in
    /// reduced form, the rest not rated.
    pub fn is_gradeable(&self) -> bool {
        matches!(self, AssetClass::Stock | AssetClass::Etf | AssetClass::MutualFund)
    }

    /// A short human label for the not-rated reason copy.
    pub fn label(&self) -> &'static str {
        match self {
            AssetClass::Stock => "a stock",
            AssetClass::Etf => "an ETF",
            AssetClass::MutualFund => "a mutual fund",
            AssetClass::OptionContract => "an option position",
            AssetClass::FixedIncome => "a fixed-income position",
            AssetClass::Cash => "cash",
            AssetClass::Other => "an unsupported position",
        }
    }
}

// ---- Holdings change tracking ------------------------------------------------

/// How a current position changed versus the prior run's snapshot
/// (`docs/portfolio-analysis.md` §Holdings change tracking), classified from
/// quantity before any model stage. `New` also covers every position on a first
/// run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionChange {
    New,
    Increased,
    Decreased,
    #[default]
    Unchanged,
}

/// The prior-run comparison for one current position, carried into its dossier.
/// The prior fields are `None` for a `New` position. Runtime-only (it rides in the
/// unserialized [`dossier::HoldingDossier`]); the persisted tag is [`PositionChange`].
#[derive(Debug, Clone, PartialEq)]
pub struct PositionDelta {
    pub change: PositionChange,
    pub prior_quantity: Option<f64>,
    pub prior_cost_basis: Option<f64>,
}

impl PositionDelta {
    pub fn new_position() -> Self {
        Self {
            change: PositionChange::New,
            prior_quantity: None,
            prior_cost_basis: None,
        }
    }
}

/// Which way an increase moved the position's average cost per share
/// (`docs/portfolio-analysis.md` §Holdings change tracking).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BasisDirection {
    /// The added shares cost more than the prior average — the average rose.
    PaidUp,
    /// The added shares cost less than the prior average — the average fell.
    AveragedDown,
}

/// The basis read beside an increase: its direction and the average cost per
/// share on the prior pull and on this one (cost basis ÷ quantity on each).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BasisMove {
    pub direction: BasisDirection,
    pub prior_average_cost: f64,
    pub average_cost: f64,
}

/// A position in the prior run's snapshot but absent now; it earns no verdict but
/// is surfaced in the roll-up (`docs/portfolio-analysis.md` §Holdings change
/// tracking).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitedPosition {
    pub symbol: String,
    pub description: String,
    pub prior_quantity: f64,
    pub prior_cost_basis: f64,
    pub prior_market_value: f64,
}

// ---- Verdict parts -----------------------------------------------------------

/// The composite letter grade, rolled up deterministically from the engine's
/// sub-scores (`docs/portfolio-analysis.md`); a fixed vocabulary so verdicts stay
/// comparable across runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    A,
    B,
    C,
    D,
    F,
}

impl Grade {
    pub fn as_str(&self) -> &'static str {
        match self {
            Grade::A => "A",
            Grade::B => "B",
            Grade::C => "C",
            Grade::D => "D",
            Grade::F => "F",
        }
    }
}

/// The sub-scores the composite grade rolls up from, each 0–100 with higher
/// better (risk is inverted at source, so a safer holding scores higher).
/// Computed by [`engine`], never authored by the model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SubScores {
    pub quality: f64,
    pub valuation: f64,
    pub momentum: f64,
    pub risk: f64,
}

/// The action ladder (`docs/portfolio-analysis.md` §The holding verdict). The
/// model selects the rung freely, the engine's set riding as evidence; the rung
/// is the whole decision — no weight range or share figure rides beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    SellAll,
    Trim,
    Hold,
    Add,
    AddAggressively,
}

impl Action {
    /// The kebab label serde uses — for building per-holding schema enums.
    pub fn as_kebab(&self) -> &'static str {
        match self {
            Action::SellAll => "sell-all",
            Action::Trim => "trim",
            Action::Hold => "hold",
            Action::Add => "add",
            Action::AddAggressively => "add-aggressively",
        }
    }

    /// The family the over-age rule demotes on a carried verdict
    /// (`docs/portfolio-analysis.md` §Triggering).
    pub fn is_add_family(&self) -> bool {
        matches!(self, Action::Add | Action::AddAggressively)
    }

    /// The family an over-age carry keeps as-is behind the stale badge
    /// (`docs/portfolio-analysis.md` §Triggering).
    pub fn is_exit_family(&self) -> bool {
        matches!(self, Action::SellAll | Action::Trim)
    }
}

/// How a verdict's action came to be: `model-chosen` by a model pass, or
/// `rule-demoted` — an over-age carried add-family action demoted to *hold* at
/// the roll-up (`docs/portfolio-analysis.md` §Triggering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionSource {
    #[default]
    ModelChosen,
    RuleDemoted,
}

/// The deterministic risk tier (`docs/portfolio-analysis.md` §Starting parameters);
/// a `role_risk_only` holding carries none. Scales the capital-efficiency hurdle
/// premium.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskTier {
    Low,
    Medium,
    High,
}

impl RiskTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskTier::Low => "low",
            RiskTier::Medium => "medium",
            RiskTier::High => "high",
        }
    }
}

/// The three-state capital-efficiency read (`docs/portfolio-analysis.md` §Starting
/// parameters): `Clears` when even the bear case clears the tier-scaled hurdle,
/// `Fails` when even the bull case misses it (the only dead-money state),
/// `Indeterminate` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HurdleState {
    Clears,
    Indeterminate,
    Fails,
    /// The read could not be computed (no scenario total returns) — the default so
    /// an empty [`engine::HurdleRead`] never fabricates a verdict.
    #[default]
    Unscorable,
}

/// The verdict's confidence, lowered when evidence is thin (below the evidence floor
/// the verdict abstains entirely instead — see [`VerdictDisposition`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Conviction {
    High,
    Medium,
    Low,
}

impl Conviction {
    /// The word the prompts print — the serde form ("low").
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// One scenario price target (account currency) with its methodology exposed; an
/// engine-arm number, never model-authored (the model arm states its own
/// expected prices on [`ThesisAppendix`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceTarget {
    pub base: f64,
    pub bear: f64,
    pub bull: f64,
    pub methodology: String,
}

/// The engine's bands at three months, twelve months and three years — rolling
/// windows from the run date, not calendar ends, since the accuracy pass scores
/// each band at its own horizon date (`docs/portfolio-analysis.md` §Starting
/// parameters). Each `None` when its inputs were missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceTargets {
    pub three_month: Option<PriceTarget>,
    pub twelve_month: Option<PriceTarget>,
    pub three_year: Option<PriceTarget>,
}

/// The thesis document's typed appendix — the model arm's only typed fields
/// (`docs/portfolio-analysis.md` §The holding verdict), persisted exactly as
/// authored. A `null` means the document states no value and is acted on by
/// presence alone. Only the declared domain is checked, at decode
/// ([`validate_appendix_domain`]); never the prices against the engine's bands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThesisAppendix {
    pub conviction: Option<Conviction>,
    pub expected_price_3m: Option<f64>,
    pub expected_price_12m: Option<f64>,
    pub expected_price_3y: Option<f64>,
}

impl ThesisAppendix {
    /// Every field null (a `role_risk_only` holding carries no appendix at all).
    pub const NONE: Self = Self {
        conviction: None,
        expected_price_3m: None,
        expected_price_12m: None,
        expected_price_3y: None,
    };

    pub fn is_empty(&self) -> bool {
        *self == Self::NONE
    }

    pub fn expected_prices(&self) -> [(&'static str, Option<f64>); 3] {
        [
            ("three-month", self.expected_price_3m),
            ("twelve-month", self.expected_price_12m),
            ("three-year", self.expected_price_3y),
        ]
    }
}

/// The model arm of a priced verdict as the thesis-document conversation returns
/// it (`docs/portfolio-workflow.md` §Step 6f); the engine arm is stamped beside it
/// at assembly ([`pipeline::graded_verdict_from_model_arm`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PricedModelArm {
    pub thesis_document: String,
    pub appendix: ThesisAppendix,
}

/// The per-stock options-activity signal from the Schwab option chain
/// (`docs/schwab-integration.md`) — an activity proxy, not positioning truth,
/// deliberately kept out of the grade sub-scores until calibration shows it adds
/// value. A field is `None` when the chain lacked the data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionsSignal {
    pub put_call_volume: Option<f64>,
    pub put_call_open_interest: Option<f64>,
    /// At-the-money implied volatility (a chain-wide proxy).
    pub implied_volatility: Option<f64>,
    /// Put-minus-call IV skew; positive means puts are richer.
    pub iv_skew: Option<f64>,
}

/// The priced body of a holding verdict — present only when the holding was
/// eligible, priceable and cleared the evidence floor. The engine arm is
/// app-stamped, never echoed through the model; the model arm persists exactly as
/// authored, type-checked only (`docs/portfolio-analysis.md` §The holding verdict).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradedVerdict {
    pub grade: Grade,
    pub sub_scores: SubScores,
    /// Authored by the action call from this holding's own evidence plus the
    /// investor profile; rung only (`docs/portfolio-analysis.md` §Portfolio action).
    pub action: Action,
    pub action_rationale: String,
    /// The model arm's prose, read as text and validated by nothing; each run's
    /// document supersedes the prior run's (`docs/portfolio-analysis.md` §The
    /// holding verdict).
    pub thesis_document: String,
    pub appendix: ThesisAppendix,
    pub price_targets: PriceTargets,
    pub options_signal: OptionsSignal,
    pub risk_tier: RiskTier,
    /// Only `Fails` is dead money.
    pub dead_money: HurdleState,
    /// True when the letter rests on an imputed (neutral-50) sub-score
    /// (`docs/portfolio-analysis.md` §Asset eligibility).
    pub low_confidence_grade: bool,
    /// The fund path's deterministic strategy label; `None` for a stock.
    pub fund_class_label: Option<String>,
    /// What a stock's letter was scored against — the card's basis line
    /// (`docs/portfolio-analysis.md` §Storage and display); `None` on a fund.
    #[serde(deserialize_with = "required_option")]
    pub grade_basis: Option<GradeBasis>,
    /// The engine's own rung ([`engine::engine_action`]), shown to the action call
    /// as a computed read, never a recommendation (`docs/portfolio-analysis.md`
    /// §Starting parameters).
    pub engine_rung: Action,
    /// Spot's relationship to the engine's twelve-month band at authoring
    /// ([`BandRelation`]); `None` where no band or spot existed, or where an
    /// unresolvable split bridge withheld the stamp (`docs/portfolio-analysis.md`
    /// §The quick check).
    pub authored_band_relation: Option<BandRelation>,
}

/// Decode an `Option` field whose key must be present: `null` reads as none and
/// an absent key fails, where a plain `Option` would read it as `None` (the
/// pre-release no-compat rule: a new persisted field is required).
pub(crate) fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// What a stock's letter was scored against, read off its factor composite
/// ([`factors::FactorComposite`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradeBasis {
    pub class: factors::MetricClass,
    /// The anchor row's label: the sector, `"market-wide"` where the sector was
    /// absent or unmapped, or the class's own row (`"Banks"`, `"REITs"`).
    pub anchor_row: String,
    /// More than half the resolved factors scored on their band alone — a
    /// data-health note, never a gate.
    pub thin_own_history: bool,
}

impl GradeBasis {
    pub fn of(composite: &factors::FactorComposite) -> GradeBasis {
        GradeBasis {
            class: composite.class,
            anchor_row: composite.sector_anchor.clone(),
            thin_own_history: composite.thin_own_history,
        }
    }
}

/// One exposure weight (a sector or country label and its fraction of the fund).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExposureWeight {
    pub label: String,
    pub weight: f64,
}

/// The `role_risk_only` branch of an analyzed verdict (`docs/portfolio-analysis.md`
/// §Intrinsic verdict): a structurally unpriceable vehicle class gets a role / risk
/// read with no letter, no price targets, no conviction and no tier; the engine
/// arm's action set is the reduced [`ROLE_RISK_ACTIONS`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleRiskVerdict {
    /// The deterministic classification label, e.g. "bond fund".
    pub class_label: String,
    /// The role read as prose; no prices, no conviction and no appendix follow
    /// it (`docs/portfolio-analysis.md` §Intrinsic verdict).
    pub thesis_document: String,
    /// Top sector or country exposure weights.
    pub exposure_tilt: Vec<ExposureWeight>,
    /// The expense ratio as an annual return headwind, where reported.
    pub expense_drag: Option<f64>,
    /// Annualized realized volatility, where computable.
    pub observable_risk: Option<f64>,
    /// Structurally path-dependent (leveraged / inverse and option-overlay
    /// vehicles).
    pub structural_flag: bool,
    /// Closed-end structure, detected from the profile's `isFund` flag plus the
    /// closed-end description fragment.
    pub is_cef: bool,
    /// Market price ÷ NAV − 1 (positive = premium), on the closed-end form only;
    /// `None` is the named gap, carried in `evidence_gaps` rather than fabricated.
    pub nav_premium: Option<f64>,
    /// This branch's confidence surface; never a fabricated conviction.
    pub evidence_gaps: Vec<String>,
    /// Authored by the action call from the branch's own attributes plus the
    /// investor profile; rung only, the full ladder open
    /// (`docs/portfolio-analysis.md` §Portfolio action).
    pub action: Action,
    pub action_rationale: String,
}

/// What a holding's analysis resolved to (`docs/portfolio-analysis.md` §Intrinsic
/// verdict): analyzed (`priced` or `role_risk_only`), can't-grade, or
/// shouldn't-grade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "status")]
pub enum VerdictDisposition {
    // Boxed: the priced body dwarfs the string variants, so without indirection
    // every disposition would be sized to it.
    Priced(Box<GradedVerdict>),
    /// A structurally unpriceable vehicle class — not an evidence deficiency, so
    /// never `insufficient-evidence` (`docs/portfolio-analysis.md` §Asset
    /// eligibility).
    RoleRiskOnly(Box<RoleRiskVerdict>),
    /// Ineligible asset class (option, bond, cash, …).
    NotRated { reason: String },
    /// Eligible but below the evidence floor — an explicit abstention. The prior
    /// thesis document is retained unrewritten so the next continuity run still
    /// reads it (`docs/portfolio-analysis.md` §Evidence floor); `None` on a debut
    /// abstention or after a prior with no document.
    InsufficientEvidence {
        reason: String,
        prior_thesis_document: Option<String>,
    },
}

/// Which statement window a holding's fundamentals were computed on
/// (`docs/portfolio-analysis.md` §Starting parameters). The prompt names it beside
/// the levels because a change of basis moves every statement-derived level
/// without the business changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StatementBasis {
    /// Four contiguous trailing quarters.
    Ttm,
    /// The SEC annual fallback, adopted when the quarterly window is gapped or
    /// short and stamped only where SEC filled a flow line
    /// (`dossier::merge_financials`); an equity-only fill stamps nothing.
    Annual,
}

impl StatementBasis {
    pub fn label(&self) -> &'static str {
        match self {
            StatementBasis::Ttm => "TTM (four trailing quarters)",
            StatementBasis::Annual => {
                "SEC annual (latest full year — the quarterly window fell back)"
            }
        }
    }
}

/// Which balance sheet supplied a holding's stockholders' equity — the
/// denominator of debt/equity and price/book, outside the flow-basis rule
/// (`docs/portfolio-analysis.md` §Starting parameters); stamped at
/// `dossier::merge_financials`. The prompt names it beside the statement basis
/// because the FMP leg is fail-soft: a gap one run and a return the next flips
/// the equity leg between a quarter-end and a year-end instant, stepping both
/// series with nothing having happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EquitySource {
    /// The preferred leg.
    FmpQuarterly,
    /// Filled where the FMP leg returned nothing.
    SecAnnual,
}

impl EquitySource {
    pub fn label(&self) -> &'static str {
        match self {
            EquitySource::FmpQuarterly => "FMP's latest quarterly balance sheet",
            EquitySource::SecAnnual => {
                "SEC's latest annual stockholders' equity (the quarterly balance-sheet leg \
                 fell back)"
            }
        }
    }
}

/// Spot's relationship to the engine's twelve-month bear–bull band, stamped on
/// the engine arm at the checkpoint so the quick check's `PriceOutsideBand` flag
/// fires on a change in the relationship, never on the standing state — a band
/// authored with spot already outside was an examined observation, not news.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BandRelation {
    Inside,
    BelowBand,
    AboveBand,
}

impl BandRelation {
    /// Classify spot against the band, order-insensitive to which target sits
    /// higher (the inverse spread mapping can put bear above bull).
    pub fn of(spot: f64, bear: f64, bull: f64) -> Self {
        let (lo, hi) = (bear.min(bull), bear.max(bull));
        if spot < lo {
            BandRelation::BelowBand
        } else if spot > hi {
            BandRelation::AboveBand
        } else {
            BandRelation::Inside
        }
    }
}

/// One holding's complete verdict record, persisted per run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingVerdict {
    pub symbol: String,
    pub asset_class: AssetClass,
    /// Set by the app from the holdings diff ([`diff`]), never by the model.
    pub position_change: PositionChange,
    /// The basis read beside an increase ([`diff::basis_move`]); `None` on every
    /// other change and where an average cost is undefined on either pull.
    pub basis_move: Option<BasisMove>,
    pub disposition: VerdictDisposition,
    /// The analysis vintage — the UTC RFC3339 timestamp of the full pass that
    /// produced this verdict, carried with a carried verdict
    /// (`docs/portfolio-analysis.md` §Triggering). `None` only on a debut
    /// abstention, where [`effective_vintage`] reads the run's `created_at`.
    pub analyzed_at: Option<String>,
    pub action_source: ActionSource,
    /// Set on a carried verdict whose position's net side reversed since it was
    /// written, so the carried thesis describes the opposite position
    /// (`docs/portfolio-analysis.md` §Triggering); a fresh pass leaves it `false`.
    pub side_reversed: bool,
}

impl HoldingVerdict {
    /// The analyzed branches' own document, an abstention's retained prior, none
    /// on a not-rated position (`docs/portfolio-analysis.md` §Continuity and
    /// isolation).
    pub fn thesis_document(&self) -> Option<&str> {
        match &self.disposition {
            VerdictDisposition::Priced(g) => Some(g.thesis_document.as_str()),
            VerdictDisposition::RoleRiskOnly(r) => Some(r.thesis_document.as_str()),
            VerdictDisposition::InsufficientEvidence {
                prior_thesis_document,
                ..
            } => prior_thesis_document.as_deref(),
            VerdictDisposition::NotRated { .. } => None,
        }
    }

    pub fn appendix(&self) -> Option<&ThesisAppendix> {
        match &self.disposition {
            VerdictDisposition::Priced(g) => Some(&g.appendix),
            _ => None,
        }
    }
}

/// A verdict's `analyzed_at`, else the `created_at` of the run it rides in — the
/// fallback for a debut abstention and for a verdict the job has not stamped yet,
/// both by construction their own run's (a carried verdict is stamped at carry).
pub fn effective_vintage<'a>(verdict: &'a HoldingVerdict, run_created_at: &'a str) -> &'a str {
    verdict.analyzed_at.as_deref().unwrap_or(run_created_at)
}

// ---- Run-level aggregate (persisted per run) ---------------------------------

/// The run-level data-health aggregate (`docs/portfolio-analysis.md` §Portfolio
/// roll-up): the per-holding fail-soft posture is silent at run level, so a
/// degraded run that looks clean would produce confidently wrong prescriptions.
/// Computed from the audits' `target_meta` plus the run-scoped counters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataHealth {
    /// Every resolved physical attempt this run's checkpoint owns, adapter
    /// failures included; summary selections never discard these rows.
    pub prompt_usage: Vec<crate::local_model::PromptUsage>,
    /// Priced holdings carrying a `target_meta` (the denominator).
    pub targets_total: usize,
    /// Targets whose multiples were rate-anchored on the DGS10 spread history.
    pub rate_anchored_count: usize,
    /// Targets on the raw-percentile fallback (thin dated-rate window).
    pub raw_percentile_count: usize,
    /// Targets on the current-multiple carry (no anchor history at all).
    pub current_multiple_carry_count: usize,
    /// Targets whose scenario band was widened to the dispersion floor.
    pub dispersion_floor_count: usize,
    /// Holdings whose deep-history (FMP dated-EOD) fetch degraded.
    pub deep_history_failures: usize,
    /// The run-level DGS10 anchor-history request failed, so every spread
    /// observation is inadmissible run-wide.
    pub dgs10_history_gap: bool,
    /// The house view was omitted for staleness (`docs/portfolio-workflow.md`
    /// §Step 5).
    pub house_view_omitted: bool,
    /// Local calls whose prompt filled at least [`CONTEXT_PRESSURE_FRACTION`] of
    /// their `num_ctx` (`docs/portfolio-analysis.md` §Portfolio roll-up): overflow
    /// silently front-truncates, so a near-full prompt is surfaced here.
    pub context_pressure: Vec<crate::local_model::PromptUsage>,
    /// The run's fullest local prompt by fraction of its `num_ctx`, recorded
    /// regardless of pressure; `None` when no call reported a count.
    pub peak_prompt: Option<crate::local_model::PromptUsage>,
    /// Commodity-context series gaps (`docs/portfolio-workflow.md` §Step 5);
    /// counted, never attention — the feed is enriching and fail-soft.
    pub commodity_gaps: usize,
    /// CFTC positioning contract gaps — the same enriching-feed posture.
    pub positioning_gaps: usize,
    /// The CBOE put/call backdrop was unavailable — the same posture.
    pub cboe_gap: bool,
    /// The FINRA short-interest file was unavailable — the same posture.
    pub finra_gap: bool,
    /// The M&A feed walk failed or truncated — the same posture (deals gathered
    /// before the failure still matched).
    pub ma_gap: bool,
    /// The accuracy pass hit an episode-store read or write error
    /// (`docs/portfolio-analysis.md` §Failure posture).
    pub accuracy_gap: bool,
    /// Distinct sector-benchmark series a completed holding read as unavailable;
    /// rebuilt from the holdings' rows, so a resumed run counts a benchmark once.
    pub benchmark_gaps: usize,
    /// Completed holdings whose research audit carries at least one gap; counted,
    /// never an attention trigger — research is additive and fail-soft.
    pub research_degraded_holdings: usize,
    /// Total research-gap entries across those holdings, folded from the typed
    /// `ResearchAuditRecord::gaps`.
    pub research_gap_count: usize,
    /// Model calls the retry-once recovered (`docs/local-models.md §The
    /// local-model adapter seam`). Every listed re-attempt succeeded — a failed
    /// holding's retry events are dropped as it is isolated — so the entries
    /// measure the absorbed transient rate.
    pub model_retries: Vec<crate::local_model::RetryEvent>,
    /// Degradation worth surfacing prominently: deep-history failures, a
    /// current-multiple carry, a run-wide DGS10 gap, context pressure, a
    /// length-stopped generation or a fired retry. Raw-percentile fallbacks and
    /// the enriching-feed gaps are counted but never flagged.
    pub attention: bool,
    /// The one line the roll-up card renders.
    pub summary: String,
}

/// The prompt-fill fraction at which a local call's context counts as under
/// pressure; the sanctioned response is compressing the prompt digests, never a
/// `num_ctx` change (`docs/portfolio-analysis.md` §Portfolio roll-up).
pub const CONTEXT_PRESSURE_FRACTION: f64 = 0.9;

/// The truncation-implausibility bound: a reported `prompt_eval_count` whose
/// `× TRUNCATION_CHARS_PER_TOKEN` cannot cover the chars the app sent reads as
/// likely front-truncation. The fill fraction alone cannot see one — Ollama's
/// count is post-truncation and lands far below `num_ctx`. Real tokenization runs
/// ~3–5 chars per token; 8 is deliberately generous so a trip is near-certain
/// truncation, not estimate noise.
pub const TRUNCATION_CHARS_PER_TOKEN: u64 = 8;

/// The deterministic run-level roll-up built after the per-holding pass
/// (`docs/portfolio-analysis.md` §Portfolio roll-up): descriptive, consumed by no
/// action logic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioRollUp {
    pub graded_count: usize,
    pub not_rated_count: usize,
    pub insufficient_evidence_count: usize,
    /// Counted beside the priced holdings, never pooled with them.
    pub role_risk_only_count: usize,
    /// The length of [`PortfolioRun::failed_holdings`]; a failed holding produced
    /// no fresh verdict, so it is never among the analyzed counts
    /// (`docs/portfolio-analysis.md` §Failure posture).
    pub failed_count: usize,
    /// The largest single-position weight, 0.0–1.0.
    pub top_position_weight: f64,
    /// Cash as a fraction of the account total.
    pub cash_weight: f64,
    /// Positions closed since the last run (`docs/portfolio-analysis.md` §Holdings
    /// change tracking); empty on a first run.
    pub exited: Vec<ExitedPosition>,
    pub data_health: DataHealth,
    /// A short deterministic synthesis line.
    pub overview: String,
}

/// The full-run pass's read of a stock's submissions feed: the item-classified
/// sweep's state and the 8-K and 8-K/A rows inside the same lookback that
/// FETCHED VALUES renders off the one read (`docs/data-sources.md` §Portfolio
/// Analysis — endpoint surface). An `Unknown` sweep carries no rows.
#[derive(Debug, Clone, PartialEq)]
pub struct FilingSweepRead {
    pub state: ForensicFilingState,
    pub filings_8k: Vec<crate::sec::RecentFiling>,
}

impl FilingSweepRead {
    /// Classify an already-fetched sweep and keep its 8-K and 8-K/A rows filed on
    /// or after `since` (ISO, inclusive), newest first; an unclassifiable sweep
    /// is `Unknown` with no rows.
    pub fn from_filings(symbol: &str, filings: &[crate::sec::RecentFiling], since: &str) -> Self {
        match crate::sec::forensic_events_from_filings(symbol, filings, since) {
            Ok(events) => {
                let mut filings_8k: Vec<crate::sec::RecentFiling> = filings
                    .iter()
                    .filter(|f| {
                        (f.form == "8-K" || f.form == "8-K/A") && f.filing_date.as_str() >= since
                    })
                    .cloned()
                    .collect();
                filings_8k.sort_by(|a, b| b.filing_date.cmp(&a.filing_date));
                let state = if events.is_empty() {
                    ForensicFilingState::Clear
                } else {
                    ForensicFilingState::Events { events }
                };
                Self { state, filings_8k }
            }
            // The sweep ran but could not classify: unknown, never a fabricated
            // clear, and no row list off a read that could not be classified.
            Err(reason) => Self {
                state: ForensicFilingState::Unknown {
                    reason,
                    queried: true,
                },
                filings_8k: Vec::new(),
            },
        }
    }
}

/// How far back a filing-classified hard-forensic event binds the hard rule, in
/// days before the run's session date (calibratable; the submissions feed's
/// window covers at least a year, so the sweep fully serves this bound). An
/// older event stays visible in the sweep but no longer trips the hard rule.
pub const FORENSIC_EVENT_LOOKBACK_DAYS: i64 = 365;

/// The per-holding outcome of the item-classified 8-K sweep
/// (`docs/portfolio-analysis.md` §Starting parameters; the shared producer
/// contract is `docs/trade-opportunities-workflow.md §Step 5c`). `Unknown` is a
/// logged degraded input, never a fabricated clear, and never trips the hard rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum ForensicFilingState {
    Events { events: Vec<crate::sec::ForensicEvent> },
    Clear,
    /// No CIK mapping (`queried: false`) or a failed / malformed fetch
    /// (`queried: true`).
    Unknown { reason: String, queried: bool },
}

impl ForensicFilingState {
    pub fn hard_tripped(&self) -> bool {
        matches!(self, ForensicFilingState::Events { events } if !events.is_empty())
    }
}

/// The audit's hard-forensic record; when tripped, the matched rule binds the
/// engine arm as a pre-profit ceiling does (`docs/portfolio-workflow.md §Step 6g`)
/// while the model's conviction and action persist as authored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForensicRead {
    pub state: ForensicFilingState,
    /// Recorded when tripped: engine conviction capped Low and the add family
    /// barred from the engine action set.
    pub matched_rule: Option<String>,
}

/// The holding's analysis as the audit carries it (`docs/portfolio-workflow.md`
/// §Step 6d), with its own authoring date and split-bridge anchor so a carried
/// analysis keeps the date and price basis it was written on, independently of
/// the vintage and anchor a successful pass stamps afresh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisRecord {
    pub text: String,
    /// The ET session date of the run that wrote the text.
    pub written: String,
    /// That run's `authoring_close`: the newest settled close before its session;
    /// `None` where its window held no such bar.
    pub anchor: Option<engine::DatedValue>,
}

/// One holding's audit record — what the verdict was based on
/// (`docs/storage.md §Local Analysis Suite Storage`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingAudit {
    pub symbol: String,
    pub metrics: engine::ComputedMetrics,
    /// The data sources the verdict actually consulted, with a note each;
    /// "(empty)" marks a leg that was queried and returned nothing, no label a
    /// leg that never ran.
    pub sources: Vec<String>,
    /// The model ids the verdict was actually authored with, in first-call order
    /// and deduplicated — drained from each outbound request, never inferred from
    /// the configured roster; empty on every no-model exit.
    pub model_ids: Vec<String>,
    pub prompt_version: String,
    /// The evidence-floor rule version (`engine::EVIDENCE_FLOOR_VERSION`), so a
    /// floor correction never silently re-reads a prior abstention or verdict.
    pub evidence_floor_version: String,
    /// Inputs a source could not resolve, carried from the financials' gap manifest.
    pub degraded_inputs: Vec<String>,
    /// App-stamped annotations from the action call: a chosen rung outside the
    /// engine's action set persists as authored with the departure recorded here
    /// (engine evidence annotates, never bars).
    pub action_annotations: Vec<String>,
    /// How the scenario targets were derived (`docs/portfolio-analysis.md`
    /// §Starting parameters); `None` on a not-rated, abstained or role-risk-only
    /// holding.
    pub target_meta: Option<engine::TargetMeta>,
    /// The grade-parameter version the letter was computed under
    /// ([`engine::GRADE_PARAMETER_VERSION`]), so the self-review recognizes a
    /// parameter boundary ([`engine::grade_parameter_change`]). Stamped on every
    /// audit, the early exits included.
    pub grade_parameter_version: String,
    /// The factor composite behind a priced stock's sub-scores, so a run's letters
    /// can be recomputed and the weights shadow-tuned from the record
    /// (`docs/portfolio-analysis.md` §Starting parameters); `None` on a fund and on
    /// every exit before a priced stock's engine output.
    #[serde(deserialize_with = "required_option")]
    pub factors: Option<factors::FactorComposite>,
    /// The closed-form re-anchor basis for the engine-only quick paths
    /// (`docs/portfolio-analysis.md` §The quick check); `None` on not-rated,
    /// abstained and role-risk-only holdings.
    pub quick_basis: Option<engine::QuickCheckBasis>,
    /// The split-bridge anchor bar (`docs/portfolio-analysis.md` §Starting
    /// parameters): the newest settled close strictly before the run's ET session,
    /// from this run's own fetched series. A later engine-only pass re-reads the
    /// same bar date from its fresh fetch, and the close ratio is the cumulative
    /// split re-basis between the two ([`engine::split_bridge_factor`]). `None` on
    /// no-price exits, whose comparisons run unbridged until a full pass stamps one.
    pub authoring_close: Option<engine::DatedValue>,
    /// The fund exposure comparators for the quick check's fund legs
    /// (`docs/portfolio-analysis.md` §Starting parameters); `None` on stocks.
    pub fund_exposure: Option<fund::FundExposureBasis>,
    /// The pre-profit overlay record (`docs/portfolio-analysis.md` §Starting
    /// parameters), present on every priced stock (the eligibility result persists
    /// even when the stock does not enter); `None` on funds and `role_risk_only`
    /// holdings.
    pub pre_profit: Option<pre_profit::PreProfitOverlay>,
    /// The full hurdle read behind the verdict's `dead_money`
    /// (`docs/portfolio-analysis.md` §Starting parameters); `None` on not-rated,
    /// abstained and role-risk-only holdings.
    pub hurdle: Option<engine::HurdleRead>,
    /// Present on a priced stock whose gather ran the 8-K sweep (`Unknown` where
    /// it couldn't); `None` on funds and skipped retrievals.
    pub forensic: Option<ForensicRead>,
    /// The four soft forensic flags ([`soft_forensic::SoftForensicFlags`]), each
    /// typed unevaluable on a missing input and never clear; `None` on funds and
    /// `role_risk_only` holdings.
    pub soft_forensic: Option<soft_forensic::SoftForensicFlags>,
    /// Present where the flag was evaluable (a carried stock with a benchmark
    /// series and a volatility read); an unevaluable flag records its reason in
    /// `degraded_inputs` instead. `None` on debuts and funds.
    pub tech_event_pre_flag: Option<engine::TechEventPreFlag>,
    /// This holding's FINRA short-interest row (`docs/data-sources.md §FINRA`),
    /// held out of every sub-score; `None` on funds, symbols absent from the file
    /// and runs whose file fetch gapped.
    pub short_interest: Option<crate::finra::ShortInterestRead>,
    /// The priced-in anchor the interpretation prompt rendered
    /// (`docs/portfolio-analysis.md` §Starting parameters); `None` on funds, the
    /// current-multiple carry and every early exit.
    pub implied_expectations: Option<engine::ImpliedExpectations>,
    /// The narrative-vs-reality read with, when tripped, its matched soft rule
    /// (the engine arm's Medium ceiling), persisted as an annotation
    /// (`docs/portfolio-workflow.md` §Step 6g); `None` on funds, debuts,
    /// unreadable paces (the reason rides `degraded_inputs`) and every early exit.
    pub narrative: Option<engine::NarrativeRead>,
    /// `None` on holdings with no option legs, funds and skipped retrievals.
    pub option_overlay: Option<dossier::OptionOverlay>,
    /// The research-loop audit record (`docs/storage.md §Local Analysis Suite
    /// Storage`); `None` on every no-research exit.
    pub research: Option<research::ResearchAuditRecord>,
    /// The only research artifact the next run reads; an evidence-floor
    /// abstention retains the prior run's unrewritten, so continuity survives the
    /// exit. `None` on a not-rated row, the listing guard and a debut abstention.
    pub analysis: Option<AnalysisRecord>,
    /// The self-review's prose over the prior position (`docs/portfolio-workflow.md`
    /// §Step 6e), read as text by this run's thesis-document message alone;
    /// `None` on a debut and on every exit before the review.
    pub review: Option<String>,
    /// The highest episode-store check id the review behind this row read
    /// (`docs/portfolio-analysis.md` §Outcome learning): the next review treats a
    /// score as read where the check that last moved it is at or below this mark
    /// and lists the checks above it as new. An abstention carries the prior
    /// row's; a debut, a `role_risk_only` holding and a not-rated row write `None`.
    pub accuracy_read_through: Option<i64>,
}

/// The prompt/schema version stamped on each run's audit and checkpoint header,
/// bumped whenever a model-facing prompt or response contract changes so a
/// checkpoint written under the prior contract cannot resume into the new one
/// (`job::resume_eligibility`). The pin in
/// `prompt_version_is_stamped_for_the_model_arm_domain_gate`
/// (`portfolio/pipeline.rs`) moves with every bump.
pub const PROMPT_VERSION: &str = "portfolio-v77";

/// One complete Portfolio Analysis run, persisted whole (`docs/storage.md §Local
/// Analysis Suite Storage`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioRun {
    pub run_id: String,
    pub created_at: String,
    pub holdings: crate::schwab::Holdings,
    pub verdicts: Vec<HoldingVerdict>,
    pub roll_up: PortfolioRollUp,
    pub audit: Vec<HoldingAudit>,
    /// The run-level `DGS2` / `DGS10` prints the targets and hurdles were computed
    /// from, with their as-of dates — the rate cache the engine-only quick paths
    /// read (`docs/portfolio-analysis.md` §The quick check).
    pub rate_prints: RatePrints,
    /// Holdings whose analysis failed while the run continued
    /// (`docs/portfolio-analysis.md` §Failure posture).
    pub failed_holdings: Vec<HoldingFailure>,
    /// The run's accuracy record (`docs/portfolio-analysis.md` §Outcome learning).
    pub accuracy: outcome::AccuracyRecord,
}

/// A per-holding analysis failure the run isolated (`docs/portfolio-analysis.md`
/// §Failure posture).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingFailure {
    pub symbol: String,
    /// The failing operation plus its root cause — the user-legible card line;
    /// the full error chain rides the run tracker's failed step detail.
    pub cause: String,
    /// Whether a prior verdict was carried forward: the card shows it
    /// vintage-stamped beside the failed badge, or is an empty debut-failure card.
    pub carried_prior: bool,
}

/// `None` where the disposition carries no action; consumed by [`job`]'s carry
/// gate.
pub(crate) fn carried_action(verdict: &HoldingVerdict) -> Option<Action> {
    match &verdict.disposition {
        VerdictDisposition::Priced(g) => Some(g.action),
        VerdictDisposition::RoleRiskOnly(r) => Some(r.action),
        _ => None,
    }
}

/// The persisted rationale beside [`carried_action`] — the model's sentence with
/// the app's caveat where one rode. PRIOR ACTION renders it through
/// `pipeline::investment_sentence`, so the caveat never reaches the decision.
pub(crate) fn carried_rationale(verdict: &HoldingVerdict) -> Option<&str> {
    match &verdict.disposition {
        VerdictDisposition::Priced(g) => Some(g.action_rationale.as_str()),
        VerdictDisposition::RoleRiskOnly(r) => Some(r.action_rationale.as_str()),
        _ => None,
    }
}

/// The persisted run-level rate prints (see [`PortfolioRun::rate_prints`]). The
/// as-of dates are the prints' FRED observation dates; `fetched_at` is the run
/// timestamp, the age fallback where a source carried no observation date.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RatePrints {
    pub dgs2: f64,
    pub dgs10: f64,
    pub dgs2_as_of: Option<String>,
    pub dgs10_as_of: Option<String>,
    pub fetched_at: String,
}

/// The frame sentences every object-returning call's system prompt closes with,
/// after the role line and the output-name sentence; the gathering call, which
/// returns no object, keeps its own frame.
pub(crate) const TWO_PART_FRAME: &str =
    "Part 1 of the message gives the inputs. Part 2 defines those outputs and gives the shape to return.";

/// The appendix message's fields in the message's order
/// (`docs/portfolio-workflow.md` §Step 6f); the schema's `required` set and the
/// return shape are both built from this list, so they cannot diverge.
pub const APPENDIX_KEYS: [&str; 4] = [
    "conviction",
    "expected_price_3m",
    "expected_price_12m",
    "expected_price_3y",
];

/// The JSON Schema handed to Ollama's `format` for the typed appendix: every
/// field required and nullable, `null` the document's silence
/// (`docs/portfolio-analysis.md` §The holding verdict). It stays within the
/// subset the local grammar converter proves out (type / properties / required /
/// enum), so positivity is enforced at decode ([`validate_appendix_domain`]),
/// never as a range keyword.
pub fn appendix_schema() -> Value {
    let price = json!({ "type": ["number", "null"] });
    json!({
        "type": "object",
        "properties": {
            "conviction": {
                "type": ["string", "null"],
                "enum": ["high", "medium", "low", Value::Null]
            },
            "expected_price_3m": price,
            "expected_price_12m": price,
            "expected_price_3y": price
        },
        "required": APPENDIX_KEYS
    })
}

/// The placeholder-only return shape the appendix message closes with.
pub fn appendix_return_shape() -> String {
    placeholder_shape(&appendix_schema(), &APPENDIX_KEYS)
}

/// A schema's nesting with every value blank — a string "", a number 0, a
/// boolean false, an enum its alternatives as "<a|b|c>" (null among them when
/// nullable), an array one item — with each object's keys in `order`, unnamed
/// keys after them alphabetically. The one renderer behind every return shape.
pub(crate) fn placeholder_shape(schema: &Value, order: &[&str]) -> String {
    fn visit(schema: &Value) -> Value {
        if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            // A nullable choice shows both halves — "<a|b|null>" — so null never
            // reads as the expected value.
            let mut names: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
            if values.iter().any(Value::is_null) {
                names.push("null");
            }
            return json!(format!("<{}>", names.join("|")));
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            return Value::Object(properties.iter().map(|(k, v)| (k.clone(), visit(v))).collect());
        }
        if let Some(items) = schema.get("items") {
            return json!([visit(items)]);
        }
        let kind = schema["type"].as_str().or_else(|| {
            schema["type"].as_array()?.iter().filter_map(Value::as_str).find(|t| *t != "null")
        });
        let placeholder = match kind {
            Some("number" | "integer") => json!(0),
            Some("boolean") => json!(false),
            Some("string") => json!(""),
            _ => return Value::Null,
        };
        let nullable = schema["type"].as_array().is_some_and(|ts| ts.iter().any(|t| t == "null"));
        if nullable {
            // A nullable scalar shows both halves too — "<0|null>" — so a number
            // the reply may leave null never reads as 0; a nullable string would
            // read "<text|null>".
            let shown = if kind == Some("string") { "text".to_string() } else { placeholder.to_string() };
            return json!(format!("<{shown}|null>"));
        }
        placeholder
    }
    fn write(v: &Value, order: &[&str]) -> String {
        match v {
            Value::Object(map) => {
                let rank = |key: &str| (order.iter().position(|k| *k == key).unwrap_or(usize::MAX), key.to_string());
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort_by_key(|k| rank(k));
                let fields: Vec<String> = keys
                    .into_iter()
                    .map(|k| format!("{}:{}", serde_json::to_string(k).expect("a key serializes"), write(&map[k], order)))
                    .collect();
                format!("{{{}}}", fields.join(","))
            }
            Value::Array(items) => format!("[{}]", items.iter().map(|v| write(v, order)).collect::<Vec<_>>().join(",")),
            other => serde_json::to_string(other).expect("a JSON value serializes"),
        }
    }
    write(&visit(schema), order)
}

/// The template renderer [`response_template_samples`] materializes its enum
/// choices from; no prompt renders it.
#[cfg(test)]
pub(crate) fn response_shape_contract(schema: &Value) -> String {
    let mut enums = Vec::new();
    fn visit(schema: &Value, path: &str, enums: &mut Vec<String>) -> Value {
        if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            enums.push(format!("{path}: {}", serde_json::to_string(values).unwrap()));
            return if values.iter().any(|v| v == "neutral") {
                serde_json::json!("neutral")
            } else if values.len() == 1 {
                values[0].clone()
            } else {
                serde_json::json!(format!("<{path}>"))
            };
        }
        if schema["type"].as_array().is_some_and(|ts| ts.iter().any(|t| t == "null")) {
            enums.push(format!("{path}: may also be null"));
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            return Value::Object(properties.iter().map(|(key, child)| {
                let next = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                (key.clone(), visit(child, &next, enums))
            }).collect());
        }
        if let Some(items) = schema.get("items") {
            return serde_json::json!([visit(items, &format!("{path}[]"), enums)]);
        }
        let kind = schema["type"].as_str().or_else(|| {
            schema["type"].as_array()?.iter().filter_map(Value::as_str).find(|t| *t != "null")
        });
        match kind {
            Some("number" | "integer") => serde_json::json!(1),
            Some("boolean") => serde_json::json!(false),
            Some("string") => serde_json::json!(format!("<{path}>")),
            _ => Value::Null,
        }
    }
    let example = visit(schema, "", &mut enums);
    format!(
        "\nResponse shape template (illustrative structure, not a completed answer; arrays may be empty). Replace each <field-path> placeholder with that field's value; for enum fields choose one of the Field alternatives below, never the literal placeholder. Sample numbers, booleans and neutral outlooks are not findings:\n{}\nField alternatives (allowed values, not preferences):\n{}\nThe entire response is one JSON object beginning with {{, with no code fence or surrounding prose.\n",
        serde_json::to_string(&example).unwrap(), enums.join("\n")
    )
}

/// Materialize every enum alternative in the rendered template so each caller
/// can test its real decoder; also verifies a placeholder names its exact schema
/// path rather than silently accepting a misspelled field.
#[cfg(test)]
pub(crate) fn response_template_samples(schema: &Value) -> Vec<Value> {
    fn fill(value: &mut Value, schema: &Value, path: &str, choice: usize, width: &mut usize) {
        if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            *width = (*width).max(values.len());
            if !values.contains(value) {
                assert_eq!(*value, serde_json::json!(format!("<{path}>")));
            }
            *value = values[choice % values.len()].clone();
        } else if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            let object = value.as_object_mut().unwrap();
            assert_eq!(object.len(), properties.len());
            for (key, child) in properties {
                let next = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                fill(object.get_mut(key).unwrap(), child, &next, choice, width);
            }
        } else if let Some(items) = schema.get("items") {
            for item in value.as_array_mut().unwrap() {
                fill(item, items, &format!("{path}[]"), choice, width);
            }
        }
    }
    let contract = response_shape_contract(schema);
    let template: Value = serde_json::from_str(contract.lines().find(|line| line.starts_with('{')).unwrap()).unwrap();
    let mut first = template.clone();
    let mut width = 1;
    fill(&mut first, schema, "", 0, &mut width);
    let mut samples = vec![first];
    for choice in 1..width {
        let mut sample = template.clone();
        let mut sample_width = 1;
        fill(&mut sample, schema, "", choice, &mut sample_width);
        assert_eq!(sample_width, width);
        samples.push(sample);
    }
    samples
}

/// The appendix's declared domain, enforced at decode
/// (`docs/portfolio-analysis.md` §The holding verdict): each present price finite
/// and strictly positive, `null` accepted everywhere. The grammar cannot express
/// range keywords ([`appendix_schema`]), so without this gate a zero or negative
/// price would persist and later score. The gate checks the declared domain only,
/// never the engine's values; it rejects the object whole rather than clamping and
/// names every offending field. A typed model-arm field added later is gated here
/// too.
pub fn validate_appendix_domain(appendix: &ThesisAppendix) -> Result<(), AppendixDomainError> {
    let mut violations = Vec::new();
    for (name, value) in [
        ("expected_price_3m", appendix.expected_price_3m),
        ("expected_price_12m", appendix.expected_price_12m),
        ("expected_price_3y", appendix.expected_price_3y),
    ] {
        if let Some(v) = value {
            if !(v.is_finite() && v > 0.0) {
                violations.push(format!("{name} = {v:?} (declared a finite positive price, or null)"));
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(AppendixDomainError { violations })
    }
}

/// Every appendix value outside its declared domain in one response, each entry
/// naming the field and the authored value.
#[derive(Debug, Clone, PartialEq)]
pub struct AppendixDomainError {
    pub violations: Vec<String>,
}

impl std::fmt::Display for AppendixDomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "appendix off its declared domain: {}", self.violations.join("; "))
    }
}

impl std::error::Error for AppendixDomainError {}

/// A `role_risk_only` holding's engine set, rendered into the action call's
/// prompt as evidence: the add family requires return evidence this branch has
/// none of by construction (`docs/portfolio-analysis.md` §Portfolio action). The
/// model's choice stays the full ladder, departures annotated on the audit.
pub const ROLE_RISK_ACTIONS: [Action; 3] = [Action::SellAll, Action::Trim, Action::Hold];

// ---- The per-holding action call (the profile's one entry point) --------------

/// The action call's grammar-constrained output (`docs/portfolio-analysis.md`
/// §Portfolio action), authored after interpretation from the finished intrinsic
/// verdict and the investor profile — the profile's only entry point into the
/// job, so the intrinsic verdict stays profile-independent by input isolation.
/// The enum is the full ladder on both branches; the engine set renders as
/// evidence, never a schema bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDecision {
    pub action: Action,
    pub rationale: String,
}

/// The fields the action call must return; the grammar's `required` set and the
/// contract line are both built from it, so they cannot diverge.
pub const ACTION_KEYS: [&str; 2] = ["action", "rationale"];

/// The output-names sentence the action call's system prompt states once; the
/// fence rule and the field meanings live in the message's Part 2, and the
/// shape is [`action_return_shape`].
pub fn action_response_contract() -> String {
    let (last, head) = ACTION_KEYS.split_last().expect("the key list is never empty");
    format!("You will return {} and {last}, as one JSON object.", head.join(", "))
}

/// The placeholder-only return shape the action message closes with.
pub fn action_return_shape() -> String {
    placeholder_shape(&action_decision_schema(), &ACTION_KEYS)
}

/// The JSON Schema for [`ActionDecision`] — the action enum lists the full
/// ladder on every branch (engine evidence annotates, never bars).
pub fn action_decision_schema() -> Value {
    let all = [
        Action::SellAll,
        Action::Trim,
        Action::Hold,
        Action::Add,
        Action::AddAggressively,
    ];
    let actions: Vec<&str> = all.iter().map(Action::as_kebab).collect();
    json!({
        "type": "object",
        "properties": {
            "action": { "type": "string", "enum": actions },
            "rationale": { "type": "string" }
        },
        "required": ACTION_KEYS
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filings_read_keeps_the_lookbacks_eight_ks_beside_the_sweep_state() {
        let filing = |form: &str, date: &str, items: Option<&[&str]>| crate::sec::RecentFiling {
            form: form.into(),
            filing_date: date.into(),
            items: items.map(|i| i.iter().map(|s| s.to_string()).collect()),
            accession: String::new(),
        };
        let filings = vec![
            filing("10-Q", "2026-08-01", Some(&[])),
            filing("8-K", "2026-07-23", Some(&["2.02", "9.01"])),
            filing("8-K/A", "2026-09-01", Some(&["5.02"])),
            filing("8-K", "2025-01-15", Some(&["8.01"])),
        ];
        let read = FilingSweepRead::from_filings("AAPL", &filings, "2025-10-08");
        assert!(
            matches!(read.state, ForensicFilingState::Clear),
            "{:?}",
            read.state
        );
        assert_eq!(
            read.filings_8k
                .iter()
                .map(|f| f.filing_date.as_str())
                .collect::<Vec<_>>(),
            ["2026-09-01", "2026-07-23"],
            "8-K and 8-K/A rows inside the lookback, newest first; the 10-Q and the old 8-K out"
        );
        // A 4.01 item classifies an event, and the rows still ride beside it.
        let events = vec![filing("8-K", "2026-06-10", Some(&["4.01"]))];
        let read = FilingSweepRead::from_filings("AAPL", &events, "2025-10-08");
        assert!(
            matches!(read.state, ForensicFilingState::Events { .. }),
            "{:?}",
            read.state
        );
        assert_eq!(read.filings_8k.len(), 1);
        // An unclassifiable row types the sweep unknown and lists nothing.
        let unknown = vec![filing("8-K", "2026-06-10", None)];
        let read = FilingSweepRead::from_filings("AAPL", &unknown, "2025-10-08");
        assert!(
            matches!(
                read.state,
                ForensicFilingState::Unknown { queried: true, .. }
            ),
            "{:?}",
            read.state
        );
        assert!(read.filings_8k.is_empty());
    }

    #[test]
    fn the_appendix_shape_is_four_nullable_keys_in_the_message_order() {
        // The appendix message carries no contract line of its own: the
        // conversation's system message is the thesis document's.
        let shape_text = appendix_return_shape();
        assert_eq!(
            shape_text,
            "{\"conviction\":\"<high|medium|low|null>\",\"expected_price_3m\":\"<0|null>\",\
             \"expected_price_12m\":\"<0|null>\",\"expected_price_3y\":\"<0|null>\"}"
        );
        for narration in ["Field notes", "Field alternatives", "code fence", "<conviction>"] {
            assert!(!shape_text.contains(narration), "`{narration}`: {shape_text}");
        }
        let schema = appendix_schema();
        let required: Vec<&str> = schema["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(required, APPENDIX_KEYS.to_vec());
        assert_eq!(schema["properties"]["conviction"]["type"], json!(["string", "null"]));
        assert!(schema["properties"]["conviction"]["enum"].as_array().unwrap().iter().any(Value::is_null));
        for key in &APPENDIX_KEYS[1..] {
            assert_eq!(schema["properties"][*key]["type"], json!(["number", "null"]), "{key}");
        }
        for sample in response_template_samples(&schema) {
            let decoded: ThesisAppendix = serde_json::from_value(sample).unwrap();
            assert!(validate_appendix_domain(&decoded).is_ok());
        }
        // A missing key reads as null too: model-written JSON stays lenient, and
        // the grammar requires every key, so a decode never sees one missing.
        let silent: ThesisAppendix = serde_json::from_value(json!({
            "conviction": null, "expected_price_3m": null,
            "expected_price_12m": null, "expected_price_3y": null
        }))
        .unwrap();
        assert!(silent.is_empty());
        assert_eq!(silent, ThesisAppendix::NONE);
        let sparse: ThesisAppendix = serde_json::from_value(json!({ "conviction": "high" })).unwrap();
        assert_eq!(sparse, ThesisAppendix { conviction: Some(Conviction::High), ..ThesisAppendix::NONE });
        let mixed: ThesisAppendix = serde_json::from_value(json!({
            "conviction": "low", "expected_price_3m": null,
            "expected_price_12m": 145.0, "expected_price_3y": 210.5
        }))
        .unwrap();
        assert_eq!(mixed.conviction, Some(Conviction::Low));
        assert_eq!(mixed.expected_prices()[0], ("three-month", None));
        assert_eq!(mixed.expected_prices()[1], ("twelve-month", Some(145.0)));
        assert!(!mixed.is_empty());
        // The action contract is the output names alone; the fence sentence and
        // the field meanings live in the message's Part 2.
        assert_eq!(action_response_contract(), "You will return action and rationale, as one JSON object.");
        assert_eq!(action_return_shape(), "{\"action\":\"<sell-all|trim|hold|add|add-aggressively>\",\"rationale\":\"\"}");
    }

    #[test]
    fn appendix_domain_admits_null_and_any_finite_positive_price() {
        assert!(validate_appendix_domain(&ThesisAppendix::NONE).is_ok());
        let edges = ThesisAppendix {
            conviction: Some(Conviction::High),
            expected_price_3m: Some(1e-9),
            expected_price_12m: None,
            expected_price_3y: Some(1e300),
        };
        assert!(validate_appendix_domain(&edges).is_ok());
    }

    #[test]
    fn appendix_domain_rejects_every_off_domain_price_and_names_each() {
        let off = ThesisAppendix {
            conviction: None,
            expected_price_3m: Some(0.0),
            expected_price_12m: Some(-12.5),
            expected_price_3y: Some(f64::INFINITY),
        };
        let err = validate_appendix_domain(&off).unwrap_err();
        assert_eq!(err.violations.len(), 3, "{err}");
        let text = err.to_string();
        assert!(text.starts_with("appendix off its declared domain: "), "{text}");
        for needle in [
            "expected_price_3m = 0.0 (declared a finite positive price, or null)",
            "expected_price_12m = -12.5",
            "expected_price_3y = inf",
        ] {
            assert!(text.contains(needle), "{needle} missing from: {text}");
        }
        let one = ThesisAppendix { expected_price_12m: Some(f64::NAN), ..ThesisAppendix::NONE };
        let err = validate_appendix_domain(&one).unwrap_err();
        assert_eq!(err.violations.len(), 1, "{err}");
        assert!(!err.to_string().contains("expected_price_3m"), "{err}");
    }

    /// The frontend types against these snake_case keys and label strings.
    #[test]
    fn investor_profile_display_pins_preset_rows() {
        let shape = serde_json::to_value(InvestorProfile::default_fixture().display()).unwrap();
        assert_eq!(
            shape,
            json!({
                "objective":
                    "maximize profit (total return; no income or capital-preservation mandate)",
                "risk_tolerance": "aggressive (medium-to-high)",
                "horizon": "long-term (durable multi-quarter / multi-year theses)",
                "tax": "tax-aware — tax consequences are an optional caveat, with no effect on \
                        the action; account type, tax lots, holding periods, and rates are unmodeled",
                "cash": "unconstrained — adds are never gated on observed Schwab cash",
            })
        );
    }


    #[test]
    fn a_stamped_vintage_wins_over_the_run_date() {
        let stamped = json!({
            "symbol": "AAPL",
            "asset_class": "stock",
            "position_change": "unchanged",
            "basis_move": null,
            "disposition": { "status": "not-rated", "reason": "fixture" },
            "analyzed_at": "2026-07-01T09:00:00+00:00",
            "action_source": "rule-demoted",
            "side_reversed": false
        });
        let parsed: HoldingVerdict = serde_json::from_value(stamped).unwrap();
        assert_eq!(
            effective_vintage(&parsed, "2026-08-03T12:00:00+00:00"),
            "2026-07-01T09:00:00+00:00",
            "a carried verdict keeps its own vintage inside a newer run"
        );
        assert_eq!(parsed.action_source, ActionSource::RuleDemoted);
    }

    #[test]
    fn a_priced_verdict_round_trips_its_document_and_a_null_bearing_appendix() {
        let graded = GradedVerdict {
            grade: Grade::B,
            sub_scores: SubScores { quality: 70.0, valuation: 55.0, momentum: 60.0, risk: 65.0 },
            action: Action::Hold,
            action_rationale: "Hold on an intact thesis.".into(),
            thesis_document: "Thesis: the franchise compounds.\n\nSummary: hold.".into(),
            appendix: ThesisAppendix {
                conviction: Some(Conviction::Medium),
                expected_price_3m: None,
                expected_price_12m: Some(145.0),
                expected_price_3y: None,
            },
            price_targets: PriceTargets { three_month: None, twelve_month: None, three_year: None },
            options_signal: OptionsSignal {
                put_call_volume: None,
                put_call_open_interest: None,
                implied_volatility: None,
                iv_skew: None,
            },
            risk_tier: RiskTier::Medium,
            dead_money: HurdleState::Indeterminate,
            low_confidence_grade: false,
            fund_class_label: None,
            grade_basis: None,
            engine_rung: Action::Hold,
            authored_band_relation: None,
        };
        let verdict = HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            basis_move: None,
            disposition: VerdictDisposition::Priced(Box::new(graded.clone())),
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        };
        let s = serde_json::to_value(&verdict).unwrap();
        assert_eq!(s["disposition"]["appendix"]["expected_price_3m"], Value::Null);
        assert_eq!(s["disposition"]["appendix"]["expected_price_12m"], 145.0);
        assert!(s["disposition"].get("thesis_ledger").is_none());
        assert!(s.get("thesis_ledger").is_none());
        let back: HoldingVerdict = serde_json::from_value(s).unwrap();
        assert_eq!(back, verdict);
        assert_eq!(back.thesis_document(), Some(graded.thesis_document.as_str()));
        assert_eq!(back.appendix(), Some(&graded.appendix));
    }

    #[test]
    fn an_abstention_retains_the_prior_thesis_document_and_a_not_rated_carries_none() {
        let abstained = HoldingVerdict {
            symbol: "PGNY".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            basis_move: None,
            disposition: VerdictDisposition::InsufficientEvidence {
                reason: "below the floor".into(),
                prior_thesis_document: Some("The prior document.".into()),
            },
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        };
        let s = serde_json::to_value(&abstained).unwrap();
        assert_eq!(s["disposition"]["status"], "insufficient-evidence");
        assert_eq!(s["disposition"]["prior_thesis_document"], "The prior document.");
        let back: HoldingVerdict = serde_json::from_value(s).unwrap();
        assert_eq!(back.thesis_document(), Some("The prior document."));
        assert_eq!(back.appendix(), None);
        let not_rated = HoldingVerdict {
            disposition: VerdictDisposition::NotRated { reason: "cash".into() },
            ..abstained
        };
        assert_eq!(not_rated.thesis_document(), None);
    }

    #[test]
    fn the_action_schema_advertises_the_full_ladder() {
        let action_schema = action_decision_schema();
        let actions = action_schema["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(actions.len(), 5);
        assert!(actions.iter().any(|a| a == "add-aggressively"));
        assert_eq!(
            action_schema["required"].as_array().unwrap().len(),
            ACTION_KEYS.len()
        );
    }

    #[test]
    fn asset_class_gradeability_matches_the_equity_pipeline() {
        assert!(AssetClass::Stock.is_gradeable());
        assert!(AssetClass::Etf.is_gradeable());
        assert!(!AssetClass::OptionContract.is_gradeable());
        assert!(!AssetClass::Cash.is_gradeable());
    }

    #[test]
    fn verdict_disposition_serializes_with_a_status_tag() {
        let v = VerdictDisposition::NotRated {
            reason: "option position".into(),
        };
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s["status"], "not-rated");
        assert_eq!(s["reason"], "option position");
    }

    #[test]
    fn role_risk_only_serializes_its_own_branch() {
        let v = VerdictDisposition::RoleRiskOnly(Box::new(RoleRiskVerdict {
            class_label: "bond fund".into(),
            thesis_document: "Role: the core fixed-income sleeve.".into(),
            exposure_tilt: vec![ExposureWeight { label: "United States".into(), weight: 0.97 }],
            expense_drag: Some(0.0003),
            observable_risk: Some(0.06),
            structural_flag: false,
            is_cef: false,
            nav_premium: None,
            evidence_gaps: vec!["valuation: no on-plan duration/credit surface".into()],
            action: Action::Hold,
            action_rationale: String::new(),
        }));
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s["status"], "role-risk-only");
        assert_eq!(s["class_label"], "bond fund");
        assert!(s.get("grade").is_none());
        assert!(s.get("price_targets").is_none());
        assert!(s.get("appendix").is_none());
        assert!(s.get("conviction").is_none());
        assert_eq!(s["thesis_document"], "Role: the core fixed-income sleeve.");
        let round: VerdictDisposition = serde_json::from_value(s).unwrap();
        assert_eq!(round, v);
    }

}
