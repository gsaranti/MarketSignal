//! The per-holding pipeline (`docs/portfolio-analysis.md` §The per-holding pipeline).
//! Orchestrates one holding from its deterministic dossier through the engine to a
//! schema-valid verdict: eligibility → financial engine → bounded research → distill
//! → interpret + grade → continuity. The engine owns the baseline arm's numbers;
//! since `portfolio-v7` the model additionally authors its own arm — sub-scores,
//! target bands, the retrospective self-assessment — beside the judgment calls and
//! prose ([`crate::portfolio::Interpretation`]), model-arm judgment values never
//! altering or binding the engine baseline (the boundary statement:
//! `docs/portfolio-analysis.md` §The holding verdict).
//!
//! The model stages live behind the [`HoldingAnalyst`] trait so `cargo test` runs the
//! whole pipeline offline against [`StubAnalyst`] with no daemon, while the live
//! [`LocalAnalyst`] wraps [`crate::local_model::LocalModelClient`] with the
//! grammar-constrained `format` schema and the right thinking modes. The substrate is
//! a *primitive*; this is one of the per-feature stages that wraps it
//! (`docs/local-models.md`).
//!
//! Scope (this slice): the **web-research stage is stubbed** ([`research`]) — the
//! SearXNG-primary web tool is a later slice — so the pipeline shape is exercised
//! without pulling live web into an offline-validation slice.

use anyhow::{Context, Result};

use crate::local_model::{options, ChatMessage, ChatRequest, LocalModelClient, StreamRole};
use crate::portfolio::dossier::HoldingDossier;
use crate::portfolio::engine::{self, EngineOutput, EngineVerdict, LedgerEvaluation, RateAnchors};
use crate::portfolio::fund::{self, FundEngineVerdict, RoleRiskReadout};
use crate::portfolio::pre_profit::{self, PreProfitOverlay};
use crate::portfolio::{
    interpretation_schema, role_risk_interpretation_schema, Action, ActionSource, ClosedCondition,
    ConditionEvalState, ConditionRole, Conviction, CrossingOutcome, ExposureWeight,
    FalsifierDraft, GradedVerdict, HoldingAudit, HoldingVerdict, HorizonOutlook, HorizonRead,
    Interpretation, KeyDriver, KeyDriverDraft, LedgerAudit, LedgerBranch, LedgerCondition,
    LedgerComparator, LedgerDraft, ModelPriceTarget, ModelPriceTargets, ModelView,
    MonitorScenario, PositionChange, PositionDelta, PriceTarget,
    QuantCore, QuantCoreDraft, RoleRiskInterpretation, RoleRiskVerdict, ScenarioDraft, SubScores,
    ScenarioKind, ThesisLedger, TriggerDraft, TriggerFamily, VerdictDisposition, HORIZON_LONG,
    HORIZON_MID, HORIZON_SHORT, PROMPT_VERSION,
};

/// The condensed findings the research stage produces — the compact object the
/// interpretation reads, never a raw transcript (`docs/local-models.md §Context-memory
/// discipline`). Stubbed this slice; the live web loop fills it later.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResearchFindings {
    /// Sourced finding lines (claim + citation). Empty while research is stubbed.
    pub notes: Vec<String>,
    /// The source URLs/titles behind the notes, for the audit.
    pub sources: Vec<String>,
}

/// The bounded web-research stage (`docs/portfolio-analysis.md` step 3). **Stubbed**
/// this slice — it returns an explicit "research deferred" finding rather than
/// hitting the network, so the offline pipeline runs end to end. The real loop (the
/// 122B reasoner + the SearXNG web tool) replaces this without changing the
/// orchestration below.
pub fn research(_dossier: &HoldingDossier) -> ResearchFindings {
    ResearchFindings {
        notes: vec!["Web research deferred in this slice; grading on the deterministic \
                     financials and the Market Signal house view only."
            .to_string()],
        sources: Vec::new(),
    }
}

/// What the interpretation stage reads: the dossier, the engine's computed analysis,
/// and the distilled research findings. The model reasons over *this* — evidence,
/// not a gathering transcript. It carries **no investor profile and no action
/// machinery**: the intrinsic verdict is profile-independent by input isolation,
/// and the per-holding action call ([`ActionInput`]) is where both live
/// (`docs/portfolio-analysis.md` §Intrinsic verdict).
pub struct InterpretationInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub engine: &'a EngineOutput,
    pub distilled: &'a str,
    /// The engine's evaluation of the prior thesis ledger's quantitative conditions
    /// (`None` on a debut — no prior ledger to evaluate).
    pub ledger_eval: Option<&'a LedgerEvaluation>,
    /// The finalized pre-profit execution / financing overlay — present only when
    /// the stock actually entered it (`docs/portfolio-workflow.md` §Step 6f: the
    /// overlay renders with its rule-bounded conviction ceiling).
    pub pre_profit: Option<&'a PreProfitOverlay>,
    /// The input delta's technology-event pre-flag, where it was evaluable
    /// (`docs/portfolio-analysis.md` §Starting parameters) — rendered only when
    /// fired; it asserts nothing about the cause.
    pub tech_pre_flag: Option<&'a engine::TechEventPreFlag>,
    /// The narrative-vs-reality read, where it was computable
    /// (`docs/portfolio-analysis.md` §Starting parameters) — layer-(b)
    /// conviction evidence; a tripped hype cap renders with its engine-matched
    /// rule.
    pub narrative: Option<&'a engine::NarrativeRead>,
    /// The rendered input delta (`docs/portfolio-workflow.md` §Step 6g) — the
    /// bracketed-id entries the what-changed rows cite as evidence. Empty on a
    /// debut.
    pub input_delta: &'a [crate::portfolio::DeltaEntry],
}

/// What the `role_risk_only` interpretation reads: the dossier plus the engine's
/// typed readout — none of the priced machinery exists on this branch.
pub struct RoleRiskInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub readout: &'a RoleRiskReadout,
    /// The engine's evaluation of the prior fund ledger's quantitative conditions.
    pub ledger_eval: Option<&'a LedgerEvaluation>,
    /// The rendered input delta — the branch's reduced entry set. Empty on a
    /// debut.
    pub input_delta: &'a [crate::portfolio::DeltaEntry],
}

/// The branch-shaped verdict evidence the per-holding action call reads — the
/// finished intrinsic read the decision acts on. The `action` field on the
/// referenced verdict bodies is a placeholder at call time (the decision
/// overwrites it) and is deliberately never rendered.
pub enum ActionSubject<'a> {
    Priced {
        graded: &'a GradedVerdict,
        engine: &'a EngineOutput,
        pre_profit: Option<&'a PreProfitOverlay>,
    },
    RoleRisk { verdict: &'a RoleRiskVerdict },
}

/// What the **per-holding action call** reads (`docs/portfolio-analysis.md`
/// §Portfolio action): the finished intrinsic verdict, the holding's own sizing
/// evidence off the dossier, the engine's per-holding action set (evidence,
/// never a bar), and the **investor profile** — its only entry point into the
/// job, so interpretation stays profile-blind by input isolation. Tunnel
/// vision by design: no whole-book context exists here.
pub struct ActionInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub subject: ActionSubject<'a>,
    /// The engine's per-holding action set ([`engine::feasible_actions`] for a
    /// priced holding; [`crate::portfolio::ROLE_RISK_ACTIONS`] for the
    /// role/risk branch).
    pub engine_set: &'a [Action],
    pub profile: &'a crate::portfolio::InvestorProfile,
}

/// The app-stamped annotation for a chosen rung outside the engine's per-holding
/// action set — the choice persists exactly as authored; the departure records on
/// the holding's audit (`docs/portfolio-analysis.md` §Portfolio action, the
/// two-arm contract: engine evidence annotates, never bars).
fn outside_set_annotation(action: Action, engine_set: &[Action]) -> Option<String> {
    (!engine_set.contains(&action)).then(|| {
        let set: Vec<&str> = engine_set.iter().map(Action::as_kebab).collect();
        format!(
            "action {} outside the engine set [{}] — persisted as authored",
            action.as_kebab(),
            set.join(", ")
        )
    })
}

/// The audit's source line for the FRED rate anchors — appended by
/// [`analyze_holding`] only where they actually fed the engine: the priced stock
/// path and the priced fund path (scenario targets + the hurdle read). Never on a
/// no-model exit or the role/risk branch, which compute nothing from them.
pub const RATE_ANCHORS_SOURCE: &str =
    "FRED rate anchors (DGS10 / DGS2 + anchor-window history)";

/// The action call's response contract says the rationale is one sentence and
/// never empty (`docs/portfolio-analysis.md` §Portfolio action) — the schema only
/// types it as a string, so the nonempty half is enforced here, fail-hard like the
/// rest of the model stage. The one-sentence shape is a prompt preference, not
/// validated (M6 of the 2026-08-18 doc/code audit).
fn ensure_action_rationale(
    symbol: &str,
    decision: &crate::portfolio::ActionDecision,
) -> Result<()> {
    anyhow::ensure!(
        !decision.rationale.trim().is_empty(),
        "action decision for {symbol} returned an empty rationale — the response \
         contract requires one sentence"
    );
    Ok(())
}

/// The model-backed stages of the pipeline, behind a trait so the orchestration is
/// stub-driven offline and daemon-driven live. The research stage is a deterministic
/// app-layer function ([`research`]) this slice, not part of the trait.
pub trait HoldingAnalyst {
    /// Consolidate the raw findings into the compact distillation the interpretation
    /// reads (the fast 35B model, live).
    fn distill(&self, dossier: &HoldingDossier, findings: &ResearchFindings) -> Result<String>;
    /// Interpret the computed analysis + distilled findings into the schema-constrained
    /// verdict judgment (the 122B reasoner in thinking mode, live).
    fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation>;
    /// Author the union's other branch for a structurally unpriceable vehicle: the
    /// role read (no action — the action call authors that;
    /// `docs/portfolio-analysis.md` §Intrinsic verdict).
    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation>;
    /// The **per-holding action call** (`docs/portfolio-analysis.md` §Portfolio
    /// action): decide this holding's rung-only portfolio action from its own
    /// finished verdict plus the investor profile — tunnel vision, no book
    /// context (the 122B reasoner in thinking mode, live).
    fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision>;
    /// The model id [`Self::distill`] runs on (the fast tier — or the reasoner,
    /// when the fast tier fell back to it). `analyze_holding` records it on the
    /// audit only after a distill call actually ran.
    fn fast_id(&self) -> String;
    /// The model id [`Self::interpret`], [`Self::interpret_role_risk`], and
    /// [`Self::decide_action`] run on. Recorded on the audit only after one of
    /// those calls actually ran.
    fn reasoner_id(&self) -> String;
    /// Drain the prompt-size observations the calls above accumulated
    /// ([`crate::local_model::PromptUsage`]) — the data-health context-fit read
    /// (`docs/portfolio-analysis.md` §Portfolio roll-up). Defaulted empty so
    /// deterministic stubs carry no instrumentation.
    fn take_prompt_usage(&self) -> Vec<crate::local_model::PromptUsage> {
        Vec::new()
    }
}

/// Run one holding through the pipeline end to end, returning its verdict and audit
/// record. Eligibility and the evidence floor short-circuit before any model call —
/// an ineligible asset class is `not-rated`, a holding below the floor is
/// `insufficient-evidence` — so the model is only ever asked to interpret a holding
/// the engine could actually grade. No book input reaches it: the retired
/// `portfolio-weight` series took the last one with it (the tunnel-vision
/// ruling, 2026-08-14).
pub fn analyze_holding(
    analyst: &dyn HoldingAnalyst,
    dossier: &HoldingDossier,
    rates: &RateAnchors,
    run_date: &str,
) -> Result<(HoldingVerdict, HoldingAudit)> {
    let symbol = dossier.position.symbol.clone();
    let asset_class = dossier.position.asset_class;
    // The 6g executability surface is class-shaped (statement series never
    // resolve on the fund path, the expense ratio only there) — computed once
    // beside the class it derives from.
    let is_fund = matches!(
        asset_class,
        crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
    );
    // App-set from the deterministic holdings diff, never the model — carried on every
    // verdict (graded or not) as the structured what-changed position tag.
    let position_change = dossier.position_delta.change;
    // The prior run's thesis ledger (rides the prior verdict) — the standing view
    // this run tests, rewrites, and carries (`docs/portfolio-analysis.md` §The
    // position thesis ledger).
    let prior_ledger = dossier.prior_ledger();
    let mut degraded = dossier.financials.gaps.clone();
    if let Some(f) = &dossier.fund {
        degraded.extend(f.fund.gaps.iter().cloned());
    }
    // A failed DGS10 anchor-window history is a run-level degraded input — the
    // targets fell to their documented fallback rather than failing the run
    // (`docs/portfolio-analysis.md` §Starting parameters), and each holding's audit
    // records why.
    if let Some(gap) = &rates.history_gap {
        degraded.push(gap.clone());
    }
    // The listing-resolution guard's unverified outcome: the holding proceeds —
    // an FMP outage must never mass-not-rate a book — but the unverified identity
    // cross-check is a recorded degraded input
    // (`docs/portfolio-analysis.md` §Asset eligibility).
    if let Some(crate::portfolio::listing::ListingResolution::Unverified { detail }) =
        &dossier.listing
    {
        degraded.push(format!("listing-resolution guard unverified — {detail}"));
    }
    // The fund exposure comparators for the quick check's fund evidence-event legs
    // — computed from the same fresh metadata the pass analyzed, on either verdict
    // branch (`docs/portfolio-analysis.md` §Starting parameters).
    let fund_exposure = dossier
        .fund
        .as_ref()
        .map(|f| crate::portfolio::fund::exposure_basis(&f.fund));
    // Whether this holding's verdict actually **received house-view content**. It is
    // `false` for every route that returns before an interpretation call — the
    // eligibility gate, the listing guard, a net-short or fully-offset position, and
    // every evidence-floor abstention — and each interpretation path sets it from the
    // predicate belonging to the prompt it is about to build.
    //
    // Not from a shared "is a house view present" test: the two prompts render
    // *different* parts of it. The priced prompt renders the latest sections **and**
    // the recent stances, the role/risk prompt only the latest sections — and
    // `load_house_view` deliberately keeps the summaries when the latest report's
    // Markdown is missing or unreadable, so a summary-only house view is reachable and
    // reaches a role/risk verdict as nothing at all. Each predicate is defined beside
    // its own render site so the claim cannot drift from what is actually rendered.
    let house_view_consulted = std::cell::Cell::new(false);
    // Whether the FRED rate anchors actually fed this holding's verdict. They enter
    // only through the priced engine outputs (the scenario targets and the hurdle
    // read, on the stock path and the priced fund path); every earlier exit and the
    // role/risk branch computes nothing from them, so their audits must not name
    // them (M3 of the 2026-08-18 doc/code audit).
    let rates_consulted = std::cell::Cell::new(false);
    // The Step-5 enriching feeds, each recorded only where a prompt actually
    // rendered it (the same actually-consulted discipline as the house view):
    // the CBOE backdrop and the fund's COT positioning render whenever present
    // on the dossier at an interpretation call; the sector-benchmark series is
    // consulted exactly where the technology-event pre-flag evaluation read it.
    let backdrop_consulted = std::cell::Cell::new(false);
    let positioning_consulted = std::cell::Cell::new(false);
    let benchmark_consulted = std::cell::Cell::new(false);
    let commodity_consulted = std::cell::Cell::new(false);
    let short_interest_consulted = std::cell::Cell::new(false);
    // The model ids this holding's verdict was **actually** authored with, in
    // first-call order — recorded beside each call that ran, never read off the
    // analyst's configuration: a no-model exit persists none, the role/risk branch
    // the reasoner only, the priced path the fast tier and the reasoner (one entry
    // when they are the same model).
    let models_used: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    let used_model = |id: String| {
        let mut used = models_used.borrow_mut();
        if !used.contains(&id) {
            used.push(id);
        }
    };
    // The audit's source list. **Both** audit construction sites go through this — the
    // closure below for every early return, and the priced path's own record — because
    // duplicating it is how the house-view claim survived the first fix.
    let audit_sources = || {
        let mut sources = dossier.sources.clone();
        if rates_consulted.get() {
            sources.push(RATE_ANCHORS_SOURCE.to_string());
        }
        if house_view_consulted.get() {
            sources.push(crate::portfolio::dossier::HOUSE_VIEW_SOURCE.to_string());
        }
        if backdrop_consulted.get() {
            sources.push("CBOE daily put/call (venue backdrop)".to_string());
        }
        if commodity_consulted.get() {
            sources.push(
                "Run-level commodity context (FRED energy / IMF metals / FMP gold)".to_string(),
            );
        }
        if positioning_consulted.get() {
            sources.push("CFTC COT positioning (fund underlying)".to_string());
        }
        if short_interest_consulted.get() {
            sources.push("FINRA consolidated short interest (biweekly file)".to_string());
        }
        if benchmark_consulted.get() {
            sources.push(
                "FMP sector benchmark series (technology-event pre-flag)".to_string(),
            );
        }
        sources
    };
    let audit = |metrics, target_meta, ledger_audit, pre_profit| HoldingAudit {
        symbol: symbol.clone(),
        metrics,
        sources: audit_sources(),
        model_ids: models_used.borrow().clone(),
        prompt_version: PROMPT_VERSION.to_string(),
        degraded_inputs: degraded.clone(),
        action_annotations: Vec::new(),
        target_meta,
        grade_parameter_version: Some(engine::GRADE_PARAMETER_VERSION.to_string()),
        ledger_audit,
        quick_basis: None,
        fund_exposure: fund_exposure.clone(),
        pre_profit,
        hurdle: None,
        // Every exit records the sweep state where the gather ran it; the
        // matched hard rule is stamped only on the priced path, where the
        // engine consequences it names were actually computed.
        forensic: dossier
            .filing_events
            .clone()
            .map(|state| crate::portfolio::ForensicRead {
                matched_rule: None,
                state,
            }),
        // The pre-flag is evaluated on the priced path only (it reads the
        // engine's volatility); an early exit records none.
        tech_event_pre_flag: None,
        // Provenance like the sweep state: the row resolved at gather time,
        // recorded wherever it exists (the source label stays render-scoped).
        short_interest: dossier.short_interest.clone(),
        // Computed on the priced path only; an early exit records none.
        implied_expectations: None,
        narrative: None,
        option_overlay: dossier.option_overlay.clone(),
        // Validated only where an interpretation ran; every early exit records
        // none.
        what_changed_audit: None,
    };
    let abstain = |reason: String, metrics, meta, pre_profit| {
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            disposition: VerdictDisposition::InsufficientEvidence { reason },
            // A below-floor exit retains the standing ledger unchanged — Steps
            // 6c–6f never ran for it (`docs/portfolio-workflow.md` §Step 6b).
            thesis_ledger: prior_ledger.cloned(),
            // Vintages are the job layer's concern: it stamps a fresh pass with the
            // run's `created_at` and preserves an abstention's prior vintage.
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
            side_reversed: false,
        };
        // An abstaining stock still records its overlay (fresh statement leg +
        // carried observation history) — engine-only state, no model dependency, so
        // the history survives an abstention like the standing ledger does.
        Ok((verdict, audit(metrics, meta, None, pre_profit)))
    };

    // Eligibility: a non-equity class is never given a fabricated grade.
    if !asset_class.is_gradeable() {
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            disposition: VerdictDisposition::NotRated {
                reason: format!("{} is not graded by the equity pipeline", asset_class.label()),
            },
            thesis_ledger: None,
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
            side_reversed: false,
        };
        return Ok((verdict, audit(Default::default(), None, None, None)));
    }

    // Eligibility: a net-short position is a direction the prescriptive layer doesn't
    // model — the ladder's verbs and the outcome labels all read long — so it
    // takes the not-rated treatment with a short-position reason
    // (`docs/portfolio-analysis.md` §Asset eligibility). An exactly-zero netted
    // position (long and short legs fully offset across accounts — deliberately
    // kept by netting) is neither long nor short: it must not carry the
    // long-ladder read on zero economic exposure, so it is not-rated too.
    if dossier.position.quantity <= 0.0 {
        let reason = if dossier.position.quantity < 0.0 {
            "held net short — the ladder's long-side semantics don't apply; \
             weighing the signed exposure against the book is the future \
             portfolio planner's work"
        } else {
            "fully offset — the netted position is zero shares, so there is no \
             economic exposure for the long-side ladder to act on"
        };
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            disposition: VerdictDisposition::NotRated {
                reason: reason.to_string(),
            },
            thesis_ledger: None,
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
            side_reversed: false,
        };
        return Ok((verdict, audit(Default::default(), None, None, None)));
    }

    // Eligibility: the loop-time listing-resolution guard, stocks only
    // (`docs/portfolio-analysis.md` §Asset eligibility). No canonical FMP
    // resolution or a non-US primary listing is a structural can't-grade — the
    // US-only data plan has no honest statement surface for it; a
    // resolved-but-conflicting issuer identity is the evidence floor's
    // conflicting-identity arm — a data problem, possibly transient — so a
    // wrong-issuer mapping can never grade the wrong company's financials.
    if matches!(asset_class, crate::portfolio::AssetClass::Stock) {
        let unsupported = match &dossier.listing {
            Some(crate::portfolio::listing::ListingResolution::Unresolved) => Some(
                "unsupported listing — no canonical FMP resolution for this symbol".to_string(),
            ),
            Some(crate::portfolio::listing::ListingResolution::NonUs { exchange }) => {
                Some(format!(
                    "unsupported listing — primary listing on {exchange}, outside the \
                     US-listed surface the suite's data plan covers"
                ))
            }
            _ => None,
        };
        if let Some(reason) = unsupported {
            let verdict = HoldingVerdict {
                symbol: symbol.clone(),
                asset_class,
                position_change,
                disposition: VerdictDisposition::NotRated { reason },
                thesis_ledger: None,
                analyzed_at: None,
                action_source: ActionSource::ModelChosen,
                side_reversed: false,
            };
            return Ok((verdict, audit(Default::default(), None, None, None)));
        }
        if let Some(crate::portfolio::listing::ListingResolution::Conflict { fmp_name }) =
            &dossier.listing
        {
            // The floor exit's overlay-survival semantics hold at the guard
            // too: the guard-terminal skip fetched no statements, so the
            // record reads eligibility-unscorable with its input gaps — but
            // the period-keyed observation history carries forward, so one
            // conflicted (possibly transient) run can never reset it
            // (`docs/storage.md` §Local Analysis Suite Storage).
            let pre_profit = crate::portfolio::pre_profit::compute_overlay(
                &dossier.financials,
                dossier.prior_pre_profit.as_ref(),
                Vec::new(),
            );
            return abstain(
                format!(
                    "conflicting identity — FMP resolves this symbol to \"{fmp_name}\", \
                     which does not match the account's \"{}\"",
                    dossier.position.description
                ),
                Default::default(),
                None,
                Some(pre_profit),
            );
        }
    }

    // The deterministic engine stage, per branch: the equity engine for a stock, the
    // reduced fund computation (strategy-routed at loop time) for a fund
    // (`docs/portfolio-workflow.md` §Step 6b).
    let mut pre_profit_overlay: Option<PreProfitOverlay> = None;
    let engine_output = if matches!(
        asset_class,
        crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
    ) {
        let Some(fund_ctx) = &dossier.fund else {
            return abstain(
                "fund metadata (etf/info) unavailable — the fund analog's floor-bearing \
                 input is missing"
                    .to_string(),
                Default::default(),
                None,
                None,
            );
        };
        let inputs = fund::FundEngineInputs {
            fund: &fund_ctx.fund,
            financials: &dossier.financials,
            sector_pe: &fund_ctx.sector_pe,
            sector_pe_history: &fund_ctx.sector_pe_history,
            rates,
            as_of: fund_ctx.as_of,
        };
        match fund::analyze_fund(&inputs) {
            FundEngineVerdict::Priced(out) => out,
            FundEngineVerdict::InsufficientEvidence(reason) => {
                return abstain(reason, Default::default(), None, None);
            }
            FundEngineVerdict::RoleRiskOnly(readout) => {
                // Evaluate the prior fund ledger's quantitative conditions against
                // the reduced surface this branch actually computes: the expense
                // ratio plus the price-derived legs (trailing return, return
                // volatility) from the closes the dossier already carries —
                // price/weight resolve from the dossier directly. The full pass
                // must cover the SAME fund-computable surface the quick check
                // evaluates, or a sweep-confirmed price-leg crossing would read
                // unevaluable here, never be acknowledged, and re-raise on every
                // later sweep after the successful pass cleared the store.
                let price_legs = engine::compute_metrics(&dossier.financials);
                let fund_metrics = engine::ComputedMetrics {
                    expense_ratio: readout.expense_ratio,
                    return_volatility: price_legs.return_volatility,
                    trailing_return: price_legs.trailing_return,
                    ..Default::default()
                };
                let ledger_eval = prior_ledger.map(|l| {
                    engine::evaluate_ledger_conditions(
                        l,
                        &fund_metrics,
                        &dossier.financials,
                        run_date,
                    )
                });
                // The union's other branch: the model authors the role read only —
                // the branch's action is authored by the dedicated per-holding
                // action call below, the full ladder structurally open while the
                // engine arm's reduced set (sell-all / trim / hold) rides as
                // annotated evidence (`docs/portfolio-analysis.md` §Portfolio
                // action).
                house_view_consulted.set(role_risk_prompt_renders_house_view(dossier));
                backdrop_consulted.set(dossier.put_call_backdrop.is_some());
                positioning_consulted
                    .set(dossier.fund.as_ref().is_some_and(|f| f.positioning.is_some()));
                // The branch's rendered input delta — the what-changed rows'
                // evidence vocabulary (`docs/portfolio-workflow.md` §Step 6g).
                let input_delta = role_risk_input_delta(
                    dossier,
                    &fund_metrics,
                    position_change,
                    ledger_eval.as_ref(),
                );
                let interpretation = analyst
                    .interpret_role_risk(&RoleRiskInput {
                        dossier,
                        readout: &readout,
                        ledger_eval: ledger_eval.as_ref(),
                        input_delta: &input_delta,
                    })
                    .context("interpreting the role/risk holding")?;
                used_model(analyst.reasoner_id());
                // The 6g what-changed attribution validator — external claims
                // resolve against the rendered delta or downgrade to
                // self-correction; a debut records no audit.
                let what_changed_audit = dossier.prior_verdict.is_some().then(|| {
                    validate_what_changed(&interpretation.what_changed_entries, input_delta)
                });
                // The 6g ledger seam: validate the rewrite — executability,
                // condition identity / carry, tripped / fired claims, the branch's
                // reductions (condition-only monitor, trim / sell triggers).
                let (ledger, ledger_audit) = validate_ledger_rewrite(
                    &interpretation.ledger,
                    prior_ledger,
                    ledger_eval.as_ref(),
                    LedgerBranch::RoleRiskOnly,
                    is_fund,
                    None,
                    dossier.financials.current_price,
                );
                // The action placeholder is overwritten by the decision below and
                // never rendered into its prompt.
                let mut rr = RoleRiskVerdict {
                    class_label: readout.class_label.clone(),
                    role_summary: interpretation.role_summary,
                    exposure_tilt: readout
                        .exposure_tilt
                        .iter()
                        .map(|(label, weight)| ExposureWeight {
                            label: label.clone(),
                            weight: *weight,
                        })
                        .collect(),
                    expense_drag: readout.expense_ratio,
                    observable_risk: readout.observable_risk,
                    structural_flag: readout.structural_flag,
                    evidence_gaps: readout.evidence_gaps.clone(),
                    action: Action::Hold,
                    action_rationale: String::new(),
                    what_changed: interpretation.what_changed,
                };
                let decision = analyst
                    .decide_action(&ActionInput {
                        dossier,
                        subject: ActionSubject::RoleRisk { verdict: &rr },
                        engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
                        profile: &dossier.profile,
                    })
                    .context("deciding the role/risk holding's action")?;
                used_model(analyst.reasoner_id());
                ensure_action_rationale(&symbol, &decision)?;
                rr.action = decision.action;
                rr.action_rationale = decision.rationale;
                // The branch's computed surface persists as the audit's metrics — the
                // same expense-ratio + price-derived legs the ledger evaluation above
                // read, never the empty default (M3 of the 2026-08-18 audit).
                let mut audit_record = audit(fund_metrics, None, Some(ledger_audit), None);
                audit_record.what_changed_audit = what_changed_audit;
                audit_record
                    .degraded_inputs
                    .extend(dossier.semantic_recall.gap.clone());
                audit_record.action_annotations.extend(outside_set_annotation(
                    decision.action,
                    &crate::portfolio::ROLE_RISK_ACTIONS,
                ));
                let verdict = HoldingVerdict {
                    symbol: symbol.clone(),
                    asset_class,
                    position_change,
                    disposition: VerdictDisposition::RoleRiskOnly(Box::new(rr)),
                    thesis_ledger: Some(ledger),
                    analyzed_at: None,
                    action_source: ActionSource::ModelChosen,
                    side_reversed: false,
                };
                return Ok((verdict, audit_record));
            }
        }
    } else {
        // The pre-profit execution / financing overlay's statement leg + observation
        // merge (`docs/portfolio-workflow.md` §Step 6b / §Step 6e — computed in one
        // place as-built, since the dormant research producer supplies no candidate
        // rows between the two seams). Computed for every stock: the eligibility
        // result persists even when the stock does not enter.
        pre_profit_overlay = Some(pre_profit::compute_overlay(
            &dossier.financials,
            dossier.prior_pre_profit.as_ref(),
            Vec::new(),
        ));
        match engine::analyze(&dossier.financials, rates) {
            EngineVerdict::Analyzed(out) => out,
            EngineVerdict::InsufficientEvidence(reason) => {
                return abstain(reason, Default::default(), None, pre_profit_overlay);
            }
        }
    };
    // Only a priced engine output computed from the rate anchors (scenario targets,
    // hurdle) — every route above returned without one.
    rates_consulted.set(true);

    // The overlay's rules join only when the stock actually entered the overlay
    // (a priced fund carries none) — they bind the engine arm's stand-in and the
    // engine's per-holding action set below.
    let overlay_rules = pre_profit_overlay
        .as_ref()
        .filter(|o| o.is_eligible())
        .map(|o| &o.consequences);

    // The hard-forensic trip — resolved only from the typed filings-sweep
    // producer (`docs/portfolio-analysis.md` §Starting parameters: the
    // conviction-layer caps' hard triggers); an `Unknown` sweep never trips.
    let hard_forensic = dossier
        .filing_events
        .as_ref()
        .map(crate::portfolio::ForensicFilingState::hard_tripped)
        .unwrap_or(false);

    // The input delta's technology-event pre-flag (`docs/portfolio-analysis.md`
    // §Starting parameters) — an equity read, evaluable only for a carried
    // stock with a sector-benchmark series; an unevaluable read records its
    // typed reason on the audit, never a fired or clear flag. A debut has
    // nothing to diff, so it is neither a flag nor a gap.
    let (tech_pre_flag, tech_pre_flag_gap) = if is_fund {
        (None, None)
    } else {
        match (&dossier.prior_vintage, &dossier.sector_benchmark) {
            (None, _) => (None, None),
            (Some(_), None) => (
                None,
                Some(
                    "technology-event pre-flag unevaluable: no sector benchmark \
                     series this run"
                        .to_string(),
                ),
            ),
            (Some(vintage), Some(bench)) => {
                match crate::market_clock::et_date_of(vintage) {
                    None => (
                        None,
                        Some(format!(
                            "technology-event pre-flag unevaluable: unreadable prior \
                             vintage {vintage:?}"
                        )),
                    ),
                    Some(session) => {
                        // The label's warrant: only here is the series actually
                        // handed to the evaluation (an unreadable vintage never
                        // reads it — Codex 2026-08-20 round 2, finding 4).
                        benchmark_consulted.set(true);
                        match engine::tech_event_pre_flag(
                        &dossier.financials.daily_closes,
                        &bench.closes,
                        &bench.symbol,
                        &session.format("%Y-%m-%d").to_string(),
                        engine_output.metrics.return_volatility,
                    ) {
                            Ok(flag) => (Some(flag), None),
                            Err(reason) => (
                                None,
                                Some(format!(
                                    "technology-event pre-flag unevaluable: {reason}"
                                )),
                            ),
                        }
                    }
                }
            }
        }
    };

    // Evaluate the prior ledger's quantitative falsifiers and triggers against this
    // run's computed surface — the crossings interpretation reads
    // (`docs/portfolio-analysis.md` §The position thesis ledger).
    let ledger_eval = prior_ledger.map(|l| {
        engine::evaluate_ledger_conditions(
            l,
            &engine_output.metrics,
            &dossier.financials,
            run_date,
        )
    });

    // The narrative-vs-reality read (`docs/portfolio-analysis.md` §Starting
    // parameters) — a stock's pace pair against the prior run's stored
    // comparator (a fund has neither consensus nor statements to read). An
    // unreadable pace on a *carried* holding records its typed reason; a debut
    // has no comparator and records nothing (the debut-null convention). A
    // tripped hype read is the suite's shared soft Medium ceiling on the
    // engine arm — annotation-recorded, never a clamp on the model's value.
    let (narrative, narrative_gap) = if is_fund {
        (None, None)
    } else {
        let elapsed = dossier.prior_vintage.as_deref().and_then(|v| {
            let prior_session = crate::market_clock::et_date_of(v)?;
            let run = chrono::NaiveDate::parse_from_str(run_date, "%Y-%m-%d").ok()?;
            Some((run - prior_session).num_days())
        });
        match engine::narrative_vs_reality(
            &dossier.financials,
            engine_output
                .quick_basis
                .as_ref()
                .map(|b| b.spot)
                .or(dossier.financials.current_price)
                .unwrap_or(f64::NAN),
            dossier.prior_spot,
            dossier.prior_consensus_eps_mid,
            elapsed,
        ) {
            Ok(read) => (Some(read), None),
            Err(reason) => (
                None,
                dossier
                    .prior_verdict
                    .is_some()
                    .then(|| format!("narrative-vs-reality unreadable: {reason}")),
            ),
        }
    };
    let narrative_hype = narrative.as_ref().is_some_and(|n| n.hype_capped());

    // Research (stubbed) → distill → interpret.
    house_view_consulted.set(priced_prompt_renders_house_view(dossier));
    backdrop_consulted.set(dossier.put_call_backdrop.is_some());
    positioning_consulted.set(dossier.fund.as_ref().is_some_and(|f| f.positioning.is_some()));
    commodity_consulted.set(!dossier.commodity_context.is_empty());
    short_interest_consulted.set(dossier.short_interest.is_some());
    let findings = research(dossier);
    let distilled = analyst
        .distill(dossier, &findings)
        .context("distilling research findings")?;
    used_model(analyst.fast_id());
    // The rendered input delta — the what-changed rows' evidence vocabulary
    // (`docs/portfolio-workflow.md` §Step 6g); empty on a debut.
    let input_delta = priced_input_delta(
        dossier,
        &engine_output,
        position_change,
        ledger_eval.as_ref(),
        tech_pre_flag.as_ref(),
        narrative.as_ref(),
        hard_forensic,
    );
    let interpretation = analyst
        .interpret(&InterpretationInput {
            dossier,
            engine: &engine_output,
            distilled: &distilled,
            ledger_eval: ledger_eval.as_ref(),
            pre_profit: pre_profit_overlay.as_ref().filter(|o| o.is_eligible()),
            tech_pre_flag: tech_pre_flag.as_ref(),
            narrative: narrative.as_ref(),
            input_delta: &input_delta,
        })
        .context("interpreting the holding")?;
    used_model(analyst.reasoner_id());
    // The 6g what-changed attribution validator — every external row resolves
    // against the rendered delta or downgrades to self-correction with a logged
    // reason; a debut records no audit.
    let what_changed_audit = dossier
        .prior_verdict
        .is_some()
        .then(|| validate_what_changed(&interpretation.what_changed_entries, input_delta));
    // The v7 unrestricted contract: the model's conviction persists exactly as
    // authored — no bail, no clamp (`docs/portfolio-analysis.md` §The holding
    // verdict). Any matched pre-profit ceiling stays recorded on the overlay /
    // engine view, so a conviction above the ceiling reads as an annotated
    // divergence, never an error.
    let conviction = interpretation.conviction;

    // The 6g ledger seam: validate the rewrite and stamp the engine's scenario
    // targets into the monitor (app-owns-the-number — a model-written target never
    // persists).
    let (ledger, ledger_audit) = validate_ledger_rewrite(
        &interpretation.ledger,
        prior_ledger,
        ledger_eval.as_ref(),
        LedgerBranch::Priced,
        is_fund,
        engine_output.price_targets.twelve_month.as_ref(),
        dossier.financials.current_price,
    );

    // The engine stand-in arm — mechanical outlook / conviction / action baselines
    // beside the model's (`docs/portfolio-analysis.md` §The holding verdict).
    let engine_view =
        engine::engine_view(&engine_output, &dossier.financials, &degraded, overlay_rules, hard_forensic, narrative_hype);
    let mut graded = GradedVerdict {
        grade: engine_output.grade,
        sub_scores: engine_output.sub_scores,
        // A placeholder — the per-holding action call below authors the action
        // and overwrites both fields; the placeholder is never rendered into
        // that call's prompt.
        action: Action::Hold,
        action_rationale: String::new(),
        conviction,
        horizon_outlook: interpretation.horizon_outlook,
        price_targets: engine_output.price_targets.clone(),
        price_target_rationale: interpretation.price_target_rationale,
        options_signal: dossier.options_signal.clone(),
        risk_tier: Some(engine_output.risk_tier),
        dead_money: Some(engine_output.hurdle.state),
        low_confidence_grade: engine_output.low_confidence_grade,
        fund_class_label: engine_output.fund_class_label.clone(),
        structural_flag: engine_output.structural_flag,
        financial_summary: interpretation.financial_summary,
        what_changed: interpretation.what_changed,
        // The model arm: persisted exactly as authored, letter derived from the
        // model's own scores through the shared cutoffs (the two-arm contract —
        // `docs/portfolio-analysis.md` §The holding verdict).
        model_view: ModelView {
            sub_scores: interpretation.model_sub_scores,
            letter: engine::grade_from_subscores(&interpretation.model_sub_scores),
            price_targets: interpretation.model_price_targets.clone(),
            self_assessment: interpretation.self_assessment.clone(),
        },
        engine_view,
    };
    // The per-holding action call — the profile's one entry point: the finished
    // verdict plus the holding's own evidence decide the rung, tunnel vision by
    // design (`docs/portfolio-analysis.md` §Portfolio action). The engine's
    // per-holding set rides as evidence; an outside-the-set choice persists as
    // authored with the departure annotated on the audit.
    let engine_set =
        engine::feasible_actions(engine_output.grade, &engine_output.hurdle, overlay_rules, hard_forensic);
    let decision = analyst
        .decide_action(&ActionInput {
            dossier,
            subject: ActionSubject::Priced {
                graded: &graded,
                engine: &engine_output,
                pre_profit: pre_profit_overlay.as_ref().filter(|o| o.is_eligible()),
            },
            engine_set: &engine_set,
            profile: &dossier.profile,
        })
        .context("deciding the holding's action")?;
    used_model(analyst.reasoner_id());
    ensure_action_rationale(&symbol, &decision)?;
    graded.action = decision.action;
    graded.action_rationale = decision.rationale;
    let verdict = HoldingVerdict {
        symbol: symbol.clone(),
        asset_class,
        position_change,
        disposition: VerdictDisposition::Priced(Box::new(graded)),
        thesis_ledger: Some(ledger),
        analyzed_at: None,
        action_source: ActionSource::ModelChosen,
        side_reversed: false,
    };
    // The engine's own gap notes (tier-input gaps, the fund composite's uncovered
    // share, an option-overlay structural flag) join the audit's degraded inputs —
    // recorded, never silently dropped.
    let mut degraded_inputs = degraded.clone();
    degraded_inputs.extend(engine_output.tier_gaps.iter().cloned());
    degraded_inputs.extend(tech_pre_flag_gap.clone());
    degraded_inputs.extend(narrative_gap.clone());
    degraded_inputs.extend(dossier.semantic_recall.gap.clone());
    let audit_record = HoldingAudit {
        symbol: symbol.clone(),
        metrics: engine_output.metrics.clone(),
        sources: audit_sources(),
        model_ids: models_used.borrow().clone(),
        prompt_version: PROMPT_VERSION.to_string(),
        degraded_inputs,
        action_annotations: outside_set_annotation(decision.action, &engine_set)
            .into_iter()
            .collect(),
        target_meta: Some(engine_output.target_meta.clone()),
        grade_parameter_version: Some(engine::GRADE_PARAMETER_VERSION.to_string()),
        ledger_audit: Some(ledger_audit),
        quick_basis: engine_output.quick_basis.clone(),
        fund_exposure: fund_exposure.clone(),
        pre_profit: pre_profit_overlay,
        // The full hurdle read persists so a decision episode's calibration
        // snapshot can freeze the hurdle inputs (`docs/portfolio-analysis.md`
        // §Outcome learning).
        hurdle: Some(engine_output.hurdle.clone()),
        forensic: dossier
            .filing_events
            .clone()
            .map(|state| crate::portfolio::ForensicRead {
                matched_rule: hard_forensic.then(|| {
                    "hard forensic trigger: engine conviction capped Low; \
                     add family barred from the engine action set"
                        .to_string()
                }),
                state,
            }),
        tech_event_pre_flag: tech_pre_flag,
        short_interest: dossier.short_interest.clone(),
        implied_expectations: engine_output.implied_expectations.clone(),
        narrative,
        option_overlay: dossier.option_overlay.clone(),
        what_changed_audit,
    };
    Ok((verdict, audit_record))
}

