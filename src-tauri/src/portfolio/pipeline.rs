//! The per-holding pipeline (`docs/portfolio-analysis.md` §The per-holding pipeline):
//! dossier → engine → research → distill → thesis-document conversation → action
//! call → checkpoint. The model stages sit behind [`HoldingAnalyst`] so the pipeline
//! runs offline against [`StubAnalyst`] and live through [`LocalAnalyst`].

use anyhow::{Context, Result};

use crate::local_model::{options, ChatMessage, ChatRequest, LocalModelClient, StreamRole};
use crate::portfolio::dossier::HoldingDossier;
use crate::portfolio::engine::{self, EngineOutput, EngineVerdict, RateAnchors};
use crate::portfolio::factors::OWN_HISTORY_MIN_POINTS;
use crate::portfolio::fund::{self, FundEngineVerdict, FundStructuralKind, RoleRiskReadout};
use crate::portfolio::pre_profit::{self, PreProfitOverlay};
use crate::portfolio::soft_forensic::{LineLeg, SoftFlagState, SoftForensicFlags};
use crate::portfolio::{
    appendix_schema, Action, ActionSource, Conviction, ExposureWeight, GradedVerdict,
    HoldingAudit, HoldingVerdict, PositionChange, PricedModelArm, RoleRiskVerdict,
    ThesisAppendix, VerdictDisposition, PROMPT_VERSION,
};

use crate::portfolio::distill;
use crate::portfolio::research::{self, HoldingBrief, HoldingResearch, ResearchAuditRecord, ResearchPlan};
use crate::portfolio::review;

/// What the thesis-document conversation reads (`docs/portfolio-workflow.md`
/// §Step 6f). Carries no investor profile, position economics or action
/// machinery: the intrinsic verdict is profile-independent by input isolation,
/// and those live on [`ActionInput`] (`docs/portfolio-analysis.md` §Intrinsic
/// verdict).
pub struct ThesisInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub engine: &'a EngineOutput,
    pub rates: &'a RateAnchors,
    /// Written this run, or the prior record carried whole where the loop
    /// produced no write-up ([`distill::consolidate`]).
    pub analysis: crate::portfolio::AnalysisRecord,
    /// Present only where the stock entered the overlay.
    pub pre_profit: Option<&'a PreProfitOverlay>,
    /// `None` on a fund.
    pub soft_forensic: Option<&'a SoftForensicFlags>,
    /// Rendered only when fired; it asserts nothing about the cause.
    pub tech_pre_flag: Option<&'a engine::TechEventPreFlag>,
    pub narrative: Option<&'a engine::NarrativeRead>,
    /// `None` on a debut or where no split intervened.
    pub prior_split: Option<SplitContext>,
    /// The accuracy record reaches the document through this alone; `None` on
    /// a debut.
    pub review: Option<String>,
}

/// The split-context line above a verbatim prior document
/// (`docs/portfolio-workflow.md` §Step 6b); the document itself is never
/// rewritten.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitContext {
    /// The cumulative factor ([`engine::split_bridge_factor`]) that brings the
    /// document's prices to today's basis, and the split since the document's
    /// session where the feed carries one.
    Rebased {
        factor: f64,
        split: Option<SplitEvent>,
    },
    /// The document's anchor bar is missing from the fetched window, so whether
    /// a split intervened is unknown; the line says so because silence would
    /// read as "no split".
    Unverifiable,
}

/// What the role/risk thesis-document message reads; no appendix follows the
/// document on this branch.
pub struct RoleRiskInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub readout: &'a RoleRiskReadout,
    pub rates: &'a RateAnchors,
    /// Written this run, or the prior record carried whole
    /// ([`distill::consolidate`]).
    pub analysis: crate::portfolio::AnalysisRecord,
    pub prior_split: Option<SplitContext>,
    /// `None` on a debut.
    pub review: Option<String>,
}

/// The verdict evidence the per-holding action call reads. The `action` field
/// on the referenced verdict bodies is a placeholder at call time (the decision
/// overwrites it) and is deliberately never rendered.
pub enum ActionSubject<'a> {
    Priced {
        graded: &'a GradedVerdict,
        engine: &'a EngineOutput,
        pre_profit: Option<&'a PreProfitOverlay>,
    },
    RoleRisk {
        verdict: &'a RoleRiskVerdict,
    },
}

/// What the per-holding action call reads (`docs/portfolio-analysis.md`
/// §Portfolio action). The investor profile's only entry point into the job,
/// so the thesis document stays profile-blind by input isolation; no
/// whole-book context exists here by design.
pub struct ActionInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub subject: ActionSubject<'a>,
    /// [`engine::feasible_actions`] for a priced holding;
    /// [`crate::portfolio::ROLE_RISK_ACTIONS`] for the role/risk branch.
    pub engine_set: &'a [Action],
    pub profile: &'a crate::portfolio::InvestorProfile,
}

/// The audit annotation for a chosen rung outside the engine's action set — the
/// choice persists as authored; engine evidence annotates, never bars
/// (`docs/portfolio-analysis.md` §Portfolio action).
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

/// The audit's source line for the FRED rate anchors, appended only where the
/// engine computed from them or a prompt rendered them, never on a no-model
/// exit.
pub const RATE_ANCHORS_SOURCE: &str =
    "FRED rate anchors (DGS10 / DGS2 + anchor-window history)";

/// The app-composed tax caveat appended to the rationale after the rung is
/// fixed (`docs/portfolio-analysis.md` §Portfolio action): the tax posture never
/// enters the decision — the packet carries no tax row and no P/L.
pub const TAX_CAVEAT_GAIN: &str = "Tax note: this position carries an unrealized gain, so \
realizing part or all of it may carry a tax cost; account type, tax lots, holding periods \
and rates are unmodeled.";
pub const TAX_CAVEAT_LOSS: &str = "Tax note: this position carries an unrealized loss, so \
realizing it may carry a tax benefit; account type, tax lots, holding periods and rates are \
unmodeled.";

/// `None` where nothing is realized, under a tax-exempt profile, at break-even,
/// or with no reported cost basis: an exactly-zero basis is an unreported one
/// (the adapter maps a missing `averagePrice` to zero), so its gain is
/// undefined rather than the whole market value (`docs/portfolio-analysis.md`
/// §Storage and display). A negative netted basis keeps its caveat.
pub fn tax_caveat(
    profile: &crate::portfolio::InvestorProfile,
    position: &crate::schwab::Position,
    action: Action,
) -> Option<&'static str> {
    if !profile.tax_sensitive || !matches!(action, Action::Trim | Action::SellAll) {
        return None;
    }
    if position.cost_basis == 0.0 {
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

/// The rationale less the app's tax caveat. PRIOR ACTION renders this form so a
/// prior trim or sell cannot re-supply the tax posture or the P/L sign to the
/// next intrinsic interpretation. The caveat is a fixed sentence joined by one
/// space (`with_tax_caveat`), so the strip is exact.
pub fn investment_sentence(rationale: &str) -> &str {
    for caveat in [TAX_CAVEAT_GAIN, TAX_CAVEAT_LOSS] {
        if let Some(sentence) = rationale.strip_suffix(caveat) {
            return sentence.trim_end();
        }
    }
    rationale
}

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

/// The response contract says the rationale is one sentence and never empty
/// (`docs/portfolio-analysis.md` §Portfolio action); the schema only types it as
/// a string, so the nonempty half is enforced here. The one-sentence shape is a
/// prompt preference, not validated.
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

/// The model-backed stages, behind a trait so the orchestration is stub-driven
/// offline and daemon-driven live. Research and the self-review carry offline
/// defaults; consolidation's two calls default to a refusal, since a stub with
/// no write-up is never asked to consolidate.
pub trait HoldingAnalyst {
    /// Step 6d's distillation call ([`distill::distillation_prompt`]):
    /// non-thinking, no grammar, prose.
    fn distill(&self, input: &distill::DistillInput<'_>) -> Result<String> {
        anyhow::bail!("{}: this analyst issues no distillation call", input.stage)
    }
    /// Step 6d's analysis call ([`distill::analysis_prompt`]): thinking, no
    /// grammar, prose.
    fn analyze(&self, input: &distill::AnalysisInput<'_>) -> Result<String> {
        anyhow::bail!("{}: this analyst issues no analysis call", input.stage())
    }
    /// Step 6c (`docs/portfolio-workflow.md` §Step 6c); defaults to the offline
    /// stub.
    fn research(
        &self,
        _dossier: &HoldingDossier,
        plan: &ResearchPlan,
    ) -> Result<HoldingResearch> {
        Ok(research::offline_stub(plan))
    }
    /// Step 6e ([`review::review_prompt`]), on a continuity run only: thinking,
    /// no grammar, prose. Defaults to [`review::stub_review`].
    fn review(&self, input: &review::ReviewInput) -> Result<String> {
        Ok(review::stub_review(input))
    }
    /// Step 6f (`docs/portfolio-workflow.md` §Step 6f): the document as prose
    /// under thinking with no grammar, then the appendix transcribed in the
    /// same conversation's second, non-thinking message under the grammar.
    /// Returns the model arm as authored, type-checked only.
    fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm>;
    /// The thesis document alone for a structurally unpriceable vehicle: no
    /// prices, no conviction, no appendix (`docs/portfolio-analysis.md`
    /// §Intrinsic verdict).
    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String>;
    /// The per-holding action call (`docs/portfolio-analysis.md` §Portfolio
    /// action): rung-only, from the holding's own verdict plus the investor
    /// profile, no book context.
    fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision>;
    /// The configured distillation-tier id; only the fallback for analysts
    /// without [`Self::take_model_calls`].
    fn fast_id(&self) -> String;
    /// The model id the reasoner calls run on; recorded on the audit only after
    /// one actually ran.
    fn reasoner_id(&self) -> String;
    /// Drain the model ids of outbound calls since the last drain, in issue
    /// order. `Some` means exact call telemetry (an empty vector included);
    /// `None` keeps stubs on the configured-id path.
    fn take_model_calls(&self) -> Option<Vec<String>> {
        None
    }
    /// Drain the prompt-size observations since the last drain — the
    /// data-health context-fit read (`docs/portfolio-analysis.md` §Portfolio
    /// roll-up). Drained at each holding's checkpoint boundary so its rows ride
    /// its checkpoint row.
    fn take_prompt_usage(&self) -> Vec<crate::local_model::PromptUsage> {
        Vec::new()
    }
    /// Drain the fired bounded-retry records since the last drain
    /// (`docs/local-models.md §The local-model adapter seam`), at the same
    /// boundary as the usage.
    fn take_retry_events(&self) -> Vec<crate::local_model::RetryEvent> {
        Vec::new()
    }
}

/// The holding-constant brief the research loop leads every gathering message
/// with (`docs/portfolio-workflow.md` §Step 6c). FETCHED VALUES is the thesis
/// message's rendering byte for byte. The thesis document's split line is the
/// verdict bridge's (`prior_split`); the analysis's comes from its own anchor
/// bar.
pub(crate) fn research_brief(
    dossier: &HoldingDossier,
    rates: &RateAnchors,
    prior_split: Option<SplitContext>,
) -> HoldingBrief {
    HoldingBrief {
        header: holding_header(dossier),
        fetched_values: fetched_values_section(dossier, rates),
        leads: dossier.news_leads.clone(),
        prior_documents: format!(
            "{}{}",
            prior_analysis_section(dossier),
            prior_thesis_section(dossier, prior_split)
        ),
    }
}

/// The split-bridge anchor bar (`docs/portfolio-analysis.md` §Starting
/// parameters): the newest close strictly before the run's ET session. A later
/// pass re-reads the same bar date, and the close ratio is the cumulative
/// split re-basis between the two fetches ([`engine::split_bridge_factor`]).
fn authoring_close_of(d: &HoldingDossier, run_date: &str) -> Option<engine::DatedValue> {
    d.financials
        .daily_closes
        .iter()
        .rev()
        .find(|c| c.date.as_str() < run_date)
        .cloned()
}

/// A split the feed carries, named by the split-context line
/// (`docs/portfolio-workflow.md` §Step 6b).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitEvent {
    pub date: chrono::NaiveDate,
    pub numerator: f64,
    pub denominator: f64,
}

/// The newest split the evidence feed carries dated after `since` (an ISO
/// session date).
fn split_event_since(d: &HoldingDossier, since: &str) -> Option<SplitEvent> {
    d.evidence
        .as_ref()?
        .splits
        .iter()
        .filter(|s| s.date.as_str() > since)
        .max_by(|a, b| a.date.cmp(&b.date))
        .and_then(|s| {
            let date = chrono::NaiveDate::parse_from_str(&s.date, "%Y-%m-%d").ok()?;
            Some(SplitEvent {
                date,
                numerator: s.numerator,
                denominator: s.denominator,
            })
        })
}

/// A prior document's split context from its bridge factor
/// (`docs/portfolio-workflow.md` §Step 6b).
fn split_context_of(bridge: Option<f64>, split: Option<SplitEvent>) -> Option<SplitContext> {
    match bridge {
        Some(f) if f != 1.0 => Some(SplitContext::Rebased { factor: f, split }),
        Some(_) => None,
        None => Some(SplitContext::Unverifiable),
    }
}

/// Steps 6c and 6d for one holding. An analysis written this run is stamped
/// with this run's session date and anchor bar; a carried one keeps the prior
/// record whole.
fn run_research(
    analyst: &dyn HoldingAnalyst,
    dossier: &HoldingDossier,
    triggers: &research::AgendaTriggers,
    rates: &RateAnchors,
    prior_split: Option<SplitContext>,
    run_date: &str,
) -> Result<(crate::portfolio::AnalysisRecord, ResearchAuditRecord)> {
    let plan = ResearchPlan {
        agenda: research::build_agenda(dossier, triggers),
        brief: research_brief(dossier, rates, prior_split),
        // The loop's rows land on the step the job already opened for this symbol.
        step_label: crate::portfolio::holding_step_key(&dossier.position.symbol),
    };
    let research_out = analyst
        .research(dossier, &plan)
        .context("researching the holding")?;
    let consolidated = distill::consolidate(
        analyst,
        &dossier.position.symbol,
        &plan.brief,
        &research_out,
        dossier.prior_analysis.as_ref(),
        &prior_analysis_section(dossier),
        &distill::ConsolidationBudgets::live(),
    )
    .context("consolidating the holding's research")?;
    let analysis = match consolidated.analysis {
        distill::Consolidated::Written(text) => crate::portfolio::AnalysisRecord {
            text,
            written: dossier.analysis_date.clone(),
            anchor: authoring_close_of(dossier, run_date),
        },
        distill::Consolidated::Carried(record) => record,
    };
    Ok((
        analysis,
        ResearchAuditRecord::from_research(&research_out, consolidated.distillation),
    ))
}

/// Run one holding through the pipeline end to end. Eligibility and the evidence
/// floor short-circuit before any model call — an ineligible asset class is
/// `not-rated`, a holding below the floor `insufficient-evidence` — so the model
/// only ever interprets a holding the engine could grade. No book input reaches
/// it.
pub fn analyze_holding(
    analyst: &dyn HoldingAnalyst,
    dossier: &HoldingDossier,
    rates: &RateAnchors,
    run_date: &str,
) -> Result<(HoldingVerdict, HoldingAudit)> {
    let symbol = dossier.position.symbol.clone();
    let asset_class = dossier.position.asset_class;
    let is_fund = matches!(
        asset_class,
        crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
    );
    // App-set from the holdings diff, never the model; carried on every verdict.
    let position_change = dossier.position_delta.change;
    let basis_move = crate::portfolio::diff::basis_move(&dossier.position_delta, &dossier.position);
    let mut degraded = dossier.financials.gaps.clone();
    if let Some(f) = &dossier.fund {
        degraded.extend(f.fund.gaps.iter().cloned());
    }
    // A failed DGS10 history degrades rather than fails the run
    // (`docs/portfolio-analysis.md` §Starting parameters).
    if let Some(gap) = &rates.history_gap {
        degraded.push(gap.clone());
    }
    // An unverified listing resolution proceeds as a degraded input — an FMP
    // outage must never mass-not-rate a book (`docs/portfolio-analysis.md`
    // §Asset eligibility).
    if let Some(crate::portfolio::listing::ListingResolution::Unverified { detail }) =
        &dossier.listing
    {
        degraded.push(format!("listing-resolution guard unverified — {detail}"));
    }
    // The split-adjustment bridge for prior price comparisons
    // (`docs/portfolio-analysis.md` §Starting parameters). `Some(1.0)`:
    // unchanged, or a prior with no anchor. `None`: the anchor's bar is missing
    // from the fresh window, so price-denominated prior comparisons are
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
    let prior_session = dossier
        .prior_vintage
        .as_deref()
        .and_then(crate::market_clock::et_date_of)
        .map(|day| day.format("%Y-%m-%d").to_string());
    let prior_split = split_context_of(
        price_bridge,
        prior_session
            .as_deref()
            .and_then(|since| split_event_since(dossier, since)),
    );
    // A continuity run (a prior thesis document, an abstention's retained one
    // included) takes the self-review (`docs/portfolio-workflow.md` §Step 6e).
    let has_prior_document = dossier
        .prior_verdict
        .as_ref()
        .and_then(|v| v.thesis_document())
        .is_some();
    // The prior spot and every consensus-EPS period scale together so their
    // ratio (the prior matched-period multiple) stays basis-free; a `None`
    // factor drops both rather than running cross-basis.
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
    // Computed on either verdict branch (`docs/portfolio-analysis.md` §Starting
    // parameters).
    let fund_exposure = dossier
        .fund
        .as_ref()
        .map(|f| crate::portfolio::fund::exposure_basis(&f.fund));
    // Whether the verdict actually received house-view content: `false` on
    // every route that returns before a thesis-document call; each thesis path
    // sets it from the predicate of the prompt it is about to build.
    let house_view_consulted = std::cell::Cell::new(false);
    // Whether the FRED rate anchors actually fed this verdict; an earlier exit
    // renders no prompt and computes nothing from them, so its audit must not
    // name them.
    let rates_consulted = std::cell::Cell::new(false);
    // Each feed is recorded only where a prompt actually rendered it (the same
    // discipline as the house view); the benchmark series only where the
    // technology-event pre-flag read it.
    let backdrop_consulted = std::cell::Cell::new(false);
    let positioning_consulted = std::cell::Cell::new(false);
    let benchmark_consulted = std::cell::Cell::new(false);
    let commodity_consulted = std::cell::Cell::new(false);
    let short_interest_consulted = std::cell::Cell::new(false);
    let ma_consulted = std::cell::Cell::new(false);
    // The model ids this verdict was actually authored with, in first-call
    // order; duplicates collapse without disturbing their first-call position.
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
    // Both audit construction sites go through this — the closure below for
    // every early return, and the priced path's own record — so a source claim
    // cannot drift between them.
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
        if ma_consulted.get() {
            sources.push("FMP M&A feed (matched as acquirer or target)".to_string());
        }
        if benchmark_consulted.get() {
            sources.push(
                "FMP sector benchmark series (technology-event pre-flag)".to_string(),
            );
        }
        sources
    };
    // Strictly before the run's session keeps the anchor off the still-forming
    // bar, so a re-fetch reads back the identical close unless the series was
    // re-based. Every pass that writes stamps its own; only an abstention
    // carries the retained document's forward (`abstain` below)
    // (`docs/portfolio-analysis.md` §Starting parameters).
    let authoring_close = authoring_close_of(dossier, run_date);
    let audit = |metrics, target_meta, pre_profit, soft_forensic| HoldingAudit {
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
        factors: None,
        quick_basis: None,
        authoring_close: authoring_close.clone(),
        fund_exposure: fund_exposure.clone(),
        pre_profit,
        hurdle: None,
        // The matched hard rule is stamped only on the priced path, where its
        // engine consequences were computed.
        forensic: dossier
            .filing_events
            .clone()
            .map(|state| crate::portfolio::ForensicRead {
                matched_rule: None,
                state,
            }),
        soft_forensic,
        // Evaluated on the priced path only (it reads the engine's volatility).
        tech_event_pre_flag: None,
        // Recorded wherever the gather resolved it; its source label stays
        // render-scoped.
        short_interest: dossier.short_interest.clone(),
        implied_expectations: None,
        narrative: None,
        option_overlay: dossier.option_overlay.clone(),
        research: None,
        analysis: None,
        review: None,
        accuracy_read_through: None,
    };
    let abstain = |reason: String, metrics, meta, pre_profit, soft_forensic| {
        // A below-floor exit retains the prior thesis document unrewritten so
        // the next continuity run still reads it (`docs/portfolio-analysis.md`
        // §Evidence floor).
        let retained = dossier
            .prior_verdict
            .as_ref()
            .and_then(|v| v.thesis_document().map(str::to_string));
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            basis_move: basis_move.clone(),
            disposition: VerdictDisposition::InsufficientEvidence {
                reason,
                prior_thesis_document: retained.clone(),
            },
            // The job layer stamps vintages: a fresh pass gets the run's
            // `created_at`, an abstention keeps its prior vintage.
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
            side_reversed: false,
        };
        let mut record = audit(metrics, meta, pre_profit, soft_forensic);
        // The retained document stays on the basis it was written, so the row
        // carries its own anchor bar: a fresh stamp would certify pre-split
        // prices on today's basis, and the next pass could never re-detect the
        // mismatch.
        if retained.is_some() {
            record.authoring_close = dossier.prior_authoring_close.clone();
        }
        // The prior analysis is retained the same way (`docs/portfolio-analysis.md`
        // §Evidence floor) — unconditionally, since it has no price basis to
        // certify.
        record.analysis = dossier.prior_analysis.clone();
        // The read-through mark travels with the retained document
        // (`docs/portfolio-analysis.md` §Outcome learning).
        record.accuracy_read_through = dossier.prior_accuracy_read_through;
        Ok((verdict, record))
    };
    let not_rated = |reason: String| {
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            basis_move: basis_move.clone(),
            disposition: VerdictDisposition::NotRated { reason },
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
            side_reversed: false,
        };
        Ok((verdict, audit(Default::default(), None, None, None)))
    };

    if !asset_class.is_gradeable() {
        return not_rated(format!(
            "{} is not graded by the equity pipeline",
            asset_class.label()
        ));
    }

    // A net-short or fully offset position is not-rated: the ladder reads long
    // (`docs/portfolio-analysis.md` §Asset eligibility).
    if dossier.position.quantity <= 0.0 {
        let reason = if dossier.position.quantity < 0.0 {
            "held net short — the ladder's long-side semantics don't apply; \
             weighing the signed exposure against the book is the future \
             portfolio planner's work"
        } else {
            "fully offset — the netted position is zero shares, so there is no \
             economic exposure for the long-side ladder to act on"
        };
        return not_rated(reason.to_string());
    }

    // The listing-resolution guard, stocks only (`docs/portfolio-analysis.md`
    // §Asset eligibility): no canonical resolution or a non-US primary listing
    // is a structural can't-grade; a conflicting issuer identity is the
    // evidence floor's arm instead, a data problem that may be transient.
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
            return not_rated(reason);
        }
        if let Some(crate::portfolio::listing::ListingResolution::Conflict { fmp_name }) =
            &dossier.listing
        {
            // The overlay and soft flags survive the guard exit as they do the
            // floor's; the skip fetched no statements, so the record reads
            // unscorable with its gaps (`docs/storage.md` §Local Analysis Suite
            // Storage).
            let pre_profit = crate::portfolio::pre_profit::compute_overlay(&dossier.financials);
            let soft_forensic = crate::portfolio::soft_forensic::compute(&dossier.financials);
            return abstain(
                format!(
                    "conflicting identity — FMP resolves this symbol to \"{fmp_name}\", \
                     which does not match the account's \"{}\"",
                    dossier.position.description
                ),
                Default::default(),
                None,
                Some(pre_profit),
                Some(soft_forensic),
            );
        }
    }

    // The engine stage (`docs/portfolio-workflow.md` §Step 6b).
    let mut pre_profit_overlay: Option<PreProfitOverlay> = None;
    let mut soft_forensic_flags: Option<crate::portfolio::soft_forensic::SoftForensicFlags> = None;
    // Nothing rewrites the engine output after Step 6b.
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
                return abstain(reason, Default::default(), None, None, None);
            }
            FundEngineVerdict::RoleRiskOnly(readout) => {
                let fund_metrics = fund_metrics(&readout, &dossier.financials);
                // The role/risk branch: the full ladder stays open to the action
                // call below; the engine arm's reduced set rides as annotated
                // evidence (`docs/portfolio-analysis.md` §Portfolio action).
                house_view_consulted.set(prompt_renders_house_view(dossier));
                backdrop_consulted.set(dossier.put_call_backdrop.is_some());
                positioning_consulted
                    .set(dossier.fund.as_ref().is_some_and(|f| f.positioning.is_some()));
                // The brief states the Treasury prints under FETCHED VALUES, so
                // the source is consulted on this branch too.
                rates_consulted.set(true);
                let (rr_analysis, rr_research_record) = run_research(
                    analyst,
                    dossier,
                    &research::AgendaTriggers::default(),
                    rates,
                    prior_split,
                    run_date,
                )?;
                record_stage_models(analyst.reasoner_id());
                // The self-review on a continuity run (`docs/portfolio-workflow.md`
                // §Step 6e); no accuracy record, since the branch states no price.
                let rr_review = if has_prior_document {
                    let text = analyst
                        .review(&review::ReviewInput {
                            dossier,
                            rates,
                            analysis: &rr_analysis,
                            prior_split,
                            price_bridge,
                            subject: review::ReviewSubject::RoleRisk,
                        })
                        .context("reviewing the role/risk holding's prior position")?;
                    record_stage_models(analyst.reasoner_id());
                    Some(text)
                } else {
                    None
                };
                let thesis_document = analyst
                    .interpret_role_risk(&RoleRiskInput {
                        dossier,
                        readout: &readout,
                        rates,
                        analysis: rr_analysis.clone(),
                        prior_split,
                        review: rr_review.clone(),
                    })
                    .context("writing the role/risk holding's thesis document")?;
                record_stage_models(analyst.reasoner_id());
                let mut rr = role_risk_verdict_from_model_arm(&readout, thesis_document);
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
                rr.action_rationale = with_tax_caveat(
                    decision.rationale,
                    &dossier.profile,
                    &dossier.position,
                    decision.action,
                );
                // The branch's computed surface persists as the audit's metrics,
                // never the empty default.
                let mut audit_record = audit(fund_metrics, None, None, None);
                audit_record.research = Some(rr_research_record);
                audit_record.analysis = Some(rr_analysis);
                audit_record.review = rr_review;
                audit_record.action_annotations.extend(outside_set_annotation(
                    decision.action,
                    &crate::portfolio::ROLE_RISK_ACTIONS,
                ));
                let verdict = HoldingVerdict {
                    symbol: symbol.clone(),
                    asset_class,
                    position_change,
                    basis_move: basis_move.clone(),
                    disposition: VerdictDisposition::RoleRiskOnly(Box::new(rr)),
                    analyzed_at: None,
                    action_source: ActionSource::ModelChosen,
                    side_reversed: false,
                };
                return Ok((verdict, audit_record));
            }
        }
    } else {
        // Computed for every stock: the eligibility result persists even when
        // the stock does not enter (`docs/portfolio-workflow.md` §Step 6b).
        pre_profit_overlay = Some(pre_profit::compute_overlay(&dossier.financials));
        soft_forensic_flags = Some(crate::portfolio::soft_forensic::compute(&dossier.financials));
        let issuer = dossier.issuer.as_ref();
        let profile = engine::GradeProfile {
            sector: issuer.and_then(|i| i.sector.as_deref()),
            industry: issuer.and_then(|i| i.industry.as_deref()),
        };
        match engine::analyze(&dossier.financials, rates, profile) {
            EngineVerdict::Analyzed(out) => out,
            EngineVerdict::InsufficientEvidence(reason) => {
                return abstain(
                    reason,
                    Default::default(),
                    None,
                    pre_profit_overlay,
                    soft_forensic_flags,
                );
            }
        }
    };
    // Only a priced engine output computes from the rate anchors.
    rates_consulted.set(true);

    // The technology-event pre-flag (`docs/portfolio-analysis.md` §Starting
    // parameters): an unevaluable read records its reason, never a fired or
    // clear flag; a debut has nothing to diff, so it is neither a flag nor a
    // gap.
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
                        // Only here is the series actually handed to the evaluation.
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

    // The narrative-vs-reality read (`docs/portfolio-analysis.md` §Starting
    // parameters): an unreadable pace on a carried holding records its reason,
    // a debut records nothing. A tripped read is evidence the model weighs,
    // never a clamp on either arm.
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
    house_view_consulted.set(prompt_renders_house_view(dossier));
    backdrop_consulted.set(dossier.put_call_backdrop.is_some());
    positioning_consulted.set(dossier.fund.as_ref().is_some_and(|f| f.positioning.is_some()));
    commodity_consulted.set(!dossier.commodity_context.is_empty());
    short_interest_consulted.set(dossier.short_interest.is_some());
    ma_consulted.set(!dossier.ma_matches.is_empty());
    // The conditional topics' triggers (`docs/portfolio-workflow.md` §Step 6c);
    // the news leads ride the brief and trigger nothing.
    let triggers = research::AgendaTriggers {
        tech_pre_flag_fired: tech_pre_flag.as_ref().is_some_and(|f| f.fired),
        overlay_eligible: pre_profit_overlay.as_ref().is_some_and(|o| o.is_eligible()),
    };
    let (analysis, research_record) =
        run_research(analyst, dossier, &triggers, rates, prior_split, run_date)?;
    record_stage_models(analyst.reasoner_id());

    // The self-review (`docs/portfolio-workflow.md` §Step 6e) precedes the
    // thesis document; the accuracy record reaches the model nowhere else.
    let review_text = if has_prior_document {
        let text = analyst
            .review(&review::ReviewInput {
                dossier,
                rates,
                analysis: &analysis,
                prior_split,
                price_bridge,
                subject: review::ReviewSubject::Priced { engine: &engine_output },
            })
            .context("reviewing the prior position")?;
        record_stage_models(analyst.reasoner_id());
        Some(text)
    } else {
        None
    };
    // The read-through mark (`docs/portfolio-analysis.md` §Outcome learning):
    // the prior mark carries where the store could not be read.
    let accuracy_read_through = match (&review_text, &dossier.accuracy) {
        (None, _) => None,
        (Some(_), Some(acc)) => acc.high_water,
        (Some(_), None) => dossier.prior_accuracy_read_through,
    };

    // The hard-forensic state trips from the classified filing kinds alone
    // (`docs/portfolio-analysis.md` §Starting parameters); a fraud allegation
    // reaches the model only as prose through the write-ups.
    let filing_state = dossier.filing_events.clone();
    let hard_forensic = filing_state
        .as_ref()
        .map(crate::portfolio::ForensicFilingState::hard_tripped)
        .unwrap_or(false);

    // The overlay's rules bind the engine's own rung and action set below only
    // where the stock entered the overlay (`docs/portfolio-workflow.md` §Step
    // 6b).
    let overlay_rules = pre_profit_overlay
        .as_ref()
        .filter(|o| o.is_eligible())
        .map(|o| &o.consequences);
    // Step 6f (`docs/portfolio-workflow.md` §Step 6f).
    let model_arm = analyst
        .interpret(&ThesisInput {
            dossier,
            engine: &engine_output,
            rates,
            analysis: analysis.clone(),
            pre_profit: pre_profit_overlay.as_ref().filter(|o| o.is_eligible()),
            soft_forensic: soft_forensic_flags.as_ref(),
            tech_pre_flag: tech_pre_flag.as_ref(),
            narrative: narrative.as_ref(),
            prior_split,
            review: review_text.clone(),
        })
        .context("writing the holding's thesis document")?;
    record_stage_models(analyst.reasoner_id());

    // The authored band relation is on the pass's own basis, like its band and
    // its anchor (`docs/portfolio-analysis.md` §Starting parameters; §The quick
    // check).
    let engine_rung =
        engine::engine_action(engine_output.grade, &engine_output.hurdle, overlay_rules, hard_forensic);
    let authored_band_relation = authored_band_relation(
        dossier.financials.current_price,
        engine_output.price_targets.twelve_month.as_ref(),
    );
    let mut graded = graded_verdict_from_model_arm(
        &engine_output,
        dossier.options_signal.clone(),
        model_arm,
        engine_rung,
        authored_band_relation,
    );
    // The per-holding action call (`docs/portfolio-analysis.md` §Portfolio
    // action).
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
        basis_move,
        disposition: VerdictDisposition::Priced(Box::new(graded)),
        analyzed_at: None,
        action_source: ActionSource::ModelChosen,
        side_reversed: false,
    };
    let mut degraded_inputs = degraded.clone();
    degraded_inputs.extend(engine_output.tier_gaps.iter().cloned());
    degraded_inputs.extend(tech_pre_flag_gap.clone());
    degraded_inputs.extend(narrative_gap.clone());
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
        factors: engine_output.factors.clone(),
        // Persisted on every priced pass, so the sweep's conversions read one
        // basis.
        quick_basis: engine_output.quick_basis.clone(),
        authoring_close: authoring_close.clone(),
        fund_exposure: fund_exposure.clone(),
        pre_profit: pre_profit_overlay,
        soft_forensic: soft_forensic_flags,
        // The hurdle basis behind the verdict's dead-money state
        // (`docs/storage.md` §Local Analysis Suite Storage).
        hurdle: Some(engine_output.hurdle.clone()),
        forensic: filing_state
            .clone()
            .map(|state| crate::portfolio::ForensicRead {
                matched_rule: hard_forensic.then(|| {
                    "hard forensic trigger: add family barred from the engine action set; \
                     the engine's own rung reads the exit family"
                        .to_string()
                }),
                state,
            }),
        tech_event_pre_flag: tech_pre_flag,
        short_interest: dossier.short_interest.clone(),
        implied_expectations: engine_output.implied_expectations.clone(),
        narrative,
        option_overlay: dossier.option_overlay.clone(),
        research: Some(research_record),
        analysis: Some(analysis),
        review: review_text,
        accuracy_read_through,
    };
    Ok((verdict, audit_record))
}

