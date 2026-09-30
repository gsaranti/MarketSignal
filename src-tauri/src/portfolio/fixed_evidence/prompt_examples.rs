//! The docs' rendered prompt examples — `docs/prompts/portfolio/`, one Markdown
//! file per Portfolio Analysis call shape, generated from the code so the set
//! is exact for the current `PROMPT_VERSION` and is regenerated on every stamp
//! bump (ruled 2026-09-27; `docs/prompts/README.md` is the reader's guide).
//!
//! The non-research data is the fixed evidence set's: TSLA for the stock
//! shapes, SPMO for the fund shapes, the synthetic BND for role/risk. Every
//! parsed article or research field is a shape-preserving `[stub: …]`
//! placeholder, and the latest report's sections under MARKET ANALYSIS are
//! stubbed the same way; ids, dates, URLs, hosts and section headers stay, so
//! the tiers and the glosses that read them render as on a run. The
//! continuity shapes carry a hand-written prior — attempt 6's persisted
//! verdict re-dated two weeks before the run, with a prior spot 3% under
//! today's and its anchor bar — since attempt 6 wrote no second run.
//!
//! Regenerate from `src-tauri/` with
//! `MARKET_SIGNAL_PROMPT_EXAMPLES_DIR=../docs/prompts/portfolio cargo test
//! portfolio_prompt_examples_write -- --ignored`; the writer removes files no
//! longer in the set and rewrites only the files whose text changed up to the
//! header's stamp, so an untouched prompt's file is never written and its
//! header keeps the stamp at which it last changed (user rule, 2026-09-28).
//! The non-ignored `portfolio_prompt_examples_render` builds the same set on
//! every gate, so the generator cannot rot beside the prompts it renders.

use super::*;
use crate::local_model::{prompt_material_chars, ChatMessage, ChatRequest};
use crate::portfolio::dossier::HoldingDossier;
use crate::portfolio::research::{self, samples as research_samples};
use crate::portfolio::{distill, ActionSource, HoldingVerdict, PositionChange, PositionDelta, ROLE_RISK_ACTIONS};
use serde_json::Value;
use std::cell::RefCell;

const REASONER: &str = "reasoner";
const FAST: &str = "fast";
const RUN_DATE: &str = "2026-09-16";
const PRIOR_VINTAGE: &str = "2026-09-02T14:00:00Z";
const PRIOR_SESSION: &str = "2026-09-02";
const STUB_DISTILLED_STOCK: &str =
    "[stub: the distilled research — the combined findings across the topics, as the reduce call returned them]";
const STUB_DISTILLED_FUND: &str =
    "[stub: the distilled research — the fund's exposure profile and holdings news, as the reduce call returned them]";
const STUB_SECTIONS: &str = "## Market Signal Thesis\n\n[stub: the latest report's Market Signal Thesis section]\n\n## Investment Strategy\n\n[stub: the latest report's Investment Strategy section]\n";

/// One rendered call: what the file says about it, and the request as the
/// pipeline builds it.
struct Example {
    file: &'static str,
    title: &'static str,
    step: &'static str,
    holding: &'static str,
    sentences: Vec<String>,
    stage: String,
    request: ChatRequest,
    variants: Vec<Variant>,
    extras: Vec<(String, String)>,
}

/// A message variant shown as the lines that differ from the base message.
struct Variant {
    heading: &'static str,
    sentences: Vec<String>,
    diff: String,
}

