//! The **multi-factor quant composite** — the shared engine's quant lens
//! (`docs/trade-opportunities.md` §The lenses, §Starting parameters;
//! `docs/trade-opportunities-workflow.md` §Step 5c). Each factor is scored against
//! a **sector-adjusted absolute band** — the value winsorized at the band's edges,
//! 50 at the sector's anchor — and against **the company's own history** (where it
//! sits within its own trailing distribution), the two blended; a factor that
//! failed to resolve is **imputed to its band's neutral midpoint**, so a missing
//! call can't silently sink a name. The factors roll up to axes, and the axes to
//! **one composite** weighted by the archetype — never chained cutoffs, since
//! value and momentum are negatively correlated. It is deliberately not a
//! within-cohort rank: no honest within-app cross-section exists to rank within.
//!
//! Pure over [`CompanyFinancials`], the profile sector, the value-creation read and
//! a caller-supplied revision pace, so every rule is unit-testable offline. The
//! output exposes the methodology — the band each factor read, its own-history
//! depth, whether it was winsorized or imputed — so the model can dispute a
//! derivation and not merely a number.

use serde::{Deserialize, Serialize};

use super::engine::{CompanyFinancials, DatedValue};
use super::value_creation::{ttm_windows, TtmWindow, ValueCreationRead};

// ---------------------------------------------------------------------------
// Calibration surface — drafted or sourced as labelled below, calibratable
// (`docs/trade-opportunities.md` §Starting parameters: the factor normalization
// basis and archetype composite weights rows).
// ---------------------------------------------------------------------------

/// The fewest prior own-history points a factor's own-history leg stands on;
/// below it the sector band stands alone.
pub const OWN_HISTORY_MIN_POINTS: usize = 8;

/// The own-history leg's weight in a factor's blended score (the sector band
/// takes the rest).
pub const OWN_HISTORY_WEIGHT: f64 = 0.5;

/// The neutral score a factor that failed to resolve is imputed to — its band's
/// midpoint.
pub const NEUTRAL_SCORE: f64 = 50.0;

/// The weight multiplier on an archetype's dominant axes over the neutral
/// baseline's 1.0.
pub const ARCHETYPE_TILT: f64 = 2.0;

/// A valuation factor's own history stops at a quarter-over-quarter move in the
/// diluted share count beyond this fraction. The dated closes are split-adjusted
/// while a statement's share count is as filed, so a past market capitalization
/// across a split — or a transformative issuance — would stand on a different
/// basis. A split since the newest quarter needs no stop: every past market
/// capitalization is rescaled onto the quote's share basis
/// ([`window_market_cap`]).
pub const OWN_HISTORY_SHARE_JUMP: f64 = 0.10;

/// The trading sessions per year the volatility read annualizes over, and the
/// trailing window it measures.
const SESSIONS_PER_YEAR: usize = 252;
/// The fewest daily returns a volatility read stands on.
const VOLATILITY_MIN_RETURNS: usize = 120;
/// The trend read's moving-average length, in sessions.
const TREND_SESSIONS: usize = 200;
/// The momentum read's lookback and skip, in calendar days: the return from a
/// year back to a month back (12−1).
const MOMENTUM_LOOKBACK_DAYS: i64 = 365;
const MOMENTUM_SKIP_DAYS: i64 = 30;
/// A price read as of a date needs a close within this many calendar days on or
/// before it — so a gapped series can't stand one stale close in for every
/// window, and momentum's lookback must reach a year back.
const PRICE_PROXIMITY_DAYS: i64 = 10;

/// One sector's anchors — the 50-point centre of each sector-banded factor, plus
/// the gross-margin anchor the value-creation read's moat proxy compares against.
///
/// **Sourced** (Aswath Damodaran's January 2026 US industry tables, NYU Stern):
/// each sector's value is the plain mean over its representative industries
/// ([`SECTOR_ANCHORS`]) — of the yields for the multiples, of the levels for the
/// margins. EBIT/EV is the reciprocal of EV/EBIT over positive-EBITDA firms
/// (`vebitda.html`); earnings yield the reciprocal of aggregate market cap over
/// trailing net income of money-making firms (`pedata.html`); book-to-price the
/// reciprocal of P/BV (`pbvdata.html`); gross margin and the pre-tax unadjusted
/// operating margin from `margin.html`. Damodaran's figures are aggregates
/// (summed numerators over summed denominators, mega-cap weighted), not medians.
///
/// **Drafted** (no matching published definition): gross profitability (GP /
/// assets — no source publishes it by sector); debt / equity (Damodaran's ratio
/// is market-value debt over market equity, the band reads book debt over book
/// equity); and volatility (Damodaran's standard deviation is a multi-year
/// average of firm-level figures, the band reads one year of realized daily
/// returns). The FCF-yield anchor is no column at all: it is the earnings-yield
/// anchor × the FCF-conversion band's anchor ([`fcf_yield_anchor`]).
///
/// **Placeholders**: Financial Services' gross profitability, margins, debt /
/// equity and volatility are drafted stand-ins — bank economics carry no gross
/// profit or operating margin — until the grade's sector-appropriate metric
/// selection lands; its EBIT/EV reads its insurers and asset managers alone,
/// banks reporting no EBIT.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectorAnchors {
    /// The FMP sector label this row serves, or `"market-wide"`.
    pub label: &'static str,
    pub earnings_yield: f64,
    pub ebit_to_ev: f64,
    pub book_to_price: f64,
    pub gross_profitability: f64,
    pub operating_margin: f64,
    pub gross_margin: f64,
    pub debt_to_equity: f64,
    /// Annualized realized volatility of daily returns.
    pub volatility: f64,
}

/// The market-wide row — Damodaran's total market excluding financials for the
/// sourced columns — and the fallback for an unmapped or absent sector.
pub const MARKET_ANCHORS: SectorAnchors = SectorAnchors {
    label: "market-wide",
    earnings_yield: 0.0347,
    ebit_to_ev: 0.0389,
    book_to_price: 0.196,
    gross_profitability: 0.25,
    operating_margin: 0.131,
    gross_margin: 0.344,
    debt_to_equity: 0.80,
    volatility: 0.32,
};

