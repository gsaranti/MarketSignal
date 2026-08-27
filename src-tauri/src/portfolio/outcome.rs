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
//!   refreshes (FMP dated EOD) — and the dividend history for the total-return
//!   leg.
//!
//! The standing-thesis episode-creation leg and the self-correction counters are
//! **live**: both read the 6g what-changed attribution validator's per-holding
//! audit ([`crate::portfolio::WhatChangedAudit`]) — an attributed thesis-level
//! move or a labeled self-correction opens an episode with the action unchanged
//! ([`OpenReason::ThesisChange`]), and the validated self-correction counts
//! accumulate per episode. Terminal outcomes are typed conservatively: no
//! corporate-action feed exists, so a previously covered series that stops
//! resolves `terminal-unscorable` past the price-coverage grace, never a
//! fabricated acquisition or bankruptcy read.

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

/// The market benchmark's FMP identity (`docs/data-sources.md §Financial
/// Modeling Prep` — the benchmark identity table). Episodes never persist this
/// symbol — it is applied at label time — so the 2026-08-12 rename from Stooq's
/// `^spx` touched only the price-bar cache (cleaned at store init).
pub const MARKET_BENCHMARK: &str = "^GSPC";

/// Coverage tolerance: a series whose latest bar sits within this many calendar
/// days before a window end still covers it (weekends and short market closures —
/// the label value is the last close at or before the window end either way).
const COVERAGE_TOLERANCE_DAYS: i64 = 4;

/// Session-proximity bound around a keyed session, in calendar days (long
/// weekend + holiday headroom). It bounds both session-adjacent reads: the
/// "next session's close" **after** the episode anchor (the entry) and the
/// close **at or before** the intrinsic-vintage session (the basis bridge). A
/// bar beyond the bound on either side is not an adjacent session — a late
/// series start, a sparse cache — so the entry case holds the window pending
/// and the bridge case excludes the bridge-dependent reads, never a much-later
/// entry or a years-stale bridge.
const ENTRY_TOLERANCE_DAYS: i64 = 7;

/// Calendar-day pad on fetch ranges, so the entry anchor (the first session after
/// the run) and month-end joins never sit exactly on a fetch boundary.
const FETCH_PAD_DAYS: i64 = 7;

// ---- Sector identity -------------------------------------------------------------

/// The SPDR sector-ETF benchmark for an FMP profile sector label
/// (`docs/data-sources.md §Financial Modeling Prep` — the sector-ETF mapping).
/// Accepts both FMP's
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

/// Why an episode opened — the recommendation-state change that minted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenReason {
    /// First analysis of the holding — or the first after the machinery landed,
    /// or the recovery re-seed after the symbol's active episode row became
    /// unreadable ([`plan_episodes`]'s two seeding seams).
    Debut,
    BranchFlip,
    ActionChange,
    /// The action change was the over-age rule-demotion, not a model decision.
    RuleDemotion,
    /// The standing thesis changed with the branch and action unchanged — the
    /// run's validated what-changed audit recorded an attributed thesis-level
    /// move or a labeled self-correction
    /// (`docs/portfolio-analysis.md §Outcome learning`; live since the 6g
    /// attribution validator landed).
    ThesisChange,
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
/// because the run's own audit record can age out of the run retention before a
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
    /// The authoring-time spot the targets were computed from (the quick-check
    /// basis's print). Target calibration scores bands **in return space over
    /// this spot**: the authored band is an absolute price in the authoring-time
    /// basis, while label-time closes are retroactively split-adjusted, so a
    /// price-space comparison would shear across a split. `None` (pre-field, or
    /// no basis persisted) excludes the episode from band scoring rather than
    /// comparing across bases.
    #[serde(default)]
    pub authoring_spot: Option<f64>,
    /// Cap signals in force at the decision (the pre-profit overlay's matched
    /// rules; empty when none).
    pub cap_signals: Vec<String>,
    pub grade_parameter_version: Option<String>,
    pub target_parameter_version: Option<String>,
    pub degraded_inputs: Vec<String>,
    /// The model arm's freely-authored target bands, frozen at open — scored by
    /// the same interval-score machinery as the engine bands over the same
    /// exclusion population, so the model-vs-engine head-to-head is fair
    /// (`docs/portfolio-analysis.md` §Outcome learning). Present on every priced
    /// episode a fresh v9-only store writes (both arms ride every verdict).
    pub model_price_targets: crate::portfolio::ModelPriceTargets,
    /// The model arm's own sub-scores at open (recorded for later predictor-quality
    /// reads; no scored read yet).
    pub model_sub_scores: SubScores,
    /// Both arms' horizon outlooks at open — the direction hit-rate read scores
    /// each against the realized sign at its mapped window.
    pub model_outlook: crate::portfolio::HorizonOutlook,
    pub engine_outlook: crate::portfolio::HorizonOutlook,
    /// The engine stand-in arm's conviction and action rung at open.
    pub engine_conviction: Conviction,
    pub engine_action: Action,
}

/// The priced branch's episode body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricedEpisode {
    /// The final portfolio action.
    pub action: Action,
    pub snapshot: CalibrationSnapshot,
}

/// The `role_risk_only` branch's reduced episode body — no lean, grade, conviction,
/// band, target, or dead-money field exists on that verdict to record. Excluded from
/// target calibration and every grade-linked read; counted in its own class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleRiskEpisode {
    pub action: Action,
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
    /// The label-basis close **at or before the intrinsic-vintage ET session**
    /// — the same-session counterpart of the snapshot's authoring spot, so
    /// authored absolute prices (band edges, the bear line) convert into the
    /// label basis as `price × anchor_close ⁄ authoring_spot`. Keyed at the
    /// intrinsic vintage, not the episode anchor: the spot and targets belong to
    /// the intrinsic pass, older than the anchor on a rule-demotion open (for
    /// vintage-fresh episodes the two key the same session). The next-session
    /// entry cannot serve this role: it sits an overnight gap away from the
    /// spot, which would shear the comparison. `None` when the series carried no
    /// proximate bar at or before that session — those labels are excluded from
    /// band scoring (the residual error of the bridge is intraday
    /// quote-vs-close, never a split or a gap).
    #[serde(default)]
    pub anchor_close: Option<f64>,
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
/// run retention and frozen into the matured archive once its 12-month labels
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
    /// The validated self-corrections accumulated on this episode — seeded from
    /// the opening run's what-changed audit and extended by later fresh passes
    /// (`docs/portfolio-analysis.md §Outcome learning`; the 6g attribution
    /// validator labels them, downgrades included). Zero on episodes persisted
    /// before the validator landed (`#[serde(default)]`).
    #[serde(default)]
    pub self_correction_count: u32,
}

impl DecisionEpisode {
    /// The anchor date (the run date the windows key on) — the anchor instant's
    /// **ET session date** ([`crate::market_clock::et_date_of`]), never the UTC
    /// date prefix: an evening-ET run has rolled to the next UTC date, and a
    /// UTC-dated anchor keys the entry one session late and the basis bridge to
    /// a session traded entirely after the decision.
    pub fn anchor_date(&self) -> Option<NaiveDate> {
        crate::market_clock::et_date_of(&self.anchor_at)
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

pub(crate) fn parse_iso_date_prefix(s: &str) -> Option<NaiveDate> {
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
    /// Mean absolute total return — the primary ordering read (quotes the
    /// price-only return per label where the TR leg was unavailable; a labeled mix
    /// is still a mix, so the pure price-only mean rides beside it).
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
    /// The intrinsic layer: vintage-fresh, model-chosen priced episodes, keyed by action.
    pub lean_cohorts: Vec<CohortStat>,
    /// The final-action strata — diagnostic only (raw return ordering cannot score
    /// a risk override), read across vintages, stratified by the action rung.
    pub final_action_cohorts: Vec<CohortStat>,
    /// `role_risk_only` episodes — their own class, never pooled.
    pub role_risk: Option<CohortStat>,
    /// Rule-demoted episodes — their own class, out of the pooled cohorts.
    pub rule_demoted: Option<CohortStat>,
}

/// Target calibration for one band window and one target-function version
/// (1- and 12-month bands score at their matching windows; the 3- and 6-month
/// labels serve the cohort reads). Reads are **split by the snapshot's target
/// parameter version** — the function is versioned exactly so calibration never
/// mixes bases (`docs/portfolio-analysis.md §Starting parameters`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetCalibrationRead {
    pub window_months: u32,
    /// The target-function parameter version this read aggregates (`None` groups
    /// pre-version episodes).
    #[serde(default)]
    pub parameter_version: Option<String>,
    /// Bands scored (vintage-fresh episodes whose window scored and whose snapshot
    /// carried the matching band plus the authoring spot).
    pub scored: usize,
    /// Fraction of realized prices inside the bear–bull band, vs the declared
    /// nominal ([`NOMINAL_BAND_COVERAGE`]).
    pub coverage_rate: Option<f64>,
    pub nominal_coverage: f64,
    /// Mean interval (Winkler) score at the nominal level — calibration and
    /// sharpness together; lower is better, ungameable by width. Scored **in
    /// return space over the authoring spot** (band edges as returns vs the
    /// price-only label), so scores are split-safe and comparable across price
    /// levels.
    pub mean_interval_score: Option<f64>,
    /// Mean signed base-case error `(realized − base) / base` — the systematic-bias
    /// read on the scenario engine (realized reconstructed in the authoring basis
    /// via `spot × (1 + price_return)`).
    pub mean_base_signed_error: Option<f64>,
}

/// One arm's outlook direction hit-rate at its mapped window (short → 1-month,
/// mid → 6-month, long → 12-month labels) — the two-arm scoreboard's directional
/// read (`docs/portfolio-analysis.md` §Outcome learning). Realized direction is
/// the price-only label's sign (a directional call is about the price path); a
/// neutral read is counted beside the hit-rate, never inside it; a zero realized
/// return scores a directional call as a miss.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutlookDirectionRead {
    /// "engine" (the stand-in arm) or "model".
    pub arm: String,
    pub window_months: u32,
    /// Directional (bullish / bearish) reads whose window scored.
    pub scored: usize,
    pub hits: usize,
    /// Neutral reads at this window, excluded from the hit-rate.
    pub neutral: usize,
}

/// The model-vs-engine band head-to-head at one window, computed over the
/// **paired population only** — episodes where BOTH arms carried a band and the
/// window scored with an authoring spot and anchor bridge — so the comparison is
/// same-events by construction, never two independently-pooled populations
/// (`docs/portfolio-analysis.md` §Outcome learning). The per-arm
/// `target_calibration` reads keep each arm's full population separately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeadToHeadRead {
    pub window_months: u32,
    /// Paired bands scored — identical for both arms by construction.
    pub scored: usize,
    pub engine_mean_interval_score: Option<f64>,
    pub model_mean_interval_score: Option<f64>,
    pub engine_coverage_rate: Option<f64>,
    pub model_coverage_rate: Option<f64>,
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

/// The per-holding self-correction accumulation — the cumulative calibration
/// signal over the counts the 6g attribution validator labels
/// (`docs/portfolio-analysis.md §Outcome learning`).
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

/// The derived scorecard reads (`docs/portfolio-analysis.md §Outcome learning`),
/// computed over the updated episode set: cohort return-spreads, both arms'
/// target-band calibration and their head-to-head, outlook-direction hit-rates,
/// falsifier lead-times, self-correction, and proposal eligibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedReads {
    pub cohorts: Vec<CohortWindowRead>,
    pub target_calibration: Vec<TargetCalibrationRead>,
    /// The model arm's band calibration — same scorer and exclusion rules as
    /// `target_calibration`, over the episodes' frozen model bands (empty until
    /// episodes mature); each arm's read covers that arm's own full band
    /// population. `#[serde(default)]` so a run persisted before the field
    /// existed still decodes.
    #[serde(default)]
    pub model_target_calibration: Vec<TargetCalibrationRead>,
    /// The paired model-vs-engine head-to-head ([`HeadToHeadRead`]) — the ONLY
    /// read the arms are compared on. `#[serde(default)]` so a run persisted
    /// before the field existed still decodes.
    #[serde(default)]
    pub head_to_head: Vec<HeadToHeadRead>,
    /// Both arms' outlook direction hit-rates. `#[serde(default)]` so a run
    /// persisted before the field existed still decodes.
    #[serde(default)]
    pub outlook_direction: Vec<OutlookDirectionRead>,
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

/// The outcome pass's retrieval seam: daily closes (FMP dated EOD) and the
/// dividend history for the total-return leg. Behind a trait so the job is
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

/// The live source: FMP dated EOD for the daily closes, FMP `dividends` for the
/// total-return leg — the same single rung as the per-holding deep history
/// (`docs/verification/2026-08-12-stooq-removal-decision.md`).
pub struct LiveOutcomePrices {
    pub fmp: crate::fmp::FmpDataSource,
}