// ---- Prompt construction (pure, testable) ------------------------------------

/// The system prompt of the thesis-document conversation
/// (`docs/portfolio-workflow.md` §Step 6f): the role and the message's shape,
/// no app concept (`docs/local-models.md` §Prompt posture). The same system
/// message heads the second, appendix message.
pub fn thesis_system_prompt(is_fund: bool) -> String {
    format!(
        "You are an {} analyst writing the thesis document for one holding in a portfolio \
         review. Part 1 of the message gives the inputs. Part 2 says what the document covers, \
         in order, and how to return it.",
        if is_fund { "investment" } else { "equity" }
    )
}

/// As [`thesis_system_prompt`], the vehicle named as a fund since this branch
/// is a fund by construction.
pub fn role_risk_system_prompt() -> String {
    "You are an investment analyst writing the thesis document for one fund holding in a \
     portfolio review. Part 1 of the message gives the inputs. Part 2 says what the document \
     covers, in order, and how to return it."
        .to_string()
}

pub(crate) fn dossier_is_fund(d: &HoldingDossier) -> bool {
    matches!(
        d.position.asset_class,
        crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
    )
}

/// Whether an interpretation message will render any market-analysis content.
/// Both branches render through one [`market_analysis_section`], so the audit's
/// house-view source claim reads the one predicate the render reads; a
/// summary-only house view (what `load_house_view` degrades to) still renders
/// as the stance lines.
pub(crate) fn prompt_renders_house_view(d: &HoldingDossier) -> bool {
    d.house_view.latest_sections.is_some() || !d.house_view.recent_summaries.is_empty()
}

/// The holding header every model-facing packet opens with. The name falls back
/// to the resolved listing's company name when Schwab supplies no description,
/// which would otherwise render as `HOLDING: PSX ()`. It carries no account
/// economics on any route — the intrinsic read is of no investor
/// (`docs/portfolio-analysis.md` §Intrinsic verdict) — and closes with the
/// analysis date so every packet anchors its period labels to the date of the
/// analysis rather than the model's training horizon.
pub(crate) fn holding_header(d: &HoldingDossier) -> String {
    format!(
        "HOLDING\n{} ({}).\nPrice: {}\nDate: {}.\n",
        d.position.symbol,
        holding_display_name(d),
        spot_line(d),
        d.analysis_date,
    )
}

/// "$358.97 per share." or "(gap)". It carries no date: the quote and the dated
/// daily closes are separate requests, so the last close's date is not the
/// quote's.
fn spot_line(d: &HoldingDossier) -> String {
    d.financials
        .current_price
        .filter(|p| p.is_finite() && *p > 0.0)
        .map(|p| format!("${p:.2} per share."))
        .unwrap_or_else(|| "(gap)".into())
}

fn holding_display_name(d: &HoldingDossier) -> &str {
    let described = d.position.description.trim();
    if !crate::portfolio::listing::describes_issuer(described, &d.position.symbol) {
        // The fallback is held to a canonical-source standard, deliberately
        // looser than the description's: a ticker-token-only legal name ("ASML
        // Holding N.V.", "eBay Inc.") is real identity from a canonical source,
        // and the stricter rule starved ticker-named issuers of any name. A
        // fund's identity rides the fund data's own name, the role-risk
        // branch's only naming source.
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

/// Built once for the audit and the message, so the two read one surface.
pub(crate) fn fund_metrics(
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

/// The role/risk thesis-document message (`docs/portfolio-workflow.md` §Step
/// 6f): Part 1 is inputs only, each section explained once and then its
/// values, with no instruction in it; Part 2 is the task only. The model
/// receives data, never a description of the app, and no position economics
/// (`docs/portfolio-analysis.md` §Intrinsic verdict).
pub fn role_risk_user_prompt(input: &RoleRiskInput) -> String {
    let d = input.dossier;
    let r = input.readout;
    let mut p = String::from("======== PART 1: INPUTS ========\n");

    p.push_str(&holding_header(d));

    p.push_str(&fetched_values_section(d, input.rates));

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
    // Renders only where the vehicle makes it meaningful; its absence is
    // already a named gap (`docs/portfolio-analysis.md` §Asset eligibility).
    if r.is_cef {
        if let Some(prem) = r.nav_premium {
            p.push('\n');
            p.push_str(&nav_premium_line(prem));
        }
    }
    if let Some(f) = &d.fund {
        p.push_str(&positioning_prompt_section(f));
    }

    // The daily volatility is a COMPUTED line, so it renders once.
    p.push_str(&format!(
        "\nRISK PROFILE\nAnnualized realized volatility: {} (a fraction; 0.14 means 14% a \
         year).\n",
        opt(r.observable_risk),
    ));
    p.push_str(&put_call_backdrop_prompt_section(d));

    let has_gaps = !r.evidence_gaps.is_empty();
    if has_gaps {
        p.push_str(&format!("\nEVIDENCE GAPS\n{}\n", r.evidence_gaps.join("; ")));
    }

    let metrics = fund_metrics(r, &d.financials);
    p.push_str("\nCOMPUTED\nThe computed metrics, each with its unit.\n");
    p.push_str(&computed_metrics_lines(true, &metrics));

    p.push_str(&market_analysis_section(d));

    p.push_str(&analysis_section(d, &input.analysis));

    p.push_str(&review_section(input.review.as_deref()));

    let has_prior = d.prior_verdict.as_ref().and_then(|v| v.thesis_document()).is_some();
    p.push_str(&prior_thesis_section(d, input.prior_split));

    p.push_str(&role_risk_task_section(has_tilt, has_gaps, has_prior, input.review.is_some()));
    p
}

fn role_risk_task_section(has_tilt: bool, has_gaps: bool, has_prior: bool, has_review: bool) -> String {
    let mut sections = vec!["CLASS"];
    if has_tilt {
        sections.push("EXPOSURE TILT");
    }
    sections.push("RISK PROFILE");
    if has_gaps {
        sections.push("EVIDENCE GAPS");
    }
    sections.extend(["FETCHED VALUES", "COMPUTED", "ANALYSIS"]);
    let (last, head) = sections.split_last().expect("at least two sections");
    let mut p = String::from(
        "\n======== PART 2: TASK ========\n\n\
         Write the thesis document for this holding as plain text — no code fence, no JSON, no \
         heading before the first line. It covers, in this order:\n",
    );
    p.push_str(&format!(
        "\n1. The role — the mandate and the exposure the vehicle exists to supply, and the cost \
         and risk of holding it, from {}, {last} and MARKET ANALYSIS.\n",
        head.join(", ")
    ));
    p.push_str("\n2. The risks — what could impair the role or the return path.\n");
    p.push_str(
        "\n3. The triggers for trimming or selling — each a concrete measure, a level and a \
         period.\n",
    );
    p.push_str(&format!(
        "\n4. A summary paragraph — the read as a whole{}.\n",
        continuity_clause(has_prior, has_review)
    ));
    p.push_str(&format!(
        "\nThe document states no expected price and no conviction. It runs {} to {} words.\n",
        fmt_thousands(THESIS_DOCUMENT_WORDS.0),
        fmt_thousands(THESIS_DOCUMENT_WORDS.1)
    ));
    p
}

/// The thesis document's drafted length band, stated in the prompt and never
/// checked by the app (`docs/portfolio-analysis.md` §Starting parameters).
pub const THESIS_DOCUMENT_WORDS: (u32, u32) = (900, 1_800);

/// The COT positioning line for a commodity / macro fund
/// (`docs/portfolio-workflow.md` §Step 5; `docs/data-sources.md §CFTC`):
/// context held out of every sub-score. Empty where no contract mapped.
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

/// The CBOE venue-level put/call backdrop (`docs/data-sources.md §CBOE`): macro
/// context, never a per-name signal. Empty where the leg failed or never ran.
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
/// parameters): evidence only, never a gate. Empty where the engine computed
/// none.
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

/// The same-underlying option overlay (`docs/portfolio-workflow.md` §Step 6a),
/// rendered into both 6f prompts because it changes what the right action is.
/// Every packet renders structure and ratios only — never contract counts or
/// share-equivalents — so position size reaches the rung by no route. Empty
/// where the holding carries no option legs.
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
/// parameters), rendered as evidence, never a clamp. Empty where the read was
/// uncomputable. A hype read with no persisted ratio is one of two states the
/// render must not conflate: a non-positive reality leg (the ratio is
/// undefined) or a positive one the quotient overflowed on. The percentage
/// render is guarded the same way: a leg whose ×100 overflows prints as the
/// decimal ratio, never `inf%`.
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
        s.push_str(&format!("Rule matched: {rule}.\n"));
    }
    s
}

/// The FINRA short-interest read (`docs/data-sources.md §FINRA`): positioning
/// evidence held out of every sub-score. Empty where the holding has no row.
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

/// The run-level commodity context for a commodity-linked holding
/// (`docs/portfolio-workflow.md` §Step 5): evidence, never a score input. Empty
/// where no prints matched the sector.
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
/// prompts (`docs/portfolio-analysis.md` §Starting parameters): a tripped
/// trigger names its rule as evidence and an engine-arm bound, never a clamp
/// on the model's values. Empty where the sweep never ran.
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
            if stage == PromptStage::Thesis {
                s.push_str(
                    "By rule: the computed action set excludes adding and the computed action \
                     reads the exit family; the grade is unchanged.\n",
                );
            }
            s
        }
    }
}

/// The thesis-document message (`docs/portfolio-workflow.md` §Step 6f): Part 1
/// is inputs only, in page order, each section explained once and then its
/// values, with no instruction in it; Part 2 is the task only, each item
/// naming the Part 1 section it draws on and restating no value. The model
/// receives data, never a description of the app; the investor profile and
/// the position's economics are absent (`docs/portfolio-analysis.md`
/// §Intrinsic verdict).
pub fn thesis_user_prompt(input: &ThesisInput) -> String {
    let d = input.dossier;
    let e = input.engine;
    let is_fund = dossier_is_fund(d);
    let mut p = String::new();

    p.push_str("======== PART 1: INPUTS ========\n");

    p.push_str(&holding_header(d));

    p.push_str(&fetched_values_section(d, input.rates));

    p.push_str(
        "\nCOMPUTED\nThe computed reads follow under their labels, up to MARKET \
         ANALYSIS; each is derived from the fetched data by fixed formulas.\n",
    );

    if let Some(f) = &d.fund {
        p.push_str("\nFUND\n");
        if let Some(label) = &e.fund_class_label {
            p.push_str(&format!("Class: {label}."));
            if let Some(class) = f.fund.asset_class.as_deref() {
                p.push_str(&format!(" Reported asset class: {class}."));
            }
            p.push('\n');
        }
        p.push_str(&format!(
            "US share of holdings: {}.\n",
            crate::portfolio::fund::us_share(&f.fund)
                .map(|s| format!("{:.0}%", s * 100.0))
                .unwrap_or_else(|| "(gap)".to_string()),
        ));
        let tilt: Vec<String> = f
            .fund
            .sector_weights
            .iter()
            .chain(f.fund.country_weights.iter())
            .take(5)
            .map(|(label, w)| format!("{label} {:.1}%", w * 100.0))
            .collect();
        if !tilt.is_empty() {
            p.push_str(&format!(
                "Exposure tilt (the largest weights, by sector where reported and otherwise by \
                 country): {}.\n",
                tilt.join(", ")
            ));
        }
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
        if !f.fund.gaps.is_empty() {
            p.push_str(&format!("Evidence gaps: {}.\n", f.fund.gaps.join("; ")));
        }
        p.push_str(&positioning_prompt_section(f));
    }

    p.push_str("\nMETRICS\n");
    p.push_str(&statement_basis_line(
        d.financials.statement_basis,
        d.financials.equity_source,
        is_fund,
    ));
    p.push_str(&computed_metrics_lines(is_fund, &e.metrics));
    p.push_str(&consensus_blend_line(&d.financials));
    if !d.financials.gaps.is_empty() {
        p.push_str(&format!("Data gaps: {}\n", d.financials.gaps.join("; ")));
    }

    let factor_gloss = if e.factors.is_some() {
        format!(
            " Each score is the mean of the factors listed under it, each read over the trailing \
             twelve months where it reads the statements. A factor scores 50 at its band's \
             anchor and 0 or 100 at the band's edges, a value past an edge held at it. Where \
             the company has {OWN_HISTORY_MIN_POINTS} or more earlier trailing-year readings of \
             a factor, that score is averaged with the factor's rank among them, on the same \
             scale. A factor that could not be computed scores 50."
        )
    } else {
        String::new()
    };
    p.push_str(&format!(
        "\nSCORES\nFour scores from 0 to 100, higher is better on every axis: quality; \
         valuation, where higher means more attractive; momentum; risk, where higher means \
         more resilient. The grade is a letter derived from the quality, valuation and risk \
         scores.{factor_gloss}\n\
         quality {:.0}, valuation {:.0}, momentum {:.0}, risk {:.0}. Grade {}.{} Risk tier: {}.\n",
        e.sub_scores.quality,
        e.sub_scores.valuation,
        e.sub_scores.momentum,
        e.sub_scores.risk,
        e.grade.as_str(),
        if e.low_confidence_grade { " One score is imputed." } else { "" },
        e.risk_tier.as_str(),
    ));
    p.push_str(&factor_score_lines(e));

    // Each band carries its method clause so the three-year leg is weighed as
    // the extrapolation it is (`docs/portfolio-analysis.md` §Starting
    // parameters).
    p.push_str("\nPRICE BANDS (USD)\n");
    if let Some(tm) = &e.price_targets.three_month {
        p.push_str(&format!(
            "- three-month: bear {:.2} / base {:.2} / bull {:.2}. Method: {}\n",
            tm.bear,
            tm.base,
            tm.bull,
            three_month_method(tm),
        ));
    }
    if let Some(tm) = &e.price_targets.twelve_month {
        p.push_str(&format!(
            "- twelve-month: bear {:.2} / base {:.2} / bull {:.2}. Method: {}\n",
            tm.bear,
            tm.base,
            tm.bull,
            twelve_month_method(&e.target_meta)
        ));
    }
    if let Some(ty) = &e.price_targets.three_year {
        p.push_str(&format!(
            "- three-year: bear {:.2} / base {:.2} / bull {:.2}. Method: {}\n",
            ty.bear,
            ty.base,
            ty.bull,
            three_year_method(&e.target_meta)
        ));
    }
    if let Some(notes) = target_notes_line(&e.target_meta) {
        p.push_str(&format!("- Notes: {notes}\n"));
    }
    p.push_str(&implied_expectations_prompt_section(e));

    p.push_str(&hurdle_read_section(&e.hurdle));

    p.push_str(&narrative_prompt_section(input.narrative));
    if let Some(overlay) = input.pre_profit {
        p.push_str(&pre_profit_prompt_section(overlay, PromptStage::Thesis));
    }

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
    p.push_str(&forensic_prompt_section(d, PromptStage::Thesis));
    if let Some(flags) = input.soft_forensic {
        p.push_str(&soft_forensic_prompt_section(flags));
    }
    p.push_str(&commodity_prompt_section(d));
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

    p.push_str(&market_analysis_section(d));

    p.push_str(&analysis_section(d, &input.analysis));

    p.push_str(&review_section(input.review.as_deref()));

    let has_prior = d.prior_verdict.as_ref().and_then(|v| v.thesis_document()).is_some();
    p.push_str(&prior_thesis_section(d, input.prior_split));

    p.push_str(&thesis_task_section(has_prior, input.review.is_some()));
    p
}

fn hurdle_read_section(h: &engine::HurdleRead) -> String {
    let scorable = h.state != crate::portfolio::HurdleState::Unscorable;
    match (scorable, h.hurdle_rate, h.tr_bear, h.tr_base, h.tr_bull) {
        (true, Some(rate), Some(bear), Some(base), Some(bull)) => format!(
            "\nCAPITAL EFFICIENCY\nThe computed twelve-month total return in each scenario (the \
             move from the current price to the scenario price, plus forward income per share, \
             as a fraction of the current price) and the hurdle rate it is measured against, \
             with the read: clears when even the bear case clears the hurdle, fails when even \
             the bull case misses it, indeterminate otherwise.\n\
             bear {:+.1}% / base {:+.1}% / bull {:+.1}%; hurdle {:.1}%; read: {}.\n",
            bear * 100.0,
            base * 100.0,
            bull * 100.0,
            rate * 100.0,
            format!("{:?}", h.state).to_lowercase(),
        ),
        _ => "\nCAPITAL EFFICIENCY\nNo assessment this run.\n".to_string(),
    }
}

/// SOFT FORENSIC FLAGS as typed evidence (`docs/portfolio-analysis.md`
/// §Starting parameters); never clear on a gap.
fn soft_forensic_prompt_section(f: &SoftForensicFlags) -> String {
    use crate::portfolio::soft_forensic::{
        ALTMAN_Z_DISTRESS, NET_INCOME_TO_OPERATING_CASH_FLOW, PIOTROSKI_WEAK,
        WORKING_CAPITAL_TO_REVENUE_GROWTH,
    };
    let state = |s: &SoftFlagState| match s {
        SoftFlagState::Fired => "fired".to_string(),
        SoftFlagState::Clear => "clear".to_string(),
        SoftFlagState::Unevaluable { missing } => {
            format!("unevaluable (missing: {})", missing.join(", "))
        }
    };
    let leg = |l: &LineLeg| match l {
        LineLeg::Evaluated { growth, fired } => format!(
            "{:+.1}%{}",
            growth * 100.0,
            if *fired { ", past the test" } else { "" }
        ),
        LineLeg::NotApplicable => "not applicable".to_string(),
        LineLeg::Missing => "missing".to_string(),
        LineLeg::NotTested => "not tested".to_string(),
    };
    let w = &f.working_capital_build;
    let n = &f.net_income_vs_operating_cash_flow;
    format!(
        "\nSOFT FORENSIC FLAGS\nFour statement tests, each fired, clear, or unevaluable where an \
         input is missing (never read as clear).\n\
         - Altman Z below {ALTMAN_Z_DISTRESS}: {} (Z {}).\n\
         - Piotroski F-score at or below {PIOTROSKI_WEAK:.0}: {} (score {}).\n\
         - TTM net income above {NET_INCOME_TO_OPERATING_CASH_FLOW}× TTM operating cash flow: {} \
         (net income {}, operating cash flow {}).\n\
         - Receivables or inventory growing faster than {WORKING_CAPITAL_TO_REVENUE_GROWTH}× \
         revenue growth, year over year on the latest quarter: {} (revenue growth {}; \
         receivables {}; inventory {}).\n",
        state(&f.altman_z.state),
        f.altman_z.value.map(|v| format!("{v:.2}")).unwrap_or_else(|| "(gap)".into()),
        state(&f.piotroski.state),
        f.piotroski.value.map(|v| format!("{v:.0}")).unwrap_or_else(|| "(gap)".into()),
        state(&n.state),
        n.ttm_net_income.map(fmt_magnitude).unwrap_or_else(|| "(gap)".into()),
        n.ttm_operating_cash_flow.map(fmt_magnitude).unwrap_or_else(|| "(gap)".into()),
        state(&w.state),
        w.revenue_growth
            .map(|g| format!("{:+.1}%", g * 100.0))
            .unwrap_or_else(|| "(gap)".into()),
        leg(&w.receivables),
        leg(&w.inventory),
    )
}

/// The prior thesis document verbatim under its date, with any split-context
/// line (`docs/portfolio-workflow.md` §Step 6b). Empty on a debut.
pub(crate) fn prior_thesis_section(d: &HoldingDossier, split: Option<SplitContext>) -> String {
    let Some(doc) = d.prior_verdict.as_ref().and_then(|v| v.thesis_document()) else {
        return String::new();
    };
    let written = d.prior_vintage.as_deref().map(|v| {
        crate::market_clock::et_date_of(v)
            .map(|day| day.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| v.to_string())
    });
    document_section("PRIOR THESIS", written.as_deref(), split, doc)
}

/// The prior analysis verbatim under its own date, with the split-context line
/// its own anchor bar yields (`docs/portfolio-workflow.md` §Step 6c, §Step 6d),
/// so a carried analysis keeps its basis whatever the verdict's. Empty on a
/// debut.
pub(crate) fn prior_analysis_section(d: &HoldingDossier) -> String {
    let Some(record) = &d.prior_analysis else {
        return String::new();
    };
    document_section(
        "PRIOR ANALYSIS",
        Some(&record.written),
        record_split_context(d, record),
        &record.text,
    )
}

/// ANALYSIS on both thesis-document messages (`docs/portfolio-workflow.md`
/// §Step 6d, §Step 6f): undated where written this session, else under its
/// date with its own split-context line so the model never reads pre-split
/// figures as today's.
pub(crate) fn analysis_section(d: &HoldingDossier, record: &crate::portfolio::AnalysisRecord) -> String {
    let written = (record.written != d.analysis_date).then_some(record.written.as_str());
    document_section("ANALYSIS", written, record_split_context(d, record), &record.text)
}

/// An analysis record's split context from its own anchor bar, read the way
/// the verdict's is.
fn record_split_context(
    d: &HoldingDossier,
    record: &crate::portfolio::AnalysisRecord,
) -> Option<SplitContext> {
    let bridge = match &record.anchor {
        None => Some(1.0),
        Some(anchor) => engine::split_bridge_factor(&d.financials.daily_closes, anchor),
    };
    split_context_of(bridge, split_event_since(d, &record.written))
}

fn document_section(
    heading: &str,
    written: Option<&str>,
    split: Option<SplitContext>,
    doc: &str,
) -> String {
    let mut p = match written {
        Some(date) => format!("\n{heading} (written {date})\n"),
        None => format!("\n{heading}\n"),
    };
    match split {
        Some(SplitContext::Rebased { factor, split }) => {
            let named = match split {
                Some(s) => format!(
                    "A {}-for-{} share split on {} ",
                    fmt_split_leg(s.numerator),
                    fmt_split_leg(s.denominator),
                    s.date.format("%Y-%m-%d")
                ),
                None => "A share split ".to_string(),
            };
            p.push_str(&format!(
                "{named}since this document was written re-based the price series by a \
                 factor of {factor:.4}: multiply the prices it states by that factor to read \
                 them on today's basis. The document is as written.\n"
            ))
        }
        Some(SplitContext::Unverifiable) => p.push_str(
            "Whether a share split re-based the price series since this document was written \
             could not be verified this run: the close its prices were anchored to is missing \
             from the fetched window. The document is as written; its prices may sit on a \
             pre-split basis.\n",
        ),
        None => {}
    }
    p.push_str(doc);
    if !doc.ends_with('\n') {
        p.push('\n');
    }
    p
}

fn fmt_split_leg(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

/// FETCHED VALUES (`docs/portfolio-workflow.md` §Step 6c): the holding's data
/// as the providers return it, in the docs' order under group labels, one
/// figure or record per line, never engine-computed (the TTM basis, the
/// trailing dividend sum, the short-interest trend and the split bridge are
/// computations and stay out). A row whose source is absent is omitted, and
/// so is a group left with no row; a cell the provider left empty reads
/// `(gap)`. One renderer, shared by the research brief and both
/// thesis-document branches, byte for byte.
pub(crate) fn fetched_values_section(d: &HoldingDossier, rates: &RateAnchors) -> String {
    use crate::portfolio::evidence::{
        MaRole, PriceTargetWindow, SegmentYear, FUND_COUNTRY_WEIGHTS_SHOWN, SURPRISE_QUARTERS,
    };
    let fin = &d.financials;
    let gap = || "(gap)".to_string();
    let money = |v: Option<f64>| v.map(fmt_magnitude).unwrap_or_else(gap);
    let num = |v: Option<f64>| v.map(|x| format!("{x:.2}")).unwrap_or_else(gap);
    let pct = |v: Option<f64>| v.map(|x| format!("{:.1}%", x * 100.0)).unwrap_or_else(gap);
    let whole = |v: Option<f64>| v.map(|x| format!("{x:.0}")).unwrap_or_else(gap);
    let count = |v: Option<u64>| v.map(|x| x.to_string()).unwrap_or_else(gap);
    let score = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(gap);
    let text = |v: Option<&str>| v.unwrap_or("(gap)").to_string();
    let group = |p: &mut String, label: &str, rows: Vec<String>| {
        if rows.is_empty() {
            return;
        }
        p.push_str(&format!("\n{label}\n"));
        for row in rows {
            p.push_str(&row);
            p.push('\n');
        }
    };
    let mut p = String::from(
        "\nFETCHED VALUES\nThe holding's data (USD; B is billions, M is millions; a yield or a \
         return as a percentage); a missing figure reads (gap). The rows follow under their \
         labels.\n",
    );

    let mut profile = Vec::new();
    let name = d
        .company_name
        .as_deref()
        .or_else(|| d.fund.as_ref().and_then(|f| f.fund.name.as_deref()));
    if let Some(name) = name {
        profile.push(format!("- name: {name}"));
    }
    if let Some(issuer) = &d.issuer {
        if let Some(x) = issuer.exchange.as_deref() {
            profile.push(format!("- exchange: {x}"));
        }
        if let Some(x) = issuer.sector.as_deref() {
            profile.push(format!("- sector: {x}"));
        }
        if let Some(x) = issuer.industry.as_deref() {
            profile.push(format!("- industry: {x}"));
        }
    }
    if let Some(mc) = fin.market_cap {
        profile.push(format!("- market capitalization: {}", fmt_magnitude(mc)));
    }
    if let Some(sh) = fin.shares_outstanding {
        profile.push(format!("- shares outstanding: {}", fmt_magnitude(sh)));
    }
    if let Some(f) = &d.fund {
        let fd = &f.fund;
        if let Some(c) = fd.asset_class.as_deref() {
            profile.push(format!("- asset class: {c}"));
        }
        if let Some(e) = fd.expense_ratio {
            profile.push(format!("- expense ratio: {}", fmt_expense_ratio(Some(e))));
        }
        if let Some(a) = fd.aum {
            profile.push(format!("- assets under management: {}", fmt_magnitude(a)));
        }
        if let Some(n) = fd.nav {
            profile.push(format!("- NAV: {n:.2}"));
        }
    }
    group(&mut p, "PROFILE", profile);

    if let Some(f) = &d.fund {
        let fd = &f.fund;
        let list = |rows: &[(String, f64)], shown: usize| -> String {
            rows.iter()
                .take(shown)
                .map(|(l, w)| format!("{l} {:.1}%", w * 100.0))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut weights = Vec::new();
        if !fd.sector_weights.is_empty() {
            weights.push(format!("- sectors: {}", list(&fd.sector_weights, usize::MAX)));
        }
        // The largest countries, whatever order the feed served them in.
        let mut countries = fd.country_weights.clone();
        countries.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        if !countries.is_empty() {
            weights.push(format!(
                "- countries, largest first: {}",
                list(&countries, FUND_COUNTRY_WEIGHTS_SHOWN)
            ));
        }
        group(&mut p, "WEIGHTS", weights);
    }

    if !fin.quarterly_income.is_empty() {
        let quarters: Vec<&engine::QuarterlyIncomeRow> = fin.quarterly_income.iter().take(8).collect();
        let cash_flow = |q: &str| fin.quarterly_cash_flow.iter().find(|c| c.period_end == q);
        let balance = |q: &str| fin.quarterly_balance_sheet.iter().find(|b| b.period_end == q);
        let cells = |cell: &dyn Fn(&engine::QuarterlyIncomeRow) -> Option<f64>| -> Vec<Option<f64>> {
            quarters.iter().map(|q| cell(q)).collect()
        };
        let series = |label: &str, cells: Vec<Option<f64>>, fmt: &dyn Fn(f64) -> String| {
            let cells: Vec<String> = quarters
                .iter()
                .zip(cells)
                .map(|(q, v)| match v {
                    Some(x) => format!("{} {}", q.period_end, fmt(x)),
                    None => format!("{} (gap)", q.period_end),
                })
                .collect();
            format!("- {label}: {}", cells.join("; "))
        };
        // A money series keeps one unit, its largest cell's, at the fewest
        // decimals that leave its smallest non-zero cell two significant
        // digits, capped at four: no quarter switches unit mid-row, and a
        // cell too small for the unit's fourth decimal (under 50,000 in a
        // billions row) still reads as zero.
        let money_series = |label: &str, cells: Vec<Option<f64>>| {
            let largest = cells.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs()));
            let (divisor, suffix) = if largest >= 1e9 {
                (1e9, "B")
            } else if largest >= 1e6 {
                (1e6, "M")
            } else {
                return series(label, cells, &fmt_magnitude);
            };
            let smallest = cells
                .iter()
                .flatten()
                .map(|v| v.abs())
                .filter(|v| *v > 0.0)
                .fold(f64::INFINITY, f64::min);
            let decimals = if smallest.is_finite() {
                (1 - ((smallest / divisor).log10().floor() as i32)).clamp(1, 4) as usize
            } else {
                1
            };
            series(label, cells, &|v| format!("{:.decimals$}{suffix}", v / divisor))
        };
        group(
            &mut p,
            "STATEMENTS",
            vec![
                "Quarterly statements as reported, newest first (period end, then the figure)"
                    .to_string(),
                money_series("revenue", cells(&|q| q.revenue)),
                money_series("gross profit", cells(&|q| q.gross_profit)),
                money_series("operating income", cells(&|q| q.operating_income)),
                money_series("net income", cells(&|q| q.net_income)),
                series("diluted EPS", cells(&|q| q.eps_diluted), &|x| format!("{x:.2}")),
                money_series("diluted shares", cells(&|q| q.diluted_shares)),
                money_series(
                    "operating cash flow",
                    cells(&|q| cash_flow(&q.period_end).and_then(|c| c.operating_cash_flow)),
                ),
                money_series(
                    "free cash flow",
                    cells(&|q| cash_flow(&q.period_end).and_then(|c| c.free_cash_flow)),
                ),
                money_series(
                    "capital expenditure",
                    cells(&|q| cash_flow(&q.period_end).and_then(|c| c.capex)),
                ),
                money_series(
                    "total debt",
                    cells(&|q| balance(&q.period_end).and_then(|b| b.total_debt)),
                ),
                money_series(
                    "total equity",
                    cells(&|q| balance(&q.period_end).and_then(|b| b.total_equity)),
                ),
                money_series(
                    "cash and equivalents",
                    cells(&|q| balance(&q.period_end).and_then(|b| b.cash_and_equivalents)),
                ),
            ],
        );
    }
    if let Some(c) = &fin.consensus {
        let periods: Vec<String> = c
            .eps_periods
            .iter()
            .map(|r| format!("{} {}", r.period_end, num(r.eps_mid)))
            .collect();
        if !periods.is_empty() {
            group(
                &mut p,
                "CONSENSUS",
                vec![format!(
                    "- consensus EPS by fiscal period end, as published: {}",
                    periods.join("; ")
                )],
            );
        }
    }
    if !fin.recent_dividends.is_empty() {
        let rows: Vec<String> = fin
            .recent_dividends
            .iter()
            .map(|r| match &r.payment_date {
                Some(paid) => format!("{} {} (paid {paid})", r.date, fmt_per_share(r.amount)),
                None => format!("{} {}", r.date, fmt_per_share(r.amount)),
            })
            .collect();
        group(
            &mut p,
            "DIVIDENDS",
            vec![format!(
                "- dividends, latest ex-dates first (ex-date, amount per share): {}",
                rows.join("; ")
            )],
        );
    }

    let mut price = Vec::new();
    if let Some(spot) = fin.current_price.filter(|p| p.is_finite() && *p > 0.0) {
        price.push(format!("- quote: {spot:.2} per share (the live print, undated)"));
        if fin.year_high.is_some() || fin.year_low.is_some() {
            price.push(format!(
                "- 52-week range as served: low {}, high {}",
                num(fin.year_low),
                num(fin.year_high)
            ));
        }
    }
    let closes = &fin.daily_closes;
    if let (Some(first), Some(last)) = (closes.first(), closes.last()) {
        price.push(format!(
            "- daily closes: {} {} from {} to {}",
            closes.len(),
            if closes.len() == 1 { "session" } else { "sessions" },
            first.date,
            last.date
        ));
        // The close on the prior analysis's date, off today's series (today's
        // basis, so no bridge is needed). The anchor bar is not this date: it
        // precedes the session and carries across an abstention.
        let prior_date = d.prior_vintage.as_deref().and_then(|v| {
            crate::market_clock::et_date_of(v).map(|day| day.format("%Y-%m-%d").to_string())
        });
        if let Some(prior_date) = prior_date {
            if let Some(c) = closes.iter().rev().find(|c| c.date <= prior_date) {
                price.push(format!(
                    "- close on the prior analysis date {prior_date}: {:.2} ({})",
                    c.value, c.date
                ));
            }
        }
        if let Ok(run) = chrono::NaiveDate::parse_from_str(&d.analysis_date, "%Y-%m-%d") {
            for (label, months) in [("three months", 3u32), ("twelve months", 12), ("three years", 36)] {
                let Some(target) = run.checked_sub_months(chrono::Months::new(months)) else {
                    continue;
                };
                let target = target.format("%Y-%m-%d").to_string();
                if let Some(c) = closes.iter().rev().find(|c| c.date <= target) {
                    price.push(format!(
                        "- close {label} before the analysis date: {:.2} ({})",
                        c.value, c.date
                    ));
                }
            }
        }
    }
    group(&mut p, "QUOTE AND CLOSES", price);

    if !d.filings_8k.is_empty() {
        let mut filings = vec![
            "8-K filings of the trailing twelve months, newest first (filing date: items)".to_string(),
        ];
        filings.extend(d.filings_8k.iter().map(|f| {
            let items = match &f.items {
                Some(items) if !items.is_empty() => items.join(", "),
                Some(_) => "(no items)".to_string(),
                None => "(items unreadable)".to_string(),
            };
            if f.form == "8-K/A" {
                format!("- {} (8-K/A): {items}", f.filing_date)
            } else {
                format!("- {}: {items}", f.filing_date)
            }
        }));
        group(&mut p, "FILINGS", filings);
    }

    if let Some(si) = &d.short_interest {
        group(
            &mut p,
            "SHORT INTEREST PRINT",
            vec![
                format!("- settlement: {} (FINRA)", si.settlement_date),
                format!("- shares short: {:.0}", si.current_short_interest),
                format!(
                    "- shares short at the prior settlement: {}",
                    whole(si.previous_short_interest)
                ),
                format!("- average daily volume: {}", whole(si.average_daily_volume)),
                format!("- days to cover: {}", num(si.days_to_cover)),
            ],
        );
    }

    if let Some(e) = &d.evidence {
        let window = |label: &str, w: &PriceTargetWindow| {
            format!("{label} {} averaging {}", count(w.count), num(w.average))
        };
        let trend = |t: &crate::portfolio::evidence::PriceTargetTrend| {
            format!(
                "- targets published: {}; {}; {}",
                window("last month", &t.last_month),
                window("last quarter", &t.last_quarter),
                window("last year", &t.last_year)
            )
        };
        let mut street = Vec::new();
        if let Some(t) = &e.price_target {
            street.push(format!(
                "- price targets: consensus {}, median {}, low {}, high {}",
                num(t.consensus),
                num(t.median),
                num(t.low),
                num(t.high)
            ));
        }
        if let Some(tr) = &e.price_target_trend {
            street.push(trend(tr));
        }
        if let Some(g) = &e.grades_consensus {
            street.push(format!(
                "- analyst ratings: strong buy {}, buy {}, hold {}, sell {}, strong sell {}; \
                 consensus {}",
                count(g.strong_buy),
                count(g.buy),
                count(g.hold),
                count(g.sell),
                count(g.strong_sell),
                text(g.consensus.as_deref()),
            ));
        }
        if !e.rating_actions.is_empty() {
            street.push(
                "Rating actions, newest first (date: firm, previous grade to new grade, action)"
                    .to_string(),
            );
            street.extend(e.rating_actions.iter().map(|a| {
                format!(
                    "- {}: {}, {} to {}, {}",
                    a.date,
                    a.firm,
                    text(a.previous_grade.as_deref()),
                    text(a.new_grade.as_deref()),
                    text(a.action.as_deref())
                )
            }));
        }
        if let Some(r) = &e.ratings_snapshot {
            street.push(format!(
                "- FMP rating {}: overall {}; discounted cash flow {}, return on equity {}, \
                 return on assets {}, debt to equity {}, price to earnings {}, price to book {}",
                text(r.rating.as_deref()),
                score(r.overall),
                score(r.discounted_cash_flow),
                score(r.return_on_equity),
                score(r.return_on_assets),
                score(r.debt_to_equity),
                score(r.price_to_earnings),
                score(r.price_to_book),
            ));
        }
        group(&mut p, "STREET", street);

        let mut insiders = Vec::new();
        if !e.insider_trades.is_empty() {
            insiders.push(
                "Insider trades, newest first (transaction date: name, role, type, shares at \
                 price, filing date)"
                    .to_string(),
            );
            insiders.extend(e.insider_trades.iter().map(|t| {
                let mut row = format!(
                    "- {}: {}, {}, {}, {} shares at {}",
                    t.transaction_date,
                    t.name,
                    text(t.owner_type.as_deref()),
                    text(t.transaction_type.as_deref()),
                    whole(t.shares),
                    num(t.price)
                );
                if let Some(filed) = &t.filing_date {
                    row.push_str(&format!(", filed {filed}"));
                }
                row
            }));
        }
        if let Some(st) = &e.insider_statistics {
            insiders.push(format!(
                "- insider statistics, {} Q{}: {} acquiring and {} disposing transactions; {} \
                 shares acquired, {} disposed",
                score(st.year),
                score(st.quarter),
                count(st.acquired_transactions),
                count(st.disposed_transactions),
                whole(st.total_acquired),
                whole(st.total_disposed),
            ));
        }
        if !e.congressional_trades.is_empty() {
            insiders.push(
                "Congressional trades, newest first (transaction date: chamber, member, owner, \
                 type, amount, disclosure date)"
                    .to_string(),
            );
            insiders.extend(e.congressional_trades.iter().map(|t| {
                let mut row = format!(
                    "- {}: {}, {}, {}, {}, {}",
                    t.transaction_date,
                    t.chamber.label(),
                    t.name,
                    text(t.owner.as_deref()),
                    text(t.kind.as_deref()),
                    text(t.amount.as_deref())
                );
                if let Some(disclosed) = &t.disclosure_date {
                    row.push_str(&format!(", disclosed {disclosed}"));
                }
                row
            }));
        }
        group(&mut p, "INSIDERS AND CONGRESS", insiders);

        let mut earnings = Vec::new();
        let reported: Vec<String> = e
            .reported_earnings()
            .take(SURPRISE_QUARTERS)
            .map(|r| {
                format!(
                    "- {}: EPS {} vs {} estimated, revenue {}",
                    r.date,
                    num(r.eps_actual),
                    num(r.eps_estimated),
                    money(r.revenue_actual)
                )
            })
            .collect();
        if !reported.is_empty() {
            earnings.push(
                "Surprises, newest first (announcement date: EPS actual vs estimate, revenue \
                 actual)"
                    .to_string(),
            );
            earnings.extend(reported);
        }
        if let Some(next) = e.next_earnings(&d.analysis_date) {
            earnings.push(format!(
                "- next earnings: {} (EPS estimate {})",
                next.date,
                num(next.eps_estimated)
            ));
        }
        group(&mut p, "EARNINGS", earnings);

        let mut valuation = Vec::new();
        if !e.ratios.is_empty() {
            let r = &e.ratios;
            valuation.push(format!(
                "- trailing-twelve-month ratios: P/E {}; EV/EBITDA {}; EV/sales {}; P/B {}; \
                 FCF yield {}; ROIC {}; ROE {}; net debt/EBITDA {}",
                num(r.pe),
                num(r.ev_to_ebitda),
                num(r.ev_to_sales),
                num(r.pb),
                pct(r.fcf_yield),
                pct(r.roic),
                pct(r.roe),
                num(r.net_debt_to_ebitda),
            ));
        }
        if let Some(o) = &e.owner_earnings {
            valuation.push(format!(
                "- owner earnings ({}, period end {}): {}; {} per share",
                o.period.as_deref().unwrap_or("latest period"),
                o.period_end,
                money(o.owners_earnings),
                num(o.per_share)
            ));
        }
        if let Some(ev) = &e.enterprise_value {
            valuation.push(format!(
                "- enterprise value ({}): {}; market capitalization {}; total debt {}; cash {}",
                ev.date,
                money(ev.enterprise_value),
                money(ev.market_cap),
                money(ev.total_debt),
                money(ev.cash)
            ));
        }
        if let Some(f) = &e.float {
            let dated = f
                .date
                .as_deref()
                .map(|dt| format!(" ({dt})"))
                .unwrap_or_default();
            valuation.push(format!(
                "- float{dated}: float shares {}; shares outstanding {}; free float {}",
                money(f.float_shares),
                money(f.outstanding_shares),
                f.free_float_percent
                    .map(|x| format!("{x:.1}%"))
                    .unwrap_or_else(gap)
            ));
        }
        group(&mut p, "VALUATION", valuation);
    }

    if !d.ma_matches.is_empty() {
        let mut deals = vec![
            "SEC merger registration filings, trailing twelve months; a filing follows a deal's \
             announcement, typically by several weeks, and covers deals paid at least partly in \
             stock"
                .to_string(),
        ];
        deals.extend(d.ma_matches.iter().map(|m| {
            let role = match m.role {
                MaRole::Acquirer => "acquirer of",
                MaRole::Target => "target of",
            };
            let mut row = format!("- {role} {}, filed {}", m.counterparty, m.date);
            if let Some(link) = &m.link {
                row.push_str(&format!(" ({link})"));
            }
            row
        }));
        group(&mut p, "M&A", deals);
    }

    if let Some(e) = &d.evidence {
        let mut segments = Vec::new();
        let mut push_years = |label: &str, years: &[SegmentYear]| {
            if years.is_empty() {
                return;
            }
            segments.push(format!("{label}, newest fiscal year first"));
            segments.extend(years.iter().map(|y| {
                let parts: Vec<String> = y
                    .segments
                    .iter()
                    .map(|(name, v)| format!("{name} {}", fmt_magnitude(*v)))
                    .collect();
                let fy = y
                    .fiscal_year
                    .map(|fy| format!("FY{fy}"))
                    .unwrap_or_else(|| "fiscal year (gap)".to_string());
                format!("- {fy} (period end {}): {}", y.period_end, parts.join(", "))
            }));
        };
        push_years("Revenue by product", &e.product_segments);
        push_years("Revenue by geography", &e.geographic_segments);
        group(&mut p, "SEGMENTS", segments);
    }

    let as_of = |d: Option<&String>| d.map(|s| format!(" (as of {s})")).unwrap_or_default();
    group(
        &mut p,
        "RATES",
        vec![format!(
            "- Treasury yields (FRED): 10-year {:.2}%{}, 2-year {:.2}%{}",
            rates.dgs10 * 100.0,
            as_of(rates.dgs10_date.as_ref()),
            rates.dgs2 * 100.0,
            as_of(rates.dgs2_date.as_ref()),
        )],
    );
    p
}

