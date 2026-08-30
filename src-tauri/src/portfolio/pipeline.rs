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
        if let Some(text) = research::assemble_topic_seed(prior, prior_ledger, now) {
            let vintage = prior
                .filter(|p| research::topic_object_fresh(p, now))
                .map(|p| p.vintage.clone())
                .unwrap_or_default();
            topic_seeds.insert(topic.key.clone(), (text, vintage));
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
    let inputs = DistillInputs {
        symbol,
        company_name: dossier.company_name.as_deref(),
        research: &research_out,
        priors: &priors,
        ledger_conditions: &ledger_conditions,
        ledger_key_drivers: &ledger_key_drivers,
        role_risk,
        overlay_eligible: triggers.overlay_eligible,
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
        tavily_fallback_used: research_out.tavily_fallback_used,
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

/// The research-fed forensic claim is **advisory by ruling (2026-08-24)** — it
/// never merges into the hard-forensic producer state (the hard rule trips
/// from the item-classified filing kinds alone; the retired merge returns only
/// with an explicit promotion ruling or a source-specific adapter that reads
/// the accused party from structured document fields). The validated claim
/// rides the research audit record and reaches interpretation as this
/// clearly-labeled attention-evidence block.
fn render_forensic_advisory(claim: &crate::portfolio::distill::ForensicEventClaim) -> String {
    format!(
        "RESEARCH-FED FRAUD CLAIM (advisory attention evidence — the citation is validated, \
         but attribution to this issuer is unconfirmed; NOT a hard trigger, binds nothing): \
         issuer {:?}, event date {}, source {} (confidence {:.2})",
        claim.issuer, claim.event_date, claim.source_url, claim.confidence
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
    // core reaches them; statement prose is model-authored and never rewritten).
    // Margins scale with thresholds (absolute, same units), so breach semantics
    // are invariant under the conversion, and a verbatim re-emission of the
    // converted number matches the normalized core and keeps its condition id.
    // The monitor's stamped engine targets convert for render coherence; on a
    // resolvable pass validation re-stamps them from this run's engine set (an
    // unresolvable pass withholds the fresh stamp — absent beats wrong).
    let bridged_ledger: Option<crate::portfolio::ThesisLedger> = match price_bridge {
        Some(f) if f != 1.0 => prior_ledger.map(|l| {
            let mut l = l.clone();
            for cond in &mut l.conditions {
                if let Some(q) = &mut cond.quant {
                    if q.series.price_denominated() {
                        q.threshold *= f;
                        q.margin *= f;
                    }
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
                let price_legs = engine::compute_metrics(&dossier.financials);
                let fund_metrics = engine::ComputedMetrics {
                    expense_ratio: readout.expense_ratio,
                    // The closed-end read joins the branch's computed surface
                    // (populated only on the CEF form), so once a NAV is served
                    // a premium move seeds its own input-delta row instead of
                    // being invisible to continuity (Codex 2026-08-21 round 3,
                    // finding 3). No ledger series reads it — the engine series
                    // surface is closed.
                    nav_premium: readout.nav_premium,
                    return_volatility: price_legs.return_volatility,
                    trailing_return: price_legs.trailing_return,
                    ..Default::default()
                };
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
                house_view_consulted.set(role_risk_prompt_renders_house_view(dossier));
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
                    &research_supported,
                    price_bridge.is_some(),
                    crate::portfolio::ContinuityStamps::of(&dossier.financials),
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
                    structural_flag: readout.structural_flag(),
                    is_cef: readout.is_cef,
                    nav_premium: readout.nav_premium,
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
                record_stage_models(analyst.reasoner_id());
                ensure_action_rationale(&symbol, &decision)?;
                rr.action = decision.action;
                rr.action_rationale = decision.rationale;
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
    house_view_consulted.set(priced_prompt_renders_house_view(dossier));
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
                    supersede: assumption.conflict_handling
                        == crate::portfolio::distill::ConflictHandling::Supersede,
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
    // The typed indicator reaches the model as ledger-driver evidence (its
    // conviction-raise role is retired suite-wide with `portfolio-v7`).
    let distilled = match &distilled_research.leading_indicator {
        Some(ind) => format!(
            "{distilled}\n\nVALIDATED LEADING INDICATOR (typed, engine-unscored \
             ledger-driver evidence): {} = {} ({}, as of {}) — confirms driver: {} [{}]{}",
            ind.metric_name,
            ind.value,
            match ind.direction {
                crate::portfolio::distill::IndicatorDirection::InflectingUp => "inflecting up",
                crate::portfolio::distill::IndicatorDirection::InflectingDown => "inflecting down",
            },
            ind.as_of,
            ind.confirms_driver,
            ind.source_url,
            if ind.driver_verified {
                " — driver reference verified against the ledger"
            } else {
                " — driver reference UNVERIFIED (evidence only; no cap suppression)"
            }
        ),
        None => distilled,
    };
    // The advisory fraud claim reaches the model as cited attention evidence —
    // clearly labeled: it is not a hard trigger and binds nothing.
    let distilled = match &distilled_research.forensic_event {
        Some(claim) => format!("{distilled}\n\n{}", render_forensic_advisory(claim)),
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
    record_stage_models(analyst.reasoner_id());
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
        &research_supported,
        price_bridge.is_some(),
        crate::portfolio::ContinuityStamps::of(&dossier.financials),
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
        risk_tier: engine_output.risk_tier,
        dead_money: engine_output.hurdle.state,
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
    record_stage_models(analyst.reasoner_id());
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
    research_supported: &std::collections::HashSet<String>,
    updated_states: &std::collections::HashMap<String, ConditionEvalState>,
    price_basis_verified: bool,
    stamps: crate::portfolio::ContinuityStamps,
    audit: &mut LedgerAudit,
) -> LedgerCondition {
    let statement = statement.trim().to_string();
    let (quant, downgraded_reason) = match quant_draft {
        None => (None, None),
        Some(qd) => match parse_quant_core(qd, is_fund) {
            Ok(core) => {
                // The unverifiable-basis supersede guard: with the split bridge
                // unresolvable, a NEW or RE-ANCHORED price-denominated core was
                // authored against fresh prices but would persist under the
                // carried prior-basis anchor — an untieable mix, so it
                // downgrades (typed, never dropped). A carried-verbatim core
                // stays quantitative: it shares the carried anchor's basis.
                let carried_verbatim = prior_pool.iter().any(|c| {
                    c.role == role
                        && c.trigger_family == trigger_family
                        && c.quant.as_ref() == Some(&core)
                });
                if !price_basis_verified
                    && core.series.price_denominated()
                    && !carried_verbatim
                {
                    let reason = "the price basis is unverifiable this run \
                                  (split-bridge anchor unresolvable) — a new or \
                                  re-anchored price-denominated core cannot be \
                                  tied to the carried anchor; re-author at a \
                                  resolvable pass"
                        .to_string();
                    audit.downgraded.push(format!("'{statement}': {reason}"));
                    (None, Some(reason))
                } else {
                    (Some(core), None)
                }
            }
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
            // statement; no machine evaluation state either way.
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
/// new or superseding quantitative condition per series
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
            research_supported,
            &updated_states,
            price_basis_verified,
            stamps,
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
        "\nPRIOR ANALYSIS RECALL (semantic, from this job's own memory — \
         continuity context, not fresh evidence):\n",
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
fn target_boundary_row(prior_stamp: &str, horizons: engine::TargetHorizons) -> String {
    format!(
        "scenario-target parameters changed ({prior_stamp} -> {}) — the {} can move with \
         no input change",
        engine::SCENARIO_TARGET_PARAMETER_VERSION,
        horizons.label()
    )
}

/// The continuity NOTE for the same boundary, in the interpretation prompt —
/// the grade NOTE's shape, naming the horizons.
fn target_boundary_note(horizons: engine::TargetHorizons) -> String {
    format!(
        "NOTE: the scenario-target function's parameters changed since the prior verdict \
         (target parameter version changed), so the {} may have moved with no change in \
         the company's inputs. Attribute such a target move in what_changed to the \
         parameter change — not to company change or a self-correction.\n",
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
                "price series re-based since the prior read (split bridge factor \
                 {f:.4}); prior price-denominated values converted onto the fresh basis"
            ),
        ),
        None => push_delta(
            entries,
            "price basis unverifiable (split-bridge anchor unresolvable) — prior \
             price-denominated comparisons excluded this run"
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
                    format!("engine sub-score {name}: {old} -> {new}"),
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
                    "engine twelve-month base target: {old_base} -> {new_base}"
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
                    "capital-efficiency read: {:?} -> {:?}",
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
    // `boundary` is `None` whenever the stamp is, so this never renders empty.
    let prior_stamp = dossier
        .prior_grade_parameter_version
        .as_deref()
        .unwrap_or_default();
    match boundary {
        Some(engine::GradeParameterChange::Letters) => push_delta(
            &mut entries,
            format!(
                "grade bands recalibrated ({prior_stamp} -> {}) — letters can move with no \
                 input change",
                engine::GRADE_PARAMETER_VERSION
            ),
        ),
        Some(engine::GradeParameterChange::FundMomentum) => push_delta(
            &mut entries,
            format!(
                "fund momentum re-homed to the short price window ({prior_stamp} -> {}) — the \
                 momentum sub-score can move with no input change; the letter cannot",
                engine::GRADE_PARAMETER_VERSION
            ),
        ),
        Some(engine::GradeParameterChange::FundSectorPeBasis) => push_delta(
            &mut entries,
            format!(
                "fund sector-P/E exchange basis tightened ({prior_stamp} -> {}) — the valuation \
                 sub-score and letter can move on the same served rows",
                engine::GRADE_PARAMETER_VERSION
            ),
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
            target_boundary_row(
                dossier
                    .prior_target_parameter_version
                    .as_deref()
                    .unwrap_or_default(),
                horizons,
            ),
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
    if let Some(kind) = r.structural_kind {
        let description = match kind {
            FundStructuralKind::LeveragedInverse => {
                "leveraged/inverse daily-reset path dependency"
            }
            FundStructuralKind::OptionOverlay => {
                "option-overlay path dependency (the options reshape the return path)"
            }
        };
        p.push_str(&format!("STRUCTURAL FLAG: {description}\n"));
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
        fmt_expense_ratio(r.expense_ratio),
        opt(r.observable_risk),
    ));
    // The closed-end read renders only where the vehicle makes it meaningful
    // (`docs/portfolio-analysis.md` §Asset eligibility); its absence is a named
    // gap already in the evidence-gap manifest, never a fabricated number.
    if r.is_cef {
        if let Some(prem) = r.nav_premium {
            p.push_str(&nav_premium_line(prem));
        }
    }
    if !r.evidence_gaps.is_empty() {
        p.push_str(&format!("EVIDENCE GAPS: {}\n", r.evidence_gaps.join("; ")));
    }
    // The fund agenda's distilled research — pure consolidation on this branch
    // (`docs/portfolio-workflow.md` §Step 6d).
    p.push_str(&format!("\nDISTILLED RESEARCH:\n{}\n", input.distilled));
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
        input.prior_ledger,
        input.ledger_eval,
        true,
        input.input_delta,
        input.dossier.financials.statement_basis,
        input.dossier.financials.equity_source,
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
        let (model_label, action_read) = match prior.action_source {
            ActionSource::ModelChosen => (
                "prior MODEL arm (yours)",
                format!("action {} (model-chosen)", g.action.as_kebab()),
            ),
            ActionSource::RuleDemoted => (
                "prior MODEL arm (authored read; action later rule-demoted)",
                format!(
                    "persisted action {} (rule-demoted after authoring; the prior model-chosen \
                     rung is unavailable in this record)",
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
            fmt_expense_ratio(f.fund.expense_ratio),
            // The guard's own read (`fund::us_share`): every US alias summed and
            // capped, never a first-label read of one spelling (Codex I8).
            crate::portfolio::fund::us_share(&f.fund)
                .map(|s| format!("{:.0}%", s * 100.0))
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
        // The closed-end read on the priced branch (structurally reachable only
        // once the surface serves a CEF weightings + NAV; gated so an open-end
        // ETF's transient premium never renders as signal).
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
        // Both horizons expose their methodology (`docs/portfolio-analysis.md`
        // §The holding verdict); the one-month line had printed bare numbers,
        // so the model could not see which basis the band stood on (Codex I10).
        p.push_str(&format!(
            "ENGINE ONE-MONTH TARGETS: bear {:.2} / base {:.2} / bull {:.2}\n  methodology: {}\n",
            om.bear, om.base, om.bull, om.methodology
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
         quality/valuation/risk through the same cutoffs; the scale is enforced — \
         a score outside 0-100 is rejected, never clamped); \
         model_price_targets — your own one-month and twelve-month base/bear/bull \
         prices (finite positive numbers — enforced, a zero or negative leg is \
         rejected; bear ≤ base ≤ bull as you mean them — an inverted band is kept \
         as authored and annotated); \
         self_assessment — your honest retrospective (on a debut: say it is a first \
         read). Depart the engine wherever your read of the evidence differs; \
         agreement is a finding, not a requirement.\n",
    );

    let s = &d.options_signal;
    p.push_str(&format!(
        "\nOPTIONS ACTIVITY (proxy only — NOT a grade input): put/call vol {}, put/call OI {}, IV {}, IV skew {} (chain-wide mean put IV minus mean call IV, in IV's decimal unit; positive = puts richer — hedging demand; negative = calls richer — call speculation)\n",
        opt(s.put_call_volume),
        opt(s.put_call_open_interest),
        opt(s.implied_volatility),
        fmt_iv_skew(s.iv_skew),
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
        Some(prior) => {
            p.push_str(
                "\nCONTINUITY: a prior verdict for this holding exists. Keep the verdict firm; \
                 only move grade/target if the evidence has materially changed, and say what.\n",
            );
            // A band recalibration moves letters with no input change; without this
            // line the model's what-changed would attribute an engine-driven letter
            // move to company evidence or a self-correction (the grade-band slice's
            // versioning finding, `docs/verification/2026-08-03-grade-band-shadow-tune.md` §6).
            // The NOTE names what the boundary changed for this holding, only over
            // a priced prior, and a holding it left unchanged gets none — the same
            // rule as the delta row.
            let boundary = match &prior.disposition {
                VerdictDisposition::Priced(_) => engine::grade_parameter_change(
                    d.prior_grade_parameter_version.as_deref(),
                    grade_branch(prior),
                ),
                _ => None,
            };
            match boundary {
                Some(engine::GradeParameterChange::Letters) => p.push_str(
                    "NOTE: the grade bands were recalibrated since the prior verdict \
                     (grade parameter version changed), so the letter may have moved \
                     with no change in the company's inputs. Attribute such a move in \
                     what_changed to the recalibration — not to company change or a \
                     self-correction.\n",
                ),
                Some(engine::GradeParameterChange::FundMomentum) => p.push_str(
                    "NOTE: the fund momentum read was re-homed to the short price window \
                     since the prior verdict (grade parameter version changed), so the \
                     momentum sub-score may have moved with no change in the fund's \
                     inputs; the letter did not move for that reason. Attribute such a \
                     momentum move in what_changed to the re-homing — not to market \
                     change or a self-correction.\n",
                ),
                Some(engine::GradeParameterChange::FundSectorPeBasis) => p.push_str(
                    "NOTE: the fund sector-P/E source now requires both exchange legs \
                     (grade parameter version changed), so the valuation sub-score and \
                     letter may have moved on the same served rows. Attribute such a move \
                     in what_changed to the exchange-basis correction — not to fund change \
                     or a self-correction.\n",
                ),
                None => {}
            }
            // The scenario-target stamp's NOTE, on the same rule (Codex I11) —
            // the delta row's twin, so the attribution the 6g validator checks
            // has a named cause to resolve to.
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
        input.prior_ledger,
        input.ledger_eval,
        false,
        input.input_delta,
        input.dossier.financials.statement_basis,
        input.dossier.financials.equity_source,
    ));

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
    format!(
        "PRICE VS NAV: {value} ({word}) — the closed-end read; a structural \
         discount or premium, not a transient ETF spread.\n",
    )
}

/// The action prompt's implied-moves block — **both arms, both horizons**
/// (`docs/portfolio-analysis.md` §Portfolio action; the 2026-08-24 review's
/// Codex I5). Each line is bear / base / bull as a percentage move from spot.
/// An engine leg the scenario function could not derive prints `(gap)`. A
/// model leg outside the declared domain (non-finite or non-positive), or one
/// whose move from spot overflows to a non-finite percentage, prints as
/// authored with an `(off-scale as authored)` tag in place of a percentage,
/// and a band authored bear above bull carries `(band inverted as authored)` —
/// the frontend's posture: annotate, never reorder, never drop. I6 owns the
/// upstream domain validation; this is the render's fail-closed read.
fn implied_moves_section(spot: f64, graded: &GradedVerdict) -> String {
    let mv = |v: f64| (v / spot - 1.0) * 100.0;
    let pct = |v: f64| format!("{:+.1}%", mv(v));
    let engine_line = |label: &str, t: Option<&PriceTarget>| match t {
        Some(t) => format!(
            "IMPLIED {label} MOVES vs spot {spot:.2} (engine targets): bear {} / base {} / \
             bull {}.\n",
            pct(t.bear),
            pct(t.base),
            pct(t.bull),
        ),
        None => format!("IMPLIED {label} MOVES vs spot {spot:.2} (engine targets): (gap).\n"),
    };
    let model_leg = |v: f64| {
        if v.is_finite() && v > 0.0 && mv(v).is_finite() {
            pct(v)
        } else {
            format!("{v} (off-scale as authored)")
        }
    };
    let model_line = |label: &str, t: &ModelPriceTarget| {
        format!(
            "IMPLIED {label} MOVES vs spot {spot:.2} (model targets — your own band authored \
             at interpretation on its declared domain, never validated against the engine, no \
             provenance to discount): bear {} / base {} / \
             bull {}{}.\n",
            model_leg(t.bear),
            model_leg(t.base),
            model_leg(t.bull),
            if t.bear > t.bull {
                " (band inverted as authored)"
            } else {
                ""
            },
        )
    };
    let e = &graded.price_targets;
    let m = &graded.model_view.price_targets;
    format!(
        "{}{}{}{}",
        engine_line("1-MONTH", e.one_month.as_ref()),
        engine_line("12-MONTH", e.twelve_month.as_ref()),
        model_line("1-MONTH", &m.one_month),
        model_line("12-MONTH", &m.twelve_month),
    )
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
     outlook, the implied upside/downside of BOTH arms' targets against the \
     current price (the engine's discounted by their stated provenance; the \
     model's are the verdict's own forward call, gated to its declared domain but \
     never validated against the engine, with no provenance to discount), and the \
     capital-efficiency read — \
     only a `fails` hurdle is dead money, and a fails read leans toward \
     realizing some or all of the position once the forward prospects are \
     independently judged poor. Follow the INVESTOR PROFILE's tax posture: for a \
     tax-aware profile, a possible benefit from booking a loss or cost from \
     realizing a gain is a user consideration to FLAG in the rationale, never \
     the mover of the rung; for a tax-exempt profile, apply no tax consideration. \
     For a role/risk-only vehicle (a \
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
    let tax_read = if !input.profile.tax_sensitive {
        "tax-exempt profile — no tax consideration applied"
    } else if pl < 0.0 {
        "an unrealized loss — booking it may carry a tax benefit; flag as a user \
         consideration, never the mover"
    } else if pl > 0.0 {
        "an unrealized gain — realizing it may carry a tax cost; flag as a user \
         consideration, never the mover"
    } else {
        "at break-even — no unrealized gain or loss to flag for tax"
    };
    p.push_str(&format!(
        "Unrealized P/L: ${pl:.0} total ({tax_read})\n"
    ));
    if let Some(prior) = d.prior_verdict.as_ref() {
        if let Some(action) = crate::portfolio::carried_action(prior) {
            match prior.action_source {
                ActionSource::ModelChosen => p.push_str(&format!(
                    "Prior model-chosen action for this holding: {} (continuity baseline — \
                     move only on materially moved evidence).\n",
                    action.as_kebab()
                )),
                ActionSource::RuleDemoted => p.push_str(&format!(
                    "Prior persisted action for this holding: {} (rule-demoted by the \
                     over-age carry rule, not chosen by a model; provenance context only, \
                     not a continuity anchor).\n",
                    action.as_kebab()
                )),
            }
        }
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
                graded.risk_tier.as_str(),
                format!("{:?}", graded.dead_money).to_lowercase(),
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
            // Both arms' implied moves, both horizons (the 2026-08-24 review's
            // Codex I5, ruled 2026-08-28): the action call acts on the model's
            // choices by design (`ModelView`), so its own authored forecast
            // reaches the rung it decides, not only the letter derived from it.
            // No usable spot → no implied line for either arm; the quote floor
            // makes that unreachable on a priced holding, the guard stays
            // defensive.
            if let Some(spot) = d
                .financials
                .current_price
                .filter(|s| s.is_finite() && *s > 0.0)
            {
                p.push_str(&implied_moves_section(spot, graded));
            }
            let t = &engine.target_meta;
            p.push_str(&format!(
                "TARGET PROVENANCE (engine targets): {} — weigh the engine's implied \
                 moves by it; the model's bands carry no provenance to discount.\n",
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
                fmt_expense_ratio(verdict.expense_drag),
                opt(verdict.observable_risk),
                if verdict.structural_flag { "yes" } else { "no" },
            ));
            // The closed-end read, where present — its absence is a named gap in
            // the evidence-gap list below, never a fabricated number.
            if verdict.is_cef {
                if let Some(prem) = verdict.nav_premium {
                    p.push_str(&nav_premium_line(prem));
                }
            }
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
/// `statement_basis` is the holding's stamped basis this run and `equity_source`
/// which balance sheet supplied the two instants' equity — rendered once beside
/// the vocabulary by [`statement_basis_line`].
pub fn ledger_prompt_section(
    prior: Option<&ThesisLedger>,
    eval: Option<&LedgerEvaluation>,
    role_risk: bool,
    input_delta: &[crate::portfolio::DeltaEntry],
    statement_basis: Option<crate::portfolio::StatementBasis>,
    equity_source: Option<crate::portfolio::EquitySource>,
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

    p.push_str("\nENGINE SERIES for quantitative ledger conditions (use exactly these labels):\n");
    for s in engine::LedgerSeries::ALL {
        p.push_str(&format!("- {}: {}\n", s.as_kebab(), s.describe()));
    }
    p.push_str(&statement_basis_line(statement_basis, equity_source));

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
                    let support = if research_supported.contains(c.condition_id.as_str()) {
                        " — RESEARCH-SUPPORTED THIS RUN: a fresh source-backed finding in the \
                         INPUT DELTA bears on this condition"
                    } else {
                        ""
                    };
                    p.push_str(&format!("- {family}[{kind}] {}{support}\n", c.statement));
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
         crossing for that same condition, or — for a qualitative condition — where the \
         ledger above marks it RESEARCH-SUPPORTED THIS RUN and the cited finding actually \
         evidences the trip; a qualitative claim with no such fresh finding is \
         unsupported. Unsupported claims are cleared by the app. \
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
) -> String {
    let kebabs = |keep: fn(&engine::LedgerSeries) -> bool| {
        engine::LedgerSeries::ALL
            .iter()
            .filter(|s| keep(s))
            .map(|s| s.as_kebab())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let flow = kebabs(|s| s.flow_basis());
    let instants = kebabs(|s| s.statement_derived() && !s.flow_basis());
    let flow_line = match basis {
        Some(b) => format!(
            "The flow series ({flow}) are read on this holding's statement basis this \
             run: {}. Author their thresholds on that basis.",
            b.label()
        ),
        None => format!(
            "The flow series ({flow}) have no statement basis this run — no flow lines \
             reached the engine — so they are unevaluable here."
        ),
    };
    let instants_line = match equity_source {
        Some(src) => format!(
            "The balance-sheet series ({instants}) read the latest balance sheet — an \
             instant on no flow basis — supplied this run by {}.",
            src.label()
        ),
        None => format!(
            "The balance-sheet series ({instants}) have no balance sheet this run — no \
             equity line reached the engine — so they are unevaluable here."
        ),
    };
    format!("{flow_line} {instants_line}\n")
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
            price_target_rationale: "Base case follows the engine's scenario midpoint.".to_string(),
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
    /// Prompt-size observations accumulated since the job's last drain
    /// ([`HoldingAnalyst::take_prompt_usage`] — once per holding checkpoint).
    /// A `Mutex` only for the `&self` receivers — the per-holding loop is
    /// sequential, so it is never contended.
    prompt_usage: std::sync::Mutex<Vec<crate::local_model::PromptUsage>>,
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
            client,
            reasoner_model,
            fast_model,
            model_calls: std::sync::Mutex::new(Vec::new()),
            prompt_usage: std::sync::Mutex::new(Vec::new()),
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
fn decode_interpretation(stage: &str, content: &str) -> Result<Interpretation> {
    let interpretation: Interpretation = serde_json::from_str(content)
        .map_err(|e| anyhow::Error::new(e).context(crate::local_model::RetryClass::SchemaParse))
        .with_context(|| format!("parsing interpretation JSON: {}", body_snippet(content)))?;
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
    prompt: String,
    schema: &serde_json::Value,
) -> ChatRequest {
    let mut req = ChatRequest::new(model, vec![ChatMessage::user(prompt)]);
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

/// Build one research-loop turn's request: thinking on, the web tools and the
/// findings grammar riding the same call (verified clean together on the
/// pinned Ollama — `docs/local-model-operations.md` §Structured output ×
/// thinking), the shared interpret context (one `num_ctx` per model).
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
        // The model seam: one thinking turn, tools + the findings grammar on
        // the same call.
        struct TurnAdapter<'a> {
            analyst: &'a LocalAnalyst,
            stage: &'a str,
        }
        impl research::ResearchModel for TurnAdapter<'_> {
            fn research_turn(
                &self,
                messages: &[ChatMessage],
                tools: Option<&serde_json::Value>,
                format: Option<&serde_json::Value>,
            ) -> Result<crate::local_model::ChatResponse> {
                let req = research_turn_request(
                    &self.analyst.reasoner_model,
                    messages.to_vec(),
                    tools,
                    format,
                );
                self.analyst.record_model_call(&req);
                let resp = self.analyst.client.chat(&req)?;
                self.analyst.record_usage(self.stage.to_string(), &req, &resp);
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
                prompt: String,
                schema: &serde_json::Value,
            ) -> Result<String> {
                // The issue guard: size the rendered prompt against its model's
                // budget before any request exists (`distill_route`).
                let (model, num_ctx) = distill_route(
                    stage,
                    prompt.chars().count(),
                    &self.analyst.fast_model,
                    &self.analyst.reasoner_model,
                )?;
                let req = distill_request(
                    model,
                    num_ctx,
                    NUM_PREDICT_DISTILL,
                    prompt.clone(),
                    schema,
                );
                self.analyst.record_model_call(&req);
                let resp = self.analyst.client.chat(&req)?;
                self.analyst.record_usage(stage.to_string(), &req, &resp);
                if hit_normal_distill_reservation(&req, &resp) {
                    // The rendered prompt already passed the reasoner's 60%
                    // input guard, leaving more than the 32 K expanded ceiling
                    // in its 128 K context. Route the one evidence-triggered
                    // re-attempt there even when the normal call used a 32 K
                    // fast tier, whose shared context could not hold both.
                    self.spent_output_retries
                        .borrow_mut()
                        .insert(stage.to_string());
                    let expanded_req = distill_request(
                        &self.analyst.reasoner_model,
                        NUM_CTX_INTERPRET,
                        NUM_PREDICT_DISTILL_RETRY,
                        prompt,
                        schema,
                    );
                    self.analyst.record_model_call(&expanded_req);
                    let expanded_resp = self.analyst.client.chat(&expanded_req)?;
                    self.analyst.record_usage(stage.to_string(), &expanded_req, &expanded_resp);
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
        let req = interpret_request(&self.reasoner_model, input);
        // Stream step-scoped: the structured body has no console value (it stays
        // accumulated, never streamed), but the reasoning streams onto this
        // holding's own "Analyze {SYM}" step, so the tracker shows live thinking
        // instead of a minutes-long quiet stretch (the first live run's F8).
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let stage = format!("interpret {}", input.dossier.position.symbol);
        self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self.client.chat_streaming(&req, StreamRole::Step(&step_key))?;
            self.record_usage(stage.clone(), &req, &resp);
            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            decode_interpretation(&stage, &resp.content)
        })
    }

    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
        let req = role_risk_request(&self.reasoner_model, input);
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let stage = format!("role-risk {}", input.dossier.position.symbol);
        self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self.client.chat_streaming(&req, StreamRole::Step(&step_key))?;
            self.record_usage(stage.clone(), &req, &resp);
            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            serde_json::from_str(&resp.content)
                .map_err(|e| {
                    anyhow::Error::new(e).context(crate::local_model::RetryClass::SchemaParse)
                })
                .with_context(|| {
                    format!(
                        "parsing role/risk interpretation JSON: {}",
                        body_snippet(&resp.content)
                    )
                })
        })
    }

    fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision> {
        let req = action_request(&self.reasoner_model, input);
        // Stream step-scoped like interpretation: the decision's reasoning lands
        // on this holding's own "Analyze {SYM}" step.
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let stage = format!("action {}", input.dossier.position.symbol);
        self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self.client.chat_streaming(&req, StreamRole::Step(&step_key))?;
            self.record_usage(stage.clone(), &req, &resp);
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
        std::mem::take(
            &mut *self
                .prompt_usage
                .lock()
                .expect("prompt-usage lock is never poisoned"),
        )
    }

    fn take_retry_events(&self) -> Vec<crate::local_model::RetryEvent> {
        self.retry.take_events()
    }
}

#[cfg(test)]
mod tests {
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
            claim: text.into(),
            source_url: format!("https://x.example/{}", text.len()),
            vintage: "2026-08-26T00:00:00+00:00".into(),
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
                .contains("— bears on ledger condition 'Trailing return collapses'"),
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
            tool_calls: None,
        };
        analyst.record_usage("construction".to_string(), &req, &counted);
        let uncounted = crate::local_model::ChatResponse {
            content: String::new(),
            thinking: None,
            prompt_eval_count: None,
            eval_count: None,
            done_reason: Some("length".into()),
            tool_calls: None,
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
            "wide prompt".into(),
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

    fn dossier(asset_class: AssetClass, financials: CompanyFinancials) -> HoldingDossier {
        HoldingDossier {
            prior_metrics: None,
            semantic_recall: Default::default(),
            news_seeds: Vec::new(),
            research_priors: Vec::new(),
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
            .find(|q| q.starts_with("Backfill obligation:"))
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
                assert_eq!(g.what_changed, "new holding");
                // The model's base-case justification is carried through, not dropped.
                assert!(!g.price_target_rationale.is_empty());
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
            statement: "price below 700".into(),
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
            statement: "price below 700".into(),
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
                statement: "price below 700".into(),
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
                statement: format!("price below {threshold}"),
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
            prior_ledger: d.prior_ledger(),
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
            prior_ledger: d.prior_ledger(),
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
            prior_ledger: bare.prior_ledger(),
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
            prior_ledger: d.prior_ledger(),
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
            prior_ledger: fd.prior_ledger(),
            readout: &RoleRiskReadout::default(),
            ledger_eval: None,
            distilled: "No research findings.",
        });
        assert!(role.contains("MARKET OPTIONS SENTIMENT"), "{role}");
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
            prior_ledger: d.prior_ledger(),
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
            prior_ledger: unknown.prior_ledger(),
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
        // Both arms' targets reach the rung, both horizons (Codex I5): the
        // engine's lines under their provenance, the model's own band beside
        // them — the stub authors its twelve-month base at 1.05× the engine's,
        // so the two base moves must differ on the page.
        let spot = d.financials.current_price.unwrap();
        let pct = |v: f64| format!("{:+.1}%", (v / spot - 1.0) * 100.0);
        let engine_12 = graded.price_targets.twelve_month.as_ref().unwrap();
        let model_12 = &graded.model_view.price_targets.twelve_month;
        assert!(
            user.contains(&format!(
                "IMPLIED 12-MONTH MOVES vs spot {spot:.2} (engine targets): bear {} / base {} \
                 / bull {}.",
                pct(engine_12.bear),
                pct(engine_12.base),
                pct(engine_12.bull)
            )),
            "{user}"
        );
        assert!(
            user.contains(&format!(
                "IMPLIED 12-MONTH MOVES vs spot {spot:.2} (model targets — your own band \
                 authored at interpretation on its declared domain, never validated against \
                 the engine, no provenance to discount): bear {} / base {} / bull {}.",
                pct(model_12.bear),
                pct(model_12.base),
                pct(model_12.bull)
            )),
            "{user}"
        );
        assert_ne!(pct(engine_12.base), pct(model_12.base));
        assert_eq!(user.matches("IMPLIED 1-MONTH MOVES vs spot").count(), 2, "{user}");
        assert_eq!(user.matches("IMPLIED 12-MONTH MOVES vs spot").count(), 2, "{user}");
        assert!(user.contains("TARGET PROVENANCE (engine targets):"), "{user}");
        assert!(user.contains("weigh the engine's implied moves by it"), "{user}");
        assert!(!user.contains("off-scale as authored"), "{user}");
        assert!(!user.contains("band inverted as authored"), "{user}");
        let system = action_system_prompt();
        assert!(system.contains("BOTH arms' targets"), "{system}");
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
        let render = |d: &HoldingDossier| {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced {
                    graded,
                    engine: &engine_output,
                    pre_profit: None,
                },
                engine_set: &engine_set,
                profile: &d.profile,
            })
        };

        let exempt = render(&d);
        assert!(
            exempt.contains(
                "Prior persisted action for this holding: hold (rule-demoted by the over-age \
                 carry rule, not chosen by a model; provenance context only, not a continuity \
                 anchor)."
            ),
            "{exempt}"
        );
        assert!(!exempt.contains("Prior model-chosen action"), "{exempt}");
        assert!(
            exempt.contains("tax-exempt profile — no tax consideration applied"),
            "{exempt}"
        );
        assert!(!exempt.contains("tax benefit"), "{exempt}");
        assert!(!exempt.contains("tax cost"), "{exempt}");

        d.profile.tax_sensitive = true;
        let taxable = render(&d);
        assert!(taxable.contains("may carry a tax cost"), "{taxable}");
        let system = action_system_prompt();
        assert!(system.contains("Follow the INVESTOR PROFILE's tax posture"), "{system}");
        assert!(system.contains("for a tax-exempt profile, apply no tax consideration"), "{system}");
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
    fn action_prompt_tags_off_domain_model_legs_and_gaps_engine_legs_as_authored() {
        // The render annotates, never reorders or drops (Codex I5, ruled
        // 2026-08-28): an engine leg the scenario function could not derive
        // prints `(gap)`; a model leg outside the declared domain prints as
        // authored with its tag in place of a percentage; a band authored bear
        // above bull carries the inverted tag — the frontend's posture. I6 owns
        // the upstream domain validation; this is the render's fail-closed read.
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
        let mut g = graded.clone();
        g.price_targets.one_month = None;
        g.model_view.price_targets.one_month = ModelPriceTarget {
            base: 100.0,
            bear: 120.0,
            bull: 90.0,
        };
        g.model_view.price_targets.twelve_month = ModelPriceTarget {
            base: f64::NAN,
            bear: -5.0,
            bull: 0.0,
        };
        let render = |d: &HoldingDossier| {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced {
                    graded: &g,
                    engine: &engine_output,
                    pre_profit: None,
                },
                engine_set: &engine_set,
                profile: &d.profile,
            })
        };
        let user = render(&d);
        let spot = d.financials.current_price.unwrap();
        let pct = |v: f64| format!("{:+.1}%", (v / spot - 1.0) * 100.0);
        assert!(
            user.contains(&format!(
                "IMPLIED 1-MONTH MOVES vs spot {spot:.2} (engine targets): (gap)."
            )),
            "{user}"
        );
        // The engine's twelve-month leg is untouched and still renders as moves.
        let engine_12 = g.price_targets.twelve_month.as_ref().unwrap();
        assert!(
            user.contains(&format!(
                "IMPLIED 12-MONTH MOVES vs spot {spot:.2} (engine targets): bear {}",
                pct(engine_12.bear)
            )),
            "{user}"
        );
        // Inverted: authored numbers, authored order, the tag beside them.
        assert!(
            user.contains(&format!(
                "bear {} / base {} / bull {} (band inverted as authored).",
                pct(120.0),
                pct(100.0),
                pct(90.0)
            )),
            "{user}"
        );
        // Off-scale: the raw authored value with its tag, no percentage.
        assert!(
            user.contains(
                "bear -5 (off-scale as authored) / base NaN (off-scale as authored) / bull 0 \
                 (off-scale as authored)."
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
        both.model_view.price_targets.one_month = ModelPriceTarget {
            base: 100.0,
            bear: f64::NAN,
            bull: 90.0,
        };
        both.model_view.price_targets.twelve_month = ModelPriceTarget {
            base: 100.0,
            bear: 120.0,
            bull: -5.0,
        };
        let user3 = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced {
                graded: &both,
                engine: &engine_output,
                pre_profit: None,
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(
            user3.contains(&format!(
                "bear NaN (off-scale as authored) / base {} / bull {}.",
                pct(100.0),
                pct(90.0)
            )),
            "{user3}"
        );
        assert!(
            user3.contains(&format!(
                "bear {} / base {} / bull -5 (off-scale as authored) (band inverted as authored).",
                pct(120.0),
                pct(100.0)
            )),
            "{user3}"
        );
        assert_eq!(user3.matches("band inverted as authored").count(), 1, "{user3}");
        // A finite, positive leg whose move from spot overflows the percentage
        // arithmetic is off-scale too — the guard reads the derived move, so the
        // prompt never carries `inf%` (Codex round 1).
        let mut penny = strong_financials();
        penny.current_price = Some(1.0);
        let d4 = dossier(AssetClass::Stock, penny);
        let mut huge = g.clone();
        huge.model_view.price_targets.twelve_month = ModelPriceTarget {
            base: 1e308,
            bear: 0.5,
            bull: 2.0,
        };
        let user4 = action_user_prompt(&ActionInput {
            dossier: &d4,
            subject: ActionSubject::Priced {
                graded: &huge,
                engine: &engine_output,
                pre_profit: None,
            },
            engine_set: &engine_set,
            profile: &d4.profile,
        });
        assert!(!user4.contains("inf"), "{user4}");
        assert!(
            user4.contains("bear -50.0% / base 1")
                && user4.contains("0 (off-scale as authored) / bull +100.0%."),
            "{user4}"
        );
        // No usable spot → no implied line for either arm; the provenance line
        // still renders, since it describes the targets rather than the moves.
        let mut unpriced = strong_financials();
        unpriced.current_price = None;
        let d2 = dossier(AssetClass::Stock, unpriced);
        let user2 = render(&d2);
        assert!(!user2.contains("IMPLIED "), "{user2}");
        assert!(user2.contains("TARGET PROVENANCE (engine targets):"), "{user2}");
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
        // moves it to v29.
        assert_eq!(PROMPT_VERSION, "portfolio-v29");
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
    fn interpretation_prompt_names_the_model_arm_domain_as_enforced() {
        // The prompt states the scale as a gate, not a preference (ruled
        // 2026-08-29), and keeps ordering the model's own.
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
        assert!(user.contains("a score outside 0-100 is rejected, never clamped"), "{user}");
        assert!(user.contains("a zero or negative leg is rejected"), "{user}");
        assert!(user.contains("an inverted band is kept as authored and annotated"), "{user}");
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
        assert!(decode_interpretation("interpret TEST", &clean).is_ok());

        let mut off = serde_json::to_value(&stub).unwrap();
        off["model_sub_scores"]["quality"] = serde_json::json!(10000.0);
        off["model_sub_scores"]["risk"] = serde_json::json!(-1.0);
        off["model_price_targets"]["twelve_month"]["bear"] = serde_json::json!(0.0);
        off["model_price_targets"]["one_month"]["bull"] = serde_json::json!(-5.0);
        let err = decode_interpretation("interpret TEST", &off.to_string()).unwrap_err();
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
        assert!(decode_interpretation("interpret TEST", &inverted.to_string()).is_ok());

        // Malformed content keeps its own class.
        let err = decode_interpretation("interpret TEST", "not json").unwrap_err();
        assert_eq!(
            crate::local_model::retry_class(&err),
            Some(crate::local_model::RetryClass::SchemaParse)
        );
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
            prior_ledger: d.prior_ledger(),
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
            demoted.contains("prior MODEL arm (authored read; action later rule-demoted)"),
            "{demoted}"
        );
        assert!(
            demoted.contains(
                "persisted action hold (rule-demoted after authoring; the prior model-chosen \
                 rung is unavailable in this record)"
            ),
            "{demoted}"
        );
        assert!(!demoted.contains("prior MODEL arm (yours)"), "{demoted}");

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
            prior_ledger: d.prior_ledger(),
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
            prior_ledger: d.prior_ledger(),
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
            prior_ledger: d.prior_ledger(),
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
            prior_ledger: d.prior_ledger(),
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

        // No prior verdict: new holding, no recalibration note.
        assert!(!prompt(&d).contains("recalibrated"), "no prior verdict");

        // Prior verdict stamped across the v2 retune: the note fires.
        d.prior_verdict = Some(prior);
        d.prior_grade_parameter_version = Some("grade-v2".into());
        let p = prompt(&d);
        assert!(p.contains("recalibrated"), "{p}");
        assert!(p.contains("what_changed"), "{p}");

        // Prior verdict stamped with the current bands: no note.
        d.prior_grade_parameter_version = Some(engine::GRADE_PARAMETER_VERSION.to_string());
        assert!(!prompt(&d).contains("recalibrated"), "same-version prior");

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
        assert!(!prompt(&d).contains("recalibrated"), "not-rated prior");
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
                    e.label.contains("recalibrated")
                        || e.label.contains("re-homed")
                        || e.label.contains("exchange basis tightened")
                })
                .map(|e| e.label.clone())
                .collect()
        };
        let silent = |d: &HoldingDossier, case: &str| {
            let p = prompt(d);
            assert!(
                !p.contains("recalibrated") && !p.contains("re-homed"),
                "{case}: {p}"
            );
            let rows = delta(d);
            assert!(boundary_rows(&rows).is_empty(), "{case}: {rows:?}");
        };
        let recalibrated = |d: &HoldingDossier, case: &str| {
            let p = prompt(d);
            assert!(p.contains("grade bands were recalibrated"), "{case}: {p}");
            assert!(!p.contains("re-homed"), "{case}: {p}");
            let rows = boundary_rows(&delta(d));
            assert_eq!(rows.len(), 1, "{case}: {rows:?}");
            assert!(
                rows[0].starts_with("grade bands recalibrated ("),
                "{case}: {rows:?}"
            );
        };
        let exchange_basis = |d: &HoldingDossier, case: &str| {
            let p = prompt(d);
            assert!(
                p.contains("sector-P/E source now requires both exchange legs"),
                "{case}: {p}"
            );
            assert!(!p.contains("re-homed"), "{case}: {p}");
            let rows = boundary_rows(&delta(d));
            assert_eq!(rows.len(), 1, "{case}: {rows:?}");
            assert!(
                rows[0].starts_with("fund sector-P/E exchange basis tightened ("),
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
        assert!(rows[0].contains("(grade-v2.1 -> "), "{rows:?}");
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
                p.contains("one-month and twelve-month targets may have moved"),
                "{case}: {p}"
            );
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
        let row = target_boundary_row("targets-v4", engine::TargetHorizons::ONE_MONTH);
        assert_eq!(
            row,
            format!(
                "scenario-target parameters changed (targets-v4 -> {}) — the one-month \
                 target can move with no input change",
                engine::SCENARIO_TARGET_PARAMETER_VERSION
            )
        );
        let note = target_boundary_note(engine::TargetHorizons::BOTH);
        assert!(
            note.starts_with(
                "NOTE: the scenario-target function's parameters changed since the prior \
                 verdict (target parameter version changed), so the one-month and \
                 twelve-month targets may have moved with no change in the company's inputs."
            ),
            "{note}"
        );
        assert!(
            note.contains("Attribute such a target move in what_changed to the parameter change")
                && note.ends_with('\n')
                && !note.contains("  "),
            "{note}"
        );
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
            prior_ledger: d.prior_ledger(),
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
        assert!(role.contains("never by itself a reason to exit"), "{role}");
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

        let overlay = prompt_for(Some(FundStructuralKind::OptionOverlay));
        assert!(
            overlay.contains("STRUCTURAL FLAG: option-overlay path dependency"),
            "{overlay}"
        );
        assert!(!overlay.contains("leveraged/inverse"), "{overlay}");

        let daily_reset = prompt_for(Some(FundStructuralKind::LeveragedInverse));
        assert!(
            daily_reset.contains("STRUCTURAL FLAG: leveraged/inverse daily-reset"),
            "{daily_reset}"
        );
        assert!(!daily_reset.contains("option-overlay"), "{daily_reset}");
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
            evidence_gaps: vec!["price-vs-NAV unavailable — no NAV".into()],
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
        let action = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::RoleRisk { verdict: &rr },
            engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
            profile: &d.profile,
        });
        assert!(action.contains("PRICE VS NAV: -7.2% (discount)"), "{action}");
        rr.nav_premium = None;
        let action_gap = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::RoleRisk { verdict: &rr },
            engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
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
            labels.contains(&"engine sub-score quality: 61.7 -> 62.3"),
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
        const CONVENTION: &str = "(chain-wide mean put IV minus mean call IV, in IV's decimal \
                                  unit; positive = puts richer — hedging demand; negative = \
                                  calls richer — call speculation)\n";

        // The fixture dossier carries `iv_skew: Some(0.03)`.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let positive = prompt(&d);
        assert!(
            positive.contains(&format!(
                "put/call vol 1.200, put/call OI 1.100, IV 0.300, IV skew +0.030 {CONVENTION}"
            )),
            "{positive}"
        );

        d.options_signal.iv_skew = Some(-0.02);
        let negative = prompt(&d);
        assert!(
            negative.contains(&format!("IV 0.300, IV skew -0.020 {CONVENTION}")),
            "{negative}"
        );

        d.options_signal.iv_skew = None;
        let gap = prompt(&d);
        assert!(
            gap.contains(&format!("IV 0.300, IV skew (gap) {CONVENTION}")),
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
        let section = ledger_prompt_section(Some(&prior), Some(&eval), false, &[], None, None);
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
    fn the_priced_fund_prompt_renders_the_guards_us_share_and_both_horizons_methodology() {
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
        assert!(us.contains("US share: 97%."), "{us}");
        let summed = prompt_for(vec![
            ("United States".into(), 0.5),
            ("USA".into(), 0.2),
            ("U.S.".into(), 0.1),
            ("Canada".into(), 0.2),
        ]);
        assert!(summed.contains("US share: 80%."), "{summed}");
        let capped = prompt_for(vec![
            ("United States of America".into(), 0.8),
            ("us".into(), 0.5),
        ]);
        assert!(capped.contains("US share: 100%."), "{capped}");
        let gap = prompt_for(vec![]);
        assert!(gap.contains("US share: (gap)."), "{gap}");
        // Both horizons' engine targets carry their methodology (Codex I10):
        // the one-month line names its basis like the twelve-month one.
        assert!(
            us.contains("ENGINE SCENARIO TARGETS (baseline arm; twelve-month rolling): bear"),
            "{us}"
        );
        let one_month = us
            .find("ENGINE ONE-MONTH TARGETS: bear")
            .unwrap_or_else(|| panic!("{us}"));
        let tail = &us[one_month..];
        let line_end = tail.find('\n').unwrap();
        assert!(
            tail[line_end..].starts_with("\n  methodology: One-month (rolling) base = spot"),
            "{tail}"
        );
        assert!(tail.contains(engine::SCENARIO_TARGET_PARAMETER_VERSION), "{tail}");
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
        assert!(
            role.contains(
                "EXPENSE RATIO (decimal fraction of assets per year; 0.0075 = 0.75%/yr): \
                 0.0003 (0.03%/yr)\n"
            ),
            "{role}"
        );

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
            interp.contains("0.0075 = 0.75%/yr): 0.0003 (0.03%/yr). US share: 99%."),
            "{interp}"
        );

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
        let action = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::RoleRisk { verdict: &rr },
            engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
            profile: &d.profile,
        });
        assert!(
            action.contains(
                "EXPENSE DRAG (decimal fraction of assets per year): 0.0003 (0.03%/yr). \
                 OBSERVABLE"
            ),
            "{action}"
        );
        for (name, p) in [
            ("role", &role),
            ("interpretation", &interp),
            ("action", &action),
        ] {
            assert!(
                !p.contains("): 0.000\n") && !p.contains("): 0.000."),
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
        assert!(role.contains("UNDERLYING POSITIONING"), "{role}");
        assert!(role.contains("snapshot as of 2026-08-11"), "{role}");
        assert!(role.contains("Gold — speculator net +200000"), "{role}");
        assert!(role.contains("never a score input"), "{role}");

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
        // turn thinks with tools + the findings grammar on the same call; every
        // stage pins an explicit `num_ctx` (never the daemon auto-size), its
        // mode's sampling row, and stay-resident `keep_alive`.
        let d = dossier(AssetClass::Stock, strong_financials());

        let schema = serde_json::json!({"type": "object"});
        let distill = distill_request(
            "fast-model",
            NUM_CTX_DISTILL,
            NUM_PREDICT_DISTILL,
            "prompt".into(),
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

        let tools = crate::portfolio::research::research_tools();
        let turn = research_turn_request(
            "reasoner-model",
            vec![ChatMessage::user("brief")],
            Some(&tools),
            Some(&schema),
        );
        assert_eq!(turn.think, Some(true));
        assert_eq!(turn.keep_alive, Some(-1));
        assert!(turn.tools.is_some(), "the web tools ride the turn");
        assert!(turn.format_schema.is_some(), "findings grammar rides the same call");
        let opts = turn.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET, "one num_ctx per model");
        assert_eq!(opts["temperature"], 1.0, "thinking-general row");

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
            "prompt".into(),
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
            "prompt".into(),
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
        assert!(!msg.contains("truncated at the output reservation"), "{msg}");

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
                    material_forward_fact: false,
                    seeded_by: Vec::new(),
                    topic_answered: true,
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
            role_risk: false,
            overlay_eligible: false,
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
                falsifier("price above 500", "price", 500.0),
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
            confidence: 0.9,
        };
        let block = render_forensic_advisory(&claim);
        assert!(block.contains("advisory attention evidence"), "{block}");
        assert!(block.contains("NOT a hard trigger"), "{block}");
        assert!(block.contains("attribution to this issuer is unconfirmed"), "{block}");
        assert!(block.contains("https://www.sec.gov/litigation/acme"), "{block}");
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
        let s = ledger_prompt_section(None, None, false, &[], None, None);
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
        let s = ledger_prompt_section(Some(&prior), Some(&eval), false, &[], None, None);
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
        let rr = ledger_prompt_section(Some(&prior), None, true, &[], None, None);
        assert!(rr.contains("trim/sell only"), "{rr}");

        // The research-supported mark (2026-08-24 review F3): a fresh research
        // entry tied to a condition marks that row — by statement, the id held
        // out — and the rewrite instruction names the mark as the qualitative
        // leg; without a tied entry no row is marked and the retired
        // "none are available this run" sentence is gone for good.
        assert!(!s.contains("RESEARCH-SUPPORTED THIS RUN:"), "{s}");
        assert!(!s.contains("none are available this run"), "{s}");
        assert!(s.contains("marks it RESEARCH-SUPPORTED THIS RUN"), "{s}");
        let tied = vec![crate::portfolio::DeltaEntry {
            id: "research-1".into(),
            label: "research finding (t): a claim [https://x.example/a]".into(),
            related_condition_id: Some("keep-1".into()),
        }];
        let marked = ledger_prompt_section(Some(&prior), Some(&eval), false, &tied, None, None);
        assert!(
            marked.contains("RESEARCH-SUPPORTED THIS RUN: a fresh source-backed finding"),
            "{marked}"
        );
        assert!(!marked.contains("keep-1"), "condition ids stay out of the prompt: {marked}");
        let rr_marked = ledger_prompt_section(Some(&prior), None, true, &tied, None, None);
        assert!(rr_marked.contains("RESEARCH-SUPPORTED THIS RUN"), "{rr_marked}");

        // Both interpretation prompts carry the section.
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
        assert!(user.contains("REWRITE THE THESIS LEDGER"), "{user}");
        assert!(interpretation_system_prompt().contains("THESIS LEDGER"));
        assert!(role_risk_system_prompt().contains("THESIS LEDGER"));
    }

    /// The 2026-08-24 large-scale review's Priority-1 minor: the vocabulary said
    /// "TTM net margin" while an annual-fallback holding's thresholds were
    /// evaluated against annual prints and no prompt said so. The labels now name
    /// no basis and the section states the holding's basis once, beside them.
    #[test]
    fn ledger_section_states_the_statement_basis() {
        use crate::portfolio::StatementBasis;
        // The flow family and the instants pinned by name, so a production drift to
        // a hand-written list or a predicate change shows here, not only in the gate
        // (Codex round 1: the gate's whole family is not basis-homogeneous).
        let flow = "net-margin, gross-margin, revenue-growth, pe-ratio, ps-ratio";
        let instants = "debt-to-equity, pb-ratio";

        use crate::portfolio::EquitySource;
        let ttm = ledger_prompt_section(
            None,
            None,
            false,
            &[],
            Some(StatementBasis::Ttm),
            Some(EquitySource::FmpQuarterly),
        );
        assert!(
            ttm.contains("- net-margin: net margin (decimal)\n"),
            "{ttm}"
        );
        assert!(
            ttm.contains("- gross-margin: gross margin (decimal)\n"),
            "{ttm}"
        );
        assert!(
            !ttm.contains("TTM net margin") && !ttm.contains("TTM gross margin"),
            "{ttm}"
        );
        assert!(
            ttm.contains(&format!("The flow series ({flow}) are read on")),
            "{ttm}"
        );
        // Codex I13 (`portfolio-v23`): the instants' sentence names which balance
        // sheet supplied their equity — the source their evaluation gates on.
        assert!(
            ttm.contains(&format!(
                "The balance-sheet series ({instants}) read the latest balance sheet — \
                 an instant on no flow basis — supplied this run by FMP's latest \
                 quarterly balance sheet.\n"
            )),
            "{ttm}"
        );
        assert!(
            ttm.contains("statement basis this run: TTM (four trailing quarters)"),
            "{ttm}"
        );
        assert!(
            ttm.contains("Author their thresholds on that basis"),
            "{ttm}"
        );

        let annual = ledger_prompt_section(
            None,
            None,
            false,
            &[],
            Some(StatementBasis::Annual),
            Some(EquitySource::SecAnnual),
        );
        assert!(
            annual.contains("statement basis this run: SEC annual (latest full year"),
            "{annual}"
        );
        assert!(!annual.contains("TTM (four trailing quarters)"), "{annual}");
        assert!(
            annual.contains(
                "supplied this run by SEC's latest annual stockholders' equity (the \
                 quarterly balance-sheet leg fell back).\n"
            ) && !annual.contains("FMP's latest quarterly balance sheet"),
            "{annual}"
        );

        // No statement lines and no equity: each sentence says so rather than
        // naming a basis or a source.
        let none = ledger_prompt_section(None, None, false, &[], None, None);
        assert!(none.contains("no statement basis this run"), "{none}");
        assert!(none.contains("so they are unevaluable here"), "{none}");
        assert!(
            none.contains(&format!(
                "The balance-sheet series ({instants}) have no balance sheet this run — \
                 no equity line reached the engine — so they are unevaluable here.\n"
            )),
            "{none}"
        );
        assert!(!none.contains("statement basis this run:"), "{none}");
        assert!(!none.contains("supplied this run by"), "{none}");

        // A balance-sheet instant standing alone (FMP's own beside thin quarters):
        // no flow basis, but the instants still read — and name their source.
        let instant_only =
            ledger_prompt_section(None, None, false, &[], None, Some(EquitySource::FmpQuarterly));
        assert!(instant_only.contains("no statement basis this run"), "{instant_only}");
        assert!(
            instant_only.contains("supplied this run by FMP's latest quarterly balance sheet."),
            "{instant_only}"
        );
        assert!(!instant_only.contains("have no balance sheet this run"), "{instant_only}");

        // The role/risk branch carries the same line (a fund reads `None`).
        let rr = ledger_prompt_section(None, None, true, &[], None, None);
        assert!(rr.contains("no statement basis this run"), "{rr}");

        // The interpretation prompt reads the dossier's stamped basis and source.
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
        assert!(
            user.contains("supplied this run by SEC's latest annual stockholders' equity"),
            "{user}"
        );
        assert!(
            user.contains("statement basis this run: SEC annual"),
            "{user}"
        );
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
            prior_ledger: d.prior_ledger(),
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
            prior_ledger: d.prior_ledger(),
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

}
