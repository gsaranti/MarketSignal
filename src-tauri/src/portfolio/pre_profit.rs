//! The **pre-profit execution / financing overlay** (`docs/portfolio-analysis.md`
//! §Starting parameters; `docs/portfolio-workflow.md` §Step 6b, §Step 6e). A priced
//! stock that is not yet operating-profitable — or has no positive forward-EPS
//! consensus while burning cash — carries a deterministic execution / financing
//! read: statement-derived runway, margin progression, capital intensity, and
//! dilution first, then app-validated operating observations research adds.
//!
//! The overlay is **conviction / risk / action context only** — never another grade
//! component, and never a license for the model to calculate a number: the engine
//! computes attainment, states, and rule consequences; the rule consequences bind
//! the engine arm (its stand-ins observe the matched ceiling), the model
//! interpreting the evidence unrestricted, departures annotated.
//!
//! **Producer status (as-built): active** — the research-loop slice connected the
//! producer after discharging both recorded obligations: the holding-identity
//! cross-check and source-text corroboration run per row over the loop's
//! fetched-page lineage ([`validate_against_source`]), and reported periods
//! normalize to one ISO-period-end convention before the dedup key is taken
//! ([`normalize_period`]). Distillation emits typed observation rows for an
//! overlay-eligible stock; an unevidenced call still rejects every candidate.

use serde::{Deserialize, Serialize};

use crate::portfolio::engine::CompanyFinancials;
use crate::portfolio::Conviction;

// ---- Calibration surface (NOT pinned — shadow-tune against live runs;
//      `docs/portfolio-analysis.md` §Starting parameters, all drafted) ----------

/// Financing-state runway bands, in months: `adequate` at or above 24, `watch` at
/// 12–<24, `constrained` below 12 (`not_burning` when TTM burn is zero,
/// `unscorable` when a required input is absent).
const RUNWAY_ADEQUATE_MONTHS: f64 = 24.0;
const RUNWAY_WATCH_MONTHS: f64 = 12.0;

/// A comparable actual is an execution miss only at or beyond this shortfall of the
/// guidance lower bound — `(bound − actual) ÷ bound ≥ 0.05`; smaller is in-line
/// noise, not a miss.
const EXECUTION_MISS_RATIO: f64 = 0.05;
/// A material single miss: the latest comparable actual at least this far below.
const MATERIAL_MISS_RATIO: f64 = 0.20;

/// Repeated miss looks at each metric identity's latest four comparable periods…
const MISS_WINDOW_PERIODS: usize = 4;
/// The backfill obligation's floor: a previously used guidance metric with fewer
/// comparable stored periods than this binds a bounded backfill — the miss
/// window's own depth, since the backfill exists to fill it
/// (`docs/portfolio-analysis.md` §Starting parameters).
const BACKFILL_MIN_COMPARABLE_PERIODS: usize = MISS_WINDOW_PERIODS;
/// …and needs misses in at least this many distinct periods for that same metric.
const REPEATED_MISS_PERIODS: usize = 2;

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
pub const PRE_PROFIT_PARAMETER_VERSION: &str = "pre-profit-v2";

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

// ---- Typed research observations ------------------------------------------------

/// The operating-proof metric families research may observe
/// (`docs/portfolio-analysis.md` §Starting parameters — production, deliveries,
/// bookings / backlog / reservations, and unit economics; guidance is a **role**,
/// not a kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricKind {
    Production,
    Deliveries,
    Bookings,
    Backlog,
    Reservations,
    UnitEconomics,
}

impl MetricKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricKind::Production => "production",
            MetricKind::Deliveries => "deliveries",
            MetricKind::Bookings => "bookings",
            MetricKind::Backlog => "backlog",
            MetricKind::Reservations => "reservations",
            MetricKind::UnitEconomics => "unit-economics",
        }
    }
}

/// What one observation *is* — an actual, a guidance bound, or sourced context —
/// so the engine pairs comparable facts without asking the model to calculate
/// attainment (`docs/portfolio-workflow.md` §Step 6d).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservationRole {
    Actual,
    GuidanceLow,
    GuidanceHigh,
    PointGuidance,
    ContextualLevel,
}

/// The typed direction the app validates against the metric kind; only a
/// higher-is-better observation enters the currently defined guidance-miss rule —
/// other polarities remain sourced context (`docs/portfolio-analysis.md` §Starting
/// parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservationPolarity {
    HigherIsBetter,
    LowerIsBetter,
    TargetBand,
}

/// One app-validated operating observation — the research leg's only entry into the
/// overlay. The model may extract a row only from source text that states the
/// value; validation and computation own every comparison and state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreProfitObservation {
    pub metric_kind: MetricKind,
    pub observation_role: ObservationRole,
    pub polarity: ObservationPolarity,
    pub numeric_value: f64,
    pub units: String,
    /// The reported period the value belongs to. Periods compare **exactly** after
    /// trimming, and order lexicographically — [`normalize_period`] maps every
    /// producer row to one convention per issuer (ISO period end preferred)
    /// before validation takes the dedup key.
    pub period: String,
    pub issuer_scope: String,
    pub source_url: String,
    pub published_at: String,
    /// Extraction confidence, 0–1.
    pub confidence: f64,
}

impl PreProfitObservation {
    /// The normalized metric identity misses group under: kind + units + issuer
    /// scope (`docs/portfolio-analysis.md` §Starting parameters — "the same
    /// normalized metric identity, issuer scope / perimeter, and units").
    pub(crate) fn identity(&self) -> (String, String, String) {
        (
            self.metric_kind.as_str().to_string(),
            self.units.trim().to_ascii_lowercase(),
            self.issuer_scope.trim().to_ascii_lowercase(),
        )
    }

    /// The dedup key (`docs/storage.md` — "deduplicated by issuer + normalized
    /// metric identity + role + period + source observation").
    fn dedup_key(&self) -> (String, String, String, ObservationRole, String, String) {
        let (kind, units, scope) = self.identity();
        (
            kind,
            units,
            scope,
            self.observation_role,
            self.period.trim().to_string(),
            self.source_url.trim().to_string(),
        )
    }
}

/// Whether the research agenda's **backfill obligation** binds
/// (`docs/portfolio-analysis.md` §Starting parameters): the holding's first
/// overlay-eligible full pass, or a previously used guidance metric with fewer
/// than `BACKFILL_MIN_COMPARABLE_PERIODS` **comparable** stored periods for
/// its normalized identity. A stored period is comparable when it holds both
/// a guidance bound (a range low or point guidance) and an actual — the
/// pairing the execution read attains against, before that rule's polarity
/// and finite-bound guards, which bound which pairs can *miss*, not whether
/// history exists. A guidance row of any role marks the metric as previously
/// used. Read at agenda-build time over the 6b overlay's carried observation
/// history.
pub fn backfill_required(current: &PreProfitOverlay, prior: Option<&PreProfitOverlay>) -> bool {
    let first_eligible_pass = !prior.is_some_and(PreProfitOverlay::is_eligible);
    if first_eligible_pass {
        return true;
    }
    use std::collections::{HashMap, HashSet};
    type Identity = (String, String, String);
    let mut guided: HashSet<Identity> = HashSet::new();
    let mut bounds: HashMap<Identity, HashSet<&str>> = HashMap::new();
    let mut actuals: HashMap<Identity, HashSet<&str>> = HashMap::new();
    for o in &current.observations {
        let identity = o.identity();
        let period = o.period.trim();
        match o.observation_role {
            ObservationRole::GuidanceLow | ObservationRole::PointGuidance => {
                bounds.entry(identity.clone()).or_default().insert(period);
                guided.insert(identity);
            }
            ObservationRole::GuidanceHigh => {
                guided.insert(identity);
            }
            ObservationRole::Actual => {
                actuals.entry(identity).or_default().insert(period);
            }
            ObservationRole::ContextualLevel => {}
        }
    }
    guided.iter().any(|identity| {
        let comparable = match (bounds.get(identity), actuals.get(identity)) {
            (Some(bound), Some(actual)) => bound.intersection(actual).count(),
            _ => 0,
        };
        comparable < BACKFILL_MIN_COMPARABLE_PERIODS
    })
}

