//! The market-data source contract: the Step-3 baseline scan
//! (`docs/report-workflow.md §Step 3`) as a trait the orchestrator drives, the
//! `fmp` / `fred` / `bls` adapters that implement it, and the composite that runs
//! them and merges their groups into one `BaselineMarketData`.

use serde::{Deserialize, Serialize};

use crate::cadence::ReportCadence;
use crate::progress::RunContext;

/// How a [`Quote`]'s [`Change`] value should be read: the change's own unit, carried
/// alongside the figure so a reader never infers it from the series id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// A percent change off the prior observation — the default for level / price / index
    /// series (and FMP's wire-reported quote change).
    #[default]
    Percent,
    /// A per-period change compounded to an annual rate (the BEA/headline convention; GDP).
    Annualized,
    /// The point change `latest − prior` — for series whose *level is already a rate* (a
    /// yield / spread / policy rate / breakeven, where the figure is the percentage-point
    /// move) or a *zero-centered index* (where a percent-of-prior is unstable across zero).
    /// Combine with the quote's `unit` to read it: percentage points for a `percent` unit,
    /// the level's own units otherwise.
    PointDelta,
}

/// A [`Quote`]'s change off its prior reading with the [`ChangeKind`] that says how to
/// read it, so the figure and its unit never desync.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Change {
    pub value: f64,
    pub kind: ChangeKind,
}

impl Change {
    pub fn percent(value: f64) -> Self {
        Self {
            value,
            kind: ChangeKind::Percent,
        }
    }
    pub fn annualized(value: f64) -> Self {
        Self {
            value,
            kind: ChangeKind::Annualized,
        }
    }
    pub fn point_delta(value: f64) -> Self {
        Self {
            value,
            kind: ChangeKind::PointDelta,
        }
    }
}

/// One quoted instrument in the baseline scan (an index or a market internal). `unit`
/// annotates `price` — the unit the level is quoted in ("index points", "percent", "USD
/// per barrel", …), supplied per series from each adapter's own table rather than the
/// wire, so the model never misreads an unlabeled level. The change's unit lives on
/// `change.kind` (combined with `unit` for a point delta), not here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    #[serde(default)]
    pub change: Change,
    pub unit: String,
}

/// One sector's period performance, as a percentage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorPerformance {
    pub sector: String,
    pub change_pct: f64,
}

/// One entry in the Step-3 economic-release calendar (`docs/report-workflow.md §Step 3`).
/// A `release` entry comes from FRED's release-dates schedule and carries the name and
/// date only — the report's figures reach the model through the `macro_levels` /
/// `labor_levels` quotes — plus, on the paid FMP tier, `surprises` per tracked component
/// print (`econ_calendar`, `docs/data-sources.md §Report enrichment`), empty for an
/// unmapped release, a null estimate or an FMP outage. A `fed` entry is an FMP-only
/// Fed / FOMC event whose date is the information; its `surprises` stay empty. `status`
/// is `"released"` for a date in the recent window or `"upcoming"` for a future one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomicRelease {
    pub release: String,
    pub date: String,
    pub status: String,
    #[serde(default)]
    pub class: CalendarEventClass,
    #[serde(default)]
    pub surprises: Vec<EconomicSurprise>,
}

/// Which kind of calendar event an [`EconomicRelease`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CalendarEventClass {
    /// A scheduled data release from FRED's release-dates schedule.
    #[default]
    Release,
    /// A Fed / FOMC event from FMP's paid economic calendar.
    Fed,
}

/// Where a print's actual landed against its consensus estimate: `in-line` when the
/// absolute gap is within the event's epsilon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurpriseRelation {
    Above,
    InLine,
    Below,
}

/// The polarity-mapped read of an out-of-band print: `beat` on the event's favorable
/// side, `miss` on the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BeatMiss {
    Beat,
    Miss,
}

/// One tracked component print of a release: the consensus estimate and, once released,
/// the actual with the deterministic surprise reads. Every optional value serializes as
/// an explicit `null` meaning *not derivable*, never zero: an upcoming print carries the
/// estimate alone, `gap_pct` is null on a zero estimate, and `beat_miss` is null for an
/// in-line print or an event with no favorable polarity. `estimate`, `actual` and
/// `abs_gap` are in `unit`; `abs_gap` is the magnitude `|actual − estimate|` (the
/// `relation` carries the direction) and `gap_pct` the signed
/// `(actual − estimate) / |estimate|` in percent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomicSurprise {
    pub event: String,
    pub unit: String,
    pub estimate: f64,
    pub actual: Option<f64>,
    pub relation: Option<SurpriseRelation>,
    pub abs_gap: Option<f64>,
    pub gap_pct: Option<f64>,
    pub beat_miss: Option<BeatMiss>,
}

/// One index's multi-horizon performance from FMP's end-of-day history. Every `*_pct`
/// is a percent; `pct_from_52w_high` is ≤ 0 (distance below the high). `low_52w` /
/// `high_52w` are price levels in the index's own units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexPerformance {
    pub symbol: String,
    pub name: String,
    pub weekly_pct: f64,
    pub mtd_pct: f64,
    pub ytd_pct: f64,
    pub low_52w: f64,
    pub high_52w: f64,
    pub pct_from_52w_high: f64,
}

/// Which of FMP's market-mover lists a [`StockMover`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MoverCategory {
    Gainer,
    Loser,
    MostActive,
}

