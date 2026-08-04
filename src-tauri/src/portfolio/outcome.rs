//! Outcome learning — the recommendation-state-keyed decision-episode machinery
//! (`docs/portfolio-analysis.md §Outcome learning`): episodes open when a holding's
//! recommendation state changes, accrue engine-computed 1/3/6/12-month outcome
//! labels (total-return primary, price-only common basis), and feed the derived
//! calibration reads. Everything here is deterministic; no model stage is involved.
//!
//! The module splits into a pure core and two thin impure seams:
//! - **Lifecycle** ([`plan_episodes`], [`tag_alignment`]) and **reads**
//!   ([`derive_reads`]) are pure functions over in-memory episodes.
//! - **Labels** ([`mature_labels`]) read price series through [`SeriesCtx`] — the
//!   shared price-bar cache plus an [`OutcomePriceSource`] for label-time
//!   refreshes (Stooq rung, FMP dated-EOD rung) — and the dividend history for
//!   the total-return leg.
//!
//! Two doc-recorded reductions ship dormant: the standing-thesis episode-creation
//! leg and the self-correction read both depend on the 6g what-changed attribution
//! validator (designed, unbuilt), so episodes open only on observable state
//! changes and the self-correction counters stay structurally zero until that
//! validator lands. Terminal outcomes are typed conservatively: no corporate-action
//! feed exists, so a previously covered series that stops resolves
//! `terminal-unscorable` past the price-coverage grace, never a fabricated
//! acquisition or bankruptcy read.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::{Months, NaiveDate};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::portfolio::diff::HoldingsDiff;
use crate::portfolio::engine::{DatedValue, HurdleRead};
use crate::portfolio::{
    store, Action, ActionSource, Conviction, Grade, HoldingAudit, HoldingVerdict, HurdleState,
    PositionChange, PriceTargets, RiskTier, SubScores, VerdictDisposition,
};
use crate::schwab::Holdings;

// ---- Calibratable constants (`docs/portfolio-analysis.md §Starting parameters`) --

/// The matured archive's row cap — matured episodes beyond it evict oldest-first.
/// The active set carries no cap: an episode still accruing labels is never evicted.
pub const MATURED_ARCHIVE_CAP: u32 = 5_000;

/// How long past a window's end a pending price leg may wait for coverage before it
/// closes as the typed `price-coverage-unscorable` label (~3 months, the constant
/// shared with Trade Opportunities — `docs/storage.md §Local Analysis Suite
/// Storage`).
pub const PRICE_COVERAGE_GRACE_DAYS: i64 = 91;

/// The proposal eligibility bar: unique holdings with matured (scored) windows,
/// clustered by holding — never raw episode counts. Below it the pass records the
/// typed below-bar note and proposes nothing.
pub const PROPOSAL_ELIGIBILITY_BAR: usize = 30;

/// The bear–bull band's declared nominal coverage, scored with the interval score
/// on the price-only label.
pub const NOMINAL_BAND_COVERAGE: f64 = 0.80;

/// The four forward label windows, in months.
pub const LABEL_WINDOWS_MONTHS: [u32; 4] = [1, 3, 6, 12];

/// The market benchmark's Stooq identity (`docs/data-sources.md §Stooq`).
pub const MARKET_BENCHMARK: &str = "^spx";

/// Coverage tolerance: a series whose latest bar sits within this many calendar
/// days before a window end still covers it (weekends and short market closures —
/// the label value is the last close at or before the window end either way).
const COVERAGE_TOLERANCE_DAYS: i64 = 4;

/// Calendar-day pad on fetch ranges, so the entry anchor (the first session after
/// the run) and month-end joins never sit exactly on a fetch boundary.
const FETCH_PAD_DAYS: i64 = 7;

// ---- Sector identity -------------------------------------------------------------

/// The SPDR sector-ETF benchmark for an FMP profile sector label
/// (`docs/data-sources.md §Stooq` — the sector-ETF mapping). Accepts both FMP's
/// label vocabulary and the near-identical GICS names; `None` for anything else
/// (the typed `sector-unscorable` path, never a guessed benchmark).
pub fn spdr_for_sector(label: &str) -> Option<&'static str> {
    let l = label.trim().to_ascii_lowercase();
    Some(match l.as_str() {
        "basic materials" | "materials" => "XLB",
        "communication services" => "XLC",
        "energy" => "XLE",
        "financial services" | "financials" => "XLF",
        "industrials" => "XLI",
        "technology" | "information technology" => "XLK",
        "consumer defensive" | "consumer staples" => "XLP",
        "real estate" => "XLRE",
        "utilities" => "XLU",
        "healthcare" | "health care" => "XLV",
        "consumer cyclical" | "consumer discretionary" => "XLY",
        _ => return None,
    })
}

/// The episode's **entry-stamped sector identity** — the sector label at the anchor
/// run plus its resolved SPDR benchmark symbol, stamped once and never re-classified
/// at label time (resolvable after an exit by construction). A holding with no valid
/// mapping carries the typed `sector-unscorable` reason on its sector legs; the
/// market leg is unaffected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorIdentity {
    pub sector: Option<String>,
    pub benchmark: Option<String>,
    /// The typed `sector-unscorable` reason when no benchmark resolved.
    pub unscorable: Option<String>,
}

impl SectorIdentity {
    /// Resolve a profile sector label into the stamped identity.
    pub fn resolve(sector_label: Option<&str>) -> Self {
        match sector_label {
            Some(label) => match spdr_for_sector(label) {
                Some(etf) => Self {
                    sector: Some(label.to_string()),
                    benchmark: Some(etf.to_string()),
                    unscorable: None,
                },
                None => Self {
                    sector: Some(label.to_string()),
                    benchmark: None,
                    unscorable: Some(format!(
                        "sector-unscorable: no SPDR mapping for sector {label:?}"
                    )),
                },
            },
            None => Self::unscorable("no sector label resolved at the anchor run"),
        }
    }

    pub fn unscorable(reason: &str) -> Self {
        Self {
            sector: None,
            benchmark: None,
            unscorable: Some(format!("sector-unscorable: {reason}")),
        }
    }
}

// ---- Episode types ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EpisodeState {
    Active,
    /// Frozen into the compact matured archive: every window label recorded.
    Matured,
}

/// The next run's deterministic net-alignment tag (`docs/portfolio-analysis.md`
/// §Outcome learning). The name is deliberate: a net diff cannot see round trips,
/// transfers, or partial execution, so the tag claims only what the diff observed —
/// never that advice was "followed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservedNetAlignment {
    Aligned,
    Contrary,
    Partial,
    Unknown,
    /// A net long↔short reversal — its own class, excluded from the aligned /
    /// contrary cohort slices (crossing zero lies outside any long-side
    /// recommendation's direction).
    Reversed,
}

/// Why an episode opened — the observable recommendation-state change that minted
/// it. The standing-thesis leg (an attributed intrinsic move with the action
/// unchanged) is dormant until the 6g what-changed attribution validator lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenReason {
    /// First analysis of the holding (or first after the machinery landed).
    Debut,
    BranchFlip,
    ActionChange,
    WeightRangeChange,
    /// The action change was the over-age rule-demotion, not a model decision.
    RuleDemotion,
}

/// What a non-opening run recorded onto the active episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservationKind {
    /// A fresh pass re-affirmed the recommendation (inputs may have moved — a
    /// re-affirmed decision over moved inputs is still one decision).
    Reaffirmed,
    /// A selective run carried the verdict forward unchanged.
    Carried,
    /// An insufficient-evidence exit retained the standing recommendation.
    Abstained,
}

/// One extension observation on an active episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeObservation {
    pub run_id: String,
    /// The observing run's `created_at`.
    pub observed_at: String,
    pub kind: ObservationKind,
}

/// A confirmed falsifier crossing recorded onto the episode that carried the
/// condition (`{condition_id, confirmed_at, confirmation_observation_id}` —
/// `docs/portfolio-analysis.md §Outcome learning`). The lead-time fields are
/// engine-stamped when the episode's twelve-month window matures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FalsifierEvent {
    pub condition_id: String,
    /// The run date the crossing confirmed on.
    pub confirmed_at: String,
    /// The distinct observation the confirmation keyed on.
    pub confirmation_observation_id: String,
    /// True when the confirmation arrived after the holding's episode had matured —
    /// recorded onto the latest matured episode as context for the next episode,
    /// feeding no lead-time read (that read is bounded to the matured episode's own
    /// window).
    #[serde(default)]
    pub post_maturity: bool,
    /// Signed trading-day distance from `confirmed_at` to the first within-window
    /// close below the recorded twelve-month bear-case line: positive = the
    /// falsifier confirmed before the breach, zero = same session, negative = the
    /// line had already broken. Stamped when the 12-month window matures.
    #[serde(default)]
    pub lead_time_trading_days: Option<i64>,
    /// Explicit `no-material-drawdown`: no within-window close below the bear line
    /// by maturity.
    #[serde(default)]
    pub no_material_drawdown: Option<bool>,
}

/// The decision-time engine snapshot a priced episode carries — what a future
/// parameter proposal needs for counterfactual re-testing, frozen on the episode
/// because the run's own audit record can age out of the 10-run retention before a
/// 12-month label matures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationSnapshot {
    pub sub_scores: SubScores,
    pub grade: Grade,
    pub conviction: Conviction,
    pub risk_tier: Option<RiskTier>,
    /// The scenario bands and base-case targets (price targets — target
    /// calibration scores these against the price-only label).
    pub price_targets: PriceTargets,
    pub dead_money: Option<HurdleState>,
    /// The full hurdle read (scenario total-return distribution + the tier-scaled
    /// hurdle rate) where the audit carried it; `None` on pre-field audits.
    pub hurdle: Option<HurdleRead>,
    /// The run-level DGS2 print the hurdle was anchored on.
    pub dgs2: Option<f64>,
    /// Cap signals in force at the decision (the pre-profit overlay's matched
    /// rules; empty when none).
    pub cap_signals: Vec<String>,
    pub grade_parameter_version: Option<String>,
    pub target_parameter_version: Option<String>,
    pub degraded_inputs: Vec<String>,
}

/// The priced branch's episode body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricedEpisode {
    /// The final portfolio action.
    pub action: Action,
    /// The standalone lean. **As-built equal to the action** — the 7b construction
    /// stage is unbuilt, so the 6f action *is* the lean (`engine.rs`
    /// `feasible_actions`); the field exists so the 7b slice diverges them without
    /// an episode-schema migration.
    pub lean: Action,
    /// The divergence-from-lean rationale (the action half's attribution category)
    /// once 7b can diverge them; `None` = matched.
    #[serde(default)]
    pub lean_divergence: Option<String>,
    /// The ledger's pre-committed target-weight range — the construction read's
    /// stable half (the sizing band is engine context recomputed at current
    /// weights, so it is deliberately not the comparison key).
    pub target_weight_low: Option<f64>,
    pub target_weight_high: Option<f64>,
    pub snapshot: CalibrationSnapshot,
}

/// The `role_risk_only` branch's reduced episode body — no lean, grade, conviction,
/// band, target, or dead-money field exists on that verdict to record. Excluded from
/// target calibration and every grade-linked read; counted in its own class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleRiskEpisode {
    pub action: Action,
    pub target_weight_low: Option<f64>,
    pub target_weight_high: Option<f64>,
    pub degraded_inputs: Vec<String>,
}

/// The branch-typed episode body — an explicit schema per branch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EpisodeBody {
    Priced(Box<PricedEpisode>),
    RoleRiskOnly(RoleRiskEpisode),
}

