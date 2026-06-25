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
use crate::portfolio::engine::{self, EngineOutput, EngineVerdict};
use crate::portfolio::{
    interpretation_schema, Action, Conviction, GradedVerdict, HoldingAudit, HoldingVerdict,
    HorizonOutlook, HorizonRead, Interpretation, VerdictDisposition, HORIZON_LONG, HORIZON_MID,
    HORIZON_SHORT, PROMPT_VERSION,
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
/// and the distilled research findings. The model reasons over *this* — evidence, not
/// a gathering transcript.
pub struct InterpretationInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub engine: &'a EngineOutput,
    pub distilled: &'a str,
}

/// The two model-backed stages of the pipeline, behind a trait so the orchestration is
/// stub-driven offline and daemon-driven live. The research stage is a deterministic
/// app-layer function ([`research`]) this slice, not part of the trait.
pub trait HoldingAnalyst {
    /// Consolidate the raw findings into the compact distillation the interpretation
    /// reads (the fast 35B model, live).
    fn distill(&self, dossier: &HoldingDossier, findings: &ResearchFindings) -> Result<String>;
    /// Interpret the computed analysis + distilled findings into the schema-constrained
    /// verdict judgment (the 122B reasoner in thinking mode, live).
    fn interpret(&self, input: &InterpretationInput) -> Result<Interpretation>;
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
) -> Result<(HoldingVerdict, HoldingAudit)> {
    let symbol = dossier.position.symbol.clone();
    let asset_class = dossier.position.asset_class;

    let audit = |metrics| HoldingAudit {
        symbol: symbol.clone(),
        metrics,
        sources: dossier.sources.clone(),
        model_ids: analyst.model_ids(),
        prompt_version: PROMPT_VERSION.to_string(),
        degraded_inputs: dossier.financials.gaps.clone(),
    };

    // Eligibility: a non-equity class is never given a fabricated grade.
    if !asset_class.is_gradeable() {
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            disposition: VerdictDisposition::NotRated {
                reason: format!("{} is not graded by the equity pipeline", asset_class.label()),
            },
        };
        return Ok((verdict, audit(Default::default())));
    }

    // The deterministic engine: sub-scores, grade, targets — or an abstention.
    let engine_output = match engine::analyze(&dossier.financials) {
        EngineVerdict::Analyzed(out) => out,
        EngineVerdict::InsufficientEvidence(reason) => {
            let verdict = HoldingVerdict {
                symbol: symbol.clone(),
                asset_class,
                disposition: VerdictDisposition::InsufficientEvidence { reason },
            };
            return Ok((verdict, audit(Default::default())));
        }
    };

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
        })
        .context("interpreting the holding")?;

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
        financial_summary: interpretation.financial_summary,
        what_changed: interpretation.what_changed,
    };
    let verdict = HoldingVerdict {
        symbol: symbol.clone(),
        asset_class,
        disposition: VerdictDisposition::Graded(Box::new(graded)),
    };
    Ok((verdict, audit(engine_output.metrics.clone())))
}

// ---- Prompt construction (pure, testable) ------------------------------------

/// The system prompt for the interpretation stage — the role and the load-bearing
/// rule: read numbers from the engine, never invent them.
pub fn interpretation_system_prompt() -> String {
    "You are a disciplined equity analyst grading one holding for a prescriptive \
     portfolio review. The quantitative analysis — sub-scores, the composite grade, \
     valuation multiples, and the scenario price targets — has already been computed \
     deterministically and is given to you. Do NOT invent or alter any number: read \
     them from the analysis. Your job is the judgment the numbers don't make: choose \
     the action on the fixed ladder, set your conviction and the three horizon reads, \
     justify the base-case price target, and write a concise financial summary and a \
     continuity note. Apply the Market Signal house view and the investor profile. \
     Respond only with the required JSON object."
        .to_string()
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
        "\nCOMPUTED GRADE: {} (do not change)\nSUB-SCORES (0-100, higher better): quality {:.0}, valuation {:.0}, momentum {:.0}, risk {:.0}\n",
        e.grade.as_str(),
        e.sub_scores.quality,
        e.sub_scores.valuation,
        e.sub_scores.momentum,
        e.sub_scores.risk,
    ));

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

    if let Some(eoy) = &e.price_targets.end_of_year {
        p.push_str(&format!(
            "\nSCENARIO TARGETS (end of year): bear {:.2} / base {:.2} / bull {:.2}\n  methodology: {}\n",
            eoy.bear, eoy.base, eoy.bull, eoy.methodology
        ));
    }

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
        "\nINVESTOR PROFILE: risk tolerance {:?}, horizon {:?}, taxable {}, cash {:.0}\n",
        d.profile.risk_tolerance, d.profile.horizon, d.profile.tax_sensitive, d.profile.available_cash
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
        let action = match e.grade {
            crate::portfolio::Grade::A => Action::Add,
            crate::portfolio::Grade::B | crate::portfolio::Grade::C => Action::Hold,
            crate::portfolio::Grade::D => Action::Trim,
            crate::portfolio::Grade::F => Action::SellAll,
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

    fn model_ids(&self) -> Vec<String> {
        vec!["stub-analyst".to_string()]
    }
}

