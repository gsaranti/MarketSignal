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
    carried_action, diff, store, ExitedPosition, HoldingAudit, HoldingVerdict, InvestorProfile,
    PortfolioRollUp, PortfolioRun,
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
    pub facts: CompanyFacts,
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

    fn sector_pe_snapshot(&self) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
        // The snapshot endpoint is date-keyed, so the date has to be a session that
        // actually traded. Two things follow, and this path had neither:
        //
        // - **The date is the ET session date**, not the UTC calendar date. An
        //   evening-ET run (after ~8 PM EDT / 7 PM EST) has already rolled to the
        //   next UTC day, so a UTC read asks for a session that has not happened —
        //   and the endpoint answers 200 with an empty array, not an error.
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
        let today = crate::market_clock::et_session_date(chrono::Utc::now());
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
    outcome_sources: Option<&crate::portfolio::outcome::OutcomeSources<'_>>,
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
        outcome_sources,
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
            // Alternate (chain) format, not `to_string()`: a context-wrapped
            // failure (e.g. the construction macro's) would otherwise persist
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
    outcome_sources: Option<&crate::portfolio::outcome::OutcomeSources<'_>>,
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

    // The run's one wall-clock instant, minted before any dated decision: the
    // house-view freshness gate, the over-age reads, the label pass, and the
    // persisted `created_at` (which the card's stale badge ages against) all
    // derive from it, so an hours-long run crossing ET midnight cannot demote on
    // one ET day and render the badge on the next. Run identity is insertion
    // order (`id`); `created_at` is display and vintage data, so stamping at run
    // start is a display choice — and the one that matches the session the run's
    // data belongs to.
    let created_at = now_rfc3339();
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
    // `created_at` — both legs convert together (see `load_house_view`).
    let (house_view, house_view_omitted) =
        dossier::load_house_view(conn, &paths.reports_dir, today);

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
    // (`created_at` / `today` are minted above, before the house-view gate — the
    // run's first dated decision.)
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
            // carried verdict survives it), an over-age carried exit-family
            // action (re-analysis is the only honest resolution; the add family
            // is rule-demoted at carry instead, and over-age holds stand), and
            // the one-time contract migration: a verdict authored under the
            // retired whole-book contract (pre-`portfolio-v9`) is never carried
            // into a tunnel-vision run — its action was a 7b-merged final that
            // may encode retired portfolio context, so it re-analyzes instead
            // (Codex 2026-08-14 round 2, finding 2). Self-neutralizing: one
            // full pass restamps the book and the check never fires again.
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
                let prior_era = prior
                    .audit
                    .iter()
                    .find(|a| a.symbol.eq_ignore_ascii_case(&p.symbol))
                    .map(|a| a.prompt_version.as_str());
                let force = delta.change == crate::portfolio::PositionChange::New
                    || delta.side_reversed(p.quantity)
                    // A current position with no prior verdict has nothing to
                    // carry, whatever the diff says.
                    || prior_verdict.is_none()
                    || crate::portfolio::whole_book_era_version(prior_era)
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

    // Deep-history health counter for the run-level data-health roll-up: a
    // non-empty gap list from `deep_price_history` means the FMP fetch degraded
    // and the holding's anchor window starved to its documented fallback.
    let mut deep_history_failures = 0usize;

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
    > = std::collections::HashMap::new();
    // The same profile lookup's issuer name, keyed alongside the sector so the
    // prompt header can name the company when Schwab's description is blank.
    let mut profile_name_by_symbol: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();

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
        let is_stock = matches!(position.asset_class, crate::portfolio::AssetClass::Stock);
        // One profile fetch per fresh-passed stock, feeding both the loop-time
        // listing-resolution guard (`docs/portfolio-analysis.md` §Asset eligibility)
        // and the entry-stamped sector identity; a fund is a multi-sector vehicle by
        // construction, typed `sector-unscorable` without a profile call.
        let listing = if is_stock {
            let lookup = company_data.profile_identity(&position.symbol);
            let (sector, name) = match &lookup {
                crate::portfolio::listing::ProfileLookup::Resolved(p) => {
                    (p.sector.clone(), p.company_name.clone())
                }
                _ => (None, None),
            };
            profile_name_by_symbol.insert(position.symbol.to_ascii_uppercase(), name);
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
        // Deep dated history (FMP dated EOD) for the anchor join and drawdown reads.
        let (deep_closes, deep_gaps) = if skip_retrieval {
            (vec![], vec![])
        } else {
            company_data.deep_price_history(&position.symbol)
        };
        if !deep_gaps.is_empty() {
            deep_history_failures += 1;
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
            })
        } else {
            None
        };
        // Fail-soft chain fetch: an auth/server fault or a malformed response degrades
        // this holding's options signal to a gap, but — unlike a silent drop — it is
        // recorded in the manifest so it reaches the audit and prompt rather than reading
        // as "no options listed" (`docs/schwab-integration.md §Failure posture`). Never a
        // whole-job failure; the error carries status/context only, never a token.
        let chain = if skip_retrieval {
            None
        } else {
            match holdings_source.option_chain(&position.symbol) {
                Ok(chain) => chain,
                Err(e) => {
                    fmp_financials
                        .gaps
                        .push(format!("Option chain unavailable for {}: {e}", position.symbol));
                    None
                }
            }
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
            listing,
            profile_name_by_symbol
                .get(&position.symbol.to_ascii_uppercase())
                .cloned()
                .flatten(),
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
        verdicts.push(verdict);
        audits.push(audit);
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
    // must survive the carry or the next sweep reads the holding `unknown`. The
    // over-age rule resolves per action family: a carried add-family action
    // rule-demotes to *hold*, stamped `action_source: rule-demoted` (exit-family
    // carries were force-included above; over-age holds stand).
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
    let roll_up = build_roll_up(
        &holdings,
        &verdicts,
        &holdings_diff.exited,
        &audits,
        deep_history_failures,
        rates.history_gap.is_some(),
        house_view_omitted,
        analyst.take_prompt_usage(),
    );
    // The deterministic outcome half: tag active episodes' net alignment from this
    // run's diff, refresh label-time price series through the shared bar cache and
    // record any newly due window labels (fail-soft — a failed retrieval leaves a
    // label pending, never a run failure), then append-or-extend this run's
    // decision episodes and derive the scorecard reads, all landing on the run
    // blob's outcome records.
    ctx.step_started("outcome", "Outcome learning");
    let run_id = uuid::Uuid::new_v4().to_string();
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
        // Authored at the persist seam. Every tunnel-vision run persists
        // complete — the marker survives for legacy degraded rows' sake
        // (`PortfolioRun::has_constructed_book`).
        constructed: Some(true),
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
    dgs10_history_gap: bool,
    house_view_omitted: bool,
    prompt_usage: Vec<crate::local_model::PromptUsage>,
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
        aggregates: None,
        construction: None,
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
            prompt_usage,
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
    prompt_usage: Vec<crate::local_model::PromptUsage>,
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
        || !output_limited.is_empty();
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
        context_pressure,
        peak_prompt,
        attention,
        summary,
    }
}