/// One row from FMP's market-mover lists, tagged with the list it came from. FMP's rows
/// carry no sector and no volume. The application filters the raw lists (penny-stock
/// price floor, major-exchange allowlist, top-N per category) before they reach the
/// packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StockMover {
    pub category: MoverCategory,
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub change_pct: f64,
    pub exchange: String,
}

/// One company's earnings event in the Step-3 window, filtered to large-cap names by
/// revenue estimate. Every figure is optional: FMP omits actuals for dates that haven't
/// reported and can omit estimates for thinly-covered names. Revenue is the quarter's
/// figure in USD; EPS is per share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EarningsEvent {
    pub symbol: String,
    pub date: String,
    pub eps_estimated: Option<f64>,
    pub eps_actual: Option<f64>,
    pub revenue_estimated: Option<f64>,
    pub revenue_actual: Option<f64>,
}

/// One sector's aggregate P/E from FMP's exchange-specific `sector-pe-snapshot`, one row
/// per (sector, exchange): `pe` is the aggregate for that one exchange's companies, not a
/// whole-market multiple. `pe` is `None` when no meaningful aggregate is available — the
/// summed earnings are non-positive (FMP reports `0.0` there) or the multiple falls
/// outside `(0.0, SECTOR_PE_MAX]` in `sector_pe_from_value`; the row still rides so the
/// model sees the sector was scanned. On the paid FMP tier the row also carries the two
/// trailing-year reads (`group_history`, `docs/data-sources.md §Report enrichment`),
/// each `None` when not derivable this run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorPe {
    pub sector: String,
    pub exchange: String,
    pub pe: Option<f64>,
    #[serde(default)]
    pub pe_history: Option<PeHistory>,
    #[serde(default)]
    pub trailing_return: Option<TrailingReturn>,
}

/// A group's latest P/E against its own trailing-year history on one exchange: the
/// `percentile` (0–100, the share of the window's valid prints at or below the newest
/// one) with the window's `min` / `median` / `max`, plus the P/E at the window's two ends —
/// `start` on `from` and `latest` on `to`, the dates of the oldest and newest valid prints.
/// The percentile and band place the multiple within its range and carry no direction; a
/// change in the multiple over the window reads from `start` against `latest`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeHistory {
    pub percentile: f64,
    pub min: f64,
    pub median: f64,
    pub max: f64,
    pub start: f64,
    pub latest: f64,
    pub from: String,
    pub to: String,
}

/// A group's cumulative return over its trailing-year window on one exchange, compounded
/// from the daily average move: `return_pct` in percent. `from` and `to` are the dates of
/// the window's oldest and newest valid days.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrailingReturn {
    pub return_pct: f64,
    pub from: String,
    pub to: String,
}

/// One industry's rotation read, joining FMP's exchange-specific
/// `industry-performance-snapshot` with `industry-pe-snapshot` by industry name within
/// one exchange, capped per exchange to the strongest and weakest movers so the packet
/// stays small. `change_pct` is the average percent move; `pe` is `None` when no
/// meaningful P/E is available — the PE snapshot didn't carry that industry, its
/// aggregate earnings are non-positive (FMP reports 0.0 there), or its call failed. On
/// the paid FMP tier the row also carries the two trailing-year reads, as [`SectorPe`]
/// does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndustrySnapshot {
    pub industry: String,
    pub exchange: String,
    pub change_pct: f64,
    pub pe: Option<f64>,
    #[serde(default)]
    pub pe_history: Option<PeHistory>,
    #[serde(default)]
    pub trailing_return: Option<TrailingReturn>,
}

/// The US equity-risk premium from FMP's `market-risk-premium` (Damodaran's per-country
/// dataset): `total_equity_risk_premium` is the expected excess return over the
/// risk-free rate, `country_risk_premium` its country component (≈ 0 for the US). A
/// near-static annual constant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketRiskPremium {
    pub country: String,
    pub country_risk_premium: f64,
    pub total_equity_risk_premium: f64,
}

/// One tracked futures contract's Commitments-of-Traders positioning, normalized across
/// the CFTC report formats into a single speculator-net view (`docs/data-sources.md
/// §CFTC`). The speculator line is leveraged money (financial futures) or managed money
/// (commodities); `real_money_*` is the asset-manager line, financial futures only. Net
/// is `long − short` contracts. The `*_weekly_change` fields are the COT report's own
/// Tuesday-over-Tuesday move, distinct from the report-over-report baseline delta
/// engine, which this group is exempt from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CotPositioning {
    /// Display name, e.g. `"E-Mini S&P 500"`.
    pub contract: String,
    /// CFTC contract market code, e.g. `"13874A"` — the stable pin / join key.
    pub contract_code: String,
    /// Asset-class bucket: `"equity-index"` | `"rates"` | `"fx"` | `"commodity"`.
    pub asset_class: String,
    /// The Tuesday the snapshot reports (`YYYY-MM-DD`) — the data's as-of, so the model
    /// reads positioning against the right week rather than today.
    pub report_date: String,
    /// Total open interest (all reportable + non-reportable), the base the nets size against.
    pub open_interest: f64,
    /// Speculator net position (`long − short`): leveraged money (financial) / managed
    /// money (commodity). Positive = net long, negative = net short.
    pub spec_net: f64,
    /// The speculator net's week-over-week change in contracts, when the report carries it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_net_weekly_change: Option<f64>,
    /// Speculator long as a percent of open interest — a crowding gauge — when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_pct_oi_long: Option<f64>,
    /// Asset-manager ("real money") net — financial futures only; `None` for commodities
    /// (the disaggregated report has no asset-manager cohort).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_money_net: Option<f64>,
    /// The asset-manager net's week-over-week change, when present (financial futures only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_money_net_weekly_change: Option<f64>,
}

