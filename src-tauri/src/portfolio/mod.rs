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
//! the engine computes every **baseline-arm** number (sub-scores, the composite
//! grade, scenario price targets, the options-activity signal, the mechanical
//! stand-ins), and since `portfolio-v7` the model authors its **own arm** beside
//! it — its sub-scores, derived letter, and target bands, plus the conviction,
//! horizon reads, and prose — with model-arm judgment values never
//! altering or binding the engine baseline (the boundary statement:
//! `docs/portfolio-analysis.md` §The holding verdict). The engine grade stays a
//! deterministic roll-up of the
//! engine's sub-scores, never a model gestalt; the model's letter derives from
//! the model's own sub-scores through the same shared cutoffs.

pub mod diff;
pub mod distill;
pub mod dossier;
pub mod engine;
pub mod fund;
pub mod job;
pub mod listing;
pub mod outcome;
pub mod pipeline;
pub mod pre_profit;
pub mod quick_check;
pub mod research;
pub mod store;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The tracker step key for one holding's per-holding pass — the single home for
/// the `holding-{SYMBOL}` format, shared by the job's step rows ([`job`]) and the
/// interpretation stages' step-scoped reasoning stream ([`pipeline`]), so the
/// streamed thinking always lands on the step the run tracker is showing for that
/// holding.
pub fn holding_step_key(symbol: &str) -> String {
    format!("holding-{symbol}")
}

// ---- Durable plan-time parameters (pinned this slice) ------------------------
//
// These three are pinned because they shape retention, the house-view loader, and
// the verdict schema; the grade-weight formula, risk-tier thresholds, and
// options-signal parameters are deliberately left calibratable (in `engine`), to be
// shadow-tuned against live runs rather than frozen now.

/// How many Portfolio Analysis runs are retained (newest-N), pruned independently of
/// the 30-report report-retention window and of Trade Opportunities
/// (`docs/storage.md §Local Analysis Suite Storage`). N=30, matching report
/// retention (ruled 2026-08-11 — `docs/verification/2026-08-10-big-run-attempt-1.md`
/// §Disposition): degraded construction-failed runs count against this one cap
/// rather than a second retention path, and the number bounds the sidebar's
/// `list_run_summaries` blob parse as well as disk.
pub const PORTFOLIO_RUN_RETENTION: u32 = 30;

/// The Step-6a semantic continuity retrieval's depth — the top-k cosine hits a
/// holding's dossier recalls from the Portfolio memory partition's `summary`
/// rows (`docs/portfolio-workflow.md` §Step 6a). Drafted, calibratable
/// (`docs/portfolio-analysis.md` §Starting parameters).
pub const SEMANTIC_RECALL_TOP_K: usize = 3;

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

/// The configured investor profile that personalizes the *action* — never the
/// intrinsic verdict (`docs/portfolio-analysis.md` §Intrinsic verdict,
/// `docs/configuration.md` §Investor Profile). It reaches the model at the
/// **per-holding action call** only ([`ActionDecision`]): objective, risk
/// tolerance, horizon, and tax posture frame the rung there, and no other model
/// call renders it. It ships as the documented fixed preset
/// ([`InvestorProfile::default_fixture`]); the configurable Settings form is a
/// later slice — Settings shows the preset read-only via [`Self::display`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestorProfile {
    pub objective: ProfileObjective,
    pub risk_tolerance: RiskTolerance,
    pub horizon: ProfileHorizon,
    /// Whether holdings sit in a taxable account (so realizing a gain or loss
    /// carries a tax consequence the action rationale flags as a user
    /// consideration) versus tax-advantaged.
    pub tax_sensitive: bool,
    /// Cash / buying power available for new purchases, in account currency.
    /// **`None` means cash is unconstrained** — the fixed preset's stance (the
    /// user may hold cash the app can't see). Consumer-less since sizing
    /// retired with the construction stage (`portfolio-v9`): cash bounds are
    /// whole-book work, the future portfolio planner's
    /// (`docs/configuration.md` §Investor Profile).
    pub available_cash: Option<f64>,
}

impl InvestorProfile {
    /// The documented fixed preset (`docs/configuration.md` §Investor Profile):
    /// profit-maximization objective, medium-to-high risk tolerance (represented as
    /// the aggressive rung of the three-step scale), a long-term horizon,
    /// taxable/tax-aware (the qualitative loss-realization counterweight only — no
    /// tax-lot modeling), and **cash treated as unconstrained** (the user may hold
    /// cash the app can't see). The real per-user profile is configured in a later
    /// Settings slice.
    pub fn default_fixture() -> Self {
        Self {
            objective: ProfileObjective::MaximizeProfit,
            risk_tolerance: RiskTolerance::Aggressive,
            horizon: ProfileHorizon::LongTerm,
            tax_sensitive: true,
            // Unconstrained cash — adds are not gated on observed Schwab cash
            // (`docs/configuration.md` §Investor Profile).
            available_cash: None,
        }
    }

    /// The read-only Settings rows for this profile — ready-to-render display
    /// strings composed here so the Settings block and the action call's prompt
    /// share one label source (`docs/interface.md` Settings tree;
    /// `docs/configuration.md` §Investor Profile).
    pub fn display(&self) -> InvestorProfileDisplay {
        InvestorProfileDisplay {
            objective: self.objective.label().to_string(),
            risk_tolerance: self.risk_tolerance.label().to_string(),
            horizon: self.horizon.label().to_string(),
            tax: if self.tax_sensitive {
                "tax-aware — the possible benefit of realizing a loss is weighed \
                 qualitatively; no tax-lot, holding-period, or rate modeling"
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
/// Single-variant today — the fixed preset's stance; income / capital-preservation
/// mandates join when the configurable profile ships.
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
    /// The shared prompt/Settings label ([`InvestorProfile::display`]). The
    /// aggressive rung carries the documented preset's "medium-to-high" framing
    /// (`docs/configuration.md` §Investor Profile — the 2026-08-05 B7 ruling: the
    /// three-rung vocabulary is kept, the preset represented as the aggressive
    /// rung and rendered with the medium-to-high posture).
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
    /// The neutral state (no add/trim detected).
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

    /// Whether the position's net side reversed versus the prior snapshot (a
    /// long↔short flip) — thesis-changing by construction, so no long-side verdict
    /// is valid across it. This per-run predicate's production caller is **outcome
    /// alignment** ([`outcome`]); the carried-verdict side-reversal *badge* is
    /// computed separately in [`job`] from the current side against a directional
    /// verdict's invariant long authoring side, robust across a flip through an
    /// exactly-zero net this predicate cannot see (`docs/portfolio-analysis.md`
    /// §Asset eligibility, §Triggering). `false` with no prior counterpart (nothing
    /// to reverse from) and on a flat side (a zero quantity has no side).
    pub fn side_reversed(&self, current_quantity: f64) -> bool {
        match self.prior_quantity {
            Some(prior) => {
                prior != 0.0
                    && current_quantity != 0.0
                    && prior.is_sign_positive() != current_quantity.is_sign_positive()
            }
            None => false,
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
/// language. Since `portfolio-v7` the model selects the rung freely (the full
/// ladder); the engine's set rides as evidence, an outside-the-set rung persisting
/// with an audit annotation. The rung is the whole decision (`portfolio-v9`) —
/// no weight range or share/dollar figure rides beside it.
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

    /// Whether the rung sits on the add side of the ladder — the family the
    /// over-age rule demotes on a carried verdict (`docs/portfolio-analysis.md`
    /// §Triggering).
    pub fn is_add_family(&self) -> bool {
        matches!(self, Action::Add | Action::AddAggressively)
    }

    /// Whether the rung sits on the exit side of the ladder — the family an
    /// over-age carry keeps as-is behind the stale badge (only the add family
    /// rule-demotes; since the 2026-08-16 ruling an over-age exit no longer
    /// force-includes — `docs/portfolio-analysis.md` §Triggering).
    pub fn is_exit_family(&self) -> bool {
        matches!(self, Action::SellAll | Action::Trim)
    }
}

/// How a verdict's action came to be — the canonical two-value vocabulary from
/// `docs/portfolio-analysis.md` §Outcome learning: **`model-chosen`** (a model
/// pass actually chose it — every fresh verdict) or **`rule-demoted`** (an over-age
/// carried add-family action rule-demoted to *hold* at the roll-up — a labeled
/// rule-based weaken that stays out of the pooled outcome cohorts, so the hold
/// cohort measures only holds a model actually chose; §Triggering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionSource {
    #[default]
    ModelChosen,
    RuleDemoted,
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
/// case; this engine-arm number is never model-authored (the model arm's own
/// bands ride [`ModelPriceTargets`]).
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
/// date**, not calendar ends (outside January, calendar year-end is not twelve
/// months away, and calibration scores these against the 1- and 12-month labels —
/// `docs/portfolio-analysis.md` §Starting parameters). Each `None` when the inputs to
/// derive it were missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceTargets {
    pub one_month: Option<PriceTarget>,
    pub twelve_month: Option<PriceTarget>,
}

/// One model-authored scenario target band — the model arm's counterpart of the
/// engine's [`PriceTarget`] (`docs/portfolio-analysis.md` §The holding verdict, the
/// two-arm contract). Authored freely at interpretation: no engine bound, band, or
/// clamp applies, and the app persists it exactly as returned — within the
/// declared domain: each leg finite and strictly positive, gated at decode by
/// [`validate_model_arm`] (Codex I6), never clamped. Ordering is not gated:
/// scoring reads the band as (min, max), so an inverted bear/bull pair still
/// scores; the render annotates disorder rather than reordering the authored
/// numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPriceTarget {
    pub base: f64,
    pub bear: f64,
    pub bull: f64,
}

/// The model arm's one-month and twelve-month targets — the same rolling windows
/// as the engine's [`PriceTargets`]. Both are always authored: the model can always
/// commit to a view, so the engine's missing-input `None` legs have no analog here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPriceTargets {
    pub one_month: ModelPriceTarget,
    pub twelve_month: ModelPriceTarget,
}

/// The model arm of the two-arm verdict (`docs/portfolio-analysis.md` §The holding
/// verdict): the model's own read of fields the engine also computes, authored with
/// the engine's values in the prompt as evidence and **never validated against
/// them**. It never alters or binds the engine baseline — the quick check,
/// hurdle tests, monitor stamps, and outcome labels read engine values only,
/// while machinery acting on the model's *choices* (the per-holding action
/// call, the episode lifecycle, the letter derivation, the scoreboard's scoring)
/// is intentional (the boundary statement: `docs/portfolio-analysis.md` §The
/// holding verdict); these values carry into the next run's retrospective.
/// Beside this struct, the model-authored `conviction` and `horizon_outlook` on
/// [`GradedVerdict`] complete the arm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelView {
    /// The model's own four sub-scores on the engine's 0–100 scale (higher = better),
    /// gated to that scale at decode ([`validate_model_arm`]) so the letter below
    /// always derives from an on-scale composite.
    pub sub_scores: SubScores,
    /// Derived app-side from the model's quality / valuation / risk through the
    /// shared composite weights and cutoffs ([`engine::grade_from_subscores`]) — the
    /// model controls its letter through its scores, and letters stay comparable
    /// across arms. Momentum stays outside the letter in both arms.
    pub letter: Grade,
    pub price_targets: ModelPriceTargets,
    /// The model's retrospective self-assessment — was the prior read right, was it
    /// better than the engine baseline, and why (prose). The scored comparison is
    /// the deterministic scoreboard's job, never this field's.
    pub self_assessment: String,
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

/// The priced body of a holding verdict — present only when the holding was eligible,
/// priceable, *and* cleared the evidence floor. The engine arm's numbers (grade,
/// sub-scores, targets, tier, hurdle, options signal) come from the engine; the
/// action with its rationale, conviction, horizon reads, prose, and the model arm
/// ([`ModelView`]) come from the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradedVerdict {
    pub grade: Grade,
    pub sub_scores: SubScores,
    /// The **per-holding portfolio action**, authored by the dedicated action
    /// call from this holding's own evidence plus the investor profile — rung
    /// only, no sizing; the whole-book reconciliation is the future portfolio
    /// planner's job (`docs/portfolio-analysis.md` §Portfolio action).
    pub action: Action,
    /// The action call's one-line rationale for the chosen rung.
    pub action_rationale: String,
    pub conviction: Conviction,
    pub horizon_outlook: HorizonOutlook,
    pub price_targets: PriceTargets,
    /// The model's justification for the engine's base-case target (it selects and
    /// explains; the engine computed the figure). Persisted so a verdict's
    /// target basis stays inspectable.
    pub price_target_rationale: String,
    pub options_signal: OptionsSignal,
    /// The deterministic per-branch risk tier (`docs/portfolio-analysis.md` §Starting
    /// parameters).
    pub risk_tier: RiskTier,
    /// The three-state capital-efficiency / dead-money read — only `fails` is dead
    /// money.
    pub dead_money: HurdleState,
    /// True when the letter rests on an imputed (neutral-50) sub-score — the visible
    /// low-confidence marker beside the letter (`docs/portfolio-analysis.md` §Asset
    /// eligibility, the priced-fund grade contract; also any stock graded over an
    /// imputed axis).
    pub low_confidence_grade: bool,
    /// The fund path's deterministic strategy classification label (`None` for a
    /// stock) — "the classification is deterministic, shown on the card"
    /// (`docs/portfolio-analysis.md` §Asset eligibility), the priced branch included.
    pub fund_class_label: Option<String>,
    /// The deterministic structural path-dependency flag on the priced branch (an
    /// option-overlay fund; leveraged / inverse routes to `role_risk_only` instead) —
    /// card-visible beside the classification.
    pub structural_flag: bool,
    /// A concise read of the company's financial health (model prose).
    pub financial_summary: String,
    /// The continuity diff against the prior run (model prose, or "new holding") —
    /// the intrinsic what-changed audit, authored at 6f. The retired action half
    /// (a 7b construction artifact) went with that stage; this is prose only.
    pub what_changed: String,
    /// The model arm of the two-arm verdict ([`ModelView`]) — the model's own
    /// sub-scores, derived letter, targets, and retrospective self-assessment.
    /// Required on every persisted verdict: a fresh v9-only store never holds a
    /// pre-`portfolio-v7` single-arm row, so a blob missing an arm fails decode
    /// and loud-skips as unreadable rather than rendering a partial verdict (the
    /// frontend types both arms as present — `src/types.ts`).
    pub model_view: ModelView,
    /// The engine's mechanical stand-in arm ([`EngineView`]) — deterministic
    /// outlook / conviction / action baselines beside the model's, so every
    /// model-authored field has a scored engine counterpart. Required on every
    /// persisted verdict, same contract as [`model_view`](Self::model_view).
    pub engine_view: EngineView,
}

/// The engine's mechanical stand-in arm of the two-arm verdict
/// (`docs/portfolio-analysis.md` §The holding verdict): deterministic, disclosed
/// formulas producing baseline counterparts for the three fields only the model
/// used to author — outlook, conviction, and the action rung. Computed by
/// [`engine::engine_view`] from data already on the dossier; no model input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineView {
    /// Trailing-return outlook over ~21 / 126 / 252 sessions with per-window flat
    /// thresholds — a mechanical short / mid / long read.
    pub outlook: HorizonOutlook,
    /// A disclosed degradation count mapped to High / Medium / Low — data
    /// completeness as confidence, never judgment.
    pub conviction: Conviction,
    /// The formalized rung rule over the existing feasible-set machinery
    /// (grade × hurdle × admission, tiebreak toward hold).
    pub action: Action,
}