/// Current time as an RFC3339 UTC string — the canonical persisted form, like
/// [`crate::jobs`]; local-time conversion is a display concern at the UI seam.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
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
        let dh = build_data_health(&[], 0, false, false, usage);
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
        let dh = build_data_health(&[], 0, false, false, usage);
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
        let dh = build_data_health(&[], 0, false, false, usage);
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
        let dh = build_data_health(&[], 0, false, false, usage);
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
        let dh = build_data_health(&[], 0, false, false, usage);
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
        fn facts(&self, _symbol: &str) -> SecData {
            SecData::default()
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
            fn sector_pe_snapshot(&self) -> Result<Vec<crate::portfolio::fund::SectorPe>> {
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
        let company = LiveCompanyData { fmp, sec, cik };

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
    /// flag stays silent), with per-symbol overrides exercising the force-include
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
                _ => 195.0,
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
                        target_weight_low: Some(0.02),
                        target_weight_high: Some(0.05),
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
    fn a_pre_v9_carried_verdict_is_force_included_not_carried() {
        // The one-time contract migration (Codex 2026-08-14 round 2, finding 2):
        // a prior verdict stamped under the whole-book era re-analyzes on a
        // selective run rather than carrying — its action was a 7b-merged final
        // that may encode retired portfolio context. One full pass restamps the
        // book, so the check self-neutralizes.
        let (_dir, paths) = paths();
        full_run(&paths, two_stocks());
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
            Some(second.created_at.as_str()),
            "a whole-book-era verdict re-analyzes; it is never carried into a v9 run"
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
        // force-includes nothing. The carried action stands as-is (rung-only);
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
