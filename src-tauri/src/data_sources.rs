//! The market-data source contract: a structured baseline-scan boundary.
//!
//! Mirrors the `agent` module's spine — the application layer owns all I/O, the
//! data source is a trait the orchestrator drives, and a deterministic stub
//! stands in for the live provider in offline tests. Three real adapters implement
//! this trait — `fmp` (equity indices, VIX, gold, sectors, and multi-horizon index
//! performance from end-of-day history), `fred` (the Treasury
//! yields, dollar index, and oil / natural-gas internals FMP's free tier omits, plus
//! the `macro_levels` group — Fed-funds target range, inflation breakevens, consumer
//! sentiment, PCE — and the economic-release `calendar`), and `bls` (the `labor_levels`
//! group — CPI, unemployment, payrolls, wages) — and the `CompositeMarketDataSource`
//! below runs them and merges them into one baseline.
//!
//! `BaselineMarketData` is the Step-6 baseline scan
//! (`docs/weekly-report-workflow.md §Step 6`), gathered before agent reasoning.
//! FMP fills indices, sectors, and the VIX + gold internals; FRED appends its
//! commodity / yield series to the same `internals` group and fills the
//! `macro_levels` group (Fed-funds target range, inflation breakevens, consumer
//! sentiment, PCE) and the economic-release `calendar` (the prior-week + upcoming US
//! reports, from FRED's free release-dates schedule — FMP's economic-calendar endpoint
//! is premium-gated); BLS fills the `labor_levels` group (CPI, unemployment, payrolls,
//! wages).

use serde::{Deserialize, Serialize};

use crate::progress::RunContext;

/// One quoted instrument in the baseline scan: a market index or a market
/// internal (VIX, the dollar index, a commodity). `change_pct` is the percent
/// change the provider reports for the quote.
///
/// `unit` annotates **`price`** — the unit the level is quoted in ("index points",
/// "percent", "USD per barrel", "thousands of persons", …), supplied per series from
/// each adapter's own table rather than the wire, so the model reading the serialized
/// baseline can't misread an unlabeled level (a payroll count of thousands as ones, a
/// yield level as a dollar figure). It does **not** describe `change_pct`, which is a
/// percent for every series regardless of the level's unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub change_pct: f64,
    pub unit: String,
}

/// One sector's period performance, as a percentage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorPerformance {
    pub sector: String,
    pub change_pct: f64,
}

/// One entry in the Step-6 economic-release calendar: a scheduled or just-released US
/// economic report (`docs/weekly-report-workflow.md §Step 6` — the "CPI/PCE/jobs
/// calendar" and "major economic reports from the prior week"). Sourced from FRED's
/// free release-dates schedule (FMP's economic-calendar endpoint is premium-gated), so
/// it carries the release **name** and **date** but not the report's figures — those
/// reach the model through the `macro_levels` / `labor_levels` series quotes. `status`
/// is `"released"` for a date in the prior-week window or `"upcoming"` for a scheduled
/// future date. `expected` is the analyst-consensus slot reserved for a future paid
/// source — no free provider supplies US consensus, so it is always `None` on the FRED
/// path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomicRelease {
    pub release: String,
    pub date: String,
    pub status: String,
    pub expected: Option<f64>,
}

/// One index's multi-horizon performance, derived from FMP's end-of-day history
/// (`historical-price-eod`). Complements the daily `Quote` in `indices` with the
/// longer-horizon returns a *weekly* thesis needs: week-over-week, month-to-date, and
/// year-to-date returns, plus where the latest close sits inside its trailing 52-week
/// range. Every `*_pct` is a percent; `pct_from_52w_high` is ≤ 0 (distance below the
/// high). `low_52w` / `high_52w` are price levels in the index's own units.
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

/// One row from FMP's free market-mover lists (biggest gainers / losers / most actives),
/// tagged with the list it came from — a micro-breadth signal for which individual names
/// moved most this run, surfacing rotation the index/sector reads can't. FMP's mover rows
/// carry **no sector** (the model infers it from the ticker) and no volume, so the shape
/// is the ticker, its latest `price` + `change_pct`, and the listing `exchange`. The
/// application filters the raw lists (penny-stock price floor, major-exchange allowlist,
/// top-N per category) before they reach the packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StockMover {
    pub category: MoverCategory,
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub change_pct: f64,
    pub exchange: String,
}