/// FMP's eleven sectors, each over its representative Damodaran industries:
/// Technology — software (system & application), semiconductors, semiconductor
/// equipment, computers / peripherals; Healthcare — pharmaceuticals, healthcare
/// products (biotech's loss-distorted aggregates left out); Financial Services —
/// money-center banks, regional banks, property / casualty insurers, investments
/// & asset management; Real Estate — REITs, real-estate operations & services;
/// Communication Services — telecom services, entertainment; Consumer Cyclical —
/// general retail, specialty retail, restaurants / dining; Consumer Defensive —
/// food processing, household products, soft beverages; Industrials —
/// machinery, aerospace / defense, electrical equipment; Energy — integrated oil
/// & gas, exploration & production, oilfield services / equipment; Basic
/// Materials — specialty chemicals, metals & mining; Utilities — general
/// utilities, power.
pub const SECTOR_ANCHORS: [SectorAnchors; 11] = [
    SectorAnchors {
        label: "Technology",
        earnings_yield: 0.0264,
        ebit_to_ev: 0.0305,
        book_to_price: 0.079,
        gross_profitability: 0.35,
        operating_margin: 0.292,
        gross_margin: 0.538,
        debt_to_equity: 0.40,
        volatility: 0.40,
    },
    SectorAnchors {
        label: "Healthcare",
        earnings_yield: 0.0365,
        ebit_to_ev: 0.0413,
        book_to_price: 0.188,
        gross_profitability: 0.30,
        operating_margin: 0.224,
        gross_margin: 0.629,
        debt_to_equity: 0.60,
        volatility: 0.38,
    },
    SectorAnchors {
        label: "Financial Services",
        earnings_yield: 0.0696,
        ebit_to_ev: 0.0764,
        book_to_price: 0.598,
        gross_profitability: 0.05,
        operating_margin: 0.25,
        gross_margin: 0.50,
        debt_to_equity: 1.50,
        volatility: 0.28,
    },
    SectorAnchors {
        label: "Real Estate",
        earnings_yield: 0.0292,
        ebit_to_ev: 0.0218,
        book_to_price: 0.415,
        gross_profitability: 0.06,
        operating_margin: 0.137,
        gross_margin: 0.451,
        debt_to_equity: 1.20,
        volatility: 0.28,
    },
    SectorAnchors {
        label: "Communication Services",
        earnings_yield: 0.0777,
        ebit_to_ev: 0.0538,
        book_to_price: 0.409,
        gross_profitability: 0.25,
        operating_margin: 0.155,
        gross_margin: 0.501,
        debt_to_equity: 1.00,
        volatility: 0.32,
    },
    SectorAnchors {
        label: "Consumer Cyclical",
        earnings_yield: 0.0335,
        ebit_to_ev: 0.0380,
        book_to_price: 0.097,
        gross_profitability: 0.35,
        operating_margin: 0.101,
        gross_margin: 0.336,
        debt_to_equity: 0.80,
        volatility: 0.35,
    },
    SectorAnchors {
        label: "Consumer Defensive",
        earnings_yield: 0.0499,
        ebit_to_ev: 0.0616,
        book_to_price: 0.291,
        gross_profitability: 0.35,
        operating_margin: 0.166,
        gross_margin: 0.430,
        debt_to_equity: 0.90,
        volatility: 0.22,
    },
    SectorAnchors {
        label: "Industrials",
        earnings_yield: 0.0290,
        ebit_to_ev: 0.0331,
        book_to_price: 0.168,
        gross_profitability: 0.25,
        operating_margin: 0.113,
        gross_margin: 0.289,
        debt_to_equity: 0.80,
        volatility: 0.30,
    },
    SectorAnchors {
        label: "Energy",
        earnings_yield: 0.0610,
        ebit_to_ev: 0.0768,
        book_to_price: 0.614,
        gross_profitability: 0.20,
        operating_margin: 0.138,
        gross_margin: 0.348,
        debt_to_equity: 0.50,
        volatility: 0.35,
    },
    SectorAnchors {
        label: "Basic Materials",
        earnings_yield: 0.0385,
        ebit_to_ev: 0.0553,
        book_to_price: 0.319,
        gross_profitability: 0.20,
        operating_margin: 0.180,
        gross_margin: 0.349,
        debt_to_equity: 0.50,
        volatility: 0.33,
    },
    SectorAnchors {
        label: "Utilities",
        earnings_yield: 0.0482,
        ebit_to_ev: 0.0450,
        book_to_price: 0.496,
        gross_profitability: 0.08,
        operating_margin: 0.225,
        gross_margin: 0.439,
        debt_to_equity: 1.50,
        volatility: 0.20,
    },
];

/// The FCF-yield anchor: the sector's earnings-yield anchor × the FCF-conversion
/// band's anchor, so a name converting earnings to cash at that anchor scores
/// alike on both yields.
pub fn fcf_yield_anchor(anchors: &SectorAnchors) -> f64 {
    anchors.earnings_yield * FCF_CONVERSION_BAND.0
}

/// A sector-banded factor's band half-width as a fraction of its anchor: 0 and
/// 100 sit at the anchor ∓ this share of it. Ratio factors span from nothing to
/// twice the sector's level; volatility from half to one and a half times it.
const SECTOR_BAND_RELATIVE_WIDTH: f64 = 1.0;
const VOLATILITY_BAND_RELATIVE_WIDTH: f64 = 0.5;

/// The market-wide bands — `(anchor, half-width)` — for the factors whose level
/// carries no structural sector difference (momentum and trend, the revision
/// pace, the ROIC spread already net of the cost of capital, FCF conversion,
/// revenue growth, beta, asset growth).
const MOMENTUM_BAND: (f64, f64) = (0.08, 0.40);
const TREND_BAND: (f64, f64) = (0.0, 0.20);
const REVISION_BAND: (f64, f64) = (0.0, 0.20);
const ROIC_SPREAD_BAND: (f64, f64) = (0.0, 0.15);
const FCF_CONVERSION_BAND: (f64, f64) = (0.90, 0.60);
const REVENUE_GROWTH_BAND: (f64, f64) = (0.06, 0.20);
const BETA_BAND: (f64, f64) = (1.0, 0.60);
const ASSET_GROWTH_BAND: (f64, f64) = (0.07, 0.22);

// ---------------------------------------------------------------------------
// Vocabulary.
// ---------------------------------------------------------------------------

/// The five archetypes the valuation lens and the composite's weighting key on
/// (`docs/trade-opportunities.md` §Archetype). Classification is the job's, not
/// the engine's; a caller with no archetype (Portfolio) takes neutral weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Archetype {
    SecularCompounder,
    AiInfra,
    CommodityCyclical,
    Disruptor,
    QualityCompounder,
}

/// The composite's axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Axis {
    Value,
    Momentum,
    Quality,
    Growth,
    LowRisk,
    Investment,
    Revision,
}

impl Axis {
    pub const ALL: [Axis; 7] = [
        Axis::Value,
        Axis::Momentum,
        Axis::Quality,
        Axis::Growth,
        Axis::LowRisk,
        Axis::Investment,
        Axis::Revision,
    ];
}

/// One factor of the composite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Factor {
    /// TTM net income over market capitalization.
    EarningsYield,
    /// TTM free cash flow over market capitalization.
    FcfYield,
    /// TTM operating income over enterprise value (market cap + debt − cash).
    EbitToEv,
    /// Book equity over market capitalization — the commodity cyclical's value
    /// axis alone, its P/B lens (no P/NAV source exists).
    BookToPrice,
    /// The 12−1 return: a year back to a month back.
    Momentum,
    /// The last close over its 200-session average, less one.
    Trend,
    /// TTM gross profit over total assets.
    GrossProfitability,
    /// TTM operating income over TTM revenue.
    OperatingMargin,
    /// ROIC less the cost of capital (`portfolio::value_creation`).
    RoicSpread,
    /// TTM free cash flow over a positive TTM net income.
    FcfConversion,
    /// TTM revenue over the prior TTM's, less one.
    RevenueGrowth,
    /// The profile's served beta.
    Beta,
    /// Annualized realized volatility over the trailing year of sessions.
    Volatility,
    /// Book debt over book equity; non-positive equity scores the band's floor.
    DebtToEquity,
    /// Total assets over the year-earlier print, less one — conservative
    /// investment, inverted.
    AssetGrowth,
    /// The caller-supplied consensus revision pace.
    Revision,
}

/// Which direction scores higher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Polarity {
    HigherBetter,
    LowerBetter,
}

impl Factor {
    pub fn axis(self) -> Axis {
        use Factor::*;
        match self {
            EarningsYield | FcfYield | EbitToEv | BookToPrice => Axis::Value,
            Momentum | Trend => Axis::Momentum,
            GrossProfitability | OperatingMargin | RoicSpread | FcfConversion => Axis::Quality,
            RevenueGrowth => Axis::Growth,
            Beta | Volatility | DebtToEquity => Axis::LowRisk,
            AssetGrowth => Axis::Investment,
            Revision => Axis::Revision,
        }
    }

