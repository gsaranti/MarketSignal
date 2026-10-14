//! The Portfolio Analysis job lifecycle (`docs/portfolio-analysis.md`): claims the
//! single global run slot ([`RunGuard`]) shared with the report job, runs each
//! holding through [`crate::portfolio::pipeline`], builds the roll-up, persists the
//! run, and records the outcome to `job_runs`. Holdings, company data and the model
//! stages are traits, so the whole job runs offline against stubs.

use anyhow::Result;
use rusqlite::Connection;

use crate::jobs::{record_run, JobRun, JobState, RunGuard, RunKind};
use crate::pipeline::ReportPaths;
use crate::portfolio::dossier::{self, HoldingDossier};
use crate::portfolio::engine::CompanyFinancials;
use crate::portfolio::pipeline::{analyze_holding, HoldingAnalyst};
use crate::portfolio::{
    diff, store, ExitedPosition, HoldingAudit, HoldingVerdict, InvestorProfile, PortfolioRollUp,
    PortfolioRun,
};
use crate::progress::RunContext;
use crate::schwab::{Holdings, HoldingsSource};
use crate::sec::{CompanyFacts, SecEdgarSource};
use crate::storage;

/// The `job_runs.job_type` slug for Portfolio Analysis runs; `pub(crate)` because
/// `jobs::job_status` scopes its per-section footer stamps by it.
pub(crate) const PORTFOLIO_JOB: &str = "portfolio_analysis";

/// Human title for the run tracker header.
const RUN_LABEL: &str = "Portfolio Analysis";

const SKIP_REASON: &str = "another run is already in progress";

/// SEC EDGAR facts plus the degraded-input notes the gather produced. A failed
/// fetch must leave a tagged gap rather than silently empty facts, or an outage is
/// indistinguishable from "SEC was unnecessary".
#[derive(Debug, Clone, Default)]
pub struct SecData {
    /// `Some` whenever the endpoint was queried (a failed fetch degrades to
    /// `Some(default)` beside its gap), `None` when it never was; the audit's
    /// source label reads the distinction.
    pub facts: Option<CompanyFacts>,
    pub gaps: Vec<String>,
}

/// The per-holding company-financials source, behind a trait so the job is
/// offline-testable. The fund-surface methods carry fail-soft defaults so a
/// stock-only stub stays small.
pub trait CompanyDataSource {
    /// Fail-soft; gaps recorded on the result.
    fn financials(&self, symbol: &str) -> CompanyFinancials;
    /// The fund flavor of the per-symbol pull: no statement or consensus surface,
    /// so a fund logs no spurious stock gaps.
    fn fund_financials(&self, symbol: &str) -> CompanyFinancials {
        self.financials(symbol)
    }
    fn facts(&self, symbol: &str) -> SecData;
    /// Reuses the already-loaded SEC identity map after the normal gather.
    fn research_cik(&self, _symbol: &str) -> Option<String> {
        None
    }
    /// Deep dated daily closes plus gap notes. Fail-soft: an empty history leaves
    /// the anchor window to its documented fallback.
    fn deep_price_history(&self, _symbol: &str) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
        (vec![], vec![])
    }
    /// The default records the missing source as a gap so the fund floors honestly.
    fn fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
        crate::portfolio::fund::FundData {
            symbol: symbol.to_string(),
            gaps: vec!["fund metadata source not wired".to_string()],
            ..Default::default()
        }
    }
    /// The per-sector aggregate P/E snapshot (both exchanges) for the run's pinned
    /// ET session: the caller passes the run's `today`, never a fresh clock read,
    /// so a run crossing ET midnight still snapshots its own session. Memoized by
    /// the caller across funds.
    fn sector_pe_snapshot(
        &self,
        _session: chrono::NaiveDate,
    ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        Ok(vec![])
    }
    /// Trailing per-sector P/E history (both exchanges); memoized by the caller
    /// across funds.
    fn sector_pe_history(&self, _sector: &str) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        Ok(vec![])
    }
    /// The FMP profile lookup feeding the listing-resolution guard
    /// (`docs/portfolio-analysis.md §Asset eligibility`) and the outcome episodes'
    /// sector identity. The `Unverified` default lets an offline stub proceed with
    /// a recorded degraded input, never a fabricated resolution.
    fn profile_identity(&self, _symbol: &str) -> crate::portfolio::listing::ProfileLookup {
        crate::portfolio::listing::ProfileLookup::Unverified(
            "profile source not wired".to_string(),
        )
    }
    /// The item-classified 8-K filings sweep (`docs/portfolio-analysis.md`
    /// §Starting parameters; `docs/trade-opportunities-workflow.md §Step 5c`).
    /// `since` is an inclusive ISO date. `None` means the leg is not wired (a
    /// stub); the live impl always returns `Some`, typing an unrunnable sweep
    /// `Unknown` rather than a fabricated clear.
    fn filing_events(
        &self,
        _symbol: &str,
        _since: &str,
    ) -> Option<crate::portfolio::FilingSweepRead> {
        None
    }
    /// The per-holding evidence surface (`docs/portfolio-workflow.md` §Step 6c),
    /// each leg fail-soft to a tagged gap. `session` is the run's pinned ET
    /// session the windows count back from. The empty default means the leg is
    /// not wired, which is not a degraded live input, so no gap.
    fn evidence(
        &self,
        symbol: &str,
        _session: chrono::NaiveDate,
    ) -> crate::portfolio::evidence::CompanyEvidence {
        crate::portfolio::evidence::CompanyEvidence::empty(symbol)
    }
    /// Symbol-scoped news items since `from` (ISO date): research leads, never
    /// evidence (`docs/web-research.md`). Fail-soft: the empty default runs the
    /// loop with no leads.
    fn news_items(&self, _symbol: &str, _from: &str) -> Vec<crate::fmp::SymbolNewsItem> {
        Vec::new()
    }
}

/// The run-level market-context source (`docs/portfolio-workflow.md` §Step 5),
/// behind a trait so the job is offline-testable.
pub trait MarketContextSource {
    /// The `DGS2` / `DGS10` prints plus the DGS10 anchor-window history, as decimal
    /// ratios. Hard-fail on the prints only (`docs/portfolio-analysis.md` §Failure
    /// posture); a failed history leaves the window empty and records the reason
    /// on `history_gap`, never a run failure.
    fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors>;

    /// The run-level commodity context (`docs/portfolio-workflow.md` §Step 5),
    /// each series fail-soft to a typed gap. `session` dates the gold quote. The
    /// default (empty, no gaps) means the leg is not wired.
    fn commodities(
        &self,
        _session: chrono::NaiveDate,
    ) -> crate::portfolio::dossier::CommodityContext {
        crate::portfolio::dossier::CommodityContext::default()
    }

    /// The run-level CFTC COT positioning on the bellwether contracts
    /// (`docs/portfolio-workflow.md` §Step 5): rows + gap notes, wholly
    /// fail-soft. The default (empty, no gaps) means the leg is not wired.
    fn positioning(
        &self,
        _session: chrono::NaiveDate,
    ) -> (Vec<crate::data_sources::CotPositioning>, Vec<String>) {
        (Vec::new(), Vec::new())
    }

    /// The CBOE venue-level put/call backdrop (`docs/data-sources.md §CBOE`),
    /// wholly fail-soft. `(None, None)` means the leg is not wired.
    fn put_call_backdrop(&self) -> (Option<crate::cboe::PutCallBackdrop>, Option<String>) {
        (None, None)
    }

    /// The FINRA consolidated short-interest file (`docs/data-sources.md
    /// §FINRA`), fetched once per run and read per stock at dossier assembly;
    /// wholly fail-soft. `(None, None)` means the leg is not wired.
    fn short_interest(&self) -> (Option<crate::finra::ShortInterestFile>, Option<String>) {
        (None, None)
    }

    /// The run-level M&A feed (`docs/data-sources.md` §Portfolio Analysis —
    /// endpoint surface), fetched once per run and pinned on the checkpoint
    /// header; wholly fail-soft. `session` is the run's pinned ET session. The
    /// default (no deals, no gap) means the leg is not wired.
    fn mergers_acquisitions(
        &self,
        _session: chrono::NaiveDate,
    ) -> (Vec<crate::portfolio::evidence::MaDeal>, Option<String>) {
        (Vec::new(), None)
    }
}

/// How many days of DGS10 history the anchor-window request covers: the ~12-quarter
/// window plus the four TTM quarters behind its oldest anchor, plus alignment slack.
const RATE_HISTORY_LOOKBACK_DAYS: i64 = 1_600;

/// The live market context: FRED rates plus the run-level commodity,
/// positioning, sentiment and short-interest pulls.
pub struct LiveMarketContext {
    pub fred: crate::fred::FredDataSource,
    /// `None` keeps a rates-only construction valid; the commodity leg then
    /// records gold as a gap.
    pub fmp: Option<crate::fmp::FmpDataSource>,
    /// `None` records the positioning leg as a gap.
    pub cot: Option<crate::cot::CotDataSource>,
    /// `None` records the sentiment leg as a gap.
    pub cboe: Option<crate::cboe::CboeDataSource>,
    /// `None` records the short-interest leg as a gap.
    pub finra: Option<crate::finra::FinraDataSource>,
}

/// The FRED commodity catalog: series id, display label, published unit, sleeve
/// (`docs/data-sources.md §Portfolio Analysis — endpoint surface`).
const FRED_COMMODITY_SERIES: &[(&str, &str, &str, crate::portfolio::dossier::CommodityGroup)] = &[
    ("DCOILWTICO", "WTI Crude Oil", "USD per barrel", crate::portfolio::dossier::CommodityGroup::Energy),
    ("DHHNGSP", "Henry Hub Natural Gas", "USD per million BTU", crate::portfolio::dossier::CommodityGroup::Energy),
    ("PCOPPUSDM", "Copper (IMF, monthly)", "USD per metric ton", crate::portfolio::dossier::CommodityGroup::Metals),
    ("PALUMUSDM", "Aluminum (IMF, monthly)", "USD per metric ton", crate::portfolio::dossier::CommodityGroup::Metals),
    ("PNICKUSDM", "Nickel (IMF, monthly)", "USD per metric ton", crate::portfolio::dossier::CommodityGroup::Metals),
    ("PIORECRUSDM", "Iron Ore (IMF, monthly)", "USD per metric ton", crate::portfolio::dossier::CommodityGroup::Metals),
    ("PURANUSDM", "Uranium (IMF, monthly)", "USD per pound", crate::portfolio::dossier::CommodityGroup::Uranium),
];

impl MarketContextSource for LiveMarketContext {
    fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors> {
        let dgs2 = self.fred.latest_rate_dated("DGS2")?;
        let dgs10 = self.fred.latest_rate_dated("DGS10")?;
        // Fetch-range upper bound deliberately stays UTC: a forward-rolled bound
        // asks for an untraded day and shifts the lookback by one.
        let to = chrono::Utc::now().date_naive();
        let from = to - chrono::Duration::days(RATE_HISTORY_LOOKBACK_DAYS);
        // Fail-soft (`docs/portfolio-analysis.md` §Starting parameters).
        let (dgs10_history, history_gap) =
            match self.fred.rate_history_decimal("DGS10", from, to) {
                Ok(history) => (history, None),
                Err(e) => (
                    Vec::new(),
                    Some(format!(
                        "DGS10 anchor-window history unavailable: {e} — every spread \
                         observation inadmissible; targets fell to the documented \
                         raw-percentile / carry fallback"
                    )),
                ),
            };
        Ok(crate::portfolio::engine::RateAnchors {
            dgs2: dgs2.value,
            dgs10: dgs10.value,
            dgs2_date: Some(dgs2.date),
            dgs10_date: Some(dgs10.date),
            dgs10_history,
            history_gap,
        })
    }

    fn commodities(
        &self,
        session: chrono::NaiveDate,
    ) -> crate::portfolio::dossier::CommodityContext {
        use crate::portfolio::dossier::{CommodityContext, CommodityGroup, CommodityPrint};
        let mut ctx = CommodityContext::default();
        // Fetch-range upper bound deliberately stays UTC (see market_clock for session dates).
        let to = chrono::Utc::now().date_naive();
        let from = to - chrono::Duration::days(crate::portfolio::dossier::COMMODITY_WINDOW_DAYS);
        for (series_id, label, unit, group) in FRED_COMMODITY_SERIES {
            match self.fred.level_window(series_id, from, to) {
                Ok(window) if !window.is_empty() => {
                    let latest = window.last().cloned().expect("non-empty window");
                    let trailing = (window.len() > 1).then(|| window[0].clone());
                    ctx.prints.push(CommodityPrint {
                        label: label.to_string(),
                        unit: unit.to_string(),
                        group: *group,
                        latest,
                        trailing,
                    });
                }
                Ok(_) => ctx
                    .gaps
                    .push(format!("{label} ({series_id}): window served no print")),
                Err(e) => ctx
                    .gaps
                    .push(format!("{label} ({series_id}): unavailable — {e}")),
            }
        }
        match self
            .fmp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no FMP source wired"))
            .and_then(|fmp| fmp.fetch_commodity_quote("GCUSD", session))
        {
            Ok(latest) => ctx.prints.push(CommodityPrint {
                label: "Gold".to_string(),
                unit: "USD per troy ounce".to_string(),
                group: CommodityGroup::Gold,
                latest,
                trailing: None,
            }),
            Err(e) => ctx.gaps.push(format!("Gold (GCUSD): unavailable — {e}")),
        }
        ctx
    }

    fn positioning(
        &self,
        session: chrono::NaiveDate,
    ) -> (Vec<crate::data_sources::CotPositioning>, Vec<String>) {
        match &self.cot {
            Some(cot) => cot.positioning(session),
            None => (
                Vec::new(),
                vec!["CFTC positioning source not wired".to_string()],
            ),
        }
    }

    fn put_call_backdrop(&self) -> (Option<crate::cboe::PutCallBackdrop>, Option<String>) {
        match &self.cboe {
            Some(cboe) => match cboe.put_call_backdrop() {
                Ok(b) => (Some(b), None),
                Err(e) => (None, Some(format!("CBOE put/call backdrop unavailable: {e}"))),
            },
            None => (None, Some("CBOE source not wired".to_string())),
        }
    }

    fn short_interest(&self) -> (Option<crate::finra::ShortInterestFile>, Option<String>) {
        match &self.finra {
            Some(finra) => match finra.short_interest() {
                Ok(f) => (Some(f), None),
                Err(e) => (None, Some(format!("FINRA short interest unavailable: {e}"))),
            },
            None => (None, Some("FINRA source not wired".to_string())),
        }
    }

    fn mergers_acquisitions(
        &self,
        session: chrono::NaiveDate,
    ) -> (Vec<crate::portfolio::evidence::MaDeal>, Option<String>) {
        match &self.fmp {
            Some(fmp) => fmp.fetch_recent_mergers(session),
            None => (
                Vec::new(),
                Some("FMP source not wired (M&A feed)".to_string()),
            ),
        }
    }
}

/// The exchange blend the fund composite is defined on
/// (`docs/portfolio-analysis.md` §Asset eligibility).
const SECTOR_PE_EXCHANGES: [&str; 2] = ["NYSE", "NASDAQ"];

/// Joins the two exchange legs only when both served rows: a one-board surface is
/// not the blend the fund valuation is defined on, so it goes through the typed-gap
/// channel instead of masquerading as a usable snapshot.
fn complete_sector_pe_exchange_basis(
    context: &str,
    fetched: [Result<Vec<crate::portfolio::fund::SectorPe>>; 2],
) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
    let mut rows = Vec::new();
    let mut missing = Vec::new();
    for (exchange, result) in SECTOR_PE_EXCHANGES.into_iter().zip(fetched) {
        match result {
            Ok(mut exchange_rows) if !exchange_rows.is_empty() => {
                rows.append(&mut exchange_rows)
            }
            Ok(_) => missing.push(format!("{exchange} served no rows")),
            Err(error) => missing.push(format!("{exchange} failed: {error:#}")),
        }
    }
    if missing.is_empty() {
        Ok(rows)
    } else {
        anyhow::bail!("{context} has an incomplete exchange basis ({})", missing.join("; "))
    }
}

/// The live company-data source: FMP per-company plus fail-soft SEC EDGAR, each
/// degradation recorded as a gap.
///
/// Ordering invariant: constructing this performs no network I/O. Every external
/// fetch, the ticker → CIK map refresh included, happens inside the global run
/// slot after `run_portfolio_job` has called `reset_cancel` + `run_started`, so
/// each request sees the run's own cancel state and streams under the active step.
pub struct LiveCompanyData {
    pub fmp: crate::fmp::FmpDataSource,
    pub sec: SecEdgarSource,
    /// Loaded on first use: the first stock's `facts` call inside the slot
    /// triggers it. An unresolved ticker is a typed gap, never a fabricated mapping.
    pub cik: crate::sec::LazyCikResolver,
}

/// Resolves the CIK (loading the map on first use), then fetches the facts. Each
/// degradation is a recorded gap, never silently empty facts.
pub(crate) fn sec_company_facts(
    cik: &crate::sec::LazyCikResolver,
    sec: &SecEdgarSource,
    symbol: &str,
) -> SecData {
    match cik.resolve(sec, symbol) {
        None => SecData {
            facts: None,
            gaps: vec![format!("SEC: no CIK mapping for {symbol}")],
        },
        Some(cik) => match sec.fetch_company_facts(cik) {
            // A clean fetch that carried nothing is not a degradation.
            Ok(facts) => SecData {
                facts: Some(facts),
                gaps: Vec::new(),
            },
            // Queried but failed: `Some(default)` so the leg still labels as consulted.
            Err(e) => SecData {
                facts: Some(CompanyFacts::default()),
                gaps: vec![format!("SEC company facts unavailable: {e}")],
            },
        },
    }
}

/// How many days of deep price history the anchor join needs: the ~12-quarter window
/// (3y) plus the TTM quarters behind its oldest anchor (1y) plus slack.
const DEEP_HISTORY_LOOKBACK_DAYS: i64 = 1_600;

impl CompanyDataSource for LiveCompanyData {
    fn financials(&self, symbol: &str) -> CompanyFinancials {
        self.fmp.fetch_company_financials(symbol)
    }

    fn news_items(&self, symbol: &str, from: &str) -> Vec<crate::fmp::SymbolNewsItem> {
        self.fmp
            .fetch_symbol_news_since(symbol, from)
            .unwrap_or_default()
    }

    fn fund_financials(&self, symbol: &str) -> CompanyFinancials {
        self.fmp.fetch_fund_financials(symbol)
    }

    fn deep_price_history(
        &self,
        symbol: &str,
    ) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
        match self.fmp.fetch_dated_eod(symbol, DEEP_HISTORY_LOOKBACK_DAYS) {
            Ok(closes) if !closes.is_empty() => (closes, vec![]),
            Ok(_) => (
                vec![],
                vec![format!(
                    "FMP deep price history empty for {symbol}; the anchor window \
                     falls to its documented fallback"
                )],
            ),
            Err(e) => (
                vec![],
                vec![format!(
                    "FMP deep price history unavailable for {symbol}: {e}; the \
                     anchor window falls to its documented fallback"
                )],
            ),
        }
    }

    fn fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
        self.fmp.fetch_fund_data(symbol)
    }

    fn sector_pe_snapshot(
        &self,
        session: chrono::NaiveDate,
    ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        // The snapshot endpoint is date-keyed and answers an untraded session with
        // 200 and an empty array, not an error, so the date is the run's pinned ET
        // session (never a UTC or clock read) and the walk backs over weekday
        // candidates as the report path does. An empty snapshot is not inert:
        // every priced US-equity fund would abstain, misattributed to "no
        // P/E-usable sector overlap".
        let today = session;
        let mut candidate_failures = Vec::new();
        for candidate in sector_pe_candidates(today) {
            let date = candidate.format("%Y-%m-%d").to_string();
            let fetched = SECTOR_PE_EXCHANGES
                .map(|exchange| self.fmp.fetch_sector_pe_snapshot(exchange, &date));
            match complete_sector_pe_exchange_basis(
                &format!("sector-P/E snapshot for {date}"),
                fetched,
            ) {
                Ok(rows) => return Ok(rows),
                Err(error) => candidate_failures.push(format!("{error:#}")),
            }
        }
        // `Err` is this seam's gap channel, so every fund names the missing source
        // instead of pricing on a different basis from its history.
        anyhow::bail!(
            "no complete sector-P/E snapshot in the {} weekdays through {today}: {}",
            crate::fmp::SECTOR_LOOKBACK_WEEKDAYS,
            candidate_failures.join(" | ")
        )
    }

    fn sector_pe_history(&self, sector: &str) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        // Fetch-range upper bound deliberately stays UTC (see market_clock for session dates).
        let to = chrono::Utc::now().date_naive();
        let from = to - chrono::Duration::days(DEEP_HISTORY_LOOKBACK_DAYS);
        let (from, to) = (
            from.format("%Y-%m-%d").to_string(),
            to.format("%Y-%m-%d").to_string(),
        );
        let fetched = SECTOR_PE_EXCHANGES.map(|exchange| {
            self.fmp
                .fetch_historical_sector_pe(sector, exchange, &from, &to)
        });
        complete_sector_pe_exchange_basis(
            &format!("sector-P/E history for {sector}"),
            fetched,
        )
    }

    fn profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
        self.fmp.fetch_profile_identity(symbol)
    }

    fn facts(&self, symbol: &str) -> SecData {
        sec_company_facts(&self.cik, &self.sec, symbol)
    }

    fn research_cik(&self, symbol: &str) -> Option<String> {
        self.cik.resolve(&self.sec, symbol).map(str::to_owned)
    }

    fn filing_events(
        &self,
        symbol: &str,
        since: &str,
    ) -> Option<crate::portfolio::FilingSweepRead> {
        use crate::portfolio::{FilingSweepRead, ForensicFilingState};
        let unknown = |reason: String, queried: bool| FilingSweepRead {
            state: ForensicFilingState::Unknown { reason, queried },
            filings_8k: Vec::new(),
        };
        Some(match self.cik.resolve(&self.sec, symbol) {
            // No EDGAR mapping is a typed unknown, never a clean no-event.
            None => unknown(format!("no CIK mapping for {symbol}"), false),
            // One submissions read serves both the sweep and the FETCHED VALUES 8-K list.
            Some(cik) => match self.sec.fetch_recent_filings(cik) {
                Ok(filings) => FilingSweepRead::from_filings(symbol, &filings, since),
                Err(e) => unknown(format!("SEC filings sweep unavailable: {e}"), true),
            },
        })
    }

    fn evidence(
        &self,
        symbol: &str,
        session: chrono::NaiveDate,
    ) -> crate::portfolio::evidence::CompanyEvidence {
        self.fmp.fetch_company_evidence(symbol, session)
    }
}

/// How a Portfolio Analysis run ended, mirroring [`crate::jobs::JobOutcome`]. The run
/// is boxed on success since [`PortfolioRun`] dwarfs the `String` variants.
#[derive(Debug)]
pub enum PortfolioJobOutcome {
    Successful(Box<PortfolioRun>),
    Failed(String),
    Skipped(String),
    Cancelled(String),
}

/// A selective re-analysis request (`docs/portfolio-analysis.md` §Triggering): the
/// per-card selection bundled with the retrieval surface the in-run safety sweep
/// over the unselected tail needs, so a selective run without its sweep source is
/// unrepresentable. `None`, an empty selection, or no prior run runs the whole book.
pub struct SelectiveRun<'a> {
    /// Case-insensitive; silently intersected with the current book.
    pub selected: Vec<String>,
    /// The engine-only retrieval surface for the tail sweep.
    pub quick_data: &'a dyn crate::portfolio::quick_check::QuickCheckDataSource,
}

/// The over-age boundary for a carried verdict, aligned with the suite's ~4-week
/// research-freshness window (`docs/portfolio-analysis.md` §Triggering). Mirrored
/// by `src/components/PortfolioView.vue` `OVER_AGE_DAYS`; recalibrate both.
const OVER_AGE_DAYS: i64 = 28;

/// Both sides date on the ET session, never the UTC date prefix, or an evening-ET
/// vintage reads one day younger than its session. An unparseable vintage reads
/// over-age, the conservative resolution.
fn over_age(vintage: &str, today: chrono::NaiveDate) -> bool {
    match crate::market_clock::et_date_of(vintage) {
        Some(d) => (today - d).num_days() > OVER_AGE_DAYS,
        None => true,
    }
}

/// The carried-forward verdict for a holding whose fresh analysis produced no
/// verdict this pass (`docs/portfolio-analysis.md` §Triggering, §Failure posture).
/// Returns the carried verdict and whether it is over-age; the caller records the
/// carry and carries the prior audit.
fn carry_prior_verdict(
    position: &crate::schwab::Position,
    prior_verdict: &HoldingVerdict,
    prior_created_at: &str,
    holdings_diff: &diff::HoldingsDiff,
    today: chrono::NaiveDate,
) -> (HoldingVerdict, bool) {
    let mut carried = prior_verdict.clone();
    let vintage =
        crate::portfolio::effective_vintage(prior_verdict, prior_created_at).to_string();
    carried.analyzed_at = Some(vintage.clone());
    let delta = holdings_diff.delta_for(&position.symbol);
    carried.position_change = delta.change;
    carried.basis_move = diff::basis_move(&delta, position);
    // A directional verdict is only ever authored for a long position (the
    // eligibility gate in `pipeline.rs`), so it is side-reversed exactly when the
    // position is now net-short. Marked for the card badge, never force-included
    // (`docs/portfolio-analysis.md` §Triggering).
    carried.side_reversed = position.quantity < 0.0
        && matches!(
            carried.disposition,
            crate::portfolio::VerdictDisposition::Priced(_)
                | crate::portfolio::VerdictDisposition::RoleRiskOnly(_)
        );
    // The over-age add-family demotion is the one deterministic carry rule.
    let stale = over_age(&vintage, today);
    if stale {
        match &mut carried.disposition {
            crate::portfolio::VerdictDisposition::Priced(g) if g.action.is_add_family() => {
                g.action = crate::portfolio::Action::Hold;
                carried.action_source = crate::portfolio::ActionSource::RuleDemoted;
            }
            // A role-risk verdict can carry an add-family action too, so the rule
            // is branch-unscoped (`docs/portfolio-analysis.md` §Triggering).
            crate::portfolio::VerdictDisposition::RoleRiskOnly(r) if r.action.is_add_family() => {
                r.action = crate::portfolio::Action::Hold;
                carried.action_source = crate::portfolio::ActionSource::RuleDemoted;
            }
            _ => {}
        }
    }
    (carried, stale)
}

/// The resume window (`docs/portfolio-analysis.md` §Starting parameters): past it
/// the checkpoints are stale against a book that may have moved, so Run analysis
/// starts a new run.
pub const RESUME_WINDOW_HOURS: i64 = 48;

/// Whether an interrupted run's checkpoints can resume; `Ok(())` = offerable
/// (`docs/portfolio-analysis.md` §Failure posture). The stamps are the resume
/// contract: a mismatch on any refuses rather than mixing verdicts across
/// contracts, so a change to completed-holding semantics must move the axis it
/// changed; no build identity is checked. The `Err` carries the reason the UI shows.
pub fn resume_eligibility(
    conn: &Connection,
    cp: &store::Checkpoint,
    current_model_ids: &[String],
    now: chrono::DateTime<chrono::Utc>,
) -> std::result::Result<(), String> {
    let created = chrono::DateTime::parse_from_rfc3339(&cp.header.created_at)
        .map_err(|e| format!("unreadable checkpoint timestamp: {e}"))?;
    let age = now.signed_duration_since(created.with_timezone(&chrono::Utc));
    if age > chrono::Duration::hours(RESUME_WINDOW_HOURS) {
        return Err(format!(
            "the pinned holdings pull is older than the {RESUME_WINDOW_HOURS}-hour resume window"
        ));
    }
    if cp.header.prompt_version != crate::portfolio::PROMPT_VERSION {
        return Err("the prompt/schema version changed since the interrupted run".into());
    }
    if cp.header.grade_parameter_version != crate::portfolio::engine::GRADE_PARAMETER_VERSION {
        return Err("the grade parameters changed since the interrupted run".into());
    }
    if cp.header.target_parameter_version
        != crate::portfolio::engine::SCENARIO_TARGET_PARAMETER_VERSION
    {
        return Err("the scenario-target parameters changed since the interrupted run".into());
    }
    if cp.header.pre_profit_parameter_version
        != crate::portfolio::pre_profit::PRE_PROFIT_PARAMETER_VERSION
    {
        return Err("the pre-profit parameters changed since the interrupted run".into());
    }
    if cp.header.quick_check_parameter_version
        != crate::portfolio::quick_check::QUICK_CHECK_PARAMETER_VERSION
    {
        return Err(
            "the quick-check evaluation parameters changed since the interrupted run".into(),
        );
    }
    if cp.header.evidence_floor_version != crate::portfolio::engine::EVIDENCE_FLOOR_VERSION {
        return Err("the evidence-floor rule changed since the interrupted run".into());
    }
    if cp.header.checkpoint_format_version != store::CHECKPOINT_FORMAT_VERSION {
        return Err("the checkpoint format changed since the interrupted run".into());
    }
    if cp.header.model_ids != current_model_ids {
        return Err("the configured model roster changed since the interrupted run".into());
    }
    // A newer persisted run means these checkpoints describe a superseded book state.
    let latest = store::latest_run(conn).ok().flatten().map(|r| r.run_id);
    if latest != cp.header.prior_run_id {
        return Err("a newer run has persisted since the interrupted run".into());
    }
    Ok(())
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// Runs one Portfolio Analysis job end to end. Returns `Err` only on an
/// infrastructure failure (the database); a failed analysis is a normal
/// `Ok(Failed)`, and a per-holding model failure is isolated rather than fatal
/// (`docs/portfolio-analysis.md` §Failure posture).
///
/// Slot ordering: the global run slot is claimed, then the cancel flag cleared and
/// `run_started` emitted, and only then does any external fetch happen, so the
/// sources handed in must not fetch during construction and a competing attempt
/// records `Skipped` having contacted nothing.
#[allow(clippy::too_many_arguments)]
pub fn run_portfolio_job(
    holdings_source: &dyn HoldingsSource,
    company_data: &dyn CompanyDataSource,
    market: &dyn MarketContextSource,
    analyst: &dyn HoldingAnalyst,
    profile: &InvestorProfile,
    selective: Option<SelectiveRun<'_>>,
    outcome_prices: Option<&dyn crate::portfolio::outcome::OutcomePriceSource>,
    resume: Option<store::Checkpoint>,
    paths: &ReportPaths,
    guard: &RunGuard,
    ctx: &RunContext,
) -> Result<PortfolioJobOutcome> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;

    // The slot is shared with the report job and held until this function returns.
    let _token = match guard.try_begin(RunKind::Portfolio) {
        Some(t) => t,
        None => {
            let now = now_rfc3339();
            record_run(
                &conn,
                &JobRun {
                    job_type: PORTFOLIO_JOB,
                    state: JobState::Skipped,
                    started_at: &now,
                    finished_at: &now,
                    report_id: None,
                    detail: Some(SKIP_REASON),
                },
            )?;
            return Ok(PortfolioJobOutcome::Skipped(SKIP_REASON.to_string()));
        }
    };

    ctx.reset_cancel();
    ctx.run_started(RUN_LABEL);
    let started_at = now_rfc3339();

    // Panic containment (`docs/portfolio-analysis.md` §Failure posture): a panic
    // below the spine must reach the same terminal lifecycle as any hard failure
    // (the job-history row, `run_finished`, an offerable checkpoint trail).
    // Unwinding panics only; an abort is not catchable in-process. The payload
    // message is the failed detail; the file:line stays on stderr via the default hook.
    let analysis = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_analysis(
            holdings_source,
            company_data,
            market,
            analyst,
            profile,
            selective,
            outcome_prices,
            resume,
            paths,
            &conn,
            ctx,
        )
    }));
    let panicked = analysis.is_err();
    let analysis = analysis.unwrap_or_else(|payload| {
        Err(anyhow::anyhow!(
            "the analysis panicked: {}",
            panic_payload_message(payload.as_ref())
        ))
    });

    match analysis {
        Ok(run) => {
            let finished_at = now_rfc3339();
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: PORTFOLIO_JOB,
                    state: JobState::Successful,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: None,
                    detail: Some(&run.run_id),
                },
            );
            ctx.run_finished("successful", None, Some(run.run_id.clone()));
            recorded?;
            Ok(PortfolioJobOutcome::Successful(Box::new(run)))
        }
        // The shared flag tells a user stop apart from a failure. A panic is never
        // a user stop: it records `Failed` even with a cancel pending, so the
        // failed-job warning surfaces the crash.
        Err(_) if ctx.is_cancelled() && !panicked => {
            let finished_at = now_rfc3339();
            let detail = "run cancelled by user".to_string();
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: PORTFOLIO_JOB,
                    state: JobState::Cancelled,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: None,
                    detail: Some(&detail),
                },
            );
            ctx.run_finished("cancelled", Some(detail.clone()), None);
            recorded?;
            Ok(PortfolioJobOutcome::Cancelled(detail))
        }
        Err(e) => {
            let finished_at = now_rfc3339();
            // Alternate (chain) format: a context-wrapped failure would otherwise
            // persist only its outermost message, hiding the root cause from
            // `job_runs.detail`.
            let msg = format!("{e:#}");
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: PORTFOLIO_JOB,
                    state: JobState::Failed,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: None,
                    detail: Some(&msg),
                },
            );
            ctx.run_finished("failed", Some(msg.clone()), None);
            recorded?;
            Ok(PortfolioJobOutcome::Failed(msg))
        }
    }
}

