//! The per-holding pipeline (`docs/portfolio-analysis.md` §The per-holding pipeline).
//! Orchestrates one holding from its deterministic dossier through the engine to a
//! two-arm verdict: eligibility → financial engine → bounded research → distill →
//! the thesis-document conversation → the action call → checkpoint. The engine
//! owns the engine arm's numbers, app-stamped and never echoed through the
//! model; the model arm is the thesis document the reasoner writes and the
//! typed appendix it transcribes from it ([`crate::portfolio::ThesisAppendix`]),
//! persisted exactly as authored, type-checked only, and never altering or
//! binding the engine baseline (the boundary statement:
//! `docs/portfolio-analysis.md` §The holding verdict).
//!
//! The model stages live behind the [`HoldingAnalyst`] trait so `cargo test` runs the
//! whole pipeline offline against [`StubAnalyst`] with no daemon, while the live
//! [`LocalAnalyst`] wraps [`crate::local_model::LocalModelClient`] with the right
//! thinking modes — the thesis document free prose under thinking, the appendix
//! a grammar-constrained non-thinking transcription. The substrate is a
//! *primitive*; this is one of the per-feature stages that wraps it
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
use crate::portfolio::engine::{self, EngineOutput, EngineVerdict, RateAnchors};
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

/// What the thesis-document conversation reads (`docs/portfolio-workflow.md`
/// §Step 6f): the dossier, the engine's computed analysis, the run-level rate
/// prints and this run's analysis. The model reasons over *this* — evidence,
/// not a gathering transcript. It carries **no investor profile, no position
/// economics and no action machinery**: the intrinsic verdict is
/// profile-independent by input isolation, and the per-holding action call
/// ([`ActionInput`]) is where both live (`docs/portfolio-analysis.md`
/// §Intrinsic verdict).
pub struct ThesisInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub engine: &'a EngineOutput,
    /// The run-level Treasury prints FETCHED VALUES states.
    pub rates: &'a RateAnchors,
    /// This run's analysis record, rendered under ANALYSIS — the document
    /// Step 6d's analysis call wrote over the write-ups, or the prior record
    /// carried whole where the loop produced no write-up
    /// ([`distill::consolidate`]); a carried one renders under the date it
    /// was written with its own anchor bar's split-context line
    /// ([`analysis_section`]).
    pub analysis: crate::portfolio::AnalysisRecord,
    /// The finalized pre-profit execution / financing overlay — present only
    /// when the stock actually entered it.
    pub pre_profit: Option<&'a PreProfitOverlay>,
    /// The four soft forensic flags beside the hard state, rendered as typed
    /// evidence on a priced stock; `None` on a fund.
    pub soft_forensic: Option<&'a SoftForensicFlags>,
    /// The input delta's technology-event pre-flag, where it was evaluable —
    /// rendered only when fired; it asserts nothing about the cause.
    pub tech_pre_flag: Option<&'a engine::TechEventPreFlag>,
    /// The narrative-vs-reality read, where it was computable.
    pub narrative: Option<&'a engine::NarrativeRead>,
    /// The split-context line PRIOR THESIS carries where a split re-based the
    /// price series since the prior document was written; `None` on a debut
    /// or where no split intervened.
    pub prior_split: Option<SplitContext>,
}

/// The split-context line's facts (`docs/portfolio-workflow.md` §Step 6b): what
/// the line above a verbatim prior document states about its price basis —
/// the document itself is never rewritten.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitContext {
    /// The series was re-based since the document was written: the cumulative
    /// factor ([`engine::split_bridge_factor`]) that brings its prices to
    /// today's basis, and the split's date and ratio where the splits feed
    /// carries a split after the document's session ([`split_event_since`]).
    Rebased {
        factor: f64,
        split: Option<SplitEvent>,
    },
    /// The document's anchor bar is missing from the fetched window: whether a
    /// split intervened is unknown, so the line says so rather than staying
    /// silent (silence would read as "no split").
    Unverifiable,
}

/// What the `role_risk_only` thesis-document message reads: the dossier plus the
/// engine's typed readout — none of the priced machinery exists on this branch,
/// and no appendix follows the document.
pub struct RoleRiskInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub readout: &'a RoleRiskReadout,
    pub rates: &'a RateAnchors,
    /// This run's analysis record — the fund agenda's write-ups consolidated
    /// by Step 6d's analysis call, or the prior record carried whole
    /// ([`distill::consolidate`]), rendered as on the priced message
    /// ([`analysis_section`]).
    pub analysis: crate::portfolio::AnalysisRecord,
    pub prior_split: Option<SplitContext>,
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
    RoleRisk {
        verdict: &'a RoleRiskVerdict,
    },
}

/// What the **per-holding action call** reads (`docs/portfolio-analysis.md`
/// §Portfolio action): the finished intrinsic verdict, the holding's own
/// evidence off the dossier, the engine's per-holding action set (evidence,
/// never a bar), and the **investor profile** — its only entry point into the
/// job, so the thesis document stays profile-blind by input isolation. Tunnel
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
/// realized (hold, the add family), under a tax-exempt profile, at break-even,
/// or with no reported cost basis: an exactly-zero basis is an unreported one
/// on the wire (the adapter maps a missing `averagePrice` to zero), so its
/// gain is undefined rather than the whole market value
/// (`docs/portfolio-analysis.md` §Storage and display); a negative netted
/// basis keeps its dollar gain and so its caveat.
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
/// stub-driven offline and daemon-driven live. Research (6c) carries a
/// **defaulted offline implementation** — pipeline-shaped, no web tool, no
/// model call — so deterministic stubs stay small; consolidation's two calls
/// (6d) default to a refusal naming the stage, since a stub with no write-up
/// is never asked to consolidate, and the offline [`StubAnalyst`] renders
/// them deterministically; the live analyst overrides all three.
pub trait HoldingAnalyst {
    /// Step 6d — one distillation call over the write-ups
    /// ([`distill::distillation_prompt`]): non-thinking, no grammar, prose.
    fn distill(&self, input: &distill::DistillInput<'_>) -> Result<String> {
        anyhow::bail!("{}: this analyst issues no distillation call", input.stage)
    }
    /// Step 6d — the analysis call ([`distill::analysis_prompt`]): thinking,
    /// no grammar, prose.
    fn analyze(&self, input: &distill::AnalysisInput<'_>) -> Result<String> {
        anyhow::bail!("{}: this analyst issues no analysis call", input.stage())
    }
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
    /// Step 6f — the thesis-document conversation (`docs/portfolio-workflow.md`
    /// §Step 6f): the document as prose under thinking with no grammar, then
    /// the typed appendix transcribed from it in the same conversation's
    /// second, non-thinking message under the grammar. Returns the model arm
    /// exactly as authored, the appendix type-checked only.
    fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm>;
    /// The union's other branch for a structurally unpriceable vehicle: the
    /// thesis document alone — the role read, with no prices, no conviction
    /// and no appendix (`docs/portfolio-analysis.md` §Intrinsic verdict). The
    /// action call authors the branch's action afterward.
    fn interpret_role_risk(&self, input: &RoleRiskInput) -> Result<String>;
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

/// The holding-constant brief the research loop leads every gathering message
/// with (`docs/portfolio-workflow.md` §Step 6c): the holding header, FETCHED
/// VALUES as the thesis-document message renders it — the same bytes, so the
/// three messages share one rendering — the news leads, and on a continuity
/// run PRIOR ANALYSIS then PRIOR THESIS, each with its date and any
/// split-context line, the prior run's analysis and thesis document verbatim.
/// The thesis document's split line is the verdict bridge's (`prior_split`);
/// the analysis's comes from its own record's anchor bar.
pub(crate) fn research_brief(
    dossier: &HoldingDossier,
    rates: &RateAnchors,
    prior_split: Option<SplitContext>,
) -> HoldingBrief {
    HoldingBrief {
        header: holding_header(dossier),
        fetched_values: fetched_values_section(dossier, rates),
        leads: dossier.news_seeds.clone(),
        prior_documents: format!(
            "{}{}",
            prior_analysis_section(dossier),
            prior_thesis_section(dossier, prior_split)
        ),
    }
}

/// The split-bridge anchor bar a run stamps on what it writes
/// (`docs/portfolio-analysis.md` §Starting parameters): the newest settled
/// close strictly before the run's ET session, from this run's own fetched
/// dated-EOD series. A later pass re-reads the same bar date from its fresh
/// fetch, and the close ratio is exactly the cumulative split re-basis
/// between the two fetch times ([`engine::split_bridge_factor`]).
fn authoring_close_of(d: &HoldingDossier, run_date: &str) -> Option<engine::DatedValue> {
    d.financials
        .daily_closes
        .iter()
        .rev()
        .find(|c| c.date.as_str() < run_date)
        .cloned()
}

/// A split the splits feed carries — the date and ratio the split-context
/// line names beside the bridge factor (`docs/portfolio-workflow.md` §Step 6b).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitEvent {
    pub date: chrono::NaiveDate,
    pub numerator: f64,
    pub denominator: f64,
}

/// The newest split the evidence record's feed carries dated after `since`
/// (an ISO session date) — `None` where the feed carries none, or the dossier
/// no evidence record.
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
/// (`docs/portfolio-workflow.md` §Step 6b): re-based where the factor moved
/// off one — naming the split the feed carries since the document's session,
/// where it carries one — none where the factor is exactly one, unverifiable
/// where the anchor's bar is missing from the fresh window.
fn split_context_of(bridge: Option<f64>, split: Option<SplitEvent>) -> Option<SplitContext> {
    match bridge {
        Some(f) if f != 1.0 => Some(SplitContext::Rebased { factor: f, split }),
        Some(_) => None,
        None => Some(SplitContext::Unverifiable),
    }
}

