//! The Portfolio Analysis job lifecycle (`docs/portfolio-analysis.md`,
//! `docs/local-models.md §Failure posture`). Parallel to [`crate::jobs::run_job`] but
//! for the local job: it claims the **same** single global run slot ([`RunGuard`]) so
//! the report and both local jobs are mutually exclusive, runs each holding through
//! the per-holding [`crate::portfolio::pipeline`], builds the roll-up, persists the
//! run (newest-N retention, [`crate::portfolio::PORTFOLIO_RUN_RETENTION`]), and
//! records the lifecycle outcome to `job_runs`.
//!
//! Offline-testable like the report job: the three external dependencies — holdings
//! ([`HoldingsSource`]), company financials ([`CompanyDataSource`]), and the model
//! stages ([`HoldingAnalyst`]) — are all traits, so the whole job runs against stubs
//! with no Schwab connection, no network, and no daemon.

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

/// The `job_runs.job_type` slug for Portfolio Analysis runs, distinct from the
/// report's `market_signal` so the two histories stay separable. `pub(crate)`
/// because `jobs::job_status` scopes its per-section footer stamps by this slug
/// (mirroring `quick_check::QUICK_CHECK_JOB`).
pub(crate) const PORTFOLIO_JOB: &str = "portfolio_analysis";

/// Human title for the run tracker header.
const RUN_LABEL: &str = "Portfolio Analysis";

/// Reason recorded when the concurrency guard rejects a run (another job is running).
const SKIP_REASON: &str = "another run is already in progress";

/// SEC EDGAR facts plus the degraded-input notes its gather produced. SEC is
/// supplementary and fail-soft, but data-honesty requires that a *failed* fetch leave
/// a tagged gap rather than silently returning empty facts — otherwise an outage,
/// 404, parse failure, or unresolvable ticker is indistinguishable from "SEC was
/// unnecessary," and the persisted audit/prompt loses a material signal.
#[derive(Debug, Clone, Default)]
pub struct SecData {
    /// The facts the company-facts endpoint returned: `Some` whenever the endpoint
    /// was **queried** (a failed fetch degrades to `Some(default)` beside its gap),
    /// `None` when it never was — a ticker with no CIK mapping, whose gap says so.
    /// The audit's source label reads this distinction (`dossier::assemble`): a
    /// queried-but-empty leg labels "(empty)", a never-queried one carries no label.
    pub facts: Option<CompanyFacts>,
    /// Degraded-input notes — empty when SEC contributed cleanly (or genuinely had
    /// nothing to add for a ticker it could resolve).
    pub gaps: Vec<String>,
}

/// The per-holding company-financials source the job reads, behind a trait so the job
/// is offline-testable. The live impl ([`LiveCompanyData`]) composes the FMP
/// per-company pull with keyless SEC EDGAR facts, deep FMP dated-EOD history, and the
/// per-fund FMP surface; a stub returns fixtures. The fund-surface methods carry
/// fail-soft defaults so a stock-only stub stays small.
pub trait CompanyDataSource {
    /// FMP per-company financials (fail-soft; gaps recorded on the result).
    fn financials(&self, symbol: &str) -> CompanyFinancials;
    /// The fund flavor of the per-symbol pull (quote / history / dividends — no
    /// statement or consensus surface, so a fund logs no spurious stock gaps).
    /// Defaults to the stock pull so a stub stays small.
    fn fund_financials(&self, symbol: &str) -> CompanyFinancials {
        self.financials(symbol)
    }
    /// SEC EDGAR company facts plus any degraded-input notes ([`SecData`]).
    fn facts(&self, symbol: &str) -> SecData;
    /// Deep dated daily closes (FMP dated EOD — the v2 anchor join's price side),
    /// plus any gap notes. Fail-soft: an empty history under-populates the anchor
    /// window, which takes its documented fallback.
    fn deep_price_history(&self, _symbol: &str) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
        (vec![], vec![])
    }
    /// The per-fund metadata surface (`etf/info` + weightings). The default records
    /// the missing source as a gap so the fund floors honestly.
    fn fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
        crate::portfolio::fund::FundData {
            symbol: symbol.to_string(),
            gaps: vec!["fund metadata source not wired".to_string()],
            ..Default::default()
        }
    }
    /// The per-sector aggregate P/E snapshot (both exchanges) for the run's
    /// pinned **ET session** `session` — the caller passes the run's `today`
    /// (minted once from `created_at`), never a fresh clock read, so a run that
    /// crosses ET midnight before its first fund still snapshots the session it
    /// belongs to (the fund context's `as_of` is stamped the same way).
    /// Run-level, memoized by the caller across funds.
    fn sector_pe_snapshot(
        &self,
        _session: chrono::NaiveDate,
    ) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        Ok(vec![])
    }
    /// The trailing per-sector P/E history (both exchanges) for one sector —
    /// memoized by the caller across funds.
    fn sector_pe_history(&self, _sector: &str) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        Ok(vec![])
    }
    /// The FMP profile lookup for a stock — one fetch feeding the
    /// listing-resolution guard (issuer name / exchange —
    /// `docs/portfolio-analysis.md §Asset eligibility`) and the outcome episodes'
    /// entry-stamped sector identity. Fail-soft: the `Unverified` default lets an
    /// offline stub proceed ungated with a recorded degraded input — never a
    /// fabricated resolution, and the sector identity types `sector-unscorable`.
    fn profile_identity(&self, _symbol: &str) -> crate::portfolio::listing::ProfileLookup {
        crate::portfolio::listing::ProfileLookup::Unverified(
            "profile source not wired".to_string(),
        )
    }
    /// The item-classified 8-K filings sweep for a stock — the hard-forensic
    /// filing kinds' producer (`docs/portfolio-analysis.md` §Starting parameters;
    /// the shared contract is `docs/trade-opportunities-workflow.md §Step 5c`).
    /// `since` bounds the classification lookback (ISO date, inclusive). The
    /// `None` default means the leg is not wired (a stub) — the dossier records
    /// no sweep; the live impl always returns `Some`, typing an unrunnable sweep
    /// `Unknown` rather than a fabricated clear.
    fn filing_events(
        &self,
        _symbol: &str,
        _since: &str,
    ) -> Option<crate::portfolio::ForensicFilingState> {
        None
    }
    /// Symbol-scoped `news/stock` items since `from` (ISO date) — the research
    /// loop's structured seeds (leads, never evidence — `docs/web-research.md`).
    /// Fail-soft: the empty default (stubs, and any failed live fetch) just
    /// runs the loop unseeded.
    fn news_items(&self, _symbol: &str, _from: &str) -> Vec<crate::fmp::SymbolNewsItem> {
        Vec::new()
    }
}

/// The run-level market-context source (`docs/portfolio-workflow.md` §Step 5): the
/// rate anchors the engine consumes numerically in every target and hurdle. Behind a
/// trait so the job is offline-testable; the live impl wraps FRED.
pub trait MarketContextSource {
    /// The `DGS2` / `DGS10` prints plus the DGS10 anchor-window history, as decimal
    /// ratios. **Hard-fail on the prints only**: a print retrieval still failing
    /// after the shared bounded retries fails the run before any per-holding work —
    /// the suite's canonical rate-anchor rule (`docs/portfolio-analysis.md` §Failure
    /// posture). The anchor-window **history** is fail-soft: a failed request leaves
    /// the window empty (every spread observation inadmissible — the targets take
    /// their documented raw-percentile / carry fallback) and records the reason on
    /// the anchors' `history_gap`, never a new failure state (§Starting parameters).
    fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors>;

    /// The run-level commodity context (`docs/portfolio-workflow.md` §Step 5):
    /// FRED daily energy + the suite-shared monthly IMF metals + FMP gold, each
    /// series fail-soft to a typed gap on the returned context. `session` is the
    /// run's pinned ET session (dates the gold quote). The default (empty
    /// context, no gaps) means the leg is not wired — an offline stub.
    fn commodities(
        &self,
        _session: chrono::NaiveDate,
    ) -> crate::portfolio::dossier::CommodityContext {
        crate::portfolio::dossier::CommodityContext::default()
    }

    /// The run-level CFTC Commitments-of-Traders positioning on the bellwether
    /// contracts (`docs/portfolio-workflow.md` §Step 5) — rows + gap notes,
    /// wholly fail-soft; a commodity / macro **fund** holding maps onto one of
    /// these rows for its underlying-positioning read. The default (empty,
    /// no gaps) means the leg is not wired — an offline stub.
    fn positioning(
        &self,
        _session: chrono::NaiveDate,
    ) -> (Vec<crate::data_sources::CotPositioning>, Vec<String>) {
        (Vec::new(), Vec::new())
    }

    /// The optional CBOE venue-level put/call backdrop (`docs/data-sources.md
    /// §CBOE`) — broad-market sentiment context, never a per-name signal,
    /// wholly fail-soft. `(None, None)` = the leg is not wired (a stub);
    /// the live impl returns the backdrop or its typed gap.
    fn put_call_backdrop(&self) -> (Option<crate::cboe::PutCallBackdrop>, Option<String>) {
        (None, None)
    }

    /// The FINRA consolidated short-interest file (`docs/data-sources.md
    /// §FINRA`) — fetched **once per run**, each held stock reading it as a
    /// local lookup at dossier assembly (risk / squeeze-context positioning
    /// evidence, never a sub-score input), wholly fail-soft. `(None, None)` =
    /// the leg is not wired (a stub); the live impl returns the parsed file or
    /// its typed gap.
    fn short_interest(&self) -> (Option<crate::finra::ShortInterestFile>, Option<String>) {
        (None, None)
    }
}

/// How many days of DGS10 history the anchor-window request covers: the ~12-quarter
/// window plus the four TTM quarters behind its oldest anchor, plus alignment slack.
const RATE_HISTORY_LOOKBACK_DAYS: i64 = 1_600;

/// The live market context: FRED rate anchors, plus the run-level commodity
/// context (FRED level windows + the FMP gold quote) and the CFTC positioning
/// pull.
pub struct LiveMarketContext {
    pub fred: crate::fred::FredDataSource,
    /// The FMP half of the commodity context (the `GCUSD` gold quote). `None`
    /// keeps a rates-only construction valid (the commodity leg then records
    /// gold as a gap).
    pub fmp: Option<crate::fmp::FmpDataSource>,
    /// The keyless CFTC COT adapter — the same pull the report makes, read
    /// per job. `None` records the positioning leg as a gap.
    pub cot: Option<crate::cot::CotDataSource>,
    /// The keyless Cboe daily-statistics adapter (the venue-level put/call
    /// backdrop). `None` records the sentiment leg as a gap.
    pub cboe: Option<crate::cboe::CboeDataSource>,
    /// The keyless FINRA short-interest adapter (the once-per-run consolidated
    /// file). `None` records the short-interest leg as a gap.
    pub finra: Option<crate::finra::FinraDataSource>,
}

/// The FRED commodity catalog the live commodity load walks: series id, display
/// label, published unit, and sleeve (`docs/data-sources.md §Portfolio Analysis —
/// endpoint surface`; the five monthly IMF series are the suite-shared commodity
/// feed catalogued under the Trade Opportunities surface).
const FRED_COMMODITY_SERIES: &[(&str, &str, &str, crate::portfolio::dossier::CommodityGroup)] = &[
    ("DCOILWTICO", "WTI Crude Oil", "USD per barrel", crate::portfolio::dossier::CommodityGroup::Energy),
    ("DHHNGSP", "Henry Hub Natural Gas", "USD per million BTU", crate::portfolio::dossier::CommodityGroup::Energy),
    ("PCOPPUSDM", "Copper (IMF, monthly)", "USD per metric ton", crate::portfolio::dossier::CommodityGroup::Metals),
    ("PALUMUSDM", "Aluminum (IMF, monthly)", "USD per metric ton", crate::portfolio::dossier::CommodityGroup::Metals),
    ("PNICKUSDM", "Nickel (IMF, monthly)", "USD per metric ton", crate::portfolio::dossier::CommodityGroup::Metals),
    ("PIORECRUSDM", "Iron Ore (IMF, monthly)", "USD per metric ton", crate::portfolio::dossier::CommodityGroup::Metals),
    ("PURANUSDM", "Uranium (IMF, monthly)", "USD per pound", crate::portfolio::dossier::CommodityGroup::Metals),
];

impl MarketContextSource for LiveMarketContext {
    fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors> {
        let dgs2 = self.fred.latest_rate_dated("DGS2")?;
        let dgs10 = self.fred.latest_rate_dated("DGS10")?;
        // Deliberately the UTC date, not the ET session: this is a fetch range's
        // inclusive upper bound, so a one-day forward roll asks for an untraded
        // day that serves no row and shifts a rolling multi-day lookback by one.
        // Only session-KEYED reads (snapshot dates, staleness bounds, evidence
        // boundaries) need converting.
        let to = chrono::Utc::now().date_naive();
        let from = to - chrono::Duration::days(RATE_HISTORY_LOOKBACK_DAYS);
        // Fail-soft: a failed history request leaves every spread observation
        // inadmissible — the targets take their documented raw-percentile / carry
        // fallback, recorded — never a run failure (`docs/portfolio-analysis.md`
        // §Starting parameters).
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
        // Fetch-range upper bound: deliberately the UTC date, not the ET session
        // (the cross-cutting range-bound convention — a forward-rolled bound asks
        // for an unpublished day and serves nothing).
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
        // Gold — the one FMP commodity quote (`GCUSD`), dated on the run session.
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
}

/// The exchanges whose sector P/Es blend into the fund composite
/// (`docs/portfolio-analysis.md` §Asset eligibility — the defined exchange blend).
const SECTOR_PE_EXCHANGES: [&str; 2] = ["NYSE", "NASDAQ"];

/// The live company-data source: FMP per-company + SEC EDGAR. SEC is supplementary and
/// fail-soft — an unresolved ticker or a fetch error degrades to empty facts, and the
/// FMP half plus the derived multiples still carry the holding — but each such
/// degradation is recorded as a gap so the audit stays honest.
///
/// **Ordering invariant:** constructing this performs no network I/O. Every external
/// fetch — including the ticker → CIK map refresh — happens inside the global run
/// slot, after `run_portfolio_job` has claimed it and called `reset_cancel` +
/// `run_started`, so each request sees the run's own cancel state and streams its
/// tracker row under the active step. The command's daemon probe is the one
/// pre-slot check, and it is local-only.
pub struct LiveCompanyData {
    pub fmp: crate::fmp::FmpDataSource,
    pub sec: SecEdgarSource,
    /// The ticker → CIK resolver over SEC's full `company_tickers.json` map,
    /// **loaded on first use** ([`crate::sec::LazyCikResolver`] over
    /// [`crate::sec::load_cik_resolver`]) — the first stock's `facts` call inside
    /// the slot triggers it. An unresolved ticker degrades to a typed gap, never a
    /// fabricated mapping.
    pub cik: crate::sec::LazyCikResolver,
}

/// The SEC company-facts leg shared by [`LiveCompanyData::facts`] and the job's
/// slot-ordering test: resolve the CIK (loading the map on first use through
/// `sec`), then fetch the facts. Each degradation — no mapping, or a failed fetch —
/// is a recorded gap, never silently-empty facts.
pub(crate) fn sec_company_facts(
    cik: &crate::sec::LazyCikResolver,
    sec: &SecEdgarSource,
    symbol: &str,
) -> SecData {
    match cik.resolve(sec, symbol) {
        // A ticker with no EDGAR mapping: the facts endpoint was never queried.
        None => SecData {
            facts: None,
            gaps: vec![format!("SEC: no CIK mapping for {symbol}")],
        },
        Some(cik) => match sec.fetch_company_facts(cik) {
            // A clean fetch that genuinely carried nothing is not a degradation.
            Ok(facts) => SecData {
                facts: Some(facts),
                gaps: Vec::new(),
            },
            // An outage / 404 / parse failure is a real degraded input — the
            // endpoint was queried, so the leg still labels as consulted (empty).
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
        // Fail-soft: a failed news fetch runs the research loop unseeded — a
        // seed is a lead, never load-bearing evidence.
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
        // The single deep-price rung (FMP dated EOD — `docs/verification/
        // 2026-08-12-stooq-removal-decision.md`): a failed or empty history is one
        // gap note and an empty window, and a cancel mid-run spends nothing (the
        // FMP suite seam returns a gap without a request when cancelled).
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
        // The snapshot endpoint is date-keyed, so the date has to be a session that
        // actually traded. Two things follow, and this path had neither:
        //
        // - **The date is the ET session date**, not the UTC calendar date. An
        //   evening-ET run (after ~8 PM EDT / 7 PM EST) has already rolled to the
        //   next UTC day, so a UTC read asks for a session that has not happened —
        //   and the endpoint answers 200 with an empty array, not an error. And it
        //   is the **run's pinned** session (`today`, minted from `created_at`),
        //   passed in by the caller — not a clock read here — so a run crossing ET
        //   midnight before its first fund cannot snapshot next-session data
        //   inside a prior-session run.
        // - **The walk backs over weekday candidates**, exactly as the report path
        //   does ([`crate::fmp::FmpDataSource::fetch_sector_pe_for_exchange`]).
        //   Its warrant is that empty answer, not holidays: the adapter records
        //   live-verified evidence that a weekday holiday *does* serve carried
        //   values (2026-07-03, Juneteenth), so the second candidate is a safety
        //   net, not an expected cost — practical cardinality stays two calls.
        //   Weekends cost no request.
        //
        // Both matter because an empty snapshot is not inert: every priced US-equity
        // fund fails `composite_yield` and abstains, attributed to "no P/E-usable
        // sector overlap" rather than to the missing snapshot.
        let today = session;
        let mut last_err = None;
        for candidate in sector_pe_candidates(today) {
            let date = candidate.format("%Y-%m-%d").to_string();
            let mut rows = Vec::new();
            for exchange in SECTOR_PE_EXCHANGES {
                match self.fmp.fetch_sector_pe_snapshot(exchange, &date) {
                    Ok(mut r) => rows.append(&mut r),
                    Err(e) => last_err = Some(e),
                }
            }
            // A partial read (one exchange served, the other faulted) is still a
            // usable snapshot — the original single-date behavior, kept.
            if !rows.is_empty() {
                return Ok(rows);
            }
        }
        if let Some(e) = last_err {
            return Err(e);
        }
        // An exhausted walk with no transport fault is a real absence. Report it as
        // a gap rather than an empty snapshot: `Err` is this seam's gap channel (the
        // caller records it on the fund's `gaps`), so the fund's abstention names the
        // missing snapshot instead of blaming the fund's own sector weights.
        anyhow::bail!(
            "no sector-P/E snapshot in the {} weekdays through {today}",
            crate::fmp::SECTOR_LOOKBACK_WEEKDAYS
        )
    }

    fn sector_pe_history(&self, sector: &str) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        // Deliberately the UTC date, not the ET session: this is a fetch range's
        // inclusive upper bound, so a one-day forward roll asks for an untraded
        // day that serves no row and shifts a rolling multi-day lookback by one.
        // Only session-KEYED reads (snapshot dates, staleness bounds, evidence
        // boundaries) need converting.
        let to = chrono::Utc::now().date_naive();
        let from = to - chrono::Duration::days(DEEP_HISTORY_LOOKBACK_DAYS);
        let (from, to) = (
            from.format("%Y-%m-%d").to_string(),
            to.format("%Y-%m-%d").to_string(),
        );
        let mut rows = Vec::new();
        let mut last_err = None;
        for exchange in SECTOR_PE_EXCHANGES {
            match self
                .fmp
                .fetch_historical_sector_pe(sector, exchange, &from, &to)
            {
                Ok(mut r) => rows.append(&mut r),
                Err(e) => last_err = Some(e),
            }
        }
        if rows.is_empty() {
            if let Some(e) = last_err {
                return Err(e);
            }
        }
        Ok(rows)
    }

    fn profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
        self.fmp.fetch_profile_identity(symbol)
    }