/// A rejected candidate row with its validation reason — persisted so the audit
/// shows what research offered and why it did not enter the history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RejectedObservation {
    pub observation: PreProfitObservation,
    pub reason: String,
}

/// A required cold-start / gap-fill backfill attempt's record — an obligation to
/// search, not to produce a row (`docs/portfolio-analysis.md` §Starting
/// parameters). The research loop's agenda requires one on the first
/// overlay-eligible full pass or a thin stored series; an attempt that never
/// reported stays a recorded gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackfillAttempt {
    pub metric_kind: MetricKind,
    pub units: String,
    pub issuer_scope: String,
    pub checked_periods: Vec<String>,
    pub sources: Vec<String>,
    pub coverage: BackfillCoverage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackfillCoverage {
    Complete,
    Partial,
    Unscorable,
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

/// One execution miss — a comparable actual at least 5% below its guidance lower
/// bound, keyed by metric identity and period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionMiss {
    pub metric_kind: MetricKind,
    pub units: String,
    pub issuer_scope: String,
    pub period: String,
    /// `(bound − actual) ÷ bound`.
    pub miss_ratio: f64,
}

/// The engine's guidance-attainment read over the validated observation history.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExecutionRead {
    /// Periods (across identities) where an actual and a finite positive
    /// higher-is-better guidance bound were comparable.
    pub comparable_periods: usize,
    pub misses: Vec<ExecutionMiss>,
    /// Misses in ≥ 2 distinct periods for one metric identity among its latest four
    /// comparable periods — metrics never combine, and two missed metrics in one
    /// period never count as two periods.
    pub repeated_miss: bool,
    /// The latest comparable actual for some identity at least 20% below its bound.
    pub material_single_miss: bool,
}

/// Which conviction ceiling an overlay rule matched — the strictest binds the
/// engine arm's stand-in as a plain min (the raise machinery is retired with
/// `portfolio-v7` — `docs/portfolio-workflow.md` §Step 6g).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConvictionCeiling {
    Medium,
    Low,
}

/// The overlay's deterministic rule consequences — separately attributed from the
/// forensic rules (`docs/portfolio-workflow.md` §Step 6g): repeated execution miss
/// → Medium ceiling; constrained runway → add-family bar; severe deterioration →
/// Low ceiling + add-family bar + an exit-family-only action rule. Since
/// `portfolio-v7` all of these bind the **engine arm** (its stand-in conviction
/// and action observe them); the model's conviction and lean are unrestricted,
/// with departures recorded as annotations.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverlayConsequences {
    pub conviction_ceiling: Option<ConvictionCeiling>,
    pub bar_add_family: bool,
    /// Severe deterioration's exit-family-only rule ({trim, sell all}) — since
    /// `portfolio-v7` it binds the engine arm's lean/action stand-ins and renders
    /// as an engine rule; the model's lean is unrestricted, departures annotated.
    pub exit_family_only: bool,
    /// The engine-matched rules, recorded so a clamped value is reconstructable
    /// (the audit's matched-cap-rule leg).
    pub matched_rules: Vec<String>,
}

/// The complete persisted overlay record — computed for **every priced stock** (the
/// eligibility result persists even when the stock does not enter), carried on the
/// holding's audit row so the period-keyed observation history survives run
/// retention and the selective-carry path (`docs/storage.md §Local Analysis Suite
/// Storage`). States and consequences are meaningful only under
/// `eligibility == Eligible`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreProfitOverlay {
    pub eligibility: PreProfitEligibility,
    pub statement_inputs: StatementInputs,
    pub financing_state: FinancingState,
    pub execution: ExecutionRead,
    /// `None` = the margin legs were unscorable.
    pub economics_deterioration: Option<bool>,
    /// `None` = the dilution leg was unscorable.
    pub material_dilution: Option<bool>,
    /// ≥ 2 independent legs among {repeated-or-material miss, constrained runway,
    /// economics deterioration, material dilution}, at least one an execution-miss
    /// or economics leg — financing plus dilution alone cannot manufacture it.
    pub severe_deterioration: bool,
    /// The period-keyed validated observation history (merged, deduplicated).
    pub observations: Vec<PreProfitObservation>,
    pub rejected: Vec<RejectedObservation>,
    pub backfill_attempts: Vec<BackfillAttempt>,
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

/// Compute the overlay for a priced stock: the statement leg, eligibility, the
/// observation validation / merge, and — when eligible — the derived states and
/// rule consequences. This unevidenced form rejects every candidate row (the
/// Step-6b pass runs it over the carried history alone; live research rows
/// enter through [`compute_overlay_with_sources`]). `prior` carries the
/// previous run's overlay so the period-keyed observation history accumulates
/// across runs (`docs/storage.md` — "continuity evidence for the holding").
pub fn compute_overlay(
    fin: &CompanyFinancials,
    prior: Option<&PreProfitOverlay>,
    candidates: Vec<PreProfitObservation>,
) -> PreProfitOverlay {
    compute_overlay_with_sources(fin, prior, candidates, None)
}