    fn polarity(self) -> Polarity {
        match self {
            Factor::Beta | Factor::Volatility | Factor::DebtToEquity | Factor::AssetGrowth => {
                Polarity::LowerBetter
            }
            _ => Polarity::HigherBetter,
        }
    }

    /// The factor's band under a sector row: sector-banded factors read the row
    /// (the market-wide row when the sector is unmapped), the rest one market-wide
    /// band.
    fn band(self, anchors: &SectorAnchors, sector_basis: BandBasis) -> Band {
        let sector = |anchor: f64, width: f64| Band {
            anchor,
            half_width: anchor.abs() * width,
            basis: sector_basis,
        };
        let market = |(anchor, half_width): (f64, f64)| Band {
            anchor,
            half_width,
            basis: BandBasis::MarketWide,
        };
        match self {
            Factor::EarningsYield => sector(anchors.earnings_yield, SECTOR_BAND_RELATIVE_WIDTH),
            Factor::FcfYield => sector(fcf_yield_anchor(anchors), SECTOR_BAND_RELATIVE_WIDTH),
            Factor::EbitToEv => sector(anchors.ebit_to_ev, SECTOR_BAND_RELATIVE_WIDTH),
            Factor::BookToPrice => sector(anchors.book_to_price, SECTOR_BAND_RELATIVE_WIDTH),
            Factor::GrossProfitability => {
                sector(anchors.gross_profitability, SECTOR_BAND_RELATIVE_WIDTH)
            }
            Factor::OperatingMargin => sector(anchors.operating_margin, SECTOR_BAND_RELATIVE_WIDTH),
            Factor::DebtToEquity => sector(anchors.debt_to_equity, SECTOR_BAND_RELATIVE_WIDTH),
            Factor::Volatility => sector(anchors.volatility, VOLATILITY_BAND_RELATIVE_WIDTH),
            Factor::Momentum => market(MOMENTUM_BAND),
            Factor::Trend => market(TREND_BAND),
            Factor::Revision => market(REVISION_BAND),
            Factor::RoicSpread => market(ROIC_SPREAD_BAND),
            Factor::FcfConversion => market(FCF_CONVERSION_BAND),
            Factor::RevenueGrowth => market(REVENUE_GROWTH_BAND),
            Factor::Beta => market(BETA_BAND),
            Factor::AssetGrowth => market(ASSET_GROWTH_BAND),
        }
    }
}

/// Which anchor a band stood on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BandBasis {
    /// The holding's own sector row.
    Sector,
    /// A sector-banded factor whose sector is absent or unmapped — the
    /// market-wide row stood in.
    UnmappedSector,
    /// A factor banded market-wide by design.
    MarketWide,
}

/// A factor's band: 50 at the anchor, 0 and 100 at the anchor ∓ the half-width
/// (polarity deciding which end scores 100).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub anchor: f64,
    pub half_width: f64,
    pub basis: BandBasis,
}

/// The sector row for a profile sector label (matched case-insensitively) with
/// its basis: the sector's own row, or [`MARKET_ANCHORS`] read as an unmapped
/// sector when the label is absent or unmapped.
pub fn sector_row(sector: Option<&str>) -> (&'static SectorAnchors, BandBasis) {
    match sector
        .map(str::trim)
        .and_then(|s| SECTOR_ANCHORS.iter().find(|row| row.label.eq_ignore_ascii_case(s)))
    {
        Some(row) => (row, BandBasis::Sector),
        None => (&MARKET_ANCHORS, BandBasis::UnmappedSector),
    }
}

/// The sector row alone ([`sector_row`]).
pub fn sector_anchors(sector: Option<&str>) -> &'static SectorAnchors {
    sector_row(sector).0
}

/// The axes an archetype weights at [`ARCHETYPE_TILT`]; empty for no archetype.
pub fn dominant_axes(archetype: Option<Archetype>) -> &'static [Axis] {
    match archetype {
        None => &[],
        Some(Archetype::AiInfra) => &[Axis::Momentum, Axis::Revision],
        Some(Archetype::CommodityCyclical) => &[Axis::Value],
        Some(Archetype::SecularCompounder) | Some(Archetype::QualityCompounder) => {
            &[Axis::Quality]
        }
        Some(Archetype::Disruptor) => &[Axis::Growth],
    }
}

/// The factors the composite reads under an archetype: the commodity cyclical's
/// value axis reads book-to-price alone; every other value axis reads the three
/// yields.
pub fn factor_set(archetype: Option<Archetype>) -> Vec<Factor> {
    use Factor::*;
    let value: &[Factor] = if archetype == Some(Archetype::CommodityCyclical) {
        &[BookToPrice]
    } else {
        &[EarningsYield, FcfYield, EbitToEv]
    };
    value
        .iter()
        .copied()
        .chain([
            Momentum,
            Trend,
            GrossProfitability,
            OperatingMargin,
            RoicSpread,
            FcfConversion,
            RevenueGrowth,
            Beta,
            Volatility,
            DebtToEquity,
            AssetGrowth,
            Revision,
        ])
        .collect()
}

// ---------------------------------------------------------------------------
// Output.
// ---------------------------------------------------------------------------

/// A factor's own-history leg.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OwnHistory {
    /// The prior points the percentile read (≥ [`OWN_HISTORY_MIN_POINTS`]).
    pub points: usize,
    /// Where today's value sits within them, 0–100 with polarity applied.
    pub score: f64,
}

/// One factor's scoring, with its methodology exposed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactorScore {
    pub factor: Factor,
    /// Today's raw value; `None` when it failed to resolve, or when it is
    /// [`Self::floored`].
    pub raw: Option<f64>,
    /// The value has no finite level but scores the band's floor — debt over a
    /// non-positive equity.
    pub floored: bool,
    pub band: Band,
    /// The raw value sat outside the band and was clamped to its edge.
    pub winsorized: bool,
    /// The sector-band leg's score.
    pub band_score: Option<f64>,
    /// The own-history leg; `None` below [`OWN_HISTORY_MIN_POINTS`] prior points.
    pub own_history: Option<OwnHistory>,
    /// The blended score, 0–100.
    pub score: f64,
    /// The factor failed to resolve and stands at [`NEUTRAL_SCORE`].
    pub imputed: bool,
}

/// One axis's score and its weight in the composite.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AxisScore {
    pub axis: Axis,
    /// The mean of its factors' scores, 0–100.
    pub score: f64,
    pub weight: f64,
}

/// The composite and its methodology.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactorComposite {
    pub archetype: Option<Archetype>,
    /// The sector row the sector-banded factors read (`"market-wide"` where the
    /// sector was absent or unmapped).
    pub sector_anchor: String,
    pub factors: Vec<FactorScore>,
    pub axes: Vec<AxisScore>,
    /// The weighted mean of the axes, 0–100.
    pub composite: f64,
    /// The low-confidence degraded-input flag: more than half the resolved
    /// factors score on their band alone, with no own-history leg — a data-health
    /// note the model reads, never a gate.
    pub thin_own_history: bool,
}

/// What the composite reads.
pub struct FactorInputs<'a> {
    pub fin: &'a CompanyFinancials,
    /// The profile's sector label.
    pub sector: Option<&'a str>,
    pub value_creation: &'a ValueCreationRead,
    /// The consensus revision pace the caller measured (a decimal change in the
    /// forward EPS consensus); `None` scores the revision axis neutral.
    pub revision_pace: Option<f64>,
}

// ---------------------------------------------------------------------------
// Derivation.
// ---------------------------------------------------------------------------

