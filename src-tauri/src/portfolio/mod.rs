//! Portfolio Analysis — the local-suite job that grades the user's holdings and
//! recommends an action for each (`docs/portfolio-analysis.md`). This is the
//! narrow single-equity slice (Phase 2): the per-holding pipeline end to end —
//! deterministic dossier ([`dossier`]) → deterministic financial-analysis engine
//! ([`engine`]) → local-model interpretation ([`pipeline`]) → schema-valid verdict
//! → persisted run ([`store`]) → the run lifecycle ([`job`]) — validated offline,
//! against a fixture Schwab source ([`crate::schwab`]) plus FMP + SEC EDGAR.
//!
//! This module root holds the **domain types** the stages exchange: the holding
//! verdict and its parts, the investor profile, and the durable plan-time
//! parameters pinned for this slice. The split between the deterministic engine and
//! the model is load-bearing (`docs/local-models.md §Context-memory discipline`):
//! the engine computes every *number* (sub-scores, the composite grade, scenario
//! price targets, the options-activity signal); the model *interprets* — it picks
//! the action, conviction, horizon reads, and writes the prose, but never invents a
//! figure. The grade is therefore a deterministic roll-up of the engine's
//! sub-scores, not a model gestalt.

pub mod diff;
pub mod dossier;
pub mod engine;
pub mod fund;
pub mod job;
pub mod pipeline;
pub mod store;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ---- Durable plan-time parameters (pinned this slice) ------------------------
//
// These three are pinned because they shape retention, the house-view loader, and
// the verdict schema; the grade-weight formula, risk-tier thresholds, and
// options-signal parameters are deliberately left calibratable (in `engine`), to be
// shadow-tuned against live runs rather than frozen now.

/// How many Portfolio Analysis runs are retained (newest-N), pruned independently of
/// the 30-report report-retention window and of Trade Opportunities
/// (`docs/storage.md §Local Analysis Suite Storage`). Pinned at N=10 this slice.
pub const PORTFOLIO_RUN_RETENTION: u32 = 10;

/// How many recent Market Signal reports load as the house-view context for a
/// holding's dossier (`docs/portfolio-analysis.md` — the report is a read-only shared
/// input, loaded deterministically, never vector-searched). Pinned at X=3, matching
/// the research router's existing recent-report window (`pipeline::ROUTER_RECENT_REPORTS`).
pub const HOUSE_VIEW_RECENT_REPORTS: u32 = 3;

/// The three horizon-outlook windows the verdict reads (`docs/portfolio-analysis.md`
/// §The holding verdict). Lengths pinned this slice — short ≈ 1 month, mid ≈ 1 year,
/// long ≈ 3–5 years — and surfaced in the interpretation prompt so the model's reads
/// share one definition across runs.
pub const HORIZON_SHORT: &str = "short term (~1 month)";
pub const HORIZON_MID: &str = "mid term (~1 year)";
pub const HORIZON_LONG: &str = "long term (~3–5 years)";

// ---- Investor profile --------------------------------------------------------

/// The configured investor profile that personalizes grading and especially the
/// action (`docs/portfolio-analysis.md`, `docs/configuration.md`). The grade is
/// intrinsic to the holding; the *action* additionally reflects horizon, risk
/// tolerance, tax sensitivity, and available cash. For this slice it is seeded as a
/// fixture ([`InvestorProfile::default_fixture`]); the configurable Settings form is
/// a later slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestorProfile {
    pub risk_tolerance: RiskTolerance,
    pub horizon: ProfileHorizon,
    /// Whether holdings sit in a taxable account (so realized gains carry a tax
    /// cost the action sizing should weigh) versus tax-advantaged.
    pub tax_sensitive: bool,
    /// Cash / buying power available for new purchases, in account currency.
    /// `Some(cap)` bounds a buy in `engine::size_action`; **`None` means cash is
    /// unconstrained** — the fixed preset's stance (the user may hold cash the app can't
    /// see), so adds are not gated on observed Schwab cash
    /// (`docs/configuration.md` §Investor Profile).
    pub available_cash: Option<f64>,
}