/// Run Steps 6c and 6d for one holding: assemble the deterministic research
/// plan (the agenda and the holding-constant brief), run the analyst's
/// research loop, then consolidate — the budget check, a distillation where
/// the analysis prompt is over budget, and the analysis call
/// ([`distill::consolidate`]) — returning the holding's analysis record
/// beside the audit record the run persists. An analysis written this run is
/// stamped with this run's session date and anchor bar; a carried one keeps
/// the prior record whole.
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
        // The holding's own tracker step: the loop's thinking and request rows
        // land on the step the job already opened for this symbol.
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
    let is_fund = matches!(
        asset_class,
        crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
    );
    // App-set from the deterministic holdings diff, never the model — carried on every
    // verdict (graded or not) as the structured what-changed position tag.
    let position_change = dossier.position_delta.change;
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
    // The split-context line a verbatim prior document carries
    // (`docs/portfolio-workflow.md` §Step 6b): the document is never rewritten;
    // the line states the factor where a split re-based the series since it
    // was written, or that the basis could not be verified this run.
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
    // `false` for every route that returns before a thesis-document call — the
    // eligibility gate, the listing guard, a net-short or fully-offset position, and
    // every evidence-floor abstention — and each thesis path sets it from the
    // predicate belonging to the prompt it is about to build.
    let house_view_consulted = std::cell::Cell::new(false);
    // Whether the FRED rate anchors actually fed this holding's verdict: the
    // priced engine outputs compute from them (the scenario targets and the
    // hurdle read, on the stock path and the priced fund path), and both
    // thesis-document messages state the prints under FETCHED VALUES. Every
    // earlier exit renders no prompt and computes nothing from them, so its
    // audit must not name them (M3 of the 2026-08-18 doc/code audit).
    let rates_consulted = std::cell::Cell::new(false);
    // The Step-5 enriching feeds, each recorded only where a prompt actually
    // rendered it (the same actually-consulted discipline as the house view):
    // the CBOE backdrop and the fund's COT positioning render whenever present
    // on the dossier at a thesis-document call; the sector-benchmark series is
    // consulted exactly where the technology-event pre-flag evaluation read it.
    let backdrop_consulted = std::cell::Cell::new(false);
    let positioning_consulted = std::cell::Cell::new(false);
    let benchmark_consulted = std::cell::Cell::new(false);
    let commodity_consulted = std::cell::Cell::new(false);
    let short_interest_consulted = std::cell::Cell::new(false);
    let ma_consulted = std::cell::Cell::new(false);
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
    // The split-bridge anchor: the newest settled bar strictly before the run's
    // ET session, off the run's own fetched series (oldest-first). Strictly
    // before keeps the anchor off the run day's still-forming bar, so a
    // re-fetch reads back the identical close unless the series was re-based.
    // Every pass that writes stamps its own, an unresolvable prior bridge
    // included: everything its row persists — the document, the appendix's
    // prices, the bands, the quick basis — is on the run's own basis, and the
    // excluded prior comparisons are that run's recorded loss, never carried
    // onto its row. Only an abstention carries an anchor forward — the retained
    // document's (`abstain` below) — since its row persists no value on the
    // run's basis (`docs/portfolio-analysis.md` §Starting parameters).
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
        // The soft flags ride wherever the overlay record does — every
        // priced-stock path, the floor and guard exits included.
        soft_forensic,
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
        // Recorded only where the research loop ran; every no-research exit
        // records none.
        research: None,
        // Written where consolidation ran; an abstention retains the prior
        // run's below, every other no-research exit records none.
        analysis: None,
    };
    let abstain = |reason: String, metrics, meta, pre_profit, soft_forensic| {
        // A below-floor exit retains the prior thesis document unrewritten —
        // Steps 6c–6f never ran for it (`docs/portfolio-analysis.md`
        // §Evidence floor), so the next continuity run still reads it.
        let retained = dossier
            .prior_verdict
            .as_ref()
            .and_then(|v| v.thesis_document().map(str::to_string));
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            disposition: VerdictDisposition::InsufficientEvidence {
                reason,
                prior_thesis_document: retained.clone(),
            },
            // Vintages are the job layer's concern: it stamps a fresh pass with the
            // run's `created_at` and preserves an abstention's prior vintage.
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
            side_reversed: false,
        };
        // An abstaining stock still records its overlay (fresh statement leg +
        // carried observation history) and its soft forensic flags — engine-only
        // state, no model dependency, so the history survives an abstention like
        // the retained document does.
        let mut record = audit(metrics, meta, pre_profit, soft_forensic);
        // The retained document stays on the basis it was written, so the row
        // carries the document's own anchor bar: the next continuity run's
        // bridge then still reads the split factor its prices need. A fresh
        // stamp would certify pre-split prices on today's basis — factor 1 on
        // the next pass, the mismatch never re-detectable.
        if retained.is_some() {
            record.authoring_close = dossier.prior_authoring_close.clone();
        }
        // The prior analysis is retained unrewritten the same way — the
        // holding's research memory survives the exit, so the next continuity
        // run still loads it by identity (`docs/portfolio-analysis.md`
        // §Evidence floor). Unconditionally, unlike the anchor bar above: the
        // analysis has no price basis to certify, so it carries whether or
        // not a document was retained beside it.
        record.analysis = dossier.prior_analysis.clone();
        Ok((verdict, record))
    };
    let not_rated = |reason: String| {
        let verdict = HoldingVerdict {
            symbol: symbol.clone(),
            asset_class,
            position_change,
            disposition: VerdictDisposition::NotRated { reason },
            analyzed_at: None,
            action_source: ActionSource::ModelChosen,
            side_reversed: false,
        };
        Ok((verdict, audit(Default::default(), None, None, None)))
    };

    // Eligibility: a non-equity class is never given a fabricated grade.
    if !asset_class.is_gradeable() {
        return not_rated(format!(
            "{} is not graded by the equity pipeline",
            asset_class.label()
        ));
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
        return not_rated(reason.to_string());
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
            return not_rated(reason);
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

    // The deterministic engine stage, per branch: the equity engine for a stock, the
    // reduced fund computation (strategy-routed at loop time) for a fund
    // (`docs/portfolio-workflow.md` §Step 6b).
    let mut pre_profit_overlay: Option<PreProfitOverlay> = None;
    // The soft forensic flags, computed beside the overlay on the stock path
    // (`docs/portfolio-workflow.md` §Step 6b) and persisted wherever it is.
    let mut soft_forensic_flags: Option<crate::portfolio::soft_forensic::SoftForensicFlags> = None;
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
                // The branch's computed surface — the expense ratio, the
                // closed-end read and the price-derived legs — persists as the
                // audit's metrics and renders into the message.
                let fund_metrics = fund_metrics(&readout, &dossier.financials);
                // The union's other branch: the model authors the thesis document
                // carrying the role read, with no prices, no conviction and no
                // appendix — the branch's action is authored by the dedicated
                // per-holding action call below, the full ladder structurally
                // open while the engine arm's reduced set (sell-all / trim /
                // hold) rides as annotated evidence (`docs/portfolio-analysis.md`
                // §Portfolio action).
                house_view_consulted.set(prompt_renders_house_view(dossier));
                backdrop_consulted.set(dossier.put_call_backdrop.is_some());
                positioning_consulted
                    .set(dossier.fund.as_ref().is_some_and(|f| f.positioning.is_some()));
                // The fund agenda runs the same 6c loop (`docs/portfolio-workflow.md`
                // §Step 6c); the brief states the Treasury prints under FETCHED
                // VALUES, so the source is consulted here too.
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
                let thesis_document = analyst
                    .interpret_role_risk(&RoleRiskInput {
                        dossier,
                        readout: &readout,
                        rates,
                        analysis: rr_analysis.clone(),
                        prior_split,
                    })
                    .context("writing the role/risk holding's thesis document")?;
                record_stage_models(analyst.reasoner_id());
                // The action placeholder is overwritten by the decision below and
                // never rendered into its prompt.
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
                // The branch's computed surface persists as the audit's metrics —
                // the same expense-ratio + price-derived legs the message rendered
                // (plus the CEF-only closed-end read), never the empty default
                // (M3 of the 2026-08-18 audit).
                let mut audit_record = audit(fund_metrics, None, None, None);
                audit_record.research = Some(rr_research_record);
                audit_record.analysis = Some(rr_analysis);
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
                    analyzed_at: None,
                    action_source: ActionSource::ModelChosen,
                    side_reversed: false,
                };
                return Ok((verdict, audit_record));
            }
        }
    } else {
        // The pre-profit overlay's statement legs over the carried observation
        // history (`docs/portfolio-workflow.md` §Step 6b) — no research-fed
        // rows exist: the execution read has no producer. Computed for every
        // stock: the eligibility result persists even when the stock does not
        // enter.
        pre_profit_overlay = Some(pre_profit::compute_overlay(
            &dossier.financials,
            dossier.prior_pre_profit.as_ref(),
            Vec::new(),
        ));
        soft_forensic_flags = Some(crate::portfolio::soft_forensic::compute(&dossier.financials));
        match engine::analyze(&dossier.financials, rates) {
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
    // → the thesis-document conversation.
    house_view_consulted.set(prompt_renders_house_view(dossier));
    backdrop_consulted.set(dossier.put_call_backdrop.is_some());
    positioning_consulted.set(dossier.fund.as_ref().is_some_and(|f| f.positioning.is_some()));
    commodity_consulted.set(!dossier.commodity_context.is_empty());
    short_interest_consulted.set(dossier.short_interest.is_some());
    ma_consulted.set(!dossier.ma_matches.is_empty());
    // The conditional topics' deterministic triggers (`docs/portfolio-workflow.md`
    // §Step 6c): the technology-event pre-flag is the technology topic's only
    // trigger, decided when the agenda is assembled; the symbol-scoped news
    // seeds ride the pass brief as leads and trigger nothing.
    let triggers = research::AgendaTriggers {
        tech_pre_flag_fired: tech_pre_flag.as_ref().is_some_and(|f| f.fired),
        overlay_eligible: pre_profit_overlay.as_ref().is_some_and(|o| o.is_eligible()),
    };
    let (analysis, research_record) =
        run_research(analyst, dossier, &triggers, rates, prior_split, run_date)?;
    record_stage_models(analyst.reasoner_id());

    // The hard-forensic state trips from the item-classified filing kinds
    // alone (`docs/portfolio-analysis.md` §Starting parameters): a fraud
    // allegation reaches the model only through the research write-ups and
    // the analysis, as prose it weighs.
    let filing_state = dossier.filing_events.clone();
    let hard_forensic = filing_state
        .as_ref()
        .map(crate::portfolio::ForensicFilingState::hard_tripped)
        .unwrap_or(false);

    // The overlay's rules join only when the stock actually entered the overlay
    // (a priced fund carries none) — they bind the engine's own rung and its
    // per-holding action set below. The overlay is the statement legs alone:
    // its execution read has no deterministic producer, and the issuer's
    // operating observations reach the model through the write-ups
    // (`docs/portfolio-workflow.md` §Step 6b).
    let overlay_rules = pre_profit_overlay
        .as_ref()
        .filter(|o| o.is_eligible())
        .map(|o| &o.consequences);
    // The thesis-document conversation (`docs/portfolio-workflow.md` §Step
    // 6f): the document as prose, then the typed appendix transcribed from it
    // — the model arm, persisted exactly as authored and type-checked only.
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
        })
        .context("writing the holding's thesis document")?;
    record_stage_models(analyst.reasoner_id());

    // The engine arm's own rung — the drafted rule over its reads, the
    // hard-forensic exit branch first — and the authoring-time band relation
    // the quick check's monitor compares against, on the pass's own basis like
    // its band and its anchor (`docs/portfolio-analysis.md` §Starting
    // parameters; §The quick check).
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
        // The basis beneath this pass's own anchor — persisted on every priced
        // pass, so the sweep's conversions read one basis.
        quick_basis: engine_output.quick_basis.clone(),
        authoring_close: authoring_close.clone(),
        fund_exposure: fund_exposure.clone(),
        pre_profit: pre_profit_overlay,
        soft_forensic: soft_forensic_flags,
        // The full hurdle read persists so a decision episode's calibration
        // snapshot can freeze the hurdle inputs (`docs/portfolio-analysis.md`
        // §Outcome learning).
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
    };
    Ok((verdict, audit_record))
}

// ---- Prompt construction (pure, testable) ------------------------------------

/// The system prompt of the thesis-document conversation (`docs/portfolio-workflow.md`
/// §Step 6f): the role and the two-part shape of the message — nothing that
/// describes the data the message carries, no app concept
/// (`docs/local-models.md` §Prompt posture). The same system message heads the
/// conversation's second, appendix message.
pub fn thesis_system_prompt(is_fund: bool) -> String {
    format!(
        "You are an {} analyst writing the thesis document for one holding in a portfolio \
         review. Part 1 of the message gives the inputs. Part 2 says what the document covers, \
         in order, and how to return it.",
        if is_fund { "investment" } else { "equity" }
    )
}