/// A factor's observation: today's value and its own-history points (prior
/// windows, newest first). A value may be `+∞` — debt over a non-positive
/// equity, the worst read the band can hold.
struct Observation {
    current: Option<f64>,
    prior: Vec<f64>,
}

fn ratio(num: Option<f64>, den: Option<f64>) -> Option<f64> {
    match (num, den) {
        (Some(n), Some(d)) if d > 0.0 && n.is_finite() => Some(n / d),
        _ => None,
    }
}

/// The closes on or before `as_of`, oldest first.
fn closes_through<'a>(closes: &'a [DatedValue], as_of: &str) -> &'a [DatedValue] {
    let end = closes.partition_point(|c| c.date.as_str() <= as_of);
    &closes[..end]
}

fn shift(date: &str, days: i64) -> Option<String> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.checked_sub_signed(chrono::Duration::days(days)))
        .map(|d| d.format("%Y-%m-%d").to_string())
}

/// The last close on or before `date`, within [`PRICE_PROXIMITY_DAYS`] of it;
/// `None` where the series has no close that near.
fn close_near<'a>(closes: &'a [DatedValue], date: &str) -> Option<&'a DatedValue> {
    let last = closes_through(closes, date).last()?;
    let earliest = shift(date, PRICE_PROXIMITY_DAYS)?;
    (last.date >= earliest).then_some(last)
}

/// The dated closes with each session once, in date order — the last row served
/// for a duplicated date wins (the engine's
/// [`super::engine::latest_on_or_before`] rule) — so every session-counting read
/// counts sessions, never rows.
fn dedupe_closes(closes: &[DatedValue]) -> Vec<DatedValue> {
    let mut sorted = closes.to_vec();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));
    let mut out: Vec<DatedValue> = Vec::with_capacity(sorted.len());
    for close in sorted {
        match out.last_mut() {
            Some(last) if last.date == close.date => *last = close,
            _ => out.push(close),
        }
    }
    out
}

/// The 12−1 return as of `as_of`; `None` where the series has no close near a
/// year back or near a month back.
fn momentum_as_of(closes: &[DatedValue], as_of: &str) -> Option<f64> {
    let base = close_near(closes, &shift(as_of, MOMENTUM_LOOKBACK_DAYS)?)?;
    let recent = close_near(closes, &shift(as_of, MOMENTUM_SKIP_DAYS)?)?;
    (base.value > 0.0).then(|| recent.value / base.value - 1.0)
}

/// The last close over its trailing [`TREND_SESSIONS`]-session average, less one.
fn trend_as_of(closes: &[DatedValue], as_of: &str) -> Option<f64> {
    close_near(closes, as_of)?;
    let window = closes_through(closes, as_of);
    if window.len() < TREND_SESSIONS {
        return None;
    }
    let tail = &window[window.len() - TREND_SESSIONS..];
    let mean = tail.iter().map(|c| c.value).sum::<f64>() / TREND_SESSIONS as f64;
    (mean > 0.0).then(|| tail[TREND_SESSIONS - 1].value / mean - 1.0)
}

/// Annualized volatility of daily log returns over the trailing year of
/// sessions through `as_of`.
fn volatility_as_of(closes: &[DatedValue], as_of: &str) -> Option<f64> {
    close_near(closes, as_of)?;
    let window = closes_through(closes, as_of);
    let tail = &window[window.len().saturating_sub(SESSIONS_PER_YEAR + 1)..];
    let returns: Vec<f64> = tail
        .windows(2)
        .filter(|p| p[0].value > 0.0 && p[1].value > 0.0)
        .map(|p| (p[1].value / p[0].value).ln())
        .collect();
    if returns.len() < VOLATILITY_MIN_RETURNS {
        return None;
    }
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Some(var.sqrt() * (SESSIONS_PER_YEAR as f64).sqrt())
}

/// How many prior windows a valuation factor's own history may read: the run of
/// windows whose diluted share count stays within [`OWN_HISTORY_SHARE_JUMP`] of
/// its newer neighbour.
fn share_continuous_prior(windows: &[TtmWindow]) -> usize {
    let within = |newer: f64, older: f64| {
        newer > 0.0 && older > 0.0 && (newer / older - 1.0).abs() <= OWN_HISTORY_SHARE_JUMP
    };
    let mut prior = 0;
    for pair in windows.windows(2) {
        match (pair[0].diluted_shares, pair[1].diluted_shares) {
            (Some(newer), Some(older)) if within(newer, older) => prior += 1,
            _ => break,
        }
    }
    prior
}

/// The factor that puts a past window's market capitalization on the quote's
/// share basis: the quote's share count over the newest quarter's diluted count.
/// It carries a split since the newest quarter (the split-adjusted closes moved,
/// the filed count did not) and the quote's own basis (basic shares, a listed
/// class) alike. `None` without both counts — the valuation factors then read no
/// own history.
fn quote_share_basis(fin: &CompanyFinancials, windows: &[TtmWindow]) -> Option<f64> {
    let quote = fin.shares_outstanding.filter(|s| s.is_finite() && *s > 0.0)?;
    let newest = windows.first()?.diluted_shares.filter(|s| s.is_finite() && *s > 0.0)?;
    Some(quote / newest)
}

/// A past window's market capitalization on the quote's share basis: the close
/// near its anchor date × its newest quarter's diluted share count × `basis`.
fn window_market_cap(closes: &[DatedValue], window: &TtmWindow, basis: f64) -> Option<f64> {
    let close = close_near(closes, window.anchor_date.as_deref()?)?.value;
    let shares = window.diluted_shares?;
    (close > 0.0 && shares > 0.0).then_some(close * shares * basis)
}

fn enterprise_value(market_cap: f64, window: &TtmWindow) -> Option<f64> {
    let ev = market_cap + window.total_debt? - window.cash?;
    (ev > 0.0).then_some(ev)
}

/// Book debt over book equity. No debt is no leverage whatever the equity;
/// positive debt over a non-positive equity is levered beyond the equity base and
/// reads `+∞`, the band's floor.
fn debt_to_equity(window: &TtmWindow) -> Option<f64> {
    let debt = window.total_debt?;
    let equity = window.total_equity?;
    if !debt.is_finite() || !equity.is_finite() || debt < 0.0 {
        return None;
    }
    Some(if debt == 0.0 {
        0.0
    } else if equity > 0.0 {
        debt / equity
    } else {
        f64::INFINITY
    })
}