/// Up to four decimals, trailing zeros trimmed past the second (`0.26`,
/// `0.2275`).
fn fmt_per_share(v: f64) -> String {
    let s = format!("{v:.4}");
    let trimmed = s.trim_end_matches('0');
    let decimals = trimmed.rsplit('.').next().map(str::len).unwrap_or(0);
    if decimals >= 2 {
        trimmed.to_string()
    } else {
        format!("{v:.2}")
    }
}

/// Billions or millions to one decimal, thousands to the unit, else two
/// places; the FETCHED VALUES gloss names the suffixes once.
fn fmt_magnitude(v: f64) -> String {
    let a = v.abs();
    if a >= 1e9 {
        format!("{:.1}B", v / 1e9)
    } else if a >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if a >= 1e3 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

/// The house view as market context, named for what it is, never by product
/// name.
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

/// The twelve-month method as a plain clause from the typed target inputs —
/// never the engine's own methodology string, which names its mechanics and
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

fn three_month_method(tm: &crate::portfolio::PriceTarget) -> String {
    let band = if tm.base > 0.0 { (tm.bull / tm.base - 1.0) * 100.0 } else { 0.0 };
    format!(
        "base = the twelve-month base price return prorated to three months; bear and bull = \
         ±{band:.1}% (two standard deviations of daily volatility over 63 sessions, capped at 26%)"
    )
}

fn three_year_method(t: &engine::TargetMeta) -> String {
    if t.driver_rung.contains("fund") {
        return "the twelve-month mix re-rating held unchanged for three years (the fund's \
                flat driver carries no growth to compound) — an extrapolation that assumes \
                today's rate and spread regime holds"
            .to_string();
    }
    let growth = match t.three_year_growth {
        Some(g) => format!(
            "compounded two further years at {:+.1}% a year (the growth the two coming \
             fiscal-year consensus rows imply, capped between −25% and +35%)",
            g * 100.0
        ),
        None => "held at flat growth for two further years (a single forward consensus row, \
                 or no definable growth)"
            .to_string(),
    };
    let floor = if t.three_year_floor_applied {
        "; the band was widened to the volatility dispersion floor"
    } else {
        ""
    };
    format!(
        "the twelve-month drivers {growth} at the same multiples — an extrapolation that \
         assumes today's rate and spread regime holds{floor}"
    )
}

/// The derivation notes that bear on how much signal the targets carry; `None`
/// where none applies.
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

/// Part 2 of the thesis-document message (`docs/portfolio-workflow.md` §Step
/// 6f).
fn thesis_task_section(has_prior: bool, has_review: bool) -> String {
    let mut p = String::from(
        "\n======== PART 2: TASK ========\n\n\
         Write the thesis document for this holding as plain text — no code fence, no JSON, no \
         heading before the first line. It covers, in this order:\n",
    );
    p.push_str(
        "\n1. The thesis — the investment case, from FETCHED VALUES, COMPUTED, ANALYSIS and \
         MARKET ANALYSIS.\n",
    );
    p.push_str("\n2. The key drivers — what the thesis depends on.\n");
    p.push_str(
        "\n3. The bear, base and bull scenarios — the conditions that produce each and your \
         probability for it; the three sum to about 100 percent.\n",
    );
    p.push_str(
        "\n4. The falsifiers — the observations that would show the thesis wrong — and the \
         triggers — the conditions on which the position would be added to, trimmed or sold \
         — each a concrete measure, a level and a period, a trigger stating the direction of \
         the position change.\n",
    );
    p.push_str(
        "\n5. The expected share price at three months, twelve months and three years, and \
         your conviction in the read as a whole as high, medium or low, each argued in the \
         text; the price bands under COMPUTED are evidence, not bounds. Where you state no \
         price at a horizon, or no conviction, say so.\n",
    );
    p.push_str(&format!(
        "\n6. A summary paragraph — the financial read, why those prices and that conviction{}.\n",
        continuity_clause(has_prior, has_review)
    ));
    p.push_str(&format!(
        "\nThe document runs {} to {} words.\n",
        fmt_thousands(THESIS_DOCUMENT_WORDS.0),
        fmt_thousands(THESIS_DOCUMENT_WORDS.1)
    ));
    p
}

fn continuity_clause(has_prior: bool, has_review: bool) -> &'static str {
    match (has_prior, has_review) {
        (true, true) => {
            ", and what changed since the prior analysis and how the prior read held up, drawing \
             on REVIEW and PRIOR THESIS"
        }
        (true, false) => ", and what changed since the prior analysis, drawing on PRIOR THESIS",
        (false, _) => "",
    }
}

fn review_section(review: Option<&str>) -> String {
    review
        .map(|text| document_section("REVIEW", None, None, text))
        .unwrap_or_default()
}

/// A thousands separator, as the docs state the length bands ("900–1,800
/// words").
fn fmt_thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The thesis-document conversation's second, non-thinking message
/// (`docs/portfolio-workflow.md` §Step 6f).
pub fn appendix_user_prompt() -> String {
    format!(
        "From the thesis document you wrote, transcribe the conviction it states (high, medium \
         or low) and the expected share price it states at three months, twelve months and \
         three years, in USD. A field is null where the document states no value. Return \
         them as one JSON object in the shape below, with no code fence and no surrounding \
         text.\n\nRETURN SHAPE (every value is a placeholder)\n{}\n",
        crate::portfolio::appendix_return_shape()
    )
}

fn consensus_blend_line(fin: &engine::CompanyFinancials) -> String {
    let Some(c) = &fin.consensus else {
        return String::new();
    };
    let per_share = |v: Option<f64>| v.map(|x| format!("{x:.2}")).unwrap_or_else(|| "(gap)".into());
    let money = |v: Option<f64>| v.map(fmt_magnitude).unwrap_or_else(|| "(gap)".into());
    format!(
        "- forward consensus, next twelve months blended over {} fiscal period{}: EPS low {} / \
         mid {} / high {}; revenue low {} / mid {} / high {} — the price bands' driver\n",
        c.periods_used,
        if c.periods_used == 1 { "" } else { "s" },
        per_share(c.eps_low),
        per_share(c.eps_mid),
        per_share(c.eps_high),
        money(c.revenue_low),
        money(c.revenue_mid),
        money(c.revenue_high),
    )
}

fn factor_score_lines(e: &engine::EngineOutput) -> String {
    use crate::portfolio::factors::Axis;
    let Some(c) = &e.factors else {
        return String::new();
    };
    let mut p = String::new();
    for (name, axis) in [
        ("quality", Axis::Quality),
        ("valuation", Axis::Value),
        ("momentum", Axis::Momentum),
        ("risk", Axis::LowRisk),
    ] {
        p.push_str(name);
        p.push('\n');
        for f in c.factors.iter().filter(|f| f.factor.axis() == axis) {
            p.push_str(&factor_line(f, &c.sector_anchor, e.value_creation.as_ref()));
        }
    }
    if c.thin_own_history {
        p.push_str(&format!(
            "Most of these factors had fewer than {OWN_HISTORY_MIN_POINTS} earlier readings, so \
             their scores rest mainly on the bands.\n"
        ));
    }
    p
}

fn factor_label(factor: crate::portfolio::factors::Factor) -> &'static str {
    use crate::portfolio::factors::Factor::*;
    match factor {
        EarningsYield => "net income over market value",
        FcfYield => "free cash flow over market value",
        EbitToEv => "operating income over enterprise value",
        BookToPrice => "book equity over market value",
        FfoYield => "net income plus depreciation and amortization over market value",
        Momentum => "price return from a year ago to a month ago",
        Trend => "last close over its 200-session average, less one",
        GrossProfitability => "gross profit over total assets",
        OperatingMargin => "operating income over revenue",
        RoicSpread => "return on invested capital less its cost of capital",
        FcfConversion => "free cash flow over net income",
        RoeSpread => "return on equity less its cost of equity",
        ReturnOnAssets => "net income over total assets",
        FfoToAssets => "net income plus depreciation and amortization over total assets",
        RevenueGrowth => "revenue growth over the prior year",
        Beta => "beta",
        Volatility => "annualized volatility of daily returns over the trailing year",
        DebtToEquity => "debt over equity",
        DebtToAssets => "debt over total assets",
        AssetGrowth => "total-asset growth over the prior year",
        Revision => "consensus estimate revision",
    }
}

fn factor_line(
    f: &crate::portfolio::factors::FactorScore,
    anchor_row: &str,
    value_creation: Option<&crate::portfolio::value_creation::ValueCreationRead>,
) -> String {
    use crate::portfolio::factors::{BandBasis, Factor};
    let label = factor_label(f.factor);
    let direction = if f.factor.higher_is_better() { "" } else { " (lower is better)" };
    if f.imputed {
        return format!("- {label}{direction}: not available; scored 50\n");
    }
    let value = match f.raw {
        Some(v) => {
            let parts = match (f.factor, value_creation) {
                (Factor::RoicSpread, Some(vc)) => match (vc.roic, vc.cost_of_capital.as_ref()) {
                    (Some(roic), Some(coc)) => {
                        format!(" (return {roic:.4} less cost of capital {:.4})", coc.rate)
                    }
                    _ => String::new(),
                },
                (Factor::RoeSpread, Some(vc)) => match vc.cost_of_equity {
                    Some(ke) => format!(" (return on equity {:.4} less cost of equity {ke:.4})", v + ke),
                    None => String::new(),
                },
                _ => String::new(),
            };
            format!("{v:.4}{parts}")
        }
        None => "positive debt over a zero or negative equity".to_string(),
    };
    let basis = match f.band.basis {
        BandBasis::Sector => anchor_row.to_string(),
        BandBasis::UnmappedSector => "market-wide; the sector has no anchors".to_string(),
        BandBasis::MarketWide => "market-wide".to_string(),
    };
    let edge = if f.winsorized { ", past its edge" } else { "" };
    let own = f
        .own_history
        .map(|o| format!("; rank among {} earlier readings: {:.0}", o.points, o.score))
        .unwrap_or_default();
    format!(
        "- {label}{direction}: {value}; band {:.4} ± {:.4} ({basis}): {:.0}{edge}{own}; score {:.0}\n",
        f.band.anchor,
        f.band.half_width,
        f.band_score.unwrap_or(f.score),
        f.score,
    )
}

/// The computed metric lines under COMPUTED, one per series the engine computes
/// for this vehicle kind. The price and the expense ratio are fetched values,
/// stated once under FETCHED VALUES, so neither renders here.
pub(crate) fn computed_metrics_lines(is_fund: bool, metrics: &engine::ComputedMetrics) -> String {
    let mut p = String::new();
    for s in engine::MetricSeries::ALL
        .iter()
        .copied()
        .filter(|s| s.computable_for(is_fund))
    {
        let value = match s.metric_value(metrics) {
            Some(v) => format!("{v:.4}"),
            None => "(gap)".to_string(),
        };
        p.push_str(&format!("- {}: {value} — {}\n", s.describe(), s.unit_note()));
    }
    p
}

fn opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.3}")).unwrap_or_else(|| "(gap)".to_string())
}

/// One shared formatter so every prompt states the expense ratio identically:
/// the decimal fraction primary, matching the computed metric lines, with the
/// percent reading beside it. Four places is one basis point; a nonzero ratio
/// that would round to zero extends its precision instead, up to ten places.
fn fmt_expense_ratio(v: Option<f64>) -> String {
    let Some(x) = v else {
        return "(gap)".to_string();
    };
    let places = render_places(x);
    let pct = places - 2;
    format!("{x:.places$} ({:.pct$}%/yr)", x * 100.0)
}

/// Four places (one basis point), extended up to ten where a nonzero value
/// would otherwise round to zero; shared so every fraction-unit value prints at
/// the same precision.
fn render_places(x: f64) -> usize {
    if x == 0.0 {
        4
    } else {
        (4..=10)
            .find(|p| (x * 10f64.powi(*p as i32)).round() != 0.0)
            .unwrap_or(10)
    }
}

/// The put-minus-call skew with an explicit sign, so the convention reads off
/// the number itself. The sign keys on the rendered three-place value: a skew
/// that rounds away prints `0.000`, never `+0.000`, since a sign would assert
/// a premium the number no longer shows.
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

/// One shared render of the closed-end price-vs-NAV line; callers gate on the
/// CEF marker (`docs/portfolio-analysis.md` §Asset eligibility). The label
/// follows the rendered tenth of a percent, so a value displaying as 0.0%
/// reads "at par", never "+0.0% premium".
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
    // A unit gloss only: what the number is, not what it means.
    format!(
        "PRICE VS NAV: {value} ({word}): the closed-end fund's market price against its \
         net asset value.\n",
    )
}

/// The system prompt for the per-holding action call
/// (`docs/portfolio-analysis.md` §Portfolio action): the role, the output names
/// and the message's shape.
pub fn action_system_prompt() -> String {
    format!(
        "You are an equity analyst deciding the portfolio action for one holding in a \
         portfolio review. {} {}",
        crate::portfolio::action_response_contract(),
        crate::portfolio::TWO_PART_FRAME
    )
}

/// The action message (`docs/portfolio-analysis.md` §Portfolio action;
/// `docs/portfolio-workflow.md` §Step 6f): Part 1 the inputs, each section
/// explained once and then its values with no instruction in it — POSITION
/// the one model-facing packet that sees the position's economics — and Part
/// 2 the task. No arm, baseline, stage, seam, validator behaviour, stamp or
/// product name; tunnel vision is enforced by input isolation, with no
/// whole-book field here.
pub fn action_user_prompt(input: &ActionInput) -> String {
    let d = input.dossier;
    let mut p = String::from("======== PART 1: INPUTS ========\n");
    p.push_str(&holding_header(d));
    p.push_str(&position_section(d));
    let prior_action = prior_action_section(d);
    match &input.subject {
        ActionSubject::Priced { graded, engine, pre_profit } => {
            p.push_str(&verdict_section(d, Some(&graded.appendix), &graded.thesis_document));
            p.push_str(&format!(
                "\nCOMPUTED\nThe computed reads follow under their labels, up to {}; \
                 each is derived from the holding's data by fixed formulas.\n",
                if prior_action.is_empty() { "SUPPORTED ACTIONS" } else { "PRIOR ACTION" }
            ));
            p.push_str(&engine_rung_section(graded));
            p.push_str(&grade_section(graded));
            p.push_str(&price_bands_section(d, graded, engine));
            p.push_str(&capital_efficiency_section(&engine.hurdle));
            p.push_str(&forensic_prompt_section(d, PromptStage::Action));
            if let Some(overlay) = pre_profit {
                p.push_str(&pre_profit_prompt_section(overlay, PromptStage::Action));
            }
            p.push_str(&option_overlay_prompt_section(d));
            p.push_str(&commodity_prompt_section(d));
        }
        ActionSubject::RoleRisk { verdict } => {
            // The branch's computed surface stays as top-level sections, as on
            // the thesis message.
            p.push_str(&format!("\nCLASS\n{}\n", verdict.class_label));
            if !verdict.exposure_tilt.is_empty() {
                let tilt: Vec<String> = verdict
                    .exposure_tilt
                    .iter()
                    .take(5)
                    .map(|w| format!("{} {:.0}%", w.label, w.weight * 100.0))
                    .collect();
                p.push_str(&format!("\nEXPOSURE TILT\n{}\n", tilt.join(", ")));
            }
            p.push_str(&format!(
                "\nRISK PROFILE\nExpense drag: {} of assets per year. \
                 Observable risk: {} (annualized realized volatility). Structural flag \
                 (leveraged / inverse or option-overlay path dependency): {}.\n",
                fmt_expense_ratio(verdict.expense_drag),
                opt(verdict.observable_risk),
                if verdict.structural_flag { "yes" } else { "no" },
            ));
            // Its absence is already a named gap below.
            if verdict.is_cef {
                if let Some(prem) = verdict.nav_premium {
                    p.push('\n');
                    p.push_str(&nav_premium_line(prem));
                }
            }
            if !verdict.evidence_gaps.is_empty() {
                p.push_str(&format!("\nEVIDENCE GAPS\n{}\n", verdict.evidence_gaps.join("; ")));
            }
            p.push_str(&verdict_section(d, None, &verdict.thesis_document));
            p.push_str(&forensic_prompt_section(d, PromptStage::Action));
            p.push_str(&option_overlay_prompt_section(d));
            p.push_str(&commodity_prompt_section(d));
        }
    }
    p.push_str(&prior_action);
    // One data line, no permission sentence: the return shape's enum shows the
    // ladder, and an outside-the-set rung persists as authored
    // (`outside_set_annotation`), never barred.
    let set: Vec<&str> = input.engine_set.iter().map(Action::as_kebab).collect();
    p.push_str(&format!(
        "\nSUPPORTED ACTIONS\nThe rungs a fixed rule over the holding's reads supports, listed \
         in full: {}. A rung not listed is outside that rule.\n",
        set.join(", ")
    ));
    // The cash row is deliberately not rendered: available capital is
    // whole-book context, kept out of this call by input isolation. The tax
    // row is not either: the tax posture is read by the app after the rung is
    // fixed (`with_tax_caveat`), never by the decision.
    let profile = input.profile.display();
    p.push_str(&format!(
        "\nINVESTOR PROFILE\n- objective: {}\n- risk tolerance: {}\n- horizon: {}\n",
        profile.objective, profile.risk_tolerance, profile.horizon,
    ));
    p.push_str(&action_task_section(input));
    p
}

/// POSITION (`docs/portfolio-analysis.md` §Portfolio action): the quantity
/// move only, never the paid-up / averaged-down read, which stays on the app's
/// surfaces (`docs/portfolio-analysis.md` §Holdings change tracking). An
/// exactly-zero basis is an unreported one on the wire, so it renders as not
/// reported with no gain or loss; a negative netted basis keeps its dollar
/// gain or loss and renders no percentage (`docs/portfolio-analysis.md`
/// §Storage and display).
fn position_section(d: &HoldingDossier) -> String {
    let pos = &d.position;
    let (basis, pl_line) = if pos.cost_basis == 0.0 {
        (
            "not reported".to_string(),
            "Unrealized gain or loss: not available without a reported cost basis.".to_string(),
        )
    } else {
        let pl = pos.market_value - pos.cost_basis;
        let share = if pos.cost_basis > 0.0 && pl.is_finite() {
            format!(" ({:+.1}% of the cost basis)", pl / pos.cost_basis * 100.0)
        } else {
            String::new()
        };
        let line = if pl > 0.0 {
            format!("Unrealized gain: {}{share}.", fmt_usd(pl))
        } else if pl < 0.0 {
            format!("Unrealized loss: {}{share}.", fmt_usd(-pl))
        } else {
            "Unrealized gain or loss: none.".to_string()
        };
        (fmt_usd(pos.cost_basis), line)
    };
    let delta = &d.position_delta;
    let change = match (delta.change, delta.prior_quantity) {
        (PositionChange::New, _) => "new".to_string(),
        (PositionChange::Unchanged, _) => format!("unchanged at {}", fmt_shares(pos.quantity)),
        (PositionChange::Increased, Some(prior)) => {
            format!("increased, from {} to {}", fmt_shares(prior), fmt_shares(pos.quantity))
        }
        (PositionChange::Decreased, Some(prior)) => {
            format!("decreased, from {} to {}", fmt_shares(prior), fmt_shares(pos.quantity))
        }
        (PositionChange::Increased, None) => "increased".to_string(),
        (PositionChange::Decreased, None) => "decreased".to_string(),
    };
    format!(
        "\nPOSITION\nThe holding as the account carries it: the shares held, the total cost \
         basis, the market value, the unrealized gain or loss (the market value less the cost \
         basis, and as a share of a positive cost basis; not available where no basis is \
         reported) and the change in the shares held since the last pull — new where the last \
         pull had none, else increased, decreased or unchanged, with the shares held then and \
         now.\n\
         Shares held: {}. Cost basis: {basis}. Market value: {}. {pl_line} Change since the \
         last pull: {change}.\n",
        fmt_shares(pos.quantity),
        fmt_usd(pos.market_value),
    )
}

/// A negative quantity reads as short.
fn fmt_shares(q: f64) -> String {
    let n = q.abs();
    let digits = if (n - n.round()).abs() < 1e-9 {
        format!("{n:.0}")
    } else {
        format!("{n:.4}").trim_end_matches('0').trim_end_matches('.').to_string()
    };
    if q < 0.0 {
        format!("{digits} short")
    } else {
        digits
    }
}

fn fmt_usd(v: f64) -> String {
    if !v.is_finite() {
        return "(gap)".to_string();
    }
    let s = format!("{:.2}", v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, "00"));
    let mut grouped = String::with_capacity(int.len() + int.len() / 3);
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{}${grouped}.{frac}", if v < 0.0 { "-" } else { "" })
}

/// `None` is unreachable on a priced holding (the quote floor); the guard stays
/// defensive.
fn usable_spot(d: &HoldingDossier) -> Option<f64> {
    d.financials.current_price.filter(|s| s.is_finite() && *s > 0.0)
}

fn fmt_price_move(spot: Option<f64>, v: f64) -> String {
    match spot {
        Some(s) => format!("{v:.2} ({:+.1}%)", (v / s - 1.0) * 100.0),
        None => format!("{v:.2}"),
    }
}

/// Labelled COMPUTED ACTION rather than ACTION so the sub-block never reads as
/// the output item (`docs/portfolio-analysis.md` §Portfolio action).
fn engine_rung_section(graded: &GradedVerdict) -> String {
    format!(
        "\nCOMPUTED ACTION\nThe rung a fixed rule gives from the computed reads: {}.\n",
        graded.engine_rung.as_kebab()
    )
}

/// The sub-scores and the risk tier render on the thesis message alone.
fn grade_section(graded: &GradedVerdict) -> String {
    format!(
        "\nGRADE\nA letter from A to F, derived from the computed quality, valuation and risk \
         scores.\n{}{}.\n",
        graded.grade.as_str(),
        if graded.low_confidence_grade {
            "; one of those scores is imputed, so the letter is low-confidence"
        } else {
            ""
        },
    )
}

/// A band the engine could not derive prints "(gap)"; a horizon the document
/// stated no price for prints "none".
fn price_bands_section(
    d: &HoldingDossier,
    graded: &GradedVerdict,
    engine: &EngineOutput,
) -> String {
    let spot = usable_spot(d);
    let leg = |v: f64| fmt_price_move(spot, v);
    let analyst = |v: Option<f64>| match v {
        Some(v) => leg(v),
        None => "none".to_string(),
    };
    let a = &graded.appendix;
    let mut p = String::from(
        "\nPRICE BANDS (USD, with the move each implies from the current price; the analyst's \
         expected price from VERDICT beside each)\n",
    );
    match &graded.price_targets.three_month {
        Some(t) => p.push_str(&format!(
            "- three-month: bear {} / base {} / bull {}; analyst {}. Method: {}.\n",
            leg(t.bear),
            leg(t.base),
            leg(t.bull),
            analyst(a.expected_price_3m),
            three_month_method(t),
        )),
        None => p.push_str(&format!(
            "- three-month: (gap); analyst {}.\n",
            analyst(a.expected_price_3m)
        )),
    }
    match &graded.price_targets.twelve_month {
        Some(t) => p.push_str(&format!(
            "- twelve-month: bear {} / base {} / bull {}; analyst {}. Method: {}.{}\n",
            leg(t.bear),
            leg(t.base),
            leg(t.bull),
            analyst(a.expected_price_12m),
            twelve_month_method(&engine.target_meta),
            target_notes_line(&engine.target_meta)
                .map(|n| format!(" Notes: {n}"))
                .unwrap_or_default(),
        )),
        None => p.push_str(&format!(
            "- twelve-month: (gap); analyst {}.\n",
            analyst(a.expected_price_12m)
        )),
    }
    match &graded.price_targets.three_year {
        Some(t) => p.push_str(&format!(
            "- three-year: bear {} / base {} / bull {}; analyst {}. Method: {}.\n",
            leg(t.bear),
            leg(t.base),
            leg(t.bull),
            analyst(a.expected_price_3y),
            three_year_method(&engine.target_meta),
        )),
        None => p.push_str(&format!(
            "- three-year: (gap); analyst {}.\n",
            analyst(a.expected_price_3y)
        )),
    }
    p
}

/// VERDICT: the packet never says the read came from the same model
/// (`docs/portfolio-analysis.md` §Portfolio action).
fn verdict_section(d: &HoldingDossier, appendix: Option<&ThesisAppendix>, document: &str) -> String {
    let mut p = String::from("\nVERDICT\n");
    if let Some(a) = appendix {
        let spot = usable_spot(d);
        let price = |v: Option<f64>| match v {
            None => "none".to_string(),
            Some(v) => fmt_price_move(spot, v),
        };
        let prices: Vec<String> = a
            .expected_prices()
            .iter()
            .map(|(label, v)| format!("{label} {}", price(*v)))
            .collect();
        p.push_str(
            "An analyst's read of the holding's data and research: the conviction, the expected \
             share price at each horizon (USD, with the move each implies from the current \
             price) and the thesis document.\n",
        );
        p.push_str(&format!(
            "Conviction: {}. Expected share price: {}.\n",
            a.conviction.map(Conviction::as_str).unwrap_or("none"),
            prices.join(", ")
        ));
    } else {
        p.push_str("An analyst's read of the holding's data and research: the thesis document.\n");
    }
    p.push_str("Thesis document:\n");
    p.push_str(document);
    if !document.ends_with('\n') {
        p.push('\n');
    }
    p
}