impl InvestorProfile {
    /// The default fixture profile this slice runs against: moderate risk tolerance,
    /// a long-term horizon, taxable/tax-aware, and **cash treated as unconstrained**
    /// (the preset's stance — the user may hold cash the app can't see). The real
    /// per-user profile is configured in a later Settings slice; this stands in so the
    /// action sizing has a profile to read.
    pub fn default_fixture() -> Self {
        Self {
            risk_tolerance: RiskTolerance::Moderate,
            horizon: ProfileHorizon::LongTerm,
            tax_sensitive: true,
            // Unconstrained cash — adds are not gated on observed Schwab cash
            // (`docs/configuration.md` §Investor Profile).
            available_cash: None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileHorizon {
    ShortTerm,
    MediumTerm,
    LongTerm,
}

// ---- Asset eligibility -------------------------------------------------------

/// A position's asset class, decided before analysis (`docs/portfolio-analysis.md`
/// §Asset eligibility). The equity-centric pipeline applies cleanly only to
/// individual stocks (full) and in reduced form to funds; everything else is marked
/// not-rated rather than given a fabricated grade.
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
    /// Whether the equity pipeline (FMP/SEC company financials) can grade this class.
    /// Stocks get the full verdict; ETFs/funds a reduced one; the rest are not rated.
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

/// How a current position changed versus the prior run's persisted snapshot
/// (`docs/portfolio-analysis.md` §Holdings change tracking). Classified
/// deterministically by the app from quantity, before any model stage — the
/// compute-don't-guess boundary the pipeline holds. `New` covers both a genuinely new
/// position and every position on a first run (no prior snapshot to diff against).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionChange {
    New,
    Increased,
    Decreased,
    /// The neutral state (no add/trim detected), and the default a run persisted
    /// before this field existed decodes to — so a legacy verdict claims no user
    /// action rather than fabricating one.
    #[default]
    Unchanged,
}

/// The prior-run comparison for one current position, carried into its dossier so the
/// verdict reasons over what the user actually did — added to, trimmed, or left the
/// position — rather than re-grading it in a vacuum. Prior quantity / cost basis are
/// `None` for a `New` position (no prior counterpart).
///
/// Runtime-only — it rides in the (unserialized) [`dossier::HoldingDossier`], so it
/// carries no serde derives; the structured tag that *is* persisted on the verdict is
/// [`PositionChange`].
#[derive(Debug, Clone, PartialEq)]
pub struct PositionDelta {
    pub change: PositionChange,
    pub prior_quantity: Option<f64>,
    pub prior_cost_basis: Option<f64>,
}

impl PositionDelta {
    /// The delta for a position with no prior-run counterpart (a new holding, or any
    /// holding on a first run).
    pub fn new_position() -> Self {
        Self {
            change: PositionChange::New,
            prior_quantity: None,
            prior_cost_basis: None,
        }
    }
}

/// A position present in the prior run's snapshot but absent now — an exited
/// (closed-since-last-run) position. It earns no per-holding verdict (nothing left to
/// grade) but is surfaced in the roll-up so a sold-out name is acknowledged rather than
/// silently vanishing from the run (`docs/portfolio-analysis.md` §Holdings change
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

/// The composite letter grade, rolled up deterministically from the engine's four
/// sub-scores (`docs/portfolio-analysis.md` — "the letter rolls up from real
/// metrics, not a model's gestalt"). Fixed vocabulary, like the report's regime
/// labels, so verdicts stay comparable across runs.
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

/// The four deterministically-computed sub-scores the composite grade rolls up from,
/// each normalized to 0–100 where **higher is better** (the risk sub-score is
/// inverted at source, so a safer holding scores higher). Computed by [`engine`]
/// from FMP/SEC fundamentals; never authored by the model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SubScores {
    pub quality: f64,
    pub valuation: f64,
    pub momentum: f64,
    pub risk: f64,
}

/// The action ladder (`docs/portfolio-analysis.md` §The holding verdict) — a fixed
/// vocabulary so verdicts stay comparable and the model can't retreat into hedged
/// language. The model selects the rung **within the engine-bounded feasible set**;
/// the sizing is computed deterministically.
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
}

