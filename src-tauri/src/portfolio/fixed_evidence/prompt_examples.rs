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
//! the tiers and the glosses that read them render as on a run. The FETCHED
//! VALUES rows the fixed set does not carry — the issuer line, the served
//! 52-week range, the 8-K list, the short-interest print, the street, the
//! insider and congressional trades, the surprises, the ratio lines, owner
//! earnings, enterprise value, the float, the M&A match and the segments —
//! are synthetic values layered onto the stock shapes here alone
//! ([`layer_synthetic_evidence`]; ruled 2026-10-08), their names stubbed, so
//! every row renders in the docs while the fixture JSON and the live
//! admission harness stay untouched. The
//! continuity shapes carry a hand-written prior — attempt 6's persisted
//! verdict, its model arm re-shaped by hand into a thesis document and an
//! appendix, re-dated two weeks before the run, with a prior spot 3% under
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
use crate::portfolio::distill::{self, AnalysisInput, DistillInput, DistillSubject, WriteUp, WriteUps};
use crate::portfolio::dossier::HoldingDossier;
use crate::portfolio::research::{self, samples as research_samples};
use crate::portfolio::{ActionSource, HoldingVerdict, PositionChange, PositionDelta, ROLE_RISK_ACTIONS};
use std::collections::HashSet;
use serde_json::Value;
use std::cell::RefCell;

const REASONER: &str = "reasoner";
const PRIOR_VINTAGE: &str = "2026-09-02T14:00:00Z";
const PRIOR_SESSION: &str = "2026-09-02";
const STUB_ANALYSIS_STOCK: &str =
    "[stub: this run's analysis — the research consolidated across the topics, as the analysis call returned it]";
const STUB_ANALYSIS_FUND: &str =
    "[stub: this run's analysis — the fund's exposure profile and holdings news consolidated, as the analysis call returned it]";
const STUB_PRIOR_ANALYSIS: &str =
    "[stub: the prior run's analysis — the holding's research memory, as the analysis call returned it on 2026-09-02]";
const STUB_REVIEW: &str =
    "[stub: this run's review — the prior position against what happened since, as the review call returned it]";
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

/// A first-analysis dossier over the fixture, the report's sections stubbed
/// and, on a stock, the synthetic evidence rows layered on.
pub(super) fn debut_dossier(f: &Fixture) -> HoldingDossier {
    let mut d = dossier_of(f, true);
    stub_house_view(&mut d);
    if f.asset_class == crate::portfolio::AssetClass::Stock {
        layer_synthetic_evidence(&mut d, f.spot);
    }
    d
}

