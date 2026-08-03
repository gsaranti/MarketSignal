//! The per-holding pipeline (`docs/portfolio-analysis.md` §The per-holding pipeline).
//! Orchestrates one holding from its deterministic dossier through the engine to a
//! schema-valid verdict: eligibility → financial engine → bounded research → distill
//! → interpret + grade → continuity. Every *number* is the engine's; the model
//! authors only the judgment calls and prose ([`crate::portfolio::Interpretation`]).
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
use crate::portfolio::{
    interpretation_schema, role_risk_interpretation_schema, Action, ClosedCondition,
    ConditionEvalState, ConditionRole, Conviction, CrossingOutcome, ExposureWeight,
    FalsifierDraft, GradedVerdict, HoldingAudit, HoldingVerdict, HorizonOutlook, HorizonRead,
    Interpretation, KeyDriver, KeyDriverDraft, LedgerAudit, LedgerBranch, LedgerCondition,
    LedgerComparator, LedgerDraft, MonitorScenario, PositionChange, PositionDelta, PriceTarget,
    QuantCore, QuantCoreDraft, RoleRiskInterpretation, RoleRiskVerdict, ScenarioDraft,
    ScenarioKind, ThesisLedger, TriggerDraft, TriggerFamily, VerdictDisposition, HORIZON_LONG,
    HORIZON_MID, HORIZON_SHORT, PROMPT_VERSION, ROLE_RISK_ACTIONS,
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
/// the distilled research findings, and the **engine-bounded feasible action set**
/// the model must choose within (`docs/portfolio-analysis.md` §Starting parameters).
/// The model reasons over *this* — evidence, not a gathering transcript.
pub struct InterpretationInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub engine: &'a EngineOutput,
    pub distilled: &'a str,
    pub feasible: &'a [Action],
    /// The engine's evaluation of the prior thesis ledger's quantitative conditions
    /// (`None` on a debut — no prior ledger to evaluate).
    pub ledger_eval: Option<&'a LedgerEvaluation>,
}

/// What the `role_risk_only` interpretation reads: the dossier plus the engine's
/// typed readout — none of the priced machinery exists on this branch.
pub struct RoleRiskInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub readout: &'a RoleRiskReadout,
    /// The engine's evaluation of the prior fund ledger's quantitative conditions.
    pub ledger_eval: Option<&'a LedgerEvaluation>,
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
    /// role read and an action from the reduced spine
    /// (`docs/portfolio-analysis.md` §Intrinsic verdict).
    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation>;
    /// The model ids this analyst used, for the run's audit record.
    fn model_ids(&self) -> Vec<String>;
}