/// One forward label window on an episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowLabel {
    pub window_months: u32,
    /// The window's end date (anchor date + the window, calendar-clamped), ISO.
    pub window_end: String,
    pub outcome: LabelOutcome,
}

/// A window label's outcome. Typed unscorable closures are counted and logged,
/// excluded from spreads and calibration denominators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LabelOutcome {
    Pending,
    Scored(Box<ScoredLabel>),
    /// The price leg never covered the window within the shared grace.
    PriceCoverageUnscorable,
    /// A previously covered series stopped resolving — conservatively terminal
    /// (no corporate-action feed exists to type an acquisition or bankruptcy).
    TerminalUnscorable,
}

/// One scored window label — reconstructed statelessly from split-adjusted daily
/// closes. The entry reference is the **next session's close** after the anchor run
/// (a consistent evaluation anchor, deliberately not called an executable price).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredLabel {
    pub entry_date: String,
    pub entry_price: f64,
    pub end_date: String,
    pub end_price: f64,
    /// Price-only forward return — the cross-entry common basis.
    pub price_return: f64,
    /// Total return: the window's cash dividends summed without reinvestment over
    /// the anchor price. `None` = the label-time dividends re-pull failed — the
    /// labeled price-only fallback, with the gap recorded.
    pub total_return: Option<f64>,
    #[serde(default)]
    pub total_return_gap: Option<String>,
    /// Maximum drawdown over the window's closes (≤ 0).
    pub max_drawdown: f64,
    /// Price-only return spread vs the market benchmark (both sides price-only).
    pub vs_market: Option<f64>,
    #[serde(default)]
    pub market_leg_gap: Option<String>,
    /// Price-only return spread vs the entry-stamped sector benchmark.
    pub vs_sector: Option<f64>,
    #[serde(default)]
    pub sector_leg_gap: Option<String>,
    /// The run date the label recorded on.
    pub labeled_at: String,
}

/// One decision episode — the bounded twelve-month measurement instrument
/// (`docs/portfolio-analysis.md §Outcome learning`), persisted independently of the
/// 10-run retention and frozen into the matured archive once its 12-month labels
/// record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionEpisode {
    pub episode_id: String,
    pub symbol: String,
    pub anchor_run_id: String,
    /// The anchor run's `created_at` (UTC RFC3339); its date keys the label windows.
    pub anchor_at: String,
    /// The run that actually authored the intrinsic fields this episode records —
    /// the verdict's effective analysis vintage.
    pub intrinsic_vintage: String,
    /// True when the intrinsic fields were authored by the anchor run itself. The
    /// lean-keyed cohorts and target calibration consume vintage-fresh episodes
    /// only (a carried verdict's forecast is scored by the episode that authored
    /// it).
    pub vintage_fresh: bool,
    pub action_source: ActionSource,
    /// The position delta at the anchor run.
    pub position_change: PositionChange,
    pub sector: SectorIdentity,
    pub opened: Vec<OpenReason>,
    pub body: EpisodeBody,
    pub observations: Vec<EpisodeObservation>,
    /// Tagged once, by the first run after the anchor, from its deterministic
    /// holdings diff.
    #[serde(default)]
    pub alignment: Option<ObservedNetAlignment>,
    #[serde(default)]
    pub falsifier_events: Vec<FalsifierEvent>,
    pub labels: Vec<WindowLabel>,
    pub state: EpisodeState,
    /// Per-holding self-correction count — **dormant**: populated only once the 6g
    /// what-changed attribution validator (designed, unbuilt) labels
    /// self-corrections; structurally zero until then.
    #[serde(default)]
    pub self_correction_count: u32,
}

impl DecisionEpisode {
    /// The anchor date (the run date the windows key on).
    pub fn anchor_date(&self) -> Option<NaiveDate> {
        parse_iso_date_prefix(&self.anchor_at)
    }

    fn body_action(&self) -> Action {
        match &self.body {
            EpisodeBody::Priced(p) => p.action,
            EpisodeBody::RoleRiskOnly(r) => r.action,
        }
    }

    /// Whether every window label has recorded (scored or typed) — the maturity
    /// boundary.
    fn fully_labeled(&self) -> bool {
        self.labels
            .iter()
            .all(|l| !matches!(l.outcome, LabelOutcome::Pending))
    }
}

fn parse_iso_date_prefix(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

/// The label windows for an anchor date, all pending.
pub fn pending_labels(anchor: NaiveDate) -> Vec<WindowLabel> {
    LABEL_WINDOWS_MONTHS
        .iter()
        .map(|&m| WindowLabel {
            window_months: m,
            window_end: window_end(anchor, m).format("%Y-%m-%d").to_string(),
            outcome: LabelOutcome::Pending,
        })
        .collect()
}

/// A window's end date: the anchor plus `months` calendar months, day-clamped
/// (Jan 31 + 1 month = Feb 28/29).
pub fn window_end(anchor: NaiveDate, months: u32) -> NaiveDate {
    anchor
        .checked_add_months(Months::new(months))
        .unwrap_or(anchor)
}

// ---- Run-facing records (persisted on the run blob) --------------------------------

/// One opened-episode note on the run record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenedEpisodeNote {
    pub symbol: String,
    pub episode_id: String,
    pub reasons: Vec<OpenReason>,
}

/// One alignment tag applied by this run's diff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentTag {
    pub symbol: String,
    pub episode_id: String,
    pub alignment: ObservedNetAlignment,
}

/// One newly recorded window label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaturedNote {
    pub symbol: String,
    pub episode_id: String,
    pub window_months: u32,
    /// "scored" / "price-coverage-unscorable" / "terminal-unscorable".
    pub outcome: String,
    /// The scored total return where the outcome scored (price-only where the
    /// total-return leg was unavailable).
    pub total_return: Option<f64>,
    pub price_return: Option<f64>,
}

/// One cohort's per-window statistics — unique-holding counted (multiple episodes
/// of one symbol average per symbol first).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CohortStat {
    /// The cohort key (an action rung's kebab label, or a class name).
    pub key: String,
    pub unique_holdings: usize,
    /// Mean absolute total return — the primary ordering read (falls back to the
    /// price-only mean per episode where the TR leg was unavailable; a labeled mix
    /// is still a mix, so the price-only mean rides beside it).
    pub mean_total_return: Option<f64>,
    pub mean_price_return: Option<f64>,
    /// Price-only relative spreads — the regime-controlled diagnostic.
    pub mean_vs_market: Option<f64>,
    pub mean_vs_sector: Option<f64>,
}

/// The action-cohort spreads for one window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CohortWindowRead {
    pub window_months: u32,
    /// The intrinsic layer: lean-keyed, vintage-fresh, model-chosen priced episodes.
    pub lean_cohorts: Vec<CohortStat>,
    /// The final-action strata — diagnostic only (raw return ordering cannot score
    /// a risk override), read across vintages, stratified by the action rung.
    pub final_action_cohorts: Vec<CohortStat>,
    /// `role_risk_only` episodes — their own class, never pooled.
    pub role_risk: Option<CohortStat>,
    /// Rule-demoted episodes — their own class, out of the pooled cohorts.
    pub rule_demoted: Option<CohortStat>,
}

/// Target calibration for one band window (1- and 12-month bands score at their
/// matching windows; the 3- and 6-month labels serve the cohort reads).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetCalibrationRead {
    pub window_months: u32,
    /// Bands scored (vintage-fresh episodes whose window scored and whose snapshot
    /// carried the matching band).
    pub scored: usize,
    /// Fraction of realized prices inside the bear–bull band, vs the declared
    /// nominal ([`NOMINAL_BAND_COVERAGE`]).
    pub coverage_rate: Option<f64>,
    pub nominal_coverage: f64,
    /// Mean interval (Winkler) score at the nominal level — calibration and
    /// sharpness together; lower is better, ungameable by width.
    pub mean_interval_score: Option<f64>,
    /// Mean signed base-case error `(realized − base) / base` — the systematic-bias
    /// read on the scenario engine.
    pub mean_base_signed_error: Option<f64>,
}

/// One falsifier lead-time record (priced episodes only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FalsifierLeadTimeRead {
    pub symbol: String,
    pub episode_id: String,
    pub condition_id: String,
    pub confirmed_at: String,
    pub lead_time_trading_days: Option<i64>,
    pub no_material_drawdown: bool,
}

/// The per-holding self-correction accumulation — dormant until the 6g attribution
/// validator labels self-corrections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelfCorrectionRead {
    pub total: u32,
    /// Holdings with a non-zero count, `(symbol, count)`.
    pub per_holding: Vec<(String, u32)>,
}

/// The proposal eligibility record — the typed below-bar note
/// (`docs/portfolio-analysis.md §Outcome learning`: no proposal below the bar; the
/// proposal statistics themselves ride a later slice once matured data exists).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EligibilityRecord {
    /// Unique holdings with at least one **scored** matured window.
    pub unique_matured_holdings: usize,
    pub bar: usize,
    pub eligible: bool,
    pub note: String,
}

/// The four derived scorecard reads (`docs/portfolio-analysis.md §Outcome
/// learning`), computed per matured window on unique-holding counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedReads {
    pub cohorts: Vec<CohortWindowRead>,
    pub target_calibration: Vec<TargetCalibrationRead>,
    pub falsifier_lead_times: Vec<FalsifierLeadTimeRead>,
    pub self_correction: SelfCorrectionRead,
    pub eligibility: EligibilityRecord,
}

/// This run's outcome-learning records, persisted with the run
/// (`docs/portfolio-workflow.md §Step 7a, §Step 8`). `#[serde(default)]` on the run
/// field keeps pre-outcome runs decodable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRecords {
    pub opened: Vec<OpenedEpisodeNote>,
    /// Symbols whose active episode this run extended (re-affirmed / carried /
    /// abstained).
    pub extended: Vec<String>,
    pub alignment_tags: Vec<AlignmentTag>,
    /// Window labels newly recorded by this run's label pass.
    pub matured: Vec<MaturedNote>,
    /// Symbols with a window pending on a price-coverage gap (within grace).
    pub pending_coverage: Vec<String>,
    pub reads: DerivedReads,
}

// ---- The label-time price source ---------------------------------------------------