    fn facts(&self, symbol: &str) -> SecData {
        sec_company_facts(&self.cik, &self.sec, symbol)
    }

    fn filing_events(
        &self,
        symbol: &str,
        since: &str,
    ) -> Option<crate::portfolio::ForensicFilingState> {
        use crate::portfolio::ForensicFilingState;
        Some(match self.cik.resolve(&self.sec, symbol) {
            // No EDGAR mapping: the submissions endpoint was never queried —
            // a typed unknown, never a clean no-event.
            None => ForensicFilingState::Unknown {
                reason: format!("no CIK mapping for {symbol}"),
                queried: false,
            },
            Some(cik) => match self.sec.fetch_recent_filings(cik) {
                Ok(filings) => {
                    match crate::sec::forensic_events_from_filings(symbol, &filings, since) {
                        Ok(events) if events.is_empty() => ForensicFilingState::Clear,
                        Ok(events) => ForensicFilingState::Events { events },
                        // An in-lookback 8-K with no readable items column: the
                        // sweep ran but cannot classify — unknown, never a
                        // fabricated clear.
                        Err(reason) => ForensicFilingState::Unknown {
                            reason,
                            queried: true,
                        },
                    }
                }
                Err(e) => ForensicFilingState::Unknown {
                    reason: format!("SEC filings sweep unavailable: {e}"),
                    queried: true,
                },
            },
        })
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

/// A **selective re-analysis** request (`docs/portfolio-analysis.md` §Triggering):
/// the user's per-card selection plus the retrieval surface the in-run safety sweep
/// over the unselected tail needs — bundled so a selective run without its sweep
/// source is unrepresentable (the sweep is the first of the three mixed-vintage
/// safety rules, never optional). `None` — or an empty selection, or no prior run
/// to carry from — runs the whole book.
pub struct SelectiveRun<'a> {
    /// The selected symbols (case-insensitive; silently intersected with the
    /// current book — a selected symbol no longer held is an exited position).
    pub selected: Vec<String>,
    /// The engine-only retrieval surface for the tail sweep.
    pub quick_data: &'a dyn crate::portfolio::quick_check::QuickCheckDataSource,
}

/// The over-age boundary for a carried verdict — aligned with the suite's ~4-week
/// research-freshness window (`docs/portfolio-analysis.md` §Triggering; §Starting
/// parameters — drafted). Beyond it a carried add-family action rule-demotes to
/// *hold*; a carried exit or hold stands as-is behind the card-facing stale badge
/// (since the 2026-08-16 ruling — an over-age exit no longer force-includes).
/// Mirrored by that stale badge (`src/components/PortfolioView.vue` `OVER_AGE_DAYS`)
/// — recalibrating one means recalibrating both.
const OVER_AGE_DAYS: i64 = 28;

/// Whether a vintage timestamp is over-age against `today`. Both sides date on
/// the ET session ([`crate::market_clock::et_date_of`] / callers passing an ET
/// `today`), never the UTC date prefix — an evening-ET vintage would otherwise
/// read one day younger than the session it belongs to. An unparseable
/// vintage reads over-age — the conservative resolution, since the stale-carry
/// rules exist to keep an unverifiable strong action from standing.
fn over_age(vintage: &str, today: chrono::NaiveDate) -> bool {
    match crate::market_clock::et_date_of(vintage) {
        Some(d) => (today - d).num_days() > OVER_AGE_DAYS,
        None => true,
    }
}

/// The resume window (`docs/portfolio-analysis.md` §Starting parameters,
/// drafted ~48 hours): an interrupted run offers resume only while its pinned
/// holdings pull is younger than this — past it the checkpoints are stale
/// against a book that may have moved, so Run analysis starts a new run.
pub const RESUME_WINDOW_HOURS: i64 = 48;

/// Whether an interrupted run's checkpoints can resume — `Ok(())` = offerable
/// (`docs/portfolio-analysis.md` §Failure posture: offered only while the
/// checkpoints exist and the pinned pull is younger than the resume window).
/// The stamps are the resume contract (ruled 2026-08-29, Codex I18): the five
/// version axes stamp what a completed holding's verdict and audit mean, the
/// format stamp the trail's own shape, the roster the models, and the
/// prior-run id the baseline — a mismatch on any refuses rather than mixing
/// verdicts across contracts. A rebuild that moves none resumes, the restored
/// holdings carrying the pre-change behaviour: a slice that changes
/// completed-holding semantics is obliged to move the axis it changed, and one
/// that changes the trail's shape the format stamp; no build identity is
/// checked. The Err carries the reason the UI shows.
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
    if cp.header.evidence_floor_version != crate::portfolio::engine::EVIDENCE_FLOOR_VERSION {
        return Err("the evidence-floor rule changed since the interrupted run".into());
    }
    if cp.header.checkpoint_format_version != store::CHECKPOINT_FORMAT_VERSION {
        return Err("the checkpoint format changed since the interrupted run".into());
    }
    if cp.header.model_ids != current_model_ids {
        return Err("the configured model roster changed since the interrupted run".into());
    }
    // The diff / carry baseline must not have moved: a newer persisted run means
    // these checkpoints describe a superseded book state (any run that persisted
    // discarded prior trails as it opened its own, so a mismatch is stale state,
    // never a live trail).
    let latest = store::latest_run(conn).ok().flatten().map(|r| r.run_id);
    if latest != cp.header.prior_run_id {
        return Err("a newer run has persisted since the interrupted run".into());
    }
    Ok(())
}