/// The analysis half; returns the persisted [`PortfolioRun`]. A cancellation
/// check and the checkpoint write sit between holdings.
#[allow(clippy::too_many_arguments)]
fn run_analysis(
    holdings_source: &dyn HoldingsSource,
    company_data: &dyn CompanyDataSource,
    market: &dyn MarketContextSource,
    analyst: &dyn HoldingAnalyst,
    profile: &InvestorProfile,
    selective: Option<SelectiveRun<'_>>,
    outcome_prices: Option<&dyn crate::portfolio::outcome::OutcomePriceSource>,
    resume: Option<store::Checkpoint>,
    paths: &ReportPaths,
    conn: &Connection,
    ctx: &RunContext,
) -> Result<PortfolioRun> {
    // A resume reopens the interrupted run's pinned pull, the one exception to
    // always-pull-fresh (`docs/portfolio-analysis.md` §Triggering): the checkpoint
    // contract is coherent only against one holdings snapshot.
    let holdings = match &resume {
        Some(cp) => {
            ctx.step_started("holdings", "Reopen pinned holdings pull (resume)");
            let h = cp.header.holdings.clone();
            ctx.step_finished("holdings", "ok", None);
            h
        }
        None => {
            ctx.step_started("holdings", "Pull holdings");
            // Every later step consumes only the normalized rows
            // (`docs/schwab-integration.md` §What is pulled;
            // `docs/portfolio-workflow.md` §Step 2).
            let h = holdings_source.holdings()?.normalized()?;
            ctx.step_finished("holdings", "ok", None);
            h
        }
    };

    // Computed before any model stage (the compute-don't-guess boundary).
    // Fail-soft: an unreadable prior run reads as no prior snapshot, so every
    // position tags `new` as on a first run.
    let prior_run = prior_state_read("prior-run", store::latest_run(conn));
    let prior_run_id = prior_run.as_ref().map(|r| r.run_id.clone());
    let prior_created_at = prior_run.as_ref().map(|r| r.created_at.clone());
    let holdings_diff = diff::diff_holdings(prior_run.as_ref().map(|r| &r.holdings), &holdings);

    // Only a quick-check state swept against the same prior run applies
    // (`docs/portfolio-analysis.md §The quick check`); an unreadable state reads
    // as no quick check since the last pass.
    let quick_state = prior_state_read("quick-check-state", store::latest_quick_check(conn))
        .filter(|s| Some(&s.swept_run_id) == prior_run_id.as_ref());

    // The run's one wall-clock instant, minted before any dated decision, so an
    // hours-long run crossing ET midnight cannot demote on one ET day and render
    // the badge on the next. A resume pins the interrupted run's instant and id.
    let created_at = match &resume {
        Some(cp) => cp.header.created_at.clone(),
        None => now_rfc3339(),
    };
    let run_id = match &resume {
        Some(cp) => cp.header.run_id.clone(),
        None => uuid::Uuid::new_v4().to_string(),
    };
    // ET session, pairing with the ET-dated vintages `over_age` compares against.
    let today = crate::market_clock::et_date_of(&created_at)
        .unwrap_or_else(|| crate::market_clock::et_session_date(chrono::Utc::now()));
    // The one `run_date` every dated stamp in this run uses.
    let run_session_date = today.format("%Y-%m-%d").to_string();

    // Freshness-gated (`docs/portfolio-workflow.md` §Step 5): a stale latest
    // report drops the whole view, the omission riding the run's data health.
    let (house_view, house_view_omitted) = match &resume {
        Some(cp) => (cp.header.house_view.clone(), cp.header.house_view_omitted),
        None => dossier::load_house_view(conn, &paths.reports_dir, today),
    };

    // Hard-fail before any per-holding work (`docs/portfolio-analysis.md`
    // §Failure posture).
    ctx.step_started("rates", "Load rate anchors (FRED)");
    let rates = match &resume {
        // Resumed holdings must compute against the same anchors the completed ones did.
        Some(cp) => {
            let r = cp.header.rates.clone();
            ctx.step_finished("rates", "ok", Some("pinned (resume)".to_string()));
            r
        }
        None => match market.rates() {
            Ok(r) => {
                ctx.step_finished("rates", "ok", None);
                r
            }
            Err(e) => {
                ctx.step_finished("rates", "failed", Some(e.to_string()));
                return Err(e.context("run-level rate-anchor load failed (DGS2/DGS10)"));
            }
        },
    };

    // Fetched once per run, wholly fail-soft (`docs/portfolio-workflow.md` §Step
    // 5); per-series gaps ride the roll-up's data health.
    ctx.step_started("commodities", "Load commodity context (FRED / FMP)");
    let commodities = match &resume {
        Some(cp) => cp.header.commodities.clone(),
        None => market.commodities(today),
    };
    ctx.step_finished(
        "commodities",
        "ok",
        (!commodities.gaps.is_empty())
            .then(|| format!("{} series gap(s)", commodities.gaps.len())),
    );

    ctx.step_started("positioning", "Load CFTC positioning");
    let (cot_rows, cot_gaps) = match &resume {
        Some(cp) => (cp.header.cot_rows.clone(), cp.header.cot_gaps.clone()),
        None => market.positioning(today),
    };
    ctx.step_finished(
        "positioning",
        "ok",
        (!cot_gaps.is_empty()).then(|| format!("{} contract gap(s)", cot_gaps.len())),
    );

    ctx.step_started("sentiment", "Load CBOE put/call backdrop");
    let (put_call_backdrop, cboe_gap) = match &resume {
        Some(cp) => (cp.header.put_call_backdrop.clone(), cp.header.cboe_gap.clone()),
        None => market.put_call_backdrop(),
    };
    ctx.step_finished("sentiment", "ok", cboe_gap.clone());

    ctx.step_started("short-interest", "Load FINRA short interest");
    let (short_interest_file, finra_gap) = match &resume {
        Some(cp) => (
            cp.header.short_interest_file.clone(),
            cp.header.finra_gap.clone(),
        ),
        None => market.short_interest(),
    };
    ctx.step_finished("short-interest", "ok", finra_gap.clone());

    ctx.step_started("mergers", "Load M&A feed");
    let (ma_deals, ma_gap) = match &resume {
        Some(cp) => (cp.header.ma_deals.clone(), cp.header.ma_gap.clone()),
        None => market.mergers_acquisitions(today),
    };
    ctx.step_finished("mergers", "ok", ma_gap.clone());

    // Before any per-holding work, so this run's reviews read every check that
    // has come due (`docs/portfolio-workflow.md` §Step 5). Never pinned: a resume
    // re-runs the pass, the due query excluding horizons already written.
    let accuracy = run_accuracy_pass(
        conn,
        outcome_prices.unwrap_or(&crate::portfolio::outcome::UnavailablePriceSource),
        ctx,
        &run_id,
        today,
        &holdings.positions.iter().map(|p| p.symbol.clone()).collect::<Vec<_>>(),
    );

    // ---- Selective work-list (`docs/portfolio-analysis.md` §Triggering) ------
    // A selective run analyzes strictly the user's selection; the safety signals
    // surface as non-blocking card badges, never force-includes. A current
    // position with no prior verdict is left not analyzed rather than pulled in.
    // `None` = the whole-book run.
    let mut swept_tail: std::collections::HashMap<
        String,
        crate::portfolio::quick_check::HoldingQuickState,
    > = std::collections::HashMap::new();
    let work_list: Option<std::collections::HashSet<String>> = if let Some(cp) = &resume {
        // A resumed selective run keeps its exact selection, and the tail sweep's
        // states ride the header rather than re-spending its retrievals
        // (`docs/portfolio-analysis.md` §Failure posture).
        for h in cp.header.swept_tail.clone() {
            swept_tail.insert(h.symbol.to_ascii_uppercase(), h);
        }
        cp.header
            .work_list
            .as_ref()
            .map(|w| w.iter().cloned().collect())
    } else {
        match (&selective, &prior_run) {
        (Some(sel), Some(prior)) if !sel.selected.is_empty() => {
            let book: std::collections::HashSet<String> = holdings
                .positions
                .iter()
                .map(|p| p.symbol.to_ascii_uppercase())
                .collect();
            let work: std::collections::HashSet<String> = sel
                .selected
                .iter()
                .map(|s| s.to_ascii_uppercase())
                .filter(|s| book.contains(s))
                .collect();
            // The quick check sweeps the carried tail only to persist the attention
            // flags and evidence notes the card badges render, never to expand the
            // work-list. A position with no prior verdict is not swept.
            let prior_symbols: std::collections::HashSet<String> = prior
                .verdicts
                .iter()
                .map(|v| v.symbol.to_ascii_uppercase())
                .collect();
            let tail: std::collections::HashSet<String> = holdings
                .positions
                .iter()
                .map(|p| p.symbol.to_ascii_uppercase())
                .filter(|k| !work.contains(k) && prior_symbols.contains(k))
                .collect();
            let states = crate::portfolio::quick_check::sweep_tail(
                crate::portfolio::quick_check::TailSweep {
                    data: sel.quick_data,
                    prior_run: prior,
                    current_positions: &holdings.positions,
                    tail: &tail,
                    prior_state: quick_state.as_ref(),
                    rates: crate::portfolio::RatePrints {
                        dgs2: rates.dgs2,
                        dgs10: rates.dgs10,
                        dgs2_as_of: rates.dgs2_date.clone(),
                        dgs10_as_of: rates.dgs10_date.clone(),
                        fetched_at: created_at.clone(),
                    },
                    run_instant: &created_at,
                },
                ctx,
            )?;
            for h in states {
                swept_tail.insert(h.symbol.to_ascii_uppercase(), h);
            }
            Some(work)
        }
        _ => None,
        }
    };

    // A fresh run discards any interrupted run's checkpoints and writes its own
    // pinned header; a resume keeps writing under the reopened header. Fail-soft:
    // a checkpoint write never fails a run that can succeed
    // (`docs/portfolio-analysis.md` §Failure posture).
    if resume.is_none() {
        let header = store::CheckpointHeader {
            run_id: run_id.clone(),
            created_at: created_at.clone(),
            prior_run_id: prior_run_id.clone(),
            holdings: holdings.clone(),
            rates: rates.clone(),
            house_view: house_view.clone(),
            house_view_omitted,
            commodities: commodities.clone(),
            cot_rows: cot_rows.clone(),
            cot_gaps: cot_gaps.clone(),
            put_call_backdrop: put_call_backdrop.clone(),
            cboe_gap: cboe_gap.clone(),
            short_interest_file: short_interest_file.clone(),
            finra_gap: finra_gap.clone(),
            ma_deals: ma_deals.clone(),
            ma_gap: ma_gap.clone(),
            work_list: work_list
                .as_ref()
                .map(|w| w.iter().cloned().collect()),
            swept_tail: swept_tail.values().cloned().collect(),
            prompt_version: crate::portfolio::PROMPT_VERSION.to_string(),
            grade_parameter_version: crate::portfolio::engine::GRADE_PARAMETER_VERSION.to_string(),
            target_parameter_version:
                crate::portfolio::engine::SCENARIO_TARGET_PARAMETER_VERSION.to_string(),
            pre_profit_parameter_version:
                crate::portfolio::pre_profit::PRE_PROFIT_PARAMETER_VERSION.to_string(),
            quick_check_parameter_version:
                crate::portfolio::quick_check::QUICK_CHECK_PARAMETER_VERSION.to_string(),
            evidence_floor_version: crate::portfolio::engine::EVIDENCE_FLOOR_VERSION.to_string(),
            checkpoint_format_version: store::CHECKPOINT_FORMAT_VERSION.to_string(),
            model_ids: vec![analyst.reasoner_id(), analyst.fast_id()],
        };
        if let Err(e) =
            store::clear_checkpoints(conn).and_then(|()| store::save_checkpoint_header(conn, &header))
        {
            eprintln!("portfolio checkpoint: header write failed ({e}) — run continues unprotected");
        }
    }

    let mut verdicts: Vec<HoldingVerdict> = Vec::with_capacity(holdings.positions.len());
    let mut audits: Vec<HoldingAudit> = Vec::with_capacity(holdings.positions.len());
    // Per-holding failures the run isolated (`docs/portfolio-analysis.md`
    // §Failure posture); the prior verdict is carried after the loop.
    let mut failed_holdings: Vec<crate::portfolio::HoldingFailure> = Vec::new();
    // The completed holdings a resume restores; their symbols skip the loop.
    let mut checkpointed: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Restored from the trail's rows on resume and extended at each holding's
    // checkpoint boundary, so a resumed run's data-health read spans both
    // processes. Each row carries its own holding's calls, so the telemetry
    // restored is exactly the rows restored (`docs/portfolio-analysis.md`
    // §Failure posture); the health counts are rebuilt from `health_rows` at the
    // roll-up, never seeded from an accumulator.
    let mut prompt_usage: Vec<crate::local_model::PromptUsage> = Vec::new();
    let mut model_retries: Vec<crate::local_model::RetryEvent> = Vec::new();
    let mut health_rows: Vec<store::HoldingHealth> = Vec::new();
    if let Some(cp) = &resume {
        ctx.step_started(
            "resume",
            format!("Resume: restore {} completed holding(s)", cp.holdings.len()),
        );
        for row in &cp.holdings {
            checkpointed.insert(row.verdict.symbol.to_ascii_uppercase());
            verdicts.push(row.verdict.clone());
            audits.push(row.audit.clone());
            prompt_usage.extend(row.prompt_usage.iter().cloned());
            model_retries.extend(row.model_retries.iter().cloned());
            health_rows.push(row.health.clone());
        }
        ctx.step_finished("resume", "ok", None);
    }

    // Seeds the run-level keyed identities from the trail on resume.
    let seeded = resume
        .as_ref()
        .map(|cp| cp.accumulators.clone())
        .unwrap_or_default();

    // Fetched on first need and memoized across funds
    // (`docs/portfolio-workflow.md` §Step 6a).
    let mut sector_pe_cache: Option<Vec<crate::portfolio::fund::SectorPe>> = None;
    // The snapshot's failure is memoized with it so every fund's gaps carry the
    // same reason, not just the fund whose turn issued the fetch.
    let mut sector_pe_gap: Option<String> = None;
    let mut sector_history_cache: std::collections::HashMap<
        String,
        Vec<crate::portfolio::fund::SectorPe>,
    > = std::collections::HashMap::new();
    // A failed history leg is memoized beside the empty rows and replayed to
    // every fund that depends on that sector.
    let mut sector_history_gap_cache: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // One fail-soft profile call per fresh-passed stock; a fund is typed
    // `sector-unscorable` without a call.
    let mut sector_by_symbol: std::collections::HashMap<
        String,
        crate::portfolio::sector::SectorIdentity,
    > = seeded.sector_by_symbol;
    // The profile's issuer name, so the prompt header can name the company when
    // Schwab's description is blank.
    let mut profile_name_by_symbol: std::collections::HashMap<String, Option<String>> =
        seeded.profile_name_by_symbol;
    // The profile's industry label, the commodity context's gold-linkage key.
    let mut industry_by_symbol: std::collections::HashMap<String, Option<String>> =
        seeded.industry_by_symbol;
    // Memoized per SPDR symbol (`docs/portfolio-workflow.md` §Step 5); `None`
    // caches a failed or empty fetch so a broken benchmark costs one request. The
    // second field is whether that fetch degraded, recorded on every reading
    // holding's health row (memo hit or fresh miss alike) and deduplicated by
    // benchmark at the roll-up.
    let mut benchmark_closes: std::collections::HashMap<
        String,
        (Option<Vec<crate::portfolio::engine::DatedValue>>, bool),
    > = std::collections::HashMap::new();

    for position in &holdings.positions {
        // Unselected holdings carry their prior verdict forward after the loop.
        if work_list
            .as_ref()
            .is_some_and(|w| !w.contains(&position.symbol.to_ascii_uppercase()))
        {
            continue;
        }
        if checkpointed.contains(&position.symbol.to_ascii_uppercase()) {
            continue;
        }
        if ctx.is_cancelled() {
            anyhow::bail!("run cancelled");
        }
        let step_key = crate::portfolio::holding_step_key(&position.symbol);
        ctx.step_started(step_key.clone(), format!("Analyze {}", position.symbol));

        // The per-company calls poll cancellation before their requests; a SEC
        // degradation folds into the financials' gap manifest so it reaches the audit.
        let is_fund = matches!(
            position.asset_class,
            crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
        );
        let is_stock = matches!(position.asset_class, crate::portfolio::AssetClass::Stock);
        // One profile fetch per fresh-passed stock feeds both the listing-resolution
        // guard (`docs/portfolio-analysis.md` §Asset eligibility) and the sector
        // identity.
        let mut research_website = None;
        // The FETCHED VALUES profile line (`docs/portfolio-workflow.md` §Step 6c).
        let mut issuer: Option<dossier::IssuerProfile> = None;
        // The profile's beta, set onto the financials after the pull.
        let mut profile_beta: Option<f64> = None;
        let listing = if is_stock {
            let lookup = company_data.profile_identity(&position.symbol);
            if let crate::portfolio::listing::ProfileLookup::Resolved(p) = &lookup {
                research_website = p.website.clone();
                profile_beta = p.beta;
                issuer = Some(dossier::IssuerProfile {
                    exchange: p.exchange.clone(),
                    sector: p.sector.clone(),
                    industry: p.industry.clone(),
                });
            }
            let (sector, name, industry) = match &lookup {
                crate::portfolio::listing::ProfileLookup::Resolved(p) => {
                    (p.sector.clone(), p.company_name.clone(), p.industry.clone())
                }
                _ => (None, None, None),
            };
            profile_name_by_symbol.insert(position.symbol.to_ascii_uppercase(), name);
            industry_by_symbol.insert(position.symbol.to_ascii_uppercase(), industry);
            sector_by_symbol.insert(
                position.symbol.to_ascii_uppercase(),
                crate::portfolio::sector::SectorIdentity::resolve(sector.as_deref()),
            );
            Some(crate::portfolio::listing::resolve_listing(
                &position.symbol,
                &position.description,
                &lookup,
            ))
        } else {
            if is_fund {
                sector_by_symbol.insert(
                    position.symbol.to_ascii_uppercase(),
                    crate::portfolio::sector::SectorIdentity::unscorable(
                        "multi-sector vehicle (fund)",
                    ),
                );
            }
            None
        };
        // A guard-terminal stock skips the remaining retrieval; `analyze_holding`
        // routes on the resolution before touching the (empty) financials.
        let guard_terminal = matches!(
            &listing,
            Some(
                crate::portfolio::listing::ListingResolution::Unresolved
                    | crate::portfolio::listing::ListingResolution::NonUs { .. }
                    | crate::portfolio::listing::ListingResolution::Conflict { .. }
                    | crate::portfolio::listing::ListingResolution::UnsupportedUnits { .. }
            )
        );
        // A non-gradeable class skips the same retrieval: `pipeline::analyze_holding`
        // routes it to `NotRated` with default metrics before the engine stage, so
        // the gate changes only the budget, never an output.
        let skip_retrieval = guard_terminal || !position.asset_class.is_gradeable();
        let mut fmp_financials = if skip_retrieval {
            CompanyFinancials {
                symbol: position.symbol.clone(),
                ..Default::default()
            }
        } else if is_fund {
            company_data.fund_financials(&position.symbol)
        } else {
            company_data.financials(&position.symbol)
        };
        // A fund never hits SEC company facts: its statement lines feed nothing on
        // the reduced path, and the trust entity behind an ETF routinely 404s the
        // facts API.
        let sec_data = if is_fund || skip_retrieval {
            SecData::default()
        } else {
            company_data.facts(&position.symbol)
        };
        fmp_financials.gaps.extend(sec_data.gaps);
        if !skip_retrieval {
            fmp_financials.beta = profile_beta;
        }
        let sec_leg = match &sec_data.facts {
            Some(facts) => dossier::LegOutcome::Got(facts),
            None => dossier::LegOutcome::NotRun,
        };
        // Stocks only: a fund wrapper has no issuer-level filing to classify. An
        // `Unknown` sweep rides the gap manifest as a degraded input; it never
        // trips the hard rule. The same read keeps the 8-K rows for FETCHED
        // VALUES (`docs/portfolio-workflow.md` §Step 6c).
        let (filing_events, filings_8k) = if is_fund || skip_retrieval {
            (None, Vec::new())
        } else {
            let since = (today
                - chrono::Duration::days(crate::portfolio::FORENSIC_EVENT_LOOKBACK_DAYS))
            .format("%Y-%m-%d")
            .to_string();
            match company_data.filing_events(&position.symbol, &since) {
                Some(read) => (Some(read.state), read.filings_8k),
                None => (None, Vec::new()),
            }
        };
        if let Some(crate::portfolio::ForensicFilingState::Unknown { reason, .. }) =
            &filing_events
        {
            fmp_financials
                .gaps
                .push(format!("SEC filings sweep degraded: {reason}"));
        }
        // Stocks only; the gaps join the financials' manifest so they reach the
        // audit and data health.
        let evidence = if is_stock && !skip_retrieval {
            let record = company_data.evidence(&position.symbol, today);
            fmp_financials.gaps.extend(record.gaps.iter().cloned());
            Some(record)
        } else {
            None
        };
        // A local lookup against the run-level feed, never a request.
        let ma_matches = if is_stock && !skip_retrieval {
            dossier::ma_matches_for_holding(&position.symbol, &ma_deals)
        } else {
            Vec::new()
        };
        let (deep_closes, deep_gaps) = if skip_retrieval {
            (vec![], vec![])
        } else {
            company_data.deep_price_history(&position.symbol)
        };
        let deep_history_failed = !deep_gaps.is_empty();
        if !deep_closes.is_empty() {
            fmp_financials.daily_closes = deep_closes;
        }
        fmp_financials.gaps.extend(deep_gaps);

        // The fund half: metadata plus the memoized sector-P/E surface
        // (`docs/portfolio-workflow.md` §Step 6b).
        let fund_ctx = if is_fund {
            let mut fund = company_data.fund_data(&position.symbol);
            if sector_pe_cache.is_none() {
                // Dated on the run's pinned ET session, never a fresh clock read.
                sector_pe_cache = Some(match company_data.sector_pe_snapshot(today) {
                    Ok(rows) => rows,
                    Err(e) => {
                        sector_pe_gap = Some(format!("sector-P/E snapshot unavailable: {e}"));
                        vec![]
                    }
                });
            }
            if let Some(gap) = &sector_pe_gap {
                fund.gaps.push(gap.clone());
            }
            let mut seen_history_sectors = std::collections::HashSet::new();
            for (sector, _) in &fund.sector_weights {
                let key = sector.to_ascii_lowercase();
                if !seen_history_sectors.insert(key.clone()) {
                    continue;
                }
                if !sector_history_cache.contains_key(&key) {
                    let rows = match company_data.sector_pe_history(sector) {
                        Ok(rows) => rows,
                        Err(e) => {
                            sector_history_gap_cache.insert(
                                key.clone(),
                                format!("sector-P/E history unavailable for {sector}: {e}"),
                            );
                            vec![]
                        }
                    };
                    sector_history_cache.insert(key.clone(), rows);
                }
                if let Some(gap) = sector_history_gap_cache.get(&key) {
                    fund.gaps.push(gap.clone());
                }
            }
            // An unmapped fund, or a mapped contract whose row didn't land,
            // fail-softs to no read (`docs/data-sources.md §CFTC`).
            let positioning = crate::portfolio::fund::cot_contract_for_fund(&fund).and_then(
                |code| {
                    cot_rows
                        .iter()
                        .find(|r| r.contract_code == code)
                        .cloned()
                },
            );
            Some(crate::portfolio::fund::FundContext {
                fund,
                sector_pe: sector_pe_cache.clone().unwrap_or_default(),
                sector_pe_history: sector_history_cache.clone(),
                // The ET session, not the UTC date: `as_of` drives
                // `fund::quarter_end_before`, and an evening-ET run under a UTC
                // date would sample a window the feed cannot serve yet.
                as_of: today,
                positioning,
            })
        } else {
            None
        };
        // Fail-soft (`docs/schwab-integration.md §Failure posture`): a failed chain
        // fetch is a recorded gap, never a whole-job failure. `Some(None)` =
        // consulted but empty, `None` = never requested.
        let chain = if skip_retrieval {
            None
        } else {
            match holdings_source.option_chain(&position.symbol) {
                Ok(chain) => Some(chain),
                Err(e) => {
                    fmp_financials
                        .gaps
                        .push(format!("Option chain unavailable for {}: {e}", position.symbol));
                    Some(None)
                }
            }
        };
        let chain_leg = match &chain {
            Some(Some(chain)) => dossier::LegOutcome::Got(chain),
            Some(None) => dossier::LegOutcome::Empty,
            None => dossier::LegOutcome::NotRun,
        };
        let prior = dossier::prior_verdict_for(prior_run.as_ref(), &position.symbol);
        // Preserved on an insufficient-evidence exit below: an abstention is not a
        // full pass, and the evidence-event boundary must not advance past events
        // no pass examined (`docs/portfolio-analysis.md` §Evidence floor).
        let prior_vintage = prior.as_ref().map(|p| {
            crate::portfolio::effective_vintage(&p.verdict, prior_created_at.as_deref().unwrap_or(""))
                .to_string()
        });
        // Fetched only where the pre-flag is evaluable (a carried stock whose
        // sector resolved to a SPDR benchmark); `benchmark_gap` reads `Some` where
        // the memoized fetch degraded, fresh fetch or memo hit alike.
        let mut benchmark_gap: Option<String> = None;
        let sector_benchmark = if is_stock && !guard_terminal && prior.is_some() {
            sector_by_symbol
                .get(&position.symbol.to_ascii_uppercase())
                .and_then(|s| s.benchmark.clone())
                .and_then(|bench| {
                    let (closes, degraded) = benchmark_closes
                        .entry(bench.clone())
                        .or_insert_with(|| {
                            let (closes, gaps) = company_data.deep_price_history(&bench);
                            let degraded = !gaps.is_empty() || closes.is_empty();
                            ((!closes.is_empty()).then_some(closes), degraded)
                        })
                        .clone();
                    if degraded {
                        benchmark_gap = Some(bench.clone());
                    }
                    closes.map(|closes| dossier::BenchmarkSeries {
                        symbol: bench,
                        closes,
                    })
                })
        } else {
            None
        };
        // The same-underlying option overlay (`docs/portfolio-workflow.md` §Step
        // 6a): the targeted delta fetch per held strike is scoped to the held
        // contracts' expiry window so the activity signal's bounded NTM query is
        // never widened; a failed fetch degrades that strike's deltas to typed gaps.
        let option_overlay = if is_stock && !skip_retrieval {
            let key = position.symbol.to_ascii_uppercase();
            let option_rows: Vec<&crate::schwab::Position> = holdings
                .positions
                .iter()
                .filter(|p| {
                    // A zero-net row (kept by normalization) carries no exposure:
                    // never a leg, never a delta-fetch trigger.
                    p.quantity != 0.0
                        && p.asset_class == crate::portfolio::AssetClass::OptionContract
                        && crate::schwab::parse_occ_symbol(&p.symbol)
                            .is_some_and(|c| c.underlying == key)
                })
                .collect();
            if option_rows.is_empty() {
                None
            } else {
                let contracts: Vec<crate::schwab::OccContract> = option_rows
                    .iter()
                    .filter_map(|p| crate::schwab::parse_occ_symbol(&p.symbol))
                    .collect();
                let mut strikes: Vec<f64> = Vec::new();
                for c in &contracts {
                    if !strikes.iter().any(|s| (s - c.strike).abs() < 1e-6) {
                        strikes.push(c.strike);
                    }
                }
                let mut served: Vec<crate::schwab::OptionQuote> = Vec::new();
                // "Consulted" comes from the capability probe, not the answer: the
                // stub default answers `Ok(None)` with no wire call.
                let consulted =
                    holdings_source.supports_targeted_chain() && !strikes.is_empty();
                // Fetch failures ride the overlay's own gap list, which owns the
                // delta legs, not the financials manifest.
                let mut fetch_gaps: Vec<String> = Vec::new();
                for strike in strikes {
                    let at_strike: Vec<&crate::schwab::OccContract> = contracts
                        .iter()
                        .filter(|c| (c.strike - strike).abs() < 1e-6)
                        .collect();
                    let from = at_strike.iter().map(|c| c.expiry.as_str()).min();
                    let to = at_strike.iter().map(|c| c.expiry.as_str()).max();
                    let (Some(from), Some(to)) = (from, to) else {
                        continue;
                    };
                    match holdings_source.option_chain_at_strike(&position.symbol, strike, from, to)
                    {
                        Ok(Some(chain)) => served.extend(chain.contracts),
                        Ok(None) => {}
                        Err(e) => fetch_gaps.push(format!(
                            "delta chain unavailable at strike {strike}: {e}"
                        )),
                    }
                }
                let mut overlay = dossier::assemble_option_overlay(
                    position.quantity,
                    &option_rows,
                    |c: &crate::schwab::OccContract| {
                        served
                            .iter()
                            .find(|q| {
                                q.kind == c.kind
                                    && q.expiry == c.expiry
                                    && (q.strike - c.strike).abs() < 1e-6
                            })
                            .and_then(|q| q.delta)
                    },
                    consulted,
                );
                if let Some(o) = overlay.as_mut() {
                    o.gaps.extend(fetch_gaps);
                }
                overlay
            }
        } else {
            None
        };
        // Stocks only (`docs/data-sources.md §FINRA`). A symbol absent from the
        // file carries no read: a market fact, not a gap.
        let short_interest = if is_stock && !skip_retrieval {
            short_interest_file
                .as_ref()
                .and_then(|f| f.lookup(&position.symbol))
                .cloned()
        } else {
            None
        };
        // News leads (`docs/portfolio-workflow.md` §Step 6a): leads, never
        // evidence. A row served without a URL cannot be deep-read and is dropped.
        let news_leads: Vec<crate::portfolio::research::NewsLead> =
            if is_stock && !skip_retrieval {
                let from = (today
                    - chrono::Duration::days(crate::portfolio::research::RESEARCH_FRESHNESS_DAYS))
                .format("%Y-%m-%d")
                .to_string();
                company_data
                    .news_items(&position.symbol, &from)
                    .into_iter()
                    .filter_map(|n| {
                        let url = n.url?;
                        Some(crate::portfolio::research::NewsLead {
                            headline: n.title,
                            url,
                            source: n.site.unwrap_or_else(|| "fmp-news".to_string()),
                            published: Some(n.published_date),
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };
        let mut dossier: HoldingDossier = dossier::assemble(
            position.clone(),
            holdings_diff.delta_for(&position.symbol),
            fmp_financials,
            // Each leg's consulted / empty / not-run outcome, so the audit can
            // label a consulted-but-empty fetch and skip a leg that never ran.
            sec_leg,
            chain_leg,
            profile.clone(),
            house_view.clone(),
            fund_ctx,
            prior,
            listing,
            profile_name_by_symbol
                .get(&position.symbol.to_ascii_uppercase())
                .cloned()
                .flatten(),
            filing_events,
            short_interest,
            option_overlay,
            put_call_backdrop.clone(),
            // Stocks only: a fund's commodity read is the CFTC positioning leg,
            // not a price block.
            if is_stock {
                dossier::commodity_prints_for_holding(
                    &commodities,
                    sector_by_symbol
                        .get(&position.symbol.to_ascii_uppercase())
                        .and_then(|s| s.sector.as_deref()),
                    industry_by_symbol
                        .get(&position.symbol.to_ascii_uppercase())
                        .and_then(|i| i.as_deref()),
                )
            } else {
                Vec::new()
            },
            sector_benchmark,
            news_leads,
            run_session_date.clone(),
            dossier::StockEvidenceLegs {
                issuer,
                evidence,
                filings_8k,
                ma_matches,
            },
        );
        // The checks ran before the loop, so this run's are in it
        // (`docs/portfolio-workflow.md` §Step 6e).
        dossier.accuracy = accuracy.subject(&position.symbol);
        if is_stock && !skip_retrieval {
            dossier.earnings_issuer = research_website.as_deref().and_then(|website| {
                crate::sec::earnings::Issuer::new(
                    &position.symbol,
                    &company_data.research_cik(&position.symbol)?,
                    website,
                )
            });
        }

        // A cancel mid-gather is observed before any model call is spent.
        if ctx.is_cancelled() {
            anyhow::bail!("run cancelled");
        }

        // The model/grade half fails hard per holding, not per run
        // (`docs/portfolio-analysis.md` §Failure posture); the run fails outright
        // only when every attempted holding fails (the post-loop guard). The run
        // date is the run's ET session date from its one instant, never re-derived
        // per holding, or a midnight-crossing run dates one book across two days.
        let run_date = run_session_date.clone();
        let (mut verdict, audit) = match analyze_holding(analyst, &dossier, &rates, &run_date) {
            Ok(pair) => pair,
            Err(e) => {
                // A cancel mid-holding is not a holding failure: re-propagate so
                // the run records Cancelled, never a failed card.
                if ctx.is_cancelled() {
                    return Err(e.context("run cancelled"));
                }
                // Isolated: record the failure and move on; the prior verdict is
                // carried after the loop. `cause` is the concise card line (stage +
                // root cause); the full chain reaches the tracker's failed step and
                // stderr.
                let outer = e.to_string();
                let root = e.root_cause().to_string();
                let cause = if root == outer {
                    outer
                } else {
                    format!("{outer}: {root}")
                };
                let chain = format!("{e:#}");
                eprintln!("portfolio: {} analysis failed — {chain}", position.symbol);
                // Drain the analyst so a partial usage / retry cannot bleed into the
                // next holding's row. Keep the prompt-fit usage (a real measurement)
                // but drop the retry events: `model_retries` carries the "every
                // listed retry succeeded" contract, and a failed holding's terminal
                // retry did not succeed.
                prompt_usage.extend(analyst.take_prompt_usage());
                let _dropped_failed_retries = analyst.take_retry_events();
                let carried_prior = prior_run.as_ref().is_some_and(|p| {
                    p.verdicts
                        .iter()
                        .any(|v| v.symbol.eq_ignore_ascii_case(&position.symbol))
                });
                ctx.step_finished(step_key, "failed", Some(chain));
                failed_holdings.push(crate::portfolio::HoldingFailure {
                    symbol: position.symbol.clone(),
                    cause,
                    carried_prior,
                });
                continue;
            }
        };
        if matches!(
            verdict.disposition,
            crate::portfolio::VerdictDisposition::InsufficientEvidence { .. }
        ) {
            verdict.analyzed_at = prior_vintage;
        }
        ctx.step_finished(step_key, "ok", None);
        verdicts.push(verdict);
        audits.push(audit);

        // Only a fresh pass contributes to the factor-distribution store
        // (`docs/storage.md §Local Analysis Suite Storage`); fail-soft, since
        // Portfolio never reads it (`docs/portfolio-analysis.md` §Failure posture).
        if let Some(composite) = audits.last().and_then(|a| a.factors.as_ref()) {
            if let Err(e) = crate::portfolio::factor_store::contribute(
                conn,
                &position.symbol,
                dossier.issuer.as_ref().and_then(|i| i.sector.as_deref()),
                dossier.financials.market_cap,
                composite,
                today,
            ) {
                eprintln!(
                    "factor-distribution store: write failed for {} ({e}) — run continues",
                    position.symbol
                );
            }
        }

        // Mid-run checkpoint (`docs/portfolio-analysis.md` §Failure posture);
        // fail-soft, losing one never fails a run that can succeed. Copy the
        // holding's calls and health row into the run-level vectors before the
        // fail-soft write, so a lost checkpoint never loses an in-process
        // observation.
        let holding_usage = analyst.take_prompt_usage();
        let holding_retries = analyst.take_retry_events();
        prompt_usage.extend(holding_usage.iter().cloned());
        model_retries.extend(holding_retries.iter().cloned());
        let health = store::HoldingHealth {
            deep_history_failed,
            benchmark_gap,
        };
        health_rows.push(health.clone());
        let cp_row = store::CheckpointHolding {
            verdict: verdicts.last().expect("just pushed").clone(),
            audit: audits.last().expect("just pushed").clone(),
            prompt_usage: holding_usage,
            model_retries: holding_retries,
            health,
        };
        if let Err(e) = store::save_checkpoint_progress(
            conn,
            &run_id,
            &position.symbol,
            &cp_row,
            &store::CheckpointAccumulators {
                sector_by_symbol: sector_by_symbol.clone(),
                industry_by_symbol: industry_by_symbol.clone(),
                profile_name_by_symbol: profile_name_by_symbol.clone(),
            },
        ) {
            eprintln!(
                "portfolio checkpoint: write failed for {} ({e})",
                position.symbol
            );
        }
    }

    // Every attempted holding failing is a systemic cause, so the run fails
    // outright rather than persisting an all-failed snapshot that would demote
    // the prior good run (`docs/portfolio-analysis.md` §Failure posture).
    // `verdicts` holds only fresh passes here; carried verdicts are appended below.
    if verdicts.is_empty() && !failed_holdings.is_empty() {
        let first = &failed_holdings[0];
        anyhow::bail!(
            "every attempted holding failed ({}); first: {} — {}",
            failed_holdings.len(),
            first.symbol,
            first.cause
        );
    }

    // A fresh pass stamps the run's `created_at` so it is distinguishable from a
    // carried verdict (`docs/portfolio-analysis.md` §Triggering); an abstention
    // keeps its preserved prior vintage.
    for v in &mut verdicts {
        if !matches!(
            v.disposition,
            crate::portfolio::VerdictDisposition::InsufficientEvidence { .. }
        ) {
            v.analyzed_at = Some(created_at.clone());
        }
    }

    // ---- Carried verdicts (a selective run's unselected tail) ----------------
    // Each carries its prior verdict forward vintage-stamped
    // (`docs/portfolio-analysis.md` §Triggering) with its prior audit row whole:
    // the stored `quick_basis` / `fund_exposure` comparators must survive the
    // carry or the next sweep reads the holding `unknown`.
    let mut carried_symbols: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut over_age_carried: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let (Some(work), Some(prior)) = (&work_list, &prior_run) {
        for position in &holdings.positions {
            let key = position.symbol.to_ascii_uppercase();
            if work.contains(&key) {
                continue;
            }
            let Some(prior_verdict) = prior
                .verdicts
                .iter()
                .find(|v| v.symbol.eq_ignore_ascii_case(&position.symbol))
            else {
                // An unselected holding with no prior verdict is left not
                // analyzed; a full run or an explicit selection grades it.
                continue;
            };
            let (carried, stale) = carry_prior_verdict(
                position,
                prior_verdict,
                &prior.created_at,
                &holdings_diff,
                today,
            );
            if stale {
                over_age_carried.insert(key.clone());
            }
            if let Some(prior_audit) = prior
                .audit
                .iter()
                .find(|a| a.symbol.eq_ignore_ascii_case(&position.symbol))
            {
                audits.push(prior_audit.clone());
            }
            carried_symbols.insert(key);
            verdicts.push(carried);
        }
    }

    // ---- Carried verdicts for isolated per-holding failures -------------------
    // A failed holding carries its prior verdict like an unselected carry, so the
    // failed card shows the last good data (`docs/portfolio-analysis.md` §Failure
    // posture). Runs after the fresh-pass vintage stamp so the carried vintage is
    // preserved; disjoint from the selective tail, since a failed holding was
    // attempted.
    if let Some(prior) = &prior_run {
        for failure in &failed_holdings {
            if !failure.carried_prior {
                continue;
            }
            let key = failure.symbol.to_ascii_uppercase();
            let (Some(position), Some(prior_verdict)) = (
                holdings
                    .positions
                    .iter()
                    .find(|p| p.symbol.eq_ignore_ascii_case(&failure.symbol)),
                prior
                    .verdicts
                    .iter()
                    .find(|v| v.symbol.eq_ignore_ascii_case(&failure.symbol)),
            ) else {
                continue;
            };
            let (carried, stale) = carry_prior_verdict(
                position,
                prior_verdict,
                &prior.created_at,
                &holdings_diff,
                today,
            );
            if stale {
                over_age_carried.insert(key.clone());
            }
            if let Some(prior_audit) = prior
                .audit
                .iter()
                .find(|a| a.symbol.eq_ignore_ascii_case(&failure.symbol))
            {
                audits.push(prior_audit.clone());
            }
            carried_symbols.insert(key);
            verdicts.push(carried);
        }
    }

    // ---- Roll-up ---------------------------------------------------------------
    if ctx.is_cancelled() {
        anyhow::bail!("run cancelled");
    }
    // Anything recorded past the last checkpoint boundary, so every observation
    // reaches the read.
    prompt_usage.extend(analyst.take_prompt_usage());
    model_retries.extend(analyst.take_retry_events());
    // Rebuilt from every completed holding's row, restored and this process's
    // alike, so a resume counts each once.
    let health = health_counts(&health_rows);
    let roll_up = build_roll_up(
        &holdings,
        &verdicts,
        &holdings_diff.exited,
        &audits,
        &carried_symbols,
        health.deep_history_failures,
        rates.history_gap.is_some(),
        house_view_omitted,
        FeedGaps {
            commodity: commodities.gaps.len(),
            positioning: cot_gaps.len(),
            cboe: cboe_gap.is_some(),
            finra: finra_gap.is_some(),
            ma: ma_gap.is_some(),
            accuracy: accuracy.gap,
            benchmark: health.benchmark_gaps.len(),
        },
        prompt_usage,
        model_retries,
        failed_holdings.len(),
    );
    let mut run = PortfolioRun {
        run_id,
        created_at: created_at.clone(),
        holdings,
        verdicts,
        roll_up,
        audit: audits,
        // The rate cache the engine-only quick paths read fail-soft
        // (`docs/portfolio-analysis.md` §The quick check).
        rate_prints: crate::portfolio::RatePrints {
            dgs2: rates.dgs2,
            dgs10: rates.dgs10,
            dgs2_as_of: rates.dgs2_date.clone(),
            dgs10_as_of: rates.dgs10_date.clone(),
            fetched_at: created_at.clone(),
        },
        failed_holdings,
        accuracy: crate::portfolio::outcome::AccuracyRecord {
            scores: accuracy.scores,
            opened: Vec::new(),
            checks: accuracy.checks,
        },
    };

    ctx.step_started("persist", "Persist run");
    // One transaction: a run persists with its episodes or with neither
    // (`docs/portfolio-workflow.md` §Step 7).
    let tx = conn.unchecked_transaction()?;
    let latest_created_on = store::latest_episode_dates(&tx)?;
    for record in episodes_to_open(
        &run.verdicts,
        &run.audit,
        &created_at,
        today,
        &latest_created_on,
    ) {
        let opened = store::insert_episode(&tx, &record)?;
        run.accuracy.opened.push(opened);
    }
    store::insert_run(&tx, &run)?;
    store::prune_runs(&tx, crate::portfolio::PORTFOLIO_RUN_RETENTION)?;
    tx.commit()?;

    // Clearing the quick-check state is per successful pass, never wholesale: an
    // abstaining or carried holding got no full pass, so its state is retained
    // re-stamped to the new run for the next sweep to chain from
    // (`docs/portfolio-analysis.md §The quick check`, §Evidence floor,
    // §Triggering). Fail-soft: the run row committed above, so an error here must
    // not record a durably persisted run as Failed.
    let retention: anyhow::Result<()> = (|| {
        let store_state = store::latest_quick_check(conn)?;
        let mut retained_holdings: Vec<crate::portfolio::quick_check::HoldingQuickState> =
            Vec::new();
        for v in &run.verdicts {
            let key = v.symbol.to_ascii_uppercase();
            let abstained = matches!(
                v.disposition,
                crate::portfolio::VerdictDisposition::InsufficientEvidence { .. }
            );
            if !abstained && !carried_symbols.contains(&key) {
                continue;
            }
            // The in-run sweep's merged state is the freshest; the store row covers
            // a holding the sweep did not.
            if let Some(h) = swept_tail.get(&key) {
                retained_holdings.push(h.clone());
            } else if let Some(h) = store_state.as_ref().and_then(|s| {
                s.holdings
                    .iter()
                    .find(|h| h.symbol.eq_ignore_ascii_case(&v.symbol))
            }) {
                retained_holdings.push(h.clone());
            }
        }
        if retained_holdings.is_empty() {
            store::clear_quick_check(conn)?;
        } else {
            let state = crate::portfolio::quick_check::QuickCheckState {
                parameter_version:
                    crate::portfolio::quick_check::QUICK_CHECK_PARAMETER_VERSION.to_string(),
                swept_run_id: run.run_id.clone(),
                // The in-run tail sweep is itself a quick-check evaluation; with none
                // (a full run retaining an abstention) the store's own timestamp holds.
                last_checked_at: if swept_tail.is_empty() {
                    store_state
                        .as_ref()
                        .map(|s| s.last_checked_at.clone())
                        .unwrap_or_else(|| created_at.clone())
                } else {
                    created_at.clone()
                },
                // The retained states predate this run, so their rate cache must not
                // shadow the fresher prints just fetched.
                rate_cache: Some(run.rate_prints.clone()),
                holdings: retained_holdings,
            };
            store::save_quick_check(conn, &state)?;
        }
        Ok(())
    })();
    if let Err(e) = retention {
        eprintln!("quick-check retention after run persist failed (run kept): {e}");
    }
    // The run persisted whole, so the trail has served its purpose; a leftover
    // trail is caught by resume validation, never trusted.
    if let Err(e) = store::clear_checkpoints(conn) {
        eprintln!("portfolio checkpoint: clear after run persist failed (run kept): {e}");
    }
    ctx.step_finished("persist", "ok", None);

    Ok(run)
}

/// The accuracy pass's product. `scores` is `None` when the episode store could
/// not be read; `checks` are this run's by run id, so a resumed run's list spans
/// both processes; `gap` is whether a store read or write failed; `store` is the
/// store as the pass left it, which each holding's self-review reads its slice of.
struct AccuracyPass {
    scores: Option<std::collections::BTreeMap<String, crate::portfolio::outcome::AccuracyScores>>,
    checks: Vec<crate::portfolio::outcome::StoredCheck>,
    gap: bool,
    store: Option<(
        Vec<crate::portfolio::outcome::StoredEpisode>,
        Vec<crate::portfolio::outcome::StoredCheck>,
    )>,
}

impl AccuracyPass {
    /// `None` where the store could not be read this run
    /// (`docs/portfolio-workflow.md` §Step 6e).
    fn subject(&self, symbol: &str) -> Option<crate::portfolio::outcome::SubjectAccuracy> {
        self.store
            .as_ref()
            .map(|(episodes, checks)| crate::portfolio::outcome::subject_accuracy(episodes, checks, symbol))
    }
}

/// Calendar-day pad below the earliest anchor bar a refresh must serve, so the
/// anchor never sits exactly on the fetch boundary.
const ACCURACY_FETCH_PAD_DAYS: i64 = 7;

/// The accuracy checks over the episode store (`docs/portfolio-workflow.md`
/// §Step 5; `docs/portfolio-analysis.md` §Outcome learning). A failed refresh
/// leaves its horizons pending for a later run; a store read or write error is
/// fail-soft, logged and counted on data health.
fn run_accuracy_pass(
    conn: &Connection,
    prices: &dyn crate::portfolio::outcome::OutcomePriceSource,
    ctx: &RunContext,
    run_id: &str,
    session: chrono::NaiveDate,
    symbols: &[String],
) -> AccuracyPass {
    use crate::portfolio::outcome::{self, Check, CheckOutcome, Horizon, StoredCheck};
    ctx.step_started("accuracy", "Accuracy checks");
    let mut store_state = match store::load_episode_store(conn) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("accuracy pass: the episode store could not be read ({e}) — scores unavailable this run");
            ctx.step_finished(
                "accuracy",
                "ok",
                Some("episode store unreadable — scores unavailable".to_string()),
            );
            return AccuracyPass {
                scores: None,
                checks: Vec::new(),
                gap: true,
                store: None,
            };
        }
    };
    let checked_on = session.format("%Y-%m-%d").to_string();
    let due = outcome::due_horizons(&store_state.episodes, &store_state.written, session);
    let records: std::collections::HashMap<i64, outcome::PriceRecord> = store_state
        .episodes
        .iter()
        .map(|e| (e.id, e.record.clone()))
        .collect();
    let mut gap = false;
    let (mut unscorable, mut pending) = (0usize, 0usize);
    let mut written: Vec<StoredCheck> = Vec::new();
    let mut write = |episode_id: i64, horizon: Horizon, found: CheckOutcome, gap: &mut bool| {
        let check = Check {
            horizon,
            checked_on: checked_on.clone(),
            run_id: run_id.to_string(),
            outcome: found,
        };
        match store::insert_check(conn, episode_id, &check) {
            Ok(stored) => {
                written.push(stored);
                true
            }
            Err(e) => {
                eprintln!(
                    "accuracy pass: check write failed for episode {episode_id} {} ({e}) — left pending",
                    horizon.key()
                );
                *gap = true;
                false
            }
        }
    };

    // The horizons no series can rescue are written at once, spending no pull.
    let mut needs_series: std::collections::BTreeMap<String, Vec<(i64, Horizon)>> =
        std::collections::BTreeMap::new();
    for (id, horizon) in due {
        let record = &records[&id];
        match outcome::unscorable_without_series(record, horizon) {
            Some(cause) => {
                if write(id, horizon, CheckOutcome::Unscorable { cause }, &mut gap) {
                    unscorable += 1;
                } else {
                    pending += 1;
                }
            }
            None => needs_series
                .entry(record.symbol.to_ascii_uppercase())
                .or_default()
                .push((id, horizon)),
        }
    }

    // One refresh per symbol with a due horizon, held or not: the record measures
    // the forecast, not the book. The anchor bar and the horizon close read from
    // that one fetch so they share a basis.
    for (symbol, items) in needs_series {
        let earliest_anchor = items
            .iter()
            .filter_map(|(id, _)| records[id].anchor.as_ref())
            .filter_map(|a| outcome::parse_date(&a.date))
            .min()
            .unwrap_or(session);
        let from = earliest_anchor - chrono::Duration::days(ACCURACY_FETCH_PAD_DAYS);
        let closes = match prices.daily_closes(&symbol, from, session) {
            Ok(closes) => closes,
            Err(e) => {
                eprintln!("accuracy pass: refresh failed for {symbol} ({e}) — left pending");
                pending += items.len();
                continue;
            }
        };
        if let Err(e) = store::merge_price_bars(conn, &symbol, &closes) {
            eprintln!("accuracy pass: price-bar cache merge failed for {symbol} ({e})");
            gap = true;
        }
        for (id, horizon) in items {
            let found = outcome::check_horizon(&records[&id], horizon, &closes);
            let is_unscorable = matches!(found, CheckOutcome::Unscorable { .. });
            if write(id, horizon, found, &mut gap) {
                if is_unscorable {
                    unscorable += 1;
                }
            } else {
                pending += 1;
            }
        }
    }

    let detail = (!written.is_empty() || pending > 0).then(|| {
        format!(
            "{} check{} written ({unscorable} unscorable), {pending} pending",
            written.len(),
            if written.len() == 1 { "" } else { "s" }
        )
    });
    store_state.checks.extend(written);
    let scores = outcome::scores_by_symbol(
        &store_state.episodes,
        &store_state.checks,
        symbols.iter().cloned(),
    );
    ctx.step_finished("accuracy", "ok", detail);
    AccuracyPass {
        scores: Some(scores),
        checks: store_state
            .checks
            .iter()
            .filter(|c| c.check.run_id == run_id)
            .cloned()
            .collect(),
        gap,
        store: Some((store_state.episodes, store_state.checks)),
    }
}

/// The price records this run opens (`docs/portfolio-workflow.md` §Step 7): one
/// per priced holding analyzed this run whose appendix stated at least one price
/// and whose cadence allows it. A role-risk-only, carried, abstained or not-rated
/// verdict opens none.
fn episodes_to_open(
    verdicts: &[HoldingVerdict],
    audits: &[HoldingAudit],
    created_at: &str,
    session: chrono::NaiveDate,
    latest_created_on: &std::collections::HashMap<String, String>,
) -> Vec<crate::portfolio::outcome::PriceRecord> {
    use crate::portfolio::outcome::{cadence_opens, HorizonPrices, PriceRecord};
    let mut out = Vec::new();
    for v in verdicts {
        if v.analyzed_at.as_deref() != Some(created_at) {
            continue;
        }
        let crate::portfolio::VerdictDisposition::Priced(g) = &v.disposition else {
            continue;
        };
        let model = HorizonPrices {
            three_month: g.appendix.expected_price_3m,
            twelve_month: g.appendix.expected_price_12m,
            three_year: g.appendix.expected_price_3y,
        };
        if !model.any() {
            continue;
        }
        let key = v.symbol.to_ascii_uppercase();
        if !cadence_opens(latest_created_on.get(&key).map(String::as_str), session) {
            continue;
        }
        let Some(audit) = audits.iter().find(|a| a.symbol.eq_ignore_ascii_case(&v.symbol)) else {
            continue;
        };
        let Some(spot) = audit.quick_basis.as_ref().map(|b| b.spot) else {
            eprintln!("accuracy record: {} has no run-time spot on its audit — no episode opened", v.symbol);
            continue;
        };
        out.push(PriceRecord {
            symbol: key,
            created_on: session.format("%Y-%m-%d").to_string(),
            spot,
            anchor: audit.authoring_close.clone(),
            model,
            engine: HorizonPrices {
                three_month: g.price_targets.three_month.as_ref().map(|t| t.base),
                twelve_month: g.price_targets.twelve_month.as_ref().map(|t| t.base),
                three_year: g.price_targets.three_year.as_ref().map(|t| t.base),
            },
        });
    }
    out
}

/// Enriching-feed gap counts feeding data health: counted, never attention, since
/// every feed here is fail-soft and additive (`docs/portfolio-analysis.md`
/// §Failure posture).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FeedGaps {
    pub commodity: usize,
    pub positioning: usize,
    pub cboe: bool,
    pub finra: bool,
    /// The M&A feed walk failed or truncated.
    pub ma: bool,
    /// The accuracy pass hit an episode-store read or write error.
    pub accuracy: bool,
    pub benchmark: usize,
}

/// The run-level data-health counts, rebuilt from every completed holding's row
/// rather than accumulated, so a resumed run counts a re-analyzed holding once
/// and a benchmark failing in both processes once.
struct HealthCounts {
    deep_history_failures: usize,
    /// Distinct, sorted by benchmark symbol.
    benchmark_gaps: Vec<String>,
}

fn health_counts(rows: &[store::HoldingHealth]) -> HealthCounts {
    let deep_history_failures = rows.iter().filter(|h| h.deep_history_failed).count();
    let benchmark_gaps: std::collections::BTreeSet<String> =
        rows.iter().filter_map(|h| h.benchmark_gap.clone()).collect();
    HealthCounts {
        deep_history_failures,
        benchmark_gaps: benchmark_gaps.into_iter().collect(),
    }
}

/// The deterministic portfolio roll-up (`docs/portfolio-analysis.md` §Portfolio
/// roll-up), descriptive only.
#[allow(clippy::too_many_arguments)]
fn build_roll_up(
    holdings: &Holdings,
    verdicts: &[HoldingVerdict],
    exited: &[ExitedPosition],
    audits: &[HoldingAudit],
    carried: &std::collections::HashSet<String>,
    deep_history_failures: usize,
    dgs10_history_gap: bool,
    house_view_omitted: bool,
    feed_gaps: FeedGaps,
    prompt_usage: Vec<crate::local_model::PromptUsage>,
    model_retries: Vec<crate::local_model::RetryEvent>,
    failed_count: usize,
) -> PortfolioRollUp {
    use crate::portfolio::VerdictDisposition;
    let mut graded = 0;
    let mut role_risk = 0;
    let mut not_rated = 0;
    let mut insufficient = 0;
    for v in verdicts {
        match v.disposition {
            VerdictDisposition::Priced(_) => graded += 1,
            VerdictDisposition::RoleRiskOnly(_) => role_risk += 1,
            VerdictDisposition::NotRated { .. } => not_rated += 1,
            VerdictDisposition::InsufficientEvidence { .. } => insufficient += 1,
        }
    }
    // Both weights persist as required floats, and a subnormal total overflows
    // the division; a weight the arithmetic cannot finish reads 0, the same as
    // no total.
    let total = holdings.account_total;
    let usable_total = total.is_finite() && total > 0.0;
    let top_position_weight = if usable_total {
        holdings
            .positions
            .iter()
            .map(|p| p.market_value / total)
            .filter(|w| w.is_finite())
            .fold(0.0_f64, f64::max)
    } else {
        0.0
    };
    let cash_weight = usable_total
        .then(|| holdings.cash / total)
        .filter(|w| w.is_finite())
        .unwrap_or(0.0);

    let exited_note = if exited.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = exited.iter().map(|e| e.symbol.as_str()).collect();
        format!(" Closed since last run: {}.", names.join(", "))
    };

    let role_note = if role_risk > 0 {
        format!(", {role_risk} role/risk-only")
    } else {
        String::new()
    };
    PortfolioRollUp {
        graded_count: graded,
        not_rated_count: not_rated,
        insufficient_evidence_count: insufficient,
        role_risk_only_count: role_risk,
        failed_count,
        top_position_weight,
        cash_weight,
        exited: exited.to_vec(),
        data_health: build_data_health(
            audits,
            carried,
            deep_history_failures,
            dgs10_history_gap,
            house_view_omitted,
            feed_gaps,
            prompt_usage,
            model_retries,
        ),
        overview: format!(
            "{graded} graded{role_note}, {not_rated} not rated, {insufficient} \
             insufficient-evidence; top position {:.0}% of the account, cash {:.0}%.{exited_note}",
            top_position_weight * 100.0,
            cash_weight * 100.0
        ),
    }
}