// ---- Thesis-ledger rewrite validation (the 6g seam) ----------------------------

/// Parse a draft's quantitative-core claim against the engine's executability
/// surface — the resolution contract's app-side check
/// (`docs/portfolio-workflow.md` §Step 6g): the series must be one the engine
/// actually computes and refreshes **for this holding's vehicle kind**, the
/// comparator well-formed, the numbers finite. `Err` carries the downgrade
/// reason.
fn parse_quant_core(qd: &QuantCoreDraft, is_fund: bool) -> std::result::Result<QuantCore, String> {
    let series = engine::LedgerSeries::parse(&qd.series).ok_or_else(|| {
        format!(
            "series '{}' does not resolve to a series the engine computes",
            qd.series
        )
    })?;
    if !series.computable_for(is_fund) {
        return Err(format!(
            "series '{}' has no {} computation — the condition would be \
             permanently unevaluable on this holding",
            qd.series,
            if is_fund { "fund-path" } else { "stock-path" }
        ));
    }
    let comparator = match qd.comparator.trim() {
        "below" => LedgerComparator::Below,
        "above" => LedgerComparator::Above,
        other => return Err(format!("comparator '{other}' is not below/above")),
    };
    if !qd.threshold.is_finite() {
        return Err("threshold is not a finite number".to_string());
    }
    let margin = if qd.margin.is_finite() {
        qd.margin.max(0.0)
    } else {
        0.0
    };
    Ok(QuantCore {
        series,
        comparator,
        threshold: qd.threshold,
        margin,
    })
}

/// Pull the prior condition whose machine core exactly matches (the carry case:
/// unchanged core → the id and evaluation state survive any re-wording). The
/// trigger family disambiguates alongside the role and core — trim-vs-sell on
/// one core are distinct pre-commitments (the dedup contract), so an equal-core
/// pair must never exchange ids, streaks, or acknowledgments on reorder.
fn take_exact_core(
    pool: &mut Vec<LedgerCondition>,
    role: ConditionRole,
    trigger_family: Option<TriggerFamily>,
    core: &QuantCore,
) -> Option<LedgerCondition> {
    pool.iter()
        .position(|c| {
            c.role == role
                && c.trigger_family == trigger_family
                && c.quant.as_ref() == Some(core)
        })
        .map(|i| pool.remove(i))
}

/// Pull a prior condition by its id — the pre-assigned supersession ancestor
/// ([`assign_supersessions`]).
fn take_by_id(pool: &mut Vec<LedgerCondition>, id: &str) -> Option<LedgerCondition> {
    pool.iter()
        .position(|c| c.condition_id == id)
        .map(|i| pool.remove(i))
}

/// The per-pair supersession cost over the **complete** machine core: comparator
/// mismatch dominates, then threshold distance, then margin distance (threshold
/// and margin share the series' units).
fn supersession_cost(d: &QuantCore, p: &QuantCore) -> (u32, f64, f64) {
    (
        u32::from(d.comparator != p.comparator),
        (d.threshold - p.threshold).abs(),
        (d.margin - p.margin).abs(),
    )
}

/// One candidate assignment: each draft's prior index (`None` = unmatched, a
/// brand-new condition) plus the summed cost tuple.
type AssignmentCandidate = (Vec<Option<usize>>, (u32, f64, f64));

/// Exhaustively search the injective assignment of changed draft cores to prior
/// conditions minimizing the summed cost tuple, requiring a maximum matching
/// (`min(m, n)` pairs). Groups are tiny, so exhaustive is exact and cheap.
fn search_assignment(
    i: usize,
    draft_cores: &[&QuantCore],
    prior_cores: &[&QuantCore],
    used: &mut [bool],
    current: &mut Vec<Option<usize>>,
    acc: (u32, f64, f64),
    best: &mut Option<AssignmentCandidate>,
) {
    let m = draft_cores.len();
    let n = prior_cores.len();
    if i == m {
        if current.iter().filter(|x| x.is_some()).count() < m.min(n) {
            return; // not a maximum matching
        }
        if best.as_ref().is_none_or(|(_, b)| acc < *b) {
            *best = Some((current.clone(), acc));
        }
        return;
    }
    for j in 0..n {
        if used[j] {
            continue;
        }
        used[j] = true;
        current[i] = Some(j);
        let c = supersession_cost(draft_cores[i], prior_cores[j]);
        search_assignment(
            i + 1,
            draft_cores,
            prior_cores,
            used,
            current,
            (acc.0 + c.0, acc.1 + c.1, acc.2 + c.2),
            best,
        );
        used[j] = false;
        current[i] = None;
    }
    if m > n {
        // More drafts than priors: this draft may go unmatched (a brand-new
        // condition), as long as the matching stays maximal.
        current[i] = None;
        search_assignment(i + 1, draft_cores, prior_cores, used, current, acc, best);
    }
}

/// A group larger than this on either side skips lineage assignment entirely —
/// conservative (fresh conditions + plain closures) over guessed links. Far above
/// any real ledger's same-series condition count.
const SUPERSESSION_GROUP_CAP: usize = 4;