/// One exposure weight (a sector or country label and its fraction of the fund).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExposureWeight {
    pub label: String,
    pub weight: f64,
}

/// The `role_risk_only` branch of an analyzed verdict (`docs/portfolio-analysis.md`
/// §Intrinsic verdict): a structurally unpriceable vehicle class gets a typed role /
/// risk read — **no letter, no price targets, no conviction, no tier** — its action
/// authored by the dedicated per-holding action call from the branch's own
/// attributes, the full ladder structurally open while the engine arm's set stays
/// the reduced [`ROLE_RISK_ACTIONS`], rendered as evidence with departures
/// annotated on the audit.
/// Engine-computed fields (exposure, expense, risk, gaps) plus the model's role read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleRiskVerdict {
    /// The deterministic classification label (e.g. "bond fund", "leveraged / inverse
    /// vehicle", "equity fund below the US-exposure guard").
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
    /// The closed-end structure marker (the CEF leg, ruled 2026-08-21) — detection is
    /// the profile's `isFund` flag plus the closed-end description fragment.
    pub is_cef: bool,
    /// Price vs NAV (market price ÷ NAV − 1; positive = premium), rendered only on
    /// the closed-end form — `None` is the named gap (no NAV on the current data
    /// surface), carried in `evidence_gaps` rather than fabricated.
    pub nav_premium: Option<f64>,
    /// The typed evidence gaps — this branch's confidence surface (never a fabricated
    /// High / Medium / Low conviction).
    pub evidence_gaps: Vec<String>,
    /// The per-holding action, authored by the dedicated action call from the
    /// branch's own attributes plus the investor profile — rung only, the full
    /// ladder open (`docs/portfolio-analysis.md` §Portfolio action).
    pub action: Action,
    /// The action call's one-line rationale.
    pub action_rationale: String,
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
    // every disposition would be sized to it.
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

// ---- Position thesis ledger ----------------------------------------------------
//
// The persisted per-holding standing thesis (`docs/portfolio-analysis.md` §The
// position thesis ledger): why the job holds a view on the position, carried
// forward run to run. The model authors the thesis / monitor / condition
// *statements* at interpretation; the app owns everything structural — condition
// ids, the machine-evaluable cores' validation, evaluation state, the engine
// scenario targets stamped into the monitor, and what carries across a rewrite.

/// Which verdict branch a ledger is typed by (`docs/portfolio-analysis.md` §The
/// position thesis ledger): a `priced` ledger carries the full shape; a
/// `role_risk_only` ledger keeps the same sections with two reductions — its
/// monitor scenarios are condition-only (no engine scenario target exists on that
/// branch) and its triggers are trim / sell only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LedgerBranch {
    Priced,
    RoleRiskOnly,
}

/// A condition's series cadence (`docs/portfolio-analysis.md` §The position thesis
/// ledger): market-data conditions are evaluable on every pass; filing-cadence
/// conditions advance only when a fresh observation of their series lands.
/// Derived from the series, never authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConditionCadence {
    MarketData,
    Filing,
}

/// The comparator of a machine-evaluable condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LedgerComparator {
    Below,
    Above,
}

impl LedgerComparator {
    pub fn as_kebab(&self) -> &'static str {
        match self {
            LedgerComparator::Below => "below",
            LedgerComparator::Above => "above",
        }
    }
}

/// Whether a ledger condition is a key falsifier or a pre-committed action trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConditionRole {
    Falsifier,
    Trigger,
}

/// The action family a trigger pre-commits (`docs/portfolio-analysis.md` §The
/// position thesis ledger — add / trim / sell triggers; a `role_risk_only` ledger's
/// triggers are trim / sell only, since its feasible set never offers the add family).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TriggerFamily {
    Add,
    Trim,
    Sell,
}

/// The machine-evaluable core of a **quantitative** condition — the structural
/// identity the app matches across rewrites (`docs/trade-opportunities.md §The
/// opportunity` — the suite's shared condition-identity contract, applied at
/// Portfolio's seams): a rewrite that leaves this core unchanged carries the
/// condition's id and evaluation state through any re-wording; an edit to it
/// supersedes. The cadence and required consecutive-observation count are derived
/// from the series ([`engine::LedgerSeries`]), so they are not part of the authored
/// core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuantCore {
    pub series: engine::LedgerSeries,
    pub comparator: LedgerComparator,
    pub threshold: f64,
    /// Materiality margin (same units as the series, absolute): a breach counts only
    /// beyond `threshold ± margin` — the noise guard. Clamped non-negative at
    /// validation.
    pub margin: f64,
}

/// Which statement window a holding's fundamentals were computed on
/// (`docs/portfolio-analysis.md` §Starting parameters — the TTM statement basis and
/// its annual fallback).
///
/// It is persisted on each condition's evaluation state because a change of basis
/// moves every statement-derived level **without the business changing**: a
/// one-quarter feed gap fails the contiguity guard, drops the holding to the SEC
/// annual basis, and a growing issuer's P/S steps (measured ~8.0 → 10.3) purely
/// because the denominator switched from four trailing quarters to a prior fiscal
/// year. Compared across that step, a model-authored threshold reads as breached by
/// evidence that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StatementBasis {
    /// Four contiguous trailing quarters.
    Ttm,
    /// The SEC same-concept annual fallback — adopted when the quarterly window is
    /// gapped, non-contiguous, or short, and stamped only where SEC filled a flow
    /// line (`dossier::merge_financials`); an equity-only fill is a balance-sheet
    /// instant outside the flow-basis rule and stamps nothing.
    Annual,
}

impl StatementBasis {
    /// The prompt's name for the basis — one vocabulary for the ledger section's
    /// basis line and the evaluation's basis-change note, so the model reads the
    /// same words wherever the basis is stated.
    pub fn label(&self) -> &'static str {
        match self {
            StatementBasis::Ttm => "TTM (four trailing quarters)",
            StatementBasis::Annual => {
                "SEC annual (latest full year — the quarterly window fell back)"
            }
        }
    }
}

/// Which balance sheet supplied a holding's stockholders' equity — the denominator
/// of debt/equity and price/book, the two balance-sheet instants outside the
/// flow-basis rule (`docs/portfolio-analysis.md` §Starting parameters, the
/// leverage leg): FMP's latest quarterly balance sheet first, SEC's annual
/// `stockholders_equity` the fallback, stamped at `dossier::merge_financials`.
///
/// It is persisted on the two instants' condition evaluation state beside the
/// statement basis because the FMP balance-sheet leg is fail-soft: a gap on one
/// run and a return on the next flips the equity leg between a quarter-end
/// instant and a year-end one under an unchanged flow basis, and both series
/// step with nothing having happened — the flow-basis step's size class, on a
/// stamp that never covered it (the 2026-08-24 review's Codex I13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EquitySource {
    /// FMP's latest quarterly balance sheet — the preferred leg.
    FmpQuarterly,
    /// SEC's latest annual `stockholders_equity` — filled where the FMP leg
    /// returned nothing.
    SecAnnual,
}

impl EquitySource {
    /// The prompt's name for the source — one vocabulary for the ledger section's
    /// basis line and the evaluation's source-change note.
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

/// A quantitative condition's **evaluation state** — engine state, distinct from the
/// model-authored ledger content (`docs/storage.md §Local Analysis Suite Storage`),
/// observation-identity-keyed so the breach streak advances only on a distinct new
/// print or filing, never on re-evaluating the same one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConditionEvalState {
    /// The last distinct observation evaluated (a trading-day date for a market-data
    /// series; a statement period end for a filing-cadence one).
    pub last_observation_id: Option<String>,
    pub last_value: Option<f64>,
    /// The run date of the last evaluation.
    pub last_evaluated_at: Option<String>,
    /// Consecutive distinct breaching observations (reset by a clean observation).
    pub breach_streak: u32,
    pub first_breach_at: Option<String>,
    /// Set when the streak reached the series' required count — a confirmed breach.
    pub confirmed_at: Option<String>,
    /// The acknowledgment transition (`docs/portfolio-analysis.md` §The position
    /// thesis ledger): once a full pass consumes a confirmed breach it stamps the
    /// confirming observation here, so the breach re-raises only when confirmed
    /// against a *later* observation, never straight back from the one already
    /// examined.
    pub acknowledged_observation_id: Option<String>,
    /// The statement basis this condition's streak was accumulated under, stamped by
    /// the engine on first evaluation ([`StatementBasis`]).
    ///
    /// A statement-derived series compared across a basis change is comparing two
    /// different measurements, so the engine types it **unevaluable** for that pass
    /// and re-stamps — the streak cannot carry across, because the observations in it
    /// were taken on the other basis. Stamped at authoring from the surface the
    /// prompt described ([`ContinuityStamps`], Step 6g); a condition authored
    /// where that surface carried no basis stays `None` until its first
    /// evaluation, which adopts the current basis without a discontinuity (there
    /// is nothing to disagree with).
    pub authored_statement_basis: Option<StatementBasis>,
    /// The equity source a balance-sheet instant's streak — debt/equity or
    /// price/book — was accumulated under, stamped at authoring
    /// ([`ContinuityStamps`]) or else by the engine on first evaluation beside
    /// the basis ([`EquitySource`]); `None` on every other series, which never
    /// read it. A source change is the instants' own
    /// discontinuity, under the same one-pass-unevaluable-and-re-stamp treatment
    /// as a basis change, and a pass on which both change is one pass, both
    /// adopted together. No serde default (the fresh-start-2 rule).
    pub authored_equity_source: Option<EquitySource>,
}

/// The continuity stamps of one authoring surface — the statement basis and the
/// equity source the interpretation prompt described to the model when it
/// authored a quantitative condition ([`ConditionEvalState`]'s two `authored_*`
/// fields). Step 6g stamps every new or superseding quantitative condition from
/// them, per series, so the first full-pass evaluation after a debut already has
/// a stamp to disagree with; a condition authored where the surface carried
/// none adopts at its first evaluation. Without this the run-1 ledger's instants
/// carried no stamp until run 2's evaluation, which adopted silently across the
/// very flip the equity gate exists for (Codex round 1 on the 2026-08-24
/// review's group 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContinuityStamps {
    pub statement_basis: Option<StatementBasis>,
    pub equity_source: Option<EquitySource>,
}

impl ContinuityStamps {
    /// No stamps — a surface with neither statement lines nor an equity leg (the
    /// research-less validator wrapper the tests use).
    pub const NONE: Self = Self {
        statement_basis: None,
        equity_source: None,
    };

    /// The stamps of the financials the prompt rendered.
    pub fn of(fin: &engine::CompanyFinancials) -> Self {
        Self {
            statement_basis: fin.statement_basis,
            equity_source: fin.equity_source,
        }
    }

    /// A newly authored quantitative condition's starting state on `series`: the
    /// basis stamp on every statement-derived series, the equity stamp on the
    /// two balance-sheet instants alone — the same per-series rule the
    /// evaluation's gate reads (`engine::evaluate_ledger_conditions_gated`).
    pub fn authored_state(self, series: engine::LedgerSeries) -> ConditionEvalState {
        let statement = series.statement_derived();
        let instant = statement && !series.flow_basis();
        ConditionEvalState {
            authored_statement_basis: self.statement_basis.filter(|_| statement),
            authored_equity_source: self.equity_source.filter(|_| instant),
            ..Default::default()
        }
    }
}

/// One ledger condition — a key falsifier or an action trigger, **quantitative**
/// (carrying a validated machine-evaluable core plus evaluation state) or
/// **qualitative** (research / model-checkable prose; no machine state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerCondition {
    /// Stable app-assigned id (`docs/portfolio-analysis.md` §The position thesis
    /// ledger) — carried across rewrites when the machine core is unchanged; a
    /// changed core supersedes into a fresh id.
    pub condition_id: String,
    pub role: ConditionRole,
    /// The trigger's action family (`None` on a falsifier).
    pub trigger_family: Option<TriggerFamily>,
    /// The model's statement of the condition (prose).
    pub statement: String,
    /// The validated machine core — present only on a quantitative condition.
    pub quant: Option<QuantCore>,
    /// Logged when a claimed-quantitative condition failed executability validation
    /// and was downgraded to qualitative (never dropped —
    /// `docs/portfolio-workflow.md` §Step 6g).
    pub downgraded_reason: Option<String>,
    /// A third-party technology-event falsifier (`docs/portfolio-analysis.md` §The
    /// position thesis ledger — the first-class qualitative falsifier class).
    pub technology_class: bool,
    /// The validated tripped (falsifier) / fired (trigger) claim — set only when the
    /// claim mapped to the engine's deterministic crossing; an unmapped claim is
    /// cleared and logged, so the ledger can't be quietly rewritten to fit a verdict.
    pub tripped: bool,
    /// The id of the condition this one superseded (a rewrite that changed the
    /// machine core — fresh streak, the old condition closed into the audit).
    pub supersedes: Option<String>,
    /// Engine evaluation state (quantitative conditions only; app-owned).
    pub eval_state: Option<ConditionEvalState>,
}

/// The three monitor scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScenarioKind {
    Bear,
    Base,
    Bull,
}

