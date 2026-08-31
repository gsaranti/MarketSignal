//! The engine-only **quick check** (`docs/portfolio-analysis.md §The quick check`,
//! `docs/portfolio-workflow.md §The quick check`): a cheap between-run pass that
//! keeps the thesis ledgers *live* — it loads the **last run's holdings snapshot and
//! ledgers** (no Schwab pull — it tests theses, not the book), refreshes prices, the
//! `DGS2`/`DGS10` prints, and the per-asset-type evidence legs, evaluates every
//! ledger's machine-checkable conditions under the shared persistence contract,
//! re-derives the hurdle (the stored v2 basis re-anchored closed-form on the fresh
//! `DGS10`) and scenario-band reads on priced verdicts, and raises **attention
//! flags** and quiet **evidence-event badges** — never rewriting any model-authored
//! content. No model call, no web research, no Schwab call.
//!
//! Its one write carve-out is each condition's **evaluation state** — engine state,
//! not authored content — persisted (with the flags, badges, and per-family sweep
//! states) in its own single-row store, **never** into `portfolio_runs`: a
//! quick-check write there would surface in the sidebar history and, worse, become
//! the next full run's diff baseline and ledger-carry source.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::fmp::{SymbolEarningsRow, SymbolNewsItem};
use crate::portfolio::engine::{
    self, CompanyFinancials, ConsensusEstimate, DatedValue,
};
use crate::portfolio::fund::{self, FundData};
use crate::portfolio::{
    holding_step_key, store, AssetClass, ConditionCadence, ConditionEvalState, ConditionRole,
    CrossingOutcome, HoldingAudit, HoldingVerdict, HurdleState, RatePrints, ScenarioKind,
    ThesisLedger, VerdictDisposition,
};
use crate::progress::RunContext;
use crate::sec::RecentFiling;

// ---- Calibration surface (drafted — `docs/portfolio-analysis.md §Starting
//      parameters`) -----------------------------------------------------------

/// The material EDGAR forms — the evidence-event leg and the statement-re-pull
/// trigger. Prefix-matched so an amended `10-K/A` counts with its base form.
const MATERIAL_FORMS: [&str; 3] = ["10-K", "10-Q", "8-K"];

/// Large-revision-move leg: the current-consensus EPS moving more than this
/// fraction — read only where the stored consensus is positive and at least the
/// absolute floor; otherwise the absolute test applies.
const REVISION_MOVE_FRACTION: f64 = 0.05;
const REVISION_ABS_FLOOR: f64 = 0.10;

/// Rate-cache max age for the quick paths' fail-soft (days, against the print's
/// as-of date — the drafted ~1-week bound reusing the house-view freshness bound).
const RATE_CACHE_MAX_AGE_DAYS: i64 = 7;

/// Fund exposure-shift leg: a top sector weight moving this much or more.
const TOP_SECTOR_SHIFT: f64 = 0.10;

/// The ≥ 70% US-exposure guard whose crossing (in either direction) is a fund
/// evidence event — the same drafted constant the strategy classification reads.
const US_EXPOSURE_GUARD: f64 = 0.70;

/// Expense-ratio float-noise guard for the material-`etf/info`-change leg.
const EXPENSE_EPS: f64 = 1e-6;

/// How many days of dated EOD closes the price refresh pulls — matching the full
/// run's 180-day volatility/trailing window so the reads share one basis.
pub const QUICK_EOD_LOOKBACK_DAYS: i64 = 180;

/// The semantics of persisted quick-check evaluation state. `v2` keeps the
/// split-anchor fetch widening isolated from the fixed 180-day trailing-return
/// and volatility window (Review 2 N3); `v3` retires rolling-NTM revision
/// events and compares estimates only on matched fiscal periods (Review 2 M11).
pub const QUICK_CHECK_PARAMETER_VERSION: &str = "quick-check-v3";

pub(crate) fn legacy_quick_check_parameter_version() -> String {
    "quick-check-v1".to_string()
}

// ---- Typed sweep results -----------------------------------------------------

/// One required signal family of a holding's sweep
/// (`docs/portfolio-analysis.md §The quick check` — the typed per-family states).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SweepFamily {
    /// The price + dated-closes refresh (also the frozen scenario-band read).
    MarketData,
    /// The EDGAR filing sweep (stock) / the `etf/info` print (fund) — the
    /// filing-cadence conditions' freshness leg.
    Filing,
    /// The lightweight `analyst-estimates` revision preflight (stock).
    Revision,
    /// The per-stock `earnings` re-pull.
    Earnings,
    /// The symbol-scoped news pull — present only while the holding carries a
    /// standing technology-class falsifier.
    NewsSeed,
    /// The per-fund `etf/info` + weightings refresh.
    FundInfo,
    /// The rate-dependent hurdle read (fresh `DGS2`/`DGS10` over the stored basis).
    RateAnchor,
}

/// A family's sweep state: **`fresh_clear`** — successfully checked and nothing
/// fired (a new observation evaluated clean, or a successful retrieval confirming
/// no unseen observation exists); **`flagged`**; or **`unknown`** — the retrieval
/// failed, the stored basis is missing, or a condition the family covers could
/// not be resolved this sweep, so the sweep could not vouch either way. On a
/// selective run `unknown` surfaces as a degraded-sweep **card badge**, exactly
/// like a `flagged` family — it no longer force-includes (since the 2026-08-16
/// ruling — `docs/portfolio-analysis.md §Triggering`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SweepState {
    FreshClear,
    Flagged,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FamilySweep {
    pub family: SweepFamily,
    pub state: SweepState,
    /// The degraded-sweep / first-breach note, where one applies.
    pub note: Option<String>,
}

/// The four deterministic attention-flag triggers
/// (`docs/portfolio-analysis.md §The quick check`; `docs/interface.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlagTrigger {
    ConfirmedFalsifierBreach,
    FiredTrigger,
    HurdleNewlyFails,
    PriceOutsideBand,
}

/// The amber, actionable attention flag — non-destructive, persisted with the
/// holding until the next successful full pass over it clears and acknowledges the
/// trigger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttentionFlag {
    pub trigger: FlagTrigger,
    pub detail: String,
    /// UTC RFC3339 of the quick check that raised it.
    pub raised_at: String,
}

/// The deterministic evidence-event legs (`docs/portfolio-analysis.md §Starting
/// parameters` — equity legs plus the fund legs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceEventKind {
    EarningsActual,
    MaterialFiling,
    RevisionMove,
    NewsSeed,
    FundInfoChange,
    ExposureShift,
}

/// One unexamined evidence event — the quiet, informational badge (the *Research
/// stale* family, never the amber action color), retained until a full pass over
/// the holding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceEvent {
    pub kind: EvidenceEventKind,
    pub detail: String,
    /// UTC RFC3339 of the quick check that observed it.
    pub observed_at: String,
}

/// One holding's persisted quick-check state — merged across successive quick
/// checks (a flag or event carries until the next full pass; only condition
/// evaluation state and the family view refresh each sweep).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingQuickState {
    pub symbol: String,
    /// The latest sweep's per-family states.
    pub families: Vec<FamilySweep>,
    /// The attention flag (plus which trigger raised it) — carried until a full
    /// pass; a later clean sweep never clears it.
    pub flag: Option<AttentionFlag>,
    /// Accumulated unexamined evidence events, deduplicated on (kind, detail).
    pub evidence_events: Vec<EvidenceEvent>,
    /// The freshest condition evaluation state per `condition_id` — the engine
    /// state the write carve-out covers; the next full run overlays these onto the
    /// prior ledger before its own evaluation so streaks and acknowledgments chain.
    pub condition_states: Vec<(String, ConditionEvalState)>,
    /// The last hurdle state this store observed — the "newly crossing into
    /// `fails`" comparator (seeded from the run's `dead_money` on first sweep).
    pub last_hurdle_state: Option<HurdleState>,
    /// Quiet notes (first-breach observations, per-condition degradations).
    pub notes: Vec<String>,
}

/// The whole persisted quick-check state — one row, keyed to the run it swept;
/// superseded (cleared) by the next successful full run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickCheckState {
    /// The evaluation semantics under which condition streaks and condition-
    /// sourced flags were produced.
    #[serde(default = "legacy_quick_check_parameter_version")]
    pub parameter_version: String,
    /// The full run these sweeps ran against (`PortfolioRun::run_id`).
    pub swept_run_id: String,
    /// UTC RFC3339 of the latest quick check.
    pub last_checked_at: String,
    /// The freshest successful `DGS2`/`DGS10` prints — the rate cache later quick
    /// checks (and their fail-soft) read alongside the run blob's.
    pub rate_cache: Option<RatePrints>,
    pub holdings: Vec<HoldingQuickState>,
}

/// Reconcile a persisted state with the current evaluation semantics while
/// preserving unrelated durable state. The v1→v2 boundary changed only the
/// trailing-return and return-volatility measurement window, so their streaks
/// and any flag explicitly sourced from one of those statements are retired.
/// The v2→v3 boundary retires only revision-move events, because their rolling
/// NTM comparator could have moved on calendar weights alone.
pub fn reconcile_parameter_version(
    state: &mut QuickCheckState,
    run: &crate::portfolio::PortfolioRun,
) {
    if state.parameter_version == QUICK_CHECK_PARAMETER_VERSION {
        return;
    }
    let legacy_v1 = state.parameter_version == "quick-check-v1";
    let legacy_v2 = state.parameter_version == "quick-check-v2";
    if !(legacy_v1 || legacy_v2) {
        for holding in &mut state.holdings {
            holding.condition_states.clear();
            holding
                .evidence_events
                .retain(|event| event.kind != EvidenceEventKind::RevisionMove);
            if holding.flag.as_ref().is_some_and(|f| {
                matches!(
                    f.trigger,
                    FlagTrigger::ConfirmedFalsifierBreach | FlagTrigger::FiredTrigger
                )
            }) {
                holding.flag = None;
            }
        }
        state.parameter_version = QUICK_CHECK_PARAMETER_VERSION.to_string();
        return;
    }
    if legacy_v1 {
        for holding in &mut state.holdings {
            let Some(verdict) = run
                .verdicts
                .iter()
                .find(|v| v.symbol.eq_ignore_ascii_case(&holding.symbol))
            else {
                // A legacy state with no ledger identity cannot be
                // interpreted safely. Preserve non-condition evidence only.
                holding.condition_states.clear();
                if holding.flag.as_ref().is_some_and(|f| {
                    matches!(
                        f.trigger,
                        FlagTrigger::ConfirmedFalsifierBreach | FlagTrigger::FiredTrigger
                    )
                }) {
                    holding.flag = None;
                }
                continue;
            };
            let affected: Vec<(&str, &str)> = verdict
                .thesis_ledger
                .as_ref()
                .into_iter()
                .flat_map(|l| &l.conditions)
                .filter(|c| {
                    c.quant.as_ref().is_some_and(|q| {
                        matches!(
                            q.series,
                            engine::LedgerSeries::TrailingReturn
                                | engine::LedgerSeries::ReturnVolatility
                        )
                    })
                })
                .map(|c| (c.condition_id.as_str(), c.statement.as_str()))
                .collect();
            holding
                .condition_states
                .retain(|(id, _)| !affected.iter().any(|(affected_id, _)| id == affected_id));
            if holding.flag.as_ref().is_some_and(|flag| {
                matches!(
                    flag.trigger,
                    FlagTrigger::ConfirmedFalsifierBreach | FlagTrigger::FiredTrigger
                ) && affected
                    .iter()
                    .any(|(_, statement)| flag.detail.ends_with(statement))
            }) {
                holding.flag = None;
            }
        }
    }
    for holding in &mut state.holdings {
        holding
            .evidence_events
            .retain(|event| event.kind != EvidenceEventKind::RevisionMove);
    }
    state.parameter_version = QUICK_CHECK_PARAMETER_VERSION.to_string();
}

// ---- The retrieval seam ------------------------------------------------------

/// The per-stock EDGAR filing sweep's outcome — typed so a missing CIK mapping or
/// a failed fetch reads `unknown` downstream rather than as no-new-filings
/// (`docs/portfolio-analysis.md §The quick check`).
#[derive(Debug, Clone)]
pub enum FilingSweep {
    Filings(Vec<RecentFiling>),
    NoCik,
    Failed(String),
}

/// The quick check's retrieval surface, behind a trait so the whole pass runs
/// offline against stubs. The live impl composes FMP (price/EOD, statements,
/// estimates, earnings, news, fund metadata), SEC EDGAR submissions (CIK-gated),
/// and the FRED prints. **No Schwab, no model.**
pub trait QuickCheckDataSource {
    /// The fresh price plus dated closes (the market-data observation identity and
    /// the volatility/trailing basis). `Err` types the market family `unknown`.
    /// `lookback_days` is at least [`QUICK_EOD_LOOKBACK_DAYS`], widened per
    /// holding so the split-bridge anchor bar stays inside the fetched window.
    fn price_and_closes(&self, symbol: &str, lookback_days: i64)
        -> Result<(f64, Vec<DatedValue>)>;
    /// The per-stock EDGAR recent-filings sweep.
    fn recent_filings(&self, symbol: &str) -> FilingSweep;
    /// The statement-and-dividends re-pull for a stock whose EDGAR sweep surfaced a
    /// new material filing (fail-soft; gaps recorded on the result).
    fn statements_refresh(&self, symbol: &str) -> CompanyFinancials;
    /// The lightweight revision preflight (`analyst-estimates`).
    fn consensus(&self, symbol: &str) -> Result<Option<ConsensusEstimate>>;
    /// Per-stock earnings rows, newest first.
    fn earnings(&self, symbol: &str) -> Result<Vec<SymbolEarningsRow>>;
    /// Symbol-scoped news since `from` — pulled only for tech-flagged holdings.
    fn news_since(&self, symbol: &str, from: &str) -> Result<Vec<SymbolNewsItem>>;
    /// The per-fund `etf/info` + weightings refresh.
    fn fund_data(&self, symbol: &str) -> FundData;
    /// The `DGS2` and `DGS10` prints (one FRED call each) — no history request.
    fn rates(&self) -> Result<(DatedValue, DatedValue)>;
}

/// The live composition: FMP + SEC (CIK-gated) + FRED. Constructing it performs no
/// network I/O — the same ordering invariant as
/// [`crate::portfolio::job::LiveCompanyData`]: every external fetch, the ticker →
/// CIK map refresh included, happens inside the global run slot after
/// `reset_cancel` + `run_started`.
pub struct LiveQuickCheckData {
    pub fmp: crate::fmp::FmpDataSource,
    pub sec: crate::sec::SecEdgarSource,
    /// The ticker → CIK map, **loaded on first use** (the first stock's filing
    /// sweep inside the slot) — [`crate::sec::LazyCikResolver`].
    pub cik: crate::sec::LazyCikResolver,
    pub fred: crate::fred::FredDataSource,
}

impl QuickCheckDataSource for LiveQuickCheckData {
    fn price_and_closes(
        &self,
        symbol: &str,
        lookback_days: i64,
    ) -> Result<(f64, Vec<DatedValue>)> {
        let price = self.fmp.fetch_live_price(symbol)?;
        let closes = self.fmp.fetch_dated_eod(symbol, lookback_days)?;
        if closes.is_empty() {
            anyhow::bail!("dated EOD history was empty for {symbol}");
        }
        Ok((price, closes))
    }

    fn recent_filings(&self, symbol: &str) -> FilingSweep {
        let Some(cik) = self.cik.resolve(&self.sec, symbol) else {
            return FilingSweep::NoCik;
        };
        match self.sec.fetch_recent_filings(cik) {
            Ok(rows) => FilingSweep::Filings(rows),
            Err(e) => FilingSweep::Failed(e.to_string()),
        }
    }

    fn statements_refresh(&self, symbol: &str) -> CompanyFinancials {
        let mut fin = CompanyFinancials {
            symbol: symbol.to_string(),
            ..CompanyFinancials::default()
        };
        fin.quarterly_income = self.fmp.fetch_quarterly_income(symbol, &mut fin.gaps);
        let balance = self.fmp.fetch_balance_sheet(symbol, &mut fin.gaps);
        fin.total_debt = balance.total_debt;
        fin.total_equity = balance.total_equity;
        fin.ttm_dividends_per_share = self.fmp.fetch_ttm_dividends(symbol, &mut fin.gaps);
        fin
    }

    fn consensus(&self, symbol: &str) -> Result<Option<ConsensusEstimate>> {
        self.fmp.fetch_analyst_estimates_strict(symbol)
    }

    fn earnings(&self, symbol: &str) -> Result<Vec<SymbolEarningsRow>> {
        self.fmp.fetch_symbol_earnings(symbol)
    }

    fn news_since(&self, symbol: &str, from: &str) -> Result<Vec<SymbolNewsItem>> {
        self.fmp.fetch_symbol_news_since(symbol, from)
    }

    fn fund_data(&self, symbol: &str) -> FundData {
        // The sweep's variant: no `profile` leg — the quick check never re-runs
        // closed-end detection (`docs/data-sources.md` §Portfolio Analysis —
        // endpoint surface: the profile row is full-pass only).
        self.fmp.fetch_fund_refresh_data(symbol)
    }

    fn rates(&self) -> Result<(DatedValue, DatedValue)> {
        let dgs2 = self.fred.latest_rate_dated("DGS2")?;
        let dgs10 = self.fred.latest_rate_dated("DGS10")?;
        Ok((dgs2, dgs10))
    }
}

// ---- The job lifecycle -------------------------------------------------------

/// The `job_runs.job_type` slug for quick checks, distinct from the full run's
/// `portfolio_analysis` so the two histories stay separable. `pub(crate)` because
/// `jobs::job_status` excludes these rows from the footer's last-run stamps — an
/// engine-only sweep is not the analysis freshness the footer reports.
pub(crate) const QUICK_CHECK_JOB: &str = "portfolio_quick_check";

/// Human title for the run tracker header.
const RUN_LABEL: &str = "Portfolio Quick Check";

/// Reason recorded when the concurrency guard rejects a run.
const SKIP_REASON: &str = "another run is already in progress";

/// How a quick check ended, mirroring [`crate::portfolio::job::PortfolioJobOutcome`].
#[derive(Debug)]
pub enum QuickCheckJobOutcome {
    Successful(Box<QuickCheckState>),
    Failed(String),
    Skipped(String),
    Cancelled(String),
}