/// The synthetic FETCHED VALUES rows for the stock shapes (the module docs):
/// round figures sized off the fixture's spot and dated against its
/// 2026-09-16 session, every name a `[stub: …]`, no fabricated URL. The
/// splits are the issuer's real ones, dated before the prior session, so no
/// split-context line renders off them.
fn layer_synthetic_evidence(d: &mut HoldingDossier, spot: f64) {
    use crate::portfolio::evidence::*;
    let round = |x: f64| (x * 100.0).round() / 100.0;
    // The issuer identity — the name the fixture never carried (attempt 6's
    // persisted row held none, so the header read "name unavailable") beside
    // the exchange, sector and industry: identity, not economics, keyed on
    // the one stock the examples render (ruled 2026-10-08).
    if d.position.symbol == "TSLA" {
        d.company_name = Some("Tesla, Inc.".into());
        d.issuer = Some(crate::portfolio::dossier::IssuerProfile {
            exchange: Some("NASDAQ".into()),
            sector: Some("Consumer Cyclical".into()),
            industry: Some("Auto - Manufacturers".into()),
        });
    }
    d.financials.year_high = Some(round(spot * 1.2));
    d.financials.year_low = Some(round(spot * 0.6));
    let filing = |form: &str, date: &str, items: &[&str]| crate::sec::RecentFiling {
        form: form.into(),
        filing_date: date.into(),
        items: Some(items.iter().map(|s| s.to_string()).collect()),
        accession: String::new(),
    };
    d.filings_8k = vec![
        filing("8-K", "2026-07-23", &["2.02", "9.01"]),
        filing("8-K/A", "2026-05-15", &["5.02"]),
        filing("8-K", "2026-04-22", &["2.02", "9.01"]),
    ];
    d.short_interest = Some(crate::finra::ShortInterestRead {
        settlement_date: "2026-08-31".into(),
        current_short_interest: 80_000_000.0,
        previous_short_interest: Some(76_000_000.0),
        average_daily_volume: Some(95_000_000.0),
        days_to_cover: Some(0.84),
    });
    let earnings = |date: &str, actual: Option<f64>, estimate: f64, revenue: Option<f64>| {
        crate::fmp::SymbolEarningsRow {
            date: date.into(),
            eps_actual: actual,
            eps_estimated: Some(estimate),
            revenue_actual: revenue,
        }
    };
    let segments = |fy: i64, end: &str, rows: &[(&str, f64)]| SegmentYear {
        fiscal_year: Some(fy),
        period_end: end.into(),
        segments: rows.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
    };
    d.evidence = Some(CompanyEvidence {
        symbol: d.position.symbol.clone(),
        ratios: RatioLines {
            pe: Some(95.0),
            pb: Some(12.5),
            ev_to_ebitda: Some(55.0),
            ev_to_sales: Some(8.0),
            fcf_yield: Some(0.006),
            roic: Some(0.07),
            roe: Some(0.11),
            net_debt_to_ebitda: Some(-1.2),
        },
        owner_earnings: Some(OwnerEarningsRow {
            period_end: "2026-06-30".into(),
            period: Some("FY2026 Q2".into()),
            owners_earnings: Some(2.1e9),
            per_share: Some(0.62),
        }),
        enterprise_value: Some(EnterpriseValueRow {
            date: "2026-06-30".into(),
            enterprise_value: Some(7.9e11),
            market_cap: Some(7.8e11),
            total_debt: Some(1.3e10),
            cash: Some(3.3e10),
        }),
        price_target: Some(PriceTargetConsensus {
            high: Some(round(spot * 2.0)),
            low: Some(round(spot * 0.5)),
            median: Some(round(spot * 1.1)),
            consensus: Some(round(spot * 1.08)),
        }),
        price_target_trend: Some(PriceTargetTrend {
            last_month: PriceTargetWindow {
                count: Some(4),
                average: Some(round(spot * 1.15)),
            },
            last_quarter: PriceTargetWindow {
                count: Some(15),
                average: Some(round(spot * 1.07)),
            },
            last_year: PriceTargetWindow {
                count: Some(48),
                average: Some(round(spot * 1.0)),
            },
        }),
        grades_consensus: Some(GradesConsensus {
            strong_buy: Some(3),
            buy: Some(14),
            hold: Some(18),
            sell: Some(7),
            strong_sell: Some(2),
            consensus: Some("Hold".into()),
        }),
        rating_actions: vec![
            RatingAction {
                date: "2026-09-10".into(),
                firm: "[stub: grading firm]".into(),
                previous_grade: Some("Hold".into()),
                new_grade: Some("Buy".into()),
                action: Some("upgrade".into()),
            },
            RatingAction {
                date: "2026-07-24".into(),
                firm: "[stub: grading firm]".into(),
                previous_grade: Some("Buy".into()),
                new_grade: Some("Buy".into()),
                action: Some("maintain".into()),
            },
            // A coverage initiation: no previous grade, so the row shows the
            // provider's empty cell (the lexicon scan reads every served word
            // as app prose, so the synthetic actions stay off its list).
            RatingAction {
                date: "2026-04-23".into(),
                firm: "[stub: grading firm]".into(),
                previous_grade: None,
                new_grade: Some("Hold".into()),
                action: Some("initiate".into()),
            },
        ],
        ratings_snapshot: Some(RatingsSnapshot {
            rating: Some("C".into()),
            overall: Some(3),
            discounted_cash_flow: Some(2),
            return_on_equity: Some(3),
            return_on_assets: Some(3),
            debt_to_equity: Some(4),
            price_to_earnings: Some(1),
            price_to_book: Some(1),
        }),
        insider_trades: vec![
            InsiderTrade {
                transaction_date: "2026-09-05".into(),
                filing_date: Some("2026-09-08".into()),
                name: "[stub: insider name]".into(),
                owner_type: Some("officer".into()),
                transaction_type: Some("S-Sale".into()),
                shares: Some(10_000.0),
                price: Some(round(spot * 0.98)),
            },
            InsiderTrade {
                transaction_date: "2026-08-20".into(),
                filing_date: Some("2026-08-21".into()),
                name: "[stub: insider name]".into(),
                owner_type: Some("director".into()),
                transaction_type: Some("A-Award".into()),
                shares: Some(2_500.0),
                price: Some(0.0),
            },
            InsiderTrade {
                transaction_date: "2026-07-30".into(),
                filing_date: Some("2026-08-01".into()),
                name: "[stub: insider name]".into(),
                owner_type: Some("10 percent owner".into()),
                transaction_type: Some("S-Sale".into()),
                shares: Some(50_000.0),
                price: Some(round(spot * 0.94)),
            },
        ],
        insider_statistics: Some(InsiderStatistics {
            year: Some(2026),
            quarter: Some(3),
            acquired_transactions: Some(1),
            disposed_transactions: Some(9),
            total_acquired: Some(2_500.0),
            total_disposed: Some(310_000.0),
        }),
        congressional_trades: vec![
            CongressionalTrade {
                chamber: Chamber::House,
                transaction_date: "2026-08-14".into(),
                disclosure_date: Some("2026-09-02".into()),
                name: "[stub: member]".into(),
                owner: Some("Joint".into()),
                kind: Some("Purchase".into()),
                amount: Some("$1,001 - $15,000".into()),
            },
            CongressionalTrade {
                chamber: Chamber::Senate,
                transaction_date: "2026-06-03".into(),
                disclosure_date: Some("2026-07-01".into()),
                name: "[stub: member]".into(),
                owner: Some("Spouse".into()),
                kind: Some("Sale".into()),
                amount: Some("$15,001 - $50,000".into()),
            },
        ],
        float: Some(SharesFloat {
            date: Some("2026-09-01".into()),
            free_float_percent: Some(86.9),
            float_shares: Some(2.8e9),
            outstanding_shares: Some(3.22e9),
        }),
        product_segments: vec![
            segments(
                2025,
                "2025-12-31",
                &[
                    ("Automotive", 7.2e10),
                    ("Services and other", 1.1e10),
                    ("Energy generation and storage", 1.0e10),
                ],
            ),
            segments(
                2024,
                "2024-12-31",
                &[
                    ("Automotive", 7.7e10),
                    ("Services and other", 1.05e10),
                    ("Energy generation and storage", 1.0e10),
                ],
            ),
        ],
        geographic_segments: vec![
            segments(
                2025,
                "2025-12-31",
                &[
                    ("United States", 4.4e10),
                    ("Other", 2.8e10),
                    ("China", 2.1e10),
                ],
            ),
            segments(
                2024,
                "2024-12-31",
                &[
                    ("United States", 4.7e10),
                    ("Other", 2.9e10),
                    ("China", 2.15e10),
                ],
            ),
        ],
        splits: vec![
            SplitRow {
                date: "2022-08-25".into(),
                numerator: 3.0,
                denominator: 1.0,
            },
            SplitRow {
                date: "2020-08-31".into(),
                numerator: 5.0,
                denominator: 1.0,
            },
        ],
        earnings: vec![
            earnings("2026-10-21", None, 0.55, None),
            earnings("2026-07-22", Some(0.40), 0.43, Some(2.25e10)),
            earnings("2026-04-22", Some(0.27), 0.42, Some(1.93e10)),
            earnings("2026-01-29", Some(0.73), 0.77, Some(2.57e10)),
            earnings("2025-10-22", Some(0.72), 0.60, Some(2.52e10)),
        ],
        gaps: vec![],
    });
    d.ma_matches = vec![MaMatch {
        role: MaRole::Acquirer,
        counterparty: "[stub: counterparty]".into(),
        date: "2026-05-20".into(),
        link: None,
    }];
}