/// Globally assign the **changed** draft cores (no exact prior match) to the
/// remaining unreserved prior conditions, per (role, trigger-family, series)
/// group — a minimum-cost matching over the complete machine core, computed on
/// the **canonically sorted** draft set so lineage depends on the set of drafted
/// conditions, never on the order the model emitted them (greedy local
/// nearest-matching flips both links when two drafts share a nearest ancestor).
/// The family is a group axis because trim-vs-sell on one core are distinct
/// pre-commitments — lineage never crosses families. Returns draft-key → prior
/// `condition_id`.
fn assign_supersessions(
    changed: &[(ConditionRole, Option<TriggerFamily>, QuantCore, String)],
    prior_pool: &[LedgerCondition],
    reserved: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, String> {
    type GroupKey = (ConditionRole, Option<TriggerFamily>, engine::LedgerSeries);
    let mut assigned: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut groups: Vec<GroupKey> = Vec::new();
    for (role, family, core, _) in changed {
        if !groups.contains(&(*role, *family, core.series)) {
            groups.push((*role, *family, core.series));
        }
    }
    for (role, family, series) in groups {
        let mut drafts: Vec<&(ConditionRole, Option<TriggerFamily>, QuantCore, String)> = changed
            .iter()
            .filter(|(r, f, c, _)| *r == role && *f == family && c.series == series)
            .collect();
        drafts.sort_by(|a, b| {
            (a.2.comparator.as_kebab(), a.2.threshold, a.2.margin)
                .partial_cmp(&(b.2.comparator.as_kebab(), b.2.threshold, b.2.margin))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let priors: Vec<&LedgerCondition> = prior_pool
            .iter()
            .filter(|c| {
                c.role == role
                    && c.trigger_family == family
                    && c.quant.as_ref().map(|q| q.series) == Some(series)
                    && !reserved.contains(&c.condition_id)
            })
            .collect();
        if priors.is_empty()
            || drafts.len() > SUPERSESSION_GROUP_CAP
            || priors.len() > SUPERSESSION_GROUP_CAP
        {
            continue;
        }
        let draft_cores: Vec<&QuantCore> = drafts.iter().map(|(_, _, c, _)| c).collect();
        let prior_cores: Vec<&QuantCore> =
            priors.iter().map(|c| c.quant.as_ref().unwrap()).collect();
        let mut best: Option<AssignmentCandidate> = None;
        let mut used = vec![false; prior_cores.len()];
        let mut current: Vec<Option<usize>> = vec![None; draft_cores.len()];
        search_assignment(
            0,
            &draft_cores,
            &prior_cores,
            &mut used,
            &mut current,
            (0, 0.0, 0.0),
            &mut best,
        );
        if let Some((mapping, _)) = best {
            for (i, slot) in mapping.iter().enumerate() {
                if let Some(j) = slot {
                    assigned.insert(drafts[i].3.clone(), priors[*j].condition_id.clone());
                }
            }
        }
    }
    assigned
}

/// Pull the prior qualitative condition with the same statement (qualitative
/// identity is textual — no machine core, no evaluation state to protect). The
/// trigger family disambiguates here too, for the same reason as the exact-core
/// carry.
fn take_same_statement(
    pool: &mut Vec<LedgerCondition>,
    role: ConditionRole,
    trigger_family: Option<TriggerFamily>,
    statement: &str,
) -> Option<LedgerCondition> {
    pool.iter()
        .position(|c| {
            c.role == role
                && c.trigger_family == trigger_family
                && c.quant.is_none()
                && c.statement == statement
        })
        .map(|i| pool.remove(i))
}

/// Validate one draft condition into a persisted [`LedgerCondition`]: executability
/// (downgrade-not-drop), app-decided identity (carry / supersede / new), and the
/// tripped / fired claim (honored only against a confirmed engine crossing on the
/// carried id — `docs/portfolio-workflow.md` §Step 6g).
#[allow(clippy::too_many_arguments)]
fn validate_condition(
    statement: &str,
    role: ConditionRole,
    trigger_family: Option<TriggerFamily>,
    quant_draft: Option<&QuantCoreDraft>,
    is_fund: bool,
    technology_class: bool,
    claimed: bool,
    prior_pool: &mut Vec<LedgerCondition>,
    assigned_prior: Option<&str>,
    confirmed_ids: &std::collections::HashSet<String>,
    updated_states: &std::collections::HashMap<String, ConditionEvalState>,
    audit: &mut LedgerAudit,
) -> LedgerCondition {
    let statement = statement.trim().to_string();
    let (quant, downgraded_reason) = match quant_draft {
        None => (None, None),
        Some(qd) => match parse_quant_core(qd, is_fund) {
            Ok(core) => (Some(core), None),
            Err(reason) => {
                // Downgraded to qualitative, logged, never dropped — and it retains
                // no machine evaluation state.
                audit
                    .downgraded
                    .push(format!("'{statement}': {reason}"));
                (None, Some(reason))
            }
        },
    };

    let (condition_id, supersedes, eval_state, carried) = match &quant {
        Some(core) => {
            if let Some(prev) = take_exact_core(prior_pool, role, trigger_family, core) {
                // Unchanged machine core: the id and accumulated state carry
                // through any re-wording.
                let state = updated_states
                    .get(&prev.condition_id)
                    .cloned()
                    .or(prev.eval_state)
                    .or_else(|| Some(ConditionEvalState::default()));
                (prev.condition_id, prev.supersedes, state, true)
            } else if let Some(mut prev) = assigned_prior.and_then(|id| take_by_id(prior_pool, id))
            {
                // Edited core: supersede the globally assigned ancestor — fresh
                // id, fresh streak, the old condition closed **whole** into the
                // audit (its state as of this run's evaluation preserved) with
                // the link.
                let new_id = uuid::Uuid::new_v4().to_string();
                prev.eval_state = updated_states
                    .get(&prev.condition_id)
                    .cloned()
                    .or(prev.eval_state);
                let prev_id = prev.condition_id.clone();
                audit.superseded.push(ClosedCondition {
                    superseded_by: Some(new_id.clone()),
                    condition: prev,
                });
                (
                    new_id,
                    Some(prev_id),
                    Some(ConditionEvalState::default()),
                    false,
                )
            } else {
                (
                    uuid::Uuid::new_v4().to_string(),
                    None,
                    Some(ConditionEvalState::default()),
                    false,
                )
            }
        }
        None => {
            // Qualitative (authored or downgraded): carry the id on an unchanged
            // statement; no machine evaluation state either way.
            match take_same_statement(prior_pool, role, trigger_family, &statement) {
                Some(prev) => (prev.condition_id, prev.supersedes, None, true),
                None => (uuid::Uuid::new_v4().to_string(), None, None, false),
            }
        }
    };

    // The tripped / fired claim: honored only where the engine confirmed a crossing
    // for this same (carried) condition; a qualitative claim needs a source-backed
    // finding — none exists while the research loop is unbuilt — so it is cleared
    // and logged. The ledger cannot be quietly rewritten to fit a new verdict.
    let tripped = if claimed {
        if quant.is_some() && carried && confirmed_ids.contains(&condition_id) {
            true
        } else {
            let reason = if quant.is_none() {
                "no source-backed research finding supports the claim"
            } else {
                "no confirmed engine crossing supports the claim"
            };
            audit
                .rejected_claims
                .push(format!("'{statement}': {reason}"));
            false
        }
    } else {
        false
    };

    LedgerCondition {
        condition_id,
        role,
        trigger_family,
        statement,
        quant,
        downgraded_reason,
        technology_class,
        tripped,
        supersedes,
        eval_state,
    }
}

/// Validate the model's rewritten ledger into the persisted [`ThesisLedger`] — the
/// ledger legs of the Step-6g continuity check (`docs/portfolio-workflow.md`
/// §Step 6g). The app owns everything structural: condition ids and what carries
/// across the rewrite (decided here, never asserted by the model), the
/// executability downgrades, the tripped / fired validation against the engine's
/// crossings, the engine scenario targets stamped into the monitor (with spot's
/// authoring-time band relation beside them, so the quick check's outside-band
/// flag fires on a change rather than the standing state), the branch's
/// reductions, and the acknowledgment stamp on each consumed confirmed crossing.
pub fn validate_ledger_rewrite(
    draft: &LedgerDraft,
    prior: Option<&ThesisLedger>,
    evaluation: Option<&LedgerEvaluation>,
    branch: LedgerBranch,
    is_fund: bool,
    engine_targets: Option<&PriceTarget>,
    spot: Option<f64>,
) -> (ThesisLedger, LedgerAudit) {
    // Structural, not conventional: a `role_risk_only` monitor is condition-only —
    // no engine scenario target exists on that branch — regardless of what the
    // call site passed (`docs/portfolio-analysis.md` §The position thesis ledger).
    let engine_targets = if branch == LedgerBranch::RoleRiskOnly {
        None
    } else {
        engine_targets
    };
    let mut audit = LedgerAudit::default();
    if let Some(eval) = evaluation {
        audit.crossings = eval.crossings.clone();
        audit.unevaluable = eval.unevaluable.clone();
    }
    let confirmed_ids: std::collections::HashSet<String> = evaluation
        .map(|e| {
            e.crossings
                .iter()
                .filter(|c| c.outcome == CrossingOutcome::Confirmed)
                .map(|c| c.condition_id.clone())
                .collect()
        })
        .unwrap_or_default();
    let updated_states: std::collections::HashMap<String, ConditionEvalState> = evaluation
        .map(|e| e.updated_states.iter().cloned().collect())
        .unwrap_or_default();

    let mut prior_pool: Vec<LedgerCondition> =
        prior.map(|p| p.conditions.clone()).unwrap_or_default();
    let mut conditions: Vec<LedgerCondition> = Vec::new();

    // The dedup / identity key for one draft condition — the parsed machine core
    // (quantitative) or the trimmed statement (qualitative), per role — and per
    // family for triggers, since trim-vs-sell on one core are distinct
    // pre-commitments.
    let dedup_key = |role: ConditionRole,
                     family: Option<TriggerFamily>,
                     quant_draft: Option<&QuantCoreDraft>,
                     statement: &str| {
        match quant_draft.and_then(|qd| parse_quant_core(qd, is_fund).ok()) {
            Some(core) => format!(
                "{role:?}|{family:?}|{}|{}|{}|{}",
                core.series.as_kebab(),
                core.comparator.as_kebab(),
                core.threshold,
                core.margin
            ),
            None => format!("{role:?}|{family:?}|qual|{}", statement.trim()),
        }
    };
    // Parse a trigger's family claim (the main loop enforces the branch rules).
    let parse_family = |family: &str| match family.trim() {
        "add" => Some(TriggerFamily::Add),
        "trim" => Some(TriggerFamily::Trim),
        "sell" => Some(TriggerFamily::Sell),
        _ => None,
    };

    // Pre-pass over the draft's quantitative conditions: resolve every exact-core
    // match globally first (an unchanged core always carries), then assign the
    // remaining **changed** cores to the remaining prior conditions by a global
    // minimum-cost matching ([`assign_supersessions`]) — so lineage is
    // order-independent: a changed condition emitted first can neither consume an
    // unchanged sibling a later draft still carries, nor claim another changed
    // sibling's nearest ancestor.
    let mut reserved: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut pre_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut changed: Vec<(ConditionRole, Option<TriggerFamily>, QuantCore, String)> = Vec::new();
    let pre_pass_rows = draft
        .falsifiers
        .iter()
        .map(|f| {
            (
                ConditionRole::Falsifier,
                None,
                f.quant.as_ref(),
                f.statement.as_str(),
            )
        })
        .chain(draft.triggers.iter().map(|t| {
            (
                ConditionRole::Trigger,
                parse_family(&t.family),
                t.quant.as_ref(),
                t.statement.as_str(),
            )
        }));
    for (role, family, quant_draft, statement) in pre_pass_rows {
        // Mirror the main loop's trigger skip rules, so a rejected trigger
        // neither reserves nor assigns.
        if role == ConditionRole::Trigger
            && (family.is_none()
                || (family == Some(TriggerFamily::Add) && branch == LedgerBranch::RoleRiskOnly))
        {
            continue;
        }
        let Some(core) = quant_draft.and_then(|qd| parse_quant_core(qd, is_fund).ok()) else {
            continue;
        };
        let key = dedup_key(role, family, quant_draft, statement);
        if !pre_seen.insert(key.clone()) {
            continue; // the main loop drops this duplicate too
        }
        if let Some(prev) = prior_pool.iter().find(|c| {
            c.role == role
                && c.trigger_family == family
                && c.quant.as_ref() == Some(&core)
                && !reserved.contains(&c.condition_id)
        }) {
            reserved.insert(prev.condition_id.clone());
        } else {
            changed.push((role, family, core, key));
        }
    }
    let assigned = assign_supersessions(&changed, &prior_pool, &reserved);

    // Dedup guard: a repetitive model returning the same condition twice must not
    // pad the ledger — the second copy is dropped and logged *before* it can touch
    // the prior pool.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for f in &draft.falsifiers {
        let key = dedup_key(ConditionRole::Falsifier, None, f.quant.as_ref(), &f.statement);
        if !seen.insert(key.clone()) {
            audit.duplicates.push(format!(
                "falsifier '{}' dropped as a duplicate of one already validated",
                f.statement.trim()
            ));
            continue;
        }
        conditions.push(validate_condition(
            &f.statement,
            ConditionRole::Falsifier,
            None,
            f.quant.as_ref(),
            is_fund,
            f.technology_class,
            f.tripped,
            &mut prior_pool,
            assigned.get(&key).map(String::as_str),
            &confirmed_ids,
            &updated_states,
            &mut audit,
        ));
    }
    for t in &draft.triggers {
        let family = parse_family(&t.family);
        // The branch reduction: a `role_risk_only` ledger's triggers are trim /
        // sell only — an add trigger would pre-commit to an action its feasible
        // set never offers (`docs/portfolio-analysis.md` §The position thesis
        // ledger). Rejected and logged, like an unparseable family.
        match family {
            Some(TriggerFamily::Add) if branch == LedgerBranch::RoleRiskOnly => {
                audit.rejected_claims.push(format!(
                    "add trigger '{}' rejected on the role_risk_only branch (trim/sell spine)",
                    t.statement.trim()
                ));
                continue;
            }
            None => {
                audit.rejected_claims.push(format!(
                    "trigger '{}' rejected: family '{}' is not add/trim/sell",
                    t.statement.trim(),
                    t.family
                ));
                continue;
            }
            Some(_) => {}
        }
        let key = dedup_key(ConditionRole::Trigger, family, t.quant.as_ref(), &t.statement);
        if !seen.insert(key.clone()) {
            audit.duplicates.push(format!(
                "trigger '{}' dropped as a duplicate of one already validated",
                t.statement.trim()
            ));
            continue;
        }
        conditions.push(validate_condition(
            &t.statement,
            ConditionRole::Trigger,
            family,
            t.quant.as_ref(),
            is_fund,
            false,
            t.fired,
            &mut prior_pool,
            assigned.get(&key).map(String::as_str),
            &confirmed_ids,
            &updated_states,
            &mut audit,
        ));
    }

    // Prior conditions the rewrite removed close **whole** into the audit record —
    // statement, core, and accumulated state (as of this run's evaluation)
    // preserved, never silently lost.
    for mut removed in prior_pool {
        removed.eval_state = updated_states
            .get(&removed.condition_id)
            .cloned()
            .or(removed.eval_state);
        audit.closed.push(ClosedCondition {
            superseded_by: None,
            condition: removed,
        });
    }

    // The acknowledgment transition (`docs/portfolio-workflow.md` §Step 6g): the
    // full pass consumed this evaluation as continuity input, so each confirmed
    // crossing's observation is stamped acknowledging — the same breach cannot
    // re-raise off the observation this pass already examined.
    for crossing in &audit.crossings {
        if crossing.outcome != CrossingOutcome::Confirmed {
            continue;
        }
        if let Some(cond) = conditions
            .iter_mut()
            .find(|c| c.condition_id == crossing.condition_id)
        {
            if let Some(state) = cond.eval_state.as_mut() {
                state.acknowledged_observation_id = Some(crossing.observation_id.clone());
            }
        }
    }

    // Key drivers: the series tie is a claim, validated like any other
    // (unresolvable → the driver keeps its name, untied, logged).
    let key_drivers: Vec<KeyDriver> = draft
        .key_drivers
        .iter()
        .map(|d| {
            let series = match &d.series {
                None => None,
                Some(claim) => match engine::LedgerSeries::parse(claim) {
                    Some(s) => Some(s),
                    None => {
                        audit.downgraded.push(format!(
                            "key driver '{}': series '{claim}' does not resolve — left untied",
                            d.name
                        ));
                        None
                    }
                },
            };
            KeyDriver {
                name: d.name.trim().to_string(),
                series,
            }
        })
        .collect();

    // The monitor: model conditions + probability leans; the engine's own scenario
    // targets stamped by the app (`None` on the condition-only role_risk branch).
    let clamp_pct = |p: f64| if p.is_finite() { p.clamp(0.0, 100.0) } else { 0.0 };
    let monitor = vec![
        MonitorScenario {
            scenario: ScenarioKind::Bear,
            conditions: draft.bear.conditions.trim().to_string(),
            probability_pct: clamp_pct(draft.bear.probability_pct),
            engine_target: engine_targets.map(|t| t.bear),
        },
        MonitorScenario {
            scenario: ScenarioKind::Base,
            conditions: draft.base.conditions.trim().to_string(),
            probability_pct: clamp_pct(draft.base.probability_pct),
            engine_target: engine_targets.map(|t| t.base),
        },
        MonitorScenario {
            scenario: ScenarioKind::Bull,
            conditions: draft.bull.conditions.trim().to_string(),
            probability_pct: clamp_pct(draft.bull.probability_pct),
            engine_target: engine_targets.map(|t| t.bull),
        },
    ];

    // Spot's authoring-time relation to the stamped band — `None` wherever no band
    // exists (the role_risk branch forced `engine_targets` to `None` above, and a
    // missing spot stamps nothing rather than guessing).
    let authored_band_relation = match (spot, engine_targets) {
        (Some(spot), Some(t)) => Some(crate::portfolio::BandRelation::of(spot, t.bear, t.bull)),
        _ => None,
    };

    let current_thesis = draft.thesis.trim().to_string();
    let ledger = ThesisLedger {
        branch,
        // The original thesis is frozen at debut and carried immutable thereafter —
        // drift stays legible (`docs/portfolio-analysis.md` §The position thesis
        // ledger).
        original_thesis: prior
            .map(|p| p.original_thesis.clone())
            .unwrap_or_else(|| current_thesis.clone()),
        current_thesis,
        key_drivers,
        monitor,
        what_must_improve: draft.what_must_improve.trim().to_string(),
        what_must_not_break: draft.what_must_not_break.trim().to_string(),
        conditions,
        authored_band_relation,
    };
    (ledger, audit)
}

/// Render the Step-6a semantic continuity recall — prompt fragments from this
/// job's own memory partition (`docs/portfolio-workflow.md` §Step 6a). Nothing
/// renders when no hit came back (a debut-empty partition, a failed lane — the
/// gap rides the audit's degraded inputs instead).
fn semantic_recall_prompt_section(d: &HoldingDossier) -> String {
    if d.semantic_recall.hits.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\nPRIOR ANALYSIS RECALL (semantic, from this job's own memory — \
         continuity context, not fresh evidence):\n",
    );
    for h in &d.semantic_recall.hits {
        s.push_str(&format!("- {h}\n"));
    }
    s
}

// ---- The what-changed attribution (the metric-level 6g validator) ----------------

/// Format one optional value for an input-delta label — `(absent)` where that run
/// could not compute the metric, never a fabricated zero.
fn delta_val(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.4}"))
        .unwrap_or_else(|| "(absent)".to_string())
}

/// Append one input-delta entry, assigning the next bracketed id.
fn push_delta(entries: &mut Vec<crate::portfolio::DeltaEntry>, label: String) {
    let id = format!("D{}", entries.len() + 1);
    entries.push(crate::portfolio::DeltaEntry { id, label });
}

/// The delta entries both branches share: the position delta, this run's ledger
/// crossings, and the house view where the prompt renders one.
fn append_shared_delta(
    entries: &mut Vec<crate::portfolio::DeltaEntry>,
    dossier: &HoldingDossier,
    position_change: PositionChange,
    ledger_eval: Option<&LedgerEvaluation>,
) {
    if position_change != PositionChange::Unchanged {
        let move_word = match position_change {
            PositionChange::New => "new",
            PositionChange::Increased => "increased",
            PositionChange::Decreased => "decreased",
            PositionChange::Unchanged => unreachable!("guarded above"),
        };
        push_delta(entries, format!("position {move_word} since the prior run"));
    }
    if let Some(eval) = ledger_eval {
        for c in &eval.crossings {
            let role = match c.role {
                crate::portfolio::ConditionRole::Falsifier => "falsifier",
                crate::portfolio::ConditionRole::Trigger => "trigger",
            };
            let outcome = match c.outcome {
                CrossingOutcome::Confirmed => "confirmed",
                CrossingOutcome::FirstBreach => "first breach",
            };
            push_delta(
                entries,
                format!(
                    "ledger {role} '{}' {outcome}: observed {:.4} vs threshold {:.4}",
                    c.statement, c.observed_value, c.threshold
                ),
            );
        }
    }
    if dossier.house_view.latest_sections.is_some() {
        push_delta(
            entries,
            "house view: the latest Market Signal report rendered this run".to_string(),
        );
    }
}

/// Assemble the priced holding's rendered **input delta**
/// (`docs/portfolio-workflow.md` §Step 6g): the concrete, engine-computed changes
/// since the prior read, each with a stable bracketed id the what-changed rows
/// cite as evidence. Empty on a debut — nothing to attribute against. Resolution
/// downstream is exact `old ≠ new` (ruled 2026-08-21): stored numerics round-trip
/// bit-exact, so any difference is a real entry.
#[allow(clippy::too_many_arguments)]
fn priced_input_delta(
    dossier: &HoldingDossier,
    engine_output: &EngineOutput,
    position_change: PositionChange,
    ledger_eval: Option<&LedgerEvaluation>,
    tech_pre_flag: Option<&engine::TechEventPreFlag>,
    narrative: Option<&engine::NarrativeRead>,
    hard_forensic: bool,
) -> Vec<crate::portfolio::DeltaEntry> {
    let Some(prior) = dossier.prior_verdict.as_ref() else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    if let (Some(old), Some(new)) = (dossier.prior_spot, dossier.financials.current_price) {
        if old != new {
            push_delta(&mut entries, format!("spot: {old:.2} -> {new:.2}"));
        }
    }
    if let Some(prior_metrics) = dossier.prior_metrics.as_ref() {
        for c in engine::metric_delta(prior_metrics, &engine_output.metrics) {
            push_delta(
                &mut entries,
                format!("metric {}: {} -> {}", c.name, delta_val(c.old), delta_val(c.new)),
            );
        }
    }
    if let VerdictDisposition::Priced(pg) = &prior.disposition {
        let axes = [
            ("quality", pg.sub_scores.quality, engine_output.sub_scores.quality),
            ("valuation", pg.sub_scores.valuation, engine_output.sub_scores.valuation),
            ("momentum", pg.sub_scores.momentum, engine_output.sub_scores.momentum),
            ("risk", pg.sub_scores.risk, engine_output.sub_scores.risk),
        ];
        for (name, old, new) in axes {
            if old != new {
                push_delta(
                    &mut entries,
                    format!("engine sub-score {name}: {old:.0} -> {new:.0}"),
                );
            }
        }
        if pg.grade != engine_output.grade {
            push_delta(
                &mut entries,
                format!(
                    "engine grade: {} -> {}",
                    pg.grade.as_str(),
                    engine_output.grade.as_str()
                ),
            );
        }
        let old_base = pg.price_targets.twelve_month.as_ref().map(|t| t.base);
        let new_base = engine_output.price_targets.twelve_month.as_ref().map(|t| t.base);
        if old_base != new_base {
            push_delta(
                &mut entries,
                format!(
                    "engine twelve-month base target: {} -> {}",
                    delta_val(old_base),
                    delta_val(new_base)
                ),
            );
        }
        if pg.risk_tier != Some(engine_output.risk_tier) {
            push_delta(
                &mut entries,
                format!(
                    "risk tier: {} -> {}",
                    pg.risk_tier.map(|t| t.as_str()).unwrap_or("(absent)"),
                    engine_output.risk_tier.as_str()
                ),
            );
        }
        if pg.dead_money != Some(engine_output.hurdle.state) {
            push_delta(
                &mut entries,
                format!(
                    "capital-efficiency read: {:?} -> {:?}",
                    pg.dead_money, engine_output.hurdle.state
                ),
            );
        }
    }
    if dossier.prior_grade_parameter_version.as_deref() != Some(engine::GRADE_PARAMETER_VERSION) {
        push_delta(
            &mut entries,
            format!(
                "grade bands recalibrated ({} -> {}) — letters can move with no input change",
                dossier
                    .prior_grade_parameter_version
                    .as_deref()
                    .unwrap_or("pre-stamp"),
                engine::GRADE_PARAMETER_VERSION
            ),
        );
    }
    append_shared_delta(&mut entries, dossier, position_change, ledger_eval);
    if let Some(f) = tech_pre_flag.filter(|f| f.fired) {
        push_delta(
            &mut entries,
            format!(
                "technology-event pre-flag fired ({:+.1}% vs {} over {} sessions)",
                f.relative_move * 100.0,
                f.benchmark,
                f.sessions
            ),
        );
    }
    if let Some(n) = narrative {
        push_delta(
            &mut entries,
            format!(
                "narrative-vs-reality read: ratio {}{}",
                n.ratio.map(|r| format!("{r:.2}")).unwrap_or_else(|| "(unbounded)".to_string()),
                if n.hype_capped() { " (hype cap tripped)" } else { "" }
            ),
        );
    }
    if hard_forensic {
        push_delta(
            &mut entries,
            "hard forensic filing event (item-classified restatement / auditor change)"
                .to_string(),
        );
    }
    entries
}

/// The `role_risk_only` branch's reduced input delta: the position delta, the
/// branch's computed-surface metric moves, ledger crossings, and the house view.
fn role_risk_input_delta(
    dossier: &HoldingDossier,
    fund_metrics: &engine::ComputedMetrics,
    position_change: PositionChange,
    ledger_eval: Option<&LedgerEvaluation>,
) -> Vec<crate::portfolio::DeltaEntry> {
    if dossier.prior_verdict.is_none() {
        return Vec::new();
    }
    let mut entries = Vec::new();
    if let Some(prior_metrics) = dossier.prior_metrics.as_ref() {
        for c in engine::metric_delta(prior_metrics, fund_metrics) {
            push_delta(
                &mut entries,
                format!("metric {}: {} -> {}", c.name, delta_val(c.old), delta_val(c.new)),
            );
        }
    }
    append_shared_delta(&mut entries, dossier, position_change, ledger_eval);
    entries
}

/// Render the input delta and the attribution rules into the user prompt — the
/// bracketed ids are the `what_changed_entries` evidence vocabulary.
fn input_delta_prompt_section(entries: &[crate::portfolio::DeltaEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\nINPUT DELTA (the concrete changes since the prior read — the evidence \
         ids for what_changed_entries):\n",
    );
    for e in entries {
        s.push_str(&format!("[{}] {}\n", e.id, e.label));
    }
    s.push_str(
        "WHAT_CHANGED_ENTRIES: author one typed row per moved intrinsic value \
         (old -> new). Every external attribution (market-data / \
         company-information / research-narrative) must cite one id above (e.g. \
         \"D2\") — or the entry's label verbatim — in `evidence`; a row whose \
         evidence resolves to no entry is downgraded to self-correction with a \
         logged reason. A row whose old and new are identical, or a duplicate \
         of another row, is dropped. Use attribution `self-correction` \
         (evidence empty) when you are revising your own prior read without new \
         facts. Author a `thesis` or `scenario-weights` row ONLY when the \
         standing thesis itself materially changed — never for a rephrasing.\n",
    );
    s
}

/// The 6g **what-changed attribution validator**
/// (`docs/portfolio-workflow.md` §Step 6g): every row the model labels external
/// must resolve to a concrete entry in the rendered input delta — by bracketed id
/// or label verbatim — or it is **downgraded to self-correction with a logged
/// reason** (ruled 2026-08-21; the research-finding and
/// `research_forward_assumption` legs join when the research loop lands, so today
/// the delta entries are the whole evidence surface). Two structural drops run
/// first — deterministic string comparisons, no appraisal of the model's prose:
/// a row whose `old` and `new` agree claims no movement, and an exact duplicate
/// of an already-kept row restates a move already counted; either is dropped
/// with a logged reason, so neither can open a thesis-change episode or inflate
/// the self-correction count. The returned audit carries the two signals
/// outcome learning consumes: the post-validation self-correction count and the
/// standing-thesis flag (a resolved external thesis / scenario-weights row, or
/// any self-correction).
pub(crate) fn validate_what_changed(
    authored: &[crate::portfolio::WhatChangedEntry],
    input_delta: Vec<crate::portfolio::DeltaEntry>,
) -> crate::portfolio::WhatChangedAudit {
    use crate::portfolio::{ChangeAttribution, ChangedValueKind};
    let resolves = |evidence: &str| {
        let e = evidence.trim();
        if e.is_empty() {
            return false;
        }
        let head = e
            .trim_start_matches('[')
            .split(|c: char| c.is_whitespace() || c == ':' || c == ',' || c == ']')
            .next()
            .unwrap_or("");
        input_delta
            .iter()
            .any(|d| d.id.eq_ignore_ascii_case(head) || d.label.eq_ignore_ascii_case(e))
    };
    let mut entries = Vec::with_capacity(authored.len());
    let mut downgrades = Vec::new();
    let mut self_correction_count = 0u32;
    let mut thesis_changed = false;
    let mut kept: Vec<&crate::portfolio::WhatChangedEntry> = Vec::new();
    for row in authored {
        if row.old.trim() == row.new.trim() {
            downgrades.push(format!(
                "{:?} '{}': old and new agree ({:?}) — dropped, no movement claimed",
                row.kind, row.detail, row.old
            ));
            continue;
        }
        if kept.contains(&row) {
            downgrades.push(format!(
                "{:?} '{}' ({} -> {}): exact duplicate row — dropped",
                row.kind, row.detail, row.old, row.new
            ));
            continue;
        }
        kept.push(row);
        let mut row = row.clone();
        if row.attribution != ChangeAttribution::SelfCorrection && !resolves(&row.evidence) {
            downgrades.push(format!(
                "{:?} '{}' ({} -> {}): claimed {} evidence {:?} resolves to no \
                 input-delta entry — downgraded to self-correction",
                row.kind,
                row.detail,
                row.old,
                row.new,
                row.attribution.as_str(),
                row.evidence
            ));
            row.attribution = ChangeAttribution::SelfCorrection;
        }
        if row.attribution == ChangeAttribution::SelfCorrection {
            self_correction_count += 1;
            thesis_changed = true;
        } else if matches!(
            row.kind,
            ChangedValueKind::Thesis | ChangedValueKind::ScenarioWeights
        ) {
            thesis_changed = true;
        }
        entries.push(row);
    }
    crate::portfolio::WhatChangedAudit {
        entries,
        input_delta,
        downgrades,
        self_correction_count,
        thesis_changed,
    }
}

// ---- Prompt construction (pure, testable) ------------------------------------

/// The system prompt for the interpretation stage — the role and the two-arm
/// contract: the engine arm's numbers are the app's, the model arm's are the
/// model's own, and the model arm's values never alter or bind the engine
/// baseline.
pub fn interpretation_system_prompt() -> String {
    format!(
        "You are a disciplined equity analyst grading one holding for a prescriptive \
     portfolio review. The verdict has TWO ARMS. The ENGINE ARM — sub-scores, the \
     composite grade, valuation multiples, the risk tier, the capital-efficiency \
     read, and the scenario price targets — has already been computed \
     deterministically and is given to you as the baseline: a disclosed calculator, \
     evidence to weigh, never numbers you must adopt. The MODEL ARM is yours to \
     author, unrestricted: your OWN four sub-scores on the same 0-100 higher-is-better \
     scale (momentum stays outside the letter; your letter is derived from your \
     quality/valuation/risk through the same cutoffs), your OWN one-month and \
     twelve-month price target bands (base, bear, bull — your numbers, free to \
     depart the engine's as far as the evidence takes you), your conviction, and the \
     three horizon reads. Do NOT choose a portfolio action here — a dedicated \
     decision stage sets it afterward from this verdict; your job is the read \
     itself. Both arms are scored against realized outcomes by a deterministic \
     scoreboard; where a RETROSPECTIVE block appears, assess your prior read against \
     the engine baseline and what actually happened — honestly, in self_assessment — \
     and let it discipline this run's numbers. Conviction means your confidence in \
     the overall read — your scores and outlook together — is exactly one of \
     'low' / 'medium' / 'high' (no numbers or percentages). On every sub-score axis \
     a HIGHER number is BETTER — a \
     high risk score means resilience (low risk), not exposure. \
     Use the Market Signal house view for the horizon reads and market-setup context \
     only — it is a market-level thesis, never by itself a reason to exit a specific \
     holding. The read is profile-independent — no investor profile is given at this \
     stage; it enters at the action decision only. \
     You also maintain the position's THESIS LEDGER — the persisted standing thesis \
     with monitorable falsifiers and pre-committed action triggers: test the prior \
     ledger against this run's evidence and the engine's deterministic condition \
     crossings, then rewrite it per the instructions in the prompt. \
     {}",
        crate::portfolio::interpretation_response_contract()
    )
}

/// The system prompt for the `role_risk_only` interpretation — the union's other
/// branch: role and risk only, no letter, no targets, no conviction.
pub fn role_risk_system_prompt() -> String {
    format!(
        "You are a disciplined portfolio analyst assessing one holding whose vehicle \
     class this pipeline is structurally unable to price (a bond or commodity fund, \
     an equity fund below the US-exposure guard, a leveraged/inverse vehicle, or a \
     fund without usable weightings). \
     Do NOT produce a grade, price target, conviction, or action — none exists for \
     this branch here (its action is set afterward by a dedicated decision stage; \
     the engine arm's set for this branch is sell-all / trim / hold, rendered there \
     as its own read — the decision is structurally open, an outside-the-set rung \
     recorded as an annotation). Your job: \
     describe the vehicle's role — the mandate and the exposure it exists to supply, \
     read in isolation — and write the continuity note. Read the engine's exposure, \
     expense, and risk figures; never invent one. \
     You also maintain the holding's THESIS LEDGER (fund-flavored drivers; \
     condition-only monitor; trim/sell triggers only) — test the prior ledger against \
     this run's evidence and rewrite it per the instructions in the prompt. \
     {}",
        crate::portfolio::role_risk_response_contract()
    )
}

/// The user prompt for the `role_risk_only` interpretation: the engine's typed
/// readout rendered for the model.
/// Whether [`role_risk_user_prompt`] will render any house-view content for this
/// dossier — the **latest sections only**. Defined here, beside the render below, so
/// the audit's house-view source claim cannot drift from what the prompt actually
/// carries: this branch never renders the recent stances, so a summary-only house view
/// (reachable whenever the latest report's Markdown is missing or unreadable, which
/// `load_house_view` degrades to deliberately) reaches a role/risk verdict as nothing.
pub(crate) fn role_risk_prompt_renders_house_view(d: &HoldingDossier) -> bool {
    d.house_view.latest_sections.is_some()
}

/// The prompt's holding header — the identity and position line both interpretation
/// branches open with. Two renderings are deliberate
/// (`docs/verification/2026-08-10-big-run-attempt-1.md` §Finding 4). The name falls
/// back to the resolved listing's company name when Schwab supplies no description,
/// which otherwise renders as `HOLDING: PSX ()` and leaves the model speculating
/// about the ticker. The money figures are marked as position totals in dollars:
/// rendered bare they read ambiguously as per-share, and the model spent reasoning
/// re-deriving them by division before starting its analysis.
fn holding_header(d: &HoldingDossier) -> String {
    let described = d.position.description.trim();
    let name = if !crate::portfolio::listing::describes_issuer(described, &d.position.symbol) {
        // The fallback is held to a *canonical-source* standard, deliberately
        // looser than the description's: FMP's parser accepts any non-blank
        // `companyName`, so bare-ticker and tokenless noise are rejected — but a
        // ticker-token-only LEGAL name ("ASML Holding N.V.", "eBay Inc.") is
        // real identity from a canonical source, and holding it to the
        // description's stricter rule starved ticker-named issuers of any name
        // at all (combined-range review). Funds have no profile call, so their
        // identity rides the fund data's own name — the role-risk branch's only
        // naming source.
        d.company_name
            .as_deref()
            .or_else(|| d.fund.as_ref().and_then(|f| f.fund.name.as_deref()))
            .map(str::trim)
            .filter(|n| {
                crate::portfolio::listing::displayable_source_name(n, &d.position.symbol)
            })
            .unwrap_or("name unavailable")
    } else {
        described
    };
    format!(
        "HOLDING: {} ({})\nQuantity: {}  Cost basis: ${:.0} total  Market value: ${:.0} total\n",
        d.position.symbol,
        name,
        d.position.quantity,
        d.position.cost_basis,
        d.position.market_value,
    )
}

pub fn role_risk_user_prompt(input: &RoleRiskInput) -> String {
    let d = input.dossier;
    let r = input.readout;
    let mut p = String::new();
    p.push_str(&holding_header(d));
    p.push_str(&format!(
        "Position change since last run: {}\n",
        describe_position_change(&d.position_delta, d.position.quantity, d.position.cost_basis)
    ));
    p.push_str(&format!("\nCLASSIFICATION: {}\n", r.class_label));
    if r.structural_flag {
        p.push_str("STRUCTURAL FLAG: structurally path-dependent (leveraged/inverse)\n");
    }
    if !r.exposure_tilt.is_empty() {
        p.push_str("EXPOSURE TILT:\n");
        for (label, weight) in &r.exposure_tilt {
            p.push_str(&format!("- {label}: {:.1}%\n", weight * 100.0));
        }
    }
    p.push_str(&format!(
        "EXPENSE RATIO (decimal fraction of assets per year; 0.0075 = 0.75%/yr): {}\n\
         OBSERVABLE RISK (annualized volatility): {}\n",
        opt(r.expense_ratio),
        opt(r.observable_risk),
    ));
    if !r.evidence_gaps.is_empty() {
        p.push_str(&format!("EVIDENCE GAPS: {}\n", r.evidence_gaps.join("; ")));
    }
    // The commodity / macro classes this branch types are exactly where the
    // underlying-positioning read carries signal (`docs/data-sources.md §CFTC`).
    if let Some(f) = &d.fund {
        p.push_str(&positioning_prompt_section(f));
    }
    p.push_str(&put_call_backdrop_prompt_section(d));
    if let Some(sections) = &d.house_view.latest_sections {
        p.push_str(&format!(
            "\nMARKET SIGNAL HOUSE VIEW (latest report — scope: market-setup context \
             only, never by itself a reason to exit this holding):\n{sections}\n"
        ));
    }
    p.push_str(
        "\nACTION: none here — this branch's action is decided afterward by the \
         dedicated per-holding action call. The engine arm's set for this branch: \
         sell-all / trim / hold (no add family); the decision is structurally \
         open, a departure recorded as an audit annotation.\n",
    );
    match &d.prior_verdict {
        Some(_) => {
            p.push_str(
                "\nCONTINUITY: a prior verdict for this holding exists. Keep the read firm; \
                 say what changed.\n",
            );
            p.push_str(&semantic_recall_prompt_section(d));
            // The rendered input delta — the what_changed_entries evidence ids
            // the 6g attribution validator resolves against.
            p.push_str(&input_delta_prompt_section(input.input_delta));
        }
        None => p.push_str(
            "\nCONTINUITY: new holding (no prior verdict). what_changed_entries \
             must be [].\n",
        ),
    }
    p.push_str(&ledger_prompt_section(
        d.prior_ledger(),
        input.ledger_eval,
        true,
    ));
    p
}

/// Render the v7 retrospective block: the prior run's both-arm values, the price
/// move since, and any matured scoreboard lines — the input the self-assessment
/// reads against (`docs/portfolio-analysis.md` §The holding verdict; a deliberate
/// reversal of the v4 anchoring guard). Empty when the prior verdict carries no
/// priced body to compare.
fn retrospective_prompt_section(d: &HoldingDossier) -> String {
    let Some(prior) = &d.prior_verdict else {
        return String::new();
    };
    let VerdictDisposition::Priced(g) = &prior.disposition else {
        return "\nRETROSPECTIVE: the prior verdict was not a priced read (role/risk-only \
                or an abstention), so there are no prior arms to compare.\n"
            .to_string();
    };
    let mut p = String::new();
    let since = d
        .prior_vintage
        .as_deref()
        .map(|t| format!(" (prior read {t})"))
        .unwrap_or_default();
    p.push_str(&format!("\nRETROSPECTIVE{since}:\n"));

    let outlook = |o: &HorizonOutlook| {
        format!(
            "outlook s/m/l {:?}/{:?}/{:?}",
            o.short, o.mid, o.long
        )
        .to_lowercase()
    };
    let engine_targets = {
        let t12 = g.price_targets.twelve_month.as_ref().map(|t| {
            format!("12-mo base {:.2} [{:.2}\u{2013}{:.2}]", t.base, t.bear, t.bull)
        });
        let t1 = g.price_targets.one_month.as_ref().map(|t| {
            format!("1-mo base {:.2} [{:.2}\u{2013}{:.2}]", t.base, t.bear, t.bull)
        });
        [t1, t12].into_iter().flatten().collect::<Vec<_>>().join(", ")
    };
    let ev = &g.engine_view;
    let engine_rest = format!(
        "conviction {:?}, {}, action {}",
        ev.conviction,
        outlook(&ev.outlook),
        ev.action.as_kebab()
    )
    .to_lowercase();
    p.push_str(&format!(
        "- prior ENGINE arm: grade {} (q {:.0} / v {:.0} / r {:.0}; momentum {:.0}); {}; {}\n",
        g.grade.as_str(),
        g.sub_scores.quality,
        g.sub_scores.valuation,
        g.sub_scores.risk,
        g.sub_scores.momentum,
        if engine_targets.is_empty() {
            "targets (gap)".to_string()
        } else {
            engine_targets
        },
        engine_rest,
    ));

    {
        let mv = &g.model_view;
        let mt = &mv.price_targets;
        p.push_str(&format!(
            "- prior MODEL arm (yours): letter {} (q {:.0} / v {:.0} / m {:.0} / r {:.0}); \
             1-mo base {:.2} [{:.2}\u{2013}{:.2}], 12-mo base {:.2} [{:.2}\u{2013}{:.2}]; \
             conviction {:?}, {}, action {}\n",
            mv.letter.as_str(),
            mv.sub_scores.quality,
            mv.sub_scores.valuation,
            mv.sub_scores.momentum,
            mv.sub_scores.risk,
            mt.one_month.base,
            mt.one_month.bear,
            mt.one_month.bull,
            mt.twelve_month.base,
            mt.twelve_month.bear,
            mt.twelve_month.bull,
            g.conviction,
            outlook(&g.horizon_outlook),
            g.action.as_kebab(),
        ));
    }

    if let Some(spot) = d.financials.current_price {
        // Every prior-basis price comparison crosses to today's basis through
        // the anchor-close bridge — the outcome slice's split-safe contract
        // (`docs/portfolio-analysis.md` §Outcome learning) keyed on the prior
        // read's vintage session. A raw prior-spot ratio would report a 2:1
        // split as a ~-50% "realized" move (Codex round 2, finding 1); no
        // anchor bar within the proximity bound → the comparison is excluded,
        // never guessed. The target-distance reads stay labeled as exactly
        // that: distance to the old targets, never a realized return.
        let anchor_close = d
            .prior_vintage
            .as_deref()
            // The vintage instant's ET session date, matching the outcome
            // slice's anchor dating — a UTC date prefix would key an evening-ET
            // vintage to a session traded entirely after the prior read.
            .and_then(crate::market_clock::et_date_of)
            .and_then(|day| {
                crate::portfolio::outcome::anchor_session_close(&d.financials.daily_closes, day)
            })
            .map(|b| b.value)
            .filter(|c| *c > 0.0);
        match anchor_close {
            Some(anchor) => {
                let mut vs: Vec<String> = vec![format!(
                    "{:+.1}% realized since the prior read (anchor close {:.2}{})",
                    (spot / anchor - 1.0) * 100.0,
                    anchor,
                    d.prior_spot
                        .filter(|s| *s > 0.0)
                        .map(|s| format!("; authoring spot {s:.2} on its own basis"))
                        .unwrap_or_default(),
                )];
                // The prior authored targets are on the prior read's basis:
                // bridge them (`target × anchor ⁄ authoring spot`) before taking
                // a distance, so a split can't fabricate one. No authoring spot →
                // no bridge → the distances are excluded, not guessed.
                if let Some(prior_spot) = d.prior_spot.filter(|s| *s > 0.0) {
                    let bridge = anchor / prior_spot;
                    if let Some(t) = g.price_targets.twelve_month.as_ref() {
                        if t.base > 0.0 {
                            vs.push(format!(
                                "distance to the prior engine 12-mo base {:+.1}% (basis-bridged)",
                                (spot / (t.base * bridge) - 1.0) * 100.0
                            ));
                        }
                    }
                    let b = g.model_view.price_targets.twelve_month.base;
                    if b > 0.0 {
                        vs.push(format!(
                            "distance to the prior model 12-mo base {:+.1}% (basis-bridged)",
                            (spot / (b * bridge) - 1.0) * 100.0
                        ));
                    }
                }
                p.push_str(&format!(
                    "- price now {:.2}: {} (split-safe via the anchor-close bridge; \
                     the scored comparison is the deterministic scoreboard's)\n",
                    spot,
                    vs.join("; ")
                ));
            }
            None => p.push_str(&format!(
                "- price now {:.2}: prior-read price comparison unavailable — no \
                 anchor-session close at the prior vintage (excluded rather than \
                 guessed; the deterministic scoreboard stays the scored ground)\n",
                spot
            )),
        }
    }

    if d.prior_matured_notes.is_empty() {
        p.push_str("- matured scored windows: none yet\n");
    } else {
        p.push_str(
            "- matured scored windows for this holding (deterministic; any vintage — \
             a window may predate the prior read):\n",
        );
        for note in &d.prior_matured_notes {
            p.push_str(&format!("  - {note}\n"));
        }
    }
    p.push_str(
        "Write self_assessment against this: was your prior read right, was it better \
         than the engine baseline, and why — then let it discipline this run's model \
         arm.\n",
    );
    p
}

/// The user prompt: the holding's evidence packet rendered for the model — the
/// position, the computed metrics/sub-scores/grade/targets, the options-activity
/// signal (an activity proxy, not a grade input), the gaps, the distilled research,
/// the house view, and the prior verdict for continuity.
/// Whether [`interpretation_user_prompt`] will render any house-view content — the
/// latest sections **or** the recent stances, both of which this branch renders. The
/// counterpart of [`role_risk_prompt_renders_house_view`], and deliberately a wider
/// predicate, because the two prompts carry different parts of the house view.
pub(crate) fn priced_prompt_renders_house_view(d: &HoldingDossier) -> bool {
    d.house_view.latest_sections.is_some() || !d.house_view.recent_summaries.is_empty()
}

/// The COT underlying-positioning line for a commodity / macro fund
/// (`docs/portfolio-workflow.md` §Step 5; `docs/data-sources.md §CFTC`):
/// weekly, as-of positioning **context** — layer (c), held out of every
/// sub-score. Empty where no contract mapped.
fn positioning_prompt_section(f: &crate::portfolio::fund::FundContext) -> String {
    let Some(p) = &f.positioning else {
        return String::new();
    };
    let mut s = format!(
        "\nUNDERLYING POSITIONING (CFTC COT, weekly — snapshot as of {}; \
         positioning context, never a score input): {} — speculator net {:+.0} \
         contracts",
        p.report_date, p.contract, p.spec_net
    );
    if let Some(pct) = p.spec_pct_oi_long {
        s.push_str(&format!(" ({pct:.1}% of OI long)"));
    }
    if let Some(chg) = p.spec_net_weekly_change {
        s.push_str(&format!(", w/w {chg:+.0}"));
    }
    if let Some(rm) = p.real_money_net {
        s.push_str(&format!("; asset-manager net {rm:+.0}"));
        if let Some(c) = p.real_money_net_weekly_change {
            s.push_str(&format!(" (w/w {c:+.0})"));
        }
    }
    s.push('\n');
    s
}

/// The CBOE venue-level put/call backdrop (`docs/data-sources.md §CBOE`):
/// broad-market options sentiment from Cboe's own venue flow — macro context,
/// never a per-name signal (the per-stock read is the Schwab-chain options
/// signal). Empty where the leg failed or never ran.
fn put_call_backdrop_prompt_section(d: &HoldingDossier) -> String {
    let Some(b) = &d.put_call_backdrop else {
        return String::new();
    };
    let fmt =
        |v: Option<f64>| v.map(|x| format!("{x:.2}")).unwrap_or_else(|| "(gap)".to_string());
    format!(
        "MARKET OPTIONS SENTIMENT (CBOE venue-level daily put/call, as of {} — \
         broad-market backdrop, never a per-name signal): total {}, index {}, \
         equity {}\n",
        b.as_of,
        fmt(b.total),
        fmt(b.index),
        fmt(b.equity)
    )
}

/// The implied-expectations range (`docs/portfolio-analysis.md` §Starting
/// parameters): the engine's scenario math inverted at the live price — the
/// priced-in anchor the forward outlook (and the trim-a-winner judgment) is
/// read against. Conviction / action evidence only, never a gate. Empty where
/// the engine computed none (a fund, the current-multiple carry).
fn implied_expectations_prompt_section(e: &engine::EngineOutput) -> String {
    let Some(ie) = &e.implied_expectations else {
        return String::new();
    };
    let pct = |g: f64| format!("{:+.1}%", g * 100.0);
    let growth = match &ie.implied_growth {
        Some(g) => format!(
            "implied {} growth vs the trailing TTM print: {} at the bull multiple, \
             {} at the base multiple, {} at the bear multiple",
            if ie.revenue_based { "revenue-per-share" } else { "EPS" },
            pct(g[2]),
            pct(g[1]),
            pct(g[0]),
        ),
        None => format!(
            "implied per-share driver ({}): {:.2} at the bull multiple, {:.2} at the \
             base multiple, {:.2} at the bear multiple (trailing print absent or \
             non-positive, so growth is undefinable)",
            ie.driver_rung, ie.implied_drivers[2], ie.implied_drivers[1], ie.implied_drivers[0],
        ),
    };
    format!(
        "\nIMPLIED EXPECTATIONS (the engine's scenario math inverted at the live \
         price — what the current price ALREADY assumes, a range under stated \
         assumptions, never a gate): {growth}. Assumptions: {} multiples{}{}. Read \
         your forward outlook against this priced-in anchor — \"strong outlook, \
         already paid for\" is a computed contrast here, not a vibe.\n",
        if ie.rate_anchored { "rate-anchored (spread-percentile)" } else { "raw-percentile" },
        if ie.rate_anchored {
            format!(", DGS10 {:.2}%", ie.dgs10 * 100.0)
        } else {
            String::new()
        },
        if ie.revenue_based {
            " — revenue rung: the range assumes prevailing margins (the margin \
             dimension is a stated assumption, not a solved number)"
        } else {
            ""
        },
    )
}

/// The same-underlying option overlay (`docs/portfolio-workflow.md` §Step 6a):
/// the holding's own option legs, classified, with coverage and net delta —
/// rendered into BOTH 6f prompts, because the overlay changes what the right
/// action is. Empty where the holding carries no option legs.
fn option_overlay_prompt_section(d: &HoldingDossier) -> String {
    use crate::portfolio::dossier::{OverlayClass, OverlayDirection};
    let Some(o) = &d.option_overlay else {
        return String::new();
    };
    let class = match o.class {
        OverlayClass::CoveredCall => "covered call",
        OverlayClass::ProtectivePut => "protective put",
        OverlayClass::Collar => "collar",
        OverlayClass::Other => "other (unrecognized or multi-leg)",
    };
    let mut s = format!(
        "\nSAME-UNDERLYING OPTION OVERLAY (held option positions on this name — \
         this changes what the right action is): classified {class}"
    );
    if let Some(cr) = o.coverage_ratio {
        s.push_str(&format!(", covering {:.0}% of the held shares", cr * 100.0));
    }
    if let Some(nd) = o.net_delta {
        s.push_str(&format!("; net delta {nd:+.0} share-equivalents"));
    }
    s.push_str(".\n");
    for l in &o.legs {
        s.push_str(&format!(
            "- {} {}× {} — strike {}, expiry {}, delta {}\n",
            match l.direction {
                OverlayDirection::Long => "LONG",
                OverlayDirection::Short => "SHORT",
            },
            l.quantity,
            l.kind
                .map(|k| match k {
                    crate::schwab::OptionKind::Call => "CALL",
                    crate::schwab::OptionKind::Put => "PUT",
                })
                .unwrap_or("UNRECOGNIZED"),
            l.strike.map(|v| format!("{v:.2}")).unwrap_or_else(|| "?".into()),
            l.expiry.as_deref().unwrap_or("?"),
            l.delta.map(|v| format!("{v:+.2}")).unwrap_or_else(|| "(gap)".into()),
        ));
    }
    if !o.gaps.is_empty() {
        s.push_str(&format!("Overlay gaps: {}\n", o.gaps.join("; ")));
    }
    s
}

/// The narrative-vs-reality read (`docs/portfolio-analysis.md` §Starting
/// parameters): the conviction-layer red-flag ratio, rendered as layer-(b)
/// evidence — a tripped hype cap names its engine-matched rule (an engine-arm
/// bound, never a clamp on the model's own values), and the letter grade is
/// untouched either way. Empty where the read was uncomputable (the audit's
/// gap manifest carries the reason).
fn narrative_prompt_section(n: Option<&engine::NarrativeRead>) -> String {
    use crate::portfolio::engine::{NarrativeClass, NarrativeForm};
    let Some(n) = n else {
        return String::new();
    };
    let pct = |v: f64| format!("{:+.1}%", v * 100.0);
    let (expansion_label, reality_label) = match n.form {
        NarrativeForm::RevisionBased => (
            "forward-multiple change since the prior read",
            "consensus-EPS revision over the same interval",
        ),
        NarrativeForm::OperatingReality => (
            "annualized price move since the prior read (thin coverage — the \
             operating-reality-vs-price fallback)",
            "reported TTM revenue growth, year over year",
        ),
    };
    let class_line = match n.classification {
        NarrativeClass::JustifiedExpensive => {
            "JUSTIFIED-EXPENSIVE — the reality leg underwrites the re-rating".to_string()
        }
        NarrativeClass::Neutral => {
            "NEUTRAL — no meaningful multiple expansion to classify".to_string()
        }
        NarrativeClass::Hype => format!(
            "HYPE — the expansion outran the reality leg{}",
            n.ratio
                .map(|r| format!(" ({r:.1}×)"))
                .unwrap_or_else(|| " (reality flat or declining)".to_string()),
        ),
    };
    let mut s = format!(
        "\nNARRATIVE VS REALITY ({} days elapsed — conviction evidence, NOT a grade \
         input): {expansion_label} {}; {reality_label} {}. Read: {class_line}.\n",
        n.elapsed_days,
        pct(n.expansion),
        pct(n.reality),
    );
    if let Some(rule) = &n.matched_rule {
        s.push_str(&format!(
            "Engine-matched soft rule (binds the ENGINE arm; your own values stay \
             yours, departures annotated): {rule}.\n"
        ));
    }
    s
}

/// The FINRA consolidated short-interest read (`docs/data-sources.md §FINRA`):
/// per-holding risk / squeeze-context **positioning evidence** off the biweekly
/// file — level, trend, and days-to-cover, held out of every sub-score. Empty
/// where the holding has no row or the leg never ran.
fn short_interest_prompt_section(d: &HoldingDossier) -> String {
    let Some(si) = &d.short_interest else {
        return String::new();
    };
    let trend = match si.previous_short_interest {
        Some(prev) if prev > 0.0 => format!(
            "{:+.1}% vs the prior settlement's {prev:.0}",
            (si.current_short_interest / prev - 1.0) * 100.0
        ),
        _ => "prior settlement (gap)".to_string(),
    };
    format!(
        "\nSHORT INTEREST (FINRA consolidated biweekly file, settlement {} — \
         risk / squeeze context, positioning evidence only, NOT a grade input; \
         the file lags its settlement by ~7 business days): {:.0} shares short \
         ({trend}), avg daily volume {}, days to cover {}\n",
        si.settlement_date,
        si.current_short_interest,
        si.average_daily_volume
            .map(|v| format!("{v:.0}"))
            .unwrap_or_else(|| "(gap)".to_string()),
        si.days_to_cover
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "(gap)".to_string()),
    )
}

/// The run-level commodity context, rendered for a commodity-linked holding
/// (`docs/portfolio-workflow.md` §Step 5): published levels with their print
/// dates — as-of evidence for the conviction and narrative reads, never a score
/// input. Empty for a holding with no sector-matched prints.
fn commodity_prompt_section(d: &HoldingDossier) -> String {
    if d.commodity_context.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\nCOMMODITY CONTEXT (run-level, matched to this holding's sector / \
         industry identity; published levels, each as of its print date — a \
         monthly series lags by design):\n",
    );
    for p in &d.commodity_context {
        s.push_str(&format!(
            "- {}: {:.2} {} (as of {}",
            p.label, p.latest.value, p.unit, p.latest.date
        ));
        if let Some(t) = &p.trailing {
            if t.value != 0.0 {
                s.push_str(&format!(
                    "; {:+.1}% vs {:.2} on {}",
                    (p.latest.value / t.value - 1.0) * 100.0,
                    t.value,
                    t.date
                ));
            }
        }
        s.push_str(")\n");
    }
    s
}

/// The hard-forensic filings section shared by the interpretation and action
/// prompts: the item-classified 8-K sweep state rendered as typed evidence
/// (`docs/portfolio-analysis.md` §Starting parameters). A tripped hard trigger
/// names the engine-matched rule — evidence and an engine-arm bound, never a
/// clamp on the model's own values. Empty where the sweep never ran (the audit's
/// gap manifest carries the reason there).
fn forensic_prompt_section(d: &HoldingDossier) -> String {
    use crate::portfolio::ForensicFilingState;
    match &d.filing_events {
        None => String::new(),
        Some(ForensicFilingState::Clear) => {
            "\nFORENSIC FILINGS (item-classified 8-K sweep): clean — no restatement \
             (Item 4.02) or auditor-change (Item 4.01) filing inside the lookback.\n"
                .to_string()
        }
        Some(ForensicFilingState::Unknown { reason, .. }) => format!(
            "\nFORENSIC FILINGS (item-classified 8-K sweep): UNKNOWN — {reason}. \
             Not a clean check; weigh accordingly.\n"
        ),
        Some(ForensicFilingState::Events { events }) => {
            let mut s = String::from(
                "\nFORENSIC FILINGS — HARD TRIGGER TRIPPED (typed, filing-classified \
                 events; never a model assertion):\n",
            );
            for ev in events {
                s.push_str(&format!(
                    "- {} — filed {} ({}; {})\n",
                    ev.kind.label(),
                    ev.filing_date,
                    ev.source,
                    ev.confidence
                ));
            }
            s.push_str(
                "Engine-matched hard rule (binds the ENGINE arm; your own values stay \
                 yours, departures annotated): engine conviction capped Low; the add \
                 family is barred from the engine action set. Weigh the event's \
                 severity and recency in your own conviction and risk read; the \
                 letter grade is untouched by rule.\n",
            );
            s
        }
    }
}