/// Run one holding through the pipeline end to end, returning its verdict and audit
/// record. Eligibility and the evidence floor short-circuit before any model call —
/// an ineligible asset class is `not-rated`, a holding below the floor is
/// `insufficient-evidence` — so the model is only ever asked to interpret a holding
/// the engine could actually grade. `account_total` sizes the action against the
/// portfolio.
pub fn analyze_holding(
    analyst: &dyn HoldingAnalyst,
    dossier: &HoldingDossier,
    account_total: f64,
    rates: &RateAnchors,
    run_date: &str,
) -> Result<(HoldingVerdict, HoldingAudit)> {
    let symbol = dossier.position.symbol.clone();
    let asset_class = dossier.position.asset_class;
    // App-set from the deterministic holdings diff, never the model — carried on every
    // verdict (graded or not) as the structured what-changed position tag.
    let position_change = dossier.position_delta.change;
    // The prior run's thesis ledger (rides the prior verdict) — the standing view
    // this run tests, rewrites, and carries (`docs/portfolio-analysis.md` §The
    // position thesis ledger).
    let prior_ledger = dossier.prior_ledger();
    // The position's book weight — the feasible-set input and the
    // `portfolio-weight` ledger series.
    let current_weight = if account_total > 0.0 {
        Some(dossier.position.market_value / account_total)
    } else {
        None
    };

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
    let audit = |metrics, target_meta, ledger_audit| HoldingAudit {
        symbol: symbol.clone(),
        metrics,
        sources: dossier.sources.clone(),
        model_ids: analyst.model_ids(),
        prompt_version: PROMPT_VERSION.to_string(),
        degraded_inputs: degraded.clone(),
        target_meta,
        grade_parameter_version: Some(engine::GRADE_PARAMETER_VERSION.to_string()),
        ledger_audit,
    };
    let abstain = |reason: String, metrics, meta| {
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            disposition: VerdictDisposition::InsufficientEvidence { reason },
            // A below-floor exit retains the standing ledger unchanged — Steps
            // 6c–6f never ran for it (`docs/portfolio-workflow.md` §Step 6b).
            thesis_ledger: prior_ledger.cloned(),
        };
        Ok((verdict, audit(metrics, meta, None)))
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
        };
        return Ok((verdict, audit(Default::default(), None, None)));
    }

    // Eligibility: a net-short position is a direction the prescriptive layer doesn't
    // model — the ladder's verbs, the sizing multipliers, and the outcome labels all
    // read long — so it takes the not-rated treatment with a short-position reason;
    // its signed (negative) market value still feeds the whole-book aggregates
    // (`docs/portfolio-analysis.md` §Asset eligibility).
    if dossier.position.quantity < 0.0 {
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            disposition: VerdictDisposition::NotRated {
                reason: "held net short — the ladder's long-side semantics don't apply; \
                         the signed exposure still feeds the whole-book aggregates"
                    .to_string(),
            },
            thesis_ledger: None,
        };
        return Ok((verdict, audit(Default::default(), None, None)));
    }

    // The deterministic engine stage, per branch: the equity engine for a stock, the
    // reduced fund computation (strategy-routed at loop time) for a fund
    // (`docs/portfolio-workflow.md` §Step 6b).
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
                return abstain(reason, Default::default(), None);
            }
            FundEngineVerdict::RoleRiskOnly(readout) => {
                // Evaluate the prior fund ledger's quantitative conditions against
                // the reduced surface this branch actually computes (the expense
                // ratio joins the metrics; price/weight resolve from the dossier).
                let fund_metrics = engine::ComputedMetrics {
                    expense_ratio: readout.expense_ratio,
                    ..Default::default()
                };
                let ledger_eval = prior_ledger.map(|l| {
                    engine::evaluate_ledger_conditions(
                        l,
                        &fund_metrics,
                        &dossier.financials,
                        current_weight,
                        run_date,
                    )
                });
                // The union's other branch: the model authors the role read; the
                // reduced {sell all, trim, hold} spine is structural in the schema.
                let interpretation = analyst
                    .interpret_role_risk(&RoleRiskInput {
                        dossier,
                        readout: &readout,
                        ledger_eval: ledger_eval.as_ref(),
                    })
                    .context("interpreting the role/risk holding")?;
                if !ROLE_RISK_ACTIONS.contains(&interpretation.action) {
                    anyhow::bail!(
                        "role_risk_only action {:?} outside the reduced spine",
                        interpretation.action
                    );
                }
                // The 6g ledger seam: validate the rewrite — executability,
                // condition identity / carry, tripped / fired claims, the branch's
                // reductions (condition-only monitor, trim / sell triggers).
                let (ledger, ledger_audit) = validate_ledger_rewrite(
                    &interpretation.ledger,
                    prior_ledger,
                    ledger_eval.as_ref(),
                    LedgerBranch::RoleRiskOnly,
                    None,
                );
                let action_sizing = engine::size_action(
                    interpretation.action,
                    &dossier.position,
                    &dossier.profile,
                    account_total,
                );
                let verdict = HoldingVerdict {
                    symbol: symbol.clone(),
                    asset_class,
                    position_change,
                    disposition: VerdictDisposition::RoleRiskOnly(Box::new(RoleRiskVerdict {
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
                        action: interpretation.action,
                        action_sizing,
                        what_changed: interpretation.what_changed,
                    })),
                    thesis_ledger: Some(ledger),
                };
                return Ok((verdict, audit(Default::default(), None, Some(ledger_audit))));
            }
        }
    } else {
        match engine::analyze(&dossier.financials, rates) {
            EngineVerdict::Analyzed(out) => out,
            EngineVerdict::InsufficientEvidence(reason) => {
                return abstain(reason, Default::default(), None);
            }
        }
    };

    // The engine bounds the feasible action set from engine-known inputs before the
    // model picks a rung (`docs/portfolio-analysis.md` §Starting parameters).
    let feasible = engine::feasible_actions(
        engine_output.grade,
        &engine_output.hurdle,
        current_weight.unwrap_or(0.0),
    );

    // Evaluate the prior ledger's quantitative falsifiers and triggers against this
    // run's computed surface — the crossings interpretation reads
    // (`docs/portfolio-analysis.md` §The position thesis ledger).
    let ledger_eval = prior_ledger.map(|l| {
        engine::evaluate_ledger_conditions(
            l,
            &engine_output.metrics,
            &dossier.financials,
            current_weight,
            run_date,
        )
    });

    // Research (stubbed) → distill → interpret.
    let findings = research(dossier);
    let distilled = analyst
        .distill(dossier, &findings)
        .context("distilling research findings")?;
    let interpretation = analyst
        .interpret(&InterpretationInput {
            dossier,
            engine: &engine_output,
            distilled: &distilled,
            feasible: &feasible,
            ledger_eval: ledger_eval.as_ref(),
        })
        .context("interpreting the holding")?;
    // Defense in depth behind the schema constraint: an action outside the
    // engine-bounded set never persists.
    if !feasible.contains(&interpretation.action) {
        anyhow::bail!(
            "interpretation chose {:?} outside the engine-bounded feasible set {:?}",
            interpretation.action,
            feasible
        );
    }

    // The 6g ledger seam: validate the rewrite and stamp the engine's scenario
    // targets into the monitor (app-owns-the-number — a model-written target never
    // persists).
    let (ledger, ledger_audit) = validate_ledger_rewrite(
        &interpretation.ledger,
        prior_ledger,
        ledger_eval.as_ref(),
        LedgerBranch::Priced,
        engine_output.price_targets.twelve_month.as_ref(),
    );

    // Merge engine numbers + model judgment into the verdict; size the action.
    let action_sizing = engine::size_action(
        interpretation.action,
        &dossier.position,
        &dossier.profile,
        account_total,
    );
    let graded = GradedVerdict {
        grade: engine_output.grade,
        sub_scores: engine_output.sub_scores,
        action: interpretation.action,
        action_sizing,
        conviction: interpretation.conviction,
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
    };
    let verdict = HoldingVerdict {
        symbol: symbol.clone(),
        asset_class,
        position_change,
        disposition: VerdictDisposition::Priced(Box::new(graded)),
        thesis_ledger: Some(ledger),
    };
    // The engine's own gap notes (tier-input gaps, the fund composite's uncovered
    // share, an option-overlay structural flag) join the audit's degraded inputs —
    // recorded, never silently dropped.
    let mut degraded_inputs = degraded.clone();
    degraded_inputs.extend(engine_output.tier_gaps.iter().cloned());
    let audit_record = HoldingAudit {
        symbol: symbol.clone(),
        metrics: engine_output.metrics.clone(),
        sources: dossier.sources.clone(),
        model_ids: analyst.model_ids(),
        prompt_version: PROMPT_VERSION.to_string(),
        degraded_inputs,
        target_meta: Some(engine_output.target_meta.clone()),
        grade_parameter_version: Some(engine::GRADE_PARAMETER_VERSION.to_string()),
        ledger_audit: Some(ledger_audit),
    };
    Ok((verdict, audit_record))
}