/// The outcome pass's retrieval seam: daily closes (Stooq rung, FMP dated-EOD rung)
/// and the dividend history for the total-return leg. Behind a trait so the job is
/// offline-testable; failures are fail-soft at the caller (a label stays pending,
/// never a run failure).
pub trait OutcomePriceSource {
    fn daily_closes(&self, symbol: &str, from: NaiveDate, to: NaiveDate)
        -> Result<Vec<DatedValue>>;
    fn dividend_history(
        &self,
        symbol: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<DatedValue>>;
}

/// The live source: Stooq primary, FMP dated-EOD second rung, FMP `dividends` for
/// the total-return leg — the same rung order as the per-holding deep history
/// (`docs/data-sources.md §Stooq`).
pub struct LiveOutcomePrices {
    pub stooq: crate::stooq::StooqSource,
    pub fmp: crate::fmp::FmpDataSource,
}

impl OutcomePriceSource for LiveOutcomePrices {
    fn daily_closes(
        &self,
        symbol: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<DatedValue>> {
        match self.stooq.daily_closes(symbol, from, to) {
            Ok(closes) => Ok(closes),
            Err(stooq_err) => {
                // The FMP dated-EOD fetch is now-anchored; a lookback from today
                // covering `from` spans the requested range (labels always read
                // through the present).
                let today = chrono::Utc::now().date_naive();
                let lookback = (today - from).num_days().max(1);
                match self.fmp.fetch_dated_eod(symbol, lookback) {
                    Ok(closes) if !closes.is_empty() => Ok(closes),
                    Ok(_) => Err(stooq_err.context("FMP dated-EOD fallback was empty")),
                    Err(fmp_err) => {
                        Err(stooq_err.context(format!("FMP dated-EOD fallback failed: {fmp_err}")))
                    }
                }
            }
        }
    }

    fn dividend_history(
        &self,
        symbol: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<DatedValue>> {
        self.fmp.fetch_dividend_history(symbol, from, to)
    }
}

/// A source with nothing to serve — every call errs, so labels stay pending
/// (the offline / demo posture; also the stub the job tests pass).
pub struct UnavailablePriceSource;

impl OutcomePriceSource for UnavailablePriceSource {
    fn daily_closes(&self, symbol: &str, _: NaiveDate, _: NaiveDate) -> Result<Vec<DatedValue>> {
        anyhow::bail!("no outcome price source available ({symbol})")
    }
    fn dividend_history(&self, symbol: &str, _: NaiveDate, _: NaiveDate) -> Result<Vec<DatedValue>> {
        anyhow::bail!("no outcome price source available ({symbol})")
    }
}

/// The outcome pass's external surfaces, bundled so the job signature grows by one
/// parameter: the price source plus the optional embedder for the matured-read
/// durable learnings. `None` at the job level = no retrievals (labels stay
/// pending) and no embeddings — the pure lifecycle machinery still runs.
pub struct OutcomeSources<'a> {
    pub price: &'a dyn OutcomePriceSource,
    pub embedder: Option<&'a dyn crate::embedding::Embedder>,
}

/// The shared price-bar cache read/refresh context (`docs/storage.md §Local
/// Analysis Suite Storage` — the label-time strict rule): a symbol's series is
/// served from the cache when it already covers the needed span; otherwise one
/// fetch per symbol per pass refreshes it through today and merges into the cache.
/// A failed refresh serves whatever the cache holds (the caller's coverage rule
/// then leaves the label pending).
pub struct SeriesCtx<'a> {
    conn: &'a Connection,
    source: Option<&'a dyn OutcomePriceSource>,
    mem: HashMap<String, Vec<DatedValue>>,
    /// Symbols already fetched this pass — success or failure alike, so a symbol
    /// whose source genuinely lacks the span (a short history, a delisting) can
    /// never spend a second request in one pass.
    fetch_attempted: HashSet<String>,
}

impl<'a> SeriesCtx<'a> {
    pub fn new(conn: &'a Connection, source: Option<&'a dyn OutcomePriceSource>) -> Self {
        Self {
            conn,
            source,
            mem: HashMap::new(),
            fetch_attempted: HashSet::new(),
        }
    }

