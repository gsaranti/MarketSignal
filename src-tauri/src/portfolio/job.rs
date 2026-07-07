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
use crate::sec::{self, CompanyFacts, SecEdgarSource};
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
/// per-company pull with keyless SEC EDGAR facts; a stub returns fixtures.
pub trait CompanyDataSource {
    /// FMP per-company financials (fail-soft; gaps recorded on the result).
    fn financials(&self, symbol: &str) -> CompanyFinancials;
    /// SEC EDGAR company facts plus any degraded-input notes ([`SecData`]).
    fn facts(&self, symbol: &str) -> SecData;
}

/// The live company-data source: FMP per-company + SEC EDGAR. SEC is supplementary and
/// fail-soft — an unresolved ticker or a fetch error degrades to empty facts, and the
/// FMP half plus the derived multiples still carry the holding — but each such
/// degradation is recorded as a gap so the audit stays honest.
pub struct LiveCompanyData {
    pub fmp: crate::fmp::FmpDataSource,
    pub sec: SecEdgarSource,
}

impl CompanyDataSource for LiveCompanyData {
    fn financials(&self, symbol: &str) -> CompanyFinancials {
        self.fmp.fetch_company_financials(symbol)
    }

    fn facts(&self, symbol: &str) -> SecData {
        match sec::cik_for_ticker(symbol) {
            // A ticker outside the slice's static CIK map: SEC could not be consulted.
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

    match run_analysis(holdings_source, company_data, analyst, profile, paths, &conn, ctx) {
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
fn run_analysis(
    holdings_source: &dyn HoldingsSource,
    company_data: &dyn CompanyDataSource,
    analyst: &dyn HoldingAnalyst,
    profile: &InvestorProfile,
    paths: &ReportPaths,
    conn: &Connection,
    ctx: &RunContext,
) -> Result<PortfolioRun> {
    ctx.step_started("holdings", "Pull holdings");
    let holdings = holdings_source.holdings()?;
    ctx.step_finished("holdings", "ok", None);

    // Deterministic holdings-change diff against the prior run's persisted snapshot
    // (Step 4), computed in the app layer before any model stage — the
    // compute-don't-guess boundary. Fail-soft: an unreadable prior run reads as "no
    // prior snapshot", so every position tags `new`, exactly as a first run does.
    let prior_holdings = store::latest_run(conn).ok().flatten().map(|r| r.holdings);
    let holdings_diff = diff::diff_holdings(prior_holdings.as_ref(), &holdings);

    let house_view = dossier::load_house_view(conn, &paths.reports_dir);

    let mut verdicts: Vec<HoldingVerdict> = Vec::with_capacity(holdings.positions.len());
    let mut audits: Vec<HoldingAudit> = Vec::with_capacity(holdings.positions.len());

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
        let mut fmp_financials = company_data.financials(&position.symbol);
        let sec_data = company_data.facts(&position.symbol);
        fmp_financials.gaps.extend(sec_data.gaps);
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
            prior,
        );

        // Cancellation checkpoint between the (now-complete) data gather and the model
        // stages, so a cancel mid-gather is observed before any model call is spent.
        if ctx.is_cancelled() {
            anyhow::bail!("run cancelled");
        }

        // The model/grade half is fail-hard: an interpretation or persistence error
        // fails the whole run (`docs/local-models.md §Failure posture`).
        let (verdict, audit) = analyze_holding(analyst, &dossier, holdings.account_total)?;
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
    let mut not_rated = 0;
    let mut insufficient = 0;
    for v in verdicts {
        match v.disposition {
            VerdictDisposition::Graded(_) => graded += 1,
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

    PortfolioRollUp {
        graded_count: graded,
        not_rated_count: not_rated,
        insufficient_evidence_count: insufficient,
        top_position_weight,
        cash_weight,
        exited: exited.to_vec(),
        overview: format!(
            "{graded} graded, {not_rated} not rated, {insufficient} insufficient-evidence; \
             top position {:.0}% of the account, cash {:.0}%.{exited_note}",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::pipeline::StubAnalyst;
    use crate::portfolio::{AssetClass, PositionChange};
    use crate::schwab::{FixtureHoldingsSource, Position};

    /// A stub company-data source serving strong fixture financials offline.
    struct StubCompanyData;
    impl CompanyDataSource for StubCompanyData {
        fn financials(&self, symbol: &str) -> CompanyFinancials {
            CompanyFinancials {
                symbol: symbol.to_string(),
                current_price: Some(195.0),
                market_cap: Some(3.0e12),
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
                ..CompanyFinancials::default()
            }
        }
        fn facts(&self, _symbol: &str) -> SecData {
            // The stub's FMP half already carries the financials, so SEC adds nothing
            // and — being a stub, not a failed fetch — records no gap.
            SecData::default()
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
        }
    }

    #[test]
    fn job_runs_end_to_end_offline_and_persists_a_graded_run() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &StubCompanyData,
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
    fn a_second_concurrent_run_is_skipped_by_the_shared_guard() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        // Hold the slot as if a report (or another local job) were running.
        let _token = guard.try_begin(RunKind::Report).unwrap();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &StubCompanyData,
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
        let company = LiveCompanyData { fmp, sec };

        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let start = std::time::Instant::now();
        let outcome = run_portfolio_job(
            &FixtureHoldingsSource::new(),
            &company,
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
            if let VerdictDisposition::Graded(g) = &v.disposition {
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
            matches!(run.verdicts[0].disposition, VerdictDisposition::Graded(_)),
            "the fixture equity should grade with live data"
        );
    }

    #[test]
    fn continuity_lookup_sees_the_prior_run_on_a_second_pass() {
        let (_dir, paths) = paths();
        let guard = RunGuard::default();
        let run_once = || {
            run_portfolio_job(
                &FixtureHoldingsSource::new(),
                &StubCompanyData,
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