/// One company's earnings event in the Step-6 window — a recent or upcoming report from
/// FMP's free earnings calendar, filtered to large-cap names by revenue estimate. A
/// forward date carries estimates with null actuals; a past date in the window carries
/// both, so the model can read beats / misses. Every figure is optional: FMP omits
/// actuals for dates that haven't reported and can omit estimates for thinly-covered
/// names. Revenue is the quarter's figure in USD; EPS is per share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EarningsEvent {
    pub symbol: String,
    pub date: String,
    pub eps_estimated: Option<f64>,
    pub eps_actual: Option<f64>,
    pub revenue_estimated: Option<f64>,
    pub revenue_actual: Option<f64>,
}

/// One sector's aggregate price-to-earnings ratio from FMP's free sector-PE snapshot
/// (`sector-pe-snapshot`) — a valuation complement to the `sectors` performance group, so
/// the model can read which sectors are rich or cheap, not just which moved. FMP's snapshot
/// is **exchange-specific**, so `exchange` is carried (not dropped): the baseline gathers
/// both NASDAQ-listed (growth / tech-tilted) and NYSE-listed (broader, more value) reads, and
/// `pe` is the aggregate for that one exchange's companies — not a whole-market multiple.
/// One row per (sector, exchange).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorPe {
    pub sector: String,
    pub exchange: String,
    pub pe: f64,
}

/// One industry's finer-rotation read, joining FMP's free `industry-performance-snapshot`
/// (the day's average move) with `industry-pe-snapshot` (the aggregate P/E) by industry name
/// within one exchange. A finer cut than the ~11 `sectors` — FMP reports ~130 industries per
/// exchange, capped here to the strongest and weakest movers so the packet stays small while
/// still surfacing the rotation the sector aggregate hides. Like [`SectorPe`] the snapshot is
/// **exchange-specific**, so `exchange` is carried and the cap is applied per exchange (both
/// NASDAQ and NYSE). `change_pct` is the average percent move; `pe` is `None` when no
/// meaningful P/E is available — the PE snapshot didn't carry that industry, its aggregate
/// earnings are non-positive (FMP reports 0.0 there), or its call failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndustrySnapshot {
    pub industry: String,
    pub exchange: String,
    pub change_pct: f64,
    pub pe: Option<f64>,
}

/// The US equity-risk-premium from FMP's free `market-risk-premium` (Damodaran's
/// per-country dataset, filtered to the United States) — a valuation anchor for the report.
/// A near-static annual constant: `total_equity_risk_premium` is the expected excess return
/// demanded over the risk-free rate, and `country_risk_premium` its country component
/// (≈ 0 for the US). `country` is retained so the serialized value is self-labelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketRiskPremium {
    pub country: String,
    pub country_risk_premium: f64,
    pub total_equity_risk_premium: f64,
}

/// Which Step-6 baseline group a [`DataGap`] belongs to. Serializes to a stable kebab
/// label so the model reading the manifest sees the same group names the data groups
/// carry, and the coverage gate (`pipeline::enforce_coverage`) can match on it.
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
}

impl GroupKind {
    /// The stable kebab label, matching the serde rename. Used by the progress
    /// stream (`progress::ProgressEvent::RequestFinished`) so a tracker row's group
    /// reads the same name the serialized baseline carries.
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
        }
    }
}

/// Why a requested series / release didn't land in the baseline this run. The
/// distinction drives two things: the coverage gate counts only the *this-run* reasons
/// against a group's coverage (`OutOfScope` is permanent and excluded, so a series a
/// deployment never had doesn't drag the ratio down every week), and the agent reads a
/// transient outage differently from a permanent absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GapReason {
    /// Permanently absent for this deployment — the provider explicitly signalled the
    /// item isn't available (FMP 402 premium / 404, FRED "does not exist"). Not a
    /// this-run failure; excluded from the coverage denominator. Reserved for explicit
    /// provider signals only: a 2xx that simply carried no value (an empty array, an
    /// all-gap window, an empty `data` block) is `Unavailable`, not this.
    OutOfScope,
    /// Unavailable this run — a 429 / 5xx that survived the retry layer, a transport
    /// error, or a successful response that carried no usable value for an expected
    /// series (an empty array, an all-gap FRED window, a BLS empty-`data` series).
    /// Counts against coverage.
    Unavailable,
    /// The provider rejected the request — a bad / expired key (401/403, FRED `api_key`)
    /// or a quota / plan limit (FMP `Error Message`, BLS `REQUEST_NOT_PROCESSED`).
    /// Counts against coverage.
    Rejected,
    /// The response didn't match the expected contract — an unparseable body, a wrong
    /// shape, a non-numeric observation, or a truncated / omitted series. Counts against
    /// coverage.
    Malformed,
}