    /// The symbol's cached-through series covering `[from, through]` where
    /// possible. Coverage is the caller's judgment ([`covers_through`]); this only
    /// guarantees the cache is as complete as one refresh could make it.
    fn series(&mut self, symbol: &str, from: NaiveDate, through: NaiveDate) -> &[DatedValue] {
        let key = symbol.to_ascii_uppercase();
        if !self.mem.contains_key(&key) {
            let cached = store::load_price_bars(self.conn, &key).unwrap_or_default();
            self.mem.insert(key.clone(), cached);
        }
        let needs_fetch = {
            let cached = &self.mem[&key];
            let covers_start = cached
                .first()
                .and_then(|b| parse_iso_date_prefix(&b.date))
                .is_some_and(|d| d <= from);
            !(covers_start && covers_through(cached, through))
        };
        if needs_fetch && !self.fetch_attempted.contains(&key) {
            if let Some(source) = self.source {
                self.fetch_attempted.insert(key.clone());
                let fetch_from = from - chrono::Duration::days(FETCH_PAD_DAYS);
                let fetch_to = chrono::Utc::now().date_naive();
                if let Ok(bars) = source.daily_closes(symbol, fetch_from, fetch_to) {
                    if !bars.is_empty() {
                        let _ = store::merge_price_bars(self.conn, &key, &bars);
                        let merged = merge_series(self.mem.remove(&key).unwrap_or_default(), bars);
                        self.mem.insert(key.clone(), merged);
                    }
                }
            }
        }
        &self.mem[&key]
    }
}

/// Merge two dated series by date (newer fetch wins on a shared date), sorted
/// oldest-first.
fn merge_series(cached: Vec<DatedValue>, fresh: Vec<DatedValue>) -> Vec<DatedValue> {
    let mut by_date: std::collections::BTreeMap<String, f64> =
        cached.into_iter().map(|b| (b.date, b.value)).collect();
    for b in fresh {
        by_date.insert(b.date, b.value);
    }
    by_date
        .into_iter()
        .map(|(date, value)| DatedValue { date, value })
        .collect()
}

/// Whether a series' latest bar reaches `through` (within the weekend/holiday
/// tolerance) — the label-time coverage rule's mechanical half.
fn covers_through(closes: &[DatedValue], through: NaiveDate) -> bool {
    closes
        .last()
        .and_then(|b| parse_iso_date_prefix(&b.date))
        .is_some_and(|d| d >= through - chrono::Duration::days(COVERAGE_TOLERANCE_DAYS))
}

fn close_at_or_before(closes: &[DatedValue], date: NaiveDate) -> Option<&DatedValue> {
    let iso = date.format("%Y-%m-%d").to_string();
    closes.iter().rev().find(|b| b.date.as_str() <= iso.as_str())
}

fn first_close_after(closes: &[DatedValue], date: NaiveDate) -> Option<&DatedValue> {
    let iso = date.format("%Y-%m-%d").to_string();
    closes.iter().find(|b| b.date.as_str() > iso.as_str())
}

// ---- The label engine ---------------------------------------------------------------

/// The interval (Winkler) score for a central `(1 − alpha)` interval `[lo, hi]`
/// against a realized value: width plus `2/alpha` times any exceedance. A proper
/// scoring rule — an arbitrarily wide band pays for its width, so raw hit-rate
/// can't be gamed. Lower is better.
pub fn interval_score(lo: f64, hi: f64, realized: f64, alpha: f64) -> f64 {
    let width = hi - lo;
    let mut score = width;
    if realized < lo {
        score += (2.0 / alpha) * (lo - realized);
    } else if realized > hi {
        score += (2.0 / alpha) * (realized - hi);
    }
    score
}

/// What the label pass changed.
pub struct LabelPassSummary {
    pub matured: Vec<MaturedNote>,
    pub pending_coverage: Vec<String>,
    pub changed: HashSet<String>,
}

/// The deterministic label pass (`docs/portfolio-workflow.md §Step 7a`): for every
/// active episode, compute any newly due window labels after refreshing the
/// episode symbol's series through the window end via the shared price-bar cache —
/// independently of the current holdings work-list, so an exited name refreshes
/// too. A window past the shared grace closes typed rather than pending forever;
/// an episode with every window recorded freezes into the matured archive. Every
/// retrieval failure is fail-soft: the label stays pending, never a run failure.
pub fn mature_labels(
    episodes: &mut [DecisionEpisode],
    ctx: &mut SeriesCtx<'_>,
    today: NaiveDate,
    run_date: &str,
) -> LabelPassSummary {
    let mut summary = LabelPassSummary {
        matured: Vec::new(),
        pending_coverage: Vec::new(),
        changed: HashSet::new(),
    };
    for ep in episodes.iter_mut() {
        if ep.state != EpisodeState::Active {
            continue;
        }
        let Some(anchor) = ep.anchor_date() else {
            continue;
        };
        let mut changed = false;
        // The furthest window this pass will read, so the symbol is fetched once.
        let due_ends: Vec<NaiveDate> = ep
            .labels
            .iter()
            .filter(|l| matches!(l.outcome, LabelOutcome::Pending))
            .filter_map(|l| NaiveDate::parse_from_str(&l.window_end, "%Y-%m-%d").ok())
            .filter(|end| *end <= today)
            .collect();
        let Some(furthest) = due_ends.iter().max().copied() else {
            continue;
        };
        let closes = ctx.series(&ep.symbol, anchor, furthest).to_vec();
        let entry = first_close_after(&closes, anchor).cloned();
        // One dividends pull per episode serves every scoring window (the
        // furthest due end bounds the span) — never one request per window.
        let mut episode_divs: Option<std::result::Result<Vec<DatedValue>, String>> = None;

        // The sector benchmark is entry-stamped; an unscorable identity types the
        // sector legs immediately and never blocks scoring.
        let sector_bench = ep.sector.benchmark.clone();
        let sector_gap = ep.sector.unscorable.clone();
        let bear_line_12m = match &ep.body {
            EpisodeBody::Priced(p) => p
                .snapshot
                .price_targets
                .twelve_month
                .as_ref()
                .map(|t| t.bear),
            EpisodeBody::RoleRiskOnly(_) => None,
        };

        for i in 0..ep.labels.len() {
            if !matches!(ep.labels[i].outcome, LabelOutcome::Pending) {
                continue;
            }
            let Ok(w_end) = NaiveDate::parse_from_str(&ep.labels[i].window_end, "%Y-%m-%d") else {
                continue;
            };
            if w_end > today {
                continue;
            }
            let holding_covered = entry.is_some() && covers_through(&closes, w_end);
            let past_grace = today > w_end + chrono::Duration::days(PRICE_COVERAGE_GRACE_DAYS);
            if !holding_covered {
                if past_grace {
                    // The grace doubles as the transient-vs-disappearance
                    // discriminator: a series that once resolved but stopped is
                    // conservatively terminal; one that never resolved takes the
                    // price-coverage state.
                    let outcome = if closes.is_empty() {
                        LabelOutcome::PriceCoverageUnscorable
                    } else {
                        LabelOutcome::TerminalUnscorable
                    };
                    summary.matured.push(MaturedNote {
                        symbol: ep.symbol.clone(),
                        episode_id: ep.episode_id.clone(),
                        window_months: ep.labels[i].window_months,
                        outcome: match outcome {
                            LabelOutcome::PriceCoverageUnscorable => {
                                "price-coverage-unscorable".to_string()
                            }
                            _ => "terminal-unscorable".to_string(),
                        },
                        total_return: None,
                        price_return: None,
                    });
                    ep.labels[i].outcome = outcome;
                    changed = true;
                } else {
                    summary.pending_coverage.push(ep.symbol.clone());
                }
                continue;
            }
            let entry = entry.as_ref().expect("holding_covered implies entry");
            let Some(end_bar) = close_at_or_before(&closes, w_end) else {
                continue;
            };
            // Benchmark legs follow the same coverage rule: an uncovered
            // resolvable leg holds the whole window pending within grace, then
            // scores with the leg typed unavailable past it.
            let market_series = ctx.series(MARKET_BENCHMARK, anchor, w_end).to_vec();
            let market_ret = bench_return(&market_series, anchor, w_end);
            let sector_series =
                sector_bench.as_ref().map(|b| ctx.series(b, anchor, w_end).to_vec());
            let sector_ret = sector_series
                .as_ref()
                .and_then(|s| bench_return(s, anchor, w_end));
            let market_pending = market_ret.is_none() && !past_grace;
            let sector_pending =
                sector_bench.is_some() && sector_ret.is_none() && !past_grace;
            if market_pending || sector_pending {
                summary.pending_coverage.push(ep.symbol.clone());
                continue;
            }

            let price_return = end_bar.value / entry.value - 1.0;
            let entry_date = parse_iso_date_prefix(&entry.date).unwrap_or(anchor);
            let symbol = ep.symbol.clone();
            let divs_result = episode_divs.get_or_insert_with(|| match ctx.source {
                Some(s) => s
                    .dividend_history(&symbol, entry_date, furthest)
                    .map_err(|e| e.to_string()),
                None => Err("no price source".to_string()),
            });
            let end_iso = w_end.format("%Y-%m-%d").to_string();
            let (total_return, total_return_gap) = match divs_result {
                Ok(rows) => {
                    let paid: f64 = rows
                        .iter()
                        .filter(|d| d.date.as_str() <= end_iso.as_str())
                        .map(|d| d.value)
                        .sum();
                    (Some((end_bar.value + paid) / entry.value - 1.0), None)
                }
                Err(e) => (
                    None,
                    Some(format!("total-return leg unavailable ({e}) — price-only label")),
                ),
            };
            let max_drawdown = drawdown_over(&closes, &entry.date, w_end);
            let scored = ScoredLabel {
                entry_date: entry.date.clone(),
                entry_price: entry.value,
                end_date: end_bar.date.clone(),
                end_price: end_bar.value,
                price_return,
                total_return,
                total_return_gap,
                max_drawdown,
                vs_market: market_ret.map(|m| price_return - m),
                market_leg_gap: market_ret
                    .is_none()
                    .then(|| "market benchmark leg never covered the window".to_string()),
                vs_sector: sector_ret.map(|s| price_return - s),
                sector_leg_gap: sector_gap.clone().or_else(|| {
                    (sector_bench.is_some() && sector_ret.is_none())
                        .then(|| "sector benchmark leg never covered the window".to_string())
                }),
                labeled_at: run_date.to_string(),
            };
            summary.matured.push(MaturedNote {
                symbol: ep.symbol.clone(),
                episode_id: ep.episode_id.clone(),
                window_months: ep.labels[i].window_months,
                outcome: "scored".to_string(),
                total_return: scored.total_return,
                price_return: Some(scored.price_return),
            });
            // The falsifier lead-time read stamps when the 12-month window scores
            // — fields already frozen at the episode's intrinsic vintage: the
            // recorded twelve-month bear target is the material-drawdown line.
            if ep.labels[i].window_months == 12 {
                if let Some(bear) = bear_line_12m {
                    stamp_lead_times(
                        &mut ep.falsifier_events,
                        &closes,
                        &entry.date,
                        w_end,
                        bear,
                    );
                }
            }
            ep.labels[i].outcome = LabelOutcome::Scored(Box::new(scored));
            changed = true;
        }
        if changed {
            if ep.fully_labeled() {
                ep.state = EpisodeState::Matured;
            }
            summary.changed.insert(ep.episode_id.clone());
        }
    }
    summary.pending_coverage.sort();
    summary.pending_coverage.dedup();
    summary
}

/// A benchmark's own price-only window return, on its own next-session entry
/// anchor. `None` when the series doesn't cover the window.
fn bench_return(closes: &[DatedValue], anchor: NaiveDate, w_end: NaiveDate) -> Option<f64> {
    if !covers_through(closes, w_end) {
        return None;
    }
    let entry = first_close_after(closes, anchor)?;
    let end = close_at_or_before(closes, w_end)?;
    Some(end.value / entry.value - 1.0)
}

/// Maximum drawdown (≤ 0) over the closes from the entry bar through the window
/// end.
fn drawdown_over(closes: &[DatedValue], entry_date: &str, w_end: NaiveDate) -> f64 {
    let end_iso = w_end.format("%Y-%m-%d").to_string();
    let mut peak = f64::MIN;
    let mut worst = 0.0f64;
    for bar in closes {
        if bar.date.as_str() < entry_date || bar.date.as_str() > end_iso.as_str() {
            continue;
        }
        peak = peak.max(bar.value);
        if peak > 0.0 {
            worst = worst.min(bar.value / peak - 1.0);
        }
    }
    worst
}

/// Stamp each unstamped, in-episode falsifier event's signed trading-day distance
/// to the first within-window close below the bear-case line — deterministic, never
/// interpretive. Positive = confirmed before the breach; explicit
/// `no-material-drawdown` when no such close occurs by maturity.
fn stamp_lead_times(
    events: &mut [FalsifierEvent],
    closes: &[DatedValue],
    entry_date: &str,
    w_end: NaiveDate,
    bear_line: f64,
) {
    let end_iso = w_end.format("%Y-%m-%d").to_string();
    let window: Vec<&DatedValue> = closes
        .iter()
        .filter(|b| b.date.as_str() >= entry_date && b.date.as_str() <= end_iso.as_str())
        .collect();
    let breach_idx = window.iter().position(|b| b.value < bear_line);
    for ev in events.iter_mut() {
        if ev.post_maturity || ev.lead_time_trading_days.is_some() || ev.no_material_drawdown.is_some()
        {
            continue;
        }
        match breach_idx {
            None => ev.no_material_drawdown = Some(true),
            Some(bi) => {
                // The confirmation's position: the first bar at or after the
                // confirmation date (clamped into the window).
                let ci = window
                    .iter()
                    .position(|b| b.date.as_str() >= ev.confirmed_at.as_str())
                    .unwrap_or(window.len().saturating_sub(1));
                ev.lead_time_trading_days = Some(bi as i64 - ci as i64);
                ev.no_material_drawdown = Some(false);
            }
        }
    }
}

// ---- Alignment tagging ---------------------------------------------------------------

/// The deterministic net-alignment mapping, from the recommended action and the
/// diff's observed net move. The full table (pinned by tests):
///
/// | action        | reversed | exited   | increased | decreased | unchanged | new     |
/// |---------------|----------|----------|-----------|-----------|-----------|---------|
/// | hold          | reversed | contrary | contrary  | contrary  | aligned   | unknown |
/// | add family    | reversed | contrary | aligned   | contrary  | partial   | unknown |
/// | trim          | reversed | aligned  | contrary  | aligned   | partial   | unknown |
/// | sell-all      | reversed | aligned  | contrary  | partial   | partial   | unknown |
///
/// `partial` claims either a move in the recommended direction short of it
/// (sell-all → decreased) or no observable move under a directional
/// recommendation; `unknown` = the diff had no prior counterpart to classify
/// against (defensive — an episode's anchor run always had one).
pub fn net_alignment(
    action: Action,
    change: PositionChange,
    exited: bool,
    reversed: bool,
) -> ObservedNetAlignment {
    use ObservedNetAlignment as A;
    if reversed {
        return A::Reversed;
    }
    if exited {
        return if action.is_exit_family() {
            A::Aligned
        } else {
            A::Contrary
        };
    }
    match (action, change) {
        (_, PositionChange::New) => A::Unknown,
        (Action::Hold, PositionChange::Unchanged) => A::Aligned,
        (Action::Hold, _) => A::Contrary,
        (Action::Add | Action::AddAggressively, PositionChange::Increased) => A::Aligned,
        (Action::Add | Action::AddAggressively, PositionChange::Decreased) => A::Contrary,
        (Action::Add | Action::AddAggressively, PositionChange::Unchanged) => A::Partial,
        (Action::Trim, PositionChange::Decreased) => A::Aligned,
        (Action::Trim, PositionChange::Increased) => A::Contrary,
        (Action::Trim, PositionChange::Unchanged) => A::Partial,
        (Action::SellAll, PositionChange::Decreased | PositionChange::Unchanged) => A::Partial,
        (Action::SellAll, PositionChange::Increased) => A::Contrary,
    }
}

/// Tag each still-untagged active episode anchored to the prior run with this
/// run's observed net alignment (`docs/portfolio-analysis.md §Outcome learning` —
/// "the next run's deterministic holdings diff tags the holding's active
/// episode"). Episodes anchored earlier stay untagged: this diff observes only the
/// prior-run → now move, so tagging an older anchor would claim an observation the
/// diff never made.
pub fn tag_alignment(
    episodes: &mut [DecisionEpisode],
    prior_run_id: Option<&str>,
    holdings: &Holdings,
    diff: &HoldingsDiff,
) -> (Vec<AlignmentTag>, HashSet<String>) {
    let mut tags = Vec::new();
    let mut changed = HashSet::new();
    let Some(prior_id) = prior_run_id else {
        return (tags, changed);
    };
    let exited: HashSet<String> = diff
        .exited
        .iter()
        .map(|e| e.symbol.to_ascii_uppercase())
        .collect();
    for ep in episodes.iter_mut() {
        if ep.state != EpisodeState::Active
            || ep.alignment.is_some()
            || ep.anchor_run_id != prior_id
        {
            continue;
        }
        let key = ep.symbol.to_ascii_uppercase();
        let position = holdings
            .positions
            .iter()
            .find(|p| p.symbol.eq_ignore_ascii_case(&ep.symbol));
        let tag = match position {
            None => net_alignment(ep.body_action(), PositionChange::Unchanged, exited.contains(&key), false),
            Some(p) => {
                let delta = diff.delta_for(&p.symbol);
                net_alignment(
                    ep.body_action(),
                    delta.change,
                    false,
                    delta.side_reversed(p.quantity),
                )
            }
        };
        ep.alignment = Some(tag);
        tags.push(AlignmentTag {
            symbol: ep.symbol.clone(),
            episode_id: ep.episode_id.clone(),
            alignment: tag,
        });
        changed.insert(ep.episode_id.clone());
    }
    (tags, changed)
}

// ---- Episode lifecycle ----------------------------------------------------------------

/// The comparable recommendation state of one verdict — the episode-creation key.
/// The standing-thesis leg is deliberately absent (dormant until the 6g
/// attribution validator lands), and the target-weight half reads the **ledger's**
/// pre-committed range, never the sizing band (which is recomputed engine context
/// every run — input movement, not a decision).
#[derive(Debug, Clone, PartialEq)]
enum RecState {
    Priced {
        action: Action,
        weights: Option<(f64, f64)>,
    },
    RoleRisk {
        action: Action,
        weights: Option<(f64, f64)>,
    },
    /// An insufficient-evidence exit — the standing recommendation is retained,
    /// so an abstention is never a state change (and never opens).
    Abstained,
    /// Not-rated (or no verdict): outside the episode machinery.
    None,
}

fn ledger_weights(v: &HoldingVerdict) -> Option<(f64, f64)> {
    v.thesis_ledger
        .as_ref()
        .map(|l| (l.target_weight_low, l.target_weight_high))
}

fn rec_state(v: &HoldingVerdict) -> RecState {
    match &v.disposition {
        VerdictDisposition::Priced(g) => RecState::Priced {
            action: g.action,
            weights: ledger_weights(v),
        },
        VerdictDisposition::RoleRiskOnly(r) => RecState::RoleRisk {
            action: r.action,
            weights: ledger_weights(v),
        },
        VerdictDisposition::InsufficientEvidence { .. } => RecState::Abstained,
        VerdictDisposition::NotRated { .. } => RecState::None,
    }
}

/// What this run does to a holding's episode stream.
#[derive(Debug, Clone, PartialEq)]
pub enum EpisodeDecision {
    Open(Vec<OpenReason>),
    Extend(ObservationKind),
    Nothing,
}

/// Decide open / extend / nothing from the prior run's verdict and this run's
/// (`docs/portfolio-analysis.md §Outcome learning` — the creation rule, narrowed
/// to observable state changes). An abstention always extends (the standing
/// recommendation stands); a prior abstention compares on what its retained ledger
/// still carries (branch + weight range — the action was not re-authored).
pub fn episode_decision(
    prior: Option<&HoldingVerdict>,
    current: &HoldingVerdict,
    current_is_fresh: bool,
) -> EpisodeDecision {
    let cur = rec_state(current);
    let extend_kind = if !current_is_fresh {
        ObservationKind::Carried
    } else {
        ObservationKind::Reaffirmed
    };
    match cur {
        RecState::None => EpisodeDecision::Nothing,
        RecState::Abstained => EpisodeDecision::Extend(ObservationKind::Abstained),
        RecState::Priced { .. } | RecState::RoleRisk { .. } => {
            let Some(prior_v) = prior else {
                return EpisodeDecision::Open(vec![OpenReason::Debut]);
            };
            match rec_state(prior_v) {
                RecState::None => EpisodeDecision::Open(vec![OpenReason::Debut]),
                RecState::Abstained => {
                    // The abstained prior retained the standing ledger: branch and
                    // weight range are still comparable; the action is not.
                    let prior_branch_priced = matches!(
                        prior_v.thesis_ledger.as_ref().map(|l| l.branch),
                        Some(crate::portfolio::LedgerBranch::Priced) | None
                    );
                    let cur_priced = matches!(cur, RecState::Priced { .. });
                    let cur_weights = match &cur {
                        RecState::Priced { weights, .. } | RecState::RoleRisk { weights, .. } => {
                            *weights
                        }
                        _ => None,
                    };
                    let mut reasons = Vec::new();
                    if prior_branch_priced != cur_priced {
                        reasons.push(OpenReason::BranchFlip);
                    }
                    if ledger_weights(prior_v) != cur_weights {
                        reasons.push(OpenReason::WeightRangeChange);
                    }
                    if reasons.is_empty() {
                        EpisodeDecision::Extend(extend_kind)
                    } else {
                        EpisodeDecision::Open(reasons)
                    }
                }
                prior_state => {
                    let mut reasons = Vec::new();
                    match (&prior_state, &cur) {
                        (RecState::Priced { .. }, RecState::RoleRisk { .. })
                        | (RecState::RoleRisk { .. }, RecState::Priced { .. }) => {
                            reasons.push(OpenReason::BranchFlip);
                        }
                        _ => {}
                    }
                    let (prior_action, prior_weights) = match prior_state {
                        RecState::Priced { action, weights }
                        | RecState::RoleRisk { action, weights } => (action, weights),
                        _ => unreachable!(),
                    };
                    let (cur_action, cur_weights) = match cur {
                        RecState::Priced { action, weights }
                        | RecState::RoleRisk { action, weights } => (action, weights),
                        _ => unreachable!(),
                    };
                    if prior_action != cur_action {
                        reasons.push(OpenReason::ActionChange);
                        if current.action_source == ActionSource::RuleDemoted {
                            reasons.push(OpenReason::RuleDemotion);
                        }
                    }
                    if prior_weights != cur_weights {
                        reasons.push(OpenReason::WeightRangeChange);
                    }
                    if reasons.is_empty() {
                        EpisodeDecision::Extend(extend_kind)
                    } else {
                        EpisodeDecision::Open(reasons)
                    }
                }
            }
        }
    }
}

/// Inputs to the pure per-run episode planning.
pub struct PlanInput<'a> {
    pub run_id: &'a str,
    pub created_at: &'a str,
    pub verdicts: &'a [HoldingVerdict],
    pub audits: &'a [HoldingAudit],
    pub prior_verdicts: Option<&'a [HoldingVerdict]>,
    /// Sector identities read at this run's fresh passes (keyed uppercase).
    pub sector_by_symbol: &'a HashMap<String, SectorIdentity>,
    /// The run-level DGS2 print, for the snapshot.
    pub dgs2: Option<f64>,
}

