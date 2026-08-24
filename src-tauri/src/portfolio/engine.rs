//! The deterministic financial-analysis engine (`docs/portfolio-analysis.md` §The
//! per-holding pipeline, step 2; `docs/local-models.md §Context-memory discipline` —
//! "Compute, don't guess"). Every **engine-arm** number in a holding's verdict
//! originates here: the four sub-scores, the composite grade they roll up to, the
//! scenario price targets with their methodology, the options-activity signal, and
//! the mechanical stand-ins. The engine never guesses — a missing input becomes a
//! gap, never a fabricated level. Since `portfolio-v7` the model authors its own
//! arm's numbers beside these (sub-scores, target bands — typed model-authored),
//! and model-arm judgment values never alter or bind the engine baseline (the
//! boundary statement: `docs/portfolio-analysis.md` §The holding verdict).
//!
//! All formulas are simple, bounded, and **calibratable** — the grade-weight
//! formula, the risk-tier thresholds, and the options-signal parameters are the
//! constants this slice deliberately leaves open to shadow-tune against live runs
//! rather than pinning (the durable plan-time parameters live in
//! [`crate::portfolio`]). They are gathered at the top of the module so the
//! calibration surface is one place.

use serde::{Deserialize, Serialize};

use crate::portfolio::{
    Action, Conviction, EngineView, Grade, HorizonOutlook, HorizonRead, OptionsSignal,
    PriceTarget, PriceTargets, SubScores,
};
use crate::schwab::{OptionChain, OptionKind};

// ---- Calibration surface (NOT pinned — shadow-tune against live runs) ---------

/// Composite-grade weights over the **letter** sub-scores (`docs/portfolio-analysis.md`
/// §Starting parameters — the settled ex-momentum re-weight: momentum is computed
/// alongside but lives **outside the letter**, re-homed to the market-setup read in
/// the conviction / positioning context, because a price move is the one grade input
/// that isn't a business fundamental). `grade_from_subscores` divides by their sum,
/// so they need not total 1.0. A sub-score that could not be computed is imputed to
/// the neutral midpoint (50) by `analyze` before the roll-up, so a missing input
/// pulls the composite toward neutral rather than being dropped — an
/// impute-to-neutral, not a renormalization over the present sub-scores.
const W_QUALITY: f64 = 0.40;
const W_VALUATION: f64 = 0.30;
const W_RISK: f64 = 0.30;

/// Composite-score cutoffs for each letter grade (0–100, higher better).
const GRADE_A: f64 = 85.0;
const GRADE_B: f64 = 70.0;
const GRADE_C: f64 = 55.0;
const GRADE_D: f64 = 40.0;

/// Evidence floor: the minimum number of computable **letter** sub-scores
/// (quality / valuation / risk — momentum is context, not a letter input) below
/// which the holding abstains rather than grading on too little
/// (`docs/portfolio-analysis.md` §Evidence floor).
const MIN_SUBSCORES_FOR_GRADE: usize = 2;

// -- Sub-score normalization bands (`docs/portfolio-analysis.md` §Starting
//    parameters — clamped linear maps, each `(lo, hi)` pair mapping lo → 0 and
//    hi → 100 through `scale`; an inverted pair scores lower inputs higher).
//    Shadow-tuned against run `3b21ae85` (the first calibration dataset) —
//    the tune is versioned by [`GRADE_PARAMETER_VERSION`].

/// Quality: net margin 0 → 0, 30%+ → 100.
const QUALITY_NET_MARGIN_BAND: (f64, f64) = (0.0, 0.30);
/// Quality: gross margin 15% → 0, 65%+ → 100.
const QUALITY_GROSS_MARGIN_BAND: (f64, f64) = (0.15, 0.65);
/// Valuation: P/E 70+ → 0, 12 → 100 (inverted — cheaper is better). The v1
/// (40, 10) band read normal large-cap-growth multiples as junk-expensive —
/// the 2026-07-31 run's whole-book C/D/F compression (F4).
const VALUATION_PE_BAND: (f64, f64) = (70.0, 12.0);
/// Valuation: the fixed low score for a non-positive P/E (a loss-maker is never
/// "cheap", never off the scale).
const VALUATION_NEGATIVE_PE_SCORE: f64 = 20.0;
/// Valuation: P/S 25+ → 0, 2 → 100 (inverted).
const VALUATION_PS_BAND: (f64, f64) = (25.0, 2.0);
/// Valuation: P/B 30+ → 0, 2 → 100 (inverted).
const VALUATION_PB_BAND: (f64, f64) = (30.0, 2.0);
/// Momentum (context, outside the letter): trailing return −30% → 0, +30% → 100.
const MOMENTUM_TRAILING_RETURN_BAND: (f64, f64) = (-0.30, 0.30);
/// Risk: per-period (daily) realized volatility 4.5%+ → 0, 0.5% → 100 (inverted —
/// calmer is safer).
const RISK_VOLATILITY_BAND: (f64, f64) = (0.045, 0.005);
/// Risk: debt/equity 2.5×+ → 0, unlevered → 100 (inverted). A **negative**
/// debt/equity (negative equity — levered beyond the equity base) scores 0, the
/// mirror of the negative-P/E rule: the inverted clamp would otherwise read it as
/// maximally safe (`risk_score`).
const RISK_DEBT_EQUITY_BAND: (f64, f64) = (2.5, 0.0);

/// The grade-band parameter version, stamped on each run's audit
/// (`HoldingAudit.grade_parameter_version`) so a band recalibration — letters
/// moving with no input change — is recognizable to the what-changed audit and
/// outcome-learning cohorts. v2 (the 2026-08-03 shadow-tune against run
/// `3b21ae85`, certified v1-exact first): the recentered-growth bands above plus
/// the negative-D/E → 0 guard; runs decoding `None` predate the stamp and carry
/// the v1 bands.
// v2.1 (2026-08-05, piece-3 walk): the dossier's P/E derive went signed, making
// the negative-P/E fixed-score guard reachable for loss-makers — an input-
// semantics change that can move letters, so it is stamped as its own version
// even though every band, weight, and cutoff is unchanged.
pub const GRADE_PARAMETER_VERSION: &str = "grade-v2.1";

/// Fallback one-month scenario half-band (fraction of the base target) when
/// realized volatility can't be computed. The twelve-month band needs no fallback
/// under v2 — its bear/bull ARE the scenario prices.
const ONE_MONTH_FALLBACK_BAND: f64 = 0.05;

// -- v2 rate-anchored scenario-target function (`docs/portfolio-analysis.md`
//    §Starting parameters — the settled shape; every rate/return is a decimal ratio).

/// The trailing anchor window the scenario multiple re-anchors over, in quarters.
const ANCHOR_WINDOW_QUARTERS: usize = 12;

/// Fewer admissible anchor observations than this drops the rate correction entirely
/// (raw multiple percentiles, direct-mapped, recorded).
const MIN_ANCHOR_OBSERVATIONS: usize = 8;

/// A scenario whose `spread_s + DGS10_now` falls below this guard (a degenerate
/// reciprocal denominator) falls back to its raw multiple percentile, recorded.
const DEGENERATE_DENOMINATOR_EPS: f64 = 0.01;

/// Filing grace applied when a quarterly statement carries no filing date: the anchor
/// date is the period end plus this many days (the suite's freshness-basis constant).
const FILING_GRACE_DAYS: i64 = 45;

/// Sanity clamp on the implied annual driver growth versus the trailing print
/// (the v1 bound, retained by the v2 ladder). Released under the targets-v4
/// trough signature — see [`CLAMP_RELEASE_MIN_CONSENSUS_ROWS`].
const DRIVER_GROWTH_MIN: f64 = -0.25;
const DRIVER_GROWTH_MAX: f64 = 0.35;

/// Anchor-multiple sanity bound (targets-v4): an anchor observation whose raw
/// multiple exceeds the holding's **current** trailing multiple by more than this
/// factor is dropped before the percentile surfaces. A trailing-multiple anchor
/// exists to describe a multiple regime the market actually paid; one several
/// times richer than today's own multiple inside a ~3-year window is a
/// denominator artifact (a near-zero-EPS quarter — attempt 2's RKT anchored at
/// 420×/1223×/2511× against a 90× current multiple) or a departed valuation
/// regime (LCID's bubble-era P/S band), not reversion evidence. Relative rather
/// than absolute so a genuinely extreme regime the market still pays (TSLA at
/// 351× against 66–297× anchors) keeps its history. Drafted, calibratable
/// (`docs/verification/2026-08-13-big-run-attempt-2.md` §Workstream 3).
const ANCHOR_MULTIPLE_SANITY_FACTOR: f64 = 3.0;

/// Corroboration floor for the targets-v4 **trough clamp release**: with at least
/// this many forward consensus rows **contributing to the selected rung's
/// blended mid** (the per-field counts on [`ConsensusEstimate`], not
/// `periods_used`) AND the current
/// trailing multiple sitting above the anchor window's own rich-end (P75) raw
/// multiple AND the trailing print itself depressed against the window's
/// demonstrated earning power ([`TROUGH_PRINT_FRACTION`]) — a recent earnings
/// trough against a normal-multiple history, where price held while the trailing
/// print collapsed — the growth clamp is released and the corroborated consensus
/// prices unclamped. Without the release the clamp compresses valid recovery
/// consensus to trough scale (attempt 2's GM: a $14.35 consensus crushed to a
/// $2.67 driver → a −79% "base"). The gates deliberately exclude whole-window
/// troughs (LUV), downward re-ratings (CRM), and price-driven multiple expansion
/// with earnings intact (the rich-multiple rally — Codex round 1, finding 1),
/// where releasing would remove the sanity bound exactly when valuation is
/// stretched. Drafted, calibratable.
const CLAMP_RELEASE_MIN_CONSENSUS_ROWS: u8 = 2;

/// The release's direct trough test: the trailing print must sit below this
/// fraction of the anchor window's **largest** admissible trailing print — the
/// earning power the issuer has actually demonstrated inside the ~3-year window.
/// A multiple can sit above its own history two ways (earnings collapsed, or
/// price rallied); only the first is a trough, and only the print itself can
/// tell them apart. Drafted, calibratable.
const TROUGH_PRINT_FRACTION: f64 = 0.67;

/// Minimum scenario half-spread on the price axis (decimal, around the base target):
/// the dispersion floor `spread_anchored_scenarios` widens a too-tight bear/bull band
/// to, volatility-scaled between these bounds (`docs/portfolio-analysis.md` §Starting
/// parameters). Zero scenario dispersion degenerates the three-state hurdle into a
/// point test, so an artificially flat surface (near-realized consensus, the
/// current-multiple carry) reads as false certainty — recorded when it binds.
const DISPERSION_FLOOR_MIN: f64 = 0.05;
const DISPERSION_FLOOR_MAX: f64 = 0.20;
/// Scale from annualized realized volatility to the half-spread floor.
const DISPERSION_FLOOR_VOL_SCALE: f64 = 0.5;

/// The scenario-target function's parameter version, stamped on each run's audit so
/// target calibration never mixes bases (`docs/portfolio-analysis.md` §Outcome
/// learning). v3: the NTM consensus blend, the dispersion floor, and the recorded
/// clamp-collapse — run `3b21ae85` is the v2 baseline. v4: the anchor-multiple
/// sanity bound and the trough clamp release, drafted against attempt 2's
/// persisted dataset (run `6a52f1dd` is the v3 baseline;
/// `docs/verification/2026-08-13-big-run-attempt-2.md` §Workstream 3).
pub const SCENARIO_TARGET_PARAMETER_VERSION: &str = "targets-v4";

// -- Risk tiers and the capital-efficiency hurdle (`docs/portfolio-analysis.md`
//    §Starting parameters).

/// Tier-scaled hurdle premium over the run-level `DGS2` (decimal ratios).
const TIER_PREMIUM_LOW: f64 = 0.03;
const TIER_PREMIUM_MEDIUM: f64 = 0.05;
const TIER_PREMIUM_HIGH: f64 = 0.08;

/// Stock tier legs (Trade Opportunities' canonical constants — `docs/trade-opportunities.md`
/// §Starting parameters; Portfolio adopts the rule under its stated missing-input rule).
const TIER_HIGH_MAX_MCAP: f64 = 2.0e9;
const TIER_LOW_MIN_MCAP: f64 = 10.0e9;
const TIER_HIGH_MIN_ANNUAL_VOL: f64 = 0.40;
const TIER_LOW_MAX_ANNUAL_VOL: f64 = 0.25;
const TIER_HIGH_MIN_DEBT_EQUITY: f64 = 2.0;
const TIER_LOW_MAX_DEBT_EQUITY: f64 = 1.0;
const TIER_HIGH_MIN_DRAWDOWN: f64 = 0.50;

/// Loose annualization for the per-period (daily) realized volatility the engine
/// computes, used by the tier legs (√252 for daily bars).
const ANNUALIZATION_FACTOR: f64 = 15.87;

// -- Thesis-ledger persistence semantics (`docs/portfolio-analysis.md` §The position
//    thesis ledger; the suite's shared condition-identity contract's drafted
//    constants — `docs/trade-opportunities.md §The opportunity`): the required
//    consecutive distinct breaching observations before a breach confirms, by the
//    series' cadence. A filing print is the period's settled observation, so the
//    first qualifying breach confirms immediately (the materiality margin is its
//    noise guard); a high-frequency series needs two, so a single noisy print logs
//    a quiet first-breach note only.

/// Required consecutive breaching observations for a market-data-cadence series.
pub const LEDGER_CONSECUTIVE_MARKET_DATA: u32 = 2;
/// Required consecutive breaching observations for a filing-cadence series.
pub const LEDGER_CONSECUTIVE_FILING: u32 = 1;

// ---- Inputs ------------------------------------------------------------------

/// One dated observation (an ISO `YYYY-MM-DD` date and a value) — daily closes, rate
/// prints, sector-P/E samples. Dates sort lexicographically in this form, which the
/// latest-on-or-before joins rely on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DatedValue {
    pub date: String,
    pub value: f64,
}

/// The run-level rate anchors (`docs/portfolio-workflow.md` §Step 5): the `DGS2` and
/// `DGS10` prints plus the dated `DGS10` anchor-window history the v2 percentile join
/// reads. All values are **decimal ratios** (`4.5%` → `0.045` — the suite's shared
/// rate representation, normalized at the adapter seam).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RateAnchors {
    pub dgs2: f64,
    pub dgs10: f64,
    /// The prints' observation dates (ISO), where the source carried them — the
    /// as-of dates the persisted rate cache records for the quick paths' fail-soft
    /// (`docs/portfolio-analysis.md` §Starting parameters, rate-cache max age).
    /// `None` on fixtures and pre-field runs (`#[serde(default)]`).
    #[serde(default)]
    pub dgs2_date: Option<String>,
    #[serde(default)]
    pub dgs10_date: Option<String>,
    /// Dated `DGS10` observations covering the trailing anchor window plus alignment
    /// slack, sorted oldest-first.
    pub dgs10_history: Vec<DatedValue>,
    /// A degraded-input note when the anchor-window history request failed: the run
    /// proceeds with an empty admissible window — every spread observation
    /// inadmissible, the targets on their documented raw-percentile / carry fallback
    /// — never a new failure state (`docs/portfolio-analysis.md` §Starting
    /// parameters). Only the two run-level prints hard-fail (§Failure posture).
    #[serde(default)]
    pub history_gap: Option<String>,
}

/// One quarterly income-statement print (newest first in
/// [`CompanyFinancials::quarterly_income`]) — the trailing driver prints the v2
/// anchor window joins on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuarterlyIncomeRow {
    /// Period end, ISO date.
    pub period_end: String,
    /// The statement feed's filing date; absent, the anchor date falls back to the
    /// period end plus the drafted filing grace.
    pub filing_date: Option<String>,
    pub revenue: Option<f64>,
    pub eps_diluted: Option<f64>,
    pub diluted_shares: Option<f64>,
    /// Statement lines for the TTM statement basis (`docs/portfolio-analysis.md`
    /// §Starting parameters — four-quarter sums feeding the margin / growth /
    /// multiple inputs, the grade-band slice's F5 closure). `#[serde(default)]`
    /// keeps rows persisted before the fields decodable.
    #[serde(default)]
    pub net_income: Option<f64>,
    #[serde(default)]
    pub gross_profit: Option<f64>,
    #[serde(default)]
    pub cost_of_revenue: Option<f64>,
    /// Quarterly operating income — the pre-profit overlay's eligibility leg (TTM
    /// operating income ≤ 0 — `docs/portfolio-analysis.md` §Starting parameters).
    /// `#[serde(default)]` keeps rows persisted before the field decodable.
    #[serde(default)]
    pub operating_income: Option<f64>,
}

/// One quarterly cash-flow-statement print (newest first in
/// [`CompanyFinancials::quarterly_cash_flow`]) — the pre-profit overlay's burn /
/// runway / capex-intensity source (`docs/portfolio-analysis.md` §Starting
/// parameters). Fetched only for stocks; the fund surface never pulls statements.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuarterlyCashFlowRow {
    /// Period end, ISO date.
    pub period_end: String,
    /// The statement feed's filing date — the canonicalization tie-break when one
    /// period arrives twice (a restatement): the latest filing wins, never wire
    /// order. `#[serde(default)]` for pre-field fixtures.
    #[serde(default)]
    pub filing_date: Option<String>,
    /// The feed's reported free cash flow, where present.
    pub free_cash_flow: Option<f64>,
    /// Operating cash flow — with capex, the derivation fallback when the feed
    /// carries no `freeCashFlow` line.
    pub operating_cash_flow: Option<f64>,
    /// Capital expenditure as reported (FMP serves it negative — an outflow);
    /// consumers read its magnitude.
    pub capex: Option<f64>,
}

impl QuarterlyCashFlowRow {
    /// The row's free cash flow: the reported line first, else derived as
    /// `operating cash flow − |capex|` (sign-tolerant — some sources report capex
    /// as a positive outflow). `None` when neither resolves.
    pub fn resolved_free_cash_flow(&self) -> Option<f64> {
        self.free_cash_flow.or(match (self.operating_cash_flow, self.capex) {
            (Some(ocf), Some(capex)) => Some(ocf - capex.abs()),
            _ => None,
        })
    }
}

/// The forward consensus the v2 driver ladder reads (`analyst-estimates`) — the
/// **next-twelve-months (NTM) read**: a time-weighted blend of the two nearest
/// forward fiscal-year rows by their month-overlap with the rolling twelve-month
/// window, so a mostly-reported current fiscal year (whose consensus ≈ the trailing
/// print) contributes only its remaining months rather than masquerading as the
/// forward year (`docs/portfolio-analysis.md` §Starting parameters). Mid is the
/// consensus average; low / high bound the bear / bull scenario drivers where the
/// spread is published.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConsensusEstimate {
    /// The near row's fiscal-period end, ISO date.
    pub period_end: String,
    pub eps_low: Option<f64>,
    pub eps_mid: Option<f64>,
    pub eps_high: Option<f64>,
    pub revenue_low: Option<f64>,
    pub revenue_mid: Option<f64>,
    pub revenue_high: Option<f64>,
    /// Forward annual rows the read blended: 2 on the NTM blend, 1 on a single
    /// forward row (0 only on a hand-built fixture — read as 1).
    #[serde(default)]
    pub periods_used: u8,
    /// The near row's blend weight (1.0 on a single-row read).
    #[serde(default)]
    pub near_weight: f64,
    /// Forward rows that actually **contributed** to the blended EPS mid — the
    /// corroboration count the targets-v4 clamp release reads for the EPS rung.
    /// `periods_used` counts blended rows, not rows behind any given field:
    /// inside an active blend a leg only one row carries is used alone, and a
    /// boundary-day near row (weight exactly 0) is present without
    /// contributing — either way a single estimate would masquerade as
    /// two-row corroboration (Codex rounds 1–2). `#[serde(default)]` for
    /// pre-field records.
    #[serde(default)]
    pub eps_mid_rows: u8,
    /// The revenue rung's counterpart: forward rows contributing to the revenue
    /// mid.
    #[serde(default)]
    pub revenue_mid_rows: u8,
}

/// The normalized financial inputs the engine reasons over, assembled by the dossier
/// from FMP per-company data and SEC EDGAR facts (`docs/data-sources.md`). Every
/// field is optional: a source that can't resolve a line records it in [`Self::gaps`]
/// rather than supplying a fabricated value, so the engine grades over what is
/// actually present.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyFinancials {
    pub symbol: String,
    pub current_price: Option<f64>,
    pub market_cap: Option<f64>,
    pub shares_outstanding: Option<f64>,
    /// Most-recent and prior-period revenue (the growth numerator/denominator).
    pub revenue: Option<f64>,
    pub revenue_prior: Option<f64>,
    pub gross_profit: Option<f64>,
    pub operating_income: Option<f64>,
    pub net_income: Option<f64>,
    pub eps: Option<f64>,
    pub total_debt: Option<f64>,
    pub total_equity: Option<f64>,
    pub free_cash_flow: Option<f64>,
    pub pe_ratio: Option<f64>,
    pub ps_ratio: Option<f64>,
    pub pb_ratio: Option<f64>,
    /// Chronological closing prices (oldest first), for momentum and volatility.
    pub price_history: Vec<f64>,
    /// Deep **dated** daily closes (oldest first) — the v2 anchor join's price side
    /// and the drawdown read (FMP dated EOD).
    #[serde(default)]
    pub daily_closes: Vec<DatedValue>,
    /// Trailing quarterly income prints, newest first — the v2 anchor window's
    /// trailing driver source (needs ~4 extra quarters beyond the window for TTM).
    #[serde(default)]
    pub quarterly_income: Vec<QuarterlyIncomeRow>,
    /// Trailing quarterly cash-flow prints, newest first — the pre-profit overlay's
    /// TTM burn / runway / capex legs (`docs/portfolio-analysis.md` §Starting
    /// parameters). `#[serde(default)]` for pre-field fixtures and stored runs.
    #[serde(default)]
    pub quarterly_cash_flow: Vec<QuarterlyCashFlowRow>,
    /// Balance-sheet liquid-resource lines from the latest quarterly print — the
    /// pre-profit runway numerator (`liquid resources = cash and cash equivalents +
    /// short-term investments`). `#[serde(default)]` for pre-field fixtures.
    #[serde(default)]
    pub cash_and_equivalents: Option<f64>,
    #[serde(default)]
    pub short_term_investments: Option<f64>,
    /// The forward consensus (nearest coming fiscal year) — the v2 driver ladder.
    #[serde(default)]
    pub consensus: Option<ConsensusEstimate>,
    /// Trailing-twelve-month dividends per share — the forward-dividend estimate the
    /// twelve-month total return adds (a sustainable basis, never a raw special).
    #[serde(default)]
    pub ttm_dividends_per_share: Option<f64>,
    /// Tagged inputs a source could not resolve, carried into the prompt so the model
    /// reasons over what is absent rather than inferring it.
    pub gaps: Vec<String>,
    /// Which statement window the values above were computed on — set at the
    /// canonicalization choke point (`dossier::apply_ttm_statement_basis`) by every
    /// producer that adopts or falls back. `None` where no statement basis applies
    /// (a fund) or was resolved.
    ///
    /// Read by the ledger evaluation to detect a basis change: it does not alter any
    /// value, only whether a statement-derived condition is comparable this pass.
    #[serde(default)]
    pub statement_basis: Option<crate::portfolio::StatementBasis>,
}

/// The shared statement canonicalization policy, applied **in place**: quarterly
/// income and cash-flow rows sort newest-first by `(period_end, filing_date)`
/// descending and deduplicate by period end, so a duplicated period (a restatement
/// served twice) resolves to the latest filing — never to wire order. The residual —
/// equal period AND equal/absent filing dates with different values — keeps the
/// first-served row.
///
/// Called once at the statement choke point (`dossier::apply_ttm_statement_basis`,
/// which every statement-consuming path passes before any engine read), so the TTM
/// sums, the driver ladder's growth-clamp trailing prints and share basis, and the
/// anchor-observation windows all read one canonical order. The pre-profit overlay
/// (`pre_profit::statement_inputs`) additionally holds the same rule locally — its
/// order-independence is a standalone, test-pinned contract.
pub fn canonicalize_statements(fin: &mut CompanyFinancials) {
    fin.quarterly_income.sort_by(|a, b| {
        b.period_end
            .cmp(&a.period_end)
            .then_with(|| b.filing_date.cmp(&a.filing_date))
    });
    fin.quarterly_income.dedup_by(|a, b| a.period_end == b.period_end);
    fin.quarterly_cash_flow.sort_by(|a, b| {
        b.period_end
            .cmp(&a.period_end)
            .then_with(|| b.filing_date.cmp(&a.filing_date))
    });
    fin.quarterly_cash_flow.dedup_by(|a, b| a.period_end == b.period_end);
}

/// Whether a run of newest-first quarterly period-ends is **consecutive
/// quarters**: each adjacent pair sits roughly one quarter apart (60–120 days,
/// covering 13/14-week fiscal calendars and transition stubs). Canonicalization
/// fixes order and duplicates but cannot detect a *skipped* quarter — and every
/// fixed-width window in the chain (the TTM statement basis, the anchor
/// windows, the trailing clamp prints, the pre-profit YoY / margin windows)
/// assumes its rows are consecutive, so a feed gap would silently stretch a
/// "TTM" past twelve months. An undatable period-end reads non-contiguous
/// (the conservative side: the window degrades to its gap path).
pub fn quarters_contiguous<'a, I>(period_ends: I) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    let mut prev: Option<chrono::NaiveDate> = None;
    for end in period_ends {
        let Ok(d) = chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d") else {
            return false;
        };
        if let Some(p) = prev {
            if !(60..=120).contains(&(p - d).num_days()) {
                return false;
            }
        }
        prev = Some(d);
    }
    true
}

/// The raw computed metrics behind the sub-scores — recorded on the run's audit so a
/// verdict's basis is inspectable, and rendered into the interpretation prompt so the
/// model reasons over real figures. Each is `None` when its inputs were missing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ComputedMetrics {
    pub net_margin: Option<f64>,
    pub gross_margin: Option<f64>,
    pub revenue_growth: Option<f64>,
    pub debt_to_equity: Option<f64>,
    pub return_volatility: Option<f64>,
    pub trailing_return: Option<f64>,
    pub pe_ratio: Option<f64>,
    pub ps_ratio: Option<f64>,
    pub pb_ratio: Option<f64>,
    /// Fund context: the reported expense ratio (decimal), where the holding is a
    /// fund. `#[serde(default)]` keeps pre-field audits decodable.
    #[serde(default)]
    pub expense_ratio: Option<f64>,
    /// Fund context: price-vs-NAV premium (decimal; meaningful on the closed-end
    /// form, context elsewhere).
    #[serde(default)]
    pub nav_premium: Option<f64>,
    /// Fund context: the share of fund weight the exposure-priced composite actually
    /// prices (`docs/portfolio-analysis.md` §Asset eligibility — the uncovered share
    /// is reported beside the read, never averaged in).
    #[serde(default)]
    pub composite_coverage: Option<f64>,
}

/// One moved metric of the engine-computed input delta
/// (`docs/portfolio-workflow.md` §Step 6b — the metric-level diff): the prior
/// audit's stored value beside this run's. Either side is `None` where that run
/// could not compute the metric — an appearing or disappearing value is a change,
/// never a fabricated zero-delta.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricChange {
    pub name: &'static str,
    pub old: Option<f64>,
    pub new: Option<f64>,
}

/// The engine-computed metric delta: every [`ComputedMetrics`] field whose stored
/// prior value differs from this run's. Resolution is **exact `old ≠ new`**
/// (ruled 2026-08-21 — no materiality margin): persisted numerics round-trip
/// bit-exact (`docs/storage.md` — the `float_roundtrip` guarantee), so equality
/// is a real no-change and any difference is a concrete delta entry. A metric
/// absent on both sides is no entry. The positioning leg of the designed delta
/// stays excluded — its data legs are unbuilt.
pub fn metric_delta(prior: &ComputedMetrics, current: &ComputedMetrics) -> Vec<MetricChange> {
    let pairs: [(&'static str, Option<f64>, Option<f64>); 12] = [
        ("net margin", prior.net_margin, current.net_margin),
        ("gross margin", prior.gross_margin, current.gross_margin),
        ("revenue growth", prior.revenue_growth, current.revenue_growth),
        ("debt/equity", prior.debt_to_equity, current.debt_to_equity),
        ("return volatility", prior.return_volatility, current.return_volatility),
        ("trailing return", prior.trailing_return, current.trailing_return),
        ("P/E", prior.pe_ratio, current.pe_ratio),
        ("P/S", prior.ps_ratio, current.ps_ratio),
        ("P/B", prior.pb_ratio, current.pb_ratio),
        ("expense ratio", prior.expense_ratio, current.expense_ratio),
        ("NAV premium", prior.nav_premium, current.nav_premium),
        ("composite coverage", prior.composite_coverage, current.composite_coverage),
    ];
    pairs
        .into_iter()
        .filter(|(_, old, new)| old != new)
        .map(|(name, old, new)| MetricChange { name, old, new })
        .collect()
}

/// The three-state hurdle read plus the scenario total returns it tested
/// (`docs/portfolio-analysis.md` §Starting parameters — the dead-money hurdle).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HurdleRead {
    pub state: crate::portfolio::HurdleState,
    /// The tier-scaled hurdle rate tested (decimal ratio): `DGS2 + tier premium`.
    pub hurdle_rate: Option<f64>,
    /// Twelve-month scenario **total returns** (price + forward dividends), decimal.
    pub tr_bear: Option<f64>,
    pub tr_base: Option<f64>,
    pub tr_bull: Option<f64>,
    /// Whether the base-case total return clears the hurdle as a **point test** — the
    /// new-money admission read (entry decision; exit-side dispersion tolerance never
    /// licenses new capital).
    pub admits_new_money: bool,
}

/// How the v2 scenario targets were derived — recorded on the audit so a target's
/// basis (and any fallback) is inspectable, and versioned so calibration never mixes
/// target bases.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TargetMeta {
    /// The ladder rung: "consensus forward EPS", "consensus forward revenue per
    /// share", or "fund exposure composite".
    pub driver_rung: String,
    /// True when the multiples were spread-anchored on the DGS10 history; false on
    /// the raw-percentile fallback (window below the observation floor).
    pub rate_anchored: bool,
    pub anchor_observations: usize,
    /// True when the driver was held flat across scenarios (no published consensus
    /// spread, or the fund form's construction).
    pub flat_driver: bool,
    /// Scenarios that individually fell back to their raw multiple percentile on the
    /// degenerate-denominator guard.
    pub degenerate_scenarios: usize,
    /// True when the finished prices needed the defensive monotonicity repair.
    pub monotonicity_repaired: bool,
    /// True when no anchor observation existed at all and the current multiple was
    /// carried (recorded, never silent).
    pub current_multiple_carry: bool,
    /// Forward annual consensus rows behind the driver (2 = the NTM blend, 1 = a
    /// single forward row; `None` on the fund form). `#[serde(default)]` for
    /// pre-field audits.
    #[serde(default)]
    pub consensus_rows: Option<u8>,
    /// The near row's NTM blend weight (1.0 on a single-row read; `None` on the
    /// fund form or a pre-blend record) — persisted so a stored run's driver
    /// provenance is fully recoverable. `#[serde(default)]` for pre-field audits.
    #[serde(default)]
    pub consensus_near_weight: Option<f64>,
    /// True when the growth clamp flattened a *published* driver spread to a single
    /// value — without this the calibration data can't tell "published flat" from
    /// "clamp-flattened". `#[serde(default)]` for pre-field audits.
    #[serde(default)]
    pub clamp_flattened: bool,
    /// True when the scenario band was widened to the volatility-scaled dispersion
    /// floor. `#[serde(default)]` for pre-field audits.
    #[serde(default)]
    pub dispersion_floor_applied: bool,
    /// (targets-v4) anchor observations dropped by the multiple sanity bound —
    /// raw multiples above the current multiple × the drafted factor.
    /// `#[serde(default)]` for pre-v4 audits.
    #[serde(default)]
    pub anchor_bounded: usize,
    /// (targets-v4) true when the trough release priced the unclamped consensus
    /// (corroborated rows + current multiple above the anchor window's rich end).
    /// `#[serde(default)]` for pre-v4 audits.
    #[serde(default)]
    pub clamp_released: bool,
    pub parameter_version: String,
}