/// Aggregates the run-level data-health read from the audits' typed fields, never
/// string matching on gap notes. `attention` marks infrastructure degradation
/// only; raw-percentile fallback and research degradation are honest fail-soft
/// states, counted but never attention triggers.
#[allow(clippy::too_many_arguments)] // the run-level inputs the roll-up folds, each one source's outcome
fn build_data_health(
    audits: &[HoldingAudit],
    carried: &std::collections::HashSet<String>,
    deep_history_failures: usize,
    dgs10_history_gap: bool,
    house_view_omitted: bool,
    feed_gaps: FeedGaps,
    prompt_usage: Vec<crate::local_model::PromptUsage>,
    model_retries: Vec<crate::local_model::RetryEvent>,
) -> crate::portfolio::DataHealth {
    let metas: Vec<&crate::portfolio::engine::TargetMeta> =
        audits.iter().filter_map(|a| a.target_meta.as_ref()).collect();
    let targets_total = metas.len();
    let rate_anchored = metas.iter().filter(|m| m.rate_anchored).count();
    let carry = metas.iter().filter(|m| m.current_multiple_carry).count();
    let raw_fallback = targets_total - rate_anchored - carry;
    let floored = metas.iter().filter(|m| m.dispersion_floor_applied).count();
    // A carried holding's prior row rides the run whole, so its prior retrieval
    // outcomes stay out of this run's research counts
    // (`docs/portfolio-analysis.md` §Portfolio roll-up).
    let (research_degraded_holdings, research_gap_count) = audits
        .iter()
        .filter(|audit| !carried.contains(&audit.symbol.to_ascii_uppercase()))
        .filter_map(|audit| audit.research.as_ref())
        .map(|research| research.gaps.len())
        .fold((0usize, 0usize), |(holdings, gaps), count| {
            (
                holdings.saturating_add(usize::from(count > 0)),
                gaps.saturating_add(count),
            )
        });

    let mut parts: Vec<String> = Vec::new();
    if targets_total > 0 {
        parts.push(format!(
            "{rate_anchored} of {targets_total} targets rate-anchored ({raw_fallback} \
             raw-percentile, {carry} multiple-carry)"
        ));
    }
    if dgs10_history_gap {
        parts.push("DGS10 anchor history failed run-wide".to_string());
    }
    if deep_history_failures > 0 {
        parts.push(format!(
            "deep price history failed on {deep_history_failures} holdings"
        ));
    }
    if floored > 0 {
        parts.push(format!("dispersion floor widened {floored} target bands"));
    }
    if house_view_omitted {
        parts.push(format!(
            "house view omitted (latest report older than {} days)",
            crate::portfolio::dossier::HOUSE_VIEW_MAX_AGE_DAYS
        ));
    }
    if feed_gaps.commodity > 0 {
        parts.push(format!("commodity context: {} series gap(s)", feed_gaps.commodity));
    }
    if feed_gaps.positioning > 0 {
        parts.push(format!(
            "CFTC positioning: {} contract gap(s)",
            feed_gaps.positioning
        ));
    }
    if feed_gaps.cboe {
        parts.push("CBOE put/call backdrop unavailable".to_string());
    }
    if feed_gaps.finra {
        parts.push("FINRA short interest unavailable".to_string());
    }
    if feed_gaps.ma {
        parts.push("M&A feed unavailable or truncated".to_string());
    }
    if feed_gaps.accuracy {
        parts.push("accuracy record degraded by a store error".to_string());
    }
    if research_gap_count > 0 {
        parts.push(format!(
            "research coverage degraded on {research_degraded_holdings} holding{} \
             ({research_gap_count} recorded gap{})",
            if research_degraded_holdings == 1 { "" } else { "s" },
            if research_gap_count == 1 { "" } else { "s" }
        ));
    }
    if feed_gaps.benchmark > 0 {
        parts.push(format!(
            "sector benchmark series failed on {} symbol(s)",
            feed_gaps.benchmark
        ));
    }

    // Two context-fit triggers (`docs/portfolio-analysis.md` §Portfolio roll-up):
    // near-full, a prompt at or past the pressure fraction of its `num_ctx`; and
    // likely front-truncation, a reported count too small to cover the chars
    // sent (Ollama's count is post-truncation, so the fill fraction alone cannot
    // see it). A daemon-omitted `prompt_eval_count` reads as `None`: fill 0 and
    // truncation false, so a count-less row never enters a context-fit line.
    let fill = |u: &crate::local_model::PromptUsage| {
        u.api.prompt_eval_count.unwrap_or(0) as f64 / u.num_ctx as f64
    };
    let truncated = |u: &crate::local_model::PromptUsage| {
        !u.phase_limited()
            && u.api.prompt_eval_count.is_some_and(|t| {
                t.saturating_mul(crate::portfolio::TRUNCATION_CHARS_PER_TOKEN) < u.prompt_chars
            })
    };
    let peak_prompt = prompt_usage
        .iter()
        .filter(|u| u.num_ctx > 0 && u.api.prompt_eval_count.is_some())
        .max_by(|a, b| fill(a).total_cmp(&fill(b)))
        .cloned();
    // A length stop already failed its call typed; the observation lands here so
    // the run-level surface names it too.
    let output_limited: Vec<crate::local_model::PromptUsage> = prompt_usage
        .iter()
        .filter(|u| u.output_limited)
        .cloned()
        .collect();
    let context_pressure: Vec<crate::local_model::PromptUsage> = prompt_usage
        .iter()
        .filter(|u| {
            u.num_ctx > 0
                && (fill(u) >= crate::portfolio::CONTEXT_PRESSURE_FRACTION || truncated(u))
        })
        .cloned()
        .collect();
    if let Some(worst) = output_limited
        .iter()
        .max_by_key(|u| u.api.eval_count.unwrap_or(0))
    {
        // `done_reason: "length"` covers two stops with different levers (the
        // call's output reservation, or the shared context filling first), so the
        // line says which the worst row's counts show, through the same predicate
        // the per-call typed failure used.
        let reading = crate::local_model::observed_length_stop_reading(
            worst.api.eval_count,
            worst.num_predict,
            worst.phase_limited(),
        );
        parts.push(format!(
            "generation length-stopped on {} local call{} (largest returned eval_count: {} API eval_count {} of {} \
             reserved — {})",
            output_limited.len(),
            if output_limited.len() == 1 { "" } else { "s" },
            worst.stage,
            worst
                .api
                .eval_count
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unreported".into()),
            worst
                .num_predict
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unset".into()),
            match reading {
                crate::local_model::LengthStopReading::AtReservation => {
                    "at the output reservation"
                }
                crate::local_model::LengthStopReading::UnderReservation => {
                    "under it; context exhaustion suspected"
                }
                crate::local_model::LengthStopReading::Unattributed => {
                    "counts incomplete or phase-limited; stop unattributed"
                }
            },
        ));
    }
    let truncation_suspects: Vec<&crate::local_model::PromptUsage> =
        context_pressure.iter().filter(|u| truncated(u)).collect();
    if let Some(worst) = truncation_suspects
        .iter()
        .max_by(|a, b| a.prompt_chars.cmp(&b.prompt_chars))
    {
        parts.push(format!(
            "likely front-truncation on {} local call{} (worst: {} API prompt_eval_count {} tokens for a \
             {}-char prompt, num_ctx {})",
            truncation_suspects.len(),
            if truncation_suspects.len() == 1 { "" } else { "s" },
            worst.stage,
            // `truncated` requires a reported count, so this is always Some.
            worst.api.prompt_eval_count.unwrap_or(0),
            worst.prompt_chars,
            worst.num_ctx
        ));
    }
    // Every listed retry succeeded (a failed holding's retry events are dropped
    // as it is isolated), so the line measures the absorbed transient rate
    // (`docs/local-models.md §The local-model adapter seam`).
    if let Some(first) = model_retries.first() {
        // The cause carries the full error chain, so the summary shows a capped
        // head; the persisted event keeps the whole chain.
        const SUMMARY_CAUSE_CAP: usize = 200;
        let (cause, cut) = crate::data_sources::cap_chars(&first.cause, SUMMARY_CAUSE_CAP);
        parts.push(format!(
            "bounded retry absorbed {} transient model-call failure{} (first: {} — {}{})",
            model_retries.len(),
            if model_retries.len() == 1 { "" } else { "s" },
            first.stage,
            cause,
            if cut { "…" } else { "" },
        ));
    }
    let near_full = context_pressure.len() - truncation_suspects.len();
    if near_full > 0 {
        let worst = context_pressure
            .iter()
            .filter(|u| !truncated(u))
            .max_by(|a, b| fill(a).total_cmp(&fill(b)))
            .expect("near_full > 0 implies a non-truncated pressured row");
        parts.push(format!(
            "context pressure on {near_full} local call{} (worst: {} API prompt_eval_count {} of {} tokens; measured runtime phase)",
            if near_full == 1 { "" } else { "s" },
            worst.stage,
            // A near-full fill requires a reported count, so this is always Some.
            worst.api.prompt_eval_count.unwrap_or(0),
            worst.num_ctx
        ));
    }

    // The house-view omission is informational, not an attention trigger: it is
    // the freshness gate working as designed, not infrastructure degradation.
    let attention = deep_history_failures > 0
        || carry > 0
        || dgs10_history_gap
        || !context_pressure.is_empty()
        || !output_limited.is_empty()
        || !model_retries.is_empty();
    let summary = if parts.is_empty() {
        "no priced targets this run".to_string()
    } else {
        let mut s = parts.join("; ");
        s.push('.');
        s
    };
    crate::portfolio::DataHealth {
        targets_total,
        rate_anchored_count: rate_anchored,
        raw_percentile_count: raw_fallback,
        current_multiple_carry_count: carry,
        dispersion_floor_count: floored,
        deep_history_failures,
        dgs10_history_gap,
        house_view_omitted,
        commodity_gaps: feed_gaps.commodity,
        positioning_gaps: feed_gaps.positioning,
        cboe_gap: feed_gaps.cboe,
        finra_gap: feed_gaps.finra,
        ma_gap: feed_gaps.ma,
        accuracy_gap: feed_gaps.accuracy,
        benchmark_gaps: feed_gaps.benchmark,
        research_degraded_holdings,
        research_gap_count,
        prompt_usage,
        context_pressure,
        peak_prompt,
        model_retries,
        attention,
        summary,
    }
}