/// What the plan changed.
pub struct PlanSummary {
    pub opened: Vec<OpenedEpisodeNote>,
    pub extended: Vec<String>,
    pub changed: HashSet<String>,
}

/// Append-or-extend this run's decision episodes in place
/// (`docs/portfolio-workflow.md §Step 8`): open on an observable
/// recommendation-state change, extend the active episode on a re-affirmation /
/// carry / abstention, record nothing post-maturity, and attach this run's
/// confirmed falsifier crossings to the episode carrying the condition (the latest
/// matured episode, typed post-maturity, when none is active).
pub fn plan_episodes(input: &PlanInput<'_>, episodes: &mut Vec<DecisionEpisode>) -> PlanSummary {
    let mut summary = PlanSummary {
        opened: Vec::new(),
        extended: Vec::new(),
        changed: HashSet::new(),
    };
    for verdict in input.verdicts {
        let key = verdict.symbol.to_ascii_uppercase();
        let prior_v = input
            .prior_verdicts
            .and_then(|pv| pv.iter().find(|v| v.symbol.eq_ignore_ascii_case(&verdict.symbol)));
        let vintage = verdict.analyzed_at.as_deref().unwrap_or(input.created_at);
        let is_fresh = vintage == input.created_at;
        match episode_decision(prior_v, verdict, is_fresh) {
            EpisodeDecision::Nothing => {}
            EpisodeDecision::Extend(kind) => {
                if let Some(ep) = episodes
                    .iter_mut()
                    .find(|e| e.state == EpisodeState::Active && e.symbol.eq_ignore_ascii_case(&verdict.symbol))
                {
                    ep.observations.push(EpisodeObservation {
                        run_id: input.run_id.to_string(),
                        observed_at: input.created_at.to_string(),
                        kind,
                    });
                    summary.extended.push(verdict.symbol.clone());
                    summary.changed.insert(ep.episode_id.clone());
                }
                // No active episode: a post-maturity re-affirmation records
                // nothing — there is no open forecast left to observe against.
            }
            EpisodeDecision::Open(reasons) => {
                let audit = input
                    .audits
                    .iter()
                    .find(|a| a.symbol.eq_ignore_ascii_case(&verdict.symbol));
                let sector = input
                    .sector_by_symbol
                    .get(&key)
                    .cloned()
                    .or_else(|| {
                        // A carried-verdict open (the rule-demotion path) had no
                        // fresh profile read; the symbol's latest episode carries
                        // the entry-stamped identity to inherit.
                        episodes
                            .iter()
                            .filter(|e| e.symbol.eq_ignore_ascii_case(&verdict.symbol))
                            .max_by(|a, b| a.anchor_at.cmp(&b.anchor_at))
                            .map(|e| e.sector.clone())
                    })
                    .unwrap_or_else(|| {
                        SectorIdentity::unscorable("no sector read at the anchor run")
                    });
                let Some(anchor) = parse_iso_date_prefix(input.created_at) else {
                    continue;
                };
                let body = match &verdict.disposition {
                    VerdictDisposition::Priced(g) => EpisodeBody::Priced(Box::new(PricedEpisode {
                        action: g.action,
                        lean: g.action,
                        lean_divergence: None,
                        target_weight_low: ledger_weights(verdict).map(|w| w.0),
                        target_weight_high: ledger_weights(verdict).map(|w| w.1),
                        snapshot: CalibrationSnapshot {
                            sub_scores: g.sub_scores,
                            grade: g.grade,
                            conviction: g.conviction,
                            risk_tier: g.risk_tier,
                            price_targets: g.price_targets.clone(),
                            dead_money: g.dead_money,
                            hurdle: audit.and_then(|a| a.hurdle.clone()),
                            dgs2: input.dgs2,
                            cap_signals: audit
                                .and_then(|a| a.pre_profit.as_ref())
                                .filter(|pp| pp.is_eligible())
                                .map(|pp| pp.consequences.matched_rules.clone())
                                .unwrap_or_default(),
                            grade_parameter_version: audit
                                .and_then(|a| a.grade_parameter_version.clone()),
                            target_parameter_version: audit
                                .and_then(|a| a.target_meta.as_ref())
                                .map(|t| t.parameter_version.clone()),
                            degraded_inputs: audit
                                .map(|a| a.degraded_inputs.clone())
                                .unwrap_or_default(),
                        },
                    })),
                    VerdictDisposition::RoleRiskOnly(r) => {
                        EpisodeBody::RoleRiskOnly(RoleRiskEpisode {
                            action: r.action,
                            target_weight_low: ledger_weights(verdict).map(|w| w.0),
                            target_weight_high: ledger_weights(verdict).map(|w| w.1),
                            degraded_inputs: audit
                                .map(|a| a.degraded_inputs.clone())
                                .unwrap_or_default(),
                        })
                    }
                    // `Open` is only decided for analyzed dispositions.
                    _ => continue,
                };
                let episode = DecisionEpisode {
                    episode_id: uuid::Uuid::new_v4().to_string(),
                    symbol: verdict.symbol.clone(),
                    anchor_run_id: input.run_id.to_string(),
                    anchor_at: input.created_at.to_string(),
                    intrinsic_vintage: vintage.to_string(),
                    vintage_fresh: is_fresh,
                    action_source: verdict.action_source,
                    position_change: verdict.position_change,
                    sector,
                    opened: reasons.clone(),
                    body,
                    observations: Vec::new(),
                    alignment: None,
                    falsifier_events: Vec::new(),
                    labels: pending_labels(anchor),
                    state: EpisodeState::Active,
                    self_correction_count: 0,
                };
                summary.opened.push(OpenedEpisodeNote {
                    symbol: verdict.symbol.clone(),
                    episode_id: episode.episode_id.clone(),
                    reasons,
                });
                summary.changed.insert(episode.episode_id.clone());
                episodes.push(episode);
            }
        }
    }

    // This run's confirmed falsifier crossings attach to the episode carrying the
    // condition. With no active episode the event lands on the latest matured
    // episode typed post-maturity — retained as context for the next episode,
    // feeding no lead-time read.
    for audit in input.audits {
        let Some(ledger_audit) = &audit.ledger_audit else {
            continue;
        };
        for crossing in &ledger_audit.crossings {
            if crossing.role != crate::portfolio::ConditionRole::Falsifier
                || crossing.outcome != crate::portfolio::CrossingOutcome::Confirmed
            {
                continue;
            }
            let confirmed_at: String = input.created_at.chars().take(10).collect();
            let (target, post_maturity) = {
                let active = episodes
                    .iter_mut()
                    .position(|e| {
                        e.state == EpisodeState::Active
                            && e.symbol.eq_ignore_ascii_case(&audit.symbol)
                    });
                match active {
                    Some(i) => (Some(i), false),
                    None => (
                        episodes
                            .iter()
                            .enumerate()
                            .filter(|(_, e)| {
                                e.state == EpisodeState::Matured
                                    && e.symbol.eq_ignore_ascii_case(&audit.symbol)
                            })
                            .max_by(|(_, a), (_, b)| a.anchor_at.cmp(&b.anchor_at))
                            .map(|(i, _)| i),
                        true,
                    ),
                }
            };
            let Some(i) = target else { continue };
            let ep = &mut episodes[i];
            let duplicate = ep.falsifier_events.iter().any(|e| {
                e.condition_id == crossing.condition_id
                    && e.confirmation_observation_id == crossing.observation_id
            });
            if duplicate {
                continue;
            }
            ep.falsifier_events.push(FalsifierEvent {
                condition_id: crossing.condition_id.clone(),
                confirmed_at,
                confirmation_observation_id: crossing.observation_id.clone(),
                post_maturity,
                lead_time_trading_days: None,
                no_material_drawdown: None,
            });
            summary.changed.insert(ep.episode_id.clone());
        }
    }
    summary
}

// ---- Derived reads ---------------------------------------------------------------------

/// Per-episode scored label for a window, where recorded.
fn scored_for(ep: &DecisionEpisode, months: u32) -> Option<&ScoredLabel> {
    ep.labels
        .iter()
        .find(|l| l.window_months == months)
        .and_then(|l| match &l.outcome {
            LabelOutcome::Scored(s) => Some(s.as_ref()),
            _ => None,
        })
}

