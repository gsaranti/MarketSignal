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

use crate::local_model::{ChatMessage, ChatRequest, LocalModelClient, StreamRole};
use crate::portfolio::dossier::HoldingDossier;
use crate::portfolio::engine::{self, EngineOutput, EngineVerdict, RateAnchors};
use crate::portfolio::fund::{self, FundEngineVerdict, RoleRiskReadout};
use crate::portfolio::{
    interpretation_schema, role_risk_interpretation_schema, Action, Conviction, ExposureWeight,
    GradedVerdict, HoldingAudit, HoldingVerdict, HorizonOutlook, HorizonRead, Interpretation,
    PositionChange, PositionDelta, RoleRiskInterpretation, RoleRiskVerdict, VerdictDisposition,
    HORIZON_LONG, HORIZON_MID, HORIZON_SHORT, PROMPT_VERSION, ROLE_RISK_ACTIONS,
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
}

/// What the `role_risk_only` interpretation reads: the dossier plus the engine's
/// typed readout — none of the priced machinery exists on this branch.
pub struct RoleRiskInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub readout: &'a RoleRiskReadout,
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
) -> Result<(HoldingVerdict, HoldingAudit)> {
    let symbol = dossier.position.symbol.clone();
    let asset_class = dossier.position.asset_class;
    // App-set from the deterministic holdings diff, never the model — carried on every
    // verdict (graded or not) as the structured what-changed position tag.
    let position_change = dossier.position_delta.change;

    let mut degraded = dossier.financials.gaps.clone();
    if let Some(f) = &dossier.fund {
        degraded.extend(f.fund.gaps.iter().cloned());
    }
    let audit = |metrics, target_meta| HoldingAudit {
        symbol: symbol.clone(),
        metrics,
        sources: dossier.sources.clone(),
        model_ids: analyst.model_ids(),
        prompt_version: PROMPT_VERSION.to_string(),
        degraded_inputs: degraded.clone(),
        target_meta,
    };
    let abstain = |reason: String, metrics, meta| {
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            disposition: VerdictDisposition::InsufficientEvidence { reason },
        };
        Ok((verdict, audit(metrics, meta)))
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
        };
        return Ok((verdict, audit(Default::default(), None)));
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
                // The union's other branch: the model authors the role read; the
                // reduced {sell all, trim, hold} spine is structural in the schema.
                let interpretation = analyst
                    .interpret_role_risk(&RoleRiskInput {
                        dossier,
                        readout: &readout,
                    })
                    .context("interpreting the role/risk holding")?;
                if !ROLE_RISK_ACTIONS.contains(&interpretation.action) {
                    anyhow::bail!(
                        "role_risk_only action {:?} outside the reduced spine",
                        interpretation.action
                    );
                }
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
                };
                return Ok((verdict, audit(Default::default(), None)));
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
    let current_weight = if account_total > 0.0 {
        dossier.position.market_value / account_total
    } else {
        0.0
    };
    let feasible =
        engine::feasible_actions(engine_output.grade, &engine_output.hurdle, current_weight);

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
        financial_summary: interpretation.financial_summary,
        what_changed: interpretation.what_changed,
    };
    let verdict = HoldingVerdict {
        symbol: symbol.clone(),
        asset_class,
        position_change,
        disposition: VerdictDisposition::Priced(Box::new(graded)),
    };
    // The engine's own gap notes (tier-input gaps, the fund composite's uncovered
    // share, an unverifiable US-exposure guard) join the audit's degraded inputs —
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
    };
    Ok((verdict, audit_record))
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
     continuity note. Apply the Market Signal house view and the investor profile. \
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
        p.push_str(&format!("\nMARKET SIGNAL HOUSE VIEW (latest report):\n{sections}\n"));
    }
    p.push_str("\nALLOWED ACTIONS: sell-all, trim, hold (the reduced spine — no add family).\n");
    match &d.prior_verdict {
        Some(_) => p.push_str(
            "\nCONTINUITY: a prior verdict for this holding exists. Keep the read firm; \
             say what changed.\n",
        ),
        None => p.push_str("\nCONTINUITY: new holding (no prior verdict).\n"),
    }
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
         `fails` is dead money — it should tilt your standalone read toward exit on \
         the holding's own merits)\n",
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
        p.push_str(&format!("\nMARKET SIGNAL HOUSE VIEW (latest report):\n{sections}\n"));
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
        Some(_) => p.push_str(
            "\nCONTINUITY: a prior verdict for this holding exists. Keep the verdict firm; \
             only move grade/action/target if the evidence has materially changed, and say what.\n",
        ),
        None => p.push_str("\nCONTINUITY: new holding (no prior verdict).\n"),
    }

    p
}

fn opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.3}")).unwrap_or_else(|| "(gap)".to_string())
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

impl HoldingAnalyst for LocalAnalyst {
    fn distill(&self, dossier: &HoldingDossier, findings: &ResearchFindings) -> Result<String> {
        // The fast model condenses the findings into a compact paragraph. With research
        // stubbed this is light, but it keeps the stage in the live path.
        let prompt = format!(
            "Condense these research findings on {} into 2-3 sentences of decision-relevant \
             signal. Findings:\n{}",
            dossier.position.symbol,
            findings.notes.join("\n")
        );
        let req = ChatRequest::new(
            &self.fast_model,
            vec![ChatMessage::user(prompt)],
        );
        let resp = self.client.chat(&req)?;
        Ok(resp.content)
    }

    fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation> {
        let mut req = ChatRequest::new(
            &self.reasoner_model,
            vec![
                ChatMessage::system(interpretation_system_prompt()),
                ChatMessage::user(interpretation_user_prompt(input)),
            ],
        );
        // The per-holding schema advertises only the engine-bounded feasible set, so
        // a barred rung is structurally unreachable (`docs/portfolio-analysis.md`
        // §Starting parameters — the feasible-set rule).
        req.format_schema = Some(interpretation_schema(input.feasible));
        req.think = true;
        // Stream silently: the structured stage has no console value, but accumulating
        // through the stream path keeps the reasoning on the tracker's thinking channel.
        let resp = self.client.chat_streaming(&req, StreamRole::Silent)?;
        serde_json::from_str(&resp.content)
            .with_context(|| format!("parsing interpretation JSON: {}", resp.content))
    }

    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<RoleRiskInterpretation> {
        let mut req = ChatRequest::new(
            &self.reasoner_model,
            vec![
                ChatMessage::system(role_risk_system_prompt()),
                ChatMessage::user(role_risk_user_prompt(input)),
            ],
        );
        req.format_schema = Some(role_risk_interpretation_schema());
        req.think = true;
        let resp = self.client.chat_streaming(&req, StreamRole::Silent)?;
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
        assert_eq!(meta.parameter_version, "targets-v2");
    }

    #[test]
    fn priced_fund_takes_the_reduced_path_with_the_grade_contract() {
        let (verdict, audit) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(us_equity_fund()),
            29_500.0,
            &rates(),
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
        )
        .unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::NotRated { .. }
        ));
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
        let err = analyze_holding(&RogueAnalyst, &d, 29_500.0, &rates()).unwrap_err();
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
}