impl GapReason {
    /// Whether this gap counts against its group's coverage ratio. Every reason except
    /// `OutOfScope` (a permanent, expected absence) is a this-run failure that lowers
    /// coverage.
    pub fn counts_against_coverage(self) -> bool {
        !matches!(self, GapReason::OutOfScope)
    }

    /// The stable kebab label, matching the serde rename — the status a tracker row
    /// shows for a series that degraded to a gap (`progress::ProgressEvent`).
    pub fn as_str(self) -> &'static str {
        match self {
            GapReason::OutOfScope => "out-of-scope",
            GapReason::Unavailable => "unavailable",
            GapReason::Rejected => "rejected",
            GapReason::Malformed => "malformed",
        }
    }
}

/// One entry in the Step-6 missing-data manifest: a series / release a provider failed
/// to resolve this run, tagged with its group and the reason it's absent. Carried on
/// [`BaselineMarketData::gaps`], merged across providers by the composite, evaluated by
/// the coverage gate, and serialized into the agent's prompt so the model reasons over
/// what's known-absent rather than silently inferring it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataGap {
    pub group: GroupKind,
    pub series_id: String,
    pub series_name: String,
    pub reason: GapReason,
}

impl DataGap {
    /// Build a gap for `series_id` / `series_name` in `group`. The string-ish args take
    /// `&str` (the adapters' series tables) or `String` alike.
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

/// Schema version stamped on each persisted baseline snapshot (`storage::baseline_snapshots`).
/// The struct evolves as enrichment groups are added, so a snapshot persisted by an older
/// build may lack groups a newer build expects; every field carries `#[serde(default)]` so an
/// older blob still deserializes (missing groups read as empty). This version is recorded
/// alongside each snapshot for future migration tooling — the current decode path relies on
/// the serde defaults, not on inspecting it. Bump it when a change can't be absorbed by a
/// field default alone.
pub const BASELINE_SCHEMA_VERSION: u32 = 1;

/// The baseline market-data scan handed to the main agent as part of its input.
/// Empty vectors are valid — a provider that fails to resolve a group degrades it to
/// empty and records the reason in [`gaps`](Self::gaps) rather than failing the whole
/// scan. The mandatory-coverage floor (`pipeline::enforce_coverage`), not the adapters,
/// is the single place a too-thin baseline fails the run.
///
/// Every field carries `#[serde(default)]` so the struct round-trips through persistence
/// forward-compatibly: a snapshot written before a group existed still deserializes, with
/// the absent group defaulting to empty (see [`BASELINE_SCHEMA_VERSION`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BaselineMarketData {
    #[serde(default)]
    pub indices: Vec<Quote>,
    #[serde(default)]
    pub internals: Vec<Quote>,
    #[serde(default)]
    pub sectors: Vec<SectorPerformance>,
    /// Step-6 macro levels (Fed-funds target range, inflation breakevens, consumer
    /// sentiment, PCE, plus the headline activity reports — PPI, retail sales, JOLTS,
    /// real GDP — that back the `calendar`'s prior-week entries) — point-in-time FRED
    /// series, kept distinct from the market `internals`. Same `Quote` shape: `price` is
    /// the latest level and `change_pct` its change from the prior observation
    /// (day-over-day, month-over-month, or quarter-over-quarter by series frequency).
    #[serde(default)]
    pub macro_levels: Vec<Quote>,
    /// Step-6 labor levels (CPI, unemployment rate, nonfarm payrolls, average hourly
    /// earnings) — point-in-time BLS series, kept distinct from the FRED `macro_levels`
    /// by source and concern. Same `Quote` shape: `price` is the latest reported level
    /// and `change_pct` its month-over-month change from the prior reading.
    #[serde(default)]
    pub labor_levels: Vec<Quote>,
    /// Step-6 economic-release calendar (`docs/weekly-report-workflow.md §Step 6`): the
    /// prior-week and upcoming US economic reports (CPI, PCE, jobs, GDP, …) as a
    /// release schedule from FRED's free release-dates endpoint. A schedule of names +
    /// dates, not figures — the actual readings reach the model via `macro_levels` /
    /// `labor_levels`. Empty is valid (a quiet window, or the calendar soft-degraded); it
    /// carries no completeness floor, unlike the series groups.
    #[serde(default)]
    pub calendar: Vec<EconomicRelease>,
    /// Step-6 multi-horizon index performance, derived from FMP's end-of-day history
    /// (`historical-price-eod`): week-over-week / MTD / YTD returns and 52-week-range
    /// position per index, enriching the daily `indices` quotes. Empty is valid — like
    /// the `calendar` it carries no completeness floor and soft-degrades if the history
    /// fetch fails, since the daily `indices` quotes already satisfy Step 6.
    #[serde(default)]
    pub index_performance: Vec<IndexPerformance>,
    /// Step-6 market movers: the filtered top gainers / losers / most-active US names this
    /// run (FMP's free mover lists). A micro-breadth signal the index/sector groups can't
    /// give — which individual names moved most. Empty is valid; like `calendar` /
    /// `index_performance` it carries no completeness floor and soft-degrades, since the
    /// breadth read is additive over the required index/internals grounding.
    #[serde(default)]
    pub movers: Vec<StockMover>,
    /// Step-6 earnings calendar: large-cap US companies reporting in the prior-week +
    /// upcoming window (FMP's free earnings calendar, filtered by revenue estimate). Recent
    /// rows carry actual-vs-estimate; upcoming rows carry estimates only. Empty is valid —
    /// additive and non-floor like `movers`, soft-degrading rather than failing the run.
    #[serde(default)]
    pub earnings: Vec<EarningsEvent>,
    /// Step-6 sector valuation: each sector's aggregate P/E per exchange (FMP's free
    /// exchange-specific sector-PE snapshot, gathered for both NASDAQ and NYSE), a valuation
    /// complement to the `sectors` performance group. Empty is valid — additive and non-floor
    /// like `movers` / `earnings`, soft-degrading rather than failing the run.
    #[serde(default)]
    pub sector_pe: Vec<SectorPe>,
    /// Step-6 finer rotation + valuation: per exchange (NASDAQ + NYSE), the strongest and
    /// weakest industries this run (FMP's free industry-performance snapshot), each joined
    /// with its aggregate P/E where available — a finer cut than the ~11 `sectors`. Empty is
    /// valid; additive and non-floor, soft-degrading rather than failing the run.
    #[serde(default)]
    pub industries: Vec<IndustrySnapshot>,
    /// Step-6 valuation anchor: the US equity-risk-premium (FMP's free market-risk-premium,
    /// filtered to the United States) — a near-static annual constant. Zero or one row.
    /// Empty is valid; additive and non-floor, soft-degrading rather than failing the run.
    #[serde(default)]
    pub market_risk_premium: Vec<MarketRiskPremium>,
    /// Step-6 missing-data manifest: the series / releases a provider failed to resolve
    /// this run (`DataGap`), each tagged with its group and reason. Populated by the
    /// adapters as they degrade instead of failing, merged across providers by the
    /// composite, read by the coverage gate (`pipeline::enforce_coverage`) to decide the
    /// floor, and serialized into the agent's prompt so the model knows what's absent.
    /// Empty when a scan resolved everything.
    #[serde(default)]
    pub gaps: Vec<DataGap>,
}