/// The engine's analyzed output for a holding that cleared the evidence floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineOutput {
    pub sub_scores: SubScores,
    pub grade: Grade,
    pub metrics: ComputedMetrics,
    pub price_targets: PriceTargets,
    /// The deterministic per-branch risk tier (`docs/portfolio-analysis.md` §Starting
    /// parameters), with any tier-input gaps logged beside it.
    pub risk_tier: crate::portfolio::RiskTier,
    pub tier_gaps: Vec<String>,
    /// The capital-efficiency / dead-money read over the scenario total returns.
    pub hurdle: HurdleRead,
    /// How the scenario targets were derived (rung, fallbacks, version).
    pub target_meta: TargetMeta,
    /// True when the letter rests on an imputed (neutral-50) sub-score — surfaced as
    /// the visible low-confidence marker beside the letter.
    pub low_confidence_grade: bool,
    /// The fund path's deterministic strategy classification label (`None` for a
    /// stock) — the classification is shown on the card
    /// (`docs/portfolio-analysis.md` §Asset eligibility).
    pub fund_class_label: Option<String>,
    /// The deterministic structural path-dependency flag (an option-overlay fund on
    /// the priced path; always false for a stock) — card-visible, and it barred the
    /// Low risk tier.
    pub structural_flag: bool,
    /// The stored closed-form re-anchor basis the engine-only quick paths read
    /// (`docs/portfolio-analysis.md` §The quick check) — persisted on the audit.
    pub quick_basis: Option<QuickCheckBasis>,
    /// The implied-expectations range ([`ImpliedExpectations`]) — computed on
    /// the stock path; `None` on the fund path (its settled flat driver prices
    /// no driver trajectory to invert), the current-multiple carry, and
    /// pre-field runs (`#[serde(default)]`).
    #[serde(default)]
    pub implied_expectations: Option<ImpliedExpectations>,
}

/// What the engine resolved to: an analysis, or an explicit abstention when the
/// evidence floor was not met (`docs/portfolio-analysis.md` §Evidence floor).
#[derive(Debug, Clone, PartialEq)]
pub enum EngineVerdict {
    Analyzed(Box<EngineOutput>),
    InsufficientEvidence(String),
}

// ---- Thesis-ledger series resolution & condition evaluation --------------------
//
// The executability surface (`docs/portfolio-analysis.md` §The position thesis
// ledger): a quantitative ledger condition must resolve to a series the engine
// actually computes and refreshes — the suite's shared resolution contract
// (`docs/trade-opportunities-workflow.md` §Step 3c), applied at Portfolio's seam.
// This closed enum IS that surface: the 6g validation parses a draft's series
// claim against it, and the evaluation below resolves each series to a value plus
// a distinct observation identity.

/// The closed set of engine-resolvable ledger series. Each maps to a value the
/// engine computes every run ([`ComputedMetrics`], the live price, the position's
/// book weight) and carries a derived cadence — statement-derived series advance on
/// filing cadence, price-derived ones on market-data cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LedgerSeries {
    NetMargin,
    GrossMargin,
    RevenueGrowth,
    DebtToEquity,
    ReturnVolatility,
    TrailingReturn,
    PeRatio,
    PsRatio,
    PbRatio,
    ExpenseRatio,
    Price,
}

impl LedgerSeries {
    /// Every resolvable series — the vocabulary the ledger schema advertises and the
    /// interpretation prompt lists.
    pub const ALL: [LedgerSeries; 11] = [
        LedgerSeries::NetMargin,
        LedgerSeries::GrossMargin,
        LedgerSeries::RevenueGrowth,
        LedgerSeries::DebtToEquity,
        LedgerSeries::ReturnVolatility,
        LedgerSeries::TrailingReturn,
        LedgerSeries::PeRatio,
        LedgerSeries::PsRatio,
        LedgerSeries::PbRatio,
        LedgerSeries::ExpenseRatio,
        LedgerSeries::Price,
    ];

    /// The kebab label serde uses — for schema enums and claim parsing.
    pub fn as_kebab(&self) -> &'static str {
        match self {
            LedgerSeries::NetMargin => "net-margin",
            LedgerSeries::GrossMargin => "gross-margin",
            LedgerSeries::RevenueGrowth => "revenue-growth",
            LedgerSeries::DebtToEquity => "debt-to-equity",
            LedgerSeries::ReturnVolatility => "return-volatility",
            LedgerSeries::TrailingReturn => "trailing-return",
            LedgerSeries::PeRatio => "pe-ratio",
            LedgerSeries::PsRatio => "ps-ratio",
            LedgerSeries::PbRatio => "pb-ratio",
            LedgerSeries::ExpenseRatio => "expense-ratio",
            LedgerSeries::Price => "price",
        }
    }

    /// Parse a draft's series claim against the closed surface — the resolution
    /// contract's app-side check; `None` means the claim doesn't resolve and the
    /// condition downgrades to qualitative.
    pub fn parse(claim: &str) -> Option<LedgerSeries> {
        LedgerSeries::ALL
            .iter()
            .copied()
            .find(|s| s.as_kebab() == claim.trim())
    }

    /// Whether the engine ever computes this series for the holding's vehicle
    /// kind. The executability surface is class-shaped: statement and multiple
    /// series exist only for stocks (the fund path skips the facts call and
    /// carries no statement lines), the expense ratio only for funds. A series
    /// the class can never resolve must downgrade at 6g — admitted, it would
    /// type unevaluable on every sweep, permanently un-clear its family, and
    /// badge the holding on every selective run.
    pub fn computable_for(self, is_fund: bool) -> bool {
        if is_fund {
            matches!(
                self,
                LedgerSeries::ExpenseRatio
                    | LedgerSeries::Price
                    | LedgerSeries::ReturnVolatility
                    | LedgerSeries::TrailingReturn
            )
        } else {
            !matches!(self, LedgerSeries::ExpenseRatio)
        }
    }

    /// The series' cadence (`docs/portfolio-analysis.md` §The position thesis
    /// ledger): statement-derived series are filing-cadence; price-derived ones
    /// market-data. The expense ratio rides the fund's `etf/info` print — a
    /// filing-like cadence.
    pub fn cadence(&self) -> crate::portfolio::ConditionCadence {
        use crate::portfolio::ConditionCadence::*;
        match self {
            LedgerSeries::NetMargin
            | LedgerSeries::GrossMargin
            | LedgerSeries::RevenueGrowth
            | LedgerSeries::DebtToEquity
            | LedgerSeries::ExpenseRatio => Filing,
            LedgerSeries::ReturnVolatility
            | LedgerSeries::TrailingReturn
            | LedgerSeries::PeRatio
            | LedgerSeries::PsRatio
            | LedgerSeries::PbRatio
            | LedgerSeries::Price => MarketData,
        }
    }

    /// Whether this series' VALUE comes off the statement window — and so moves when
    /// the statement basis changes, independently of the business
    /// ([`crate::portfolio::StatementBasis`]).
    ///
    /// Deliberately wider than the filing *cadence*: the three multiples are keyed to
    /// the marks' trading day (market cadence) but their denominators are statement
    /// lines, so a TTM → annual flip steps them exactly as it steps the margins. That
    /// is the dangerous combination — a market-cadence series confirms in two
    /// distinct observations, so a basis step can confirm within days.
    ///
    /// The expense ratio rides the fund's own print and funds carry no statement
    /// lines at all; the price-derived series are untouched by a basis change.
    pub fn statement_derived(&self) -> bool {
        matches!(
            self,
            LedgerSeries::NetMargin
                | LedgerSeries::GrossMargin
                | LedgerSeries::RevenueGrowth
                | LedgerSeries::DebtToEquity
                | LedgerSeries::PeRatio
                | LedgerSeries::PsRatio
                | LedgerSeries::PbRatio
        )
    }

    /// The required consecutive distinct breaching observations for this series —
    /// the persistence-semantics count, derived from cadence (drafted constants).
    pub fn required_consecutive(&self) -> u32 {
        match self.cadence() {
            crate::portfolio::ConditionCadence::Filing => LEDGER_CONSECUTIVE_FILING,
            crate::portfolio::ConditionCadence::MarketData => LEDGER_CONSECUTIVE_MARKET_DATA,
        }
    }

    /// A short human description for the interpretation prompt's vocabulary list.
    pub fn describe(&self) -> &'static str {
        match self {
            LedgerSeries::NetMargin => "TTM net margin (decimal)",
            LedgerSeries::GrossMargin => "TTM gross margin (decimal)",
            LedgerSeries::RevenueGrowth => "year-over-year revenue growth (decimal)",
            LedgerSeries::DebtToEquity => "debt / equity ratio",
            LedgerSeries::ReturnVolatility => "daily realized return volatility (decimal)",
            LedgerSeries::TrailingReturn => "trailing price return (decimal)",
            LedgerSeries::PeRatio => "price / earnings multiple",
            LedgerSeries::PsRatio => "price / sales multiple",
            LedgerSeries::PbRatio => "price / book multiple",
            LedgerSeries::ExpenseRatio => "fund expense ratio (decimal)",
            LedgerSeries::Price => "the holding's price (account currency)",
        }
    }
}

/// One resolved series observation: the value plus the distinct observation identity
/// the persistence semantics key on (a trading-day date for market-data series, the
/// newest statement period end for filing-cadence ones).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedObservation {
    pub value: f64,
    pub observation_id: String,
}

/// Resolve one series against this run's computed surface. `Err` carries the typed
/// reason the series could not be resolved this run (a metric gap, no dated print
/// to key the observation) — an unevaluable condition is a typed state, never a
/// silent clear.
pub fn resolve_series(
    series: LedgerSeries,
    metrics: &ComputedMetrics,
    fin: &CompanyFinancials,
) -> Result<ResolvedObservation, String> {
    // Observation identities — always a real print, never the calendar. Market-data:
    // the newest daily close's date (the actual trading-day print); with no dated
    // history the series is unevaluable rather than keyed to the run date, which
    // would let successive degraded runs advance a streak against unchanged data.
    // Filing: the newest quarterly period end — the statement vintage anchor; absent
    // quarterly rows, likewise unevaluable. The expense ratio's `etf/info` print
    // carries no date, so its identity keys to the **value itself** — a distinct
    // observation exists only when the print actually changed
    // (`docs/portfolio-analysis.md` §The position thesis ledger: a fund-ledger
    // condition advances on a *changed* `etf/info` print), so repeated runs against
    // one unchanged print can never advance a streak or re-raise an acknowledged
    // breach.
    let market_obs = || -> Result<String, String> {
        fin.daily_closes
            .last()
            .map(|d| d.date.clone())
            .ok_or_else(|| "no dated price print to key the observation".to_string())
    };
    // Accepted consequence of the period-end identity (ruled 2026-08-05,
    // piece-3 walk): an amendment restating the SAME period keys the same
    // observation, so a restated breach cannot advance a filing streak until
    // the next quarter's filing — the material-filing badge and statement
    // re-pull still fire, only the streak waits.
    let filing_obs = || -> Result<String, String> {
        fin.quarterly_income
            .iter()
            .map(|r| r.period_end.clone())
            .max()
            .ok_or_else(|| "no quarterly statement print to key the observation".to_string())
    };
    let metric = |v: Option<f64>, label: &str| -> Result<f64, String> {
        v.ok_or_else(|| format!("{label} is a gap this run"))
    };
    // A signed multiple is OFF-SCALE for a threshold comparison, so it resolves
    // **unevaluable** rather than comparing. Both hazards are real and only one
    // direction of each is obvious:
    //
    // - A negative P/E means the company has just gone loss-making. Compared
    //   naively it satisfies "P/E below 15" and fires an *add* trigger on
    //   exactly the evidence that should stop one. The signed derive upstream is
    //   deliberate (`dossier.rs`, grade-v2.1 — the sign must survive so the
    //   engine's "a loss-maker is never cheap" valuation guard is reachable), so
    //   the guard belongs here at the comparator, not at the derive.
    // - A negative debt/equity means liabilities exceed the equity base —
    //   maximal leverage. It cannot breach "debt/equity above 3", and the worse
    //   half is that it then reads as a **clean** observation: the clean arm
    //   resets `breach_streak`, `first_breach_at`, `confirmed_at` and the
    //   acknowledgment, silently clearing a standing breach. That silent clear is
    //   the failure the streak machinery exists to prevent.
    //
    // Unevaluable — not a sentinel "maximal" value — because a ledger threshold
    // is model-authored with an open comparator, so any sentinel would have to
    // assert a direction that is wrong for the other comparator ("debt/equity
    // below 1" must not be satisfied by negative equity either), and `f64`
    // infinities do not survive the `serde_json` round-trip `last_value` takes.
    // This is a deliberate divergence from `assign_stock_tier`, whose closed
    // internal predicates CAN express "maximal" safely and do
    // (`RISK_DEBT_EQUITY_BAND`). Resolving unevaluable moves no state at all, so
    // it can neither fabricate a crossing nor clear one, and the typed
    // `unevaluable_series` channel downgrades the family's claimed clear.
    // `on_scale` names the admissible range per series rather than inferring it:
    // zero debt is a real debt/equity reading, zero is degenerate for a P/E.
    let on_scale =
        |v: Option<f64>, label: &str, admissible: fn(f64) -> bool| -> Result<f64, String> {
            let value = metric(v, label)?;
            if !admissible(value) {
                return Err(format!(
                    "{label} is {value} — off-scale for a threshold comparison, so this \
                     condition is unevaluable rather than compared"
                ));
            }
            Ok(value)
        };

    match series {
        LedgerSeries::NetMargin => Ok(ResolvedObservation {
            value: metric(metrics.net_margin, "net margin")?,
            observation_id: filing_obs()?,
        }),
        LedgerSeries::GrossMargin => Ok(ResolvedObservation {
            value: metric(metrics.gross_margin, "gross margin")?,
            observation_id: filing_obs()?,
        }),
        LedgerSeries::RevenueGrowth => Ok(ResolvedObservation {
            value: metric(metrics.revenue_growth, "revenue growth")?,
            observation_id: filing_obs()?,
        }),
        // Negative equity is off-scale, never "low leverage" — see `on_scale`.
        LedgerSeries::DebtToEquity => Ok(ResolvedObservation {
            value: on_scale(metrics.debt_to_equity, "debt/equity", |d| d >= 0.0)?,
            observation_id: filing_obs()?,
        }),
        LedgerSeries::ExpenseRatio => {
            let value = metric(metrics.expense_ratio, "expense ratio")?;
            Ok(ResolvedObservation {
                value,
                observation_id: format!("expense-ratio:{value}"),
            })
        }
        LedgerSeries::ReturnVolatility => Ok(ResolvedObservation {
            value: metric(metrics.return_volatility, "return volatility")?,
            observation_id: market_obs()?,
        }),
        LedgerSeries::TrailingReturn => Ok(ResolvedObservation {
            value: metric(metrics.trailing_return, "trailing return")?,
            observation_id: market_obs()?,
        }),
        // A loss-maker's negative P/E is off-scale, never "cheap" — see
        // `on_scale`. Zero is degenerate on the same scale and goes with it.
        LedgerSeries::PeRatio => Ok(ResolvedObservation {
            value: on_scale(metrics.pe_ratio, "P/E", |p| p > 0.0)?,
            observation_id: market_obs()?,
        }),
        LedgerSeries::PsRatio => Ok(ResolvedObservation {
            value: metric(metrics.ps_ratio, "P/S")?,
            observation_id: market_obs()?,
        }),
        LedgerSeries::PbRatio => Ok(ResolvedObservation {
            value: metric(metrics.pb_ratio, "P/B")?,
            observation_id: market_obs()?,
        }),
        LedgerSeries::Price => Ok(ResolvedObservation {
            value: metric(fin.current_price, "current price")?,
            observation_id: market_obs()?,
        }),
    }
}

/// The engine's evaluation of a prior ledger's quantitative conditions — the
/// crossings interpretation reads, the typed unevaluable notes, and each evaluated
/// condition's updated state (`docs/portfolio-analysis.md` §The position thesis
/// ledger: the engine tests which quantitative falsifiers and triggers crossed this
/// run, under their persistence semantics).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LedgerEvaluation {
    pub crossings: Vec<crate::portfolio::ConditionCrossing>,
    /// "condition '<statement>': <reason>" lines for series unresolvable this run.
    pub unevaluable: Vec<String>,
    /// The same unresolvable conditions' series, typed — the quick check maps each
    /// to its signal family so a claimed `fresh_clear` downgrades to `unknown`
    /// (an allowed condition the sweep could not resolve means the family cannot
    /// vouch for the carried verdict).
    pub unevaluable_series: Vec<LedgerSeries>,
    /// Updated evaluation state per evaluated condition id (unevaluated conditions
    /// keep their carried state).
    pub updated_states: Vec<(String, crate::portfolio::ConditionEvalState)>,
}

/// Evaluate the prior ledger's quantitative conditions against this run's computed
/// surface, under the persistence semantics: the breach streak advances **only on a
/// distinct new observation** (a new trading day's print, a new filing); a
/// filing-cadence breach confirms immediately (count 1, the margin its noise
/// guard); a market-data breach needs [`LEDGER_CONSECUTIVE_MARKET_DATA`] consecutive
/// distinct observations; and an acknowledged confirmed breach re-raises only when
/// confirmed against a **later** observation than the acknowledged one.
pub fn evaluate_ledger_conditions(
    ledger: &crate::portfolio::ThesisLedger,
    metrics: &ComputedMetrics,
    fin: &CompanyFinancials,
    run_date: &str,
) -> LedgerEvaluation {
    evaluate_ledger_conditions_gated(ledger, metrics, fin, run_date, |_| true)
}

/// The cadence-gated form of [`evaluate_ledger_conditions`] — the engine-only quick
/// check's entry (`docs/portfolio-analysis.md` §The quick check): market-data
/// conditions evaluate on every pass, filing-cadence conditions only when a fresh
/// observation of their series landed, so `allow` filters by series. A gated-out
/// condition is **skipped whole** — no unevaluable note, no state update — its
/// carried state simply stands (a skipped family is vouched for by the retrieval
/// that found no new observation, which is the caller's sweep-state concern, not an
/// evaluation failure).
pub fn evaluate_ledger_conditions_gated(
    ledger: &crate::portfolio::ThesisLedger,
    metrics: &ComputedMetrics,
    fin: &CompanyFinancials,
    run_date: &str,
    allow: impl Fn(LedgerSeries) -> bool,
) -> LedgerEvaluation {
    use crate::portfolio::{ConditionCrossing, ConditionEvalState, CrossingOutcome};

    let mut out = LedgerEvaluation::default();
    for cond in &ledger.conditions {
        let Some(quant) = &cond.quant else { continue };
        if !allow(quant.series) {
            continue;
        }
        let resolved = match resolve_series(quant.series, metrics, fin) {
            Ok(r) => r,
            Err(reason) => {
                out.unevaluable
                    .push(format!("condition '{}': {reason}", cond.statement));
                out.unevaluable_series.push(quant.series);
                continue;
            }
        };

        let mut st = cond.eval_state.clone().unwrap_or_default();

        // **Basis continuity.** A statement-derived series compared across a change
        // of statement basis is comparing two different measurements of the same
        // business: a one-quarter feed gap fails the contiguity guard, the holding
        // drops to the SEC annual basis, and a growing issuer's P/S steps (~8.0 →
        // 10.3) with nothing having happened. The prior ledger is evaluated against
        // THIS run's metrics, so the step lands on a threshold authored under the
        // other basis; the streak then carries into the sweep, which rescales the
        // stored — now flipped — multiples by price alone, and a market-cadence
        // series needs only one more distinct close to CONFIRM. That is a fabricated
        // crossing on a thesis that is intact, and on a falsifier it forces archival.
        //
        // So the pass types the series unevaluable and re-stamps: no state movement
        // beyond adopting the new basis, and the streak is dropped rather than
        // carried, because the observations in it were taken on the other basis. It
        // is deliberately NOT the clean arm — a clean read would clear the
        // acknowledgment and report a thesis confirmation the evidence does not
        // support. Once re-stamped, the condition evaluates normally on the new
        // basis, so this fires once per flip, not permanently.
        //
        // The annual fallback itself is retained: falling back is honest, and the
        // basis-flip rate is a big-run watch. It is the CROSSING it manufactures
        // that is not.
        if quant.series.statement_derived() {
            if let Some(current_basis) = fin.statement_basis {
                match st.authored_statement_basis {
                    Some(prior) if prior != current_basis => {
                        out.unevaluable.push(format!(
                            "condition '{}': statement basis changed ({prior:?} →                              {current_basis:?}) — the level moved with the measurement,                              so this pass cannot compare it",
                            cond.statement
                        ));
                        out.unevaluable_series.push(quant.series);
                        st.authored_statement_basis = Some(current_basis);
                        st.breach_streak = 0;
                        st.first_breach_at = None;
                        st.confirmed_at = None;
                        out.updated_states.push((cond.condition_id.clone(), st));
                        continue;
                    }
                    // First evaluation, or a pre-stamp state: adopt without a
                    // discontinuity — there is nothing to disagree with.
                    _ => st.authored_statement_basis = Some(current_basis),
                }
            }
        }

        let margin = quant.margin.max(0.0);
        let breached = match quant.comparator {
            crate::portfolio::LedgerComparator::Below => resolved.value < quant.threshold - margin,
            crate::portfolio::LedgerComparator::Above => resolved.value > quant.threshold + margin,
        };
        // Observation ordering is MONOTONIC for date-keyed ids (closes, period
        // ends, marks days): the sweep and the full run read EOD through
        // different FMP endpoints at different moments, so an out-of-order
        // *older* print is reachable and must neither advance a streak, reset
        // one, nor regress the recorded state. The value-keyed expense-ratio id
        // has no order and keeps the distinct test.
        let iso_date = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
        let resolved_date = iso_date(&resolved.observation_id);
        let last_date = st.last_observation_id.as_deref().and_then(iso_date);
        let (new_observation, stale_observation) = match (resolved_date, last_date) {
            (Some(r), Some(l)) => (r > l, r < l),
            _ => (
                st.last_observation_id.as_deref() != Some(resolved.observation_id.as_str()),
                false,
            ),
        };
        if stale_observation {
            // Older than the recorded observation: no state movement at all.
        } else if new_observation {
            st.last_observation_id = Some(resolved.observation_id.clone());
            st.last_value = Some(resolved.value);
            st.last_evaluated_at = Some(run_date.to_string());
            if breached {
                st.breach_streak += 1;
                if st.breach_streak == 1 {
                    st.first_breach_at = Some(run_date.to_string());
                }
                if st.breach_streak >= quant.series.required_consecutive()
                    && st.confirmed_at.is_none()
                {
                    st.confirmed_at = Some(run_date.to_string());
                }
            } else {
                // A clean distinct observation resets the streak — the prior streak's
                // record lives in the previously persisted runs. The acknowledgment
                // clears with it: a later re-breach is a genuinely new observation
                // (without the clear, a value-keyed id re-printing its acknowledged
                // value would be suppressed indefinitely).
                st.breach_streak = 0;
                st.first_breach_at = None;
                st.confirmed_at = None;
                st.acknowledged_observation_id = None;
            }
        } else {
            // Same observation: re-evaluation never advances the streak — repeated
            // passes against one print can't confirm a breach. The VALUE may have
            // been corrected under the same identity (a fixed close, a
            // same-period restated print): a corrected-clean read supersedes the
            // breached read of the same observation, so any standing breach
            // state resets — a confirmed crossing must never stand on, or emit
            // carrying, a value that no longer breaches.
            st.last_value = Some(resolved.value);
            st.last_evaluated_at = Some(run_date.to_string());
            if !breached && st.breach_streak > 0 {
                st.breach_streak = 0;
                st.first_breach_at = None;
                st.confirmed_at = None;
            }
        }

        let confirmed_now = st.breach_streak >= quant.series.required_consecutive();
        // The acknowledgment transition: a consumed breach re-raises only against
        // an observation past the acknowledged one — strictly newer for date-keyed
        // ids (an older cross-feed print must not read as new), distinct for the
        // orderless value-keyed id (whose stale-suppression case the reset-clear
        // above closes).
        let past_ack = match (&st.acknowledged_observation_id, &st.last_observation_id) {
            (Some(ack), Some(last)) => match (iso_date(ack), iso_date(last)) {
                (Some(a), Some(l)) => l > a,
                _ => last != ack,
            },
            (None, _) => true,
            (Some(_), None) => false,
        };
        // A still-unconsumed confirmed breach re-raises each pass until 6g acks
        // it — keyed to the RECORDED observation when this pass's print is
        // stale (acking the stale id would let the next sweep's newer print
        // read as past-ack and re-raise the just-consumed breach).
        // Snapshotted before `st` moves into `final_state`.
        let st_confirmed_at = st.confirmed_at.clone();
        let (crossing_obs, crossing_val) = if stale_observation {
            (
                st.last_observation_id
                    .clone()
                    .unwrap_or_else(|| resolved.observation_id.clone()),
                st.last_value.unwrap_or(resolved.value),
            )
        } else {
            (resolved.observation_id.clone(), resolved.value)
        };
        if confirmed_now && past_ack {
            out.crossings.push(ConditionCrossing {
                condition_id: cond.condition_id.clone(),
                statement: cond.statement.clone(),
                role: cond.role,
                outcome: CrossingOutcome::Confirmed,
                observed_value: crossing_val,
                threshold: quant.threshold,
                observation_id: crossing_obs,
                // The date the streak actually reached its count — set once, on the
                // confirming pass, and held until the streak resets. A between-run
                // sweep can confirm days before the next full run consumes this
                // crossing, so the consuming run's date is not the confirmation's.
                confirmed_at: st_confirmed_at,
            });
        } else if breached && new_observation && !confirmed_now {
            out.crossings.push(ConditionCrossing {
                condition_id: cond.condition_id.clone(),
                statement: cond.statement.clone(),
                role: cond.role,
                outcome: CrossingOutcome::FirstBreach,
                observed_value: resolved.value,
                threshold: quant.threshold,
                observation_id: resolved.observation_id.clone(),
                // Nothing has confirmed yet.
                confirmed_at: None,
            });
        }
        let final_state: ConditionEvalState = st;
        out.updated_states
            .push((cond.condition_id.clone(), final_state));
    }
    out
}

// ---- The engine --------------------------------------------------------------

/// Analyze a holding's financials into sub-scores, a grade, and scenario targets — or
/// abstain. The evidence floor fails when there is no current price (nothing to
/// target or value against) or fewer than [`MIN_SUBSCORES_FOR_GRADE`] sub-scores are
/// computable; either is an explicit `insufficient-evidence`, never a low-conviction
/// guess.
pub fn analyze(fin: &CompanyFinancials, rates: &RateAnchors) -> EngineVerdict {
    let metrics = compute_metrics(fin);

    let quality = quality_score(&metrics);
    let valuation = valuation_score(&metrics);
    let momentum = momentum_score(&metrics);
    let risk = risk_score(&metrics);

    // The evidence floor counts **letter** sub-scores only — momentum is computed
    // alongside but lives outside the letter (`docs/portfolio-analysis.md` §Starting
    // parameters, the settled ex-momentum re-weight).
    let computed = [quality, valuation, risk]
        .iter()
        .filter(|s| s.is_some())
        .count();

    let Some(price) = fin.current_price else {
        return EngineVerdict::InsufficientEvidence(
            "no current price for the holding — cannot value or set targets".to_string(),
        );
    };
    if computed < MIN_SUBSCORES_FOR_GRADE {
        return EngineVerdict::InsufficientEvidence(format!(
            "only {computed} of 3 letter sub-scores computable (need {MIN_SUBSCORES_FOR_GRADE}); \
             financial inputs too sparse to grade"
        ));
    }

    // The v2 rate-anchored scenario targets. No admissible driver on any ladder rung
    // is the named evidence-floor reason (`docs/portfolio-analysis.md` §Evidence floor).
    let bundle = match scenario_targets_v2(price, fin, rates, &metrics) {
        TargetOutcome::Computed(b) => b,
        TargetOutcome::NoAdmissibleDriver => {
            return EngineVerdict::InsufficientEvidence(
                "no-admissible-driver: no positive forward-EPS consensus and no computable \
                 forward revenue per share on any ladder rung"
                    .to_string(),
            );
        }
    };

    // A missing sub-score takes the neutral midpoint (50) so the composite stays
    // defined; dividing by the full fixed weight sum keeps it on the same 0–100 scale
    // (an impute-to-neutral, not a renormalization over the present sub-scores). The
    // count gate above guarantees at least two are real, so this never grades on all
    // defaults — and a letter resting on any imputed axis carries the visible
    // low-confidence marker.
    let low_confidence_grade = quality.is_none() || valuation.is_none() || risk.is_none();
    let sub_scores = SubScores {
        quality: quality.unwrap_or(50.0),
        valuation: valuation.unwrap_or(50.0),
        momentum: momentum.unwrap_or(50.0),
        risk: risk.unwrap_or(50.0),
    };
    let grade = grade_from_subscores(&sub_scores);

    // Deterministic per-branch tier assignment, then the tier-scaled hurdle over the
    // scenario total returns — assigned before anything downstream consumes it
    // (`docs/portfolio-workflow.md` §Step 6b).
    let (risk_tier, tier_gaps) = assign_stock_tier(fin, &metrics);
    let hurdle = hurdle_read(&bundle.scenario, rates.dgs2, risk_tier);

    EngineVerdict::Analyzed(Box::new(EngineOutput {
        sub_scores,
        grade,
        metrics,
        price_targets: bundle.targets,
        risk_tier,
        tier_gaps,
        hurdle,
        target_meta: bundle.meta,
        low_confidence_grade,
        fund_class_label: None,
        structural_flag: false,
        quick_basis: Some(bundle.basis),
        implied_expectations: bundle.implied,
    }))
}

/// Roll the **letter** sub-scores (quality / valuation / risk) up to a letter grade
/// through the fixed ex-momentum weights — momentum is context, never a letter input
/// (`docs/portfolio-analysis.md` §Starting parameters). Public so a reviewer (and the
/// live smoke) can assert the roll-up directly.
pub fn grade_from_subscores(s: &SubScores) -> Grade {
    let composite = (s.quality * W_QUALITY + s.valuation * W_VALUATION + s.risk * W_RISK)
        / (W_QUALITY + W_VALUATION + W_RISK);
    if composite >= GRADE_A {
        Grade::A
    } else if composite >= GRADE_B {
        Grade::B
    } else if composite >= GRADE_C {
        Grade::C
    } else if composite >= GRADE_D {
        Grade::D
    } else {
        Grade::F
    }
}

pub(crate) fn compute_metrics(fin: &CompanyFinancials) -> ComputedMetrics {
    let ratio = |num: Option<f64>, den: Option<f64>| match (num, den) {
        (Some(n), Some(d)) if d != 0.0 => Some(n / d),
        _ => None,
    };
    ComputedMetrics {
        net_margin: ratio(fin.net_income, fin.revenue),
        gross_margin: ratio(fin.gross_profit, fin.revenue),
        revenue_growth: match (fin.revenue, fin.revenue_prior) {
            (Some(now), Some(prior)) if prior > 0.0 => Some(now / prior - 1.0),
            _ => None,
        },
        debt_to_equity: ratio(fin.total_debt, fin.total_equity),
        return_volatility: return_volatility(&fin.price_history),
        trailing_return: trailing_return(&fin.price_history),
        pe_ratio: fin.pe_ratio,
        ps_ratio: fin.ps_ratio,
        pb_ratio: fin.pb_ratio,
        // Fund context fields — set by the fund path only.
        expense_ratio: None,
        nav_premium: None,
        composite_coverage: None,
    }
}

/// Linearly map `value` from `[lo, hi]` onto a 0–100 score, clamped at the ends.
/// `lo` maps to 0 and `hi` to 100 (pass `lo > hi` to invert — lower input scores
/// higher).
pub(crate) fn scale(value: f64, lo: f64, hi: f64) -> f64 {
    let t = (value - lo) / (hi - lo);
    (t.clamp(0.0, 1.0)) * 100.0
}

/// Average the present components, or `None` when none are present.
fn average(parts: &[Option<f64>]) -> Option<f64> {
    let present: Vec<f64> = parts.iter().filter_map(|p| *p).collect();
    if present.is_empty() {
        None
    } else {
        Some(present.iter().sum::<f64>() / present.len() as f64)
    }
}