/// CAPITAL EFFICIENCY as numbers only — no state word; the sunk-cost rule is
/// one clause of the task.
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

/// PRIOR ACTION (`docs/portfolio-analysis.md` §Portfolio action): a
/// model-chosen rung with its rationale less the app's caveat
/// (`investment_sentence`); a rule-demoted rung with no rationale, since that
/// rationale argued the rung the rule replaced. Empty where the prior carries
/// no action.
fn prior_action_section(d: &HoldingDossier) -> String {
    let Some(prior) = d.prior_verdict.as_ref() else {
        return String::new();
    };
    let Some(action) = crate::portfolio::carried_action(prior) else {
        return String::new();
    };
    match prior.action_source {
        ActionSource::ModelChosen => {
            let mut p = format!("\nPRIOR ACTION\n{}, chosen in the prior analysis.\n", action.as_kebab());
            let rationale = crate::portfolio::carried_rationale(prior)
                .map(investment_sentence)
                .unwrap_or("")
                .trim();
            if !rationale.is_empty() {
                p.push_str(&format!("Rationale: {rationale}\n"));
            }
            p
        }
        ActionSource::RuleDemoted => format!(
            "\nPRIOR ACTION\n{}, set by rule after the prior analysis, not chosen in it.\n",
            action.as_kebab()
        ),
    }
}