/// One bear / base / bull monitor scenario (`docs/portfolio-analysis.md` §The
/// position thesis ledger): its defining conditions, a rough probability lean, and —
/// on the `priced` branch — the **engine's** scenario price target, stamped by the
/// app from the engine's own scenario set (never a model-written number). A
/// `role_risk_only` scenario is condition-only (`engine_target` stays `None`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorScenario {
    pub scenario: ScenarioKind,
    /// The conditions that define this scenario (prose).
    pub conditions: String,
    /// Rough probability lean, percent (0–100).
    pub probability_pct: f64,
    /// The engine's twelve-month scenario price target for this scenario — app-stamped.
    pub engine_target: Option<f64>,
}

/// Spot's relationship to the monitor's bear–bull band. Stamped onto the ledger at
/// authoring (beside the engine targets) so the quick check's `PriceOutsideBand`
/// flag fires on a *change* in the relationship, never on the standing state — a
/// band authored with spot already outside was an examined observation (the model
/// wrote the ledger seeing it), not news worth re-raising every sweep.
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

/// One key driver — a variable the thesis actually depends on, tied where possible to
/// an engine-tracked series so the next run can read whether it moved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyDriver {
    /// The app-assigned stable identity (ruled 2026-08-24): assigned at ledger
    /// validation, preserved across rewrites while the driver's name carries,
    /// and the referential anchor a validated leading indicator must cite
    /// before its presence may suppress the narrative cap.
    pub driver_id: String,
    pub name: String,
    /// The engine series backing the driver, where one exists.
    pub series: Option<engine::LedgerSeries>,
}

/// The persisted per-holding **thesis ledger** (`docs/portfolio-analysis.md` §The
/// position thesis ledger): the standing thesis with its goalposts, carried forward
/// run to run, re-evaluated by the engine, rewritten by interpretation, and
/// validated by the continuity check. Persisted on the holding's verdict; an
/// insufficient-evidence exit retains the prior ledger unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThesisLedger {
    pub branch: LedgerBranch,
    /// The thesis when the position first entered an analysis — frozen at debut and
    /// carried immutable, so drift stays legible.
    pub original_thesis: String,
    pub current_thesis: String,
    pub key_drivers: Vec<KeyDriver>,
    /// The bear / base / bull monitor.
    pub monitor: Vec<MonitorScenario>,
    /// What must improve to migrate toward the bull case.
    pub what_must_improve: String,
    /// What must not break to stay in the base case.
    pub what_must_not_break: String,
    /// Key falsifiers and action triggers.
    pub conditions: Vec<LedgerCondition>,
    /// Spot's relationship to the monitor band at authoring — app-stamped beside
    /// the engine targets; `None` wherever no band exists (`role_risk_only`,
    /// missing spot).
    pub authored_band_relation: Option<BandRelation>,
}

/// One engine-detected condition crossing (`docs/portfolio-analysis.md` §The
/// position thesis ledger — the engine tests which quantitative falsifiers and
/// triggers crossed this run, under their persistence semantics), fed to
/// interpretation and recorded on the audit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionCrossing {
    pub condition_id: String,
    pub statement: String,
    pub role: ConditionRole,
    pub outcome: CrossingOutcome,
    pub observed_value: f64,
    pub threshold: f64,
    /// The distinct observation the evaluation keyed on.
    pub observation_id: String,
    /// The date the crossing **confirmed** on — the run/sweep whose print pushed the
    /// streak to its required count, carried from the condition's own
    /// [`ConditionEvalState::confirmed_at`]. `None` only on a `FirstBreach` (nothing
    /// has confirmed yet); a confirmed crossing is always stamped, so the consumer
    /// reads the date directly rather than dating it at the consuming run.
    ///
    /// It exists because the confirming pass and the pass that *consumes* the
    /// crossing are not the same event: a between-run sweep can confirm days before
    /// the next full run reads it. Anything positioning the confirmation in time —
    /// the falsifier lead-time read above all — must date it here, not at the
    /// consuming run.
    pub confirmed_at: Option<String>,
}

/// A crossing's persistence-semantics outcome: a lone noisy print is a quiet
/// first-breach note; only a confirmed breach (the series' required consecutive
/// distinct observations) trips a falsifier or fires a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrossingOutcome {
    FirstBreach,
    Confirmed,
}

/// A prior condition the rewrite closed — superseded by an edited core, or removed
/// outright — preserved **whole**: statement, machine core, and its accumulated
/// evaluation state as of this run's evaluation. The shared contract requires the
/// old condition to close *with its state* into the audit record
/// (`docs/trade-opportunities.md §The opportunity`), so the record stays
/// reconstructible after older runs prune.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosedCondition {
    /// The successor's condition id on a supersession; `None` on a removal.
    pub superseded_by: Option<String>,
    pub condition: LedgerCondition,
}

/// The ledger legs of a holding's continuity audit (`docs/portfolio-workflow.md`
/// §Step 6g): what the engine detected, what validation downgraded / superseded /
/// closed / rejected — recorded so a ledger rewrite is traceable, never silent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LedgerAudit {
    /// The crossings this pass consumed as continuity input.
    pub crossings: Vec<ConditionCrossing>,
    /// Conditions whose series could not be resolved this run (typed, not silent).
    pub unevaluable: Vec<String>,
    /// Claimed-quantitative conditions downgraded to qualitative (logged, never dropped).
    pub downgraded: Vec<String>,
    /// Rewrites that changed a machine core — the old condition closed whole (state
    /// included), the successor starting a fresh streak with a `supersedes` link.
    pub superseded: Vec<ClosedCondition>,
    /// Prior conditions the rewrite removed — closed whole into this record.
    pub closed: Vec<ClosedCondition>,
    /// Tripped / fired claims that mapped to no engine crossing (or no source-backed
    /// finding) and were cleared.
    pub rejected_claims: Vec<String>,
    /// Draft conditions dropped as duplicates of one already validated this pass
    /// (identical role + machine core, or an identical qualitative statement) — a
    /// repetitive model can't pad the ledger with copies.
    pub duplicates: Vec<String>,
}

/// One holding's complete verdict record, persisted per run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingVerdict {
    pub symbol: String,
    pub asset_class: AssetClass,
    /// How the position changed since the prior run — set by the app from the
    /// deterministic holdings diff ([`diff`]; `docs/portfolio-analysis.md` §What
    /// changed: the what-changed line carries the position delta), never authored by
    /// the model.
    pub position_change: PositionChange,
    pub disposition: VerdictDisposition,
    /// The holding's thesis ledger (`docs/portfolio-analysis.md` §The position
    /// thesis ledger) — present on an analyzed (priced / role-risk-only) verdict, and
    /// carried unchanged on an insufficient-evidence exit; `None` on a not-rated
    /// position.
    pub thesis_ledger: Option<ThesisLedger>,
    /// The holding's **analysis vintage** — the UTC RFC3339 timestamp of the full
    /// pass that produced this verdict (`docs/portfolio-analysis.md` §Triggering:
    /// carried verdicts ride vintage-stamped). The job stamps it at persist — a
    /// fresh verdict with the run's own `created_at`, a carried verdict with the
    /// vintage it carries, an insufficient-evidence exit with its prior's vintage
    /// — so `None` persists only on a debut abstention, which has no prior to
    /// inherit from; [`effective_vintage`] reads the run's `created_at` there,
    /// which is that verdict's own run.
    pub analyzed_at: Option<String>,
    /// How the action came to be ([`ActionSource`]) — `model-chosen` unless the
    /// over-age rule demoted a carried add-family action.
    pub action_source: ActionSource,
    /// Set on a **carried** verdict whose position's net side reversed since the
    /// verdict was written (`docs/portfolio-analysis.md` §Triggering) — the carried
    /// thesis describes the opposite position. Surfaced as a non-blocking card badge
    /// so the stale, wrong-direction advice is visible rather than silently trusted;
    /// a selective run no longer force-includes on a reversal (selective = strictly
    /// the user's selection, ruled 2026-08-16). A fresh pass leaves this `false`.
    pub side_reversed: bool,
}

/// A verdict's effective analysis vintage: its own `analyzed_at` stamp, else the
/// `created_at` of the run it rides in — the fallback for a debut abstention
/// (no prior vintage to inherit) and for a verdict the job has not stamped yet,
/// both by construction their own run's (a carried verdict is always stamped at
/// carry time, so the fallback never mis-dates one).
pub fn effective_vintage<'a>(verdict: &'a HoldingVerdict, run_created_at: &'a str) -> &'a str {
    verdict.analyzed_at.as_deref().unwrap_or(run_created_at)
}

// ---- Run-level aggregate (persisted per run) ---------------------------------

/// The run-level **data-health** aggregate (`docs/portfolio-analysis.md` §Portfolio
/// roll-up): the per-holding fail-soft posture is honest but silent at run level — a
/// degraded run that looks clean produces confidently wrong prescriptions (the
/// 2026-07-31 first live run: 43 of 44 anchor windows empty, invisible outside the
/// audits). Computed deterministically from the audits' typed `target_meta` plus the
/// run-scoped deep-history counter, persisted with the roll-up, and rendered as one
/// line on the Portfolio page's roll-up card.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataHealth {
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
    /// Holdings whose deep-history (FMP dated-EOD) fetch degraded — each one's
    /// anchor window starved to its documented fallback.
    pub deep_history_failures: usize,
    /// The run-level DGS10 anchor-history request failed (every spread observation
    /// inadmissible run-wide).
    pub dgs10_history_gap: bool,
    /// The house view was omitted for staleness — the latest report is older than the
    /// pinned freshness window (`docs/portfolio-workflow.md` §Step 5), so it was
    /// recorded as a gap rather than fed as current.
    pub house_view_omitted: bool,
    /// Local chat calls whose prompt filled at least [`CONTEXT_PRESSURE_FRACTION`] of
    /// their declared `num_ctx` — the digest-compression covenant's detection leg
    /// (`docs/portfolio-analysis.md` §Portfolio roll-up): `num_ctx` overflow silently
    /// front-truncates, so a near-full prompt is surfaced here rather than discovered
    /// as a corrupted read.
    pub context_pressure: Vec<crate::local_model::PromptUsage>,
    /// The run's fullest local prompt (by fraction of its `num_ctx`), recorded
    /// regardless of pressure — the measurement the big-run prompt-fit watch reads.
    /// `None` when no call reported a count.
    pub peak_prompt: Option<crate::local_model::PromptUsage>,
    /// Run-level commodity-context series gaps (FRED energy / IMF metals / FMP gold —
    /// `docs/portfolio-workflow.md` §Step 5). Counted, never attention: the feed is
    /// enriching and fail-soft.
    pub commodity_gaps: usize,
    /// Run-level CFTC positioning contract gaps — same enriching-feed posture.
    pub positioning_gaps: usize,
    /// The CBOE put/call backdrop was unavailable this run — same posture.
    pub cboe_gap: bool,
    /// The FINRA consolidated short-interest file was unavailable this run — same
    /// posture.
    pub finra_gap: bool,
    /// Distinct sector-benchmark series a completed holding read as unavailable (each
    /// starves the technology-event pre-flag for its holdings) — same counted-only
    /// posture; rebuilt from the holdings' rows, so a resumed run counts a benchmark
    /// once (Codex I17).
    pub benchmark_gaps: usize,
    /// Model calls the bounded retry-once recovered — each fired retry's stage and
    /// failure class (`docs/local-models.md §The local-model adapter seam`). In a
    /// persisted run every listed re-attempt succeeded (a second failure is not
    /// listed — the Portfolio job drops a failed holding's retry events as it
    /// isolates it), so entries measure the absorbed transient rate — the big-run
    /// retry watch's read.
    pub model_retries: Vec<crate::local_model::RetryEvent>,
    /// Infrastructure degradation worth surfacing prominently: deep-history
    /// failures, any current-multiple carry, a run-wide DGS10 history gap,
    /// context pressure on any local call, a length-stopped generation, or a
    /// fired model-call retry — a raw-percentile fallback from genuinely thin
    /// issuer history is counted but not flagged (as are the enriching-feed
    /// gaps above).
    pub attention: bool,
    /// The one-line deterministic summary the roll-up card renders.
    pub summary: String,
}

/// The prompt-fill fraction at which a local call's context is considered under
/// pressure (`context_pressure` above): at or beyond it, the sanctioned response
/// is compressing the prompt digests, never a `num_ctx` change
/// (`docs/portfolio-analysis.md` §Portfolio roll-up).
pub const CONTEXT_PRESSURE_FRACTION: f64 = 0.9;

/// The truncation-implausibility bound: a reported `prompt_eval_count` whose
/// `× TRUNCATION_CHARS_PER_TOKEN` cannot cover the chars the app actually sent
/// reads as **likely front-truncation**. Needed because the fill fraction alone
/// cannot see a truncation — Ollama's count is post-truncation and lands far
/// *below* `num_ctx`, not near it (live marker test: a ~4.6K-token prompt into
/// `num_ctx` 2,048 reported 1,026 — 50% fill —
/// `docs/verification/2026-07-28-m5-preflight.md` §Truncation behavior). Real
/// tokenization of this pipeline's prose/JSON prompts runs ~3–5 chars per
/// token; 8 is a deliberately generous bound so a trip is near-certain
/// truncation, not estimate noise.
pub const TRUNCATION_CHARS_PER_TOKEN: u64 = 8;

/// The deterministic run-level roll-up built after the per-holding pass
/// (`docs/portfolio-analysis.md` §Portfolio roll-up): verdict counts, the
/// concentration and cash reads — descriptive, consumed by no action logic —
/// plus the run-level data-health read. Whole-book reasoning is the future
/// portfolio planner's (`portfolio-v9` tunnel vision).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioRollUp {
    pub graded_count: usize,
    pub not_rated_count: usize,
    pub insufficient_evidence_count: usize,
    /// Analyzed holdings on the `role_risk_only` branch (`docs/portfolio-analysis.md`
    /// §Intrinsic verdict) — counted beside the priced (graded) holdings, never
    /// pooled with them.
    pub role_risk_only_count: usize,
    /// Holdings whose fresh analysis **failed** this run and was isolated
    /// (`docs/portfolio-analysis.md` §Failure posture) — the length of
    /// [`PortfolioRun::failed_holdings`]. A failed holding produced no fresh verdict
    /// (its prior may be carried), so it is counted here, never among the analyzed
    /// counts above.
    pub failed_count: usize,
    /// The largest single-position weight (0.0–1.0) — the concentration read.
    pub top_position_weight: f64,
    /// Cash as a fraction of the account total.
    pub cash_weight: f64,
    /// Positions closed since the last run (`docs/portfolio-analysis.md` §Holdings
    /// change tracking) — graded nowhere, but acknowledged here rather than silently
    /// dropped. Empty on a first run or when nothing was sold.
    pub exited: Vec<ExitedPosition>,
    /// The run-level data-health aggregate.
    pub data_health: DataHealth,
    /// A short deterministic synthesis line.
    pub overview: String,
}