/// Run one quick check end to end with the lifecycle contract: claim the **single
/// global run slot** (shared with the report and both local jobs), sweep, persist
/// the merged state, and record the outcome to `job_runs`. Returns `Err` only on
/// an infrastructure failure (the database); a failed sweep is a normal
/// `Ok(Failed)`.
pub fn run_quick_check_job(
    data: &dyn QuickCheckDataSource,
    paths: &crate::pipeline::ReportPaths,
    guard: &crate::jobs::RunGuard,
    ctx: &RunContext,
) -> Result<QuickCheckJobOutcome> {
    use crate::jobs::{record_run, JobRun, JobState, RunKind};

    let conn = crate::storage::open(&paths.db_path)?;
    crate::storage::init_schema(&conn)?;

    let _token = match guard.try_begin(RunKind::PortfolioQuickCheck) {
        Some(t) => t,
        None => {
            let now = now_rfc3339();
            record_run(
                &conn,
                &JobRun {
                    job_type: QUICK_CHECK_JOB,
                    state: JobState::Skipped,
                    started_at: &now,
                    finished_at: &now,
                    report_id: None,
                    detail: Some(SKIP_REASON),
                },
            )?;
            return Ok(QuickCheckJobOutcome::Skipped(SKIP_REASON.to_string()));
        }
    };

    ctx.reset_cancel();
    ctx.run_started(RUN_LABEL);
    let started_at = now_rfc3339();

    match run_quick_check(data, &conn, ctx) {
        Ok(state) => {
            let finished_at = now_rfc3339();
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: QUICK_CHECK_JOB,
                    state: JobState::Successful,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: None,
                    detail: Some(&state.swept_run_id),
                },
            );
            ctx.run_finished("successful", None, None);
            recorded?;
            Ok(QuickCheckJobOutcome::Successful(Box::new(state)))
        }
        Err(_) if ctx.is_cancelled() => {
            let finished_at = now_rfc3339();
            let detail = "run cancelled by user".to_string();
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: QUICK_CHECK_JOB,
                    state: JobState::Cancelled,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: None,
                    detail: Some(&detail),
                },
            );
            ctx.run_finished("cancelled", Some(detail.clone()), None);
            recorded?;
            Ok(QuickCheckJobOutcome::Cancelled(detail))
        }
        Err(e) => {
            let finished_at = now_rfc3339();
            let msg = e.to_string();
            let recorded = record_run(
                &conn,
                &JobRun {
                    job_type: QUICK_CHECK_JOB,
                    state: JobState::Failed,
                    started_at: &started_at,
                    finished_at: &finished_at,
                    report_id: None,
                    detail: Some(&msg),
                },
            );
            ctx.run_finished("failed", Some(msg.clone()), None);
            recorded?;
            Ok(QuickCheckJobOutcome::Failed(msg))
        }
    }
}

// ---- The pass ----------------------------------------------------------------

/// Run one quick check over the latest persisted run. Returns the merged,
/// persisted [`QuickCheckState`]; `Err` when no run exists to sweep (the honest
/// refusal — there is no ledger to keep live) or on a storage failure.
pub fn run_quick_check(
    data: &dyn QuickCheckDataSource,
    conn: &Connection,
    ctx: &RunContext,
) -> Result<QuickCheckState> {
    run_quick_check_at(data, conn, ctx, now_rfc3339())
}

fn run_quick_check_at(
    data: &dyn QuickCheckDataSource,
    conn: &Connection,
    ctx: &RunContext,
    now: String,
) -> Result<QuickCheckState> {
    let run = match store::latest_run(conn)? {
        Some(run) => run,
        // Two distinct refusals — `latest_run` returns `None` for both, and each
        // must be named truthfully: runs that exist but decoded on none of the
        // loud-skip passes are *unreadable*; only the empty store never ran.
        None if store::any_runs(conn)? => anyhow::bail!(
            "the retained Portfolio Analysis runs could not be read (see the app \
             log) — nothing to quick-check"
        ),
        None => anyhow::bail!("no Portfolio Analysis run exists yet — nothing to quick-check"),
    };
    let today = sweep_session_date(&now);

    // The prior quick-check state chains streaks / flags — but only against the
    // same run; a newer full run supersedes it wholesale.
    let mut prior_state = store::latest_quick_check(conn)?
        .filter(|s| s.swept_run_id == run.run_id);
    if let Some(state) = &mut prior_state {
        reconcile_parameter_version(state, &run);
    }

    // The run-level rate prints, fail-soft to the freshest cached print within the
    // drafted max age — none eligible reads the rate-dependent families `unknown`
    // (`docs/portfolio-analysis.md §The quick check`).
    ctx.step_started("rates", "Refresh rate prints (FRED)");
    let (rates, rate_note) = match data.rates() {
        Ok((dgs2, dgs10)) => {
            ctx.step_finished("rates", "ok", None);
            (
                Some(RatePrints {
                    dgs2: dgs2.value,
                    dgs10: dgs10.value,
                    dgs2_as_of: Some(dgs2.date),
                    dgs10_as_of: Some(dgs10.date),
                    fetched_at: now.clone(),
                }),
                None,
            )
        }
        Err(e) => {
            // Freshest cache first: a prior quick check's prints, else the run's.
            let cached = prior_state
                .as_ref()
                .and_then(|s| s.rate_cache.clone())
                .or_else(|| Some(run.rate_prints.clone()));
            match cached {
                Some(c) if rate_cache_fresh(&c, &today) => {
                    ctx.step_finished(
                        "rates",
                        "ok",
                        Some(format!("cached print ({e})")),
                    );
                    let note = format!(
                        "rate refresh failed — cached print as of {} used",
                        c.dgs10_as_of.as_deref().unwrap_or(&c.fetched_at)
                    );
                    (Some(c), Some(note))
                }
                _ => {
                    ctx.step_finished("rates", "failed", Some(e.to_string()));
                    (None, Some(format!("rate refresh failed and no cache within \
                         {RATE_CACHE_MAX_AGE_DAYS} days — rate-dependent families unknown ({e})")))
                }
            }
        }
    };

    // Sweep-eligible holdings, each with its **own** last-full-pass boundary —
    // per holding, not per run: a verdict a selective run carried forward keeps
    // its older vintage, so the evidence-event legs still look back to the pass
    // that actually examined it (`docs/portfolio-analysis.md` §Triggering).
    let targets: Vec<SweepTarget<'_>> = run
        .holdings
        .positions
        .iter()
        .filter_map(|p| {
            let verdict = run
                .verdicts
                .iter()
                .find(|v| v.symbol.eq_ignore_ascii_case(&p.symbol))?;
            sweep_eligible(verdict).then(|| SweepTarget {
                position: p,
                verdict,
                audit: run
                    .audit
                    .iter()
                    .find(|a| a.symbol.eq_ignore_ascii_case(&p.symbol)),
                last_pass_date: vintage_date(verdict, &run.created_at),
            })
        })
        .collect();

    let holdings_state = sweep_targets(
        SweepPass {
            data,
            targets,
            prior_state: prior_state.as_ref(),
            rates: rates.as_ref(),
            rate_note: rate_note.as_deref(),
            now: &now,
            today: &today,
        },
        ctx,
    )?;

    let state = QuickCheckState {
        parameter_version: QUICK_CHECK_PARAMETER_VERSION.to_string(),
        swept_run_id: run.run_id.clone(),
        last_checked_at: now,
        rate_cache: rates.or_else(|| {
            prior_state
                .as_ref()
                .and_then(|s| s.rate_cache.clone())
                .or_else(|| Some(run.rate_prints.clone()))
        }),
        holdings: holdings_state,
    };

    ctx.step_started("persist", "Persist quick-check state");
    store::save_quick_check(conn, &state)?;
    ctx.step_finished("persist", "ok", None);
    Ok(state)
}

/// Whether a verdict is sweep-eligible: an analyzed disposition or a standing
/// ledger (an insufficient-evidence exit retains its ledger, which the sweep
/// keeps evaluating).
fn sweep_eligible(verdict: &HoldingVerdict) -> bool {
    verdict.thesis_ledger.is_some()
        || matches!(
            verdict.disposition,
            VerdictDisposition::Priced(_) | VerdictDisposition::RoleRiskOnly(_)
        )
}

/// A verdict's effective last-full-pass **ET session date** (YYYY-MM-DD) inside
/// the run whose `created_at` is given — the per-holding "since the last full
/// pass" boundary ([`crate::portfolio::effective_vintage`]). ET, not the UTC
/// date prefix: an evening-ET pass has already rolled to the next UTC date, and
/// a UTC-dated boundary would hide the pass's own ET day *and* the entire next
/// ET day from the date-only filing/earnings feeds
/// ([`crate::market_clock::et_date_of`]).
/// Fetch slack past the holding's last-pass boundary so the split-bridge anchor
/// bar — the newest settled close strictly before that ET session — stays inside
/// the sweep's dated-EOD window across weekends, holidays, and short halts.
const ANCHOR_FETCH_SLACK_DAYS: i64 = 14;

/// The sweep's per-holding dated-EOD lookback: the shared 180-day floor
/// ([`QUICK_EOD_LOOKBACK_DAYS`]), widened to reach the holding's split-bridge
/// anchor bar when the last full pass is older than the floor. An unparseable
/// date keeps the floor — the bridge then degrades typed, never a wrong fetch.
fn eod_lookback_for(last_pass_date: &str, today: &str) -> i64 {
    use chrono::NaiveDate;
    let span = match (
        NaiveDate::parse_from_str(last_pass_date, "%Y-%m-%d"),
        NaiveDate::parse_from_str(today, "%Y-%m-%d"),
    ) {
        (Ok(v), Ok(t)) => (t - v).num_days() + ANCHOR_FETCH_SLACK_DAYS,
        _ => 0,
    };
    QUICK_EOD_LOOKBACK_DAYS.max(span)
}

fn vintage_date(verdict: &HoldingVerdict, run_created_at: &str) -> String {
    let vintage = crate::portfolio::effective_vintage(verdict, run_created_at);
    match crate::market_clock::et_date_of(vintage) {
        Some(d) => d.format("%Y-%m-%d").to_string(),
        // Unparseable vintage: the old prefix read, degraded rather than absent.
        None => vintage.chars().take(10).collect(),
    }
}

/// One holding's sweep assignment: the position row the price pass refreshes,
/// its verdict + audit from the run being swept, and the holding's own
/// last-full-pass boundary date.
pub struct SweepTarget<'a> {
    pub position: &'a crate::schwab::Position,
    pub verdict: &'a HoldingVerdict,
    pub audit: Option<&'a HoldingAudit>,
    pub last_pass_date: String,
}

/// One sweep pass over a set of targets — the core both the standalone quick
/// check and a selective run's in-run tail sweep execute.
struct SweepPass<'a> {
    data: &'a dyn QuickCheckDataSource,
    targets: Vec<SweepTarget<'a>>,
    /// The prior sweep's per-holding states: the carried flag / streak chain,
    /// and each holding's last values where a refresh fails.
    prior_state: Option<&'a QuickCheckState>,
    rates: Option<&'a RatePrints>,
    rate_note: Option<&'a str>,
    now: &'a str,
    today: &'a str,
}

/// Sweep every target: the price pass first (so every holding's legs read this
/// sweep's fresh marks), then the per-holding evidence legs and condition
/// evaluation, merged with each holding's carried quick-check state.
fn sweep_targets(pass: SweepPass<'_>, ctx: &RunContext) -> Result<Vec<HoldingQuickState>> {
    let mut prices: std::collections::HashMap<String, (f64, Vec<DatedValue>)> =
        std::collections::HashMap::new();
    let mut price_errors: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // The price pass runs under its own step: its per-symbol fetches emit
    // request rows, and a row arriving while no backend step runs falls
    // through to the tracker's synthesized never-finished "Baseline market
    // data" step — the exact Finding 6 phantom, reachable on the quick-check
    // and selective-tail paths after the report-pipeline half was fixed
    // (attempt-1 review sweep).
    ctx.step_started("sweep-prices", "Refresh prices (sweep)");
    for target in &pass.targets {
        if ctx.is_cancelled() {
            anyhow::bail!("run cancelled");
        }
        // The window must reach the split-bridge anchor's own bar — an
        // unresolvable full pass carries its prior anchor forward, so the
        // anchor can sit older than the holding's last-pass boundary.
        let anchor_date = target
            .audit
            .and_then(|a| a.authoring_close.as_ref())
            .map(|d| d.date.as_str())
            .filter(|d| *d < target.last_pass_date.as_str());
        let boundary = anchor_date.unwrap_or(&target.last_pass_date);
        let lookback = eod_lookback_for(boundary, pass.today);
        match pass.data.price_and_closes(&target.position.symbol, lookback) {
            Ok(pc) => {
                prices.insert(target.position.symbol.clone(), pc);
            }
            Err(e) => {
                price_errors.insert(target.position.symbol.clone(), e.to_string());
            }
        }
    }
    ctx.step_finished(
        "sweep-prices",
        "ok",
        (!price_errors.is_empty()).then(|| {
            format!(
                "{} of {} price refreshes failed; those holdings' market family reads unknown",
                price_errors.len(),
                pass.targets.len()
            )
        }),
    );

    let mut holdings_state: Vec<HoldingQuickState> = Vec::with_capacity(pass.targets.len());
    for target in &pass.targets {
        if ctx.is_cancelled() {
            anyhow::bail!("run cancelled");
        }
        let position = target.position;
        let step_key = holding_step_key(&position.symbol);
        ctx.step_started(step_key.clone(), format!("Check {}", position.symbol));
        // Case-insensitive like every neighboring symbol join (the diff, the
        // retention seam, `prior_verdict_for`) — a casing drift between the
        // fresh pull and the stored sweep state must not silently drop the
        // carried flag / streak chain.
        let prior_holding = pass.prior_state.and_then(|s| {
            s.holdings
                .iter()
                .find(|h| h.symbol.eq_ignore_ascii_case(&position.symbol))
        });
        let state = sweep_holding(SweepInputs {
            data: pass.data,
            position,
            verdict: target.verdict,
            audit: target.audit,
            prior: prior_holding,
            price: prices.get(&position.symbol),
            price_error: price_errors.get(&position.symbol).map(String::as_str),
            rates: pass.rates,
            rate_note: pass.rate_note,
            last_pass_date: &target.last_pass_date,
            today: pass.today,
            now: pass.now,
        });
        let status = match (&state.flag, state.families.iter().any(|f| f.state == SweepState::Unknown)) {
            (Some(_), _) => "flagged",
            (None, true) => "unknown",
            (None, false) => "ok",
        };
        ctx.step_finished(step_key, status, None);
        holdings_state.push(state);
    }
    Ok(holdings_state)
}

/// The in-run sweep a **selective run** executes over its unselected tail before
/// the per-holding loop (`docs/portfolio-analysis.md` §Triggering — the first
/// mixed-vintage safety rule). Positions come from the run's **fresh Step-2
/// pull** (current quantities and marks); verdicts, audits, and ledgers come
/// from the prior run being carried; each
/// holding's evidence-event boundary is its own effective vintage. The caller
/// owns persistence — the run's persist seam retains carried holdings' states
/// re-stamped to the new run.
pub struct TailSweep<'a> {
    pub data: &'a dyn QuickCheckDataSource,
    pub prior_run: &'a crate::portfolio::PortfolioRun,
    /// The current book (the fresh pull) — the sweep's position rows and marks.
    pub current_positions: &'a [crate::schwab::Position],
    /// Uppercased symbols of the unselected tail to sweep.
    pub tail: &'a std::collections::HashSet<String>,
    pub prior_state: Option<&'a QuickCheckState>,
    /// The run's fresh rate prints (a full run hard-fails without them).
    pub rates: RatePrints,
    /// The parent run's one pinned UTC instant. The tail sweep is part of that
    /// run, so every evaluation and badge must use its session rather than a
    /// second clock read that can cross the ET rollover.
    pub run_instant: &'a str,
}

pub fn sweep_tail(input: TailSweep<'_>, ctx: &RunContext) -> Result<Vec<HoldingQuickState>> {
    let today = sweep_session_date(input.run_instant);
    let targets: Vec<SweepTarget<'_>> = input
        .current_positions
        .iter()
        .filter(|p| input.tail.contains(&p.symbol.to_ascii_uppercase()))
        .filter_map(|p| {
            let verdict = input
                .prior_run
                .verdicts
                .iter()
                .find(|v| v.symbol.eq_ignore_ascii_case(&p.symbol))?;
            sweep_eligible(verdict).then(|| SweepTarget {
                position: p,
                verdict,
                audit: input
                    .prior_run
                    .audit
                    .iter()
                    .find(|a| a.symbol.eq_ignore_ascii_case(&p.symbol)),
                last_pass_date: vintage_date(verdict, &input.prior_run.created_at),
            })
        })
        .collect();
    sweep_targets(
        SweepPass {
            data: input.data,
            targets,
            prior_state: input.prior_state,
            rates: Some(&input.rates),
            rate_note: None,
            now: input.run_instant,
            today: &today,
        },
        ctx,
    )
}

/// Whether a cached rate print is young enough for the quick paths' fail-soft —
/// aged against the print's as-of date (falling back to the fetch date), the
/// drafted rate-cache max age.
/// The sweep's own date — the **ET session** of its instant, never the UTC date
/// prefix. Both consumers are session quantities: [`rate_cache_fresh`] compares
/// it against a FRED observation date (a market date), and it is the `run_date`
/// the ledger evaluation stamps into `first_breach_at` / `confirmed_at` /
/// `last_evaluated_at`, which must land on the same calendar the full run's
/// `run_date` uses or a sweep and the run that consumes it disagree by a day.
fn sweep_session_date(now: &str) -> String {
    crate::market_clock::et_date_of(now)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| now.chars().take(10).collect())
}

/// The fixed company-EOD window used by the full pass, carved out of the
/// sweep's potentially wider dated pull. Older rows exist only so the
/// split-adjustment bridge can re-read a carried anchor; admitting them into
/// `price_history` would silently change trailing-return and volatility
/// conditions for older carried holdings.
fn condition_price_history(closes: &[DatedValue], now: &str) -> Vec<f64> {
    use chrono::{DateTime, Duration, NaiveDate};

    let end = DateTime::parse_from_rfc3339(now)
        .ok()
        .map(|d| d.naive_utc().date())
        // Defensive fallback for a malformed persisted instant: anchor the
        // fixed-width window on the newest readable market row, never admit an
        // arbitrarily deep series just because the clock string drifted.
        .or_else(|| {
            closes
                .iter()
                .filter_map(|d| NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok())
                .max()
        });
    let Some(end) = end else {
        return Vec::new();
    };
    let start = end - Duration::days(QUICK_EOD_LOOKBACK_DAYS);
    closes
        .iter()
        .filter_map(|d| {
            let date = NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok()?;
            (date >= start && date <= end).then_some(d.value)
        })
        .collect()
}

fn rate_cache_fresh(cache: &RatePrints, today: &str) -> bool {
    // A FRED `as_of` is already a market day, so it parses directly. The
    // fallback — the cache's own fetch instant, used only where FRED served no
    // observation date — dates through the **ET session** like every other
    // session-keyed read: its UTC prefix would put an evening fetch a day ahead
    // and read the cache one day younger than it is.
    let as_of = cache
        .dgs10_as_of
        .as_deref()
        .or(cache.dgs2_as_of.as_deref())
        .map(|d| d.chars().take(10).collect::<String>())
        .unwrap_or_else(|| sweep_session_date(&cache.fetched_at));
    match (
        chrono::NaiveDate::parse_from_str(&as_of, "%Y-%m-%d"),
        chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d"),
    ) {
        (Ok(a), Ok(t)) => (t - a).num_days() <= RATE_CACHE_MAX_AGE_DAYS,
        _ => false,
    }
}

struct SweepInputs<'a> {
    data: &'a dyn QuickCheckDataSource,
    position: &'a crate::schwab::Position,
    verdict: &'a HoldingVerdict,
    audit: Option<&'a HoldingAudit>,
    prior: Option<&'a HoldingQuickState>,
    price: Option<&'a (f64, Vec<DatedValue>)>,
    price_error: Option<&'a str>,
    rates: Option<&'a RatePrints>,
    rate_note: Option<&'a str>,
    last_pass_date: &'a str,
    today: &'a str,
    now: &'a str,
}