/// The deterministic risk tier (`docs/portfolio-analysis.md` §Starting parameters —
/// assigned per branch in the engine stage; Trade Opportunities' High/Low/else-Medium
/// rule is canonical for priced stocks, a fund mapping for priced equity funds; a
/// `role_risk_only` holding carries none). Scales the capital-efficiency hurdle
/// premium and rides the audit.
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

/// The three-state capital-efficiency / dead-money read (`docs/portfolio-analysis.md`
/// §Starting parameters): **clears** when even the bear-case total return clears the
/// tier-scaled hurdle; **fails** when even the bull case misses it (only this state
/// is dead money); **indeterminate** otherwise — a point estimate missing the hurdle
/// inside its own scenario dispersion proves nothing. `unscorable` when the scenario
/// total returns could not be computed.
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

/// A directional read for one horizon window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HorizonRead {
    Bullish,
    Neutral,
    Bearish,
}

/// Separate short-, mid-, and long-term reads (`docs/portfolio-analysis.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HorizonOutlook {
    pub short: HorizonRead,
    pub mid: HorizonRead,
    pub long: HorizonRead,
}

/// One scenario price target with its methodology exposed (`docs/portfolio-analysis.md`
/// — "computed by the financial-analysis engine as scenario outputs with their
/// methodology and assumptions exposed"). The model selects and justifies the base
/// case; it never invents the number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceTarget {
    /// The base-case target value (account currency).
    pub base: f64,
    /// The bearish and bullish scenario bounds bracketing the base case.
    pub bear: f64,
    pub bull: f64,
    /// A one-line statement of how the targets were derived (the exposed methodology).
    pub methodology: String,
}

/// One-month and twelve-month scenario targets — **rolling windows from the run
/// date**, not calendar ends (the settled rename of the as-built end-of-month /
/// end-of-year fields: outside January, calendar year-end is not twelve months away,
/// and calibration scores these against the 1- and 12-month labels —
/// `docs/portfolio-analysis.md` §Starting parameters). Each `None` when the inputs to
/// derive it were missing. The serde aliases keep runs persisted under the old field
/// names decodable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceTargets {
    #[serde(alias = "end_of_month")]
    pub one_month: Option<PriceTarget>,
    #[serde(alias = "end_of_year")]
    pub twelve_month: Option<PriceTarget>,
}

/// The per-stock options-activity signal computed from the Schwab option chain
/// (`docs/schwab-integration.md`) — a rough *activity proxy*, not positioning truth.
/// Deliberately **kept out of the grade sub-scores until shadow-mode calibration**
/// shows it adds value; it grounds the narrative read only. Any field is `None` when
/// the chain lacked the data to compute it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionsSignal {
    /// Put/call ratio by traded volume across the chain.
    pub put_call_volume: Option<f64>,
    /// Put/call ratio by open interest.
    pub put_call_open_interest: Option<f64>,
    /// At-the-money implied volatility (a simple chain-wide proxy).
    pub implied_volatility: Option<f64>,
    /// Put-minus-call IV skew (positive = puts richer, a hedging-demand tell).
    pub iv_skew: Option<f64>,
}

/// The deterministic action sizing the engine derives once the model has chosen the
/// action rung (`docs/portfolio-analysis.md` — "a target portfolio-weight range and
/// an estimated share/dollar adjustment, bounded by concentration limits, available
/// cash, and tax sensitivity"). No orders are ever placed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionSizing {
    /// The target portfolio-weight band (fractions, 0.0–1.0).
    pub target_weight_low: f64,
    pub target_weight_high: f64,
    /// Estimated share and dollar adjustment to reach the band's midpoint
    /// (negative = sell). `None` when it can't be sized (no price, no portfolio value).
    pub est_share_delta: Option<f64>,
    pub est_dollar_delta: Option<f64>,
}

