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
#[cfg(test)]
mod fixed_evidence;
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
pub mod soft_forensic;
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

// ---- Investor profile --------------------------------------------------------

/// The configured investor profile that personalizes the *action* — never the
/// intrinsic verdict (`docs/portfolio-analysis.md` §Intrinsic verdict,
/// `docs/configuration.md` §Investor Profile). It reaches the model at the
/// **per-holding action call** only ([`ActionDecision`]): objective, risk
/// tolerance and horizon frame the rung there; tax posture permits only a
/// rationale caveat, never an action input. No other model call renders it.
/// It ships as the documented fixed preset
/// ([`InvestorProfile::default_fixture`]); the configurable Settings form is a
/// later slice — Settings shows the preset read-only via [`Self::display`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestorProfile {
    pub objective: ProfileObjective,
    pub risk_tolerance: RiskTolerance,
    pub horizon: ProfileHorizon,
    /// Whether the rationale may flag possible tax consequences as a user
    /// caveat, with no effect on the action. Actual account type, tax lots,
    /// holding periods and rates are unmodeled.
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
    /// taxable/tax-aware (an optional tax caveat, never an action input — no
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

/// One scenario price target with its methodology exposed (`docs/portfolio-analysis.md`
/// — "computed by the financial-analysis engine as scenario outputs with their
/// methodology and assumptions exposed"). The model selects and justifies the base
/// case; this engine-arm number is never model-authored (the model arm states
/// its own expected prices on the thesis appendix, [`ThesisAppendix`]).
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

/// The engine's bear / base / bull bands at three months, twelve months and
/// three years — **rolling windows from the run date**, not calendar ends
/// (outside January, calendar year-end is not twelve months away, and the
/// accuracy pass scores each band at its own horizon date —
/// `docs/portfolio-analysis.md` §Starting parameters). Each `None` when the
/// inputs to derive it were missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceTargets {
    pub three_month: Option<PriceTarget>,
    pub twelve_month: Option<PriceTarget>,
    pub three_year: Option<PriceTarget>,
}

/// The **typed appendix** of the thesis document — the model arm's only typed
/// fields (`docs/portfolio-analysis.md` §The holding verdict, the two-arm
/// contract): the conviction and the expected share price at three months,
/// twelve months and three years, transcribed from the document by the
/// conversation's second, non-thinking message and persisted exactly as
/// authored. Every field is `null` where the document states no value, and a
/// null is acted on by presence alone — a null price opens no model leg at
/// that horizon, a null conviction renders as none. The declared domain is
/// app-enforced at decode ([`validate_appendix_domain`]): each present price
/// finite and strictly positive, a present conviction one of its three values;
/// nothing else about the arm is checked — not the document against the
/// appendix, not the prices against the engine's bands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThesisAppendix {
    pub conviction: Option<Conviction>,
    pub expected_price_3m: Option<f64>,
    pub expected_price_12m: Option<f64>,
    pub expected_price_3y: Option<f64>,
}

impl ThesisAppendix {
    /// The appendix with every field null — the document's silence on every
    /// typed value (a `role_risk_only` holding never carries one at all).
    pub const NONE: Self = Self {
        conviction: None,
        expected_price_3m: None,
        expected_price_12m: None,
        expected_price_3y: None,
    };

    /// Whether every field is null — an appendix that opens no episode.
    pub fn is_empty(&self) -> bool {
        *self == Self::NONE
    }

    /// The three expected prices in horizon order with their horizon labels,
    /// null where the document stated none.
    pub fn expected_prices(&self) -> [(&'static str, Option<f64>); 3] {
        [
            ("three-month", self.expected_price_3m),
            ("twelve-month", self.expected_price_12m),
            ("three-year", self.expected_price_3y),
        ]
    }
}

/// The model arm of a priced verdict as the thesis-document conversation
/// returns it: the document as prose, read as text and validated by nothing,
/// and the typed appendix transcribed from it (`docs/portfolio-workflow.md`
/// §Step 6f). The engine arm is app-stamped beside it at assembly
/// ([`pipeline::graded_verdict_from_model_arm`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PricedModelArm {
    pub thesis_document: String,
    pub appendix: ThesisAppendix,
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
/// sub-scores, bands, tier, hurdle, options signal, its own rung) come from the
/// engine, app-stamped and never echoed through the model; the model arm — the
/// thesis document and its typed appendix — persists exactly as authored,
/// type-checked only, never validated against the engine (the two-arm contract,
/// `docs/portfolio-analysis.md` §The holding verdict); the action with its
/// rationale comes from the separate action call.
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
    /// The **thesis document** — the model arm's prose: the thesis, the key
    /// drivers, the bear / base / bull scenarios with their conditions and
    /// probabilities, the falsifiers and triggers in words, the expected share
    /// price at each horizon and the conviction argued in the text, and the
    /// summary paragraph. Read as text, validated by nothing; each run's
    /// document supersedes the prior run's, and it is where the holding's
    /// standing view lives (`docs/portfolio-analysis.md` §The holding verdict).
    pub thesis_document: String,
    /// The typed appendix transcribed from the document ([`ThesisAppendix`]).
    pub appendix: ThesisAppendix,
    pub price_targets: PriceTargets,
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
    /// The engine's own action rung — the drafted rule over its reads
    /// ([`engine::engine_action`]: the hard-forensic exit branch, dead money
    /// and the letter, the add admission, walked into the engine's own
    /// feasible set), app-stamped on the engine arm and shown to the action
    /// call as a computed read, never a recommendation
    /// (`docs/portfolio-analysis.md` §Starting parameters). The engine authors
    /// no conviction and no outlook.
    pub engine_rung: Action,
    /// Spot's relationship to the engine's twelve-month band at authoring —
    /// app-stamped on the engine arm at the checkpoint, so the quick check's
    /// band monitor fires on a *change* in the relationship, never on the
    /// standing state ([`BandRelation`]); `None` where no band or spot existed,
    /// or where an unresolvable split bridge withheld the stamp
    /// (`docs/portfolio-analysis.md` §The quick check).
    pub authored_band_relation: Option<BandRelation>,
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
/// Engine-computed fields (exposure, expense, risk, gaps) plus the model's
/// thesis document carrying the role read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleRiskVerdict {
    /// The deterministic classification label (e.g. "bond fund", "leveraged / inverse
    /// vehicle", "equity fund below the US-exposure guard").
    pub class_label: String,
    /// The branch's **thesis document**: the role read — the mandate and the
    /// exposure the vehicle exists to supply, read in isolation — the risks,
    /// the triggers for trimming or selling and the summary paragraph, as
    /// prose; no prices, no conviction and no appendix follow it
    /// (`docs/portfolio-analysis.md` §Intrinsic verdict).
    pub thesis_document: String,
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
    /// low-conviction guess. The holding's prior thesis document is retained
    /// unrewritten on the exit (`docs/portfolio-analysis.md` §Evidence floor:
    /// only a full pass writes a new one), so the next continuity run still
    /// reads it; `None` on a debut abstention or after a prior with no document.
    InsufficientEvidence {
        reason: String,
        prior_thesis_document: Option<String>,
    },
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