/// RFC3339 UTC, the canonical persisted form; local-time conversion is a display
/// concern at the UI seam.
fn now_rfc3339() -> String {
    #[cfg(test)]
    if let Some(now) = TEST_NOW.with(|t| t.borrow().clone()) {
        return now;
    }
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
thread_local! {
    /// A test's stepped run clock, read by [`now_rfc3339`] while set.
    static TEST_NOW: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The fail-soft shell around a prior-state store read: an `Err` degrades to
/// `None` (no prior state), never a run failure, but leaves one labeled stderr
/// line so a degraded read is distinguishable from a genuine first run.
fn prior_state_read<T>(what: &str, read: Result<Option<T>>) -> Option<T> {
    match read {
        Ok(state) => state,
        Err(e) => {
            eprintln!("[portfolio-job] {what} read degraded to none: {e:#}");
            None
        }
    }
}

/// The snapshot dates to try for ET session `today`: that session first, then
/// earlier weekdays. Shares the report chain's walk so both jobs treat holidays
/// alike.
fn sector_pe_candidates(today: chrono::NaiveDate) -> Vec<chrono::NaiveDate> {
    crate::fmp::sector_candidate_dates(today, crate::fmp::SECTOR_LOOKBACK_WEEKDAYS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::pipeline::StubAnalyst;
    use crate::portfolio::{AssetClass, PositionChange};
    use crate::schwab::{FixtureHoldingsSource, Position};

    struct DealMarket;
    impl MarketContextSource for DealMarket {
        fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors> {
            StubMarket.rates()
        }
        fn mergers_acquisitions(
            &self,
            _session: chrono::NaiveDate,
        ) -> (Vec<crate::portfolio::evidence::MaDeal>, Option<String>) {
            (
                vec![crate::portfolio::evidence::MaDeal {
                    acquirer_symbol: Some("GNL".into()),
                    acquirer_name: Some("Global Net Lease, Inc.".into()),
                    target_symbol: Some("AAPL".into()),
                    target_name: None,
                    transaction_date: "2026-06-01".into(),
                    link: None,
                }],
                Some("FMP M&A feed truncated at page 1 (test)".into()),
            )
        }
    }

    #[test]
    fn a_holding_the_ma_feed_names_is_matched_at_assembly_and_the_audit_says_so() {
        let (_dir, paths) = paths();
        let holdings = holdings_of(vec![
            stock("AAPL", 20.0, 3_900.0),
            stock("MSFT", 10.0, 4_000.0),
        ]);
        let run = match run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings),
            &StubCompanyData,
            &DealMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap()
        {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        let sources = |symbol: &str| {
            run.audit
                .iter()
                .find(|a| a.symbol == symbol)
                .unwrap_or_else(|| panic!("{symbol} audit"))
                .sources
                .clone()
        };
        assert!(
            sources("AAPL").iter().any(|s| s.contains("M&A feed")),
            "{:?}",
            sources("AAPL")
        );
        assert!(
            !sources("MSFT").iter().any(|s| s.contains("M&A feed")),
            "{:?}",
            sources("MSFT")
        );
        let dh = &run.roll_up.data_health;
        assert!(dh.ma_gap);
        assert!(dh.summary.contains("M&A feed unavailable or truncated"), "{}", dh.summary);
        assert!(!dh.attention, "an enriching-feed gap never trips attention");
    }

    #[test]
    fn roll_up_weights_over_an_unusable_total_read_zero_never_non_finite() {
        // A subnormal account total overflows the quotient.
        let holdings = crate::schwab::Holdings {
            positions: vec![Position {
                symbol: "AAPL".into(),
                description: "Apple".into(),
                asset_class: AssetClass::Stock,
                quantity: 1.0,
                cost_basis: 1.0,
                market_value: 1e10,
                current_price: Some(1.0),
            }],
            cash: 1e10,
            account_total: 1e-310,
            source_rows: vec![],
        };
        let roll_up = build_roll_up(
            &holdings,
            &[],
            &[],
            &[],
            &std::collections::HashSet::new(),
            0,
            false,
            false,
            FeedGaps::default(),
            vec![],
            vec![],
            0,
        );
        assert_eq!(roll_up.top_position_weight, 0.0);
        assert_eq!(roll_up.cash_weight, 0.0);
    }

    #[test]
    fn sector_pe_candidates_start_at_the_et_session_and_walk_back_weekdays() {
        // 2026-08-12 01:30 UTC = 2026-08-11 21:30 EDT. The session that traded is
        // Tuesday the 11th; the UTC date is Wednesday the 12th, whose session has
        // not happened.
        let evening = crate::market_clock::et_date_of("2026-08-12T01:30:00+00:00").unwrap();
        assert_eq!(evening, chrono::NaiveDate::from_ymd_opt(2026, 8, 11).unwrap());
        let got: Vec<String> = sector_pe_candidates(evening)
            .iter()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            got[0], "2026-08-11",
            "the ET session leads, never the rolled-over UTC day"
        );
        assert_eq!(
            got,
            ["2026-08-11", "2026-08-10", "2026-08-07", "2026-08-06", "2026-08-05"],
            "weekends cost no request"
        );

        // A Sunday run starts at the prior Friday, as the report chain does.
        let sunday = chrono::NaiveDate::from_ymd_opt(2026, 8, 9).unwrap();
        assert_eq!(
            sector_pe_candidates(sunday)[0],
            chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap()
        );
    }

    #[test]
    fn a_sector_pe_surface_requires_both_exchange_legs() {
        let row = |exchange: &str| crate::portfolio::fund::SectorPe {
            sector: "Technology".into(),
            exchange: exchange.into(),
            date: "2026-08-07".into(),
            pe: 25.0,
        };
        let complete = complete_sector_pe_exchange_basis(
            "snapshot",
            [Ok(vec![row("NYSE")]), Ok(vec![row("NASDAQ")])],
        )
        .unwrap();
        assert_eq!(complete.len(), 2);

        let partial = complete_sector_pe_exchange_basis(
            "snapshot",
            [
                Ok(vec![row("NYSE")]),
                Err(anyhow::anyhow!("rate limited")),
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(partial.contains("incomplete exchange basis"), "{partial}");
        assert!(partial.contains("NASDAQ failed"), "{partial}");

        let empty = complete_sector_pe_exchange_basis(
            "history",
            [Ok(vec![row("NYSE")]), Ok(vec![])],
        )
        .unwrap_err()
        .to_string();
        assert!(empty.contains("NASDAQ served no rows"), "{empty}");
    }

    #[test]
    fn over_age_dates_the_vintage_on_its_et_session() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        // 2026-07-08 01:30 UTC = 2026-07-07 21:30 EDT: the ET session of the 7th,
        // 29 days before today; the UTC prefix (the 8th, 28 days) would read it
        // one day younger.
        assert!(over_age("2026-07-08T01:30:00+00:00", today));
        // A daytime vintage on the boundary day itself stays within age.
        assert!(!over_age("2026-07-08T15:00:00+00:00", today));
        // Unparseable stays conservatively over-age.
        assert!(over_age("soon", today));
    }

    #[test]
    fn health_counts_rebuild_from_rows_and_deduplicate_benchmarks() {
        let row = |deep: bool, bench: Option<&str>| store::HoldingHealth {
            deep_history_failed: deep,
            benchmark_gap: bench.map(str::to_string),
        };
        let counts = health_counts(&[
            row(true, Some("XLK")),
            row(false, Some("XLK")),
            row(true, None),
            row(false, Some("XLF")),
        ]);
        assert_eq!(counts.deep_history_failures, 2);
        assert_eq!(counts.benchmark_gaps, ["XLF", "XLK"]);
        let empty = health_counts(&[]);
        assert_eq!((empty.deep_history_failures, empty.benchmark_gaps.len()), (0, 0));
    }

    #[test]
    fn data_health_counts_feed_gaps_without_raising_attention() {
        let dh = build_data_health(
            &[],
            &std::collections::HashSet::new(),
            0,
            false,
            false,
            FeedGaps {
                commodity: 2,
                positioning: 1,
                cboe: true,
                finra: true,
                ma: true,
                accuracy: true,
                benchmark: 1,
            },
            vec![],
            vec![],
        );
        assert_eq!(dh.commodity_gaps, 2);
        assert_eq!(dh.positioning_gaps, 1);
        assert!(dh.cboe_gap);
        assert!(dh.finra_gap);
        assert!(dh.ma_gap);
        assert!(dh.accuracy_gap);
        assert_eq!(dh.benchmark_gaps, 1);
        assert!(!dh.attention, "enriching-feed gaps never trip attention");
        assert!(dh.summary.contains("commodity context: 2 series gap(s)"), "{}", dh.summary);
        assert!(dh.summary.contains("CFTC positioning: 1 contract gap(s)"), "{}", dh.summary);
        assert!(dh.summary.contains("CBOE put/call backdrop unavailable"), "{}", dh.summary);
        assert!(dh.summary.contains("FINRA short interest unavailable"), "{}", dh.summary);
        assert!(dh.summary.contains("M&A feed unavailable or truncated"), "{}", dh.summary);
        assert!(dh.summary.contains("accuracy record degraded by a store error"), "{}", dh.summary);
        assert!(dh.summary.contains("sector benchmark series failed on 1 symbol(s)"), "{}", dh.summary);
        let dh = build_data_health(&[], &std::collections::HashSet::new(), 0, false, false, FeedGaps::default(), vec![], vec![]);
        assert!(!dh.summary.contains("commodity"), "{}", dh.summary);
    }

    fn audit_with_research_gaps(symbol: &str, gaps: Vec<String>) -> HoldingAudit {
        HoldingAudit {
            symbol: symbol.into(),
            metrics: Default::default(),
            sources: vec![],
            model_ids: vec![],
            prompt_version: crate::portfolio::PROMPT_VERSION.into(),
            evidence_floor_version: crate::portfolio::engine::EVIDENCE_FLOOR_VERSION.into(),
            degraded_inputs: vec![],
            action_annotations: vec![],
            target_meta: None,
            grade_parameter_version: crate::portfolio::engine::GRADE_PARAMETER_VERSION.into(),
            factors: None,
            quick_basis: None,
            authoring_close: None,
            fund_exposure: None,
            pre_profit: None,
            hurdle: None,
            forensic: None,
            soft_forensic: None,
            tech_event_pre_flag: None,
            short_interest: None,
            implied_expectations: None,
            narrative: None,
            option_overlay: None,
            research: Some(crate::portfolio::research::ResearchAuditRecord {
                write_ups: vec![],
                disconfirming: None,
                roster: vec![],
                fetches_spent: 0,
                elapsed_secs: 0,
                gaps,
                distillation: crate::portfolio::distill::DistillationRecord::none(),
            }),
            analysis: None,
            review: None,
            accuracy_read_through: None,
        }
    }

    #[test]
    fn data_health_rolls_persisted_research_gaps_to_the_visible_summary() {
        let audits = vec![
            audit_with_research_gaps("DEG1", vec!["gathering partial".into(), "page omitted".into()]),
            audit_with_research_gaps("CLEAN", vec![]),
            audit_with_research_gaps(
                "DEG2",
                vec![
                    "topic competitive-position: gathering degraded — 1 fetched page(s) \
                     truncated at the 12000-character fetch cap; coverage partial"
                        .into(),
                ],
            ),
        ];
        let dh = build_data_health(
            &audits,
            &std::collections::HashSet::new(),
            0,
            false,
            false,
            FeedGaps::default(),
            vec![],
            vec![],
        );

        assert_eq!(dh.research_degraded_holdings, 2);
        assert_eq!(dh.research_gap_count, 3);
        assert!(
            dh.summary
                .contains("research coverage degraded on 2 holdings (3 recorded gaps)"),
            "{}",
            dh.summary
        );
        assert!(!dh.attention, "research remains additive and fail-soft");
    }

    /// The carried set is keyed by upper-cased symbol as the loop keys it, so the
    /// join must not turn on the audit row's case.
    #[test]
    fn data_health_keeps_a_carried_holdings_prior_research_gaps_out_of_the_counts() {
        let audits = vec![
            audit_with_research_gaps("FRESH", vec!["gathering partial".into()]),
            audit_with_research_gaps(
                "Crrd",
                vec!["page omitted".into(), "gathering partial".into()],
            ),
        ];
        let carried: std::collections::HashSet<String> = ["CRRD".to_string()].into_iter().collect();
        let dh = build_data_health(
            &audits,
            &carried,
            0,
            false,
            false,
            FeedGaps::default(),
            vec![],
            vec![],
        );
        assert_eq!(dh.research_degraded_holdings, 1);
        assert_eq!(dh.research_gap_count, 1);
        assert!(
            dh.summary
                .contains("research coverage degraded on 1 holding (1 recorded gap)"),
            "{}",
            dh.summary
        );
        let dh = build_data_health(
            &audits,
            &std::collections::HashSet::new(),
            0,
            false,
            false,
            FeedGaps::default(),
            vec![],
            vec![],
        );
        assert_eq!((dh.research_degraded_holdings, dh.research_gap_count), (2, 3));
    }

    #[test]
    fn data_health_research_counts_are_required_on_app_written_runs() {
        let mut value = serde_json::to_value(crate::portfolio::DataHealth::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        assert!(object.remove("research_gap_count").is_some());
        assert!(
            serde_json::from_value::<crate::portfolio::DataHealth>(value).is_err(),
            "the wiped-store pre-release posture writes the new run field strictly"
        );
    }

    #[test]
    fn data_health_names_absorbed_retries_and_raises_attention() {
        let retries = vec![crate::local_model::RetryEvent {
            stage: "interpret WID".into(),
            cause: "daemon error status".into(),
        }];
        let dh = build_data_health(&[], &std::collections::HashSet::new(), 0, false, false, FeedGaps::default(), vec![], retries);
        assert!(dh.attention, "an absorbed transient is infrastructure degradation");
        assert_eq!(dh.model_retries.len(), 1);
        assert!(
            dh.summary.contains(
                "bounded retry absorbed 1 transient model-call failure \
                 (first: interpret WID — daemon error status)"
            ),
            "{}",
            dh.summary
        );
        // Capped on the summary line only; the structured event keeps the whole cause.
        let long_cause = format!("content failed its parse: {}", "x".repeat(1_000));
        let retries = vec![crate::local_model::RetryEvent {
            stage: "research WID synthesis".into(),
            cause: long_cause.clone(),
        }];
        let dh = build_data_health(&[], &std::collections::HashSet::new(), 0, false, false, FeedGaps::default(), vec![], retries);
        let line = dh
            .summary
            .split("; ")
            .find(|part| part.starts_with("bounded retry absorbed"))
            .expect("the retry line");
        assert!(line.contains("…)"), "{line}");
        assert!(line.chars().count() < 300, "{}", line.chars().count());
        assert_eq!(dh.model_retries[0].cause, long_cause);
        let dh = build_data_health(&[], &std::collections::HashSet::new(), 0, false, false, FeedGaps::default(), vec![], vec![]);
        assert!(!dh.attention);
        assert!(!dh.summary.contains("bounded retry"), "{}", dh.summary);
    }

    #[test]
    fn entry7_all_attempts_survive_summary_and_phase_limited_counts_stay_unattributed() {
        use crate::local_model::{ApiCounters, PromptUsage, ThinkingObservation};
        let ordinary = PromptUsage {
            stage: "holding-AAPL research t1 gathering turn 1".into(),
            model: "qwen".into(),
            num_ctx: 131_072,
            prompt_chars: 400,
            elapsed_ms: 500,
            adapter_ok: true,
            thinking: ThinkingObservation::Complete { chars: 20 },
            api: ApiCounters {
                prompt_eval_count: Some(100),
                eval_count: Some(2),
                ..Default::default()
            },
            ..Default::default()
        };
        let phase_limited = PromptUsage {
            stage: "interpret AAPL".into(),
            think: Some(true),
            format: true,
            prompt_chars: 80_000,
            num_predict: Some(65_536),
            output_limited: true,
            done_reason: Some("length".into()),
            ..ordinary.clone()
        };
        let failed = PromptUsage {
            stage: "action AAPL".into(),
            num_ctx: 131_072,
            prompt_chars: 300,
            elapsed_ms: 99,
            thinking: ThinkingObservation::Partial { chars: 7 },
            ..Default::default()
        };
        let rows = vec![ordinary, phase_limited.clone(), failed];
        let dh = build_data_health(
            &[],
            &std::collections::HashSet::new(),
            0,
            false,
            false,
            FeedGaps::default(),
            rows.clone(),
            vec![],
        );
        assert_eq!(dh.prompt_usage, rows);
        assert!(dh.context_pressure.is_empty());
        assert!(
            dh.summary.contains("phase-limited; stop unattributed"),
            "{}",
            dh.summary
        );
        assert!(!dh.summary.contains("context exhaustion suspected"));
        assert!(dh.attention, "length stops still demand attention");
        let single_phase = PromptUsage {
            think: Some(false),
            ..phase_limited
        };
        let dh = build_data_health(
            &[],
            &std::collections::HashSet::new(),
            0,
            false,
            false,
            FeedGaps::default(),
            vec![single_phase],
            vec![],
        );
        assert!(dh.summary.contains("context exhaustion suspected"));
        assert!(dh.summary.contains("likely front-truncation"));
    }

    #[test]
    fn data_health_flags_context_pressure_and_records_the_peak() {
        let usage = vec![
            crate::local_model::PromptUsage {
                stage: "interpret AAPL".into(),
                num_ctx: 131_072,
                prompt_chars: 200_000,
                num_predict: None,
                output_limited: false,
                api: crate::local_model::ApiCounters {
                    prompt_eval_count: Some(50_000),
                    eval_count: None,
                    ..Default::default()
                },
                ..Default::default()
            },
            crate::local_model::PromptUsage {
                stage: "construction".into(),
                num_ctx: 131_072,
                prompt_chars: 500_000,
                num_predict: None,
                output_limited: false,
                api: crate::local_model::ApiCounters {
                    prompt_eval_count: Some(125_000),
                    eval_count: None,
                    ..Default::default()
                },
                ..Default::default()
            },
        ];
        let dh = build_data_health(&[], &std::collections::HashSet::new(), 0, false, false, FeedGaps::default(), usage, vec![]);
        assert_eq!(dh.context_pressure.len(), 1);
        assert_eq!(dh.context_pressure[0].stage, "construction");
        assert_eq!(dh.peak_prompt.as_ref().unwrap().stage, "construction");
        assert!(dh.attention, "{}", dh.summary);
        let expected = "context pressure on 1 local call (worst: construction API prompt_eval_count 125000 of \
                        131072 tokens; measured runtime phase)";
        assert!(dh.summary.contains(expected), "{}", dh.summary);
    }

    #[test]
    fn data_health_records_the_peak_without_pressure() {
        let usage = vec![crate::local_model::PromptUsage {
            stage: "interpret MSFT".into(),
            num_ctx: 131_072,
            prompt_chars: 360_000,
            num_predict: None,
            output_limited: false,
            api: crate::local_model::ApiCounters {
                prompt_eval_count: Some(90_000),
                eval_count: None,
                ..Default::default()
            },
            ..Default::default()
        }];
        let dh = build_data_health(&[], &std::collections::HashSet::new(), 0, false, false, FeedGaps::default(), usage, vec![]);
        assert!(dh.context_pressure.is_empty());
        let peak = dh
            .peak_prompt
            .expect("peak recorded regardless of pressure");
        assert_eq!(peak.api.prompt_eval_count, Some(90_000));
        assert!(!dh.attention, "{}", dh.summary);
        assert!(!dh.summary.contains("context pressure"), "{}", dh.summary);
    }

    #[test]
    fn data_health_names_an_output_limited_call() {
        let usage = vec![crate::local_model::PromptUsage {
            stage: "construction".into(),
            num_ctx: 131_072,
            prompt_chars: 240_000,
            num_predict: Some(65_536),
            output_limited: true,
            api: crate::local_model::ApiCounters {
                prompt_eval_count: Some(60_000),
                eval_count: Some(65_536),
                ..Default::default()
            },
            ..Default::default()
        }];
        let dh = build_data_health(&[], &std::collections::HashSet::new(), 0, false, false, FeedGaps::default(), usage, vec![]);
        assert!(dh.attention, "{}", dh.summary);
        let expected =
            "generation length-stopped on 1 local call (largest returned eval_count: construction API eval_count 65536 of \
             65536 reserved — at the output reservation)";
        assert!(dh.summary.contains(expected), "{}", dh.summary);
    }

    #[test]
    fn data_health_keeps_a_length_stop_with_no_prompt_count() {
        let usage = vec![crate::local_model::PromptUsage {
            stage: "construction".into(),
            num_ctx: 131_072,
            prompt_chars: 240_000,
            num_predict: Some(65_536),
            output_limited: true,
            api: crate::local_model::ApiCounters {
                prompt_eval_count: None,
                eval_count: None,
                ..Default::default()
            },
            ..Default::default()
        }];
        let dh = build_data_health(&[], &std::collections::HashSet::new(), 0, false, false, FeedGaps::default(), usage, vec![]);
        assert!(dh.attention, "{}", dh.summary);
        let expected = "generation length-stopped on 1 local call (largest returned eval_count: construction \
                        API eval_count unreported of 65536 reserved — counts incomplete or phase-limited; stop \
                        unattributed)";
        assert!(dh.summary.contains(expected), "{}", dh.summary);
        assert!(dh.context_pressure.is_empty(), "{}", dh.summary);
        assert!(!dh.summary.contains("front-truncation"), "{}", dh.summary);
        assert!(dh.peak_prompt.is_none(), "{:?}", dh.peak_prompt);
    }

    /// The post-truncation count reads as a comfortable ~50% fill, so only the
    /// chars-vs-count check can see it.
    #[test]
    fn data_health_flags_likely_truncation_despite_comfortable_fill() {
        let usage = vec![crate::local_model::PromptUsage {
            stage: "interpret NVDA".into(),
            num_ctx: 2_048,
            prompt_chars: 18_400,
            num_predict: None,
            output_limited: false,
            api: crate::local_model::ApiCounters {
                prompt_eval_count: Some(1_026),
                eval_count: None,
                ..Default::default()
            },
            ..Default::default()
        }];
        let dh = build_data_health(&[], &std::collections::HashSet::new(), 0, false, false, FeedGaps::default(), usage, vec![]);
        assert_eq!(dh.context_pressure.len(), 1);
        assert!(dh.attention, "{}", dh.summary);
        let expected =
            "likely front-truncation on 1 local call (worst: interpret NVDA API prompt_eval_count \
                        1026 tokens for a 18400-char prompt, num_ctx 2048)";
        assert!(dh.summary.contains(expected), "{}", dh.summary);
        assert!(!dh.summary.contains("context pressure on"), "{}", dh.summary);
    }

    /// Decimal ratios, with a DGS10 history covering the fixture anchor window.
    struct StubMarket;
    impl MarketContextSource for StubMarket {
        fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors> {
            Ok(crate::portfolio::engine::RateAnchors {
                dgs2: 0.04,
                dgs10: 0.045,
                dgs10_history: (2022..=2026)
                    .flat_map(|y| {
                        ["01-02", "04-01", "07-01", "10-01"].iter().map(move |md| {
                            crate::portfolio::engine::DatedValue {
                                date: format!("{y}-{md}"),
                                value: 0.04,
                            }
                        })
                    })
                    .collect(),
                history_gap: None,
                ..Default::default()
            })
        }
    }

    struct HistoryGapMarket;
    impl MarketContextSource for HistoryGapMarket {
        fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors> {
            Ok(crate::portfolio::engine::RateAnchors {
                dgs2: 0.04,
                dgs10: 0.045,
                dgs10_history: vec![],
                history_gap: Some(
                    "DGS10 anchor-window history unavailable: simulated outage — \
                     every spread observation inadmissible; targets fell to the \
                     documented raw-percentile / carry fallback"
                        .to_string(),
                ),
                ..Default::default()
            })
        }
    }

    struct FailingMarket;
    impl MarketContextSource for FailingMarket {
        fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors> {
            anyhow::bail!("simulated FRED outage")
        }
    }

    /// Strong fixture financials, including the quarterly prints, consensus and
    /// dated closes the driver ladder and anchor window need.
    struct StubCompanyData;
    impl CompanyDataSource for StubCompanyData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            use crate::portfolio::engine::{
                ConsensusEpsPeriod, ConsensusEstimate, DatedValue, QuarterlyBalanceSheetRow,
                QuarterlyIncomeRow,
            };
            let ends = [
                "2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30", "2025-06-30",
                "2025-03-31", "2024-12-31", "2024-09-30", "2024-06-30", "2024-03-31",
                "2023-12-31", "2023-09-30", "2023-06-30", "2023-03-31", "2022-12-31",
                "2022-09-30",
            ];
            CompanyFinancials {
                symbol: symbol.to_string(),
                current_price: Some(195.0),
                market_cap: Some(3.0e12),
                shares_outstanding: Some(1.5e10),
                revenue: Some(400.0),
                revenue_prior: Some(360.0),
                gross_profit: Some(180.0),
                net_income: Some(100.0),
                total_equity: Some(200.0),
                total_debt: Some(100.0),
                pe_ratio: Some(28.0),
                ps_ratio: Some(7.5),
                pb_ratio: Some(6.0),
                price_history: vec![170.0, 180.0, 188.0, 195.0],
                daily_closes: ends
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(i, end)| DatedValue {
                        date: end.to_string(),
                        value: 130.0 + 4.0 * i as f64,
                    })
                    .collect(),
                quarterly_income: ends
                    .iter()
                    .enumerate()
                    .map(|(i, end)| QuarterlyIncomeRow {
                        period_end: end.to_string(),
                        filing_date: None,
                        revenue: Some(100.0e9 - 1.0e9 * i as f64),
                        eps_diluted: Some(1.55 - 0.01 * i as f64),
                        // The factor composite's value and quality legs.
                        net_income: Some(2.5e10),
                        gross_profit: Some(45.0e9),
                        cost_of_revenue: None,
                        diluted_shares: Some(1.5e10),
                        operating_income: None,
                        research_and_development: None,
                        income_before_tax: None,
                        income_tax_expense: None,
                    })
                    .collect(),
                // The low-risk axis's debt / equity and the quality axis's assets.
                quarterly_balance_sheet: ends
                    .iter()
                    .map(|end| QuarterlyBalanceSheetRow {
                        period_end: end.to_string(),
                        total_assets: Some(3.5e11),
                        total_debt: Some(1.0e11),
                        total_equity: Some(7.0e10),
                        cash_and_equivalents: Some(6.0e10),
                        short_term_investments: Some(0.0),
                        ..Default::default()
                    })
                    .collect(),
                consensus: Some(ConsensusEstimate {
                    period_end: "2027-06-30".into(),
                    eps_low: Some(6.0),
                    eps_mid: Some(6.5),
                    eps_high: Some(7.0),
                    revenue_low: Some(420.0e9),
                    revenue_mid: Some(430.0e9),
                    revenue_high: Some(440.0e9),
                    eps_periods: vec![ConsensusEpsPeriod {
                        period_end: "2027-06-30".into(),
                        eps_mid: Some(6.5),
                        ntm_weight: 1.0,
                    }],
                    ..ConsensusEstimate::default()
                }),
                ttm_dividends_per_share: Some(1.0),
                ..CompanyFinancials::default()
            }
        }
        fn facts(&self, _symbol: &str) -> SecData {
            // A stub, not a failed fetch, so no gap; `Some` so the audit labels the
            // leg consulted-but-empty.
            SecData {
                facts: Some(CompanyFacts::default()),
                gaps: Vec::new(),
            }
        }
    }

    /// A US equity ETF with a full sector-P/E snapshot + history, so the fund path
    /// runs offline.
    struct FundCompanyData;
    impl CompanyDataSource for FundCompanyData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            use crate::portfolio::engine::DatedValue;
            CompanyFinancials {
                symbol: symbol.to_string(),
                current_price: Some(195.0),
                price_history: vec![170.0, 180.0, 188.0, 195.0],
                daily_closes: vec![
                    DatedValue { date: "2026-04-01".into(), value: 170.0 },
                    DatedValue { date: "2026-05-01".into(), value: 180.0 },
                    DatedValue { date: "2026-06-01".into(), value: 188.0 },
                    DatedValue { date: "2026-07-15".into(), value: 195.0 },
                ],
                ttm_dividends_per_share: Some(2.4),
                ..CompanyFinancials::default()
            }
        }
        fn facts(&self, _symbol: &str) -> SecData {
            SecData::default()
        }
        fn fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
            crate::portfolio::fund::FundData {
                symbol: symbol.to_string(),
                name: Some("Total US Market ETF".into()),
                asset_class: Some("Equity".into()),
                expense_ratio: Some(0.0003),
                aum: Some(4.0e11),
                nav: Some(194.0),
                sector_weights: vec![
                    ("Technology".into(), 0.6),
                    ("Financial Services".into(), 0.4),
                ],
                country_weights: vec![("United States".into(), 0.99)],
                profile_is_fund: None,
                profile_description: None,
                gaps: vec![],
            }
        }
        fn sector_pe_snapshot(
            &self,
            _session: chrono::NaiveDate,
        ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
            Ok([("Technology", 30.0, 34.0), ("Financial Services", 14.0, 16.0)]
                .iter()
                .flat_map(|(sector, nyse, nasdaq)| {
                    vec![
                        crate::portfolio::fund::SectorPe {
                            sector: sector.to_string(),
                            exchange: "NYSE".into(),
                            date: "2026-07-15".into(),
                            pe: *nyse,
                        },
                        crate::portfolio::fund::SectorPe {
                            sector: sector.to_string(),
                            exchange: "NASDAQ".into(),
                            date: "2026-07-15".into(),
                            pe: *nasdaq,
                        },
                    ]
                })
                .collect())
        }
        fn sector_pe_history(&self, sector: &str) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
            let base = if sector == "Technology" { 26.0 } else { 13.0 };
            let dates = [
                "2022-09-15", "2022-12-15", "2023-03-15", "2023-06-15", "2023-09-15",
                "2023-12-15", "2024-03-15", "2024-06-15", "2024-09-15", "2024-12-15",
                "2025-03-15", "2025-06-15", "2025-09-15", "2025-12-15", "2026-03-15",
                "2026-06-15",
            ];
            Ok(dates
                .iter()
                .enumerate()
                .flat_map(|(i, date)| {
                    ["NYSE", "NASDAQ"].iter().map(move |ex| crate::portfolio::fund::SectorPe {
                        sector: sector.to_string(),
                        exchange: ex.to_string(),
                        date: date.to_string(),
                        pe: base + 0.2 * i as f64,
                    })
                })
                .collect())
        }
    }

    struct DegradedDeepHistoryData;
    impl CompanyDataSource for DegradedDeepHistoryData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            let mut fin = StubCompanyData.financials(symbol);
            // Blank the fixture's closes so `deep_price_history` decides, as live.
            fin.daily_closes = vec![];
            fin
        }
        fn facts(&self, symbol: &str) -> SecData {
            StubCompanyData.facts(symbol)
        }
        fn deep_price_history(
            &self,
            symbol: &str,
        ) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
            (
                vec![],
                vec![format!(
                    "FMP deep price history unavailable for {symbol}: throttled; the \
                     anchor window falls to its documented fallback"
                )],
            )
        }
    }

    struct FailingSecCompanyData;
    impl CompanyDataSource for FailingSecCompanyData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            StubCompanyData.financials(symbol)
        }
        fn facts(&self, symbol: &str) -> SecData {
            SecData {
                facts: Some(CompanyFacts::default()),
                gaps: vec![format!("SEC company facts unavailable: simulated outage for {symbol}")],
            }
        }
    }

    fn paths() -> (tempfile::TempDir, ReportPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = ReportPaths::under(dir.path());
        (dir, paths)
    }

    fn ctx() -> std::sync::Arc<RunContext> {
        RunContext::noop()
    }

    /// Flips the shared cancel flag when a step whose label contains `trigger`
    /// starts: the deterministic mid-run interrupt that leaves a resumable trail.
    struct CancelOnStep {
        trigger: &'static str,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl crate::progress::ProgressReporter for CancelOnStep {
        fn report(&self, message: &crate::progress::ProgressMessage) {
            if let crate::progress::ProgressEvent::StepStarted { label, .. } = &message.event {
                if label.contains(self.trigger) {
                    self.cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
    }

    fn cancel_ctx(trigger: &'static str) -> std::sync::Arc<RunContext> {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        RunContext::new(
            "cancel-on-step",
            std::sync::Arc::new(CancelOnStep {
                trigger,
                cancel: cancel.clone(),
            }),
            cancel,
        )
    }

    /// Cost basis derived so the account math stays consistent; the diff
    /// classifies by quantity.
    fn stock(symbol: &str, quantity: f64, market_value: f64) -> Position {
        Position {
            symbol: symbol.into(),
            description: format!("{symbol} Inc."),
            asset_class: AssetClass::Stock,
            quantity,
            cost_basis: market_value * 0.8,
            market_value,
            current_price: Some(market_value / quantity),
        }
    }

    fn holdings_of(positions: Vec<Position>) -> Holdings {
        let cash = 10_000.0;
        let account_total = positions.iter().map(|p| p.market_value).sum::<f64>() + cash;
        Holdings {
            positions,
            cash,
            account_total,
            source_rows: vec![],
        }
    }

    /// Resolves one symbol to a non-US (PNK) listing and refuses to serve its
    /// statements or SEC facts: the guard-terminal skip's tripwire.
    struct NonUsListingData;
    impl CompanyDataSource for NonUsListingData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            assert_ne!(
                symbol, "NTDOF",
                "a guard-terminal stock must not fetch statements"
            );
            StubCompanyData.financials(symbol)
        }
        fn facts(&self, symbol: &str) -> SecData {
            assert_ne!(symbol, "NTDOF", "a guard-terminal stock must not hit SEC");
            StubCompanyData.facts(symbol)
        }
        fn deep_price_history(
            &self,
            symbol: &str,
        ) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
            assert_ne!(
                symbol, "NTDOF",
                "a guard-terminal stock must not pull deep history"
            );
            assert_ne!(
                symbol, "XLK",
                "a guard-terminal stock must not pull its sector benchmark"
            );
            (vec![], vec![])
        }
        fn profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
            use crate::portfolio::listing::{ProfileIdentity, ProfileLookup};
            if symbol == "NTDOF" {
                ProfileLookup::Resolved(ProfileIdentity {
                    website: None,
                    currency: Some("USD".into()),
                    is_adr: Some(false),
                    company_name: Some("Nintendo Co., Ltd.".into()),
                    exchange: Some("PNK".into()),
                    sector: Some("Communication Services".into()),
                    industry: None,
                    beta: None,
                })
            } else {
                ProfileLookup::Unverified("profile source not wired".into())
            }
        }
    }

    struct ChainTripwire {
        inner: FixtureHoldingsSource,
        barred: &'static str,
    }
    impl crate::schwab::HoldingsSource for ChainTripwire {
        fn holdings(&self) -> anyhow::Result<crate::schwab::Holdings> {
            self.inner.holdings()
        }
        fn option_chain(
            &self,
            symbol: &str,
        ) -> anyhow::Result<Option<crate::schwab::OptionChain>> {
            assert_ne!(
                symbol, self.barred,
                "a guard-terminal stock must not pull an option chain"
            );
            self.inner.option_chain(symbol)
        }
    }

    #[test]
    fn a_non_us_listing_is_not_rated_and_skips_the_statement_fetch() {
        let (_dir, paths) = paths();
        let holdings = holdings_of(vec![
            stock("AAPL", 20.0, 3_900.0),
            stock("NTDOF", 100.0, 1_000.0),
        ]);
        let run = match run_portfolio_job(
            &ChainTripwire {
                inner: FixtureHoldingsSource::with_holdings(holdings),
                barred: "NTDOF",
            },
            &NonUsListingData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap()
        {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        let ntdof = run
            .verdicts
            .iter()
            .find(|v| v.symbol == "NTDOF")
            .expect("NTDOF verdict");
        match &ntdof.disposition {
            crate::portfolio::VerdictDisposition::NotRated { reason } => {
                assert!(
                    reason.contains("unsupported listing") && reason.contains("PNK"),
                    "{reason}"
                );
            }
            other => panic!("expected not-rated, got {other:?}"),
        }
        // The never-consulted statement surface is not claimed.
        let ntdof_audit = run
            .audit
            .iter()
            .find(|a| a.symbol == "NTDOF")
            .expect("NTDOF audit");
        assert!(
            ntdof_audit
                .sources
                .iter()
                .any(|s| s.contains("listing-resolution guard")),
            "{:?}",
            ntdof_audit.sources
        );
        assert!(
            !ntdof_audit
                .sources
                .iter()
                .any(|s| s.contains("FMP company financials")),
            "{:?}",
            ntdof_audit.sources
        );
        // No guard input is never a terminal outcome.
        let aapl = run
            .verdicts
            .iter()
            .find(|v| v.symbol == "AAPL")
            .expect("AAPL verdict");
        assert!(matches!(
            aapl.disposition,
            crate::portfolio::VerdictDisposition::Priced(_)
        ));
    }

    #[test]
    fn a_carried_guard_terminal_stock_skips_its_sector_benchmark() {
        let (_dir, paths) = paths();
        let holdings = || {
            holdings_of(vec![
                stock("AAPL", 20.0, 3_900.0),
                stock("NTDOF", 100.0, 1_000.0),
            ])
        };
        let prior = full_run(&paths, holdings());
        assert!(prior.verdicts.iter().any(|v| v.symbol == "NTDOF"));

        let run = match run_portfolio_job(
            &ChainTripwire {
                inner: FixtureHoldingsSource::with_holdings(holdings()),
                barred: "NTDOF",
            },
            &NonUsListingData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap()
        {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        let ntdof = verdict(&run, "NTDOF");
        assert!(matches!(
            ntdof.disposition,
            crate::portfolio::VerdictDisposition::NotRated { .. }
        ));
        assert_eq!(
            run.roll_up.data_health.benchmark_gaps, 0,
            "a guard-terminal holding cannot create benchmark telemetry"
        );
    }

    #[test]
    fn a_conflict_abstention_preserves_the_prior_vintage_and_document() {
        // Run 2's profile read resolves MSFT to a different issuer.
        struct ConflictData;
        impl CompanyDataSource for ConflictData {
            fn financials(&self, symbol: &str) -> CompanyFinancials {
                assert_ne!(
                    symbol, "MSFT",
                    "a guard-terminal stock must not fetch statements"
                );
                StubCompanyData.financials(symbol)
            }
            fn facts(&self, symbol: &str) -> SecData {
                assert_ne!(symbol, "MSFT", "a guard-terminal stock must not hit SEC");
                StubCompanyData.facts(symbol)
            }
            fn deep_price_history(
                &self,
                symbol: &str,
            ) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
                assert_ne!(
                    symbol, "MSFT",
                    "a guard-terminal stock must not pull deep history"
                );
                (vec![], vec![])
            }
            fn profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
                use crate::portfolio::listing::{ProfileIdentity, ProfileLookup};
                if symbol == "MSFT" {
                    ProfileLookup::Resolved(ProfileIdentity {
                        website: None,
                        currency: Some("USD".into()),
                        is_adr: Some(false),
                        company_name: Some("Zenith Mining Corp".into()),
                        exchange: Some("NYSE".into()),
                        sector: None,
                        industry: None,
                        beta: None,
                    })
                } else {
                    ProfileLookup::Unverified("profile source not wired".into())
                }
            }
        }
        let (_dir, paths) = paths();
        // The fixture's default "MSFT Inc." description is ticker-only-token, which
        // the guard reads unverifiable rather than conflict, so the collision
        // needs a real issuer name.
        let named = || {
            let mut h = two_stocks();
            for p in &mut h.positions {
                if p.symbol == "MSFT" {
                    p.description = "Microsoft Corporation".into();
                }
            }
            h
        };
        let first = full_run(&paths, named());
        let second = match run_portfolio_job(
            &ChainTripwire {
                inner: FixtureHoldingsSource::with_holdings(named()),
                barred: "MSFT",
            },
            &ConflictData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap()
        {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        let msft = verdict(&second, "MSFT");
        assert!(
            matches!(
                &msft.disposition,
                crate::portfolio::VerdictDisposition::InsufficientEvidence { reason, .. }
                    if reason.contains("conflicting identity")
            ),
            "{:?}",
            msft.disposition
        );
        assert_eq!(
            msft.analyzed_at.as_deref(),
            Some(first.created_at.as_str()),
            "the abstention preserves the prior full pass's vintage"
        );
        assert!(
            msft.thesis_document().is_some(),
            "the prior thesis document rides through the conflict abstention"
        );
    }

    #[test]
    fn job_runs_end_to_end_offline_and_persists_a_graded_run() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &StubCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        assert_eq!(run.verdicts.len(), 1);
        assert_eq!(run.roll_up.graded_count, 1);
        assert!(run.roll_up.top_position_weight > 0.0);

        let dh = &run.roll_up.data_health;
        assert_eq!(dh.targets_total, 1);
        assert_eq!(dh.rate_anchored_count, 1);
        assert_eq!(dh.deep_history_failures, 0);
        assert!(!dh.attention, "{}", dh.summary);
        assert!(dh.summary.contains("1 of 1 targets rate-anchored"), "{}", dh.summary);

        let conn = storage::open(&paths.db_path).unwrap();
        let latest = store::latest_run(&conn).unwrap().unwrap();
        assert_eq!(latest.run_id, run.run_id);
        let composite = latest.audit[0].factors.as_ref().expect("a priced stock's composite");
        assert_eq!(latest.audit[0].factors, run.audit[0].factors);
        assert!(!composite.factors.is_empty());
        let crate::portfolio::VerdictDisposition::Priced(g) = &latest.verdicts[0].disposition else {
            panic!("a graded stock");
        };
        assert_eq!(g.grade_basis, Some(crate::portfolio::GradeBasis::of(composite)));
        let state: String = conn
            .query_row(
                "SELECT state FROM job_runs WHERE job_type = ?1 ORDER BY id DESC LIMIT 1",
                [PORTFOLIO_JOB],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "successful");
    }

    #[test]
    fn deep_history_failure_starves_the_anchor_window_and_demands_attention() {
        let (_dir, paths) = paths();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &DegradedDeepHistoryData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        let dh = &run.roll_up.data_health;
        assert_eq!(dh.deep_history_failures, 1);
        assert_eq!(dh.current_multiple_carry_count, 1, "{}", dh.summary);
        assert!(dh.attention, "{}", dh.summary);
        assert!(dh.summary.contains("deep price history failed on 1 holdings"), "{}", dh.summary);
    }

    #[test]
    fn failed_sec_fetch_is_recorded_as_a_degraded_input() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &FailingSecCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        let audit = &run.audit[0];
        assert!(
            audit
                .degraded_inputs
                .iter()
                .any(|g| g.contains("SEC company facts unavailable")),
            "a failed SEC fetch must surface as a degraded input: {:?}",
            audit.degraded_inputs
        );
    }

    #[test]
    fn a_failed_rate_anchor_fails_the_run_before_any_holding() {
        // `docs/portfolio-analysis.md` §Failure posture.
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &StubCompanyData,
            &FailingMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        match outcome {
            PortfolioJobOutcome::Failed(msg) => {
                assert!(msg.contains("rate-anchor"), "{msg}");
            }
            other => panic!("expected a failed run, got {other:?}"),
        }
        let conn = storage::open(&paths.db_path).unwrap();
        assert!(store::latest_run(&conn).unwrap().is_none());
    }

    #[test]
    fn a_failed_dgs10_history_degrades_to_the_fallback_not_a_failed_run() {
        // `docs/portfolio-analysis.md` §Starting parameters.
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &StubCompanyData,
            &HistoryGapMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        assert_eq!(run.roll_up.graded_count, 1, "{}", run.roll_up.overview);
        let audit = &run.audit[0];
        assert!(
            audit
                .degraded_inputs
                .iter()
                .any(|g| g.contains("DGS10 anchor-window history")),
            "the run-level degradation must reach the audit: {:?}",
            audit.degraded_inputs
        );
        let meta = audit.target_meta.as_ref().expect("target meta rides the audit");
        assert!(
            !meta.rate_anchored,
            "an empty admissible window cannot rate-anchor"
        );
        assert!(
            !meta.current_multiple_carry,
            "a failed DGS10 join must not degrade to the current-multiple carry"
        );
        let dh = &run.roll_up.data_health;
        assert!(dh.dgs10_history_gap);
        assert!(dh.attention, "{}", dh.summary);
        assert!(dh.summary.contains("DGS10 anchor history failed run-wide"), "{}", dh.summary);
        match &run.verdicts[0].disposition {
            crate::portfolio::VerdictDisposition::Priced(g) => {
                let tm = g.price_targets.twelve_month.as_ref().unwrap();
                assert!(
                    tm.methodology.contains("raw multiple percentiles"),
                    "{}",
                    tm.methodology
                );
            }
            other => panic!("expected a priced verdict, got {other:?}"),
        }
    }

    #[test]
    fn a_failed_sector_pe_snapshot_records_its_gap_on_every_fund_not_just_the_first() {
        struct NoSnapshot;
        impl CompanyDataSource for NoSnapshot {
            fn financials(&self, symbol: &str) -> CompanyFinancials {
                FundCompanyData.financials(symbol)
            }
            fn facts(&self, symbol: &str) -> SecData {
                FundCompanyData.facts(symbol)
            }
            fn fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
                FundCompanyData.fund_data(symbol)
            }
            fn sector_pe_snapshot(
                &self,
                _session: chrono::NaiveDate,
            ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
                anyhow::bail!("no sector-P/E snapshot in the 5 weekdays through 2026-08-07")
            }
            fn sector_pe_history(
                &self,
                sector: &str,
            ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
                FundCompanyData.sector_pe_history(sector)
            }
        }

        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let mut first = stock("VTI", 50.0, 9_750.0);
        first.asset_class = AssetClass::Etf;
        let mut second = stock("ITOT", 40.0, 7_800.0);
        second.asset_class = AssetClass::Etf;
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings_of(vec![first, second])),
            &NoSnapshot,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };

        assert_eq!(run.audit.len(), 2, "both funds analyzed");
        for audit in &run.audit {
            assert!(
                audit
                    .degraded_inputs
                    .iter()
                    .any(|g| g.contains("sector-P/E snapshot unavailable")),
                "{} lost the snapshot gap: {:?}",
                audit.symbol,
                audit.degraded_inputs
            );
        }
    }

    #[test]
    fn a_failed_sector_pe_history_records_its_gap_on_every_dependent_fund() {
        struct NoHistory;
        impl CompanyDataSource for NoHistory {
            fn financials(&self, symbol: &str) -> CompanyFinancials {
                FundCompanyData.financials(symbol)
            }
            fn facts(&self, symbol: &str) -> SecData {
                FundCompanyData.facts(symbol)
            }
            fn fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
                FundCompanyData.fund_data(symbol)
            }
            fn sector_pe_snapshot(
                &self,
                session: chrono::NaiveDate,
            ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
                FundCompanyData.sector_pe_snapshot(session)
            }
            fn sector_pe_history(
                &self,
                sector: &str,
            ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
                anyhow::bail!("history fault for {sector}")
            }
        }

        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let mut first = stock("VTI", 50.0, 9_750.0);
        first.asset_class = AssetClass::Etf;
        let mut second = stock("ITOT", 40.0, 7_800.0);
        second.asset_class = AssetClass::Etf;
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings_of(vec![first, second])),
            &NoHistory,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };

        assert_eq!(run.audit.len(), 2, "both funds analyzed");
        for audit in &run.audit {
            assert!(
                audit
                    .degraded_inputs
                    .iter()
                    .any(|gap| gap.contains("sector-P/E history unavailable for Technology")),
                "{} lost the memoized history gap: {:?}",
                audit.symbol,
                audit.degraded_inputs
            );
        }
    }

    #[test]
    fn a_fund_holding_takes_the_reduced_path_end_to_end() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let mut fund_position = stock("VTI", 50.0, 9_750.0);
        fund_position.asset_class = AssetClass::Etf;
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings_of(vec![fund_position])),
            &FundCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        assert_eq!(run.roll_up.graded_count, 1, "{}", run.roll_up.overview);
        match &run.verdicts[0].disposition {
            crate::portfolio::VerdictDisposition::Priced(g) => {
                assert!(g.low_confidence_grade);
                assert_eq!(g.sub_scores.quality, 50.0);
                let tm = g.price_targets.twelve_month.as_ref().unwrap();
                assert!(tm.methodology.contains("fund exposure composite"));
            }
            other => panic!("expected a priced fund verdict, got {other:?}"),
        }
        // A fund grades on no factor composite, so it contributes nothing to the store.
        assert!(run.audit[0].factors.is_none());
        assert!(factor_observations(&paths).is_empty());
    }

    /// [`FundCompanyData`] with a tripwired SEC leg; the fund path must never
    /// reach it.
    struct SecTripwireFundData;
    impl CompanyDataSource for SecTripwireFundData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            FundCompanyData.financials(symbol)
        }
        fn facts(&self, symbol: &str) -> SecData {
            panic!("SEC company facts must not be fetched for a fund ({symbol})");
        }
        fn fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
            FundCompanyData.fund_data(symbol)
        }
        fn sector_pe_snapshot(
            &self,
            session: chrono::NaiveDate,
        ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
            FundCompanyData.sector_pe_snapshot(session)
        }
        fn sector_pe_history(
            &self,
            sector: &str,
        ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
            FundCompanyData.sector_pe_history(sector)
        }
    }

    #[test]
    fn a_fund_holding_never_fetches_sec_facts() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let mut fund_position = stock("VTI", 50.0, 9_750.0);
        fund_position.asset_class = AssetClass::Etf;
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings_of(vec![fund_position])),
            &SecTripwireFundData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        // The tripwire not firing is the assertion.
        let audit = &run.audit[0];
        assert!(
            !audit.sources.iter().any(|s| s.contains("SEC")),
            "no SEC source on a fund audit: {:?}",
            audit.sources
        );
        assert!(
            !audit.degraded_inputs.iter().any(|g| g.contains("SEC")),
            "no SEC gap noise on a fund audit: {:?}",
            audit.degraded_inputs
        );
    }

    #[test]
    fn a_second_concurrent_run_is_skipped_by_the_shared_guard() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        // Hold the slot as if a report (or another local job) were running.
        let _token = guard.try_begin(RunKind::Report).unwrap();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &StubCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        assert!(matches!(outcome, PortfolioJobOutcome::Skipped(_)));
    }

    #[test]
    fn the_sector_pe_snapshot_is_dated_on_the_runs_pinned_et_session() {
        struct RecordingSnapshotData {
            asked: std::sync::Mutex<Vec<chrono::NaiveDate>>,
        }
        impl CompanyDataSource for RecordingSnapshotData {
            fn financials(&self, symbol: &str) -> CompanyFinancials {
                FundCompanyData.financials(symbol)
            }
            fn facts(&self, symbol: &str) -> SecData {
                FundCompanyData.facts(symbol)
            }
            fn fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
                FundCompanyData.fund_data(symbol)
            }
            fn sector_pe_snapshot(
                &self,
                session: chrono::NaiveDate,
            ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
                self.asked.lock().unwrap().push(session);
                FundCompanyData.sector_pe_snapshot(session)
            }
            fn sector_pe_history(
                &self,
                sector: &str,
            ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
                FundCompanyData.sector_pe_history(sector)
            }
        }

        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let data = RecordingSnapshotData {
            asked: std::sync::Mutex::new(Vec::new()),
        };
        let mut first = stock("VTI", 50.0, 9_750.0);
        first.asset_class = AssetClass::Etf;
        let mut second = stock("ITOT", 40.0, 7_800.0);
        second.asset_class = AssetClass::Etf;
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings_of(vec![first, second])),
            &data,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        let asked = data.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 1, "fetched once, memoized across both funds");
        let run_session = crate::market_clock::et_date_of(&run.created_at)
            .expect("created_at is RFC3339");
        assert_eq!(
            asked[0], run_session,
            "the snapshot date is the run's created_at ET session"
        );
    }

    /// Driven through the real lazy CIK path against a localhost mock, with the
    /// cancel flag left set from an earlier run: `reset_cancel` must run before
    /// any fetch, or the map silently bails to empty.
    #[test]
    fn the_cik_map_is_fetched_inside_the_slot_after_run_started_and_reset_cancel() {
        use crate::progress::{ProgressEvent, RecordingReporter};
        use crate::sec::LazyCikResolver;
        use crate::test_http::{Canned, MockHttp};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        struct LazySecData {
            sec: SecEdgarSource,
            cik: LazyCikResolver,
        }
        impl CompanyDataSource for LazySecData {
            fn financials(&self, symbol: &str) -> CompanyFinancials {
                StubCompanyData.financials(symbol)
            }
            fn facts(&self, symbol: &str) -> SecData {
                sec_company_facts(&self.cik, &self.sec, symbol)
            }
        }

        // The mock serves the ticker map, then one company-facts reply.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"0": {"cik_str": 320193, "ticker": "AAPL", "title": "Apple Inc."}}"#,
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"cik": 320193, "facts": {"us-gaap": {}}}"#,
            },
        ]);
        let cache_dir = tempfile::tempdir().unwrap();
        // A cancel flag still set from an earlier cancelled run.
        let cancel = Arc::new(AtomicBool::new(true));
        let recorder = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("slot-order", recorder.clone(), cancel.clone());
        let sec = SecEdgarSource::new()
            .unwrap()
            .with_base_url(&server.base_url)
            .with_context(ctx.clone());
        let data = LazySecData {
            sec,
            cik: LazyCikResolver::new(cache_dir.path().join("sec_company_tickers.json")),
        };
        assert!(!data.cik.is_loaded(), "construction loads nothing");
        assert_eq!(server.attempts(), 0, "construction fetches nothing");

        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings_of(vec![stock(
                "AAPL", 20.0, 3_900.0,
            )])),
            &data,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx,
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        assert!(!cancel.load(Ordering::Relaxed), "the slot claim reset the flag");
        assert!(data.cik.is_loaded(), "the map loaded during the run");
        assert_eq!(
            server.request_paths(),
            vec![
                "/files/company_tickers.json".to_string(),
                "/api/xbrl/companyfacts/CIK0000320193.json".to_string(),
            ],
            "the map fetch happened (not bailed) and resolved AAPL for the facts call"
        );
        assert!(
            !run.audit[0]
                .degraded_inputs
                .iter()
                .any(|g| g.contains("no CIK mapping")),
            "no spurious CIK gap: {:?}",
            run.audit[0].degraded_inputs
        );

        let msgs = recorder.messages();
        let run_started_seq = msgs
            .iter()
            .find(|m| matches!(m.event, ProgressEvent::RunStarted { .. }))
            .map(|m| m.seq)
            .expect("run_started emitted");
        let tickers_row = msgs
            .iter()
            .find(|m| {
                matches!(
                    &m.event,
                    ProgressEvent::RequestStarted { provider, group, .. }
                        if provider == "SEC" && group == "company-tickers"
                )
            })
            .expect("the ticker-map fetch emitted a request row");
        assert!(
            tickers_row.seq > run_started_seq,
            "ticker-map fetch (seq {}) must follow run_started (seq {run_started_seq})",
            tickers_row.seq
        );
        match &tickers_row.event {
            ProgressEvent::RequestStarted { step, .. } => assert!(
                step.is_some(),
                "the ticker-map request row is owned by the active step, not unattributed"
            ),
            _ => unreachable!(),
        }
    }

    /// Drives the real local daemon over the fixture holding plus live FMP and
    /// keyless SEC. Requires the daemon up with the configured roster, plus
    /// FMP_API_KEY; spends one FMP call:
    ///   `cargo test portfolio_live_smoke -- --ignored --nocapture`
    #[test]
    #[ignore = "hits the live local daemon + FMP/SEC; set MARKET_SIGNAL_LOCAL_* and FMP_API_KEY"]
    fn portfolio_live_smoke() {
        use crate::config::AppConfig;
        use crate::fmp::FmpDataSource;
        use crate::local_model::{self, DaemonProbe, LocalModelClient};
        use crate::portfolio::pipeline::LocalAnalyst;
        use crate::portfolio::VerdictDisposition;

        let cfg = AppConfig::from_env();
        let endpoint = local_model::endpoint_from_config(&cfg)
            .expect("MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT set");
        let roster = local_model::roster_from_config(&cfg);
        let client = LocalModelClient::new(&endpoint).expect("build local client");
        match client.probe_daemon(&roster) {
            DaemonProbe::Reachable { missing } if missing.is_empty() => {}
            other => panic!("local daemon/roster not ready for the smoke: {other:?}"),
        }
        let analyst =
            LocalAnalyst::new(client, roster.reasoner.clone(), roster.fast.clone());
        let fmp = FmpDataSource::new(cfg.fmp_api_key.clone().unwrap_or_default())
            .expect("build FMP source");
        let sec = SecEdgarSource::new().expect("build SEC source");
        let (_cik_dir, cik_cache) = {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("sec_company_tickers.json");
            (dir, path)
        };
        let cik = crate::sec::LazyCikResolver::new(cik_cache);
        let company = LiveCompanyData { fmp, sec, cik };

        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let start = std::time::Instant::now();
        let market = LiveMarketContext {
            fred: crate::fred::FredDataSource::from_env().expect("FRED_API_KEY set"),
            fmp: None,
            cot: None,
            cboe: None,
            finra: None,
        };
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &company,
            &market,
            &analyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .expect("job runs");
        let elapsed = start.elapsed();

        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        eprintln!(
            "portfolio live smoke: {} verdict(s) in {:.1}s\nroll-up: {}",
            run.verdicts.len(),
            elapsed.as_secs_f64(),
            run.roll_up.overview
        );
        for v in &run.verdicts {
            if let VerdictDisposition::Priced(g) = &v.disposition {
                eprintln!(
                    "  {} — grade {} action {:?} conviction {:?}\n    summary: {}",
                    v.symbol, g.grade.as_str(), g.action, g.appendix.conviction, g.thesis_document
                );
            } else {
                eprintln!("  {} — {:?}", v.symbol, v.disposition);
            }
        }
        assert_eq!(run.verdicts.len(), 1);
        assert!(
            matches!(run.verdicts[0].disposition, VerdictDisposition::Priced(_)),
            "the fixture equity should grade with live data"
        );
    }

    #[test]
    fn same_symbol_rows_net_into_one_book_level_verdict() {
        // `docs/schwab-integration.md` §What is pulled.
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings_of(vec![
                stock("AAPL", 100.0, 19_500.0),
                stock("AAPL", 50.0, 9_750.0),
            ])),
            &StubCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        assert_eq!(run.verdicts.len(), 1, "netted book-level rows, not per-account rows");
        assert_eq!(run.holdings.positions.len(), 1);
        assert_eq!(run.holdings.positions[0].quantity, 150.0);
        // The per-source rows survive on the snapshot for display and audit.
        assert_eq!(run.holdings.source_rows.len(), 2);
    }

    #[test]
    fn continuity_lookup_sees_the_prior_run_on_a_second_pass() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let run_once = || {
            run_portfolio_job(
                &FixtureHoldingsSource::new(),
                &StubCompanyData,
                &StubMarket,
                &StubAnalyst,
                &InvestorProfile::default_fixture(),
                None,
                None,
                None,
                &paths,
                &guard,
                &ctx(),
            )
            .unwrap()
        };
        let first = match run_once() {
            PortfolioJobOutcome::Successful(r) => *r,
            other => panic!("expected success, got {other:?}"),
        };
        let conn = storage::open(&paths.db_path).unwrap();
        let latest = store::latest_run(&conn).unwrap();
        assert!(dossier::prior_verdict_for(latest.as_ref(), "AAPL").is_some());
        let second = match run_once() {
            PortfolioJobOutcome::Successful(r) => *r,
            other => panic!("expected success, got {other:?}"),
        };
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM portfolio_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);

        // Run 2 reads run 1's document as PRIOR THESIS and supersedes it
        // (`docs/portfolio-analysis.md` §The holding verdict).
        let d1 = first.verdicts[0].thesis_document().expect("run 1 writes the debut document");
        let d2 = second.verdicts[0].thesis_document().expect("run 2 writes its own document");
        assert!(!d1.trim().is_empty() && !d2.trim().is_empty());
        assert!(
            d2.contains("Since the prior analysis"),
            "run 2's document is the continuity read: {d2}"
        );
        assert!(!d1.contains("Since the prior analysis"), "{d1}");
        // The stub's research writes no write-up, so run 1's analysis is the one
        // sentence and run 2 keeps it verbatim with no distillation
        // (`docs/portfolio-workflow.md` §Step 6d).
        let first_record = first.audit[0].analysis.as_ref().expect("run 1 writes the sentence");
        assert_eq!(first_record.text, crate::portfolio::distill::NO_WRITE_UP_SENTENCE);
        assert_eq!(first_record.anchor, first.audit[0].authoring_close);
        assert_eq!(second.audit[0].analysis.as_ref(), Some(first_record));
        assert_eq!(
            second.audit[0].research.as_ref().expect("run 2 ran the loop").distillation,
            crate::portfolio::distill::DistillationRecord::none()
        );
    }

    #[test]
    fn a_full_run_over_a_flagged_holding_clears_its_quick_check_state() {
        use crate::portfolio::quick_check::{
            AttentionFlag, FlagTrigger, HoldingQuickState, QuickCheckState,
        };

        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let run_once = || {
            match run_portfolio_job(
                &FixtureHoldingsSource::new(),
                &StubCompanyData,
                &StubMarket,
                &StubAnalyst,
                &InvestorProfile::default_fixture(),
                None,
                None,
                None,
                &paths,
                &guard,
                &ctx(),
            )
            .unwrap()
            {
                PortfolioJobOutcome::Successful(r) => *r,
                other => panic!("expected success, got {other:?}"),
            }
        };
        let first = run_once();
        let symbol = first.verdicts[0].symbol.clone();

        let conn = storage::open(&paths.db_path).unwrap();
        store::save_quick_check(
            &conn,
            &QuickCheckState {
                parameter_version:
                    crate::portfolio::quick_check::QUICK_CHECK_PARAMETER_VERSION.into(),
                swept_run_id: first.run_id.clone(),
                last_checked_at: "2026-08-03T00:00:00Z".into(),
                rate_cache: None,
                holdings: vec![HoldingQuickState {
                    symbol: symbol.clone(),
                    families: vec![],
                    flag: Some(AttentionFlag {
                        trigger: FlagTrigger::PriceOutsideBand,
                        detail: "price crossed outside the monitor band".into(),
                        raised_at: "2026-08-03T00:00:00Z".into(),
                    }),
                    evidence_events: vec![],
                    last_hurdle_state: None,
                    notes: vec![],
                }],
            },
        )
        .unwrap();

        let second = run_once();
        // Every holding analyzed afresh and none abstaining leaves nothing to
        // retain, so the store cleared.
        let fresh = second
            .verdicts
            .iter()
            .find(|v| v.symbol == symbol)
            .expect("the flagged holding took a fresh pass");
        assert_eq!(fresh.analyzed_at.as_deref(), Some(second.created_at.as_str()));
        assert!(store::latest_quick_check(&conn).unwrap().is_none());
    }

    #[test]
    fn an_abstaining_holding_retains_its_quick_check_state_through_a_full_run() {
        use crate::portfolio::quick_check::{
            AttentionFlag, FlagTrigger, HoldingQuickState, QuickCheckState,
        };

        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let book = || {
            FixtureHoldingsSource::with_holdings(holdings_of(vec![
                stock("AAA", 10.0, 1_000.0),
                stock("BBB", 10.0, 1_000.0),
            ]))
        };
        let run = |company: &dyn CompanyDataSource| {
            match run_portfolio_job(
                &book(),
                company,
                &StubMarket,
                &StubAnalyst,
                &InvestorProfile::default_fixture(),
                None,
                None,
                None,
                &paths,
                &guard,
                &ctx(),
            )
            .unwrap()
            {
                PortfolioJobOutcome::Successful(r) => *r,
                other => panic!("expected success, got {other:?}"),
            }
        };
        let first = run(&StubCompanyData);

        let conn = storage::open(&paths.db_path).unwrap();
        let entry = |symbol: &str| HoldingQuickState {
            symbol: symbol.into(),
            families: vec![],
            flag: Some(AttentionFlag {
                trigger: FlagTrigger::PriceOutsideBand,
                detail: "price crossed outside the monitor band".into(),
                raised_at: "2026-08-03T00:00:00Z".into(),
            }),
            evidence_events: vec![],
            last_hurdle_state: None,
            notes: vec![],
        };
        store::save_quick_check(
            &conn,
            &QuickCheckState {
                parameter_version:
                    crate::portfolio::quick_check::QUICK_CHECK_PARAMETER_VERSION.into(),
                swept_run_id: first.run_id.clone(),
                last_checked_at: "2026-08-03T00:00:00Z".into(),
                rate_cache: None,
                holdings: vec![entry("AAA"), entry("BBB")],
            },
        )
        .unwrap();

        // BBB's floor-bearing inputs are gone, so it exits `insufficient-evidence`.
        struct AbstainBbb;
        impl CompanyDataSource for AbstainBbb {
            fn financials(&self, symbol: &str) -> CompanyFinancials {
                if symbol == "BBB" {
                    CompanyFinancials {
                        symbol: symbol.to_string(),
                        ..CompanyFinancials::default()
                    }
                } else {
                    StubCompanyData.financials(symbol)
                }
            }
            fn facts(&self, symbol: &str) -> SecData {
                StubCompanyData.facts(symbol)
            }
        }
        let second = run(&AbstainBbb);
        assert!(
            second.verdicts.iter().any(|v| v.symbol == "BBB"
                && matches!(
                    v.disposition,
                    crate::portfolio::VerdictDisposition::InsufficientEvidence { .. }
                )),
            "BBB abstained this run"
        );
        let vintage_of = |sym: &str| {
            second
                .verdicts
                .iter()
                .find(|v| v.symbol == sym)
                .and_then(|v| v.analyzed_at.as_deref())
        };
        assert_eq!(vintage_of("BBB"), Some(first.created_at.as_str()));
        assert_eq!(vintage_of("AAA"), Some(second.created_at.as_str()));

        // An abstention is not a successful pass (`docs/portfolio-analysis.md`
        // §Evidence floor), so BBB's flag survives, re-stamped to the new run.
        let retained = store::latest_quick_check(&conn)
            .unwrap()
            .expect("the abstaining holding's state survives the full run");
        assert_eq!(retained.swept_run_id, second.run_id);
        assert_eq!(retained.holdings.len(), 1);
        assert_eq!(retained.holdings[0].symbol, "BBB");
        assert!(
            retained.holdings[0].flag.is_some(),
            "the attention flag survived the abstention"
        );
        // The retained sweep's rate cache follows the run's fresher prints.
        assert_eq!(retained.rate_cache, Some(second.rate_prints));
    }

    #[test]
    fn holdings_diff_tags_changes_and_surfaces_exits_across_runs() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let run = |source: &dyn HoldingsSource| {
            match run_portfolio_job(
                source,
                &StubCompanyData,
                &StubMarket,
                &StubAnalyst,
                &InvestorProfile::default_fixture(),
                None,
                None,
                None,
                &paths,
                &guard,
                &ctx(),
            )
            .unwrap()
            {
                PortfolioJobOutcome::Successful(r) => *r,
                other => panic!("expected success, got {other:?}"),
            }
        };

        // Run 1: every position is new, nothing exited.
        let first = run(&FixtureHoldingsSource::with_holdings(holdings_of(vec![
            stock("AAPL", 100.0, 19_500.0),
            stock("MSFT", 50.0, 20_000.0),
        ])));
        for v in &first.verdicts {
            assert_eq!(v.position_change, PositionChange::New, "{} on run 1", v.symbol);
        }
        assert!(first.roll_up.exited.is_empty());

        // Run 2: AAPL increased, MSFT sold out, NVDA new.
        let second = run(&FixtureHoldingsSource::with_holdings(holdings_of(vec![
            stock("AAPL", 140.0, 27_300.0),
            stock("NVDA", 30.0, 30_000.0),
        ])));
        let change = |sym: &str| {
            second
                .verdicts
                .iter()
                .find(|v| v.symbol == sym)
                .map(|v| v.position_change)
        };
        assert_eq!(change("AAPL"), Some(PositionChange::Increased));
        assert_eq!(change("NVDA"), Some(PositionChange::New));
        assert_eq!(change("MSFT"), None);
        assert_eq!(second.roll_up.exited.len(), 1);
        assert_eq!(second.roll_up.exited[0].symbol, "MSFT");
        assert_eq!(second.roll_up.exited[0].prior_quantity, 50.0);
        assert!(
            second.roll_up.overview.contains("MSFT"),
            "the exit is noted in the overview: {}",
            second.roll_up.overview
        );
    }

    // ---- Selective re-analysis (`docs/portfolio-analysis.md` §Triggering) ----

    /// Quiet by default (the stub's 195 price matches the authoring-time marks,
    /// so the transition-only band flag stays silent), with per-symbol overrides.
    /// Its `rates` leg is deliberately unreachable: the in-run sweep reads the
    /// run's own prints, never a second FRED call.
    #[derive(Default)]
    struct SelectiveQuickData {
        crash_price: Option<(&'static str, f64)>,
        fail_price: Option<&'static str>,
        earnings_date: Option<String>,
    }
    impl crate::portfolio::quick_check::QuickCheckDataSource for SelectiveQuickData {
        fn price_and_closes(
            &self,
            symbol: &str,
            _lookback_days: i64,
        ) -> Result<(f64, Vec<crate::portfolio::engine::DatedValue>)> {
            use crate::portfolio::engine::DatedValue;
            if self
                .fail_price
                .is_some_and(|s| s.eq_ignore_ascii_case(symbol))
            {
                anyhow::bail!("simulated price outage");
            }
            let price = match self.crash_price {
                Some((s, p)) if s.eq_ignore_ascii_case(symbol) => p,
                _ => 195.0,
            };
            let today = chrono::Utc::now().date_naive();
            Ok((
                price,
                vec![
                    // The full-run fixtures' anchor bars: a stub without the stamped
                    // anchor would fail-close every price comparison.
                    DatedValue { date: "2026-06-30".into(), value: 190.0 },
                    DatedValue { date: "2026-07-15".into(), value: 195.0 },
                    DatedValue {
                        date: (today - chrono::Duration::days(30))
                            .format("%Y-%m-%d")
                            .to_string(),
                        value: 190.0,
                    },
                    DatedValue {
                        date: today.format("%Y-%m-%d").to_string(),
                        value: price,
                    },
                ],
            ))
        }
        fn recent_filings(&self, _symbol: &str) -> crate::portfolio::quick_check::FilingSweep {
            crate::portfolio::quick_check::FilingSweep::Filings(vec![])
        }
        fn statements_refresh(&self, _symbol: &str) -> CompanyFinancials {
            CompanyFinancials::default()
        }
        fn consensus(
            &self,
            _symbol: &str,
        ) -> Result<Option<crate::portfolio::engine::ConsensusEstimate>> {
            Ok(Some(crate::portfolio::engine::ConsensusEstimate {
                eps_mid: Some(6.5),
                eps_periods: vec![crate::portfolio::engine::ConsensusEpsPeriod {
                    period_end: "2027-06-30".into(),
                    eps_mid: Some(6.5),
                    ntm_weight: 1.0,
                }],
                ..Default::default()
            }))
        }
        fn earnings(&self, _symbol: &str) -> Result<Vec<crate::fmp::SymbolEarningsRow>> {
            Ok(self
                .earnings_date
                .iter()
                .map(|d| crate::fmp::SymbolEarningsRow {
                    date: d.clone(),
                    eps_actual: Some(2.0),
                    eps_estimated: Some(1.9),
                    revenue_actual: None,
                })
                .collect())
        }
        fn fund_data(&self, _symbol: &str) -> crate::portfolio::fund::FundData {
            Default::default()
        }
        fn rates(
            &self,
        ) -> Result<(
            crate::portfolio::engine::DatedValue,
            crate::portfolio::engine::DatedValue,
        )> {
            anyhow::bail!("the in-run sweep reads the run's own prints, never FRED")
        }
    }

    fn two_stocks() -> Holdings {
        holdings_of(vec![stock("AAPL", 20.0, 3_900.0), stock("MSFT", 20.0, 3_900.0)])
    }

    // ---- Checkpoint / resume (`docs/portfolio-analysis.md` §Failure posture) ----

    /// Fails hard on one symbol, or on none ([`FailOn::recording`]). Instrumented
    /// like `LocalAnalyst`: every `research` records one usage row (the failing
    /// symbol's before it bails, the abandoned call a resume must not carry), and
    /// every `interpret` one usage row at `interpret_tokens` plus one fired-retry
    /// event.
    struct FailOn {
        symbol: Option<&'static str>,
        interpret_tokens: u64,
        prompt_usage: std::sync::Mutex<Vec<crate::local_model::PromptUsage>>,
        retries: std::sync::Mutex<Vec<crate::local_model::RetryEvent>>,
    }

    impl FailOn {
        /// Interpret rows fill past the context-pressure threshold (120 k of
        /// 128 k), so the pre-crash process leaves a pressure row behind.
        fn on(symbol: &'static str) -> Self {
            Self {
                symbol: Some(symbol),
                interpret_tokens: 120_000,
                prompt_usage: std::sync::Mutex::new(Vec::new()),
                retries: std::sync::Mutex::new(Vec::new()),
            }
        }
        /// Never fails; records at the given interpret fill.
        fn recording(interpret_tokens: u64) -> Self {
            Self {
                symbol: None,
                interpret_tokens,
                prompt_usage: std::sync::Mutex::new(Vec::new()),
                retries: std::sync::Mutex::new(Vec::new()),
            }
        }
        fn record_usage(&self, stage: String, prompt_tokens: u64) {
            self.prompt_usage
                .lock()
                .unwrap()
                .push(crate::local_model::PromptUsage {
                    stage,
                    num_ctx: 131_072,
                    prompt_chars: prompt_tokens * 4,
                    num_predict: None,
                    output_limited: false,
                    api: crate::local_model::ApiCounters {
                        prompt_eval_count: Some(prompt_tokens),
                        eval_count: Some(1_000),
                        ..Default::default()
                    },
                    ..Default::default()
                });
        }
    }

    impl crate::portfolio::pipeline::HoldingAnalyst for FailOn {
        fn research(
            &self,
            dossier: &crate::portfolio::dossier::HoldingDossier,
            plan: &crate::portfolio::research::ResearchPlan,
        ) -> Result<crate::portfolio::research::HoldingResearch> {
            let symbol = &dossier.position.symbol;
            self.record_usage(format!("research {symbol}"), 20_000);
            if self.symbol.is_some_and(|s| s == symbol) {
                anyhow::bail!("injected model failure on {symbol}");
            }
            Ok(crate::portfolio::research::offline_stub(plan))
        }
        fn interpret(
            &self,
            input: &crate::portfolio::pipeline::ThesisInput,
        ) -> Result<crate::portfolio::PricedModelArm> {
            let stage = format!("interpret {}", input.dossier.position.symbol);
            self.record_usage(stage.clone(), self.interpret_tokens);
            self.retries
                .lock()
                .unwrap()
                .push(crate::local_model::RetryEvent {
                    stage,
                    cause: "transport-level connection failure".into(),
                });
            crate::portfolio::pipeline::StubAnalyst.interpret(input)
        }
        fn take_prompt_usage(&self) -> Vec<crate::local_model::PromptUsage> {
            std::mem::take(&mut *self.prompt_usage.lock().unwrap())
        }
        fn take_retry_events(&self) -> Vec<crate::local_model::RetryEvent> {
            std::mem::take(&mut *self.retries.lock().unwrap())
        }
        fn interpret_role_risk(
            &self,
            input: &crate::portfolio::pipeline::RoleRiskInput,
        ) -> Result<String> {
            crate::portfolio::pipeline::StubAnalyst.interpret_role_risk(input)
        }
        fn decide_action(
            &self,
            input: &crate::portfolio::pipeline::ActionInput,
        ) -> Result<crate::portfolio::ActionDecision> {
            crate::portfolio::pipeline::StubAnalyst.decide_action(input)
        }
        fn fast_id(&self) -> String {
            crate::portfolio::pipeline::StubAnalyst.fast_id()
        }
        fn reasoner_id(&self) -> String {
            crate::portfolio::pipeline::StubAnalyst.reasoner_id()
        }
    }

    /// Panics on one symbol; can flip the cancel flag first, since a panic is
    /// never a user stop.
    struct PanicOn {
        symbol: &'static str,
        cancel_first: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    }

    impl crate::portfolio::pipeline::HoldingAnalyst for PanicOn {
        fn research(
            &self,
            dossier: &crate::portfolio::dossier::HoldingDossier,
            plan: &crate::portfolio::research::ResearchPlan,
        ) -> Result<crate::portfolio::research::HoldingResearch> {
            if dossier.position.symbol == self.symbol {
                if let Some(flag) = &self.cancel_first {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                panic!("injected panic on {}", self.symbol);
            }
            Ok(crate::portfolio::research::offline_stub(plan))
        }
        fn interpret(
            &self,
            input: &crate::portfolio::pipeline::ThesisInput,
        ) -> Result<crate::portfolio::PricedModelArm> {
            crate::portfolio::pipeline::StubAnalyst.interpret(input)
        }
        fn interpret_role_risk(
            &self,
            input: &crate::portfolio::pipeline::RoleRiskInput,
        ) -> Result<String> {
            crate::portfolio::pipeline::StubAnalyst.interpret_role_risk(input)
        }
        fn decide_action(
            &self,
            input: &crate::portfolio::pipeline::ActionInput,
        ) -> Result<crate::portfolio::ActionDecision> {
            crate::portfolio::pipeline::StubAnalyst.decide_action(input)
        }
        fn fast_id(&self) -> String {
            crate::portfolio::pipeline::StubAnalyst.fast_id()
        }
        fn reasoner_id(&self) -> String {
            crate::portfolio::pipeline::StubAnalyst.reasoner_id()
        }
    }

    #[test]
    fn a_mid_run_panic_is_contained_as_a_failed_run_with_a_resumable_trail() {
        use crate::progress::{ProgressEvent, RecordingReporter};
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let (_dir, paths) = paths();
        let recorder = Arc::new(RecordingReporter::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let ctx = RunContext::new("panic-run", recorder.clone(), cancel.clone());
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(two_stocks()),
            &StubCompanyData,
            &StubMarket,
            &PanicOn {
                symbol: "MSFT",
                cancel_first: Some(cancel.clone()),
            },
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            &ctx,
        )
        .unwrap();
        // Never `Cancelled`, though the flag was set before the panic.
        let msg = match outcome {
            PortfolioJobOutcome::Failed(msg) => msg,
            other => panic!("expected a contained failure, got {other:?}"),
        };
        assert!(msg.starts_with("the analysis panicked: "), "{msg}");
        assert!(msg.contains("injected panic on MSFT"), "{msg}");
        assert!(
            cancel.load(std::sync::atomic::Ordering::SeqCst),
            "the stub set the cancel flag before panicking, so the cancel arm was live"
        );

        let conn = storage::open(&paths.db_path).unwrap();
        let (state, detail): (String, Option<String>) = conn
            .query_row(
                "SELECT state, detail FROM job_runs ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "failed");
        assert_eq!(detail.as_deref(), Some(msg.as_str()));
        let last = recorder.messages().pop().expect("the run emitted events");
        match last.event {
            ProgressEvent::RunFinished { status, detail, .. } => {
                assert_eq!(status, "failed");
                assert_eq!(detail.as_deref(), Some(msg.as_str()));
            }
            other => panic!("the last event must be the terminal one, got {other:?}"),
        }

        assert!(store::latest_run(&conn).unwrap().is_none(), "no partial run persists");
        let cp = store::load_checkpoint(&conn)
            .unwrap()
            .expect("checkpoints survive the panic");
        assert_eq!(cp.holdings.len(), 1, "AAPL completed before the MSFT panic");
        assert_eq!(cp.holdings[0].verdict.symbol, "AAPL");
        let ids = vec!["stub-analyst".to_string(), "stub-analyst".to_string()];
        resume_eligibility(&conn, &cp, &ids, chrono::Utc::now())
            .expect("a fresh checkpoint under the same versions is resumable");
    }

    struct NoPullSource;

    impl crate::schwab::HoldingsSource for NoPullSource {
        fn holdings(&self) -> Result<Holdings> {
            anyhow::bail!("a resume must reopen the pinned pull, never pull fresh")
        }
        // Only the Step-2 pull is pinned; chain fetches run live on a resumed holding.
        fn option_chain(&self, _symbol: &str) -> Result<Option<crate::schwab::OptionChain>> {
            Ok(None)
        }
    }

    #[test]
    fn a_mid_run_cancel_checkpoints_and_resume_completes_without_a_pull() {
        let (_dir, paths) = paths();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(two_stocks()),
            &StubCompanyData,
            &StubMarket,
            &FailOn::on("MSFT"),
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            // AAPL completes and checkpoints; MSFT is cancelled before its model call.
            &cancel_ctx("Analyze MSFT"),
        )
        .unwrap();
        assert!(matches!(outcome, PortfolioJobOutcome::Cancelled(_)), "{outcome:?}");
        let conn = storage::open(&paths.db_path).unwrap();
        assert!(store::latest_run(&conn).unwrap().is_none(), "no partial run persists");

        let cp = store::load_checkpoint(&conn)
            .unwrap()
            .expect("checkpoints survive the cancel");
        assert_eq!(cp.holdings.len(), 1, "AAPL completed before the MSFT cancel");
        assert_eq!(cp.holdings[0].verdict.symbol, "AAPL");
        assert!(cp.header.work_list.is_none(), "a whole-book run pins no selection");
        assert!(
            cp.accumulators.sector_by_symbol.contains_key("AAPL"),
            "the accumulators carry the completed holding's sector identity"
        );
        // The cancelled holding runs no model call, so it reaches no row.
        let stages: Vec<&str> = cp.holdings[0]
            .prompt_usage
            .iter()
            .map(|u| u.stage.as_str())
            .collect();
        assert_eq!(stages, ["research AAPL", "interpret AAPL"], "{stages:?}");
        assert_eq!(cp.holdings[0].model_retries.len(), 1, "{:?}", cp.holdings[0].model_retries);
        assert_eq!(cp.holdings[0].model_retries[0].stage, "interpret AAPL");

        let ids = vec!["stub-analyst".to_string(), "stub-analyst".to_string()];
        resume_eligibility(&conn, &cp, &ids, chrono::Utc::now())
            .expect("a fresh checkpoint under the same versions is resumable");

        // A source that refuses to pull proves no fresh pull happens.
        let pinned_run_id = cp.header.run_id.clone();
        let pinned_created = cp.header.created_at.clone();
        // The resumed process records at a smaller fill, so the merge order
        // across both processes is pinned.
        let outcome = run_portfolio_job(
            &NoPullSource,
            &StubCompanyData,
            &StubMarket,
            &FailOn::recording(60_000),
            &InvestorProfile::default_fixture(),
            None,
            None,
            Some(cp),
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected a successful resume, got {other:?}"),
        };
        assert_eq!(run.run_id, pinned_run_id, "resume reopens the interrupted run's id");
        assert_eq!(
            run.created_at, pinned_created,
            "the finished run is stamped with the pinned pull's as-of"
        );
        assert_eq!(run.verdicts.len(), 2, "restored + resumed holdings");
        for v in &run.verdicts {
            assert_eq!(
                v.analyzed_at.as_deref(),
                Some(pinned_created.as_str()),
                "{}: every fresh verdict carries the pinned vintage",
                v.symbol
            );
        }
        // The data-health read spans both processes in order.
        let dh = &run.roll_up.data_health;
        assert_eq!(
            dh.prompt_usage
                .iter()
                .map(|u| u.stage.as_str())
                .collect::<Vec<_>>(),
            [
                "research AAPL",
                "interpret AAPL",
                "research MSFT",
                "interpret MSFT"
            ]
        );
        let loaded = store::latest_run(&conn).unwrap().unwrap();
        assert_eq!(loaded.roll_up.data_health.prompt_usage, dh.prompt_usage);
        let retry_stages: Vec<&str> = dh.model_retries.iter().map(|r| r.stage.as_str()).collect();
        assert_eq!(
            retry_stages,
            ["interpret AAPL", "interpret MSFT"],
            "{retry_stages:?}"
        );
        let peak = dh.peak_prompt.as_ref().expect("a peak is recorded");
        assert_eq!(
            (peak.stage.as_str(), peak.api.prompt_eval_count),
            ("interpret AAPL", Some(120_000))
        );
        let pressure: Vec<&str> = dh
            .context_pressure
            .iter()
            .map(|u| u.stage.as_str())
            .collect();
        assert_eq!(pressure, ["interpret AAPL"], "{pressure:?}");
        assert!(dh.attention, "pressure and a fired retry are attention triggers: {}", dh.summary);
        assert!(store::load_checkpoint(&conn).unwrap().is_none());
    }

    #[test]
    fn a_new_run_discards_the_interrupted_runs_checkpoints() {
        let (_dir, paths) = paths();
        run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(two_stocks()),
            &StubCompanyData,
            &StubMarket,
            &FailOn::on("MSFT"),
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            // AAPL completes and checkpoints; MSFT is cancelled before its model call.
            &cancel_ctx("Analyze MSFT"),
        )
        .unwrap();
        let conn = storage::open(&paths.db_path).unwrap();
        let stale = store::load_checkpoint(&conn).unwrap().expect("trail exists");

        // A new run discards the trail at entry and clears its own on success.
        let run = full_run(&paths, two_stocks());
        assert!(store::load_checkpoint(&conn).unwrap().is_none());

        let ids = vec!["stub-analyst".to_string(), "stub-analyst".to_string()];
        let err = resume_eligibility(&conn, &stale, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("a newer run has persisted"), "{err}");
        assert!(!run.verdicts.is_empty());
    }

    #[test]
    fn resume_eligibility_refuses_expiry_and_version_drift() {
        let (_dir, paths) = paths();
        run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(two_stocks()),
            &StubCompanyData,
            &StubMarket,
            &FailOn::on("MSFT"),
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            // AAPL completes and checkpoints; MSFT is cancelled before its model call.
            &cancel_ctx("Analyze MSFT"),
        )
        .unwrap();
        let conn = storage::open(&paths.db_path).unwrap();
        let cp = store::load_checkpoint(&conn).unwrap().expect("trail exists");
        let ids = vec!["stub-analyst".to_string(), "stub-analyst".to_string()];

        let later = chrono::Utc::now() + chrono::Duration::hours(RESUME_WINDOW_HOURS + 1);
        let err = resume_eligibility(&conn, &cp, &ids, later).unwrap_err();
        assert!(err.contains("resume window"), "{err}");

        let err =
            resume_eligibility(&conn, &cp, &["other-model".to_string()], chrono::Utc::now())
                .unwrap_err();
        assert!(err.contains("roster"), "{err}");

        let mut drifted = cp.clone();
        drifted.header.prompt_version = "portfolio-v0".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("prompt/schema"), "{err}");

        let mut drifted = cp.clone();
        drifted.header.evidence_floor_version = "evidence-floor-v1".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("evidence-floor"), "{err}");
        let mut drifted = cp.clone();
        drifted.header.evidence_floor_version = "evidence-floor-v3".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("evidence-floor"), "{err}");

        let mut drifted = cp.clone();
        drifted.header.pre_profit_parameter_version = "pre-profit-v3".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("pre-profit"), "{err}");

        let mut drifted = cp.clone();
        drifted.header.quick_check_parameter_version = "quick-check-v1".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("quick-check evaluation"), "{err}");

        let mut drifted = cp.clone();
        drifted.header.checkpoint_format_version = "checkpoint-v6".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("checkpoint format"), "{err}");
        let mut drifted = cp.clone();
        drifted.header.checkpoint_format_version = "checkpoint-v1".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("checkpoint format"), "{err}");
        // Through the real loader: a header under another format loads with its
        // rows unread, and the gate refuses it with the format reason.
        conn.execute(
            "UPDATE portfolio_checkpoints SET header_json = ?1 WHERE run_id = ?2",
            rusqlite::params![
                serde_json::to_string(&drifted.header).unwrap(),
                cp.header.run_id
            ],
        )
        .unwrap();
        let loaded = store::load_checkpoint(&conn).unwrap().expect("the header still loads");
        assert_eq!(loaded.header.checkpoint_format_version, "checkpoint-v1");
        assert!(loaded.holdings.is_empty(), "rows under another format are not read");
        let err = resume_eligibility(&conn, &loaded, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("checkpoint format"), "{err}");
    }

    #[test]
    fn a_resume_counts_a_re_analyzed_holdings_deep_history_failure_once() {
        let (_dir, paths) = paths();
        let three = holdings_of(vec![
            stock("AAPL", 20.0, 3_900.0),
            stock("GOOG", 20.0, 3_900.0),
            stock("MSFT", 20.0, 3_900.0),
        ]);
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(three),
            &DegradedDeepHistoryData,
            &StubMarket,
            &FailOn::on("MSFT"),
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            // AAPL and GOOG complete and checkpoint; MSFT is cancelled before its
            // model call.
            &cancel_ctx("Analyze MSFT"),
        )
        .unwrap();
        assert!(matches!(outcome, PortfolioJobOutcome::Cancelled(_)), "{outcome:?}");
        let conn = storage::open(&paths.db_path).unwrap();
        let cp = store::load_checkpoint(&conn).unwrap().expect("trail exists");
        assert_eq!(cp.holdings.len(), 2, "AAPL and GOOG completed before MSFT was cancelled");
        assert!(
            cp.holdings.iter().all(|h| h.health.deep_history_failed),
            "each completed holding's row carries its own degraded fetch"
        );

        // AAPL's row stops reading: it loud-skips at load and re-analyzes.
        conn.execute(
            "UPDATE portfolio_checkpoint_holdings SET row_json = '{' WHERE symbol = 'AAPL'",
            [],
        )
        .unwrap();
        let cp = store::load_checkpoint(&conn).unwrap().expect("trail exists");
        assert_eq!(cp.holdings.len(), 1);
        assert_eq!(cp.holdings[0].verdict.symbol, "GOOG");
        let ids = vec!["stub-analyst".to_string(), "stub-analyst".to_string()];
        resume_eligibility(&conn, &cp, &ids, chrono::Utc::now()).expect("resumable");

        let outcome = run_portfolio_job(
            &NoPullSource,
            &DegradedDeepHistoryData,
            &StubMarket,
            &FailOn::recording(60_000),
            &InvestorProfile::default_fixture(),
            None,
            None,
            Some(cp),
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected a successful resume, got {other:?}"),
        };
        assert_eq!(
            run.verdicts.len(),
            3,
            "restored GOOG + re-analyzed AAPL and MSFT"
        );
        let dh = &run.roll_up.data_health;
        assert_eq!(
            dh.prompt_usage
                .iter()
                .map(|u| u.stage.as_str())
                .collect::<Vec<_>>(),
            [
                "research GOOG",
                "interpret GOOG",
                "research AAPL",
                "interpret AAPL",
                "research MSFT",
                "interpret MSFT"
            ]
        );
        let aapl = dh
            .prompt_usage
            .iter()
            .find(|u| u.stage == "interpret AAPL")
            .unwrap();
        assert_eq!(
            aapl.api.prompt_eval_count,
            Some(60_000),
            "superseded pre-crash AAPL calls are absent"
        );
        assert_eq!(
            dh.deep_history_failures, 3,
            "three holdings, each degraded once — never AAPL twice: {}",
            dh.summary
        );
    }

    /// Stocks resolve to Technology (SPDR benchmark `XLK`) and every deep-history
    /// fetch degrades, the benchmark's included.
    struct TechSectorDegradedData;
    impl CompanyDataSource for TechSectorDegradedData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            DegradedDeepHistoryData.financials(symbol)
        }
        fn facts(&self, symbol: &str) -> SecData {
            DegradedDeepHistoryData.facts(symbol)
        }
        fn deep_price_history(
            &self,
            symbol: &str,
        ) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
            DegradedDeepHistoryData.deep_price_history(symbol)
        }
        fn profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
            use crate::portfolio::listing::{ProfileIdentity, ProfileLookup};
            ProfileLookup::Resolved(ProfileIdentity {
                website: None,
                currency: Some("USD".into()),
                is_adr: Some(false),
                company_name: Some(format!("{symbol} Inc.")),
                exchange: Some("NASDAQ".into()),
                sector: Some("Technology".into()),
                industry: None,
                beta: None,
            })
        }
    }

    #[test]
    fn a_resume_counts_a_benchmark_failing_in_both_processes_once() {
        let (_dir, paths) = paths();
        // A prior run, so each stock's pre-flag read reaches the benchmark at all.
        let prior = full_run(&paths, two_stocks());
        assert_eq!(prior.verdicts.len(), 2);

        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(two_stocks()),
            &TechSectorDegradedData,
            &StubMarket,
            &FailOn::on("MSFT"),
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            // AAPL completes and checkpoints; MSFT is cancelled before its model call.
            &cancel_ctx("Analyze MSFT"),
        )
        .unwrap();
        assert!(matches!(outcome, PortfolioJobOutcome::Cancelled(_)), "{outcome:?}");
        let conn = storage::open(&paths.db_path).unwrap();
        let cp = store::load_checkpoint(&conn).unwrap().expect("trail exists");
        assert_eq!(cp.holdings.len(), 1);
        assert_eq!(cp.holdings[0].verdict.symbol, "AAPL");
        assert_eq!(
            cp.holdings[0].health.benchmark_gap.as_deref(),
            Some("XLK"),
            "the row carries the benchmark it read as unavailable"
        );
        let ids = vec!["stub-analyst".to_string(), "stub-analyst".to_string()];
        resume_eligibility(&conn, &cp, &ids, chrono::Utc::now()).expect("resumable");

        let outcome = run_portfolio_job(
            &NoPullSource,
            &TechSectorDegradedData,
            &StubMarket,
            &FailOn::recording(60_000),
            &InvestorProfile::default_fixture(),
            None,
            None,
            Some(cp),
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected a successful resume, got {other:?}"),
        };
        assert_eq!(run.verdicts.len(), 2, "restored AAPL + resumed MSFT");
        let dh = &run.roll_up.data_health;
        assert_eq!(
            dh.benchmark_gaps, 1,
            "one benchmark, read unavailable by both halves — never twice: {}",
            dh.summary
        );
        assert_eq!(dh.deep_history_failures, 2, "{}", dh.summary);
    }

    fn full_run(paths: &ReportPaths, holdings: Holdings) -> PortfolioRun {
        match run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings),
            &StubCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap()
        {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn a_per_holding_failure_isolates_and_carries_the_prior_verdict() {
        let (_dir, paths) = paths();
        let prior = full_run(&paths, two_stocks());
        let prior_vintage = prior
            .verdicts
            .iter()
            .find(|v| v.symbol == "MSFT")
            .expect("prior MSFT verdict")
            .analyzed_at
            .clone();

        let run = match run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(two_stocks()),
            &StubCompanyData,
            &StubMarket,
            &FailOn::on("MSFT"),
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap()
        {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("a single per-holding failure must not fail the run: {other:?}"),
        };

        assert_eq!(run.failed_holdings.len(), 1, "{:?}", run.failed_holdings);
        assert_eq!(run.failed_holdings[0].symbol, "MSFT");
        assert!(
            run.failed_holdings[0].carried_prior,
            "MSFT had a prior verdict to carry"
        );
        assert!(
            run.failed_holdings[0].cause.contains("injected model failure"),
            "the root cause reaches the card, not only the outer stage: {}",
            run.failed_holdings[0].cause
        );
        assert_eq!(run.roll_up.failed_count, 1);

        let aapl = run
            .verdicts
            .iter()
            .find(|v| v.symbol == "AAPL")
            .expect("AAPL fresh verdict");
        assert_eq!(aapl.analyzed_at.as_deref(), Some(run.created_at.as_str()));
        let msft = run
            .verdicts
            .iter()
            .find(|v| v.symbol == "MSFT")
            .expect("MSFT carried verdict shown, not emptied");
        assert_eq!(
            msft.analyzed_at, prior_vintage,
            "MSFT keeps its prior vintage — the card shows stale data, not this run's"
        );
    }

    #[test]
    fn a_debut_holding_failure_isolates_with_no_carry() {
        let (_dir, paths) = paths();
        let run = match run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(two_stocks()),
            &StubCompanyData,
            &StubMarket,
            &FailOn::on("MSFT"),
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap()
        {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("a debut failure must not fail the run: {other:?}"),
        };

        assert_eq!(run.failed_holdings.len(), 1);
        assert_eq!(run.failed_holdings[0].symbol, "MSFT");
        assert!(!run.failed_holdings[0].carried_prior, "no prior to carry");
        let observations = &run.roll_up.data_health.prompt_usage;
        assert_eq!(
            observations
                .iter()
                .map(|u| u.stage.as_str())
                .collect::<Vec<_>>(),
            ["research AAPL", "interpret AAPL", "research MSFT"]
        );
        let conn = storage::open(&paths.db_path).unwrap();
        assert!(store::load_checkpoint(&conn).unwrap().is_none());
        assert_eq!(
            &store::latest_run(&conn)
                .unwrap()
                .unwrap()
                .roll_up
                .data_health
                .prompt_usage,
            observations
        );
        assert_eq!(run.roll_up.failed_count, 1);
        assert!(run.verdicts.iter().any(|v| v.symbol == "AAPL"), "AAPL analyzed");
        assert!(
            !run.verdicts.iter().any(|v| v.symbol == "MSFT"),
            "a debut failure carries no verdict"
        );
    }

    #[test]
    fn every_attempted_holding_failing_fails_the_run() {
        let (_dir, paths) = paths();
        let one = holdings_of(vec![stock("MSFT", 20.0, 3_900.0)]);
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(one),
            &StubCompanyData,
            &StubMarket,
            &FailOn::on("MSFT"),
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            &paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap();
        assert!(matches!(outcome, PortfolioJobOutcome::Failed(_)), "{outcome:?}");
        let conn = storage::open(&paths.db_path).unwrap();
        assert!(
            store::latest_run(&conn).unwrap().is_none(),
            "an all-failed run persists no snapshot"
        );
    }

    /// [`full_run`] against a caller-supplied company source.
    fn run_with_company(
        paths: &ReportPaths,
        holdings: Holdings,
        company: &dyn CompanyDataSource,
    ) -> PortfolioRun {
        match run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings),
            company,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            None,
            None,
            paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap()
        {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        }
    }

    fn selective_run(
        paths: &ReportPaths,
        holdings: Holdings,
        selected: &[&str],
        quick: &SelectiveQuickData,
    ) -> PortfolioRun {
        match run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings),
            &StubCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            Some(SelectiveRun {
                selected: selected.iter().map(|s| s.to_string()).collect(),
                quick_data: quick,
            }),
            None,
            None,
            paths,
            &RunGuard::default(),
            &ctx(),
        )
        .unwrap()
        {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        }
    }

    // ---- The accuracy record (`docs/portfolio-analysis.md` §Outcome learning) ----

    /// The stub's own closes (the anchor bar a run stamps is the 2026-06-30
    /// close, 190), extended with weekday closes at `close` through the
    /// request's end, every bar times `rebase` as a split-adjusted series would
    /// serve them.
    struct AnchoredPrices {
        close: f64,
        rebase: f64,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl AnchoredPrices {
        fn at(close: f64) -> Self {
            Self {
                close,
                rebase: 1.0,
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    impl crate::portfolio::outcome::OutcomePriceSource for AnchoredPrices {
        fn daily_closes(
            &self,
            symbol: &str,
            _from: chrono::NaiveDate,
            to: chrono::NaiveDate,
        ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
            use chrono::Datelike;
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut out = StubCompanyData.financials(symbol).daily_closes;
            let mut d = chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
            while d <= to {
                if d.weekday().number_from_monday() <= 5 {
                    out.push(crate::portfolio::engine::DatedValue {
                        date: d.format("%Y-%m-%d").to_string(),
                        value: self.close,
                    });
                }
                d += chrono::Duration::days(1);
            }
            for bar in &mut out {
                bar.value *= self.rebase;
            }
            Ok(out)
        }
    }

    /// Runs the whole-book job at a stepped instant on the test clock.
    fn run_at(
        paths: &ReportPaths,
        holdings: Holdings,
        at: &str,
        prices: &dyn crate::portfolio::outcome::OutcomePriceSource,
        selective: Option<SelectiveRun<'_>>,
    ) -> PortfolioRun {
        run_with(paths, holdings, at, prices, selective, &StubCompanyData, &StubAnalyst)
    }

    fn run_with(
        paths: &ReportPaths,
        holdings: Holdings,
        at: &str,
        prices: &dyn crate::portfolio::outcome::OutcomePriceSource,
        selective: Option<SelectiveRun<'_>>,
        company: &dyn CompanyDataSource,
        analyst: &dyn HoldingAnalyst,
    ) -> PortfolioRun {
        TEST_NOW.with(|t| *t.borrow_mut() = Some(at.to_string()));
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings),
            company,
            &StubMarket,
            analyst,
            &InvestorProfile::default_fixture(),
            selective,
            Some(prices),
            None,
            paths,
            &RunGuard::default(),
            &ctx(),
        );
        TEST_NOW.with(|t| *t.borrow_mut() = None);
        match outcome.unwrap() {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        }
    }

    fn priced(v: &HoldingVerdict) -> &crate::portfolio::GradedVerdict {
        match &v.disposition {
            crate::portfolio::VerdictDisposition::Priced(g) => g,
            other => panic!("expected a priced verdict, got {other:?}"),
        }
    }

    #[test]
    fn a_two_run_sequence_scores_a_due_horizon_for_both_arms() {
        use crate::portfolio::outcome::{CheckOutcome, Horizon};
        let (_dir, paths) = paths();
        let prices = AnchoredPrices::at(200.0);

        let first = run_at(&paths, two_stocks(), "2026-07-06T15:00:00Z", &prices, None);
        assert_eq!(first.accuracy.opened.len(), 2, "a debut opens one episode per priced holding");
        let aapl = &first.accuracy.opened.iter().find(|e| e.record.symbol == "AAPL").unwrap().record;
        let g = priced(verdict(&first, "AAPL"));
        assert_eq!(aapl.created_on, "2026-07-06");
        assert_eq!(aapl.spot, 195.0);
        assert_eq!(aapl.anchor.as_ref().map(|a| (a.date.as_str(), a.value)), Some(("2026-06-30", 190.0)));
        assert_eq!(aapl.model.three_month, g.appendix.expected_price_3m);
        assert_eq!(aapl.engine.three_month, g.price_targets.three_month.as_ref().map(|t| t.base));
        assert_ne!(aapl.model.three_month, aapl.engine.three_month, "the stub arms differ");
        assert!(first.accuracy.checks.is_empty(), "nothing is due on a debut");
        assert_eq!(prices.calls.load(std::sync::atomic::Ordering::SeqCst), 0, "no pull with nothing due");
        let first_scores = first.accuracy.scores.as_ref().unwrap();
        assert_eq!(first_scores["AAPL"], Default::default(), "no score yet");

        // Three months and a few days later the three-month horizon (2026-10-06)
        // is due; the twelve-month and three-year horizons are not.
        let second = run_at(&paths, two_stocks(), "2026-10-09T15:00:00Z", &prices, None);
        assert_eq!(second.accuracy.checks.len(), 2, "{:?}", second.accuracy.checks);
        for check in &second.accuracy.checks {
            assert_eq!(check.check.horizon, Horizon::ThreeMonth);
            assert_eq!(check.check.checked_on, "2026-10-09");
            assert_eq!(check.check.run_id, second.run_id);
            match &check.check.outcome {
                CheckOutcome::Scored { close, bridge_factor, model, engine } => {
                    assert_eq!(close.date, "2026-10-06");
                    assert_eq!(close.value, 200.0);
                    assert_eq!(*bridge_factor, 1.0);
                    assert!(model.is_some() && engine.is_some());
                }
                other => panic!("expected a scored check, got {other:?}"),
            }
        }
        let scores = &second.accuracy.scores.as_ref().unwrap()["AAPL"];
        let expected = |p: Option<f64>| crate::portfolio::outcome::per_check_score(p.unwrap(), 200.0);
        let model = scores.three_month.model.as_ref().expect("the model arm scored");
        let engine = scores.three_month.engine.as_ref().expect("the engine arm scored");
        assert!((model.score - expected(aapl.model.three_month)).abs() < 1e-9);
        assert!((engine.score - expected(aapl.engine.three_month)).abs() < 1e-9);
        assert_ne!(model.score, engine.score);
        assert_eq!(model.last_moved_on, "2026-10-09");
        assert!(scores.twelve_month.model.is_none() && scores.three_year.engine.is_none());
        // The monthly cadence opens this run's episodes too.
        assert_eq!(second.accuracy.opened.len(), 2);
        assert!(!second.roll_up.data_health.accuracy_gap);

        let conn = storage::open(&paths.db_path).unwrap();
        let persisted = store::latest_run(&conn).unwrap().unwrap();
        assert_eq!(persisted.accuracy, second.accuracy);
        let all = store::load_episode_store(&conn).unwrap();
        assert_eq!((all.episodes.len(), all.checks.len()), (4, 2));
    }

    /// Dated closes running from the stub's quarter ends to the session before
    /// the test clock's, so the self-review reads the horizon closes the
    /// accuracy pass reads.
    struct ClosesThroughToday;

    impl CompanyDataSource for ClosesThroughToday {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            StubCompanyData.financials(symbol)
        }
        fn facts(&self, symbol: &str) -> SecData {
            StubCompanyData.facts(symbol)
        }
        fn deep_price_history(&self, symbol: &str) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
            use chrono::Datelike;
            let today = crate::market_clock::et_date_of(&now_rfc3339()).expect("the test clock");
            let mut out = StubCompanyData.financials(symbol).daily_closes;
            let mut d = chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
            while d < today {
                if d.weekday().number_from_monday() <= 5 {
                    out.push(crate::portfolio::engine::DatedValue {
                        date: d.format("%Y-%m-%d").to_string(),
                        value: 200.0,
                    });
                }
                d += chrono::Duration::days(1);
            }
            (out, Vec::new())
        }
    }

    /// The stub analyst, recording each review message by symbol.
    #[derive(Default)]
    struct ReviewRecorder(std::cell::RefCell<Vec<(String, String)>>);

    impl HoldingAnalyst for ReviewRecorder {
        fn review(&self, input: &crate::portfolio::review::ReviewInput) -> Result<String> {
            self.0.borrow_mut().push((
                input.dossier.position.symbol.clone(),
                crate::portfolio::review::review_prompt(input).user,
            ));
            Ok(crate::portfolio::review::stub_review(input))
        }
        fn interpret(&self, input: &crate::portfolio::pipeline::ThesisInput) -> Result<crate::portfolio::PricedModelArm> {
            StubAnalyst.interpret(input)
        }
        forward_stub_verdict_calls!();
    }

    fn review_of(recorder: &ReviewRecorder, symbol: &str) -> String {
        let reviews = recorder.0.borrow();
        let matching: Vec<&String> = reviews.iter().filter(|(s, _)| s == symbol).map(|(_, r)| r).collect();
        assert_eq!(matching.len(), 1, "one review for {symbol}");
        matching[0].clone()
    }

    fn audit_for<'a>(run: &'a PortfolioRun, symbol: &str) -> &'a HoldingAudit {
        run.audit.iter().find(|a| a.symbol == symbol).expect("the audit row")
    }

    struct BetaProfileData;

    impl CompanyDataSource for BetaProfileData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            StubCompanyData.financials(symbol)
        }
        fn facts(&self, symbol: &str) -> SecData {
            StubCompanyData.facts(symbol)
        }
        fn profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
            use crate::portfolio::listing::{ProfileIdentity, ProfileLookup};
            ProfileLookup::Resolved(ProfileIdentity {
                website: None,
                currency: Some("USD".into()),
                is_adr: Some(false),
                company_name: Some(format!("{symbol} Inc.")),
                exchange: Some("NASDAQ".into()),
                sector: Some("Technology".into()),
                industry: None,
                beta: (symbol == "AAPL").then_some(1.3),
            })
        }
    }

    #[derive(Default)]
    struct BetaRecorder(std::cell::RefCell<Vec<(String, Option<f64>)>>);

    impl HoldingAnalyst for BetaRecorder {
        fn interpret(&self, input: &crate::portfolio::pipeline::ThesisInput) -> Result<crate::portfolio::PricedModelArm> {
            self.0.borrow_mut().push((
                input.dossier.position.symbol.clone(),
                input.dossier.financials.beta,
            ));
            StubAnalyst.interpret(input)
        }
        forward_stub_verdict_calls!();
    }

    #[test]
    fn the_profiles_beta_reaches_the_stocks_financials() {
        let (_dir, paths) = paths();
        let recorder = BetaRecorder::default();
        run_with(
            &paths,
            two_stocks(),
            "2026-07-06T15:00:00Z",
            &AnchoredPrices::at(200.0),
            None,
            &BetaProfileData,
            &recorder,
        );
        let mut seen = recorder.0.borrow().clone();
        seen.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(seen, vec![("AAPL".to_string(), Some(1.3)), ("MSFT".to_string(), None)]);
    }

    #[test]
    fn a_continuity_run_reviews_the_prior_position_against_the_accuracy_record() {
        let (_dir, paths) = paths();
        let prices = AnchoredPrices::at(200.0);
        let run = |at: &str, recorder: &ReviewRecorder, selective: Option<SelectiveRun<'_>>| {
            run_with(&paths, two_stocks(), at, &prices, selective, &ClosesThroughToday, recorder)
        };

        let debut = ReviewRecorder::default();
        let first = run("2026-07-06T15:00:00Z", &debut, None);
        assert!(debut.0.borrow().is_empty(), "a debut reviews nothing");
        let aapl_first = audit_for(&first, "AAPL");
        assert_eq!((aapl_first.review.as_deref(), aapl_first.accuracy_read_through), (None, None));
        let prior_3m = priced(verdict(&first, "AAPL")).appendix.expected_price_3m.expect("a three-month price");

        let second_recorder = ReviewRecorder::default();
        let second = run("2026-10-09T15:00:00Z", &second_recorder, None);
        let review = review_of(&second_recorder, "AAPL");
        let score = crate::portfolio::outcome::per_check_score(prior_3m, 200.0);
        assert!(
            review.contains(&format!("- three months (2026-10-06): close $200.00 on 2026-10-06; score {score:.1}.\n")),
            "{review}"
        );
        // The prior was a debut, so it read no accuracy record at all.
        assert!(review.contains("The prior analysis read no accuracy record, so none of these was read before."), "{review}");
        assert!(review.contains("- forecast of 2026-07-06, three months (2026-10-06): close $200.00 on 2026-10-06; analyst $"), "{review}");
        assert!(review.contains("last moved 2026-10-09, not read by the prior analysis"), "{review}");
        let high_water = second.accuracy.checks.iter().map(|c| c.id).max();
        let aapl_second = audit_for(&second, "AAPL");
        assert!(aapl_second.review.as_deref().is_some_and(|r| r.starts_with("[stub: the review of the prior position on AAPL")));
        assert_eq!(aapl_second.accuracy_read_through, high_water);

        let quick = SelectiveQuickData::default();
        let third_recorder = ReviewRecorder::default();
        let third = run(
            "2026-10-12T15:00:00Z",
            &third_recorder,
            Some(SelectiveRun { selected: vec!["MSFT".to_string()], quick_data: &quick }),
        );
        let review = review_of(&third_recorder, "MSFT");
        assert!(
            review.contains("No check landed after the prior analysis was written: the scores above are the ones it read"),
            "{review}"
        );
        assert!(review.contains("last moved 2026-10-09, read by the prior analysis"), "{review}");
        assert!(third_recorder.0.borrow().iter().all(|(s, _)| s != "AAPL"), "the carried holding is not reviewed");
        assert_eq!(audit_for(&third, "AAPL").accuracy_read_through, high_water);
        assert_eq!(audit_for(&third, "MSFT").accuracy_read_through, high_water);
    }

    #[test]
    fn a_selective_run_scores_the_carried_tail_and_opens_only_fresh_episodes() {
        let (_dir, paths) = paths();
        let prices = AnchoredPrices::at(200.0);
        run_at(&paths, two_stocks(), "2026-07-06T15:00:00Z", &prices, None);
        let quick = SelectiveQuickData::default();
        let second = run_at(
            &paths,
            two_stocks(),
            "2026-10-09T15:00:00Z",
            &prices,
            Some(SelectiveRun {
                selected: vec!["AAPL".to_string()],
                quick_data: &quick,
            }),
        );
        let scores = second.accuracy.scores.as_ref().unwrap();
        assert!(scores["MSFT"].three_month.engine.is_some(), "the carried holding is scored");
        let opened: Vec<&str> = second.accuracy.opened.iter().map(|e| e.record.symbol.as_str()).collect();
        assert_eq!(opened, vec!["AAPL"], "a carried verdict opens no episode");
    }

    // ---- The factor-distribution store (`docs/storage.md` §Local Analysis Suite
    // Storage) ----

    /// [`StubCompanyData`] behind a resolved Technology profile, so the store
    /// can bucket the name.
    struct TechProfileData;
    impl CompanyDataSource for TechProfileData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            StubCompanyData.financials(symbol)
        }
        fn facts(&self, symbol: &str) -> SecData {
            StubCompanyData.facts(symbol)
        }
        fn profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
            use crate::portfolio::listing::{ProfileIdentity, ProfileLookup};
            ProfileLookup::Resolved(ProfileIdentity {
                website: None,
                currency: Some("USD".into()),
                is_adr: Some(false),
                company_name: Some(format!("{symbol} Inc.")),
                exchange: Some("NASDAQ".into()),
                sector: Some("Technology".into()),
                industry: None,
                beta: None,
            })
        }
    }

    /// One stored factor observation: issuer, factor, sector, cap band, value,
    /// floored, observed on.
    type ObservationRow = (String, String, String, String, Option<f64>, bool, String);

    fn factor_observations(paths: &ReportPaths) -> Vec<ObservationRow> {
        let conn = storage::open(&paths.db_path).unwrap();
        storage::init_schema(&conn).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT issuer, factor, sector, cap_band, value, floored, observed_on
                 FROM factor_observations ORDER BY issuer, factor",
            )
            .unwrap();
        stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    /// Each audit's resolved readings in the Technology mega-cap bucket on `on`.
    fn expected_observations(audits: &[&HoldingAudit], on: &str) -> Vec<ObservationRow> {
        use crate::portfolio::factor_store::{factor_key, resolved_readings, FactorReading};
        let mut rows: Vec<ObservationRow> = audits
            .iter()
            .flat_map(|audit| {
                let composite = audit.factors.as_ref().expect("a priced stock's composite");
                resolved_readings(composite).into_iter().map(|(factor, reading)| {
                    let (value, floored) = match reading {
                        FactorReading::Level(v) => (Some(v), false),
                        FactorReading::Floored => (None, true),
                    };
                    (
                        audit.symbol.clone(),
                        factor_key(factor).unwrap(),
                        "Technology".to_string(),
                        "mega".to_string(),
                        value,
                        floored,
                        on.to_string(),
                    )
                })
            })
            .collect();
        rows.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        rows
    }

    #[test]
    fn a_fresh_pass_contributes_its_factor_readings_to_the_shared_store() {
        let (_dir, paths) = paths();
        let prices = AnchoredPrices::at(200.0);
        let run = run_with(
            &paths,
            two_stocks(),
            "2026-07-06T15:00:00Z",
            &prices,
            None,
            &TechProfileData,
            &StubAnalyst,
        );
        let audits: Vec<&HoldingAudit> = run.audit.iter().collect();
        assert_eq!(audits.len(), 2);
        let expected = expected_observations(&audits, "2026-07-06");
        assert!(!expected.is_empty());
        assert_eq!(factor_observations(&paths), expected);
    }

    #[test]
    fn a_failed_factor_store_write_never_fails_the_run() {
        let (_dir, paths) = paths();
        {
            let conn = storage::open(&paths.db_path).unwrap();
            storage::init_schema(&conn).unwrap();
            conn.execute_batch(
                "CREATE TRIGGER factor_store_offline BEFORE INSERT ON factor_observations
                 BEGIN SELECT RAISE(ABORT, 'store offline'); END;",
            )
            .unwrap();
        }
        let prices = AnchoredPrices::at(200.0);
        let run = run_with(
            &paths,
            two_stocks(),
            "2026-07-06T15:00:00Z",
            &prices,
            None,
            &TechProfileData,
            &StubAnalyst,
        );
        assert_eq!(run.verdicts.len(), 2);
        assert!(run.failed_holdings.is_empty());
        let conn = storage::open(&paths.db_path).unwrap();
        assert_eq!(store::latest_run(&conn).unwrap().unwrap().run_id, run.run_id);
        assert!(factor_observations(&paths).is_empty());
    }

    #[test]
    fn a_selective_run_contributes_only_its_fresh_passes() {
        let (_dir, paths) = paths();
        let prices = AnchoredPrices::at(200.0);
        run_with(
            &paths,
            two_stocks(),
            "2026-07-06T15:00:00Z",
            &prices,
            None,
            &TechProfileData,
            &StubAnalyst,
        );
        storage::open(&paths.db_path)
            .unwrap()
            .execute("DELETE FROM factor_observations", [])
            .unwrap();
        let quick = SelectiveQuickData::default();
        let second = run_with(
            &paths,
            two_stocks(),
            "2026-10-09T15:00:00Z",
            &prices,
            Some(SelectiveRun {
                selected: vec!["AAPL".to_string()],
                quick_data: &quick,
            }),
            &TechProfileData,
            &StubAnalyst,
        );
        let aapl: Vec<&HoldingAudit> =
            second.audit.iter().filter(|a| a.symbol == "AAPL").collect();
        assert_eq!(aapl.len(), 1);
        assert_eq!(
            factor_observations(&paths),
            expected_observations(&aapl, "2026-10-09"),
            "the carried MSFT contributes nothing"
        );
    }

    fn accuracy_store() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        conn
    }

    fn open_episode(conn: &Connection, symbol: &str, anchor: Option<f64>) -> i64 {
        use crate::portfolio::outcome::{HorizonPrices, PriceRecord};
        store::insert_episode(
            conn,
            &PriceRecord {
                symbol: symbol.into(),
                created_on: "2026-07-06".into(),
                spot: 195.0,
                anchor: anchor.map(|value| crate::portfolio::engine::DatedValue {
                    date: "2026-06-30".into(),
                    value,
                }),
                model: HorizonPrices {
                    three_month: Some(210.0),
                    twelve_month: Some(230.0),
                    three_year: None,
                },
                engine: HorizonPrices {
                    three_month: Some(200.0),
                    twelve_month: None,
                    three_year: None,
                },
            },
        )
        .unwrap()
        .id
    }

    fn session(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn a_failed_refresh_stays_pending_and_a_rerun_writes_nothing_twice() {
        use crate::portfolio::outcome::UnavailablePriceSource;
        let conn = accuracy_store();
        open_episode(&conn, "AAPL", Some(190.0));
        let symbols = vec!["AAPL".to_string()];
        let failed = run_accuracy_pass(&conn, &UnavailablePriceSource, &ctx(), "run-2", session("2026-10-09"), &symbols);
        assert!(failed.checks.is_empty());
        assert!(!failed.gap, "a failed refresh is the normal pending path, never a store gap");
        assert_eq!(failed.scores.unwrap()["AAPL"], Default::default());

        let prices = AnchoredPrices::at(200.0);
        let scored = run_accuracy_pass(&conn, &prices, &ctx(), "run-3", session("2026-10-12"), &symbols);
        assert_eq!(scored.checks.len(), 1);
        let again = run_accuracy_pass(&conn, &prices, &ctx(), "run-3", session("2026-10-12"), &symbols);
        assert_eq!(again.checks, scored.checks, "the re-run lists the run's checks, writing none");
        assert_eq!(prices.calls.load(std::sync::atomic::Ordering::SeqCst), 1, "nothing due, no pull");
        assert_eq!(store::load_episode_store(&conn).unwrap().checks.len(), 1);
    }

    struct EmptyPrices {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl crate::portfolio::outcome::OutcomePriceSource for EmptyPrices {
        fn daily_closes(
            &self,
            _symbol: &str,
            _from: chrono::NaiveDate,
            _to: chrono::NaiveDate,
        ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    #[test]
    fn an_empty_served_series_is_unscorable_at_once_and_never_pulled_again() {
        use crate::portfolio::outcome::{CheckOutcome, UnscorableCause};
        let conn = accuracy_store();
        open_episode(&conn, "GONE", Some(190.0));
        let prices = EmptyPrices {
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let symbols = vec!["GONE".to_string()];
        let pass = run_accuracy_pass(&conn, &prices, &ctx(), "run-2", session("2026-10-09"), &symbols);
        assert_eq!(
            pass.checks[0].check.outcome,
            CheckOutcome::Unscorable { cause: UnscorableCause::NoCloseInProximity }
        );
        assert!(!pass.gap);
        let again = run_accuracy_pass(&conn, &prices, &ctx(), "run-3", session("2026-10-12"), &symbols);
        assert!(again.checks.is_empty());
        assert_eq!(prices.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn an_unreadable_check_row_is_never_re_due() {
        let conn = accuracy_store();
        let id = open_episode(&conn, "AAPL", Some(190.0));
        conn.execute(
            "INSERT INTO portfolio_episode_checks (episode_id, horizon, run_id, check_json)
             VALUES (?1, 'three_month', 'run-2', '{not json')",
            [id],
        )
        .unwrap();
        let prices = AnchoredPrices::at(200.0);
        let pass = run_accuracy_pass(&conn, &prices, &ctx(), "run-3", session("2026-10-12"), &["AAPL".to_string()]);
        assert!(pass.checks.is_empty());
        assert!(!pass.gap);
        assert_eq!(prices.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn a_split_since_creation_is_bridged_before_scoring() {
        use crate::portfolio::outcome::CheckOutcome;
        let conn = accuracy_store();
        open_episode(&conn, "AAPL", Some(190.0));
        let prices = AnchoredPrices {
            rebase: 0.5,
            ..AnchoredPrices::at(200.0)
        };
        let pass = run_accuracy_pass(&conn, &prices, &ctx(), "run-2", session("2026-10-09"), &["AAPL".to_string()]);
        match &pass.checks[0].check.outcome {
            CheckOutcome::Scored { bridge_factor, close, model, engine } => {
                assert_eq!(*bridge_factor, 0.5);
                assert_eq!(close.value, 100.0);
                assert_eq!(model.unwrap().expected, 105.0);
                assert_eq!(engine.unwrap().score, 100.0, "the engine's 200 bridges to 100, a hit");
            }
            other => panic!("expected a scored check, got {other:?}"),
        }
    }

    #[test]
    fn an_anchorless_episode_spends_no_pull_and_a_store_error_is_a_gap() {
        use crate::portfolio::outcome::{CheckOutcome, UnscorableCause};
        let conn = accuracy_store();
        open_episode(&conn, "AAPL", None);
        let prices = AnchoredPrices::at(200.0);
        let pass = run_accuracy_pass(&conn, &prices, &ctx(), "run-2", session("2026-10-09"), &["AAPL".to_string()]);
        assert_eq!(prices.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(pass.checks.len(), 1, "only the three-month horizon is due");
        assert_eq!(
            pass.checks[0].check.outcome,
            CheckOutcome::Unscorable { cause: UnscorableCause::NoAnchor }
        );
        assert_eq!(pass.scores.unwrap()["AAPL"], Default::default(), "an unscorable check moves no score");

        conn.execute("DROP TABLE portfolio_episode_checks", []).unwrap();
        let broken = run_accuracy_pass(&conn, &prices, &ctx(), "run-3", session("2026-10-12"), &["AAPL".to_string()]);
        assert!(broken.gap);
        assert!(broken.scores.is_none(), "an unreadable store leaves the scores unavailable");
    }

    fn verdict<'a>(run: &'a PortfolioRun, symbol: &str) -> &'a HoldingVerdict {
        run.verdicts
            .iter()
            .find(|v| v.symbol.eq_ignore_ascii_case(symbol))
            .unwrap_or_else(|| panic!("{symbol} in run"))
    }

    /// Re-persists the latest run with one verdict doctored into the prior-run
    /// shape a selective test needs.
    fn doctor_latest_run(paths: &ReportPaths, symbol: &str, f: impl FnOnce(&mut HoldingVerdict)) {
        let conn = storage::open(&paths.db_path).unwrap();
        let mut run = store::latest_run(&conn).unwrap().unwrap();
        let v = run
            .verdicts
            .iter_mut()
            .find(|v| v.symbol.eq_ignore_ascii_case(symbol))
            .unwrap();
        f(v);
        run.run_id = format!("{}-d", run.run_id);
        run.created_at = now_rfc3339();
        store::insert_run(&conn, &run).unwrap();
    }

    fn doctor_latest_run_audit(
        paths: &ReportPaths,
        symbol: &str,
        f: impl FnOnce(&mut crate::portfolio::HoldingAudit),
    ) {
        let conn = storage::open(&paths.db_path).unwrap();
        let mut run = store::latest_run(&conn).unwrap().unwrap();
        let a = run
            .audit
            .iter_mut()
            .find(|a| a.symbol.eq_ignore_ascii_case(symbol))
            .unwrap();
        f(a);
        run.run_id = format!("{}-da", run.run_id);
        run.created_at = now_rfc3339();
        store::insert_run(&conn, &run).unwrap();
    }

    fn days_ago(n: i64) -> String {
        (chrono::Utc::now() - chrono::Duration::days(n)).to_rfc3339()
    }

    #[test]
    fn a_pre_v9_carried_verdict_now_carries_rather_than_re_analyzing() {
        // An unselected whole-book-era verdict carries like any other.
        let (_dir, paths) = paths();
        let first = full_run(&paths, two_stocks());
        doctor_latest_run_audit(&paths, "MSFT", |a| {
            a.prompt_version = "portfolio-v8".into();
        });
        let second = selective_run(
            &paths,
            two_stocks(),
            &["AAPL"],
            &SelectiveQuickData::default(),
        );
        let msft = verdict(&second, "MSFT");
        assert_eq!(
            msft.analyzed_at.as_deref(),
            Some(first.created_at.as_str()),
            "a whole-book-era verdict carries vintage-stamped, no longer force-included"
        );
    }

    #[test]
    fn a_selective_run_carries_the_unselected_tail_vintage_stamped() {
        let (_dir, paths) = paths();
        let first = full_run(&paths, two_stocks());
        let second = selective_run(
            &paths,
            two_stocks(),
            &["AAPL"],
            &SelectiveQuickData::default(),
        );
        assert_eq!(second.verdicts.len(), 2);
        assert_eq!(
            verdict(&second, "AAPL").analyzed_at.as_deref(),
            Some(second.created_at.as_str())
        );
        let msft = verdict(&second, "MSFT");
        assert_eq!(msft.analyzed_at.as_deref(), Some(first.created_at.as_str()));
        assert_eq!(
            msft.action_source,
            crate::portfolio::ActionSource::ModelChosen
        );
        // Carried numerics compare exactly: serde_json's `float_roundtrip` feature
        // makes the store's JSON round-trip bit-exact (store.rs pins it).
        match (&msft.disposition, &verdict(&first, "MSFT").disposition) {
            (
                crate::portfolio::VerdictDisposition::Priced(carried),
                crate::portfolio::VerdictDisposition::Priced(prior),
            ) => {
                assert_eq!(carried.grade, prior.grade);
                assert_eq!(carried.action, prior.action);
                assert_eq!(carried.appendix, prior.appendix);
                assert_eq!(carried.thesis_document, prior.thesis_document);
                assert_eq!(carried.price_targets, prior.price_targets);
                assert_eq!(carried.sub_scores, prior.sub_scores);
            }
            other => panic!("expected carried priced verdicts, got {other:?}"),
        }
        // The stored re-anchor basis must survive the carry or the next sweep
        // reads the holding `unknown`.
        let msft_audit = second
            .audit
            .iter()
            .find(|a| a.symbol == "MSFT")
            .expect("carried audit row");
        assert!(msft_audit.quick_basis.is_some());
        assert!(
            msft_audit.pre_profit.is_some(),
            "the overlay record survives the whole-row audit carry"
        );
        assert_eq!(second.roll_up.graded_count, 2);
        let conn = storage::open(&paths.db_path).unwrap();
        let qc = store::latest_quick_check(&conn)
            .unwrap()
            .expect("carried sweep state retained");
        assert_eq!(qc.swept_run_id, second.run_id);
        assert!(qc.holdings.iter().any(|h| h.symbol == "MSFT"));
        assert!(!qc.holdings.iter().any(|h| h.symbol == "AAPL"));
    }

    #[test]
    fn a_tail_sweep_flag_carries_and_badges_rather_than_force_including() {
        let (_dir, paths) = paths();
        let first = full_run(&paths, two_stocks());
        // MSFT's price crashes far outside its stored band: the sweep flags it,
        // and the flag rides the retained sweep state as a card badge.
        let second = selective_run(
            &paths,
            two_stocks(),
            &["AAPL"],
            &SelectiveQuickData {
                crash_price: Some(("MSFT", 5.0)),
                ..Default::default()
            },
        );
        assert_eq!(
            verdict(&second, "MSFT").analyzed_at.as_deref(),
            Some(first.created_at.as_str()),
            "a flagged holding carries, no longer force-included"
        );
        let conn = storage::open(&paths.db_path).unwrap();
        let qc = store::latest_quick_check(&conn)
            .unwrap()
            .expect("carried sweep state retained for the badge");
        assert_eq!(qc.swept_run_id, second.run_id);
        let msft_state = qc
            .holdings
            .iter()
            .find(|h| h.symbol == "MSFT")
            .expect("MSFT swept state retained");
        assert!(
            msft_state.flag.is_some(),
            "the attention flag rides the badge overlay"
        );
    }

    #[test]
    fn an_unknown_sweep_family_carries_and_badges_rather_than_force_including() {
        let (_dir, paths) = paths();
        let first = full_run(&paths, two_stocks());
        // MSFT's price retrieval fails, so a required family reads `unknown`.
        let second = selective_run(
            &paths,
            two_stocks(),
            &["AAPL"],
            &SelectiveQuickData {
                fail_price: Some("MSFT"),
                ..Default::default()
            },
        );
        assert_eq!(
            verdict(&second, "MSFT").analyzed_at.as_deref(),
            Some(first.created_at.as_str()),
            "an `unknown` family carries, no longer force-included"
        );
        let conn = storage::open(&paths.db_path).unwrap();
        let qc = store::latest_quick_check(&conn)
            .unwrap()
            .expect("carried sweep state retained for the badge");
        let msft_state = qc
            .holdings
            .iter()
            .find(|h| h.symbol == "MSFT")
            .expect("MSFT swept state retained");
        assert!(
            msft_state
                .families
                .iter()
                .any(|f| f.state == crate::portfolio::quick_check::SweepState::Unknown),
            "the `unknown` family rides the badge overlay"
        );
    }

    #[test]
    fn an_unexamined_evidence_event_carries_and_badges_rather_than_force_including() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        // MSFT's last full pass was 10 days ago; an earnings actual landed 5 days
        // ago, an unexamined event.
        let old = days_ago(10);
        doctor_latest_run(&paths, "MSFT", |v| {
            v.analyzed_at = Some(old.clone());
        });
        let second = selective_run(
            &paths,
            two_stocks(),
            &["AAPL"],
            &SelectiveQuickData {
                earnings_date: Some(days_ago(5).chars().take(10).collect()),
                ..Default::default()
            },
        );
        assert_eq!(
            verdict(&second, "MSFT").analyzed_at.as_deref(),
            Some(old.as_str()),
            "an unexamined evidence event carries vintage-stamped, no longer force-included"
        );
        let conn = storage::open(&paths.db_path).unwrap();
        let qc = store::latest_quick_check(&conn)
            .unwrap()
            .expect("carried sweep state retained for the badge");
        let msft_state = qc
            .holdings
            .iter()
            .find(|h| h.symbol == "MSFT")
            .expect("MSFT swept state retained");
        assert!(
            !msft_state.evidence_events.is_empty(),
            "the evidence event rides the badge overlay"
        );
    }

    #[test]
    fn an_over_age_carried_add_action_is_rule_demoted_to_hold() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        let old = days_ago(40);
        doctor_latest_run(&paths, "MSFT", |v| {
            v.analyzed_at = Some(old.clone());
            if let crate::portfolio::VerdictDisposition::Priced(g) = &mut v.disposition {
                g.action = crate::portfolio::Action::Add;
            }
        });
        let second = selective_run(
            &paths,
            two_stocks(),
            &["AAPL"],
            &SelectiveQuickData::default(),
        );
        let msft = verdict(&second, "MSFT");
        assert_eq!(
            msft.analyzed_at.as_deref(),
            Some(old.as_str()),
            "the demotion is a labeled weaken on the carried verdict, not a fresh pass"
        );
        assert_eq!(
            msft.action_source,
            crate::portfolio::ActionSource::RuleDemoted
        );
        match &msft.disposition {
            crate::portfolio::VerdictDisposition::Priced(g) => {
                assert_eq!(g.action, crate::portfolio::Action::Hold);
            }
            other => panic!("expected a priced carry, got {other:?}"),
        }
    }

    #[test]
    fn an_over_age_carried_role_risk_add_action_is_rule_demoted_to_hold() {
        // A role-risk verdict can carry an add-family action, and the
        // stale-strong-action rule is branch-unscoped.
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        let old = days_ago(40);
        doctor_latest_run(&paths, "MSFT", |v| {
            v.analyzed_at = Some(old.clone());
            v.disposition = crate::portfolio::VerdictDisposition::RoleRiskOnly(Box::new(
                crate::portfolio::RoleRiskVerdict {
                    class_label: "bond fund".into(),
                    thesis_document: "Role: the core fixed-income sleeve.".into(),
                    exposure_tilt: Vec::new(),
                    expense_drag: None,
                    observable_risk: None,
                    structural_flag: false,
                    is_cef: false,
                    nav_premium: None,
                    evidence_gaps: Vec::new(),
                    action: crate::portfolio::Action::Add,
                    action_rationale: String::new(),
                },
            ));
        });
        let second = selective_run(
            &paths,
            two_stocks(),
            &["AAPL"],
            &SelectiveQuickData::default(),
        );
        let msft = verdict(&second, "MSFT");
        assert_eq!(
            msft.analyzed_at.as_deref(),
            Some(old.as_str()),
            "the demotion is a labeled weaken on the carried verdict, not a fresh pass"
        );
        assert_eq!(
            msft.action_source,
            crate::portfolio::ActionSource::RuleDemoted
        );
        match &msft.disposition {
            crate::portfolio::VerdictDisposition::RoleRiskOnly(r) => {
                assert_eq!(r.action, crate::portfolio::Action::Hold);
            }
            other => panic!("expected a role-risk carry, got {other:?}"),
        }
    }

    #[test]
    fn a_carried_verdict_keeps_its_action_and_refreshes_its_position_tag() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        // A same-side decrease raises no badge; only the position-change tag
        // reads today's diff.
        let trimmed = holdings_of(vec![
            stock("AAPL", 20.0, 3_900.0),
            stock("MSFT", 10.0, 1_950.0),
        ]);
        let second = selective_run(&paths, trimmed, &["AAPL"], &SelectiveQuickData::default());
        let msft = verdict(&second, "MSFT");
        assert_ne!(
            msft.analyzed_at.as_deref(),
            Some(second.created_at.as_str()),
            "MSFT was carried, not re-analyzed"
        );
        assert_eq!(msft.position_change, PositionChange::Decreased);
        match &msft.disposition {
            crate::portfolio::VerdictDisposition::Priced(g) => {
                assert_eq!(g.action, crate::portfolio::Action::Hold);
            }
            other => panic!("expected a priced carry, got {other:?}"),
        }
    }

    #[test]
    fn fresh_and_carried_verdicts_read_the_basis_move_on_an_increase() {
        let (_dir, paths) = paths();
        // Both names start at 20 shares on a 3,120 basis — 156 a share.
        full_run(&paths, two_stocks());
        // AAPL is added to above its average and re-analyzed; MSFT is added to
        // below its average and carried. Both read today's diff.
        let added = holdings_of(vec![
            Position {
                cost_basis: 6_000.0,
                ..stock("AAPL", 30.0, 6_500.0)
            },
            Position {
                cost_basis: 3_900.0,
                ..stock("MSFT", 30.0, 5_850.0)
            },
        ]);
        let second = selective_run(&paths, added, &["AAPL"], &SelectiveQuickData::default());
        let aapl = verdict(&second, "AAPL");
        assert_eq!(aapl.analyzed_at.as_deref(), Some(second.created_at.as_str()));
        let read = aapl.basis_move.as_ref().expect("the fresh verdict reads the move");
        assert_eq!(read.direction, crate::portfolio::BasisDirection::PaidUp);
        assert!((read.prior_average_cost - 156.0).abs() < 1e-9);
        assert!((read.average_cost - 200.0).abs() < 1e-9);
        let msft = verdict(&second, "MSFT");
        assert_ne!(msft.analyzed_at.as_deref(), Some(second.created_at.as_str()));
        let read = msft.basis_move.as_ref().expect("the carried verdict reads the move");
        assert_eq!(read.direction, crate::portfolio::BasisDirection::AveragedDown);
        assert!((read.average_cost - 130.0).abs() < 1e-9);
    }

    #[test]
    fn an_over_age_carried_exit_action_now_carries_rather_than_force_including() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        let old = days_ago(40);
        doctor_latest_run(&paths, "MSFT", |v| {
            v.analyzed_at = Some(old.clone());
            if let crate::portfolio::VerdictDisposition::Priced(g) = &mut v.disposition {
                g.action = crate::portfolio::Action::Trim;
            }
        });
        let second = selective_run(
            &paths,
            two_stocks(),
            &["AAPL"],
            &SelectiveQuickData::default(),
        );
        let msft = verdict(&second, "MSFT");
        // An over-age exit carry stands as-is; only over-age add-family carries
        // rule-demote.
        assert_eq!(
            msft.analyzed_at.as_deref(),
            Some(old.as_str()),
            "an over-age exit-family carry stands, no longer force-included"
        );
        assert_eq!(msft.action_source, crate::portfolio::ActionSource::ModelChosen);
        match &msft.disposition {
            crate::portfolio::VerdictDisposition::Priced(g) => {
                assert_eq!(g.action, crate::portfolio::Action::Trim);
            }
            other => panic!("expected a priced carry, got {other:?}"),
        }
    }

    #[test]
    fn a_new_unselected_holding_is_left_not_analyzed() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        // A third holding appears, unselected: no prior verdict to carry, so it is
        // left not analyzed.
        let three = holdings_of(vec![
            stock("AAPL", 20.0, 3_900.0),
            stock("MSFT", 20.0, 3_900.0),
            stock("NVDA", 10.0, 2_000.0),
        ]);
        let second = selective_run(&paths, three, &["AAPL"], &SelectiveQuickData::default());
        assert!(
            second.verdicts.iter().all(|v| v.symbol != "NVDA"),
            "an unselected new holding earns no verdict — it is left not analyzed"
        );
        assert_eq!(second.verdicts.len(), 2);
    }

    /// Wraps [`StubCompanyData`] recording every per-symbol retrieval call.
    #[derive(Default)]
    struct CountingCompanyData {
        financials: std::cell::RefCell<Vec<String>>,
        facts: std::cell::RefCell<Vec<String>>,
        deep_history: std::cell::RefCell<Vec<String>>,
        evidence: std::cell::RefCell<Vec<String>>,
    }

    impl CompanyDataSource for CountingCompanyData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            self.financials.borrow_mut().push(symbol.to_string());
            StubCompanyData.financials(symbol)
        }
        fn evidence(
            &self,
            symbol: &str,
            _session: chrono::NaiveDate,
        ) -> crate::portfolio::evidence::CompanyEvidence {
            self.evidence.borrow_mut().push(symbol.to_string());
            crate::portfolio::evidence::CompanyEvidence::empty(symbol)
        }
        fn facts(&self, symbol: &str) -> SecData {
            self.facts.borrow_mut().push(symbol.to_string());
            StubCompanyData.facts(symbol)
        }
        fn deep_price_history(
            &self,
            symbol: &str,
        ) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
            self.deep_history.borrow_mut().push(symbol.to_string());
            StubCompanyData.deep_price_history(symbol)
        }
    }

    #[test]
    fn a_class_the_pipeline_never_grades_spends_no_per_symbol_retrieval() {
        let (_dir, paths) = paths();
        let mut cash = stock("SWVXX", 5_000.0, 5_000.0);
        cash.asset_class = AssetClass::Cash;
        cash.description = "Schwab Value Advantage Money Fund".into();
        let mut option = stock("AAPL  260116C00250000", 5.0, 1_250.0);
        option.asset_class = AssetClass::OptionContract;
        option.description = "CALL AAPL 01/16/2026 250".into();
        let holdings = holdings_of(vec![stock("AAPL", 20.0, 3_900.0), cash, option]);

        let company = CountingCompanyData::default();
        let run = run_with_company(&paths, holdings, &company);

        // Output-neutral: the eligibility routing decides NotRated before the
        // engine stage.
        for symbol in ["SWVXX", "AAPL  260116C00250000"] {
            assert!(
                matches!(
                    &verdict(&run, symbol).disposition,
                    crate::portfolio::VerdictDisposition::NotRated { .. }
                ),
                "{symbol} must still be not-rated"
            );
        }
        // The loop spends nothing on them.
        assert_eq!(
            *company.financials.borrow(),
            vec!["AAPL".to_string()],
            "only the gradeable holding is retrieved"
        );
        assert_eq!(*company.facts.borrow(), vec!["AAPL".to_string()]);
        assert_eq!(*company.deep_history.borrow(), vec!["AAPL".to_string()]);
        assert_eq!(
            *company.evidence.borrow(),
            vec!["AAPL".to_string()],
            "the evidence surface is a graded stock's alone"
        );

        // The audit says what it consulted, not a financials pull the gate skipped.
        let audit = run
            .audit
            .iter()
            .find(|a| a.symbol == "SWVXX")
            .expect("the not-rated row still records an audit");
        // The exact set: an any()-shaped assertion passed while the audit still
        // claimed the house view, which this verdict never reads.
        assert_eq!(
            audit.sources,
            vec!["Schwab position (cash — not graded by the equity pipeline)".to_string()],
            "a not-rated audit lists only the evidence that decided it"
        );
    }

    #[test]
    fn a_side_reversal_carries_and_is_marked_rather_than_re_entering() {
        let (_dir, paths) = paths();
        let first = full_run(&paths, two_stocks());
        // MSFT flipped net long → net short: it carries its prior verdict, marked
        // `side_reversed` for the card badge.
        let flipped = holdings_of(vec![
            stock("AAPL", 20.0, 3_900.0),
            stock("MSFT", -20.0, -3_900.0),
        ]);
        let second = selective_run(&paths, flipped, &["AAPL"], &SelectiveQuickData::default());
        let msft = verdict(&second, "MSFT");
        assert_eq!(
            msft.analyzed_at.as_deref(),
            Some(first.created_at.as_str()),
            "a side-reversed holding carries, no longer force-included"
        );
        assert!(msft.side_reversed, "the reversal is marked for the card badge");
        assert!(
            matches!(&msft.disposition, crate::portfolio::VerdictDisposition::Priced(_)),
            "it carries its prior priced verdict rather than re-entering as not-rated: {:?}",
            msft.disposition
        );
    }

    #[test]
    fn the_side_reversal_marker_persists_across_repeated_carries_and_clears_on_flip_back() {
        // A carried directional verdict is authored long, so `side_reversed` is
        // read from the current side each run: set while the position stays
        // net-short, cleared when it returns to long.
        let (_dir, paths) = paths();
        let first = full_run(&paths, two_stocks()); // MSFT long +20
        let short = || {
            holdings_of(vec![
                stock("AAPL", 20.0, 3_900.0),
                stock("MSFT", -20.0, -1_950.0),
            ])
        };
        // Run 2: MSFT flips long → short — marked reversed on the first carry.
        let r2 = selective_run(&paths, short(), &["AAPL"], &SelectiveQuickData::default());
        assert!(
            verdict(&r2, "MSFT").side_reversed,
            "reversed on the first carry after the flip"
        );
        // Run 3: MSFT still short; the marker must persist though the run-2→run-3
        // diff sees no flip.
        let r3 = selective_run(&paths, short(), &["AAPL"], &SelectiveQuickData::default());
        let msft3 = verdict(&r3, "MSFT");
        assert!(
            msft3.side_reversed,
            "the marker persists while the carried verdict stays opposite the held side"
        );
        assert_eq!(
            msft3.analyzed_at.as_deref(),
            Some(first.created_at.as_str()),
            "still the original long verdict, carried"
        );
        // Run 4: MSFT flips back to long, matching the original verdict; the
        // marker clears.
        let r4 = selective_run(
            &paths,
            holdings_of(vec![
                stock("AAPL", 20.0, 3_900.0),
                stock("MSFT", 20.0, 1_950.0),
            ]),
            &["AAPL"],
            &SelectiveQuickData::default(),
        );
        assert!(
            !verdict(&r4, "MSFT").side_reversed,
            "a flip back to the verdict's original side clears the marker"
        );
    }

    #[test]
    fn the_side_reversal_marker_survives_a_flip_through_a_zero_net_position() {
        // The marker reads the carried verdict's invariant long side against the
        // current side, so a reversal through an exactly-zero net (kept by
        // netting, invisible to a sign-flip read) is still caught.
        let (_dir, paths) = paths();
        let first = full_run(&paths, two_stocks()); // MSFT long +20, priced
        // Run 2: MSFT nets to exactly zero, so no opposite side yet.
        let zero_net = holdings_of(vec![
            stock("AAPL", 20.0, 3_900.0),
            Position {
                symbol: "MSFT".into(),
                description: "MSFT Inc.".into(),
                asset_class: AssetClass::Stock,
                quantity: 0.0,
                cost_basis: 0.0,
                market_value: 0.0,
                current_price: None,
            },
        ]);
        let r2 = selective_run(&paths, zero_net, &["AAPL"], &SelectiveQuickData::default());
        assert!(
            !verdict(&r2, "MSFT").side_reversed,
            "a flat (zero-net) position has no opposite side to badge"
        );
        // Run 3: MSFT nets short with no single-run long→short sign flip.
        let short = holdings_of(vec![
            stock("AAPL", 20.0, 3_900.0),
            stock("MSFT", -20.0, -1_950.0),
        ]);
        let r3 = selective_run(&paths, short, &["AAPL"], &SelectiveQuickData::default());
        let msft = verdict(&r3, "MSFT");
        assert!(
            msft.side_reversed,
            "a reversal through a zero net is still caught"
        );
        assert_eq!(
            msft.analyzed_at.as_deref(),
            Some(first.created_at.as_str()),
            "still the original long verdict, carried"
        );
    }

    #[test]
    fn an_empty_selection_or_missing_prior_run_runs_the_whole_book() {
        let (_dir, paths) = paths();
        // No prior run: nothing to carry, so the whole book runs.
        let first = selective_run(
            &paths,
            two_stocks(),
            &["AAPL"],
            &SelectiveQuickData::default(),
        );
        assert_eq!(first.verdicts.len(), 2);
        for v in &first.verdicts {
            assert_eq!(v.analyzed_at.as_deref(), Some(first.created_at.as_str()));
        }
        // An empty selection is the whole-book run too.
        let second = selective_run(&paths, two_stocks(), &[], &SelectiveQuickData::default());
        assert_eq!(second.verdicts.len(), 2);
        for v in &second.verdicts {
            assert_eq!(v.analyzed_at.as_deref(), Some(second.created_at.as_str()));
        }
    }

}