/// Emit the `RequestFinished` row for one baseline request, derived from the adapter's
/// existing gap/value bookkeeping so its (well-tested) branch logic stays untouched.
/// Always emits exactly one row — pairing the `RequestStarted` the caller sent before
/// the HTTP call. `gaps_before` is `gaps.len()` captured before the request; `produced`
/// is whether it yielded a value. Produced wins (`ok`); otherwise a gap pushed this
/// request carries its reason; otherwise `empty` (a 2xx that yielded nothing and
/// recorded no gap — e.g. an additive enrichment skipped silently).
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
) {
    let status = if produced {
        "ok"
    } else if gaps.len() > gaps_before {
        gaps[gaps.len() - 1].reason.as_str()
    } else {
        "empty"
    };
    ctx.request_finished(provider, group.as_str(), series_id, name, status, None);
}

/// The data-source stage. One method: gather the required baseline scan. Sync,
/// like the `MainAgent` trait — the blocking HTTP call inside the real adapter is
/// offloaded via `spawn_blocking` at the Tauri command seam.
pub trait MarketDataSource {
    fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData>;
}

/// Deterministic offline stand-in for the real data adapter. Returns a small,
/// fixed baseline so the pipeline and its tests run without live keys.
#[derive(Debug, Default)]
pub struct StubMarketDataSource;