/// The evidenced form — the research loop's producer path: candidate rows are
/// validated with the two activation legs against the loop's fetched pages
/// (`docs/portfolio-workflow.md` §Step 6e). The unevidenced [`compute_overlay`]
/// rejects every candidate, so rows enter the history only through this seam.
pub fn compute_overlay_with_sources(
    fin: &CompanyFinancials,
    prior: Option<&PreProfitOverlay>,
    candidates: Vec<PreProfitObservation>,
    evidence: Option<&SourceEvidence<'_>>,
) -> PreProfitOverlay {
    let mut gaps: Vec<String> = Vec::new();
    let inputs = statement_inputs(fin, &mut gaps);
    let eligibility = eligibility(&inputs);

    // History accumulates regardless of this run's eligibility: a name that turned
    // profitable keeps its history for a later re-entry, and validation is
    // producer-independent.
    let prior_history: Vec<PreProfitObservation> = prior
        .map(|p| p.observations.clone())
        .unwrap_or_default();
    let (accepted, rejected) = validate_observations(candidates, &prior_history, evidence);
    let observations = merge_observations(prior_history, accepted);
    let backfill_attempts = prior
        .map(|p| p.backfill_attempts.clone())
        .unwrap_or_default();

    let eligible = matches!(eligibility, PreProfitEligibility::Eligible { .. });
    let (financing_state, execution, economics_deterioration, material_dilution) = if eligible {
        (
            financing_state(&inputs),
            execution_read(&observations),
            economics_deterioration(&inputs),
            material_dilution(&inputs),
        )
    } else {
        (FinancingState::Unscorable, ExecutionRead::default(), None, None)
    };

    let severe = eligible
        && severe_deterioration(
            &execution,
            financing_state,
            economics_deterioration,
            material_dilution,
        );
    let consequences = if eligible {
        derive_consequences(&execution, financing_state, severe)
    } else {
        OverlayConsequences::default()
    };

    PreProfitOverlay {
        eligibility,
        statement_inputs: inputs,
        financing_state,
        execution,
        economics_deterioration,
        material_dilution,
        severe_deterioration: severe,
        observations,
        rejected,
        backfill_attempts,
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

/// The research loop's source evidence for the two activation legs
/// (`docs/portfolio-workflow.md` §Step 6e): the loop's fetched page texts
/// (keyed by normalized URL), the holding's symbol, and its issuer name.
pub struct SourceEvidence<'a> {
    pub texts: &'a std::collections::HashMap<String, String>,
    pub symbol: &'a str,
    pub company_name: Option<&'a str>,
}

/// Normalize a reported period to one convention per issuer — ISO period end
/// preferred (the `period` field's hard rule): `YYYY-MM-DD` stands, `YYYY-MM`
/// takes its month end, `Qn YYYY` / `YYYY Qn` / `YYYY-Qn` the calendar
/// quarter end, `H1/H2 YYYY` the half end, and `FYYYYY` / bare `YYYY` the
/// year end. Anything else trims and stands (comparable only exactly). The
/// calendar-quarter mapping is a drafted convention — consistent per issuer,
/// which is what comparability needs.
pub fn normalize_period(period: &str) -> String {
    let p = period.trim();
    let up = p.to_ascii_uppercase();
    // YYYY-MM-DD stands.
    if p.len() == 10 && p.as_bytes()[4] == b'-' && p.as_bytes()[7] == b'-' {
        return p.to_string();
    }
    // YYYY-MM → month end.
    if p.len() == 7 && p.as_bytes()[4] == b'-' {
        if let (Ok(y), Ok(m)) = (p[..4].parse::<i32>(), p[5..7].parse::<u32>()) {
            if let Some(d) = month_end(y, m) {
                return d;
            }
        }
    }
    // Quarter / half forms in either order.
    let tokens: Vec<&str> = up
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let year = tokens.iter().find_map(|t| {
        if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
            t.parse::<i32>().ok()
        } else if t.len() == 6 && t.starts_with("FY") && t[2..].chars().all(|c| c.is_ascii_digit())
        {
            t[2..].parse::<i32>().ok()
        } else {
            None
        }
    });
    let quarter = tokens.iter().find_map(|t| match *t {
        "Q1" => Some(1u32),
        "Q2" => Some(2),
        "Q3" => Some(3),
        "Q4" => Some(4),
        _ => None,
    });
    let half = tokens.iter().find_map(|t| match *t {
        "H1" => Some(1u32),
        "H2" => Some(2),
        _ => None,
    });
    if let (Some(y), Some(q)) = (year, quarter) {
        return month_end(y, q * 3).unwrap_or_else(|| p.to_string());
    }
    if let (Some(y), Some(h)) = (year, half) {
        return month_end(y, h * 6).unwrap_or_else(|| p.to_string());
    }
    // FY2026 / bare 2026 → year end.
    if let Some(y) = year {
        if tokens.len() == 1 || (tokens.len() == 2 && tokens.contains(&"FY")) {
            return format!("{y}-12-31");
        }
    }
    p.to_string()
}

