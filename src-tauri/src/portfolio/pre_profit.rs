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
//! normalize to an ISO end plus an explicit span before the dedup key is taken
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
/// `pre-profit-v3`: the guidance vintage policy (the 2026-08-24 review's
/// Codex I4) — the execution read pairs an actual only against ex-ante
/// guidance (dated on or before the period end and strictly before the
/// period's earliest actual), the latest such revision binding, and a
/// same-vintage conflict on either side drops the period; a v2 read could
/// pair a results release's restated guidance against its own actual and
/// selected among revisions by persistence order, so a v2 record's
/// execution read does not mean what a v3 read means. `pre-profit-v4`: the
/// reporting span is part of the comparison identity, so a full-year or
/// half-year bound can never attain against a quarter ending on the same day,
/// nor can unlike spans discharge one another's backfill depth.
pub const PRE_PROFIT_PARAMETER_VERSION: &str = "pre-profit-v4";

/// The cap on a row's quoted source excerpt (drafted): the excerpt is a
/// locator — the page's own sentence that states the value — never a page,
/// so an over-cap quote rejects structurally before any page comparison.
pub const SOURCE_EXCERPT_CAP_CHARS: usize = 400;

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

/// The duration or instant one observation covers. The period end alone is
/// not a period: a quarter, half, and full year can all end on the same date.
/// `Unknown` remains admissible as sourced audit context but never enters a
/// guidance-vs-actual comparison.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum PeriodSpan {
    Quarter,
    HalfYear,
    FullYear,
    YearToDate,
    PointInTime,
    Unknown,
}

/// One operating observation as the model offers it — the 6d wire row and the
/// pre-admission candidate (`docs/portfolio-workflow.md` §Step 6d). The model
/// may extract a row only from source text that states the value; validation
/// and computation own every comparison and state. A candidate reaches the
/// history only as a [`PreProfitObservation`], stamped at acceptance — this
/// type carries no stamp field, so the model can never author one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationCandidate {
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
    /// The duration represented by [`Self::period`]. Guidance and actuals pair
    /// only inside one exact, non-unknown span.
    pub period_span: PeriodSpan,
    pub issuer_scope: String,
    pub source_url: String,
    /// The page's own sentence that states the value, quoted verbatim — the
    /// locator the app verifies against the fetched page (whitespace-run
    /// normalized), corroborates the value inside sign-aware, and reads the
    /// metric-family language from (`docs/portfolio-workflow.md` §Step 6e).
    /// Persisted on every accepted and rejected row so an audit can read the
    /// sentence a number was taken from.
    pub source_excerpt: String,
    pub published_at: String,
    /// Extraction confidence, 0–1.
    pub confidence: f64,
}

/// One app-validated operating observation in the overlay's period-end-and-span-keyed
/// history — the research leg's only entry into the overlay: a candidate the
/// admission contract accepted, carrying the prompt stamp it was admitted
/// under. The fields mirror [`ObservationCandidate`] one for one; only the
/// stamp is added, by [`ObservationCandidate::admit`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreProfitObservation {
    pub metric_kind: MetricKind,
    pub observation_role: ObservationRole,
    pub polarity: ObservationPolarity,
    pub numeric_value: f64,
    pub units: String,
    /// See [`ObservationCandidate::period`].
    pub period: String,
    /// See [`ObservationCandidate::period_span`].
    pub period_span: PeriodSpan,
    pub issuer_scope: String,
    pub source_url: String,
    /// See [`ObservationCandidate::source_excerpt`].
    pub source_excerpt: String,
    pub published_at: String,
    /// Extraction confidence, 0–1.
    pub confidence: f64,
    /// The prompt stamp (`portfolio::PROMPT_VERSION`) whose admission contract
    /// admitted the row — written by the app at acceptance, never by the model
    /// (`docs/portfolio-workflow.md` §Step 6e; the 2026-08-24 review's Codex
    /// I20). The history is never re-admitted through a later filter, so a row
    /// admitted under a looser contract stays in the history telling itself
    /// apart by this stamp, read by the audit and by a later calibration of
    /// the stem table. The dedup key leaves it out: the same fact re-offered
    /// under a later contract is a duplicate, and the first admission stands.
    /// No serde default — no store holds a row without it.
    pub admitted_under: String,
}

/// The normalized comparison identity misses group under: kind + units +
/// issuer scope + reporting span (`docs/portfolio-analysis.md` §Starting
/// parameters). Keeping span in the identity prevents annual and quarterly
/// rows sharing one end date from combining anywhere downstream.
type ObservationIdentity = (String, String, String, PeriodSpan);

fn identity_of(
    metric_kind: MetricKind,
    units: &str,
    issuer_scope: &str,
    period_span: PeriodSpan,
) -> ObservationIdentity {
    (
        metric_kind.as_str().to_string(),
        units.trim().to_ascii_lowercase(),
        issuer_scope.trim().to_ascii_lowercase(),
        period_span,
    )
}

/// The dedup key's shape: the identity (including span), the role, the period
/// end, the source URL, the publication date, and the value's bit pattern.
type DedupKey = (
    String,
    String,
    String,
    PeriodSpan,
    ObservationRole,
    String,
    String,
    String,
    u64,
);

/// The dedup key (`docs/storage.md` — "deduplicated by issuer + normalized
/// metric identity + span + role + period end + source URL + publication date +
/// value"). A duplicate is the same fact re-offered — the same source
/// stating the same value on the same date; a same-source revision (a new
/// date and value) or a same-page conflict (one date, two values) is a
/// distinct observation that must reach the execution read, where the
/// guidance vintage policy selects the revision or drops the conflicting
/// period (Codex I4, round 1). The date is the parsed ISO render so the two
/// spellings of one day collapse; the value keys on its bit pattern (every
/// admitted value is finite). The admission stamp is deliberately outside
/// the key (Codex I20): a stored row re-offered under a later contract is a
/// duplicate and keeps its first stamp. Shared by the candidate and the
/// admitted row, so a candidate's key compares against the history's.
#[allow(clippy::too_many_arguments)]
fn dedup_key_of(
    metric_kind: MetricKind,
    units: &str,
    issuer_scope: &str,
    period_span: PeriodSpan,
    observation_role: ObservationRole,
    period: &str,
    source_url: &str,
    published_at: &str,
    numeric_value: f64,
) -> DedupKey {
    let (kind, units, scope, span) =
        identity_of(metric_kind, units, issuer_scope, period_span);
    let published = published_date(published_at)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| published_at.trim().to_string());
    (
        kind,
        units,
        scope,
        span,
        observation_role,
        period.trim().to_string(),
        source_url.trim().to_string(),
        published,
        numeric_value.to_bits(),
    )
}

impl ObservationCandidate {
    /// Admit the candidate into the history under the prompt stamp whose
    /// contract accepted it — the one place a [`PreProfitObservation`] is
    /// built from a model row, so every admitted row carries its stamp.
    pub fn admit(self, admitted_under: &str) -> PreProfitObservation {
        PreProfitObservation {
            metric_kind: self.metric_kind,
            observation_role: self.observation_role,
            polarity: self.polarity,
            numeric_value: self.numeric_value,
            units: self.units,
            period: self.period,
            period_span: self.period_span,
            issuer_scope: self.issuer_scope,
            source_url: self.source_url,
            source_excerpt: self.source_excerpt,
            published_at: self.published_at,
            confidence: self.confidence,
            admitted_under: admitted_under.to_string(),
        }
    }

    fn dedup_key(&self) -> DedupKey {
        dedup_key_of(
            self.metric_kind,
            &self.units,
            &self.issuer_scope,
            self.period_span,
            self.observation_role,
            &self.period,
            &self.source_url,
            &self.published_at,
            self.numeric_value,
        )
    }
}

impl PreProfitObservation {
    /// See [`identity_of`].
    pub(crate) fn identity(&self) -> ObservationIdentity {
        identity_of(
            self.metric_kind,
            &self.units,
            &self.issuer_scope,
            self.period_span,
        )
    }