pub fn interpretation_user_prompt(input: &InterpretationInput) -> String {
    let d = input.dossier;
    let e = input.engine;
    let mut p = String::new();

    p.push_str(&holding_header(d));
    p.push_str(&format!(
        "Position change since last run: {}\n",
        describe_position_change(&d.position_delta, d.position.quantity, d.position.cost_basis)
    ));

    p.push_str(&format!(
        "\nENGINE GRADE (the baseline arm{}): {}\nENGINE SUB-SCORES (0-100, higher better on every axis — a high risk score = \
         resilient/low-risk): quality {:.0}, valuation {:.0}, risk {:.0}; \
         momentum {:.0} rides as market-setup context OUTSIDE the letter\n",
        if e.low_confidence_grade {
            "; low-confidence — an imputed sub-score underlies it"
        } else {
            ""
        },
        e.grade.as_str(),
        e.sub_scores.quality,
        e.sub_scores.valuation,
        e.sub_scores.risk,
        e.sub_scores.momentum,
    ));
    p.push_str(&format!(
        "RISK TIER: {} (deterministic). CAPITAL-EFFICIENCY READ: {} (hurdle {}; only \
         `fails` is dead money. A `fails` read is one input to weigh — set it against \
         the TARGET PROVENANCE below and the data quality: a fails verdict built on \
         low-signal targets is weak evidence for an exit, not an instruction.)\n",
        e.risk_tier.as_str(),
        format!("{:?}", e.hurdle.state).to_lowercase(),
        e.hurdle
            .hurdle_rate
            .map(|h| format!("{:.1}%", h * 100.0))
            .unwrap_or_else(|| "(gap)".to_string()),
    ));
    if let Some(f) = &d.fund {
        p.push_str(&format!(
            "\nFUND CONTEXT: this holding is a fund graded on the reduced path — real \
             valuation (exposure-priced composite) and risk; the quality axis is \
             structurally absent and neutral-imputed (the letter carries a visible \
             low-confidence marker). Expense ratio (decimal fraction of assets per \
             year; 0.0075 = 0.75%/yr): {}. US share: {}.\n",
            opt(f.fund.expense_ratio),
            f.fund
                .country_weights
                .iter()
                .filter(|(c, _)| c.to_ascii_lowercase().contains("united states"))
                .map(|(_, w)| format!("{:.0}%", w * 100.0))
                .next()
                .unwrap_or_else(|| "(gap)".to_string()),
        ));
        if let Some(cov) = e.metrics.composite_coverage {
            p.push_str(&format!(
                "Composite P/E coverage: {:.0}% of fund weight; the uncovered {:.0}% \
                 is reported beside the valuation read, never averaged in.\n",
                cov * 100.0,
                (1.0 - cov) * 100.0
            ));
        }
        p.push_str(&positioning_prompt_section(f));
    }

    p.push_str("\nCOMPUTED METRICS:\n");
    let m = &e.metrics;
    let line = |label: &str, v: Option<f64>| match v {
        Some(x) => format!("- {label}: {x:.4}\n"),
        None => format!("- {label}: (gap)\n"),
    };
    p.push_str(&line("net margin", m.net_margin));
    p.push_str(&line("gross margin", m.gross_margin));
    p.push_str(&line("revenue growth", m.revenue_growth));
    p.push_str(&line("debt/equity", m.debt_to_equity));
    p.push_str(&line("return volatility (DAILY std dev of returns)", m.return_volatility));
    p.push_str(&line("trailing return", m.trailing_return));
    p.push_str(&line("P/E", m.pe_ratio));
    p.push_str(&line("P/S", m.ps_ratio));
    p.push_str(&line("P/B", m.pb_ratio));

    if let Some(tm) = &e.price_targets.twelve_month {
        p.push_str(&format!(
            "\nENGINE SCENARIO TARGETS (baseline arm; twelve-month rolling): bear {:.2} / base {:.2} / bull {:.2}\n  methodology: {}\n",
            tm.bear, tm.base, tm.bull, tm.methodology
        ));
    }
    if let Some(om) = &e.price_targets.one_month {
        p.push_str(&format!(
            "ENGINE ONE-MONTH TARGETS: bear {:.2} / base {:.2} / bull {:.2}\n",
            om.bear, om.base, om.bull
        ));
    }

    // How much signal the targets carry — the audit-level derivation flags surfaced
    // to the model so a low-signal target surface is weighed, not obeyed (the
    // 2026-07-31 run's F6: the sell-all cascade rode targets the audit already knew
    // were a current-multiple carry).
    let t = &e.target_meta;
    let anchor = if t.rate_anchored {
        format!(
            "multiples spread-anchored on {} rate observations",
            t.anchor_observations
        )
    } else if t.current_multiple_carry {
        "NO anchor history — the current multiple was carried, so the targets hug the \
         current price and carry little forward signal"
            .to_string()
    } else {
        "raw-percentile fallback (anchor window below the observation floor)".to_string()
    };
    p.push_str(&format!("TARGET PROVENANCE: {anchor}; driver: {}", t.driver_rung));
    if let Some(rows) = t.consensus_rows {
        p.push_str(if rows >= 2 {
            " (two-row NTM blend)"
        } else {
            " (single forward consensus row)"
        });
    }
    if t.flat_driver {
        p.push_str("; driver held FLAT across scenarios");
    }
    if t.clamp_flattened {
        p.push_str("; published scenario spread clamp-flattened");
    }
    if t.dispersion_floor_applied {
        p.push_str("; band widened to the volatility dispersion floor");
    }
    p.push_str(
        ".\n  Weigh the targets by this provenance: spread-anchored multiples carry \
         real signal even with a flat driver (the scenario spread then rides the \
         anchored multiple range — structural on the fund form). The low-signal \
         shape is the current-multiple carry with a flat or clamp-flattened driver \
         — targets that simply hug the current price. A floor-widened band inherits \
         its base's signal quality: over an anchored base, discount the band's \
         width, not its level — a `fails` that survives even the widened bull leg \
         is robust exit evidence; over the low-signal carry shape, the floor only \
         manufactures width around the current price, and a `fails` there stays \
         weak exit evidence.\n",
    );

    p.push_str(&implied_expectations_prompt_section(e));
    p.push_str(&narrative_prompt_section(input.narrative));

    if let Some(overlay) = input.pre_profit {
        p.push_str(&pre_profit_prompt_section(overlay, PromptStage::Interpretation));
    }

    p.push_str(
        "\nYOUR MODEL ARM (authored by you, unrestricted, scored against realized \
         outcomes beside the engine baseline): model_sub_scores — your own \
         quality/valuation/momentum/risk on the 0-100 higher-is-better scale (higher \
         risk score = lower risk; your letter derives from your \
         quality/valuation/risk through the same cutoffs); \
         model_price_targets — your own one-month and twelve-month base/bear/bull \
         prices (positive numbers, bear ≤ base ≤ bull as you mean them); \
         self_assessment — your honest retrospective (on a debut: say it is a first \
         read). Depart the engine wherever your read of the evidence differs; \
         agreement is a finding, not a requirement.\n",
    );

    let s = &d.options_signal;
    p.push_str(&format!(
        "\nOPTIONS ACTIVITY (proxy only — NOT a grade input): put/call vol {}, put/call OI {}, IV {}, IV skew {}\n",
        opt(s.put_call_volume),
        opt(s.put_call_open_interest),
        opt(s.implied_volatility),
        opt(s.iv_skew),
    ));
    p.push_str(&put_call_backdrop_prompt_section(d));
    p.push_str(&short_interest_prompt_section(d));
    p.push_str(&option_overlay_prompt_section(d));

    if !d.financials.gaps.is_empty() {
        p.push_str(&format!("\nDATA GAPS: {}\n", d.financials.gaps.join("; ")));
    }

    p.push_str(&forensic_prompt_section(d));
    p.push_str(&commodity_prompt_section(d));

    p.push_str(&format!("\nDISTILLED RESEARCH:\n{}\n", input.distilled));

    if let Some(sections) = &d.house_view.latest_sections {
        p.push_str(&format!(
            "\nMARKET SIGNAL HOUSE VIEW (latest report — scope: the horizon reads and \
             market-setup context only, never by itself a reason to exit this \
             holding):\n{sections}\n"
        ));
    }
    if !d.house_view.recent_summaries.is_empty() {
        p.push_str("\nRECENT REPORT STANCES:\n");
        for s in &d.house_view.recent_summaries {
            p.push_str(&format!(
                "- {}: thesis {}, risk posture {}\n",
                s.created_at,
                s.thesis_stance.as_str(),
                s.risk_posture.as_str()
            ));
        }
    }

    // The investor profile is deliberately NOT rendered here: the intrinsic
    // verdict is profile-independent and the profile enters at the per-holding
    // action call only (`docs/portfolio-workflow.md` §Step 6f "deliberately
    // absent"; `docs/portfolio-analysis.md` §Intrinsic verdict).

    p.push_str("\nHORIZONS for the outlook: ");
    p.push_str(&format!("{HORIZON_SHORT}, {HORIZON_MID}, {HORIZON_LONG}.\n"));

    // The input delta's technology-event pre-flag — rendered only when fired
    // (`docs/portfolio-analysis.md` §Starting parameters: the flag asserts
    // nothing about the cause; its research-topic consumer lands with the
    // research loop).
    if let Some(f) = input.tech_pre_flag.filter(|f| f.fired) {
        p.push_str(&format!(
            "\nTECHNOLOGY-EVENT PRE-FLAG (input delta): the sector-relative move \
             since the prior read ({:+.1}% vs {} over {} sessions) exceeds the \
             ±{:.1}% threshold (2× interval-scaled realized volatility). This \
             flags a POSSIBLE third-party repricing event; it asserts nothing \
             about the cause — weigh whether a real technology event explains \
             the move before treating it as impairment or opportunity.\n",
            f.relative_move * 100.0,
            f.benchmark,
            f.sessions,
            f.threshold * 100.0,
        ));
    }

    match &d.prior_verdict {
        Some(_) => {
            p.push_str(
                "\nCONTINUITY: a prior verdict for this holding exists. Keep the verdict firm; \
                 only move grade/target if the evidence has materially changed, and say what.\n",
            );
            // A band recalibration moves letters with no input change; without this
            // line the model's what-changed would attribute an engine-driven letter
            // move to company evidence or a self-correction (the grade-band slice's
            // versioning finding, `docs/verification/2026-08-03-grade-band-shadow-tune.md` §6).
            if d.prior_grade_parameter_version.as_deref() != Some(engine::GRADE_PARAMETER_VERSION)
            {
                p.push_str(
                    "NOTE: the grade bands were recalibrated since the prior verdict \
                     (grade parameter version changed), so the letter may have moved \
                     with no change in the company's inputs. Attribute such a move in \
                     what_changed to the recalibration — not to company change or a \
                     self-correction.\n",
                );
            }
            // The v7 retrospective: the prior run's BOTH-arm values plus what has
            // happened since — a deliberate reversal of the v4 anchoring guard,
            // because self-assessment against the baseline is the point of the
            // model arm (`docs/portfolio-analysis.md` §The holding verdict).
            p.push_str(&retrospective_prompt_section(d));
            p.push_str(&semantic_recall_prompt_section(d));
            // The rendered input delta — the what_changed_entries evidence ids
            // the 6g attribution validator resolves against.
            p.push_str(&input_delta_prompt_section(input.input_delta));
        }
        None => p.push_str(
            "\nCONTINUITY: new holding (no prior verdict). what_changed_entries \
             must be [].\n",
        ),
    }

    p.push_str(&ledger_prompt_section(
        d.prior_ledger(),
        input.ledger_eval,
        false,
    ));

    p
}

fn opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.3}")).unwrap_or_else(|| "(gap)".to_string())
}

/// The system prompt for the **per-holding action call** — the profile's one
/// entry point into the job (`docs/portfolio-analysis.md` §Portfolio action).
/// Tunnel vision is stated as the contract: the decision weighs this holding
/// and the profile alone; the whole-book reconciliation belongs to the future
/// portfolio-planner job and none of its inputs are given.
pub fn action_system_prompt() -> String {
    format!(
        "You are deciding the portfolio action for ONE holding, in isolation. The \
     holding's verdict has already been authored; your job is the rung alone. \
     TUNNEL VISION IS THE CONTRACT: judge this holding on its own merits and the \
     investor profile alone — the rest of the portfolio (available cash, sector \
     weights, concentration, correlation, every other holding) is deliberately \
     out of scope and none of it is given to you; a separate planning stage \
     reconciles actions across the whole book later, so do not hedge this call \
     against unseen portfolio context. Choose exactly ONE rung from the fixed \
     ladder — sell-all, trim, hold, add, add-aggressively — the rung only: no \
     share counts, dollar amounts, or portfolio weights. Weigh the verdict's own \
     evidence: both arms' grades and scores, the conviction, the horizon \
     outlook, the targets' implied upside/downside against the current price \
     (discounted by their stated provenance), and the capital-efficiency read — \
     only a `fails` hurdle is dead money, and a fails read leans toward \
     realizing some or all of the position once the forward prospects are \
     independently judged poor; the possible tax benefit of booking a loss (or \
     the tax cost of realizing a gain) is a user consideration to FLAG in the \
     rationale, never the mover of the rung. For a role/risk-only vehicle (a \
     class this pipeline cannot price) decide from its role read, expense drag, \
     observable risk, structural flags, and evidence gaps — an add-side rung \
     there must be earned by the vehicle's own merits, stated in the rationale. \
     The ENGINE SET shown is the engine arm's own restriction — evidence, never \
     a bound on you: the full ladder is open, and an outside-the-set choice \
     persists exactly as authored with the departure annotated beside it. The \
     INVESTOR PROFILE frames the decision — an aggressive risk tolerance can \
     justify the aggressive rung where the evidence supports it; the profile \
     never changes the verdict's facts. The rationale is exactly ONE sentence, \
     never empty — the single reason the rung was chosen. Keep the action firm \
     run to run: move it only when the verdict's evidence has materially moved. {}",
        crate::portfolio::action_response_contract()
    )
}

/// The user prompt for the action call: the finished verdict digest, the
/// position's own economics, the engine's per-holding action set (the engine
/// arm's own pick deliberately withheld — the scoreboard needs the arms
/// independent, the same ruling as the 6f render), and the investor profile.
pub fn action_user_prompt(input: &ActionInput) -> String {
    let d = input.dossier;
    let mut p = String::new();
    p.push_str(&holding_header(d));

    let pl = d.position.market_value - d.position.cost_basis;
    p.push_str(&format!(
        "Unrealized P/L: ${pl:.0} total ({})\n",
        if pl < 0.0 {
            "an unrealized loss — booking it may carry a tax benefit; flag as a user \
             consideration, never the mover"
        } else {
            "an unrealized gain — a sale realizes it as taxable; flag as a user \
             consideration, never the mover"
        }
    ));
    if let Some(prior) = d.prior_verdict.as_ref().and_then(crate::portfolio::carried_action) {
        p.push_str(&format!(
            "Prior run's action for this holding: {} (continuity baseline — move \
             only on materially moved evidence).\n",
            prior.as_kebab()
        ));
    }

    match &input.subject {
        ActionSubject::Priced {
            graded,
            engine,
            pre_profit,
        } => {
            p.push_str(&format!(
                "\nTHE VERDICT (already authored — the evidence you act on):\n\
                 ENGINE ARM: grade {}{}; sub-scores quality {:.0} / valuation {:.0} / \
                 risk {:.0} (momentum {:.0} outside the letter); risk tier {}; \
                 capital-efficiency read {} (only `fails` is dead money).\n",
                graded.grade.as_str(),
                if graded.low_confidence_grade {
                    " (low-confidence — an imputed sub-score underlies it)"
                } else {
                    ""
                },
                graded.sub_scores.quality,
                graded.sub_scores.valuation,
                graded.sub_scores.risk,
                graded.sub_scores.momentum,
                graded
                    .risk_tier
                    .map(|t| t.as_str())
                    .unwrap_or("(gap)"),
                graded
                    .dead_money
                    .map(|s| format!("{s:?}").to_lowercase())
                    .unwrap_or_else(|| "(gap)".to_string()),
            ));
            {
                let mv = &graded.model_view;
                p.push_str(&format!(
                    "MODEL ARM: letter {}; sub-scores quality {:.0} / valuation {:.0} / \
                     momentum {:.0} / risk {:.0}.\n",
                    mv.letter.as_str(),
                    mv.sub_scores.quality,
                    mv.sub_scores.valuation,
                    mv.sub_scores.momentum,
                    mv.sub_scores.risk,
                ));
            }
            p.push_str(&format!(
                "CONVICTION: {:?}. HORIZON OUTLOOK: short {:?} / mid {:?} / long {:?}.\n",
                graded.conviction,
                graded.horizon_outlook.short,
                graded.horizon_outlook.mid,
                graded.horizon_outlook.long,
            ));
            if let (Some(spot), Some(tm)) = (
                d.financials.current_price,
                graded.price_targets.twelve_month.as_ref(),
            ) {
                if spot > 0.0 {
                    p.push_str(&format!(
                        "IMPLIED 12-MONTH MOVES vs spot {spot:.2} (engine targets): bear \
                         {:+.1}% / base {:+.1}% / bull {:+.1}%.\n",
                        (tm.bear / spot - 1.0) * 100.0,
                        (tm.base / spot - 1.0) * 100.0,
                        (tm.bull / spot - 1.0) * 100.0,
                    ));
                }
            }
            let t = &engine.target_meta;
            p.push_str(&format!(
                "TARGET PROVENANCE: {} — weigh the implied moves by it.\n",
                if t.rate_anchored {
                    "rate-anchored (real forward signal)"
                } else if t.current_multiple_carry {
                    "current-multiple carry (targets hug the current price — low signal)"
                } else {
                    "raw-percentile fallback (thin issuer history)"
                }
            ));
            p.push_str(&format!("FINANCIAL SUMMARY: {}\n", graded.financial_summary));
            if let Some(overlay) = pre_profit {
                p.push_str(&pre_profit_prompt_section(overlay, PromptStage::Action));
            }
        }
        ActionSubject::RoleRisk { verdict } => {
            p.push_str(&format!(
                "\nTHE VERDICT (already authored — a role/risk-only vehicle; no grade, \
                 targets, or conviction exist for this class):\nCLASS: {}\nROLE: {}\n",
                verdict.class_label, verdict.role_summary
            ));
            if !verdict.exposure_tilt.is_empty() {
                let tilt: Vec<String> = verdict
                    .exposure_tilt
                    .iter()
                    .take(5)
                    .map(|w| format!("{} {:.0}%", w.label, w.weight * 100.0))
                    .collect();
                p.push_str(&format!("EXPOSURE TILT: {}\n", tilt.join(", ")));
            }
            p.push_str(&format!(
                "EXPENSE DRAG (decimal fraction of assets per year): {}. OBSERVABLE \
                 RISK (annualized realized volatility): {}. STRUCTURAL FLAG \
                 (leveraged/inverse or option-overlay path dependency): {}.\n",
                opt(verdict.expense_drag),
                opt(verdict.observable_risk),
                if verdict.structural_flag { "yes" } else { "no" },
            ));
            if !verdict.evidence_gaps.is_empty() {
                p.push_str(&format!(
                    "EVIDENCE GAPS: {}\n",
                    verdict.evidence_gaps.join("; ")
                ));
            }
        }
    }

    p.push_str(&forensic_prompt_section(input.dossier));
    p.push_str(&commodity_prompt_section(input.dossier));
    p.push_str(&option_overlay_prompt_section(input.dossier));

    p.push_str(
        "\nENGINE SET (the engine arm's own restriction — evidence, not a bound; \
         your choice is open on the full ladder, an outside-the-set rung persists \
         as authored with the departure annotated): ",
    );
    let set: Vec<&str> = input.engine_set.iter().map(Action::as_kebab).collect();
    p.push_str(&set.join(", "));
    p.push_str(
        "\nWhich rung the engine arm itself picked is deliberately not shown: form \
         your own decision and let the scoreboard compare the two arms.\n",
    );

    p.push_str("\nINVESTOR PROFILE (frames the decision; the verdict's facts are fixed):\n");
    let profile = input.profile.display();
    // The cash row is deliberately not rendered: available capital is
    // whole-book context — the planner's domain — and the system prompt
    // promises none of it is given (Codex 2026-08-14, finding 3).
    p.push_str(&format!(
        "- objective: {}\n- risk tolerance: {}\n- horizon: {}\n- tax: {}\n",
        profile.objective, profile.risk_tolerance, profile.horizon, profile.tax,
    ));

    p
}

/// Which prompt the pre-profit overlay section is rendered into. The deterministic
/// facts block is the same on both; the consequence lines address what THAT stage
/// authors — conviction at interpretation (which chooses no action), the rung at
/// the per-holding action call (which authors no conviction) — and state the other
/// stage's consequence as context only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptStage {
    /// The intrinsic interpretation prompt (Step 6f): authors conviction, no action.
    Interpretation,
    /// The per-holding action call: authors the rung, no conviction.
    Action,
}

/// Render the finalized pre-profit execution / financing overlay for an eligible
/// stock's interpretation prompt and its per-holding action prompt
/// (`docs/portfolio-workflow.md` §Step 6f): the engine's states and matched rules,
/// framed as the ENGINE arm's own bindings — evidence the unrestricted model arm
/// weighs, with departures recorded as annotations, never prompt-level clamps (the
/// v7 two-arm contract). `stage` picks which consequence line carries the model
/// arm's guidance (see [`PromptStage`]).
fn pre_profit_prompt_section(o: &PreProfitOverlay, stage: PromptStage) -> String {
    use crate::portfolio::pre_profit::{ConvictionCeiling, FinancingState};
    let i = &o.statement_inputs;
    let mut p = String::new();
    p.push_str(
        "\nPRE-PROFIT EXECUTION / FINANCING OVERLAY (deterministic — conviction / risk / \
         action context, never a grade component; every number below is engine-computed):\n",
    );
    let financing = match o.financing_state {
        FinancingState::NotBurning => "not-burning (TTM free cash flow non-negative)".to_string(),
        FinancingState::Unscorable => "unscorable (a required input is missing)".to_string(),
        state => format!(
            "{} (runway {} months; liquid resources {}, TTM burn {})",
            match state {
                FinancingState::Adequate => "adequate",
                FinancingState::Watch => "watch",
                _ => "constrained",
            },
            i.runway_months
                .map(|m| format!("{m:.1}"))
                .unwrap_or_else(|| "(gap)".to_string()),
            opt(i.liquid_resources),
            opt(i.ttm_cash_burn),
        ),
    };
    p.push_str(&format!("- financing state: {financing}\n"));
    p.push_str(&format!(
        "- gross margin (latest 2q avg): {} (change vs preceding 2q: {})\n",
        i.gross_margin_recent_2q
            .map(|m| format!("{:.1}%", m * 100.0))
            .unwrap_or_else(|| "(gap)".to_string()),
        i.gross_margin_change_2q
            .map(|c| format!("{:+.1}pp", c * 100.0))
            .unwrap_or_else(|| "(gap)".to_string()),
    ));
    p.push_str(&format!(
        "- diluted shares YoY (split-adjusted): {}\n",
        i.diluted_share_change_yoy
            .map(|c| format!("{:+.1}%", c * 100.0))
            .unwrap_or_else(|| "(gap)".to_string()),
    ));
    p.push_str(&format!(
        "- capex intensity (TTM |capex| / revenue): {}\n",
        i.ttm_capex_intensity
            .map(|c| format!("{:.1}%", c * 100.0))
            .unwrap_or_else(|| "(gap)".to_string()),
    ));
    if o.execution.comparable_periods == 0 {
        p.push_str("- guidance attainment: no validated guidance/actual observation pairs yet\n");
    } else {
        p.push_str(&format!(
            "- guidance attainment: {} comparable period(s), {} miss(es); repeated miss: {}; \
             material single miss: {}\n",
            o.execution.comparable_periods,
            o.execution.misses.len(),
            if o.execution.repeated_miss { "YES" } else { "no" },
            if o.execution.material_single_miss { "YES" } else { "no" },
        ));
    }
    p.push_str(&format!(
        "- severe deterioration (conjunctive): {}\n",
        if o.severe_deterioration { "YES" } else { "no" }
    ));
    // The consequence lines are stage-aware: each stage gets the model-arm guidance
    // for what it authors, and the other stage's consequence as context only.
    if let Some(ceiling) = o.consequences.conviction_ceiling {
        let ceiling = match ceiling {
            ConvictionCeiling::Medium => "medium",
            ConvictionCeiling::Low => "low",
        };
        let rules = o.consequences.matched_rules.join("; ");
        match stage {
            PromptStage::Interpretation => p.push_str(&format!(
                "CONVICTION CEILING (engine rule): the engine arm holds its own conviction \
                 at or beneath {ceiling} — matched rule(s): {rules}. Your conviction is \
                 UNRESTRICTED: exceeding the ceiling persists as authored, with the \
                 departure recorded beside the rule — so weigh the execution evidence \
                 honestly rather than deferring to the ceiling.\n",
            )),
            // The action call authors no conviction: the ceiling is context.
            PromptStage::Action => p.push_str(&format!(
                "CONVICTION CEILING (engine rule, context): the engine arm holds its own \
                 conviction at or beneath {ceiling} — matched rule(s): {rules}. The \
                 interpretation stage authored the conviction; this call authors no \
                 conviction.\n",
            )),
        }
    }
    if o.consequences.exit_family_only {
        match stage {
            // Interpretation chooses no action: the narrowing is context; the
            // evidence behind it belongs in the conviction and risk read.
            PromptStage::Interpretation => p.push_str(
                "SEVERE DETERIORATION (engine rule): the engine's own action set narrows to \
                 the exit family {trim, sell-all} and its stand-in action follows it; the \
                 action decision stage that follows weighs this — here, weigh the \
                 validated deterioration evidence in your conviction and risk read.\n",
            ),
            PromptStage::Action => p.push_str(
                "SEVERE DETERIORATION (engine rule): the engine's own action set narrows to \
                 the exit family {trim, sell-all} and its stand-in action follows it. Your \
                 rung is UNRESTRICTED — a rung outside the exit family persists as authored \
                 with the departure recorded; weigh the validated deterioration evidence \
                 before departing.\n",
            ),
        }
    } else if o.consequences.bar_add_family {
        match stage {
            PromptStage::Interpretation => p.push_str(
                "Note: the engine's own action set drops the add family on the overlay's \
                 financing rule; the action decision stage that follows weighs this — \
                 here, weigh the financing evidence in your conviction and risk read.\n",
            ),
            PromptStage::Action => p.push_str(
                "Note: the engine's own action set drops the add family on the overlay's \
                 financing rule; your rung is UNRESTRICTED — an add-family rung persists as \
                 authored with the departure recorded; weigh the financing evidence before \
                 departing.\n",
            ),
        }
    }
    p
}

/// Render the thesis-ledger block for either interpretation prompt: the engine
/// series vocabulary, the prior ledger with its condition states, the engine's
/// crossings this run, and the rewrite instructions
/// (`docs/portfolio-analysis.md` §The position thesis ledger). This is the first
/// prior-run *content* the prompt carries — the standing view the model tests
/// against fresh evidence rather than re-deriving from scratch.
pub fn ledger_prompt_section(
    prior: Option<&ThesisLedger>,
    eval: Option<&LedgerEvaluation>,
    role_risk: bool,
) -> String {
    let mut p = String::new();

    p.push_str("\nENGINE SERIES for quantitative ledger conditions (use exactly these labels):\n");
    for s in engine::LedgerSeries::ALL {
        p.push_str(&format!("- {}: {}\n", s.as_kebab(), s.describe()));
    }

    match prior {
        Some(l) => {
            p.push_str("\nTHESIS LEDGER (prior run — the standing view this run tests):\n");
            p.push_str(&format!("Original thesis: {}\n", l.original_thesis));
            p.push_str(&format!("Current thesis: {}\n", l.current_thesis));
            if !l.key_drivers.is_empty() {
                let drivers: Vec<String> = l
                    .key_drivers
                    .iter()
                    .map(|d| match d.series {
                        Some(s) => format!("{} [{}]", d.name, s.as_kebab()),
                        None => d.name.clone(),
                    })
                    .collect();
                p.push_str(&format!("Key drivers: {}\n", drivers.join("; ")));
            }
            p.push_str("Monitor:\n");
            for m in &l.monitor {
                let target = m
                    .engine_target
                    .map(|t| format!(" [engine target {t:.2}]"))
                    .unwrap_or_default();
                p.push_str(&format!(
                    "- {:?} (p≈{:.0}%){target}: {}\n",
                    m.scenario, m.probability_pct, m.conditions
                ));
            }
            if !l.what_must_improve.is_empty() {
                p.push_str(&format!("What must improve: {}\n", l.what_must_improve));
            }
            if !l.what_must_not_break.is_empty() {
                p.push_str(&format!("What must not break: {}\n", l.what_must_not_break));
            }
            for (title, role) in [
                ("Falsifiers:", ConditionRole::Falsifier),
                ("Action triggers:", ConditionRole::Trigger),
            ] {
                let rows: Vec<&LedgerCondition> =
                    l.conditions.iter().filter(|c| c.role == role).collect();
                if rows.is_empty() {
                    continue;
                }
                p.push_str(title);
                p.push('\n');
                for c in rows {
                    let family = c
                        .trigger_family
                        .map(|f| format!("{f:?} ").to_lowercase())
                        .unwrap_or_default();
                    let kind = match &c.quant {
                        Some(q) => {
                            let streak = c
                                .eval_state
                                .as_ref()
                                .filter(|s| s.breach_streak > 0)
                                .map(|s| format!("; breach streak {}", s.breach_streak))
                                .unwrap_or_default();
                            // The full machine core, margin included — an unstated
                            // margin would make the model guess one, and any
                            // mismatch reads as a core edit that supersedes the
                            // condition and resets its breach history.
                            format!(
                                "quantitative: {} {} {} (margin {}){streak}",
                                q.series.as_kebab(),
                                q.comparator.as_kebab(),
                                q.threshold,
                                q.margin
                            )
                        }
                        None => "qualitative".to_string(),
                    };
                    p.push_str(&format!("- {family}[{kind}] {}\n", c.statement));
                }
            }

            p.push_str("\nENGINE CONDITION CROSSINGS THIS RUN (deterministic):\n");
            let mut any = false;
            if let Some(e) = eval {
                for c in &e.crossings {
                    any = true;
                    let what = match (c.outcome, c.role) {
                        (CrossingOutcome::Confirmed, ConditionRole::Trigger) => {
                            "TRIGGER FIRED (confirmed)"
                        }
                        (CrossingOutcome::Confirmed, ConditionRole::Falsifier) => {
                            "CONFIRMED BREACH"
                        }
                        (CrossingOutcome::FirstBreach, _) => {
                            "first-breach note (not yet confirmed — a lone print)"
                        }
                    };
                    p.push_str(&format!(
                        "- {what}: '{}' — observed {:.4} vs threshold {} (observation {})\n",
                        c.statement, c.observed_value, c.threshold, c.observation_id
                    ));
                }
                for u in &e.unevaluable {
                    any = true;
                    p.push_str(&format!("- unevaluable this run: {u}\n"));
                }
            }
            if !any {
                p.push_str("- none: no quantitative condition crossed or degraded\n");
            }
        }
        None => {
            p.push_str(
                "\nTHESIS LEDGER: none exists — this is the position's debut. Author the \
                 initial ledger; your thesis becomes the position's original thesis.\n",
            );
        }
    }

    p.push_str(
        "\nREWRITE THE THESIS LEDGER in `ledger`: the current thesis (the app carries \
         the original thesis unchanged); the key drivers the thesis actually depends \
         on, each tied to an engine series where one fits; the bear/base/bull monitor \
         conditions with rough probability leans (percent, roughly summing to 100); \
         what must improve to migrate toward the bull case and what must not break to \
         stay in the base case; the key falsifiers; and the action triggers. \
         State every quantitative falsifier or trigger machine-evaluably: the engine \
         series (exactly one label from the list above), below/above, a numeric \
         threshold in the series' units, and a materiality margin in the same units \
         (moves inside the margin don't count — the noise guard). A condition no \
         engine series fits is qualitative (quant: null) — state it precisely enough \
         to be researched. \
         Mark tripped/fired ONLY where the ENGINE CONDITION CROSSINGS show a CONFIRMED \
         crossing for that same condition; a qualitative claim needs a source-backed \
         research finding, and none are available this run. Unsupported claims are \
         cleared by the app. \
         Keep a condition's series/comparator/threshold/margin unchanged unless the \
         condition itself has genuinely changed — an edit to that core resets its \
         tracked breach history.\n",
    );
    if role_risk {
        p.push_str(
            "This is a role/risk-only holding: its ledger drivers are fund-flavored — \
             exposure tilt, expense drag, mandate/tracking fidelity, role in the \
             portfolio, house-view fit — its monitor scenarios carry no price targets, \
             and its triggers are trim/sell only (no add family).\n",
        );
    }
    p
}

/// A one-line description of the position's change since the prior run, for the
/// interpretation prompt — the structured delta the app computed, so the model reasons
/// over what the user actually did with the position: both the quantity move and the
/// cost-basis move (paid-up vs averaged-down).
fn describe_position_change(
    delta: &PositionDelta,
    current_qty: f64,
    current_cost_basis: f64,
) -> String {
    match delta.change {
        // "NEW" means new to this run history, nothing more — attempt 2's streams
        // burned large reasoning shares re-litigating "NEW" against a legacy cost
        // basis as if it meant a fresh purchase
        // (`docs/verification/2026-08-13-big-run-attempt-2.md` §Workstream 2).
        PositionChange::New => "NEW (no prior verdict in this run history — the position \
             itself may long predate this analysis, so the cost basis is the account's \
             history, not a recent entry)"
            .to_string(),
        PositionChange::Unchanged => "unchanged".to_string(),
        PositionChange::Increased | PositionChange::Decreased => {
            let dir = if matches!(delta.change, PositionChange::Increased) {
                "INCREASED"
            } else {
                "DECREASED"
            };
            let qty = match delta.prior_quantity {
                Some(prev) => format!(" quantity {prev} → now {current_qty}"),
                None => String::new(),
            };
            // Dollar-marked like the header two lines above it. Rendered bare, these
            // read as per-share against a header that says total, which is the
            // ambiguity Finding 4 documented the model paying to resolve
            // (`docs/verification/2026-08-10-big-run-attempt-1.md`).
            let basis = match delta.prior_cost_basis {
                Some(prev) => {
                    format!(", cost basis ${prev:.0} → now ${current_cost_basis:.0} total")
                }
                None => String::new(),
            };
            if qty.is_empty() && basis.is_empty() {
                dir.to_string()
            } else {
                format!("{dir} (prior{qty}{basis})")
            }
        }
    }
}

// ---- The deterministic stub analyst (offline) --------------------------------

/// A deterministic, offline [`HoldingAnalyst`] used by `cargo test` and any
/// daemon-free path. It derives a coherent interpretation from the engine's grade
/// (numbers still come from the engine), so the whole pipeline produces a schema-valid
/// verdict with no model call.
pub struct StubAnalyst;

/// The stub's ledger draft: echo the prior ledger where one exists (statements and
/// machine cores unchanged, so the carry path is exercised exactly as a live model
/// keeping its conditions would), else author a deterministic initial ledger.
fn stub_ledger_draft(prior: Option<&ThesisLedger>, symbol: &str, role_risk: bool) -> LedgerDraft {
    if let Some(l) = prior {
        let core_draft = |q: &QuantCore| QuantCoreDraft {
            series: q.series.as_kebab().to_string(),
            comparator: q.comparator.as_kebab().to_string(),
            threshold: q.threshold,
            margin: q.margin,
        };
        let scenario = |kind: ScenarioKind| {
            l.monitor
                .iter()
                .find(|m| m.scenario == kind)
                .map(|m| ScenarioDraft {
                    conditions: m.conditions.clone(),
                    probability_pct: m.probability_pct,
                })
                .unwrap_or(ScenarioDraft {
                    conditions: "unspecified".into(),
                    probability_pct: 33.0,
                })
        };
        return LedgerDraft {
            thesis: l.current_thesis.clone(),
            key_drivers: l
                .key_drivers
                .iter()
                .map(|d| KeyDriverDraft {
                    name: d.name.clone(),
                    series: d.series.map(|s| s.as_kebab().to_string()),
                })
                .collect(),
            bear: scenario(ScenarioKind::Bear),
            base: scenario(ScenarioKind::Base),
            bull: scenario(ScenarioKind::Bull),
            what_must_improve: l.what_must_improve.clone(),
            what_must_not_break: l.what_must_not_break.clone(),
            falsifiers: l
                .conditions
                .iter()
                .filter(|c| c.role == ConditionRole::Falsifier)
                .map(|c| FalsifierDraft {
                    statement: c.statement.clone(),
                    quant: c.quant.as_ref().map(core_draft),
                    technology_class: c.technology_class,
                    tripped: false,
                })
                .collect(),
            triggers: l
                .conditions
                .iter()
                .filter(|c| c.role == ConditionRole::Trigger)
                .map(|c| TriggerDraft {
                    statement: c.statement.clone(),
                    family: match c.trigger_family {
                        Some(TriggerFamily::Add) => "add".into(),
                        Some(TriggerFamily::Sell) => "sell".into(),
                        _ => "trim".into(),
                    },
                    quant: c.quant.as_ref().map(core_draft),
                    fired: false,
                })
                .collect(),
        };
    }
    // The debut draft — one quantitative falsifier and trigger on always-computable
    // series, so offline runs exercise the executable-condition path end to end.
    let (falsifier, f_quant) = if role_risk {
        (
            "Expense ratio rises above 0.75% (mandate/cost drift)".to_string(),
            QuantCoreDraft {
                series: "expense-ratio".into(),
                comparator: "above".into(),
                threshold: 0.0075,
                margin: 0.0005,
            },
        )
    } else {
        (
            "The holding's price falls more than 40% below its current level".to_string(),
            QuantCoreDraft {
                series: "trailing-return".into(),
                comparator: "below".into(),
                threshold: -0.40,
                margin: 0.02,
            },
        )
    };
    LedgerDraft {
        thesis: format!("Hold {symbol} for its established role; evidence supports the standing position."),
        key_drivers: vec![KeyDriverDraft {
            name: if role_risk {
                "expense drag".into()
            } else {
                "margin trajectory".into()
            },
            series: Some(if role_risk {
                "expense-ratio".into()
            } else {
                "net-margin".into()
            }),
        }],
        bear: ScenarioDraft {
            conditions: "Fundamentals deteriorate materially".into(),
            probability_pct: 25.0,
        },
        base: ScenarioDraft {
            conditions: "The current trajectory holds".into(),
            probability_pct: 50.0,
        },
        bull: ScenarioDraft {
            conditions: "Growth re-accelerates".into(),
            probability_pct: 25.0,
        },
        what_must_improve: "Revenue growth and margins".into(),
        what_must_not_break: "The core franchise and balance sheet".into(),
        falsifiers: vec![FalsifierDraft {
            statement: falsifier,
            quant: Some(f_quant),
            technology_class: false,
            tripped: false,
        }],
        triggers: vec![TriggerDraft {
            statement: "Trim above the priced-in ceiling".into(),
            family: "trim".into(),
            quant: Some(QuantCoreDraft {
                series: "price".into(),
                comparator: "above".into(),
                threshold: 150.0,
                margin: 0.0,
            }),
            fired: false,
        }],
    }
}

impl HoldingAnalyst for StubAnalyst {
    fn distill(&self, _dossier: &HoldingDossier, findings: &ResearchFindings) -> Result<String> {
        Ok(if findings.notes.is_empty() {
            "No research findings.".to_string()
        } else {
            findings.notes.join(" ")
        })
    }

    fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
        let e = input.engine;
        let conviction = match e.grade {
            crate::portfolio::Grade::A | crate::portfolio::Grade::B => Conviction::High,
            crate::portfolio::Grade::C => Conviction::Medium,
            _ => Conviction::Low,
        };
        let read = |s: f64| {
            if s >= 60.0 {
                HorizonRead::Bullish
            } else if s >= 40.0 {
                HorizonRead::Neutral
            } else {
                HorizonRead::Bearish
            }
        };
        let what_changed = if input.dossier.prior_verdict.is_some() {
            "Reaffirmed; no material change since the prior run.".to_string()
        } else {
            "new holding".to_string()
        };
        Ok(Interpretation {
            conviction,
            horizon_outlook: HorizonOutlook {
                short: read(e.sub_scores.momentum),
                mid: read(e.sub_scores.quality),
                long: read((e.sub_scores.quality + e.sub_scores.valuation) / 2.0),
            },
            financial_summary: format!(
                "Composite grade {} on quality {:.0} / valuation {:.0} / momentum {:.0} / risk {:.0}.",
                e.grade.as_str(),
                e.sub_scores.quality,
                e.sub_scores.valuation,
                e.sub_scores.momentum,
                e.sub_scores.risk
            ),
            price_target_rationale: "Base case follows the engine's scenario midpoint.".to_string(),
            what_changed,
            // The stub re-affirms — no typed rows, matching the empty-audit
            // re-affirmation contract.
            what_changed_entries: Vec::new(),
            ledger: stub_ledger_draft(
                input.dossier.prior_ledger(),
                &input.dossier.position.symbol,
                false,
            ),
            // The stub's model arm: the engine's values deterministically nudged,
            // so the two arms are distinguishable in tests and demo runs without
            // being random.
            model_sub_scores: SubScores {
                quality: (e.sub_scores.quality + 5.0).min(100.0),
                valuation: (e.sub_scores.valuation + 5.0).min(100.0),
                momentum: (e.sub_scores.momentum + 5.0).min(100.0),
                risk: (e.sub_scores.risk + 5.0).min(100.0),
            },
            model_price_targets: {
                let spot = input.dossier.financials.current_price.unwrap_or(100.0);
                let mt = |t: Option<&PriceTarget>, scale: f64| ModelPriceTarget {
                    base: t.map(|t| t.base).unwrap_or(spot) * scale,
                    bear: t.map(|t| t.bear).unwrap_or(spot * 0.9) * scale,
                    bull: t.map(|t| t.bull).unwrap_or(spot * 1.1) * scale,
                };
                ModelPriceTargets {
                    one_month: mt(e.price_targets.one_month.as_ref(), 1.01),
                    twelve_month: mt(e.price_targets.twelve_month.as_ref(), 1.05),
                }
            },
            self_assessment: if input.dossier.prior_verdict.is_some() {
                "Prior read broadly held; no basis to fault the baseline yet.".to_string()
            } else {
                "First read for this holding — no prior call to assess.".to_string()
            },
        })
    }

    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
        Ok(RoleRiskInterpretation {
            role_summary: format!(
                "{} supplying {} exposure; held for its portfolio role.",
                input.readout.class_label,
                input
                    .readout
                    .exposure_tilt
                    .first()
                    .map(|(l, _)| l.as_str())
                    .unwrap_or("its mandated")
            ),
            what_changed: if input.dossier.prior_verdict.is_some() {
                "Reaffirmed; no material change since the prior run.".to_string()
            } else {
                "new holding".to_string()
            },
            what_changed_entries: Vec::new(),
            ledger: stub_ledger_draft(
                input.dossier.prior_ledger(),
                &input.dossier.position.symbol,
                true,
            ),
        })
    }

    fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision> {
        // The stub's decision is the deterministic grade-mapped rung (hold for a
        // role/risk vehicle), deliberately kept inside the engine set so no
        // outside-set annotation fires — the annotation path is rogue-stub
        // territory. Falls back to the least-drastic offered rung (hold is not
        // always offered — a severe pre-profit overlay restricts the set to the
        // exit family).
        let preferred = match &input.subject {
            ActionSubject::Priced { graded, .. } => match graded.grade {
                crate::portfolio::Grade::A => Action::Add,
                crate::portfolio::Grade::B | crate::portfolio::Grade::C => Action::Hold,
                crate::portfolio::Grade::D => Action::Trim,
                crate::portfolio::Grade::F => Action::SellAll,
            },
            ActionSubject::RoleRisk { .. } => Action::Hold,
        };
        let action = if input.engine_set.contains(&preferred) {
            preferred
        } else if input.engine_set.contains(&Action::Hold) {
            Action::Hold
        } else {
            *input.engine_set.last().unwrap_or(&Action::Hold)
        };
        Ok(crate::portfolio::ActionDecision {
            action,
            rationale: "Stub action: the grade-mapped rung inside the engine set.".to_string(),
        })
    }

    fn fast_id(&self) -> String {
        "stub-analyst".to_string()
    }

    fn reasoner_id(&self) -> String {
        "stub-analyst".to_string()
    }
}