/// The priced body of a holding verdict — present only when the holding was eligible,
/// priceable, *and* cleared the evidence floor. Numbers (grade, sub-scores, targets,
/// tier, hurdle, options signal, sizing) come from the engine; the action, conviction,
/// horizon reads, and prose come from the model's interpretation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradedVerdict {
    pub grade: Grade,
    pub sub_scores: SubScores,
    pub action: Action,
    pub action_sizing: ActionSizing,
    pub conviction: Conviction,
    pub horizon_outlook: HorizonOutlook,
    pub price_targets: PriceTargets,
    /// The model's justification for the engine's base-case target (it selects and
    /// explains; the engine computed the figure). Persisted so a verdict's
    /// target basis stays inspectable.
    pub price_target_rationale: String,
    pub options_signal: OptionsSignal,
    /// The deterministic per-branch risk tier (`docs/portfolio-analysis.md` §Starting
    /// parameters). `#[serde(default)]` so a run persisted before the field decodes.
    #[serde(default)]
    pub risk_tier: Option<RiskTier>,
    /// The three-state capital-efficiency / dead-money read — only `fails` is dead
    /// money. `#[serde(default)]` for pre-field runs.
    #[serde(default)]
    pub dead_money: Option<HurdleState>,
    /// True when the letter rests on an imputed (neutral-50) sub-score — the visible
    /// low-confidence marker beside the letter (`docs/portfolio-analysis.md` §Asset
    /// eligibility, the priced-fund grade contract; also any stock graded over an
    /// imputed axis). `#[serde(default)]` for pre-field runs.
    #[serde(default)]
    pub low_confidence_grade: bool,
    /// A concise read of the company's financial health (model prose).
    pub financial_summary: String,
    /// The continuity diff against the prior run (model prose, or "new holding").
    pub what_changed: String,
}

/// One exposure weight (a sector or country label and its fraction of the fund).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExposureWeight {
    pub label: String,
    pub weight: f64,
}

/// The `role_risk_only` branch of an analyzed verdict (`docs/portfolio-analysis.md`
/// §Intrinsic verdict): a structurally unpriceable vehicle class gets a typed role /
/// risk read — **no letter, no price targets, no conviction, no tier** — while the
/// action machinery still applies over the reduced {sell all, trim, hold} spine.
/// Engine-computed fields (exposure, expense, risk, gaps) plus the model's role read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleRiskVerdict {
    /// The deterministic classification label (e.g. "bond fund", "leveraged / inverse
    /// vehicle", "ex-US equity fund below the US-exposure guard").
    pub class_label: String,
    /// The model's role read: the mandate and the exposure the vehicle exists to
    /// supply, read in isolation (prose).
    pub role_summary: String,
    /// Top exposure weights (sector or country), engine-computed from the weightings.
    pub exposure_tilt: Vec<ExposureWeight>,
    /// The expense ratio as an annual return headwind, where reported.
    pub expense_drag: Option<f64>,
    /// Annualized realized volatility — the observable risk read, where computable.
    pub observable_risk: Option<f64>,
    /// The deterministic structurally-path-dependent flag (leveraged / inverse and
    /// option-overlay vehicles).
    pub structural_flag: bool,
    /// The typed evidence gaps — this branch's confidence surface (never a fabricated
    /// High / Medium / Low conviction).
    pub evidence_gaps: Vec<String>,
    /// The action from the reduced {sell all, trim, hold} spine.
    pub action: Action,
    pub action_sizing: ActionSizing,
    /// The continuity diff against the prior run (model prose, or "new holding").
    pub what_changed: String,
}

/// What a holding's analysis resolved to (`docs/portfolio-analysis.md` §Intrinsic
/// verdict): the outer three-arm disposition — analyzed / can't-grade / shouldn't-grade
/// — with the analyzed verdict a **discriminated union of two branches**: the default
/// `priced` record (the full read) and the `role_risk_only` read for a structurally
/// unpriceable vehicle class. A not-rated position never receives a fabricated grade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "status")]
pub enum VerdictDisposition {
    // Boxed: the priced body dwarfs the string variants, so without indirection
    // every disposition would be sized to it. The `graded` alias keeps runs
    // persisted by the single-equity slice decodable as the priced branch.
    #[serde(alias = "graded")]
    Priced(Box<GradedVerdict>),
    /// A structurally unpriceable vehicle class — the typed role / risk read
    /// (`docs/portfolio-analysis.md` §Asset eligibility), never `insufficient-evidence`
    /// (the evidence isn't deficient; the class is unpriceable to this pipeline).
    RoleRiskOnly(Box<RoleRiskVerdict>),
    /// Ineligible asset class (option, bond, cash, …) — excluded from grading.
    NotRated { reason: String },
    /// Eligible but below the evidence floor — an explicit abstention, never a
    /// low-conviction guess.
    InsufficientEvidence { reason: String },
}