/// Sweep one holding: the per-asset-type evidence legs, the gated condition
/// evaluation, the hurdle and band reads, and the typed per-family states —
/// merged with the holding's carried quick-check state.
fn sweep_holding(inp: SweepInputs<'_>) -> HoldingQuickState {
    let symbol = inp.position.symbol.clone();
    let is_fund = matches!(
        inp.position.asset_class,
        AssetClass::Etf | AssetClass::MutualFund
    );
    let is_stock = inp.position.asset_class == AssetClass::Stock;
    let priced = matches!(inp.verdict.disposition, VerdictDisposition::Priced(_));
    let basis = inp.audit.and_then(|a| a.quick_basis.as_ref());
    // The withheld-comparator signature: a priced pass that could not verify
    // its price basis carries its anchor but withholds the quick basis (and
    // stamps the monitor target-less). Its band, multiple, and revision legs
    // don't exist to check — the families read `unknown`, never a silent
    // `fresh_clear` vouch through legs the basis withheld. (An abstained row
    // lacks the anchor too, so it never matches.)
    let comparators_withheld = priced
        && inp
            .audit
            .is_some_and(|a| a.authoring_close.is_some() && a.quick_basis.is_none());

    let mut families: Vec<FamilySweep> = Vec::new();
    let mut events: Vec<EvidenceEvent> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut flags: Vec<AttentionFlag> = Vec::new();
    let mut flagged_families: std::collections::HashSet<SweepFamily> =
        std::collections::HashSet::new();

    let event = |kind, detail: String, now: &str| EvidenceEvent {
        kind,
        detail,
        observed_at: now.to_string(),
    };

    // The split-adjustment bridge (`docs/portfolio-analysis.md` §Starting
    // parameters): the stored anchor bar re-read from this sweep's fresh series
    // gives the exact cumulative re-basis factor since the last full pass —
    // exactly 1.0 in the unchanged common case. `Some(f)`: stored
    // price-denominated comparators convert through `f` (1.0 also covers a
    // no-price exit's row, which carries no anchor and runs as stored until a
    // full pass stamps one). `None`: an anchor exists but its bar is missing from the
    // fresh window — the basis is unverifiable, so price-denominated
    // comparisons are excluded this sweep rather than run cross-basis.
    let bridge: Option<f64> = match inp.audit.and_then(|a| a.authoring_close.as_ref()) {
        None => Some(1.0),
        Some(anchor) => inp
            .price
            .and_then(|(_, closes)| engine::split_bridge_factor(closes, anchor)),
    };
    match bridge {
        Some(f) if f != 1.0 => notes.push(format!(
            "price series re-based since the last full pass (split bridge factor \
             {f:.4}); stored price comparators converted onto the fresh basis"
        )),
        None if inp.price.is_some() => notes.push(
            "split-bridge anchor bar missing from the fresh price window — \
             price-denominated stored comparisons excluded this sweep"
                .to_string(),
        ),
        _ => {}
    }

    // -- Market-data leg (every holding) --------------------------------------
    let market_ok = inp.price.is_some();
    if let Some(err) = inp.price_error {
        families.push(FamilySweep {
            family: SweepFamily::MarketData,
            state: SweepState::Unknown,
            note: Some(format!("price refresh failed: {err}")),
        });
    }

    // -- Stock legs ------------------------------------------------------------
    let mut new_filing = false;
    let mut statements: Option<CompanyFinancials> = None;
    // The filing re-pull's dividend leg, captured before condition evaluation
    // consumes the refresh: the hurdle's payout leg refreshes on filing cadence
    // (`docs/portfolio-analysis.md` §The quick check).
    let mut filing_dividends: Option<f64> = None;
    if is_stock {
        match inp.data.recent_filings(&symbol) {
            FilingSweep::NoCik => families.push(FamilySweep {
                family: SweepFamily::Filing,
                state: SweepState::Unknown,
                note: Some(
                    "no CIK mapping — the filing feed is unverifiable for this symbol"
                        .to_string(),
                ),
            }),
            FilingSweep::Failed(e) => families.push(FamilySweep {
                family: SweepFamily::Filing,
                state: SweepState::Unknown,
                note: Some(format!("EDGAR sweep failed: {e}")),
            }),
            FilingSweep::Filings(rows) => {
                // Inclusive of the boundary day: filing dates are date-only, so
                // a filing landing later on the pass's own ET day cannot be told
                // apart from one the pass already saw. Inclusive re-raises a
                // seen boundary-day item's badge on every sweep until a full pass
                // on a *later* ET day advances the
                // vintage past it (a same-day re-pass re-lands on the boundary);
                // strict `>` would instead hide the unseen one permanently. One
                // contract across the filing / earnings / news legs.
                let fresh_material: Vec<&RecentFiling> = rows
                    .iter()
                    .filter(|f| {
                        f.filing_date.as_str() >= inp.last_pass_date
                            && MATERIAL_FORMS.iter().any(|m| f.form.starts_with(m))
                    })
                    .collect();
                let mut refresh_gaps: Vec<String> = Vec::new();
                if let Some(newest) = fresh_material.first() {
                    new_filing = true;
                    events.push(event(
                        EvidenceEventKind::MaterialFiling,
                        format!("{} filed {}", newest.form, newest.filing_date),
                        inp.now,
                    ));
                    // The statement-and-dividends re-pull: a filing-cadence
                    // condition's fresh observation arrives with the *value* the
                    // condition reads, not just the fact of the filing.
                    let mut fin = inp.data.statements_refresh(&symbol);
                    crate::portfolio::dossier::apply_ttm_statement_basis(&mut fin);
                    // The payout leg: a `None` dividends read with **no recorded
                    // gap** is the adapter's confirmed non-payer, so a dividend
                    // elimination reaches the hurdle as zero; a failed retrieval
                    // (gap recorded) keeps the stored leg instead.
                    let dividends_failed = fin
                        .gaps
                        .iter()
                        .any(|g| g.starts_with(crate::fmp::DIVIDENDS_GAP_PREFIX));
                    if !dividends_failed {
                        filing_dividends = Some(fin.ttm_dividends_per_share.unwrap_or(0.0));
                    }
                    refresh_gaps = fin.gaps.clone();
                    if fin.quarterly_income.is_empty() && refresh_gaps.is_empty() {
                        refresh_gaps.push("no quarterly income".to_string());
                    }
                    statements = Some(fin);
                }
                // A filing landed but a leg of the value re-pull failed
                // (statements, balance sheet, or dividends): the fact of the
                // filing is recorded (the event above), yet the sweep could not
                // fully evaluate it — the family cannot vouch, so it degrades
                // rather than clears.
                families.push(if refresh_gaps.is_empty() {
                    FamilySweep {
                        family: SweepFamily::Filing,
                        state: SweepState::FreshClear,
                        note: None,
                    }
                } else {
                    FamilySweep {
                        family: SweepFamily::Filing,
                        state: SweepState::Unknown,
                        note: Some(format!(
                            "a new filing landed but the statement re-pull was \
                             incomplete ({}) — the sweep could not fully evaluate it",
                            refresh_gaps.join("; ")
                        )),
                    }
                });
            }
        }

        // Revision preflight on fiscal periods shared with the stored snapshot.
        // The rolling NTM blend remains the valuation input, but is never a
        // revision comparator: its calendar weights move even when analysts do not.
        match inp.data.consensus(&symbol) {
            Err(e) => families.push(FamilySweep {
                family: SweepFamily::Revision,
                state: SweepState::Unknown,
                note: Some(format!("revision preflight failed: {e}")),
            }),
            Ok(fresh) => {
                // Stored per-share rows convert onto the fresh basis. The helper
                // then matches period ids and holds the prior maturity weights
                // constant, renormalizing them if only a surviving row overlaps.
                let stored_periods = basis
                    .map(|b| b.consensus_eps_periods.as_slice())
                    .unwrap_or_default();
                let fresh_periods = fresh
                    .as_ref()
                    .map(|c| c.eps_periods.as_slice())
                    .unwrap_or_default();
                let comparison = bridge.and_then(|factor| {
                    engine::consensus_revision_pair(stored_periods, fresh_periods, factor)
                });
                if let Some(pair) = &comparison {
                    if revision_moved(pair.prior, pair.current) {
                        events.push(event(
                            EvidenceEventKind::RevisionMove,
                            format!(
                                "same-period consensus EPS moved {:.2} → {:.2} since the \
                                 last full pass (fiscal period{})",
                                pair.prior,
                                pair.current,
                                if pair.period_ends.len() == 1 {
                                    format!(" {}", pair.period_ends[0])
                                } else {
                                    format!("s {}", pair.period_ends.join(", "))
                                }
                            ),
                            inp.now,
                        ));
                    }
                }
                if !stored_periods.is_empty() && bridge.is_none() {
                    let note = if inp.price.is_some() {
                        "price basis unverifiable (split-bridge anchor unresolvable) — \
                         the revision-move comparison was excluded this sweep"
                    } else {
                        "fresh price unavailable — the split-adjusted revision-move \
                         comparison was excluded this sweep"
                    };
                    families.push(FamilySweep {
                        family: SweepFamily::Revision,
                        state: SweepState::Unknown,
                        note: Some(note.to_string()),
                    });
                } else if comparators_withheld {
                    families.push(FamilySweep {
                        family: SweepFamily::Revision,
                        state: SweepState::Unknown,
                        note: Some(
                            "the last full pass could not verify the price basis \
                             and withheld its comparators — the revision-move \
                             comparison is unknown until a resolvable full pass"
                                .to_string(),
                        ),
                    });
                } else if comparison.is_none() {
                    let note = if stored_periods.is_empty() {
                        "the last full pass carried no fiscal-period consensus \
                         comparator — revision is unknown until a full pass \
                         records one"
                            .to_string()
                    } else if fresh_periods.is_empty() {
                        "fresh read carried no fiscal-period consensus — the \
                         revision-move comparison could not run this sweep"
                            .to_string()
                    } else {
                        "stored and fresh consensus carried no usable common \
                         fiscal period — revision is unknown rather than inferred \
                         from calendar roll-forward"
                            .to_string()
                    };
                    families.push(FamilySweep {
                        family: SweepFamily::Revision,
                        state: SweepState::Unknown,
                        note: Some(note),
                    });
                } else {
                    families.push(FamilySweep {
                        family: SweepFamily::Revision,
                        state: SweepState::FreshClear,
                        note: None,
                    });
                }
            }
        }

        // Earnings re-pull: a fresh actual since the last full pass.
        match inp.data.earnings(&symbol) {
            Err(e) => families.push(FamilySweep {
                family: SweepFamily::Earnings,
                state: SweepState::Unknown,
                note: Some(format!("earnings re-pull failed: {e}")),
            }),
            Ok(rows) => {
                if let Some(row) = rows
                    .iter()
                    // Inclusive, matching the filing leg's boundary contract.
                    .find(|r| r.date.as_str() >= inp.last_pass_date && r.eps_actual.is_some())
                {
                    events.push(event(
                        EvidenceEventKind::EarningsActual,
                        format!("earnings actual reported {}", row.date),
                        inp.now,
                    ));
                }
                families.push(FamilySweep {
                    family: SweepFamily::Earnings,
                    state: SweepState::FreshClear,
                    note: None,
                });
            }
        }

        // The qualifying-news-seed leg — only while a technology-class falsifier
        // stands (deliberately high-recall, never topic-matched).
        let tech_flagged = inp
            .verdict
            .thesis_ledger
            .as_ref()
            .map(|l| l.conditions.iter().any(|c| c.technology_class))
            .unwrap_or(false);
        if tech_flagged {
            match inp.data.news_since(&symbol, inp.last_pass_date) {
                Err(e) => families.push(FamilySweep {
                    family: SweepFamily::NewsSeed,
                    state: SweepState::Unknown,
                    note: Some(format!("news pull failed: {e}")),
                }),
                Ok(items) => {
                    if let Some(item) = items.first() {
                        events.push(event(
                            EvidenceEventKind::NewsSeed,
                            format!(
                                "fresh news on a tech-flagged holding: {} ({})",
                                item.title, item.published_date
                            ),
                            inp.now,
                        ));
                    }
                    families.push(FamilySweep {
                        family: SweepFamily::NewsSeed,
                        state: SweepState::FreshClear,
                        note: None,
                    });
                }
            }
        }
    }

    // -- Fund legs -------------------------------------------------------------
    let mut fund_metrics_expense: Option<f64> = None;
    let mut fund_info_ok = false;
    if is_fund {
        let fresh_fund = inp.data.fund_data(&symbol);
        fund_info_ok = fresh_fund.asset_class.is_some()
            || fresh_fund.expense_ratio.is_some()
            || fresh_fund.name.is_some()
            || !fresh_fund.sector_weights.is_empty();
        if !fund_info_ok {
            families.push(FamilySweep {
                family: SweepFamily::FundInfo,
                state: SweepState::Unknown,
                note: Some(format!(
                    "fund metadata unavailable: {}",
                    fresh_fund.gaps.join("; ")
                )),
            });
        } else if let Some(stored) = inp.audit.and_then(|a| a.fund_exposure.as_ref()) {
            fund_metrics_expense = fresh_fund.expense_ratio;
            let fresh_exposure = fund::exposure_basis(&fresh_fund);
            let equity_fund = stored.class_label.contains("equity")
                || fresh_exposure.class_label.contains("equity");
            // Legs the stored basis expects but this refresh couldn't supply:
            // a missing fresh print is a non-observation, never a zero or a
            // "change" — those legs degrade the family instead of fabricating
            // an evidence event. Endpoint gaps ride into the degraded list —
            // but the weightings legs bear on **equity** funds alone (no series
            // in the closed ledger surface reads exposure), so a bond or
            // commodity fund's empty equity weightings are its expected shape,
            // never a degraded sweep.
            let weightings_gap = |g: &&String| {
                g.starts_with(crate::fmp::FUND_SECTOR_WEIGHTS_GAP_PREFIX)
                    || g.starts_with(crate::fmp::FUND_COUNTRY_WEIGHTS_GAP_PREFIX)
            };
            let mut degraded: Vec<String> = fresh_fund
                .gaps
                .iter()
                .filter(|g| equity_fund || !weightings_gap(g))
                .cloned()
                .collect();
            let relevant_gaps_clean = degraded.is_empty();
            let weightings_degraded =
                fresh_fund.sector_weights.is_empty() && stored.top_sector.is_some();
            let us_degraded = stored.us_share.is_some() && fresh_exposure.us_share.is_none();
            // Material `etf/info` change: the expense ratio moving, or the
            // strategy-classification routing changing. A print appearing
            // where none was stored is a real change; a stored print the
            // refresh couldn't read is a degraded leg. The class-label
            // comparison runs only on a relevantly-ungapped refresh whose
            // exposure inputs are intact where the label depends on them — a
            // failed weightings or US-share leg reshapes an equity fund's
            // *derived* class ("equity fund without usable weightings"), which
            // is retrieval damage, not a mandate change; a non-equity label
            // derives from `etf/info` alone.
            let expense_moved = match (stored.expense_ratio, fresh_exposure.expense_ratio) {
                (Some(a), Some(b)) => (a - b).abs() > EXPENSE_EPS,
                (None, Some(_)) => true,
                (Some(_), None) => {
                    degraded.push("expense ratio unreadable this sweep".to_string());
                    false
                }
                (None, None) => false,
            };
            let info_leg_healthy = !fresh_fund
                .gaps
                .iter()
                .any(|g| g.starts_with(crate::fmp::FUND_INFO_GAP_PREFIX));
            // Every label derives from the asset-class string, so the label-level
            // comparison additionally requires it: an omitted `assetClass`
            // reshapes the label to "fund with unresolved strategy class" —
            // retrieval damage, not a mandate change.
            let class_comparable = relevant_gaps_clean
                && fresh_fund.asset_class.is_some()
                && (!equity_fund || (!weightings_degraded && !us_degraded));
            // The coarse mandate family (equity vs not) derives from `etf/info`
            // alone, so a real asset-class transition — a stored equity fund
            // freshly reporting Fixed Income — fires **for every fund** even
            // while the weightings-shaped label refinement is degraded (the
            // every-fund asset-class-change contract). Only an unreadable
            // mandate suppresses it — and that reads the family degraded, never
            // a silent clear.
            let mandate_comparable = info_leg_healthy && fresh_fund.asset_class.is_some();
            if info_leg_healthy && fresh_fund.asset_class.is_none() {
                degraded.push(
                    "asset class unreadable this sweep — the mandate leg was not checked"
                        .to_string(),
                );
            }
            let is_equity_label = |l: &str| l.contains("equity");
            let mandate_moved = mandate_comparable
                && is_equity_label(&stored.class_label)
                    != is_equity_label(&fresh_exposure.class_label);
            // The structural (option-overlay) flag keeps its class routing, so a
            // flag transition changes no label — it is its own change leg of the
            // every-fund contract (a structural-flag reclassification counts).
            // The flag reads from the fund's *name* blob, so it stays checkable
            // when only the asset class is missing — but needs a healthy info
            // leg and a returned name (a missing one would fake a flag-clear).
            let flag_comparable = info_leg_healthy && fresh_fund.name.is_some();
            let flag_moved =
                flag_comparable && stored.structural_flag != fresh_exposure.structural_flag;
            if fresh_fund.name.is_none() {
                degraded.push(
                    "fund name unreadable this sweep — the overlay-flag leg was not checked"
                        .to_string(),
                );
            }
            let class_moved = mandate_moved
                || flag_moved
                || (class_comparable && stored.class_label != fresh_exposure.class_label);
            if expense_moved || class_moved {
                events.push(event(
                    EvidenceEventKind::FundInfoChange,
                    format!(
                        "etf/info changed: class '{}' → '{}', expense {:?} → {:?}, \
                         overlay flag {} → {}",
                        stored.class_label,
                        fresh_exposure.class_label,
                        stored.expense_ratio,
                        fresh_exposure.expense_ratio,
                        stored.structural_flag,
                        fresh_exposure.structural_flag
                    ),
                    inp.now,
                ));
            }
            // Exposure shift (equity funds, either branch): the US-guard
            // crossing in either direction, or a top-sector move.
            if equity_fund {
                match (stored.us_share, fresh_exposure.us_share) {
                    (Some(a), Some(b)) => {
                        let crossed = (a >= US_EXPOSURE_GUARD) != (b >= US_EXPOSURE_GUARD);
                        if crossed {
                            events.push(event(
                                EvidenceEventKind::ExposureShift,
                                format!(
                                    "US share crossed the {:.0}% guard: {:.0}% → {:.0}%",
                                    US_EXPOSURE_GUARD * 100.0,
                                    a * 100.0,
                                    b * 100.0
                                ),
                                inp.now,
                            ));
                        }
                    }
                    (Some(_), None) => {
                        degraded.push("US-share read unreadable this sweep".to_string())
                    }
                    _ => {}
                }
                if let Some((label, stored_w)) = &stored.top_sector {
                    if fresh_fund.sector_weights.is_empty() {
                        degraded.push("sector weightings unreadable this sweep".to_string());
                    } else {
                        // A successful weightings read missing the stored
                        // top sector is a real observation (the sector left
                        // the fund's weightings) — zero is honest here.
                        let fresh_w = fresh_fund
                            .sector_weights
                            .iter()
                            .find(|(l, _)| l == label)
                            .map(|(_, w)| *w)
                            .unwrap_or(0.0);
                        if (fresh_w - stored_w).abs() >= TOP_SECTOR_SHIFT {
                            events.push(event(
                                EvidenceEventKind::ExposureShift,
                                format!(
                                    "top sector {label} moved {:.0}% → {:.0}%",
                                    stored_w * 100.0,
                                    fresh_w * 100.0
                                ),
                                inp.now,
                            ));
                        }
                    }
                }
            }
            families.push(if degraded.is_empty() {
                FamilySweep {
                    family: SweepFamily::FundInfo,
                    state: SweepState::FreshClear,
                    note: None,
                }
            } else {
                FamilySweep {
                    family: SweepFamily::FundInfo,
                    state: SweepState::Unknown,
                    note: Some(format!(
                        "fund refresh could not supply: {} — those legs were not checked",
                        degraded.join("; ")
                    )),
                }
            });
        } else {
            // No stored comparator (a pre-basis run): none of the fund change
            // legs can be evaluated, so the family cannot vouch — the
            // degraded-sweep state, never a claimed clear. Self-resolves when
            // the next full run persists the exposure basis.
            fund_metrics_expense = fresh_fund.expense_ratio;
            families.push(FamilySweep {
                family: SweepFamily::FundInfo,
                state: SweepState::Unknown,
                note: Some(
                    "no stored exposure basis from the last full pass — the fund \
                     change legs were not evaluated"
                        .to_string(),
                ),
            });
        }
    }

    // -- Condition evaluation (the write carve-out) ----------------------------
    let mut condition_states: Vec<(String, ConditionEvalState)> = inp
        .prior
        .map(|p| p.condition_states.clone())
        .unwrap_or_default();
    if let Some(ledger) = &inp.verdict.thesis_ledger {
        // Overlay the carried quick-check states so streaks chain sweep-to-sweep.
        let mut overlaid: ThesisLedger = (*ledger).clone();
        for cond in &mut overlaid.conditions {
            if let Some((_, st)) = condition_states
                .iter()
                .find(|(id, _)| *id == cond.condition_id)
            {
                cond.eval_state = Some(st.clone());
            }
        }
        // Price-denominated thresholds (and their absolute margins, same units)
        // convert onto the fresh basis before evaluation, so both sides of every
        // comparison share one basis; the streak semantics are invariant under
        // the conversion. Transient only — the sweep never rewrites the stored
        // ledger. An unresolvable bridge excludes these conditions below.
        if let Some(f) = bridge.filter(|f| *f != 1.0) {
            for cond in &mut overlaid.conditions {
                if let Some(q) = &mut cond.quant {
                    if q.series.price_denominated() {
                        q.threshold *= f;
                        q.margin *= f;
                    }
                }
            }
        }

        // The evaluation surface: fresh price + closes; valuation ratios scaled
        // from the stored full-pass metrics by the price move (denominators only
        // change on filing); statement metrics only when a fresh filing landed;
        // the fund expense ratio from the fresh `etf/info` print.
        let mut eval_fin = statements.take().unwrap_or_else(|| CompanyFinancials {
            symbol: symbol.clone(),
            ..CompanyFinancials::default()
        });
        if let Some((price, closes)) = inp.price {
            eval_fin.current_price = Some(*price);
            eval_fin.daily_closes = closes.clone();
            eval_fin.price_history = condition_price_history(closes, inp.now);
        }
        // The sweep is **not** the authority on statement-basis continuity, so it
        // neither fires the gate nor re-stamps: the values it evaluates span two
        // bases at once — filing series off this refresh, the three multiples
        // rescaled from the STORED full-pass audit by price alone — and one marker
        // cannot describe both. Left set, a refresh that flipped basis would adopt
        // the new stamp while the multiples were still on the old one, and the
        // genuine flip at the next full pass would then pass unnoticed. The full
        // pass computes every evaluated value from one basis, so it owns the gate.
        // The equity-source marker clears on the same terms (Codex I13): the
        // sweep's debt/equity reads its own FMP-only refresh — no SEC merge, so no
        // source is stamped — while price/book is rescaled from the stored audit,
        // and one marker cannot describe both here either. A debt/equity condition
        // stamped with another source is withheld below instead.
        eval_fin.statement_basis = None;
        eval_fin.equity_source = None;
        let mut metrics = engine::compute_metrics(&eval_fin);
        if let (Some((price, _)), Some(b), Some(stored)) =
            (inp.price, basis, inp.audit.map(|a| &a.metrics))
        {
            // The stored spot converts onto the fresh basis first, so the ratio
            // is the true price move; an unresolvable bridge leaves the stored
            // multiples absent rather than mis-scaled (fail closed — the mapped
            // conditions then type unevaluable).
            if let Some(f) = bridge.filter(|f| *f > 0.0) {
                if b.spot > 0.0 {
                    let ratio = price / (b.spot * f);
                    metrics.pe_ratio = stored.pe_ratio.map(|v| v * ratio);
                    metrics.ps_ratio = stored.ps_ratio.map(|v| v * ratio);
                    metrics.pb_ratio = stored.pb_ratio.map(|v| v * ratio);
                }
            }
        }
        metrics.expense_ratio = fund_metrics_expense;

        let statements_ok = !eval_fin.quarterly_income.is_empty();
        let allow = |series: engine::LedgerSeries| {
            // An unverifiable price basis excludes price-denominated conditions
            // whole — never a cross-basis comparison (the family downgrade
            // below keeps the exclusion visible).
            if series.price_denominated() && bridge.is_none() {
                return false;
            }
            match series.cadence() {
                ConditionCadence::MarketData => market_ok,
                ConditionCadence::Filing => {
                    if is_fund {
                        fund_info_ok
                    } else {
                        new_filing && statements_ok
                    }
                }
            }
        };
        // Codex rounds 1–2 on group 4 (I13): the sweep's debt/equity reads its
        // own FMP-only refresh — FMP's quarterly balance sheet, always — and the
        // sweep cannot re-stamp (it is not the authority: the marker is cleared
        // above). So it evaluates a D/E condition only when its streak is stamped
        // with the sweep's own source, and withholds it whole otherwise — another
        // source (SEC's annual equity, after a full pass whose FMP leg gapped: a
        // healed gap would step it, and a filing-cadence breach confirms at count
        // one), or no stamp at all (authored on a surface with no equity leg: a
        // sweep confirmation would persist unstamped, and the next full pass
        // would adopt whichever source it found, SEC included, with nothing to
        // disagree with). Withheld means typed unevaluable, no state movement,
        // the filing family downgraded to `unknown` like any
        // allowed-but-unresolvable series; the stamp lands at the next full pass.
        // Price/book is rescaled from the stored audit on the stamp's own source
        // and evaluates — an unstamped P/B has no stored ratio to rescale.
        let sweep_equity_source = crate::portfolio::EquitySource::FmpQuarterly;
        let withheld_reason = |c: &crate::portfolio::LedgerCondition| -> Option<String> {
            let quant = c.quant.as_ref()?;
            if quant.series != engine::LedgerSeries::DebtToEquity {
                return None;
            }
            match c.eval_state.as_ref().and_then(|s| s.authored_equity_source) {
                Some(src) if src == sweep_equity_source => None,
                Some(src) => Some(format!(
                    "debt/equity streak accumulated on {} — the sweep reads {} and \
                     cannot compare across the source; the full pass owns the gate",
                    src.label(),
                    sweep_equity_source.label()
                )),
                None => Some(format!(
                    "debt/equity streak carries no equity-source stamp — the sweep \
                     reads {} and cannot vouch the streak was accumulated on it; the \
                     full pass stamps it",
                    sweep_equity_source.label()
                )),
            }
        };
        let evaluable = ThesisLedger {
            conditions: overlaid
                .conditions
                .iter()
                .filter(|c| withheld_reason(c).is_none())
                .cloned()
                .collect(),
            ..overlaid.clone()
        };
        let mut eval = engine::evaluate_ledger_conditions_gated(
            &evaluable,
            &metrics,
            &eval_fin,
            inp.today,
            allow,
        );
        if allow(engine::LedgerSeries::DebtToEquity) {
            for c in &overlaid.conditions {
                if let Some(reason) = withheld_reason(c) {
                    eval.unevaluable
                        .push(format!("condition '{}': {reason}", c.statement));
                    eval.unevaluable_series.push(engine::LedgerSeries::DebtToEquity);
                }
            }
        }
        for line in &eval.unevaluable {
            // An allowed-but-unresolvable condition: its family could not vouch.
            notes.push(format!("unevaluable this sweep: {line}"));
        }
        for crossing in &eval.crossings {
            match crossing.outcome {
                CrossingOutcome::Confirmed => {
                    let (trigger, label) = match crossing.role {
                        ConditionRole::Falsifier => (
                            FlagTrigger::ConfirmedFalsifierBreach,
                            "confirmed falsifier breach",
                        ),
                        ConditionRole::Trigger => (FlagTrigger::FiredTrigger, "fired trigger"),
                    };
                    flags.push(AttentionFlag {
                        trigger,
                        detail: format!("{label}: {}", crossing.statement),
                        raised_at: inp.now.to_string(),
                    });
                    if let Some(cond) = overlaid
                        .conditions
                        .iter()
                        .find(|c| c.condition_id == crossing.condition_id)
                    {
                        if let Some(q) = &cond.quant {
                            flagged_families.insert(match q.series.cadence() {
                                ConditionCadence::MarketData => SweepFamily::MarketData,
                                ConditionCadence::Filing => {
                                    if is_fund {
                                        SweepFamily::FundInfo
                                    } else {
                                        SweepFamily::Filing
                                    }
                                }
                            });
                        }
                    }
                }
                CrossingOutcome::FirstBreach => {
                    notes.push(format!(
                        "first-breach note (unconfirmed): {}",
                        crossing.statement
                    ));
                }
            }
        }
        // Update the carried per-condition states with this sweep's evaluations.
        for (id, st) in eval.updated_states {
            match condition_states.iter_mut().find(|(cid, _)| *cid == id) {
                Some(entry) => entry.1 = st,
                None => condition_states.push((id, st)),
            }
        }

        // An allowed condition the sweep could not resolve (a missing statement
        // line, a sub-4-quarter TTM basis, an absent stored ratio): the mapped
        // family cannot vouch for the carried verdict, so a claimed clear
        // downgrades to `unknown` — a failed leg is typed, never a silent clear
        // (`docs/portfolio-analysis.md` §The quick check). The market family
        // records no clear entry, so it gains an `unknown` one.
        let unresolved: std::collections::HashSet<SweepFamily> = eval
            .unevaluable_series
            .iter()
            .map(|s| match s.cadence() {
                ConditionCadence::MarketData => SweepFamily::MarketData,
                ConditionCadence::Filing => {
                    if is_fund {
                        SweepFamily::FundInfo
                    } else {
                        SweepFamily::Filing
                    }
                }
            })
            .collect();
        // Bridge-excluded price legs: the market family cannot vouch for the
        // carried verdict this sweep, exactly like an unresolvable series. Both
        // excluded legs count — a price-denominated condition, and the frozen
        // band read a priced verdict would otherwise run (else a band-only
        // holding would read `fresh_clear` through a leg the basis skipped).
        // Downgraded with its own accurate note — the generic
        // condition-could-not-resolve wording would misdescribe a band-only
        // holding with no quantitative conditions at all.
        let band_read_skipped = priced && {
            let target = |kind: ScenarioKind| {
                overlaid
                    .monitor
                    .iter()
                    .find(|m| m.scenario == kind)
                    .and_then(|m| m.engine_target)
            };
            target(ScenarioKind::Bear).is_some() && target(ScenarioKind::Bull).is_some()
        };
        if comparators_withheld
            || (inp.price.is_some()
                && bridge.is_none()
                && (band_read_skipped
                    || overlaid
                        .conditions
                        .iter()
                        .any(|c| c.quant.as_ref().is_some_and(|q| q.series.price_denominated()))))
        {
            let note = if comparators_withheld {
                "the last full pass could not verify the price basis and withheld \
                 its comparators — the band and multiple legs are unknown until a \
                 resolvable full pass"
            } else {
                "price basis unverifiable (split-bridge anchor unresolvable) — \
                 price-denominated legs were excluded this sweep"
            };
            match families
                .iter_mut()
                .find(|f| f.family == SweepFamily::MarketData)
            {
                Some(entry) => {
                    if entry.state == SweepState::FreshClear {
                        entry.state = SweepState::Unknown;
                    }
                    entry.note = Some(match entry.note.take() {
                        Some(n) => format!("{n}; {note}"),
                        None => note.to_string(),
                    });
                }
                None => families.push(FamilySweep {
                    family: SweepFamily::MarketData,
                    state: SweepState::Unknown,
                    note: Some(note.to_string()),
                }),
            }
        }
        for fam in unresolved {
            let note = "a ledger condition on this family could not be resolved this sweep";
            match families.iter_mut().find(|f| f.family == fam) {
                Some(entry) => {
                    if entry.state == SweepState::FreshClear {
                        entry.state = SweepState::Unknown;
                        entry.note = Some(match entry.note.take() {
                            Some(n) => format!("{n}; {note}"),
                            None => note.to_string(),
                        });
                    }
                }
                None => families.push(FamilySweep {
                    family: fam,
                    state: SweepState::Unknown,
                    note: Some(note.to_string()),
                }),
            }
        }
    }

    // -- Hurdle read (priced only; rate-dependent) -----------------------------
    let mut last_hurdle_state = inp.prior.and_then(|p| p.last_hurdle_state);
    if priced {
        match (basis, inp.rates, inp.price, bridge.filter(|f| *f > 0.0)) {
            (Some(b), Some(r), Some((price, _)), Some(f)) => {
                // The payout leg refreshes on filing cadence (`docs/portfolio-analysis.md`
                // §The quick check): a new filing's dividend re-pull replaces the
                // stored trailing-TTM payout proxy, so a filing-driven change can
                // reach the hurdle; drivers and percentiles stay the stored basis
                // (no re-estimation). A gapped dividend read keeps the stored leg.
                let mut hurdle_basis = b.clone();
                if let Some(d) = filing_dividends {
                    // A fresh filing's per-share dividend arrives on the FRESH
                    // basis while the scenario runs on the stored one, so it
                    // converts like the price (`d ⁄ f`) — else a split plus a
                    // same-sweep filing mis-scales the payout leg by the factor.
                    hurdle_basis.forward_dividends = d / f;
                }
                // The fresh price converts INTO the stored basis (`price ⁄ f`), so
                // the whole scenario set stays on one basis; the total-return
                // reads are ratios, basis-free either way.
                let scenario = engine::reanchor_scenarios(&hurdle_basis, *price / f, r.dgs10);
                let tier = match &inp.verdict.disposition {
                    VerdictDisposition::Priced(g) => Some(g.risk_tier),
                    _ => None,
                };
                if let Some(tier) = tier {
                    let hurdle = engine::hurdle_read(&scenario, r.dgs2, tier);
                    let prior_hurdle = last_hurdle_state.or(match &inp.verdict.disposition {
                        VerdictDisposition::Priced(g) => Some(g.dead_money),
                        _ => None,
                    });
                    if hurdle.state == HurdleState::Fails
                        && prior_hurdle != Some(HurdleState::Fails)
                    {
                        flags.push(AttentionFlag {
                            trigger: FlagTrigger::HurdleNewlyFails,
                            detail: format!(
                                "capital-efficiency read newly fails the hurdle \
                                 (re-anchored TR bull {:.1}% vs hurdle {:.1}%)",
                                scenario.tr_bull * 100.0,
                                hurdle.hurdle_rate.unwrap_or(0.0) * 100.0
                            ),
                            raised_at: inp.now.to_string(),
                        });
                        flagged_families.insert(SweepFamily::RateAnchor);
                    }
                    last_hurdle_state = Some(hurdle.state);
                    families.push(FamilySweep {
                        family: SweepFamily::RateAnchor,
                        state: SweepState::FreshClear,
                        note: inp.rate_note.map(str::to_string),
                    });
                } else {
                    families.push(FamilySweep {
                        family: SweepFamily::RateAnchor,
                        state: SweepState::Unknown,
                        note: Some("no stored risk tier — hurdle not re-derivable".into()),
                    });
                }
            }
            (None, _, _, _) => families.push(FamilySweep {
                family: SweepFamily::RateAnchor,
                state: SweepState::Unknown,
                note: Some(if comparators_withheld {
                    "the last full pass could not verify the price basis and \
                     withheld the re-anchor basis — hurdle unknown until a \
                     resolvable full pass"
                        .into()
                } else {
                    "no stored re-anchor basis (the last full pass persisted none) — \
                     hurdle unknown until the next full run"
                        .into()
                }),
            }),
            (_, None, _, _) => families.push(FamilySweep {
                family: SweepFamily::RateAnchor,
                state: SweepState::Unknown,
                note: inp.rate_note.map(str::to_string).or_else(|| {
                    Some("rate prints unavailable — rate-dependent families unknown".into())
                }),
            }),
            (_, _, None, _) => families.push(FamilySweep {
                family: SweepFamily::RateAnchor,
                state: SweepState::Unknown,
                note: Some("no fresh price — hurdle not re-derivable".into()),
            }),
            (_, _, _, None) => families.push(FamilySweep {
                family: SweepFamily::RateAnchor,
                state: SweepState::Unknown,
                note: Some(
                    "price basis unverifiable (split-bridge anchor unresolvable) — \
                     hurdle not re-derivable this sweep"
                        .into(),
                ),
            }),
        }
    }

    // -- Scenario-band read (priced only; the stored monitor band, frozen) -----
    if priced {
        if let (Some((price, _)), Some(ledger), Some(f)) =
            (inp.price, &inp.verdict.thesis_ledger, bridge.filter(|f| *f > 0.0))
        {
            // The frozen band converts onto the fresh basis (`target × f`), so
            // the compared — and rendered — pair share one basis; an
            // unresolvable bridge skips the read (fail closed).
            let target = |kind: ScenarioKind| {
                ledger
                    .monitor
                    .iter()
                    .find(|m| m.scenario == kind)
                    .and_then(|m| m.engine_target)
                    .map(|t| t * f)
            };
            if let (Some(bear), Some(bull)) = (target(ScenarioKind::Bear), target(ScenarioKind::Bull))
            {
                let (lo, hi) = (bear.min(bull), bear.max(bull));
                // The flag fires on a *change* in spot's relationship to the frozen
                // band, never on the standing state: a band authored with spot
                // already outside was an examined observation (the model wrote the
                // ledger seeing it), so re-raising it every sweep — and force-
                // including the holding on every selective run — is noise. A
                // ledger authored with no spot to relate reads as authored-inside.
                let current = crate::portfolio::BandRelation::of(*price, bear, bull);
                let authored = ledger
                    .authored_band_relation
                    .unwrap_or(crate::portfolio::BandRelation::Inside);
                if current != authored {
                    let detail = match (authored, current) {
                        (crate::portfolio::BandRelation::Inside, _) => format!(
                            "price {price:.2} outside the ledger's bear–bull band \
                             [{lo:.2}, {hi:.2}] — the scenario read is stale in a way \
                             worth a fresh look"
                        ),
                        (_, crate::portfolio::BandRelation::Inside) => format!(
                            "price {price:.2} re-entered the ledger's bear–bull band \
                             [{lo:.2}, {hi:.2}] it was authored outside — the scenario \
                             read is stale in a way worth a fresh look"
                        ),
                        _ => format!(
                            "price {price:.2} crossed to the other side of the ledger's \
                             bear–bull band [{lo:.2}, {hi:.2}] — the scenario read is \
                             stale in a way worth a fresh look"
                        ),
                    };
                    flags.push(AttentionFlag {
                        trigger: FlagTrigger::PriceOutsideBand,
                        detail,
                        raised_at: inp.now.to_string(),
                    });
                    flagged_families.insert(SweepFamily::MarketData);
                }
            }
        }
    }

    // The market family's positive state lands last so a flag can upgrade it.
    if market_ok && !families.iter().any(|f| f.family == SweepFamily::MarketData) {
        families.push(FamilySweep {
            family: SweepFamily::MarketData,
            state: SweepState::FreshClear,
            note: None,
        });
    }
    for f in &mut families {
        if flagged_families.contains(&f.family) && f.state == SweepState::FreshClear {
            f.state = SweepState::Flagged;
        }
    }

    // -- Merge with the carried state ------------------------------------------
    // The flag persists until the next successful full pass over the holding —
    // a later clean sweep never clears it; the earliest raise wins.
    let flag = inp
        .prior
        .and_then(|p| p.flag.clone())
        .or_else(|| flags.into_iter().next());
    let mut evidence_events = inp
        .prior
        .map(|p| p.evidence_events.clone())
        .unwrap_or_default();
    for e in events {
        if !evidence_events
            .iter()
            .any(|prior| prior.kind == e.kind && prior.detail == e.detail)
        {
            evidence_events.push(e);
        }
    }

    HoldingQuickState {
        symbol,
        families,
        flag,
        evidence_events,
        condition_states,
        last_hurdle_state,
        notes,
    }
}