/// Quality (higher better): profitability margins on one statement basis.
fn quality_score(m: &ComputedMetrics) -> Option<f64> {
    let (nl, nh) = QUALITY_NET_MARGIN_BAND;
    let (gl, gh) = QUALITY_GROSS_MARGIN_BAND;
    average(&[
        m.net_margin.map(|x| scale(x, nl, nh)),
        m.gross_margin.map(|x| scale(x, gl, gh)),
    ])
}

/// Valuation (higher better == cheaper): inverted multiples. A negative P/E (no
/// earnings) is not "cheap" — it scores low rather than off the scale.
fn valuation_score(m: &ComputedMetrics) -> Option<f64> {
    let (pel, peh) = VALUATION_PE_BAND;
    let (psl, psh) = VALUATION_PS_BAND;
    let (pbl, pbh) = VALUATION_PB_BAND;
    let pe = m.pe_ratio.map(|x| {
        if x <= 0.0 {
            VALUATION_NEGATIVE_PE_SCORE
        } else {
            scale(x, pel, peh)
        }
    });
    average(&[
        pe,
        m.ps_ratio.map(|x| scale(x, psl, psh)),
        m.pb_ratio.map(|x| scale(x, pbl, pbh)),
    ])
}

/// Momentum (higher better): trailing price return over the available history.
fn momentum_score(m: &ComputedMetrics) -> Option<f64> {
    let (lo, hi) = MOMENTUM_TRAILING_RETURN_BAND;
    m.trailing_return.map(|r| scale(r, lo, hi))
}

/// Risk (higher == safer): low realized volatility and low leverage. Negative
/// equity makes the D/E ratio negative — levered beyond the equity base, not
/// unlevered — so it takes the band's floor rather than riding the inverted
/// clamp to "maximally safe" (the mirror of the negative-P/E rule).
fn risk_score(m: &ComputedMetrics) -> Option<f64> {
    let (vl, vh) = RISK_VOLATILITY_BAND;
    let (dl, dh) = RISK_DEBT_EQUITY_BAND;
    average(&[
        m.return_volatility.map(|v| scale(v, vl, vh)),
        m.debt_to_equity
            .map(|d| if d < 0.0 { 0.0 } else { scale(d, dl, dh) }),
    ])
}

/// Simple total return from the first to the last close.
fn trailing_return(history: &[f64]) -> Option<f64> {
    match (history.first(), history.last()) {
        (Some(&first), Some(&last)) if history.len() >= 2 && first > 0.0 => Some(last / first - 1.0),
        _ => None,
    }
}

/// Population standard deviation of simple period-over-period returns.
pub(crate) fn return_volatility(history: &[f64]) -> Option<f64> {
    if history.len() < 3 {
        return None;
    }
    let returns: Vec<f64> = history
        .windows(2)
        .filter_map(|w| if w[0] > 0.0 { Some(w[1] / w[0] - 1.0) } else { None })
        .collect();
    if returns.len() < 2 {
        return None;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    Some(var.sqrt())
}

// ---- v2 rate-anchored scenario-target function ---------------------------------
//
// `docs/portfolio-analysis.md` §Starting parameters — the settled shape: per-share
// driver × scenario multiple, the multiple re-anchoring the driver multiple's own
// history on the run-level DGS10 through the spread percentiles (inverse mapping),
// with the documented guards and fallbacks. The fund form (the settled fund-form
// bullet) shares the core through `spread_anchored_scenarios`.

/// One admissible anchor-window observation: the driver-yield spread over the
/// contemporaneous DGS10, and the raw multiple for the fallback paths. The spread is
/// `None` when the quarter's dated `DGS10` join found no observation (a failed or
/// thin history) — the quarter's **raw multiple stays admissible** so the
/// raw-percentile fallback still reads real history rather than degrading straight
/// to the current-multiple carry (`docs/portfolio-analysis.md` §Starting parameters:
/// a failed history request leaves every *spread* observation inadmissible and takes
/// the documented raw-percentile fallback).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorObservation {
    pub spread: Option<f64>,
    pub raw_multiple: f64,
}

/// The scenario set the core computes: three prices, their twelve-month total
/// returns, and the fallback record.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioSet {
    pub bear: f64,
    pub base: f64,
    pub bull: f64,
    pub tr_bear: f64,
    pub tr_base: f64,
    pub tr_bull: f64,
    pub rate_anchored: bool,
    /// Dated-rate (spread-admissible) anchors — the count the ≥ 8 floor reads.
    pub anchor_observations: usize,
    /// Driver-admissible quarters backing the raw-multiple fallback percentiles.
    pub raw_observations: usize,
    pub degenerate_scenarios: usize,
    pub monotonicity_repaired: bool,
    pub current_multiple_carry: bool,
    /// True when the bear/bull band was widened to the dispersion-floor half-spread.
    pub dispersion_floor_applied: bool,
    /// The spread percentile surface behind the multiples, `[bear (P75), base (P50),
    /// bull (P25)]` — present only on the rate-anchored path. Persisted (via
    /// [`QuickCheckBasis`]) so the engine-only quick paths can re-anchor closed-form
    /// on a fresh DGS10 without re-estimating the window.
    pub spread_percentiles: Option<[f64; 3]>,
    /// The direct raw-multiple percentiles `[bear (P25), base (P50), bull (P75)]` —
    /// present whenever any driver-admissible quarter existed.
    pub raw_percentiles: Option<[f64; 3]>,
}

/// What the v2 wrapper resolved to: a computed bundle, or the named
/// `no-admissible-driver` evidence-floor reason.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetOutcome {
    // Boxed: the bundle dwarfs the unit variant.
    Computed(Box<TargetBundle>),
    NoAdmissibleDriver,
}

/// The v2 function's full output: the persisted targets plus the scenario set (the
/// hurdle's input), the derivation record, and the stored quick-path basis.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetBundle {
    pub targets: PriceTargets,
    pub scenario: ScenarioSet,
    pub meta: TargetMeta,
    /// The closed-form re-anchor basis the run persists for the engine-only quick
    /// paths (`docs/portfolio-analysis.md` §The quick check).
    pub basis: QuickCheckBasis,
    /// The implied-expectations range ([`ImpliedExpectations`]) — the same
    /// scenario multiples inverted at the spot. `None` on the current-multiple
    /// carry (no independent multiple to invert against).
    pub implied: Option<ImpliedExpectations>,
}

/// The stored basis the engine-only quick paths re-anchor against
/// (`docs/portfolio-analysis.md` §The quick check: the v2 scenario multiples
/// re-anchored closed-form on the fresh DGS10 against the **stored** anchor-window
/// percentiles and **stored** drivers from the last full pass — no re-estimation).
/// Persisted per priced holding on the run's audit record; every field is what the
/// full pass actually computed, never re-derived at quick-check time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickCheckBasis {
    /// The run-time price the targets were computed from — also the price the
    /// stored valuation multiples (P/E, P/S, P/B) were read at, so a quick pass can
    /// re-scale them to a fresh print without re-fetching statements.
    pub spot: f64,
    /// `[bear, base, bull]` per-share drivers the full pass settled on.
    pub drivers: [f64; 3],
    #[serde(default)]
    pub spread_percentiles: Option<[f64; 3]>,
    #[serde(default)]
    pub raw_percentiles: Option<[f64; 3]>,
    /// The forward-dividend (fund: distribution) leg of the twelve-month total return.
    pub forward_dividends: f64,
    /// The volatility-scaled dispersion floor the full pass applied.
    pub dispersion_floor: f64,
    /// The NTM consensus EPS mid the run read (`None` where no consensus existed) —
    /// the quick check's revision-preflight comparator
    /// (`docs/portfolio-analysis.md` §Starting parameters, the large-revision-move leg).
    #[serde(default)]
    pub consensus_eps_mid: Option<f64>,
}

/// The latest value in a dated, oldest-first series on or before `date` (ISO dates
/// compare lexicographically). `None` when the series is empty or starts after.
pub fn latest_on_or_before(series: &[DatedValue], date: &str) -> Option<f64> {
    let idx = series.partition_point(|d| d.date.as_str() <= date);
    idx.checked_sub(1).map(|i| series[i].value)
}

/// Linear-interpolated percentile (`p` in 0..=1) over an unsorted sample.
fn percentile(values: &[f64], p: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("finite percentile inputs"));
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = p * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// The shared v2 core (`docs/portfolio-analysis.md` §Starting parameters): scenario
/// prices from per-scenario drivers × spread-anchored multiples. With enough anchor
/// observations the multiples come from the spread percentiles under the **inverse**
/// mapping (`spread_bear = P75 … spread_bull = P25`, a wider spread being a cheaper
/// multiple), each scenario guarded against a degenerate reciprocal denominator;
/// below the observation floor the rate correction drops entirely and the mapping
/// flips back to **direct** raw-multiple percentiles (the cheap multiple is the bear
/// multiple in both domains); with no observations at all the current multiple is
/// carried (recorded — scenario spread then comes from driver dispersion alone).
/// Scenario identity comes from the mapping, never from sorting the finished prices —
/// the monotonicity sort is a recorded defensive repair only. `dispersion_floor` is
/// the minimum half-spread (decimal, around the base price) the finished band is
/// widened to when the driver and multiple axes both collapse — see
/// [`dispersion_floor`]; pass `0.0` to disable (a test pinning the raw mapping).
pub fn spread_anchored_scenarios(
    spot: f64,
    drivers: [f64; 3],
    observations: &[AnchorObservation],
    dgs10_now: f64,
    forward_income_per_share: f64,
    dispersion_floor: f64,
) -> ScenarioSet {
    // The ≥ 8 floor reads the dated-rate (spread-admissible) anchors; the raw
    // multiples of every driver-admissible quarter back the fallback percentiles, so
    // a failed DGS10 join degrades to real history — never straight to the carry.
    let spreads: Vec<f64> = observations.iter().filter_map(|o| o.spread).collect();
    let raws: Vec<f64> = observations.iter().map(|o| o.raw_multiple).collect();
    let n_spread = spreads.len();
    let n_raw = raws.len();

    let spread_ps = (n_spread >= MIN_ANCHOR_OBSERVATIONS).then(|| {
        [
            percentile(&spreads, 0.75), // bear
            percentile(&spreads, 0.50), // base
            percentile(&spreads, 0.25), // bull
        ]
    });
    let raw_ps = (n_raw >= 1).then(|| {
        [
            percentile(&raws, 0.25), // bear
            percentile(&raws, 0.50), // base
            percentile(&raws, 0.75), // bull
        ]
    });

    let mut set = scenarios_from_surfaces(
        spot,
        drivers,
        spread_ps,
        raw_ps,
        // The carry multiple is the spot's own multiple on the base driver — at
        // full-run time the live spot IS the basis spot.
        spot / drivers[1],
        dgs10_now,
        forward_income_per_share,
        dispersion_floor,
    );
    set.anchor_observations = n_spread;
    set.raw_observations = n_raw;
    set
}

/// The shared v2 finishing core over the **percentile surfaces** — multiples
/// (inverse spread map with the degenerate guard; direct raw fallback; the
/// current-multiple carry), scenario prices, the defensive monotonicity repair, the
/// dispersion floor, and total returns. Called by [`spread_anchored_scenarios`] on
/// the live window and by [`reanchor_scenarios`] on a stored basis, so the quick
/// paths' closed-form re-anchor is the same arithmetic, never a re-implementation.
#[allow(clippy::too_many_arguments)] // each is one leg of the stored basis, documented on `QuickCheckBasis`
fn scenarios_from_surfaces(
    spot: f64,
    drivers: [f64; 3],
    spread_ps: Option<[f64; 3]>,
    raw_ps: Option<[f64; 3]>,
    carry_multiple: f64,
    dgs10_now: f64,
    forward_income_per_share: f64,
    dispersion_floor: f64,
) -> ScenarioSet {
    let (multiples, degenerate, current_multiple_carry) =
        scenario_multiples(spread_ps, raw_ps, carry_multiple, dgs10_now);

    let mut prices = [
        drivers[0] * multiples[0],
        drivers[1] * multiples[1],
        drivers[2] * multiples[2],
    ];
    // Defensive repair only — a residual crossing remains possible through the
    // fallback seams (one scenario rate-anchored, another raw).
    let monotonicity_repaired = !(prices[0] <= prices[1] && prices[1] <= prices[2]);
    if monotonicity_repaired {
        prices.sort_by(|a, b| a.partial_cmp(b).expect("finite scenario prices"));
    }

    // The dispersion floor: widen (never narrow) each side to at least the
    // half-spread around base, after the repair so base is the middle price. Zero
    // dispersion turns the three-state hurdle into a point test — even the bull leg
    // sits at base, so a flat surface reads `fails` with false certainty; the floor
    // restores the exit-side hysteresis the three-state read is defined by, recorded.
    let mut dispersion_floor_applied = false;
    if dispersion_floor > 0.0 && prices[1].is_finite() && prices[1] > 0.0 {
        let min_bear = prices[1] * (1.0 - dispersion_floor);
        let min_bull = prices[1] * (1.0 + dispersion_floor);
        if prices[0] > min_bear {
            prices[0] = min_bear;
            dispersion_floor_applied = true;
        }
        if prices[2] < min_bull {
            prices[2] = min_bull;
            dispersion_floor_applied = true;
        }
    }

    let tr = |p: f64| (p + forward_income_per_share) / spot - 1.0;
    ScenarioSet {
        bear: prices[0],
        base: prices[1],
        bull: prices[2],
        tr_bear: tr(prices[0]),
        tr_base: tr(prices[1]),
        tr_bull: tr(prices[2]),
        rate_anchored: spread_ps.is_some(),
        anchor_observations: 0,
        raw_observations: 0,
        degenerate_scenarios: degenerate,
        monotonicity_repaired,
        current_multiple_carry,
        dispersion_floor_applied,
        spread_percentiles: spread_ps,
        raw_percentiles: raw_ps,
    }
}

/// The three scenario multiples `[bear, base, bull]` off the percentile
/// surfaces, plus the degenerate-scenario count and the carry marker — the
/// **single** multiple derivation: [`scenarios_from_surfaces`] prices with it
/// and [`implied_expectations`] inverts against it, so the two can never
/// disagree on which multiple a scenario used.
fn scenario_multiples(
    spread_ps: Option<[f64; 3]>,
    raw_ps: Option<[f64; 3]>,
    carry_multiple: f64,
    dgs10_now: f64,
) -> ([f64; 3], usize, bool) {
    let mut degenerate = 0usize;
    let mut current_multiple_carry = false;
    let multiples: [f64; 3] = match (spread_ps, raw_ps) {
        (Some(spread_ps), raw_ps) => {
            // Inverse mapping in the spread domain; the raw fallback maps direct.
            // The rate-anchored path always has raw percentiles too (every
            // spread-admissible quarter is driver-admissible), so the degenerate
            // guard's fallback is real history; a stored basis missing them keeps
            // the reciprocal (recorded via `degenerate_scenarios` staying zero).
            let mut ms = [0.0; 3];
            for s in 0..3 {
                let denom = spread_ps[s] + dgs10_now;
                if denom < DEGENERATE_DENOMINATOR_EPS {
                    if let Some(raw_ps) = raw_ps {
                        degenerate += 1;
                        ms[s] = raw_ps[s];
                    } else {
                        ms[s] = 1.0 / denom.max(DEGENERATE_DENOMINATOR_EPS);
                    }
                } else {
                    ms[s] = 1.0 / denom;
                }
            }
            ms
        }
        (None, Some(raw_ps)) => raw_ps,
        (None, None) => {
            // No anchor history at all: the caller's carry multiple (the full pass's
            // spot over its base driver — a *stored* multiple on the re-anchor path,
            // never the fresh print's), so scenario spread comes from driver
            // dispersion alone — recorded.
            current_multiple_carry = true;
            [carry_multiple, carry_multiple, carry_multiple]
        }
    };
    (multiples, degenerate, current_multiple_carry)
}

/// The implied-expectations range (`docs/portfolio-analysis.md` §Starting
/// parameters): the scenario math inverted at the live price — the per-share
/// driver, and its growth against the trailing TTM print, that the spot
/// **already assumes** at each scenario multiple `M_bear … M_bull`. A
/// closed-form **range under stated assumptions**, never one solved number:
/// the surface that produced the multiples and the DGS10 print ride the
/// record as the assumptions. On the revenue rung the range reads as the
/// revenue trajectory the price assumes at prevailing margins — the margin
/// dimension is a stated assumption, not a second solved axis. Conviction /
/// action evidence only — never a gate, never a sub-score input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImpliedExpectations {
    /// The per-share driver the spot implies at each scenario multiple,
    /// `[at M_bear, at M_base, at M_bull]`. The inversion runs opposite to
    /// pricing: the cheap bear multiple demands the **largest** driver, so the
    /// range's demanding end is index 0.
    pub implied_drivers: [f64; 3],
    /// Implied growth vs the trailing TTM print (decimal, same order); `None`
    /// where the trailing print is absent or non-positive (growth undefinable —
    /// the implied drivers still carry the level read).
    pub implied_growth: Option<[f64; 3]>,
    /// The driver ladder rung the read inverts (the same rung the targets
    /// priced).
    pub driver_rung: String,
    /// True on the revenue rung — the margin-dimension caveat above applies.
    pub revenue_based: bool,
    /// Which surface produced the multiples (the stated assumption): the
    /// rate-anchored spread percentiles, else the raw-multiple percentiles.
    pub rate_anchored: bool,
    /// The DGS10 print the rate-anchored multiples used (decimal ratio).
    pub dgs10: f64,
}

/// Invert the scenario multiples at the live price ([`ImpliedExpectations`]).
/// `None` on the current-multiple carry — its multiple is derived *from* the
/// spot, so the inversion would only hand back the priced driver (no
/// independent read exists) — and on a non-positive spot or a multiple the
/// surfaces cannot produce finitely.
pub fn implied_expectations(
    spot: f64,
    scenario: &ScenarioSet,
    trailing_print: Option<f64>,
    driver_rung: &str,
    use_eps: bool,
    dgs10_now: f64,
) -> Option<ImpliedExpectations> {
    if !(spot.is_finite() && spot > 0.0) || scenario.current_multiple_carry {
        return None;
    }
    let (multiples, _, carry) = scenario_multiples(
        scenario.spread_percentiles,
        scenario.raw_percentiles,
        f64::NAN,
        dgs10_now,
    );
    if carry || multiples.iter().any(|m| !m.is_finite() || *m <= 0.0) {
        return None;
    }
    let implied_drivers = multiples.map(|m| spot / m);
    let implied_growth = trailing_print
        .filter(|t| *t > 0.0)
        .map(|t| implied_drivers.map(|d| d / t - 1.0));
    Some(ImpliedExpectations {
        implied_drivers,
        implied_growth,
        driver_rung: driver_rung.to_string(),
        revenue_based: !use_eps,
        rate_anchored: scenario.rate_anchored,
        dgs10: dgs10_now,
    })
}

// ---- Step-6e forward-assumption refinement ---------------------------------------

/// Which target driver a validated forward assumption addresses — the
/// pipeline's deterministic mapping of the distilled `affects` field (drafted:
/// EPS / revenue, the two ladder rungs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssumptionMetric {
    ForwardEps,
    ForwardRevenue,
}

/// The Step-6e forward-assumption input, after the pipeline mapped the
/// distilled claim's fields (`docs/portfolio-workflow.md` §Step 6e).
#[derive(Debug, Clone)]
pub struct ForwardAssumptionInput {
    pub metric: AssumptionMetric,
    pub value: f64,
    /// The claim's stated units — validated and magnitude-normalized before
    /// the value may fill a driver (a bare `4.5` for "$4.5 billion" must never
    /// ride into `revenue_mid` unscaled).
    pub units: String,
    /// The model's typed `conflict_handling` declaration — a claim this
    /// policy validates, never a rule the model selects.
    pub supersede: bool,
    pub fact_type: String,
    pub as_of: String,
    pub source_url: String,
}

/// The target-side fields a successful refinement replaces on the engine
/// output — the backward-looking sub-scores are untouched by contract.
#[derive(Debug, Clone)]
pub struct RefinedTargets {
    pub price_targets: crate::portfolio::PriceTargets,
    pub target_meta: TargetMeta,
    pub hurdle: HurdleRead,
    pub implied_expectations: Option<ImpliedExpectations>,
    pub quick_basis: Option<QuickCheckBasis>,
    /// The policy rule the engine matched — the audit's resolution log.
    pub matched_rule: String,
}

/// The primary-source fact-type whitelist a `supersede` requires (drafted —
/// issued company guidance, a signed contract, a filed figure). A supplement
/// holds the same bar: an assumption that moves a target is always a
/// primary-class fact. Matching is **whole-token**, never substring — an
/// `"unfiled rumor"` must not satisfy `filed` — and any negating or
/// hedging token disqualifies the whole label (`"not guidance"`,
/// `"withdrawn guidance"`, `"rumored contract"` are non-facts by their own
/// words).
fn assumption_fact_whitelisted(fact_type: &str) -> bool {
    let mut whitelisted = false;
    for t in fact_type
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        if matches!(
            t,
            "not" | "no" | "non" | "never" | "without" | "rumor" | "rumored" | "unconfirmed"
                | "speculative" | "withdrawn" | "denied" | "retracted"
        ) {
            return false;
        }
        if matches!(t, "guidance" | "contract" | "filed" | "filing" | "filings") {
            whitelisted = true;
        }
    }
    whitelisted
}

/// Deterministic unit validation + magnitude normalization for the driver fill
/// (drafted): the units must read **monetary** for either driver — an EPS fact
/// accepts only per-share / currency vocabulary (so `"vehicles"` can never
/// fill an EPS driver) and **rejects** any magnitude token (an EPS "in
/// millions" is malformed), while a revenue fact must carry a currency or
/// magnitude token, magnitude words scaling the value (trillion / billion /
/// million / thousand, plus tn / bn / mn / mm) and a bare sub-1e6 value
/// rejecting as unit-ambiguous. Single-letter suffixes ("B", "M") are
/// deliberately not recognized — too ambiguous to scale on. Rejection is
/// fail-soft: the structured targets stand.
fn normalized_assumption_value(
    metric: AssumptionMetric,
    value: f64,
    units: &str,
) -> Result<f64, String> {
    const CURRENCY_TOKENS: &[&str] = &[
        "usd", "eur", "gbp", "jpy", "cad", "aud", "chf", "dollar", "dollars", "cent", "cents",
    ];
    // The extra vocabulary a per-share unit may carry beside currency tokens.
    const PER_SHARE_TOKENS: &[&str] =
        &["per", "share", "shares", "eps", "diluted", "basic"];
    if units.trim().is_empty() {
        return Err("rejected: the assumption carries no units".to_string());
    }
    let lowered = units
        .replace('$', " usd ")
        .replace('€', " eur ")
        .replace('£', " gbp ")
        .to_ascii_lowercase();
    let tokens: Vec<&str> = lowered
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let mut magnitude: Option<f64> = None;
    let mut currency = false;
    let mut foreign = false;
    for token in &tokens {
        let m = match *token {
            "trillion" | "trillions" | "tn" => Some(1e12),
            "billion" | "billions" | "bn" => Some(1e9),
            "million" | "millions" | "mn" | "mm" => Some(1e6),
            "thousand" | "thousands" => Some(1e3),
            _ => None,
        };
        if let Some(m) = m {
            if magnitude.is_some_and(|prev| prev != m) {
                return Err(format!(
                    "rejected: units {units:?} carry conflicting magnitude tokens"
                ));
            }
            magnitude = Some(m);
            continue;
        }
        if CURRENCY_TOKENS.contains(token) {
            currency = true;
        } else if !PER_SHARE_TOKENS.contains(token) {
            foreign = true;
        }
    }
    match metric {
        AssumptionMetric::ForwardEps => {
            if magnitude.is_some() {
                return Err(format!(
                    "rejected: a per-share fact cannot carry a magnitude in its units ({units:?})"
                ));
            }
            // Non-monetary vocabulary ("vehicles", "units") is not a
            // per-share quantity; an empty unit string passes (the value is
            // already per-share by the metric's contract).
            if foreign {
                return Err(format!(
                    "rejected: units {units:?} name a non-per-share quantity"
                ));
            }
            Ok(value)
        }
        AssumptionMetric::ForwardRevenue => {
            if !currency && magnitude.is_none() {
                return Err(format!(
                    "rejected: revenue units {units:?} carry no currency or magnitude token"
                ));
            }
            match magnitude {
                Some(m) => Ok(value * m),
                None if value >= 1e6 => Ok(value),
                None => Err(format!(
                    "rejected: revenue value {value} with units {units:?} is unit-ambiguous \
                     (no magnitude token and below the 1e6 absolute-dollar floor — drafted)"
                )),
            }
        }
    }
}

/// The **app-owned Step-6e conflict policy** and target recompute
/// (`docs/portfolio-workflow.md` §Step 6e): a validated forward assumption may
/// move a scenario target — the engine, never the model, recomputes it as an
/// explicit, logged assumption. A `supplement` may only fill a driver value
/// the structured feeds don't carry (it never displaces a present feed value);
/// a `supersede` is honored only when every check verifies — and the consensus
/// feed carries **no as-of date** to compare against, so as-built a supersede
/// always rejects on that named condition (structured-wins is the default).
/// `Err` carries the failed condition for the audit; the structured targets
/// stand.
pub fn refine_targets_with_assumption(
    fin: &CompanyFinancials,
    rates: &RateAnchors,
    input: &ForwardAssumptionInput,
) -> Result<RefinedTargets, String> {
    if !input.value.is_finite() || input.value <= 0.0 {
        return Err("rejected: non-positive or non-finite assumption value".to_string());
    }
    if !assumption_fact_whitelisted(&input.fact_type) {
        return Err(format!(
            "rejected: fact type {:?} is outside the primary-source whitelist \
             (issued guidance / signed contract / filed figure)",
            input.fact_type
        ));
    }
    if chrono::NaiveDate::parse_from_str(input.as_of.trim(), "%Y-%m-%d").is_err() {
        return Err(format!(
            "rejected: as-of {:?} is not an ISO date",
            input.as_of
        ));
    }
    let normalized_value = normalized_assumption_value(input.metric, input.value, &input.units)?;
    let feed_value = fin.consensus.as_ref().and_then(|c| match input.metric {
        AssumptionMetric::ForwardEps => c.eps_mid,
        AssumptionMetric::ForwardRevenue => c.revenue_mid,
    });
    let feed_present = feed_value.is_some_and(|v| v > 0.0);
    if feed_present {
        if input.supersede {
            return Err(
                "rejected: supersede unverifiable — the structured consensus carries no \
                 as-of date to compare the fact against (structured-wins default)"
                    .to_string(),
            );
        }
        return Err(
            "rejected: supplement may not displace a present structured value — the \
             feed's value stands"
                .to_string(),
        );
    }

    // The supplement applies: fill the absent driver and re-run the analysis,
    // splicing only the target-side outputs (grade inputs are untouched — the
    // statements did not change).
    let mut refined_fin = fin.clone();
    let consensus = refined_fin.consensus.get_or_insert_with(|| ConsensusEstimate {
        period_end: input.as_of.trim().to_string(),
        eps_low: None,
        eps_mid: None,
        eps_high: None,
        revenue_low: None,
        revenue_mid: None,
        revenue_high: None,
        periods_used: 1,
        near_weight: 1.0,
        eps_mid_rows: 0,
        revenue_mid_rows: 0,
    });
    match input.metric {
        AssumptionMetric::ForwardEps => {
            // A single sourced figure carries no spread: the driver rides flat
            // (the function records flat-driver on the meta) and never counts
            // as consensus corroboration (rows stay 0 — a research fact must
            // not fake a two-row clamp release).
            consensus.eps_low = Some(normalized_value);
            consensus.eps_mid = Some(normalized_value);
            consensus.eps_high = Some(normalized_value);
        }
        AssumptionMetric::ForwardRevenue => {
            consensus.revenue_low = Some(normalized_value);
            consensus.revenue_mid = Some(normalized_value);
            consensus.revenue_high = Some(normalized_value);
        }
    }
    match analyze(&refined_fin, rates) {
        EngineVerdict::Analyzed(out) => Ok(RefinedTargets {
            price_targets: out.price_targets,
            target_meta: out.target_meta,
            hurdle: out.hurdle,
            implied_expectations: out.implied_expectations,
            quick_basis: out.quick_basis,
            matched_rule: format!(
                "supplement: filled the absent {} driver with {normalized_value} \
                 (stated {} {}) from {} ({}, as of {})",
                match input.metric {
                    AssumptionMetric::ForwardEps => "forward-EPS",
                    AssumptionMetric::ForwardRevenue => "forward-revenue",
                },
                input.value,
                input.units,
                input.source_url,
                input.fact_type,
                input.as_of
            ),
        }),
        EngineVerdict::InsufficientEvidence(reason) => Err(format!(
            "rejected: the refined analysis abstained ({reason}) — the structured targets stand"
        )),
    }
}

/// The engine-only quick paths' **closed-form re-anchor**
/// (`docs/portfolio-analysis.md` §The quick check): the stored spread percentiles
/// and drivers from the last full pass, re-anchored on the fresh `DGS10`, with the
/// total returns measured from the **fresh** price — one extra FRED print, no
/// re-estimation, no new heavy retrieval. The ledger's authored monitor band is
/// deliberately **not** derived from this — the re-anchor serves the hurdle read
/// only.
pub fn reanchor_scenarios(
    basis: &QuickCheckBasis,
    fresh_spot: f64,
    dgs10_now: f64,
) -> ScenarioSet {
    scenarios_from_surfaces(
        fresh_spot,
        basis.drivers,
        basis.spread_percentiles,
        basis.raw_percentiles,
        // The carry path re-uses the *stored* multiple (the full pass's spot over
        // its base driver) — a fresh-spot carry would make the target track the
        // live price and hollow the total return.
        basis.spot / basis.drivers[1],
        dgs10_now,
        basis.forward_dividends,
        basis.dispersion_floor,
    )
}

/// The volatility-scaled minimum scenario half-spread (decimal, on the price axis):
/// annualized realized volatility × the scale, clamped to the drafted bounds; the
/// lower bound when volatility can't be computed. Shared by the stock and fund forms
/// (`docs/portfolio-analysis.md` §Starting parameters).
pub fn dispersion_floor(return_volatility: Option<f64>) -> f64 {
    return_volatility
        .map(|v| {
            (v * ANNUALIZATION_FACTOR * DISPERSION_FLOOR_VOL_SCALE)
                .clamp(DISPERSION_FLOOR_MIN, DISPERSION_FLOOR_MAX)
        })
        .unwrap_or(DISPERSION_FLOOR_MIN)
}

/// What the dated anchor walk found: the admissible observations, the
/// sanity-bound drop count, and the window's largest admissible trailing print —
/// the demonstrated earning power the release's trough test reads.
struct AnchorScan {
    observations: Vec<AnchorObservation>,
    bounded: usize,
    /// Max over every window that passed the finite-positive print test —
    /// tracked before the close join and the multiple bound, since a print is
    /// earnings evidence even where its quarter cannot anchor a multiple.
    max_window_print: Option<f64>,
}