fn graded_of(f: &Fixture) -> &crate::portfolio::GradedVerdict {
    match &f.disposition {
        VerdictDisposition::Priced(g) => g,
        other => panic!("{}: every fixed-set holding is priced: {other:?}", f.symbol),
    }
}

/// The priced continuity shape's dossier: the fixture's persisted verdict as
/// the prior (its thesis document and appendix as re-shaped by hand), the
/// hand-written prior spot and anchor bar, and the prior's engine stamps so
/// the pipeline reads it as a same-vintage prior.
pub(super) fn continuity_dossier(f: &Fixture) -> HoldingDossier {
    let mut d = debut_dossier(f);
    let prior_spot = (f.spot * 0.97 * 100.0).round() / 100.0;
    let anchor = engine::DatedValue { date: PRIOR_SESSION.into(), value: prior_spot };
    d.financials.daily_closes.insert(0, anchor.clone());
    d.prior_verdict = Some(HoldingVerdict {
        symbol: f.symbol.clone(),
        asset_class: f.asset_class,
        position_change: PositionChange::New,
        basis_move: None,
        disposition: f.disposition.clone(),
        analyzed_at: Some(PRIOR_VINTAGE.into()),
        action_source: ActionSource::ModelChosen,
        side_reversed: false,
    });
    d.prior_vintage = Some(PRIOR_VINTAGE.into());
    d.prior_spot = Some(prior_spot);
    // The prior analysis was written by the same run as the prior document,
    // on the same anchor bar.
    d.prior_analysis = Some(crate::portfolio::AnalysisRecord {
        text: STUB_PRIOR_ANALYSIS.into(),
        written: PRIOR_SESSION.into(),
        anchor: Some(anchor.clone()),
    });
    d.prior_authoring_close = Some(anchor);
    // The episode store read this run, empty: no check has landed on the
    // fixture (the stock review example layers synthetic ones on).
    d.accuracy = Some(crate::portfolio::outcome::SubjectAccuracy::default());
    d.prior_metrics = Some(f.engine_output.metrics.clone());
    d.prior_grade_parameter_version = Some(engine::GRADE_PARAMETER_VERSION.into());
    d.prior_target_parameter_version = Some(engine::SCENARIO_TARGET_PARAMETER_VERSION.into());
    d.position_delta = PositionDelta {
        change: PositionChange::Unchanged,
        prior_quantity: Some(f.synthetic_position.quantity),
        prior_cost_basis: Some(f.synthetic_position.cost_basis),
    };
    d
}

/// The thesis-document input over a dossier, the research stubbed.
fn thesis_input<'a>(f: &'a Fixture, d: &'a HoldingDossier) -> ThesisInput<'a> {
    ThesisInput {
        dossier: d,
        engine: &f.engine_output,
        rates: rates(),
        analysis: analysis_of(d, STUB_ANALYSIS_STOCK),
        pre_profit: None,
        soft_forensic: None,
        tech_pre_flag: None,
        narrative: None,
        prior_split: None,
        review: None,
    }
}

/// A stub that keeps the role/risk request the pipeline builds on a second
/// run — the continuity variant only `analyze_holding` can assemble.
#[derive(Default)]
struct RequestCapture {
    role_risk: RefCell<Option<ChatRequest>>,
    review: RefCell<Option<ChatRequest>>,
}

impl HoldingAnalyst for RequestCapture {
    fn review(&self, input: &crate::portfolio::review::ReviewInput) -> anyhow::Result<String> {
        *self.review.borrow_mut() = Some(pipeline::review_request(REASONER, input));
        Ok(crate::portfolio::review::stub_review(input))
    }
    fn interpret(&self, input: &ThesisInput) -> anyhow::Result<PricedModelArm> {
        pipeline::StubAnalyst.interpret(input)
    }
    fn interpret_role_risk(&self, input: &RoleRiskInput) -> anyhow::Result<String> {
        *self.role_risk.borrow_mut() = Some(pipeline::role_risk_request(REASONER, input));
        pipeline::StubAnalyst.interpret_role_risk(input)
    }
    fn decide_action(&self, input: &ActionInput) -> anyhow::Result<crate::portfolio::ActionDecision> {
        pipeline::StubAnalyst.decide_action(input)
    }
    fn fast_id(&self) -> String {
        "fast".into()
    }
    fn reasoner_id(&self) -> String {
        REASONER.into()
    }
}

/// The role/risk second run: a stub first run on 2026-09-03, its row's anchor
/// bar and fund basis loaded as the prior, the requests captured on
/// 2026-09-17.
fn role_risk_second_run(fx: &SyntheticRoleRisk) -> RequestCapture {
    let rates = pipeline::tests::rates();
    let (mut first, first_audit) =
        pipeline::analyze_holding(&pipeline::StubAnalyst, &fx.dossier, &rates, "2026-09-03")
            .expect("the stub's first run");
    // The prior rationale stands stubbed, as a model's would.
    if let crate::portfolio::VerdictDisposition::RoleRiskOnly(rr) = &mut first.disposition {
        rr.action_rationale = "[stub: the prior action's rationale, as the action call returned it]".into();
    }
    let mut second = fx.dossier.clone();
    second.prior_verdict = Some(first);
    second.prior_vintage = Some("2026-09-03T14:00:00Z".into());
    second.prior_authoring_close = first_audit.authoring_close.clone();
    second.prior_fund_exposure = first_audit.fund_exposure.clone();
    second.position_delta = PositionDelta {
        change: PositionChange::Unchanged,
        prior_quantity: Some(10.0),
        prior_cost_basis: Some(730.0),
    };
    let capture = RequestCapture::default();
    let _ = pipeline::analyze_holding(&capture, &second, &rates, "2026-09-17").expect("the second run");
    capture
}