    /// The basis word a rendered ledger statement carries in parentheses on a
    /// flow series ([`QuantCore::render`]).
    pub fn short(&self) -> &'static str {
        match self {
            StatementBasis::Ttm => "TTM",
            StatementBasis::Annual => "annual",
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

/// Spot's relationship to the engine's twelve-month bear–bull band. Stamped onto
/// the engine arm at the checkpoint ([`GradedVerdict::authored_band_relation`])
/// so the quick check's `PriceOutsideBand` flag fires on a *change* in the
/// relationship, never on the standing state — a band authored with spot
/// already outside was an examined observation (the model wrote its document
/// seeing it), not news worth re-raising every sweep.
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
    /// How the position changed since the prior run — set by the app from the
    /// deterministic holdings diff ([`diff`]; `docs/portfolio-analysis.md` §What
    /// changed: the what-changed line carries the position delta), never authored by
    /// the model.
    pub position_change: PositionChange,
    pub disposition: VerdictDisposition,
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

impl HoldingVerdict {
    /// The thesis document this verdict carries — the analyzed branches' own,
    /// an abstention's retained prior document, and none on a not-rated
    /// position. The continuity read of the holding's standing view
    /// (`docs/portfolio-analysis.md` §Continuity and isolation).
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

    /// The typed appendix where the verdict carries one — the priced branch
    /// alone; a `role_risk_only` read and every exit carry none.
    pub fn appendix(&self) -> Option<&ThesisAppendix> {
        match &self.disposition {
            VerdictDisposition::Priced(g) => Some(&g.appendix),
            _ => None,
        }
    }
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
    /// Every resolved physical attempt retained by this run's checkpoint ownership,
    /// including adapter failures. Summary selections never discard these rows.
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
    /// Completed holdings whose persisted research audit carries at least one gap —
    /// partial gathering, evidence omission/truncation, or downstream distillation.
    /// Counted and named in the run-level summary, never an attention trigger: web
    /// research is additive and fail-soft, but its degraded coverage must be visible.
    pub research_degraded_holdings: usize,
    /// Total persisted research-gap entries across those holdings. This is folded
    /// from the typed `ResearchAuditRecord::gaps` field without matching gap prose.
    pub research_gap_count: usize,
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
    /// gaps above, including research coverage gaps).
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
    /// The four soft forensic flags ([`soft_forensic::SoftForensicFlags`]) beside
    /// the hard state — Altman Z, Piotroski, net income against operating cash
    /// flow, and the working-capital build — computed from `financial-scores`
    /// and the statements wherever the overlay record is computed (every
    /// priced-stock path, the engine-floor and guard exits included), each flag
    /// typed unevaluable on a missing input and never clear. `None` on funds and
    /// `role_risk_only` holdings.
    pub soft_forensic: Option<soft_forensic::SoftForensicFlags>,
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
    /// The research-loop audit record (`docs/storage.md §Local Analysis Suite
    /// Storage` — the research-derived artifacts): the write-ups as written,
    /// the disconfirming pass's write-up, the page roster, the budget spend
    /// and the gaps. `None` on every no-research exit (not-rated, the listing
    /// guard, an evidence-floor abstention).
    pub research: Option<research::ResearchAuditRecord>,
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
/// (`EngineView`, since retired) so every model field has a scored baseline counterpart.
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
/// The decode now rejects an off-domain response (`validate_model_arm`, since
/// replaced by [`validate_appendix_domain`] on the appendix)
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
/// leaked in as a soft sell-lean (Signal 2; since `portfolio-v37` every line
/// states the hurdle fact and that it neither requires nor forbids any rung,
/// the `fails` line joining under `portfolio-v38`). Both were the action prompt lagging
/// contracts the docs already state (`docs/portfolio-analysis.md` §Portfolio
/// action: the departure is app-stamped; indeterminate neither tilts the decision
/// nor creates dead money); prompt-prose only, no schema or other axis moves
/// (`docs/verification/2026-08-30-big-run-findings.md` §Finding 3). Attempt 4's
/// Finding 4 fix B splits the Step-6c research turn — the gathering turns carry
/// the tools with no grammar, and a separate synthesis call authors the findings
/// from a fresh, tool-history-free conversation carrying the grammar and no tools
/// (the tools-plus-`format` interleaving had left the terminal turn emitting empty
/// or fenced bodies) — rewording both the gathering and synthesis prompts, so it
/// moves to v33 so an interrupted pre-fix run cannot resume into the new synthesis
/// contract (`docs/verification/2026-08-31-big-run-attempt-4-findings.md` §Finding 4).
/// The post-landing review of fix B (2026-08-31 §Post-landing review) reworded the
/// synthesis brief again — dropping empty-body pages from the evidence, leading each
/// source with its extracted title, prepending a gathering-degradation note, and
/// bounding the pass prefix (per-claim, ledger-block, follow-up, and a head-cap so
/// neither the gathering request nor the synthesis prefix can exceed the input
/// guard) — which changes the synthesis input and so a completed holding's analysis,
/// so it moves to v34. The final pre-debut sweep also bounds the aggregate growing
/// gather packet and per-turn tool batch, and jointly selects evidence headers and
/// usable bodies so omitted headers are reclaimed rather than starving every page;
/// those corrections fold into the same never-run v34 contract. The resume contract,
/// `job::resume_eligibility`, refuses a resume across changed synthesis semantics
/// rather than mixing them.
///
/// `portfolio-v35`: the Step-6c synthesis prompt shows the model the findings
/// object's shape — exact keys, types, required members, and a terse
/// placeholder-valued example pinned to `findings_schema` by test — in place of
/// telling it only that "your output grammar" exists. The `format` grammar is a
/// decoding mask the model never sees; attempt 5's PSX trace showed the model
/// resolving "JSON or Markdown?" toward a hand-built Markdown block while
/// planning its content, and the topic worked under that confusion dropped whole
/// at reconciliation (attempt-5 Finding 5,
/// `docs/verification/2026-09-01-big-run-attempt-5-findings.md`). Before v35's
/// debut, the joint prompt review also adds pass-local source-id citations
/// (resolved to the existing persisted URLs), dedicated synthesis orientation,
/// schema-derived nested examples, bounded original text for distillation
/// extraction, explicit daily ledger volatility and spot, branch-specific
/// action facts, and validated continuity evidence. These fold into the unrun
/// v35 contract; persisted shapes and the other version axes are unchanged.
/// The pre-debut bundle also names option-overlay funds in the role/risk prompt.
///
/// `portfolio-v36`: the ledger-conditions and action-packet slice off attempt 6
/// (`docs/verification/2026-09-16-ledger-conditions-and-action-packet.md`).
/// The interpretation prompts render a holding-scoped ledger-authoring contract
/// — only the series the engine computes for the vehicle kind, each with its
/// unit, current observation and confirmation cadence, plus two worked
/// examples — and the 6g seam validates prose-versus-core agreement (unit,
/// comparator, metric, basis, level, margin, qualifier), downgrading a
/// disagreeing core to qualitative with a class-prefixed reason. The action
/// call reads an investment-only packet: no cost basis, unrealized P/L, tax
/// row, quantity or market value; the engine set stated once as evidence; the
/// score polarity spelled out on both arms; the overlay rendered as structure
/// and ratios; the tax caveat app-appended after the rung on an exit-family
/// choice. Prompt prose and the model-facing contract change, so a v35 trail
/// cannot resume into v36; the persisted shapes and every other axis are
/// unchanged.
///
/// `portfolio-v37`: the ledger-validator follow-up off the §8.2 live read
/// (`docs/verification/2026-09-16-ledger-validator-follow-up.md`). The 6g seam
/// downgrades a statement naming no figure as `no-level` on every series,
/// resolves a percent-from-current price level against the spot, keeps a
/// duration that names the market-data cadence exactly, no longer reads
/// "confirming …" or a unit-restating parenthetical as a second condition, and
/// caps the margin at a share of the level; the authoring contract states that
/// the threshold is the level the sentence names and the margin the separate
/// band; the action packet's capital-efficiency line states the hurdle fact
/// and that it neither requires nor forbids any rung. Prompt prose and the
/// validator's meaning change, so a v36 trail cannot resume into v37; the
/// persisted shapes and every other axis are unchanged.
///
/// `portfolio-v38`: the §3 interpretation slice (fix list 3.1–3.3, 1.9 and 2.5;
/// `docs/verification/2026-09-16-interpretation-slice.md`). The priced
/// verdict's `price_target_rationale` becomes `model_target_rationale`, the
/// model explaining its own bands (a persisted-shape change — `checkpoint-v10`,
/// portability format v7). Every model-facing packet opens with an
/// identity-and-spot header — the interpretation, role/risk and research
/// packets stop carrying quantity, cost basis and market value, the position
/// line states its direction only, and the option overlay renders unsized on
/// every packet. The ledger schema's series enum is scoped to the vehicle
/// kind; a debut requests neither continuity field and the app writes both;
/// the continuity contract states the two fields' relation once; the template
/// gains notes for the ledger's numeric fields. The authoring contract asks
/// for the level in the sentence, and the action packet's `fails` line states
/// the hurdle fact, the sunk-cost lean and its reach. A v37 trail cannot
/// resume into v38 on either the prompt or the checkpoint axis.
///
/// `portfolio-v39` is the residue slice off the v38 read and the 2026-09-17
/// Codex churn analysis (fix list 3.5–3.14; the record is
/// `docs/verification/2026-09-17-residue-slice.md`): the cadence exemption
/// refuses a period adjective on the unit; the ledger contract states the
/// margin's purpose and caps and the key-driver null rule; the interpretation
/// score line glosses every axis; both response contracts close with no code
/// fence or surrounding prose; the action packet's capital-efficiency lines
/// name the engine's twelve-month total-return test and the hurdle rate, its
/// ACTION BASIS defines the grade, the low-confidence letter is stated apart
/// from conviction, and the financial summary is labelled model-authored. No
/// persisted shape changes: the checkpoint stamp stays `checkpoint-v10`. A v38
/// trail cannot resume into v39 on the prompt axis.
/// `portfolio-v40` is the interpretation-prompt rewrite (ruled 2026-09-17 off
/// the v39 read and the user's reading of the rendered prompts): one message
/// in two parts — inputs, each section explained once and then its values,
/// then the task in output order — with no app concept in it (no arms,
/// baselines, stages, seams, validator behaviour or stamps), the margin caps
/// unshown and sized by example, the capital-efficiency read left to the action
/// call, and a placeholder-only return shape; the optional input sections lose
/// their narration on every packet they render into. No persisted shape
/// changes: the checkpoint stamp stays `checkpoint-v10`. A v39 trail cannot
/// resume into v40 on the prompt axis.
/// `portfolio-v41` is the action-prompt rewrite on the same principle (ruled
/// 2026-09-17; `docs/verification/2026-09-17-action-prompt-rewrite.md`): one
/// message in two parts, the system prompt the role line and the output
/// names; SCORES, PRICE TARGETS and SUPPORTED ACTIONS as computed / analyst
/// data with no arm, evidence or permission sentence; the capital-efficiency
/// read as the three tested returns and the hurdle rate with no state word;
/// the analyst's target rationale, thesis and scenario rows added; the
/// targets' method clauses in place of the provenance label; the overlay's
/// and the forensic sweep's consequence lines off the action packet; the
/// weighing order, the profile tie-break and the sunk-cost rule as task
/// clauses; the firmness clause with a chosen prior only; the harness's
/// Facts form removed. No persisted shape changes: the checkpoint stamp stays
/// `checkpoint-v10`. A v40 trail cannot resume into v41 on the prompt axis.
/// `portfolio-v42` is the role/risk interpretation-prompt rewrite on the same
/// principle (ruled 2026-09-17;
/// `docs/verification/2026-09-17-role-risk-prompt-rewrite.md`): one message in
/// two parts on the interpretation's frame, the system prompt the role line and
/// the output names; CLASS with the reported asset class, EXPOSURE TILT, RISK
/// PROFILE, EVIDENCE GAPS, the shared FINANCIAL METRICS and MARKET ANALYSIS
/// with the stances, and on continuity PRIOR ANALYSIS with the prior role read;
/// the shared ledger item with trim and sell families and the fund threshold
/// example and driver clause on both fund variants; a placeholder-only return
/// shape; the engine's evidence-gap strings and the shared PRICE VS NAV line
/// reworded as data on every surface that renders them. No persisted shape
/// changes: the checkpoint stamp stays `checkpoint-v10`. A v41 trail cannot
/// resume into v42 on the prompt axis.
/// `portfolio-v43` is the research gathering and synthesis prompt rewrite on
/// the same principle (ruled 2026-09-17;
/// `docs/verification/2026-09-17-research-prompt-rewrite.md`): both calls one
/// message in two parts with the role-line system prompt; the shared holding
/// header closes with the analysis date on every packet; the gathering
/// message with the topic, the follow-up, the claims so far, the standing
/// conditions and prior findings, the news leads and the tool-results gloss,
/// then the task with the source-weighing clause, the per-reply tool-call
/// bound and the stopping rule; the synthesis message with the searching note
/// in plain words and the evidence glossed once (the tier scale stated, the
/// published date shown, recency unshown), then the task and a
/// placeholder-only shape of findings, claims and the follow-up (findings and
/// claims on the disconfirming pass, whose grammar carries no follow-up); the
/// topic-answered, material-forward-fact and model-attributed seed fields
/// gone from the grammar, the wire and the pass record; the no-page pass
/// app-assembled; the fund exposure topic `fund-exposure-profile` (fix list
/// 4.4). The pass record's shape changes in the audit JSON only: the
/// checkpoint stamp stays `checkpoint-v10` and portability format 7 stands. A
/// v42 trail cannot resume into v43 on the prompt axis.
/// v44 (2026-09-17): the four distillation prompts — the pass, tier-1,
/// tree-reduce and reduce calls, the last pre-v40 shape on the Portfolio
/// local-model surface — are one message in two parts behind a role-line
/// system prompt, with no app concept in them: the shared holding header
/// with the date, STANDING CONDITIONS and KEY DRIVERS with their ids, TOPICS
/// as searches, claims and dated prior findings with no retrieval timestamp,
/// CONTRARY EVIDENCE, SOURCE TEXT with each page's publication date; the
/// task in output order with one object per topic required and a
/// placeholder-only shape whose alternatives — the topic keys, the condition
/// ids, the driver ids — ride the grammar; the typed fields on a stock's
/// call only, the forward figure narrowed to EPS or revenue from guidance, a
/// contract or a filing, the leading indicator only where key drivers
/// render, the backfill record only where the obligation bound, and
/// `conflict_handling` and the three side-channel confidences dropped (the
/// persisted audit's typed shapes change: `checkpoint-v11`; portability
/// format 7 stands). The two lines a validated typed field puts under
/// RESEARCH SUMMARY are data. A v43 trail cannot resume into v44 on the
/// prompt axis.
///
/// `portfolio-v45` (the rendered-ledger slice, 2026-09-18): a quantitative
/// condition is authored as fields plus a short name and the app renders its
/// statement from the core, so the prose-versus-core checks are gone; a new
/// or superseding core that already holds on the authoring surface is refused
/// (`holds-at-authoring`); the persisted condition gains `label`
/// (`checkpoint-v12`, portability format 8). A v44 trail cannot resume into
/// v45 on either axis.
/// `portfolio-v46` (2026-09-18): gathering sees bounded raw pages already
/// retrieved for the holding and a remaining-reply count refreshed per turn.
/// The model judges unanswered questions; no question-status wire is added.
/// Source snapshots remain transient; that slice kept checkpoint-v12 and portability 8.
/// Entry 7 later moves telemetry alone to checkpoint-v13 and portability 9.
/// A v45 trail cannot resume into v46 on the prompt axis.
/// `portfolio-v47`: distinct claim dates and evidence-reference reconciliation.
/// `portfolio-v48`: append-only gathering countdown and roots-first research.
/// Shared delivery stamp with the separately implemented attempt-7 prompt slice.
/// `portfolio-v49` (attempt-8 Slice A, 2026-09-27): the synthesis task's first
/// item says findings is written first and never left empty, the shape's
/// fact-period value placeholder lists one format per kind, the fiscal gloss
/// drops "exact", and the action packet's SUPPORTED ACTIONS line names its
/// list as complete. Shared delivery stamp with the separately implemented
/// attempt-8 latency slice (message order). The checkpoint trail is unchanged.
/// `portfolio-v50` (prompt read-through, 2026-09-28): the gathering brief's
/// Part 1 drops its TOOL RESULTS legend — the two tool descriptions state
/// what a search result and a fetched page carry, the tier scale's range
/// (0 to 5) beside its endpoints included — the synthesis EVIDENCE gloss
/// states the same range, and the gathering task's item 1 reads the pages
/// under PAGES ALREADY RETRIEVED only where one is shown, asking a brief
/// with none to search first, and names the news leads as fetch candidates
/// beside the search results under the one relevance test; the gathering
/// system message names the two tools, web_search and web_fetch; the fetch
/// tool's description states the extraction-quality range (0 to 1) and the
/// stub flag in plain words, and the fallible-source clause moves from it
/// onto Part 2's weighing sentence; the page header's subject field reads
/// `trusted on`, named in the fetch description and the EVIDENCE gloss as
/// the subjects the source is trusted on, within which its tier holds. The
/// checkpoint trail is unchanged.
/// `portfolio-v51` (prompt read-through, file 02, 2026-09-28): the follow-up
/// pass's opening is two sentences — the second says the TOPIC questions are
/// what the FOLLOW-UP question serves and that the pass does not search them,
/// the CLAIMS SO FAR clause riding it where claims render — and its items 1
/// and 3 name the FOLLOW-UP question where the other passes say "the
/// questions"; the claim provenance line reads `published: …; fact period: …`
/// on every surface that renders it (the gathering brief, the seed's prior
/// findings and the distillation prompts), the follow-up pass's CLAIMS SO FAR
/// gloss naming the two as the publication date the search or lead reported
/// and the period the fact covers; the reply countdown states that pages
/// fetched on the last reply are kept. The checkpoint trail is unchanged.
/// `portfolio-v52` (prompt read-through, file 03, 2026-09-29): the source-
/// quality value is named `source tier` on every surface the model sees —
/// the two tool descriptions, each search result line, each page header in
/// gathering and in the synthesis EVIDENCE block, the weighing sentences and
/// the EVIDENCE gloss — so the word carries its object; the gathering
/// weighing sentence prefers a source tier nearer 0 and an extraction
/// quality nearer 1, stated by the scales' endpoints in the words the
/// results carry; the subject-tier relation leaves the fetch description's
/// bracket and the EVIDENCE gloss and rides the task of both calls as its
/// own sentence, "A source tier applies to the subjects the source is
/// trusted on." The checkpoint trail is unchanged.
/// `portfolio-v53` (file 03, 2026-09-29): that sentence is dropped from both
/// calls — the header's `source tier N | trusted on …` fields carry the
/// relation, and the sentence only restated their link. The checkpoint
/// trail is unchanged.
/// `portfolio-v54` (file 03, 2026-09-29): a prior claim under PRIOR FINDINGS
/// takes the CLAIMS SO FAR shape — the claim and its source, then
/// `published: …; fact period: …` under them — through one renderer the
/// rendered example shares, and the gloss names the two fields; the
/// fact-period gloss reads "the period the fact applies to" on every surface
/// (CLAIMS SO FAR, PRIOR FINDINGS, the synthesis claims item, the
/// distillation date rule); the continuity clause names the headings it
/// draws on, PRIOR FINDINGS and STANDING CONDITIONS, only those the brief
/// shows. The checkpoint trail is unchanged.
/// `portfolio-v55` (prompt read-through, file 04, 2026-09-29): the
/// disconfirming pass's opening is two sentences — what to find on the
/// question under TOPIC, then that the claims under CLAIMS SO FAR are what
/// that question tests, searched for evidence against and not for — and its
/// items 1 and 3 say "the question", the brief carrying one; its CLAIMS SO
/// FAR gloss names the two provenance fields in the follow-up pass's words.
/// The checkpoint trail is unchanged.
/// `portfolio-v56` (prompt read-through, file 05, 2026-09-29): the PAGES
/// ALREADY RETRIEVED gloss says each page is shown as web_fetch returns it,
/// so the header fields point at the fetch description that defines them.
/// The checkpoint trail is unchanged.
/// `portfolio-v57` (prompt read-through, file 06 and the agenda, 2026-09-29):
/// the research topics' questions drop their filler words — "actually" (the
/// fund exposure and stock results topics), "genuinely" (forward-thematic and
/// technology), "exactly" and "real" (technology), "real" (forward-thematic)
/// — the questions otherwise unchanged. The checkpoint trail is unchanged.
/// `portfolio-v58` (prompt read-through, file 07, 2026-09-29): a tool result
/// never carries the operator's error text. A failed search returns one
/// fixed sentence; a failed fetch returns one of five chosen by the
/// failure's typed class (the site's HTTP answer with its status, an address
/// the app does not fetch, a page that could not be read, an invalid
/// address, no answer), a remembered failure replaying its class; the raw
/// error rides the run tracker's request row. A search whose results all
/// fall to the rank-time filter is an empty answer — "No results.", counted
/// as empty, not failed. The checkpoint trail is unchanged.
/// `portfolio-v59` (prompt read-through, file 08, 2026-09-29): the synthesis
/// EVIDENCE gloss names the TOPIC heading its pages were retrieved for (the
/// order unchanged, for the cache) and states the page header's fields in
/// the fetch description's words and shape — a semicolon list with each
/// explanation bracketed, in the header's order — with the stub flag glossed
/// as on the fetch description; the fallible-source clause moves from the
/// gloss onto the task's weighing sentence. The synthesis brief carries no
/// SEARCHING note: the synthesis is a fresh conversation that never saw
/// which search or fetch served which question, so a note of aggregate
/// losses could only be guessed onto a gap; the degradation stays a
/// persisted data-health gap, and item 1 ends at what the evidence leaves
/// unanswered. The task's items are plain sentences: "findings. Write this
/// first and never leave it empty." then the pass's subject, the figure
/// clause ("Where a page gives a figure, quote it with the date or period
/// the page gives for it"), disagreement and what stays unanswered; "claims.
/// The statements the findings rest on, one statement per claim, each
/// stated by a page in EVIDENCE" with source_id and fact_period each
/// defined where named, the kinds a semicolon list with each format
/// bracketed, "end is null for every kind but range"; the headings keep
/// their dashes; the preamble says once that the names below are the
/// object's fields, keeping the no-fence clause (fix list 3.7: it cuts the
/// model's thinking-time deliberation, which the grammar cannot); and the
/// shape shows a string-or-null field's both alternatives inside quotes
/// (`"end":"<YYYY-MM-DD|null>"`, `"followup_question":"<question|null>"`,
/// `"followup_rationale":"<why|null>"`). The checkpoint trail is unchanged.
/// `portfolio-v60` (prompt read-through, file 09, 2026-09-29): the follow-up
/// pass points at its headings as every other pass does, never with a
/// heading as an adjective. The synthesis subject reads "For the question
/// under FOLLOW-UP, state what EVIDENCE shows.", the topic pass's
/// construction. The gathering opening reads "Find what the web shows on the
/// question under FOLLOW-UP for this holding, as of the date under HOLDING.
/// The questions under TOPIC are what that question serves; this pass does
/// not search them, and the claims under CLAIMS SO FAR need no second
/// search.", and items 1 and 3 name "the question under FOLLOW-UP". A topic's
/// last pass under the depth cap asks for no follow-up proposal, as the
/// disconfirming pass: a proposal there could never be spent, so its system
/// message names findings and claims alone, its task has no follow-up item,
/// and its shape and grammar carry no follow-up fields
/// (`PassContext::offers_followup`, one depth test with the scheduler). The
/// checkpoint trail is unchanged.
/// `portfolio-v61` (prompt read-through, file 10, 2026-09-29): the
/// disconfirming synthesis's CLAIMS SO FAR gloss names the two provenance
/// fields its lines carry, in the gathering brief's words and without the
/// source those lines do not show ("What this run's research established on
/// the holding, each with the publication date the search or lead reported
/// and the period the fact applies to."), and its EVIDENCE gloss names "the
/// question under TOPIC", the one its topic holds; that synthesis opens on
/// its own system message, so the wording costs no shared prefix. The
/// checkpoint trail is unchanged.
/// `portfolio-v62` (prompt read-through, file 11, 2026-09-29): on every
/// object-returning call — synthesis, distillation, interpretation, role/risk
/// and action — the system message names the outputs before the two-part
/// frame, and the frame says where their definitions and shape sit: "You
/// will return …, as one JSON object. Part 1 of the message gives the
/// inputs. Part 2 defines those outputs and gives the shape to return."
/// (`TWO_PART_FRAME`, shared by the five builders). Before, the frame came
/// first and said Part 2 "states what to determine from them and the shape
/// to return", which the next sentence then stated. The gathering call
/// keeps its own frame. Every system message names the outputs by the
/// object's keys (the distillation and synthesis named them in English). On
/// the distillation prompts: the TOPICS gloss names the fields each claim
/// line carries (the address, its reference, the publication date the
/// search or lead reported and the period the fact applies to) and CONTRARY
/// EVIDENCE points at that form; the task opens in the synthesis's words
/// ("in the shape under RETURN SHAPE, … the names below are its fields");
/// the claims items read "one statement per claim"; and the claim rules sit
/// under their own heading, CLAIM RULES, which the items point at by name in
/// place of "as described below". The checkpoint trail is unchanged.
/// `portfolio-v63` (prompt read-through, file 11, item 1, 2026-09-29): every
/// claim line a distillation message shows sits under a pass-local id — `C1`,
/// `C2`, … in render order, restarting in every message — and a returned
/// claim cites that id as `evidence_id`, an enum of the ids the message
/// showed, in place of copying the 64-hex evidence reference and the page's
/// address (attempt 8, W9: 12 claims dropped on a mis-copied reference or
/// address). The app resolves the id to the reference and address it
/// rendered (`distill::ClaimIndex`, the retention allow-set), so the claim
/// object is `claim`, `evidence_id` and the tie; the reference no longer
/// renders; the TOPICS gloss names the id first; item 2 defines evidence_id
/// where it names it and CLAIM RULES loses the reference-copying sentence.
/// The typed items still cite pages by address. The persisted claim carries
/// no reference (it is derived at match time), so the checkpoint trail is
/// unchanged.
/// `portfolio-v64` (prompt read-through, file 11, 2026-09-30): the TOPICS
/// gloss names each topic's heading as its key and its title, and item 2
/// points at it ("topic_key is the topic's key under TOPICS") where before it
/// read "the key as shown", a key nothing in Part 1 named; the claims item
/// is one sentence per rule — what claims is; on a continuity run, where its
/// statements come from; what evidence_id is ("evidence_id is the id of the
/// claim under TOPICS or CONTRARY EVIDENCE the statement rests on"); then
/// that a fact two topics state is one claim — where one sentence carried
/// them all, and the tier-1, pass-level and tree-level items split the same
/// way; the CLAIMS SO FAR, PRIOR FINDINGS and TOPICS glosses state a line's
/// fields as one colon-introduced semicolon list ("Each claim carries: its
/// source; the publication date the search or lead reported; and the period
/// the fact applies to."), the shape the synthesis EVIDENCE gloss took at
/// `portfolio-v59`, where "each with a, b, and c" ran two lists on one
/// comma; the distillation's other lists carry the serial comma; and the
/// shared placeholder renderer shows a nullable
/// scalar's both halves ("<0|null>" on stated_low and stated_high), as the
/// synthesis shape shows a string-or-null field's since `portfolio-v59`. The
/// rendered examples' claim lines carry the publication dates and fact
/// periods a run renders (a fixture change, no prompt change). The
/// checkpoint trail is unchanged.
/// `portfolio-v65` (prompt read-through, file 12, 2026-09-30): the
/// distillation names the current analysis's side without "this time", a
/// phrase Part 1 defined only by contrast with the gloss's "an earlier
/// analysis": item 2's sources sentence reads "The statements come from the
/// searches and the prior findings" — "the searches" is the gloss's word for
/// the Search blocks, bare as item 1's "what the searches left unanswered"
/// and the summary sentence already use it, and the tier-1 item reads the
/// same — and a topic with prior findings only is "not searched in this
/// analysis" on the TOPICS gloss, the topic heading and item 2's two
/// sentences, so the two sides ride one noun. Item 4 asks for a measure
/// whose latest change "confirms" a driver, the word the field and the cap
/// rule use, where it read "bears on" (the engine reads the id alone, so a
/// measure turning against the driver still lifted the cap), and for the
/// driver's id alone — the model-authored name (`confirms_driver`) is gone
/// from the schema, the struct and the rendered line, the app resolving the
/// name from the id, so the persisted indicator's shape moves the trail to
/// `checkpoint-v16` and the archive to format v12; metric_name carries a
/// gloss. Where a topic rides dormant the one-claim rule binds the searched
/// topics ("A fact two searched topics state is one claim"), so the dormant
/// rule's kept copy no longer collides with it. Item 6 glosses confidence's
/// referent ("that the excerpt states that metric, value and period" — the
/// engine's selection among competing rows reads it); item 7 states
/// coverage's denominator ("complete where all four periods are found,
/// partial where fewer") and the checked periods' date form; item 1 drops
/// "prior findings assessed by the same rules", item 2 binding every
/// statement to CLAIM RULES already. Files 12, 14, 16 and 17 regenerated.
/// `portfolio-v66` (prompt read-through, file 14, 2026-09-30): the tier-1,
/// pass-level and tree-level calls each carry one topic, as their system
/// message says, so on those three the TOPICS gloss opens on that scope ("The
/// research on one topic of this holding, headed by its key and its title")
/// and CLAIM RULES closes on the call's own outputs ("Apply the same
/// resolution in the summary and the claims."), where both sentences were
/// the reduce's ("one topic at a time, each headed by …"; "in the combined
/// findings, summaries, and every topic's claims") and pointed at outputs
/// the call never returns. The reduce keeps both. Files 14, 15 and 16
/// regenerated. The checkpoint trail is unchanged.
/// `portfolio-v67` (prompt read-through, file 15, 2026-09-30): the pass-level
/// call shows one of the topic's searches, so its TOPICS gloss says so ("what
/// one of its searches established, then its claims"), pairing with its items'
/// "this search" where the gloss read "what its searches established" over one
/// Search block; and its summary item reconciles ("where two claims cover the
/// same fact, reconcile them by the rules under CLAIM RULES"), since one search
/// fetches several pages, so CLAIM RULES is pointed at on every distillation
/// call where before it sat on this one unreferenced. File 15 regenerated. The
/// checkpoint trail is unchanged.
/// `portfolio-v68` (the engine arm at three horizons, 2026-10-07): the engine
/// arm's bands are three-month / twelve-month / three-year, so COMPUTED PRICE
/// TARGETS prints the three legs — the three-month line without a method as
/// the one-month line was, the three-year line with its extrapolation clause —
/// and the action packet's computed legs likewise; the engine stand-in arm is
/// gone, so PRIOR ANALYSIS's prior computed read names the three bands, the
/// risk tier, the capital-efficiency state and the engine's own action rung in
/// place of the stand-in's conviction, outlook and action, and the matured
/// scored-window lines leave it with the suspended scoreboard; the hard
/// forensic rule sentence reads the exit family, no conviction cap; the
/// pre-profit section loses its conviction-ceiling line; the action packet's
/// computed SCORES line carries the engine's own rung as a computed read; the
/// three-year method clause names its floor widening. The persisted verdict
/// moves the trail to `checkpoint-v17` and the archive to format 13.
/// `portfolio-v69` (the engine arm, task 2 — the soft forensic flags and the
/// statement-only severe rule): the overlay's execution leg has no producer, so
/// the PRE-PROFIT EXECUTION AND FINANCING section loses its guidance-attainment
/// line and its severe-deterioration line drops the "(conjunctive)" label; the
/// soft flags are computed and persisted beside the hard-forensic record and
/// render on no prompt yet. The persisted audit moves the trail to
/// `checkpoint-v18` and the archive to format 14.
/// `portfolio-v70` (the holding verdict, task 1 — the thesis document and the
/// typed appendix): the structured interpretation is replaced by the
/// thesis-document conversation — a thinking message with no grammar that
/// writes the document over HOLDING, FETCHED VALUES, COMPUTED, MARKET
/// ANALYSIS, ANALYSIS and on a continuity run PRIOR THESIS verbatim, then a
/// non-thinking appendix message under the four-key nullable grammar; the
/// `role_risk_only` message is the first message alone; the soft forensic
/// flags render for the first time; the action packet renders VERDICT (the
/// conviction, the three expected prices and the document verbatim) in place
/// of the analyst SCORES row, the analyst PRICE TARGETS rows, TARGET
/// RATIONALE, CONVICTION AND OUTLOOK, FINANCIAL SUMMARY, THESIS, SCENARIOS,
/// PRIOR ANALYSIS and CHANGES SINCE THE PRIOR ANALYSIS; the ledger, the
/// what-changed audit, the retrospective and the input delta leave every
/// prompt. The persisted verdict moves the trail to `checkpoint-v20` and the
/// archive to format 16.
/// `portfolio-v71` (the holding verdict, task 2 — the action call): the action
/// packet takes the docs' shape — HOLDING; POSITION (the shares held, the cost
/// basis, the market value, the unrealized gain or loss and the change since
/// the last pull — the one packet that sees the position's economics); on a
/// priced holding VERDICT then one COMPUTED heading with labelled sub-blocks
/// (COMPUTED ACTION, GRADE without the sub-scores or the tier, PRICE BANDS
/// with the analyst's expected price beside each horizon, CAPITAL EFFICIENCY,
/// the forensic, overlay, option-overlay and commodity sections where they
/// render); on a continuity run PRIOR ACTION with the prior rationale less
/// the caveat, a rule-demoted prior carrying none; the two-reads preamble and
/// the provenance suffixes gone; Part 2 weighs VERDICT and COMPUTED first,
/// POSITION among the refining inputs. No persisted shape moves: the trail
/// stays at `checkpoint-v20` and the archive at format 16.
/// `portfolio-v72` (the research chain, task 1 — the loop writes prose): the
/// synthesis conversation returns the pass's write-up as prose under no
/// grammar, and its second message asks for the follow-up question (the one
/// word `none` the only reply the app reads); the gathering brief leads with
/// the holding-constant block — HOLDING, FETCHED VALUES as the thesis message
/// renders it, NEWS LEADS, on a continuity run PRIOR THESIS — then PAGES
/// ALREADY RETRIEVED and the topic text, FOLLOW-UP with WRITE-UP SO FAR on a
/// follow-up pass and WRITE-UPS SO FAR on the disconfirming pass; the claims
/// layer, PRIOR FINDINGS, CLAIMS SO FAR, the findings grammar and the
/// distillation grammar with its typed channels leave every prompt; ANALYSIS
/// is this run's write-ups under their topic headings until consolidation
/// lands. The persisted audit — the write-ups, the disconfirming write-up,
/// the page roster — moves the trail to `checkpoint-v21`, and the seed table
/// leaves the archive at format 17.
pub const PROMPT_VERSION: &str = "portfolio-v72";

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

/// The persisted rationale beside [`carried_action`] — the model's sentence
/// with the app's caveat where one rode — on the two branches that carry an
/// action; `None` on an abstained or not-rated verdict. The action packet's
/// PRIOR ACTION renders it through `pipeline::investment_sentence`, so the
/// caveat never reaches the decision.
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

/// The frame sentences every object-returning call's system prompt closes
/// with, after the role line and the output-name sentence (`portfolio-v62`,
/// ruled 2026-09-29): the outputs are named once, up front, so the model
/// knows what it reads Part 1 for, and the frame then says where their
/// definitions and the shape sit — before v62 the frame preceded the names
/// and said Part 2 "states what to determine", which the next sentence then
/// stated. The gathering call, which returns no object, keeps its own frame.
pub(crate) const TWO_PART_FRAME: &str =
    "Part 1 of the message gives the inputs. Part 2 defines those outputs and gives the shape to return.";

/// The fields the typed appendix message must return, in the message's own
/// order (`docs/portfolio-workflow.md` §Step 6f): the schema's `required` set
/// and the return shape are both built from this list, so the enforced
/// grammar and the stated shape cannot diverge.
pub const APPENDIX_KEYS: [&str; 4] = [
    "conviction",
    "expected_price_3m",
    "expected_price_12m",
    "expected_price_3y",
];

/// The JSON Schema handed to Ollama's `format` for the typed appendix: the
/// conviction a nullable string enum, each expected price a nullable number,
/// every field required — the grammar admits `null` on each, the document's
/// silence (`docs/portfolio-analysis.md` §The holding verdict). The schema
/// stays within the subset the local grammar converter proves out (type /
/// properties / required / enum); positivity is stated in the prompt and
/// enforced at decode ([`validate_appendix_domain`]), never as a range keyword.
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

/// The placeholder-only return shape the appendix message closes with: the
/// schema's four keys in the message's order, the conviction as its
/// alternatives with null among them, each price as "<0|null>".
pub fn appendix_return_shape() -> String {
    placeholder_shape(&appendix_schema(), &APPENDIX_KEYS)
}

/// A schema's nesting with every value blank — a string is "", a number 0, a
/// boolean false, an enum its alternatives as "<a|b|c>", a nullable enum with
/// null among them, an array one item — written with each object's keys in
/// `order` (unnamed keys after the named ones, alphabetically). The one
/// renderer behind [`appendix_return_shape`], [`action_return_shape`] and the
/// distillation shapes (`distill.rs`).
pub(crate) fn placeholder_shape(schema: &Value, order: &[&str]) -> String {
    fn visit(schema: &Value) -> Value {
        if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            // A nullable choice shows both halves — "<a|b|null>" — so null never
            // reads as the expected value (the 3.8 skip).
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
            // A nullable scalar shows both halves too — "<0|null>" — so a
            // number the reply may leave null never reads as 0
            // (`portfolio-v64`). No served schema carries a nullable plain
            // string today; one would read "<text|null>".
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

/// The pre-v44 template renderer, retired from every prompt with the
/// distillation rewrite (`portfolio-v44`) and kept test-only as the source
/// [`response_template_samples`] materializes its enum choices from.
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
    // The ledger's numeric fields, whose `1` placeholders read as magnitudes
    // (attempt-6 Finding 8; ruled 2026-09-16, F4): each note restates the
    // ledger contract's own rule (`docs/portfolio-analysis.md` §The position
    // thesis ledger), never a preference.
    let notes = if schema["properties"].get("ledger").is_some() {
        "\nField notes (the ledger's numeric fields; the template's 1 values are placeholders without magnitude):\n\
         ledger.bear, ledger.base and ledger.bull are three sibling scenario objects under ledger, each with its conditions and probability_pct (0-100).\n\
         ledger.*.quant.threshold is exactly the level the statement names, in the series' unit.\n\
         ledger.*.quant.margin is the separate noise band around that level in the same unit, non-negative and a fraction of a nonzero level (a zero level has no cap), never folded into the threshold.\n"
    } else {
        ""
    };
    format!(
        "\nResponse shape template (illustrative structure, not a completed answer; arrays may be empty). Replace each <field-path> placeholder with that field's value; for enum fields choose one of the Field alternatives below, never the literal placeholder. Sample numbers, booleans and neutral outlooks are not findings:\n{}\nField alternatives (allowed values, not preferences):\n{}\n{notes}The entire response is one JSON object beginning with {{, with no code fence or surrounding prose.\n",
        serde_json::to_string(&example).unwrap(), enums.join("\n")
    )
}

/// Materialize every enum alternative in the rendered template, then let each
/// caller test its real decoder. This also verifies that a placeholder names
/// its exact schema path rather than silently accepting a misspelled field.
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

/// The appendix's declared domain, enforced app-side at the appendix message's
/// decode (`docs/portfolio-analysis.md` §The holding verdict): each present
/// expected price finite and strictly positive — a value a share price can
/// take — a present conviction one of its three values (the type carries
/// that), and `null` accepted on every field as the document's silence. The
/// grammar cannot express range keywords ([`appendix_schema`]), so without this
/// gate a zero or negative price would persist and later score. The gate is the
/// declared domain, never the engine's values — the two-arm contract's "never
/// validated against the engine" holds — and it rejects the object whole,
/// never clamps. The error names every offending field with its authored
/// value, never the first alone, so the failure detail reads the whole
/// response. The rule is general: a typed model-arm field is gated to its
/// declared domain here, so a slice that adds one extends this gate with it.
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

/// Every appendix value outside its declared domain in one response
/// ([`validate_appendix_domain`]), each entry naming the field and the authored
/// value.
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

/// The reduced action set — a `role_risk_only` holding's **engine set**, rendered
/// into the action call's prompt as the engine arm's evidence: the add family
/// requires return evidence this branch has none of by construction
/// (`docs/portfolio-analysis.md` §Portfolio action). The model's choice stays
/// structurally open (the full ladder), departures annotated on the audit.
pub const ROLE_RISK_ACTIONS: [Action; 3] = [Action::SellAll, Action::Trim, Action::Hold];

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

/// The action call's response-contract line, generated from [`ACTION_KEYS`]:
/// the output names the system prompt states once (`portfolio-v41`), on the
/// same footing as [`interpretation_response_contract`]. The fence rule and
/// the field meanings live in the message's Part 2; the shape is
/// [`action_return_shape`].
pub fn action_response_contract() -> String {
    let (last, head) = ACTION_KEYS.split_last().expect("the key list is never empty");
    format!("You will return {} and {last}, as one JSON object.", head.join(", "))
}

/// The placeholder-only return shape the action message closes with
/// (`portfolio-v41`): the rung enum inline, the rationale blank — rendered from
/// [`action_decision_schema`] by the same visitor as the interpretation shape.
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
    fn the_appendix_shape_is_four_nullable_keys_in_the_message_order() {
        // The appendix message carries no contract line of its own — the
        // conversation's system message is the thesis document's — and its
        // shape is the placeholder-only return shape on the shared renderer:
        // the four declared keys in the message's order, the conviction as its
        // alternatives with null among them, each price "<0|null>", no field
        // notes or template narration.
        let shape_text = appendix_return_shape();
        assert_eq!(
            shape_text,
            "{\"conviction\":\"<high|medium|low|null>\",\"expected_price_3m\":\"<0|null>\",\
             \"expected_price_12m\":\"<0|null>\",\"expected_price_3y\":\"<0|null>\"}"
        );
        for narration in ["Field notes", "Field alternatives", "code fence", "<conviction>"] {
            assert!(!shape_text.contains(narration), "`{narration}`: {shape_text}");
        }
        // The schema requires every key and admits null on each.
        let schema = appendix_schema();
        let required: Vec<&str> = schema["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(required, APPENDIX_KEYS.to_vec());
        assert_eq!(schema["properties"]["conviction"]["type"], json!(["string", "null"]));
        assert!(schema["properties"]["conviction"]["enum"].as_array().unwrap().iter().any(Value::is_null));
        for key in &APPENDIX_KEYS[1..] {
            assert_eq!(schema["properties"][*key]["type"], json!(["number", "null"]), "{key}");
        }
        // Every materialized enum choice decodes, null included.
        for sample in response_template_samples(&schema) {
            let decoded: ThesisAppendix = serde_json::from_value(sample).unwrap();
            assert!(validate_appendix_domain(&decoded).is_ok());
        }
        // A wholly null body decodes as the document's silence; a missing key
        // reads as null too — model-written JSON stays lenient, and the grammar
        // requires every key, so a decode never sees one missing.
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
        // The action contract is the output names alone (`portfolio-v41`); the
        // fence sentence and the field meanings live in the message's Part 2,
        // and the shape is placeholder-only with the ladder inline.
        assert_eq!(action_response_contract(), "You will return action and rationale, as one JSON object.");
        assert_eq!(action_return_shape(), "{\"action\":\"<sell-all|trim|hold|add|add-aggressively>\",\"rationale\":\"\"}");
    }

    #[test]
    fn appendix_domain_admits_null_and_any_finite_positive_price() {
        // Null on every field is the document's silence; a tiny or huge finite
        // positive price is a price.
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
        // Every violation in one response is named with its authored value —
        // the failure detail reads the whole appendix, never the first miss
        // alone — and the in-domain and null legs beside them are not.
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
        // The model arm persists exactly as authored: the document as text, the
        // appendix with its nulls — a null field reads back null, never a
        // defaulted value — beside the app-stamped engine arm.
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
            engine_rung: Action::Hold,
            authored_band_relation: None,
        };
        let verdict = HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
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
        // The action call's schema advertises the full ladder on every branch,
        // and its required set is exactly the declared keys.
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
        // The branch carries no grade / targets / appendix keys at all — its
        // document stands alone.
        assert!(s.get("grade").is_none());
        assert!(s.get("price_targets").is_none());
        assert!(s.get("appendix").is_none());
        assert!(s.get("conviction").is_none());
        assert_eq!(s["thesis_document"], "Role: the core fixed-income sleeve.");
        let round: VerdictDisposition = serde_json::from_value(s).unwrap();
        assert_eq!(round, v);
    }

}