/// The trailing-twelve-month driver print anchored at each of the newest
/// [`ANCHOR_WINDOW_QUARTERS`] quarters, joined to the dated closes and DGS10 history
/// (`docs/portfolio-analysis.md` §Starting parameters — the dated anchor join): each
/// admissible quarter anchors on its filing date (period end + the filing grace when
/// absent), reads the latest close on or before that date, and the latest published
/// DGS10 on or before the same date. A quarter whose trailing print is not finite
/// and positive is excluded (an economically invalid multiple observation), and —
/// targets-v4 — one whose raw multiple exceeds `current_multiple ×`
/// [`ANCHOR_MULTIPLE_SANITY_FACTOR`] is excluded as a sanity-bound artifact, the
/// count returned beside the survivors for the audit record.
fn stock_anchor_observations(
    fin: &CompanyFinancials,
    rates: &RateAnchors,
    use_eps: bool,
    current_multiple: Option<f64>,
) -> AnchorScan {
    use chrono::NaiveDate;
    let q = &fin.quarterly_income;
    let mut out = Vec::new();
    let mut bounded = 0usize;
    let mut max_window_print: Option<f64> = None;
    let multiple_cap = current_multiple
        .filter(|cm| cm.is_finite() && *cm > 0.0)
        .map(|cm| cm * ANCHOR_MULTIPLE_SANITY_FACTOR);
    for i in 0..ANCHOR_WINDOW_QUARTERS.min(q.len()) {
        // TTM print: this quarter plus the three before it (rows are newest-first).
        if i + 4 > q.len() {
            break;
        }
        let window = &q[i..i + 4];
        // A window spanning a feed gap is not a TTM — skip it (the anchor set
        // just gets one fewer observation, like any other inadmissible window).
        if !quarters_contiguous(window.iter().map(|r| r.period_end.as_str())) {
            continue;
        }
        let ttm: Option<f64> = if use_eps {
            window.iter().map(|r| r.eps_diluted).sum()
        } else {
            let revenue: Option<f64> = window.iter().map(|r| r.revenue).sum();
            let shares = window[0]
                .diluted_shares
                .or_else(|| q.first().and_then(|r| r.diluted_shares))
                .or(fin.shares_outstanding);
            match (revenue, shares) {
                (Some(rev), Some(sh)) if sh > 0.0 => Some(rev / sh),
                _ => None,
            }
        };
        let Some(ttm) = ttm else { continue };
        if !ttm.is_finite() || ttm <= 0.0 {
            continue;
        }
        max_window_print = Some(max_window_print.map_or(ttm, |m: f64| m.max(ttm)));
        let anchor_date = match &window[0].filing_date {
            Some(d) => d.clone(),
            None => match NaiveDate::parse_from_str(&window[0].period_end, "%Y-%m-%d") {
                Ok(d) => (d + chrono::Duration::days(FILING_GRACE_DAYS))
                    .format("%Y-%m-%d")
                    .to_string(),
                Err(_) => continue,
            },
        };
        let Some(close) = latest_on_or_before(&fin.daily_closes, &anchor_date) else {
            continue;
        };
        if close <= 0.0 {
            continue;
        }
        // The v4 sanity bound: a multiple far above the name's own current
        // multiple is a near-zero-denominator artifact or a departed regime,
        // not reversion evidence — dropped and counted.
        if let Some(cap) = multiple_cap {
            if close / ttm > cap {
                bounded += 1;
                continue;
            }
        }
        // The dated-rate join is per-leg: a quarter with no DGS10 on or before its
        // anchor date loses only its spread — the raw multiple stays admissible for
        // the fallback percentiles.
        let yield_t = ttm / close;
        out.push(AnchorObservation {
            spread: latest_on_or_before(&rates.dgs10_history, &anchor_date)
                .map(|dgs10_t| yield_t - dgs10_t),
            raw_multiple: close / ttm,
        });
    }
    AnchorScan {
        observations: out,
        bounded,
        max_window_print,
    }
}

/// What the driver ladder picked: the scenario drivers plus the derivation record.
struct DriverRead {
    /// `[bear, base, bull]` per-share drivers.
    drivers: [f64; 3],
    /// The same drivers with the growth clamp not applied (positivity fallback
    /// only) — swapped in by the targets-v4 trough clamp release, which is decided
    /// upstream where the anchor surfaces exist.
    unclamped_drivers: [f64; 3],
    /// The rung-matching trailing TTM print (EPS or revenue per share) — the
    /// current-multiple denominator for the v4 anchor bound and release signature.
    trailing_print: Option<f64>,
    rung: &'static str,
    use_eps: bool,
    /// No published low/high spread — the driver held at mid.
    flat_driver: bool,
    /// A published spread collapsed to a single value by the growth clamp — recorded
    /// so calibration can tell it apart from a published-flat spread.
    clamp_flattened: bool,
}

/// The v2 driver ladder (`docs/portfolio-analysis.md` §Starting parameters): pick the
/// per-share fundamental deterministically — consensus forward EPS where a positive
/// consensus exists, else consensus forward revenue per share on the latest reported
/// diluted share count — with each scenario driver's implied growth clamped by the v1
/// sanity bound against the trailing print, and a missing published spread holding
/// the driver flat (recorded). `None` when no rung is admissible.
fn driver_ladder(fin: &CompanyFinancials) -> Option<DriverRead> {
    let c = fin.consensus.as_ref();

    // Trailing TTM prints for the growth clamp (newest four quarters) — only a
    // contiguous four-quarter run is a TTM; a gapped window would clamp against
    // a >12-month print, so the clamp just goes unavailable instead.
    let ttm_window_ok = fin.quarterly_income.len() >= 4
        && quarters_contiguous(fin.quarterly_income[..4].iter().map(|r| r.period_end.as_str()));
    let ttm_eps: Option<f64> = ttm_window_ok
        .then(|| fin.quarterly_income[..4].iter().map(|r| r.eps_diluted).sum())
        .flatten();
    let latest_shares = fin
        .quarterly_income
        .first()
        .and_then(|r| r.diluted_shares)
        .or(fin.shares_outstanding);
    let ttm_rev_ps: Option<f64> = match (
        ttm_window_ok
            .then(|| fin.quarterly_income[..4].iter().map(|r| r.revenue).sum::<Option<f64>>())
            .flatten(),
        latest_shares,
    ) {
        (Some(rev), Some(sh)) if sh > 0.0 => Some(rev / sh),
        _ => None,
    };

    // Clamp a scenario driver's implied growth vs the trailing print (only where the
    // trailing print is positive, so growth is definable); a non-positive scenario
    // driver falls back to the base value rather than pricing a negative driver.
    let clamp = |driver: f64, trailing: Option<f64>, base: f64| -> f64 {
        let d = match trailing {
            Some(t) if t > 0.0 => {
                driver.clamp(t * (1.0 + DRIVER_GROWTH_MIN), t * (1.0 + DRIVER_GROWTH_MAX))
            }
            _ => driver,
        };
        if d.is_finite() && d > 0.0 {
            d
        } else {
            base
        }
    };

    // A published (non-flat) spread whose clamped drivers all landed on one value was
    // flattened by the clamp, not by the consensus — recorded distinctly.
    let clamp_collapse = |drivers: &[f64; 3], flat: bool| {
        !flat && (drivers[0] - drivers[1]).abs() < 1e-12 && (drivers[2] - drivers[1]).abs() < 1e-12
    };

    // The unclamped counterpart: positivity/finiteness fallback only — what the
    // targets-v4 trough release prices when corroborated consensus disagrees
    // with a trough-scale trailing print.
    let sanitize = |driver: f64, base: f64| -> f64 {
        if driver.is_finite() && driver > 0.0 {
            driver
        } else {
            base
        }
    };

    // Rung 1: forward EPS, eligible only on a finite positive consensus mid.
    if let Some(mid) = c.and_then(|c| c.eps_mid).filter(|m| m.is_finite() && *m > 0.0) {
        let base = clamp(mid, ttm_eps, mid);
        // A half-published spread (either leg missing) reads as a missing
        // spread: both legs hold at mid, so `flat_driver` describes the drivers
        // exactly and a clamp collapse stays distinguishable from a
        // consensus-flat band (`docs/portfolio-analysis.md` §Starting
        // parameters — "held flat").
        let spread = c.and_then(|c| c.eps_low.zip(c.eps_high));
        let flat = spread.is_none();
        let (low, high) = spread.unwrap_or((mid, mid));
        let drivers = [
            clamp(low, ttm_eps, base),
            base,
            clamp(high, ttm_eps, base),
        ];
        return Some(DriverRead {
            clamp_flattened: clamp_collapse(&drivers, flat),
            drivers,
            unclamped_drivers: [sanitize(low, mid), mid, sanitize(high, mid)],
            trailing_print: ttm_eps,
            rung: "consensus forward EPS",
            use_eps: true,
            flat_driver: flat,
        });
    }

    // Rung 2: forward revenue per share on the latest reported diluted count.
    if let (Some(mid), Some(sh)) = (
        c.and_then(|c| c.revenue_mid).filter(|m| m.is_finite() && *m > 0.0),
        latest_shares.filter(|s| *s > 0.0),
    ) {
        let base = clamp(mid / sh, ttm_rev_ps, mid / sh);
        // Same half-published-spread convention as the EPS rung above.
        let spread = c.and_then(|c| c.revenue_low.zip(c.revenue_high));
        let flat = spread.is_none();
        let (low, high) = spread
            .map(|(l, h)| (l / sh, h / sh))
            .unwrap_or((mid / sh, mid / sh));
        let drivers = [
            clamp(low, ttm_rev_ps, base),
            base,
            clamp(high, ttm_rev_ps, base),
        ];
        return Some(DriverRead {
            clamp_flattened: clamp_collapse(&drivers, flat),
            drivers,
            unclamped_drivers: [sanitize(low, mid / sh), mid / sh, sanitize(high, mid / sh)],
            trailing_print: ttm_rev_ps,
            rung: "consensus forward revenue per share",
            use_eps: false,
            flat_driver: flat,
        });
    }

    None
}

/// The v2 rate-anchored scenario-target function for a **priced stock**
/// (`docs/portfolio-analysis.md` §Starting parameters): driver ladder → dated anchor
/// join → spread-percentile multiples (inverse map, guarded) → scenario prices and
/// total returns. The twelve-month target's bear/bull **are** the scenario prices;
/// the one-month leg keeps the v1 mechanics (base = spot × (1 + PR_base ⁄ 12), the
/// price-return leg, dividends excluded; volatility-scaled bands with the fixed
/// fallbacks).
pub fn scenario_targets_v2(
    spot: f64,
    fin: &CompanyFinancials,
    rates: &RateAnchors,
    m: &ComputedMetrics,
) -> TargetOutcome {
    let Some(read) = driver_ladder(fin) else {
        return TargetOutcome::NoAdmissibleDriver;
    };

    // The current trailing multiple on the rung's own basis — the v4 anchor
    // bound's cap denominator and one half of the trough release signature.
    let current_multiple = read
        .trailing_print
        .filter(|t| *t > 0.0)
        .map(|t| spot / t)
        .filter(|cm| cm.is_finite() && *cm > 0.0);
    let scan = stock_anchor_observations(fin, rates, read.use_eps, current_multiple);
    let (observations, anchor_bounded) = (scan.observations, scan.bounded);

    // The targets-v4 trough clamp release — three gates, each closing a distinct
    // false-release path (Codex round 1, findings 1–2):
    // 1. Corroboration counts rows CONTRIBUTING to the selected rung's blended
    //    mid — never `periods_used`, and never mere field presence (a
    //    boundary-day near row is present at weight 0) — so a single estimate
    //    cannot masquerade as two rows.
    // 2. The multiple signature (current above the post-bound window's P75) is
    //    evaluated on the bounded observation set, so a sanity-dropped artifact
    //    can never mask or fake it.
    // 3. The direct trough test: the trailing print must be depressed against
    //    the window's own demonstrated earning power — a price rally satisfies
    //    the multiple signature with earnings intact, and only the print
    //    separates the two.
    let raws: Vec<f64> = observations.iter().map(|o| o.raw_multiple).collect();
    let corroborated = fin.consensus.as_ref().is_some_and(|c| {
        let rows = if read.use_eps {
            c.eps_mid_rows
        } else {
            c.revenue_mid_rows
        };
        rows >= CLAMP_RELEASE_MIN_CONSENSUS_ROWS
    });
    let trough_signature = match (current_multiple, raws.is_empty()) {
        (Some(cm), false) => cm > percentile(&raws, 0.75),
        _ => false,
    };
    let trough_print = read
        .trailing_print
        .zip(scan.max_window_print)
        .is_some_and(|(t, mx)| t < TROUGH_PRINT_FRACTION * mx);
    let clamp_released = corroborated
        && trough_signature
        && trough_print
        && read.drivers != read.unclamped_drivers;
    let drivers = if clamp_released {
        read.unclamped_drivers
    } else {
        read.drivers
    };

    let forward_dividends = fin.ttm_dividends_per_share.unwrap_or(0.0);
    let floor = dispersion_floor(m.return_volatility);
    let scenario = spread_anchored_scenarios(
        spot,
        drivers,
        &observations,
        rates.dgs10,
        forward_dividends,
        floor,
    );

    let basis = QuickCheckBasis {
        spot,
        drivers,
        spread_percentiles: scenario.spread_percentiles,
        raw_percentiles: scenario.raw_percentiles,
        forward_dividends,
        dispersion_floor: floor,
        consensus_eps_mid: fin.consensus.as_ref().and_then(|c| c.eps_mid),
    };

    let targets = build_price_targets(
        spot,
        &scenario,
        m,
        read.rung,
        read.flat_driver,
        anchor_bounded,
        clamp_released,
    );
    let meta = TargetMeta {
        driver_rung: read.rung.to_string(),
        rate_anchored: scenario.rate_anchored,
        anchor_observations: scenario.anchor_observations,
        flat_driver: read.flat_driver,
        degenerate_scenarios: scenario.degenerate_scenarios,
        monotonicity_repaired: scenario.monotonicity_repaired,
        current_multiple_carry: scenario.current_multiple_carry,
        consensus_rows: fin.consensus.as_ref().map(|c| c.periods_used.max(1)),
        // Recorded only off a live parse (`periods_used > 0`) — a hand-built
        // fixture's default 0.0 is not a real weight.
        consensus_near_weight: fin
            .consensus
            .as_ref()
            .filter(|c| c.periods_used > 0)
            .map(|c| c.near_weight),
        // A released clamp means the flattening no longer describes the drivers
        // actually priced — the release supersedes the collapse record.
        clamp_flattened: read.clamp_flattened && !clamp_released,
        dispersion_floor_applied: scenario.dispersion_floor_applied,
        anchor_bounded,
        clamp_released,
        parameter_version: SCENARIO_TARGET_PARAMETER_VERSION.to_string(),
    };
    let implied = implied_expectations(
        spot,
        &scenario,
        read.trailing_print,
        read.rung,
        read.use_eps,
        rates.dgs10,
    );
    TargetOutcome::Computed(Box::new(TargetBundle {
        targets,
        scenario,
        meta,
        basis,
        implied,
    }))
}

/// Render a [`ScenarioSet`] into the persisted [`PriceTargets`]: the twelve-month
/// target carries the scenario prices; the one-month leg keeps the v1 mechanics.
/// Shared by the stock and fund forms — the fund form passes the v4 stock-only
/// provenance (`anchor_bounded`, `clamp_released`) as `0` / `false`.
pub fn build_price_targets(
    spot: f64,
    scenario: &ScenarioSet,
    m: &ComputedMetrics,
    rung: &str,
    flat_driver: bool,
    anchor_bounded: usize,
    clamp_released: bool,
) -> PriceTargets {
    let pr_base = scenario.base / spot - 1.0;
    let om_base = spot * (1.0 + pr_base / 12.0);
    let om_band = m
        .return_volatility
        .map(|v| (v * 2.0).clamp(0.02, 0.15))
        .unwrap_or(ONE_MONTH_FALLBACK_BAND);

    let anchor_note = if scenario.current_multiple_carry {
        "no anchor history — current multiple carried".to_string()
    } else if scenario.rate_anchored {
        format!(
            "DGS10 spread-anchored P75/P50/P25 multiples over {} quarterly anchors (inverse map{}{})",
            scenario.anchor_observations,
            if scenario.degenerate_scenarios > 0 {
                "; degenerate-denominator raw fallback on some scenario(s)"
            } else {
                ""
            },
            if scenario.monotonicity_repaired {
                "; monotonicity repaired"
            } else {
                ""
            },
        )
    } else {
        format!(
            "raw multiple percentiles P25/P50/P75 over {} quarterly anchors (direct \
             map; dated-rate spread window below the {MIN_ANCHOR_OBSERVATIONS}-observation \
             floor at {})",
            scenario.raw_observations, scenario.anchor_observations
        )
    };
    let release_note = if clamp_released {
        ", growth clamp released on corroborated consensus (trough signature)"
    } else {
        ""
    };
    let driver_note = if flat_driver {
        format!("{rung}, held flat across scenarios{release_note}")
    } else {
        format!("{rung} low/mid/high{release_note}")
    };
    let floor_note = if scenario.dispersion_floor_applied {
        "; band widened to the volatility-scaled dispersion floor"
    } else {
        ""
    };
    let bound_note = if anchor_bounded > 0 {
        format!("; {anchor_bounded} anchor(s) dropped by the multiple sanity bound")
    } else {
        String::new()
    };

    PriceTargets {
        one_month: Some(PriceTarget {
            base: om_base,
            bear: om_base * (1.0 - om_band),
            bull: om_base * (1.0 + om_band),
            methodology: format!(
                "One-month (rolling) base = spot × (1 + PR_base/12), the twelve-month \
                 price-return leg prorated (v1 mechanics, dividends excluded); bull/bear \
                 ± {:.1}% from realized volatility [{}]",
                om_band * 100.0,
                SCENARIO_TARGET_PARAMETER_VERSION
            ),
        }),
        twelve_month: Some(PriceTarget {
            base: scenario.base,
            bear: scenario.bear,
            bull: scenario.bull,
            methodology: format!(
                "Twelve-month (rolling) scenarios = {driver_note} × {anchor_note}{bound_note}{floor_note} [{}]",
                SCENARIO_TARGET_PARAMETER_VERSION
            ),
        }),
    }
}

// ---- Risk tier and the capital-efficiency hurdle --------------------------------

/// Deterministic stock risk-tier assignment — Trade Opportunities' canonical
/// High / Low / else-Medium rule (`docs/trade-opportunities.md` §Starting parameters)
/// under Portfolio's **stated missing-input rule**: a leg whose input this job's
/// surface doesn't carry (event exposure; liquidity is enriching here) simply cannot
/// trigger, and a holding whose tier inputs are wholesale missing reads **Medium with
/// a logged tier-input gap** — the neutral-imputation stance, never a fabricated
/// High or Low.
pub fn assign_stock_tier(
    fin: &CompanyFinancials,
    m: &ComputedMetrics,
) -> (crate::portfolio::RiskTier, Vec<String>) {
    use crate::portfolio::RiskTier;
    let annual_vol = m.return_volatility.map(|v| v * ANNUALIZATION_FACTOR);
    let drawdown = max_drawdown(&fin.daily_closes, &fin.price_history);
    let profitable = fin.net_income.or(fin.operating_income).map(|v| v > 0.0);

    let inputs_present = [
        fin.market_cap.is_some(),
        annual_vol.is_some(),
        m.debt_to_equity.is_some(),
        profitable.is_some(),
        drawdown.is_some(),
    ];
    if !inputs_present.iter().any(|p| *p) {
        return (
            RiskTier::Medium,
            vec!["risk tier: every tier input missing — Medium imputed (logged gap)".to_string()],
        );
    }

    let high = fin.market_cap.map(|c| c < TIER_HIGH_MAX_MCAP).unwrap_or(false)
        || annual_vol.map(|v| v > TIER_HIGH_MIN_ANNUAL_VOL).unwrap_or(false)
        // Negative equity is levered beyond the equity base — maximal leverage, not
        // minimal. The naked `>` read it as passing neither leg, so a negative-book
        // issuer fell through to the Low conjunction below. Same stance as
        // `risk_score`'s `RISK_DEBT_EQUITY_BAND` guard, which scores it 0.
        || m.debt_to_equity.map(|d| !(0.0..=TIER_HIGH_MIN_DEBT_EQUITY).contains(&d)).unwrap_or(false)
        || profitable.map(|p| !p).unwrap_or(false)
        || drawdown.map(|d| d > TIER_HIGH_MIN_DRAWDOWN).unwrap_or(false);
    if high {
        return (crate::portfolio::RiskTier::High, vec![]);
    }

    // The Low conjunction requires each surface-carried leg present *and* passing;
    // the liquidity leg is absent from this job's surface, so it neither blocks nor
    // triggers (the missing-input rule).
    let low = fin.market_cap.map(|c| c > TIER_LOW_MIN_MCAP).unwrap_or(false)
        && profitable.unwrap_or(false)
        // Guarded on both ends: the High leg above already claims a negative
        // debt/equity, but the bound is stated here too so the Low conjunction reads
        // correctly on its own and survives any reordering of the legs.
        && m.debt_to_equity.map(|d| (0.0..TIER_LOW_MAX_DEBT_EQUITY).contains(&d)).unwrap_or(false)
        && annual_vol.map(|v| v < TIER_LOW_MAX_ANNUAL_VOL).unwrap_or(false);
    if low {
        (RiskTier::Low, vec![])
    } else {
        (RiskTier::Medium, vec![])
    }
}

/// Deterministic **priced-equity-fund** tier mapping (`docs/portfolio-analysis.md`
/// §Starting parameters, drafted): High on a **leveraged / inverse** structural flag,
/// annualized volatility > 40%, or maximum drawdown > 50%; Low on volatility < 25%
/// with **no structural flag of either kind** — an option-overlay flag bars Low
/// without forcing High (the doc keys the High leg to leveraged / inverse
/// specifically, while Low requires no structural flag at all); else Medium.
pub fn assign_fund_tier(
    leveraged_inverse: bool,
    structural_flag: bool,
    annual_vol: Option<f64>,
    drawdown: Option<f64>,
) -> crate::portfolio::RiskTier {
    use crate::portfolio::RiskTier;
    if leveraged_inverse
        || annual_vol.map(|v| v > TIER_HIGH_MIN_ANNUAL_VOL).unwrap_or(false)
        || drawdown.map(|d| d > TIER_HIGH_MIN_DRAWDOWN).unwrap_or(false)
    {
        RiskTier::High
    } else if !structural_flag
        && annual_vol.map(|v| v < TIER_LOW_MAX_ANNUAL_VOL).unwrap_or(false)
    {
        RiskTier::Low
    } else {
        RiskTier::Medium
    }
}

/// Maximum peak-to-trough drawdown over the available history (dated closes when
/// present, else the undated history), as a positive fraction. `None` on too little
/// history.
pub fn max_drawdown(dated: &[DatedValue], undated: &[f64]) -> Option<f64> {
    let closes: Vec<f64> = if !dated.is_empty() {
        dated.iter().map(|d| d.value).collect()
    } else {
        undated.to_vec()
    };
    if closes.len() < 2 {
        return None;
    }
    let mut peak = f64::MIN;
    let mut worst = 0.0_f64;
    for c in closes {
        if c > peak {
            peak = c;
        }
        if peak > 0.0 {
            worst = worst.max(1.0 - c / peak);
        }
    }
    Some(worst)
}

/// The tier-scaled hurdle premium (decimal ratio).
pub fn tier_premium(tier: crate::portfolio::RiskTier) -> f64 {
    match tier {
        crate::portfolio::RiskTier::Low => TIER_PREMIUM_LOW,
        crate::portfolio::RiskTier::Medium => TIER_PREMIUM_MEDIUM,
        crate::portfolio::RiskTier::High => TIER_PREMIUM_HIGH,
    }
}

/// The three-state capital-efficiency / dead-money read over the scenario total
/// returns (`docs/portfolio-analysis.md` §Starting parameters): **clears** when even
/// the bear case clears the hurdle, **fails** when even the bull case misses it (dead
/// money), **indeterminate** otherwise; the base-case **point test** is the separate
/// new-money admission read (entry decision — dispersion tolerance is exit-side
/// hysteresis, never a license for new capital).
pub fn hurdle_read(
    scenario: &ScenarioSet,
    dgs2: f64,
    tier: crate::portfolio::RiskTier,
) -> HurdleRead {
    use crate::portfolio::HurdleState;
    let hurdle = dgs2 + tier_premium(tier);
    let state = if scenario.tr_bear >= hurdle {
        HurdleState::Clears
    } else if scenario.tr_bull < hurdle {
        HurdleState::Fails
    } else {
        HurdleState::Indeterminate
    };
    HurdleRead {
        state,
        hurdle_rate: Some(hurdle),
        tr_bear: Some(scenario.tr_bear),
        tr_base: Some(scenario.tr_base),
        tr_bull: Some(scenario.tr_bull),
        admits_new_money: scenario.tr_base >= hurdle,
    }
}

/// Bound the feasible action set from **per-holding** engine inputs only
/// (`docs/portfolio-analysis.md` §Starting parameters — the feasible-set rule;
/// conviction is model-authored, so it can't pre-gate). The add family is offered
/// only when the new-money admission point test passes, the hurdle isn't `fails`
/// (dead money drops the family a fortiori at any grade), the grade isn't F, no
/// pre-profit overlay rule bars it (constrained runway / severe deterioration),
/// and no hard forensic trigger is tripped (a filing-classified restatement /
/// auditor change — the trip resolving only from the typed producer, never a
/// bare model assertion); *add aggressively* additionally needs an A/B grade.
/// Severe deterioration restricts the whole set to the exit family
/// `{trim, sell all}`. Since `portfolio-v9` the set carries **no book-level
/// term** — the retired concentration-headroom gate was whole-book context,
/// which is the future portfolio planner's domain. Rendered into the action
/// call's prompt as the ENGINE SET — the engine arm's own action stand-in walks
/// its rung into it, while an outside-the-set model choice persists with an
/// annotation on the audit, never a schema bar. Every grade test reads the
/// momentum-free letter.
pub fn feasible_actions(
    grade: Grade,
    hurdle: &HurdleRead,
    overlay_rules: Option<&crate::portfolio::pre_profit::OverlayConsequences>,
    hard_forensic: bool,
) -> Vec<Action> {
    use crate::portfolio::HurdleState;
    if overlay_rules.map(|r| r.exit_family_only).unwrap_or(false) {
        return vec![Action::SellAll, Action::Trim];
    }
    let mut set = vec![Action::SellAll, Action::Trim, Action::Hold];
    let dead_money = hurdle.state == HurdleState::Fails;
    let overlay_bar = overlay_rules.map(|r| r.bar_add_family).unwrap_or(false);
    let add_ok = hurdle.admits_new_money
        && !dead_money
        && grade != Grade::F
        && !overlay_bar
        && !hard_forensic;
    if add_ok {
        set.push(Action::Add);
        if matches!(grade, Grade::A | Grade::B) {
            set.push(Action::AddAggressively);
        }
    }
    set
}

// ---- The engine stand-in arm (the two-arm baseline) ---------------------------
//
// Mechanical counterparts for the three verdict fields only the model used to
// author — outlook, conviction, action — so every model-authored field has a
// deterministic, disclosed baseline the scoreboard can score it against
// (`docs/portfolio-analysis.md` §The holding verdict, the two-arm contract).
// All three are calibratable constants in the module's spirit: simple, bounded,
// legible — a glorified calculator, never judgment.

/// The stand-in outlook's session windows (trading days) and per-window flat
/// thresholds: a trailing return inside ±threshold reads neutral, outside reads
/// bullish/bearish by sign. ~1 / 6 / 12 months of sessions.
const OUTLOOK_WINDOWS: [(usize, f64); 3] = [(21, 0.02), (126, 0.05), (252, 0.08)];

/// The degradation count at or above which the stand-in conviction reads Low
/// (0 → High, 1–2 → Medium, ≥3 → Low).
const CONVICTION_LOW_AT: usize = 3;

/// The mechanical short / mid / long outlook: the trailing return over each of
/// [`OUTLOOK_WINDOWS`]'s session counts, read against its flat threshold. A window
/// the dated series cannot cover reads **neutral** — the rule's null, never a
/// fabricated direction (the series is the deep dated-closes leg the v2 anchor
/// join already fetches, so this adds no retrieval).
pub fn engine_outlook(daily_closes: &[DatedValue]) -> HorizonOutlook {
    let read = |sessions: usize, flat: f64| -> HorizonRead {
        if daily_closes.len() < sessions + 1 {
            return HorizonRead::Neutral;
        }
        let last = daily_closes[daily_closes.len() - 1].value;
        let first = daily_closes[daily_closes.len() - 1 - sessions].value;
        if first <= 0.0 || last <= 0.0 {
            return HorizonRead::Neutral;
        }
        let r = last / first - 1.0;
        if r > flat {
            HorizonRead::Bullish
        } else if r < -flat {
            HorizonRead::Bearish
        } else {
            HorizonRead::Neutral
        }
    };
    HorizonOutlook {
        short: read(OUTLOOK_WINDOWS[0].0, OUTLOOK_WINDOWS[0].1),
        mid: read(OUTLOOK_WINDOWS[1].0, OUTLOOK_WINDOWS[1].1),
        long: read(OUTLOOK_WINDOWS[2].0, OUTLOOK_WINDOWS[2].1),
    }
}

/// The mechanical conviction: a disclosed degradation count over the analysis's
/// own data-quality flags — completeness as confidence, never judgment. Counted:
/// an imputed letter axis, a non-rate-anchored target surface, a current-multiple
/// carry, a flat or clamp-flattened driver, a dispersion-floor widening, any
/// tier-input gap, an unscorable hurdle, and any dossier input gap. 0 → High,
/// 1–2 → Medium, ≥[`CONVICTION_LOW_AT`] → Low.
pub fn engine_conviction(out: &EngineOutput, input_gaps: &[String]) -> Conviction {
    use crate::portfolio::HurdleState;
    let mut count = 0usize;
    if out.low_confidence_grade {
        count += 1;
    }
    if !out.target_meta.rate_anchored {
        count += 1;
    }
    if out.target_meta.current_multiple_carry {
        count += 1;
    }
    if out.target_meta.flat_driver || out.target_meta.clamp_flattened {
        count += 1;
    }
    if out.target_meta.dispersion_floor_applied {
        count += 1;
    }
    if !out.tier_gaps.is_empty() {
        count += 1;
    }
    if out.hurdle.state == HurdleState::Unscorable {
        count += 1;
    }
    if !input_gaps.is_empty() {
        count += 1;
    }
    if count == 0 {
        Conviction::High
    } else if count < CONVICTION_LOW_AT {
        Conviction::Medium
    } else {
        Conviction::Low
    }
}

/// The mechanical action rung: grade × hurdle × admission over the engine's own
/// feasible machinery, tiebreak toward hold. F-grade or dead money leans exit
/// (sell-all only when both agree); an A/B grade whose base case clears the hurdle
/// and admits new money leans add (`Add` is the rule's top rung — the most
/// aggressive rung stays a judgment call no formula fakes); everything else holds.
/// The chosen rung is then walked toward hold until it sits inside
/// [`feasible_actions`] — the engine arm always obeys its own bars.
pub fn engine_action(
    grade: Grade,
    hurdle: &HurdleRead,
    overlay_rules: Option<&crate::portfolio::pre_profit::OverlayConsequences>,
    hard_forensic: bool,
) -> Action {
    use crate::portfolio::HurdleState;
    let dead_money = hurdle.state == HurdleState::Fails;
    let rule = if grade == Grade::F && dead_money {
        Action::SellAll
    } else if grade == Grade::F || dead_money {
        Action::Trim
    } else if matches!(grade, Grade::A | Grade::B)
        && hurdle.state == HurdleState::Clears
        && hurdle.admits_new_money
    {
        Action::Add
    } else {
        Action::Hold
    };
    let feasible = feasible_actions(grade, hurdle, overlay_rules, hard_forensic);
    if feasible.contains(&rule) {
        return rule;
    }
    // Walk toward hold; a set without hold (severe → exit family) takes trim.
    let toward_hold: &[Action] = match rule {
        Action::Add | Action::AddAggressively => &[Action::Hold, Action::Trim],
        Action::SellAll => &[Action::Trim, Action::Hold],
        _ => &[Action::Hold, Action::Trim],
    };
    toward_hold
        .iter()
        .copied()
        .find(|a| feasible.contains(a))
        .unwrap_or(Action::Trim)
}