/// A continuity dossier for the review examples: the priced continuity shape
/// with a close on the session before the prior analysis, so the move since
/// renders from it as a run's would.
fn review_dossier(f: &Fixture) -> HoldingDossier {
    let mut d = continuity_dossier(f);
    let prior_spot = d.prior_spot.expect("the continuity shape's prior spot");
    d.financials
        .daily_closes
        .insert(0, engine::DatedValue { date: "2026-09-01".into(), value: prior_spot });
    d
}

/// The synthetic accuracy record the stock review example renders (ruled
/// 2026-10-09): four earlier forecasts on the holding, two checks the prior
/// analysis read (ids 4 and 7) and two this run wrote after it (9, scored on
/// the analyst's price alone, and 10, a horizon with no forecast), so the
/// three-month scores read both ways and the lines show both outcomes. The
/// prices and closes are hand-written; the checks and scores are the
/// accuracy pass's own arithmetic.
fn layer_synthetic_accuracy(d: &mut HoldingDossier) {
    use crate::portfolio::outcome::{
        check_horizon, Check, HorizonPrices, Horizon, PriceRecord, StoredCheck, StoredEpisode,
    };
    let close = |date: &str, value: f64| engine::DatedValue { date: date.into(), value };
    let episode = |id: i64, created_on: &str, anchor: engine::DatedValue, model: HorizonPrices, engine: HorizonPrices| {
        StoredEpisode {
            id,
            record: PriceRecord {
                symbol: "TSLA".into(),
                created_on: created_on.into(),
                spot: anchor.value,
                anchor: Some(anchor),
                model,
                engine,
            },
        }
    };
    let three = |v: Option<f64>| HorizonPrices { three_month: v, ..Default::default() };
    let episodes = vec![
        episode(1, "2026-03-02", close("2026-02-27", 340.0), three(Some(300.0)), three(Some(310.0))),
        episode(2, "2026-05-01", close("2026-04-30", 330.0), three(Some(360.0)), three(Some(320.0))),
        episode(3, "2026-06-08", close("2026-06-05", 335.0), three(Some(380.0)), three(None)),
        episode(
            4,
            "2026-06-10",
            close("2026-06-09", 336.0),
            HorizonPrices { twelve_month: Some(400.0), ..Default::default() },
            three(None),
        ),
    ];
    // Each check reads the series a run would have fetched: the anchor bar
    // and the horizon's close.
    let series = |ep: &StoredEpisode, horizon_close: engine::DatedValue| {
        vec![ep.record.anchor.clone().expect("anchored"), horizon_close]
    };
    let check = |id: i64, ep: &StoredEpisode, on: &str, horizon_close: engine::DatedValue| StoredCheck {
        id,
        episode_id: ep.id,
        check: Check {
            horizon: Horizon::ThreeMonth,
            checked_on: on.into(),
            run_id: "synthetic".into(),
            outcome: check_horizon(&ep.record, Horizon::ThreeMonth, &series(ep, horizon_close)),
        },
    };
    let checks = vec![
        check(4, &episodes[0], "2026-06-05", close("2026-06-02", 330.0)),
        check(7, &episodes[1], "2026-08-04", close("2026-07-31", 345.0)),
        check(9, &episodes[2], "2026-09-16", close("2026-09-08", 352.4)),
        check(10, &episodes[3], "2026-09-16", close("2026-09-10", 350.0)),
    ];
    d.accuracy = Some(crate::portfolio::outcome::subject_accuracy(&episodes, &checks, "TSLA"));
    d.prior_accuracy_read_through = Some(7);
}

/// The review input over a priced continuity dossier, the analysis stubbed.
fn review_input<'a>(
    f: &'a Fixture,
    d: &'a HoldingDossier,
    analysis: &'a crate::portfolio::AnalysisRecord,
) -> crate::portfolio::review::ReviewInput<'a> {
    crate::portfolio::review::ReviewInput {
        dossier: d,
        rates: rates(),
        analysis,
        prior_split: None,
        price_bridge: Some(1.0),
        subject: crate::portfolio::review::ReviewSubject::Priced { engine: &f.engine_output },
    }
}