fn month_end(year: i32, month: u32) -> Option<String> {
    let first = chrono::NaiveDate::from_ymd_opt(year, month, 1)?;
    let next = if month == 12 {
        chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        chrono::NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    (first < next)
        .then_some((next - chrono::Duration::days(1)).format("%Y-%m-%d").to_string())
}

/// The source-text corroboration test: the observation's numeric value must
/// appear in the fetched page's text (commas stripped; an integer value also
/// matches its decimal render). Deterministic and deliberately literal — the
/// model may extract a row only from source text that states the value.
pub(crate) fn value_in_text(value: f64, text: &str) -> bool {
    let haystack: String = text.replace(',', "");
    // An integer value matches either render ("41" or "41.0") — the boundary
    // rules below would otherwise reject "41" against a printed "41.0".
    let needles: Vec<String> = if value.fract() == 0.0 && value.abs() < 1e15 {
        vec![format!("{}", value as i64), format!("{}.0", value as i64)]
    } else {
        let s = format!("{value}");
        vec![s.trim_end_matches('0').trim_end_matches('.').to_string()]
    };
    // Number-boundary containment: "41" must not corroborate off "141", "412",
    // "41.5", or "3.41" — a neighbor may be neither a digit nor a decimal
    // point that continues a number.
    let bytes = haystack.as_bytes();
    let continues_left = |i: usize| {
        i > 0
            && (bytes[i - 1].is_ascii_digit()
                || (bytes[i - 1] == b'.' && i >= 2 && bytes[i - 2].is_ascii_digit()))
    };
    let continues_right = |i: usize| {
        i < bytes.len()
            && (bytes[i].is_ascii_digit()
                || (bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()))
    };
    for needle in needles {
        if needle.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(pos) = haystack[from..].find(&needle) {
            let start = from + pos;
            let end = start + needle.len();
            if !continues_left(start) && !continues_right(end) {
                return true;
            }
            from = start + 1;
        }
    }
    false
}

/// The holding-identity cross-check: the fetched page must mention the
/// holding — its symbol as a standalone word, or a **distinctive** issuer-name
/// token (generic corporate suffixes like "Holdings" never qualify) — so a
/// cross-issuer citation cannot enter the observation history. The matcher is
/// the shared [`crate::portfolio::text_names_holding`].
fn page_mentions_holding(text: &str, symbol: &str, company_name: Option<&str>) -> bool {
    crate::portfolio::text_names_holding(text, symbol, company_name)
}

/// The two activation legs over one row (`docs/portfolio-workflow.md` §Step
/// 6e — the research-loop slice's recorded obligation, discharged here): the
/// row's source page must have been fetched by THIS holding's loop, must
/// mention the holding (identity cross-check), and must state the value
/// (source-text corroboration).
fn validate_against_source(
    o: &PreProfitObservation,
    evidence: &SourceEvidence<'_>,
) -> Result<(), String> {
    let normalized = crate::web_research::store::normalize_url(&o.source_url);
    let text = evidence
        .texts
        .get(&normalized)
        .or_else(|| evidence.texts.get(o.source_url.trim()))
        .ok_or_else(|| {
            "holding-identity cross-check failed: the source URL was not fetched by this \
             holding's research loop"
                .to_string()
        })?;
    if !page_mentions_holding(text, evidence.symbol, evidence.company_name) {
        return Err(
            "holding-identity cross-check failed: the fetched page never mentions the holding"
                .to_string(),
        );
    }
    if !value_in_text(o.numeric_value, text) {
        return Err(
            "source-text corroboration failed: the stated value does not appear in the fetched \
             page"
                .to_string(),
        );
    }
    Ok(())
}

/// Validate candidate rows against the typed contract, rejecting with a reason;
/// duplicates of stored history (or of an earlier candidate in the same batch) are
/// rejected rather than silently dropped. Periods normalize to the one-per-issuer
/// convention before the dedup key is taken. With `evidence` present the two
/// activation legs run per row; **without it every candidate is rejected** —
/// the producer's rows can only enter through the research loop's lineage.
pub fn validate_observations(
    candidates: Vec<PreProfitObservation>,
    history: &[PreProfitObservation],
    evidence: Option<&SourceEvidence<'_>>,
) -> (Vec<PreProfitObservation>, Vec<RejectedObservation>) {
    let mut seen: std::collections::BTreeSet<_> =
        history.iter().map(|o| o.dedup_key()).collect();
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for mut candidate in candidates {
        candidate.period = normalize_period(&candidate.period);
        if let Err(reason) = validate_observation(&candidate) {
            rejected.push(RejectedObservation { observation: candidate, reason });
            continue;
        }
        match evidence {
            None => {
                rejected.push(RejectedObservation {
                    observation: candidate,
                    reason: "no source evidence supplied — observation rows enter only through \
                             the research loop's fetched-page lineage"
                        .to_string(),
                });
                continue;
            }
            Some(evidence) => {
                if let Err(reason) = validate_against_source(&candidate, evidence) {
                    rejected.push(RejectedObservation { observation: candidate, reason });
                    continue;
                }
            }
        }
        let key = candidate.dedup_key();
        if seen.contains(&key) {
            rejected.push(RejectedObservation {
                observation: candidate,
                reason: "duplicate of a stored observation (issuer + metric identity + role + \
                         period + source)"
                    .to_string(),
            });
            continue;
        }
        seen.insert(key);
        accepted.push(candidate);
    }
    (accepted, rejected)
}

/// One row's **structural** validation — the legs checkable from the typed row
/// alone: metric kind, polarity, numeric value, units, period, issuer scope,
/// source URL, publication date, confidence.
///
/// The two once-promised activation legs — the **holding-identity cross-check**
/// and **source-text corroboration** — are live in
/// [`validate_against_source`], run by [`validate_observations`] over the
/// research loop's fetched-page lineage (the research-loop slice discharged
/// the recorded obligation when it connected the producer); an unevidenced
/// call rejects every candidate, so this structural pass alone can never
/// admit a row.
fn validate_observation(o: &PreProfitObservation) -> Result<(), String> {
    if !o.numeric_value.is_finite() {
        return Err("non-finite numeric value".to_string());
    }
    if o.units.trim().is_empty() {
        return Err("missing units".to_string());
    }
    // Validation sees the row AFTER `normalize_period` ran on it — the
    // documented one-ISO-period-end-per-issuer convention has teeth only if a
    // period that did not normalize rejects, else two rows sharing a fabricated
    // prose period could pair into an execution miss.
    if chrono::NaiveDate::parse_from_str(o.period.trim(), "%Y-%m-%d").is_err() {
        return Err(format!(
            "period {:?} did not normalize to an ISO period-end",
            o.period
        ));
    }
    if o.issuer_scope.trim().is_empty() {
        return Err("missing issuer scope".to_string());
    }
    if !o.source_url.trim().starts_with("http") {
        return Err("missing or non-URL source".to_string());
    }
    // An ISO date (or an RFC 3339 timestamp's date prefix) — a bare non-date
    // string cannot anchor the observation's publication.
    let published = o.published_at.trim();
    if published
        .get(..10)
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .is_none()
    {
        return Err(format!(
            "published-at {:?} is not an ISO date",
            o.published_at
        ));
    }
    if !(0.0..=1.0).contains(&o.confidence) {
        return Err("confidence outside [0, 1]".to_string());
    }
    // Polarity validated against the metric kind: the volume-like operating
    // families are higher-is-better by construction; unit economics may carry any
    // declared direction (drafted mapping).
    let volume_like = !matches!(o.metric_kind, MetricKind::UnitEconomics);
    if volume_like && o.polarity == ObservationPolarity::LowerIsBetter {
        return Err(format!(
            "polarity lower-is-better conflicts with metric kind {}",
            o.metric_kind.as_str()
        ));
    }
    Ok(())
}

/// Merge accepted rows into the period-keyed history, sorted for stable persistence
/// (identity, then period descending, then role).
pub fn merge_observations(
    mut history: Vec<PreProfitObservation>,
    accepted: Vec<PreProfitObservation>,
) -> Vec<PreProfitObservation> {
    history.extend(accepted);
    history.sort_by(|a, b| {
        a.identity()
            .cmp(&b.identity())
            .then_with(|| b.period.trim().cmp(a.period.trim()))
            .then_with(|| a.observation_role.cmp(&b.observation_role))
    });
    history
}

/// The guidance-attainment read: pair actuals against guidance lower bounds per
/// metric identity and period, compute miss ratios, and derive the repeated /
/// material states over each identity's latest four comparable periods.
pub fn execution_read(observations: &[PreProfitObservation]) -> ExecutionRead {
    use std::collections::BTreeMap;

    // identity → period → (bound, actual): deterministic iteration via BTreeMap.
    // Only higher-is-better rows enter (the rule's polarity guard); the bound is
    // the stated low for a range, the stated value for point guidance — a
    // GuidanceLow wins over a PointGuidance for the same period.
    type Key = (String, String, String);
    let mut bounds: BTreeMap<Key, BTreeMap<String, (f64, bool)>> = BTreeMap::new();
    let mut actuals: BTreeMap<Key, BTreeMap<String, (f64, f64, String)>> = BTreeMap::new();

    for o in observations {
        if o.polarity != ObservationPolarity::HigherIsBetter {
            continue;
        }
        let key = o.identity();
        let period = o.period.trim().to_string();
        match o.observation_role {
            ObservationRole::GuidanceLow | ObservationRole::PointGuidance => {
                let is_range_low = o.observation_role == ObservationRole::GuidanceLow;
                let entry = bounds.entry(key).or_default().entry(period);
                entry
                    .and_modify(|(bound, range_low)| {
                        // A range's stated low takes precedence over point guidance.
                        if is_range_low && !*range_low {
                            *bound = o.numeric_value;
                            *range_low = true;
                        }
                    })
                    .or_insert((o.numeric_value, is_range_low));
            }
            ObservationRole::Actual => {
                let entry = actuals.entry(key).or_default().entry(period);
                entry
                    .and_modify(|(value, confidence, published)| {
                        // Deterministic pick among multiple actuals: highest
                        // confidence, then latest published-at.
                        if (o.confidence, o.published_at.as_str())
                            > (*confidence, published.as_str())
                        {
                            *value = o.numeric_value;
                            *confidence = o.confidence;
                            *published = o.published_at.clone();
                        }
                    })
                    .or_insert((o.numeric_value, o.confidence, o.published_at.clone()));
            }
            ObservationRole::GuidanceHigh | ObservationRole::ContextualLevel => {}
        }
    }

    let mut read = ExecutionRead::default();
    for (key, period_bounds) in &bounds {
        let Some(period_actuals) = actuals.get(key) else {
            continue;
        };
        // Comparable periods for this identity, newest first — the miss window.
        let mut comparable: Vec<(&String, f64, f64)> = period_bounds
            .iter()
            .filter_map(|(period, (bound, _))| {
                let (actual, _, _) = period_actuals.get(period)?;
                (bound.is_finite() && *bound > 0.0).then_some((period, *bound, *actual))
            })
            .collect();
        comparable.sort_by(|a, b| b.0.cmp(a.0));
        // Counted before the window truncation: the field's contract is
        // "periods (across identities) where an actual and a bound were
        // comparable" — the miss window bounds which periods can *miss*, not
        // how many were comparable.
        read.comparable_periods += comparable.len();
        comparable.truncate(MISS_WINDOW_PERIODS);

        let mut missed_periods = 0usize;
        for (i, (period, bound, actual)) in comparable.iter().enumerate() {
            let miss_ratio = (bound - actual) / bound;
            if at_least(miss_ratio, EXECUTION_MISS_RATIO) {
                missed_periods += 1;
                read.misses.push(ExecutionMiss {
                    metric_kind: identity_kind(&key.0),
                    units: key.1.clone(),
                    issuer_scope: key.2.clone(),
                    period: (*period).clone(),
                    miss_ratio,
                });
                if i == 0 && at_least(miss_ratio, MATERIAL_MISS_RATIO) {
                    read.material_single_miss = true;
                }
            }
        }
        if missed_periods >= REPEATED_MISS_PERIODS {
            read.repeated_miss = true;
        }
    }
    read
}

/// Recover the typed kind from an identity key's kebab label (identity keys carry
/// the kebab string for deterministic BTreeMap ordering).
fn identity_kind(label: &str) -> MetricKind {
    match label {
        "production" => MetricKind::Production,
        "deliveries" => MetricKind::Deliveries,
        "bookings" => MetricKind::Bookings,
        "backlog" => MetricKind::Backlog,
        "reservations" => MetricKind::Reservations,
        _ => MetricKind::UnitEconomics,
    }
}

/// The conjunctive severe state: ≥ 2 independent legs with at least one
/// execution-miss or economics leg (financing + dilution alone never suffices).
fn severe_deterioration(
    execution: &ExecutionRead,
    financing: FinancingState,
    economics: Option<bool>,
    dilution: Option<bool>,
) -> bool {
    let execution_leg = execution.repeated_miss || execution.material_single_miss;
    let runway_leg = financing == FinancingState::Constrained;
    let economics_leg = economics == Some(true);
    let dilution_leg = dilution == Some(true);
    let legs = [execution_leg, runway_leg, economics_leg, dilution_leg]
        .iter()
        .filter(|l| **l)
        .count();
    legs >= 2 && (execution_leg || economics_leg)
}

/// The deterministic rule consequences (`docs/portfolio-analysis.md` §Starting
/// parameters). The strictest matched ceiling binds.
fn derive_consequences(
    execution: &ExecutionRead,
    financing: FinancingState,
    severe: bool,
) -> OverlayConsequences {
    let mut c = OverlayConsequences::default();
    if execution.repeated_miss {
        c.conviction_ceiling = Some(ConvictionCeiling::Medium);
        c.matched_rules
            .push("repeated-execution-miss → conviction ceiling Medium".to_string());
    }
    if financing == FinancingState::Constrained {
        c.bar_add_family = true;
        c.matched_rules
            .push("constrained-runway → add family barred".to_string());
    }
    if severe {
        c.conviction_ceiling = Some(ConvictionCeiling::Low);
        c.bar_add_family = true;
        c.exit_family_only = true;
        c.matched_rules.push(
            "severe-deterioration → engine-arm rules: conviction ceiling Low, \
             add family barred, exit family only {trim, sell all}"
                .to_string(),
        );
    }
    c
}

/// Clamp a conviction to an engine-matched ceiling — a plain `min(value,
/// ceiling)`; Portfolio's raise machinery is retired, so there is no raise leg.
/// Returns the clamped value and whether the clamp actually lowered it.
/// **Re-scoped with `portfolio-v7`** (the two-arm unrestriction): the matched
/// ceiling never clamps the model's value anymore — its one production caller is
/// [`crate::portfolio::engine::engine_view`], where it binds the **engine
/// stand-in arm's** conviction (the engine obeys its own rules; the model's
/// exceedance persists as an annotation — `docs/portfolio-analysis.md` §The
/// holding verdict).
pub fn clamp_conviction(
    conviction: Conviction,
    ceiling: Option<ConvictionCeiling>,
) -> (Conviction, bool) {
    let rank = |c: Conviction| match c {
        Conviction::Low => 0u8,
        Conviction::Medium => 1,
        Conviction::High => 2,
    };
    let cap = match ceiling {
        None => return (conviction, false),
        Some(ConvictionCeiling::Medium) => Conviction::Medium,
        Some(ConvictionCeiling::Low) => Conviction::Low,
    };
    if rank(conviction) > rank(cap) {
        (cap, true)
    } else {
        (conviction, false)
    }
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

    fn observation(
        kind: MetricKind,
        role: ObservationRole,
        value: f64,
        period: &str,
    ) -> PreProfitObservation {
        PreProfitObservation {
            metric_kind: kind,
            observation_role: role,
            polarity: ObservationPolarity::HigherIsBetter,
            numeric_value: value,
            units: "units".into(),
            period: period.into(),
            issuer_scope: "company".into(),
            source_url: "https://example.com/report".into(),
            published_at: "2026-08-01".into(),
            confidence: 0.9,
        }
    }

    // ---- Eligibility ----

    #[test]
    fn eligible_on_negative_operating_income() {
        let overlay = compute_overlay(&burning_stock(), None, vec![]);
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
        let overlay = compute_overlay(&fin, None, vec![]);
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
        let overlay = compute_overlay(&fin, None, vec![]);
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
        let overlay = compute_overlay(&fin, None, vec![]);
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
        let overlay = compute_overlay(&fin, None, vec![]);
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
        let overlay = compute_overlay(&fin, None, vec![]);
        assert_eq!(overlay.financing_state, FinancingState::Adequate);
        assert_eq!(overlay.statement_inputs.runway_months, Some(24.0));

        // Liquid 200M → runway 15 months → watch.
        fin.cash_and_equivalents = Some(200.0e6);
        fin.short_term_investments = None;
        let overlay = compute_overlay(&fin, None, vec![]);
        assert_eq!(overlay.financing_state, FinancingState::Watch);

        // Liquid 100M → runway 7.5 months → constrained (and the add bar).
        fin.cash_and_equivalents = Some(100.0e6);
        let overlay = compute_overlay(&fin, None, vec![]);
        assert_eq!(overlay.financing_state, FinancingState::Constrained);
        assert!(overlay.consequences.bar_add_family);
        assert!(overlay.consequences.conviction_ceiling.is_none());

        // Positive FCF → not burning, no runway.
        for row in &mut fin.quarterly_cash_flow {
            row.free_cash_flow = Some(5.0e6);
        }
        // Keep eligibility via arm 1 (operating income stays negative).
        let overlay = compute_overlay(&fin, None, vec![]);
        assert_eq!(overlay.financing_state, FinancingState::NotBurning);
        assert_eq!(overlay.statement_inputs.runway_months, None);

        // No cash line while burning → unscorable.
        for row in &mut fin.quarterly_cash_flow {
            row.free_cash_flow = Some(-40.0e6);
        }
        fin.cash_and_equivalents = None;
        let overlay = compute_overlay(&fin, None, vec![]);
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
        let overlay = compute_overlay(&fin, None, vec![]);
        assert_eq!(overlay.material_dilution, Some(true));
        assert_eq!(overlay.economics_deterioration, Some(true));
        // Economics + dilution = two legs incl. an economics leg → severe.
        assert!(overlay.severe_deterioration);
        assert_eq!(
            overlay.consequences.conviction_ceiling,
            Some(ConvictionCeiling::Low)
        );
        assert!(overlay.consequences.bar_add_family);
        assert!(overlay.consequences.exit_family_only);
    }

    #[test]
    fn financing_plus_dilution_alone_is_not_severe() {
        let mut fin = burning_stock();
        fin.cash_and_equivalents = Some(50.0e6); // constrained runway
        fin.short_term_investments = None;
        fin.quarterly_income[0].diluted_shares = Some(130.0e6); // material dilution
        let overlay = compute_overlay(&fin, None, vec![]);
        assert_eq!(overlay.financing_state, FinancingState::Constrained);
        assert_eq!(overlay.material_dilution, Some(true));
        assert_eq!(overlay.economics_deterioration, Some(false));
        assert!(!overlay.severe_deterioration);
        // The constrained-runway bar still stands on its own.
        assert!(overlay.consequences.bar_add_family);
        assert!(!overlay.consequences.exit_family_only);
    }

    #[test]
    fn capex_intensity_reads_magnitude_over_ttm_revenue() {
        let mut fin = burning_stock();
        for row in &mut fin.quarterly_cash_flow {
            row.capex = Some(-10.0e6);
        }
        let overlay = compute_overlay(&fin, None, vec![]);
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
        let overlay = compute_overlay(&fin, None, vec![]);
        assert_eq!(
            overlay.statement_inputs.ttm_capex_intensity, None,
            "shifted-but-contiguous windows must not divide"
        );
        assert!(overlay.statement_inputs.ttm_operating_income.is_some());
        assert!(overlay.statement_inputs.ttm_free_cash_flow.is_some());
    }

    // ---- Observation validation, merge, and the miss rules ----

    /// A source-evidence fixture whose one page mentions the holding and
    /// states the given values — the activation legs' pass case.
    fn evidence_texts(values: &[f64]) -> std::collections::HashMap<String, String> {
        let stated = values
            .iter()
            .map(|v| format!("{v}"))
            .collect::<Vec<_>>()
            .join(" and ");
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            format!("ACME Motors (NASDAQ: ACME) reported deliveries of {stated} units this period."),
        );
        texts
    }

    #[test]
    fn validation_rejects_malformed_rows_with_reasons() {
        let good = observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q2");
        let bad_value = PreProfitObservation {
            numeric_value: f64::NAN,
            ..good.clone()
        };
        let bad_source = PreProfitObservation {
            source_url: "not a url".into(),
            ..good.clone()
        };
        let bad_polarity = PreProfitObservation {
            polarity: ObservationPolarity::LowerIsBetter,
            ..good.clone()
        };
        let texts = evidence_texts(&[100.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: Some("ACME Motors"),
        };
        let (accepted, rejected) = validate_observations(
            vec![good, bad_value, bad_source, bad_polarity],
            &[],
            Some(&evidence),
        );
        assert_eq!(accepted.len(), 1);
        // The accepted row's period normalized to the ISO quarter end.
        assert_eq!(accepted[0].period, "2026-06-30");
        assert_eq!(rejected.len(), 3);
        assert!(rejected.iter().any(|r| r.reason.contains("non-finite")));
        assert!(rejected.iter().any(|r| r.reason.contains("source")));
        assert!(rejected.iter().any(|r| r.reason.contains("polarity")));
    }

    #[test]
    fn duplicate_of_stored_history_is_rejected() {
        // Stored history holds normalized periods (it came from prior accepted
        // rows); a re-offered duplicate normalizes to the same key.
        let stored =
            observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-06-30");
        let dup = observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q2");
        let fresh = observation(MetricKind::Deliveries, ObservationRole::Actual, 90.0, "2026-Q1");
        let texts = evidence_texts(&[100.0, 90.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let (accepted, rejected) = validate_observations(
            vec![dup, fresh],
            std::slice::from_ref(&stored),
            Some(&evidence),
        );
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].period, "2026-03-31");
        assert_eq!(rejected.len(), 1);
        assert!(rejected[0].reason.contains("duplicate"));
    }

    #[test]
    fn the_activation_legs_bind_identity_corroboration_and_lineage() {
        let good = observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q2");

        // No evidence at all: every candidate is rejected — rows enter only
        // through the research loop's lineage.
        let (accepted, rejected) = validate_observations(vec![good.clone()], &[], None);
        assert!(accepted.is_empty());
        assert!(rejected[0].reason.contains("no source evidence"));

        // A URL the loop never fetched fails the lineage leg.
        let empty = std::collections::HashMap::new();
        let evidence = SourceEvidence {
            texts: &empty,
            symbol: "ACME",
            company_name: None,
        };
        let (_, rejected) = validate_observations(vec![good.clone()], &[], Some(&evidence));
        assert!(rejected[0].reason.contains("was not fetched"));

        // A page that never mentions the holding fails the identity leg.
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "Some other issuer reported deliveries of 100 units.".to_string(),
        );
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: Some("ACME Motors"),
        };
        let (_, rejected) = validate_observations(vec![good.clone()], &[], Some(&evidence));
        assert!(rejected[0].reason.contains("never mentions the holding"));

        // A page that mentions the holding but never states the value fails
        // corroboration.
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "$ACME reported strong deliveries this period.".to_string(),
        );
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let (_, rejected) = validate_observations(vec![good.clone()], &[], Some(&evidence));
        assert!(rejected[0].reason.contains("does not appear"));

        // A digit-substring never corroborates: 41 must not match inside 141.
        let short = observation(MetricKind::Deliveries, ObservationRole::Actual, 41.0, "2026-Q2");
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "$ACME reported 141 deliveries.".to_string(),
        );
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let (_, rejected) = validate_observations(vec![short], &[], Some(&evidence));
        assert!(rejected[0].reason.contains("does not appear"));

        // Comma-separated renderings still corroborate (12,000 states 12000).
        let big = observation(MetricKind::Deliveries, ObservationRole::Actual, 12000.0, "2026-Q2");
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "$ACME reported 12,000 deliveries.".to_string(),
        );
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let (accepted, _) = validate_observations(vec![big], &[], Some(&evidence));
        assert_eq!(accepted.len(), 1);
    }

    #[test]
    fn corroboration_respects_decimal_number_boundaries() {
        // An integer must not corroborate off a decimal it merely prefixes or
        // trails: 41 is not stated by "41.5" or "3.41".
        assert!(!value_in_text(41.0, "ACME guided to 41.5 units."));
        assert!(!value_in_text(41.0, "margin of 3.41 percent"));
        // The digit-neighbor rules still hold.
        assert!(!value_in_text(41.0, "some 141 units"));
        assert!(!value_in_text(41.0, "about 412 units"));
        // Exact statements corroborate, decimals included — and an integer
        // matches its decimal render.
        assert!(value_in_text(41.0, "delivered 41 units"));
        assert!(value_in_text(41.5, "guided to 41.5 units"));
        assert!(value_in_text(41.0, "(41)"));
        assert!(value_in_text(41.0, "delivered 41.0 units"));
        assert!(!value_in_text(41.0, "delivered 41.05 units"));
    }

    #[test]
    fn identity_check_ignores_generic_name_tokens_and_matches_whole_words() {
        // A generic corporate suffix ("Holdings", "Company") never identifies
        // the issuer — a cross-issuer page mentioning only those words fails.
        assert!(!page_mentions_holding(
            "Rival Holdings Company reported record output of widgets.",
            "ACME",
            Some("ACME Holdings Company"),
        ));
        // A distinctive name token passes (sentence-initial needs the
        // proper-noun run — the following word capitalized)…
        assert!(page_mentions_holding(
            "Acme Holdings reported record output.",
            "XYZ",
            Some("ACME Holdings Company"),
        ));
        assert!(page_mentions_holding(
            "Shares of Acme rallied on the report.",
            "XYZ",
            Some("ACME Holdings Company"),
        ));
        // …but not as a substring of a longer word (COMPANY / ACCOMPANYING).
        assert!(!page_mentions_holding(
            "The accompanying tables cover the sector.",
            "XYZ",
            Some("Widget Company"),
        ));
        // The symbol leg needs TICKER CONTEXT ($ or an exchange/label colon) —
        // a bare uppercase word is not identity evidence, so English-word
        // tickers (ALL, LOW, CAT) can never match prose or page furniture.
        assert!(page_mentions_holding("$ACME beat estimates.", "ACME", None));
        assert!(page_mentions_holding(
            "(NASDAQ: ACME) beat estimates.",
            "ACME",
            None,
        ));
        assert!(!page_mentions_holding("ACME beat estimates.", "ACME", None));
        assert!(!page_mentions_holding(
            "© 2026. ALL RIGHTS RESERVED.",
            "ALL",
            None,
        ));
        assert!(!page_mentions_holding(
            "The CAT scan showed LOW readings.",
            "CAT",
            None,
        ));
        assert!(page_mentions_holding("$CAT rallied.", "CAT", None));
        // A colon qualifies only under an exchange / ticker label — a generic
        // label's value never becomes holding identity.
        assert!(!page_mentions_holding("Risk: LOW across the book.", "LOW", None));
        assert!(!page_mentions_holding("Rating: ALL clear.", "ALL", None));
        assert!(!page_mentions_holding("Category: CAT equipment.", "CAT", None));
        assert!(page_mentions_holding("Listed as NYSE: CAT today.", "CAT", None));
        assert!(page_mentions_holding("ticker: CAT", "CAT", None));
        // An ordinary-word issuer name matches only as a capitalized proper
        // noun, and a sentence-initial match needs the proper-noun run —
        // "Target price increased…" never identifies Target Corporation.
        assert!(!page_mentions_holding(
            "Analysts raised the price target on several retailers.",
            "TGT",
            Some("Target Corporation"),
        ));
        assert!(!page_mentions_holding(
            "Target price increased to $200 across the group.",
            "TGT",
            Some("Target Corporation"),
        ));
        assert!(page_mentions_holding(
            "Target Corporation reported quarterly results.",
            "TGT",
            Some("Target Corporation"),
        ));
        assert!(page_mentions_holding(
            "Comparable sales at Target rose 4%.",
            "TGT",
            Some("Target Corporation"),
        ));
    }

    #[test]
    fn published_at_must_be_an_iso_date() {
        // Structural validation sees post-normalization rows, so use an ISO
        // period here (the period leg has its own test below).
        let mut o =
            observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-06-30");
        o.published_at = "recently".into();
        let err = validate_observation(&o).unwrap_err();
        assert!(err.contains("not an ISO date"), "{err}");
        o.published_at = "2026-08-20".into();
        assert!(validate_observation(&o).is_ok());
        // An RFC 3339 timestamp's date prefix also anchors.
        o.published_at = "2026-08-20T14:00:00Z".into();
        assert!(validate_observation(&o).is_ok());
    }

    #[test]
    fn a_period_that_does_not_normalize_to_iso_rejects_the_row() {
        // Two model-authored rows sharing a fabricated prose period must never
        // pair into an execution miss — the one-ISO-convention rule has teeth.
        let good = observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q2");
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "$ACME reported 100 deliveries.".to_string(),
        );
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        // The recognized form normalizes and passes end to end.
        let (accepted, _) = validate_observations(vec![good.clone()], &[], Some(&evidence));
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].period, "2026-06-30");
        // Arbitrary prose does not normalize and rejects with the reason.
        let mut prose = good;
        prose.period = "thirteen weeks ended".into();
        let (accepted, rejected) = validate_observations(vec![prose], &[], Some(&evidence));
        assert!(accepted.is_empty());
        assert!(
            rejected[0].reason.contains("did not normalize to an ISO period-end"),
            "{}",
            rejected[0].reason
        );
    }

    #[test]
    fn periods_normalize_to_one_iso_convention() {
        assert_eq!(normalize_period("2026-06-30"), "2026-06-30");
        assert_eq!(normalize_period("2026-06"), "2026-06-30");
        assert_eq!(normalize_period("Q2 2026"), "2026-06-30");
        assert_eq!(normalize_period("2026 Q2"), "2026-06-30");
        assert_eq!(normalize_period("2026-Q2"), "2026-06-30");
        assert_eq!(normalize_period("H1 2026"), "2026-06-30");
        assert_eq!(normalize_period("FY2026"), "2026-12-31");
        assert_eq!(normalize_period("FY 2026"), "2026-12-31");
        assert_eq!(normalize_period("2026"), "2026-12-31");
        assert_eq!(normalize_period("Q4 2025"), "2025-12-31");
        // An unrecognized form trims and stands here — structural validation
        // then rejects the non-ISO result, so it can never enter the history.
        assert_eq!(normalize_period(" thirteen weeks ended "), "thirteen weeks ended");
    }

    /// Guidance/actual pairs across four periods for one identity.
    fn guided_history(pairs: &[(&str, f64, f64)]) -> Vec<PreProfitObservation> {
        pairs
            .iter()
            .flat_map(|(period, bound, actual)| {
                vec![
                    observation(MetricKind::Deliveries, ObservationRole::GuidanceLow, *bound, period),
                    observation(MetricKind::Deliveries, ObservationRole::Actual, *actual, period),
                ]
            })
            .collect()
    }

    #[test]
    fn miss_rules_five_percent_and_material_twenty() {
        // Latest period 25% below bound → miss AND material single miss; a 4%
        // shortfall is in-line noise.
        let history = guided_history(&[
            ("2026-Q2", 100.0, 75.0),
            ("2026-Q1", 100.0, 96.0),
        ]);
        let read = execution_read(&history);
        assert_eq!(read.comparable_periods, 2);
        assert_eq!(read.misses.len(), 1);
        assert!((read.misses[0].miss_ratio - 0.25).abs() < 1e-12);
        assert!(read.material_single_miss);
        assert!(!read.repeated_miss);
    }

    #[test]
    fn repeated_miss_needs_two_distinct_periods_same_metric() {
        let history = guided_history(&[
            ("2026-Q2", 100.0, 90.0),
            ("2026-Q1", 100.0, 92.0),
            ("2025-Q4", 100.0, 99.0),
        ]);
        let read = execution_read(&history);
        assert!(read.repeated_miss);
        assert!(!read.material_single_miss);
    }

    #[test]
    fn two_metrics_missing_in_one_period_never_count_twice() {
        let mut history = guided_history(&[("2026-Q2", 100.0, 90.0)]);
        history.extend(vec![
            observation(MetricKind::Bookings, ObservationRole::GuidanceLow, 200.0, "2026-Q2"),
            observation(MetricKind::Bookings, ObservationRole::Actual, 180.0, "2026-Q2"),
        ]);
        let read = execution_read(&history);
        assert_eq!(read.misses.len(), 2);
        assert!(!read.repeated_miss, "two metrics in one period are never repeated");
    }

    #[test]
    fn miss_window_is_latest_four_comparable_periods() {
        // Two old misses outside the latest-four window; the window's periods are
        // all in-line → no repeated miss.
        let history = guided_history(&[
            ("2026-Q2", 100.0, 100.0),
            ("2026-Q1", 100.0, 100.0),
            ("2025-Q4", 100.0, 100.0),
            ("2025-Q3", 100.0, 100.0),
            ("2025-Q2", 100.0, 80.0),
            ("2025-Q1", 100.0, 80.0),
        ]);
        let read = execution_read(&history);
        assert!(!read.repeated_miss);
        assert!(read.misses.is_empty());
        // The field's contract counts every comparable period across identities;
        // only the MISS rule is window-scoped (a pre-fix truncate capped this
        // at 4, understating the persisted/prompted count).
        assert_eq!(read.comparable_periods, 6);
    }

    #[test]
    fn point_guidance_is_the_bound_and_range_low_wins() {
        let mut history = vec![
            observation(MetricKind::Deliveries, ObservationRole::PointGuidance, 100.0, "2026-Q2"),
            observation(MetricKind::Deliveries, ObservationRole::Actual, 90.0, "2026-Q2"),
        ];
        let read = execution_read(&history);
        assert_eq!(read.misses.len(), 1, "point guidance supplies the bound");

        // A stated range low (95) displaces the point bound (100): 90 vs 95 → ~5.3%.
        history.push(observation(
            MetricKind::Deliveries,
            ObservationRole::GuidanceLow,
            95.0,
            "2026-Q2",
        ));
        let read = execution_read(&history);
        assert_eq!(read.misses.len(), 1);
        assert!((read.misses[0].miss_ratio - (5.0 / 95.0)).abs() < 1e-12);
    }

    #[test]
    fn lower_is_better_rows_never_enter_the_miss_rule() {
        let mut o = observation(MetricKind::UnitEconomics, ObservationRole::GuidanceLow, 100.0, "2026-Q2");
        o.polarity = ObservationPolarity::LowerIsBetter;
        let mut a = observation(MetricKind::UnitEconomics, ObservationRole::Actual, 150.0, "2026-Q2");
        a.polarity = ObservationPolarity::LowerIsBetter;
        let read = execution_read(&[o, a]);
        assert_eq!(read.comparable_periods, 0);
        assert!(read.misses.is_empty());
    }

    #[test]
    fn repeated_miss_caps_medium_and_severe_needs_a_second_leg() {
        let mut fin = burning_stock();
        // Healthy margins and shares; adequate runway → repeated miss alone.
        let history = guided_history(&[
            ("2026-Q2", 100.0, 90.0),
            ("2026-Q1", 100.0, 92.0),
        ]);
        let prior = PreProfitOverlay {
            observations: history,
            ..compute_overlay(&fin, None, vec![])
        };
        let overlay = compute_overlay(&fin, Some(&prior), vec![]);
        assert!(overlay.execution.repeated_miss);
        assert_eq!(
            overlay.consequences.conviction_ceiling,
            Some(ConvictionCeiling::Medium)
        );
        assert!(!overlay.consequences.bar_add_family);
        assert!(!overlay.severe_deterioration);

        // Add constrained runway → second leg beside the execution leg → severe,
        // and the Low ceiling displaces Medium (strictest binds).
        fin.cash_and_equivalents = Some(50.0e6);
        fin.short_term_investments = None;
        let overlay = compute_overlay(&fin, Some(&prior), vec![]);
        assert!(overlay.severe_deterioration);
        assert_eq!(
            overlay.consequences.conviction_ceiling,
            Some(ConvictionCeiling::Low)
        );
        assert!(overlay.consequences.exit_family_only);
    }

    // ---- History carry ----

    #[test]
    fn history_carries_and_accumulates_across_runs() {
        let fin = burning_stock();
        let texts = evidence_texts(&[100.0, 110.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let first = compute_overlay_with_sources(
            &fin,
            None,
            vec![observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q1")],
            Some(&evidence),
        );
        assert_eq!(first.observations.len(), 1);
        let second = compute_overlay_with_sources(
            &fin,
            Some(&first),
            vec![observation(MetricKind::Deliveries, ObservationRole::Actual, 110.0, "2026-Q2")],
            Some(&evidence),
        );
        assert_eq!(second.observations.len(), 2);
    }

    #[test]
    fn the_unevidenced_producer_path_admits_no_row() {
        // The structural contract behind the discharged activation obligation:
        // without the research loop's fetched-page lineage, a candidate can
        // only be rejected — `compute_overlay` (the 6b seam) can never grow
        // the observation history.
        let fin = burning_stock();
        let overlay = compute_overlay(
            &fin,
            None,
            vec![observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q1")],
        );
        assert!(overlay.observations.is_empty());
        assert_eq!(overlay.rejected.len(), 1);
        assert!(overlay.rejected[0].reason.contains("no source evidence"));
    }

    #[test]
    fn history_survives_a_not_eligible_run() {
        let mut fin = burning_stock();
        let texts = evidence_texts(&[100.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let first = compute_overlay_with_sources(
            &fin,
            None,
            vec![observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q1")],
            Some(&evidence),
        );
        // The name turns profitable: not eligible, but the history rides along.
        for row in &mut fin.quarterly_income {
            row.operating_income = Some(10.0e6);
        }
        for row in &mut fin.quarterly_cash_flow {
            row.free_cash_flow = Some(5.0e6);
        }
        let second = compute_overlay(&fin, Some(&first), vec![]);
        assert_eq!(second.eligibility, PreProfitEligibility::NotEligible);
        assert_eq!(second.observations.len(), 1);
    }

    #[test]
    fn backfill_counts_comparable_periods_not_any_role() {
        let base = compute_overlay(&burning_stock(), None, vec![]);
        assert!(base.is_eligible());
        let with = |observations: Vec<PreProfitObservation>| PreProfitOverlay {
            observations,
            ..base.clone()
        };
        let rows = |role: ObservationRole, periods: &[&str]| -> Vec<PreProfitObservation> {
            periods
                .iter()
                .map(|p| observation(MetricKind::Deliveries, role, 100.0, p))
                .collect()
        };
        let periods = ["2026-Q2", "2026-Q1", "2025-Q4", "2025-Q3"];
        // The first overlay-eligible pass binds regardless of history.
        assert!(backfill_required(&with(vec![]), None));
        // Four guidance rows in four periods and no actuals: zero comparable
        // periods, so the obligation binds (the any-role count of four had
        // suppressed it).
        let guidance_only = rows(ObservationRole::GuidanceLow, &periods);
        assert!(backfill_required(&with(guidance_only), Some(&base)));
        // Four bound + actual pairs discharge it.
        let four_pairs = guided_history(&[
            ("2026-Q2", 100.0, 100.0),
            ("2026-Q1", 100.0, 100.0),
            ("2025-Q4", 100.0, 100.0),
            ("2025-Q3", 100.0, 100.0),
        ]);
        assert!(!backfill_required(&with(four_pairs.clone()), Some(&base)));
        // Three pairs plus an unpaired guidance period and an unpaired actual
        // period: five distinct periods, three comparable — binds.
        let mut three = guided_history(&[
            ("2026-Q2", 100.0, 100.0),
            ("2026-Q1", 100.0, 100.0),
            ("2025-Q4", 100.0, 100.0),
        ]);
        three.extend(rows(ObservationRole::GuidanceLow, &["2025-Q3"]));
        three.extend(rows(ObservationRole::Actual, &["2025-Q2"]));
        assert!(backfill_required(&with(three), Some(&base)));
        // Point guidance is a bound; a range high alone is not.
        let actuals = rows(ObservationRole::Actual, &periods);
        let mut point = rows(ObservationRole::PointGuidance, &periods);
        point.extend(actuals.clone());
        assert!(!backfill_required(&with(point), Some(&base)));
        let mut high = rows(ObservationRole::GuidanceHigh, &periods);
        high.extend(actuals.clone());
        assert!(backfill_required(&with(high), Some(&base)));
        // A never-guided metric carries no obligation.
        assert!(!backfill_required(&with(actuals), Some(&base)));
        // A covered identity never discharges a thin one.
        let bookings = |role| observation(MetricKind::Bookings, role, 50.0, "2026-Q2");
        let mut mixed = four_pairs;
        mixed.push(bookings(ObservationRole::GuidanceLow));
        mixed.push(bookings(ObservationRole::Actual));
        assert!(backfill_required(&with(mixed), Some(&base)));
    }

    // ---- Clamp + schema labels ----

    #[test]
    fn conviction_clamps_to_the_matched_ceiling() {
        use crate::portfolio::Conviction::*;
        assert_eq!(clamp_conviction(High, None), (High, false));
        assert_eq!(
            clamp_conviction(High, Some(ConvictionCeiling::Medium)),
            (Medium, true)
        );
        assert_eq!(
            clamp_conviction(Medium, Some(ConvictionCeiling::Medium)),
            (Medium, false)
        );
        assert_eq!(
            clamp_conviction(Medium, Some(ConvictionCeiling::Low)),
            (Low, true)
        );
        assert_eq!(clamp_conviction(Low, Some(ConvictionCeiling::Low)), (Low, false));
    }

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
        let canonical = compute_overlay(&fin, None, vec![]);
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
        let shuffled = compute_overlay(&shuffled_fin, None, vec![]);
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

        let a = compute_overlay(&fin_a, None, vec![]);
        let b = compute_overlay(&fin_b, None, vec![]);
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
        let overlay = compute_overlay(&fin, None, vec![]);
        assert!(
            (overlay.statement_inputs.diluted_share_change_yoy.unwrap() - 0.15).abs() < 1e-9
        );
        assert_eq!(overlay.material_dilution, Some(true));

        // A miss exactly at the 20% material boundary: (9 − 7.2) ÷ 9 rounds just
        // below 0.20 — still material.
        let history = guided_history(&[("2026-Q2", 9.0, 7.2)]);
        let read = execution_read(&history);
        assert!(read.material_single_miss, "{:?}", read.misses);
    }

    // ---- Serde stability ----

    #[test]
    fn overlay_round_trips_and_pre_field_json_decodes() {
        let overlay = compute_overlay(&burning_stock(), None, vec![]);
        let json = serde_json::to_string(&overlay).expect("serialize");
        let back: PreProfitOverlay = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(overlay, back);
    }
}