/// One holding's complete verdict record, persisted per run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingVerdict {
    pub symbol: String,
    pub asset_class: AssetClass,
    /// How the position changed since the prior run — set by the app from the
    /// deterministic holdings diff ([`diff`]; `docs/portfolio-analysis.md` §What
    /// changed: the what-changed line carries the position delta), never authored by
    /// the model. `#[serde(default)]` so a run persisted before the field existed
    /// still decodes.
    #[serde(default)]
    pub position_change: PositionChange,
    pub disposition: VerdictDisposition,
}

// ---- Run-level aggregate (persisted per run) ---------------------------------

/// The portfolio-level view produced after the per-holding pass
/// (`docs/portfolio-analysis.md` §Portfolio roll-up): concentration and a cash
/// stance, read against the house view and the profile. For this single-equity
/// slice it is a deterministic summary; the 122B synthesis pass is a later slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioRollUp {
    pub graded_count: usize,
    pub not_rated_count: usize,
    pub insufficient_evidence_count: usize,
    /// Analyzed holdings on the `role_risk_only` branch (`docs/portfolio-analysis.md`
    /// §Intrinsic verdict) — counted beside the priced (graded) holdings, never
    /// pooled with them. `#[serde(default)]` for pre-field runs.
    #[serde(default)]
    pub role_risk_only_count: usize,
    /// The largest single-position weight (0.0–1.0) — the concentration read.
    pub top_position_weight: f64,
    /// Cash as a fraction of the account total.
    pub cash_weight: f64,
    /// Positions closed since the last run (`docs/portfolio-analysis.md` §Holdings
    /// change tracking) — graded nowhere, but acknowledged here rather than silently
    /// dropped. Empty on a first run or when nothing was sold. `#[serde(default)]` so a
    /// run persisted before the field existed still decodes.
    #[serde(default)]
    pub exited: Vec<ExitedPosition>,
    /// A short deterministic synthesis line.
    pub overview: String,
}

/// One holding's audit record (`docs/storage.md §Local Analysis Suite Storage`):
/// what the verdict was based on, so a run is traceable and reviewable — the
/// computed metrics and price-target methodology behind the numbers, the sources
/// used, the model ids, the prompt/schema version, and any degraded-input flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingAudit {
    pub symbol: String,
    pub metrics: engine::ComputedMetrics,
    /// The data sources consulted, with a note each (e.g. "FMP company financials").
    pub sources: Vec<String>,
    /// The local model ids used (reasoner / fast), for reproducibility.
    pub model_ids: Vec<String>,
    /// The prompt/schema version the interpretation ran under.
    pub prompt_version: String,
    /// Inputs a source could not resolve, carried from the financials' gap manifest.
    pub degraded_inputs: Vec<String>,
    /// How the scenario targets were derived — rung, fallbacks, and the parameter
    /// version target calibration keys on (`docs/portfolio-analysis.md` §Outcome
    /// learning). `None` on a not-rated / abstained / role-risk-only holding, and on
    /// runs persisted before the field existed (`#[serde(default)]`).
    #[serde(default)]
    pub target_meta: Option<engine::TargetMeta>,
}

/// The schema/prompt version stamped on each run's audit, bumped when the
/// interpretation contract changes so older runs stay legible. v2: the verdict union
/// (priced / role-risk-only), the engine-bounded feasible action set, the v2
/// rate-anchored scenario targets, and the rolling-window target rename.
pub const PROMPT_VERSION: &str = "portfolio-v2";

/// One complete Portfolio Analysis run, persisted whole (`docs/storage.md §Local
/// Analysis Suite Storage`): the holdings snapshot it ran against, the per-holding
/// verdicts, the roll-up, and the per-holding audit records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioRun {
    pub run_id: String,
    pub created_at: String,
    pub holdings: crate::schwab::Holdings,
    pub verdicts: Vec<HoldingVerdict>,
    pub roll_up: PortfolioRollUp,
    pub audit: Vec<HoldingAudit>,
}

// ---- The model's schema-constrained interpretation ---------------------------