// ---- Thesis-ledger rewrite validation (the 6g seam) ----------------------------

/// Parse a draft's quantitative-core claim against the engine's executability
/// surface — the resolution contract's app-side check
/// (`docs/portfolio-workflow.md` §Step 6g): the series must be one the engine
/// actually computes and refreshes, the comparator well-formed, the numbers finite.
/// `Err` carries the downgrade reason.
fn parse_quant_core(qd: &QuantCoreDraft) -> std::result::Result<QuantCore, String> {
    let series = engine::LedgerSeries::parse(&qd.series).ok_or_else(|| {
        format!(
            "series '{}' does not resolve to a series the engine computes",
            qd.series
        )
    })?;
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
        Some(qd) => match parse_quant_core(qd) {
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
/// crossings, the engine scenario targets stamped into the monitor, the branch's
/// reductions, and the acknowledgment stamp on each consumed confirmed crossing.
pub fn validate_ledger_rewrite(
    draft: &LedgerDraft,
    prior: Option<&ThesisLedger>,
    evaluation: Option<&LedgerEvaluation>,
    branch: LedgerBranch,
    engine_targets: Option<&PriceTarget>,
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
        match quant_draft.and_then(|qd| parse_quant_core(qd).ok()) {
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
        let Some(core) = quant_draft.and_then(|qd| parse_quant_core(qd).ok()) else {
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

    // The target-weight range: clamped to fractions, ordered.
    let clamp_w = |w: f64| if w.is_finite() { w.clamp(0.0, 1.0) } else { 0.0 };
    let (mut low, mut high) = (clamp_w(draft.target_weight_low), clamp_w(draft.target_weight_high));
    if low > high {
        std::mem::swap(&mut low, &mut high);
    }

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
        target_weight_low: low,
        target_weight_high: high,
    };
    (ledger, audit)
}

// ---- Prompt construction (pure, testable) ------------------------------------

/// The system prompt for the interpretation stage — the role and the load-bearing
/// rule: read numbers from the engine, never invent them.
pub fn interpretation_system_prompt() -> String {
    "You are a disciplined equity analyst grading one holding for a prescriptive \
     portfolio review. The quantitative analysis — sub-scores, the composite grade, \
     valuation multiples, the risk tier, the capital-efficiency read, and the scenario \
     price targets — has already been computed deterministically and is given to you. \
     Do NOT invent or alter any number: read them from the analysis. Your job is the \
     judgment the numbers don't make: choose the action from the ALLOWED ACTIONS the \
     engine offers (never outside it), set your conviction and the three horizon reads, \
     justify the base-case price target, and write a concise financial summary and a \
     continuity note. Conviction means your confidence in the overall read — grade, \
     outlook, and action together — and must match the action's decisiveness: a \
     decisive action (sell all, add aggressively) requires conviction you actually \
     hold; if your conviction is low, choose a less decisive allowed action instead. \
     Use the Market Signal house view for the horizon reads and market-setup context \
     only — it is a market-level thesis, never by itself a reason to exit a specific \
     holding. Apply the investor profile. \
     You also maintain the position's THESIS LEDGER — the persisted standing thesis \
     with monitorable falsifiers and pre-committed action triggers: test the prior \
     ledger against this run's evidence and the engine's deterministic condition \
     crossings, then rewrite it per the instructions in the prompt. \
     Respond only with the required JSON object."
        .to_string()
}

/// The system prompt for the `role_risk_only` interpretation — the union's other
/// branch: role and risk only, no letter, no targets, no conviction.
pub fn role_risk_system_prompt() -> String {
    "You are a disciplined portfolio analyst assessing one holding whose vehicle \
     class this pipeline is structurally unable to price (a bond or commodity fund, \
     an ex-US fund, a leveraged/inverse vehicle, or a fund without usable weightings). \
     Do NOT produce a grade, price target, or conviction — none exists for this \
     branch. Your job: describe the vehicle's role — the mandate and the exposure it \
     exists to supply, read in isolation — and choose an action from the reduced \
     ladder (sell-all / trim / hold) with the rationale limited to portfolio role and \
     risk. Read the engine's exposure, expense, and risk figures; never invent one. \
     You also maintain the holding's THESIS LEDGER (fund-flavored drivers; \
     condition-only monitor; trim/sell triggers only) — test the prior ledger against \
     this run's evidence and rewrite it per the instructions in the prompt. \
     Respond only with the required JSON object."
        .to_string()
}

/// The user prompt for the `role_risk_only` interpretation: the engine's typed
/// readout rendered for the model.
pub fn role_risk_user_prompt(input: &RoleRiskInput) -> String {
    let d = input.dossier;
    let r = input.readout;
    let mut p = String::new();
    p.push_str(&format!(
        "HOLDING: {} ({})\nQuantity: {}  Cost basis: {:.0}  Market value: {:.0}\n",
        d.position.symbol,
        d.position.description,
        d.position.quantity,
        d.position.cost_basis,
        d.position.market_value,
    ));
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
        "EXPENSE RATIO: {}\nOBSERVABLE RISK (annualized volatility): {}\n",
        opt(r.expense_ratio),
        opt(r.observable_risk),
    ));
    if !r.evidence_gaps.is_empty() {
        p.push_str(&format!("EVIDENCE GAPS: {}\n", r.evidence_gaps.join("; ")));
    }
    if let Some(sections) = &d.house_view.latest_sections {
        p.push_str(&format!(
            "\nMARKET SIGNAL HOUSE VIEW (latest report — scope: market-setup context \
             only, never by itself a reason to exit this holding):\n{sections}\n"
        ));
    }
    p.push_str("\nALLOWED ACTIONS: sell-all, trim, hold (the reduced spine — no add family).\n");
    match &d.prior_verdict {
        Some(_) => p.push_str(
            "\nCONTINUITY: a prior verdict for this holding exists. Keep the read firm; \
             say what changed.\n",
        ),
        None => p.push_str("\nCONTINUITY: new holding (no prior verdict).\n"),
    }
    p.push_str(&ledger_prompt_section(
        d.prior_ledger(),
        input.ledger_eval,
        true,
    ));
    p
}

/// The user prompt: the holding's evidence packet rendered for the model — the
/// position, the computed metrics/sub-scores/grade/targets, the options-activity
/// signal (an activity proxy, not a grade input), the gaps, the distilled research,
/// the house view, and the prior verdict for continuity.
pub fn interpretation_user_prompt(input: &InterpretationInput) -> String {
    let d = input.dossier;
    let e = input.engine;
    let mut p = String::new();

    p.push_str(&format!(
        "HOLDING: {} ({})\nQuantity: {}  Cost basis: {:.0}  Market value: {:.0}\n",
        d.position.symbol,
        d.position.description,
        d.position.quantity,
        d.position.cost_basis,
        d.position.market_value,
    ));
    p.push_str(&format!(
        "Position change since last run: {}\n",
        describe_position_change(&d.position_delta, d.position.quantity, d.position.cost_basis)
    ));

    p.push_str(&format!(
        "\nCOMPUTED GRADE: {} (do not change{})\nSUB-SCORES (0-100, higher better): quality {:.0}, valuation {:.0}, risk {:.0}; \
         momentum {:.0} rides as market-setup context OUTSIDE the letter\n",
        e.grade.as_str(),
        if e.low_confidence_grade {
            "; low-confidence — an imputed sub-score underlies it"
        } else {
            ""
        },
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
             low-confidence marker). Expense ratio: {}. US share: {}.\n",
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
    p.push_str(&line("return volatility", m.return_volatility));
    p.push_str(&line("trailing return", m.trailing_return));
    p.push_str(&line("P/E", m.pe_ratio));
    p.push_str(&line("P/S", m.ps_ratio));
    p.push_str(&line("P/B", m.pb_ratio));

    if let Some(tm) = &e.price_targets.twelve_month {
        p.push_str(&format!(
            "\nSCENARIO TARGETS (twelve-month rolling): bear {:.2} / base {:.2} / bull {:.2}\n  methodology: {}\n",
            tm.bear, tm.base, tm.bull, tm.methodology
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

    p.push_str("\nALLOWED ACTIONS (the engine-bounded feasible set — choose within it): ");
    let allowed: Vec<&str> = input.feasible.iter().map(Action::as_kebab).collect();
    p.push_str(&allowed.join(", "));
    p.push('\n');

    let s = &d.options_signal;
    p.push_str(&format!(
        "\nOPTIONS ACTIVITY (proxy only — NOT a grade input): put/call vol {}, put/call OI {}, IV {}, IV skew {}\n",
        opt(s.put_call_volume),
        opt(s.put_call_open_interest),
        opt(s.implied_volatility),
        opt(s.iv_skew),
    ));

    if !d.financials.gaps.is_empty() {
        p.push_str(&format!("\nDATA GAPS: {}\n", d.financials.gaps.join("; ")));
    }

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

    p.push_str(&format!(
        "\nINVESTOR PROFILE: risk tolerance {:?}, horizon {:?}, taxable {}, cash {}\n",
        d.profile.risk_tolerance,
        d.profile.horizon,
        d.profile.tax_sensitive,
        d.profile
            .available_cash
            .map(|c| format!("{c:.0}"))
            .unwrap_or_else(|| "unconstrained".to_string()),
    ));

    p.push_str("\nHORIZONS for the outlook: ");
    p.push_str(&format!("{HORIZON_SHORT}, {HORIZON_MID}, {HORIZON_LONG}.\n"));

    match &d.prior_verdict {
        Some(_) => {
            p.push_str(
                "\nCONTINUITY: a prior verdict for this holding exists. Keep the verdict firm; \
                 only move grade/action/target if the evidence has materially changed, and say what.\n",
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
        }
        None => p.push_str("\nCONTINUITY: new holding (no prior verdict).\n"),
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
            p.push_str(&format!(
                "Target weight range: {:.1}%–{:.1}%\n",
                l.target_weight_low * 100.0,
                l.target_weight_high * 100.0
            ));

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
         stay in the base case; the key falsifiers; and the action triggers with a \
         target-weight range (fractions of the portfolio, e.g. 0.05). \
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
        PositionChange::New => "NEW (not held last run)".to_string(),
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
            let basis = match delta.prior_cost_basis {
                Some(prev) => format!(", cost basis {prev:.0} → now {current_cost_basis:.0}"),
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
            target_weight_low: l.target_weight_low,
            target_weight_high: l.target_weight_high,
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
            statement: "Trim above 25% of the portfolio".into(),
            family: "trim".into(),
            quant: Some(QuantCoreDraft {
                series: "portfolio-weight".into(),
                comparator: "above".into(),
                threshold: 0.25,
                margin: 0.0,
            }),
            fired: false,
        }],
        target_weight_low: 0.02,
        target_weight_high: 0.10,
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
        let preferred = match e.grade {
            crate::portfolio::Grade::A => Action::Add,
            crate::portfolio::Grade::B | crate::portfolio::Grade::C => Action::Hold,
            crate::portfolio::Grade::D => Action::Trim,
            crate::portfolio::Grade::F => Action::SellAll,
        };
        // The live path's schema constrains the action to the feasible set; the stub
        // honors the same bound by falling back to hold (always offered).
        let action = if input.feasible.contains(&preferred) {
            preferred
        } else {
            Action::Hold
        };
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
            action,
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
            ledger: stub_ledger_draft(
                input.dossier.prior_ledger(),
                &input.dossier.position.symbol,
                false,
            ),
        })
    }

    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
        Ok(RoleRiskInterpretation {
            action: Action::Hold,
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
            ledger: stub_ledger_draft(
                input.dossier.prior_ledger(),
                &input.dossier.position.symbol,
                true,
            ),
        })
    }

    fn model_ids(&self) -> Vec<String> {
        vec!["stub-analyst".to_string()]
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
}

impl LocalAnalyst {
    /// A blank `fast_model` falls back to the reasoner: the fast tier is **optional**
    /// and never gates (`docs/configuration.md §Local Analysis Suite Configuration`),
    /// and the documented roster default runs distillation on the resident reasoner
    /// anyway (`docs/local-models.md §The model roster and per-task routing`) — so a
    /// reasoner+embedder-only setup runs rather than failing mid-run on an empty id.
    pub fn new(client: LocalModelClient, reasoner_model: String, fast_model: String) -> Self {
        let fast_model = if fast_model.trim().is_empty() {
            reasoner_model.clone()
        } else {
            fast_model
        };
        Self {
            client,
            reasoner_model,
            fast_model,
        }
    }
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
    req.options = Some(options::non_thinking_general(num_ctx));
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
    // The per-holding schema advertises only the engine-bounded feasible set, so
    // a barred rung is structurally unreachable (`docs/portfolio-analysis.md`
    // §Starting parameters — the feasible-set rule).
    req.format_schema = Some(interpretation_schema(input.feasible));
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET));
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
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET));
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
        serde_json::from_str(&resp.content)
            .with_context(|| format!("parsing interpretation JSON: {}", resp.content))
    }

    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
        let req = role_risk_request(&self.reasoner_model, input);
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let resp = self.client.chat_streaming(&req, StreamRole::Step(&step_key))?;
        serde_json::from_str(&resp.content)
            .with_context(|| format!("parsing role/risk interpretation JSON: {}", resp.content))
    }

    fn model_ids(&self) -> Vec<String> {
        let mut ids = vec![self.reasoner_model.clone(), self.fast_model.clone()];
        // One entry when the fast tier fell back to the reasoner, so the audit
        // record doesn't list the same model twice.
        ids.dedup();
        ids
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
            prior_grade_parameter_version: None,
            sources: vec!["FMP".into()],
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
    fn gradeable_holding_produces_a_priced_verdict_offline() {
        let (verdict, audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            29_500.0,
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
        assert_eq!(meta.parameter_version, "targets-v3");
    }

    #[test]
    fn priced_fund_takes_the_reduced_path_with_the_grade_contract() {
        let (verdict, audit) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(us_equity_fund()),
            29_500.0,
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
            29_500.0,
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
            29_500.0,
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        match verdict.disposition {
            VerdictDisposition::RoleRiskOnly(r) => {
                assert_eq!(r.class_label, "bond fund");
                // The reduced spine only; the stub holds.
                assert_eq!(r.action, Action::Hold);
                assert!(!r.role_summary.is_empty());
                assert!(!r.evidence_gaps.is_empty());
            }
            other => panic!("expected role_risk_only, got {other:?}"),
        }
    }

    #[test]
    fn ineligible_asset_class_is_not_rated_without_a_model_call() {
        let (verdict, _audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::OptionContract, strong_financials()),
            29_500.0,
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
            analyze_holding(&StubAnalyst, &d, 29_500.0, &rates(), "2026-08-03").unwrap();
        match verdict.disposition {
            VerdictDisposition::NotRated { reason } => {
                assert!(reason.contains("short"), "{reason}");
            }
            other => panic!("expected not-rated, got {other:?}"),
        }
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
            29_500.0,
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
        assert_eq!(
            describe_position_change(&PositionDelta::new_position(), 10.0, 1_000.0),
            "NEW (not held last run)"
        );
    }

    #[test]
    fn interpretation_prompt_carries_the_engine_numbers_and_the_do_not_invent_rule() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let feasible = vec![Action::SellAll, Action::Trim, Action::Hold];
        let input = InterpretationInput {
            dossier: &d,
            engine: &engine_output,
            distilled: "distilled findings",
            feasible: &feasible,
            ledger_eval: None,
        };
        let user = interpretation_user_prompt(&input);
        assert!(user.contains("COMPUTED GRADE"), "{user}");
        assert!(user.contains("SUB-SCORES"), "{user}");
        assert!(user.contains("NOT a grade input"), "options proxy is flagged: {user}");
        assert!(user.contains("RISK TIER"), "{user}");
        // The engine-bounded feasible set is stated, and a barred rung isn't listed.
        assert!(user.contains("ALLOWED ACTIONS"), "{user}");
        let allowed_line = user
            .lines()
            .find(|l| l.contains("ALLOWED ACTIONS"))
            .unwrap();
        assert!(!allowed_line.contains("add"), "{allowed_line}");
        assert!(interpretation_system_prompt().contains("Do NOT invent"));

        // The prompt-adjustments slice (portfolio-v3): target provenance always
        // renders, the dead-money read is a weighed input (not an instruction), and
        // the system prompt defines conviction and scopes the house view.
        assert!(user.contains("TARGET PROVENANCE"), "{user}");
        assert!(user.contains("one input to weigh"), "{user}");
        let system = interpretation_system_prompt();
        assert!(system.contains("Conviction means"), "{system}");
        assert!(system.contains("horizon reads and market-setup context"), "{system}");
        assert!(system.contains("never by itself a reason to exit"), "{system}");
    }

    #[test]
    fn target_provenance_renders_the_anchored_and_carry_branches() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let mut engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let feasible = vec![Action::Hold];

        engine_output.target_meta.rate_anchored = true;
        engine_output.target_meta.anchor_observations = 40;
        engine_output.target_meta.current_multiple_carry = false;
        let anchored = interpretation_user_prompt(&InterpretationInput {
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            feasible: &feasible,
            ledger_eval: None,
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
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            feasible: &feasible,
            ledger_eval: None,
        });
        assert!(carried.contains("current multiple was carried"), "{carried}");
        assert!(carried.contains("driver held FLAT"), "{carried}");
        assert!(carried.contains("volatility dispersion floor"), "{carried}");

        // Neither anchored nor carried: the raw-percentile fallback branch.
        engine_output.target_meta.current_multiple_carry = false;
        let fallback = interpretation_user_prompt(&InterpretationInput {
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            feasible: &feasible,
            ledger_eval: None,
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
        };
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let feasible = vec![Action::Hold];
        let prompt = |d: &HoldingDossier| {
            interpretation_user_prompt(&InterpretationInput {
                dossier: d,
                engine: &engine_output,
                distilled: "",
                feasible: &feasible,
                ledger_eval: None,
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
        let feasible = vec![Action::Hold];
        let user = interpretation_user_prompt(&InterpretationInput {
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            feasible: &feasible,
            ledger_eval: None,
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
            class_label: "ex-US equity fund".into(),
            structural_flag: false,
            exposure_tilt: vec![],
            expense_ratio: None,
            observable_risk: None,
            evidence_gaps: vec![],
        };
        let role = role_risk_user_prompt(&RoleRiskInput {
            dossier: &d,
            readout: &readout,
            ledger_eval: None,
        });
        assert!(role.contains("never by itself a reason to exit"), "{role}");
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
        assert_eq!(opts["temperature"], 0.7, "non-thinking-general row");
        assert!(distill.format_schema.is_none(), "distill is free prose");

        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let feasible = vec![Action::SellAll, Action::Trim, Action::Hold];
        let interpret = interpret_request(
            "reasoner-model",
            &InterpretationInput {
                dossier: &d,
                engine: &engine_output,
                distilled: "distilled findings",
                feasible: &feasible,
                ledger_eval: None,
            },
        );
        assert_eq!(interpret.think, Some(true));
        assert_eq!(interpret.keep_alive, Some(-1));
        let opts = interpret.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET);
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
                dossier: &d,
                readout: &readout,
                ledger_eval: None,
            },
        );
        assert_eq!(role_risk.think, Some(true));
        assert_eq!(role_risk.keep_alive, Some(-1));
        let opts = role_risk.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET);
        assert!(role_risk.format_schema.is_some(), "grammar-constrained");
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
    fn feasible_set_violation_is_rejected_in_depth() {
        // An analyst that ignores the feasible set must not persist its action.
        struct RogueAnalyst;
        impl HoldingAnalyst for RogueAnalyst {
            fn distill(&self, _d: &HoldingDossier, _f: &ResearchFindings) -> Result<String> {
                Ok("".into())
            }
            fn interpret(&self, _input: &InterpretationInput) -> Result<Interpretation> {
                Ok(Interpretation {
                    action: Action::AddAggressively,
                    conviction: Conviction::High,
                    horizon_outlook: HorizonOutlook {
                        short: HorizonRead::Bullish,
                        mid: HorizonRead::Bullish,
                        long: HorizonRead::Bullish,
                    },
                    financial_summary: "".into(),
                    price_target_rationale: "".into(),
                    what_changed: "".into(),
                    ledger: stub_ledger_draft(None, "AAPL", false),
                })
            }
            fn interpret_role_risk(&self, _input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
                unreachable!()
            }
            fn model_ids(&self) -> Vec<String> {
                vec!["rogue".into()]
            }
        }
        // The strong fixture reads dead-money under the conservative flat anchor
        // (base target below spot), so add-aggressively is outside the feasible set.
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let feasible = engine::feasible_actions(
            engine_output.grade,
            &engine_output.hurdle,
            19_500.0 / 29_500.0,
        );
        if feasible.contains(&Action::AddAggressively) {
            // Fixture drift made the rung feasible — the guard has nothing to reject.
            return;
        }
        let err =
            analyze_holding(&RogueAnalyst, &d, 29_500.0, &rates(), "2026-08-03").unwrap_err();
        assert!(err.to_string().contains("feasible"), "{err}");
    }

    #[test]
    fn blank_fast_tier_falls_back_to_the_reasoner() {
        // The fast tier is optional and never gates (`docs/configuration.md`), so a
        // blank slot must not reach the daemon as an empty model id — distillation
        // runs on the reasoner instead, and the audit's model list carries it once.
        let client = LocalModelClient::new("http://127.0.0.1:1").unwrap();
        let analyst = LocalAnalyst::new(client, "qwen3.5:122b".into(), "  ".into());
        assert_eq!(analyst.model_ids(), vec!["qwen3.5:122b".to_string()]);

        // A configured fast tier is used as-is.
        let client = LocalModelClient::new("http://127.0.0.1:1").unwrap();
        let analyst = LocalAnalyst::new(client, "r".into(), "f".into());
        assert_eq!(
            analyst.model_ids(),
            vec!["r".to_string(), "f".to_string()]
        );
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
                    statement: "Trim above 25% of the portfolio".into(),
                    quant: Some(QuantCore {
                        series: engine::LedgerSeries::PortfolioWeight,
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
            target_weight_low: 0.02,
            target_weight_high: 0.10,
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
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, Some(&targets));
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
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, None);
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
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
                validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
                validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
        let weight_core = QuantCore {
            series: engine::LedgerSeries::PortfolioWeight,
            comparator: LedgerComparator::Above,
            threshold: 0.25,
            margin: 0.0,
        };
        let mut prior = prior_with_conditions();
        prior.conditions = vec![
            LedgerCondition {
                condition_id: "trim-1".into(),
                role: ConditionRole::Trigger,
                trigger_family: Some(TriggerFamily::Trim),
                statement: "Trim above a quarter of the book".into(),
                quant: Some(weight_core.clone()),
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
                statement: "Exit fully above a quarter of the book".into(),
                quant: Some(weight_core.clone()),
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
            statement: format!("{family} above a quarter of the book"),
            family: family.into(),
            quant: Some(QuantCoreDraft {
                series: "portfolio-weight".into(),
                comparator: "above".into(),
                threshold: 0.25,
                margin: 0.0,
            }),
            fired: false,
        };
        for order in [["trim", "sell"], ["sell", "trim"]] {
            let mut draft = stub_ledger_draft(Some(&prior), "AAPL", false);
            draft.triggers = order.iter().map(|f| trigger(f)).collect();
            let (ledger, audit) =
                validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
            statement: "Exit fully above a quarter of the book".into(),
            quant: Some(QuantCore {
                series: engine::LedgerSeries::PortfolioWeight,
                comparator: LedgerComparator::Above,
                threshold: 0.25,
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
            statement: "Trim above thirty percent".into(),
            family: "trim".into(),
            quant: Some(QuantCoreDraft {
                series: "portfolio-weight".into(),
                comparator: "above".into(),
                threshold: 0.30,
                margin: 0.0,
            }),
            fired: false,
        }];
        let (ledger, audit) =
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
            }],
            unevaluable: vec![],
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
            validate_ledger_rewrite(&draft, None, None, LedgerBranch::RoleRiskOnly, None);
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
            Some(&targets),
        );
        assert!(ledger.monitor.iter().all(|m| m.engine_target.is_none()));
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
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
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
            validate_ledger_rewrite(&draft, Some(&prior), None, LedgerBranch::Priced, None);
        assert_eq!(audit.duplicates.len(), 1, "{:?}", audit.duplicates);
    }

    #[test]
    fn weight_range_clamps_to_fractions_and_orders() {
        let mut draft = stub_ledger_draft(None, "AAPL", false);
        draft.target_weight_low = 0.5;
        draft.target_weight_high = 0.1;
        let (ledger, _) = validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, None);
        assert_eq!(
            (ledger.target_weight_low, ledger.target_weight_high),
            (0.1, 0.5),
            "swapped into order"
        );
        draft.target_weight_low = -0.2;
        draft.target_weight_high = 3.0;
        let (ledger, _) = validate_ledger_rewrite(&draft, None, None, LedgerBranch::Priced, None);
        assert_eq!((ledger.target_weight_low, ledger.target_weight_high), (0.0, 1.0));
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
            }],
            unevaluable: vec!["condition 'x': net margin is a gap this run".into()],
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
        assert!(s.contains("2.0%–10.0%"), "the weight range renders: {s}");

        // The role_risk variant names the branch reductions.
        let rr = ledger_prompt_section(Some(&prior), None, true);
        assert!(rr.contains("trim/sell only"), "{rr}");

        // Both interpretation prompts carry the section.
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let feasible = vec![Action::Hold];
        let user = interpretation_user_prompt(&InterpretationInput {
            dossier: &d,
            engine: &engine_output,
            distilled: "",
            feasible: &feasible,
            ledger_eval: None,
        });
        assert!(user.contains("REWRITE THE THESIS LEDGER"), "{user}");
        assert!(interpretation_system_prompt().contains("THESIS LEDGER"));
        assert!(role_risk_system_prompt().contains("THESIS LEDGER"));
    }

    #[test]
    fn debut_authors_a_ledger_and_an_abstention_retains_the_prior_one() {
        // A priced debut carries the authored ledger, its monitor stamped from the
        // engine's own scenario set.
        let (verdict, audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            29_500.0,
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
        });
        let (v2, _) =
            analyze_holding(&StubAnalyst, &d, 29_500.0, &rates(), "2026-08-03").unwrap();
        assert!(matches!(
            v2.disposition,
            VerdictDisposition::InsufficientEvidence { .. }
        ));
        assert_eq!(v2.thesis_ledger, Some(prior_with_conditions()));
    }

    #[test]
    fn oversized_weight_walks_the_trigger_through_first_breach_to_confirmed_and_ack() {
        // The stub's debut trigger (portfolio-weight above 0.25) is breached at
        // weight 0.66 — a market-data condition (count 2) whose observation
        // identity is the marks' trading day: run 2 logs a quiet first-breach
        // note, run 3 — carrying a genuinely NEW trading print — confirms and
        // fires, and the consuming pass stamps the acknowledging observation.
        let d1 = dossier(AssetClass::Stock, strong_financials());
        let (v1, _) = analyze_holding(&StubAnalyst, &d1, 29_500.0, &rates(), "2026-08-03").unwrap();

        let mut d2 = dossier(AssetClass::Stock, strong_financials());
        d2.prior_verdict = Some(v1);
        let (v2, audit2) =
            analyze_holding(&StubAnalyst, &d2, 29_500.0, &rates(), "2026-08-04").unwrap();
        let a2 = audit2.ledger_audit.unwrap();
        assert!(
            a2.crossings.iter().any(|c| c.role == ConditionRole::Trigger
                && c.outcome == CrossingOutcome::FirstBreach),
            "{:?}",
            a2.crossings
        );

        // A rerun with NO new trading print must not advance the streak — weight
        // identity is the marks' day, never the calendar date of the run.
        let mut d2b = dossier(AssetClass::Stock, strong_financials());
        d2b.prior_verdict = Some(v2.clone());
        let (_, audit2b) =
            analyze_holding(&StubAnalyst, &d2b, 29_500.0, &rates(), "2026-08-05").unwrap();
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
            analyze_holding(&StubAnalyst, &d3, 29_500.0, &rates(), "2026-08-05").unwrap();
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
}