fn lines(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn stub_house_view(d: &mut HoldingDossier) {
    d.house_view.latest_sections = Some(STUB_SECTIONS.into());
}

fn fixture(symbol: &str) -> Fixture {
    fixtures()
        .into_iter()
        .find(|f| f.symbol == symbol)
        .unwrap_or_else(|| panic!("the {symbol} fixture"))
}

/// A first-analysis dossier over the fixture, the report's sections stubbed.
fn debut_dossier(f: &Fixture) -> HoldingDossier {
    let mut d = dossier_of(f, true);
    stub_house_view(&mut d);
    d
}

fn graded_of(f: &Fixture) -> &crate::portfolio::GradedVerdict {
    match &f.disposition {
        VerdictDisposition::Priced(g) => g,
        other => panic!("{}: every fixed-set holding is priced: {other:?}", f.symbol),
    }
}

/// The priced continuity shape's inputs: the fixture's persisted verdict as
/// the prior (its conditions validated through 6g as on the debut, its prose
/// as persisted), the hand-written prior spot and anchor bar, and the
/// pipeline's own evaluation and rendered delta over them.
struct Continuity {
    dossier: HoldingDossier,
    ledger: ThesisLedger,
    ledger_eval: engine::LedgerEvaluation,
    input_delta: Vec<crate::portfolio::DeltaEntry>,
}

fn continuity_of(f: &Fixture) -> Continuity {
    let mut d = debut_dossier(f);
    let validated = validate_ledger_rewrite_with_research(
        &draft_of(f),
        None,
        None,
        LedgerBranch::Priced,
        f.is_fund,
        None,
        Some(f.spot),
        Some(&f.engine_output.metrics),
        &HashSet::new(),
        true,
        stamps_of(f),
    )
    .0;
    let mut ledger = ledger_of(f);
    ledger.conditions = validated.conditions;
    let prior_spot = (f.spot * 0.97 * 100.0).round() / 100.0;
    let anchor = engine::DatedValue { date: PRIOR_SESSION.into(), value: prior_spot };
    d.financials.daily_closes.insert(0, anchor.clone());
    d.prior_verdict = Some(HoldingVerdict {
        symbol: f.symbol.clone(),
        asset_class: f.asset_class,
        position_change: PositionChange::New,
        disposition: f.disposition.clone(),
        thesis_ledger: Some(ledger.clone()),
        analyzed_at: Some(PRIOR_VINTAGE.into()),
        action_source: ActionSource::ModelChosen,
        side_reversed: false,
    });
    d.prior_vintage = Some(PRIOR_VINTAGE.into());
    d.prior_spot = Some(prior_spot);
    d.prior_authoring_close = Some(anchor);
    d.prior_metrics = Some(f.engine_output.metrics.clone());
    d.prior_grade_parameter_version = Some(engine::GRADE_PARAMETER_VERSION.into());
    d.prior_target_parameter_version = Some(engine::SCENARIO_TARGET_PARAMETER_VERSION.into());
    d.position_delta = PositionDelta {
        change: PositionChange::Unchanged,
        prior_quantity: Some(f.synthetic_position.quantity),
        prior_cost_basis: Some(f.synthetic_position.cost_basis),
    };
    let ledger_eval = engine::evaluate_ledger_conditions_gated(
        &ledger,
        &f.engine_output.metrics,
        &d.financials,
        RUN_DATE,
        |_| true,
    );
    let input_delta = pipeline::priced_input_delta(
        &d,
        &f.engine_output,
        PositionChange::Unchanged,
        Some(&ledger_eval),
        None,
        None,
        false,
        Some(1.0),
    );
    Continuity { dossier: d, ledger, ledger_eval, input_delta }
}

/// A stub that keeps the role/risk request the pipeline builds on a second
/// run — the continuity variant only `analyze_holding` can assemble.
#[derive(Default)]
struct RequestCapture {
    role_risk: RefCell<Option<ChatRequest>>,
}

impl HoldingAnalyst for RequestCapture {
    fn interpret(&self, input: &InterpretationInput) -> anyhow::Result<crate::portfolio::Interpretation> {
        pipeline::StubAnalyst.interpret(input)
    }
    fn interpret_role_risk(
        &self,
        input: &RoleRiskInput,
    ) -> anyhow::Result<crate::portfolio::RoleRiskInterpretation> {
        *self.role_risk.borrow_mut() = Some(pipeline::role_risk_request(REASONER, input));
        pipeline::StubAnalyst.interpret_role_risk(input)
    }
    fn decide_action(&self, input: &ActionInput) -> anyhow::Result<crate::portfolio::ActionDecision> {
        pipeline::StubAnalyst.decide_action(input)
    }
    fn fast_id(&self) -> String {
        FAST.into()
    }
    fn reasoner_id(&self) -> String {
        REASONER.into()
    }
}

fn role_risk_continuity_request(fx: &SyntheticRoleRisk) -> ChatRequest {
    let rates = pipeline::tests::rates();
    let (first, _) = pipeline::analyze_holding(&pipeline::StubAnalyst, &fx.dossier, &rates, "2026-09-03")
        .expect("the stub's first run");
    let mut second = fx.dossier.clone();
    second.prior_verdict = Some(first);
    second.prior_vintage = Some("2026-09-03T14:00:00Z".into());
    second.position_delta = PositionDelta {
        change: PositionChange::Unchanged,
        prior_quantity: Some(10.0),
        prior_cost_basis: Some(730.0),
    };
    let capture = RequestCapture::default();
    let _ = pipeline::analyze_holding(&capture, &second, &rates, "2026-09-17").expect("the second run");
    capture.role_risk.take().expect("the role/risk request was captured")
}

fn research_request(s: &research_samples::Sample) -> ChatRequest {
    let mut messages = vec![ChatMessage::system(s.system.clone()), ChatMessage::user(s.user.clone())];
    messages.extend(s.appended.iter().cloned());
    pipeline::research_turn_request(REASONER, messages, s.tools.as_ref(), s.format.as_ref())
}

fn distill_request(s: &distill::samples::Sample) -> ChatRequest {
    pipeline::distill_request(
        FAST,
        pipeline::distill_num_ctx(FAST, REASONER),
        pipeline::NUM_PREDICT_DISTILL,
        &distill::DistillPrompt { system: s.system.clone(), user: s.user.clone() },
        &s.schema,
    )
}

fn diff_lines(base: &str, variant: &str) -> String {
    let b: Vec<&str> = base.lines().collect();
    let v: Vec<&str> = variant.lines().collect();
    let mut out = String::new();
    for l in &b {
        if !v.contains(l) {
            out.push_str(&format!("- {l}\n"));
        }
    }
    for l in &v {
        if !b.contains(l) {
            out.push_str(&format!("+ {l}\n"));
        }
    }
    if out.is_empty() {
        "(no line differs)\n".into()
    } else {
        out
    }
}

/// The whole set, in pipeline order.
fn examples() -> Vec<Example> {
    let tsla = fixture("TSLA");
    let spmo = fixture("SPMO");
    let mut fx = synthetic_role_risk_fixture();
    stub_house_view(&mut fx.dossier);
    let tsla_debut = debut_dossier(&tsla);
    let brief = pipeline::holding_header(&tsla_debut);
    let fund_brief = pipeline::holding_header(&fx.dossier);
    let agenda = research::build_agenda(&tsla_debut, &research::AgendaTriggers::default());
    let fund_agenda = research::build_agenda(&fx.dossier, &research::AgendaTriggers::default());
    let exposure = fund_agenda
        .iter()
        .find(|t| t.key == "fund-exposure-profile")
        .expect("the fund exposure topic");
    let stock_gathering = research_samples::gathering_messages(
        "TSLA",
        &brief,
        &agenda[0],
        &research_samples::stock_leads(true),
        true,
    );
    let stock_synthesis = research_samples::synthesis_messages("TSLA", &brief, &agenda[0], true);
    let fund_gathering = research_samples::gathering_messages(
        "BND",
        &fund_brief,
        exposure,
        &research_samples::fund_leads(true),
        true,
    );
    let tool_results = research_samples::tool_results(true);
    let distillation = distill::samples::messages(&brief, &fund_brief, true);
    let tsla_holding = "TSLA, a stock of the fixed evidence set (attempt 6, reconstructed), on its first analysis";
    let bnd_holding = "BND, the synthetic total bond market ETF the fixed evidence set carries for the role/risk branch";

    let mut out = Vec::new();

    // ---- Step 6c: the research loop ----
    let gathering_common = [
        "The loop appends the reply countdown before every turn, so the third message is part of the first request.",
        "The model answers with tool calls and writes nothing up; the orchestrator runs each call and feeds the result back as a tool message (the tool-turn file shows the second request).",
    ];
    let g = |i: usize| &stock_gathering[i];
    out.push(Example {
        file: "01-research-gathering-root-pass-first-analysis",
        title: "Research gathering — root pass, first analysis",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            "The first gathering turn of a holding's first research topic on a first analysis.",
            "Part 1 carries the holding header, the topic and the two news leads; Part 2 the search task and its stopping rule.",
            gathering_common[0],
            gathering_common[1],
        ]),
        stage: g(0).stage.clone(),
        request: research_request(g(0)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "02-research-gathering-follow-up-pass",
        title: "Research gathering — follow-up pass",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            "The follow-up pass on the same topic, taken when the root pass's synthesis proposed a question worth one more pass.",
            "The approved question and the topic's claims so far ride in Part 1, so the search starts from what the root pass established.",
            gathering_common[0],
        ]),
        stage: g(1).stage.clone(),
        request: research_request(g(1)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "03-research-gathering-root-pass-continuity",
        title: "Research gathering — root pass on a continuity run",
        step: "6c",
        holding: "TSLA, on a continuity run over a prior analysis of 2026-09-01",
        sentences: lines(&[
            "A root pass on a continuity run: the topic's standing ledger conditions and the prior run's kept findings ride as the seed.",
            "The seed is app-assembled from the prior ledger and the prior claims, never a model call, so the search starts from what is already known and tests it.",
            gathering_common[0],
        ]),
        stage: g(2).stage.clone(),
        request: research_request(g(2)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "04-research-gathering-disconfirming-pass",
        title: "Research gathering — the disconfirming pass",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            "The disconfirming pass, run once per holding after its topics: the run's claims so far are the target and the task is to find what contradicts them.",
            "It carries no follow-up proposal by contract, and its synthesis shape (file 10) drops that key.",
            gathering_common[0],
        ]),
        stage: g(3).stage.clone(),
        request: research_request(g(3)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "05-research-gathering-later-topic-reused-pages",
        title: "Research gathering — a later topic with previously retrieved pages",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            "A later topic's root pass on the same holding: pages fetched under an earlier topic render first, in first-retrieval order, so the model reads what the run already holds before searching again (portfolio-v49).",
            "A reused page counts against no fetch; an explicit re-fetch of the same URL is answered from the run's store.",
            gathering_common[0],
        ]),
        stage: g(4).stage.clone(),
        request: research_request(g(4)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "06-research-gathering-fund-exposure-profile",
        title: "Research gathering — a fund's exposure-profile topic",
        step: "6c",
        holding: bnd_holding,
        sentences: lines(&[
            "The root-pass shape on a fund: the agenda's fund topics replace the stock topics, and the header names the fund.",
            "Everything else — the leads, the countdown, the tools, the stopping rule — is the stock shape.",
        ]),
        stage: fund_gathering[0].stage.clone(),
        request: research_request(&fund_gathering[0]),
        variants: vec![],
        extras: vec![],
    });
    {
        let root = g(0);
        let mut messages = vec![ChatMessage::system(root.system.clone()), ChatMessage::user(root.user.clone())];
        messages.extend(root.appended.iter().cloned());
        messages.push(research_samples::tool_call_turn());
        messages.push(ChatMessage::tool(tool_results[0].1.clone()));
        messages.push(ChatMessage::tool(tool_results[3].1.clone()));
        messages.push(research_samples::countdown(research::MAX_TURNS_PER_PASS - 1));
        let stage = root.stage.replace("turn 1", "turn 2");
        out.push(Example {
            file: "07-research-gathering-tool-turn",
            title: "Research gathering — the second turn, after tool results",
            step: "6c",
            holding: tsla_holding,
            sentences: lines(&[
                "The second gathering request: the first request's messages, then the model's tool calls echoed back as an assistant message, one tool message per call in order, and the next countdown.",
                "The history is never rewritten; each turn appends, and the whole packet is sized against the input guard before issue.",
                "Shown here: a search that returned results and a fetch that served a page; the other results a tool message can carry follow the request — an empty search, a failed search, a thin stub, the five fetch failures by class, and a call the app could not read.",
            ]),
            stage,
            request: pipeline::research_turn_request(REASONER, messages, root.tools.as_ref(), None),
            variants: vec![],
            extras: [1usize, 2, 4, 5, 6, 7, 8, 9, 10]
                .iter()
                .map(|&i| (format!("Tool message — {}", tool_results[i].0), tool_results[i].1.clone()))
                .collect(),
        });
    }
    let s = |i: usize| &stock_synthesis[i];
    let synthesis_common =
        "The synthesis call closes a pass: no tools, the findings grammar as the format, no history — the evidence packet is rebuilt from the run's store, and the model cites sources by the pass-local id the packet shows.";
    out.push(Example {
        file: "08-research-synthesis-root-pass-later-topic",
        title: "Research synthesis — root pass on a later topic",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            synthesis_common,
            "The packet lists the reused page first and this pass's fetch after it (portfolio-v49); what gathering lost is a persisted data-health gap and reaches no model (portfolio-v59).",
        ]),
        stage: s(0).stage.clone(),
        request: research_request(s(0)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "09-research-synthesis-follow-up-pass",
        title: "Research synthesis — follow-up pass, gathering clean",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            synthesis_common,
            "On a follow-up pass the approved question and the prior claims join the packet, and the shape still offers a follow-up proposal.",
        ]),
        stage: s(1).stage.clone(),
        request: research_request(s(1)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "10-research-synthesis-disconfirming-pass",
        title: "Research synthesis — the disconfirming pass",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            synthesis_common,
            "The disconfirming pass's shape carries no follow-up proposal; its findings and claims join the holding's research as the contrary-evidence pass.",
        ]),
        stage: s(2).stage.clone(),
        request: research_request(s(2)),
        variants: vec![],
        extras: vec![],
    });

    // ---- Step 6d: distillation ----
    let distill_common = [
        "Distillation is explicitly non-thinking, grammar-constrained, and issued on the fast tier where the roster has one.".to_string(),
        format!(
            "On the default roster, where the fast tier is the reasoner, the call issues on the reasoner at `num_ctx` {} and the rendered prompt is measured against that budget instead.",
            pipeline::NUM_CTX_INTERPRET
        ),
        format!(
            "A reply that stops exactly at the {}-token reservation is re-issued once on the reasoner with `num_predict` {}, its stage label suffixed `(expanded)`.",
            pipeline::NUM_PREDICT_DISTILL,
            pipeline::NUM_PREDICT_DISTILL_RETRY
        ),
    ];
    let with_common = |own: &[&str]| -> Vec<String> {
        let mut v = lines(own);
        v.extend(distill_common.iter().cloned());
        v
    };
    let d = |i: usize| &distillation[i];
    out.push(Example {
        file: "11-distillation-single-pass-stock-first-analysis",
        title: "Distillation — single pass, stock, first analysis",
        step: "6d",
        holding: tsla_holding,
        sentences: with_common(&[
            "The reduce over every topic's searches at once — the single-pass route, taken when the whole input fits the budget.",
            "On a first analysis there are no standing conditions, no key drivers and no prior topic objects; the typed fields asked for are the forward assumption and the forensic event, read from SOURCE TEXT.",
        ]),
        stage: d(1).stage.clone(),
        request: distill_request(d(1)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "12-distillation-single-pass-stock-continuity",
        title: "Distillation — single pass, stock, continuity run with the overlay and the backfill obligation",
        step: "6d",
        holding: "TSLA, on a continuity run over a prior analysis of 2026-09-01, overlay-eligible",
        sentences: with_common(&[
            "The single-pass reduce on a continuity run: the standing conditions and key drivers render for citation, the prior topic objects merge at their topic, a prior topic not searched this time rides as dormant, and the contrary-evidence pass follows the topics.",
            "The overlay-eligible stock with the backfill obligation asks for every typed field: the forward assumption, the leading indicator, the forensic event, the pre-profit observation rows and the backfill record.",
        ]),
        stage: d(0).stage.clone(),
        request: distill_request(d(0)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "13-distillation-single-pass-fund",
        title: "Distillation — single pass, fund",
        step: "6d",
        holding: bnd_holding,
        sentences: with_common(&[
            "A fund's reduce is consolidation only: the combined findings and the topic layer, with the standing condition rendered for citation, and no source text or typed field.",
        ]),
        stage: d(2).stage.clone(),
        request: distill_request(d(2)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "14-distillation-tier-1-topic-tree",
        title: "Distillation — tier-1 call over one topic tree",
        step: "6d",
        holding: "TSLA, on a continuity run, the hierarchical route",
        sentences: with_common(&[
            "The hierarchical route's tier-1 call, taken per topic when the single-pass prompt outgrows the budget: one topic's searches with its prior, returning that topic's summary and claims.",
        ]),
        stage: d(3).stage.clone(),
        request: distill_request(d(3)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "15-distillation-pass-level",
        title: "Distillation — pass-level sub-distillation",
        step: "6d",
        holding: "TSLA, on a continuity run, the hierarchical route",
        sentences: with_common(&[
            "The pass-level sub-distillation, taken when a topic tree itself outgrows the tier-1 budget: one search of one topic, returning its summary and claims.",
        ]),
        stage: d(4).stage.clone(),
        request: distill_request(d(4)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "16-distillation-tree-level-reduce",
        title: "Distillation — tree-level reduce over the pass outputs",
        step: "6d",
        holding: "TSLA, on a continuity run, the hierarchical route",
        sentences: with_common(&[
            "The tree-level reduce that follows the pass-level calls: each pass output renders as its summary and claims, the prior merges here, and the call returns the topic's summary and claims.",
        ]),
        stage: d(5).stage.clone(),
        request: distill_request(d(5)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "17-distillation-hierarchical-reduce",
        title: "Distillation — the final reduce over the tier-1 outputs",
        step: "6d",
        holding: "TSLA, on a continuity run, the hierarchical route",
        sentences: with_common(&[
            "The hierarchical route's final reduce: the tier-1 outputs stand in for the searches, with the dormant prior and the contrary-evidence pass, returning the combined findings and the topic layer.",
        ]),
        stage: d(6).stage.clone(),
        request: distill_request(d(6)),
        variants: vec![],
        extras: vec![],
    });

    // ---- Step 6f: interpretation ----
    let interpretation_common =
        "The interpretation is a thinking call under the schema grammar: Part 1 the computed evidence, the options read, the research summary and the market analysis; Part 2 the read to return.";
    let continuity_note = "The prior here is attempt 6's persisted verdict re-dated to 2026-09-02, with a hand-written prior spot 3% under today's and its anchor bar; attempt 6 wrote no second run, so the retrospective's figures are illustrative.";
    for (f, file, title, holding, own) in [
        (
            &tsla,
            "18-interpretation-stock-first-analysis",
            "Interpretation — stock, first analysis",
            tsla_holding,
            "On a first analysis the model authors the initial thesis ledger, and the self-assessment is one sentence noting there is no prior read.",
        ),
        (
            &spmo,
            "20-interpretation-fund-first-analysis",
            "Interpretation — priced fund, first analysis",
            "SPMO, an ETF of the fixed evidence set (attempt 6, reconstructed), on its first analysis; the fixture carries no fund context, so the fund-specific sections do not render",
            "A priced fund takes the same call with the fund's metric labels; on a first analysis the model authors the initial thesis ledger.",
        ),
    ] {
        let d = debut_dossier(f);
        let input = InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: None,
            engine: &f.engine_output,
            distilled: STUB_DISTILLED_STOCK,
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        };
        out.push(Example {
            file,
            title,
            step: "6f",
            holding,
            sentences: lines(&[interpretation_common, own]),
            stage: format!("interpret {}", f.symbol),
            request: pipeline::interpret_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
    }
    for (f, file, title, holding, own) in [
        (
            &tsla,
            "19-interpretation-stock-continuity",
            "Interpretation — stock, continuity run",
            "TSLA, on a continuity run over the prior read of 2026-09-02",
            "On a continuity run the prior read and its ledger render as PRIOR ANALYSIS and PRIOR THESIS LEDGER with each quantitative condition's evaluation, the rendered input delta is the what-changed vocabulary, and the schema adds the what-changed rows and the ledger rewrite.",
        ),
        (
            &spmo,
            "21-interpretation-fund-continuity",
            "Interpretation — priced fund, continuity run",
            "SPMO, on a continuity run over the prior read of 2026-09-02; the fixture carries no fund context, so the fund-specific sections do not render",
            "The fund's continuity shape carries the same prior sections and what-changed rows as the stock's.",
        ),
    ] {
        let c = continuity_of(f);
        let input = InterpretationInput {
            input_delta: &c.input_delta,
            dossier: &c.dossier,
            prior_ledger: c.dossier.prior_ledger(),
            engine: &f.engine_output,
            distilled: STUB_DISTILLED_STOCK,
            ledger_eval: Some(&c.ledger_eval),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        };
        out.push(Example {
            file,
            title,
            step: "6f",
            holding,
            sentences: lines(&[interpretation_common, own, continuity_note]),
            stage: format!("interpret {}", f.symbol),
            request: pipeline::interpret_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
    }
    {
        let input = RoleRiskInput {
            input_delta: &[],
            dossier: &fx.dossier,
            prior_ledger: None,
            readout: &fx.readout,
            ledger_eval: None,
            distilled: STUB_DISTILLED_FUND,
        };
        out.push(Example {
            file: "22-role-risk-first-analysis",
            title: "Role/risk interpretation — first analysis",
            step: "6f",
            holding: bnd_holding,
            sentences: lines(&[
                "The role/risk branch of the intrinsic verdict, taken for a vehicle class the engine cannot price: the fund readout stands where the computed scores would, and the model returns the role read and the ledger, never a grade or a target.",
                "The hand-written Treasury positioning line and the venue put/call backdrop render because the synthetic dossier carries them.",
            ]),
            stage: "role-risk BND".into(),
            request: pipeline::role_risk_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
        out.push(Example {
            file: "23-role-risk-continuity",
            title: "Role/risk interpretation — continuity run",
            step: "6f",
            holding: "BND, on a continuity run over a stub first run of 2026-09-03",
            sentences: lines(&[
                "The role/risk call on a continuity run, as the pipeline itself renders it on a second run: the prior read, the prior ledger with each condition's evaluation, the position sentence and the what-changed rows.",
                "The research summary is the offline stub's, since the stub run issues no research call.",
            ]),
            stage: "role-risk BND".into(),
            request: role_risk_continuity_request(&fx),
            variants: vec![],
            extras: vec![],
        });
    }

    // ---- Step 6f: the per-holding action call ----
    let action_common =
        "The action call is the investor profile's one entry point: the finished verdict, the holding's own evidence, the engine's supported set and the profile decide the rung and one rationale — never a comparison with other holdings.";
    {
        let graded = graded_of(&tsla);
        let engine_set = engine::feasible_actions(tsla.engine_output.grade, &tsla.engine_output.hurdle, None, false);
        let ledger = ledger_of(&tsla);
        let make = |d: &HoldingDossier| -> String {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded, engine: &tsla.engine_output, pre_profit: None, ledger: &ledger },
                engine_set: &engine_set,
                changes: None,
                profile: &d.profile,
            })
        };
        let base = pipeline::action_request(
            REASONER,
            &ActionInput {
                dossier: &tsla_debut,
                subject: ActionSubject::Priced { graded, engine: &tsla.engine_output, pre_profit: None, ledger: &ledger },
                engine_set: &engine_set,
                changes: None,
                profile: &tsla_debut.profile,
            },
        );
        let base_user = base.messages[1].content.clone();
        let mut exempt = dossier_of(&tsla, false);
        stub_house_view(&mut exempt);
        let mut costly = debut_dossier(&tsla);
        costly.position.cost_basis *= 3.0;
        out.push(Example {
            file: "24-action-priced-first-analysis",
            title: "Action — priced holding, first analysis",
            step: "6f",
            holding: tsla_holding,
            sentences: lines(&[
                action_common,
                "The packet carries no account economics: the two variants below show which lines a tax-exempt profile and a tripled cost basis change.",
            ]),
            stage: "action TSLA".into(),
            request: base,
            variants: vec![
                Variant {
                    heading: "Variant — tax-exempt profile",
                    sentences: lines(&["The lines that differ from the user message above when the profile is not tax-sensitive."]),
                    diff: diff_lines(&base_user, &make(&exempt)),
                },
                Variant {
                    heading: "Variant — cost basis tripled",
                    sentences: lines(&["The lines that differ from the user message above when the position's cost basis is three times higher."]),
                    diff: diff_lines(&base_user, &make(&costly)),
                },
            ],
            extras: vec![],
        });
    }
    {
        let c = continuity_of(&tsla);
        let input = InterpretationInput {
            input_delta: &c.input_delta,
            dossier: &c.dossier,
            prior_ledger: c.dossier.prior_ledger(),
            engine: &tsla.engine_output,
            distilled: STUB_DISTILLED_STOCK,
            ledger_eval: Some(&c.ledger_eval),
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        };
        let interpretation = pipeline::StubAnalyst.interpret(&input).expect("the stub interprets");
        let changes = pipeline::validate_what_changed(&interpretation.what_changed_entries, c.input_delta.clone());
        let mut graded = graded_of(&tsla).clone();
        graded.what_changed = interpretation.what_changed.clone();
        let engine_set = engine::feasible_actions(tsla.engine_output.grade, &tsla.engine_output.hurdle, None, false);
        let input = ActionInput {
            dossier: &c.dossier,
            subject: ActionSubject::Priced { graded: &graded, engine: &tsla.engine_output, pre_profit: None, ledger: &c.ledger },
            engine_set: &engine_set,
            changes: Some(&changes),
            profile: &c.dossier.profile,
        };
        out.push(Example {
            file: "25-action-priced-continuity",
            title: "Action — priced holding, continuity run",
            step: "6f",
            holding: "TSLA, on a continuity run over the prior read of 2026-09-02",
            sentences: lines(&[
                action_common,
                "On a continuity run the what-changed sentence and the validated what-changed rows join the packet beside the position delta.",
                "The verdict shown is attempt 6's persisted read with this run's what-changed sentence from the offline stub, which re-affirms and authors no rows.",
            ]),
            stage: "action TSLA".into(),
            request: pipeline::action_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
    }
    {
        let (verdict, ledger) = synthetic_role_risk_verdict(&fx);
        let input = ActionInput {
            dossier: &fx.dossier,
            subject: ActionSubject::RoleRisk { verdict: &verdict, ledger: &ledger },
            engine_set: &ROLE_RISK_ACTIONS,
            changes: None,
            profile: &fx.dossier.profile,
        };
        out.push(Example {
            file: "26-action-role-risk",
            title: "Action — role/risk branch",
            step: "6f",
            holding: bnd_holding,
            sentences: lines(&[
                action_common,
                "On the role/risk branch the role read and its ledger stand in for the graded verdict, and the engine set is the reduced ladder (sell-all, trim, hold).",
                "The verdict and ledger are the offline stub's on the synthetic fund, validated through the same 6g seam a run uses.",
            ]),
            stage: "action BND".into(),
            request: pipeline::action_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
    }
    // The files are numbered in pipeline order; the contents table and the
    // render test read that order, not the order the shapes were built in.
    out.sort_by(|a, b| a.file.cmp(b.file));
    out
}

// ---- Rendering ----

/// The page width the fenced message text wraps to, for reading without a
/// horizontal scroll. The model sees the unwrapped text; the `(N chars)`
/// headings count it.
const WRAP_WIDTH: usize = 100;

/// Word-wrap a message for the page: a line past the width breaks at a space,
/// and a space-free run past the width (the shape line's JSON) breaks after
/// its next comma; a run with neither stays whole. Only line breaks are
/// added — no character of the message is changed or dropped.
fn wrap(text: &str) -> String {
    let mut out = String::new();
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        wrap_line(line, &mut out);
    }
    out
}

fn wrap_line(line: &str, out: &mut String) {
    let mut col = 0usize;
    for (i, word) in line.split(' ').enumerate() {
        let width = word.chars().count();
        if i > 0 {
            if col > 0 && col + 1 + width > WRAP_WIDTH {
                out.push('\n');
                col = 0;
            } else {
                out.push(' ');
                col += 1;
            }
        }
        if width <= WRAP_WIDTH {
            out.push_str(word);
            col += width;
            continue;
        }
        for c in word.chars() {
            out.push(c);
            col += 1;
            if c == ',' && col >= WRAP_WIDTH {
                out.push('\n');
                col = 0;
            }
        }
    }
}

fn fence(lang: &str, s: &str) -> String {
    format!("~~~~{lang}\n{}\n~~~~\n\n", s.trim_end())
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("serializable json")
}

fn model_label(id: &str) -> &'static str {
    match id {
        REASONER => "the roster's resident reasoner",
        FAST => "the roster's fast tier where one is configured, else the reasoner (the default roster)",
        _ => "the configured model",
    }
}

fn request_table(ex: &Example) -> String {
    let r = &ex.request;
    let think = match r.think {
        Some(true) => "on (`think: true`)",
        Some(false) => "off (`think: false`)",
        None => "the model's default (`think` omitted)",
    };
    let protocol = match (&r.tools, &r.format_schema) {
        (Some(_), None) => "the two tools below; no `format` grammar",
        (None, Some(_)) => "the JSON schema below as the `format` grammar; no tools",
        (Some(_), Some(_)) => "tools and a `format` grammar",
        (None, None) => "free text: no tools, no grammar",
    };
    let options = r
        .options
        .as_ref()
        .map(|o| serde_json::to_string(o).expect("serializable options"))
        .unwrap_or_else(|| "none".into());
    let keep = r
        .keep_alive
        .map(|k| format!("`keep_alive: {k}` (stays resident)"))
        .unwrap_or_else(|| "the daemon's default".into());
    let chars = prompt_material_chars(&r.messages, r.tools.as_ref());
    format!(
        "| Field | Value |\n| --- | --- |\n| Workflow step | `docs/portfolio-workflow.md` §Step {} |\n| Stage label | `{}` |\n| Model | {} |\n| Thinking | {} |\n| Options | `{}` |\n| Output protocol | {} |\n| Residency | {} |\n| Prompt material | {} chars — the messages and tools as serialized |\n\n",
        ex.step,
        ex.stage,
        model_label(&r.model_id),
        think,
        options,
        protocol,
        keep,
        chars
    )
}

fn render_messages(messages: &[ChatMessage]) -> String {
    let mut out = String::new();
    let mut users = 0;
    for m in messages {
        let n = m.content.chars().count();
        match m.role.as_str() {
            "system" => out.push_str("## System message\n\n"),
            "user" => {
                users += 1;
                if users == 1 {
                    out.push_str(&format!("## User message ({n} chars)\n\n"));
                } else {
                    out.push_str("## Appended user message (the turn countdown)\n\n");
                }
            }
            "assistant" => out.push_str("## Assistant message (the model's tool calls, echoed back)\n\n"),
            "tool" => out.push_str(&format!("## Tool message ({n} chars)\n\n")),
            other => out.push_str(&format!("## {other} message\n\n")),
        }
        if !m.content.is_empty() || m.tool_calls.is_none() {
            out.push_str(&fence("text", &wrap(&m.content)));
        }
        if let Some(calls) = &m.tool_calls {
            out.push_str("`tool_calls`:\n\n");
            out.push_str(&fence("json", &pretty(calls)));
        }
    }
    out
}

fn render(ex: &Example) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", ex.title));
    out.push_str(&format!(
        "*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `{}`; regenerate rather than edit (`docs/prompts/README.md`).*\n\n",
        crate::portfolio::PROMPT_VERSION
    ));
    out.push_str(&format!("Holding: {}.\n", ex.holding));
    for s in &ex.sentences {
        out.push_str(s);
        out.push('\n');
    }
    if ex.request.messages.iter().any(|m| m.content.contains("[stub: ")) {
        out.push_str("Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.\n");
    } else {
        out.push_str("This packet renders no article or research text, so it carries no stub.\n");
    }
    out.push('\n');
    out.push_str("## Request\n\n");
    out.push_str(&request_table(ex));
    out.push_str(&render_messages(&ex.request.messages));
    if let Some(tools) = &ex.request.tools {
        out.push_str("## Tools\n\n");
        out.push_str(&fence("json", &pretty(tools)));
    }
    if let Some(schema) = &ex.request.format_schema {
        out.push_str("## Response schema (`format`)\n\n");
        out.push_str(&fence("json", &pretty(schema)));
    }
    for v in &ex.variants {
        out.push_str(&format!("## {}\n\n", v.heading));
        for s in &v.sentences {
            out.push_str(s);
            out.push('\n');
        }
        out.push('\n');
        out.push_str(&fence("text", &v.diff));
    }
    for (heading, text) in &ex.extras {
        out.push_str(&format!("## {heading}\n\n"));
        out.push_str(&fence("text", &wrap(text)));
    }
    out.trim_end().to_string() + "\n"
}

fn render_contents(examples: &[Example]) -> String {
    let mut out = String::new();
    out.push_str("# Portfolio Analysis prompts — contents\n\n");
    out.push_str(&format!(
        "*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `{}`; regenerate rather than edit (`docs/prompts/README.md`).*\n\n",
        crate::portfolio::PROMPT_VERSION
    ));
    out.push_str("One file per call shape, in pipeline order: the research loop (Step 6c), distillation (Step 6d), then interpretation and the action call (Step 6f).\n");
    out.push_str("Each file carries the request envelope, every message as sent, and the tools or the response schema.\n\n");
    out.push_str("| File | Call | Step |\n| --- | --- | --- |\n");
    for ex in examples {
        out.push_str(&format!("| [{0}]({0}.md) | {1} | {2} |\n", ex.file, ex.title, ex.step));
    }
    out
}

/// A file's text with the header's stamp token replaced by a fixed
/// placeholder, so two renders that differ only in `PROMPT_VERSION` compare
/// equal. The writer then touches only the files whose prompt changed, and a
/// file whose prompt did not move keeps its header, so its stamp reads as the
/// one at which it last changed and a regeneration's diff shows only the
/// prompts that moved (user rule, 2026-09-28). Only the header line is
/// normalized: a stamp cited in an example's own prose is history, not the
/// current stamp.
fn stamp_normalized(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.starts_with("*Generated from the code") {
                if let Some(start) = line.find("`portfolio-v") {
                    if let Some(len) = line[start + 1..].find('`') {
                        return format!("{}`portfolio-vN`{}", &line[..start], &line[start + 1 + len + 1..]);
                    }
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Write `text` to `path` unless the file there already holds the same text up
/// to the header's stamp; returns whether it wrote.
fn write_if_changed(path: &std::path::Path, text: &str) -> bool {
    if let Ok(existing) = std::fs::read_to_string(path) {
        if stamp_normalized(&existing) == stamp_normalized(text) {
            return false;
        }
    }
    std::fs::write(path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    true
}

/// Write the set to `MARKET_SIGNAL_PROMPT_EXAMPLES_DIR`: Markdown files no
/// longer in the set are removed, and each file in the set is written only
/// where its text changed up to the header's stamp (`write_if_changed`).
#[test]
#[ignore = "writes the docs prompt examples to MARKET_SIGNAL_PROMPT_EXAMPLES_DIR"]
fn portfolio_prompt_examples_write() {
    let Ok(dir) = std::env::var("MARKET_SIGNAL_PROMPT_EXAMPLES_DIR") else { return };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).expect("create the examples directory");
    let examples = examples();
    let mut expected: HashSet<String> = examples.iter().map(|ex| format!("{}.md", ex.file)).collect();
    expected.insert("00-contents.md".to_string());
    for entry in std::fs::read_dir(dir).expect("read the examples directory") {
        let path = entry.expect("a directory entry").path();
        let stale = path.extension().is_some_and(|e| e == "md")
            && !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| expected.contains(n));
        if stale {
            std::fs::remove_file(&path).expect("remove an example no longer in the set");
        }
    }
    let mut written = 0usize;
    for ex in &examples {
        if write_if_changed(&dir.join(format!("{}.md", ex.file)), &render(ex)) {
            written += 1;
        }
    }
    if write_if_changed(&dir.join("00-contents.md"), &render_contents(&examples)) {
        written += 1;
    }
    println!(
        "wrote {written} of {} files to {} (the rest unchanged up to the header's stamp)",
        examples.len() + 1,
        dir.display()
    );
}

/// The set builds and renders on every gate: every shape present, every file
/// name unique, a system then a user message on every request, the stubs in
/// place wherever research renders, and none of the samples' hand-written
/// prose leaking past them.
#[test]
fn portfolio_prompt_examples_render() {
    let examples = examples();
    assert_eq!(examples.len(), 26);
    // The header names the stamp this file last changed at, and the writer's
    // comparison sets that stamp aside and nothing else.
    let first = render(&examples[0]);
    assert!(first.contains(&format!("; last changed at `{}`;", crate::portfolio::PROMPT_VERSION)), "{first}");
    let restamped = first.replacen(crate::portfolio::PROMPT_VERSION, "portfolio-v0", 1);
    assert_eq!(stamp_normalized(&first), stamp_normalized(&restamped));
    assert_ne!(stamp_normalized(&first), stamp_normalized(&format!("{first}x")));
    let mut files = HashSet::new();
    for ex in &examples {
        assert!(files.insert(ex.file), "{}: duplicate file name", ex.file);
        assert_eq!(ex.request.messages[0].role, "system", "{}", ex.file);
        assert_eq!(ex.request.messages[1].role, "user", "{}", ex.file);
        assert!(!ex.sentences.is_empty(), "{}: no description", ex.file);
        let text = render(ex);
        if ex.file.contains("action") {
            assert!(text.contains("carries no stub"), "{}: the action packet renders research", ex.file);
        } else {
            assert!(text.contains("[stub: "), "{}: no stub rendered", ex.file);
        }
        for leak in [
            "fourth consecutive month",
            "Sign in to continue reading",
            "Total revenues of $25.5B",
            "11,000 investment-grade",
            "Cybercab production at Giga Texas",
            "Base case: we pivot",
            "Preliminary Evaluation (PE26-014)",
        ] {
            assert!(!text.contains(leak), "{}: hand-written prose leaked: {leak}", ex.file);
        }
    }
    let contents = render_contents(&examples);
    assert_eq!(contents.matches("](").count(), examples.len());
    // The wrap adds line breaks and nothing else: the non-whitespace text is
    // the message's, every original line break survives, and a wrapped line
    // past the width holds no space and no comma past it but the one it
    // broke after.
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    for ex in &examples {
        for m in &ex.request.messages {
            let wrapped = wrap(&m.content);
            assert_eq!(squash(&wrapped), squash(&m.content), "{}: the wrap changed a character", ex.file);
            assert!(wrapped.lines().count() >= m.content.lines().count(), "{}", ex.file);
            for line in wrapped.lines() {
                let width = line.chars().count();
                if width > WRAP_WIDTH {
                    let tail: String = line.chars().skip(WRAP_WIDTH).collect();
                    let inner = tail.strip_suffix(',').unwrap_or(&tail);
                    assert!(!line.contains(' ') && !inner.contains(','), "{}: unwrapped line: {line}", ex.file);
                }
            }
        }
    }
}