/// The model's grammar-constrained output (Ollama native `format`) — the only thing
/// the 122B authors. Every *number* in the final verdict comes from the engine; this
/// carries the judgment calls (action, conviction, horizon reads) and the prose. A
/// schema-valid object is guaranteed by grammar-constrained decoding, so there is no
/// parse-and-pray path (`docs/local-models.md §Schema-constrained output`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interpretation {
    pub action: Action,
    pub conviction: Conviction,
    pub horizon_outlook: HorizonOutlook,
    pub financial_summary: String,
    /// The model's justification for the engine's base-case price target (it selects
    /// and explains; the engine computed the figure).
    pub price_target_rationale: String,
    pub what_changed: String,
}

/// The JSON Schema handed to Ollama's `format` so the interpretation is structurally
/// valid by construction. Mirrors [`Interpretation`]'s shape; enums are string enums
/// with the same kebab labels serde uses, so the decoded object round-trips. The
/// action enum lists only the **engine-bounded feasible set** for this holding
/// (`docs/portfolio-analysis.md` §Starting parameters — the feasible-set rule: the
/// prompt states the allowed set; the model chooses within it, enforced structurally
/// here).
pub fn interpretation_schema(feasible: &[Action]) -> Value {
    let read = json!({ "type": "string", "enum": ["bullish", "neutral", "bearish"] });
    let actions: Vec<&str> = feasible.iter().map(Action::as_kebab).collect();
    json!({
        "type": "object",
        "properties": {
            "action": { "type": "string", "enum": actions },
            "conviction": { "type": "string", "enum": ["high", "medium", "low"] },
            "horizon_outlook": {
                "type": "object",
                "properties": { "short": read, "mid": read, "long": read },
                "required": ["short", "mid", "long"]
            },
            "financial_summary": { "type": "string" },
            "price_target_rationale": { "type": "string" },
            "what_changed": { "type": "string" }
        },
        "required": [
            "action", "conviction", "horizon_outlook",
            "financial_summary", "price_target_rationale", "what_changed"
        ]
    })
}

/// The model's schema-constrained output for a **`role_risk_only`** holding — the
/// union's other branch (`docs/portfolio-analysis.md` §Intrinsic verdict): the role
/// read and the continuity note, plus an action from the reduced spine. None of the
/// priced fields exist — no grade, conviction, horizon, or target rationale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleRiskInterpretation {
    /// Reduced spine only: sell-all / trim / hold.
    pub action: Action,
    /// The vehicle's mandate and the exposure it exists to supply (prose).
    pub role_summary: String,
    pub what_changed: String,
}

/// The reduced action spine a `role_risk_only` holding's feasible set offers —
/// the add family requires return evidence this branch has none of by construction
/// (`docs/portfolio-analysis.md` §Portfolio action).
pub const ROLE_RISK_ACTIONS: [Action; 3] = [Action::SellAll, Action::Trim, Action::Hold];