/// The system prompt of the `role_risk_only` thesis-document message: the same
/// footing as [`thesis_system_prompt`], the vehicle named as a fund since this
/// branch is a fund by construction.
pub fn role_risk_system_prompt() -> String {
    "You are an investment analyst writing the thesis document for one fund holding in a \
     portfolio review. Part 1 of the message gives the inputs. Part 2 says what the document \
     covers, in order, and how to return it."
        .to_string()
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
/// closed-end read off the readout, plus the price legs (trailing return, return
/// volatility) from the closes the dossier carries. Built once for the audit
/// and the message, so the two read one surface.
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

/// The `role_risk_only` thesis-document message (`docs/portfolio-workflow.md`
/// §Step 6f): one message in two marked parts. Part 1 is inputs only — HOLDING,
/// FETCHED VALUES, then the fund's reads in place of the equity reads: CLASS
/// (the label, the reported asset class and the structure line), EXPOSURE TILT
/// with the closed-end line and the positioning line, RISK PROFILE with the
/// market-wide options backdrop, EVIDENCE GAPS as data statements, the
/// COMPUTED metric lines, MARKET ANALYSIS, ANALYSIS and on a continuity run
/// PRIOR THESIS verbatim — each section explained once and then its values,
/// with no instruction in it. Part 2 is the task only: the document the model
/// writes — the role, the risks, the trim / sell triggers and the summary —
/// with no prices and no conviction. The model receives data, never a
/// description of the app that produced it, and no position economics
/// (`docs/portfolio-analysis.md` §Intrinsic verdict).
pub fn role_risk_user_prompt(input: &RoleRiskInput) -> String {
    let d = input.dossier;
    let r = input.readout;
    let mut p = String::from("======== PART 1: INPUTS ========\n");

    // HOLDING
    p.push_str(&holding_header(d));

    // FETCHED VALUES
    p.push_str(&fetched_values_section(d, input.rates));

    // CLASS: the label, the fund's reported asset class where the metadata
    // carries one, and the structure line where it applies.
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
    // backdrop. The daily volatility is a COMPUTED line, so it renders once.
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

    // COMPUTED: the branch's computed surface, each line with its unit.
    let metrics = fund_metrics(r, &d.financials);
    p.push_str("\nCOMPUTED\nThe computed metrics, each with its unit.\n");
    p.push_str(&computed_metrics_lines(true, &metrics));

    // MARKET ANALYSIS
    p.push_str(&market_analysis_section(d));

    // ANALYSIS: this run's analysis — the fund agenda's write-ups consolidated
    // by Step 6d's analysis call, or the prior record carried, dated and
    // bridged.
    p.push_str(&analysis_section(d, &input.analysis));

    // PRIOR THESIS, on a continuity run.
    let has_prior = d.prior_verdict.as_ref().and_then(|v| v.thesis_document()).is_some();
    p.push_str(&prior_thesis_section(d, input.prior_split));

    // PART 2
    p.push_str(&role_risk_task_section(has_tilt, has_gaps, has_prior));
    p
}

/// Part 2 of the role/risk message: the document's items in output order — the
/// role from the sections that rendered, the risks, the trim / sell triggers,
/// the summary paragraph — within the thesis document's length band, with no
/// prices and no conviction.
fn role_risk_task_section(has_tilt: bool, has_gaps: bool, has_prior: bool) -> String {
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
        if has_prior {
            ", and what changed since the prior analysis, drawing on PRIOR THESIS"
        } else {
            ""
        }
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
        s.push_str(&format!("Rule matched: {rule}.\n"));
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

/// The thesis-document message (`docs/portfolio-workflow.md` §Step 6f): one
/// message in two marked parts on the frame every Portfolio prompt shares.
/// Part 1 is inputs only, in page order — HOLDING with the analysis date;
/// FETCHED VALUES; COMPUTED (the engine's metrics with the statement-basis
/// line, the grade and sub-scores with the polarity gloss and the imputed
/// disclosure, the three bands with the twelve-month method and the target
/// provenance notes, the risk tier, the capital-efficiency read, the hard
/// forensic read as typed evidence with its rule as a fact, the soft forensic
/// flags, the narrative-vs-reality read, the implied-expectations range, the
/// short interest, the options signal, the option overlay, the overlay's
/// financing / economics / dilution legs, and the Step-5 context loads where
/// they apply); MARKET ANALYSIS; ANALYSIS; and on a continuity run PRIOR
/// THESIS verbatim with any split-context line — each section explained once
/// and then its values, with no instruction in it. Part 2 is the task only:
/// what the document covers, in order, each item naming the Part 1 section it
/// draws on and restating no value. The model receives data, never a
/// description of the app that produced it; the investor profile and the
/// position's economics are absent — the intrinsic verdict is of no investor
/// (`docs/portfolio-analysis.md` §Intrinsic verdict).
pub fn thesis_user_prompt(input: &ThesisInput) -> String {
    let d = input.dossier;
    let e = input.engine;
    let is_fund = dossier_is_fund(d);
    let mut p = String::new();

    p.push_str("======== PART 1: INPUTS ========\n");

    // HOLDING
    p.push_str(&holding_header(d));

    // FETCHED VALUES
    p.push_str(&fetched_values_section(d, input.rates));

    // COMPUTED — one heading, labelled sub-blocks, running to MARKET ANALYSIS.
    p.push_str(
        "\nCOMPUTED\nThe computed reads follow under their labels, up to MARKET \
         ANALYSIS; each is derived from the fetched data by fixed formulas.\n",
    );

    // FUND (fund only): the class line, the exposure tilt with the closed-end
    // line and the positioning line, the evidence gaps as data statements.
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

    // METRICS — the values, each with its unit, under the statement-basis line.
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

    // SCORES
    p.push_str(&format!(
        "\nSCORES\nFour scores from 0 to 100, higher is better on every axis: quality; \
         valuation, where higher means more attractive; momentum; risk, where higher means \
         more resilient. The grade is a letter derived from the quality, valuation and risk \
         scores.\n\
         quality {:.0}, valuation {:.0}, momentum {:.0}, risk {:.0}. Grade {}.{} Risk tier: {}.\n",
        e.sub_scores.quality,
        e.sub_scores.valuation,
        e.sub_scores.momentum,
        e.sub_scores.risk,
        e.grade.as_str(),
        if e.low_confidence_grade { " One score is imputed." } else { "" },
        e.risk_tier.as_str(),
    ));

    // PRICE BANDS — the three legs, each with its method clause so the
    // three-year leg is weighed as the extrapolation it is
    // (`docs/portfolio-analysis.md` §Starting parameters).
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

    // CAPITAL EFFICIENCY — the three tested total returns, the hurdle rate and
    // the three-state read.
    p.push_str(&hurdle_read_section(&e.hurdle));

    p.push_str(&narrative_prompt_section(input.narrative));
    if let Some(overlay) = input.pre_profit {
        p.push_str(&pre_profit_prompt_section(overlay, PromptStage::Thesis));
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

    // MARKET ANALYSIS
    p.push_str(&market_analysis_section(d));

    // ANALYSIS — this run's analysis, Step 6d's consolidation of the
    // write-ups, or the prior record carried, dated and bridged.
    p.push_str(&analysis_section(d, &input.analysis));

    // PRIOR THESIS, on a continuity run.
    let has_prior = d.prior_verdict.as_ref().and_then(|v| v.thesis_document()).is_some();
    p.push_str(&prior_thesis_section(d, input.prior_split));

    // PART 2
    p.push_str(&thesis_task_section(has_prior));
    p
}

/// CAPITAL EFFICIENCY on the thesis message: the three tested twelve-month
/// total returns, the hurdle rate and the three-state read, with one gloss. An
/// unscorable read says no assessment exists this run.
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
/// §Starting parameters): each of the four tests fired, clear or unevaluable
/// naming its missing input — never clear on a gap — with the inputs it read.
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

/// PRIOR THESIS on a continuity run: the prior run's thesis document verbatim
/// under its date, with the split-context line above it where a split re-based
/// the price series since it was written, or where its basis could not be
/// verified this run (`docs/portfolio-workflow.md` §Step 6b) — the document is
/// never rewritten. Empty on a debut, and on a prior that carries no document.
fn prior_thesis_section(d: &HoldingDossier, split: Option<SplitContext>) -> String {
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

/// PRIOR ANALYSIS on a continuity run: the prior analysis verbatim under the
/// date its record carries, with the split-context line its own anchor bar
/// yields against this run's series (`docs/portfolio-workflow.md` §Step 6c,
/// §Step 6d) — the holding's research memory, never rewritten and never
/// distilled, so a carried analysis keeps its basis whatever the verdict's.
/// Empty on a debut, and on a prior that carries none.
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
/// §Step 6d, §Step 6f): this run's analysis — written this session on this
/// run's basis, so undated and with no line — or, where the loop wrote
/// nothing and the prior record stands as this run's, that record under the
/// date it was written with the split-context line its own anchor bar
/// yields, so the model never reads pre-split figures as today's.
fn analysis_section(d: &HoldingDossier, record: &crate::portfolio::AnalysisRecord) -> String {
    let written = (record.written != d.analysis_date).then_some(record.written.as_str());
    document_section("ANALYSIS", written, record_split_context(d, record), &record.text)
}

/// An analysis record's split context against this run's series — its own
/// anchor bar's bridge, read the way the verdict's is: no anchor —
/// comparisons run as stored, no line; an anchor whose bar is missing from
/// the fresh window — unverifiable, the line says so.
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

/// One document rendered verbatim under its heading and, where given, the
/// date it was written, the split-context line above it where a split
/// re-based the price series since it was written, or where its basis could
/// not be verified this run (`docs/portfolio-workflow.md` §Step 6b).
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

/// One leg of a split ratio as the feed states it — whole where whole.
fn fmt_split_leg(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

/// FETCHED VALUES — the holding's fetched data as the providers return it,
/// glossed once and never engine-computed (`docs/portfolio-workflow.md` §Step
/// 6c; the TTM basis, the trailing dividend sum, the short-interest trend and
/// the split bridge are computations and stay out), in the docs' order: the
/// profile line (name, exchange, sector, industry, the quote's size lines), a
/// fund's reported lines with every sector weighting and its largest
/// countries, the quarterly statements' headline lines for the latest eight
/// quarters, the forward consensus, the latest dividend payments, the quote
/// with its served 52-week range, the close on the prior analysis's date and
/// the dated closes three, twelve and thirty-six months back, the 8-K filings
/// of the lookback, the short-interest print, the street price targets with
/// their trend, the analyst ratings with the rating actions, FMP's ratings
/// snapshot, the insider and congressional trades, the earnings surprises,
/// the eight TTM ratio lines, owner earnings, enterprise value, the float, any
/// M&A match, the revenue segments, and the Treasury prints. A row whose
/// source is absent is omitted; a cell the provider left empty reads `(gap)`.
/// One renderer, shared by the research brief and both thesis-document
/// branches, byte for byte.
fn fetched_values_section(d: &HoldingDossier, rates: &RateAnchors) -> String {
    use crate::portfolio::evidence::{
        MaRole, PriceTargetWindow, FUND_COUNTRY_WEIGHTS_SHOWN, SURPRISE_QUARTERS,
    };
    let fin = &d.financials;
    let gap = || "(gap)".to_string();
    let money = |v: Option<f64>| v.map(fmt_magnitude).unwrap_or_else(gap);
    let num = |v: Option<f64>| v.map(|x| format!("{x:.2}")).unwrap_or_else(gap);
    let pct = |v: Option<f64>| v.map(|x| format!("{:.1}%", x * 100.0)).unwrap_or_else(gap);
    let count = |v: Option<u64>| v.map(|x| x.to_string()).unwrap_or_else(gap);
    let score = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(gap);
    let text = |v: Option<&str>| v.unwrap_or("(gap)").to_string();
    let mut p = String::from(
        "\nFETCHED VALUES\nThe holding's data as its providers return it, each figure as \
         reported (USD; B is billions, M is millions; a yield or a return as a percentage); \
         none is computed, and a cell the provider left empty reads (gap).\n",
    );
    // Profile: the name, the listing identity, and the quote's size lines.
    let mut profile = Vec::new();
    let name = d
        .company_name
        .as_deref()
        .or_else(|| d.fund.as_ref().and_then(|f| f.fund.name.as_deref()));
    if let Some(name) = name {
        profile.push(format!("name {name}"));
    }
    if let Some(issuer) = &d.issuer {
        if let Some(x) = issuer.exchange.as_deref() {
            profile.push(format!("exchange {x}"));
        }
        if let Some(x) = issuer.sector.as_deref() {
            profile.push(format!("sector {x}"));
        }
        if let Some(x) = issuer.industry.as_deref() {
            profile.push(format!("industry {x}"));
        }
    }
    if let Some(mc) = fin.market_cap {
        profile.push(format!("market capitalization {}", fmt_magnitude(mc)));
    }
    if let Some(sh) = fin.shares_outstanding {
        profile.push(format!("shares outstanding {}", fmt_magnitude(sh)));
    }
    if !profile.is_empty() {
        p.push_str(&format!("Profile: {}.\n", profile.join("; ")));
    }
    // A fund's reported lines: every sector weighting, the largest countries.
    if let Some(f) = &d.fund {
        let fd = &f.fund;
        let mut lines = Vec::new();
        if let Some(c) = fd.asset_class.as_deref() {
            lines.push(format!("asset class {c}"));
        }
        if let Some(e) = fd.expense_ratio {
            lines.push(format!("expense ratio {}", fmt_expense_ratio(Some(e))));
        }
        if let Some(a) = fd.aum {
            lines.push(format!("assets under management {}", fmt_magnitude(a)));
        }
        if let Some(n) = fd.nav {
            lines.push(format!("NAV {n:.2}"));
        }
        if !lines.is_empty() {
            p.push_str(&format!("Fund: {}.\n", lines.join("; ")));
        }
        let weights = |label: &str, rows: &[(String, f64)], shown: usize| -> String {
            if rows.is_empty() {
                return String::new();
            }
            let top: Vec<String> = rows
                .iter()
                .take(shown)
                .map(|(l, w)| format!("{l} {:.1}%", w * 100.0))
                .collect();
            format!("{label}: {}.\n", top.join(", "))
        };
        p.push_str(&weights("Sector weights", &fd.sector_weights, usize::MAX));
        // The ten LARGEST countries, whatever order the feed served them in.
        let mut countries = fd.country_weights.clone();
        countries.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        p.push_str(&weights(
            "Country weights",
            &countries,
            FUND_COUNTRY_WEIGHTS_SHOWN,
        ));
    }
    // Quarterly statements — the latest eight as reported, newest first, the
    // cash-flow and balance-sheet lines joined by period end.
    if !fin.quarterly_income.is_empty() {
        p.push_str(
            "Quarterly statements, newest first, as reported — period end: revenue; gross \
             profit; operating income; net income; diluted EPS; diluted shares; operating \
             cash flow; free cash flow; capital expenditure; total debt; total equity; cash \
             and equivalents.\n",
        );
        for row in fin.quarterly_income.iter().take(8) {
            let cf = fin
                .quarterly_cash_flow
                .iter()
                .find(|c| c.period_end == row.period_end);
            let bs = fin
                .quarterly_balance_sheet
                .iter()
                .find(|b| b.period_end == row.period_end);
            p.push_str(&format!(
                "- {}: {}; {}; {}; {}; {}; {}; {}; {}; {}; {}; {}; {}\n",
                row.period_end,
                money(row.revenue),
                money(row.gross_profit),
                money(row.operating_income),
                money(row.net_income),
                num(row.eps_diluted),
                money(row.diluted_shares),
                money(cf.and_then(|c| c.operating_cash_flow)),
                money(cf.and_then(|c| c.free_cash_flow)),
                money(cf.and_then(|c| c.capex)),
                money(bs.and_then(|b| b.total_debt)),
                money(bs.and_then(|b| b.total_equity)),
                money(bs.and_then(|b| b.cash_and_equivalents)),
            ));
        }
    }
    // The published consensus EPS by fiscal period, as reported; the engine's
    // twelve-month blend of these rows is a computation and renders under
    // COMPUTED (`consensus_blend_line`).
    if let Some(c) = &fin.consensus {
        let periods: Vec<String> = c
            .eps_periods
            .iter()
            .map(|r| format!("{} {}", r.period_end, num(r.eps_mid)))
            .collect();
        if !periods.is_empty() {
            p.push_str(&format!(
                "Consensus EPS by fiscal period end, as published: {}.\n",
                periods.join("; ")
            ));
        }
    }
    // The latest dividend payments as reported — the trailing sum is an
    // engine computation and stays out.
    if !fin.recent_dividends.is_empty() {
        let rows: Vec<String> = fin
            .recent_dividends
            .iter()
            .map(|r| match &r.payment_date {
                Some(paid) => format!("{} {} (paid {paid})", r.date, fmt_per_share(r.amount)),
                None => format!("{} {}", r.date, fmt_per_share(r.amount)),
            })
            .collect();
        p.push_str(&format!(
            "Dividends, latest ex-dates first (ex-date, amount per share): {}.\n",
            rows.join("; ")
        ));
    }
    // The quote with its served 52-week range, and the fetched closes.
    if let Some(spot) = fin.current_price.filter(|p| p.is_finite() && *p > 0.0) {
        let mut line = format!("Quote: {spot:.2} per share (the live print, undated)");
        if fin.year_high.is_some() || fin.year_low.is_some() {
            line.push_str(&format!(
                ", 52-week low {} and high {} as served",
                num(fin.year_low),
                num(fin.year_high)
            ));
        }
        line.push_str(".\n");
        p.push_str(&line);
    }
    let closes = &fin.daily_closes;
    if let (Some(first), Some(last)) = (closes.first(), closes.last()) {
        let run = chrono::NaiveDate::parse_from_str(&d.analysis_date, "%Y-%m-%d").ok();
        let mut line = format!(
            "Daily closes: {} sessions from {} to {}",
            closes.len(),
            first.date,
            last.date
        );
        // The close on the prior analysis's date — the prior verdict's effective
        // vintage as an ET session, the date PRIOR THESIS is written under —
        // from today's fetched series (today's basis, so a split since needs no
        // bridge here). The anchor bar is provenance for the bridge, not this
        // date: it precedes the session and carries across an abstention.
        let prior_date = d.prior_vintage.as_deref().and_then(|v| {
            crate::market_clock::et_date_of(v).map(|day| day.format("%Y-%m-%d").to_string())
        });
        if let Some(prior_date) = prior_date {
            if let Some(c) = closes.iter().rev().find(|c| c.date <= prior_date) {
                line.push_str(&format!(
                    "; close on the prior analysis date {prior_date}: {:.2} ({})",
                    c.value, c.date
                ));
            }
        }
        if let Some(run) = run {
            let mut back = Vec::new();
            for (label, months) in [("three months", 3u32), ("twelve months", 12), ("three years", 36)] {
                let Some(target) = run.checked_sub_months(chrono::Months::new(months)) else {
                    continue;
                };
                let target = target.format("%Y-%m-%d").to_string();
                if let Some(c) = closes.iter().rev().find(|c| c.date <= target) {
                    back.push(format!("{label} back {:.2} ({})", c.value, c.date));
                }
            }
            if !back.is_empty() {
                line.push_str(&format!("; {}", back.join(", ")));
            }
        }
        line.push_str(".\n");
        p.push_str(&line);
    }
    // The 8-K filings of the forensic lookback, off the same submissions read
    // as the item-classified sweep (date and filer-declared items).
    if !d.filings_8k.is_empty() {
        let rows: Vec<String> = d
            .filings_8k
            .iter()
            .map(|f| {
                let items = match &f.items {
                    Some(items) if !items.is_empty() => items.join(", "),
                    Some(_) => "(no items)".to_string(),
                    None => "(items unreadable)".to_string(),
                };
                if f.form == "8-K/A" {
                    format!("{} (8-K/A): {items}", f.filing_date)
                } else {
                    format!("{}: {items}", f.filing_date)
                }
            })
            .collect();
        p.push_str(&format!(
            "8-K filings of the trailing twelve months, newest first (filing date: items): {}.\n",
            rows.join("; ")
        ));
    }
    // The latest short-interest print as the FINRA file reports it (the
    // trend against the prior settlement is a computation under COMPUTED).
    if let Some(si) = &d.short_interest {
        let whole = |v: Option<f64>| v.map(|x| format!("{x:.0}")).unwrap_or_else(gap);
        p.push_str(&format!(
            "Short interest (FINRA, settlement {}): {:.0} shares; prior settlement {}; \
             average daily volume {}; days to cover {}.\n",
            si.settlement_date,
            si.current_short_interest,
            whole(si.previous_short_interest),
            whole(si.average_daily_volume),
            num(si.days_to_cover),
        ));
    }
    // The evidence surface: the street, the insiders, the surprises, the
    // ratio lines, the valuation rows and the float.
    if let Some(e) = &d.evidence {
        let window = |label: &str, w: &PriceTargetWindow| {
            format!(
                "{label} {} targets averaging {}",
                count(w.count),
                num(w.average)
            )
        };
        let trend = |t: &crate::portfolio::evidence::PriceTargetTrend| {
            format!(
                "published {}, {}, {}",
                window("last month", &t.last_month),
                window("last quarter", &t.last_quarter),
                window("last year", &t.last_year)
            )
        };
        match (&e.price_target, &e.price_target_trend) {
            (Some(t), trend_row) => {
                let mut line = format!(
                    "Street price targets: consensus {}, median {}, low {}, high {}",
                    num(t.consensus),
                    num(t.median),
                    num(t.low),
                    num(t.high)
                );
                if let Some(tr) = trend_row {
                    line.push_str(&format!("; {}", trend(tr)));
                }
                line.push_str(".\n");
                p.push_str(&line);
            }
            (None, Some(tr)) => p.push_str(&format!("Street price targets: {}.\n", trend(tr))),
            (None, None) => {}
        }
        if let Some(g) = &e.grades_consensus {
            p.push_str(&format!(
                "Analyst ratings: strong buy {}, buy {}, hold {}, sell {}, strong sell {}; \
                 consensus {}.\n",
                count(g.strong_buy),
                count(g.buy),
                count(g.hold),
                count(g.sell),
                count(g.strong_sell),
                text(g.consensus.as_deref()),
            ));
        }
        if !e.rating_actions.is_empty() {
            let rows: Vec<String> = e
                .rating_actions
                .iter()
                .map(|a| {
                    format!(
                        "{}: {}, {} to {}, {}",
                        a.date,
                        a.firm,
                        text(a.previous_grade.as_deref()),
                        text(a.new_grade.as_deref()),
                        text(a.action.as_deref())
                    )
                })
                .collect();
            p.push_str(&format!(
                "Rating actions, newest first (date: firm, previous grade to new grade, \
                 action): {}.\n",
                rows.join("; ")
            ));
        }
        if let Some(r) = &e.ratings_snapshot {
            p.push_str(&format!(
                "FMP rating {} (overall {}; discounted cash flow {}, return on equity {}, \
                 return on assets {}, debt to equity {}, price to earnings {}, price to \
                 book {}).\n",
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
        if !e.insider_trades.is_empty() {
            let rows: Vec<String> = e
                .insider_trades
                .iter()
                .map(|t| {
                    let mut row = format!(
                        "{}: {}, {}, {}, {} shares at {}",
                        t.transaction_date,
                        t.name,
                        text(t.owner_type.as_deref()),
                        text(t.transaction_type.as_deref()),
                        t.shares.map(|x| format!("{x:.0}")).unwrap_or_else(gap),
                        num(t.price)
                    );
                    if let Some(filed) = &t.filing_date {
                        row.push_str(&format!(", filed {filed}"));
                    }
                    row
                })
                .collect();
            p.push_str(&format!(
                "Insider trades, newest first (transaction date: name, role, type, shares at \
                 price, filing date): {}.\n",
                rows.join("; ")
            ));
        }
        if let Some(st) = &e.insider_statistics {
            p.push_str(&format!(
                "Insider statistics, {} Q{}: {} acquiring and {} disposing transactions; {} \
                 shares acquired, {} disposed.\n",
                score(st.year),
                score(st.quarter),
                count(st.acquired_transactions),
                count(st.disposed_transactions),
                st.total_acquired
                    .map(|x| format!("{x:.0}"))
                    .unwrap_or_else(gap),
                st.total_disposed
                    .map(|x| format!("{x:.0}"))
                    .unwrap_or_else(gap),
            ));
        }
        if !e.congressional_trades.is_empty() {
            let rows: Vec<String> = e
                .congressional_trades
                .iter()
                .map(|t| {
                    let mut row = format!(
                        "{}: {}, {}, {}, {}, {}",
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
                })
                .collect();
            p.push_str(&format!(
                "Congressional trades, newest first (transaction date: chamber, member, owner, \
                 type, amount, disclosure date): {}.\n",
                rows.join("; ")
            ));
        }
        let reported: Vec<String> = e
            .reported_earnings()
            .take(SURPRISE_QUARTERS)
            .map(|r| {
                format!(
                    "{}: EPS {} vs {} estimated, revenue {}",
                    r.date,
                    num(r.eps_actual),
                    num(r.eps_estimated),
                    money(r.revenue_actual)
                )
            })
            .collect();
        if !reported.is_empty() {
            p.push_str(&format!(
                "Earnings surprises, newest first (announcement date: EPS actual vs estimate, \
                 revenue actual): {}.\n",
                reported.join("; ")
            ));
        }
        if let Some(next) = e.next_earnings(&d.analysis_date) {
            p.push_str(&format!(
                "Next earnings: {} (EPS estimate {}).\n",
                next.date,
                num(next.eps_estimated)
            ));
        }
        if !e.ratios.is_empty() {
            let r = &e.ratios;
            p.push_str(&format!(
                "Trailing-twelve-month ratios: P/E {}; EV/EBITDA {}; EV/sales {}; P/B {}; FCF \
                 yield {}; ROIC {}; ROE {}; net debt/EBITDA {}.\n",
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
            p.push_str(&format!(
                "Owner earnings ({}, period end {}): {}; {} per share.\n",
                o.period.as_deref().unwrap_or("latest period"),
                o.period_end,
                money(o.owners_earnings),
                num(o.per_share)
            ));
        }
        if let Some(ev) = &e.enterprise_value {
            p.push_str(&format!(
                "Enterprise value ({}): {}; market capitalization {}; total debt {}; cash {}.\n",
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
            p.push_str(&format!(
                "Float{dated}: float shares {}; shares outstanding {}; free float {}.\n",
                money(f.float_shares),
                money(f.outstanding_shares),
                f.free_float_percent
                    .map(|x| format!("{x:.1}%"))
                    .unwrap_or_else(gap)
            ));
        }
    }
    // The holding's matches against the run-level M&A feed.
    if !d.ma_matches.is_empty() {
        let rows: Vec<String> = d
            .ma_matches
            .iter()
            .map(|m| {
                let role = match m.role {
                    MaRole::Acquirer => "acquirer of",
                    MaRole::Target => "target of",
                };
                let mut row = format!("{role} {}, announced {}", m.counterparty, m.date);
                if let Some(link) = &m.link {
                    row.push_str(&format!(" ({link})"));
                }
                row
            })
            .collect();
        p.push_str(&format!(
            "M&A (the market-wide feed, trailing twelve months): {}.\n",
            rows.join("; ")
        ));
    }
    // The revenue segments by product and by geography, the latest fiscal
    // years as reported.
    if let Some(e) = &d.evidence {
        let segments = |label: &str, years: &[crate::portfolio::evidence::SegmentYear]| -> String {
            if years.is_empty() {
                return String::new();
            }
            let rows: Vec<String> = years
                .iter()
                .map(|y| {
                    let parts: Vec<String> = y
                        .segments
                        .iter()
                        .map(|(name, v)| format!("{name} {}", fmt_magnitude(*v)))
                        .collect();
                    let fy = y
                        .fiscal_year
                        .map(|fy| format!("FY{fy}"))
                        .unwrap_or_else(|| "fiscal year (gap)".to_string());
                    format!("{fy} (period end {}) {}", y.period_end, parts.join(", "))
                })
                .collect();
            format!("{label}, newest fiscal year first: {}.\n", rows.join("; "))
        };
        p.push_str(&segments("Revenue by product", &e.product_segments));
        p.push_str(&segments("Revenue by geography", &e.geographic_segments));
    }
    // The Treasury prints
    let as_of = |d: Option<&String>| d.map(|s| format!(" (as of {s})")).unwrap_or_default();
    p.push_str(&format!(
        "Treasury yields (FRED): 10-year {:.2}%{}, 2-year {:.2}%{}.\n",
        rates.dgs10 * 100.0,
        as_of(rates.dgs10_date.as_ref()),
        rates.dgs2 * 100.0,
        as_of(rates.dgs2_date.as_ref()),
    ));
    p
}

/// A per-share amount as reported — up to four decimals, trailing zeros
/// trimmed past the second (`0.26`, `0.2275`).
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

/// A reported figure in a readable magnitude — billions or millions to one
/// decimal, thousands to the unit, smaller figures to two places — with the
/// sign kept. The FETCHED VALUES gloss names the suffixes once.
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

/// The three-month method as a plain clause: the base prorated from the
/// twelve-month base return, the band read back from the target itself.
fn three_month_method(tm: &crate::portfolio::PriceTarget) -> String {
    let band = if tm.base > 0.0 { (tm.bull / tm.base - 1.0) * 100.0 } else { 0.0 };
    format!(
        "base = the twelve-month base price return prorated to three months; bear and bull = \
         ±{band:.1}% (two standard deviations of daily volatility over 63 sessions, capped at 26%)"
    )
}

/// The three-year method as a plain clause from the typed target inputs: the
/// twelve-month drivers compounded at the rows' growth, or held, at the same
/// multiples — stated as the extrapolation it is.
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

/// Part 2 of the thesis-document message: what the document covers, in
/// output order, each item naming the Part 1 section it draws on and restating
/// no value or unit; the expected prices and the conviction argued in the
/// text, the computed bands evidence and never bounds; the length band stated
/// and never checked (`docs/portfolio-workflow.md` §Step 6f).
fn thesis_task_section(has_prior: bool) -> String {
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
        if has_prior {
            ", and what changed since the prior analysis, drawing on PRIOR THESIS"
        } else {
            ""
        }
    ));
    p.push_str(&format!(
        "\nThe document runs {} to {} words.\n",
        fmt_thousands(THESIS_DOCUMENT_WORDS.0),
        fmt_thousands(THESIS_DOCUMENT_WORDS.1)
    ));
    p
}

/// A count with a thousands separator, as the docs state the length bands
/// ("900–1,800 words").
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

/// The appendix message — the thesis-document conversation's second, non-thinking
/// message (`docs/portfolio-workflow.md` §Step 6f): a transcription of the
/// conviction and the three expected prices as the document states them, null
/// where it states none, closing on the placeholder-only return shape.
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

/// The engine's twelve-month consensus blend as a METRICS line — a computation
/// over the published fiscal-period rows FETCHED VALUES states, and the
/// bands' driver; empty without a consensus.
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

/// The computed metric lines under COMPUTED (`portfolio-v40`): one per series
/// the engine computes for this vehicle kind — the value and its unit gloss.
/// A missing value prints "(gap)". The price and the expense ratio are
/// fetched values, not computed ones: FETCHED VALUES states each once, so
/// neither renders here.
pub(crate) fn computed_metrics_lines(is_fund: bool, metrics: &engine::ComputedMetrics) -> String {
    let mut p = String::new();
    for s in engine::LedgerSeries::ALL
        .iter()
        .copied()
        .filter(|s| s.computable_for(is_fund))
        .filter(|s| !matches!(s, engine::LedgerSeries::Price | engine::LedgerSeries::ExpenseRatio))
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
/// the role, the output names and the two-part shape of the message. The
/// ladder, the one-sentence rationale and the profile tie-break are the
/// message's Part 2; the app's words about arms, evidence and departures are
/// gone (`docs/portfolio-analysis.md` §Portfolio action).
pub fn action_system_prompt() -> String {
    format!(
        "You are an equity analyst deciding the portfolio action for one holding in a \
         portfolio review. {} {}",
        crate::portfolio::action_response_contract(),
        crate::portfolio::TWO_PART_FRAME
    )
}

/// The action message (`portfolio-v71`): Part 1 the inputs, each section
/// explained once and then its values with no instruction in it, in the docs'
/// order — HOLDING; POSITION, the one model-facing packet that sees the
/// position's economics; on a priced holding VERDICT, then COMPUTED as one
/// heading with labelled sub-blocks (the rung a fixed rule gives, the grade,
/// the price bands with the analyst's expected price beside each, the
/// capital-efficiency numbers, then the forensic filings, the overlay's
/// financing legs, the option overlay and the commodity prints where they
/// render); on a `role_risk_only` holding the branch's own sections then
/// VERDICT; on a continuity run PRIOR ACTION with its rationale; SUPPORTED
/// ACTIONS as one data line; the investor profile less its tax row. Part 2
/// the task in output order and the placeholder-only shape. No arm, baseline,
/// stage, seam, validator behaviour, stamp or product name. Tunnel vision is
/// enforced by input isolation: no whole-book field exists here
/// (`docs/portfolio-analysis.md` §Portfolio action; `docs/portfolio-workflow.md`
/// §Step 6f).
pub fn action_user_prompt(input: &ActionInput) -> String {
    let d = input.dossier;
    let mut p = String::from("======== PART 1: INPUTS ========\n");
    p.push_str(&holding_header(d));
    p.push_str(&position_section(d));
    let prior_action = prior_action_section(d);
    match &input.subject {
        ActionSubject::Priced { graded, engine, pre_profit } => {
            p.push_str(&verdict_section(d, Some(&graded.appendix), &graded.thesis_document));
            // COMPUTED — one heading, labelled sub-blocks, running to the next
            // top-level section, which is PRIOR ACTION only on a continuity run.
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
            // The branch's computed surface stays as top-level sections — the
            // thesis message's role/risk form (ruled 2026-10-08).
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
            // The closed-end read, where present — its absence is a named gap in
            // the evidence-gap list below, never a fabricated number.
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
    // The per-holding set as one data line (fix list 3.9, ruled 2026-09-17: no
    // permission sentence — the return shape's enum shows the ladder). An
    // outside-the-set rung persists as authored with the departure on the audit
    // (`outside_set_annotation`), never a bar. Since `portfolio-v49` (ruled
    // 2026-09-27) the line says the list is complete and that an unlisted rung
    // is outside the read; since `portfolio-v71` the rule is named on the line
    // itself, the two-reads preamble that glossed "computed" having gone.
    let set: Vec<&str> = input.engine_set.iter().map(Action::as_kebab).collect();
    p.push_str(&format!(
        "\nSUPPORTED ACTIONS\nThe rungs a fixed rule over the holding's reads supports, listed \
         in full: {}. A rung not listed is outside that rule.\n",
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

/// POSITION — the holding as the account carries it
/// (`docs/portfolio-analysis.md` §Portfolio action): the shares held, the
/// total cost basis, the market value, the unrealized gain or loss as dollars
/// and as a share of the cost basis, and the change in the shares held since
/// the last pull — the quantity move with the shares then and now, never the
/// paid-up / averaged-down read, which stays on the app's surfaces
/// (`docs/portfolio-analysis.md` §Holdings change tracking). The basis follows
/// the card's contract (`docs/portfolio-analysis.md` §Storage and display): an
/// exactly-zero basis is indistinguishable on the wire from an unreported one
/// (the adapter maps a missing `averagePrice` to zero), so it renders as not
/// reported with no gain or loss; a negative netted basis keeps its dollar
/// gain or loss and renders no percentage. The one model-facing packet that
/// sees the position's economics; the thesis message renders none of it.
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

/// A share count for the POSITION lines: whole where it is whole, else to four
/// places with the trailing zeros dropped; a negative quantity reads as short.
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

/// A dollar amount for the POSITION lines: two places and thousands separators,
/// a negative total signed; a non-finite value prints as a gap.
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

/// The current price where one is usable — finite and positive — for the
/// implied-move legs; `None` otherwise (unreachable on a priced holding, the
/// quote floor, so the guard stays defensive).
fn usable_spot(d: &HoldingDossier) -> Option<f64> {
    d.financials.current_price.filter(|s| s.is_finite() && *s > 0.0)
}

/// A price as the action packet prints one: two places, with the move it
/// implies from the current price where one is usable.
fn fmt_price_move(spot: Option<f64>, v: f64) -> String {
    match spot {
        Some(s) => format!("{v:.2} ({:+.1}%)", (v / s - 1.0) * 100.0),
        None => format!("{v:.2}"),
    }
}

/// The engine's own rung as its own read under COMPUTED: the rung a fixed rule
/// gives from the computed reads, a data line and never a recommendation
/// (`docs/portfolio-analysis.md` §Portfolio action). Labelled COMPUTED ACTION
/// rather than ACTION so the sub-block never reads as the output item.
fn engine_rung_section(graded: &GradedVerdict) -> String {
    format!(
        "\nCOMPUTED ACTION\nThe rung a fixed rule gives from the computed reads: {}.\n",
        graded.engine_rung.as_kebab()
    )
}

/// GRADE under COMPUTED: the letter with its derivation glossed once and the
/// imputed-score disclosure where it applies. The sub-scores and the risk tier
/// render on the thesis message alone (ruled 2026-10-08: the docs name the
/// grade for this packet).
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

/// PRICE BANDS under COMPUTED: the three computed legs as prices with the move
/// each implies from the current price, the analyst's expected price from
/// VERDICT beside each horizon (ruled 2026-10-08), and the method clauses the
/// thesis message renders. A band the scenario function could not derive
/// prints "(gap)"; a horizon the document stated no price for prints "none".
/// With no usable current price the prices render without moves (unreachable
/// on a priced holding — the quote floor — so the guard stays defensive).
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

/// VERDICT: an analyst's read of the holding — on a priced holding the
/// conviction and the expected share price at each horizon with the move each
/// implies, a null field as "none", then the thesis document verbatim; on the
/// role/risk branch the document alone. The gloss carries the provenance the
/// two-reads preamble once did, and the packet does not say the read came
/// from the same model (`docs/portfolio-analysis.md` §Portfolio action).
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

/// PRIOR ACTION, rendered only with a prior verdict that carries an action: a
/// model-chosen rung glossed as chosen in the prior analysis, with its
/// rationale less the app's caveat sentence (`investment_sentence`); a
/// rule-demoted rung glossed as set by rule after the prior analysis, with no
/// rationale, since that rationale argued the rung the rule replaced
/// (`docs/portfolio-analysis.md` §Portfolio action; ruled 2026-10-08). An
/// abstained or not-rated prior renders nothing.
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

/// Part 2 of the action message: the two items in output order, each naming
/// the Part 1 sections it draws on — the weighing order as a task clause with
/// POSITION in the refining list (ruled 2026-10-08), the profile tie-break, on
/// a priced holding the sunk-cost rule as one clause on every packet naming
/// its sub-block under COMPUTED (`docs/portfolio-analysis.md` §Portfolio
/// action), with a chosen prior the firmness clause — and the
/// placeholder-only shape.
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

/// Which packet a shared section is rendered into. The facts are the same on
/// both; the consequence lines (the overlay's and the forensic sweep's) render
/// on the thesis-document message only — the action packet's SUPPORTED
/// ACTIONS line already carries the narrowed set (ruled 2026-09-17, F1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptStage {
    /// The thesis-document message (Step 6f): authors the document, no action.
    Thesis,
    /// The per-holding action call: authors the rung, no document.
    Action,
}

/// Render the finalized pre-profit execution / financing overlay for an eligible
/// stock's interpretation prompt and its per-holding action prompt
/// (`docs/portfolio-workflow.md` §Step 6f): the computed states as data on both,
/// the consequence lines on the interpretation packet only (see [`PromptStage`]).
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
    // The execution leg has no producer and renders nothing: the financing,
    // economics and dilution legs are the overlay's rendered evidence
    // (`docs/portfolio-workflow.md` §Step 6f).
    p.push_str(&format!(
        "- severe deterioration: {}\n",
        if o.severe_deterioration { "YES" } else { "no" }
    ));
    // The consequence lines state the computed rule's effect; the action packet
    // carries the narrowed set on its own line, so they render on the
    // interpretation packet only (ruled 2026-09-17, F1).
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

/// The role/risk verdict assembled from a fresh thesis document: the readout's
/// computed surface app-stamped, the document as authored, the action fields
/// placeholders the per-holding action call overwrites (never rendered into
/// that call's prompt). One assembly serves the pipeline and the
/// fixed-evidence harness, beside [`graded_verdict_from_model_arm`].
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

/// The priced verdict assembled from a fresh model arm: the engine arm's
/// figures app-stamped from the engine output, the thesis document and its
/// appendix persisted exactly as authored (the two-arm contract —
/// `docs/portfolio-analysis.md` §The holding verdict). The action fields are
/// placeholders the per-holding action call overwrites; they are never
/// rendered into that call's prompt. One assembly serves the pipeline and the
/// fixed-evidence harness.
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
        engine_rung,
        authored_band_relation,
    }
}

/// Spot's authoring-time relation to the engine's twelve-month band — the stamp
/// the quick check's band monitor compares against (`docs/portfolio-analysis.md`
/// §The quick check). `None` wherever no band or spot exists: a missing spot
/// stamps nothing rather than guessing, and the caller withholds the band when
/// the split bridge is unresolvable.
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

/// A deterministic, offline [`HoldingAnalyst`] used by `cargo test` and any
/// daemon-free path. It derives a coherent interpretation from the engine's grade
/// (numbers still come from the engine), so the whole pipeline produces a schema-valid
/// verdict with no model call.
pub struct StubAnalyst;

impl HoldingAnalyst for StubAnalyst {
    // Research rides the trait's offline default (no write-up, so consolidation
    // spends no call on a demo or test run); the two consolidation renders
    // below serve a scripted research run deterministically.

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
        // The stub's expected prices: the engine's base values deterministically
        // nudged, so the two arms are distinguishable in tests and demo runs
        // without being random; a horizon the engine carries no band for
        // states no price — the appendix's null, as a document's silence would.
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

    /// Test seam: the retry-once gate without its pause, so a retry test
    /// against a daemon stand-in does not sleep.
    #[cfg(test)]
    fn without_retry_delay(mut self) -> Self {
        self.retry = crate::local_model::RetryOnce::without_delay();
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

/// Decode the appendix message's completion: the schema-valid parse, then the
/// declared domain ([`crate::portfolio::validate_appendix_domain`]). Each
/// failure carries the class the bounded retry-once classifies on — a parse
/// failure `SchemaParse`, an off-domain value `ModelArmDomain` — so the
/// re-issue fires for both and a hard failure's annotation names which one.
/// Runs inside the retry closure, after [`ensure_nonempty_completion`], so an
/// off-domain response gets exactly the one re-issue every content failure
/// gets and never a second retry layer; the object is rejected whole, never
/// clamped.
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
// always explicit — the daemon's memory-dependent auto-size (~256 K on 128 GB)
// over-allocates KV cache, while an unset small default silently front-truncates
// the deterministic packet. Sized to hold packet + thinking budget + output.
/// Distillation: a compact findings condense — small packet, no thinking chain.
/// The fast rung of [`distill_route`]'s issue guard: a distillation prompt that
/// outgrows this context's budget issues on the reasoner at
/// [`NUM_CTX_INTERPRET`] instead of front-truncating here.
pub(super) const NUM_CTX_DISTILL: u32 = 32_768;
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
pub(super) const NUM_PREDICT_THINKING: u32 = 65_536;
/// The appendix message's ceiling: four short fields under the grammar, so a
/// stop at this reservation is a runaway, never a legitimate transcription.
pub(super) const NUM_PREDICT_APPENDIX: u32 = 1_024;
/// Normal distillation ceiling. The response is a potentially wide structured
/// object: combined narrative, per-topic claims and URLs, typed side channels,
/// and bounded observation excerpts. A reservation-bound stop gets one larger
/// retry below; this first ceiling remains the runaway/latency guardrail.
/// Raised from 8,192 after attempt 8 (Finding 3, ruled 2026-09-27): a
/// six-topic stock's ordinary distillation ran to 8,055 tokens, so the old
/// ceiling bound on ordinary work and the wasted first pass cost more than
/// the guardrail protected. 12,288 leaves 1.5× that output, and beside the
/// issue guard's input budget it still fits a 32 K fast-tier context
/// (`distill::input_budget_chars`), so the exact-reservation stop stays the
/// data-health signal for an oversized distillation rather than a routine
/// event.
pub(super) const NUM_PREDICT_DISTILL: u32 = 12_288;
/// One evidence-triggered distillation re-attempt after the normal reservation
/// binds exactly. It issues on the reasoner's 128 K context so the prompt and
/// this full ceiling fit together under the same 60% input sizing guard.
pub(super) const NUM_PREDICT_DISTILL_RETRY: u32 = 32_768;

/// The distill stage's context size, resolved per *model*, not per call: Ollama
/// reloads a resident runner whenever a request's load-time options — `num_ctx`
/// included — differ from the loaded ones, even under `keep_alive: -1`. So when
/// the fast tier fell back to the reasoner (the documented default roster),
/// distillation shares the interpretation context rather than bouncing the 81 GB
/// runner between 32 K and 128 K at every stage transition; the smaller distill
/// context applies only to a genuinely distinct fast model
/// (`docs/local-model-operations.md §The num_ctx trap`).
pub(super) fn distill_num_ctx(fast_model: &str, reasoner_model: &str) -> u32 {
    if fast_model == reasoner_model {
        NUM_CTX_INTERPRET
    } else {
        NUM_CTX_DISTILL
    }
}

/// Where one distillation call issues — the app-side guard against the
/// daemon's silent front-truncation (`docs/local-models.md §The local-model
/// adapter seam`; the 2026-08-24 review's reduce-prompt minor, ruled
/// 2026-08-28). The rendered prompt — the instruction scaffolding and the
/// write-ups it distills together — is measured in chars against its
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
/// the first live run ~45 minutes, F3), non-thinking sampling, **no grammar**
/// (`docs/portfolio-workflow.md` §Step 6d: a distillation returns prose), the
/// caller-routed model and context size ([`distill_route`]). Pure, so the
/// per-stage wiring is asserted offline.
pub(super) fn distill_request(
    model: &str,
    num_ctx: u32,
    num_predict: u32,
    prompt: &distill::DistillPrompt,
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
    req.format_schema = None;
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

/// One distillation call on the live adapter — the [`HoldingAnalyst::distill`]
/// seam: the issue guard sizes the rendered prompt — both messages — against
/// its model's budget before any request exists ([`distill_route`]); a reply
/// that stops exactly at the normal reservation takes the one expanded
/// re-attempt on the reasoner (`docs/local-models.md §The local-model adapter
/// seam`). `expanded_spent` is set the moment that re-attempt issues — on the
/// error path too — so the transport retry gate above never layers a third
/// request after it: the expanded attempt is final for the stage.
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
        // The rendered prompt already passed the reasoner's 60% input guard,
        // leaving more than the 32 K expanded ceiling in its 128 K context.
        // Route the one evidence-triggered re-attempt there even when the
        // normal call used a 32 K fast tier, whose shared context could not
        // hold both.
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

/// Build one research-loop turn's request: thinking on, the shared interpret
/// context (one `num_ctx` per model). The gathering turns carry `tools` with
/// no `format`; the synthesis conversation carries neither — its write-up and
/// its follow-up reply are prose (`docs/portfolio-workflow.md` §Step 6c).
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

/// Build the analysis request (`docs/portfolio-workflow.md` §Step 6d): the
/// resident reasoner, thinking on, **no grammar** — the analysis is prose —
/// thinking sampling, the interpret-sized context, resident.
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

/// Build the thesis-document request (`docs/portfolio-workflow.md` §Step 6f):
/// thinking on, **no grammar** — the document is free prose — thinking
/// sampling, the interpret-sized context.
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

/// Build the appendix request — the same conversation's second message: the
/// system and user messages of the thesis request, the document as the
/// assistant's turn, then the transcription ask; **thinking off**, the
/// appendix grammar ([`crate::portfolio::appendix_schema`]), non-thinking
/// sampling at the shared context.
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

/// Build the `role_risk_only` thesis-document request — the thesis request's
/// wiring on the branch's message: thinking on, no grammar.
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

/// Build the per-holding action request under the chosen engine-set rendering
/// (fix list 3.9; the pipeline passes `List`): thinking on (the rung is a judgment
/// call weighing the whole verdict against the profile), the action schema, and
/// the **shared** interpret context size — the one-`num_ctx`-per-model rule (an
/// Ollama `num_ctx` change reloads the resident runner,
/// `docs/local-model-operations.md §The num_ctx trap`).
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
        // Non-streaming and non-thinking: the issue guard, the expanded
        // re-attempt and the completion checks live in the call. The bounded
        // retry-once gate wraps the issued call like every chat call
        // (`docs/local-models.md §The local-model adapter seam`): a transient
        // first failure re-issues the identical request exactly once, a
        // second failure fails hard annotated with the first attempt's class,
        // and once the expanded re-attempt has issued the stage is final —
        // no third request after it.
        let prompt = distill::distillation_prompt(input);
        let stage = input.stage.as_str();
        let expanded_spent = std::cell::Cell::new(false);
        match distill_prose_call(self, stage, &prompt, &expanded_spent) {
            Ok(text) => Ok(text),
            Err(first) => {
                if expanded_spent.get()
                    || !self.retry.permit(self.client.progress(), stage, &first)
                {
                    return Err(first);
                }
                distill_prose_call(self, stage, &prompt, &expanded_spent).map_err(|second| {
                    second.context(crate::local_model::retried_once_annotation(&first))
                })
            }
        }
    }

    fn analyze(&self, input: &distill::AnalysisInput<'_>) -> Result<String> {
        // Stream step-scoped like the thesis document: the chain lands on this
        // holding's own "Analyze {SYM}" step. The retry gate re-issues the
        // identical request once on a transient class; a length stop is not
        // re-issued.
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
        runner.run_holding_with_issuer(
            &plan.brief,
            &plan.agenda,
            dossier.earnings_issuer.as_ref(),
        )
    }

    fn interpret(&self, input: &ThesisInput) -> Result<PricedModelArm> {
        let symbol = &input.dossier.position.symbol;
        // Stream step-scoped: the reasoning and the document stream onto this
        // holding's own "Analyze {SYM}" step, so the tracker shows live thinking
        // instead of a minutes-long quiet stretch (the first live run's F8).
        let step_key = crate::portfolio::holding_step_key(symbol);
        // The thesis document: the retry gate re-issues the identical request
        // once on a transient class (transport, an empty completion); a length
        // stop is not re-issued.
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
        // The appendix: the same conversation's second message, decoded under
        // the grammar and the declared domain; an off-domain or unparseable
        // object re-issues the identical appendix message once under the same
        // gate (`docs/portfolio-analysis.md` §The holding verdict).
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
    use crate::portfolio::{
        AssetClass, InvestorProfile, OptionsSignal, PositionChange, PositionDelta, PriceTarget,
    };
    use crate::portfolio::dossier::HouseView;
    use crate::schwab::Position;
    use std::collections::HashMap;

    // ---- The 6g what-changed attribution validator ----

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

    /// An action packet with its POSITION block cut out — the gloss and the
    /// values up to the blank line before the next section — so a test can
    /// pin that a repriced position changes nothing else.
    pub(crate) fn without_position(packet: &str) -> String {
        let start = packet.find("\nPOSITION\n").expect("a POSITION block");
        let end = start + 1 + packet[start + 1..].find("\n\n").expect("the block's end");
        format!("{}{}", &packet[..start], &packet[end..])
    }

    /// The shared rate anchors as a static reference — a struct literal's
    /// field can borrow it without a binding.
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

    pub(crate) fn dossier(asset_class: AssetClass, financials: CompanyFinancials) -> HoldingDossier {
        HoldingDossier {
            earnings_issuer: None,
            prior_metrics: None,
            semantic_recall: Default::default(),
            news_seeds: Vec::new(),
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
            prior_pre_profit: None,
            prior_analysis: None,
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
        // The pre-flag is the topic's only trigger (`docs/portfolio-analysis.md`
        // §The per-holding pipeline): a fresh news seed fires nothing on its
        // own — the seed is a lead in the pass brief — and the fired flag adds
        // the topic once.
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

        // The role/risk branch runs the fund agenda's research on the reasoner,
        // then the reasoner's role read + action call: the reasoner alone, once
        // (no distillation call exists until consolidation lands).
        let (v, a) = run(&fund_dossier(bond_fund()));
        assert!(matches!(v.disposition, VerdictDisposition::RoleRiskOnly(_)));
        assert_eq!(a.model_ids, vec!["reasoner".to_string()]);

        // The priced path likewise: research, then the reasoner's
        // interpretation + action call — one id, deduplicated in place.
        let (v, a) = run(&dossier(AssetClass::Stock, strong_financials()));
        assert!(matches!(v.disposition, VerdictDisposition::Priced(_)));
        assert_eq!(a.model_ids, vec!["reasoner".to_string()]);
        // And the priced fund path likewise.
        let (v, a) = run(&fund_dossier(us_equity_fund()));
        assert!(matches!(v.disposition, VerdictDisposition::Priced(_)), "{v:?}");
        assert_eq!(a.model_ids, vec!["reasoner".to_string()]);

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
        // is the first model call and every later judgment runs on the same
        // reasoner, so the recorded ids dedup to the one in place; the fast
        // tier is never called while no distillation issues.
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
        // The FRED anchors feed the scenario targets and the hurdle read, and both
        // thesis-document messages state the prints under FETCHED VALUES — so the
        // priced paths and the role/risk branch name the source. Every earlier
        // exit renders no prompt and computes nothing from them, so its audit must
        // not name them (M3, 2026-08-18).
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
                // The model arm persists exactly as authored: the document as
                // text and the appendix the stub transcribed from it.
                assert!(g.thesis_document.starts_with("Thesis: "), "{}", g.thesis_document);
                assert!(g.appendix.conviction.is_some());
                assert!(g.appendix.expected_price_12m.is_some());
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
        // A net-short equity is a direction the prescriptive layer doesn't model —
        // not-rated with a short-position reason, never graded with long-side
        // semantics (`docs/portfolio-analysis.md` §Asset eligibility).
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
        // A no-research exit outside the floor retains nothing: no research
        // record and no analysis, the prior's included.
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
        // The evidence floor's conflicting-identity arm: a wrong-issuer mapping
        // must never grade the wrong company's financials — and like every
        // abstention, the prior thesis document rides through unrewritten,
        // here from a prior that was itself an abstention carrying one.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        d.listing = Some(ListingResolution::Conflict {
            fmp_name: "Zenith Mining Corp".into(),
        });
        d.prior_verdict = Some(HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
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
        // The prior analysis record is retained whole on the abstention's
        // audit — its own date and anchor with it — the way the anchor bar
        // is, with no research record of its own, so the next continuity run
        // loads it by identity.
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

    /// Write → split and abstain → recover: an abstention retains the prior
    /// document on the basis it was written and carries the document's own
    /// anchor bar, so the next continuity run's bridge reads the split factor
    /// and PRIOR THESIS carries the conversion line beside the pre-split
    /// prices. A fresh stamp on the abstention would have read factor 1 there.
    #[test]
    fn an_abstention_carries_the_retained_documents_anchor_across_a_split() {
        use crate::portfolio::listing::ListingResolution;
        // Run 1 writes the document on the pre-split basis and stamps its anchor.
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
        // The recovering pass re-stamps its own fresh anchor on the new basis.
        let fresh = a3.authoring_close.expect("run 3 stamps its anchor");
        assert_eq!(fresh.date, anchor.date);
        assert!((fresh.value - anchor.value / 4.0).abs() < 1e-9, "{fresh:?}");
    }

    /// Write → unresolvable bridge → recover: a successful pass whose prior
    /// anchor bar is missing from the fetched window records the excluded prior
    /// comparisons as a degraded input, renders PRIOR THESIS under the
    /// unverifiable line, and still stamps its OWN anchor with its quick basis
    /// and band relation — its row is on its own basis — so the next pass, the
    /// bar back in the window, bridges from that anchor at factor 1 and renders
    /// no split line beside a document written on today's basis. (A carried
    /// anchor there would have told the next pass to re-base already-adjusted
    /// prices.)
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
        // B's row is on its own basis: its own anchor, its basis, its band relation.
        let anchor_b = a2.authoring_close.clone().expect("run B stamps its own anchor");
        assert_ne!(anchor_b, anchor_a, "never the carried pre-split anchor");
        assert_eq!(anchor_b.date, "2026-06-30", "the newest bar before the session");
        assert!(a2.quick_basis.is_some(), "the basis persists beneath the pass's own anchor");
        let VerdictDisposition::Priced(g2) = &v2.disposition else { unreachable!() };
        assert!(g2.authored_band_relation.is_some(), "the band relation stamps too");
        let doc_b = g2.thesis_document.clone();
        // Run C: the bar back in the window, the series on B's basis — the
        // bridge from B's anchor is 1 and B's document renders with no line.
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
    fn the_research_brief_carries_prior_analysis_before_prior_thesis_each_dated() {
        // The holding-constant block on a continuity run
        // (`docs/portfolio-workflow.md` §Step 6c): PRIOR ANALYSIS then PRIOR
        // THESIS, each verbatim under the prior vintage's ET session date with
        // the same split-context line; a debut carries neither; the analysis
        // message's PRIOR ANALYSIS section is the same bytes.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        assert_eq!(research_brief(&d, rates_static(), None).prior_documents, "");
        let (v, _) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        d.prior_verdict = Some(v);
        d.prior_vintage = Some("2026-08-03T20:00:00Z".into());
        // The analysis record carries its own date and its own anchor bar: an
        // anchor stored at twice this run's close on the same bar date reads
        // as a 2-for-1 re-basis of the analysis alone, while the verdict's
        // bridge (the thesis document's line) is this run's own.
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
        // An anchor whose bar is missing from the fetched window: the basis is
        // unverifiable and the line says so; no anchor at all: no line.
        d.prior_analysis.as_mut().unwrap().anchor = Some(crate::portfolio::engine::DatedValue {
            date: "1999-01-04".into(),
            value: 1.0,
        });
        assert!(prior_analysis_section(&d).contains("could not be verified this run"), "{}", prior_analysis_section(&d));
        d.prior_analysis.as_mut().unwrap().anchor = None;
        let bare = prior_analysis_section(&d);
        assert!(bare.starts_with("\nPRIOR ANALYSIS (written 2026-08-01)\nThe prior analysis, verbatim.\n"), "{bare}");
        // The thesis-document message never carries PRIOR ANALYSIS — it reads
        // this run's analysis under ANALYSIS and the prior document under
        // PRIOR THESIS (`docs/portfolio-workflow.md` §Step 6f).
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
        });
        assert!(!thesis.contains("PRIOR ANALYSIS"), "{thesis}");
        assert!(thesis.contains("\nANALYSIS\nThis run's analysis.\n") && thesis.contains("\nPRIOR THESIS (written 2026-08-03)\n"), "{thesis}");
        // A prior carrying no analysis renders PRIOR THESIS alone.
        d.prior_analysis = None;
        let docs = research_brief(&d, rates_static(), None).prior_documents;
        assert!(docs.starts_with("\nPRIOR THESIS (written 2026-08-03)\n") && !docs.contains("PRIOR ANALYSIS"), "{docs}");
    }

    /// An analysis record written this session on the test fixture's date,
    /// with no anchor — the tests' shorthand for "this run's analysis".
    fn analysis_record(text: &str) -> crate::portfolio::AnalysisRecord {
        crate::portfolio::AnalysisRecord {
            text: text.into(),
            written: "2026-07-28".into(),
            anchor: None,
        }
    }

    #[test]
    fn a_carried_analysis_renders_dated_and_bridged_under_analysis_on_both_thesis_messages() {
        // A holding whose loop wrote nothing carries the prior analysis as
        // this run's (Step 6d): under ANALYSIS it renders with the date it was
        // written and the split-context line its own anchor bar yields, so
        // the model never reads pre-split figures as today's; a fresh
        // analysis renders undated with no line.
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
        // The role/risk message takes the same render.
        let fund = fund_dossier(bond_fund());
        let role = role_risk_user_prompt(&RoleRiskInput {
            dossier: &fund,
            readout: &RoleRiskReadout::default(),
            rates: rates_static(),
            analysis: carried_on(&fund),
            prior_split: None,
        });
        assert!(
            role.contains("\nANALYSIS (written 2026-07-01)\nA share split since this document was written re-based the price series by a factor of 0.5000: "),
            "{role}"
        );
        assert!(role.contains("The document is as written.\nThe carried analysis.\n"), "{role}");
    }

    #[test]
    fn a_scripted_research_run_persists_the_analysis_and_the_thesis_message_reads_it() {
        // Steps 6c → 6d → 6f end to end on the offline seam: a research loop
        // that writes two write-ups yields an analysis — the stub's render of
        // the write-ups under their titles, within budget so no distillation —
        // persisted on the audit beside the research record, and the
        // thesis-document message carries it under ANALYSIS.
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
        // Written this run: stamped with this run's session date and anchor bar.
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
        // The default seam refuses a stub that never scripted the call, naming
        // the stage, so a double with write-ups must implement it.
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
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
        });
        assert!(!interp.contains("COMMODITY PRICES"), "{interp}");
    }

    /// The research brief's FETCHED VALUES is the thesis-document message's
    /// block to the byte on both branches, so the gathering, synthesis and
    /// thesis messages share one rendering (`docs/portfolio-workflow.md`
    /// §Step 6c); the statement line carries every headline the dossier holds,
    /// diluted shares and capital expenditure among them, as reported.
    /// The full evidence record over the reference bodies' values — every
    /// row FETCHED VALUES renders, so the order pin reads the whole block.
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
        // Step 6c's order, each row as rendered.
        let expected = [
            "Profile: name Apple Inc.; exchange NASDAQ; sector Technology; industry Consumer \
             Electronics; market capitalization 3000.0B; shares outstanding 15.0B.",
            "Quarterly statements, newest first",
            "Consensus EPS by fiscal period end",
            "Dividends, latest ex-dates first (ex-date, amount per share): 2026-08-11 0.26 \
             (paid 2026-08-14); 2026-05-12 0.25.",
            "Quote: 195.00 per share (the live print, undated), 52-week low 170.00 and high \
             260.00 as served.",
            "Daily closes:",
            "8-K filings of the trailing twelve months, newest first (filing date: items): \
             2026-07-31: 2.02, 9.01; 2026-05-02 (8-K/A): (no items).",
            "Short interest (FINRA, settlement 2026-09-30): 5000000 shares; prior settlement \
             4000000; average daily volume 2000000; days to cover 2.50.",
            "Street price targets: consensus 323.82, median 325.00, low 253.00, high 400.00; \
             published last month 3 targets averaging 380.00, last quarter 10 targets \
             averaging 322.60, last year 58 targets averaging 293.43.",
            "Analyst ratings: strong buy 1, buy 69, hold 33, sell 7, strong sell 0; consensus Buy.",
            "Rating actions, newest first (date: firm, previous grade to new grade, action): \
             2026-05-26: B of A Securities, Buy to Buy, maintain.",
            "FMP rating B (overall 3; discounted cash flow 3, return on equity 5, return on \
             assets 5, debt to equity 1, price to earnings 2, price to book 1).",
            "Insider trades, newest first (transaction date: name, role, type, shares at \
             price, filing date): 2026-06-05: BOLDUC JOHN, director, P-Purchase, 3570 shares \
             at 6.77, filed 2026-06-06.",
            "Insider statistics, 2026 Q2: 5 acquiring and 35 disposing transactions; 272855 \
             shares acquired, 880558 disposed.",
            "Congressional trades, newest first (transaction date: chamber, member, owner, \
             type, amount, disclosure date): 2026-04-17: Senate, Shelley Moore Capito, \
             Spouse, Sale, $1,001 - $15,000, disclosed 2026-05-07.",
            "Earnings surprises, newest first (announcement date: EPS actual vs estimate, \
             revenue actual): 2026-07-22: EPS 1.10 vs 1.00 estimated, revenue 90.0B.",
            "Next earnings: 2026-10-29 (EPS estimate 1.20).",
            "Trailing-twelve-month ratios: P/E 32.89; EV/EBITDA 23.42; EV/sales 8.13; P/B \
             47.37; FCF yield 3.1%; ROIC 45.2%; ROE 145.3%; net debt/EBITDA 0.48.",
            "Owner earnings (FY2025 Q1, period end 2024-12-28): 27.7B; 1.83 per share.",
            "Enterprise value (2024-09-28): 3570.0B; market capitalization 3500.0B; total \
             debt 106.6B; cash 29.9B.",
            "Float (2026-06-05): float shares 14.7B; shares outstanding 14.7B; free float 99.8%.",
            "M&A (the market-wide feed, trailing twelve months): target of Global Net Lease, \
             Inc., announced 2026-06-01 (https://sec.gov/x).",
            "Revenue by product, newest fiscal year first: FY2024 (period end 2024-09-28) \
             iPhone 201.2B, Mac 30.0B.",
            "Revenue by geography, newest fiscal year first: FY2024 (period end 2024-09-28) \
             Americas 167.0B.",
            "Treasury yields (FRED):",
        ];
        let mut cursor = 0;
        for line in expected {
            let at = block[cursor..]
                .find(line)
                .unwrap_or_else(|| panic!("missing or out of order: {line}\n{block}"));
            cursor += at + line.len();
        }
        // The computations stay out: the trailing dividend sum, the computed
        // 52-week range over the closes, the short-interest trend percentage.
        assert!(
            !block.contains("over the trailing twelve months"),
            "{block}"
        );
        assert!(
            !block.contains("52-week low 1") || block.contains("low 170.00 and high"),
            "{block}"
        );
        assert!(!block.contains("% vs the prior settlement"), "{block}");
        // The block is the same bytes on the brief and both thesis messages.
        assert_eq!(
            research_brief(&d, rates_static(), None).fetched_values,
            block
        );
    }

    #[test]
    fn fetched_values_omits_absent_rows_and_cuts_a_funds_countries_at_ten() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let block = fetched_values_section(&d, rates_static());
        for absent in [
            "Dividends,",
            "52-week",
            "8-K filings",
            "Short interest",
            "Street price",
            "Analyst ratings",
            "Rating actions",
            "FMP rating",
            "Insider",
            "Congressional",
            "Earnings surprises",
            "Next earnings",
            "Trailing-twelve-month ratios",
            "Owner earnings",
            "Enterprise value",
            "Float",
            "M&A",
            "Revenue by",
        ] {
            assert!(
                !block.contains(absent),
                "{absent} rendered with no source:\n{block}"
            );
        }
        // An empty evidence record renders nothing either.
        let mut d = d;
        d.evidence = Some(crate::portfolio::evidence::CompanyEvidence::empty("AAPL"));
        let block = fetched_values_section(&d, rates_static());
        assert!(!block.contains("Trailing-twelve-month ratios"), "{block}");
        assert!(!block.contains("Street price"), "{block}");
        // A fund: every sector weighting, the ten largest countries.
        // The countries are served smallest first here, so the cut must sort.
        let mut fund = us_equity_fund();
        fund.sector_weights = (0..12).map(|i| (format!("Sector {i}"), 0.05)).collect();
        fund.country_weights = (0..12)
            .map(|i| (format!("Country {i}"), 0.01 * (i + 1) as f64))
            .collect();
        let block = fetched_values_section(&fund_dossier(fund), rates_static());
        assert!(block.contains("Sector 11 5.0%"), "{block}");
        assert!(
            block.contains("Country weights: Country 11 12.0%, Country 10 11.0%"),
            "{block}"
        );
        assert!(block.contains("Country 2 3.0%."), "{block}");
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
            })
            .collect();
        let d = dossier(AssetClass::Stock, fin);
        let engine_output = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        let block = research_brief(&d, rates_static(), None).fetched_values;
        assert!(block.starts_with("\nFETCHED VALUES\n"), "{block}");
        assert!(
            block.contains(
                "period end: revenue; gross profit; operating income; net income; diluted \
                 EPS; diluted shares; operating cash flow; free cash flow; capital \
                 expenditure; total debt; total equity; cash and equivalents.\n\
                 - 2026-06-30: 100.0B; (gap); (gap); (gap); 1.55; 15.0B; 28.0B; 20.0B; \
                 -8.0B; (gap); (gap); (gap)\n"
            ),
            "{block}"
        );
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
        });
        assert!(role.contains(block.as_str()), "{role}");
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
        });
        assert!(role.contains(LINE), "{role}");
        // Absent, neither prompt claims it.
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

        // The audit records the sweep state with the engine-matched hard rule.
        let forensic = audit.forensic.expect("the sweep state persists on the audit");
        assert!(forensic.state.hard_tripped());
        assert!(
            forensic.matched_rule.as_deref().unwrap_or("").contains("reads the exit family"),
            "{forensic:?}"
        );
        // The engine arm is bound: its own rung reads the exit family; the
        // model arm persists as authored (the stub's own conviction survives).
        let crate::portfolio::VerdictDisposition::Priced(graded) = &v.disposition else {
            panic!("expected a priced verdict");
        };
        assert!(matches!(graded.engine_rung, Action::Trim | Action::SellAll), "{:?}", graded.engine_rung);
        // Both prompts render the typed section; the sweep is a consulted source.
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
        // The action packet carries the narrowed set on its own line, so the
        // sweep's rule sentence renders on the interpretation packet only
        // (ruled 2026-09-17, F1).
        assert!(!action.contains("By rule:"), "{action}");
        assert!(
            action.contains("\nSUPPORTED ACTIONS\nThe rungs a fixed rule over the holding's reads supports, listed in full: sell-all, trim, hold. A rung not listed is outside that rule.\n"),
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
        // The rung is the untripped rule's — whatever the hurdle and grade read.
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
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
        // The sections in the docs' order (`docs/portfolio-workflow.md` §Step 6f),
        // each once: POSITION, VERDICT, then one COMPUTED heading with its
        // labelled sub-blocks, SUPPORTED ACTIONS and the profile.
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
        // POSITION: the test position's economics and the debut's change tag.
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
        // The COMPUTED heading names the section it runs to — SUPPORTED ACTIONS
        // on a debut, which carries no PRIOR ACTION.
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
        // Part 1 instructs nothing; Part 2 carries the two items and the shape.
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
        // The computed bands reach the rung with the move each implies, the
        // analyst's expected price beside each horizon (ruled 2026-10-08): the
        // stub authors its twelve-month base at 1.05× the computed base, so the
        // two moves differ on the line.
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
        // The analyst's read is the VERDICT section: its gloss carries the
        // provenance, then the appendix's conviction and expected prices with
        // the move each implies, a null as none, then the thesis document
        // verbatim.
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
        // GRADE carries the letter with its derivation glossed once; the
        // sub-scores, their polarity gloss and the risk tier stay on the thesis
        // message (ruled 2026-10-08). The set once as data with no permission
        // sentence (3.9, ruled 2026-09-17), the rule named on the line.
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
        // No app concept, no whole-book vocabulary, no provenance suffix or
        // two-reads preamble, and — on a debut — no continuity section or
        // firmness clause.
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
        // A rule-demoted prior renders as data with its gloss (ruled 2026-09-17,
        // F4), carries no rationale — its rationale argued the rung the rule
        // replaced (ruled 2026-10-08) — and anchors no firmness clause.
        assert!(
            exempt.contains("\nPRIOR ACTION\nhold, set by rule after the prior analysis, not chosen in it.\n\nSUPPORTED ACTIONS\n"),
            "{exempt}"
        );
        assert!(!exempt.contains("Rationale:"), "{exempt}");
        assert!(!exempt.contains("chosen in the prior analysis."), "{exempt}");
        assert!(!exempt.contains("Move from PRIOR ACTION"), "{exempt}");
        assert!(!exempt.contains("rule-demoted"), "{exempt}");
        // The COMPUTED heading runs to PRIOR ACTION on a continuity run.
        assert!(exempt.contains("under their labels, up to PRIOR ACTION; each"), "{exempt}");
        // No tax row and no caveat under either profile, and the two renders
        // are byte-identical, so the tax posture cannot reach the rung by any
        // route; the position's economics render once, under POSITION alone
        // (ruled 2026-10-08).
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
        // A repriced position changes POSITION's lines and nothing else.
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
        let mut engine = match engine::analyze(&d.financials, &rates()) {
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
            // The sunk-cost rule is one task clause on every priced packet,
            // naming its sub-block under COMPUTED.
            assert_eq!(prompt.matches("Where even the bull case under CAPITAL EFFICIENCY misses the hurdle").count(), 1, "{prompt}");
            // The low-confidence letter is a gloss on the GRADE line.
            assert!(
                prompt.contains(&format!(
                    "\nGRADE\nA letter from A to F, derived from the computed quality, valuation and risk scores.\n{}; one of those scores is imputed, so the letter is low-confidence.\n",
                    low.grade.as_str()
                )),
                "{prompt}"
            );
            // The continuity section: the prior action as chosen with its
            // rationale, and the firmness clause in the task; the retired PRIOR
            // ANALYSIS and CHANGES sections render nowhere — the thesis document
            // carries the continuity read.
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
            // With no hurdle rate there is no assessment to render.
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
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
                    "Rule matched: narrative-vs-reality hype: test rule.\n"
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
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(action.contains("SAME-UNDERLYING OPTION OVERLAY"), "{action}");
        assert!(!action.contains("SHORT INTEREST"), "positioning stays interpretation-side: {action}");
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
        // Attempt-8 Slice A (2026-09-27) moves the synthesis task, its shape's
        // value placeholder and the SUPPORTED ACTIONS line to v49; the retry
        // cause grew as a string, so the trail stays at v15.
        // The prompt read-through (2026-09-28) moves the gathering brief's
        // tool-results legend onto the tool descriptions, states the tier scale's
        // range, conditions item 1 on a shown page, folds the news leads into
        // its fetch clause, names the two tools in the gathering system message
        // and moves the fallible-source clause onto the weighing sentence, the
        // header's subject field reading `trusted on`: v50, the trail unchanged.
        // File 02 of the read-through (2026-09-28) splits the follow-up pass's
        // opening into two sentences, names the FOLLOW-UP question in its items,
        // shortens the claim provenance label to `published` and glosses it,
        // and adds the last-reply line to the countdown: v51, the trail unchanged.
        // File 03 (2026-09-29) names the value `source tier` on every model-facing
        // surface, states the weighing preference by the scales' endpoints and
        // moves the subject-tier relation onto the task: v52, the trail unchanged.
        // The same file then drops that sentence from both calls, the header's
        // fields carrying the relation: v53, the trail unchanged.
        // Then PRIOR FINDINGS takes the CLAIMS SO FAR shape and gloss, the
        // fact-period gloss aligns across surfaces and the continuity clause
        // names its headings: v54, the trail unchanged.
        // File 04 (2026-09-29) gives the disconfirming pass a two-sentence
        // opening naming TOPIC and CLAIMS SO FAR, singular items and the
        // provenance gloss: v55, the trail unchanged.
        // File 05 (2026-09-29) points the reused-pages gloss at web_fetch's
        // header: v56, the trail unchanged.
        // File 06 (2026-09-29) drops the filler words from the agenda's topic
        // questions: v57, the trail unchanged.
        // File 07 (2026-09-29) gives the failed tool results fixed sentences by
        // class and makes the empty search an empty answer: v58, the trail
        // unchanged.
        // File 08 (2026-09-29) restates the synthesis EVIDENCE gloss in the
        // fetch description's shape, naming TOPIC, the fallible-source clause
        // on the weighing sentence, drops the SEARCHING note from the
        // synthesis brief and restates the task's items as plain sentences:
        // v59, the trail unchanged.
        // File 09 (2026-09-29) points the follow-up pass at its headings as
        // "the question under FOLLOW-UP", "the questions under TOPIC" and "the
        // claims under CLAIMS SO FAR", in the synthesis subject and the
        // gathering opening and items, and asks a topic's last pass under the
        // depth cap for no follow-up proposal: v60, the trail unchanged.
        // File 10 (2026-09-29) glosses the disconfirming synthesis's CLAIMS SO
        // FAR fields and names its one question in the EVIDENCE gloss: v61,
        // the trail unchanged.
        // File 11 (2026-09-29) puts the output names — the object's keys — before
        // the two-part frame on every object-returning call's system message,
        // the frame then saying Part 2 defines those outputs; the distillation
        // TOPICS gloss names its claim-line fields, the task opens in the
        // synthesis's words, "one statement per claim", and the claim rules
        // get the CLAIM RULES heading: v62, the trail unchanged.
        // Item 1 of file 11 (2026-09-29) gives every distillation claim line a
        // pass-local id the reply cites as evidence_id, an enum of the shown
        // ids, in place of the copied reference and address: v63, the trail
        // unchanged (the persisted claim carries no reference).
        // The last sweep of file 11 (2026-09-30) names the topic heading's key
        // in the TOPICS gloss and points item 2 at it, splits the claims item
        // into one sentence per rule, states a claim line's fields as one
        // semicolon list in the CLAIMS SO FAR, PRIOR FINDINGS and TOPICS
        // glosses, adds the serial comma to the distillation's other lists,
        // and shows a nullable number's both halves in the shape ("<0|null>"):
        // v64, the trail unchanged.
        // File 12 (2026-09-30) names the current analysis's side "the searches"
        // and "in this analysis" (no "this time"), says the leading indicator
        // "confirms" a driver as the field does and asks for its id alone,
        // binds the one-claim rule to the searched topics where one rides
        // dormant, glosses confidence's referent, states coverage's
        // denominator, and drops item 1's prior-findings clause: v65, the
        // trail at v16 (the persisted indicator loses the model-authored name).
        // File 14 (2026-09-30) opens the TOPICS gloss on one topic and closes
        // CLAIM RULES on the call's own outputs on the tier-1, pass-level and
        // tree-level calls: v66, the trail unchanged.
        // File 15 (2026-09-30) says the pass-level call shows one of the
        // topic's searches and makes its summary item reconcile, pointing at
        // CLAIM RULES: v67, the trail unchanged.
        // The engine arm at three horizons (2026-10-07) prints the three bands
        // and the engine's own rung in place of the stand-in: v68, the trail
        // to checkpoint-v17.
        // Its task 2 (2026-10-07) drops the overlay's guidance-attainment line
        // — the execution leg has no producer — and persists the soft forensic
        // flags on the audit: v69, the trail to checkpoint-v18.
        // Its task 3 (2026-10-07) sweeps the quick check on the two engine
        // monitors alone — the ledger-condition evaluation and the news-seed
        // leg gone; no prompt renders the sweep, so v69 stands, the trail to
        // checkpoint-v19 (the pinned tail sweep loses its condition states).
        // The holding verdict, task 1 (2026-10-07): the model arm becomes the
        // thesis document and its typed appendix — the interpretation grammar,
        // the ledger, the what-changed rows and the self-assessment gone; the
        // action packet reads VERDICT — v70, the trail to checkpoint-v20 (the
        // verdict record's shape).
        // Its task 2 (2026-10-08) gives the action packet the docs' shape —
        // POSITION, VERDICT, one COMPUTED heading with the grade alone and the
        // analyst's price beside each band, PRIOR ACTION with its rationale
        // less the caveat, the suffixes and the two-reads preamble gone: v71,
        // the trail unchanged (no persisted shape moves).
        // The research chain, task 1 (2026-10-08): the loop writes prose —
        // the synthesis returns the write-up under no grammar and its second
        // message the follow-up question or `none`; the brief leads with the
        // holding-constant block (FETCHED VALUES, NEWS LEADS, PRIOR THESIS);
        // the claims layer, PRIOR FINDINGS, CLAIMS SO FAR and the distillation
        // grammar with its typed channels gone — v72, the trail to
        // checkpoint-v21 (the audit's research record: the write-ups and the
        // typed roster).
        // Its task 2 (2026-10-08): consolidation — the analysis call writes
        // the holding's analysis over the write-ups (thinking, no grammar),
        // a distillation first (non-thinking, no grammar) only where that
        // prompt is over budget, in the merged or the per-write-up shape; the
        // brief carries PRIOR ANALYSIS before PRIOR THESIS; ANALYSIS is the
        // analysis — v73, the trail to checkpoint-v22 (the audit's analysis
        // and the record's distillation shape with its call count).
        // Its task 3 (2026-10-08): the endpoint table's remaining pulls —
        // FETCHED VALUES carries the full Step 6c block (the issuer line,
        // the last four dividends, the served 52-week range, the 8-K list,
        // the short-interest print, the street, the insiders, the surprises,
        // the ratio lines, owner earnings, enterprise value, the float, the
        // M&A match, the segments; a fund's full weightings) and the split
        // line names the split — v74, the trail to checkpoint-v23 (the M&A
        // feed pinned on the header); data health's count of the walk's gap
        // moves the archive to format 19.
        assert_eq!(PROMPT_VERSION, "portfolio-v74");
        assert_eq!(
            crate::portfolio::store::CHECKPOINT_FORMAT_VERSION,
            "checkpoint-v23"
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
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        assert!(
            user.contains(&format!("\nPRIOR ACTION\n{}, chosen in the prior analysis.\n", prior_action.as_kebab())),
            "{user}"
        );
        // The prior rationale rides the line less the app's caveat sentence
        // (ruled 2026-10-08): the profile is tax-aware and the position carries
        // a gain, so a prior exit rung persisted with the caveat appended.
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
        // A rogue stub choosing outside the engine set: the choice persists as
        // authored; the departure is app-stamped on the audit (annotate, never
        // bar — the two-arm contract).
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
    fn target_provenance_renders_the_anchored_and_carry_branches() {
        let d = dossier(AssetClass::Stock, strong_financials());
        let mut engine_output = match engine::analyze(&d.financials, &rates()) {
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
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
        });
        // The priced message names the latest report for what it is — a
        // market-level analysis, never by product name — as data with no scope
        // narration (`portfolio-v40`); the outlook item draws on it by that name.
        assert!(
            user.contains("\nMARKET ANALYSIS\nA market-level analysis.\nThesis: risk-off.\n"),
            "{user}"
        );
        assert!(user.contains("from FETCHED VALUES, COMPUTED, ANALYSIS and MARKET ANALYSIS."), "{user}");
        for narration in ["MARKET SIGNAL", "HOUSE VIEW", "never by itself a reason to exit", "scope:"] {
            assert!(!user.contains(narration), "`{narration}` leaked: {user}");
        }
        // Absent, the section is absent (the item's reference stays).
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
        });
        assert!(!bare_user.contains("\nMARKET ANALYSIS\n"), "{bare_user}");

        // The role/risk message renders the same section through the same
        // renderer (`portfolio-v42`): no product name, no scope clause, and the
        // role item draws on it by name.
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
            rates: rates_static(),
            prior_split: None,
                dossier: &d,
                readout: r,
                analysis: analysis_record("No research findings."),
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
        // The role/risk message on the same two-part frame (`portfolio-v41`):
        // its own sections, the reduced set as one data line, no capital
        // efficiency or targets, and its own weighing clause.
        assert!(action.contains("\nCLASS\nclosed-end fund\n"), "{action}");
        assert!(
            action.contains("\nVERDICT\nAn analyst's read of the holding's data and research: the thesis document.\nThesis document:\nRole: an income sleeve.\n"),
            "{action}"
        );
        // POSITION renders on this branch too, and no COMPUTED heading does —
        // the branch's sections stay top-level (ruled 2026-10-08).
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
        // Each band carries its method clause — the three-month its proration,
        // the twelve-month its drivers, the three-year its extrapolation — so
        // each is weighed as what it is.
        assert!(us.contains("\nPRICE BANDS (USD)\n- three-month: bear "), "{us}");
        let twelve = us.find("- twelve-month: bear ").unwrap_or_else(|| panic!("{us}"));
        let line = us[twelve..].lines().next().unwrap();
        // The method is a plain clause from the typed target inputs, never the
        // engine's methodology string with its mechanics and stamp (`portfolio-v40`).
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
            rates: rates_static(),
            prior_split: None,
            dossier: &d,
            readout: &readout,
            analysis: analysis_record("No research findings."),
        });
        // The role/risk message renders the ratio once, as the fund's reported
        // line under FETCHED VALUES; the computed lines never restate it.
        assert!(
            role.contains("\nFund: asset class Equity; expense ratio 0.0003 (0.03%/yr); "),
            "{role}"
        );
        assert_eq!(role.matches("0.0003 (0.03%/yr)").count(), 1, "renders once: {role}");
        assert!(!role.contains("fund expense ratio:"), "{role}");
        assert!(!role.contains("EXPENSE RATIO ("), "{role}");

        let engine_output = match engine::analyze(&strong_financials(), &rates()) {
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
        });
        assert!(interp.contains("\nFUND\nUS share of holdings: 99%.\n"), "{interp}");
        assert!(
            interp.contains("\nFund: asset class Equity; expense ratio 0.0003 (0.03%/yr); "),
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
            rates: rates_static(),
            prior_split: None,
            dossier: &d,
            readout: &readout,
            analysis: analysis_record("No research findings."),
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

        // The synthesis conversation carries neither tools nor a grammar: the
        // write-up and the follow-up reply are prose.
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

        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
            },
        );
        assert_eq!(interpret.think, Some(true));
        assert_eq!(interpret.keep_alive, Some(-1));
        let opts = interpret.options.as_ref().unwrap();
        assert_eq!(opts["num_ctx"], NUM_CTX_INTERPRET);
        assert_eq!(opts["num_predict"], NUM_PREDICT_THINKING, "output reservation");
        assert_eq!(opts["temperature"], 1.0, "thinking-general row");
        assert!(interpret.format_schema.is_none(), "the document is free prose");
        // The appendix: the same conversation continued, thinking off under
        // the nullable grammar, the non-thinking row at the shared context.
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
        // Attempt-8 Finding 3 (ruled 2026-09-27): the normal ceiling sits in
        // the 12,288–16,384 band, and on either distill context the prompt
        // budget the issue guard admits plus the reservation fits the window,
        // so a ceiling-bound stop is a reservation hit, never context
        // exhaustion in disguise; the expanded ceiling fits the reasoner's.
        assert!((12_288..=16_384).contains(&NUM_PREDICT_DISTILL));
        let budget_tokens = |num_ctx: u32| {
            (distill::input_budget_chars(num_ctx) as f64 / distill::CHARS_PER_TOKEN) as u32
        };
        for num_ctx in [NUM_CTX_DISTILL, NUM_CTX_INTERPRET] {
            assert!(budget_tokens(num_ctx) + NUM_PREDICT_DISTILL <= num_ctx, "{num_ctx}");
        }
        assert!(budget_tokens(NUM_CTX_INTERPRET) + NUM_PREDICT_DISTILL_RETRY <= NUM_CTX_INTERPRET);
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
    fn the_analysis_request_is_a_thinking_prose_call_on_the_reasoner() {
        // Step 6d's analysis call: the resident reasoner at the interpretation
        // context, thinking on, no tools, no grammar, resident — the thesis
        // call's wiring over the analysis prompt — under the `analysis {SYM}`
        // stage; the distillation request keeps the seam's non-thinking,
        // grammar-free wiring; the offline stub renders both deterministically.
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
        // One rendered prompt that outgrows even the reasoner's budget.
        let over = distill::input_budget_chars(NUM_CTX_INTERPRET) + 1;
        let prompt = distill::DistillPrompt { system: "s".into(), user: "x".repeat(over) };
        let err = distill_prose_call(&analyst, "distill TEST", &prompt, &std::cell::Cell::new(false))
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("refused before issue"), "{msg}");
        assert!(msg.contains("distill TEST"), "{msg}");
        // Nothing connected: the refusal preceded any request.
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "the daemon stand-in must have accepted nothing"
        );
    }

    /// One merged-shape distillation input over a one-line header, for the
    /// daemon stand-in tests below.
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
        // The retry-once gate on the live distillation call
        // (`docs/local-models.md §The local-model adapter seam`): a daemon
        // error status on the first attempt re-issues the identical request
        // exactly once and records the retry; a second failure fails hard,
        // annotated with the first attempt's class, with no third request.
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
        // A stop at exactly the normal reservation spends the one expanded
        // re-attempt; when that attempt then fails on a transient class, the
        // gate refuses a third request — the expanded attempt is final for
        // the stage, and no transport retry fires past it.
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

    // ---- The engine's realized data + prompt rendering ----------------------------

    #[test]
    fn realized_engine_data_resolves_every_stored_value_at_exact_inequality() {
        // The then-versus-now carrier (`docs/portfolio-workflow.md` §Step 6b):
        // every stored metric, sub-score, grade, band, tier and hurdle value
        // beside this run's, resolved at exact `then != now` — a sub-precision
        // move reads as moved and renders distinguishable, equal values read
        // unmoved — and the parameter boundaries named on the prior's branch.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        // The prior vintage's anchor-session close, in the series before either
        // pass computes, so both passes read one surface.
        d.financials.daily_closes.push(DatedValue { date: "2026-07-29".into(), value: 180.0 });
        let (prior, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-07-29").unwrap();
        let VerdictDisposition::Priced(pg) = &prior.disposition else {
            panic!("expected a priced prior");
        };
        d.prior_metrics = Some(audit.metrics.clone());
        d.prior_vintage = Some("2026-07-29T12:00:00Z".into());
        let current = match engine::analyze(&d.financials, &rates()) {
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
        // Same inputs: nothing moved, every side present, the bands bridged.
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

        // A sub-precision move on one sub-score is a move, and the renderer
        // keeps the pair distinguishable.
        let mut nudged = (*current).clone();
        nudged.sub_scores.quality += 1e-9;
        let moved = read(Some(engine::SCENARIO_TARGET_PARAMETER_VERSION), Some(1.0), &nudged);
        let (name, pair) = moved.sub_scores.iter().find(|(_, p)| p.moved()).unwrap();
        assert_eq!(*name, "quality");
        assert_ne!(pair.then, pair.now);
        assert_eq!(moved.sub_scores.iter().filter(|(_, p)| p.moved()).count(), 1);

        // No certified bridge: the prior band side is withheld, never compared
        // cross-basis; a prior on an older target stamp names the horizons.
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
        // The stamp lives on the engine arm (`docs/portfolio-analysis.md` §The
        // quick check): spot against the twelve-month bear–bull band, `None`
        // with no spot or no band — the caller withholds the band when the
        // split bridge is unresolvable.
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

    /// The thesis conversation's requests are scoped to the vehicle and the
    /// debut / continuity shape: the thesis request free prose under thinking
    /// (no grammar), its system line naming an equity analyst on a stock and an
    /// investment analyst on a fund, its METRICS block the vehicle's own
    /// series; the appendix request the same conversation continued under the
    /// nullable grammar with thinking off, every key the grammar requires
    /// declared in the ask (Finding 2: a contract enforced in one place and
    /// unstated in the other drifts silently); PRIOR THESIS and the continuity
    /// clause only with a prior document; the role/risk request the thesis
    /// wiring with no grammar.
    #[test]
    fn the_thesis_conversation_requests_are_scoped_to_the_vehicle_and_the_debut_shape() {
        let stock = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&stock.financials, &rates()) {
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

        // The appendix: the conversation continued, the grammar's required keys
        // each declared in the ask.
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

        // A continuity run: PRIOR THESIS under the prior's date — the stub's
        // document verbatim — and the continuity clause on the summary item.
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
        // FETCHED VALUES states the close on the prior analysis's date — the
        // vintage's ET session, the date PRIOR THESIS is written under.
        assert!(user.contains("; close on the prior analysis date 2026-08-03: "), "{user}");
        // The twelve-month consensus blend is a computation: it renders under
        // COMPUTED as a METRICS line, while FETCHED VALUES states the published
        // fiscal-period rows alone.
        let (part1, _) = user.split_once("======== PART 2: TASK ========").unwrap();
        let fetched = part1.split("\nFETCHED VALUES\n").nth(1).unwrap().split("\nCOMPUTED\n").next().unwrap();
        assert!(fetched.contains("Consensus EPS by fiscal period end, as published: "), "{fetched}");
        assert!(!fetched.contains("blended"), "{fetched}");
        let metrics = part1.split("\nMETRICS\n").nth(1).unwrap().split("\nSCORES\n").next().unwrap();
        assert!(metrics.contains("- forward consensus, next twelve months blended over "), "{metrics}");

        // A fund: the investment analyst's line and the fund's series — no
        // stock multiple, and the expense ratio under FETCHED VALUES alone.
        let fund = fund_dossier(us_equity_fund());
        let fund_req = thesis_request("qwen", &input(&fund, &engine_output));
        assert!(fund_req.messages[0].content.starts_with("You are an investment analyst writing the thesis document"), "{}", fund_req.messages[0].content);
        let user = &fund_req.messages[1].content;
        let metrics = user.split("\nMETRICS\n").nth(1).unwrap().split("\nSCORES\n").next().unwrap();
        assert!(metrics.contains("- daily realized return volatility: ") && !metrics.contains("price / earnings multiple") && !metrics.contains("fund expense ratio"), "{metrics}");
        assert!(user.contains("; expense ratio 0.0003 (0.03%/yr); "), "{user}");

        // A role/risk prior and an abstained prior that kept its document
        // render PRIOR THESIS the same way; an abstained prior without one
        // renders no section and no continuity clause.
        let prior_of = |disposition: VerdictDisposition| HoldingVerdict {
            symbol: "AAPL".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
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
        });
        assert!(role_req.format_schema.is_none() && role_req.think == Some(true));
        assert_eq!(role_req.messages[0].content, role_risk_system_prompt());
    }

    /// An off-domain appendix is rejected whole under `ModelArmDomain` and an
    /// unparseable body under `SchemaParse`, so the bounded retry-once
    /// re-issues the identical message exactly once and then fails the
    /// holding naming the class; a null-bearing in-domain body decodes as the
    /// document's silence.
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
        // The retry gate over the decode: two off-domain replies are exactly
        // two attempts and a failure, one retry event between them; nothing
        // is clamped into domain.
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

    /// The soft forensic flags render under COMPUTED as typed evidence in all
    /// three states — fired, clear, and unevaluable naming the missing input
    /// — each with the inputs it read; a holding without them renders no
    /// section.
    #[test]
    fn the_soft_forensic_section_renders_fired_clear_and_unevaluable() {
        use crate::portfolio::soft_forensic::{
            LineLeg, NetIncomeVsOperatingCashFlow, ScoreFlag, SoftFlagState, SoftForensicFlags,
            WorkingCapitalBuild,
        };
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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

    use crate::portfolio::pre_profit::{
        MetricKind, ObservationPolarity, ObservationRole, PreProfitObservation,
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

    /// A prior overlay whose history carries guidance/actual pairs in two
    /// distinct periods for one metric — carried history, no read derives
    /// from it.
    fn prior_overlay_with_guidance_history() -> crate::portfolio::pre_profit::PreProfitOverlay {
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
        // The soft forensic flags ride beside it — typed, every missing input
        // unevaluable (the fixture carries no scores and no balance rows).
        let flags = audit.soft_forensic.expect("every stock records its soft flags");
        assert!(matches!(
            flags.altman_z.state,
            crate::portfolio::soft_forensic::SoftFlagState::Unevaluable { .. }
        ));

        // A priced fund records none — the overlay and the flags are stock surface.
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
        let mut d = dossier(AssetClass::Stock, pre_profit_financials());
        d.prior_pre_profit = Some(prior_overlay_with_guidance_history());
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();

        let overlay = audit.pre_profit.expect("overlay rides the audit");
        assert!(overlay.is_eligible());
        assert_eq!(overlay.execution, crate::portfolio::pre_profit::ExecutionLeg::Unscorable);
        // The carried history binds nothing: the execution leg has no producer,
        // the engine caps no conviction, and the model's High persists as
        // authored.
        assert!(overlay.consequences.matched_rules.is_empty());
        let VerdictDisposition::Priced(g) = verdict.disposition else {
            panic!("expected a priced verdict");
        };
        assert_eq!(g.appendix.conviction, Some(Conviction::High));
        // The observation history carried through the run.
        assert_eq!(overlay.observations.len(), 4);

        // The prompt renders the overlay block under the same input the live
        // call builds.
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
        });
        assert!(user.contains("\nPRE-PROFIT EXECUTION AND FINANCING\n"), "{user}");
        // The states render as data, with no arm narration and no binding
        // language aimed at the model (`portfolio-v40`; Codex round 1, finding 1).
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
        let overlay = pre_profit::compute_overlay(&fin, None, vec![]);
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
        // The action packet renders the facts alone — its SUPPORTED ACTIONS
        // line carries the narrowed set (ruled 2026-09-17, F1) — and the facts
        // block is shared byte for byte.
        assert!(!action.contains("computed action set"), "{action}");
        assert!(interp.starts_with(action.as_str()), "{interp}\n{action}");
    }

    #[test]
    fn severe_overlay_binds_the_engine_arm_never_the_model() {
        // Economics deterioration + constrained runway (tiny cash against the
        // burn) → the severe conjunction, statement legs alone. A defiant model
        // lean and conviction persist exactly as authored — no bail, no clamp —
        // while the consequences bind the ENGINE arm's action and stay recorded
        // for the annotation render.
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
        let mut d = dossier(AssetClass::Stock, fin);
        d.prior_pre_profit = Some(prior_overlay_with_guidance_history());
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

        // The overlay section keeps the engine rule factual in both prompts and
        // states the model arm's freedom only where that prompt returns the field.
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
        // The overlay's facts render on the action packet; its consequence
        // lines do not — SUPPORTED ACTIONS carries the narrowed set (ruled
        // 2026-09-17, F1).
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
        d.prior_pre_profit = Some(prior_overlay_with_guidance_history());
        let (verdict, audit) =
            analyze_holding(&StubAnalyst, &d, &rates(), "2026-08-03").unwrap();
        assert!(matches!(
            verdict.disposition,
            VerdictDisposition::InsufficientEvidence { .. }
        ));
        let overlay = audit.pre_profit.expect("overlay survives an abstention");
        assert!(overlay.is_eligible());
        assert_eq!(overlay.observations.len(), 4, "history carried");
        assert!(audit.soft_forensic.is_some(), "the soft flags persist with the abstention");
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
        d.prior_pre_profit = Some(prior_overlay_with_guidance_history());
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
        // The flags take the same posture: recorded, every input missing,
        // nothing inferred clear.
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


    // ---- The 6g core checks and the rendered statement (`portfolio-v45`) --------

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
            },
            engine_set: &engine_set,
            profile: &d.profile,
        });
        // The set once, as one data line with no permission sentence (2.2; 3.9
        // ruled 2026-09-17); the sub-scores' polarity gloss stays on the thesis
        // message (ruled 2026-10-08).
        assert_eq!(user.matches("higher is better on every axis").count(), 0, "{user}");
        assert_eq!(user.matches("\nSUPPORTED ACTIONS\n").count(), 1, "{user}");
        for absent in [
            "The full ladder is yours", "its own pick", "ENGINE SET", "ENGINE ADMISSION FACTS",
            "restriction", "not a bound", "departure", "Its selected action",
        ] {
            assert!(!user.contains(absent), "{absent}: {user}");
        }
        // The position's economics render once, under POSITION (ruled
        // 2026-10-08); the header carries identity and spot only, and the
        // overlay renders structure and ratios, never the held share count.
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
        // The interpretation prompt renders the same unsized overlay since
        // portfolio-v38 (fix list 3.2, the Codex plan review): contracts over
        // coverage had given the held share count away.
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
    fn position_section_renders_the_economics_and_the_change_since_the_last_pull() {
        // POSITION (`portfolio-v71`, ruled 2026-10-08): the shares held, the
        // cost basis, the market value, the unrealized gain or loss as dollars
        // and as a share of the cost basis, and the change tag with the shares
        // then and now — never the paid-up / averaged-down read.
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let values = |d: &HoldingDossier| {
            let s = position_section(d);
            s.lines().last().unwrap().to_string()
        };
        // A gain on a debut (every position is new with no prior snapshot).
        assert_eq!(
            values(&d),
            "Shares held: 100. Cost basis: $14,000.00. Market value: $19,500.00. Unrealized gain: \
             $5,500.00 (+39.3% of the cost basis). Change since the last pull: new."
        );
        // A loss, the position increased since the last pull.
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
        // Decreased, fractional shares, at cost.
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
        // Unchanged; a short reads as short on both counts, and its negative
        // netted basis keeps the dollar gain with no percentage — the card's
        // contract (`docs/portfolio-analysis.md` §Storage and display).
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
        // An exactly-zero basis is an unreported one on the wire (the adapter
        // maps a missing `averagePrice` to zero): not reported, and no gain or
        // loss — never the market value read as a gain.
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
        // The gloss once, above the values.
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
        // The intrinsic verdict is of no investor and no position: POSITION is
        // the action packet's alone (`docs/portfolio-analysis.md` §Intrinsic
        // verdict).
        let d = dossier(AssetClass::Stock, strong_financials());
        let engine_output = match engine::analyze(&d.financials, &rates()) {
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
        // An unreported basis (zero on the wire) has no defined gain, so no
        // caveat; a negative netted basis keeps its dollar gain and the caveat.
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

        // Through the pipeline: the persisted rationale is the model's sentence
        // plus the app's caveat, appended after the rung is fixed.
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