/// The human-readable message of a caught panic payload — a `&str` or `String`
/// from `panic!`, else the placeholder — for the failed run's detail line.
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// Run one Portfolio Analysis job end to end with the lifecycle contract. Returns
/// `Err` only on an infrastructure failure (the database); a failed analysis is a
/// normal `Ok(Failed)`. The model/persistence half is **fail-hard** (a model error
/// fails the run); the research half is fail-soft (stubbed this slice, so moot).
///
/// **Slot ordering:** the global run slot is claimed here (`try_begin`), then the
/// cancel flag is cleared and `run_started` emitted, and only then does any
/// external fetch happen — the sources handed in must not have fetched during
/// construction (the live ones defer, e.g. [`LiveCompanyData`]'s lazy CIK map).
/// The command's daemon probe is the one pre-slot check, and it is local-only. A
/// competing attempt therefore records `Skipped` having contacted nothing.
#[allow(clippy::too_many_arguments)]
pub fn run_portfolio_job(
    holdings_source: &dyn HoldingsSource,
    company_data: &dyn CompanyDataSource,
    market: &dyn MarketContextSource,
    analyst: &dyn HoldingAnalyst,
    profile: &InvestorProfile,
    selective: Option<SelectiveRun<'_>>,
    outcome_sources: Option<&crate::portfolio::outcome::OutcomeSources<'_>>,
    resume: Option<store::Checkpoint>,
    paths: &ReportPaths,
    guard: &RunGuard,
    ctx: &RunContext,
) -> Result<PortfolioJobOutcome> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;

    // Claim the single global run slot — shared with the report job, so the two are
    // mutually exclusive. Held until this function returns.
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
    // anywhere below the spine — the compute modules over hostile feed values —
    // must reach the same terminal lifecycle as any hard failure: the
    // job-history row, the `run_finished` event, and any eligible standing
    // checkpoint trail offerable from the tracker (an early panic may have
    // opened none). Unwinding panics only; an abort (stack overflow, OOM)
    // is not catchable in-process. The payload message is the failed detail —
    // the file:line stays on stderr via the default hook (ruled 2026-08-28).
    let analysis = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_analysis(
            holdings_source,
            company_data,
            market,
            analyst,
            profile,
            selective,
            outcome_sources,
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
        // A cancel requested mid-run surfaces as an error; the shared flag tells a
        // user-initiated stop apart from a genuine failure. A panic is never a
        // user stop: it records `Failed` even with a cancel pending, so the
        // failed-job warning surfaces the crash (ruled 2026-08-28).
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
            // Alternate (chain) format, not `to_string()`: a context-wrapped
            // failure (e.g. the action call's) would otherwise persist
            // only its outermost message, hiding the typed root cause — the
            // length stop, the parse failure, the HTTP error — from
            // `job_runs.detail`, the forensic surface a failed run leaves.
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

/// The Step-6a semantic-recall query, built deterministically from the holding's
/// identity and the prior verdict's themes (`docs/portfolio-workflow.md`
/// §Step 6a) — the embedding request builder byte-caps it before the call.
fn semantic_query_text(
    symbol: &str,
    sector: Option<&str>,
    industry: Option<&str>,
    prior: Option<&dossier::PriorHolding>,
) -> String {
    let mut q = format!("holding {symbol}");
    if let Some(s) = sector {
        q.push_str(&format!(", sector {s}"));
    }
    if let Some(i) = industry {
        q.push_str(&format!(", industry {i}"));
    }
    if let Some(ledger) = prior.and_then(|p| p.verdict.thesis_ledger.as_ref()) {
        q.push_str(&format!(". Standing thesis: {}", ledger.current_thesis));
        if !ledger.key_drivers.is_empty() {
            let drivers: Vec<&str> =
                ledger.key_drivers.iter().map(|d| d.name.as_str()).collect();
            q.push_str(&format!(" Key drivers: {}", drivers.join(", ")));
        }
    }
    q
}

/// Run the Step-6a semantic continuity retrieval — fail-soft
/// (`docs/portfolio-workflow.md` §Step 6a): an unconfigured embedder or an
/// empty partition (the first post-slice run, by design) is silent absence,
/// while a failed embed, count, or search records the typed gap and skips
/// recall for this holding only.
fn semantic_recall_for(
    conn: &Connection,
    embedder: Option<&dyn crate::embedding::Embedder>,
    query: &str,
) -> dossier::SemanticRecall {
    use crate::vector_memory::{self, MemoryKind, MemoryNamespace};
    let Some(embedder) = embedder else {
        return dossier::SemanticRecall::default();
    };
    let gap = |reason: String| dossier::SemanticRecall {
        hits: Vec::new(),
        gap: Some(format!("semantic recall skipped: {reason}")),
    };
    // The cheap guard: an empty summary shelf needs no query embedding at all.
    // Kind-scoped deliberately — the partition's durable-learning rows never
    // participate in this recall, so they must not make it look searchable.
    match vector_memory::count_memory_kind(conn, MemoryKind::Summary, MemoryNamespace::Portfolio)
    {
        Ok(0) => return dossier::SemanticRecall::default(),
        Ok(_) => {}
        Err(e) => return gap(format!("memory count failed: {e}")),
    }
    let vector = match embedder.embed(query) {
        Ok(v) => v,
        Err(e) => return gap(format!("query embedding failed: {e}")),
    };
    match vector_memory::search_memory(
        conn,
        &vector,
        Some(MemoryKind::Summary),
        MemoryNamespace::Portfolio,
        crate::portfolio::SEMANTIC_RECALL_TOP_K,
    ) {
        Ok(hits) => dossier::SemanticRecall {
            hits: hits.iter().map(|h| h.prompt_fragment()).collect(),
            gap: None,
        },
        Err(e) => gap(format!("memory search failed: {e}")),
    }
}

/// The per-holding continuity summary text the Step-7 embedding vectorizes
/// (`docs/portfolio-workflow.md` §Step 7's run-result embeddings): the standing
/// thesis (ledger thesis, key drivers, scenario lean), the intrinsic read —
/// grade and conviction, or the role read and structural flag on the
/// `role_risk_only` branch — and the portfolio action, so cross-run recall
/// surfaces the substance of prior analysis rather than a bare grade. `None` on
/// a not-rated or insufficient-evidence verdict — nothing analyzed to recall.
fn holding_summary_text(v: &crate::portfolio::HoldingVerdict) -> Option<String> {
    let ledger = v.thesis_ledger.as_ref();
    let thesis = ledger
        .map(|l| l.current_thesis.as_str())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or("(none recorded)");
    let drivers = ledger
        .map(|l| {
            l.key_drivers
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "(none recorded)".to_string());
    let lean = ledger
        .map(|l| {
            l.monitor
                .iter()
                .map(|m| format!("{:?} {:.0}%", m.scenario, m.probability_pct))
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(none recorded)".to_string());
    match &v.disposition {
        crate::portfolio::VerdictDisposition::Priced(g) => Some(format!(
            "{}: grade {}, conviction {:?}, action {} — {}. Standing thesis: {} \
             Key drivers: {}. Scenario lean: {}.",
            v.symbol,
            g.grade.as_str(),
            g.conviction,
            g.action.as_kebab(),
            g.action_rationale,
            thesis,
            drivers,
            lean,
        )),
        crate::portfolio::VerdictDisposition::RoleRiskOnly(r) => Some(format!(
            "{}: role/risk-only ({}{}), action {} — {}. Role: {} Standing \
             thesis: {} Key drivers: {}. Scenario lean: {}.",
            v.symbol,
            r.class_label,
            if r.structural_flag {
                ", structurally path-dependent"
            } else {
                ""
            },
            r.action.as_kebab(),
            r.action_rationale,
            r.role_summary,
            thesis,
            drivers,
            lean,
        )),
        _ => None,
    }
}

/// The analysis half: pull holdings, load the house view, run each holding through the
/// pipeline, build the roll-up, and persist the run. Returns the persisted
/// [`PortfolioRun`]. A cancellation check and the per-holding checkpoint write
/// sit between holdings.
#[allow(clippy::too_many_arguments)]
fn run_analysis(
    holdings_source: &dyn HoldingsSource,
    company_data: &dyn CompanyDataSource,
    market: &dyn MarketContextSource,
    analyst: &dyn HoldingAnalyst,
    profile: &InvestorProfile,
    selective: Option<SelectiveRun<'_>>,
    outcome_sources: Option<&crate::portfolio::outcome::OutcomeSources<'_>>,
    resume: Option<store::Checkpoint>,
    paths: &ReportPaths,
    conn: &Connection,
    ctx: &RunContext,
) -> Result<PortfolioRun> {
    // A resume reopens the interrupted run's pinned pull — the one exception to
    // Run-analysis-always-pulls-fresh (`docs/portfolio-analysis.md` §Triggering,
    // §Failure posture): the checkpoint contract is only coherent against one
    // holdings snapshot, so completed and resumed holdings compute against the
    // same book and the finished run is stamped with that pull's as-of time.
    let holdings = match &resume {
        Some(cp) => {
            ctx.step_started("holdings", "Reopen pinned holdings pull (resume)");
            let h = cp.header.holdings.clone();
            ctx.step_finished("holdings", "ok", None);
            h
        }
        None => {
            ctx.step_started("holdings", "Pull holdings");
            // Snapshot assembly runs the holdings-normalization step: same-symbol
            // rows across granted accounts net into one book-level position per
            // symbol, and every later step consumes only the normalized rows
            // (`docs/schwab-integration.md` §What is pulled;
            // `docs/portfolio-workflow.md` §Step 2).
            let h = holdings_source.holdings()?.normalized()?;
            ctx.step_finished("holdings", "ok", None);
            h
        }
    };

    // Deterministic holdings-change diff against the prior run's persisted snapshot
    // (Step 4), computed in the app layer before any model stage — the
    // compute-don't-guess boundary. Fail-soft: an unreadable prior run reads as "no
    // prior snapshot", so every position tags `new`, exactly as a first run does.
    // A corrupt row is loud-skipped inside the store; a store read error (SQL /
    // query failure) degrades the same way here, logged (`prior_state_read`).
    let prior_run = prior_state_read("prior-run", store::latest_run(conn));
    let prior_run_id = prior_run.as_ref().map(|r| r.run_id.clone());
    let prior_created_at = prior_run.as_ref().map(|r| r.created_at.clone());
    let holdings_diff = diff::diff_holdings(prior_run.as_ref().map(|r| &r.holdings), &holdings);

    // The quick-check store's fresher condition evaluation states — overlaid onto
    // each prior ledger before this run evaluates it, so the between-run sweeps'
    // streaks and acknowledgments chain instead of silently resetting
    // (`docs/portfolio-analysis.md §The quick check`). Only a state swept against
    // the same prior run applies. Same fail-soft as the prior run: an unreadable
    // (corrupt — loud-skipped in the store) or unreadable-by-error state reads as
    // "no quick check since the last pass", logged.
    let quick_state = prior_state_read("quick-check-state", store::latest_quick_check(conn))
        .filter(|s| Some(&s.swept_run_id) == prior_run_id.as_ref());

    // The run's one wall-clock instant, minted before any dated decision: the
    // house-view freshness gate, the over-age reads, the label pass, and the
    // persisted `created_at` (which the card's stale badge ages against) all
    // derive from it, so an hours-long run crossing ET midnight cannot demote on
    // one ET day and render the badge on the next. Run identity is insertion
    // order (`id`); `created_at` is display and vintage data, so stamping at run
    // start is a display choice — and the one that matches the session the run's
    // data belongs to.
    // A resume pins the interrupted run's instant, so every dated decision —
    // the vintages, the session stamps, the resume-window aging — stays the
    // original run's; its identity (`run_id`) is likewise reopened.
    let created_at = match &resume {
        Some(cp) => cp.header.created_at.clone(),
        None => now_rfc3339(),
    };
    let run_id = match &resume {
        Some(cp) => cp.header.run_id.clone(),
        None => uuid::Uuid::new_v4().to_string(),
    };
    // ET, pairing with the ET-dated vintages `over_age` and the house-view gate
    // compare against.
    let today = crate::market_clock::et_date_of(&created_at)
        .unwrap_or_else(|| crate::market_clock::et_session_date(chrono::Utc::now()));
    // The same session as a `YYYY-MM-DD` string — the one `run_date` every dated
    // stamp in this run uses (the per-holding ledger evaluation and the label
    // pass alike), so a run cannot stamp its own book across two ET days.
    let run_session_date = today.format("%Y-%m-%d").to_string();

    // Freshness-gated (`docs/portfolio-workflow.md` §Step 5): a stale latest
    // report drops the whole view; the omission rides the run's data health.
    // Dated on the run's own ET session, against the report's ET-dated
    // `created_at` — both legs convert together (see `load_house_view`). A
    // resume reads the pinned Step-5 context instead of reloading.
    let (house_view, house_view_omitted) = match &resume {
        Some(cp) => (cp.header.house_view.clone(), cp.header.house_view_omitted),
        None => dossier::load_house_view(conn, &paths.reports_dir, today),
    };

    // The run-level rate anchors — **hard-fail before any per-holding work** (the
    // suite's canonical rate-anchor rule: the engine consumes the rates numerically
    // in every target and hurdle, so the run fails here rather than computing off a
    // stale or guessed print; `docs/portfolio-analysis.md` §Failure posture).
    ctx.step_started("rates", "Load rate anchors (FRED)");
    let rates = match &resume {
        // Pinned with the run: resumed holdings must compute against the same
        // anchors the completed ones did (one coherent snapshot).
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

    // The run-level commodity context — fetched **once per run and shared across
    // every holding** (`docs/portfolio-workflow.md` §Step 5), wholly fail-soft:
    // every gap is typed onto the context, never a run failure. The step reads
    // "ok" whenever the leg ran; per-series gaps ride the roll-up's data health.
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

    // The run-level CFTC positioning pull — one bellwether-contract sweep shared
    // across every holding; a commodity / macro fund maps onto a row at dossier
    // time. Fail-soft like the commodity leg.
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

    // The optional CBOE venue-level put/call backdrop — one fail-soft fetch,
    // shared across every holding's dossier as broad-market sentiment context.
    ctx.step_started("sentiment", "Load CBOE put/call backdrop");
    let (put_call_backdrop, cboe_gap) = match &resume {
        Some(cp) => (cp.header.put_call_backdrop.clone(), cp.header.cboe_gap.clone()),
        None => market.put_call_backdrop(),
    };
    ctx.step_finished("sentiment", "ok", cboe_gap.clone());

    // The FINRA consolidated short-interest file — one fail-soft fetch per
    // run; each held stock reads it as a local lookup at dossier assembly
    // (risk / squeeze-context positioning evidence — `docs/data-sources.md
    // §FINRA`).
    ctx.step_started("short-interest", "Load FINRA short interest");
    let (short_interest_file, finra_gap) = match &resume {
        Some(cp) => (
            cp.header.short_interest_file.clone(),
            cp.header.finra_gap.clone(),
        ),
        None => market.short_interest(),
    };
    ctx.step_finished("short-interest", "ok", finra_gap.clone());

    // ---- Selective work-list (`docs/portfolio-analysis.md` §Triggering) ------
    // A selective run analyzes **strictly the user's selection** (ruled
    // 2026-08-16, `docs/verification/2026-08-16-selective-badges-ruling.md`). The
    // former automatic safety additions — a sweep flag, an `unknown` family, an
    // unexamined evidence event, a side reversal, an over-age exit — no longer
    // force-include; each instead surfaces as a **non-blocking card badge** so an
    // urgent single-holding run is never blocked by the rest of the book. (A
    // pre-`v9` verdict likewise no longer force-includes — the migration gate was
    // removed — but it carries silently, like any other, with no badge of its
    // own.) A current position with no prior verdict is left **not analyzed**
    // (no verdict emitted; the frontend renders it from holdings-minus-verdicts)
    // rather than pulled in. `None` = the whole-book run — a selective request with
    // an empty selection, or no prior run to carry from. (`created_at` / `today`
    // are minted above, before the house-view gate — the run's first dated
    // decision.)
    let mut swept_tail: std::collections::HashMap<
        String,
        crate::portfolio::quick_check::HoldingQuickState,
    > = std::collections::HashMap::new();
    let work_list: Option<std::collections::HashSet<String>> = if let Some(cp) = &resume {
        // The pinned selective work-list and tail sweep: a resumed selective run
        // keeps its exact selection, and the tail sweep's states ride the header
        // rather than re-spending its retrievals (a selective re-analysis
        // checkpoints identically — `docs/portfolio-analysis.md` §Failure
        // posture).
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
            // The engine-only quick check still sweeps the **carried tail** — every
            // unselected holding with a prior verdict to carry — but no longer to
            // expand the work-list. Its two remaining jobs: refresh each carried
            // verdict's condition eval-state overlay (so breach streaks and
            // acknowledgments chain into this run), and persist the attention flags /
            // evidence-event / degraded notes the card badges render. A position with
            // no prior verdict is not swept (nothing to check) and stays not analyzed.
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
                        fetched_at: now_rfc3339(),
                    },
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

    // Open this run's checkpoint trail (`docs/portfolio-analysis.md` §Failure
    // posture): a fresh run **discards any interrupted run's checkpoints** (its
    // partial verdicts never became a persisted run) and writes its own pinned
    // header; a resume keeps writing under the reopened header. Fail-soft — a
    // checkpoint write must never fail a run that can succeed (a stale trail is
    // caught by resume validation, never trusted).
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
    // The completed holdings a resume restores — their symbols skip the loop.
    let mut checkpointed: std::collections::HashSet<String> = std::collections::HashSet::new();
    // The context-fit observations and fired-retry events of every completed
    // holding — restored from the trail's rows in completion order on resume,
    // extended at each holding's checkpoint boundary by draining the analyst,
    // and handed to the roll-up whole, so a resumed run's data-health read
    // spans both processes. Each row carries its own holding's calls, so the
    // telemetry restored is exactly the rows restored: a holding whose row
    // never landed or no longer reads re-analyzes whole with its calls
    // re-issued, and the interrupted holding's abandoned calls reach no row
    // (`docs/portfolio-analysis.md` §Failure posture, ruled 2026-08-28).
    // The deep-history and benchmark health rides each row the same way
    // (Codex I17): the counts are rebuilt from `health_rows` at the roll-up,
    // never seeded from a cumulative accumulator.
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

    // A resume seeds the run-level keyed identities from the checkpoint trail
    // so the completed holdings' contributions survive into the finished run's
    // episode identities and prompt headers. The data-health counts are not
    // seeded: each restored row carried its own contribution into
    // `health_rows` above, and the roll-up rebuilds the counts from the rows
    // (Codex I17).
    let seeded = resume
        .as_ref()
        .map(|cp| cp.accumulators.clone())
        .unwrap_or_default();

    // The run-level sector-P/E surface, fetched on first need and memoized across
    // funds (`docs/portfolio-workflow.md` §Step 6a): the snapshot once per exchange
    // per candidate session tried (the walk is inside the source and is expected to
    // stop at the first), the per-sector histories as each fund's weightings
    // introduce sectors.
    let mut sector_pe_cache: Option<Vec<crate::portfolio::fund::SectorPe>> = None;
    // The snapshot is fetched once and memoized, so its failure has to be memoized
    // too: every fund's composite reads the same empty surface, so every fund's
    // gaps must carry the same reason. Pushing it only where the fetch happened
    // left funds 2..N abstaining as "no P/E-usable sector overlap" — the exact
    // misattribution the typed gap exists to prevent.
    let mut sector_pe_gap: Option<String> = None;
    let mut sector_history_cache: std::collections::HashMap<
        String,
        Vec<crate::portfolio::fund::SectorPe>,
    > = std::collections::HashMap::new();

    // The entry-stamped sector identities read at this run's fresh passes — one
    // fail-soft profile call per fresh-passed stock (`docs/portfolio-analysis.md`
    // §Outcome learning); a fund is a multi-sector vehicle by construction, typed
    // `sector-unscorable` without a profile call.
    let mut sector_by_symbol: std::collections::HashMap<
        String,
        crate::portfolio::outcome::SectorIdentity,
    > = seeded.sector_by_symbol;
    // The same profile lookup's issuer name, keyed alongside the sector so the
    // prompt header can name the company when Schwab's description is blank.
    let mut profile_name_by_symbol: std::collections::HashMap<String, Option<String>> =
        seeded.profile_name_by_symbol;
    // The same lookup's industry label — the commodity context's gold-linkage
    // key (an industry naming gold / precious metals, never the whole sector).
    let mut industry_by_symbol: std::collections::HashMap<String, Option<String>> =
        seeded.industry_by_symbol;
    // The run-level sector-benchmark series (FMP dated EOD, memoized per SPDR
    // symbol across holdings — `docs/portfolio-workflow.md` §Step 5): fetched on
    // first need by a carried stock whose pre-flag will read it; `None` caches a
    // failed or empty fetch so a broken benchmark costs one request, not one per
    // holding. The memo's second field is whether that fetch degraded (a gap
    // note or no closes), recorded on every reading holding's health row —
    // memo hit or fresh miss alike — and deduplicated by benchmark at the
    // roll-up. The memo is per process, so a run-level list pushed only at
    // the fetch counted a benchmark failing in both halves of a resumed run
    // twice (Codex I17).
    let mut benchmark_closes: std::collections::HashMap<
        String,
        (Option<Vec<crate::portfolio::engine::DatedValue>>, bool),
    > = std::collections::HashMap::new();

    for position in &holdings.positions {
        // A selective run analyzes only the work-list; everything else carries
        // its prior verdict forward vintage-stamped (appended after the loop).
        if work_list
            .as_ref()
            .is_some_and(|w| !w.contains(&position.symbol.to_ascii_uppercase()))
        {
            continue;
        }
        // A resumed run skips the holdings its checkpoints restored.
        if checkpointed.contains(&position.symbol.to_ascii_uppercase()) {
            continue;
        }
        if ctx.is_cancelled() {
            anyhow::bail!("run cancelled");
        }
        let step_key = crate::portfolio::holding_step_key(&position.symbol);
        ctx.step_started(step_key.clone(), format!("Analyze {}", position.symbol));

        // Gather the holding's evidence (fail-soft external data). The per-company FMP
        // and SEC calls poll cancellation before their requests; a SEC degradation is
        // folded into the financials' gap manifest so it reaches the audit and prompt
        // rather than vanishing into empty facts.
        let is_fund = matches!(
            position.asset_class,
            crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
        );
        let is_stock = matches!(position.asset_class, crate::portfolio::AssetClass::Stock);
        // One profile fetch per fresh-passed stock, feeding both the loop-time
        // listing-resolution guard (`docs/portfolio-analysis.md` §Asset eligibility)
        // and the entry-stamped sector identity; a fund is a multi-sector vehicle by
        // construction, typed `sector-unscorable` without a profile call.
        let listing = if is_stock {
            let lookup = company_data.profile_identity(&position.symbol);
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
                crate::portfolio::outcome::SectorIdentity::resolve(sector.as_deref()),
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
                    crate::portfolio::outcome::SectorIdentity::unscorable(
                        "multi-sector vehicle (fund)",
                    ),
                );
            }
            None
        };
        // A guard-terminal stock (unsupported listing / conflicting identity) skips
        // the remaining per-symbol retrieval — no statement, SEC, history, or chain
        // pull is spent on a holding the guard already routed; `analyze_holding`
        // routes on the resolution before touching the (empty) financials.
        let guard_terminal = matches!(
            &listing,
            Some(
                crate::portfolio::listing::ListingResolution::Unresolved
                    | crate::portfolio::listing::ListingResolution::NonUs { .. }
                    | crate::portfolio::listing::ListingResolution::Conflict { .. }
            )
        );
        // A class the equity pipeline never grades skips the same retrieval, for the
        // same reason: `pipeline::analyze_holding` routes every non-gradeable class
        // (options, fixed income, cash, unsupported) to `NotRated` with **default**
        // metrics before the engine stage, reading none of the statements, SEC facts,
        // deep history or chain fetched for it — so the gate cannot change any
        // output, only the budget. Ungated, a book's option and cash-equivalent rows
        // each spent the full per-symbol FMP surface plus an EDGAR facts call and a
        // deep-history leg to reach a verdict fixed before the first request.
        // This is what `data-sources.md`'s "per-holding (optionable equity)"
        // cardinality has always described.
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
        // the reduced path (quality is imputed, valuation composite-priced), and the
        // trust entity behind an ETF routinely 404s the facts API — pure gap noise
        // on the audit (the 2026-07-31 run's QQQ finding, F5).
        let sec_data = if is_fund || skip_retrieval {
            SecData::default()
        } else {
            company_data.facts(&position.symbol)
        };
        fmp_financials.gaps.extend(sec_data.gaps);
        // The SEC leg's outcome for the audit: the facts endpoint was queried (`Some`
        // — a clean fetch, an empty one, or a failed one with its gap) or never was
        // (a fund, a skipped retrieval, or no CIK mapping — the gap says which).
        let sec_leg = match &sec_data.facts {
            Some(facts) => dossier::LegOutcome::Got(facts),
            None => dossier::LegOutcome::NotRun,
        };
        // The item-classified 8-K filings sweep — the hard-forensic filing kinds'
        // producer, stocks only (a fund wrapper has no issuer-level filing to
        // classify). An `Unknown` sweep rides the gap manifest as a degraded
        // input; it never trips the hard rule.
        let filing_events = if is_fund || skip_retrieval {
            None
        } else {
            let since = (today
                - chrono::Duration::days(crate::portfolio::FORENSIC_EVENT_LOOKBACK_DAYS))
            .format("%Y-%m-%d")
            .to_string();
            company_data.filing_events(&position.symbol, &since)
        };
        if let Some(crate::portfolio::ForensicFilingState::Unknown { reason, .. }) =
            &filing_events
        {
            fmp_financials
                .gaps
                .push(format!("SEC filings sweep degraded: {reason}"));
        }
        // Deep dated history (FMP dated EOD) for the anchor join and drawdown reads.
        let (deep_closes, deep_gaps) = if skip_retrieval {
            (vec![], vec![])
        } else {
            company_data.deep_price_history(&position.symbol)
        };
        // This holding's deep-history health, carried on its checkpoint row: a
        // non-empty gap list means the FMP fetch degraded and the anchor
        // window starved to its documented fallback.
        let deep_history_failed = !deep_gaps.is_empty();
        if !deep_closes.is_empty() {
            fmp_financials.daily_closes = deep_closes;
        }
        fmp_financials.gaps.extend(deep_gaps);

        // The fund half for an ETF / mutual fund: metadata plus the memoized
        // sector-P/E surface (the strategy classification and reduced computation
        // happen in the engine stage — `docs/portfolio-workflow.md` §Step 6b).
        let fund_ctx = if is_fund {
            let mut fund = company_data.fund_data(&position.symbol);
            if sector_pe_cache.is_none() {
                // Dated on the run's pinned ET session (`today`), the same
                // session the fund context's `as_of` below carries — never a
                // fresh clock read at fetch time.
                sector_pe_cache = Some(match company_data.sector_pe_snapshot(today) {
                    Ok(rows) => rows,
                    Err(e) => {
                        sector_pe_gap = Some(format!("sector-P/E snapshot unavailable: {e}"));
                        vec![]
                    }
                });
            }
            // Carried to every fund, not just the one whose turn triggered the
            // fetch — they all price off the same memoized surface.
            if let Some(gap) = &sector_pe_gap {
                fund.gaps.push(gap.clone());
            }
            for (sector, _) in &fund.sector_weights {
                let key = sector.to_ascii_lowercase();
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    sector_history_cache.entry(key)
                {
                    let rows = match company_data.sector_pe_history(sector) {
                        Ok(rows) => rows,
                        Err(e) => {
                            fund.gaps
                                .push(format!("sector-P/E history unavailable for {sector}: {e}"));
                            vec![]
                        }
                    };
                    entry.insert(rows);
                }
            }
            // The underlying-positioning read: map this fund onto one of the
            // run-level COT rows (`docs/data-sources.md §CFTC`); an unmapped
            // fund — or a mapped contract whose row didn't land — fail-softs
            // to no read.
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
                // The run's ET session, not the UTC date: `as_of` drives
                // `fund::quarter_end_before`, so on a quarter boundary an
                // evening-ET run under a UTC date would treat the quarter that
                // has just ended as already complete and sample a snapshot
                // window the feed cannot serve yet.
                as_of: today,
                positioning,
            })
        } else {
            None
        };
        // Fail-soft chain fetch: an auth/server fault or a malformed response degrades
        // this holding's options signal to a gap, but — unlike a silent drop — it is
        // recorded in the manifest so it reaches the audit and prompt rather than reading
        // as "no options listed" (`docs/schwab-integration.md §Failure posture`). Never a
        // whole-job failure; the error carries status/context only, never a token.
        // The chain leg's outcome for the audit: requested and returned (`Got`),
        // requested and none came back or the request failed (`Empty` — a consulted
        // adapter either way), or never requested (`NotRun`, the retrieval gate).
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
        let mut prior = dossier::prior_verdict_for(prior_run.as_ref(), &position.symbol);
        // The prior verdict's effective analysis vintage — preserved on an
        // insufficient-evidence exit below, since an abstention is not a full pass
        // and the evidence-event boundary must not silently advance past events no
        // pass examined (`docs/portfolio-analysis.md` §Evidence floor).
        let prior_vintage = prior.as_ref().map(|p| {
            crate::portfolio::effective_vintage(&p.verdict, prior_created_at.as_deref().unwrap_or(""))
                .to_string()
        });
        if let Some(verdict) = prior.as_mut().map(|p| &mut p.verdict) {
            // The freshest condition evaluation states win: a carried
            // holding's in-run tail sweep already chained from the persisted
            // store, so its states supersede the store's; a selected holding
            // (never tail-swept) still overlays the store's.
            if let Some(h) = swept_tail.get(&position.symbol.to_ascii_uppercase()) {
                crate::portfolio::quick_check::overlay_condition_states(verdict, h);
            } else if let Some(h) = quick_state.as_ref().and_then(|qs| {
                qs.holdings
                    .iter()
                    .find(|h| h.symbol.eq_ignore_ascii_case(&position.symbol))
            }) {
                crate::portfolio::quick_check::overlay_condition_states(verdict, h);
            }
        }
        // This holding's sector-benchmark series — the pre-flag's read-against
        // leg, fetched only where the flag is evaluable at all (a carried stock
        // whose sector resolved to a SPDR benchmark). Its health-row read is
        // `Some(bench)` where the memoized fetch degraded, off a fresh fetch or
        // a memo hit alike.
        let mut benchmark_gap: Option<String> = None;
        let sector_benchmark = if is_stock && prior.is_some() {
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
        // The same-underlying option overlay (`docs/portfolio-workflow.md`
        // §Step 6a): the Step-2 pull's option rows linked by the deterministic
        // OCC symbol decode, with the **targeted delta fetch** per distinct held
        // strike — scoped to the held contracts' expiry window so the activity
        // signal's bounded NTM query is never widened. Each failed targeted
        // fetch degrades that strike's deltas to typed gaps, recorded.
        let option_overlay = if is_stock && !skip_retrieval {
            let key = position.symbol.to_ascii_uppercase();
            let option_rows: Vec<&crate::schwab::Position> = holdings
                .positions
                .iter()
                .filter(|p| {
                    // A zero-net row (fully offset contracts, deliberately kept
                    // by normalization) carries no economic exposure — never a
                    // leg, never a delta-fetch trigger.
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
                // "Consulted" comes from the capability probe, not the answer:
                // the stub default answers `Ok(None)` with no wire call, so
                // only a source that actually issues targeted requests labels
                // — a live empty answer included.
                let consulted =
                    holdings_source.supports_targeted_chain() && !strikes.is_empty();
                // Fetch failures ride the overlay's own gap list (it is the
                // record that owns the delta legs), not the financials manifest.
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
        // The per-holding FINRA lookup off the once-per-run file — stocks only
        // (`docs/data-sources.md §FINRA`: a held-equity risk / squeeze read; a
        // fund wrapper has no issuer-level short-interest row). A symbol absent
        // from the consolidated file carries no read — a market fact, not a gap.
        let short_interest = if is_stock && !skip_retrieval {
            short_interest_file
                .as_ref()
                .and_then(|f| f.by_symbol.get(&position.symbol.to_ascii_uppercase()))
                .cloned()
        } else {
            None
        };
        // Step-6a semantic continuity retrieval (`docs/portfolio-workflow.md`
        // §Step 6a): a deterministic query over the holding's identity and the
        // prior verdict's themes, embedded and cosine-searched against this
        // job's own `summary` partition — fail-soft: a failed lane records a
        // degraded input; the deterministically loaded prior verdict and ledger
        // are unaffected. Skipped whole for a holding the loop never grades.
        let semantic_recall = if !skip_retrieval {
            let symbol_key = position.symbol.to_ascii_uppercase();
            let query = semantic_query_text(
                &position.symbol,
                sector_by_symbol.get(&symbol_key).and_then(|s| s.sector.as_deref()),
                industry_by_symbol.get(&symbol_key).and_then(|i| i.as_deref()),
                prior.as_ref(),
            );
            semantic_recall_for(
                conn,
                outcome_sources.and_then(|s| s.embedder),
                &query,
            )
        } else {
            dossier::SemanticRecall::default()
        };
        // The dossier's research-loop seed leg (`docs/portfolio-workflow.md`
        // §Step 6a): symbol-scoped news since the shared research-freshness
        // window, as typed seeds with stable app-assigned IDs — leads, never
        // evidence. Stocks only (the endpoint is company-scoped); a row the
        // wire served without a URL cannot be deep-read and is dropped.
        let news_seeds: Vec<crate::portfolio::research::ResearchSeed> =
            if is_stock && !skip_retrieval {
                let from = (today
                    - chrono::Duration::days(crate::portfolio::research::RESEARCH_FRESHNESS_DAYS))
                .format("%Y-%m-%d")
                .to_string();
                company_data
                    .news_items(&position.symbol, &from)
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, n)| {
                        let url = n.url?;
                        Some(crate::portfolio::research::ResearchSeed {
                            id: format!("seed-{}", i + 1),
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
        // The persisted per-topic layer (the research-reuse priors). A read
        // failure degrades to a cold loop, never a failed run.
        let research_priors = if !skip_retrieval {
            store::load_topic_distillates(conn, &position.symbol).unwrap_or_default()
        } else {
            Vec::new()
        };
        let dossier: HoldingDossier = dossier::assemble(
            position.clone(),
            holdings_diff.delta_for(&position.symbol),
            fmp_financials,
            // Each leg's consulted / empty / not-run outcome, so the audit can label a
            // consulted-but-empty fetch and skip a leg that never ran
            // (`dossier::assemble`).
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
            // The run-level commodity prints matched to this holding's profile
            // sector (stocks only — a fund's commodity read is the designed
            // CFTC underlying-positioning leg, not a price block).
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
            semantic_recall,
            news_seeds,
            research_priors,
        );

        // Cancellation checkpoint between the (now-complete) data gather and the model
        // stages, so a cancel mid-gather is observed before any model call is spent.
        if ctx.is_cancelled() {
            anyhow::bail!("run cancelled");
        }

        // The model/grade half is fail-hard: an interpretation or persistence error
        // fails the whole run (`docs/local-models.md §Failure posture`).
        // The run date keys the ledger evaluation's observation identities and
        // timestamps (deterministic under test — injected, never re-derived inside
        // the engine). It is the run's **ET session date**, taken from the run's
        // one instant rather than re-derived per holding: the values it stamps —
        // `first_breach_at`, `last_evaluated_at`, and the `confirmed_at` the
        // falsifier lead-time read positions against bar dates — are all session
        // quantities, and re-deriving per holding let a midnight-crossing run
        // stamp one book across two days.
        let run_date = run_session_date.clone();
        let (mut verdict, audit) =
            analyze_holding(analyst, &dossier, &rates, &run_date)?;
        if matches!(
            verdict.disposition,
            crate::portfolio::VerdictDisposition::InsufficientEvidence { .. }
        ) {
            verdict.analyzed_at = prior_vintage;
        }
        ctx.step_finished(step_key, "ok", None);
        // Persist the holding's fresh per-topic distilled-findings layer — the
        // next run's research seeds, surviving independently of run retention
        // (`docs/portfolio-analysis.md` §Starting parameters). Fail-soft: a
        // lost write costs the next run's warm seeds, never this run.
        if let Some(research) = &audit.research {
            if !research.seed_layer.is_empty() {
                if let Err(e) =
                    store::save_topic_distillates(conn, &position.symbol, &research.seed_layer)
                {
                    eprintln!(
                        "research seed layer: write failed for {} ({e})",
                        position.symbol
                    );
                }
            }
            // A topic the distillation failed to re-emit reconciled loses its
            // stored row — a stale seed must not survive the run that should
            // have rewritten it (each is also a recorded gap).
            if !research.unreconciled_topics.is_empty() {
                if let Err(e) = store::delete_topic_distillates(
                    conn,
                    &position.symbol,
                    &research.unreconciled_topics,
                ) {
                    eprintln!(
                        "research seed layer: stale-row delete failed for {} ({e})",
                        position.symbol
                    );
                }
            }
        }
        verdicts.push(verdict);
        audits.push(audit);

        // Mid-run checkpoint (`docs/portfolio-analysis.md` §Failure posture):
        // the completed holding — an insufficient-evidence exit included —
        // persists so a cancellation or a single model failure resumes the
        // unfinished holdings rather than restarting the run. Fail-soft: losing
        // a checkpoint must never fail a run that can succeed.
        // Drain the holding just completed: its calls ride its own row, and
        // the run-level vectors take a copy *before* the fail-soft write, so a
        // lost checkpoint never loses an in-process observation while the row
        // that did not land takes its calls out of the trail with it. Its
        // health row rides the same way (Codex I17).
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

    // Stamp each fresh pass's analysis vintage with the run's own `created_at`
    // (minted at run start, above — `docs/portfolio-analysis.md` §Triggering:
    // carried verdicts ride vintage-stamped, so a fresh one must be
    // distinguishable). An abstention already carries its preserved prior
    // vintage from the loop.
    for v in &mut verdicts {
        if !matches!(
            v.disposition,
            crate::portfolio::VerdictDisposition::InsufficientEvidence { .. }
        ) {
            v.analyzed_at = Some(created_at.clone());
        }
    }

    // ---- Carried verdicts (a selective run's unselected tail) ----------------
    // Each carries its prior intrinsic verdict and ledger forward vintage-stamped
    // (`docs/portfolio-analysis.md` §Triggering), with the tail sweep's fresher
    // condition evaluation states overlaid so streaks and acknowledgments chain,
    // its position-change tag refreshed from this run's diff, and its prior audit
    // row carried whole — the stored `quick_basis` / `fund_exposure` comparators
    // must survive the carry or the next sweep reads the holding `unknown`. Since
    // the 2026-08-16 badge ruling a carried holding is never force-included, so a
    // side-reversed carry (marked here for its card badge) and an over-age
    // exit-family carry now stand — badged, not re-analyzed. The one deterministic
    // carry rule left is the over-age add-family demotion to *hold*, stamped
    // `action_source: rule-demoted` (over-age holds and exits stand as-is).
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
                // No prior verdict to carry — an unselected new holding is left
                // not analyzed since the 2026-08-16 badge ruling (the frontend
                // renders it from holdings-minus-verdicts); a full run or an
                // explicit selection grades it.
                continue;
            };
            let mut carried = prior_verdict.clone();
            let vintage =
                crate::portfolio::effective_vintage(prior_verdict, &prior.created_at).to_string();
            carried.analyzed_at = Some(vintage.clone());
            let carried_delta = holdings_diff.delta_for(&position.symbol);
            carried.position_change = carried_delta.change;
            // A carried directional verdict (priced or role/risk) is "side-reversed"
            // when it now describes the opposite position — marked for the card badge
            // (`docs/portfolio-analysis.md` §Triggering); no longer force-included. A
            // directional verdict is only ever authored for a **long** position — a
            // net-short or net-zero holding takes the not-rated treatment at the
            // eligibility gate (`pipeline.rs`) — so its authoring side is invariantly
            // long, and it is reversed exactly when the position it now sits on is
            // **net-short**, whatever path it took there. Comparing that invariant
            // authoring side against the current side is robust where a per-run flip
            // read is not: a flip *through* an exactly-zero net (kept by netting) is
            // invisible to the diff's sign compare. A fresh pass re-authors for the
            // current side (or returns not-rated) and leaves the field `false`.
            carried.side_reversed = position.quantity < 0.0
                && matches!(
                    carried.disposition,
                    crate::portfolio::VerdictDisposition::Priced(_)
                        | crate::portfolio::VerdictDisposition::RoleRiskOnly(_)
                );
            if let Some(h) = swept_tail.get(&key) {
                crate::portfolio::quick_check::overlay_condition_states(&mut carried, h);
            }
            // The intrinsic *action* carries as-is (rung-only since
            // `portfolio-v9` — no sizing to recompute). The over-age
            // add-family demotion is the one deterministic carry rule.
            let stale = over_age(&vintage, today);
            if stale {
                over_age_carried.insert(key.clone());
            }
            match &mut carried.disposition {
                crate::portfolio::VerdictDisposition::Priced(g)
                    if stale && g.action.is_add_family() =>
                {
                    g.action = crate::portfolio::Action::Hold;
                    carried.action_source = crate::portfolio::ActionSource::RuleDemoted;
                }
                // A role-risk verdict can carry an add-family action (the action
                // call's choice is structurally open), so the stale-strong-action
                // rule is branch-unscoped (`docs/portfolio-analysis.md`
                // §Triggering).
                crate::portfolio::VerdictDisposition::RoleRiskOnly(r)
                    if stale && r.action.is_add_family() =>
                {
                    r.action = crate::portfolio::Action::Hold;
                    carried.action_source = crate::portfolio::ActionSource::RuleDemoted;
                }
                _ => {}
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

    // The episode store loads once; the outcome pass below consumes it.
    let (mut episodes, unreadable_active_symbols) = match store::load_episodes(conn) {
        Ok(load) => {
            // A skipped *active* row (unreadable JSON, readable SQL columns)
            // re-seeds its symbol through the plan's recovery seam; the row
            // itself is never deleted.
            let lost =
                crate::portfolio::outcome::lost_active_symbols(&load.skipped, &load.episodes);
            (load.episodes, lost)
        }
        Err(e) => {
            // Store-level failure only — a single bad row is skipped and logged
            // inside the loader, never an error here. Proceeding with an empty
            // set re-debuts the whole book (the never-seeded-symbol rule); this
            // log line is what makes that state diagnosable rather than silent.
            eprintln!(
                "outcome learning: episode store unreadable ({e}) — proceeding with an empty set"
            );
            (Vec::new(), std::collections::HashSet::new())
        }
    };

    // ---- Roll-up + outcome pass (the loop's actions are final since
    // `portfolio-v9` — no construction stage exists to reconcile them) ----------
    if ctx.is_cancelled() {
        anyhow::bail!("run cancelled");
    }
    // Anything recorded past the last checkpoint boundary (nothing today — every
    // loop call precedes its holding's checkpoint); kept so every recorded
    // observation reaches the read.
    prompt_usage.extend(analyst.take_prompt_usage());
    model_retries.extend(analyst.take_retry_events());
    // The data-health counts, rebuilt from every completed holding's row —
    // restored and this process's alike — so a resume counts a re-analyzed
    // holding once and a benchmark failing in both processes once (Codex I17).
    let health = health_counts(&health_rows);
    let roll_up = build_roll_up(
        &holdings,
        &verdicts,
        &holdings_diff.exited,
        &audits,
        health.deep_history_failures,
        rates.history_gap.is_some(),
        house_view_omitted,
        FeedGaps {
            commodity: commodities.gaps.len(),
            positioning: cot_gaps.len(),
            cboe: cboe_gap.is_some(),
            finra: finra_gap.is_some(),
            benchmark: health.benchmark_gaps.len(),
        },
        prompt_usage,
        model_retries,
    );
    // The deterministic outcome half: tag active episodes' net alignment from this
    // run's diff, refresh label-time price series through the shared bar cache and
    // record any newly due window labels (fail-soft — a failed retrieval leaves a
    // label pending, never a run failure), then append-or-extend this run's
    // decision episodes and derive the scorecard reads, all landing on the run
    // blob's outcome records.
    ctx.step_started("outcome", "Outcome learning");
    // `run_id` was minted at run start (or reopened by a resume) so the
    // checkpoint trail could key on it.
    // The run's ET session date, the same string the per-holding ledger
    // evaluation stamped — `mature_labels` takes it beside the ET `today` below,
    // and a UTC prefix here would disagree with that `today` on an evening run.
    let run_date: String = run_session_date.clone();
    let (alignment_tags, align_changed) = crate::portfolio::outcome::tag_alignment(
        &mut episodes,
        prior_run_id.as_deref(),
        &holdings,
        &holdings_diff,
    );
    let mut series_ctx =
        crate::portfolio::outcome::SeriesCtx::new(conn, outcome_sources.map(|s| s.price));
    let label_summary =
        crate::portfolio::outcome::mature_labels(&mut episodes, &mut series_ctx, today, &run_date);
    drop(series_ctx);
    let plan = crate::portfolio::outcome::plan_episodes(
        &crate::portfolio::outcome::PlanInput {
            run_id: &run_id,
            created_at: &created_at,
            verdicts: &verdicts,
            audits: &audits,
            prior_verdicts: prior_run.as_ref().map(|r| r.verdicts.as_slice()),
            sector_by_symbol: &sector_by_symbol,
            dgs2: Some(rates.dgs2),
            unreadable_active_symbols,
            carried_symbols: &carried_symbols,
        },
        &mut episodes,
    );
    let reads = crate::portfolio::outcome::derive_reads(&episodes);
    let outcome_records = crate::portfolio::outcome::OutcomeRecords {
        opened: plan.opened,
        extended: plan.extended,
        alignment_tags,
        matured: label_summary.matured,
        pending_coverage: label_summary.pending_coverage,
        reads,
    };
    let mut changed_episodes = align_changed;
    changed_episodes.extend(label_summary.changed);
    changed_episodes.extend(plan.changed);
    ctx.step_finished("outcome", "ok", None);

    let run = PortfolioRun {
        run_id,
        created_at: created_at.clone(),
        holdings,
        verdicts,
        roll_up,
        audit: audits,
        // The persisted rate cache the engine-only quick paths' fail-soft reads
        // (`docs/portfolio-analysis.md` §The quick check).
        rate_prints: Some(crate::portfolio::RatePrints {
            dgs2: rates.dgs2,
            dgs10: rates.dgs10,
            dgs2_as_of: rates.dgs2_date.clone(),
            dgs10_as_of: rates.dgs10_date.clone(),
            fetched_at: created_at.clone(),
        }),
        outcome: Some(outcome_records),
    };

    ctx.step_started("persist", "Persist run");
    // One transaction: the run row and the episode mutations it claims land (and
    // prune) together, so a failed write can never leave the episode store
    // claiming a run that was never persisted.
    let tx = conn.unchecked_transaction()?;
    store::insert_run(&tx, &run)?;
    for ep in episodes
        .iter()
        .filter(|e| changed_episodes.contains(&e.episode_id))
    {
        store::save_episode(&tx, ep)?;
    }
    store::prune_runs(&tx, crate::portfolio::PORTFOLIO_RUN_RETENTION)?;
    store::prune_matured_episodes(&tx, crate::portfolio::outcome::MATURED_ARCHIVE_CAP)?;
    tx.commit()?;

    // Matured reads embed as durable learnings in the Portfolio memory partition —
    // best-effort: a failed or invalid embedding costs the memory row (logged),
    // never the persisted run (`docs/portfolio-analysis.md` §Outcome learning).
    if let (Some(sources), Some(records)) = (outcome_sources, run.outcome.as_ref()) {
        if let Some(embedder) = sources.embedder {
            if let Some(text) = crate::portfolio::outcome::matured_learning_text(records, &run_date)
            {
                match embedder.embed(&text) {
                    Ok(vector) => {
                        if let Err(e) = crate::vector_memory::insert_memory(
                            conn,
                            crate::vector_memory::MemoryKind::Learning,
                            crate::vector_memory::MemoryNamespace::Portfolio,
                            None,
                            &text,
                            &vector,
                            &created_at,
                        ) {
                            eprintln!("outcome learning: durable-learning insert failed: {e}");
                        }
                    }
                    Err(e) => eprintln!(
                        "outcome learning: matured-read embedding failed (learning row skipped): {e}"
                    ),
                }
            }
        }
    }
    // Per-holding verdict summaries embed as continuity `summary` rows in the
    // Portfolio partition (`docs/portfolio-workflow.md` §Step 7's run-result
    // embeddings) — fresh-vintage analyzed verdicts only (a carried verdict's
    // summary already rode its authoring run), keyed `{run_id}:{SYMBOL}` so the
    // rows prune with their run (`store::prune_runs`) under the summary-kind
    // unique index. Best-effort like the learning row above: a failed or
    // invalid embedding costs that holding's memory row, never the persisted
    // run.
    if let Some(embedder) = outcome_sources.and_then(|s| s.embedder) {
        for v in &run.verdicts {
            if v.analyzed_at.as_deref() != Some(created_at.as_str()) {
                continue;
            }
            let Some(text) = holding_summary_text(v) else {
                continue;
            };
            let row_id = format!("{}:{}", run.run_id, v.symbol.to_ascii_uppercase());
            match embedder.embed(&text) {
                Ok(vector) => {
                    if let Err(e) = crate::vector_memory::insert_memory(
                        conn,
                        crate::vector_memory::MemoryKind::Summary,
                        crate::vector_memory::MemoryNamespace::Portfolio,
                        Some(&row_id),
                        &text,
                        &vector,
                        &created_at,
                    ) {
                        eprintln!(
                            "holding summary: memory insert failed for {} (row skipped): {e}",
                            v.symbol
                        );
                    }
                }
                Err(e) => eprintln!(
                    "holding summary: embedding failed for {} (row skipped): {e}",
                    v.symbol
                ),
            }
        }
    }
    // The successful full pass consumed each analyzed holding's triggering
    // observations in interpretation / continuity (the acknowledgment stamps ride
    // the 6g seam), so those holdings' quick-check flags, badges, and carried
    // states end with it — but clearing is **per successful pass**, never
    // wholesale: an `insufficient-evidence` exit is not a successful pass
    // (`docs/portfolio-analysis.md` §Evidence floor — the attention flag and
    // unexamined events survive it), so an abstaining holding's carried state is
    // retained, re-stamped to the new run so the next sweep chains from it
    // instead of superseding it (`docs/portfolio-analysis.md §The quick check`).
    // A selective run widens the retention the same way: a carried holding got no
    // full pass either, so its sweep state — freshly merged by the in-run tail
    // sweep where one ran — is retained re-stamped to the new run rather than
    // cleared (`docs/portfolio-analysis.md` §Triggering).
    // Fail-soft, matching the run-start read's posture (`.ok().flatten()`): the
    // run row committed above, so an error in this bookkeeping step must not
    // record a durably persisted run as Failed — the "failed" run would still
    // be the next run's diff baseline and carry source, a half-written
    // lifecycle state. The cost of a swallowed error here is a stale
    // quick-check row the next sweep supersedes.
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
            // The in-run sweep's merged state is the freshest; the persisted store row
            // covers a holding the sweep did not cover (a selected holding that
            // abstained, or a full run's abstention).
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
                // shadow the fresher prints this run just fetched — the next sweep's
                // fail-soft prefers the prior state's cache over the run blob's.
                rate_cache: run.rate_prints.clone(),
                holdings: retained_holdings,
            };
            store::save_quick_check(conn, &state)?;
        }
        Ok(())
    })();
    if let Err(e) = retention {
        eprintln!("quick-check retention after run persist failed (run kept): {e}");
    }
    // The run persisted whole, so its checkpoint trail has served its purpose —
    // cleared like the fail-soft bookkeeping above (a leftover trail is caught
    // by resume validation, never trusted).
    if let Err(e) = store::clear_checkpoints(conn) {
        eprintln!("portfolio checkpoint: clear after run persist failed (run kept): {e}");
    }
    ctx.step_finished("persist", "ok", None);

    Ok(run)
}

/// Run-level enriching-feed gap counts feeding data health — counted, never
/// attention: every feed here is fail-soft and additive
/// (`docs/portfolio-analysis.md` §Failure posture), so a gap is surfaced on the
/// roll-up line without tripping the infrastructure flag.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FeedGaps {
    pub commodity: usize,
    pub positioning: usize,
    pub cboe: bool,
    pub finra: bool,
    pub benchmark: usize,
}

/// The run-level data-health counts every completed holding's health row
/// contributes ([`store::HoldingHealth`]): the holdings whose deep-history
/// fetch degraded, and the distinct sector-benchmark series any holding read
/// as unavailable. Rebuilt from the rows — restored and fresh alike — rather
/// than accumulated, so a resumed run counts a re-analyzed holding once and a
/// benchmark failing in both processes once (Codex I17).
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

/// Build the deterministic portfolio roll-up (`docs/portfolio-analysis.md` §Portfolio
/// roll-up): verdict counts, the concentration read (largest position weight), the cash
/// stance, the positions closed since the last run (the Step-4 diff's exited
/// names), and the run-level **data-health** aggregate over the per-holding audits —
/// so a degraded-but-successful run (the 2026-07-31 "43 of 44 anchor windows empty"
/// pattern) is visible at a glance rather than only inside 47 audit records.
/// Descriptive only; whole-book reasoning is the future portfolio planner's.
#[allow(clippy::too_many_arguments)]
fn build_roll_up(
    holdings: &Holdings,
    verdicts: &[HoldingVerdict],
    exited: &[ExitedPosition],
    audits: &[HoldingAudit],
    deep_history_failures: usize,
    dgs10_history_gap: bool,
    house_view_omitted: bool,
    feed_gaps: FeedGaps,
    prompt_usage: Vec<crate::local_model::PromptUsage>,
    model_retries: Vec<crate::local_model::RetryEvent>,
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
    // Usable total, finite quotients: both weights persist as required floats,
    // and a subnormal total overflows the division (Codex I16). A weight the
    // arithmetic cannot finish reads 0, the same as no total.
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

    // Acknowledge positions closed since the last run rather than letting them vanish.
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
        top_position_weight,
        cash_weight,
        exited: exited.to_vec(),
        data_health: Some(build_data_health(
            audits,
            deep_history_failures,
            dgs10_history_gap,
            house_view_omitted,
            feed_gaps,
            prompt_usage,
            model_retries,
        )),
        overview: format!(
            "{graded} graded{role_note}, {not_rated} not rated, {insufficient} \
             insufficient-evidence; top position {:.0}% of the account, cash {:.0}%.{exited_note}",
            top_position_weight * 100.0,
            cash_weight * 100.0
        ),
    }
}

/// Aggregate the run-level data-health read from the per-holding audits' typed
/// `target_meta` plus the run-scoped deep-history counters — no string matching on
/// gap notes. `attention` marks *infrastructure* degradation (a failed deep-history
/// source, a target on the current-multiple carry, a failed DGS10 history request);
/// a raw-percentile fallback from genuinely thin issuer history is counted in the
/// line but is an honest state, not an attention trigger.
fn build_data_health(
    audits: &[HoldingAudit],
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
    if feed_gaps.benchmark > 0 {
        parts.push(format!(
            "sector benchmark series failed on {} symbol(s)",
            feed_gaps.benchmark
        ));
    }

    // The context-fit read (`docs/portfolio-analysis.md` §Portfolio roll-up), two
    // triggers with distinct signatures: **near-full** — a prompt at or past the
    // pressure fraction of its `num_ctx` is one digest away from truncation; and
    // **likely front-truncation** — a reported count too small to cover the chars
    // the app actually sent (Ollama's count is post-truncation and lands far
    // *below* `num_ctx`, so the fill fraction alone cannot see it — the preflight
    // marker test's 1,026-of-2,048 signature). The peak fill is recorded
    // regardless — the big-run prompt-fit watch's measurement.
    // A daemon-omitted `prompt_eval_count` records as `None` (the row survives
    // for its output-side observation): fill reads 0 and truncation reads
    // false, so a count-less row can never enter a context-fit line.
    let fill = |u: &crate::local_model::PromptUsage| {
        u.prompt_tokens.unwrap_or(0) as f64 / u.num_ctx as f64
    };
    let truncated = |u: &crate::local_model::PromptUsage| {
        u.prompt_tokens.is_some_and(|t| {
            t.saturating_mul(crate::portfolio::TRUNCATION_CHARS_PER_TOKEN) < u.prompt_chars
        })
    };
    let peak_prompt = prompt_usage
        .iter()
        .filter(|u| u.num_ctx > 0 && u.prompt_tokens.is_some())
        .max_by(|a, b| fill(a).total_cmp(&fill(b)))
        .cloned();
    // The output-budget read (`num_predict` — `docs/verification/
    // 2026-08-10-big-run-attempt-1.md` §Fix candidates 4): a length stop
    // already failed its call typed, and the observation lands here so the
    // run-level surface names it too — a degraded run's roll-up carries it.
    let output_limited: Vec<crate::local_model::PromptUsage> = prompt_usage
        .iter()
        .filter(|u| u.output_limited)
        .cloned()
        .collect();
    let context_pressure: Vec<crate::local_model::PromptUsage> = prompt_usage
        .into_iter()
        .filter(|u| {
            u.num_ctx > 0
                && (fill(u) >= crate::portfolio::CONTEXT_PRESSURE_FRACTION || truncated(u))
        })
        .collect();
    if let Some(worst) = output_limited
        .iter()
        .max_by_key(|u| u.completion_tokens.unwrap_or(0))
    {
        // `done_reason: "length"` covers two stops with different levers — the
        // call's own output reservation, or the shared context filling first —
        // so the line says which one the worst row's counts show, through the
        // same single-homed predicate the per-call typed failure used
        // (`local_model::length_stop_reading`), and claims nothing when the
        // counts are incomplete.
        let reading = crate::local_model::length_stop_reading(
            worst.completion_tokens,
            worst.num_predict,
        );
        parts.push(format!(
            "generation length-stopped on {} local call{} (worst: {} generated {} of {} \
             reserved — {})",
            output_limited.len(),
            if output_limited.len() == 1 { "" } else { "s" },
            worst.stage,
            worst
                .completion_tokens
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
                    "counts incomplete; stop unattributed"
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
            "likely front-truncation on {} local call{} (worst: {} reported {} tokens for a \
             {}-char prompt, num_ctx {})",
            truncation_suspects.len(),
            if truncation_suspects.len() == 1 { "" } else { "s" },
            worst.stage,
            // `truncated` requires a reported count, so this is always Some.
            worst.prompt_tokens.unwrap_or(0),
            worst.prompt_chars,
            worst.num_ctx
        ));
    }
    // The bounded retry-once's fired events (`docs/local-models.md §The
    // local-model adapter seam`): in a persisted run every listed re-attempt
    // succeeded (a second failure fails the run), so the line measures the
    // absorbed transient rate — the big-run retry watch's read.
    if let Some(first) = model_retries.first() {
        parts.push(format!(
            "bounded retry absorbed {} transient model-call failure{} (first: {} — {})",
            model_retries.len(),
            if model_retries.len() == 1 { "" } else { "s" },
            first.stage,
            first.cause
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
            "context pressure on {near_full} local call{} (worst: {} at {} of {} tokens)",
            if near_full == 1 { "" } else { "s" },
            worst.stage,
            // A near-full fill requires a reported count, so this is always Some.
            worst.prompt_tokens.unwrap_or(0),
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
        benchmark_gaps: feed_gaps.benchmark,
        context_pressure,
        peak_prompt,
        model_retries,
        attention,
        summary,
    }
}

/// Current time as an RFC3339 UTC string — the canonical persisted form, like
/// [`crate::jobs`]; local-time conversion is a display concern at the UI seam.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// The fail-soft shell around a prior-state store read (the prior run, the
/// quick-check state): a store `Err` — a SQL / query failure, since the store
/// already loud-skips corrupt rows to `Ok(None)` — degrades to `None` ("no prior
/// state"), never a run failure, but leaves one labeled stderr line, the
/// portfolio counterpart of the report pipeline's `read_db_fail_soft`. Without
/// the trace a degraded read was indistinguishable from a genuine first run —
/// no diff baseline, no carries, no quick-check chaining — with nothing to show
/// for it.
fn prior_state_read<T>(what: &str, read: Result<Option<T>>) -> Option<T> {
    match read {
        Ok(state) => state,
        Err(e) => {
            eprintln!("[portfolio-job] {what} read degraded to none: {e:#}");
            None
        }
    }
}

/// The ordered sector-P/E snapshot dates to try for a run whose **ET session date**
/// is `today`: that session first, then earlier weekdays, weekends skipped without
/// spending a request. Shares the report chain's walk
/// ([`crate::fmp::sector_candidate_dates`]) so both jobs treat holidays alike.
fn sector_pe_candidates(today: chrono::NaiveDate) -> Vec<chrono::NaiveDate> {
    crate::fmp::sector_candidate_dates(today, crate::fmp::SECTOR_LOOKBACK_WEEKDAYS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::pipeline::StubAnalyst;
    use crate::portfolio::{AssetClass, PositionChange};
    use crate::schwab::{FixtureHoldingsSource, Position};

    /// The sector-P/E snapshot is date-keyed, so an evening-ET run must ask for the
    /// session that traded, not the UTC calendar day it has already rolled into —
    /// and it must walk back over holidays rather than accept the empty answer.
    /// The UTC read returned `Ok(vec![])`, which every priced US-equity fund then
    /// misattributed to "no P/E-usable sector overlap".
    #[test]
    fn roll_up_weights_over_an_unusable_total_read_zero_never_non_finite() {
        // Codex I16 (ruled 2026-08-29): both weights persist as required
        // floats, and a subnormal account total overflows the quotient. A
        // weight the arithmetic cannot finish reads 0, the same as no total.
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
            0,
            false,
            false,
            FeedGaps::default(),
            vec![],
            vec![],
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
        // The walk continues over earlier weekdays so a market holiday gaps one
        // candidate instead of stranding the whole fund cohort.
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
    fn over_age_dates_the_vintage_on_its_et_session() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        // 2026-07-08 01:30 UTC = 2026-07-07 21:30 EDT: the vintage belongs to
        // the ET session of the 7th — 29 days before today, over-age. The UTC
        // date prefix (the 8th, exactly 28 days) would have read it one day
        // younger and let the carry stand.
        assert!(over_age("2026-07-08T01:30:00+00:00", today));
        // A daytime vintage on the boundary day itself stays within age.
        assert!(!over_age("2026-07-08T15:00:00+00:00", today));
        // Unparseable stays conservatively over-age.
        assert!(over_age("soon", today));
    }

    /// The data-health counts are rebuilt from the completed holdings' rows:
    /// each degraded deep-history fetch counts once per holding, and a
    /// benchmark read as unavailable by several holdings — or by one holding
    /// in each half of a resumed run — counts once (Codex I17).
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

    /// The enriching-feed gaps are counted and named on the summary line but
    /// never trip attention — every feed is fail-soft and additive
    /// (`docs/portfolio-analysis.md` §Failure posture).
    #[test]
    fn data_health_counts_feed_gaps_without_raising_attention() {
        let dh = build_data_health(
            &[],
            0,
            false,
            false,
            FeedGaps {
                commodity: 2,
                positioning: 1,
                cboe: true,
                finra: true,
                benchmark: 1,
            },
            vec![],
            vec![],
        );
        assert_eq!(dh.commodity_gaps, 2);
        assert_eq!(dh.positioning_gaps, 1);
        assert!(dh.cboe_gap);
        assert!(dh.finra_gap);
        assert_eq!(dh.benchmark_gaps, 1);
        assert!(!dh.attention, "enriching-feed gaps never trip attention");
        assert!(dh.summary.contains("commodity context: 2 series gap(s)"), "{}", dh.summary);
        assert!(dh.summary.contains("CFTC positioning: 1 contract gap(s)"), "{}", dh.summary);
        assert!(dh.summary.contains("CBOE put/call backdrop unavailable"), "{}", dh.summary);
        assert!(dh.summary.contains("FINRA short interest unavailable"), "{}", dh.summary);
        assert!(dh.summary.contains("sector benchmark series failed on 1 symbol(s)"), "{}", dh.summary);
        // Clean feeds leave the line untouched.
        let dh = build_data_health(&[], 0, false, false, FeedGaps::default(), vec![], vec![]);
        assert!(!dh.summary.contains("commodity"), "{}", dh.summary);
    }

    /// The bounded retry-once's data-health read: an absorbed transient is
    /// named in the summary, carried structured, and trips attention — the
    /// big-run retry watch's measurement.
    #[test]
    fn data_health_names_absorbed_retries_and_raises_attention() {
        let retries = vec![crate::local_model::RetryEvent {
            stage: "interpret WID".into(),
            cause: "daemon error status".into(),
        }];
        let dh = build_data_health(&[], 0, false, false, FeedGaps::default(), vec![], retries);
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
        // No fired retries: no line, no attention from this trigger.
        let dh = build_data_health(&[], 0, false, false, FeedGaps::default(), vec![], vec![]);
        assert!(!dh.attention);
        assert!(!dh.summary.contains("bounded retry"), "{}", dh.summary);
    }

    /// The context-fit fold: a call at or past the pressure fraction of its
    /// `num_ctx` is named in the summary and trips attention; the peak fill is
    /// recorded either way — the big-run prompt-fit watch's measurement.
    #[test]
    fn data_health_flags_context_pressure_and_records_the_peak() {
        let usage = vec![
            crate::local_model::PromptUsage {
                stage: "interpret AAPL".into(),
                prompt_tokens: Some(50_000),
                num_ctx: 131_072,
                prompt_chars: 200_000,
                completion_tokens: None,
                num_predict: None,
                output_limited: false,
            },
            crate::local_model::PromptUsage {
                stage: "construction".into(),
                prompt_tokens: Some(125_000),
                num_ctx: 131_072,
                prompt_chars: 500_000,
                completion_tokens: None,
                num_predict: None,
                output_limited: false,
            },
        ];
        let dh = build_data_health(&[], 0, false, false, FeedGaps::default(), usage, vec![]);
        assert_eq!(dh.context_pressure.len(), 1);
        assert_eq!(dh.context_pressure[0].stage, "construction");
        assert_eq!(dh.peak_prompt.as_ref().unwrap().stage, "construction");
        assert!(dh.attention, "{}", dh.summary);
        let expected = "context pressure on 1 local call (worst: construction at 125000 of \
                        131072 tokens)";
        assert!(dh.summary.contains(expected), "{}", dh.summary);
    }

    #[test]
    fn data_health_records_the_peak_without_pressure() {
        let usage = vec![crate::local_model::PromptUsage {
            stage: "interpret MSFT".into(),
            prompt_tokens: Some(90_000),
            num_ctx: 131_072,
            prompt_chars: 360_000,
            completion_tokens: None,
            num_predict: None,
            output_limited: false,
        }];
        let dh = build_data_health(&[], 0, false, false, FeedGaps::default(), usage, vec![]);
        assert!(dh.context_pressure.is_empty());
        let peak = dh.peak_prompt.expect("peak recorded regardless of pressure");
        assert_eq!(peak.prompt_tokens, Some(90_000));
        assert!(!dh.attention, "{}", dh.summary);
        assert!(!dh.summary.contains("context pressure"), "{}", dh.summary);
    }

    /// The output-budget line: a `done_reason: "length"` observation is named in
    /// the summary with the stage and counts, and trips attention — the
    /// run-level surface of the typed per-call truncation error.
    #[test]
    fn data_health_names_an_output_limited_call() {
        let usage = vec![crate::local_model::PromptUsage {
            stage: "construction".into(),
            prompt_tokens: Some(60_000),
            num_ctx: 131_072,
            prompt_chars: 240_000,
            completion_tokens: Some(65_536),
            num_predict: Some(65_536),
            output_limited: true,
        }];
        let dh = build_data_health(&[], 0, false, false, FeedGaps::default(), usage, vec![]);
        assert!(dh.attention, "{}", dh.summary);
        let expected =
            "generation length-stopped on 1 local call (worst: construction generated 65536 of \
             65536 reserved — at the output reservation)";
        assert!(dh.summary.contains(expected), "{}", dh.summary);
    }

    /// A daemon-omitted `prompt_eval_count` must not cost the run its
    /// length-stop observation: the row records, the summary names the stop
    /// without attributing it, attention trips, and no context-fit line is
    /// faked from the count-less row (attempt-1 review sweep).
    #[test]
    fn data_health_keeps_a_length_stop_with_no_prompt_count() {
        let usage = vec![crate::local_model::PromptUsage {
            stage: "construction".into(),
            prompt_tokens: None,
            num_ctx: 131_072,
            prompt_chars: 240_000,
            completion_tokens: None,
            num_predict: Some(65_536),
            output_limited: true,
        }];
        let dh = build_data_health(&[], 0, false, false, FeedGaps::default(), usage, vec![]);
        assert!(dh.attention, "{}", dh.summary);
        let expected = "generation length-stopped on 1 local call (worst: construction \
                        generated unreported of 65536 reserved — counts incomplete; stop \
                        unattributed)";
        assert!(dh.summary.contains(expected), "{}", dh.summary);
        // The count-less row must not fake a context-fit read.
        assert!(dh.context_pressure.is_empty(), "{}", dh.summary);
        assert!(!dh.summary.contains("front-truncation"), "{}", dh.summary);
        assert!(dh.peak_prompt.is_none(), "{:?}", dh.peak_prompt);
    }

    /// The demonstrated truncation signature
    /// (`docs/verification/2026-07-28-m5-preflight.md` §Truncation behavior): the
    /// post-truncation count reads as a *comfortable* ~50% fill, so only the
    /// chars-vs-count implausibility check can see it — the fill trigger alone
    /// must not be the detector.
    #[test]
    fn data_health_flags_likely_truncation_despite_comfortable_fill() {
        let usage = vec![crate::local_model::PromptUsage {
            stage: "interpret NVDA".into(),
            prompt_tokens: Some(1_026),
            num_ctx: 2_048,
            prompt_chars: 18_400,
            completion_tokens: None,
            num_predict: None,
            output_limited: false,
        }];
        let dh = build_data_health(&[], 0, false, false, FeedGaps::default(), usage, vec![]);
        assert_eq!(dh.context_pressure.len(), 1);
        assert!(dh.attention, "{}", dh.summary);
        let expected = "likely front-truncation on 1 local call (worst: interpret NVDA reported \
                        1026 tokens for a 18400-char prompt, num_ctx 2048)";
        assert!(dh.summary.contains(expected), "{}", dh.summary);
        assert!(!dh.summary.contains("context pressure on"), "{}", dh.summary);
    }

    /// The offline rate fixture — decimal ratios, with a dated DGS10 history
    /// covering the fixture anchor window.
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

    /// A market whose prints load but whose anchor-window history request failed —
    /// the fail-soft leg of the rate-anchor rule.
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

    /// A market context whose rate fetch fails — the hard-fail rule's fixture.
    struct FailingMarket;
    impl MarketContextSource for FailingMarket {
        fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors> {
            anyhow::bail!("simulated FRED outage")
        }
    }

    /// A stub company-data source serving strong fixture financials offline —
    /// including the v2 surface (quarterly prints, consensus, dated closes) so the
    /// driver ladder and anchor window are exercised end to end.
    struct StubCompanyData;
    impl CompanyDataSource for StubCompanyData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            use crate::portfolio::engine::{ConsensusEstimate, DatedValue, QuarterlyIncomeRow};
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
                        net_income: None,
                        gross_profit: None,
                        cost_of_revenue: None,
                        diluted_shares: Some(1.5e10),
                        operating_income: None,
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
                    ..ConsensusEstimate::default()
                }),
                ttm_dividends_per_share: Some(1.0),
                ..CompanyFinancials::default()
            }
        }
        fn facts(&self, _symbol: &str) -> SecData {
            // The stub's FMP half already carries the financials, so SEC adds nothing
            // and — being a stub, not a failed fetch — records no gap. The endpoint
            // was "queried" (`Some`), so the audit labels the leg consulted-but-empty.
            SecData {
                facts: Some(CompanyFacts::default()),
                gaps: Vec::new(),
            }
        }
    }

    /// A company-data source that also serves a fund surface: a US equity ETF with a
    /// full sector-P/E snapshot + history, so the fund path runs offline end to end.
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

    /// A company-data source whose deep-history fetch degrades — the single FMP
    /// dated-EOD rung failing: empty closes, one gap note.
    struct DegradedDeepHistoryData;
    impl CompanyDataSource for DegradedDeepHistoryData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            let mut fin = StubCompanyData.financials(symbol);
            // The fixture's own dated closes come from the deep-history seam in the
            // live shape, so blank them here and let `deep_price_history` decide.
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

    /// A company-data source whose SEC fetch fails, to prove the degradation is
    /// recorded as a gap rather than silently swallowed.
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

    /// A gradeable equity position at a given quantity (cost basis derived so the
    /// account math stays consistent; the diff classifies by quantity).
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

    /// A stub resolving one symbol to a non-US (PNK) listing — and refusing to
    /// serve its statements or SEC facts, the guard-terminal skip's tripwire.
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
            (vec![], vec![])
        }
        fn profile_identity(&self, symbol: &str) -> crate::portfolio::listing::ProfileLookup {
            use crate::portfolio::listing::{ProfileIdentity, ProfileLookup};
            if symbol == "NTDOF" {
                ProfileLookup::Resolved(ProfileIdentity {
                    company_name: Some("Nintendo Co., Ltd.".into()),
                    exchange: Some("PNK".into()),
                    sector: Some("Communication Services".into()),
                    industry: None,
                })
            } else {
                ProfileLookup::Unverified("profile source not wired".into())
            }
        }
    }

    /// A holdings source that forbids the option-chain pull for one symbol — the
    /// guard-terminal skip's chain-side tripwire.
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
        // The audit's sources tell the truth about a guard-terminal holding: the
        // profile identity read drove the verdict, and the never-consulted
        // statement surface is not claimed.
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
        // The sibling stock still grades normally through the unverified default
        // (no guard input is never a terminal outcome).
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
    fn a_conflict_abstention_preserves_the_prior_vintage_and_ledger() {
        // Run 1 grades MSFT fully; run 2's profile read resolves the symbol to a
        // different issuer — the conflict abstains, retains the standing ledger,
        // and keeps the prior full pass's vintage (an abstention is not a pass).
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
                        company_name: Some("Zenith Mining Corp".into()),
                        exchange: Some("NYSE".into()),
                        sector: None,
                        industry: None,
                    })
                } else {
                    ProfileLookup::Unverified("profile source not wired".into())
                }
            }
        }
        let (_dir, paths) = paths();
        // The fixture's default "MSFT Inc." description is ticker-only-token —
        // the shape the guard deliberately reads unverifiable, never conflict —
        // so the conflict under test needs a real issuer name to collide.
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
                crate::portfolio::VerdictDisposition::InsufficientEvidence { reason }
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
            msft.thesis_ledger.is_some(),
            "the standing ledger rides through the conflict abstention"
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

        // The data-health aggregate rides the roll-up: a clean run reads
        // rate-anchored with no deep-history degradation and no attention flag.
        let dh = run.roll_up.data_health.as_ref().expect("data health on the roll-up");
        assert_eq!(dh.targets_total, 1);
        assert_eq!(dh.rate_anchored_count, 1);
        assert_eq!(dh.deep_history_failures, 0);
        assert!(!dh.attention, "{}", dh.summary);
        assert!(dh.summary.contains("1 of 1 targets rate-anchored"), "{}", dh.summary);

        // The run persisted and is retrievable as the latest run.
        let conn = storage::open(&paths.db_path).unwrap();
        let latest = store::latest_run(&conn).unwrap().unwrap();
        assert_eq!(latest.run_id, run.run_id);
        // A job_runs row recorded the successful outcome.
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
        // The single FMP rung failed: no deep history at all — the anchor window
        // starves to the current-multiple carry, the failure is counted, and the
        // run demands attention.
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
        let dh = run.roll_up.data_health.as_ref().unwrap();
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
        // The SEC outage rides into the audit's degraded inputs rather than vanishing
        // into empty facts indistinguishable from "SEC was unnecessary."
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
        // The canonical rate-anchor rule: the engine consumes the rates numerically
        // in every target and hurdle, so a failed retrieval fails the run before any
        // per-holding work (`docs/portfolio-analysis.md` §Failure posture).
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
        // No partial run persisted.
        let conn = storage::open(&paths.db_path).unwrap();
        assert!(store::latest_run(&conn).unwrap().is_none());
    }

    #[test]
    fn a_failed_dgs10_history_degrades_to_the_fallback_not_a_failed_run() {
        // The hard-fail rule covers the two run-level prints only: a failed
        // anchor-window history leaves every spread observation inadmissible, the
        // targets take their documented fallback, and the degradation is recorded —
        // never a failed run (`docs/portfolio-analysis.md` §Starting parameters).
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
        // The documented fallback is the raw-multiple percentiles over the real
        // driver history — never the current-multiple carry while quarters exist.
        assert!(
            !meta.current_multiple_carry,
            "a failed DGS10 join must not degrade to the current-multiple carry"
        );
        // The run-level degradation also aggregates into the data-health read — the
        // "degraded run that looks clean" gap the first live run exposed.
        let dh = run.roll_up.data_health.as_ref().expect("data health on the roll-up");
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

    /// The snapshot is fetched once and memoized across funds, so its failure must
    /// be memoized too. Recording the gap only where the fetch happened left every
    /// later fund abstaining as "no P/E-usable sector overlap" — blaming the fund's
    /// own weightings for a missing run-level surface.
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
                // The priced-fund grade contract rides through the whole job.
                assert!(g.low_confidence_grade);
                assert_eq!(g.sub_scores.quality, 50.0);
                let tm = g.price_targets.twelve_month.as_ref().unwrap();
                assert!(tm.methodology.contains("fund exposure composite"));
            }
            other => panic!("expected a priced fund verdict, got {other:?}"),
        }
    }

    /// [`FundCompanyData`] with a tripwired SEC leg — the fund path must never reach
    /// it (its statement lines feed nothing on the reduced path, and the trust
    /// entity behind an ETF routinely 404s the facts API into pure gap noise).
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
        // The tripwire not firing is the assertion; the audit records no SEC source
        // and no SEC gap for the fund.
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

    /// The sector-P/E snapshot is dated on the run's **pinned** ET session — the
    /// `today` minted from `created_at` — not on a fresh clock read at fetch time,
    /// so a run crossing ET midnight before its first fund cannot pull
    /// next-session sector data into a prior-session run. The recording stub
    /// captures the date the job passed; it must equal the persisted run's
    /// `created_at` ET session (the same date the fund context's `as_of` carries).
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

    /// The slot-ordering invariant, driven through the **real** lazy CIK path: a
    /// [`LazyCikResolver`] over a real [`SecEdgarSource`] pointed at a localhost
    /// mock, with the FMP half stubbed. Constructing the source fetches nothing;
    /// the ticker-map fetch fires only inside the run — after `run_started`, under
    /// the active per-holding step (its request row is attributed, not dropped) —
    /// and it fires **even though the context's cancel flag was left set** by an
    /// earlier cancelled run: `reset_cancel` runs after the slot claim and before
    /// any fetch, so the map loads instead of silently bailing to empty (which
    /// gapped every holding's EDGAR leg before this fix).
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
        // A cancel flag still set from an earlier cancelled run — the shape that
        // made the eager pre-slot load bail to empty.
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

        // Ordering on the emitted stream: RunStarted precedes the ticker-map
        // request row, and that row is owned by the active per-holding step.
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

    /// The slice's acceptance check: drive the **real** local daemon (the 122B
    /// reasoner + 35B fast model) over the fixture holding plus live FMP + keyless SEC,
    /// and validate that a graded verdict comes back, and the wall-clock runtime. This
    /// is the offline-from-cloud quality/runtime validation the slice exists to prove.
    ///
    /// Requires the local Ollama daemon up with the configured roster present, plus
    /// FMP_API_KEY for the per-company price/financials. Run once (it spends one FMP
    /// call against the free daily cap):
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
        // The live smoke resolves CIKs from the real map (fetched or cached in the
        // temp dir), the same lazy-inside-the-slot path the command wires.
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
                    v.symbol, g.grade.as_str(), g.action, g.conviction, g.financial_summary
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
        // Two accounts each holding AAPL must produce one netted position — one
        // verdict, one diff entry — never two positions or a silent collision
        // (`docs/schwab-integration.md` §What is pulled).
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
        // First run is a "new holding"; the second run's dossier sees the prior verdict.
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
        // Two runs persisted; the retention cap is well clear.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM portfolio_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);

        // The thesis ledger carries run to run: run 1 authored the debut ledger;
        // run 2's validated rewrite kept the unchanged cores' condition ids and the
        // frozen original thesis (`docs/portfolio-analysis.md` §The position thesis
        // ledger), and its audit records the ledger legs.
        let l1 = first.verdicts[0]
            .thesis_ledger
            .as_ref()
            .expect("run 1 authors the debut ledger");
        let l2 = second.verdicts[0]
            .thesis_ledger
            .as_ref()
            .expect("run 2 carries a ledger");
        assert_eq!(l1.original_thesis, l1.current_thesis, "frozen at debut");
        assert_eq!(l2.original_thesis, l1.original_thesis);
        let ids1: Vec<&str> = l1.conditions.iter().map(|c| c.condition_id.as_str()).collect();
        let ids2: Vec<&str> = l2.conditions.iter().map(|c| c.condition_id.as_str()).collect();
        assert_eq!(ids1, ids2, "unchanged cores carry their ids");
        assert!(second.audit[0].ledger_audit.is_some());
    }

    #[test]
    fn a_full_run_overlays_quick_check_state_then_consumes_and_clears_it() {
        use crate::portfolio::quick_check::{HoldingQuickState, QuickCheckState};
        use crate::portfolio::ConditionEvalState;

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
        let cond_id = first.verdicts[0]
            .thesis_ledger
            .as_ref()
            .and_then(|l| l.conditions.iter().find(|c| c.quant.is_some()))
            .map(|c| c.condition_id.clone())
            .expect("the debut ledger carries a quantitative condition");
        let symbol = first.verdicts[0].symbol.clone();

        // A between-run quick check advanced this condition to a confirmed streak
        // on a NEWER observation than the fixture history the full run re-serves
        // (the cross-feed lag: the sweep's FMP print leads the run's deep
        // history by days) with a genuinely breaching recorded value — the
        // run's older print is a stale non-event, so the overlaid state chains
        // whole and the crossing keys to the recorded observation.
        let conn = storage::open(&paths.db_path).unwrap();
        store::save_quick_check(
            &conn,
            &QuickCheckState {
                swept_run_id: first.run_id.clone(),
                last_checked_at: "2026-08-03T00:00:00Z".into(),
                rate_cache: None,
                holdings: vec![HoldingQuickState {
                    symbol: symbol.clone(),
                    families: vec![],
                    flag: None,
                    evidence_events: vec![],
                    condition_states: vec![(
                        cond_id.clone(),
                        ConditionEvalState {
                            last_observation_id: Some("2026-07-04".into()),
                            last_value: Some(-0.45),
                            last_evaluated_at: Some("2026-08-03".into()),
                            breach_streak: 5,
                            first_breach_at: Some("2026-08-02".into()),
                            confirmed_at: Some("2026-08-03".into()),
                            acknowledged_observation_id: None,
                            authored_statement_basis: None,
                        },
                    )],
                    last_hurdle_state: None,
                    notes: vec![],
                }],
            },
        )
        .unwrap();

        let second = run_once();
        // The overlaid confirmed streak reached the run's evaluation: the carried
        // condition emits a confirmed crossing, which the 6g seam consumes and
        // acknowledges — the ack transition's stamp proves the overlay chained
        // rather than resetting to the blob's older (empty) state.
        let carried = second.verdicts[0]
            .thesis_ledger
            .as_ref()
            .and_then(|l| l.conditions.iter().find(|c| c.condition_id == cond_id))
            .expect("the unchanged core carried its id");
        let st = carried.eval_state.as_ref().expect("evaluation state persisted");
        assert!(st.breach_streak >= 5, "the overlaid streak chained: {st:?}");
        assert_eq!(
            st.acknowledged_observation_id.as_deref(),
            Some("2026-07-04"),
            "the ack stamps the RECORDED (newer) observation, never the run's stale print"
        );
        // And the successful pass — every holding analyzed, none abstaining —
        // left nothing to retain, so the between-run store cleared.
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

        // A between-run quick check flagged both holdings.
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
            condition_states: vec![],
            last_hurdle_state: None,
            notes: vec![],
        };
        store::save_quick_check(
            &conn,
            &QuickCheckState {
                swept_run_id: first.run_id.clone(),
                last_checked_at: "2026-08-03T00:00:00Z".into(),
                rate_cache: None,
                holdings: vec![entry("AAA"), entry("BBB")],
            },
        )
        .unwrap();

        // The next full run: BBB's floor-bearing inputs are gone, so it exits
        // `insufficient-evidence` — an abstention, not a successful pass over it.
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
        // The abstention preserves its prior analysis vintage — the evidence-event
        // boundary must not advance past events no pass examined — while the
        // successful pass stamps the run's own `created_at`.
        let vintage_of = |sym: &str| {
            second
                .verdicts
                .iter()
                .find(|v| v.symbol == sym)
                .and_then(|v| v.analyzed_at.as_deref())
        };
        assert_eq!(vintage_of("BBB"), Some(first.created_at.as_str()));
        assert_eq!(vintage_of("AAA"), Some(second.created_at.as_str()));

        // AAA's successful pass consumed its carried state; BBB's abstention is
        // not a successful pass (`docs/portfolio-analysis.md` §Evidence floor),
        // so its flag survives, re-stamped to the new run so the next sweep
        // chains from it rather than superseding it.
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
        // The retained sweep predates the run: its rate cache follows the run's
        // fresher prints so a later FRED failure never falls back past them.
        assert_eq!(retained.rate_cache, second.rate_prints);
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

        // Run 1: hold AAPL (100 sh) and MSFT (50 sh). First run — every position is new
        // (no prior snapshot), and nothing has exited.
        let first = run(&FixtureHoldingsSource::with_holdings(holdings_of(vec![
            stock("AAPL", 100.0, 19_500.0),
            stock("MSFT", 50.0, 20_000.0),
        ])));
        for v in &first.verdicts {
            assert_eq!(v.position_change, PositionChange::New, "{} on run 1", v.symbol);
        }
        assert!(first.roll_up.exited.is_empty());

        // Run 2: AAPL increased to 140, MSFT sold out, NVDA newly opened.
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
        // The sold-out name earns no verdict but is surfaced in the roll-up.
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

    /// The tail sweep's retrieval stub for selective-run tests: quiet by default
    /// (every leg succeeds, nothing fires — the stub's 195 price matches the
    /// authoring-time marks, so spot's relationship to the stored bear–bull band
    /// is unchanged since the ledger was authored and the transition-only band
    /// flag stays silent), with per-symbol overrides exercising the sweep's
    /// badge legs. Its `rates` leg is deliberately unreachable: the in-run sweep reads
    /// the run's own fresh prints, never a second FRED call.
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
                    // The full-run fixtures' anchor bars, unchanged on re-fetch
                    // (no re-basis): a real dated-EOD window contains the stamped
                    // split-bridge anchor, and a stub without it would fail-close
                    // every price comparison rather than exercise the legs.
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
        fn news_since(
            &self,
            _symbol: &str,
            _from: &str,
        ) -> Result<Vec<crate::fmp::SymbolNewsItem>> {
            Ok(vec![])
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

    /// Two small equity positions whose weights stay under the stub ledger's 25%
    /// trim trigger at both the persisted marks and the sweep's quiet price.
    fn two_stocks() -> Holdings {
        holdings_of(vec![stock("AAPL", 20.0, 3_900.0), stock("MSFT", 20.0, 3_900.0)])
    }

    // ---- Checkpoint / resume (`docs/portfolio-analysis.md` §Failure posture) ----

    /// A stub analyst that fails hard on one symbol — the mid-book model failure
    /// — or on none ([`FailOn::recording`], the resumed process's analyst).
    /// Instrumented like `LocalAnalyst`: every `distill_research` records one
    /// prompt-usage row (the failing symbol's *before* it bails — the abandoned
    /// call a resume must not carry), and every `interpret` records one usage
    /// row at `interpret_tokens` plus one fired-retry event; both drain through
    /// the trait's `take_*`.
    struct FailOn {
        symbol: Option<&'static str>,
        interpret_tokens: u64,
        prompt_usage: std::sync::Mutex<Vec<crate::local_model::PromptUsage>>,
        retries: std::sync::Mutex<Vec<crate::local_model::RetryEvent>>,
    }

    impl FailOn {
        /// Fails on `symbol`; its interpret rows fill past the context-pressure
        /// threshold (120 k of 128 k), so the pre-crash process leaves a
        /// pressure row and the run's peak behind.
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
                    prompt_tokens: Some(prompt_tokens),
                    num_ctx: 131_072,
                    prompt_chars: prompt_tokens * 4,
                    completion_tokens: Some(1_000),
                    num_predict: None,
                    output_limited: false,
                });
        }
    }

    impl crate::portfolio::pipeline::HoldingAnalyst for FailOn {
        fn distill_research(
            &self,
            inputs: &crate::portfolio::distill::DistillInputs,
        ) -> Result<crate::portfolio::distill::DistilledResearch> {
            self.record_usage(format!("distill {}", inputs.symbol), 20_000);
            if self.symbol.is_some_and(|s| s == inputs.symbol) {
                anyhow::bail!("injected model failure on {}", inputs.symbol);
            }
            Ok(crate::portfolio::distill::offline_consolidate(inputs))
        }
        fn interpret(
            &self,
            input: &crate::portfolio::pipeline::InterpretationInput,
        ) -> Result<crate::portfolio::Interpretation> {
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
        ) -> Result<crate::portfolio::RoleRiskInterpretation> {
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

    /// A stub analyst that panics on one symbol — the compute-module panic the
    /// job seam must contain. It can flip the run's cancel flag first: a panic
    /// is never a user stop, so the run still records `Failed` (ruled 2026-08-28).
    struct PanicOn {
        symbol: &'static str,
        cancel_first: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    }

    impl crate::portfolio::pipeline::HoldingAnalyst for PanicOn {
        fn distill_research(
            &self,
            inputs: &crate::portfolio::distill::DistillInputs,
        ) -> Result<crate::portfolio::distill::DistilledResearch> {
            if inputs.symbol == self.symbol {
                if let Some(flag) = &self.cancel_first {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                panic!("injected panic on {}", self.symbol);
            }
            Ok(crate::portfolio::distill::offline_consolidate(inputs))
        }
        fn interpret(
            &self,
            input: &crate::portfolio::pipeline::InterpretationInput,
        ) -> Result<crate::portfolio::Interpretation> {
            crate::portfolio::pipeline::StubAnalyst.interpret(input)
        }
        fn interpret_role_risk(
            &self,
            input: &crate::portfolio::pipeline::RoleRiskInput,
        ) -> Result<crate::portfolio::RoleRiskInterpretation> {
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
        // Contained: a normal `Failed` outcome carrying the payload — never
        // `Cancelled`, though the flag was set before the panic.
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

        // The lifecycle completed: the job-history row and the terminal event.
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

        // No partial run; the completed holding's checkpoint stands and resumes.
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

    /// A holdings source that refuses to pull — the resume no-new-pull proof.
    struct NoPullSource;

    impl crate::schwab::HoldingsSource for NoPullSource {
        fn holdings(&self) -> Result<Holdings> {
            anyhow::bail!("a resume must reopen the pinned pull, never pull fresh")
        }
        // The per-holding chain fetches run live on a resumed holding's gather
        // — only the Step-2 pull itself is pinned.
        fn option_chain(&self, _symbol: &str) -> Result<Option<crate::schwab::OptionChain>> {
            Ok(None)
        }
    }

    #[test]
    fn a_mid_run_model_failure_checkpoints_and_resume_completes_without_a_pull() {
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
            &ctx(),
        )
        .unwrap();
        assert!(matches!(outcome, PortfolioJobOutcome::Failed(_)), "{outcome:?}");
        let conn = storage::open(&paths.db_path).unwrap();
        assert!(store::latest_run(&conn).unwrap().is_none(), "no partial run persists");

        // The completed holding checkpointed; the failing one did not.
        let cp = store::load_checkpoint(&conn)
            .unwrap()
            .expect("checkpoints survive the failure");
        assert_eq!(cp.holdings.len(), 1, "AAPL completed before the MSFT failure");
        assert_eq!(cp.holdings[0].verdict.symbol, "AAPL");
        assert!(cp.header.work_list.is_none(), "a whole-book run pins no selection");
        assert!(
            cp.accumulators.sector_by_symbol.contains_key("AAPL"),
            "the accumulators carry the completed holding's sector identity"
        );
        // The completed holding's context-fit rows and fired retry ride its
        // own row; the interrupted holding's abandoned distill call reaches no
        // row (ruled 2026-08-28: telemetry membership is row membership).
        let stages: Vec<&str> = cp.holdings[0]
            .prompt_usage
            .iter()
            .map(|u| u.stage.as_str())
            .collect();
        assert_eq!(stages, ["distill AAPL", "interpret AAPL"], "{stages:?}");
        assert_eq!(cp.holdings[0].model_retries.len(), 1, "{:?}", cp.holdings[0].model_retries);
        assert_eq!(cp.holdings[0].model_retries[0].stage, "interpret AAPL");

        // Offerable right now, under the same roster.
        let ids = vec!["stub-analyst".to_string(), "stub-analyst".to_string()];
        resume_eligibility(&conn, &cp, &ids, chrono::Utc::now())
            .expect("a fresh checkpoint under the same versions is resumable");

        // Resume: reopens the pinned run — a source that refuses to pull proves
        // no fresh pull happens, and the finished run carries the pinned
        // identity and as-of stamps.
        let pinned_run_id = cp.header.run_id.clone();
        let pinned_created = cp.header.created_at.clone();
        // The resumed process records too, at a smaller fill, so the merge and
        // its order across both processes are pinned — not just survival.
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
        // The finished run's data-health read spans both processes in order —
        // the read the big-run prompt-fit and fired-retry watches consume: the
        // retries list pre-crash AAPL then post-resume MSFT, the peak is AAPL's
        // pre-crash 120 k fill over MSFT's 60 k, and AAPL's pressure row survives.
        let dh = run
            .roll_up
            .data_health
            .as_ref()
            .expect("the data-health aggregate persists");
        let retry_stages: Vec<&str> = dh.model_retries.iter().map(|r| r.stage.as_str()).collect();
        assert_eq!(retry_stages, ["interpret AAPL", "interpret MSFT"], "{retry_stages:?}");
        let peak = dh.peak_prompt.as_ref().expect("a peak is recorded");
        assert_eq!((peak.stage.as_str(), peak.prompt_tokens), ("interpret AAPL", Some(120_000)));
        let pressure: Vec<&str> = dh.context_pressure.iter().map(|u| u.stage.as_str()).collect();
        assert_eq!(pressure, ["interpret AAPL"], "{pressure:?}");
        assert!(dh.attention, "pressure and a fired retry are attention triggers: {}", dh.summary);
        // The trail cleared with the successful persist.
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
            &ctx(),
        )
        .unwrap();
        let conn = storage::open(&paths.db_path).unwrap();
        let stale = store::load_checkpoint(&conn).unwrap().expect("trail exists");

        // A new run (what Run analysis always starts) discards the trail at
        // entry and clears its own on success.
        let run = full_run(&paths, two_stocks());
        assert!(store::load_checkpoint(&conn).unwrap().is_none());

        // The stale trail loaded before the new run cannot resurrect: the
        // baseline has moved.
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
            &ctx(),
        )
        .unwrap();
        let conn = storage::open(&paths.db_path).unwrap();
        let cp = store::load_checkpoint(&conn).unwrap().expect("trail exists");
        let ids = vec!["stub-analyst".to_string(), "stub-analyst".to_string()];

        // Past the resume window: the pinned pull is stale against a book that
        // may have moved.
        let later = chrono::Utc::now() + chrono::Duration::hours(RESUME_WINDOW_HOURS + 1);
        let err = resume_eligibility(&conn, &cp, &ids, later).unwrap_err();
        assert!(err.contains("resume window"), "{err}");

        // A changed roster refuses rather than mixing models mid-run.
        let err =
            resume_eligibility(&conn, &cp, &["other-model".to_string()], chrono::Utc::now())
                .unwrap_err();
        assert!(err.contains("roster"), "{err}");

        // A prompt/schema drift refuses rather than mixing contracts.
        let mut drifted = cp.clone();
        drifted.header.prompt_version = "portfolio-v0".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("prompt/schema"), "{err}");

        // An evidence-floor rule drift refuses too: the trail's completed
        // holdings were floored under another rule (Codex I1, round 1).
        let mut drifted = cp.clone();
        drifted.header.evidence_floor_version = "evidence-floor-v1".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("evidence-floor"), "{err}");
        // The v3 → v4 move (the dated-EOD usability rule, Codex I16 round 1):
        // a trail stamped v3 is refused too — its completed holdings admitted
        // closes the v4 parse drops, so a resume must never mix them.
        let mut drifted = cp.clone();
        drifted.header.evidence_floor_version = "evidence-floor-v3".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("evidence-floor"), "{err}");

        // A pre-profit parameter drift refuses too: the trail's completed
        // holdings paired guidance under another vintage rule (Codex I4).
        let mut drifted = cp.clone();
        drifted.header.pre_profit_parameter_version = "pre-profit-v2".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("pre-profit"), "{err}");

        // A trail persisted before the stamp existed deserializes as the
        // presence floor and is refused with the same reason — never dropped
        // as an unreadable header (Codex I1, round 2).
        let mut json = serde_json::to_value(&cp.header).unwrap();
        json.as_object_mut()
            .unwrap()
            .remove("evidence_floor_version")
            .expect("the current producer writes the field");
        let v1: store::CheckpointHeader = serde_json::from_value(json).unwrap();
        assert_eq!(v1.evidence_floor_version, "evidence-floor-v1");
        let legacy = store::Checkpoint {
            header: v1,
            ..cp.clone()
        };
        let err = resume_eligibility(&conn, &legacy, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("evidence-floor"), "{err}");

        // A checkpoint-format drift refuses too: the trail's rows were written
        // under another shape, and the gate refuses with its reason rather than
        // loud-skipping every row and offering a resume that restores nothing
        // (Codex I17 / I18, ruled 2026-08-29).
        let mut drifted = cp.clone();
        drifted.header.checkpoint_format_version = "checkpoint-v1".into();
        let err = resume_eligibility(&conn, &drifted, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("checkpoint format"), "{err}");

        // A trail persisted before the format stamp existed decodes as
        // `checkpoint-v1` and is refused with the same reason.
        let mut json = serde_json::to_value(&cp.header).unwrap();
        json.as_object_mut()
            .unwrap()
            .remove("checkpoint_format_version")
            .expect("the current producer writes the field");
        let pre_stamp: store::CheckpointHeader = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(pre_stamp.checkpoint_format_version, "checkpoint-v1");
        let legacy = store::Checkpoint {
            header: pre_stamp,
            ..cp.clone()
        };
        let err = resume_eligibility(&conn, &legacy, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("checkpoint format"), "{err}");
        // Through the real loader (Codex I18, round 1): the stripped header
        // written back to the trail loads as `checkpoint-v1` with its rows
        // unread — AAPL's row still decodes and is still not restored — and
        // the gate refuses it with the format reason.
        conn.execute(
            "UPDATE portfolio_checkpoints SET header_json = ?1 WHERE run_id = ?2",
            rusqlite::params![serde_json::to_string(&json).unwrap(), cp.header.run_id],
        )
        .unwrap();
        let loaded = store::load_checkpoint(&conn).unwrap().expect("the header still loads");
        assert_eq!(loaded.header.checkpoint_format_version, "checkpoint-v1");
        assert!(loaded.holdings.is_empty(), "rows under another format are not read");
        let err = resume_eligibility(&conn, &loaded, &ids, chrono::Utc::now()).unwrap_err();
        assert!(err.contains("checkpoint format"), "{err}");
    }

    /// A resume rebuilds the deep-history count from the rows it restored
    /// (Codex I17): a holding whose row dropped re-analyzes and counts once,
    /// where the retired cumulative counter — re-written whole beside the next
    /// holding's write — kept its first contribution and counted it again.
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
            &ctx(),
        )
        .unwrap();
        assert!(matches!(outcome, PortfolioJobOutcome::Failed(_)), "{outcome:?}");
        let conn = storage::open(&paths.db_path).unwrap();
        let cp = store::load_checkpoint(&conn).unwrap().expect("trail exists");
        assert_eq!(cp.holdings.len(), 2, "AAPL and GOOG completed before MSFT failed");
        assert!(
            cp.holdings.iter().all(|h| h.health.deep_history_failed),
            "each completed holding's row carries its own degraded fetch"
        );

        // AAPL's row stops reading: it loud-skips at load and re-analyzes.
        // GOOG's row — the write that carried the retired counter's AAPL
        // contribution — still stands.
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
        assert_eq!(run.verdicts.len(), 3, "restored GOOG + re-analyzed AAPL and MSFT");
        let dh = run.roll_up.data_health.as_ref().expect("data health persists");
        assert_eq!(
            dh.deep_history_failures, 3,
            "three holdings, each degraded once — never AAPL twice: {}",
            dh.summary
        );
    }

    /// A company-data source whose stocks resolve to the Technology sector
    /// (SPDR benchmark `XLK`) and whose deep-history fetch degrades for every
    /// symbol — the benchmark's included — so a carried stock's pre-flag read
    /// finds its benchmark unavailable.
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
                company_name: Some(format!("{symbol} Inc.")),
                exchange: Some("NASDAQ".into()),
                sector: Some("Technology".into()),
                industry: None,
            })
        }
    }

    /// A resume deduplicates the benchmark gap list by benchmark (Codex I17):
    /// the sector-benchmark memo is per process, so a benchmark failing in
    /// both halves of a resumed run reached the retired run-level list twice
    /// with no write failure needed; rebuilt from the rows it counts once.
    #[test]
    fn a_resume_counts_a_benchmark_failing_in_both_processes_once() {
        let (_dir, paths) = paths();
        // A prior run, so each stock carries a prior verdict and its pre-flag
        // read reaches the benchmark at all.
        let prior = full_run(&paths, two_stocks());
        assert_eq!(prior.verdicts.len(), 2);

        // The interrupted run: AAPL reads XLK off a fresh failed fetch, MSFT
        // fails before its own read.
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
            &ctx(),
        )
        .unwrap();
        assert!(matches!(outcome, PortfolioJobOutcome::Failed(_)), "{outcome:?}");
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

        // Resume: MSFT reads XLK afresh in the new process and finds it
        // unavailable again.
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
        let dh = run.roll_up.data_health.as_ref().expect("data health persists");
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

    /// [`full_run`] against a caller-supplied company source, for the tests that
    /// assert on which per-symbol calls the loop actually spent.
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

    /// Synthetic weekday closes through today (per-symbol offset), so a backdated
    /// episode's windows are all coverable in-run; no dividends.
    struct SyntheticOutcomePrices;

    impl crate::portfolio::outcome::OutcomePriceSource for SyntheticOutcomePrices {
        fn daily_closes(
            &self,
            symbol: &str,
            from: chrono::NaiveDate,
            to: chrono::NaiveDate,
        ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
            use chrono::Datelike;
            let offset = symbol.len() as f64;
            let mut out = Vec::new();
            let mut d = from;
            let mut i = 0f64;
            while d <= to {
                if d.weekday().number_from_monday() <= 5 {
                    out.push(crate::portfolio::engine::DatedValue {
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
            _from: chrono::NaiveDate,
            _to: chrono::NaiveDate,
        ) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
            Ok(vec![])
        }
    }

    /// A fixed-vector embedder for the matured-learning leg.
    struct FixedEmbedder;

    impl crate::embedding::Embedder for FixedEmbedder {
        fn embed(&self, _text: &str) -> Result<Vec<f32>> {
            Ok(vec![0.1, 0.2, 0.3, 0.4])
        }
    }

    #[test]
    fn outcome_episodes_open_then_extend_across_runs() {
        let (_dir, paths) = paths();
        // Run 1: both stocks debut an episode; nothing is due to mature.
        let first = full_run(&paths, two_stocks());
        let records = first.outcome.as_ref().expect("outcome records on the run");
        assert_eq!(records.opened.len(), 2);
        assert!(records
            .opened
            .iter()
            .all(|o| o.reasons == vec![crate::portfolio::outcome::OpenReason::Debut]));
        assert!(records.matured.is_empty(), "fresh anchors: nothing due");
        assert!(!records.reads.eligibility.eligible, "far below the 30-holding bar");
        {
            let conn = storage::open(&paths.db_path).unwrap();
            let episodes = store::load_episodes(&conn).unwrap().episodes;
            assert_eq!(episodes.len(), 2);
            assert!(episodes
                .iter()
                .all(|e| e.state == crate::portfolio::outcome::EpisodeState::Active
                    && e.vintage_fresh));
        }

        // Run 2, same book and deterministic stub verdicts: the recommendation
        // state is unchanged, so both episodes extend (no new anchor) and this
        // run's diff tags them.
        let second = full_run(&paths, two_stocks());
        let records = second.outcome.as_ref().unwrap();
        assert!(records.opened.is_empty(), "a re-affirmation never mints an episode");
        assert_eq!(records.extended.len(), 2);
        assert_eq!(records.alignment_tags.len(), 2);
        let conn = storage::open(&paths.db_path).unwrap();
        let episodes = store::load_episodes(&conn).unwrap().episodes;
        assert_eq!(episodes.len(), 2);
        assert!(episodes.iter().all(|e| e.observations.len() == 1
            && e.observations[0].kind
                == crate::portfolio::outcome::ObservationKind::Reaffirmed));
        assert!(episodes.iter().all(|e| e.alignment.is_some()));
    }

    #[test]
    fn a_backdated_episode_matures_in_run_and_embeds_a_learning() {
        let (_dir, paths) = paths();
        // Seed an old active episode for a symbol not in the book — an exited
        // name's labels must still mature (the pass is independent of the
        // holdings work-list).
        let anchor_at = (chrono::Utc::now() - chrono::Duration::days(430)).to_rfc3339();
        let anchor = chrono::NaiveDate::parse_from_str(&anchor_at[..10], "%Y-%m-%d").unwrap();
        {
            let conn = storage::open(&paths.db_path).unwrap();
            storage::init_schema(&conn).unwrap();
            let episode = crate::portfolio::outcome::DecisionEpisode {
                episode_id: "ep-gone".into(),
                symbol: "GONE".into(),
                anchor_run_id: "run-old".into(),
                anchor_at: anchor_at.clone(),
                intrinsic_vintage: anchor_at.clone(),
                vintage_fresh: true,
                action_source: Default::default(),
                position_change: crate::portfolio::PositionChange::New,
                sector: crate::portfolio::outcome::SectorIdentity::resolve(Some("Technology")),
                opened: vec![crate::portfolio::outcome::OpenReason::Debut],
                body: crate::portfolio::outcome::EpisodeBody::RoleRiskOnly(
                    crate::portfolio::outcome::RoleRiskEpisode {
                        action: crate::portfolio::Action::Hold,
                        degraded_inputs: vec![],
                    },
                ),
                observations: vec![],
                alignment: None,
                falsifier_events: vec![],
                labels: crate::portfolio::outcome::pending_labels(anchor),
                state: crate::portfolio::outcome::EpisodeState::Active,
                self_correction_count: 0,
            };
            store::save_episode(&conn, &episode).unwrap();
        }
        let prices = SyntheticOutcomePrices;
        let embedder = FixedEmbedder;
        let sources = crate::portfolio::outcome::OutcomeSources {
            price: &prices,
            embedder: Some(&embedder),
        };
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(two_stocks()),
            &StubCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            Some(&sources),
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
        let records = run.outcome.as_ref().unwrap();
        assert_eq!(
            records
                .matured
                .iter()
                .filter(|m| m.symbol == "GONE" && m.outcome == "scored")
                .count(),
            4,
            "all four backdated windows scored: {:?}",
            records.matured
        );
        let conn = storage::open(&paths.db_path).unwrap();
        let episodes = store::load_episodes(&conn).unwrap().episodes;
        let gone = episodes.iter().find(|e| e.symbol == "GONE").unwrap();
        assert_eq!(gone.state, crate::portfolio::outcome::EpisodeState::Matured);
        // The fetched series landed in the shared bar cache.
        assert!(!store::load_price_bars(&conn, "GONE").unwrap().is_empty());
        assert!(!store::load_price_bars(&conn, crate::portfolio::outcome::MARKET_BENCHMARK)
            .unwrap()
            .is_empty());
        // The matured reads embedded as one durable learning in the Portfolio
        // namespace.
        let learnings: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vector_memory
                 WHERE namespace = 'portfolio' AND kind = 'learning'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(learnings, 1);
    }

    fn verdict<'a>(run: &'a PortfolioRun, symbol: &str) -> &'a HoldingVerdict {
        run.verdicts
            .iter()
            .find(|v| v.symbol.eq_ignore_ascii_case(symbol))
            .unwrap_or_else(|| panic!("{symbol} in run"))
    }

    // ---- Step-6a semantic recall + per-holding summary embeddings ----

    struct FailingEmbedder;

    impl crate::embedding::Embedder for FailingEmbedder {
        fn embed(&self, _text: &str) -> Result<Vec<f32>> {
            anyhow::bail!("daemon unreachable")
        }
    }

    #[test]
    fn holding_summary_text_captures_the_read_and_skips_exits() {
        let created = "2026-08-21T12:00:00+00:00";
        let mut v = crate::portfolio::HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: crate::portfolio::AssetClass::Stock,
            position_change: Default::default(),
            disposition: crate::portfolio::VerdictDisposition::InsufficientEvidence {
                reason: "thin".into(),
            },
            thesis_ledger: None,
            analyzed_at: Some(created.into()),
            action_source: Default::default(),
            side_reversed: false,
        };
        assert!(holding_summary_text(&v).is_none(), "an abstention has nothing to recall");

        // A priced verdict summarizes thesis, read, and action.
        let run = {
            // Reuse the demo-run pipeline's stub output for a realistic verdict.
            let (tempdir, paths) = paths();
            let outcome = run_portfolio_job(
                &FixtureHoldingsSource::with_holdings(two_stocks()),
                &StubCompanyData,
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
            drop(tempdir);
            match outcome {
                PortfolioJobOutcome::Successful(run) => *run,
                other => panic!("expected success, got {other:?}"),
            }
        };
        let text = holding_summary_text(verdict(&run, "AAPL")).expect("priced summarizes");
        assert!(text.starts_with("AAPL: grade "), "{text}");
        assert!(text.contains("action "), "{text}");
        assert!(text.contains("Standing thesis:"), "{text}");

        v.disposition = crate::portfolio::VerdictDisposition::NotRated {
            reason: "cash".into(),
        };
        assert!(holding_summary_text(&v).is_none(), "not-rated has nothing to recall");
    }

    #[test]
    fn a_successful_run_writes_per_holding_summary_rows_that_recall_reads() {
        let (_tempdir, paths) = paths();
        let prices = SyntheticOutcomePrices;
        let embedder = FixedEmbedder;
        let sources = crate::portfolio::outcome::OutcomeSources {
            price: &prices,
            embedder: Some(&embedder),
        };
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(two_stocks()),
            &StubCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            Some(&sources),
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
        let conn = storage::open(&paths.db_path).unwrap();
        // One summary row per fresh analyzed verdict, keyed {run_id}:{SYMBOL}.
        let mut stmt = conn
            .prepare(
                "SELECT report_id FROM vector_memory
                 WHERE namespace = 'portfolio' AND kind = 'summary' ORDER BY report_id",
            )
            .unwrap();
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        let mut expected: Vec<String> = run
            .verdicts
            .iter()
            .filter(|v| holding_summary_text(v).is_some())
            .map(|v| format!("{}:{}", run.run_id, v.symbol.to_ascii_uppercase()))
            .collect();
        expected.sort();
        assert_eq!(ids, expected, "per-holding rows keyed {{run_id}}:{{SYMBOL}}");
        assert!(!ids.is_empty(), "the fixture book has analyzed holdings");

        // The Step-6a lane reads them back: hits, no gap.
        let recall = semantic_recall_for(&conn, Some(&embedder), "holding AAPL, sector Technology");
        assert!(recall.gap.is_none(), "{:?}", recall.gap);
        assert!(!recall.hits.is_empty());
        assert!(recall.hits[0].starts_with("[summary · "), "{}", recall.hits[0]);

        // And pruning to one run keeps these rows (theirs) while a foreign-run id
        // sweeps.
        crate::vector_memory::insert_memory(
            &conn,
            crate::vector_memory::MemoryKind::Summary,
            crate::vector_memory::MemoryNamespace::Portfolio,
            Some("dead-run-id:GONE"),
            "orphan",
            &[0.1, 0.2, 0.3, 0.4],
            "2026-08-01T00:00:00+00:00",
        )
        .unwrap();
        store::prune_runs(&conn, 1).unwrap();
        let survivors: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vector_memory
                 WHERE namespace = 'portfolio' AND kind = 'summary'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(survivors as usize, expected.len(), "orphan swept, own rows kept");
    }

    #[test]
    fn semantic_recall_is_silent_when_absent_and_gaps_on_failure() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        // No embedder configured: silent absence, matching the learning embed's
        // guard.
        let r = semantic_recall_for(&conn, None, "q");
        assert!(r.hits.is_empty() && r.gap.is_none());
        // Empty partition: silent absence (the first post-slice run, by design).
        let r = semantic_recall_for(&conn, Some(&FixedEmbedder), "q");
        assert!(r.hits.is_empty() && r.gap.is_none());
        // A learnings-only partition is still silent absence: the guard counts
        // summary rows, and the failing embedder proves no query embed is spent
        // on a search that cannot hit.
        crate::vector_memory::insert_memory(
            &conn,
            crate::vector_memory::MemoryKind::Learning,
            crate::vector_memory::MemoryNamespace::Portfolio,
            None,
            "a matured calibration learning",
            &[0.4, 0.3, 0.2, 0.1],
            "2026-08-01T00:00:00+00:00",
        )
        .unwrap();
        let r = semantic_recall_for(&conn, Some(&FailingEmbedder), "q");
        assert!(r.hits.is_empty() && r.gap.is_none());
        // A populated summary shelf with a failing embedder: the typed gap.
        crate::vector_memory::insert_memory(
            &conn,
            crate::vector_memory::MemoryKind::Summary,
            crate::vector_memory::MemoryNamespace::Portfolio,
            Some("run:AAPL"),
            "AAPL: grade B",
            &[0.1, 0.2, 0.3, 0.4],
            "2026-08-01T00:00:00+00:00",
        )
        .unwrap();
        let r = semantic_recall_for(&conn, Some(&FailingEmbedder), "q");
        assert!(r.hits.is_empty());
        assert!(
            r.gap.as_deref().unwrap().contains("query embedding failed"),
            "{:?}",
            r.gap
        );
    }

    /// Re-persist the latest run with one verdict doctored — the prior-run shapes
    /// (an old vintage, a carried action) the selective tests need.
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
        // The one-time pre-`v9` migration force-include was retired by the
        // 2026-08-16 badge ruling: a whole-book-era stamp no longer forces a fresh
        // pass on a selective run. An unselected pre-`v9` verdict carries like any
        // other (a full run re-grades the whole book under `v9`).
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
        // The selected holding got a fresh pass; the tail carried, stamped with
        // the pass that actually produced its verdict.
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
        // The carried disposition is the prior verdict's. Carried numerics compare
        // exactly: serde_json's `float_roundtrip` feature makes the store's JSON
        // round-trip bit-exact (store.rs pins it), so drift here is a real bug.
        match (&msft.disposition, &verdict(&first, "MSFT").disposition) {
            (
                crate::portfolio::VerdictDisposition::Priced(carried),
                crate::portfolio::VerdictDisposition::Priced(prior),
            ) => {
                assert_eq!(carried.grade, prior.grade);
                assert_eq!(carried.action, prior.action);
                assert_eq!(carried.conviction, prior.conviction);
                assert_eq!(carried.what_changed, prior.what_changed);
                assert_eq!(carried.price_targets, prior.price_targets);
                assert_eq!(carried.sub_scores, prior.sub_scores);
            }
            other => panic!("expected carried priced verdicts, got {other:?}"),
        }
        // The carried audit row rides along — the stored re-anchor basis must
        // survive the carry or the next sweep reads the holding `unknown`, and the
        // pre-profit overlay record (the observation history's home) rides with it.
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
        // The roll-up ran over the mixed-vintage verdicts.
        assert_eq!(second.roll_up.graded_count, 2);
        // The carried holding's sweep state is retained, re-stamped to the new
        // run; the fresh-passed holding's is cleared per holding.
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
        // MSFT's price crashes far outside its stored bear–bull band: the sweep
        // flags it. Since the 2026-08-16 badge ruling the flag no longer forces a
        // re-analysis — MSFT carries, and its attention flag rides the retained
        // sweep state the frontend renders as a card badge.
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
        // The flag is persisted on the retained sweep state → the badge overlay.
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
        // MSFT's price retrieval fails: a required family reads `unknown`. Since
        // the 2026-08-16 badge ruling this no longer force-includes — MSFT carries,
        // and the degraded-sweep note rides the badge overlay.
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
        // ago — an unexamined event. Since the 2026-08-16 badge ruling it no longer
        // force-includes; MSFT carries and the evidence-event badge rides the overlay.
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
        // A role-risk verdict can persist an add-family action (the action
        // call's choice is structurally open) — and the stale-strong-action
        // rule is branch-unscoped, so the carry must demote it exactly like a
        // priced add.
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        let old = days_ago(40);
        doctor_latest_run(&paths, "MSFT", |v| {
            v.analyzed_at = Some(old.clone());
            v.disposition = crate::portfolio::VerdictDisposition::RoleRiskOnly(Box::new(
                crate::portfolio::RoleRiskVerdict {
                    class_label: "bond fund".into(),
                    role_summary: "Core fixed-income sleeve.".into(),
                    exposure_tilt: Vec::new(),
                    expense_drag: None,
                    observable_risk: None,
                    structural_flag: false,
                    is_cef: false,
                    nav_premium: None,
                    evidence_gaps: Vec::new(),
                    action: crate::portfolio::Action::Add,
                    action_rationale: String::new(),
                    what_changed: "new holding".into(),
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
        // The user trimmed MSFT between runs — a same-side decrease, which
        // raises no badge. The carried action stands as-is (rung-only);
        // only the position-change tag reads today's diff.
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
        // Since the 2026-08-16 badge ruling an over-age exit carry stands as-is
        // (badged by the stale-vintage tag), no longer force-included or demoted —
        // only over-age add-family carries rule-demote.
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
        // A third holding appears, unselected. Since the 2026-08-16 badge ruling a
        // new holding is no longer force-included into a selective run — it has no
        // prior verdict to carry, so it is left not analyzed (no verdict emitted;
        // the frontend renders it from holdings-minus-verdicts).
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
        // The selected AAPL analyzed fresh, MSFT carried — the two prior holdings.
        assert_eq!(second.verdicts.len(), 2);
    }

    /// Wraps [`StubCompanyData`] recording every per-symbol retrieval call, so the
    /// eligibility gate's saving is measurable rather than asserted.
    #[derive(Default)]
    struct CountingCompanyData {
        financials: std::cell::RefCell<Vec<String>>,
        facts: std::cell::RefCell<Vec<String>>,
        deep_history: std::cell::RefCell<Vec<String>>,
    }

    impl CompanyDataSource for CountingCompanyData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            self.financials.borrow_mut().push(symbol.to_string());
            StubCompanyData.financials(symbol)
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

        // Output-neutral: both non-gradeable rows still reach the same NotRated
        // verdict they always did — the eligibility routing decides it before the
        // engine stage, reading none of the retrieval.
        for symbol in ["SWVXX", "AAPL  260116C00250000"] {
            assert!(
                matches!(
                    &verdict(&run, symbol).disposition,
                    crate::portfolio::VerdictDisposition::NotRated { .. }
                ),
                "{symbol} must still be not-rated"
            );
        }
        // ...and the loop spends nothing on them. Ungated, each cost the full FMP
        // statement surface, an EDGAR facts call and a deep-history leg to
        // reach a verdict fixed before the first request.
        assert_eq!(
            *company.financials.borrow(),
            vec!["AAPL".to_string()],
            "only the gradeable holding is retrieved"
        );
        assert_eq!(*company.facts.borrow(), vec!["AAPL".to_string()]);
        assert_eq!(*company.deep_history.borrow(), vec!["AAPL".to_string()]);

        // The audit says what it actually consulted, rather than naming a financials
        // pull the gate skipped.
        let audit = run
            .audit
            .iter()
            .find(|a| a.symbol == "SWVXX")
            .expect("the not-rated row still records an audit");
        // The EXACT set, not merely the presence of the right entry: an
        // any()-shaped assertion passed while the audit still claimed the house view,
        // which this holding's verdict never reads (it returns at the eligibility
        // gate, ahead of both interpretation prompts).
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
        // MSFT flipped net long → net short at equal magnitude since its verdict.
        // Since the 2026-08-16 badge ruling the reversal no longer force-includes;
        // MSFT carries its prior (now opposite-side) verdict, marked `side_reversed`
        // for the card badge — the stale, wrong-direction advice stays visible.
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
        // Regression (Codex 2026-08-16): a carried directional verdict is authored
        // long, so its `side_reversed` marker is read from the *current* side each
        // run — it stays set while the position keeps carrying net-short and clears
        // when the position returns to long (a fresh pass would also clear it).
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
        // Run 3: MSFT still short — the marker must PERSIST (the bug cleared it here,
        // since the run-2→run-3 diff sees no flip).
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
        // Run 4: MSFT flips back short → long, matching the original long verdict —
        // the marker clears (direction is coherent again; staleness stays the stale badge's job).
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
        // Regression (Codex 2026-08-16, round 2): the marker reads the carried
        // directional verdict's invariant long authoring side against the *current*
        // side, so a reversal that passes through an exactly-zero net (kept by
        // netting, and invisible to a per-run sign-flip read) is still caught.
        let (_dir, paths) = paths();
        let first = full_run(&paths, two_stocks()); // MSFT long +20, priced
        // Run 2: MSFT nets to exactly zero — flat, so no opposite side yet.
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
        // Run 3: MSFT nets short — the carried long verdict is now reversed, even
        // though no single run showed a long→short sign flip.
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
        // No prior run: a selective request degrades to the whole-book run
        // (everything is new — there is nothing to carry).
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