/// The JSON Schema for [`RoleRiskInterpretation`] — the reduced spine is structural.
pub fn role_risk_interpretation_schema() -> Value {
    let actions: Vec<&str> = ROLE_RISK_ACTIONS.iter().map(Action::as_kebab).collect();
    json!({
        "type": "object",
        "properties": {
            "action": { "type": "string", "enum": actions },
            "role_summary": { "type": "string" },
            "what_changed": { "type": "string" }
        },
        "required": ["action", "role_summary", "what_changed"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpretation_round_trips_through_its_schema_labels() {
        // The kebab labels the schema advertises are exactly what serde decodes, so a
        // grammar-valid model object deserializes into `Interpretation` cleanly.
        let raw = json!({
            "action": "add",
            "conviction": "high",
            "horizon_outlook": { "short": "neutral", "mid": "bullish", "long": "bullish" },
            "financial_summary": "Durable margins, light leverage.",
            "price_target_rationale": "Base case tracks the engine's DCF midpoint.",
            "what_changed": "new holding"
        });
        let parsed: Interpretation = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.action, Action::Add);
        assert_eq!(parsed.conviction, Conviction::High);
        assert_eq!(parsed.horizon_outlook.mid, HorizonRead::Bullish);
    }

    #[test]
    fn interpretation_schema_lists_every_required_field() {
        let all = [
            Action::SellAll,
            Action::Trim,
            Action::Hold,
            Action::Add,
            Action::AddAggressively,
        ];
        let schema = interpretation_schema(&all);
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for field in [
            "action",
            "conviction",
            "horizon_outlook",
            "financial_summary",
            "price_target_rationale",
            "what_changed",
        ] {
            assert!(required.contains(&field), "schema must require {field}");
        }
        // The action enum advertises exactly the feasible set, so the model can't
        // pick a rung the engine barred.
        let actions = schema["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(actions.len(), 5);
        let bounded = interpretation_schema(&[Action::SellAll, Action::Trim, Action::Hold]);
        let bounded_actions = bounded["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(bounded_actions.len(), 3);
        assert!(!bounded_actions.iter().any(|a| a == "add"));
    }

    #[test]
    fn role_risk_schema_offers_only_the_reduced_spine() {
        let schema = role_risk_interpretation_schema();
        let actions: Vec<&str> = schema["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(actions, vec!["sell-all", "trim", "hold"]);
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
    fn legacy_graded_rows_decode_as_the_priced_branch() {
        // A run persisted by the single-equity slice carries `status: "graded"` and
        // the old target field names; both must decode into the new union so old
        // runs stay legible (`docs/portfolio-analysis.md` §Intrinsic verdict).
        let legacy = json!({
            "status": "graded",
            "grade": "B",
            "sub_scores": { "quality": 70.0, "valuation": 60.0, "momentum": 55.0, "risk": 65.0 },
            "action": "hold",
            "action_sizing": {
                "target_weight_low": 0.05, "target_weight_high": 0.07,
                "est_share_delta": null, "est_dollar_delta": null
            },
            "conviction": "medium",
            "horizon_outlook": { "short": "neutral", "mid": "bullish", "long": "bullish" },
            "price_targets": {
                "end_of_month": null,
                "end_of_year": {
                    "base": 210.0, "bear": 180.0, "bull": 240.0,
                    "methodology": "v1 drift"
                }
            },
            "price_target_rationale": "midpoint",
            "options_signal": {
                "put_call_volume": null, "put_call_open_interest": null,
                "implied_volatility": null, "iv_skew": null
            },
            "financial_summary": "fine",
            "what_changed": "new holding"
        });
        let parsed: VerdictDisposition = serde_json::from_value(legacy).unwrap();
        match parsed {
            VerdictDisposition::Priced(g) => {
                assert_eq!(g.grade, Grade::B);
                // Old field names decode through the aliases into the rolling windows.
                assert!(g.price_targets.one_month.is_none());
                assert_eq!(g.price_targets.twelve_month.unwrap().base, 210.0);
                // Pre-field runs default the new engine reads rather than fabricating.
                assert!(g.risk_tier.is_none());
                assert!(g.dead_money.is_none());
                assert!(!g.low_confidence_grade);
            }
            other => panic!("legacy graded row must decode as priced, got {other:?}"),
        }
    }

    #[test]
    fn role_risk_only_serializes_its_own_branch() {
        let v = VerdictDisposition::RoleRiskOnly(Box::new(RoleRiskVerdict {
            class_label: "bond fund".into(),
            role_summary: "Core fixed-income sleeve.".into(),
            exposure_tilt: vec![ExposureWeight { label: "United States".into(), weight: 0.97 }],
            expense_drag: Some(0.0003),
            observable_risk: Some(0.06),
            structural_flag: false,
            evidence_gaps: vec!["valuation: no on-plan duration/credit surface".into()],
            action: Action::Hold,
            action_sizing: ActionSizing {
                target_weight_low: 0.09,
                target_weight_high: 0.11,
                est_share_delta: None,
                est_dollar_delta: None,
            },
            what_changed: "new holding".into(),
        }));
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s["status"], "role-risk-only");
        assert_eq!(s["class_label"], "bond fund");
        // The branch carries no grade / targets / conviction keys at all.
        assert!(s.get("grade").is_none());
        assert!(s.get("price_targets").is_none());
        assert!(s.get("conviction").is_none());
        let round: VerdictDisposition = serde_json::from_value(s).unwrap();
        assert_eq!(round, v);
    }
}