/// How far back a filing-classified hard-forensic event binds the hard rule, in
/// days before the run's session date (drafted, calibratable — the submissions
/// feed's `filings.recent` window covers at least a year, so the sweep fully
/// serves this bound). An older event remains visible history in the filings
/// sweep but no longer trips the hard consequences.
pub const FORENSIC_EVENT_LOOKBACK_DAYS: i64 = 365;

/// Generic corporate-suffix tokens that cannot identify an issuer on their own
/// (drafted): the identity matcher skips them so "Company" or "Holdings" never
/// corroborates a cross-issuer citation.
const GENERIC_NAME_TOKENS: &[&str] = &[
    "COMPANY",
    "COMPANIES",
    "HOLDING",
    "HOLDINGS",
    "CORPORATION",
    "CORP",
    "INCORPORATED",
    "GROUP",
    "INTERNATIONAL",
    "INDUSTRIES",
    "ENTERPRISES",
    "ENTERPRISE",
    "LIMITED",
    "TECHNOLOGIES",
    "TECHNOLOGY",
    "GLOBAL",
    "PARTNERS",
    "CAPITAL",
    "FINANCIAL",
    "SYSTEMS",
    "SOLUTIONS",
    "SERVICES",
    "BRANDS",
    "RESOURCES",
    "TRUST",
    "FUND",
    "FUNDS",
    "SHARES",
    "CLASS",
    "COMMON",
    "STOCK",
    // Short suffix forms — below the name-token length floor anyway, listed
    // for the acronym derivation's trailing-suffix strip.
    "INC",
    "LTD",
    "PLC",
    "CO",
];

/// Whether a name word is a generic corporate suffix (case-insensitive) —
/// exposed for the first-party host check's acronym derivation.
pub(crate) fn is_generic_name_token(word: &str) -> bool {
    GENERIC_NAME_TOKENS.contains(&word.to_ascii_uppercase().as_str())
}

/// The distinctive issuer-name tokens (uppercased): ≥4 chars and not a
/// generic corporate suffix — the identity vocabulary shared by the page
/// matcher and the first-party host check.
pub(crate) fn distinctive_name_tokens(company_name: Option<&str>) -> Vec<String> {
    company_name
        .map(|name| {
            name.to_ascii_uppercase()
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|t| t.len() >= 4 && !GENERIC_NAME_TOKENS.contains(t))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The label words that qualify a colon as ticker context (drafted): exchange
/// and symbol markers only — a generic label (`Risk:`, `Rating:`,
/// `Category:`) must never turn its value into holding identity.
const TICKER_LABELS: &[&str] = &[
    "NYSE", "NASDAQ", "AMEX", "OTC", "OTCMKTS", "ARCA", "BATS", "CBOE", "TICKER", "SYMBOL",
];

/// Whether a symbol occurrence sits in ticker context: preceded (one optional
/// space skipped) by `$`, or by a colon whose own label word is an exchange /
/// ticker marker — `$CAT`, `NYSE: CAT`, `ticker:CAT`, but never `Risk: LOW`.
fn ticker_context(text: &str, start: usize) -> bool {
    let bytes = text.as_bytes();
    if start == 0 {
        return false;
    }
    let mut k = start - 1;
    if bytes[k] == b' ' {
        if k == 0 {
            return false;
        }
        k -= 1;
    }
    match bytes[k] {
        b'$' => true,
        b':' => {
            // Read the label word ending at the colon (one optional space).
            let mut end = k;
            if end > 0 && bytes[end - 1] == b' ' {
                end -= 1;
            }
            let mut label_start = end;
            while label_start > 0 && bytes[label_start - 1].is_ascii_alphanumeric() {
                label_start -= 1;
            }
            label_start < end
                && TICKER_LABELS
                    .iter()
                    .any(|l| text[label_start..end].eq_ignore_ascii_case(l))
        }
        _ => false,
    }
}

/// Whether the gap before a word carries a sentence terminator (or the word
/// opens the text) — the name leg's sentence-initial test.
fn sentence_initial(bytes: &[u8], start: usize, prev_end: Option<usize>) -> bool {
    let Some(prev_end) = prev_end else { return true };
    bytes[prev_end..start]
        .iter()
        .any(|b| matches!(b, b'.' | b'!' | b'?' | b'\n'))
}

/// Whether `text` names the holding. Two legs, both structural rather than
/// list-driven, because bare uppercase words are not reliable identity
/// evidence (any English-word ticker — `LOW`, `CAT`, `ALL` — collides with
/// page prose and furniture):
///
/// - **Symbol** — an exact-case word match accepted only in **ticker
///   context**: preceded by `$`, or by a colon whose label word is an
///   exchange / ticker marker (`$CAT`, `NYSE: CAT` — never `Risk: LOW`). A
///   bare uppercase word never satisfies this leg; prose identity is the
///   name leg's job.
/// - **Name** — a **distinctive** issuer-name token (≥4 chars, not a generic
///   corporate suffix) as a capitalized word. A sentence-initial match counts
///   only when the next word is also capitalized (a proper-noun run: "Target
///   Corporation reported" yes, "Target price increased" no) — mid-sentence
///   capitalization is itself the proper-noun signal.
///
/// Word-boundary matching throughout — a token inside a longer word (COMPANY
/// in ACCOMPANYING) never matches. Shared by the pre-profit page cross-check
/// and the typed-channel issuer validations; rejection is always fail-soft
/// (a gap-logged dropped row or claim, never a failed run).
pub(crate) fn text_names_holding(text: &str, symbol: &str, company_name: Option<&str>) -> bool {
    let sym = symbol.trim();
    let name_tokens = distinctive_name_tokens(company_name);
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut prev_end: Option<usize> = None;
    while i < bytes.len() {
        if !bytes[i].is_ascii_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
            i += 1;
        }
        let word = &text[start..i];
        if !sym.is_empty() && word == sym && ticker_context(text, start) {
            return true;
        }
        if !name_tokens.is_empty()
            && word.as_bytes()[0].is_ascii_uppercase()
            && name_tokens.iter().any(|t| t.eq_ignore_ascii_case(word))
        {
            if !sentence_initial(bytes, start, prev_end) {
                return true;
            }
            // Sentence-initial: require a following capitalized word.
            let mut j = i;
            while j < bytes.len() && !bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_uppercase() {
                return true;
            }
        }
        prev_end = Some(i);
    }
    false
}

/// The per-holding outcome of the item-classified 8-K filings sweep — the
/// hard-forensic **filing kinds'** producer state (`docs/portfolio-analysis.md`
/// §Starting parameters — the conviction-layer caps; the shared producer
/// contract is `docs/trade-opportunities-workflow.md §Step 5c`). `Unknown` is a
/// logged degraded input, never a fabricated clear and never a silent no-event —
/// and it never trips the hard rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum ForensicFilingState {
    /// The sweep ran and classified these events inside the lookback.
    Events { events: Vec<crate::sec::ForensicEvent> },
    /// The sweep ran clean — no qualifying item inside the lookback.
    Clear,
    /// The sweep could not run or could not be read: no CIK mapping
    /// (`queried: false`) or a failed / malformed fetch (`queried: true`).
    Unknown { reason: String, queried: bool },
}

impl ForensicFilingState {
    /// Whether the hard rule trips — only a classified event does; `Clear` and
    /// `Unknown` never do.
    pub fn hard_tripped(&self) -> bool {
        matches!(self, ForensicFilingState::Events { events } if !events.is_empty())
    }
}

/// The audit's hard-forensic record: the sweep state plus, when tripped, the
/// engine-matched rule — persisted as an annotation binding the **engine arm**
/// exactly as a pre-profit ceiling does (`docs/portfolio-workflow.md §Step 6g`);
/// the model's conviction and action persist as authored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForensicRead {
    pub state: ForensicFilingState,
    /// The matched hard rule, recorded when tripped (engine conviction capped
    /// Low; the add family barred from the engine action set).
    pub matched_rule: Option<String>,
}

/// One holding's audit record (`docs/storage.md §Local Analysis Suite Storage`):
/// what the verdict was based on, so a run is traceable and reviewable — the
/// computed metrics and price-target methodology behind the numbers, the sources
/// used, the model ids, the prompt/schema version, and any degraded-input flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingAudit {
    pub symbol: String,
    pub metrics: engine::ComputedMetrics,
    /// The data sources this holding's verdict **actually consulted**, with a note
    /// each (e.g. "FMP company financials"). Assembly-time labels come from the
    /// dossier (the Schwab holdings snapshot every position reads from, the FMP
    /// pull that actually ran — the stock statement / consensus surface or the
    /// fund's quote + EOD + dividend surface — the profile lookup where it ran, the SEC leg where its facts endpoint
    /// was queried — "(empty)" when it returned nothing, no label when no CIK
    /// mapping meant it never was — the chain leg where it was requested — "(none
    /// returned)" when nothing came back — and the fund surface); the pipeline
    /// appends the FRED rate anchors only where a priced engine output computed
    /// from them and the house view only where a prompt rendered it.
    pub sources: Vec<String>,
    /// The local model ids the verdict was **actually authored with**, in
    /// first-call order — drained from each outbound request's routed model,
    /// never inferred from the configured roster: empty on every no-model exit
    /// (not-rated, the listing guard, an evidence-floor abstention); normally
    /// reasoner then fast tier on analyzed live branches because research
    /// precedes distillation, with each id present only if a call actually used
    /// it and duplicate ids collapsed in place.
    pub model_ids: Vec<String>,
    /// The prompt/schema version the interpretation ran under.
    pub prompt_version: String,
    /// The evidence-floor rule version the holding was floored under
    /// (`engine::EVIDENCE_FLOOR_VERSION`) — attribution, so a floor correction
    /// never silently re-reads a prior abstention or priced verdict.
    pub evidence_floor_version: String,
    /// Inputs a source could not resolve, carried from the financials' gap manifest.
    pub degraded_inputs: Vec<String>,
    /// App-stamped annotations from the per-holding action call — today the one case
    /// is a chosen rung outside the engine's per-holding action set, which persists
    /// exactly as authored with the departure recorded here (the two-arm contract:
    /// engine evidence annotates, never bars).
    pub action_annotations: Vec<String>,
    /// How the scenario targets were derived — rung, fallbacks, and the parameter
    /// version target calibration keys on (`docs/portfolio-analysis.md` §Outcome
    /// learning). `None` on a not-rated / abstained / role-risk-only holding.
    pub target_meta: Option<engine::TargetMeta>,
    /// The grade-parameter version the letter and sub-scores were computed under
    /// ([`engine::GRADE_PARAMETER_VERSION`]) — the boundary marker that lets the
    /// what-changed audit and outcome-learning cohorts recognize a parameter boundary
    /// for what it changed: a band recalibration (letters moving with no input
    /// change) or a stamped sub-score's input re-homing
    /// ([`engine::grade_parameter_change`]). Stamped on every audit, the early
    /// exits included.
    pub grade_parameter_version: String,
    /// The ledger legs of the continuity audit — crossings consumed, downgrades,
    /// supersessions, closures, rejected claims (`docs/portfolio-workflow.md` §Step
    /// 6g). `None` on a not-rated holding.
    pub ledger_audit: Option<LedgerAudit>,
    /// The stored closed-form re-anchor basis for the engine-only quick paths
    /// (`docs/portfolio-analysis.md` §The quick check) — the anchor-window spread
    /// percentiles, drivers, and comparators the last full pass computed. `None` on
    /// not-rated / abstained / role-risk-only holdings.
    pub quick_basis: Option<engine::QuickCheckBasis>,
    /// The split-bridge anchor bar (`docs/portfolio-analysis.md` §Starting
    /// parameters): the newest settled close strictly before the run's ET session,
    /// from this run's own fetched dated-EOD series. A later engine-only pass
    /// re-reads the same bar date from its fresh fetch and the close ratio is exactly
    /// the cumulative split re-basis between the two fetch times
    /// ([`engine::split_bridge_factor`]), converting every stored price-denominated
    /// value onto the fresh basis. Stamped on both analyzed branches; `None` on
    /// no-price exits — those rows' comparisons run unbridged until their next full
    /// pass stamps one.
    pub authoring_close: Option<engine::DatedValue>,
    /// The fund exposure comparators for the quick check's fund evidence-event legs
    /// (`docs/portfolio-analysis.md` §Starting parameters) — present on a fund
    /// holding of either verdict branch; `None` on stocks.
    pub fund_exposure: Option<fund::FundExposureBasis>,
    /// The pre-profit execution / financing overlay record
    /// (`docs/portfolio-analysis.md` §Starting parameters) — present on every priced
    /// stock (the eligibility result persists even when the stock does not enter; the
    /// period-end-and-span-keyed observation history rides here so it survives run retention and
    /// the selective carry). `None` on funds, `role_risk_only` holdings.
    pub pre_profit: Option<pre_profit::PreProfitOverlay>,
    /// The full hurdle read behind the verdict's three-state `dead_money` field — the
    /// scenario total-return distribution plus the tier-scaled hurdle rate, persisted
    /// so a decision episode's calibration snapshot can freeze the hurdle inputs
    /// (`docs/portfolio-analysis.md` §Outcome learning). `None` on not-rated /
    /// abstained / role-risk-only holdings.
    pub hurdle: Option<engine::HurdleRead>,
    /// The hard-forensic filings-sweep record ([`ForensicRead`]) — present on a
    /// priced stock whose gather ran the item-classified 8-K sweep (state `Unknown`
    /// where it couldn't); `None` on funds, skipped retrievals.
    pub forensic: Option<ForensicRead>,
    /// The input delta's technology-event pre-flag record
    /// ([`engine::TechEventPreFlag`]) — present where the flag was evaluable (a
    /// carried stock with a benchmark series and a volatility read); an unevaluable
    /// flag records its reason in `degraded_inputs` instead. `None` on debuts, funds.
    pub tech_event_pre_flag: Option<engine::TechEventPreFlag>,
    /// This holding's FINRA short-interest row off the once-per-run consolidated file
    /// (`docs/data-sources.md §FINRA`) — risk / squeeze-context positioning evidence,
    /// held out of every sub-score. `None` on funds, symbols absent from the file,
    /// runs whose file fetch gapped.
    pub short_interest: Option<crate::finra::ShortInterestRead>,
    /// The implied-expectations range ([`engine::ImpliedExpectations`]) the
    /// interpretation prompt rendered — the priced-in anchor, recorded per priced
    /// stock (`docs/portfolio-analysis.md` §Starting parameters). `None` on funds,
    /// the current-multiple carry, every early exit.
    pub implied_expectations: Option<engine::ImpliedExpectations>,
    /// The narrative-vs-reality read ([`engine::NarrativeRead`]) — the
    /// conviction-layer red-flag ratio with, when tripped, its matched soft rule (the
    /// engine arm's Medium ceiling), persisted as an annotation exactly as a
    /// pre-profit ceiling is (`docs/portfolio-workflow.md` §Step 6g). `None` on
    /// funds, debuts, unreadable paces (the reason rides `degraded_inputs`), every
    /// early exit.
    pub narrative: Option<engine::NarrativeRead>,
    /// The typed same-underlying option overlay ([`dossier::OptionOverlay`]) the
    /// holding carried — legs, class, coverage, and delta gaps, recorded wherever the
    /// dossier assembled one. `None` on holdings with no option legs, funds, skipped
    /// retrievals.
    pub option_overlay: Option<dossier::OptionOverlay>,
    /// The validated what-changed attribution ([`WhatChangedAudit`]) — the typed rows
    /// resolved against the rendered input delta at the 6g seam, with the
    /// standing-thesis and self-correction signals outcome learning consumes. `None`
    /// on debuts, every early exit.
    pub what_changed_audit: Option<WhatChangedAudit>,
    /// The research-loop audit record (`docs/storage.md §Local Analysis Suite
    /// Storage` — the research-derived artifacts): sources with retrieval timestamps,
    /// the distilled findings (the combined object and the reconciled per-topic seed
    /// layer), per-topic seeded-vs-cold decisions, budget spend, and the distillation
    /// shape. `None` on every no-research exit (not-rated, the listing guard, an
    /// evidence-floor abstention).
    pub research: Option<distill::ResearchAuditRecord>,
}