fn research_request(s: &research_samples::Sample) -> ChatRequest {
    let mut messages = vec![ChatMessage::system(s.system.clone()), ChatMessage::user(s.user.clone())];
    messages.extend(s.appended.iter().cloned());
    pipeline::research_turn_request(REASONER, messages, s.tools.as_ref(), s.format.as_ref())
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
    // The holding-constant brief as the pipeline assembles it, the leads
    // replaced by the hand-written sample leads (stubbed).
    let mut brief = pipeline::research_brief(&tsla_debut, rates(), None);
    brief.leads = research_samples::stock_leads(true);
    let mut continuity = pipeline::research_brief(&continuity_dossier(&tsla), rates(), None);
    continuity.leads = research_samples::stock_leads(true);
    let mut fund_brief = pipeline::research_brief(&fx.dossier, rates(), None);
    fund_brief.leads = research_samples::fund_leads(true);
    let agenda = research::build_agenda(&tsla_debut, &research::AgendaTriggers::default());
    let fund_agenda = research::build_agenda(&fx.dossier, &research::AgendaTriggers::default());
    let exposure = fund_agenda
        .iter()
        .find(|t| t.key == "fund-exposure-profile")
        .expect("the fund exposure topic");
    let stock_gathering = research_samples::gathering_messages("TSLA", &brief, &continuity, &agenda[0], true);
    let stock_synthesis = research_samples::synthesis_messages("TSLA", &brief, &agenda[0], true);
    let followup_ask = research_samples::followup_ask_sample("TSLA", &brief, &agenda[0], true);
    let fund_gathering = research_samples::gathering_messages("BND", &fund_brief, &fund_brief, exposure, true);
    let tool_results = research_samples::tool_results(true);
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
            "Part 1 leads with the holding-constant block — the header, FETCHED VALUES as the thesis-document message renders it, the two news leads — then the topic; Part 2 the search task and its stopping rule.",
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
            "The follow-up pass on the same topic, taken when the root pass's synthesis answered the follow-up ask with a question.",
            "The question and the topic's write-up so far ride in Part 1 after the holding-constant block, so the search starts from what the root pass established and the pass rewrites the write-up whole.",
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
        holding: "TSLA, on a continuity run over a prior analysis of 2026-09-02",
        sentences: lines(&[
            "A root pass on a continuity run: the prior run's analysis and thesis document ride the holding-constant block verbatim under their dates — PRIOR ANALYSIS, the holding's research memory, then PRIOR THESIS — so research knows what was established and what the falsifiers and triggers are, and tests them.",
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
            "The disconfirming pass, run once per holding after its topics: the run's write-ups so far are the target and the task is to find what contradicts them.",
            "Its synthesis (file 10) writes its own write-up and asks no follow-up.",
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
            "The root-pass shape on a fund: the agenda's fund topics replace the stock topics, and the header and FETCHED VALUES name the fund's reported lines.",
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
        "The synthesis conversation closes a pass: no tools, no grammar, no history — the evidence packet is rebuilt from the run's store behind the holding header and FETCHED VALUES, and the reply is the pass's write-up as prose, read as text and validated by nothing.";
    out.push(Example {
        file: "08-research-synthesis-root-pass-later-topic",
        title: "Research synthesis — root pass on a later topic",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            synthesis_common,
            "The packet lists the reused page first and this pass's fetch after it (portfolio-v49); what gathering lost is a persisted data-health gap and reaches no model (portfolio-v59).",
            "On a root pass the conversation continues with the follow-up ask (file 11).",
        ]),
        stage: s(0).stage.clone(),
        request: research_request(s(0)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "09-research-synthesis-follow-up-pass",
        title: "Research synthesis — follow-up pass",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            synthesis_common,
            "On a follow-up pass the question joins the packet under FOLLOW-UP with the topic's write-up so far under WRITE-UP SO FAR, and the task is to rewrite that write-up whole with the new evidence folded in, so a topic has one write-up at any time.",
            "The conversation asks for a follow-up afterwards unless the pass is the topic's last under the depth cap, whose question no pass could take up (portfolio-v60).",
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
            "The disconfirming pass's write-up states how the evidence bears on the run's write-ups under WRITE-UPS SO FAR; the conversation asks no follow-up, and the write-up joins the holding's research as the contrary-evidence pass.",
        ]),
        stage: s(2).stage.clone(),
        request: research_request(s(2)),
        variants: vec![],
        extras: vec![],
    });
    out.push(Example {
        file: "11-research-synthesis-follow-up-ask",
        title: "Research synthesis — the follow-up ask, the conversation's second message",
        step: "6c",
        holding: tsla_holding,
        sentences: lines(&[
            "The synthesis conversation's second message on a pass that offers a follow-up: the first message, the write-up the model returned echoed as the assistant's turn, then the ask.",
            "The reply is the follow-up question as plain text, which becomes the next pass's question under the depth cap and the budget, or the one word none — the only reply the app interprets.",
            "The message is not sent on a topic's last pass under the depth cap nor on the disconfirming pass.",
        ]),
        stage: followup_ask.stage.clone(),
        request: research_request(&followup_ask),
        variants: vec![],
        extras: vec![],
    });

    // ---- Step 6d: consolidation ----
    let stock_write_ups: Vec<WriteUp> = research_samples::write_ups_so_far(true)
        .into_iter()
        .zip(["competitive-position", "results-revisions"])
        .map(|((title, text), key)| WriteUp { key: key.into(), title, text })
        .chain(std::iter::once(WriteUp {
            key: distill::CONTRARY_KEY.into(),
            title: distill::CONTRARY_TITLE.into(),
            text: research_samples::prose(
                true,
                "the disconfirming pass's write-up — how the contrary evidence bears on the run's write-ups",
                "",
            ),
        }))
        .collect();
    let stock_distillates: Vec<WriteUp> = stock_write_ups
        .iter()
        .map(|w| WriteUp {
            key: w.key.clone(),
            title: w.title.clone(),
            text: research_samples::prose(true, "the write-up shortened by its own distillation call", ""),
        })
        .collect();
    let fund_write_ups = vec![WriteUp {
        key: exposure.key.clone(),
        title: exposure.title.clone(),
        text: research_samples::prose(true, "the fund's exposure-profile write-up", ""),
    }];
    let distill_req = |input: &DistillInput<'_>| {
        pipeline::distill_request(
            REASONER,
            pipeline::distill_num_ctx(REASONER, REASONER),
            pipeline::NUM_PREDICT_DISTILL,
            &distill::distillation_prompt(input),
        )
    };
    let distill_common = "A distillation is a non-thinking call with no grammar on the resident reasoner (the fast tier where the roster has one), issued only where the analysis prompt is over budget; the shape is the orchestrator's choice from size, never the model's, and the write-ups persist on the audit as written.";
    for (file, title, subject, stage, own) in [
        (
            "12-distillation-merged-write-ups",
            "Distillation — the merged write-ups",
            DistillSubject::Merged(&stock_write_ups),
            "distill TSLA".to_string(),
            "The merged shape: every write-up of the run under its topic, the contrary-evidence pass last, shortened in one call; taken where the analysis prompt is over budget and this prompt fits the widest issuable budget.",
        ),
        (
            "13-distillation-one-write-up",
            "Distillation — one write-up of the per-write-up shape",
            DistillSubject::Single(&stock_write_ups[0]),
            "distill TSLA competitive-position".to_string(),
            "The per-write-up shape's first calls, one per write-up, taken where the merged prompt outgrows the widest issuable budget; the stage label names the topic.",
        ),
        (
            "14-distillation-merge-of-outputs",
            "Distillation — the merge of the per-write-up outputs",
            DistillSubject::MergedDistillates(&stock_distillates),
            "distill TSLA merge".to_string(),
            "The per-write-up shape's last call, over the shortened write-ups under their topics; it runs whenever that shape is taken, so the shape spends the write-ups plus one call.",
        ),
    ] {
        let input = DistillInput { header: &brief.header, subject, stage: stage.clone() };
        out.push(Example {
            file,
            title,
            step: "6d",
            holding: tsla_holding,
            sentences: lines(&[distill_common, own]),
            stage,
            request: distill_req(&input),
            variants: vec![],
            extras: vec![],
        });
    }
    let analysis_common = "The analysis call is a thinking call with no grammar: Part 1 the holding header and FETCHED VALUES as the brief carries them, on a continuity run PRIOR ANALYSIS, then WRITE-UPS; Part 2 what the analysis consolidates and its length band. The analysis is the only research artifact the next run reads.";
    {
        let input = AnalysisInput {
            symbol: "TSLA",
            brief: &brief,
            prior_analysis: "",
            write_ups: WriteUps::AsWritten(&stock_write_ups),
        };
        out.push(Example {
            file: "15-analysis-stock-first-analysis",
            title: "Analysis — stock, first analysis",
            step: "6d",
            holding: tsla_holding,
            sentences: lines(&[
                analysis_common,
                "On a first analysis there is no PRIOR ANALYSIS and no continuity clause; the write-ups go in as written, since the prompt fit its budget.",
            ]),
            stage: "analysis TSLA".into(),
            request: pipeline::analysis_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
        let prior_analysis = pipeline::prior_analysis_section(&continuity_dossier(&tsla));
        let input = AnalysisInput {
            symbol: "TSLA",
            brief: &continuity,
            prior_analysis: &prior_analysis,
            write_ups: WriteUps::AsWritten(&stock_write_ups),
        };
        out.push(Example {
            file: "16-analysis-stock-continuity",
            title: "Analysis — stock, continuity run",
            step: "6d",
            holding: "TSLA, on a continuity run over the prior analysis of 2026-09-02",
            sentences: lines(&[
                analysis_common,
                "On a continuity run the prior analysis renders verbatim as PRIOR ANALYSIS under its date, never distilled, and the task asks what it said that this run confirms, revises or leaves untouched; a topic with no write-up this run keeps what it says.",
            ]),
            stage: "analysis TSLA".into(),
            request: pipeline::analysis_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
        let input = AnalysisInput {
            symbol: "BND",
            brief: &fund_brief,
            prior_analysis: "",
            write_ups: WriteUps::AsWritten(&fund_write_ups),
        };
        out.push(Example {
            file: "17-analysis-fund",
            title: "Analysis — fund, first analysis",
            step: "6d",
            holding: bnd_holding,
            sentences: lines(&[
                analysis_common,
                "A fund takes the same call over its agenda's write-ups — here the exposure-profile topic's — with the fund's FETCHED VALUES.",
            ]),
            stage: "analysis BND".into(),
            request: pipeline::analysis_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
    }

    // ---- Step 6e: the self-review ----
    let review_common = "The review is a thinking call with no grammar, on a continuity run only: Part 1 the holding header and FETCHED VALUES, PRIOR POSITION, PRIOR THESIS, this run's ANALYSIS and REALIZED; Part 2 what the review covers, in order, and its length band. The review reaches this run's thesis document alone.";
    {
        let mut d = review_dossier(&tsla);
        layer_synthetic_accuracy(&mut d);
        let analysis = analysis_of(&d, STUB_ANALYSIS_STOCK);
        out.push(Example {
            file: "18-review-stock",
            title: "Self-review — stock, continuity run",
            step: "6e",
            holding: "TSLA, on a continuity run over the prior position of 2026-09-02",
            sentences: lines(&[
                review_common,
                "REALIZED carries the price now with its move and the path since, each prior expected price at its horizon date, the accuracy scores with whether the prior analysis read each, the checks written since the prior analysis, and the computed reads then and now.",
                "The accuracy record here is synthetic, layered for this example alone: four earlier forecasts, two checks the prior analysis read and two this run wrote after it — one scored, one with no forecast at its horizon — so the three-month scores read both ways and the lines show both outcomes.",
                "The prior here is attempt 6's persisted verdict, re-dated to 2026-09-02, with a hand-written prior spot 3% under today's; its twelve-month price has not reached its date.",
            ]),
            stage: "review TSLA".into(),
            request: pipeline::review_request(REASONER, &review_input(&tsla, &d, &analysis)),
            variants: vec![],
            extras: vec![],
        });
        let mut d = review_dossier(&spmo);
        // The prior read the store (other holdings' checks, through id 12);
        // none has landed on this fund since.
        d.prior_accuracy_read_through = Some(12);
        let analysis = analysis_of(&d, STUB_ANALYSIS_STOCK);
        out.push(Example {
            file: "19-review-fund",
            title: "Self-review — priced fund, continuity run",
            step: "6e",
            holding: "SPMO, on a continuity run over the prior position of 2026-09-02; the fixture carries no fund context",
            sentences: lines(&[
                review_common,
                "A priced fund takes the same call; with no check landed since the prior analysis, the fixed sentence says the scores shown are the ones it read. The prior's read-through mark here is hand-written.",
            ]),
            stage: "review SPMO".into(),
            request: pipeline::review_request(REASONER, &review_input(&spmo, &d, &analysis)),
            variants: vec![],
            extras: vec![],
        });
        out.push(Example {
            file: "20-review-role-risk",
            title: "Self-review — role/risk, continuity run",
            step: "6e",
            holding: "BND, on a continuity run over a stub first run of 2026-09-03",
            sentences: lines(&[
                review_common,
                "A role read states no price, so PRIOR POSITION carries the prior action and its rationale alone, and REALIZED the price, the NAV and the fund's reads then and now with no accuracy record; the stub first run's row supplies the then side.",
            ]),
            stage: "review BND".into(),
            request: role_risk_second_run(&fx).review.take().expect("the review request was captured"),
            variants: vec![],
            extras: vec![],
        });
    }

    // ---- Step 6f: the thesis document and its appendix ----
    let thesis_common =
        "The thesis document is a thinking call with no grammar: Part 1 the fetched values, the computed reads under one heading, the market analysis and this run's analysis; Part 2 what the document covers, in order, and its length band.";
    let continuity_note = "The prior here is attempt 6's persisted verdict, its model arm re-shaped by hand into a thesis document and an appendix, re-dated to 2026-09-02, with a hand-written prior spot 3% under today's and its anchor bar; attempt 6 wrote no second run.";
    for (f, file, title, holding, own) in [
        (
            &tsla,
            "21-thesis-document-stock-first-analysis",
            "Thesis document — stock, first analysis",
            tsla_holding,
            "On a first analysis there is no PRIOR THESIS, and the summary item asks for no continuity clause.",
        ),
        (
            &spmo,
            "23-thesis-document-fund-first-analysis",
            "Thesis document — priced fund, first analysis",
            "SPMO, an ETF of the fixed evidence set (attempt 6, reconstructed), on its first analysis; the fixture carries no fund context, so the FUND block does not render",
            "A priced fund takes the same call with the fund's metric labels and an investment analyst's role line.",
        ),
    ] {
        let d = debut_dossier(f);
        out.push(Example {
            file,
            title,
            step: "6f",
            holding,
            sentences: lines(&[thesis_common, own]),
            stage: format!("thesis {}", f.symbol),
            request: pipeline::thesis_request(REASONER, &thesis_input(f, &d)),
            variants: vec![],
            extras: vec![],
        });
    }
    for (f, file, title, holding, own) in [
        (
            &tsla,
            "22-thesis-document-stock-continuity",
            "Thesis document — stock, continuity run",
            "TSLA, on a continuity run over the prior document of 2026-09-02",
            "On a continuity run this run's review renders under REVIEW, then the prior document verbatim as PRIOR THESIS under its date, and the summary item asks what changed since it and how the prior read held up; the document is never rewritten.",
        ),
        (
            &spmo,
            "24-thesis-document-fund-continuity",
            "Thesis document — priced fund, continuity run",
            "SPMO, on a continuity run over the prior document of 2026-09-02; the fixture carries no fund context, so the FUND block does not render",
            "The fund's continuity shape carries the same REVIEW and PRIOR THESIS sections and continuity clause as the stock's.",
        ),
    ] {
        let d = continuity_dossier(f);
        let mut input = thesis_input(f, &d);
        input.review = Some(STUB_REVIEW.into());
        out.push(Example {
            file,
            title,
            step: "6f",
            holding,
            sentences: lines(&[thesis_common, own, continuity_note]),
            stage: format!("thesis {}", f.symbol),
            request: pipeline::thesis_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
    }
    {
        let d = debut_dossier(&tsla);
        let input = thesis_input(&tsla, &d);
        let document = pipeline::StubAnalyst.interpret(&input).expect("the stub writes the document").thesis_document;
        out.push(Example {
            file: "25-thesis-appendix",
            title: "Thesis appendix — the conversation's second message",
            step: "6f",
            holding: tsla_holding,
            sentences: lines(&[
                "The appendix is the thesis conversation's second message: the same system and user messages, the document the model returned as the assistant turn, then the transcription ask under the nullable grammar with thinking off.",
                "The grammar admits a null in every field; an object off its declared domain — a conviction outside high, medium and low, a price not finite and positive — is rejected whole and the identical message re-issued once.",
                "The assistant turn shown is the offline stub's document on the TSLA debut packet; a run's is the model's own.",
            ]),
            stage: "appendix TSLA".into(),
            request: pipeline::appendix_request(REASONER, &input, &document),
            variants: vec![],
            extras: vec![],
        });
    }
    {
        let input = RoleRiskInput {
            dossier: &fx.dossier,
            readout: &fx.readout,
            rates: rates(),
            analysis: analysis_of(&fx.dossier, STUB_ANALYSIS_FUND),
            prior_split: None,
            review: None,
        };
        out.push(Example {
            file: "26-role-risk-thesis-document-first-analysis",
            title: "Role/risk thesis document — first analysis",
            step: "6f",
            holding: bnd_holding,
            sentences: lines(&[
                "The role/risk branch of the intrinsic verdict, taken for a vehicle class the engine cannot price: the fund readout stands where the computed scores would, and the document states no expected price and no conviction, so the conversation has no appendix message.",
                "The hand-written Treasury positioning line and the venue put/call backdrop render because the synthetic dossier carries them.",
            ]),
            stage: "thesis BND".into(),
            request: pipeline::role_risk_request(REASONER, &input),
            variants: vec![],
            extras: vec![],
        });
        out.push(Example {
            file: "27-role-risk-thesis-document-continuity",
            title: "Role/risk thesis document — continuity run",
            step: "6f",
            holding: "BND, on a continuity run over a stub first run of 2026-09-03",
            sentences: lines(&[
                "The role/risk call on a continuity run, as the pipeline itself renders it on a second run: the review under REVIEW, the prior document verbatim as PRIOR THESIS under its date, and the summary item's continuity clause.",
                "The analysis is the one no-write-up sentence: the stub run issues no research call and writes nothing, so consolidation spends no analysis call.",
            ]),
            stage: "thesis BND".into(),
            request: role_risk_second_run(&fx).role_risk.take().expect("the role/risk request was captured"),
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
        let make = |d: &HoldingDossier| -> String {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded, engine: &tsla.engine_output, pre_profit: None },
                engine_set: &engine_set,
                profile: &d.profile,
            })
        };
        let base = pipeline::action_request(
            REASONER,
            &ActionInput {
                dossier: &tsla_debut,
                subject: ActionSubject::Priced { graded, engine: &tsla.engine_output, pre_profit: None },
                engine_set: &engine_set,
                profile: &tsla_debut.profile,
            },
        );
        let base_user = base.messages[1].content.clone();
        let mut exempt = dossier_of(&tsla, false);
        stub_house_view(&mut exempt);
        let mut costly = debut_dossier(&tsla);
        costly.position.cost_basis *= 3.0;
        out.push(Example {
            file: "28-action-priced-first-analysis",
            title: "Action — priced holding, first analysis",
            step: "6f",
            holding: tsla_holding,
            sentences: lines(&[
                action_common,
                "POSITION is the one packet that sees the position's economics — the synthetic ten units, their cost basis and market value, the unrealized gain and the change since the last pull, new on a first analysis.",
                "VERDICT carries the appendix's conviction and expected prices, each with the move it implies from the current price, then the thesis document verbatim; COMPUTED follows as one heading whose sub-blocks carry the rule's rung, the grade, the bands with the analyst's price beside each, and the capital-efficiency numbers.",
                "The two variants below show that a tax-exempt profile changes no line and a tripled cost basis changes POSITION's lines alone.",
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
                    sentences: lines(&["The lines that differ from the user message above when the position's cost basis is three times higher: the gain becomes a loss, under POSITION alone."]),
                    diff: diff_lines(&base_user, &make(&costly)),
                },
            ],
            extras: vec![],
        });
    }
    {
        let d = continuity_dossier(&tsla);
        let graded = graded_of(&tsla);
        let engine_set = engine::feasible_actions(tsla.engine_output.grade, &tsla.engine_output.hurdle, None, false);
        let make = |d: &HoldingDossier| -> String {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded, engine: &tsla.engine_output, pre_profit: None },
                engine_set: &engine_set,
                profile: &d.profile,
            })
        };
        let base = pipeline::action_request(
            REASONER,
            &ActionInput {
                dossier: &d,
                subject: ActionSubject::Priced { graded, engine: &tsla.engine_output, pre_profit: None },
                engine_set: &engine_set,
                profile: &d.profile,
            },
        );
        let base_user = base.messages[1].content.clone();
        // The position increased since the last pull: eight units then, the
        // synthetic ten now.
        let mut increased = continuity_dossier(&tsla);
        increased.position_delta = PositionDelta {
            change: PositionChange::Increased,
            prior_quantity: Some(8.0),
            prior_cost_basis: Some(tsla.synthetic_position.cost_basis * 0.8),
        };
        // A rule-demoted prior: the hold the over-age rule leaves on an
        // add-family prior, stamped set by rule, carrying no rationale.
        let mut demoted = continuity_dossier(&tsla);
        {
            let prior = demoted.prior_verdict.as_mut().expect("the continuity prior");
            prior.action_source = ActionSource::RuleDemoted;
            if let VerdictDisposition::Priced(g) = &mut prior.disposition {
                g.action = Action::Hold;
            }
        }
        out.push(Example {
            file: "29-action-priced-continuity",
            title: "Action — priced holding, continuity run",
            step: "6f",
            holding: "TSLA, on a continuity run over the prior document of 2026-09-02",
            sentences: lines(&[
                action_common,
                "On a continuity run the prior rung joins the packet as PRIOR ACTION with its rationale, glossed as chosen in the prior analysis or set by rule after it, and the task holds the action firm unless the inputs materially changed.",
                "The verdict shown is attempt 6's persisted read as the prior and as this run's — the fixture carries one verdict; PRIOR ACTION's rationale is the fixture's labelled placeholder, since attempt 6's sentence referenced the position's gain or loss.",
                "POSITION reads the position as unchanged at the synthetic ten units; the two variants below show the lines a position increased since the last pull and a rule-demoted prior change.",
            ]),
            stage: "action TSLA".into(),
            request: base,
            variants: vec![
                Variant {
                    heading: "Variant — position increased since the last pull",
                    sentences: lines(&["The lines that differ from the user message above when the last pull held eight units: the change line alone."]),
                    diff: diff_lines(&base_user, &make(&increased)),
                },
                Variant {
                    heading: "Variant — rule-demoted prior",
                    sentences: lines(&["The lines that differ from the user message above when the prior action was set by rule after the prior analysis: PRIOR ACTION carries the rung and its gloss with no rationale, and the task's firmness clause goes."]),
                    diff: diff_lines(&base_user, &make(&demoted)),
                },
            ],
            extras: vec![],
        });
    }
    {
        let verdict = synthetic_role_risk_verdict(&fx);
        let input = ActionInput {
            dossier: &fx.dossier,
            subject: ActionSubject::RoleRisk { verdict: &verdict },
            engine_set: &ROLE_RISK_ACTIONS,
            profile: &fx.dossier.profile,
        };
        out.push(Example {
            file: "30-action-role-risk",
            title: "Action — role/risk branch",
            step: "6f",
            holding: bnd_holding,
            sentences: lines(&[
                action_common,
                "On the role/risk branch the readout's computed sections stay top-level under no COMPUTED heading and the thesis document stands in for the graded verdict — VERDICT carries the document alone — and the engine set is the reduced ladder (sell-all, trim, hold).",
                "POSITION renders the synthetic fund's ten units the same way as a stock's.",
                "The document is the offline stub's on the synthetic fund, assembled into the verdict by the pipeline's own seam.",
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
                } else if m.content.starts_with("SEARCHING\n") {
                    out.push_str("## Appended user message (the turn countdown)\n\n");
                } else {
                    out.push_str(&format!("## Appended user message ({n} chars)\n\n"));
                }
            }
            "assistant" if m.tool_calls.is_none() => {
                out.push_str(&format!("## Assistant message (the model's reply, echoed back; {n} chars)\n\n"))
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
    if ex
        .request
        .messages
        .iter()
        .any(|m| m.content.contains("Street price targets:"))
    {
        out.push_str("Under FETCHED VALUES, the issuer line, the 52-week range, the 8-K list, the short-interest print, the street, insider and congressional rows, the surprises, the ratio lines, owner earnings, enterprise value, the float, the M&A match and the segments are synthetic values layered onto the fixture for these examples alone, their names stubbed; the statements, the quote and the closes are the fixed set's.\n");
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
    out.push_str("One file per call shape, in pipeline order: the research loop (Step 6c), consolidation — the distillation shapes and the analysis call (Step 6d) — the self-review on a continuity run (Step 6e), then the thesis document, its appendix and the action call (Step 6f).\n");
    out.push_str("Each file carries the request envelope, every message as sent, and the tools or the response schema.\n");
    out.push_str("On the stock shapes, the FETCHED VALUES rows the fixed set does not carry — the issuer line, the 52-week range, the 8-K list, the short-interest print, the street, insider and congressional rows, the surprises, the ratio lines, owner earnings, enterprise value, the float, the M&A match and the segments — are synthetic values layered on for the examples alone, their names stubbed (`docs/prompts/README.md`).\n\n");
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
    assert_eq!(examples.len(), 30);
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