/// The primary-market pace (`docs/data-sources.md §Report enrichment`): new listings from
/// FMP's paid `ipos-calendar` and the takeovers registered with the SEC from its paid
/// `mergers-acquisitions-latest`, counted over three windows on the ET session date T —
/// `recent` [T−N, T) and `prior` [T−2N, T−N), N the report cadence clamped to 7–31 days,
/// and `upcoming` [T, T+14) — each serialized as its first and last day. Each side is
/// `None` when it was not gathered this run; the group is absent when neither was. The
/// recent-vs-prior pairs are the group's own trend, so like `cot_positioning` it stays out
/// of the level-delta engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrimaryMarketActivity {
    pub recent: ActivityWindow,
    pub prior: ActivityWindow,
    pub upcoming: ActivityWindow,
    pub ipos: Option<IpoActivity>,
    pub deals: Option<DealActivity>,
}

/// One activity window's first and last day, both included (`YYYY-MM-DD`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityWindow {
    pub from: String,
    pub to: String,
}

/// The IPO side of [`PrimaryMarketActivity`]: the offerings priced in the recent and prior
/// windows and those scheduled in the upcoming one, a row counting only when it carries a
/// share count (the calendar also lists fund launches, notes, warrants, rights and
/// when-issued lines, none of which do), plus the recent window's largest offerings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpoActivity {
    pub recent_count: u32,
    pub prior_count: u32,
    pub upcoming_count: u32,
    pub notable: Vec<NotableIpo>,
}

/// One of the recent window's largest IPOs; `deal_size_usd` is the offering's dollar size
/// as the feed states it (`dealSize`), and each field is `None` where the feed carries none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotableIpo {
    pub symbol: Option<String>,
    pub company: Option<String>,
    pub date: String,
    pub exchange: Option<String>,
    pub deal_size_usd: Option<f64>,
}

/// The deal side of [`PrimaryMarketActivity`]: the merger and acquisition deals whose SEC
/// registration statement was filed in the recent and prior windows, one per filing, plus
/// the recent window's latest. `prior_count` is `None` when the page walk covered the recent
/// window but not the prior one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DealActivity {
    pub recent_count: u32,
    pub prior_count: Option<u32>,
    pub notable: Vec<NotableDeal>,
}

/// One of the recent window's latest deals: each party's name as served (its symbol where
/// the feed served no name) and the target's ticker where it is listed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotableDeal {
    pub acquirer: Option<String>,
    pub target: Option<String>,
    pub target_symbol: Option<String>,
    pub date: String,
}

/// Which Step-3 baseline group a [`DataGap`] belongs to; the kebab label is the one the
/// serialized groups carry and the coverage gate (`pipeline::enforce_coverage`) matches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroupKind {
    Indices,
    Internals,
    Sectors,
    MacroLevels,
    LaborLevels,
    Calendar,
    IndexPerformance,
    Movers,
    Earnings,
    SectorPe,
    Industries,
    MarketRiskPremium,
    CotPositioning,
    PrimaryMarket,
}

impl GroupKind {
    /// The kebab label, matching the serde rename, so a tracker row's group reads the
    /// same name the serialized baseline carries.
    pub fn as_str(self) -> &'static str {
        match self {
            GroupKind::Indices => "indices",
            GroupKind::Internals => "internals",
            GroupKind::Sectors => "sectors",
            GroupKind::MacroLevels => "macro-levels",
            GroupKind::LaborLevels => "labor-levels",
            GroupKind::Calendar => "calendar",
            GroupKind::IndexPerformance => "index-performance",
            GroupKind::Movers => "movers",
            GroupKind::Earnings => "earnings",
            GroupKind::SectorPe => "sector-pe",
            GroupKind::Industries => "industries",
            GroupKind::MarketRiskPremium => "market-risk-premium",
            GroupKind::CotPositioning => "cot-positioning",
            GroupKind::PrimaryMarket => "primary-market",
        }
    }
}

/// Why a requested series / release didn't land in the baseline this run. The coverage
/// gate counts only the this-run reasons against a group's coverage: `OutOfScope` is
/// permanent and excluded, so a series a deployment never had doesn't drag the ratio
/// down on every run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GapReason {
    /// Permanently absent for this deployment: the provider explicitly signalled the item
    /// isn't available (FMP 402 premium / 404, FRED "does not exist"). Reserved for
    /// explicit provider signals only — a 2xx that simply carried no value (an empty
    /// array, an all-gap window, an empty `data` block) is `Unavailable`, not this.
    OutOfScope,
    /// A 429 / 5xx that survived the retry layer, a transport error, or a successful
    /// response that carried no usable value for an expected series.
    Unavailable,
    /// A bad / expired key (401/403, FRED `api_key`) or a quota / plan limit (FMP
    /// `Error Message`, BLS `REQUEST_NOT_PROCESSED`).
    Rejected,
    /// An unparseable body, a wrong shape, a non-numeric observation, or a truncated /
    /// omitted series.
    Malformed,
}

impl GapReason {
    pub fn counts_against_coverage(self) -> bool {
        !matches!(self, GapReason::OutOfScope)
    }