fn observe(
    factor: Factor,
    inputs: &FactorInputs,
    windows: &[TtmWindow],
    closes: &[DatedValue],
) -> Observation {
    let fin = inputs.fin;
    let none = Observation { current: None, prior: vec![] };
    let Some(w0) = windows.first() else {
        // No statement window: only the price, beta, spread and revision reads
        // can stand.
        return match factor {
            Factor::Momentum | Factor::Trend | Factor::Volatility => {
                price_observation(factor, closes, &[])
            }
            Factor::Beta => Observation { current: fin.beta.filter(|b| b.is_finite()), prior: vec![] },
            Factor::RoicSpread => Observation {
                current: inputs.value_creation.roic_spread,
                prior: vec![],
            },
            Factor::Revision => Observation { current: inputs.revision_pace, prior: vec![] },
            _ => none,
        };
    };
    let market_cap = fin.market_cap.filter(|m| m.is_finite() && *m > 0.0);
    // Valuation: today's quote against today's window; the past against each
    // window's own anchor close on the quote's share basis, inside the
    // share-continuous run.
    let basis = quote_share_basis(fin, windows);
    let valuation = |value: &dyn Fn(f64, &TtmWindow) -> Option<f64>| {
        let current = market_cap.and_then(|m| value(m, w0));
        let prior = match basis {
            Some(basis) => windows[1..=share_continuous_prior(windows)]
                .iter()
                .filter_map(|w| window_market_cap(closes, w, basis).and_then(|m| value(m, w)))
                .collect(),
            None => vec![],
        };
        Observation { current, prior }
    };
    // Fundamentals: each window read the same way.
    let fundamental = |value: &dyn Fn(usize) -> Option<f64>| Observation {
        current: value(0),
        prior: (1..windows.len()).filter_map(value).collect(),
    };
    match factor {
        Factor::EarningsYield => valuation(&|m, w| w.net_income.map(|n| n / m)),
        Factor::FcfYield => valuation(&|m, w| w.free_cash_flow.map(|f| f / m)),
        Factor::EbitToEv => {
            valuation(&|m, w| ratio(w.operating_income, enterprise_value(m, w)))
        }
        Factor::BookToPrice => valuation(&|m, w| w.total_equity.map(|e| e / m)),
        Factor::GrossProfitability => {
            fundamental(&|k| ratio(windows[k].gross_profit, windows[k].total_assets))
        }
        Factor::OperatingMargin => {
            fundamental(&|k| ratio(windows[k].operating_income, windows[k].revenue))
        }
        Factor::FcfConversion => {
            fundamental(&|k| ratio(windows[k].free_cash_flow, windows[k].net_income))
        }
        Factor::RevenueGrowth => fundamental(&|k| {
            ratio(windows[k].revenue, windows.get(k + 4)?.revenue).map(|r| r - 1.0)
        }),
        Factor::AssetGrowth => fundamental(&|k| {
            ratio(windows[k].total_assets, windows.get(k + 4)?.total_assets).map(|r| r - 1.0)
        }),
        Factor::DebtToEquity => fundamental(&|k| debt_to_equity(&windows[k])),
        Factor::Momentum | Factor::Trend | Factor::Volatility => {
            price_observation(factor, closes, windows)
        }
        Factor::Beta => Observation { current: fin.beta.filter(|b| b.is_finite()), prior: vec![] },
        Factor::RoicSpread => Observation {
            current: inputs.value_creation.roic_spread,
            prior: vec![],
        },
        Factor::Revision => Observation { current: inputs.revision_pace, prior: vec![] },
    }
}

/// A price-series factor today (as of the newest close) and at each prior
/// window's anchor date.
fn price_observation(factor: Factor, closes: &[DatedValue], windows: &[TtmWindow]) -> Observation {
    let read = |as_of: &str| match factor {
        Factor::Momentum => momentum_as_of(closes, as_of),
        Factor::Trend => trend_as_of(closes, as_of),
        _ => volatility_as_of(closes, as_of),
    };
    let current = closes.last().and_then(|c| read(&c.date));
    let prior = windows
        .iter()
        .skip(1)
        .filter_map(|w| w.anchor_date.as_deref())
        .filter(|d| closes.last().is_some_and(|c| *d < c.date.as_str()))
        .filter_map(read)
        .collect();
    Observation { current, prior }
}

// ---------------------------------------------------------------------------
// Scoring.
// ---------------------------------------------------------------------------

/// The band leg: the value winsorized at the band's edges, then mapped linearly
/// — 50 at the anchor, 0 / 100 at the edges by polarity. Returns the score and
/// whether the value was clamped.
fn band_score(value: f64, band: &Band, polarity: Polarity) -> (f64, bool) {
    let lo = band.anchor - band.half_width;
    let hi = band.anchor + band.half_width;
    let clamped = value.clamp(lo, hi);
    let t = if band.half_width > 0.0 { (clamped - band.anchor) / band.half_width } else { 0.0 };
    let score = match polarity {
        Polarity::HigherBetter => 50.0 + 50.0 * t,
        Polarity::LowerBetter => 50.0 - 50.0 * t,
    };
    // Clamped again so floating-point rounding at an edge can't stray past it.
    (score.clamp(0.0, 100.0), clamped != value)
}

/// The own-history leg: today's mid-rank percentile among the prior points,
/// polarity applied; `None` below [`OWN_HISTORY_MIN_POINTS`].
fn own_history_score(current: f64, prior: &[f64], polarity: Polarity) -> Option<OwnHistory> {
    if prior.len() < OWN_HISTORY_MIN_POINTS {
        return None;
    }
    let below = prior.iter().filter(|p| **p < current).count() as f64;
    let equal = prior.iter().filter(|p| **p == current).count() as f64;
    let percentile = 100.0 * (below + 0.5 * equal) / prior.len() as f64;
    let score = match polarity {
        Polarity::HigherBetter => percentile,
        Polarity::LowerBetter => 100.0 - percentile,
    };
    Some(OwnHistory { points: prior.len(), score })
}

/// Score one factor from its observation.
fn score_factor(factor: Factor, band: Band, obs: &Observation) -> FactorScore {
    let polarity = factor.polarity();
    let Some(current) = obs.current.filter(|v| !v.is_nan()) else {
        return FactorScore {
            factor,
            raw: None,
            floored: false,
            band,
            winsorized: false,
            band_score: None,
            own_history: None,
            score: NEUTRAL_SCORE,
            imputed: true,
        };
    };
    let (band_leg, winsorized) = band_score(current, &band, polarity);
    let own_history = own_history_score(current, &obs.prior, polarity);
    let score = match own_history {
        Some(own) => (1.0 - OWN_HISTORY_WEIGHT) * band_leg + OWN_HISTORY_WEIGHT * own.score,
        None => band_leg,
    };
    FactorScore {
        factor,
        raw: current.is_finite().then_some(current),
        floored: !current.is_finite(),
        band,
        winsorized,
        band_score: Some(band_leg),
        own_history,
        score,
        imputed: false,
    }
}

/// The thin-own-history flag: more than half the resolved factors score on their
/// band alone — or nothing resolved at all.
fn thin_own_history(factors: &[FactorScore]) -> bool {
    let resolved = factors.iter().filter(|f| !f.imputed).count();
    let band_only = factors
        .iter()
        .filter(|f| !f.imputed && f.own_history.is_none())
        .count();
    resolved == 0 || 2 * band_only > resolved
}