/// The schema/prompt version stamped on each run's audit, bumped when the
/// interpretation contract changes so older runs stay legible. v2: the verdict union
/// (priced / role-risk-only), the engine-bounded feasible action set, the v2
/// rate-anchored scenario targets, and the rolling-window target rename. v3: the
/// interpretation-prompt adjustments (2026-07-31 F6 + the grade-band slice's
/// versioning finding) — target provenance rendered from the typed `TargetMeta`,
/// the dead-money tilt softened to a weighed input, conviction defined against the
/// action's decisiveness, the house view scoped to horizon/market-setup context,
/// and a band-recalibration continuity note when the prior verdict's
/// `grade_parameter_version` differs from the current bands. v4: the thesis ledger
/// (`docs/portfolio-analysis.md` §The position thesis ledger) — the prior ledger and
/// the engine's condition crossings rendered into the prompt (the first prior-run
/// content the prompt carries), and the rewritten ledger required in the response,
/// validated at the 6g seam. v5: the pre-profit execution / financing overlay
/// (`docs/portfolio-analysis.md` §Starting parameters) — the finalized overlay
/// rendered into an eligible stock's prompt with its engine-matched conviction
/// ceiling, the conviction enum structurally narrowed beneath a matched ceiling,
/// and severe deterioration restricting the offered action set to the exit family.
/// v6: the 7b construction stage (`docs/portfolio-workflow.md` §Step 7b) — 6f now
/// authors the **standalone action lean** over the intrinsic bars alone (the full
/// ladder; only severe pre-profit deterioration restricts, to the exit family —
/// the feasible-set bars moved to construction), the `role_risk_only`
/// interpretation no longer authors an action (its action arises wholly at
/// construction from the reduced spine), and the new run-level **portfolio
/// construction** call sets each holding's final action + target-weight range,
/// the action half of the what-changed audit, and the portfolio-level view,
/// validated by the deterministic joint-feasibility check with one
/// named-violation re-run ([`construction`]).
/// v7: the two-arm verdict (`docs/portfolio-analysis.md` §The holding verdict) —
/// the interpretation additionally authors the **model arm** (its own four
/// sub-scores, freely-authored one-/twelve-month target bands, and a
/// retrospective self-assessment; the model letter is derived app-side from the
/// model's scores through the shared cutoffs), the lean and conviction enums
/// are **unrestricted** (the full ladder / full enum — engine bounds and the
/// pre-profit ceiling render as prompt evidence and audit annotations, never
/// schema bars or clamps), the prior run's both-arm values plus realized-since
/// render into the prompt (the retrospective — deliberately reversing the v4
/// anchoring guard), and the engine gains its mechanical stand-in arm
/// ([`EngineView`]) so every model field has a scored baseline counterpart.
/// Model-arm values never alter or bind the engine baseline
/// (`docs/portfolio-analysis.md` §The holding verdict).
///
/// `portfolio-v8`: at construction, the divergence-cause vocabulary gains the
/// sell-side `cash-raised` twin and is stated in the construction prompt with
/// per-cause checkability semantics and the null-cause escape hatch; an uncaused
/// lean departure annotates as an unattributed divergence instead of failing
/// validation, and a checkability-failed divergence cause surviving the single
/// repair is stripped and annotated rather than failing the run (ruled
/// 2026-08-13, `docs/verification/2026-08-13-big-run-attempt-2.md` §Disposition).
/// At interpretation, the attempt-2 clarity tightenings: the NEW-position line
/// disarms the fresh-purchase misread, volatility and expense-ratio carry unit
/// labels, conviction declares its three-value type, and both sub-score blocks
/// state the risk-score polarity (same record, §Workstream 2).
///
/// `portfolio-v9`: the tunnel-vision contract (user decision 2026-08-14) — the
/// job stops comparing holdings. The 7b construction stage is **removed**
/// (aggregates, joint-feasibility solve, divergence causes, repair re-run,
/// degraded persist all retired); every action is authored by the new
/// **per-holding action call** — a dedicated stage after interpretation that
/// reads the finished intrinsic verdict, the holding's own sizing evidence, and
/// the **investor profile** (which now enters here, keeping 6f profile-blind)
/// and returns a rung-only action with a one-line rationale, the full ladder
/// open on both branches (`role_risk_only` included — its engine set stays the
/// reduced evidence set). Interpretation no longer authors an action; sizing
/// (target-weight ranges, share/dollar deltas) is retired wholesale, and the
/// thesis ledger drops its pre-committed target-weight range. The whole-book
/// reconciliation is deferred to the future portfolio-planner job.
///
/// `portfolio-v10`: the run-evidence slice — the prompts gain the Step-5
/// run-level context evidence (the commodity price context for
/// commodity-linked holdings, the CFTC underlying-positioning read for
/// commodity / macro funds, and the CBOE venue-level put/call backdrop) and
/// the **hard-forensic filings read**: an item-classified restatement /
/// auditor-change 8-K renders as typed evidence with the engine-matched hard
/// rule (engine conviction capped Low, the add family barred from the engine
/// set), binding the engine arm and annotating — never clamping — the model's
/// (`docs/portfolio-analysis.md` §Starting parameters).
///
/// `portfolio-v11`: the typed what-changed attribution (the metric-level 6g
/// validator — `docs/portfolio-workflow.md` §Step 6g). The prompt renders the
/// engine's **input delta** as bracketed-id entries (position delta, the
/// metric / sub-score / grade / target moves against the prior audit's stored
/// values, ledger crossings, the technology pre-flag, the narrative read, a
/// band recalibration, the house view), and the 6f response adds
/// `what_changed_entries` — one typed row per moved intrinsic value (kind,
/// old → new, an external attribution or self-correction, the evidence id).
/// The 6g seam validates every external attribution against the rendered
/// delta; an unresolvable one is **downgraded to self-correction with a
/// logged reason** (exact old ≠ new resolution, ruled 2026-08-21 — no
/// materiality margin). The validated audit wakes the outcome layer's
/// standing-thesis episode leg and self-correction counters
/// (`docs/portfolio-analysis.md` §Outcome learning).
///
/// `portfolio-v12`: the live research loop (`docs/portfolio-workflow.md`
/// §Step 6c–6e). The 6c prompt surface is new (per-topic pass conversations
/// with the web tools and the findings grammar), 6d is schema-constrained
/// with the typed side-channels (`research_forward_assumption`,
/// `validated_leading_indicator`, `forensic_event`,
/// `pre_profit_execution_observations`), the interpretation prompt carries
/// real distilled research (a validated leading indicator rendering as
/// ledger-driver evidence), the input delta gains addressable
/// research-finding entries, and the `role_risk_only` branch runs the fund
/// agenda + pure consolidation (its prompt gains the distilled section).
///
/// `portfolio-v13`: the research→ledger tie channel closed (the 2026-08-24
/// review's F3). Every claim-emitting 6d prompt renders the ledger conditions
/// with their ids and asks for `related_condition_id`; the interpretation
/// prompt's ledger projection marks research-supported conditions off the
/// input delta's tied research entries (`DeltaEntry.related_condition_id`,
/// persisted on the what-changed audit) and its rewrite instruction names
/// that mark as the qualitative leg.
///
/// `portfolio-v14`: the expense-ratio render (the 2026-08-24 review's
/// Priority-1 minor). The role-risk, interpretation, and action prompts state
/// a fund's expense ratio / drag through one shared formatter as the decimal
/// fraction at four places — the ledger's unit — beside its percent reading
/// (`0.0003 (0.03%/yr)`), where `opt()`'s three places had flattened a 0.03%
/// fund to `0.000` against the legend's own arithmetic. A value-format change
/// under an unchanged legend, stamped so a pre-fix checkpoint cannot resume
/// into the corrected render and every record names the render it was
/// authored under.
///
/// `portfolio-v15`: the ledger's statement basis (the 2026-08-24 review's
/// Priority-1 minor on the TTM vocabulary). The engine-series vocabulary no
/// longer says "TTM net margin" / "TTM gross margin" — the statement-derived
/// labels name no basis — and the ledger section of both the interpretation
/// and the role-risk prompt states the holding's flow basis this run (TTM,
/// SEC annual, or none) beside the vocabulary, naming debt / equity and
/// price / book as balance-sheet instants outside it, so a flow-series
/// threshold is authored on the basis it is evaluated against; the
/// evaluation's basis-change note reads the same labels. A vocabulary and
/// section change, stamped so a pre-fix checkpoint cannot resume into it.
///
/// `portfolio-v16`: the IV-skew sign convention (the 2026-08-24 review's
/// Priority-1 minor). The interpretation prompt's options-activity line
/// renders the skew signed (`+0.030` / `-0.020`, an unsigned `0.000` where it
/// rounds away) and states its convention on the line — chain-wide mean put
/// IV minus mean call IV, in IV's decimal unit; positive = puts richer
/// (hedging demand), negative = calls richer (call speculation) — where
/// `opt()` had printed the bare value and put-minus-call lived only in a doc
/// comment, so a model assuming the inverse read hedging demand as call
/// speculation. A value-format and label change under an unchanged line
/// header, stamped so a pre-fix checkpoint cannot resume into the corrected
/// render.
///
/// `portfolio-v17`: the pre-profit observation row's source excerpt (the
/// 2026-08-24 review's Codex I3). The 6d distillation schema's
/// `pre_profit_observations` row gains a required `source_excerpt` — the
/// fetched page's own sentence that states the value, quoted verbatim — and
/// the prompt line asks for it, so Step 6e's corroboration binds the number
/// to one sentence about the declared metric at its printed sign rather than
/// to "somewhere on the page". A schema and prompt-line change, stamped so a
/// pre-fix checkpoint cannot resume into rows the new leg would reject.
///
/// `portfolio-v18`: the observation row's `published_at` named in the 6d
/// prompt (the 2026-08-24 review's Codex I4). The guidance vintage policy
/// makes the row's publication date load-bearing for the first time — the
/// execution read pairs an actual only against ex-ante guidance and the
/// latest revision binds — and the prompt had never said what the date is,
/// so the line now asks for the quoted page's own publication date, a
/// guidance row's own issue date, never the fetch date. A prompt-line
/// change, stamped so a pre-fix checkpoint cannot resume into rows dated
/// under no stated meaning.
///
/// `portfolio-v19`: the action call receives both arms' price targets (the
/// 2026-08-24 review's Codex I5). The action prompt had rendered the model
/// arm's letter and sub-scores only, then implied moves from the engine's
/// twelve-month band alone, so the model's own authored forecast never
/// reached the rung it then decided. The prompt now renders both arms'
/// one-month and twelve-month implied bear/base/bull moves — the engine's
/// under their provenance (`(gap)` where a leg was underivable), the model's
/// as its own unvalidated band, an off-domain leg or an inverted band tagged
/// as authored — and the system prompt names both arms and how each is
/// weighed. An evidence-set change to the action call, stamped so a pre-fix
/// checkpoint cannot resume into rungs decided on a different input set.
///
/// `portfolio-v20`: the model arm's declared numeric domain enforced (the
/// 2026-08-24 review's Codex I6). The prompt had stated the 0–100 sub-score
/// scale and target positivity, but the grammar cannot express range
/// keywords and the app never checked, so a finite `10000` derived an
/// ordinary A and a zero or negative target persisted into the scoreboard.
/// The decode now rejects an off-domain response ([`validate_model_arm`])
/// under the bounded retry-once's own class, and the model-arm paragraph
/// names each domain as enforced — a prompt-line change and an admission
/// gate together, stamped so a pre-fix checkpoint cannot resume into rows
/// the gate would reject.
///
/// `portfolio-v21`: the narrative-vs-reality render distinguishes a hype read
/// whose ratio overflowed (a positive reality leg the expansion outran beyond
/// any finite multiple — persisted with the ratio absent since the review's
/// Codex I16) from one whose reality leg is non-positive, where the prompt
/// had called both "reality flat or declining"; and a finite decimal leg
/// whose ×100 overflows renders as the decimal ratio, never `inf%`. An
/// edge-only render change, stamped on I12's precedent (Codex I16, round 2;
/// ruled 2026-08-29).
///
/// `portfolio-v22`: the group-3 prompt renders and the period-word guard (the
/// 2026-08-24 review's Codex I8, I10, I12 and I19, ruled 2026-08-29). The
/// priced-fund FUND CONTEXT line renders the ≥ 70% guard's own US share
/// (`fund::us_share` — every alias summed, capped) where it had read the
/// first "united states" label alone; the one-month engine targets carry
/// their methodology line like the twelve-month ones; both ledger-crossing
/// renders print observed and threshold as one pair at one comparison-safe
/// precision — the expense-ratio render's floor, extended until the rendered
/// pair, read back as numbers, orders as the values do, the shortest
/// round-trip render past ten places; and Step 6e's one-fact admission
/// filter rejects a value that is itself the period — a 1900–2099 year
/// printed without a thousands separator right after `for / in / of / by /
/// through / fiscal / FY`, a range when both endpoints read so — with the 6d
/// prompt line stating the rule. Prompt content and the admission leg move
/// together on I3's `portfolio-v17` precedent; no other axis moves.
///
/// `portfolio-v23`: the two continuity-attribution mirrors (the 2026-08-24
/// review's Codex I11 and I13, group 4, ruled 2026-08-29). The scenario-target
/// stamp gains the grade stamp's mechanism — a stamp history
/// (`engine::SCENARIO_TARGET_PARAMETER_HISTORY`), the prior audit's
/// `target_meta.parameter_version` carried onto the dossier, an input-delta row
/// and a continuity NOTE naming the horizons a boundary can have moved on the
/// prior's branch — where a target moved on a version bump alone had been
/// attributable to company evidence or a self-correction. And the two
/// balance-sheet instants carry a second continuity stamp, the equity source
/// (`EquitySource`, stamped at the SEC merge, `authored_equity_source` on the
/// evaluation state), under the flow-basis gate's one-pass-unevaluable
/// treatment, with the ledger section's basis line naming which balance sheet
/// supplied their equity this run. A prompt-content change (the NOTE and the
/// basis line) beside a new persisted evaluation-state field, stamped so a
/// pre-fix checkpoint cannot resume into rows the new stamp never reached;
/// the grade, target, evidence-floor, pre-profit and checkpoint-format axes
/// stay — the target function itself is unchanged. Codex round 1 on the group
/// added, under the same stamp, the authoring stamps ([`ContinuityStamps`] —
/// Step 6g writes the prompt's basis and source onto every new or superseding
/// quantitative condition) and the sweep's withhold of a debt/equity condition
/// stamped off its own FMP-quarterly source.
///
/// `portfolio-v24`: pre-profit observation rows carry a required reporting
/// span beside their normalized period end. The 6d schema and prompt name the
/// field, and the app rejects a span that conflicts with an explicit Q / H /
/// FY / YTD label; an unknown span remains audit context and never pairs.
///
/// `portfolio-v25`: analyst revision is measured only across fiscal-period EPS
/// rows present in both the prior and current snapshots, using the prior NTM
/// weights renormalized across those matches. Rolling NTM remains the valuation
/// driver, but its changing calendar weights can no longer manufacture the
/// quick-check revision event or distort narrative-vs-reality; no common period
/// makes revision unavailable and sends narrative to its operating fallback.
///
/// `portfolio-v26`: forward-assumption units scale USD cents to dollars and
/// reject named foreign currencies because target refinement has no dated FX
/// input. The shadow resolution and the interpretation input delta therefore
/// move together under the corrected unit-admission semantics (Review 2 M1).
///
/// `portfolio-v27`: fund classification keeps explicit allocation / multi-asset
/// classes out of the pure-equity pricing path, recognizes fixed-income
/// ultra-short duration names, and renders the exact leveraged/inverse versus
/// option-overlay structural cause to the role-risk model (Review 2 M4 / M5 / Q4).
///
/// `portfolio-v28`: commodity context uses the profile industry to route uranium
/// to its own print and to withhold oil / gas proxies from coal producers
/// (Review 2 M14). Guard-terminal benchmark suppression changes retrieval only.
///
/// `portfolio-v29`: input deltas preserve every exact move in their rendered
/// old/new pair, rule-demoted actions keep their provenance in continuity and
/// retrospective context, and action tax framing follows the investor profile
/// (Review 2 M17 / M19 / M20).
///
/// `portfolio-v30`: leading-indicator validation accepts grounded percentage
/// renders and ISO month precision, forensic issuer fields accept their typed
/// one-word identity, and research prompts state the four-id seed-lineage cap
/// (Review 2 M24–M27).
///
/// `portfolio-v31`: schema-carrying prompt clarity, from the 2026-08-30 big-run
/// Finding 2 investigation. The ledger-authoring prose names the `quant` object
/// and its four fields, states the anti-pattern that produced the finding — a
/// numeric threshold on an engine series left in the statement text with `quant`
/// null, which cannot be machine-evaluated and silently degrades to a prose-only
/// condition — gives the decimal-scale example (gross margin below 16% is
/// threshold 0.16, not 16), and describes the falsifier-only `technology_class`
/// (true only for a third-party technology-event falsifier, false for an ordinary
/// financial-metric condition) where the prompt had left that required flag
/// unexplained — the falsifier/trigger field split named exactly, so the prose
/// mirrors the schema (`quant` on both, `technology_class` on falsifiers, `family`
/// on triggers). The same
/// anti-pattern is stated once more where the shape recurs: the distillation
/// typed-field header says a value one of those machine-read fields captures
/// (forward_assumption / leading_indicator / forensic_event / pre-profit rows)
/// belongs in the typed field, not only in the free-text `combined_findings`
/// prose; and the what-changed authoring section names the row's six fields
/// rather than leaving them to the grammar and the response-contract sentence
/// alone. Prompt-prose changes across the interpretation (both branches) and the
/// distillation reduce / single-pass prompt, stamped so a pre-fix checkpoint
/// cannot resume into them; no schema or other axis moves
/// (`docs/verification/2026-08-30-big-run-findings.md` §Finding 2).
///
/// `portfolio-v32`: action-call prompt clarity, from the 2026-08-30 big-run
/// Finding 3 investigation. The ENGINE SET prose (both the system prompt and the
/// user prompt's set line) now says the app stamps a departure from the set onto
/// the holding's audit for the model, so the model emits only the rung and the
/// one-sentence rationale — the schema carries no annotation field — where the
/// passive "with the departure annotated" had left the model re-deriving whether
/// an in-set pick owes an annotation of its own (Signal 1). The capital-efficiency
/// prose (both prompts) now states that a `clears` or `indeterminate` read is
/// neutral — neither dead money nor an exit input — so it must not tilt the rung
/// toward selling, where the prompt had said only that `fails` is dead money and
/// left the non-`fails` states' neutrality unstated, and an `indeterminate` read
/// leaked in as a soft sell-lean (Signal 2). Both were the action prompt lagging
/// contracts the docs already state (`docs/portfolio-analysis.md` §Portfolio
/// action: the departure is app-stamped; indeterminate neither tilts the decision
/// nor creates dead money); prompt-prose only, no schema or other axis moves
/// (`docs/verification/2026-08-30-big-run-findings.md` §Finding 3).
pub const PROMPT_VERSION: &str = "portfolio-v32";

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
    /// The run-level `DGS2` / `DGS10` prints the targets and hurdles were computed
    /// from, with their as-of dates — the persisted rate cache the engine-only quick
    /// paths' fail-soft reads (`docs/portfolio-analysis.md` §The quick check;
    /// §Starting parameters, rate-cache max age).
    pub rate_prints: RatePrints,
    /// This run's outcome-learning records — appended / extended episodes, this run's
    /// alignment tags, newly matured window labels, and the derived scorecard reads
    /// (`docs/portfolio-analysis.md` §Outcome learning; `docs/portfolio-workflow.md`
    /// §Step 7a, §Step 8).
    pub outcome: outcome::OutcomeRecords,
    /// Per-holding analysis failures the run **isolated** rather than aborting on
    /// (`docs/portfolio-analysis.md` §Failure posture): the model/grade half
    /// hard-fails **per holding**, and the run records the failure here and moves
    /// on. A symbol present here renders a failed card — its prior verdict carried
    /// into `verdicts` where one exists (the data shows vintage-stamped beside the
    /// failed badge), or an empty debut-failure card where none does. Empty on a
    /// clean run.
    pub failed_holdings: Vec<HoldingFailure>,
}