    /// The kebab label, matching the serde rename: the status a tracker row shows for a
    /// series that degraded to a gap.
    pub fn as_str(self) -> &'static str {
        match self {
            GapReason::OutOfScope => "out-of-scope",
            GapReason::Unavailable => "unavailable",
            GapReason::Rejected => "rejected",
            GapReason::Malformed => "malformed",
        }
    }
}

/// One entry in the Step-3 missing-data manifest: a series / release a provider failed
/// to resolve this run. Merged across providers by the composite, read by the coverage
/// gate, and serialized into the agent's prompt so the model reasons over what's
/// known-absent rather than silently inferring it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataGap {
    pub group: GroupKind,
    pub series_id: String,
    pub series_name: String,
    pub reason: GapReason,
}

impl DataGap {
    pub fn new(
        group: GroupKind,
        series_id: impl Into<String>,
        series_name: impl Into<String>,
        reason: GapReason,
    ) -> Self {
        Self {
            group,
            series_id: series_id.into(),
            series_name: series_name.into(),
            reason,
        }
    }
}

/// Schema version stamped on each persisted baseline snapshot
/// (`storage::baseline_snapshots`), recorded for migration tooling; the decode path
/// relies on the fields' serde defaults, not on inspecting it. Bump it when a change
/// can't be absorbed by a field default alone.
pub const BASELINE_SCHEMA_VERSION: u32 = 1;

/// The baseline market-data scan handed to the main agent. Empty vectors are valid: a
/// provider that fails to resolve a group degrades it to empty and records the reason in
/// [`gaps`](Self::gaps) rather than failing the scan, and the mandatory-coverage floor
/// (`pipeline::enforce_coverage`), not the adapters, is the single place a too-thin
/// baseline fails the run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BaselineMarketData {
    #[serde(default)]
    pub indices: Vec<Quote>,
    #[serde(default)]
    pub internals: Vec<Quote>,
    #[serde(default)]
    pub sectors: Vec<SectorPerformance>,
    /// Point-in-time FRED macro series, kept distinct from the market `internals`;
    /// `change` is the move from the prior observation at the series' own frequency
    /// (day, month or quarter).
    #[serde(default)]
    pub macro_levels: Vec<Quote>,
    /// Point-in-time BLS labor series; `change` is the month-over-month move.
    #[serde(default)]
    pub labor_levels: Vec<Quote>,
    /// Empty is valid (a quiet window, or the calendar soft-degraded); it carries no
    /// completeness floor, unlike the series groups.
    #[serde(default)]
    pub calendar: Vec<EconomicRelease>,
    /// No completeness floor: soft-degrades if the history fetch fails, since the daily
    /// `indices` quotes already satisfy Step 3.
    #[serde(default)]
    pub index_performance: Vec<IndexPerformance>,
    /// Additive and non-floor: empty is valid and soft-degrades rather than failing the run.
    #[serde(default)]
    pub movers: Vec<StockMover>,
    /// Additive and non-floor: empty is valid and soft-degrades rather than failing the run.
    #[serde(default)]
    pub earnings: Vec<EarningsEvent>,
    /// Additive and non-floor: empty is valid and soft-degrades rather than failing the run.
    #[serde(default)]
    pub sector_pe: Vec<SectorPe>,
    /// Additive and non-floor: empty is valid and soft-degrades rather than failing the run.
    #[serde(default)]
    pub industries: Vec<IndustrySnapshot>,
    /// Zero or one row. Additive and non-floor: empty is valid and soft-degrades.
    #[serde(default)]
    pub market_risk_premium: Vec<MarketRiskPremium>,
    /// Additive and non-floor: empty is valid and soft-degrades rather than failing the run.
    #[serde(default)]
    pub cot_positioning: Vec<CotPositioning>,
    /// `None` when neither feed was gathered. Additive and non-floor: soft-degrades rather
    /// than failing the run.
    #[serde(default)]
    pub primary_market: Option<PrimaryMarketActivity>,
    /// Empty when a scan resolved everything.
    #[serde(default)]
    pub gaps: Vec<DataGap>,
}

/// Take at most `max` characters (never bytes — provider text carries multi-byte
/// punctuation), reporting whether anything was cut. The one char-boundary-safe core
/// under every capped surface string; each caller keeps its own cut marker, several of
/// which are message-pinned by tests.
pub(crate) fn cap_chars(s: &str, max: usize) -> (String, bool) {
    let head: String = s.chars().take(max).collect();
    let cut = head.len() < s.len();
    (head, cut)
}

/// Emit the one `RequestFinished` row pairing the `RequestStarted` the caller sent
/// before the HTTP call. `gaps_before` is `gaps.len()` captured before the request and
/// `produced` whether it yielded a value: produced wins (`ok`); otherwise a gap pushed
/// by this request carries its reason; otherwise `empty` (a 2xx that yielded nothing and
/// recorded no gap).
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_series_row(
    ctx: &RunContext,
    provider: &str,
    group: GroupKind,
    series_id: &str,
    name: &str,
    gaps: &[DataGap],
    gaps_before: usize,
    produced: bool,
    detail: Option<String>,
) {
    let status = if produced {
        "ok"
    } else if gaps.len() > gaps_before {
        gaps[gaps.len() - 1].reason.as_str()
    } else {
        "empty"
    };
    // The detail rides only a non-produced row: a produced request has nothing to explain.
    let detail = if produced { None } else { detail };
    ctx.request_finished(provider, group.as_str(), series_id, name, status, detail);
}