    fn dedup_key(&self) -> DedupKey {
        dedup_key_of(
            self.metric_kind,
            &self.units,
            &self.issuer_scope,
            self.period_span,
            self.observation_role,
            &self.period,
            &self.source_url,
            &self.published_at,
            self.numeric_value,
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
    type Identity = ObservationIdentity;
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
/// shows what research offered and why it did not enter the history. It holds
/// the candidate as offered and takes no admission stamp: the list is rebuilt
/// from the candidate batch whenever the holding is re-analyzed (the prior
/// overlay's rejected rows are never read), and a carried verdict carries its
/// prior audit whole, rejected rows included, so the audit's own prompt
/// version names the contract that rejected them either way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RejectedObservation {
    pub observation: ObservationCandidate,
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
    /// The exact reporting span this attempt tried to fill.
    pub period_span: PeriodSpan,
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
    pub period_span: PeriodSpan,
    /// `(bound − actual) ÷ bound`.
    pub miss_ratio: f64,
    /// The ISO date the binding guidance was published — the vintage the
    /// bound was read from under the guidance vintage policy
    /// (`docs/portfolio-analysis.md` §Starting parameters), so an audit can
    /// see which revision a miss was measured against.
    pub bound_published_at: String,
    /// The ISO date the selected actual was published.
    pub actual_published_at: String,
}

/// The engine's guidance-attainment read over the validated observation history.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExecutionRead {
    /// Periods (across identities) where an actual and a finite positive
    /// higher-is-better guidance bound were comparable under the guidance
    /// vintage policy (an ex-ante bound, no same-vintage conflict on either
    /// side).
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
/// holding's audit row so the period-end-and-span-keyed observation history survives run
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
    /// The period-end-and-span-keyed validated observation history.
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
/// previous run's overlay so the period-end-and-span-keyed history accumulates
/// across runs (`docs/storage.md` — "continuity evidence for the holding").
pub fn compute_overlay(
    fin: &CompanyFinancials,
    prior: Option<&PreProfitOverlay>,
    candidates: Vec<ObservationCandidate>,
) -> PreProfitOverlay {
    compute_overlay_with_sources(fin, prior, candidates, None)
}

/// The evidenced form — the research loop's producer path: candidate rows are
/// validated with the two activation legs against the loop's fetched pages
/// (`docs/portfolio-workflow.md` §Step 6e). The unevidenced [`compute_overlay`]
/// rejects every candidate, so rows enter the history only through this seam,
/// each stamped with the prompt version it was admitted under (Codex I20).
pub fn compute_overlay_with_sources(
    fin: &CompanyFinancials,
    prior: Option<&PreProfitOverlay>,
    candidates: Vec<ObservationCandidate>,
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

/// Where a period label itself declares a span, require the typed span to say
/// the same thing. ISO-only labels deliberately carry no inference: fiscal
/// calendars and point-in-time facts need the producer's explicit field.
fn validate_period_span_label(period: &str, span: PeriodSpan) -> Result<(), String> {
    let up = period.trim().to_ascii_uppercase();
    let tokens: Vec<&str> = up
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let declared = if tokens.contains(&"YTD") {
        Some(PeriodSpan::YearToDate)
    } else if tokens
        .iter()
        .any(|t| matches!(*t, "Q1" | "Q2" | "Q3" | "Q4"))
    {
        Some(PeriodSpan::Quarter)
    } else if tokens.iter().any(|t| matches!(*t, "H1" | "H2")) {
        Some(PeriodSpan::HalfYear)
    } else if tokens.contains(&"FY")
        || tokens.iter().any(|t| {
            t.len() == 6
                && t.starts_with("FY")
                && t[2..].chars().all(|c| c.is_ascii_digit())
        })
        || (tokens.len() == 1
            && tokens[0].len() == 4
            && tokens[0].chars().all(|c| c.is_ascii_digit()))
    {
        Some(PeriodSpan::FullYear)
    } else {
        None
    };
    match declared {
        Some(declared) if declared != span => Err(format!(
            "period {period:?} declares span {declared:?}, not {span:?}"
        )),
        _ => Ok(()),
    }
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
/// appear in the text (commas stripped; an integer value also matches its
/// decimal render) **at the sign it was stated with** — the magnitude is
/// located at number boundaries and the printed sign read beside it
/// ([`printed_negative`]), so a positive candidate never corroborates off
/// `-41` or the accounting `(41)`, and a negative one never off a bare `41`.
/// Zero is unsigned. Deterministic and deliberately literal — the model may
/// extract a row only from source text that states the value. Shared by the
/// forward-assumption and leading-indicator legs, which inherit the sign rule.
pub(crate) fn value_in_text(value: f64, text: &str) -> bool {
    let haystack: String = text.replace(',', "");
    let len = haystack.len();
    !value_stated_in(value, &haystack, 0..len).is_empty()
}

/// The occurrence search behind [`value_in_text`]: `haystack` is already
/// comma-stripped, and only an occurrence whose digits lie inside `span`
/// counts — while the boundary and the sign are still read from the
/// neighbours outside it, which is how the excerpt leg reads a quoted span
/// with the page's own context around it. Returns every qualifying
/// occurrence's span in text order — a value printed twice binds through
/// whichever occurrence the metric states.
fn value_stated_in(
    value: f64,
    haystack: &str,
    span: std::ops::Range<usize>,
) -> Vec<std::ops::Range<usize>> {
    let magnitude = value.abs();
    // An integer value matches either render ("41" or "41.0") — the boundary
    // rules below would otherwise reject "41" against a printed "41.0".
    let rendered = format!("{magnitude}");
    let needles: Vec<String> = if magnitude.fract() == 0.0 {
        vec![rendered.clone(), format!("{rendered}.0")]
    } else {
        vec![rendered]
    };
    let wants_negative = value < 0.0;
    let unsigned = value == 0.0;
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
    let mut hits = Vec::new();
    for needle in needles {
        if needle.is_empty() {
            continue;
        }
        let mut from = span.start;
        while let Some(pos) = haystack[from..span.end].find(&needle) {
            let start = from + pos;
            let end = start + needle.len();
            if !continues_left(start)
                && !continues_right(end)
                && (unsigned || printed_negative(haystack, start, end) == wants_negative)
            {
                hits.push(start..end);
            }
            from = start + 1;
        }
    }
    hits.sort_by_key(|hit| hit.start);
    hits
}

/// What reading the value through a quoted excerpt found.
enum ExcerptRead {
    /// The excerpt never appears in the page.
    NotInPage,
    /// The excerpt appears, but the value is not stated inside it as the page
    /// prints it.
    ValueNotStated,
    /// The value is stated: the comma-stripped quote, every sign-correct
    /// occurrence's span in it, and where the stripping removed a comma — each
    /// mark the count of retained bytes before it, so a mark strictly inside a
    /// digit run says the page printed that run with a thousands separator.
    Stated {
        quoted: String,
        value_spans: Vec<std::ops::Range<usize>>,
        comma_marks: Vec<usize>,
    },
}

/// How many chars of page context ride on each side of a quoted span when the
/// value is read inside it: enough for a sign through a currency symbol and
/// the range test behind it (`-$41`, `40-45`), a parenthesis pair, a leading
/// digit, and a decimal continuation. The context is cut before commas are
/// stripped, so a comma inside it costs one char of reach — the adjacent digit
/// or sign is still exposed; keep that margin if the constant ever moves.
const EXCERPT_EDGE_CHARS: usize = 4;

/// Whether the value is stated inside the quoted excerpt **as the page prints
/// it**: for each occurrence of the (whitespace-collapsed) excerpt in the
/// (whitespace-collapsed) page, the value is searched within the quoted span
/// only, with [`EXCERPT_EDGE_CHARS`] of the page's own text on either side
/// supplying the boundary and the sign — so a quote trimmed to the digits
/// cannot shed a `-`, a `(`, or a leading digit that sits just outside it,
/// and a number just outside the quote never counts as quoted.
fn value_stated_in_excerpt(value: f64, page: &str, excerpt: &str) -> ExcerptRead {
    if excerpt.is_empty() {
        return ExcerptRead::NotInPage;
    }
    let step = excerpt.chars().next().map_or(1, char::len_utf8);
    let quoted = excerpt.replace(',', "");
    let comma_marks: Vec<usize> = excerpt
        .bytes()
        .scan(0usize, |retained, b| {
            if b == b',' {
                Some(Some(*retained))
            } else {
                *retained += 1;
                Some(None)
            }
        })
        .flatten()
        .collect();
    let mut found = false;
    let mut from = 0;
    while let Some(pos) = page[from..].find(excerpt) {
        found = true;
        let start = from + pos;
        let end = start + excerpt.len();
        let prefix_start = page[..start]
            .char_indices()
            .rev()
            .nth(EXCERPT_EDGE_CHARS - 1)
            .map_or(0, |(i, _)| i);
        let suffix_end = page[end..]
            .char_indices()
            .nth(EXCERPT_EDGE_CHARS)
            .map_or(page.len(), |(i, _)| end + i);
        let prefix = page[prefix_start..start].replace(',', "");
        let suffix = page[end..suffix_end].replace(',', "");
        let window = format!("{prefix}{quoted}{suffix}");
        let span = prefix.len()..prefix.len() + quoted.len();
        let hits = value_stated_in(value, &window, span);
        if !hits.is_empty() {
            return ExcerptRead::Stated {
                value_spans: hits
                    .into_iter()
                    .map(|hit| hit.start - prefix.len()..hit.end - prefix.len())
                    .collect(),
                quoted,
                comma_marks,
            };
        }
        from = start + step;
    }
    if found {
        ExcerptRead::ValueNotStated
    } else {
        ExcerptRead::NotInPage
    }
}

/// Whether the number occupying `haystack[start..end]` is printed negative: a
/// minus sign (ASCII hyphen-minus, U+2212, or U+2013 en dash) hugging the
/// digits whose own left
/// neighbour is neither a digit nor a percent sign nor a closing parenthesis
/// (`of -41` is a sign; `40-45`, `40%-45%`, and a date's `-30` are
/// separators), optionally through one currency symbol (`-$41`), or an
/// accounting parenthesis pair wrapping exactly the number (`(41)`, never
/// `(41 units)`). A spaced sign (`40 - 45`) never reads as a sign, and the
/// left-neighbour guard keeps a hugging en-dash range (`40–45`) positive.
fn printed_negative(haystack: &str, start: usize, end: usize) -> bool {
    let mut before = haystack[..start].chars().rev();
    let mut prev = before.next();
    if matches!(prev, Some('$' | '€' | '£')) {
        prev = before.next();
    }
    match prev {
        Some('-' | '−' | '–') => !before
            .next()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '%' | ')')),
        Some('(') => haystack[end..].starts_with(')'),
        _ => false,
    }
}

/// The metric-family stems a row's excerpt must carry for its declared kind
/// (drafted, calibratable): the corroborated sentence must be *about* the
/// metric the row types, so a revenue sentence can never back a deliveries row.
/// Each stem matches case-insensitively at a word start, so "delivered" /
/// "deliveries" / "delivery" all read `deliver` while "border" never reads
/// `order`.
fn metric_stems(kind: MetricKind) -> &'static [&'static str] {
    match kind {
        MetricKind::Production => &["produc", "output", "manufactur", "built"],
        MetricKind::Deliveries => &["deliver", "shipment", "shipped"],
        // Plural and compound forms only: a bare `order` reads "in order to"
        // and a bare `contract` any legal clause (the slice's Codex round 1).
        MetricKind::Bookings => &[
            "booking",
            "booked",
            "orders",
            "order intake",
            "order value",
            "contracts",
            "contract value",
            "contract wins",
            "signed contract",
        ],
        MetricKind::Backlog => &[
            "backlog",
            "order book",
            "remaining performance",
            "unfilled",
            "unshipped",
        ],
        MetricKind::Reservations => &[
            "reservation",
            "reserved",
            "pre-order",
            "preorder",
            "deposit",
            "waitlist",
        ],
        MetricKind::UnitEconomics => &[
            "margin",
            "per unit",
            "per vehicle",
            "per mile",
            "per customer",
            "per user",
            "unit economics",
            "unit-economics",
            "unit cost",
            "cost per",
            "revenue per",
            "average selling",
            "average revenue",
            "contribution margin",
            "payback",
            "lifetime value",
            "acquisition cost",
        ],
    }
}

/// Every word-start occurrence of the kind's stems in the text (its left
/// neighbour not alphanumeric), case-insensitive; spans index the original.
fn metric_stem_spans(text: &str, kind: MetricKind) -> Vec<std::ops::Range<usize>> {
    let lower = text.to_ascii_lowercase();
    let mut spans = Vec::new();
    for stem in metric_stems(kind) {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(stem) {
            let start = from + pos;
            let word_start = lower[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric());
            if word_start {
                spans.push(start..start + stem.len());
            }
            from = start + stem.len();
        }
    }
    spans
}

/// Whether the excerpt carries metric-family language for the row's kind —
/// the stem table's own pin; the validator reads the spans through
/// [`excerpt_binds_metric`].
#[cfg(test)]
fn excerpt_names_metric(excerpt: &str, kind: MetricKind) -> bool {
    !metric_stem_spans(excerpt, kind).is_empty()
}

/// The number census over a comma-stripped quote: every digit run, a decimal
/// continuation included. A year, a quarter, a percentage, and a date part
/// all count — the contract asks for a quote with one number in it, and a
/// sub-quote without the period token always exists when the period sits
/// outside the metric's clause (the slice's Codex round 3).
fn quoted_numbers(quoted: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = quoted.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        out.push(start..i);
    }
    out
}

/// Why the metric-context binding failed.
enum MetricContext {
    /// The quote carries no stem for the row's kind.
    NoLanguage,
    /// The quote states more than the one fact — its number count.
    ManyNumbers(usize),
    /// The quote's one number is not the value, or a quoted range's endpoint
    /// is not the one the row's role names.
    NotTheValue,
    /// The quote's one number (or its range) is the period the sentence
    /// names, not the value.
    PeriodValue,
}

/// The words that make a following year a period label, never a value —
/// "guidance for 2025", "in 2025", "as of 2025", "by 2025", "through 2025",
/// "fiscal 2025", "FY2025" (drafted, calibratable; ruled 2026-08-29 off the
/// review's I19). Read as the alphabetic run immediately left of the digits,
/// whitespace skipped, so `FY2025` yields `fy`.
const PERIOD_WORDS: &[&str] = &["for", "in", "of", "by", "through", "fiscal", "fy"];

/// Whether the digit run at `span` reads as a calendar year: exactly four
/// digits (no decimal continuation), 1900–2099, and printed without a
/// thousands separator — a comma mark strictly inside the run says the page
/// printed `2,025`, a count and never a year.
fn reads_as_year(quoted: &str, span: &std::ops::Range<usize>, comma_marks: &[usize]) -> bool {
    let run = &quoted[span.clone()];
    run.len() == 4
        && run.bytes().all(|b| b.is_ascii_digit())
        && (1900..=2099).contains(&run.parse::<u32>().unwrap_or(0))
        && !comma_marks.iter().any(|m| span.start < *m && *m < span.end)
}