/// The archetype-weighted multi-factor composite for one stock.
pub fn factor_composite(inputs: &FactorInputs, archetype: Option<Archetype>) -> FactorComposite {
    let (anchors, sector_basis) = sector_row(inputs.sector);
    let windows = ttm_windows(inputs.fin);
    let closes = dedupe_closes(&inputs.fin.daily_closes);
    let factors: Vec<FactorScore> = factor_set(archetype)
        .into_iter()
        .map(|factor| {
            let obs = observe(factor, inputs, &windows, &closes);
            score_factor(factor, factor.band(anchors, sector_basis), &obs)
        })
        .collect();

    let dominant = dominant_axes(archetype);
    let axes: Vec<AxisScore> = Axis::ALL
        .iter()
        .map(|&axis| {
            let scores: Vec<f64> = factors
                .iter()
                .filter(|f| f.factor.axis() == axis)
                .map(|f| f.score)
                .collect();
            AxisScore {
                axis,
                score: scores.iter().sum::<f64>() / scores.len() as f64,
                weight: if dominant.contains(&axis) { ARCHETYPE_TILT } else { 1.0 },
            }
        })
        .collect();
    let weight_sum: f64 = axes.iter().map(|a| a.weight).sum();
    let composite = axes.iter().map(|a| a.weight * a.score).sum::<f64>() / weight_sum;

    FactorComposite {
        archetype,
        sector_anchor: anchors.label.to_string(),
        thin_own_history: thin_own_history(&factors),
        factors,
        axes,
        composite,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::value_creation::tests::{approx, plain_name};
    use crate::portfolio::value_creation::value_creation_read;

    /// Weekday closes from `from` through `to` inclusive, each priced by `price`.
    fn weekday_closes(from: &str, to: &str, price: impl Fn(chrono::NaiveDate) -> f64) -> Vec<DatedValue> {
        use chrono::Datelike;
        let mut day = chrono::NaiveDate::parse_from_str(from, "%Y-%m-%d").unwrap();
        let end = chrono::NaiveDate::parse_from_str(to, "%Y-%m-%d").unwrap();
        let mut out = Vec::new();
        while day <= end {
            if day.weekday().number_from_monday() <= 5 {
                out.push(DatedValue { date: day.format("%Y-%m-%d").to_string(), value: price(day) });
            }
            day = day.succ_opt().unwrap();
        }
        out
    }

    /// The plain fixture with four-plus years of flat closes at 40 — the quote's
    /// level, so every window's market capitalization matches today's.
    fn plain_with_closes() -> CompanyFinancials {
        let mut fin = plain_name();
        fin.daily_closes = weekday_closes("2022-01-03", "2026-09-30", |_| 40.0);
        fin
    }

    fn composite_of(fin: &CompanyFinancials, sector: Option<&str>, archetype: Option<Archetype>) -> FactorComposite {
        let vc = value_creation_read(fin, sector, 0.04);
        factor_composite(
            &FactorInputs { fin, sector, value_creation: &vc, revision_pace: None },
            archetype,
        )
    }

    fn factor(c: &FactorComposite, f: Factor) -> &FactorScore {
        c.factors.iter().find(|s| s.factor == f).unwrap_or_else(|| panic!("{f:?} not scored"))
    }

    #[test]
    fn the_band_scores_fifty_at_the_anchor_and_winsorizes_at_its_edges() {
        let band = Band { anchor: 0.04, half_width: 0.04, basis: BandBasis::Sector };
        let up = Polarity::HigherBetter;
        assert_eq!(band_score(0.04, &band, up), (50.0, false));
        assert_eq!(band_score(0.08, &band, up), (100.0, false));
        assert_eq!(band_score(0.0, &band, up), (0.0, false));
        assert_eq!(band_score(0.12, &band, up), (100.0, true), "clamped to the edge");
        assert_eq!(band_score(-0.02, &band, up), (0.0, true));
        let (score, _) = band_score(0.05, &band, up);
        assert!(approx(score, 62.5));
        // Lower-better inverts the map; an infinite read holds the floor.
        let de = Band { anchor: 0.4, half_width: 0.4, basis: BandBasis::Sector };
        let down = Polarity::LowerBetter;
        assert_eq!(band_score(0.0, &de, down), (100.0, false));
        assert_eq!(band_score(0.8, &de, down), (0.0, false));
        assert_eq!(band_score(f64::INFINITY, &de, down), (0.0, true));
    }

    #[test]
    fn own_history_is_a_mid_rank_percentile_over_at_least_eight_points() {
        let prior: Vec<f64> = (1..=8).map(f64::from).collect();
        let up = Polarity::HigherBetter;
        assert_eq!(own_history_score(4.5, &prior, up), Some(OwnHistory { points: 8, score: 50.0 }));
        // A tie counts half: three below, one equal → 3.5 / 8.
        assert_eq!(own_history_score(4.0, &prior, up).unwrap().score, 43.75);
        assert_eq!(own_history_score(9.0, &prior, up).unwrap().score, 100.0);
        assert_eq!(own_history_score(4.5, &prior, Polarity::LowerBetter).unwrap().score, 50.0);
        assert_eq!(own_history_score(1.0, &prior, Polarity::LowerBetter).unwrap().score, 93.75);
        assert_eq!(own_history_score(4.5, &prior[..7], up), None, "seven points: band alone");
    }

    #[test]
    fn a_factor_blends_its_band_and_own_history_half_and_half() {
        let band = Band { anchor: 0.04, half_width: 0.04, basis: BandBasis::Sector };
        // Band leg: 0.056 → 70. Own leg: three of ten priors below → 30.
        let obs = Observation {
            current: Some(0.056),
            prior: vec![0.01, 0.02, 0.03, 0.06, 0.07, 0.08, 0.09, 0.10, 0.11, 0.12],
        };
        let s = score_factor(Factor::EarningsYield, band, &obs);
        assert!(approx(s.band_score.unwrap(), 70.0));
        assert_eq!(s.own_history, Some(OwnHistory { points: 10, score: 30.0 }));
        assert!(approx(s.score, 50.0));
        assert!(!s.imputed && !s.winsorized);
        // Below the minimum the band stands alone.
        let thin = Observation { current: Some(0.056), prior: vec![0.01; 7] };
        let s = score_factor(Factor::EarningsYield, band, &thin);
        assert_eq!(s.own_history, None);
        assert!(approx(s.score, 70.0));
    }

    #[test]
    fn the_plain_name_reads_its_raw_factors_off_the_statements() {
        let c = composite_of(&plain_with_closes(), Some("Technology"), None);
        assert_eq!(c.sector_anchor, "Technology");
        let raw = |f| factor(&c, f).raw.unwrap();
        assert!(approx(raw(Factor::EarningsYield), 60.0 / 4000.0));
        assert!(approx(raw(Factor::FcfYield), 48.0 / 4000.0));
        assert!(approx(raw(Factor::EbitToEv), 80.0 / 4100.0));
        assert!(approx(raw(Factor::GrossProfitability), 0.16));
        assert!(approx(raw(Factor::OperatingMargin), 0.2));
        assert!(approx(raw(Factor::FcfConversion), 0.8));
        assert!(approx(raw(Factor::RevenueGrowth), 0.0));
        assert!(approx(raw(Factor::AssetGrowth), 0.0));
        assert!(approx(raw(Factor::DebtToEquity), 0.4));
        assert!(approx(raw(Factor::Beta), 1.2));
        assert!(approx(raw(Factor::Momentum), 0.0), "flat closes");
        assert!(approx(raw(Factor::Trend), 0.0));
        assert!(approx(raw(Factor::Volatility), 0.0));
        // Earnings yield on Technology's band: (0.015 − 0.0264) / 0.0264 below
        // the anchor, and a flat own history ranks today mid-pack.
        let ey = factor(&c, Factor::EarningsYield);
        let band = 50.0 + 50.0 * (0.015 - 0.0264) / 0.0264;
        assert!(approx(ey.band_score.unwrap(), band));
        assert_eq!(ey.own_history.unwrap().points, 12);
        assert!(approx(ey.score, 0.5 * band + 0.5 * 50.0));
        // Revision is caller-supplied: absent here, so imputed neutral.
        let rev = factor(&c, Factor::Revision);
        assert!(rev.imputed && rev.score == NEUTRAL_SCORE);
    }

    #[test]
    fn the_neutral_composite_is_the_plain_mean_of_seven_axes() {
        let c = composite_of(&plain_with_closes(), Some("Technology"), None);
        assert_eq!(c.factors.len(), 15);
        assert_eq!(c.axes.len(), 7);
        assert!(c.axes.iter().all(|a| a.weight == 1.0));
        let mean = c.axes.iter().map(|a| a.score).sum::<f64>() / 7.0;
        assert!(approx(c.composite, mean));
        for a in &c.axes {
            let scores: Vec<f64> =
                c.factors.iter().filter(|f| f.factor.axis() == a.axis).map(|f| f.score).collect();
            assert!(approx(a.score, scores.iter().sum::<f64>() / scores.len() as f64));
        }
        assert!((0.0..=100.0).contains(&c.composite));
        assert_eq!(c, composite_of(&plain_with_closes(), Some("Technology"), None), "deterministic");
    }

    #[test]
    fn an_archetype_doubles_its_dominant_axes() {
        let fin = plain_with_closes();
        let weight = |c: &FactorComposite, axis| c.axes.iter().find(|a| a.axis == axis).unwrap().weight;
        let ai = composite_of(&fin, Some("Technology"), Some(Archetype::AiInfra));
        assert_eq!(weight(&ai, Axis::Momentum), ARCHETYPE_TILT);
        assert_eq!(weight(&ai, Axis::Revision), ARCHETYPE_TILT);
        assert_eq!(weight(&ai, Axis::Quality), 1.0);
        let weighted = ai.axes.iter().map(|a| a.weight * a.score).sum::<f64>()
            / ai.axes.iter().map(|a| a.weight).sum::<f64>();
        assert!(approx(ai.composite, weighted));
        for (archetype, axis) in [
            (Archetype::SecularCompounder, Axis::Quality),
            (Archetype::QualityCompounder, Axis::Quality),
            (Archetype::Disruptor, Axis::Growth),
            (Archetype::CommodityCyclical, Axis::Value),
        ] {
            let c = composite_of(&fin, Some("Technology"), Some(archetype));
            assert_eq!(weight(&c, axis), ARCHETYPE_TILT, "{archetype:?}");
            assert_eq!(c.axes.iter().filter(|a| a.weight == ARCHETYPE_TILT).count(), 1);
        }
        // The commodity cyclical's value axis reads book-to-price alone.
        let cyc = composite_of(&fin, Some("Energy"), Some(Archetype::CommodityCyclical));
        let value: Vec<Factor> =
            cyc.factors.iter().filter(|f| f.factor.axis() == Axis::Value).map(|f| f.factor).collect();
        assert_eq!(value, vec![Factor::BookToPrice]);
        assert!(approx(factor(&cyc, Factor::BookToPrice).raw.unwrap(), 500.0 / 4000.0));
        let neutral = composite_of(&fin, Some("Energy"), None);
        assert!(neutral.factors.iter().all(|f| f.factor != Factor::BookToPrice));
    }

    #[test]
    fn a_missing_factor_imputes_neutral_and_never_sinks_the_composite() {
        let mut high_beta = plain_with_closes();
        high_beta.beta = Some(3.0);
        let mut no_beta = plain_with_closes();
        no_beta.beta = None;
        let hb = composite_of(&high_beta, Some("Technology"), None);
        let nb = composite_of(&no_beta, Some("Technology"), None);
        let beta = factor(&nb, Factor::Beta);
        assert!(beta.imputed);
        assert_eq!((beta.raw, beta.band_score, beta.score), (None, None, NEUTRAL_SCORE));
        assert_eq!(factor(&hb, Factor::Beta).score, 0.0);
        assert!(nb.composite > hb.composite, "{} vs {}", nb.composite, hb.composite);
    }

    #[test]
    fn no_debt_reads_no_leverage_whatever_the_equity() {
        let mut fin = plain_with_closes();
        for row in &mut fin.quarterly_balance_sheet {
            row.total_debt = Some(0.0);
            row.total_equity = Some(-100.0);
        }
        let c = composite_of(&fin, Some("Technology"), None);
        let de = factor(&c, Factor::DebtToEquity);
        assert!(!de.floored && !de.imputed);
        assert_eq!(de.raw, Some(0.0));
        assert_eq!(de.band_score, Some(100.0));
    }

    #[test]
    fn debt_over_non_positive_equity_scores_the_floor_never_neutral() {
        let mut fin = plain_with_closes();
        for row in &mut fin.quarterly_balance_sheet {
            row.total_equity = Some(-100.0);
        }
        let c = composite_of(&fin, Some("Technology"), None);
        let de = factor(&c, Factor::DebtToEquity);
        assert!(de.floored && !de.imputed);
        assert_eq!(de.raw, None);
        assert_eq!(de.band_score, Some(0.0));
    }

    #[test]
    fn an_unmapped_sector_reads_the_market_wide_row() {
        assert_eq!(sector_anchors(Some(" technology ")).label, "Technology");
        assert_eq!(sector_anchors(Some("Widgets")).label, "market-wide");
        assert_eq!(sector_anchors(None).label, "market-wide");
        for row in &SECTOR_ANCHORS {
            assert_eq!(sector_anchors(Some(row.label)), row);
        }
        let c = composite_of(&plain_with_closes(), Some("Widgets"), None);
        assert_eq!(c.sector_anchor, "market-wide");
        let ey = factor(&c, Factor::EarningsYield);
        assert_eq!(ey.band.basis, BandBasis::UnmappedSector);
        assert_eq!(ey.band.anchor, MARKET_ANCHORS.earnings_yield);
        assert_eq!(factor(&c, Factor::Momentum).band.basis, BandBasis::MarketWide);
        let mapped = composite_of(&plain_with_closes(), Some("Technology"), None);
        assert_eq!(factor(&mapped, Factor::EarningsYield).band.basis, BandBasis::Sector);
    }

    #[test]
    fn valuation_own_history_stops_at_a_share_count_jump() {
        let fin = plain_with_closes();
        let full = composite_of(&fin, Some("Technology"), None);
        assert_eq!(factor(&full, Factor::EarningsYield).own_history.unwrap().points, 12);
        // A 2:1 split behind the fifth quarter: the statements' older counts sit
        // on the pre-split basis the split-adjusted closes no longer share.
        let mut split = fin.clone();
        for row in split.quarterly_income.iter_mut().skip(5) {
            row.diluted_shares = Some(50.0);
        }
        let c = composite_of(&split, Some("Technology"), None);
        assert_eq!(factor(&c, Factor::EarningsYield).own_history, None, "four points survive");
        // The fundamental factors read no share count and keep their history.
        assert_eq!(factor(&c, Factor::GrossProfitability).own_history.unwrap().points, 12);
        assert_eq!(share_continuous_prior(&ttm_windows(&fin)), 12);
        // Without the quote's share count there is no basis to rescale onto.
        let mut no_quote = fin.clone();
        no_quote.shares_outstanding = None;
        let c = composite_of(&no_quote, Some("Technology"), None);
        assert_eq!(factor(&c, Factor::EarningsYield).own_history, None);
    }

    #[test]
    fn a_split_since_the_newest_quarter_rescales_onto_the_quote_basis() {
        // A 2:1 split after the newest quarter: the quote reads 20 on 200
        // shares, every dated close is split-adjusted to 20, and the filed
        // diluted counts still read 100. Rescaled by 200 / 100, each past market
        // capitalization is 4,000 — today's — so the history ranks today
        // mid-pack instead of reading it as twice as expensive.
        let mut fin = plain_name();
        fin.current_price = Some(20.0);
        fin.shares_outstanding = Some(200.0);
        fin.daily_closes = weekday_closes("2022-01-03", "2026-09-30", |_| 20.0);
        let c = composite_of(&fin, Some("Technology"), None);
        let ey = factor(&c, Factor::EarningsYield);
        assert!(approx(ey.raw.unwrap(), 60.0 / 4000.0));
        assert_eq!(ey.own_history, Some(OwnHistory { points: 12, score: 50.0 }));
        // The basis also carries a quote on basic shares against a diluted count.
        let windows = ttm_windows(&fin);
        assert!(approx(quote_share_basis(&fin, &windows).unwrap(), 2.0));
    }

    #[test]
    fn a_stale_close_never_stands_in_for_a_window_it_is_far_from() {
        // One close years before every anchor, then the last quarter's sessions:
        // no past window has a close within ten days of its anchor, so the
        // valuation history is empty rather than twelve copies of one price.
        let mut fin = plain_name();
        let mut closes = vec![DatedValue { date: "2021-01-04".into(), value: 40.0 }];
        closes.extend(weekday_closes("2026-07-01", "2026-09-30", |_| 40.0));
        fin.daily_closes = closes;
        let c = composite_of(&fin, Some("Technology"), None);
        assert_eq!(factor(&c, Factor::EarningsYield).own_history, None);
        assert_eq!(factor(&c, Factor::Momentum).raw, None, "no close near a year back");
        let near = weekday_closes("2026-01-05", "2026-01-09", |_| 1.0);
        assert!(close_near(&near, "2026-01-12").is_some(), "three days back");
        assert!(close_near(&near, "2026-01-20").is_none(), "eleven days back");
        assert!(close_near(&near, "2026-01-02").is_none(), "nothing on or before");
    }

    #[test]
    fn a_session_served_twice_counts_once() {
        let alternating: Vec<DatedValue> = weekday_closes("2025-01-01", "2026-12-31", |_| 0.0)
            .into_iter()
            .enumerate()
            .map(|(i, c)| DatedValue { value: if i % 2 == 0 { 100.0 } else { 101.0 }, ..c })
            .collect();
        let doubled: Vec<DatedValue> =
            alternating.iter().flat_map(|c| [c.clone(), c.clone()]).collect();
        assert_eq!(dedupe_closes(&doubled), alternating);
        let mut once = plain_name();
        once.daily_closes = alternating.clone();
        let mut twice = plain_name();
        twice.daily_closes = doubled;
        let a = composite_of(&once, Some("Technology"), None);
        let b = composite_of(&twice, Some("Technology"), None);
        for f in [Factor::Volatility, Factor::Trend, Factor::Momentum] {
            assert_eq!(factor(&a, f), factor(&b, f), "{f:?}");
        }
        // Sixty-one sessions served twice are sixty returns — under the floor.
        let short: Vec<DatedValue> =
            alternating[..61].iter().flat_map(|c| [c.clone(), c.clone()]).collect();
        let mut thin = plain_name();
        thin.daily_closes = short;
        assert_eq!(factor(&composite_of(&thin, None, None), Factor::Volatility).raw, None);
        // The last row served for a duplicated date wins.
        let dup = vec![
            DatedValue { date: "2026-01-05".into(), value: 1.0 },
            DatedValue { date: "2026-01-05".into(), value: 2.0 },
        ];
        assert_eq!(dedupe_closes(&dup), vec![DatedValue { date: "2026-01-05".into(), value: 2.0 }]);
    }

    #[test]
    fn the_sector_lookup_names_its_basis_explicitly() {
        let (row, basis) = sector_row(Some("Energy"));
        assert_eq!((row.label, basis), ("Energy", BandBasis::Sector));
        for missing in [None, Some("Widgets"), Some("")] {
            let (row, basis) = sector_row(missing);
            assert_eq!((row.label, basis), ("market-wide", BandBasis::UnmappedSector), "{missing:?}");
        }
    }

    #[test]
    fn the_fcf_yield_anchor_is_the_earnings_yield_anchor_at_the_conversion_anchor() {
        for row in SECTOR_ANCHORS.iter().chain([&MARKET_ANCHORS]) {
            assert!(approx(fcf_yield_anchor(row), row.earnings_yield * 0.90), "{}", row.label);
        }
        let c = composite_of(&plain_with_closes(), Some("Energy"), None);
        assert!(approx(factor(&c, Factor::FcfYield).band.anchor, 0.0610 * 0.90));
    }

    #[test]
    fn every_anchor_row_is_labelled_once_and_carries_positive_levels() {
        let mut labels: Vec<&str> = SECTOR_ANCHORS.iter().map(|r| r.label).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), 11);
        for row in SECTOR_ANCHORS.iter().chain([&MARKET_ANCHORS]) {
            for v in [
                row.earnings_yield,
                row.ebit_to_ev,
                row.book_to_price,
                row.gross_profitability,
                row.operating_margin,
                row.gross_margin,
                row.debt_to_equity,
                row.volatility,
            ] {
                assert!(v.is_finite() && v > 0.0, "{}: {v}", row.label);
            }
        }
    }

    #[test]
    fn momentum_skips_the_last_month_and_needs_a_year_of_closes() {
        use chrono::NaiveDate;
        let jump = NaiveDate::from_ymd_opt(2025, 6, 1).unwrap();
        let last = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        let closes = weekday_closes("2025-01-01", "2026-03-31", |d| {
            if d >= last {
                150.0
            } else if d >= jump {
                120.0
            } else {
                100.0
            }
        });
        // A year back reads 100, a month back 120; the final month's 150 is skipped.
        assert!(approx(momentum_as_of(&closes, "2026-03-31").unwrap(), 0.2));
        assert_eq!(momentum_as_of(&closes[200..], "2026-03-31"), None, "under a year of closes");
    }

    #[test]
    fn trend_and_volatility_read_the_trailing_sessions() {
        let mut flat: Vec<DatedValue> = weekday_closes("2025-01-01", "2026-12-31", |_| 100.0);
        let n = flat.len();
        let as_of = flat[n - 1].date.clone();
        flat[n - 1].value = 150.0;
        let mean = (199.0 * 100.0 + 150.0) / 200.0;
        assert!(approx(trend_as_of(&flat, &as_of).unwrap(), 150.0 / mean - 1.0));
        assert_eq!(trend_as_of(&flat[n - 199..], &as_of), None, "199 sessions");

        let alternating: Vec<DatedValue> = weekday_closes("2025-01-01", "2026-12-31", |_| 0.0)
            .into_iter()
            .enumerate()
            .map(|(i, c)| DatedValue { value: if i % 2 == 0 { 100.0 } else { 101.0 }, ..c })
            .collect();
        let last = alternating.last().unwrap().date.clone();
        let vol = volatility_as_of(&alternating, &last).unwrap();
        let expected = (1.01f64).ln() * (252.0f64).sqrt();
        assert!((vol - expected).abs() < 1e-3, "{vol} vs {expected}");
        assert_eq!(volatility_as_of(&alternating[..100], &alternating[99].date), None);
    }

    #[test]
    fn the_thin_flag_trips_when_more_than_half_the_resolved_factors_lack_history() {
        let s = |imputed: bool, own: bool| FactorScore {
            factor: Factor::Beta,
            raw: None,
            floored: false,
            band: Band { anchor: 1.0, half_width: 0.6, basis: BandBasis::MarketWide },
            winsorized: false,
            band_score: None,
            own_history: own.then_some(OwnHistory { points: 8, score: 50.0 }),
            score: 50.0,
            imputed,
        };
        // Two of four resolved lack history: exactly half, not thin.
        assert!(!thin_own_history(&[s(false, true), s(false, true), s(false, false), s(false, false)]));
        assert!(thin_own_history(&[s(false, true), s(false, false), s(false, false), s(false, false)]));
        // Imputed factors don't count either way.
        assert!(!thin_own_history(&[s(false, true), s(true, false), s(true, false)]));
        assert!(thin_own_history(&[s(true, false)]), "nothing resolved");

        // End to end: a plain name with sixteen quarters is not thin; one with
        // eight quarters (five windows, four prior points) is.
        assert!(!composite_of(&plain_with_closes(), Some("Technology"), None).thin_own_history);
        let mut short = plain_with_closes();
        short.quarterly_income.truncate(8);
        assert!(composite_of(&short, Some("Technology"), None).thin_own_history);
    }
}