/// The data-source stage. Sync, like `MainAgent`: the real adapter's blocking HTTP call
/// is offloaded via `spawn_blocking` at the Tauri command seam.
pub trait MarketDataSource {
    /// `cadence` — the elapsed interval since the previous report — sizes the
    /// cadence-dependent lookback windows (the earnings / economic-release calendars) so
    /// a monthly run's calendars cover the whole gap; only the FMP and FRED adapters read
    /// it.
    fn baseline_scan(&self, cadence: ReportCadence) -> anyhow::Result<BaselineMarketData>;
}

/// Deterministic offline stand-in: a small fixed baseline so the pipeline and its tests
/// run without live keys.
#[derive(Debug, Default)]
pub struct StubMarketDataSource;

impl MarketDataSource for StubMarketDataSource {
    fn baseline_scan(&self, _cadence: ReportCadence) -> anyhow::Result<BaselineMarketData> {
        Ok(BaselineMarketData {
            indices: vec![
                Quote {
                    symbol: "^GSPC".into(),
                    name: "S&P 500".into(),
                    price: 5_500.0,
                    change: Change::percent(0.4),
                    unit: "index points".into(),
                },
                Quote {
                    symbol: "^IXIC".into(),
                    name: "Nasdaq Composite".into(),
                    price: 17_800.0,
                    change: Change::percent(0.6),
                    unit: "index points".into(),
                },
            ],
            internals: vec![Quote {
                symbol: "^VIX".into(),
                name: "CBOE Volatility Index".into(),
                price: 14.2,
                change: Change::percent(-1.1),
                unit: "index points".into(),
            }],
            sectors: vec![
                SectorPerformance {
                    sector: "Technology".into(),
                    change_pct: 1.2,
                },
                SectorPerformance {
                    sector: "Energy".into(),
                    change_pct: -0.8,
                },
            ],
            macro_levels: vec![Quote {
                symbol: "DFEDTARU".into(),
                name: "Fed Funds Target Range — Upper Limit".into(),
                price: 4.5,
                change: Change::percent(0.0),
                unit: "percent".into(),
            }],
            labor_levels: vec![Quote {
                symbol: "LNS14000000".into(),
                name: "Unemployment Rate".into(),
                price: 4.1,
                change: Change::percent(0.0),
                unit: "percent".into(),
            }],
            calendar: vec![EconomicRelease {
                release: "Employment Situation".into(),
                date: "2026-06-05".into(),
                status: "released".into(),
                class: CalendarEventClass::Release,
                surprises: Vec::new(),
            }],
            index_performance: vec![IndexPerformance {
                symbol: "^GSPC".into(),
                name: "S&P 500".into(),
                weekly_pct: 1.1,
                mtd_pct: 2.3,
                ytd_pct: 8.4,
                low_52w: 4_200.0,
                high_52w: 5_600.0,
                pct_from_52w_high: -1.8,
            }],
            movers: vec![
                StockMover {
                    category: MoverCategory::Gainer,
                    symbol: "NVDA".into(),
                    name: "NVIDIA Corporation".into(),
                    price: 142.0,
                    change_pct: 4.2,
                    exchange: "NASDAQ".into(),
                },
                StockMover {
                    category: MoverCategory::Loser,
                    symbol: "INTC".into(),
                    name: "Intel Corporation".into(),
                    price: 21.5,
                    change_pct: -3.1,
                    exchange: "NASDAQ".into(),
                },
            ],
            earnings: vec![EarningsEvent {
                symbol: "ADBE".into(),
                date: "2026-06-11".into(),
                eps_estimated: Some(5.83),
                eps_actual: None,
                revenue_estimated: Some(6_453_568_000.0),
                revenue_actual: None,
            }],
            sector_pe: vec![
                SectorPe {
                    sector: "Technology".into(),
                    exchange: "NASDAQ".into(),
                    pe: Some(38.4),
                    pe_history: None,
                    trailing_return: None,
                },
                SectorPe {
                    sector: "Technology".into(),
                    exchange: "NYSE".into(),
                    pe: Some(24.6),
                    pe_history: None,
                    trailing_return: None,
                },
                SectorPe {
                    sector: "Energy".into(),
                    exchange: "NYSE".into(),
                    pe: Some(12.1),
                    pe_history: None,
                    trailing_return: None,
                },
            ],
            industries: vec![
                IndustrySnapshot {
                    industry: "Semiconductors".into(),
                    exchange: "NASDAQ".into(),
                    change_pct: 2.4,
                    pe: Some(41.2),
                    pe_history: None,
                    trailing_return: None,
                },
                IndustrySnapshot {
                    industry: "Oil & Gas Midstream".into(),
                    exchange: "NYSE".into(),
                    change_pct: -1.9,
                    pe: None,
                    pe_history: None,
                    trailing_return: None,
                },
            ],
            market_risk_premium: vec![MarketRiskPremium {
                country: "United States".into(),
                country_risk_premium: 0.0,
                total_equity_risk_premium: 4.46,
            }],
            cot_positioning: vec![
                CotPositioning {
                    contract: "E-Mini S&P 500".into(),
                    contract_code: "13874A".into(),
                    asset_class: "equity-index".into(),
                    report_date: "2026-06-16".into(),
                    open_interest: 2_579_920.0,
                    spec_net: -515_520.0,
                    spec_net_weekly_change: Some(-63_934.0),
                    spec_pct_oi_long: Some(6.1),
                    real_money_net: Some(984_009.0),
                    real_money_net_weekly_change: Some(-1_500.0),
                },
                CotPositioning {
                    contract: "Gold".into(),
                    contract_code: "088691".into(),
                    asset_class: "commodity".into(),
                    report_date: "2026-06-16".into(),
                    open_interest: 500_000.0,
                    spec_net: 200_000.0,
                    spec_net_weekly_change: Some(4_000.0),
                    spec_pct_oi_long: Some(50.0),
                    real_money_net: None,
                    real_money_net_weekly_change: None,
                },
            ],
            primary_market: None,
            gaps: Vec::new(),
        })
    }
}