/// Whether the word immediately before `start` (whitespace skipped, the
/// maximal ASCII-alphabetic run) is one of [`PERIOD_WORDS`].
fn period_word_before(quoted: &str, start: usize) -> bool {
    let before = quoted[..start].trim_end();
    let word_start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphabetic())
        .last()
        .map_or(before.len(), |(i, _)| i);
    let word = before[word_start..].to_ascii_lowercase();
    !word.is_empty() && PERIOD_WORDS.contains(&word.as_str())
}

/// The metric-context binding — the narrow one-fact contract (ruled
/// 2026-08-28 off the slice's Codex round 3, replacing the positional
/// binding of rounds 1 and 2): the quote must carry the row's metric-family
/// language and state **exactly one number**, the row's value at its sign.
/// Every digit run counts, so a year, a quarter, a percentage, or a
/// prior-period figure beside the value rejects the quote and the model must
/// trim to the clause; a sentence that cannot be trimmed loses its row, which
/// is safer than admitting a wrong one. The one carve-out is a guidance
/// range: a guidance-low or guidance-high row may quote two numbers joined
/// by a hyphen, a dash, `to`, or `and`, and its value must be the endpoint
/// its role names. A compound sentence therefore can never lend a stem to a
/// number from another clause. The contract is a syntactic admission filter,
/// never semantic proof: it cannot tell what the one number it admits means.
/// One shape it can tell syntactically is closed here (ruled 2026-08-29 off
/// the review's I19): a value that is itself the period — the one number
/// reads as a 1900–2099 year printed without a thousands separator and sits
/// right after one of [`PERIOD_WORDS`] ("delivery guidance for 2025"), or
/// for a range both endpoints read so and the word precedes the left one
/// ("guidance for 2025-2026") — rejects. A genuine count in that band after
/// such a word ("deliveries of 1950 units") is the accepted loss, an optional
/// row. What remains — a stem with no number of its own beside a competing
/// noun the lexicon does not know ("deliveries and revenue of 41 million") —
/// still passes; the persisted excerpt is the audit for that residual and
/// the run's rejection split calibrates the stem table.
fn excerpt_binds_metric(
    quoted: &str,
    value_spans: &[std::ops::Range<usize>],
    comma_marks: &[usize],
    role: ObservationRole,
    kind: MetricKind,
) -> Result<(), MetricContext> {
    if metric_stem_spans(quoted, kind).is_empty() {
        return Err(MetricContext::NoLanguage);
    }
    let numbers = quoted_numbers(quoted);
    match numbers.as_slice() {
        [only] if value_spans.contains(only) => {
            if reads_as_year(quoted, only, comma_marks) && period_word_before(quoted, only.start) {
                Err(MetricContext::PeriodValue)
            } else {
                Ok(())
            }
        }
        [_] => Err(MetricContext::NotTheValue),
        [left, right] if range_partners(quoted, left, right) => {
            let endpoint = match role {
                ObservationRole::GuidanceLow => left,
                ObservationRole::GuidanceHigh => right,
                _ => return Err(MetricContext::ManyNumbers(2)),
            };
            if !value_spans.contains(endpoint) {
                Err(MetricContext::NotTheValue)
            } else if reads_as_year(quoted, left, comma_marks)
                && reads_as_year(quoted, right, comma_marks)
                && period_word_before(quoted, left.start)
            {
                Err(MetricContext::PeriodValue)
            } else {
                Ok(())
            }
        }
        _ => Err(MetricContext::ManyNumbers(numbers.len())),
    }
}

/// Whether two printed numbers are the ends of one range — joined by a
/// hyphen, a dash, `to`, or `and` (`130-140`, `130 to 140`, `between 130
/// and 140`).
fn range_partners(quoted: &str, a: &std::ops::Range<usize>, b: &std::ops::Range<usize>) -> bool {
    let (l, r) = if a.start <= b.start { (a, b) } else { (b, a) };
    l.end <= r.start && matches!(quoted[l.end..r.start].trim(), "-" | "–" | "—" | "to" | "and")
}

/// Whitespace-run normalization for the verbatim excerpt match: readability
/// extraction and the model's quoting may differ in line breaks and run
/// lengths, never in words, so runs collapse to one space on both sides.
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The holding-identity cross-check: the fetched page must mention the
/// holding — its symbol as a standalone word, or a **distinctive** issuer-name
/// token (generic corporate suffixes like "Holdings" never qualify) — so a
/// cross-issuer citation cannot enter the observation history. The matcher is
/// the shared [`crate::portfolio::text_names_holding`].
fn page_mentions_holding(text: &str, symbol: &str, company_name: Option<&str>) -> bool {
    crate::portfolio::text_names_holding(text, symbol, company_name)
}

/// The activation legs over one row (`docs/portfolio-workflow.md` §Step 6e —
/// the research-loop slice's recorded obligation, discharged here): the row's
/// source page must have been fetched by THIS holding's loop, must mention the
/// holding (identity cross-check), and must state the value (source-text
/// corroboration) — the last read through the row's quoted excerpt: the
/// excerpt must appear verbatim in the page (whitespace-run normalized), the
/// value must appear inside it at its sign, and the excerpt must carry the
/// row's metric-family language, so the number is bound to one sentence about
/// the declared metric rather than to "somewhere on the page".
fn validate_against_source(
    o: &ObservationCandidate,
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
    let excerpt = collapse_whitespace(&o.source_excerpt);
    let page = collapse_whitespace(text);
    let (quoted, value_spans, comma_marks) = match value_stated_in_excerpt(
        o.numeric_value,
        &page,
        &excerpt,
    ) {
        ExcerptRead::NotInPage => {
            return Err(
                "source-text corroboration failed: the quoted excerpt does not appear in the \
                 fetched page"
                    .to_string(),
            );
        }
        ExcerptRead::ValueNotStated => {
            return Err(
                "source-text corroboration failed: the stated value does not appear at its \
                 sign in the quoted excerpt as the page prints it"
                    .to_string(),
            );
        }
        ExcerptRead::Stated {
            quoted,
            value_spans,
            comma_marks,
        } => (quoted, value_spans, comma_marks),
    };
    excerpt_binds_metric(
        &quoted,
        &value_spans,
        &comma_marks,
        o.observation_role,
        o.metric_kind,
    )
    .map_err(|why| match why {
        MetricContext::NoLanguage => format!(
            "metric-context check failed: the quoted excerpt carries no {} language",
            o.metric_kind.as_str()
        ),
        MetricContext::ManyNumbers(count) => format!(
            "metric-context check failed: the quoted excerpt states {count} numbers — a \
             quote states the value and no other number, only a guidance-low or \
             guidance-high row a range's two endpoints"
        ),
        MetricContext::NotTheValue => "metric-context check failed: the quoted excerpt's \
                                       one number is not the stated value, or not the \
                                       range endpoint the row's role names"
            .to_string(),
        MetricContext::PeriodValue => "metric-context check failed: the quoted excerpt's \
                                       one number reads as the period the sentence names \
                                       (a 1900–2099 year after for / in / of / by / \
                                       through / fiscal / FY), not the value"
            .to_string(),
    })?;
    Ok(())
}

/// Validate candidate rows against the typed contract, rejecting with a reason;
/// duplicates of stored history (or of an earlier candidate in the same batch) are
/// rejected rather than silently dropped. Period labels are checked against
/// their typed span, then normalize to the ISO-end-plus-span convention before
/// the dedup key is taken. With `evidence` present the two
/// activation legs run per row; **without it every candidate is rejected** —
/// the producer's rows can only enter through the research loop's lineage.
/// An accepted row is stamped at acceptance with the prompt version whose
/// contract admitted it ([`PreProfitObservation::admitted_under`]); a
/// rejected row is returned as the candidate it was offered as.
pub fn validate_observations(
    candidates: Vec<ObservationCandidate>,
    history: &[PreProfitObservation],
    evidence: Option<&SourceEvidence<'_>>,
) -> (Vec<PreProfitObservation>, Vec<RejectedObservation>) {
    let mut seen: std::collections::BTreeSet<_> =
        history.iter().map(|o| o.dedup_key()).collect();
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for mut candidate in candidates {
        if let Err(reason) = validate_period_span_label(&candidate.period, candidate.period_span) {
            rejected.push(RejectedObservation { observation: candidate, reason });
            continue;
        }
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
                reason: "duplicate of a stored observation (issuer + metric identity + span + \
                         role + period end + source + publication date + value)"
                    .to_string(),
            });
            continue;
        }
        seen.insert(key);
        accepted.push(candidate.admit(crate::portfolio::PROMPT_VERSION));
    }
    (accepted, rejected)
}

/// One row's **structural** validation — the legs checkable from the typed row
/// alone: metric kind, polarity, numeric value, units, period end, reporting
/// span, issuer scope, source URL, publication date, confidence.
///
/// The two once-promised activation legs — the **holding-identity cross-check**
/// and **source-text corroboration** — are live in
/// [`validate_against_source`], run by [`validate_observations`] over the
/// research loop's fetched-page lineage (the research-loop slice discharged
/// the recorded obligation when it connected the producer); an unevidenced
/// call rejects every candidate, so this structural pass alone can never
/// admit a row.
fn validate_observation(o: &ObservationCandidate) -> Result<(), String> {
    if !o.numeric_value.is_finite() {
        return Err("non-finite numeric value".to_string());
    }
    if o.units.trim().is_empty() {
        return Err("missing units".to_string());
    }
    // Validation sees the row AFTER the raw label's span consistency check and
    // `normalize_period` — the documented ISO-end-plus-span convention has
    // teeth only if a period that did not normalize rejects, else two rows
    // sharing fabricated prose could pair into an execution miss.
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
    // The excerpt is a locator, checked against the page only once the row is
    // otherwise well-formed: present, and short enough to be one sentence.
    if o.source_excerpt.trim().is_empty() {
        return Err("missing source excerpt".to_string());
    }
    if o.source_excerpt.trim().chars().count() > SOURCE_EXCERPT_CAP_CHARS {
        return Err(format!(
            "source excerpt exceeds {SOURCE_EXCERPT_CAP_CHARS} characters"
        ));
    }
    // An ISO date (or an RFC 3339 timestamp's date prefix) — a bare non-date
    // string cannot anchor the observation's publication, nor take a vintage
    // at pairing (the same parse, shared with the execution read).
    if published_date(&o.published_at).is_none() {
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

/// Merge accepted rows into the period-end-and-span-keyed history, sorted for stable persistence
/// (comparison identity including span, then period descending, then role).
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

/// The calendar date a row was published: the ISO date prefix of
/// `published_at` (an RFC 3339 timestamp's date part), **parsed** rather than
/// compared as a string, so `2026-05-01` and `2026-05-01T09:00:00Z` read as
/// one day. `None` for an undatable row — impossible past validation, so the
/// read fails closed on it rather than panicking.
fn published_date(published_at: &str) -> Option<chrono::NaiveDate> {
    published_at
        .trim()
        .get(..10)
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
}

/// A guidance row as the pairing reads it.
#[derive(Clone, Copy)]
struct GuidanceRow {
    value: f64,
    range_low: bool,
    published: chrono::NaiveDate,
    confidence: f64,
}

/// An actual as the pairing reads it.
#[derive(Clone, Copy)]
struct ActualRow {
    value: f64,
    published: chrono::NaiveDate,
    confidence: f64,
}

/// The binding guidance for one identity + period under the **guidance
/// vintage policy** (`docs/portfolio-analysis.md` §Starting parameters): only
/// an ex-ante row is admissible — published on or before the period end and
/// strictly before the period's earliest actual, so a results release can
/// never supply its own bound and a post-period preview never binds — and
/// among those the **latest revision** binds, a range low over point guidance
/// at the same date, then the higher confidence. A residual tie between
/// different values is a conflict and yields nothing: the period is not
/// comparable rather than bound by persistence order.
fn select_bound(
    period_end: chrono::NaiveDate,
    earliest_actual: chrono::NaiveDate,
    rows: &[GuidanceRow],
) -> Option<GuidanceRow> {
    let mut admissible: Vec<&GuidanceRow> = rows
        .iter()
        .filter(|g| g.published <= period_end && g.published < earliest_actual)
        .collect();
    admissible.sort_by(|a, b| {
        b.published
            .cmp(&a.published)
            .then_with(|| b.range_low.cmp(&a.range_low))
            .then_with(|| b.confidence.total_cmp(&a.confidence))
    });
    let first = *admissible.first()?;
    let conflict = admissible
        .iter()
        .skip(1)
        .take_while(|g| {
            g.published == first.published
                && g.range_low == first.range_low
                && g.confidence.total_cmp(&first.confidence).is_eq()
        })
        .any(|g| g.value != first.value);
    (!conflict).then_some(*first)
}

/// The actual for one identity + period: the highest confidence, then the
/// latest publication (a restatement over the release it restates); a
/// residual tie between different values is a conflict and yields nothing.
fn select_actual(rows: &[ActualRow]) -> Option<ActualRow> {
    let mut ordered: Vec<&ActualRow> = rows.iter().collect();
    ordered.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| b.published.cmp(&a.published))
    });
    let first = *ordered.first()?;
    let conflict = ordered
        .iter()
        .skip(1)
        .take_while(|a| {
            a.confidence.total_cmp(&first.confidence).is_eq() && a.published == first.published
        })
        .any(|a| a.value != first.value);
    (!conflict).then_some(*first)
}

