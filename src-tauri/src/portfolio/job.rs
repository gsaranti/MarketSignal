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
    /// ratios. **Hard-fail on the prints only**: a print retrieval still failing
    /// after the shared bounded retries fails the run before any per-holding work —
    /// the suite's canonical rate-anchor rule (`docs/portfolio-analysis.md` §Failure
    /// posture). The anchor-window **history** is fail-soft: a failed request leaves
    /// the window empty (every spread observation inadmissible — the targets take
    /// their documented raw-percentile / carry fallback) and records the reason on
    /// the anchors' `history_gap`, never a new failure state (§Starting parameters).
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
        let dgs2 = self.fred.latest_rate_dated("DGS2")?;
        let dgs10 = self.fred.latest_rate_dated("DGS10")?;
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
            Err(e) => {
                // Second rung: FMP's dated EOD serves the anchor window's price leg
                // so one throttled keyless source can't flatten the whole target
                // surface (`docs/data-sources.md §Stooq` — the 2026-07-31 F2
                // finding). Both notes ride the gap manifest so the audit shows the
                // substitution, and a cancel mid-run spends nothing (the FMP suite
                // seam returns a gap without a request when cancelled).
                let stooq_gap = format!("Stooq deep price history unavailable for {symbol}: {e}");
                match self.fmp.fetch_dated_eod(symbol, DEEP_HISTORY_LOOKBACK_DAYS) {
                    Ok(closes) if !closes.is_empty() => (
                        closes,
                        vec![format!(
                            "{stooq_gap} — FMP dated EOD served the anchor window's \
                             price leg in its place"
                        )],
                    ),
                    Ok(_) => (
                        vec![],
                        vec![format!(
                            "{stooq_gap} — the FMP dated-EOD fallback was empty; the \
                             anchor window falls to its documented fallback"
                        )],
                    ),
                    Err(fmp_e) => (
                        vec![],
                        vec![format!(
                            "{stooq_gap} — the FMP dated-EOD fallback also failed \
                             ({fmp_e}); the anchor window falls to its documented \
                             fallback"
                        )],
                    ),
                }
            }
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
/// parameters — drafted). Beyond it a carried exit-family action force-includes
/// and a carried add-family action rule-demotes to *hold*. Mirrored by the
/// card-facing stale badge (`src/components/PortfolioView.vue` `OVER_AGE_DAYS`)
/// — recalibrating one means recalibrating both.
const OVER_AGE_DAYS: i64 = 28;

/// Whether a vintage timestamp is over-age against `today`. An unparseable
/// vintage reads over-age — the conservative resolution, since the stale-carry
/// rules exist to keep an unverifiable strong action from standing.
fn over_age(vintage: &str, today: chrono::NaiveDate) -> bool {
    match vintage
        .get(..10)
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    {
        Some(d) => (today - d).num_days() > OVER_AGE_DAYS,
        None => true,
    }
}