/// Compose two `MarketDataSource`s into one baseline scan, primary then secondary, every
/// group merged primary-first. The adapters degrade to recorded gaps rather than failing
/// for data reasons, so a child `Err` is reserved for a catastrophic (non-data) fault and
/// propagates. Sources compose by nesting: the run path wraps FMP+FRED, then nests `bls`
/// as the outer secondary.
pub struct CompositeMarketDataSource<P, S> {
    pub primary: P,
    pub secondary: S,
}

impl<P, S> CompositeMarketDataSource<P, S> {
    pub fn new(primary: P, secondary: S) -> Self {
        Self { primary, secondary }
    }
}

impl<P: MarketDataSource, S: MarketDataSource> MarketDataSource
    for CompositeMarketDataSource<P, S>
{
    fn baseline_scan(&self, cadence: ReportCadence) -> anyhow::Result<BaselineMarketData> {
        let mut merged = self.primary.baseline_scan(cadence)?;
        let extra = self.secondary.baseline_scan(cadence)?;
        merged.indices.extend(extra.indices);
        merged.internals.extend(extra.internals);
        merged.sectors.extend(extra.sectors);
        merged.macro_levels.extend(extra.macro_levels);
        merged.labor_levels.extend(extra.labor_levels);
        merged.calendar.extend(extra.calendar);
        merged.index_performance.extend(extra.index_performance);
        merged.movers.extend(extra.movers);
        merged.earnings.extend(extra.earnings);
        merged.sector_pe.extend(extra.sector_pe);
        merged.industries.extend(extra.industries);
        merged.market_risk_premium.extend(extra.market_risk_premium);
        merged.cot_positioning.extend(extra.cot_positioning);
        merged.primary_market = merged.primary_market.or(extra.primary_market);
        merged.gaps.extend(extra.gaps);
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_decodes_a_pre_typed_change_snapshot_with_a_default_change() {
        // serde ignores the unknown `change_pct`; `#[serde(default)]` fills `change`.
        let old: Quote = serde_json::from_str(
            r#"{"symbol":"DGS10","name":"10-Year Treasury Yield","price":4.3,"change_pct":0.1,"unit":"percent"}"#,
        )
        .unwrap();
        assert_eq!(old.price, 4.3);
        assert_eq!(old.change, Change::default());
        assert_eq!(old.change.kind, ChangeKind::Percent);
    }

    #[test]
    fn a_pre_enrichment_calendar_snapshot_decodes_to_a_release_with_no_surprises() {
        let old: BaselineMarketData = serde_json::from_str(
            r#"{"calendar":[{"release":"Consumer Price Index","date":"2026-06-10","status":"upcoming"}]}"#,
        )
        .unwrap();
        assert_eq!(old.calendar.len(), 1);
        assert_eq!(old.calendar[0].class, CalendarEventClass::Release);
        assert!(old.calendar[0].surprises.is_empty());
    }

    #[test]
    fn the_calendar_enrichment_round_trips_with_explicit_nulls() {
        let baseline = BaselineMarketData {
            calendar: vec![
                EconomicRelease {
                    release: "Employment Situation".into(),
                    date: "2026-10-02".into(),
                    status: "released".into(),
                    class: CalendarEventClass::Release,
                    surprises: vec![
                        EconomicSurprise {
                            event: "Non Farm Payrolls".into(),
                            unit: "thousands of jobs".into(),
                            estimate: 90.0,
                            actual: Some(29.0),
                            relation: Some(SurpriseRelation::Below),
                            abs_gap: Some(61.0),
                            gap_pct: Some(-67.77777777777779),
                            beat_miss: Some(BeatMiss::Miss),
                        },
                        EconomicSurprise {
                            event: "Unemployment Rate".into(),
                            unit: "percent".into(),
                            estimate: 4.1,
                            actual: Some(4.1),
                            relation: Some(SurpriseRelation::InLine),
                            abs_gap: Some(0.0),
                            gap_pct: Some(0.0),
                            beat_miss: None,
                        },
                    ],
                },
                EconomicRelease {
                    release: "Gross Domestic Product".into(),
                    date: "2026-10-29".into(),
                    status: "upcoming".into(),
                    class: CalendarEventClass::Release,
                    surprises: vec![EconomicSurprise {
                        event: "GDP Growth Rate QoQ".into(),
                        unit: "% annualized quarter-over-quarter".into(),
                        estimate: 0.9,
                        actual: None,
                        relation: None,
                        abs_gap: None,
                        gap_pct: None,
                        beat_miss: None,
                    }],
                },
                EconomicRelease {
                    release: "Fed Interest Rate Decision".into(),
                    date: "2026-10-28".into(),
                    status: "upcoming".into(),
                    class: CalendarEventClass::Fed,
                    surprises: Vec::new(),
                },
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&baseline).unwrap();
        // Absent reads serialize as explicit nulls, never omitted and never zero.
        assert!(json.contains(r#""actual":null"#), "{json}");
        assert!(json.contains(r#""gap_pct":null"#), "{json}");
        assert!(json.contains(r#""relation":"in-line""#), "{json}");
        assert!(json.contains(r#""class":"fed""#), "{json}");
        let back: BaselineMarketData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, baseline);
    }

    #[test]
    fn a_pre_enrichment_valuation_snapshot_decodes_with_no_history_reads() {
        let old: BaselineMarketData = serde_json::from_str(
            r#"{"sector_pe":[{"sector":"Energy","exchange":"NYSE","pe":12.1}],
                "industries":[{"industry":"Semiconductors","exchange":"NASDAQ","change_pct":2.4,"pe":41.2}]}"#,
        )
        .unwrap();
        assert_eq!(old.sector_pe[0].pe, Some(12.1));
        assert!(old.sector_pe[0].pe_history.is_none());
        assert!(old.sector_pe[0].trailing_return.is_none());
        assert!(old.industries[0].pe_history.is_none());
        assert!(old.industries[0].trailing_return.is_none());
    }

    #[test]
    fn the_history_reads_round_trip_with_explicit_nulls() {
        let baseline = BaselineMarketData {
            sector_pe: vec![SectorPe {
                sector: "Energy".into(),
                exchange: "NYSE".into(),
                pe: Some(17.1),
                pe_history: Some(PeHistory {
                    percentile: 6.5,
                    min: 16.2,
                    median: 21.7,
                    max: 35.8,
                    start: 21.4,
                    latest: 17.1,
                    from: "2025-10-13".into(),
                    to: "2026-10-12".into(),
                }),
                trailing_return: None,
            }],
            industries: vec![IndustrySnapshot {
                industry: "Oil & Gas Midstream".into(),
                exchange: "NYSE".into(),
                change_pct: 0.7,
                pe: None,
                pe_history: None,
                trailing_return: Some(TrailingReturn {
                    return_pct: 20.1,
                    from: "2025-10-13".into(),
                    to: "2026-10-12".into(),
                }),
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&baseline).unwrap();
        // An absent read serializes as an explicit null, never omitted and never zero.
        assert!(json.contains(r#""trailing_return":null"#), "{json}");
        assert!(json.contains(r#""pe_history":null"#), "{json}");
        assert!(json.contains(r#""percentile":6.5"#), "{json}");
        assert!(json.contains(r#""start":21.4,"latest":17.1"#), "{json}");
        assert!(json.contains(r#""return_pct":20.1"#), "{json}");
        let back: BaselineMarketData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, baseline);
    }

    fn sample_primary_market() -> PrimaryMarketActivity {
        let window = |from: &str, to: &str| ActivityWindow {
            from: from.into(),
            to: to.into(),
        };
        PrimaryMarketActivity {
            recent: window("2026-09-12", "2026-10-11"),
            prior: window("2026-08-13", "2026-09-11"),
            upcoming: window("2026-10-12", "2026-10-25"),
            ipos: None,
            deals: Some(DealActivity {
                recent_count: 7,
                prior_count: None,
                notable: vec![NotableDeal {
                    acquirer: Some("Inflection Point Acquisition Corp. VII".into()),
                    target: Some("Intuitive Machines, Inc.".into()),
                    target_symbol: None,
                    date: "2026-10-08".into(),
                }],
            }),
        }
    }

    #[test]
    fn a_snapshot_persisted_before_the_primary_market_group_decodes_without_it() {
        let old: BaselineMarketData =
            serde_json::from_str(r#"{"indices":[],"cot_positioning":[],"gaps":[]}"#).unwrap();
        assert_eq!(old.primary_market, None);
    }

    #[test]
    fn the_primary_market_group_round_trips_with_explicit_nulls() {
        let baseline = BaselineMarketData {
            primary_market: Some(sample_primary_market()),
            ..Default::default()
        };
        let json = serde_json::to_string(&baseline).unwrap();
        // An absent read serializes as an explicit null, never omitted and never zero.
        assert!(json.contains(r#""ipos":null"#), "{json}");
        assert!(json.contains(r#""recent_count":7,"prior_count":null"#), "{json}");
        assert!(json.contains(r#""target_symbol":null"#), "{json}");
        assert!(
            json.contains(r#""recent":{"from":"2026-09-12","to":"2026-10-11"}"#),
            "{json}"
        );
        let back: BaselineMarketData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, baseline);
        let none = serde_json::to_string(&BaselineMarketData::default()).unwrap();
        assert!(none.contains(r#""primary_market":null"#), "{none}");
    }

    /// A stub that reads only the primary-market group, as the FMP adapter does.
    struct PrimaryMarketStub;

    impl MarketDataSource for PrimaryMarketStub {
        fn baseline_scan(&self, _cadence: ReportCadence) -> anyhow::Result<BaselineMarketData> {
            Ok(BaselineMarketData {
                primary_market: Some(sample_primary_market()),
                ..Default::default()
            })
        }
    }

    #[test]
    fn the_composite_keeps_the_primary_market_group_from_the_source_that_read_it() {
        let cadence = ReportCadence::from_elapsed(None);
        let first = CompositeMarketDataSource::new(PrimaryMarketStub, StubMarketDataSource)
            .baseline_scan(cadence)
            .unwrap();
        assert_eq!(first.primary_market, Some(sample_primary_market()));
        let second = CompositeMarketDataSource::new(StubMarketDataSource, PrimaryMarketStub)
            .baseline_scan(cadence)
            .unwrap();
        assert_eq!(second.primary_market, Some(sample_primary_market()));
    }

    /// A stub shaped like the secondary sources (`fred` + `bls`), so the composite merge
    /// runs offline.
    struct FredShapedStub {
        internals: Vec<Quote>,
        macro_levels: Vec<Quote>,
        labor_levels: Vec<Quote>,
        calendar: Vec<EconomicRelease>,
        gaps: Vec<DataGap>,
    }

    impl MarketDataSource for FredShapedStub {
        fn baseline_scan(&self, _cadence: ReportCadence) -> anyhow::Result<BaselineMarketData> {
            Ok(BaselineMarketData {
                internals: self.internals.clone(),
                macro_levels: self.macro_levels.clone(),
                labor_levels: self.labor_levels.clone(),
                calendar: self.calendar.clone(),
                gaps: self.gaps.clone(),
                ..Default::default()
            })
        }
    }

    struct FailingStub;

    impl MarketDataSource for FailingStub {
        fn baseline_scan(&self, _cadence: ReportCadence) -> anyhow::Result<BaselineMarketData> {
            anyhow::bail!("provider down")
        }
    }

    fn quote(symbol: &str) -> Quote {
        Quote {
            symbol: symbol.into(),
            name: symbol.into(),
            price: 1.0,
            change: Change::percent(0.0),
            unit: "index points".into(),
        }
    }

    #[test]
    fn composite_merges_both_sources_into_one_baseline() {
        // Every group merges primary-first.
        let secondary = FredShapedStub {
            internals: vec![quote("DGS10"), quote("DTWEXBGS")],
            macro_levels: vec![quote("T10YIE")],
            labor_levels: vec![quote("CUUR0000SA0")],
            calendar: vec![EconomicRelease {
                release: "Consumer Price Index".into(),
                date: "2026-06-10".into(),
                status: "upcoming".into(),
                class: CalendarEventClass::Release,
                surprises: Vec::new(),
            }],
            gaps: vec![DataGap::new(
                GroupKind::Internals,
                "DCOILWTICO",
                "WTI Crude Oil",
                GapReason::Unavailable,
            )],
        };
        let composite = CompositeMarketDataSource::new(StubMarketDataSource, secondary);
        let data = composite.baseline_scan(ReportCadence::default()).unwrap();

        assert!(!data.indices.is_empty(), "primary indices survive");
        assert!(!data.sectors.is_empty(), "primary sectors survive");
        assert_eq!(data.internals.len(), 3);
        assert_eq!(data.internals[0].symbol, "^VIX");
        assert_eq!(data.internals[1].symbol, "DGS10");
        assert_eq!(data.internals[2].symbol, "DTWEXBGS");
        assert_eq!(data.macro_levels.len(), 2);
        assert_eq!(data.macro_levels[0].symbol, "DFEDTARU");
        assert_eq!(data.macro_levels[1].symbol, "T10YIE");
        assert_eq!(data.labor_levels.len(), 2);
        assert_eq!(data.labor_levels[0].symbol, "LNS14000000");
        assert_eq!(data.labor_levels[1].symbol, "CUUR0000SA0");
        assert_eq!(data.calendar.len(), 2);
        assert_eq!(data.calendar[0].release, "Employment Situation");
        assert_eq!(data.calendar[1].release, "Consumer Price Index");
        assert_eq!(data.gaps.len(), 1);
        assert_eq!(data.gaps[0].series_id, "DCOILWTICO");
        assert_eq!(data.gaps[0].reason, GapReason::Unavailable);
    }

    #[test]
    fn composite_propagates_a_secondary_failure() {
        let composite = CompositeMarketDataSource::new(StubMarketDataSource, FailingStub);
        assert!(composite.baseline_scan(ReportCadence::default()).is_err());
    }

    #[test]
    fn stub_populates_groups_and_round_trips() {
        let data = StubMarketDataSource.baseline_scan(ReportCadence::default()).unwrap();
        assert!(!data.indices.is_empty());
        assert!(!data.internals.is_empty());
        assert!(!data.sectors.is_empty());
        assert!(!data.macro_levels.is_empty());
        assert!(!data.labor_levels.is_empty());
        assert!(!data.calendar.is_empty());
        assert!(!data.index_performance.is_empty());
        assert!(!data.sector_pe.is_empty());
        assert!(!data.industries.is_empty());
        assert!(!data.market_risk_premium.is_empty());
        assert!(!data.cot_positioning.is_empty());

        let mut data = data;
        data.gaps.push(DataGap::new(
            GroupKind::LaborLevels,
            "CES0500000003",
            "Average Hourly Earnings, Total Private",
            GapReason::Rejected,
        ));
        let json = serde_json::to_string(&data).unwrap();
        let back: BaselineMarketData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn baseline_deserializes_an_older_snapshot_missing_groups() {
        let older = r#"{
            "indices": [
                {"symbol":"^GSPC","name":"S&P 500","price":5500.0,"change_pct":0.4,"unit":"index points"}
            ]
        }"#;
        let back: BaselineMarketData = serde_json::from_str(older).unwrap();
        assert_eq!(back.indices.len(), 1);
        assert_eq!(back.indices[0].symbol, "^GSPC");
        assert!(back.internals.is_empty());
        assert!(back.macro_levels.is_empty());
        assert!(back.sector_pe.is_empty());
        assert!(back.market_risk_premium.is_empty());
        assert!(back.gaps.is_empty());
    }
}