// ---- The live local analyst (Ollama daemon) ----------------------------------

/// The live [`HoldingAnalyst`]: wraps a [`LocalModelClient`] and the roster's reasoner
/// and fast model ids. Distillation runs on the fast model — or on the reasoner when no
/// fast tier is configured; interpretation runs on the reasoner in thinking mode with
/// the grammar-constrained interpretation schema, so the returned object is
/// structurally valid by construction.
pub struct LocalAnalyst {
    client: LocalModelClient,
    reasoner_model: String,
    fast_model: String,
    /// Prompt-size observations accumulated across this run's chat calls
    /// (drained by [`HoldingAnalyst::take_prompt_usage`]). A `Mutex` only for the
    /// `&self` receivers — the per-holding loop is sequential, so it is never
    /// contended.
    prompt_usage: std::sync::Mutex<Vec<crate::local_model::PromptUsage>>,
}

/// The fast tier's effective model id: a blank `fast_model` falls back to the
/// reasoner — the fast tier is **optional** and never gates
/// (`docs/configuration.md §Local Analysis Suite Configuration`), and the
/// documented roster default runs distillation on the resident reasoner anyway
/// (`docs/local-models.md §The model roster and per-task routing`) — so a
/// reasoner+embedder-only setup runs rather than failing mid-run on an empty id.
/// The single home for the rule: [`LocalAnalyst::new`] and the resume-status
/// roster check both read it, so the two cannot drift.
pub fn effective_fast_model(reasoner_model: &str, fast_model: &str) -> String {
    if fast_model.trim().is_empty() {
        reasoner_model.to_string()
    } else {
        fast_model.to_string()
    }
}

