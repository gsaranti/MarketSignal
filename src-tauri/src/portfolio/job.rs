//! The Portfolio Analysis job lifecycle (`docs/portfolio-analysis.md`,
//! `docs/local-models.md §Failure posture`). Parallel to [`crate::jobs::run_job`] but
//! for the local job: it claims the **same** single global run slot ([`RunGuard`]) so
//! the report and both local jobs are mutually exclusive, runs each holding through
//! the per-holding [`crate::portfolio::pipeline`], builds the roll-up, persists the
//! run (with N=10 retention), and records the lifecycle outcome to `job_runs`.
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
/// report's `market_signal` so the two histories stay separable.
const PORTFOLIO_JOB: &str = "portfolio_analysis";

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
    pub facts: CompanyFacts,
    /// Degraded-input notes — empty when SEC contributed cleanly (or genuinely had
    /// nothing to add for a ticker it could resolve).
    pub gaps: Vec<String>,
}

/// The per-holding company-financials source the job reads, behind a trait so the job
/// is offline-testable. The live impl ([`LiveCompanyData`]) composes the FMP
/// per-company pull with keyless SEC EDGAR facts, deep Stooq history, and the
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
    /// Deep dated daily closes (Stooq — the v2 anchor join's price side), plus any
    /// gap notes. Fail-soft: an empty history under-populates the anchor window,
    /// which takes its documented fallback.
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
    /// Today's per-sector aggregate P/E snapshot (both exchanges) — run-level,
    /// memoized by the caller across funds.
    fn sector_pe_snapshot(&self) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        Ok(vec![])
    }
    /// The trailing per-sector P/E history (both exchanges) for one sector —
    /// memoized by the caller across funds.
    fn sector_pe_history(&self, _sector: &str) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        Ok(vec![])
    }
}

/// The run-level market-context source (`docs/portfolio-workflow.md` §Step 5): the
/// rate anchors the engine consumes numerically in every target and hurdle. Behind a
/// trait so the job is offline-testable; the live impl wraps FRED.
pub trait MarketContextSource {
    /// The `DGS2` / `DGS10` prints plus the DGS10 anchor-window history, as decimal
    /// ratios. **Hard-fail**: a retrieval still failing after the shared bounded
    /// retries fails the run before any per-holding work — the suite's canonical
    /// rate-anchor rule (`docs/portfolio-analysis.md` §Failure posture).
    fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors>;
}

/// How many days of DGS10 history the anchor-window request covers: the ~12-quarter
/// window plus the four TTM quarters behind its oldest anchor, plus alignment slack.
const RATE_HISTORY_LOOKBACK_DAYS: i64 = 1_600;

/// The live market context: FRED rate anchors.
pub struct LiveMarketContext {
    pub fred: crate::fred::FredDataSource,
}

impl MarketContextSource for LiveMarketContext {
    fn rates(&self) -> Result<crate::portfolio::engine::RateAnchors> {
        let dgs2 = self.fred.latest_rate_decimal("DGS2")?;
        let dgs10 = self.fred.latest_rate_decimal("DGS10")?;
        let to = chrono::Utc::now().date_naive();
        let from = to - chrono::Duration::days(RATE_HISTORY_LOOKBACK_DAYS);
        let dgs10_history = self.fred.rate_history_decimal("DGS10", from, to)?;
        Ok(crate::portfolio::engine::RateAnchors {
            dgs2,
            dgs10,
            dgs10_history,
        })
    }
}

/// The exchanges whose sector P/Es blend into the fund composite
/// (`docs/portfolio-analysis.md` §Asset eligibility — the defined exchange blend).
const SECTOR_PE_EXCHANGES: [&str; 2] = ["NYSE", "NASDAQ"];

/// The live company-data source: FMP per-company + SEC EDGAR. SEC is supplementary and
/// fail-soft — an unresolved ticker or a fetch error degrades to empty facts, and the
/// FMP half plus the derived multiples still carry the holding — but each such
/// degradation is recorded as a gap so the audit stays honest.
pub struct LiveCompanyData {
    pub fmp: crate::fmp::FmpDataSource,
    pub sec: SecEdgarSource,
    /// The ticker → CIK resolver over SEC's full `company_tickers.json` map
    /// ([`crate::sec::load_cik_resolver`]) — an unresolved ticker degrades to a typed
    /// gap, never a fabricated mapping.
    pub cik: crate::sec::CikResolver,
    /// Keyless Stooq daily bars — the deep dated history the v2 anchor join reads.
    pub stooq: crate::stooq::StooqSource,
}

/// How many days of deep price history the anchor join needs: the ~12-quarter window
/// (3y) plus the TTM quarters behind its oldest anchor (1y) plus slack.
const DEEP_HISTORY_LOOKBACK_DAYS: i64 = 1_600;