/// Assemble the full engine stand-in arm for a priced holding — outlook off the
/// dated closes, conviction off the degradation flags, and the action rung, all
/// from per-holding inputs alone (`portfolio-v9`: no position/book context —
/// sizing is retired with the construction stage). Runs at the pipeline merge,
/// not inside [`analyze`]. `input_gaps` is the dossier's **assembled**
/// degraded-input list (financials gaps plus fund-metadata gaps, the
/// DGS10-history gap, and the listing-guard unverified note), so the conviction
/// read counts *any* dossier gap (`docs/portfolio-analysis.md` §Starting
/// parameters) — tier gaps stay out of it, since they carry their own counter.
pub fn engine_view(
    out: &EngineOutput,
    fin: &CompanyFinancials,
    input_gaps: &[String],
    overlay_rules: Option<&crate::portfolio::pre_profit::OverlayConsequences>,
    hard_forensic: bool,
    narrative_hype: bool,
) -> EngineView {
    let action = engine_action(out.grade, &out.hurdle, overlay_rules, hard_forensic);
    // The engine arm observes its own cap rules: a matched pre-profit conviction
    // ceiling binds the stand-in conviction exactly as the feasible-set bars bind
    // the stand-in action (`docs/portfolio-analysis.md` §The holding verdict —
    // caps bind the engine arm, annotate the model's). A tripped hard forensic
    // trigger is the strict Low ceiling, dominating any soft Medium ceiling
    // (hard > soft — `docs/portfolio-analysis.md` §Starting parameters). The
    // soft ceilings merge strictest-binds: a matched overlay Low outranks the
    // narrative read's Medium ([`NarrativeRead`] — conviction only, never an
    // action-set bar).
    use crate::portfolio::pre_profit::ConvictionCeiling;
    let ceiling = if hard_forensic {
        Some(ConvictionCeiling::Low)
    } else {
        let overlay = overlay_rules.and_then(|r| r.conviction_ceiling);
        let narrative = narrative_hype.then_some(ConvictionCeiling::Medium);
        match (overlay, narrative) {
            (Some(ConvictionCeiling::Low), _) => Some(ConvictionCeiling::Low),
            (Some(ConvictionCeiling::Medium), _) | (None, Some(_)) => {
                Some(ConvictionCeiling::Medium)
            }
            (None, None) => None,
        }
    };
    let (conviction, _) = crate::portfolio::pre_profit::clamp_conviction(
        engine_conviction(out, input_gaps),
        ceiling,
    );
    EngineView {
        outlook: engine_outlook(&fin.daily_closes),
        conviction,
        action,
    }
}

// ---- Narrative-vs-reality (the conviction-layer red-flag ratio) ---------------

/// The hype threshold: multiple expansion outrunning revisions by more than this
/// ratio trips the soft cap (drafted, calibratable — `docs/portfolio-analysis.md`
/// §Starting parameters).
pub const NARRATIVE_HYPE_RATIO: f64 = 1.5;

/// The multiple-expansion floor (decimal) below which the read never caps —
/// sub-threshold expansion is noise, not a re-rating (drafted, calibratable).
pub const NARRATIVE_MIN_MULTIPLE_EXPANSION: f64 = 0.05;

/// The minimum elapsed interval for a pace comparison — under it the two legs
/// are same-week noise, and the fallback's annualization would explode
/// (drafted, calibratable).
pub const NARRATIVE_MIN_ELAPSED_DAYS: i64 = 7;

/// Which form the read took (`docs/trade-opportunities.md` §The two
/// non-negotiables — the shared definition; Portfolio computes the held-name
/// form): the primary revisions-vs-multiple pace comparison, or the
/// operating-reality-vs-price fallback where analyst coverage is too thin to
/// read revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NarrativeForm {
    RevisionBased,
    OperatingReality,
}

/// What the ratio classified: *justified-expensive* (estimates underwrite the
/// re-rating), *hype* (the multiple outran flat or declining estimates — the
/// soft-cap signature), or *neutral* (no meaningful expansion to classify).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NarrativeClass {
    JustifiedExpensive,
    Hype,
    Neutral,
}

/// The narrative-vs-reality read (`docs/portfolio-analysis.md` §Starting
/// parameters — the conviction-layer caps): revision pace vs multiple change
/// since the prior run, both legs over the same elapsed interval (the
/// cadence-honest pace pair — the interval cancels in the ratio), falling back
/// to the company's own reported operating series against the annualized price
/// move where coverage is too thin. Conviction / risk evidence only — a
/// tripped cap is the suite's shared **soft Medium ceiling on the engine
/// arm's** mechanical conviction, an annotation beside the model's own value,
/// never a clamp on it, and never a letter input. As-built the cap fires on
/// the ratio alone: no leading-metric anchor producer exists in Portfolio, so
/// every holding reads anchor-absent — the anchor exception joins with the
/// research loop (ruled 2026-08-21).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NarrativeRead {
    pub form: NarrativeForm,
    /// The expansion leg (decimal): the forward-multiple change since the prior
    /// run on the revision form; the annualized price move on the fallback.
    pub expansion: f64,
    /// The reality leg (decimal): the consensus-mid revision over the same
    /// interval, or the TTM-revenue year-over-year growth on the fallback.
    pub reality: f64,
    /// expansion ÷ reality, where reality is positive — `None` on flat or
    /// declining reality (the ratio is unbounded there; classification says
    /// what that means).
    pub ratio: Option<f64>,
    pub classification: NarrativeClass,
    /// Elapsed days between the prior read's session and this run's.
    pub elapsed_days: i64,
    /// The matched soft rule, recorded when the cap fired (engine conviction
    /// ceiling Medium) — the audit's matched-cap-rule leg.
    pub matched_rule: Option<String>,
}

impl NarrativeRead {
    /// Whether the soft cap fired (the engine arm's Medium ceiling).
    pub fn hype_capped(&self) -> bool {
        self.matched_rule.is_some()
    }
}

/// TTM revenue summed over quarters `[start, start+4)` — `None` unless the
/// window is contiguous with every revenue line present.
fn ttm_revenue_window(fin: &CompanyFinancials, start: usize) -> Option<f64> {
    let rows = fin.quarterly_income.get(start..start + 4)?;
    if !quarters_contiguous(rows.iter().map(|r| r.period_end.as_str())) {
        return None;
    }
    rows.iter().map(|r| r.revenue).sum()
}

/// Compute the narrative-vs-reality read against the prior run's stored
/// comparator ([`QuickCheckBasis`]'s spot + consensus mid — both persisted, so
/// the pace pair needs no new history). `Err` is the typed unreadable reason
/// (a debut, a too-short interval, or neither form's legs resolving) — a gap,
/// never a fabricated neutral.
pub fn narrative_vs_reality(
    fin: &CompanyFinancials,
    spot: f64,
    prior_spot: Option<f64>,
    prior_consensus_eps_mid: Option<f64>,
    elapsed_days: Option<i64>,
) -> std::result::Result<NarrativeRead, String> {
    let prior_spot = prior_spot
        .filter(|s| s.is_finite() && *s > 0.0)
        .ok_or("no prior authoring-time spot (a debut, or a prior audit without a basis)")?;
    let elapsed_days =
        elapsed_days.ok_or("no readable elapsed interval since the prior read")?;
    if elapsed_days < NARRATIVE_MIN_ELAPSED_DAYS {
        return Err(format!(
            "only {elapsed_days} day(s) since the prior read (need {NARRATIVE_MIN_ELAPSED_DAYS})"
        ));
    }
    if !(spot.is_finite() && spot > 0.0) {
        return Err("no positive current price".to_string());
    }

    let mid_now = fin
        .consensus
        .as_ref()
        .and_then(|c| c.eps_mid)
        .filter(|m| m.is_finite() && *m > 0.0);
    let prior_mid = prior_consensus_eps_mid.filter(|m| m.is_finite() && *m > 0.0);

    let (form, expansion, reality) = match (mid_now, prior_mid) {
        // The primary form: forward-multiple change vs consensus revision, both
        // over the same interval — the ratio is interval-invariant.
        (Some(mid_now), Some(prior_mid)) => {
            let expansion = (spot / mid_now) / (prior_spot / prior_mid) - 1.0;
            let reality = mid_now / prior_mid - 1.0;
            (NarrativeForm::RevisionBased, expansion, reality)
        }
        // The thin-coverage fallback (`docs/trade-opportunities.md` §The two
        // non-negotiables): the company's own reported operating momentum (TTM
        // revenue YoY — an annual rate) against the price move annualized onto
        // the same basis.
        _ => {
            let ttm_now = ttm_revenue_window(fin, 0)
                .filter(|r| *r > 0.0)
                .ok_or("thin coverage and no contiguous current TTM revenue window")?;
            let ttm_prior = ttm_revenue_window(fin, 4)
                .filter(|r| *r > 0.0)
                .ok_or("thin coverage and no contiguous prior-year TTM revenue window")?;
            let reality = ttm_now / ttm_prior - 1.0;
            let expansion =
                (spot / prior_spot).powf(365.25 / elapsed_days as f64) - 1.0;
            (NarrativeForm::OperatingReality, expansion, reality)
        }
    };

    let ratio = (reality > 0.0).then(|| expansion / reality);
    let classification = if expansion < NARRATIVE_MIN_MULTIPLE_EXPANSION {
        NarrativeClass::Neutral
    } else if reality <= 0.0 || ratio.is_some_and(|r| r > NARRATIVE_HYPE_RATIO) {
        NarrativeClass::Hype
    } else {
        NarrativeClass::JustifiedExpensive
    };
    let matched_rule = (classification == NarrativeClass::Hype).then(|| {
        format!(
            "narrative-vs-reality hype: {} outran {} >{NARRATIVE_HYPE_RATIO}× (no \
             leading-metric anchor) — engine conviction capped Medium",
            match form {
                NarrativeForm::RevisionBased => "forward-multiple expansion",
                NarrativeForm::OperatingReality => "the annualized price move",
            },
            match form {
                NarrativeForm::RevisionBased => "estimate revisions",
                NarrativeForm::OperatingReality => "reported TTM revenue growth",
            },
        )
    });
    Ok(NarrativeRead {
        form,
        expansion,
        reality,
        ratio,
        classification,
        elapsed_days,
        matched_rule,
    })
}

// ---- Technology-event pre-flag (the input delta's repricing screen) -----------

/// The pre-flag's threshold multiple over interval-scaled realized volatility
/// (drafted, calibratable — `docs/portfolio-analysis.md` §Starting parameters).
pub const TECH_EVENT_SIGMA: f64 = 2.0;

/// The input delta's **technology-event pre-flag** record — an equity-holding
/// read flagging a possible third-party repricing event when the holding's
/// sector-relative move since the prior read exceeds
/// [`TECH_EVENT_SIGMA`] × its interval-scaled realized volatility (√-of-time
/// scaling, the suite's cadence-honest convention). The flag only adds the
/// conditional research topic; it asserts nothing
/// about the cause (`docs/portfolio-analysis.md` §Starting parameters).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TechEventPreFlag {
    pub fired: bool,
    /// The holding's sector-relative move since the prior read (decimal).
    pub relative_move: f64,
    /// The firing threshold (decimal): sigma × daily vol × √sessions.
    pub threshold: f64,
    /// Elapsed sessions between the prior read's session and the latest close,
    /// counted on the holding's own dated series.
    pub sessions: usize,
    /// The sector benchmark the move was read against (SPDR symbol).
    pub benchmark: String,
}

/// Evaluate the pre-flag from the holding's dated closes, its sector
/// benchmark's, and the engine's realized-volatility read (one vol basis per
/// holding — never a second definition). `prior_session` is the prior read's
/// **ET session date** (ISO). `Err` carries the typed unevaluable reason — a
/// gap, never a fired or clear flag.
pub fn tech_event_pre_flag(
    holding_closes: &[DatedValue],
    benchmark_closes: &[DatedValue],
    benchmark_symbol: &str,
    prior_session: &str,
    daily_return_volatility: Option<f64>,
) -> std::result::Result<TechEventPreFlag, String> {
    let vol = daily_return_volatility.ok_or("no realized-volatility read")?;
    if vol <= 0.0 {
        return Err("non-positive realized volatility".to_string());
    }
    let latest = holding_closes
        .last()
        .ok_or("no holding price history")?;
    let sessions = holding_closes
        .iter()
        .filter(|d| d.date.as_str() > prior_session && d.date <= latest.date)
        .count();
    if sessions == 0 {
        return Err("no elapsed sessions since the prior read".to_string());
    }
    let h0 = latest_on_or_before(holding_closes, prior_session)
        .ok_or("no holding close on or before the prior read")?;
    let b0 = latest_on_or_before(benchmark_closes, prior_session)
        .ok_or("no benchmark close on or before the prior read")?;
    let b1 = latest_on_or_before(benchmark_closes, &latest.date)
        .ok_or("no benchmark close for the current window")?;
    if h0 <= 0.0 || b0 <= 0.0 || b1 <= 0.0 || latest.value <= 0.0 {
        return Err("non-positive close in the window".to_string());
    }
    let relative_move = (latest.value / h0 - 1.0) - (b1 / b0 - 1.0);
    let threshold = TECH_EVENT_SIGMA * vol * (sessions as f64).sqrt();
    Ok(TechEventPreFlag {
        fired: relative_move.abs() > threshold,
        relative_move,
        threshold,
        sessions,
        benchmark: benchmark_symbol.to_string(),
    })
}

// ---- Options-activity signal (kept out of the grade) -------------------------