/// A per-holding analysis failure the run isolated (`docs/portfolio-analysis.md`
/// §Failure posture). The model/grade half is fail-hard **per holding, not per
/// run**: a hard failure in one holding's interpretation / action / persistence is
/// recorded here and the run continues to the next holding, rather than failing the
/// whole run — the run only fails outright when **every** attempted holding fails
/// (a systemic cause) or a run-level infrastructure step fails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingFailure {
    pub symbol: String,
    /// The concise failure read — the failing operation **plus its root cause**
    /// (e.g. "distilling research findings: <root>", or a single-level
    /// "action decision for TSLA returned an empty rationale"). The full error chain
    /// rides the run tracker's failed step detail and stderr; this is the
    /// user-legible card line.
    pub cause: String,
    /// Whether a prior successful verdict was carried forward for this holding: the
    /// card shows that vintage-stamped data beside the failed badge (`true`), or is
    /// an empty debut-failure card with no data to show (`false`).
    pub carried_prior: bool,
}

/// The action a carried verdict would stand on — `None` where the disposition
/// carries no action (not-rated / insufficient-evidence). Consumed by [`job`]'s
/// carry gate.
///
/// The action is the per-holding action call's rung.
pub(crate) fn carried_action(verdict: &HoldingVerdict) -> Option<Action> {
    match &verdict.disposition {
        VerdictDisposition::Priced(g) => Some(g.action),
        VerdictDisposition::RoleRiskOnly(r) => Some(r.action),
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

// ---- The model's schema-constrained interpretation ---------------------------

/// The model's **draft** of a quantitative condition core — the claim the app
/// validates (`docs/portfolio-workflow.md` §Step 6g: a class claim is app-validated,
/// never a bare assertion): `series` / `comparator` are strings here so an
/// unresolvable claim downgrades to qualitative with a logged reason rather than
/// failing deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuantCoreDraft {
    pub series: String,
    pub comparator: String,
    pub threshold: f64,
    #[serde(default)]
    pub margin: f64,
}

/// The model's draft of one falsifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FalsifierDraft {
    pub statement: String,
    /// The machine core where the condition is quantitative; `null` = qualitative.
    #[serde(default)]
    pub quant: Option<QuantCoreDraft>,
    #[serde(default)]
    pub technology_class: bool,
    /// The model's tripped claim — honored only when it maps to an engine crossing
    /// or a source-backed finding (Step 6g).
    #[serde(default)]
    pub tripped: bool,
}

/// The model's draft of one action trigger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerDraft {
    pub statement: String,
    /// "add" / "trim" / "sell" (a `role_risk_only` ledger offers trim / sell only).
    pub family: String,
    #[serde(default)]
    pub quant: Option<QuantCoreDraft>,
    /// The model's fired claim — validated like a tripped falsifier.
    #[serde(default)]
    pub fired: bool,
}

/// The model's draft of one monitor scenario — conditions and a probability lean
/// only; the engine's scenario price target is stamped by the app, never authored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioDraft {
    pub conditions: String,
    pub probability_pct: f64,
}

/// The model's rewritten thesis ledger (`docs/portfolio-analysis.md` §The position
/// thesis ledger — interpretation "rewrites the thesis, re-weights the scenarios,
/// and re-sets the triggers"). The app validates it at the 6g seam: executability,
/// condition identity / carry, and tripped / fired claims.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerDraft {
    /// The current thesis (the original thesis is app-frozen at debut, never re-authored).
    pub thesis: String,
    pub key_drivers: Vec<KeyDriverDraft>,
    pub bear: ScenarioDraft,
    pub base: ScenarioDraft,
    pub bull: ScenarioDraft,
    pub what_must_improve: String,
    pub what_must_not_break: String,
    pub falsifiers: Vec<FalsifierDraft>,
    pub triggers: Vec<TriggerDraft>,
}

/// One key-driver draft (name + the engine series claim, validated app-side).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyDriverDraft {
    pub name: String,
    #[serde(default)]
    pub series: Option<String>,
}

// ---- The what-changed attribution (the metric-level 6g validator) ---------------
//
// The typed half of the what-changed audit (`docs/portfolio-analysis.md` §What
// changed): the model authors one row per moved intrinsic value, attributing it to
// an external change or a self-correction; the 6g seam resolves every external
// attribution against the engine's rendered input delta and downgrades an
// unresolvable one to self-correction with a logged reason — so a no-new-facts
// swing cannot be laundered as "the market changed" (`docs/portfolio-workflow.md`
// §Step 6g).

/// Which intrinsic value a what-changed row claims moved. The thesis-scoped kinds
/// (`Thesis`, `ScenarioWeights`) are the standing-thesis episode leg's key
/// (`docs/portfolio-analysis.md` §Outcome learning): value-level moves are input
/// movement — observations on the active episode, never an open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangedValueKind {
    Grade,
    SubScore,
    Conviction,
    Target,
    Horizon,
    ScenarioWeights,
    Thesis,
    Condition,
    /// The `role_risk_only` branch's role / exposure / risk reads.
    RoleRead,
}

/// How a moved value is attributed (`docs/portfolio-analysis.md` §What changed):
/// one of the three external categories, tied to evidence — or the flagged
/// self-correction, where the inputs did not materially change and the model is
/// revising its own prior read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeAttribution {
    MarketData,
    CompanyInformation,
    ResearchNarrative,
    SelfCorrection,
}

impl ChangeAttribution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MarketData => "market-data",
            Self::CompanyInformation => "company-information",
            Self::ResearchNarrative => "research-narrative",
            Self::SelfCorrection => "self-correction",
        }
    }
}

/// One typed row of the what-changed audit, authored at 6f beside the prose line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhatChangedEntry {
    pub kind: ChangedValueKind,
    /// Which value moved (e.g. "quality sub-score", "twelve-month base target").
    pub detail: String,
    pub old: String,
    pub new: String,
    pub attribution: ChangeAttribution,
    /// The input-delta entry backing an external attribution — the bracketed id
    /// (e.g. "D2") or the entry's label verbatim; empty on a self-correction.
    pub evidence: String,
}