impl OutcomePriceSource for LiveOutcomePrices {
    fn daily_closes(
        &self,
        symbol: &str,
        from: NaiveDate,
        _to: NaiveDate,
    ) -> Result<Vec<DatedValue>> {
        // The FMP dated-EOD fetch is now-anchored; a lookback from today
        // covering `from` spans the requested range (labels always read
        // through the present), which is why the range's own upper bound goes
        // unread here.
        // A lookback COUNT, not a session key: the UTC date is fine here —
        // one extra day of history is harmless, and the bars are then
        // selected by their own dates.
        let today = chrono::Utc::now().date_naive();
        let lookback = (today - from).num_days().max(1);
        match self.fmp.fetch_dated_eod(symbol, lookback) {
            Ok(closes) if !closes.is_empty() => Ok(closes),
            Ok(_) => anyhow::bail!("FMP dated EOD served no rows for {symbol}"),
            Err(fmp_err) => Err(fmp_err.context(format!("FMP dated EOD failed for {symbol}"))),
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
            // Start coverage means an anchor-adjacent bar, not merely any bar
            // before `from`: a sparse cache whose nearest pre-anchor bar is
            // years old has a hole at the anchor, and serving it as covered
            // would hand the callers a stale basis bridge.
            let covers_start = anchor_session_close(cached, from).is_some();
            !(covers_start && covers_through(cached, through))
        };
        if needs_fetch && !self.fetch_attempted.contains(&key) {
            if let Some(source) = self.source {
                self.fetch_attempted.insert(key.clone());
                let fetch_from = from - chrono::Duration::days(FETCH_PAD_DAYS);
                // A fetch range's upper bound, not a session key (see
                // `fetch_floor` above for the bound that is one).
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

/// The window's entry reference: the first close after the anchor, **bounded by
/// [`ENTRY_TOLERANCE_DAYS`]** — a first bar beyond the bound is a series that
/// never covered the start, not a next-session close.
fn entry_close(closes: &[DatedValue], anchor: NaiveDate) -> Option<&DatedValue> {
    first_close_after(closes, anchor).filter(|b| {
        parse_iso_date_prefix(&b.date)
            .is_some_and(|d| d <= anchor + chrono::Duration::days(ENTRY_TOLERANCE_DAYS))
    })
}

/// The anchor-session close — the basis bridge's realized leg
/// ([`ScoredLabel::anchor_close`]): the last close at or before the anchor,
/// **bounded by the same [`ENTRY_TOLERANCE_DAYS`]** — a years-old bar from a
/// sparse cache sits at the anchor's date position but is no decision-instant
/// close, so it must exclude the bridge-dependent reads rather than scale them.
/// `pub(crate)` because the 6f retrospective's price comparisons ride the same
/// bridge contract (`pipeline::retrospective_section`) — one home, one bound.
pub(crate) fn anchor_session_close(closes: &[DatedValue], anchor: NaiveDate) -> Option<&DatedValue> {
    close_at_or_before(closes, anchor).filter(|b| {
        parse_iso_date_prefix(&b.date)
            .is_some_and(|d| d >= anchor - chrono::Duration::days(ENTRY_TOLERANCE_DAYS))
    })
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
    // The per-series fetch floor: the earliest active-episode anchor touching
    // each fetched symbol (the holding itself, the market benchmark, the sector
    // benchmark), so the one fetch per symbol per pass spans every active
    // episode's windows on a single adjustment basis. Floored at the fetching
    // episode's own anchor instead, a partial-range merge after a split could
    // leave one series in two bases, and a second episode with an older anchor
    // had no fetch left to heal its start coverage (piece-3 ruling 9).
    let mut fetch_floor: HashMap<String, NaiveDate> = HashMap::new();
    for ep in episodes.iter() {
        if ep.state != EpisodeState::Active {
            continue;
        }
        let Some(anchor) = ep.anchor_date() else {
            continue;
        };
        let mut floor = |symbol: &str, from: NaiveDate| {
            fetch_floor
                .entry(symbol.to_ascii_uppercase())
                .and_modify(|d| *d = (*d).min(from))
                .or_insert(from);
        };
        // The holding's own series is additionally floored at its **intrinsic
        // vintage**, which on a rule-demotion open is OLDER than the anchor: that
        // is the session the basis bridge keys at (`anchor_close` below), and a
        // floor that stops at the anchor leaves the bridge's own bar outside the
        // refreshed range. Because `merge_price_bars` rewrites only fetched dates
        // and `price_bars` is never pruned, a bar cached before a split then
        // satisfies the bridge on a **stale adjustment basis** rather than being
        // excluded — fabricating a material-drawdown breach off a pre-split price.
        // The absent-bar sibling case was already excluded correctly; this is the
        // present-but-stale hole in the same guard.
        let vintage = crate::market_clock::et_date_of(&ep.intrinsic_vintage);
        floor(&ep.symbol, vintage.map_or(anchor, |v| v.min(anchor)));
        // The benchmark legs are read from the anchor only (`bench_return`), never
        // bridged, so they keep the anchor floor.
        floor(MARKET_BENCHMARK, anchor);
        if let Some(b) = &ep.sector.benchmark {
            floor(b, anchor);
        }
    }
    let floor_for = |fetch_floor: &HashMap<String, NaiveDate>, symbol: &str, own: NaiveDate| {
        fetch_floor
            .get(&symbol.to_ascii_uppercase())
            .copied()
            .unwrap_or(own)
            .min(own)
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
        let closes = ctx
            .series(&ep.symbol, floor_for(&fetch_floor, &ep.symbol, anchor), furthest)
            .to_vec();
        let entry = entry_close(&closes, anchor).cloned();
        // One dividends pull per episode serves every scoring window (the
        // furthest due end bounds the span) — never one request per window.
        let mut episode_divs: Option<std::result::Result<Vec<DatedValue>, String>> = None;

        // The sector benchmark is entry-stamped; an unscorable identity types the
        // sector legs immediately and never blocks scoring.
        let sector_bench = ep.sector.benchmark.clone();
        let sector_gap = ep.sector.unscorable.clone();
        // The label-basis close at the decision instant — the split-bridge the
        // authored absolute prices convert through ([`ScoredLabel::anchor_close`]),
        // session-proximity-bounded like the entry — keyed at the episode's
        // **intrinsic vintage** (ET session), not the episode anchor:
        // `authoring_spot` and the authored targets belong to the intrinsic
        // pass, which on a rule-demotion open is older than the anchor run, and
        // an anchor-keyed bridge sheared the line for vintage-stale episodes
        // (piece-3 ruling 9). Vintage-fresh episodes key the same session either
        // way.
        let anchor_close = crate::market_clock::et_date_of(&ep.intrinsic_vintage)
            .and_then(|d| anchor_session_close(&closes, d))
            .map(|b| b.value);
        // The material-drawdown line converted into the **label basis** via that
        // bridge (`anchor_close × bear ⁄ authoring_spot`): the authored bear
        // target is an absolute price in its authoring-time basis, label-time
        // closes are retroactively split-adjusted, and both bridge legs share the
        // authoring instant — anchoring on the next-session entry instead would
        // inject its overnight gap into the line (an upward gap fabricates a
        // breach, a downward one hides it). No spot or no bridge bar (pre-field
        // episodes, a start-uncovered series, an uncovered intrinsic session)
        // leaves the events unstamped, excluded from the read — never a
        // cross-basis comparison.
        let bear_line_12m = match &ep.body {
            EpisodeBody::Priced(p) => p
                .snapshot
                .price_targets
                .twelve_month
                .as_ref()
                .map(|t| t.bear)
                .zip(p.snapshot.authoring_spot.filter(|s| *s > 0.0))
                .zip(anchor_close)
                .map(|((bear, spot), bridge)| bridge * bear / spot),
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
                    // discriminator: a series alive at the entry that stopped
                    // before the window end is conservatively terminal; one that
                    // never covered the start (empty, or first bar beyond the
                    // entry bound) takes the price-coverage state.
                    let outcome = if entry.is_none() {
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
            let market_series = ctx
                .series(
                    MARKET_BENCHMARK,
                    floor_for(&fetch_floor, MARKET_BENCHMARK, anchor),
                    w_end,
                )
                .to_vec();
            let market_ret = bench_return(&market_series, anchor, w_end);
            let sector_series = sector_bench
                .as_ref()
                .map(|b| ctx.series(b, floor_for(&fetch_floor, b, anchor), w_end).to_vec());
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
            // The window's end is the END BAR's own date, not the calendar
            // `w_end`: the end price is the last close at or before `w_end`, so a
            // dividend going ex AFTER that close is not yet out of the price it is
            // being added to. Counting it inflates the label — always signed
            // positive, since dividends only add — and the gap is routine: any
            // `w_end` on a weekend, a holiday, or a stale-cache tail leaves days
            // between the last close and the calendar bound. This mirrors the
            // entry side, which already bounds on the entry bar's own date.
            let end_iso = end_bar.date.chars().take(10).collect::<String>();
            let entry_iso = entry_date.format("%Y-%m-%d").to_string();
            let (total_return, total_return_gap) = match divs_result {
                Ok(rows) => {
                    // The window off an entry CLOSE is `(entry, end]`: an
                    // ex-date on the entry session itself is already out of the
                    // entry price, so counting it would overstate the label by
                    // one payment.
                    let paid: f64 = rows
                        .iter()
                        .filter(|d| {
                            d.date.as_str() > entry_iso.as_str()
                                && d.date.as_str() <= end_iso.as_str()
                        })
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
                anchor_close,
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
                if let Some(bear_line) = bear_line_12m {
                    stamp_lead_times(
                        &mut ep.falsifier_events,
                        &closes,
                        &entry.date,
                        w_end,
                        bear_line,
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
/// anchor (the same [`ENTRY_TOLERANCE_DAYS`] bound as the holding leg). `None`
/// when the series doesn't cover the window at either end.
fn bench_return(closes: &[DatedValue], anchor: NaiveDate, w_end: NaiveDate) -> Option<f64> {
    if !covers_through(closes, w_end) {
        return None;
    }
    let entry = entry_close(closes, anchor)?;
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
/// interpretive. `bear_line` arrives already converted into the label basis
/// (`anchor_close × bear ⁄ authoring_spot` — the caller's split-bridge), so the
/// comparison is the documented absolute one: the first close below the line.
/// Positive = confirmed before the breach; explicit `no-material-drawdown` when no
/// such close occurs by maturity.
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
/// Since `portfolio-v9` identity compares the branch and the action alone
/// (`docs/portfolio-analysis.md §Outcome learning`): the retired ledger
/// target-weight range no longer exists to compare. The standing-thesis leg
/// deliberately stays outside this key — it is a per-run signal from the
/// validated what-changed audit ([`episode_decision`]'s `thesis_changed`), not a
/// comparable state.
#[derive(Debug, Clone, PartialEq)]
enum RecState {
    Priced { action: Action },
    RoleRisk { action: Action },
    /// An insufficient-evidence exit — the standing recommendation is retained,
    /// so an abstention is never a state change (and never opens).
    Abstained,
    /// Not-rated (or no verdict): outside the episode machinery.
    None,
}

fn rec_state(v: &HoldingVerdict) -> RecState {
    match &v.disposition {
        VerdictDisposition::Priced(g) => RecState::Priced { action: g.action },
        VerdictDisposition::RoleRiskOnly(r) => RecState::RoleRisk { action: r.action },
        VerdictDisposition::InsufficientEvidence { .. } => RecState::Abstained,
        VerdictDisposition::NotRated { .. } => RecState::None,
    }
}

/// The decision the symbol's latest **active episode** is standing on — its own
/// recorded action, not a verdict's.
///
/// It exists for the abstained-prior arm. An abstained verdict carries no action at
/// all (`InsufficientEvidence` has none to carry), so comparing "did the
/// recommendation change across the abstention?" against the prior *verdict* can
/// only ever compare branch. The episode the abstention extended does carry the
/// action it was opened on, and that is the forecast calibration scores — so it
/// is the correct thing to compare against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StandingDecision {
    pub action: Action,
}

impl StandingDecision {
    fn of(ep: &DecisionEpisode) -> Self {
        match &ep.body {
            EpisodeBody::Priced(p) => Self { action: p.action },
            EpisodeBody::RoleRiskOnly(r) => Self { action: r.action },
        }
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
/// (`docs/portfolio-analysis.md §Outcome learning` — the creation rule). An
/// abstention always extends (the standing recommendation stands); a prior
/// abstention compares its retained ledger's branch **plus the action its
/// standing episode carries**, since the abstained verdict itself re-authored
/// neither (`standing`). `thesis_changed` is the standing-thesis leg's signal —
/// the run's validated what-changed audit recorded an attributed thesis-level
/// move or a labeled self-correction for this holding (fresh passes only; a
/// carried audit's what-changed is its own run's fact) — and opens an episode
/// even with the branch and action unchanged; input movement alone never does.
pub fn episode_decision(
    prior: Option<&HoldingVerdict>,
    current: &HoldingVerdict,
    current_is_fresh: bool,
    standing: Option<StandingDecision>,
    thesis_changed: bool,
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
                    // A ledger-less abstained prior is a *debut* abstention — the
                    // holding was never tracked, so nothing is comparable and the
                    // episode opens as a debut (a weight-range "change" against
                    // a never-committed range would be a fabricated reason).
                    if prior_v.thesis_ledger.is_none() {
                        return EpisodeDecision::Open(vec![OpenReason::Debut]);
                    }
                    // The abstained prior retained the standing ledger: the branch
                    // is still comparable; the action is not.
                    let prior_branch_priced = matches!(
                        prior_v.thesis_ledger.as_ref().map(|l| l.branch),
                        Some(crate::portfolio::LedgerBranch::Priced) | None
                    );
                    let cur_priced = matches!(cur, RecState::Priced { .. });
                    let mut reasons = Vec::new();
                    if prior_branch_priced != cur_priced {
                        reasons.push(OpenReason::BranchFlip);
                    }
                    // The action comes from the STANDING EPISODE, not the
                    // abstained verdict (which carries none). Without this the
                    // first fresh pass after an abstention extended the episode it
                    // had just superseded: the recommendation had moved, but the
                    // comparison could not see it, so the new forecast accrued onto
                    // the old one's window and calibration scored a decision against
                    // observations made before it existed.
                    if let Some(st) = standing {
                        let cur_action = match &cur {
                            RecState::Priced { action } | RecState::RoleRisk { action } => *action,
                            _ => unreachable!("the outer match admits only analyzed states"),
                        };
                        if st.action != cur_action {
                            reasons.push(OpenReason::ActionChange);
                            if current.action_source == ActionSource::RuleDemoted {
                                reasons.push(OpenReason::RuleDemotion);
                            }
                        }
                    }
                    if thesis_changed {
                        reasons.push(OpenReason::ThesisChange);
                    }
                    reasons.dedup();
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
                    let prior_action = match prior_state {
                        RecState::Priced { action } | RecState::RoleRisk { action } => action,
                        _ => unreachable!(),
                    };
                    let cur_action = match cur {
                        RecState::Priced { action } | RecState::RoleRisk { action } => action,
                        _ => unreachable!(),
                    };
                    if prior_action != cur_action {
                        reasons.push(OpenReason::ActionChange);
                        if current.action_source == ActionSource::RuleDemoted {
                            reasons.push(OpenReason::RuleDemotion);
                        }
                    }
                    if thesis_changed {
                        reasons.push(OpenReason::ThesisChange);
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

/// The symbols whose **active** episode row was unreadable at load and is not
/// superseded by a newer readable episode — the recovery-seed set (uppercase).
///
/// The supersession bound is what keeps the flag from becoming a zombie: once a
/// recovery debut (or any later episode) lands, the lost row stops mattering, so a
/// post-maturity re-affirmation can never re-open off it. It reads **insertion
/// order** — `readable_before`, the corrupt row's position in the `id`-ordered scan —
/// not `anchor_at`: under a backwards clock step a later-inserted recovery episode
/// carries an older timestamp, so a timestamp bound left the symbol permanently
/// flagged and opened a fresh debut episode on every subsequent run, filling the
/// store with one-run episodes and polluting every cohort read.
pub fn lost_active_symbols(
    skipped: &[store::SkippedEpisodeRow],
    episodes: &[DecisionEpisode],
) -> HashSet<String> {
    skipped
        .iter()
        .filter(|row| row.state == "active")
        .filter(|row| {
            let after = row.readable_before.min(episodes.len());
            !episodes[after..]
                .iter()
                .any(|e| e.symbol.eq_ignore_ascii_case(&row.symbol))
        })
        .map(|row| row.symbol.to_ascii_uppercase())
        .collect()
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
    /// Symbols whose active episode row was unreadable and unsuperseded
    /// ([`lost_active_symbols`], uppercase) — a symbol here re-seeds via a
    /// debut (any readable active is an older predecessor, never the lost
    /// decision's carrier), so a lost row can't leave the current decision
    /// untracked until the next state change.
    pub unreadable_active_symbols: HashSet<String>,
    /// Symbols whose verdict (and audit) were carried, not freshly passed
    /// (uppercase). A carried audit's `ledger_audit.crossings` are its *prior*
    /// run's crossings — they attached to an episode in that run, so the
    /// falsifier-attach loop must skip them: re-attaching one to an episode
    /// newly opened this run (whose empty event list defeats the per-episode
    /// dedup) would fabricate a fresh confirmation dated today.
    pub carried_symbols: &'a HashSet<String>,
}

/// What the plan changed.
pub struct PlanSummary {
    pub opened: Vec<OpenedEpisodeNote>,
    pub extended: Vec<String>,
    pub changed: HashSet<String>,
}

/// Append-or-extend this run's decision episodes in place
/// (`docs/portfolio-workflow.md §Step 8`): open on an observable
/// recommendation-state change — including the never-seeded debut of a symbol
/// with no episode at all (the upgrade seam) — extend the **latest** active
/// episode on a re-affirmation / carry / abstention, record nothing
/// post-maturity, and attach this run's confirmed falsifier crossings to the
/// latest active episode (the latest matured episode, typed post-maturity, when
/// none is active).
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
        // The symbol's latest active episode, selected in insertion order exactly as
        // the extend target below is — the decision an abstention has been standing
        // on.
        let standing = episodes
            .iter()
            .rfind(|e| {
                e.state == EpisodeState::Active
                    && e.symbol.eq_ignore_ascii_case(&verdict.symbol)
            })
            .map(StandingDecision::of);
        let audit = input
            .audits
            .iter()
            .find(|a| a.symbol.eq_ignore_ascii_case(&verdict.symbol));
        // The standing-thesis and self-correction signals, from this run's
        // validated what-changed audit — **fresh passes only**: a carried
        // audit's what-changed is its own run's fact, already consumed there
        // (the same premise as the carried-crossings skip below).
        let what_changed = audit
            .filter(|_| is_fresh)
            .and_then(|a| a.what_changed_audit.as_ref());
        let thesis_changed = what_changed.is_some_and(|w| w.thesis_changed);
        let self_corrections = what_changed.map(|w| w.self_correction_count).unwrap_or(0);
        let mut decision =
            episode_decision(prior_v, verdict, is_fresh, standing, thesis_changed);
        // Two seeding seams convert an `Extend` into the debut open
        // ([`OpenReason::Debut`]); an abstention still never opens. **Upgrade**: a
        // prior run that predates the episode machinery yields `Extend` for a
        // stable holding, but a symbol with no episode at all was never seeded —
        // not a post-maturity re-affirmation, which leaves matured history
        // behind. **Recovery**: a symbol whose active episode row was unreadable
        // and unsuperseded re-seeds unconditionally — `lost_active` membership
        // already means nothing readable is newer than the corrupt row, so any
        // readable active episode is an older predecessor whose forecast stopped
        // accruing when the lost successor opened; extending it would graft the
        // current decision onto a forecast it never re-affirmed (the corrupt row
        // itself is never deleted).
        if matches!(decision, EpisodeDecision::Extend(_))
            && !matches!(rec_state(verdict), RecState::Abstained)
        {
            let any_readable = episodes
                .iter()
                .any(|e| e.symbol.eq_ignore_ascii_case(&verdict.symbol));
            if !any_readable || input.unreadable_active_symbols.contains(&key) {
                decision = EpisodeDecision::Open(vec![OpenReason::Debut]);
            }
        }
        match decision {
            EpisodeDecision::Nothing => {}
            EpisodeDecision::Extend(kind) => {
                // Attach to the **latest** active episode: an older episode still
                // maturing stopped accruing observations when the state change
                // opened its successor (`docs/portfolio-analysis.md §Outcome
                // learning` — "the old one stops accruing").
                // "Latest" is **insertion order** — the last matching row in the
                // store's `id`-ordered load — never `max_by(anchor_at)`: a
                // backwards wall-clock step would otherwise select an older
                // active episode forever and permanently shadow the one just
                // opened. Same premise as the piece-3 `latest_run` / `prune_runs`
                // fixes.
                if let Some(ep) = episodes.iter_mut().rfind(|e| {
                    e.state == EpisodeState::Active
                        && e.symbol.eq_ignore_ascii_case(&verdict.symbol)
                }) {
                    ep.observations.push(EpisodeObservation {
                        run_id: input.run_id.to_string(),
                        observed_at: input.created_at.to_string(),
                        kind,
                    });
                    // Defensive: a self-correction opens via `thesis_changed`,
                    // so an extension normally adds zero — accumulate anyway so
                    // no labeled count can be dropped.
                    ep.self_correction_count += self_corrections;
                    summary.extended.push(verdict.symbol.clone());
                    summary.changed.insert(ep.episode_id.clone());
                }
                // No active episode: a post-maturity re-affirmation records
                // nothing — there is no open forecast left to observe against.
            }
            EpisodeDecision::Open(reasons) => {
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
                            .rfind(|e| e.symbol.eq_ignore_ascii_case(&verdict.symbol))
                            .map(|e| e.sector.clone())
                    })
                    .unwrap_or_else(|| {
                        SectorIdentity::unscorable("no sector read at the anchor run")
                    });
                // The ET session date, matching [`DecisionEpisode::anchor_date`]
                // — the window ends stamped here key on the same day that
                // read-side derivation yields.
                let Some(anchor) = crate::market_clock::et_date_of(input.created_at) else {
                    continue;
                };
                let body = match &verdict.disposition {
                    VerdictDisposition::Priced(g) => EpisodeBody::Priced(Box::new(PricedEpisode {
                        action: g.action,
                        snapshot: CalibrationSnapshot {
                            sub_scores: g.sub_scores,
                            grade: g.grade,
                            conviction: g.conviction,
                            risk_tier: g.risk_tier,
                            price_targets: g.price_targets.clone(),
                            dead_money: g.dead_money,
                            hurdle: audit.and_then(|a| a.hurdle.clone()),
                            dgs2: input.dgs2,
                            authoring_spot: audit
                                .and_then(|a| a.quick_basis.as_ref())
                                .map(|b| b.spot),
                            // The cap signals in force: the pre-profit overlay's
                            // matched rules, a tripped hard-forensic rule, and a
                            // tripped narrative-vs-reality soft rule — each an
                            // engine-arm annotation the counterfactual re-test
                            // needs (`docs/portfolio-analysis.md` §Outcome
                            // learning).
                            cap_signals: audit
                                .and_then(|a| a.pre_profit.as_ref())
                                .filter(|pp| pp.is_eligible())
                                .map(|pp| pp.consequences.matched_rules.clone())
                                .unwrap_or_default()
                                .into_iter()
                                .chain(
                                    audit
                                        .and_then(|a| a.forensic.as_ref())
                                        .and_then(|f| f.matched_rule.clone()),
                                )
                                .chain(
                                    audit
                                        .and_then(|a| a.narrative.as_ref())
                                        .and_then(|n| n.matched_rule.clone()),
                                )
                                .collect(),
                            grade_parameter_version: audit
                                .and_then(|a| a.grade_parameter_version.clone()),
                            target_parameter_version: audit
                                .and_then(|a| a.target_meta.as_ref())
                                .map(|t| t.parameter_version.clone()),
                            degraded_inputs: audit
                                .map(|a| a.degraded_inputs.clone())
                                .unwrap_or_default(),
                            // The two-arm freeze (v7): both arms' authored values
                            // ride the episode so the scoreboard can score them
                            // long after the run ages out.
                            model_price_targets: g.model_view.price_targets.clone(),
                            model_sub_scores: g.model_view.sub_scores,
                            model_outlook: g.horizon_outlook,
                            engine_outlook: g.engine_view.outlook,
                            engine_conviction: g.engine_view.conviction,
                            engine_action: g.engine_view.action,
                        },
                    })),
                    VerdictDisposition::RoleRiskOnly(r) => {
                        EpisodeBody::RoleRiskOnly(RoleRiskEpisode {
                            action: r.action,
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
                    // Seeded with this run's labeled count — the validated
                    // what-changed audit's, zero on every other open.
                    self_correction_count: self_corrections,
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
    // feeding no lead-time read. Carried audits are skipped: their crossings
    // are prior-run facts that already attached in their own run.
    for audit in input.audits {
        if input.carried_symbols.contains(&audit.symbol.to_ascii_uppercase()) {
            continue;
        }
        let Some(ledger_audit) = &audit.ledger_audit else {
            continue;
        };
        for crossing in &ledger_audit.crossings {
            if crossing.role != crate::portfolio::ConditionRole::Falsifier
                || crossing.outcome != crate::portfolio::CrossingOutcome::Confirmed
            {
                continue;
            }
            // The date the crossing **confirmed** — carried on the crossing from the
            // condition's own eval state, which stamps it once on the confirming
            // pass. Not the consuming run's date: a between-run sweep confirms and
            // the next full run reads that crossing days later, and this is the one
            // stamp `stamp_lead_times` positions against bar dates, so dating it
            // here understated every lead time by the whole sweep-to-run gap — and
            // could sign-flip a falsifier that actually led its drawdown into one
            // that appeared to follow it. A confirmed crossing always carries the
            // stamp its confirming pass wrote; a confirmed one missing it can't
            // occur on a fresh v9 store, so skip it rather than guess a date.
            let Some(confirmed_at) = crossing.confirmed_at.clone() else {
                continue;
            };
            let (target, post_maturity) = {
                // The latest active episode carries the current ledger's
                // conditions; older still-maturing episodes' forecasts predate
                // them. "Latest" is insertion order, like the extend target above.
                let active = episodes
                    .iter()
                    .enumerate()
                    .rfind(|(_, e)| {
                        e.state == EpisodeState::Active
                            && e.symbol.eq_ignore_ascii_case(&audit.symbol)
                    })
                    .map(|(i, _)| i);
                match active {
                    Some(i) => (Some(i), false),
                    None => (
                        episodes
                            .iter()
                            .enumerate()
                            .rfind(|(_, e)| {
                                e.state == EpisodeState::Matured
                                    && e.symbol.eq_ignore_ascii_case(&audit.symbol)
                            })
                            .map(|(i, _)| i),
                        true,
                    ),
                }
            };
            let Some(i) = target else { continue };
            let ep = &mut episodes[i];
            // Dedup on the **standing confirmation**, not the observation that
            // re-raised it. `confirmation_observation_id` is designed to change on
            // every re-raise — an unconsumed confirmed breach re-raises each pass
            // against the newest print until 6g acknowledges it — so keying on it
            // accrued a fresh event per run: on a market-cadence falsifier, ~40
            // over a twelve-month episode, each one separately mis-stamped. The
            // confirmation date is set once when the streak reaches its count and
            // held until the streak resets, so it identifies the standing breach
            // and changes only when a genuinely new one confirms.
            //
            // Accepted collapse: a streak that resets and re-confirms within the
            // same ET session reads as the same event. Same-session reset and
            // re-confirmation is one day's information, and the alternative — the
            // changing observation id — is the defect.
            let duplicate = ep
                .falsifier_events
                .iter()
                .any(|e| e.condition_id == crossing.condition_id && e.confirmed_at == confirmed_at);
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
        // The primary mean quotes price-only where a label's total-return leg was
        // unavailable (`docs/portfolio-analysis.md §Outcome learning` — "any
        // comparison with a missing total-return leg quotes price-only") — a
        // labeled mix, never a silently shrunk population; the pure price-only
        // mean rides beside it.
        if let Some(v) = mean(
            labels
                .iter()
                .map(|l| l.total_return.unwrap_or(l.price_return))
                .collect(),
        ) {
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

/// Compute the derived scorecard reads over the episode store
/// (`docs/portfolio-analysis.md §Outcome learning`). Pure; attribution stays at the
/// cohort level — no per-decision P&L verdict is ever assigned.
pub fn derive_reads(episodes: &[DecisionEpisode]) -> DerivedReads {
    let mut cohorts = Vec::new();
    for &months in &LABEL_WINDOWS_MONTHS {
        let scored: Vec<(&DecisionEpisode, &ScoredLabel)> = episodes
            .iter()
            .filter_map(|ep| scored_for(ep, months).map(|s| (ep, s)))
            .collect();
        // The intrinsic layer: vintage-fresh, model-chosen, priced.
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
                        && matches!(&ep.body, EpisodeBody::Priced(p) if p.action == action)
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
    // only, scored on the price-only label — in return space over the authoring
    // spot (split-safe), split per target-function parameter version (bases
    // never mix). One accumulation, parameterized on the band source, runs for
    // BOTH arms: the engine bands and the model arm's frozen bands share the
    // scorer, the exclusion rules, and the population, so the model-vs-engine
    // head-to-head is fair (`docs/portfolio-analysis.md` §Outcome learning):
    // under v9 both arms ride every priced episode, so they score the same band
    // population.
    #[derive(Default)]
    struct BandAcc {
        scores: Vec<f64>,
        hits: usize,
        base_errors: Vec<f64>,
    }
    /// A band source for the shared accumulation: (episode, window months) → the
    /// arm's (bear, base, bull), `None` when the arm carries no band there.
    type BandSource = dyn Fn(&PricedEpisode, u32) -> Option<(f64, f64, f64)>;
    let band_calibration =
        |band_of: &BandSource| {
            let mut reads = Vec::new();
            for months in [1u32, 12u32] {
                let mut by_version: std::collections::BTreeMap<Option<String>, BandAcc> =
                    std::collections::BTreeMap::new();
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
                    let Some((bear, base, bull)) = band_of(p, months) else {
                        continue;
                    };
                    let Some(spot) = p.snapshot.authoring_spot.filter(|s| *s > 0.0) else {
                        // No authoring spot recorded: the bases can't be
                        // reconciled, so the band is excluded rather than
                        // compared across them.
                        continue;
                    };
                    let Some(bridge) = label.anchor_close.filter(|a| *a > 0.0) else {
                        // No anchor-session close on the label: the realized side
                        // has no same-instant bridge to the authoring basis (the
                        // next-session entry sits an overnight gap away), so the
                        // band is excluded.
                        continue;
                    };
                    let (lo, hi) = (bear.min(bull), bear.max(bull));
                    let (lo_r, hi_r) = (lo / spot - 1.0, hi / spot - 1.0);
                    // Realized return from the same decision instant: end over the
                    // anchor-session close — both sides of the comparison now share
                    // one anchor, so neither a split nor the overnight gap into the
                    // entry can shear it.
                    let realized_r = label.end_price / bridge - 1.0;
                    let acc = by_version
                        .entry(p.snapshot.target_parameter_version.clone())
                        .or_default();
                    if realized_r >= lo_r && realized_r <= hi_r {
                        acc.hits += 1;
                    }
                    acc.scores
                        .push(interval_score(lo_r, hi_r, realized_r, 1.0 - NOMINAL_BAND_COVERAGE));
                    if base != 0.0 {
                        let realized_authoring_basis = spot * (1.0 + realized_r);
                        acc.base_errors.push((realized_authoring_basis - base) / base);
                    }
                }
                if by_version.is_empty() {
                    // Keep the per-window read present (scored: 0) so an empty
                    // store still reports the window rather than omitting it.
                    by_version.insert(None, BandAcc::default());
                }
                for (version, acc) in by_version {
                    let n = acc.scores.len();
                    reads.push(TargetCalibrationRead {
                        window_months: months,
                        parameter_version: version,
                        scored: n,
                        coverage_rate: (n > 0).then(|| acc.hits as f64 / n as f64),
                        nominal_coverage: NOMINAL_BAND_COVERAGE,
                        mean_interval_score: (n > 0)
                            .then(|| acc.scores.iter().sum::<f64>() / n as f64),
                        mean_base_signed_error: (!acc.base_errors.is_empty()).then(|| {
                            acc.base_errors.iter().sum::<f64>() / acc.base_errors.len() as f64
                        }),
                    });
                }
            }
            reads
        };
    let engine_band = |p: &PricedEpisode, months: u32| -> Option<(f64, f64, f64)> {
        let band = match months {
            1 => p.snapshot.price_targets.one_month.as_ref(),
            _ => p.snapshot.price_targets.twelve_month.as_ref(),
        }?;
        Some((band.bear, band.base, band.bull))
    };
    let model_band = |p: &PricedEpisode, months: u32| -> Option<(f64, f64, f64)> {
        let t = &p.snapshot.model_price_targets;
        let band = match months {
            1 => &t.one_month,
            _ => &t.twelve_month,
        };
        Some((band.bear, band.base, band.bull))
    };
    let target_calibration = band_calibration(&engine_band);
    let model_target_calibration = band_calibration(&model_band);

    // The paired head-to-head: only episodes where BOTH arms carry the band (and
    // the shared spot/bridge exclusions pass) enter, and both arms score the same
    // realized outcome — same-events by construction, so the comparison can't be
    // skewed by one arm's easier population (Codex round 1, finding 3).
    let mut head_to_head = Vec::new();
    for months in [1u32, 12u32] {
        let (mut e_scores, mut m_scores) = (Vec::new(), Vec::new());
        let (mut e_hits, mut m_hits) = (0usize, 0usize);
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
            let (Some(eb), Some(mb)) = (engine_band(p, months), model_band(p, months)) else {
                continue;
            };
            let Some(spot) = p.snapshot.authoring_spot.filter(|s| *s > 0.0) else {
                continue;
            };
            let Some(bridge) = label.anchor_close.filter(|a| *a > 0.0) else {
                continue;
            };
            let realized_r = label.end_price / bridge - 1.0;
            let score = |band: (f64, f64, f64), scores: &mut Vec<f64>, hits: &mut usize| {
                let (lo, hi) = (band.0.min(band.2), band.0.max(band.2));
                let (lo_r, hi_r) = (lo / spot - 1.0, hi / spot - 1.0);
                if realized_r >= lo_r && realized_r <= hi_r {
                    *hits += 1;
                }
                scores.push(interval_score(lo_r, hi_r, realized_r, 1.0 - NOMINAL_BAND_COVERAGE));
            };
            score(eb, &mut e_scores, &mut e_hits);
            score(mb, &mut m_scores, &mut m_hits);
        }
        let n = e_scores.len();
        head_to_head.push(HeadToHeadRead {
            window_months: months,
            scored: n,
            engine_mean_interval_score: (n > 0).then(|| e_scores.iter().sum::<f64>() / n as f64),
            model_mean_interval_score: (n > 0).then(|| m_scores.iter().sum::<f64>() / n as f64),
            engine_coverage_rate: (n > 0).then(|| e_hits as f64 / n as f64),
            model_coverage_rate: (n > 0).then(|| m_hits as f64 / n as f64),
        });
    }

    // Outlook direction hit-rate, both arms: each horizon read scored against the
    // realized price-only sign at its mapped window (short → 1-month, mid →
    // 6-month, long → 12-month), vintage-fresh episodes only; a neutral read is
    // counted beside the hit-rate, never inside it.
    let mut outlook_direction = Vec::new();
    for (arm, pick) in [
        (
            "engine",
            &(|p: &PricedEpisode| p.snapshot.engine_outlook)
                as &dyn Fn(&PricedEpisode) -> crate::portfolio::HorizonOutlook,
        ),
        ("model", &(|p: &PricedEpisode| p.snapshot.model_outlook)),
    ] {
        for (months, read_of) in [
            (1u32, &(|o: &crate::portfolio::HorizonOutlook| o.short)
                as &dyn Fn(&crate::portfolio::HorizonOutlook) -> crate::portfolio::HorizonRead),
            (6u32, &(|o: &crate::portfolio::HorizonOutlook| o.mid)),
            (12u32, &(|o: &crate::portfolio::HorizonOutlook| o.long)),
        ] {
            let (mut scored, mut hits, mut neutral) = (0usize, 0usize, 0usize);
            for ep in episodes {
                if !ep.vintage_fresh {
                    continue;
                }
                let EpisodeBody::Priced(p) = &ep.body else {
                    continue;
                };
                let outlook = pick(p);
                let Some(label) = scored_for(ep, months) else {
                    continue;
                };
                let pr = label.price_return;
                match read_of(&outlook) {
                    crate::portfolio::HorizonRead::Neutral => neutral += 1,
                    crate::portfolio::HorizonRead::Bullish => {
                        scored += 1;
                        if pr > 0.0 {
                            hits += 1;
                        }
                    }
                    crate::portfolio::HorizonRead::Bearish => {
                        scored += 1;
                        if pr < 0.0 {
                            hits += 1;
                        }
                    }
                }
            }
            outlook_direction.push(OutlookDirectionRead {
                arm: arm.to_string(),
                window_months: months,
                scored,
                hits,
                neutral,
            });
        }
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
        model_target_calibration,
        head_to_head,
        outlook_direction,
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
    // The model-vs-engine head-to-head — the PAIRED read only (same episodes,
    // both arms scoring the same realized outcomes), never two independently
    // pooled populations.
    for h in &records.reads.head_to_head {
        if let (Some(model), Some(engine)) =
            (h.model_mean_interval_score, h.engine_mean_interval_score)
        {
            lines.push(format!(
                "model-vs-engine {}-month interval score (paired, {} bands): model \
                 {model:.4} vs engine {engine:.4} — lower is better",
                h.window_months, h.scored
            ));
        }
    }
    lines.push(records.reads.eligibility.note.clone());
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{
        GradedVerdict, HorizonOutlook, HorizonRead, OptionsSignal, PriceTarget, ThesisLedger,
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
            action_rationale: String::new(),
            model_view: crate::portfolio::ModelView {
                sub_scores: SubScores {
                    quality: 70.0,
                    valuation: 60.0,
                    momentum: 55.0,
                    risk: 65.0,
                },
                letter: Grade::B,
                price_targets: crate::portfolio::ModelPriceTargets {
                    one_month: crate::portfolio::ModelPriceTarget {
                        base: 102.0,
                        bear: 95.0,
                        bull: 108.0,
                    },
                    twelve_month: crate::portfolio::ModelPriceTarget {
                        base: 120.0,
                        bear: 90.0,
                        bull: 150.0,
                    },
                },
                self_assessment: String::new(),
            },
            engine_view: crate::portfolio::EngineView {
                outlook: HorizonOutlook {
                    short: HorizonRead::Neutral,
                    mid: HorizonRead::Bullish,
                    long: HorizonRead::Bullish,
                },
                conviction: Conviction::Medium,
                action,
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

    fn ledger(_low: f64, _high: f64) -> ThesisLedger {
        ThesisLedger {
            branch: crate::portfolio::LedgerBranch::Priced,
            original_thesis: "t".into(),
            current_thesis: "t".into(),
            key_drivers: vec![],
            monitor: vec![],
            what_must_improve: String::new(),
            what_must_not_break: String::new(),
            conditions: vec![],
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
            side_reversed: false,
        }
    }

    fn fresh(mut v: HoldingVerdict, created_at: &str) -> HoldingVerdict {
        v.analyzed_at = Some(created_at.to_string());
        v
    }

    fn empty_carried() -> &'static HashSet<String> {
        static EMPTY: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(HashSet::new)
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
            unreadable_active_symbols: HashSet::new(),
            carried_symbols: empty_carried(),
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

    fn audit_with_wc(symbol: &str, thesis_changed: bool, self_corrections: u32) -> HoldingAudit {
        HoldingAudit {
            symbol: symbol.into(),
            metrics: Default::default(),
            sources: vec![],
            model_ids: vec![],
            prompt_version: "test".into(),
            degraded_inputs: vec![],
            action_annotations: vec![],
            target_meta: None,
            grade_parameter_version: None,
            ledger_audit: None,
            quick_basis: None,
            authoring_close: None,
            fund_exposure: None,
            pre_profit: None,
            hurdle: None,
            forensic: None,
            tech_event_pre_flag: None,
            short_interest: None,
            implied_expectations: None,
            narrative: None,
            option_overlay: None,
            what_changed_audit: Some(crate::portfolio::WhatChangedAudit {
                entries: vec![],
                input_delta: vec![],
                downgrades: vec![],
                self_correction_count: self_corrections,
                thesis_changed,
            }),
            research: None,
        }
    }

    #[test]
    fn a_thesis_change_opens_with_the_action_unchanged_and_seeds_the_count() {
        let c1 = "2026-08-04T12:00:00+00:00";
        let prior = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &prior, None, &sector), &mut episodes);
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].self_correction_count, 0);

        // Same branch, same action — but this run's validated what-changed audit
        // records a thesis change with two labeled self-corrections: the
        // standing-thesis leg opens a successor episode carrying the count.
        let c2 = "2026-08-11T12:00:00+00:00";
        let verdicts = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c2)];
        let audits = vec![audit_with_wc("AAPL", true, 2)];
        let mut input = plan_input("run-2", c2, &verdicts, Some(&prior), &sector);
        input.audits = &audits;
        let s = plan_episodes(&input, &mut episodes);
        assert_eq!(s.opened.len(), 1);
        assert_eq!(s.opened[0].reasons, vec![OpenReason::ThesisChange]);
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[1].self_correction_count, 2);
    }

    #[test]
    fn a_carried_audits_thesis_flag_never_opens() {
        // A selective carry re-persists the prior audit — its what-changed flags
        // are its own run's facts, so the standing-thesis leg must not re-fire
        // off them (the carried-crossings premise).
        let c1 = "2026-08-04T12:00:00+00:00";
        let prior = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &prior, None, &sector), &mut episodes);

        let c2 = "2026-08-11T12:00:00+00:00";
        // Carried: the verdict keeps its prior vintage (analyzed_at != created_at).
        let verdicts = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let audits = vec![audit_with_wc("AAPL", true, 1)];
        let mut input = plan_input("run-2", c2, &verdicts, Some(&prior), &sector);
        input.audits = &audits;
        let s = plan_episodes(&input, &mut episodes);
        assert!(s.opened.is_empty());
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].self_correction_count, 0);
        assert_eq!(episodes[0].observations[0].kind, ObservationKind::Carried);
    }

    #[test]
    fn episode_decision_thesis_leg_opens_only_on_the_signal() {
        let prior = verdict("AAPL", Action::Hold, (0.03, 0.06));
        let current = verdict("AAPL", Action::Hold, (0.03, 0.06));
        assert_eq!(
            episode_decision(Some(&prior), &current, true, None, false),
            EpisodeDecision::Extend(ObservationKind::Reaffirmed)
        );
        assert_eq!(
            episode_decision(Some(&prior), &current, true, None, true),
            EpisodeDecision::Open(vec![OpenReason::ThesisChange])
        );
        // Beside an action change the thesis signal records as a second reason.
        let moved = verdict("AAPL", Action::Trim, (0.03, 0.06));
        assert_eq!(
            episode_decision(Some(&prior), &moved, true, None, true),
            EpisodeDecision::Open(vec![OpenReason::ActionChange, OpenReason::ThesisChange])
        );
    }

    #[test]
    fn a_backwards_clock_step_cannot_shadow_the_newly_opened_episode() {
        // Run 1 opens the episode. Run 2 changes the action, so it opens a
        // successor — but its `created_at` is EARLIER than run 1's, the shape a
        // clock correction or an NTP step produces. Under `max_by(anchor_at)` every
        // later extension, falsifier event and inherited sector identity attached
        // to the run-1 predecessor forever, and the successor — the episode
        // carrying the current recommendation — accrued nothing for the rest of its
        // twelve months. Insertion order cannot invert.
        let c1 = "2026-08-11T12:00:00+00:00";
        let prior = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &prior, None, &sector), &mut episodes);

        let c2 = "2026-08-04T12:00:00+00:00"; // a week BEFORE run 1
        let changed = vec![fresh(verdict("AAPL", Action::Trim, (0.03, 0.06)), c2)];
        let s = plan_episodes(
            &plan_input("run-2", c2, &changed, Some(&prior), &sector),
            &mut episodes,
        );
        assert_eq!(s.opened.len(), 1, "the action change still opens a successor");
        assert_eq!(episodes.len(), 2);
        assert!(
            episodes[1].anchor_at < episodes[0].anchor_at,
            "the fixture must actually invert the wall clock"
        );
        let successor = episodes[1].episode_id.clone();

        // Run 3 re-affirms: the observation must land on the successor.
        let c3 = "2026-08-18T12:00:00+00:00";
        let same = vec![fresh(verdict("AAPL", Action::Trim, (0.03, 0.06)), c3)];
        let s3 = plan_episodes(
            &plan_input("run-3", c3, &same, Some(&changed), &sector),
            &mut episodes,
        );
        assert_eq!(s3.extended, vec!["AAPL".to_string()]);
        let carrying: Vec<&str> = episodes
            .iter()
            .filter(|e| !e.observations.is_empty())
            .map(|e| e.episode_id.as_str())
            .collect();
        assert_eq!(
            carrying,
            vec![successor.as_str()],
            "the extension must attach to the successor, not the wall-clock-newer predecessor"
        );
    }

    #[test]
    fn an_action_change_opens_a_fresh_episode_and_a_reaffirmation_extends() {
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

        // A re-affirmed action extends; episode identity reads the action alone
        // under the tunnel-vision contract (the ledger carries no weight range).
        let c3 = "2026-08-18T12:00:00+00:00";
        let reaffirmed = vec![fresh(verdict("AAPL", Action::Trim, (0.02, 0.04)), c3)];
        let s = plan_episodes(
            &plan_input("run-3", c3, &reaffirmed, Some(&action_changed), &sector),
            &mut episodes,
        );
        assert!(s.opened.is_empty());
        assert_eq!(episodes.len(), 2);
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
    fn a_pre_outcome_prior_run_seeds_a_debut_episode() {
        // The upgrade seam: the prior run predates the episode machinery, so the
        // store is empty while a prior verdict exists. An unchanged
        // recommendation must still seed the symbol's debut episode — otherwise
        // stable holdings stay outside outcome learning until their
        // recommendation happens to change.
        let c1 = "2026-08-04T12:00:00+00:00";
        let prior = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let c2 = "2026-08-11T12:00:00+00:00";
        let same = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c2)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        let s = plan_episodes(
            &plan_input("run-2", c2, &same, Some(&prior), &sector),
            &mut episodes,
        );
        assert_eq!(s.opened.len(), 1, "the never-seeded symbol debuts");
        assert_eq!(s.opened[0].reasons, vec![OpenReason::Debut]);
        assert!(s.extended.is_empty());
        assert_eq!(episodes.len(), 1);

        // An abstained current verdict still never opens, seeded or not.
        let mut abstained = verdict("MSFT", Action::Hold, (0.03, 0.06));
        abstained.disposition = VerdictDisposition::InsufficientEvidence {
            reason: "thin".into(),
        };
        let prior_msft = vec![fresh(verdict("MSFT", Action::Hold, (0.03, 0.06)), c1)];
        let s = plan_episodes(
            &plan_input("run-2", c2, &[abstained], Some(&prior_msft), &sector),
            &mut episodes,
        );
        assert!(s.opened.is_empty());
        assert_eq!(episodes.len(), 1, "no MSFT episode was minted");
    }

    #[test]
    fn a_lost_active_row_re_seeds_beside_readable_history() {
        // The corrupt-latest-active case: readable matured AAPL history exists,
        // so the symbol is "seeded", but the active row carrying the current
        // decision was unreadable — the recovery seam must re-open tracking.
        let c1 = "2025-08-04T12:00:00+00:00";
        let c2 = "2026-08-11T12:00:00+00:00";
        let mut matured = old_episode("AAPL", c1);
        matured.state = EpisodeState::Matured;
        let mut episodes = vec![matured];
        let skipped = vec![store::SkippedEpisodeRow {
            episode_id: "ep-bad".into(),
            symbol: "AAPL".into(),
            anchor_at: "2026-06-01T00:00:00+00:00".into(),
            state: "active".into(),
            // The matured row was read BEFORE the corrupt one, so nothing readable
            // supersedes it — supersession is insertion order, not the timestamp.
            readable_before: 1,
        }];
        let lost = lost_active_symbols(&skipped, &episodes);
        assert!(lost.contains("AAPL"));

        let prior = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let same = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c2)];
        let sector = HashMap::new();
        let mut input = plan_input("run-2", c2, &same, Some(&prior), &sector);
        input.unreadable_active_symbols = lost;
        let s = plan_episodes(&input, &mut episodes);
        assert_eq!(s.opened.len(), 1, "the lost decision re-enters tracking");
        assert_eq!(s.opened[0].reasons, vec![OpenReason::Debut]);
        assert_eq!(episodes.len(), 2);

        // The recovery episode's newer anchor supersedes the lost row: the flag
        // dies, so a later post-maturity re-affirmation can never re-open off it.
        assert!(lost_active_symbols(&skipped, &episodes).is_empty());

        // A readable active episode *older* than the lost row is a predecessor
        // whose forecast stopped accruing when the lost successor opened —
        // recovery still re-seeds, and the predecessor absorbs no observation.
        let mut with_older_active = vec![old_episode("MSFT", c1)];
        let skipped_msft = vec![store::SkippedEpisodeRow {
            episode_id: "ep-bad-2".into(),
            symbol: "MSFT".into(),
            anchor_at: "2026-06-01T00:00:00+00:00".into(),
            state: "active".into(),
            // Inserted after the predecessor: nothing readable follows it.
            readable_before: 1,
        }];
        let lost = lost_active_symbols(&skipped_msft, &with_older_active);
        assert!(lost.contains("MSFT"));
        let prior_m = vec![fresh(verdict("MSFT", Action::Hold, (0.03, 0.06)), c1)];
        let same_m = vec![fresh(verdict("MSFT", Action::Hold, (0.03, 0.06)), c2)];
        let mut input = plan_input("run-2", c2, &same_m, Some(&prior_m), &sector);
        input.unreadable_active_symbols = lost;
        let s = plan_episodes(&input, &mut with_older_active);
        assert_eq!(
            s.opened.len(),
            1,
            "an older active predecessor never absorbs the lost decision"
        );
        assert_eq!(s.opened[0].reasons, vec![OpenReason::Debut]);
        assert!(with_older_active[0].observations.is_empty());
        assert_eq!(with_older_active.len(), 2);

        // A readable episode *newer* than the lost row supersedes it entirely:
        // the flag never forms, and the ordinary extend applies.
        let c_new = "2026-07-01T12:00:00+00:00";
        let mut with_newer_active = vec![old_episode("NVDA", c_new)];
        let skipped_nvda = vec![store::SkippedEpisodeRow {
            episode_id: "ep-bad-3".into(),
            symbol: "NVDA".into(),
            anchor_at: "2026-06-01T00:00:00+00:00".into(),
            state: "active".into(),
            // The readable episode was inserted AFTER the corrupt row, so it
            // supersedes it.
            readable_before: 0,
        }];
        let lost = lost_active_symbols(&skipped_nvda, &with_newer_active);
        assert!(lost.is_empty(), "a newer readable episode supersedes the lost row");
        let prior_n = vec![fresh(verdict("NVDA", Action::Hold, (0.03, 0.06)), c_new)];
        let same_n = vec![fresh(verdict("NVDA", Action::Hold, (0.03, 0.06)), c2)];
        let mut input = plan_input("run-2", c2, &same_n, Some(&prior_n), &sector);
        input.unreadable_active_symbols = lost;
        let s = plan_episodes(&input, &mut with_newer_active);
        assert!(s.opened.is_empty());
        assert_eq!(s.extended, vec!["NVDA".to_string()]);
        assert_eq!(with_newer_active.len(), 1);
    }

    #[test]
    fn extensions_and_crossings_attach_to_the_latest_active_episode() {
        // An action change opens a successor while the older episode keeps
        // maturing — both active. Re-affirmations and falsifier crossings must
        // land on the latest episode (the current recommendation / ledger), not
        // the oldest still-labeling one.
        let c1 = "2026-08-04T12:00:00+00:00";
        let hold = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &hold, None, &sector), &mut episodes);
        let c2 = "2026-08-11T12:00:00+00:00";
        let trim = vec![fresh(verdict("AAPL", Action::Trim, (0.03, 0.06)), c2)];
        plan_episodes(
            &plan_input("run-2", c2, &trim, Some(&hold), &sector),
            &mut episodes,
        );
        assert_eq!(episodes.len(), 2);
        assert!(episodes.iter().all(|e| e.state == EpisodeState::Active));

        let c3 = "2026-08-18T12:00:00+00:00";
        let trim_again = vec![fresh(verdict("AAPL", Action::Trim, (0.03, 0.06)), c3)];
        let audit = HoldingAudit {
            what_changed_audit: None,
            research: None,
            symbol: "AAPL".into(),
            metrics: Default::default(),
            sources: vec![],
            model_ids: vec![],
            prompt_version: "portfolio-v5".into(),
            degraded_inputs: vec![],
            action_annotations: vec![],
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
                    // The engine stamped this on the confirming pass with the run's
                    // ET session date (`run_date`).
                    confirmed_at: Some("2026-08-18".into()),
                }],
                ..Default::default()
            }),
            quick_basis: None,
            authoring_close: None,
            fund_exposure: None,
            pre_profit: None,
            hurdle: None,
            forensic: None,
            tech_event_pre_flag: None,
            short_interest: None,
            implied_expectations: None,
            narrative: None,
            option_overlay: None,
        };
        let audits = vec![audit];
        let mut input = plan_input("run-3", c3, &trim_again, Some(&trim), &sector);
        input.audits = &audits;
        let s = plan_episodes(&input, &mut episodes);
        assert_eq!(s.extended, vec!["AAPL".to_string()]);
        assert!(
            episodes[0].observations.is_empty(),
            "the older episode stopped accruing at the state change"
        );
        assert_eq!(episodes[1].observations.len(), 1);
        assert!(
            episodes[0].falsifier_events.is_empty(),
            "the crossing belongs to the episode carrying the current ledger"
        );
        assert_eq!(episodes[1].falsifier_events.len(), 1);
        // Noon UTC = the same ET day: the confirmation stamps the run's session.
        assert_eq!(episodes[1].falsifier_events[0].confirmed_at, "2026-08-18");
    }

    #[test]
    fn a_confirmation_stamps_the_et_session_date() {
        // An evening-ET run: 2026-08-19 01:30 UTC = 2026-08-18 21:30 EDT. The
        // engine stamps the crossing's `confirmed_at` with the run's ET session
        // date (`run_date`, ET-derived in `job.rs`), so the confirmation belongs
        // to the ET session whose print confirmed it — the UTC date prefix (the
        // 19th) would place it one session late in the lead-time read. The
        // consumer carries that stamp straight onto the event.
        let c1 = "2026-08-19T01:30:00+00:00";
        let hold = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let audit = HoldingAudit {
            what_changed_audit: None,
            research: None,
            symbol: "AAPL".into(),
            metrics: Default::default(),
            sources: vec![],
            model_ids: vec![],
            prompt_version: "portfolio-v5".into(),
            degraded_inputs: vec![],
            action_annotations: vec![],
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
                    observation_id: "2026-08-18".into(),
                    // The engine stamped this on the confirming pass with the run's
                    // ET session date (`run_date`).
                    confirmed_at: Some("2026-08-18".into()),
                }],
                ..Default::default()
            }),
            quick_basis: None,
            authoring_close: None,
            fund_exposure: None,
            pre_profit: None,
            hurdle: None,
            forensic: None,
            tech_event_pre_flag: None,
            short_interest: None,
            implied_expectations: None,
            narrative: None,
            option_overlay: None,
        };
        let audits = vec![audit];
        let mut episodes = Vec::new();
        let mut input = plan_input("run-1", c1, &hold, None, &sector);
        input.audits = &audits;
        plan_episodes(&input, &mut episodes);
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].falsifier_events.len(), 1);
        assert_eq!(episodes[0].falsifier_events[0].confirmed_at, "2026-08-18");
    }

    #[test]
    fn confirmed_falsifier_crossings_attach_to_the_carrying_episode() {
        let c1 = "2026-08-04T12:00:00+00:00";
        let verdicts = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        let audit = HoldingAudit {
            what_changed_audit: None,
            research: None,
            symbol: "AAPL".into(),
            metrics: Default::default(),
            sources: vec![],
            model_ids: vec![],
            prompt_version: "portfolio-v5".into(),
            degraded_inputs: vec![],
            action_annotations: vec![],
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
                    // The engine stamped this on the confirming pass with the run's
                    // ET session date (`run_date`).
                    confirmed_at: Some("2026-08-04".into()),
                }],
                ..Default::default()
            }),
            quick_basis: None,
            authoring_close: None,
            fund_exposure: None,
            pre_profit: None,
            hurdle: None,
            forensic: None,
            tech_event_pre_flag: None,
            short_interest: None,
            implied_expectations: None,
            narrative: None,
            option_overlay: None,
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

    /// An audit carrying one confirmed falsifier crossing: the observation id it was
    /// re-raised against, and the confirmation date the engine stamps on it.
    fn confirmed_crossing(observation_id: &str, confirmed_at: &str) -> HoldingAudit {
        HoldingAudit {
            what_changed_audit: None,
            research: None,
            symbol: "AAPL".into(),
            metrics: Default::default(),
            sources: vec![],
            model_ids: vec![],
            prompt_version: "portfolio-v5".into(),
            degraded_inputs: vec![],
            action_annotations: vec![],
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
                    observation_id: observation_id.into(),
                    confirmed_at: Some(confirmed_at.to_string()),
                }],
                ..Default::default()
            }),
            quick_basis: None,
            authoring_close: None,
            fund_exposure: None,
            pre_profit: None,
            hurdle: None,
            forensic: None,
            tech_event_pre_flag: None,
            short_interest: None,
            implied_expectations: None,
            narrative: None,
            option_overlay: None,
        }
    }

    #[test]
    fn one_standing_breach_accrues_one_event_however_often_it_re_raises() {
        // The defect's teeth. An unconsumed confirmed breach re-raises every pass
        // against the NEWEST print until 6g acknowledges it, so
        // `confirmation_observation_id` changes each run BY DESIGN. Keyed on it, one
        // standing breach accrued a fresh event per run — about forty over a
        // twelve-month episode on a market-cadence falsifier, each separately
        // mis-stamped. Keyed on the confirmation date, which is set once when the
        // streak reaches its count and held until it resets, the same breach is one
        // event no matter how many passes re-raise it.
        let c1 = "2026-08-04T12:00:00+00:00";
        let verdicts = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-0", c1, &verdicts, None, &sector), &mut episodes);

        // Three later passes, each re-raising the SAME standing confirmation
        // (`confirmed_at` fixed at 2026-08-05) against a newer close each time.
        for (run, obs, created) in [
            ("run-1", "2026-08-05", "2026-08-05T12:00:00+00:00"),
            ("run-2", "2026-08-06", "2026-08-06T12:00:00+00:00"),
            ("run-3", "2026-08-07", "2026-08-07T12:00:00+00:00"),
        ] {
            let audits = vec![confirmed_crossing(obs, "2026-08-05")];
            let mut input = plan_input(run, created, &verdicts, Some(&verdicts), &sector);
            input.audits = &audits;
            plan_episodes(&input, &mut episodes);
        }
        assert_eq!(
            episodes[0].falsifier_events.len(),
            1,
            "one standing breach is one event, however many passes re-raise it"
        );
        assert_eq!(
            episodes[0].falsifier_events[0].confirmed_at, "2026-08-05",
            "stamped from the CONFIRMING pass, not the run that consumed the crossing"
        );

        // A genuine re-confirmation after a reset is a distinct standing breach and
        // does accrue its own event.
        let audits = vec![confirmed_crossing("2026-09-10", "2026-09-10")];
        let mut input = plan_input("run-4", "2026-09-10T12:00:00+00:00", &verdicts, Some(&verdicts), &sector);
        input.audits = &audits;
        plan_episodes(&input, &mut episodes);
        assert_eq!(episodes[0].falsifier_events.len(), 2);
    }

    #[test]
    fn carried_audit_crossings_never_attach_as_fresh_events() {
        // A carried audit's `ledger_audit.crossings` are its PRIOR run's facts —
        // they attached to an episode in that run. Re-processing them (the
        // whole-audit carry rides `input.audits`) against an episode newly
        // opened this run would fabricate a falsifier confirmation dated today
        // (the new episode's empty event list defeats the per-episode dedup).
        let c1 = "2026-08-04T12:00:00+00:00";
        let verdicts = vec![fresh(verdict("AAPL", Action::Add, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        let audit = HoldingAudit {
            what_changed_audit: None,
            research: None,
            symbol: "AAPL".into(),
            metrics: Default::default(),
            sources: vec![],
            model_ids: vec![],
            prompt_version: "portfolio-v7".into(),
            degraded_inputs: vec![],
            action_annotations: vec![],
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
                    // The engine stamped this on the confirming pass with the run's
                    // ET session date (`run_date`).
                    confirmed_at: Some("2026-08-04".into()),
                }],
                ..Default::default()
            }),
            quick_basis: None,
            authoring_close: None,
            fund_exposure: None,
            pre_profit: None,
            hurdle: None,
            forensic: None,
            tech_event_pre_flag: None,
            short_interest: None,
            implied_expectations: None,
            narrative: None,
            option_overlay: None,
        };
        let audits = vec![audit];
        let mut input = plan_input("run-1", c1, &verdicts, None, &sector);
        input.audits = &audits;
        plan_episodes(&input, &mut episodes);
        assert_eq!(episodes[0].falsifier_events.len(), 1, "the fresh run attaches");

        // Run 2: the verdict's action changed (a new episode opens) and the
        // audit rides the carry — the month-old crossing must not re-attach.
        let c2 = "2026-09-08T12:00:00+00:00";
        let mut carried_verdict = verdict("AAPL", Action::Hold, (0.03, 0.06));
        carried_verdict.analyzed_at = Some(c1.to_string());
        let current = vec![carried_verdict];
        let carried: HashSet<String> = ["AAPL".to_string()].into();
        let mut input2 = plan_input("run-2", c2, &current, Some(&verdicts), &sector);
        input2.audits = &audits;
        input2.carried_symbols = &carried;
        plan_episodes(&input2, &mut episodes);
        let total_events: usize = episodes.iter().map(|e| e.falsifier_events.len()).sum();
        assert_eq!(
            total_events, 1,
            "no fresh event from a carried audit: {episodes:#?}"
        );
    }

    #[test]
    fn an_action_change_across_an_abstention_opens_instead_of_extending() {
        // Run 1 recommends Hold. Run 2 abstains, retaining the standing ledger, so
        // the episode extends. Run 3 comes back fresh with Trim — the
        // recommendation has MOVED across the abstention.
        //
        // The abstained verdict carries no action to compare against
        // (`InsufficientEvidence` has none), so before this fix run 3 compared only
        // branch and weight range, found them unchanged, and EXTENDED the episode it
        // had just superseded: the Trim forecast accrued onto the Hold episode's
        // window, and calibration scored the Hold decision against observations
        // made after it stopped being the recommendation.
        let c1 = "2026-08-04T12:00:00+00:00";
        let hold = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &hold, None, &sector), &mut episodes);
        assert_eq!(episodes.len(), 1);

        let c2 = "2026-08-11T12:00:00+00:00";
        let mut abstained = verdict("AAPL", Action::Hold, (0.03, 0.06));
        abstained.disposition = VerdictDisposition::InsufficientEvidence {
            reason: "inconclusive re-read".into(),
        };
        let abstained = vec![fresh(abstained, c2)];
        let s2 = plan_episodes(
            &plan_input("run-2", c2, &abstained, Some(&hold), &sector),
            &mut episodes,
        );
        assert_eq!(s2.extended, vec!["AAPL".to_string()], "an abstention extends");
        assert_eq!(episodes.len(), 1);

        let c3 = "2026-08-18T12:00:00+00:00";
        let trim = vec![fresh(verdict("AAPL", Action::Trim, (0.03, 0.06)), c3)];
        let s3 = plan_episodes(
            &plan_input("run-3", c3, &trim, Some(&abstained), &sector),
            &mut episodes,
        );
        assert!(
            s3.extended.is_empty(),
            "the moved recommendation must not extend the superseded episode"
        );
        assert_eq!(s3.opened.len(), 1);
        assert!(
            s3.opened[0].reasons.contains(&OpenReason::ActionChange),
            "the action moved across the abstention: {:?}",
            s3.opened[0].reasons
        );
        assert_eq!(episodes.len(), 2);
    }

    #[test]
    fn an_unchanged_recommendation_across_an_abstention_still_extends() {
        // The other half: when nothing actually moved, the abstention's episode
        // keeps accruing. The fix must not mint an episode per abstention.
        let c1 = "2026-08-04T12:00:00+00:00";
        let hold = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c1)];
        let sector = HashMap::new();
        let mut episodes = Vec::new();
        plan_episodes(&plan_input("run-1", c1, &hold, None, &sector), &mut episodes);

        let c2 = "2026-08-11T12:00:00+00:00";
        let mut abstained = verdict("AAPL", Action::Hold, (0.03, 0.06));
        abstained.disposition = VerdictDisposition::InsufficientEvidence {
            reason: "inconclusive re-read".into(),
        };
        let abstained = vec![fresh(abstained, c2)];
        plan_episodes(
            &plan_input("run-2", c2, &abstained, Some(&hold), &sector),
            &mut episodes,
        );

        let c3 = "2026-08-18T12:00:00+00:00";
        let same = vec![fresh(verdict("AAPL", Action::Hold, (0.03, 0.06)), c3)];
        let s3 = plan_episodes(
            &plan_input("run-3", c3, &same, Some(&abstained), &sector),
            &mut episodes,
        );
        assert_eq!(s3.extended, vec!["AAPL".to_string()]);
        assert!(s3.opened.is_empty());
        assert_eq!(episodes.len(), 1, "no episode churn from an abstention alone");
    }

    #[test]
    fn a_priced_verdict_after_a_ledger_less_abstention_opens_as_debut() {
        // A debut abstention retained no standing ledger — nothing is
        // comparable, so the first priced verdict is a DEBUT open, never a
        // fabricated weight-range change against a never-committed range.
        let mut abstained = verdict("AAPL", Action::Hold, (0.03, 0.06));
        abstained.disposition = VerdictDisposition::InsufficientEvidence {
            reason: "debut abstention".into(),
        };
        abstained.thesis_ledger = None;
        let current = verdict("AAPL", Action::Hold, (0.03, 0.06));
        // No standing episode either — a debut abstention was never seeded.
        let decision = episode_decision(Some(&abstained), &current, true, None, false);
        assert_eq!(
            decision,
            EpisodeDecision::Open(vec![OpenReason::Debut]),
            "ledger-less abstained prior must read as a debut"
        );
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

    /// Wraps [`SyntheticPrices`] recording every `daily_closes` call's
    /// `(symbol, from)` — the fetch-floor pin.
    struct RecordingPrices {
        inner: SyntheticPrices,
        calls: std::cell::RefCell<Vec<(String, NaiveDate)>>,
    }

    impl OutcomePriceSource for RecordingPrices {
        fn daily_closes(
            &self,
            symbol: &str,
            from: NaiveDate,
            to: NaiveDate,
        ) -> Result<Vec<DatedValue>> {
            self.calls.borrow_mut().push((symbol.to_string(), from));
            self.inner.daily_closes(symbol, from, to)
        }
        fn dividend_history(
            &self,
            symbol: &str,
            from: NaiveDate,
            to: NaiveDate,
        ) -> Result<Vec<DatedValue>> {
            self.inner.dividend_history(symbol, from, to)
        }
    }

    fn old_episode(symbol: &str, anchor_at: &str) -> DecisionEpisode {
        // The ET session date, as the production open path stamps it.
        let anchor = crate::market_clock::et_date_of(anchor_at).unwrap();
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
                    authoring_spot: Some(100.0),
                    cap_signals: vec![],
                    grade_parameter_version: Some("grade-v2".into()),
                    target_parameter_version: Some("targets-v3".into()),
                    degraded_inputs: vec![],
                    // The two-arm freeze: a model band wider than the engine's and
                    // opposite-direction outlooks, so the head-to-head reads have
                    // something to distinguish.
                    model_price_targets: crate::portfolio::ModelPriceTargets {
                        one_month: crate::portfolio::ModelPriceTarget {
                            base: 108.0,
                            bear: 90.0,
                            bull: 125.0,
                        },
                        twelve_month: crate::portfolio::ModelPriceTarget {
                            base: 140.0,
                            bear: 70.0,
                            bull: 200.0,
                        },
                    },
                    model_sub_scores: SubScores {
                        quality: 80.0,
                        valuation: 40.0,
                        momentum: 60.0,
                        risk: 70.0,
                    },
                    model_outlook: crate::portfolio::HorizonOutlook {
                        short: crate::portfolio::HorizonRead::Bullish,
                        mid: crate::portfolio::HorizonRead::Bullish,
                        long: crate::portfolio::HorizonRead::Bullish,
                    },
                    engine_outlook: crate::portfolio::HorizonOutlook {
                        short: crate::portfolio::HorizonRead::Bearish,
                        mid: crate::portfolio::HorizonRead::Neutral,
                        long: crate::portfolio::HorizonRead::Bearish,
                    },
                    engine_conviction: Conviction::Medium,
                    engine_action: Action::Hold,
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
        // Relative legs computed against the market benchmark and the stamped
        // XLK benchmark.
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
    fn an_evening_et_anchor_keys_entry_and_bridge_to_the_et_session() {
        // 2026-02-04 01:30 UTC = 2026-02-03 20:30 EST: the decision belongs to
        // the ET session of Tue the 3rd. Entry = the next session's close (Wed
        // the 4th) and the bridge = the 3rd's close. The old UTC-prefix dating
        // anchored on the 4th — entry one session late (the 5th) and a bridge
        // close from a session traded entirely after the decision.
        let conn = mem_conn();
        let mut episodes = vec![old_episode("ETAN", "2026-02-04T01:30:00+00:00")];
        let source = SyntheticPrices {
            fail_dividends: false,
        };
        let mut ctx = SeriesCtx::new(&conn, Some(&source));
        let today = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        mature_labels(&mut episodes, &mut ctx, today, "2026-05-01");
        let scored = scored_for(&episodes[0], 1).expect("1-month scored");
        assert_eq!(scored.entry_date, "2026-02-04", "next session after the ET day");
        // The bridge close is the ET session's own bar: fetch floor 2026-02-03
        // − 7 pad = 01-27 (i=0), so 02-03 is i=7 → 100 + 4 + 0.7.
        let bridge = scored.anchor_close.expect("bridge covered");
        assert!((bridge - 104.7).abs() < 1e-9, "{bridge}");
        assert!((scored.entry_price - 104.8).abs() < 1e-9, "{}", scored.entry_price);
    }

    #[test]
    fn the_bridge_keys_at_the_intrinsic_vintage_healed_by_the_shared_fetch_floor() {
        // Two active episodes on one symbol: EP-OLD anchored 02-10 (vintage
        // fresh) and EP-NEW anchored 03-10 carrying an intrinsic vintage of
        // 02-10 (a rule-demotion open). The symbol fetch floors at the earliest
        // active anchor (02-10), so EP-NEW's bridge session is covered — and the
        // bridge keys at the intrinsic vintage's session close, not the episode
        // anchor's (which would shear the bear line by the 02-10 → 03-10 move).
        let conn = mem_conn();
        let mut ep_new = old_episode("BRDG", "2026-03-10T12:00:00+00:00");
        ep_new.episode_id = "ep-BRDG-new".into();
        ep_new.intrinsic_vintage = "2026-02-10T12:00:00+00:00".into();
        ep_new.vintage_fresh = false;
        let mut episodes = vec![
            old_episode("BRDG", "2026-02-10T12:00:00+00:00"),
            ep_new,
        ];
        let source = RecordingPrices {
            inner: SyntheticPrices {
                fail_dividends: false,
            },
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let mut ctx = SeriesCtx::new(&conn, Some(&source));
        let today = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        mature_labels(&mut episodes, &mut ctx, today, "2026-05-01");
        // One holding fetch, floored at the earliest active anchor − pad.
        let calls = source.calls.borrow();
        let brdg: Vec<_> = calls.iter().filter(|(s, _)| s == "BRDG").collect();
        assert_eq!(brdg.len(), 1, "one fetch per symbol per pass: {calls:?}");
        assert_eq!(
            brdg[0].1,
            NaiveDate::from_ymd_opt(2026, 2, 3).unwrap(),
            "floored at the earliest active-episode anchor minus the pad"
        );
        drop(calls);
        // Both episodes bridge through the 02-10 session close (i=7 from the
        // 02-03 fetch start → 100 + 4 + 0.7): a vintage-fresh no-op for EP-OLD,
        // the intrinsic-vintage keying for EP-NEW.
        let old_scored = scored_for(&episodes[0], 1).expect("EP-OLD 1-month scored");
        let new_scored = scored_for(&episodes[1], 1).expect("EP-NEW 1-month scored");
        let old_bridge = old_scored.anchor_close.expect("EP-OLD bridge covered");
        let new_bridge = new_scored.anchor_close.expect("EP-NEW bridge covered");
        assert!((old_bridge - 104.7).abs() < 1e-9, "{old_bridge}");
        assert!((new_bridge - 104.7).abs() < 1e-9, "{new_bridge}");
        // EP-NEW's entry still keys on its own anchor (the session after 03-10).
        assert_eq!(new_scored.entry_date, "2026-03-11");
    }

    #[test]
    fn the_fetch_floor_covers_the_intrinsic_session_the_bridge_keys_at() {
        // A lone rule-demotion episode: anchor 03-10, intrinsic vintage 02-10 —
        // the vintage is OLDER than the anchor, which is the shape that exposed
        // the hole. The fetch is floored at `min(anchor, vintage)`, so the session
        // the bridge keys at is inside the refreshed range and the bridge resolves
        // there. Floored at the anchor alone, `merge_price_bars` rewrote only the
        // fetched dates and left the 02-10 bar however the cache last held it —
        // and since `price_bars` is never pruned, a bar cached before a split
        // satisfied the bridge on a stale basis, fabricating a drawdown breach.
        let conn = mem_conn();
        let mut ep = old_episode("LONE", "2026-03-10T12:00:00+00:00");
        ep.intrinsic_vintage = "2026-02-10T12:00:00+00:00".into();
        ep.vintage_fresh = false;
        let mut episodes = vec![ep];
        let source = RecordingPrices {
            inner: SyntheticPrices {
                fail_dividends: false,
            },
            calls: Default::default(),
        };
        let mut ctx = SeriesCtx::new(&conn, Some(&source));
        let today = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        mature_labels(&mut episodes, &mut ctx, today, "2026-05-01");
        let holding_from = source
            .calls
            .borrow()
            .iter()
            .find(|(sym, _)| sym == "LONE")
            .map(|(_, from)| *from)
            .expect("the holding's series was fetched");
        assert!(
            holding_from <= NaiveDate::from_ymd_opt(2026, 2, 10).unwrap(),
            "the fetch must reach the intrinsic session, not stop at the anchor: {holding_from}"
        );
        let scored = scored_for(&episodes[0], 1).expect("1-month scored");
        let anchor_close = scored.anchor_close.expect("the bridge resolves at the vintage");
        // The synthetic series rises with date, so a bridge keyed at the intrinsic
        // session (02-10) must sit strictly below the anchor session's own close —
        // proving it keyed at the vintage rather than silently falling back to the
        // anchor, which is what the old exclusion was protecting against.
        let at_anchor = {
            let series = ctx.series("LONE", holding_from, today).to_vec();
            anchor_session_close(&series, NaiveDate::from_ymd_opt(2026, 3, 10).unwrap())
                .map(|b| b.value)
                .expect("the anchor session is covered")
        };
        assert!(
            anchor_close < at_anchor,
            "the bridge keyed at {anchor_close}, the anchor session closes at {at_anchor} — \
             it must key at the intrinsic vintage, never the anchor"
        );
        assert!(scored.price_return.is_finite());
    }

    #[test]
    fn a_genuinely_unservable_intrinsic_session_still_excludes_the_bridge() {
        // The exclusion arm the fix must preserve: when the SOURCE cannot serve the
        // intrinsic session at all, the bridge is excluded rather than keyed at the
        // anchor instead. Excluded-not-guessed survives the wider floor.
        struct LateStart;
        impl OutcomePriceSource for LateStart {
            fn daily_closes(
                &self,
                symbol: &str,
                from: NaiveDate,
                to: NaiveDate,
            ) -> Result<Vec<DatedValue>> {
                // Nothing before 03-01, whatever the caller asks for.
                let clamped = from.max(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
                SyntheticPrices {
                    fail_dividends: false,
                }
                .daily_closes(symbol, clamped, to)
            }
            fn dividend_history(&self, _: &str, _: NaiveDate, _: NaiveDate) -> Result<Vec<DatedValue>> {
                Ok(Vec::new())
            }
        }
        let conn = mem_conn();
        let mut ep = old_episode("LONE", "2026-03-10T12:00:00+00:00");
        ep.intrinsic_vintage = "2026-02-10T12:00:00+00:00".into();
        ep.vintage_fresh = false;
        let mut episodes = vec![ep];
        let mut ctx = SeriesCtx::new(&conn, Some(&LateStart));
        mature_labels(
            &mut episodes,
            &mut ctx,
            NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            "2026-05-01",
        );
        let scored = scored_for(&episodes[0], 1).expect("1-month scored");
        assert!(
            scored.anchor_close.is_none(),
            "an unservable intrinsic session excludes the bridge: {:?}",
            scored.anchor_close
        );
        assert!(scored.price_return.is_finite(), "the window itself still scores");
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

    /// Serves no closes (the cache is pre-seeded) but one dividend, dated between
    /// the window's last close and its calendar end — finding 9's population.
    struct GapDividend {
        ex_date: &'static str,
    }

    impl OutcomePriceSource for GapDividend {
        fn daily_closes(&self, _: &str, _: NaiveDate, _: NaiveDate) -> Result<Vec<DatedValue>> {
            Ok(Vec::new())
        }
        fn dividend_history(&self, _: &str, _: NaiveDate, _: NaiveDate) -> Result<Vec<DatedValue>> {
            Ok(bars(&[(self.ex_date, 5.0)]))
        }
    }

    #[test]
    fn a_dividend_going_ex_after_the_last_close_is_not_counted_in_total_return() {
        let conn = mem_conn();
        // The 1-month window ends 2026-06-01 (a Monday). The series' last close is
        // 2026-05-28 — inside COVERAGE_TOLERANCE_DAYS, so the window is covered and
        // scores, with the end price taken from the 28th.
        let seeded = bars(&[
            ("2026-05-04", 100.0),
            ("2026-05-18", 105.0),
            ("2026-05-28", 110.0),
        ]);
        // Both benchmark legs too — an unseeded resolvable leg holds the whole
        // window pending inside grace, which would mask the assertion.
        for sym in ["AAPL", MARKET_BENCHMARK, "XLK"] {
            store::merge_price_bars(&conn, sym, &seeded).unwrap();
        }
        // Ex-date 2026-05-29: after the end bar's close, but on or before the
        // calendar `w_end`. The holder has not earned it as of the price the label
        // divides by, so adding it overstates the return — and dividends only add,
        // so the error is always signed positive.
        let source = GapDividend {
            ex_date: "2026-05-29",
        };
        let mut episodes = vec![old_episode("AAPL", "2026-05-01T12:00:00+00:00")];
        let mut ctx = SeriesCtx::new(&conn, Some(&source));
        mature_labels(
            &mut episodes,
            &mut ctx,
            NaiveDate::from_ymd_opt(2026, 6, 3).unwrap(),
            "2026-06-03",
        );
        let scored = scored_for(&episodes[0], 1).expect("the 1-month window scored");
        let price_return = 110.0 / 100.0 - 1.0;
        assert!(
            (scored.price_return - price_return).abs() < 1e-9,
            "price leg unchanged: {} vs {price_return}",
            scored.price_return
        );
        assert!(
            scored
                .total_return
                .is_some_and(|tr| (tr - price_return).abs() < 1e-9),
            "the out-of-window dividend must not ride the total-return leg: {:?}",
            scored.total_return
        );
    }

    #[test]
    fn a_dividend_on_the_end_bars_own_session_still_counts() {
        // The boundary the fix must not over-correct: an ex-date ON the end bar's
        // session IS out of that close, so it belongs in the window. Bounding at
        // the bar's date keeps it (`<=`), exactly as the entry side excludes its
        // own session's ex-date with a strict `>`.
        let conn = mem_conn();
        let seeded = bars(&[
            ("2026-05-04", 100.0),
            ("2026-05-18", 105.0),
            ("2026-05-28", 110.0),
        ]);
        // Both benchmark legs too — an unseeded resolvable leg holds the whole
        // window pending inside grace, which would mask the assertion.
        for sym in ["AAPL", MARKET_BENCHMARK, "XLK"] {
            store::merge_price_bars(&conn, sym, &seeded).unwrap();
        }
        let source = GapDividend {
            ex_date: "2026-05-28",
        };
        let mut episodes = vec![old_episode("AAPL", "2026-05-01T12:00:00+00:00")];
        let mut ctx = SeriesCtx::new(&conn, Some(&source));
        mature_labels(
            &mut episodes,
            &mut ctx,
            NaiveDate::from_ymd_opt(2026, 6, 3).unwrap(),
            "2026-06-03",
        );
        let scored = scored_for(&episodes[0], 1).expect("the 1-month window scored");
        assert!(
            scored
                .total_return
                .is_some_and(|tr| (tr - ((110.0 + 5.0) / 100.0 - 1.0)).abs() < 1e-9),
            "an ex-date on the end session must still count: {:?}",
            scored.total_return
        );
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
    fn a_late_starting_series_never_scores_a_late_entry() {
        // A cached series that starts well after the anchor (a partial refresh, a
        // late series start) reaches the window end but never covered the start:
        // the window must hold pending — then close price-coverage-unscorable —
        // rather than score a months-late bar as the "next session" entry.
        let conn = mem_conn();
        store::merge_price_bars(
            &conn,
            "LATE",
            &bars(&[("2026-07-01", 100.0), ("2026-12-31", 110.0)]),
        )
        .unwrap();
        let mut episodes = vec![old_episode("LATE", "2026-05-01T12:00:00+00:00")];
        let mut ctx = SeriesCtx::new(&conn, None);
        let today = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        let summary = mature_labels(&mut episodes, &mut ctx, today, "2026-06-15");
        assert!(summary.matured.is_empty(), "a late entry never scores");
        assert_eq!(summary.pending_coverage, vec!["LATE".to_string()]);
        // Past the grace it closes as never-covered, not terminal.
        let mut ctx = SeriesCtx::new(&conn, None);
        let today = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
        let summary = mature_labels(&mut episodes, &mut ctx, today, "2026-09-15");
        assert_eq!(summary.matured.len(), 1);
        assert_eq!(summary.matured[0].outcome, "price-coverage-unscorable");
    }

    #[test]
    fn a_benchmark_series_starting_late_never_supplies_a_return() {
        let closes = bars(&[("2026-07-01", 100.0), ("2026-12-31", 110.0)]);
        let anchor = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let w_end = NaiveDate::from_ymd_opt(2026, 12, 20).unwrap();
        assert_eq!(bench_return(&closes, anchor, w_end), None);
        // The same series with a timely start supplies one.
        let timely = bars(&[("2026-05-04", 100.0), ("2026-12-31", 110.0)]);
        assert!(bench_return(&timely, anchor, w_end).is_some());
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
    fn falsifier_lead_times_are_split_safe_across_a_retroactive_adjustment() {
        // Authored at spot 100 with bear target 60 (a −40% line); a 2:1 split
        // then re-bases the label-time series around 50. Price-space comparison
        // would read every close below 60 as an instant false breach; return
        // space reads the −10% dip as no material drawdown.
        let conn = mem_conn();
        store::merge_price_bars(
            &conn,
            "SPLT",
            &bars(&[
                ("2025-06-02", 50.0),
                ("2025-06-03", 50.0),
                ("2025-09-01", 48.0),
                ("2026-01-15", 45.0),
                ("2026-06-05", 52.0),
            ]),
        )
        .unwrap();
        let mut ep = old_episode("SPLT", "2025-06-02T12:00:00+00:00");
        ep.falsifier_events.push(FalsifierEvent {
            condition_id: "c-1".into(),
            confirmed_at: "2025-08-01".into(),
            confirmation_observation_id: "obs-1".into(),
            post_maturity: false,
            lead_time_trading_days: None,
            no_material_drawdown: None,
        });
        let mut episodes = vec![ep];
        let mut ctx = SeriesCtx::new(&conn, None);
        let today = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
        mature_labels(&mut episodes, &mut ctx, today, "2026-09-15");
        let ev = &episodes[0].falsifier_events[0];
        assert_eq!(
            ev.no_material_drawdown,
            Some(true),
            "no false breach across the split"
        );
        assert!(ev.lead_time_trading_days.is_none());

        // A genuine −46% close still breaches, with the confirmation's positive
        // lead over the later breach bar.
        let conn = mem_conn();
        store::merge_price_bars(
            &conn,
            "DEEP",
            &bars(&[
                ("2025-06-02", 50.0),
                ("2025-06-03", 50.0),
                ("2025-09-01", 48.0),
                ("2026-01-15", 27.0),
                ("2026-06-05", 52.0),
            ]),
        )
        .unwrap();
        let mut ep = old_episode("DEEP", "2025-06-02T12:00:00+00:00");
        ep.falsifier_events.push(FalsifierEvent {
            condition_id: "c-1".into(),
            confirmed_at: "2025-08-01".into(),
            confirmation_observation_id: "obs-1".into(),
            post_maturity: false,
            lead_time_trading_days: None,
            no_material_drawdown: None,
        });
        let mut episodes = vec![ep];
        let mut ctx = SeriesCtx::new(&conn, None);
        mature_labels(&mut episodes, &mut ctx, today, "2026-09-15");
        let ev = &episodes[0].falsifier_events[0];
        assert_eq!(ev.no_material_drawdown, Some(false));
        assert_eq!(
            ev.lead_time_trading_days,
            Some(1),
            "confirmed one bar before the breach"
        );
    }

    #[test]
    fn a_stale_pre_anchor_bar_never_serves_as_the_bridge() {
        // A sparse cache holds a years-old bar, a valid next-session entry, and
        // window-end coverage. The labels still score (they need no bridge),
        // but the bridge-dependent reads — band calibration, lead-time
        // stamping — must exclude rather than scale through the stale close.
        let conn = mem_conn();
        store::merge_price_bars(
            &conn,
            "SPRS",
            &bars(&[
                ("2023-01-10", 80.0),
                ("2025-06-03", 50.0),
                ("2025-09-01", 48.0),
                ("2026-01-15", 45.0),
                ("2026-06-05", 52.0),
            ]),
        )
        .unwrap();
        let mut ep = old_episode("SPRS", "2025-06-02T12:00:00+00:00");
        ep.falsifier_events.push(FalsifierEvent {
            condition_id: "c-1".into(),
            confirmed_at: "2025-08-01".into(),
            confirmation_observation_id: "obs-1".into(),
            post_maturity: false,
            lead_time_trading_days: None,
            no_material_drawdown: None,
        });
        let mut episodes = vec![ep];
        let mut ctx = SeriesCtx::new(&conn, None);
        let today = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
        let summary = mature_labels(&mut episodes, &mut ctx, today, "2026-09-15");
        assert!(
            summary.matured.iter().all(|m| m.outcome == "scored"),
            "labels score without the bridge"
        );
        let scored = scored_for(&episodes[0], 12).expect("scored");
        assert_eq!(
            scored.anchor_close, None,
            "a 2023 bar is no decision-instant bridge"
        );
        let ev = &episodes[0].falsifier_events[0];
        assert!(
            ev.lead_time_trading_days.is_none() && ev.no_material_drawdown.is_none(),
            "lead time stays unstamped, excluded from the read"
        );
        let reads = derive_reads(&episodes);
        assert!(
            reads.target_calibration.iter().all(|t| t.scored == 0),
            "no band scores through a stale bridge"
        );
    }

    #[test]
    fn lead_time_breaches_key_on_the_anchor_close_not_the_entry_gap() {
        // Line = anchor_close × bear ⁄ spot = 50 × 60 ⁄ 100 = 30, regardless of
        // the overnight gap into the entry. A −20% gap-down entry (40) would
        // make the entry-anchored rule miss the genuine 28-close breach; a
        // gap-up entry (60) would make it fabricate one from the 32 close.
        let event = || FalsifierEvent {
            condition_id: "c-1".into(),
            confirmed_at: "2025-08-01".into(),
            confirmation_observation_id: "obs-1".into(),
            post_maturity: false,
            lead_time_trading_days: None,
            no_material_drawdown: None,
        };
        let today = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();

        let conn = mem_conn();
        store::merge_price_bars(
            &conn,
            "GAPD",
            &bars(&[
                ("2025-06-02", 50.0),
                ("2025-06-03", 40.0),
                ("2025-09-01", 38.0),
                ("2026-01-15", 28.0),
                ("2026-06-05", 41.0),
            ]),
        )
        .unwrap();
        let mut ep = old_episode("GAPD", "2025-06-02T12:00:00+00:00");
        ep.falsifier_events.push(event());
        let mut episodes = vec![ep];
        let mut ctx = SeriesCtx::new(&conn, None);
        mature_labels(&mut episodes, &mut ctx, today, "2026-09-15");
        let ev = &episodes[0].falsifier_events[0];
        assert_eq!(ev.no_material_drawdown, Some(false), "the 28 close breaches the 30 line");
        assert_eq!(ev.lead_time_trading_days, Some(1));

        let conn = mem_conn();
        store::merge_price_bars(
            &conn,
            "GAPU",
            &bars(&[
                ("2025-06-02", 50.0),
                ("2025-06-03", 60.0),
                ("2025-09-01", 58.0),
                ("2026-01-15", 32.0),
                ("2026-06-05", 55.0),
            ]),
        )
        .unwrap();
        let mut ep = old_episode("GAPU", "2025-06-02T12:00:00+00:00");
        ep.falsifier_events.push(event());
        let mut episodes = vec![ep];
        let mut ctx = SeriesCtx::new(&conn, None);
        mature_labels(&mut episodes, &mut ctx, today, "2026-09-15");
        let ev = &episodes[0].falsifier_events[0];
        assert_eq!(
            ev.no_material_drawdown,
            Some(true),
            "32 sits above the 30 line — no fabricated breach off the gap-up entry"
        );
        assert!(ev.lead_time_trading_days.is_none());
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
        // The model arm scored over the SAME population with its own frozen bands
        // — a fair head-to-head, and a different result (the fixture's model band
        // differs from the engine's).
        let model_cal = reads
            .model_target_calibration
            .iter()
            .find(|t| t.window_months == 12)
            .unwrap();
        assert_eq!(model_cal.scored, 2, "same exclusion rules as the engine read");
        assert!(model_cal.mean_interval_score.is_some());
        assert_ne!(
            model_cal.mean_interval_score, cal.mean_interval_score,
            "distinct bands must score distinctly"
        );
        // The paired head-to-head runs over the intersection — here both fixture
        // episodes carry both bands, so the pair scores 2 with distinct means.
        let paired = reads
            .head_to_head
            .iter()
            .find(|h| h.window_months == 12)
            .unwrap();
        assert_eq!(paired.scored, 2, "paired population = episodes with BOTH bands");
        assert_ne!(
            paired.engine_mean_interval_score, paired.model_mean_interval_score,
            "the pair scores both arms on the same events"
        );
        // Direction reads: both arms present at all three mapped windows; the
        // fixture's model is bullish everywhere and the engine bearish/neutral,
        // so on the synthetic series exactly one directional arm can be hitting.
        assert_eq!(reads.outlook_direction.len(), 6);
        let read = |arm: &str, months: u32| {
            reads
                .outlook_direction
                .iter()
                .find(|r| r.arm == arm && r.window_months == months)
                .unwrap()
        };
        assert_eq!(read("model", 12).scored, 2);
        assert_eq!(read("engine", 6).neutral, 2, "neutral counts beside the hit-rate");
        assert_eq!(
            read("model", 12).hits + read("engine", 12).hits,
            2,
            "opposite directional calls: exactly one arm hits per episode"
        );
        assert!(!reads.eligibility.eligible, "2 of 30: below the bar");
        assert!(reads.eligibility.note.contains("below the proposal eligibility bar"));
    }

    #[test]
    fn matured_learning_text_renders_the_paired_head_to_head_only() {
        // The comparison line comes from the PAIRED read alone (same episodes,
        // both arms) and renders only where the pair scored — never from the two
        // arms' independently-pooled per-arm reads (Codex round 1, finding 3).
        let records = OutcomeRecords {
            opened: vec![],
            extended: vec![],
            alignment_tags: vec![],
            matured: vec![MaturedNote {
                symbol: "AAPL".into(),
                episode_id: "ep-1".into(),
                window_months: 12,
                outcome: "scored".into(),
                total_return: Some(0.10),
                price_return: Some(0.08),
            }],
            pending_coverage: vec![],
            reads: DerivedReads {
                cohorts: vec![],
                target_calibration: vec![],
                model_target_calibration: vec![],
                head_to_head: vec![
                    // An unscored 1-mo pair renders no line.
                    HeadToHeadRead {
                        window_months: 1,
                        scored: 0,
                        engine_mean_interval_score: None,
                        model_mean_interval_score: None,
                        engine_coverage_rate: None,
                        model_coverage_rate: None,
                    },
                    HeadToHeadRead {
                        window_months: 12,
                        scored: 4,
                        engine_mean_interval_score: Some(0.50),
                        model_mean_interval_score: Some(0.30),
                        engine_coverage_rate: Some(0.75),
                        model_coverage_rate: Some(1.0),
                    },
                ],
                outlook_direction: vec![],
                falsifier_lead_times: vec![],
                self_correction: SelfCorrectionRead {
                    total: 0,
                    per_holding: vec![],
                },
                eligibility: EligibilityRecord {
                    unique_matured_holdings: 1,
                    bar: PROPOSAL_ELIGIBILITY_BAR,
                    eligible: false,
                    note: "below the proposal eligibility bar".into(),
                },
            },
        };
        let text = matured_learning_text(&records, "2026-08-05").expect("matured → text");
        assert!(
            text.contains(
                "model-vs-engine 12-month interval score (paired, 4 bands): model 0.3000 \
                 vs engine 0.5000 — lower is better"
            ),
            "{text}"
        );
        assert!(!text.contains("1-month interval score"), "{text}");
    }

    fn scored_label(price_return: f64, total_return: Option<f64>) -> ScoredLabel {
        ScoredLabel {
            entry_date: "2026-08-05".into(),
            entry_price: 50.0,
            end_date: "2027-08-04".into(),
            end_price: 50.0 * (1.0 + price_return),
            anchor_close: Some(50.0),
            price_return,
            total_return,
            total_return_gap: total_return
                .is_none()
                .then(|| "total-return leg unavailable — price-only label".into()),
            max_drawdown: -0.1,
            vs_market: None,
            market_leg_gap: None,
            vs_sector: None,
            sector_leg_gap: None,
            labeled_at: "2027-08-04".into(),
        }
    }

    fn set_scored(ep: &mut DecisionEpisode, months: u32, label: ScoredLabel) {
        let slot = ep
            .labels
            .iter_mut()
            .find(|l| l.window_months == months)
            .unwrap();
        slot.outcome = LabelOutcome::Scored(Box::new(label));
    }

    #[test]
    fn a_missing_total_return_leg_quotes_price_only_in_the_primary_mean() {
        // The labeled-mix rule: a label whose TR leg failed contributes its
        // price-only return to the primary mean — the population never silently
        // shrinks below the reported holding count.
        let anchor = "2026-08-04T12:00:00+00:00";
        let mut with_tr = old_episode("AAPL", anchor);
        set_scored(&mut with_tr, 12, scored_label(0.05, Some(0.10)));
        let mut without_tr = old_episode("MSFT", anchor);
        set_scored(&mut without_tr, 12, scored_label(0.20, None));
        let reads = derive_reads(&[with_tr, without_tr]);
        let twelve = reads.cohorts.iter().find(|c| c.window_months == 12).unwrap();
        let hold = twelve.lean_cohorts.iter().find(|c| c.key == "hold").unwrap();
        assert_eq!(hold.unique_holdings, 2);
        assert!((hold.mean_total_return.unwrap() - 0.15).abs() < 1e-12);
        assert!((hold.mean_price_return.unwrap() - 0.125).abs() < 1e-12);
    }

    #[test]
    fn target_calibration_is_split_safe_across_a_retroactive_adjustment() {
        // Authored at spot 100 (band 60–160); a 2:1 split then re-bases the whole
        // label-time series, so the window scores entry 50 → end 55 (+10%).
        // Price-space comparison would read 55 against the 60–160 band — a false
        // miss; return space over the authoring spot reads +10% inside
        // [−40%, +60%].
        let mut ep = old_episode("SPLT", "2026-08-04T12:00:00+00:00");
        set_scored(&mut ep, 12, scored_label(0.10, None));
        let reads = derive_reads(&[ep]);
        let cal = reads
            .target_calibration
            .iter()
            .find(|t| t.window_months == 12)
            .unwrap();
        assert_eq!(cal.scored, 1);
        assert_eq!(cal.coverage_rate, Some(1.0), "no false miss across the split");
        // Base error reconstructs the realized price in the authoring basis:
        // 100 × 1.10 = 110 vs base 120.
        assert!((cal.mean_base_signed_error.unwrap() - (110.0 - 120.0) / 120.0).abs() < 1e-12);
    }

    #[test]
    fn target_calibration_keys_on_the_anchor_close_not_the_entry_gap() {
        // Anchor-session close 50, a gap-down entry (45), window end 29. From
        // the decision instant the realized move is −42% — outside the authored
        // −40% bear edge (60 at spot 100) — while the entry-anchored read
        // (−35.6%) would have called it inside the band.
        let mut ep = old_episode("GAP", "2026-08-04T12:00:00+00:00");
        let mut label = scored_label(29.0 / 45.0 - 1.0, None);
        label.entry_price = 45.0;
        label.end_price = 29.0;
        label.anchor_close = Some(50.0);
        set_scored(&mut ep, 12, label);
        let reads = derive_reads(&[ep]);
        let cal = reads
            .target_calibration
            .iter()
            .find(|t| t.window_months == 12)
            .unwrap();
        assert_eq!(cal.scored, 1);
        assert_eq!(
            cal.coverage_rate,
            Some(0.0),
            "outside the band from the decision instant"
        );
        // Base error through the bridge: realized in the authoring basis is
        // 100 × 29 ⁄ 50 = 58 vs base 120.
        assert!((cal.mean_base_signed_error.unwrap() - (58.0 - 120.0) / 120.0).abs() < 1e-12);
    }

    #[test]
    fn target_calibration_never_mixes_parameter_versions() {
        let anchor = "2026-08-04T12:00:00+00:00";
        let mut v3 = old_episode("AAPL", anchor);
        set_scored(&mut v3, 12, scored_label(0.10, None));
        let mut v2 = old_episode("MSFT", anchor);
        if let EpisodeBody::Priced(p) = &mut v2.body {
            p.snapshot.target_parameter_version = Some("targets-v2".into());
        }
        set_scored(&mut v2, 12, scored_label(0.10, None));
        let reads = derive_reads(&[v3, v2]);
        let twelve: Vec<_> = reads
            .target_calibration
            .iter()
            .filter(|t| t.window_months == 12)
            .collect();
        assert_eq!(twelve.len(), 2, "one read per parameter version");
        assert!(twelve.iter().all(|t| t.scored == 1));
        let versions: Vec<_> = twelve
            .iter()
            .map(|t| t.parameter_version.as_deref())
            .collect();
        assert!(versions.contains(&Some("targets-v2")));
        assert!(versions.contains(&Some("targets-v3")));
    }
}