impl CompanyDataSource for LiveCompanyData {
    fn financials(&self, symbol: &str) -> CompanyFinancials {
        self.fmp.fetch_company_financials(symbol)
    }

    fn fund_financials(&self, symbol: &str) -> CompanyFinancials {
        self.fmp.fetch_fund_financials(symbol)
    }

    fn deep_price_history(
        &self,
        symbol: &str,
    ) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
        let to = chrono::Utc::now().date_naive();
        let from = to - chrono::Duration::days(DEEP_HISTORY_LOOKBACK_DAYS);
        match self.stooq.daily_closes(symbol, from, to) {
            Ok(closes) => (closes, vec![]),
            Err(e) => (
                vec![],
                vec![format!(
                    "Stooq deep price history unavailable for {symbol}: {e} — the \
                     anchor window falls to its documented fallback"
                )],
            ),
        }
    }

    fn fund_data(&self, symbol: &str) -> crate::portfolio::fund::FundData {
        self.fmp.fetch_fund_data(symbol)
    }

    fn sector_pe_snapshot(&self) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        // The most recent weekday: the snapshot endpoint is date-keyed and a weekend
        // date returns nothing. A market holiday can still gap — recorded, calibrated
        // against live runs.
        let date = last_weekday(chrono::Utc::now().date_naive())
            .format("%Y-%m-%d")
            .to_string();
        let mut rows = Vec::new();
        let mut last_err = None;
        for exchange in SECTOR_PE_EXCHANGES {
            match self.fmp.fetch_sector_pe_snapshot(exchange, &date) {
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

    fn sector_pe_history(&self, sector: &str) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
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

    fn facts(&self, symbol: &str) -> SecData {
        match self.cik.resolve(symbol) {
            // A ticker with no EDGAR mapping: SEC could not be consulted.
            None => SecData {
                facts: CompanyFacts::default(),
                gaps: vec![format!("SEC: no CIK mapping for {symbol}")],
            },
            Some(cik) => match self.sec.fetch_company_facts(cik) {
                // A clean fetch that genuinely carried nothing is not a degradation.
                Ok(facts) => SecData {
                    facts,
                    gaps: Vec::new(),
                },
                // An outage / 404 / parse failure is a real degraded input.
                Err(e) => SecData {
                    facts: CompanyFacts::default(),
                    gaps: vec![format!("SEC company facts unavailable: {e}")],
                },
            },
        }
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

/// Run one Portfolio Analysis job end to end with the lifecycle contract. Returns
/// `Err` only on an infrastructure failure (the database); a failed analysis is a
/// normal `Ok(Failed)`. The model/persistence half is **fail-hard** (a model error
/// fails the run); the research half is fail-soft (stubbed this slice, so moot).
#[allow(clippy::too_many_arguments)]
pub fn run_portfolio_job(
    holdings_source: &dyn HoldingsSource,
    company_data: &dyn CompanyDataSource,
    market: &dyn MarketContextSource,
    analyst: &dyn HoldingAnalyst,
    profile: &InvestorProfile,
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

    match run_analysis(
        holdings_source,
        company_data,
        market,
        analyst,
        profile,
        paths,
        &conn,
        ctx,
    ) {
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
        // user-initiated stop apart from a genuine failure.
        Err(_) if ctx.is_cancelled() => {
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
            let msg = e.to_string();
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

/// The analysis half: pull holdings, load the house view, run each holding through the
/// pipeline, build the roll-up, and persist the run. Returns the persisted
/// [`PortfolioRun`]. A cancel checkpoint sits between holdings.
#[allow(clippy::too_many_arguments)]
fn run_analysis(
    holdings_source: &dyn HoldingsSource,
    company_data: &dyn CompanyDataSource,
    market: &dyn MarketContextSource,
    analyst: &dyn HoldingAnalyst,
    profile: &InvestorProfile,
    paths: &ReportPaths,
    conn: &Connection,
    ctx: &RunContext,
) -> Result<PortfolioRun> {
    ctx.step_started("holdings", "Pull holdings");
    // Snapshot assembly runs the holdings-normalization step: same-symbol rows across
    // granted accounts net into one book-level position per symbol, and every later
    // step consumes only the normalized rows (`docs/schwab-integration.md` §What is
    // pulled; `docs/portfolio-workflow.md` §Step 2).
    let holdings = holdings_source.holdings()?.normalized();
    ctx.step_finished("holdings", "ok", None);

    // Deterministic holdings-change diff against the prior run's persisted snapshot
    // (Step 4), computed in the app layer before any model stage — the
    // compute-don't-guess boundary. Fail-soft: an unreadable prior run reads as "no
    // prior snapshot", so every position tags `new`, exactly as a first run does.
    let prior_holdings = store::latest_run(conn).ok().flatten().map(|r| r.holdings);
    let holdings_diff = diff::diff_holdings(prior_holdings.as_ref(), &holdings);

    let house_view = dossier::load_house_view(conn, &paths.reports_dir);

    // The run-level rate anchors — **hard-fail before any per-holding work** (the
    // suite's canonical rate-anchor rule: the engine consumes the rates numerically
    // in every target and hurdle, so the run fails here rather than computing off a
    // stale or guessed print; `docs/portfolio-analysis.md` §Failure posture).
    ctx.step_started("rates", "Load rate anchors (FRED)");
    let rates = match market.rates() {
        Ok(r) => {
            ctx.step_finished("rates", "ok", None);
            r
        }
        Err(e) => {
            ctx.step_finished("rates", "failed", Some(e.to_string()));
            return Err(e.context("run-level rate-anchor load failed (DGS2/DGS10)"));
        }
    };

    let mut verdicts: Vec<HoldingVerdict> = Vec::with_capacity(holdings.positions.len());
    let mut audits: Vec<HoldingAudit> = Vec::with_capacity(holdings.positions.len());

    // The run-level sector-P/E surface, fetched on first need and memoized across
    // funds (`docs/portfolio-workflow.md` §Step 6a): the snapshot once (per
    // exchange, inside the source), the per-sector histories as each fund's
    // weightings introduce sectors.
    let mut sector_pe_cache: Option<Vec<crate::portfolio::fund::SectorPe>> = None;
    let mut sector_history_cache: std::collections::HashMap<
        String,
        Vec<crate::portfolio::fund::SectorPe>,
    > = std::collections::HashMap::new();

    for position in &holdings.positions {
        if ctx.is_cancelled() {
            anyhow::bail!("run cancelled");
        }
        let step_key = format!("holding-{}", position.symbol);
        ctx.step_started(step_key.clone(), format!("Analyze {}", position.symbol));

        // Gather the holding's evidence (fail-soft external data). The per-company FMP
        // and SEC calls poll cancellation before their requests; a SEC degradation is
        // folded into the financials' gap manifest so it reaches the audit and prompt
        // rather than vanishing into empty facts.
        let is_fund = matches!(
            position.asset_class,
            crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
        );
        let mut fmp_financials = if is_fund {
            company_data.fund_financials(&position.symbol)
        } else {
            company_data.financials(&position.symbol)
        };
        let sec_data = company_data.facts(&position.symbol);
        fmp_financials.gaps.extend(sec_data.gaps);
        // Deep dated history (Stooq) for the anchor join and drawdown reads.
        let (deep_closes, deep_gaps) = company_data.deep_price_history(&position.symbol);
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
                sector_pe_cache = Some(match company_data.sector_pe_snapshot() {
                    Ok(rows) => rows,
                    Err(e) => {
                        fund.gaps
                            .push(format!("sector-P/E snapshot unavailable: {e}"));
                        vec![]
                    }
                });
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
            Some(crate::portfolio::fund::FundContext {
                fund,
                sector_pe: sector_pe_cache.clone().unwrap_or_default(),
                sector_pe_history: sector_history_cache.clone(),
                as_of: chrono::Utc::now().date_naive(),
            })
        } else {
            None
        };
        // Fail-soft chain fetch: an auth/server fault or a malformed response degrades
        // this holding's options signal to a gap, but — unlike a silent drop — it is
        // recorded in the manifest so it reaches the audit and prompt rather than reading
        // as "no options listed" (`docs/schwab-integration.md §Failure posture`). Never a
        // whole-job failure; the error carries status/context only, never a token.
        let chain = match holdings_source.option_chain(&position.symbol) {
            Ok(chain) => chain,
            Err(e) => {
                fmp_financials
                    .gaps
                    .push(format!("Option chain unavailable for {}: {e}", position.symbol));
                None
            }
        };
        let prior = dossier::prior_verdict_for(conn, &position.symbol);
        let dossier: HoldingDossier = dossier::assemble(
            position.clone(),
            holdings_diff.delta_for(&position.symbol),
            fmp_financials,
            &sec_data.facts,
            chain.as_ref(),
            profile.clone(),
            house_view.clone(),
            fund_ctx,
            prior,
        );

        // Cancellation checkpoint between the (now-complete) data gather and the model
        // stages, so a cancel mid-gather is observed before any model call is spent.
        if ctx.is_cancelled() {
            anyhow::bail!("run cancelled");
        }

        // The model/grade half is fail-hard: an interpretation or persistence error
        // fails the whole run (`docs/local-models.md §Failure posture`).
        let (verdict, audit) =
            analyze_holding(analyst, &dossier, holdings.account_total, &rates)?;
        ctx.step_finished(step_key, "ok", None);
        verdicts.push(verdict);
        audits.push(audit);
    }

    let roll_up = build_roll_up(&holdings, &verdicts, &holdings_diff.exited);
    let run = PortfolioRun {
        run_id: uuid::Uuid::new_v4().to_string(),
        created_at: now_rfc3339(),
        holdings,
        verdicts,
        roll_up,
        audit: audits,
    };

    ctx.step_started("persist", "Persist run");
    store::record_run(conn, &run)?;
    ctx.step_finished("persist", "ok", None);

    Ok(run)
}

/// Build the deterministic portfolio roll-up (`docs/portfolio-analysis.md` §Portfolio
/// roll-up): verdict counts, the concentration read (largest position weight), the cash
/// stance, and the positions closed since the last run (the Step-4 diff's exited
/// names). The 122B synthesis pass is a later slice; this is the deterministic summary
/// for the single-equity slice.
fn build_roll_up(
    holdings: &Holdings,
    verdicts: &[HoldingVerdict],
    exited: &[ExitedPosition],
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
    let total = holdings.account_total;
    let top_position_weight = if total > 0.0 {
        holdings
            .positions
            .iter()
            .map(|p| p.market_value / total)
            .fold(0.0_f64, f64::max)
    } else {
        0.0
    };
    let cash_weight = if total > 0.0 { holdings.cash / total } else { 0.0 };

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
        overview: format!(
            "{graded} graded{role_note}, {not_rated} not rated, {insufficient} \
             insufficient-evidence; top position {:.0}% of the account, cash {:.0}%.{exited_note}",
            top_position_weight * 100.0,
            cash_weight * 100.0
        ),
    }
}

/// Current time as an RFC3339 UTC string — the canonical persisted form, like
/// [`crate::jobs`]; local-time conversion is a display concern at the UI seam.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// The most recent weekday on or before `date` (the date-keyed sector-P/E snapshot
/// returns nothing for a weekend date).
fn last_weekday(date: chrono::NaiveDate) -> chrono::NaiveDate {
    use chrono::Datelike;
    let mut d = date;
    while matches!(d.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
        d -= chrono::Duration::days(1);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::pipeline::StubAnalyst;
    use crate::portfolio::{AssetClass, PositionChange};
    use crate::schwab::{FixtureHoldingsSource, Position};

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
                        diluted_shares: Some(1.5e10),
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
                }),
                ttm_dividends_per_share: Some(1.0),
                ..CompanyFinancials::default()
            }
        }
        fn facts(&self, _symbol: &str) -> SecData {
            // The stub's FMP half already carries the financials, so SEC adds nothing
            // and — being a stub, not a failed fetch — records no gap.
            SecData::default()
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
                gaps: vec![],
            }
        }
        fn sector_pe_snapshot(&self) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
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

    /// A company-data source whose SEC fetch fails, to prove the degradation is
    /// recorded as a gap rather than silently swallowed.
    struct FailingSecCompanyData;
    impl CompanyDataSource for FailingSecCompanyData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            StubCompanyData.financials(symbol)
        }
        fn facts(&self, symbol: &str) -> SecData {
            SecData {
                facts: CompanyFacts::default(),
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
    fn failed_sec_fetch_is_recorded_as_a_degraded_input() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &FailingSecCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
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
            &paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        assert!(matches!(outcome, PortfolioJobOutcome::Skipped(_)));
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
        // temp dir), the same path the command wires.
        let (_cik_dir, cik_cache) = {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("sec_company_tickers.json");
            (dir, path)
        };
        let cik = crate::sec::load_cik_resolver(&cik_cache, &sec);
        let stooq = crate::stooq::StooqSource::new().expect("build Stooq source");
        let company = LiveCompanyData { fmp, sec, cik, stooq };

        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let start = std::time::Instant::now();
        let market = LiveMarketContext {
            fred: crate::fred::FredDataSource::from_env().expect("FRED_API_KEY set"),
        };
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &company,
            &market,
            &analyst,
            &InvestorProfile::default_fixture(),
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
                &paths,
                &guard,
                &ctx(),
            )
            .unwrap()
        };
        // First run is a "new holding"; the second run's dossier sees the prior verdict.
        let _first = run_once();
        let conn = storage::open(&paths.db_path).unwrap();
        assert!(dossier::prior_verdict_for(&conn, "AAPL").is_some());
        let _second = run_once();
        // Two runs persisted; retention (N=10) is well clear.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM portfolio_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
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
}