/// The action a carried verdict would stand on — `None` where the disposition
/// carries no action (not-rated / insufficient-evidence).
fn carried_action(verdict: &HoldingVerdict) -> Option<crate::portfolio::Action> {
    match &verdict.disposition {
        crate::portfolio::VerdictDisposition::Priced(g) => Some(g.action),
        crate::portfolio::VerdictDisposition::RoleRiskOnly(r) => Some(r.action),
        _ => None,
    }
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
    selective: Option<SelectiveRun<'_>>,
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
        selective,
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
    selective: Option<SelectiveRun<'_>>,
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
    let prior_run = store::latest_run(conn).ok().flatten();
    let prior_run_id = prior_run.as_ref().map(|r| r.run_id.clone());
    let prior_created_at = prior_run.as_ref().map(|r| r.created_at.clone());
    let holdings_diff = diff::diff_holdings(prior_run.as_ref().map(|r| &r.holdings), &holdings);

    // The quick-check store's fresher condition evaluation states — overlaid onto
    // each prior ledger before this run evaluates it, so the between-run sweeps'
    // streaks and acknowledgments chain instead of silently resetting
    // (`docs/portfolio-analysis.md §The quick check`). Only a state swept against
    // the same prior run applies.
    let quick_state = store::latest_quick_check(conn)
        .ok()
        .flatten()
        .filter(|s| Some(&s.swept_run_id) == prior_run_id.as_ref());

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

    // ---- Selective work-list (`docs/portfolio-analysis.md` §Triggering) ------
    // The initial work-list is the selection plus every holding new since the
    // last run; the safety sweep and the deterministic legs below then expand it
    // with every force-inclusion. `None` = the whole-book run — including a
    // selective request with an empty selection or no prior run to carry from.
    let today = chrono::Utc::now().date_naive();
    let mut swept_tail: std::collections::HashMap<
        String,
        crate::portfolio::quick_check::HoldingQuickState,
    > = std::collections::HashMap::new();
    let work_list: Option<std::collections::HashSet<String>> = match (&selective, &prior_run) {
        (Some(sel), Some(prior)) if !sel.selected.is_empty() => {
            let book: std::collections::HashSet<String> = holdings
                .positions
                .iter()
                .map(|p| p.symbol.to_ascii_uppercase())
                .collect();
            let mut work: std::collections::HashSet<String> = sel
                .selected
                .iter()
                .map(|s| s.to_ascii_uppercase())
                .filter(|s| book.contains(s))
                .collect();
            // The deterministic force-include legs that need no retrieval: a
            // holding new since the last run (no verdict to carry), a position
            // whose net side reversed (thesis-changing by construction — no
            // carried verdict survives it), and an over-age carried exit-family
            // action (re-analysis is the only honest resolution; the add family
            // is rule-demoted at carry instead, and over-age holds stand).
            for p in &holdings.positions {
                let key = p.symbol.to_ascii_uppercase();
                if work.contains(&key) {
                    continue;
                }
                let delta = holdings_diff.delta_for(&p.symbol);
                let prior_verdict = prior
                    .verdicts
                    .iter()
                    .find(|v| v.symbol.eq_ignore_ascii_case(&p.symbol));
                let force = delta.change == crate::portfolio::PositionChange::New
                    || delta.side_reversed(p.quantity)
                    // A current position with no prior verdict has nothing to
                    // carry, whatever the diff says.
                    || prior_verdict.is_none()
                    || prior_verdict.is_some_and(|v| {
                        over_age(crate::portfolio::effective_vintage(v, &prior.created_at), today)
                            && carried_action(v).is_some_and(|a| a.is_exit_family())
                    });
                if force {
                    work.insert(key);
                }
            }
            // The first mixed-vintage safety rule: the engine-only quick check
            // over the unselected tail. A flag, an `unknown` family (the sweep
            // could not vouch), or an unexamined evidence event force-includes.
            let tail: std::collections::HashSet<String> = holdings
                .positions
                .iter()
                .map(|p| p.symbol.to_ascii_uppercase())
                .filter(|k| !work.contains(k))
                .collect();
            let states = crate::portfolio::quick_check::sweep_tail(
                crate::portfolio::quick_check::TailSweep {
                    data: sel.quick_data,
                    prior_run: prior,
                    current_positions: &holdings.positions,
                    cash: holdings.cash,
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
                let key = h.symbol.to_ascii_uppercase();
                let force = h.flag.is_some()
                    || h.families
                        .iter()
                        .any(|f| f.state == crate::portfolio::quick_check::SweepState::Unknown)
                    || !h.evidence_events.is_empty();
                if force {
                    work.insert(key.clone());
                }
                swept_tail.insert(key, h);
            }
            Some(work)
        }
        _ => None,
    };

    let mut verdicts: Vec<HoldingVerdict> = Vec::with_capacity(holdings.positions.len());
    let mut audits: Vec<HoldingAudit> = Vec::with_capacity(holdings.positions.len());

    // Deep-history health counters for the run-level data-health roll-up: a
    // non-empty gap list from `deep_price_history` means the primary (Stooq) source
    // degraded; closes arriving anyway mean the FMP fallback served.
    let mut deep_history_failures = 0usize;
    let mut deep_history_fallbacks = 0usize;

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
        // A selective run analyzes only the work-list; everything else carries
        // its prior verdict forward vintage-stamped (appended after the loop).
        if work_list
            .as_ref()
            .is_some_and(|w| !w.contains(&position.symbol.to_ascii_uppercase()))
        {
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
        let mut fmp_financials = if is_fund {
            company_data.fund_financials(&position.symbol)
        } else {
            company_data.financials(&position.symbol)
        };
        // A fund never hits SEC company facts: its statement lines feed nothing on
        // the reduced path (quality is imputed, valuation composite-priced), and the
        // trust entity behind an ETF routinely 404s the facts API — pure gap noise
        // on the audit (the 2026-07-31 run's QQQ finding, F5).
        let sec_data = if is_fund {
            SecData::default()
        } else {
            company_data.facts(&position.symbol)
        };
        fmp_financials.gaps.extend(sec_data.gaps);
        // Deep dated history (Stooq, FMP dated-EOD fallback) for the anchor join and
        // drawdown reads.
        let (deep_closes, deep_gaps) = company_data.deep_price_history(&position.symbol);
        if !deep_gaps.is_empty() {
            deep_history_failures += 1;
            if !deep_closes.is_empty() {
                deep_history_fallbacks += 1;
            }
        }
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
        let mut prior = dossier::prior_verdict_for(conn, &position.symbol);
        // The prior verdict's effective analysis vintage — preserved on an
        // insufficient-evidence exit below, since an abstention is not a full pass
        // and the evidence-event boundary must not silently advance past events no
        // pass examined (`docs/portfolio-analysis.md` §Evidence floor).
        let prior_vintage = prior.as_ref().map(|(v, _)| {
            crate::portfolio::effective_vintage(v, prior_created_at.as_deref().unwrap_or(""))
                .to_string()
        });
        if let Some((verdict, _)) = prior.as_mut() {
            // The freshest condition evaluation states win: a force-included
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
        // The run date keys the ledger evaluation's observation identities and
        // timestamps (deterministic under test — injected, never re-derived inside
        // the engine). It is the UTC date, consistent with stored `created_at` —
        // engine-internal state, never rendered; a card-facing date must convert to
        // local per the project's date convention.
        let run_date = now_rfc3339().chars().take(10).collect::<String>();
        let (mut verdict, audit) =
            analyze_holding(analyst, &dossier, holdings.account_total, &rates, &run_date)?;
        if matches!(
            verdict.disposition,
            crate::portfolio::VerdictDisposition::InsufficientEvidence { .. }
        ) {
            verdict.analyzed_at = prior_vintage;
        }
        ctx.step_finished(step_key, "ok", None);
        verdicts.push(verdict);
        audits.push(audit);
    }

    let created_at = now_rfc3339();
    // Stamp each fresh pass's analysis vintage with the run's own `created_at`
    // (`docs/portfolio-analysis.md` §Triggering — carried verdicts ride
    // vintage-stamped, so a fresh one must be distinguishable). An abstention
    // already carries its preserved prior vintage from the loop.
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
    // must survive the carry or the next sweep reads the holding `unknown`. The
    // over-age rule resolves per action family: a carried add-family action
    // rule-demotes to *hold*, stamped `action_source: rule-demoted` (exit-family
    // carries were force-included above; over-age holds stand).
    let mut carried_symbols: std::collections::HashSet<String> = std::collections::HashSet::new();
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
                continue; // unreachable: no prior verdict force-includes above
            };
            let mut carried = prior_verdict.clone();
            let vintage =
                crate::portfolio::effective_vintage(prior_verdict, &prior.created_at).to_string();
            carried.analyzed_at = Some(vintage.clone());
            carried.position_change = holdings_diff.delta_for(&position.symbol).change;
            if let Some(h) = swept_tail.get(&key) {
                crate::portfolio::quick_check::overlay_condition_states(&mut carried, h);
            }
            // The intrinsic *action* carries; its **sizing is engine context,
            // recomputed at current weights** like the rest of the roll-up
            // (`docs/portfolio-analysis.md` §Triggering — the roll-up re-runs
            // over the mixed-vintage verdicts at current weights), so a carried
            // card never shows today's weight beside the prior book's target
            // band or share/dollar adjustment. The over-age add-family demotion
            // lands first, so the demoted *hold* is what gets sized.
            let stale = over_age(&vintage, today);
            match &mut carried.disposition {
                crate::portfolio::VerdictDisposition::Priced(g) => {
                    if stale && g.action.is_add_family() {
                        g.action = crate::portfolio::Action::Hold;
                        carried.action_source = crate::portfolio::ActionSource::RuleDemoted;
                    }
                    g.action_sizing = crate::portfolio::engine::size_action(
                        g.action,
                        position,
                        profile,
                        holdings.account_total,
                    );
                }
                crate::portfolio::VerdictDisposition::RoleRiskOnly(r) => {
                    r.action_sizing = crate::portfolio::engine::size_action(
                        r.action,
                        position,
                        profile,
                        holdings.account_total,
                    );
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

    let roll_up = build_roll_up(
        &holdings,
        &verdicts,
        &holdings_diff.exited,
        &audits,
        deep_history_failures,
        deep_history_fallbacks,
        rates.history_gap.is_some(),
    );
    let run = PortfolioRun {
        run_id: uuid::Uuid::new_v4().to_string(),
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
    };

    ctx.step_started("persist", "Persist run");
    store::record_run(conn, &run)?;
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
    let store_state = store::latest_quick_check(conn)?;
    let mut retained_holdings: Vec<crate::portfolio::quick_check::HoldingQuickState> = Vec::new();
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
        } else if let Some(h) = store_state
            .as_ref()
            .and_then(|s| s.holdings.iter().find(|h| h.symbol.eq_ignore_ascii_case(&v.symbol)))
        {
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
    ctx.step_finished("persist", "ok", None);

    Ok(run)
}

/// Build the deterministic portfolio roll-up (`docs/portfolio-analysis.md` §Portfolio
/// roll-up): verdict counts, the concentration read (largest position weight), the cash
/// stance, the positions closed since the last run (the Step-4 diff's exited
/// names), and the run-level **data-health** aggregate over the per-holding audits —
/// so a degraded-but-successful run (the 2026-07-31 "43 of 44 anchor windows empty"
/// pattern) is visible at a glance rather than only inside 47 audit records. The 122B
/// synthesis pass is a later slice; this is the deterministic summary.
#[allow(clippy::too_many_arguments)]
fn build_roll_up(
    holdings: &Holdings,
    verdicts: &[HoldingVerdict],
    exited: &[ExitedPosition],
    audits: &[HoldingAudit],
    deep_history_failures: usize,
    deep_history_fallbacks: usize,
    dgs10_history_gap: bool,
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
        data_health: Some(build_data_health(
            audits,
            deep_history_failures,
            deep_history_fallbacks,
            dgs10_history_gap,
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
    deep_history_fallbacks: usize,
    dgs10_history_gap: bool,
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
            "deep price history failed on {deep_history_failures} holdings \
             ({deep_history_fallbacks} recovered via the FMP fallback)"
        ));
    }
    if floored > 0 {
        parts.push(format!("dispersion floor widened {floored} target bands"));
    }
    let attention = deep_history_failures > deep_history_fallbacks
        || carry > 0
        || dgs10_history_gap;
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
        deep_history_fallbacks,
        dgs10_history_gap,
        attention,
        summary,
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

    /// A company-data source whose deep-history fetch degrades: `recovered` mimics
    /// the FMP dated-EOD fallback serving closes beside the Stooq gap note;
    /// `!recovered` mimics both rungs failing (empty closes, gap note only).
    struct DegradedDeepHistoryData {
        recovered: bool,
    }
    impl CompanyDataSource for DegradedDeepHistoryData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            let mut fin = StubCompanyData.financials(symbol);
            // The fixture's own dated closes come from the deep-history seam in the
            // live shape, so blank them here and let `deep_price_history` decide.
            fin.daily_closes = vec![];
            fin
        }
        fn facts(&self, _symbol: &str) -> SecData {
            SecData::default()
        }
        fn deep_price_history(
            &self,
            symbol: &str,
        ) -> (Vec<crate::portfolio::engine::DatedValue>, Vec<String>) {
            let gap = format!(
                "Stooq deep price history unavailable for {symbol}: throttled — \
                 FMP dated EOD served the anchor window's price leg in its place"
            );
            if self.recovered {
                (StubCompanyData.financials(symbol).daily_closes, vec![gap])
            } else {
                (vec![], vec![gap])
            }
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
    fn deep_history_health_distinguishes_recovered_from_unrecovered_failures() {
        // Recovered: the fallback served dated closes, so the target still anchors —
        // counted as a degradation but not an attention state.
        let (_dir, recovered_paths) = paths();
        let guard = RunGuard::default();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &DegradedDeepHistoryData { recovered: true },
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            &recovered_paths,
            &guard,
            &ctx(),
        )
        .unwrap();
        let run = match outcome {
            PortfolioJobOutcome::Successful(run) => *run,
            other => panic!("expected success, got {other:?}"),
        };
        let dh = run.roll_up.data_health.as_ref().unwrap();
        assert_eq!(dh.deep_history_failures, 1);
        assert_eq!(dh.deep_history_fallbacks, 1);
        assert_eq!(dh.rate_anchored_count, 1, "{}", dh.summary);
        assert!(!dh.attention, "a recovered failure is not an attention state: {}", dh.summary);
        assert!(dh.summary.contains("1 recovered via the FMP fallback"), "{}", dh.summary);

        // Unrecovered: no deep history at all — the anchor window starves to the
        // current-multiple carry and the run demands attention.
        let (_dir2, unrecovered_paths) = paths();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &DegradedDeepHistoryData { recovered: false },
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
            None,
            &unrecovered_paths,
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
        assert_eq!(dh.deep_history_fallbacks, 0);
        assert_eq!(dh.current_multiple_carry_count, 1, "{}", dh.summary);
        assert!(dh.attention, "{}", dh.summary);
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
        fn sector_pe_snapshot(&self) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
            FundCompanyData.sector_pe_snapshot()
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
        assert!(dossier::prior_verdict_for(&conn, "AAPL").is_some());
        let second = match run_once() {
            PortfolioJobOutcome::Successful(r) => *r,
            other => panic!("expected success, got {other:?}"),
        };
        // Two runs persisted; retention (N=10) is well clear.
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
        // against the same observation the fixture data will serve again.
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
                            last_observation_id: Some("2026-06-30".into()),
                            last_value: Some(1.0),
                            last_evaluated_at: Some("2026-08-03".into()),
                            breach_streak: 5,
                            first_breach_at: Some("2026-08-02".into()),
                            confirmed_at: Some("2026-08-03".into()),
                            acknowledged_observation_id: None,
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
            Some("2026-06-30"),
            "the full pass stamped the acknowledging observation"
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
            fn facts(&self, _symbol: &str) -> SecData {
                SecData::default()
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
    /// (every leg succeeds, nothing fires — the stub's 170 price sits inside the
    /// fixture verdict's stored bear–bull band, whose engine targets lie below
    /// the 195 marks), with per-symbol overrides exercising the force-include
    /// legs. Its `rates` leg is deliberately unreachable: the in-run sweep reads
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
                _ => 170.0,
            };
            let today = chrono::Utc::now().date_naive();
            Ok((
                price,
                vec![
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

    fn full_run(paths: &ReportPaths, holdings: Holdings) -> PortfolioRun {
        match run_portfolio_job(
            &FixtureHoldingsSource::with_holdings(holdings),
            &StubCompanyData,
            &StubMarket,
            &StubAnalyst,
            &InvestorProfile::default_fixture(),
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

    fn verdict<'a>(run: &'a PortfolioRun, symbol: &str) -> &'a HoldingVerdict {
        run.verdicts
            .iter()
            .find(|v| v.symbol.eq_ignore_ascii_case(symbol))
            .unwrap_or_else(|| panic!("{symbol} in run"))
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

    fn days_ago(n: i64) -> String {
        (chrono::Utc::now() - chrono::Duration::days(n)).to_rfc3339()
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
        // The carried disposition is the prior verdict's (compared on its stable
        // fields — the store's JSON round-trip can drift floats by an ulp).
        match (&msft.disposition, &verdict(&first, "MSFT").disposition) {
            (
                crate::portfolio::VerdictDisposition::Priced(carried),
                crate::portfolio::VerdictDisposition::Priced(prior),
            ) => {
                assert_eq!(carried.grade, prior.grade);
                assert_eq!(carried.action, prior.action);
                assert_eq!(carried.conviction, prior.conviction);
                assert_eq!(carried.what_changed, prior.what_changed);
            }
            other => panic!("expected carried priced verdicts, got {other:?}"),
        }
        // The carried audit row rides along — the stored re-anchor basis must
        // survive the carry or the next sweep reads the holding `unknown`.
        let msft_audit = second
            .audit
            .iter()
            .find(|a| a.symbol == "MSFT")
            .expect("carried audit row");
        assert!(msft_audit.quick_basis.is_some());
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
    fn a_tail_sweep_flag_forces_the_holding_into_the_work_list() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        // MSFT's price crashes far outside its stored bear–bull band: the sweep
        // flags it, so the selective run must re-analyze it despite no selection.
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
            Some(second.created_at.as_str()),
            "a flagged holding is force-included, never carried"
        );
        // Every holding got a full pass, so nothing is retained.
        let conn = storage::open(&paths.db_path).unwrap();
        assert!(store::latest_quick_check(&conn).unwrap().is_none());
    }

    #[test]
    fn an_unknown_sweep_family_forces_the_holding_into_the_work_list() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        // MSFT's price retrieval fails: the sweep cannot vouch for the carried
        // verdict, and a verdict the sweep couldn't check never stands on its
        // silence — the degraded-sweep force-include.
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
            Some(second.created_at.as_str())
        );
    }

    #[test]
    fn an_unexamined_evidence_event_since_the_holdings_own_vintage_forces_inclusion() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        // MSFT's last full pass was 10 days ago; an earnings actual landed 5 days
        // ago. The per-holding boundary makes it an unexamined event.
        doctor_latest_run(&paths, "MSFT", |v| {
            v.analyzed_at = Some(days_ago(10));
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
            Some(second.created_at.as_str()),
            "an unexamined evidence event force-includes"
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
                g.action_sizing.est_share_delta = Some(10.0);
                g.action_sizing.est_dollar_delta = Some(1_950.0);
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
                // The demoted hold is re-sized at current weights — the stale
                // add band never survives the demotion.
                let w = 3_900.0 / second.holdings.account_total;
                assert!(
                    (g.action_sizing.target_weight_low - 0.9 * w).abs() < 1e-12
                        && (g.action_sizing.target_weight_high - 1.1 * w).abs() < 1e-12,
                    "hold band re-anchored on today's weight: {:?}",
                    g.action_sizing
                );
                assert!(
                    g.action_sizing.est_dollar_delta.unwrap().abs() < 1e-6,
                    "a hold at current weight implies no adjustment: {:?}",
                    g.action_sizing
                );
            }
            other => panic!("expected a priced carry, got {other:?}"),
        }
    }

    #[test]
    fn a_carried_verdicts_sizing_recomputes_at_current_weights() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        // The user trimmed MSFT between runs — a same-side decrease, which
        // force-includes nothing. The carried action stands, but its sizing is
        // engine context and must read today's book, not the prior run's
        // (`docs/portfolio-analysis.md` §Triggering — current weights).
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
        let w = 1_950.0 / second.holdings.account_total;
        match &msft.disposition {
            crate::portfolio::VerdictDisposition::Priced(g) => {
                assert_eq!(g.action, crate::portfolio::Action::Hold);
                assert!(
                    (g.action_sizing.target_weight_low - 0.9 * w).abs() < 1e-12
                        && (g.action_sizing.target_weight_high - 1.1 * w).abs() < 1e-12,
                    "band re-anchored on today's weight: {:?}",
                    g.action_sizing
                );
                assert!(
                    g.action_sizing.est_dollar_delta.unwrap().abs() < 1e-6
                        && g.action_sizing.est_share_delta.unwrap().abs() < 1e-6,
                    "a carried hold at current weight implies no adjustment: {:?}",
                    g.action_sizing
                );
            }
            other => panic!("expected a priced carry, got {other:?}"),
        }
    }

    #[test]
    fn an_over_age_carried_exit_action_is_force_included_not_demoted() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        doctor_latest_run(&paths, "MSFT", |v| {
            v.analyzed_at = Some(days_ago(40));
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
        assert_eq!(
            verdict(&second, "MSFT").analyzed_at.as_deref(),
            Some(second.created_at.as_str()),
            "an over-age exit-family carry earns re-analysis, never a demotion"
        );
        assert_eq!(
            verdict(&second, "MSFT").action_source,
            crate::portfolio::ActionSource::ModelChosen
        );
    }

    #[test]
    fn a_side_reversal_re_enters_as_what_it_now_is() {
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
        // MSFT flipped net long → net short at equal magnitude since its verdict:
        // thesis-changing by construction, so no carried verdict survives it.
        let flipped = holdings_of(vec![
            stock("AAPL", 20.0, 3_900.0),
            stock("MSFT", -20.0, -3_900.0),
        ]);
        let second = selective_run(&paths, flipped, &["AAPL"], &SelectiveQuickData::default());
        let msft = verdict(&second, "MSFT");
        assert_eq!(msft.analyzed_at.as_deref(), Some(second.created_at.as_str()));
        assert!(
            matches!(
                &msft.disposition,
                crate::portfolio::VerdictDisposition::NotRated { reason }
                    if reason.contains("net short")
            ),
            "a long→short flip re-enters as the not-rated short it now is: {:?}",
            msft.disposition
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