/// One concrete entry of the run's input delta, rendered into the interpretation
/// prompt with a stable id the what-changed rows cite as evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeltaEntry {
    pub id: String,
    pub label: String,
    /// For a fresh research-finding entry, the ledger condition the distillation
    /// tied it to (the validated `related_condition_id`) — the source-backed leg
    /// a qualitative tripped/fired claim needs, surfaced to the interpretation
    /// prompt as a research-supported mark on that condition
    /// (`docs/portfolio-workflow.md` §Step 6d, §Step 6g).
    pub related_condition_id: Option<String>,
}

/// The validated what-changed attribution, persisted with the holding's audit
/// (`docs/portfolio-workflow.md` §Step 6g): the post-validation rows (an
/// unresolvable external attribution downgraded to self-correction), the input
/// delta they resolved against, and the two signals outcome learning consumes.
/// `None` on debuts (nothing to attribute) and every early exit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhatChangedAudit {
    pub entries: Vec<WhatChangedEntry>,
    /// The rendered input-delta entries the external attributions resolved against.
    pub input_delta: Vec<DeltaEntry>,
    /// One line per validator adjustment (the logged reason): a downgraded
    /// external row, a dropped no-move row (`old` == `new`), or a dropped
    /// exact-duplicate row.
    pub downgrades: Vec<String>,
    /// Post-validation self-corrections this run (authored plus downgraded).
    pub self_correction_count: u32,
    /// The standing-thesis signal (`docs/portfolio-analysis.md` §Outcome
    /// learning): a resolved external thesis / scenario-weights row, or any
    /// self-correction.
    pub thesis_changed: bool,
}

/// The `what_changed_entries` array schema, per branch: the priced kinds cover the
/// intrinsic values; the role-risk branch swaps them for its role read.
fn what_changed_entries_schema(role_risk: bool) -> Value {
    let kinds: Vec<&str> = if role_risk {
        vec!["role-read", "scenario-weights", "thesis", "condition"]
    } else {
        vec![
            "grade",
            "sub-score",
            "conviction",
            "target",
            "horizon",
            "scenario-weights",
            "thesis",
            "condition",
        ]
    };
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": kinds },
                "detail": { "type": "string" },
                "old": { "type": "string" },
                "new": { "type": "string" },
                "attribution": {
                    "type": "string",
                    "enum": [
                        "market-data",
                        "company-information",
                        "research-narrative",
                        "self-correction"
                    ]
                },
                "evidence": { "type": "string" }
            },
            "required": ["kind", "detail", "old", "new", "attribution", "evidence"]
        }
    })
}

/// The model's grammar-constrained output (Ollama native `format`) — the only thing
/// the 122B authors at interpretation. The engine arm's numbers come from the
/// engine; since `portfolio-v7` this also carries the model arm's own numbers
/// (sub-scores, target bands — [`ModelView`]'s sources) beside the judgment calls
/// (conviction, horizon reads), the prose, the retrospective self-assessment, and
/// the rewritten thesis ledger. Since `portfolio-v9` it carries **no action** —
/// the per-holding action call authors that afterward ([`ActionDecision`]), so
/// this stage stays profile-blind. A schema-valid object is guaranteed by
/// grammar-constrained decoding, so there is no parse-and-pray path
/// (`docs/local-models.md §Schema-constrained output`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interpretation {
    pub conviction: Conviction,
    pub horizon_outlook: HorizonOutlook,
    pub financial_summary: String,
    /// The model's justification for the engine's base-case price target (it selects
    /// and explains; the engine computed the figure).
    pub price_target_rationale: String,
    pub what_changed: String,
    /// The typed what-changed rows beside the prose line ([`WhatChangedEntry`]) —
    /// one per moved intrinsic value, validated at the 6g seam against the
    /// rendered input delta. Empty on a debut ("new holding").
    #[serde(default)]
    pub what_changed_entries: Vec<WhatChangedEntry>,
    /// The rewritten thesis ledger — required; validated at the 6g seam.
    pub ledger: LedgerDraft,
    /// The model arm's four sub-scores (0–100, higher better) — its own read
    /// beside the engine's, never validated against them (the two-arm contract),
    /// gated to the declared scale at decode ([`validate_model_arm`]).
    pub model_sub_scores: SubScores,
    /// The model arm's freely-authored one- / twelve-month targets — no engine
    /// bound or clamp applies; each leg finite and positive by the same gate.
    pub model_price_targets: ModelPriceTargets,
    /// The retrospective self-assessment (see [`ModelView::self_assessment`]).
    pub self_assessment: String,
}

/// The ledger half of the interpretation schema (`docs/portfolio-analysis.md` §The
/// position thesis ledger), shared by both branches: the series and comparator
/// enums are structural, so a grammar-valid draft names only series the engine
/// actually computes (the app still validates the claim — defense in depth behind
/// the constraint). A `role_risk_only` ledger's trigger-family enum drops `add`
/// (its feasible set never offers the add family).
pub fn ledger_schema(role_risk: bool) -> Value {
    let series: Vec<&str> = engine::LedgerSeries::ALL.iter().map(|s| s.as_kebab()).collect();
    // The nullable series enum for a key driver's optional backing series.
    let mut series_or_null: Vec<Value> = series.iter().map(|s| json!(s)).collect();
    series_or_null.push(Value::Null);
    let quant = json!({
        "type": ["object", "null"],
        "properties": {
            "series": { "type": "string", "enum": series },
            "comparator": { "type": "string", "enum": ["below", "above"] },
            "threshold": { "type": "number" },
            "margin": { "type": "number" }
        },
        "required": ["series", "comparator", "threshold", "margin"]
    });
    let scenario = json!({
        "type": "object",
        "properties": {
            "conditions": { "type": "string" },
            "probability_pct": { "type": "number" }
        },
        "required": ["conditions", "probability_pct"]
    });
    let families: Vec<&str> = if role_risk {
        vec!["trim", "sell"]
    } else {
        vec!["add", "trim", "sell"]
    };
    json!({
        "type": "object",
        "properties": {
            "thesis": { "type": "string" },
            "key_drivers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "series": { "type": ["string", "null"], "enum": series_or_null }
                    },
                    "required": ["name", "series"]
                }
            },
            "bear": scenario, "base": scenario, "bull": scenario,
            "what_must_improve": { "type": "string" },
            "what_must_not_break": { "type": "string" },
            "falsifiers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "statement": { "type": "string" },
                        "quant": quant,
                        "technology_class": { "type": "boolean" },
                        "tripped": { "type": "boolean" }
                    },
                    "required": ["statement", "quant", "technology_class", "tripped"]
                }
            },
            "triggers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "statement": { "type": "string" },
                        "family": { "type": "string", "enum": families },
                        "quant": quant,
                        "fired": { "type": "boolean" }
                    },
                    "required": ["statement", "family", "quant", "fired"]
                }
            }
        },
        "required": [
            "thesis", "key_drivers", "bear", "base", "bull",
            "what_must_improve", "what_must_not_break",
            "falsifiers", "triggers"
        ]
    })
}

/// The fields the priced interpretation must return. The schema's `required` set and
/// the prompt's declaration are both built from this list, so the enforced grammar and
/// the stated contract cannot diverge. Three of these names — `conviction`,
/// `ledger`, `self_assessment` — also appear in the instructional prose above the
/// declaration, where a containment test cannot tell a real declaration from an
/// incidental mention (`docs/verification/2026-08-10-big-run-attempt-1.md` §Finding 2).
pub const INTERPRETATION_KEYS: [&str; 10] = [
    "conviction",
    "horizon_outlook",
    "financial_summary",
    "price_target_rationale",
    "what_changed",
    "what_changed_entries",
    "ledger",
    "model_sub_scores",
    "model_price_targets",
    "self_assessment",
];

/// The `role_risk_only` branch's fields, on the same shared-constant footing —
/// `ledger` is shadowed by that prompt's prose too.
pub const ROLE_RISK_KEYS: [&str; 4] =
    ["role_summary", "what_changed", "what_changed_entries", "ledger"];

/// The priced branch's response-contract sentence, generated from
/// [`INTERPRETATION_KEYS`]. The nested shapes are stated after the key list because
/// they are structure the model benefits from, not part of the top-level set.
///
/// The enforcement clause claims only what the decode path guarantees: the schema
/// grammar enforces the `required` keys and each value's shape, and an extra key is
/// **dropped** on decode (the schema sets no `additionalProperties: false`, and the
/// struct denies no unknown fields) — never rejected.
pub fn interpretation_response_contract() -> String {
    format!(
        "Respond with a single JSON object carrying exactly these keys: {}. \
         Within them: horizon_outlook is short / mid / long; model_sub_scores is \
         quality / valuation / momentum / risk; model_price_targets is one_month and \
         twelve_month, each base / bear / bull; what_changed_entries is a list of \
         typed rows (kind, detail, old, new, attribution, evidence), empty on a new \
         holding. The decoder's grammar enforces the \
         required keys and value shapes, and any key outside this set is dropped on \
         decode — so spend no reasoning on shape; put it into the read.",
        INTERPRETATION_KEYS.join(", ")
    )
}

/// The `role_risk_only` branch's contract, generated from [`ROLE_RISK_KEYS`] — the
/// same enforcement clause as [`interpretation_response_contract`], for the same
/// reason.
pub fn role_risk_response_contract() -> String {
    format!(
        "Respond with a single JSON object carrying exactly these keys: {}. The \
         decoder's grammar enforces the required keys and value shapes, and any key \
         outside this set is dropped on decode — so spend no reasoning on shape; put \
         it into the read.",
        ROLE_RISK_KEYS.join(", ")
    )
}

/// The JSON Schema handed to Ollama's `format` so the interpretation is structurally
/// valid by construction. Mirrors [`Interpretation`]'s shape; enums are string enums
/// with the same kebab labels serde uses, so the decoded object round-trips. Since
/// `portfolio-v7` the schema is **structurally unrestricted** (the two-arm
/// contract — `docs/portfolio-analysis.md` §The holding verdict): the conviction
/// enum lists all three values; the engine's evidence and any pre-profit
/// conviction ceiling render into the prompt as evidence and into the audit as
/// annotations, never as schema bars.
pub fn interpretation_schema() -> Value {
    let read = json!({ "type": "string", "enum": ["bullish", "neutral", "bearish"] });
    let convictions = vec!["high", "medium", "low"];
    // The model target band stays within the schema subset the local grammar
    // converter proves out (type / properties / required / enum) — the 0–100
    // sub-score scale and target positivity are stated in the prompt, never as
    // numeric range keywords the grammar cannot express. What the grammar
    // cannot express the app enforces at decode: [`validate_model_arm`] gates
    // the declared domain (Codex I6).
    let target = json!({
        "type": "object",
        "properties": {
            "base": { "type": "number" },
            "bear": { "type": "number" },
            "bull": { "type": "number" }
        },
        "required": ["base", "bear", "bull"]
    });
    json!({
        "type": "object",
        "properties": {
            "conviction": { "type": "string", "enum": convictions },
            "horizon_outlook": {
                "type": "object",
                "properties": { "short": read, "mid": read, "long": read },
                "required": ["short", "mid", "long"]
            },
            "financial_summary": { "type": "string" },
            "price_target_rationale": { "type": "string" },
            "what_changed": { "type": "string" },
            "what_changed_entries": what_changed_entries_schema(false),
            "ledger": ledger_schema(false),
            "model_sub_scores": {
                "type": "object",
                "properties": {
                    "quality": { "type": "number" },
                    "valuation": { "type": "number" },
                    "momentum": { "type": "number" },
                    "risk": { "type": "number" }
                },
                "required": ["quality", "valuation", "momentum", "risk"]
            },
            "model_price_targets": {
                "type": "object",
                "properties": { "one_month": target, "twelve_month": target },
                "required": ["one_month", "twelve_month"]
            },
            "self_assessment": { "type": "string" }
        },
        "required": INTERPRETATION_KEYS
    })
}

/// The model arm's numeric domain, enforced app-side at the interpretation
/// call's decode (`docs/portfolio-analysis.md` §The holding verdict; the
/// 2026-08-24 review's Codex I6, ruled 2026-08-29): each of the four sub-scores
/// finite within 0–100 inclusive, each of the six target legs finite and
/// strictly positive. The grammar cannot express range keywords
/// ([`interpretation_schema`]) and the engine's own sub-scores are clamped at
/// source, so without this gate a finite `10000` derived an ordinary A and a
/// zero or negative target persisted into the scoreboard. The gate is the
/// declared scale, never the engine's values — the two-arm contract's "never
/// validated against the engine" holds — and it rejects, never clamps.
/// Ordering is deliberately outside the domain: a band authored bear above
/// bull persists as authored and renders tagged (Codex I5), scoring reading it
/// as (min, max). The error names every offending field with its authored
/// value, never the first alone, so the failure detail reads the whole
/// response.
pub fn validate_model_arm(
    sub_scores: &SubScores,
    targets: &ModelPriceTargets,
) -> Result<(), ModelArmDomainError> {
    let mut violations = Vec::new();
    let axes = [
        ("quality", sub_scores.quality),
        ("valuation", sub_scores.valuation),
        ("momentum", sub_scores.momentum),
        ("risk", sub_scores.risk),
    ];
    for (axis, v) in axes {
        if !(v.is_finite() && (0.0..=100.0).contains(&v)) {
            violations.push(format!("model_sub_scores.{axis} = {v:?} (declared 0–100)"));
        }
    }
    let windows = [("one_month", &targets.one_month), ("twelve_month", &targets.twelve_month)];
    for (window, band) in windows {
        for (leg, v) in [("base", band.base), ("bear", band.bear), ("bull", band.bull)] {
            if !(v.is_finite() && v > 0.0) {
                violations.push(format!(
                    "model_price_targets.{window}.{leg} = {v:?} (declared a finite positive price)"
                ));
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(ModelArmDomainError { violations })
    }
}

/// Every model-arm value outside its declared domain in one response
/// ([`validate_model_arm`]), each entry naming the field and the authored value.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelArmDomainError {
    pub violations: Vec<String>,
}

impl std::fmt::Display for ModelArmDomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "model arm off its declared domain: {}", self.violations.join("; "))
    }
}

impl std::error::Error for ModelArmDomainError {}