/// Group episodes into one cohort stat: per-symbol means first, then across
/// symbols — unique-holding counted, never raw episode counts.
fn cohort_stat(key: &str, members: &[(&DecisionEpisode, &ScoredLabel)]) -> Option<CohortStat> {
    if members.is_empty() {
        return None;
    }
    let mut per_symbol: HashMap<String, Vec<&ScoredLabel>> = HashMap::new();
    for (ep, label) in members {
        per_symbol
            .entry(ep.symbol.to_ascii_uppercase())
            .or_default()
            .push(label);
    }
    let mut tr = Vec::new();
    let mut pr = Vec::new();
    let mut vm = Vec::new();
    let mut vs = Vec::new();
    for labels in per_symbol.values() {
        let mean = |xs: Vec<f64>| {
            if xs.is_empty() {
                None
            } else {
                Some(xs.iter().sum::<f64>() / xs.len() as f64)
            }
        };
        if let Some(v) = mean(labels.iter().filter_map(|l| l.total_return).collect()) {
            tr.push(v);
        }
        if let Some(v) = mean(labels.iter().map(|l| l.price_return).collect()) {
            pr.push(v);
        }
        if let Some(v) = mean(labels.iter().filter_map(|l| l.vs_market).collect()) {
            vm.push(v);
        }
        if let Some(v) = mean(labels.iter().filter_map(|l| l.vs_sector).collect()) {
            vs.push(v);
        }
    }
    let agg = |xs: &[f64]| {
        if xs.is_empty() {
            None
        } else {
            Some(xs.iter().sum::<f64>() / xs.len() as f64)
        }
    };
    Some(CohortStat {
        key: key.to_string(),
        unique_holdings: per_symbol.len(),
        mean_total_return: agg(&tr),
        mean_price_return: agg(&pr),
        mean_vs_market: agg(&vm),
        mean_vs_sector: agg(&vs),
    })
}

/// Compute the four derived scorecard reads over the episode store
/// (`docs/portfolio-analysis.md §Outcome learning`). Pure; attribution stays at the
/// cohort level — no per-decision P&L verdict is ever assigned.
pub fn derive_reads(episodes: &[DecisionEpisode]) -> DerivedReads {
    let mut cohorts = Vec::new();
    for &months in &LABEL_WINDOWS_MONTHS {
        let scored: Vec<(&DecisionEpisode, &ScoredLabel)> = episodes
            .iter()
            .filter_map(|ep| scored_for(ep, months).map(|s| (ep, s)))
            .collect();
        // The intrinsic layer: lean-keyed, vintage-fresh, model-chosen, priced.
        let mut lean_cohorts = Vec::new();
        let mut final_action_cohorts = Vec::new();
        for action in [
            Action::SellAll,
            Action::Trim,
            Action::Hold,
            Action::Add,
            Action::AddAggressively,
        ] {
            let lean_members: Vec<_> = scored
                .iter()
                .filter(|(ep, _)| {
                    ep.vintage_fresh
                        && ep.action_source == ActionSource::ModelChosen
                        && matches!(&ep.body, EpisodeBody::Priced(p) if p.lean == action)
                })
                .cloned()
                .collect();
            if let Some(stat) = cohort_stat(action.as_kebab(), &lean_members) {
                lean_cohorts.push(stat);
            }
            let final_members: Vec<_> = scored
                .iter()
                .filter(|(ep, _)| {
                    ep.action_source == ActionSource::ModelChosen
                        && matches!(&ep.body, EpisodeBody::Priced(p) if p.action == action)
                })
                .cloned()
                .collect();
            if let Some(stat) = cohort_stat(action.as_kebab(), &final_members) {
                final_action_cohorts.push(stat);
            }
        }
        let role_members: Vec<_> = scored
            .iter()
            .filter(|(ep, _)| matches!(ep.body, EpisodeBody::RoleRiskOnly(_)))
            .cloned()
            .collect();
        let demoted_members: Vec<_> = scored
            .iter()
            .filter(|(ep, _)| ep.action_source == ActionSource::RuleDemoted)
            .cloned()
            .collect();
        cohorts.push(CohortWindowRead {
            window_months: months,
            lean_cohorts,
            final_action_cohorts,
            role_risk: cohort_stat("role-risk-only", &role_members),
            rule_demoted: cohort_stat("rule-demoted", &demoted_members),
        });
    }

    // Target calibration: each band at its matching window, vintage-fresh bands
    // only, scored on the price-only label.
    let mut target_calibration = Vec::new();
    for months in [1u32, 12u32] {
        let mut scores = Vec::new();
        let mut hits = 0usize;
        let mut base_errors = Vec::new();
        for ep in episodes {
            if !ep.vintage_fresh {
                continue;
            }
            let EpisodeBody::Priced(p) = &ep.body else {
                continue;
            };
            let Some(label) = scored_for(ep, months) else {
                continue;
            };
            let band = match months {
                1 => p.snapshot.price_targets.one_month.as_ref(),
                _ => p.snapshot.price_targets.twelve_month.as_ref(),
            };
            let Some(band) = band else { continue };
            let (lo, hi) = (band.bear.min(band.bull), band.bear.max(band.bull));
            let realized = label.end_price;
            if realized >= lo && realized <= hi {
                hits += 1;
            }
            scores.push(interval_score(lo, hi, realized, 1.0 - NOMINAL_BAND_COVERAGE));
            if band.base != 0.0 {
                base_errors.push((realized - band.base) / band.base);
            }
        }
        let n = scores.len();
        target_calibration.push(TargetCalibrationRead {
            window_months: months,
            scored: n,
            coverage_rate: (n > 0).then(|| hits as f64 / n as f64),
            nominal_coverage: NOMINAL_BAND_COVERAGE,
            mean_interval_score: (n > 0).then(|| scores.iter().sum::<f64>() / n as f64),
            mean_base_signed_error: (!base_errors.is_empty())
                .then(|| base_errors.iter().sum::<f64>() / base_errors.len() as f64),
        });
    }

    let falsifier_lead_times = episodes
        .iter()
        .flat_map(|ep| {
            ep.falsifier_events
                .iter()
                .filter(|ev| !ev.post_maturity)
                .filter(|ev| {
                    ev.lead_time_trading_days.is_some() || ev.no_material_drawdown == Some(true)
                })
                .map(|ev| FalsifierLeadTimeRead {
                    symbol: ep.symbol.clone(),
                    episode_id: ep.episode_id.clone(),
                    condition_id: ev.condition_id.clone(),
                    confirmed_at: ev.confirmed_at.clone(),
                    lead_time_trading_days: ev.lead_time_trading_days,
                    no_material_drawdown: ev.no_material_drawdown == Some(true),
                })
        })
        .collect();

    let mut per_holding: HashMap<String, u32> = HashMap::new();
    for ep in episodes {
        if ep.self_correction_count > 0 {
            *per_holding.entry(ep.symbol.to_ascii_uppercase()).or_default() +=
                ep.self_correction_count;
        }
    }
    let total = per_holding.values().sum();
    let mut per_holding: Vec<(String, u32)> = per_holding.into_iter().collect();
    per_holding.sort();

    let unique_matured: HashSet<String> = episodes
        .iter()
        .filter(|ep| {
            LABEL_WINDOWS_MONTHS
                .iter()
                .any(|&m| scored_for(ep, m).is_some())
        })
        .map(|ep| ep.symbol.to_ascii_uppercase())
        .collect();
    let eligible = unique_matured.len() >= PROPOSAL_ELIGIBILITY_BAR;
    let eligibility = EligibilityRecord {
        unique_matured_holdings: unique_matured.len(),
        bar: PROPOSAL_ELIGIBILITY_BAR,
        eligible,
        note: if eligible {
            format!(
                "{} unique holdings with matured windows clear the ≥ {} bar — parameter \
                 proposals are eligible (the proposal statistics ride a later slice)",
                unique_matured.len(),
                PROPOSAL_ELIGIBILITY_BAR
            )
        } else {
            format!(
                "below the proposal eligibility bar ({} of ≥ {} unique holdings with \
                 matured windows) — no proposal is made from a small sample",
                unique_matured.len(),
                PROPOSAL_ELIGIBILITY_BAR
            )
        },
    };

    DerivedReads {
        cohorts,
        target_calibration,
        falsifier_lead_times,
        self_correction: SelfCorrectionRead { total, per_holding },
        eligibility,
    }
}