/// Overlay the quick-check store's fresher condition evaluation states onto a
/// prior verdict's ledger before the full run evaluates it — without this, streaks
/// and acknowledgments the between-run sweeps advanced would silently reset to the
/// blob's older state (`docs/portfolio-analysis.md §The quick check`: the
/// evaluation-state carve-out is engine state the full pass must consume, not
/// discard).
pub fn overlay_condition_states(verdict: &mut HoldingVerdict, holding_state: &HoldingQuickState) {
    let Some(ledger) = verdict.thesis_ledger.as_mut() else {
        return;
    };
    for cond in &mut ledger.conditions {
        if let Some((_, st)) = holding_state
            .condition_states
            .iter()
            .find(|(id, _)| *id == cond.condition_id)
        {
            cond.eval_state = Some(st.clone());
        }
    }
}

/// The large-revision-move rule (`docs/portfolio-analysis.md §Starting
/// parameters`, drafted): a percentage read only where the stored consensus is
/// positive and at least the denominator floor; a negative or below-floor
/// consensus tests the absolute move instead, so the ratio can't explode into a
/// noisy trigger.
fn revision_moved(stored: f64, fresh: f64) -> bool {
    // The floor is positive, so clearing it implies a positive consensus.
    if stored >= REVISION_ABS_FLOOR {
        ((fresh - stored) / stored).abs() > REVISION_MOVE_FRACTION
    } else {
        (fresh - stored).abs() >= REVISION_ABS_FLOOR
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_sweep_dates_itself_on_the_et_session_not_the_utc_prefix() {
        use super::sweep_session_date;
        // 9:30 PM ET on 2026-08-04 — the UTC instant has rolled to the 5th, but the
        // sweep belongs to the 4th's session. The old UTC-prefix read dated it the
        // 5th, which aged a rate print by one against `RATE_CACHE_MAX_AGE_DAYS` (a
        // FRED observation date is a market day) and stamped the ledger evaluation's
        // `first_breach_at` / `confirmed_at` on a session the sweep never saw — a day
        // ahead of the full run that consumes those states.
        assert_eq!(sweep_session_date("2026-08-05T01:30:00+00:00"), "2026-08-04");
        // Mid-session, where the two readings agree.
        assert_eq!(sweep_session_date("2026-08-05T15:00:00+00:00"), "2026-08-05");
        // Degradation is the old prefix read, never a panic or an empty date: a
        // date-only value carries no instant to convert, and a malformed stamp keeps
        // the pre-conversion behavior rather than vanishing.
        assert_eq!(sweep_session_date("2026-08-05"), "2026-08-05");
        assert_eq!(sweep_session_date("2026-08-05T99:99:99"), "2026-08-05");
    }

    use super::*;
    use crate::portfolio::engine::{LedgerSeries, QuickCheckBasis};
    use crate::portfolio::{
        Grade, GradedVerdict, HorizonOutlook, HorizonRead, LedgerBranch, LedgerComparator,
        LedgerCondition, MonitorScenario, OptionsSignal, PortfolioRollUp, PortfolioRun,
        PriceTarget, PriceTargets, QuantCore, RiskTier, SubScores,
    };
    use crate::schwab::{Holdings, Position};

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::storage::init_schema(&conn).unwrap();
        conn
    }

    fn noop_ctx() -> std::sync::Arc<RunContext> {
        RunContext::noop()
    }

    fn position(symbol: &str, class: AssetClass) -> Position {
        Position {
            symbol: symbol.into(),
            description: symbol.into(),
            asset_class: class,
            quantity: 100.0,
            cost_basis: 10_000.0,
            market_value: 19_500.0,
            current_price: Some(195.0),
        }
    }

    fn price_condition(id: &str, role: ConditionRole, threshold: f64) -> LedgerCondition {
        LedgerCondition {
            condition_id: id.into(),
            role,
            trigger_family: (role == ConditionRole::Trigger)
                .then_some(crate::portfolio::TriggerFamily::Trim),
            statement: format!("price below {threshold}"),
            quant: Some(QuantCore {
                series: LedgerSeries::Price,
                comparator: LedgerComparator::Below,
                threshold,
                margin: 0.0,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: None,
        }
    }

    fn ledger(conditions: Vec<LedgerCondition>) -> ThesisLedger {
        ThesisLedger {
            branch: LedgerBranch::Priced,
            original_thesis: "debut thesis".into(),
            current_thesis: "standing thesis".into(),
            key_drivers: vec![],
            monitor: vec![
                MonitorScenario {
                    scenario: ScenarioKind::Bear,
                    conditions: "bear".into(),
                    probability_pct: 25.0,
                    engine_target: Some(150.0),
                },
                MonitorScenario {
                    scenario: ScenarioKind::Base,
                    conditions: "base".into(),
                    probability_pct: 50.0,
                    engine_target: Some(210.0),
                },
                MonitorScenario {
                    scenario: ScenarioKind::Bull,
                    conditions: "bull".into(),
                    probability_pct: 25.0,
                    engine_target: Some(260.0),
                },
            ],
            what_must_improve: String::new(),
            what_must_not_break: String::new(),
            conditions,
            authored_band_relation: None,
        }
    }

    fn priced_verdict(symbol: &str, conditions: Vec<LedgerCondition>) -> HoldingVerdict {
        HoldingVerdict {
            symbol: symbol.into(),
            asset_class: AssetClass::Stock,
            position_change: Default::default(),
            disposition: VerdictDisposition::Priced(Box::new(GradedVerdict {
                grade: Grade::B,
                sub_scores: SubScores { quality: 70.0, valuation: 60.0, momentum: 50.0, risk: 65.0 },
                action: crate::portfolio::Action::Hold,
                action_rationale: String::new(),
                model_view: crate::portfolio::ModelView {
                    sub_scores: SubScores { quality: 70.0, valuation: 60.0, momentum: 50.0, risk: 65.0 },
                    letter: Grade::B,
                    price_targets: crate::portfolio::ModelPriceTargets {
                        one_month: crate::portfolio::ModelPriceTarget { base: 195.0, bear: 180.0, bull: 210.0 },
                        twelve_month: crate::portfolio::ModelPriceTarget { base: 210.0, bear: 150.0, bull: 260.0 },
                    },
                    self_assessment: String::new(),
                },
                engine_view: crate::portfolio::EngineView {
                    outlook: HorizonOutlook {
                        short: HorizonRead::Neutral,
                        mid: HorizonRead::Bullish,
                        long: HorizonRead::Bullish,
                    },
                    conviction: crate::portfolio::Conviction::Medium,
                    action: crate::portfolio::Action::Hold,
                },
                conviction: crate::portfolio::Conviction::Medium,
                horizon_outlook: HorizonOutlook {
                    short: HorizonRead::Neutral,
                    mid: HorizonRead::Bullish,
                    long: HorizonRead::Bullish,
                },
                price_targets: PriceTargets {
                    one_month: None,
                    twelve_month: Some(PriceTarget {
                        base: 210.0,
                        bear: 150.0,
                        bull: 260.0,
                        methodology: "fixture".into(),
                    }),
                },
                price_target_rationale: "fixture".into(),
                options_signal: OptionsSignal {
                    put_call_volume: None,
                    put_call_open_interest: None,
                    implied_volatility: None,
                    iv_skew: None,
                },
                risk_tier: RiskTier::Medium,
                dead_money: HurdleState::Indeterminate,
                low_confidence_grade: false,
                fund_class_label: None,
                structural_flag: false,
                financial_summary: "fixture".into(),
                what_changed: "fixture".into(),
            })),
            thesis_ledger: Some(ledger(conditions)),
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        }
    }

    fn basis() -> QuickCheckBasis {
        // Spread percentiles chosen so the re-anchored targets bracket the fixture
        // prices (~182 / 210 / 241 at DGS10 4.5%): the hurdle reads indeterminate
        // at the quiet test prices, so only the trigger under test can flag.
        QuickCheckBasis {
            spot: 195.0,
            drivers: [6.0, 6.5, 7.0],
            spread_percentiles: Some([-0.012, -0.014, -0.016]),
            raw_percentiles: Some([25.0, 28.0, 31.0]),
            forward_dividends: 1.0,
            dispersion_floor: 0.05,
            consensus_eps_mid: Some(6.5),
            consensus_eps_periods: vec![engine::ConsensusEpsPeriod {
                period_end: "2026-12-31".into(),
                eps_mid: Some(6.5),
                ntm_weight: 1.0,
            }],
        }
    }

    fn audit_for(symbol: &str, quick_basis: Option<QuickCheckBasis>) -> HoldingAudit {
        HoldingAudit {
            what_changed_audit: None,
            research: None,
            symbol: symbol.into(),
            metrics: engine::ComputedMetrics {
                pe_ratio: Some(30.0),
                ps_ratio: Some(7.5),
                pb_ratio: Some(6.0),
                ..Default::default()
            },
            sources: vec![],
            model_ids: vec![],
            prompt_version: crate::portfolio::PROMPT_VERSION.into(),
            evidence_floor_version: crate::portfolio::engine::EVIDENCE_FLOOR_VERSION.to_string(),
            degraded_inputs: vec![],
            action_annotations: vec![],
            target_meta: None,
            grade_parameter_version: "grade-v2".into(),
            ledger_audit: None,
            quick_basis,
            authoring_close: None,
            fund_exposure: None,
            pre_profit: None,
            hurdle: None,
            forensic: None,
            tech_event_pre_flag: None,
            short_interest: None,
            implied_expectations: None,
            narrative: None,
            option_overlay: None,
        }
    }

    fn sample_run(verdict: HoldingVerdict, audit: HoldingAudit) -> PortfolioRun {
        let pos = position(&verdict.symbol, verdict.asset_class);
        PortfolioRun {
            run_id: "run-1".into(),
            created_at: "2026-07-20T00:00:00Z".into(),
            holdings: Holdings {
                positions: vec![pos],
                cash: 10_000.0,
                account_total: 29_500.0,
                source_rows: vec![],
            },
            verdicts: vec![verdict],
            roll_up: PortfolioRollUp {
                graded_count: 1,
                not_rated_count: 0,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                failed_count: 0,
                top_position_weight: 0.66,
                cash_weight: 0.34,
                exited: vec![],
                data_health: Default::default(),
                overview: "fixture".into(),
            },
            audit: vec![audit],
            rate_prints: RatePrints {
                dgs2: 0.04,
                dgs10: 0.045,
                dgs2_as_of: Some("2026-07-18".into()),
                dgs10_as_of: Some("2026-07-18".into()),
                fetched_at: "2026-07-20T00:00:00Z".into(),
            },
            outcome: Default::default(),
            failed_holdings: Vec::new(),
        }
    }

    /// A scriptable stub source. Every leg succeeds with quiet values unless a
    /// field overrides it.
    struct StubData {
        price: Result<(f64, Vec<DatedValue>), String>,
        filings: FilingSweep,
        statements: CompanyFinancials,
        consensus: Result<Option<ConsensusEstimate>, String>,
        earnings: Result<Vec<SymbolEarningsRow>, String>,
        news: Result<Vec<SymbolNewsItem>, String>,
        fund: FundData,
        rates: Result<(DatedValue, DatedValue), String>,
    }

    impl StubData {
        fn quiet(price: f64, close_date: &str) -> Self {
            Self {
                price: Ok((
                    price,
                    vec![
                        DatedValue { date: "2026-07-01".into(), value: 190.0 },
                        DatedValue { date: close_date.into(), value: price },
                    ],
                )),
                filings: FilingSweep::Filings(vec![]),
                statements: CompanyFinancials::default(),
                consensus: Ok(Some(ConsensusEstimate {
                    eps_mid: Some(6.5),
                    eps_periods: vec![engine::ConsensusEpsPeriod {
                        period_end: "2026-12-31".into(),
                        eps_mid: Some(6.5),
                        ntm_weight: 1.0,
                    }],
                    ..Default::default()
                })),
                earnings: Ok(vec![]),
                news: Ok(vec![]),
                fund: FundData::default(),
                rates: Ok((
                    DatedValue { date: "2026-08-01".into(), value: 0.04 },
                    DatedValue { date: "2026-08-01".into(), value: 0.045 },
                )),
            }
        }
    }

    impl QuickCheckDataSource for StubData {
        fn price_and_closes(
            &self,
            _symbol: &str,
            _lookback_days: i64,
        ) -> Result<(f64, Vec<DatedValue>)> {
            self.price.clone().map_err(|e| anyhow::anyhow!(e))
        }
        fn recent_filings(&self, _symbol: &str) -> FilingSweep {
            self.filings.clone()
        }
        fn statements_refresh(&self, _symbol: &str) -> CompanyFinancials {
            self.statements.clone()
        }
        fn consensus(&self, _symbol: &str) -> Result<Option<ConsensusEstimate>> {
            self.consensus.clone().map_err(|e| anyhow::anyhow!(e))
        }
        fn earnings(&self, _symbol: &str) -> Result<Vec<SymbolEarningsRow>> {
            self.earnings.clone().map_err(|e| anyhow::anyhow!(e))
        }
        fn news_since(&self, _symbol: &str, _from: &str) -> Result<Vec<SymbolNewsItem>> {
            self.news.clone().map_err(|e| anyhow::anyhow!(e))
        }
        fn fund_data(&self, _symbol: &str) -> FundData {
            self.fund.clone()
        }
        fn rates(&self) -> Result<(DatedValue, DatedValue)> {
            self.rates.clone().map_err(|e| anyhow::anyhow!(e))
        }
    }

    #[test]
    fn the_evidence_event_boundary_is_per_holding_not_per_run() {
        // Two holdings in one run: FRESH was analyzed by the run itself; CARRD
        // rides vintage-stamped from an older pass (a selective run's carry). An
        // earnings actual dated between the two vintages is an unexamined event
        // for the carried holding only — the boundary is each holding's own
        // last full pass, never the run's `created_at`.
        let conn = mem();
        let mut fresh = priced_verdict("FRESH", vec![]);
        fresh.analyzed_at = Some("2026-08-01T00:00:00Z".into());
        let mut carried = priced_verdict("CARRD", vec![]);
        carried.analyzed_at = Some("2026-07-20T00:00:00Z".into());
        let run = PortfolioRun {
            run_id: "run-mixed".into(),
            created_at: "2026-08-01T00:00:00Z".into(),
            holdings: Holdings {
                positions: vec![
                    position("FRESH", AssetClass::Stock),
                    position("CARRD", AssetClass::Stock),
                ],
                cash: 10_000.0,
                account_total: 49_000.0,
                source_rows: vec![],
            },
            verdicts: vec![fresh, carried],
            roll_up: PortfolioRollUp {
                graded_count: 2,
                not_rated_count: 0,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                failed_count: 0,
                top_position_weight: 0.4,
                cash_weight: 0.2,
                exited: vec![],
                data_health: Default::default(),
                overview: "fixture".into(),
            },
            audit: vec![
                audit_for("FRESH", Some(basis())),
                audit_for("CARRD", Some(basis())),
            ],
            rate_prints: Default::default(),
            outcome: Default::default(),
            failed_holdings: Vec::new(),
        };
        store::insert_run(&conn, &run).unwrap();
        let mut data = StubData::quiet(195.0, "2026-08-02");
        data.earnings = Ok(vec![SymbolEarningsRow {
            date: "2026-07-25".into(),
            eps_actual: Some(2.10),
            eps_estimated: Some(2.00),
            revenue_actual: None,
        }]);
        let state = run_quick_check(&data, &conn, &noop_ctx()).unwrap();
        let by = |sym: &str| {
            state
                .holdings
                .iter()
                .find(|h| h.symbol == sym)
                .unwrap_or_else(|| panic!("{sym} swept"))
        };
        assert!(
            by("CARRD")
                .evidence_events
                .iter()
                .any(|e| e.kind == EvidenceEventKind::EarningsActual),
            "the carried holding's older vintage sees the event"
        );
        assert!(
            !by("FRESH")
                .evidence_events
                .iter()
                .any(|e| e.kind == EvidenceEventKind::EarningsActual),
            "the fresh holding's own vintage bounds its event window"
        );
    }

    #[test]
    fn the_evidence_boundary_is_the_inclusive_et_session_date() {
        // An evening-ET full pass: 2026-08-05 01:30 UTC = 2026-08-04 21:30 EDT.
        // The boundary is the ET session day (the 4th), inclusive: an 8-K and an
        // earnings actual dated the pass's own ET day are visible to the sweep.
        // Under the old UTC-prefix strict boundary (`> "2026-08-05"`) both were
        // permanently invisible — the piece-3 ruling-1 repro.
        let conn = mem();
        let mut verdict = priced_verdict("AAPL", vec![]);
        verdict.analyzed_at = Some("2026-08-05T01:30:00+00:00".into());
        let mut run = sample_run(verdict, audit_for("AAPL", Some(basis())));
        run.created_at = "2026-08-05T01:30:00+00:00".into();
        store::insert_run(&conn, &run).unwrap();
        let mut data = StubData::quiet(195.0, "2026-08-05");
        data.filings = FilingSweep::Filings(vec![RecentFiling {
            form: "8-K".into(),
            filing_date: "2026-08-04".into(),
            ..Default::default()
        }]);
        data.earnings = Ok(vec![SymbolEarningsRow {
            date: "2026-08-04".into(),
            eps_actual: Some(2.10),
            eps_estimated: Some(2.00),
            revenue_actual: None,
        }]);
        let state = run_quick_check(&data, &conn, &noop_ctx()).unwrap();
        let h = &state.holdings[0];
        assert!(
            h.evidence_events
                .iter()
                .any(|e| e.kind == EvidenceEventKind::MaterialFiling),
            "a filing on the pass's own ET day is visible: {:?}",
            h.evidence_events
        );
        assert!(
            h.evidence_events
                .iter()
                .any(|e| e.kind == EvidenceEventKind::EarningsActual),
            "an earnings actual on the pass's own ET day is visible: {:?}",
            h.evidence_events
        );
    }

    #[test]
    fn refuses_without_a_prior_run() {
        let conn = mem();
        let err = run_quick_check(&StubData::quiet(195.0, "2026-08-01"), &conn, &noop_ctx())
            .unwrap_err();
        assert!(err.to_string().contains("no Portfolio Analysis run"), "{err}");
    }

    #[test]
    fn the_sweep_price_pass_runs_under_its_own_step() {
        // Every request row needs an owning backend step: a row arriving while
        // nothing runs synthesizes the tracker's phantom never-finished
        // "Baseline market data" step (Finding 6's quick-check half — the
        // price pass fetches per symbol before the first per-holding step).
        let conn = mem();
        let run = sample_run(priced_verdict("AAPL", vec![]), audit_for("AAPL", None));
        store::record_run(&conn, &run).unwrap();
        let rec = std::sync::Arc::new(crate::progress::RecordingReporter::default());
        let ctx = crate::progress::RunContext::new(
            "run",
            rec.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        run_quick_check(&StubData::quiet(195.0, "2026-08-01"), &conn, &ctx).unwrap();
        let steps: Vec<(bool, String)> = rec
            .messages()
            .into_iter()
            .filter_map(|m| match m.event {
                crate::progress::ProgressEvent::StepStarted { step, .. } => Some((true, step)),
                crate::progress::ProgressEvent::StepFinished { step, .. } => Some((false, step)),
                _ => None,
            })
            .collect();
        let started = steps
            .iter()
            .position(|(s, k)| *s && k == "sweep-prices")
            .expect("the price pass opens its own step");
        let finished = steps
            .iter()
            .position(|(s, k)| !*s && k == "sweep-prices")
            .expect("the price pass closes its step");
        let first_holding = steps
            .iter()
            .position(|(s, k)| *s && k.starts_with("holding-"))
            .expect("a per-holding step follows");
        assert!(started < finished, "{steps:?}");
        assert!(finished < first_holding, "{steps:?}");
    }

    #[test]
    fn an_unreadable_only_store_refuses_as_unreadable_not_never_ran() {
        // Rows that decode on none of the loud-skip passes are unreadable, not
        // absent — the refusal must not claim "no run exists" over persisted
        // (if corrupt) work (combined-range review).
        let conn = mem();
        conn.execute(
            "INSERT INTO portfolio_runs (run_id, created_at, run_json) \
             VALUES ('run-corrupt', '2026-08-11T00:00:00Z', 'not json')",
            [],
        )
        .unwrap();
        let err = run_quick_check(&StubData::quiet(195.0, "2026-08-01"), &conn, &noop_ctx())
            .unwrap_err();
        assert!(err.to_string().contains("could not be read"), "{err}");
        assert!(!err.to_string().contains("no Portfolio Analysis run exists"), "{err}");
    }

    #[test]
    fn a_market_breach_confirms_across_two_distinct_prints_and_flags() {
        let conn = mem();
        let verdict = priced_verdict(
            "AAPL",
            vec![price_condition("c1", ConditionRole::Falsifier, 180.0)],
        );
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(basis())))).unwrap();

        // Sweep 1: price 170 breaches (below 180) on print date A — market-data
        // cadence needs two distinct observations, so this is a quiet first breach.
        let s1 = run_quick_check(&StubData::quiet(170.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        let h1 = &s1.holdings[0];
        assert!(h1.flag.is_none(), "first breach never flags: {:?}", h1.flag);
        assert!(h1.notes.iter().any(|n| n.contains("first-breach")));
        let (_, st1) = h1
            .condition_states
            .iter()
            .find(|(id, _)| id == "c1")
            .expect("state persisted");
        assert_eq!(st1.breach_streak, 1);

        // Sweep 2 against the SAME print: no advance, still no flag.
        let s2 = run_quick_check(&StubData::quiet(170.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        let (_, st2) = s2.holdings[0]
            .condition_states
            .iter()
            .find(|(id, _)| id == "c1")
            .unwrap();
        assert_eq!(st2.breach_streak, 1, "same observation cannot advance");
        assert!(s2.holdings[0].flag.is_none());

        // Sweep 3 on a NEW print date, still breaching: confirmed → amber flag.
        let s3 = run_quick_check(&StubData::quiet(171.0, "2026-08-02"), &conn, &noop_ctx()).unwrap();
        let h3 = &s3.holdings[0];
        let flag = h3.flag.as_ref().expect("confirmed breach flags");
        assert_eq!(flag.trigger, FlagTrigger::ConfirmedFalsifierBreach);

        // Sweep 4 with a clean price: the flag persists (only a full pass clears).
        let s4 = run_quick_check(&StubData::quiet(200.0, "2026-08-03"), &conn, &noop_ctx()).unwrap();
        assert!(s4.holdings[0].flag.is_some(), "a clean sweep never clears the flag");
    }

    #[test]
    fn price_outside_the_frozen_band_flags() {
        let conn = mem();
        let verdict = priced_verdict("AAPL", vec![]);
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(basis())))).unwrap();
        // Band is [150, 260]; 140 sits below it.
        let s = run_quick_check(&StubData::quiet(140.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        let flag = s.holdings[0].flag.as_ref().expect("band exit flags");
        assert_eq!(flag.trigger, FlagTrigger::PriceOutsideBand);
        // In-band price does not flag (fresh store).
        store::clear_quick_check(&conn).unwrap();
        let s = run_quick_check(&StubData::quiet(200.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        assert!(s.holdings[0].flag.is_none());
    }

    #[test]
    fn authored_outside_band_flags_only_on_a_relation_change() {
        // A band authored with spot already outside it was an examined observation
        // (the model wrote the ledger seeing it): the standing state must not flag —
        // badging the holding on every selective run — sweep after sweep.
        let insert = |conn: &rusqlite::Connection, authored| {
            let mut verdict = priced_verdict("AAPL", vec![]);
            if let Some(l) = verdict.thesis_ledger.as_mut() {
                l.authored_band_relation = Some(authored);
            }
            store::insert_run(conn, &sample_run(verdict, audit_for("AAPL", Some(basis())))).unwrap();
        };

        // Band [150, 260] authored with spot below; 140 stays below — same
        // relation, no flag.
        let conn = mem();
        insert(&conn, crate::portfolio::BandRelation::BelowBand);
        let s = run_quick_check(&StubData::quiet(140.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        assert!(
            s.holdings[0].flag.is_none(),
            "the authored-outside standing state never flags"
        );

        // Re-entering the band IS a relation change: flags.
        let s = run_quick_check(&StubData::quiet(200.0, "2026-08-02"), &conn, &noop_ctx()).unwrap();
        let flag = s.holdings[0].flag.as_ref().expect("band re-entry flags");
        assert_eq!(flag.trigger, FlagTrigger::PriceOutsideBand);
        assert!(flag.detail.contains("re-entered"));

        // Crossing to the other side of the band is too (authored above, now below).
        let conn = mem();
        insert(&conn, crate::portfolio::BandRelation::AboveBand);
        let s = run_quick_check(&StubData::quiet(140.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        let flag = s.holdings[0].flag.as_ref().expect("side cross flags");
        assert_eq!(flag.trigger, FlagTrigger::PriceOutsideBand);
        assert!(flag.detail.contains("other side"));
    }

    /// A stub whose fresh series sits on a 4:1-split basis relative to the
    /// stored audit (spot 195, anchor close 190 @ 2026-07-01): every fresh
    /// value is the old-basis print ÷ 4, and the fresh consensus is restated
    /// the way the provider restates it.
    fn split_stub(price: f64, close_date: &str) -> StubData {
        let mut data = StubData::quiet(price, close_date);
        data.price = Ok((
            price,
            vec![
                DatedValue { date: "2026-07-01".into(), value: 47.5 },
                DatedValue { date: close_date.into(), value: price },
            ],
        ));
        data.consensus = Ok(Some(ConsensusEstimate {
            eps_mid: Some(1.625),
            eps_periods: vec![engine::ConsensusEpsPeriod {
                period_end: "2026-12-31".into(),
                eps_mid: Some(1.625),
                ntm_weight: 1.0,
            }],
            ..Default::default()
        }));
        data
    }

    fn audit_with_anchor(symbol: &str) -> HoldingAudit {
        let mut audit = audit_for(symbol, Some(basis()));
        audit.authoring_close =
            Some(DatedValue { date: "2026-07-01".into(), value: 190.0 });
        audit
    }

    #[test]
    fn a_split_rebasis_never_fabricates_a_breach_or_band_exit() {
        let conn = mem();
        // Old-basis falsifiers: price below 180, P/E below 20 (stored P/E 30 at
        // spot 195). A 4:1 split (bridge 47.5 ⁄ 190 = 0.25) makes every fresh
        // print read as a breach without the bridge: 48 < 180, and the unbridged
        // rescale 30 × 48 ⁄ 195 ≈ 7.4 < 20.
        let mut pe = price_condition("c-pe", ConditionRole::Falsifier, 20.0);
        pe.quant.as_mut().unwrap().series = LedgerSeries::PeRatio;
        pe.statement = "P/E below 20".into();
        let verdict = priced_verdict(
            "AAPL",
            vec![price_condition("c-px", ConditionRole::Falsifier, 180.0), pe],
        );
        store::insert_run(&conn, &sample_run(verdict, audit_with_anchor("AAPL"))).unwrap();

        // Two sweeps on distinct prints — enough to confirm a market breach if
        // one were (wrongly) observed.
        let s1 = run_quick_check(&split_stub(48.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        let h1 = &s1.holdings[0];
        assert!(h1.flag.is_none(), "no false breach on sweep 1: {:?}", h1.flag);
        assert!(
            !h1.notes.iter().any(|n| n.contains("first-breach")),
            "no first-breach note either: {:?}",
            h1.notes
        );
        assert!(
            h1.notes.iter().any(|n| n.contains("re-based")),
            "the re-basis is noted: {:?}",
            h1.notes
        );
        let s2 = run_quick_check(&split_stub(48.2, "2026-08-02"), &conn, &noop_ctx()).unwrap();
        let h2 = &s2.holdings[0];
        assert!(h2.flag.is_none(), "no false breach on sweep 2: {:?}", h2.flag);
        for (id, st) in &h2.condition_states {
            assert_eq!(st.breach_streak, 0, "clean streak on {id}");
        }
        // The revision comparator bridged too: stored 6.5 × 0.25 = 1.625 vs the
        // restated fresh 1.625 — no fabricated revision-move event.
        assert!(
            h2.evidence_events.is_empty(),
            "no fabricated evidence events: {:?}",
            h2.evidence_events
        );
    }

    #[test]
    fn a_genuine_post_split_breach_still_confirms() {
        let conn = mem();
        let verdict = priced_verdict(
            "AAPL",
            vec![price_condition("c-px", ConditionRole::Falsifier, 180.0)],
        );
        store::insert_run(&conn, &sample_run(verdict, audit_with_anchor("AAPL"))).unwrap();

        // The bridged threshold is 180 × 0.25 = 45: prints at 44 breach for real.
        let s1 = run_quick_check(&split_stub(44.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        assert!(s1.holdings[0].flag.is_none(), "first breach is quiet");
        assert!(s1.holdings[0].notes.iter().any(|n| n.contains("first-breach")));
        let s2 = run_quick_check(&split_stub(44.5, "2026-08-02"), &conn, &noop_ctx()).unwrap();
        let flag = s2.holdings[0].flag.as_ref().expect("a real breach still confirms");
        assert_eq!(flag.trigger, FlagTrigger::ConfirmedFalsifierBreach);
    }

    #[test]
    fn an_unresolvable_bridge_excludes_price_comparisons_not_runs_them() {
        let conn = mem();
        let verdict = priced_verdict(
            "AAPL",
            vec![price_condition("c-px", ConditionRole::Falsifier, 180.0)],
        );
        // An anchor whose bar date is absent from the fresh window: the basis is
        // unverifiable — the comparison must be excluded, never run cross-basis.
        let mut audit = audit_for("AAPL", Some(basis()));
        audit.authoring_close =
            Some(DatedValue { date: "2026-06-15".into(), value: 190.0 });
        store::insert_run(&conn, &sample_run(verdict, audit)).unwrap();

        let s = run_quick_check(&split_stub(48.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        assert!(h.flag.is_none(), "no cross-basis breach: {:?}", h.flag);
        assert!(
            h.notes.iter().any(|n| n.contains("excluded")),
            "the exclusion is noted: {:?}",
            h.notes
        );
        // The market family cannot vouch this sweep.
        let market = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::MarketData)
            .expect("market family present");
        assert_eq!(market.state, SweepState::Unknown, "{market:?}");
        // Neither can the revision family: its stored comparator was excluded,
        // so a successful retrieval must not read `fresh_clear` through it.
        let revision = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::Revision)
            .expect("revision family present");
        assert_eq!(revision.state, SweepState::Unknown, "{revision:?}");
    }

    #[test]
    fn an_unresolvable_bridge_downgrades_a_band_only_market_family() {
        // No quantitative conditions at all — only the frozen band. The skipped
        // band read must still downgrade the market family, or a band-only
        // holding would read `fresh_clear` through a leg the basis excluded.
        let conn = mem();
        let verdict = priced_verdict("AAPL", vec![]);
        let mut audit = audit_for("AAPL", Some(basis()));
        audit.authoring_close =
            Some(DatedValue { date: "2026-06-15".into(), value: 190.0 });
        store::insert_run(&conn, &sample_run(verdict, audit)).unwrap();

        let s = run_quick_check(&split_stub(48.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        assert!(h.flag.is_none(), "no cross-basis band flag: {:?}", h.flag);
        let market = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::MarketData)
            .expect("market family present");
        assert_eq!(market.state, SweepState::Unknown, "{market:?}");
        // The note names the actual cause — with zero quantitative conditions,
        // the generic condition-could-not-resolve wording would misdescribe it.
        assert!(
            market
                .note
                .as_deref()
                .is_some_and(|n| n.contains("price basis unverifiable")),
            "{market:?}"
        );
    }

    #[test]
    fn a_withheld_comparator_row_reads_unknown_not_fresh_clear() {
        // The full-pass-output → quick-check seam: an unresolvable full pass
        // carries its anchor, withholds the quick basis, and stamps the monitor
        // target-less. The next sweep must read the affected families `unknown`
        // — never a silent `fresh_clear` vouch through legs that don't exist —
        // while the carried-verbatim price core still evaluates correctly
        // through the carried anchor.
        let conn = mem();
        let mut verdict = priced_verdict(
            "AAPL",
            vec![price_condition("c-px", ConditionRole::Falsifier, 180.0)],
        );
        if let Some(l) = verdict.thesis_ledger.as_mut() {
            for m in &mut l.monitor {
                m.engine_target = None;
            }
        }
        let mut audit = audit_for("AAPL", None);
        audit.authoring_close =
            Some(DatedValue { date: "2026-07-01".into(), value: 190.0 });
        store::insert_run(&conn, &sample_run(verdict, audit)).unwrap();

        let s = run_quick_check(&split_stub(48.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        assert!(h.flag.is_none(), "no false flag: {:?}", h.flag);
        // The old-basis core converts through the carried anchor (180 × 0.25 =
        // 45; 48 is clean) — evaluable, not excluded.
        assert!(
            !h.notes.iter().any(|n| n.contains("first-breach")),
            "{:?}",
            h.notes
        );
        for fam in [
            SweepFamily::Revision,
            SweepFamily::MarketData,
            SweepFamily::RateAnchor,
        ] {
            let f = h
                .families
                .iter()
                .find(|f| f.family == fam)
                .unwrap_or_else(|| panic!("{fam:?} family present"));
            assert_eq!(f.state, SweepState::Unknown, "{f:?}");
            assert!(
                f.note.as_deref().is_some_and(|n| n.contains("withheld")),
                "the note names the withheld cause: {f:?}"
            );
        }
    }

    #[test]
    fn the_sweep_lookback_widens_to_reach_an_old_anchor() {
        assert_eq!(eod_lookback_for("2026-07-20", "2026-08-01"), QUICK_EOD_LOOKBACK_DAYS);
        // A vintage 400 days back needs the window widened past the floor.
        assert_eq!(eod_lookback_for("2025-06-27", "2026-08-01"), 400 + ANCHOR_FETCH_SLACK_DAYS);
        // Unparseable dates keep the floor.
        assert_eq!(eod_lookback_for("soon", "2026-08-01"), QUICK_EOD_LOOKBACK_DAYS);
    }

    #[test]
    fn widened_anchor_rows_stay_out_of_the_condition_price_window() {
        let conn = mem();
        let mut condition = price_condition("c-tr", ConditionRole::Falsifier, 0.50);
        condition.statement = "trailing return above 50%".into();
        let quant = condition.quant.as_mut().unwrap();
        quant.series = LedgerSeries::TrailingReturn;
        quant.comparator = LedgerComparator::Above;

        let verdict = priced_verdict("AAPL", vec![condition]);
        let mut audit = audit_for("AAPL", Some(basis()));
        audit.authoring_close =
            Some(DatedValue { date: "2025-06-01".into(), value: 10.0 });
        store::insert_run(&conn, &sample_run(verdict, audit)).unwrap();

        let mut data = StubData::quiet(195.0, "2026-08-01");
        data.price = Ok((
            195.0,
            vec![
                // Needed to recover the carried split anchor, but outside the
                // full pass's inclusive 180-day EOD range.
                DatedValue { date: "2025-06-01".into(), value: 10.0 },
                DatedValue { date: "2026-02-03".into(), value: 195.0 },
                DatedValue { date: "2026-08-01".into(), value: 195.0 },
            ],
        ));
        let state = run_quick_check_at(
            &data,
            &conn,
            &noop_ctx(),
            "2026-08-01T12:00:00Z".into(),
        )
        .unwrap();
        let condition = state.holdings[0]
            .condition_states
            .iter()
            .find(|(id, _)| id == "c-tr")
            .expect("trailing-return condition evaluated");
        assert_eq!(condition.1.last_value, Some(0.0));
        assert_eq!(condition.1.breach_streak, 0);
        assert!(
            state.holdings[0].flag.is_none(),
            "the anchor-only 900% return must not enter the condition read"
        );
    }

    #[test]
    fn the_v1_to_v3_boundary_retires_only_affected_quick_state() {
        let mut trailing = price_condition("c-tr", ConditionRole::Falsifier, 0.50);
        trailing.statement = "trailing return above 50%".into();
        trailing.quant.as_mut().unwrap().series = LedgerSeries::TrailingReturn;
        let price = price_condition("c-price", ConditionRole::Falsifier, 180.0);
        let run = sample_run(
            priced_verdict("AAPL", vec![trailing, price]),
            audit_for("AAPL", Some(basis())),
        );
        let event = EvidenceEvent {
            kind: EvidenceEventKind::EarningsActual,
            detail: "earnings actual posted".into(),
            observed_at: "2026-08-01T12:00:00Z".into(),
        };
        let legacy_revision = EvidenceEvent {
            kind: EvidenceEventKind::RevisionMove,
            detail: "rolling NTM moved".into(),
            observed_at: "2026-08-01T12:00:00Z".into(),
        };
        let mut state = QuickCheckState {
            parameter_version: "quick-check-v1".into(),
            swept_run_id: run.run_id.clone(),
            last_checked_at: "2026-08-01T12:00:00Z".into(),
            rate_cache: None,
            holdings: vec![HoldingQuickState {
                symbol: "AAPL".into(),
                families: vec![],
                flag: Some(AttentionFlag {
                    trigger: FlagTrigger::ConfirmedFalsifierBreach,
                    detail: "confirmed falsifier breach: trailing return above 50%".into(),
                    raised_at: "2026-08-01T12:00:00Z".into(),
                }),
                evidence_events: vec![event.clone(), legacy_revision],
                condition_states: vec![
                    ("c-tr".into(), ConditionEvalState::default()),
                    ("c-price".into(), ConditionEvalState::default()),
                ],
                last_hurdle_state: Some(HurdleState::Clears),
                notes: vec![],
            }],
        };

        reconcile_parameter_version(&mut state, &run);

        assert_eq!(state.parameter_version, QUICK_CHECK_PARAMETER_VERSION);
        assert_eq!(
            state.holdings[0]
                .condition_states
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            ["c-price"]
        );
        assert!(state.holdings[0].flag.is_none());
        assert_eq!(state.holdings[0].evidence_events, vec![event]);
        assert_eq!(state.holdings[0].last_hurdle_state, Some(HurdleState::Clears));
    }

    #[test]
    fn the_v2_to_v3_boundary_retires_only_rolling_revision_events() {
        let run = sample_run(
            priced_verdict("AAPL", vec![]),
            audit_for("AAPL", Some(basis())),
        );
        let keep = EvidenceEvent {
            kind: EvidenceEventKind::EarningsActual,
            detail: "earnings actual posted".into(),
            observed_at: "2026-08-01T12:00:00Z".into(),
        };
        let mut state = QuickCheckState {
            parameter_version: "quick-check-v2".into(),
            swept_run_id: run.run_id.clone(),
            last_checked_at: "2026-08-01T12:00:00Z".into(),
            rate_cache: None,
            holdings: vec![HoldingQuickState {
                symbol: "AAPL".into(),
                families: vec![],
                flag: Some(AttentionFlag {
                    trigger: FlagTrigger::PriceOutsideBand,
                    detail: "price crossed the frozen band".into(),
                    raised_at: "2026-08-01T12:00:00Z".into(),
                }),
                evidence_events: vec![
                    keep.clone(),
                    EvidenceEvent {
                        kind: EvidenceEventKind::RevisionMove,
                        detail: "rolling NTM moved".into(),
                        observed_at: "2026-08-01T12:00:00Z".into(),
                    },
                ],
                condition_states: vec![("c-price".into(), ConditionEvalState::default())],
                last_hurdle_state: Some(HurdleState::Clears),
                notes: vec!["durable note".into()],
            }],
        };

        reconcile_parameter_version(&mut state, &run);

        assert_eq!(state.parameter_version, QUICK_CHECK_PARAMETER_VERSION);
        assert_eq!(state.holdings[0].evidence_events, vec![keep]);
        assert_eq!(state.holdings[0].condition_states.len(), 1);
        assert_eq!(state.holdings[0].flag.as_ref().unwrap().trigger, FlagTrigger::PriceOutsideBand);
        assert_eq!(state.holdings[0].notes, vec!["durable note"]);
    }

    #[test]
    fn a_tail_sweep_uses_the_parent_runs_pinned_session() {
        let verdict = priced_verdict(
            "AAPL",
            vec![price_condition("c-price", ConditionRole::Falsifier, 180.0)],
        );
        let run = sample_run(verdict, audit_for("AAPL", Some(basis())));
        let tail = std::collections::HashSet::from(["AAPL".to_string()]);
        let data = StubData::quiet(170.0, "2026-08-01");
        let state = sweep_tail(
            TailSweep {
                data: &data,
                prior_run: &run,
                current_positions: &run.holdings.positions,
                tail: &tail,
                prior_state: None,
                rates: run.rate_prints.clone(),
                // 02:59 UTC is still the prior ET session in daylight time.
                // A separate clock read one minute later could cross 8 PM ET.
                run_instant: "2026-08-02T02:59:59Z",
            },
            &noop_ctx(),
        )
        .unwrap();
        let condition = state[0]
            .condition_states
            .iter()
            .find(|(id, _)| id == "c-price")
            .expect("price condition evaluated");
        assert_eq!(condition.1.last_evaluated_at.as_deref(), Some("2026-08-01"));
        assert_eq!(condition.1.first_breach_at.as_deref(), Some("2026-08-01"));
    }

    #[test]
    fn hurdle_newly_failing_flags_and_indeterminate_never_does() {
        let conn = mem();
        let verdict = priced_verdict("AAPL", vec![]);
        // A basis whose re-anchored targets sit far below a very high fresh price:
        // TR deeply negative → fails even at the bull leg.
        let mut b = basis();
        b.spread_percentiles = None;
        b.raw_percentiles = Some([20.0, 22.0, 24.0]); // targets ≈ 120–168
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(b)))).unwrap();
        // Price 240 stays inside the fixture band [150, 260], so only the hurdle
        // trigger can fire; prior dead_money is Indeterminate → newly fails.
        let s = run_quick_check(&StubData::quiet(240.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        let flag = s.holdings[0].flag.as_ref().expect("newly-fails flags");
        assert_eq!(flag.trigger, FlagTrigger::HurdleNewlyFails);
        assert!(flag.detail.contains("TR bull"), "{}", flag.detail);
        assert!(!flag.detail.contains("TR base"), "{}", flag.detail);
        assert_eq!(s.holdings[0].last_hurdle_state, Some(HurdleState::Fails));
    }

    #[test]
    fn failed_price_and_missing_cik_read_unknown_never_clear() {
        let conn = mem();
        let verdict = priced_verdict(
            "AAPL",
            vec![price_condition("c1", ConditionRole::Falsifier, 180.0)],
        );
        store::insert_run(&conn, &sample_run(verdict, audit_with_anchor("AAPL"))).unwrap();
        let mut stub = StubData::quiet(170.0, "2026-08-01");
        stub.price = Err("quote gate".into());
        stub.filings = FilingSweep::NoCik;
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        let state_of = |fam: SweepFamily| {
            h.families
                .iter()
                .find(|f| f.family == fam)
                .map(|f| f.state)
        };
        assert_eq!(state_of(SweepFamily::MarketData), Some(SweepState::Unknown));
        assert_eq!(state_of(SweepFamily::Filing), Some(SweepState::Unknown));
        assert!(
            h.families
                .iter()
                .filter_map(|f| f.note.as_deref())
                .all(|n| !n.contains("split-bridge anchor")),
            "an outage must not be blamed on the anchor: {:?}",
            h.families
        );
        assert!(
            h.families
                .iter()
                .any(|f| f.note.as_deref().is_some_and(|n| n.contains("price refresh failed"))),
            "the actual quote failure remains visible: {:?}",
            h.families
        );
        // No price → the breach condition was not evaluated at all.
        assert!(h.condition_states.is_empty());
        assert!(h.flag.is_none());
    }

    #[test]
    fn rate_failure_falls_to_a_fresh_cache_and_unknown_past_the_max_age() {
        let conn = mem();
        let verdict = priced_verdict("AAPL", vec![]);
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(basis())))).unwrap();
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.rates = Err("FRED down".into());
        // The run's cached prints are dated 2026-07-18 — older than 7 days from
        // today, so the cache is ineligible and the rate family reads unknown.
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let rate_family = s.holdings[0]
            .families
            .iter()
            .find(|f| f.family == SweepFamily::RateAnchor)
            .expect("priced holding sweeps the rate family");
        assert_eq!(rate_family.state, SweepState::Unknown);
    }

    #[test]
    fn evidence_events_accumulate_without_flagging() {
        let conn = mem();
        let verdict = priced_verdict("AAPL", vec![]);
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(basis())))).unwrap();
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.earnings = Ok(vec![SymbolEarningsRow {
            date: "2026-07-30".into(),
            eps_actual: Some(1.61),
            eps_estimated: Some(1.55),
            revenue_actual: Some(96.0e9),
        }]);
        stub.filings = FilingSweep::Filings(vec![
            RecentFiling { form: "4".into(), filing_date: "2026-07-31".into(), ..Default::default() },
            RecentFiling { form: "10-Q".into(), filing_date: "2026-07-30".into(), ..Default::default() },
        ]);
        stub.consensus = Ok(Some(ConsensusEstimate {
            eps_mid: Some(7.2), // vs stored 6.5 → > 5% move
            eps_periods: vec![engine::ConsensusEpsPeriod {
                period_end: "2026-12-31".into(),
                eps_mid: Some(7.2),
                ntm_weight: 1.0,
            }],
            ..Default::default()
        }));
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        let kinds: Vec<EvidenceEventKind> = h.evidence_events.iter().map(|e| e.kind).collect();
        assert!(kinds.contains(&EvidenceEventKind::EarningsActual));
        assert!(kinds.contains(&EvidenceEventKind::MaterialFiling));
        assert!(kinds.contains(&EvidenceEventKind::RevisionMove));
        // Events are the quiet badge, never the amber flag.
        assert!(h.flag.is_none());
        // A second identical sweep does not duplicate the events.
        let s2 = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        assert_eq!(s2.holdings[0].evidence_events.len(), h.evidence_events.len());
    }

    #[test]
    fn revision_preflight_ignores_calendar_roll_and_compares_the_surviving_period() {
        let conn = mem();
        let verdict = priced_verdict("AAPL", vec![]);
        let mut stored = basis();
        stored.consensus_eps_mid = Some(5.5);
        stored.consensus_eps_periods = vec![
            engine::ConsensusEpsPeriod {
                period_end: "2026-12-31".into(),
                eps_mid: Some(5.0),
                ntm_weight: 0.5,
            },
            engine::ConsensusEpsPeriod {
                period_end: "2027-12-31".into(),
                eps_mid: Some(6.0),
                ntm_weight: 0.5,
            },
        ];
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(stored)))).unwrap();

        // Rolling NTM rises 5.5 → 6.5 solely because 2026 rolled out and 2028
        // rolled in. The shared 2027 estimate is unchanged, so no event exists.
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.consensus = Ok(Some(ConsensusEstimate {
            eps_mid: Some(6.5),
            eps_periods: vec![
                engine::ConsensusEpsPeriod {
                    period_end: "2027-12-31".into(),
                    eps_mid: Some(6.0),
                    ntm_weight: 0.5,
                },
                engine::ConsensusEpsPeriod {
                    period_end: "2028-12-31".into(),
                    eps_mid: Some(7.0),
                    ntm_weight: 0.5,
                },
            ],
            ..Default::default()
        }));
        let state = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        assert!(!state.holdings[0]
            .evidence_events
            .iter()
            .any(|event| event.kind == EvidenceEventKind::RevisionMove));
        let revision = state.holdings[0]
            .families
            .iter()
            .find(|family| family.family == SweepFamily::Revision)
            .unwrap();
        assert_eq!(revision.state, SweepState::FreshClear);

        // A genuine change to the surviving fiscal period still fires.
        stub.consensus.as_mut().unwrap().as_mut().unwrap().eps_periods[0].eps_mid = Some(6.5);
        let state = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        assert!(state.holdings[0]
            .evidence_events
            .iter()
            .any(|event| event.kind == EvidenceEventKind::RevisionMove));
    }

    #[test]
    fn revision_preflight_with_no_common_fiscal_period_is_unknown_not_clear() {
        let conn = mem();
        store::insert_run(
            &conn,
            &sample_run(
                priced_verdict("AAPL", vec![]),
                audit_for("AAPL", Some(basis())),
            ),
        )
        .unwrap();
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.consensus.as_mut().unwrap().as_mut().unwrap().eps_periods[0].period_end =
            "2028-12-31".into();
        let state = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let revision = state.holdings[0]
            .families
            .iter()
            .find(|family| family.family == SweepFamily::Revision)
            .unwrap();
        assert_eq!(revision.state, SweepState::Unknown);
        assert!(revision.note.as_deref().unwrap().contains("no usable common"));
    }

    #[test]
    fn a_new_filing_evaluates_filing_conditions_with_fresh_statement_values() {
        let conn = mem();
        let mut cond = price_condition("c-margin", ConditionRole::Falsifier, 0.20);
        cond.statement = "net margin below 20%".into();
        cond.quant = Some(QuantCore {
            series: LedgerSeries::NetMargin,
            comparator: LedgerComparator::Below,
            threshold: 0.20,
            margin: 0.0,
        });
        let verdict = priced_verdict("AAPL", vec![cond]);
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(basis())))).unwrap();

        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.filings = FilingSweep::Filings(vec![RecentFiling {
            form: "10-Q".into(),
            filing_date: "2026-07-30".into(),
            ..Default::default()
        }]);
        // Fresh statements: 10% net margin — breaches the 20% floor; filing
        // cadence confirms on the first qualifying print.
        // Real quarterly spacing — the statement windows are contiguity-gated,
        // so a synthetic monthly run would (correctly) fail TTM adoption.
        let quarter_ends = ["2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30"];
        stub.statements = CompanyFinancials {
            symbol: "AAPL".into(),
            quarterly_income: (0..4)
                .map(|i| engine::QuarterlyIncomeRow {
                    period_end: quarter_ends[i].to_string(),
                    filing_date: None,
                    revenue: Some(100.0),
                    eps_diluted: Some(1.0),
                    diluted_shares: Some(100.0),
                    net_income: Some(10.0),
                    gross_profit: Some(40.0),
                    cost_of_revenue: Some(60.0),
                    operating_income: None,
                })
                .collect(),
            ..Default::default()
        };
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        let flag = h.flag.as_ref().expect("filing-cadence breach confirms at count 1");
        assert_eq!(flag.trigger, FlagTrigger::ConfirmedFalsifierBreach);
        // Without a new filing the same condition is skipped whole (state carried).
        store::clear_quick_check(&conn).unwrap();
        let quiet = StubData::quiet(200.0, "2026-08-01");
        let s2 = run_quick_check(&quiet, &conn, &noop_ctx()).unwrap();
        assert!(s2.holdings[0]
            .condition_states
            .iter()
            .all(|(id, _)| id != "c-margin"));
    }

    /// Codex rounds 1–2 on group 4 (I13): the sweep's debt/equity reads FMP's
    /// quarterly balance sheet, so a D/E condition whose streak was accumulated
    /// on SEC's annual equity is withheld — unevaluable, no flag, no state
    /// movement, the filing family `unknown` — where the fresh print would have
    /// confirmed a breach at count one off the source step alone; so is an
    /// unstamped one (authored on a surface with no equity leg), whose sweep
    /// confirmation would otherwise persist unstamped for the next full pass to
    /// adopt any source under; only the condition stamped with the sweep's own
    /// source evaluates.
    #[test]
    fn a_debt_equity_condition_not_stamped_with_the_sweeps_source_is_withheld_not_confirmed() {
        use crate::portfolio::{ConditionEvalState, EquitySource};
        let quarter_ends = ["2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30"];
        let statements = || CompanyFinancials {
            symbol: "AAPL".into(),
            quarterly_income: (0..4)
                .map(|i| engine::QuarterlyIncomeRow {
                    period_end: quarter_ends[i].to_string(),
                    filing_date: None,
                    revenue: Some(100.0),
                    eps_diluted: Some(1.0),
                    diluted_shares: Some(100.0),
                    net_income: Some(10.0),
                    gross_profit: Some(40.0),
                    cost_of_revenue: Some(60.0),
                    operating_income: None,
                })
                .collect(),
            // Levered 3× on FMP's quarter-end equity — breaches "above 2".
            total_debt: Some(300.0),
            total_equity: Some(100.0),
            ..Default::default()
        };
        let sweep = |source: Option<EquitySource>| {
            let conn = mem();
            let mut cond = price_condition("c-de", ConditionRole::Falsifier, 2.0);
            cond.statement = "debt/equity above 2".into();
            cond.quant = Some(QuantCore {
                series: LedgerSeries::DebtToEquity,
                comparator: LedgerComparator::Above,
                threshold: 2.0,
                margin: 0.0,
            });
            cond.eval_state = Some(ConditionEvalState {
                authored_equity_source: source,
                ..Default::default()
            });
            let verdict = priced_verdict("AAPL", vec![cond]);
            store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(basis()))))
                .unwrap();
            let mut stub = StubData::quiet(200.0, "2026-08-01");
            stub.filings = FilingSweep::Filings(vec![RecentFiling {
                form: "10-Q".into(),
                filing_date: "2026-07-30".into(),
                ..Default::default()
            }]);
            stub.statements = statements();
            let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
            s.holdings.into_iter().next().unwrap()
        };

        // Accumulated on SEC's annual equity, or never stamped: withheld whole,
        // each with its own note.
        for (source, note) in [
            (
                Some(EquitySource::SecAnnual),
                "debt/equity streak accumulated on SEC's latest annual",
            ),
            (None, "debt/equity streak carries no equity-source stamp"),
        ] {
            let h = sweep(source);
            assert!(h.flag.is_none(), "{source:?}: {:?}", h.flag);
            assert!(
                h.notes
                    .iter()
                    .any(|n| n.contains("unevaluable this sweep") && n.contains(note)),
                "{source:?}: {:?}",
                h.notes
            );
            assert!(
                h.condition_states.iter().all(|(id, _)| id != "c-de"),
                "{source:?}: no state movement: {:?}",
                h.condition_states
            );
            let fam = h
                .families
                .iter()
                .find(|f| f.family == SweepFamily::Filing)
                .unwrap();
            assert_eq!(fam.state, SweepState::Unknown, "{source:?}");
        }

        // Stamped with the sweep's own source: evaluates, and the filing-cadence
        // breach confirms at count one.
        let h = sweep(Some(EquitySource::FmpQuarterly));
        let flag = h.flag.as_ref().expect("filing-cadence breach confirms");
        assert_eq!(flag.trigger, FlagTrigger::ConfirmedFalsifierBreach);
        assert!(
            h.notes.iter().all(|n| !n.contains("debt/equity streak")),
            "{:?}",
            h.notes
        );
    }

    #[test]
    fn fund_info_change_and_exposure_shift_read_from_the_stored_comparators() {
        let conn = mem();
        // A role-risk fund with a standing expense-ratio condition.
        let mut fund_ledger = ledger(vec![]);
        fund_ledger.branch = LedgerBranch::RoleRiskOnly;
        for m in &mut fund_ledger.monitor {
            m.engine_target = None;
        }
        fund_ledger.conditions = vec![LedgerCondition {
            condition_id: "c-exp".into(),
            role: ConditionRole::Falsifier,
            trigger_family: None,
            statement: "expense ratio above 20 bps".into(),
            quant: Some(QuantCore {
                series: LedgerSeries::ExpenseRatio,
                comparator: LedgerComparator::Above,
                threshold: 0.002,
                margin: 0.0,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: None,
        }];
        let verdict = HoldingVerdict {
            symbol: "BONDX".into(),
            asset_class: AssetClass::MutualFund,
            position_change: Default::default(),
            disposition: VerdictDisposition::RoleRiskOnly(Box::new(
                crate::portfolio::RoleRiskVerdict {
                    class_label: "US equity fund".into(),
                    role_summary: "fixture".into(),
                    exposure_tilt: vec![],
                    expense_drag: Some(0.001),
                    observable_risk: None,
                    structural_flag: false,
                    is_cef: false,
                    nav_premium: None,
                    evidence_gaps: vec![],
                    action: crate::portfolio::Action::Hold,
                    action_rationale: String::new(),
                    what_changed: "fixture".into(),
                },
            )),
            thesis_ledger: Some(fund_ledger),
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        };
        let mut audit = audit_for("BONDX", None);
        audit.fund_exposure = Some(fund::FundExposureBasis {
            class_label: "US equity fund".into(),
            expense_ratio: Some(0.001),
            us_share: Some(0.75),
            top_sector: Some(("Technology".into(), 0.30)),
            structural_flag: false,
        });
        let mut run = sample_run(verdict, audit);
        run.holdings.positions[0].asset_class = AssetClass::MutualFund;
        store::insert_run(&conn, &run).unwrap();

        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.fund = FundData {
            symbol: "BONDX".into(),
            name: Some("Fixture Fund".into()),
            asset_class: Some("Equity".into()),
            expense_ratio: Some(0.003), // moved AND breaches the condition
            aum: None,
            nav: None,
            sector_weights: vec![("Technology".into(), 0.45)], // +15 pts
            country_weights: vec![("United States".into(), 0.60)], // crossed below 70%
            profile_is_fund: None,
            profile_description: None,
            gaps: vec![],
        };
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        let kinds: Vec<EvidenceEventKind> = h.evidence_events.iter().map(|e| e.kind).collect();
        assert!(kinds.contains(&EvidenceEventKind::FundInfoChange));
        assert!(kinds.contains(&EvidenceEventKind::ExposureShift));
        // The expense-ratio condition (filing cadence, value-keyed identity)
        // confirms on the changed print and flags.
        let flag = h.flag.as_ref().expect("expense breach flags");
        assert_eq!(flag.trigger, FlagTrigger::ConfirmedFalsifierBreach);
        // A repeat sweep against the SAME print cannot re-advance (value-keyed).
        let s2 = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let (_, st) = s2.holdings[0]
            .condition_states
            .iter()
            .find(|(id, _)| id == "c-exp")
            .unwrap();
        assert_eq!(st.breach_streak, 1);
    }

    /// The BONDX role-risk fund fixture with a parameterized stored exposure basis.
    fn fund_run(exposure: Option<fund::FundExposureBasis>) -> PortfolioRun {
        let mut fund_ledger = ledger(vec![]);
        fund_ledger.branch = LedgerBranch::RoleRiskOnly;
        for m in &mut fund_ledger.monitor {
            m.engine_target = None;
        }
        let verdict = HoldingVerdict {
            symbol: "BONDX".into(),
            asset_class: AssetClass::MutualFund,
            position_change: Default::default(),
            disposition: VerdictDisposition::RoleRiskOnly(Box::new(
                crate::portfolio::RoleRiskVerdict {
                    class_label: "US equity fund".into(),
                    role_summary: "fixture".into(),
                    exposure_tilt: vec![],
                    expense_drag: Some(0.001),
                    observable_risk: None,
                    structural_flag: false,
                    is_cef: false,
                    nav_premium: None,
                    evidence_gaps: vec![],
                    action: crate::portfolio::Action::Hold,
                    action_rationale: String::new(),
                    what_changed: "fixture".into(),
                },
            )),
            thesis_ledger: Some(fund_ledger),
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        };
        let mut audit = audit_for("BONDX", None);
        audit.fund_exposure = exposure;
        let mut run = sample_run(verdict, audit);
        run.holdings.positions[0].asset_class = AssetClass::MutualFund;
        run
    }

    #[test]
    fn a_partial_fund_refresh_degrades_to_unknown_never_fabricating_change_events() {
        let conn = mem();
        store::insert_run(
            &conn,
            &fund_run(Some(fund::FundExposureBasis {
                class_label: "US equity fund".into(),
                expense_ratio: Some(0.001),
                us_share: Some(0.75),
                top_sector: Some(("Technology".into(), 0.30)),
                structural_flag: false,
            })),
        )
        .unwrap();

        // Weightings failed: the fresh derivation reads "equity fund without
        // usable weightings" — retrieval damage, not a mandate change — and the
        // stored top sector / US share have no fresh side. No event may fire.
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.fund = FundData {
            symbol: "BONDX".into(),
            name: Some("Fixture Fund".into()),
            asset_class: Some("Equity".into()),
            expense_ratio: Some(0.001), // unchanged
            aum: None,
            nav: None,
            sector_weights: vec![],
            country_weights: vec![],
            profile_is_fund: None,
            profile_description: None,
            gaps: vec!["FMP weightings unavailable (transport)".into()],
        };
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        assert!(h.evidence_events.is_empty(), "{:?}", h.evidence_events);
        assert!(h.flag.is_none());
        let fam = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::FundInfo)
            .unwrap();
        assert_eq!(fam.state, SweepState::Unknown);
    }

    #[test]
    fn a_non_equity_fund_is_not_degraded_by_its_empty_equity_weightings() {
        // A bond fund's empty equity weightings are its expected shape — the
        // weightings legs bear on equity funds alone, so the recorded endpoint
        // gaps must not read the family unknown (which would badge the
        // fund on every selective run forever).
        let conn = mem();
        store::insert_run(
            &conn,
            &fund_run(Some(fund::FundExposureBasis {
                class_label: "bond fund".into(),
                expense_ratio: Some(0.001),
                us_share: None,
                top_sector: None,
                structural_flag: false,
            })),
        )
        .unwrap();
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.fund = FundData {
            symbol: "BONDX".into(),
            name: Some("Fixture Bond Fund".into()),
            asset_class: Some("Fixed Income".into()),
            expense_ratio: Some(0.001),
            aum: None,
            nav: None,
            sector_weights: vec![],
            country_weights: vec![],
            profile_is_fund: None,
            profile_description: None,
            gaps: vec![
                format!("{} were empty", crate::fmp::FUND_SECTOR_WEIGHTS_GAP_PREFIX),
                format!("{} were empty", crate::fmp::FUND_COUNTRY_WEIGHTS_GAP_PREFIX),
            ],
        };
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        assert!(h.evidence_events.is_empty(), "{:?}", h.evidence_events);
        let fam = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::FundInfo)
            .unwrap();
        assert_eq!(fam.state, SweepState::FreshClear, "{:?}", fam.note);
    }

    #[test]
    fn a_mandate_transition_fires_for_every_fund_even_with_degraded_weightings() {
        // A stored US equity fund freshly reports Fixed Income. The coarse
        // mandate family derives from `etf/info` alone, so the change event
        // must fire even though the (now empty) equity-weight endpoints record
        // gaps that keep the label-level comparison off.
        let conn = mem();
        store::insert_run(
            &conn,
            &fund_run(Some(fund::FundExposureBasis {
                class_label: "US equity fund".into(),
                expense_ratio: Some(0.001),
                us_share: Some(0.75),
                top_sector: Some(("Technology".into(), 0.30)),
                structural_flag: false,
            })),
        )
        .unwrap();
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.fund = FundData {
            symbol: "BONDX".into(),
            name: Some("Fixture Fund".into()),
            asset_class: Some("Fixed Income".into()),
            expense_ratio: Some(0.001),
            aum: None,
            nav: None,
            sector_weights: vec![],
            country_weights: vec![],
            profile_is_fund: None,
            profile_description: None,
            gaps: vec![
                format!("{} were empty", crate::fmp::FUND_SECTOR_WEIGHTS_GAP_PREFIX),
                format!("{} were empty", crate::fmp::FUND_COUNTRY_WEIGHTS_GAP_PREFIX),
            ],
        };
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        let kinds: Vec<EvidenceEventKind> = h.evidence_events.iter().map(|e| e.kind).collect();
        assert!(kinds.contains(&EvidenceEventKind::FundInfoChange), "{kinds:?}");
        // The degraded weighting legs still read the family unknown — the
        // transition sweep badges rather than silently clearing.
        let fam = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::FundInfo)
            .unwrap();
        assert_eq!(fam.state, SweepState::Unknown);
    }

    #[test]
    fn an_overlay_flag_transition_fires_the_change_event_with_no_label_change() {
        // A US equity fund newly reading covered-call: the structural flag flips
        // while the class label stays "US equity fund" — the every-fund contract
        // counts a structural-flag reclassification, so the event must fire off
        // the persisted flag, not the label.
        let conn = mem();
        store::insert_run(
            &conn,
            &fund_run(Some(fund::FundExposureBasis {
                class_label: "US equity fund".into(),
                expense_ratio: Some(0.001),
                us_share: Some(0.75),
                top_sector: Some(("Technology".into(), 0.30)),
                structural_flag: false,
            })),
        )
        .unwrap();
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.fund = FundData {
            symbol: "BONDX".into(),
            name: Some("Fixture Covered Call ETF".into()),
            asset_class: Some("Equity".into()),
            expense_ratio: Some(0.001),
            aum: None,
            nav: None,
            sector_weights: vec![("Technology".into(), 0.30)],
            country_weights: vec![("United States".into(), 0.75)],
            profile_is_fund: None,
            profile_description: None,
            gaps: vec![],
        };
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        let change = h
            .evidence_events
            .iter()
            .find(|e| e.kind == EvidenceEventKind::FundInfoChange)
            .expect("the flag transition fires the change event");
        assert!(change.detail.contains("overlay flag false → true"), "{}", change.detail);
        // Clean data throughout — the family still vouches.
        let fam = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::FundInfo)
            .unwrap();
        assert_eq!(fam.state, SweepState::FreshClear, "{:?}", fam.note);
    }

    #[test]
    fn a_missing_asset_class_degrades_but_the_overlay_leg_still_reads_from_the_name() {
        // etf/info returns a usable name but no assetClass (which records no
        // gap): the mandate and label legs cannot be checked — the family
        // degrades — while the overlay flag, read from the name blob, still
        // detects a real covered-call transition.
        let conn = mem();
        store::insert_run(
            &conn,
            &fund_run(Some(fund::FundExposureBasis {
                class_label: "US equity fund".into(),
                expense_ratio: Some(0.001),
                us_share: Some(0.75),
                top_sector: Some(("Technology".into(), 0.30)),
                structural_flag: false,
            })),
        )
        .unwrap();
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.fund = FundData {
            symbol: "BONDX".into(),
            name: Some("Fixture Covered Call ETF".into()),
            asset_class: None,
            expense_ratio: Some(0.001),
            aum: None,
            nav: None,
            sector_weights: vec![("Technology".into(), 0.30)],
            country_weights: vec![("United States".into(), 0.75)],
            profile_is_fund: None,
            profile_description: None,
            gaps: vec![],
        };
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        let change = h
            .evidence_events
            .iter()
            .find(|e| e.kind == EvidenceEventKind::FundInfoChange)
            .expect("the flag transition fires off the name");
        assert!(
            change.detail.contains("overlay flag false → true"),
            "{}",
            change.detail
        );
        let fam = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::FundInfo)
            .unwrap();
        assert_eq!(fam.state, SweepState::Unknown);
        assert!(
            fam.note.as_ref().unwrap().contains("asset class unreadable"),
            "{:?}",
            fam.note
        );
    }


    #[test]
    fn an_unresolvable_filing_condition_downgrades_the_filing_family() {
        let conn = mem();
        let mut cond = price_condition("c-margin", ConditionRole::Falsifier, 0.20);
        cond.statement = "net margin below 20%".into();
        cond.quant = Some(QuantCore {
            series: LedgerSeries::NetMargin,
            comparator: LedgerComparator::Below,
            threshold: 0.20,
            margin: 0.0,
        });
        let verdict = priced_verdict("AAPL", vec![cond]);
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(basis()))))
            .unwrap();

        // A new filing whose re-pull returns only three quarters: the TTM basis
        // cannot form, so the margin condition is unresolvable — the filing
        // family must not claim a clear it could not check.
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.filings = FilingSweep::Filings(vec![RecentFiling {
            form: "10-Q".into(),
            filing_date: "2026-07-30".into(),
            ..Default::default()
        }]);
        stub.statements = CompanyFinancials {
            symbol: "AAPL".into(),
            quarterly_income: (0..3)
                .map(|i| engine::QuarterlyIncomeRow {
                    period_end: format!("2026-0{}-30", 6 - i),
                    filing_date: None,
                    revenue: Some(100.0),
                    eps_diluted: Some(1.0),
                    diluted_shares: Some(100.0),
                    net_income: Some(10.0),
                    gross_profit: Some(40.0),
                    cost_of_revenue: Some(60.0),
                    operating_income: None,
                })
                .collect(),
            ..Default::default()
        };
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        let filing = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::Filing)
            .unwrap();
        assert_eq!(filing.state, SweepState::Unknown, "{:?}", filing.note);
        assert!(h.flag.is_none());
        // The human note channel still names the specific condition.
        assert!(h.notes.iter().any(|n| n.contains("unevaluable")));
    }

    #[test]
    fn a_fund_without_a_stored_exposure_basis_reads_unknown_not_clear() {
        // A pre-basis run has no comparator: none of the fund change legs can be
        // evaluated, so the family must not claim a clear it never checked.
        let conn = mem();
        store::insert_run(&conn, &fund_run(None)).unwrap();
        let mut stub = StubData::quiet(200.0, "2026-08-01");
        stub.fund = FundData {
            symbol: "BONDX".into(),
            name: Some("Fixture Fund".into()),
            asset_class: Some("Equity".into()),
            expense_ratio: Some(0.001),
            aum: None,
            nav: None,
            sector_weights: vec![("Technology".into(), 0.30)],
            country_weights: vec![("United States".into(), 0.75)],
            profile_is_fund: None,
            profile_description: None,
            gaps: vec![],
        };
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let fam = s.holdings[0]
            .families
            .iter()
            .find(|f| f.family == SweepFamily::FundInfo)
            .cloned()
            .unwrap();
        assert_eq!(fam.state, SweepState::Unknown);
        assert!(fam.note.unwrap().contains("no stored exposure basis"));
        assert!(s.holdings[0].evidence_events.is_empty());
    }

    #[test]
    fn a_dividend_elimination_reaches_the_hurdle_but_a_failed_pull_keeps_the_stored_leg() {
        // Basis tuned so the payout leg decides the hurdle: targets 120/143/168
        // (drivers × raw percentiles), fresh price 160, hurdle 0.04 + 0.05 = 0.09.
        // Stored 26.0 payout → tr_bull = (168+26)/160 − 1 ≈ 0.21 (indeterminate);
        // eliminated to zero → tr_bull = 0.05 < 0.09 → newly fails.
        let dividend_basis = || {
            let mut b = basis();
            b.spread_percentiles = None;
            b.raw_percentiles = Some([20.0, 22.0, 24.0]);
            b.forward_dividends = 26.0;
            b
        };
        let filing_stub = || {
            let mut stub = StubData::quiet(160.0, "2026-08-01");
            stub.filings = FilingSweep::Filings(vec![RecentFiling {
                form: "10-Q".into(),
                filing_date: "2026-07-30".into(),
                ..Default::default()
            }]);
            stub.statements = CompanyFinancials {
                symbol: "AAPL".into(),
                quarterly_income: (0..4)
                    .map(|i| engine::QuarterlyIncomeRow {
                        period_end: format!("2026-0{}-30", 6 - i),
                        filing_date: None,
                        revenue: Some(100.0),
                        eps_diluted: Some(1.0),
                        diluted_shares: Some(100.0),
                        net_income: Some(10.0),
                        gross_profit: Some(40.0),
                        cost_of_revenue: Some(60.0),
                        operating_income: None,
                    })
                    .collect(),
                // `None` with no gap: the adapter's confirmed non-payer.
                ttm_dividends_per_share: None,
                ..Default::default()
            };
            stub
        };

        // A clean re-pull with no dividends is an elimination — the hurdle
        // newly fails on the zeroed payout leg.
        let conn = mem();
        let verdict = priced_verdict("AAPL", vec![]);
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(dividend_basis()))))
            .unwrap();
        let s = run_quick_check(&filing_stub(), &conn, &noop_ctx()).unwrap();
        let flag = s.holdings[0]
            .flag
            .as_ref()
            .expect("the elimination newly fails the hurdle");
        assert_eq!(flag.trigger, FlagTrigger::HurdleNewlyFails);

        // The same read with a recorded dividend gap is a failed retrieval —
        // the stored payout leg stands (no flag) and the filing family reads
        // `unknown` (the re-pull was incomplete).
        let conn = mem();
        let verdict = priced_verdict("AAPL", vec![]);
        store::insert_run(&conn, &sample_run(verdict, audit_for("AAPL", Some(dividend_basis()))))
            .unwrap();
        let mut stub = filing_stub();
        stub.statements.gaps =
            vec![format!("{} (transport)", crate::fmp::DIVIDENDS_GAP_PREFIX)];
        let s = run_quick_check(&stub, &conn, &noop_ctx()).unwrap();
        let h = &s.holdings[0];
        assert!(h.flag.is_none(), "the stored payout leg stands: {:?}", h.flag);
        let filing = h
            .families
            .iter()
            .find(|f| f.family == SweepFamily::Filing)
            .unwrap();
        assert_eq!(filing.state, SweepState::Unknown);
    }

    #[test]
    fn quick_state_round_trips_and_a_new_run_supersedes_it() {
        let conn = mem();
        let verdict = priced_verdict("AAPL", vec![]);
        store::insert_run(&conn, &sample_run(verdict.clone(), audit_for("AAPL", Some(basis()))))
            .unwrap();
        let s = run_quick_check(&StubData::quiet(200.0, "2026-08-01"), &conn, &noop_ctx()).unwrap();
        assert_eq!(store::latest_quick_check(&conn).unwrap().unwrap(), s);
        // A newer full run supersedes the stored sweep state wholesale.
        let mut newer = sample_run(verdict, audit_for("AAPL", Some(basis())));
        newer.run_id = "run-2".into();
        newer.created_at = "2026-08-02T00:00:00Z".into();
        store::insert_run(&conn, &newer).unwrap();
        let s2 = run_quick_check(&StubData::quiet(140.0, "2026-08-03"), &conn, &noop_ctx()).unwrap();
        assert_eq!(s2.swept_run_id, "run-2");
        // The band flag raised against run-2 is fresh state, not run-1 carry-over.
        assert!(s2.holdings[0].flag.is_some());
        store::clear_quick_check(&conn).unwrap();
        assert!(store::latest_quick_check(&conn).unwrap().is_none());
    }
}