impl MarketDataSource for StubMarketDataSource {
    fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
        Ok(BaselineMarketData {
            indices: vec![
                Quote {
                    symbol: "^GSPC".into(),
                    name: "S&P 500".into(),
                    price: 5_500.0,
                    change_pct: 0.4,
                    unit: "index points".into(),
                },
                Quote {
                    symbol: "^IXIC".into(),
                    name: "Nasdaq Composite".into(),
                    price: 17_800.0,
                    change_pct: 0.6,
                    unit: "index points".into(),
                },
            ],
            internals: vec![Quote {
                symbol: "^VIX".into(),
                name: "CBOE Volatility Index".into(),
                price: 14.2,
                change_pct: -1.1,
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
                change_pct: 0.0,
                unit: "percent".into(),
            }],
            labor_levels: vec![Quote {
                symbol: "LNS14000000".into(),
                name: "Unemployment Rate".into(),
                price: 4.1,
                change_pct: 0.0,
                unit: "percent".into(),
            }],
            calendar: vec![EconomicRelease {
                release: "Employment Situation".into(),
                date: "2026-06-05".into(),
                status: "released".into(),
                expected: None,
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
                    pe: 38.4,
                },
                SectorPe {
                    sector: "Technology".into(),
                    exchange: "NYSE".into(),
                    pe: 24.6,
                },
                SectorPe {
                    sector: "Energy".into(),
                    exchange: "NYSE".into(),
                    pe: 12.1,
                },
            ],
            industries: vec![
                IndustrySnapshot {
                    industry: "Semiconductors".into(),
                    exchange: "NASDAQ".into(),
                    change_pct: 2.4,
                    pe: Some(41.2),
                },
                IndustrySnapshot {
                    industry: "Oil & Gas Midstream".into(),
                    exchange: "NYSE".into(),
                    change_pct: -1.9,
                    pe: None,
                },
            ],
            market_risk_premium: vec![MarketRiskPremium {
                country: "United States".into(),
                country_risk_premium: 0.0,
                total_equity_risk_premium: 4.46,
            }],
            gaps: Vec::new(),
        })
    }
}

