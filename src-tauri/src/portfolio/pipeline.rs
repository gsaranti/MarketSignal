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
//! The **web-research stage is live** (the research-loop slice): Step 6c runs
//! the bounded per-topic loop ([`crate::portfolio::research`]) and Step 6d the
//! deterministic single/hierarchical distillation primitive
//! ([`crate::portfolio::distill`]) — both behind the [`HoldingAnalyst`] trait,
//! whose defaulted offline paths keep every deterministic stub pipeline-shaped
//! with no web tool or daemon.

use anyhow::{Context, Result};

use crate::local_model::{options, ChatMessage, ChatRequest, LocalModelClient, StreamRole};
use crate::portfolio::dossier::HoldingDossier;
use crate::portfolio::engine::{self, EngineOutput, EngineVerdict, LedgerEvaluation, RateAnchors};
use crate::portfolio::fund::{self, FundEngineVerdict, FundStructuralKind, RoleRiskReadout};
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

use crate::portfolio::distill::{self, DistillInputs, DistilledResearch, ResearchAuditRecord};
use crate::portfolio::research::{self, HoldingResearch, ResearchPlan};

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
    /// The prior thesis ledger AS INGESTED by this run — basis-normalized where
    /// a split re-based the series (`docs/portfolio-analysis.md` §Starting
    /// parameters), so the render, the evaluation, and the 6g carry all read one
    /// instance. `None` on a debut. Never re-derived from the dossier.
    pub prior_ledger: Option<&'a ThesisLedger>,
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
    /// The prior thesis ledger as ingested by this run (basis-normalized where a
    /// split re-based the series) — the render's single instance; never
    /// re-derived from the dossier.
    pub prior_ledger: Option<&'a ThesisLedger>,
    /// The engine's evaluation of the prior fund ledger's quantitative conditions.
    pub ledger_eval: Option<&'a LedgerEvaluation>,
    /// The rendered input delta — the branch's reduced entry set. Empty on a
    /// debut.
    pub input_delta: &'a [crate::portfolio::DeltaEntry],
    /// The distilled fund research — pure consolidation on this branch (the
    /// fund agenda ran; no typed field exists here).
    pub distilled: &'a str,
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
        /// The holding's ledger as validated this run — the packet renders its
        /// thesis and scenario rows (ruled 2026-09-17).
        ledger: &'a ThesisLedger,
    },
    RoleRisk {
        verdict: &'a RoleRiskVerdict,
        ledger: &'a ThesisLedger,
    },
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
    /// Validated attribution from this run's interpretation, before action.
    pub changes: Option<&'a crate::portfolio::WhatChangedAudit>,
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

/// The app-composed tax caveat appended to the rationale after the rung is
/// fixed (`docs/portfolio-analysis.md` §Portfolio action, ruled 2026-09-16): the
/// tax posture never enters the decision — the packet carries no tax row and no
/// P/L — and the caveat rides only an exit-family rung under a tax-aware profile
/// on a position with an unrealized gain or loss.
pub const TAX_CAVEAT_GAIN: &str = "Tax note: this position carries an unrealized gain, so \
realizing part or all of it may carry a tax cost; account type, tax lots, holding periods \
and rates are unmodeled.";
pub const TAX_CAVEAT_LOSS: &str = "Tax note: this position carries an unrealized loss, so \
realizing it may carry a tax benefit; account type, tax lots, holding periods and rates are \
unmodeled.";

/// The caveat for this rung, profile and position — `None` where nothing is
/// realized (hold, the add family), under a tax-exempt profile, or at break-even.
pub fn tax_caveat(
    profile: &crate::portfolio::InvestorProfile,
    position: &crate::schwab::Position,
    action: Action,
) -> Option<&'static str> {
    if !profile.tax_sensitive || !matches!(action, Action::Trim | Action::SellAll) {
        return None;
    }
    let pl = position.market_value - position.cost_basis;
    if pl > 0.0 {
        Some(TAX_CAVEAT_GAIN)
    } else if pl < 0.0 {
        Some(TAX_CAVEAT_LOSS)
    } else {
        None
    }
}

/// The model's investment sentence alone — the persisted rationale less the
/// app's appended tax caveat, where one rides. The Step-7 summary embedding
/// vectorizes this form, never the persisted rationale, so the Step-6a recall
/// of a prior trim or sell cannot re-supply the tax posture or the P/L sign to
/// the next intrinsic interpretation (fix list 3.2; the §3 slice's Codex
/// implementation review, superseding the §1+§2 slice's A3 acceptance). The
/// caveat is a fixed sentence joined by one space (`with_tax_caveat`), so the
/// strip is exact; a rationale without one passes through untouched.
pub fn investment_sentence(rationale: &str) -> &str {
    for caveat in [TAX_CAVEAT_GAIN, TAX_CAVEAT_LOSS] {
        if let Some(sentence) = rationale.strip_suffix(caveat) {
            return sentence.trim_end();
        }
    }
    rationale
}

/// The persisted rationale: the model's sentence, then the app's caveat where one
/// applies.
fn with_tax_caveat(
    rationale: String,
    profile: &crate::portfolio::InvestorProfile,
    position: &crate::schwab::Position,
    action: Action,
) -> String {
    match tax_caveat(profile, position, action) {
        Some(caveat) => format!("{} {caveat}", rationale.trim_end()),
        None => rationale,
    }
}

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
/// stub-driven offline and daemon-driven live. Research (6c) and distillation
/// (6d) carry **defaulted offline implementations** — pipeline-shaped, no web
/// tool, no model call — so deterministic stubs stay small; the live analyst
/// overrides both.
pub trait HoldingAnalyst {
    /// Step 6c — the bounded per-topic research loop
    /// (`docs/portfolio-workflow.md` §Step 6c). Defaults to the offline stub
    /// (a deterministic research-unavailable note plus recorded gaps).
    fn research(
        &self,
        _dossier: &HoldingDossier,
        plan: &ResearchPlan,
    ) -> Result<HoldingResearch> {
        Ok(research::offline_stub(plan))
    }
    /// Step 6d — the distillation primitive (`docs/portfolio-workflow.md`
    /// §Step 6d). Defaults to the deterministic offline consolidation (no
    /// model call, no typed fields).
    fn distill_research(&self, inputs: &DistillInputs) -> Result<DistilledResearch> {
        Ok(distill::offline_consolidate(inputs))
    }
    /// The consolidation call's input budget (chars) the deterministic
    /// single-vs-hierarchical routing sizes against. The live analyst derives
    /// it from the resolved distill `num_ctx`; the offline default is generous.
    fn distill_input_budget(&self) -> usize {
        200_000
    }

    /// The widest rendered prompt the adapter will issue — the reasoner's
    /// budget on a distinct roster, the shared budget on the default one. The
    /// rendered-size fallbacks in `distill` compare against it, so a smaller
    /// shape is taken only where the issue guard would refuse.
    fn distill_issue_budget(&self) -> usize {
        self.distill_input_budget()
    }
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
    /// The configured distillation-tier id, used only as the compatibility
    /// fallback for analysts that do not expose [`Self::take_model_calls`].
    fn fast_id(&self) -> String;
    /// The model id [`Self::interpret`], [`Self::interpret_role_risk`], and
    /// [`Self::decide_action`] run on. Recorded on the audit only after one of
    /// those calls actually ran.
    fn reasoner_id(&self) -> String;
    /// Drain the model ids of outbound calls since the last drain, in issue
    /// order. `Some` means the analyst provides exact call telemetry (including
    /// an honestly empty vector); `None` keeps deterministic/custom stubs on
    /// the configured-id compatibility path. The live analyst records the
    /// request's routed model before every daemon call, so research-before-
    /// distill order and a distill routed up to the reasoner survive exactly.
    fn take_model_calls(&self) -> Option<Vec<String>> {
        None
    }
    /// Drain the prompt-size observations the calls above accumulated since
    /// the last drain ([`crate::local_model::PromptUsage`]) — the data-health
    /// context-fit read (`docs/portfolio-analysis.md` §Portfolio roll-up). The
    /// job drains at each holding's checkpoint boundary so a completed
    /// holding's rows ride its checkpoint row. Defaulted empty so deterministic
    /// stubs carry no instrumentation.
    fn take_prompt_usage(&self) -> Vec<crate::local_model::PromptUsage> {
        Vec::new()
    }
    /// Drain the fired bounded-retry records since the last drain
    /// ([`crate::local_model::RetryEvent`]) — the data-health model-retry read
    /// (`docs/local-models.md §The local-model adapter seam`), drained at the
    /// same boundary as the usage. Defaulted empty like the usage drain.
    fn take_retry_events(&self) -> Vec<crate::local_model::RetryEvent> {
        Vec::new()
    }
}

/// Run Steps 6c–6d for one holding: assemble the deterministic research plan
/// (agenda, structured seeds, per-topic cross-run seed texts), run the
/// analyst's research loop, then the distillation primitive — returning the
/// distilled output beside the audit record the run persists.
fn run_research_and_distill(
    analyst: &dyn HoldingAnalyst,
    dossier: &HoldingDossier,
    triggers: &research::AgendaTriggers,
    prior_ledger: Option<&ThesisLedger>,
    role_risk: bool,
    run_date: &str,
) -> Result<(DistilledResearch, ResearchAuditRecord, HoldingResearch)> {
    let symbol = &dossier.position.symbol;
    // The run's session date at midnight UTC — the topic layer's vintage stamp
    // and the seed windows' "now" (day precision is ample against ~4-week
    // windows; claims keep their own full retrieval timestamps).
    let now = chrono::NaiveDate::parse_from_str(run_date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);

    let agenda = research::build_agenda(dossier, triggers);
    // Per-topic cross-run seeds, assembled deterministically — never by a
    // model call: the topic object's window gates seeding; each claim expires
    // by its own vintage; the ledger's conditions lead the priority order
    // (`docs/portfolio-analysis.md` §Starting parameters — Research reuse).
    let mut topic_seeds = std::collections::HashMap::new();
    for topic in &agenda {
        let prior = dossier
            .research_priors
            .iter()
            .find(|p| p.topic_key == topic.key);
        if let Some(seed) = research::assemble_topic_seed(prior, prior_ledger, now) {
            let vintage = prior
                .filter(|p| research::topic_object_fresh(p, now))
                .map(|p| p.vintage.clone())
                .unwrap_or_default();
            topic_seeds.insert(topic.key.clone(), (seed, vintage));
        }
    }
    let plan = ResearchPlan {
        agenda,
        seeds: dossier.news_seeds.clone(),
        topic_seeds,
        // The holding's own tracker step: the loop's thinking and request rows
        // land on the step the job already opened for this symbol.
        step_label: crate::portfolio::holding_step_key(symbol),
    };
    let research_out = analyst
        .research(dossier, &plan)
        .context("researching the holding")?;

    // Only non-expired topic objects join the distillation merge.
    let priors: Vec<research::TopicDistillate> = dossier
        .research_priors
        .iter()
        .filter(|p| research::topic_object_fresh(p, now))
        .cloned()
        .collect();
    let ledger_conditions: Vec<LedgerCondition> = prior_ledger
        .map(|l| l.conditions.clone())
        .unwrap_or_default();
    let ledger_key_drivers: Vec<crate::portfolio::KeyDriver> = prior_ledger
        .map(|l| l.key_drivers.clone())
        .unwrap_or_default();
    // The shared holding header opens every distillation message
    // (`portfolio-v44`); a fund's distillation is pure consolidation like a
    // role/risk holding's, since no consensus driver, narrative cap or overlay
    // reads a fund's typed field (ruled 2026-09-17).
    let brief = holding_header(dossier);
    let inputs = DistillInputs {
        symbol,
        company_name: dossier.company_name.as_deref(),
        holding_brief: &brief,
        research: &research_out,
        priors: &priors,
        ledger_conditions: &ledger_conditions,
        ledger_key_drivers: &ledger_key_drivers,
        consolidation_only: role_risk || dossier_is_fund(dossier),
        overlay_eligible: triggers.overlay_eligible,
        backfill_required: triggers.pre_profit_backfill,
        input_budget_chars: analyst.distill_input_budget(),
        issue_budget_chars: analyst.distill_issue_budget(),
        now,
    };
    let distilled = analyst
        .distill_research(&inputs)
        .context("distilling research findings")?;

    let mut sources: Vec<String> = research_out
        .topics
        .iter()
        .flat_map(|t| t.passes.iter())
        .chain(research_out.disconfirming.iter())
        .flat_map(|p| p.claims.iter())
        .map(|c| format!("{} ({})", c.source_url, c.retrieved_at))
        .collect();
    sources.sort();
    sources.dedup();
    let mut gaps = research_out.gaps.clone();
    gaps.extend(distilled.gaps.iter().cloned());
    let record = ResearchAuditRecord {
        combined: distilled.combined.clone(),
        seed_layer: distilled.topic_layer.clone(),
        shape: distilled.shape.clone(),
        fetches_spent: research_out.fetches_spent,
        elapsed_secs: research_out.elapsed_secs,
        seed_decisions: research_out.seed_decisions.clone(),
        sources,
        gaps,
        unreconciled_topics: distilled.unreconciled_topics.clone(),
        forward_assumption_resolution: None,
        forward_assumption: distilled.forward_assumption.clone(),
        leading_indicator: distilled.leading_indicator.clone(),
        forensic_event: distilled.forensic_event.clone(),
    };
    Ok((distilled, record, research_out))
}

/// The Step-6e shadow resolution line (ruled 2026-08-24): the engine evaluated
/// the assumption and computed the hypothetical refinement, but the write-back
/// is parked — this recorded would-have outcome is what the promotion decision
/// reads after manually inspected shadow cases. The standing Step-6b targets
/// are untouched by construction (`engine_output` is immutable past 6b).
fn shadow_assumption_resolution(
    standing_base: Option<f64>,
    refined: &engine::RefinedTargets,
) -> String {
    let would_be = refined.price_targets.twelve_month.as_ref().map(|t| t.base);
    format!(
        "shadow (write-back parked — pending shadow-mode evidence): {}; would have moved the \
         12-month base target {} -> {}",
        refined.matched_rule,
        standing_base.map_or("n/a".to_string(), |v| format!("{v:.2}")),
        would_be.map_or("n/a".to_string(), |v| format!("{v:.2}")),
    )
}

/// The validated fraud claim as one data line under RESEARCH SUMMARY: the
/// document's host, its date, the issuer as the document names it and the
/// address — and that whether it concerns this holding is not established
/// (advisory by the 2026-08-24 ruling, stated in words since `portfolio-v44`).
pub(crate) fn render_fraud_record(claim: &crate::portfolio::distill::ForensicEventClaim) -> String {
    let host = reqwest::Url::parse(claim.source_url.trim())
        .ok()
        .and_then(|u| u.host_str().map(crate::web_research::registry::normalize_host))
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "regulator or court".to_string());
    format!(
        "Fraud record: a {host} document dated {} names {} in a fraud matter; source {}. \
         Whether it concerns this holding is not established.",
        claim.event_date, claim.issuer, claim.source_url
    )
}

/// The validated leading indicator as one data line under RESEARCH SUMMARY;
/// the driver clause names the ledger's driver only where the cited id
/// verified (`portfolio-v44`).
pub(crate) fn render_leading_indicator(
    ind: &crate::portfolio::distill::ValidatedLeadingIndicator,
    prior_ledger: Option<&ThesisLedger>,
) -> String {
    let direction = match ind.direction {
        crate::portfolio::distill::IndicatorDirection::InflectingUp => "inflecting up",
        crate::portfolio::distill::IndicatorDirection::InflectingDown => "inflecting down",
    };
    let driver = ind
        .driver_verified
        .then(|| {
            prior_ledger
                .and_then(|l| {
                    l.key_drivers
                        .iter()
                        .find(|d| d.driver_id == ind.confirms_driver_id.trim())
                })
                .map(|d| d.name.as_str())
                .unwrap_or(ind.confirms_driver.as_str())
        })
        .map(|name| format!(", bearing on the driver \"{name}\""))
        .unwrap_or_default();
    format!(
        "Leading indicator: {} = {} ({direction}, as of {}){driver}; source {}.",
        ind.metric_name, ind.value, ind.as_of, ind.source_url
    )
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
    // The split-adjustment bridge for this run's stored-basis price comparisons
    // against the prior read (`docs/portfolio-analysis.md` §Starting
    // parameters): the prior audit's anchor bar re-read from this run's fresh
    // series is the exact cumulative re-basis factor since the prior pass.
    // `Some(1.0)` — the unchanged common case, and a prior with no anchor (a
    // no-price exit's row: comparisons run as stored until this run stamps one).
    // `None` — an anchor exists but its bar is missing from the fresh window:
    // the basis is unverifiable, so price-denominated prior comparisons are
    // excluded rather than run cross-basis.
    let price_bridge: Option<f64> = match &dossier.prior_authoring_close {
        None => Some(1.0),
        Some(anchor) => engine::split_bridge_factor(&dossier.financials.daily_closes, anchor),
    };
    if price_bridge.is_none() {
        degraded.push(
            "prior split-bridge anchor bar missing from the fresh price window — \
             price-denominated prior comparisons excluded this run"
                .to_string(),
        );
    }
    // The ingested prior ledger converts onto this run's basis ONCE, here, so
    // every machine consumer — evaluation, 6g validation and carry,
    // persistence — sees one basis and the rewrite lands new-basis (the 6c/6d
    // prompts read this same instance but render statements only — no machine
    // core reaches them). Margins scale with thresholds (absolute, same units),
    // so breach semantics are invariant under the conversion. A price
    // condition's statement is re-rendered from the re-based core
    // (`portfolio-v45`), so the sentence names the level the core now carries.
    // The monitor's stamped engine targets convert for render coherence; on a
    // resolvable pass validation re-stamps them from this run's engine set (an
    // unresolvable pass withholds the fresh stamp — absent beats wrong).
    let bridged_ledger: Option<crate::portfolio::ThesisLedger> = match price_bridge {
        Some(f) if f != 1.0 => prior_ledger.map(|l| {
            let mut l = l.clone();
            for cond in &mut l.conditions {
                let rebased = cond.quant.as_mut().is_some_and(|q| {
                    if q.series.price_denominated() {
                        q.threshold *= f;
                        q.margin *= f;
                        true
                    } else {
                        false
                    }
                });
                if rebased {
                    cond.rerender_statement();
                }
            }
            for m in &mut l.monitor {
                if let Some(t) = &mut m.engine_target {
                    *t *= f;
                }
            }
            l
        }),
        _ => None,
    };
    let prior_ledger = bridged_ledger.as_ref().or(prior_ledger);
    // The prior read's per-share comparators on this run's basis: the spot and
    // every raw consensus-EPS period scale TOGETHER (their ratio — the prior
    // matched-period multiple — is basis-free and must stay so). `None` factor
    // drops both — excluded, never cross-basis.
    let (bridged_prior_spot, bridged_prior_periods) = match price_bridge {
        Some(f) => {
            let periods = dossier
                .prior_consensus_eps_periods
                .iter()
                .cloned()
                .map(|mut period| {
                    period.eps_mid = period.eps_mid.map(|mid| mid * f);
                    period
                })
                .collect();
            (dossier.prior_spot.map(|s| s * f), periods)
        }
        None => (None, Vec::new()),
    };
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
    // first-call order. The live analyst drains the routed id of every outbound
    // request; deterministic/custom stubs without call telemetry retain the
    // configured-id fallback. A no-model exit persists none, and duplicate ids
    // collapse without disturbing their first-call position.
    let models_used: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    let used_model = |id: String| {
        let mut used = models_used.borrow_mut();
        if !used.contains(&id) {
            used.push(id);
        }
    };
    // A prior holding can fail after issuing a call but before it produces an
    // audit. Clear that abandoned per-holding telemetry before this holding's
    // first gate; exact telemetry must never leak across rows.
    let exact_model_telemetry = analyst.take_model_calls().is_some();
    let record_stage_models = |fallback: String| {
        if exact_model_telemetry {
            for id in analyst.take_model_calls().unwrap_or_default() {
                used_model(id);
            }
        } else {
            used_model(fallback);
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
    // The split-bridge anchor: the newest settled bar strictly before the run's
    // ET session, off the run's own fetched series (oldest-first). Strictly
    // before keeps the anchor off the run day's still-forming bar, so a
    // re-fetch reads back the identical close unless the series was re-based.
    // A run whose own bridge was unresolvable CARRIES the prior anchor forward
    // instead: the carried price values stay on that anchor's basis (the
    // supersede guard in ledger validation holds the invariant), so provenance
    // is preserved — later passes stay fail-closed while the bar is missing and
    // convert correctly the moment it resolves. A fresh stamp would certify the
    // carried values on a basis this run could not verify (~1.0 next pass, the
    // mismatch never re-detectable); no stamp would fail open the same way.
    let authoring_close = if price_bridge.is_some() {
        dossier
            .financials
            .daily_closes
            .iter()
            .rev()
            .find(|d| d.date.as_str() < run_date)
            .cloned()
    } else {
        dossier.prior_authoring_close.clone()
    };
    let audit = |metrics, target_meta, ledger_audit, pre_profit| HoldingAudit {
        symbol: symbol.clone(),
        metrics,
        sources: audit_sources(),
        model_ids: models_used.borrow().clone(),
        prompt_version: PROMPT_VERSION.to_string(),
        evidence_floor_version: crate::portfolio::engine::EVIDENCE_FLOOR_VERSION.to_string(),
        degraded_inputs: degraded.clone(),
        action_annotations: Vec::new(),
        target_meta,
        grade_parameter_version: engine::GRADE_PARAMETER_VERSION.to_string(),
        ledger_audit,
        quick_basis: None,
        authoring_close: authoring_close.clone(),
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
        // Recorded only where the research loop ran; every no-research exit
        // records none.
        research: None,
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
            Some(crate::portfolio::listing::ListingResolution::UnsupportedUnits { detail }) => {
                Some(format!("unsupported financial units — {detail}"))
            }
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
    // No longer `mut`: the Step-6e assumption recompute runs in shadow mode
    // (ruled 2026-08-24), so nothing rewrites the engine output after 6b.
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
                let fund_metrics = fund_ledger_metrics(&readout, &dossier.financials);
                let ledger_eval = prior_ledger.map(|l| {
                    // The same unverifiable-basis gate as the priced branch:
                    // price-denominated conditions never compare cross-basis.
                    engine::evaluate_ledger_conditions_gated(
                        l,
                        &fund_metrics,
                        &dossier.financials,
                        run_date,
                        |series| price_bridge.is_some() || !series.price_denominated(),
                    )
                });
                // The union's other branch: the model authors the role read only —
                // the branch's action is authored by the dedicated per-holding
                // action call below, the full ladder structurally open while the
                // engine arm's reduced set (sell-all / trim / hold) rides as
                // annotated evidence (`docs/portfolio-analysis.md` §Portfolio
                // action).
                house_view_consulted.set(prompt_renders_house_view(dossier));
                backdrop_consulted.set(dossier.put_call_backdrop.is_some());
                positioning_consulted
                    .set(dossier.fund.as_ref().is_some_and(|f| f.positioning.is_some()));
                // The branch's rendered input delta — the what-changed rows'
                // evidence vocabulary (`docs/portfolio-workflow.md` §Step 6g).
                let mut input_delta = role_risk_input_delta(
                    dossier,
                    &fund_metrics,
                    position_change,
                    ledger_eval.as_ref(),
                    price_bridge,
                );
                // The fund agenda runs the same 6c loop and a
                // pure-consolidation 6d — the stub-time bypass is retired with
                // the research slice (`docs/portfolio-workflow.md` §Step 6d).
                let (rr_distilled, rr_research_record, _rr_research) = run_research_and_distill(
                    analyst,
                    dossier,
                    &research::AgendaTriggers::default(),
                    prior_ledger,
                    true,
                    run_date,
                )?;
                record_stage_models(analyst.fast_id());
                // The fund research's fresh claims join the rendered delta with
                // their ledger ties, as on the priced path.
                push_research_delta_entries(&mut input_delta, &rr_distilled, prior_ledger);
                let interpretation = analyst
                    .interpret_role_risk(&RoleRiskInput {
                        dossier,
                        readout: &readout,
                        prior_ledger,
                        ledger_eval: ledger_eval.as_ref(),
                        input_delta: &input_delta,
                        distilled: &rr_distilled.combined,
                    })
                    .context("interpreting the role/risk holding")?;
                let interpretation = own_debut_continuity_role_risk(
                    interpretation,
                    dossier.prior_verdict.is_none(),
                );
                record_stage_models(analyst.reasoner_id());
                // The 6g what-changed attribution validator — external claims
                // resolve against the rendered delta or downgrade to
                // self-correction; a debut records no audit.
                let what_changed_audit = dossier.prior_verdict.is_some().then(|| {
                    validate_what_changed(&interpretation.what_changed_entries, input_delta)
                });
                // The 6g ledger seam: validate the rewrite — executability,
                // condition identity / carry, tripped / fired claims, the branch's
                // reductions (condition-only monitor, trim / sell triggers). The
                // fund research's fresh claims carry the source-backed leg.
                let research_supported: std::collections::HashSet<String> = rr_distilled
                    .topic_layer
                    .iter()
                    .flat_map(|t| t.claims.iter())
                    .filter(|c| !c.cached)
                    .filter_map(|c| c.related_condition_id.clone())
                    .collect();
                let (ledger, ledger_audit) = validate_ledger_rewrite_with_research(
                    &interpretation.ledger,
                    prior_ledger,
                    ledger_eval.as_ref(),
                    LedgerBranch::RoleRiskOnly,
                    is_fund,
                    None,
                    dossier.financials.current_price,
                    Some(&fund_metrics),
                    &research_supported,
                    price_bridge.is_some(),
                    crate::portfolio::ContinuityStamps::of(&dossier.financials),
                );
                // The action placeholder is overwritten by the decision below and
                // never rendered into its prompt.
                let mut rr = role_risk_verdict_from_interpretation(&readout, interpretation);
                let decision = analyst
                    .decide_action(&ActionInput {
                        dossier,
                        subject: ActionSubject::RoleRisk { verdict: &rr, ledger: &ledger },
                        engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
                        profile: &dossier.profile,
                        changes: what_changed_audit.as_ref(),
                    })
                    .context("deciding the role/risk holding's action")?;
                record_stage_models(analyst.reasoner_id());
                ensure_action_rationale(&symbol, &decision)?;
                rr.action = decision.action;
                rr.action_rationale = with_tax_caveat(
                    decision.rationale,
                    &dossier.profile,
                    &dossier.position,
                    decision.action,
                );
                // The branch's computed surface persists as the audit's metrics — the
                // same expense-ratio + price-derived legs the ledger evaluation above
                // read (plus the CEF-only closed-end read), never the empty default
                // (M3 of the 2026-08-18 audit).
                let mut audit_record = audit(fund_metrics, None, Some(ledger_audit), None);
                audit_record.what_changed_audit = what_changed_audit;
                audit_record.research = Some(rr_research_record);
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
        // The pre-profit overlay's statement leg over the carried observation
        // history (`docs/portfolio-workflow.md` §Step 6b) — no candidate rows
        // exist yet at this seam; the research-fed rows arrive at the Step-6e
        // finalization below, which recomputes the overlay whole. Computed for
        // every stock: the eligibility result persists even when the stock
        // does not enter.
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
    // (`docs/portfolio-analysis.md` §The position thesis ledger). An unverifiable
    // price basis gates price-denominated conditions out whole — never a
    // cross-basis comparison (the degraded input above records it).
    let ledger_eval = prior_ledger.map(|l| {
        engine::evaluate_ledger_conditions_gated(
            l,
            &engine_output.metrics,
            &dossier.financials,
            run_date,
            |series| price_bridge.is_some() || !series.price_denominated(),
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
            bridged_prior_spot,
            &bridged_prior_periods,
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
    // Research (the live 6c loop, or the analyst's offline default) → distill
    // → interpret.
    house_view_consulted.set(prompt_renders_house_view(dossier));
    backdrop_consulted.set(dossier.put_call_backdrop.is_some());
    positioning_consulted.set(dossier.fund.as_ref().is_some_and(|f| f.positioning.is_some()));
    commodity_consulted.set(!dossier.commodity_context.is_empty());
    short_interest_consulted.set(dossier.short_interest.is_some());
    // The conditional topics' deterministic triggers (`docs/portfolio-workflow.md`
    // §Step 6c). The symbol-scoped news seeds are no trigger here: a
    // qualifying seed is fresh news beside a standing technology falsifier,
    // and the falsifier fires the topic on its own — the seeds reach the loop
    // as leads in the pass brief (retired 2026-08-29, Codex I15; the quick
    // check's news leg is distinct and reads the conjunction as its badge).
    let triggers = research::AgendaTriggers {
        tech_pre_flag_fired: tech_pre_flag.as_ref().is_some_and(|f| f.fired),
        tech_ledger_falsifier: research::ledger_has_technology_falsifier(prior_ledger),
        overlay_eligible: pre_profit_overlay.as_ref().is_some_and(|o| o.is_eligible()),
        // The backfill obligation binds on the first overlay-eligible full
        // pass, or while a previously used guidance metric-and-span identity
        // has fewer than four comparable stored periods
        // (`docs/portfolio-analysis.md` §Starting parameters).
        pre_profit_backfill: pre_profit_overlay
            .as_ref()
            .filter(|o| o.is_eligible())
            .is_some_and(|o| pre_profit::backfill_required(o, dossier.prior_pre_profit.as_ref())),
    };
    let (distilled_research, mut research_record, research_out) = run_research_and_distill(
        analyst,
        dossier,
        &triggers,
        prior_ledger,
        false,
        run_date,
    )?;
    record_stage_models(analyst.fast_id());
    let distilled = distilled_research.combined.clone();

    // Step 6e — the observation-driven overlay finalization
    // (`docs/portfolio-workflow.md` §Step 6e): the research-fed typed rows are
    // validated with the two activation legs over the loop's fetched pages
    // (holding identity + source-text corroboration — the discharged
    // obligation), merged into the period-end-and-span-keyed history, and the overlay
    // recomputed whole; the backfill attempt's record joins where the agenda
    // required one.
    if pre_profit_overlay.as_ref().is_some_and(pre_profit::PreProfitOverlay::is_eligible)
        && (!distilled_research.pre_profit_observations.is_empty()
            || distilled_research.backfill.is_some())
    {
        let evidence = pre_profit::SourceEvidence {
            texts: &research_out.page_texts,
            symbol: &symbol,
            company_name: dossier.company_name.as_deref(),
        };
        let mut refined = pre_profit::compute_overlay_with_sources(
            &dossier.financials,
            dossier.prior_pre_profit.as_ref(),
            distilled_research.pre_profit_observations.clone(),
            Some(&evidence),
        );
        if let Some(backfill) = distilled_research.backfill.clone() {
            refined.backfill_attempts.push(backfill);
        }
        pre_profit_overlay = Some(refined);
    }
    if triggers.pre_profit_backfill && distilled_research.backfill.is_none() {
        // The obligation was to search; an attempt that never reported stays a
        // recorded gap, never an inferred observation.
        research_record
            .gaps
            .push("pre-profit backfill required but no attempt was reported".to_string());
    }

    // Step 6e — the forward-assumption target recompute runs in **shadow
    // mode** (ruled 2026-08-24): the engine still evaluates the claim under
    // the app-owned conflict policy and computes the hypothetical refined
    // targets, but the result is **never spliced into the baseline** — the
    // mechanical legs cannot verify that the number is semantically the
    // claimed forward driver, so the write-back is parked and the recorded
    // would-have outcome is the evidence the promotion decision reads after
    // manually inspected shadow cases. Every resolution — the shadow
    // would-have line or the failed condition — records on the audit; the
    // structured Step-6b targets always stand.
    if let Some(assumption) = &distilled_research.forward_assumption {
        let affects = format!(
            "{} {}",
            assumption.affects.to_ascii_lowercase(),
            assumption.fact_type.to_ascii_lowercase()
        );
        let metric = if affects.contains("eps") || affects.contains("earnings") {
            Some(engine::AssumptionMetric::ForwardEps)
        } else if affects.contains("revenue") || affects.contains("sales") {
            Some(engine::AssumptionMetric::ForwardRevenue)
        } else {
            None
        };
        let resolution = match metric {
            None => format!(
                "rejected: {:?} maps to no recomputable driver (drafted mapping: EPS / revenue)",
                assumption.affects
            ),
            Some(metric) => {
                let input = engine::ForwardAssumptionInput {
                    metric,
                    value: assumption.numeric_value,
                    units: assumption.units.clone(),
                    // The model declares no conflict handling since
                    // `portfolio-v44` (ruled 2026-09-17: with a feed value
                    // present both declarations rejected, without one both
                    // filled): every fact reads as a supplement fill under
                    // the app-owned policy.
                    supersede: false,
                    fact_type: assumption.fact_type.clone(),
                    as_of: assumption.as_of.clone(),
                    source_url: assumption.source_url.clone(),
                };
                match engine::refine_targets_with_assumption(&dossier.financials, rates, &input) {
                    Ok(refined) => shadow_assumption_resolution(
                        engine_output
                            .price_targets
                            .twelve_month
                            .as_ref()
                            .map(|t| t.base),
                        &refined,
                    ),
                    Err(condition) => condition,
                }
            }
        };
        research_record.forward_assumption_resolution = Some(resolution);
    }

    // The research-fed fraud claim is **advisory** (ruled 2026-08-24): the
    // deterministic legs establish provenance and relevance, not that the
    // issuer is the accused party, so the claim never joins the hard-forensic
    // producer state — the hard rule trips from the item-classified filing
    // kinds alone. The validated claim rides the audit record and reaches the
    // model as cited attention evidence below; promotion back to a hard
    // trigger waits on explicit acknowledgment or a source-specific adapter
    // that reads the accused party from structured document fields.
    let filing_state = dossier.filing_events.clone();
    let hard_forensic = filing_state
        .as_ref()
        .map(crate::portfolio::ForensicFilingState::hard_tripped)
        .unwrap_or(false);
    // The narrative soft ceiling's **anchor exception** joins with the loop
    // (`docs/portfolio-analysis.md` §Starting parameters — the cap fired on
    // the ratio alone while every holding read anchor-absent): a validated
    // leading indicator whose **driver reference verified** against the prior
    // ledger's app-assigned driver ids is the leading-metric anchor, so the
    // engine-arm ceiling is suppressed, the suppression annotated on the read
    // itself (ruled 2026-08-24: referential integrity gates the exception — an
    // indicator with a missing or stale driver id stays visible evidence but
    // never suppresses the cap).
    let mut narrative = narrative;
    let anchor_verified = distilled_research
        .leading_indicator
        .as_ref()
        .is_some_and(|l| l.driver_verified);
    let narrative_hype = narrative.as_ref().is_some_and(|n| n.hype_capped()) && !anchor_verified;
    if let Some(read) = narrative
        .as_mut()
        .filter(|n| n.hype_capped() && anchor_verified)
    {
        if let Some(rule) = read.matched_rule.take() {
            read.matched_rule = Some(format!(
                "{rule} — ceiling suppressed: validated leading-indicator anchor present \
                 (driver reference verified)"
            ));
        }
    }
    // The typed indicator reaches the model as evidence on a driver it names
    // (its conviction-raise role is retired suite-wide with `portfolio-v7`):
    // one data line under RESEARCH SUMMARY, the driver clause only where the
    // reference verified against the ledger (`portfolio-v44`).
    let distilled = match &distilled_research.leading_indicator {
        Some(ind) => format!("{distilled}\n\n{}", render_leading_indicator(ind, prior_ledger)),
        None => distilled,
    };
    // The advisory fraud claim reaches the model as a data line that states
    // its attribution to this holding as not established (the 2026-08-24
    // ruling, in words): it is not a hard trigger and binds nothing.
    let distilled = match &distilled_research.forensic_event {
        Some(claim) => format!("{distilled}\n\n{}", render_fraud_record(claim)),
        None => distilled,
    };

    // The overlay's rules join only when the stock actually entered the overlay
    // (a priced fund carries none) — they bind the engine arm's stand-in and the
    // engine's per-holding action set below. Derived after the Step-6e
    // finalization so a research-fed execution/severe state binds this run.
    let overlay_rules = pre_profit_overlay
        .as_ref()
        .filter(|o| o.is_eligible())
        .map(|o| &o.consequences);
    // The rendered input delta — the what-changed rows' evidence vocabulary
    // (`docs/portfolio-workflow.md` §Step 6g); empty on a debut.
    let mut input_delta = priced_input_delta(
        dossier,
        &engine_output,
        position_change,
        ledger_eval.as_ref(),
        tech_pre_flag.as_ref(),
        narrative.as_ref(),
        hard_forensic,
        price_bridge,
    );
    // The research evidence joins the rendered delta surface — the 6g
    // research-finding and forward-assumption legs (`docs/portfolio-workflow.md`
    // §Step 6g): each fresh distilled claim an addressable entry, the logged
    // assumption its own, so an external what-changed row can cite them
    // exactly like any engine entry.
    push_research_delta_entries(&mut input_delta, &distilled_research, prior_ledger);
    if let Some(a) = &distilled_research.forward_assumption {
        input_delta.push(crate::portfolio::DeltaEntry {
            id: "forward-assumption".to_string(),
            label: format!(
                "research forward assumption: {} = {} {} (as of {}) [{}]",
                a.affects, a.numeric_value, a.units, a.as_of, a.source_url
            ),
            related_condition_id: None,
        });
    }
    let interpretation = analyst
        .interpret(&InterpretationInput {
            dossier,
            engine: &engine_output,
            distilled: &distilled,
            prior_ledger,
            ledger_eval: ledger_eval.as_ref(),
            pre_profit: pre_profit_overlay.as_ref().filter(|o| o.is_eligible()),
            tech_pre_flag: tech_pre_flag.as_ref(),
            narrative: narrative.as_ref(),
            input_delta: &input_delta,
        })
        .context("interpreting the holding")?;
    let interpretation = own_debut_continuity(interpretation, dossier.prior_verdict.is_none());
    record_stage_models(analyst.reasoner_id());
    // The 6g what-changed attribution validator — every external row resolves
    // against the rendered delta or downgrades to self-correction with a logged
    // reason; a debut records no audit.
    let what_changed_audit = dossier
        .prior_verdict
        .is_some()
        .then(|| validate_what_changed(&interpretation.what_changed_entries, input_delta));
    // The 6g ledger seam: validate the rewrite and stamp the engine's scenario
    // targets into the monitor (app-owns-the-number — a model-written target never
    // persists). The research-supported ids carry the source-backed-finding
    // leg for qualitative tripped/fired claims.
    let research_supported: std::collections::HashSet<String> = distilled_research
        .topic_layer
        .iter()
        .flat_map(|t| t.claims.iter())
        .filter(|c| !c.cached)
        .filter_map(|c| c.related_condition_id.clone())
        .collect();
    let (ledger, ledger_audit) = validate_ledger_rewrite_with_research(
        &interpretation.ledger,
        prior_ledger,
        ledger_eval.as_ref(),
        LedgerBranch::Priced,
        is_fund,
        // Fresh-basis engine targets never stamp beneath a carried anchor: an
        // unresolvable pass stamps `None` (the band read goes absent, not
        // wrong) — same absent-beats-wrong rule as the quick basis below.
        engine_output
            .price_targets
            .twelve_month
            .as_ref()
            .filter(|_| price_bridge.is_some()),
        dossier.financials.current_price,
        Some(&engine_output.metrics),
        &research_supported,
        price_bridge.is_some(),
        crate::portfolio::ContinuityStamps::of(&dossier.financials),
    );

    // The engine stand-in arm — mechanical outlook / conviction / action baselines
    // beside the model's (`docs/portfolio-analysis.md` §The holding verdict).
    let engine_view =
        engine::engine_view(&engine_output, &dossier.financials, &degraded, overlay_rules, hard_forensic, narrative_hype);
    let mut graded = graded_verdict_from_interpretation(
        &engine_output,
        dossier.options_signal.clone(),
        interpretation,
        engine_view,
    );
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
                ledger: &ledger,
            },
            engine_set: &engine_set,
            profile: &dossier.profile,
            changes: what_changed_audit.as_ref(),
        })
        .context("deciding the holding's action")?;
    record_stage_models(analyst.reasoner_id());
    ensure_action_rationale(&symbol, &decision)?;
    graded.action = decision.action;
    graded.action_rationale = with_tax_caveat(
        decision.rationale,
        &dossier.profile,
        &dossier.position,
        decision.action,
    );
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
        evidence_floor_version: crate::portfolio::engine::EVIDENCE_FLOOR_VERSION.to_string(),
        degraded_inputs,
        action_annotations: outside_set_annotation(decision.action, &engine_set)
            .into_iter()
            .collect(),
        target_meta: Some(engine_output.target_meta.clone()),
        grade_parameter_version: engine::GRADE_PARAMETER_VERSION.to_string(),
        ledger_audit: Some(ledger_audit),
        // An unresolvable pass persists NO quick-check basis: the row's anchor
        // is the carried prior-basis one, and a fresh-basis spot/consensus
        // beneath it would double-convert the moment the anchor resolves
        // (fabricated revision events, mis-scaled multiples and hurdle reads).
        // Absent beats wrong — the sweep's rate-anchor family reads its typed
        // no-stored-basis state until a resolvable pass re-persists.
        quick_basis: if price_bridge.is_some() {
            engine_output.quick_basis.clone()
        } else {
            None
        },
        authoring_close: authoring_close.clone(),
        fund_exposure: fund_exposure.clone(),
        pre_profit: pre_profit_overlay,
        // The full hurdle read persists so a decision episode's calibration
        // snapshot can freeze the hurdle inputs (`docs/portfolio-analysis.md`
        // §Outcome learning).
        hurdle: Some(engine_output.hurdle.clone()),
        forensic: filing_state
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
        research: Some(research_record),
    };
    Ok((verdict, audit_record))
}

// ---- Thesis-ledger rewrite validation (the 6g seam) ----------------------------

/// The reason classes a claimed-quantitative condition downgrades under at the 6g
/// seam (`docs/portfolio-workflow.md` §Step 6g). Every persisted
/// `downgraded_reason` opens with its class and a colon, so a consumer or a test
/// reads the class by prefix while the persisted shape stays a `String` (ruled
/// 2026-09-16). Since `portfolio-v45` the statement is rendered from the core,
/// so no class reads prose: the structural classes and the authoring-surface
/// class remain.
pub mod downgrade_class {
    /// The series claim does not resolve to a series the engine computes.
    pub const SERIES_UNRESOLVED: &str = "series-unresolved";
    /// The series resolves but the holding's vehicle kind never computes it.
    pub const SERIES_UNCOMPUTABLE: &str = "series-uncomputable";
    /// The comparator or a number is malformed.
    pub const MALFORMED: &str = "malformed";
    /// A margin at or beyond the threshold's magnitude, or past the relative cap.
    pub const MARGIN: &str = "margin-implausible";
    /// A new or superseding core that already holds on the authoring surface —
    /// the value the prompt showed is already past the threshold by more than
    /// the margin — so it is not a crossing ahead but a reversed direction, a
    /// unit slip or a present-tense claim (the rendered-ledger slice, ruled
    /// 2026-09-18).
    pub const HOLDS_AT_AUTHORING: &str = "holds-at-authoring";
}

/// The relative margin cap on the price, the multiples and the debt / equity
/// ratio: a margin past this share of the level is implausible even when it
/// clears the magnitude bound (fix list 1.8, ruled 2026-09-16 off the live
/// read's L5 — a 489.5 margin on a $490 level).
const MARGIN_CAP_PRICE_RATIO: f64 = 0.25;
/// The relative margin cap on the fraction-unit series, whose levels sit near
/// zero so their noise band is proportionally wider.
const MARGIN_CAP_FRACTION: f64 = 0.50;

/// Parse a draft's core — the series (and, with `vehicle` given, that the
/// vehicle kind computes it), the comparator, a finite threshold and a
/// non-negative margin — with the structural reason where it does not parse,
/// opening with its [`downgrade_class`]. A refused draft's statement renders
/// from this parse alone (no vehicle check), so a series the vehicle never
/// computes still renders what was asked.
fn parse_draft_core(qd: &QuantCoreDraft, vehicle: Option<bool>) -> std::result::Result<QuantCore, String> {
    use downgrade_class as class;
    let series = engine::LedgerSeries::parse(&qd.series).ok_or_else(|| {
        format!(
            "{}: series '{}' does not resolve to a series the engine computes",
            class::SERIES_UNRESOLVED,
            qd.series
        )
    })?;
    if let Some(is_fund) = vehicle {
        if !series.computable_for(is_fund) {
            return Err(format!(
                "{}: series '{}' has no {} computation — the condition would be \
                 permanently unevaluable on this holding",
                class::SERIES_UNCOMPUTABLE,
                qd.series,
                if is_fund { "fund-path" } else { "stock-path" }
            ));
        }
    }
    let comparator = match qd.comparator.trim() {
        "below" => LedgerComparator::Below,
        "above" => LedgerComparator::Above,
        other => {
            return Err(format!(
                "{}: comparator '{other}' is not below/above",
                class::MALFORMED
            ))
        }
    };
    if !qd.threshold.is_finite() {
        return Err(format!("{}: threshold is not a finite number", class::MALFORMED));
    }
    Ok(QuantCore {
        series,
        comparator,
        threshold: qd.threshold,
        margin: if qd.margin.is_finite() { qd.margin.max(0.0) } else { 0.0 },
    })
}

/// Validate a draft's quantitative-core claim into a persisted [`QuantCore`] — the
/// resolution contract's app-side check (`docs/portfolio-workflow.md` §Step 6g).
/// `Err` carries the downgrade reason, opening with its [`downgrade_class`]. The
/// checks run in a fixed order and the first disagreement wins: series →
/// vehicle → comparator → threshold → margin. A core that fails is downgraded
/// to qualitative, never repaired or clamped toward the engine's view. The
/// authoring-surface check ([`holds_at_authoring`]) runs at the condition seam,
/// where a carried core is told apart from a new one.
fn validate_quant_core(qd: &QuantCoreDraft, is_fund: bool) -> std::result::Result<QuantCore, String> {
    use downgrade_class as class;
    let core = parse_draft_core(qd, Some(is_fund))?;
    let QuantCore { series, comparator, threshold, margin } = core;

    // Margin: at or beyond the threshold's magnitude moves the effective boundary
    // to zero or past double the level (a zero threshold is exempt — the margin
    // is its only scale).
    if threshold != 0.0 && margin >= threshold.abs() {
        return Err(format!(
            "{}: margin {} is at or beyond the threshold's magnitude {} — the effective \
             boundary would sit at {} rather than near the stated level",
            class::MARGIN,
            fmt_num(margin),
            fmt_num(threshold.abs()),
            fmt_num(match comparator {
                LedgerComparator::Below => threshold - margin,
                LedgerComparator::Above => threshold + margin,
            })
        ));
    }
    let cap = if series.percent_unit() { MARGIN_CAP_FRACTION } else { MARGIN_CAP_PRICE_RATIO };
    if threshold != 0.0 && margin > cap * threshold.abs() {
        return Err(format!(
            "{}: margin {} is more than {}% of the level {} — a noise band that wide would \
             confirm a crossing far from the stated level",
            class::MARGIN,
            fmt_num(margin),
            fmt_num(cap * 100.0),
            fmt_num(threshold.abs())
        ));
    }

    Ok(core)
}

/// The authoring-surface check: the value the prompt showed for the core's
/// series — the run's computed metric, or the spot on the price — read through
/// the evaluator's own predicate (`engine::evaluate_ledger_conditions_gated`:
/// past the threshold by more than the margin). `Some(value)` where the core
/// already holds, so a new or superseding condition is refused as
/// `holds-at-authoring`; `None` where it does not, or where the surface carries
/// no value for the series — or a value the evaluator would call off-scale
/// ([`engine::LedgerSeries::admissible`]: negative equity, a non-positive P/E)
/// — since the condition is then unevaluable rather than wrong, and keeps its
/// core (ruled 2026-09-18).
fn holds_at_authoring(
    core: &QuantCore,
    metrics: Option<&engine::ComputedMetrics>,
    spot: Option<f64>,
) -> Option<f64> {
    let value = if core.series == engine::LedgerSeries::Price {
        engine::usable_price(spot)
    } else {
        metrics
            .and_then(|m| core.series.metric_value(m))
            .filter(|v| v.is_finite() && core.series.admissible(*v))
    }?;
    let margin = core.margin.max(0.0);
    let holds = match core.comparator {
        LedgerComparator::Below => value < core.threshold - margin,
        LedgerComparator::Above => value > core.threshold + margin,
    };
    holds.then_some(value)
}

/// The data phrase the continuity prompt prints on a refused condition's row,
/// per reason class — what the model needs to author differently, with no app
/// word: the `holds-at-authoring` reason is already that sentence; each
/// structural class maps to one phrase (ruled 2026-09-18).
fn refusal_phrase(reason: &str) -> String {
    let (class, rest) = reason.split_once(':').unwrap_or(("", reason));
    match class {
        downgrade_class::HOLDS_AT_AUTHORING => rest.trim().to_string(),
        downgrade_class::SERIES_UNRESOLVED | downgrade_class::SERIES_UNCOMPUTABLE => {
            "no computed series for this holding".to_string()
        }
        downgrade_class::MALFORMED => "the comparator or a number did not parse".to_string(),
        downgrade_class::MARGIN => "the margin is too wide for the level".to_string(),
        _ => "the price basis could not be tied to the prior analysis this run".to_string(),
    }
}

/// The statement a refused draft renders where its core never parsed (an
/// unresolvable series, a malformed comparator or threshold): the draft's own
/// words and figures behind the model's name, in the series' formats where the
/// series parses, so the refused condition still says what was asked.
fn render_unparsed_draft(qd: &QuantCoreDraft, label: &str) -> String {
    let series = engine::LedgerSeries::parse(&qd.series);
    let level = match series {
        Some(s) if qd.threshold.is_finite() => s.render_level(qd.threshold),
        _ => fmt_num(qd.threshold),
    };
    let margin = match series {
        Some(s) if qd.margin.is_finite() => s.render_margin(qd.margin),
        _ => fmt_num(qd.margin),
    };
    let rule = format!(
        "{} {} {level} (margin {margin})",
        series.map(|s| s.render_name()).unwrap_or(qd.series.trim()),
        qd.comparator.trim()
    );
    if label.is_empty() {
        rule
    } else {
        format!("{label} — {rule}")
    }
}

/// A compact number render for the downgrade reasons (no trailing zeros).
fn fmt_num(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-" { "0".to_string() } else { s }
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
/// (downgrade-not-drop), the authoring-surface check, app-decided identity
/// (carry / supersede / new), and the tripped / fired claim (honored only against
/// a confirmed engine crossing on the carried id — `docs/portfolio-workflow.md`
/// §Step 6g). A quantitative draft's `statement` is the model's short name; the
/// persisted statement is rendered from the core, kept or refused
/// (`portfolio-v45`).
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
    research_supported: &std::collections::HashSet<String>,
    updated_states: &std::collections::HashMap<String, ConditionEvalState>,
    price_basis_verified: bool,
    stamps: crate::portfolio::ContinuityStamps,
    metrics: Option<&engine::ComputedMetrics>,
    spot: Option<f64>,
    audit: &mut LedgerAudit,
) -> LedgerCondition {
    let text = statement.trim().to_string();
    // The model's name for a quantitative condition; a blank one persists as none.
    let name = (!text.is_empty()).then(|| text.clone());
    let (statement, label, quant, downgraded_reason) = match quant_draft {
        None => (text, None, None, None),
        Some(qd) => match validate_quant_core(qd, is_fund) {
            Ok(core) => {
                let carried_verbatim = prior_pool.iter().any(|c| {
                    c.role == role
                        && c.trigger_family == trigger_family
                        && c.quant.as_ref() == Some(&core)
                });
                let statement = core.render(name.as_deref(), stamps.statement_basis);
                if !price_basis_verified && core.series.price_denominated() && !carried_verbatim {
                    // The unverifiable-basis supersede guard: with the split bridge
                    // unresolvable, a NEW or RE-ANCHORED price-denominated core was
                    // authored against fresh prices but would persist under the
                    // carried prior-basis anchor — an untieable mix, so it
                    // downgrades (typed, never dropped). A carried-verbatim core
                    // stays quantitative: it shares the carried anchor's basis.
                    let reason = "the price basis is unverifiable this run \
                                  (split-bridge anchor unresolvable) — a new or \
                                  re-anchored price-denominated core cannot be \
                                  tied to the carried anchor; re-author at a \
                                  resolvable pass"
                        .to_string();
                    audit.downgraded.push(format!("'{statement}': {reason}"));
                    (statement, name, None, Some(reason))
                } else if let Some(value) = (!carried_verbatim)
                    .then(|| holds_at_authoring(&core, metrics, spot))
                    .flatten()
                {
                    // The authoring-surface check: a carried-verbatim core was
                    // authored earlier and may legitimately be in breach now (its
                    // streak is the point); a new or superseding one that already
                    // holds is not a crossing ahead.
                    // The reason after its class is a data sentence: the
                    // continuity prompt prints it on the refused row.
                    let reason = format!(
                        "{}: the {} was already {} {} when authored, past the margin {}; \
                         it stood at {}",
                        downgrade_class::HOLDS_AT_AUTHORING,
                        core.series.render_name(),
                        core.comparator.as_kebab(),
                        core.series.render_level(core.threshold),
                        core.series.render_margin(core.margin),
                        core.series.render_level(value)
                    );
                    audit.downgraded.push(format!("'{statement}': {reason}"));
                    (statement, name, None, Some(reason))
                } else {
                    (statement, name, Some(core), None)
                }
            }
            Err(reason) => {
                // Downgraded to qualitative, logged, never dropped — and it retains
                // no machine evaluation state. The statement still renders from the
                // draft as far as it parses, so the refused condition says what the
                // model asked.
                let statement = parse_draft_core(qd, None)
                    .map(|c| c.render(name.as_deref(), stamps.statement_basis))
                    .unwrap_or_else(|_| render_unparsed_draft(qd, name.as_deref().unwrap_or("")));
                audit.downgraded.push(format!("'{statement}': {reason}"));
                (statement, name, None, Some(reason))
            }
        },
    };

    let (condition_id, supersedes, eval_state, carried) = match &quant {
        Some(core) => {
            if let Some(prev) = take_exact_core(prior_pool, role, trigger_family, core) {
                // Unchanged machine core: the id and accumulated state carry
                // through any re-naming.
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
                // A fresh streak starts stamped with the basis and source the
                // prompt stated for this series (`ContinuityStamps`), so the
                // first evaluation can already disagree with a flip.
                (
                    new_id,
                    Some(prev_id),
                    Some(stamps.authored_state(core.series)),
                    false,
                )
            } else {
                (
                    uuid::Uuid::new_v4().to_string(),
                    None,
                    Some(stamps.authored_state(core.series)),
                    false,
                )
            }
        }
        None => {
            // Qualitative (authored or downgraded): carry the id on an unchanged
            // statement — a refused draft re-emitted unchanged renders the same
            // sentence; no machine evaluation state either way.
            match take_same_statement(prior_pool, role, trigger_family, &statement) {
                Some(prev) => (prev.condition_id, prev.supersedes, None, true),
                None => (uuid::Uuid::new_v4().to_string(), None, None, false),
            }
        }
    };

    // The tripped / fired claim: a quantitative claim is honored only where the
    // engine confirmed a crossing for this same (carried) condition; a
    // qualitative claim only where a **source-backed research finding**
    // references the carried condition — the distillation's validated
    // `related_condition_id` linkage, fresh claims only
    // (`docs/portfolio-workflow.md` §Step 6g). Anything else is cleared and
    // logged: the ledger cannot be quietly rewritten to fit a new verdict.
    let tripped = if claimed {
        let honored = if quant.is_some() {
            carried && confirmed_ids.contains(&condition_id)
        } else {
            carried && research_supported.contains(&condition_id)
        };
        if honored {
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
        label,
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
    validate_ledger_rewrite_with_research(
        draft,
        prior,
        evaluation,
        branch,
        is_fund,
        engine_targets,
        spot,
        None,
        &std::collections::HashSet::new(),
        true,
        crate::portfolio::ContinuityStamps::NONE,
    )
}

/// The full 6g form: `research_supported` carries the condition ids that a
/// **fresh** distilled research claim references (the validated
/// `related_condition_id` linkage) — the source-backed-finding leg a
/// qualitative tripped/fired claim needs. The research-less
/// [`validate_ledger_rewrite`] passes the empty set, so a qualitative claim
/// can never self-certify. `stamps` is the authoring surface's continuity
/// stamps — the basis and equity source the prompt stated — written onto every
/// new or superseding quantitative condition per series; `metrics` is the
/// authoring surface the prompt showed, read by the `holds-at-authoring` check
/// (the price reads the spot)
/// ([`crate::portfolio::ContinuityStamps`]); the wrapper passes none.
#[allow(clippy::too_many_arguments)]
pub fn validate_ledger_rewrite_with_research(
    draft: &LedgerDraft,
    prior: Option<&ThesisLedger>,
    evaluation: Option<&LedgerEvaluation>,
    branch: LedgerBranch,
    is_fund: bool,
    engine_targets: Option<&PriceTarget>,
    spot: Option<f64>,
    metrics: Option<&engine::ComputedMetrics>,
    research_supported: &std::collections::HashSet<String>,
    price_basis_verified: bool,
    stamps: crate::portfolio::ContinuityStamps,
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
        match quant_draft.and_then(|qd| validate_quant_core(qd, is_fund).ok()) {
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
        let Some(core) = quant_draft.and_then(|qd| validate_quant_core(qd, is_fund).ok()) else {
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
            research_supported,
            &updated_states,
            price_basis_verified,
            stamps,
            metrics,
            spot,
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
            research_supported,
            &updated_states,
            price_basis_verified,
            stamps,
            metrics,
            spot,
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
    // (unresolvable → the driver keeps its name, untied, logged). Each driver
    // gets an **app-assigned stable `driver_id`** (ruled 2026-08-24): a
    // rewritten driver whose name carries (trimmed, case-insensitive) keeps
    // the prior driver's id — the referential anchor the next run's leading
    // indicator must cite — while a new or renamed driver mints a fresh one
    // (a changed statement is a different driver).
    let mut prior_driver_pool: Vec<&KeyDriver> = prior
        .map(|p| p.key_drivers.iter().collect())
        .unwrap_or_default();
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
            let name = d.name.trim().to_string();
            let carried = prior_driver_pool
                .iter()
                .position(|p| !p.driver_id.is_empty() && p.name.trim().eq_ignore_ascii_case(&name))
                .map(|i| prior_driver_pool.swap_remove(i).driver_id.clone());
            KeyDriver {
                driver_id: carried.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                name,
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
        "\nPRIOR ANALYSIS NOTES (from memory; context, not fresh evidence)\n",
    );
    for h in &d.semantic_recall.hits {
        s.push_str(&format!("- {h}\n"));
    }
    s
}

// ---- The what-changed attribution (the metric-level 6g validator) ----------------

/// The decimal floor for one comparison value. A nonzero value extends past
/// `min_places` rather than rendering as zero; a still-smaller value falls back
/// to its shortest round-trip representation in [`comparison_safe_pair`] or
/// [`delta_value`].
fn comparison_places(x: f64, min_places: usize) -> usize {
    if x == 0.0 {
        min_places
    } else {
        (min_places..=10)
            .find(|p| (x * 10f64.powi(*p as i32)).round() != 0.0)
            .unwrap_or(10)
    }
}

/// Render two values at one shared precision while preserving their numeric
/// ordering when the strings are read back. Input-delta rows use different
/// presentation floors (spot 2, metrics 4, sub-scores 0), but none may turn an
/// exact `old != new` into a displayed equality. Values too close to distinguish
/// at ten places use their shortest round-trip representations.
fn comparison_safe_pair(old: f64, new: f64, min_places: usize) -> (String, String) {
    let old = if old == 0.0 { 0.0 } else { old };
    let new = if new == 0.0 { 0.0 } else { new };
    let order = old.partial_cmp(&new);
    let faithful = |a: &str, b: &str| match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(a), Ok(b)) => a.partial_cmp(&b) == order,
        _ => false,
    };
    let floor = comparison_places(old, min_places).max(comparison_places(new, min_places));
    let render = |places: usize| (format!("{old:.places$}"), format!("{new:.places$}"));
    (floor..=10)
        .map(render)
        .find(|(a, b)| faithful(a, b))
        .unwrap_or_else(|| (format!("{old}"), format!("{new}")))
}

/// Format one side of an optional input-delta pair — `(absent)` where that run
/// could not compute the metric, and never a fabricated zero for a tiny value.
fn delta_value(v: Option<f64>, min_places: usize) -> String {
    let Some(x) = v else {
        return "(absent)".to_string();
    };
    let x = if x == 0.0 { 0.0 } else { x };
    let places = comparison_places(x, min_places);
    let rendered = format!("{x:.places$}");
    if x != 0.0 && rendered.parse::<f64>() == Ok(0.0) {
        format!("{x}")
    } else {
        rendered
    }
}

/// Render an optional old/new pair. Two present values share comparison-safe
/// precision; an absent side stays explicit and the present side keeps the
/// requested presentation floor without flattening a tiny nonzero value.
fn optional_delta_pair(
    old: Option<f64>,
    new: Option<f64>,
    min_places: usize,
) -> (String, String) {
    match (old, new) {
        (Some(old), Some(new)) => comparison_safe_pair(old, new, min_places),
        (old, new) => (
            delta_value(old, min_places),
            delta_value(new, min_places),
        ),
    }
}

/// The grade branch a PRIOR record was scored on — its persisted asset class,
/// the key the job routes the fund path on (`job.rs`, `is_fund`), so the class
/// is the branch for every record ever written; the fund path's
/// `fund_class_label` is a derived marker of the same fact and post-field. The
/// stamp belongs to that record, so the branch must be its branch, not the
/// current dossier's: priors join by symbol, and a symbol reclassified between
/// runs would otherwise read the wrong boundary in both directions.
fn grade_branch(prior: &HoldingVerdict) -> engine::GradeBranch {
    match prior.asset_class {
        crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund => {
            engine::GradeBranch::Fund
        }
        _ => engine::GradeBranch::Stock,
    }
}

/// The input-delta row for a scenario-target parameter boundary — the target
/// mirror of the grade rows: only over a priced prior with a stamped target
/// record, naming the horizons the boundary can have moved on the prior's branch
/// (`engine::target_parameter_change`), so an engine target move across it is
/// attributed to the parameter change rather than to evidence or a
/// self-correction (the 2026-08-24 review's Codex I11).
fn target_boundary_row(horizons: engine::TargetHorizons) -> String {
    format!(
        "scenario-target parameters changed since the prior analysis — the {} can move with \
         no input change",
        horizons.label()
    )
}

/// The continuity NOTE for the same boundary, in the interpretation prompt —
/// the grade NOTE's shape, naming the horizons.
fn target_boundary_note(horizons: engine::TargetHorizons) -> String {
    format!(
        "- The scenario-target parameters changed since the prior analysis, so the {} may \
         have moved with no change in the company's inputs.\n",
        horizons.label()
    )
}

/// Append one input-delta entry, assigning the next bracketed id.
fn push_delta(entries: &mut Vec<crate::portfolio::DeltaEntry>, label: String) {
    let id = format!("D{}", entries.len() + 1);
    entries.push(crate::portfolio::DeltaEntry {
        id,
        label,
        related_condition_id: None,
    });
}

/// Append this run's fresh distilled claims to the rendered input delta as
/// addressable entries — the 6g research-finding leg — each carrying the ledger
/// condition it bears on where the distillation tied one (the validated
/// `related_condition_id`, rendered by statement so the id stays app-owned).
/// The interpretation prompt marks that condition research-supported off the
/// entry, and a what-changed row can cite the finding like any engine entry
/// (`docs/portfolio-workflow.md` §Step 6d, §Step 6g).
fn push_research_delta_entries(
    entries: &mut Vec<crate::portfolio::DeltaEntry>,
    distilled: &DistilledResearch,
    prior_ledger: Option<&ThesisLedger>,
) {
    let mut n = 0usize;
    for topic in &distilled.topic_layer {
        for claim in topic.claims.iter().filter(|c| !c.cached) {
            n += 1;
            let bears_on = claim.related_condition_id.as_deref().and_then(|id| {
                prior_ledger?
                    .conditions
                    .iter()
                    .find(|c| c.condition_id == id)
            });
            let tie = bears_on
                .map(|c| format!(" — bears on ledger condition '{}'", c.statement))
                .unwrap_or_default();
            entries.push(crate::portfolio::DeltaEntry {
                id: format!("research-{n}"),
                label: format!(
                    "research finding ({}): {} [{}]{tie}",
                    topic.topic_key, claim.claim, claim.source_url
                ),
                related_condition_id: bears_on.map(|c| c.condition_id.clone()),
            });
        }
    }
}

/// The delta entries both branches share: the position delta, this run's ledger
/// crossings, and the house view where the prompt renders one.
fn append_shared_delta(
    entries: &mut Vec<crate::portfolio::DeltaEntry>,
    dossier: &HoldingDossier,
    position_change: PositionChange,
    ledger_eval: Option<&LedgerEvaluation>,
    price_bridge: Option<f64>,
) {
    // A detected re-basis is itself an input change worth attributing against —
    // without the row, a split's apparent price collapse has no evidence entry.
    match price_bridge {
        Some(f) if f != 1.0 => push_delta(
            entries,
            format!(
                "price series re-based since the prior analysis (split factor {f:.4}); prior \
                 price-denominated values converted onto the fresh basis"
            ),
        ),
        None => push_delta(
            entries,
            "price basis unverifiable this run — prior price-denominated comparisons \
             excluded"
                .to_string(),
        ),
        _ => {}
    }
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
            let (observed, threshold) = fmt_crossing_pair(c.observed_value, c.threshold);
            push_delta(
                entries,
                format!(
                    "ledger {role} '{}' {outcome}: observed {observed} vs threshold {threshold}",
                    c.statement
                ),
            );
        }
    }
    if dossier.house_view.latest_sections.is_some() {
        push_delta(
            entries,
            "market analysis: a market-level analysis supplied this run".to_string(),
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
    price_bridge: Option<f64>,
) -> Vec<crate::portfolio::DeltaEntry> {
    let Some(prior) = dossier.prior_verdict.as_ref() else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    // The prior side converts onto the fresh basis before the row renders — a
    // split must never read as a spot collapse. An unresolvable bridge skips the
    // row (the shared delta carries the exclusion entry).
    if let (Some(old), Some(new), Some(f)) = (
        dossier.prior_spot,
        dossier.financials.current_price,
        price_bridge,
    ) {
        let old = old * f;
        if old != new {
            let (old, new) = comparison_safe_pair(old, new, 2);
            push_delta(&mut entries, format!("spot: {old} -> {new}"));
        }
    }
    if let Some(prior_metrics) = dossier.prior_metrics.as_ref() {
        // The NAV-premium row carries signal only on the closed-end form
        // (`docs/portfolio-analysis.md` §Asset eligibility) — an open-end ETF's
        // transient premium flicker would otherwise seed a delta row every run.
        let cef = dossier
            .fund
            .as_ref()
            .is_some_and(|f| crate::portfolio::fund::is_closed_end(&f.fund));
        for c in engine::metric_delta(prior_metrics, &engine_output.metrics) {
            if c.name == "NAV premium" && !cef {
                continue;
            }
            let (old, new) = optional_delta_pair(c.old, c.new, 4);
            push_delta(
                &mut entries,
                format!("metric {}: {old} -> {new}", c.name),
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
                let (old, new) = comparison_safe_pair(old, new, 0);
                push_delta(
                    &mut entries,
                    format!("computed sub-score {name}: {old} -> {new}"),
                );
            }
        }
        if pg.grade != engine_output.grade {
            push_delta(
                &mut entries,
                format!(
                    "computed grade: {} -> {}",
                    pg.grade.as_str(),
                    engine_output.grade.as_str()
                ),
            );
        }
        // The prior target converts like the spot row; skipped when the basis is
        // unverifiable rather than compared cross-basis. It also requires the
        // prior pass to have CERTIFIED its basis (`prior_spot` rides the prior
        // quick basis, withheld by an unresolvable pass): a target persisted
        // fresh beneath a carried anchor would double-convert here the moment
        // that anchor resolved — a fabricated target-change row in the 6g
        // evidence vocabulary. Absent beats wrong.
        let prior_basis_certified = dossier.prior_spot.is_some();
        let old_base = pg
            .price_targets
            .twelve_month
            .as_ref()
            .and_then(|t| price_bridge.map(|f| t.base * f));
        let new_base = engine_output.price_targets.twelve_month.as_ref().map(|t| t.base);
        if price_bridge.is_some() && prior_basis_certified && old_base != new_base {
            let (old_base, new_base) = optional_delta_pair(old_base, new_base, 4);
            push_delta(
                &mut entries,
                format!(
                    "computed twelve-month base target: {old_base} -> {new_base}"
                ),
            );
        }
        if pg.risk_tier != engine_output.risk_tier {
            push_delta(
                &mut entries,
                format!(
                    "risk tier: {} -> {}",
                    pg.risk_tier.as_str(),
                    engine_output.risk_tier.as_str()
                ),
            );
        }
        if pg.dead_money != engine_output.hurdle.state {
            push_delta(
                &mut entries,
                format!(
                    "{CAPITAL_EFFICIENCY_DELTA_PREFIX}{:?} -> {:?}",
                    pg.dead_money, engine_output.hurdle.state
                ),
            );
        }
    }
    // A stamp boundary is a delta row only where it changed what this holding's
    // prior record means — read cumulatively from the stamp history on the
    // prior record's branch (`engine::grade_parameter_change`), and only over a
    // priced prior, since a record with no letter or sub-score had nothing to move. A
    // holding the boundary left unchanged gets no row: a citable row for a cause
    // that could not have operated would let a real move be attributed to it.
    let boundary = match &prior.disposition {
        VerdictDisposition::Priced(_) => engine::grade_parameter_change(
            dossier.prior_grade_parameter_version.as_deref(),
            grade_branch(prior),
        ),
        _ => None,
    };
    match boundary {
        Some(engine::GradeParameterChange::Letters) => push_delta(
            &mut entries,
            "grade bands changed since the prior analysis — letters can move with no input \
             change"
                .to_string(),
        ),
        Some(engine::GradeParameterChange::FundMomentum) => push_delta(
            &mut entries,
            "fund momentum moved to the short price window since the prior analysis — the \
             momentum sub-score can move with no input change; the letter cannot"
                .to_string(),
        ),
        Some(engine::GradeParameterChange::FundSectorPeBasis) => push_delta(
            &mut entries,
            "fund sector-P/E source now requires both exchange legs since the prior analysis \
             — the valuation sub-score and letter can move on the same served rows"
                .to_string(),
        ),
        None => {}
    }
    // The scenario-target stamp reads its own history on the same rule (Codex
    // I11): over a priced prior with a stamped target record, on the prior's
    // branch, naming the horizons the rows after its stamp touched — so a target
    // that moved on a version bump alone is never attributed to company evidence
    // or a self-correction (which marks `thesis_changed` and can open a successor
    // episode). `None` whenever the stamp is, so this never renders empty.
    let target_boundary = match &prior.disposition {
        VerdictDisposition::Priced(_) => engine::target_parameter_change(
            dossier.prior_target_parameter_version.as_deref(),
            grade_branch(prior),
        ),
        _ => None,
    };
    if let Some(horizons) = target_boundary {
        push_delta(
            &mut entries,
            target_boundary_row(horizons),
        );
    }
    append_shared_delta(&mut entries, dossier, position_change, ledger_eval, price_bridge);
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
    price_bridge: Option<f64>,
) -> Vec<crate::portfolio::DeltaEntry> {
    if dossier.prior_verdict.is_none() {
        return Vec::new();
    }
    let mut entries = Vec::new();
    if let Some(prior_metrics) = dossier.prior_metrics.as_ref() {
        for c in engine::metric_delta(prior_metrics, fund_metrics) {
            let (old, new) = optional_delta_pair(c.old, c.new, 4);
            push_delta(
                &mut entries,
                format!("metric {}: {old} -> {new}", c.name),
            );
        }
    }
    append_shared_delta(&mut entries, dossier, position_change, ledger_eval, price_bridge);
    entries
}

/// Render the input delta and the attribution rules into the user prompt — the
/// bracketed ids are the `what_changed_entries` evidence vocabulary.
fn input_delta_prompt_section(entries: &[crate::portfolio::DeltaEntry]) -> String {
    let mut s = String::from(
        "\nCHANGES SINCE THE PRIOR ANALYSIS (each with an id)\n",
    );
    // The capital-efficiency row is an action-call input: it stays in the audit
    // and the action packet's evidence list, and never reaches the interpretation
    // projection (`portfolio-v40`, Codex round 1).
    let shown: Vec<&crate::portfolio::DeltaEntry> = entries
        .iter()
        .filter(|e| !e.label.starts_with(CAPITAL_EFFICIENCY_DELTA_PREFIX))
        .collect();
    for e in &shown {
        s.push_str(&format!("[{}] {}\n", e.id, e.label));
    }
    if shown.is_empty() {
        s.push_str("None recorded.\n");
    }
    s
}

/// The capital-efficiency delta row's label prefix — one home for the push and
/// the interpretation projection's filter.
const CAPITAL_EFFICIENCY_DELTA_PREFIX: &str = "capital-efficiency read: ";

/// The 6g **what-changed attribution validator**
/// (`docs/portfolio-workflow.md` §Step 6g): every row the model labels external
/// must resolve to a concrete entry in the rendered input delta — by bracketed id
/// or label verbatim — or it is **downgraded to self-correction with a logged
/// reason** (ruled 2026-08-21; the research-finding and
/// `research_forward_assumption` legs are live as rendered delta entries, so
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

/// The system prompt for the interpretation stage (`portfolio-v40`): the role,
/// the two-part shape of the message, and the output names — nothing that
/// describes the data the message carries. The output-name line is
/// [`crate::portfolio::interpretation_response_contract`], built from the same
/// key list as the schema's required set.
pub fn interpretation_system_prompt(_is_fund: bool, debut: bool) -> String {
    format!(
        "You are an equity analyst producing an independent read of one holding for a \
         portfolio review. Part 1 of the message gives the inputs. Part 2 states what to \
         determine from them and the shape to return. {}",
        crate::portfolio::interpretation_response_contract(debut)
    )
}

/// The system prompt for the `role_risk_only` interpretation (`portfolio-v42`):
/// the role line, the two-part shape of the message and the output names — the
/// same footing as [`interpretation_system_prompt`], the vehicle named as a fund
/// since this branch is a fund by construction (ruled 2026-09-17). The
/// output-name line is [`crate::portfolio::role_risk_response_contract`], built
/// from the same key list as the schema's required set.
pub fn role_risk_system_prompt(debut: bool) -> String {
    format!(
        "You are an investment analyst producing an independent read of one fund holding \
         for a portfolio review. Part 1 of the message gives the inputs. Part 2 states what \
         to determine from them and the shape to return. {}",
        crate::portfolio::role_risk_response_contract(debut)
    )
}

/// Whether a dossier's vehicle is a fund — the class-shaped executability
/// surface (statement series never resolve on the fund path, the expense ratio
/// only there), computed once from the asset class wherever a prompt, schema or
/// validator needs it.
pub(crate) fn dossier_is_fund(d: &HoldingDossier) -> bool {
    matches!(
        d.position.asset_class,
        crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
    )
}

/// Whether an interpretation message will render any market-analysis content —
/// the latest sections or the recent stances. Both branches render the house
/// view through one [`market_analysis_section`] since `portfolio-v42` (ruled
/// 2026-09-17: the role/risk message gains the stances), so the audit's
/// house-view source claim reads the one predicate the render reads; a
/// summary-only house view (reachable whenever the latest report's Markdown is
/// missing or unreadable, which `load_house_view` degrades to deliberately)
/// reaches both verdicts as the stance lines.
pub(crate) fn prompt_renders_house_view(d: &HoldingDossier) -> bool {
    d.house_view.latest_sections.is_some() || !d.house_view.recent_summaries.is_empty()
}

/// The prompt's holding header — the identity and per-share quote every
/// model-facing packet opens with: both interpretation branches, the research
/// brief and the action call. The name falls back to the resolved listing's
/// company name when Schwab supplies no description, which otherwise renders
/// as `HOLDING: PSX ()` and leaves the model speculating about the ticker
/// (`docs/verification/2026-08-10-big-run-attempt-1.md` §Finding 4). Since
/// `portfolio-v38` the header carries no account economics on any route —
/// no quantity, cost basis, market value or unrealized P/L (fix list 3.2,
/// ruled 2026-09-16): those are the account's ownership history, not the
/// issuer's condition, and the intrinsic read is of no investor
/// (`docs/portfolio-analysis.md` §Intrinsic verdict). The action call's
/// header had held that form since `portfolio-v36`; the one function now
/// serves every packet. Since `portfolio-v43` it closes with the analysis
/// date (the run's session date), so every packet anchors its period labels
/// to the date of the analysis rather than the model's training horizon
/// (attempt-6 Finding 7; fix list 5.1, ruled 2026-09-17 for the header).
pub(crate) fn holding_header(d: &HoldingDossier) -> String {
    format!(
        "HOLDING\n{} ({}).\nPrice: {}\nDate: {}.\n",
        d.position.symbol,
        holding_display_name(d),
        spot_line(d),
        d.analysis_date,
    )
}

/// The per-share quote — "$358.97 per share." — or "(gap)" where no usable price
/// reached the packet. It carries no date: the quote and the dated daily closes
/// are separate requests, so the last close's date is not the quote's (Codex,
/// `portfolio-v40` round 1).
fn spot_line(d: &HoldingDossier) -> String {
    d.financials
        .current_price
        .filter(|p| p.is_finite() && *p > 0.0)
        .map(|p| format!("${p:.2} per share."))
        .unwrap_or_else(|| "(gap)".into())
}

/// The holding's display name for the prompt headers — the Schwab description
/// where it names the issuer, else the canonical-source fallback.
fn holding_display_name(d: &HoldingDossier) -> &str {
    let described = d.position.description.trim();
    if !crate::portfolio::listing::describes_issuer(described, &d.position.symbol) {
        // The fallback is held to a *canonical-source* standard, deliberately
        // looser than the description's: FMP's parser accepts any non-blank
        // `companyName`, so bare-ticker and tokenless noise are rejected — but a
        // ticker-token-only LEGAL name ("ASML Holding N.V.", "eBay Inc.") is
        // real identity from a canonical source, and holding it to the
        // description's stricter rule starved ticker-named issuers of any name
        // at all (combined-range review). A fund's profile read is
        // structure-only (closed-end detection — no identity mapping), so its
        // identity rides the fund data's own name — the role-risk branch's
        // only naming source.
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
    }
}

/// The role/risk branch's computed metric surface — the expense ratio and the
/// closed-end read off the readout, plus the price legs the ledger evaluation
/// reads (the closed-end read joins so a served NAV's premium move seeds its own
/// input-delta row — Codex 2026-08-21 round 3, finding 3). Built once for the
/// evaluation, the audit and the prompt's authoring contract, so the three read
/// one surface.
pub(crate) fn fund_ledger_metrics(
    readout: &RoleRiskReadout,
    fin: &engine::CompanyFinancials,
) -> engine::ComputedMetrics {
    let price_legs = engine::compute_metrics(fin);
    engine::ComputedMetrics {
        expense_ratio: readout.expense_ratio,
        nav_premium: readout.nav_premium,
        return_volatility: price_legs.return_volatility,
        trailing_return: price_legs.trailing_return,
        ..Default::default()
    }
}

/// The role/risk message (`portfolio-v42`, ruled 2026-09-17 on the `portfolio-v40`
/// frame; `docs/verification/2026-09-17-role-risk-prompt-rewrite.md`): one
/// message in two marked parts. Part 1 is inputs only — HOLDING, CLASS (the
/// label, the reported asset class and the structure line), EXPOSURE TILT with
/// the closed-end line and the positioning line, RISK PROFILE with the
/// market-wide options backdrop, EVIDENCE GAPS, the shared FINANCIAL METRICS,
/// RESEARCH SUMMARY, the shared MARKET ANALYSIS, and on a continuity call PRIOR
/// ANALYSIS (the prior class and role read), the recall notes and the changes
/// since; then the shared prior-ledger data and crossings — each section
/// explained once and then its values, with no instruction in it. Part 2 is
/// the task only: the role read, the shared ledger item with trim and sell
/// families, on continuity the shared what-changed items, and the
/// placeholder-only return shape. The model receives data, never a description
/// of the app that produced it. The section names match the action packet's
/// role/risk branch so the two packets read the holding the same way.
pub fn role_risk_user_prompt(input: &RoleRiskInput) -> String {
    let d = input.dossier;
    let r = input.readout;
    let debut = d.prior_verdict.is_none();
    let mut p = String::from("======== PART 1: INPUTS ========\n");

    // HOLDING
    p.push_str(&holding_header(d));
    p.push_str(&format!("{}\n", describe_position_change(&d.position_delta)));

    // CLASS: the label, the fund's reported asset class where the metadata
    // carries one (ruled 2026-09-17), and the structure line where it applies.
    p.push_str(&format!("\nCLASS\n{}.", r.class_label));
    if let Some(class) = d.fund.as_ref().and_then(|f| f.fund.asset_class.as_deref()) {
        p.push_str(&format!(" Reported asset class: {class}."));
    }
    p.push('\n');
    if let Some(kind) = r.structural_kind {
        p.push_str(match kind {
            FundStructuralKind::LeveragedInverse => {
                "Structure: leveraged / inverse, resetting daily.\n"
            }
            FundStructuralKind::OptionOverlay => {
                "Structure: option overlay; the options reshape the return path.\n"
            }
        });
    }

    // EXPOSURE TILT: the readout's rows as computed, the basis glossed once.
    let has_tilt = !r.exposure_tilt.is_empty();
    if has_tilt {
        p.push_str(
            "\nEXPOSURE TILT\nThe fund's largest weights, by sector where reported and \
             otherwise by country.\n",
        );
        for (label, weight) in &r.exposure_tilt {
            p.push_str(&format!("- {label}: {:.1}%\n", weight * 100.0));
        }
    }
    // The closed-end read renders only where the vehicle makes it meaningful
    // (`docs/portfolio-analysis.md` §Asset eligibility); its absence is a named
    // gap already in the evidence-gap manifest, never a fabricated number.
    if r.is_cef {
        if let Some(prem) = r.nav_premium {
            p.push('\n');
            p.push_str(&nav_premium_line(prem));
        }
    }
    // The commodity / macro classes this branch types are exactly where the
    // underlying-positioning read carries signal (`docs/data-sources.md §CFTC`).
    if let Some(f) = &d.fund {
        p.push_str(&positioning_prompt_section(f));
    }

    // RISK PROFILE: the annualized read with its unit, and the market-wide
    // backdrop. The daily volatility the ledger evaluates is a FINANCIAL
    // METRICS line, so it renders once.
    p.push_str(&format!(
        "\nRISK PROFILE\nAnnualized realized volatility: {} (a fraction; 0.14 means 14% a \
         year).\n",
        opt(r.observable_risk),
    ));
    p.push_str(&put_call_backdrop_prompt_section(d));

    // EVIDENCE GAPS
    let has_gaps = !r.evidence_gaps.is_empty();
    if has_gaps {
        p.push_str(&format!("\nEVIDENCE GAPS\n{}\n", r.evidence_gaps.join("; ")));
    }

    // FINANCIAL METRICS: the branch's computed surface, each line with its
    // ledger label, unit and confirmation rule — the shared section.
    let fund_metrics = fund_ledger_metrics(r, &d.financials);
    let contract = LedgerSeriesContract::build(true, Some(&fund_metrics), Some(&d.financials));
    p.push_str(&financial_metrics_section(&contract, d, true));

    // RESEARCH SUMMARY: the fund agenda's distilled research — pure
    // consolidation on this branch (`docs/portfolio-workflow.md` §Step 6d).
    p.push_str(&format!("\nRESEARCH SUMMARY\n{}\n", input.distilled));

    // MARKET ANALYSIS
    p.push_str(&market_analysis_section(d));

    // Continuity inputs: the prior read, the prior notes, the changes since,
    // then the prior ledger and its crossings.
    if let Some(prior) = &d.prior_verdict {
        p.push_str(&prior_role_read_section(prior, d.prior_vintage.as_deref()));
        p.push_str(&semantic_recall_prompt_section(d));
        p.push_str(&input_delta_prompt_section(input.input_delta));
    }
    p.push_str(&prior_ledger_data_section(
        input.prior_ledger,
        input.ledger_eval,
        input.input_delta,
    ));

    // PART 2
    p.push_str(&role_risk_task_section(
        &contract,
        debut,
        input.prior_ledger.is_some(),
        has_tilt,
        has_gaps,
    ));
    p
}

/// PRIOR ANALYSIS on the role/risk branch (`portfolio-v42`, ruled 2026-09-17):
/// the prior class and the prior role read verbatim, with the prior read's
/// vintage, so a role-read change row has an old value to cite and the read is
/// tested against something the model can see. A prior that was not a role and
/// risk read (a priced read, or an abstention) says only that, as data — no
/// sentence explains the line's reach (task review, 2026-09-17).
fn prior_role_read_section(prior: &HoldingVerdict, vintage: Option<&str>) -> String {
    let since = vintage
        .map(|t| format!(" (prior read {t})"))
        .unwrap_or_default();
    match &prior.disposition {
        VerdictDisposition::RoleRiskOnly(rr) => format!(
            "\nPRIOR ANALYSIS{since}\n- prior class: {}.\n- prior role read: {}\n",
            rr.class_label, rr.role_summary
        ),
        _ => format!("\nPRIOR ANALYSIS{since}\nThe prior analysis was not a role and risk read.\n"),
    }
}

/// Part 2 of the role/risk message (`portfolio-v42`): the role read from the
/// sections that rendered, the shared ledger item with trim and sell families,
/// on a continuity call the shared what-changed items with this branch's detail
/// gloss and no parameter-boundary sentence (no grade or target parameter exists
/// here), and the placeholder-only return shape.
fn role_risk_task_section(
    contract: &LedgerSeriesContract,
    debut: bool,
    has_prior_ledger: bool,
    has_tilt: bool,
    has_gaps: bool,
) -> String {
    let mut p = String::from(
        "\n======== PART 2: TASK ========\n\n\
         Determine the following from the inputs and return them as one JSON object in the \
         shape at the end, with no code fence and no surrounding text.\n",
    );
    let mut sections = vec!["CLASS"];
    if has_tilt {
        sections.push("EXPOSURE TILT");
    }
    sections.push("RISK PROFILE");
    if has_gaps {
        sections.push("EVIDENCE GAPS");
    }
    sections.push("FINANCIAL METRICS");
    let (last, head) = sections.split_last().expect("at least two sections");
    p.push_str(&format!(
        "\n1. role_summary — a few sentences on the vehicle's mandate, the exposure it exists \
         to supply, and the cost and risk of holding it, from {}, {last} and RESEARCH \
         SUMMARY.\n",
        head.join(", ")
    ));
    p.push_str(&ledger_task_item(2, contract, LedgerItemBranch::RoleRisk, has_prior_ledger));
    if !debut {
        p.push_str(&what_changed_task_items(
            3,
            "the role read, the scenario or the condition",
            false,
        ));
    }
    p.push_str(&format!(
        "\nRETURN SHAPE (every value is a placeholder; an array holds as many items as apply)\n{}\n",
        crate::portfolio::role_risk_return_shape(debut)
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
        return "\nPRIOR ANALYSIS\nThe prior analysis was not a priced read (a role and risk \
                read, or an abstention), so there are no prior scores or targets to compare.\n"
            .to_string();
    };
    let mut p = String::new();
    let since = d
        .prior_vintage
        .as_deref()
        .map(|t| format!(" (prior read {t})"))
        .unwrap_or_default();
    p.push_str(&format!("\nPRIOR ANALYSIS{since}\n"));

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
        "- prior computed read: grade {} (q {:.0} / v {:.0} / r {:.0}; momentum {:.0}); {}; {}\n",
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
        let (model_label, action_read) = match prior.action_source {
            ActionSource::ModelChosen => (
                "your prior read",
                format!("action {} (model-chosen)", g.action.as_kebab()),
            ),
            ActionSource::RuleDemoted => (
                "your prior read (its action was later demoted by rule)",
                format!(
                    "action {} (demoted by rule after authoring; the rung you chose is not on \
                     record)",
                    g.action.as_kebab()
                ),
            ),
        };
        p.push_str(&format!(
            "- {model_label}: letter {} (q {:.0} / v {:.0} / m {:.0} / r {:.0}); \
             1-mo base {:.2} [{:.2}\u{2013}{:.2}], 12-mo base {:.2} [{:.2}\u{2013}{:.2}]; \
             conviction {:?}, {}, {action_read}\n",
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
                                "distance to the prior computed 12-mo base {:+.1}%",
                                (spot / (t.base * bridge) - 1.0) * 100.0
                            ));
                        }
                    }
                    let b = g.model_view.price_targets.twelve_month.base;
                    if b > 0.0 {
                        vs.push(format!(
                            "distance to your prior 12-mo base {:+.1}%",
                            (spot / (b * bridge) - 1.0) * 100.0
                        ));
                    }
                }
                p.push_str(&format!(
                    "- price now {:.2}: {} (split-adjusted)\n",
                    spot,
                    vs.join("; ")
                ));
            }
            None => p.push_str(&format!(
                "- price now {:.2}: prior-read price comparison unavailable — no \
                 anchor-session close at the prior vintage (excluded rather than \
                 guessed)\n",
                spot
            )),
        }
    }

    if d.prior_matured_notes.is_empty() {
        p.push_str("- matured scored windows: none yet\n");
    } else {
        p.push_str(
            "- matured scored windows for this holding (any vintage — a window may predate \
             the prior read):\n",
        );
        for note in &d.prior_matured_notes {
            p.push_str(&format!("  - {note}\n"));
        }
    }
    p
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
        "\nUNDERLYING POSITIONING (CFTC weekly, as of {})\n{} — speculator net {:+.0} \
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
        "Market-wide options sentiment (CBOE daily put/call, as of {}): total {}, index {}, \
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
            "{} growth versus the trailing print of {} at the bull multiple, \
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
        "- What the current price implies, at each scenario's multiple: {growth}. \
         Assumptions: {} multiples{}{}.\n",
        if ie.rate_anchored { "rate-anchored (spread-percentile)" } else { "raw-percentile" },
        if ie.rate_anchored {
            format!(", 10-year Treasury {:.2}%", ie.dgs10 * 100.0)
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
/// Every packet renders structure and ratios only — class, coverage ratio, net
/// delta as a fraction of the held shares, each leg's direction / kind / strike /
/// expiry / delta — never contract counts or share-equivalents: the action
/// packet since `portfolio-v36` (ruled 2026-09-16, C1) so position size reaches
/// the rung by no route, and the interpretation packets since `portfolio-v38`
/// (fix list 3.2, the Codex plan review), where contracts over coverage had
/// still given the held share count away.
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
        "\nSAME-UNDERLYING OPTION OVERLAY (held option positions on this name): \
         classified {class}"
    );
    if let Some(cr) = o.coverage_ratio {
        s.push_str(&format!(", covering {:.0}% of the held shares", cr * 100.0));
    }
    if let Some(nd) = o.net_delta {
        if d.position.quantity > 0.0 {
            s.push_str(&format!(
                "; net delta {:+.0}% of the held shares",
                nd / d.position.quantity * 100.0
            ));
        }
    }
    s.push_str(".\n");
    for l in &o.legs {
        s.push_str(&format!(
            "- {} {} — strike {}, expiry {}, delta {}\n",
            match l.direction {
                OverlayDirection::Long => "LONG",
                OverlayDirection::Short => "SHORT",
            },
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
/// gap manifest carries the reason). A hype read with no persisted ratio is
/// one of two states the render must not conflate: a non-positive reality leg
/// (the ratio is undefined there), or a positive one the expansion outran
/// beyond any finite multiple — the quotient overflowed, so the engine
/// classified hype and persisted the ratio absent (Codex I16, round 2;
/// `portfolio-v21`). The percentage render is guarded the same way: a finite
/// decimal leg whose ×100 overflows prints as the decimal ratio, never `inf%`.
fn narrative_prompt_section(n: Option<&engine::NarrativeRead>) -> String {
    use crate::portfolio::engine::{NarrativeClass, NarrativeForm};
    let Some(n) = n else {
        return String::new();
    };
    let pct = |v: f64| {
        let scaled = v * 100.0;
        if scaled.is_finite() {
            format!("{scaled:+.1}%")
        } else {
            format!("{v:+.2e} as a decimal ratio (beyond the percentage render's range)")
        }
    };
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
            match n.ratio {
                Some(r) => format!(" ({r:.1}×)"),
                None if n.reality > 0.0 =>
                    " (by more than any finite multiple — the ratio overflowed)".to_string(),
                None => " (reality flat or declining)".to_string(),
            },
        ),
    };
    let mut s = format!(
        "\nNARRATIVE VS REALITY ({} days elapsed)\n{expansion_label} {}; {reality_label} {}. \
         Read: {class_line}.\n",
        n.elapsed_days,
        pct(n.expansion),
        pct(n.reality),
    );
    if let Some(rule) = &n.matched_rule {
        s.push_str(&format!("Rule matched, capping the computed conviction: {rule}.\n"));
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
        "\nSHORT INTEREST (FINRA biweekly file, settlement {}; the file lags its \
         settlement by about 7 business days)\n{:.0} shares short ({trend}), average \
         daily volume {}, days to cover {}\n",
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
        "\nCOMMODITY PRICES (matched to this holding's sector; each as of its print date, \
         and a monthly series lags)\n",
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
fn forensic_prompt_section(d: &HoldingDossier, stage: PromptStage) -> String {
    use crate::portfolio::ForensicFilingState;
    match &d.filing_events {
        None => String::new(),
        Some(ForensicFilingState::Clear) => {
            "\nFORENSIC FILINGS (8-K sweep)\nClean — no restatement (Item 4.02) or \
             auditor-change (Item 4.01) filing inside the lookback.\n"
                .to_string()
        }
        Some(ForensicFilingState::Unknown { reason, .. }) => format!(
            "\nFORENSIC FILINGS (8-K sweep)\nUnknown — {reason}. Not a clean check.\n"
        ),
        Some(ForensicFilingState::Events { events }) => {
            let mut s = String::from("\nFORENSIC FILINGS (8-K sweep)\nEvents found:\n");
            for ev in events {
                s.push_str(&format!(
                    "- {} — filed {} ({}; {})\n",
                    ev.kind.label(),
                    ev.filing_date,
                    ev.source,
                    ev.confidence
                ));
            }
            if stage == PromptStage::Interpretation {
                s.push_str(
                    "By rule: the computed conviction is capped at low and the computed action \
                     set excludes adding; the grade is unchanged.\n",
                );
            }
            s
        }
    }
}

/// The interpretation message (`portfolio-v40`, ruled 2026-09-17 off the v39
/// read): one message in two marked parts. Part 1 is inputs only — every data
/// section explained once (what it is, each field's unit or polarity) and then
/// its values, with no instruction in it. Part 2 is the task only — numbered
/// items in output order, each naming the input section it draws on and never
/// restating a value or a unit, closing with the placeholder-only return shape.
/// The model receives data, never a description of the app that produced it:
/// no arms, baselines, stages, seams, validator behaviour, stamps or product
/// names (`docs/portfolio-workflow.md` §Step 6f; the principle is recorded in
/// `docs/verification/2026-09-17-interpretation-prompt-rewrite.md`). The
/// investor profile is deliberately absent — the intrinsic verdict is of no
/// investor (`docs/portfolio-analysis.md` §Intrinsic verdict).
pub fn interpretation_user_prompt(input: &InterpretationInput) -> String {
    let d = input.dossier;
    let e = input.engine;
    let is_fund = dossier_is_fund(d);
    let debut = d.prior_verdict.is_none();
    let mut p = String::new();

    p.push_str("======== PART 1: INPUTS ========\n");

    // HOLDING
    p.push_str(&holding_header(d));
    p.push_str(&format!("{}\n", describe_position_change(&d.position_delta)));

    // FUND (fund only)
    if let Some(f) = &d.fund {
        p.push_str(&format!(
            "\nFUND\nExpense ratio: {} (a fraction of assets per year; 0.0075 means 0.75%). \
             US share of holdings: {}.\n",
            fmt_expense_ratio(f.fund.expense_ratio),
            crate::portfolio::fund::us_share(&f.fund)
                .map(|s| format!("{:.0}%", s * 100.0))
                .unwrap_or_else(|| "(gap)".to_string()),
        ));
        if let Some(cov) = e.metrics.composite_coverage {
            p.push_str(&format!(
                "Composite P/E coverage: {:.0}% of fund weight; the uncovered {:.0}% is \
                 outside the valuation score, not averaged in.\n",
                cov * 100.0,
                (1.0 - cov) * 100.0
            ));
        }
        if crate::portfolio::fund::is_closed_end(&f.fund) {
            match e.metrics.nav_premium {
                Some(prem) => p.push_str(&nav_premium_line(prem)),
                None => p.push_str(
                    "PRICE VS NAV: (gap) — closed-end fund with no NAV on the \
                     current data surface.\n",
                ),
            }
        }
        p.push_str(&positioning_prompt_section(f));
    }

    // FINANCIAL METRICS — the values, each with its ledger label, unit and
    // confirmation rule, so the ledger item in Part 2 points here by name.
    let contract = LedgerSeriesContract::build(is_fund, Some(&e.metrics), Some(&d.financials));
    p.push_str(&financial_metrics_section(&contract, d, is_fund));

    // COMPUTED SCORES
    p.push_str(&format!(
        "\nCOMPUTED SCORES\nFour scores from 0 to 100, higher is better on every axis: quality; \
         valuation, where higher means more attractive; momentum; risk, where higher means \
         more resilient.\n\
         quality {:.0}, valuation {:.0}, momentum {:.0}, risk {:.0}.{} Risk tier: {}.\n",
        e.sub_scores.quality,
        e.sub_scores.valuation,
        e.sub_scores.momentum,
        e.sub_scores.risk,
        if e.low_confidence_grade { " One score is imputed." } else { "" },
        e.risk_tier.as_str(),
    ));

    // COMPUTED PRICE TARGETS
    p.push_str("\nCOMPUTED PRICE TARGETS (USD)\n");
    if let Some(tm) = &e.price_targets.twelve_month {
        p.push_str(&format!(
            "- twelve-month: bear {:.2} / base {:.2} / bull {:.2}. Method: {}\n",
            tm.bear,
            tm.base,
            tm.bull,
            twelve_month_method(&e.target_meta)
        ));
    }
    if let Some(om) = &e.price_targets.one_month {
        p.push_str(&format!(
            "- one-month: bear {:.2} / base {:.2} / bull {:.2}.\n",
            om.bear,
            om.base,
            om.bull,
        ));
    }
    if let Some(notes) = target_notes_line(&e.target_meta) {
        p.push_str(&format!("- Notes: {notes}\n"));
    }
    p.push_str(&implied_expectations_prompt_section(e));
    p.push_str(&narrative_prompt_section(input.narrative));
    if let Some(overlay) = input.pre_profit {
        p.push_str(&pre_profit_prompt_section(overlay, PromptStage::Interpretation));
    }

    // OPTIONS ACTIVITY and the positioning legs
    let s = &d.options_signal;
    p.push_str(&format!(
        "\nOPTIONS ACTIVITY\nput/call volume {}, put/call open interest {}, implied volatility {}, \
         IV skew {} (mean put IV minus mean call IV, in IV's decimal unit; positive means \
         puts are richer).\n",
        opt(s.put_call_volume),
        opt(s.put_call_open_interest),
        opt(s.implied_volatility),
        fmt_iv_skew(s.iv_skew),
    ));
    p.push_str(&put_call_backdrop_prompt_section(d));
    p.push_str(&short_interest_prompt_section(d));
    p.push_str(&option_overlay_prompt_section(d));
    p.push_str(&forensic_prompt_section(d, PromptStage::Interpretation));
    p.push_str(&commodity_prompt_section(d));

    // RESEARCH SUMMARY
    p.push_str(&format!("\nRESEARCH SUMMARY\n{}\n", input.distilled));

    // MARKET ANALYSIS
    p.push_str(&market_analysis_section(d));

    // Continuity inputs: the sector-relative move, the prior read, the prior
    // notes, the changes since, the prior ledger and its crossings.
    if let Some(f) = input.tech_pre_flag.filter(|f| f.fired) {
        p.push_str(&format!(
            "\nSECTOR-RELATIVE MOVE\n{:+.1}% versus {} over {} sessions since the prior \
             analysis, beyond the ±{:.1}% threshold (two times the interval-scaled realized \
             volatility). A possible third-party repricing event; the cause is not known.\n",
            f.relative_move * 100.0,
            f.benchmark,
            f.sessions,
            f.threshold * 100.0,
        ));
    }
    if let Some(prior) = &d.prior_verdict {
        p.push_str(&retrospective_prompt_section(d));
        let boundary = match &prior.disposition {
            VerdictDisposition::Priced(_) => engine::grade_parameter_change(
                d.prior_grade_parameter_version.as_deref(),
                grade_branch(prior),
            ),
            _ => None,
        };
        match boundary {
            Some(engine::GradeParameterChange::Letters) => p.push_str(
                "- The grade bands changed since the prior analysis, so the letter may have \
                 moved with no change in the company's inputs.\n",
            ),
            Some(engine::GradeParameterChange::FundMomentum) => p.push_str(
                "- The fund momentum read moved to the short price window since the prior \
                 analysis, so the momentum score may have moved with no change in the fund's \
                 inputs; the letter did not move for that reason.\n",
            ),
            Some(engine::GradeParameterChange::FundSectorPeBasis) => p.push_str(
                "- The fund sector-P/E source now requires both exchange legs since the prior \
                 analysis, so the valuation score and the letter may have moved on the same \
                 served rows.\n",
            ),
            None => {}
        }
        let target_boundary = match &prior.disposition {
            VerdictDisposition::Priced(_) => engine::target_parameter_change(
                d.prior_target_parameter_version.as_deref(),
                grade_branch(prior),
            ),
            _ => None,
        };
        if let Some(horizons) = target_boundary {
            p.push_str(&target_boundary_note(horizons));
        }
        p.push_str(&semantic_recall_prompt_section(d));
        p.push_str(&input_delta_prompt_section(input.input_delta));
    }
    p.push_str(&prior_ledger_data_section(
        input.prior_ledger,
        input.ledger_eval,
        input.input_delta,
    ));

    // PART 2
    p.push_str(&interpretation_task_section(
        &contract,
        is_fund,
        d.fund.is_some(),
        debut,
        input.prior_ledger.is_some(),
    ));
    p
}

/// The market-analysis input: the latest report's thesis and strategy sections
/// and the stance of the recent reports, as market context — named for what it
/// is, never by product name.
fn market_analysis_section(d: &HoldingDossier) -> String {
    let mut p = String::new();
    let latest_date = d
        .house_view
        .recent_summaries
        .first()
        .map(|s| s.created_at.chars().take(10).collect::<String>());
    if let Some(sections) = &d.house_view.latest_sections {
        p.push_str("\nMARKET ANALYSIS\n");
        p.push_str(&match (&latest_date, d.house_view.recent_summaries.len()) {
            (Some(date), n) if n > 1 => format!(
                "A market-level analysis dated {date}, followed by the stance of the {n} most \
                 recent analyses.\n"
            ),
            (Some(date), _) => format!("A market-level analysis dated {date}.\n"),
            (None, _) => "A market-level analysis.\n".to_string(),
        });
        p.push_str(sections);
        p.push('\n');
    } else if !d.house_view.recent_summaries.is_empty() {
        p.push_str("\nMARKET ANALYSIS\nThe stance of the most recent market-level analyses.\n");
    }
    for s in &d.house_view.recent_summaries {
        p.push_str(&format!(
            "- {}: thesis {}, risk posture {}\n",
            s.created_at.chars().take(10).collect::<String>(),
            s.thesis_stance.as_str(),
            s.risk_posture.as_str()
        ));
    }
    p
}

/// The twelve-month method as a plain clause from the typed target inputs
/// (`portfolio-v40`): the driver, the multiple and the anchoring — never the
/// engine's own methodology string, which names its mechanics and its
/// parameter stamp for the audit.
fn twelve_month_method(t: &engine::TargetMeta) -> String {
    let driver = t.driver_rung.as_str();
    let multiple = if driver.contains("revenue") {
        "P/S"
    } else if driver.contains("fund") {
        "composite P/E"
    } else {
        "P/E"
    };
    let scenarios = if t.flat_driver { "" } else { " (low / mid / high)" };
    if t.current_multiple_carry {
        format!(
            "{driver}{scenarios} × the current {multiple} multiple (no anchor history, so the \
             targets sit near the current price and carry little forward signal)"
        )
    } else if t.rate_anchored {
        format!(
            "{driver}{scenarios} × {multiple} multiples at the 75th / 50th / 25th percentile of \
             their spread to the 10-year Treasury over the last {} quarterly observations",
            t.anchor_observations
        )
    } else {
        format!(
            "{driver}{scenarios} × {multiple} multiples at the 25th / 50th / 75th percentile of \
             their own history (too few rate observations to anchor)"
        )
    }
}

/// The one-month method as a plain clause: the base prorated from the
/// twelve-month base return, the band read back from the target itself.
fn one_month_method(om: &crate::portfolio::PriceTarget) -> String {
    let band = if om.base > 0.0 { (om.bull / om.base - 1.0) * 100.0 } else { 0.0 };
    format!(
        "base = the twelve-month base price return prorated to one month; bear and bull = \
         ±{band:.1}% (two standard deviations of daily volatility over 21 sessions, capped at 15%)"
    )
}

/// The targets' derivation notes that bear on how much signal they carry,
/// rendered only where one applies (the 2026-07-31 run's F6).
fn target_notes_line(t: &engine::TargetMeta) -> Option<String> {
    let mut notes = Vec::new();
    if let Some(rows) = t.consensus_rows {
        notes.push(if rows >= 2 {
            "the driver blends two consensus rows".to_string()
        } else {
            "the driver is a single forward consensus row".to_string()
        });
    }
    if t.flat_driver {
        notes.push("the driver is held flat across scenarios".to_string());
    }
    if t.clamp_flattened {
        notes.push("the published scenario spread was flattened".to_string());
    }
    if t.dispersion_floor_applied {
        notes.push("the band was widened to the volatility dispersion floor".to_string());
    }
    if notes.is_empty() {
        None
    } else {
        Some(format!("{}.", notes.join("; ")))
    }
}

/// Part 2 of the interpretation message: what to determine from the inputs, in
/// output order, each item naming the Part 1 section it draws on. The ledger
/// item carries the authoring contract as requirements on the output — the
/// label, comparator, threshold and margin; the statement agreeing with the
/// core; one level, no qualifier; null otherwise — with the margin sized by
/// example and its caps unshown (`portfolio-v40`, ruled 2026-09-17 off the v39
/// read, where the shown caps were sized toward). Every rule the text no longer
/// explains is still enforced at the 6g seam.
fn interpretation_task_section(
    contract: &LedgerSeriesContract,
    is_fund: bool,
    has_fund_section: bool,
    debut: bool,
    has_prior_ledger: bool,
) -> String {
    let mut p = String::from(
        "\n======== PART 2: TASK ========\n\n\
         Determine the following from the inputs and return them as one JSON object in the \
         shape at the end, with no code fence and no surrounding text.\n",
    );
    let summary_scope = if has_fund_section {
        "the fund's cost, exposure and risk, from FUND, FINANCIAL METRICS and RESEARCH SUMMARY"
    } else if is_fund {
        "the fund's cost, exposure and risk, from FINANCIAL METRICS and RESEARCH SUMMARY"
    } else {
        "the holding's financial condition, from FINANCIAL METRICS and RESEARCH SUMMARY"
    };
    p.push_str(&format!(
        "\n1. financial_summary — two or three sentences on {summary_scope}.\n"
    ));
    p.push_str(
        "\n2. model_sub_scores — your own quality, valuation, momentum and risk, as integers on \
         the scale defined in COMPUTED SCORES. They may agree with the computed scores or not.\n",
    );
    p.push_str(
        "\n3. model_price_targets — your own one_month and twelve_month bands, each with base, \
         bear and bull as positive prices in USD, bear ≤ base ≤ bull. The computed bands are \
         inputs; your bands may agree with them or differ.\n   \
         model_target_rationale — the assumptions behind your base case; where your \
         twelve-month base differs, name your figure and the computed figure and explain why.\n",
    );
    // The shared horizon definitions less their "<name> term" prefix, so the
    // item reads "short (~1 month)" rather than "short (short term (~1 month))".
    let window = |h: &str| h.split_once(" (").map(|(_, p)| format!("({p}")).unwrap_or_else(|| h.to_string());
    p.push_str(&format!(
        "\n4. horizon_outlook — \"bullish\", \"neutral\" or \"bearish\" for short {}, mid {} and \
         long {}, drawing on MARKET ANALYSIS for the market setup.\n",
        window(HORIZON_SHORT),
        window(HORIZON_MID),
        window(HORIZON_LONG),
    ));
    // 5. ledger — the shared item, the full trigger ladder on this branch.
    p.push_str(&ledger_task_item(
        5,
        contract,
        LedgerItemBranch::priced(is_fund),
        has_prior_ledger,
    ));
    if !debut {
        p.push_str(&what_changed_task_items(
            6,
            "the named score or target horizon",
            true,
        ));
        p.push_str(
            "\n7. conviction — your confidence in this read as a whole: \"low\", \"medium\" or \
             \"high\".\n",
        );
        p.push_str(
            "\n8. self_assessment — your prior read against the computed read and what happened \
             since, from PRIOR ANALYSIS: was it right, was it better than the computed read, \
             and why.\n",
        );
    } else {
        p.push_str(
            "\n6. conviction — your confidence in this read as a whole: \"low\", \"medium\" or \
             \"high\".\n",
        );
        p.push_str(
            "\n7. self_assessment — one sentence noting that this is a first analysis with no \
             prior read to assess.\n",
        );
    }
    p.push_str(&format!(
        "\nRETURN SHAPE (every value is a placeholder; an array holds as many items as apply)\n{}\n",
        crate::portfolio::interpretation_return_shape(is_fund, debut)
    ));
    p
}

/// The message a ledger item is rendered for — the priced stock, the priced
/// fund or the role/risk fund — which fixes the threshold example and the
/// driver clause (a fund's on both fund variants, ruled 2026-09-17), the
/// trigger families (trim and sell on the role/risk branch, whose schema enum
/// drops the add family — `docs/portfolio-analysis.md` §The position thesis
/// ledger), and whether the thesis line cites MARKET ANALYSIS (the priced
/// message's outlook item cites it instead).
#[derive(Clone, Copy)]
enum LedgerItemBranch {
    PricedStock,
    PricedFund,
    RoleRisk,
}

impl LedgerItemBranch {
    /// The priced message's branch for its vehicle kind.
    fn priced(is_fund: bool) -> Self {
        if is_fund {
            Self::PricedFund
        } else {
            Self::PricedStock
        }
    }

    fn is_fund(self) -> bool {
        !matches!(self, Self::PricedStock)
    }

    fn thesis_draws_on_market(self) -> bool {
        matches!(self, Self::RoleRisk)
    }

    fn triggers(self) -> &'static str {
        match self {
            Self::RoleRisk => {
                "pre-committed conditions for trimming or selling, with family \"trim\" or \
                 \"sell\""
            }
            Self::PricedStock | Self::PricedFund => {
                "pre-committed conditions for adding, trimming or selling, with family \"add\", \
                 \"trim\" or \"sell\""
            }
        }
    }
}

/// The ledger item of Part 2, shared by the priced and role/risk messages
/// (`portfolio-v42`): the parts in output order, the quant contract as
/// requirements on the output — the label, comparator, threshold and margin;
/// the statement agreeing with the core; one level, no qualifier; null
/// otherwise — with threshold and margin in one clause each, the margin sized
/// by example and its caps unshown (`portfolio-v40`), and the two worked
/// examples in the vehicle's vocabulary; the branch fixes the fund form, the
/// families and the market-analysis reference. Every rule the text no longer
/// explains is still enforced at the 6g seam.
fn ledger_task_item(
    number: u8,
    contract: &LedgerSeriesContract,
    branch: LedgerItemBranch,
    has_prior_ledger: bool,
) -> String {
    let is_fund = branch.is_fund();
    let mut p = String::new();
    let labels = contract
        .rows
        .iter()
        .map(|r| r.series.as_kebab())
        .collect::<Vec<_>>()
        .join(", ");
    if has_prior_ledger {
        p.push_str(&format!(
            "\n{number}. ledger — the position's thesis ledger, rewritten from PRIOR THESIS LEDGER \
             against this analysis's inputs. Keep a condition's series, comparator, threshold \
             and margin unless the condition itself has changed. The thesis is the current \
             thesis; the original is kept separately.\n",
        ));
    } else {
        p.push_str(&format!("\n{number}. ledger — the position's initial thesis ledger:\n"));
    }
    p.push_str(&format!(
        "   - thesis: the standing thesis, in a few sentences{}.\n   \
         - key_drivers: what the thesis depends on{}. Where a driver is one of the labelled \
         metrics in FINANCIAL METRICS, series is its label; otherwise series is null.\n   \
         - base, bear, bull: the conditions that define each case, with a probability in \
         percent; the three sum to about 100.\n   \
         - what_must_improve: what has to improve for the bull case. what_must_not_break: \
         what has to hold for the base case.\n   \
         - falsifiers: the observations that would show the thesis wrong. technology_class \
         is true only for a third party's technology event (a competitor's or supplier's \
         product or standard) and false otherwise.{}\n   \
         - triggers: {}.{}\n\n   \
         Every falsifier and trigger has a quant field.\n   \
         A condition on one labelled metric is quantitative: quant holds series (one of {labels}), \
         comparator (\"below\" or \"above\"), threshold and margin, and statement is a short name \
         for the condition, without a figure. It is a single level on a single metric, {}, with \
         no duration, volume or second condition (\"for two weeks\", \"on elevated volume\", \
         \"unless …\").\n   \
         Any other condition is qualitative: quant is null, and statement is the observation \
         itself, specific enough to be researched. A condition that needs a duration, volume or \
         second condition is qualitative.\n   \
         threshold: the level, in the metric's unit ({}).\n   \
         margin: the noise around the threshold that a crossing must clear, in the same unit — \
         small relative to the level, for example {}.\n   \
         {}\n",
        if branch.thesis_draws_on_market() {
            ", drawing on MARKET ANALYSIS for the market setup"
        } else {
            ""
        },
        if is_fund {
            " — for a fund, the exposure it supplies, its cost and its fidelity to its mandate"
        } else {
            ""
        },
        if has_prior_ledger {
            " tripped is true only where CONDITION CROSSINGS THIS RUN shows a confirmed crossing for \
             that condition, or, for a qualitative condition, where a finding in CHANGES SINCE \
             THE PRIOR ANALYSIS marked research-supported evidences it; otherwise false."
        } else {
            " tripped is false."
        },
        branch.triggers(),
        if has_prior_ledger {
            " fired follows the same rule as tripped."
        } else {
            " fired is false."
        },
        // A kept condition stays as it is even after a crossing — its streak is
        // the point; only a new one must sit ahead of the metric (Codex,
        // 2026-09-18, the rendered-ledger slice).
        if has_prior_ledger {
            "a new one at a level the metric has not already crossed and a kept one unchanged even \
             after a crossing"
        } else {
            "one the metric has not already crossed"
        },
        if is_fund {
            "\"above 0.75%\" on expense-ratio is 0.0075"
        } else {
            "\"below 16%\" on gross-margin is 0.16"
        },
        if is_fund {
            "2 on a price of 100, 0.002 on a daily volatility of 0.02, or 0.0005 on an \
             expense ratio of 0.0075"
        } else {
            "2 on a price of 100, 0.005 on a net margin of 0.16, or 1 on a P/E of 25"
        },
        contract.examples(),
    ));
    p
}

/// The two continuity items of Part 2, shared by both messages (`portfolio-v42`):
/// what_changed_entries and what_changed, numbered from `first`. `detail` is the
/// branch's gloss on which value a row names; `parameter_sentence` adds the
/// priced branch's parameter-boundary attribution, which the role/risk branch
/// has no source for. The requirements stand on the output — no validator
/// behaviour is described.
fn what_changed_task_items(first: u8, detail: &str, parameter_sentence: bool) -> String {
    format!(
        "\n{first}. what_changed_entries — one row per intrinsic value that moved since the prior \
         analysis: kind (which kind of value, from the alternatives in the shape), detail \
         (which one — {detail}), old and new (the value before and \
         after), attribution, and evidence. An attribution of market-data, \
         company-information or research-narrative cites one bracketed id from CHANGES \
         SINCE THE PRIOR ANALYSIS, or that entry's text verbatim, in evidence; a revision of \
         your own prior read with no new fact is attribution self-correction with evidence \
         empty.{} A thesis or \
         scenario-weights row is for a material change to the standing thesis, never a \
         rephrasing. No row repeats another, and no row has old equal to new.\n   \
         what_changed — one sentence summarizing those rows.\n",
        if parameter_sentence {
            " A move noted in PRIOR ANALYSIS as caused by a parameter change is attributed \
             to that change, not to the company or to a self-correction."
        } else {
            ""
        }
    )
}

/// FINANCIAL METRICS, shared by the priced and role/risk messages
/// (`portfolio-v42`): the basis line, the label-and-confirmation sentence, one
/// line per computable series with its ledger label, unit gloss and confirmation
/// rule, and the data gaps — so the ledger item in Part 2 points here by name
/// and no metric renders twice.
fn financial_metrics_section(
    contract: &LedgerSeriesContract,
    d: &HoldingDossier,
    is_fund: bool,
) -> String {
    let mut p = String::from("\nFINANCIAL METRICS\n");
    p.push_str(&statement_basis_line(
        d.financials.statement_basis,
        d.financials.equity_source,
        is_fund,
    ));
    p.push_str(
        "Each metric has a label in brackets and a confirmation rule, the number of prints \
         past a level that count as a crossing; both are used by the ledger in Part 2.\n",
    );
    p.push_str(&contract.metric_lines());
    if !d.financials.gaps.is_empty() {
        p.push_str(&format!("Data gaps: {}\n", d.financials.gaps.join("; ")));
    }
    p
}

fn opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.3}")).unwrap_or_else(|| "(gap)".to_string())
}

/// The expense-ratio prompt render — one shared formatter so the role-risk,
/// interpretation, and action prompts state the value identically. The decimal
/// fraction stays primary because it is the ledger's unit
/// (`LedgerSeries::ExpenseRatio` is declared to the model as a decimal, and the
/// debut falsifier's threshold is authored in it); the percent reading rides
/// beside it so the number is legible without the legend's arithmetic. Four
/// places is one basis point — the resolution expense ratios are usually
/// quoted at — and a nonzero ratio that would round to zero extends its
/// precision instead, up to ten places, so a ratio prints as free only below
/// 5e-11. `opt()`'s three places flattened a 0.03% fund to `0.000`
/// (large-scale review 2026-08-24, Priority-1 minor).
fn fmt_expense_ratio(v: Option<f64>) -> String {
    let Some(x) = v else {
        return "(gap)".to_string();
    };
    let places = render_places(x);
    let pct = places - 2;
    format!("{x:.places$} ({:.pct$}%/yr)", x * 100.0)
}

/// The decimal places a prompt-rendered value takes: four (one basis point),
/// extended up to ten where a nonzero value would otherwise round to zero —
/// the expense-ratio render's own rule, shared so every site that prints a
/// ledger-unit value states it at the same precision.
fn render_places(x: f64) -> usize {
    if x == 0.0 {
        4
    } else {
        (4..=10)
            .find(|p| (x * 10f64.powi(*p as i32)).round() != 0.0)
            .unwrap_or(10)
    }
}

/// The ledger-crossing prompt render — the observed value and the threshold
/// as one pair at one shared precision, for both sites that print a crossing
/// (the input-delta entry and the 6f ENGINE CONDITION CROSSINGS section).
/// `ConditionCrossing` carries no series, so the rule is series-agnostic:
/// four places had flattened a sub-basis-point expense ratio to `0.0000`
/// while the direct render extended its precision, and the two sites had
/// printed the threshold at two precisions in one prompt (the 2026-08-24
/// review's Codex I12). The precision is **comparison-safe** (the group's
/// Codex round 1): it starts at the pair's [`render_places`] floor and
/// extends, to ten places, until the rendered pair orders as the values do
/// — `0.00006` against `0.00005` had rendered `0.0001` against `0.0001`,
/// a real crossing shown as equality. The test is on the rendered pair read
/// back as numbers, never on the strings: the two must order as the values
/// do, so `-0.0000000000` beside `0.0000000000` — distinct strings that read
/// as equal — is refused (the group's Codex round 3). Rounding is monotone,
/// so a pair that orders correctly never inverts. Two distinct values still
/// alike at ten places fall back to the shortest round-trip render (`{}`),
/// which differs for any two distinct `f64`s and keeps their order — the
/// engine's comparison is exact and a zero margin is valid, so a crossing
/// can sit closer than that (the group's Codex round 2).
fn fmt_crossing_pair(observed: f64, threshold: f64) -> (String, String) {
    comparison_safe_pair(observed, threshold, 4)
}

/// The IV-skew prompt render — the put-minus-call difference with an explicit
/// sign, so the convention the options-activity line states beside it reads
/// off the number itself. `opt()` had printed the value bare while
/// put-minus-call lived only in a doc comment, so a model assuming the
/// inverse read hedging demand as call speculation (large-scale review
/// 2026-08-24, Priority-1 minor). The sign keys on the rendered three-place
/// value: a skew that rounds away prints `0.000`, never `+0.000`, since a sign
/// would assert a put premium the number no longer shows. The convention text
/// is the line's label, not this value's, and renders beside a `(gap)` too.
fn fmt_iv_skew(v: Option<f64>) -> String {
    let Some(x) = v else {
        return "(gap)".to_string();
    };
    let shown = (x * 1000.0).round() / 1000.0;
    if shown == 0.0 {
        "0.000".to_string()
    } else {
        format!("{shown:+.3}")
    }
}

/// The closed-end price-vs-NAV prompt line — one shared render so the
/// interpretation, action, and priced-fund prompts state the read identically
/// (`docs/portfolio-analysis.md` §Asset eligibility: signal on the closed-end
/// form only; callers gate on the CEF marker). The label follows the RENDERED
/// tenth-of-a-percent, so a value displaying as 0.0% reads "at par" — never
/// "+0.0% premium" or "-0.0% discount" (Codex 2026-08-21 round 3, finding 2).
fn nav_premium_line(premium: f64) -> String {
    let rounded = (premium * 1000.0).round() / 1000.0;
    let (value, word) = if rounded == 0.0 {
        ("0.0%".to_string(), "at par")
    } else {
        (
            format!("{:+.1}%", rounded * 100.0),
            if rounded > 0.0 { "premium" } else { "discount" },
        )
    };
    // A unit gloss only, on every packet that renders the line (ruled
    // 2026-09-17, `portfolio-v42`): what the number is, not what it means.
    format!(
        "PRICE VS NAV: {value} ({word}): the closed-end fund's market price against its \
         net asset value.\n",
    )
}

/// The system prompt for the **per-holding action call** (`portfolio-v41`):
/// the role, the two-part shape of the message and the output names — the
/// same footing as [`interpretation_system_prompt`]. The ladder, the
/// one-sentence rationale and the profile tie-break are the message's Part 2;
/// the app's words about arms, evidence and departures are gone
/// (`docs/portfolio-analysis.md` §Portfolio action).
pub fn action_system_prompt() -> String {
    format!(
        "You are an equity analyst deciding the portfolio action for one holding in a \
         portfolio review. Part 1 of the message gives the inputs. Part 2 states what to \
         determine from them and the shape to return. {}",
        crate::portfolio::action_response_contract()
    )
}

/// The one sentence Part 1 opens with on both branches: what the two labels
/// mean, once (ruled 2026-09-17: "computed" and "analyst"; the packet does not
/// say the analyst's read came from the same model).
const TWO_READS: &str = "Two reads of this holding appear below: a computed read, derived from \
its financial data by fixed formulas, and an analyst's read of the same data and research.\n";

/// The gloss beside a computed grade resting on an imputed sub-score — one
/// string for the interpretation and action packets.
const LOW_CONFIDENCE_GLOSS: &str = " (low-confidence: one score is imputed)";

/// The three outlook words a packet prints, short / mid / long.
fn horizon_words(o: &HorizonOutlook) -> [&'static str; 3] {
    [o.short.as_str(), o.mid.as_str(), o.long.as_str()]
}

/// The action message (`portfolio-v41`): Part 1 the inputs, each section
/// explained once and then its values with no instruction in it; Part 2 the
/// task in output order and the placeholder-only shape. No arm, baseline,
/// stage, seam, validator behaviour, stamp or product name — the rulings and
/// the rendered draft are `docs/verification/2026-09-17-action-prompt-rewrite.md`.
/// Tunnel vision is enforced by input isolation: no whole-book field exists
/// here (`docs/portfolio-analysis.md` §Portfolio action).
pub fn action_user_prompt(input: &ActionInput) -> String {
    let d = input.dossier;
    let mut p = String::from("======== PART 1: INPUTS ========\n");
    p.push_str(&holding_header(d));
    p.push_str(&format!("\n{TWO_READS}"));
    match &input.subject {
        ActionSubject::Priced { graded, engine, pre_profit, ledger } => {
            p.push_str(&scores_section(graded));
            p.push_str(&price_targets_section(d, graded, engine));
            p.push_str(&format!(
                "\nTARGET RATIONALE (analyst)\n{}\n",
                graded.model_target_rationale
            ));
            p.push_str(&capital_efficiency_section(&engine.hurdle));
            let [short, mid, long] = horizon_words(&graded.horizon_outlook);
            p.push_str(&format!(
                "\nCONVICTION AND OUTLOOK (analyst)\nConviction {}: the analyst's confidence in \
                 the read as a whole. Outlook: short (about 1 month) {short}, mid (about 1 year) \
                 {mid}, long (3–5 years) {long}.\n",
                graded.conviction.as_str(),
            ));
            p.push_str(&format!(
                "\nFINANCIAL SUMMARY (analyst)\n{}\n",
                graded.financial_summary
            ));
            p.push_str(&ledger_prose_sections(ledger));
            p.push_str(&action_continuity_sections(d, &graded.what_changed, input.changes));
            if let Some(overlay) = pre_profit {
                p.push_str(&pre_profit_prompt_section(overlay, PromptStage::Action));
            }
        }
        ActionSubject::RoleRisk { verdict, ledger } => {
            p.push_str(&format!("\nCLASS (computed)\n{}\n", verdict.class_label));
            p.push_str(&format!("\nROLE (analyst)\n{}\n", verdict.role_summary));
            if !verdict.exposure_tilt.is_empty() {
                let tilt: Vec<String> = verdict
                    .exposure_tilt
                    .iter()
                    .take(5)
                    .map(|w| format!("{} {:.0}%", w.label, w.weight * 100.0))
                    .collect();
                p.push_str(&format!("\nEXPOSURE TILT (computed)\n{}\n", tilt.join(", ")));
            }
            p.push_str(&format!(
                "\nRISK PROFILE (computed)\nExpense drag: {} of assets per year. \
                 Observable risk: {} (annualized realized volatility). Structural flag \
                 (leveraged / inverse or option-overlay path dependency): {}.\n",
                fmt_expense_ratio(verdict.expense_drag),
                opt(verdict.observable_risk),
                if verdict.structural_flag { "yes" } else { "no" },
            ));
            // The closed-end read, where present — its absence is a named gap in
            // the evidence-gap list below, never a fabricated number.
            if verdict.is_cef {
                if let Some(prem) = verdict.nav_premium {
                    p.push('\n');
                    p.push_str(&nav_premium_line(prem));
                }
            }
            if !verdict.evidence_gaps.is_empty() {
                p.push_str(&format!(
                    "\nEVIDENCE GAPS (computed)\n{}\n",
                    verdict.evidence_gaps.join("; ")
                ));
            }
            p.push_str(&ledger_prose_sections(ledger));
            p.push_str(&action_continuity_sections(d, &verdict.what_changed, input.changes));
        }
    }
    p.push_str(&forensic_prompt_section(d, PromptStage::Action));
    p.push_str(&commodity_prompt_section(d));
    p.push_str(&option_overlay_prompt_section(d));
    // The computed per-holding set as one data line (fix list 3.9, ruled
    // 2026-09-17: no permission sentence — the return shape's enum shows the
    // ladder). An outside-the-set rung persists as authored with the departure
    // on the audit (`outside_set_annotation`), never a bar.
    let set: Vec<&str> = input.engine_set.iter().map(Action::as_kebab).collect();
    p.push_str(&format!(
        "\nSUPPORTED ACTIONS (computed)\nThe rungs the computed read supports on its own: {}.\n",
        set.join(", ")
    ));
    // The cash row is deliberately not rendered: available capital is
    // whole-book context — the planner's domain — kept out of this
    // tunnel-vision call by input isolation (Codex 2026-08-14, finding 3). The
    // tax row is not rendered either: the tax posture is read by the app after
    // the rung is fixed (`with_tax_caveat`), never by the decision.
    let profile = input.profile.display();
    p.push_str(&format!(
        "\nINVESTOR PROFILE\n- objective: {}\n- risk tolerance: {}\n- horizon: {}\n",
        profile.objective, profile.risk_tolerance, profile.horizon,
    ));
    p.push_str(&action_task_section(input));
    p
}

/// SCORES: the polarity gloss once, the grade's derivation once (fix list 2.3
/// and 3.11 folded into one data gloss), then the computed and analyst rows.
fn scores_section(graded: &GradedVerdict) -> String {
    let e = &graded.sub_scores;
    let m = &graded.model_view.sub_scores;
    format!(
        "\nSCORES\nFour scores from 0 to 100, higher is better on every axis: quality; \
         valuation, where higher means more attractive; momentum; risk, where higher means \
         more resilient. The grade is a letter derived from the quality, valuation and risk \
         scores.\n\
         - computed: quality {:.0}, valuation {:.0}, momentum {:.0}, risk {:.0}. Grade {}{}. \
         Risk tier: {}.\n\
         - analyst: quality {:.0}, valuation {:.0}, momentum {:.0}, risk {:.0}. Grade {}.\n",
        e.quality,
        e.valuation,
        e.momentum,
        e.risk,
        graded.grade.as_str(),
        if graded.low_confidence_grade { LOW_CONFIDENCE_GLOSS } else { "" },
        graded.risk_tier.as_str(),
        m.quality,
        m.valuation,
        m.momentum,
        m.risk,
        graded.model_view.letter.as_str(),
    )
}

/// PRICE TARGETS: both reads' bands as prices with the move each implies from
/// the current price, the computed bands with the method clauses the
/// interpretation packet renders (ruled 2026-09-17, in place of the provenance
/// label). A computed band the scenario function could not derive prints
/// "(gap)"; an analyst leg outside its domain, or whose move overflows the
/// percentage arithmetic, prints as authored with "(off-scale as authored)";
/// a band authored bear above bull carries "(band inverted as authored)" —
/// annotate, never reorder, never drop (Codex I5, ruled 2026-08-28). With no
/// usable current price the prices render without moves (unreachable on a
/// priced holding — the quote floor — so the guard stays defensive).
fn price_targets_section(
    d: &HoldingDossier,
    graded: &GradedVerdict,
    engine: &EngineOutput,
) -> String {
    let spot = d.financials.current_price.filter(|s| s.is_finite() && *s > 0.0);
    let mv = |v: f64| spot.map(|s| (v / s - 1.0) * 100.0);
    let leg = |v: f64| match mv(v) {
        Some(m) => format!("{v:.2} ({m:+.1}%)"),
        None => format!("{v:.2}"),
    };
    let analyst_leg = |v: f64| {
        if v.is_finite() && v > 0.0 && mv(v).is_none_or(f64::is_finite) {
            leg(v)
        } else {
            format!("{v} (off-scale as authored)")
        }
    };
    let mut p = String::from(
        "\nPRICE TARGETS (USD, with the move each implies from the current price)\n",
    );
    match &graded.price_targets.twelve_month {
        Some(t) => p.push_str(&format!(
            "- computed twelve-month: bear {} / base {} / bull {}. Method: {}.{}\n",
            leg(t.bear),
            leg(t.base),
            leg(t.bull),
            twelve_month_method(&engine.target_meta),
            target_notes_line(&engine.target_meta)
                .map(|n| format!(" Notes: {n}"))
                .unwrap_or_default(),
        )),
        None => p.push_str("- computed twelve-month: (gap)\n"),
    }
    match &graded.price_targets.one_month {
        Some(t) => p.push_str(&format!(
            "- computed one-month: bear {} / base {} / bull {}. Method: {}.\n",
            leg(t.bear),
            leg(t.base),
            leg(t.bull),
            one_month_method(t),
        )),
        None => p.push_str("- computed one-month: (gap)\n"),
    }
    let m = &graded.model_view.price_targets;
    for (label, t) in [("twelve-month", &m.twelve_month), ("one-month", &m.one_month)] {
        p.push_str(&format!(
            "- analyst {label}: bear {} / base {} / bull {}{}.\n",
            analyst_leg(t.bear),
            analyst_leg(t.base),
            analyst_leg(t.bull),
            if t.bear > t.bull { " (band inverted as authored)" } else { "" },
        ));
    }
    p
}

/// CAPITAL EFFICIENCY as numbers (ruled 2026-09-17 off the v39 read's L19): the
/// three tested twelve-month total returns and the hurdle rate with one gloss —
/// no state word, no reach sentence. The sunk-cost rule is one clause of the
/// task. An unscorable read, or one missing a figure, says no assessment exists.
fn capital_efficiency_section(h: &engine::HurdleRead) -> String {
    let scorable = h.state != crate::portfolio::HurdleState::Unscorable;
    match (scorable, h.hurdle_rate, h.tr_bear, h.tr_base, h.tr_bull) {
        (true, Some(rate), Some(bear), Some(base), Some(bull)) => format!(
            "\nCAPITAL EFFICIENCY\nThe computed twelve-month total return in each scenario (the \
             move from the current price to the scenario price, plus forward income per share, \
             as a fraction of the current price) and the hurdle rate it is measured against.\n\
             bear {:+.1}% / base {:+.1}% / bull {:+.1}%; hurdle {:.1}%.\n",
            bear * 100.0,
            base * 100.0,
            bull * 100.0,
            rate * 100.0,
        ),
        _ => "\nCAPITAL EFFICIENCY\nNo assessment this run.\n".to_string(),
    }
}

/// THESIS and SCENARIOS from the holding's ledger as validated this run (ruled
/// 2026-09-17): the standing thesis, then each case's conditions with the
/// analyst's probability for it. The must-improve / must-not-break lines stay
/// out (ruled the same day).
fn ledger_prose_sections(ledger: &ThesisLedger) -> String {
    let mut p = format!("\nTHESIS (analyst)\n{}\n", ledger.current_thesis);
    p.push_str(
        "\nSCENARIOS (analyst)\nThe conditions that define each case, with the analyst's \
         probability for it.\n",
    );
    for s in &ledger.monitor {
        p.push_str(&format!(
            "- {} ({:.0}%): {}\n",
            s.scenario.as_str(),
            s.probability_pct,
            s.conditions
        ));
    }
    p
}

/// The continuity sections, rendered only with a prior verdict: PRIOR ACTION
/// (the rung, glossed as chosen in the prior analysis or set by rule after it —
/// ruled 2026-09-17, F4), PRIOR ANALYSIS (the prior read's values) and CHANGES
/// SINCE THE PRIOR ANALYSIS (the analyst's summary, the validated rows in
/// words, the input-delta rows they cite). Absent attribution says so and is
/// never read as unchanged evidence.
fn action_continuity_sections(
    d: &HoldingDossier,
    summary: &str,
    changes: Option<&crate::portfolio::WhatChangedAudit>,
) -> String {
    let Some(prior) = d.prior_verdict.as_ref() else {
        return String::new();
    };
    let mut p = String::new();
    if let Some(action) = crate::portfolio::carried_action(prior) {
        p.push_str(&format!(
            "\nPRIOR ACTION\n{}, {}.\n",
            action.as_kebab(),
            match prior.action_source {
                ActionSource::ModelChosen => "chosen in the prior analysis",
                ActionSource::RuleDemoted => {
                    "set by rule after the prior analysis, not chosen in it"
                }
            }
        ));
    }
    p.push_str("\nPRIOR ANALYSIS\n");
    match &prior.disposition {
        VerdictDisposition::Priced(g) => {
            let [short, mid, long] = horizon_words(&g.horizon_outlook);
            p.push_str(&format!(
                "computed grade {}; analyst grade {}; conviction {}; outlook short {short}, \
                 mid {mid}, long {long}.\nFinancial summary: {}\n",
                g.grade.as_str(),
                g.model_view.letter.as_str(),
                g.conviction.as_str(),
                g.financial_summary
            ))
        }
        VerdictDisposition::RoleRiskOnly(r) => {
            p.push_str(&format!("class {}; role: {}\n", r.class_label, r.role_summary))
        }
        _ => p.push_str("No comparable prior read.\n"),
    }
    p.push_str(&format!(
        "\nCHANGES SINCE THE PRIOR ANALYSIS\nSummary (analyst): {summary}\n"
    ));
    match changes {
        Some(c) => {
            for e in &c.entries {
                let attribution = e.attribution.as_words();
                let evidence = if e.evidence.trim().is_empty() {
                    String::new()
                } else {
                    format!("; evidence {}", e.evidence)
                };
                p.push_str(&format!(
                    "- {}: {} -> {} ({attribution}{evidence})\n",
                    e.detail, e.old, e.new
                ));
            }
            for e in &c.input_delta {
                p.push_str(&format!("[{}] {}\n", e.id, e.label));
            }
            if c.entries.is_empty() {
                p.push_str("No attributed changes are recorded.\n");
            }
        }
        None => p.push_str("No change attribution is available.\n"),
    }
    p
}

/// Part 2 of the action message: the two items in output order, each naming
/// the Part 1 sections it draws on — the weighing order (the former ACTION
/// BASIS hierarchy as a task clause), the profile tie-break, on a priced
/// holding the sunk-cost rule as one clause on every packet
/// (`docs/portfolio-analysis.md` §Portfolio action; ruled 2026-09-17), with a
/// chosen prior the firmness clause — and the placeholder-only shape.
fn action_task_section(input: &ActionInput) -> String {
    let prior = input.dossier.prior_verdict.as_ref();
    let prior_chosen = prior.is_some_and(|v| {
        v.action_source == ActionSource::ModelChosen
            && crate::portfolio::carried_action(v).is_some()
    });
    let mut p = String::from(
        "\n======== PART 2: TASK ========\n\nDetermine the following from the inputs and return \
         them as one JSON object in the shape at the end, with no code fence and no \
         surrounding text.\n\
         \n1. action — one rung for this holding, from these inputs alone: \"sell-all\", \
         \"trim\", \"hold\", \"add\" or \"add-aggressively\". The rung alone: no share count, \
         dollar amount or portfolio weight. ",
    );
    let (first, mut refining): (Vec<&str>, Vec<&str>) = match &input.subject {
        ActionSubject::Priced { .. } => (
            vec!["SCORES", "PRICE TARGETS"],
            vec!["CAPITAL EFFICIENCY", "CONVICTION AND OUTLOOK", "THESIS", "SCENARIOS"],
        ),
        ActionSubject::RoleRisk { verdict, .. } => {
            let mut first = vec!["CLASS", "ROLE"];
            if !verdict.exposure_tilt.is_empty() {
                first.push("EXPOSURE TILT");
            }
            first.push("RISK PROFILE");
            if verdict.is_cef && verdict.nav_premium.is_some() {
                first.push("PRICE VS NAV");
            }
            let mut refining = Vec::new();
            if !verdict.evidence_gaps.is_empty() {
                refining.push("EVIDENCE GAPS");
            }
            refining.extend(["THESIS", "SCENARIOS"]);
            (first, refining)
        }
    };
    if prior.is_some() {
        refining.extend(["PRIOR ANALYSIS", "CHANGES SINCE THE PRIOR ANALYSIS"]);
    }
    refining.extend(["SUPPORTED ACTIONS", "INVESTOR PROFILE"]);
    let list = |names: Vec<&str>| -> String {
        match names.split_last() {
            Some((last, [])) => last.to_string(),
            Some((last, head)) => format!("{} and {last}", head.join(", ")),
            None => String::new(),
        }
    };
    p.push_str(&format!(
        "Decide it from {} first, refined by {}. An aggressive risk tolerance admits \
         add-aggressively where the other inputs support it.",
        list(first),
        list(refining),
    ));
    match &input.subject {
        ActionSubject::Priced { .. } => p.push_str(
            " Where even the bull case in CAPITAL EFFICIENCY misses the hurdle and the forward \
             read is poor, lean toward realizing some or all of the position.",
        ),
        ActionSubject::RoleRisk { .. } => p.push_str(
            " An add-side rung needs support from the vehicle's own attributes, stated in the \
             rationale.",
        ),
    }
    if prior_chosen {
        p.push_str(
            " Move from PRIOR ACTION only where the inputs have materially changed since the \
             prior analysis.",
        );
    }
    p.push_str(
        "\n\n2. rationale — one sentence giving the single investment reason for the rung.",
    );
    if matches!(&input.subject, ActionSubject::Priced { .. }) {
        p.push_str(" Name the returns you weighed by their values; do not describe them by their relation to another figure.");
    }
    p.push('\n');
    p.push_str(&format!(
        "\nRETURN SHAPE (every value is a placeholder)\n{}\n",
        crate::portfolio::action_return_shape()
    ));
    p
}

/// Which packet a shared section is rendered into. The facts are the same on
/// both; the consequence lines (the overlay's and the forensic sweep's) render
/// on the interpretation packet only — the action packet's SUPPORTED ACTIONS
/// line already carries the narrowed set, and its conviction is the analyst's
/// read (ruled 2026-09-17, F1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptStage {
    /// The intrinsic interpretation prompt (Step 6f): authors conviction, no action.
    Interpretation,
    /// The per-holding action call: authors the rung, no conviction.
    Action,
}

/// Render the finalized pre-profit execution / financing overlay for an eligible
/// stock's interpretation prompt and its per-holding action prompt
/// (`docs/portfolio-workflow.md` §Step 6f): the computed states as data on both,
/// the consequence lines on the interpretation packet only (see [`PromptStage`]).
fn pre_profit_prompt_section(o: &PreProfitOverlay, stage: PromptStage) -> String {
    use crate::portfolio::pre_profit::{ConvictionCeiling, FinancingState};
    let i = &o.statement_inputs;
    let mut p = String::new();
    p.push_str("\nPRE-PROFIT EXECUTION AND FINANCING\n");
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
    // The consequence lines state the computed rule's effect; the action packet
    // carries the narrowed set on its own line, so they render on the
    // interpretation packet only (ruled 2026-09-17, F1).
    if stage == PromptStage::Action {
        return p;
    }
    if let Some(ceiling) = o.consequences.conviction_ceiling {
        let ceiling = match ceiling {
            ConvictionCeiling::Medium => "medium",
            ConvictionCeiling::Low => "low",
        };
        let rules = o.consequences.matched_rules.join("; ");
        p.push_str(&format!(
            "- computed conviction capped at {ceiling} by rule: {rules}.\n",
        ));
    }
    if o.consequences.exit_family_only {
        p.push_str(
            "- severe deterioration: the computed action set narrows to trim and sell-all.\n",
        );
    } else if o.consequences.bar_add_family {
        p.push_str(
            "- the computed action set excludes adding, on the financing rule.\n",
        );
    }
    p
}

/// One row of the ledger-authoring contract: a series the engine computes for this
/// holding's vehicle kind, with its current observation — or the typed reason it
/// is unavailable this run — where the caller supplied the computed surface.
pub struct SeriesContractRow {
    pub series: engine::LedgerSeries,
    pub observation: Option<std::result::Result<engine::ResolvedObservation, String>>,
    /// The computed value as the metric line prints it — present even where the
    /// observation could not be keyed (no dated print), since the value is the
    /// input and the identity is the seam's.
    pub value: Option<f64>,
}

/// The holding-scoped ledger-authoring contract (`docs/portfolio-analysis.md`
/// §The position thesis ledger; ruled 2026-09-16 off attempt-6 Finding 2): only
/// the series the engine computes for this vehicle kind, each rendered with its
/// unit, its current observation and its confirmation cadence, so the model
/// authors a level in the series' own units against a value it can see and a
/// fund never sees a stock-only series.
pub struct LedgerSeriesContract {
    pub is_fund: bool,
    pub rows: Vec<SeriesContractRow>,
}

impl LedgerSeriesContract {
    /// Build the contract for a vehicle kind. `metrics` and `fin` are the
    /// computed surface the observations resolve against; either absent renders
    /// the rows without observations (the offline tests' form).
    pub fn build(
        is_fund: bool,
        metrics: Option<&engine::ComputedMetrics>,
        fin: Option<&engine::CompanyFinancials>,
    ) -> Self {
        let rows = engine::LedgerSeries::ALL
            .iter()
            .copied()
            .filter(|s| s.computable_for(is_fund))
            .map(|series| SeriesContractRow {
                series,
                observation: match (metrics, fin) {
                    (Some(m), Some(f)) => Some(engine::resolve_series(series, m, f)),
                    _ => None,
                },
                value: match (series, metrics, fin) {
                    (engine::LedgerSeries::Price, _, Some(f)) => {
                        f.current_price.filter(|p| p.is_finite() && *p > 0.0)
                    }
                    (engine::LedgerSeries::Price, _, None) => None,
                    (s, Some(m), _) => s.metric_value(m),
                    (_, None, _) => None,
                },
            })
            .collect();
        Self { is_fund, rows }
    }

    /// The metric lines every interpretation message carries under FINANCIAL
    /// METRICS (`portfolio-v40`): one per computable series — the value, its
    /// ledger label in brackets, its unit gloss and its confirmation rule — so the
    /// ledger item points here by name and no series list is rendered twice.
    /// A missing value prints "(gap)"; an observation the seam could not key
    /// (no dated print) still shows the computed value, since the value is the
    /// input and the identity is the seam's.
    pub fn metric_lines(&self) -> String {
        let mut p = String::new();
        for row in &self.rows {
            let s = row.series;
            let value = match row.value {
                Some(v) if s == engine::LedgerSeries::ExpenseRatio => fmt_expense_ratio(Some(v)),
                Some(v) if s == engine::LedgerSeries::Price => format!("{v:.2}"),
                Some(v) => format!("{v:.4}"),
                None => "(gap)".to_string(),
            };
            p.push_str(&format!(
                "- {} [{}]: {value} — {}; {}\n",
                s.describe(),
                s.as_kebab(),
                s.unit_note(),
                s.confirmation_note()
            ));
        }
        p
    }

    /// Two branch-scoped worked examples — illustrative shapes in the vehicle's
    /// own vocabulary, never findings.
    fn examples(&self) -> &'static str {
        if self.is_fund {
            "Example, quantitative: statement \"Price support\", quant {\"series\": \
             \"price\", \"comparator\": \"below\", \"threshold\": 38, \"margin\": 0.4}.\n   \
             Example, qualitative: statement \"The mandate drifts from the stated index \
             methodology\", quant null."
        } else {
            "Example, quantitative: statement \"Gross-margin floor\", quant \
             {\"series\": \"gross-margin\", \"comparator\": \"below\", \"threshold\": 0.16, \
             \"margin\": 0.005}.\n   \
             Example, qualitative: statement \"A credible second supplier ships at scale\", \
             quant null."
        }
    }
}

/// The prior ledger and this run's crossings as data (`portfolio-v40`): the
/// standing thesis, drivers, monitor, conditions with their cores and streaks,
/// the research-supported marks, and the crossings — rendered into Part 1 of
/// both interpretation messages (`docs/portfolio-analysis.md` §The position
/// thesis ledger). This is the first prior-run *content* the message carries —
/// the standing view the model tests against fresh evidence rather than
/// re-deriving from scratch. On a first analysis it says there is no prior
/// ledger and nothing else.
pub(crate) fn prior_ledger_data_section(
    prior: Option<&ThesisLedger>,
    eval: Option<&LedgerEvaluation>,
    input_delta: &[crate::portfolio::DeltaEntry],
) -> String {
    let mut p = String::new();
    // The conditions a fresh research finding bears on this run — the delta's
    // tied research entries (`push_research_delta_entries`), so the model can see
    // which qualitative claims the 6g validator will honor; the ids themselves
    // stay held out of the projection (app-owned bookkeeping).
    let research_supported: std::collections::HashSet<&str> = input_delta
        .iter()
        .filter_map(|e| e.related_condition_id.as_deref())
        .collect();
    match prior {
        Some(l) => {
            p.push_str("\nPRIOR THESIS LEDGER (the standing view this analysis tests)\n");
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
                    .map(|t| format!(" [computed target {t:.2}]"))
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
                        // A refused condition says why in one data phrase, so the
                        // model authors it differently rather than re-issuing the
                        // same core (ruled 2026-09-18).
                        None => match &c.downgraded_reason {
                            Some(reason) => format!("qualitative; {}", refusal_phrase(reason)),
                            None => "qualitative".to_string(),
                        },
                    };
                    let support = if research_supported.contains(c.condition_id.as_str()) {
                        " — research-supported: a finding in CHANGES SINCE THE PRIOR ANALYSIS \
                         bears on this condition"
                    } else {
                        ""
                    };
                    // A kept core prints raw beside the model's own name (an
                    // empty one where the name was blank — never its render, which
                    // the model would echo back as the name); a qualitative or
                    // refused condition prints its statement.
                    let text = match (&c.quant, &c.label) {
                        (Some(_), Some(label)) => label.as_str(),
                        (Some(_), None) => "",
                        (None, _) => c.statement.as_str(),
                    };
                    p.push_str(&format!("- {family}[{kind}] {text}{support}\n"));
                }
            }

            p.push_str("\nCONDITION CROSSINGS THIS RUN\n");
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
                    let (observed, threshold) = fmt_crossing_pair(c.observed_value, c.threshold);
                    p.push_str(&format!(
                        "- {what}: '{}' — observed {observed} vs threshold {threshold} (observation {})\n",
                        c.statement, c.observation_id
                    ));
                }
                for u in &e.unevaluable {
                    any = true;
                    p.push_str(&format!("- unevaluable this run: {u}\n"));
                }
            }
            if !any {
                p.push_str("- none crossed\n");
            }
        }
        None => {
            p.push_str("\nPRIOR THESIS LEDGER\nNone: this is the first analysis.\n");
        }
    }
    p
}

/// The ledger section's statement-basis line — the one place the prompt says
/// which basis the flow series stand on this run, so a flow-series threshold is
/// authored on the basis it will be evaluated against. The flow family is read off
/// `LedgerSeries::flow_basis` and the balance-sheet instants off
/// `statement_derived` less it — never a second list — and the instants are named
/// as instants, since debt / equity and price / book read the latest balance sheet
/// on either basis (Codex round 1). The basis is the holding's `statement_basis`,
/// stamped at `dossier::apply_ttm_statement_basis` and settled by the SEC merge.
/// `None` — no flow lines this run (a fund, a stock whose statement surface
/// resolved to nothing, or a balance-sheet instant standing alone, FMP's or an
/// equity-only SEC fill) — says so:
/// the flow series are unevaluable here rather than silently on some basis, while
/// an instant still reads where a balance sheet exists
/// (`docs/portfolio-analysis.md` §Starting parameters; large-scale review
/// 2026-08-24, Priority-1 minor). The instants' sentence names which balance
/// sheet supplied their equity this run (`equity_source`, stamped at the SEC
/// merge — Codex I13, `portfolio-v23`): the source is the instants' own
/// continuity stamp, so the model reads what the evaluation gates on; `None` —
/// no equity line reached the engine — says the instants are unevaluable here.
fn statement_basis_line(
    basis: Option<crate::portfolio::StatementBasis>,
    equity_source: Option<crate::portfolio::EquitySource>,
    is_fund: bool,
) -> String {
    if is_fund {
        return "The market metrics are daily; the expense ratio is the fund's published figure.\n"
            .to_string();
    }
    let flow_line = match basis {
        Some(b) => format!(
            "Flow metrics (net margin, gross margin, revenue growth, P/E, P/S) are on a {} basis.",
            b.label()
        ),
        None => "Flow metrics (net margin, gross margin, revenue growth, P/E, P/S) have no \
                 statement basis this run — no income-statement lines were available — so they \
                 are not evaluable here."
            .to_string(),
    };
    let instants_line = match equity_source {
        Some(src) => format!(
            "Balance-sheet metrics (debt / equity, P/B) are from {}.",
            src.label()
        ),
        None => "Balance-sheet metrics (debt / equity, P/B) have no balance sheet this run — no \
                 equity line was available — so they are not evaluable here."
            .to_string(),
    };
    format!("{flow_line} {instants_line}\n")
}

/// The app owns a debut's continuity fields on every analyst path (fix list
/// 3.3, `portfolio-v38`): the model path never requests them and the decoder
/// inserts them, but a stub or any other analyst may author its own line, so
/// the pipeline writes [`crate::portfolio::DEBUT_WHAT_CHANGED`] and an empty
/// row set itself before the verdict is assembled. A continuity call's fields
/// pass through untouched.
fn own_debut_continuity(mut interpretation: Interpretation, debut: bool) -> Interpretation {
    if debut {
        interpretation.what_changed = crate::portfolio::DEBUT_WHAT_CHANGED.to_string();
        interpretation.what_changed_entries.clear();
    }
    interpretation
}

/// The role/risk branch's form of [`own_debut_continuity`].
fn own_debut_continuity_role_risk(
    mut interpretation: RoleRiskInterpretation,
    debut: bool,
) -> RoleRiskInterpretation {
    if debut {
        interpretation.what_changed = crate::portfolio::DEBUT_WHAT_CHANGED.to_string();
        interpretation.what_changed_entries.clear();
    }
    interpretation
}

/// The role/risk verdict assembled from a fresh interpretation: the readout's
/// computed surface app-stamped, the model's role read and continuity line as
/// authored, the action fields placeholders the per-holding action call
/// overwrites (never rendered into that call's prompt). One assembly serves the
/// pipeline and the fixed-evidence harness's synthetic role/risk case
/// (`portfolio-v42`), beside [`graded_verdict_from_interpretation`].
pub(crate) fn role_risk_verdict_from_interpretation(
    readout: &RoleRiskReadout,
    interpretation: RoleRiskInterpretation,
) -> RoleRiskVerdict {
    RoleRiskVerdict {
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
        structural_flag: readout.structural_flag(),
        is_cef: readout.is_cef,
        nav_premium: readout.nav_premium,
        evidence_gaps: readout.evidence_gaps.clone(),
        action: Action::Hold,
        action_rationale: String::new(),
        what_changed: interpretation.what_changed,
    }
}

/// The priced verdict assembled from a fresh interpretation: the engine arm's
/// figures app-stamped from the engine output, the model arm persisted exactly
/// as authored with its letter derived through the shared cutoffs (the two-arm
/// contract — `docs/portfolio-analysis.md` §The holding verdict). The action
/// fields are placeholders the per-holding action call overwrites; they are
/// never rendered into that call's prompt. One assembly serves the pipeline and
/// the fixed-evidence harness, so the harness's fresh-interpretation action
/// call reads the verdict the run would have persisted (the §3 slice's Codex
/// plan review).
pub(crate) fn graded_verdict_from_interpretation(
    engine_output: &EngineOutput,
    options_signal: crate::portfolio::OptionsSignal,
    interpretation: Interpretation,
    engine_view: crate::portfolio::EngineView,
) -> GradedVerdict {
    GradedVerdict {
        grade: engine_output.grade,
        sub_scores: engine_output.sub_scores,
        action: Action::Hold,
        action_rationale: String::new(),
        // The v7 unrestricted contract: the model's conviction persists exactly
        // as authored — no bail, no clamp; a matched pre-profit ceiling stays
        // recorded on the engine view as an annotated divergence.
        conviction: interpretation.conviction,
        horizon_outlook: interpretation.horizon_outlook,
        price_targets: engine_output.price_targets.clone(),
        model_target_rationale: interpretation.model_target_rationale,
        options_signal,
        risk_tier: engine_output.risk_tier,
        dead_money: engine_output.hurdle.state,
        low_confidence_grade: engine_output.low_confidence_grade,
        fund_class_label: engine_output.fund_class_label.clone(),
        financial_summary: interpretation.financial_summary,
        what_changed: interpretation.what_changed,
        model_view: ModelView {
            sub_scores: interpretation.model_sub_scores,
            letter: engine::grade_from_subscores(&interpretation.model_sub_scores),
            price_targets: interpretation.model_price_targets,
            self_assessment: interpretation.self_assessment,
        },
        engine_view,
    }
}

/// A one-line description of the position's change since the prior run, for the
/// interpretation prompt — the direction of the app-computed delta only
/// (`docs/portfolio-analysis.md` §Holdings change tracking), so the model reasons
/// over what the user did with the position — added to, trimmed, or left it —
/// without the quantity or cost-basis figures. Since `portfolio-v38` those
/// figures are account economics the intrinsic packet withholds (fix list 3.2,
/// ruled 2026-09-16 — direction only): a paid-up-versus-averaged-down read is
/// the account's history, not the issuer's condition.
fn describe_position_change(delta: &PositionDelta) -> String {
    match delta.change {
        // "NEW" means new to this run history, nothing more — attempt 2's streams
        // burned large reasoning shares re-litigating "NEW" as if it meant a
        // fresh purchase (`docs/verification/2026-08-13-big-run-attempt-2.md`
        // §Workstream 2).
        PositionChange::New => "This is the first analysis of this holding.".to_string(),
        PositionChange::Unchanged => {
            "The position is unchanged since the prior analysis.".to_string()
        }
        PositionChange::Increased => "The position grew since the prior analysis.".to_string(),
        PositionChange::Decreased => "The position shrank since the prior analysis.".to_string(),
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
pub(crate) fn stub_ledger_draft(prior: Option<&ThesisLedger>, symbol: &str, role_risk: bool) -> LedgerDraft {
    if let Some(l) = prior {
        let core_draft = |q: &QuantCore| QuantCoreDraft {
            series: q.series.as_kebab().to_string(),
            comparator: q.comparator.as_kebab().to_string(),
            threshold: q.threshold,
            margin: q.margin,
        };
        // A verbatim re-emission: a kept core comes back under its name (blank
        // where it had none — never its render), a qualitative or refused
        // condition under the statement the prior ledger showed.
        let echo = |c: &LedgerCondition| match (&c.quant, &c.label) {
            (Some(_), label) => label.clone().unwrap_or_default(),
            (None, _) => c.statement.clone(),
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
                    statement: echo(c),
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
                    statement: echo(c),
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
            "Cost drift".to_string(),
            QuantCoreDraft {
                series: "expense-ratio".into(),
                comparator: "above".into(),
                threshold: 0.0075,
                margin: 0.0005,
            },
        )
    } else {
        (
            "Deep drawdown".to_string(),
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
            statement: "Priced-in ceiling".into(),
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
    // Research + distillation ride the trait's offline defaults.

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
            model_target_rationale: "Base case follows the engine's scenario midpoint.".to_string(),
            what_changed,
            // The stub re-affirms — no typed rows, matching the empty-audit
            // re-affirmation contract.
            what_changed_entries: Vec::new(),
            ledger: stub_ledger_draft(
                input.prior_ledger,
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
                input.prior_ledger,
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

/// The live [`HoldingAnalyst`]: wraps a [`LocalModelClient`] and the roster's
/// reasoner and fast model ids. Distillation normally runs on the fast model,
/// routes an oversized prompt or reservation-bound retry to the reasoner, and
/// uses the reasoner directly when no fast tier is configured. Interpretation
/// runs on the reasoner in thinking mode with the grammar-constrained schema,
/// so the returned object is structurally valid by construction.
pub struct LocalAnalyst {
    client: LocalModelClient,
    reasoner_model: String,
    fast_model: String,
    /// Routed model ids accumulated for the current holding, in outbound-call
    /// order. This is separate from prompt usage because provenance must also
    /// survive a transport failure before the daemon returns counters.
    model_calls: std::sync::Mutex<Vec<String>>,
    /// The bounded retry-once gate shared by every model-call site this run
    /// (`docs/local-models.md §The local-model adapter seam`); its fired
    /// events drain through [`HoldingAnalyst::take_retry_events`].
    retry: crate::local_model::RetryOnce,
    /// The live web tool + tracker context for the 6c research loop
    /// ([`LocalAnalyst::with_research`]); `None` runs the trait's offline
    /// research default — the demo path and any construction without a web
    /// stack.
    research_ctx: Option<LiveResearchCtx>,
}

/// What the live research loop needs beyond the model client: the web seam
/// (search + cached fetch + telemetry) and the run's progress context.
pub struct LiveResearchCtx {
    pub web: std::sync::Arc<dyn research::ResearchWeb + Send + Sync>,
    pub progress: std::sync::Arc<crate::progress::RunContext>,
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
            client: client.with_usage_capture(),
            reasoner_model,
            fast_model,
            model_calls: std::sync::Mutex::new(Vec::new()),
            retry: crate::local_model::RetryOnce::new(),
            research_ctx: None,
        }
    }

    /// Attach the live web tool + progress context so [`HoldingAnalyst::research`]
    /// runs the real 6c loop; without it the offline default runs (fail-soft,
    /// a recorded gap — never a failed run).
    pub fn with_research(mut self, ctx: LiveResearchCtx) -> Self {
        self.research_ctx = Some(ctx);
        self
    }

    /// Record the request's actual routed model immediately before issue.
    /// Failed and retried attempts count as calls; the holding audit dedups the
    /// drained sequence while retaining first-call order.
    fn record_model_call(&self, req: &ChatRequest) {
        self.model_calls
            .lock()
            .expect("model-call lock is never poisoned")
            .push(req.model_id.clone());
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
/// Called after adapter telemetry capture, so the observation survives on the run's
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
        // and incomplete or phase-limited counts name no lever at all.
        match crate::local_model::observed_length_stop_reading(generated, reservation, crate::local_model::phase_limited(req.think, req.format_schema.is_some())) {
            crate::local_model::LengthStopReading::AtReservation => anyhow::bail!(
                "{stage}: response truncated at the output reservation (num_predict {}, \
                 API eval_count {} tokens) — a runaway chain or a genuinely undersized \
                 reservation; raise it only on evidence",
                show_res(reservation),
                show(generated),
            ),
            crate::local_model::LengthStopReading::UnderReservation => anyhow::bail!(
                "{stage}: generation length-stopped under the output reservation (API eval_count {} \
                 of {} reserved) — context exhaustion suspected; the sanctioned lever is \
                 compressing the digest, never raising num_ctx",
                show(generated),
                show_res(reservation),
            ),
            crate::local_model::LengthStopReading::Unattributed => anyhow::bail!(
                "{stage}: generation length-stopped with incomplete or phase-limited counts (API eval_count {}, \
                 num_predict {}) — reservation-hit vs context exhaustion cannot be told \
                 apart; read the Ollama server log before reaching for either lever",
                show(generated),
                show_res(reservation),
            ),
        }
    }
    Ok(())
}

/// Fail a stage whose call completed with no completion at all: a blank
/// `content` and no tool calls (a research turn's tool request carries its
/// substance in `tool_calls`, so it passes). Without this check an empty body
/// dies at the call site's parse as an opaque serde EOF; typed here it carries
/// the class the bounded retry-once classifies on. Runs after
/// [`ensure_not_output_limited`], so a length stop keeps its own reading.
fn ensure_nonempty_completion(
    stage: &str,
    resp: &crate::local_model::ChatResponse,
) -> Result<()> {
    if resp.content.trim().is_empty() && resp.tool_calls.is_none() {
        return Err(
            anyhow::Error::new(crate::local_model::RetryClass::EmptyCompletion).context(format!(
                "{stage}: the model returned an empty completion body (done_reason: {})",
                resp.done_reason.as_deref().unwrap_or("unreported")
            )),
        );
    }
    Ok(())
}

/// Decode the interpretation call's completion: the schema-valid parse, then
/// the model arm's declared numeric domain
/// ([`crate::portfolio::validate_model_arm`]). Each failure carries the class the
/// bounded retry-once classifies on — a parse failure `SchemaParse`, an
/// off-domain value `ModelArmDomain` — so the re-issue fires for both and a
/// hard failure's annotation names which one (the 2026-08-24 review's Codex
/// I6, ruled 2026-08-29). Runs inside the retry closure, after
/// [`ensure_nonempty_completion`], so an off-domain response gets exactly the
/// one re-issue every content failure gets and never a second retry layer.
/// Parse a schema-constrained response body, inserting the app-written debut
/// continuity fields first on a debut (fix list 3.3, `portfolio-v38`): the
/// debut grammar carries neither field, so the body is completed before it is
/// typed, and a structural failure keeps the bounded-retry `SchemaParse` class.
fn decode_response_body<T: serde::de::DeserializeOwned>(
    what: &str,
    content: &str,
    debut: bool,
) -> Result<T> {
    let typed = || -> std::result::Result<T, serde_json::Error> {
        let mut body: serde_json::Value = serde_json::from_str(content)?;
        if debut {
            crate::portfolio::complete_debut_response(&mut body);
        }
        serde_json::from_value(body)
    };
    typed()
        .map_err(|e| anyhow::Error::new(e).context(crate::local_model::RetryClass::SchemaParse))
        .with_context(|| format!("parsing {what} JSON: {}", body_snippet(content)))
}

fn decode_interpretation(stage: &str, content: &str, debut: bool) -> Result<Interpretation> {
    let interpretation: Interpretation = decode_response_body("interpretation", content, debut)?;
    crate::portfolio::validate_model_arm(
        &interpretation.model_sub_scores,
        &interpretation.model_price_targets,
    )
    .map_err(|e| anyhow::Error::new(e).context(crate::local_model::RetryClass::ModelArmDomain))
    .with_context(|| stage.to_string())?;
    Ok(interpretation)
}

// Per-stage context sizes (`docs/local-model-operations.md §The num_ctx trap`):
// always explicit — the daemon's memory-dependent auto-size (~256 K on 128 GB)
// over-allocates KV cache, while an unset small default silently front-truncates
// the deterministic packet. Sized to hold packet + thinking budget + output.
/// Distillation: a compact findings condense — small packet, no thinking chain.
/// The fast rung of [`distill_route`]'s issue guard: a distillation prompt that
/// outgrows this context's budget issues on the reasoner at
/// [`NUM_CTX_INTERPRET`] instead of front-truncating here.
const NUM_CTX_DISTILL: u32 = 32_768;
/// Interpretation: the vendor advises ≥ 128 K context to preserve thinking
/// capability (chains run tens of thousands of tokens); hybrid attention keeps
/// the KV cost of this a few GB (`docs/local-model-operations.md §Context window`).
pub(crate) const NUM_CTX_INTERPRET: u32 = 131_072;
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
/// Normal distillation ceiling. The response is a potentially wide structured
/// object: combined narrative, per-topic claims and URLs, typed side channels,
/// and bounded observation excerpts. A reservation-bound stop gets one larger
/// retry below; this first ceiling remains the runaway/latency guardrail.
const NUM_PREDICT_DISTILL: u32 = 8_192;
/// One evidence-triggered distillation re-attempt after the normal reservation
/// binds exactly. It issues on the reasoner's 128 K context so the prompt and
/// this full ceiling fit together under the same 60% input sizing guard.
const NUM_PREDICT_DISTILL_RETRY: u32 = 32_768;

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

/// Where one distillation call issues — the app-side guard against the
/// daemon's silent front-truncation (`docs/local-models.md §The local-model
/// adapter seam`; the 2026-08-24 review's reduce-prompt minor, ruled
/// 2026-08-28). The rendered prompt — instruction scaffolding, ledger
/// conditions, and distillates together — is measured in chars against its
/// model's input budget before any request exists. Within the fast tier's
/// budget it issues there at [`distill_num_ctx`]; over it but within the
/// reasoner's, it issues on the resident reasoner at the interpretation
/// context — a model choice, never a `num_ctx` change (the reasoner already
/// loads at that size, so nothing reloads, and the fast tier co-resides by
/// the roster's own precondition); over the widest budget it is refused here,
/// unclassified so the retry gate never re-issues a deterministic outcome,
/// and the run fails legibly. On the default roster (fast = reasoner) the two
/// rungs are one budget and only the refusal is live. Pure, so the routing is
/// pinned offline.
fn distill_route<'a>(
    stage: &str,
    prompt_chars: usize,
    fast_model: &'a str,
    reasoner_model: &'a str,
) -> Result<(&'a str, u32)> {
    let fast_ctx = distill_num_ctx(fast_model, reasoner_model);
    if prompt_chars <= distill::input_budget_chars(fast_ctx) {
        return Ok((fast_model, fast_ctx));
    }
    let widest = distill::input_budget_chars(NUM_CTX_INTERPRET);
    if fast_model != reasoner_model && prompt_chars <= widest {
        return Ok((reasoner_model, NUM_CTX_INTERPRET));
    }
    anyhow::bail!(
        "{stage}: distillation prompt of {prompt_chars} chars exceeds the widest input budget \
         ({widest} chars at num_ctx {NUM_CTX_INTERPRET}) — refused before issue; the sanctioned \
         lever is compressing the digest, never raising num_ctx"
    )
}

/// Build one distillation call's request: **explicitly non-thinking**
/// (`Some(false)` — an omitted flag rides Qwen's thinking-on default and cost
/// the first live run ~45 minutes, F3), non-thinking sampling, the
/// grammar-constraining `format` schema, the caller-routed model and context
/// size ([`distill_route`]). Pure, so the per-stage wiring is asserted offline.
fn distill_request(
    model: &str,
    num_ctx: u32,
    num_predict: u32,
    prompt: &distill::DistillPrompt,
    schema: &serde_json::Value,
) -> ChatRequest {
    // The role line and the two-part message (`portfolio-v44`).
    let mut req = ChatRequest::new(
        model,
        vec![
            ChatMessage::system(prompt.system.clone()),
            ChatMessage::user(prompt.user.clone()),
        ],
    );
    req.think = Some(false);
    req.format_schema = Some(schema.clone());
    req.options = Some(options::non_thinking_general(num_ctx, num_predict));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// The only stop that activates the larger distillation attempt: the normal
/// request declared the normal ceiling and the daemon reports that it generated
/// exactly that many tokens. A stop below it is context-bound or unattributable
/// and keeps the existing hard-failure posture.
fn hit_normal_distill_reservation(
    req: &ChatRequest,
    resp: &crate::local_model::ChatResponse,
) -> bool {
    resp.done_reason.as_deref() == Some("length")
        && crate::local_model::request_num_predict(req) == Some(NUM_PREDICT_DISTILL)
        && resp.eval_count == Some(u64::from(NUM_PREDICT_DISTILL))
}

/// Build one research-loop turn's request: thinking on, the shared interpret
/// context (one `num_ctx` per model). Tools and the findings grammar are passed
/// per phase and never together — the gathering turns carry `tools` with no
/// `format`, the synthesis call carries `format` with no `tools` (attempt-4
/// Finding 4, fix B).
fn research_turn_request(
    reasoner_model: &str,
    messages: Vec<ChatMessage>,
    tools: Option<&serde_json::Value>,
    format: Option<&serde_json::Value>,
) -> ChatRequest {
    let mut req = ChatRequest::new(reasoner_model, messages);
    req.tools = tools.cloned();
    req.format_schema = format.cloned();
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Build the priced-branch interpretation request: thinking on (composes with the
/// grammar-constrained `format`), thinking sampling, interpret-sized context.
fn interpret_request(reasoner_model: &str, input: &InterpretationInput) -> ChatRequest {
    let is_fund = dossier_is_fund(input.dossier);
    let debut = input.dossier.prior_verdict.is_none();
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![
            ChatMessage::system(interpretation_system_prompt(is_fund, debut)),
            ChatMessage::user(interpretation_user_prompt(input)),
        ],
    );
    // The v7 unrestricted schema: full ladder, full conviction enum — the engine's
    // own lean bars and any pre-profit ceiling render into the prompt as evidence,
    // never as schema narrowing (`docs/portfolio-analysis.md` §The holding verdict,
    // the two-arm contract). Scoped per call since `portfolio-v38`: the series
    // enum to the vehicle kind, the shape to debut / continuity (fix list 3.3).
    req.format_schema = Some(interpretation_schema(is_fund, debut));
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Build the `role_risk_only`-branch interpretation request — same mode wiring as
/// the priced branch, reduced schema.
fn role_risk_request(reasoner_model: &str, input: &RoleRiskInput) -> ChatRequest {
    let debut = input.dossier.prior_verdict.is_none();
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![
            ChatMessage::system(role_risk_system_prompt(debut)),
            ChatMessage::user(role_risk_user_prompt(input)),
        ],
    );
    req.format_schema = Some(role_risk_interpretation_schema(debut));
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Build the per-holding action request under the chosen engine-set rendering
/// (fix list 3.9; the pipeline passes `List`): thinking on (the rung is a judgment
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
    fn research(
        &self,
        dossier: &HoldingDossier,
        plan: &ResearchPlan,
    ) -> Result<HoldingResearch> {
        let Some(ctx) = &self.research_ctx else {
            // No web stack attached: the offline default, its absence a
            // recorded gap on the audit (never a failed run).
            return Ok(research::offline_stub(plan));
        };
        // The model seam: a thinking gathering turn carries the tools (no
        // grammar); the pass's findings ride a separate synthesis call with the
        // grammar (no tools), so the two never share a request (attempt-4
        // Finding 4, fix B).
        struct TurnAdapter<'a> {
            analyst: &'a LocalAnalyst,
            stage: &'a str,
        }
        impl research::ResearchModel for TurnAdapter<'_> {
            fn research_turn(
                &self,
                stage: &str,
                messages: &[ChatMessage],
                tools: Option<&serde_json::Value>,
                format: Option<&serde_json::Value>,
            ) -> Result<crate::local_model::ChatResponse> {
                let mut req = research_turn_request(
                    &self.analyst.reasoner_model,
                    messages.to_vec(),
                    tools,
                    format,
                );
                req.stage = Some(stage.to_string());
                self.analyst.record_model_call(&req);
                // Non-streaming (the tool protocol), the reply's thinking
                // forwarded whole onto the holding's step once it lands.
                let resp = self
                    .analyst
                    .client
                    .chat_with_role(&req, StreamRole::Step(self.stage))?;

                ensure_not_output_limited(self.stage, &req, &resp)?;
                ensure_nonempty_completion(self.stage, &resp)?;
                Ok(resp)
            }

            fn retry_permitted(&self, stage: &str, err: &anyhow::Error) -> bool {
                self.analyst
                    .retry
                    .permit(self.analyst.client.progress(), stage, err)
            }
        }
        let clock = crate::research_executor::WallClock::new();
        let model = TurnAdapter {
            analyst: self,
            stage: &plan.step_label,
        };
        let runner = research::ResearchRunner {
            model: &model,
            web: ctx.web.as_ref(),
            budget: research::ResearchBudget {
                max_fetches: research::MAX_FETCHES_PER_HOLDING,
                max_wall: research::MAX_WALL_PER_HOLDING,
                clock: &clock,
            },
            progress: &ctx.progress,
            step_label: plan.step_label.clone(),
        };
        let brief = holding_header(dossier);
        runner.run_holding(&brief, &plan.agenda, &plan.seeds, &|key| {
            plan.topic_seeds.get(key).cloned()
        })
    }

    fn distill_research(&self, inputs: &DistillInputs) -> Result<DistilledResearch> {
        struct ModelAdapter<'a> {
            analyst: &'a LocalAnalyst,
            /// Stages that spent their single re-attempt on an expanded output
            /// request. The outer parse/transport retry gate must not add a
            /// third call afterward.
            spent_output_retries: std::cell::RefCell<std::collections::HashSet<String>>,
        }
        impl distill::DistillModel for ModelAdapter<'_> {
            fn distill_call(
                &self,
                stage: &str,
                prompt: &distill::DistillPrompt,
                schema: &serde_json::Value,
            ) -> Result<String> {
                // The issue guard: size the rendered prompt — both messages —
                // against its model's budget before any request exists
                // (`distill_route`).
                let (model, num_ctx) = distill_route(
                    stage,
                    prompt.chars(),
                    &self.analyst.fast_model,
                    &self.analyst.reasoner_model,
                )?;
                let mut req = distill_request(
                    model,
                    num_ctx,
                    NUM_PREDICT_DISTILL,
                    prompt,
                    schema,
                );
                req.stage = Some(stage.to_string());
                self.analyst.record_model_call(&req);
                let resp = self.analyst.client.chat(&req)?;

                if hit_normal_distill_reservation(&req, &resp) {
                    // The rendered prompt already passed the reasoner's 60%
                    // input guard, leaving more than the 32 K expanded ceiling
                    // in its 128 K context. Route the one evidence-triggered
                    // re-attempt there even when the normal call used a 32 K
                    // fast tier, whose shared context could not hold both.
                    self.spent_output_retries
                        .borrow_mut()
                        .insert(stage.to_string());
                    let mut expanded_req = distill_request(
                        &self.analyst.reasoner_model,
                        NUM_CTX_INTERPRET,
                        NUM_PREDICT_DISTILL_RETRY,
                        prompt,
                        schema,
                    );
                    expanded_req.stage = Some(format!("{stage} (expanded)"));
                    self.analyst.record_model_call(&expanded_req);
                    let expanded_resp = self.analyst.client.chat(&expanded_req)?;

                    ensure_not_output_limited(stage, &expanded_req, &expanded_resp).with_context(
                        || {
                            format!(
                                "{stage}: expanded distillation attempt also length-stopped after \
                                 the normal {NUM_PREDICT_DISTILL}-token reservation bound"
                            )
                        },
                    )?;
                    ensure_nonempty_completion(stage, &expanded_resp)?;
                    return Ok(expanded_resp.content);
                }
                ensure_not_output_limited(stage, &req, &resp)?;
                ensure_nonempty_completion(stage, &resp)?;
                Ok(resp.content)
            }

            fn retry_permitted(&self, stage: &str, err: &anyhow::Error) -> bool {
                // A normal-reservation stop already spent this stage's single
                // re-attempt on the expanded request. Do not let the outer
                // parse/transport retry layer add a third call afterward.
                if self.spent_output_retries.borrow().contains(stage) {
                    return false;
                }
                self.analyst
                    .retry
                    .permit(self.analyst.client.progress(), stage, err)
            }
        }
        distill::distill(
            &ModelAdapter {
                analyst: self,
                spent_output_retries: std::cell::RefCell::new(
                    std::collections::HashSet::new(),
                ),
            },
            inputs,
        )
    }

    fn distill_input_budget(&self) -> usize {
        distill::input_budget_chars(distill_num_ctx(&self.fast_model, &self.reasoner_model))
    }

    fn distill_issue_budget(&self) -> usize {
        // The guard's widest rung (`distill_route`): the reasoner's context on
        // both rosters — equal to the routing budget on the default one.
        distill::input_budget_chars(NUM_CTX_INTERPRET)
    }

    fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
        let mut req = interpret_request(&self.reasoner_model, input);
        // Stream step-scoped: the structured body has no console value (it stays
        // accumulated, never streamed), but the reasoning streams onto this
        // holding's own "Analyze {SYM}" step, so the tracker shows live thinking
        // instead of a minutes-long quiet stretch (the first live run's F8).
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let stage = format!("interpret {}", input.dossier.position.symbol);
        let debut = input.dossier.prior_verdict.is_none();
        req.stage = Some(stage.clone());
        self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self
                .client
                .chat_streaming(&req, StreamRole::Step(&step_key))?;

            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            decode_interpretation(&stage, &resp.content, debut)
        })
    }

    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
        let mut req = role_risk_request(&self.reasoner_model, input);
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let stage = format!("role-risk {}", input.dossier.position.symbol);
        let debut = input.dossier.prior_verdict.is_none();
        req.stage = Some(stage.clone());
        self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self
                .client
                .chat_streaming(&req, StreamRole::Step(&step_key))?;

            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            decode_response_body("role/risk interpretation", &resp.content, debut)
        })
    }

    fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision> {
        let mut req = action_request(&self.reasoner_model, input);
        // Stream step-scoped like interpretation: the decision's reasoning lands
        // on this holding's own "Analyze {SYM}" step.
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let stage = format!("action {}", input.dossier.position.symbol);
        req.stage = Some(stage.clone());
        self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self
                .client
                .chat_streaming(&req, StreamRole::Step(&step_key))?;

            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            serde_json::from_str(&resp.content)
                .map_err(|e| {
                    anyhow::Error::new(e).context(crate::local_model::RetryClass::SchemaParse)
                })
                .with_context(|| {
                    format!(
                        "parsing action-decision JSON: {}",
                        body_snippet(&resp.content)
                    )
                })
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

    fn take_model_calls(&self) -> Option<Vec<String>> {
        Some(std::mem::take(
            &mut *self
                .model_calls
                .lock()
                .expect("model-call lock is never poisoned"),
        ))
    }

    fn take_prompt_usage(&self) -> Vec<crate::local_model::PromptUsage> {
        self.client.take_prompt_usage()
    }

    fn take_retry_events(&self) -> Vec<crate::local_model::RetryEvent> {
        self.retry.take_events()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::portfolio::engine::{
        CompanyFinancials, ConsensusEpsPeriod, ConsensusEstimate, DatedValue,
        QuarterlyIncomeRow,
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

    /// The research-finding delta entries carry the distillation's ledger tie —
    /// rendered by statement (the id app-owned) — for fresh claims only; a
    /// cached claim never becomes an entry, and a tie to a condition no longer
    /// on the ledger renders as no tie (2026-08-24 review F3).
    #[test]
    fn research_delta_entries_carry_the_ledger_tie_by_statement_for_fresh_claims() {
        use crate::portfolio::research::{DistilledClaim, TopicDistillate};
        let claim = |text: &str, cached: bool, tie: Option<&str>| DistilledClaim {
            publication: crate::portfolio::research::PublicationDate::default(),
            fact_period: crate::portfolio::research::FactPeriod::default(),
            claim: text.into(),
            source_url: format!("https://x.example/{}", text.len()),
            retrieved_at: "2026-08-26T00:00:00+00:00".into(),
            cached,
            related_condition_id: tie.map(str::to_string),
        };
        let distilled = DistilledResearch {
            combined: "c".into(),
            topic_layer: vec![TopicDistillate {
                topic_key: "t".into(),
                vintage: "2026-08-26T00:00:00+00:00".into(),
                summary: "s".into(),
                claims: vec![
                    claim("fresh tied", false, Some("keep-1")),
                    claim("cached tied", true, Some("keep-1")),
                    claim("fresh untied", false, None),
                    claim("fresh stale tie", false, Some("gone")),
                ],
            }],
            unreconciled_topics: vec![],
            forward_assumption: None,
            leading_indicator: None,
            forensic_event: None,
            pre_profit_observations: vec![],
            backfill: None,
            shape: distill::DistillShape::SinglePass,
            gaps: vec![],
        };
        let prior = prior_with_conditions();
        let mut entries = Vec::new();
        push_research_delta_entries(&mut entries, &distilled, Some(&prior));
        assert_eq!(
            entries.len(),
            3,
            "cached claims never become entries: {entries:?}"
        );
        assert_eq!(entries[0].id, "research-1");
        assert!(
            entries[0]
                .label
                .contains("— bears on ledger condition 'Trailing return collapses to -40%'"),
            "{}",
            entries[0].label
        );
        assert!(
            !entries[0].label.contains("keep-1"),
            "ids stay out: {}",
            entries[0].label
        );
        assert_eq!(entries[0].related_condition_id.as_deref(), Some("keep-1"));
        for e in &entries[1..] {
            assert!(!e.label.contains("bears on"), "{}", e.label);
            assert_eq!(e.related_condition_id, None, "{e:?}");
        }
        // A debut (no prior ledger) renders every finding untied.
        let mut debut = Vec::new();
        push_research_delta_entries(&mut debut, &distilled, None);
        assert!(debut.iter().all(|e| e.related_condition_id.is_none()));
    }

    fn delta_fixture() -> Vec<crate::portfolio::DeltaEntry> {
        vec![
            crate::portfolio::DeltaEntry {
                id: "D1".into(),
                label: "spot: 100.00 -> 92.00".into(),
                related_condition_id: None,
            },
            crate::portfolio::DeltaEntry {
                id: "D2".into(),
                label: "metric gross margin: 0.4200 -> 0.3800".into(),
                related_condition_id: None,
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
    fn input_delta_section_renders_ids_and_rules_even_without_external_changes() {
        let s = input_delta_prompt_section(&delta_fixture());
        assert!(s.starts_with("\nCHANGES SINCE THE PRIOR ANALYSIS (each with an id)\n"), "{s}");
        assert!(s.contains("[D1] spot: 100.00 -> 92.00"), "{s}");
        assert!(s.contains("[D2] metric gross margin"), "{s}");
        // The section is data only (`portfolio-v40`): the attribution rules live
        // in each message's Part 2 items, never beside the ids.
        for narration in ["downgraded", "self-correction", "WHAT_CHANGED_ENTRIES"] {
            assert!(!s.contains(narration), "`{narration}` leaked: {s}");
        }
        let empty = input_delta_prompt_section(&[]);
        assert!(empty.contains("None recorded.\n"), "{empty}");
        assert!(!empty.contains("No input-delta entries are available"), "{empty}");
        // The role/risk message states the same requirements as its own Part 2
        // items, with this branch's detail gloss and no parameter sentence
        // (`portfolio-v42`).
        let rules = what_changed_task_items(3, "the role read, the scenario or the condition", false);
        assert!(
            rules.contains("\n3. what_changed_entries — one row per intrinsic value that moved")
                && rules.contains("detail (which one — the role read, the scenario or the condition)"),
            "{rules}"
        );
        assert!(!rules.contains("downgraded") && !rules.contains("parameter change"), "{rules}");
        assert!(rules.contains("never a rephrasing"), "{rules}");
        assert!(rules.contains("\n   what_changed — one sentence summarizing those rows.\n"), "{rules}");
        // The priced continuity item states the same requirements on the output,
        // citing the section by name, with no validator narration; a debut
        // requests no rows at all.
        let contract = LedgerSeriesContract::build(false, None, None);
        let task = interpretation_task_section(&contract, false, false, false, true);
        assert!(task.contains("\n6. what_changed_entries — one row per intrinsic value that moved"), "{task}");
        assert!(task.contains("kind (which kind of value, from the alternatives in the shape)"), "{task}");
        assert!(task.contains("cites one bracketed id from CHANGES SINCE THE PRIOR ANALYSIS"), "{task}");
        assert!(task.contains("never a rephrasing"), "{task}");
        assert!(task.contains("No row repeats another, and no row has old equal to new."), "{task}");
        for narration in ["downgraded", "validator", "logged reason", "WHAT_CHANGED_ENTRIES"] {
            assert!(!task.contains(narration), "`{narration}` leaked: {task}");
        }
        let debut = interpretation_task_section(&contract, false, false, true, false);
        assert!(!debut.contains("what_changed"), "{debut}");
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

    #[test]
    fn local_analyst_records_and_drains_failed_physical_attempts() {
        let analyst = LocalAnalyst::new(
            LocalModelClient::new("http://127.0.0.1:1").unwrap(),
            "reasoner".into(),
            String::new(),
        );
        let mut req = ChatRequest::new("reasoner", vec![ChatMessage::user("café 世界")]);
        req.stage = Some("holding-AAPL research earnings gathering turn 2".into());
        assert!(analyst.client.chat(&req).is_err());
        req.stage = Some("distill AAPL (expanded)".into());
        assert!(analyst
            .client
            .chat_streaming(&req, StreamRole::Silent)
            .is_err());
        let rows = analyst.take_prompt_usage();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].stage,
            "holding-AAPL research earnings gathering turn 2"
        );
        assert_eq!(rows[1].stage, "distill AAPL (expanded)");
        assert!(rows
            .iter()
            .all(|u| !u.adapter_ok && u.api.eval_count.is_none()));
        assert_eq!(
            rows[0].prompt_chars,
            crate::local_model::prompt_material_chars(&req.messages, None) as u64
        );
        assert!(analyst.take_prompt_usage().is_empty());
    }

    #[test]
    fn local_analyst_records_routed_models_in_first_call_order() {
        let analyst = LocalAnalyst::new(
            LocalModelClient::new("http://127.0.0.1:1").unwrap(),
            "reasoner".into(),
            "fast-tier".into(),
        );
        let research = research_turn_request(
            "reasoner",
            vec![ChatMessage::user("brief")],
            None,
            None,
        );
        analyst.record_model_call(&research);

        let fast_budget = distill::input_budget_chars(NUM_CTX_DISTILL);
        let (routed, num_ctx) = distill_route(
            "distill TEST reduce",
            fast_budget + 1,
            &analyst.fast_model,
            &analyst.reasoner_model,
        )
        .unwrap();
        assert_eq!(routed, "reasoner", "oversized distill routes upward");
        let distill = distill_request(
            routed,
            num_ctx,
            NUM_PREDICT_DISTILL,
            &distill::DistillPrompt { system: "s".into(), user: "wide prompt".into() },
            &serde_json::json!({"type": "object"}),
        );
        analyst.record_model_call(&distill);

        assert_eq!(
            analyst.take_model_calls(),
            Some(vec!["reasoner".to_string(), "reasoner".to_string()]),
            "the request's routed id is recorded, not the configured fast slot"
        );
        assert_eq!(analyst.take_model_calls(), Some(Vec::new()));
    }

    pub(crate) fn position(asset_class: AssetClass) -> Position {
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

    pub(crate) fn rates() -> RateAnchors {
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

    pub(crate) fn strong_financials() -> CompanyFinancials {
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

    /// A minimal ledger for the action packet's THESIS and SCENARIOS sections.
    pub(crate) fn test_ledger() -> ThesisLedger {
        ThesisLedger {
            branch: LedgerBranch::Priced,
            original_thesis: "A standing thesis.".into(),
            current_thesis: "A standing thesis.".into(),
            key_drivers: vec![],
            monitor: [(ScenarioKind::Bear, 25.0), (ScenarioKind::Base, 50.0), (ScenarioKind::Bull, 25.0)]
                .into_iter()
                .map(|(scenario, probability_pct)| MonitorScenario {
                    scenario,
                    conditions: format!("{} case conditions.", scenario.as_str()),
                    probability_pct,
                    engine_target: None,
                })
                .collect(),
            what_must_improve: String::new(),
            what_must_not_break: String::new(),
            conditions: vec![],
            authored_band_relation: None,
        }
    }

    pub(crate) fn dossier(asset_class: AssetClass, financials: CompanyFinancials) -> HoldingDossier {
        HoldingDossier {
            prior_metrics: None,
            semantic_recall: Default::default(),
            news_seeds: Vec::new(),
            research_priors: Vec::new(),
            analysis_date: "2026-07-28".into(),
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
            prior_consensus_eps_periods: Vec::new(),
            prior_matured_notes: Vec::new(),
            prior_grade_parameter_version: None,
            prior_target_parameter_version: None,
            prior_authoring_close: None,
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

    #[test]
    fn the_technology_topic_fires_from_the_pre_flag_or_a_standing_falsifier_and_only_once() {
        // A fresh news seed beside a standing falsifier fires the topic from
        // the falsifier line alone — the seed is a lead in the pass brief,
        // never a trigger of its own (retired 2026-08-29, Codex I15) — and no
        // combination of the triggers adds the topic twice.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.news_seeds = vec![research::ResearchSeed {
            id: "seed-1".into(),
            headline: "Rival unveils a competing chip".into(),
            url: "https://reuters.com/rival-chip".into(),
            source: "reuters.com".into(),
            published: Some("2026-08-22".into()),
        }];
        let tech_topics = |triggers: research::AgendaTriggers| {
            research::build_agenda(&d, &triggers)
                .iter()
                .filter(|t| t.key == "technology-event")
                .count()
        };
        // A seed with nothing standing behind it fires nothing.
        assert_eq!(tech_topics(research::AgendaTriggers::default()), 0);
        assert_eq!(
            tech_topics(research::AgendaTriggers {
                tech_ledger_falsifier: true,
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            tech_topics(research::AgendaTriggers {
                tech_pre_flag_fired: true,
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            tech_topics(research::AgendaTriggers {
                tech_pre_flag_fired: true,
                tech_ledger_falsifier: true,
                ..Default::default()
            }),
            1
        );
    }

    #[test]
    fn slice2_fund_agenda_excludes_technology_even_with_both_triggers() {
        let d = fund_dossier(us_equity_fund());
        let agenda = research::build_agenda(&d, &research::AgendaTriggers {
            tech_pre_flag_fired: true,
            tech_ledger_falsifier: true,
            ..Default::default()
        });
        assert!(!agenda.iter().any(|t| t.key == "technology-event"));
    }

    #[test]
    fn the_pre_profit_backfill_agenda_keeps_reporting_spans_separate() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let agenda = research::build_agenda(
            &d,
            &research::AgendaTriggers {
                overlay_eligible: true,
                pre_profit_backfill: true,
                ..Default::default()
            },
        );
        let topic = agenda
            .iter()
            .find(|t| t.key == "pre-profit-execution")
            .expect("the eligible stock gets the pre-profit topic");
        let backfill = topic
            .questions
            .iter()
            .find(|q| q.starts_with("Also find the issuer's latest four reported periods"))
            .expect("the binding obligation reaches the agenda");
        assert!(backfill.contains("exact reporting span"), "{backfill}");
        assert!(backfill.contains("never substitute quarterly"), "{backfill}");
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
            profile_is_fund: None,
            profile_description: None,
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
    fn a_summary_only_house_view_is_claimed_by_both_prompts_since_both_render_the_stances() {
        // Both messages render the house view through one MARKET ANALYSIS
        // section — the latest sections and the recent stances (`portfolio-v42`;
        // through v41 the role/risk message rendered the sections only). And
        // `load_house_view` deliberately keeps the summaries when the latest
        // report's Markdown is missing or unreadable, so a summary-only house
        // view is reachable and reaches both verdicts as the stance lines — which
        // the one predicate the audit reads says.
        for d in [
            {
                let mut d = fund_dossier(us_equity_fund());
                d.house_view = house_view_of(None, 2);
                d
            },
            {
                let mut d = dossier(AssetClass::Stock, strong_financials());
                d.house_view = house_view_of(None, 2);
                d
            },
        ] {
            assert!(prompt_renders_house_view(&d), "the stances render, so the source is consulted");
        }
        assert!(
            !prompt_renders_house_view(&fund_dossier(us_equity_fund())),
            "no house view at all: nothing renders, nothing is claimed"
        );

        // End to end on the role/risk branch.
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
            claims(role_risk_sources(house_view_of(None, 2))),
            "summary-only: the stances render on the role/risk message, so the claim is earned"
        );
        assert!(
            claims(role_risk_sources(house_view_of(Some("## Thesis\nrisk-on."), 0))),
            "sections present: it does render them, so the claim is earned"
        );
        assert!(
            !claims(role_risk_sources(HouseView::default())),
            "no house view: nothing renders, so the claim is not made"
        );
    }

    /// A stub with a distinct fast tier and reasoner, so the audit's model list can
    /// be checked against the calls that actually ran.
    struct TieredStub;
    impl HoldingAnalyst for TieredStub {
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

    /// Exact-call telemetry stub: research really is the first modeled stage,
    /// followed by distillation and the reasoner judgments.
    #[derive(Default)]
    struct TelemetryTieredStub {
        calls: std::sync::Mutex<Vec<String>>,
    }
    impl TelemetryTieredStub {
        fn called(&self, model: &str) {
            self.calls.lock().unwrap().push(model.to_string());
        }
    }
    impl HoldingAnalyst for TelemetryTieredStub {
        fn research(
            &self,
            _dossier: &HoldingDossier,
            plan: &ResearchPlan,
        ) -> Result<HoldingResearch> {
            self.called("reasoner");
            Ok(research::offline_stub(plan))
        }
        fn distill_research(&self, inputs: &DistillInputs) -> Result<DistilledResearch> {
            self.called("fast-tier");
            Ok(distill::offline_consolidate(inputs))
        }
        fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
            self.called("reasoner");
            StubAnalyst.interpret(input)
        }
        fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
            self.called("reasoner");
            StubAnalyst.interpret_role_risk(input)
        }
        fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision> {
            self.called("reasoner");
            StubAnalyst.decide_action(input)
        }
        fn fast_id(&self) -> String {
            "fast-tier".into()
        }
        fn reasoner_id(&self) -> String {
            "reasoner".into()
        }
        fn take_model_calls(&self) -> Option<Vec<String>> {
            Some(std::mem::take(&mut *self.calls.lock().unwrap()))
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

        // The role/risk branch runs the fund agenda's research + a
        // pure-consolidation distillation (the stub-time bypass is retired
        // with the research slice — `docs/portfolio-workflow.md` §Step 6d),
        // then the reasoner's role read + action call: fast tier + reasoner.
        let (v, a) = run(&fund_dossier(bond_fund()));
        assert!(matches!(v.disposition, VerdictDisposition::RoleRiskOnly(_)));
        assert_eq!(a.model_ids, vec!["fast-tier".to_string(), "reasoner".to_string()]);

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

        // Exact telemetry supersedes configured-stage guesses: live research
        // is the first model call, so a distinct roster persists reasoner then
        // fast tier (and dedups the later reasoner judgments in place).
        let exact = TelemetryTieredStub::default();
        let (_, a) = analyze_holding(
            &exact,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert_eq!(a.model_ids, vec!["reasoner".to_string(), "fast-tier".to_string()]);
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
        // The reduced surface only — no statement-derived stock legs, and the
        // closed-end read stays None off the CEF form.
        assert!(audit.metrics.pe_ratio.is_none());
        assert!(audit.metrics.revenue_growth.is_none());
        assert_eq!(audit.metrics.nav_premium, None);

        // The CEF variant threads the closed-end read into the audit metrics,
        // so a premium move can seed its own input-delta row across runs
        // (Codex 2026-08-21 round 3, finding 3).
        let mut cef = bond_fund();
        cef.profile_is_fund = Some(true);
        cef.profile_description = Some("a closed-end fixed income fund".into());
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &fund_dossier(cef), &rates(), "2026-08-03").unwrap();
        assert!(matches!(verdict.disposition, VerdictDisposition::RoleRiskOnly(_)));
        let expected = 195.0 / 194.0 - 1.0;
        assert!(
            (audit.metrics.nav_premium.expect("CEF premium on the audit") - expected).abs()
                < 1e-12,
            "{:?}",
            audit.metrics.nav_premium
        );
    }

    #[test]
    fn an_empty_action_rationale_fails_the_holding() {
        // M6 of the 2026-08-18 audit: the schema types the rationale as any string,
        // so the contract's "never empty" is enforced app-side, fail-hard like the
        // rest of the model stage — on both branches' action calls.
        struct EmptyRationaleStub;
        impl HoldingAnalyst for EmptyRationaleStub {
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
                // The debut line is the app's since portfolio-v38 (fix list
                // 3.3, F6), whatever the analyst authored.
                assert_eq!(g.what_changed, crate::portfolio::DEBUT_WHAT_CHANGED);
                // The model's own-target explanation is carried through, not dropped.
                assert!(!g.model_target_rationale.is_empty());
                // The options signal rides on the verdict but never entered the grade.
                assert!(g.options_signal.put_call_volume.is_some());
                // The new engine reads persist on the priced branch.
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
                // The deterministic classification reaches the card-visible verdict.
                assert_eq!(g.fund_class_label.as_deref(), Some("US equity fund"));
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
        // The sentence must mean what the core says, or 6g downgrades the
        // re-emitted condition (the agreement checks, 2026-09-16).
        falsifier.statement =
            "The holding's price falls more than 40% below its current level".into();
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
    fn unsupported_units_and_overlay_payoffs_never_persist_priced_outputs() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.listing = Some(
            crate::portfolio::listing::ListingResolution::UnsupportedUnits {
                detail: "unverified ADR share basis".into(),
            },
        );
        let (verdict, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-04").unwrap();
        assert!(
            matches!(verdict.disposition, VerdictDisposition::NotRated { reason } if reason.contains("unsupported financial units"))
        );
        assert!(audit.target_meta.is_none());
        assert!(audit.quick_basis.is_none());

        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.financials.unit_issues.push(engine::StatementUnitIssue {
            surface: "income".into(),
            reported_currency: Some("TWD".into()),
        });
        let (verdict, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-04").unwrap();
        assert!(
            matches!(verdict.disposition, VerdictDisposition::InsufficientEvidence { reason } if reason.contains("unsupported financial units"))
        );
        assert!(audit.target_meta.is_none());
        assert!(audit.quick_basis.is_none());

        let mut fund = us_equity_fund();
        fund.name = Some("US Equity Covered Call ETF".into());
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &fund_dossier(fund), &rates(), "2026-08-04").unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::RoleRiskOnly(_)
        ));
        assert!(audit.target_meta.is_none());
        assert!(audit.quick_basis.is_none());
        assert!(verdict
            .thesis_ledger
            .as_ref()
            .unwrap()
            .monitor
            .iter()
            .all(|m| m.engine_target.is_none()));
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
    fn a_split_rebasis_normalizes_the_ingested_ledger_and_bridges_the_prior_reads() {
        // The prior read was authored pre-4:1-split (values ×4 against today's
        // retroactively re-based series). Its anchor bar 2026-06-30 closed at
        // 760 old-basis; today's series carries 190 for the same session, so the
        // bridge factor is exactly 0.25.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let mut ledger = prior_with_conditions();
        ledger.conditions = vec![LedgerCondition {
            condition_id: "px-1".into(),
            role: ConditionRole::Falsifier,
            trigger_family: None,
            label: None,
            statement: "price below $700".into(),
            quant: Some(QuantCore {
                series: engine::LedgerSeries::Price,
                comparator: LedgerComparator::Below,
                threshold: 700.0,
                margin: 20.0,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: None,
        }];
        d.prior_verdict = Some(HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated { reason: "fixture".into() },
            thesis_ledger: Some(ledger),
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        });
        d.prior_vintage = Some("2026-06-30T20:00:00Z".into());
        d.prior_spot = Some(780.0);
        d.prior_consensus_eps_periods = vec![engine::ConsensusEpsPeriod {
            period_end: "2027-06-30".into(),
            eps_mid: Some(26.0),
            ntm_weight: 1.0,
        }];
        d.prior_authoring_close =
            Some(DatedValue { date: "2026-06-30".into(), value: 760.0 });

        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(matches!(verdict.disposition, VerdictDisposition::Priced(_)));
        // No fabricated crossing: unbridged, spot 195 sits far "below 700".
        let la = audit.ledger_audit.as_ref().expect("ledger audit");
        assert!(la.crossings.is_empty(), "no cross-basis crossing: {:?}", la.crossings);
        // The persisted condition carries its id with the CONVERTED core — the
        // stub re-emitted the normalized threshold verbatim, so the carry held
        // (700 × 0.25 = 175; margin 20 × 0.25 = 5).
        assert!(la.superseded.is_empty(), "conversion must not supersede: {:?}", la.superseded);
        let persisted = verdict.thesis_ledger.as_ref().expect("ledger persists");
        let px = persisted
            .conditions
            .iter()
            .find(|c| c.condition_id == "px-1")
            .expect("carried id survives the conversion");
        let q = px.quant.as_ref().expect("still quantitative");
        assert!((q.threshold - 175.0).abs() < 1e-9, "converted threshold: {}", q.threshold);
        assert!((q.margin - 5.0).abs() < 1e-9, "converted margin: {}", q.margin);
        // The sentence re-rendered from the converted core, once — a prior with
        // no name echoes an empty name, never its render (`portfolio-v45`).
        assert_eq!(
            px.statement,
            "Price below $175.00, confirmed by two consecutive daily closes; margin ±$5.00"
        );
        assert_eq!(px.label, None);
        // This run stamps its own anchor: the newest settled bar strictly before
        // the run session.
        let anchor = audit.authoring_close.as_ref().expect("anchor stamped");
        assert_eq!(anchor.date, "2026-07-15");
        assert!((anchor.value - 195.0).abs() < 1e-9);

        // The narrative fallback pace reads off the BRIDGED prior spot (780 ×
        // 0.25 = 195 vs spot 195 → ~0), not a fabricated −75% collapse.
        let mut fallback = dossier(AssetClass::Stock, strong_financials());
        // EPS legs absent, revenue legs kept: the target ladder still prices off
        // forward revenue per share while the narrative read drops to its
        // operating-reality fallback — the one form whose pace leg reads
        // `prior_spot` directly.
        if let Some(c) = fallback.financials.consensus.as_mut() {
            c.eps_low = None;
            c.eps_mid = None;
            c.eps_high = None;
        }
        fallback.prior_verdict = d.prior_verdict.clone();
        fallback.prior_vintage = d.prior_vintage.clone();
        fallback.prior_spot = d.prior_spot;
        fallback.prior_authoring_close = d.prior_authoring_close.clone();
        let (v2, a2) = analyze_holding(&StubAnalyst, &fallback, &rates(), "2026-08-03").unwrap();
        let n = a2.narrative.as_ref().unwrap_or_else(|| {
            panic!(
                "fallback narrative reads; disposition {:?}; degraded: {:?}",
                v2.disposition, a2.degraded_inputs
            )
        });
        assert!(
            n.expansion.abs() < 0.05,
            "bridged pace is flat, not a split-shaped collapse: {}",
            n.expansion
        );
    }

    #[test]
    fn an_unresolvable_full_pass_bridge_gates_price_conditions_and_carries_the_anchor() {
        // The prior anchor's bar date is absent from this run's series: the
        // basis is unverifiable. The old-basis falsifier ("below 700") would
        // false-cross at spot 195 if compared — it must be gated out whole, the
        // degraded input recorded, and the prior anchor carried forward so a
        // later pass stays fail-closed (and heals) instead of reading ~1.0.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let mut ledger = prior_with_conditions();
        ledger.conditions = vec![LedgerCondition {
            condition_id: "px-1".into(),
            role: ConditionRole::Falsifier,
            trigger_family: None,
            label: None,
            statement: "price below $700".into(),
            quant: Some(QuantCore {
                series: engine::LedgerSeries::Price,
                comparator: LedgerComparator::Below,
                threshold: 700.0,
                margin: 0.0,
            }),
            downgraded_reason: None,
            technology_class: false,
            tripped: false,
            supersedes: None,
            eval_state: None,
        }];
        d.prior_verdict = Some(HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated { reason: "fixture".into() },
            thesis_ledger: Some(ledger),
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        });
        d.prior_vintage = Some("2026-06-15T20:00:00Z".into());
        d.prior_spot = Some(780.0);
        d.prior_authoring_close =
            Some(DatedValue { date: "2026-06-15".into(), value: 760.0 });

        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(matches!(verdict.disposition, VerdictDisposition::Priced(_)));
        let la = audit.ledger_audit.as_ref().expect("ledger audit");
        assert!(la.crossings.is_empty(), "gated, never cross-basis: {:?}", la.crossings);
        assert!(
            audit
                .degraded_inputs
                .iter()
                .any(|g| g.contains("split-bridge anchor")),
            "the exclusion is a recorded degraded input: {:?}",
            audit.degraded_inputs
        );
        // The prior anchor CARRIES — provenance preserved, so the carried
        // old-basis threshold stays tied to its own basis rather than being
        // certified fresh (never re-detectable) or dropped (fail-open next pass).
        assert_eq!(
            audit.authoring_close,
            Some(DatedValue { date: "2026-06-15".into(), value: 760.0 }),
            "the unresolvable pass carries the prior anchor forward"
        );
        // The carried-verbatim condition stays quantitative as stored, never
        // half-converted (the stub re-emits it unchanged).
        let persisted = verdict.thesis_ledger.as_ref().expect("ledger persists");
        let px = persisted
            .conditions
            .iter()
            .find(|c| c.condition_id == "px-1")
            .expect("carried id survives");
        assert_eq!(px.quant.as_ref().expect("still quantitative").threshold, 700.0);
        // No fresh anchor-dependent comparator persists beneath the carried
        // anchor: the quick basis is withheld and the monitor stamps no fresh
        // engine targets, so nothing on this row can double-convert when the
        // anchor later resolves.
        assert!(
            audit.quick_basis.is_none(),
            "no fresh quick basis beneath a carried anchor: {:?}",
            audit.quick_basis
        );
        assert!(
            persisted.monitor.iter().all(|m| m.engine_target.is_none()),
            "no fresh engine targets beneath a carried anchor: {:?}",
            persisted.monitor
        );

        // Pass 2 fed from pass 1's ACTUAL persisted outputs (the prior spot
        // comes from pass 1's quick basis, which was withheld), the bar still
        // missing: STILL fail-closed — no crossing, the anchor still carried.
        // The original F1 hole was exactly this pass reading a dropped anchor
        // as factor 1.0.
        let mut d2 = dossier(AssetClass::Stock, strong_financials());
        d2.prior_verdict = Some(verdict.clone());
        d2.prior_vintage = Some("2026-06-15T20:00:00Z".into());
        d2.prior_spot = audit.quick_basis.as_ref().map(|b| b.spot);
        d2.prior_authoring_close = audit.authoring_close.clone();
        let (v2, a2) = analyze_holding(&StubAnalyst, &d2, &rates(), "2026-08-04").unwrap();
        let la2 = a2.ledger_audit.as_ref().expect("ledger audit");
        assert!(la2.crossings.is_empty(), "still gated: {:?}", la2.crossings);
        assert_eq!(
            a2.authoring_close.as_ref().map(|b| b.date.as_str()),
            Some("2026-06-15"),
            "the anchor keeps carrying while unresolvable"
        );
        let px2 = v2
            .thesis_ledger
            .as_ref()
            .unwrap()
            .conditions
            .iter()
            .find(|c| c.condition_id == "px-1")
            .expect("carry holds");
        assert_eq!(px2.quant.as_ref().unwrap().threshold, 700.0);

        // Pass 3, the anchor's bar back in the fresh window (190 = 760 ÷ 4):
        // the carried anchor resolves, the threshold converts under its carried
        // id, and a fresh anchor re-stamps. Fail-closed healed into correct.
        let mut fin3 = strong_financials();
        fin3.daily_closes
            .push(DatedValue { date: "2026-06-15".into(), value: 190.0 });
        fin3.daily_closes.sort_by(|a, b| a.date.cmp(&b.date));
        let mut d3 = dossier(AssetClass::Stock, fin3);
        d3.prior_verdict = Some(v2.clone());
        d3.prior_vintage = Some("2026-06-15T20:00:00Z".into());
        d3.prior_spot = a2.quick_basis.as_ref().map(|b| b.spot);
        d3.prior_authoring_close = a2.authoring_close.clone();
        let (v3, a3) = analyze_holding(&StubAnalyst, &d3, &rates(), "2026-08-05").unwrap();
        let px3 = v3
            .thesis_ledger
            .as_ref()
            .unwrap()
            .conditions
            .iter()
            .find(|c| c.condition_id == "px-1")
            .expect("carried id survives the healing conversion");
        assert!(
            (px3.quant.as_ref().unwrap().threshold - 175.0).abs() < 1e-9,
            "healed conversion: {}",
            px3.quant.as_ref().unwrap().threshold
        );
        assert_eq!(
            a3.authoring_close.as_ref().map(|b| b.date.as_str()),
            Some("2026-07-15"),
            "a resolvable pass re-stamps its own fresh anchor"
        );
        // The healed pass persists fresh comparators again, coherent with its
        // fresh anchor.
        assert!(a3.quick_basis.is_some(), "quick basis returns with a fresh anchor");
        assert!(
            v3.thesis_ledger
                .as_ref()
                .unwrap()
                .monitor
                .iter()
                .all(|m| m.engine_target.is_some()),
            "engine targets return with a fresh anchor"
        );
    }

    #[test]
    fn an_unverified_basis_downgrades_a_reanchored_price_core_but_keeps_a_carried_one() {
        // The supersede guard: with the bridge unresolvable, a RE-ANCHORED
        // price core (authored against fresh prices) cannot persist under the
        // carried prior-basis anchor — it downgrades, typed. A carried-verbatim
        // core shares the carried anchor's basis and stays quantitative.
        let prior = {
            let mut l = prior_with_conditions();
            l.conditions = vec![LedgerCondition {
                condition_id: "px-1".into(),
                role: ConditionRole::Falsifier,
                trigger_family: None,
                label: None,
                statement: "price below $700".into(),
                quant: Some(QuantCore {
                    series: engine::LedgerSeries::Price,
                    comparator: LedgerComparator::Below,
                    threshold: 700.0,
                    margin: 0.0,
                }),
                downgraded_reason: None,
                technology_class: false,
                tripped: false,
                supersedes: None,
                eval_state: None,
            }];
            l
        };
        let draft = |threshold: f64| LedgerDraft {
            thesis: "t".into(),
            key_drivers: vec![],
            bear: ScenarioDraft { conditions: "b".into(), probability_pct: 30.0 },
            base: ScenarioDraft { conditions: "m".into(), probability_pct: 40.0 },
            bull: ScenarioDraft { conditions: "u".into(), probability_pct: 30.0 },
            what_must_improve: String::new(),
            what_must_not_break: String::new(),
            falsifiers: vec![FalsifierDraft {
                statement: format!("price below ${threshold}"),
                quant: Some(QuantCoreDraft {
                    series: "price".into(),
                    comparator: "below".into(),
                    threshold,
                    margin: 0.0,
                }),
                technology_class: false,
                tripped: false,
            }],
            triggers: vec![],
        };
        // Re-anchored core, basis unverified → downgraded with the typed reason.
        let (ledger, audit) = validate_ledger_rewrite_with_research(
            &draft(150.0),
            Some(&prior),
            None,
            LedgerBranch::Priced,
            false,
            None,
            None,
            None,
            &std::collections::HashSet::new(),
            false,
            crate::portfolio::ContinuityStamps::NONE,
        );
        let c = &ledger.conditions[0];
        assert!(c.quant.is_none(), "re-anchored core must not persist: {c:?}");
        assert!(
            c.downgraded_reason
                .as_deref()
                .is_some_and(|r| r.contains("unverifiable")),
            "{c:?}"
        );
        assert!(audit.downgraded.iter().any(|d| d.contains("unverifiable")));
        // Carried-verbatim core, basis unverified → stays quantitative.
        let (ledger, _) = validate_ledger_rewrite_with_research(
            &draft(700.0),
            Some(&prior),
            None,
            LedgerBranch::Priced,
            false,
            None,
            None,
            None,
            &std::collections::HashSet::new(),
            false,
            crate::portfolio::ContinuityStamps::NONE,
        );
        let c = &ledger.conditions[0];
        assert_eq!(c.condition_id, "px-1", "carried id");
        assert_eq!(c.quant.as_ref().expect("stays quantitative").threshold, 700.0);
        // Same re-anchored core with a VERIFIED basis supersedes normally.
        let (ledger, audit) = validate_ledger_rewrite_with_research(
            &draft(150.0),
            Some(&prior),
            None,
            LedgerBranch::Priced,
            false,
            None,
            None,
            None,
            &std::collections::HashSet::new(),
            true,
            crate::portfolio::ContinuityStamps::NONE,
        );
        let c = &ledger.conditions[0];
        assert_eq!(c.quant.as_ref().expect("quant supersede").threshold, 150.0);
        assert_eq!(c.supersedes.as_deref(), Some("px-1"));
        assert!(audit.superseded.len() == 1);
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
    fn position_change_line_states_the_direction_and_no_figure() {
        // Direction only since portfolio-v38 (fix list 3.2, ruled 2026-09-16):
        // the quantity and cost-basis moves are account economics the intrinsic
        // packet withholds; the delta's direction still reaches the read
        // (`docs/portfolio-analysis.md` §Holdings change tracking).
        let increased = PositionDelta {
            change: PositionChange::Increased,
            prior_quantity: Some(100.0),
            prior_cost_basis: Some(14_000.0),
        };
        let line = describe_position_change(&increased);
        assert_eq!(line, "The position grew since the prior analysis.");
        for figure in ["100", "140", "14000", "19500", "$", "cost basis", "quantity"] {
            assert!(!line.contains(figure), "{figure} leaked: {line}");
        }
        let decreased = PositionDelta { change: PositionChange::Decreased, ..increased };
        assert_eq!(
            describe_position_change(&decreased),
            "The position shrank since the prior analysis."
        );
        let unchanged = PositionDelta { change: PositionChange::Unchanged, ..increased };
        assert_eq!(
            describe_position_change(&unchanged),
            "The position is unchanged since the prior analysis."
        );
        // The debut line must disarm the fresh-purchase misread: a first analysis
        // says nothing about when the position was entered (`portfolio-v40`
        // drops the NEW marker and its gloss for the plain sentence).
        let debut = describe_position_change(&PositionDelta::new_position());
        assert_eq!(debut, "This is the first analysis of this holding.");
        for narration in ["NEW", "purchase", "cost basis", "may long predate", "prior verdict"] {
            assert!(!debut.contains(narration), "`{narration}` leaked: {debut}");
        }
    }

    #[test]
    fn interpretation_prompt_carries_the_computed_numbers_in_two_parts_and_no_app_narration() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let input = InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        };
        let user = interpretation_user_prompt(&input);
        // One message in two marked parts (`portfolio-v40`): the inputs, then
        // the task.
        let part1 = user.find("======== PART 1: INPUTS ========").expect("part 1");
        let part2 = user.find("======== PART 2: TASK ========").expect("part 2");
        assert!(part1 < part2, "{user}");
        assert_eq!(user.matches("======== PART").count(), 2, "{user}");
        // The computed numbers, each section explained once and then its values.
        assert!(
            user.contains(
                "\nCOMPUTED SCORES\nFour scores from 0 to 100, higher is better on every axis"
            ),
            "{user}"
        );
        assert!(
            user.contains(&format!(
                "quality {:.0}, valuation {:.0}, momentum {:.0}, risk {:.0}. Risk tier: {}.",
                engine_output.sub_scores.quality,
                engine_output.sub_scores.valuation,
                engine_output.sub_scores.momentum,
                engine_output.sub_scores.risk,
                engine_output.risk_tier.as_str(),
            )),
            "{user}"
        );
        assert!(user.contains("\nCOMPUTED PRICE TARGETS (USD)\n- twelve-month: bear "), "{user}");
        assert!(user.contains(". Method: "), "{user}");
        assert!(!user.contains("prorated to one month") && !user.contains("capped at 15%"), "{user}");
        let scores = user.split("COMPUTED SCORES\n").nth(1).unwrap().split("COMPUTED PRICE TARGETS").next().unwrap();
        assert!(!scores.contains("Grade") && !scores.contains("grade"), "{scores}");
        assert!(user.contains("\nOPTIONS ACTIVITY\nput/call volume "), "{user}");
        assert!(user.contains("\nRESEARCH SUMMARY\ndistilled findings\n"), "{user}");
        // The task states the scale and the domain as requirements on the output.
        assert!(
            user.contains(
                "2. model_sub_scores — your own quality, valuation, momentum and risk, as \
                 integers on the scale defined in COMPUTED SCORES"
            ),
            "{user}"
        );
        assert!(user.contains("as positive prices in USD, bear ≤ base ≤ bull"), "{user}");
        // The model is told nothing about the app: no arms, baselines, stages,
        // validator behaviour, product names or stamps — and no weighing
        // narrative (the F6 cross-check and the provenance / capital-efficiency
        // lines stay in the action packet).
        for narration in [
            "ENGINE ",
            "engine arm",
            "model arm",
            "MODEL ARM",
            "TWO ARMS",
            "baseline",
            "deterministic",
            "the app",
            "validator",
            "rejected",
            "downgraded",
            "this stage",
            "never a gate",
            "NOT a grade input",
            "Market Signal",
            "portfolio-v",
            PROMPT_VERSION,
            "TARGET PROVENANCE",
            "dead money",
            "CAPITAL-EFFICIENCY",
            "hurdle",
            "one input to weigh",
            "Weigh the targets by this provenance",
            "unrestricted",
            "UNRESTRICTED",
        ] {
            assert!(!user.contains(narration), "`{narration}` leaked: {user}");
        }
        // Profile independence is input isolation, not instruction
        // (`docs/portfolio-workflow.md` §Step 6f "deliberately absent"): the
        // intrinsic prompt renders no investor profile; the profile enters at
        // the per-holding action call only.
        assert!(!user.contains("INVESTOR PROFILE"), "{user}");
        // Interpretation authors no action under the tunnel-vision contract —
        // the shape requests none, so no instruction has to forbid one.
        assert!(!crate::portfolio::interpretation_keys(false).contains(&"action"));
        assert!(!user.contains("portfolio action"), "{user}");

        // The system prompt: the role, the two-part shape and the output names —
        // nothing that describes the data or the app.
        let system = interpretation_system_prompt(false, false);
        assert!(
            system.starts_with(
                "You are an equity analyst producing an independent read of one holding for a \
                 portfolio review. Part 1 of the message gives the inputs. Part 2 states what \
                 to determine from them and the shape to return. "
            ),
            "{system}"
        );
        assert!(
            system.ends_with(&crate::portfolio::interpretation_response_contract(false)),
            "{system}"
        );
        for narration in [
            "TWO ARMS",
            "MODEL ARM",
            "never outside them",
            "profile-independent",
            "Do NOT choose a portfolio action",
            "Conviction means",
            "THESIS LEDGER",
            "never by itself a reason to exit",
            "engine",
            "baseline",
        ] {
            assert!(!system.contains(narration), "`{narration}` leaked: {system}");
        }
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
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            interp.contains(
                "\nCOMMODITY PRICES (matched to this holding's sector; each as of its print \
                 date, and a monthly series lags)\n"
            ),
            "{interp}"
        );
        assert!(
            interp.contains(
                "- WTI Crude Oil: 78.40 USD per barrel (as of 2026-08-18; -4.5% vs 82.10 on \
                 2025-08-20)\n"
            ),
            "{interp}"
        );
        // Data only: no score-input narration (`portfolio-v40`).
        for narration in ["COMMODITY CONTEXT", "never a score input", "as-of evidence"] {
            assert!(!interp.contains(narration), "`{narration}` leaked: {interp}");
        }
        // A holding with no sector-matched prints renders no section.
        let bare = dossier(AssetClass::Stock, strong_financials());
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &bare,
            prior_ledger: bare.prior_ledger(),
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(!interp.contains("COMMODITY PRICES"), "{interp}");
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
                prior_ledger: d.prior_ledger(),
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
        assert!(
            user.contains(
                "\nSECTOR-RELATIVE MOVE\n-12.0% versus XLK over 4 sessions since the prior \
                 analysis, beyond the ±8.0% threshold (two times the interval-scaled realized \
                 volatility). A possible third-party repricing event; the cause is not known.\n"
            ),
            "{user}"
        );
        for narration in ["TECHNOLOGY-EVENT PRE-FLAG", "PRE-FLAG", "asserts nothing about the cause"] {
            assert!(!user.contains(narration), "`{narration}` leaked: {user}");
        }
        let unfired = flag(false);
        assert!(!prompt(Some(&unfired)).contains("SECTOR-RELATIVE MOVE"));
        assert!(!prompt(None).contains("SECTOR-RELATIVE MOVE"));
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
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        const LINE: &str = "Market-wide options sentiment (CBOE daily put/call, as of August 19, \
                            2026): total 0.80, index 0.97, equity (gap)\n";
        assert!(interp.contains(LINE), "{interp}");
        assert!(!interp.contains("MARKET OPTIONS SENTIMENT"), "{interp}");

        let mut fd = fund_dossier(us_equity_fund());
        fd.put_call_backdrop = Some(backdrop);
        let role = role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: &fd,
            prior_ledger: fd.prior_ledger(),
            readout: &RoleRiskReadout::default(),
            ledger_eval: None,
            distilled: "No research findings.",
        });
        assert!(role.contains(LINE), "{role}");
        // Absent, neither prompt claims it.
        let bare = dossier(AssetClass::Stock, strong_financials());
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &bare,
            prior_ledger: bare.prior_ledger(),
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(!interp.contains("Market-wide options sentiment"), "{interp}");
    }

    #[test]
    fn a_tripped_hard_forensic_states_the_rule_and_annotates_the_audit() {
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
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            interp.contains(
                "\nFORENSIC FILINGS (8-K sweep)\nEvents found:\n- restatement (Item 4.02 \
                 non-reliance) — filed 2026-07-20 (8-K accession 0000320193-26-000042; \
                 filing-declared item code)\n"
            ),
            "{interp}"
        );
        // The consequence is stated as the rule's effect on the computed read —
        // no arm, trigger or binding narration (`portfolio-v40`).
        const RULE: &str = "By rule: the computed conviction is capped at low and the computed \
                            action set excludes adding; the grade is unchanged.\n";
        assert!(interp.contains(RULE), "{interp}");
        for narration in ["HARD TRIGGER TRIPPED", "engine arm", "ENGINE arm", "binds"] {
            assert!(!interp.contains(narration), "`{narration}` leaked: {interp}");
        }
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
                ledger: v.thesis_ledger.as_ref().unwrap(),
            },
            engine_set: &engine_set,
            changes: None,
            profile: &d.profile,
        });
        assert!(action.contains("\nFORENSIC FILINGS (8-K sweep)\nEvents found:\n"), "{action}");
        // The action packet carries the narrowed set on its own line, so the
        // sweep's rule sentence renders on the interpretation packet only
        // (ruled 2026-09-17, F1).
        assert!(!action.contains("By rule:"), "{action}");
        assert!(
            action.contains("\nSUPPORTED ACTIONS (computed)\nThe rungs the computed read supports on its own: sell-all, trim, hold.\n"),
            "{action}"
        );
        assert!(!action.contains("HARD TRIGGER TRIPPED"), "{action}");

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
            prior_ledger: unknown.prior_ledger(),
            engine: &engine_output,
            distilled: "findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            interp.contains(
                "\nFORENSIC FILINGS (8-K sweep)\nUnknown — no CIK mapping for AAPL. Not a clean \
                 check.\n"
            ),
            "{interp}"
        );
        assert!(!interp.contains("By rule:"), "{interp}");
    }

    #[test]
    fn action_prompt_is_two_parts_with_both_reads_the_supported_set_and_the_profile() {
        // The action call is the profile's ONE entry point (tunnel vision): the
        // message renders the two reads, the computed per-holding set as one
        // data line, and the profile — and names no app concept
        // (`portfolio-v41`; `docs/verification/2026-09-17-action-prompt-rewrite.md`).
        let d = dossier(AssetClass::Stock, strong_financials());
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        let ledger = v.thesis_ledger.as_ref().unwrap();
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let engine_set =
            engine::feasible_actions(engine_output.grade, &engine_output.hurdle, None, false);
        let user = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced { graded, engine: &engine_output, pre_profit: None, ledger },
            engine_set: &engine_set,
            changes: None,
            profile: &d.profile,
        });
        let system = action_system_prompt();
        assert_eq!(
            system,
            "You are an equity analyst deciding the portfolio action for one holding in a \
             portfolio review. Part 1 of the message gives the inputs. Part 2 states what to \
             determine from them and the shape to return. You will return action and \
             rationale, as one JSON object."
        );
        let (part1, part2) = user.split_once("\n======== PART 2: TASK ========\n").unwrap();
        assert!(part1.starts_with("======== PART 1: INPUTS ========\nHOLDING\n"), "{part1}");
        for section in [
            "SCORES\n",
            "PRICE TARGETS (USD, with the move each implies from the current price)\n",
            "TARGET RATIONALE (analyst)\n",
            "CAPITAL EFFICIENCY\n",
            "CONVICTION AND OUTLOOK (analyst)\n",
            "FINANCIAL SUMMARY (analyst)\n",
            "THESIS (analyst)\n",
            "SCENARIOS (analyst)\n",
            "SUPPORTED ACTIONS (computed)\n",
            "INVESTOR PROFILE\n",
        ] {
            assert_eq!(part1.matches(&format!("\n{section}")).count(), 1, "{section}: {part1}");
        }
        // Part 1 instructs nothing; Part 2 carries the two items and the shape.
        assert!(!part1.contains("Return "), "{part1}");
        assert!(
            part2.contains(
                "\n1. action — one rung for this holding, from these inputs alone: \"sell-all\", \
                 \"trim\", \"hold\", \"add\" or \"add-aggressively\". The rung alone: no share count, \
                 dollar amount or portfolio weight. Decide it from SCORES and PRICE TARGETS first, \
                 refined by CAPITAL EFFICIENCY, CONVICTION AND OUTLOOK, THESIS, SCENARIOS, \
                 SUPPORTED ACTIONS and INVESTOR PROFILE. An aggressive risk tolerance admits \
                 add-aggressively where the other inputs support it. Where even the bull case in \
                 CAPITAL EFFICIENCY misses the hurdle and the forward read is poor, lean toward \
                 realizing some or all of the position.\n"
            ),
            "{part2}"
        );
        assert!(
            part2.contains("\n2. rationale — one sentence giving the single investment reason for the rung. Name the returns you weighed by their values; do not describe them by their relation to another figure.\n"),
            "{part2}"
        );
        assert!(
            part2.ends_with(&format!(
                "\nRETURN SHAPE (every value is a placeholder)\n{}\n",
                crate::portfolio::action_return_shape()
            )),
            "{part2}"
        );
        assert_eq!(
            crate::portfolio::action_return_shape(),
            "{\"action\":\"<sell-all|trim|hold|add|add-aggressively>\",\"rationale\":\"\"}"
        );
        // Both reads' bands reach the rung with the move each implies, both
        // horizons (Codex I5): the stub authors its twelve-month base at 1.05×
        // the computed base, so the two base moves differ on the page.
        let spot = d.financials.current_price.unwrap();
        let leg = |v: f64| format!("{v:.2} ({:+.1}%)", (v / spot - 1.0) * 100.0);
        let engine_12 = graded.price_targets.twelve_month.as_ref().unwrap();
        let model_12 = &graded.model_view.price_targets.twelve_month;
        assert!(
            user.contains(&format!(
                "- computed twelve-month: bear {} / base {} / bull {}. Method: ",
                leg(engine_12.bear), leg(engine_12.base), leg(engine_12.bull)
            )),
            "{user}"
        );
        assert!(
            user.contains(&format!(
                "- analyst twelve-month: bear {} / base {} / bull {}.\n",
                leg(model_12.bear), leg(model_12.base), leg(model_12.bull)
            )),
            "{user}"
        );
        assert_ne!(leg(engine_12.base), leg(model_12.base));
        assert_eq!(user.matches("- computed one-month: ").count(), 1, "{user}");
        assert_eq!(user.matches("- analyst one-month: ").count(), 1, "{user}");
        // The polarity gloss once and the grade's derivation once (2.3 and 3.11
        // as one data gloss); the set once as data with no permission sentence
        // (3.9, ruled 2026-09-17).
        assert_eq!(user.matches("higher is better on every axis").count(), 1, "{user}");
        assert!(
            user.contains(
                "The grade is a letter derived from the quality, valuation and risk scores.\n\
                 - computed: quality "
            ),
            "{user}"
        );
        let set: Vec<&str> = engine_set.iter().map(Action::as_kebab).collect();
        assert!(
            user.contains(&format!(
                "\nSUPPORTED ACTIONS (computed)\nThe rungs the computed read supports on its \
                 own: {}.\n",
                set.join(", ")
            )),
            "{user}"
        );
        // The ledger's thesis and scenarios, as validated this run.
        assert!(user.contains(&format!("\nTHESIS (analyst)\n{}\n", ledger.current_thesis)), "{user}");
        for s in &ledger.monitor {
            assert!(
                user.contains(&format!("- {} ({:.0}%): {}\n", s.scenario.as_str(), s.probability_pct, s.conditions)),
                "{user}"
            );
        }
        // No app concept, no whole-book vocabulary, no account economics, and
        // — on a debut — no continuity section or firmness clause.
        for absent in [
            "ENGINE SET", "ENGINE ARM", "MODEL ARM", "THE VERDICT", "ACTION BASIS", "IMPLIED ",
            "TARGET PROVENANCE", "engine arm", "model arm", "engine targets",
            "model targets", "its own pick", "full ladder", "neither requires nor forbids",
            "indeterminate", "dead money", "scoreboard", "concentration", "OVERLAP", "- cash:",
            "unconstrained", "Unrealized", "Cost basis", "PRIOR ACTION", "PRIOR ANALYSIS",
            "CHANGES SINCE", "Move from PRIOR ACTION", "exactly ONE", "Keep the action firm",
        ] {
            assert!(!user.contains(absent), "`{absent}` in the message: {user}");
            assert!(!system.contains(absent), "`{absent}` in the system prompt: {system}");
        }
    }

    #[test]
    fn action_prompt_distinguishes_rule_demotion_and_follows_tax_posture() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let mut prior = v.clone();
        prior.action_source = ActionSource::RuleDemoted;
        let VerdictDisposition::Priced(prior_graded) = &mut prior.disposition else {
            panic!("expected a priced prior");
        };
        prior_graded.action = Action::Hold;
        d.prior_verdict = Some(prior);
        d.profile.tax_sensitive = false;

        let VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let engine_set =
            engine::feasible_actions(engine_output.grade, &engine_output.hurdle, None, false);
        let ledger = v.thesis_ledger.as_ref().unwrap();
        let render = |d: &HoldingDossier| {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced {
                    graded,
                    engine: &engine_output,
                    pre_profit: None,
                    ledger,
                },
                engine_set: &engine_set,
                changes: None,
                profile: &d.profile,
            })
        };

        let exempt = render(&d);
        // A rule-demoted prior renders as data with its gloss (ruled 2026-09-17,
        // F4) and anchors no firmness clause.
        assert!(
            exempt.contains("\nPRIOR ACTION\nhold, set by rule after the prior analysis, not chosen in it.\n"),
            "{exempt}"
        );
        assert!(!exempt.contains("chosen in the prior analysis."), "{exempt}");
        assert!(!exempt.contains("Move from PRIOR ACTION"), "{exempt}");
        assert!(!exempt.contains("rule-demoted"), "{exempt}");
        // The investment-only packet (2.1, ruled 2026-09-16): no tax row, no
        // P/L, no cost basis under either profile — and the two renders are
        // byte-identical, so the tax posture cannot reach the rung by any route.
        for token in ["tax", "Tax", "Unrealized", "Cost basis", "P/L"] {
            assert!(!exempt.contains(token), "{token} leaked: {exempt}");
        }
        d.profile.tax_sensitive = true;
        let taxable = render(&d);
        assert_eq!(exempt, taxable, "the tax posture must not change the packet");
        d.position.cost_basis *= 3.0;
        let repriced = render(&d);
        assert_eq!(exempt, repriced, "the cost basis must not change the packet");
    }

    #[test]
    fn action_packet_renders_the_hurdle_as_numbers_and_the_continuity_sections() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let VerdictDisposition::Priced(graded) = &prior.disposition else { panic!("priced"); };
        let ledger = prior.thesis_ledger.as_ref().unwrap();
        let mut engine = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(engine) => engine,
            _ => panic!("priced"),
        };
        d.prior_verdict = Some(prior.clone());
        let changes = validate_what_changed(&[crate::portfolio::WhatChangedEntry {
            kind: crate::portfolio::ChangedValueKind::Target,
            detail: "twelve-month base".into(), old: "100".into(), new: "110".into(),
            attribution: crate::portfolio::ChangeAttribution::CompanyInformation,
            evidence: "D1".into(),
        }], vec![crate::portfolio::DeltaEntry {
            id: "D1".into(), label: "forward earnings changed".into(), related_condition_id: None,
        }]);
        for state in [crate::portfolio::HurdleState::Fails, crate::portfolio::HurdleState::Clears,
            crate::portfolio::HurdleState::Indeterminate, crate::portfolio::HurdleState::Unscorable] {
            engine.hurdle.state = state;
            let mut low = graded.clone();
            low.low_confidence_grade = true;
            let prompt = action_user_prompt(&ActionInput {
                dossier: &d, subject: ActionSubject::Priced { graded: &low, engine: &engine, pre_profit: None, ledger },
                engine_set: &[Action::Hold], profile: &d.profile, changes: Some(&changes),
            });
            // CAPITAL EFFICIENCY as numbers (ruled 2026-09-17 off L19): the three
            // tested returns and the hurdle, no state word, no reach sentence; an
            // unscorable read says no assessment exists.
            assert_eq!(prompt.matches("Name the returns you weighed by their values").count(), 1);
            let h = &engine.hurdle;
            if state == crate::portfolio::HurdleState::Unscorable {
                assert!(prompt.contains("\nCAPITAL EFFICIENCY\nNo assessment this run.\n"), "{prompt}");
            } else {
                assert!(
                    prompt.contains(&format!(
                        "\nCAPITAL EFFICIENCY\nThe computed twelve-month total return in each scenario \
                         (the move from the current price to the scenario price, plus forward income \
                         per share, as a fraction of the current price) and the hurdle rate it is \
                         measured against.\nbear {:+.1}% / base {:+.1}% / bull {:+.1}%; hurdle {:.1}%.\n",
                        h.tr_bear.unwrap() * 100.0, h.tr_base.unwrap() * 100.0,
                        h.tr_bull.unwrap() * 100.0, h.hurdle_rate.unwrap() * 100.0
                    )),
                    "{prompt}"
                );
            }
            for absent in [
                "indeterminate", "dead money", "neither requires nor forbids", "fails —", "clears —",
                "twelve-month total-return test", "carries no exit signal",
                "a property of the grade, not the conviction",
            ] {
                assert!(!prompt.contains(absent), "`{absent}`: {prompt}");
            }
            // The sunk-cost rule is one task clause on every priced packet.
            assert_eq!(prompt.matches("Where even the bull case in CAPITAL EFFICIENCY misses the hurdle").count(), 1, "{prompt}");
            // The low-confidence letter is a gloss on the computed grade line.
            assert!(prompt.contains(&format!("Grade {}{LOW_CONFIDENCE_GLOSS}. Risk tier: ", low.grade.as_str())), "{prompt}");
            // The continuity sections: the prior action as chosen, the prior read,
            // the validated rows in words with their evidence ids, and the
            // firmness clause in the task.
            assert!(prompt.contains(&format!("\nPRIOR ACTION\n{}, chosen in the prior analysis.\n", graded.action.as_kebab())), "{prompt}");
            assert!(
                prompt.contains(&format!(
                    "\nPRIOR ANALYSIS\ncomputed grade {}; analyst grade {}; conviction {}; outlook short {}, mid {}, long {}.\nFinancial summary: {}\n",
                    graded.grade.as_str(), graded.model_view.letter.as_str(), graded.conviction.as_str(),
                    graded.horizon_outlook.short.as_str(), graded.horizon_outlook.mid.as_str(),
                    graded.horizon_outlook.long.as_str(), graded.financial_summary
                )),
                "{prompt}"
            );
            assert!(
                prompt.contains(&format!(
                    "\nCHANGES SINCE THE PRIOR ANALYSIS\nSummary (analyst): {}\n- twelve-month base: 100 -> 110 (company information; evidence D1)\n[D1] forward earnings changed\n",
                    low.what_changed
                )),
                "{prompt}"
            );
            assert!(prompt.contains("SCENARIOS, PRIOR ANALYSIS, CHANGES SINCE THE PRIOR ANALYSIS, SUPPORTED ACTIONS and INVESTOR PROFILE."), "{prompt}");
            assert!(prompt.contains(" Move from PRIOR ACTION only where the inputs have materially changed since the prior analysis.\n"), "{prompt}");
            for absent in ["Prior engine grade", "continuity baseline", "Validated change attribution is unavailable", "CompanyInformation"] {
                assert!(!prompt.contains(absent), "`{absent}`: {prompt}");
            }
            // With no hurdle rate there is no assessment to render.
            let mut no_rate = engine.clone();
            no_rate.hurdle.hurdle_rate = None;
            let prompt = action_user_prompt(&ActionInput {
                dossier: &d, subject: ActionSubject::Priced { graded: &low, engine: &no_rate, pre_profit: None, ledger },
                engine_set: &[Action::Hold], profile: &d.profile, changes: Some(&changes),
            });
            assert!(prompt.contains("\nCAPITAL EFFICIENCY\nNo assessment this run.\n"), "{prompt}");
            assert!(!prompt.contains("; hurdle "), "{prompt}");
        }
        // Absent attribution says so and is never read as unchanged evidence.
        let prompt = action_user_prompt(&ActionInput {
            dossier: &d, subject: ActionSubject::Priced { graded, engine: &engine, pre_profit: None, ledger },
            engine_set: &[Action::Hold], profile: &d.profile, changes: None,
        });
        assert!(prompt.contains("\nCHANGES SINCE THE PRIOR ANALYSIS\nSummary (analyst): "), "{prompt}");
        assert!(prompt.contains("\nNo change attribution is available.\n"), "{prompt}");
    }

    #[test]
    fn role_risk_ledger_observation_uses_daily_short_history_without_conversion() {
        let mut d = dossier(AssetClass::Etf, strong_financials());
        d.financials.price_history = vec![100.0, 101.0, 99.0, 102.0];
        d.financials.daily_closes = vec![
            DatedValue { date: "2025-01-01".into(), value: 20.0 },
            DatedValue { date: "2026-01-01".into(), value: 100.0 },
        ];
        let readout = RoleRiskReadout { observable_risk: Some(0.417), ..Default::default() };
        let prompt = role_risk_user_prompt(&RoleRiskInput {
            dossier: &d, readout: &readout, prior_ledger: None, ledger_eval: None,
            input_delta: &[], distilled: "research",
        });
        let daily = engine::compute_metrics(&d.financials).return_volatility.unwrap();
        // The daily figure renders once, as the FINANCIAL METRICS line the
        // ledger evaluates; the annualized read sits under RISK PROFILE with
        // its unit gloss (`portfolio-v42`).
        assert!(
            prompt.contains(&format!(
                "- daily realized return volatility [return-volatility]: {daily:.4} — a daily fraction"
            )),
            "{prompt}"
        );
        assert_eq!(prompt.matches("[return-volatility]").count(), 1, "{prompt}");
        assert!(
            prompt.contains(
                "\nRISK PROFILE\nAnnualized realized volatility: 0.417 (a fraction; 0.14 means 14% a year).\n"
            ),
            "{prompt}"
        );
        for narration in ["short price-history window", "LEDGER OBSERVATION", "OBSERVABLE RISK", "deep history"] {
            assert!(!prompt.contains(narration), "`{narration}` leaked: {prompt}");
        }
        assert!(prompt.contains("Price: $195.00 per share.\n"), "{prompt}");
        assert!(!prompt.contains("Current price"), "{prompt}");
        assert!((daily - 0.417 / 15.87).abs() > 0.001);
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
                prior_ledger: d.prior_ledger(),
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
                && p.contains(
                    "Rule matched, capping the computed conviction: narrative-vs-reality hype: \
                     test rule.\n"
                ),
            "{p}"
        );
        assert!(!p.contains("binds the ENGINE arm"), "{p}");
        assert!(p.contains("- What the current price implies, at each scenario's multiple:"), "{p}");
        assert!(!p.contains("IMPLIED EXPECTATIONS"), "{p}");
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
                ledger: v.thesis_ledger.as_ref().unwrap(),
            },
            engine_set: &engine_set,
            changes: None,
            profile: &d.profile,
        });
        assert!(action.contains("SAME-UNDERLYING OPTION OVERLAY"), "{action}");
        assert!(!action.contains("SHORT INTEREST"), "positioning stays interpretation-side: {action}");
    }

    #[test]
    fn action_prompt_tags_off_domain_analyst_legs_and_gaps_computed_legs_as_authored() {
        // The render annotates, never reorders or drops (Codex I5, ruled
        // 2026-08-28): a computed band the scenario function could not derive
        // prints `(gap)`; an analyst leg outside the declared domain prints as
        // authored with its tag in place of a price and move; a band authored
        // bear above bull carries the inverted tag. I6 owns the upstream domain
        // validation; this is the render's fail-closed read.
        let d = dossier(AssetClass::Stock, strong_financials());
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        let ledger = v.thesis_ledger.as_ref().unwrap();
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let engine_set =
            engine::feasible_actions(engine_output.grade, &engine_output.hurdle, None, false);
        let mut g = graded.clone();
        g.price_targets.one_month = None;
        g.model_view.price_targets.one_month = ModelPriceTarget { base: 100.0, bear: 120.0, bull: 90.0 };
        g.model_view.price_targets.twelve_month = ModelPriceTarget { base: f64::NAN, bear: -5.0, bull: 0.0 };
        let render = |d: &HoldingDossier, g: &GradedVerdict| {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded: g, engine: &engine_output, pre_profit: None, ledger },
                engine_set: &engine_set,
                changes: None,
                profile: &d.profile,
            })
        };
        let user = render(&d, &g);
        let spot = d.financials.current_price.unwrap();
        let leg = |v: f64| format!("{v:.2} ({:+.1}%)", (v / spot - 1.0) * 100.0);
        assert!(user.contains("- computed one-month: (gap)\n"), "{user}");
        // The computed twelve-month band is untouched and still renders with moves.
        let engine_12 = g.price_targets.twelve_month.as_ref().unwrap();
        assert!(user.contains(&format!("- computed twelve-month: bear {}", leg(engine_12.bear))), "{user}");
        // Inverted: authored numbers, authored order, the tag beside them.
        assert!(
            user.contains(&format!(
                "- analyst one-month: bear {} / base {} / bull {} (band inverted as authored).\n",
                leg(120.0), leg(100.0), leg(90.0)
            )),
            "{user}"
        );
        // Off-scale: the raw authored value with its tag, no price or move.
        assert!(
            user.contains(
                "- analyst twelve-month: bear -5 (off-scale as authored) / base NaN (off-scale as \
                 authored) / bull 0 (off-scale as authored).\n"
            ),
            "{user}"
        );
        // Bear -5 sits below bull 0 on plain arithmetic, so the off-scale band
        // carries no inverted tag here; the two tags are independent reads.
        assert_eq!(user.matches("band inverted as authored").count(), 1, "{user}");
        // A NaN leg compares false on the inverted predicate, so a NaN bear or
        // bull is never tagged inverted; an in-domain bear above an off-scale
        // bull carries both tags — annotate, never drop.
        let mut both = g.clone();
        both.model_view.price_targets.one_month = ModelPriceTarget { base: 100.0, bear: f64::NAN, bull: 90.0 };
        both.model_view.price_targets.twelve_month = ModelPriceTarget { base: 100.0, bear: 120.0, bull: -5.0 };
        let user3 = render(&d, &both);
        assert!(
            user3.contains(&format!(
                "- analyst one-month: bear NaN (off-scale as authored) / base {} / bull {}.\n",
                leg(100.0), leg(90.0)
            )),
            "{user3}"
        );
        assert!(
            user3.contains(&format!(
                "- analyst twelve-month: bear {} / base {} / bull -5 (off-scale as authored) (band inverted as authored).\n",
                leg(120.0), leg(100.0)
            )),
            "{user3}"
        );
        assert_eq!(user3.matches("band inverted as authored").count(), 1, "{user3}");
        // A finite, positive leg whose move from spot overflows the percentage
        // arithmetic is off-scale too — the guard reads the derived move, so the
        // message never carries `inf%` (Codex round 1).
        let mut penny = strong_financials();
        penny.current_price = Some(1.0);
        let d4 = dossier(AssetClass::Stock, penny);
        let mut huge = g.clone();
        huge.model_view.price_targets.twelve_month = ModelPriceTarget { base: 1e308, bear: 0.5, bull: 2.0 };
        let user4 = render(&d4, &huge);
        assert!(!user4.contains("inf"), "{user4}");
        assert!(
            user4.contains("- analyst twelve-month: bear 0.50 (-50.0%) / base 1")
                && user4.contains("0 (off-scale as authored) / bull 2.00 (+100.0%).\n"),
            "{user4}"
        );
        // No usable current price → prices without moves on both reads; the
        // method clauses still render, since they describe the bands.
        let mut unpriced = strong_financials();
        unpriced.current_price = None;
        let d2 = dossier(AssetClass::Stock, unpriced);
        let user2 = render(&d2, &g);
        assert!(
            user2.contains(&format!(
                "- computed twelve-month: bear {:.2} / base {:.2} / bull {:.2}. Method: ",
                engine_12.bear, engine_12.base, engine_12.bull
            )),
            "{user2}"
        );
        assert!(
            user2.contains("- analyst one-month: bear 120.00 / base 100.00 / bull 90.00 (band inverted as authored).\n"),
            "{user2}"
        );
    }

    #[test]
    fn prompt_version_is_stamped_for_the_model_arm_domain_gate() {
        // Codex I5 changed the action call's evidence set (both arms' targets,
        // both horizons) and Codex I6 made the model arm's declared domain a
        // decode gate with its clauses in the prompt, so a pre-fix checkpoint
        // cannot resume into rows the gate would reject: the stamp moved and
        // stays pinned. Group 3 (Codex I8 / I10 / I12 renders and the I19
        // period-word guard, ruled 2026-08-29) moved it again, and group 4
        // (the Codex I11 target-boundary NOTE and the I13 equity-source line
        // in the basis sentence, beside the new evaluation-state stamp) again;
        // Review 2 M11's constant-period revision semantics moved it to v25;
        // M1's forward-assumption currency admission moved it to v26; the M4 /
        // M5 / Q4 fund-classification contract moved it to v27; M14's
        // industry-routed commodity context moved it to v28; M17 / M19 / M20's
        // comparison-safe delta, action-provenance, and profile-tax contract
        // moved it to v29; M24–M27's typed-channel validation and seed-lineage
        // contract moved it to v30; the 2026-08-30 big-run Finding 2
        // schema-carrying prompt-clarity touches (the ledger `quant` object and
        // its prose-only anti-pattern with `technology_class` explained, the same
        // anti-pattern on the distillation typed fields, and the what-changed
        // row's named fields) move it to v31; the same run's Finding 3 action-call
        // prompt clarity (the app-stamps-the-departure statement and the
        // non-`fails` capital-efficiency neutrality clause, both prompts) moves it
        // to v32; attempt 4's Finding 4 fix B — splitting the research gathering
        // turn (tools, no grammar) from a fresh, tool-history-free synthesis call
        // (grammar, no tools) — reworded both the gathering and synthesis prompts,
        // moving it to v33 (so an interrupted pre-fix run cannot resume into the
        // new synthesis contract); fix B's post-landing review reworded the
        // synthesis brief again (drop empty-body pages, lead each source with its
        // title, prepend a gathering-degradation note), changing the synthesis
        // input and so a completed holding's analysis, moving it to v34. The
        // final pre-debut sweep's aggregate gather-packet / tool-batch bounds and
        // joint header-plus-body evidence selector fold into that same never-run
        // v34 contract. Attempt 5 ran v34 (four holdings persist under it); its
        // Finding-5 fix shows the synthesis prompt the findings object's shape,
        // changing the synthesis input again, so it moves to v35.
        // Attempt 6 ran v35 (six holdings persist under it); the ledger-authoring
        // contract, the 6g agreement checks and the investment-only action packet
        // change the prompts and the model-facing contract, so they move to v36.
        // The §8.2 live read of v36 (never run at book scale) raised the
        // validator follow-up — no-level, the cadence exemption, the lexicon
        // narrowing, the margin caps, the contract's threshold sentence and the
        // capital-efficiency wording — changing the prompts and what 6g keeps,
        // so it moves to v37.
        // The §3 interpretation slice renames the target rationale with its
        // meaning, strips account economics from every packet, scopes the
        // schema per call and rewords two contract lines, changing the prompts,
        // the grammar and a persisted field, so it moves to v38 (and the
        // checkpoint trail to v10).
        // The residue slice (3.5–3.9) moved it to v39; the interpretation
        // rewrite — one message in two parts, no app narration, the caps unshown
        // — changes the interpretation prompts and their contract, so it moves
        // to v40. The checkpoint trail is unchanged. The action rewrite moved it
        // to v41 and the role/risk rewrite to v42, the trail still unchanged;
        // the research rewrite to v43. The distillation rewrite moves it to v44
        // and the trail to v11 (the audit's persisted typed shapes change).
        // The rendered-ledger slice (2026-09-18) authors a quantitative condition
        // as fields plus a name and renders its statement from the core, refusing
        // a core that already holds at authoring: the prompt, the persisted
        // condition (`label`) and the trail move to v45 / v12.
        assert_eq!(PROMPT_VERSION, "portfolio-v48");
        assert_eq!(
            crate::portfolio::store::CHECKPOINT_FORMAT_VERSION,
            "checkpoint-v15"
        );
    }

    #[test]
    fn the_narrative_render_names_an_overflowed_ratio_and_never_prints_inf() {
        // Codex I16, round 2 (`portfolio-v21`): a hype read with no persisted
        // ratio is either a non-positive reality leg or a positive one the
        // expansion outran beyond any finite multiple — the prompt must say
        // which — and a finite leg whose ×100 overflows renders as the decimal
        // ratio, never `inf%`.
        let read = |expansion: f64, reality: f64, ratio: Option<f64>| engine::NarrativeRead {
            form: engine::NarrativeForm::RevisionBased,
            expansion,
            reality,
            ratio,
            classification: engine::NarrativeClass::Hype,
            elapsed_days: 30,
            matched_rule: Some("narrative-vs-reality hype: test rule".into()),
        };
        let overflowed = narrative_prompt_section(Some(&read(1e300, f64::EPSILON, None)));
        assert!(overflowed.contains("the ratio overflowed"), "{overflowed}");
        assert!(!overflowed.contains("flat or declining"), "{overflowed}");
        let flat = narrative_prompt_section(Some(&read(0.36, -0.02, None)));
        assert!(flat.contains("reality flat or declining"), "{flat}");
        assert!(!flat.contains("overflowed"), "{flat}");
        let finite = narrative_prompt_section(Some(&read(0.36, 0.10, Some(3.6))));
        assert!(finite.contains("(3.6×)") && finite.contains("+36.0%"), "{finite}");
        let extreme = narrative_prompt_section(Some(&read(1e307, 0.10, None)));
        assert!(!extreme.contains("inf"), "{extreme}");
        assert!(extreme.contains("as a decimal ratio"), "{extreme}");
    }

    #[test]
    fn interpretation_prompt_states_the_model_domain_as_a_requirement() {
        // The domain is stated as a requirement on the output (`portfolio-v40`):
        // the scale by reference to COMPUTED SCORES, the bands as ordered
        // positive prices — never a description of what the decode rejects,
        // clamps or annotates (the gate itself is unchanged, ruled 2026-08-29).
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            user.contains("as integers on the scale defined in COMPUTED SCORES"),
            "{user}"
        );
        assert!(
            user.contains(
                "each with base, bear and bull as positive prices in USD, bear ≤ base ≤ bull"
            ),
            "{user}"
        );
        for narration in ["rejected", "never clamped", "annotated", "a zero or negative leg"] {
            assert!(!user.contains(narration), "`{narration}` leaked: {user}");
        }
    }

    #[test]
    fn decode_interpretation_rejects_an_off_domain_model_arm_under_its_own_class() {
        // Codex I6 (ruled 2026-08-29): the schema grammar cannot express range
        // keywords, so the decode enforces the declared domain — every
        // offending field named — under `ModelArmDomain`, the class the bounded
        // retry-once re-issues on, distinct from a parse failure's `SchemaParse`.
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let input = InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        };
        let stub = StubAnalyst.interpret(&input).unwrap();
        // The stub's own arm is in-domain, so the offline fixture cannot drift
        // off the gate silently.
        let clean = serde_json::to_string(&stub).unwrap();
        assert!(decode_interpretation("interpret TEST", &clean, false).is_ok());

        let mut off = serde_json::to_value(&stub).unwrap();
        off["model_sub_scores"]["quality"] = serde_json::json!(10000.0);
        off["model_sub_scores"]["risk"] = serde_json::json!(-1.0);
        off["model_price_targets"]["twelve_month"]["bear"] = serde_json::json!(0.0);
        off["model_price_targets"]["one_month"]["bull"] = serde_json::json!(-5.0);
        let err = decode_interpretation("interpret TEST", &off.to_string(), false).unwrap_err();
        assert_eq!(
            crate::local_model::retry_class(&err),
            Some(crate::local_model::RetryClass::ModelArmDomain)
        );
        let detail = format!("{err:#}");
        for field in [
            "model_sub_scores.quality = 10000.0",
            "model_sub_scores.risk = -1.0",
            "model_price_targets.twelve_month.bear = 0.0",
            "model_price_targets.one_month.bull = -5.0",
        ] {
            assert!(detail.contains(field), "{field} missing from: {detail}");
        }
        // The chain reads stage → class → the named violations, each layer once.
        assert!(
            detail.starts_with(
                "interpret TEST: model arm value off its declared domain: model arm off its \
                 declared domain: "
            ),
            "{detail}"
        );

        // An inverted band is in-domain — I5's authored-and-annotated posture
        // holds; ordering is the model's own.
        let mut inverted = serde_json::to_value(&stub).unwrap();
        inverted["model_price_targets"]["twelve_month"]["bear"] = serde_json::json!(500.0);
        inverted["model_price_targets"]["twelve_month"]["bull"] = serde_json::json!(50.0);
        assert!(decode_interpretation("interpret TEST", &inverted.to_string(), false).is_ok());

        // Malformed content keeps its own class.
        let err = decode_interpretation("interpret TEST", "not json", false).unwrap_err();
        assert_eq!(
            crate::local_model::retry_class(&err),
            Some(crate::local_model::RetryClass::SchemaParse)
        );
    }

    #[test]
    fn action_prompt_names_a_chosen_prior_action_and_the_firmness_clause() {
        // A model-chosen prior renders as PRIOR ACTION and the task's firmness
        // clause refers to it (ruled 2026-09-17, F4). The retired whole-book-era
        // history label went with the pre-v9 legacy (fresh-start ruling
        // 2026-08-17).
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let prior_action = crate::portfolio::carried_action(&prior).unwrap();
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
        let user = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced {
                graded,
                engine: &engine_output,
                pre_profit: None,
                ledger: v.thesis_ledger.as_ref().unwrap(),
            },
            engine_set: &engine_set,
            changes: None,
            profile: &d.profile,
        });
        assert!(
            user.contains(&format!("\nPRIOR ACTION\n{}, chosen in the prior analysis.\n", prior_action.as_kebab())),
            "{user}"
        );
        assert!(
            user.contains(" Move from PRIOR ACTION only where the inputs have materially changed since the prior analysis.\n"),
            "{user}"
        );
        for absent in ["continuity baseline", "RETIRED whole-book contract", "Keep the action firm", "Prior model-chosen action"] {
            assert!(!user.contains(absent), "`{absent}`: {user}");
        }
    }

    #[test]
    fn action_call_outside_engine_set_records_an_audit_annotation() {
        // A rogue stub choosing outside the engine set: the choice persists as
        // authored; the departure is app-stamped on the audit (annotate, never
        // bar — the two-arm contract).
        struct RogueActionStub;
        impl HoldingAnalyst for RogueActionStub {
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
    fn retrospective_renders_both_prior_reads_and_the_realized_since() {
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
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            user.contains(
                "\nPRIOR ANALYSIS (prior read 2026-07-29T12:00:00Z)\n- prior computed read: grade "
            ),
            "{user}"
        );
        assert!(user.contains("\n- your prior read: letter "), "{user}");
        // The continuity message carries no app concept either (`portfolio-v40`):
        // the prior read, the changes list and the prior ledger are data.
        let hits = crate::portfolio::fixed_evidence::banned_hits(&user);
        assert!(hits.is_empty(), "continuity message carries {hits:?}\n{user}");
        // The production delta labels are data too (Codex, round 1): none carries a
        // banned word, and the capital-efficiency row stays out of the projection.
        let mut moved = engine_output.clone();
        moved.grade = crate::portfolio::Grade::A;
        moved.sub_scores.quality += 5.0;
        let prior_state = match &d.prior_verdict.as_ref().unwrap().disposition {
            VerdictDisposition::Priced(g) => g.dead_money,
            other => panic!("{other:?}"),
        };
        moved.hurdle.state = if prior_state == crate::portfolio::HurdleState::Fails {
            crate::portfolio::HurdleState::Clears
        } else {
            crate::portfolio::HurdleState::Fails
        };
        let delta = priced_input_delta(&d, &moved, PositionChange::Increased, None, None, None, true, Some(0.5));
        assert!(delta.iter().any(|e| e.label.starts_with(CAPITAL_EFFICIENCY_DELTA_PREFIX)), "{delta:?}");
        for e in &delta {
            let hits = crate::portfolio::fixed_evidence::banned_hits(&e.label);
            assert!(hits.is_empty(), "delta label carries {hits:?}: {}", e.label);
            assert!(!e.label.contains("Market Signal") && !e.label.contains("-v"), "{}", e.label);
        }
        let section = input_delta_prompt_section(&delta);
        assert!(!section.contains("capital-efficiency"), "{section}");
        assert!(section.contains("computed grade: "), "{section}");
        for narration in ["RETROSPECTIVE", "ENGINE arm", "MODEL arm", "(yours)"] {
            assert!(!user.contains(narration), "`{narration}` leaked: {user}");
        }
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
            user.contains("distance to the prior computed 12-mo base"),
            "{user}"
        );
        assert!(
            user.contains("distance to your prior 12-mo base"),
            "{user}"
        );
        assert!(user.contains("any vintage"), "{user}");
        assert!(user.contains("1-month window scored: total return +4.2%"), "{user}");
        // The self-assessment is a Part 2 item drawing on the section by name.
        assert!(
            user.contains(
                "8. self_assessment — your prior read against the computed read and what \
                 happened since, from PRIOR ANALYSIS"
            ),
            "{user}"
        );
        assert!(!user.contains("Write self_assessment against this"), "{user}");

        // A carry rule can overwrite the persisted action without preserving
        // the model's original rung. The retrospective keeps the authored model
        // read but names that action provenance instead of calling the rule's
        // hold "yours".
        let prior = d.prior_verdict.as_mut().unwrap();
        prior.action_source = ActionSource::RuleDemoted;
        let VerdictDisposition::Priced(graded) = &mut prior.disposition else {
            panic!("expected a priced prior");
        };
        graded.action = Action::Hold;
        let demoted = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            demoted.contains("\n- your prior read (its action was later demoted by rule): letter "),
            "{demoted}"
        );
        assert!(
            demoted.contains(
                "action hold (demoted by rule after authoring; the rung you chose is not on \
                 record)"
            ),
            "{demoted}"
        );
        assert!(!demoted.contains("- your prior read: letter"), "{demoted}");

        // A debut renders no retrospective and says so in the model-arm brief.
        let debut = dossier(AssetClass::Stock, strong_financials());
        let debut_user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &debut,
            prior_ledger: debut.prior_ledger(),
            engine: &engine_output,
            distilled: "distilled findings",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(!debut_user.contains("PRIOR ANALYSIS"), "{debut_user}");
        assert!(debut_user.contains("This is the first analysis of this holding.\n"), "{debut_user}");
        assert!(
            debut_user.contains(
                "7. self_assessment — one sentence noting that this is a first analysis with no \
                 prior read to assess."
            ),
            "{debut_user}"
        );
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
            prior_ledger: d.prior_ledger(),
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
            prior_ledger: d.prior_ledger(),
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
        assert!(user.contains("(split-adjusted)") && !user.contains("bridge"), "{user}");
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
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(user.contains("\nPRIOR ANALYSIS (prior read"), "{user}");
        assert!(
            user.contains("prior-read price comparison unavailable"),
            "{user}"
        );
        assert!(!user.contains("% realized"), "{user}");
        assert!(!user.contains("distance to the prior computed"), "{user}");
        assert!(!user.contains("distance to your prior"), "{user}");
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
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            anchored.contains("percentile of their spread to the 10-year Treasury over the last 40 quarterly observations"),
            "{anchored}"
        );
        // The facts are one data clause under COMPUTED PRICE TARGETS, never a
        // provenance heading (`portfolio-v40`).
        assert!(!anchored.contains("TARGET PROVENANCE"), "{anchored}");
        assert!(!anchored.contains("spread-anchored on "), "{anchored}");
        // Prompt-posture audit (F6 trim): the provenance TYPE renders as a fact,
        // but the how-to-weigh narrative that spelled out how a `fails` interacts
        // with target signal quality was cut — the model infers it from the
        // provenance facts rendered beside it. Attempt 5 re-checks the regression
        // risk the F6 lesson guarded against.
        assert!(!anchored.contains("Weigh the targets by this provenance"), "{anchored}");
        assert!(!anchored.contains("robust exit evidence"), "{anchored}");
        assert!(!anchored.contains("discount the band's width, not its level"), "{anchored}");
        assert!(!anchored.contains("stays weak exit evidence"), "{anchored}");

        engine_output.target_meta.rate_anchored = false;
        engine_output.target_meta.current_multiple_carry = true;
        engine_output.target_meta.flat_driver = true;
        engine_output.target_meta.dispersion_floor_applied = true;
        let carried = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(carried.contains("× the current P/E multiple (no anchor history"), "{carried}");
        // The signal-quality FACT survives the trim — the carry branch still names
        // it, so the model has the fact without the weighing narrative.
        assert!(carried.contains("carry little forward signal"), "{carried}");
        assert!(carried.contains("- Notes: ") && carried.contains("the driver is held flat across scenarios"), "{carried}");
        assert!(!carried.contains("FLAT"), "{carried}");
        assert!(carried.contains("the band was widened to the volatility dispersion floor"), "{carried}");

        // Neither anchored nor carried: the raw-percentile fallback branch.
        engine_output.target_meta.current_multiple_carry = false;
        let fallback = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            fallback.contains(
                "at the 25th / 50th / 75th percentile of their own history (too few rate \
                 observations to anchor)"
            ),
            "{fallback}"
        );
        assert!(!fallback.contains("raw-percentile fallback"), "{fallback}");
    }

    #[test]
    fn continuity_notes_a_band_recalibration_only_on_version_mismatch() {
        let base = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) = analyze_holding(&StubAnalyst, &base, &rates(), "2026-08-01").unwrap();
        assert!(
            matches!(prior.disposition, VerdictDisposition::Priced(_)),
            "fixture sanity: the prior is priced"
        );
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let prompt = |d: &HoldingDossier| {
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: d,
                prior_ledger: d.prior_ledger(),
                engine: &engine_output,
                distilled: "",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            })
        };

        const NOTE: &str = "- The grade bands changed since the prior analysis, so the letter may \
                            have moved with no change in the company's inputs.\n";
        // No prior verdict: new holding, no recalibration note.
        assert!(!prompt(&d).contains(NOTE), "no prior verdict");

        // Prior verdict stamped across the v2 retune: the note fires, as data
        // under PRIOR ANALYSIS; the attribution of a parameter-driven move is
        // a Part 2 requirement (`portfolio-v40`).
        d.prior_verdict = Some(prior);
        d.prior_grade_parameter_version = Some("grade-v2".into());
        let p = prompt(&d);
        assert!(p.contains(NOTE), "{p}");
        assert!(!p.contains("recalibrated") && !p.contains("NOTE:"), "{p}");
        assert!(
            p.contains(
                "A move noted in PRIOR ANALYSIS as caused by a parameter change is attributed to \
                 that change, not to the company or to a self-correction."
            ),
            "{p}"
        );

        // Prior verdict stamped with the current bands: no note.
        d.prior_grade_parameter_version = Some(engine::GRADE_PARAMETER_VERSION.to_string());
        assert!(!prompt(&d).contains(NOTE), "same-version prior");

        // A prior that was never priced had no letter to move: no note, whatever
        // its stamp says — and a prior with no stamp asserts no cause.
        d.prior_verdict = Some(HoldingVerdict {
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
        });
        d.prior_grade_parameter_version = None;
        assert!(!prompt(&d).contains(NOTE), "not-rated prior");
    }

    /// A stamp boundary reaches the model as what it changed for THIS holding,
    /// read from the stamp history on the PRIOR record's branch and only over a
    /// priced prior. A stock across v2.1 → v2.3 gets neither NOTE nor delta row
    /// (neither fund-only change touched it); a priced fund across v2.2 gets the
    /// exchange-basis NOTE and row, and that letter-bearing correction dominates
    /// the older momentum-only re-homing when the prior is older; an unrecognized stamp, a
    /// missing stamp, and a never-priced prior get nothing; the branch is the
    /// prior's persisted asset
    /// class, so a fund record without the derived label still reads as a fund;
    /// and a symbol reclassified between runs reads its prior's branch, not the
    /// current dossier's.
    #[test]
    fn the_stamp_boundary_names_what_changed_and_skips_an_unchanged_holding() {
        let engine_output = match engine::analyze(&strong_financials(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let prompt = |d: &HoldingDossier| {
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: d,
                prior_ledger: d.prior_ledger(),
                engine: &engine_output,
                distilled: "",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            })
        };
        let delta = |d: &HoldingDossier| {
            priced_input_delta(
                d,
                &engine_output,
                PositionChange::Unchanged,
                None,
                None,
                None,
                false,
                Some(1.0),
            )
        };
        let boundary_rows = |entries: &[crate::portfolio::DeltaEntry]| -> Vec<String> {
            entries
                .iter()
                .filter(|e| {
                    e.label.contains("grade bands changed")
                        || e.label.contains("momentum moved to the short price window")
                        || e.label.contains("requires both exchange legs")
                })
                .map(|e| e.label.clone())
                .collect()
        };
        let silent = |d: &HoldingDossier, case: &str| {
            let p = prompt(d);
            assert!(
                !p.contains("grade bands changed")
                    && !p.contains("momentum read moved")
                    && !p.contains("sector-P/E source now requires"),
                "{case}: {p}"
            );
            let rows = delta(d);
            assert!(boundary_rows(&rows).is_empty(), "{case}: {rows:?}");
        };
        let recalibrated = |d: &HoldingDossier, case: &str| {
            let p = prompt(d);
            assert!(
                p.contains(
                    "- The grade bands changed since the prior analysis, so the letter may have \
                     moved with no change in the company's inputs.\n"
                ),
                "{case}: {p}"
            );
            assert!(
                !p.contains("momentum read moved") && !p.contains("recalibrated"),
                "{case}: {p}"
            );
            let rows = boundary_rows(&delta(d));
            assert_eq!(rows.len(), 1, "{case}: {rows:?}");
            assert!(
                rows[0].starts_with("grade bands changed since the prior analysis"),
                "{case}: {rows:?}"
            );
        };
        let exchange_basis = |d: &HoldingDossier, case: &str| {
            let p = prompt(d);
            assert!(
                p.contains(
                    "- The fund sector-P/E source now requires both exchange legs since the prior \
                     analysis, so the valuation score and the letter may have moved on the same \
                     served rows.\n"
                ),
                "{case}: {p}"
            );
            assert!(
                !p.contains("momentum read moved") && !p.contains("grade bands changed"),
                "{case}: {p}"
            );
            let rows = boundary_rows(&delta(d));
            assert_eq!(rows.len(), 1, "{case}: {rows:?}");
            assert!(
                rows[0].starts_with("fund sector-P/E source now requires both exchange legs since the prior analysis"),
                "{case}: {rows:?}"
            );
            assert!(rows[0].contains("letter can move"), "{case}: {rows:?}");
        };

        // A priced stock prior.
        let base = dossier(AssetClass::Stock, strong_financials());
        let (stock_prior, _) =
            analyze_holding(&StubAnalyst, &base, &rates(), "2026-08-01").unwrap();
        assert!(matches!(
            stock_prior.disposition,
            VerdictDisposition::Priced(_)
        ));
        let mut stock = dossier(AssetClass::Stock, strong_financials());
        stock.prior_verdict = Some(stock_prior.clone());
        stock.prior_grade_parameter_version = Some("grade-v2.1".into());
        silent(&stock, "stock across v2.1");
        stock.prior_grade_parameter_version = Some("grade-v2.2".into());
        silent(&stock, "stock across v2.2");
        stock.prior_grade_parameter_version = Some("grade-v2".into());
        recalibrated(&stock, "stock across v2 (signed P/E)");
        stock.prior_grade_parameter_version = None;
        silent(&stock, "stock with no stamp (no audit row)");
        stock.prior_grade_parameter_version = Some("grade-v9.9".into());
        silent(&stock, "stock from an unrecognized stamp");

        // A priced fund prior.
        let (fund_prior, _) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(us_equity_fund()),
            &rates(),
            "2026-08-01",
        )
        .unwrap();
        assert!(matches!(
            fund_prior.disposition,
            VerdictDisposition::Priced(_)
        ));
        let mut fund = fund_dossier(us_equity_fund());
        fund.prior_verdict = Some(fund_prior.clone());
        fund.prior_grade_parameter_version = Some("grade-v2.2".into());
        exchange_basis(&fund, "fund across v2.2");
        fund.prior_grade_parameter_version = Some("grade-v2.1".into());
        exchange_basis(&fund, "fund across v2.1");
        let rows = boundary_rows(&delta(&fund));
        // The row names the change, never the stamps (`portfolio-v40`).
        assert!(rows[0].starts_with("fund sector-P/E source now requires both exchange legs") && !rows[0].contains("grade-v"), "{rows:?}");
        fund.prior_grade_parameter_version = Some("grade-v2".into());
        exchange_basis(&fund, "fund across v2");
        fund.prior_grade_parameter_version = None;
        silent(&fund, "fund with no stamp (no audit row)");
        fund.prior_grade_parameter_version = Some("grade-v9.9".into());
        silent(&fund, "fund from an unrecognized stamp");

        // The branch is the prior's persisted asset class — the routing key — so a
        // fund record without the derived `fund_class_label` (no label derived)
        // still reads the fund branch.
        let mut unlabeled = fund_prior.clone();
        if let VerdictDisposition::Priced(g) = &mut unlabeled.disposition {
            g.fund_class_label = None;
        }
        fund.prior_verdict = Some(unlabeled);
        fund.prior_grade_parameter_version = Some("grade-v2.2".into());
        exchange_basis(&fund, "fund prior without the derived label");

        // The branch is the PRIOR record's, not the current dossier's: a fund
        // prior now scored as a stock still crosses the exchange-basis
        // correction, and a stock prior now scored as a fund crosses nothing.
        let mut now_stock = dossier(AssetClass::Stock, strong_financials());
        now_stock.prior_verdict = Some(fund_prior);
        now_stock.prior_grade_parameter_version = Some("grade-v2.2".into());
        exchange_basis(&now_stock, "fund prior on a stock dossier");
        let mut now_fund = fund_dossier(us_equity_fund());
        now_fund.prior_verdict = Some(stock_prior);
        now_fund.prior_grade_parameter_version = Some("grade-v2.2".into());
        silent(&now_fund, "stock prior on a fund dossier");

        // A fund prior that was never priced had no letter or target to move.
        fund.prior_verdict = Some(HoldingVerdict {
            symbol: fund.position.symbol.clone(),
            asset_class: AssetClass::Etf,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated {
                reason: "fixture".into(),
            },
            thesis_ledger: None,
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        });
        fund.prior_grade_parameter_version = Some("grade-v2.2".into());
        silent(&fund, "never-priced fund prior");
        fund.prior_grade_parameter_version = None;
        silent(&fund, "never-priced fund prior with no stamp");
    }

    /// Codex I11: the scenario-target stamp carries the same attribution, read
    /// from the prior audit's `target_meta.parameter_version`. The v6
    /// complete-exchange rule moves both fund horizons but no stock horizon, so
    /// a priced v5 fund gets exactly one row and NOTE while a v5 stock stays
    /// silent. The current stamp, no target record, pre-anchor v4, an
    /// unrecognized stamp, and a never-priced prior stay silent.
    #[test]
    fn the_target_stamp_boundary_is_silent_on_every_reachable_stamp_and_renders_the_horizons() {
        let engine_output = match engine::analyze(&strong_financials(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let prompt = |d: &HoldingDossier| {
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: d,
                prior_ledger: d.prior_ledger(),
                engine: &engine_output,
                distilled: "",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            })
        };
        let delta = |d: &HoldingDossier| {
            priced_input_delta(
                d,
                &engine_output,
                PositionChange::Unchanged,
                None,
                None,
                None,
                false,
                Some(1.0),
            )
        };
        let silent = |d: &HoldingDossier, case: &str| {
            let p = prompt(d);
            assert!(
                !p.contains("target parameter version changed")
                    && !p.contains("scenario-target parameters changed"),
                "{case}: {p}"
            );
            let rows = delta(d);
            assert!(
                rows
                    .iter()
                    .all(|e| !e.label.contains("scenario-target parameters changed")),
                "{case}: {rows:?}"
            );
        };
        let moved = |d: &HoldingDossier, case: &str| {
            let p = prompt(d);
            assert!(
                p.contains(
                    "- The scenario-target parameters changed since the prior analysis, so the \
                     one-month and twelve-month targets may have moved with no change in the \
                     company's inputs.\n"
                ),
                "{case}: {p}"
            );
            assert!(!p.contains("NOTE:"), "{case}: {p}");
            let rows = delta(d);
            let target_rows: Vec<_> = rows
                .iter()
                .filter(|entry| entry.label.contains("scenario-target parameters changed"))
                .collect();
            assert_eq!(target_rows.len(), 1, "{case}: {rows:?}");
            assert!(
                target_rows[0]
                    .label
                    .contains("one-month and twelve-month targets can move"),
                "{case}: {target_rows:?}"
            );
        };

        let base = dossier(AssetClass::Stock, strong_financials());
        let (stock_prior, _) =
            analyze_holding(&StubAnalyst, &base, &rates(), "2026-08-01").unwrap();
        assert!(matches!(
            stock_prior.disposition,
            VerdictDisposition::Priced(_)
        ));
        let mut stock = dossier(AssetClass::Stock, strong_financials());
        stock.prior_verdict = Some(stock_prior);
        stock.prior_target_parameter_version =
            Some(engine::SCENARIO_TARGET_PARAMETER_VERSION.to_string());
        silent(&stock, "stock on the current stamp");
        stock.prior_target_parameter_version = Some("targets-v5".into());
        silent(&stock, "stock from v5 (v6 touched funds only)");
        stock.prior_target_parameter_version = None;
        silent(&stock, "stock with no target record");
        stock.prior_target_parameter_version = Some("targets-v4".into());
        silent(&stock, "stock from targets-v4 (unrecognized by ruling)");
        stock.prior_target_parameter_version = Some("targets-v9.9".into());
        silent(&stock, "stock from an unrecognized stamp");

        let (fund_prior, _) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(us_equity_fund()),
            &rates(),
            "2026-08-01",
        )
        .unwrap();
        assert!(matches!(
            fund_prior.disposition,
            VerdictDisposition::Priced(_)
        ));
        let mut fund = fund_dossier(us_equity_fund());
        fund.prior_verdict = Some(fund_prior);
        fund.prior_target_parameter_version = Some("targets-v5".into());
        moved(&fund, "fund prior across the complete-exchange boundary");
        for stamp in [Some(engine::SCENARIO_TARGET_PARAMETER_VERSION.to_string()), None, Some("targets-v4".into())] {
            fund.prior_target_parameter_version = stamp;
            silent(&fund, "fund prior");
        }

        // A never-priced prior had no target to move, whatever it carries.
        stock.prior_verdict = Some(HoldingVerdict {
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
        });
        stock.prior_target_parameter_version = Some("targets-v4".into());
        silent(&stock, "never-priced prior");

        // The renders, on explicit horizons — the label is the engine's one
        // vocabulary for both.
        let row = target_boundary_row(engine::TargetHorizons::ONE_MONTH);
        assert_eq!(
            row,
            "scenario-target parameters changed since the prior analysis — the one-month \
             target can move with no input change"
        );
        let note = target_boundary_note(engine::TargetHorizons::BOTH);
        assert_eq!(
            note,
            "- The scenario-target parameters changed since the prior analysis, so the one-month \
             and twelve-month targets may have moved with no change in the company's inputs.\n"
        );
        // The note is data (`portfolio-v40`); the attribution of such a move is a
        // Part 2 requirement, never an instruction riding the note.
        for narration in [
            "NOTE:",
            "Attribute such a target move",
            "target parameter version changed",
            "what_changed",
        ] {
            assert!(!note.contains(narration), "`{narration}` leaked: {note}");
        }
    }

    #[test]
    fn house_view_renders_as_market_analysis_on_both_interpretation_messages() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.house_view.latest_sections = Some("Thesis: risk-off.".into());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        // The priced message names the latest report for what it is — a
        // market-level analysis, never by product name — as data with no scope
        // narration (`portfolio-v40`); the outlook item draws on it by that name.
        assert!(
            user.contains("\nMARKET ANALYSIS\nA market-level analysis.\nThesis: risk-off.\n"),
            "{user}"
        );
        assert!(user.contains("drawing on MARKET ANALYSIS for the market setup"), "{user}");
        for narration in ["MARKET SIGNAL", "HOUSE VIEW", "never by itself a reason to exit", "scope:"] {
            assert!(!user.contains(narration), "`{narration}` leaked: {user}");
        }
        // Absent, the section is absent (the item's reference stays).
        let bare = dossier(AssetClass::Stock, strong_financials());
        let bare_user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &bare,
            prior_ledger: bare.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(!bare_user.contains("\nMARKET ANALYSIS\n"), "{bare_user}");

        // The role/risk message renders the same section through the same
        // renderer (`portfolio-v42`): no product name, no scope clause, and the
        // thesis line of its ledger item draws on it by name.
        let readout = RoleRiskReadout {
            class_label: "equity fund below the US-exposure guard".into(),
            structural_kind: None,
            exposure_tilt: vec![],
            expense_ratio: None,
            observable_risk: None,
            is_cef: false,
            nav_premium: None,
            evidence_gaps: vec![],
        };
        let role = role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            readout: &readout,
            ledger_eval: None,
            distilled: "No research findings.",
        });
        assert!(
            role.contains("\nMARKET ANALYSIS\nA market-level analysis.\nThesis: risk-off.\n"),
            "{role}"
        );
        assert!(role.contains("drawing on MARKET ANALYSIS for the market setup"), "{role}");
        for narration in ["MARKET SIGNAL", "HOUSE VIEW", "never by itself a reason to exit", "scope:"] {
            assert!(!role.contains(narration), "`{narration}` leaked: {role}");
        }
    }

    #[test]
    fn role_risk_prompt_names_the_exact_structural_fund_kind() {
        let d = fund_dossier(us_equity_fund());
        let prompt_for = |structural_kind| {
            let readout = RoleRiskReadout {
                class_label: "equity fund below the US-exposure guard".into(),
                structural_kind,
                ..Default::default()
            };
            role_risk_user_prompt(&RoleRiskInput {
                input_delta: &[],
                dossier: &d,
                prior_ledger: d.prior_ledger(),
                readout: &readout,
                ledger_eval: None,
                distilled: "No research findings.",
            })
        };

        // CLASS carries the label, the fund's reported asset class and the
        // structure line where one applies (`portfolio-v42`, ruled 2026-09-17).
        let overlay = prompt_for(Some(FundStructuralKind::OptionOverlay));
        assert!(
            overlay.contains(
                "\nCLASS\nequity fund below the US-exposure guard. Reported asset class: Equity.\n\
                 Structure: option overlay; the options reshape the return path.\n"
            ),
            "{overlay}"
        );
        assert!(!overlay.contains("leveraged"), "{overlay}");

        let daily_reset = prompt_for(Some(FundStructuralKind::LeveragedInverse));
        assert!(
            daily_reset.contains("\nStructure: leveraged / inverse, resetting daily.\n"),
            "{daily_reset}"
        );
        assert!(!daily_reset.contains("option overlay"), "{daily_reset}");

        let plain = prompt_for(None);
        assert!(!plain.contains("Structure:"), "{plain}");
        for narration in ["STRUCTURAL FLAG", "CLASSIFICATION:", "path dependency"] {
            assert!(!overlay.contains(narration), "`{narration}` leaked: {overlay}");
        }
    }

    #[test]
    fn the_price_vs_nav_line_renders_only_on_the_closed_end_form() {
        // The CEF read (ruled 2026-08-21): prompt evidence + card only, rendered
        // where the vehicle makes it meaningful — a present premium on a CEF
        // renders, an absent one stays a named gap, a non-CEF never renders even
        // with a computed premium (an open-end ETF's transient spread).
        let d = fund_dossier(us_equity_fund());
        let readout = |is_cef: bool, nav_premium: Option<f64>| RoleRiskReadout {
            class_label: "closed-end fund".into(),
            structural_kind: None,
            exposure_tilt: vec![],
            expense_ratio: None,
            observable_risk: None,
            is_cef,
            nav_premium,
            evidence_gaps: vec!["price-vs-NAV unavailable: no NAV".into()],
        };
        let prompt = |r: &RoleRiskReadout| {
            role_risk_user_prompt(&RoleRiskInput {
                input_delta: &[],
                dossier: &d,
                prior_ledger: d.prior_ledger(),
                readout: r,
                ledger_eval: None,
                distilled: "No research findings.",
            })
        };
        let discount = prompt(&readout(true, Some(-0.072)));
        assert!(discount.contains("PRICE VS NAV: -7.2% (discount)"), "{discount}");
        // A unit gloss only, on every packet (`portfolio-v42`).
        assert!(
            discount.contains(
                "\nPRICE VS NAV: -7.2% (discount): the closed-end fund's market price against its \
                 net asset value.\n"
            ),
            "{discount}"
        );
        for narration in ["the closed-end read", "transient ETF spread", "structural discount"] {
            assert!(!discount.contains(narration), "`{narration}` leaked: {discount}");
        }
        let premium = prompt(&readout(true, Some(0.031)));
        assert!(premium.contains("PRICE VS NAV: +3.1% (premium)"), "{premium}");
        // Boundary: a value that renders as 0.0% reads "at par" — never a
        // signed-zero "+0.0% premium" or "-0.0% discount" (Codex round 3).
        let par = prompt(&readout(true, Some(0.0)));
        assert!(par.contains("PRICE VS NAV: 0.0% (at par)"), "{par}");
        let tiny = prompt(&readout(true, Some(-0.0004)));
        assert!(tiny.contains("PRICE VS NAV: 0.0% (at par)"), "{tiny}");
        // The exact negative half rounds away from zero (f64::round) — the Vue
        // helper mirrors this deliberately, since bare Math.round would take
        // -0.05% to "at par" instead (Codex round 4).
        let half = prompt(&readout(true, Some(-0.0005)));
        assert!(half.contains("PRICE VS NAV: -0.1% (discount)"), "{half}");
        let gap = prompt(&readout(true, None));
        assert!(!gap.contains("PRICE VS NAV:"), "{gap}");
        assert!(gap.contains("price-vs-NAV unavailable"), "the named gap: {gap}");
        let open_end = prompt(&readout(false, Some(0.002)));
        assert!(!open_end.contains("PRICE VS NAV:"), "{open_end}");

        // The action prompt's role-risk arm holds the same gate.
        let mut rr = crate::portfolio::RoleRiskVerdict {
            class_label: "closed-end fund".into(),
            role_summary: "income sleeve".into(),
            exposure_tilt: vec![],
            expense_drag: None,
            observable_risk: None,
            structural_flag: false,
            is_cef: true,
            nav_premium: Some(-0.072),
            evidence_gaps: vec![],
            action: crate::portfolio::Action::Hold,
            action_rationale: String::new(),
            what_changed: "new holding".into(),
        };
        let ledger = test_ledger();
        let action = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::RoleRisk { verdict: &rr, ledger: &ledger },
            engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
            changes: None,
            profile: &d.profile,
        });
        assert!(action.contains("\nPRICE VS NAV: -7.2% (discount)"), "{action}");
        // The role/risk message on the same two-part frame (`portfolio-v41`):
        // its own sections, the reduced set as one data line, no capital
        // efficiency or targets, and its own weighing clause.
        assert!(action.contains("\nCLASS (computed)\nclosed-end fund\n"), "{action}");
        assert!(action.contains("\nROLE (analyst)\nincome sleeve\n"), "{action}");
        assert!(action.contains("\nTHESIS (analyst)\nA standing thesis.\n"), "{action}");
        assert!(action.contains("- base (50%): base case conditions.\n"), "{action}");
        assert!(
            action.contains("\nSUPPORTED ACTIONS (computed)\nThe rungs the computed read supports on its own: sell-all, trim, hold.\n"),
            "{action}"
        );
        assert!(
            action.contains(
                "Decide it from CLASS, ROLE, RISK PROFILE and PRICE VS NAV first, refined by THESIS, SCENARIOS, \
                 SUPPORTED ACTIONS and INVESTOR PROFILE. An aggressive risk tolerance admits \
                 add-aggressively where the other inputs support it. An add-side rung needs support \
                 from the vehicle's own attributes, stated in the rationale.\n"
            ),
            "{action}"
        );
        for absent in [
            "CAPITAL EFFICIENCY", "PRICE TARGETS", "even the bull case", "This branch has no price forecast",
            "ENGINE SET", "ENGINE ADMISSION FACTS", "EXPOSURE TILT", "EVIDENCE GAPS", "both arms",
        ] {
            assert!(!action.contains(absent), "`{absent}`: {action}");
        }
        rr.nav_premium = None;
        let action_gap = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::RoleRisk { verdict: &rr, ledger: &ledger },
            engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
            changes: None,
            profile: &d.profile,
        });
        assert!(!action_gap.contains("PRICE VS NAV:"), "{action_gap}");
    }

    #[test]
    fn the_target_delta_row_requires_a_certified_prior_basis() {
        // A prior pass that withheld its basis (unresolvable bridge) persisted
        // its verdict targets fresh; the next pass's bridge must not convert
        // them — the row is excluded on an uncertified prior basis, never a
        // fabricated target-change entry in the 6g evidence vocabulary.
        let base = dossier(AssetClass::Stock, strong_financials());
        let (prior_verdict, _) =
            analyze_holding(&StubAnalyst, &base, &rates(), "2026-08-01").unwrap();
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.prior_verdict = Some(prior_verdict);
        d.prior_spot = None;
        let engine_output = match engine::analyze(&strong_financials(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let entries = priced_input_delta(
            &d,
            &engine_output,
            PositionChange::Unchanged,
            None,
            None,
            None,
            false,
            Some(0.25),
        );
        assert!(
            !entries.iter().any(|e| e.label.contains("twelve-month base target")),
            "uncertified prior basis must exclude the row: {entries:?}"
        );
        // With a certified prior basis the same comparison renders (the 0.25
        // conversion moves the old side).
        d.prior_spot = Some(780.0);
        let entries = priced_input_delta(
            &d,
            &engine_output,
            PositionChange::Unchanged,
            None,
            None,
            None,
            false,
            Some(0.25),
        );
        assert!(
            entries.iter().any(|e| e.label.contains("twelve-month base target")),
            "certified prior basis renders the bridged row: {entries:?}"
        );
    }

    #[test]
    fn input_delta_renders_every_exact_move_as_a_visible_move() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (mut prior, _) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-01").unwrap();
        let VerdictDisposition::Priced(prior_graded) = &mut prior.disposition else {
            panic!("expected a priced prior");
        };
        prior_graded.sub_scores.quality = 61.7;
        d.prior_verdict = Some(prior);
        d.prior_spot = Some(195.001);
        d.financials.current_price = Some(195.002);

        let mut engine_output = match engine::analyze(&strong_financials(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let mut prior_metrics = engine_output.metrics.clone();
        prior_metrics.net_margin = Some(0.10001);
        engine_output.metrics.net_margin = Some(0.10002);
        engine_output.sub_scores.quality = 62.3;
        d.prior_metrics = Some(prior_metrics);

        let entries = priced_input_delta(
            &d,
            &engine_output,
            PositionChange::Unchanged,
            None,
            None,
            None,
            false,
            Some(1.0),
        );
        let labels = entries
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<Vec<_>>();
        assert!(labels.contains(&"spot: 195.001 -> 195.002"), "{labels:?}");
        assert!(
            labels.contains(&"metric net margin: 0.10001 -> 0.10002"),
            "{labels:?}"
        );
        assert!(
            labels.contains(&"computed sub-score quality: 61.7 -> 62.3"),
            "{labels:?}"
        );
        for label in labels {
            if let Some((old, new)) = label.split_once(" -> ") {
                assert_ne!(old.rsplit_once(": ").map_or(old, |(_, value)| value), new, "{label}");
            }
        }
    }

    #[test]
    fn the_nav_premium_delta_row_is_gated_to_the_closed_end_form() {
        // An open-end ETF's transient premium flicker must not seed a 6g input
        // delta row every run; on the closed-end form the move IS the read.
        let mut d = fund_dossier(us_equity_fund());
        d.prior_verdict = Some(HoldingVerdict {
            symbol: d.position.symbol.clone(),
            asset_class: AssetClass::Etf,
            position_change: PositionChange::Unchanged,
            disposition: VerdictDisposition::NotRated { reason: "fixture".into() },
            thesis_ledger: None,
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        });
        d.prior_metrics = Some(engine::ComputedMetrics {
            nav_premium: Some(0.001),
            ..Default::default()
        });
        let engine_output = match engine::analyze(&strong_financials(), &rates()) {
            EngineVerdict::Analyzed(mut o) => {
                o.metrics = engine::ComputedMetrics {
                    nav_premium: Some(0.004),
                    ..Default::default()
                };
                o
            }
            other => panic!("{other:?}"),
        };
        let entries =
            priced_input_delta(&d, &engine_output, PositionChange::Unchanged, None, None, None, false, Some(1.0));
        assert!(
            !entries.iter().any(|e| e.label.contains("NAV premium")),
            "open-end: {entries:?}"
        );
        // The same move on a closed-end fund is a delta row.
        if let Some(f) = d.fund.as_mut() {
            f.fund.profile_is_fund = Some(true);
            f.fund.profile_description = Some("a closed-end equity fund".into());
        }
        let entries =
            priced_input_delta(&d, &engine_output, PositionChange::Unchanged, None, None, None, false, Some(1.0));
        assert!(
            entries.iter().any(|e| e.label.contains("NAV premium")),
            "closed-end: {entries:?}"
        );
    }

    #[test]
    fn the_priced_fund_prompt_renders_the_closed_end_arm() {
        // The priced branch's FUND CONTEXT arm — structurally unreachable for a
        // real CEF today (pricing needs weightings the surface never serves one),
        // but shipped code: open-end never renders, a closed-end premium renders
        // the shared line, an absent NAV renders the explicit gap line.
        let mut d = fund_dossier(us_equity_fund());
        let mut engine_output = match engine::analyze(&strong_financials(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let prompt = |d: &HoldingDossier, e: &EngineOutput| {
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: d,
                prior_ledger: d.prior_ledger(),
                engine: e,
                distilled: "",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            })
        };
        engine_output.metrics.nav_premium = Some(0.002);
        let open_end = prompt(&d, &engine_output);
        assert!(!open_end.contains("PRICE VS NAV"), "{open_end}");
        if let Some(f) = d.fund.as_mut() {
            f.fund.profile_is_fund = Some(true);
            f.fund.profile_description = Some("a closed-end equity fund".into());
        }
        engine_output.metrics.nav_premium = Some(-0.072);
        let cef = prompt(&d, &engine_output);
        assert!(cef.contains("PRICE VS NAV: -7.2% (discount)"), "{cef}");
        engine_output.metrics.nav_premium = None;
        let gap = prompt(&d, &engine_output);
        assert!(gap.contains("PRICE VS NAV: (gap)"), "{gap}");
    }

    #[test]
    fn iv_skew_renders_signed_and_keys_the_sign_on_the_rendered_value() {
        // `opt()` printed the skew bare — no `+`, no convention — while
        // put-minus-call lived only in a doc comment, so a model assuming the
        // inverse read hedging demand as call speculation (large-scale review
        // 2026-08-24, P1 minor).
        assert_eq!(fmt_iv_skew(Some(0.03)), "+0.030");
        assert_eq!(fmt_iv_skew(Some(-0.02)), "-0.020");
        assert_eq!(fmt_iv_skew(Some(0.0)), "0.000");
        // A skew that rounds away carries no sign — `+0.000` would assert a
        // put premium the rendered number no longer shows.
        assert_eq!(fmt_iv_skew(Some(0.0004)), "0.000");
        assert_eq!(fmt_iv_skew(Some(-0.0004)), "0.000");
        assert_eq!(fmt_iv_skew(Some(0.0005)), "+0.001");
        assert_eq!(fmt_iv_skew(Some(-0.0005)), "-0.001");
        assert_eq!(fmt_iv_skew(None), "(gap)");
    }

    #[test]
    fn iv_skew_convention_rides_the_options_line_on_value_negative_and_gap() {
        // The convention is the line's label, not the value's: it renders
        // beside a positive, a negative, and a `(gap)` alike, so the line keeps
        // one shape across holdings.
        let engine_output = match engine::analyze(&strong_financials(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let prompt = |d: &HoldingDossier| {
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: d,
                prior_ledger: d.prior_ledger(),
                engine: &engine_output,
                distilled: "",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            })
        };
        const CONVENTION: &str = "(mean put IV minus mean call IV, in IV's decimal unit; positive \
                                  means puts are richer).\n";

        // The fixture dossier carries `iv_skew: Some(0.03)`.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let positive = prompt(&d);
        assert!(
            positive.contains(&format!(
                "\nOPTIONS ACTIVITY\nput/call volume 1.200, put/call open interest 1.100, implied \
                 volatility 0.300, IV skew +0.030 {CONVENTION}"
            )),
            "{positive}"
        );
        // The unit and polarity stay; the trading gloss and the grade-input
        // disclaimer are gone (`portfolio-v40`).
        for narration in ["hedging demand", "call speculation", "NOT a grade input", "chain-wide"] {
            assert!(!positive.contains(narration), "`{narration}` leaked: {positive}");
        }

        d.options_signal.iv_skew = Some(-0.02);
        let negative = prompt(&d);
        assert!(
            negative.contains(&format!("implied volatility 0.300, IV skew -0.020 {CONVENTION}")),
            "{negative}"
        );

        d.options_signal.iv_skew = None;
        let gap = prompt(&d);
        assert!(
            gap.contains(&format!("implied volatility 0.300, IV skew (gap) {CONVENTION}")),
            "{gap}"
        );
    }

    #[test]
    fn expense_ratio_renders_the_fraction_and_its_percent_reading() {
        // A 0.03% fund flattened to `0.000` under `opt()`'s three places — read
        // as free against the legend's own arithmetic — and the legend's example
        // 0.0075 was unrepresentable (large-scale review 2026-08-24, P1 minor).
        assert_eq!(fmt_expense_ratio(Some(0.0003)), "0.0003 (0.03%/yr)");
        assert_eq!(fmt_expense_ratio(Some(0.0075)), "0.0075 (0.75%/yr)");
        assert_eq!(fmt_expense_ratio(Some(0.0125)), "0.0125 (1.25%/yr)");
        // The seam's actual arithmetic (`etf/info` serves percent; the adapter
        // divides by 100) is not exactly 0.0003 in f64 — fixed precision, never
        // shortest-round-trip display.
        assert_eq!(fmt_expense_ratio(Some(0.03 / 100.0)), "0.0003 (0.03%/yr)");
        // A fee-waived fund is genuinely zero.
        assert_eq!(fmt_expense_ratio(Some(0.0)), "0.0000 (0.00%/yr)");
        // A nonzero ratio below half a basis point extends its precision rather
        // than printing as free.
        assert_eq!(fmt_expense_ratio(Some(0.00004)), "0.00004 (0.004%/yr)");
        assert_eq!(fmt_expense_ratio(None), "(gap)");
    }

    #[test]
    fn crossing_pairs_render_at_one_comparison_safe_precision() {
        // Codex I12: one formatter for the pair — four places extending where
        // a nonzero value would round to zero (the expense-ratio rule), and
        // further (the group's Codex rounds 1–3) until the rendered pair, read
        // back as numbers, orders as the values do, so a real crossing never
        // renders as equality and, on that fixed-decimal branch, the two
        // values print at one shared precision; past ten places the
        // round-trip fallback prints each at its own shortest exact form.
        // Order is the guarantee, not distance: `0.0000451`
        // against `0.0000449` renders `0.00005` against `0.00004`, the gap
        // magnified, and `0.0000649` against `0.0000451` renders `0.00006`
        // against `0.00005`, the gap shrunk — fixed-precision rounding can
        // do either to a distance, and the render promises neither.
        let pair = |o: f64, t: f64| {
            let (a, b) = fmt_crossing_pair(o, t);
            format!("{a} vs {b}")
        };
        assert_eq!(pair(0.0075, 0.0075), "0.0075 vs 0.0075");
        assert_eq!(pair(-0.45, -0.4), "-0.4500 vs -0.4000");
        assert_eq!(pair(1234.5678, 1234.5677), "1234.5678 vs 1234.5677");
        assert_eq!(pair(0.03 / 100.0, 0.0), "0.0003 vs 0.0000");
        // Each value alone would take four places and print `0.0001`; the
        // pair extends until the crossing shows.
        assert_eq!(pair(0.00006, 0.00005), "0.00006 vs 0.00005");
        // The observed value's own floor governs both, so the threshold never
        // reads as `0.0001` beside `0.00004`.
        assert_eq!(pair(0.00004, 0.00005), "0.00004 vs 0.00005");
        assert_eq!(pair(0.00001, 0.00002), "0.00001 vs 0.00002");
        assert_eq!(pair(-0.00004, 0.00003), "-0.00004 vs 0.00003");
        // A negative zero is zero.
        assert_eq!(pair(-0.0, 0.0), "0.0000 vs 0.0000");
        // Past ten places the pair falls back to the shortest round-trip
        // render, so a zero-margin crossing that close still reads as one
        // (Codex round 2) — distinct values never render alike.
        assert_eq!(pair(1e-12, 0.0), "0.000000000001 vs 0");
        assert_eq!(pair(0.1000000000001, 0.1), "0.1000000000001 vs 0.1");
        let (o, t) = fmt_crossing_pair(0.1 + f64::EPSILON, 0.1);
        assert_ne!(o, t);
        assert!(o.parse::<f64>().unwrap() > t.parse::<f64>().unwrap());
        // The stop test reads the pair back as numbers (Codex round 3): a
        // tiny negative against zero renders `-0.0000000000` beside
        // `0.0000000000` at ten places — distinct strings that read as equal
        // — so it falls through to the round-trip render like any other
        // crossing too close to show.
        assert_eq!(pair(-1e-12, 0.0), "-0.000000000001 vs 0");
        assert_eq!(pair(0.0, -1e-12), "0 vs -0.000000000001");
        assert_eq!(pair(-1e-12, 1e-12), "-0.000000000001 vs 0.000000000001");
        // Every rendered pair orders as its values do.
        for (o, t) in [
            (0.0075, 0.0075),
            (-0.45, -0.4),
            (0.00006, 0.00005),
            (0.00004, 0.00005),
            (-1e-12, 0.0),
            (0.1 + f64::EPSILON, 0.1),
            (-0.0, 0.0),
        ] {
            let (ro, rt) = fmt_crossing_pair(o, t);
            assert_eq!(
                ro.parse::<f64>().unwrap().partial_cmp(&rt.parse::<f64>().unwrap()),
                o.partial_cmp(&t),
                "{o} vs {t} rendered {ro} vs {rt}"
            );
        }
    }

    #[test]
    fn comparison_safe_pairs_respect_each_delta_surface_floor() {
        assert_eq!(
            comparison_safe_pair(61.7, 62.3, 0),
            ("61.7".to_string(), "62.3".to_string())
        );
        assert_eq!(
            comparison_safe_pair(195.001, 195.002, 2),
            ("195.001".to_string(), "195.002".to_string())
        );
        assert_eq!(
            comparison_safe_pair(0.10001, 0.10002, 4),
            ("0.10001".to_string(), "0.10002".to_string())
        );
        assert_eq!(delta_value(Some(1e-12), 4), "0.000000000001");
        assert_eq!(delta_value(Some(-0.0), 4), "0.0000");
        assert_eq!(delta_value(None, 4), "(absent)");
    }

    #[test]
    fn both_crossing_renders_state_observed_and_threshold_identically() {
        // Codex I12: the input-delta entry and the 6f ENGINE CONDITION
        // CROSSINGS section print the same crossing through one pair
        // formatter — a sub-basis-point expense ratio no longer flattens to
        // `0.0000` at either site, the threshold no longer prints at four
        // places in one and shortest-round-trip in the other, and a crossing
        // whose two values each round to `0.0001` shows as the crossing it is.
        let prior = prior_with_conditions();
        let crossing = |observed: f64, threshold: f64| ConditionCrossing {
            condition_id: "keep-1".into(),
            statement: "Expense ratio rises".into(),
            role: ConditionRole::Falsifier,
            outcome: CrossingOutcome::Confirmed,
            observed_value: observed,
            threshold,
            observation_id: "2026-07-16".into(),
            confirmed_at: Some("2026-07-16".into()),
        };
        let eval = LedgerEvaluation {
            crossings: vec![crossing(0.00006, 0.00005), crossing(-0.45, -0.4)],
            unevaluable: vec![],
            unevaluable_series: vec![],
            updated_states: vec![],
        };
        let section = prior_ledger_data_section(Some(&prior), Some(&eval), &[]);
        let d = fund_dossier(us_equity_fund());
        let mut entries = Vec::new();
        append_shared_delta(&mut entries, &d, PositionChange::Unchanged, Some(&eval), Some(1.0));
        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
        for rendered in [
            "observed 0.00006 vs threshold 0.00005",
            "observed -0.4500 vs threshold -0.4000",
        ] {
            assert!(section.contains(rendered), "{rendered}: {section}");
            assert!(
                labels.iter().any(|l| l.contains(rendered)),
                "{rendered}: {labels:?}"
            );
        }
        assert!(!section.contains("0.0000 vs"), "{section}");
        assert!(!section.contains("0.0001 vs threshold 0.0001"), "{section}");
        assert!(!section.contains("threshold -0.4 "), "{section}");
    }

    #[test]
    fn the_priced_fund_prompt_renders_guards_us_share_and_twelve_month_methodology() {
        // Codex I8: the FUND CONTEXT line reads `fund::us_share` — every US
        // alias summed and capped, the ≥ 70% guard's own read — where it had
        // taken the first label containing "united states", so a `US` row
        // passed the guard at 97% while the prompt said `(gap)`.
        let prompt_for = |weights: Vec<(String, f64)>| {
            let mut fund = us_equity_fund();
            fund.country_weights = weights;
            let d = fund_dossier(fund);
            let engine_output = match engine::analyze(&strong_financials(), &rates()) {
                EngineVerdict::Analyzed(o) => o,
                other => panic!("{other:?}"),
            };
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: &d,
                prior_ledger: d.prior_ledger(),
                engine: &engine_output,
                distilled: "",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            })
        };
        let us = prompt_for(vec![("US".into(), 0.97), ("Canada".into(), 0.03)]);
        assert!(us.contains("US share of holdings: 97%."), "{us}");
        let summed = prompt_for(vec![
            ("United States".into(), 0.5),
            ("USA".into(), 0.2),
            ("U.S.".into(), 0.1),
            ("Canada".into(), 0.2),
        ]);
        assert!(summed.contains("US share of holdings: 80%."), "{summed}");
        let capped = prompt_for(vec![
            ("United States of America".into(), 0.8),
            ("us".into(), 0.5),
        ]);
        assert!(capped.contains("US share of holdings: 100%."), "{capped}");
        let gap = prompt_for(vec![]);
        assert!(gap.contains("US share of holdings: (gap)."), "{gap}");
        // Slice 2 keeps the twelve-month method; the one-month computed band
        // supplies its prices without the proration method.
        assert!(us.contains("\nCOMPUTED PRICE TARGETS (USD)\n- twelve-month: bear "), "{us}");
        let twelve = us.find("- twelve-month: bear ").unwrap_or_else(|| panic!("{us}"));
        let line = us[twelve..].lines().next().unwrap();
        // The method is a plain clause from the typed target inputs, never the
        // engine's methodology string with its mechanics and stamp (`portfolio-v40`).
        assert!(line.contains(". Method: ") && line.contains(" × ") && line.contains("percentile"), "{line}");
        let one_month = us.find("- one-month: bear ").unwrap_or_else(|| panic!("{us}"));
        let line = us[one_month..].lines().next().unwrap();
        assert!(line.contains(" / base ") && line.contains(" / bull "), "{line}");
        assert!(!line.contains("Method:") && !line.contains("prorated"), "{line}");
        assert!(!us.contains(engine::SCENARIO_TARGET_PARAMETER_VERSION), "{us}");
        for narration in ["ENGINE SCENARIO TARGETS", "baseline arm", "ENGINE ONE-MONTH TARGETS", "methodology:", "v1 mechanics", "degenerate", "clamp", "inverse map", "P75", "DGS10", "PR_base"] {
            assert!(!us.contains(narration), "`{narration}` leaked: {us}");
        }
    }

    #[test]
    fn expense_ratio_renders_both_readings_on_every_fund_prompt() {
        // All three fund prompts route through the one formatter: the role-risk
        // prompt, the priced branch's FUND CONTEXT arm, and the action prompt's
        // role-risk arm (`expense_drag`). The fixture fund carries 0.0003.
        let d = fund_dossier(us_equity_fund());
        let readout = RoleRiskReadout {
            class_label: "bond fund".into(),
            expense_ratio: Some(0.0003),
            ..Default::default()
        };
        let role = role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            readout: &readout,
            ledger_eval: None,
            distilled: "No research findings.",
        });
        // The role/risk message renders the ratio once, as its FINANCIAL
        // METRICS line (`portfolio-v42`).
        assert!(
            role.contains(
                "- fund expense ratio [expense-ratio]: 0.0003 (0.03%/yr) — a fraction of assets \
                 per year, never a percent (0.0075 means 0.75%); confirmed by one filing\n"
            ),
            "{role}"
        );
        assert_eq!(role.matches("0.0003 (0.03%/yr)").count(), 1, "renders once: {role}");
        assert!(!role.contains("EXPENSE RATIO ("), "{role}");

        let engine_output = match engine::analyze(&strong_financials(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            interp.contains(
                "\nFUND\nExpense ratio: 0.0003 (0.03%/yr) (a fraction of assets per year; 0.0075 \
                 means 0.75%). US share of holdings: 99%.\n"
            ),
            "{interp}"
        );
        assert!(!interp.contains("FUND CONTEXT"), "{interp}");

        let rr = crate::portfolio::RoleRiskVerdict {
            class_label: "bond fund".into(),
            role_summary: "income sleeve".into(),
            exposure_tilt: vec![],
            expense_drag: Some(0.0003),
            observable_risk: None,
            structural_flag: false,
            is_cef: false,
            nav_premium: None,
            evidence_gaps: vec![],
            action: crate::portfolio::Action::Hold,
            action_rationale: String::new(),
            what_changed: "new holding".into(),
        };
        let ledger = test_ledger();
        let action = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::RoleRisk { verdict: &rr, ledger: &ledger },
            engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
            changes: None,
            profile: &d.profile,
        });
        assert!(
            action.contains(
                "\nRISK PROFILE (computed)\nExpense drag: 0.0003 (0.03%/yr) of assets per year. \
                 Observable risk:"
            ),
            "{action}"
        );
        for (name, p) in [
            ("role", &role),
            ("interpretation", &interp),
            ("action", &action),
        ] {
            assert!(
                !p.contains("): 0.000\n") && !p.contains("): 0.000.") && !p.contains(": 0.000 ("),
                "{name} prompt flattened the ratio: {p}"
            );
        }
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
            prior_ledger: d.prior_ledger(),
            readout: &readout,
            ledger_eval: None,
            distilled: "No research findings.",
        });
        const SECTION: &str = "\nUNDERLYING POSITIONING (CFTC weekly, as of 2026-08-11)\nGold — \
                               speculator net +200000 contracts (50.0% of OI long), w/w +4000\n";
        assert!(role.contains(SECTION), "{role}");
        // Dated data only — no layer or score-input narration (`portfolio-v40`).
        for narration in ["snapshot", "never a score input", "layer (c)"] {
            assert!(!role.contains(narration), "`{narration}` leaked: {role}");
        }
        // The priced fund message renders the same line under FUND.
        let engine_output = match engine::analyze(&strong_financials(), &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(interp.contains(SECTION), "{interp}");
        let fund_section = interp.find("\nFUND\n").expect("fund section");
        let positioning = interp.find(SECTION).expect("positioning line");
        let metrics = interp.find("\nFINANCIAL METRICS\n").expect("metrics section");
        assert!(fund_section < positioning && positioning < metrics, "{interp}");

        let bare = fund_dossier(us_equity_fund());
        let role = role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: &bare,
            prior_ledger: bare.prior_ledger(),
            readout: &readout,
            ledger_eval: None,
            distilled: "No research findings.",
        });
        assert!(!role.contains("UNDERLYING POSITIONING"), "{role}");
    }

    #[test]
    fn stage_requests_carry_the_per_stage_mode_options_and_residency() {
        // The options-wiring contract (`docs/local-model-operations.md`): distill is
        // explicitly non-thinking (F3 — an omitted flag rides the thinking-on
        // default) and grammar-constrained since the research slice retired the
        // stub-era free-prose exception; interpretation thinks; the research
        // gathering turn thinks with tools and no grammar, while the separate
        // synthesis call thinks with the findings grammar and no tools (fix B —
        // never both on one call); every stage pins an explicit `num_ctx` (never
        // the daemon auto-size), its mode's sampling row, and stay-resident
        // `keep_alive`.
        let d = dossier(AssetClass::Stock, strong_financials());

        let schema = serde_json::json!({"type": "object"});
        let distill = distill_request(
            "fast-model",
            NUM_CTX_DISTILL,
            NUM_PREDICT_DISTILL,
            &distill::DistillPrompt { system: "s".into(), user: "prompt".into() },
            &schema,
        );
        assert_eq!(distill.think, Some(false));
        assert_eq!(distill.keep_alive, Some(-1));
        let opts = distill.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_DISTILL);
        assert_eq!(opts["num_predict"], NUM_PREDICT_DISTILL, "output reservation");
        assert_eq!(opts["temperature"], 0.7, "non-thinking-general row");
        assert!(
            distill.format_schema.is_some(),
            "distill is grammar-constrained (the stub-era exception is retired)"
        );

        // Fix B: gathering and synthesis are separate calls — tools and the
        // findings grammar never ride one request.
        let tools = crate::portfolio::research::research_tools();
        let gather = research_turn_request(
            "reasoner-model",
            vec![ChatMessage::user("brief")],
            Some(&tools),
            None,
        );
        assert_eq!(gather.think, Some(true));
        assert_eq!(gather.keep_alive, Some(-1));
        assert!(gather.tools.is_some(), "gathering carries the web tools");
        assert!(
            gather.format_schema.is_none(),
            "gathering carries no grammar (it is not the findings call)"
        );
        let opts = gather.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET, "one num_ctx per model");
        assert_eq!(opts["temperature"], 1.0, "thinking-general row");

        let synth = research_turn_request(
            "reasoner-model",
            vec![ChatMessage::user("evidence")],
            None,
            Some(&schema),
        );
        assert_eq!(synth.think, Some(true));
        assert_eq!(synth.keep_alive, Some(-1));
        assert!(synth.tools.is_none(), "synthesis carries no tools");
        assert!(
            synth.format_schema.is_some(),
            "the findings grammar rides the separate synthesis call"
        );
        assert_eq!(
            synth.options.as_ref().unwrap()["num_ctx"],
            NUM_CTX_INTERPRET,
            "one num_ctx per model"
        );

        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interpret = interpret_request(
            "reasoner-model",
            &InterpretationInput {
                input_delta: &[],
                dossier: &d,
                prior_ledger: d.prior_ledger(),
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
            structural_kind: None,
            is_cef: false,
            nav_premium: None,
            evidence_gaps: vec![],
        };
        let role_risk = role_risk_request(
            "reasoner-model",
            &RoleRiskInput {
                input_delta: &[],
                dossier: &d,
                prior_ledger: d.prior_ledger(),
                readout: &readout,
                ledger_eval: None,
                distilled: "No research findings.",
            },
        );
        assert_eq!(role_risk.think, Some(true));
        assert_eq!(role_risk.keep_alive, Some(-1));
        let opts = role_risk.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET);
        assert_eq!(opts["num_predict"], NUM_PREDICT_THINKING, "output reservation");
        assert!(role_risk.format_schema.is_some(), "grammar-constrained");
    }

    #[test]
    fn distill_expands_only_an_exact_normal_reservation_stop() {
        let schema = serde_json::json!({"type": "object"});
        let normal = distill_request(
            "fast-tier",
            NUM_CTX_DISTILL,
            NUM_PREDICT_DISTILL,
            &distill::DistillPrompt { system: "s".into(), user: "prompt".into() },
            &schema,
        );
        let response = |eval_count| crate::local_model::ChatResponse {
            content: "partial".into(),
            thinking: None,
            prompt_eval_count: Some(1_000),
            eval_count,
            done_reason: Some("length".into()),
            tool_calls: None,
        };
        assert!(hit_normal_distill_reservation(
            &normal,
            &response(Some(u64::from(NUM_PREDICT_DISTILL)))
        ));
        assert!(
            !hit_normal_distill_reservation(&normal, &response(Some(8_000))),
            "a context-bound stop must not repeat with a larger ceiling"
        );
        assert!(
            !hit_normal_distill_reservation(&normal, &response(None)),
            "an unattributed stop must not guess at a lever"
        );

        let expanded = distill_request(
            "reasoner",
            NUM_CTX_INTERPRET,
            NUM_PREDICT_DISTILL_RETRY,
            &distill::DistillPrompt { system: "s".into(), user: "prompt".into() },
            &schema,
        );
        assert_eq!(
            crate::local_model::request_num_ctx(&expanded),
            Some(NUM_CTX_INTERPRET)
        );
        assert_eq!(
            crate::local_model::request_num_predict(&expanded),
            Some(NUM_PREDICT_DISTILL_RETRY)
        );
        assert!(
            !hit_normal_distill_reservation(
                &expanded,
                &response(Some(u64::from(NUM_PREDICT_DISTILL_RETRY)))
            ),
            "the expanded ceiling never activates a second expansion"
        );
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
            tool_calls: None,
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
            tool_calls: None,
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
            tool_calls: None,
        };
        let err = ensure_not_output_limited("construction", &req, &uncounted).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cannot be told apart"), "{msg}");
        assert!(!msg.contains("context exhaustion suspected"), "{msg}");
        assert!(
            !msg.contains("truncated at the output reservation"),
            "{msg}"
        );

        req.think = Some(true);
        req.format_schema = Some(serde_json::json!({"type":"object"}));
        let error =
            ensure_not_output_limited("interpret AAPL", &req, &context_stopped).unwrap_err();
        assert!(error.to_string().contains("phase-limited"));
        assert!(!error.to_string().contains("context exhaustion suspected"));
        assert!(
            crate::local_model::retry_class(&error).is_none(),
            "length stop is not retryable"
        );

        let complete = crate::local_model::ChatResponse {
            content: "{}".into(),
            thinking: None,
            prompt_eval_count: Some(100_000),
            eval_count: Some(9_000),
            done_reason: Some("stop".into()),
            tool_calls: None,
        };
        assert!(ensure_not_output_limited("construction", &req, &complete).is_ok());
    }

    #[test]
    fn ensure_nonempty_completion_classifies_and_tolerates_tool_calls() {
        let blank = crate::local_model::ChatResponse {
            content: "  ".into(),
            thinking: None,
            prompt_eval_count: None,
            eval_count: None,
            done_reason: Some("stop".into()),
            tool_calls: None,
        };
        let err = ensure_nonempty_completion("interpret TEST", &blank).unwrap_err();
        assert_eq!(
            crate::local_model::retry_class(&err),
            Some(crate::local_model::RetryClass::EmptyCompletion)
        );
        assert!(err.to_string().contains("empty completion body"), "{err}");

        // A research turn's tool request legitimately carries no content.
        let tool_turn = crate::local_model::ChatResponse {
            content: String::new(),
            tool_calls: Some(serde_json::json!([
                {"function": {"name": "web_search", "arguments": {"query": "q"}}}
            ])),
            ..blank.clone()
        };
        assert!(ensure_nonempty_completion("research TEST", &tool_turn).is_ok());

        let normal = crate::local_model::ChatResponse {
            content: "{}".into(),
            ..blank
        };
        assert!(ensure_nonempty_completion("interpret TEST", &normal).is_ok());
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
    fn distill_calls_are_sized_at_issue_and_route_up_before_they_refuse() {
        // The issue guard (the 2026-08-24 review's reduce-prompt minor, ruled
        // 2026-08-28): the rendered prompt is measured against its model's
        // input budget before any request exists, closing the daemon's silent
        // front-truncation off from 6d as far as a chars-per-token estimate
        // can close it.
        let fast_budget = distill::input_budget_chars(NUM_CTX_DISTILL);
        let wide_budget = distill::input_budget_chars(NUM_CTX_INTERPRET);
        assert!(fast_budget < wide_budget);
        let route = |chars: usize, fast: &'static str, reasoner: &'static str| {
            distill_route("distill X reduce", chars, fast, reasoner)
        };
        // Within the fast tier's budget: the fast model at the distill context.
        assert_eq!(
            route(fast_budget, "qwen3.5:35b", "qwen3.5:122b").unwrap(),
            ("qwen3.5:35b", NUM_CTX_DISTILL)
        );
        // One char over it: the resident reasoner at the interpretation context
        // — a model choice, never a num_ctx change.
        assert_eq!(
            route(fast_budget + 1, "qwen3.5:35b", "qwen3.5:122b").unwrap(),
            ("qwen3.5:122b", NUM_CTX_INTERPRET)
        );
        assert_eq!(
            route(wide_budget, "qwen3.5:35b", "qwen3.5:122b").unwrap(),
            ("qwen3.5:122b", NUM_CTX_INTERPRET)
        );
        // Over the widest budget: refused before issue, naming the stage and
        // the sizes.
        let err = route(wide_budget + 1, "qwen3.5:35b", "qwen3.5:122b").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("distill X reduce"), "{msg}");
        assert!(msg.contains(&format!("{} chars", wide_budget + 1)), "{msg}");
        assert!(msg.contains(&format!("{wide_budget} chars")), "{msg}");
        assert!(msg.contains("refused before issue"), "{msg}");
        // Unclassified: the whitelist gate never re-issues a deterministic
        // outcome.
        assert_eq!(crate::local_model::retry_class(&err), None);
        // The default roster collapses the two rungs into one budget: within
        // it the reasoner at the interpretation context, over it the same
        // refusal.
        assert_eq!(
            route(wide_budget, "qwen3.5:122b", "qwen3.5:122b").unwrap(),
            ("qwen3.5:122b", NUM_CTX_INTERPRET)
        );
        assert!(route(wide_budget + 1, "qwen3.5:122b", "qwen3.5:122b").is_err());
    }

    #[test]
    fn an_over_budget_distillation_prompt_never_reaches_the_daemon() {
        // The refusal happens before a request exists: a listener standing in
        // for the daemon accepts nothing, and the failure names the refusal —
        // the run fails legibly instead of the daemon front-truncating.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let analyst = LocalAnalyst::new(
            LocalModelClient::new(endpoint).unwrap(),
            "qwen3.5:122b".into(),
            "qwen3.5:35b".into(),
        );
        // One topic whose single pass alone outgrows even the reasoner's
        // budget: the routing sub-distills it along the pass seam, and that
        // pass call is the first prompt the adapter would issue.
        let over = distill::input_budget_chars(NUM_CTX_INTERPRET) + 1;
        let research = research::HoldingResearch {
            topics: vec![research::TopicResearch {
                topic_key: "competitive-position".into(),
                title: "Competitive position".into(),
                seeded_vintage: None,
                passes: vec![research::PassFindings {
                    findings: "x".repeat(over),
                    claims: Vec::new(),
                    followup: None,
                }],
                skipped: None,
            }],
            ..Default::default()
        };
        let inputs = DistillInputs {
            symbol: "TEST",
            company_name: None,
            research: &research,
            priors: &[],
            ledger_conditions: &[],
            ledger_key_drivers: &[],
            holding_brief: "HOLDING\nTEST (name unavailable).\nPrice: (gap)\nDate: 2026-09-16.\n",
            consolidation_only: false,
            overlay_eligible: false,
            backfill_required: false,
            input_budget_chars: analyst.distill_input_budget(),
            issue_budget_chars: analyst.distill_issue_budget(),
            now: chrono::Utc::now(),
        };
        let err = analyst.distill_research(&inputs).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("refused before issue"), "{msg}");
        assert!(
            msg.contains("distill TEST competitive-position pass 0"),
            "{msg}"
        );
        // Nothing connected: the refusal preceded any request.
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "the daemon stand-in must have accepted nothing"
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
                driver_id: "kd-margins".into(),
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
                    label: None,
                    statement: "Trailing return collapses to -40%".into(),
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
                    label: None,
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
                    label: None,
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

    /// Codex round 1 on group 4 (I11 + I13): a new or superseding quantitative
    /// condition is stamped at authoring from the surface the prompt described,
    /// per series — so the first full-pass evaluation after a debut has a stamp
    /// to disagree with, where the run-1 ledger's instants used to carry none
    /// until run 2's evaluation adopted silently across the very flip I13 gates.
    #[test]
    fn a_new_condition_is_stamped_at_authoring_per_series_so_a_flip_before_its_first_evaluation_is_caught(
    ) {
        use crate::portfolio::{ContinuityStamps, EquitySource, FalsifierDraft, StatementBasis};
        let falsifier = |statement: &str, series: &str, threshold: f64| FalsifierDraft {
            statement: statement.into(),
            quant: Some(QuantCoreDraft {
                series: series.into(),
                comparator: "above".into(),
                threshold,
                margin: 0.0,
            }),
            technology_class: false,
            tripped: false,
        };
        let draft_with = |de_threshold: f64| {
            let mut draft = stub_ledger_draft(None, "AAPL", false);
            draft.falsifiers = vec![
                falsifier(
                    &format!("debt/equity above {de_threshold}"),
                    "debt-to-equity",
                    de_threshold,
                ),
                falsifier("net margin above 90%", "net-margin", 0.9),
                falsifier("price above $500", "price", 500.0),
            ];
            draft.triggers = vec![];
            draft
        };
        let validate = |draft: &LedgerDraft, prior: Option<&ThesisLedger>, stamps| {
            validate_ledger_rewrite_with_research(
                draft,
                prior,
                None,
                LedgerBranch::Priced,
                false,
                None,
                None,
                None,
                &std::collections::HashSet::new(),
                true,
                stamps,
            )
        };
        let state_of = |ledger: &ThesisLedger, kebab: &str| {
            ledger
                .conditions
                .iter()
                .find(|c| c.quant.as_ref().is_some_and(|q| q.series.as_kebab() == kebab))
                .unwrap_or_else(|| panic!("{kebab}"))
                .eval_state
                .clone()
                .expect("a quantitative condition starts machine state")
        };

        // The debut surface: TTM flows, FMP's quarterly equity.
        let authored = ContinuityStamps {
            statement_basis: Some(StatementBasis::Ttm),
            equity_source: Some(EquitySource::FmpQuarterly),
        };
        let (debut, _) = validate(&draft_with(3.0), None, authored);
        let de = state_of(&debut, "debt-to-equity");
        assert_eq!(de.authored_statement_basis, Some(StatementBasis::Ttm));
        assert_eq!(de.authored_equity_source, Some(EquitySource::FmpQuarterly));
        assert_eq!(de.breach_streak, 0);
        let nm = state_of(&debut, "net-margin");
        assert_eq!(nm.authored_statement_basis, Some(StatementBasis::Ttm));
        assert_eq!(
            nm.authored_equity_source, None,
            "a flow series never carries the equity stamp"
        );
        let px = state_of(&debut, "price");
        assert_eq!(
            (px.authored_statement_basis, px.authored_equity_source),
            (None, None),
            "a price series carries neither"
        );

        // The research-less wrapper stamps nothing — a surface with no lines.
        let (bare, _) =
            validate_ledger_rewrite(&draft_with(3.0), None, None, LedgerBranch::Priced, false, None, None);
        let bare_de = state_of(&bare, "debt-to-equity");
        assert_eq!(
            (bare_de.authored_statement_basis, bare_de.authored_equity_source),
            (None, None)
        );

        // A superseding core (edited threshold) starts a fresh streak stamped with
        // THIS run's surface; a carried-verbatim core keeps its carried state.
        let later = ContinuityStamps {
            statement_basis: Some(StatementBasis::Annual),
            equity_source: Some(EquitySource::SecAnnual),
        };
        let (next, audit) = validate(&draft_with(4.0), Some(&debut), later);
        assert_eq!(audit.superseded.len(), 1, "{:?}", audit.superseded);
        let de2 = state_of(&next, "debt-to-equity");
        assert_eq!(de2.authored_statement_basis, Some(StatementBasis::Annual));
        assert_eq!(de2.authored_equity_source, Some(EquitySource::SecAnnual));
        let nm2 = state_of(&next, "net-margin");
        assert_eq!(
            nm2.authored_statement_basis,
            Some(StatementBasis::Ttm),
            "carried verbatim: the carried stamp stands"
        );

        // The teeth: the debut's D/E condition, evaluated for the first time on a
        // surface whose equity leg fell to SEC's annual print, is typed
        // unevaluable — never silently adopted and compared across the step.
        let mut fin = strong_financials();
        fin.statement_basis = Some(StatementBasis::Ttm);
        fin.equity_source = Some(EquitySource::SecAnnual);
        let mut metrics = engine::compute_metrics(&fin);
        metrics.debt_to_equity = Some(3.5);
        let eval = engine::evaluate_ledger_conditions(&debut, &metrics, &fin, "2026-08-20");
        assert!(
            eval.crossings.iter().all(|c| !c.statement.contains("debt/equity")),
            "{:?}",
            eval.crossings
        );
        assert!(
            eval.unevaluable.iter().any(|u| u.contains("debt/equity above 3")
                && u.contains(
                    "equity source changed (FMP's latest quarterly balance sheet → SEC's"
                )),
            "{:?}",
            eval.unevaluable
        );
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
        draft.falsifiers[0].statement = "Trailing return collapses to -50%".into();
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
            label: None,
            statement: "Trailing return collapses harder to -60%".into(),
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
                statement: "Collapses even harder to -65%".into(),
                quant: Some(core_draft(-0.65)), // keep-2's core, edited
                technology_class: false,
                tripped: false,
            },
            FalsifierDraft {
                statement: "Trailing return collapses to -40%".into(),
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
            label: None,
            statement: "Trailing return collapses harder to -60%".into(),
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
            statement: format!("Edited at {:.0}%", threshold * 100.0),
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
                label: None,
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
            statement: format!("Edited at {:.0}%", threshold * 100.0),
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
                label: None,
                statement: "Trim above the priced-in ceiling of $150".into(),
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
                label: None,
                statement: "Exit fully above the priced-in ceiling of $150".into(),
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
            statement: format!("{family} above the priced-in ceiling of $150"),
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
            label: None,
            statement: "Exit fully above the priced-in ceiling of $150".into(),
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
            statement: "Trim above a higher ceiling of $180".into(),
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
            statement: "Widened noise guard at -40%".into(),
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
                statement: "Trailing return collapses to -40%".into(),
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
    fn a_qualitative_tripped_claim_is_honored_by_a_source_backed_research_finding() {
        // The 6g research leg (`docs/portfolio-workflow.md` §Step 6g): a
        // qualitative falsifier claimed tripped is honored when a fresh
        // distilled claim references its carried condition id — and only then.
        let prior = prior_with_conditions();
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        draft.falsifiers[1].tripped = true; // qualitative (qual-1)

        let supported: std::collections::HashSet<String> =
            ["qual-1".to_string()].into_iter().collect();
        let (ledger, audit) = validate_ledger_rewrite_with_research(
            &draft,
            Some(&prior),
            None,
            LedgerBranch::Priced,
            false,
            None,
            None,
            None,
            &supported,
            true,
            crate::portfolio::ContinuityStamps::NONE,
        );
        let q = ledger
            .conditions
            .iter()
            .find(|c| c.condition_id == "qual-1")
            .unwrap();
        assert!(q.tripped, "{:?}", audit.rejected_claims);
        assert!(audit.rejected_claims.is_empty());

        // A finding referencing some OTHER condition never certifies this one.
        let unrelated: std::collections::HashSet<String> =
            ["other-id".to_string()].into_iter().collect();
        let (ledger, audit) = validate_ledger_rewrite_with_research(
            &draft,
            Some(&prior),
            None,
            LedgerBranch::Priced,
            false,
            None,
            None,
            None,
            &unrelated,
            true,
            crate::portfolio::ContinuityStamps::NONE,
        );
        assert!(ledger.conditions.iter().all(|c| !c.tripped));
        assert!(audit
            .rejected_claims
            .iter()
            .any(|r| r.contains("no source-backed research finding")));
    }

    #[test]
    fn the_assumption_recompute_is_shadow_only() {
        // Ruled 2026-08-24: the engine's hypothetical refinement records as a
        // would-have line; nothing splices into the baseline (structurally —
        // `engine_output` is immutable past 6b).
        let refined = engine::RefinedTargets {
            price_targets: crate::portfolio::PriceTargets {
                one_month: None,
                twelve_month: Some(crate::portfolio::PriceTarget {
                    base: 120.0,
                    bear: 80.0,
                    bull: 150.0,
                    methodology: "test".into(),
                }),
            },
            target_meta: engine::TargetMeta::default(),
            hurdle: engine::HurdleRead::default(),
            implied_expectations: None,
            quick_basis: None,
            matched_rule: "supplement: filled the absent forward-revenue driver".into(),
        };
        let line = shadow_assumption_resolution(Some(100.0), &refined);
        assert!(line.starts_with("shadow (write-back parked"), "{line}");
        assert!(line.contains("100.00 -> 120.00"), "{line}");
        assert!(line.contains("supplement: filled"), "{line}");
        let no_standing = shadow_assumption_resolution(None, &refined);
        assert!(no_standing.contains("n/a -> 120.00"), "{no_standing}");
    }

    #[test]
    fn key_driver_ids_carry_by_name_and_mint_fresh_otherwise() {
        // Ruled 2026-08-24: app-assigned stable driver identity — a rewrite
        // whose driver name carries keeps the prior id; a new name mints one.
        let prior = prior_with_conditions();
        let mut draft = stub_ledger_draft(None, "WID", false);
        draft.key_drivers = vec![
            KeyDriverDraft {
                name: "margins".into(),
                series: None,
            },
            KeyDriverDraft {
                name: "unit demand".into(),
                series: None,
            },
        ];
        let (ledger, _) = validate_ledger_rewrite(
            &draft,
            Some(&prior),
            None,
            LedgerBranch::Priced,
            false,
            None,
            None,
        );
        let carried = ledger
            .key_drivers
            .iter()
            .find(|d| d.name == "margins")
            .unwrap();
        assert_eq!(carried.driver_id, "kd-margins", "same-name driver keeps its id");
        let fresh = ledger
            .key_drivers
            .iter()
            .find(|d| d.name == "unit demand")
            .unwrap();
        assert!(!fresh.driver_id.is_empty());
        assert_ne!(fresh.driver_id, "kd-margins");
    }

    #[test]
    fn the_research_fraud_claim_is_advisory_and_never_a_hard_trigger() {
        // Ruled 2026-08-24: the research-fed claim renders as clearly-labeled
        // attention evidence — the hard-forensic state comes from the
        // item-classified filings alone (no merge path exists any more, so a
        // validated claim structurally cannot trip the hard rule).
        use crate::portfolio::distill::ForensicEventClaim;
        let claim = ForensicEventClaim {
            kind: "fraud".into(),
            issuer: "ACME Motors".into(),
            event_date: "2026-08-01".into(),
            source_url: "https://www.sec.gov/litigation/acme".into(),
        };
        let block = render_fraud_record(&claim);
        assert_eq!(
            block,
            "Fraud record: a sec.gov document dated 2026-08-01 names ACME Motors in a fraud \
             matter; source https://www.sec.gov/litigation/acme. Whether it concerns this \
             holding is not established."
        );
        assert!(crate::portfolio::fixed_evidence::banned_hits(&block).is_empty(), "{block}");
        // The indicator line names the ledger's driver only where the id verified.
        use crate::portfolio::distill::{IndicatorDirection, ValidatedLeadingIndicator};
        let mut ind = ValidatedLeadingIndicator {
            metric_name: "EU BEV registrations".into(),
            value: 21_400.0,
            direction: IndicatorDirection::InflectingUp,
            as_of: "2026-08".into(),
            source_url: "https://www.acea.auto/august".into(),
            confirms_driver: "the model's own wording".into(),
            confirms_driver_id: "d-energy".into(),
            driver_verified: false,
        };
        let ledger = ThesisLedger {
            key_drivers: vec![crate::portfolio::KeyDriver {
                driver_id: "d-energy".into(),
                name: "Energy storage growth".into(),
                series: None,
            }],
            ..test_ledger()
        };
        let unverified = render_leading_indicator(&ind, Some(&ledger));
        assert_eq!(
            unverified,
            "Leading indicator: EU BEV registrations = 21400 (inflecting up, as of 2026-08); \
             source https://www.acea.auto/august."
        );
        ind.driver_verified = true;
        let verified = render_leading_indicator(&ind, Some(&ledger));
        assert!(verified.contains(", bearing on the driver \"Energy storage growth\"; source"), "{verified}");
        for line in [&unverified, &verified] {
            assert!(crate::portfolio::fixed_evidence::banned_hits(line).is_empty(), "{line}");
        }
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
            label: None,
            statement: "Trailing return collapses harder to -60%".into(),
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
    fn ledger_data_and_task_item_render_debut_prior_and_crossings() {
        // Debut: the data section says none; the shared task item carries the
        // authoring contract as requirements on the output (`portfolio-v42`).
        let none = prior_ledger_data_section(None, None, &[]);
        assert!(none.contains("\nPRIOR THESIS LEDGER\nNone: this is the first analysis.\n"), "{none}");
        let stock = LedgerSeriesContract::build(false, None, None);
        let item = ledger_task_item(5, &stock, LedgerItemBranch::PricedStock, false);
        assert!(item.starts_with("\n5. ledger — the position's initial thesis ledger:\n"), "{item}");
        assert!(item.contains("Every falsifier and trigger has a quant field."), "{item}");
        assert!(item.contains("technology_class is true only for a third party's technology event"), "{item}");
        assert!(item.contains("with family \"add\", \"trim\" or \"sell\". fired is false."), "{item}");
        assert!(item.contains("(\"below 16%\" on gross-margin is 0.16)"), "{item}");
        assert!(!item.contains("for a fund, the exposure it supplies"), "{item}");
        for narration in ["REWRITE THE THESIS LEDGER", "the app", "engine", "downgrades", "machine-evaluated", "action call"] {
            assert!(!item.contains(narration), "`{narration}` leaked: {item}");
        }
        // The fund form on both fund variants (ruled 2026-09-17): the fund
        // threshold example and the driver clause; on the role/risk branch the
        // trim / sell families and the market-analysis reference on the thesis
        // line, which the priced fund message's outlook item carries instead.
        let fund = LedgerSeriesContract::build(true, None, None);
        let rr = ledger_task_item(2, &fund, LedgerItemBranch::RoleRisk, false);
        assert!(rr.starts_with("\n2. ledger — the position's initial thesis ledger:\n"), "{rr}");
        assert!(
            rr.contains("- thesis: the standing thesis, in a few sentences, drawing on MARKET ANALYSIS for the market setup.\n"),
            "{rr}"
        );
        assert!(
            rr.contains("- key_drivers: what the thesis depends on — for a fund, the exposure it supplies, its cost and its fidelity to its mandate. Where a driver"),
            "{rr}"
        );
        assert!(
            rr.contains("- triggers: pre-committed conditions for trimming or selling, with family \"trim\" or \"sell\". fired is false.\n"),
            "{rr}"
        );
        assert!(!rr.contains("\"add\""), "{rr}");
        assert!(rr.contains("(\"above 0.75%\" on expense-ratio is 0.0075)"), "{rr}");
        assert!(rr.contains("0.0005 on an expense ratio of 0.0075"), "{rr}");
        assert!(rr.contains("Price support"), "{rr}");
        let priced_fund = ledger_task_item(5, &fund, LedgerItemBranch::PricedFund, false);
        assert!(priced_fund.contains("for a fund, the exposure it supplies"), "{priced_fund}");
        assert!(priced_fund.contains("(\"above 0.75%\" on expense-ratio is 0.0075)"), "{priced_fund}");
        assert!(priced_fund.contains("with family \"add\", \"trim\" or \"sell\"."), "{priced_fund}");
        assert!(!priced_fund.contains("drawing on MARKET ANALYSIS"), "{priced_fund}");
        // On continuity the carry rule and the tripped / fired rule.
        let cont = ledger_task_item(5, &stock, LedgerItemBranch::PricedStock, true);
        assert!(cont.contains("rewritten from PRIOR THESIS LEDGER"), "{cont}");
        assert!(cont.contains("tripped is true only where CONDITION CROSSINGS THIS RUN shows a confirmed"), "{cont}");
        assert!(cont.contains("marked research-supported evidences it"), "{cont}");
        assert!(cont.contains("fired follows the same rule as tripped."), "{cont}");
        assert!(cont.contains("a kept one unchanged even after a crossing"), "{cont}");
        assert!(!item.contains("kept one"), "{item}");

        // A prior ledger renders whole — the first prior-run content in the prompt —
        // with the engine's crossings and typed unevaluable notes beside it.
        let prior = prior_with_conditions();
        let eval = LedgerEvaluation {
            crossings: vec![ConditionCrossing {
                condition_id: "keep-1".into(),
                statement: "Trailing return collapses to -40%".into(),
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
        let s = prior_ledger_data_section(Some(&prior), Some(&eval), &[]);
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

        // The research-supported mark (2026-08-24 review F3): a fresh research
        // entry tied to a condition marks that row — by statement, the id held
        // out — and the task item names the mark as the qualitative leg;
        // without a tied entry no row is marked and the retired "none are
        // available this run" sentence is gone for good.
        assert!(!s.contains("RESEARCH-SUPPORTED THIS RUN:"), "{s}");
        assert!(!s.contains("none are available this run"), "{s}");
        let tied = vec![crate::portfolio::DeltaEntry {
            id: "research-1".into(),
            label: "research finding (t): a claim [https://x.example/a]".into(),
            related_condition_id: Some("keep-1".into()),
        }];
        let marked = prior_ledger_data_section(Some(&prior), Some(&eval), &tied);
        assert!(
            marked.contains(
                " — research-supported: a finding in CHANGES SINCE THE PRIOR ANALYSIS bears on \
                 this condition\n"
            ),
            "{marked}"
        );
        assert!(!marked.contains("RESEARCH-SUPPORTED THIS RUN:"), "{marked}");
        assert!(!marked.contains("keep-1"), "condition ids stay out of the prompt: {marked}");

        // Both interpretation messages carry the section and the item.
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        // Both messages state the ledger as a Part 2 item over the prior ledger
        // rendered as data (`portfolio-v40`; the role/risk message since
        // `portfolio-v42`).
        assert!(user.contains("\n5. ledger — the position's initial thesis ledger:\n"), "{user}");
        assert!(user.contains("\nPRIOR THESIS LEDGER\nNone: this is the first analysis.\n"), "{user}");
        assert!(!user.contains("REWRITE THE THESIS LEDGER"), "{user}");
        for system in [interpretation_system_prompt(false, false), role_risk_system_prompt(false)] {
            assert!(system.contains("ledger") && !system.contains("THESIS LEDGER"), "{system}");
        }
        let rr = role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: None,
            readout: &RoleRiskReadout::default(),
            ledger_eval: None,
            distilled: "",
        });
        assert!(rr.contains("\n2. ledger — the position's initial thesis ledger:\n"), "{rr}");
        assert!(rr.contains("\nPRIOR THESIS LEDGER\nNone: this is the first analysis.\n"), "{rr}");
        assert!(!rr.contains("REWRITE THE THESIS LEDGER"), "{rr}");
    }

    /// The 2026-08-24 large-scale review's Priority-1 minor: the vocabulary said
    /// "TTM net margin" while an annual-fallback holding's thresholds were
    /// evaluated against annual prints and no prompt said so. The labels now name
    /// no basis and the section states the holding's basis once, beside them.
    #[test]
    fn ledger_section_states_the_statement_basis() {
        use crate::portfolio::{EquitySource, StatementBasis};
        // The flow family and the instants pinned by name, so a production drift to
        // a hand-written list or a predicate change shows here, not only in the gate
        // (Codex round 1: the gate's whole family is not basis-homogeneous).
        const FLOW: &str = "Flow metrics (net margin, gross margin, revenue growth, P/E, P/S)";
        const INSTANTS: &str = "Balance-sheet metrics (debt / equity, P/B)";
        // The basis line and the metric lines, as FINANCIAL METRICS renders them
        // on both messages (`portfolio-v42`).
        let section = |basis: Option<StatementBasis>, equity: Option<EquitySource>| {
            statement_basis_line(basis, equity, false)
                + &LedgerSeriesContract::build(false, None, None).metric_lines()
        };

        let ttm = section(Some(StatementBasis::Ttm), Some(EquitySource::FmpQuarterly));
        // The metric lines carry the label, the unit and the confirmation rule,
        // and name no basis (`portfolio-v40`).
        assert!(
            ttm.contains("- net margin [net-margin]: (gap) — a fraction, never a percent (0.16 means 16%); confirmed by one filing\n"),
            "{ttm}"
        );
        assert!(
            ttm.contains("- gross margin [gross-margin]: (gap) — a fraction, never a percent (0.16 means 16%); confirmed by one filing\n"),
            "{ttm}"
        );
        assert!(
            !ttm.contains("TTM net margin") && !ttm.contains("TTM gross margin"),
            "{ttm}"
        );
        // One sentence per family: the flow basis, then which balance sheet
        // supplied the instants' equity (Codex I13, `portfolio-v23`).
        assert!(
            ttm.contains(&format!(
                "{FLOW} are on a TTM (four trailing quarters) basis. {INSTANTS} are from FMP's \
                 latest quarterly balance sheet.\n"
            )),
            "{ttm}"
        );
        for narration in ["statement basis this run:", "Author their thresholds", "supplied this run by"] {
            assert!(!ttm.contains(narration), "`{narration}` leaked: {ttm}");
        }

        let annual = section(Some(StatementBasis::Annual), Some(EquitySource::SecAnnual));
        assert!(
            annual.contains(&format!(
                "{FLOW} are on a SEC annual (latest full year — the quarterly window fell back) \
                 basis. {INSTANTS} are from SEC's latest annual stockholders' equity (the \
                 quarterly balance-sheet leg fell back).\n"
            )),
            "{annual}"
        );
        assert!(
            !annual.contains("TTM (four trailing quarters)")
                && !annual.contains("FMP's latest quarterly balance sheet"),
            "{annual}"
        );

        // No statement lines and no equity: each sentence says so rather than
        // naming a basis or a source.
        let none = section(None, None);
        assert!(
            none.contains(&format!(
                "{FLOW} have no statement basis this run — no income-statement lines were \
                 available — so they are not evaluable here. {INSTANTS} have no balance sheet \
                 this run — no equity line was available — so they are not evaluable here.\n"
            )),
            "{none}"
        );
        assert!(!none.contains(" are on a ") && !none.contains(" are from "), "{none}");

        // A balance-sheet instant standing alone (FMP's own beside thin quarters):
        // no flow basis, but the instants still read — and name their source.
        let instant_only = section(None, Some(EquitySource::FmpQuarterly));
        assert!(
            instant_only.contains(&format!("{FLOW} have no statement basis this run")),
            "{instant_only}"
        );
        assert!(
            instant_only.contains(&format!("{INSTANTS} are from FMP's latest quarterly balance sheet.\n")),
            "{instant_only}"
        );
        assert!(!instant_only.contains("have no balance sheet this run"), "{instant_only}");

        // A fund has no statement series: its line names the market metrics'
        // cadence and the expense ratio's source instead.
        let rr = statement_basis_line(None, None, true);
        assert!(
            rr.contains("The market metrics are daily; the expense ratio is the fund's published figure.\n"),
            "{rr}"
        );
        assert!(!rr.contains("statement basis") && !rr.contains(FLOW), "{rr}");

        // The interpretation message reads the dossier's stamped basis and source,
        // once, under FINANCIAL METRICS.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.financials.statement_basis = Some(StatementBasis::Annual);
        d.financials.equity_source = Some(EquitySource::SecAnnual);
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        let line = format!(
            "\nFINANCIAL METRICS\n{FLOW} are on a SEC annual (latest full year — the quarterly \
             window fell back) basis. {INSTANTS} are from SEC's latest annual stockholders' \
             equity (the quarterly balance-sheet leg fell back).\n"
        );
        assert_eq!(user.matches(&line).count(), 1, "{user}");
        assert!(!user.contains("METRICS AVAILABLE FOR QUANTITATIVE LEDGER CONDITIONS"), "{user}");
    }

    /// Finding 4 (`docs/verification/2026-08-10-big-run-attempt-1.md`): the header
    /// names the issuer. Since portfolio-v38 it carries identity and the per-share
    /// quote only — no quantity, cost basis or market value on any packet (fix
    /// list 3.2), the form the action header had held since v36.
    #[test]
    fn the_holding_header_carries_identity_and_spot_only_and_names_the_issuer() {
        let mut d = dossier(AssetClass::Stock, strong_financials());

        // A usable account description is used as-is.
        d.position.description = "Phillips 66".to_string();
        d.company_name = Some("Phillips 66 Company".to_string());
        let h = holding_header(&d);
        assert!(h.starts_with("HOLDING\nAAPL (Phillips 66).\n"), "{h}");
        assert!(h.contains("Price: $195.00 per share.\n"), "{h}");
        // The analysis date closes the header on every packet (`portfolio-v43`).
        assert!(h.ends_with("Date: 2026-07-28.\n"), "{h}");
        assert!(!h.contains("Current price"), "{h}");
        for absent in ["Quantity", "Cost basis", "Market value", " total"] {
            assert!(!h.contains(absent), "{absent} leaked: {h}");
        }
        // Account economics cannot change the header at all.
        let mut repriced = d.clone();
        repriced.position.quantity *= 3.0;
        repriced.position.cost_basis *= 3.0;
        repriced.position.market_value *= 3.0;
        assert_eq!(holding_header(&repriced), h);

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

    /// The fund half of Finding 4's fallback: a fund's profile read is
    /// structure-only (no identity mapping), so `company_name` is structurally
    /// `None` on the role-risk branch — the fetched fund data's own name is
    /// that branch's only naming source, and a blank Schwab description must
    /// reach it rather than "name unavailable".
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
            /// The placeholder-only return shape (`portfolio-v40` on the priced
            /// branch, `portfolio-v41` on the action call), rendered at the end
            /// of the user message; the role/risk prompt declares the key list
            /// inline in the contract.
            return_shape: Option<String>,
        }
        // Every per-call shape since portfolio-v38 (fix list 3.3): stock and
        // fund, continuity and debut, on both branches.
        let mut cases = vec![ContractCase {
            what: "action call",
            required: required_keys(&pf::action_decision_schema()),
            keys: pf::ACTION_KEYS.to_vec(),
            contract: pf::action_response_contract(),
            prompt: action_system_prompt(),
            return_shape: Some(pf::action_return_shape()),
        }];
        for (is_fund, debut) in [(false, false), (false, true), (true, false), (true, true)] {
            cases.push(ContractCase {
                what: if debut { "priced debut" } else { "priced continuity" },
                required: required_keys(&pf::interpretation_schema(is_fund, debut)),
                keys: pf::interpretation_keys(debut),
                contract: pf::interpretation_response_contract(debut),
                prompt: interpretation_system_prompt(is_fund, debut),
                return_shape: Some(pf::interpretation_return_shape(is_fund, debut)),
            });
            // The user message closes on the shape, verbatim.
            let task = interpretation_task_section(
                &LedgerSeriesContract::build(is_fund, None, None),
                is_fund,
                false,
                debut,
                !debut,
            );
            let expected = format!(
                "\nRETURN SHAPE (every value is a placeholder; an array holds as many items as apply)\n{}\n",
                pf::interpretation_return_shape(is_fund, debut)
            );
            assert!(task.ends_with(&expected), "fund {is_fund} debut {debut}: {task}");
        }
        for debut in [false, true] {
            cases.push(ContractCase {
                what: if debut { "role-risk debut" } else { "role-risk continuity" },
                required: required_keys(&pf::role_risk_interpretation_schema(debut)),
                keys: pf::role_risk_keys(debut),
                contract: pf::role_risk_response_contract(debut),
                prompt: role_risk_system_prompt(debut),
                return_shape: Some(pf::role_risk_return_shape(debut)),
            });
            // The role/risk message closes on its shape, verbatim (`portfolio-v42`).
            let task = role_risk_task_section(
                &LedgerSeriesContract::build(true, None, None),
                debut,
                !debut,
                true,
                true,
            );
            let expected = format!(
                "\nRETURN SHAPE (every value is a placeholder; an array holds as many items as apply)\n{}\n",
                pf::role_risk_return_shape(debut)
            );
            assert!(task.ends_with(&expected), "role/risk debut {debut}: {task}");
        }

        for c in cases {
            assert_eq!(
                non_empty(c.required, c.what),
                c.keys.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
                "{}: schema drifted from the key constant",
                c.what
            );
            match &c.return_shape {
                Some(shape) => {
                    // The contract names every key ("You will return k1, …
                    // and kN"), and the shape carries exactly the declared set.
                    assert!(
                        c.contract.starts_with("You will return ")
                            && c.contract.ends_with(", as one JSON object."),
                        "{}: {}",
                        c.what,
                        c.contract
                    );
                    for k in &c.keys {
                        assert!(
                            c.contract.contains(k),
                            "{}: contract does not name `{k}`: {}",
                            c.what,
                            c.contract
                        );
                    }
                    let parsed: serde_json::Value =
                        serde_json::from_str(shape).expect("the return shape is JSON");
                    let mut top: Vec<&str> = parsed
                        .as_object()
                        .expect("an object")
                        .keys()
                        .map(String::as_str)
                        .collect();
                    top.sort_unstable();
                    let mut declared = c.keys.clone();
                    declared.sort_unstable();
                    assert_eq!(
                        top, declared,
                        "{}: return shape drifted from the key constant",
                        c.what
                    );
                }
                None => {
                    let declared = c.keys.join(", ");
                    assert!(
                        c.contract.contains(&declared),
                        "{}: contract does not declare the exact key list `{declared}`",
                        c.what
                    );
                }
            }
            assert!(
                c.prompt.contains(&c.contract),
                "{}: prompt does not carry the contract",
                c.what
            );
            for residue in ["decoder", "dropped on decode", "spend no reasoning"] {
                assert!(
                    !c.contract.contains(residue),
                    "{}: response contract leaked `{residue}`: {}",
                    c.what,
                    c.contract
                );
            }
        }

        // The branch carries no action of its own — declaring one would invite it.
        assert!(!pf::role_risk_response_contract(false).contains("model_price_targets"));

        // The internal build vocabulary of Finding 3 stays out of every prompt.
        for p in [
            interpretation_system_prompt(false, false),
            interpretation_system_prompt(true, true),
            role_risk_system_prompt(false),
            role_risk_system_prompt(true),
            action_system_prompt(),
        ] {
            assert!(!p.contains("pre-v7"), "internal version vocabulary leaked: {p}");
        }
    }

    /// Fix list 3.3 (portfolio-v38): the request built for a holding carries the
    /// schema and contract for its vehicle kind and its debut / continuity
    /// shape — a fund's grammar lists no stock-only series, a debut's requests
    /// no continuity field — and the debut user prompt no longer instructs on
    /// `what_changed_entries`.
    #[test]
    fn the_interpretation_request_is_scoped_to_the_vehicle_and_the_debut_shape() {
        let stock = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&stock.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        fn input<'a>(d: &'a HoldingDossier, engine: &'a EngineOutput) -> InterpretationInput<'a> {
            InterpretationInput {
                input_delta: &[],
                dossier: d,
                prior_ledger: None,
                engine,
                distilled: "",
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            }
        }
        let debut_req = interpret_request("qwen", &input(&stock, &engine_output));
        let schema = debut_req.format_schema.as_ref().unwrap();
        assert!(schema["properties"].get("what_changed").is_none());
        assert!(schema["properties"].get("what_changed_entries").is_none());
        let series = schema["properties"]["ledger"]["properties"]["falsifiers"]["items"]["properties"]["quant"]["properties"]["series"]["enum"].to_string();
        assert!(series.contains("pe-ratio") && !series.contains("expense-ratio"), "{series}");
        let user = interpretation_user_prompt(&input(&stock, &engine_output));
        assert!(user.contains("This is the first analysis of this holding.\n"), "{user}");
        assert!(
            user.contains(
                "7. self_assessment — one sentence noting that this is a first analysis with no \
                 prior read to assess."
            ),
            "{user}"
        );
        for absent in ["must be []", "what_changed", "CONTINUITY:"] {
            assert!(!user.contains(absent), "`{absent}` leaked: {user}");
        }

        let mut fund = dossier(AssetClass::Etf, strong_financials());
        fund.prior_verdict = stock.prior_verdict.clone();
        let (v, _) = analyze_holding(&StubAnalyst, &fund, &rates(), "2026-08-03").unwrap();
        fund.prior_verdict = Some(v);
        let cont_req = interpret_request("qwen", &input(&fund, &engine_output));
        let schema = cont_req.format_schema.as_ref().unwrap();
        assert!(schema["properties"].get("what_changed").is_some());
        let series = schema["properties"]["ledger"]["properties"]["falsifiers"]["items"]["properties"]["quant"]["properties"]["series"]["enum"].to_string();
        assert!(series.contains("expense-ratio") && !series.contains("pe-ratio"), "{series}");
        let role_req = role_risk_request("qwen", &RoleRiskInput {
            input_delta: &[],
            dossier: &stock,
            prior_ledger: None,
            readout: &RoleRiskReadout::default(),
            ledger_eval: None,
            distilled: "",
        });
        assert!(role_req.format_schema.as_ref().unwrap()["properties"].get("what_changed").is_none());
    }

    /// The app owns a debut's continuity fields on every analyst path
    /// (fix list 3.3, ruled 2026-09-16 F6): the stub authors its own line, and
    /// the persisted verdict still carries the app's sentence and no rows; a
    /// continuity run keeps the analyst's line.
    #[test]
    fn a_debut_persists_the_app_written_continuity_fields_on_both_branches() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let VerdictDisposition::Priced(graded) = &v.disposition else { panic!("priced") };
        assert_eq!(graded.what_changed, crate::portfolio::DEBUT_WHAT_CHANGED);
        let mut second = d.clone();
        second.prior_verdict = Some(v.clone());
        let (v2, _) = analyze_holding(&StubAnalyst, &second, &rates(), "2026-08-10").unwrap();
        let VerdictDisposition::Priced(graded) = &v2.disposition else { panic!("priced") };
        assert!(graded.what_changed.starts_with("Reaffirmed"), "{}", graded.what_changed);

        let fund = fund_dossier(bond_fund());
        let (rv, _) = analyze_holding(&StubAnalyst, &fund, &rates(), "2026-08-03").unwrap();
        let VerdictDisposition::RoleRiskOnly(role) = &rv.disposition else { panic!("role/risk: {:?}", rv.disposition) };
        assert_eq!(role.what_changed, crate::portfolio::DEBUT_WHAT_CHANGED);
        // The role/risk continuity run keeps the analyst's line too.
        let mut second = fund.clone();
        second.prior_verdict = Some(rv.clone());
        let (rv2, _) = analyze_holding(&StubAnalyst, &second, &rates(), "2026-08-10").unwrap();
        let VerdictDisposition::RoleRiskOnly(role) = &rv2.disposition else { panic!("role/risk: {:?}", rv2.disposition) };
        assert!(role.what_changed.starts_with("Reaffirmed"), "{}", role.what_changed);
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
        // The stub's debut trigger (price above 150) is authored at a spot of
        // 140 — a crossing ahead, so the authoring-surface check keeps it — and
        // breached at spot 195 from run 2 on: a market-data condition (count 2)
        // whose observation identity is the marks' trading day, so run 2 logs
        // a quiet first-breach note, run 3 — carrying a genuinely NEW trading
        // print — confirms and fires, and the consuming pass stamps the
        // acknowledging observation.
        let mut fin1 = strong_financials();
        fin1.current_price = Some(140.0);
        let d1 = dossier(AssetClass::Stock, fin1);
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

    /// A well-formed row whose period normalizes to its ISO period end and
    /// whose publication date is role-aware under the guidance vintage policy
    /// (Codex I4) — guidance sixty days before the period end, an actual
    /// thirty days after — so a fixture pair is ex ante by construction.
    fn pre_profit_observation(
        role: ObservationRole,
        value: f64,
        period: &str,
    ) -> PreProfitObservation {
        let period = crate::portfolio::pre_profit::normalize_period(period);
        let end = chrono::NaiveDate::parse_from_str(&period, "%Y-%m-%d")
            .expect("the fixture period normalizes");
        let days = if role == ObservationRole::Actual {
            30
        } else {
            -60
        };
        let published_at = (end + chrono::Duration::days(days))
            .format("%Y-%m-%d")
            .to_string();
        PreProfitObservation {
            metric_kind: MetricKind::Deliveries,
            observation_role: role,
            polarity: ObservationPolarity::HigherIsBetter,
            numeric_value: value,
            units: "units".into(),
            period,
            period_span: crate::portfolio::pre_profit::PeriodSpan::Quarter,
            issuer_scope: "company".into(),
            source_url: "https://example.com/ir".into(),
            source_excerpt: format!("reported deliveries of {value} units"),
            published_at,
            confidence: 0.9,
            admitted_under: crate::portfolio::PROMPT_VERSION.into(),
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
    fn eligible_overlay_renders_the_ceiling_rule_and_persists() {
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
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "none",
            ledger_eval: None,
            pre_profit: Some(&overlay),
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(user.contains("\nPRE-PROFIT EXECUTION AND FINANCING\n"), "{user}");
        // The ceiling renders as the rule's effect on the computed conviction —
        // data, with no arm narration and no binding language aimed at the
        // model (`portfolio-v40`; Codex round 1, finding 1).
        assert!(user.contains("- computed conviction capped at medium by rule: "), "{user}");
        assert!(user.contains("repeated-execution-miss"), "{user}");
        for narration in [
            "PRE-PROFIT EXECUTION / FINANCING OVERLAY",
            "CONVICTION CEILING",
            "engine arm",
            "UNRESTRICTED",
            "binds after any raise",
            "your action must be",
        ] {
            assert!(!user.contains(narration), "`{narration}` leaked: {user}");
        }
    }

    #[test]
    fn financing_bar_renders_the_rule_without_stage_narration() {
        let mut fin = pre_profit_financials();
        fin.cash_and_equivalents = Some(1.0e9);
        fin.short_term_investments = None;
        let overlay = pre_profit::compute_overlay(&fin, None, vec![]);
        assert!(overlay.consequences.bar_add_family);
        assert!(!overlay.consequences.exit_family_only);

        let interp = pre_profit_prompt_section(&overlay, PromptStage::Interpretation);
        let action = pre_profit_prompt_section(&overlay, PromptStage::Action);
        assert!(
            interp.contains("- the computed action set excludes adding, on the financing rule.\n"),
            "{interp}"
        );
        for prompt in [&interp, &action] {
            for narration in ["stage that follows", "This stage authors", "UNRESTRICTED", "engine"] {
                assert!(!prompt.contains(narration), "`{narration}` leaked: {prompt}");
            }
        }
        // The action packet renders the facts alone — its SUPPORTED ACTIONS
        // line carries the narrowed set (ruled 2026-09-17, F1) — and the facts
        // block is shared byte for byte.
        assert!(!action.contains("computed action set"), "{action}");
        assert!(interp.starts_with(action.as_str()), "{interp}\n{action}");
    }

    #[test]
    fn severe_overlay_binds_the_engine_arm_never_the_model() {
        // Repeated miss + constrained runway (tiny cash against the burn) → the
        // severe conjunction. Under v7 a defiant model lean and conviction persist
        // exactly as authored — no bail, no clamp — while the consequences bind
        // the ENGINE arm's action and stay recorded for the annotation render.
        struct DefiantAnalyst;
        impl HoldingAnalyst for DefiantAnalyst {
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

        // The overlay section keeps the engine rule factual in both prompts and
        // states the model arm's freedom only where that prompt returns the field.
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: d.prior_ledger(),
            engine: &engine_output,
            distilled: "none",
            ledger_eval: None,
            pre_profit: Some(&overlay),
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(interp.contains("- computed conviction capped at low by rule: "), "{interp}");
        assert!(
            interp.contains(
                "- severe deterioration: the computed action set narrows to trim and sell-all.\n"
            ),
            "{interp}"
        );
        for narration in ["UNRESTRICTED", "engine arm", "engine's own action set", "narrows to the exit family"] {
            assert!(!interp.contains(narration), "`{narration}` leaked: {interp}");
        }

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
                ledger: verdict.thesis_ledger.as_ref().unwrap(),
            },
            engine_set: &engine_set,
            changes: None,
            profile: &d.profile,
        });
        assert!(!action.contains("UNRESTRICTED"), "{action}");
        // The overlay's facts render on the action packet; its consequence
        // lines do not — SUPPORTED ACTIONS carries the narrowed set (ruled
        // 2026-09-17, F1).
        assert!(action.contains("- severe deterioration (conjunctive): YES\n"), "{action}");
        assert!(!action.contains("conviction capped"), "{action}");
        assert!(!action.contains("computed action set"), "{action}");
        assert!(
            action.contains("\nSUPPORTED ACTIONS (computed)\nThe rungs the computed read supports on its own: sell-all, trim.\n"),
            "{action}"
        );
        assert!(!action.contains("CONVICTION CEILING"), "{action}");
        for prompt in [&interp, &action] {
            for residue in [
                "stage that follows",
                "authored the conviction",
                "this call authors no conviction",
                "This stage authors",
            ] {
                assert!(!prompt.contains(residue), "prompt leaked `{residue}`: {prompt}");
            }
        }

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


    // ---- The 6g core checks and the rendered statement (`portfolio-v45`) --------

    fn core(series: &str, comparator: &str, threshold: f64, margin: f64) -> QuantCoreDraft {
        QuantCoreDraft {
            series: series.into(),
            comparator: comparator.into(),
            threshold,
            margin,
        }
    }

    /// The reason class a check returned, or `None` where the core validated.
    fn class_of(qd: QuantCoreDraft, is_fund: bool) -> Option<String> {
        validate_quant_core(&qd, is_fund)
            .err()
            .map(|e| e.split(':').next().unwrap().to_string())
    }

    #[test]
    fn six_g_core_checks_cover_the_structural_classes_and_the_margin_bounds() {
        use downgrade_class as c;
        // Equality moves the "below" boundary to zero (Codex, plan round 2).
        assert_eq!(class_of(core("price", "below", 100.0, 100.0), false).as_deref(), Some(c::MARGIN));
        // Under the magnitude bound but past the relative cap (fix list 1.8, ruled
        // 2026-09-16): a quarter of the level on the price, half on a fraction.
        assert_eq!(class_of(core("price", "below", 100.0, 99.0), false).as_deref(), Some(c::MARGIN));
        assert_eq!(class_of(core("price", "below", 100.0, 26.0), false).as_deref(), Some(c::MARGIN));
        assert_eq!(class_of(core("price", "below", 100.0, 25.0), false), None);
        // A zero threshold is exempt: the margin is its only scale.
        assert_eq!(class_of(core("net-margin", "below", 0.0, 0.02), false), None);
        // A negative threshold compares on magnitude.
        assert_eq!(class_of(core("trailing-return", "below", -0.40, 0.40), false).as_deref(), Some(c::MARGIN));
        assert_eq!(class_of(core("trailing-return", "below", -0.40, 0.21), false).as_deref(), Some(c::MARGIN));
        assert_eq!(class_of(core("trailing-return", "below", -0.40, 0.20), false), None);
        // The structural classes: an unresolvable series, a series the vehicle
        // never computes, a malformed comparator.
        assert_eq!(class_of(core("ebitda-margin", "below", 0.2, 0.01), false).as_deref(), Some(c::SERIES_UNRESOLVED));
        assert_eq!(class_of(core("pe-ratio", "above", 38.0, 0.5), true).as_deref(), Some(c::SERIES_UNCOMPUTABLE));
        assert_eq!(class_of(core("price", "under", 100.0, 1.0), false).as_deref(), Some(c::MALFORMED));
    }

    #[test]
    fn a_rendered_statement_states_the_rule_the_engine_runs() {
        use crate::portfolio::StatementBasis;
        let q = |series: engine::LedgerSeries, comparator: LedgerComparator, threshold: f64, margin: f64| QuantCore {
            series,
            comparator,
            threshold,
            margin,
        };
        assert_eq!(
            q(engine::LedgerSeries::GrossMargin, LedgerComparator::Below, 0.16, 0.005)
                .render(Some("Gross-margin floor"), Some(StatementBasis::Ttm)),
            "Gross-margin floor — gross margin (TTM) below 16%, confirmed by one filing; margin ±0.5pp"
        );
        assert_eq!(
            q(engine::LedgerSeries::Price, LedgerComparator::Below, 38.0, 0.4).render(Some("Price support"), None),
            "Price support — price below $38.00, confirmed by two consecutive daily closes; margin ±$0.40"
        );
        assert_eq!(
            q(engine::LedgerSeries::PeRatio, LedgerComparator::Above, 25.0, 1.0)
                .render(Some("Multiple ceiling"), Some(StatementBasis::Annual)),
            "Multiple ceiling — price / earnings multiple (annual) above 25x, confirmed by two consecutive daily closes; margin ±1x"
        );
        assert_eq!(
            q(engine::LedgerSeries::ExpenseRatio, LedgerComparator::Above, 0.0075, 0.0005).render(Some("Cost drift"), None),
            "Cost drift — fund expense ratio above 0.75%, confirmed by one filing; margin ±0.05pp"
        );
        // No name: the sentence opens capitalized; the basis rides only a flow
        // series; a zero margin reads as none.
        assert_eq!(
            q(engine::LedgerSeries::TrailingReturn, LedgerComparator::Below, -0.40, 0.02)
                .render(None, Some(StatementBasis::Ttm)),
            "Trailing price return below -40%, confirmed by two consecutive daily closes; margin ±2pp"
        );
        assert_eq!(
            q(engine::LedgerSeries::DebtToEquity, LedgerComparator::Above, 1.5, 0.0).render(Some(" "), None),
            "Debt / equity ratio above 1.5, confirmed by one filing; no margin"
        );
        assert_eq!(
            q(engine::LedgerSeries::ReturnVolatility, LedgerComparator::Above, 0.0267, 0.002)
                .render(Some("Regime break"), None),
            "Regime break — daily realized return volatility above 2.67%, confirmed by two consecutive daily closes; margin ±0.2pp"
        );
        // A refused draft whose core never parsed still renders what was asked.
        assert_eq!(
            render_unparsed_draft(&core("ebitda-margin", "under", 0.2, 0.01), "Cash floor"),
            "Cash floor — ebitda-margin under 0.2 (margin 0.01)"
        );
    }

    #[test]
    fn six_g_refuses_a_new_core_that_already_holds_and_keeps_a_carried_one() {
        use downgrade_class as c;
        // A debut trigger "price above $150" at a spot of 200 already holds: not
        // a crossing ahead, refused with the class and the shown value, its
        // statement still rendered from the draft behind the model's name.
        let draft = stub_ledger_draft(None, "AAPL", false);
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, false, None, Some(200.0));
        let trigger = ledger.conditions.iter().find(|c| c.role == ConditionRole::Trigger).unwrap();
        assert!(trigger.quant.is_none() && trigger.eval_state.is_none());
        let reason = trigger.downgraded_reason.as_deref().unwrap();
        assert!(reason.starts_with("holds-at-authoring:"), "{reason}");
        assert_eq!(
            reason,
            "holds-at-authoring: the price was already above $150.00 when authored, past the margin $0.00; it stood at $200.00"
        );
        assert_eq!(trigger.label.as_deref(), Some("Priced-in ceiling"));
        assert_eq!(
            trigger.statement,
            "Priced-in ceiling — price above $150.00, confirmed by two consecutive daily closes; no margin"
        );
        assert!(audit.downgraded.iter().any(|d| d.contains(c::HOLDS_AT_AUTHORING)), "{:?}", audit.downgraded);
        assert_eq!(ledger.conditions.len(), 2, "refused, never dropped");
        // A kept core's statement is the render and its label the model's name.
        let falsifier = ledger.conditions.iter().find(|c| c.role == ConditionRole::Falsifier).unwrap();
        assert_eq!(falsifier.label.as_deref(), Some("Deep drawdown"));
        assert_eq!(
            falsifier.statement,
            "Deep drawdown — trailing price return below -40%, confirmed by two consecutive daily closes; margin ±2pp"
        );
        // At a spot of 120 the same core is a crossing ahead and keeps its core;
        // the trailing-return core reads no metric here and keeps too.
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, false, None, Some(120.0));
        assert!(ledger.conditions.iter().all(|c| c.quant.is_some()), "{:?}", audit.downgraded);
        assert!(audit.downgraded.is_empty());

        // A carried-verbatim core that now holds keeps its id and state: it was
        // authored earlier, and its streak is the point.
        let mut prior = prior_with_conditions();
        prior.conditions.push(LedgerCondition {
            condition_id: "px-keep".into(),
            role: ConditionRole::Trigger,
            trigger_family: Some(TriggerFamily::Trim),
            statement: "Priced-in ceiling — price above $150.00, confirmed by two consecutive daily closes; no margin".into(),
            label: Some("Priced-in ceiling".into()),
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
            eval_state: Some(ConditionEvalState {
                breach_streak: 1,
                ..Default::default()
            }),
        });
        let draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, Some(200.0));
        let carried = ledger.conditions.iter().find(|c| c.condition_id == "px-keep").expect("carried verbatim");
        assert!(carried.quant.is_some());
        assert_eq!(carried.eval_state.as_ref().unwrap().breach_streak, 1);
        assert!(audit.downgraded.is_empty(), "{:?}", audit.downgraded);
        // An edited core that holds at the spot is refused; its assigned
        // ancestor closes whole rather than lending its streak.
        let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
        let t = draft.triggers.iter_mut().find(|t| t.statement == "Priced-in ceiling").unwrap();
        t.quant.as_mut().unwrap().threshold = 160.0;
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, false, None, Some(200.0));
        let refused = ledger.conditions.iter().find(|c| c.label.as_deref() == Some("Priced-in ceiling")).unwrap();
        assert!(refused.quant.is_none());
        assert!(refused.downgraded_reason.as_deref().unwrap().starts_with("holds-at-authoring:"));
        assert!(refused.statement.contains("above $160.00"), "{}", refused.statement);
        assert!(
            audit.closed.iter().any(|c| c.condition.condition_id == "px-keep"),
            "the ancestor closes whole: {:?}",
            audit.closed.iter().map(|c| &c.condition.condition_id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_refused_core_persists_its_render_and_the_class_prefix() {
        // Through the seam: a structurally refused core persists qualitative
        // with the class-prefixed reason, no machine state, the audit line and
        // a statement rendered from the draft — never dropped, never repaired.
        let mut draft = stub_ledger_draft(None, "PGNY", false);
        draft.falsifiers = vec![FalsifierDraft {
            statement: "Volatility regime".into(),
            quant: Some(core("return-volatility", "above", 0.0267, 0.15)),
            technology_class: false,
            tripped: false,
        }];
        draft.triggers = vec![];
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, false, None, None);
        let cond = &ledger.conditions[0];
        assert!(cond.quant.is_none() && cond.eval_state.is_none());
        assert_eq!(cond.label.as_deref(), Some("Volatility regime"));
        assert_eq!(
            cond.statement,
            "Volatility regime — daily realized return volatility above 2.67%, confirmed by two consecutive daily closes; margin ±15pp"
        );
        let reason = cond.downgraded_reason.as_deref().unwrap();
        assert!(reason.starts_with("margin-implausible:"), "{reason}");
        assert!(audit.downgraded.iter().any(|d| d.contains("margin-implausible:")), "{:?}", audit.downgraded);
        assert_eq!(ledger.conditions.len(), 1, "refused, never dropped");
        // A re-emission of the same refused draft carries the id: the render is
        // deterministic, so qualitative identity holds without any prose read.
        let (again, _) =
            validate_ledger_rewrite(&draft, Some(&ledger), None, LedgerBranch::Priced, false, None, None);
        assert_eq!(again.conditions[0].condition_id, cond.condition_id);
    }

    #[test]
    fn the_prior_ledger_row_prints_the_raw_core_beside_the_name() {
        // The continuity prompt shows a kept core once, raw, with the model's
        // own name — never the rendered sentence, whose rounded figures the
        // model would have to convert back (ruled 2026-09-18).
        let mut prior = prior_with_conditions();
        prior.conditions[0].label = Some("Deep drawdown".into());
        prior.conditions[0].rerender_statement();
        let section = prior_ledger_data_section(Some(&prior), None, &[]);
        assert!(
            section.contains("[quantitative: trailing-return below -0.4 (margin 0.02); breach streak 1] Deep drawdown\n"),
            "{section}"
        );
        assert!(!section.contains("confirmed by two consecutive daily closes"), "{section}");
        // A refused condition prints its statement with one data phrase naming
        // why, so the model authors it differently; a qualitative one prints as
        // before.
        prior.conditions[0].quant = None;
        prior.conditions[0].downgraded_reason = Some(
            "holds-at-authoring: the trailing price return was already below -40% when authored, past the margin 2pp; it stood at -55%".into(),
        );
        let section = prior_ledger_data_section(Some(&prior), None, &[]);
        assert!(
            section.contains("[qualitative; the trailing price return was already below -40% when authored, past the margin 2pp; it stood at -55%] Deep drawdown — trailing price return below -40%"),
            "{section}"
        );
        prior.conditions[0].downgraded_reason = Some("margin-implausible: margin 0.4 is at or beyond the threshold's magnitude 0.4".into());
        let section = prior_ledger_data_section(Some(&prior), None, &[]);
        assert!(section.contains("[qualitative; the margin is too wide for the level] Deep drawdown"), "{section}");
        for narration in ["the app", "engine", "downgrade", "machine-evaluated", "unevaluable"] {
            assert!(!section.contains(narration), "`{narration}` leaked: {section}");
        }
    }

    #[test]
    fn an_off_scale_value_skips_the_authoring_check_like_a_missing_one() {
        // A loss-maker's negative P/E is off-scale for the evaluator, which
        // types the condition unevaluable rather than compared — so a new
        // "P/E below 15x" keeps its core there (ruled 2026-09-18), while the
        // same core at a positive P/E of 10 already holds and is refused.
        let mut draft = stub_ledger_draft(None, "X", false);
        draft.falsifiers = vec![FalsifierDraft {
            statement: "Cheap multiple".into(),
            quant: Some(core("pe-ratio", "below", 15.0, 0.5)),
            technology_class: false,
            tripped: false,
        }];
        draft.triggers = vec![];
        let at = |pe: f64| {
            let metrics = engine::ComputedMetrics { pe_ratio: Some(pe), ..Default::default() };
            validate_ledger_rewrite_with_research(
                &draft, None, None, LedgerBranch::Priced, false, None, None, Some(&metrics),
                &std::collections::HashSet::new(), true, crate::portfolio::ContinuityStamps::NONE,
            ).0
        };
        assert!(at(-20.0).conditions[0].quant.is_some(), "off-scale skips the check");
        let refused = at(10.0);
        assert!(refused.conditions[0].downgraded_reason.as_deref().unwrap().starts_with("holds-at-authoring:"));
        assert!(at(20.0).conditions[0].quant.is_some(), "a crossing ahead keeps");
    }

    #[test]
    fn a_rebased_price_core_rerenders_its_statement() {
        // The split re-basis scales a price core and re-renders its sentence,
        // so the statement names the level the core now carries.
        let mut cond = prior_with_conditions().conditions.remove(0);
        cond.label = Some("Support".into());
        cond.quant = Some(QuantCore {
            series: engine::LedgerSeries::Price,
            comparator: LedgerComparator::Below,
            threshold: 700.0,
            margin: 20.0,
        });
        cond.rerender_statement();
        assert_eq!(cond.statement, "Support — price below $700.00, confirmed by two consecutive daily closes; margin ±$20.00");
        let q = cond.quant.as_mut().unwrap();
        q.threshold *= 0.25;
        q.margin *= 0.25;
        cond.rerender_statement();
        assert_eq!(cond.statement, "Support — price below $175.00, confirmed by two consecutive daily closes; margin ±$5.00");
    }

    // ---- The investment-only action packet and the app-appended tax caveat ------

    #[test]
    fn the_action_packet_states_polarity_and_the_set_once_and_carries_no_size() {
        use crate::portfolio::dossier::{OptionOverlay, OverlayClass, OverlayDirection, OverlayLeg};
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.option_overlay = Some(OptionOverlay {
            legs: vec![OverlayLeg {
                contract: "AAPL 260117C00220000".into(),
                direction: OverlayDirection::Short,
                quantity: 2.0,
                kind: Some(crate::schwab::OptionKind::Call),
                strike: Some(220.0),
                expiry: Some("2026-01-17".into()),
                delta: Some(-0.35),
            }],
            class: OverlayClass::CoveredCall,
            coverage_ratio: Some(1.0),
            net_delta: Some(-70.0),
            delta_source_consulted: true,
            gaps: vec![],
        });
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let VerdictDisposition::Priced(graded) = &v.disposition else { panic!("priced") };
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let engine_set =
            engine::feasible_actions(engine_output.grade, &engine_output.hurdle, None, false);
        let user = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced {
                graded,
                engine: &engine_output,
                pre_profit: None,
                ledger: v.thesis_ledger.as_ref().unwrap(),
            },
            engine_set: &engine_set,
            changes: None,
            profile: &d.profile,
        });
        // Polarity once, on the shared gloss (2.3); the set once, as one data
        // line with no permission sentence (2.2; 3.9 ruled 2026-09-17).
        assert_eq!(user.matches("higher is better on every axis").count(), 1, "{user}");
        assert_eq!(user.matches("\nSUPPORTED ACTIONS (computed)\n").count(), 1, "{user}");
        for absent in [
            "The full ladder is yours", "its own pick", "ENGINE SET", "ENGINE ADMISSION FACTS",
            "restriction", "not a bound", "departure", "Its selected action",
        ] {
            assert!(!user.contains(absent), "{absent}: {user}");
        }
        // No account economics on any route (2.1, F5, C1): the header carries
        // identity and spot only, and the overlay renders structure and ratios.
        for absent in ["Quantity", "Market value", "Cost basis", "Unrealized", "share-equivalents", "2×", "- tax"] {
            assert!(!user.contains(absent), "{absent}: {user}");
        }
        assert!(
            user.starts_with("======== PART 1: INPUTS ========\nHOLDING\nAAPL (Apple).\nPrice: $195.00 per share.\n"),
            "{user}"
        );
        assert!(user.contains("covering 100% of the held shares; net delta -70% of the held shares"), "{user}");
        assert!(user.contains("- SHORT CALL — strike 220.00"), "{user}");
        // The interpretation prompt renders the same unsized overlay since
        // portfolio-v38 (fix list 3.2, the Codex plan review): contracts over
        // coverage had given the held share count away.
        let interp = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: None,
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(interp.contains("covering 100% of the held shares; net delta -70% of the held shares"), "{interp}");
        assert!(interp.contains("- SHORT CALL — strike 220.00"), "{interp}");
        for absent in ["share-equivalents", "2×", "Quantity", "Market value", "Cost basis"] {
            assert!(!interp.contains(absent), "{absent}: {interp}");
        }
    }

    #[test]
    fn the_investment_sentence_strips_exactly_the_appended_caveat() {
        let gain = with_tax_caveat(
            "Trim on the verdict.  ".into(),
            &InvestorProfile::default_fixture(),
            &position(AssetClass::Stock),
            Action::Trim,
        );
        assert!(gain.ends_with(TAX_CAVEAT_GAIN));
        assert_eq!(investment_sentence(&gain), "Trim on the verdict.");
        assert_eq!(investment_sentence(&format!("Sell it. {TAX_CAVEAT_LOSS}")), "Sell it.");
        // No caveat, nothing stripped — a sentence that merely mentions tax stays.
        assert_eq!(investment_sentence("Hold; tax is not the reason."), "Hold; tax is not the reason.");
        assert_eq!(investment_sentence(""), "");
    }

    #[test]
    fn the_tax_caveat_rides_only_an_exit_rung_under_a_tax_aware_profile() {
        let mut profile = InvestorProfile::default_fixture();
        let mut pos = position(AssetClass::Stock); // market 19,500 vs cost 14,000: a gain
        assert_eq!(tax_caveat(&profile, &pos, Action::Trim), Some(TAX_CAVEAT_GAIN));
        assert_eq!(tax_caveat(&profile, &pos, Action::SellAll), Some(TAX_CAVEAT_GAIN));
        for rung in [Action::Hold, Action::Add, Action::AddAggressively] {
            assert_eq!(tax_caveat(&profile, &pos, rung), None, "{rung:?}");
        }
        pos.cost_basis = 30_000.0;
        assert_eq!(tax_caveat(&profile, &pos, Action::SellAll), Some(TAX_CAVEAT_LOSS));
        pos.cost_basis = pos.market_value;
        assert_eq!(tax_caveat(&profile, &pos, Action::SellAll), None, "break-even");
        profile.tax_sensitive = false;
        pos.cost_basis = 14_000.0;
        assert_eq!(tax_caveat(&profile, &pos, Action::Trim), None, "tax-exempt");
        assert_eq!(
            with_tax_caveat("Trim on the verdict.  ".into(), &InvestorProfile::default_fixture(), &pos, Action::Trim),
            format!("Trim on the verdict. {TAX_CAVEAT_GAIN}")
        );

        // Through the pipeline: the persisted rationale is the model's sentence
        // plus the app's caveat, appended after the rung is fixed.
        struct Trimmer;
        impl HoldingAnalyst for Trimmer {
            fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
                StubAnalyst.interpret(input)
            }
            fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
                StubAnalyst.interpret_role_risk(input)
            }
            fn decide_action(&self, _: &ActionInput) -> Result<crate::portfolio::ActionDecision> {
                Ok(crate::portfolio::ActionDecision {
                    action: Action::Trim,
                    rationale: "Trim on the verdict.".into(),
                })
            }
            fn fast_id(&self) -> String {
                StubAnalyst.fast_id()
            }
            fn reasoner_id(&self) -> String {
                StubAnalyst.reasoner_id()
            }
        }
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (v, _) = analyze_holding(&Trimmer, &d, &rates(), "2026-08-03").unwrap();
        let VerdictDisposition::Priced(g) = &v.disposition else { panic!("priced") };
        assert_eq!(g.action, Action::Trim);
        assert_eq!(g.action_rationale, format!("Trim on the verdict. {TAX_CAVEAT_GAIN}"));
        d.profile.tax_sensitive = false;
        let (v, _) = analyze_holding(&Trimmer, &d, &rates(), "2026-08-03").unwrap();
        let VerdictDisposition::Priced(g) = &v.disposition else { panic!("priced") };
        assert_eq!(g.action_rationale, "Trim on the verdict.");
    }

    #[test]
    fn the_ledger_contract_scopes_series_by_vehicle_and_shows_observations() {
        // A fund never sees a stock-only series; a stock's current value renders
        // beside each series it may threshold (1.1's checks). Both messages
        // render one set of metric lines under FINANCIAL METRICS through one
        // section (`portfolio-v42`).
        let fund_contract = LedgerSeriesContract::build(true, None, None);
        let fund = fund_contract.metric_lines();
        for absent in ["[net-margin]", "[gross-margin]", "[revenue-growth]", "[pe-ratio]", "[debt-to-equity]"] {
            assert!(!fund.contains(absent), "{absent}: {fund}");
        }
        for present in ["[expense-ratio]: (gap)", "[price]: (gap)", "[return-volatility]: (gap)", "[trailing-return]: (gap)"] {
            assert!(fund.contains(present), "{present}: {fund}");
        }
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let stock = LedgerSeriesContract::build(false, Some(&engine_output.metrics), Some(&d.financials));
        let rendered = stock.metric_lines();
        assert!(!rendered.contains("[expense-ratio]"), "{rendered}");
        let net = engine_output.metrics.net_margin.unwrap();
        assert!(
            rendered.contains(&format!(
                "- net margin [net-margin]: {net:.4} — a fraction, never a percent (0.16 means 16%); confirmed by one filing\n"
            )),
            "{rendered}"
        );
        assert!(
            rendered.contains("- the holding's price (account currency) [price]: 195.00 — dollars per share; confirmed by two consecutive daily closes\n"),
            "{rendered}"
        );
        // The item's authoring sentences (1.7, 1.9 and 3.8) on both vehicles:
        // the level in the sentence, the threshold exactly it, the margin the
        // separate band, and the key-driver null rule — with the margin caps
        // enforced, not shown (`portfolio-v40`).
        for (contract, branch, label) in [
            (&stock, LedgerItemBranch::PricedStock, "stock"),
            (&fund_contract, LedgerItemBranch::PricedFund, "fund"),
        ] {
            let item = ledger_task_item(5, contract, branch, false);
            assert!(item.contains("statement is a short name for the condition, without a figure"), "{label}: {item}");
            assert!(item.contains("one the metric has not already crossed"), "{label}: {item}");
            assert!(item.contains("threshold: the level, in the metric's unit"), "{label}: {item}");
            assert!(item.contains("margin: the noise around the threshold that a crossing must clear, in the same unit — small relative to the level"), "{label}: {item}");
            assert!(item.contains("otherwise series is null."), "{label}: {item}");
            for narration in ["at most", "A zero level has no cap", "current observation", "confirms on", "ENGINE SERIES", "METRICS AVAILABLE"] {
                assert!(!item.contains(narration), "{label}: `{narration}` leaked: {item}");
            }
        }
        // Both messages carry the worked examples in the vehicle's vocabulary.
        assert!(stock.examples().contains("gross-margin"), "{}", stock.examples());
        assert!(fund_contract.examples().contains("Price support"));
        let user = interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: None,
            engine: &engine_output,
            distilled: "",
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        assert!(
            user.contains("Example, quantitative: statement \"Gross-margin floor\", quant {\"series\": \"gross-margin\", \"comparator\": \"below\", \"threshold\": 0.16, \"margin\": 0.005}."),
            "{user}"
        );
        // The priced message renders each metric line once, under FINANCIAL
        // METRICS, and the ledger item points there by label; the sizing
        // examples stand where the caps were shown.
        assert_eq!(user.matches("[net-margin]:").count(), 1, "{user}");
        assert!(
            user.contains("quant holds series (one of net-margin, gross-margin, revenue-growth, debt-to-equity, return-volatility, trailing-return, pe-ratio, ps-ratio, pb-ratio, price)"),
            "{user}"
        );
        assert!(user.contains("statement is a short name for the condition, without a figure"), "{user}");
        assert!(
            user.contains("for example 2 on a price of 100, 0.005 on a net margin of 0.16, or 1 on a P/E of 25."),
            "{user}"
        );
        for narration in [
            "Worked examples",
            "The statement and `quant` must agree",
            "at most",
            "METRICS AVAILABLE FOR QUANTITATIVE LEDGER CONDITIONS",
            "the app",
            "downgrades",
        ] {
            assert!(!user.contains(narration), "`{narration}` leaked: {user}");
        }
    }
}