/// The guidance-attainment read: pair actuals against guidance lower bounds per
/// metric-and-span identity and period end under the guidance vintage policy
/// ([`select_bound`], [`select_actual`]), compute miss ratios, and derive the
/// repeated / material states over each identity's latest four comparable
/// periods.
pub fn execution_read(observations: &[PreProfitObservation]) -> ExecutionRead {
    use std::collections::BTreeMap;

    // identity (including span) → period end → every candidate row:
    // deterministic iteration via BTreeMap, the selection per period a pure
    // function of the candidates.
    // Only higher-is-better rows enter (the rule's polarity guard); the bound
    // is the stated low for a range, the stated value for point guidance.
    type Key = ObservationIdentity;
    let mut bounds: BTreeMap<Key, BTreeMap<String, Vec<GuidanceRow>>> = BTreeMap::new();
    let mut actuals: BTreeMap<Key, BTreeMap<String, Vec<ActualRow>>> = BTreeMap::new();

    for o in observations {
        if o.polarity != ObservationPolarity::HigherIsBetter
            || o.period_span == PeriodSpan::Unknown
        {
            continue;
        }
        // An undatable row cannot take a vintage, so it never pairs.
        let Some(published) = published_date(&o.published_at) else {
            continue;
        };
        let key = o.identity();
        let period = o.period.trim().to_string();
        match o.observation_role {
            ObservationRole::GuidanceLow | ObservationRole::PointGuidance => {
                bounds
                    .entry(key)
                    .or_default()
                    .entry(period)
                    .or_default()
                    .push(GuidanceRow {
                        value: o.numeric_value,
                        range_low: o.observation_role == ObservationRole::GuidanceLow,
                        published,
                        confidence: o.confidence,
                    });
            }
            ObservationRole::Actual => {
                actuals
                    .entry(key)
                    .or_default()
                    .entry(period)
                    .or_default()
                    .push(ActualRow {
                        value: o.numeric_value,
                        published,
                        confidence: o.confidence,
                    });
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
        let mut comparable: Vec<(&String, GuidanceRow, ActualRow)> = period_bounds
            .iter()
            .filter_map(|(period, guidance)| {
                let reports = period_actuals.get(period)?;
                // The period end anchors the ex-ante leg; a period that never
                // normalized (impossible past validation) fails closed here.
                let period_end = chrono::NaiveDate::parse_from_str(period, "%Y-%m-%d").ok()?;
                // Ex ante is measured against the FIRST time the actual became
                // public, not the actual selected — a restatement's later date
                // must not readmit a release's restated guidance.
                let earliest_actual = reports.iter().map(|a| a.published).min()?;
                let actual = select_actual(reports)?;
                let bound = select_bound(period_end, earliest_actual, guidance)?;
                (bound.value.is_finite() && bound.value > 0.0).then_some((period, bound, actual))
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
            let miss_ratio = (bound.value - actual.value) / bound.value;
            // Finite legs, unbounded difference and quotient: an overflowed
            // ratio is no miss — it would persist as `null` on a required
            // float (Codex I16). The period stays counted as comparable.
            if !miss_ratio.is_finite() {
                continue;
            }
            if at_least(miss_ratio, EXECUTION_MISS_RATIO) {
                missed_periods += 1;
                read.misses.push(ExecutionMiss {
                    metric_kind: identity_kind(&key.0),
                    units: key.1.clone(),
                    issuer_scope: key.2.clone(),
                    period: (*period).clone(),
                    period_span: key.3,
                    miss_ratio,
                    bound_published_at: bound.published.format("%Y-%m-%d").to_string(),
                    actual_published_at: actual.published.format("%Y-%m-%d").to_string(),
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

    /// A well-formed row. The period normalizes to its ISO period end (the
    /// history only ever holds normalized periods), and the publication date
    /// is role-aware under the guidance vintage policy (Codex I4) — a guidance
    /// row dated sixty days before its period end, every other role thirty
    /// days after — so a fixture's guidance and actual pair by construction; a
    /// period that does not normalize keeps a fixed date.
    fn observation(
        kind: MetricKind,
        role: ObservationRole,
        value: f64,
        period: &str,
    ) -> ObservationCandidate {
        let period = normalize_period(period);
        let published_at = chrono::NaiveDate::parse_from_str(&period, "%Y-%m-%d")
            .ok()
            .map(|end| {
                let days = match role {
                    ObservationRole::GuidanceLow
                    | ObservationRole::GuidanceHigh
                    | ObservationRole::PointGuidance => -60,
                    ObservationRole::Actual | ObservationRole::ContextualLevel => 30,
                };
                (end + chrono::Duration::days(days))
                    .format("%Y-%m-%d")
                    .to_string()
            })
            .unwrap_or_else(|| "2026-08-01".to_string());
        ObservationCandidate {
            metric_kind: kind,
            observation_role: role,
            polarity: ObservationPolarity::HigherIsBetter,
            numeric_value: value,
            units: "units".into(),
            period,
            period_span: PeriodSpan::Quarter,
            issuer_scope: "company".into(),
            source_url: "https://example.com/report".into(),
            source_excerpt: format!("reported {} of {value} units", kind.as_str()),
            published_at,
            confidence: 0.9,
        }
    }

    /// The candidate re-dated — the vintage tests' one knob.
    fn dated(mut o: ObservationCandidate, published_at: &str) -> ObservationCandidate {
        o.published_at = published_at.into();
        o
    }

    /// An admitted row re-dated — the same knob on a history row.
    fn redated(mut o: PreProfitObservation, published_at: &str) -> PreProfitObservation {
        o.published_at = published_at.into();
        o
    }

    /// The candidate admitted into a history fixture under the current prompt
    /// stamp — what acceptance writes (Codex I20).
    fn admitted(o: ObservationCandidate) -> PreProfitObservation {
        o.admit(crate::portfolio::PROMPT_VERSION)
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
        // One sentence per value, so each row's quoted excerpt ("reported
        // deliveries of N units") is a verbatim substring of the page.
        let stated = values
            .iter()
            .map(|v| format!("ACME Motors (NASDAQ: ACME) reported deliveries of {v} units this period."))
            .collect::<Vec<_>>()
            .join(" ");
        let mut texts = std::collections::HashMap::new();
        texts.insert("https://example.com/report".to_string(), stated);
        texts
    }

    #[test]
    fn validation_rejects_malformed_rows_with_reasons() {
        let good = observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q2");
        let bad_value = ObservationCandidate {
            numeric_value: f64::NAN,
            ..good.clone()
        };
        let bad_source = ObservationCandidate {
            source_url: "not a url".into(),
            ..good.clone()
        };
        let bad_polarity = ObservationCandidate {
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
            admitted(observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-06-30"));
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
    fn a_same_source_revision_is_not_a_duplicate_and_the_vintage_read_selects_it() {
        // Codex I4, round 1: the dedup key once read identity + role + period +
        // source, so an issuer page updated with revised guidance re-offered
        // the old key and the revision was rejected — the vintage policy never
        // saw it. Through the production validator: stored guidance 100 from
        // the page in January, the same page offering 90 in May, the actual 88
        // in July — the revision enters and binds (2.2%, in-line), in either
        // candidate order.
        let mut prior = compute_overlay(&burning_stock(), None, vec![]);
        prior.observations.push(admitted(dated(
            observation(MetricKind::Deliveries, ObservationRole::PointGuidance, 100.0, "2026-06-30"),
            "2026-01-15",
        )));
        let revised = dated(
            observation(MetricKind::Deliveries, ObservationRole::PointGuidance, 90.0, "2026-06-30"),
            "2026-05-10",
        );
        let actual = dated(
            observation(MetricKind::Deliveries, ObservationRole::Actual, 88.0, "2026-06-30"),
            "2026-07-25",
        );
        let texts = evidence_texts(&[90.0, 88.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        for candidates in [
            vec![revised.clone(), actual.clone()],
            vec![actual.clone(), revised.clone()],
        ] {
            let refined = compute_overlay_with_sources(
                &burning_stock(),
                Some(&prior),
                candidates,
                Some(&evidence),
            );
            assert!(refined.rejected.is_empty(), "{:?}", refined.rejected);
            assert_eq!(refined.observations.len(), 3);
            assert_eq!(refined.execution.comparable_periods, 1);
            assert!(refined.execution.misses.is_empty(), "{:?}", refined.execution.misses);
        }
        // The exact fact re-offered — the same page, date, and value — is
        // still the duplicate the key exists to stop.
        let refined = compute_overlay_with_sources(
            &burning_stock(),
            Some(&prior),
            vec![revised.clone(), revised, actual],
            Some(&evidence),
        );
        assert_eq!(refined.rejected.len(), 1);
        assert!(refined.rejected[0].reason.contains("duplicate"));
        assert_eq!(refined.observations.len(), 3);
    }

    #[test]
    fn a_same_page_conflict_enters_the_history_and_drops_the_period() {
        // Codex I4, round 1: one page offering two values for the same
        // guidance on one date once collapsed to the first row seen; now both
        // enter, the read finds the same-vintage conflict, and the period is
        // not comparable — both rows persisted for the audit.
        let guide = |value: f64| {
            dated(
                observation(MetricKind::Deliveries, ObservationRole::PointGuidance, value, "2026-06-30"),
                "2026-05-01",
            )
        };
        let actual = dated(
            observation(MetricKind::Deliveries, ObservationRole::Actual, 90.0, "2026-06-30"),
            "2026-07-25",
        );
        let texts = evidence_texts(&[100.0, 110.0, 90.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let refined = compute_overlay_with_sources(
            &burning_stock(),
            None,
            vec![guide(100.0), guide(110.0), actual],
            Some(&evidence),
        );
        assert!(refined.rejected.is_empty(), "{:?}", refined.rejected);
        assert_eq!(refined.observations.len(), 3);
        assert_eq!(refined.execution.comparable_periods, 0);
        assert!(refined.execution.misses.is_empty());
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

        // A page that mentions the holding but never printed the quoted
        // sentence fails corroboration at the excerpt leg.
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
        assert!(rejected[0].reason.contains("quoted excerpt does not appear"));

        // A digit-substring never corroborates: 41 must not match inside 141,
        // even when the quoted sentence is genuinely on the page.
        let short = ObservationCandidate {
            source_excerpt: "reported deliveries of 141 units".into(),
            ..observation(MetricKind::Deliveries, ObservationRole::Actual, 41.0, "2026-Q2")
        };
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "$ACME reported deliveries of 141 units this period.".to_string(),
        );
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let (_, rejected) = validate_observations(vec![short], &[], Some(&evidence));
        assert!(rejected[0].reason.contains("does not appear at its sign in the quoted excerpt"));

        // Comma-separated renderings still corroborate (12,000 states 12000).
        let big = ObservationCandidate {
            source_excerpt: "reported deliveries of 12,000 units".into(),
            ..observation(MetricKind::Deliveries, ObservationRole::Actual, 12000.0, "2026-Q2")
        };
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "$ACME reported deliveries of 12,000 units this period.".to_string(),
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
    fn the_excerpt_is_a_bounded_locator_checked_before_the_page() {
        let good = observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q2");
        let missing = ObservationCandidate {
            source_excerpt: "  ".into(),
            ..good.clone()
        };
        let over_cap = ObservationCandidate {
            source_excerpt: "x".repeat(SOURCE_EXCERPT_CAP_CHARS + 1),
            ..good.clone()
        };
        let texts = evidence_texts(&[100.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let (accepted, rejected) =
            validate_observations(vec![good, missing, over_cap], &[], Some(&evidence));
        assert_eq!(accepted.len(), 1);
        assert_eq!(rejected.len(), 2);
        assert!(rejected.iter().any(|r| r.reason == "missing source excerpt"));
        assert!(rejected.iter().any(|r| r.reason.contains("exceeds")));
    }

    #[test]
    fn the_excerpt_binds_the_value_to_one_sentence_about_the_metric() {
        // The review's I3 case: a release states 41 in a revenue sentence and
        // 141 in the deliveries sentence. A deliveries-41 row can ride neither
        // — the revenue sentence carries no deliveries language, the
        // deliveries sentence never states 41 — and the page as a whole no
        // longer vouches for a number found "somewhere on it".
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "$ACME reported revenue of 41 million.\nDeliveries   rose to 141 units in the \
             quarter, while gross margin was (12)%."
                .to_string(),
        );
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let row = |value: f64, excerpt: &str| ObservationCandidate {
            source_excerpt: excerpt.into(),
            ..observation(MetricKind::Deliveries, ObservationRole::Actual, value, "2026-Q2")
        };
        // The revenue sentence quoted: the value is there, the metric is not.
        let (_, rejected) = validate_observations(
            vec![row(41.0, "reported revenue of 41 million")],
            &[],
            Some(&evidence),
        );
        assert!(
            rejected[0].reason.contains("no deliveries language"),
            "{}",
            rejected[0].reason
        );
        // The deliveries sentence quoted: the metric is there, 41 is not.
        let (_, rejected) = validate_observations(
            vec![row(41.0, "Deliveries rose to 141 units")],
            &[],
            Some(&evidence),
        );
        assert!(
            rejected[0].reason.contains("at its sign in the quoted excerpt"),
            "{}",
            rejected[0].reason
        );
        // A sentence the page never printed rejects before either leg.
        let (_, rejected) = validate_observations(
            vec![row(41.0, "Deliveries rose to 41 units")],
            &[],
            Some(&evidence),
        );
        assert!(
            rejected[0].reason.contains("quoted excerpt does not appear"),
            "{}",
            rejected[0].reason
        );
        // The honest row: the deliveries sentence with its own number, matched
        // across the page's whitespace run and line break.
        let (accepted, rejected) = validate_observations(
            vec![row(141.0, "Deliveries rose to 141 units in the quarter")],
            &[],
            Some(&evidence),
        );
        assert_eq!(accepted.len(), 1, "{rejected:?}");
        // A positive unit-economics row cannot ride the accounting-negative
        // print; the negative row it actually states does.
        let margin = ObservationCandidate {
            source_excerpt: "gross margin was (12)%".into(),
            ..observation(MetricKind::UnitEconomics, ObservationRole::Actual, 12.0, "2026-Q2")
        };
        let (_, rejected) = validate_observations(vec![margin.clone()], &[], Some(&evidence));
        assert!(
            rejected[0].reason.contains("at its sign"),
            "{}",
            rejected[0].reason
        );
        let negative = ObservationCandidate {
            numeric_value: -12.0,
            ..margin
        };
        let (accepted, rejected) = validate_observations(vec![negative], &[], Some(&evidence));
        assert_eq!(accepted.len(), 1, "{rejected:?}");
    }

    #[test]
    fn the_excerpt_edge_reads_the_page_neighbours() {
        // The reviewer's round-1 cases: a quote trimmed to the digits must not
        // shed the sign, the parenthesis, or the leading digit that sits just
        // outside it on the page — the value is read inside the quoted span
        // with the page's own neighbours, and a number just outside the quote
        // never counts as quoted.
        let page = |text: &str| {
            let mut texts = std::collections::HashMap::new();
            texts.insert("https://example.com/report".to_string(), text.to_string());
            texts
        };
        let row = |kind: MetricKind, value: f64, excerpt: &str| ObservationCandidate {
            source_excerpt: excerpt.into(),
            ..observation(kind, ObservationRole::Actual, value, "2026-Q2")
        };
        let run = |texts: &std::collections::HashMap<String, String>,
                   candidate: ObservationCandidate| {
            let evidence = SourceEvidence {
                texts,
                symbol: "ACME",
                company_name: None,
            };
            validate_observations(vec![candidate], &[], Some(&evidence))
        };
        // The sign just outside a digit-leading quote.
        let texts = page("$ACME booked a loss of -41 million in deliveries revenue.");
        let (accepted, rejected) =
            run(&texts, row(MetricKind::Deliveries, 41.0, "41 million in deliveries"));
        assert!(accepted.is_empty());
        assert!(rejected[0].reason.contains("at its sign"), "{}", rejected[0].reason);
        // The leading digit just outside it.
        let texts = page("$ACME deliveries: 141 units delivered this quarter.");
        let (accepted, rejected) =
            run(&texts, row(MetricKind::Deliveries, 41.0, "41 units delivered"));
        assert!(accepted.is_empty());
        assert!(rejected[0].reason.contains("at its sign"), "{}", rejected[0].reason);
        // The opening parenthesis just outside it: the positive row rejects,
        // the negative row the page states passes.
        let texts = page("$ACME margin: (12)% margin on deliveries this quarter.");
        let (accepted, rejected) =
            run(&texts, row(MetricKind::UnitEconomics, 12.0, "12)% margin on deliveries"));
        assert!(accepted.is_empty());
        assert!(rejected[0].reason.contains("at its sign"), "{}", rejected[0].reason);
        let (accepted, rejected) =
            run(&texts, row(MetricKind::UnitEconomics, -12.0, "12)% margin on deliveries"));
        assert_eq!(accepted.len(), 1, "{rejected:?}");
        // A number just outside the quoted span is not quoted.
        let texts = page("$ACME 41 deliveries rose sharply.");
        let (accepted, rejected) =
            run(&texts, row(MetricKind::Deliveries, 41.0, "deliveries rose sharply"));
        assert!(accepted.is_empty());
        assert!(rejected[0].reason.contains("at its sign"), "{}", rejected[0].reason);
        // A legitimate digit-leading quote still passes.
        let texts = page("$ACME said: 41 units delivered in Q2.");
        let (accepted, rejected) =
            run(&texts, row(MetricKind::Deliveries, 41.0, "41 units delivered"));
        assert_eq!(accepted.len(), 1, "{rejected:?}");
    }

    #[test]
    fn the_quote_states_one_number_the_metric_owns() {
        // The narrow one-fact contract (Codex round 3, replacing the positional
        // binding of rounds 1 and 2): a quote carries the metric stem and
        // exactly one number, the row's value at its sign — every digit run
        // counts — so a compound sentence rejects whichever way round it is
        // and the model must trim to the clause; a guidance-low / -high row
        // alone may quote a range's two endpoints.
        let page = |text: &str| {
            let mut texts = std::collections::HashMap::new();
            texts.insert("https://example.com/report".to_string(), text.to_string());
            texts
        };
        let row = |kind: MetricKind, role: ObservationRole, value: f64, excerpt: &str| {
            ObservationCandidate {
                source_excerpt: excerpt.into(),
                ..observation(kind, role, value, "2026-Q2")
            }
        };
        let run = |texts: &std::collections::HashMap<String, String>,
                   candidate: ObservationCandidate| {
            let evidence = SourceEvidence {
                texts,
                symbol: "ACME",
                company_name: None,
            };
            validate_observations(vec![candidate], &[], Some(&evidence))
        };
        let actual = ObservationRole::Actual;
        let deliveries = MetricKind::Deliveries;
        // Compound sentences reject in both orderings and for every number
        // they print; the trimmed clause passes.
        for sentence in [
            "revenue was 41 million while deliveries reached 141 units",
            "deliveries reached 141 units, while revenue was 41 million",
            "deliveries increased 25%, while revenue was 41 million",
            "Revenue was 41 million while deliveries reached 2,025 units",
            "revenue was 41 million while deliveries were 41 units",
        ] {
            let texts = page(&format!("$ACME said {sentence}."));
            for value in [41.0, 141.0, 2025.0, 25.0] {
                let (accepted, rejected) = run(&texts, row(deliveries, actual, value, sentence));
                assert!(accepted.is_empty(), "{sentence} / {value}: {accepted:?}");
                if !rejected[0].reason.contains("does not appear") {
                    assert!(
                        rejected[0].reason.contains("states 2 numbers"),
                        "{sentence} / {value}: {}",
                        rejected[0].reason
                    );
                }
            }
        }
        let texts = page("$ACME said revenue was 41 million while deliveries reached 141 units.");
        let (accepted, rejected) =
            run(&texts, row(deliveries, actual, 141.0, "deliveries reached 141 units"));
        assert_eq!(accepted.len(), 1, "{rejected:?}");
        let (accepted, rejected) =
            run(&texts, row(deliveries, actual, 41.0, "revenue was 41 million"));
        assert!(accepted.is_empty());
        assert!(
            rejected[0].reason.contains("no deliveries language"),
            "{}",
            rejected[0].reason
        );
        let texts = page("$ACME: Revenue was 41 million while deliveries reached 2,025 units.");
        let (accepted, rejected) =
            run(&texts, row(deliveries, actual, 2025.0, "deliveries reached 2,025 units"));
        assert_eq!(accepted.len(), 1, "{rejected:?}");
        // Every digit run counts: a period token, a percentage, or a prior
        // print beside the value rejects the quote; the clause without them
        // passes, and a sentence that cannot be trimmed loses its row.
        for (sentence, clause) in [
            ("In Q2 2026, deliveries reached 141 units", "deliveries reached 141 units"),
            ("Deliveries were 141 units, up from 120", "Deliveries were 141 units"),
            ("141 units delivered in Q2", "141 units delivered"),
            ("FY26 deliveries reached 141 units on 2026-06-30", "deliveries reached 141 units"),
        ] {
            let texts = page(&format!("$ACME: {sentence}."));
            let (accepted, _) = run(&texts, row(deliveries, actual, 141.0, sentence));
            assert!(accepted.is_empty(), "{sentence}");
            let (accepted, rejected) = run(&texts, row(deliveries, actual, 141.0, clause));
            assert_eq!(accepted.len(), 1, "{clause}: {rejected:?}");
        }
        let texts = page("$ACME: deliveries rose 12% to 141 units.");
        let (accepted, rejected) =
            run(&texts, row(deliveries, actual, 141.0, "deliveries rose 12% to 141 units"));
        assert!(accepted.is_empty());
        assert!(
            rejected[0].reason.contains("states 2 numbers"),
            "{}",
            rejected[0].reason
        );
        // A guidance range: both endpoints bind to the role that names them,
        // hyphenated or worded; the wrong role, or an actual, rejects.
        for sentence in [
            "guided deliveries of 130-140 units",
            "guided deliveries of between 130 and 140 units",
            "guided deliveries of 130 to 140 units",
        ] {
            let texts = page(&format!("$ACME {sentence}."));
            let (accepted, rejected) =
                run(&texts, row(deliveries, ObservationRole::GuidanceLow, 130.0, sentence));
            assert_eq!(accepted.len(), 1, "{sentence}: {rejected:?}");
            let (accepted, rejected) =
                run(&texts, row(deliveries, ObservationRole::GuidanceHigh, 140.0, sentence));
            assert_eq!(accepted.len(), 1, "{sentence}: {rejected:?}");
            let (accepted, rejected) =
                run(&texts, row(deliveries, ObservationRole::GuidanceHigh, 130.0, sentence));
            assert!(accepted.is_empty(), "{sentence}");
            assert!(
                rejected[0].reason.contains("not the stated value"),
                "{}",
                rejected[0].reason
            );
            let (accepted, _) = run(&texts, row(deliveries, actual, 130.0, sentence));
            assert!(accepted.is_empty(), "{sentence}");
        }
        // The lexicon case: "in order to" is not bookings language.
        let sentence = "revenue was 41 million in order to fund expansion";
        let texts = page(&format!("$ACME said {sentence}."));
        let (accepted, rejected) = run(&texts, row(MetricKind::Bookings, actual, 41.0, sentence));
        assert!(accepted.is_empty());
        assert!(
            rejected[0].reason.contains("no bookings language"),
            "{}",
            rejected[0].reason
        );
    }

    #[test]
    fn the_value_that_is_the_period_rejects() {
        // The one syntactic shape of the one-fact residual closed by ruling
        // (the review's I19, 2026-08-29): the quote's one number reads as a
        // 1900–2099 year printed without a thousands separator and sits right
        // after for / in / of / by / through / fiscal / FY — so it is the
        // period the sentence names, never the value. A range rejects when
        // both endpoints read so and the word precedes the left one. A
        // genuine count in the band after such a word is the accepted loss,
        // pinned as a choice; a comma-bearing run, a non-year, or a value
        // after any other word admits.
        let page = |text: &str| {
            let mut texts = std::collections::HashMap::new();
            texts.insert("https://example.com/report".to_string(), text.to_string());
            texts
        };
        let row = |kind: MetricKind, role: ObservationRole, value: f64, excerpt: &str| {
            ObservationCandidate {
                source_excerpt: excerpt.into(),
                ..observation(kind, role, value, "2026-Q2")
            }
        };
        let run = |texts: &std::collections::HashMap<String, String>,
                   candidate: ObservationCandidate| {
            let evidence = SourceEvidence {
                texts,
                symbol: "ACME",
                company_name: None,
            };
            validate_observations(vec![candidate], &[], Some(&evidence))
        };
        let actual = ObservationRole::Actual;
        let deliveries = MetricKind::Deliveries;
        for (sentence, value) in [
            ("delivery guidance for 2025", 2025.0),
            ("deliveries in 2025", 2025.0),
            ("deliveries as of 2025", 2025.0),
            ("deliveries by 2025", 2025.0),
            ("deliveries through 2025", 2025.0),
            ("deliveries in fiscal 2025", 2025.0),
            ("FY2025 deliveries", 2025.0),
            ("FY 1999 deliveries", 1999.0),
            // A comma hugging the run from outside sits at the span's end,
            // never inside it, so it exempts nothing (reviewer round 1).
            ("in 2025, deliveries", 2025.0),
            // The accepted loss: a real count in the year band after `of`.
            ("deliveries of 1950 units", 1950.0),
        ] {
            let texts = page(&format!("$ACME reported {sentence}."));
            let (accepted, rejected) = run(&texts, row(deliveries, actual, value, sentence));
            assert!(accepted.is_empty(), "{sentence}: {accepted:?}");
            assert!(
                rejected[0].reason.contains("reads as the period"),
                "{sentence}: {}",
                rejected[0].reason
            );
        }
        // A period range rejects for both roles; the word before the left
        // endpoint governs the right one, whose own neighbour is the dash.
        for sentence in ["delivery guidance for 2025-2026", "delivery guidance for 2025 to 2026"] {
            let texts = page(&format!("$ACME issued {sentence}."));
            for (role, value) in [
                (ObservationRole::GuidanceLow, 2025.0),
                (ObservationRole::GuidanceHigh, 2026.0),
            ] {
                let (accepted, rejected) = run(&texts, row(deliveries, role, value, sentence));
                assert!(accepted.is_empty(), "{sentence} / {value}: {accepted:?}");
                assert!(
                    rejected[0].reason.contains("reads as the period"),
                    "{sentence} / {value}: {}",
                    rejected[0].reason
                );
            }
        }
        // Admitted: a value after any other word, a non-year, a comma-bearing
        // run (a year never prints with a thousands separator), and a range
        // whose left endpoint is no year.
        for (sentence, role, value) in [
            ("delivered 2025 vehicles", actual, 2025.0),
            ("deliveries for the year reached 2025", actual, 2025.0),
            ("delivery guidance for 41000", ObservationRole::GuidanceLow, 41000.0),
            ("delivery guidance for 2100", ObservationRole::GuidanceLow, 2100.0),
            ("delivery guidance for 1899", ObservationRole::GuidanceLow, 1899.0),
            ("delivery guidance for 2025.5", ObservationRole::GuidanceLow, 2025.5),
            ("a total of 2,025 units delivered", actual, 2025.0),
            ("delivery guidance for 2,025 units", ObservationRole::GuidanceLow, 2025.0),
            ("delivery guidance for 1500-2025 units", ObservationRole::GuidanceHigh, 2025.0),
            ("delivery guidance for 1,950-2,025 units", ObservationRole::GuidanceLow, 1950.0),
        ] {
            let texts = page(&format!("$ACME said {sentence}."));
            let (accepted, rejected) = run(&texts, row(deliveries, role, value, sentence));
            assert_eq!(accepted.len(), 1, "{sentence}: {rejected:?}");
        }
    }

    #[test]
    fn metric_stems_match_at_word_starts_only() {
        assert!(excerpt_names_metric("reported deliveries of 41 units", MetricKind::Deliveries));
        assert!(excerpt_names_metric("Delivered 41 vehicles", MetricKind::Deliveries));
        assert!(excerpt_names_metric("orders rose to 41", MetricKind::Bookings));
        assert!(excerpt_names_metric("order intake of 41", MetricKind::Bookings));
        assert!(!excerpt_names_metric("the border crossing handled 41", MetricKind::Bookings));
        // A bare `order` or `contract` is not bookings language (Codex round 1).
        assert!(!excerpt_names_metric(
            "revenue was 41 million in order to fund expansion",
            MetricKind::Bookings
        ));
        assert!(!excerpt_names_metric("under the contract, revenue was 41", MetricKind::Bookings));
        assert!(!excerpt_names_metric("revenue of 41 million", MetricKind::Deliveries));
        assert!(excerpt_names_metric("gross margin of -4.1%", MetricKind::UnitEconomics));
        assert!(excerpt_names_metric("backlog stood at 41", MetricKind::Backlog));
        assert!(excerpt_names_metric("reservations reached 41", MetricKind::Reservations));
        assert!(excerpt_names_metric("produced 41 units", MetricKind::Production));
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
        assert!(value_in_text(41.0, "delivered 41.0 units"));
        assert!(!value_in_text(41.0, "delivered 41.05 units"));
        // A large integral render keeps every trailing zero. It must never
        // collapse to the bare leading digit while building the search needle.
        assert!(value_in_text(
            2e15,
            "backlog reached 2000000000000000 units"
        ));
        assert!(!value_in_text(2e15, "backlog reached 2 units"));
    }

    #[test]
    fn corroboration_reads_the_printed_sign() {
        // A positive candidate never corroborates off a negative print — a
        // hugging minus (ASCII, U+2212, or U+2013), through a currency symbol, or the
        // accounting parenthesis pair — and a negative candidate never off a
        // bare or `+` print.
        assert!(!value_in_text(41.0, "(41)"));
        assert!(!value_in_text(41.0, "a loss of -41 million"));
        assert!(!value_in_text(41.0, "a loss of −41 million"));
        assert!(!value_in_text(41.0, "a loss of –41 million"));
        assert!(!value_in_text(41.0, "a loss of -$41 million"));
        assert!(value_in_text(-41.0, "(41)"));
        assert!(value_in_text(-41.0, "(41.0)"));
        assert!(value_in_text(-41.0, "a loss of -41 million"));
        assert!(value_in_text(-41.0, "a loss of −41 million"));
        assert!(value_in_text(-41.0, "a loss of –41 million"));
        assert!(value_in_text(-41.0, "a loss of -$41 million"));
        assert!(!value_in_text(-41.0, "delivered 41 units"));
        assert!(!value_in_text(-41.0, "delivered +41 units"));
        assert!(value_in_text(41.0, "delivered +41 units"));
        // A hyphen or en dash between digits is a range or date separator,
        // never a sign; a spaced sign does not hug the upper endpoint either.
        assert!(value_in_text(45.0, "guided to 40-45 units"));
        assert!(!value_in_text(-45.0, "guided to 40-45 units"));
        assert!(value_in_text(45.0, "guided to 40 - 45 units"));
        assert!(value_in_text(45.0, "guided to 40–45 units"));
        assert!(!value_in_text(-45.0, "guided to 40–45 units"));
        assert!(value_in_text(30.0, "the quarter ended 2026-06-30"));
        // A minus after a percent sign or a closing parenthesis is a range
        // separator too, never a sign on the upper endpoint.
        assert!(value_in_text(45.0, "guided to 40%-45% growth"));
        assert!(!value_in_text(-45.0, "guided to 40%-45% growth"));
        assert!(value_in_text(45.0, "a (40)-45 swing"));
        // Parentheses wrapping more than the number are prose, not a sign.
        assert!(value_in_text(41.0, "(41 units)"));
        assert!(!value_in_text(-41.0, "(41 units)"));
        // Zero is unsigned.
        assert!(value_in_text(0.0, "backlog of (0)"));
        assert!(value_in_text(0.0, "backlog of 0"));
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
        // pair into an execution miss — the ISO-end-plus-span rule has teeth.
        // The fixture pre-normalizes, so the raw spelling is put back to prove
        // validation's own normalization end to end.
        let mut good = observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q2");
        good.period = "2026-Q2".into();
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "$ACME reported deliveries of 100 units this period.".to_string(),
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
    fn observation_admission_rejects_a_span_label_conflict_and_dedups_with_span() {
        let mut texts = std::collections::HashMap::new();
        texts.insert(
            "https://example.com/report".to_string(),
            "$ACME reported deliveries of 100 units this period.".to_string(),
        );
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let mut conflict =
            observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q4");
        conflict.period = "Q4 2026".into();
        conflict.period_span = PeriodSpan::FullYear;
        let (accepted, rejected) =
            validate_observations(vec![conflict], &[], Some(&evidence));
        assert!(accepted.is_empty());
        assert!(rejected[0].reason.contains("declares span Quarter"));

        // An ISO end does not imply a duration. Two otherwise identical rows
        // with different declared spans are distinct facts in the history.
        let quarter = observation(
            MetricKind::Deliveries,
            ObservationRole::Actual,
            100.0,
            "2026-12-31",
        );
        let annual = ObservationCandidate {
            period_span: PeriodSpan::FullYear,
            ..quarter.clone()
        };
        let (accepted, rejected) =
            validate_observations(vec![quarter, annual], &[], Some(&evidence));
        assert_eq!(accepted.len(), 2);
        assert!(rejected.is_empty());
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

    #[test]
    fn explicit_period_labels_must_agree_with_the_typed_span() {
        assert!(validate_period_span_label("Q2 2026", PeriodSpan::Quarter).is_ok());
        assert!(validate_period_span_label("H1 2026", PeriodSpan::HalfYear).is_ok());
        assert!(validate_period_span_label("FY2026", PeriodSpan::FullYear).is_ok());
        assert!(validate_period_span_label("YTD Q2 2026", PeriodSpan::YearToDate).is_ok());
        assert!(validate_period_span_label("2026-06-30", PeriodSpan::PointInTime).is_ok());
        let err = validate_period_span_label("Q4 2026", PeriodSpan::FullYear).unwrap_err();
        assert!(err.contains("declares span Quarter"), "{err}");
        assert!(validate_period_span_label("FY2026", PeriodSpan::Unknown).is_err());
    }

    #[test]
    fn unlike_period_spans_never_pair_even_when_the_end_date_matches() {
        let annual_guide = PreProfitObservation {
            period_span: PeriodSpan::FullYear,
            ..admitted(observation(
                MetricKind::Deliveries,
                ObservationRole::GuidanceLow,
                500_000.0,
                "2026-Q4",
            ))
        };
        let q4_actual = admitted(observation(
            MetricKind::Deliveries,
            ObservationRole::Actual,
            140_000.0,
            "2026-Q4",
        ));
        let read = execution_read(&[annual_guide.clone(), q4_actual.clone()]);
        assert_eq!(read.comparable_periods, 0);
        assert!(read.misses.is_empty());

        let half_guide = PreProfitObservation {
            period_span: PeriodSpan::HalfYear,
            ..annual_guide.clone()
        };
        assert_eq!(
            execution_read(&[half_guide, q4_actual.clone()]).comparable_periods,
            0
        );

        let annual_actual = PreProfitObservation {
            period_span: PeriodSpan::FullYear,
            ..q4_actual.clone()
        };
        let read = execution_read(&[annual_guide.clone(), annual_actual]);
        assert_eq!(read.comparable_periods, 1);
        assert_eq!(read.misses.len(), 1);
        assert_eq!(read.misses[0].period_span, PeriodSpan::FullYear);

        let unknown_actual = PreProfitObservation {
            period_span: PeriodSpan::Unknown,
            ..q4_actual
        };
        assert_eq!(
            execution_read(&[annual_guide, unknown_actual]).comparable_periods,
            0,
            "an unknown span stays audit context rather than pairing"
        );
    }

    /// Guidance/actual pairs across four periods for one identity.
    fn guided_history(pairs: &[(&str, f64, f64)]) -> Vec<PreProfitObservation> {
        pairs
            .iter()
            .flat_map(|(period, bound, actual)| {
                vec![
                    admitted(observation(MetricKind::Deliveries, ObservationRole::GuidanceLow, *bound, period)),
                    admitted(observation(MetricKind::Deliveries, ObservationRole::Actual, *actual, period)),
                ]
            })
            .collect()
    }

    #[test]
    fn an_overflowing_miss_ratio_is_no_miss_and_the_period_stays_comparable() {
        // Codex I16 (ruled 2026-08-29): finite legs, unbounded quotient — a
        // vanishing bound beside a large negative actual overflows the ratio,
        // which would have persisted as `null` on the miss's required float.
        let history = guided_history(&[("2026-Q2", 1e-300, -1e10)]);
        let read = execution_read(&history);
        assert_eq!(read.comparable_periods, 1);
        assert!(read.misses.is_empty(), "{:?}", read.misses);
        assert!(!read.material_single_miss);
        assert!(!read.repeated_miss);
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
        // The miss records the vintages it was read from (Codex I4): the
        // fixture's guidance sits sixty days before the 2026-06-30 period end,
        // its actual thirty days after.
        assert_eq!(read.misses[0].period, "2026-06-30");
        assert_eq!(read.misses[0].bound_published_at, "2026-05-01");
        assert_eq!(read.misses[0].actual_published_at, "2026-07-30");
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
            admitted(observation(MetricKind::Bookings, ObservationRole::GuidanceLow, 200.0, "2026-Q2")),
            admitted(observation(MetricKind::Bookings, ObservationRole::Actual, 180.0, "2026-Q2")),
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
            admitted(observation(MetricKind::Deliveries, ObservationRole::PointGuidance, 100.0, "2026-Q2")),
            admitted(observation(MetricKind::Deliveries, ObservationRole::Actual, 90.0, "2026-Q2")),
        ];
        let read = execution_read(&history);
        assert_eq!(read.misses.len(), 1, "point guidance supplies the bound");

        // A stated range low (95) displaces the point bound (100): 90 vs 95 → ~5.3%.
        history.push(admitted(observation(
            MetricKind::Deliveries,
            ObservationRole::GuidanceLow,
            95.0,
            "2026-Q2",
        )));
        let read = execution_read(&history);
        assert_eq!(read.misses.len(), 1);
        assert!((read.misses[0].miss_ratio - (5.0 / 95.0)).abs() < 1e-12);
    }

    // ---- Guidance vintage (Codex I4) ----

    /// A deliveries row for the 2026-06-30 period, re-dated — the vintage
    /// tests' one fixture.
    fn q2(role: ObservationRole, value: f64, published_at: &str) -> PreProfitObservation {
        admitted(dated(
            observation(MetricKind::Deliveries, role, value, "2026-06-30"),
            published_at,
        ))
    }

    #[test]
    fn a_results_release_never_supplies_its_own_guidance() {
        // The finding's case: a results release restating the period's
        // guidance beside the actual is dated the same day, so the guidance is
        // retrospective and the pair never forms — one page can never supply
        // both sides of its own attainment test.
        let history = vec![
            q2(ObservationRole::PointGuidance, 100.0, "2026-07-25"),
            q2(ObservationRole::Actual, 80.0, "2026-07-25"),
        ];
        let read = execution_read(&history);
        assert_eq!(read.comparable_periods, 0);
        assert!(read.misses.is_empty());
        assert!(!read.material_single_miss);
    }

    #[test]
    fn the_latest_ex_ante_revision_binds_in_either_order() {
        // Original guidance 100 in January, revised to 90 in May, actual 88 in
        // July: the standing guidance at results time binds, so the 2.2%
        // shortfall is in-line; under the original it would be a 12% miss.
        let original = q2(ObservationRole::PointGuidance, 100.0, "2026-01-15");
        let revised = q2(ObservationRole::PointGuidance, 90.0, "2026-05-10");
        let actual = q2(ObservationRole::Actual, 88.0, "2026-07-25");
        for history in [
            vec![original.clone(), revised.clone(), actual.clone()],
            vec![actual.clone(), revised.clone(), original.clone()],
        ] {
            let read = execution_read(&history);
            assert_eq!(read.comparable_periods, 1);
            assert!(read.misses.is_empty(), "{:?}", read.misses);
        }
        // Without the revision the original binds and the period misses.
        let read = execution_read(&[original, actual]);
        assert_eq!(read.misses.len(), 1);
        assert!((read.misses[0].miss_ratio - 0.12).abs() < 1e-12);
        assert_eq!(read.misses[0].bound_published_at, "2026-01-15");
        assert_eq!(read.misses[0].actual_published_at, "2026-07-25");
    }

    #[test]
    fn guidance_after_the_period_end_is_a_preview_not_a_promise() {
        // A post-period pre-announcement typed as guidance never binds; a row
        // dated on the period end itself is still ex ante.
        let actual = q2(ObservationRole::Actual, 90.0, "2026-07-25");
        let preview = q2(ObservationRole::PointGuidance, 100.0, "2026-07-03");
        let read = execution_read(&[preview, actual.clone()]);
        assert_eq!(read.comparable_periods, 0);
        let on_the_end = q2(ObservationRole::PointGuidance, 100.0, "2026-06-30");
        let read = execution_read(&[on_the_end, actual]);
        assert_eq!(read.comparable_periods, 1);
        assert_eq!(read.misses.len(), 1);
    }

    #[test]
    fn guidance_dated_on_the_first_actual_is_retrospective_even_under_a_restatement() {
        // The press release (low confidence) and a later 10-Q restatement
        // (high confidence, the one selected): guidance dated on the press
        // release is retrospective against the period's EARLIEST actual, not
        // the actual selected.
        let release = PreProfitObservation {
            confidence: 0.6,
            ..q2(ObservationRole::Actual, 90.0, "2026-07-25")
        };
        let restated = q2(ObservationRole::Actual, 91.0, "2026-08-10");
        let retrospective = q2(ObservationRole::PointGuidance, 100.0, "2026-07-25");
        let read = execution_read(&[release.clone(), restated.clone(), retrospective]);
        assert_eq!(read.comparable_periods, 0);
        // Ex-ante guidance pairs with the selected (restated) actual.
        let ex_ante = q2(ObservationRole::PointGuidance, 100.0, "2026-06-01");
        let read = execution_read(&[release, restated, ex_ante]);
        assert_eq!(read.comparable_periods, 1);
        assert_eq!(read.misses.len(), 1);
        assert!((read.misses[0].miss_ratio - 0.09).abs() < 1e-12);
        assert_eq!(read.misses[0].bound_published_at, "2026-06-01");
        assert_eq!(read.misses[0].actual_published_at, "2026-08-10");
    }

    #[test]
    fn vintage_beats_role_and_range_low_wins_only_at_the_same_date() {
        let actual = q2(ObservationRole::Actual, 90.0, "2026-07-25");
        let range_low = q2(ObservationRole::GuidanceLow, 95.0, "2026-05-01");
        let point = q2(ObservationRole::PointGuidance, 100.0, "2026-05-01");
        let read = execution_read(&[point.clone(), range_low.clone(), actual.clone()]);
        assert!(
            (read.misses[0].miss_ratio - (5.0 / 95.0)).abs() < 1e-12,
            "range low over point at one date"
        );
        // A later point guidance displaces the earlier range low.
        let later_point = q2(ObservationRole::PointGuidance, 98.0, "2026-06-01");
        let read = execution_read(&[point, range_low, later_point, actual]);
        assert!(
            (read.misses[0].miss_ratio - (8.0 / 98.0)).abs() < 1e-12,
            "{:?}",
            read.misses
        );
        assert_eq!(read.misses[0].bound_published_at, "2026-06-01");
    }

    #[test]
    fn a_same_vintage_conflict_makes_the_period_not_comparable_on_either_side() {
        let actual = q2(ObservationRole::Actual, 90.0, "2026-07-25");
        let guide = |value: f64, confidence: f64| PreProfitObservation {
            confidence,
            ..q2(ObservationRole::PointGuidance, value, "2026-05-01")
        };
        // Same date, role, and confidence with different values: a conflict.
        let read = execution_read(&[guide(100.0, 0.9), guide(110.0, 0.9), actual.clone()]);
        assert_eq!(read.comparable_periods, 0);
        // The same value twice (two sources) is no conflict.
        let read = execution_read(&[guide(100.0, 0.9), guide(100.0, 0.9), actual.clone()]);
        assert_eq!(read.comparable_periods, 1);
        // Confidence breaks the tie before it becomes a conflict.
        let read = execution_read(&[guide(100.0, 0.9), guide(110.0, 0.8), actual.clone()]);
        assert_eq!(read.comparable_periods, 1);
        assert!((read.misses[0].miss_ratio - 0.10).abs() < 1e-12);
        // The actual side under the same rule.
        let guidance = guide(100.0, 0.9);
        let report = |value: f64| q2(ObservationRole::Actual, value, "2026-07-25");
        let read = execution_read(&[guidance.clone(), report(90.0), report(95.0)]);
        assert_eq!(read.comparable_periods, 0);
        let read = execution_read(&[guidance, report(90.0), report(90.0)]);
        assert_eq!(read.comparable_periods, 1);
    }

    #[test]
    fn publication_dates_compare_as_dates_never_as_strings() {
        // A timestamp form on the period end is still on the period end; two
        // actuals on one day in two forms tie on the date (and conflict on
        // value) rather than the longer string winning.
        let guidance = q2(ObservationRole::PointGuidance, 100.0, "2026-06-30T23:00:00Z");
        let actual = q2(ObservationRole::Actual, 90.0, "2026-07-25");
        let read = execution_read(&[guidance.clone(), actual.clone()]);
        assert_eq!(read.comparable_periods, 1);
        assert_eq!(read.misses[0].bound_published_at, "2026-06-30");
        let timestamped = q2(ObservationRole::Actual, 95.0, "2026-07-25T09:00:00Z");
        let read = execution_read(&[guidance, actual, timestamped]);
        assert_eq!(
            read.comparable_periods,
            0,
            "a same-day value conflict, never a string order"
        );
    }

    #[test]
    fn an_undatable_row_or_period_never_pairs_and_never_panics() {
        let actual = q2(ObservationRole::Actual, 90.0, "2026-07-25");
        let guidance = q2(ObservationRole::PointGuidance, 100.0, "2026-05-01");
        let read = execution_read(&[redated(guidance.clone(), "recently"), actual.clone()]);
        assert_eq!(read.comparable_periods, 0);
        let read = execution_read(&[guidance.clone(), redated(actual.clone(), "recently")]);
        assert_eq!(read.comparable_periods, 0);
        // A period that never normalized (impossible past validation) cannot
        // anchor the period-end leg, so the pair fails closed.
        let prose = |mut o: PreProfitObservation| {
            o.period = "thirteen weeks ended".into();
            o
        };
        let read = execution_read(&[prose(guidance), prose(actual)]);
        assert_eq!(read.comparable_periods, 0);
    }

    #[test]
    fn the_overlay_stamp_is_pre_profit_v4() {
        // Span-aware comparison changes what a persisted execution read means,
        // so the stamp moves and the resume gate refuses a v3 trail.
        assert_eq!(PRE_PROFIT_PARAMETER_VERSION, "pre-profit-v4");
        let overlay = compute_overlay(&burning_stock(), None, vec![]);
        assert_eq!(overlay.parameter_version, "pre-profit-v4");
    }

    #[test]
    fn lower_is_better_rows_never_enter_the_miss_rule() {
        let mut o = admitted(observation(MetricKind::UnitEconomics, ObservationRole::GuidanceLow, 100.0, "2026-Q2"));
        o.polarity = ObservationPolarity::LowerIsBetter;
        let mut a = admitted(observation(MetricKind::UnitEconomics, ObservationRole::Actual, 150.0, "2026-Q2"));
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
                .map(|p| admitted(observation(MetricKind::Deliveries, role, 100.0, p)))
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
        // Four annual bounds cannot be discharged by four quarterly actuals
        // ending on the same dates: span is part of the comparison identity.
        let annual_bounds: Vec<_> = rows(ObservationRole::GuidanceLow, &periods)
            .into_iter()
            .map(|o| PreProfitObservation {
                period_span: PeriodSpan::FullYear,
                ..o
            })
            .collect();
        let mut unlike_spans = annual_bounds.clone();
        unlike_spans.extend(actuals.clone());
        assert!(backfill_required(&with(unlike_spans), Some(&base)));
        let annual_actuals: Vec<_> = actuals
            .clone()
            .into_iter()
            .map(|o| PreProfitObservation {
                period_span: PeriodSpan::FullYear,
                ..o
            })
            .collect();
        let mut annual_pairs = annual_bounds;
        annual_pairs.extend(annual_actuals);
        assert!(!backfill_required(&with(annual_pairs), Some(&base)));
        // A never-guided metric carries no obligation.
        assert!(!backfill_required(&with(actuals), Some(&base)));
        // A covered identity never discharges a thin one.
        let bookings = |role| admitted(observation(MetricKind::Bookings, role, 50.0, "2026-Q2"));
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
    fn a_backfill_attempt_requires_its_reporting_span_on_the_wire() {
        let mut value = serde_json::json!({
            "metric_kind": "deliveries",
            "units": "vehicles",
            "issuer_scope": "company",
            "period_span": "full-year",
            "checked_periods": ["2025-12-31", "2024-12-31"],
            "sources": ["https://example.com/report"],
            "coverage": "partial"
        });
        let decoded: BackfillAttempt = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(decoded.period_span, PeriodSpan::FullYear);
        value
            .as_object_mut()
            .unwrap()
            .remove("period_span")
            .unwrap();
        assert!(serde_json::from_value::<BackfillAttempt>(value).is_err());
    }

    #[test]
    fn overlay_round_trips_through_json() {
        let overlay = compute_overlay(&burning_stock(), None, vec![]);
        let json = serde_json::to_string(&overlay).expect("serialize");
        let back: PreProfitOverlay = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(overlay, back);
        // An overlay carrying an accepted and a rejected row round-trips both
        // rows' source excerpts through its JSON (Codex round 1).
        let texts = evidence_texts(&[100.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let rejected = ObservationCandidate {
            source_excerpt: "a sentence the page never printed 90".into(),
            ..observation(MetricKind::Deliveries, ObservationRole::Actual, 90.0, "2026-Q1")
        };
        let overlay = compute_overlay_with_sources(
            &burning_stock(),
            None,
            vec![
                observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q2"),
                rejected,
            ],
            Some(&evidence),
        );
        assert_eq!(overlay.observations.len(), 1);
        assert_eq!(overlay.rejected.len(), 1);
        let json = serde_json::to_string(&overlay).expect("serialize");
        let back: PreProfitOverlay = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(overlay, back);
        assert_eq!(back.observations[0].source_excerpt, "reported deliveries of 100 units");
        assert_eq!(back.observations[0].admitted_under, crate::portfolio::PROMPT_VERSION);
        assert_eq!(
            back.rejected[0].observation.source_excerpt,
            "a sentence the page never printed 90"
        );
        // The stamp is required on the wire (Codex I20): a persisted row
        // without it fails to decode rather than reading as anything.
        let mut value: serde_json::Value = serde_json::from_str(&json).expect("json");
        value["observations"][0]
            .as_object_mut()
            .expect("row object")
            .remove("admitted_under")
            .expect("the stamp was written");
        assert!(serde_json::from_value::<PreProfitOverlay>(value).is_err());
        // The span is equally required: silently reading an old row as an
        // arbitrary duration would recreate the cross-span pairing bug.
        let mut value: serde_json::Value = serde_json::from_str(&json).expect("json");
        value["observations"][0]
            .as_object_mut()
            .expect("row object")
            .remove("period_span")
            .expect("the span was written");
        assert!(serde_json::from_value::<PreProfitOverlay>(value).is_err());
    }

    #[test]
    fn an_accepted_row_is_stamped_with_the_prompt_version_at_acceptance() {
        // Codex I20: the app writes the admission stamp on acceptance — the
        // candidate type has no stamp field — and a rejected row goes back as
        // the candidate it was offered as.
        let texts = evidence_texts(&[100.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let good = observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q2");
        let bad = ObservationCandidate {
            source_excerpt: "a sentence the page never printed 90".into(),
            ..observation(MetricKind::Deliveries, ObservationRole::Actual, 90.0, "2026-Q1")
        };
        let (accepted, rejected) =
            validate_observations(vec![good.clone(), bad.clone()], &[], Some(&evidence));
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].admitted_under, crate::portfolio::PROMPT_VERSION);
        assert_eq!(accepted[0], good.admit(crate::portfolio::PROMPT_VERSION));
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].observation, bad);
    }

    #[test]
    fn a_carried_row_keeps_its_own_stamp_and_is_never_re_admitted() {
        // Codex I20, attribute-never-re-filter: a history row admitted under an
        // earlier contract rides the merge with its own stamp beside a fresh
        // acceptance's, and the same fact re-offered under the current contract
        // is a duplicate — the stored row keeps its first stamp.
        let texts = evidence_texts(&[100.0, 110.0]);
        let evidence = SourceEvidence {
            texts: &texts,
            symbol: "ACME",
            company_name: None,
        };
        let earlier = observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q1")
            .admit("portfolio-v17");
        let mut prior = compute_overlay(&burning_stock(), None, vec![]);
        prior.observations.push(earlier.clone());
        let overlay = compute_overlay_with_sources(
            &burning_stock(),
            Some(&prior),
            vec![
                observation(MetricKind::Deliveries, ObservationRole::Actual, 100.0, "2026-Q1"),
                observation(MetricKind::Deliveries, ObservationRole::Actual, 110.0, "2026-Q2"),
            ],
            Some(&evidence),
        );
        let stamps: Vec<(&str, &str)> = overlay
            .observations
            .iter()
            .map(|o| (o.period.as_str(), o.admitted_under.as_str()))
            .collect();
        let (q1, q2) = (normalize_period("2026-Q1"), normalize_period("2026-Q2"));
        assert_eq!(
            stamps,
            vec![
                (q2.as_str(), crate::portfolio::PROMPT_VERSION),
                (q1.as_str(), "portfolio-v17"),
            ]
        );
        assert!(overlay.observations.contains(&earlier), "the first admission stands");
        assert_eq!(overlay.rejected.len(), 1);
        assert!(
            overlay.rejected[0].reason.contains("duplicate"),
            "{}",
            overlay.rejected[0].reason
        );
    }
}