/// Compose two `MarketDataSource`s into one baseline scan: run the `primary`
/// (e.g. FMP — indices, sectors, VIX, gold), then the `secondary`, and merge them
/// across every group (`indices`, `internals`, `sectors`, `macro_levels`,
/// `labor_levels`, `calendar`, `index_performance`) plus the `gaps` manifest. The
/// adapters degrade to recorded gaps rather than failing for data reasons, so a
/// provider that can't reach a group contributes empty data + gaps here instead of
/// aborting the run; the mandatory-coverage floor (`pipeline::enforce_coverage`)
/// downstream is the single place a too-thin merged baseline fails. A child returning
/// `Err` is now reserved for a catastrophic (non-data) fault and still propagates.
/// Order is primary-then-secondary, so the merged `internals` reads the primary's
/// quotes first. Sources compose by nesting: the run path wraps FMP+FRED, then nests
/// `bls` as the outer secondary to fold in the `labor_levels` group.
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
    fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
        let mut merged = self.primary.baseline_scan()?;
        let extra = self.secondary.baseline_scan()?;
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
        merged.gaps.extend(extra.gaps);
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub shaped like the secondary sources (`fred` + `bls`) — it contributes the
    /// `internals` / `macro_levels` (FRED) and `labor_levels` (BLS) groups the
    /// secondaries own, so the composite merge can be exercised offline.
    struct FredShapedStub {
        internals: Vec<Quote>,
        macro_levels: Vec<Quote>,
        labor_levels: Vec<Quote>,
        calendar: Vec<EconomicRelease>,
        gaps: Vec<DataGap>,
    }

    impl MarketDataSource for FredShapedStub {
        fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
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

    /// A stub whose scan always fails, to check that a secondary failure propagates.
    struct FailingStub;

    impl MarketDataSource for FailingStub {
        fn baseline_scan(&self) -> anyhow::Result<BaselineMarketData> {
            anyhow::bail!("provider down")
        }
    }

    fn quote(symbol: &str) -> Quote {
        Quote {
            symbol: symbol.into(),
            name: symbol.into(),
            price: 1.0,
            change_pct: 0.0,
            unit: "index points".into(),
        }
    }

    #[test]
    fn composite_merges_both_sources_into_one_baseline() {
        // Primary (FMP-shaped) carries indices + a VIX internal + sectors + one macro
        // level (DFEDTARU) + one labor level (LNS14000000); the secondary adds two more
        // internals, a macro level, and a labor level. The merge keeps every group and
        // orders each primary-first.
        let secondary = FredShapedStub {
            internals: vec![quote("DGS10"), quote("DTWEXBGS")],
            macro_levels: vec![quote("T10YIE")],
            labor_levels: vec![quote("CUUR0000SA0")],
            calendar: vec![EconomicRelease {
                release: "Consumer Price Index".into(),
                date: "2026-06-10".into(),
                status: "upcoming".into(),
                expected: None,
            }],
            // A FRED-side gap (an oil series that 5xx'd this run) must survive the merge
            // into the unified manifest the gate and the agent read.
            gaps: vec![DataGap::new(
                GroupKind::Internals,
                "DCOILWTICO",
                "WTI Crude Oil",
                GapReason::Unavailable,
            )],
        };
        let composite = CompositeMarketDataSource::new(StubMarketDataSource, secondary);
        let data = composite.baseline_scan().unwrap();

        assert!(!data.indices.is_empty(), "primary indices survive");
        assert!(!data.sectors.is_empty(), "primary sectors survive");
        // VIX (from the stub) first, then the two FRED series.
        assert_eq!(data.internals.len(), 3);
        assert_eq!(data.internals[0].symbol, "^VIX");
        assert_eq!(data.internals[1].symbol, "DGS10");
        assert_eq!(data.internals[2].symbol, "DTWEXBGS");
        // macro_levels merges primary-first too: the stub's DFEDTARU, then FRED's.
        assert_eq!(data.macro_levels.len(), 2);
        assert_eq!(data.macro_levels[0].symbol, "DFEDTARU");
        assert_eq!(data.macro_levels[1].symbol, "T10YIE");
        // labor_levels merges primary-first too: the stub's LNS14000000, then the
        // secondary's BLS series.
        assert_eq!(data.labor_levels.len(), 2);
        assert_eq!(data.labor_levels[0].symbol, "LNS14000000");
        assert_eq!(data.labor_levels[1].symbol, "CUUR0000SA0");
        // calendar merges primary-first too: the stub's Employment Situation, then the
        // secondary's CPI release.
        assert_eq!(data.calendar.len(), 2);
        assert_eq!(data.calendar[0].release, "Employment Situation");
        assert_eq!(data.calendar[1].release, "Consumer Price Index");
        // The secondary's gap rides into the merged manifest (the primary stub has none).
        assert_eq!(data.gaps.len(), 1);
        assert_eq!(data.gaps[0].series_id, "DCOILWTICO");
        assert_eq!(data.gaps[0].reason, GapReason::Unavailable);
    }

    #[test]
    fn composite_propagates_a_secondary_failure() {
        // A FRED-side failure must fail the whole scan, since FRED sources
        // non-optional baseline series.
        let composite = CompositeMarketDataSource::new(StubMarketDataSource, FailingStub);
        assert!(composite.baseline_scan().is_err());
    }

    #[test]
    fn stub_populates_groups_and_round_trips() {
        let data = StubMarketDataSource.baseline_scan().unwrap();
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

        // The whole packet — including the gaps manifest — serializes and parses back
        // unchanged: the contract the agent input and the model prompt both lean on.
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
        // Simulates a snapshot persisted by a build that predates several groups: the
        // JSON carries only `indices` and omits everything else. `#[serde(default)]` on
        // every field must let it decode, with the absent groups reading as empty — the
        // forward-compatibility the persisted-history feature relies on.
        let older = r#"{
            "indices": [
                {"symbol":"^GSPC","name":"S&P 500","price":5500.0,"change_pct":0.4,"unit":"index points"}
            ]
        }"#;
        let back: BaselineMarketData = serde_json::from_str(older).unwrap();
        assert_eq!(back.indices.len(), 1);
        assert_eq!(back.indices[0].symbol, "^GSPC");
        // Groups absent from the older blob decode to empty, not an error.
        assert!(back.internals.is_empty());
        assert!(back.macro_levels.is_empty());
        assert!(back.sector_pe.is_empty());
        assert!(back.market_risk_premium.is_empty());
        assert!(back.gaps.is_empty());
    }
}