/// The durable-learning text for a run whose label pass recorded matured windows —
/// embedded into the Portfolio memory partition (`docs/portfolio-analysis.md`
/// §Outcome learning). `None` when nothing matured (no learning row is written).
pub fn matured_learning_text(records: &OutcomeRecords, run_date: &str) -> Option<String> {
    if records.matured.is_empty() {
        return None;
    }
    let mut lines = vec![format!(
        "Portfolio outcome learning ({run_date}): {} window label(s) recorded.",
        records.matured.len()
    )];
    for m in &records.matured {
        let detail = match (m.total_return, m.price_return) {
            (Some(tr), _) => format!("total return {:+.1}%", tr * 100.0),
            (None, Some(pr)) => format!("price-only return {:+.1}%", pr * 100.0),
            _ => m.outcome.clone(),
        };
        lines.push(format!(
            "{} {}-month window: {} ({})",
            m.symbol, m.window_months, m.outcome, detail
        ));
    }
    lines.push(records.reads.eligibility.note.clone());
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{
        ActionSizing, GradedVerdict, HorizonOutlook, HorizonRead, OptionsSignal, PriceTarget,
        ThesisLedger,
    };

    fn bars(rows: &[(&str, f64)]) -> Vec<DatedValue> {
        rows.iter()
            .map(|(d, v)| DatedValue {
                date: d.to_string(),
                value: *v,
            })
            .collect()
    }

    fn graded(action: Action) -> GradedVerdict {
        GradedVerdict {
            grade: Grade::B,
            sub_scores: SubScores {
                quality: 70.0,
                valuation: 60.0,
                momentum: 55.0,
                risk: 65.0,
            },
            action,
            action_sizing: ActionSizing {
                target_weight_low: 0.04,
                target_weight_high: 0.06,
                est_share_delta: None,
                est_dollar_delta: None,
            },
            conviction: Conviction::Medium,
            horizon_outlook: HorizonOutlook {
                short: HorizonRead::Neutral,
                mid: HorizonRead::Bullish,
                long: HorizonRead::Bullish,
            },
            price_targets: PriceTargets {
                one_month: Some(PriceTarget {
                    base: 102.0,
                    bear: 95.0,
                    bull: 108.0,
                    methodology: "test".into(),
                }),
                twelve_month: Some(PriceTarget {
                    base: 120.0,
                    bear: 90.0,
                    bull: 150.0,
                    methodology: "test".into(),
                }),
            },
            price_target_rationale: "test".into(),
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
            financial_summary: "fine".into(),
            what_changed: "new holding".into(),
        }
    }

    fn ledger(low: f64, high: f64) -> ThesisLedger {
        ThesisLedger {
            branch: crate::portfolio::LedgerBranch::Priced,
            original_thesis: "t".into(),
            current_thesis: "t".into(),
            key_drivers: vec![],
            monitor: vec![],
            what_must_improve: String::new(),
            what_must_not_break: String::new(),
            conditions: vec![],
            target_weight_low: low,
            target_weight_high: high,
            authored_band_relation: None,
        }
    }

    fn verdict(symbol: &str, action: Action, weights: (f64, f64)) -> HoldingVerdict {
        HoldingVerdict {
            symbol: symbol.into(),
            asset_class: crate::portfolio::AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::Priced(Box::new(graded(action))),
            thesis_ledger: Some(ledger(weights.0, weights.1)),
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
        }
    }

    fn fresh(mut v: HoldingVerdict, created_at: &str) -> HoldingVerdict {
        v.analyzed_at = Some(created_at.to_string());
        v
    }

    fn plan_input<'a>(
        run_id: &'a str,
        created_at: &'a str,
        verdicts: &'a [HoldingVerdict],
        prior: Option<&'a [HoldingVerdict]>,
        sector: &'a HashMap<String, SectorIdentity>,
    ) -> PlanInput<'a> {
        PlanInput {
            run_id,
            created_at,
            verdicts,
            audits: &[],
            prior_verdicts: prior,
            sector_by_symbol: sector,
            dgs2: Some(0.04),
        }
    }

    // ---- SPDR map / sector identity ----

    #[test]
    fn spdr_map_covers_the_eleven_sectors_and_rejects_unknowns() {
        assert_eq!(spdr_for_sector("Technology"), Some("XLK"));
        assert_eq!(spdr_for_sector("consumer cyclical"), Some("XLY"));
        assert_eq!(spdr_for_sector("Health Care"), Some("XLV"));
        assert_eq!(spdr_for_sector("Healthcare"), Some("XLV"));
        assert_eq!(spdr_for_sector("Financial Services"), Some("XLF"));
        assert_eq!(spdr_for_sector("Space Mining"), None);
        let id = SectorIdentity::resolve(Some("Technology"));
        assert_eq!(id.benchmark.as_deref(), Some("XLK"));
        assert!(id.unscorable.is_none());
        let un = SectorIdentity::resolve(None);
        assert!(un.benchmark.is_none());
        assert!(un.unscorable.as_deref().unwrap().contains("sector-unscorable"));
    }

    // ---- Lifecycle: the transition matrix ----

    #[test]
    fn a_debut_opens_and_a_reaffirmation_extends_once() {
        let created = "2026-08-04T12:00:00+00:00";
        let verdicts = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), created)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        let s = plan_episodes(&plan_input("run-1", created, &verdicts, None, &sector), &mut episodes);
        assert_eq!(s.opened.len(), 1);
        assert_eq!(s.opened[0].reasons, vec![OpenReason::Debut]);
        assert_eq!(episodes.len(), 1);
        assert!(episodes[0].vintage_fresh);
        assert_eq!(episodes[0].labels.len(), 4);

        // Same recommendation next run: extend, never a second episode.
        let created2 = "2026-08-11T12:00:00+00:00";
        let verdicts2 = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), created2)];
        let s2 = plan_episodes(
            &plan_input("run-2", created2, &verdicts2, Some(&verdicts), &sector),
            &mut episodes,
        );
        assert!(s2.opened.is_empty());
        assert_eq!(s2.extended, vec!["AAPL".to_string()]);
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].observations.len(), 1);
        assert_eq!(episodes[0].observations[0].kind, ObservationKind::Reaffirmed);
    }

    #[test]
    fn an_action_change_and_a_weight_change_open_fresh_episodes() {
        let c1 = "2026-08-04T12:00:00+00:00";
        let prior = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &prior, None, &sector), &mut episodes);

        let c2 = "2026-08-11T12:00:00+00:00";
        let action_changed = vec![fresh(verdict("AAPL", Action::Trim, (0.03, 0.06)), c2)];
        let s = plan_episodes(
            &plan_input("run-2", c2, &action_changed, Some(&prior), &sector),
            &mut episodes,
        );
        assert_eq!(s.opened.len(), 1);
        assert_eq!(s.opened[0].reasons, vec![OpenReason::ActionChange]);

        let c3 = "2026-08-18T12:00:00+00:00";
        let weight_changed = vec![fresh(verdict("AAPL", Action::Trim, (0.02, 0.04)), c3)];
        let s = plan_episodes(
            &plan_input("run-3", c3, &weight_changed, Some(&action_changed), &sector),
            &mut episodes,
        );
        assert_eq!(s.opened.len(), 1);
        assert_eq!(s.opened[0].reasons, vec![OpenReason::WeightRangeChange]);
        assert_eq!(episodes.len(), 3);
    }

    #[test]
    fn an_abstention_extends_and_never_opens() {
        let c1 = "2026-08-04T12:00:00+00:00";
        let prior = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &prior, None, &sector), &mut episodes);

        let c2 = "2026-08-11T12:00:00+00:00";
        let mut abstained = verdict("AAPL", Action::Hold, (0.03, 0.06));
        abstained.disposition = VerdictDisposition::InsufficientEvidence {
            reason: "thin".into(),
        };
        abstained.analyzed_at = Some(c1.to_string()); // preserved prior vintage
        let s = plan_episodes(
            &plan_input("run-2", c2, &[abstained], Some(&prior), &sector),
            &mut episodes,
        );
        assert!(s.opened.is_empty());
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].observations[0].kind, ObservationKind::Abstained);
    }

    #[test]
    fn post_maturity_reaffirmation_records_nothing_and_a_change_reopens() {
        let c1 = "2025-06-02T12:00:00+00:00";
        let prior = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &prior, None, &sector), &mut episodes);
        episodes[0].state = EpisodeState::Matured;

        let c2 = "2026-08-11T12:00:00+00:00";
        let same = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c2)];
        let s = plan_episodes(
            &plan_input("run-2", c2, &same, Some(&prior), &sector),
            &mut episodes,
        );
        assert!(s.opened.is_empty());
        assert!(s.extended.is_empty(), "no active episode: nothing records");
        assert!(episodes[0].observations.is_empty());

        let c3 = "2026-08-18T12:00:00+00:00";
        let changed = vec![fresh(verdict("AAPL", Action::Add, (0.03, 0.06)), c3)];
        let s = plan_episodes(
            &plan_input("run-3", c3, &changed, Some(&same), &sector),
            &mut episodes,
        );
        assert_eq!(s.opened.len(), 1, "the next genuine change opens fresh");
        assert_eq!(episodes.len(), 2);
    }

    #[test]
    fn a_rule_demotion_opens_a_vintage_stale_episode_in_its_own_class() {
        let c1 = "2026-07-01T12:00:00+00:00";
        let prior = vec![fresh(verdict("AAPL", Action::Add, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &prior, None, &sector), &mut episodes);

        // A selective run demotes the carried over-age add to hold; the carried
        // verdict keeps its older vintage.
        let c2 = "2026-08-11T12:00:00+00:00";
        let mut demoted = verdict("AAPL", Action::Hold, (0.03, 0.06));
        demoted.analyzed_at = Some(c1.to_string());
        demoted.action_source = ActionSource::RuleDemoted;
        let s = plan_episodes(
            &plan_input("run-2", c2, &[demoted], Some(&prior), &sector),
            &mut episodes,
        );
        assert_eq!(s.opened.len(), 1);
        assert!(s.opened[0].reasons.contains(&OpenReason::RuleDemotion));
        let ep = episodes.last().unwrap();
        assert!(!ep.vintage_fresh, "carried fields keep their older vintage");
        assert_eq!(ep.action_source, ActionSource::RuleDemoted);
        // It inherited the debut episode's sector identity (no fresh profile read).
        assert_eq!(ep.sector, episodes[0].sector);
    }

    #[test]
    fn confirmed_falsifier_crossings_attach_to_the_carrying_episode() {
        let c1 = "2026-08-04T12:00:00+00:00";
        let verdicts = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        let audit = HoldingAudit {
            symbol: "AAPL".into(),
            metrics: Default::default(),
            sources: vec![],
            model_ids: vec![],
            prompt_version: "portfolio-v5".into(),
            degraded_inputs: vec![],
            target_meta: None,
            grade_parameter_version: None,
            ledger_audit: Some(crate::portfolio::LedgerAudit {
                crossings: vec![crate::portfolio::ConditionCrossing {
                    condition_id: "c-1".into(),
                    statement: "margin below 15%".into(),
                    role: crate::portfolio::ConditionRole::Falsifier,
                    outcome: crate::portfolio::CrossingOutcome::Confirmed,
                    observed_value: 0.12,
                    threshold: 0.15,
                    observation_id: "2026-06-30".into(),
                }],
                ..Default::default()
            }),
            quick_basis: None,
            fund_exposure: None,
            pre_profit: None,
            hurdle: None,
        };
        let audits = vec![audit];
        let mut input = plan_input("run-1", c1, &verdicts, None, &sector);
        input.audits = &audits;
        plan_episodes(&input, &mut episodes);
        assert_eq!(episodes[0].falsifier_events.len(), 1);
        let ev = &episodes[0].falsifier_events[0];
        assert_eq!(ev.condition_id, "c-1");
        assert_eq!(ev.confirmation_observation_id, "2026-06-30");
        assert!(!ev.post_maturity);

        // Re-running the same crossing dedups; a matured-only symbol takes the
        // post-maturity form.
        plan_episodes(&input, &mut episodes);
        assert_eq!(episodes[0].falsifier_events.len(), 1, "deduplicated");
        episodes[0].state = EpisodeState::Matured;
        let mut input2 = plan_input("run-2", "2026-08-11T12:00:00+00:00", &[], None, &sector);
        input2.audits = &audits;
        plan_episodes(&input2, &mut episodes);
        assert_eq!(episodes[0].falsifier_events.len(), 1, "same observation dedups");
    }

    // ---- Alignment ----

    #[test]
    fn the_alignment_table_is_pinned() {
        use ObservedNetAlignment as A;
        use PositionChange as C;
        // Reversal dominates everything.
        assert_eq!(net_alignment(Action::Add, C::Increased, false, true), A::Reversed);
        // Exits.
        assert_eq!(net_alignment(Action::SellAll, C::Unchanged, true, false), A::Aligned);
        assert_eq!(net_alignment(Action::Trim, C::Unchanged, true, false), A::Aligned);
        assert_eq!(net_alignment(Action::Hold, C::Unchanged, true, false), A::Contrary);
        // Hold.
        assert_eq!(net_alignment(Action::Hold, C::Unchanged, false, false), A::Aligned);
        assert_eq!(net_alignment(Action::Hold, C::Increased, false, false), A::Contrary);
        // Add family.
        assert_eq!(net_alignment(Action::Add, C::Increased, false, false), A::Aligned);
        assert_eq!(net_alignment(Action::Add, C::Decreased, false, false), A::Contrary);
        assert_eq!(net_alignment(Action::Add, C::Unchanged, false, false), A::Partial);
        // Trim / sell.
        assert_eq!(net_alignment(Action::Trim, C::Decreased, false, false), A::Aligned);
        assert_eq!(net_alignment(Action::SellAll, C::Decreased, false, false), A::Partial);
        assert_eq!(net_alignment(Action::SellAll, C::Increased, false, false), A::Contrary);
        // No prior counterpart.
        assert_eq!(net_alignment(Action::Add, C::New, false, false), A::Unknown);
    }

    // ---- Label engine (pure pieces) ----

    #[test]
    fn window_end_clamps_calendar_months() {
        let jan31 = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        assert_eq!(window_end(jan31, 1), NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
        let mar1 = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        assert_eq!(window_end(mar1, 12), NaiveDate::from_ymd_opt(2027, 3, 1).unwrap());
    }

    #[test]
    fn interval_score_penalizes_exceedance_and_width() {
        // Inside the band: just the width.
        assert!((interval_score(90.0, 110.0, 100.0, 0.2) - 20.0).abs() < 1e-12);
        // Below: width + (2/alpha) * distance.
        assert!((interval_score(90.0, 110.0, 80.0, 0.2) - (20.0 + 100.0)).abs() < 1e-12);
        // A gamed-wide band pays for its width.
        assert!(interval_score(0.0, 1000.0, 100.0, 0.2) > interval_score(90.0, 110.0, 100.0, 0.2));
    }

    #[test]
    fn drawdown_is_computed_from_the_running_peak() {
        let closes = bars(&[
            ("2026-01-02", 100.0),
            ("2026-01-05", 110.0),
            ("2026-01-06", 88.0),
            ("2026-01-07", 95.0),
        ]);
        let dd = drawdown_over(&closes, "2026-01-02", NaiveDate::from_ymd_opt(2026, 1, 31).unwrap());
        assert!((dd - (88.0 / 110.0 - 1.0)).abs() < 1e-12);
    }

    // ---- The label pass over the cached-through series ----

    /// A synthetic source: linear weekday closes over `[from, to]`, per-symbol
    /// offset so holding and benchmark returns differ; no dividends.
    struct SyntheticPrices {
        fail_dividends: bool,
    }

    impl OutcomePriceSource for SyntheticPrices {
        fn daily_closes(
            &self,
            symbol: &str,
            from: NaiveDate,
            to: NaiveDate,
        ) -> Result<Vec<DatedValue>> {
            let offset = symbol.len() as f64;
            let mut out = Vec::new();
            let mut d = from;
            let mut i = 0f64;
            while d <= to {
                use chrono::Datelike;
                if d.weekday().number_from_monday() <= 5 {
                    out.push(DatedValue {
                        date: d.format("%Y-%m-%d").to_string(),
                        value: 100.0 + offset + i * 0.1,
                    });
                }
                i += 1.0;
                d += chrono::Duration::days(1);
            }
            Ok(out)
        }
        fn dividend_history(
            &self,
            _symbol: &str,
            _from: NaiveDate,
            _to: NaiveDate,
        ) -> Result<Vec<DatedValue>> {
            if self.fail_dividends {
                anyhow::bail!("dividends endpoint down")
            }
            Ok(vec![])
        }
    }

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::storage::init_schema(&conn).unwrap();
        conn
    }

    fn old_episode(symbol: &str, anchor_at: &str) -> DecisionEpisode {
        let anchor = parse_iso_date_prefix(anchor_at).unwrap();
        DecisionEpisode {
            episode_id: format!("ep-{symbol}"),
            symbol: symbol.into(),
            anchor_run_id: "run-old".into(),
            anchor_at: anchor_at.into(),
            intrinsic_vintage: anchor_at.into(),
            vintage_fresh: true,
            action_source: ActionSource::ModelChosen,
            position_change: PositionChange::New,
            sector: SectorIdentity::resolve(Some("Technology")),
            opened: vec![OpenReason::Debut],
            body: EpisodeBody::Priced(Box::new(PricedEpisode {
                action: Action::Hold,
                lean: Action::Hold,
                lean_divergence: None,
                target_weight_low: Some(0.03),
                target_weight_high: Some(0.06),
                snapshot: CalibrationSnapshot {
                    sub_scores: SubScores {
                        quality: 70.0,
                        valuation: 60.0,
                        momentum: 55.0,
                        risk: 65.0,
                    },
                    grade: Grade::B,
                    conviction: Conviction::Medium,
                    risk_tier: Some(RiskTier::Medium),
                    price_targets: PriceTargets {
                        one_month: Some(crate::portfolio::PriceTarget {
                            base: 105.0,
                            bear: 95.0,
                            bull: 115.0,
                            methodology: "test".into(),
                        }),
                        twelve_month: Some(crate::portfolio::PriceTarget {
                            base: 120.0,
                            bear: 60.0,
                            bull: 160.0,
                            methodology: "test".into(),
                        }),
                    },
                    dead_money: Some(HurdleState::Indeterminate),
                    hurdle: None,
                    dgs2: Some(0.04),
                    cap_signals: vec![],
                    grade_parameter_version: Some("grade-v2".into()),
                    target_parameter_version: Some("targets-v3".into()),
                    degraded_inputs: vec![],
                },
            })),
            observations: vec![],
            alignment: None,
            falsifier_events: vec![],
            labels: pending_labels(anchor),
            state: EpisodeState::Active,
            self_correction_count: 0,
        }
    }

    #[test]
    fn the_label_pass_scores_due_windows_and_matures_the_episode() {
        let conn = mem_conn();
        // Anchored ~14 months ago: every window is due, all within the synthetic
        // series' coverage (through today), so the episode fully matures.
        let anchor_at = (chrono::Utc::now() - chrono::Duration::days(430))
            .to_rfc3339();
        let mut episodes = vec![old_episode("GONE", &anchor_at)];
        let source = SyntheticPrices {
            fail_dividends: false,
        };
        let mut ctx = SeriesCtx::new(&conn, Some(&source));
        let today = chrono::Utc::now().date_naive();
        let summary = mature_labels(&mut episodes, &mut ctx, today, "2026-08-04");
        assert_eq!(summary.matured.len(), 4, "all four windows recorded");
        assert!(summary.matured.iter().all(|m| m.outcome == "scored"));
        assert_eq!(episodes[0].state, EpisodeState::Matured);
        let scored = scored_for(&episodes[0], 12).expect("12-month scored");
        // Empty dividend history: the total-return leg equals the price leg.
        assert_eq!(scored.total_return, Some(scored.price_return));
        // Relative legs computed against ^spx and the stamped XLK benchmark.
        assert!(scored.vs_market.is_some());
        assert!(scored.vs_sector.is_some());
        // The fetched series landed in the shared bar cache — holding + both
        // benchmarks.
        assert!(!store::load_price_bars(&conn, "GONE").unwrap().is_empty());
        assert!(!store::load_price_bars(&conn, MARKET_BENCHMARK).unwrap().is_empty());
        assert!(!store::load_price_bars(&conn, "XLK").unwrap().is_empty());
        // A second pass is a no-op (nothing pending), served from cache.
        let source2 = UnavailablePriceSource;
        let mut ctx2 = SeriesCtx::new(&conn, Some(&source2));
        let summary2 = mature_labels(&mut episodes, &mut ctx2, today, "2026-08-05");
        assert!(summary2.matured.is_empty());
    }

    #[test]
    fn a_failed_dividends_pull_takes_the_labeled_price_only_fallback() {
        let conn = mem_conn();
        let anchor_at = (chrono::Utc::now() - chrono::Duration::days(430)).to_rfc3339();
        let mut episodes = vec![old_episode("AAPL", &anchor_at)];
        let source = SyntheticPrices {
            fail_dividends: true,
        };
        let mut ctx = SeriesCtx::new(&conn, Some(&source));
        let today = chrono::Utc::now().date_naive();
        mature_labels(&mut episodes, &mut ctx, today, "2026-08-04");
        let scored = scored_for(&episodes[0], 12).expect("scored");
        assert!(scored.total_return.is_none());
        assert!(scored
            .total_return_gap
            .as_deref()
            .unwrap()
            .contains("price-only"));
    }

    #[test]
    fn no_source_leaves_labels_pending_then_grace_closes_them_typed() {
        let conn = mem_conn();
        let anchor_at = "2026-05-01T12:00:00+00:00";
        let mut episodes = vec![old_episode("AAPL", anchor_at)];
        // 1-month window (2026-06-01) is due but uncovered; within grace it stays
        // pending.
        let mut ctx = SeriesCtx::new(&conn, None);
        let today = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        let summary = mature_labels(&mut episodes, &mut ctx, today, "2026-06-15");
        assert!(summary.matured.is_empty());
        assert_eq!(summary.pending_coverage, vec!["AAPL".to_string()]);
        assert_eq!(episodes[0].state, EpisodeState::Active);
        // Past the grace with a never-covered series: price-coverage-unscorable.
        let mut ctx = SeriesCtx::new(&conn, None);
        let today = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
        let summary = mature_labels(&mut episodes, &mut ctx, today, "2026-09-15");
        assert_eq!(summary.matured.len(), 1);
        assert_eq!(summary.matured[0].outcome, "price-coverage-unscorable");
        // A series that existed but stopped resolves terminal past grace: seed
        // bars that end before the 3-month window, then age past its grace.
        store::merge_price_bars(
            &conn,
            "MSFT",
            &bars(&[("2026-05-04", 100.0), ("2026-06-05", 101.0)]),
        )
        .unwrap();
        let mut episodes = vec![old_episode("MSFT", anchor_at)];
        let mut ctx = SeriesCtx::new(&conn, None);
        let today = NaiveDate::from_ymd_opt(2026, 12, 20).unwrap();
        let summary = mature_labels(&mut episodes, &mut ctx, today, "2026-12-20");
        assert!(summary
            .matured
            .iter()
            .any(|m| m.outcome == "terminal-unscorable"));
    }

    #[test]
    fn lead_time_stamps_against_the_bear_line_when_the_12_month_window_scores() {
        let conn = mem_conn();
        let anchor_at = (chrono::Utc::now() - chrono::Duration::days(430)).to_rfc3339();
        let mut ep = old_episode("AAPL", &anchor_at);
        // A confirmed falsifier early in the window; the synthetic series never
        // drops below the 60.0 bear line, so the read is no-material-drawdown.
        let confirmed = (chrono::Utc::now() - chrono::Duration::days(400))
            .format("%Y-%m-%d")
            .to_string();
        ep.falsifier_events.push(FalsifierEvent {
            condition_id: "c-1".into(),
            confirmed_at: confirmed,
            confirmation_observation_id: "2025-06-30".into(),
            post_maturity: false,
            lead_time_trading_days: None,
            no_material_drawdown: None,
        });
        let mut episodes = vec![ep];
        let source = SyntheticPrices {
            fail_dividends: false,
        };
        let mut ctx = SeriesCtx::new(&conn, Some(&source));
        let today = chrono::Utc::now().date_naive();
        mature_labels(&mut episodes, &mut ctx, today, "2026-08-04");
        let ev = &episodes[0].falsifier_events[0];
        assert_eq!(ev.no_material_drawdown, Some(true));
        assert!(ev.lead_time_trading_days.is_none());
        // The derived read collects it.
        let reads = derive_reads(&episodes);
        assert_eq!(reads.falsifier_lead_times.len(), 1);
        assert!(reads.falsifier_lead_times[0].no_material_drawdown);
    }

    #[test]
    fn derived_reads_cohorts_and_calibration_come_from_scored_windows() {
        let conn = mem_conn();
        let anchor_at = (chrono::Utc::now() - chrono::Duration::days(430)).to_rfc3339();
        let mut episodes = vec![old_episode("AAPL", &anchor_at), old_episode("MSFT", &anchor_at)];
        let source = SyntheticPrices {
            fail_dividends: false,
        };
        let mut ctx = SeriesCtx::new(&conn, Some(&source));
        let today = chrono::Utc::now().date_naive();
        mature_labels(&mut episodes, &mut ctx, today, "2026-08-04");
        let reads = derive_reads(&episodes);
        let twelve = reads
            .cohorts
            .iter()
            .find(|c| c.window_months == 12)
            .unwrap();
        let hold = twelve
            .lean_cohorts
            .iter()
            .find(|c| c.key == "hold")
            .expect("hold cohort");
        assert_eq!(hold.unique_holdings, 2);
        assert!(hold.mean_total_return.is_some());
        let cal = reads
            .target_calibration
            .iter()
            .find(|t| t.window_months == 12)
            .unwrap();
        assert_eq!(cal.scored, 2);
        assert!(cal.mean_interval_score.is_some());
        assert!(!reads.eligibility.eligible, "2 of 30: below the bar");
        assert!(reads.eligibility.note.contains("below the proposal eligibility bar"));
    }
}