impl LocalAnalyst {
    /// See [`effective_fast_model`] for the blank-fast-tier fallback this applies.
    pub fn new(client: LocalModelClient, reasoner_model: String, fast_model: String) -> Self {
        let fast_model = effective_fast_model(&reasoner_model, &fast_model);
        Self {
            client,
            reasoner_model,
            fast_model,
            prompt_usage: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Record one call's usage observation. Recorded unconditionally: the
    /// context-fit pair (`prompt_eval_count` × `num_ctx`) may be daemon-omitted,
    /// but the output-side observation (`eval_count`, `done_reason`) must survive
    /// regardless — a length stop whose row was dropped for a missing prompt
    /// count would vanish from the run's data-health read. Context-fit consumers
    /// gate on the fields being present (`build_data_health`).
    fn record_usage(
        &self,
        stage: String,
        req: &ChatRequest,
        resp: &crate::local_model::ChatResponse,
    ) {
        // The sent size in chars — the ground truth a post-truncation
        // `prompt_eval_count` is checked against (`build_data_health`).
        let prompt_chars = req
            .messages
            .iter()
            .map(|m| m.content.chars().count() as u64)
            .sum();
        self.prompt_usage
            .lock()
            .expect("prompt-usage lock is never poisoned")
            .push(crate::local_model::PromptUsage {
                stage,
                prompt_tokens: resp.prompt_eval_count,
                num_ctx: crate::local_model::request_num_ctx(req).unwrap_or(0),
                prompt_chars,
                completion_tokens: resp.eval_count,
                num_predict: crate::local_model::request_num_predict(req),
                output_limited: resp.done_reason.as_deref() == Some("length"),
            });
    }
}

/// Cap on how much of a model body a parse-failure context embeds. serde's own
/// error names the line/column; the head is for eyeballing shape — the full
/// body (up to ~250 KB at the construction reservation) must never ride an
/// error chain into a tracker step detail, a stderr tee line, or the persisted
/// `job_runs.detail`. Mirrors the `analyst_agent` snippet idiom.
const PARSE_CONTEXT_BODY_CAP: usize = 500;

/// Truncate a model body to [`PARSE_CONTEXT_BODY_CAP`] chars on a char
/// boundary, marking the cut with the full length.
fn body_snippet(content: &str) -> String {
    let (head, cut) = crate::data_sources::cap_chars(content, PARSE_CONTEXT_BODY_CAP);
    if cut {
        format!("{head} …(truncated, {} chars total)", content.chars().count())
    } else {
        head
    }
}

/// Fail a stage whose generation stopped at the output budget — a length stop
/// still returns `Ok` with a partial body and HTTP 200, so without this check
/// the truncation surfaces only as an opaque downstream parse failure
/// (`docs/verification/2026-08-10-big-run-attempt-1.md` §Fix candidates 4).
/// Called after `record_usage`, so the observation survives on the run's
/// data-health read even though the call fails.
fn ensure_not_output_limited(
    stage: &str,
    req: &ChatRequest,
    resp: &crate::local_model::ChatResponse,
) -> Result<()> {
    if resp.done_reason.as_deref() == Some("length") {
        let generated = resp.eval_count;
        let reservation = crate::local_model::request_num_predict(req);
        let show = |n: Option<u64>| n.map(|v| v.to_string()).unwrap_or_else(|| "unreported".into());
        let show_res =
            |n: Option<u32>| n.map(|v| v.to_string()).unwrap_or_else(|| "unset".into());
        // `done_reason: "length"` covers two stops with different levers: the
        // request's own `num_predict` reservation (generated ≈ reservation), or
        // the shared context filling first (generated well under it). The
        // classification is single-homed in `length_stop_reading` — the
        // data-health line reads the same stop through the same predicate —
        // and a stop with incomplete counts names no lever at all.
        match crate::local_model::length_stop_reading(generated, reservation) {
            crate::local_model::LengthStopReading::AtReservation => anyhow::bail!(
                "{stage}: response truncated at the output reservation (num_predict {}, \
                 generated {} tokens) — a runaway chain or a genuinely undersized \
                 reservation; raise it only on evidence",
                show_res(reservation),
                show(generated),
            ),
            crate::local_model::LengthStopReading::UnderReservation => anyhow::bail!(
                "{stage}: generation length-stopped under the output reservation (generated {} \
                 of {} reserved) — context exhaustion suspected; the sanctioned lever is \
                 compressing the digest, never raising num_ctx",
                show(generated),
                show_res(reservation),
            ),
            crate::local_model::LengthStopReading::Unattributed => anyhow::bail!(
                "{stage}: generation length-stopped with incomplete counts (generated {}, \
                 num_predict {}) — reservation-hit vs context exhaustion cannot be told \
                 apart; read the Ollama server log before reaching for either lever",
                show(generated),
                show_res(reservation),
            ),
        }
    }
    Ok(())
}

// Per-stage context sizes (`docs/local-model-operations.md §The num_ctx trap`):
// always explicit — the daemon's memory-dependent auto-size (~256 K on 128 GB)
// over-allocates KV cache, while an unset small default silently front-truncates
// the deterministic packet. Sized to hold packet + thinking budget + output.
/// Distillation: a compact findings condense — small packet, no thinking chain.
const NUM_CTX_DISTILL: u32 = 32_768;
/// Interpretation: the vendor advises ≥ 128 K context to preserve thinking
/// capability (chains run tens of thousands of tokens); hybrid attention keeps
/// the KV cost of this a few GB (`docs/local-model-operations.md §Context window`).
const NUM_CTX_INTERPRET: u32 = 131_072;
/// Ollama `keep_alive: -1` — never idle-unload. The roster's documented posture:
/// the reasoner (and embedder) stay resident between calls and runs
/// (`docs/local-models.md §The model roster and per-task routing`).
const KEEP_ALIVE_RESIDENT: i64 = -1;

// Per-stage output reservations (`num_predict`) — diagnostic ceilings, drafted
// and calibratable like the engine's other starting parameters, none yet
// calibrated against live evidence. Attempt 1's construction calls generated
// for 7–8 minutes (`docs/verification/2026-08-10-big-run-attempt-1.md` §Cost of
// the failed stage), roughly 12–15 K tokens with thinking included, so these sit
// far above any legitimate answer: a stop at the limit is evidence of a runaway
// or a squeeze, surfaced as a typed truncation error rather than an opaque
// parse failure. Generation shares `num_ctx` with the prompt, so a large prompt
// can exhaust the context before the ceiling binds — that stop reports the same
// `done_reason: "length"` and lands in the same typed guard.
/// Thinking stages (interpretation, role-risk, construction): chains run tens
/// of thousands of tokens and count against the same budget as the answer.
const NUM_PREDICT_THINKING: u32 = 65_536;
/// Distillation emits 2–3 sentences; generous by two orders of magnitude.
const NUM_PREDICT_DISTILL: u32 = 8_192;

/// The distill stage's context size, resolved per *model*, not per call: Ollama
/// reloads a resident runner whenever a request's load-time options — `num_ctx`
/// included — differ from the loaded ones, even under `keep_alive: -1`. So when
/// the fast tier fell back to the reasoner (the documented default roster),
/// distillation shares the interpretation context rather than bouncing the 81 GB
/// runner between 32 K and 128 K at every stage transition; the smaller distill
/// context applies only to a genuinely distinct fast model
/// (`docs/local-model-operations.md §The num_ctx trap`).
fn distill_num_ctx(fast_model: &str, reasoner_model: &str) -> u32 {
    if fast_model == reasoner_model {
        NUM_CTX_INTERPRET
    } else {
        NUM_CTX_DISTILL
    }
}

/// Build the distillation request: **explicitly non-thinking** (`Some(false)` —
/// an omitted flag rides Qwen's thinking-on default and cost the first live run
/// ~45 minutes, F3), non-thinking sampling, the caller-resolved context size
/// ([`distill_num_ctx`]). Pure, so the per-stage wiring is asserted offline.
fn distill_request(
    fast_model: &str,
    num_ctx: u32,
    dossier: &HoldingDossier,
    findings: &ResearchFindings,
) -> ChatRequest {
    // The fast model condenses the findings into a compact paragraph. With research
    // stubbed this is light, but it keeps the stage in the live path.
    let prompt = format!(
        "Condense these research findings on {} into 2-3 sentences of decision-relevant \
         signal. Findings:\n{}",
        dossier.position.symbol,
        findings.notes.join("\n")
    );
    let mut req = ChatRequest::new(fast_model, vec![ChatMessage::user(prompt)]);
    req.think = Some(false);
    req.options = Some(options::non_thinking_general(num_ctx, NUM_PREDICT_DISTILL));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Build the priced-branch interpretation request: thinking on (composes with the
/// grammar-constrained `format`), thinking sampling, interpret-sized context.
fn interpret_request(reasoner_model: &str, input: &InterpretationInput) -> ChatRequest {
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![
            ChatMessage::system(interpretation_system_prompt()),
            ChatMessage::user(interpretation_user_prompt(input)),
        ],
    );
    // The v7 unrestricted schema: full ladder, full conviction enum — the engine's
    // own lean bars and any pre-profit ceiling render into the prompt as evidence,
    // never as schema narrowing (`docs/portfolio-analysis.md` §The holding verdict,
    // the two-arm contract).
    req.format_schema = Some(interpretation_schema());
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Build the `role_risk_only`-branch interpretation request — same mode wiring as
/// the priced branch, reduced schema.
fn role_risk_request(reasoner_model: &str, input: &RoleRiskInput) -> ChatRequest {
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![
            ChatMessage::system(role_risk_system_prompt()),
            ChatMessage::user(role_risk_user_prompt(input)),
        ],
    );
    req.format_schema = Some(role_risk_interpretation_schema());
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Build the per-holding action request: thinking on (the rung is a judgment
/// call weighing the whole verdict against the profile), the action schema, and
/// the **shared** interpret context size — the one-`num_ctx`-per-model rule (an
/// Ollama `num_ctx` change reloads the resident runner,
/// `docs/local-model-operations.md §The num_ctx trap`).
fn action_request(reasoner_model: &str, input: &ActionInput) -> ChatRequest {
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![
            ChatMessage::system(action_system_prompt()),
            ChatMessage::user(action_user_prompt(input)),
        ],
    );
    req.format_schema = Some(crate::portfolio::action_decision_schema());
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

impl HoldingAnalyst for LocalAnalyst {
    fn distill(&self, dossier: &HoldingDossier, findings: &ResearchFindings) -> Result<String> {
        let req = distill_request(
            &self.fast_model,
            distill_num_ctx(&self.fast_model, &self.reasoner_model),
            dossier,
            findings,
        );
        let resp = self.client.chat(&req)?;
        self.record_usage(
            format!("distill {}", dossier.position.symbol),
            &req,
            &resp,
        );
        // A truncated distillation is otherwise fully silent — no schema guards
        // this stage, so the cut-off digest would flow onward as if complete.
        ensure_not_output_limited(
            &format!("distill {}", dossier.position.symbol),
            &req,
            &resp,
        )?;
        Ok(resp.content)
    }

    fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
        let req = interpret_request(&self.reasoner_model, input);
        // Stream step-scoped: the structured body has no console value (it stays
        // accumulated, never streamed), but the reasoning streams onto this
        // holding's own "Analyze {SYM}" step, so the tracker shows live thinking
        // instead of a minutes-long quiet stretch (the first live run's F8).
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let resp = self.client.chat_streaming(&req, StreamRole::Step(&step_key))?;
        self.record_usage(
            format!("interpret {}", input.dossier.position.symbol),
            &req,
            &resp,
        );
        ensure_not_output_limited(
            &format!("interpret {}", input.dossier.position.symbol),
            &req,
            &resp,
        )?;
        serde_json::from_str(&resp.content)
            .with_context(|| format!("parsing interpretation JSON: {}", body_snippet(&resp.content)))
    }

    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
        let req = role_risk_request(&self.reasoner_model, input);
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let resp = self.client.chat_streaming(&req, StreamRole::Step(&step_key))?;
        self.record_usage(
            format!("role-risk {}", input.dossier.position.symbol),
            &req,
            &resp,
        );
        ensure_not_output_limited(
            &format!("role-risk {}", input.dossier.position.symbol),
            &req,
            &resp,
        )?;
        serde_json::from_str(&resp.content).with_context(|| {
            format!(
                "parsing role/risk interpretation JSON: {}",
                body_snippet(&resp.content)
            )
        })
    }

    fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision> {
        let req = action_request(&self.reasoner_model, input);
        // Stream step-scoped like interpretation: the decision's reasoning lands
        // on this holding's own "Analyze {SYM}" step.
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let resp = self.client.chat_streaming(&req, StreamRole::Step(&step_key))?;
        self.record_usage(
            format!("action {}", input.dossier.position.symbol),
            &req,
            &resp,
        );
        ensure_not_output_limited(
            &format!("action {}", input.dossier.position.symbol),
            &req,
            &resp,
        )?;
        serde_json::from_str(&resp.content).with_context(|| {
            format!(
                "parsing action-decision JSON: {}",
                body_snippet(&resp.content)
            )
        })
    }

    fn fast_id(&self) -> String {
        // The reasoner's id when the fast tier fell back to it (`new`), so the
        // audit records the model distillation actually ran on.
        self.fast_model.clone()
    }

    fn reasoner_id(&self) -> String {
        self.reasoner_model.clone()
    }

    fn take_prompt_usage(&self) -> Vec<crate::local_model::PromptUsage> {
        std::mem::take(
            &mut *self
                .prompt_usage
                .lock()
                .expect("prompt-usage lock is never poisoned"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::engine::{
        CompanyFinancials, ConsensusEstimate, DatedValue, QuarterlyIncomeRow,
    };
    use crate::portfolio::fund::{FundContext, FundData, SectorPe};
    use crate::portfolio::{AssetClass, InvestorProfile, OptionsSignal};
    use crate::portfolio::dossier::HouseView;
    use crate::schwab::Position;
    use std::collections::HashMap;

    // ---- The 6g what-changed attribution validator ----

    fn wc_entry(
        kind: crate::portfolio::ChangedValueKind,
        attribution: crate::portfolio::ChangeAttribution,
        evidence: &str,
    ) -> crate::portfolio::WhatChangedEntry {
        crate::portfolio::WhatChangedEntry {
            kind,
            detail: "conviction".into(),
            old: "high".into(),
            new: "medium".into(),
            attribution,
            evidence: evidence.into(),
        }
    }

    fn delta_fixture() -> Vec<crate::portfolio::DeltaEntry> {
        vec![
            crate::portfolio::DeltaEntry {
                id: "D1".into(),
                label: "spot: 100.00 -> 92.00".into(),
            },
            crate::portfolio::DeltaEntry {
                id: "D2".into(),
                label: "metric gross margin: 0.4200 -> 0.3800".into(),
            },
        ]
    }

    /// A resolvable external attribution survives as authored — by bracketed id,
    /// by id with trailing prose, or by the entry's label verbatim.
    #[test]
    fn a_resolvable_external_attribution_is_kept() {
        use crate::portfolio::{ChangeAttribution as CA, ChangedValueKind as CK};
        for evidence in ["D2", "[D2]", "d2 — gross margin fell", "metric gross margin: 0.4200 -> 0.3800"] {
            let audit = validate_what_changed(
                &[wc_entry(CK::Conviction, CA::CompanyInformation, evidence)],
                delta_fixture(),
            );
            assert_eq!(audit.entries[0].attribution, CA::CompanyInformation, "{evidence}");
            assert!(audit.downgrades.is_empty(), "{evidence}");
            assert_eq!(audit.self_correction_count, 0);
            assert!(!audit.thesis_changed, "a value-level external move is input movement");
        }
    }

    /// The laundering guard: an external claim resolving to no input-delta entry
    /// is downgraded to self-correction with a logged reason — never kept, never
    /// dropped.
    #[test]
    fn an_unresolvable_external_attribution_downgrades_to_self_correction() {
        use crate::portfolio::{ChangeAttribution as CA, ChangedValueKind as CK};
        let audit = validate_what_changed(
            &[wc_entry(CK::Conviction, CA::MarketData, "the market repriced growth")],
            delta_fixture(),
        );
        assert_eq!(audit.entries[0].attribution, CA::SelfCorrection);
        assert_eq!(audit.downgrades.len(), 1);
        assert!(audit.downgrades[0].contains("downgraded to self-correction"));
        assert_eq!(audit.self_correction_count, 1);
        assert!(audit.thesis_changed, "a self-correction counts as a thesis change");
    }

    /// The rendered section carries every bracketed id plus the attribution rules;
    /// with no entries (a debut) it renders nothing at all.
    #[test]
    fn input_delta_section_renders_ids_and_rules_or_nothing() {
        let s = input_delta_prompt_section(&delta_fixture());
        assert!(s.contains("[D1] spot: 100.00 -> 92.00"), "{s}");
        assert!(s.contains("[D2] metric gross margin"), "{s}");
        assert!(s.contains("downgraded to self-correction"), "{s}");
        assert!(s.contains("never for a rephrasing"), "{s}");
        assert_eq!(input_delta_prompt_section(&[]), "");
    }

    /// The standing-thesis signal: a resolved external thesis-level row trips it;
    /// an authored self-correction needs no evidence and counts.
    #[test]
    fn thesis_scoped_rows_and_self_corrections_set_the_thesis_flag() {
        use crate::portfolio::{ChangeAttribution as CA, ChangedValueKind as CK};
        let audit = validate_what_changed(
            &[wc_entry(CK::Thesis, CA::CompanyInformation, "D2")],
            delta_fixture(),
        );
        assert!(audit.thesis_changed);
        assert_eq!(audit.self_correction_count, 0);

        let audit = validate_what_changed(
            &[wc_entry(CK::SubScore, CA::SelfCorrection, "")],
            delta_fixture(),
        );
        assert!(audit.thesis_changed);
        assert_eq!(audit.self_correction_count, 1);
        assert!(audit.downgrades.is_empty(), "an authored self-correction is no downgrade");
    }

    /// The structural drops — deterministic string checks, no appraisal of the
    /// model's prose: a row claiming no movement (old == new after trim) and an
    /// exact duplicate of a kept row are dropped with logged reasons, so
    /// neither opens a thesis-change episode nor inflates the self-correction
    /// count.
    #[test]
    fn no_move_and_duplicate_rows_are_dropped() {
        use crate::portfolio::{ChangeAttribution as CA, ChangedValueKind as CK};
        // An A -> A thesis row with valid evidence: dropped, no thesis change.
        let mut same = wc_entry(CK::Thesis, CA::CompanyInformation, "D2");
        same.old = "expansion thesis".into();
        same.new = " expansion thesis ".into();
        let audit = validate_what_changed(&[same], delta_fixture());
        assert!(audit.entries.is_empty());
        assert_eq!(audit.downgrades.len(), 1);
        assert!(audit.downgrades[0].contains("no movement"), "{}", audit.downgrades[0]);
        assert!(!audit.thesis_changed);
        assert_eq!(audit.self_correction_count, 0);

        // Two identical self-correction rows: one counted, one dropped.
        let row = wc_entry(CK::SubScore, CA::SelfCorrection, "");
        let audit = validate_what_changed(&[row.clone(), row], delta_fixture());
        assert_eq!(audit.entries.len(), 1);
        assert_eq!(audit.self_correction_count, 1);
        assert_eq!(audit.downgrades.len(), 1);
        assert!(audit.downgrades[0].contains("duplicate"), "{}", audit.downgrades[0]);
        assert!(audit.thesis_changed, "the surviving self-correction still counts");
    }

    /// The prompt-usage collector: a counted response records against the request's
    /// `num_ctx`; a count-less one (an older daemon) still records — with a `None`
    /// prompt count, so its output-side observation (a length stop above all)
    /// survives to the data-health read; and draining empties the buffer.
    #[test]
    fn local_analyst_records_and_drains_prompt_usage() {
        let analyst = LocalAnalyst::new(
            LocalModelClient::new("http://127.0.0.1:1").unwrap(),
            "reasoner".into(),
            String::new(),
        );
        let mut req = ChatRequest::new("m", vec![ChatMessage::user("x")]);
        req.options = Some(options::thinking_general(131_072, NUM_PREDICT_THINKING));
        let counted = crate::local_model::ChatResponse {
            content: String::new(),
            thinking: None,
            prompt_eval_count: Some(120_000),
            eval_count: Some(NUM_PREDICT_THINKING as u64),
            done_reason: Some("length".into()),
        };
        analyst.record_usage("construction".to_string(), &req, &counted);
        let uncounted = crate::local_model::ChatResponse {
            content: String::new(),
            thinking: None,
            prompt_eval_count: None,
            eval_count: None,
            done_reason: Some("length".into()),
        };
        analyst.record_usage("interpret AAPL".to_string(), &req, &uncounted);
        let drained = analyst.take_prompt_usage();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].stage, "construction");
        assert_eq!(drained[0].prompt_tokens, Some(120_000));
        assert_eq!(drained[0].num_ctx, 131_072);
        assert_eq!(drained[0].prompt_chars, 1, "the one-char user message");
        // The output-side half rides the same observation.
        assert_eq!(drained[0].completion_tokens, Some(NUM_PREDICT_THINKING as u64));
        assert_eq!(drained[0].num_predict, Some(NUM_PREDICT_THINKING));
        assert!(drained[0].output_limited, "a length stop is recorded");
        // The count-less row keeps its length-stop observation instead of
        // being dropped with it (attempt-1 review sweep).
        assert_eq!(drained[1].stage, "interpret AAPL");
        assert_eq!(drained[1].prompt_tokens, None);
        assert!(drained[1].output_limited, "the observation survives a missing count");
        assert!(
            analyst.take_prompt_usage().is_empty(),
            "drain empties the buffer"
        );
    }

    fn position(asset_class: AssetClass) -> Position {
        Position {
            symbol: "AAPL".into(),
            description: "Apple".into(),
            asset_class,
            quantity: 100.0,
            cost_basis: 14_000.0,
            market_value: 19_500.0,
            current_price: Some(195.0),
        }
    }

    fn rates() -> RateAnchors {
        RateAnchors {
            dgs2: 0.04,
            dgs10: 0.045,
            dgs10_history: (2023..=2026)
                .flat_map(|y| {
                    ["01-02", "04-01", "07-01", "10-01"]
                        .iter()
                        .map(move |md| DatedValue {
                            date: format!("{y}-{md}"),
                            value: 0.04,
                        })
                })
                .collect(),
            history_gap: None,
            ..Default::default()
        }
    }

    fn strong_financials() -> CompanyFinancials {
        let ends = [
            "2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30", "2025-06-30",
            "2025-03-31", "2024-12-31", "2024-09-30", "2024-06-30", "2024-03-31",
            "2023-12-31", "2023-09-30", "2023-06-30", "2023-03-31", "2022-12-31",
            "2022-09-30",
        ];
        let quarterly_income = ends
            .iter()
            .enumerate()
            .map(|(i, end)| QuarterlyIncomeRow {
                period_end: end.to_string(),
                filing_date: None,
                revenue: Some(100.0e9 - 1.0e9 * i as f64),
                eps_diluted: Some(1.55 - 0.01 * i as f64),
                diluted_shares: Some(1.5e10),
                net_income: None,
                gross_profit: None,
                cost_of_revenue: None,
                operating_income: None,
            })
            .collect();
        let daily_closes = ends
            .iter()
            .rev()
            .enumerate()
            .map(|(i, end)| DatedValue {
                date: end.to_string(),
                value: 130.0 + 4.0 * i as f64,
            })
            .chain(std::iter::once(DatedValue {
                date: "2026-07-15".into(),
                value: 195.0,
            }))
            .collect();
        CompanyFinancials {
            symbol: "AAPL".into(),
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
            daily_closes,
            quarterly_income,
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

    fn dossier(asset_class: AssetClass, financials: CompanyFinancials) -> HoldingDossier {
        HoldingDossier {
            prior_metrics: None,
            semantic_recall: Default::default(),
            company_name: None,
            position: position(asset_class),
            position_delta: PositionDelta::new_position(),
            financials,
            options_signal: OptionsSignal {
                put_call_volume: Some(1.2),
                put_call_open_interest: Some(1.1),
                implied_volatility: Some(0.3),
                iv_skew: Some(0.03),
            },
            profile: InvestorProfile::default_fixture(),
            house_view: HouseView::default(),
            fund: None,
            prior_verdict: None,
            prior_vintage: None,
            prior_spot: None,
            prior_consensus_eps_mid: None,
            prior_matured_notes: Vec::new(),
            prior_grade_parameter_version: None,
            sources: vec!["FMP".into()],
            prior_pre_profit: None,
            listing: None,
            filing_events: None,
            short_interest: None,
            option_overlay: None,
            put_call_backdrop: None,
            commodity_context: Vec::new(),
            sector_benchmark: None,
        }
    }

    /// A priced-fund dossier: a US equity ETF with a full sector-P/E surface.
    fn fund_dossier(fund: FundData) -> HoldingDossier {
        let mut pos = position(AssetClass::Etf);
        pos.symbol = fund.symbol.clone();
        let snapshot: Vec<SectorPe> = [
            ("Technology", 30.0, 34.0),
            ("Financial Services", 14.0, 16.0),
        ]
        .iter()
        .flat_map(|(sector, nyse, nasdaq)| {
            vec![
                SectorPe {
                    sector: sector.to_string(),
                    exchange: "NYSE".into(),
                    date: "2026-07-15".into(),
                    pe: *nyse,
                },
                SectorPe {
                    sector: sector.to_string(),
                    exchange: "NASDAQ".into(),
                    date: "2026-07-15".into(),
                    pe: *nasdaq,
                },
            ]
        })
        .collect();
        let mut history: HashMap<String, Vec<SectorPe>> = HashMap::new();
        let dates = [
            "2022-09-15", "2022-12-15", "2023-03-15", "2023-06-15", "2023-09-15",
            "2023-12-15", "2024-03-15", "2024-06-15", "2024-09-15", "2024-12-15",
            "2025-03-15", "2025-06-15", "2025-09-15", "2025-12-15", "2026-03-15",
            "2026-06-15",
        ];
        for (sector, base) in [("Technology", 26.0), ("Financial Services", 13.0)] {
            let prints = dates
                .iter()
                .enumerate()
                .flat_map(|(i, date)| {
                    ["NYSE", "NASDAQ"].iter().map(move |ex| SectorPe {
                        sector: sector.to_string(),
                        exchange: ex.to_string(),
                        date: date.to_string(),
                        pe: base + 0.2 * i as f64,
                    })
                })
                .collect();
            history.insert(sector.to_ascii_lowercase(), prints);
        }
        let mut financials = CompanyFinancials {
            symbol: fund.symbol.clone(),
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
        };
        financials.gaps = vec![];
        let mut d = dossier(AssetClass::Etf, financials);
        d.position = pos;
        d.fund = Some(FundContext {
            fund,
            sector_pe: snapshot,
            sector_pe_history: history,
            as_of: chrono::NaiveDate::from_ymd_opt(2026, 7, 16).unwrap(),
            positioning: None,
        });
        d
    }

    fn us_equity_fund() -> FundData {
        FundData {
            symbol: "VTI".into(),
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

    #[test]
    fn only_a_verdict_that_reached_interpretation_claims_the_house_view() {
        // The audit's sources must name what the VERDICT consulted, and the house view
        // is loaded once per run and rides every dossier — so the claim has to be
        // earned by reaching an interpretation call, not inherited from assembly.
        //
        // The routes that return first are the reason: the eligibility gate, the
        // listing guard, a net-short or fully-offset position, and every
        // evidence-floor abstention. Enumerating them is the shape that kept going
        // wrong (the first fix covered two of them), so the default is absent and the
        // two interpretation paths opt in.
        let with_house_view = |asset_class, quantity: f64| {
            let mut d = dossier(asset_class, strong_financials());
            d.position.quantity = quantity;
            d.house_view = crate::portfolio::dossier::HouseView {
                recent_summaries: Vec::new(),
                latest_sections: Some("## Market Signal Thesis\nrisk-on.".into()),
            };
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03")
                .unwrap()
                .1
                .sources
        };
        let claims = |sources: Vec<String>| sources.iter().any(|s| s.contains("house view"));

        // The ordinary priced path reads it, so it is recorded.
        assert!(
            claims(with_house_view(AssetClass::Stock, 100.0)),
            "an interpreted holding records the house view it read"
        );

        // A net-short position returns not-rated before either 6f prompt — Codex
        // round 2's reachable case, which the dossier-level gate could not see.
        assert!(
            !claims(with_house_view(AssetClass::Stock, -100.0)),
            "a net-short position never reaches interpretation"
        );
        // A fully-offset (zero) netted position, same route.
        assert!(!claims(with_house_view(AssetClass::Stock, 0.0)));
        // And a class the equity pipeline never grades.
        assert!(!claims(with_house_view(AssetClass::Cash, 100.0)));

        // The listing guard: a guard-terminal stock routes to not-rated on the profile
        // read alone.
        let mut guarded = dossier(AssetClass::Stock, strong_financials());
        guarded.house_view = house_view_of(Some("## Thesis\nrisk-on."), 0);
        guarded.listing = Some(crate::portfolio::listing::ListingResolution::Unresolved);
        assert!(!claims(
            analyze_holding(&StubAnalyst, &guarded, &rates(), "2026-08-03")
                .unwrap()
                .1
                .sources
        ));

        // An evidence-floor abstention: no current price, so the engine stage exits
        // below the floor before any interpretation call.
        let mut floored = dossier(AssetClass::Stock, strong_financials());
        floored.house_view = house_view_of(Some("## Thesis\nrisk-on."), 0);
        floored.financials.current_price = None;
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &floored, &rates(), "2026-08-03").unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::InsufficientEvidence { .. }
        ));
        assert!(!claims(audit.sources));
    }

    /// A house view with the given latest sections and `summaries` recent stances.
    fn house_view_of(
        latest_sections: Option<&str>,
        summaries: usize,
    ) -> crate::portfolio::dossier::HouseView {
        crate::portfolio::dossier::HouseView {
            recent_summaries: (0..summaries)
                .map(|i| {
                    use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
                    crate::agent::ReportSummary {
                        report_id: format!("rep-{i}"),
                        report_type: "weekly_market".into(),
                        created_at: format!("2026-08-0{}", i + 1),
                        title: "Sample headline".into(),
                        risk_posture: RiskPosture::Mixed,
                        market_cycle: MarketCycle::LateCycle,
                        thesis_stance: ThesisStance::Uncertain,
                        header_summary_bullets: vec![],
                        key_risks: vec![],
                        unresolved_questions: vec![],
                        forward_outlook_themes: vec![],
                    }
                })
                .collect(),
            latest_sections: latest_sections.map(|s| s.to_string()),
        }
    }

    #[test]
    fn a_summary_only_house_view_is_claimed_only_by_the_prompt_that_renders_it() {
        // The two prompts render DIFFERENT parts of the house view: the priced prompt
        // renders the latest sections and the recent stances, the role/risk prompt only
        // the latest sections. And `load_house_view` deliberately keeps the summaries
        // when the latest report's Markdown is missing or unreadable — so a
        // summary-only house view is reachable, and reaches a role/risk verdict as
        // nothing at all while its audit claimed the source.
        assert!(
            !role_risk_prompt_renders_house_view(&{
                let mut d = fund_dossier(us_equity_fund());
                d.house_view = house_view_of(None, 2);
                d
            }),
            "the role/risk prompt renders no summaries, so it receives nothing"
        );
        assert!(
            priced_prompt_renders_house_view(&{
                let mut d = dossier(AssetClass::Stock, strong_financials());
                d.house_view = house_view_of(None, 2);
                d
            }),
            "the priced prompt does render the stances, so it does receive them"
        );

        // End to end on the role/risk branch, which the earlier pin never covered.
        let mut bond = us_equity_fund();
        bond.symbol = "BND".into();
        bond.asset_class = Some("Fixed Income".into());
        bond.sector_weights = vec![];
        let role_risk_sources = |house_view| {
            let mut d = fund_dossier(bond.clone());
            d.house_view = house_view;
            let (verdict, audit) =
                analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
            assert!(
                matches!(verdict.disposition, VerdictDisposition::RoleRiskOnly(_)),
                "the fixture must actually take the role/risk branch"
            );
            audit.sources
        };
        let claims = |sources: Vec<String>| sources.iter().any(|s| s.contains("house view"));
        assert!(
            !claims(role_risk_sources(house_view_of(None, 2))),
            "summary-only: the role/risk audit must not claim what its prompt omits"
        );
        assert!(
            claims(role_risk_sources(house_view_of(Some("## Thesis\nrisk-on."), 0))),
            "sections present: it does render them, so the claim is earned"
        );
    }

    /// A stub with a distinct fast tier and reasoner, so the audit's model list can
    /// be checked against the calls that actually ran.
    struct TieredStub;
    impl HoldingAnalyst for TieredStub {
        fn distill(&self, d: &HoldingDossier, f: &ResearchFindings) -> Result<String> {
            StubAnalyst.distill(d, f)
        }
        fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
            StubAnalyst.interpret(input)
        }
        fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
            StubAnalyst.interpret_role_risk(input)
        }
        fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision> {
            StubAnalyst.decide_action(input)
        }
        fn fast_id(&self) -> String {
            "fast-tier".into()
        }
        fn reasoner_id(&self) -> String {
            "reasoner".into()
        }
    }

    /// The role/risk fixture: a bond fund routes to the union's other branch.
    fn bond_fund() -> FundData {
        let mut bond = us_equity_fund();
        bond.symbol = "BND".into();
        bond.asset_class = Some("Fixed Income".into());
        bond.sector_weights = vec![];
        bond
    }

    #[test]
    fn audit_model_ids_name_only_the_models_actually_called() {
        // M3 of the 2026-08-18 doc/code audit: the audit's model list was the
        // analyst's configured roster on every row — a not-rated cash row and an
        // evidence-floor abstention both "used" two models. It must record only
        // the calls that ran.
        let run = |d: &HoldingDossier| {
            analyze_holding(&TieredStub, d, &rates(), "2026-08-03").unwrap()
        };

        // No-model exits persist an empty list: the eligibility gate ...
        let (v, a) = run(&dossier(AssetClass::Cash, strong_financials()));
        assert!(matches!(v.disposition, VerdictDisposition::NotRated { .. }));
        assert!(a.model_ids.is_empty(), "{:?}", a.model_ids);
        // ... a net-short position ...
        let mut short = dossier(AssetClass::Stock, strong_financials());
        short.position.quantity = -100.0;
        let (v, a) = run(&short);
        assert!(matches!(v.disposition, VerdictDisposition::NotRated { .. }));
        assert!(a.model_ids.is_empty(), "{:?}", a.model_ids);
        // ... the listing guard ...
        let mut guarded = dossier(AssetClass::Stock, strong_financials());
        guarded.listing = Some(crate::portfolio::listing::ListingResolution::Unresolved);
        let (_, a) = run(&guarded);
        assert!(a.model_ids.is_empty(), "{:?}", a.model_ids);
        // ... and an evidence-floor abstention (no current price).
        let mut floored = dossier(AssetClass::Stock, strong_financials());
        floored.financials.current_price = None;
        let (v, a) = run(&floored);
        assert!(matches!(v.disposition, VerdictDisposition::InsufficientEvidence { .. }));
        assert!(a.model_ids.is_empty(), "{:?}", a.model_ids);

        // The role/risk branch runs the reasoner only (role read + action call) —
        // never the fast tier, which only distills on the priced path.
        let (v, a) = run(&fund_dossier(bond_fund()));
        assert!(matches!(v.disposition, VerdictDisposition::RoleRiskOnly(_)));
        assert_eq!(a.model_ids, vec!["reasoner".to_string()]);

        // The priced path runs both, in call order: distill (fast) then the
        // reasoner's interpretation + action call.
        let (v, a) = run(&dossier(AssetClass::Stock, strong_financials()));
        assert!(matches!(v.disposition, VerdictDisposition::Priced(_)));
        assert_eq!(a.model_ids, vec!["fast-tier".to_string(), "reasoner".to_string()]);
        // And the priced fund path likewise.
        let (v, a) = run(&fund_dossier(us_equity_fund()));
        assert!(matches!(v.disposition, VerdictDisposition::Priced(_)), "{v:?}");
        assert_eq!(a.model_ids, vec!["fast-tier".to_string(), "reasoner".to_string()]);

        // One entry when the fast tier is the reasoner (the blank-fast-tier
        // fallback, or a same-model stub) — never the same id twice.
        let (_, a) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert_eq!(a.model_ids, vec!["stub-analyst".to_string()]);
    }

    #[test]
    fn rate_anchors_are_a_source_only_where_a_priced_output_computed_from_them() {
        // The FRED anchors feed the scenario targets and the hurdle read — priced
        // outputs only. The role/risk branch and every earlier exit compute nothing
        // from them, so their audits must not name them (M3, 2026-08-18).
        let sources = |d: &HoldingDossier| {
            analyze_holding(&StubAnalyst, d, &rates(), "2026-08-03")
                .unwrap()
                .1
                .sources
        };
        let names_fred = |s: &[String]| s.iter().any(|x| x == RATE_ANCHORS_SOURCE);

        assert!(names_fred(&sources(&dossier(AssetClass::Stock, strong_financials()))));
        assert!(names_fred(&sources(&fund_dossier(us_equity_fund()))));

        assert!(!names_fred(&sources(&fund_dossier(bond_fund()))), "role/risk");
        assert!(!names_fred(&sources(&dossier(AssetClass::Cash, strong_financials()))));
        let mut floored = dossier(AssetClass::Stock, strong_financials());
        floored.financials.current_price = None;
        assert!(!names_fred(&sources(&floored)), "evidence-floor abstention");
        let mut short = dossier(AssetClass::Stock, strong_financials());
        short.position.quantity = -100.0;
        assert!(!names_fred(&sources(&short)), "net-short");
    }

    #[test]
    fn role_risk_audit_persists_the_branch_computed_metrics() {
        // The role/risk branch computes the expense ratio plus the price-derived
        // legs (the surface its ledger evaluation reads); the audit row must carry
        // them, not the empty default it used to persist (M3, 2026-08-18).
        let (verdict, audit) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(bond_fund()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert!(matches!(verdict.disposition, VerdictDisposition::RoleRiskOnly(_)));
        assert_eq!(audit.metrics.expense_ratio, Some(0.0003));
        assert!(audit.metrics.trailing_return.is_some(), "{:?}", audit.metrics);
        assert!(audit.metrics.return_volatility.is_some(), "{:?}", audit.metrics);
        // The reduced surface only — no statement-derived stock legs.
        assert!(audit.metrics.pe_ratio.is_none());
        assert!(audit.metrics.revenue_growth.is_none());
    }

    #[test]
    fn an_empty_action_rationale_fails_the_holding() {
        // M6 of the 2026-08-18 audit: the schema types the rationale as any string,
        // so the contract's "never empty" is enforced app-side, fail-hard like the
        // rest of the model stage — on both branches' action calls.
        struct EmptyRationaleStub;
        impl HoldingAnalyst for EmptyRationaleStub {
            fn distill(&self, d: &HoldingDossier, f: &ResearchFindings) -> Result<String> {
                StubAnalyst.distill(d, f)
            }
            fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
                StubAnalyst.interpret(input)
            }
            fn interpret_role_risk(
                &self,
                input: &RoleRiskInput,
            ) -> Result<RoleRiskInterpretation> {
                StubAnalyst.interpret_role_risk(input)
            }
            fn decide_action(
                &self,
                _input: &ActionInput,
            ) -> Result<crate::portfolio::ActionDecision> {
                Ok(crate::portfolio::ActionDecision {
                    action: Action::Hold,
                    rationale: "   \n".to_string(),
                })
            }
            fn fast_id(&self) -> String {
                "empty".into()
            }
            fn reasoner_id(&self) -> String {
                "empty".into()
            }
        }
        let err = analyze_holding(
            &EmptyRationaleStub,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .expect_err("an empty rationale must fail the priced holding");
        let msg = format!("{err:#}");
        assert!(msg.contains("AAPL") && msg.contains("empty rationale"), "{msg}");

        let err = analyze_holding(
            &EmptyRationaleStub,
            &fund_dossier(bond_fund()),
            &rates(),
            "2026-08-03",
        )
        .expect_err("an empty rationale must fail the role/risk holding");
        let msg = format!("{err:#}");
        assert!(msg.contains("BND") && msg.contains("empty rationale"), "{msg}");

        // A rationale with content passes untouched — the one-sentence shape is a
        // prompt preference, not validated.
        assert!(analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .is_ok());
    }

    #[test]
    fn gradeable_holding_produces_a_priced_verdict_offline() {
        let (verdict, audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        // The app-set holdings-change tag rides on the verdict (the dossier's delta is
        // a new position), independent of the model's prose what_changed.
        assert_eq!(verdict.position_change, PositionChange::New);
        match verdict.disposition {
            VerdictDisposition::Priced(g) => {
                // Engine numbers carried through; model judgment present.
                assert!(matches!(
                    g.grade,
                    crate::portfolio::Grade::A
                        | crate::portfolio::Grade::B
                        | crate::portfolio::Grade::C
                ));
                assert_eq!(g.what_changed, "new holding");
                // The model's base-case justification is carried through, not dropped.
                assert!(!g.price_target_rationale.is_empty());
                // The options signal rides on the verdict but never entered the grade.
                assert!(g.options_signal.put_call_volume.is_some());
                // The new engine reads persist on the priced branch.
                assert!(g.risk_tier.is_some());
                assert!(g.dead_money.is_some());
            }
            other => panic!("expected a priced verdict, got {other:?}"),
        }
        assert_eq!(audit.prompt_version, PROMPT_VERSION);
        // The audit records how the targets were derived, versioned for calibration.
        let meta = audit.target_meta.expect("target meta rides the audit");
        assert_eq!(
            meta.parameter_version,
            crate::portfolio::engine::SCENARIO_TARGET_PARAMETER_VERSION
        );
    }

    #[test]
    fn priced_fund_takes_the_reduced_path_with_the_grade_contract() {
        let (verdict, audit) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(us_equity_fund()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        match verdict.disposition {
            VerdictDisposition::Priced(g) => {
                // The fund grade contract: neutral-imputed quality + the visible
                // low-confidence marker; fund-form targets.
                assert_eq!(g.sub_scores.quality, 50.0);
                assert!(g.low_confidence_grade);
                let tm = g.price_targets.twelve_month.as_ref().unwrap();
                assert!(tm.methodology.contains("fund exposure composite"));
                assert!(g.risk_tier.is_some());
                // The deterministic classification reaches the card-visible verdict.
                assert_eq!(g.fund_class_label.as_deref(), Some("US equity fund"));
                assert!(!g.structural_flag);
            }
            other => panic!("expected a priced fund verdict, got {other:?}"),
        }
        assert!(audit.target_meta.unwrap().flat_driver);
    }

    #[test]
    fn engine_gap_notes_reach_the_audit() {
        // A partially covered fund (80% P/E-usable) grades, and the engine's
        // uncovered-share note lands in the audit's degraded inputs — reported,
        // never silently dropped.
        let mut partial = us_equity_fund();
        partial.sector_weights = vec![
            ("Technology".into(), 0.5),
            ("Financial Services".into(), 0.3),
            ("Utilities".into(), 0.2), // unpriced by the snapshot/history
        ];
        let (verdict, audit) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(partial),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert!(matches!(verdict.disposition, VerdictDisposition::Priced(_)));
        assert!(
            audit
                .degraded_inputs
                .iter()
                .any(|g| g.contains("composite P/E coverage")),
            "{:?}",
            audit.degraded_inputs
        );
    }

    #[test]
    fn unpriceable_fund_class_returns_the_role_risk_branch() {
        let mut bond = us_equity_fund();
        bond.symbol = "BND".into();
        bond.asset_class = Some("Fixed Income".into());
        bond.sector_weights = vec![];
        let (verdict, _audit) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(bond),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        match verdict.disposition {
            VerdictDisposition::RoleRiskOnly(r) => {
                assert_eq!(r.class_label, "bond fund");
                // The per-holding action call authors the branch's action; the
                // stub's role/risk decision is hold.
                assert_eq!(r.action, Action::Hold);
                assert!(!r.role_summary.is_empty());
                assert!(!r.evidence_gaps.is_empty());
            }
            other => panic!("expected role_risk_only, got {other:?}"),
        }
    }

    #[test]
    fn role_risk_full_pass_evaluates_the_price_derived_ledger_series() {
        // The full role-risk pass must cover the SAME fund-computable surface
        // the quick check evaluates (expense ratio + the price-derived legs) —
        // with metrics carrying only the expense ratio, a sweep-confirmed
        // trailing-return crossing read unevaluable here, was never
        // acknowledged, and re-raised on every later sweep after the
        // successful pass cleared the store.
        let mut bond = us_equity_fund();
        bond.symbol = "BND".into();
        bond.asset_class = Some("Fixed Income".into());
        bond.sector_weights = vec![];
        let (mut prior, _) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(bond.clone()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        // Re-point the prior ledger's falsifier at a price-derived series.
        let ledger = prior.thesis_ledger.as_mut().expect("role-risk ledger");
        let falsifier = ledger
            .conditions
            .iter_mut()
            .find(|c| c.role == ConditionRole::Falsifier)
            .expect("a falsifier");
        falsifier.quant = Some(QuantCore {
            series: engine::LedgerSeries::TrailingReturn,
            comparator: LedgerComparator::Below,
            threshold: -0.40,
            margin: 0.02,
        });
        let mut d = fund_dossier(bond);
        d.prior_verdict = Some(prior);
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-04").unwrap();
        let la = audit.ledger_audit.expect("ledger audit");
        assert!(
            !la.unevaluable.iter().any(|u| u.contains("trailing")),
            "the price-derived series must evaluate on the full role-risk pass: {:?}",
            la.unevaluable
        );
        // The evaluated state keys to the marks' trading day, proving the leg
        // actually resolved rather than silently skipping.
        let evaluated = verdict
            .thesis_ledger
            .as_ref()
            .and_then(|l| {
                l.conditions
                    .iter()
                    .find(|c| {
                        c.quant.as_ref().map(|q| q.series)
                            == Some(engine::LedgerSeries::TrailingReturn)
                    })
            })
            .and_then(|c| c.eval_state.as_ref())
            .expect("an evaluated state on the carried condition");
        assert_eq!(evaluated.last_observation_id.as_deref(), Some("2026-07-15"));
    }

    #[test]
    fn ineligible_asset_class_is_not_rated_without_a_model_call() {
        let (verdict, _audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::OptionContract, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::NotRated { .. }
        ));
    }

    #[test]
    fn a_net_short_position_is_not_rated_with_a_short_reason() {
        // A net-short equity is a direction the prescriptive layer doesn't model —
        // not-rated with a short-position reason, never graded with long-side
        // semantics (`docs/portfolio-analysis.md` §Asset eligibility).
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.position.quantity = -100.0;
        d.position.market_value = -19_500.0;
        let (verdict, _audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        match verdict.disposition {
            VerdictDisposition::NotRated { reason } => {
                assert!(reason.contains("short"), "{reason}");
            }
            other => panic!("expected not-rated, got {other:?}"),
        }
    }

    #[test]
    fn a_fully_offset_zero_position_is_not_rated_not_graded_long() {
        // Exactly-zero netted shares (long and short legs fully offset,
        // deliberately kept by netting) is neither long nor short — the strict
        // `< 0.0` gate previously waved it onto the long-semantics ladder with
        // zero economic exposure.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.position.quantity = 0.0;
        d.position.market_value = 0.0;
        let (verdict, _audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-05").unwrap();
        match verdict.disposition {
            VerdictDisposition::NotRated { reason } => {
                assert!(reason.contains("offset"), "{reason}");
            }
            other => panic!("expected not-rated, got {other:?}"),
        }
    }

    #[test]
    fn six_g_downgrades_a_series_the_asset_class_never_computes() {
        // A statement series on a fund validates as quantitative under a
        // class-blind check, then types unevaluable on every sweep — the family
        // never clears and every selective run badges the holding. The
        // class-aware check downgrades it to qualitative at 6g instead; the
        // expense ratio is the stock-side mirror.
        let mut fund_draft = stub_ledger_draft(None, "VTI", false);
        fund_draft.falsifiers = vec![FalsifierDraft {
            statement: "net margin below 5%".into(),
            quant: Some(QuantCoreDraft {
                series: "net-margin".into(),
                comparator: "below".into(),
                threshold: 0.05,
                margin: 0.0,
            }),
            technology_class: false,
            tripped: false,
        }];
        fund_draft.triggers = vec![];
        let (ledger, audit) =
            validate_ledger_rewrite(&fund_draft, None, None, LedgerBranch::Priced, true, None, None);
        let cond = ledger
            .conditions
            .iter()
            .find(|c| c.statement.contains("net margin"))
            .unwrap();
        assert!(cond.quant.is_none(), "downgraded to qualitative");
        assert!(
            audit.downgraded.iter().any(|d| d.contains("fund-path")),
            "{:?}",
            audit.downgraded
        );

        let mut stock_draft = stub_ledger_draft(None, "AAPL", false);
        stock_draft.falsifiers = vec![FalsifierDraft {
            statement: "expense ratio above 40 bps".into(),
            quant: Some(QuantCoreDraft {
                series: "expense-ratio".into(),
                comparator: "above".into(),
                threshold: 0.004,
                margin: 0.0,
            }),
            technology_class: false,
            tripped: false,
        }];
        stock_draft.triggers = vec![];
        let (ledger, audit) = validate_ledger_rewrite(
            &stock_draft,
            None,
            None,
            LedgerBranch::Priced,
            false,
            None,
            None,
        );
        let cond = ledger
            .conditions
            .iter()
            .find(|c| c.statement.contains("expense ratio"))
            .unwrap();
        assert!(cond.quant.is_none(), "downgraded to qualitative");
        assert!(
            audit.downgraded.iter().any(|d| d.contains("stock-path")),
            "{:?}",
            audit.downgraded
        );
    }

    #[test]
    fn an_unsupported_listing_is_not_rated_with_that_reason() {
        use crate::portfolio::listing::ListingResolution;
        // Strong financials prove the gate routes before the engine could grade —
        // no resolution and a non-US primary listing are structural can't-grades
        // (`docs/portfolio-analysis.md` §Asset eligibility).
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.listing = Some(ListingResolution::Unresolved);
        let (verdict, _audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-04").unwrap();
        match verdict.disposition {
            VerdictDisposition::NotRated { reason } => {
                assert!(reason.contains("unsupported listing"), "{reason}");
            }
            other => panic!("expected not-rated, got {other:?}"),
        }

        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.listing = Some(ListingResolution::NonUs { exchange: "LSE".into() });
        let (verdict, _audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-04").unwrap();
        match verdict.disposition {
            VerdictDisposition::NotRated { reason } => {
                assert!(
                    reason.contains("unsupported listing") && reason.contains("LSE"),
                    "{reason}"
                );
            }
            other => panic!("expected not-rated, got {other:?}"),
        }
    }

    #[test]
    fn a_conflicting_identity_abstains_and_retains_the_prior_ledger() {
        use crate::portfolio::listing::ListingResolution;
        // The evidence floor's conflicting-identity arm: a wrong-issuer mapping
        // must never grade the wrong company's financials — and like every
        // abstention, the standing ledger rides through unchanged.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.listing = Some(ListingResolution::Conflict {
            fmp_name: "Zenith Mining Corp".into(),
        });
        d.prior_verdict = Some(HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated { reason: "fixture".into() },
            thesis_ledger: Some(prior_with_conditions()),
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        });
        let (verdict, _audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-04").unwrap();
        match &verdict.disposition {
            VerdictDisposition::InsufficientEvidence { reason } => {
                assert!(
                    reason.contains("conflicting identity") && reason.contains("Zenith"),
                    "{reason}"
                );
            }
            other => panic!("expected insufficient-evidence, got {other:?}"),
        }
        assert_eq!(verdict.thesis_ledger, Some(prior_with_conditions()));
    }

    #[test]
    fn an_unverified_guard_proceeds_and_records_the_degraded_input() {
        use crate::portfolio::listing::ListingResolution;
        // An FMP outage must never mass-not-rate a book: the holding grades
        // normally with the unverified cross-check recorded as a degraded input.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.listing = Some(ListingResolution::Unverified {
            detail: "FMP profile unavailable (unavailable)".into(),
        });
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-04").unwrap();
        assert!(matches!(verdict.disposition, VerdictDisposition::Priced(_)));
        assert!(
            audit
                .degraded_inputs
                .iter()
                .any(|g| g.contains("listing-resolution guard unverified")),
            "{:?}",
            audit.degraded_inputs
        );
    }

    #[test]
    fn below_the_evidence_floor_abstains() {
        // Only a price — the engine abstains, and no model interpretation is attempted.
        let thin = CompanyFinancials {
            symbol: "X".into(),
            current_price: Some(50.0),
            ..CompanyFinancials::default()
        };
        let (verdict, _audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, thin),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::InsufficientEvidence { .. }
        ));
    }

    #[test]
    fn position_change_line_shows_quantity_and_cost_basis_moves() {
        let increased = PositionDelta {
            change: PositionChange::Increased,
            prior_quantity: Some(100.0),
            prior_cost_basis: Some(14_000.0),
        };
        let line = describe_position_change(&increased, 140.0, 19_500.0);
        assert!(line.contains("INCREASED"), "{line}");
        assert!(line.contains("100") && line.contains("140"), "quantity move: {line}");
        assert!(line.contains("14000") && line.contains("19500"), "cost-basis move: {line}");
        // Dollar-marked like the header this line sits under: bare integers here read
        // as per-share against a header that says total (Finding 4).
        assert!(line.contains("$14000") && line.contains("$19500 total"), "units: {line}");
        // The debut line must disarm the fresh-purchase misread: NEW means no
        // prior verdict, and the cost basis is account history (v8 tightening).
        let debut = describe_position_change(&PositionDelta::new_position(), 10.0, 1_000.0);
        assert!(debut.starts_with("NEW (no prior verdict"), "{debut}");
        assert!(debut.contains("not a recent entry"), "{debut}");
    }

    #[test]
    fn interpretation_prompt_carries_the_engine_numbers_and_the_do_not_invent_rule() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let input = InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        };
        let user = interpretation_user_prompt(&input);
        assert!(user.contains("ENGINE GRADE (the baseline arm"), "{user}");
        assert!(user.contains("ENGINE SUB-SCORES"), "{user}");
        assert!(user.contains("NOT a grade input"), "options proxy is flagged: {user}");
        assert!(user.contains("RISK TIER"), "{user}");
        assert!(user.contains("YOUR MODEL ARM"), "{user}");
        assert!(user.contains("unrestricted"), "{user}");
        let system = interpretation_system_prompt();
        assert!(system.contains("TWO ARMS"), "{system}");
        assert!(system.contains("MODEL ARM"), "{system}");
        assert!(!system.contains("never outside them"), "{system}");

        // The prompt-adjustments slice (portfolio-v3): target provenance always
        // renders, the dead-money read is a weighed input (not an instruction), and
        // the system prompt defines conviction and scopes the house view.
        assert!(user.contains("TARGET PROVENANCE"), "{user}");
        assert!(user.contains("one input to weigh"), "{user}");
        let system = interpretation_system_prompt();
        assert!(system.contains("Conviction means"), "{system}");
        assert!(system.contains("horizon reads and market-setup context"), "{system}");

        // Profile independence is input isolation, not instruction
        // (`docs/portfolio-workflow.md` §Step 6f "deliberately absent"): the
        // intrinsic prompt renders no investor profile; the profile enters at
        // the per-holding action call only.
        assert!(!user.contains("INVESTOR PROFILE"), "{user}");
        assert!(system.contains("profile-independent"), "{system}");
        assert!(system.contains("never by itself a reason to exit"), "{system}");
        // Interpretation authors no action under the tunnel-vision contract —
        // the dedicated action call owns it.
        assert!(system.contains("Do NOT choose a portfolio action here"), "{system}");
    }

    #[test]
    fn commodity_context_renders_as_dated_levels_in_both_prompts() {
        use crate::portfolio::dossier::{CommodityGroup, CommodityPrint};
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.commodity_context = vec![CommodityPrint {
            label: "WTI Crude Oil".into(),
            unit: "USD per barrel".into(),
            group: CommodityGroup::Energy,
            latest: crate::portfolio::engine::DatedValue {
                date: "2026-08-18".into(),
                value: 78.4,
            },
            trailing: Some(crate::portfolio::engine::DatedValue {
                date: "2025-08-20".into(),
                value: 82.1,
            }),
        }];
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(interp.contains("COMMODITY CONTEXT"), "{interp}");
        assert!(interp.contains("78.40 USD per barrel (as of 2026-08-18"), "{interp}");
        assert!(interp.contains("-4.5% vs 82.10 on 2025-08-20"), "{interp}");
        // A holding with no sector-matched prints renders no section.
        let bare = dossier(AssetClass::Stock, strong_financials());
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &bare,
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(!interp.contains("COMMODITY CONTEXT"), "{interp}");
    }

    #[test]
    fn a_fired_pre_flag_renders_and_an_unfired_one_stays_silent() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let flag = |fired| engine::TechEventPreFlag {
            fired,
            relative_move: -0.12,
            threshold: 0.08,
            sessions: 4,
            benchmark: "XLK".into(),
        };
        let prompt = |f: Option<&engine::TechEventPreFlag>| {
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: &d,
                engine: &engine_output,
                distilled: "findings",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: f,
                narrative: None,
            })
        };
        let fired = flag(true);
        let user = prompt(Some(&fired));
        assert!(user.contains("TECHNOLOGY-EVENT PRE-FLAG"), "{user}");
        assert!(user.contains("-12.0% vs XLK over 4 sessions"), "{user}");
        assert!(user.contains("asserts nothing about the cause"), "{user}");
        let unfired = flag(false);
        assert!(!prompt(Some(&unfired)).contains("TECHNOLOGY-EVENT PRE-FLAG"));
        assert!(!prompt(None).contains("TECHNOLOGY-EVENT PRE-FLAG"));
    }

    #[test]
    fn the_pre_flag_wires_through_analyze_holding_onto_the_audit() {
        use crate::portfolio::dossier::BenchmarkSeries;
        // A carried stock with a benchmark series records an evaluated flag (or
        // its typed gap); a debut records neither.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.prior_vintage = Some("2026-07-20T14:00:00Z".to_string());
        d.sector_benchmark = Some(BenchmarkSeries {
            symbol: "XLK".into(),
            closes: d.financials.daily_closes.clone(),
        });
        let (_, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(
            audit.tech_event_pre_flag.is_some()
                || audit
                    .degraded_inputs
                    .iter()
                    .any(|g| g.contains("technology-event pre-flag unevaluable")),
            "an evaluable carried stock records the flag or its typed gap: {audit:?}"
        );
        // A carried stock with NO benchmark series records the typed gap.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.prior_vintage = Some("2026-07-20T14:00:00Z".to_string());
        let (_, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(audit.tech_event_pre_flag.is_none());
        assert!(
            audit
                .degraded_inputs
                .iter()
                .any(|g| g.contains("no sector benchmark series")),
            "{:?}",
            audit.degraded_inputs
        );
        // A debut records neither a flag nor a gap.
        let d = dossier(AssetClass::Stock, strong_financials());
        let (_, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(audit.tech_event_pre_flag.is_none());
        assert!(!audit
            .degraded_inputs
            .iter()
            .any(|g| g.contains("technology-event pre-flag")));
    }

    #[test]
    fn newly_consulted_feeds_land_on_the_audit_source_labels() {
        // The actually-consulted discipline extends to the Step-5 feeds: the
        // backdrop and a fund's positioning label where a prompt rendered
        // them, the benchmark where the pre-flag evaluation read it — and none
        // of them label where absent (Codex 2026-08-20, finding 5).
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.put_call_backdrop = Some(crate::cboe::PutCallBackdrop {
            as_of: "2026-08-19".into(),
            total: Some(0.8),
            index: None,
            equity: None,
        });
        d.prior_vintage = Some("2026-07-20T14:00:00Z".to_string());
        d.sector_benchmark = Some(crate::portfolio::dossier::BenchmarkSeries {
            symbol: "XLK".into(),
            closes: d.financials.daily_closes.clone(),
        });
        let (_, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(
            audit.sources.iter().any(|s| s.contains("CBOE daily put/call")),
            "{:?}",
            audit.sources
        );
        assert!(
            audit.sources.iter().any(|s| s.contains("sector benchmark series")),
            "{:?}",
            audit.sources
        );

        let mut fd = fund_dossier(us_equity_fund());
        if let Some(f) = fd.fund.as_mut() {
            f.positioning = Some(crate::data_sources::CotPositioning {
                contract: "E-Mini S&P 500".into(),
                contract_code: "13874A".into(),
                asset_class: "equity-index".into(),
                report_date: "2026-08-11".into(),
                open_interest: 2_579_920.0,
                spec_net: -515_520.0,
                spec_net_weekly_change: None,
                spec_pct_oi_long: None,
                real_money_net: Some(984_009.0),
                real_money_net_weekly_change: None,
            });
        }
        let (_, audit) = analyze_holding(&StubAnalyst, &fd, &rates(), "2026-08-03").unwrap();
        assert!(
            audit.sources.iter().any(|s| s.contains("CFTC COT positioning")),
            "{:?}",
            audit.sources
        );

        // Absent feeds claim nothing.
        let bare = dossier(AssetClass::Stock, strong_financials());
        let (_, audit) = analyze_holding(&StubAnalyst, &bare, &rates(), "2026-08-03").unwrap();
        assert!(!audit.sources.iter().any(|s| {
            s.contains("CBOE") || s.contains("CFTC") || s.contains("benchmark")
        }));

        // The commodity context labels only where a prompt rendered it: on the
        // interpreted path, never on an early exit that consumed no prompt
        // (Codex 2026-08-20 round 2, finding 4).
        let commodity_print = crate::portfolio::dossier::CommodityPrint {
            label: "WTI Crude Oil".into(),
            unit: "USD per barrel".into(),
            group: crate::portfolio::dossier::CommodityGroup::Energy,
            latest: crate::portfolio::engine::DatedValue {
                date: "2026-08-18".into(),
                value: 78.4,
            },
            trailing: None,
        };
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.commodity_context = vec![commodity_print.clone()];
        let (_, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(
            audit.sources.iter().any(|s| s.contains("commodity context")),
            "{:?}",
            audit.sources
        );
        let mut floored = dossier(AssetClass::Stock, strong_financials());
        floored.commodity_context = vec![commodity_print];
        floored.financials.current_price = None;
        let (_, audit) = analyze_holding(&StubAnalyst, &floored, &rates(), "2026-08-03").unwrap();
        assert!(
            !audit.sources.iter().any(|s| s.contains("commodity context")),
            "an evidence-floor exit renders no prompt, so it claims no commodity \
             source: {:?}",
            audit.sources
        );

        // An unreadable prior vintage never hands the benchmark series to the
        // evaluation, so it must not label it.
        let mut unreadable = dossier(AssetClass::Stock, strong_financials());
        unreadable.prior_vintage = Some("soon".to_string());
        unreadable.sector_benchmark = Some(crate::portfolio::dossier::BenchmarkSeries {
            symbol: "XLK".into(),
            closes: unreadable.financials.daily_closes.clone(),
        });
        let (_, audit) =
            analyze_holding(&StubAnalyst, &unreadable, &rates(), "2026-08-03").unwrap();
        assert!(
            !audit.sources.iter().any(|s| s.contains("benchmark")),
            "{:?}",
            audit.sources
        );
        assert!(audit
            .degraded_inputs
            .iter()
            .any(|g| g.contains("unreadable prior vintage")));
    }

    #[test]
    fn put_call_backdrop_renders_as_broad_market_context_on_both_read_prompts() {
        let backdrop = crate::cboe::PutCallBackdrop {
            as_of: "August 19, 2026".into(),
            total: Some(0.80),
            index: Some(0.97),
            equity: None,
        };
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.put_call_backdrop = Some(backdrop.clone());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(interp.contains("MARKET OPTIONS SENTIMENT"), "{interp}");
        assert!(interp.contains("as of August 19, 2026"), "{interp}");
        assert!(interp.contains("total 0.80, index 0.97, equity (gap)"), "{interp}");

        let mut fd = fund_dossier(us_equity_fund());
        fd.put_call_backdrop = Some(backdrop);
        let role = role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: &fd,
            readout: &RoleRiskReadout::default(),
            ledger_eval: None,
        });
        assert!(role.contains("MARKET OPTIONS SENTIMENT"), "{role}");
        // Absent, neither prompt claims it.
        let bare = dossier(AssetClass::Stock, strong_financials());
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &bare,
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(!interp.contains("MARKET OPTIONS SENTIMENT"), "{interp}");
    }

    #[test]
    fn a_tripped_hard_forensic_binds_the_engine_arm_and_annotates_the_audit() {
        use crate::portfolio::{Conviction, ForensicFilingState};
        let event = crate::sec::ForensicEvent {
            kind: crate::sec::ForensicEventKind::Restatement,
            issuer: "AAPL".into(),
            filing_date: "2026-07-20".into(),
            source: "8-K accession 0000320193-26-000042".into(),
            confidence: "filing-declared item code".into(),
        };
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.filing_events = Some(ForensicFilingState::Events { events: vec![event] });
        let (v, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();

        // The audit records the sweep state with the engine-matched hard rule.
        let forensic = audit.forensic.expect("the sweep state persists on the audit");
        assert!(forensic.state.hard_tripped());
        assert!(
            forensic.matched_rule.as_deref().unwrap_or("").contains("capped Low"),
            "{forensic:?}"
        );
        // The engine arm is bound: stand-in conviction hard-capped Low; the
        // model arm persists as authored (the stub's own conviction survives).
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        assert_eq!(graded.engine_view.conviction, Conviction::Low);
        // Both prompts render the typed section; the sweep is a consulted source.
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(interp.contains("HARD TRIGGER TRIPPED"), "{interp}");
        assert!(interp.contains("restatement (Item 4.02 non-reliance)"), "{interp}");
        let engine_set = engine::feasible_actions(
            engine_output.grade,
            &engine_output.hurdle,
            None,
            true,
        );
        assert!(!engine_set.contains(&Action::Add));
        let action = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced {
                graded,
                engine: &engine_output,
                pre_profit: None,
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(action.contains("HARD TRIGGER TRIPPED"), "{action}");

        // An `Unknown` sweep renders as unknown and never trips the rule.
        let mut unknown = dossier(AssetClass::Stock, strong_financials());
        unknown.filing_events = Some(ForensicFilingState::Unknown {
            reason: "no CIK mapping for AAPL".into(),
            queried: false,
        });
        let (v, audit) =
            analyze_holding(&StubAnalyst, &unknown, &rates(), "2026-08-03").unwrap();
        let forensic = audit.forensic.expect("unknown persists too");
        assert!(!forensic.state.hard_tripped());
        assert!(forensic.matched_rule.is_none());
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        assert_ne!(graded.engine_view.conviction, Conviction::Low);
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &unknown,
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(interp.contains("UNKNOWN — no CIK mapping"), "{interp}");
    }

    #[test]
    fn action_prompt_carries_the_profile_the_engine_set_and_the_verdict_digest() {
        // The action call is the profile's ONE entry point (tunnel vision): the
        // prompt renders the finished verdict digest, the engine's per-holding
        // set (its own pick withheld — the ruled 6f precedent), and the profile.
        let d = dossier(AssetClass::Stock, strong_financials());
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let engine_set =
            engine::feasible_actions(engine_output.grade, &engine_output.hurdle, None, false);
        let input = ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced {
                graded,
                engine: &engine_output,
                pre_profit: None,
            },
            engine_set: &engine_set,
            profile: &d.profile,
        };
        let user = action_user_prompt(&input);
        assert!(user.contains("INVESTOR PROFILE"), "{user}");
        assert!(user.contains("ENGINE SET"), "{user}");
        assert!(user.contains("deliberately not shown"), "{user}");
        assert!(user.contains("THE VERDICT"), "{user}");
        assert!(user.contains("Unrealized P/L"), "{user}");
        let system = action_system_prompt();
        assert!(system.contains("TUNNEL VISION IS THE CONTRACT"), "{system}");
        assert!(system.contains("ONE rung"), "{system}");
        assert!(system.contains("never a bound"), "{system}");
        assert!(system.contains("exactly ONE sentence"), "{system}");
        // No whole-book vocabulary leaks into the user prompt — the cash row
        // included: the system prompt promises no book-level capital input is
        // given, so the profile renders without it (Codex 2026-08-14, finding 3).
        assert!(!user.contains("concentration"), "{user}");
        assert!(!user.contains("OVERLAP"), "{user}");
        assert!(!user.contains("- cash:"), "{user}");
        assert!(!user.contains("unconstrained"), "{user}");
    }

    #[test]
    fn evidence_leg_sections_render_when_present_and_stay_silent_when_absent() {
        use crate::portfolio::dossier::{
            OptionOverlay, OverlayClass, OverlayDirection, OverlayLeg,
        };
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = |d: &HoldingDossier, narrative: Option<&engine::NarrativeRead>| {
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: d,
                engine: &engine_output,
                distilled: "findings",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative,
            })
        };
        // Absent legs stay silent — no empty scaffolding sections.
        let base = interp(&d, None);
        assert!(!base.contains("SHORT INTEREST"), "{base}");
        assert!(!base.contains("SAME-UNDERLYING OPTION OVERLAY"), "{base}");
        assert!(!base.contains("NARRATIVE VS REALITY"), "{base}");
        // Present legs render with their evidence framing.
        d.short_interest = Some(crate::finra::ShortInterestRead {
            settlement_date: "2026-07-31".into(),
            current_short_interest: 5_000_000.0,
            previous_short_interest: Some(4_000_000.0),
            average_daily_volume: Some(2_000_000.0),
            days_to_cover: Some(2.5),
        });
        d.option_overlay = Some(OptionOverlay {
            legs: vec![OverlayLeg {
                contract: "AAPL  270115C00210000".into(),
                direction: OverlayDirection::Short,
                quantity: 1.0,
                kind: Some(crate::schwab::OptionKind::Call),
                strike: Some(210.0),
                expiry: Some("2027-01-15".into()),
                delta: Some(0.40),
            }],
            class: OverlayClass::CoveredCall,
            coverage_ratio: Some(1.0),
            net_delta: Some(-40.0),
            delta_source_consulted: true,
            gaps: vec![],
        });
        let n = engine::NarrativeRead {
            form: engine::NarrativeForm::RevisionBased,
            expansion: 0.36,
            reality: 0.10,
            ratio: Some(3.6),
            classification: engine::NarrativeClass::Hype,
            elapsed_days: 30,
            matched_rule: Some("narrative-vs-reality hype: test rule".into()),
        };
        let p = interp(&d, Some(&n));
        assert!(
            p.contains("SHORT INTEREST") && p.contains("+25.0% vs the prior settlement"),
            "{p}"
        );
        assert!(
            p.contains("SAME-UNDERLYING OPTION OVERLAY") && p.contains("covered call"),
            "{p}"
        );
        assert!(
            p.contains("NARRATIVE VS REALITY")
                && p.contains("HYPE")
                && p.contains("binds the ENGINE arm"),
            "{p}"
        );
        assert!(p.contains("IMPLIED EXPECTATIONS"), "{p}");
        // The action call sees the overlay too — it changes what the right
        // action is (`docs/portfolio-analysis.md` §The per-holding pipeline).
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        let engine_set =
            engine::feasible_actions(engine_output.grade, &engine_output.hurdle, None, false);
        let action = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced {
                graded,
                engine: &engine_output,
                pre_profit: None,
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(action.contains("SAME-UNDERLYING OPTION OVERLAY"), "{action}");
        assert!(!action.contains("SHORT INTEREST"), "positioning stays interpretation-side: {action}");
    }

    #[test]
    fn action_prompt_labels_the_prior_action_as_a_continuity_baseline() {
        // The prior action anchors the fresh call as a plain continuity baseline.
        // The retired whole-book-era history label was removed with the pre-v9
        // legacy (fresh-start ruling 2026-08-17).
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        d.prior_verdict = Some(prior);
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let engine_set =
            engine::feasible_actions(engine_output.grade, &engine_output.hurdle, None, false);
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        let graded = graded.clone();
        let user = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced {
                graded: &graded,
                engine: &engine_output,
                pre_profit: None,
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(user.contains("continuity baseline"), "{user}");
        assert!(!user.contains("RETIRED whole-book contract"), "{user}");
    }

    #[test]
    fn action_call_outside_engine_set_records_an_audit_annotation() {
        // A rogue stub choosing outside the engine set: the choice persists as
        // authored; the departure is app-stamped on the audit (annotate, never
        // bar — the two-arm contract).
        struct RogueActionStub;
        impl HoldingAnalyst for RogueActionStub {
            fn distill(&self, d: &HoldingDossier, f: &ResearchFindings) -> Result<String> {
                StubAnalyst.distill(d, f)
            }
            fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
                StubAnalyst.interpret(input)
            }
            fn interpret_role_risk(
                &self,
                input: &RoleRiskInput,
            ) -> Result<RoleRiskInterpretation> {
                StubAnalyst.interpret_role_risk(input)
            }
            fn decide_action(
                &self,
                _input: &ActionInput,
            ) -> Result<crate::portfolio::ActionDecision> {
                Ok(crate::portfolio::ActionDecision {
                    action: Action::AddAggressively,
                    rationale: "rogue: aggressive regardless of the engine set".to_string(),
                })
            }
            fn fast_id(&self) -> String {
                "rogue-stub".to_string()
            }
            fn reasoner_id(&self) -> String {
                "rogue-stub".to_string()
            }
        }
        // A weak book: the C-ish fixture's engine set won't offer add-aggressively
        // (A/B only), so the rogue choice lands outside it.
        let d = dossier(AssetClass::Stock, strong_financials());
        let (v, audit) =
            analyze_holding(&RogueActionStub, &d, &rates(), "2026-08-03").unwrap();
        let crate::portfolio::VerdictDisposition::Priced(g) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        assert_eq!(g.action, Action::AddAggressively, "persists as authored");
        assert_eq!(g.action_rationale, "rogue: aggressive regardless of the engine set");
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let set = engine::feasible_actions(engine_output.grade, &engine_output.hurdle, None, false);
        if set.contains(&Action::AddAggressively) {
            assert!(audit.action_annotations.is_empty(), "{:?}", audit.action_annotations);
        } else {
            assert_eq!(audit.action_annotations.len(), 1, "{:?}", audit.action_annotations);
            assert!(audit.action_annotations[0].contains("outside the engine set"));
        }
    }

    #[test]
    fn retrospective_renders_both_prior_arms_and_the_realized_since() {
        // The v7 retrospective (the deliberate reversal of the v4 anchoring
        // guard): a prior priced verdict's engine + model arms render with the
        // price-since read and the matured scoreboard lines.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-07-29").unwrap();
        d.prior_verdict = Some(prior);
        d.prior_vintage = Some("2026-07-29T12:00:00Z".into());
        d.prior_spot = Some(180.0);
        d.prior_matured_notes = vec!["1-month window scored: total return +4.2%".into()];
        // The prior vintage's anchor-session close (same basis, no split): the
        // bridge's realized leg. Without a bar inside the proximity bound the
        // comparison would be excluded, so the fixture carries one.
        d.financials.daily_closes.push(DatedValue {
            date: "2026-07-29".into(),
            value: 180.0,
        });

        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(user.contains("RETROSPECTIVE (prior read 2026-07-29T12:00:00Z)"), "{user}");
        assert!(user.contains("prior ENGINE arm: grade"), "{user}");
        assert!(user.contains("prior MODEL arm (yours): letter"), "{user}");
        // The realized move computes off the prior vintage's anchor-session
        // close (the split-safe bridge — Codex round 2, finding 1), never
        // against a target; here the anchor bar equals the authoring spot
        // (180 → 195 = +8.3%). The target reads are labeled as distances,
        // not returns (Codex round 1, finding 2).
        assert!(
            user.contains(
                "+8.3% realized since the prior read (anchor close 180.00; \
                 authoring spot 180.00 on its own basis)"
            ),
            "{user}"
        );
        assert!(
            user.contains("distance to the prior engine 12-mo base"),
            "{user}"
        );
        assert!(
            user.contains("distance to the prior model 12-mo base"),
            "{user}"
        );
        assert!(user.contains("any vintage"), "{user}");
        assert!(user.contains("1-month window scored: total return +4.2%"), "{user}");
        assert!(user.contains("Write self_assessment against this"), "{user}");

        // A debut renders no retrospective and says so in the model-arm brief.
        let debut = dossier(AssetClass::Stock, strong_financials());
        let debut_user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &debut,
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(!debut_user.contains("RETROSPECTIVE"), "{debut_user}");
        assert!(debut_user.contains("new holding (no prior verdict)"), "{debut_user}");
    }

    #[test]
    fn retrospective_bridge_keys_the_prior_vintage_to_its_et_session() {
        // An evening-ET prior read: 2026-07-30 01:30 UTC = 2026-07-29 21:30 EDT
        // — the vintage belongs to the ET session of the 29th. The bridge must
        // key that session's close (180), not the UTC-dated 30th's (250, a
        // session traded entirely after the prior read).
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-07-29").unwrap();
        d.prior_verdict = Some(prior);
        d.prior_vintage = Some("2026-07-30T01:30:00Z".into());
        d.prior_spot = Some(180.0);
        d.financials.daily_closes.push(DatedValue {
            date: "2026-07-29".into(),
            value: 180.0,
        });
        d.financials.daily_closes.push(DatedValue {
            date: "2026-07-30".into(),
            value: 250.0,
        });

        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(user.contains("anchor close 180.00"), "{user}");
    }

    #[test]
    fn retrospective_realized_move_is_split_safe_via_the_anchor_close_bridge() {
        // A 2:1 split between reads: the prior read authored at 180.00; the same
        // economic level trades near 90 today. A raw prior-spot ratio would
        // report ~−46% "realized" (Codex round 2, finding 1); the anchor-close
        // bridge keys both legs to today's basis — the true +8.3% renders, and
        // the prior targets cross through `target × anchor ⁄ authoring spot`.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-07-29").unwrap();
        d.prior_verdict = Some(prior);
        d.prior_vintage = Some("2026-07-29T12:00:00Z".into());
        d.prior_spot = Some(180.0); // pre-split basis
        d.financials.current_price = Some(97.5); // post-split basis
        d.financials.daily_closes.push(DatedValue {
            date: "2026-07-29".into(),
            value: 90.0, // the vintage session's close on today's basis
        });

        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            user.contains(
                "+8.3% realized since the prior read (anchor close 90.00; \
                 authoring spot 180.00 on its own basis)"
            ),
            "{user}"
        );
        // The raw cross-basis ratio (97.5 ⁄ 180 − 1 ≈ −45.8%) must be nowhere.
        assert!(!user.contains("-45.8"), "{user}");
        assert!(user.contains("(basis-bridged)"), "{user}");
    }

    #[test]
    fn retrospective_excludes_the_price_comparison_without_an_anchor_close() {
        // The fixture's dated closes end 2026-07-15 — outside the proximity
        // bound around the 2026-07-29 vintage — so the bridge has no anchor
        // session and every price comparison is excluded, never guessed (the
        // outcome slice's shared contract). The rest of the retrospective
        // still renders.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-07-29").unwrap();
        d.prior_verdict = Some(prior);
        d.prior_vintage = Some("2026-07-29T12:00:00Z".into());
        d.prior_spot = Some(180.0);

        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(user.contains("RETROSPECTIVE (prior read"), "{user}");
        assert!(
            user.contains("prior-read price comparison unavailable"),
            "{user}"
        );
        assert!(!user.contains("% realized"), "{user}");
        assert!(!user.contains("distance to the prior engine"), "{user}");
    }

    #[test]
    fn target_provenance_renders_the_anchored_and_carry_branches() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let mut engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };

        engine_output.target_meta.rate_anchored = true;
        engine_output.target_meta.anchor_observations = 40;
        engine_output.target_meta.current_multiple_carry = false;
        let anchored = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(anchored.contains("spread-anchored on 40 rate observations"), "{anchored}");
        // The weighing sentence's signal grammar (two Codex rounds): flat_driver is
        // hardcoded true on every priced fund and must not read as low-signal; the
        // low-signal shape is the carry + flat/clamped driver combination; and the
        // dispersion floor's evidence is conditional — it inherits the base's
        // provenance (robust over an anchored base, still weak over a carried one,
        // where the floor merely manufactures width around spot).
        assert!(anchored.contains("even with a flat driver"), "{anchored}");
        assert!(
            anchored.contains("carry with a flat or clamp-flattened driver"),
            "{anchored}"
        );
        assert!(
            anchored.contains("inherits its base's signal quality"),
            "{anchored}"
        );
        assert!(anchored.contains("discount the band's width, not its level"), "{anchored}");
        assert!(anchored.contains("stays weak exit evidence"), "{anchored}");

        engine_output.target_meta.rate_anchored = false;
        engine_output.target_meta.current_multiple_carry = true;
        engine_output.target_meta.flat_driver = true;
        engine_output.target_meta.dispersion_floor_applied = true;
        let carried = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(carried.contains("current multiple was carried"), "{carried}");
        assert!(carried.contains("driver held FLAT"), "{carried}");
        assert!(carried.contains("volatility dispersion floor"), "{carried}");

        // Neither anchored nor carried: the raw-percentile fallback branch.
        engine_output.target_meta.current_multiple_carry = false;
        let fallback = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(fallback.contains("raw-percentile fallback"), "{fallback}");
    }

    #[test]
    fn continuity_notes_a_band_recalibration_only_on_version_mismatch() {
        let prior = HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated {
                reason: "fixture".into(),
            },
            thesis_ledger: None,
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        };
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let prompt = |d: &HoldingDossier| {
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: d,
                engine: &engine_output,
                distilled: "",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            })
        };

        // No prior verdict: new holding, no recalibration note.
        assert!(!prompt(&d).contains("recalibrated"), "no prior verdict");

        // Prior verdict from a pre-stamp run (None = the v1 bands): the note fires —
        // the exact shape of the first post-tune run over run 3b21ae85's book.
        d.prior_verdict = Some(prior);
        d.prior_grade_parameter_version = None;
        let p = prompt(&d);
        assert!(p.contains("recalibrated"), "{p}");
        assert!(p.contains("what_changed"), "{p}");

        // Prior verdict stamped with the current bands: no note.
        d.prior_grade_parameter_version = Some(engine::GRADE_PARAMETER_VERSION.to_string());
        assert!(!prompt(&d).contains("recalibrated"), "same-version prior");
    }

    #[test]
    fn house_view_blocks_carry_the_scope_line_in_both_prompts() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.house_view.latest_sections = Some("Thesis: risk-off.".into());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        // The scope rides the house-view block header itself, not a floating line.
        let hv_block = user
            .split("MARKET SIGNAL HOUSE VIEW")
            .nth(1)
            .expect("house-view block present");
        assert!(
            hv_block.starts_with(" (latest report — scope:"),
            "scope on the header: {hv_block}"
        );
        assert!(user.contains("never by itself a reason to exit"), "{user}");

        let readout = RoleRiskReadout {
            class_label: "equity fund below the US-exposure guard".into(),
            structural_flag: false,
            exposure_tilt: vec![],
            expense_ratio: None,
            observable_risk: None,
            evidence_gaps: vec![],
        };
        let role = role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: &d,
            readout: &readout,
            ledger_eval: None,
        });
        assert!(role.contains("never by itself a reason to exit"), "{role}");
    }

    #[test]
    fn cot_positioning_renders_as_of_in_the_fund_prompts() {
        // A commodity / macro fund's mapped COT row renders as dated positioning
        // context in the role-risk prompt (where commodity funds land); an
        // unmapped fund renders no section.
        let mut d = fund_dossier(us_equity_fund());
        if let Some(f) = d.fund.as_mut() {
            f.positioning = Some(crate::data_sources::CotPositioning {
                contract: "Gold".into(),
                contract_code: "088691".into(),
                asset_class: "commodity".into(),
                report_date: "2026-08-11".into(),
                open_interest: 500_000.0,
                spec_net: 200_000.0,
                spec_net_weekly_change: Some(4_000.0),
                spec_pct_oi_long: Some(50.0),
                real_money_net: None,
                real_money_net_weekly_change: None,
            });
        }
        let readout = RoleRiskReadout {
            class_label: "commodity fund".into(),
            ..Default::default()
        };
        let role = role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: &d,
            readout: &readout,
            ledger_eval: None,
        });
        assert!(role.contains("UNDERLYING POSITIONING"), "{role}");
        assert!(role.contains("snapshot as of 2026-08-11"), "{role}");
        assert!(role.contains("Gold — speculator net +200000"), "{role}");
        assert!(role.contains("never a score input"), "{role}");

        let bare = fund_dossier(us_equity_fund());
        let role = role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: &bare,
            readout: &readout,
            ledger_eval: None,
        });
        assert!(!role.contains("UNDERLYING POSITIONING"), "{role}");
    }

    #[test]
    fn stage_requests_carry_the_per_stage_mode_options_and_residency() {
        // The options-wiring contract (`docs/local-model-operations.md`): distill is
        // explicitly non-thinking (F3 — an omitted flag rides the thinking-on
        // default), interpretation thinks; every stage pins an explicit `num_ctx`
        // (never the daemon auto-size), its mode's sampling row, and stay-resident
        // `keep_alive`.
        let d = dossier(AssetClass::Stock, strong_financials());
        let findings = research(&d);

        let distill = distill_request("fast-model", NUM_CTX_DISTILL, &d, &findings);
        assert_eq!(distill.think, Some(false));
        assert_eq!(distill.keep_alive, Some(-1));
        let opts = distill.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_DISTILL);
        assert_eq!(opts["num_predict"], NUM_PREDICT_DISTILL, "output reservation");
        assert_eq!(opts["temperature"], 0.7, "non-thinking-general row");
        assert!(distill.format_schema.is_none(), "distill is free prose");

        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interpret = interpret_request(
            "reasoner-model",
            &InterpretationInput {
                input_delta: &[],
                dossier: &d,
                engine: &engine_output,
                distilled: "distilled findings",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            },
        );
        assert_eq!(interpret.think, Some(true));
        assert_eq!(interpret.keep_alive, Some(-1));
        let opts = interpret.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET);
        assert_eq!(opts["num_predict"], NUM_PREDICT_THINKING, "output reservation");
        assert_eq!(opts["temperature"], 1.0, "thinking-general row");
        assert!(interpret.format_schema.is_some(), "grammar-constrained");

        let readout = RoleRiskReadout {
            class_label: "commodity fund".into(),
            exposure_tilt: vec![("gold".into(), 1.0)],
            expense_ratio: Some(0.4),
            observable_risk: None,
            structural_flag: false,
            evidence_gaps: vec![],
        };
        let role_risk = role_risk_request(
            "reasoner-model",
            &RoleRiskInput {
                input_delta: &[],
                dossier: &d,
                readout: &readout,
                ledger_eval: None,
            },
        );
        assert_eq!(role_risk.think, Some(true));
        assert_eq!(role_risk.keep_alive, Some(-1));
        let opts = role_risk.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET);
        assert_eq!(opts["num_predict"], NUM_PREDICT_THINKING, "output reservation");
        assert!(role_risk.format_schema.is_some(), "grammar-constrained");
    }

    /// The output-budget guard: a `done_reason: "length"` response fails typed —
    /// naming the stage, the reservation, and the generated count — instead of
    /// surfacing as an opaque schema parse failure downstream.
    #[test]
    fn a_length_stop_fails_the_stage_with_a_typed_truncation_error() {
        let mut req = ChatRequest::new("m", vec![ChatMessage::user("x")]);
        req.options = Some(options::thinking_general(131_072, 65_536));
        let truncated = crate::local_model::ChatResponse {
            content: "{\"partial\":".into(),
            thinking: None,
            prompt_eval_count: Some(100_000),
            eval_count: Some(65_536),
            done_reason: Some("length".into()),
        };
        let err = ensure_not_output_limited("construction", &req, &truncated).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("construction"), "{msg}");
        assert!(msg.contains("truncated at the output reservation"), "{msg}");
        assert!(msg.contains("65536"), "{msg}");

        // A length stop well UNDER the reservation is the other cause — the
        // shared context filled first — and must not be blamed on the
        // reservation (the two have different levers).
        let context_stopped = crate::local_model::ChatResponse {
            content: "{\"partial\":".into(),
            thinking: None,
            prompt_eval_count: Some(120_000),
            eval_count: Some(11_000),
            done_reason: Some("length".into()),
        };
        let err = ensure_not_output_limited("construction", &req, &context_stopped).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("context exhaustion suspected"), "{msg}");
        assert!(!msg.contains("truncated at the output reservation"), "{msg}");

        // A daemon that omits `eval_count` leaves the stop unattributable: the
        // error must fail typed without naming either lever on a guess.
        let uncounted = crate::local_model::ChatResponse {
            content: "{\"partial\":".into(),
            thinking: None,
            prompt_eval_count: None,
            eval_count: None,
            done_reason: Some("length".into()),
        };
        let err = ensure_not_output_limited("construction", &req, &uncounted).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cannot be told apart"), "{msg}");
        assert!(!msg.contains("context exhaustion suspected"), "{msg}");
        assert!(!msg.contains("truncated at the output reservation"), "{msg}");

        let complete = crate::local_model::ChatResponse {
            content: "{}".into(),
            thinking: None,
            prompt_eval_count: Some(100_000),
            eval_count: Some(9_000),
            done_reason: Some("stop".into()),
        };
        assert!(ensure_not_output_limited("construction", &req, &complete).is_ok());
    }

    #[test]
    fn blank_fast_tier_never_bounces_the_reasoner_context() {
        // The same-model context rule: an Ollama `num_ctx` change reloads the
        // resident runner even under `keep_alive: -1`, so when distillation and
        // interpretation share one model the distill call must ride the
        // interpretation context — alternating 32 K / 128 K would bounce the 81 GB
        // load at every stage transition (external review finding, 2026-08-01).
        assert_eq!(
            distill_num_ctx("qwen3.5:122b", "qwen3.5:122b"),
            NUM_CTX_INTERPRET
        );
        // A genuinely distinct fast model keeps the smaller distill context.
        assert_eq!(
            distill_num_ctx("qwen3.5:35b", "qwen3.5:122b"),
            NUM_CTX_DISTILL
        );
        // The documented default roster (blank fast tier) resolves to the same-model
        // path at construction, so the rule engages for the default configuration.
        let analyst = LocalAnalyst::new(
            LocalModelClient::new("http://localhost:11434").unwrap(),
            "qwen3.5:122b".into(),
            "   ".into(),
        );
        assert_eq!(analyst.fast_model, analyst.reasoner_model);
        assert_eq!(
            distill_num_ctx(&analyst.fast_model, &analyst.reasoner_model),
            NUM_CTX_INTERPRET
        );
    }

    #[test]
    fn blank_fast_tier_falls_back_to_the_reasoner() {
        // The fast tier is optional and never gates (`docs/configuration.md`), so a
        // blank slot must not reach the daemon as an empty model id — distillation
        // runs on the reasoner instead, so the id the audit records for the distill
        // call is the reasoner's (`analyze_holding` dedups the two into one entry).
        let client = LocalModelClient::new("http://127.0.0.1:1").unwrap();
        let analyst = LocalAnalyst::new(client, "qwen3.5:122b".into(), "  ".into());
        assert_eq!(analyst.fast_id(), "qwen3.5:122b");
        assert_eq!(analyst.reasoner_id(), "qwen3.5:122b");

        // A configured fast tier is used as-is.
        let client = LocalModelClient::new("http://127.0.0.1:1").unwrap();
        let analyst = LocalAnalyst::new(client, "r".into(), "f".into());
        assert_eq!(analyst.reasoner_id(), "r");
        assert_eq!(analyst.fast_id(), "f");
    }

    // ---- Thesis-ledger validation (the 6g seam) + prompt rendering -------------

    use crate::portfolio::{ConditionCrossing, ConditionEvalState};

    /// A prior priced ledger with one quantitative falsifier ("keep-1", carrying a
    /// live first-breach streak), one quantitative trim trigger ("trig-1"), and one
    /// qualitative falsifier ("qual-1").
    fn prior_with_conditions() -> ThesisLedger {
        ThesisLedger {
            branch: LedgerBranch::Priced,
            original_thesis: "the debut thesis".into(),
            current_thesis: "the standing thesis".into(),
            key_drivers: vec![KeyDriver {
                name: "margins".into(),
                series: Some(engine::LedgerSeries::NetMargin),
            }],
            monitor: vec![
                MonitorScenario {
                    scenario: ScenarioKind::Bear,
                    conditions: "bear case".into(),
                    probability_pct: 25.0,
                    engine_target: Some(150.0),
                },
                MonitorScenario {
                    scenario: ScenarioKind::Base,
                    conditions: "base case".into(),
                    probability_pct: 50.0,
                    engine_target: Some(210.0),
                },
                MonitorScenario {
                    scenario: ScenarioKind::Bull,
                    conditions: "bull case".into(),
                    probability_pct: 25.0,
                    engine_target: Some(240.0),
                },
            ],
            what_must_improve: "growth".into(),
            what_must_not_break: "margins".into(),
            conditions: vec![
                LedgerCondition {
                    condition_id: "keep-1".into(),
                    role: ConditionRole::Falsifier,
                    trigger_family: None,
                    statement: "Trailing return collapses".into(),
                    quant: Some(QuantCore {
                        series: engine::LedgerSeries::TrailingReturn,
                        comparator: LedgerComparator::Below,
                        threshold: -0.40,
                        margin: 0.02,
                    }),
                    downgraded_reason: None,
                    technology_class: false,
                    tripped: false,
                    supersedes: None,
                    eval_state: Some(ConditionEvalState {
                        last_observation_id: Some("2026-07-15".into()),
                        breach_streak: 1,
                        first_breach_at: Some("2026-08-01".into()),
                        ..Default::default()
                    }),
                },
                LedgerCondition {
                    condition_id: "trig-1".into(),
                    role: ConditionRole::Trigger,
                    trigger_family: Some(TriggerFamily::Trim),
                    statement: "Trim after a 25% trailing run-up".into(),
                    quant: Some(QuantCore {
                        series: engine::LedgerSeries::TrailingReturn,
                        comparator: LedgerComparator::Above,
                        threshold: 0.25,
                        margin: 0.0,
                    }),
                    downgraded_reason: None,
                    technology_class: false,
                    tripped: false,
                    supersedes: None,
                    eval_state: Some(ConditionEvalState::default()),
                },
                LedgerCondition {
                    condition_id: "qual-1".into(),
                    role: ConditionRole::Falsifier,
                    trigger_family: None,
                    statement: "A credible competitor ships at scale".into(),
                    quant: None,
                    downgraded_reason: None,
                    technology_class: true,
                    tripped: false,
                    supersedes: None,
                    eval_state: None,
                },
            ],
            authored_band_relation: None,
        }
    }

    #[test]
    fn debut_rewrite_freezes_the_original_thesis_and_stamps_engine_targets() {
        let draft = stub_ledger_draft(None, "AAPL", false);
        let targets = PriceTarget {
            base: 210.0,
            bear: 180.0,
            bull: 240.0,
            methodology: "m".into(),
        };
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, false, Some(&targets), None);
        assert_eq!(ledger.branch, LedgerBranch::Priced);
        assert_eq!(ledger.original_thesis, ledger.current_thesis, "frozen at debut");
        assert_eq!(ledger.conditions.len(), 2);
        for c in &ledger.conditions {
            assert!(!c.condition_id.is_empty());
            if c.quant.is_some() {
                assert!(c.eval_state.is_some(), "quant conditions start machine state");
            }
        }
        // The engine's own scenario targets stamped into the monitor — never a
        // model-written number.
        let target_of = |k: ScenarioKind| {
            ledger
                .monitor
                .iter()
                .find(|m| m.scenario == k)
                .unwrap()
                .engine_target
        };
        assert_eq!(target_of(ScenarioKind::Bear), Some(180.0));
        assert_eq!(target_of(ScenarioKind::Base), Some(210.0));
        assert_eq!(target_of(ScenarioKind::Bull), Some(240.0));
        assert!(audit.downgraded.is_empty());
        assert!(audit.rejected_claims.is_empty());
        // No spot passed → no authoring-time band relation stamped.
        assert!(ledger.authored_band_relation.is_none());
    }

    #[test]
    fn rewrite_stamps_spots_authoring_time_band_relation() {
        let draft = stub_ledger_draft(None, "AAPL", false);
        let targets = PriceTarget {
            base: 210.0,
            bear: 180.0,
            bull: 240.0,
            methodology: "m".into(),
        };
        let relation_at = |spot: f64| {
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, false, Some(&targets), Some(spot))
                .0
                .authored_band_relation
        };
        use crate::portfolio::BandRelation;
        assert_eq!(relation_at(200.0), Some(BandRelation::Inside));
        assert_eq!(relation_at(150.0), Some(BandRelation::BelowBand));
        assert_eq!(relation_at(300.0), Some(BandRelation::AboveBand));
    }

    #[test]
    fn unresolvable_series_downgrades_to_qualitative_logged_never_dropped() {
        let mut draft = stub_ledger_draft(None, "AAPL", false);
        draft.falsifiers[0].quant = Some(QuantCoreDraft {
            series: "made-up-series".into(),
            comparator: "below".into(),
            threshold: 1.0,
            margin: 0.0,
        });
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, false, None, None);
        let f = ledger
            .conditions
            .iter()
            .find(|c| c.role == ConditionRole::Falsifier)
            .unwrap();
        assert!(f.quant.is_none(), "downgraded to qualitative");
        assert!(f.downgraded_reason.is_some());
        assert!(f.eval_state.is_none(), "a downgraded condition carries no machine state");
        assert_eq!(audit.downgraded.len(), 1);
        assert_eq!(
            ledger.conditions.len(),
            2,
            "downgraded, never dropped: {:?}",
            ledger.conditions
        );
    }

    #[test]
    fn unchanged_core_carries_id_and_state_through_rewording() {
        let prior = prior_with_conditions();
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        // Re-word the quantitative falsifier; the machine core is untouched.
        draft.falsifiers[0].statement = "The price collapses more than 40% (reworded)".into();
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
        let f = ledger
            .conditions
            .iter()
            .find(|c| c.role == ConditionRole::Falsifier && c.quant.is_some())
            .unwrap();
        assert_eq!(f.condition_id, "keep-1", "unchanged core carries the id");
        assert_eq!(
            f.eval_state.as_ref().unwrap().breach_streak,
            1,
            "accumulated state carries through the re-wording"
        );
        assert!(audit.superseded.is_empty());
        // The qualitative condition carried by unchanged statement.
        let q = ledger.conditions.iter().find(|c| c.quant.is_none()).unwrap();
        assert_eq!(q.condition_id, "qual-1");
    }

    #[test]
    fn changed_core_supersedes_with_a_fresh_streak() {
        let prior = prior_with_conditions();
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        // Edit the falsifier's threshold: same series + role, changed core.
        draft.falsifiers[0].quant.as_mut().unwrap().threshold = -0.50;
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
        let f = ledger
            .conditions
            .iter()
            .find(|c| c.role == ConditionRole::Falsifier && c.quant.is_some())
            .unwrap();
        assert_ne!(f.condition_id, "keep-1", "a threshold edit cannot inherit the id");
        assert_eq!(f.supersedes.as_deref(), Some("keep-1"));
        assert_eq!(
            f.eval_state.as_ref().unwrap().breach_streak,
            0,
            "the successor starts a fresh streak"
        );
        assert_eq!(audit.superseded.len(), 1);
        let closed = &audit.superseded[0];
        assert_eq!(closed.condition.condition_id, "keep-1");
        assert_eq!(closed.superseded_by.as_deref(), Some(f.condition_id.as_str()));
        // The shared contract: the old condition closes WITH its accumulated
        // state into the audit record — reconstructible after run pruning.
        assert_eq!(
            closed.condition.eval_state.as_ref().unwrap().breach_streak,
            1,
            "{:?}",
            closed.condition.eval_state
        );
    }

    #[test]
    fn carry_is_order_independent_across_same_series_siblings() {
        // Prior holds two same-series falsifiers; the draft emits a CHANGED
        // version of keep-2 FIRST and the unchanged keep-1 second. The changed
        // condition must supersede keep-2 — never consume the unchanged sibling
        // keep-1 that a later draft condition still carries.
        let mut prior = prior_with_conditions();
        prior.conditions.push(LedgerCondition {
            condition_id: "keep-2".into(),
            role: ConditionRole::Falsifier,
            trigger_family: None,
            statement: "Trailing return collapses harder".into(),
            quant: Some(QuantCore {
                series: engine::LedgerSeries::TrailingReturn,
                comparator: LedgerComparator::Below,
                threshold: -0.60,
                margin: 0.02,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: Some(ConditionEvalState::default()),
        });
        let core_draft = |threshold: f64| QuantCoreDraft {
            series: "trailing-return".into(),
            comparator: "below".into(),
            threshold,
            margin: 0.02,
        };
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        draft.falsifiers = vec![
            FalsifierDraft {
                statement: "Collapses even harder".into(),
                quant: Some(core_draft(-0.65)), // keep-2's core, edited
                technology_class: false,
                tripped: false,
            },
            FalsifierDraft {
                statement: "Trailing return collapses".into(),
                quant: Some(core_draft(-0.40)), // keep-1's core, unchanged
                technology_class: false,
                tripped: false,
            },
        ];
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
        let unchanged = ledger
            .conditions
            .iter()
            .find(|c| c.quant.as_ref().map(|q| q.threshold) == Some(-0.40))
            .unwrap();
        assert_eq!(unchanged.condition_id, "keep-1", "the sibling's carry survives");
        assert_eq!(unchanged.eval_state.as_ref().unwrap().breach_streak, 1);
        let changed = ledger
            .conditions
            .iter()
            .find(|c| c.quant.as_ref().map(|q| q.threshold) == Some(-0.65))
            .unwrap();
        assert_eq!(changed.supersedes.as_deref(), Some("keep-2"));
        assert_eq!(audit.superseded.len(), 1);
        assert_eq!(audit.superseded[0].condition.condition_id, "keep-2");
    }

    #[test]
    fn two_edited_siblings_each_supersede_their_nearest_ancestor_in_either_order() {
        // BOTH same-series siblings edited (nothing reserved): each draft must
        // link to its nearest prior core — never to whichever sat first in the
        // pool — and the pairing must not depend on draft order (Codex round 2,
        // finding 2). Priors: -0.40 (keep-1) and -0.60 (keep-2); drafts: -0.65
        // (nearest -0.60) and -0.45 (nearest -0.40).
        let mut prior = prior_with_conditions();
        prior.conditions.push(LedgerCondition {
            condition_id: "keep-2".into(),
            role: ConditionRole::Falsifier,
            trigger_family: None,
            statement: "Trailing return collapses harder".into(),
            quant: Some(QuantCore {
                series: engine::LedgerSeries::TrailingReturn,
                comparator: LedgerComparator::Below,
                threshold: -0.60,
                margin: 0.02,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: Some(ConditionEvalState::default()),
        });
        let falsifier = |threshold: f64| FalsifierDraft {
            statement: format!("Edited at {threshold}"),
            quant: Some(QuantCoreDraft {
                series: "trailing-return".into(),
                comparator: "below".into(),
                threshold,
                margin: 0.02,
            }),
            technology_class: false,
            tripped: false,
        };
        for order in [[-0.65, -0.45], [-0.45, -0.65]] {
            let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
            draft.falsifiers = order.iter().map(|t| falsifier(*t)).collect();
            let (ledger, audit) =
                validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
            let ancestor_of = |threshold: f64| {
                ledger
                    .conditions
                    .iter()
                    .find(|c| c.quant.as_ref().map(|q| q.threshold) == Some(threshold))
                    .unwrap()
                    .supersedes
                    .clone()
            };
            assert_eq!(
                ancestor_of(-0.65).as_deref(),
                Some("keep-2"),
                "draft order {order:?}"
            );
            assert_eq!(
                ancestor_of(-0.45).as_deref(),
                Some("keep-1"),
                "draft order {order:?}"
            );
            assert_eq!(audit.superseded.len(), 2, "draft order {order:?}");
        }
    }

    /// A bare priced prior ledger holding only the given trailing-return
    /// falsifier cores (id, comparator, threshold, margin).
    fn prior_of_cores(cores: &[(&str, LedgerComparator, f64, f64)]) -> ThesisLedger {
        let mut prior = prior_with_conditions();
        prior.conditions = cores
            .iter()
            .map(|(id, comparator, threshold, margin)| LedgerCondition {
                condition_id: (*id).into(),
                role: ConditionRole::Falsifier,
                trigger_family: None,
                statement: format!("prior {id}"),
                quant: Some(QuantCore {
                    series: engine::LedgerSeries::TrailingReturn,
                    comparator: *comparator,
                    threshold: *threshold,
                    margin: *margin,
                }),
                downgraded_reason: None,
                technology_class: false,
                tripped: false,
                supersedes: None,
                eval_state: Some(ConditionEvalState::default()),
            })
            .collect();
        prior
    }

    #[test]
    fn shared_nearest_ancestor_resolves_globally_in_either_order() {
        // Codex round 3: both drafts (-0.41, -0.42) are locally nearest to the
        // SAME prior (-0.40); greedy matching flips both links with draft order.
        // The global assignment must give -0.41 → -0.40 and -0.42 → -0.60 (the
        // minimum total distance) regardless of emission order.
        let prior = prior_of_cores(&[
            ("keep-1", LedgerComparator::Below, -0.40, 0.02),
            ("keep-2", LedgerComparator::Below, -0.60, 0.02),
        ]);
        let falsifier = |threshold: f64| FalsifierDraft {
            statement: format!("Edited at {threshold}"),
            quant: Some(QuantCoreDraft {
                series: "trailing-return".into(),
                comparator: "below".into(),
                threshold,
                margin: 0.02,
            }),
            technology_class: false,
            tripped: false,
        };
        for order in [[-0.41, -0.42], [-0.42, -0.41]] {
            let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
            draft.falsifiers = order.iter().map(|t| falsifier(*t)).collect();
            let (ledger, _) =
                validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
            let ancestor_of = |threshold: f64| {
                ledger
                    .conditions
                    .iter()
                    .find(|c| c.quant.as_ref().map(|q| q.threshold) == Some(threshold))
                    .unwrap()
                    .supersedes
                    .clone()
            };
            assert_eq!(
                ancestor_of(-0.41).as_deref(),
                Some("keep-1"),
                "draft order {order:?}"
            );
            assert_eq!(
                ancestor_of(-0.42).as_deref(),
                Some("keep-2"),
                "draft order {order:?}"
            );
        }
    }

    #[test]
    fn equal_core_trigger_families_never_exchange_identity_on_reorder() {
        // Codex round 4: trim and sell triggers on ONE machine core are distinct
        // pre-commitments (the dedup contract) — reordering them must never swap
        // their stable ids, streaks, or acknowledgments.
        let price_core = QuantCore {
            series: engine::LedgerSeries::Price,
            comparator: LedgerComparator::Above,
            threshold: 150.0,
            margin: 0.0,
        };
        let mut prior = prior_with_conditions();
        prior.conditions = vec![
            LedgerCondition {
                condition_id: "trim-1".into(),
                role: ConditionRole::Trigger,
                trigger_family: Some(TriggerFamily::Trim),
                statement: "Trim above the priced-in ceiling".into(),
                quant: Some(price_core.clone()),
                downgraded_reason: None,
                technology_class: false,
                tripped: false,
                supersedes: None,
                // Distinct marker state: a live first-breach streak.
                eval_state: Some(ConditionEvalState {
                    last_observation_id: Some("2026-07-15".into()),
                    breach_streak: 1,
                    first_breach_at: Some("2026-08-01".into()),
                    ..Default::default()
                }),
            },
            LedgerCondition {
                condition_id: "sell-1".into(),
                role: ConditionRole::Trigger,
                trigger_family: Some(TriggerFamily::Sell),
                statement: "Exit fully above the priced-in ceiling".into(),
                quant: Some(price_core.clone()),
                downgraded_reason: None,
                technology_class: false,
                tripped: false,
                supersedes: None,
                // Distinct marker state: an acknowledged prior confirmation.
                eval_state: Some(ConditionEvalState {
                    last_observation_id: Some("2026-07-10".into()),
                    acknowledged_observation_id: Some("2026-07-10".into()),
                    ..Default::default()
                }),
            },
        ];
        let trigger = |family: &str| TriggerDraft {
            statement: format!("{family} above the priced-in ceiling"),
            family: family.into(),
            quant: Some(QuantCoreDraft {
                series: "price".into(),
                comparator: "above".into(),
                threshold: 150.0,
                margin: 0.0,
            }),
            fired: false,
        };
        for order in [["trim", "sell"], ["sell", "trim"]] {
            let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
            draft.triggers = order.iter().map(|f| trigger(f)).collect();
            let (ledger, audit) =
                validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
            let by_family = |family: TriggerFamily| {
                ledger
                    .conditions
                    .iter()
                    .find(|c| c.trigger_family == Some(family))
                    .unwrap()
            };
            let trim = by_family(TriggerFamily::Trim);
            assert_eq!(trim.condition_id, "trim-1", "draft order {order:?}");
            assert_eq!(
                trim.eval_state.as_ref().unwrap().breach_streak,
                1,
                "trim keeps its own streak, draft order {order:?}"
            );
            let sell = by_family(TriggerFamily::Sell);
            assert_eq!(sell.condition_id, "sell-1", "draft order {order:?}");
            assert_eq!(
                sell.eval_state
                    .as_ref()
                    .unwrap()
                    .acknowledged_observation_id
                    .as_deref(),
                Some("2026-07-10"),
                "sell keeps its own acknowledgment, draft order {order:?}"
            );
            assert!(audit.superseded.is_empty(), "draft order {order:?}");
            assert!(audit.closed.is_empty(), "draft order {order:?}");
        }
    }

    #[test]
    fn supersession_lineage_never_crosses_trigger_families() {
        // A changed trim core with only a SELL prior on the same series: no link
        // — the sell prior closes as removed, the trim condition starts fresh.
        let mut prior = prior_with_conditions();
        prior.conditions = vec![LedgerCondition {
            condition_id: "sell-1".into(),
            role: ConditionRole::Trigger,
            trigger_family: Some(TriggerFamily::Sell),
            statement: "Exit fully above the priced-in ceiling".into(),
            quant: Some(QuantCore {
                series: engine::LedgerSeries::Price,
                comparator: LedgerComparator::Above,
                threshold: 150.0,
                margin: 0.0,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: Some(ConditionEvalState::default()),
        }];
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        draft.triggers = vec![TriggerDraft {
            statement: "Trim above a higher ceiling".into(),
            family: "trim".into(),
            quant: Some(QuantCoreDraft {
                series: "price".into(),
                comparator: "above".into(),
                threshold: 180.0,
                margin: 0.0,
            }),
            fired: false,
        }];
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
        let trim = ledger
            .conditions
            .iter()
            .find(|c| c.trigger_family == Some(TriggerFamily::Trim))
            .unwrap();
        assert_eq!(trim.supersedes, None, "no cross-family lineage");
        assert!(audit.superseded.is_empty());
        assert!(
            audit
                .closed
                .iter()
                .any(|c| c.condition.condition_id == "sell-1"),
            "{:?}",
            audit.closed
        );
    }

    #[test]
    fn margin_participates_in_supersession_lineage() {
        // Two priors identical except for margin (margin is part of the machine
        // core); an edited draft must link to the margin-nearest ancestor, never
        // fall back to pool order.
        let prior = prior_of_cores(&[
            ("tight", LedgerComparator::Below, -0.40, 0.02),
            ("wide", LedgerComparator::Below, -0.40, 0.10),
        ]);
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        draft.falsifiers = vec![FalsifierDraft {
            statement: "Widened noise guard".into(),
            quant: Some(QuantCoreDraft {
                series: "trailing-return".into(),
                comparator: "below".into(),
                threshold: -0.40,
                margin: 0.09,
            }),
            technology_class: false,
            tripped: false,
        }];
        let (ledger, _) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
        let edited = ledger
            .conditions
            .iter()
            .find(|c| c.quant.as_ref().map(|q| q.margin) == Some(0.09))
            .unwrap();
        assert_eq!(edited.supersedes.as_deref(), Some("wide"));
    }

    #[test]
    fn removed_conditions_close_into_the_audit() {
        let prior = prior_with_conditions();
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        draft.triggers.clear();
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
        assert!(!ledger.conditions.iter().any(|c| c.condition_id == "trig-1"));
        assert!(
            audit
                .closed
                .iter()
                .any(|c| c.condition.condition_id == "trig-1" && c.superseded_by.is_none()),
            "{:?}",
            audit.closed
        );
    }

    #[test]
    fn tripped_claims_are_honored_only_against_a_confirmed_crossing() {
        let prior = prior_with_conditions();
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        draft.falsifiers[0].tripped = true; // quantitative (keep-1)
        draft.falsifiers[1].tripped = true; // qualitative (qual-1)

        // No engine crossing at all: both claims cleared and logged — the ledger
        // cannot be quietly rewritten to fit a new verdict.
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
        assert!(ledger.conditions.iter().all(|c| !c.tripped));
        assert_eq!(audit.rejected_claims.len(), 2, "{:?}", audit.rejected_claims);

        // A confirmed crossing on keep-1: the quantitative claim is honored, the
        // consumed crossing's observation stamped acknowledging; the qualitative
        // claim stays rejected (no source-backed finding exists).
        let eval = LedgerEvaluation {
            crossings: vec![ConditionCrossing {
                condition_id: "keep-1".into(),
                statement: "Trailing return collapses".into(),
                role: ConditionRole::Falsifier,
                outcome: CrossingOutcome::Confirmed,
                observed_value: -0.45,
                threshold: -0.40,
                observation_id: "2026-07-16".into(),
                // The engine stamped this on the confirming pass with the run's
                // ET session date (`run_date`).
                confirmed_at: Some("2026-07-16".into()),
            }],
            unevaluable: vec![],
            unevaluable_series: vec![],
            updated_states: vec![(
                "keep-1".into(),
                ConditionEvalState {
                    last_observation_id: Some("2026-07-16".into()),
                    breach_streak: 2,
                    confirmed_at: Some("2026-08-03".into()),
                    ..Default::default()
                },
            )],
        };
        let (ledger, audit) = validate_ledger_rewrite(
            &draft,
            Some(&prior),
            Some(&eval),
            LedgerBranch::Priced,
            false,
            None,
            None,
        );
        let f = ledger
            .conditions
            .iter()
            .find(|c| c.condition_id == "keep-1")
            .unwrap();
        assert!(f.tripped);
        assert_eq!(
            f.eval_state
                .as_ref()
                .unwrap()
                .acknowledged_observation_id
                .as_deref(),
            Some("2026-07-16"),
            "the consumed confirmation acknowledges its observation"
        );
        assert_eq!(audit.rejected_claims.len(), 1, "{:?}", audit.rejected_claims);
        assert_eq!(audit.crossings.len(), 1, "the consumed crossing rides the audit");
    }

    #[test]
    fn role_risk_reductions_bind_condition_only_monitor_and_no_add_trigger() {
        let mut draft = stub_ledger_draft(None, "BND", true);
        draft.triggers.push(TriggerDraft {
            statement: "Add on weakness".into(),
            family: "add".into(),
            quant: None,
            fired: false,
        });
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::RoleRiskOnly, false, None, None);
        assert_eq!(ledger.branch, LedgerBranch::RoleRiskOnly);
        assert!(
            ledger.monitor.iter().all(|m| m.engine_target.is_none()),
            "condition-only monitor on this branch"
        );
        assert!(
            !ledger
                .conditions
                .iter()
                .any(|c| c.trigger_family == Some(TriggerFamily::Add)),
            "no add trigger persists on the reduced spine"
        );
        assert!(
            audit.rejected_claims.iter().any(|r| r.contains("add trigger")),
            "{:?}",
            audit.rejected_claims
        );
    }

    #[test]
    fn role_risk_guard_strips_engine_targets_even_when_the_call_site_passes_them() {
        // The condition-only monitor is structural inside the validator, not a
        // call-site convention: a role_risk_only rewrite handed engine targets
        // still persists none.
        let draft = stub_ledger_draft(None, "BND", true);
        let targets = PriceTarget {
            base: 210.0,
            bear: 180.0,
            bull: 240.0,
            methodology: "m".into(),
        };
        let (ledger, _) = validate_ledger_rewrite(
            &draft,
            None,
            None,
            LedgerBranch::RoleRiskOnly,
            false,
            Some(&targets),
            Some(195.0),
        );
        assert!(ledger.monitor.iter().all(|m| m.engine_target.is_none()));
        // The branch guard strips the band relation with the targets: no band, no stamp.
        assert!(ledger.authored_band_relation.is_none());
    }

    #[test]
    fn duplicate_conditions_drop_with_a_logged_note_and_never_touch_the_pool() {
        // Prior holds TWO same-series falsifiers with different thresholds; the
        // draft repeats one condition twice. The duplicate must be dropped before
        // carry matching — otherwise it would wrongly supersede the sibling.
        let mut prior = prior_with_conditions();
        prior.conditions.push(LedgerCondition {
            condition_id: "keep-2".into(),
            role: ConditionRole::Falsifier,
            trigger_family: None,
            statement: "Trailing return collapses harder".into(),
            quant: Some(QuantCore {
                series: engine::LedgerSeries::TrailingReturn,
                comparator: LedgerComparator::Below,
                threshold: -0.60,
                margin: 0.02,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: Some(ConditionEvalState::default()),
        });
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        // Duplicate the first falsifier (the -0.40 core) and drop the -0.60 one
        // from the draft, so a leaked duplicate would supersede "keep-2".
        let dup = draft.falsifiers[0].clone();
        draft
            .falsifiers
            .retain(|f| f.quant.as_ref().map(|q| q.threshold) != Some(-0.60));
        draft.falsifiers.push(dup);
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
        assert_eq!(audit.duplicates.len(), 1, "{:?}", audit.duplicates);
        // Exactly one -0.40 condition persists, carrying its id; keep-2 was
        // closed (removed by the rewrite), never superseded by the duplicate.
        let kept: Vec<&LedgerCondition> = ledger
            .conditions
            .iter()
            .filter(|c| c.quant.as_ref().map(|q| q.threshold) == Some(-0.40))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].condition_id, "keep-1");
        assert!(audit.superseded.is_empty(), "{:?}", audit.superseded);
        assert!(
            audit
                .closed
                .iter()
                .any(|c| c.condition.condition_id == "keep-2"),
            "{:?}",
            audit.closed
        );

        // A duplicated qualitative statement dedups the same way.
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        let dup = draft
            .falsifiers
            .iter()
            .find(|f| f.quant.is_none())
            .unwrap()
            .clone();
        draft.falsifiers.push(dup);
        let (_, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, None);
        assert_eq!(audit.duplicates.len(), 1, "{:?}", audit.duplicates);
    }

    #[test]
    fn ledger_section_renders_debut_prior_and_crossings() {
        // Debut: the vocabulary and the authoring instruction.
        let s = ledger_prompt_section(None, None, false);
        assert!(s.contains("ENGINE SERIES"), "{s}");
        assert!(s.contains("net-margin"), "{s}");
        assert!(s.contains("debut"), "{s}");
        assert!(s.contains("REWRITE THE THESIS LEDGER"), "{s}");

        // A prior ledger renders whole — the first prior-run content in the prompt —
        // with the engine's crossings and typed unevaluable notes beside it.
        let prior = prior_with_conditions();
        let eval = LedgerEvaluation {
            crossings: vec![ConditionCrossing {
                condition_id: "keep-1".into(),
                statement: "Trailing return collapses".into(),
                role: ConditionRole::Falsifier,
                outcome: CrossingOutcome::Confirmed,
                observed_value: -0.45,
                threshold: -0.40,
                observation_id: "2026-07-16".into(),
                // The engine stamped this on the confirming pass with the run's
                // ET session date (`run_date`).
                confirmed_at: Some("2026-07-16".into()),
            }],
            unevaluable: vec!["condition 'x': net margin is a gap this run".into()],
            unevaluable_series: vec![engine::LedgerSeries::NetMargin],
            updated_states: vec![],
        };
        let s = ledger_prompt_section(Some(&prior), Some(&eval), false);
        assert!(s.contains("the debut thesis"), "original thesis renders: {s}");
        assert!(s.contains("the standing thesis"), "{s}");
        assert!(s.contains("CONFIRMED BREACH"), "{s}");
        assert!(s.contains("unevaluable this run"), "{s}");
        assert!(s.contains("breach streak 1"), "the live streak renders: {s}");
        // The FULL machine core renders, margin included — an unstated margin
        // would force the model to guess one, and a guessed mismatch reads as a
        // core edit that supersedes the condition (Codex round 1, finding 1).
        assert!(s.contains("(margin 0.02)"), "{s}");
        // The ledger carries no target-weight range under the tunnel-vision
        // contract — a weight is a book fact, retired from the per-holding loop.
        assert!(!s.contains("Target weight range"), "{s}");

        // The role_risk variant names the branch reductions.
        let rr = ledger_prompt_section(Some(&prior), None, true);
        assert!(rr.contains("trim/sell only"), "{rr}");

        // Both interpretation prompts carry the section.
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(user.contains("REWRITE THE THESIS LEDGER"), "{user}");
        assert!(interpretation_system_prompt().contains("THESIS LEDGER"));
        assert!(role_risk_system_prompt().contains("THESIS LEDGER"));
    }

    /// Finding 4 (`docs/verification/2026-08-10-big-run-attempt-1.md`): the header
    /// names the issuer and marks the money figures as dollar totals, so the model
    /// does not spend its reasoning deciding whether a bare integer is per-share.
    #[test]
    fn the_holding_header_marks_dollar_totals_and_names_the_issuer() {
        let mut d = dossier(AssetClass::Stock, strong_financials());

        // A usable account description is used as-is.
        d.position.description = "Phillips 66".to_string();
        d.company_name = Some("Phillips 66 Company".to_string());
        let h = holding_header(&d);
        assert!(h.contains("Phillips 66)"), "{h}");
        assert!(h.contains("Cost basis: $"), "{h}");
        assert!(h.contains(" total"), "{h}");
        assert!(h.contains("Market value: $"), "{h}");

        // Every no-identity shape falls back to the profile name — blank, whitespace,
        // the ticker repeated, and corporate-form noise that tokenizes to nothing.
        // `listing::describes_issuer` owns that rule; the guard pins the same set.
        let ticker = d.position.symbol.clone();
        for described in ["", "  ", &ticker, "COMMON STOCK", "CL A ORD SHS"] {
            d.position.description = described.to_string();
            let h = holding_header(&d);
            assert!(
                h.contains("Phillips 66 Company"),
                "no-identity description {described:?} should fall back: {h}"
            );
        }

        // The fallback is held to the same standard, so profile-side noise cannot
        // rebuild the header: FMP accepts any non-blank `companyName`, and the guard
        // never compares it once the description has no identity of its own.
        d.position.description = String::new();
        for profile_name in ["COMMON STOCK", &ticker, "  ", "ORD SHS"] {
            d.company_name = Some(profile_name.to_string());
            let h = holding_header(&d);
            assert!(
                h.contains("name unavailable"),
                "profile name {profile_name:?} carries no identity and must not render: {h}"
            );
        }

        // Nothing to fall back to is stated, never rendered as an empty pair.
        d.company_name = None;
        let h = holding_header(&d);
        assert!(h.contains("name unavailable"), "{h}");
        assert!(!h.contains("()"), "an empty name pair is the defect itself: {h}");

        // A ticker-named issuer: description AND profile name both tokenize to
        // just the ticker, but the profile name is a canonical legal name and
        // must render — holding the fallback to the description's stricter
        // rule starved these headers entirely (combined-range review).
        d.position.symbol = "ASML".to_string();
        d.position.description = "ASML HOLDING NV".to_string();
        d.company_name = Some("ASML Holding N.V.".to_string());
        let h = holding_header(&d);
        assert!(h.contains("ASML Holding N.V."), "{h}");
        assert!(!h.contains("name unavailable"), "{h}");
        // The bare ticker as a profile name is still rejected.
        d.company_name = Some("ASML".to_string());
        let h = holding_header(&d);
        assert!(h.contains("name unavailable"), "{h}");
    }

    /// The fund half of Finding 4's fallback: funds get no /profile call, so
    /// `company_name` is structurally `None` on the role-risk branch — the
    /// fetched fund data's own name is that branch's only naming source, and a
    /// blank Schwab description must reach it rather than "name unavailable".
    #[test]
    fn the_holding_header_falls_back_to_the_fund_name_for_funds() {
        let mut d = dossier(AssetClass::Etf, strong_financials());
        d.position.description = String::new();
        d.company_name = None;
        d.fund = Some(FundContext {
            fund: us_equity_fund(),
            sector_pe: vec![],
            sector_pe_history: Default::default(),
            as_of: chrono::NaiveDate::from_ymd_opt(2026, 7, 16).unwrap(),
            positioning: None,
        });
        let h = holding_header(&d);
        assert!(h.contains("Total US Market ETF"), "{h}");

        // The fund name is held to the same identity standard: noise or the
        // ticker repeated must not rebuild the header.
        if let Some(f) = d.fund.as_mut() {
            f.fund.name = Some(d.position.symbol.clone());
        }
        let h = holding_header(&d);
        assert!(h.contains("name unavailable"), "{h}");
    }

    /// Finding 2: each grammar-constrained call declares the object it is enforced
    /// to produce, so the model does not re-derive the key set on the shared budget.
    /// The required-key list is read off each schema rather than restated here, so a
    /// field added to a grammar fails this test until its prompt declares it. A
    /// hand-copied list would drift silently, which is the whole defect Finding 2
    /// describes: a contract enforced in one place and unstated in the other.
    fn required_keys(schema: &serde_json::Value) -> Vec<String> {
        schema["required"]
            .as_array()
            .expect("every schema pins its required set")
            .iter()
            .map(|k| k.as_str().expect("required entries are strings").to_string())
            .collect()
    }

    /// Guards the loops below against reading an empty set and passing vacuously.
    fn non_empty(keys: Vec<String>, what: &str) -> Vec<String> {
        assert!(!keys.is_empty(), "{what} declared no required keys");
        keys
    }

    #[test]
    fn every_constrained_prompt_declares_its_own_response_keys() {
        // Containment over a whole prompt proves nothing here: every one of these
        // prompts mentions some of its own key names in the instructional prose above
        // the declaration (`conviction`, `ledger`, `self_assessment` in the priced
        // branch; `ledger` in role-risk; `action`, `rationale` in the action call).
        // So each contract is generated from the constant its schema's `required`
        // set is built from, and the two seams that leaves are what this pins:
        // schema-from-constant, and prompt-carries-contract.
        use crate::portfolio as pf;

        struct ContractCase {
            what: &'static str,
            required: Vec<String>,
            keys: Vec<&'static str>,
            contract: String,
            prompt: String,
        }
        let cases = [
            ContractCase {
                what: "priced",
                required: required_keys(&pf::interpretation_schema()),
                keys: pf::INTERPRETATION_KEYS.to_vec(),
                contract: pf::interpretation_response_contract(),
                prompt: interpretation_system_prompt(),
            },
            ContractCase {
                what: "role-risk",
                required: required_keys(&pf::role_risk_interpretation_schema()),
                keys: pf::ROLE_RISK_KEYS.to_vec(),
                contract: pf::role_risk_response_contract(),
                prompt: role_risk_system_prompt(),
            },
            ContractCase {
                what: "action call",
                required: required_keys(&pf::action_decision_schema()),
                keys: pf::ACTION_KEYS.to_vec(),
                contract: pf::action_response_contract(),
                prompt: action_system_prompt(),
            },
        ];

        for c in cases {
            assert_eq!(
                non_empty(c.required, c.what),
                c.keys.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
                "{}: schema drifted from the key constant",
                c.what
            );
            let declared = c.keys.join(", ");
            assert!(
                c.contract.contains(&declared),
                "{}: contract does not declare the exact key list `{declared}`",
                c.what
            );
            assert!(
                c.prompt.contains(&c.contract),
                "{}: prompt does not carry the contract",
                c.what
            );
        }

        // The branch carries no action of its own — declaring one would invite it.
        assert!(!pf::role_risk_response_contract().contains("model_price_targets"));

        // The internal build vocabulary of Finding 3 stays out of every prompt.
        for p in [
            interpretation_system_prompt(),
            role_risk_system_prompt(),
            action_system_prompt(),
        ] {
            assert!(!p.contains("pre-v7"), "internal version vocabulary leaked: {p}");
        }
    }

    #[test]
    fn debut_authors_a_ledger_and_an_abstention_retains_the_prior_one() {
        // A priced debut carries the authored ledger, its monitor stamped from the
        // engine's own scenario set.
        let (verdict, audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        let ledger = verdict.thesis_ledger.as_ref().expect("priced verdict carries a ledger");
        assert_eq!(ledger.branch, LedgerBranch::Priced);
        assert_eq!(ledger.original_thesis, ledger.current_thesis);
        let twelve = match &verdict.disposition {
            VerdictDisposition::Priced(g) => g.price_targets.twelve_month.clone().unwrap(),
            other => panic!("{other:?}"),
        };
        let base = ledger
            .monitor
            .iter()
            .find(|m| m.scenario == ScenarioKind::Base)
            .unwrap();
        assert_eq!(base.engine_target, Some(twelve.base));
        assert!(audit.ledger_audit.is_some());

        // An insufficient-evidence exit retains the standing ledger unchanged —
        // 6c–6f never ran for it (`docs/portfolio-workflow.md` §Step 6b).
        let thin = CompanyFinancials {
            symbol: "X".into(),
            current_price: Some(50.0),
            ..CompanyFinancials::default()
        };
        let mut d = dossier(AssetClass::Stock, thin);
        d.prior_verdict = Some(HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated { reason: "fixture".into() },
            thesis_ledger: Some(prior_with_conditions()),
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        });
        let (v2, _) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(matches!(
            v2.disposition,
            VerdictDisposition::InsufficientEvidence { .. }
        ));
        assert_eq!(v2.thesis_ledger, Some(prior_with_conditions()));
    }

    #[test]
    fn a_market_data_trigger_walks_through_first_breach_to_confirmed_and_ack() {
        // The stub's debut trigger (price above 150) is breached at spot 195 —
        // a market-data condition (count 2) whose observation identity is the
        // marks' trading day: run 2 logs a quiet first-breach note, run 3 —
        // carrying a genuinely NEW trading print — confirms and fires, and the
        // consuming pass stamps the acknowledging observation.
        let d1 = dossier(AssetClass::Stock, strong_financials());
        let (v1, _) = analyze_holding(&StubAnalyst, &d1, &rates(), "2026-08-03").unwrap();

        let mut d2 = dossier(AssetClass::Stock, strong_financials());
        d2.prior_verdict = Some(v1);
        let (v2, audit2) =
            analyze_holding(&StubAnalyst, &d2, &rates(), "2026-08-04").unwrap();
        let a2 = audit2.ledger_audit.unwrap();
        assert!(
            a2.crossings.iter().any(|c| c.role == ConditionRole::Trigger
                && c.outcome == CrossingOutcome::FirstBreach),
            "{:?}",
            a2.crossings
        );

        // A rerun with NO new trading print must not advance the streak — the
        // observation identity is the marks' day, never the run's calendar date.
        let mut d2b = dossier(AssetClass::Stock, strong_financials());
        d2b.prior_verdict = Some(v2.clone());
        let (_, audit2b) =
            analyze_holding(&StubAnalyst, &d2b, &rates(), "2026-08-05").unwrap();
        let a2b = audit2b.ledger_audit.unwrap();
        assert!(
            !a2b
                .crossings
                .iter()
                .any(|c| c.outcome == CrossingOutcome::Confirmed),
            "{:?}",
            a2b.crossings
        );

        let mut fin3 = strong_financials();
        fin3.daily_closes.push(
            crate::portfolio::engine::DatedValue {
                date: "2026-08-05".into(),
                value: 196.0,
            },
        );
        let mut d3 = dossier(AssetClass::Stock, fin3);
        d3.prior_verdict = Some(v2);
        let (v3, audit3) =
            analyze_holding(&StubAnalyst, &d3, &rates(), "2026-08-05").unwrap();
        let a3 = audit3.ledger_audit.unwrap();
        assert!(
            a3.crossings.iter().any(|c| c.role == ConditionRole::Trigger
                && c.outcome == CrossingOutcome::Confirmed),
            "{:?}",
            a3.crossings
        );
        let l3 = v3.thesis_ledger.unwrap();
        let trigger = l3
            .conditions
            .iter()
            .find(|c| c.role == ConditionRole::Trigger)
            .unwrap();
        assert_eq!(
            trigger
                .eval_state
                .as_ref()
                .unwrap()
                .acknowledged_observation_id
                .as_deref(),
            Some("2026-08-05"),
            "the consuming pass acknowledges the confirming observation"
        );
    }

    // ---- The pre-profit execution / financing overlay ----------------------------

    use crate::portfolio::pre_profit::{
        ConvictionCeiling, MetricKind, ObservationPolarity, ObservationRole, PreProfitObservation,
    };

    /// An overlay-eligible stock: the strong fixture with negative TTM operating
    /// income, quarterly cash-flow prints, and balance-sheet cash lines.
    fn pre_profit_financials() -> CompanyFinancials {
        let mut fin = strong_financials();
        for row in &mut fin.quarterly_income {
            row.operating_income = Some(-2.0e9);
        }
        fin.quarterly_cash_flow = fin
            .quarterly_income
            .iter()
            .take(8)
            .map(|r| crate::portfolio::engine::QuarterlyCashFlowRow {
                period_end: r.period_end.clone(),
                filing_date: None,
                free_cash_flow: Some(-1.0e9),
                operating_cash_flow: None,
                capex: Some(-0.5e9),
            })
            .collect();
        fin.cash_and_equivalents = Some(6.0e9);
        fin.short_term_investments = Some(4.0e9);
        fin
    }

    fn pre_profit_observation(
        role: ObservationRole,
        value: f64,
        period: &str,
    ) -> PreProfitObservation {
        PreProfitObservation {
            metric_kind: MetricKind::Deliveries,
            observation_role: role,
            polarity: ObservationPolarity::HigherIsBetter,
            numeric_value: value,
            units: "units".into(),
            period: period.into(),
            issuer_scope: "company".into(),
            source_url: "https://example.com/ir".into(),
            published_at: "2026-08-01".into(),
            confidence: 0.9,
        }
    }

    /// A prior overlay whose history carries guidance misses in two distinct
    /// periods for one metric — the repeated-miss shape.
    fn prior_overlay_with_repeated_miss() -> crate::portfolio::pre_profit::PreProfitOverlay {
        let mut prior =
            crate::portfolio::pre_profit::compute_overlay(&pre_profit_financials(), None, vec![]);
        prior.observations = vec![
            pre_profit_observation(ObservationRole::GuidanceLow, 100.0, "2026-Q1"),
            pre_profit_observation(ObservationRole::Actual, 90.0, "2026-Q1"),
            pre_profit_observation(ObservationRole::GuidanceLow, 100.0, "2026-Q2"),
            pre_profit_observation(ObservationRole::Actual, 92.0, "2026-Q2"),
        ];
        prior
    }

    #[test]
    fn every_stock_records_an_overlay_and_funds_record_none() {
        // A profitable stock with no operating-income prints: the eligibility result
        // still persists (unscorable — not entered, gap recorded).
        let (_, audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        let overlay = audit.pre_profit.expect("every stock records an overlay");
        assert!(!overlay.is_eligible());
        assert!(matches!(
            overlay.eligibility,
            crate::portfolio::pre_profit::PreProfitEligibility::Unscorable { .. }
        ));

        // A priced fund records none — the overlay is stock surface.
        let (_, audit) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(us_equity_fund()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert!(audit.pre_profit.is_none());
    }

    #[test]
    fn eligible_overlay_renders_clamps_and_persists() {
        let mut d = dossier(AssetClass::Stock, pre_profit_financials());
        d.prior_pre_profit = Some(prior_overlay_with_repeated_miss());
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();

        let overlay = audit.pre_profit.expect("overlay rides the audit");
        assert!(overlay.is_eligible());
        assert!(overlay.execution.repeated_miss);
        assert_eq!(
            overlay.consequences.conviction_ceiling,
            Some(ConvictionCeiling::Medium)
        );
        // The stub proposed High (A/B grade); under v7 it persists as authored —
        // the engine-matched ceiling stays recorded on the overlay as an
        // annotation the render sets beside the model's value, never a clamp.
        let VerdictDisposition::Priced(g) = verdict.disposition else {
            panic!("expected a priced verdict");
        };
        assert_eq!(g.conviction, Conviction::High);
        assert!(overlay
            .consequences
            .matched_rules
            .iter()
            .any(|r| r.contains("repeated-execution-miss")));
        // The observation history carried through the run.
        assert_eq!(overlay.observations.len(), 4);

        // The prompt renders the overlay block with the ceiling under the same
        // input the live call builds.
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "none",
            ledger_eval: None,
            pre_profit: Some(&overlay),
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(user.contains("PRE-PROFIT EXECUTION / FINANCING OVERLAY"), "{user}");
        // The ceiling renders as the ENGINE arm's own rule with the model arm
        // explicitly unrestricted — never binding language aimed at the model
        // (Codex round 1, finding 1).
        assert!(
            user.contains("CONVICTION CEILING (engine rule): the engine arm holds its own \
                           conviction at or beneath medium"),
            "{user}"
        );
        assert!(user.contains("Your conviction is UNRESTRICTED"), "{user}");
        assert!(!user.contains("binds after any raise"), "{user}");
        assert!(!user.contains("your action must be"), "{user}");
        assert!(user.contains("repeated-execution-miss"), "{user}");
    }

    #[test]
    fn severe_overlay_binds_the_engine_arm_never_the_model() {
        // Repeated miss + constrained runway (tiny cash against the burn) → the
        // severe conjunction. Under v7 a defiant model lean and conviction persist
        // exactly as authored — no bail, no clamp — while the consequences bind
        // the ENGINE arm's action and stay recorded for the annotation render.
        struct DefiantAnalyst;
        impl HoldingAnalyst for DefiantAnalyst {
            fn distill(&self, d: &HoldingDossier, f: &ResearchFindings) -> Result<String> {
                StubAnalyst.distill(d, f)
            }
            fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
                let mut i = StubAnalyst.interpret(input)?;
                i.conviction = Conviction::High;
                Ok(i)
            }
            fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
                StubAnalyst.interpret_role_risk(input)
            }
            fn decide_action(
                &self,
                _input: &ActionInput,
            ) -> Result<crate::portfolio::ActionDecision> {
                Ok(crate::portfolio::ActionDecision {
                    action: Action::Add,
                    rationale: "defiant: add against the severe overlay".to_string(),
                })
            }
            fn fast_id(&self) -> String {
                "defiant".into()
            }
            fn reasoner_id(&self) -> String {
                "defiant".into()
            }
        }
        let mut fin = pre_profit_financials();
        fin.cash_and_equivalents = Some(1.0e9);
        fin.short_term_investments = None;
        let mut d = dossier(AssetClass::Stock, fin);
        d.prior_pre_profit = Some(prior_overlay_with_repeated_miss());
        let (verdict, audit) =
            analyze_holding(&DefiantAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let overlay = audit.pre_profit.expect("overlay rides the audit");
        assert!(overlay.severe_deterioration);
        assert_eq!(
            overlay.consequences.conviction_ceiling,
            Some(ConvictionCeiling::Low)
        );
        let VerdictDisposition::Priced(g) = verdict.disposition else {
            panic!("expected a priced verdict");
        };
        assert_eq!(g.action, Action::Add, "the model's lean persists as authored");
        assert_eq!(g.conviction, Conviction::High, "no clamp under v7");
        let ev = &g.engine_view;
        assert!(
            matches!(ev.action, Action::Trim | Action::SellAll),
            "the engine arm obeys its own severe bar, got {:?}",
            ev.action
        );
        // The engine arm's conviction observes its own ceiling too: severe
        // deterioration's Low ceiling binds the stand-in, never the model.
        assert_eq!(
            ev.conviction,
            Conviction::Low,
            "the severe overlay's Low ceiling binds the engine arm's conviction"
        );

        // The overlay section is stage-aware: each prompt carries the model-arm
        // guidance for what THAT stage authors, and the other stage's consequence
        // as context only. Interpretation authors conviction and no action; the
        // action call authors the rung and no conviction.
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            engine: &engine_output,
            distilled: "none",
            ledger_eval: None,
            pre_profit: Some(&overlay),
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(interp.contains("Your conviction is UNRESTRICTED"), "{interp}");
        assert!(
            interp.contains("the engine's own action set narrows to the exit family"),
            "{interp}"
        );
        assert!(interp.contains("the action decision stage that follows weighs this"), "{interp}");
        assert!(!interp.contains("Your rung is UNRESTRICTED"), "{interp}");

        let engine_set = engine::feasible_actions(
            engine_output.grade,
            &engine_output.hurdle,
            Some(&overlay.consequences),
            false,
        );
        let action = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced {
                graded: &g,
                engine: &engine_output,
                pre_profit: Some(&overlay),
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(action.contains("Your rung is UNRESTRICTED"), "{action}");
        assert!(action.contains("CONVICTION CEILING (engine rule, context)"), "{action}");
        assert!(action.contains("this call authors no conviction"), "{action}");
        assert!(!action.contains("Your conviction is UNRESTRICTED"), "{action}");
        assert!(!action.contains("the action decision stage that follows"), "{action}");

        // The retired "lean" vocabulary is gone from both renders.
        for prompt in [&interp, &action] {
            assert!(!prompt.contains("lean set"), "{prompt}");
            assert!(!prompt.contains("your lean"), "{prompt}");
            assert!(!prompt.contains("Your lean"), "{prompt}");
        }
    }

    #[test]
    fn abstaining_stock_still_records_the_overlay() {
        // No consensus at all: the engine abstains (no-admissible-driver), but the
        // overlay record — statement leg + carried history — persists with the
        // abstention, like the standing ledger does.
        let mut fin = pre_profit_financials();
        fin.consensus = None;
        let mut d = dossier(AssetClass::Stock, fin);
        d.prior_pre_profit = Some(prior_overlay_with_repeated_miss());
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::InsufficientEvidence { .. }
        ));
        let overlay = audit.pre_profit.expect("overlay survives an abstention");
        assert!(overlay.is_eligible());
        assert_eq!(overlay.observations.len(), 4, "history carried");
    }

    #[test]
    fn a_guard_conflict_abstention_records_a_carrying_overlay() {
        use crate::portfolio::listing::ListingResolution;
        use crate::portfolio::pre_profit::PreProfitEligibility;
        // The conflicting-identity exit takes the floor exit's full overlay
        // semantics: the guard-terminal skip fetched no statements, so the
        // record reads eligibility-unscorable with its input gaps — but the
        // period-keyed observation history carries, so one conflicted
        // (possibly transient) run can never reset it.
        let mut d = dossier(
            AssetClass::Stock,
            CompanyFinancials { symbol: "X".into(), ..CompanyFinancials::default() },
        );
        d.listing = Some(ListingResolution::Conflict {
            fmp_name: "Wrong Issuer Inc.".into(),
        });
        d.prior_pre_profit = Some(prior_overlay_with_repeated_miss());
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-05").unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::InsufficientEvidence { .. }
        ));
        let overlay = audit.pre_profit.expect("the record survives the guard exit");
        assert!(
            matches!(overlay.eligibility, PreProfitEligibility::Unscorable { .. }),
            "no statement was fetched — the read is unscorable, never inferred: {:?}",
            overlay.eligibility
        );
        assert_eq!(overlay.observations.len(), 4, "history carried, not reset");
    }

    #[test]
    fn pre_overlay_audit_json_decodes_with_a_none_overlay() {
        // A HoldingAudit persisted before the field existed decodes with `None`
        // (the `#[serde(default)]` contract the whole-row carry path relies on).
        let (_, audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        let mut json = serde_json::to_value(&audit).unwrap();
        json.as_object_mut().unwrap().remove("pre_profit");
        let back: HoldingAudit = serde_json::from_value(json).unwrap();
        assert!(back.pre_profit.is_none());
    }
}