// ---- The live local analyst (Ollama daemon) ----------------------------------

/// The live [`HoldingAnalyst`]: wraps a [`LocalModelClient`] and the roster's reasoner
/// and fast model ids. Distillation runs on the fast model; interpretation runs on the
/// reasoner in thinking mode with the grammar-constrained interpretation schema, so the
/// returned object is structurally valid by construction.
pub struct LocalAnalyst {
    client: LocalModelClient,
    reasoner_model: String,
    fast_model: String,
}

impl LocalAnalyst {
    pub fn new(client: LocalModelClient, reasoner_model: String, fast_model: String) -> Self {
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
        req.format_schema = Some(interpretation_schema());
        req.think = true;
        // Stream silently: the structured stage has no console value, but accumulating
        // through the stream path keeps the reasoning on the tracker's thinking channel.
        let resp = self.client.chat_streaming(&req, StreamRole::Silent)?;
        serde_json::from_str(&resp.content)
            .with_context(|| format!("parsing interpretation JSON: {}", resp.content))
    }

    fn model_ids(&self) -> Vec<String> {
        vec![self.reasoner_model.clone(), self.fast_model.clone()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::engine::CompanyFinancials;
    use crate::portfolio::{AssetClass, InvestorProfile, OptionsSignal};
    use crate::portfolio::dossier::HouseView;
    use crate::schwab::Position;

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

    fn strong_financials() -> CompanyFinancials {
        CompanyFinancials {
            symbol: "AAPL".into(),
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

    fn dossier(asset_class: AssetClass, financials: CompanyFinancials) -> HoldingDossier {
        HoldingDossier {
            position: position(asset_class),
            financials,
            options_signal: OptionsSignal {
                put_call_volume: Some(1.2),
                put_call_open_interest: Some(1.1),
                implied_volatility: Some(0.3),
                iv_skew: Some(0.03),
            },
            profile: InvestorProfile::default_fixture(),
            house_view: HouseView::default(),
            prior_verdict: None,
            sources: vec!["FMP".into()],
        }
    }

    #[test]
    fn gradeable_holding_produces_a_graded_verdict_offline() {
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &dossier(AssetClass::Stock, strong_financials()), 29_500.0)
                .unwrap();
        match verdict.disposition {
            VerdictDisposition::Graded(g) => {
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
            }
            other => panic!("expected a graded verdict, got {other:?}"),
        }
        assert_eq!(audit.prompt_version, PROMPT_VERSION);
    }

    #[test]
    fn ineligible_asset_class_is_not_rated_without_a_model_call() {
        let (verdict, _audit) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::OptionContract, strong_financials()),
            29_500.0,
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
        let (verdict, _audit) =
            analyze_holding(&StubAnalyst, &dossier(AssetClass::Stock, thin), 29_500.0).unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::InsufficientEvidence { .. }
        ));
    }

    #[test]
    fn interpretation_prompt_carries_the_engine_numbers_and_the_do_not_invent_rule() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let input = InterpretationInput {
            dossier: &d,
            engine: &engine_output,
            distilled: "distilled findings",
        };
        let user = interpretation_user_prompt(&input);
        assert!(user.contains("COMPUTED GRADE"), "{user}");
        assert!(user.contains("SUB-SCORES"), "{user}");
        assert!(user.contains("NOT a grade input"), "options proxy is flagged: {user}");
        assert!(interpretation_system_prompt().contains("Do NOT invent"));
    }
}