/// Part 2 of the action message (`docs/portfolio-analysis.md` §Portfolio
/// action).
fn action_task_section(input: &ActionInput) -> String {
    let prior = input.dossier.prior_verdict.as_ref();
    let prior_chosen = prior.is_some_and(|v| {
        v.action_source == ActionSource::ModelChosen
            && crate::portfolio::carried_action(v).is_some()
    });
    let prior_action = prior.is_some_and(|v| crate::portfolio::carried_action(v).is_some());
    let mut p = String::from(
        "\n======== PART 2: TASK ========\n\nDetermine the following from the inputs and return \
         them as one JSON object in the shape at the end, with no code fence and no \
         surrounding text.\n\
         \n1. action — one rung for this holding, from these inputs alone: \"sell-all\", \
         \"trim\", \"hold\", \"add\" or \"add-aggressively\". The rung alone: no share count, \
         dollar amount or portfolio weight. ",
    );
    let (first, mut refining): (Vec<&str>, Vec<&str>) = match &input.subject {
        ActionSubject::Priced { .. } => (vec!["VERDICT", "COMPUTED"], vec!["POSITION"]),
        ActionSubject::RoleRisk { verdict } => {
            let mut first = vec!["CLASS", "VERDICT"];
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
            refining.push("POSITION");
            (first, refining)
        }
    };
    if prior_action {
        refining.push("PRIOR ACTION");
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
            " Where even the bull case under CAPITAL EFFICIENCY misses the hurdle and the \
             forward read is poor, lean toward realizing some or all of the position.",
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

/// Which packet a shared section is rendered into. The consequence lines (the
/// overlay's and the forensic sweep's) render on the thesis-document message
/// only; the action packet's SUPPORTED ACTIONS line already carries the
/// narrowed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptStage {
    Thesis,
    Action,
}

/// The pre-profit overlay for both 6f packets (`docs/portfolio-workflow.md`
/// §Step 6f); the consequence lines on the thesis packet only
/// ([`PromptStage`]).
fn pre_profit_prompt_section(o: &PreProfitOverlay, stage: PromptStage) -> String {
    use crate::portfolio::pre_profit::FinancingState;
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
    // The execution leg has no producer and renders nothing
    // (`docs/portfolio-workflow.md` §Step 6f).
    p.push_str(&format!(
        "- severe deterioration: {}\n",
        if o.severe_deterioration { "YES" } else { "no" }
    ));
    if stage == PromptStage::Action {
        return p;
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

/// The one place the prompt says which basis the flow metrics stand on this
/// run. The instants are named separately, since debt / equity and price /
/// book read the latest balance sheet on either basis. A `None` basis or
/// equity source says the lines are unevaluable here rather than silently on
/// some basis (`docs/portfolio-analysis.md` §Starting parameters).
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

/// The action fields are placeholders the action call overwrites, never
/// rendered into its prompt. One assembly serves the pipeline and the
/// fixed-evidence harness.
pub(crate) fn role_risk_verdict_from_model_arm(
    readout: &RoleRiskReadout,
    thesis_document: String,
) -> RoleRiskVerdict {
    RoleRiskVerdict {
        class_label: readout.class_label.clone(),
        thesis_document,
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
    }
}

/// The engine arm's figures app-stamped, the model arm persisted exactly as
/// authored (`docs/portfolio-analysis.md` §The holding verdict). The action
/// fields are placeholders the action call overwrites, never rendered into its
/// prompt.
pub(crate) fn graded_verdict_from_model_arm(
    engine_output: &EngineOutput,
    options_signal: crate::portfolio::OptionsSignal,
    model_arm: PricedModelArm,
    engine_rung: Action,
    authored_band_relation: Option<crate::portfolio::BandRelation>,
) -> GradedVerdict {
    GradedVerdict {
        grade: engine_output.grade,
        sub_scores: engine_output.sub_scores,
        action: Action::Hold,
        action_rationale: String::new(),
        thesis_document: model_arm.thesis_document,
        appendix: model_arm.appendix,
        price_targets: engine_output.price_targets.clone(),
        options_signal,
        risk_tier: engine_output.risk_tier,
        dead_money: engine_output.hurdle.state,
        low_confidence_grade: engine_output.low_confidence_grade,
        fund_class_label: engine_output.fund_class_label.clone(),
        grade_basis: engine_output.factors.as_ref().map(crate::portfolio::GradeBasis::of),
        engine_rung,
        authored_band_relation,
    }
}

/// Spot's authoring-time relation to the engine's twelve-month band, the stamp
/// the quick check's monitor compares against (`docs/portfolio-analysis.md`
/// §The quick check). `None` where no band or spot exists; the caller withholds
/// the band when the split bridge is unresolvable.
pub(crate) fn authored_band_relation(
    spot: Option<f64>,
    twelve_month: Option<&crate::portfolio::PriceTarget>,
) -> Option<crate::portfolio::BandRelation> {
    match (spot, twelve_month) {
        (Some(spot), Some(t)) => Some(crate::portfolio::BandRelation::of(spot, t.bear, t.bull)),
        _ => None,
    }
}

// ---- The deterministic stub analyst (offline) --------------------------------

/// A deterministic, offline [`HoldingAnalyst`]: a grade-derived interpretation,
/// so the whole pipeline produces a schema-valid verdict with no model call.
pub struct StubAnalyst;

impl HoldingAnalyst for StubAnalyst {
    // Research rides the trait's offline default; the consolidation renders
    // below serve a scripted research run.

    fn distill(&self, input: &distill::DistillInput<'_>) -> Result<String> {
        Ok(distill::stub_distillate(input))
    }

    fn analyze(&self, input: &distill::AnalysisInput<'_>) -> Result<String> {
        Ok(distill::stub_analysis(input))
    }

    fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
        let e = input.engine;
        let symbol = &input.dossier.position.symbol;
        let conviction = match e.grade {
            crate::portfolio::Grade::A | crate::portfolio::Grade::B => Conviction::High,
            crate::portfolio::Grade::C => Conviction::Medium,
            _ => Conviction::Low,
        };
        // The engine's base values nudged, so the two arms are distinguishable
        // in tests without being random; a horizon with no band states no
        // price.
        let nudged = |t: Option<&crate::portfolio::PriceTarget>, scale: f64| t.map(|t| t.base * scale);
        let appendix = ThesisAppendix {
            conviction: Some(conviction),
            expected_price_3m: nudged(e.price_targets.three_month.as_ref(), 1.01),
            expected_price_12m: nudged(e.price_targets.twelve_month.as_ref(), 1.05),
            expected_price_3y: nudged(e.price_targets.three_year.as_ref(), 1.10),
        };
        let price_words = |v: Option<f64>| v.map(|p| format!("${p:.2}")).unwrap_or_else(|| "no stated price".into());
        let continuity = if input.dossier.prior_verdict.is_some() {
            " Since the prior analysis the read is reaffirmed; nothing material changed."
        } else {
            ""
        };
        let thesis_document = format!(
            "Thesis: hold {symbol} for its established role; the computed read grades it {} on \
             quality {:.0}, valuation {:.0}, momentum {:.0} and risk {:.0}, and the evidence \
             supports the standing position.\n\n\
             Key drivers: the margin trajectory, the revenue growth and the balance sheet.\n\n\
             Scenarios: bear (25%) — fundamentals deteriorate materially; base (50%) — the \
             current trajectory holds; bull (25%) — growth re-accelerates.\n\n\
             Falsifiers and triggers: a trailing twelve-month net margin below 10% over two \
             quarters would show the thesis wrong; trim on a price above twice the twelve-month \
             base over a month; add on a price below the twelve-month bear case for a month.\n\n\
             Expected price: three months {}, twelve months {}, three years {}; conviction {}.\n\n\
             Summary: the financial read is the computed one, the prices follow the computed \
             bands with a modest premium, and the conviction follows the grade.{continuity}",
            e.grade.as_str(),
            e.sub_scores.quality,
            e.sub_scores.valuation,
            e.sub_scores.momentum,
            e.sub_scores.risk,
            price_words(appendix.expected_price_3m),
            price_words(appendix.expected_price_12m),
            price_words(appendix.expected_price_3y),
            conviction.as_str(),
        );
        Ok(PricedModelArm { thesis_document, appendix })
    }

    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String> {
        let r = input.readout;
        let continuity = if input.dossier.prior_verdict.is_some() {
            " Since the prior analysis the role is reaffirmed; nothing material changed."
        } else {
            ""
        };
        Ok(format!(
            "Role: {} supplying {} exposure; held for its portfolio role.\n\n\
             Risks: the expense drag, the structural path dependency where one applies, and \
             the exposure drifting from its mandate.\n\n\
             Triggers: trim on an expense ratio above 0.75% at the next published figure; sell \
             on a mandate change.\n\n\
             Summary: the vehicle supplies the exposure it exists to supply at its reported \
             cost.{continuity}",
            r.class_label,
            r.exposure_tilt
                .first()
                .map(|(l, _)| l.as_str())
                .unwrap_or("its mandated"),
        ))
    }

    fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision> {
        // Deliberately kept inside the engine set so no outside-set annotation
        // fires (that path is rogue-stub territory); hold is not always
        // offered, so the fallback is the least drastic offered rung.
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

/// The live [`HoldingAnalyst`]. Distillation normally runs on the fast model,
/// routing an oversized prompt or a reservation-bound retry to the reasoner;
/// the reasoner alone when no fast tier is configured.
pub struct LocalAnalyst {
    client: LocalModelClient,
    reasoner_model: String,
    fast_model: String,
    /// Routed model ids for the current holding, in outbound-call order;
    /// separate from prompt usage so provenance survives a transport failure
    /// before the daemon returns counters.
    model_calls: std::sync::Mutex<Vec<String>>,
    /// The retry-once gate shared by every model-call site
    /// (`docs/local-models.md §The local-model adapter seam`).
    retry: crate::local_model::RetryOnce,
    /// `None` runs the trait's offline research default.
    research_ctx: Option<LiveResearchCtx>,
}

pub struct LiveResearchCtx {
    pub web: std::sync::Arc<dyn research::ResearchWeb + Send + Sync>,
    pub progress: std::sync::Arc<crate::progress::RunContext>,
}

/// A blank `fast_model` falls back to the reasoner: the fast tier is optional
/// and never gates (`docs/configuration.md §Local Analysis Suite
/// Configuration`). The single home for the rule, read by [`LocalAnalyst::new`]
/// and the resume-status roster check so the two cannot drift.
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

    /// Without it the offline research default runs: a recorded gap, never a
    /// failed run.
    pub fn with_research(mut self, ctx: LiveResearchCtx) -> Self {
        self.research_ctx = Some(ctx);
        self
    }

    /// Test seam: the retry-once gate without its pause.
    #[cfg(test)]
    fn without_retry_delay(mut self) -> Self {
        self.retry = crate::local_model::RetryOnce::without_delay();
        self
    }

    /// Recorded immediately before issue; failed and retried attempts count as
    /// calls.
    fn record_model_call(&self, req: &ChatRequest) {
        self.model_calls
            .lock()
            .expect("model-call lock is never poisoned")
            .push(req.model_id.clone());
    }
}

/// Cap on how much of a model body a parse-failure context embeds: the full
/// body (up to ~250 KB) must never ride an error chain into a tracker step
/// detail, a stderr line or the persisted `job_runs.detail`.
const PARSE_CONTEXT_BODY_CAP: usize = 500;

fn body_snippet(content: &str) -> String {
    let (head, cut) = crate::data_sources::cap_chars(content, PARSE_CONTEXT_BODY_CAP);
    if cut {
        format!("{head} …(truncated, {} chars total)", content.chars().count())
    } else {
        head
    }
}

/// Fail a stage whose generation stopped at the output budget — a length stop
/// still returns `Ok` with a partial body, so without this check the
/// truncation surfaces only as an opaque downstream parse failure. Called
/// after adapter telemetry capture, so the observation survives on the
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
        // request's own reservation, or the shared context filling first. The
        // classification is single-homed so the data-health line reads the
        // same stop through the same predicate.
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
/// substance in `tool_calls`, so it passes). Typed so the retry-once can
/// classify it; runs after [`ensure_not_output_limited`], so a length stop
/// keeps its own reading.
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

/// The schema-valid parse, then the declared domain
/// ([`crate::portfolio::validate_appendix_domain`]), each failure carrying its
/// retry class. Runs inside the retry closure so an off-domain response gets
/// exactly the one re-issue every content failure gets, never a second retry
/// layer; the object is rejected whole, never clamped.
fn decode_appendix(stage: &str, content: &str) -> Result<ThesisAppendix> {
    let appendix: ThesisAppendix = serde_json::from_str(content)
        .map_err(|e| anyhow::Error::new(e).context(crate::local_model::RetryClass::SchemaParse))
        .with_context(|| format!("parsing appendix JSON: {}", body_snippet(content)))?;
    crate::portfolio::validate_appendix_domain(&appendix)
        .map_err(|e| anyhow::Error::new(e).context(crate::local_model::RetryClass::ModelArmDomain))
        .with_context(|| stage.to_string())?;
    Ok(appendix)
}

// Per-stage context sizes (`docs/local-model-operations.md §The num_ctx trap`):
// always explicit, since the daemon's auto-size over-allocates KV cache while
// an unset small default silently front-truncates the packet.
/// A distillation prompt that outgrows this budget issues on the reasoner at
/// [`NUM_CTX_INTERPRET`] instead of front-truncating here ([`distill_route`]).
pub(super) const NUM_CTX_DISTILL: u32 = 32_768;
/// The vendor advises ≥ 128 K context to preserve thinking capability
/// (`docs/local-model-operations.md §Context window`).
pub(crate) const NUM_CTX_INTERPRET: u32 = 131_072;
/// Ollama `keep_alive: -1`: never idle-unload (`docs/local-models.md §The
/// model roster and per-task routing`).
const KEEP_ALIVE_RESIDENT: i64 = -1;

// Per-stage output reservations (`num_predict`): diagnostic ceilings far above
// any legitimate answer, so a stop at the limit is evidence of a runaway or a
// squeeze, surfaced as a typed truncation error. Generation shares `num_ctx`
// with the prompt, so a large prompt can exhaust the context before the
// ceiling binds; that stop reports the same `done_reason: "length"`.
/// Thinking stages: chains run tens of thousands of tokens and count against
/// the same budget as the answer.
pub(super) const NUM_PREDICT_THINKING: u32 = 65_536;
/// Four short fields under the grammar, so a stop at this reservation is a
/// runaway, never a legitimate transcription.
pub(super) const NUM_PREDICT_APPENDIX: u32 = 1_024;
/// The normal distillation ceiling; a reservation-bound stop gets one larger
/// retry ([`NUM_PREDICT_DISTILL_RETRY`]), so this one remains the
/// runaway/latency guardrail. Beside the issue guard's input budget it still
/// fits a 32 K fast-tier context (`distill::input_budget_chars`).
pub(super) const NUM_PREDICT_DISTILL: u32 = 12_288;
/// One re-attempt after the normal reservation binds exactly, issued on the
/// reasoner's 128 K context so the prompt and this ceiling fit together under
/// the same input sizing guard.
pub(super) const NUM_PREDICT_DISTILL_RETRY: u32 = 32_768;

/// Resolved per model, not per call: Ollama reloads a resident runner whenever
/// a request's `num_ctx` differs from the loaded one, even under `keep_alive:
/// -1`, so when the fast tier fell back to the reasoner distillation shares
/// the interpretation context rather than bouncing the runner between 32 K
/// and 128 K (`docs/local-model-operations.md §The num_ctx trap`).
pub(super) fn distill_num_ctx(fast_model: &str, reasoner_model: &str) -> u32 {
    if fast_model == reasoner_model {
        NUM_CTX_INTERPRET
    } else {
        NUM_CTX_DISTILL
    }
}

/// Where one distillation call issues (`docs/local-models.md §The local-model
/// adapter seam`): the rendered prompt is measured in chars against its
/// model's input budget before any request exists. Over the fast tier's
/// budget it issues on the reasoner at the interpretation context — a model
/// choice, never a `num_ctx` change, so nothing reloads; over the widest
/// budget it is refused here, unclassified so the retry gate never re-issues
/// a deterministic outcome. Pure, so the routing is pinned offline.
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

/// Explicitly non-thinking (`Some(false)`: an omitted flag rides the model's
/// thinking-on default), no grammar (`docs/portfolio-workflow.md` §Step 6d: a
/// distillation returns prose), on the caller-routed model and context
/// ([`distill_route`]). Pure, so the wiring is asserted offline.
pub(super) fn distill_request(
    model: &str,
    num_ctx: u32,
    num_predict: u32,
    prompt: &distill::DistillPrompt,
) -> ChatRequest {
    let mut req = ChatRequest::new(
        model,
        vec![
            ChatMessage::system(prompt.system.clone()),
            ChatMessage::user(prompt.user.clone()),
        ],
    );
    req.think = Some(false);
    req.format_schema = None;
    req.options = Some(options::non_thinking_general(num_ctx, num_predict));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// The only stop that activates the larger distillation attempt; a stop below
/// the reservation is context-bound or unattributable and keeps the
/// hard-failure posture.
fn hit_normal_distill_reservation(
    req: &ChatRequest,
    resp: &crate::local_model::ChatResponse,
) -> bool {
    resp.done_reason.as_deref() == Some("length")
        && crate::local_model::request_num_predict(req) == Some(NUM_PREDICT_DISTILL)
        && resp.eval_count == Some(u64::from(NUM_PREDICT_DISTILL))
}

/// One distillation call on the live adapter. `expanded_spent` is set the
/// moment the expanded re-attempt issues — on the error path too — so the
/// retry gate never layers a third request after it: the expanded attempt is
/// final for the stage (`docs/local-models.md §The local-model adapter seam`).
fn distill_prose_call(
    analyst: &LocalAnalyst,
    stage: &str,
    prompt: &distill::DistillPrompt,
    expanded_spent: &std::cell::Cell<bool>,
) -> Result<String> {
    let (model, num_ctx) = distill_route(
        stage,
        prompt.chars(),
        &analyst.fast_model,
        &analyst.reasoner_model,
    )?;
    let mut req = distill_request(model, num_ctx, NUM_PREDICT_DISTILL, prompt);
    req.stage = Some(stage.to_string());
    analyst.record_model_call(&req);
    let resp = analyst.client.chat(&req)?;

    if hit_normal_distill_reservation(&req, &resp) {
        // Routed to the reasoner even when the normal call used the fast tier:
        // its 128 K context holds the prompt plus the expanded ceiling, which a
        // 32 K tier could not.
        expanded_spent.set(true);
        let mut expanded_req = distill_request(
            &analyst.reasoner_model,
            NUM_CTX_INTERPRET,
            NUM_PREDICT_DISTILL_RETRY,
            prompt,
        );
        expanded_req.stage = Some(format!("{stage} (expanded)"));
        analyst.record_model_call(&expanded_req);
        let expanded_resp = analyst.client.chat(&expanded_req)?;

        ensure_not_output_limited(stage, &expanded_req, &expanded_resp).with_context(|| {
            format!(
                "{stage}: expanded distillation attempt also length-stopped after the normal \
                 {NUM_PREDICT_DISTILL}-token reservation bound"
            )
        })?;
        ensure_nonempty_completion(stage, &expanded_resp)?;
        return Ok(expanded_resp.content);
    }
    ensure_not_output_limited(stage, &req, &resp)?;
    ensure_nonempty_completion(stage, &resp)?;
    Ok(resp.content)
}

/// Thinking on, at the shared interpret context (one `num_ctx` per model).
/// Gathering turns carry `tools` with no `format`; the synthesis conversation
/// carries neither (`docs/portfolio-workflow.md` §Step 6c).
pub(super) fn research_turn_request(
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

/// Thinking on, no grammar (`docs/portfolio-workflow.md` §Step 6d).
pub(super) fn analysis_request(reasoner_model: &str, input: &distill::AnalysisInput<'_>) -> ChatRequest {
    let prompt = distill::analysis_prompt(input);
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![ChatMessage::system(prompt.system), ChatMessage::user(prompt.user)],
    );
    req.format_schema = None;
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Thinking on, no grammar (`docs/portfolio-workflow.md` §Step 6e).
pub(super) fn review_request(reasoner_model: &str, input: &review::ReviewInput) -> ChatRequest {
    let prompt = review::review_prompt(input);
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![ChatMessage::system(prompt.system), ChatMessage::user(prompt.user)],
    );
    req.format_schema = None;
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Thinking on, no grammar (`docs/portfolio-workflow.md` §Step 6f).
pub(super) fn thesis_request(reasoner_model: &str, input: &ThesisInput) -> ChatRequest {
    let is_fund = dossier_is_fund(input.dossier);
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![
            ChatMessage::system(thesis_system_prompt(is_fund)),
            ChatMessage::user(thesis_user_prompt(input)),
        ],
    );
    req.format_schema = None;
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// The same conversation's second message — the document as the assistant's
/// turn, then the transcription ask — thinking off under the appendix grammar
/// ([`crate::portfolio::appendix_schema`]).
pub(super) fn appendix_request(
    reasoner_model: &str,
    input: &ThesisInput,
    thesis_document: &str,
) -> ChatRequest {
    let is_fund = dossier_is_fund(input.dossier);
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![
            ChatMessage::system(thesis_system_prompt(is_fund)),
            ChatMessage::user(thesis_user_prompt(input)),
            ChatMessage::assistant(thesis_document.to_string()),
            ChatMessage::user(appendix_user_prompt()),
        ],
    );
    req.format_schema = Some(appendix_schema());
    req.think = Some(false);
    req.options = Some(options::non_thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_APPENDIX));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Thinking on, no grammar, as [`thesis_request`].
pub(super) fn role_risk_request(reasoner_model: &str, input: &RoleRiskInput) -> ChatRequest {
    let mut req = ChatRequest::new(
        reasoner_model,
        vec![
            ChatMessage::system(role_risk_system_prompt()),
            ChatMessage::user(role_risk_user_prompt(input)),
        ],
    );
    req.format_schema = None;
    req.think = Some(true);
    req.options = Some(options::thinking_general(NUM_CTX_INTERPRET, NUM_PREDICT_THINKING));
    req.keep_alive = Some(KEEP_ALIVE_RESIDENT);
    req
}

/// Thinking on (the rung is a judgment call), the action schema, and the
/// shared interpret context — one `num_ctx` per model, since a change reloads
/// the resident runner (`docs/local-model-operations.md §The num_ctx trap`).
pub(super) fn action_request(reasoner_model: &str, input: &ActionInput) -> ChatRequest {
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
    fn distill(&self, input: &distill::DistillInput<'_>) -> Result<String> {
        // The retry gate wraps the call (`docs/local-models.md §The local-model
        // adapter seam`); the expanded re-attempt is final for the stage.
        let prompt = distill::distillation_prompt(input);
        let stage = input.stage.as_str();
        let expanded_spent = std::cell::Cell::new(false);
        self.retry.run_unless(
            self.client.progress(),
            stage,
            || expanded_spent.get(),
            || distill_prose_call(self, stage, &prompt, &expanded_spent),
        )
    }

    fn analyze(&self, input: &distill::AnalysisInput<'_>) -> Result<String> {
        // Streamed onto the holding's own step; a length stop is not re-issued.
        let step_key = crate::portfolio::holding_step_key(input.symbol);
        let mut req = analysis_request(&self.reasoner_model, input);
        let stage = input.stage();
        req.stage = Some(stage.clone());
        self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self
                .client
                .chat_streaming(&req, StreamRole::Step(&step_key))?;

            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            Ok(resp.content)
        })
    }

    fn review(&self, input: &review::ReviewInput) -> Result<String> {
        // Streamed onto the holding's own step; a length stop is not re-issued.
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let mut req = review_request(&self.reasoner_model, input);
        let stage = input.stage();
        req.stage = Some(stage.clone());
        self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self
                .client
                .chat_streaming(&req, StreamRole::Step(&step_key))?;

            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            Ok(resp.content)
        })
    }

    fn research(
        &self,
        dossier: &HoldingDossier,
        plan: &ResearchPlan,
    ) -> Result<HoldingResearch> {
        let Some(ctx) = &self.research_ctx else {
            // A recorded gap, never a failed run.
            return Ok(research::offline_stub(plan));
        };
        // Tools and a grammar never share a request.
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
                // Non-streaming (the tool protocol); the thinking is forwarded
                // whole onto the step once it lands.
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
        runner.run_holding_with_issuer(
            &plan.brief,
            &plan.agenda,
            dossier.earnings_issuer.as_ref(),
        )
    }

    fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
        let symbol = &input.dossier.position.symbol;
        // Streamed onto the holding's own step so the tracker shows live
        // thinking.
        let step_key = crate::portfolio::holding_step_key(symbol);
        // A length stop is not re-issued.
        let mut req = thesis_request(&self.reasoner_model, input);
        let stage = format!("thesis {symbol}");
        req.stage = Some(stage.clone());
        let thesis_document = self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self
                .client
                .chat_streaming(&req, StreamRole::Step(&step_key))?;

            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            Ok(resp.content)
        })?;
        // The same conversation's second message (`docs/portfolio-analysis.md`
        // §The holding verdict).
        let mut req = appendix_request(&self.reasoner_model, input, &thesis_document);
        let stage = format!("appendix {symbol}");
        req.stage = Some(stage.clone());
        let appendix = self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self
                .client
                .chat_streaming(&req, StreamRole::Step(&step_key))?;

            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            decode_appendix(&stage, &resp.content)
        })?;
        Ok(PricedModelArm { thesis_document, appendix })
    }

    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String> {
        let mut req = role_risk_request(&self.reasoner_model, input);
        let step_key = crate::portfolio::holding_step_key(&input.dossier.position.symbol);
        let stage = format!("thesis {}", input.dossier.position.symbol);
        req.stage = Some(stage.clone());
        self.retry.run(self.client.progress(), &stage, || {
            self.record_model_call(&req);
            let resp = self
                .client
                .chat_streaming(&req, StreamRole::Step(&step_key))?;

            ensure_not_output_limited(&stage, &req, &resp)?;
            ensure_nonempty_completion(&stage, &resp)?;
            Ok(resp.content)
        })
    }

    fn decide_action(&self, input: &ActionInput) -> Result<crate::portfolio::ActionDecision> {
        let mut req = action_request(&self.reasoner_model, input);
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
        // The reasoner's id when the fast tier fell back to it, so the audit
        // records the model distillation actually ran on.
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
        QuarterlyBalanceSheetRow, QuarterlyIncomeRow,
    };
    use crate::portfolio::fund::{FundContext, FundData, SectorPe};
    use crate::portfolio::{
        AssetClass, InvestorProfile, OptionsSignal, PositionChange, PositionDelta, PriceTarget,
    };
    use crate::portfolio::dossier::HouseView;
    use crate::schwab::Position;
    use std::collections::HashMap;

    // ---- The local analyst's recorded attempts ----

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

    /// The action packet with its POSITION block cut out, so a test can pin
    /// that a repriced position changes nothing else.
    pub(crate) fn without_position(packet: &str) -> String {
        let start = packet.find("\nPOSITION\n").expect("a POSITION block");
        let end = start + 1 + packet[start + 1..].find("\n\n").expect("the block's end");
        format!("{}{}", &packet[..start], &packet[end..])
    }

    /// A static reference so a struct literal's field can borrow it without a
    /// binding.
    pub(crate) fn rates_static() -> &'static RateAnchors {
        static RATES: std::sync::OnceLock<RateAnchors> = std::sync::OnceLock::new();
        RATES.get_or_init(rates)
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
                net_income: Some(2.5e10),
                gross_profit: Some(45.0e9),
                cost_of_revenue: None,
                operating_income: None,
                research_and_development: None,
                income_before_tax: None,
                income_tax_expense: None,
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
            quarterly_balance_sheet: ends
                .iter()
                .map(|end| QuarterlyBalanceSheetRow {
                    period_end: end.to_string(),
                    total_assets: Some(3.5e11),
                    total_debt: Some(1.0e11),
                    total_equity: Some(7.0e10),
                    cash_and_equivalents: Some(6.0e10),
                    short_term_investments: Some(0.0),
                    ..Default::default()
                })
                .collect(),
            beta: Some(1.1),
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

    pub(crate) fn dossier(asset_class: AssetClass, financials: CompanyFinancials) -> HoldingDossier {
        HoldingDossier {
            earnings_issuer: None,
            prior_metrics: None,
            news_leads: Vec::new(),
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
            prior_grade_parameter_version: None,
            prior_target_parameter_version: None,
            prior_authoring_close: None,
            sources: vec!["FMP".into()],
            prior_analysis: None,
            prior_accuracy_read_through: None,
            prior_fund_exposure: None,
            accuracy: None,
            listing: None,
            filing_events: None,
            short_interest: None,
            option_overlay: None,
            put_call_backdrop: None,
            commodity_context: Vec::new(),
            sector_benchmark: None,
            issuer: None,
            evidence: None,
            filings_8k: Vec::new(),
            ma_matches: Vec::new(),
        }
    }

    #[test]
    fn the_technology_topic_fires_from_the_pre_flag_alone_and_only_once() {
        // A news lead rides the brief and fires nothing on its own.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.news_leads = vec![research::NewsLead {
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
        assert_eq!(tech_topics(research::AgendaTriggers::default()), 0);
        assert_eq!(
            tech_topics(research::AgendaTriggers {
                tech_pre_flag_fired: true,
                ..Default::default()
            }),
            1
        );
    }

    #[test]
    fn slice2_fund_agenda_excludes_technology_even_with_the_pre_flag() {
        let d = fund_dossier(us_equity_fund());
        let agenda = research::build_agenda(&d, &research::AgendaTriggers {
            tech_pre_flag_fired: true,
            ..Default::default()
        });
        assert!(!agenda.iter().any(|t| t.key == "technology-event"));
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
        // The house view rides every dossier, so the source claim has to be
        // earned by reaching an interpretation call; the default is absent and
        // the two interpretation paths opt in.
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

        assert!(
            claims(with_house_view(AssetClass::Stock, 100.0)),
            "an interpreted holding records the house view it read"
        );

        assert!(
            !claims(with_house_view(AssetClass::Stock, -100.0)),
            "a net-short position never reaches interpretation"
        );
        assert!(!claims(with_house_view(AssetClass::Stock, 0.0)));
        assert!(!claims(with_house_view(AssetClass::Cash, 100.0)));

        let mut guarded = dossier(AssetClass::Stock, strong_financials());
        guarded.house_view = house_view_of(Some("## Thesis\nrisk-on."), 0);
        guarded.listing = Some(crate::portfolio::listing::ListingResolution::Unresolved);
        assert!(!claims(
            analyze_holding(&StubAnalyst, &guarded, &rates(), "2026-08-03")
                .unwrap()
                .1
                .sources
        ));

        // No current price exits below the evidence floor.
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
        // `load_house_view` keeps the summaries when the latest report's
        // Markdown is missing, so a summary-only house view is reachable and
        // renders as the stance lines on both messages.
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

    /// A distinct fast tier and reasoner, so the audit's model list can be
    /// checked against the calls that ran.
    struct TieredStub;
    impl HoldingAnalyst for TieredStub {
        fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
            StubAnalyst.interpret(input)
        }
        fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String> {
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

    /// Exact-call telemetry: research is the first modeled stage.
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
        fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
            self.called("reasoner");
            StubAnalyst.interpret(input)
        }
        fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String> {
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

    /// A bond fund routes to the role/risk branch.
    fn bond_fund() -> FundData {
        let mut bond = us_equity_fund();
        bond.symbol = "BND".into();
        bond.asset_class = Some("Fixed Income".into());
        bond.sector_weights = vec![];
        bond
    }

    #[test]
    fn audit_model_ids_name_only_the_models_actually_called() {
        let run = |d: &HoldingDossier| {
            analyze_holding(&TieredStub, d, &rates(), "2026-08-03").unwrap()
        };

        // No-model exits persist an empty list.
        let (v, a) = run(&dossier(AssetClass::Cash, strong_financials()));
        assert!(matches!(v.disposition, VerdictDisposition::NotRated { .. }));
        assert!(a.model_ids.is_empty(), "{:?}", a.model_ids);
        let mut short = dossier(AssetClass::Stock, strong_financials());
        short.position.quantity = -100.0;
        let (v, a) = run(&short);
        assert!(matches!(v.disposition, VerdictDisposition::NotRated { .. }));
        assert!(a.model_ids.is_empty(), "{:?}", a.model_ids);
        let mut guarded = dossier(AssetClass::Stock, strong_financials());
        guarded.listing = Some(crate::portfolio::listing::ListingResolution::Unresolved);
        let (_, a) = run(&guarded);
        assert!(a.model_ids.is_empty(), "{:?}", a.model_ids);
        let mut floored = dossier(AssetClass::Stock, strong_financials());
        floored.financials.current_price = None;
        let (v, a) = run(&floored);
        assert!(matches!(v.disposition, VerdictDisposition::InsufficientEvidence { .. }));
        assert!(a.model_ids.is_empty(), "{:?}", a.model_ids);

        let (v, a) = run(&fund_dossier(bond_fund()));
        assert!(matches!(v.disposition, VerdictDisposition::RoleRiskOnly(_)));
        assert_eq!(a.model_ids, vec!["reasoner".to_string()]);

        let (v, a) = run(&dossier(AssetClass::Stock, strong_financials()));
        assert!(matches!(v.disposition, VerdictDisposition::Priced(_)));
        assert_eq!(a.model_ids, vec!["reasoner".to_string()]);
        let (v, a) = run(&fund_dossier(us_equity_fund()));
        assert!(matches!(v.disposition, VerdictDisposition::Priced(_)), "{v:?}");
        assert_eq!(a.model_ids, vec!["reasoner".to_string()]);

        let (_, a) = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert_eq!(a.model_ids, vec!["stub-analyst".to_string()]);

        // Exact telemetry supersedes the configured-id fallback.
        let exact = TelemetryTieredStub::default();
        let (_, a) = analyze_holding(
            &exact,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert_eq!(a.model_ids, vec!["reasoner".to_string()]);
    }

    #[test]
    fn rate_anchors_are_a_source_only_where_a_prompt_or_a_priced_output_read_them() {
        let sources = |d: &HoldingDossier| {
            analyze_holding(&StubAnalyst, d, &rates(), "2026-08-03")
                .unwrap()
                .1
                .sources
        };
        let names_fred = |s: &[String]| s.iter().any(|x| x == RATE_ANCHORS_SOURCE);

        assert!(names_fred(&sources(&dossier(AssetClass::Stock, strong_financials()))));
        assert!(names_fred(&sources(&fund_dossier(us_equity_fund()))));

        assert!(names_fred(&sources(&fund_dossier(bond_fund()))), "role/risk: the message states the prints");
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
        // The reduced surface only; the closed-end read stays None off the CEF
        // form.
        assert!(audit.metrics.pe_ratio.is_none());
        assert!(audit.metrics.revenue_growth.is_none());
        assert_eq!(audit.metrics.nav_premium, None);

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
        struct EmptyRationaleStub;
        impl HoldingAnalyst for EmptyRationaleStub {
            fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
                StubAnalyst.interpret(input)
            }
            fn interpret_role_risk(
                &self,
                input: &RoleRiskInput,
            ) -> Result<String> {
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

        // The one-sentence shape is a prompt preference, not validated.
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
        // App-set from the dossier's delta, never the model.
        assert_eq!(verdict.position_change, PositionChange::New);
        match verdict.disposition {
            VerdictDisposition::Priced(g) => {
                assert!(matches!(
                    g.grade,
                    crate::portfolio::Grade::A
                        | crate::portfolio::Grade::B
                        | crate::portfolio::Grade::C
                ));
                assert!(g.thesis_document.starts_with("Thesis: "), "{}", g.thesis_document);
                assert!(g.appendix.conviction.is_some());
                assert!(g.appendix.expected_price_12m.is_some());
                assert!(g.options_signal.put_call_volume.is_some());
            }
            other => panic!("expected a priced verdict, got {other:?}"),
        }
        assert_eq!(audit.prompt_version, PROMPT_VERSION);
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
                // Neutral-imputed quality with the visible low-confidence marker.
                assert_eq!(g.sub_scores.quality, 50.0);
                assert!(g.low_confidence_grade);
                let tm = g.price_targets.twelve_month.as_ref().unwrap();
                assert!(tm.methodology.contains("fund exposure composite"));
                assert_eq!(g.fund_class_label.as_deref(), Some("US equity fund"));
            }
            other => panic!("expected a priced fund verdict, got {other:?}"),
        }
        assert!(audit.target_meta.unwrap().flat_driver);
    }

    #[test]
    fn engine_gap_notes_reach_the_audit() {
        // A partially covered fund (80% P/E-usable) still grades.
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
                // The stub's role/risk decision is hold.
                assert_eq!(r.action, Action::Hold);
                assert!(r.thesis_document.starts_with("Role: "), "{}", r.thesis_document);
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
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.position.quantity = -100.0;
        d.position.market_value = -19_500.0;
        d.prior_analysis = Some(crate::portfolio::AnalysisRecord {
            text: "A prior analysis.".into(),
            written: "2026-07-20".into(),
            anchor: None,
        });
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        // A no-research exit outside the floor retains nothing, the prior
        // analysis included.
        assert!(audit.research.is_none() && audit.analysis.is_none());
        match verdict.disposition {
            VerdictDisposition::NotRated { reason } => {
                assert!(reason.contains("short"), "{reason}");
            }
            other => panic!("expected not-rated, got {other:?}"),
        }
    }

    #[test]
    fn a_fully_offset_zero_position_is_not_rated_not_graded_long() {
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
    fn an_unsupported_listing_is_not_rated_with_that_reason() {
        use crate::portfolio::listing::ListingResolution;
        // Strong financials prove the gate routes before the engine could grade.
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
            matches!(verdict.disposition, VerdictDisposition::InsufficientEvidence { reason, .. } if reason.contains("unsupported financial units"))
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
        assert!(verdict.thesis_document().is_some_and(|doc| doc.starts_with("Role: ")));
        assert_eq!(verdict.appendix(), None, "the role/risk branch carries no appendix");
    }

    #[test]
    fn a_conflicting_identity_abstains_and_retains_the_prior_thesis_document() {
        use crate::portfolio::listing::ListingResolution;
        // The prior is itself an abstention carrying a retained document.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.listing = Some(ListingResolution::Conflict {
            fmp_name: "Zenith Mining Corp".into(),
        });
        d.prior_verdict = Some(HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            basis_move: None,
            disposition: VerdictDisposition::InsufficientEvidence {
                reason: "fixture".into(),
                prior_thesis_document: Some("The prior document, verbatim.".into()),
            },
            analyzed_at: None,
            action_source: Default::default(),
            side_reversed: false,
        });
        d.prior_analysis = Some(crate::portfolio::AnalysisRecord {
            text: "The prior analysis, retained.".into(),
            written: "2026-07-20".into(),
            anchor: Some(crate::portfolio::engine::DatedValue { date: "2026-07-17".into(), value: 90.0 }),
        });
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-04").unwrap();
        match &verdict.disposition {
            VerdictDisposition::InsufficientEvidence { reason, .. } => {
                assert!(
                    reason.contains("conflicting identity") && reason.contains("Zenith"),
                    "{reason}"
                );
            }
            other => panic!("expected insufficient-evidence, got {other:?}"),
        }
        assert_eq!(verdict.thesis_document(), Some("The prior document, verbatim."));
        // The prior analysis is retained whole, with its own date and anchor,
        // and no research record.
        assert_eq!(audit.analysis, d.prior_analysis);
        assert!(audit.research.is_none());
    }

    /// A stub that keeps the thesis-document message the pipeline renders.
    #[derive(Default)]
    struct ThesisCapture(std::cell::RefCell<Option<String>>);
    impl HoldingAnalyst for ThesisCapture {
        fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
            *self.0.borrow_mut() = Some(thesis_user_prompt(input));
            StubAnalyst.interpret(input)
        }
        fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String> {
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

    /// An abstention carries the retained document's own anchor bar, so the next
    /// continuity run's bridge reads the split factor; a fresh stamp would have
    /// read factor 1.
    #[test]
    fn an_abstention_carries_the_retained_documents_anchor_across_a_split() {
        use crate::portfolio::listing::ListingResolution;
        // Run 1 writes on the pre-split basis.
        let d1 = dossier(AssetClass::Stock, strong_financials());
        let (v1, a1) = analyze_holding(&StubAnalyst, &d1, &rates(), "2026-08-03").unwrap();
        let document = v1.thesis_document().expect("a priced document").to_string();
        let anchor = a1.authoring_close.clone().expect("run 1 stamps its anchor");
        // A 4-for-1 split re-bases the whole fetched series after run 1.
        let mut rebased = strong_financials();
        for c in &mut rebased.daily_closes {
            c.value /= 4.0;
        }
        rebased.current_price = rebased.current_price.map(|p| p / 4.0);
        // Run 2 abstains (a conflicting identity) over run 1's persisted row.
        let mut d2 = dossier(AssetClass::Stock, rebased.clone());
        d2.listing = Some(ListingResolution::Conflict { fmp_name: "Zenith Mining Corp".into() });
        d2.prior_verdict = Some(v1);
        d2.prior_vintage = Some("2026-08-03T20:00:00Z".into());
        d2.prior_spot = a1.quick_basis.as_ref().map(|b| b.spot);
        d2.prior_authoring_close = a1.authoring_close.clone();
        let (v2, a2) = analyze_holding(&StubAnalyst, &d2, &rates(), "2026-08-04").unwrap();
        assert_eq!(v2.thesis_document(), Some(document.as_str()), "the document is retained verbatim");
        assert_eq!(
            a2.authoring_close.as_ref(),
            Some(&anchor),
            "the abstention carries the document's own anchor, never a fresh post-split stamp"
        );
        // Run 3 prices again over the abstention's row (the job preserves the
        // vintage across an abstention; the abstention persisted no quick basis).
        let mut d3 = dossier(AssetClass::Stock, rebased);
        d3.prior_verdict = Some(v2);
        d3.prior_vintage = Some("2026-08-03T20:00:00Z".into());
        d3.prior_spot = a2.quick_basis.as_ref().map(|b| b.spot);
        d3.prior_authoring_close = a2.authoring_close.clone();
        let capture = ThesisCapture::default();
        let (v3, a3) = analyze_holding(&capture, &d3, &rates(), "2026-08-05").unwrap();
        assert!(matches!(v3.disposition, VerdictDisposition::Priced(_)), "{:?}", v3.disposition);
        let message = capture.0.take().expect("the thesis message rendered");
        assert!(
            message.contains(&format!(
                "\nPRIOR THESIS (written 2026-08-03)\nA share split since this document was written \
                 re-based the price series by a factor of 0.2500: multiply the prices it states by \
                 that factor to read them on today's basis. The document is as written.\n{document}"
            )),
            "{message}"
        );
        let fresh = a3.authoring_close.expect("run 3 stamps its anchor");
        assert_eq!(fresh.date, anchor.date);
        assert!((fresh.value - anchor.value / 4.0).abs() < 1e-9, "{fresh:?}");
    }

    /// A pass whose prior anchor bar is missing from the window still stamps its
    /// own anchor, so the next pass bridges from it at factor 1; a carried
    /// anchor would have told the next pass to re-base already-adjusted prices.
    #[test]
    fn an_unresolvable_pass_stamps_its_own_anchor_so_its_document_is_never_rebridged() {
        // Run A on the pre-split basis stamps the 2026-07-15 bar.
        let d1 = dossier(AssetClass::Stock, strong_financials());
        let (v1, a1) = analyze_holding(&StubAnalyst, &d1, &rates(), "2026-08-03").unwrap();
        let anchor_a = a1.authoring_close.clone().expect("run A stamps its anchor");
        assert_eq!(anchor_a.date, "2026-07-15");
        // Run B: a 4-for-1 split re-based the series, and the fetched window no
        // longer carries A's anchor bar.
        let mut rebased = strong_financials();
        for c in &mut rebased.daily_closes {
            c.value /= 4.0;
        }
        rebased.current_price = rebased.current_price.map(|p| p / 4.0);
        let mut missing_bar = rebased.clone();
        missing_bar.daily_closes.retain(|c| c.date != anchor_a.date);
        let mut d2 = dossier(AssetClass::Stock, missing_bar);
        d2.prior_verdict = Some(v1);
        d2.prior_vintage = Some("2026-08-03T20:00:00Z".into());
        d2.prior_spot = a1.quick_basis.as_ref().map(|b| b.spot);
        d2.prior_authoring_close = Some(anchor_a.clone());
        let capture = ThesisCapture::default();
        let (v2, a2) = analyze_holding(&capture, &d2, &rates(), "2026-08-04").unwrap();
        assert!(matches!(v2.disposition, VerdictDisposition::Priced(_)), "{:?}", v2.disposition);
        assert!(
            a2.degraded_inputs.iter().any(|g| g.contains("split-bridge anchor")),
            "the exclusion is a recorded degraded input: {:?}",
            a2.degraded_inputs
        );
        let message = capture.0.take().expect("the thesis message rendered");
        assert!(
            message.contains(
                "\nPRIOR THESIS (written 2026-08-03)\nWhether a share split re-based the price \
                 series since this document was written could not be verified this run: the \
                 close its prices were anchored to is missing from the fetched window. The \
                 document is as written; its prices may sit on a pre-split basis.\nThesis: hold AAPL"
            ),
            "{message}"
        );
        assert!(!message.contains("by a factor of"), "{message}");
        let anchor_b = a2.authoring_close.clone().expect("run B stamps its own anchor");
        assert_ne!(anchor_b, anchor_a, "never the carried pre-split anchor");
        assert_eq!(anchor_b.date, "2026-06-30", "the newest bar before the session");
        assert!(a2.quick_basis.is_some(), "the basis persists beneath the pass's own anchor");
        let VerdictDisposition::Priced(g2) = &v2.disposition else { unreachable!() };
        assert!(g2.authored_band_relation.is_some(), "the band relation stamps too");
        let doc_b = g2.thesis_document.clone();
        // Run C: the bar back in the window, on B's basis.
        let mut d3 = dossier(AssetClass::Stock, rebased);
        d3.prior_verdict = Some(v2.clone());
        d3.prior_vintage = Some("2026-08-04T20:00:00Z".into());
        d3.prior_spot = a2.quick_basis.as_ref().map(|b| b.spot);
        d3.prior_authoring_close = a2.authoring_close.clone();
        let capture = ThesisCapture::default();
        let (_v3, a3) = analyze_holding(&capture, &d3, &rates(), "2026-08-05").unwrap();
        let message = capture.0.take().expect("the thesis message rendered");
        assert!(message.contains(&format!("\nPRIOR THESIS (written 2026-08-04)\n{doc_b}")), "{message}");
        assert!(
            !message.contains("by a factor of") && !message.contains("could not be verified"),
            "{message}"
        );
        assert!(a3.degraded_inputs.iter().all(|g| !g.contains("split-bridge anchor")), "{:?}", a3.degraded_inputs);
    }

    #[test]
    fn an_unverified_guard_proceeds_and_records_the_degraded_input() {
        use crate::portfolio::listing::ListingResolution;
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
    fn the_research_brief_carries_prior_analysis_before_prior_thesis_each_dated() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        assert_eq!(research_brief(&d, rates_static(), None).prior_documents, "");
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        d.prior_verdict = Some(v);
        d.prior_vintage = Some("2026-08-03T20:00:00Z".into());
        // An anchor at twice this run's close on the same bar date reads as a
        // 2-for-1 re-basis of the analysis alone.
        let bar = d.financials.daily_closes[0].clone();
        d.prior_analysis = Some(crate::portfolio::AnalysisRecord {
            text: "The prior analysis, verbatim.".into(),
            written: "2026-08-01".into(),
            anchor: Some(crate::portfolio::engine::DatedValue {
                date: bar.date.clone(),
                value: bar.value * 2.0,
            }),
        });
        let docs = research_brief(&d, rates_static(), None).prior_documents;
        assert!(
            docs.starts_with(
                "\nPRIOR ANALYSIS (written 2026-08-01)\nA share split since this document was \
                 written re-based the price series by a factor of 0.5000: "
            ),
            "{docs}"
        );
        let thesis_at = docs.find("\nPRIOR THESIS (written 2026-08-03)\n").expect("PRIOR THESIS follows");
        assert!(docs[..thesis_at].ends_with("The prior analysis, verbatim.\n"), "{docs}");
        assert!(!docs[thesis_at..].contains("re-based the price series"), "the thesis line is the verdict's own: {docs}");
        assert!(docs[thesis_at..].contains("Thesis: hold AAPL "), "{docs}");
        let section = prior_analysis_section(&d);
        assert!(docs.starts_with(&section) && section.ends_with("The prior analysis, verbatim.\n"), "{section}");
        // A bar missing from the window is unverifiable; no anchor at all, no
        // line.
        d.prior_analysis.as_mut().unwrap().anchor = Some(crate::portfolio::engine::DatedValue {
            date: "1999-01-04".into(),
            value: 1.0,
        });
        assert!(prior_analysis_section(&d).contains("could not be verified this run"), "{}", prior_analysis_section(&d));
        d.prior_analysis.as_mut().unwrap().anchor = None;
        let bare = prior_analysis_section(&d);
        assert!(bare.starts_with("\nPRIOR ANALYSIS (written 2026-08-01)\nThe prior analysis, verbatim.\n"), "{bare}");
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let thesis = thesis_user_prompt(&ThesisInput {
            dossier: &d,
            engine: &engine_output,
            rates: rates_static(),
            analysis: analysis_record("This run's analysis."),
            pre_profit: None,
            soft_forensic: None,
            tech_pre_flag: None,
            narrative: None,
            prior_split: None,
            review: None,
        });
        assert!(!thesis.contains("PRIOR ANALYSIS"), "{thesis}");
        assert!(thesis.contains("\nANALYSIS\nThis run's analysis.\n") && thesis.contains("\nPRIOR THESIS (written 2026-08-03)\n"), "{thesis}");
        d.prior_analysis = None;
        let docs = research_brief(&d, rates_static(), None).prior_documents;
        assert!(docs.starts_with("\nPRIOR THESIS (written 2026-08-03)\n") && !docs.contains("PRIOR ANALYSIS"), "{docs}");
    }

    /// The tests' shorthand for "this run's analysis": written on the fixture's
    /// date, no anchor.
    fn analysis_record(text: &str) -> crate::portfolio::AnalysisRecord {
        crate::portfolio::AnalysisRecord {
            text: text.into(),
            written: "2026-07-28".into(),
            anchor: None,
        }
    }

    #[test]
    fn a_carried_analysis_renders_dated_and_bridged_under_analysis_on_both_thesis_messages() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let carried_on = |series: &HoldingDossier| {
            let bar = series.financials.daily_closes[0].clone();
            crate::portfolio::AnalysisRecord {
                text: "The carried analysis.".into(),
                written: "2026-07-01".into(),
                anchor: Some(crate::portfolio::engine::DatedValue {
                    date: bar.date.clone(),
                    value: bar.value * 2.0,
                }),
            }
        };
        let input = |analysis: crate::portfolio::AnalysisRecord| ThesisInput {
            dossier: &d,
            engine: &engine_output,
            rates: rates_static(),
            analysis,
            pre_profit: None,
            soft_forensic: None,
            tech_pre_flag: None,
            narrative: None,
            prior_split: None,
            review: None,
        };
        let thesis = thesis_user_prompt(&input(carried_on(&d)));
        assert!(
            thesis.contains(
                "\nANALYSIS (written 2026-07-01)\nA share split since this document was written \
                 re-based the price series by a factor of 0.5000: multiply the prices it states \
                 by that factor to read them on today's basis. The document is as written.\nThe \
                 carried analysis.\n"
            ),
            "{thesis}"
        );
        assert!(!thesis.contains("\nANALYSIS\n"), "{thesis}");
        let fresh = thesis_user_prompt(&input(analysis_record("This run's analysis.")));
        assert!(fresh.contains("\nANALYSIS\nThis run's analysis.\n"), "{fresh}");
        assert!(!fresh.contains("re-based the price series") && !fresh.contains("ANALYSIS (written"), "{fresh}");
        let fund = fund_dossier(bond_fund());
        let role = role_risk_user_prompt(&RoleRiskInput {
            dossier: &fund,
            readout: &RoleRiskReadout::default(),
            rates: rates_static(),
            analysis: carried_on(&fund),
            prior_split: None,
            review: None,
        });
        assert!(
            role.contains("\nANALYSIS (written 2026-07-01)\nA share split since this document was written re-based the price series by a factor of 0.5000: "),
            "{role}"
        );
        assert!(role.contains("The document is as written.\nThe carried analysis.\n"), "{role}");
    }

    #[test]
    fn a_scripted_research_run_persists_the_analysis_and_the_thesis_message_reads_it() {
        // Two write-ups within budget, so no distillation.
        #[derive(Default)]
        struct Scripted(std::cell::RefCell<Option<String>>);
        impl HoldingAnalyst for Scripted {
            fn research(&self, _: &HoldingDossier, plan: &ResearchPlan) -> Result<HoldingResearch> {
                let mut out = research::offline_stub(plan);
                for (i, t) in out.topics.iter_mut().enumerate().take(2) {
                    t.write_up = Some(format!("Write-up {i} on {}.", t.title));
                    t.passes = 1;
                    t.skipped = None;
                }
                out.disconfirming = Some("Nothing contrary.".into());
                Ok(out)
            }
            fn analyze(&self, input: &distill::AnalysisInput<'_>) -> Result<String> {
                StubAnalyst.analyze(input)
            }
            fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
                *self.0.borrow_mut() = Some(thesis_user_prompt(input));
                StubAnalyst.interpret(input)
            }
            forward_stub_verdict_calls!();
        }
        let analyst = Scripted::default();
        let d = dossier(AssetClass::Stock, strong_financials());
        let (verdict, audit) = analyze_holding(&analyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(matches!(verdict.disposition, VerdictDisposition::Priced(_)), "{verdict:?}");
        let written = audit.analysis.as_ref().expect("the analysis persists on the audit");
        assert_eq!(written.written, d.analysis_date);
        assert_eq!(written.anchor, audit.authoring_close);
        assert!(written.anchor.is_some());
        let analysis = written.text.as_str();
        let record = audit.research.as_ref().expect("the research record persists");
        let titles: Vec<&str> = record.write_ups.iter().take(2).map(|t| t.title.as_str()).collect();
        assert!(
            analysis.starts_with(&format!(
                "{}\nWrite-up 0 on {}.\n\n{}\nWrite-up 1 on {}.",
                titles[0], titles[0], titles[1], titles[1]
            )),
            "{analysis}"
        );
        assert!(analysis.ends_with("\n\nContrary evidence\nNothing contrary."), "{analysis}");
        assert_eq!(record.distillation, distill::DistillationRecord::none());
        let message = analyst.0.take().expect("the thesis message was captured");
        assert!(message.contains(&format!("\nANALYSIS\n{analysis}\n")), "{message}");
        // The default seam refuses an unscripted analysis call, naming the stage.
        struct Bare;
        impl HoldingAnalyst for Bare {
            fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
                StubAnalyst.interpret(input)
            }
            forward_stub_verdict_calls!();
        }
        let brief = research_brief(&d, rates_static(), None);
        let write_ups = [distill::WriteUp { key: "k".into(), title: "T".into(), text: "t".into() }];
        let err = Bare
            .analyze(&distill::AnalysisInput {
                symbol: "AAPL",
                brief: &brief,
                prior_analysis: "",
                write_ups: distill::WriteUps::AsWritten(&write_ups),
            })
            .unwrap_err();
        assert!(format!("{err:#}").starts_with("analysis AAPL: "), "{err:#}");
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
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record("findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
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
        // Data only, no score-input narration.
        for narration in ["COMMODITY CONTEXT", "never a score input", "as-of evidence"] {
            assert!(!interp.contains(narration), "`{narration}` leaked: {interp}");
        }
        let bare = dossier(AssetClass::Stock, strong_financials());
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &bare,
            engine: &engine_output,
            analysis: analysis_record("findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(!interp.contains("COMMODITY PRICES"), "{interp}");
    }

    /// Every row FETCHED VALUES renders, so the order pin reads the whole block.
    fn full_evidence() -> crate::portfolio::evidence::CompanyEvidence {
        use crate::portfolio::evidence::*;
        CompanyEvidence {
            symbol: "AAPL".into(),
            ratios: RatioLines {
                pe: Some(32.89),
                pb: Some(47.37),
                ev_to_ebitda: Some(23.42),
                ev_to_sales: Some(8.13),
                fcf_yield: Some(0.0312),
                roic: Some(0.452),
                roe: Some(1.453),
                net_debt_to_ebitda: Some(0.484),
            },
            owner_earnings: Some(OwnerEarningsRow {
                period_end: "2024-12-28".into(),
                period: Some("FY2025 Q1".into()),
                owners_earnings: Some(27.66e9),
                per_share: Some(1.83),
            }),
            enterprise_value: Some(EnterpriseValueRow {
                date: "2024-09-28".into(),
                enterprise_value: Some(3.57e12),
                market_cap: Some(3.50e12),
                total_debt: Some(106.6e9),
                cash: Some(29.9e9),
            }),
            price_target: Some(PriceTargetConsensus {
                high: Some(400.0),
                low: Some(253.0),
                median: Some(325.0),
                consensus: Some(323.82),
            }),
            price_target_trend: Some(PriceTargetTrend {
                last_month: PriceTargetWindow {
                    count: Some(3),
                    average: Some(380.0),
                },
                last_quarter: PriceTargetWindow {
                    count: Some(10),
                    average: Some(322.6),
                },
                last_year: PriceTargetWindow {
                    count: Some(58),
                    average: Some(293.43),
                },
            }),
            grades_consensus: Some(GradesConsensus {
                strong_buy: Some(1),
                buy: Some(69),
                hold: Some(33),
                sell: Some(7),
                strong_sell: Some(0),
                consensus: Some("Buy".into()),
            }),
            rating_actions: vec![RatingAction {
                date: "2026-05-26".into(),
                firm: "B of A Securities".into(),
                previous_grade: Some("Buy".into()),
                new_grade: Some("Buy".into()),
                action: Some("maintain".into()),
            }],
            ratings_snapshot: Some(RatingsSnapshot {
                rating: Some("B".into()),
                overall: Some(3),
                discounted_cash_flow: Some(3),
                return_on_equity: Some(5),
                return_on_assets: Some(5),
                debt_to_equity: Some(1),
                price_to_earnings: Some(2),
                price_to_book: Some(1),
            }),
            insider_trades: vec![InsiderTrade {
                transaction_date: "2026-06-05".into(),
                filing_date: Some("2026-06-06".into()),
                name: "BOLDUC JOHN".into(),
                owner_type: Some("director".into()),
                transaction_type: Some("P-Purchase".into()),
                shares: Some(3570.0),
                price: Some(6.77),
            }],
            insider_statistics: Some(InsiderStatistics {
                year: Some(2026),
                quarter: Some(2),
                acquired_transactions: Some(5),
                disposed_transactions: Some(35),
                total_acquired: Some(272_855.0),
                total_disposed: Some(880_558.0),
            }),
            congressional_trades: vec![CongressionalTrade {
                chamber: Chamber::Senate,
                transaction_date: "2026-04-17".into(),
                disclosure_date: Some("2026-05-07".into()),
                name: "Shelley Moore Capito".into(),
                owner: Some("Spouse".into()),
                kind: Some("Sale".into()),
                amount: Some("$1,001 - $15,000".into()),
            }],
            float: Some(SharesFloat {
                date: Some("2026-06-05".into()),
                free_float_percent: Some(99.83),
                float_shares: Some(14.66e9),
                outstanding_shares: Some(14.69e9),
            }),
            product_segments: vec![SegmentYear {
                fiscal_year: Some(2024),
                period_end: "2024-09-28".into(),
                segments: vec![("iPhone".into(), 201.2e9), ("Mac".into(), 30.0e9)],
            }],
            geographic_segments: vec![SegmentYear {
                fiscal_year: Some(2024),
                period_end: "2024-09-28".into(),
                segments: vec![("Americas".into(), 167.0e9)],
            }],
            splits: vec![SplitRow {
                date: "2020-08-31".into(),
                numerator: 4.0,
                denominator: 1.0,
            }],
            earnings: vec![
                crate::fmp::SymbolEarningsRow {
                    date: "2026-10-29".into(),
                    eps_actual: None,
                    eps_estimated: Some(1.2),
                    revenue_actual: None,
                },
                crate::fmp::SymbolEarningsRow {
                    date: "2026-07-22".into(),
                    eps_actual: Some(1.1),
                    eps_estimated: Some(1.0),
                    revenue_actual: Some(90.0e9),
                },
            ],
            gaps: vec![],
        }
    }

    #[test]
    fn fetched_values_renders_every_row_in_the_docs_order() {
        use crate::portfolio::evidence::{DividendRow, MaMatch, MaRole};
        let mut fin = strong_financials();
        fin.year_high = Some(260.0);
        fin.year_low = Some(170.0);
        fin.recent_dividends = vec![
            DividendRow {
                date: "2026-08-11".into(),
                amount: 0.26,
                payment_date: Some("2026-08-14".into()),
            },
            DividendRow {
                date: "2026-05-12".into(),
                amount: 0.25,
                payment_date: None,
            },
        ];
        let mut d = dossier(AssetClass::Stock, fin);
        d.analysis_date = "2026-10-08".into();
        d.company_name = Some("Apple Inc.".into());
        d.issuer = Some(crate::portfolio::dossier::IssuerProfile {
            exchange: Some("NASDAQ".into()),
            sector: Some("Technology".into()),
            industry: Some("Consumer Electronics".into()),
        });
        d.filings_8k = vec![
            crate::sec::RecentFiling {
                form: "8-K".into(),
                filing_date: "2026-07-31".into(),
                items: Some(vec!["2.02".into(), "9.01".into()]),
                accession: String::new(),
            },
            crate::sec::RecentFiling {
                form: "8-K/A".into(),
                filing_date: "2026-05-02".into(),
                items: Some(vec![]),
                accession: String::new(),
            },
        ];
        d.short_interest = Some(crate::finra::ShortInterestRead {
            settlement_date: "2026-09-30".into(),
            current_short_interest: 5_000_000.0,
            previous_short_interest: Some(4_000_000.0),
            average_daily_volume: Some(2_000_000.0),
            days_to_cover: Some(2.5),
        });
        d.evidence = Some(full_evidence());
        d.ma_matches = vec![MaMatch {
            role: MaRole::Target,
            counterparty: "Global Net Lease, Inc.".into(),
            date: "2026-06-01".into(),
            link: Some("https://sec.gov/x".into()),
        }];
        let block = fetched_values_section(&d, rates_static());
        let expected = [
            "\nPROFILE\n- name: Apple Inc.\n- exchange: NASDAQ\n- sector: Technology\n\
             - industry: Consumer Electronics\n- market capitalization: 3000.0B\n\
             - shares outstanding: 15.0B\n",
            "\nSTATEMENTS\nQuarterly statements as reported, newest first (period end, then \
             the figure)\n- revenue: 2026-06-30 100.0B; 2026-03-31 99.0B; ",
            "\n- diluted EPS: 2026-06-30 1.55; 2026-03-31 1.54; ",
            "\n- cash and equivalents: 2026-06-30 60.0B; ",
            "\nCONSENSUS\n- consensus EPS by fiscal period end, as published: 2027-06-30 6.50\n",
            "\nDIVIDENDS\n- dividends, latest ex-dates first (ex-date, amount per share): \
             2026-08-11 0.26 (paid 2026-08-14); 2026-05-12 0.25\n",
            "\nQUOTE AND CLOSES\n- quote: 195.00 per share (the live print, undated)\n- 52-week range as \
             served: low 170.00, high 260.00\n- daily closes: 17 sessions from 2022-09-30 to \
             2026-07-15\n- close three months before the analysis date: 190.00 (2026-06-30)\n\
             - close twelve months before the analysis date: 178.00 (2025-09-30)\n- close \
             three years before the analysis date: 146.00 (2023-09-30)\n",
            "\nFILINGS\n8-K filings of the trailing twelve months, newest first (filing date: \
             items)\n- 2026-07-31: 2.02, 9.01\n- 2026-05-02 (8-K/A): (no items)\n",
            "\nSHORT INTEREST PRINT\n- settlement: 2026-09-30 (FINRA)\n- shares short: 5000000\n\
             - shares short at the prior settlement: 4000000\n- average daily volume: \
             2000000\n- days to cover: 2.50\n",
            "\nSTREET\n- price targets: consensus 323.82, median 325.00, low 253.00, high \
             400.00\n- targets published: last month 3 averaging 380.00; last quarter 10 \
             averaging 322.60; last year 58 averaging 293.43\n- analyst ratings: strong buy 1, \
             buy 69, hold 33, sell 7, strong sell 0; consensus Buy\nRating actions, newest \
             first (date: firm, previous grade to new grade, action)\n- 2026-05-26: B of A \
             Securities, Buy to Buy, maintain\n- FMP rating B: overall 3; discounted cash flow \
             3, return on equity 5, return on assets 5, debt to equity 1, price to earnings 2, \
             price to book 1\n",
            "\nINSIDERS AND CONGRESS\nInsider trades, newest first (transaction date: name, \
             role, type, shares at price, filing date)\n- 2026-06-05: BOLDUC JOHN, director, \
             P-Purchase, 3570 shares at 6.77, filed 2026-06-06\n- insider statistics, 2026 Q2: \
             5 acquiring and 35 disposing transactions; 272855 shares acquired, 880558 \
             disposed\nCongressional trades, newest first (transaction date: chamber, member, \
             owner, type, amount, disclosure date)\n- 2026-04-17: Senate, Shelley Moore \
             Capito, Spouse, Sale, $1,001 - $15,000, disclosed 2026-05-07\n",
            "\nEARNINGS\nSurprises, newest first (announcement date: EPS actual vs estimate, \
             revenue actual)\n- 2026-07-22: EPS 1.10 vs 1.00 estimated, revenue 90.0B\n- next \
             earnings: 2026-10-29 (EPS estimate 1.20)\n",
            "\nVALUATION\n- trailing-twelve-month ratios: P/E 32.89; EV/EBITDA 23.42; \
             EV/sales 8.13; P/B 47.37; FCF yield 3.1%; ROIC 45.2%; ROE 145.3%; net \
             debt/EBITDA 0.48\n- owner earnings (FY2025 Q1, period end 2024-12-28): 27.7B; \
             1.83 per share\n- enterprise value (2024-09-28): 3570.0B; market capitalization \
             3500.0B; total debt 106.6B; cash 29.9B\n- float (2026-06-05): float shares 14.7B; \
             shares outstanding 14.7B; free float 99.8%\n",
            "\nM&A\nSEC merger registration filings, trailing twelve months; a filing follows \
             a deal's announcement, typically by several weeks, and covers deals paid at least \
             partly in stock\n- target of Global Net Lease, Inc., filed 2026-06-01 \
             (https://sec.gov/x)\n",
            "\nSEGMENTS\nRevenue by product, newest fiscal year first\n- FY2024 (period end \
             2024-09-28): iPhone 201.2B, Mac 30.0B\nRevenue by geography, newest fiscal year \
             first\n- FY2024 (period end 2024-09-28): Americas 167.0B\n",
            "\nRATES\n- Treasury yields (FRED): 10-year ",
        ];
        let mut cursor = 0;
        for line in expected {
            let at = block[cursor..]
                .find(line)
                .unwrap_or_else(|| panic!("missing or out of order: {line}\n{block}"));
            cursor += at + line.len();
        }
        // The computations stay out.
        assert!(
            !block.contains("over the trailing twelve months"),
            "{block}"
        );
        assert!(!block.contains("% vs the prior settlement"), "{block}");
        assert_eq!(
            research_brief(&d, rates_static(), None).fetched_values,
            block
        );
    }

    #[test]
    fn a_statement_series_keeps_one_unit_and_its_smallest_cell() {
        let mut fin = strong_financials();
        fin.quarterly_income[1].net_income = Some(0.04e9);
        fin.quarterly_income[2].net_income = Some(-0.04e9);
        fin.quarterly_income[3].net_income = Some(0.01e9);
        fin.quarterly_income[4].net_income = None;
        fin.quarterly_income[1].gross_profit = Some(1.0e5);
        fin.quarterly_income[2].gross_profit = Some(4.0e4);
        for row in &mut fin.quarterly_income {
            row.operating_income = Some(5.0e6);
        }
        let block = fetched_values_section(&dossier(AssetClass::Stock, fin), rates_static());
        assert!(
            block.contains("\n- net income: 2026-06-30 25.000B; 2026-03-31 0.040B; 2025-12-31 -0.040B; 2025-09-30 0.010B; 2025-06-30 (gap); 2025-03-31 25.000B; "),
            "{block}"
        );
        // At the four-decimal cap 100,000 keeps a digit in a billions row and 40,000 does not.
        assert!(
            block.contains("\n- gross profit: 2026-06-30 45.0000B; 2026-03-31 0.0001B; 2025-12-31 0.0000B; "),
            "{block}"
        );
        assert!(block.contains("\n- operating income: 2026-06-30 5.0M; 2026-03-31 5.0M; "), "{block}");
        assert!(block.contains("\n- revenue: 2026-06-30 100.0B; 2026-03-31 99.0B; "), "{block}");
    }

    #[test]
    fn fetched_values_omits_absent_rows_and_cuts_a_funds_countries_at_ten() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let block = fetched_values_section(&d, rates_static());
        for absent in [
            "DIVIDENDS",
            "52-week",
            "FILINGS",
            "SHORT INTEREST PRINT",
            "STREET",
            "price targets",
            "analyst ratings",
            "Rating actions",
            "FMP rating",
            "INSIDERS",
            "Congressional",
            "EARNINGS",
            "next earnings",
            "VALUATION",
            "trailing-twelve-month ratios",
            "owner earnings",
            "enterprise value",
            "float",
            "M&A",
            "SEGMENTS",
        ] {
            assert!(
                !block.contains(absent),
                "{absent} rendered with no source:\n{block}"
            );
        }
        let mut d = d;
        d.evidence = Some(crate::portfolio::evidence::CompanyEvidence::empty("AAPL"));
        let block = fetched_values_section(&d, rates_static());
        assert!(!block.contains("trailing-twelve-month ratios"), "{block}");
        assert!(!block.contains("price targets"), "{block}");
        // The countries are served smallest first, so the cut must sort.
        let mut fund = us_equity_fund();
        fund.sector_weights = (0..12).map(|i| (format!("Sector {i}"), 0.05)).collect();
        fund.country_weights = (0..12)
            .map(|i| (format!("Country {i}"), 0.01 * (i + 1) as f64))
            .collect();
        let block = fetched_values_section(&fund_dossier(fund), rates_static());
        assert!(block.contains("\nWEIGHTS\n- sectors: Sector 0 5.0%, "), "{block}");
        assert!(block.contains("Sector 11 5.0%\n"), "{block}");
        assert!(
            block.contains("- countries, largest first: Country 11 12.0%, Country 10 11.0%"),
            "{block}"
        );
        assert!(block.contains("Country 2 3.0%\n"), "{block}");
        assert!(
            !block.contains("Country 1 2.0%") && !block.contains("Country 0 1.0%"),
            "{block}"
        );
    }

    #[test]
    fn the_split_line_names_the_split_the_feed_carries_after_the_document() {
        use crate::portfolio::evidence::{CompanyEvidence, SplitRow};
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let mut e = CompanyEvidence::empty("AAPL");
        e.splits = vec![
            SplitRow {
                date: "2026-06-15".into(),
                numerator: 3.0,
                denominator: 1.0,
            },
            SplitRow {
                date: "2020-08-31".into(),
                numerator: 4.0,
                denominator: 1.0,
            },
        ];
        d.evidence = Some(e);
        let since = split_event_since(&d, "2026-06-01").expect("the June split");
        assert_eq!((since.numerator, since.denominator), (3.0, 1.0));
        assert!(
            split_event_since(&d, "2026-06-15").is_none(),
            "a split on the document's own session is not after it"
        );
        assert!(split_event_since(
            &dossier(AssetClass::Stock, strong_financials()),
            "2020-01-01"
        )
        .is_none());
        let named = document_section(
            "PRIOR THESIS",
            Some("2026-06-01"),
            split_context_of(Some(0.3333), Some(since)),
            "doc",
        );
        assert!(
            named.contains(
                "A 3-for-1 share split on 2026-06-15 since this document was written re-based \
                 the price series by a factor of 0.3333"
            ),
            "{named}"
        );
        let bare = document_section(
            "PRIOR THESIS",
            Some("2026-06-01"),
            split_context_of(Some(0.3333), None),
            "doc",
        );
        assert!(
            bare.contains("A share split since this document was written re-based"),
            "{bare}"
        );
        assert!(
            split_context_of(Some(1.0), Some(since)).is_none(),
            "no re-basis, no line, whatever the feed carries"
        );
    }

    #[test]
    fn a_matched_ma_feed_labels_the_audit_where_the_thesis_renders_it() {
        use crate::portfolio::evidence::{MaMatch, MaRole};
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.ma_matches = vec![MaMatch {
            role: MaRole::Acquirer,
            counterparty: "Modiv Industrial, Inc.".into(),
            date: "2026-06-01".into(),
            link: None,
        }];
        let sources = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03")
            .unwrap()
            .1
            .sources;
        assert!(
            sources.iter().any(|s| s.contains("M&A feed")),
            "{sources:?}"
        );
        let plain = analyze_holding(
            &StubAnalyst,
            &dossier(AssetClass::Stock, strong_financials()),
            &rates(),
            "2026-08-03",
        )
        .unwrap()
        .1
        .sources;
        assert!(!plain.iter().any(|s| s.contains("M&A feed")), "{plain:?}");
    }

    fn scores_block(fin: CompanyFinancials, profile: engine::GradeProfile) -> String {
        let d = dossier(AssetClass::Stock, fin);
        let engine_output = match engine::analyze(&d.financials, &rates(), profile) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record("findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        let start = user.find("\nSCORES\n").unwrap();
        let end = user.find("\nPRICE BANDS (USD)\n").unwrap();
        user[start..end].to_string()
    }

    #[test]
    fn the_scores_block_lists_each_scores_factors_with_their_bands() {
        let tech = engine::GradeProfile { sector: Some("Technology"), industry: None };
        let block = scores_block(strong_financials(), tech);
        assert!(
            block.contains(
                "The grade is a letter derived from the quality, valuation and risk scores. Each \
                 score is the mean of the factors listed under it, each read over the trailing \
                 twelve months where it reads the statements. A factor scores 50 at its band's \
                 anchor and 0 or 100 at the band's edges, a value past an edge held at it. Where \
                 the company has 8 or more earlier trailing-year readings of a factor, that score \
                 is averaged with the factor's rank among them, on the same scale. A factor that \
                 could not be computed scores 50.\nquality "
            ),
            "{block}"
        );
        assert!(
            block.contains(
                "\nquality\n- gross profit over total assets: 0.5143; band 0.3500 ± 0.3500 \
                 (Technology): 73; rank among 12 earlier readings: 50; score 62\n\
                 - operating income over revenue: not available; scored 50\n"
            ),
            "{block}"
        );
        assert!(block.contains("\nvaluation\n- net income over market value: 0.0333; band 0.0264 ± 0.0264 (Technology): 63; score 63\n"), "{block}");
        assert!(block.contains("\nmomentum\n- price return from a year ago to a month ago: not available; scored 50\n"), "{block}");
        assert!(block.contains("\nrisk\n- beta (lower is better): 1.1000; band 1.0000 ± 0.6000 (market-wide): 42; score 42\n"), "{block}");
        assert!(
            block.contains("- debt over equity (lower is better): 1.4286; band 0.4000 ± 0.4000 (Technology): 0, past its edge; rank among 12 earlier readings: 50; score 25\n"),
            "{block}"
        );
        assert!(!block.contains("One score is imputed"), "{block}");
        assert!(!block.contains("earlier readings, so"), "not thin: {block}");
    }

    #[test]
    fn a_banks_scores_read_its_own_factors_and_name_the_return_and_hurdle() {
        let bank = engine::GradeProfile {
            sector: Some("Financial Services"),
            industry: Some("Banks - Regional"),
        };
        let block = scores_block(strong_financials(), bank);
        assert!(
            block.contains(
                "\nquality\n- return on equity less its cost of equity: 1.3382 (return on equity \
                 1.4286 less cost of equity 0.0903); band 0.0000 ± 0.1500 (market-wide): 100, past \
                 its edge; rank among 12 earlier readings: 50; score 75\n\
                 - net income over total assets: 0.2857; band 0.0120 ± 0.0120 (Banks): 100, past \
                 its edge; rank among 12 earlier readings: 50; score 75\nvaluation\n"
            ),
            "{block}"
        );
        assert!(block.contains("- book equity over market value: 0.0233; band 0.7470 ± 0.7470 (Banks): 2; score 2\n"), "{block}");
        assert!(!block.contains("debt over equity"), "a bank reads no debt / equity: {block}");
        assert!(!block.contains("gross profit"), "{block}");
    }

    #[test]
    fn the_roic_spread_line_names_its_return_and_cost_of_capital() {
        let mut fin = strong_financials();
        for row in &mut fin.quarterly_income {
            row.operating_income = Some(30.0e9);
        }
        let block = scores_block(fin, engine::GradeProfile::default());
        let line = block
            .lines()
            .find(|l| l.starts_with("- return on invested capital less its cost of capital: "))
            .unwrap_or_else(|| panic!("{block}"));
        assert!(line.contains(" (return 0."), "{line}");
        assert!(line.contains(" less cost of capital 0."), "{line}");
        assert!(line.contains("(market-wide): "), "{line}");
    }

    #[test]
    fn an_imputed_axis_and_thin_history_are_said_once() {
        // With no gross profit every quality factor is unavailable, so the
        // score is imputed.
        let mut fin = strong_financials();
        for row in &mut fin.quarterly_income {
            row.gross_profit = None;
        }
        let block = scores_block(fin, engine::GradeProfile::default());
        assert!(block.contains(". One score is imputed. Risk tier: "), "{block}");
        assert!(
            block.contains(
                "\nquality\n- gross profit over total assets: not available; scored 50\n\
                 - operating income over revenue: not available; scored 50\n"
            ),
            "{block}"
        );
        // Four quarters alone: one window, no earlier readings — thin.
        let mut fin = strong_financials();
        fin.quarterly_income.truncate(4);
        fin.quarterly_balance_sheet.truncate(4);
        let block = scores_block(fin, engine::GradeProfile::default());
        assert!(
            block.ends_with(
                "Most of these factors had fewer than 8 earlier readings, so their scores rest \
                 mainly on the bands.\n"
            ),
            "{block}"
        );
    }

    #[test]
    fn a_funds_scores_carry_no_factor_lines() {
        let fd = fund_dossier(us_equity_fund());
        let ctx = fd.fund.as_ref().unwrap();
        let FundEngineVerdict::Priced(out) = fund::analyze_fund(&fund::FundEngineInputs {
            fund: &ctx.fund,
            financials: &fd.financials,
            sector_pe: &ctx.sector_pe,
            sector_pe_history: &ctx.sector_pe_history,
            rates: &rates(),
            as_of: ctx.as_of,
        }) else {
            panic!("a priced fund");
        };
        let user = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &fd,
            engine: &out,
            analysis: analysis_record("findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        let block = &user[user.find("\nSCORES\n").unwrap()..user.find("\nPRICE BANDS (USD)\n").unwrap()];
        assert!(block.contains("risk scores.\nquality 50, valuation "), "{block}");
        assert!(!block.contains("Each score is the mean"), "{block}");
        assert!(!block.contains("\nquality\n"), "{block}");
    }

    #[test]
    fn the_research_brief_fetched_values_are_the_thesis_message_block_to_the_byte() {
        let mut fin = strong_financials();
        fin.quarterly_cash_flow = fin
            .quarterly_income
            .iter()
            .take(8)
            .map(|r| crate::portfolio::engine::QuarterlyCashFlowRow {
                period_end: r.period_end.clone(),
                filing_date: None,
                free_cash_flow: Some(20.0e9),
                operating_cash_flow: Some(28.0e9),
                capex: Some(-8.0e9),
                depreciation_and_amortization: None,
            })
            .collect();
        let d = dossier(AssetClass::Stock, fin);
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let block = research_brief(&d, rates_static(), None).fetched_values;
        assert!(block.starts_with("\nFETCHED VALUES\n"), "{block}");
        for line in [
            "\nSTATEMENTS\nQuarterly statements as reported, newest first (period end, then \
             the figure)\n- revenue: 2026-06-30 100.0B; 2026-03-31 99.0B; ",
            "\n- gross profit: 2026-06-30 45.0B; ",
            "\n- operating income: 2026-06-30 (gap); 2026-03-31 (gap); ",
            "\n- net income: 2026-06-30 25.0B; ",
            "\n- diluted EPS: 2026-06-30 1.55; ",
            "\n- diluted shares: 2026-06-30 15.0B; ",
            "\n- operating cash flow: 2026-06-30 28.0B; ",
            "\n- free cash flow: 2026-06-30 20.0B; ",
            "\n- capital expenditure: 2026-06-30 -8.0B; ",
            "\n- total debt: 2026-06-30 100.0B; ",
            "\n- total equity: 2026-06-30 70.0B; ",
            "\n- cash and equivalents: 2026-06-30 60.0B; ",
        ] {
            assert!(block.contains(line), "{line}\n{block}");
        }
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record("findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(interp.contains(block.as_str()), "{interp}");

        let fd = fund_dossier(us_equity_fund());
        let block = research_brief(&fd, rates_static(), None).fetched_values;
        let role = role_risk_user_prompt(&RoleRiskInput {
            rates: rates_static(),
            prior_split: None,
            dossier: &fd,
            readout: &RoleRiskReadout::default(),
            analysis: analysis_record("No research findings."),
            review: None,
        });
        assert!(role.contains(block.as_str()), "{role}");
    }

    #[test]
    fn a_fired_pre_flag_renders_and_an_unfired_one_stays_silent() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
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
            thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
                dossier: &d,
                engine: &engine_output,
                analysis: analysis_record("findings"),
                pre_profit: None,
                tech_pre_flag: f,
                narrative: None,
                review: None,
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

        let bare = dossier(AssetClass::Stock, strong_financials());
        let (_, audit) = analyze_holding(&StubAnalyst, &bare, &rates(), "2026-08-03").unwrap();
        assert!(!audit.sources.iter().any(|s| {
            s.contains("CBOE") || s.contains("CFTC") || s.contains("benchmark")
        }));

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
        // evaluation.
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
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record("findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        const LINE: &str = "Market-wide options sentiment (CBOE daily put/call, as of August 19, \
                            2026): total 0.80, index 0.97, equity (gap)\n";
        assert!(interp.contains(LINE), "{interp}");
        assert!(!interp.contains("MARKET OPTIONS SENTIMENT"), "{interp}");

        let mut fd = fund_dossier(us_equity_fund());
        fd.put_call_backdrop = Some(backdrop);
        let role = role_risk_user_prompt(&RoleRiskInput {
            rates: rates_static(),
            prior_split: None,
            dossier: &fd,
            readout: &RoleRiskReadout::default(),
            analysis: analysis_record("No research findings."),
            review: None,
        });
        assert!(role.contains(LINE), "{role}");
        let bare = dossier(AssetClass::Stock, strong_financials());
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &bare,
            engine: &engine_output,
            analysis: analysis_record("findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(!interp.contains("Market-wide options sentiment"), "{interp}");
    }

    #[test]
    fn a_tripped_hard_forensic_states_the_rule_and_annotates_the_audit() {
        use crate::portfolio::ForensicFilingState;
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

        let forensic = audit.forensic.expect("the sweep state persists on the audit");
        assert!(forensic.state.hard_tripped());
        assert!(
            forensic.matched_rule.as_deref().unwrap_or("").contains("reads the exit family"),
            "{forensic:?}"
        );
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        assert!(matches!(graded.engine_rung, Action::Trim | Action::SellAll), "{:?}", graded.engine_rung);
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record("findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(
            interp.contains(
                "\nFORENSIC FILINGS (8-K sweep)\nEvents found:\n- restatement (Item 4.02 \
                 non-reliance) — filed 2026-07-20 (8-K accession 0000320193-26-000042; \
                 filing-declared item code)\n"
            ),
            "{interp}"
        );
        // No arm, trigger or binding narration.
        const RULE: &str = "By rule: the computed action set excludes adding and the computed \
                            action reads the exit family; the grade is unchanged.\n";
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
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(action.contains("\nFORENSIC FILINGS (8-K sweep)\nEvents found:\n"), "{action}");
        // The rule sentence renders on the thesis packet only; the action
        // packet carries the narrowed set on its own line.
        assert!(!action.contains("By rule:"), "{action}");
        assert!(
            action.contains("\nSUPPORTED ACTIONS\nThe rungs a fixed rule over the holding's reads supports, listed in full: sell-all, trim, hold. A rung not listed is outside that rule.\n"),
            "{action}"
        );
        assert!(!action.contains("HARD TRIGGER TRIPPED"), "{action}");

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
        assert_eq!(
            graded.engine_rung,
            engine::engine_action(engine_output.grade, &engine_output.hurdle, None, false)
        );
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &unknown,
            engine: &engine_output,
            analysis: analysis_record("findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
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
        let d = dossier(AssetClass::Stock, strong_financials());
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let engine_set =
            engine::feasible_actions(engine_output.grade, &engine_output.hurdle, None, false);
        let user = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::Priced { graded, engine: &engine_output, pre_profit: None },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        let system = action_system_prompt();
        assert_eq!(
            system,
            "You are an equity analyst deciding the portfolio action for one holding in a \
             portfolio review. You will return action and rationale, as one JSON object. \
             Part 1 of the message gives the inputs. Part 2 defines those outputs and gives \
             the shape to return."
        );
        let (part1, part2) = user.split_once("\n======== PART 2: TASK ========\n").unwrap();
        assert!(part1.starts_with("======== PART 1: INPUTS ========\nHOLDING\n"), "{part1}");
        let sections = [
            "POSITION\n",
            "VERDICT\n",
            "COMPUTED\n",
            "COMPUTED ACTION\n",
            "GRADE\n",
            "PRICE BANDS (USD, with the move each implies from the current price; the analyst's \
             expected price from VERDICT beside each)\n",
            "CAPITAL EFFICIENCY\n",
            "SUPPORTED ACTIONS\n",
            "INVESTOR PROFILE\n",
        ];
        let mut last = 0;
        for section in sections {
            let key = format!("\n{section}");
            assert_eq!(part1.matches(&key).count(), 1, "{section}: {part1}");
            let at = part1.find(&key).unwrap();
            assert!(at > last, "{section} out of order: {part1}");
            last = at;
        }
        assert!(
            part1.contains(
                "\nPOSITION\nThe holding as the account carries it: the shares held, the total \
                 cost basis, the market value, the unrealized gain or loss (the market value \
                 less the cost basis, and as a share of a positive cost basis; not available \
                 where no basis is reported) and the change in the shares held since the last \
                 pull — new where the last pull had none, else increased, decreased or \
                 unchanged, with the shares held then and now.\n\
                 Shares held: 100. Cost basis: $14,000.00. Market value: $19,500.00. Unrealized \
                 gain: $5,500.00 (+39.3% of the cost basis). Change since the last pull: new.\n"
            ),
            "{part1}"
        );
        // On a debut COMPUTED runs to SUPPORTED ACTIONS, there being no PRIOR
        // ACTION.
        assert!(
            part1.contains(
                "\nCOMPUTED\nThe computed reads follow under their labels, up to SUPPORTED \
                 ACTIONS; each is derived from the holding's data by fixed formulas.\n"
            ),
            "{part1}"
        );
        assert!(
            part1.contains(&format!(
                "\nCOMPUTED ACTION\nThe rung a fixed rule gives from the computed reads: {}.\n",
                graded.engine_rung.as_kebab()
            )),
            "{part1}"
        );
        // Part 1 instructs nothing.
        assert!(!part1.contains("Return "), "{part1}");
        assert!(
            part2.contains(
                "\n1. action — one rung for this holding, from these inputs alone: \"sell-all\", \
                 \"trim\", \"hold\", \"add\" or \"add-aggressively\". The rung alone: no share count, \
                 dollar amount or portfolio weight. Decide it from VERDICT and COMPUTED first, \
                 refined by POSITION, SUPPORTED ACTIONS and INVESTOR PROFILE. An aggressive \
                 risk tolerance admits add-aggressively where the other inputs support it. \
                 Where even the bull case under CAPITAL EFFICIENCY misses the hurdle and the \
                 forward read is poor, lean toward realizing some or all of the position.\n"
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
        // The stub authors its twelve-month base at 1.05× the computed base, so
        // the two moves differ.
        let spot = d.financials.current_price.unwrap();
        let leg = |v: f64| format!("{v:.2} ({:+.1}%)", (v / spot - 1.0) * 100.0);
        let engine_12 = graded.price_targets.twelve_month.as_ref().unwrap();
        let model_12 = graded.appendix.expected_price_12m.unwrap();
        assert!(
            user.contains(&format!(
                "- twelve-month: bear {} / base {} / bull {}; analyst {}. Method: ",
                leg(engine_12.bear), leg(engine_12.base), leg(engine_12.bull), leg(model_12)
            )),
            "{user}"
        );
        assert_ne!(leg(engine_12.base), leg(model_12));
        assert!(
            user.contains(&format!(
                "\nVERDICT\nAn analyst's read of the holding's data and research: the conviction, \
                 the expected share price at each horizon (USD, with the move each implies from \
                 the current price) and the thesis document.\nConviction: {}. Expected share \
                 price: three-month {}, twelve-month {}, three-year {}.\nThesis document:\n{}",
                graded.appendix.conviction.unwrap().as_str(),
                leg(graded.appendix.expected_price_3m.unwrap()),
                leg(model_12),
                leg(graded.appendix.expected_price_3y.unwrap()),
                graded.thesis_document
            )),
            "{user}"
        );
        assert_eq!(user.matches("- three-month: ").count(), 1, "{user}");
        assert_eq!(user.matches("- three-year: ").count(), 1, "{user}");
        assert!(user.contains("- three-year: bear ") && user.contains("extrapolation"), "{user}");
        assert!(
            user.contains(&format!(
                "\nGRADE\nA letter from A to F, derived from the computed quality, valuation and \
                 risk scores.\n{}.\n",
                graded.grade.as_str()
            )),
            "{user}"
        );
        assert!(!user.contains("higher is better on every axis"), "{user}");
        assert!(!user.contains("Risk tier"), "{user}");
        let set: Vec<&str> = engine_set.iter().map(Action::as_kebab).collect();
        assert!(
            user.contains(&format!(
                "\nSUPPORTED ACTIONS\nThe rungs a fixed rule over the holding's reads supports, \
                 listed in full: {}. A rung not listed is outside that rule.\n",
                set.join(", ")
            )),
            "{user}"
        );
        // No app concept or whole-book vocabulary; on a debut, no continuity
        // section or firmness clause.
        for absent in [
            "ENGINE SET", "ENGINE ARM", "MODEL ARM", "THE VERDICT", "ACTION BASIS", "IMPLIED ",
            "TARGET PROVENANCE", "engine arm", "model arm", "engine targets",
            "model targets", "its own pick", "full ladder", "neither requires nor forbids",
            "indeterminate", "dead money", "scoreboard", "concentration", "OVERLAP", "- cash:",
            "unconstrained", "PRIOR ACTION", "PRIOR ANALYSIS",
            "CHANGES SINCE", "Move from PRIOR ACTION", "exactly ONE", "Keep the action firm",
            "(computed)", "(analyst)", "Two reads of this holding", "SCORES", "PRICE TARGETS",
        ] {
            assert!(!user.contains(absent), "`{absent}` in the message: {user}");
            assert!(!system.contains(absent), "`{absent}` in the system prompt: {system}");
        }
    }

    #[test]
    fn action_prompt_distinguishes_rule_demotion_and_renders_the_position_tax_invariant() {
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
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
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
        // A rule-demoted prior carries no rationale and anchors no firmness
        // clause.
        assert!(
            exempt.contains("\nPRIOR ACTION\nhold, set by rule after the prior analysis, not chosen in it.\n\nSUPPORTED ACTIONS\n"),
            "{exempt}"
        );
        assert!(!exempt.contains("Rationale:"), "{exempt}");
        assert!(!exempt.contains("chosen in the prior analysis."), "{exempt}");
        assert!(!exempt.contains("Move from PRIOR ACTION"), "{exempt}");
        assert!(!exempt.contains("rule-demoted"), "{exempt}");
        assert!(exempt.contains("under their labels, up to PRIOR ACTION; each"), "{exempt}");
        // The two renders are byte-identical, so the tax posture cannot reach
        // the rung by any route.
        for token in ["tax", "Tax", "P/L"] {
            assert!(!exempt.contains(token), "{token} leaked: {exempt}");
        }
        for once in ["Cost basis:", "Market value:", "Unrealized gain:"] {
            assert_eq!(exempt.matches(once).count(), 1, "{once}: {exempt}");
        }
        assert!(!without_position(&exempt).contains("Cost basis"), "{exempt}");
        d.profile.tax_sensitive = true;
        let taxable = render(&d);
        assert_eq!(exempt, taxable, "the tax posture must not change the packet");
        d.position.cost_basis *= 3.0;
        let repriced = render(&d);
        assert_ne!(exempt, repriced);
        assert_eq!(without_position(&exempt), without_position(&repriced));
        assert!(repriced.contains("Unrealized loss: $22,500.00 (-53.6% of the cost basis)."), "{repriced}");
    }

    #[test]
    fn action_packet_renders_the_hurdle_as_numbers_and_the_continuity_sections() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let VerdictDisposition::Priced(graded) = &prior.disposition else { panic!("priced"); };
        let mut engine = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(engine) => engine,
            _ => panic!("priced"),
        };
        d.prior_verdict = Some(prior.clone());
        for state in [crate::portfolio::HurdleState::Fails, crate::portfolio::HurdleState::Clears,
            crate::portfolio::HurdleState::Indeterminate, crate::portfolio::HurdleState::Unscorable] {
            engine.hurdle.state = state;
            let mut low = graded.clone();
            low.low_confidence_grade = true;
            let prompt = action_user_prompt(&ActionInput {
                dossier: &d, subject: ActionSubject::Priced { graded: &low, engine: &engine, pre_profit: None },
                engine_set: &[Action::Hold], profile: &d.profile,
            });
            // CAPITAL EFFICIENCY as numbers: no state word, no reach sentence.
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
            assert_eq!(prompt.matches("Where even the bull case under CAPITAL EFFICIENCY misses the hurdle").count(), 1, "{prompt}");
            assert!(
                prompt.contains(&format!(
                    "\nGRADE\nA letter from A to F, derived from the computed quality, valuation and risk scores.\n{}; one of those scores is imputed, so the letter is low-confidence.\n",
                    low.grade.as_str()
                )),
                "{prompt}"
            );
            assert!(
                prompt.contains(&format!(
                    "\nPRIOR ACTION\n{}, chosen in the prior analysis.\nRationale: Stub action: the grade-mapped rung inside the engine set.\n",
                    graded.action.as_kebab()
                )),
                "{prompt}"
            );
            assert!(prompt.contains("Decide it from VERDICT and COMPUTED first, refined by POSITION, PRIOR ACTION, SUPPORTED ACTIONS and INVESTOR PROFILE."), "{prompt}");
            assert!(prompt.contains(" Move from PRIOR ACTION only where the inputs have materially changed since the prior analysis.\n"), "{prompt}");
            for absent in ["PRIOR ANALYSIS", "CHANGES SINCE", "Prior engine grade", "continuity baseline", "CompanyInformation", "THESIS (analyst)", "SCENARIOS (analyst)"] {
                assert!(!prompt.contains(absent), "`{absent}`: {prompt}");
            }
            let mut no_rate = engine.clone();
            no_rate.hurdle.hurdle_rate = None;
            let prompt = action_user_prompt(&ActionInput {
                dossier: &d, subject: ActionSubject::Priced { graded: &low, engine: &no_rate, pre_profit: None },
                engine_set: &[Action::Hold], profile: &d.profile,
            });
            assert!(prompt.contains("\nCAPITAL EFFICIENCY\nNo assessment this run.\n"), "{prompt}");
            assert!(!prompt.contains("; hurdle "), "{prompt}");
        }
    }

    #[test]
    fn evidence_leg_sections_render_when_present_and_stay_silent_when_absent() {
        use crate::portfolio::dossier::{
            OptionOverlay, OverlayClass, OverlayDirection, OverlayLeg,
        };
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = |d: &HoldingDossier, narrative: Option<&engine::NarrativeRead>| {
            thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
                dossier: d,
                engine: &engine_output,
                analysis: analysis_record("findings"),
                pre_profit: None,
                tech_pre_flag: None,
                narrative,
                review: None,
            })
        };
        let base = interp(&d, None);
        assert!(!base.contains("SHORT INTEREST"), "{base}");
        assert!(!base.contains("SAME-UNDERLYING OPTION OVERLAY"), "{base}");
        assert!(!base.contains("NARRATIVE VS REALITY"), "{base}");
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
                    "Rule matched: narrative-vs-reality hype: test rule.\n"
                ),
            "{p}"
        );
        assert!(!p.contains("binds the ENGINE arm"), "{p}");
        assert!(p.contains("- What the current price implies, at each scenario's multiple:"), "{p}");
        assert!(!p.contains("IMPLIED EXPECTATIONS"), "{p}");
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
    fn prompt_version_is_stamped_for_the_model_arm_domain_gate() {
        // Each stamp moves with every change to its prompts or persisted
        // shapes, so an interrupted pre-change run cannot resume into the new
        // contract; the pins here move with each bump.
        assert_eq!(PROMPT_VERSION, "portfolio-v78");
        assert_eq!(
            crate::portfolio::store::CHECKPOINT_FORMAT_VERSION,
            "checkpoint-v26"
        );
        assert_eq!(crate::portability::FORMAT_VERSION, 24);
    }

    #[test]
    fn the_narrative_render_names_an_overflowed_ratio_and_never_prints_inf() {
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
    fn action_prompt_names_a_chosen_prior_action_and_the_firmness_clause() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, _) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let prior_action = crate::portfolio::carried_action(&prior).unwrap();
        d.prior_verdict = Some(prior);
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
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
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(
            user.contains(&format!("\nPRIOR ACTION\n{}, chosen in the prior analysis.\n", prior_action.as_kebab())),
            "{user}"
        );
        // The prior rationale renders less the app's caveat.
        let prior_rationale = crate::portfolio::carried_rationale(d.prior_verdict.as_ref().unwrap()).unwrap();
        assert_eq!(investment_sentence(prior_rationale), "Stub action: the grade-mapped rung inside the engine set.");
        assert!(
            user.contains("\nRationale: Stub action: the grade-mapped rung inside the engine set.\n"),
            "{user}"
        );
        assert!(!user.contains("Tax note"), "{user}");
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
        struct RogueActionStub;
        impl HoldingAnalyst for RogueActionStub {
            fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
                StubAnalyst.interpret(input)
            }
            fn interpret_role_risk(
                &self,
                input: &RoleRiskInput,
            ) -> Result<String> {
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
        // The fixture's engine set should not offer add-aggressively, so the
        // rogue choice lands outside it.
        let d = dossier(AssetClass::Stock, strong_financials());
        let (v, audit) =
            analyze_holding(&RogueActionStub, &d, &rates(), "2026-08-03").unwrap();
        let crate::portfolio::VerdictDisposition::Priced(g) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        assert_eq!(g.action, Action::AddAggressively, "persists as authored");
        assert_eq!(g.action_rationale, "rogue: aggressive regardless of the engine set");
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
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
    fn target_provenance_renders_the_anchored_and_carry_branches() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let mut engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };

        engine_output.target_meta.rate_anchored = true;
        engine_output.target_meta.anchor_observations = 40;
        engine_output.target_meta.current_multiple_carry = false;
        let anchored = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record(""),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(
            anchored.contains("percentile of their spread to the 10-year Treasury over the last 40 quarterly observations"),
            "{anchored}"
        );
        // One data clause, never a provenance heading.
        assert!(!anchored.contains("TARGET PROVENANCE"), "{anchored}");
        assert!(!anchored.contains("spread-anchored on "), "{anchored}");
        // The how-to-weigh narrative stays out; the model infers it from the
        // provenance facts.
        assert!(!anchored.contains("Weigh the targets by this provenance"), "{anchored}");
        assert!(!anchored.contains("robust exit evidence"), "{anchored}");
        assert!(!anchored.contains("discount the band's width, not its level"), "{anchored}");
        assert!(!anchored.contains("stays weak exit evidence"), "{anchored}");

        engine_output.target_meta.rate_anchored = false;
        engine_output.target_meta.current_multiple_carry = true;
        engine_output.target_meta.flat_driver = true;
        engine_output.target_meta.dispersion_floor_applied = true;
        let carried = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record(""),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(carried.contains("× the current P/E multiple (no anchor history"), "{carried}");
        // The signal-quality fact survives without the weighing narrative.
        assert!(carried.contains("carry little forward signal"), "{carried}");
        assert!(carried.contains("- Notes: ") && carried.contains("the driver is held flat across scenarios"), "{carried}");
        assert!(!carried.contains("FLAT"), "{carried}");
        assert!(carried.contains("the band was widened to the volatility dispersion floor"), "{carried}");

        engine_output.target_meta.current_multiple_carry = false;
        let fallback = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record(""),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
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
    fn house_view_renders_as_market_analysis_on_both_interpretation_messages() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.house_view.latest_sections = Some("Thesis: risk-off.".into());
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record(""),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        // Named for what it is, never by product name.
        assert!(
            user.contains("\nMARKET ANALYSIS\nA market-level analysis.\nThesis: risk-off.\n"),
            "{user}"
        );
        assert!(user.contains("from FETCHED VALUES, COMPUTED, ANALYSIS and MARKET ANALYSIS."), "{user}");
        for narration in ["MARKET SIGNAL", "HOUSE VIEW", "never by itself a reason to exit", "scope:"] {
            assert!(!user.contains(narration), "`{narration}` leaked: {user}");
        }
        let bare = dossier(AssetClass::Stock, strong_financials());
        let bare_user = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &bare,
            engine: &engine_output,
            analysis: analysis_record(""),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(!bare_user.contains("\nMARKET ANALYSIS\n"), "{bare_user}");

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
            rates: rates_static(),
            prior_split: None,
            dossier: &d,
            readout: &readout,
            analysis: analysis_record("No research findings."),
            review: None,
        });
        assert!(
            role.contains("\nMARKET ANALYSIS\nA market-level analysis.\nThesis: risk-off.\n"),
            "{role}"
        );
        assert!(role.contains("from CLASS, RISK PROFILE, FETCHED VALUES, COMPUTED, ANALYSIS and MARKET ANALYSIS."), "{role}");
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
            rates: rates_static(),
            prior_split: None,
                dossier: &d,
                readout: &readout,
                analysis: analysis_record("No research findings."),
                review: None,
            })
        };

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
        // A non-CEF never renders even with a computed premium (an open-end
        // ETF's transient spread).
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
            rates: rates_static(),
            prior_split: None,
                dossier: &d,
                readout: r,
                analysis: analysis_record("No research findings."),
                review: None,
            })
        };
        let discount = prompt(&readout(true, Some(-0.072)));
        assert!(discount.contains("PRICE VS NAV: -7.2% (discount)"), "{discount}");
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
        let par = prompt(&readout(true, Some(0.0)));
        assert!(par.contains("PRICE VS NAV: 0.0% (at par)"), "{par}");
        let tiny = prompt(&readout(true, Some(-0.0004)));
        assert!(tiny.contains("PRICE VS NAV: 0.0% (at par)"), "{tiny}");
        // The exact negative half rounds away from zero (f64::round); the Vue
        // helper mirrors this deliberately, since bare Math.round would read
        // -0.05% as at par.
        let half = prompt(&readout(true, Some(-0.0005)));
        assert!(half.contains("PRICE VS NAV: -0.1% (discount)"), "{half}");
        let gap = prompt(&readout(true, None));
        assert!(!gap.contains("PRICE VS NAV:"), "{gap}");
        assert!(gap.contains("price-vs-NAV unavailable"), "the named gap: {gap}");
        let open_end = prompt(&readout(false, Some(0.002)));
        assert!(!open_end.contains("PRICE VS NAV:"), "{open_end}");

        let mut rr = crate::portfolio::RoleRiskVerdict {
            class_label: "closed-end fund".into(),
            thesis_document: "Role: an income sleeve.".into(),
            exposure_tilt: vec![],
            expense_drag: None,
            observable_risk: None,
            structural_flag: false,
            is_cef: true,
            nav_premium: Some(-0.072),
            evidence_gaps: vec![],
            action: crate::portfolio::Action::Hold,
            action_rationale: String::new(),
        };
        let action = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::RoleRisk { verdict: &rr },
            engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
            profile: &d.profile,
        });
        assert!(action.contains("\nPRICE VS NAV: -7.2% (discount)"), "{action}");
        assert!(action.contains("\nCLASS\nclosed-end fund\n"), "{action}");
        assert!(
            action.contains("\nVERDICT\nAn analyst's read of the holding's data and research: the thesis document.\nThesis document:\nRole: an income sleeve.\n"),
            "{action}"
        );
        assert_eq!(action.matches("\nPOSITION\n").count(), 1, "{action}");
        assert!(!action.contains("\nCOMPUTED\n"), "{action}");
        assert!(
            action.contains("\nSUPPORTED ACTIONS\nThe rungs a fixed rule over the holding's reads supports, listed in full: sell-all, trim, hold. A rung not listed is outside that rule.\n"),
            "{action}"
        );
        assert!(
            action.contains(
                "Decide it from CLASS, VERDICT, RISK PROFILE and PRICE VS NAV first, refined by \
                 POSITION, SUPPORTED ACTIONS and INVESTOR PROFILE. An aggressive risk tolerance \
                 admits add-aggressively where the other inputs support it. An add-side rung needs \
                 support from the vehicle's own attributes, stated in the rationale.\n"
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
            subject: ActionSubject::RoleRisk { verdict: &rr },
            engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
            profile: &d.profile,
        });
        assert!(!action_gap.contains("PRICE VS NAV:"), "{action_gap}");
    }

    #[test]
    fn the_priced_fund_prompt_renders_the_closed_end_arm() {
        // Structurally unreachable for a real CEF (pricing needs weightings the
        // surface never serves one), but shipped code.
        let mut d = fund_dossier(us_equity_fund());
        let mut engine_output = match engine::analyze(&strong_financials(), &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let prompt = |d: &HoldingDossier, e: &EngineOutput| {
            thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
                dossier: d,
                engine: e,
                analysis: analysis_record(""),
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
                review: None,
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
        assert_eq!(fmt_iv_skew(Some(0.03)), "+0.030");
        assert_eq!(fmt_iv_skew(Some(-0.02)), "-0.020");
        assert_eq!(fmt_iv_skew(Some(0.0)), "0.000");
        // A skew that rounds away carries no sign.
        assert_eq!(fmt_iv_skew(Some(0.0004)), "0.000");
        assert_eq!(fmt_iv_skew(Some(-0.0004)), "0.000");
        assert_eq!(fmt_iv_skew(Some(0.0005)), "+0.001");
        assert_eq!(fmt_iv_skew(Some(-0.0005)), "-0.001");
        assert_eq!(fmt_iv_skew(None), "(gap)");
    }

    #[test]
    fn iv_skew_convention_rides_the_options_line_on_value_negative_and_gap() {
        let engine_output = match engine::analyze(&strong_financials(), &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let prompt = |d: &HoldingDossier| {
            thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
                dossier: d,
                engine: &engine_output,
                analysis: analysis_record(""),
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
                review: None,
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
        // No trading gloss or grade-input disclaimer.
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
        assert_eq!(fmt_expense_ratio(Some(0.0003)), "0.0003 (0.03%/yr)");
        assert_eq!(fmt_expense_ratio(Some(0.0075)), "0.0075 (0.75%/yr)");
        assert_eq!(fmt_expense_ratio(Some(0.0125)), "0.0125 (1.25%/yr)");
        // The adapter's percent / 100 is not exactly 0.0003 in f64: fixed
        // precision, never shortest-round-trip display.
        assert_eq!(fmt_expense_ratio(Some(0.03 / 100.0)), "0.0003 (0.03%/yr)");
        // A fee-waived fund is genuinely zero.
        assert_eq!(fmt_expense_ratio(Some(0.0)), "0.0000 (0.00%/yr)");
        // A nonzero ratio below half a basis point extends its precision.
        assert_eq!(fmt_expense_ratio(Some(0.00004)), "0.00004 (0.004%/yr)");
        assert_eq!(fmt_expense_ratio(None), "(gap)");
    }

    #[test]
    fn the_priced_fund_prompt_renders_guards_us_share_and_twelve_month_methodology() {
        // US share sums every US alias and caps, as the ≥ 70% guard reads it.
        let prompt_for = |weights: Vec<(String, f64)>| {
            let mut fund = us_equity_fund();
            fund.country_weights = weights;
            let d = fund_dossier(fund);
            let engine_output = match engine::analyze(&strong_financials(), &rates(), Default::default()) {
                EngineVerdict::Analyzed(o) => o,
                other => panic!("{other:?}"),
            };
            thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
                dossier: &d,
                engine: &engine_output,
                analysis: analysis_record(""),
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
                review: None,
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
        assert!(us.contains("\nPRICE BANDS (USD)\n- three-month: bear "), "{us}");
        let twelve = us.find("- twelve-month: bear ").unwrap_or_else(|| panic!("{us}"));
        let line = us[twelve..].lines().next().unwrap();
        assert!(line.contains(". Method: ") && line.contains(" × ") && line.contains("percentile"), "{line}");
        let three_month = us.find("- three-month: bear ").unwrap_or_else(|| panic!("{us}"));
        let line = us[three_month..].lines().next().unwrap();
        assert!(line.contains(" / base ") && line.contains(" / bull "), "{line}");
        assert!(line.contains(". Method: ") && line.contains("prorated to three months"), "{line}");
        let three_year = us.find("- three-year: bear ").unwrap_or_else(|| panic!("{us}"));
        let line = us[three_year..].lines().next().unwrap();
        assert!(line.contains(". Method: ") && line.contains("extrapolation"), "{line}");
        assert!(!us.contains(engine::SCENARIO_TARGET_PARAMETER_VERSION), "{us}");
        for narration in ["ENGINE SCENARIO TARGETS", "baseline arm", "ENGINE ONE-MONTH TARGETS", "methodology:", "v1 mechanics", "degenerate", "clamp", "inverse map", "P75", "DGS10", "PR_base"] {
            assert!(!us.contains(narration), "`{narration}` leaked: {us}");
        }
    }

    #[test]
    fn expense_ratio_renders_both_readings_on_every_fund_prompt() {
        // The fixture fund carries 0.0003.
        let d = fund_dossier(us_equity_fund());
        let readout = RoleRiskReadout {
            class_label: "bond fund".into(),
            expense_ratio: Some(0.0003),
            ..Default::default()
        };
        let role = role_risk_user_prompt(&RoleRiskInput {
            rates: rates_static(),
            prior_split: None,
            dossier: &d,
            readout: &readout,
            analysis: analysis_record("No research findings."),
            review: None,
        });
        assert!(
            role.contains("\n- asset class: Equity\n- expense ratio: 0.0003 (0.03%/yr)\n"),
            "{role}"
        );
        assert_eq!(role.matches("0.0003 (0.03%/yr)").count(), 1, "renders once: {role}");
        assert!(!role.contains("fund expense ratio:"), "{role}");
        assert!(!role.contains("EXPENSE RATIO ("), "{role}");

        let engine_output = match engine::analyze(&strong_financials(), &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record(""),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(interp.contains("\nFUND\nUS share of holdings: 99%.\n"), "{interp}");
        assert!(
            interp.contains("\n- asset class: Equity\n- expense ratio: 0.0003 (0.03%/yr)\n"),
            "{interp}"
        );
        assert_eq!(interp.matches("0.0003 (0.03%/yr)").count(), 1, "renders once: {interp}");
        assert!(!interp.contains("FUND CONTEXT") && !interp.contains("fund expense ratio:"), "{interp}");

        let rr = crate::portfolio::RoleRiskVerdict {
            class_label: "bond fund".into(),
            thesis_document: "Role: an income sleeve.".into(),
            exposure_tilt: vec![],
            expense_drag: Some(0.0003),
            observable_risk: None,
            structural_flag: false,
            is_cef: false,
            nav_premium: None,
            evidence_gaps: vec![],
            action: crate::portfolio::Action::Hold,
            action_rationale: String::new(),
        };
        let action = action_user_prompt(&ActionInput {
            dossier: &d,
            subject: ActionSubject::RoleRisk { verdict: &rr },
            engine_set: &crate::portfolio::ROLE_RISK_ACTIONS,
            profile: &d.profile,
        });
        assert!(
            action.contains(
                "\nRISK PROFILE\nExpense drag: 0.0003 (0.03%/yr) of assets per year. \
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
            rates: rates_static(),
            prior_split: None,
            dossier: &d,
            readout: &readout,
            analysis: analysis_record("No research findings."),
            review: None,
        });
        const SECTION: &str = "\nUNDERLYING POSITIONING (CFTC weekly, as of 2026-08-11)\nGold — \
                               speculator net +200000 contracts (50.0% of OI long), w/w +4000\n";
        assert!(role.contains(SECTION), "{role}");
        // No layer or score-input narration.
        for narration in ["snapshot", "never a score input", "layer (c)"] {
            assert!(!role.contains(narration), "`{narration}` leaked: {role}");
        }
        let engine_output = match engine::analyze(&strong_financials(), &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record(""),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(interp.contains(SECTION), "{interp}");
        let fund_section = interp.find("\nFUND\n").expect("fund section");
        let positioning = interp.find(SECTION).expect("positioning line");
        let metrics = interp.find("\nMETRICS\n").expect("metrics section");
        assert!(fund_section < positioning && positioning < metrics, "{interp}");

        let bare = fund_dossier(us_equity_fund());
        let role = role_risk_user_prompt(&RoleRiskInput {
            rates: rates_static(),
            prior_split: None,
            dossier: &bare,
            readout: &readout,
            analysis: analysis_record("No research findings."),
            review: None,
        });
        assert!(!role.contains("UNDERLYING POSITIONING"), "{role}");
    }

    #[test]
    fn stage_requests_carry_the_per_stage_mode_options_and_residency() {
        let d = dossier(AssetClass::Stock, strong_financials());

        let distill = distill_request(
            "fast-model",
            NUM_CTX_DISTILL,
            NUM_PREDICT_DISTILL,
            &distill::DistillPrompt { system: "s".into(), user: "prompt".into() },
        );
        assert_eq!(distill.think, Some(false));
        assert_eq!(distill.keep_alive, Some(-1));
        let opts = distill.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_DISTILL);
        assert_eq!(opts["num_predict"], NUM_PREDICT_DISTILL, "output reservation");
        assert_eq!(opts["temperature"], 0.7, "non-thinking-general row");
        assert!(distill.format_schema.is_none(), "a distillation returns prose under no grammar (docs §Step 6d)");

        // Tools and a grammar never ride one request.
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
            None,
        );
        assert_eq!(synth.think, Some(true));
        assert_eq!(synth.keep_alive, Some(-1));
        assert!(synth.tools.is_none(), "synthesis carries no tools");
        assert!(synth.format_schema.is_none(), "the write-up is prose under no grammar");
        assert_eq!(
            synth.options.as_ref().unwrap()["num_ctx"],
            NUM_CTX_INTERPRET,
            "one num_ctx per model"
        );

        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interpret = thesis_request(
            "reasoner-model",
            &ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
                dossier: &d,
                engine: &engine_output,
                analysis: analysis_record("distilled findings"),
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
                review: None,
            },
        );
        assert_eq!(interpret.think, Some(true));
        assert_eq!(interpret.keep_alive, Some(-1));
        let opts = interpret.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET);
        assert_eq!(opts["num_predict"], NUM_PREDICT_THINKING, "output reservation");
        assert_eq!(opts["temperature"], 1.0, "thinking-general row");
        assert!(interpret.format_schema.is_none(), "the document is free prose");
        let appendix = appendix_request("reasoner-model", &ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record("distilled findings"),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        }, "The document.");
        assert_eq!(appendix.think, Some(false));
        assert_eq!(appendix.keep_alive, Some(-1));
        assert_eq!(appendix.format_schema.as_ref(), Some(&crate::portfolio::appendix_schema()));
        let opts = appendix.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET, "one num_ctx per model");
        assert_eq!(opts["num_predict"], NUM_PREDICT_APPENDIX, "output reservation");
        assert_eq!(opts["temperature"], 0.7, "non-thinking-general row");

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
            rates: rates_static(),
            prior_split: None,
                dossier: &d,
                readout: &readout,
                analysis: analysis_record("No research findings."),
                review: None,
            },
        );
        assert_eq!(role_risk.think, Some(true));
        assert_eq!(role_risk.keep_alive, Some(-1));
        let opts = role_risk.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET);
        assert_eq!(opts["num_predict"], NUM_PREDICT_THINKING, "output reservation");
        assert!(role_risk.format_schema.is_none(), "the document is free prose");
    }

    #[test]
    fn distill_expands_only_an_exact_normal_reservation_stop() {
        let normal = distill_request(
            "fast-tier",
            NUM_CTX_DISTILL,
            NUM_PREDICT_DISTILL,
            &distill::DistillPrompt { system: "s".into(), user: "prompt".into() },
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

    #[test]
    fn distill_ceiling_sits_in_the_ruled_band_and_fits_beside_the_input_budget() {
        // The prompt budget the issue guard admits plus the reservation fits
        // the window, so a ceiling-bound stop is a reservation hit, never
        // context exhaustion in disguise.
        assert!((12_288..=16_384).contains(&NUM_PREDICT_DISTILL));
        let budget_tokens = |num_ctx: u32| {
            (distill::input_budget_chars(num_ctx) as f64 / distill::CHARS_PER_TOKEN) as u32
        };
        for num_ctx in [NUM_CTX_DISTILL, NUM_CTX_INTERPRET] {
            assert!(budget_tokens(num_ctx) + NUM_PREDICT_DISTILL <= num_ctx, "{num_ctx}");
        }
        assert!(budget_tokens(NUM_CTX_INTERPRET) + NUM_PREDICT_DISTILL_RETRY <= NUM_CTX_INTERPRET);
    }

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

        // A stop well under the reservation is the context filling first, a
        // different lever.
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

        // Without `eval_count` the stop is unattributable; neither lever is
        // named on a guess.
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
        assert_eq!(
            distill_num_ctx("qwen3.5:122b", "qwen3.5:122b"),
            NUM_CTX_INTERPRET
        );
        assert_eq!(
            distill_num_ctx("qwen3.5:35b", "qwen3.5:122b"),
            NUM_CTX_DISTILL
        );
        // A blank fast tier resolves to the same-model path at construction.
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
    fn the_analysis_request_is_a_thinking_prose_call_on_the_reasoner() {
        let brief = HoldingBrief {
            header: "HOLDING\nAAPL (Apple).\nPrice: $195.00 per share.\nDate: 2026-08-03.\n".into(),
            fetched_values: "\nFETCHED VALUES\nQuote: 195.00.\n".into(),
            leads: vec![],
            prior_documents: String::new(),
        };
        let write_ups = [distill::WriteUp {
            key: "competitive-position".into(),
            title: "Competitive / business position".into(),
            text: "Share held.".into(),
        }];
        let input = distill::AnalysisInput {
            symbol: "AAPL",
            brief: &brief,
            prior_analysis: "",
            write_ups: distill::WriteUps::AsWritten(&write_ups),
        };
        assert_eq!(input.stage(), "analysis AAPL");
        let req = analysis_request("qwen3.5:122b", &input);
        assert_eq!(req.model_id, "qwen3.5:122b");
        assert_eq!(req.think, Some(true));
        assert!(req.tools.is_none() && req.format_schema.is_none());
        assert_eq!(req.keep_alive, Some(KEEP_ALIVE_RESIDENT));
        let opts = req.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], serde_json::json!(NUM_CTX_INTERPRET));
        assert_eq!(opts["num_predict"], serde_json::json!(NUM_PREDICT_THINKING));
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert!(
            req.messages[0].content.starts_with("You are an investment analyst consolidating one holding's research"),
            "{}",
            req.messages[0].content
        );
        assert!(
            req.messages[1].content.starts_with("======== PART 1: INPUTS ========\nHOLDING\nAAPL (Apple).\n"),
            "{}",
            req.messages[1].content
        );
        assert!(req.messages[1].content.contains("\nWRITE-UPS\n"), "{}", req.messages[1].content);
        assert_eq!(StubAnalyst.analyze(&input).unwrap(), "Competitive / business position\nShare held.");

        let d_input = distill::DistillInput {
            header: &brief.header,
            subject: distill::DistillSubject::Merged(&write_ups),
            stage: "distill AAPL".into(),
        };
        let d_req = distill_request(
            "qwen3.5:35b",
            NUM_CTX_DISTILL,
            NUM_PREDICT_DISTILL,
            &distill::distillation_prompt(&d_input),
        );
        assert_eq!(d_req.think, Some(false));
        assert!(d_req.tools.is_none() && d_req.format_schema.is_none());
        assert!(
            d_req.messages[1].content.contains(
                "\nWRITE-UPS\nThis run's write-ups on the holding, each under its topic, the \
                 contrary-evidence pass last.\n"
            ),
            "{}",
            d_req.messages[1].content
        );
        assert_eq!(StubAnalyst.distill(&d_input).unwrap(), "Competitive / business position\nShare");
    }

    #[test]
    fn distill_calls_are_sized_at_issue_and_route_up_before_they_refuse() {
        let fast_budget = distill::input_budget_chars(NUM_CTX_DISTILL);
        let wide_budget = distill::input_budget_chars(NUM_CTX_INTERPRET);
        assert!(fast_budget < wide_budget);
        let route = |chars: usize, fast: &'static str, reasoner: &'static str| {
            distill_route("distill X reduce", chars, fast, reasoner)
        };
        assert_eq!(
            route(fast_budget, "qwen3.5:35b", "qwen3.5:122b").unwrap(),
            ("qwen3.5:35b", NUM_CTX_DISTILL)
        );
        assert_eq!(
            route(fast_budget + 1, "qwen3.5:35b", "qwen3.5:122b").unwrap(),
            ("qwen3.5:122b", NUM_CTX_INTERPRET)
        );
        assert_eq!(
            route(wide_budget, "qwen3.5:35b", "qwen3.5:122b").unwrap(),
            ("qwen3.5:122b", NUM_CTX_INTERPRET)
        );
        let err = route(wide_budget + 1, "qwen3.5:35b", "qwen3.5:122b").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("distill X reduce"), "{msg}");
        assert!(msg.contains(&format!("{} chars", wide_budget + 1)), "{msg}");
        assert!(msg.contains(&format!("{wide_budget} chars")), "{msg}");
        assert!(msg.contains("refused before issue"), "{msg}");
        // Unclassified, so the retry gate never re-issues a deterministic
        // outcome.
        assert_eq!(crate::local_model::retry_class(&err), None);
        // On the default roster the two rungs are one budget.
        assert_eq!(
            route(wide_budget, "qwen3.5:122b", "qwen3.5:122b").unwrap(),
            ("qwen3.5:122b", NUM_CTX_INTERPRET)
        );
        assert!(route(wide_budget + 1, "qwen3.5:122b", "qwen3.5:122b").is_err());
    }

    #[test]
    fn an_over_budget_distillation_prompt_never_reaches_the_daemon() {
        // A listener stands in for the daemon and must accept nothing.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let analyst = LocalAnalyst::new(
            LocalModelClient::new(endpoint).unwrap(),
            "qwen3.5:122b".into(),
            "qwen3.5:35b".into(),
        );
        let over = distill::input_budget_chars(NUM_CTX_INTERPRET) + 1;
        let prompt = distill::DistillPrompt { system: "s".into(), user: "x".repeat(over) };
        let err = distill_prose_call(&analyst, "distill TEST", &prompt, &std::cell::Cell::new(false))
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("refused before issue"), "{msg}");
        assert!(msg.contains("distill TEST"), "{msg}");
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "the daemon stand-in must have accepted nothing"
        );
    }

    fn stand_in_distill_input(write_ups: &[distill::WriteUp]) -> distill::DistillInput<'_> {
        distill::DistillInput {
            header: "HOLDING\nAAPL (Apple).\nPrice: $195.00 per share.\nDate: 2026-08-03.\n",
            subject: distill::DistillSubject::Merged(write_ups),
            stage: "distill AAPL".into(),
        }
    }

    fn stand_in_analyst(base_url: &str) -> LocalAnalyst {
        LocalAnalyst::new(
            LocalModelClient::new(base_url).unwrap(),
            "qwen3.5:122b".into(),
            "qwen3.5:122b".into(),
        )
        .without_retry_delay()
    }

    #[test]
    fn a_transient_distillation_failure_re_issues_once_and_a_second_fails_hard() {
        use crate::test_http::{Canned, MockHttp};
        let write_ups = [distill::WriteUp { key: "k".into(), title: "T".into(), text: "t".into() }];
        let input = stand_in_distill_input(&write_ups);
        let recovered = MockHttp::serve(vec![
            Canned::Reply { status: 500, headers: vec![], body: "boom" },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: r#"{"message":{"content":"shortened"},"eval_count":3,"done_reason":"stop"}"#,
            },
        ]);
        let analyst = stand_in_analyst(&recovered.base_url);
        assert_eq!(analyst.distill(&input).unwrap(), "shortened");
        assert_eq!(recovered.attempts(), 2, "one re-issue");
        let events = analyst.take_retry_events();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].stage, "distill AAPL");
        assert_eq!(
            analyst.take_model_calls(),
            Some(vec!["qwen3.5:122b".to_string(), "qwen3.5:122b".to_string()]),
            "both issued calls are recorded"
        );

        let failing = MockHttp::serve(vec![
            Canned::Reply { status: 500, headers: vec![], body: "boom" },
            Canned::Reply { status: 500, headers: vec![], body: "boom again" },
        ]);
        let analyst = stand_in_analyst(&failing.base_url);
        let err = analyst.distill(&input).unwrap_err();
        assert_eq!(failing.attempts(), 2, "never a third request");
        let msg = format!("{err:#}");
        assert!(msg.contains("failed again after one retry"), "{msg}");
        assert_eq!(analyst.take_retry_events().len(), 1);
    }

    #[test]
    fn an_expanded_distillation_attempt_is_final_for_the_stage() {
        use crate::test_http::{Canned, MockHttp};
        let write_ups = [distill::WriteUp { key: "k".into(), title: "T".into(), text: "t".into() }];
        let input = stand_in_distill_input(&write_ups);
        let length_stop: &'static str = Box::leak(
            format!(
                r#"{{"message":{{"content":"cut"}},"eval_count":{NUM_PREDICT_DISTILL},"done_reason":"length"}}"#
            )
            .into_boxed_str(),
        );
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 200, headers: vec![], body: length_stop },
            Canned::Reply { status: 500, headers: vec![], body: "boom" },
        ]);
        let analyst = stand_in_analyst(&server.base_url);
        let err = analyst.distill(&input).unwrap_err();
        assert_eq!(
            server.attempts(),
            2,
            "the normal call and its expanded re-attempt, nothing after: {err:#}"
        );
        assert!(
            analyst.take_retry_events().is_empty(),
            "no transport retry fires past the expanded attempt"
        );
        assert_eq!(
            analyst.take_model_calls(),
            Some(vec!["qwen3.5:122b".to_string(), "qwen3.5:122b".to_string()])
        );
    }

    #[test]
    fn blank_fast_tier_falls_back_to_the_reasoner() {
        let client = LocalModelClient::new("http://127.0.0.1:1").unwrap();
        let analyst = LocalAnalyst::new(client, "qwen3.5:122b".into(), "  ".into());
        assert_eq!(analyst.fast_id(), "qwen3.5:122b");
        assert_eq!(analyst.reasoner_id(), "qwen3.5:122b");

        let client = LocalModelClient::new("http://127.0.0.1:1").unwrap();
        let analyst = LocalAnalyst::new(client, "r".into(), "f".into());
        assert_eq!(analyst.reasoner_id(), "r");
        assert_eq!(analyst.fast_id(), "f");
    }

    // ---- The self-review (Step 6e) ---------------------------------------------

    /// Records every review message a pass issues and the thesis-document
    /// messages that follow, replying with a fixed review.
    #[derive(Default)]
    struct ReviewCapture {
        reviews: std::cell::RefCell<Vec<String>>,
        thesis: std::cell::RefCell<Option<String>>,
        role_risk: std::cell::RefCell<Option<String>>,
    }
    impl HoldingAnalyst for ReviewCapture {
        fn review(&self, input: &review::ReviewInput) -> Result<String> {
            self.reviews.borrow_mut().push(review::review_prompt(input).user);
            Ok(format!("The review of {}.", input.dossier.position.symbol))
        }
        fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
            *self.thesis.borrow_mut() = Some(thesis_user_prompt(input));
            StubAnalyst.interpret(input)
        }
        fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String> {
            *self.role_risk.borrow_mut() = Some(role_risk_user_prompt(input));
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

    #[test]
    fn a_continuity_pass_reviews_before_the_thesis_and_records_the_review_and_its_mark() {
        let d1 = dossier(AssetClass::Stock, strong_financials());
        let cap = ReviewCapture::default();
        let (v1, a1) = analyze_holding(&cap, &d1, &rates(), "2026-08-03").unwrap();
        assert!(cap.reviews.borrow().is_empty());
        let debut = cap.thesis.borrow().clone().unwrap();
        assert!(!debut.contains("\nREVIEW\n"), "{debut}");
        assert_eq!((a1.review.as_deref(), a1.accuracy_read_through), (None, None));

        let mut d2 = dossier(AssetClass::Stock, strong_financials());
        d2.prior_verdict = Some(v1);
        d2.prior_vintage = Some("2026-08-03T20:00:00Z".into());
        d2.prior_spot = a1.quick_basis.as_ref().map(|b| b.spot);
        d2.prior_authoring_close = a1.authoring_close.clone();
        d2.prior_accuracy_read_through = Some(3);
        d2.accuracy = Some(crate::portfolio::outcome::SubjectAccuracy {
            high_water: Some(9),
            ..Default::default()
        });
        let cap = ReviewCapture::default();
        let (_, a2) = analyze_holding(&cap, &d2, &rates(), "2026-08-04").unwrap();
        assert_eq!(cap.reviews.borrow().len(), 1);
        assert!(cap.reviews.borrow()[0].contains("\nREALIZED\n"));
        let thesis = cap.thesis.borrow().clone().unwrap();
        let at = |h: &str| thesis.find(h).unwrap_or_else(|| panic!("{h}:\n{thesis}"));
        assert!(at("\nANALYSIS") < at("\nREVIEW\nThe review of AAPL.\n"));
        assert!(at("\nREVIEW\n") < at("\nPRIOR THESIS"));
        assert!(
            thesis.contains("and how the prior read held up, drawing on REVIEW and PRIOR THESIS"),
            "{thesis}"
        );
        assert_eq!(a2.review.as_deref(), Some("The review of AAPL."));
        assert_eq!(a2.accuracy_read_through, Some(9));

        // An unreadable store keeps the prior mark.
        d2.accuracy = None;
        let (_, a3) = analyze_holding(&ReviewCapture::default(), &d2, &rates(), "2026-08-04").unwrap();
        assert_eq!(a3.accuracy_read_through, Some(3));

        d2.listing = Some(crate::portfolio::listing::ListingResolution::Conflict {
            fmp_name: "Zenith Mining Corp".into(),
        });
        let cap = ReviewCapture::default();
        let (v4, a4) = analyze_holding(&cap, &d2, &rates(), "2026-08-04").unwrap();
        assert!(cap.reviews.borrow().is_empty());
        assert!(matches!(v4.disposition, VerdictDisposition::InsufficientEvidence { .. }));
        assert_eq!((a4.review.as_deref(), a4.accuracy_read_through), (None, Some(3)));

        let mut d5 = dossier(AssetClass::Stock, strong_financials());
        d5.prior_verdict = Some(v4);
        d5.prior_vintage = Some("2026-08-03T20:00:00Z".into());
        d5.prior_authoring_close = a4.authoring_close.clone();
        d5.prior_accuracy_read_through = a4.accuracy_read_through;
        let cap = ReviewCapture::default();
        let (_, a5) = analyze_holding(&cap, &d5, &rates(), "2026-08-05").unwrap();
        assert_eq!(cap.reviews.borrow().len(), 1);
        assert!(cap.reviews.borrow()[0].contains("A later analysis made no call"));
        assert_eq!(a5.review.as_deref(), Some("The review of AAPL."));
    }

    #[test]
    fn a_role_risk_continuity_pass_reviews_and_records_no_mark() {
        let d1 = fund_dossier(bond_fund());
        let (v1, a1) = analyze_holding(&StubAnalyst, &d1, &rates(), "2026-08-03").unwrap();
        assert!(matches!(v1.disposition, VerdictDisposition::RoleRiskOnly(_)));
        assert_eq!(a1.review, None);
        let mut d2 = fund_dossier(bond_fund());
        d2.prior_verdict = Some(v1);
        d2.prior_vintage = Some("2026-08-03T20:00:00Z".into());
        d2.prior_authoring_close = a1.authoring_close.clone();
        d2.prior_fund_exposure = a1.fund_exposure.clone();
        d2.accuracy = Some(crate::portfolio::outcome::SubjectAccuracy {
            high_water: Some(9),
            ..Default::default()
        });
        let cap = ReviewCapture::default();
        let (_, a2) = analyze_holding(&cap, &d2, &rates(), "2026-08-04").unwrap();
        let reviews = cap.reviews.borrow();
        assert_eq!(reviews.len(), 1);
        assert!(reviews[0].contains("- NAV per share: "), "{}", reviews[0]);
        assert!(!reviews[0].contains("ACCURACY SCORES"), "{}", reviews[0]);
        let message = cap.role_risk.borrow().clone().unwrap();
        assert!(message.find("\nREVIEW\n").unwrap() < message.find("\nPRIOR THESIS").unwrap());
        assert!(message.contains("drawing on REVIEW and PRIOR THESIS"), "{message}");
        assert_eq!(a2.review.as_deref(), Some("The review of BND."));
        // A role read states no price, so its review reads no accuracy record.
        assert_eq!(a2.accuracy_read_through, None);
        assert_eq!(a2.fund_exposure.as_ref().map(|b| b.nav), Some(bond_fund().nav));
    }

    #[test]
    fn the_review_request_is_a_thinking_prose_call_at_the_interpret_context() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let now = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let analysis = crate::portfolio::AnalysisRecord {
            text: "The analysis.".into(),
            written: d.analysis_date.clone(),
            anchor: None,
        };
        let input = review::ReviewInput {
            dossier: &d,
            rates: &rates(),
            analysis: &analysis,
            prior_split: None,
            price_bridge: Some(1.0),
            subject: review::ReviewSubject::Priced { engine: &now },
        };
        let req = review_request("reasoner", &input);
        assert_eq!(req.think, Some(true));
        assert!(req.format_schema.is_none());
        assert_eq!(req.keep_alive, Some(KEEP_ALIVE_RESIDENT));
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert!(req.messages[1].content.contains("The review runs 400 to 900 words."));
        assert_eq!(input.stage(), "review AAPL");
    }

    // ---- The engine's realized data + prompt rendering ----------------------------

    #[test]
    fn realized_engine_data_resolves_every_stored_value_at_exact_inequality() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        // The prior session's close goes in before either pass computes, so
        // both read one surface.
        d.financials.daily_closes.push(DatedValue { date: "2026-07-29".into(), value: 180.0 });
        let (prior, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-07-29").unwrap();
        let VerdictDisposition::Priced(pg) = &prior.disposition else {
            panic!("expected a priced prior");
        };
        d.prior_metrics = Some(audit.metrics.clone());
        d.prior_vintage = Some("2026-07-29T12:00:00Z".into());
        let current = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let read = |target_stamp: Option<&str>, bridge: Option<f64>, current: &EngineOutput| {
            engine::realized_engine_data(
                &engine::PriorEngineRead {
                    verdict: pg,
                    metrics: d.prior_metrics.as_ref(),
                    grade_parameter_version: Some(engine::GRADE_PARAMETER_VERSION),
                    target_parameter_version: target_stamp,
                    branch: engine::GradeBranch::Stock,
                },
                current,
                d.financials.current_price,
                d.financials.current_price.map(|s| s / 180.0 - 1.0),
                bridge,
            )
        };
        let same = read(Some(engine::SCENARIO_TARGET_PARAMETER_VERSION), Some(1.0), &current);
        assert!(same.sub_scores.iter().all(|(_, p)| !p.moved()));
        assert!(!same.grade.moved() && !same.tier.moved() && !same.hurdle.moved());
        assert!(same.metrics.iter().all(|(_, p)| !p.moved()));
        assert_eq!(same.metrics.len(), 12);
        assert!(same.bands.iter().all(|(_, p)| p.then.is_some() && p.now.is_some() && !p.moved()));
        assert_eq!(same.grade_boundary, None);
        assert_eq!(same.target_boundary, None);
        let spot = d.financials.current_price.unwrap();
        assert!((same.move_since_prior.unwrap() - (spot / 180.0 - 1.0)).abs() < 1e-12);
        assert_eq!(same.price_now, Some(spot));

        // A sub-precision move is a move.
        let mut nudged = (*current).clone();
        nudged.sub_scores.quality += 1e-9;
        let moved = read(Some(engine::SCENARIO_TARGET_PARAMETER_VERSION), Some(1.0), &nudged);
        let (name, pair) = moved.sub_scores.iter().find(|(_, p)| p.moved()).unwrap();
        assert_eq!(*name, "quality");
        assert_ne!(pair.then, pair.now);
        assert_eq!(moved.sub_scores.iter().filter(|(_, p)| p.moved()).count(), 1);

        // No certified bridge withholds the prior band side; an older target
        // stamp names the horizons.
        let unbridged = read(Some("targets-v6"), None, &current);
        assert!(unbridged.bands.iter().all(|(_, p)| p.then.is_none() && p.now.is_some()));
        assert_eq!(
            unbridged.target_boundary,
            Some(engine::TargetHorizons::of(true, false, true))
        );
    }

    #[test]
    fn three_year_method_names_its_floor_widening_and_its_growth() {
        let mut meta = engine::TargetMeta {
            driver_rung: "consensus forward EPS".into(),
            ..Default::default()
        };
        let flat = three_year_method(&meta);
        assert!(flat.contains("held at flat growth"), "{flat}");
        assert!(!flat.contains("dispersion floor"), "{flat}");
        meta.three_year_floor_applied = true;
        assert!(
            three_year_method(&meta)
                .ends_with("; the band was widened to the volatility dispersion floor"),
            "{}",
            three_year_method(&meta)
        );
        meta.three_year_growth = Some(0.1);
        assert!(three_year_method(&meta).contains("+10.0% a year"));
        let fund = engine::TargetMeta {
            driver_rung: "fund exposure composite".into(),
            ..Default::default()
        };
        assert!(three_year_method(&fund).contains("held unchanged"));
    }

    #[test]
    fn the_band_relation_stamp_reads_spot_against_the_twelve_month_band() {
        let targets = PriceTarget {
            base: 210.0,
            bear: 180.0,
            bull: 240.0,
            methodology: "m".into(),
        };
        use crate::portfolio::BandRelation;
        let relation_at = |spot: f64| authored_band_relation(Some(spot), Some(&targets));
        assert_eq!(relation_at(200.0), Some(BandRelation::Inside));
        assert_eq!(relation_at(150.0), Some(BandRelation::BelowBand));
        assert_eq!(relation_at(300.0), Some(BandRelation::AboveBand));
        assert_eq!(authored_band_relation(None, Some(&targets)), None);
        assert_eq!(authored_band_relation(Some(200.0), None), None);
    }

    #[test]
    fn the_holding_header_carries_identity_and_spot_only_and_names_the_issuer() {
        let mut d = dossier(AssetClass::Stock, strong_financials());

        d.position.description = "Phillips 66".to_string();
        d.company_name = Some("Phillips 66 Company".to_string());
        let h = holding_header(&d);
        assert!(h.starts_with("HOLDING\nAAPL (Phillips 66).\n"), "{h}");
        assert!(h.contains("Price: $195.00 per share.\n"), "{h}");
        assert!(h.ends_with("Date: 2026-07-28.\n"), "{h}");
        assert!(!h.contains("Current price"), "{h}");
        for absent in ["Quantity", "Cost basis", "Market value", " total"] {
            assert!(!h.contains(absent), "{absent} leaked: {h}");
        }
        let mut repriced = d.clone();
        repriced.position.quantity *= 3.0;
        repriced.position.cost_basis *= 3.0;
        repriced.position.market_value *= 3.0;
        assert_eq!(holding_header(&repriced), h);

        // `listing::describes_issuer` owns the no-identity rule; the guard pins
        // the same set.
        let ticker = d.position.symbol.clone();
        for described in ["", "  ", &ticker, "COMMON STOCK", "CL A ORD SHS"] {
            d.position.description = described.to_string();
            let h = holding_header(&d);
            assert!(
                h.contains("Phillips 66 Company"),
                "no-identity description {described:?} should fall back: {h}"
            );
        }

        // The fallback is held to the same standard: FMP accepts any non-blank
        // `companyName`.
        d.position.description = String::new();
        for profile_name in ["COMMON STOCK", &ticker, "  ", "ORD SHS"] {
            d.company_name = Some(profile_name.to_string());
            let h = holding_header(&d);
            assert!(
                h.contains("name unavailable"),
                "profile name {profile_name:?} carries no identity and must not render: {h}"
            );
        }

        d.company_name = None;
        let h = holding_header(&d);
        assert!(h.contains("name unavailable"), "{h}");
        assert!(!h.contains("()"), "an empty name pair is the defect itself: {h}");

        // A ticker-named issuer: both tokenize to the ticker, but the profile
        // name is a canonical legal name and must render.
        d.position.symbol = "ASML".to_string();
        d.position.description = "ASML HOLDING NV".to_string();
        d.company_name = Some("ASML Holding N.V.".to_string());
        let h = holding_header(&d);
        assert!(h.contains("ASML Holding N.V."), "{h}");
        assert!(!h.contains("name unavailable"), "{h}");
        d.company_name = Some("ASML".to_string());
        let h = holding_header(&d);
        assert!(h.contains("name unavailable"), "{h}");
    }

    /// A fund's `company_name` is structurally `None`, so the fund data's own
    /// name is the role-risk branch's only naming source.
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

        // The fund name is held to the same identity standard.
        if let Some(f) = d.fund.as_mut() {
            f.fund.name = Some(d.position.symbol.clone());
        }
        let h = holding_header(&d);
        assert!(h.contains("name unavailable"), "{h}");
    }

    /// Read off each schema rather than restated, so a field added to a grammar
    /// fails the test until its prompt declares it.
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
    fn the_thesis_conversation_requests_are_scoped_to_the_vehicle_and_the_debut_shape() {
        let stock = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&stock.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        fn input<'a>(d: &'a HoldingDossier, engine: &'a EngineOutput) -> ThesisInput<'a> {
            ThesisInput {
                dossier: d,
                engine,
                rates: rates_static(),
                analysis: analysis_record(""),
                pre_profit: None,
                soft_forensic: None,
                tech_pre_flag: None,
                narrative: None,
                prior_split: None,
                review: None,
            }
        }
        let debut_req = thesis_request("qwen", &input(&stock, &engine_output));
        assert!(debut_req.format_schema.is_none() && debut_req.think == Some(true));
        assert_eq!(debut_req.messages.len(), 2);
        assert!(debut_req.messages[0].content.starts_with("You are an equity analyst writing the thesis document"), "{}", debut_req.messages[0].content);
        let user = &debut_req.messages[1].content;
        assert!(!user.contains("PRIOR THESIS") && !user.contains("drawing on PRIOR THESIS"), "{user}");
        assert!(user.contains("\n6. A summary paragraph — the financial read, why those prices and that conviction.\n"), "{user}");
        let metrics = user.split("\nMETRICS\n").nth(1).unwrap().split("\nSCORES\n").next().unwrap();
        assert!(metrics.contains("- price / earnings multiple: ") && !metrics.contains("fund expense ratio"), "{metrics}");
        for absent in ["This is the first analysis of this holding.", "self_assessment", "what_changed", "CONTINUITY:", "RETURN SHAPE"] {
            assert!(!user.contains(absent), "`{absent}` leaked: {user}");
        }

        let appendix = appendix_request("qwen", &input(&stock, &engine_output), "The document.");
        assert_eq!(appendix.format_schema.as_ref(), Some(&crate::portfolio::appendix_schema()));
        assert_eq!(appendix.think, Some(false));
        assert_eq!(appendix.messages.len(), 4);
        assert_eq!(appendix.messages[0].content, debut_req.messages[0].content);
        assert_eq!(appendix.messages[1].content, debut_req.messages[1].content);
        assert_eq!((appendix.messages[2].role.as_str(), appendix.messages[2].content.as_str()), ("assistant", "The document."));
        assert_eq!(appendix.messages[3].role, "user");
        let ask = &appendix.messages[3].content;
        for key in non_empty(required_keys(appendix.format_schema.as_ref().unwrap()), "the appendix grammar") {
            assert!(ask.contains(&format!("\"{key}\"")), "the appendix ask does not declare {key}\n{ask}");
        }
        assert_eq!(
            appendix.options.as_ref().map(|o| o["num_predict"].clone()),
            Some(serde_json::json!(NUM_PREDICT_APPENDIX))
        );

        let mut cont = dossier(AssetClass::Stock, strong_financials());
        let (v, _) = analyze_holding(&StubAnalyst, &cont, &rates(), "2026-08-03").unwrap();
        assert!(v.thesis_document().is_some(), "{v:?}");
        cont.prior_verdict = Some(v);
        cont.prior_vintage = Some("2026-08-03T20:00:00Z".into());
        let cont_req = thesis_request("qwen", &input(&cont, &engine_output));
        assert!(cont_req.format_schema.is_none());
        let user = &cont_req.messages[1].content;
        assert!(user.contains("\nPRIOR THESIS (written 2026-08-03)\nThesis: hold AAPL "), "{user}");
        assert!(user.contains(", and what changed since the prior analysis, drawing on PRIOR THESIS.\n"), "{user}");
        assert!(user.contains("\n- close on the prior analysis date 2026-08-03: "), "{user}");
        let (part1, _) = user.split_once("======== PART 2: TASK ========").unwrap();
        let fetched = part1.split("\nFETCHED VALUES\n").nth(1).unwrap().split("\nCOMPUTED\n").next().unwrap();
        assert!(fetched.contains("\nCONSENSUS\n- consensus EPS by fiscal period end, as published: "), "{fetched}");
        assert!(!fetched.contains("blended"), "{fetched}");
        let metrics = part1.split("\nMETRICS\n").nth(1).unwrap().split("\nSCORES\n").next().unwrap();
        assert!(metrics.contains("- forward consensus, next twelve months blended over "), "{metrics}");

        let fund = fund_dossier(us_equity_fund());
        let fund_req = thesis_request("qwen", &input(&fund, &engine_output));
        assert!(fund_req.messages[0].content.starts_with("You are an investment analyst writing the thesis document"), "{}", fund_req.messages[0].content);
        let user = &fund_req.messages[1].content;
        let metrics = user.split("\nMETRICS\n").nth(1).unwrap().split("\nSCORES\n").next().unwrap();
        assert!(metrics.contains("- daily realized return volatility: ") && !metrics.contains("price / earnings multiple") && !metrics.contains("fund expense ratio"), "{metrics}");
        assert!(user.contains("\n- expense ratio: 0.0003 (0.03%/yr)\n"), "{user}");

        let prior_of = |disposition: VerdictDisposition| HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            basis_move: None,
            disposition,
            analyzed_at: Some("2026-08-03T20:00:00Z".into()),
            action_source: Default::default(),
            side_reversed: false,
        };
        let cases = [
            (
                prior_of(VerdictDisposition::RoleRiskOnly(Box::new(role_risk_verdict_from_model_arm(
                    &RoleRiskReadout::default(),
                    "Role: a sleeve.".into(),
                )))),
                Some("Role: a sleeve."),
            ),
            (
                prior_of(VerdictDisposition::InsufficientEvidence {
                    reason: "thin".into(),
                    prior_thesis_document: Some("The retained document.".into()),
                }),
                Some("The retained document."),
            ),
            (
                prior_of(VerdictDisposition::InsufficientEvidence {
                    reason: "thin".into(),
                    prior_thesis_document: None,
                }),
                None,
            ),
        ];
        for (prior, expected) in cases {
            let mut d = dossier(AssetClass::Stock, strong_financials());
            d.prior_verdict = Some(prior);
            d.prior_vintage = Some("2026-08-03T20:00:00Z".into());
            let user = thesis_user_prompt(&input(&d, &engine_output));
            match expected {
                Some(doc) => {
                    assert!(user.contains(&format!("\nPRIOR THESIS (written 2026-08-03)\n{doc}")), "{user}");
                    assert!(user.contains("drawing on PRIOR THESIS"), "{user}");
                }
                None => assert!(!user.contains("PRIOR THESIS"), "{user}"),
            }
        }

        let role_req = role_risk_request("qwen", &RoleRiskInput {
            dossier: &stock,
            readout: &RoleRiskReadout::default(),
            rates: rates_static(),
            analysis: analysis_record(""),
            prior_split: None,
            review: None,
        });
        assert!(role_req.format_schema.is_none() && role_req.think == Some(true));
        assert_eq!(role_req.messages[0].content, role_risk_system_prompt());
    }

    #[test]
    fn an_off_domain_appendix_re_issues_once_then_fails() {
        use crate::local_model::{retry_class, RetryClass, RetryOnce};
        let off_domain = r#"{"conviction":"high","expected_price_3m":-5,"expected_price_12m":null,"expected_price_3y":null}"#;
        let err = decode_appendix("appendix AAPL", off_domain).unwrap_err();
        assert_eq!(retry_class(&err), Some(RetryClass::ModelArmDomain));
        assert!(format!("{err:#}").contains("appendix off its declared domain"), "{err:#}");
        let err = decode_appendix("appendix AAPL", "not json").unwrap_err();
        assert_eq!(retry_class(&err), Some(RetryClass::SchemaParse));
        let silent = decode_appendix(
            "appendix AAPL",
            r#"{"conviction":null,"expected_price_3m":null,"expected_price_12m":null,"expected_price_3y":null}"#,
        )
        .unwrap();
        assert!(silent.is_empty());
        let retry = RetryOnce::without_delay();
        let progress = crate::progress::RunContext::noop();
        let attempts = std::cell::Cell::new(0u32);
        let out: Result<ThesisAppendix> = retry.run(&progress, "appendix AAPL", || {
            attempts.set(attempts.get() + 1);
            decode_appendix("appendix AAPL", off_domain)
        });
        let err = out.unwrap_err();
        assert_eq!(attempts.get(), 2, "one re-issue, never a second");
        assert!(format!("{err:#}").contains("appendix off its declared domain"), "{err:#}");
        let events = retry.take_events();
        assert_eq!(events.len(), 1, "{events:?}");
        assert!(events[0].cause.to_lowercase().contains("domain"), "{}", events[0].cause);
    }

    #[test]
    fn the_soft_forensic_section_renders_fired_clear_and_unevaluable() {
        use crate::portfolio::soft_forensic::{
            LineLeg, NetIncomeVsOperatingCashFlow, ScoreFlag, SoftFlagState, SoftForensicFlags,
            WorkingCapitalBuild,
        };
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let flags = SoftForensicFlags {
            altman_z: ScoreFlag { value: Some(1.21), state: SoftFlagState::Fired },
            piotroski: ScoreFlag { value: Some(7.0), state: SoftFlagState::Clear },
            net_income_vs_operating_cash_flow: NetIncomeVsOperatingCashFlow {
                ttm_net_income: None,
                ttm_operating_cash_flow: Some(2.5e9),
                state: SoftFlagState::Unevaluable { missing: vec!["TTM net income".into()] },
            },
            working_capital_build: WorkingCapitalBuild {
                revenue_growth: Some(0.08),
                receivables: LineLeg::Evaluated { growth: 0.31, fired: true },
                inventory: LineLeg::NotApplicable,
                state: SoftFlagState::Fired,
            },
        };
        fn input<'a>(
            d: &'a HoldingDossier,
            engine: &'a EngineOutput,
            soft: Option<&'a SoftForensicFlags>,
        ) -> ThesisInput<'a> {
            ThesisInput {
                dossier: d,
                engine,
                rates: rates_static(),
                analysis: analysis_record(""),
                pre_profit: None,
                soft_forensic: soft,
                tech_pre_flag: None,
                narrative: None,
                prior_split: None,
                review: None,
            }
        }
        let user = thesis_user_prompt(&input(&d, &engine_output, Some(&flags)));
        let (part1, _) = user.split_once("======== PART 2: TASK ========").unwrap();
        let computed = part1.find("\nCOMPUTED\n").expect("COMPUTED");
        let section = part1.find("\nSOFT FORENSIC FLAGS\n").expect("the section");
        let analysis = part1.find("\nANALYSIS\n").expect("ANALYSIS");
        assert!(computed < section && section < analysis, "{part1}");
        assert!(part1.contains("- Altman Z below 1.8: fired (Z 1.21).\n"), "{part1}");
        assert!(part1.contains("- Piotroski F-score at or below 3: clear (score 7).\n"), "{part1}");
        assert!(
            part1.contains(
                "- TTM net income above 1.3× TTM operating cash flow: unevaluable (missing: TTM net \
                 income) (net income (gap), operating cash flow 2.5B).\n"
            ),
            "{part1}"
        );
        assert!(
            part1.contains(
                "revenue growth, year over year on the latest quarter: fired (revenue growth +8.0%; \
                 receivables +31.0%, past the test; inventory not applicable).\n"
            ),
            "{part1}"
        );
        assert!(!thesis_user_prompt(&input(&d, &engine_output, None)).contains("SOFT FORENSIC FLAGS"));
    }

    // ---- The pre-profit execution / financing overlay ----------------------------

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
                depreciation_and_amortization: None,
            })
            .collect();
        fin.cash_and_equivalents = Some(6.0e9);
        fin.short_term_investments = Some(4.0e9);
        fin
    }

    #[test]
    fn every_stock_records_an_overlay_and_funds_record_none() {
        // A profitable stock with no operating-income prints is unscorable, not
        // entered.
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
        // The fixture carries no scores, so the flags are unevaluable.
        let flags = audit.soft_forensic.expect("every stock records its soft flags");
        assert!(matches!(
            flags.altman_z.state,
            crate::portfolio::soft_forensic::SoftFlagState::Unevaluable { .. }
        ));

        let (_, audit) = analyze_holding(
            &StubAnalyst,
            &fund_dossier(us_equity_fund()),
            &rates(),
            "2026-08-03",
        )
        .unwrap();
        assert!(audit.pre_profit.is_none());
        assert!(audit.soft_forensic.is_none());
    }

    #[test]
    fn eligible_overlay_renders_its_states_and_persists() {
        let d = dossier(AssetClass::Stock, pre_profit_financials());
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();

        let overlay = audit.pre_profit.expect("overlay rides the audit");
        assert!(overlay.is_eligible());
        assert_eq!(overlay.execution, crate::portfolio::pre_profit::ExecutionLeg::Unscorable);
        assert!(overlay.consequences.matched_rules.is_empty());
        let VerdictDisposition::Priced(g) = verdict.disposition else {
            panic!("expected a priced verdict");
        };
        let authored = match g.grade {
            crate::portfolio::Grade::A | crate::portfolio::Grade::B => Conviction::High,
            crate::portfolio::Grade::C => Conviction::Medium,
            _ => Conviction::Low,
        };
        assert_eq!(g.appendix.conviction, Some(authored));

        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let user = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record("none"),
            pre_profit: Some(&overlay),
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        assert!(user.contains("\nPRE-PROFIT EXECUTION AND FINANCING\n"), "{user}");
        // No arm narration or binding language aimed at the model.
        assert!(!user.contains("computed conviction capped"), "{user}");
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
        let overlay = pre_profit::compute_overlay(&fin);
        assert!(overlay.consequences.bar_add_family);
        assert!(!overlay.consequences.exit_family_only);

        let interp = pre_profit_prompt_section(&overlay, PromptStage::Thesis);
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
        // The action packet renders the facts alone, shared byte for byte.
        assert!(!action.contains("computed action set"), "{action}");
        assert!(interp.starts_with(action.as_str()), "{interp}\n{action}");
    }

    #[test]
    fn severe_overlay_binds_the_engine_arm_never_the_model() {
        struct DefiantAnalyst;
        impl HoldingAnalyst for DefiantAnalyst {
            fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
                let mut i = StubAnalyst.interpret(input)?;
                i.appendix.conviction = Some(Conviction::High);
                Ok(i)
            }
            fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String> {
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
        // The economics leg from the statements: the latest two quarters' gross
        // margin non-positive and 30pp below the preceding two.
        for (i, row) in fin.quarterly_income.iter_mut().take(4).enumerate() {
            let revenue = row.revenue.expect("the fixture carries revenue");
            row.gross_profit = Some(if i < 2 { -0.1 * revenue } else { 0.2 * revenue });
        }
        let d = dossier(AssetClass::Stock, fin);
        let (verdict, audit) =
            analyze_holding(&DefiantAnalyst, &d, &rates(), "2026-08-03").unwrap();
        let overlay = audit.pre_profit.expect("overlay rides the audit");
        assert_eq!(overlay.economics_deterioration, Some(true));
        assert!(overlay.severe_deterioration);
        assert!(overlay.consequences.exit_family_only);
        let VerdictDisposition::Priced(g) = verdict.disposition else {
            panic!("expected a priced verdict");
        };
        assert_eq!(g.action, Action::Add, "the model's lean persists as authored");
        assert_eq!(g.appendix.conviction, Some(Conviction::High), "the engine caps no conviction");
        assert!(
            matches!(g.engine_rung, Action::Trim | Action::SellAll),
            "the engine arm's own rung obeys its severe bar, got {:?}",
            g.engine_rung
        );

        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record("none"),
            pre_profit: Some(&overlay),
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
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
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(!action.contains("UNRESTRICTED"), "{action}");
        assert!(action.contains("- severe deterioration: YES\n"), "{action}");
        assert!(!action.contains("guidance attainment"), "the execution leg renders nothing: {action}");
        assert!(!action.contains("conviction capped"), "{action}");
        assert!(!action.contains("computed action set"), "{action}");
        assert!(
            action.contains("\nSUPPORTED ACTIONS\nThe rungs a fixed rule over the holding's reads supports, listed in full: sell-all, trim. A rung not listed is outside that rule.\n"),
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

        for prompt in [&interp, &action] {
            assert!(!prompt.contains("lean set"), "{prompt}");
            assert!(!prompt.contains("your lean"), "{prompt}");
            assert!(!prompt.contains("Your lean"), "{prompt}");
        }
    }

    #[test]
    fn abstaining_stock_still_records_the_overlay() {
        // No consensus abstains the engine.
        let mut fin = pre_profit_financials();
        fin.consensus = None;
        let d = dossier(AssetClass::Stock, fin);
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::InsufficientEvidence { .. }
        ));
        let overlay = audit.pre_profit.expect("overlay survives an abstention");
        assert!(overlay.is_eligible());
        assert!(audit.soft_forensic.is_some(), "the soft flags persist with the abstention");
    }

    #[test]
    fn a_guard_conflict_abstention_records_the_overlay() {
        use crate::portfolio::listing::ListingResolution;
        use crate::portfolio::pre_profit::PreProfitEligibility;
        let mut d = dossier(
            AssetClass::Stock,
            CompanyFinancials { symbol: "X".into(), ..CompanyFinancials::default() },
        );
        d.listing = Some(ListingResolution::Conflict {
            fmp_name: "Wrong Issuer Inc.".into(),
        });
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
        let flags = audit.soft_forensic.expect("the flags survive the guard exit");
        for state in [
            &flags.altman_z.state,
            &flags.piotroski.state,
            &flags.net_income_vs_operating_cash_flow.state,
            &flags.working_capital_build.state,
        ] {
            assert!(
                matches!(state, crate::portfolio::soft_forensic::SoftFlagState::Unevaluable { .. }),
                "{state:?}"
            );
        }
    }

    // ---- The investment-only action packet and the app-appended tax caveat ------

    #[test]
    fn the_action_packet_states_the_set_and_the_position_once_and_sizes_no_overlay() {
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
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
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
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert_eq!(user.matches("higher is better on every axis").count(), 0, "{user}");
        assert_eq!(user.matches("\nSUPPORTED ACTIONS\n").count(), 1, "{user}");
        for absent in [
            "The full ladder is yours", "its own pick", "ENGINE SET", "ENGINE ADMISSION FACTS",
            "restriction", "not a bound", "departure", "Its selected action",
        ] {
            assert!(!user.contains(absent), "{absent}: {user}");
        }
        // The overlay renders structure and ratios, never the held share count.
        assert_eq!(user.matches("\nPOSITION\n").count(), 1, "{user}");
        for once in ["Shares held: 100.", "Cost basis: $14,000.00.", "Market value: $19,500.00."] {
            assert_eq!(user.matches(once).count(), 1, "{once}: {user}");
        }
        for absent in ["share-equivalents", "2×", "- tax", "Quantity"] {
            assert!(!user.contains(absent), "{absent}: {user}");
        }
        assert!(
            user.starts_with("======== PART 1: INPUTS ========\nHOLDING\nAAPL (Apple).\nPrice: $195.00 per share.\n"),
            "{user}"
        );
        assert!(user.contains("covering 100% of the held shares; net delta -70% of the held shares"), "{user}");
        assert!(user.contains("- SHORT CALL — strike 220.00"), "{user}");
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record(""),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
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
        // A sentence that merely mentions tax stays.
        assert_eq!(investment_sentence("Hold; tax is not the reason."), "Hold; tax is not the reason.");
        assert_eq!(investment_sentence(""), "");
    }

    #[test]
    fn position_section_renders_the_economics_and_the_change_since_the_last_pull() {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let values = |d: &HoldingDossier| {
            let s = position_section(d);
            s.lines().last().unwrap().to_string()
        };
        assert_eq!(
            values(&d),
            "Shares held: 100. Cost basis: $14,000.00. Market value: $19,500.00. Unrealized gain: \
             $5,500.00 (+39.3% of the cost basis). Change since the last pull: new."
        );
        d.position.cost_basis = 30_000.0;
        d.position_delta = PositionDelta {
            change: PositionChange::Increased,
            prior_quantity: Some(60.0),
            prior_cost_basis: Some(8_000.0),
        };
        assert_eq!(
            values(&d),
            "Shares held: 100. Cost basis: $30,000.00. Market value: $19,500.00. Unrealized loss: \
             $10,500.00 (-35.0% of the cost basis). Change since the last pull: increased, from \
             60 to 100."
        );
        assert!(!values(&d).contains("averaged"), "{}", values(&d));
        d.position.quantity = 12.5;
        d.position.cost_basis = 2_437.5;
        d.position.market_value = 2_437.5;
        d.position_delta = PositionDelta {
            change: PositionChange::Decreased,
            prior_quantity: Some(20.0),
            prior_cost_basis: Some(3_900.0),
        };
        assert_eq!(
            values(&d),
            "Shares held: 12.5. Cost basis: $2,437.50. Market value: $2,437.50. Unrealized gain or \
             loss: none. Change since the last pull: decreased, from 20 to 12.5."
        );
        // A negative netted basis keeps the dollar gain with no percentage
        // (`docs/portfolio-analysis.md` §Storage and display).
        d.position.quantity = -40.0;
        d.position.cost_basis = -8_000.0;
        d.position.market_value = -7_800.0;
        d.position_delta = PositionDelta {
            change: PositionChange::Unchanged,
            prior_quantity: Some(-40.0),
            prior_cost_basis: Some(-8_000.0),
        };
        assert_eq!(
            values(&d),
            "Shares held: 40 short. Cost basis: -$8,000.00. Market value: -$7,800.00. Unrealized \
             gain: $200.00. Change since the last pull: unchanged at 40 short."
        );
        // An exactly-zero basis is an unreported one on the wire.
        d.position.quantity = 5.0;
        d.position.cost_basis = 0.0;
        d.position.market_value = 50.0;
        d.position_delta = PositionDelta::new_position();
        assert_eq!(
            values(&d),
            "Shares held: 5. Cost basis: not reported. Market value: $50.00. Unrealized gain or \
             loss: not available without a reported cost basis. Change since the last pull: new."
        );
        assert!(!values(&d).contains("Unrealized gain:"), "{}", values(&d));
        let s = position_section(&d);
        assert!(s.starts_with("\nPOSITION\nThe holding as the account carries it: "), "{s}");
        assert!(s.contains("as a share of a positive cost basis; not available where no basis is reported)"), "{s}");
        assert_eq!(s.trim_start().lines().count(), 3, "{s}");
    }

    #[test]
    fn position_amounts_format_with_separators_and_counts_drop_trailing_zeros() {
        assert_eq!(fmt_usd(0.0), "$0.00");
        assert_eq!(fmt_usd(999.999), "$1,000.00");
        assert_eq!(fmt_usd(1_234_567.891), "$1,234,567.89");
        assert_eq!(fmt_usd(-42.5), "-$42.50");
        assert_eq!(fmt_usd(f64::NAN), "(gap)");
        assert_eq!(fmt_shares(100.0), "100");
        assert_eq!(fmt_shares(0.3333), "0.3333");
        assert_eq!(fmt_shares(2.5), "2.5");
        assert_eq!(fmt_shares(-3.0), "3 short");
    }

    #[test]
    fn the_thesis_message_renders_no_position_block() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates(), Default::default()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let interp = thesis_user_prompt(&ThesisInput {
            rates: rates_static(),
            soft_forensic: None,
            prior_split: None,
            dossier: &d,
            engine: &engine_output,
            analysis: analysis_record(""),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
            review: None,
        });
        for absent in ["\nPOSITION\n", "Shares held:", "Cost basis:", "Market value:", "Unrealized"] {
            assert!(!interp.contains(absent), "{absent}: {interp}");
        }
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
        pos.cost_basis = 0.0;
        assert_eq!(tax_caveat(&profile, &pos, Action::SellAll), None, "unreported basis");
        pos.cost_basis = -1_000.0;
        assert_eq!(tax_caveat(&profile, &pos, Action::Trim), Some(TAX_CAVEAT_GAIN), "negative netted basis");
        profile.tax_sensitive = false;
        pos.cost_basis = 14_000.0;
        assert_eq!(tax_caveat(&profile, &pos, Action::Trim), None, "tax-exempt");
        assert_eq!(
            with_tax_caveat("Trim on the verdict.  ".into(), &InvestorProfile::default_fixture(), &pos, Action::Trim),
            format!("Trim on the verdict. {TAX_CAVEAT_GAIN}")
        );

        struct Trimmer;
        impl HoldingAnalyst for Trimmer {
            fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
                StubAnalyst.interpret(input)
            }
            fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String> {
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

}