/// Compute the per-stock options-activity signal from the chain (`docs/schwab-integration.md`).
/// A rough activity *proxy* — put/call by volume and open interest, an at-the-money
/// IV read, and the put-minus-call IV skew — **never folded into the grade
/// sub-scores** until calibration shows it adds value; it grounds the narrative read
/// only. Any field is `None` when the chain lacked the data.
pub fn options_signal(chain: &OptionChain) -> OptionsSignal {
    let sum = |kind: OptionKind, f: fn(&crate::schwab::OptionQuote) -> f64| -> f64 {
        chain
            .contracts
            .iter()
            .filter(|c| c.kind == kind)
            .map(f)
            .sum()
    };
    let call_vol = sum(OptionKind::Call, |c| c.volume);
    let put_vol = sum(OptionKind::Put, |c| c.volume);
    let call_oi = sum(OptionKind::Call, |c| c.open_interest);
    let put_oi = sum(OptionKind::Put, |c| c.open_interest);

    let ratio = |put: f64, call: f64| if call > 0.0 { Some(put / call) } else { None };

    let avg_iv = |kind: OptionKind| -> Option<f64> {
        let ivs: Vec<f64> = chain
            .contracts
            .iter()
            .filter(|c| c.kind == kind)
            .filter_map(|c| c.implied_volatility)
            .collect();
        if ivs.is_empty() {
            None
        } else {
            Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
        }
    };
    let call_iv = avg_iv(OptionKind::Call);
    let put_iv = avg_iv(OptionKind::Put);
    let implied_volatility = average(&[call_iv, put_iv]);
    let iv_skew = match (put_iv, call_iv) {
        (Some(p), Some(c)) => Some(p - c),
        _ => None,
    };

    OptionsSignal {
        put_call_volume: ratio(put_vol, call_vol),
        put_call_open_interest: ratio(put_oi, call_oi),
        implied_volatility,
        iv_skew,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{HurdleState, RiskTier};
    use crate::schwab::{OptionQuote, OptionKind};

    #[test]
    fn metric_delta_is_exact_and_types_appearances() {
        let prior = ComputedMetrics {
            gross_margin: Some(0.42),
            net_margin: Some(0.10),
            pe_ratio: Some(21.5),
            ..Default::default()
        };
        let current = ComputedMetrics {
            gross_margin: Some(0.38), // moved
            net_margin: Some(0.10),   // unchanged — no entry
            pe_ratio: None,           // disappeared
            revenue_growth: Some(0.07), // appeared
            ..Default::default()
        };
        let delta = metric_delta(&prior, &current);
        let names: Vec<&str> = delta.iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["gross margin", "revenue growth", "P/E"]);
        let gm = &delta[0];
        assert_eq!((gm.old, gm.new), (Some(0.42), Some(0.38)));
        let pe = delta.iter().find(|c| c.name == "P/E").unwrap();
        assert_eq!((pe.old, pe.new), (Some(21.5), None));
    }

    #[test]
    fn metric_delta_of_identical_metrics_is_empty() {
        let m = ComputedMetrics {
            gross_margin: Some(0.42),
            debt_to_equity: Some(1.1),
            ..Default::default()
        };
        assert!(metric_delta(&m, &m.clone()).is_empty());
    }

    /// Quarterly dates walking back from mid-2026, newest first.
    fn quarter_ends(n: usize) -> Vec<String> {
        let anchors = [
            "2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30",
            "2025-06-30", "2025-03-31", "2024-12-31", "2024-09-30",
            "2024-06-30", "2024-03-31", "2023-12-31", "2023-09-30",
            "2023-06-30", "2023-03-31", "2022-12-31", "2022-09-30",
        ];
        anchors.iter().take(n).map(|s| s.to_string()).collect()
    }

    /// The run-level rate fixture: DGS2 4%, DGS10 4.5%, and a dated DGS10 history
    /// covering the anchor window (all decimal ratios).
    pub(crate) fn rates() -> RateAnchors {
        let history = quarter_ends(16)
            .into_iter()
            .rev()
            .map(|date| DatedValue { date, value: 0.04 })
            .collect();
        RateAnchors {
            dgs2: 0.04,
            dgs10: 0.045,
            dgs10_history: history,
            history_gap: None,
            ..Default::default()
        }
    }

    /// A healthy large-cap with a full v2 surface: 16 quarterly prints, dated closes,
    /// and a forward consensus — the driver ladder's rung 1.
    pub(crate) fn strong() -> CompanyFinancials {
        let ends = quarter_ends(16);
        let quarterly_income = ends
            .iter()
            .enumerate()
            .map(|(i, end)| QuarterlyIncomeRow {
                period_end: end.clone(),
                filing_date: None, // period end + the 45-day grace anchors the join
                revenue: Some(100.0 - i as f64),
                eps_diluted: Some(1.55 - 0.01 * i as f64),
                diluted_shares: Some(1.5e10),
                net_income: None,
                gross_profit: None,
                cost_of_revenue: None,
                operating_income: None,
            })
            .collect();
        // Dated closes: one per quarter end plus a recent print, rising over time.
        let mut daily_closes: Vec<DatedValue> = ends
            .iter()
            .rev()
            .enumerate()
            .map(|(i, end)| DatedValue {
                date: end.clone(),
                value: 130.0 + 4.0 * i as f64,
            })
            .collect();
        daily_closes.push(DatedValue { date: "2026-07-15".into(), value: 195.0 });
        CompanyFinancials {
            symbol: "AAPL".into(),
            // The shared fixture stands on a contiguous quarterly window, as the
            // production adopt path would stamp it.
            statement_basis: Some(crate::portfolio::StatementBasis::Ttm),
            current_price: Some(195.0),
            market_cap: Some(3.0e12),
            shares_outstanding: Some(1.5e10),
            revenue: Some(400.0),
            revenue_prior: Some(360.0),
            gross_profit: Some(180.0),
            operating_income: Some(120.0),
            net_income: Some(100.0),
            eps: Some(6.5),
            total_debt: Some(100.0),
            total_equity: Some(200.0),
            free_cash_flow: Some(95.0),
            pe_ratio: Some(28.0),
            ps_ratio: Some(7.5),
            pb_ratio: Some(6.0),
            price_history: vec![170.0, 175.0, 180.0, 188.0, 195.0],
            daily_closes,
            quarterly_income,
            quarterly_cash_flow: vec![],
            cash_and_equivalents: None,
            short_term_investments: None,
            consensus: Some(ConsensusEstimate {
                period_end: "2027-06-30".into(),
                eps_low: Some(6.0),
                eps_mid: Some(6.5),
                eps_high: Some(7.0),
                revenue_low: Some(420.0e9),
                revenue_mid: Some(430.0e9),
                revenue_high: Some(440.0e9),
                ..ConsensusEstimate::default()
            }),
            ttm_dividends_per_share: Some(1.0),
            gaps: vec![],
        }
    }

    #[test]
    fn strong_company_grades_and_computes_v2_targets() {
        match analyze(&strong(), &rates()) {
            EngineVerdict::Analyzed(out) => {
                for s in [
                    out.sub_scores.quality,
                    out.sub_scores.valuation,
                    out.sub_scores.momentum,
                    out.sub_scores.risk,
                ] {
                    assert!((0.0..=100.0).contains(&s), "{s}");
                }
                assert!(matches!(out.grade, Grade::A | Grade::B | Grade::C), "{:?}", out.grade);
                // The twelve-month target is the v2 scenario set: rate-anchored over
                // the full window, ordered, methodology versioned.
                let tm = out.price_targets.twelve_month.as_ref().unwrap();
                assert!(tm.bear <= tm.base && tm.base <= tm.bull, "ordered scenarios");
                assert!(tm.methodology.contains("spread-anchored"), "{}", tm.methodology);
                assert!(tm.methodology.contains(SCENARIO_TARGET_PARAMETER_VERSION));
                assert!(out.target_meta.rate_anchored);
                assert_eq!(out.target_meta.anchor_observations, ANCHOR_WINDOW_QUARTERS);
                assert_eq!(out.target_meta.driver_rung, "consensus forward EPS");
                assert!(!out.target_meta.flat_driver, "published low/high spread");
                // The one-month leg keeps the v1 mechanics off the price-return leg.
                let om = out.price_targets.one_month.as_ref().unwrap();
                let pr_base = tm.base / 195.0 - 1.0;
                assert!((om.base - 195.0 * (1.0 + pr_base / 12.0)).abs() < 1e-9);
                // The hurdle read is computed off the scenario TRs with the tier premium.
                assert_ne!(out.hurdle.state, crate::portfolio::HurdleState::Unscorable);
                assert!(out.hurdle.hurdle_rate.unwrap() > 0.04);
                // Grade rests on three real letter sub-scores — no low-confidence marker.
                assert!(!out.low_confidence_grade);
            }
            other => panic!("expected an analysis, got {other:?}"),
        }
    }

    #[test]
    fn grade_is_deterministic_for_the_same_inputs() {
        let a = analyze(&strong(), &rates());
        let b = analyze(&strong(), &rates());
        assert_eq!(a, b, "same financials always grade identically");
    }

    #[test]
    fn missing_price_abstains_below_the_evidence_floor() {
        let mut fin = strong();
        fin.current_price = None;
        match analyze(&fin, &rates()) {
            EngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("no current price"), "{reason}");
            }
            other => panic!("expected abstention, got {other:?}"),
        }
    }

    #[test]
    fn too_few_subscores_abstains() {
        // Only a price and a single multiple — one letter sub-score (valuation) at most.
        let fin = CompanyFinancials {
            symbol: "X".into(),
            current_price: Some(50.0),
            ps_ratio: Some(3.0),
            ..CompanyFinancials::default()
        };
        match analyze(&fin, &rates()) {
            EngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("sub-scores"), "{reason}");
            }
            other => panic!("expected abstention, got {other:?}"),
        }
    }

    #[test]
    fn no_admissible_driver_is_the_named_floor_reason() {
        // A gradeable surface with no consensus at all: neither ladder rung is
        // admissible, so the holding abstains under the named reason rather than
        // pricing off nothing (`docs/portfolio-analysis.md` §Evidence floor).
        let mut fin = strong();
        fin.consensus = None;
        match analyze(&fin, &rates()) {
            EngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("no-admissible-driver"), "{reason}");
            }
            other => panic!("expected abstention, got {other:?}"),
        }
    }

    #[test]
    fn negative_eps_consensus_skips_to_the_revenue_rung() {
        let mut fin = strong();
        let c = fin.consensus.as_mut().unwrap();
        c.eps_mid = Some(-0.50); // pre-profit: reciprocal-yield math is meaningless
        c.eps_low = None;
        c.eps_high = None;
        match analyze(&fin, &rates()) {
            EngineVerdict::Analyzed(out) => {
                assert_eq!(
                    out.target_meta.driver_rung,
                    "consensus forward revenue per share"
                );
            }
            other => panic!("expected the revenue rung, got {other:?}"),
        }
    }

    #[test]
    fn missing_consensus_spread_holds_the_driver_flat() {
        let mut fin = strong();
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = None;
        c.eps_high = None;
        match analyze(&fin, &rates()) {
            EngineVerdict::Analyzed(out) => {
                assert!(out.target_meta.flat_driver);
                let tm = out.price_targets.twelve_month.unwrap();
                assert!(tm.methodology.contains("held flat"), "{}", tm.methodology);
                // Scenario spread then comes from the multiple axis alone.
                assert!(tm.bear <= tm.base && tm.base <= tm.bull);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn driver_growth_is_clamped_by_the_sanity_bound() {
        // Trailing TTM EPS ≈ 6.14 (1.55+1.54+1.53+1.52); a wild 20.0 consensus mid
        // clamps to ttm × 1.35, and a collapsed 1.0 clamps to ttm × 0.75.
        let ttm: f64 = 1.55 + 1.54 + 1.53 + 1.52;
        let mut fin = strong();
        fin.consensus.as_mut().unwrap().eps_mid = Some(20.0);
        let drivers = driver_ladder(&fin).unwrap().drivers;
        assert!((drivers[1] - ttm * (1.0 + DRIVER_GROWTH_MAX)).abs() < 1e-9);
        fin.consensus.as_mut().unwrap().eps_mid = Some(1.0);
        let drivers = driver_ladder(&fin).unwrap().drivers;
        assert!((drivers[1] - ttm * (1.0 + DRIVER_GROWTH_MIN)).abs() < 1e-9);
    }

    // ---- Step-6e forward-assumption refinement ----

    #[test]
    fn a_supplement_fills_an_absent_driver_and_recomputes_targets() {
        // The charter case: no positive forward-EPS consensus — the ladder sat
        // on the revenue rung — and research supplies issued EPS guidance.
        let mut fin = strong();
        {
            let c = fin.consensus.as_mut().unwrap();
            c.eps_low = None;
            c.eps_mid = None;
            c.eps_high = None;
        }
        let rates = rates();
        let before = match analyze(&fin, &rates) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let input = ForwardAssumptionInput {
            metric: AssumptionMetric::ForwardEps,
            value: 7.4,
            units: "USD per share".into(),
            supersede: false,
            fact_type: "issued company guidance".into(),
            as_of: "2026-08-20".into(),
            source_url: "https://ir.example.com/guidance".into(),
        };
        let refined = refine_targets_with_assumption(&fin, &rates, &input).unwrap();
        assert!(refined.matched_rule.contains("supplement"));
        assert!(refined.matched_rule.contains("forward-EPS"));
        // The refined targets price the EPS rung now — a different surface
        // than the revenue-rung baseline.
        assert_ne!(
            refined.price_targets.twelve_month.as_ref().map(|t| t.base),
            before.price_targets.twelve_month.as_ref().map(|t| t.base),
            "the affected scenario target moved"
        );
    }

    #[test]
    fn the_conflict_policy_rejects_displacement_supersede_and_off_whitelist_facts() {
        let fin = strong(); // carries a positive EPS consensus
        let rates = rates();
        let mut input = ForwardAssumptionInput {
            metric: AssumptionMetric::ForwardEps,
            value: 9.0,
            units: "USD per share".into(),
            supersede: false,
            fact_type: "issued company guidance".into(),
            as_of: "2026-08-20".into(),
            source_url: "https://ir.example.com/guidance".into(),
        };
        // A supplement may never displace a present feed value.
        let err = refine_targets_with_assumption(&fin, &rates, &input).unwrap_err();
        assert!(err.contains("may not displace"), "{err}");
        // A supersede rejects on the named unverifiable condition —
        // structured-wins is the default.
        input.supersede = true;
        let err = refine_targets_with_assumption(&fin, &rates, &input).unwrap_err();
        assert!(err.contains("no as-of date"), "{err}");
        // An off-whitelist fact type rejects even where the driver is absent.
        let mut no_eps = strong();
        {
            let c = no_eps.consensus.as_mut().unwrap();
            c.eps_low = None;
            c.eps_mid = None;
            c.eps_high = None;
        }
        input.supersede = false;
        input.fact_type = "analyst blog estimate".into();
        let err = refine_targets_with_assumption(&no_eps, &rates, &input).unwrap_err();
        assert!(err.contains("whitelist"), "{err}");
        // A malformed as-of rejects.
        input.fact_type = "issued company guidance".into();
        input.as_of = "next quarter".into();
        let err = refine_targets_with_assumption(&no_eps, &rates, &input).unwrap_err();
        assert!(err.contains("ISO date"), "{err}");
    }

    #[test]
    fn assumption_units_normalize_or_reject_before_the_fill() {
        // Magnitude words scale a revenue fact deterministically.
        assert_eq!(
            normalized_assumption_value(AssumptionMetric::ForwardRevenue, 4.5, "USD billions"),
            Ok(4.5e9)
        );
        assert_eq!(
            normalized_assumption_value(AssumptionMetric::ForwardRevenue, 850.0, "million USD"),
            Ok(850.0e6)
        );
        // A bare absolute-dollar figure passes unchanged.
        assert_eq!(
            normalized_assumption_value(AssumptionMetric::ForwardRevenue, 4.5e9, "USD"),
            Ok(4.5e9)
        );
        // A bare small revenue value is unit-ambiguous — "4.5" for $4.5B must
        // never ride into the driver unscaled.
        let err =
            normalized_assumption_value(AssumptionMetric::ForwardRevenue, 4.5, "USD").unwrap_err();
        assert!(err.contains("unit-ambiguous"), "{err}");
        // A per-share fact rejects any magnitude token outright.
        let err = normalized_assumption_value(AssumptionMetric::ForwardEps, 7.4, "USD millions")
            .unwrap_err();
        assert!(err.contains("per-share"), "{err}");
        assert_eq!(
            normalized_assumption_value(AssumptionMetric::ForwardEps, 7.4, "USD per share"),
            Ok(7.4)
        );
        // Conflicting magnitudes reject rather than guessing.
        let err = normalized_assumption_value(
            AssumptionMetric::ForwardRevenue,
            4.5,
            "billion (prior: million)",
        )
        .unwrap_err();
        assert!(err.contains("conflicting"), "{err}");
        // Empty units reject for either driver, and a non-monetary unit can
        // never fill EPS or revenue.
        let err = normalized_assumption_value(AssumptionMetric::ForwardEps, 7.4, "  ").unwrap_err();
        assert!(err.contains("no units"), "{err}");
        let err = normalized_assumption_value(AssumptionMetric::ForwardEps, 7.4, "vehicles")
            .unwrap_err();
        assert!(err.contains("non-per-share"), "{err}");
        let err = normalized_assumption_value(AssumptionMetric::ForwardRevenue, 2.0e6, "vehicles")
            .unwrap_err();
        assert!(err.contains("no currency or magnitude"), "{err}");
        // The whitelist matches whole tokens — "unfiled rumor" never satisfies
        // `filed` — and negating / hedging tokens disqualify outright.
        assert!(!assumption_fact_whitelisted("unfiled rumor"));
        assert!(assumption_fact_whitelisted("filed figure (10-Q)"));
        assert!(assumption_fact_whitelisted("issued company guidance"));
        assert!(!assumption_fact_whitelisted("not guidance"));
        assert!(!assumption_fact_whitelisted("withdrawn guidance"));
        assert!(!assumption_fact_whitelisted("rumored contract"));

        // End to end: the billions-stated supplement fills the driver scaled.
        let mut fin = strong();
        {
            let c = fin.consensus.as_mut().unwrap();
            c.revenue_low = None;
            c.revenue_mid = None;
            c.revenue_high = None;
        }
        let rates = rates();
        let input = ForwardAssumptionInput {
            metric: AssumptionMetric::ForwardRevenue,
            value: 4.5,
            units: "USD billions".into(),
            supersede: false,
            fact_type: "issued company guidance".into(),
            as_of: "2026-08-20".into(),
            source_url: "https://ir.example.com/guidance".into(),
        };
        let refined = refine_targets_with_assumption(&fin, &rates, &input).unwrap();
        assert!(refined.matched_rule.contains("4500000000"), "{}", refined.matched_rule);
    }

    /// The attempt-2 RKT shape: recovered current earnings against a trail of
    /// near-zero-EPS quarters whose trailing multiples are astronomical.
    fn trough_artifact_fin() -> CompanyFinancials {
        let mut fin = strong();
        for (i, row) in fin.quarterly_income.iter_mut().enumerate() {
            // Newest four quarters recovered; everything older is noise-scale.
            row.eps_diluted = Some(if i < 4 { 1.0 } else { 0.01 });
        }
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = Some(4.0);
        c.eps_mid = Some(4.2);
        c.eps_high = Some(4.4);
        fin
    }

    #[test]
    fn artifact_anchor_multiples_are_sanity_bounded_and_counted() {
        // Current TTM EPS = 4.0 at spot 195 → current multiple 48.75, cap ≈ 146×.
        // The nine windows containing near-zero quarters anchor at 170×–4,800×
        // and are dropped; the three recovered-era anchors (≈ 49×/62×/90×) stay.
        let fin = trough_artifact_fin();
        let rates = rates();
        let m = compute_metrics(&fin);
        let bundle = match scenario_targets_v2(195.0, &fin, &rates, &m) {
            TargetOutcome::Computed(b) => b,
            TargetOutcome::NoAdmissibleDriver => panic!("fixture must compute"),
        };
        assert_eq!(bundle.meta.anchor_bounded, 9, "the artifact windows drop");
        assert!(
            !bundle.meta.rate_anchored,
            "3 surviving anchors sit below the {MIN_ANCHOR_OBSERVATIONS}-observation floor"
        );
        // The base prices off surviving history (P50 ≈ 62×), not the artifact
        // percentiles that produced attempt 2's +1503% base.
        let tm = bundle.targets.twelve_month.as_ref().unwrap();
        assert!(
            tm.base < 195.0 * 3.0,
            "bounded base must be same-order with spot, got {}",
            tm.base
        );
        assert!(
            tm.methodology.contains("dropped by the multiple sanity bound"),
            "{}",
            tm.methodology
        );
        // Unreleased without corroboration: fixture consensus has periods_used 0.
        assert!(!bundle.meta.clamp_released);
    }

    /// The attempt-2 GM shape: a recent earnings trough (current multiple far
    /// above the anchor window's own normal-era multiples, trailing print
    /// depressed against the window's earning power) with a recovery consensus
    /// the growth clamp would otherwise crush. Corroboration is the per-rung
    /// row count — `periods_used` deliberately stays 2 in every variant, so a
    /// passing release proves the gate reads `eps_mid_rows`, never the blend
    /// count (Codex round 1, finding 2).
    fn recent_trough_fin(eps_mid_rows: u8) -> CompanyFinancials {
        let mut fin = strong();
        for (i, row) in fin.quarterly_income.iter_mut().enumerate() {
            // Newest four quarters troughed; the older history is normal-era.
            row.eps_diluted = Some(if i < 4 { 0.5 } else { 2.5 });
        }
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = Some(7.5);
        c.eps_mid = Some(8.0);
        c.eps_high = Some(8.5);
        c.periods_used = 2;
        c.eps_mid_rows = eps_mid_rows;
        fin
    }

    #[test]
    fn corroborated_recovery_consensus_releases_the_trough_clamp() {
        // Current TTM EPS = 2.0 at spot 195 → current multiple 97.5, far above
        // every anchor multiple (≈ 13×–47×), and the trailing print is a fifth
        // of the window's 10.0 earning power: the full trough signature. With
        // two EPS-carrying rows the clamp releases and the 8.0 consensus prices
        // raw; with one it stays clamped to 2.0 × 1.35 = 2.7.
        let rates = rates();
        let released_fin = recent_trough_fin(2);
        let m = compute_metrics(&released_fin);
        let released = match scenario_targets_v2(195.0, &released_fin, &rates, &m) {
            TargetOutcome::Computed(b) => b,
            TargetOutcome::NoAdmissibleDriver => panic!("fixture must compute"),
        };
        assert!(released.meta.clamp_released);
        assert!(
            !released.meta.clamp_flattened,
            "the release supersedes the collapse record"
        );
        assert_eq!(released.basis.drivers, [7.5, 8.0, 8.5], "unclamped consensus priced");
        assert!(
            released
                .targets
                .twelve_month
                .as_ref()
                .unwrap()
                .methodology
                .contains("growth clamp released"),
            "{}",
            released.targets.twelve_month.as_ref().unwrap().methodology
        );

        // One EPS-carrying row is not corroboration — even though periods_used
        // still reads 2 (the blend count a single estimate can ride).
        let clamped_fin = recent_trough_fin(1);
        let clamped = match scenario_targets_v2(195.0, &clamped_fin, &rates, &m) {
            TargetOutcome::Computed(b) => b,
            TargetOutcome::NoAdmissibleDriver => panic!("fixture must compute"),
        };
        assert!(!clamped.meta.clamp_released, "one EPS-carrying row is not corroboration");
        assert!(clamped.meta.clamp_flattened, "the clamp collapsed the published spread");
        let released_base = released.targets.twelve_month.as_ref().unwrap().base;
        let clamped_base = clamped.targets.twelve_month.as_ref().unwrap().base;
        assert!(
            released_base > clamped_base * 2.0,
            "the release must lift the crushed base: {released_base} vs {clamped_base}"
        );
    }

    #[test]
    fn a_price_rally_with_earnings_intact_never_releases_the_clamp() {
        // The rich-multiple rally: earnings flat across the whole window (the
        // trailing print IS the window's earning power) while the rising closes
        // put the current multiple above the anchor P75 — the multiple
        // signature alone reads true, and releasing here would remove the
        // sanity bound exactly when valuation is stretched. The direct trough
        // test must veto it.
        let mut fin = strong();
        for row in fin.quarterly_income.iter_mut() {
            row.eps_diluted = Some(1.0); // TTM 4.0 everywhere — no trough
        }
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = Some(7.5);
        c.eps_mid = Some(8.0); // +100% vs trailing — the clamp bites
        c.eps_high = Some(8.5);
        c.periods_used = 2;
        c.eps_mid_rows = 2; // fully corroborated — the veto must come from the print
        let rates = rates();
        let m = compute_metrics(&fin);
        let bundle = match scenario_targets_v2(195.0, &fin, &rates, &m) {
            TargetOutcome::Computed(b) => b,
            TargetOutcome::NoAdmissibleDriver => panic!("fixture must compute"),
        };
        assert!(
            !bundle.meta.clamp_released,
            "a rally is not a trough — earnings never fell"
        );
        assert!(bundle.meta.clamp_flattened, "the clamp stays in force");
    }

    #[test]
    fn a_downward_rerating_never_releases_the_clamp() {
        // The attempt-2 CRM shape: current multiple BELOW the anchor window's
        // rich end (the multiple re-rated down; the trailing print did not
        // collapse) — corroborated consensus alone must not release, or the
        // release would compound the anchors' own regime staleness.
        let mut fin = strong();
        for (i, row) in fin.quarterly_income.iter_mut().enumerate() {
            // Newest four quarters strong (current multiple ≈ 24×); the older
            // history earned less (anchor multiples ≈ 40×).
            row.eps_diluted = Some(if i < 4 { 2.0 } else { 1.0 });
        }
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = Some(11.0);
        c.eps_mid = Some(12.0);
        c.eps_high = Some(13.0);
        c.periods_used = 2;
        let rates = rates();
        let m = compute_metrics(&fin);
        let bundle = match scenario_targets_v2(195.0, &fin, &rates, &m) {
            TargetOutcome::Computed(b) => b,
            TargetOutcome::NoAdmissibleDriver => panic!("fixture must compute"),
        };
        assert!(!bundle.meta.clamp_released);
        assert_eq!(bundle.meta.anchor_bounded, 0, "a re-rating drops nothing");
    }

    #[test]
    fn canonicalize_statements_restores_newest_first_and_resolves_restatements() {
        // Reversed wire order with the newest income period served twice (a
        // restatement): canonicalization sorts both statement vecs newest-first
        // in place and keeps the later-filed print, never the wire head.
        let mut fin = strong();
        fin.quarterly_cash_flow = fin
            .quarterly_income
            .iter()
            .take(4)
            .map(|r| QuarterlyCashFlowRow {
                period_end: r.period_end.clone(),
                filing_date: None,
                free_cash_flow: Some(1.0),
                operating_cash_flow: None,
                capex: None,
            })
            .collect();
        let newest = fin.quarterly_income[0].period_end.clone();
        fin.quarterly_income[0].filing_date = Some("2026-07-01".into());
        let mut restated = fin.quarterly_income[0].clone();
        restated.eps_diluted = Some(9.99);
        restated.filing_date = Some("2026-08-01".into());
        fin.quarterly_income.reverse();
        fin.quarterly_income.push(restated); // the restatement arrives at the tail
        fin.quarterly_cash_flow.reverse();

        canonicalize_statements(&mut fin);
        assert_eq!(fin.quarterly_income[0].period_end, newest);
        assert_eq!(fin.quarterly_income[0].eps_diluted, Some(9.99), "latest filing wins");
        assert_eq!(
            fin.quarterly_income.iter().filter(|r| r.period_end == newest).count(),
            1,
            "the duplicated period deduplicates"
        );
        assert_eq!(fin.quarterly_cash_flow[0].period_end, newest);
    }

    #[test]
    fn the_driver_ladder_reads_the_canonical_statement_order() {
        // The wire-order parity guarantee: driver_ladder reads fin order directly,
        // relying on the choke-point canonicalization upstream. A clamp-tripping
        // consensus makes the trailing prints observable — a reversed feed left
        // uncanonicalized would clamp against the OLDEST four quarters instead.
        let ttm: f64 = 1.55 + 1.54 + 1.53 + 1.52;
        let mut canonical = strong();
        canonical.consensus.as_mut().unwrap().eps_mid = Some(20.0);
        let mut shuffled = canonical.clone();
        shuffled.quarterly_income.reverse();
        canonicalize_statements(&mut shuffled);
        let a = driver_ladder(&canonical).unwrap();
        let b = driver_ladder(&shuffled).unwrap();
        assert_eq!(a.drivers, b.drivers);
        assert!((b.drivers[1] - ttm * (1.0 + DRIVER_GROWTH_MAX)).abs() < 1e-9);
    }

    #[test]
    fn the_ntm_selection_record_is_persisted_on_the_target_meta() {
        // A live-parsed consensus (periods_used > 0) lands its row count and near
        // weight on the persisted meta — the stored run's driver provenance; a
        // hand-built fixture (periods_used 0) records the row count floor with no
        // fabricated weight.
        let mut fin = strong();
        {
            let c = fin.consensus.as_mut().unwrap();
            c.periods_used = 2;
            c.near_weight = 0.3;
        }
        match analyze(&fin, &rates()) {
            EngineVerdict::Analyzed(out) => {
                assert_eq!(out.target_meta.consensus_rows, Some(2));
                assert_eq!(out.target_meta.consensus_near_weight, Some(0.3));
            }
            other => panic!("{other:?}"),
        }
        match analyze(&strong(), &rates()) {
            EngineVerdict::Analyzed(out) => {
                assert_eq!(out.target_meta.consensus_rows, Some(1), "fixture floor");
                assert_eq!(out.target_meta.consensus_near_weight, None);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_clamp_flattened_published_spread_is_recorded_distinctly() {
        // Low/mid/high all above the growth-clamp ceiling collapse to ttm × 1.35 — a
        // published spread flattened by the clamp, not by the consensus, and the
        // record must tell the two apart (the calibration-data honesty the flat-target
        // finding exposed).
        let mut fin = strong();
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = Some(18.0);
        c.eps_mid = Some(20.0);
        c.eps_high = Some(22.0);
        let read = driver_ladder(&fin).unwrap();
        assert!(!read.flat_driver, "the spread was published");
        assert!(read.clamp_flattened, "…but the clamp collapsed it");
        assert!((read.drivers[0] - read.drivers[2]).abs() < 1e-12);
        // The healthy fixture spread is not flagged.
        let read = driver_ladder(&strong()).unwrap();
        assert!(!read.clamp_flattened);
    }

    #[test]
    fn a_half_published_spread_reads_flat_on_both_legs() {
        // One published leg beside a missing one must not spread the drivers
        // while recording `flat_driver`: the record would misdescribe the band,
        // and a clamp collapse on the published leg could never be recorded
        // (`clamp_collapse` is suppressed on flat reads). Either leg missing →
        // both hold at mid — the doc's "held flat".
        let all_flat = |read: &DriverRead| {
            (read.drivers[0] - read.drivers[1]).abs() < 1e-12
                && (read.drivers[2] - read.drivers[1]).abs() < 1e-12
        };
        let mut fin = strong();
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = Some(5.0);
        c.eps_high = None;
        let read = driver_ladder(&fin).unwrap();
        assert!(read.flat_driver);
        assert!(!read.clamp_flattened);
        assert!(all_flat(&read), "both legs hold at mid: {:?}", read.drivers);
        // The opposite missing leg reads identically.
        let mut fin = strong();
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = None;
        c.eps_high = Some(9.0);
        let read = driver_ladder(&fin).unwrap();
        assert!(read.flat_driver);
        assert!(all_flat(&read), "both legs hold at mid: {:?}", read.drivers);
        // The revenue rung shares the convention (EPS legs removed so rung 2
        // fires on the published revenue mid + one-sided spread).
        let mut fin = strong();
        let c = fin.consensus.as_mut().unwrap();
        c.eps_low = None;
        c.eps_mid = None;
        c.eps_high = None;
        c.revenue_high = None;
        let read = driver_ladder(&fin).unwrap();
        assert!(!read.use_eps, "rung 2 must be the driver under an EPS-less consensus");
        assert!(read.flat_driver);
        assert!(all_flat(&read), "both legs hold at mid: {:?}", read.drivers);
    }

    #[test]
    fn inverse_spread_mapping_orders_the_multiples() {
        // Nine spread observations from wide (cheap) to narrow (rich): the inverse
        // mapping must give M_bear ≤ M_base ≤ M_bull without sorting prices.
        let observations: Vec<AnchorObservation> = (0..9)
            .map(|i| {
                let spread = 0.06 - 0.005 * i as f64; // 6% down to 2%
                AnchorObservation { spread: Some(spread), raw_multiple: 1.0 / (spread + 0.045) }
            })
            .collect();
        let s = spread_anchored_scenarios(100.0, [5.0, 5.0, 5.0], &observations, 0.045, 0.0, 0.0);
        assert!(s.rate_anchored);
        assert_eq!(s.degenerate_scenarios, 0);
        assert!(!s.monotonicity_repaired, "inverse map orders without repair");
        assert!(s.bear < s.base && s.base < s.bull);
    }

    #[test]
    fn degenerate_denominator_falls_back_per_scenario_and_is_recorded() {
        // Spreads near −DGS10: the reciprocal denominator collapses below ε, so the
        // guarded scenarios take their raw multiple percentiles instead.
        let observations: Vec<AnchorObservation> = (0..9)
            .map(|i| AnchorObservation {
                spread: Some(-0.041 - 0.0005 * i as f64), // denom = spread + 0.045 < 0.01
                raw_multiple: 20.0 + i as f64,
            })
            .collect();
        let s = spread_anchored_scenarios(100.0, [5.0, 5.0, 5.0], &observations, 0.045, 0.0, 0.0);
        assert!(s.rate_anchored);
        assert_eq!(s.degenerate_scenarios, 3, "every scenario hit the ε guard");
        assert!(s.bear <= s.base && s.base <= s.bull, "direct raw map holds the order");
    }

    #[test]
    fn a_thin_window_drops_the_rate_correction() {
        let observations: Vec<AnchorObservation> = (0..5)
            .map(|i| AnchorObservation { spread: Some(0.01), raw_multiple: 18.0 + i as f64 })
            .collect();
        let s = spread_anchored_scenarios(100.0, [5.0, 5.5, 6.0], &observations, 0.045, 0.0, 0.0);
        assert!(!s.rate_anchored, "below the 8-observation floor");
        assert_eq!(s.anchor_observations, 5);
        assert!(!s.current_multiple_carry);
        assert!(s.bear <= s.base && s.base <= s.bull);
    }

    #[test]
    fn a_failed_dgs10_join_falls_back_to_raw_percentiles_not_the_carry() {
        // Twelve driver-admissible quarters whose dated-rate join all failed (a
        // failed DGS10 history request): the fallback must read the real raw-multiple
        // history — never degrade straight to the current-multiple carry
        // (`docs/portfolio-analysis.md` §Starting parameters).
        let observations: Vec<AnchorObservation> = (0..12)
            .map(|i| AnchorObservation { spread: None, raw_multiple: 14.0 + i as f64 })
            .collect();
        let s = spread_anchored_scenarios(100.0, [5.0, 5.0, 5.0], &observations, 0.045, 0.0, 0.0);
        assert!(!s.rate_anchored, "no dated-rate anchors");
        assert_eq!(s.anchor_observations, 0);
        assert_eq!(s.raw_observations, 12);
        assert!(!s.current_multiple_carry, "raw history must back the fallback");
        // Direct-mapped raw percentiles over 14..=25 (flat driver 5.0): P25/P50/P75.
        assert!((s.bear - 5.0 * percentile(&(0..12).map(|i| 14.0 + i as f64).collect::<Vec<_>>(), 0.25)).abs() < 1e-9);
        assert!(s.bear < s.base && s.base < s.bull);
    }

    #[test]
    fn no_anchor_history_carries_the_current_multiple() {
        let s = spread_anchored_scenarios(100.0, [6.0, 6.5, 7.0], &[], 0.045, 2.0, 0.0);
        assert!(s.current_multiple_carry);
        // Carry multiple = spot / base driver, so the base scenario lands on spot and
        // the spread comes from driver dispersion alone.
        assert!((s.base - 100.0).abs() < 1e-9);
        assert!(s.bear < s.base && s.base < s.bull);
        // TR decomposition: (P + forward income) / spot − 1.
        assert!((s.tr_base - (100.0 + 2.0) / 100.0 + 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_flat_carry_surface_is_widened_to_the_dispersion_floor() {
        // Flat drivers on the carry path: both scenario axes collapsed, so without
        // the floor bear == base == bull and the three-state hurdle degenerates into
        // a point test (the live-run flat-target syndrome). The floor widens each
        // side to ± the half-spread around base, recorded.
        let s = spread_anchored_scenarios(100.0, [5.0, 5.0, 5.0], &[], 0.045, 0.0, 0.08);
        assert!(s.current_multiple_carry);
        assert!(s.dispersion_floor_applied);
        assert!((s.base - 100.0).abs() < 1e-9);
        assert!((s.bear - 92.0).abs() < 1e-9);
        assert!((s.bull - 108.0).abs() < 1e-9);
        // The TR legs read the widened band, so `fails` needs the bull leg to miss
        // for real, not by construction.
        assert!(s.tr_bear < s.tr_base && s.tr_base < s.tr_bull);
    }

    #[test]
    fn a_wide_observed_band_is_never_narrowed_by_the_floor() {
        // Real dispersion wider than the floor on both sides: the floor must be a
        // widen-only guard, never a haircut on honest scenario spread.
        let observations: Vec<AnchorObservation> = (0..9)
            .map(|i| {
                let spread = 0.06 - 0.005 * i as f64;
                AnchorObservation { spread: Some(spread), raw_multiple: 1.0 / (spread + 0.045) }
            })
            .collect();
        let with_floor =
            spread_anchored_scenarios(100.0, [5.0, 5.0, 5.0], &observations, 0.045, 0.0, 0.02);
        let without =
            spread_anchored_scenarios(100.0, [5.0, 5.0, 5.0], &observations, 0.045, 0.0, 0.0);
        assert!(!with_floor.dispersion_floor_applied);
        assert!((with_floor.bear - without.bear).abs() < 1e-12);
        assert!((with_floor.bull - without.bull).abs() < 1e-12);
    }

    #[test]
    fn dispersion_floor_scales_with_volatility_inside_the_bounds() {
        // No vol → the lower bound; a calm large cap stays near it; a volatile name
        // scales up; an extreme one clamps at the ceiling.
        assert!((dispersion_floor(None) - 0.05).abs() < 1e-12);
        let calm = dispersion_floor(Some(0.004)); // ~6.3% annualized → below the min
        assert!((calm - 0.05).abs() < 1e-12);
        let vol = dispersion_floor(Some(0.015)); // ~23.8% annualized → ~0.119
        assert!((vol - 0.015 * ANNUALIZATION_FACTOR * 0.5).abs() < 1e-12);
        let extreme = dispersion_floor(Some(0.10));
        assert!((extreme - 0.20).abs() < 1e-12);
    }

    #[test]
    fn grade_bands_are_monotone_and_momentum_free() {
        let f = |v: f64, momentum: f64| {
            grade_from_subscores(&SubScores {
                quality: v,
                valuation: v,
                momentum,
                risk: v,
            })
        };
        assert_eq!(f(95.0, 0.0), Grade::A);
        assert_eq!(f(72.0, 0.0), Grade::B);
        assert_eq!(f(60.0, 0.0), Grade::C);
        assert_eq!(f(45.0, 0.0), Grade::D);
        assert_eq!(f(10.0, 0.0), Grade::F);
        // Momentum no longer moves the letter — the settled ex-momentum re-weight.
        assert_eq!(f(72.0, 0.0), f(72.0, 100.0));
    }

    /// The letter cutoffs are ≥-inclusive at exactly A 85 / B 70 / C 55 / D 40
    /// (`docs/portfolio-analysis.md` §Starting parameters), certified with the
    /// grade-band shadow-tune slice.
    #[test]
    fn letter_cutoffs_are_inclusive_at_the_documented_boundaries() {
        let at = |v: f64| {
            grade_from_subscores(&SubScores {
                quality: v,
                valuation: v,
                momentum: 0.0,
                risk: v,
            })
        };
        assert_eq!(at(GRADE_A), Grade::A);
        assert_eq!(at(GRADE_A - 0.01), Grade::B);
        assert_eq!(at(GRADE_B), Grade::B);
        assert_eq!(at(GRADE_B - 0.01), Grade::C);
        assert_eq!(at(GRADE_C), Grade::C);
        assert_eq!(at(GRADE_C - 0.01), Grade::D);
        assert_eq!(at(GRADE_D), Grade::D);
        assert_eq!(at(GRADE_D - 0.01), Grade::F);
    }

    /// A missing sub-score is imputed to the neutral midpoint (50) — the composite
    /// divides by the full weight sum, never renormalizes over the present axes —
    /// and the letter carries the visible low-confidence marker
    /// (`docs/portfolio-analysis.md` §Starting parameters).
    #[test]
    fn missing_subscore_imputes_neutral_and_marks_low_confidence() {
        let mut fin = strong();
        // Kill both quality legs (margins) while valuation/risk stay computable;
        // the multiples ride the fixture's direct pe/ps/pb fields.
        fin.net_income = None;
        fin.gross_profit = None;
        let EngineVerdict::Analyzed(out) = analyze(&fin, &rates()) else {
            panic!("two real sub-scores must still grade");
        };
        assert_eq!(out.sub_scores.quality, 50.0, "imputed to the neutral midpoint");
        assert!(out.low_confidence_grade, "imputed axis must mark the letter");
        // The letter is exactly the roll-up over the imputed struct — the
        // full-weight-sum arithmetic, not a present-axes renormalization.
        assert_eq!(out.grade, grade_from_subscores(&out.sub_scores));
        // And a fully-supplied strong company carries no marker.
        let EngineVerdict::Analyzed(full) = analyze(&strong(), &rates()) else {
            panic!("strong fixture grades");
        };
        assert!(!full.low_confidence_grade);
    }

    /// A negative P/E (no earnings) scores low — never "cheap", never off the scale
    /// (`docs/portfolio-analysis.md` §Starting parameters).
    #[test]
    fn negative_pe_scores_low_never_cheap() {
        let m = |pe: Option<f64>| ComputedMetrics {
            pe_ratio: pe,
            ..ComputedMetrics::default()
        };
        let negative = valuation_score(&m(Some(-5.0))).unwrap();
        let cheap = valuation_score(&m(Some(12.0))).unwrap();
        assert_eq!(negative, 20.0, "the fixed low score for a loss-maker");
        assert!(negative < cheap, "a loss-maker must never outscore a cheap earner");
    }

    /// The tier legs read a negative debt/equity the same way `risk_score` does —
    /// as maximal leverage. The naked `>` / `<` comparisons passed neither the High
    /// leg nor failed the Low one, so a negative-book large cap landed on `Low`:
    /// the smallest hurdle premium, admitting the add family on a scenario the
    /// correct tier leaves indeterminate.
    #[test]
    fn negative_equity_reads_as_maximal_leverage_in_the_tier_not_minimal() {
        // Strong on every other leg — large cap, profitable, calm — so only the
        // leverage leg decides. Equity has gone negative on buybacks.
        let mut fin = strong();
        fin.total_debt = Some(48.0e9);
        fin.total_equity = Some(-11.0e9);
        let m = compute_metrics(&fin);
        assert!(m.debt_to_equity.unwrap() < 0.0, "fixture sanity: signed ratio");

        let (tier, _) = assign_stock_tier(&fin, &m);
        assert_eq!(
            tier,
            RiskTier::High,
            "negative equity is levered beyond the equity base"
        );
        // And the same input reads the same way on the risk sub-score's leverage
        // leg — isolated, since `risk_score` averages it with the volatility leg.
        let leverage_only = ComputedMetrics {
            debt_to_equity: m.debt_to_equity,
            ..ComputedMetrics::default()
        };
        assert_eq!(risk_score(&leverage_only).unwrap(), 0.0);

        // The Low conjunction rejects it on its own, independent of leg order.
        let low_ok = m
            .debt_to_equity
            .map(|d| (0.0..TIER_LOW_MAX_DEBT_EQUITY).contains(&d))
            .unwrap_or(false);
        assert!(!low_ok, "a negative ratio must never satisfy the Low leg");
    }

    /// A negative debt/equity (negative equity) takes the leverage band's floor —
    /// the inverted clamp must never read "levered beyond the equity base" as
    /// maximally safe (the grade-v2 guard, the negative-P/E rule's mirror).
    #[test]
    fn negative_debt_equity_scores_zero_never_safe() {
        let m = |de: Option<f64>| ComputedMetrics {
            debt_to_equity: de,
            ..ComputedMetrics::default()
        };
        assert_eq!(risk_score(&m(Some(-2.9))).unwrap(), 0.0);
        assert_eq!(risk_score(&m(Some(0.0))).unwrap(), 100.0, "unlevered stays safest");
    }

    #[test]
    fn stock_tier_legs_trigger_and_default_per_the_missing_input_rule() {
        let fin = strong();
        let m = compute_metrics(&fin);
        // The strong large-cap: profitable, low leverage, low vol → Low.
        let (tier, gaps) = assign_stock_tier(&fin, &m);
        assert_eq!(tier, RiskTier::Low, "gaps: {gaps:?}");

        // A small cap trips a High leg regardless of the rest.
        let mut small = strong();
        small.market_cap = Some(1.0e9);
        let (tier, _) = assign_stock_tier(&small, &compute_metrics(&small));
        assert_eq!(tier, RiskTier::High);

        // Unprofitable trips High.
        let mut lossy = strong();
        lossy.net_income = Some(-5.0);
        lossy.operating_income = Some(-3.0);
        let (tier, _) = assign_stock_tier(&lossy, &compute_metrics(&lossy));
        assert_eq!(tier, RiskTier::High);

        // Wholesale-missing inputs read Medium with a logged gap — never a
        // fabricated High or Low.
        let empty = CompanyFinancials { symbol: "X".into(), ..Default::default() };
        let (tier, gaps) = assign_stock_tier(&empty, &compute_metrics(&empty));
        assert_eq!(tier, RiskTier::Medium);
        assert!(!gaps.is_empty());
    }

    #[test]
    fn fund_tier_maps_flag_vol_and_drawdown() {
        assert_eq!(assign_fund_tier(true, true, Some(0.10), Some(0.05)), RiskTier::High);
        assert_eq!(assign_fund_tier(false, false, Some(0.45), None), RiskTier::High);
        assert_eq!(assign_fund_tier(false, false, Some(0.30), Some(0.60)), RiskTier::High);
        assert_eq!(assign_fund_tier(false, false, Some(0.12), Some(0.15)), RiskTier::Low);
        // An option-overlay structural flag bars Low without forcing High.
        assert_eq!(assign_fund_tier(false, true, Some(0.12), Some(0.15)), RiskTier::Medium);
        assert_eq!(assign_fund_tier(false, false, Some(0.30), Some(0.20)), RiskTier::Medium);
        assert_eq!(assign_fund_tier(false, false, None, None), RiskTier::Medium);
    }

    #[test]
    fn hurdle_read_is_three_state_with_exit_side_hysteresis() {
        let scenario = |bear: f64, base: f64, bull: f64| ScenarioSet {
            bear: 0.0, base: 0.0, bull: 0.0,
            tr_bear: bear, tr_base: base, tr_bull: bull,
            rate_anchored: true, anchor_observations: 12, raw_observations: 12,
            degenerate_scenarios: 0, monotonicity_repaired: false,
            current_multiple_carry: false, dispersion_floor_applied: false,
            spread_percentiles: None, raw_percentiles: None,
        };
        // Hurdle = dgs2 0.04 + medium 0.05 = 0.09.
        let clears = hurdle_read(&scenario(0.10, 0.15, 0.20), 0.04, RiskTier::Medium);
        assert_eq!(clears.state, HurdleState::Clears);
        assert!(clears.admits_new_money);
        // Even the bull case misses → dead money.
        let fails = hurdle_read(&scenario(-0.05, 0.00, 0.05), 0.04, RiskTier::Medium);
        assert_eq!(fails.state, HurdleState::Fails);
        assert!(!fails.admits_new_money);
        // A base below the hurdle inside its own dispersion proves nothing — but the
        // point test still refuses new money.
        let indet = hurdle_read(&scenario(0.02, 0.07, 0.20), 0.04, RiskTier::Medium);
        assert_eq!(indet.state, HurdleState::Indeterminate);
        assert!(!indet.admits_new_money);
        // The admission point test can pass inside an indeterminate hurdle.
        let admit = hurdle_read(&scenario(0.02, 0.12, 0.20), 0.04, RiskTier::Medium);
        assert_eq!(admit.state, HurdleState::Indeterminate);
        assert!(admit.admits_new_money);
    }

    // ---- The engine stand-in arm --------------------------------------------------

    #[test]
    fn engine_outlook_reads_windowed_trailing_returns_per_threshold() {
        let closes = |last: f64| -> Vec<DatedValue> {
            let mut vals: Vec<DatedValue> = (0..300)
                .map(|i| DatedValue { date: format!("d{i}"), value: 100.0 })
                .collect();
            vals.last_mut().unwrap().value = last;
            vals
        };
        // +10% clears every window's flat threshold.
        let up = engine_outlook(&closes(110.0));
        assert_eq!(
            (up.short, up.mid, up.long),
            (HorizonRead::Bullish, HorizonRead::Bullish, HorizonRead::Bullish)
        );
        // −10% likewise bearish everywhere.
        let down = engine_outlook(&closes(90.0));
        assert_eq!(
            (down.short, down.mid, down.long),
            (HorizonRead::Bearish, HorizonRead::Bearish, HorizonRead::Bearish)
        );
        // +3% clears only the short window's 2% threshold — mid (5%) and long (8%)
        // read neutral.
        let mild = engine_outlook(&closes(103.0));
        assert_eq!(
            (mild.short, mild.mid, mild.long),
            (HorizonRead::Bullish, HorizonRead::Neutral, HorizonRead::Neutral)
        );
        // A series too short for any window reads neutral — the rule's null, never
        // a fabricated direction.
        let thin = engine_outlook(&closes(120.0)[..10]);
        assert_eq!(
            (thin.short, thin.mid, thin.long),
            (HorizonRead::Neutral, HorizonRead::Neutral, HorizonRead::Neutral)
        );
    }

    #[test]
    fn engine_conviction_maps_the_degradation_count() {
        let out = match analyze(&strong(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        // Force a clean read: no degradation → High.
        let mut clean = (*out).clone();
        clean.low_confidence_grade = false;
        clean.target_meta = TargetMeta { rate_anchored: true, ..Default::default() };
        clean.tier_gaps.clear();
        clean.hurdle.state = crate::portfolio::HurdleState::Clears;
        assert_eq!(engine_conviction(&clean, &[]), Conviction::High);
        // One flag → Medium.
        let mut one = clean.clone();
        one.low_confidence_grade = true;
        assert_eq!(engine_conviction(&one, &[]), Conviction::Medium);
        // Three flags → Low.
        let mut three = clean.clone();
        three.low_confidence_grade = true;
        three.target_meta.current_multiple_carry = true;
        assert_eq!(
            engine_conviction(&three, &["no dividends history".to_string()]),
            Conviction::Low
        );
    }

    #[test]
    fn engine_view_conviction_counts_the_assembled_dossier_gaps() {
        // The gap leg reads the caller's ASSEMBLED degraded-input list — fund
        // metadata gaps, the DGS10-history gap, and the listing-guard
        // unverified note ride beside the financials manifest
        // (`docs/portfolio-analysis.md` §Starting parameters: "any dossier
        // gap") — so a holding whose only degradation is a non-financials gap
        // still counts it, and the financials manifest is no longer consulted
        // directly.
        let out = match analyze(&strong(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let mut clean = (*out).clone();
        clean.low_confidence_grade = false;
        clean.target_meta = TargetMeta { rate_anchored: true, ..Default::default() };
        clean.tier_gaps.clear();
        clean.hurdle.state = crate::portfolio::HurdleState::Clears;
        let fin = strong();
        let view = engine_view(&clean, &fin, &[], None, false, false);
        assert_eq!(view.conviction, Conviction::High, "no assembled gap → High");
        let degraded =
            ["listing-resolution guard unverified — FMP profile unavailable".to_string()];
        let view = engine_view(&clean, &fin, &degraded, None, false, false);
        assert_eq!(
            view.conviction,
            Conviction::Medium,
            "a non-financials dossier gap counts against the stand-in read"
        );
    }

    #[test]
    fn engine_action_applies_the_rung_rule_and_walks_toward_hold() {
        use crate::portfolio::HurdleState;
        let hurdle = |state: HurdleState, admits: bool| HurdleRead {
            state,
            admits_new_money: admits,
            ..Default::default()
        };
        // A/B + clears + admits → add.
        assert_eq!(
            engine_action(Grade::B, &hurdle(HurdleState::Clears, true), None, false),
            Action::Add
        );
        // F + dead money → sell all; dead money alone → trim.
        assert_eq!(
            engine_action(Grade::F, &hurdle(HurdleState::Fails, false), None, false),
            Action::SellAll
        );
        assert_eq!(
            engine_action(Grade::C, &hurdle(HurdleState::Fails, false), None, false),
            Action::Trim
        );
        // The default read is hold.
        assert_eq!(
            engine_action(Grade::C, &hurdle(HurdleState::Indeterminate, false), None, false),
            Action::Hold
        );
        // Severe deterioration (exit family only): hold is off the set → trim.
        let severe = crate::portfolio::pre_profit::OverlayConsequences {
            conviction_ceiling: None,
            bar_add_family: true,
            exit_family_only: true,
            matched_rules: vec!["severe".into()],
        };
        assert_eq!(
            engine_action(Grade::B, &hurdle(HurdleState::Clears, true), Some(&severe), false),
            Action::Trim
        );
    }

    #[test]
    fn reanchor_reproduces_the_live_computation_at_the_same_inputs() {
        // The quick paths' closed-form re-anchor over the stored basis must be the
        // same arithmetic as the live v2 computation — at an unchanged spot and
        // DGS10 the scenario prices and total returns are identical.
        let fin = strong();
        let rates = rates();
        let m = compute_metrics(&fin);
        let bundle = match scenario_targets_v2(195.0, &fin, &rates, &m) {
            TargetOutcome::Computed(b) => b,
            TargetOutcome::NoAdmissibleDriver => panic!("fixture must compute"),
        };
        assert!(bundle.scenario.rate_anchored, "fixture is rate-anchored");
        assert!(bundle.basis.spread_percentiles.is_some());
        assert!((bundle.basis.spot - 195.0).abs() < 1e-12);
        assert_eq!(bundle.basis.consensus_eps_mid, Some(6.5));

        let re = reanchor_scenarios(&bundle.basis, 195.0, rates.dgs10);
        for (a, b) in [
            (re.bear, bundle.scenario.bear),
            (re.base, bundle.scenario.base),
            (re.bull, bundle.scenario.bull),
            (re.tr_bear, bundle.scenario.tr_bear),
            (re.tr_base, bundle.scenario.tr_base),
            (re.tr_bull, bundle.scenario.tr_bull),
        ] {
            assert!((a - b).abs() < 1e-9, "re-anchor drifted: {a} vs {b}");
        }
    }

    #[test]
    fn reanchor_moves_multiples_with_the_fresh_dgs10_and_trs_with_the_fresh_spot() {
        let fin = strong();
        let rates = rates();
        let m = compute_metrics(&fin);
        let bundle = match scenario_targets_v2(195.0, &fin, &rates, &m) {
            TargetOutcome::Computed(b) => b,
            TargetOutcome::NoAdmissibleDriver => panic!("fixture must compute"),
        };
        // A higher DGS10 widens every reciprocal denominator → cheaper multiples →
        // lower scenario prices (the closed form `1/(spread + DGS10)`).
        let tighter = reanchor_scenarios(&bundle.basis, 195.0, rates.dgs10 + 0.01);
        assert!(tighter.base < bundle.scenario.base);
        // A lower fresh price raises the total returns against the same targets.
        let cheaper = reanchor_scenarios(&bundle.basis, 150.0, rates.dgs10);
        assert!(cheaper.tr_base > bundle.scenario.tr_base);
        // The carry path re-uses the *stored* multiple: with no percentile surface
        // at all, the target must stay put while the fresh spot moves the TR — a
        // fresh-spot carry would pin the TR to the dividend leg alone.
        let carry_basis = QuickCheckBasis {
            spread_percentiles: None,
            raw_percentiles: None,
            ..bundle.basis.clone()
        };
        let carried = reanchor_scenarios(&carry_basis, 150.0, rates.dgs10);
        assert!(carried.current_multiple_carry);
        // Stored carry multiple = 195 / base driver; base price = driver × that = 195.
        assert!((carried.base - 195.0).abs() < 1e-6);
        assert!(carried.tr_base > 0.25, "TR measured from the fresh 150 spot");
    }

    #[test]
    fn gated_evaluation_skips_disallowed_series_whole() {
        use crate::portfolio::{
            ConditionRole, LedgerBranch, LedgerComparator, LedgerCondition, QuantCore,
            ThesisLedger,
        };
        let cond = |id: &str, series: LedgerSeries| LedgerCondition {
            condition_id: id.into(),
            role: ConditionRole::Falsifier,
            trigger_family: None,
            statement: format!("{id} statement"),
            quant: Some(QuantCore {
                series,
                comparator: LedgerComparator::Below,
                threshold: 1_000.0, // always breached — every value sits below
                margin: 0.0,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: None,
        };
        let ledger = ThesisLedger {
            branch: LedgerBranch::Priced,
            original_thesis: String::new(),
            current_thesis: String::new(),
            key_drivers: vec![],
            monitor: vec![],
            what_must_improve: String::new(),
            what_must_not_break: String::new(),
            conditions: vec![
                cond("c-price", LedgerSeries::Price),
                cond("c-margin", LedgerSeries::NetMargin),
            ],
            authored_band_relation: None,
        };
        let fin = strong();
        let m = compute_metrics(&fin);
        // Market-data only: the filing condition is skipped whole — no unevaluable
        // note, no state update — its carried state simply stands.
        let gated = evaluate_ledger_conditions_gated(&ledger, &m, &fin, "2026-08-03", |s| {
            s.cadence() == crate::portfolio::ConditionCadence::MarketData
        });
        assert!(gated.updated_states.iter().any(|(id, _)| id == "c-price"));
        assert!(!gated.updated_states.iter().any(|(id, _)| id == "c-margin"));
        assert!(gated.unevaluable.is_empty());
        // The ungated form still evaluates both.
        let full = evaluate_ledger_conditions(&ledger, &m, &fin, "2026-08-03");
        assert!(full.updated_states.iter().any(|(id, _)| id == "c-margin"));
    }

    #[test]
    fn feasible_set_bounds_the_add_family() {
        let read = |state, admits| HurdleRead {
            state,
            hurdle_rate: Some(0.09),
            tr_bear: None, tr_base: None, tr_bull: None,
            admits_new_money: admits,
        };
        // A clean A-grade offers the full ladder.
        let full = feasible_actions(Grade::A, &read(HurdleState::Clears, true), None, false);
        assert!(full.contains(&Action::Add) && full.contains(&Action::AddAggressively));
        // Dead money drops the add family at any grade; hold stays (hysteresis).
        let dead = feasible_actions(Grade::A, &read(HurdleState::Fails, false), None, false);
        assert!(!dead.contains(&Action::Add));
        assert!(dead.contains(&Action::Hold));
        // Grade F bars the family; a C-grade passing admission gets add but never
        // add-aggressively (A/B only).
        assert!(!feasible_actions(Grade::F, &read(HurdleState::Clears, true), None, false)
            .contains(&Action::Add));
        let c = feasible_actions(Grade::C, &read(HurdleState::Indeterminate, true), None, false);
        assert!(c.contains(&Action::Add) && !c.contains(&Action::AddAggressively));
    }

    #[test]
    fn overlay_rules_bar_the_add_family_and_severe_restricts_to_exits() {
        use crate::portfolio::pre_profit::OverlayConsequences;
        let read = |state, admits| HurdleRead {
            state,
            hurdle_rate: Some(0.09),
            tr_bear: None, tr_base: None, tr_bull: None,
            admits_new_money: admits,
        };
        // A constrained-runway bar strips the add family from an otherwise-clean
        // A-grade; hold survives (the bar is add-side only).
        let barred = OverlayConsequences {
            bar_add_family: true,
            ..Default::default()
        };
        let set = feasible_actions(Grade::A, &read(HurdleState::Clears, true), Some(&barred), false);
        assert!(!set.contains(&Action::Add));
        assert!(set.contains(&Action::Hold));
        // Severe deterioration restricts the whole set to the exit family.
        let severe = OverlayConsequences {
            bar_add_family: true,
            exit_family_only: true,
            ..Default::default()
        };
        let set = feasible_actions(Grade::A, &read(HurdleState::Clears, true), Some(&severe), false);
        assert_eq!(set, vec![Action::SellAll, Action::Trim]);
    }

    #[test]
    fn hard_forensic_bars_the_add_family_and_hard_caps_the_stand_in_conviction() {
        let read = |state, admits| HurdleRead {
            state,
            hurdle_rate: Some(0.09),
            tr_bear: None, tr_base: None, tr_bull: None,
            admits_new_money: admits,
        };
        // A tripped hard trigger strips the add family from an otherwise-clean
        // A-grade at any hurdle state; hold survives (the strongest disposition
        // an owned name admits keeps hold available —
        // `docs/portfolio-analysis.md` §Starting parameters).
        let set = feasible_actions(Grade::A, &read(HurdleState::Clears, true), None, true);
        assert_eq!(set, vec![Action::SellAll, Action::Trim, Action::Hold]);
        // The stand-in action obeys the bar: the A-grade add walks to hold.
        assert_eq!(
            engine_action(Grade::A, &read(HurdleState::Clears, true), None, true),
            Action::Hold
        );
        // The engine arm's conviction is hard-capped at Low — strictly dominating
        // the soft Medium ceiling — while a clean view stays unclamped.
        let out = match analyze(&strong(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let mut clean = (*out).clone();
        clean.low_confidence_grade = false;
        clean.target_meta = TargetMeta { rate_anchored: true, ..Default::default() };
        clean.tier_gaps.clear();
        clean.hurdle.state = crate::portfolio::HurdleState::Clears;
        let fin = strong();
        let view = engine_view(&clean, &fin, &[], None, true, false);
        assert_eq!(view.conviction, Conviction::Low);
        let view = engine_view(&clean, &fin, &[], None, false, false);
        assert_eq!(view.conviction, Conviction::High);
    }

    #[test]
    fn tech_event_pre_flag_scales_by_sqrt_time_and_types_its_gaps() {
        let series = |rows: &[(&str, f64)]| -> Vec<DatedValue> {
            rows.iter()
                .map(|(d, v)| DatedValue { date: d.to_string(), value: *v })
                .collect()
        };
        // Holding: 100 at the prior read, four sessions later at 90 (−10%);
        // benchmark flat → sector-relative −10%.
        let holding = series(&[
            ("2026-08-01", 100.0),
            ("2026-08-04", 97.0),
            ("2026-08-05", 95.0),
            ("2026-08-06", 93.0),
            ("2026-08-07", 90.0),
        ]);
        let bench = series(&[("2026-08-01", 500.0), ("2026-08-07", 500.0)]);
        // vol 0.02 → threshold 2 × 0.02 × √4 = 8% < 10% → fires.
        let f = tech_event_pre_flag(&holding, &bench, "XLK", "2026-08-01", Some(0.02)).unwrap();
        assert!(f.fired, "{f:?}");
        assert_eq!(f.sessions, 4);
        assert_eq!(f.benchmark, "XLK");
        assert!((f.relative_move + 0.10).abs() < 1e-12, "{f:?}");
        assert!((f.threshold - 0.08).abs() < 1e-12, "{f:?}");
        // vol 0.03 → threshold 12% > 10% → present but not fired.
        let f = tech_event_pre_flag(&holding, &bench, "XLK", "2026-08-01", Some(0.03)).unwrap();
        assert!(!f.fired, "{f:?}");
        // A benchmark move absorbs the holding's: both −10% → relative ~0.
        let bench_down = series(&[("2026-08-01", 500.0), ("2026-08-07", 450.0)]);
        let f =
            tech_event_pre_flag(&holding, &bench_down, "XLK", "2026-08-01", Some(0.02)).unwrap();
        assert!(!f.fired, "{f:?}");
        // Typed gaps, never a flag: no vol; no benchmark cover; no elapsed
        // sessions (prior read on the latest close).
        assert!(tech_event_pre_flag(&holding, &bench, "XLK", "2026-08-01", None).is_err());
        assert!(tech_event_pre_flag(&holding, &[], "XLK", "2026-08-01", Some(0.02)).is_err());
        assert!(tech_event_pre_flag(&holding, &bench, "XLK", "2026-08-07", Some(0.02)).is_err());
    }

    #[test]
    fn dated_join_and_drawdown_helpers_behave() {
        let series = vec![
            DatedValue { date: "2026-01-01".into(), value: 1.0 },
            DatedValue { date: "2026-02-01".into(), value: 2.0 },
            DatedValue { date: "2026-03-01".into(), value: 3.0 },
        ];
        assert_eq!(latest_on_or_before(&series, "2026-02-15"), Some(2.0));
        assert_eq!(latest_on_or_before(&series, "2026-03-01"), Some(3.0));
        assert_eq!(latest_on_or_before(&series, "2025-12-31"), None);
        // Percentiles interpolate linearly.
        assert!((percentile(&[1.0, 2.0, 3.0, 4.0, 5.0], 0.5) - 3.0).abs() < 1e-12);
        assert!((percentile(&[1.0, 2.0], 0.25) - 1.25).abs() < 1e-12);
        // Max drawdown: peak 100 → trough 60 = 40%.
        let closes = vec![80.0, 100.0, 70.0, 60.0, 90.0];
        assert!((max_drawdown(&[], &closes).unwrap() - 0.40).abs() < 1e-12);
    }

    #[test]
    fn options_signal_reads_put_skew_from_the_chain() {
        let chain = OptionChain {
            underlying: "AAPL".into(),
            underlying_price: Some(195.0),
            contracts: vec![
                OptionQuote {
                    kind: OptionKind::Call,
                    strike: 195.0,
                    expiry: "2026-07-17".into(),
                    volume: 1000.0,
                    open_interest: 5000.0,
                    implied_volatility: Some(0.25),
                    delta: None,
                },
                OptionQuote {
                    kind: OptionKind::Put,
                    strike: 195.0,
                    expiry: "2026-07-17".into(),
                    volume: 2000.0,
                    open_interest: 9000.0,
                    implied_volatility: Some(0.33),
                    delta: None,
                },
            ],
        };
        let sig = options_signal(&chain);
        assert!((sig.put_call_volume.unwrap() - 2.0).abs() < 1e-9);
        assert!((sig.put_call_open_interest.unwrap() - 1.8).abs() < 1e-9);
        // Puts richer than calls → positive skew (a hedging-demand tell).
        assert!(sig.iv_skew.unwrap() > 0.0);
    }

    /// Certification replay over a persisted live run — the grade-band shadow-tune
    /// slice's opening step (`docs/verification/2026-07-31-first-live-portfolio-run.md`
    /// §F4: the sub-score formulas were judged by their outputs, never audited against
    /// spec): recompute each priced holding's sub-scores and letter from its audit's
    /// persisted metrics and diff them against the persisted verdict. Stocks get the
    /// full derivation check; a priced fund's valuation/risk derive from the composite
    /// path whose history inputs the audit doesn't carry, so funds get the
    /// sub-scores→letter roll-up check only. Ignored like the live smokes: it reads a
    /// run exported from a real store (`MARKET_SIGNAL_RUN_JSON` = path to a
    /// `portfolio_runs.run_json` export), which holds a real book and never enters the
    /// repo.
    #[test]
    #[ignore]
    fn certify_run_grade_path() {
        let path = std::env::var("MARKET_SIGNAL_RUN_JSON")
            .expect("set MARKET_SIGNAL_RUN_JSON to an exported portfolio_runs.run_json");
        let body = std::fs::read_to_string(&path).expect("reading the run export");
        let run: crate::portfolio::PortfolioRun =
            serde_json::from_str(&body).expect("decoding PortfolioRun");

        let audits: std::collections::HashMap<&str, &crate::portfolio::HoldingAudit> =
            run.audit.iter().map(|a| (a.symbol.as_str(), a)).collect();

        let (mut priced, mut stocks) = (0usize, 0usize);
        let (mut derivation_mismatches, mut rollup_mismatches) = (0usize, 0usize);

        println!(
            "symbol   | persisted Q/V/M/R → letter    | recomputed Q/V/M/R → letter   | missing inputs"
        );
        for v in &run.verdicts {
            let crate::portfolio::VerdictDisposition::Priced(g) = &v.disposition else {
                continue;
            };
            priced += 1;
            let audit = audits
                .get(v.symbol.as_str())
                .unwrap_or_else(|| panic!("no audit row for {}", v.symbol));
            let m = &audit.metrics;

            // Roll-up leg (every priced holding): the persisted sub-scores must
            // reproduce the persisted letter through the fixed weights and cutoffs.
            let rolled = grade_from_subscores(&g.sub_scores);
            if rolled != g.grade {
                rollup_mismatches += 1;
                println!(
                    "{:8} | ROLL-UP MISMATCH: persisted {} ≠ rolled {}",
                    v.symbol,
                    g.grade.as_str(),
                    rolled.as_str()
                );
            }
            if g.fund_class_label.is_some() {
                continue;
            }
            // The derivation leg replays the CURRENT band constants, so it only
            // certifies a run stamped with the current grade parameter version —
            // an older-vintage run (or a pre-stamp `None`) keeps the roll-up check
            // alone. (Run `3b21ae85` was derivation-certified exact against its own
            // v1 bands on 2026-08-03, before the grade-v2 retune — the evidence
            // record in `docs/verification/`.)
            if audit.grade_parameter_version.as_deref() != Some(GRADE_PARAMETER_VERSION) {
                continue;
            }

            // Derivation leg (stocks): metrics → sub-score maps → imputation → letter.
            stocks += 1;
            let q = quality_score(m);
            let val = valuation_score(m);
            let mom = momentum_score(m);
            let r = risk_score(m);
            let recomputed = SubScores {
                quality: q.unwrap_or(50.0),
                valuation: val.unwrap_or(50.0),
                momentum: mom.unwrap_or(50.0),
                risk: r.unwrap_or(50.0),
            };
            let letter = grade_from_subscores(&recomputed);
            let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
            let ok = close(recomputed.quality, g.sub_scores.quality)
                && close(recomputed.valuation, g.sub_scores.valuation)
                && close(recomputed.momentum, g.sub_scores.momentum)
                && close(recomputed.risk, g.sub_scores.risk)
                && letter == g.grade
                && (q.is_none() || val.is_none() || r.is_none()) == g.low_confidence_grade;
            if !ok {
                derivation_mismatches += 1;
            }
            let gap = |name: &str, x: Option<f64>| {
                if x.is_none() {
                    format!("{name} ")
                } else {
                    String::new()
                }
            };
            println!(
                "{:8} | {:5.1}/{:5.1}/{:5.1}/{:5.1} → {} | {:5.1}/{:5.1}/{:5.1}/{:5.1} → {} {}| {}{}{}{}{}{}{}{}",
                v.symbol,
                g.sub_scores.quality,
                g.sub_scores.valuation,
                g.sub_scores.momentum,
                g.sub_scores.risk,
                g.grade.as_str(),
                recomputed.quality,
                recomputed.valuation,
                recomputed.momentum,
                recomputed.risk,
                letter.as_str(),
                if ok { "" } else { "← MISMATCH " },
                gap("net_margin", m.net_margin),
                gap("gross_margin", m.gross_margin),
                gap("pe", m.pe_ratio),
                gap("ps", m.ps_ratio),
                gap("pb", m.pb_ratio),
                gap("vol", m.return_volatility),
                gap("d/e", m.debt_to_equity),
                gap("rev_growth", m.revenue_growth),
            );
        }
        println!(
            "{priced} priced ({stocks} stocks): {derivation_mismatches} derivation mismatches, \
             {rollup_mismatches} roll-up mismatches"
        );
        assert_eq!(
            derivation_mismatches, 0,
            "stock sub-score derivation must reproduce the persisted audit"
        );
        assert_eq!(
            rollup_mismatches, 0,
            "sub-scores → letter roll-up must reproduce the persisted grade"
        );
    }

    /// One candidate normalization-band set for the shadow-tune sweep below —
    /// the same clamped-map shapes as the shipped formulas, parameterized.
    struct BandSet {
        name: &'static str,
        nm: (f64, f64),
        gm: (f64, f64),
        pe: (f64, f64),
        ps: (f64, f64),
        pb: (f64, f64),
        vol: (f64, f64),
        de: (f64, f64),
        /// Score a negative debt/equity (negative equity — levered beyond the
        /// equity base) as 0 instead of letting the inverted clamp read it as
        /// maximally safe (the mirror of the negative-P/E rule).
        negative_de_scores_zero: bool,
    }

    impl BandSet {
        fn scores(&self, m: &ComputedMetrics) -> (Option<f64>, Option<f64>, Option<f64>) {
            let quality = average(&[
                m.net_margin.map(|x| scale(x, self.nm.0, self.nm.1)),
                m.gross_margin.map(|x| scale(x, self.gm.0, self.gm.1)),
            ]);
            let pe = m.pe_ratio.map(|x| {
                if x <= 0.0 {
                    VALUATION_NEGATIVE_PE_SCORE
                } else {
                    scale(x, self.pe.0, self.pe.1)
                }
            });
            let valuation = average(&[
                pe,
                m.ps_ratio.map(|x| scale(x, self.ps.0, self.ps.1)),
                m.pb_ratio.map(|x| scale(x, self.pb.0, self.pb.1)),
            ]);
            let de = m.debt_to_equity.map(|d| {
                if d < 0.0 && self.negative_de_scores_zero {
                    0.0
                } else {
                    scale(d, self.de.0, self.de.1)
                }
            });
            let risk = average(&[
                m.return_volatility.map(|v| scale(v, self.vol.0, self.vol.1)),
                de,
            ]);
            (quality, valuation, risk)
        }

        fn letter(&self, m: &ComputedMetrics) -> (f64, Grade) {
            let (q, v, r) = self.scores(m);
            let s = SubScores {
                quality: q.unwrap_or(50.0),
                valuation: v.unwrap_or(50.0),
                momentum: 50.0,
                risk: r.unwrap_or(50.0),
            };
            let composite = (s.quality * W_QUALITY + s.valuation * W_VALUATION + s.risk * W_RISK)
                / (W_QUALITY + W_VALUATION + W_RISK);
            (composite, grade_from_subscores(&s))
        }
    }

    /// Spearman rank correlation between two equal-length score vectors — the
    /// ordering-preservation check (F4: relative ordering carries real signal).
    /// Ties take the average rank and the coefficient is Pearson over the ranks,
    /// so clamped scores that tie (composites pinned at a band edge) don't skew
    /// the statistic the no-ties shortcut would.
    fn spearman(a: &[f64], b: &[f64]) -> f64 {
        fn ranks(xs: &[f64]) -> Vec<f64> {
            let mut idx: Vec<usize> = (0..xs.len()).collect();
            idx.sort_by(|&i, &j| xs[i].partial_cmp(&xs[j]).unwrap());
            let mut r = vec![0.0; xs.len()];
            let mut pos = 0;
            while pos < idx.len() {
                let mut end = pos;
                while end + 1 < idx.len() && xs[idx[end + 1]] == xs[idx[pos]] {
                    end += 1;
                }
                let avg = (pos + end) as f64 / 2.0;
                for &i in &idx[pos..=end] {
                    r[i] = avg;
                }
                pos = end + 1;
            }
            r
        }
        fn pearson(xs: &[f64], ys: &[f64]) -> f64 {
            let n = xs.len() as f64;
            let (mx, my) = (
                xs.iter().sum::<f64>() / n,
                ys.iter().sum::<f64>() / n,
            );
            let cov: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
            let (vx, vy): (f64, f64) = (
                xs.iter().map(|x| (x - mx).powi(2)).sum(),
                ys.iter().map(|y| (y - my).powi(2)).sum(),
            );
            cov / (vx.sqrt() * vy.sqrt())
        }
        pearson(&ranks(a), &ranks(b))
    }

    /// Pin the tie handling: identical vectors correlate 1.0 even with ties, and
    /// average-rank ties beat the arbitrary-distinct-rank assignment (which would
    /// read the same tied vector as imperfectly correlated with itself under a
    /// different tiebreak order).
    #[test]
    fn spearman_handles_ties_with_average_ranks() {
        let tied = [50.0, 50.0, 70.0, 30.0, 50.0];
        assert!((spearman(&tied, &tied) - 1.0).abs() < 1e-12);
        let reversed = [5.0, 4.0, 3.0, 2.0, 1.0];
        let ascending = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((spearman(&ascending, &reversed) + 1.0).abs() < 1e-12);
    }

    /// The grade-band shadow-tune sweep (the slice's closing step): candidate band
    /// sets over the persisted metric surface AND the probe-refreshed surface
    /// (`MARKET_SIGNAL_REFRESHED_METRICS` — optional), printing per-stock letters,
    /// the letter distribution, and rank correlation against the shipped bands.
    /// A decision aid, not a gate: the chosen constants land in the calibration
    /// surface above and re-certify through `certify_run_grade_path`.
    #[test]
    #[ignore]
    fn sweep_grade_bands() {
        let path = std::env::var("MARKET_SIGNAL_RUN_JSON").expect("MARKET_SIGNAL_RUN_JSON");
        let run: crate::portfolio::PortfolioRun =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let refreshed: std::collections::BTreeMap<String, ComputedMetrics> =
            match std::env::var("MARKET_SIGNAL_REFRESHED_METRICS") {
                Ok(p) => serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap(),
                Err(_) => Default::default(),
            };

        let baseline = BandSet {
            // Reads whatever constants are currently shipped — after a retune this
            // is the NEW basis, not the vintage the run was graded under. The
            // negative-D/E guard mirrors production `risk_score` (grade-v2+), so
            // the baseline is the real shipped behavior, not a hybrid.
            name: "current consts (shipped)",
            nm: QUALITY_NET_MARGIN_BAND,
            gm: QUALITY_GROSS_MARGIN_BAND,
            pe: VALUATION_PE_BAND,
            ps: VALUATION_PS_BAND,
            pb: VALUATION_PB_BAND,
            vol: RISK_VOLATILITY_BAND,
            de: RISK_DEBT_EQUITY_BAND,
            negative_de_scores_zero: true,
        };
        let moderate = BandSet {
            name: "moderate",
            nm: (0.0, 0.25),
            gm: (0.20, 0.60),
            pe: (55.0, 10.0),
            ps: (18.0, 1.5),
            pb: (20.0, 1.5),
            vol: (0.04, 0.0),
            de: (2.0, 0.0),
            negative_de_scores_zero: true,
        };
        let recentered = BandSet {
            name: "recentered-growth",
            nm: (0.0, 0.30),
            gm: (0.15, 0.65),
            pe: (70.0, 12.0),
            ps: (25.0, 2.0),
            pb: (30.0, 2.0),
            vol: (0.045, 0.005),
            de: (2.5, 0.0),
            negative_de_scores_zero: true,
        };

        let stocks: Vec<(&str, &ComputedMetrics)> = run
            .verdicts
            .iter()
            .filter_map(|v| match &v.disposition {
                crate::portfolio::VerdictDisposition::Priced(g) if g.fund_class_label.is_none() => {
                    run.audit
                        .iter()
                        .find(|a| a.symbol == v.symbol)
                        .map(|a| (v.symbol.as_str(), &a.metrics))
                }
                _ => None,
            })
            .collect();

        let surfaces: Vec<(&str, Vec<(&str, &ComputedMetrics)>)> = if refreshed.is_empty() {
            vec![("as-persisted", stocks.clone())]
        } else {
            vec![
                ("as-persisted", stocks.clone()),
                (
                    "refreshed",
                    stocks
                        .iter()
                        .map(|(s, m)| (*s, refreshed.get(*s).unwrap_or(m)))
                        .collect(),
                ),
            ]
        };

        for (surface_name, surface) in &surfaces {
            println!("\n=== surface: {surface_name} ===");
            let base_composites: Vec<f64> =
                surface.iter().map(|(_, m)| baseline.letter(m).0).collect();
            for bands in [&baseline, &moderate, &recentered] {
                let mut dist = std::collections::BTreeMap::new();
                let mut composites = vec![];
                println!("--- {} ---", bands.name);
                for (symbol, m) in surface {
                    let (composite, letter) = bands.letter(m);
                    composites.push(composite);
                    *dist.entry(letter.as_str()).or_insert(0usize) += 1;
                    println!("{symbol:8} {composite:5.1} {}", letter.as_str());
                }
                println!(
                    "distribution: {:?}  spearman-vs-shipped: {:.3}",
                    dist,
                    spearman(&base_composites, &composites)
                );
            }
        }
    }

    // ---- Thesis-ledger series resolution & condition evaluation ----------------

    use crate::portfolio::{
        ConditionCadence, ConditionEvalState, ConditionRole, CrossingOutcome, LedgerBranch,
        LedgerComparator, LedgerCondition, QuantCore, ThesisLedger,
    };

    fn ledger_of(conditions: Vec<LedgerCondition>) -> ThesisLedger {
        ThesisLedger {
            branch: LedgerBranch::Priced,
            original_thesis: "o".into(),
            current_thesis: "c".into(),
            key_drivers: vec![],
            monitor: vec![],
            what_must_improve: String::new(),
            what_must_not_break: String::new(),
            conditions,
            authored_band_relation: None,
        }
    }

    fn quant_cond(
        id: &str,
        series: LedgerSeries,
        comparator: LedgerComparator,
        threshold: f64,
        margin: f64,
        state: Option<ConditionEvalState>,
    ) -> LedgerCondition {
        LedgerCondition {
            condition_id: id.into(),
            role: ConditionRole::Falsifier,
            trigger_family: None,
            statement: format!(
                "{} {} {threshold}",
                series.as_kebab(),
                comparator.as_kebab()
            ),
            quant: Some(QuantCore {
                series,
                comparator,
                threshold,
                margin,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: state,
        }
    }

    #[test]
    fn series_resolution_maps_values_and_observation_identities() {
        let fin = strong();
        let metrics = compute_metrics(&fin);
        let m = resolve_series(LedgerSeries::NetMargin, &metrics, &fin).unwrap();
        assert_eq!(m.observation_id, "2026-06-30", "filing series keys to the newest period end");
        let p = resolve_series(LedgerSeries::Price, &metrics, &fin).unwrap();
        assert_eq!(p.value, 195.0);
        assert_eq!(p.observation_id, "2026-07-15", "market series keys to the newest close date");
        // A gap is a typed error, never a silent clear.
        assert!(resolve_series(LedgerSeries::ExpenseRatio, &metrics, &fin).is_err());
    }

    #[test]
    fn market_series_without_a_dated_print_read_unevaluable_never_calendar_keyed() {
        // A degraded run (no dated close history) has no print to key a
        // market-data observation, so the series is unevaluable — never keyed to
        // the calendar date, which would let successive runs advance a streak
        // against unchanged data (Codex round 2, finding 1).
        let mut fin = strong();
        fin.daily_closes.clear();
        let metrics = compute_metrics(&fin);
        assert!(resolve_series(LedgerSeries::Price, &metrics, &fin).is_err());
        let ledger = ledger_of(vec![quant_cond(
            "p",
            LedgerSeries::Price,
            LedgerComparator::Above,
            100.0,
            0.0,
            None,
        )]);
        let eval = evaluate_ledger_conditions(&ledger, &metrics, &fin, "2026-08-03");
        assert!(eval.crossings.is_empty());
        assert_eq!(eval.unevaluable.len(), 1);
        assert!(eval.updated_states.is_empty(), "state untouched — a typed non-detection");
    }

    #[test]
    fn a_statement_basis_flip_cannot_confirm_a_crossing() {
        use crate::portfolio::StatementBasis;
        // The holding's thesis is intact; only the MEASUREMENT changed. A
        // one-quarter feed gap fails the contiguity guard, the levels drop to the
        // annual basis, and a growing issuer's P/S steps up with nothing having
        // happened. The condition's streak was accumulated on the TTM basis.
        let mut fin = strong();
        fin.statement_basis = Some(StatementBasis::Annual);
        let mut metrics = compute_metrics(&fin);
        metrics.ps_ratio = Some(10.3); // was ~8.0 on the TTM window
        let standing = ConditionEvalState {
            last_observation_id: Some("2026-07-14".into()),
            last_value: Some(8.0),
            breach_streak: 1,
            first_breach_at: Some("2026-07-14".into()),
            acknowledged_observation_id: Some("2026-05-01".into()),
            authored_statement_basis: Some(StatementBasis::Ttm),
            ..Default::default()
        };
        let ledger = ledger_of(vec![quant_cond(
            "ps",
            LedgerSeries::PsRatio,
            LedgerComparator::Above,
            10.0,
            0.0,
            Some(standing),
        )]);
        let eval = evaluate_ledger_conditions(&ledger, &metrics, &fin, "2026-08-03");
        // P/S is market-cadence, so it confirms in two distinct observations: one
        // more breaching close after this and the falsifier would have tripped and
        // forced archival — off the basis step alone.
        assert!(
            eval.crossings.is_empty(),
            "a basis step must not cross: {:?}",
            eval.crossings
        );
        assert_eq!(eval.unevaluable.len(), 1);
        assert_eq!(eval.unevaluable_series, vec![LedgerSeries::PsRatio]);
        let (_, st) = &eval.updated_states[0];
        assert_eq!(st.breach_streak, 0, "the old basis's streak cannot carry across");
        assert_eq!(
            st.authored_statement_basis,
            Some(StatementBasis::Annual),
            "the new basis is adopted, so the gate fires once per flip — not forever"
        );
        assert_eq!(
            st.acknowledged_observation_id,
            Some("2026-05-01".into()),
            "NOT the clean arm — a clean read would clear the acknowledgment and \
             report a confirmation the evidence does not support"
        );

        // Re-evaluated on the now-adopted basis, the same breaching level counts
        // normally: the gate suppresses the step, not the series.
        let mut cond = ledger.conditions[0].clone();
        cond.eval_state = Some(st.clone());
        let after = evaluate_ledger_conditions(
            &ledger_of(vec![cond]),
            &metrics,
            &fin,
            "2026-08-04",
        );
        assert!(after.unevaluable.is_empty(), "the flip is not permanent");
    }

    #[test]
    fn a_price_derived_condition_is_untouched_by_a_basis_flip() {
        use crate::portfolio::StatementBasis;
        // Scope check: only statement-DERIVED series gate. A price condition's value
        // does not move with the statement window, so a flip must not cost it a pass.
        let mut fin = strong();
        fin.statement_basis = Some(StatementBasis::Annual);
        let metrics = compute_metrics(&fin);
        let standing = ConditionEvalState {
            authored_statement_basis: Some(StatementBasis::Ttm),
            ..Default::default()
        };
        let ledger = ledger_of(vec![quant_cond(
            "px",
            LedgerSeries::Price,
            LedgerComparator::Below,
            300.0,
            0.0,
            Some(standing),
        )]);
        let eval = evaluate_ledger_conditions(&ledger, &metrics, &fin, "2026-08-03");
        assert!(eval.unevaluable.is_empty(), "{:?}", eval.unevaluable);
        assert_eq!(eval.crossings.len(), 1, "the price condition still evaluates");
    }

    #[test]
    fn a_pre_stamp_state_adopts_the_basis_without_a_discontinuity() {
        use crate::portfolio::StatementBasis;
        // Upgrade path: states persisted before the marker existed carry `None`, and
        // there is nothing for the current basis to disagree with — so the first pass
        // adopts silently rather than spending every holding's first sweep on a
        // fabricated discontinuity.
        let mut fin = strong();
        fin.statement_basis = Some(StatementBasis::Annual);
        let metrics = compute_metrics(&fin);
        let ledger = ledger_of(vec![quant_cond(
            "nm",
            LedgerSeries::NetMargin,
            LedgerComparator::Below,
            0.9,
            0.0,
            Some(ConditionEvalState::default()),
        )]);
        let eval = evaluate_ledger_conditions(&ledger, &metrics, &fin, "2026-08-03");
        assert!(eval.unevaluable.is_empty(), "{:?}", eval.unevaluable);
        let (_, st) = &eval.updated_states[0];
        assert_eq!(st.authored_statement_basis, Some(StatementBasis::Annual));
    }

    #[test]
    fn a_negative_debt_equity_never_clears_a_standing_breach_streak() {
        // The defect's teeth. A negative debt/equity (liabilities past the equity
        // base — maximal leverage) cannot breach "debt/equity above 3", so it used
        // to land in the CLEAN arm and reset the whole streak: `breach_streak`,
        // `first_breach_at`, `confirmed_at` and the acknowledgment all wiped, on
        // the most levered reading the series can produce.
        let mut fin = strong();
        fin.total_debt = Some(500.0);
        fin.total_equity = Some(-100.0);
        let metrics = compute_metrics(&fin);
        assert!(
            metrics.debt_to_equity.is_some_and(|d| d < 0.0),
            "the fixture must actually produce a negative ratio"
        );
        let standing = ConditionEvalState {
            last_observation_id: Some("2026-03-31".into()),
            last_value: Some(4.0),
            breach_streak: 1,
            first_breach_at: Some("2026-04-01".into()),
            ..Default::default()
        };
        let ledger = ledger_of(vec![quant_cond(
            "de",
            LedgerSeries::DebtToEquity,
            LedgerComparator::Above,
            3.0,
            0.0,
            Some(standing),
        )]);
        let eval = evaluate_ledger_conditions(&ledger, &metrics, &fin, "2026-08-03");
        assert!(eval.crossings.is_empty(), "off-scale must not fabricate a crossing either");
        assert_eq!(eval.unevaluable.len(), 1, "it resolves unevaluable, not clean");
        assert_eq!(eval.unevaluable_series, vec![LedgerSeries::DebtToEquity]);
        assert!(
            eval.updated_states.is_empty(),
            "no state movement at all — the standing streak survives"
        );
    }

    #[test]
    fn a_negative_pe_does_not_read_as_cheap_on_a_below_threshold() {
        // The other direction: a loss-maker's negative P/E compared naively
        // satisfies "P/E below 15" and fires an add trigger on exactly the
        // evidence that should stop one.
        let mut fin = strong();
        fin.market_cap = Some(1.0e11);
        fin.net_income = Some(-5.0e9);
        fin.pe_ratio = None;
        let mut merged = fin.clone();
        merged.pe_ratio = Some(-20.0);
        let metrics = compute_metrics(&merged);
        assert!(metrics.pe_ratio.is_some_and(|p| p < 0.0));
        let ledger = ledger_of(vec![quant_cond(
            "pe",
            LedgerSeries::PeRatio,
            LedgerComparator::Below,
            15.0,
            0.0,
            None,
        )]);
        let eval = evaluate_ledger_conditions(&ledger, &metrics, &merged, "2026-08-03");
        assert!(eval.crossings.is_empty(), "a negative P/E is not a cheap P/E");
        assert_eq!(eval.unevaluable.len(), 1);
    }

    #[test]
    fn the_off_scale_guard_admits_the_legitimate_boundary_readings() {
        // Zero debt is a real debt/equity reading and must still evaluate — the
        // guard is off-scale detection, not a blanket non-positive reject. (A P/E
        // of zero is degenerate on its own scale and stays out, which is why the
        // two series carry different admissible ranges rather than one shared
        // floor.)
        let mut fin = strong();
        fin.total_debt = Some(0.0);
        fin.total_equity = Some(1000.0);
        let metrics = compute_metrics(&fin);
        assert_eq!(metrics.debt_to_equity, Some(0.0));
        let resolved = resolve_series(LedgerSeries::DebtToEquity, &metrics, &fin)
            .expect("zero leverage is on-scale");
        assert_eq!(resolved.value, 0.0);
    }

    #[test]
    fn cadence_and_counts_derive_from_the_series() {
        assert_eq!(LedgerSeries::NetMargin.cadence(), ConditionCadence::Filing);
        assert_eq!(
            LedgerSeries::NetMargin.required_consecutive(),
            LEDGER_CONSECUTIVE_FILING
        );
        assert_eq!(LedgerSeries::Price.cadence(), ConditionCadence::MarketData);
        assert_eq!(
            LedgerSeries::Price.required_consecutive(),
            LEDGER_CONSECUTIVE_MARKET_DATA
        );
        // Every kebab label round-trips through the claim parser.
        for s in LedgerSeries::ALL {
            assert_eq!(LedgerSeries::parse(s.as_kebab()), Some(s));
        }
        assert_eq!(LedgerSeries::parse("made-up-series"), None);
    }

    #[test]
    fn quarters_contiguous_accepts_fiscal_spacing_and_rejects_gaps() {
        // Ordinary calendar quarters and 13/14-week fiscal spacing pass; a
        // skipped quarter (~182 days) or an undatable print reads
        // non-contiguous, and short runs are trivially contiguous.
        assert!(quarters_contiguous(["2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30"]));
        assert!(quarters_contiguous(["2026-06-27", "2026-03-28", "2025-12-27"])); // 13-week
        assert!(!quarters_contiguous([
            "2026-06-30",
            "2025-12-31", // 2026-03-31 missing — a ~182-day jump
            "2025-09-30"
        ]));
        assert!(!quarters_contiguous(["2026-06-30", "not-a-date"]));
        assert!(quarters_contiguous(["2026-06-30"]));
        assert!(quarters_contiguous(std::iter::empty::<&str>()));
    }

    #[test]
    fn filing_cadence_confirms_on_the_first_qualifying_breach_beyond_the_margin() {
        let fin = strong();
        let metrics = compute_metrics(&fin); // net margin 100/400 = 0.25
        // Below 0.30 with margin 0.06: the 0.05 shortfall sits inside the noise
        // guard — no breach at all.
        let inside = ledger_of(vec![quant_cond(
            "a",
            LedgerSeries::NetMargin,
            LedgerComparator::Below,
            0.30,
            0.06,
            None,
        )]);
        let eval = evaluate_ledger_conditions(&inside, &metrics, &fin, "2026-08-03");
        assert!(eval.crossings.is_empty(), "{:?}", eval.crossings);

        // Beyond the margin, a filing print is the period's settled observation —
        // the first qualifying breach confirms immediately (count 1).
        let beyond = ledger_of(vec![quant_cond(
            "a",
            LedgerSeries::NetMargin,
            LedgerComparator::Below,
            0.30,
            0.01,
            None,
        )]);
        let eval = evaluate_ledger_conditions(&beyond, &metrics, &fin, "2026-08-03");
        assert_eq!(eval.crossings.len(), 1);
        assert_eq!(eval.crossings[0].outcome, CrossingOutcome::Confirmed);
        let (_, st) = &eval.updated_states[0];
        assert_eq!(st.breach_streak, 1);
        assert!(st.confirmed_at.is_some());
    }

    #[test]
    fn market_data_breach_needs_two_distinct_observations_and_never_advances_on_a_reprint() {
        let mut fin = strong();
        let metrics = compute_metrics(&fin);
        // Price 195 below 200 — a market-data condition (count 2).
        let ledger = ledger_of(vec![quant_cond(
            "p",
            LedgerSeries::Price,
            LedgerComparator::Below,
            200.0,
            0.0,
            None,
        )]);
        let eval1 = evaluate_ledger_conditions(&ledger, &metrics, &fin, "2026-08-03");
        assert_eq!(
            eval1.crossings[0].outcome,
            CrossingOutcome::FirstBreach,
            "a lone print is a quiet note"
        );
        let st1 = eval1.updated_states[0].1.clone();
        assert_eq!(st1.breach_streak, 1);

        // Re-evaluating the same print never advances the streak or confirms.
        let carried = ledger_of(vec![quant_cond(
            "p",
            LedgerSeries::Price,
            LedgerComparator::Below,
            200.0,
            0.0,
            Some(st1),
        )]);
        let eval2 = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-04");
        assert!(eval2.crossings.is_empty(), "{:?}", eval2.crossings);
        assert_eq!(eval2.updated_states[0].1.breach_streak, 1);

        // A second distinct breaching observation confirms.
        fin.daily_closes.push(DatedValue {
            date: "2026-07-16".into(),
            value: 195.0,
        });
        let eval3 = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-05");
        assert_eq!(eval3.crossings[0].outcome, CrossingOutcome::Confirmed);
        assert_eq!(eval3.updated_states[0].1.breach_streak, 2);
    }

    #[test]
    fn an_acknowledged_breach_re_raises_only_against_a_later_observation() {
        let mut fin = strong();
        let metrics = compute_metrics(&fin);
        let acknowledged = ConditionEvalState {
            last_observation_id: Some("2026-07-15".into()),
            last_value: Some(195.0),
            last_evaluated_at: Some("2026-08-01".into()),
            breach_streak: 2,
            first_breach_at: Some("2026-07-31".into()),
            confirmed_at: Some("2026-08-01".into()),
            acknowledged_observation_id: Some("2026-07-15".into()),
            authored_statement_basis: None,
        };
        let carried = ledger_of(vec![quant_cond(
            "p",
            LedgerSeries::Price,
            LedgerComparator::Below,
            200.0,
            0.0,
            Some(acknowledged),
        )]);
        // The same observation the full pass already examined: no re-raise.
        let eval = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-03");
        assert!(eval.crossings.is_empty(), "{:?}", eval.crossings);
        // A later distinct breaching observation re-raises the confirmed breach.
        fin.daily_closes.push(DatedValue {
            date: "2026-07-20".into(),
            value: 190.0,
        });
        let eval = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-04");
        assert_eq!(eval.crossings.len(), 1);
        assert_eq!(eval.crossings[0].outcome, CrossingOutcome::Confirmed);
    }

    #[test]
    fn a_clean_distinct_observation_resets_the_streak() {
        let mut fin = strong();
        let metrics = compute_metrics(&fin);
        let st = ConditionEvalState {
            last_observation_id: Some("2026-07-15".into()),
            breach_streak: 1,
            first_breach_at: Some("2026-08-01".into()),
            ..Default::default()
        };
        // Price 195 is NOT below 180 — a clean print on a new observation resets.
        let carried = ledger_of(vec![quant_cond(
            "p",
            LedgerSeries::Price,
            LedgerComparator::Below,
            180.0,
            0.0,
            Some(st),
        )]);
        fin.daily_closes.push(DatedValue {
            date: "2026-07-16".into(),
            value: 195.0,
        });
        let eval = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-04");
        assert!(eval.crossings.is_empty());
        let s = &eval.updated_states[0].1;
        assert_eq!(s.breach_streak, 0);
        assert!(s.first_breach_at.is_none());
    }

    #[test]
    fn an_out_of_order_older_print_neither_advances_nor_resets_nor_regresses() {
        // The sweep and the full run read EOD through different FMP endpoints
        // at different moments — a lagged read can serve an OLDER print
        // than the recorded observation. Date-keyed identity is monotonic: the
        // stale print is a non-event, whatever its value.
        let fin = strong(); // newest close 2026-07-15
        let metrics = compute_metrics(&fin);
        let st = ConditionEvalState {
            last_observation_id: Some("2026-07-20".into()),
            last_value: Some(193.0),
            breach_streak: 1,
            first_breach_at: Some("2026-08-01".into()),
            ..Default::default()
        };
        // Breaching value (195 < 200) on the stale print: no advance.
        let carried = ledger_of(vec![quant_cond(
            "p",
            LedgerSeries::Price,
            LedgerComparator::Below,
            200.0,
            0.0,
            Some(st.clone()),
        )]);
        let eval = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-04");
        let s = &eval.updated_states[0].1;
        assert_eq!(s.breach_streak, 1, "a stale print must not advance the streak");
        assert_eq!(s.last_observation_id.as_deref(), Some("2026-07-20"));
        assert_eq!(s.last_value, Some(193.0), "state must not regress to the stale value");
        assert!(eval.crossings.is_empty(), "{:?}", eval.crossings);

        // Clean value on a stale print: no reset either (the recorded streak
        // was keyed to a NEWER observation the stale print can't overrule).
        let clean_carried = ledger_of(vec![quant_cond(
            "p",
            LedgerSeries::Price,
            LedgerComparator::Below,
            180.0,
            0.0,
            Some(st),
        )]);
        let eval = evaluate_ledger_conditions(&clean_carried, &metrics, &fin, "2026-08-04");
        assert_eq!(eval.updated_states[0].1.breach_streak, 1, "no reset on a stale print");
    }

    #[test]
    fn a_stale_print_re_raise_keys_to_the_recorded_observation() {
        // A confirmed-unacked breach still re-raises on a stale-print pass, but
        // the crossing must carry the RECORDED (newer) observation id — 6g acks
        // whatever the crossing names, and acking the stale id would let the
        // next sweep's newer print read as past-ack and re-raise the breach it
        // just consumed.
        let fin = strong(); // newest close 2026-07-15
        let metrics = compute_metrics(&fin);
        let confirmed = ConditionEvalState {
            last_observation_id: Some("2026-07-20".into()),
            last_value: Some(190.0),
            breach_streak: 2,
            first_breach_at: Some("2026-07-31".into()),
            confirmed_at: Some("2026-08-01".into()),
            ..Default::default()
        };
        let carried = ledger_of(vec![quant_cond(
            "p",
            LedgerSeries::Price,
            LedgerComparator::Below,
            200.0,
            0.0,
            Some(confirmed),
        )]);
        let eval = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-04");
        assert_eq!(eval.crossings.len(), 1);
        assert_eq!(eval.crossings[0].observation_id, "2026-07-20");
        assert_eq!(eval.crossings[0].observed_value, 190.0);
    }

    #[test]
    fn a_same_observation_corrected_clean_value_resets_the_breach_state() {
        // Same identity, corrected VALUE (a fixed close, a same-period restated
        // print): the observation's settled value is clean, so any standing
        // breach state resets and no Confirmed crossing can emit carrying a
        // value that no longer breaches.
        let fin = strong(); // newest close 2026-07-15, value 195
        let metrics = compute_metrics(&fin);
        let confirmed = ConditionEvalState {
            last_observation_id: Some("2026-07-15".into()),
            last_value: Some(185.0), // the earlier, breached read of the same print
            breach_streak: 2,
            first_breach_at: Some("2026-07-31".into()),
            confirmed_at: Some("2026-08-01".into()),
            ..Default::default()
        };
        // Below 190: the corrected 195 reads clean on the SAME observation id.
        let carried = ledger_of(vec![quant_cond(
            "p",
            LedgerSeries::Price,
            LedgerComparator::Below,
            190.0,
            0.0,
            Some(confirmed),
        )]);
        let eval = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-04");
        assert!(
            eval.crossings.is_empty(),
            "no crossing may carry a clean value: {:?}",
            eval.crossings
        );
        let s = &eval.updated_states[0].1;
        assert_eq!(s.breach_streak, 0);
        assert!(s.confirmed_at.is_none());
        assert_eq!(s.last_value, Some(195.0));
    }

    #[test]
    fn a_clean_reset_clears_the_acknowledgment_so_a_re_breach_re_raises() {
        // Value-keyed identity has no order, so without the reset-clear a
        // genuine re-breach at the previously acknowledged value would read as
        // the already-examined observation indefinitely (left, came back —
        // suppressed until some third value printed).
        let fin = strong();
        let metrics_high = ComputedMetrics {
            expense_ratio: Some(0.005),
            ..Default::default()
        };
        let acked = ConditionEvalState {
            last_observation_id: Some("expense-ratio:0.005".into()),
            last_value: Some(0.005),
            breach_streak: 1,
            confirmed_at: Some("2026-08-01".into()),
            acknowledged_observation_id: Some("expense-ratio:0.005".into()),
            ..Default::default()
        };
        let cond = |st: Option<ConditionEvalState>| {
            ledger_of(vec![quant_cond(
                "e",
                LedgerSeries::ExpenseRatio,
                LedgerComparator::Above,
                0.004,
                0.0,
                st,
            )])
        };
        // The print moves clean (0.004, distinct, not above the threshold):
        // streak resets AND the acknowledgment clears with it.
        let metrics_clean = ComputedMetrics {
            expense_ratio: Some(0.004),
            ..Default::default()
        };
        let eval = evaluate_ledger_conditions(
            &cond(Some(acked)),
            &metrics_clean,
            &fin,
            "2026-08-05",
        );
        let reset = eval.updated_states[0].1.clone();
        assert_eq!(reset.breach_streak, 0);
        assert!(reset.acknowledged_observation_id.is_none(), "ack must clear on reset");
        // The ratio returns to 0.005 — a genuinely new observation of the old
        // value: filing-cadence count 1 confirms and the crossing re-raises.
        let eval = evaluate_ledger_conditions(
            &cond(Some(reset)),
            &metrics_high,
            &fin,
            "2026-08-06",
        );
        assert_eq!(eval.crossings.len(), 1, "{:?}", eval.crossings);
        assert_eq!(eval.crossings[0].outcome, CrossingOutcome::Confirmed);
    }

    #[test]
    fn expense_ratio_identity_keys_to_the_changed_print_not_the_run_date() {
        // The `etf/info` print carries no date, so its observation identity is the
        // value itself: repeated runs against one unchanged print are the SAME
        // observation — no streak advance, no re-raise of an acknowledged breach —
        // and only a changed print is a distinct observation.
        let fin = strong();
        let mut metrics = ComputedMetrics {
            expense_ratio: Some(0.009),
            ..Default::default()
        };
        // Breach: 0.009 > 0.0075 + 0.0005. Filing cadence (count 1) confirms.
        let ledger = ledger_of(vec![quant_cond(
            "e",
            LedgerSeries::ExpenseRatio,
            LedgerComparator::Above,
            0.0075,
            0.0005,
            None,
        )]);
        let eval1 = evaluate_ledger_conditions(&ledger, &metrics, &fin, "2026-08-03");
        assert_eq!(eval1.crossings[0].outcome, CrossingOutcome::Confirmed);
        let mut st = eval1.updated_states[0].1.clone();
        assert_eq!(st.last_observation_id.as_deref(), Some("expense-ratio:0.009"));

        // Acknowledged; the same unchanged print on a later run date never
        // re-raises (the run date plays no part in the identity).
        st.acknowledged_observation_id = st.last_observation_id.clone();
        let carried = ledger_of(vec![quant_cond(
            "e",
            LedgerSeries::ExpenseRatio,
            LedgerComparator::Above,
            0.0075,
            0.0005,
            Some(st.clone()),
        )]);
        let eval2 = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-10");
        assert!(eval2.crossings.is_empty(), "{:?}", eval2.crossings);
        assert_eq!(eval2.updated_states[0].1.breach_streak, st.breach_streak);

        // A changed print is a distinct observation: the breach re-raises past
        // the acknowledgment.
        metrics.expense_ratio = Some(0.010);
        let eval3 = evaluate_ledger_conditions(&carried, &metrics, &fin, "2026-08-17");
        assert_eq!(eval3.crossings.len(), 1);
        assert_eq!(eval3.crossings[0].outcome, CrossingOutcome::Confirmed);
    }

    #[test]
    fn unresolvable_series_reads_unevaluable_never_a_silent_clear() {
        let fin = strong();
        let metrics = compute_metrics(&fin);
        let ledger = ledger_of(vec![quant_cond(
            "e",
            LedgerSeries::ExpenseRatio,
            LedgerComparator::Above,
            0.01,
            0.0,
            None,
        )]);
        let eval = evaluate_ledger_conditions(&ledger, &metrics, &fin, "2026-08-03");
        assert!(eval.crossings.is_empty());
        assert_eq!(eval.unevaluable.len(), 1);
        assert!(
            eval.updated_states.is_empty(),
            "state untouched on an unevaluable family"
        );
    }

    /// The implied-expectations inversion round-trips the pricing arithmetic:
    /// inverting at a scenario's own price recovers that scenario's driver,
    /// because both sides read the one shared multiple derivation.
    #[test]
    fn implied_expectations_round_trips_the_scenario_multiples() {
        // Constant spread 0.05 + DGS10 0.05 → every scenario multiple is 10.
        let obs: Vec<AnchorObservation> = (0..8)
            .map(|_| AnchorObservation {
                spread: Some(0.05),
                raw_multiple: 10.0,
            })
            .collect();
        let drivers = [9.0, 10.0, 11.0];
        let scenario = spread_anchored_scenarios(100.0, drivers, &obs, 0.05, 0.0, 0.0);
        assert!(scenario.rate_anchored);
        assert_eq!(scenario.base, 100.0);
        // Inverting at the base price recovers the base driver exactly.
        let ie = implied_expectations(
            scenario.base,
            &scenario,
            Some(8.0),
            "consensus forward EPS",
            true,
            0.05,
        )
        .expect("rate-anchored surface inverts");
        assert!((ie.implied_drivers[1] - drivers[1]).abs() < 1e-9, "{ie:?}");
        // Growth vs the trailing print: 10 / 8 − 1 = 25% at every multiple here.
        let g = ie.implied_growth.expect("positive trailing print");
        assert!((g[1] - 0.25).abs() < 1e-9, "{g:?}");
        assert!(ie.rate_anchored);
        assert!(!ie.revenue_based);
        // No trailing print → the level read survives, growth is undefinable.
        let ie = implied_expectations(100.0, &scenario, None, "r", false, 0.05).unwrap();
        assert!(ie.implied_growth.is_none());
        assert!(ie.revenue_based);
    }

    /// The inversion runs opposite to pricing — the cheap bear multiple demands
    /// the largest implied driver — and the current-multiple carry inverts to
    /// nothing (its multiple is derived from the spot itself).
    #[test]
    fn implied_expectations_orders_inversely_and_declines_the_carry() {
        let spreads = [0.02, 0.04, 0.06, 0.08, 0.10, 0.12, 0.14, 0.16];
        let obs: Vec<AnchorObservation> = spreads
            .iter()
            .map(|s| AnchorObservation {
                spread: Some(*s),
                raw_multiple: 12.0,
            })
            .collect();
        let scenario = spread_anchored_scenarios(100.0, [10.0, 10.0, 10.0], &obs, 0.04, 0.0, 0.0);
        let ie = implied_expectations(100.0, &scenario, Some(9.0), "r", true, 0.04).unwrap();
        assert!(
            ie.implied_drivers[0] > ie.implied_drivers[2],
            "the bear multiple is the demanding end: {ie:?}"
        );
        // No anchor history at all → the carry — nothing independent to invert.
        let scenario = spread_anchored_scenarios(100.0, [10.0, 10.0, 10.0], &[], 0.04, 0.0, 0.0);
        assert!(scenario.current_multiple_carry);
        assert!(implied_expectations(100.0, &scenario, Some(9.0), "r", true, 0.04).is_none());
        // A non-positive spot never inverts.
        let scenario = spread_anchored_scenarios(100.0, [10.0, 10.0, 10.0], &obs, 0.04, 0.0, 0.0);
        assert!(implied_expectations(0.0, &scenario, Some(9.0), "r", true, 0.04).is_none());
    }

    /// A minimal narrative-read fixture: an optional NTM consensus mid and an
    /// optional 8-quarter contiguous revenue window (newest first).
    fn narrative_fin(mid: Option<f64>, quarterly_revenue: Option<[f64; 8]>) -> CompanyFinancials {
        let mut fin = CompanyFinancials {
            symbol: "NARR".into(),
            ..CompanyFinancials::default()
        };
        if let Some(mid) = mid {
            fin.consensus = Some(ConsensusEstimate {
                eps_mid: Some(mid),
                ..ConsensusEstimate::default()
            });
        }
        if let Some(revs) = quarterly_revenue {
            fin.quarterly_income = quarter_ends(8)
                .iter()
                .zip(revs)
                .map(|(end, r)| QuarterlyIncomeRow {
                    period_end: end.clone(),
                    filing_date: None,
                    revenue: Some(r),
                    eps_diluted: None,
                    diluted_shares: None,
                    net_income: None,
                    gross_profit: None,
                    cost_of_revenue: None,
                    operating_income: None,
                })
                .collect();
        }
        fin
    }

    #[test]
    fn narrative_revision_form_classifies_hype_justified_and_neutral() {
        // Hype: forward multiple 10 → 13.6 (+36%) on a +10% revision → ratio > 1.5.
        let fin = narrative_fin(Some(11.0), None);
        let n = narrative_vs_reality(&fin, 150.0, Some(100.0), Some(10.0), Some(30)).unwrap();
        assert_eq!(n.form, NarrativeForm::RevisionBased);
        assert_eq!(n.classification, NarrativeClass::Hype);
        assert!(n.matched_rule.is_some());
        assert!(n.ratio.unwrap() > NARRATIVE_HYPE_RATIO);
        // Justified-expensive: a +20% revision underwrites a +8.3% multiple move.
        let fin = narrative_fin(Some(12.0), None);
        let n = narrative_vs_reality(&fin, 130.0, Some(100.0), Some(10.0), Some(30)).unwrap();
        assert_eq!(n.classification, NarrativeClass::JustifiedExpensive);
        assert!(n.matched_rule.is_none());
        // Real expansion over FLAT revisions: the unbounded case — hype, ratio None.
        let fin = narrative_fin(Some(10.0), None);
        let n = narrative_vs_reality(&fin, 150.0, Some(100.0), Some(10.0), Some(30)).unwrap();
        assert_eq!(n.classification, NarrativeClass::Hype);
        assert!(n.ratio.is_none());
        // Sub-floor expansion is neutral even over flat revisions — noise, not a
        // re-rating.
        let n = narrative_vs_reality(&fin, 102.0, Some(100.0), Some(10.0), Some(30)).unwrap();
        assert_eq!(n.classification, NarrativeClass::Neutral);
        assert!(n.matched_rule.is_none());
    }

    #[test]
    fn narrative_falls_back_to_operating_reality_and_types_its_absences() {
        // Thin coverage (no consensus leg on either side) → the
        // operating-reality-vs-price fallback: TTM revenue YoY vs the
        // annualized price move.
        let revs = [110.0, 110.0, 110.0, 110.0, 100.0, 100.0, 100.0, 100.0];
        let fin = narrative_fin(None, Some(revs));
        let n = narrative_vs_reality(&fin, 140.0, Some(100.0), None, Some(365)).unwrap();
        assert_eq!(n.form, NarrativeForm::OperatingReality);
        assert!((n.reality - 0.1).abs() < 1e-9, "{n:?}");
        assert_eq!(n.classification, NarrativeClass::Hype, "{n:?}");
        // Price pace ≈ operating pace reads justified.
        let n = narrative_vs_reality(&fin, 110.0, Some(100.0), None, Some(365)).unwrap();
        assert_eq!(n.classification, NarrativeClass::JustifiedExpensive, "{n:?}");
        // Absences are typed errors, never fabricated neutrals: a debut, a
        // too-short interval, and thin coverage with no statement window.
        assert!(narrative_vs_reality(&fin, 140.0, None, None, Some(365)).is_err());
        assert!(narrative_vs_reality(&fin, 140.0, Some(100.0), None, Some(3)).is_err());
        let bare = narrative_fin(None, None);
        assert!(narrative_vs_reality(&bare, 140.0, Some(100.0), None, Some(365)).is_err());
    }

    #[test]
    fn narrative_hype_caps_the_engine_arm_at_medium_and_low_still_dominates() {
        let out = match analyze(&strong(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let mut clean = (*out).clone();
        clean.low_confidence_grade = false;
        clean.target_meta = TargetMeta { rate_anchored: true, ..Default::default() };
        clean.tier_gaps.clear();
        clean.hurdle.state = crate::portfolio::HurdleState::Clears;
        let fin = strong();
        // The soft cap is a plain min on the engine arm's mechanical conviction.
        let view = engine_view(&clean, &fin, &[], None, false, true);
        assert_eq!(view.conviction, Conviction::Medium, "hype soft cap → Medium");
        // A matched overlay Low outranks the narrative Medium (strictest binds).
        let overlay = crate::portfolio::pre_profit::OverlayConsequences {
            conviction_ceiling: Some(crate::portfolio::pre_profit::ConvictionCeiling::Low),
            ..Default::default()
        };
        let view = engine_view(&clean, &fin, &[], Some(&overlay), false, true);
        assert_eq!(view.conviction, Conviction::Low);
    }
}