/// The model's schema-constrained output for a **`role_risk_only`** holding — the
/// union's other branch (`docs/portfolio-analysis.md` §Intrinsic verdict): the role
/// read and the continuity note. None of the priced fields exist — no grade,
/// conviction, horizon, or target rationale — **and no action**: the branch's
/// action is authored by the dedicated per-holding action call afterward
/// ([`ActionDecision`]), the full ladder structurally open while the engine
/// arm's set stays the reduced [`ROLE_RISK_ACTIONS`], rendered as evidence
/// (`docs/portfolio-analysis.md` §Portfolio action).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleRiskInterpretation {
    /// The vehicle's mandate and the exposure it exists to supply (prose).
    pub role_summary: String,
    pub what_changed: String,
    /// The typed what-changed rows beside the prose line ([`WhatChangedEntry`],
    /// the branch's reduced kind set) — validated at the 6g seam. Empty on a
    /// debut.
    #[serde(default)]
    pub what_changed_entries: Vec<WhatChangedEntry>,
    /// The rewritten fund ledger — same sections, the branch's two reductions
    /// enforced at validation (condition-only monitor, trim / sell triggers).
    pub ledger: LedgerDraft,
}

/// The reduced action set — a `role_risk_only` holding's **engine set**, rendered
/// into the action call's prompt as the engine arm's evidence: the add family
/// requires return evidence this branch has none of by construction
/// (`docs/portfolio-analysis.md` §Portfolio action). The model's choice stays
/// structurally open (the full ladder), departures annotated on the audit.
pub const ROLE_RISK_ACTIONS: [Action; 3] = [Action::SellAll, Action::Trim, Action::Hold];

/// The JSON Schema for [`RoleRiskInterpretation`] — no action field (the branch's
/// action is authored by the per-holding action call, where the reduced set is
/// the engine arm's evidence and the model's choice is structurally open), and
/// the ledger's reduced trigger-family enum is structural.
pub fn role_risk_interpretation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "role_summary": { "type": "string" },
            "what_changed": { "type": "string" },
            "what_changed_entries": what_changed_entries_schema(true),
            "ledger": ledger_schema(true)
        },
        "required": ROLE_RISK_KEYS
    })
}

// ---- The per-holding action call (the profile's one entry point) --------------

/// The action call's grammar-constrained output — the **per-holding portfolio
/// action** with its one-line rationale (`docs/portfolio-analysis.md` §Portfolio
/// action). Authored by a dedicated stage after interpretation that reads the
/// finished intrinsic verdict, the holding's own sizing evidence, and the
/// **investor profile** — the profile's only entry point into the job, so the
/// intrinsic verdict stays profile-independent by input isolation. Rung only:
/// sizing is retired; the whole-book reconciliation is the future portfolio
/// planner's job. The action enum is structurally the full ladder on **both**
/// branches — the engine set renders as evidence, never a schema bar (the
/// two-arm contract).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDecision {
    pub action: Action,
    /// The one-line rationale for the chosen rung (persisted on the verdict).
    pub rationale: String,
}

/// The fields the action call must return — the same shared-constant footing as
/// [`INTERPRETATION_KEYS`], so the enforced grammar and the stated contract
/// cannot diverge.
pub const ACTION_KEYS: [&str; 2] = ["action", "rationale"];

/// The action call's response-contract sentence, generated from [`ACTION_KEYS`] —
/// the same enforcement clause as [`interpretation_response_contract`], for the same
/// reason.
pub fn action_response_contract() -> String {
    format!(
        "Respond with a single JSON object carrying exactly these keys: {}. The \
         decoder's grammar enforces the required keys and value shapes, and any key \
         outside this set is dropped on decode — so spend no reasoning on shape; put \
         it into the decision.",
        ACTION_KEYS.join(", ")
    )
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
    fn model_arm_domain_admits_the_scale_edges_and_an_inverted_band() {
        // 0 and 100 are on the scale; a tiny or huge finite positive price is a
        // price; bear above bull is I5's authored-and-annotated case, in-domain.
        let scores = SubScores { quality: 0.0, valuation: 100.0, momentum: 50.0, risk: 99.999 };
        let bands = ModelPriceTargets {
            one_month: ModelPriceTarget { base: 1e-9, bear: 500.0, bull: 50.0 },
            twelve_month: ModelPriceTarget { base: 1e300, bear: 1.0, bull: 2.0 },
        };
        assert!(validate_model_arm(&scores, &bands).is_ok());
    }

    #[test]
    fn model_arm_domain_rejects_every_off_scale_value_and_names_each() {
        // Every violation in one response is named with its authored value —
        // the failure detail reads the whole arm, never the first miss alone —
        // and the in-domain leg beside them is not.
        let scores = SubScores {
            quality: 100.0001,
            valuation: -0.0001,
            momentum: f64::NAN,
            risk: f64::INFINITY,
        };
        let bands = ModelPriceTargets {
            one_month: ModelPriceTarget { base: 0.0, bear: -1.0, bull: f64::NAN },
            twelve_month: ModelPriceTarget {
                base: f64::NEG_INFINITY,
                bear: 10.0,
                bull: f64::INFINITY,
            },
        };
        let err = validate_model_arm(&scores, &bands).unwrap_err();
        assert_eq!(err.violations.len(), 9, "{err}");
        let text = err.to_string();
        assert!(text.starts_with("model arm off its declared domain: "), "{text}");
        for needle in [
            "model_sub_scores.quality = 100.0001 (declared 0–100)",
            "model_sub_scores.valuation = -0.0001",
            "model_sub_scores.momentum = NaN",
            "model_sub_scores.risk = inf",
            "model_price_targets.one_month.base = 0.0 (declared a finite positive price)",
            "model_price_targets.one_month.bear = -1.0",
            "model_price_targets.one_month.bull = NaN",
            "model_price_targets.twelve_month.base = -inf",
            "model_price_targets.twelve_month.bull = inf",
        ] {
            assert!(text.contains(needle), "{needle} missing from: {text}");
        }
        assert!(!text.contains("twelve_month.bear"), "{text}");
    }

    /// Pins the read-only Settings payload for the fixed preset — the exact
    /// snake_case keys the frontend types against and the shared label strings
    /// (one label source with the per-holding action call's prompt).
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
                "tax": "tax-aware — the possible benefit of realizing a loss is weighed \
                        qualitatively; no tax-lot, holding-period, or rate modeling",
                "cash": "unconstrained — adds are never gated on observed Schwab cash",
            })
        );
    }

    /// A grammar-valid ledger object in the schema's own labels, for the
    /// interpretation round-trip tests.
    fn raw_ledger() -> Value {
        json!({
            "thesis": "Durable franchise at a fair multiple.",
            "key_drivers": [
                { "name": "margin trajectory", "series": "net-margin" },
                { "name": "platform stickiness", "series": null }
            ],
            "bear": { "conditions": "margins compress", "probability_pct": 20.0 },
            "base": { "conditions": "trajectory holds", "probability_pct": 55.0 },
            "bull": { "conditions": "growth re-accelerates", "probability_pct": 25.0 },
            "what_must_improve": "services mix",
            "what_must_not_break": "gross margin",
            "falsifiers": [{
                "statement": "TTM net margin falls below 15%",
                "quant": {
                    "series": "net-margin", "comparator": "below",
                    "threshold": 0.15, "margin": 0.01
                },
                "technology_class": false,
                "tripped": false
            }],
            "triggers": [{
                "statement": "Trim above the priced-in ceiling",
                "family": "trim",
                "quant": {
                    "series": "price", "comparator": "above",
                    "threshold": 150.0, "margin": 0.0
                },
                "fired": false
            }]
        })
    }

    #[test]
    fn interpretation_round_trips_through_its_schema_labels() {
        // The kebab labels the schema advertises are exactly what serde decodes, so a
        // grammar-valid model object deserializes into `Interpretation` cleanly.
        let raw = json!({
            "conviction": "high",
            "horizon_outlook": { "short": "neutral", "mid": "bullish", "long": "bullish" },
            "financial_summary": "Durable margins, light leverage.",
            "price_target_rationale": "Base case tracks the engine's DCF midpoint.",
            "what_changed": "new holding",
            "ledger": raw_ledger(),
            "model_sub_scores": { "quality": 88.0, "valuation": 35.0, "momentum": 70.0, "risk": 60.0 },
            "model_price_targets": {
                "one_month": { "base": 210.0, "bear": 195.0, "bull": 225.0 },
                "twelve_month": { "base": 260.0, "bear": 180.0, "bull": 320.0 }
            },
            "self_assessment": "First read; no prior call to assess."
        });
        let parsed: Interpretation = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.conviction, Conviction::High);
        assert_eq!(parsed.horizon_outlook.mid, HorizonRead::Bullish);
        // The model arm decoded exactly as authored — no bound or clamp applies.
        assert_eq!(parsed.model_sub_scores.valuation, 35.0);
        assert_eq!(parsed.model_price_targets.twelve_month.bull, 320.0);
        assert_eq!(parsed.self_assessment, "First read; no prior call to assess.");
        // The ledger draft decoded structurally: the machine-core claims arrive as
        // strings for app-side validation, never pre-trusted types.
        assert_eq!(parsed.ledger.falsifiers.len(), 1);
        let quant = parsed.ledger.falsifiers[0].quant.as_ref().unwrap();
        assert_eq!(quant.series, "net-margin");
        assert_eq!(quant.comparator, "below");
        assert_eq!(parsed.ledger.key_drivers[1].series, None);
    }

    #[test]
    fn ledger_schema_constrains_series_families_and_requires_the_ledger() {
        // Both interpretation schemas require the rewritten ledger, the quant series
        // enum is exactly the engine's closed executability surface, and the
        // role-risk trigger-family enum drops `add` (the reduced spine).
        let schema = interpretation_schema();
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(required.contains(&"ledger"), "{required:?}");
        // The model arm is schema-required in full (the two-arm contract): its own
        // sub-scores, both freely-authored target bands, and the retrospective.
        for field in ["model_sub_scores", "model_price_targets", "self_assessment"] {
            assert!(required.contains(&field), "{required:?}");
        }
        let bands = &schema["properties"]["model_price_targets"]["properties"];
        for window in ["one_month", "twelve_month"] {
            let req: Vec<&str> = bands[window]["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(req, vec!["base", "bear", "bull"]);
        }

        let ledger = &schema["properties"]["ledger"];
        let series: Vec<&str> = ledger["properties"]["falsifiers"]["items"]["properties"]["quant"]
            ["properties"]["series"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(series.len(), engine::LedgerSeries::ALL.len());
        assert!(series.contains(&"net-margin"));
        // Retired from the closed surface (the tunnel-vision ruling, 2026-08-14).
        assert!(!series.contains(&"portfolio-weight"));
        let families: Vec<&str> = ledger["properties"]["triggers"]["items"]["properties"]
            ["family"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(families, vec!["add", "trim", "sell"]);

        let role = role_risk_interpretation_schema();
        let role_required: Vec<&str> = role["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(role_required.contains(&"ledger"), "{role_required:?}");
        let role_families: Vec<&str> = role["properties"]["ledger"]["properties"]["triggers"]
            ["items"]["properties"]["family"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(role_families, vec!["trim", "sell"], "no add family on role_risk");
    }


    #[test]
    fn a_stamped_vintage_wins_over_the_run_date() {
        let stamped = json!({
            "symbol": "AAPL",
            "asset_class": "stock",
            "position_change": "unchanged",
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
    fn thesis_ledger_round_trips_with_eval_state() {
        let ledger = ThesisLedger {
            branch: LedgerBranch::Priced,
            original_thesis: "debut thesis".into(),
            current_thesis: "current thesis".into(),
            key_drivers: vec![KeyDriver {
                driver_id: "kd-margins".into(),
                name: "margins".into(),
                series: Some(engine::LedgerSeries::NetMargin),
            }],
            monitor: vec![MonitorScenario {
                scenario: ScenarioKind::Base,
                conditions: "holds".into(),
                probability_pct: 55.0,
                engine_target: Some(210.0),
            }],
            what_must_improve: "growth".into(),
            what_must_not_break: "margins".into(),
            conditions: vec![LedgerCondition {
                condition_id: "c-1".into(),
                role: ConditionRole::Falsifier,
                trigger_family: None,
                statement: "net margin below 15%".into(),
                quant: Some(QuantCore {
                    series: engine::LedgerSeries::NetMargin,
                    comparator: LedgerComparator::Below,
                    threshold: 0.15,
                    margin: 0.01,
                }),
                downgraded_reason: None,
                technology_class: false,
                tripped: false,
                supersedes: None,
                eval_state: Some(ConditionEvalState {
                    last_observation_id: Some("2026-06-30".into()),
                    last_value: Some(0.22),
                    last_evaluated_at: Some("2026-08-03".into()),
                    breach_streak: 0,
                    first_breach_at: None,
                    confirmed_at: None,
                    acknowledged_observation_id: None,
                    authored_statement_basis: None,
                    authored_equity_source: None,
                }),
            }],
            authored_band_relation: None,
        };
        let verdict = HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated { reason: "fixture".into() },
            thesis_ledger: Some(ledger.clone()),
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        };
        let s = serde_json::to_value(&verdict).unwrap();
        let back: HoldingVerdict = serde_json::from_value(s).unwrap();
        assert_eq!(back.thesis_ledger, Some(ledger));
    }

    #[test]
    fn interpretation_schema_lists_every_required_field() {
        let schema = interpretation_schema();
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for field in [
            "conviction",
            "horizon_outlook",
            "financial_summary",
            "price_target_rationale",
            "what_changed",
        ] {
            assert!(required.contains(&field), "schema must require {field}");
        }
        // The tunnel-vision contract: interpretation authors no action — the
        // dedicated action call owns it — and the conviction enum stays the full
        // three values (engine evidence annotates, never bars).
        assert!(schema["properties"].get("action").is_none());
        let convictions = schema["properties"]["conviction"]["enum"].as_array().unwrap();
        assert_eq!(convictions.len(), 3);
        // The action call's schema advertises the full ladder on every branch.
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
    fn role_risk_schema_carries_no_action_field() {
        // The branch's action is authored by the dedicated per-holding action
        // call — the 6f role/risk interpretation authors none.
        let schema = role_risk_interpretation_schema();
        assert!(schema["properties"].get("action").is_none());
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(!required.contains(&"action"));
        assert!(required.contains(&"role_summary"));
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
            role_summary: "Core fixed-income sleeve.".into(),
            exposure_tilt: vec![ExposureWeight { label: "United States".into(), weight: 0.97 }],
            expense_drag: Some(0.0003),
            observable_risk: Some(0.06),
            structural_flag: false,
            is_cef: false,
            nav_premium: None,
            evidence_gaps: vec!["valuation: no on-plan duration/credit surface".into()],
            action: Action::Hold,
            action_rationale: String::new(),
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
